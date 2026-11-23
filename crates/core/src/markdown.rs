//! Markdown を、FE が自前で組み立てて見せるための形（見出し・段落・リスト・引用・表・コードなどの並び）にする
//! （History の「すべてのファイル」の整形表示）。読み取りは pulldown-cmark（CommonMark と GitHub の拡張の表・取り消し線・
//! タスクリスト・脚注）。生の HTML は描かず、文字のまま残す（FE は薄く見せる）。

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use serde::Serialize;

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
    /// 生の HTML（描かずに文字のまま）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub html: bool,
    /// リンク先（リンクの中の文字なら）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// 画像（`text` は代わりの文、`image` はその場所）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// 脚注への参照（ラベル）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footnote: Option<String>,
    /// GitHub 独自の絵文字（`:octocat:` などの画像だけのもの。名前）。FE は `text`（`:octocat:`）を薄い札で見せる。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_emoji: Option<String>,
    /// 段落の中の改行（`text` は空）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub line_break: bool,
}

/// リストの 1 項目。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    /// タスクリストの印（`[x]` なら true、`[ ]` なら false、なければ None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

/// 表の列のそろえ方。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// ブロック 1 つ。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph {
        runs: Vec<Run>,
    },
    /// コードブロック。`language` はフェンスの言葉（` ```rust ` の rust、なければ空）。
    Code {
        language: String,
        text: String,
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
    /// 生の HTML のブロック（描かずに文字のまま）。
    Html {
        text: String,
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
    Html(String),
    Footnote(String, Vec<Block>),
}

#[derive(Default)]
struct Style {
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
    image: Option<String>,
}

struct Builder {
    stack: Vec<Open>,
    blocks: Vec<Block>,
    /// 今の段落・見出し・表のマスの文字。
    runs: Vec<Run>,
    style: Style,
}

impl Builder {
    fn push_run(&mut self, text: &str, code: bool, html: bool) {
        let mut run = Run {
            text: text.to_owned(),
            bold: self.style.bold > 0,
            italic: self.style.italic > 0,
            strike: self.style.strike > 0,
            code,
            html,
            link: self.style.links.last().cloned(),
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
            run.link = None;
        }
        // 同じ飾りの続きはつなげる。
        if let Some(last) = self.runs.last_mut()
            && last.image.is_none()
            && last.footnote.is_none()
            && !last.line_break
            && (
                last.bold,
                last.italic,
                last.strike,
                last.code,
                last.html,
                &last.link,
            ) == (
                run.bold, run.italic, run.strike, run.code, run.html, &run.link,
            )
        {
            last.text.push_str(text);
            return;
        }
        self.runs.push(run);
    }

    /// 閉じたブロックを、入れ子の親（なければ一番上）に足す。
    fn add(&mut self, block: Block) {
        for open in self.stack.iter_mut().rev() {
            match open {
                Open::Quote(_, blocks) | Open::Item(_, blocks) | Open::Footnote(_, blocks) => {
                    blocks.push(block);
                    return;
                }
                _ => {}
            }
        }
        self.blocks.push(block);
    }

    /// 段落を開かずに書かれた文字（「きつい」リストの項目の中）を段落にする。
    fn flush_loose(&mut self) {
        if !self.runs.is_empty()
            && matches!(
                self.stack.last(),
                Some(Open::Item(..)) | Some(Open::Quote(..)) | Some(Open::Footnote(..)) | None
            )
        {
            let runs = std::mem::take(&mut self.runs);
            self.add(Block::Paragraph { runs });
        }
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
    let mut blocks = parse_blocks(text);
    for block in &mut blocks {
        linkify_block(block, github);
    }
    blocks
}

fn parse_blocks(text: &str) -> Vec<Block> {
    let options = Options::ENABLE_GFM
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut b = Builder {
        stack: Vec::new(),
        blocks: Vec::new(),
        runs: Vec::new(),
        style: Style::default(),
    };
    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    b.flush_loose();
                    b.stack.push(Open::Paragraph);
                }
                Tag::Heading { level, .. } => {
                    b.flush_loose();
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
                    b.flush_loose();
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
                    b.flush_loose();
                    let language = match kind {
                        CodeBlockKind::Fenced(info) => {
                            info.split_whitespace().next().unwrap_or("").to_owned()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    b.stack.push(Open::Code(language, String::new()));
                }
                Tag::HtmlBlock => {
                    b.flush_loose();
                    b.stack.push(Open::Html(String::new()));
                }
                Tag::List(start) => {
                    b.flush_loose();
                    b.stack.push(Open::List(start, Vec::new()));
                }
                Tag::Item => b.stack.push(Open::Item(None, Vec::new())),
                Tag::FootnoteDefinition(label) => {
                    b.flush_loose();
                    b.stack.push(Open::Footnote(label.to_string(), Vec::new()));
                }
                Tag::Table(aligns) => {
                    b.flush_loose();
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
                        ..Run::default()
                    });
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Paragraph => {
                    b.stack.pop();
                    let runs = std::mem::take(&mut b.runs);
                    b.add(Block::Paragraph { runs });
                }
                TagEnd::Heading(_) => {
                    if let Some(Open::Heading(level)) = b.stack.pop() {
                        let runs = std::mem::take(&mut b.runs);
                        b.add(Block::Heading { level, runs });
                    }
                }
                TagEnd::BlockQuote(_) => {
                    b.flush_loose();
                    if let Some(Open::Quote(alert, blocks)) = b.stack.pop() {
                        b.add(Block::Quote { alert, blocks });
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(Open::Code(language, mut text)) = b.stack.pop() {
                        if text.ends_with('\n') {
                            text.pop();
                        }
                        b.add(Block::Code { language, text });
                    }
                }
                TagEnd::HtmlBlock => {
                    if let Some(Open::Html(text)) = b.stack.pop() {
                        b.add(Block::Html {
                            text: text.trim_end().to_owned(),
                        });
                    }
                }
                TagEnd::Item => {
                    b.flush_loose();
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
                    b.flush_loose();
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
            },
            Event::Text(text) => match b.stack.last_mut() {
                Some(Open::Code(_, code)) => code.push_str(&text),
                Some(Open::Html(html)) => html.push_str(&text),
                _ => b.push_run(&text, false, false),
            },
            Event::Code(text) => b.push_run(&text, true, false),
            Event::Html(text) => match b.stack.last_mut() {
                Some(Open::Html(html)) => html.push_str(&text),
                _ => b.push_run(&text, false, true),
            },
            Event::InlineHtml(text) => b.push_run(&text, false, true),
            Event::SoftBreak => b.push_run(" ", false, false),
            Event::HardBreak => b.runs.push(Run {
                line_break: true,
                ..Run::default()
            }),
            Event::Rule => {
                b.flush_loose();
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
            _ => {}
        }
    }
    b.flush_loose();
    b.blocks
}

/// `parse_with` の結果の JSON（FE 向け）。
pub fn parse_json(text: &str, github: Option<&str>) -> String {
    serde_json::to_string(&parse_with(text, github)).unwrap_or_else(|_| "[]".to_owned())
}

fn linkify_block(block: &mut Block, github: Option<&str>) {
    match block {
        Block::Heading { runs, .. } | Block::Paragraph { runs } => {
            *runs = linkify(std::mem::take(runs), github)
        }
        Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
            blocks.iter_mut().for_each(|b| linkify_block(b, github))
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
            || run.html
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
            || run.html
            || run.image.is_some()
            || run.footnote.is_some()
            || run.line_break
        {
            out.push(run);
            continue;
        }
        let text = emojify(&run.text);
        let mut last = 0;
        for (start, end, url) in find_links(&text, github) {
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

/// 文の中のリンクにする所（始まり・終わり・リンク先）。
fn find_links(text: &str, github: Option<&str>) -> Vec<(usize, usize, String)> {
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
        if at_word(i)
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
        let Block::Heading { level, runs } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!((*level, text(runs).as_str()), (1, "Title"));
        let Block::Paragraph { runs } = &blocks[1] else {
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
        assert!(matches!(&blocks[5], Block::Html { text } if text == "<div>raw</div>"));
    }

    #[test]
    fn reads_images_and_footnotes() {
        let blocks = parse("![a logo](img/logo.png) text[^1]\n\n[^1]: The note.\n");
        let Block::Paragraph { runs } = &blocks[0] else {
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
    fn reads_github_alerts_and_links() {
        let blocks = parse_with(
            "> [!WARNING]\n> Careful.\n\nSee https://example.com/a_(b). and www.x.org, fixes #12 by @dev in 1a2b3c4d, mail a@b.co.\n",
            Some("https://github.com/o/r"),
        );
        assert!(matches!(&blocks[0], Block::Quote { alert: Some(a), .. } if a == "warning"));
        let Block::Paragraph { runs } = &blocks[1] else {
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
        let Block::Paragraph { runs } = &plain[0] else {
            panic!()
        };
        assert_eq!(runs.iter().filter(|r| r.link.is_some()).count(), 1);
    }

    #[test]
    fn github_custom_emoji_become_chips() {
        let blocks = parse("Ship it :shipit: with :octocat: and :nope: :smile:\n");
        let Block::Paragraph { runs } = &blocks[0] else {
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
}
