//! Markdown を、FE が自前で組み立てて見せるための形（見出し・段落・リスト・引用・表・コードなどの並び）にする
//! （History の「すべてのファイル」の整形表示）。読み取りは pulldown-cmark（CommonMark と GitHub の拡張の表・取り消し線・
//! タスクリスト・脚注・数式）。数式は `math` で描く。HTML は GitHub が描くものだけを同じ形に移す（`html`。太字・リンク・画像・表・`<details>`
//! など。`style`・`class`・`<script>`・フォームは描かず、知らないタグは落として中の文字を残す）。

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use serde::Serialize;

mod dollars;
mod entities;
mod html;

/// 文字の飾り（ひと続きの文字）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Run {
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    /// インラインのコード（`code`）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub code: bool,
    /// キー（HTML の `<kbd>`。FE は小さなキーの札で見せる）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub kbd: bool,
    /// 下付き（`<sub>`）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sub: bool,
    /// 上付き（`<sup>`）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sup: bool,
    /// 下線（`<ins>`）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    /// リンク先（リンクの中の文字なら）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// 画像（`text` は代わりの文、`image` はその場所）。画像にもリンクがあれば `link`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// `<picture>` の、暗い外観で見せる画像（`<source media="(prefers-color-scheme: dark)">`）。FE は外観に合う
    /// ものがあればそれを、なければ `image` を見せる。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_dark: Option<String>,
    /// `<picture>` の、明るい外観で見せる画像。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_light: Option<String>,
    /// 画像の幅・高さ（HTML の `width`・`height`、画素）。片方だけなら、もう片方は比を保つ。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// 画像の幅の割合（HTML の `width="50%"`、1〜100）。FE は表示する列の幅（表のマスの中ならマスの幅）に対して
    /// 決める。`width` とは同時に付かない。高さの割合は読まない（ブラウザでも、高さの決まらない親の中では効かない）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width_percent: Option<u32>,
    /// 脚注への参照（ラベル）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footnote: Option<String>,
    /// GitHub 独自の絵文字（`:octocat:` などの画像だけのもの。名前）。FE は `text`（`:octocat:`）を薄い札で見せる。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_emoji: Option<String>,
    /// 段落の中の改行（`text` は空）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub line_break: bool,
    /// 文中の数式（`$…$`・`` $`…`$ ``、見出し・表のマスの中の `$$…$$`）を描いた絵。`text` は書いたままの原文
    /// （`$x^2$`）で、`code` が付いているので、絵を描かない FE はそれをコードとして見せる。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub math: Option<crate::math::MathPicture>,
    /// 文中の数式を描けなかった理由（FE は原文をコードとして見せ、理由を添える）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub math_error: Option<String>,
    /// 文中の数式の TeX（区切りの `$` を除いたもの）と、ブロックの組み方で描くか（`math::render_all` が描く）。
    /// `math` フィーチャーのない作り（エンジン）でも書き出すので、絵を別に描く FE（VSCode 拡張の `amonhen-render`）は
    /// これで文中の式を見分ける。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) math_tex: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) math_display: bool,
    /// HTML の中の文字（Markdown として読んだ文でない）。GitHub と同じく、素の URL はリンクにしない。
    #[serde(skip)]
    pub(crate) html: bool,
}

/// リストの 1 項目。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    /// タスクリストの印（`[x]` なら true、`[ ]` なら false、なければ None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

/// 表の列・段落・見出しのそろえ方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    None,
    Left,
    Center,
    Right,
}

impl Align {
    pub fn is_none(&self) -> bool {
        *self == Align::None
    }

    /// HTML の `align` の値（left・center・right。ほかは None）。
    fn parse(value: &str) -> Align {
        match value.trim().to_ascii_lowercase().as_str() {
            "left" => Align::Left,
            "center" | "middle" => Align::Center,
            "right" => Align::Right,
            _ => Align::None,
        }
    }
}

/// HTML の表（`<table>`）のマス 1 つ。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TableCell {
    /// 左上の位置（0 から）と、またがる行・列の数（1 以上）。
    pub row: u32,
    pub column: u32,
    pub rowspan: u32,
    pub colspan: u32,
    /// 見出しのマス（`<th>`）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub header: bool,
    #[serde(skip_serializing_if = "Align::is_none")]
    pub align: Align,
    pub runs: Vec<Run>,
}

/// ブロック 1 つ。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    /// 見出し。`align` は HTML の `align`（`<h1 align="center">`、`<div align="center">` の中）。
    Heading {
        level: u8,
        runs: Vec<Run>,
        #[serde(skip_serializing_if = "Align::is_none")]
        align: Align,
    },
    /// 段落。`align` は HTML の `align`（`<p align="center">` など）。
    Paragraph {
        runs: Vec<Run>,
        #[serde(skip_serializing_if = "Align::is_none")]
        align: Align,
    },
    /// コードブロック。`language` はフェンスの言葉（` ```rust ` の rust、なければ空）。
    Code {
        language: String,
        text: String,
    },
    /// 図（` ```mermaid ` など、`diagram::draw` で描けたもの）。`svg` は色を CSS の変数のままにした SVG で、FE は
    /// 描くときに `diagram::colorize` で配色を当てる。描けなければ `svg` はなく、`error` に理由（FE は `text` を
    /// コードとして見せ、理由を添える）。
    Diagram {
        language: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        svg: Option<String>,
        /// 図の中で指定した Mermaid のテーマ（設定の配色より先に使う）。
        #[serde(skip_serializing_if = "Option::is_none")]
        config: Option<crate::diagram::MermaidConfig>,
        /// 近いもので描いたことを伝える一文（FE は絵の下に添える）。
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// 数式のブロック（```` ```math ````・`$$…$$`）。`text` は TeX。`math` の絵（`svg`・`width`・`height`・`depth`、
    /// 大きさは em）は色を CSS の変数のままにしたもので、FE は描くときに `math::colorize` で文字の色を当てる。描けな
    /// ければ絵はなく、`error` に理由（FE は `text` をコードとして見せ、理由を添える）。`math` フィーチャーのない
    /// 作りでは、どちらもない（FE は `text` をコードとして見せる）。
    Math {
        text: String,
        #[serde(flatten, skip_serializing_if = "Option::is_none")]
        math: Option<crate::math::MathPicture>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// 引用。`alert` は GitHub の囲み（`> [!NOTE]` の note・tip・important・warning・caution）。
    Quote {
        #[serde(skip_serializing_if = "Option::is_none")]
        alert: Option<String>,
        blocks: Vec<Block>,
    },
    /// リスト。`start` は番号つきのときの最初の番号（番号なしなら None）。
    List {
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<u64>,
        items: Vec<Item>,
    },
    Table {
        aligns: Vec<Align>,
        head: Vec<Vec<Run>>,
        rows: Vec<Vec<Vec<Run>>>,
    },
    Rule,
    /// HTML の `<details>`: 押すと開く・閉じる。`summary` は見出し（なければ空で、FE は「Details」）、`open` は
    /// 最初から開いているか。
    Details {
        summary: Vec<Run>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        open: bool,
        blocks: Vec<Block>,
    },
    /// HTML の表（`<table>`）。マスは行・列の順で、マスのない所は空のマスで埋めてある（`colspan`・`rowspan` を
    /// 解いた後の位置）。
    HtmlTable {
        columns: u32,
        rows: u32,
        cells: Vec<TableCell>,
    },
    /// 脚注の中身。
    Footnote {
        label: String,
        blocks: Vec<Block>,
    },
}

/// 読みかけのブロック（入れ子）。
enum Open {
    Heading(u8),
    Paragraph,
    Code(String, String),
    Quote(Option<String>, Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Option<bool>, Vec<Block>),
    Table(Vec<Align>, Vec<Vec<Run>>, Vec<Vec<Vec<Run>>>, bool),
    Row(Vec<Vec<Run>>),
    Cell,
    Footnote(String, Vec<Block>),
    /// HTML の要素（タグの名前と中身）。閉じのタグか、外の要素・Markdown のブロックが閉じたときに閉じる。
    Tag(String, Elem),
}

/// HTML の要素の読みかけ。
enum Elem {
    /// 文字の飾り（閉じたら戻す）。
    Style(Inline),
    /// リンク（`style.links` に積んだもの）。
    Link,
    /// `<p>`・`<div>`・`<h1>` など、そろえ方（と見出しの段）を持つ箱。中の文字は段落か見出しになる。
    Box {
        align: Align,
        heading: Option<u8>,
    },
    Details {
        open: bool,
        summary: Option<Vec<Run>>,
        blocks: Vec<Block>,
    },
    /// `<details>` の見出し（中のブロックは文字にしてつなぐ）。
    Summary(Vec<Run>),
    Quote(Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Vec<Block>),
    Table(Vec<Vec<CellDraft>>),
    /// 行（マスと、行の `align`）。
    Row(Vec<CellDraft>, Align),
    Cell(CellDraft),
    Pre {
        language: String,
        text: String,
    },
    /// `<picture>`（`<source>` で選んだ、暗い外観・明るい外観の画像）。
    Picture {
        dark: Option<String>,
        light: Option<String>,
    },
}

impl Elem {
    /// ブロックの要素か（開け閉めの前に、そこまでの文字を段落にする）。
    fn is_block(&self) -> bool {
        !matches!(self, Elem::Style(_) | Elem::Link | Elem::Picture { .. })
    }
}

/// HTML の文字の飾り。
#[derive(Clone, Copy)]
enum Inline {
    Bold,
    Italic,
    Strike,
    Code,
    Kbd,
    Sub,
    Sup,
    Underline,
}

/// HTML の表のマスの読みかけ。
struct CellDraft {
    header: bool,
    rowspan: u32,
    colspan: u32,
    align: Align,
    runs: Vec<Run>,
}

/// HTML のタグの数の上限（超えたら、タグは読まずに中の文字だけを出す）。
const MAX_TAGS: usize = 10_000;
/// 開いたままの HTML の要素の数の上限（超えたら、入れ子にする要素は開かない）。
const MAX_DEPTH: usize = 64;
/// HTML の表の行・列の数の上限。
const MAX_ROWS: u32 = 1000;
const MAX_COLUMNS: u32 = 64;
/// 画像の `width`・`height` の上限（画素）。
const MAX_IMAGE_SIDE: u32 = 4096;

#[derive(Default)]
struct Style {
    bold: u32,
    italic: u32,
    strike: u32,
    code: u32,
    kbd: u32,
    sub: u32,
    sup: u32,
    underline: u32,
    links: Vec<String>,
    image: Option<String>,
}

impl Style {
    fn counter(&mut self, inline: Inline) -> &mut u32 {
        match inline {
            Inline::Bold => &mut self.bold,
            Inline::Italic => &mut self.italic,
            Inline::Strike => &mut self.strike,
            Inline::Code => &mut self.code,
            Inline::Kbd => &mut self.kbd,
            Inline::Sub => &mut self.sub,
            Inline::Sup => &mut self.sup,
            Inline::Underline => &mut self.underline,
        }
    }
}

/// 同じ飾りの文字か（つなげてよいか）。
fn same_style(a: &Run, b: &Run) -> bool {
    (
        a.bold,
        a.italic,
        a.strike,
        a.code,
        a.kbd,
        a.sub,
        a.sup,
        a.underline,
        &a.link,
        a.html,
    ) == (
        b.bold,
        b.italic,
        b.strike,
        b.code,
        b.kbd,
        b.sub,
        b.sup,
        b.underline,
        &b.link,
        b.html,
    )
}

/// 段落の頭と終わりの空白・改行を落とす（HTML の中の文字は改行や字下げを含む）。
fn tidy(mut runs: Vec<Run>) -> Vec<Run> {
    let blank = |c: char| c.is_ascii_whitespace();
    while let Some(first) = runs.first_mut() {
        if first.line_break {
            runs.remove(0);
            continue;
        }
        if first.image.is_some() || first.footnote.is_some() {
            break;
        }
        let trimmed = first.text.trim_start_matches(blank);
        if trimmed.is_empty() {
            runs.remove(0);
            continue;
        }
        first.text = trimmed.to_owned();
        break;
    }
    while let Some(last) = runs.last_mut() {
        if last.line_break {
            runs.pop();
            continue;
        }
        if last.image.is_some() || last.footnote.is_some() {
            break;
        }
        let trimmed = last.text.trim_end_matches(blank);
        if trimmed.is_empty() {
            runs.pop();
            continue;
        }
        last.text.truncate(trimmed.len());
        break;
    }
    runs
}

/// `runs` に `more` を改行でつなぐ。
fn join(runs: &mut Vec<Run>, more: Vec<Run>) {
    if more.is_empty() {
        return;
    }
    if !runs.is_empty() {
        runs.push(Run {
            line_break: true,
            ..Run::default()
        });
    }
    runs.extend(more);
}

/// ブロックを文字にする（表のマス・`<summary>` の中のブロック）。
fn flatten(block: Block) -> Vec<Run> {
    let mut out = Vec::new();
    match block {
        Block::Paragraph { runs, .. } => out = runs,
        Block::Heading { runs, .. } => {
            out = runs.into_iter().map(|r| Run { bold: true, ..r }).collect()
        }
        Block::Code { text, .. } | Block::Diagram { text, .. } => out.push(Run {
            text,
            code: true,
            ..Run::default()
        }),
        // 表のマス・見出しの中では、文中の式としてブロックの組み方で描く
        Block::Math { text, .. } => out.push(Run {
            text: format!("$${text}$$"),
            code: true,
            math_tex: Some(text),
            math_display: true,
            ..Run::default()
        }),
        Block::Quote { blocks, .. }
        | Block::Footnote { blocks, .. }
        | Block::Details { blocks, .. } => {
            for b in blocks {
                join(&mut out, flatten(b));
            }
        }
        Block::List { items, .. } => {
            for item in items {
                let mut line = vec![Run {
                    text: "• ".into(),
                    ..Run::default()
                }];
                for b in item.blocks {
                    line.extend(flatten(b));
                }
                join(&mut out, line);
            }
        }
        Block::Table { head, rows, .. } => {
            for row in std::iter::once(head).chain(rows) {
                let mut line = Vec::new();
                for cell in row {
                    if !line.is_empty() {
                        line.push(Run {
                            text: " ".into(),
                            ..Run::default()
                        });
                    }
                    line.extend(cell);
                }
                join(&mut out, line);
            }
        }
        Block::HtmlTable { cells, .. } => {
            let mut row = 0;
            let mut line = Vec::new();
            for cell in cells {
                if cell.row != row {
                    join(&mut out, std::mem::take(&mut line));
                    row = cell.row;
                }
                if !cell.runs.is_empty() {
                    if !line.is_empty() {
                        line.push(Run {
                            text: " ".into(),
                            ..Run::default()
                        });
                    }
                    line.extend(cell.runs);
                }
            }
            join(&mut out, line);
        }
        Block::Rule => {}
    }
    out
}

/// リンク・画像の場所として使ってよいか（http・https と相対パス、リンクは mailto も。`javascript:` などは落とす）。
fn safe_url(value: &str, link: bool) -> Option<String> {
    let url: String = value
        .trim()
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    if url.is_empty() {
        return None;
    }
    let scheme_end = url.find([':', '/', '?', '#']);
    if let Some(end) = scheme_end
        && url[end..].starts_with(':')
    {
        let scheme = url[..end].to_ascii_lowercase();
        let allowed = matches!(scheme.as_str(), "http" | "https") || (link && scheme == "mailto");
        if !allowed {
            return None;
        }
    }
    Some(url)
}

/// `width`・`height` の値（HTML の大きさの読み方: 頭の数字を読み、すぐ後に `%` があれば割合、ほかは画素。`px` など
/// 後ろの文字は読まない）。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Pixels(u32),
    Percent(u32),
}

fn image_side(value: &str) -> Option<Side> {
    let v = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
    if digits == 0 {
        return None;
    }
    let mut end = digits;
    if v[end..].starts_with('.') {
        let fraction = v[end + 1..]
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(v.len() - end - 1);
        if fraction > 0 {
            end += 1 + fraction;
        }
    }
    let n: f64 = v[..end].parse().ok()?;
    if !n.is_finite() || n < 1.0 {
        return None;
    }
    // 割合は 100% まで（それより広くても列の幅に収める）。
    Some(match v[end..].starts_with('%') {
        true => Side::Percent((n.round() as u32).min(100)),
        false => Side::Pixels((n.round() as u32).min(MAX_IMAGE_SIDE)),
    })
}

/// `colspan`・`rowspan` の値（数字の頭だけ。読めなければ 1）。
fn span(value: Option<&str>) -> u32 {
    let Some(v) = value else { return 1 };
    let digits: String = v.trim().chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u32>().map_or(1, |n| n.min(MAX_ROWS))
}

/// HTML の表の行を、マスの位置に解く（`colspan`・`rowspan` を数え、隙間は空のマスで埋める）。
fn layout(rows: Vec<Vec<CellDraft>>) -> Option<Block> {
    let row_count = (rows.len() as u32).min(MAX_ROWS);
    let mut taken = vec![[false; MAX_COLUMNS as usize]; row_count as usize];
    let mut cells = Vec::new();
    let mut columns = 0;
    for (r, row) in rows.into_iter().take(row_count as usize).enumerate() {
        let r = r as u32;
        let mut c = 0;
        for cell in row {
            while c < MAX_COLUMNS && taken[r as usize][c as usize] {
                c += 1;
            }
            if c >= MAX_COLUMNS {
                break;
            }
            let colspan = cell.colspan.clamp(1, MAX_COLUMNS - c);
            let rowspan = match cell.rowspan {
                // 0 は表の終わりまで。
                0 => row_count - r,
                n => n.min(row_count - r),
            }
            .max(1);
            for line in &mut taken[r as usize..(r + rowspan) as usize] {
                for spot in &mut line[c as usize..(c + colspan) as usize] {
                    *spot = true;
                }
            }
            columns = columns.max(c + colspan);
            cells.push(TableCell {
                row: r,
                column: c,
                rowspan,
                colspan,
                header: cell.header,
                align: cell.align,
                runs: tidy(cell.runs),
            });
            c += colspan;
        }
    }
    if cells.is_empty() {
        return None;
    }
    for (r, line) in taken.iter().enumerate() {
        for (c, spot) in line.iter().enumerate().take(columns as usize) {
            if !spot {
                cells.push(TableCell {
                    row: r as u32,
                    column: c as u32,
                    rowspan: 1,
                    colspan: 1,
                    header: false,
                    align: Align::None,
                    runs: Vec::new(),
                });
            }
        }
    }
    cells.sort_by_key(|c| (c.row, c.column));
    Some(Block::HtmlTable {
        columns,
        rows: row_count,
        cells,
    })
}

struct Builder {
    stack: Vec<Open>,
    blocks: Vec<Block>,
    /// 今の段落・見出し・表のマスの文字。
    runs: Vec<Run>,
    style: Style,
    tokenizer: html::Tokenizer,
    /// 読みかけの HTML のブロックの文字（タグは行をまたぐので、閉じたときにまとめて読む）。
    html_block: Option<String>,
    /// 読んだ HTML のタグの数と、開いたままの HTML の要素の数。
    tags: usize,
    depth: usize,
    /// `<kbd>` を開け閉めした後（次の文字は前のキーにつなげない）。
    kbd_break: bool,
}

impl Builder {
    fn new() -> Self {
        Builder {
            stack: Vec::new(),
            blocks: Vec::new(),
            runs: Vec::new(),
            style: Style::default(),
            tokenizer: html::Tokenizer::default(),
            html_block: None,
            tags: 0,
            depth: 0,
            kbd_break: false,
        }
    }

    fn push_run(&mut self, text: &str, code: bool) {
        self.push_text(text, code, false);
    }

    /// 文字を足す。`html` は HTML の中の文字（素の URL をリンクにしない）。
    fn push_text(&mut self, text: &str, code: bool, html: bool) {
        // 頭の空白は、前の文字の終わりの空白にたたむ（HTML のコメントを落とした前後の `a <!-- x --> b` など。ブラウザ
        // と同じく空白 1 つに）。コードと画像の代わりの文はそのまま。
        let blank = |c: char| c.is_ascii_whitespace();
        let text = if !code
            && self.style.code == 0
            && self.style.image.is_none()
            && text.starts_with(blank)
            && self.runs.last().is_some_and(|r| {
                !r.line_break
                    && !r.code
                    && r.image.is_none()
                    && r.footnote.is_none()
                    && r.text.ends_with(blank)
            }) {
            let rest = text.trim_start_matches(blank);
            if rest.is_empty() {
                return;
            }
            rest
        } else {
            text
        };
        let s = &self.style;
        let mut run = Run {
            text: text.to_owned(),
            bold: s.bold > 0,
            italic: s.italic > 0,
            strike: s.strike > 0,
            code: code || s.code > 0,
            kbd: s.kbd > 0,
            sub: s.sub > 0,
            sup: s.sup > 0,
            underline: s.underline > 0,
            link: s.links.last().cloned(),
            html,
            ..Run::default()
        };
        if let Some(url) = &self.style.image {
            // 画像の中の文字は代わりの文（ひとまとめにする）。
            if let Some(last) = self.runs.last_mut()
                && last.image.as_deref() == Some(url)
            {
                last.text.push_str(text);
                return;
            }
            run.image = Some(url.clone());
        }
        // 同じ飾りの続きはつなげる。
        if let Some(last) = self.runs.last_mut()
            && last.image.is_none()
            && last.footnote.is_none()
            && !last.line_break
            && last.math_tex.is_none()
            // キーはタグごとに 1 つの札（`<kbd>A</kbd><kbd>B</kbd>` を 1 つにしない）。
            && !(last.kbd && run.kbd && self.kbd_break)
            && same_style(last, &run)
        {
            last.text.push_str(text);
            return;
        }
        self.kbd_break = false;
        self.runs.push(run);
    }

    /// pulldown-cmark が数式にした所を、地の文として段落に置く（式にするかは `dollars` が選び直す）。原文 `raw` は
    /// 行をまたぐと引用の `>`・字下げも含むので、行は `tex`（入れ物の印を外したもの）から取る。GitHub と同じく、
    /// 行の境は文字の並びに残し（`$a` と次の行の頭の `$` は組になる）、硬い改行（行末の空白 2 つ・`\`）はそこで
    /// 改行にする（それをまたぐ `$` は組にならない）。
    fn math_source(&mut self, raw: &str, tex: &str) {
        let delim = &raw[..raw.len() - raw.trim_start_matches('$').len()];
        if !raw.contains('\n')
            || delim.is_empty()
            || raw.len() < 2 * delim.len()
            || !raw.ends_with(delim)
        {
            self.push_run(&dollars::unescape(raw), false);
            return;
        }
        let source = format!("{delim}{tex}{delim}");
        let mut lines = source.split('\n').peekable();
        while let Some(line) = lines.next() {
            if lines.peek().is_none() {
                self.push_run(&dollars::unescape(line), false);
                break;
            }
            let slashes = line.len() - line.trim_end_matches('\\').len();
            if line.ends_with("  ") {
                self.push_run(&dollars::unescape(line.trim_end_matches(' ')), false);
                self.line_break();
            } else if slashes % 2 == 1 {
                self.push_run(&dollars::unescape(&line[..line.len() - 1]), false);
                self.line_break();
            } else {
                self.push_run(&dollars::unescape(line), false);
                self.push_run("\n", false);
            }
        }
    }

    fn line_break(&mut self) {
        self.runs.push(Run {
            line_break: true,
            ..Run::default()
        });
    }

    /// `$$…$$` を、段落を分けて数式のブロックにする（MathJax・GitHub と同じく、独立した行に中央寄せで描く）。
    /// 見出し・表のマス・画像の代わりの文の中はブロックにせず false（文中の式として `dollars` が選ぶ）。
    fn display_math(&mut self, tex: &str) -> bool {
        if self.style.image.is_some() {
            return false;
        }
        let place = self.container().map(|i| &self.stack[i]);
        match place {
            Some(Open::Paragraph) => {
                let runs = tidy(std::mem::take(&mut self.runs));
                if !runs.is_empty() {
                    self.add(Block::Paragraph {
                        runs,
                        align: Align::None,
                    });
                }
            }
            Some(
                Open::Heading(_)
                | Open::Cell
                | Open::Tag(
                    _,
                    Elem::Summary(_)
                    | Elem::Cell(_)
                    | Elem::Box {
                        heading: Some(_), ..
                    },
                ),
            ) => return false,
            _ => self.boundary(),
        }
        self.add(Block::Math {
            text: tex.to_owned(),
            math: None,
            error: None,
        });
        true
    }

    /// 閉じたブロックを、入れ子の親（なければ一番上）に足す。段落・見出しは、囲む HTML の `align` でそろえる。
    fn add(&mut self, mut block: Block) {
        if let Block::Paragraph { align, .. } | Block::Heading { align, .. } = &mut block
            && align.is_none()
        {
            *align = self
                .stack
                .iter()
                .rev()
                .find_map(|open| match open {
                    Open::Tag(_, Elem::Box { align, .. }) if !align.is_none() => Some(*align),
                    _ => None,
                })
                .unwrap_or_default();
        }
        for open in self.stack.iter_mut().rev() {
            match open {
                Open::Quote(_, blocks)
                | Open::Item(_, blocks)
                | Open::Footnote(_, blocks)
                | Open::Tag(
                    _,
                    Elem::Details { blocks, .. } | Elem::Quote(blocks) | Elem::Item(blocks),
                ) => {
                    blocks.push(block);
                    return;
                }
                Open::Tag(_, Elem::Summary(runs) | Elem::Cell(CellDraft { runs, .. })) => {
                    join(runs, flatten(block));
                    return;
                }
                Open::Tag(_, Elem::List(_, items)) => {
                    // `<li>` の外に書いたもの。
                    items.push(Item {
                        task: None,
                        blocks: vec![block],
                    });
                    return;
                }
                _ => {}
            }
        }
        self.blocks.push(block);
    }

    /// 一番内側の、文字の飾りでない入れ子の位置。
    fn container(&self) -> Option<usize> {
        self.stack.iter().rposition(|open| match open {
            Open::Tag(_, elem) => elem.is_block(),
            _ => true,
        })
    }

    /// Markdown の段落・見出し・表のマスの中か（その中の HTML は文字の飾りだけを読む）。
    fn in_markdown_text(&self) -> bool {
        self.container().is_some_and(|i| {
            matches!(
                self.stack[i],
                Open::Paragraph | Open::Heading(_) | Open::Cell
            )
        })
    }

    /// `<pre>` の中ならその文字。
    fn pre(&mut self) -> Option<&mut String> {
        let i = self.container()?;
        match &mut self.stack[i] {
            Open::Tag(_, Elem::Pre { text, .. }) => Some(text),
            _ => None,
        }
    }

    /// ブロックの区切り: 段落を開かずに書かれた文字（「きつい」リストの項目の中、HTML の中）を段落にする。表のマス・
    /// `<summary>` の中なら、その文字に改行でつなぐ。
    fn boundary(&mut self) {
        if self.runs.is_empty() {
            return;
        }
        let mut heading = None;
        if let Some(i) = self.container() {
            match &mut self.stack[i] {
                Open::Paragraph | Open::Heading(_) | Open::Cell | Open::Code(..) => return,
                Open::Tag(_, Elem::Summary(runs) | Elem::Cell(CellDraft { runs, .. })) => {
                    let more = tidy(std::mem::take(&mut self.runs));
                    join(runs, more);
                    return;
                }
                Open::Tag(
                    _,
                    Elem::Box {
                        heading: Some(level),
                        ..
                    },
                ) => heading = Some(*level),
                _ => {}
            }
        }
        let runs = tidy(std::mem::take(&mut self.runs));
        if runs.is_empty() {
            return;
        }
        self.add(match heading {
            Some(level) => Block::Heading {
                level,
                runs,
                align: Align::None,
            },
            None => Block::Paragraph {
                runs,
                align: Align::None,
            },
        });
    }

    /// Markdown のブロックが閉じる前に、その中で開いたままの HTML の要素を閉じる。
    fn close_tags(&mut self) {
        while matches!(self.stack.last(), Some(Open::Tag(..))) {
            self.pop_tag();
        }
        self.tokenizer.leave_raw();
    }

    /// HTML を読む（HTML のブロックの全体か、段落の中のタグ 1 つ）。
    fn html(&mut self, input: &str) {
        let mut tokens = Vec::new();
        self.tokenizer.feed(input, &mut tokens);
        for token in tokens {
            match token {
                html::Token::Text(text) => self.html_text(&text),
                html::Token::Open { name, attributes } => {
                    if self.tags < MAX_TAGS {
                        self.tags += 1;
                        self.open_tag(&name, &attributes);
                    }
                }
                html::Token::Close { name } => {
                    if self.tags < MAX_TAGS {
                        self.tags += 1;
                        self.close_tag(&name);
                    }
                }
            }
        }
    }

    /// HTML の中の文字（空白はまとめて 1 つに、`<pre>` の中はそのまま）。
    fn html_text(&mut self, text: &str) {
        if let Some(pre) = self.pre() {
            // `<pre>` の直後の改行は数えない。
            let text = if pre.is_empty() {
                text.strip_prefix('\n').unwrap_or(text)
            } else {
                text
            };
            pre.push_str(text);
            return;
        }
        let mut collapsed = String::with_capacity(text.len());
        let mut space = self.runs.last().is_none_or(|r| {
            r.line_break
                || (r.image.is_none() && r.text.ends_with(|c: char| c.is_ascii_whitespace()))
        });
        for c in text.chars() {
            if c.is_ascii_whitespace() {
                if !space {
                    collapsed.push(' ');
                }
                space = true;
            } else {
                collapsed.push(c);
                space = false;
            }
        }
        if collapsed.is_empty() {
            return;
        }
        // 表・行・リストの中の、マス・項目の外の空白は落とす。
        if collapsed == " "
            && self.container().is_some_and(|i| {
                matches!(
                    self.stack[i],
                    Open::Tag(_, Elem::Table(_) | Elem::Row(..) | Elem::List(..))
                )
            })
        {
            return;
        }
        self.push_text(&collapsed, false, true);
    }

    /// 要素を開く（深すぎれば開かない）。
    fn push_tag(&mut self, name: &str, elem: Elem) {
        if self.depth >= MAX_DEPTH {
            return;
        }
        if let Elem::Style(inline) = elem {
            *self.style.counter(inline) += 1;
            self.kbd_break |= matches!(inline, Inline::Kbd);
        }
        self.depth += 1;
        self.stack.push(Open::Tag(name.to_owned(), elem));
    }

    /// リンクを開く。
    fn push_link(&mut self, name: &str, href: String) {
        if self.depth >= MAX_DEPTH {
            return;
        }
        self.depth += 1;
        self.style.links.push(href);
        self.stack.push(Open::Tag(name.to_owned(), Elem::Link));
    }

    /// 一番上の HTML の要素を閉じて、できたブロックを親に足す。
    fn pop_tag(&mut self) {
        if matches!(self.stack.last(), Some(Open::Tag(_, elem)) if elem.is_block()) {
            self.boundary();
        }
        if !matches!(self.stack.last(), Some(Open::Tag(..))) {
            return;
        }
        let Some(Open::Tag(_, elem)) = self.stack.pop() else {
            return;
        };
        self.depth = self.depth.saturating_sub(1);
        match elem {
            Elem::Style(inline) => {
                let n = self.style.counter(inline);
                *n = n.saturating_sub(1);
                self.kbd_break |= matches!(inline, Inline::Kbd);
            }
            Elem::Link => {
                self.style.links.pop();
            }
            Elem::Box { .. } | Elem::Picture { .. } => {}
            Elem::Details {
                open,
                summary,
                blocks,
            } => self.add(Block::Details {
                summary: summary.unwrap_or_default(),
                open,
                blocks,
            }),
            Elem::Summary(runs) => {
                let runs = tidy(runs);
                // 囲む `<details>` の見出しにする（ほかに見出しがあれば段落）。
                let details = self.find_tag(
                    |_, e| matches!(e, Elem::Details { summary: None, .. }),
                    |e| matches!(e, Elem::Details { .. }),
                );
                if let Some(i) = details
                    && let Open::Tag(_, Elem::Details { summary, .. }) = &mut self.stack[i]
                {
                    *summary = Some(runs);
                } else if !runs.is_empty() {
                    self.add(Block::Paragraph {
                        runs,
                        align: Align::None,
                    });
                }
            }
            Elem::Quote(blocks) => self.add(Block::Quote {
                alert: None,
                blocks,
            }),
            Elem::List(start, items) => {
                if !items.is_empty() {
                    self.add(Block::List { start, items });
                }
            }
            Elem::Item(blocks) => match self.stack.last_mut() {
                Some(Open::Tag(_, Elem::List(_, items))) => items.push(Item { task: None, blocks }),
                _ => blocks.into_iter().for_each(|b| self.add(b)),
            },
            Elem::Table(rows) => {
                if let Some(table) = layout(rows) {
                    self.add(table);
                }
            }
            Elem::Row(cells, _) => {
                if let Some(Open::Tag(_, Elem::Table(rows))) = self.stack.last_mut() {
                    rows.push(cells);
                }
            }
            Elem::Cell(cell) => {
                if let Some(Open::Tag(_, Elem::Row(cells, _))) = self.stack.last_mut() {
                    cells.push(cell);
                }
            }
            Elem::Pre { language, mut text } => {
                if text.ends_with('\n') {
                    text.pop();
                }
                self.add(Block::Code { language, text });
            }
        }
    }

    /// `stack[i]` から上をすべて閉じる。
    fn close_from(&mut self, i: usize) {
        while self.stack.len() > i && matches!(self.stack.last(), Some(Open::Tag(..))) {
            self.pop_tag();
        }
    }

    /// 開いている HTML の要素のうち、Markdown のブロックをまたがずに見つかる、`found` に合う一番内側のもの。
    /// `stop` に合う要素があればそこで探すのをやめる。
    fn find_tag(
        &self,
        found: impl Fn(&str, &Elem) -> bool,
        stop: impl Fn(&Elem) -> bool,
    ) -> Option<usize> {
        for (i, open) in self.stack.iter().enumerate().rev() {
            match open {
                Open::Tag(name, elem) if found(name, elem) => return Some(i),
                Open::Tag(_, elem) if stop(elem) => return None,
                Open::Tag(..) => {}
                _ => return None,
            }
        }
        None
    }

    /// 一番内側の要素が `<p>` なら閉じる（ブロックの要素は `<p>` の中に入らない）。
    fn close_p(&mut self) {
        if let Some(i) = self.container()
            && matches!(&self.stack[i], Open::Tag(name, Elem::Box { .. }) if name == "p")
        {
            self.close_from(i);
        }
    }

    /// ブロックの要素を開く前に: 開いている `<p>` を閉じ、そこまでの文字を段落にする。
    fn begin_block(&mut self) {
        self.close_p();
        self.boundary();
    }

    fn open_tag(&mut self, name: &str, attributes: &[(String, String)]) {
        let attr = |key: &str| {
            attributes
                .iter()
                .find(|(a, _)| a == key)
                .map(|(_, v)| v.as_str())
        };
        if let Some(pre) = self.pre() {
            // `<pre>` の中は文字だけ（改行のほかの飾りは読まない）。
            if name == "br" {
                pre.push('\n');
            }
            return;
        }
        let style = |inline| Some(Elem::Style(inline));
        let inline = match name {
            "b" | "strong" => style(Inline::Bold),
            "i" | "em" | "var" | "cite" | "dfn" => style(Inline::Italic),
            "s" | "strike" | "del" => style(Inline::Strike),
            "code" | "tt" | "samp" => style(Inline::Code),
            "kbd" => style(Inline::Kbd),
            "sub" => style(Inline::Sub),
            "sup" => style(Inline::Sup),
            "ins" => style(Inline::Underline),
            _ => None,
        };
        if let Some(elem) = inline {
            self.push_tag(name, elem);
            return;
        }
        match name {
            "a" => {
                if let Some(href) = attr("href").and_then(|h| safe_url(h, true)) {
                    self.push_link(name, href);
                }
                return;
            }
            "br" => return self.line_break(),
            "img" => return self.image(attributes),
            "picture" => {
                return self.push_tag(
                    name,
                    Elem::Picture {
                        dark: None,
                        light: None,
                    },
                );
            }
            "source" => return self.source(attributes),
            _ => {}
        }
        // Markdown の段落の中では、ブロックの要素は読まない（文字は残す）。
        if self.in_markdown_text() {
            return;
        }
        let align = attr("align").map(Align::parse).unwrap_or_default();
        match name {
            "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "dl" | "dt" | "dd"
            | "figure" | "figcaption" | "caption" | "section" | "article" | "header" | "footer"
            | "nav" | "main" | "aside" | "address" => {
                self.begin_block();
                let heading = name
                    .strip_prefix('h')
                    .and_then(|n| n.parse::<u8>().ok())
                    .filter(|n| (1..=6).contains(n));
                self.push_tag(name, Elem::Box { align, heading });
            }
            "hr" => {
                self.begin_block();
                self.add(Block::Rule);
            }
            "details" => {
                self.begin_block();
                self.push_tag(
                    name,
                    Elem::Details {
                        open: attr("open").is_some(),
                        summary: None,
                        blocks: Vec::new(),
                    },
                );
            }
            "summary" => {
                self.begin_block();
                self.push_tag(name, Elem::Summary(Vec::new()));
            }
            "blockquote" => {
                self.begin_block();
                self.push_tag(name, Elem::Quote(Vec::new()));
            }
            "ul" | "ol" => {
                self.begin_block();
                let start = (name == "ol").then(|| {
                    attr("start")
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .unwrap_or(1)
                });
                self.push_tag(name, Elem::List(start, Vec::new()));
            }
            "li" => {
                // 閉じていない前の項目を閉じる。
                if let Some(i) = self.find_tag(
                    |_, e| matches!(e, Elem::Item(_)),
                    |e| matches!(e, Elem::List(..)),
                ) {
                    self.close_from(i);
                }
                self.begin_block();
                let in_list = self
                    .container()
                    .is_some_and(|i| matches!(self.stack[i], Open::Tag(_, Elem::List(..))));
                if in_list {
                    self.push_tag(name, Elem::Item(Vec::new()));
                } else {
                    self.push_tag(
                        name,
                        Elem::Box {
                            align,
                            heading: None,
                        },
                    );
                }
            }
            "table" => {
                self.begin_block();
                self.push_tag(name, Elem::Table(Vec::new()));
            }
            "tr" => {
                if let Some(i) = self.find_tag(
                    |_, e| matches!(e, Elem::Row(..)),
                    |e| matches!(e, Elem::Table(_)),
                ) {
                    self.close_from(i);
                }
                let in_table = self
                    .container()
                    .is_some_and(|i| matches!(self.stack[i], Open::Tag(_, Elem::Table(_))));
                if in_table {
                    self.push_tag(name, Elem::Row(Vec::new(), align));
                }
            }
            "td" | "th" => {
                if let Some(i) = self.find_tag(
                    |_, e| matches!(e, Elem::Cell(_)),
                    |e| matches!(e, Elem::Row(..) | Elem::Table(_)),
                ) {
                    self.close_from(i);
                }
                // `<tr>` を書かずに書いたマス。
                if self
                    .container()
                    .is_some_and(|i| matches!(self.stack[i], Open::Tag(_, Elem::Table(_))))
                {
                    self.push_tag("tr", Elem::Row(Vec::new(), Align::None));
                }
                let Some(i) = self.container() else { return };
                let Open::Tag(_, Elem::Row(_, row_align)) = &self.stack[i] else {
                    return;
                };
                let cell = CellDraft {
                    header: name == "th",
                    rowspan: span(attr("rowspan")),
                    colspan: span(attr("colspan")).min(MAX_COLUMNS),
                    align: if align.is_none() { *row_align } else { align },
                    runs: Vec::new(),
                };
                self.push_tag(name, Elem::Cell(cell));
            }
            "pre" => {
                self.begin_block();
                self.push_tag(
                    name,
                    Elem::Pre {
                        language: attr("lang").unwrap_or_default().trim().to_owned(),
                        text: String::new(),
                    },
                );
            }
            // 知らないタグ（と `<thead>`・`<tbody>` など、形のないもの）は落として、中の文字は残す。
            _ => {}
        }
    }

    fn close_tag(&mut self, name: &str) {
        if self.pre().is_some() && name != "pre" {
            return;
        }
        if name == "br" {
            // `</br>` は `<br>`。
            return self.line_break();
        }
        if let Some(i) = self.find_tag(|n, _| n == name, |_| false) {
            self.close_from(i);
        }
    }

    /// `<img>`: その場所の画像（`<picture>` の中なら、`<source>` で選んだ暗い外観・明るい外観のものも）。
    fn image(&mut self, attributes: &[(String, String)]) {
        let attr = |key: &str| {
            attributes
                .iter()
                .find(|(a, _)| a == key)
                .map(|(_, v)| v.as_str())
        };
        let (dark, light) =
            match self.find_tag(|_, e| matches!(e, Elem::Picture { .. }), |e| e.is_block()) {
                Some(i) => match &self.stack[i] {
                    Open::Tag(_, Elem::Picture { dark, light }) => (dark.clone(), light.clone()),
                    _ => (None, None),
                },
                None => (None, None),
            };
        let Some(src) = attr("src")
            .and_then(|s| safe_url(s, false))
            .or_else(|| light.clone())
            .or_else(|| dark.clone())
        else {
            return;
        };
        let alt = attr("alt").unwrap_or_default().trim().to_owned();
        let width = attr("width").and_then(image_side);
        self.runs.push(Run {
            text: alt,
            image: Some(src),
            image_dark: dark,
            image_light: light,
            width: match width {
                Some(Side::Pixels(w)) => Some(w),
                _ => None,
            },
            height: match attr("height").and_then(image_side) {
                Some(Side::Pixels(h)) => Some(h),
                _ => None,
            },
            width_percent: match width {
                Some(Side::Percent(p)) => Some(p),
                _ => None,
            },
            link: self.style.links.last().cloned(),
            ..Run::default()
        });
    }

    /// `<picture>` の中の `<source>`: `media` が暗い外観・明るい外観に合う最初のものを選ぶ（`media` がなければ
    /// どちらにも合う）。
    fn source(&mut self, attributes: &[(String, String)]) {
        let attr = |key: &str| {
            attributes
                .iter()
                .find(|(a, _)| a == key)
                .map(|(_, v)| v.as_str())
        };
        let Some(src) = attr("srcset")
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| safe_url(s, false))
        else {
            return;
        };
        let media: String = attr("media")
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        let (for_dark, for_light) = if media.is_empty() {
            (true, true)
        } else {
            (
                media.contains("prefers-color-scheme:dark"),
                media.contains("prefers-color-scheme:light"),
            )
        };
        let Some(i) = self.find_tag(|_, e| matches!(e, Elem::Picture { .. }), |e| e.is_block())
        else {
            return;
        };
        if let Open::Tag(_, Elem::Picture { dark, light }) = &mut self.stack[i] {
            if for_dark && dark.is_none() {
                *dark = Some(src.clone());
            }
            if for_light && light.is_none() {
                *light = Some(src);
            }
        }
    }

    /// 残りをすべて閉じる（文書の終わり）。
    fn finish(mut self) -> Vec<Block> {
        while matches!(self.stack.last(), Some(Open::Tag(..))) {
            self.pop_tag();
        }
        self.boundary();
        self.blocks
    }
}

/// `text` の Markdown を、ブロックの並びにする。
pub fn parse(text: &str) -> Vec<Block> {
    parse_with(text, None)
}

/// `parse` の、GitHub の書き方もリンクにするもの。URL をそのまま書いたものはいつでもリンクにし、`github`
/// （`https://github.com/owner/repo`）があれば `#123`・`@user`・コミットの ID もリンクにする。描くのに外へは何も
/// 読みに行かない（リンクを押したときにブラウザで開くだけ）。
pub fn parse_with(text: &str, github: Option<&str>) -> Vec<Block> {
    parse_in_zone(text, github, None)
}

/// `parse_with` の、図（gantt）の日付をこの時間帯の地方時で数えるもの（None は UTC）。
pub fn parse_in_zone(
    text: &str,
    github: Option<&str>,
    zone: Option<std::sync::Arc<dyn crate::diagram::TimeZone>>,
) -> Vec<Block> {
    parse_with_images(text, github, zone, None)
}

/// `parse_in_zone` の、DOT の図の画像（ノードの `image`・表の `<IMG>`）を `images` から読むもの（Markdown の
/// ファイルと同じコミットの中のファイルを、そのファイルのフォルダからの相対パスで）。
pub fn parse_with_images(
    text: &str,
    github: Option<&str>,
    zone: Option<std::sync::Arc<dyn crate::diagram::TimeZone>>,
    images: Option<crate::diagram::ImageSource>,
) -> Vec<Block> {
    let images = images.map(|i| i.bounded());
    let mut blocks = parse_blocks(text, zone, images.as_ref());
    dollars::find(&mut blocks);
    dollars::settle(&mut blocks);
    crate::math::render_all(&mut blocks);
    for block in &mut blocks {
        linkify_block(block, github);
    }
    blocks
}

fn parse_blocks(
    text: &str,
    zone: Option<std::sync::Arc<dyn crate::diagram::TimeZone>>,
    images: Option<&crate::diagram::ImageSource>,
) -> Vec<Block> {
    let options = Options::ENABLE_GFM
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH;
    let mut b = Builder::new();
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    b.boundary();
                    b.stack.push(Open::Paragraph);
                }
                Tag::Heading { level, .. } => {
                    b.boundary();
                    let n = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    b.stack.push(Open::Heading(n));
                }
                Tag::BlockQuote(kind) => {
                    b.boundary();
                    let alert = kind.map(|k| {
                        match k {
                            BlockQuoteKind::Note => "note",
                            BlockQuoteKind::Tip => "tip",
                            BlockQuoteKind::Important => "important",
                            BlockQuoteKind::Warning => "warning",
                            BlockQuoteKind::Caution => "caution",
                        }
                        .to_owned()
                    });
                    b.stack.push(Open::Quote(alert, Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    b.boundary();
                    let language = match kind {
                        CodeBlockKind::Fenced(info) => {
                            info.split_whitespace().next().unwrap_or("").to_owned()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    b.stack.push(Open::Code(language, String::new()));
                }
                Tag::HtmlBlock => {
                    b.boundary();
                    b.html_block = Some(String::new());
                }
                Tag::List(start) => {
                    b.boundary();
                    b.stack.push(Open::List(start, Vec::new()));
                }
                Tag::Item => b.stack.push(Open::Item(None, Vec::new())),
                Tag::FootnoteDefinition(label) => {
                    b.boundary();
                    b.stack.push(Open::Footnote(label.to_string(), Vec::new()));
                }
                Tag::Table(aligns) => {
                    b.boundary();
                    let aligns = aligns
                        .iter()
                        .map(|a| match a {
                            Alignment::None => Align::None,
                            Alignment::Left => Align::Left,
                            Alignment::Center => Align::Center,
                            Alignment::Right => Align::Right,
                        })
                        .collect();
                    b.stack
                        .push(Open::Table(aligns, Vec::new(), Vec::new(), false));
                }
                Tag::TableHead => {
                    if let Some(Open::Table(_, _, _, head)) = b.stack.last_mut() {
                        *head = true;
                    }
                    b.stack.push(Open::Row(Vec::new()));
                }
                Tag::TableRow => b.stack.push(Open::Row(Vec::new())),
                Tag::TableCell => {
                    b.runs.clear();
                    b.stack.push(Open::Cell);
                }
                Tag::Emphasis => b.style.italic += 1,
                Tag::Strong => b.style.bold += 1,
                Tag::Strikethrough => b.style.strike += 1,
                Tag::Link { dest_url, .. } => b.style.links.push(dest_url.to_string()),
                Tag::Image { dest_url, .. } => {
                    b.style.image = Some(dest_url.to_string());
                    // 代わりの文が空でも画像は出す。
                    b.runs.push(Run {
                        image: Some(dest_url.to_string()),
                        link: b.style.links.last().cloned(),
                        ..Run::default()
                    });
                }
                _ => {}
            },
            Event::End(end) => {
                // ブロックが閉じるときは、その中で開いたままの HTML の要素も閉じる（HTML のブロックは、Markdown を
                // またいで次の HTML のブロックで閉じるので閉じない）。
                if !matches!(
                    end,
                    TagEnd::HtmlBlock
                        | TagEnd::Emphasis
                        | TagEnd::Strong
                        | TagEnd::Strikethrough
                        | TagEnd::Link
                        | TagEnd::Image
                ) {
                    b.close_tags();
                }
                match end {
                    TagEnd::Paragraph => {
                        b.stack.pop();
                        let runs = tidy(std::mem::take(&mut b.runs));
                        if !runs.is_empty() {
                            b.add(Block::Paragraph {
                                runs,
                                align: Align::None,
                            });
                        }
                    }
                    TagEnd::Heading(_) => {
                        if let Some(Open::Heading(level)) = b.stack.pop() {
                            let runs = std::mem::take(&mut b.runs);
                            b.add(Block::Heading {
                                level,
                                runs,
                                align: Align::None,
                            });
                        }
                    }
                    TagEnd::BlockQuote(_) => {
                        b.boundary();
                        if let Some(Open::Quote(alert, blocks)) = b.stack.pop() {
                            b.add(Block::Quote { alert, blocks });
                        }
                    }
                    TagEnd::CodeBlock => {
                        if let Some(Open::Code(language, mut text)) = b.stack.pop() {
                            if text.ends_with('\n') {
                                text.pop();
                            }
                            if language.eq_ignore_ascii_case("math") {
                                b.add(Block::Math {
                                    text,
                                    math: None,
                                    error: None,
                                });
                                continue;
                            }
                            let block = match crate::diagram::draw_with(
                                &language,
                                &text,
                                zone.clone(),
                                images,
                            ) {
                                None => Block::Code { language, text },
                                Some(crate::diagram::Drawn::Svg(svg, config)) => Block::Diagram {
                                    language,
                                    text,
                                    svg: Some(svg),
                                    config,
                                    note: None,
                                    error: None,
                                },
                                Some(crate::diagram::Drawn::Noted(svg, note)) => Block::Diagram {
                                    language,
                                    text,
                                    svg: Some(svg),
                                    config: None,
                                    note: Some(note),
                                    error: None,
                                },
                                Some(crate::diagram::Drawn::Error(error)) => Block::Diagram {
                                    language,
                                    text,
                                    svg: None,
                                    config: None,
                                    note: None,
                                    error: Some(error),
                                },
                            };
                            b.add(block);
                        }
                    }
                    TagEnd::HtmlBlock => {
                        if let Some(html) = b.html_block.take() {
                            b.html(&html);
                        }
                        b.tokenizer.leave_raw();
                    }
                    TagEnd::Item => {
                        b.boundary();
                        if let Some(Open::Item(task, blocks)) = b.stack.pop()
                            && let Some(Open::List(_, items)) = b.stack.last_mut()
                        {
                            items.push(Item { task, blocks });
                        }
                    }
                    TagEnd::List(_) => {
                        if let Some(Open::List(start, items)) = b.stack.pop() {
                            b.add(Block::List { start, items });
                        }
                    }
                    TagEnd::FootnoteDefinition => {
                        b.boundary();
                        if let Some(Open::Footnote(label, blocks)) = b.stack.pop() {
                            b.add(Block::Footnote { label, blocks });
                        }
                    }
                    TagEnd::TableCell => {
                        b.stack.pop();
                        let runs = std::mem::take(&mut b.runs);
                        if let Some(Open::Row(cells)) = b.stack.last_mut() {
                            cells.push(runs);
                        }
                    }
                    TagEnd::TableHead | TagEnd::TableRow => {
                        if let Some(Open::Row(cells)) = b.stack.pop()
                            && let Some(Open::Table(_, head, rows, in_head)) = b.stack.last_mut()
                        {
                            if *in_head {
                                *head = cells;
                                *in_head = false;
                            } else {
                                rows.push(cells);
                            }
                        }
                    }
                    TagEnd::Table => {
                        if let Some(Open::Table(aligns, head, rows, _)) = b.stack.pop() {
                            b.add(Block::Table { aligns, head, rows });
                        }
                    }
                    TagEnd::Emphasis => b.style.italic = b.style.italic.saturating_sub(1),
                    TagEnd::Strong => b.style.bold = b.style.bold.saturating_sub(1),
                    TagEnd::Strikethrough => b.style.strike = b.style.strike.saturating_sub(1),
                    TagEnd::Link => {
                        b.style.links.pop();
                    }
                    TagEnd::Image => b.style.image = None,
                    _ => {}
                }
            }
            Event::Text(text) => {
                if let Some(html) = &mut b.html_block {
                    html.push_str(&text);
                } else if let Some(Open::Code(_, code)) = b.stack.last_mut() {
                    code.push_str(&text);
                } else if let Some(pre) = b.pre() {
                    pre.push_str(&text);
                } else if !b.tokenizer.in_raw() {
                    b.push_run(&text, false);
                }
            }
            Event::Code(text) => {
                if !b.tokenizer.in_raw() {
                    b.push_run(&text, true);
                }
            }
            Event::InlineMath(tex) | Event::DisplayMath(tex) => {
                let raw = text.get(range.clone()).unwrap_or_default();
                if let Some(html) = &mut b.html_block {
                    html.push_str(raw);
                } else if let Some(pre) = b.pre() {
                    pre.push_str(raw);
                } else if b.tokenizer.in_raw() {
                    // 読まない HTML の要素（`<script>` など）の中
                } else if raw.starts_with("$$")
                    && dollars::opens_after(text[..range.start].chars().next_back())
                    && dollars::closes_before(text[range.end..].chars().next())
                    && b.display_math(tex.trim())
                {
                    // 段落の中の `$$…$$` はブロック
                } else {
                    // 文中の式は、段落ができてから GitHub と同じ決まりで選ぶ（`dollars`）
                    b.math_source(raw, &tex);
                }
            }
            Event::Html(text) => match &mut b.html_block {
                Some(html) => html.push_str(&text),
                None => b.html(&text),
            },
            Event::InlineHtml(text) => b.html(&text),
            Event::SoftBreak => {
                if !b.tokenizer.in_raw() {
                    // `dollars` が行の境を見分ける。式を選んだ後で空白にする（`dollars::settle`）
                    b.push_run("\n", false);
                }
            }
            Event::HardBreak => b.line_break(),
            Event::Rule => {
                b.boundary();
                b.add(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                if let Some(Open::Item(task, _)) = b.stack.last_mut() {
                    *task = Some(done);
                }
            }
            Event::FootnoteReference(label) => b.runs.push(Run {
                text: label.to_string(),
                footnote: Some(label.to_string()),
                ..Run::default()
            }),
        }
    }
    b.finish()
}

/// GitHub の `` $`…`$ `` の中身（コードの区切りと同じく、両端の同じ数の `` ` `` を除き、両端が空白なら 1 つずつ除く）。
/// そうでなければそのまま。
fn code_span(tex: &str) -> &str {
    let ticks = tex.len() - tex.trim_start_matches('`').len();
    if ticks == 0 || tex.len() < ticks * 2 + 1 {
        return tex;
    }
    let tail = tex.len() - tex.trim_end_matches('`').len();
    if tail != ticks {
        return tex;
    }
    let inner = &tex[ticks..tex.len() - ticks];
    match inner.strip_prefix(' ').and_then(|i| i.strip_suffix(' ')) {
        Some(trimmed) if !trimmed.trim().is_empty() => trimmed,
        _ => inner,
    }
}

/// `parse_with` の結果の JSON（FE 向け）。
pub fn parse_json(text: &str, github: Option<&str>) -> String {
    parse_json_in_zone(text, github, None)
}

/// `parse_in_zone` の結果の JSON（FE 向け）。
pub fn parse_json_in_zone(
    text: &str,
    github: Option<&str>,
    zone: Option<std::sync::Arc<dyn crate::diagram::TimeZone>>,
) -> String {
    parse_json_with_images(text, github, zone, None)
}

/// `parse_with_images` の結果の JSON（FE 向け）。
pub fn parse_json_with_images(
    text: &str,
    github: Option<&str>,
    zone: Option<std::sync::Arc<dyn crate::diagram::TimeZone>>,
    images: Option<crate::diagram::ImageSource>,
) -> String {
    serde_json::to_string(&parse_with_images(text, github, zone, images))
        .unwrap_or_else(|_| "[]".to_owned())
}

fn linkify_block(block: &mut Block, github: Option<&str>) {
    match block {
        Block::Heading { runs, .. } | Block::Paragraph { runs, .. } => {
            *runs = linkify(std::mem::take(runs), github)
        }
        Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
            blocks.iter_mut().for_each(|b| linkify_block(b, github))
        }
        Block::Details {
            summary, blocks, ..
        } => {
            *summary = linkify(std::mem::take(summary), github);
            blocks.iter_mut().for_each(|b| linkify_block(b, github))
        }
        Block::HtmlTable { cells, .. } => {
            for cell in cells {
                cell.runs = linkify(std::mem::take(&mut cell.runs), github);
            }
        }
        Block::List { items, .. } => {
            for item in items {
                item.blocks
                    .iter_mut()
                    .for_each(|b| linkify_block(b, github));
            }
        }
        Block::Table { head, rows, .. } => {
            for cell in head.iter_mut().chain(rows.iter_mut().flatten()) {
                *cell = linkify(std::mem::take(cell), github);
            }
        }
        _ => {}
    }
}

/// リンクでない地の文の中の URL（と GitHub の参照）をリンクの文字に分ける。
fn linkify(runs: Vec<Run>, github: Option<&str>) -> Vec<Run> {
    chip_custom_emoji(linkify_text(runs, github))
}

/// GitHub 独自の絵文字の名前（画像だけで Unicode にないもの。GitHub の絵文字の一覧のうち、gemoji にないもの）。
const CUSTOM_EMOJI: [&str; 20] = [
    "atom",
    "basecamp",
    "basecampy",
    "bowtie",
    "electron",
    "feelsgood",
    "finnadie",
    "fishsticks",
    "goberserk",
    "godmode",
    "hurtrealbad",
    "neckbeard",
    "octocat",
    "rage1",
    "rage2",
    "rage3",
    "rage4",
    "shipit",
    "suspect",
    "trollface",
];

/// 地の文の `:octocat:` などを、札で見せる文字に分ける（画像は持たないので名前のまま）。
fn chip_custom_emoji(runs: Vec<Run>) -> Vec<Run> {
    let mut out = Vec::new();
    for run in runs {
        if run.link.is_some()
            || run.code
            || run.kbd
            || run.image.is_some()
            || run.footnote.is_some()
            || !run.text.contains(':')
        {
            out.push(run);
            continue;
        }
        let text = run.text.clone();
        let mut last = 0;
        let mut i = 0;
        while let Some(p) = text[i..].find(':') {
            let start = i + p;
            let after = &text[start + 1..];
            let name_end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            let name = &after[..name_end];
            if name_end > 0 && after[name_end..].starts_with(':') && CUSTOM_EMOJI.contains(&name) {
                if start > last {
                    out.push(Run {
                        text: text[last..start].to_owned(),
                        ..run.clone()
                    });
                }
                let end = start + name_end + 2;
                out.push(Run {
                    text: text[start..end].to_owned(),
                    custom_emoji: Some(name.to_owned()),
                    ..run.clone()
                });
                last = end;
                i = end;
            } else {
                i = start + 1;
            }
        }
        if last < text.len() {
            out.push(Run {
                text: text[last..].to_owned(),
                ..run
            });
        }
    }
    out
}

fn linkify_text(runs: Vec<Run>, github: Option<&str>) -> Vec<Run> {
    let mut out = Vec::new();
    for run in runs {
        if run.link.is_some()
            || run.code
            || run.kbd
            || run.image.is_some()
            || run.footnote.is_some()
            || run.line_break
        {
            out.push(run);
            continue;
        }
        let text = emojify(&run.text);
        let mut last = 0;
        for (start, end, url) in find_links(&text, github, !run.html) {
            if start > last {
                out.push(Run {
                    text: text[last..start].to_owned(),
                    ..run.clone()
                });
            }
            out.push(Run {
                text: text[start..end].to_owned(),
                link: Some(url),
                ..run.clone()
            });
            last = end;
        }
        if last < text.len() {
            out.push(Run {
                text: text[last..].to_owned(),
                ..run
            });
        }
    }
    out
}

/// GitHub の絵文字の名前（`:smile:`）を絵文字にする。知らない名前はそのまま。
fn emojify(text: &str) -> String {
    if !text.contains(':') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let name_end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-')))
            .unwrap_or(after.len());
        if name_end > 0
            && after[name_end..].starts_with(':')
            && let Some(emoji) = crate::emoji::lookup(&after[..name_end])
        {
            out.push_str(emoji);
            rest = &after[name_end + 1..];
        } else {
            out.push(':');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// 文の中のリンクにする所（始まり・終わり・リンク先）。`urls` が false なら素の URL はリンクにしない（HTML の中の
/// 文字。GitHub の URL の自動リンクは Markdown の文の中だけで、`#123`・`@user` などは HTML の中でもリンクにする）。
fn find_links(text: &str, github: Option<&str>, urls: bool) -> Vec<(usize, usize, String)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    // 語の始まり（行頭・空白・括弧の後）か。
    let at_word =
        |i: usize| i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'(' | b'[' | b',');
    let word_end = |from: usize, ok: fn(u8) -> bool| {
        let mut j = from;
        while j < bytes.len() && ok(bytes[j]) {
            j += 1;
        }
        j
    };
    while i < bytes.len() {
        let rest = &text[i..];
        if urls
            && at_word(i)
            && ["https://", "http://", "www."]
                .iter()
                .any(|p| rest.starts_with(p))
        {
            let mut end = i + rest
                .find(|c: char| c.is_whitespace() || c == '<')
                .unwrap_or(rest.len());
            // 文の終わりの句読点と、対になっていない閉じ括弧は含めない。
            while end > i {
                let c = text[..end].chars().last().unwrap_or(' ');
                let opens = text[i..end].matches('(').count();
                let closes = text[i..end].matches(')').count();
                if matches!(
                    c,
                    '.' | ',' | ':' | ';' | '!' | '?' | '"' | '\'' | '*' | '_' | '~'
                ) || (c == ')' && closes > opens)
                {
                    end -= c.len_utf8();
                } else {
                    break;
                }
            }
            let url = &text[i..end];
            if end > i + 4 {
                let href = if url.starts_with("www.") {
                    format!("https://{url}")
                } else {
                    url.to_owned()
                };
                found.push((i, end, href));
                i = end;
                continue;
            }
        }
        if let Some(base) = github
            && at_word(i)
        {
            if bytes[i] == b'#' {
                let end = word_end(i + 1, |b| b.is_ascii_digit());
                if end > i + 1 && (end == bytes.len() || !bytes[end].is_ascii_alphanumeric()) {
                    found.push((i, end, format!("{base}/issues/{}", &text[i + 1..end])));
                    i = end;
                    continue;
                }
            }
            if bytes[i] == b'@' {
                let end = word_end(i + 1, |b| b.is_ascii_alphanumeric() || b == b'-');
                // メールアドレスの一部（`@` の後に `.` が続く語）は除く。
                let email = end < bytes.len()
                    && bytes[end] == b'.'
                    && bytes.get(end + 1).is_some_and(u8::is_ascii_alphanumeric);
                if end > i + 1 && !email {
                    let host = base.rsplitn(3, '/').nth(2).unwrap_or("https://github.com");
                    found.push((i, end, format!("{host}/{}", &text[i + 1..end])));
                    i = end;
                    continue;
                }
            }
            if bytes[i].is_ascii_hexdigit() {
                let end = word_end(i, |b| b.is_ascii_hexdigit());
                let word = &text[i..end];
                if (7..=40).contains(&word.len())
                    && word.bytes().any(|b| b.is_ascii_digit())
                    && word.bytes().any(|b| b.is_ascii_alphabetic())
                    && (end == bytes.len() || !bytes[end].is_ascii_alphanumeric())
                {
                    found.push((i, end, format!("{base}/commit/{word}")));
                    i = end;
                    continue;
                }
            }
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// Markdown として整形して見せるファイルか（拡張子で）。
pub fn is_markdown(path: &str) -> bool {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "md" | "markdown" | "mdown" | "mkd" | "mkdn")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn reads_headings_paragraphs_and_styles() {
        let blocks =
            parse("# Title\n\nSome **bold** and *it* and `code` and ~~gone~~ [link](https://x).\n");
        assert_eq!(blocks.len(), 2);
        let Block::Heading { level, runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!((*level, text(runs).as_str()), (1, "Title"));
        let Block::Paragraph { runs, .. } = &blocks[1] else {
            panic!()
        };
        assert!(runs.iter().any(|r| r.bold && r.text == "bold"));
        assert!(runs.iter().any(|r| r.italic && r.text == "it"));
        assert!(runs.iter().any(|r| r.code && r.text == "code"));
        assert!(runs.iter().any(|r| r.strike && r.text == "gone"));
        assert!(
            runs.iter()
                .any(|r| r.link.as_deref() == Some("https://x") && r.text == "link")
        );
    }

    #[test]
    fn reads_lists_tasks_quotes_code_and_tables() {
        let md = "- [x] done\n- [ ] todo\n  1. nested\n\n> quoted\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|:-|-:|\n| 1 | **2** |\n\n---\n<div>raw</div>\n";
        let blocks = parse(md);
        let Block::List { start: None, items } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(items[0].task, Some(true));
        assert_eq!(items[1].task, Some(false));
        assert!(matches!(
            items[1].blocks.last(),
            Some(Block::List { start: Some(1), .. })
        ));
        assert!(
            matches!(&blocks[1], Block::Quote { alert: None, blocks } if matches!(&blocks[0], Block::Paragraph { .. }))
        );
        assert_eq!(
            blocks[2],
            Block::Code {
                language: "rust".into(),
                text: "fn main() {}".into()
            }
        );
        let Block::Table { aligns, head, rows } = &blocks[3] else {
            panic!("{blocks:?}")
        };
        assert_eq!(aligns, &vec![Align::Left, Align::Right]);
        assert_eq!(text(&head[1]), "b");
        assert!(rows[0][1][0].bold);
        assert_eq!(blocks[4], Block::Rule);
        assert!(matches!(&blocks[5], Block::Paragraph { runs, .. } if text(runs) == "raw"));
    }

    #[test]
    fn reads_images_and_footnotes() {
        let blocks = parse("![a logo](img/logo.png) text[^1]\n\n[^1]: The note.\n");
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(runs[0].image.as_deref(), Some("img/logo.png"));
        assert_eq!(runs[0].text, "a logo");
        assert!(runs.iter().any(|r| r.footnote.as_deref() == Some("1")));
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, Block::Footnote { label, .. } if label == "1"))
        );
        assert!(parse_json("# x", None).contains("\"kind\":\"heading\""));
        assert!(is_markdown("docs/README.MD") && !is_markdown("a.txt"));
    }

    #[test]
    fn the_json_carries_inline_math_tex_for_front_ends_that_draw_it() {
        // 描かない作り（エンジン）でも、文中の式の TeX を書き出す（VSCode 拡張が `amonhen-render` で描く）
        let json: serde_json::Value =
            serde_json::from_str(&parse_json("# Sum $$x$$\n\nArea $\\pi r^2$ here.\n", None))
                .unwrap();
        let math: Vec<_> = [&json[0]["runs"], &json[1]["runs"]]
            .into_iter()
            .flat_map(|runs| runs.as_array().unwrap())
            .filter(|r| r["math_tex"].is_string())
            .collect();
        assert_eq!(math.len(), 2, "{json}");
        assert_eq!(math[0]["math_tex"], "x");
        assert_eq!(math[0]["math_display"], true, "{json}");
        assert_eq!(math[1]["math_tex"], "\\pi r^2");
        assert_eq!(math[1]["text"], "$\\pi r^2$");
        assert!(math[1].get("math_display").is_none(), "{json}");
    }

    #[test]
    fn reads_github_alerts_and_links() {
        let blocks = parse_with(
            "> [!WARNING]\n> Careful.\n\nSee https://example.com/a_(b). and www.x.org, fixes #12 by @dev in 1a2b3c4d, mail a@b.co.\n",
            Some("https://github.com/o/r"),
        );
        assert!(matches!(&blocks[0], Block::Quote { alert: Some(a), .. } if a == "warning"));
        let Block::Paragraph { runs, .. } = &blocks[1] else {
            panic!("{blocks:?}")
        };
        let links: Vec<(&str, &str)> = runs
            .iter()
            .filter_map(|r| Some((r.text.as_str(), r.link.as_deref()?)))
            .collect();
        assert_eq!(
            links,
            vec![
                ("https://example.com/a_(b)", "https://example.com/a_(b)"),
                ("www.x.org", "https://www.x.org"),
                ("#12", "https://github.com/o/r/issues/12"),
                ("@dev", "https://github.com/dev"),
                ("1a2b3c4d", "https://github.com/o/r/commit/1a2b3c4d"),
            ]
        );
        assert_eq!(
            emojify("ship it :rocket: :+1: at 10:30 :nope:"),
            "ship it 🚀 👍 at 10:30 :nope:"
        );
        // GitHub の remote がなければ、URL だけ。
        let plain = parse("fixes #12 at https://a.b\n");
        let Block::Paragraph { runs, .. } = &plain[0] else {
            panic!()
        };
        assert_eq!(runs.iter().filter(|r| r.link.is_some()).count(), 1);
    }

    #[test]
    fn github_custom_emoji_become_chips() {
        let blocks = parse("Ship it :shipit: with :octocat: and :nope: :smile:\n");
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        let chips: Vec<&str> = runs
            .iter()
            .filter_map(|r| r.custom_emoji.as_deref())
            .collect();
        assert_eq!(chips, vec!["shipit", "octocat"]);
        assert!(
            runs.iter()
                .any(|r| r.custom_emoji.is_some() && r.text == ":octocat:")
        );
        assert!(
            runs.iter()
                .any(|r| r.text.contains(":nope:") && r.text.contains("😄"))
        );
    }

    #[test]
    fn bare_urls_in_html_stay_text() {
        // GitHub と同じく、URL の自動リンクは Markdown の文の中だけ（`#12`・`@dev` は HTML の中でもリンク）。
        let blocks = parse_with(
            "<p>See https://a.org and www.b.org, #12 by @dev</p>\n\n\
             <div>\n\nMarkdown https://c.org\n\n</div>\n\n\
             Text <b>https://d.org</b> and <span>https://e.org</span>\n",
            Some("https://github.com/o/r"),
        );
        let links = |block: &Block| -> Vec<String> {
            paragraph(block)
                .0
                .iter()
                .filter_map(|r| r.link.clone())
                .collect()
        };
        assert_eq!(
            links(&blocks[0]),
            vec![
                "https://github.com/o/r/issues/12".to_owned(),
                "https://github.com/dev".to_owned(),
            ]
        );
        assert_eq!(
            text(paragraph(&blocks[0]).0),
            "See https://a.org and www.b.org, #12 by @dev"
        );
        // HTML の中でも、空行を挟んで Markdown として読む所はリンクにする。
        assert_eq!(links(&blocks[1]), vec!["https://c.org".to_owned()]);
        // Markdown の段落の中のタグの中身は Markdown の文。
        assert_eq!(
            links(&blocks[2]),
            vec!["https://d.org".to_owned(), "https://e.org".to_owned()]
        );
    }

    #[test]
    fn dropped_comments_leave_one_space() {
        let blocks = parse(
            "a <!-- x --> b <!-- y -->\nc <b>d</b> <!-- z --> `e  f`\n\n<p>g <!-- w --> h</p>\n",
        );
        assert_eq!(text(paragraph(&blocks[0]).0), "a b c d e  f");
        assert_eq!(text(paragraph(&blocks[1]).0), "g h");
    }

    fn paragraph(block: &Block) -> (&[Run], Align) {
        match block {
            Block::Paragraph { runs, align } => (runs, *align),
            _ => panic!("{block:?}"),
        }
    }

    #[test]
    fn html_inline_tags_become_styles() {
        let blocks = parse(
            "Press <kbd>Ctrl</kbd>+<KBD>C</KBD>, H<sub>2</sub>O, x<sup>2</sup>, <ins>new</ins> \
             <del>old</del> <s>gone</s> <b>b</b> <strong>s</strong> <i>i</i> <em>e</em> \
             <code>c</code> <span class=x style=\"color:red\">plain</span><br>next\n",
        );
        let (runs, _) = paragraph(&blocks[0]);
        let find = |t: &str| {
            runs.iter()
                .find(|r| r.text == t)
                .unwrap_or_else(|| panic!("{t}: {runs:?}"))
        };
        assert!(find("Ctrl").kbd && find("C").kbd);
        let keys = parse("<kbd>A</kbd><kbd>B</kbd> <kbd>Ctrl&amp;</kbd>");
        let keys: Vec<&str> = paragraph(&keys[0])
            .0
            .iter()
            .filter(|r| r.kbd)
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(keys, ["A", "B", "Ctrl&"]);
        assert!(find("2").sub);
        assert!(runs.iter().any(|r| r.sup && r.text == "2"));
        assert!(find("new").underline);
        assert!(find("old").strike && find("gone").strike);
        assert!(find("b").bold && find("s").bold);
        assert!(find("i").italic && find("e").italic);
        assert!(find("c").code);
        // 知らないタグは落として、文字は残す（style・class は描かない）。
        assert!(
            runs.iter()
                .any(|r| r.text.contains("plain") && !r.bold && !r.code)
        );
        assert!(runs.iter().any(|r| r.line_break));
        assert!(!text(runs).contains('<'));
    }

    #[test]
    fn html_links_and_images() {
        let blocks = parse(
            "<a href=\"https://x.org\">site</a> <a href=\"javascript:alert(1)\">bad</a> \
             <a href=\"java\tscript:x\">tab</a> <a href=\"docs/a.md\">rel</a>\n\n\
             <img src=\"img/logo.png\" width=\"120\" height=\"40px\" alt=\"Logo\">\n\
             <img src=\"javascript:x\" alt=\"no\"><img src=a.png width=50% height=20%>\n\n\
             <img src=b.png width=\" 33.4%\" height=\"12.5em\"><img src=c.png width=250% height=x>\n",
        );
        let (runs, _) = paragraph(&blocks[0]);
        let link = |t: &str| {
            runs.iter()
                .find(|r| r.text == t)
                .and_then(|r| r.link.clone())
        };
        assert_eq!(link("site").as_deref(), Some("https://x.org"));
        assert_eq!(link("bad"), None);
        assert_eq!(link("tab"), None);
        assert_eq!(link("rel").as_deref(), Some("docs/a.md"));
        let images: Vec<&Run> = blocks
            .iter()
            .flat_map(|b| paragraph(b).0)
            .filter(|r| r.image.is_some())
            .collect();
        assert_eq!(images.len(), 4, "{blocks:?}");
        assert_eq!(images[0].image.as_deref(), Some("img/logo.png"));
        assert_eq!((images[0].width, images[0].height), (Some(120), Some(40)));
        assert_eq!(images[0].text, "Logo");
        // 割合の幅は割合のまま（高さの割合は読まない）。数字の後ろの単位は画素として読む（HTML の読み方）。
        let sides = |r: &Run| (r.width, r.height, r.width_percent);
        assert_eq!(images[1].image.as_deref(), Some("a.png"));
        assert_eq!(sides(images[1]), (None, None, Some(50)));
        assert_eq!(sides(images[2]), (None, Some(13), Some(33)));
        assert_eq!(sides(images[3]), (None, None, Some(100)));
    }

    #[test]
    fn centered_logo_and_badges() {
        let blocks = parse(
            "<p align=\"center\">\n  <img src=\"logo.png\" width=\"200\">\n</p>\n\n\
             <p align=\"center\">\n  <a href=\"https://ci\"><img src=\"ci.svg\" alt=\"CI\"></a>\n  \
             <img src=\"v.svg\" alt=\"v\">\n</p>\n\n<h1 align=\"right\">Title</h1>\n\n\
             <div align=\"center\">\n\n# Centered\n\nText **here**.\n\n</div>\n\nAfter\n",
        );
        let (logo, align) = paragraph(&blocks[0]);
        assert_eq!(align, Align::Center);
        assert_eq!(logo.len(), 1);
        assert_eq!(logo[0].width, Some(200));
        let (badges, align) = paragraph(&blocks[1]);
        assert_eq!(align, Align::Center);
        let images: Vec<_> = badges.iter().filter(|r| r.image.is_some()).collect();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].link.as_deref(), Some("https://ci"));
        assert_eq!(images[1].link, None);
        // 絵の間の空白は 1 つ。
        assert_eq!(badges.len(), 3, "{badges:?}");
        assert_eq!(badges[1].text, " ");
        assert!(
            matches!(&blocks[2], Block::Heading { level: 1, align: Align::Right, runs } if text(runs) == "Title")
        );
        assert!(matches!(
            &blocks[3],
            Block::Heading {
                level: 1,
                align: Align::Center,
                ..
            }
        ));
        let (here, align) = paragraph(&blocks[4]);
        assert_eq!(align, Align::Center);
        assert!(here.iter().any(|r| r.bold && r.text == "here"));
        assert_eq!(paragraph(&blocks[5]).1, Align::None);
        assert_eq!(blocks.len(), 6, "{blocks:?}");
    }

    #[test]
    fn details_hold_markdown() {
        let blocks = parse(
            "<details>\n<summary>More <b>info</b></summary>\n\n**bold** and `code`\n\n- a\n- b\n\n</details>\n\n\
             <details open><summary>Open</summary>\nplain text\n</details>\n\n<details>\n\nNo summary\n\n</details>\n",
        );
        let Block::Details {
            summary,
            open,
            blocks: inner,
        } = &blocks[0]
        else {
            panic!("{blocks:?}")
        };
        assert!(!open);
        assert_eq!(text(summary), "More info");
        assert!(summary.iter().any(|r| r.bold && r.text == "info"));
        assert!(
            paragraph(&inner[0])
                .0
                .iter()
                .any(|r| r.bold && r.text == "bold")
        );
        assert!(matches!(&inner[1], Block::List { items, .. } if items.len() == 2));
        let Block::Details {
            summary,
            open,
            blocks: inner,
        } = &blocks[1]
        else {
            panic!("{blocks:?}")
        };
        assert!(open);
        assert_eq!(text(summary), "Open");
        assert_eq!(text(paragraph(&inner[0]).0), "plain text");
        assert!(matches!(&blocks[2], Block::Details { summary, .. } if summary.is_empty()));
        assert_eq!(blocks.len(), 3);
    }

    #[test]
    fn nested_details_and_details_in_lists() {
        let blocks = parse(
            "<details><summary>Outer</summary>\n\n<details><summary>Inner</summary>\n\nDeep\n\n</details>\n\n</details>\n\n\
             - item\n\n  <details><summary>In item</summary>\n\n  Hidden\n\n  </details>\n",
        );
        let Block::Details { blocks: outer, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert!(
            matches!(&outer[0], Block::Details { summary, blocks, .. } if text(summary) == "Inner" && blocks.len() == 1)
        );
        let Block::List { items, .. } = &blocks[1] else {
            panic!("{blocks:?}")
        };
        assert!(
            items[0]
                .blocks
                .iter()
                .any(|b| matches!(b, Block::Details { summary, .. } if text(summary) == "In item"))
        );
    }

    #[test]
    fn html_tables_with_spans() {
        let blocks = parse(
            "<table>\n<thead><tr><th colspan=\"2\">Wide</th><th>C</th></tr></thead>\n\
             <tr><td rowspan=2 align=right>Tall</td><td>b</td><td>c</td></tr>\n\
             <tr align=\"center\"><td>e<br>f</td><td><b>g</b></td></tr>\n\
             <tr><td>h\n<td>i</table>\n",
        );
        let Block::HtmlTable {
            columns,
            rows,
            cells,
        } = &blocks[0]
        else {
            panic!("{blocks:?}")
        };
        assert_eq!((*columns, *rows), (3, 4));
        let at = |r: u32, c: u32| {
            cells
                .iter()
                .find(|x| x.row == r && x.column == c)
                .unwrap_or_else(|| panic!("{r},{c}: {cells:?}"))
        };
        assert!(at(0, 0).header && at(0, 0).colspan == 2 && text(&at(0, 0).runs) == "Wide");
        assert_eq!(text(&at(0, 2).runs), "C");
        assert_eq!((at(1, 0).rowspan, at(1, 0).align), (2, Align::Right));
        assert_eq!(text(&at(2, 1).runs), "ef");
        assert!(at(2, 1).runs.iter().any(|r| r.line_break));
        assert_eq!(at(2, 1).align, Align::Center);
        assert!(at(2, 2).runs[0].bold);
        // 閉じていないマス・行も読み、足りない所は空のマスで埋める。
        assert_eq!(text(&at(3, 1).runs), "i");
        assert!(at(3, 2).runs.is_empty());
        // 覆われたマスはない。
        assert!(!cells.iter().any(|x| x.row == 0 && x.column == 1));
        assert!(!cells.iter().any(|x| x.row == 2 && x.column == 0));
        assert_eq!(cells.len(), 2 + 3 + 2 + 3);
    }

    #[test]
    fn picture_sources_by_appearance() {
        let blocks = parse(
            "<picture>\n  <source media=\"(prefers-color-scheme: dark)\" srcset=\"dark.png 2x, d1.png 1x\">\n  \
             <source media=\"(prefers-color-scheme:light)\" srcset=\"light.png\">\n  \
             <img alt=\"Logo\" src=\"fallback.png\">\n</picture>\n\n\
             <picture><source srcset=\"any.webp\"><img src=\"a.png\"></picture>\n\n\
             <picture><source media=\"(prefers-color-scheme: dark)\" srcset=\"d.png\"><img src=\"l.png\"></picture>\n",
        );
        let image = |i: usize| {
            paragraph(&blocks[i])
                .0
                .iter()
                .find(|r| r.image.is_some())
                .cloned()
                .unwrap_or_else(|| panic!("{blocks:?}"))
        };
        let a = image(0);
        assert_eq!(
            (
                a.image.as_deref(),
                a.image_dark.as_deref(),
                a.image_light.as_deref()
            ),
            (Some("fallback.png"), Some("dark.png"), Some("light.png"))
        );
        assert_eq!(a.text, "Logo");
        let b = image(1);
        assert_eq!(
            (b.image_dark.as_deref(), b.image_light.as_deref()),
            (Some("any.webp"), Some("any.webp"))
        );
        let c = image(2);
        assert_eq!(
            (c.image_dark.as_deref(), c.image_light),
            (Some("d.png"), None)
        );
    }

    #[test]
    fn sanitizes_like_github() {
        let blocks = parse(
            "Before\n\n<script>alert('x')</script>\n\n<style>\np { color: red }\n</style>\n\n\
             <!-- a comment -->\n\nInline <script>bad()</script>ok <!-- hidden --> end \
             <iframe src=\"https://x\">frame</iframe>\n\n\
             <form><input value=x><button onclick=\"x()\">Go</button></form>\n\n<p onclick=\"x()\" style=\"x\">P</p>\n",
        );
        let all: String = blocks.iter().map(|b| text(paragraph(b).0) + "|").collect();
        // コメントを落とした前後の空白は、ブラウザと同じく 1 つ。
        assert_eq!(all, "Before|Inline ok end|Go|P|");
    }

    #[test]
    fn html_lists_quotes_pre_and_rules() {
        let blocks = parse(
            "<ol start=\"3\"><li>three<li>four <ul><li>nested</ul></ol>\n\n\
             <blockquote>quoted</blockquote>\n\n<pre lang=\"rust\">\nfn  main() {\n    <b>x</b> &lt;\n}\n</pre>\n\n<hr>\n\n<p>a<p>b\n",
        );
        let Block::List { start, items } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!((*start, items.len()), (Some(3), 2));
        assert_eq!(text(paragraph(&items[0].blocks[0]).0), "three");
        assert!(
            matches!(items[1].blocks.last(), Some(Block::List { start: None, items }) if items.len() == 1)
        );
        assert!(
            matches!(&blocks[1], Block::Quote { alert: None, blocks } if text(paragraph(&blocks[0]).0) == "quoted")
        );
        assert_eq!(
            blocks[2],
            Block::Code {
                language: "rust".into(),
                text: "fn  main() {\n    x <\n}".into()
            }
        );
        assert_eq!(blocks[3], Block::Rule);
        assert_eq!(text(paragraph(&blocks[4]).0), "a");
        assert_eq!(text(paragraph(&blocks[5]).0), "b");
    }

    #[test]
    fn broken_html_never_panics_and_is_bounded() {
        let inputs = [
            "<div><b><i>x</div></b></i>",
            "</div></details></table></td>",
            "<table><td>a<table><tr><td>b</table></td></table>",
            "<td colspan=99999 rowspan=0>x</td>",
            "<table><tr><td colspan=100000 rowspan=100000>x</td></tr><tr><td>y</td></tr></table>",
            "<details><summary><h2>T</h2></summary>body",
            "<summary>alone</summary>",
            "<li>stray</li><tr><td>x</td></tr>",
            "<pre>unclosed",
            "<picture><source srcset=x>",
            "<img>",
            "<a href=x><a href=y>z</a>",
            "- <div>\n\n  x\n- y </div>\n",
            "> <details>\n> a\n\nb </details>",
            "| a |\n|---|\n| <div>x</div> <kbd>k |\n",
            "<sub><sub><sub>",
            "<",
            "<!--",
            "&#xFFFFFFFF; &#; &",
        ];
        for input in inputs {
            let _ = parse(input);
            let _ = parse_json(input, Some("https://github.com/o/r"));
        }
        // 深い入れ子と、たくさんのタグ。
        let deep = "<div>".repeat(5000) + "x" + &"</div>".repeat(5000);
        assert!(!parse(&deep).is_empty());
        let deep_details = "<details>\n\n".repeat(500) + "x";
        let _ = parse(&deep_details);
        let many = "<b>x</b>".repeat(20_000);
        let blocks = parse(&many);
        let runs = paragraph(&blocks[0]).0;
        assert_eq!(text(runs).len(), 20_000);
        // 上限を超えた後のタグは読まない（文字は残す）。
        assert!(runs.first().is_some_and(|r| r.bold));
        let Block::HtmlTable { columns, rows, .. } = &parse(
            "<table><tr><td colspan=100000 rowspan=100000>x</td></tr><tr><td>y</td></tr></table>",
        )[0] else {
            panic!()
        };
        assert_eq!((*columns, *rows), (MAX_COLUMNS, 2));
        // 閉じない <sup> でも、Markdown の段落が閉じたら戻す。
        let blocks = parse("a <sup>b\n\nc\n");
        assert!(!paragraph(&blocks[1]).0[0].sup);
    }

    #[test]
    fn summary_holds_block_tags_as_text() {
        let blocks =
            parse("<details><summary><h3>Title</h3> more</summary>\n\nBody\n\n</details>\n");
        let Block::Details {
            summary,
            blocks: inner,
            ..
        } = &blocks[0]
        else {
            panic!("{blocks:?}")
        };
        assert!(summary[0].bold && summary[0].text == "Title");
        assert!(summary.iter().any(|r| r.text == "more"));
        assert_eq!(inner.len(), 1);
    }

    #[test]
    fn html_json_shape() {
        let json = parse_json(
            "<p align=center><kbd>K</kbd></p>\n\n<details open><summary>S</summary>x</details>\n\n<table><tr><td colspan=2>a</td></tr></table>\n",
            None,
        );
        assert!(json.contains("\"align\":\"center\""), "{json}");
        assert!(json.contains("\"kbd\":true"), "{json}");
        assert!(json.contains("\"kind\":\"details\""), "{json}");
        assert!(json.contains("\"open\":true"), "{json}");
        assert!(json.contains("\"kind\":\"htmlTable\""), "{json}");
        assert!(json.contains("\"colspan\":2"), "{json}");
        // そろえ方のない段落には align を出さない。
        assert!(!parse_json("x", None).contains("align"));
    }
}
