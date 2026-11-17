//! コミットの詳細: 件名とメッセージ、作者と日付、変わったファイル、差分。上から下へ 1 本の文書として並べ、
//! スクロールする。
//!
//! 差分は窓口の答え（行のレコードと文字の表）をそのまま持ち、見えている行だけをそこから描く。行ごとの文字列は
//! 作らない。

use amonhen_front::Reply;
use amonhen_layout::{DiffRow, SyntaxSpan};
use amonhen_model::{Oid, tr};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::Clock;
use crate::history::ago;
use crate::loader::Request;

/// タブはこの幅の倍数まで進める（端末の `git diff` と同じ）。
const TAB: usize = 8;
/// これより狭ければ、差分の行番号を出さない。
const NARROW: u16 = 50;
/// 横に動かす 1 歩。
pub const PAN: i32 = 8;

const DIM: Color = Color::Indexed(244);

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

struct File {
    path: String,
    old_path: Option<String>,
    /// 「added」「modified」など。
    kind: String,
    added: u64,
    removed: u64,
    binary: bool,
}

struct Commit {
    id: Oid,
    info: Value,
    files: Vec<File>,
    /// 差分の行（u32 × 4: 種類と印、前の行番号、後の行番号、`text` での位置）。
    rows: Vec<u32>,
    lengths: Vec<u32>,
    text: Vec<u8>,
    /// 変わった単語（u32 × 3: 行、その行の文字での始まりと終わり。UTF-16 の単位）。行の順。
    highlights: Vec<u32>,
    /// 構文の色（u32 × 4: 行、始まり、終わり、種類）。行の順。届くまで空。
    syntax: Vec<u32>,
    /// ファイルの見出しの行（差分の行の番号）。
    file_rows: Vec<u32>,
    /// 行番号の桁数。
    digits: u16,
    /// 頭の部分（件名から「Parents」まで）を、幅 `head_width` で組んだもの。
    head: Vec<(String, Style)>,
    head_width: u16,
}

impl Commit {
    fn new(id: Oid, info: Value, diff: &Reply) -> Self {
        let files = diff.json["files"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| File {
                path: f["path"].as_str().unwrap_or("").to_owned(),
                old_path: f["oldPath"].as_str().map(str::to_owned),
                kind: f["kind"].as_str().unwrap_or("").to_owned(),
                added: f["added"].as_u64().unwrap_or(0),
                removed: f["removed"].as_u64().unwrap_or(0),
                binary: f["binary"] == true,
            })
            .collect();
        let rows = diff.u32s("rows");
        let mut highlights = diff.u32s("highlights");
        sort_spans::<3>(&mut highlights);
        let mut file_rows = Vec::new();
        let mut largest = 0;
        for (i, r) in rows.as_chunks::<4>().0.iter().enumerate() {
            if (r[0] & 0xff) as u8 == DiffRow::FILE {
                file_rows.push(i as u32);
            } else {
                largest = largest.max(r[1]).max(r[2]);
            }
        }
        Self {
            id,
            info,
            files,
            rows,
            lengths: diff.u32s("textLengths"),
            text: diff.section("text").to_vec(),
            highlights,
            syntax: Vec::new(),
            file_rows,
            digits: largest.to_string().len() as u16,
            head: Vec::new(),
            head_width: 0,
        }
    }

    fn diff_rows(&self) -> u32 {
        (self.rows.len() / 4) as u32
    }

    /// 差分の前に置く行の数: 頭、ファイルの一覧、空行。
    fn before_diff(&self) -> u32 {
        let files = self.files.len() as u32;
        self.head.len() as u32 + files + u32::from(files > 0)
    }

    fn lines(&self) -> u32 {
        self.before_diff() + self.diff_rows()
    }

    fn row_text(&self, row: usize) -> &str {
        let start = self.rows[row * 4 + 3] as usize;
        let len = self.lengths.get(row).copied().unwrap_or(0) as usize;
        self.text
            .get(start..start + len)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    /// 頭の部分を幅 `width` で組む（幅が変わったときだけ）。
    fn build_head(&mut self, width: u16, clock: &Clock) {
        if self.head_width == width && !self.head.is_empty() {
            return;
        }
        self.head_width = width;
        let room = usize::from(width.max(1));
        let plain = Style::new();
        let dim = Style::new().fg(DIM);
        let mut head = Vec::new();
        let message = self.info["message"].as_str().unwrap_or("").trim_end();
        let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
        for line in wrap(subject, room) {
            head.push((line, plain.add_modifier(Modifier::BOLD)));
        }
        let body = body.trim_matches('\n');
        if !body.is_empty() {
            head.push((String::new(), plain));
            for paragraph in body.lines() {
                for line in wrap(paragraph, room) {
                    head.push((line, plain));
                }
            }
        }
        head.push((String::new(), plain));
        let person = |name: &str, email: &str| {
            let name = self.info[name].as_str().unwrap_or("");
            match self.info[email].as_str().filter(|e| !e.is_empty()) {
                Some(email) => format!("{name} <{email}>"),
                None => name.to_owned(),
            }
        };
        let when = |key: &str| {
            self.info[key].as_i64().map(|t| {
                format!(
                    "{} ({})",
                    (clock.local)(t),
                    tr!("{time} ago", time = ago(clock.now, t))
                )
            })
        };
        // 見出しと値。値は、一番長い見出しの 2 つ右でそろえる（セルの幅で）。
        let mut fields = Vec::new();
        let author = person("author", "authorEmail");
        fields.push((tr!("Author"), author.clone()));
        if let Some(date) = when("authorTime") {
            fields.push((tr!("Date"), date));
        }
        // コミッターは、作者と違うときだけ。
        let committer = person("committer", "committerEmail");
        if committer != author {
            fields.push((tr!("Committer"), committer));
            if let Some(date) = when("committerTime") {
                fields.push((tr!("Committed"), date));
            }
        }
        fields.push((tr!("Commit"), self.id.to_string()));
        let parents: Vec<&str> = self.info["parents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|p| &p[..p.len().min(8)])
            .collect();
        if !parents.is_empty() {
            fields.push((tr!("Parents"), parents.join(" ")));
        }
        let column = fields.iter().map(|(l, _)| l.width()).max().unwrap_or(0) + 2;
        for (label, value) in fields {
            let pad = " ".repeat(column - label.width());
            head.push((format!("{label}{pad}{value}"), dim));
        }
        head.push((String::new(), plain));
        self.head = head;
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

#[derive(Default)]
pub struct Details {
    /// 出したいコミット。
    wanted: Option<Oid>,
    /// 頼んで、答えを待っているコミット。
    loading: Option<Oid>,
    commit: Option<Commit>,
    /// 読めなかったコミットと、その理由。
    failed: Option<(Oid, String)>,
    /// 一番上に見えている行（文書の行）。
    top: u32,
    /// 差分を横に動かした幅（セル）。
    left: u16,
    width: u16,
    height: u16,
}

impl Details {
    /// 出すコミットを替える。替わったら true。
    pub fn want(&mut self, id: Oid) -> bool {
        if self.wanted == Some(id) {
            return false;
        }
        self.wanted = Some(id);
        true
    }

    /// 頼むことがあれば、その中身（答えを待っている間は、次を頼まない）。
    pub fn request(&mut self) -> Option<Request> {
        let wanted = self.wanted?;
        if self.loading.is_some()
            || self.commit.as_ref().is_some_and(|c| c.id == wanted)
            || self.failed.as_ref().is_some_and(|f| f.0 == wanted)
        {
            return None;
        }
        self.loading = Some(wanted);
        Some(Request::Commit {
            id: wanted.to_string(),
        })
    }

    /// 詳細と差分が届いた。出したいコミットのものなら入れ替え、構文の色を頼む中身を返す。
    pub fn loaded(
        &mut self,
        id: &str,
        result: std::result::Result<(Value, Reply), String>,
        clock: &Clock,
    ) -> Option<Request> {
        self.loading = None;
        let id = Oid::from_hex(id)?;
        // 待つ間に選択が動いていたら捨てる（`request` が次を頼む）。
        if self.wanted != Some(id) {
            return None;
        }
        match result {
            Ok((info, diff)) => {
                let view = diff.json["view"].clone();
                // 行数が決まるよう、頭の部分は届いたときに組む。
                let mut commit = Commit::new(id, info, &diff);
                commit.build_head(self.width, clock);
                self.commit = Some(commit);
                self.failed = None;
                self.top = 0;
                self.left = 0;
                (!view.is_null()).then(|| Request::Syntax {
                    id: id.to_string(),
                    view,
                })
            }
            Err(error) => {
                self.failed = Some((id, error));
                None
            }
        }
    }

    pub fn syntax(&mut self, id: &str, reply: Option<&Reply>) {
        let (Some(commit), Some(reply)) = (&mut self.commit, reply) else {
            return;
        };
        if Oid::from_hex(id) == Some(commit.id) {
            commit.syntax = reply.u32s("spans");
            sort_spans::<4>(&mut commit.syntax);
        }
    }

    /// 出す所の大きさ。幅が変わったら、頭の部分を組み直す。
    pub fn set_size(&mut self, width: u16, height: u16, clock: &Clock) {
        self.width = width;
        self.height = height;
        if let Some(commit) = &mut self.commit {
            commit.build_head(width, clock);
        }
        self.clamp();
    }

    fn lines(&self) -> u32 {
        self.commit.as_ref().map_or(0, Commit::lines)
    }

    fn last_top(&self) -> u32 {
        self.lines().saturating_sub(u32::from(self.height))
    }

    fn clamp(&mut self) {
        self.top = self.top.min(self.last_top());
    }

    /// 今の位置（一番下に見えている行）と、全体の行数。
    pub fn position(&self) -> (u32, u32) {
        let lines = self.lines();
        ((self.top + u32::from(self.height)).min(lines), lines)
    }

    pub fn page(&self) -> i64 {
        i64::from(self.height.max(2)) - 1
    }

    pub fn scroll(&mut self, delta: i64) -> bool {
        self.scroll_to((i64::from(self.top) + delta).max(0) as u32)
    }

    pub fn scroll_to(&mut self, top: u32) -> bool {
        let top = top.min(self.last_top());
        let changed = top != self.top;
        self.top = top;
        changed
    }

    /// 差分を横に動かす。
    pub fn pan(&mut self, delta: i32) -> bool {
        let left = (i32::from(self.left) + delta).clamp(0, i32::from(u16::MAX)) as u16;
        let changed = left != self.left;
        self.left = left;
        changed
    }

    /// 次（`forward`）か前のファイルの見出しを一番上に。
    pub fn jump_file(&mut self, forward: bool) -> bool {
        let Some(commit) = &self.commit else {
            return false;
        };
        let before = commit.before_diff();
        let mut starts = commit.file_rows.iter().map(|r| before + r);
        let target = if forward {
            starts.find(|&line| line > self.top)
        } else {
            starts.take_while(|&line| line < self.top).last()
        };
        target.is_some_and(|line| self.scroll_to(line))
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, clock: &Clock, theme: &Theme) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.height = area.height;
        let Some(commit) = &mut self.commit else {
            let (text, style) = match &self.failed {
                Some((_, error)) => (error.clone(), Style::new().fg(Color::Red)),
                None if self.wanted.is_some() => (tr!("Loading…"), Style::new().fg(DIM)),
                None => return,
            };
            buf.set_stringn(area.x, area.y, text, usize::from(area.width), style);
            return;
        };
        commit.build_head(area.width, clock);
        let commit = &*commit;
        self.top = self
            .top
            .min(commit.lines().saturating_sub(u32::from(self.height)));
        // 読めなかったコミットを選んでいる間は、理由を 1 行目に重ねる。
        let failed = self
            .failed
            .as_ref()
            .filter(|f| Some(f.0) == self.wanted)
            .map(|f| f.1.as_str());
        let head = commit.head.len() as u32;
        let before = commit.before_diff();
        for y in 0..area.height {
            let line = self.top + u32::from(y);
            if line >= commit.lines() {
                break;
            }
            let at = Rect::new(area.x, area.y + y, area.width, 1);
            if line < head {
                let (text, style) = &commit.head[line as usize];
                buf.set_stringn(at.x, at.y, text, usize::from(at.width), *style);
            } else if line < before {
                if let Some(file) = commit.files.get((line - head) as usize) {
                    draw_file(at, buf, file);
                }
            } else {
                draw_row(at, buf, commit, (line - before) as usize, self.left, theme);
            }
        }
        if let Some(error) = failed {
            let at = Rect::new(area.x, area.y, area.width, 1);
            buf.set_style(at, Style::new());
            buf.set_stringn(
                at.x,
                at.y,
                format!("{error:<width$}", width = usize::from(at.width)),
                usize::from(at.width),
                Style::new().fg(Color::Red),
            );
        }
    }
}

/// ファイルの一覧の 1 行: 種類の 1 文字、パス、増えた行と減った行。
fn draw_file(at: Rect, buf: &mut Buffer, file: &File) {
    let (letter, color) = match file.kind.as_str() {
        "added" => ('A', Color::Green),
        "deleted" => ('D', Color::Red),
        "renamed" => ('R', Color::Cyan),
        "copied" => ('C', Color::Cyan),
        _ => ('M', Color::Yellow),
    };
    buf.set_stringn(
        at.x,
        at.y,
        letter.to_string(),
        usize::from(at.width),
        Style::new().fg(color),
    );
    let counts = counts(file.added, file.removed, file.binary);
    let counts_width = counts.iter().map(|(t, _)| t.width() + 1).sum::<usize>() as u16;
    let path = match &file.old_path {
        Some(old) if old != &file.path => format!("{old} → {}", file.path),
        _ => file.path.clone(),
    };
    let room = at.width.saturating_sub(2 + counts_width);
    let end = buf
        .set_stringn(
            at.x + 2,
            at.y,
            fit_path(&path, usize::from(room)),
            usize::from(room),
            Style::new(),
        )
        .0;
    let mut x = end + 1;
    for (text, style) in counts {
        if x >= at.right() {
            break;
        }
        x = buf
            .set_stringn(x, at.y, text, usize::from(at.right() - x), style)
            .0
            + 1;
    }
}

/// 「+12」「-3」（0 のものは出さない）。バイナリなら「binary」。
fn counts(added: u64, removed: u64, binary: bool) -> Vec<(String, Style)> {
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
fn fit_path(path: &str, width: usize) -> String {
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
fn draw_row(at: Rect, buf: &mut Buffer, commit: &Commit, row: usize, left: u16, theme: &Theme) {
    let r = &commit.rows[row * 4..row * 4 + 4];
    let kind = (r[0] & 0xff) as u8;
    let flags = ((r[0] >> 8) & 0xff) as u8;
    let text = commit.row_text(row);
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
        let digits = usize::from(commit.digits);
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
    if x < at.right() {
        buf[(x, at.y)].set_char(sign).set_fg(sign_color);
        x += 1;
    }
    let highlights = spans_of::<3>(&commit.highlights, row as u32);
    let syntax = spans_of::<4>(&commit.syntax, row as u32);
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

/// `width` セルに収まるよう、空白で折り返す（空白のない長い語は途中で切る）。
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for word in text.split(' ') {
        let w = word.width();
        if used > 0 && used + 1 + w > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        if used > 0 {
            line.push(' ');
            used += 1;
        }
        if w <= width {
            line.push_str(word);
            used += w;
            continue;
        }
        for ch in word.chars() {
            let cw = ch.width().unwrap_or(0);
            if used + cw > width && used > 0 {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(ch);
            used += cw;
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 折り返しは空白で。空白のない長い語は途中で切る
    #[test]
    fn wrap_breaks_at_spaces() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefgh ij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("日本語の文", 6), ["日本語", "の文"]);
        assert_eq!(wrap("", 10), [""]);
    }

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
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.path().join("a.rs"), "// 日本語\nlet lane = 0;\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "Start"]);
        std::fs::write(dir.path().join("a.rs"), "// 日本語\nlet lane = trunk();\n").unwrap();
        git(&["commit", "-q", "-am", "Change"]);
        let id = git(&["rev-parse", "HEAD"]);

        let front = InProcess::open(dir.path(), None).unwrap();
        let info = front
            .call(serde_json::json!({"op": "run", "command": {"op": "commitInfo", "oid": id}}))
            .unwrap()
            .json["info"]
            .clone();
        let diff = front
            .call(serde_json::json!({"op": "commitDiff", "id": id}))
            .unwrap();
        let mut commit = Commit::new(Oid::from_hex(&id).unwrap(), info, &diff);
        let syntax = front
            .call(serde_json::json!({"op": "diffSyntax", "view": diff.json["view"]}))
            .unwrap();
        commit.syntax = syntax.u32s("spans");
        sort_spans::<4>(&mut commit.syntax);

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
