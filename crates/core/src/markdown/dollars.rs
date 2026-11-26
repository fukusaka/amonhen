//! 地の文の中の `$…$`・`$$…$$`・`` $`…`$ `` を、GitHub と同じ決まりで文中の数式にする。
//!
//! GitHub は Markdown を読んだ後の文字の並びから `$` の組を探す。pulldown-cmark が数式にしたものも、しなかったものも、
//! いったん原文の文字として段落に置き（`\$` は `$` にする）、ここで GitHub と同じ決まりで選び直す。決まり:
//!
//! - 開きの `$` の前は、文字の並びの頭・ASCII の空白・`(` のどれか（`a$x$`・`日本語の$x$`・`-$x$` は数式にしない）。
//!   開きのすぐ後が空白なら数式にしない（`$ x$`）。
//! - 閉じは開きの後の最初の `$`。その後に ASCII の英数字か `_` が続けば数式にしない（`$5/$10`・`$x$a`）。
//!   閉じの前が空白なら、その後は終わりか ASCII の記号のときだけ数式にする（`$x $.` はする、`$y $ z$` はしない）。
//! - 開きが `$$` なら次の `$$` までを、ブロックの組み方の式にする。開きが `` $` `` なら、同じ数の `` ` `` と `$` までを
//!   コードと同じ書き方の式にする（中に `$` を書ける。閉じの後に英数字が続いてもよい）。
//! - 斜体・リンク・画像の中の文字、コード、脚注の中身は数式にしない（GitHub と同じ）。
//!
//! 数式の中の `\{`・`\\` などは、GitHub と違って TeX のまま残す（GitHub は Markdown のエスケープとして先に外すので、
//! 式が崩れる）。`\$` だけは GitHub と同じく `$` にする（それで組が決まるため）。

use super::{Block, Run};

/// `blocks` の中の段落・見出し・表のマス・`<summary>` の文字から数式を探す（脚注の中身は探さない）。
pub(super) fn find(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Heading { runs, .. } | Block::Paragraph { runs, .. } => split(runs),
            Block::Quote { blocks, .. } => find(blocks),
            Block::Details {
                summary, blocks, ..
            } => {
                split(summary);
                find(blocks);
            }
            Block::List { items, .. } => {
                for item in items {
                    find(&mut item.blocks);
                }
            }
            Block::Table { head, rows, .. } => {
                for cell in head.iter_mut().chain(rows.iter_mut().flatten()) {
                    split(cell);
                }
            }
            Block::HtmlTable { cells, .. } => {
                for cell in cells {
                    split(&mut cell.runs);
                }
            }
            _ => {}
        }
    }
}

/// pulldown-cmark が数式にした原文を、段落に置く文字にする（`\$` は `$` に。`\\` はそのまま）。
pub(super) fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('$') => out.push('$'),
            Some(next) => {
                out.push('\\');
                out.push(next);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// 開きの `$` の前に置ける文字か（`None` は文字の並びの頭）。
pub(super) fn opens_after(prev: Option<char>) -> bool {
    prev.is_none_or(|c| c.is_ascii_whitespace() || c == '(')
}

/// 閉じの `$` の後に置ける文字か（`None` は文字の並びの終わり）。
pub(super) fn closes_before(next: Option<char>) -> bool {
    next.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

fn split(runs: &mut Vec<Run>) {
    if !runs.iter().any(|r| scannable(r) && r.text.contains('$')) {
        return;
    }
    let mut out = Vec::with_capacity(runs.len());
    for mut run in std::mem::take(runs) {
        if !scannable(&run) || !run.text.contains('$') {
            out.push(run);
            continue;
        }
        let found = scan(&run.text);
        if found.is_empty() {
            out.push(run);
            continue;
        }
        // 地の文の飾り（文字は抜いて、式の間の文字ごとに写す）
        let text = std::mem::take(&mut run.text);
        let mut last = 0;
        for (range, tex, display) in found {
            if range.start > last {
                out.push(Run {
                    text: text[last..range.start].to_owned(),
                    ..run.clone()
                });
            }
            out.push(Run {
                text: text[range.clone()].to_owned(),
                code: true,
                math_tex: Some(tex),
                math_display: display,
                ..Run::default()
            });
            last = range.end;
        }
        if last < text.len() {
            out.push(Run {
                text: text[last..].to_owned(),
                ..run
            });
        }
    }
    *runs = out;
}

/// 行の境（`\n`）を空白にする（式を選び終えた後で。式の原文はそのまま）。
pub(super) fn settle(blocks: &mut [Block]) {
    let runs = |runs: &mut [Run]| {
        for run in runs.iter_mut().filter(|r| r.math_tex.is_none()) {
            if run.text.contains('\n') {
                run.text = run.text.replace('\n', " ");
            }
        }
    };
    for block in blocks {
        match block {
            Block::Heading { runs: r, .. } | Block::Paragraph { runs: r, .. } => runs(r),
            Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => settle(blocks),
            Block::Details {
                summary, blocks, ..
            } => {
                runs(summary);
                settle(blocks);
            }
            Block::List { items, .. } => {
                for item in items {
                    settle(&mut item.blocks);
                }
            }
            Block::Table { head, rows, .. } => {
                for cell in head.iter_mut().chain(rows.iter_mut().flatten()) {
                    runs(cell);
                }
            }
            Block::HtmlTable { cells, .. } => {
                for cell in cells {
                    runs(&mut cell.runs);
                }
            }
            Block::Code { .. } | Block::Diagram { .. } | Block::Math { .. } | Block::Rule => {}
        }
    }
}

/// 数式を探してよい文字か。
fn scannable(run: &Run) -> bool {
    !run.code
        && !run.italic
        && run.link.is_none()
        && run.image.is_none()
        && run.footnote.is_none()
        && run.math_tex.is_none()
        && !run.line_break
}

/// `text` の中の数式（原文の範囲・TeX・ブロックの組み方か）。
fn scan(text: &str) -> Vec<(std::ops::Range<usize>, String, bool)> {
    let bytes = text.as_bytes();
    let char_at = |i: usize| text.get(i..).and_then(|s| s.chars().next());
    let char_before = |i: usize| text[..i].chars().next_back();
    let mut found = Vec::new();
    // 閉じの見つからない開きが続いても、毎回終わりまで探さない（`$$` も `` `$ `` もこの先にない）
    let mut no_double = false;
    let last_tick = text.rfind("`$");
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            i += 1;
            continue;
        }
        let double = bytes.get(i + 1) == Some(&b'$');
        if !opens_after(char_before(i)) {
            i += if double { 2 } else { 1 };
            continue;
        }
        if double {
            let start = i + 2;
            let end = if no_double {
                None
            } else {
                text[start..].find("$$").map(|j| start + j)
            };
            no_double = end.is_none();
            match end {
                Some(end)
                    if end > start
                        && !text[start..end].trim().is_empty()
                        && closes_before(char_at(end + 2)) =>
                {
                    found.push((i..end + 2, text[start..end].trim().to_owned(), true));
                    i = end + 2;
                }
                _ => i += 2,
            }
            continue;
        }
        match char_at(i + 1) {
            None => break,
            Some(c) if c.is_whitespace() => {
                i += 1;
                continue;
            }
            Some('`') => {
                // `` $`…`$ `` は閉じの後を見ない（GitHub と同じ）
                if last_tick.is_some_and(|t| t > i)
                    && let Some(end) = backtick_end(text, i + 1)
                {
                    let tex = super::code_span(&text[i + 1..end]).to_owned();
                    found.push((i..end + 1, tex, false));
                    i = end + 1;
                    continue;
                }
            }
            Some(_) => {}
        }
        let Some(end) = text[i + 1..].find('$').map(|j| i + 1 + j) else {
            break;
        };
        let next = char_at(end + 1);
        // 行の境（次の行の頭の閉じ）は空白に数えない
        let spaced = char_before(end).is_some_and(|c| c.is_whitespace() && c != '\n');
        let fits =
            closes_before(next) && (!spaced || next.is_none_or(|c| c.is_ascii_punctuation()));
        if fits {
            found.push((i..end + 1, text[i + 1..end].to_owned(), false));
            i = end + 1;
        } else {
            i += 1;
        }
    }
    found
}

/// `` $`…`$ `` の閉じの `$` の位置（`start` は開きの `` ` `` の位置。同じ数の `` ` `` のすぐ後の `$`）。
fn backtick_end(text: &str, start: usize) -> Option<usize> {
    let ticks = text[start..].len() - text[start..].trim_start_matches('`').len();
    let mut from = start + ticks;
    while let Some(j) = text[from..].find('`').map(|j| from + j) {
        let run = text[j..].len() - text[j..].trim_start_matches('`').len();
        if run == ticks && text.as_bytes().get(j + run) == Some(&b'$') {
            return Some(j + run);
        }
        from = j + run;
    }
    None
}
