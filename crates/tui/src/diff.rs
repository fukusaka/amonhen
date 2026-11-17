//! 差分の行を描く所。コミットの差分にも、作業ツリーの差分にも使う。
//!
//! 差分は窓口の答え（行のレコードと文字の表）をそのまま持ち、見えている行だけをそこから描く。行ごとの文字列は
//! 作らない。

use amonhen_front::Reply;
use amonhen_layout::{DiffRow, SyntaxSpan};
use amonhen_model::tr;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// タブはこの幅の倍数まで進める（端末の `git diff` と同じ）。
const TAB: usize = 8;
/// これより狭ければ、差分の行番号を出さない。
const NARROW: u16 = 50;
/// 横に動かす 1 歩。
pub const PAN: i32 = 8;

pub const DIM: Color = Color::Indexed(244);

/// 差分の色。地の色は端末の背景が暗いか明るいかで替える。文字の色は端末の 16 色で、端末のテーマに従う。
#[derive(Clone, Copy)]
pub struct Theme {
    added: Color,
    added_word: Color,
    removed: Color,
    removed_word: Color,
}

impl Theme {
    const DARK: Self = Self {
        added: Color::Indexed(22),
        added_word: Color::Indexed(28),
        removed: Color::Indexed(52),
        removed_word: Color::Indexed(88),
    };
    const LIGHT: Self = Self {
        added: Color::Indexed(194),
        added_word: Color::Indexed(157),
        removed: Color::Indexed(224),
        removed_word: Color::Indexed(217),
    };

    /// `COLORFGBG`（「文字;背景」の色番号）の背景が白なら明るい端末。わからなければ暗いとみなす。
    pub fn detect() -> Self {
        Self::from_colorfgbg(std::env::var("COLORFGBG").ok().as_deref())
    }

    fn from_colorfgbg(value: Option<&str>) -> Self {
        let background = value
            .and_then(|v| v.rsplit(';').next())
            .and_then(|b| b.parse::<u8>().ok());
        match background {
            Some(7 | 15) => Self::LIGHT,
            _ => Self::DARK,
        }
    }
}

/// 構文の種類ごとの文字の色（なければ端末の文字の色）。
fn syntax_color(kind: u32) -> Option<Color> {
    Some(match kind as u8 {
        SyntaxSpan::KEYWORD | SyntaxSpan::ATTRIBUTE => Color::Magenta,
        SyntaxSpan::STRING | SyntaxSpan::CONSTANT => Color::Yellow,
        SyntaxSpan::COMMENT => Color::DarkGray,
        SyntaxSpan::NUMBER | SyntaxSpan::ESCAPE => Color::LightRed,
        SyntaxSpan::FUNCTION | SyntaxSpan::TAG => Color::Blue,
        SyntaxSpan::TYPE | SyntaxSpan::MODULE => Color::Cyan,
        _ => return None,
    })
}

/// 窓口の差分の答え（`commitDiff`・`workingDiff`）。
pub struct Diff {
    /// 差分の行（u32 × 4: 種類と印、前の行番号、後の行番号、`text` での位置）。
    pub rows: Vec<u32>,
    lengths: Vec<u32>,
    text: Vec<u8>,
    /// 変わった単語（u32 × 3: 行、その行の文字での始まりと終わり。UTF-16 の単位）。行の順。
    highlights: Vec<u32>,
    /// 構文の色（u32 × 4: 行、始まり、終わり、種類）。行の順。届くまで空。
    syntax: Vec<u32>,
    /// ファイルの見出しの行（差分の行の番号）。
    pub file_rows: Vec<u32>,
    /// hunk の見出しの行。
    pub hunk_rows: Vec<u32>,
    /// 行番号の桁数。
    digits: u16,
    /// stage 済みの行に印をつける（HEAD と作業ツリーを比べた差分）。
    marks: bool,
}

impl Diff {
    pub fn new(reply: &Reply, marks: bool) -> Self {
        let rows = reply.u32s("rows");
        let mut highlights = reply.u32s("highlights");
        sort_spans::<3>(&mut highlights);
        let mut file_rows = Vec::new();
        let mut hunk_rows = Vec::new();
        let mut largest = 0;
        for (i, r) in rows.as_chunks::<4>().0.iter().enumerate() {
            match (r[0] & 0xff) as u8 {
                DiffRow::FILE => file_rows.push(i as u32),
                DiffRow::HUNK => hunk_rows.push(i as u32),
                _ => largest = largest.max(r[1]).max(r[2]),
            }
        }
        Self {
            rows,
            lengths: reply.u32s("textLengths"),
            text: reply.section("text").to_vec(),
            highlights,
            syntax: Vec::new(),
            file_rows,
            hunk_rows,
            digits: largest.to_string().len() as u16,
            marks,
        }
    }

    /// 行の数。
    pub fn len(&self) -> u32 {
        (self.rows.len() / 4) as u32
    }

    /// 構文の色（`diffSyntax` の答え）を入れる。
    pub fn set_syntax(&mut self, reply: &Reply) {
        self.syntax = reply.u32s("spans");
        sort_spans::<4>(&mut self.syntax);
    }

    fn row_text(&self, row: usize) -> &str {
        let start = self.rows[row * 4 + 3] as usize;
        let len = self.lengths.get(row).copied().unwrap_or(0) as usize;
        self.text
            .get(start..start + len)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }
}

/// u32 × `N` の並びを、先頭の 2 つ（行、始まり）の順に並べる。
fn sort_spans<const N: usize>(spans: &mut Vec<u32>) {
    let mut chunks: Vec<[u32; N]> = spans.as_chunks::<N>().0.to_vec();
    if chunks.is_sorted_by_key(|c| (c[0], c[1])) {
        return;
    }
    chunks.sort_unstable_by_key(|c| (c[0], c[1]));
    *spans = chunks.into_iter().flatten().collect();
}

/// `row` の行の分だけを取り出す。
fn spans_of<const N: usize>(spans: &[u32], row: u32) -> &[[u32; N]] {
    let chunks = spans.as_chunks::<N>().0;
    let start = chunks.partition_point(|c| c[0] < row);
    let end = start + chunks[start..].partition_point(|c| c[0] == row);
    &chunks[start..end]
}

/// 縦と横のスクロールの位置。
#[derive(Default)]
pub struct Scroll {
    /// 一番上に見えている行。
    pub top: u32,
    /// 差分を横に動かした幅（セル）。
    pub left: u16,
    pub height: u16,
}

impl Scroll {
    fn last_top(&self, lines: u32) -> u32 {
        lines.saturating_sub(u32::from(self.height))
    }

    pub fn clamp(&mut self, lines: u32) {
        self.top = self.top.min(self.last_top(lines));
    }

    /// 今の位置（一番下に見えている行）と、全体の行数。
    pub fn position(&self, lines: u32) -> (u32, u32) {
        ((self.top + u32::from(self.height)).min(lines), lines)
    }

    pub fn page(&self) -> i64 {
        i64::from(self.height.max(2)) - 1
    }

    pub fn by(&mut self, delta: i64, lines: u32) -> bool {
        self.to((i64::from(self.top) + delta).max(0) as u32, lines)
    }

    pub fn to(&mut self, top: u32, lines: u32) -> bool {
        let top = top.min(self.last_top(lines));
        let changed = top != self.top;
        self.top = top;
        changed
    }

    /// 横に動かす。
    pub fn pan(&mut self, delta: i32) -> bool {
        let left = (i32::from(self.left) + delta).clamp(0, i32::from(u16::MAX)) as u16;
        let changed = left != self.left;
        self.left = left;
        changed
    }
}

/// 「+12」「-3」（0 のものは出さない）。バイナリなら「binary」。
pub fn counts(added: u64, removed: u64, binary: bool) -> Vec<(String, Style)> {
    if binary {
        return vec![(tr!("binary"), Style::new().fg(DIM))];
    }
    let mut out = Vec::new();
    if added > 0 {
        out.push((format!("+{added}"), Style::new().fg(Color::Green)));
    }
    if removed > 0 {
        out.push((format!("-{removed}"), Style::new().fg(Color::Red)));
    }
    out
}

/// パスが収まらなければ、前を「…」にして後ろ（ファイル名の側）を残す。
pub fn fit_path(path: &str, width: usize) -> String {
    if path.width() <= width {
        return path.to_owned();
    }
    let mut kept = Vec::new();
    let mut used = 1;
    for ch in path.chars().rev() {
        let w = ch.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        kept.push(ch);
    }
    if width == 0 {
        return String::new();
    }
    std::iter::once('…').chain(kept.into_iter().rev()).collect()
}

/// 差分の 1 行。
pub fn draw_row(at: Rect, buf: &mut Buffer, diff: &Diff, row: usize, left: u16, theme: &Theme) {
    let r = &diff.rows[row * 4..row * 4 + 4];
    let kind = (r[0] & 0xff) as u8;
    let flags = ((r[0] >> 8) & 0xff) as u8;
    let text = diff.row_text(row);
    let dim = Style::new().fg(DIM);
    match kind {
        DiffRow::FILE => {
            // 「── パス ────── +12 -3」
            buf.set_style(at, dim);
            for x in at.x..at.right() {
                buf[(x, at.y)].set_char('─');
            }
            let counts = counts(
                u64::from(r[2]),
                u64::from(r[1]),
                flags & DiffRow::BINARY != 0,
            );
            let counts_width = counts.iter().map(|(t, _)| t.width() + 1).sum::<usize>() as u16;
            let room = at.width.saturating_sub(5 + counts_width);
            let name = format!(" {} ", fit_path(text, usize::from(room)));
            let end = buf
                .set_stringn(
                    at.x + 2,
                    at.y,
                    name,
                    usize::from(at.width.saturating_sub(2)),
                    Style::new().add_modifier(Modifier::BOLD),
                )
                .0;
            // 数は、線を 1 つ空けた右に。
            let mut x = end + 1;
            for (i, (text, style)) in counts.into_iter().enumerate() {
                if x + 1 >= at.right() {
                    break;
                }
                let text = if i == 0 {
                    format!(" {text} ")
                } else {
                    format!("{text} ")
                };
                x = buf
                    .set_stringn(x, at.y, text, usize::from(at.right() - x), style)
                    .0;
            }
            return;
        }
        DiffRow::HUNK => {
            buf.set_stringn(
                at.x,
                at.y,
                text,
                usize::from(at.width),
                Style::new().fg(Color::Cyan),
            );
            return;
        }
        _ => {}
    }
    let (sign, sign_color, line_bg, word_bg) = match kind {
        DiffRow::ADDED => ('+', Color::Green, Some(theme.added), Some(theme.added_word)),
        DiffRow::REMOVED => (
            '-',
            Color::Red,
            Some(theme.removed),
            Some(theme.removed_word),
        ),
        _ => (' ', DIM, None, None),
    };
    let base = match line_bg {
        Some(bg) => Style::new().bg(bg),
        None => Style::new(),
    };
    buf.set_style(at, base);
    let mut x = at.x;
    if at.width >= NARROW {
        let digits = usize::from(diff.digits);
        let number = |show: bool, n: u32| match show {
            true => format!("{n:>digits$} "),
            false => " ".repeat(digits + 1),
        };
        let old = number(kind != DiffRow::ADDED, r[1]);
        let new = number(kind != DiffRow::REMOVED, r[2]);
        x = buf
            .set_stringn(x, at.y, old, usize::from(at.right() - x), base.fg(DIM))
            .0;
        x = buf
            .set_stringn(x, at.y, new, usize::from(at.right() - x), base.fg(DIM))
            .0;
    }
    if diff.marks && x + 1 < at.right() {
        // stage 済みの行の印。
        if flags & DiffRow::STAGED != 0 {
            buf[(x, at.y)].set_char('●').set_fg(Color::Green);
        }
        x += 2;
    }
    if x < at.right() {
        buf[(x, at.y)].set_char(sign).set_fg(sign_color);
        x += 1;
    }
    let highlights = spans_of::<3>(&diff.highlights, row as u32);
    let syntax = spans_of::<4>(&diff.syntax, row as u32);
    let (mut h, mut s) = (0, 0);
    // 文字の位置（タブを広げ、横に動かす前のセル）。
    let mut column = 0usize;
    let left = usize::from(left);
    let room = usize::from(at.right().saturating_sub(x));
    let mut utf8 = [0u8; 4];
    // 区間の位置は、窓口の決まりで UTF-16 の単位。
    let mut next = 0u32;
    for ch in text.chars() {
        let offset = next;
        next += ch.len_utf16() as u32;
        let width = match ch {
            '\t' => TAB - column % TAB,
            _ => match ch.width() {
                // 制御文字（CRLF の CR など）は出さない。
                None => continue,
                Some(w) => w,
            },
        };
        let start = column;
        column += width;
        if column <= left || width == 0 {
            continue;
        }
        if start >= left + room {
            break;
        }
        while h < highlights.len() && highlights[h][2] <= offset {
            h += 1;
        }
        while s < syntax.len() && syntax[s][2] <= offset {
            s += 1;
        }
        let mut style = base;
        if let Some(color) = syntax
            .get(s)
            .filter(|span| span[1] <= offset)
            .and_then(|span| syntax_color(span[3]))
        {
            style = style.fg(color);
        }
        if let (Some(bg), true) = (
            word_bg,
            highlights.get(h).is_some_and(|span| span[1] <= offset),
        ) {
            style = style.bg(bg);
        }
        // 横に動かして左が切れた文字（全角・タブ）は、見える分だけ空白に。
        let visible_start = start.max(left);
        let cx = x + (visible_start - left) as u16;
        if ch == '\t' || start < left {
            for i in 0..(column - visible_start).min(left + room - visible_start) {
                buf[(cx + i as u16, at.y)].set_char(' ').set_style(style);
            }
        } else if column <= left + room {
            buf.set_string(cx, at.y, ch.encode_utf8(&mut utf8), style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// パスは後ろ（ファイル名の側）を残す
    #[test]
    fn fit_path_keeps_the_file_name() {
        assert_eq!(fit_path("src/graph.rs", 20), "src/graph.rs");
        assert_eq!(fit_path("crates/tui/src/graph.rs", 9), "…graph.rs");
    }

    /// 行ごとの区間は、その行の分だけを取り出す
    #[test]
    fn spans_of_picks_one_row() {
        let spans = [0, 1, 2, 2, 0, 3, 2, 5, 9, 4, 0, 1];
        assert_eq!(spans_of::<3>(&spans, 2), [[2, 0, 3], [2, 5, 9]]);
        assert!(spans_of::<3>(&spans, 1).is_empty());
        assert!(spans_of::<3>(&spans, 9).is_empty());
    }

    /// 書き換えた行は、変わった単語の地の色が濃く、構文の色がつく
    #[test]
    fn changed_words_and_syntax_are_painted() {
        use amonhen_front::{Front, InProcess};
        amonhen_model::testing::isolate_from_user_settings();
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = amonhen_model::process::command("git")
                .args(["-c", "user.name=Ann", "-c", "user.email=ann@example.com"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        git(&["init", "-q"]);
        std::fs::write(dir.path().join("a.rs"), "// 日本語\nlet lane = 0;\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "Start"]);
        std::fs::write(dir.path().join("a.rs"), "// 日本語\nlet lane = trunk();\n").unwrap();
        git(&["commit", "-q", "-am", "Change"]);
        let id = git(&["rev-parse", "HEAD"]);

        let front = InProcess::open(dir.path(), None).unwrap();
        let diff = front
            .call(serde_json::json!({"op": "commitDiff", "id": id}))
            .unwrap();
        let mut commit = Diff::new(&diff, false);
        let syntax = front
            .call(serde_json::json!({"op": "diffSyntax", "view": diff.json["view"]}))
            .unwrap();
        commit.set_syntax(&syntax);

        let theme = Theme::DARK;
        let row = |kind: u8| {
            let at = commit
                .rows
                .as_chunks::<4>()
                .0
                .iter()
                .position(|r| (r[0] & 0xff) as u8 == kind)
                .unwrap();
            let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1));
            draw_row(buf.area, &mut buf, &commit, at, 0, &theme);
            buf
        };
        let find =
            |buf: &Buffer, symbol: &str| (0..40).find(|&x| buf[(x, 0)].symbol() == symbol).unwrap();
        // 「-let lane = 0;」: 行の地の色に、「0」だけ濃い色。`let` はキーワード。
        let removed = row(DiffRow::REMOVED);
        let l = find(&removed, "l");
        assert_eq!(removed[(l, 0)].bg, theme.removed);
        // 構文の色は、コアの `syntax` フィーチャーがあるとき（ワークスペースのテストではある）だけ届く。
        let colored = !commit.syntax.is_empty();
        if colored {
            assert_eq!(removed[(l, 0)].fg, Color::Magenta);
        }
        assert_eq!(removed[(find(&removed, "0"), 0)].bg, theme.removed_word);
        let added = row(DiffRow::ADDED);
        assert_eq!(added[(find(&added, "u"), 0)].bg, theme.added_word);
        assert_eq!(added[(find(&added, "="), 0)].bg, theme.added);
        // 全角の文字の後ろでも、区間の位置がずれない（コメントの終わりまで同じ色）。
        let context = row(DiffRow::CONTEXT);
        if colored {
            assert_eq!(context[(find(&context, "語"), 0)].fg, Color::DarkGray);
        }
    }

    /// 背景が白（7・15）なら明るい色
    #[test]
    fn theme_follows_colorfgbg() {
        let light = |v| Theme::from_colorfgbg(v).added == Theme::LIGHT.added;
        assert!(light(Some("0;15")));
        assert!(light(Some("0;default;7")));
        assert!(!light(Some("15;0")));
        assert!(!light(None));
    }
}
