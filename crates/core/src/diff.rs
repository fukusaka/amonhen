//! ファイル 1 つの行単位の差分。各行は変更前・変更後の blob の中を位置で指す（コピーしない）。

use std::ops::Range;
use std::sync::Arc;

use amonhen_layout::{DiffRow, Highlight, SyntaxSpan};
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

/// `before` から `after` への行の差分で、`before` の消された行と `after` の足された行の印。
fn changed_lines(before: &[u8], after: &[u8]) -> (Vec<bool>, Vec<bool>) {
    let input = InternedInput::new(before, after);
    let mut removed = vec![false; input.before.len()];
    let mut added = vec![false; input.after.len()];
    let mut d = Diff::compute(Algorithm::Histogram, &input);
    d.postprocess_lines(&input);
    for h in d.hunks() {
        for r in h.before {
            removed[r as usize] = true;
        }
        for a in h.after {
            added[a as usize] = true;
        }
    }
    (removed, added)
}

/// HEAD と作業ツリーの差分 `diff` の各行について、その変更が `index` に入っているか（変更の行以外は false）。
/// 消した行は「HEAD と index の差分でも消えている」、足した行は「index と作業ツリーの差分で足されていない」
/// なら入っているとみなす。index が「HEAD に `diff` の変更の一部を当てたもの」として表せなければ
/// （作業ツリーで書き戻した変更が index にだけあるなど）None。
pub fn staged_lines(diff: &FileDiff, index: &[u8]) -> Option<Vec<bool>> {
    if diff.binary || is_binary(index) {
        return None;
    }
    let (removed_in_index, _) = changed_lines(&diff.old, index);
    let (_, added_after_index) = changed_lines(index, &diff.new);
    let staged: Vec<bool> = diff
        .lines
        .iter()
        .map(|l| match l.kind {
            DiffRow::REMOVED => removed_in_index[l.old_no as usize - 1],
            DiffRow::ADDED => !added_after_index[l.new_no as usize - 1],
            _ => false,
        })
        .collect();
    (apply_selection(diff, |r| staged[r]) == index).then_some(staged)
}

/// 行の中のバイトの位置 `at` を、FE の符号単位の位置に直す。
fn units(encoding: StringEncoding, text: &[u8], at: usize) -> u32 {
    if encoding == StringEncoding::Utf8 || text[..at].is_ascii() {
        at as u32
    } else {
        String::from_utf8_lossy(&text[..at]).encode_utf16().count() as u32
    }
}

/// 続けて並ぶ削除行と追加行を 1 行ずつ組にして、変わった単語を FE の符号単位で返す。
fn word_highlights(diff: &FileDiff, encoding: StringEncoding) -> Vec<Highlight> {
    let units = |text: &[u8], at: usize| units(encoding, text, at);
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

/// FE 向けの差分。行の文字列は、変更前・変更後の blob（文字列置き場の 1 番以降）を直接指す。
/// 1 ファイルの差分のほか、コミットの全ファイルを見出しの行（`DiffRow::FILE`）でつないだもの（`StackedDiff`）もある。
pub struct DiffView {
    pub rows: Vec<DiffRow>,
    /// 削除行と追加行の組の中で、変わった単語の範囲。
    pub highlights: Vec<Highlight>,
    pub strings: StringTable,
    pub binary: bool,
    pub added: u32,
    pub removed: u32,
    /// 後から色付けするための、ファイルごとの中身と各行が指す場所。
    #[cfg_attr(not(feature = "syntax"), allow(dead_code))]
    pub(crate) parts: Vec<Part>,
    #[cfg_attr(not(feature = "syntax"), allow(dead_code))]
    pub(crate) encoding: StringEncoding,
}

/// 差分の表示の中の、1 ファイル分の行。
#[cfg_attr(not(feature = "syntax"), allow(dead_code))]
pub(crate) struct Part {
    /// ファイルのパス（色付けの言語を決める）。
    pub(crate) path: String,
    pub(crate) old: Arc<Vec<u8>>,
    pub(crate) new: Arc<Vec<u8>>,
    /// この部分の最初の行（表示の中の番号）。
    pub(crate) first_row: u32,
    /// 各行が指す場所（`first_row` からの順）。
    pub(crate) sources: Vec<(Source, Range<u32>)>,
}

/// `DiffView` を組み立てる。ファイルの中身は共有したまま、行が指す場所だけを作る。
struct ViewBuilder {
    strings: StringTableBuilder,
    rows: Vec<DiffRow>,
    highlights: Vec<Highlight>,
    parts: Vec<Part>,
    encoding: StringEncoding,
}

impl ViewBuilder {
    fn new(profile: &FrontendProfile) -> Self {
        Self {
            strings: StringTableBuilder::new(profile.string),
            rows: Vec::new(),
            highlights: Vec::new(),
            parts: Vec::new(),
            encoding: profile.string,
        }
    }

    /// `diff` の行を足す。
    fn push_file(&mut self, path: &str, diff: &FileDiff) {
        let headers = Arc::new(diff.headers.clone().into_bytes());
        let sources = [
            (self.strings.share(diff.old.clone()), &diff.old),
            (self.strings.share(diff.new.clone()), &diff.new),
            (self.strings.share(headers.clone()), &headers),
        ];
        let first_row = self.rows.len() as u32;
        for l in &diff.lines {
            let (buf, data) = sources[l.source as usize];
            let text = self
                .strings
                .add(buf, &data[l.range.start as usize..l.range.end as usize]);
            self.rows.push(DiffRow {
                kind: l.kind,
                flags: 0,
                _pad: [0; 2],
                old_no: l.old_no,
                new_no: l.new_no,
                text,
            });
        }
        self.highlights
            .extend(
                word_highlights(diff, self.encoding)
                    .into_iter()
                    .map(|h| Highlight {
                        row: h.row + first_row,
                        ..h
                    }),
            );
        self.parts.push(Part {
            path: path.to_owned(),
            old: diff.old.clone(),
            new: diff.new.clone(),
            first_row,
            sources: diff
                .lines
                .iter()
                .map(|l| (l.source, l.range.clone()))
                .collect(),
        });
    }

    fn finish(self, binary: bool, added: u32, removed: u32) -> DiffView {
        DiffView {
            rows: self.rows,
            highlights: self.highlights,
            strings: self.strings.finish(),
            binary,
            added,
            removed,
            parts: self.parts,
            encoding: self.encoding,
        }
    }
}

impl DiffView {
    pub fn new(diff: &FileDiff, profile: &FrontendProfile) -> Self {
        let mut builder = ViewBuilder::new(profile);
        builder.push_file("", diff);
        builder.finish(diff.binary, diff.added, diff.removed)
    }

    pub fn with_path(mut self, path: &str) -> Self {
        if let Some(part) = self.parts.first_mut() {
            part.path = path.to_owned();
        }
        self
    }

    /// 構文の色付け。変更前・変更後のファイル全体を解析して、各行に割り当てる。時間がかかることがあるので、
    /// FE は差分を色なしで先に表示し、これを裏で呼ぶ。言語がわからなければ空。
    #[cfg(feature = "syntax")]
    pub fn syntax(&self) -> Vec<SyntaxSpan> {
        let mut out = Vec::new();
        for part in &self.parts {
            self.part_syntax(part, &mut out);
        }
        out
    }

    #[cfg(feature = "syntax")]
    fn part_syntax(&self, part: &Part, out: &mut Vec<SyntaxSpan>) {
        if amonhen_syntax::Language::from_path(&part.path).is_none()
            || is_binary(&part.old)
            || is_binary(&part.new)
        {
            return;
        }
        let uses = |side: Source| part.sources.iter().any(|(s, _)| *s == side);
        let tokens = |side: Source, data: &[u8]| {
            if uses(side) {
                amonhen_syntax::highlight(&part.path, data)
            } else {
                Vec::new()
            }
        };
        let (old, new) = (
            tokens(Source::Old, &part.old),
            tokens(Source::New, &part.new),
        );
        for (i, (source, range)) in part.sources.iter().enumerate() {
            let (data, tokens) = match source {
                Source::Old => (&part.old, &old),
                Source::New => (&part.new, &new),
                Source::Header => continue,
            };
            let text = &data[range.start as usize..range.end as usize];
            // 行の先頭より後ろで終わる最初のトークンから、行の終わりまで。
            let first = tokens.partition_point(|t| t.end <= range.start);
            for t in tokens[first..].iter().take_while(|t| t.start < range.end) {
                let start = t.start.max(range.start) - range.start;
                let end = t.end.min(range.end) - range.start;
                out.push(SyntaxSpan {
                    row: part.first_row + i as u32,
                    start: units(self.encoding, text, start as usize),
                    end: units(self.encoding, text, end as usize),
                    kind: t.kind,
                    _pad: [0; 3],
                });
            }
        }
    }

    #[cfg(not(feature = "syntax"))]
    pub fn syntax(&self) -> Vec<SyntaxSpan> {
        Vec::new()
    }
}

/// コミットの全ファイルの差分。一度計算しておき、畳むファイルを変えるたびに `view` で表示を作り直す。
pub struct StackedDiff {
    /// パスと差分（コミットの変更ファイルの順）。
    pub files: Vec<(String, FileDiff)>,
}

impl StackedDiff {
    /// 各ファイルの見出しの行（`DiffRow::FILE`。文字列はパス、`old_no` は削除の行数、`new_no` は追加の行数）に、
    /// 畳まないファイルの差分の行を続けた表示。`collapsed[i]` が true のファイルは見出しだけ。
    pub fn view(&self, collapsed: &[bool], profile: &FrontendProfile) -> DiffView {
        let mut builder = ViewBuilder::new(profile);
        // 見出しのパスは 1 つのバッファに並べて共有する。
        let names: Vec<u8> = self.files.iter().flat_map(|(p, _)| p.bytes()).collect();
        let names = Arc::new(names);
        let names_buf = builder.strings.share(names.clone());
        let (mut at, mut added, mut removed) = (0usize, 0, 0);
        for (i, (path, diff)) in self.files.iter().enumerate() {
            let hidden = collapsed.get(i).copied().unwrap_or(false);
            let mut flags = 0;
            if hidden {
                flags |= DiffRow::COLLAPSED;
            }
            if diff.binary {
                flags |= DiffRow::BINARY;
            }
            let text = builder.strings.add(names_buf, &names[at..at + path.len()]);
            at += path.len();
            builder.rows.push(DiffRow {
                kind: DiffRow::FILE,
                flags,
                _pad: [0; 2],
                old_no: diff.removed,
                new_no: diff.added,
                text,
            });
            added += diff.added;
            removed += diff.removed;
            if !hidden && !diff.binary {
                builder.push_file(path, diff);
            }
        }
        builder.finish(false, added, removed)
    }
}

/// テキスト全体の構文の色付け（マージエディタなど、行に分けずに表示するもの）。`start..end` はテキストの
/// 先頭からの UTF-16 の位置で、`row` は 0。言語がわからなければ空。
#[cfg(feature = "syntax")]
pub fn text_syntax(path: &str, text: &[u8]) -> Vec<SyntaxSpan> {
    if amonhen_syntax::Language::from_path(path).is_none() {
        return Vec::new();
    }
    let tokens = amonhen_syntax::highlight(path, text);
    // トークンは位置の順なので、バイト位置から UTF-16 の位置へは前から順に数えて進める。
    let (mut byte, mut unit) = (0usize, 0u32);
    let mut to_units = |at: usize| {
        let chunk = &text[byte..at];
        unit += if chunk.is_ascii() {
            chunk.len() as u32
        } else {
            String::from_utf8_lossy(chunk).encode_utf16().count() as u32
        };
        byte = at;
        unit
    };
    tokens
        .iter()
        .map(|t| SyntaxSpan {
            row: 0,
            start: to_units(t.start as usize),
            end: to_units(t.end as usize),
            kind: t.kind,
            _pad: [0; 3],
        })
        .collect()
}

#[cfg(not(feature = "syntax"))]
pub fn text_syntax(_path: &str, _text: &[u8]) -> Vec<SyntaxSpan> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacks_files_under_headers_and_collapses_them() {
        let a = file_diff(
            Arc::new(b"1\n2\n".to_vec()),
            Arc::new(b"1\nx\n".to_vec()),
            3,
        );
        let b = file_diff(Arc::new(b"".to_vec()), Arc::new(b"new\n".to_vec()), 3);
        let stacked = StackedDiff {
            files: vec![("a.txt".into(), a), ("dir/b.rs".into(), b)],
        };
        let profile = FrontendProfile::MACOS;
        let view = stacked.view(&[false, false], &profile);
        let kinds: Vec<u8> = view.rows.iter().map(|r| r.kind).collect();
        // a.txt: 見出し、@@、1、-2、+x。dir/b.rs: 見出し、@@、+new。
        assert_eq!(
            kinds,
            [
                DiffRow::FILE,
                DiffRow::HUNK,
                DiffRow::CONTEXT,
                DiffRow::REMOVED,
                DiffRow::ADDED,
                DiffRow::FILE,
                DiffRow::HUNK,
                DiffRow::ADDED
            ]
        );
        assert_eq!(view.strings.bytes(view.rows[5].text), b"dir/b.rs");
        assert_eq!((view.rows[0].old_no, view.rows[0].new_no), (1, 1));
        assert_eq!((view.added, view.removed), (2, 1));
        // 単語の強調は、つないだ後の行の番号になる。
        assert!(view.highlights.iter().all(|h| h.row == 3 || h.row == 4));
        // 畳んだファイルは見出しだけ。
        let view = stacked.view(&[true, false], &profile);
        assert_eq!(view.rows.len(), 4);
        assert_eq!(view.rows[0].flags & DiffRow::COLLAPSED, DiffRow::COLLAPSED);
        assert_eq!(view.rows[1].kind, DiffRow::FILE);
    }

    #[cfg(feature = "syntax")]
    #[test]
    fn colors_whole_text_in_utf16_units() {
        let text = "// é\nfn main() {}\n";
        let spans = text_syntax("a.rs", text.as_bytes());
        let comment = spans
            .iter()
            .find(|s| s.kind == SyntaxSpan::COMMENT)
            .unwrap();
        assert_eq!((comment.start, comment.end), (0, 4), "é は UTF-16 で 1 つ");
        let keyword = spans
            .iter()
            .find(|s| s.kind == SyntaxSpan::KEYWORD)
            .unwrap();
        let utf16: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&utf16[keyword.start as usize..keyword.end as usize]).unwrap(),
            "fn"
        );
        assert!(text_syntax("a.unknown", text.as_bytes()).is_empty());
    }

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

    #[cfg(feature = "syntax")]
    #[test]
    fn syntax_spans_are_mapped_onto_rows_in_frontend_units() {
        // 変更前の 2 行目は、1 行目から続くコメントの中。差分の断片だけ見ても色はわからない。
        let old = "/* start\n still comment */\nint a;\n";
        let new = "/* start\n still comment */\nint \u{3042} = \"\u{3044}\";\n";
        let d = diff(old, new, 3);
        let v = DiffView::new(&d, &FrontendProfile::MACOS).with_path("x.c");
        let spans = v.syntax();
        let on = |row: usize| -> Vec<(u32, u32, u8)> {
            spans
                .iter()
                .filter(|s| s.row as usize == row)
                .map(|s| (s.start, s.end, s.kind))
                .collect()
        };
        // 変更後の行番号で探す（ASCII でない行は UTF-16 に変換されている）。
        let row = |kind: u8, no: u32| {
            v.rows
                .iter()
                .position(|r| r.kind == kind && r.new_no == no)
                .unwrap()
        };
        assert_eq!(on(row(DiffRow::CONTEXT, 2)), [(0, 17, SyntaxSpan::COMMENT)]);
        // 「あ」は UTF-16 で 1 単位なので、文字列は 8..11。
        let added = on(row(DiffRow::ADDED, 3));
        assert!(added.contains(&(0, 3, SyntaxSpan::TYPE)), "{added:?}");
        assert!(added.contains(&(8, 11, SyntaxSpan::STRING)), "{added:?}");
        let plain = DiffView::new(&d, &FrontendProfile::MACOS).with_path("notes.txt");
        assert!(
            plain.syntax().is_empty(),
            "言語がわからなければ色を付けない"
        );
    }

    #[test]
    fn additions_to_empty_files_and_binaries() {
        assert_eq!(render(&diff("", "x\n", 3)), "@@ -0,0 +1 @@\n+x\n");
        assert!(diff("a\0b", "a\0c", 3).binary);
    }
}
