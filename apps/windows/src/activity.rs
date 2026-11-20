//! Activity の画面（GTK4 版の `activity.rs`、macOS 版の `Activity.swift` のタブのうち Operations と History）:
//! Operations は実行した git とコアの命令（`amonhen_model::activity` の記録。新しい順）、History は Undo できる
//! 操作（`undoLog`）と「ここまで戻す」。開いている間は 0.5 秒ごとに読み直す。GTK4 版・macOS 版は別の窓にするが、
//! ここではまだ History と右の列の場所に重ねる。History Cache のタブ（開いているリポジトリだけ）と JSON への書き出しもある。
//! Performance（読み込みの速さ）と Events（ファイルの変化の知らせ・読み直し）も、GTK4 版と同じく最近の 500 まで覚えて
//! 出す（裏のスレッドからも記録するので、ロックで守る）。Daemon は共有の daemon の様子（`amonhen daemon status` と
//! 同じ答え）と起動・停止、daemon が開いているリポジトリ（GTK4 版と同じ。出している間は 2 秒ごとに読み直す）。

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// 出す記録の数（新しいものから）。
const SHOWN: usize = 300;

/// 速さ・出来事を覚えておく数（GTK4 版と同じ）。
const KEEP: usize = 500;

/// 項目ごとの速さ（ms。古いものから）。
static PERFORMANCE: std::sync::Mutex<
    std::collections::BTreeMap<&'static str, std::collections::VecDeque<f64>>,
> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// 内部の出来事（UNIX 時刻の ms、リポジトリ、内容。古いものから）。
static EVENTS: std::sync::Mutex<std::collections::VecDeque<(u64, String, String)>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// 速さを 1 つ記録する（どのスレッドからでも。`metric` は訳す前の名前）。
pub fn record(metric: &'static str, ms: f64) {
    let Ok(mut map) = PERFORMANCE.lock() else {
        return;
    };
    let values = map.entry(metric).or_default();
    values.push_back(ms);
    if values.len() > KEEP {
        values.pop_front();
    }
}

/// 内部の出来事を 1 つ記録する（どのスレッドからでも）。
pub fn event(repo: &str, text: String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let Ok(mut events) = EVENTS.lock() else {
        return;
    };
    events.push_back((now, repo.to_owned(), text));
    if events.len() > KEEP {
        events.pop_front();
    }
}

/// Performance の 1 行（名前、数、p50、p95、最大、最後。ms）。
pub struct Speed {
    pub name: String,
    pub count: usize,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
    pub last: f64,
}

/// 項目ごとの速さのまとめ（名前の順）。
fn speeds() -> Vec<Speed> {
    let Ok(map) = PERFORMANCE.lock() else {
        return Vec::new();
    };
    map.iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(name, values)| {
            let mut sorted: Vec<f64> = values.iter().copied().collect();
            sorted.sort_by(f64::total_cmp);
            let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
            Speed {
                name: amonhen_model::i18n::translate(name).into_owned(),
                count: sorted.len(),
                p50: at(0.5),
                p95: at(0.95),
                max: sorted[sorted.len() - 1],
                last: values.back().copied().unwrap_or(0.0),
            }
        })
        .collect()
}

pub struct State {
    /// 0 は Operations、1 は History。
    pub tab: usize,
    pub records: Vec<amonhen_model::activity::Record>,
    /// `undoLog` の答えの entries（新しい順）。
    pub undo: Vec<Value>,
    /// 開いているリポジトリの名前と `historyCache` の答えの info（History のキャッシュの書き込み）。
    pub cache: Option<(String, Value)>,
    /// Performance の行と、Events（新しい順）。
    pub speeds: Vec<Speed>,
    pub events: Vec<(u64, String, String)>,
    /// 共有の daemon の様子（読む前は None、動いていなければ Some(None)）。
    pub daemon: Option<Option<Value>>,
    /// Daemon のタブを出している間の数え（4 回に 1 回、2 秒ごとに読み直す）。
    pub daemon_tick: u32,
    /// この窓のリポジトリの、daemon・SSH 越しのつながり（手元のリポジトリなら None。窓が読むたびに入れる）。
    pub connection: Option<Connection>,
}

/// この窓のリポジトリのつながり（GTK4 版の「このアプリのつながり」の 1 行）。
#[derive(Clone, PartialEq)]
pub struct Connection {
    pub name: String,
    pub location: String,
    pub closed: Option<String>,
    pub sent: u64,
    pub received: u64,
    pub engine: Option<String>,
}

impl State {
    pub fn new() -> Self {
        let mut state = Self {
            tab: 0,
            records: Vec::new(),
            undo: Vec::new(),
            cache: None,
            speeds: Vec::new(),
            events: Vec::new(),
            daemon: None,
            daemon_tick: 0,
            connection: None,
        };
        state.refresh();
        state
    }

    /// 記録を読み直す（軽い: ロック 1 回とコピー 1 回）。
    pub fn refresh(&mut self) {
        let mut records = amonhen_model::activity::records();
        records.reverse();
        records.truncate(SHOWN);
        self.records = records;
        self.speeds = speeds();
        self.events = EVENTS
            .lock()
            .map(|e| e.iter().rev().cloned().collect())
            .unwrap_or_default();
    }
}

pub struct Callbacks<'a> {
    pub tab: Callback<Option<usize>>,
    /// History の Undo・Undo to Here（戻す数）。
    pub undo: Box<dyn Fn(usize) -> Callback<()> + 'a>,
    pub close: Callback<()>,
    /// Operations の記録を JSON に書き出す。
    pub export: Callback<()>,
    /// 走っている操作を取り消す（記録の通し番号）。
    pub cancel: Box<dyn Fn(u64) -> Callback<()> + 'a>,
    /// 共有の daemon を起動する・止める。
    pub daemon_toggle: Callback<()>,
}

/// バイト数を読みやすく（GTK4 版の `glib::format_size` に近い形）。
fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64;
    let mut unit = "";
    for u in UNITS {
        value /= 1000.0;
        unit = u;
        if value < 1000.0 {
            break;
        }
    }
    format!("{value:.1} {unit}")
}

/// UNIX 時刻のミリ秒を、この機械の時刻の `HH:MM:SS.mmm` に。
fn time(ms: u64) -> String {
    let date = crate::render::local_date((ms / 1000) as i64);
    let clock = date.rsplit(' ').next().unwrap_or_default();
    let seconds = (ms / 1000) % 60;
    format!("{clock}:{seconds:02}.{:03}", ms % 1000)
}

fn duration(value: f64) -> String {
    if value < 10.0 {
        format!("{value:.2} ms")
    } else {
        format!("{value:.0} ms")
    }
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let head = Grid::new()
        .columns([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            TextBlock::new()
                .text("Activity")
                .font_size(14.0)
                .vertical_alignment(VerticalAlignment::Center)
                .margin(Thickness::new(0.0, 0.0, 16.0, 0.0))
                .grid_column(0),
            RadioButtons::new()
                .items_source(vec![
                    tr!("Operations"),
                    "History".to_owned(),
                    tr!("History Cache"),
                    tr!("Performance"),
                    tr!("Events"),
                    "Daemon".to_owned(),
                ])
                .selected_index(state.tab)
                .max_columns(6)
                .on_selection_changed(callbacks.tab)
                .grid_column(1),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .grid_column(2)
                .children((
                    if state.tab == 0 {
                        Button::new()
                            .on_click(callbacks.export)
                            .content(TextBlock::new().text(tr!("Export JSON…")))
                    } else {
                        View::empty()
                    },
                    Button::new()
                        .on_click(callbacks.close)
                        .content(TextBlock::new().text(tr!("Close"))),
                )),
        ));
    let body: View = if state.tab == 5 {
        daemon_view(
            state.daemon.as_ref(),
            state.connection.as_ref(),
            callbacks.daemon_toggle,
        )
    } else if state.tab == 0 {
        let rows = state.records.iter().map(|r| {
            let status = match r.ok {
                None => "…",
                Some(true) => "✓",
                Some(false) => "✗",
            };
            let line = Grid::new()
                .columns([
                    GridLength::Pixel(100.0),
                    GridLength::Pixel(24.0),
                    GridLength::Pixel(70.0),
                    GridLength::Star(1.0),
                    GridLength::Pixel(80.0),
                ])
                .children((
                    TextBlock::new()
                        .text(time(r.started))
                        .font_size(12.0)
                        .opacity(0.6)
                        .grid_column(0),
                    TextBlock::new().text(status).font_size(12.0).grid_column(1),
                    TextBlock::new()
                        .text(r.kind)
                        .font_size(12.0)
                        .opacity(0.6)
                        .grid_column(2),
                    // 内容（走っていれば進み具合）と、走っていて取り消せる操作には「取り消す」（GTK4 版と同じ）。
                    Grid::new()
                        .columns([GridLength::Star(1.0), GridLength::Auto])
                        .grid_column(3)
                        .children((
                            TextBlock::new()
                                .text(r.progress.clone().map_or_else(
                                    || r.label.clone(),
                                    |p| format!("{} — {p}", r.label),
                                ))
                                .font_size(12.0)
                                .text_trimming(TextTrimming::CharacterEllipsis)
                                .vertical_alignment(VerticalAlignment::Center)
                                .grid_column(0),
                            if r.ok.is_none() && r.cancellable {
                                Button::new()
                                    .on_click((callbacks.cancel)(r.seq))
                                    .margin(Thickness::new(6.0, 0.0, 0.0, 0.0))
                                    .grid_column(1)
                                    .content(
                                        TextBlock::new()
                                            .text(tr!("Cancel Operation"))
                                            .font_size(11.0),
                                    )
                            } else {
                                View::empty()
                            },
                        )),
                    TextBlock::new()
                        .text(r.duration.map(duration).unwrap_or_default())
                        .font_size(12.0)
                        .opacity(0.6)
                        .horizontal_alignment(HorizontalAlignment::Right)
                        .grid_column(4),
                ));
            // 失敗したものはその理由を、成功して出力（hooks など）があればそれを下に（選んでコピーできる）。
            let below = match (&r.error, &r.output) {
                (Some(e), _) => Some((e.clone(), Some(Color::rgb(0xc4, 0x2b, 0x1c)))),
                (None, Some(output)) if !output.trim().is_empty() => {
                    Some((output.trim_end().to_owned(), None))
                }
                _ => None,
            };
            let row: View = match below {
                Some((text, color)) => {
                    let mut text = TextBlock::new()
                        .text(text)
                        .font_size(11.0)
                        .text_wrapping(TextWrapping::Wrap)
                        .is_text_selection_enabled(true)
                        .margin(Thickness::new(124.0, 0.0, 0.0, 2.0));
                    text = match color {
                        Some(color) => text.foreground(Brush::Solid(color)),
                        None => text.opacity(0.65),
                    };
                    StackPanel::new().children((line, text))
                }
                None => line,
            };
            (
                r.seq,
                Border::new()
                    .padding(Thickness::new(0.0, 2.0, 0.0, 2.0))
                    .content(row),
            )
        });
        ScrollViewer::new()
            .grid_row(1)
            .margin(Thickness::new(12.0, 0.0, 12.0, 8.0))
            .content(StackPanel::new().children((View::keyed_fragment(rows),)))
    } else if state.tab == 3 {
        performance_view(&state.speeds)
    } else if state.tab == 4 {
        events_view(&state.events)
    } else if state.tab == 2 {
        // History のキャッシュ（GTK4 版の Activity の History Cache）: 大きさと、この起動での書き込み。
        let body: View = match &state.cache {
            None => TextBlock::new().text(tr!("Loading…")).opacity(0.6).into(),
            Some((name, info)) => {
                let writes = &info["writes"];
                let last = match writes["lastAt"].as_i64() {
                    Some(at) => format!(
                        "{} · {}",
                        size(writes["lastBytes"].as_u64().unwrap_or(0)),
                        time(at as u64)
                    ),
                    None => "—".to_owned(),
                };
                let cell = |text: String, column: i32| {
                    TextBlock::new()
                        .text(text)
                        .font_size(12.0)
                        .grid_column(column)
                };
                let heading = |text: String, column: i32| {
                    TextBlock::new()
                        .text(text)
                        .font_size(12.0)
                        .opacity(0.6)
                        .grid_column(column)
                };
                let columns = || {
                    [
                        GridLength::Star(1.0),
                        GridLength::Pixel(100.0),
                        GridLength::Pixel(70.0),
                        GridLength::Pixel(100.0),
                        GridLength::Pixel(200.0),
                    ]
                };
                StackPanel::new().spacing(4.0).children((
                    Grid::new().columns(columns()).children((
                        heading(tr!("Repository"), 0),
                        heading(tr!("Size"), 1),
                        heading(tr!("Writes"), 2),
                        heading(tr!("Written"), 3),
                        heading(tr!("Last Write"), 4),
                    )),
                    Grid::new().columns(columns()).children((
                        cell(name.clone(), 0),
                        cell(size(info["size"].as_u64().unwrap_or(0)), 1),
                        cell(writes["count"].as_u64().unwrap_or(0).to_string(), 2),
                        cell(size(writes["bytes"].as_u64().unwrap_or(0)), 3),
                        cell(last, 4),
                    )),
                ))
            }
        };
        StackPanel::new()
            .grid_row(1)
            .margin(Thickness::new(12.0, 0.0, 12.0, 8.0))
            .children((body,))
    } else {
        let rows = state.undo.iter().enumerate().map(|(i, e)| {
            let ms = e["time"].as_f64().unwrap_or(0.0) as u64;
            let (title, tip) = if i == 0 {
                ("Undo".to_owned(), tr!("Undo this operation."))
            } else {
                (
                    tr!("Undo to Here"),
                    tr!(
                        "Undo the {count} latest operations, down to and including this one.",
                        count = i + 1
                    ),
                )
            };
            let line = Grid::new()
                .columns([
                    GridLength::Pixel(100.0),
                    GridLength::Star(1.0),
                    GridLength::Auto,
                ])
                .children((
                    TextBlock::new()
                        .text(time(ms))
                        .font_size(12.0)
                        .opacity(0.6)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(0),
                    TextBlock::new()
                        .text(e["label"].as_str().unwrap_or_default())
                        .vertical_alignment(VerticalAlignment::Center)
                        .text_trimming(TextTrimming::CharacterEllipsis)
                        .grid_column(1),
                    Button::new()
                        .on_click((callbacks.undo)(i + 1))
                        .grid_column(2)
                        .content(TextBlock::new().text(title))
                        .tooltip(tip),
                ));
            (
                i,
                Border::new()
                    .padding(Thickness::new(0.0, 2.0, 0.0, 2.0))
                    .content(line),
            )
        });
        StackPanel::new()
            .grid_row(1)
            .margin(Thickness::new(12.0, 0.0, 12.0, 8.0))
            .children((
                TextBlock::new()
                    .text(tr!(
                        "Operations you can Undo (newest first). It stops where another tool has moved a ref."
                    ))
                    .font_size(12.0)
                    .opacity(0.6)
                    .text_wrapping(TextWrapping::Wrap)
                    .margin(Thickness::new(0.0, 0.0, 0.0, 6.0)),
                ScrollViewer::new()
                    .content(StackPanel::new().children((View::keyed_fragment(rows),))),
            ))
    };
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((head, body))
}

/// 時間（ミリ秒）を、秒・分・時間で（GTK4 版・macOS 版の Daemon のタブと同じ）。
fn span(ms: u64) -> String {
    match ms / 1000 {
        s if s < 60 => tr!("{s} s", s = s),
        s if s < 3600 && s % 60 == 0 => tr!("{m} min", m = s / 60),
        s if s < 3600 => tr!("{m} min {s} s", m = s / 60, s = s % 60),
        s => tr!("{h} h {m} min", h = s / 3600, m = s % 3600 / 60),
    }
}

/// Daemon のタブ（GTK4 版の Daemon）: 動いているか（pid・版・動いている時間・つながりの数とソケット）と起動・停止、
/// daemon が開いているリポジトリの表。
/// つながりの種類（GTK4 版の `location_kind`）。
fn location_kind(location: &str) -> String {
    if location.starts_with("ssh://") {
        tr!("SSH host (direct)")
    } else if location.starts_with("daemon:ssh://") {
        tr!("Shared daemon → SSH host")
    } else {
        tr!("Local shared daemon")
    }
}

/// 「このアプリの daemon へのつながり」（GTK4 版・macOS 版の表。Windows 版は窓ごとにプロセスが分かれ、開いて
/// いるのは前のタブのリポジトリだけなので、この窓のもの）。
fn connections_view(connection: Option<&Connection>) -> View {
    let columns = || {
        [
            GridLength::Star(1.0),
            GridLength::Pixel(170.0),
            GridLength::Pixel(100.0),
            GridLength::Pixel(150.0),
            GridLength::Pixel(220.0),
        ]
    };
    let header = Grid::new().columns(columns()).children((
        heading(tr!("Repository"), 0),
        heading(tr!("Location"), 1),
        heading(tr!("Status"), 2),
        heading(tr!("Sent · Received"), 3),
        heading(tr!("Engine on the SSH Host"), 4),
    ));
    let row: View = match connection {
        None => cell("—".to_owned(), 0).opacity(0.6).into(),
        Some(c) => {
            let (status, color) = match &c.closed {
                Some(_) => (tr!("Disconnected"), Color::rgb(0xd0, 0x3a, 0x3a)),
                None => (tr!("Connected"), Color::rgb(0x2e, 0x9e, 0x44)),
            };
            let status = cell(status, 2).foreground(Brush::Solid(color));
            let status: View = match &c.closed {
                Some(reason) => status.tooltip(reason.clone()),
                None => status.into(),
            };
            let engine = c.engine.clone().unwrap_or_else(|| "—".to_owned());
            Grid::new().columns(columns()).children((
                cell(c.name.clone(), 0).tooltip(c.location.clone()),
                cell(location_kind(&c.location), 1).opacity(0.7),
                status,
                cell(format!("{} · {}", size(c.sent), size(c.received)), 3),
                cell(engine.clone(), 4).opacity(0.7).tooltip(engine),
            ))
        }
    };
    StackPanel::new().spacing(4.0).children((
        TextBlock::new()
            .text(tr!("This App’s Connections to the Daemon"))
            .font_size(14.0)
            .margin(Thickness::new(0.0, 8.0, 0.0, 0.0)),
        header,
        row,
    ))
}

fn daemon_view(
    status: Option<&Option<Value>>,
    connection: Option<&Connection>,
    toggle: Callback<()>,
) -> View {
    let running = matches!(status, Some(Some(_)));
    let (mark, title, detail) = match status {
        None => ("○", tr!("Checking…"), not_running()),
        Some(None) => ("○", tr!("The shared daemon isn’t running"), not_running()),
        Some(Some(d)) => (
            "●",
            tr!("The shared daemon is running"),
            tr!(
                "pid {pid} · version {version} (build {build}) · up {uptime} · connections {connections}",
                pid = d["pid"],
                version = d["version"].as_str().unwrap_or_default(),
                build = d["build"].as_str().unwrap_or_default(),
                uptime = span(d["uptimeMs"].as_u64().unwrap_or(0)),
                connections = d["connections"],
            ),
        ),
    };
    let green = Brush::Solid(Color::rgb(0x2e, 0x9e, 0x44));
    let headline = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(6.0)
        .children((
            if running {
                TextBlock::new().text(mark).foreground(green)
            } else {
                TextBlock::new().text(mark).opacity(0.6)
            },
            if running {
                TextBlock::new()
                    .text(title)
                    .font_size(15.0)
                    .foreground(green)
            } else {
                TextBlock::new().text(title).font_size(15.0).opacity(0.6)
            },
        ));
    let socket = match status {
        Some(Some(d)) => TextBlock::new()
            .text(d["socket"].as_str().unwrap_or_default())
            .font_size(12.0)
            .opacity(0.6)
            .is_text_selection_enabled(true)
            .into(),
        _ => View::empty(),
    };
    let button = Button::new()
        .on_click(toggle)
        .vertical_alignment(VerticalAlignment::Top);
    let top = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .children((
            StackPanel::new().spacing(2.0).grid_column(0).children((
                headline,
                TextBlock::new()
                    .text(detail)
                    .font_size(12.0)
                    .opacity(if running { 1.0 } else { 0.6 })
                    .text_wrapping(TextWrapping::Wrap),
                socket,
            )),
            button
                .grid_column(1)
                .content(TextBlock::new().text(if running { tr!("Stop") } else { tr!("Start") })),
        ));
    let columns = || {
        [
            GridLength::Star(1.0),
            GridLength::Pixel(150.0),
            GridLength::Pixel(150.0),
            GridLength::Pixel(70.0),
            GridLength::Pixel(70.0),
            GridLength::Pixel(170.0),
        ]
    };
    let header = Grid::new().columns(columns()).children((
        heading(tr!("Repository"), 0),
        heading("SSH".to_owned(), 1),
        heading(tr!("Connections"), 2),
        heading("History".to_owned(), 3),
        heading(tr!("Watching"), 4),
        heading(tr!("Search Index"), 5),
    ));
    let repos: Vec<Value> = match status {
        Some(Some(d)) => d["repos"].as_array().cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let rows = repos.iter().map(|r| {
        let yes = |key: &str| {
            if r[key].as_bool() == Some(true) {
                tr!("Yes")
            } else {
                "—".to_owned()
            }
        };
        let clients = match r["clients"].as_u64().unwrap_or(0) {
            0 => tr!(
                "None (for {duration})",
                duration = span(r["idleMs"].as_u64().unwrap_or(0))
            ),
            n => n.to_string(),
        };
        let bytes = |key: &str| size(r[key].as_u64().unwrap_or(0));
        let ssh = if r["ssh"].as_bool() != Some(true) {
            "—".to_owned()
        } else if r["closed"].as_str().is_some() {
            tr!("Disconnected")
        } else {
            format!("{} · {}", bytes("sent"), bytes("received"))
        };
        let index = match r["indexRows"].as_u64() {
            Some(rows) => tr!(
                "{rows} rows · {size}",
                rows = rows,
                size = bytes("indexBytes")
            ),
            None => "—".to_owned(),
        };
        let path = r["path"].as_str().unwrap_or_default().to_owned();
        (
            path.clone(),
            Grid::new().columns(columns()).children((
                cell(path.clone(), 0)
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .tooltip(path),
                cell(ssh, 1),
                cell(clients, 2),
                cell(yes("history"), 3),
                cell(yes("watching"), 4),
                cell(index, 5),
            )),
        )
    });
    let idle = match status {
        Some(Some(d)) => d["idleExitMs"].as_u64().unwrap_or(600_000),
        _ => 600_000,
    };
    StackPanel::new()
        .grid_row(1)
        .spacing(8.0)
        .margin(Thickness::new(12.0, 0.0, 12.0, 8.0))
        .children((
            top,
            TextBlock::new()
                .text(tr!("Open Repositories"))
                .font_size(14.0)
                .margin(Thickness::new(0.0, 8.0, 0.0, 0.0)),
            header,
            ScrollViewer::new()
                .max_height(320.0)
                .content(StackPanel::new().children((View::keyed_fragment(rows),))),
            connections_view(connection),
            note(tr!(
                "The daemon exits after {duration} with no connections. From a terminal, run amonhen daemon status to see it.",
                duration = span(idle)
            )),
            // 止めたときのこと（GTK4 版では Stop のツールチップ）。
            if running {
                note(tr!(
                    "Repositories opened through the daemon get disconnected (Reconnect starts it again)."
                ))
            } else {
                View::empty()
            },
        ))
}

fn not_running() -> String {
    tr!(
        "It starts automatically when you open a repository through the daemon (right-click in the repository list, or Settings > Performance)."
    )
}

/// 見出しの文字（薄く）と、表の文字。
fn heading(text: String, column: i32) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(12.0)
        .opacity(0.6)
        .grid_column(column)
}

fn cell(text: String, column: i32) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(12.0)
        .text_trimming(TextTrimming::CharacterEllipsis)
        .grid_column(column)
}

/// 下に置く、表の説明。
fn note(text: String) -> View {
    TextBlock::new()
        .text(text)
        .font_size(11.0)
        .opacity(0.6)
        .text_wrapping(TextWrapping::Wrap)
        .margin(Thickness::new(0.0, 6.0, 0.0, 0.0))
        .into()
}

/// Performance: 項目ごとの数・p50・p95・最大・最後（GTK4 版と同じ。p95 が 1 フレームの 2 倍を超えたら橙）。
fn performance_view(speeds: &[Speed]) -> View {
    let ms = |value: f64| {
        if value < 10.0 {
            format!("{value:.2}")
        } else {
            format!("{value:.0}")
        }
    };
    let columns = || {
        [
            GridLength::Star(1.0),
            GridLength::Pixel(60.0),
            GridLength::Pixel(70.0),
            GridLength::Pixel(70.0),
            GridLength::Pixel(70.0),
            GridLength::Pixel(70.0),
        ]
    };
    let header = Grid::new().columns(columns()).children((
        heading(tr!("Item"), 0),
        heading(tr!("Count"), 1),
        heading("p50 ms".to_owned(), 2),
        heading("p95 ms".to_owned(), 3),
        heading(tr!("Max ms"), 4),
        heading(tr!("Last ms"), 5),
    ));
    let orange = Brush::Solid(crate::palette::warning_text());
    let rows = speeds.iter().map(|s| {
        let slow = s.p95 > 16.7;
        let p95 = cell(ms(s.p95), 3);
        let p95 = if slow { p95.foreground(orange) } else { p95 };
        (
            s.name.clone(),
            Grid::new().columns(columns()).children((
                cell(s.name.clone(), 0),
                cell(s.count.to_string(), 1),
                cell(ms(s.p50), 2),
                p95,
                cell(ms(s.max), 4),
                cell(ms(s.last), 5),
            )),
        )
    });
    Grid::new()
        .grid_row(1)
        .margin(Thickness::new(12.0, 0.0, 12.0, 8.0))
        .rows([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
        .children((
            Border::new().grid_row(0).content(header),
            ScrollViewer::new().grid_row(1).content(
                StackPanel::new()
                    .spacing(2.0)
                    .children((View::keyed_fragment(rows),)),
            ),
            Border::new().grid_row(2).content(note(tr!(
                "The last 500 of each since the app started. Orange when p95 exceeds twice a frame (16.7 ms)."
            ))),
        ))
}

/// Events: ファイルの変化の知らせと、それで読み直したもの（新しい順。GTK4 版と同じ）。
fn events_view(events: &[(u64, String, String)]) -> View {
    let columns = || {
        [
            GridLength::Pixel(100.0),
            GridLength::Pixel(140.0),
            GridLength::Star(1.0),
        ]
    };
    let header = Grid::new().columns(columns()).children((
        heading(tr!("Time"), 0),
        heading(tr!("Repository"), 1),
        heading(tr!("Content"), 2),
    ));
    let rows = events.iter().enumerate().map(|(i, (at, repo, text))| {
        (
            i,
            Grid::new().columns(columns()).children((
                cell(time(*at), 0).opacity(0.6),
                cell(repo.clone(), 1),
                cell(text.clone(), 2),
            )),
        )
    });
    Grid::new()
        .grid_row(1)
        .margin(Thickness::new(12.0, 0.0, 12.0, 8.0))
        .rows([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
        .children((
            Border::new().grid_row(0).content(header),
            ScrollViewer::new().grid_row(1).content(
                StackPanel::new()
                    .spacing(2.0)
                    .children((View::keyed_fragment(rows),)),
            ),
            Border::new().grid_row(2).content(note(tr!(
                "File change notices and the reloads they cause (newest first, up to 500)."
            ))),
        ))
}
