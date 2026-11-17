//! コミットの詳細: 件名とメッセージ、作者と日付、変わったファイル、差分。上から下へ 1 本の文書として並べ、
//! スクロールする。

use amonhen_front::Reply;
use amonhen_model::{Oid, tr};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::diff::{DIM, Diff, Scroll, Theme, counts, draw_row, fit_path};

use crate::Clock;
use crate::history::ago;
use crate::loader::Request;

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
    diff: Diff,
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
        Self {
            id,
            info,
            files,
            diff: Diff::new(diff, false),
            head: Vec::new(),
            head_width: 0,
        }
    }

    fn diff_rows(&self) -> u32 {
        self.diff.len()
    }

    /// 差分の前に置く行の数: 頭、ファイルの一覧、空行。
    fn before_diff(&self) -> u32 {
        let files = self.files.len() as u32;
        self.head.len() as u32 + files + u32::from(files > 0)
    }

    fn lines(&self) -> u32 {
        self.before_diff() + self.diff_rows()
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
        // このコミットを指すブランチとタグ。
        let refs: Vec<&str> = self.info["refs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !refs.is_empty() {
            fields.push((tr!("Refs"), refs.join(", ")));
        }
        // 署名があることと、その種類（検証はしていない）。
        if let Some(kind) = self.info["signed"].as_str() {
            fields.push((
                tr!("Signed"),
                tr!("{kind} (not verified here)", kind = kind.to_uppercase()),
            ));
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

#[derive(Default)]
pub struct Details {
    /// 出したいコミット。
    wanted: Option<Oid>,
    /// 出したいコミットは stash（stash としての差分を出す）。
    wanted_stash: bool,
    /// 頼んで、答えを待っているコミット。
    loading: Option<Oid>,
    commit: Option<Commit>,
    /// 読めなかったコミットと、その理由。
    failed: Option<(Oid, String)>,
    scroll: Scroll,
    width: u16,
    /// `[` `]` で移ったファイル（その後にスクロールしたら忘れる）。文書の終わり近くでは、見出しを一番上まで
    /// 動かせないので、どのファイルへ移ったかを覚えておく。
    jumped: Option<usize>,
}

impl Details {
    /// 出すコミットを替える。替わったら true。
    pub fn want(&mut self, id: Oid, stash: bool) -> bool {
        if self.wanted == Some(id) {
            return false;
        }
        self.wanted = Some(id);
        self.wanted_stash = stash;
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
            stash: self.wanted_stash,
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
                self.scroll.top = 0;
                self.scroll.left = 0;
                self.jumped = None;
                (!view.is_null()).then(|| Request::Syntax {
                    id: id.to_string(),
                    of: serde_json::json!({"view": view}),
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
            commit.diff.set_syntax(reply);
        }
    }

    /// 出す所の大きさ。幅が変わったら、頭の部分を組み直す。
    pub fn set_size(&mut self, width: u16, height: u16, clock: &Clock) {
        self.width = width;
        self.scroll.height = height;
        if let Some(commit) = &mut self.commit {
            commit.build_head(width, clock);
        }
        self.scroll.clamp(self.lines());
    }

    fn lines(&self) -> u32 {
        self.commit.as_ref().map_or(0, Commit::lines)
    }

    /// 今の位置（一番下に見えている行）と、全体の行数。
    pub fn position(&self) -> (u32, u32) {
        self.scroll.position(self.lines())
    }

    pub fn page(&self) -> i64 {
        self.scroll.page()
    }

    pub fn scroll(&mut self, delta: i64) -> bool {
        self.jumped = None;
        self.scroll.by(delta, self.lines())
    }

    /// `down` の向きに、もうスクロールできない。
    pub fn at_end(&self, down: bool) -> bool {
        if down {
            self.scroll.position(self.lines()).0 >= self.lines()
        } else {
            self.scroll.top == 0
        }
    }

    pub fn scroll_to(&mut self, top: u32) -> bool {
        self.jumped = None;
        self.scroll.to(top, self.lines())
    }

    /// 差分を横に動かす。
    pub fn pan(&mut self, delta: i32) -> bool {
        self.scroll.pan(delta)
    }

    /// 差分を横に動かしてあるか。
    pub fn panned(&self) -> bool {
        self.scroll.left > 0
    }

    /// 今見ているファイルのパスと、出しているコミット。ファイルは、見えている最初の見出しのもの。見出しが
    /// 見えていなければ、上にある最後の見出しのもの（それもなければ最初のファイル）。
    pub fn current_file(&self) -> Option<(&str, Oid)> {
        let commit = self.commit.as_ref()?;
        let before = commit.before_diff();
        let (top, bottom) = (
            self.scroll.top,
            self.scroll.top + u32::from(self.scroll.height),
        );
        let lines = commit.diff.file_rows.iter().map(|row| before + row);
        let index = self.jumped.unwrap_or_else(|| {
            lines
                .clone()
                .position(|line| top <= line && line < bottom)
                .unwrap_or_else(|| lines.filter(|&line| line < top).count().saturating_sub(1))
        });
        Some((commit.files.get(index)?.path.as_str(), commit.id))
    }

    /// 次（`forward`）か前のファイルの見出しを一番上に。
    pub fn jump_file(&mut self, forward: bool) -> bool {
        let Some(commit) = &self.commit else {
            return false;
        };
        let before = commit.before_diff();
        // 今のファイル（覚えていればそれ、なければ一番上の行より上の最後の見出し）の、次か前。
        let starts: Vec<u32> = commit.diff.file_rows.iter().map(|r| before + r).collect();
        let current = self.jumped.map(|index| index as i64).unwrap_or_else(|| {
            starts
                .iter()
                .filter(|&&line| line <= self.scroll.top)
                .count() as i64
                - 1
        });
        let target = current + if forward { 1 } else { -1 };
        let Some(&line) = usize::try_from(target).ok().and_then(|t| starts.get(t)) else {
            return false;
        };
        self.scroll_to(line);
        self.jumped = Some(target as usize);
        true
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, clock: &Clock, theme: &Theme) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.scroll.height = area.height;
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
        self.scroll.clamp(commit.lines());
        // 読めなかったコミットを選んでいる間は、理由を 1 行目に重ねる。
        let failed = self
            .failed
            .as_ref()
            .filter(|f| Some(f.0) == self.wanted)
            .map(|f| f.1.as_str());
        let head = commit.head.len() as u32;
        let before = commit.before_diff();
        for y in 0..area.height {
            let line = self.scroll.top + u32::from(y);
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
                draw_row(
                    at,
                    buf,
                    &commit.diff,
                    (line - before) as usize,
                    self.scroll.left,
                    theme,
                );
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
}
