//! Activity の画面（GTK4 版の `activity.rs`、macOS 版の `Activity.swift` のタブのうち Operations と History）:
//! Operations は実行した git とコアの命令（`amonhen_model::activity` の記録。新しい順）、History は Undo できる
//! 操作（`undoLog`）と「ここまで戻す」。開いている間は 0.5 秒ごとに読み直す。GTK4 版・macOS 版は別の窓にするが、
//! ここではまだ History と右の列の場所に重ねる。Performance・Events・Daemon のタブはまだない。

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
}

impl State {
    pub fn new() -> Self {
        let mut state = Self {
            tab: 0,
            records: Vec::new(),
            undo: Vec::new(),
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
                .items_source(vec![tr!("Operations"), "History".to_owned()])
                .selected_index(state.tab)
                .max_columns(2)
                .on_selection_changed(callbacks.tab)
                .grid_column(1),
            Button::new()
                .on_click(callbacks.close)
                .grid_column(2)
                .content(TextBlock::new().text(tr!("Close"))),
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
