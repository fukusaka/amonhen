//! Activity の画面（GTK4 版の `activity.rs`、macOS 版の `Activity.swift` のタブのうち Operations と History）:
//! Operations は実行した git とコアの命令（`amonhen_model::activity` の記録。新しい順）、History は Undo できる
//! 操作（`undoLog`）と「ここまで戻す」。開いている間は 0.5 秒ごとに読み直す。GTK4 版・macOS 版は別の窓にするが、
//! ここではまだ History と右の列の場所に重ねる。History Cache のタブ（開いているリポジトリだけ）と JSON への書き出しもある。
//! Performance・Events・Daemon のタブはまだない。

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// 出す記録の数（新しいものから）。
const SHOWN: usize = 300;

pub struct State {
    /// 0 は Operations、1 は History。
    pub tab: usize,
    pub records: Vec<amonhen_model::activity::Record>,
    /// `undoLog` の答えの entries（新しい順）。
    pub undo: Vec<Value>,
    /// 開いているリポジトリの名前と `historyCache` の答えの info（History のキャッシュの書き込み）。
    pub cache: Option<(String, Value)>,
}

impl State {
    pub fn new() -> Self {
        let mut state = Self {
            tab: 0,
            records: Vec::new(),
            undo: Vec::new(),
            cache: None,
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
    }
}

pub struct Callbacks<'a> {
    pub tab: Callback<Option<usize>>,
    /// History の Undo・Undo to Here（戻す数）。
    pub undo: Box<dyn Fn(usize) -> Callback<()> + 'a>,
    pub close: Callback<()>,
    /// Operations の記録を JSON に書き出す。
    pub export: Callback<()>,
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
                ])
                .selected_index(state.tab)
                .max_columns(3)
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
                    TextBlock::new()
                        .text(
                            r.progress
                                .clone()
                                .map_or_else(|| r.label.clone(), |p| format!("{} — {p}", r.label)),
                        )
                        .font_size(12.0)
                        .text_trimming(TextTrimming::CharacterEllipsis)
                        .grid_column(3),
                    TextBlock::new()
                        .text(r.duration.map(duration).unwrap_or_default())
                        .font_size(12.0)
                        .opacity(0.6)
                        .horizontal_alignment(HorizontalAlignment::Right)
                        .grid_column(4),
                ));
            // 失敗したものは、その理由を下に（選んでコピーできる）。
            let row: View = match &r.error {
                Some(e) => StackPanel::new().children((
                    line,
                    TextBlock::new()
                        .text(e.clone())
                        .font_size(11.0)
                        .foreground(Brush::Solid(Color::rgb(0xc4, 0x2b, 0x1c)))
                        .text_wrapping(TextWrapping::Wrap)
                        .is_text_selection_enabled(true)
                        .margin(Thickness::new(124.0, 0.0, 0.0, 2.0)),
                )),
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
