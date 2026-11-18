//! Activity の画面（GTK4 版の `activity.rs`、macOS 版の `Activity.swift` のタブのうち Operations と History）:
//! Operations は実行した git とコアの命令（`amonhen_model::activity` の記録。新しい順）、History は Undo できる
//! 操作（`undoLog`）と「ここまで戻す」。開いている間は 0.5 秒ごとに読み直す。GTK4 版・macOS 版は別の窓にするが、
//! ここではまだ History と右の列の場所に重ねる。History Cache のタブ（開いているリポジトリだけ）と JSON への書き出しもある。
//! Performance（読み込みの速さ）と Events（ファイルの変化の知らせ・読み直し）も、GTK4 版と同じく最近の 500 まで覚えて
//! 出す（裏のスレッドからも記録するので、ロックで守る）。Daemon のタブはまだない。

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
                ])
                .selected_index(state.tab)
                .max_columns(5)
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
    let body: View = if state.tab == 0 {
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
    let orange = Brush::Solid(Color::rgb(0xc4, 0x6a, 0x00));
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
