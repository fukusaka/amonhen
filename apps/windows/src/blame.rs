//! Blame の画面: ファイルの各行を最後に変えたコミット。同じコミットの行をまとめ、まとまりの 1 行目に ID・作者・
//! 日時、2 行目に件名を出し、まとまりごとに背景を交互に変える（GTK4 版の `filetools.rs` の Blame、macOS 版の
//! `FileTools.swift` と同じ）。GTK4 版・macOS 版はタブにするが、ここではまだ History と右の列の場所に重ねる。
//! 行の右クリックで、そのコミットを History に出す（GTK4 版はダブルクリック）。

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// `blame` の答えを並べ直したもの。
pub struct Data {
    /// (oid, 作者, 時刻, 件名)。
    pub commits: Vec<(String, String, i64, String)>,
    /// 行ごとの (コミットの番号, 中身)。
    pub lines: Vec<(usize, String)>,
    /// 行ごとの構文の色（行の中のバイトの範囲と種類。位置の順）。言語がわからなければ空。
    pub syntax: Vec<Vec<(usize, usize, u8)>>,
}

/// `blame` の答えを並べ直し、ファイル全体を `path` の言語で色付けする（裏のスレッドで呼ぶ）。
pub fn data_of(info: &Value, path: &str) -> Data {
    let commits = info["commits"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            (
                c["oid"].as_str().unwrap_or_default().to_owned(),
                c["author"].as_str().unwrap_or_default().to_owned(),
                c["time"].as_i64().unwrap_or_default(),
                c["summary"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let lines = info["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            (
                l["c"].as_u64().unwrap_or_default() as usize,
                l["t"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect::<Vec<(usize, String)>>();
    let syntax = syntax_of(path, &lines);
    Data {
        commits,
        lines,
        syntax,
    }
}

/// ファイル全体（行を改行でつないだもの）を色付けし、色の範囲を行ごとに分ける（GTK4 版の Blame と同じく、行を
/// またぐ範囲は行ごとに切る）。
fn syntax_of(path: &str, lines: &[(usize, String)]) -> Vec<Vec<(usize, usize, u8)>> {
    let mut source = Vec::new();
    let mut starts = Vec::with_capacity(lines.len());
    for (_, text) in lines {
        starts.push(source.len());
        source.extend_from_slice(text.as_bytes());
        source.push(b'\n');
    }
    let mut out = vec![Vec::new(); lines.len()];
    for token in amonhen_syntax::highlight(path, &source) {
        let (start, end) = (token.start as usize, token.end as usize);
        // 範囲の始まりの行から、終わりの行まで。
        let first = starts.partition_point(|&s| s <= start).saturating_sub(1);
        for (line, &line_start) in starts.iter().enumerate().skip(first) {
            if line_start >= end {
                break;
            }
            let len = lines[line].1.len();
            let from = start.max(line_start) - line_start;
            let to = (end - line_start).min(len);
            if from < to {
                out[line].push((from, to, token.kind));
            }
        }
    }
    out
}

/// 行の中身を、構文の色の付いた runs にする（タブは 4 つの空白に。色のない所はそのまま）。
fn code_runs(text: &str, spans: &[(usize, usize, u8)]) -> RichText {
    let run = |part: &str, kind: Option<u8>| {
        let foreground = kind
            .and_then(crate::render::syntax_rgb)
            .map(|(r, g, b)| Color::rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8));
        RichTextInline::Run(RichTextRun {
            text: part.replace('\t', "    "),
            foreground,
            ..RichTextRun::default()
        })
    };
    let mut inlines = Vec::new();
    let mut at = 0;
    for &(start, end, kind) in spans {
        // 文字の境目でない範囲は使わない（壊れた位置で切らない）。
        if start < at || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        if at < start {
            inlines.push(run(&text[at..start], None));
        }
        inlines.push(run(&text[start..end], Some(kind)));
        at = end;
    }
    if at < text.len() {
        inlines.push(run(&text[at..], None));
    }
    RichText::single_paragraph(inlines)
}

pub struct State {
    pub path: String,
    /// コミット（なければ作業ツリー）。
    pub rev: Option<String>,
    pub data: Option<Result<Data, String>>,
}

pub struct Callbacks<'a> {
    pub close: Callback<()>,
    /// 行の右クリックのメニュー（`show:<oid>`・`copy:<oid>`）。
    pub menu: Box<dyn Fn() -> Callback<String> + 'a>,
    /// 行番号と中身の列（等幅の字にする。GTK4 版の `monospace`）。
    pub code: &'a ElementRef<Grid>,
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let mut title = format!("Blame — {}", state.path);
    if let Some(rev) = &state.rev {
        title.push_str(&format!(" @ {}", &rev[..8.min(rev.len())]));
    }
    let head = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            TextBlock::new()
                .text(title)
                .font_size(14.0)
                .vertical_alignment(VerticalAlignment::Center)
                .text_trimming(TextTrimming::CharacterEllipsis)
                .grid_column(0),
            crate::small::button(tr!("Close"), |b| b.on_click(callbacks.close).grid_column(1)),
        ));
    let body: View = match &state.data {
        None => TextBlock::new()
            .text(tr!("Loading…"))
            .opacity(0.6)
            .margin(16.0)
            .into(),
        Some(Err(e)) => TextBlock::new()
            .text(e.clone())
            .text_wrapping(TextWrapping::Wrap)
            .margin(16.0)
            .into(),
        Some(Ok(data)) => {
            let digits = data.lines.len().to_string().len();
            let (mut block, mut offset, mut previous) = (0usize, 0usize, usize::MAX);
            // 左に ID・作者・日時・件名の列、右に行番号と中身の列（こちらだけ等幅。GTK4 版と同じ）。右の列は字体を
            // 継ぐ Control（スクロールしない ScrollViewer）に入れて、まとめて等幅にする。
            let mut infos = Vec::with_capacity(data.lines.len());
            let mut codes = Vec::with_capacity(data.lines.len());
            for (i, (commit, text)) in data.lines.iter().enumerate() {
                if *commit != previous {
                    if i > 0 {
                        block += 1;
                    }
                    offset = 0;
                    previous = *commit;
                } else {
                    offset += 1;
                }
                let Some((oid, author, time, summary)) = data.commits.get(*commit) else {
                    continue;
                };
                let committed = oid.bytes().any(|b| b != b'0');
                // まとまりの 1 行目に ID・作者・日時、2 行目に件名。
                let info = match (committed, offset) {
                    (false, 0) => tr!("Not committed yet"),
                    (true, 0) => format!(
                        "{}  {author}  {}",
                        &oid[..8.min(oid.len())],
                        crate::render::local_date(*time)
                    ),
                    (true, 1) => summary.clone(),
                    _ => String::new(),
                };
                let background = if block % 2 == 1 {
                    crate::palette::bar()
                } else {
                    crate::palette::background()
                };
                let info = TextBlock::new()
                    .text(info)
                    .font_size(11.0)
                    .opacity(if offset == 0 { 0.75 } else { 0.5 })
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .vertical_alignment(VerticalAlignment::Center)
                    .margin(Thickness::new(6.0, 0.0, 6.0, 0.0));
                let code = Grid::new()
                    .columns([
                        GridLength::Pixel(8.0 * digits as f64 + 12.0),
                        GridLength::Star(1.0),
                    ])
                    .children((
                        TextBlock::new()
                            .text(format!("{:>digits$}", i + 1))
                            .font_size(12.0)
                            .opacity(0.5)
                            .horizontal_alignment(HorizontalAlignment::Right)
                            .vertical_alignment(VerticalAlignment::Center)
                            .margin(Thickness::new(0.0, 0.0, 8.0, 0.0))
                            .grid_column(0),
                        RichTextBlock::new()
                            .paragraphs(code_runs(
                                text,
                                data.syntax.get(i).map_or(&[][..], Vec::as_slice),
                            ))
                            .font_size(12.0)
                            .text_wrapping(TextWrapping::NoWrap)
                            .vertical_alignment(VerticalAlignment::Center)
                            .grid_column(1),
                    ));
                // まだコミットしていない行は History に出せない。
                let menu = |view: View| -> View {
                    if committed {
                        view.menu(Menu::new(
                            [
                                MenuItem::item(format!("show:{oid}"), tr!("Show in History")),
                                MenuItem::item(format!("copy:{oid}"), tr!("Copy Commit ID")),
                            ],
                            (callbacks.menu)(),
                        ))
                    } else {
                        view
                    }
                };
                infos.push(menu(
                    Border::new()
                        .height(20.0)
                        .background(Brush::Solid(background))
                        .content(info),
                ));
                codes.push(menu(
                    Border::new()
                        .height(20.0)
                        .background(Brush::Solid(background))
                        .content(code),
                ));
            }
            let infos = View::keyed_fragment(infos.into_iter().enumerate());
            let codes = View::keyed_fragment(codes.into_iter().enumerate());
            ScrollViewer::new().content(
                Grid::new()
                    .columns([GridLength::Pixel(300.0), GridLength::Star(1.0)])
                    .children((
                        StackPanel::new().grid_column(0).children((infos,)),
                        Border::new().grid_column(1).content(
                            ScrollViewer::new()
                                .vertical_scroll_bar_visibility(ScrollBarVisibility::Disabled)
                                .content(
                                    Grid::new()
                                        .element_ref(callbacks.code)
                                        .children((StackPanel::new().children((codes,)),)),
                                ),
                        ),
                    )),
            )
        }
    };
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((head, Border::new().grid_row(1).content(body)))
}
