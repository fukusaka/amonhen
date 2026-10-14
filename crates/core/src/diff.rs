//! ファイル 1 つの行単位の差分。各行は変更前・変更後の blob の中を位置で指す（コピーしない）。

use std::ops::Range;
use std::sync::Arc;

use amonhen_layout::{DiffRow, Highlight};
use amonhen_projection::{FrontendProfile, StringEncoding, StringTable, StringTableBuilder};
use imara_diff::{Algorithm, Diff, InternedInput};

/// 差分の 1 行が指す文字列の場所。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Old = 0,
    New = 1,
    /// `FileDiff::headers` の中（hunk の見出し）。
    Header,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: u8,
    pub old_no: u32,
    pub new_no: u32,
    pub source: Source,
    /// 改行（`\n`・`\r\n`）を除いたバイト範囲。
    pub range: Range<u32>,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub old: Arc<Vec<u8>>,
    pub new: Arc<Vec<u8>>,
    /// hunk の見出しを並べた文字列。
    pub headers: String,
    pub lines: Vec<Line>,
    /// NUL を含むファイル。行の差分は出さない。
    pub binary: bool,
    pub added: u32,
    pub removed: u32,
}

/// 先頭 8000 バイトに NUL があればバイナリとみなす（git と同じ）。
fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

/// 行ごとの、改行を除いたバイト範囲。imara-diff の行の区切り（`\n` まで含めて 1 行）と同じ数になる。
fn line_ranges(data: &[u8]) -> Vec<Range<u32>> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < data.len() {
        let end = data[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(data.len(), |i| start + i + 1);
        let mut text_end = end;
        if text_end > start && data[text_end - 1] == b'\n' {
            text_end -= 1;
        }
        if text_end > start && data[text_end - 1] == b'\r' {
            text_end -= 1;
        }
        lines.push(start as u32..text_end as u32);
        start = end;
    }
    lines
}

/// 前後に `context` 行の文脈をつけた差分。
pub fn file_diff(old: Arc<Vec<u8>>, new: Arc<Vec<u8>>, context: u32) -> FileDiff {
    let mut diff = FileDiff {
        old: old.clone(),
        new: new.clone(),
        headers: String::new(),
        lines: Vec::new(),
        binary: is_binary(&old) || is_binary(&new),
        added: 0,
        removed: 0,
    };
    if diff.binary {
        return diff;
    }
    let old_lines = line_ranges(&old);
    let new_lines = line_ranges(&new);
    let input = InternedInput::new(&old[..], &new[..]);
    debug_assert_eq!(input.before.len(), old_lines.len());
    debug_assert_eq!(input.after.len(), new_lines.len());
    let mut d = Diff::compute(Algorithm::Histogram, &input);
    d.postprocess_lines(&input);
    diff.added = d.count_additions();
    diff.removed = d.count_removals();

    // 間が 2 * context 行以下の変更は 1 つの hunk にまとめる。
    let hunks: Vec<_> = d.hunks().collect();
    let mut groups: Vec<Vec<imara_diff::Hunk>> = Vec::new();
    for h in hunks {
        match groups.last_mut() {
            Some(g) if h.before.start - g.last().unwrap().before.end <= 2 * context => g.push(h),
            _ => groups.push(vec![h]),
        }
    }

    let n_old = old_lines.len() as u32;
    for group in groups {
        let first = &group[0];
        let last = group.last().unwrap();
        let start_old = first.before.start.saturating_sub(context);
        let end_old = (last.before.end + context).min(n_old);
        // 変更前と変更後の行は、変更のない所では同じ差だけずれている。
        let start_new = first.after.start - (first.before.start - start_old);
        let end_new = last.after.end + (end_old - last.before.end);

        let header_start = diff.headers.len() as u32;
        let count = |start: u32, end: u32| match end - start {
            1 => format!("{}", start + 1),
            0 => format!("{start},0"),
            n => format!("{},{n}", start + 1),
        };
        diff.headers += &format!(
            "@@ -{} +{} @@",
            count(start_old, end_old),
            count(start_new, end_new)
        );
        diff.lines.push(Line {
            kind: DiffRow::HUNK,
            old_no: 0,
            new_no: 0,
            source: Source::Header,
            range: header_start..diff.headers.len() as u32,
        });

        let mut o = start_old;
        let mut n = start_new;
        let context_until = |diff: &mut FileDiff, o: &mut u32, n: &mut u32, until_old: u32| {
            while *o < until_old {
                diff.lines.push(Line {
                    kind: DiffRow::CONTEXT,
                    old_no: *o + 1,
                    new_no: *n + 1,
                    source: Source::New,
                    range: new_lines[*n as usize].clone(),
                });
                *o += 1;
                *n += 1;
            }
        };
        for h in &group {
            context_until(&mut diff, &mut o, &mut n, h.before.start);
            for r in h.before.clone() {
                diff.lines.push(Line {
                    kind: DiffRow::REMOVED,
                    old_no: r + 1,
                    new_no: 0,
                    source: Source::Old,
                    range: old_lines[r as usize].clone(),
                });
            }
            for a in h.after.clone() {
                diff.lines.push(Line {
                    kind: DiffRow::ADDED,
                    old_no: 0,
                    new_no: a + 1,
                    source: Source::New,
                    range: new_lines[a as usize].clone(),
                });
            }
            o = h.before.end;
            n = h.after.end;
        }
        context_until(&mut diff, &mut o, &mut n, end_old);
        debug_assert_eq!(n, end_new);
    }
    diff
}

/// 行ごとの、改行まで含めたバイト範囲。
fn full_line_ranges(data: &[u8]) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < data.len() {
        let end = data[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(data.len(), |i| start + i + 1);
        lines.push(start..end);
        start = end;
    }
    lines
}

/// 変更前の中身に、`apply` が真を返す行の変更（追加・削除）だけを反映した中身。
/// stage では選んだ行を、unstage では選ばなかった行を反映する（stage 済みの差分に対して）。
pub fn apply_selection(diff: &FileDiff, apply: impl Fn(usize) -> bool) -> Vec<u8> {
    let old = full_line_ranges(&diff.old);
    let new = full_line_ranges(&diff.new);
    let mut out = Vec::with_capacity(diff.old.len().max(diff.new.len()));
    let push = |out: &mut Vec<u8>, bytes: &[u8]| {
        // 改行のない最後の行の後ろに行を足すときは、改行を補う。
        if out.last().is_some_and(|&b| b != b'\n') {
            out.push(b'\n');
        }
        out.extend_from_slice(bytes);
    };
    // 次に写す変更前の行（0 始まり）。
    let mut next_old = 0usize;
    for (row, line) in diff.lines.iter().enumerate() {
        match line.kind {
            DiffRow::CONTEXT | DiffRow::REMOVED => {
                let at = line.old_no as usize - 1;
                for r in &old[next_old..at] {
                    push(&mut out, &diff.old[r.clone()]);
                }
                next_old = at + 1;
                if line.kind == DiffRow::CONTEXT || !apply(row) {
                    push(&mut out, &diff.old[old[at].clone()]);
                }
            }
            DiffRow::ADDED if apply(row) => {
                push(&mut out, &diff.new[new[line.new_no as usize - 1].clone()])
            }
            _ => {}
        }
    }
    for r in &old[next_old..] {
        push(&mut out, &diff.old[r.clone()]);
    }
    out
}

/// 続けて並ぶ削除行と追加行を 1 行ずつ組にして、変わった単語を FE の符号単位で返す。
fn word_highlights(diff: &FileDiff, encoding: StringEncoding) -> Vec<Highlight> {
    // バイトの位置を FE の符号単位の位置に直す。
    let units = |text: &[u8], at: usize| -> u32 {
        if encoding == StringEncoding::Utf8 || text[..at].is_ascii() {
            at as u32
        } else {
            String::from_utf8_lossy(&text[..at]).encode_utf16().count() as u32
        }
    };
    let mut out = Vec::new();
    let lines = &diff.lines;
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind != DiffRow::REMOVED {
            i += 1;
            continue;
        }
        let removed_start = i;
        while i < lines.len() && lines[i].kind == DiffRow::REMOVED {
            i += 1;
        }
        let added_start = i;
        while i < lines.len() && lines[i].kind == DiffRow::ADDED {
            i += 1;
        }
        for (r, a) in (removed_start..added_start).zip(added_start..i) {
            let (old, new) = (diff.text(&lines[r]), diff.text(&lines[a]));
            let Some((removed, added)) = crate::words::changed_words(old, new) else {
                continue;
            };
            for (row, text, ranges) in [(r, old, removed), (a, new, added)] {
                out.extend(ranges.into_iter().map(|x| Highlight {
                    row: row as u32,
                    start: units(text, x.start),
                    end: units(text, x.end),
                }));
            }
        }
    }
    out
}

impl FileDiff {
    pub fn text(&self, line: &Line) -> &[u8] {
        let r = line.range.start as usize..line.range.end as usize;
        match line.source {
            Source::Old => &self.old[r],
            Source::New => &self.new[r],
            Source::Header => &self.headers.as_bytes()[r],
        }
    }
}

/// FE 向けの差分。行の文字列は、変更前・変更後の blob（文字列置き場の 1・2 番）を直接指す。
pub struct DiffView {
    pub rows: Vec<DiffRow>,
    /// 削除行と追加行の組の中で、変わった単語の範囲。
    pub highlights: Vec<Highlight>,
    pub strings: StringTable,
    pub binary: bool,
    pub added: u32,
    pub removed: u32,
}

impl DiffView {
    pub fn new(diff: &FileDiff, profile: &FrontendProfile) -> Self {
        let headers = Arc::new(diff.headers.clone().into_bytes());
        let mut strings = StringTableBuilder::new(profile.string);
        let sources = [
            (strings.share(diff.old.clone()), &diff.old),
            (strings.share(diff.new.clone()), &diff.new),
            (strings.share(headers.clone()), &headers),
        ];
        let rows = diff
            .lines
            .iter()
            .map(|l| {
                let (buf, data) = sources[l.source as usize];
                DiffRow {
                    kind: l.kind,
                    _pad: [0; 3],
                    old_no: l.old_no,
                    new_no: l.new_no,
                    text: strings.add(buf, &data[l.range.start as usize..l.range.end as usize]),
                }
            })
            .collect();
        let highlights = word_highlights(diff, profile.string);
        Self {
            rows,
            highlights,
            strings: strings.finish(),
            binary: diff.binary,
            added: diff.added,
            removed: diff.removed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(d: &FileDiff) -> String {
        d.lines
            .iter()
            .map(|l| {
                let prefix = match l.kind {
                    DiffRow::ADDED => "+",
                    DiffRow::REMOVED => "-",
                    DiffRow::HUNK => "",
                    _ => " ",
                };
                format!("{prefix}{}\n", String::from_utf8_lossy(d.text(l)))
            })
            .collect()
    }

    fn diff(old: &str, new: &str, context: u32) -> FileDiff {
        file_diff(
            Arc::new(old.as_bytes().to_vec()),
            Arc::new(new.as_bytes().to_vec()),
            context,
        )
    }

    #[test]
    fn matches_git_unified_output() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let new = "1\n2\nthree\n4\n5\n6\n7\n8\n9\n10\neleven\n";
        let d = diff(old, new, 2);
        assert_eq!(
            render(&d),
            "@@ -1,5 +1,5 @@\n 1\n 2\n-3\n+three\n 4\n 5\n@@ -9,2 +9,3 @@\n 9\n 10\n+eleven\n"
        );
        assert_eq!((d.added, d.removed), (2, 1));
        // 変更のない行は変更後の blob を直接指す。
        let ctx = &d.lines[1];
        assert_eq!((ctx.old_no, ctx.new_no, ctx.source), (1, 1, Source::New));
    }

    #[test]
    fn close_changes_share_a_hunk_and_crlf_is_not_shown() {
        let d = diff("a\r\nb\r\nc\r\n", "A\r\nb\r\nC\r\n", 1);
        assert_eq!(render(&d), "@@ -1,3 +1,3 @@\n-a\n+A\n b\n-c\n+C\n");
    }

    #[test]
    fn the_view_points_ascii_lines_into_the_blobs() {
        let d = diff("a\nb\n", "a\n日本\n", 3);
        let v = DiffView::new(&d, &FrontendProfile::MACOS);
        assert_eq!(v.rows.len(), 4);
        let ctx = v.rows[1];
        assert_eq!(v.strings.bytes(ctx.text).as_ptr(), d.new.as_ptr());
        let added = v.rows[3];
        assert!(!added.text.is_ascii());
        assert_eq!(v.strings.bytes(v.rows[0].text), b"@@ -1,2 +1,2 @@");
    }

    #[test]
    fn applies_only_the_selected_lines() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let new = "1\nTWO\n3\n4\n5\n6\n7\n8\nNINE\nten\n";
        let d = diff(old, new, 1);
        // 行: @@, 1, -2, +TWO, 3, @@, 8, -9, +NINE, +ten
        let rows: Vec<&[u8]> = d.lines.iter().map(|l| d.text(l)).collect();
        assert_eq!(rows[2], b"2");
        assert_eq!(rows[9], b"ten");

        let all = apply_selection(&d, |_| true);
        assert_eq!(all, new.as_bytes());
        let none = apply_selection(&d, |_| false);
        assert_eq!(none, old.as_bytes());
        // 2 → TWO だけ（hunk 1 つ分）。
        let first = apply_selection(&d, |r| (2..=3).contains(&r));
        assert_eq!(first, b"1\nTWO\n3\n4\n5\n6\n7\n8\n9\n");
        // ten の追加だけ。
        let ten = apply_selection(&d, |r| r == 9);
        assert_eq!(ten, b"1\n2\n3\n4\n5\n6\n7\n8\n9\nten\n");
    }

    #[test]
    fn appending_after_a_last_line_without_newline() {
        let d = diff("a", "a\nb\n", 3);
        let added_b = d.lines.iter().position(|l| d.text(l) == b"b").unwrap();
        assert_eq!(apply_selection(&d, |r| r == added_b), b"a\nb\n");
    }

    #[test]
    fn highlights_changed_words_in_paired_lines() {
        let d = diff("let x = foo(1);\nkeep\n", "let x = bar(1);\nkeep\n", 1);
        let v = DiffView::new(&d, &FrontendProfile::MACOS);
        // @@, -let x = foo(1);, +let x = bar(1);, keep
        assert_eq!(
            v.highlights,
            [
                Highlight {
                    row: 1,
                    start: 8,
                    end: 11
                },
                Highlight {
                    row: 2,
                    start: 8,
                    end: 11
                }
            ]
        );
        // UTF-16 の FE では、日本語の後ろの位置は UTF-16 の符号単位で数える。
        let d = diff("名前 = foo\n", "名前 = bar\n", 1);
        let v = DiffView::new(&d, &FrontendProfile::MACOS);
        assert_eq!(
            v.highlights[0],
            Highlight {
                row: 1,
                start: 5,
                end: 8
            }
        );
    }

    #[test]
    fn additions_to_empty_files_and_binaries() {
        assert_eq!(render(&diff("", "x\n", 3)), "@@ -0,0 +1 @@\n+x\n");
        assert!(diff("a\0b", "a\0c", 3).binary);
    }
}
