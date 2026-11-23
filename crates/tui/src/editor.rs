//! 複数行の入力欄（コミットメッセージ）。

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style};

/// 件名の目安の文字数（アプリの 50/72 と同じ）。
pub const SUMMARY_GUIDE: usize = 50;
/// 本文の行の目安の文字数。
const BODY_GUIDE: usize = 72;
use ratatui_crossterm::crossterm::event::KeyCode;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub struct Editor {
    /// 行（少なくとも 1 つ）。
    lines: Vec<String>,
    row: usize,
    /// カーソルの位置（その行の、文字の数で）。
    column: usize,
    /// 一番上に見えている行。
    top: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            column: 0,
            top: 0,
        }
    }
}

impl Editor {
    /// 中身（終わりの空行は除く）。
    pub fn text(&self) -> String {
        self.lines.join("\n").trim_end().to_owned()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }

    /// 中身を入れ替え、カーソルを終わりに置く（終わりの空行は除く。行の終わりの空白は残す: `fix: ` の後に打てる）。
    pub fn set_text(&mut self, text: &str) {
        self.lines = (text.trim_end_matches(['\n', '\r']).split('\n'))
            .map(str::to_owned)
            .collect();
        self.row = self.lines.len() - 1;
        self.column = self.lines[self.row].chars().count();
        self.top = 0;
    }

    /// カーソルを件名（1 行目）の終わりに置く。
    pub fn move_to_summary_end(&mut self) {
        self.row = 0;
        self.column = self.lines[0].chars().count();
        self.top = 0;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// 1 行目（件名）の文字数。
    pub fn summary_length(&self) -> usize {
        self.lines[0].chars().count()
    }

    /// 今の行の、カーソルの位置のバイト。
    fn offset(&self) -> usize {
        let line = &self.lines[self.row];
        line.char_indices()
            .nth(self.column)
            .map_or(line.len(), |(i, _)| i)
    }

    fn line_length(&self) -> usize {
        self.lines[self.row].chars().count()
    }

    /// キーを入力として扱う。中身かカーソルが変わったら true。
    pub fn key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char(ch) => {
                let at = self.offset();
                self.lines[self.row].insert(at, ch);
                self.column += 1;
            }
            KeyCode::Enter => {
                let at = self.offset();
                let rest = self.lines[self.row].split_off(at);
                self.lines.insert(self.row + 1, rest);
                self.row += 1;
                self.column = 0;
            }
            KeyCode::Backspace if self.column > 0 => {
                self.column -= 1;
                let at = self.offset();
                self.lines[self.row].remove(at);
            }
            // 行の頭では、前の行につなぐ。
            KeyCode::Backspace if self.row > 0 => {
                let line = self.lines.remove(self.row);
                self.row -= 1;
                self.column = self.line_length();
                self.lines[self.row].push_str(&line);
            }
            KeyCode::Delete if self.column < self.line_length() => {
                let at = self.offset();
                self.lines[self.row].remove(at);
            }
            KeyCode::Delete if self.row + 1 < self.lines.len() => {
                let line = self.lines.remove(self.row + 1);
                self.lines[self.row].push_str(&line);
            }
            KeyCode::Left if self.column > 0 => self.column -= 1,
            KeyCode::Left if self.row > 0 => {
                self.row -= 1;
                self.column = self.line_length();
            }
            KeyCode::Right if self.column < self.line_length() => self.column += 1,
            KeyCode::Right if self.row + 1 < self.lines.len() => {
                self.row += 1;
                self.column = 0;
            }
            KeyCode::Up if self.row > 0 => {
                self.row -= 1;
                self.column = self.column.min(self.line_length());
            }
            KeyCode::Down if self.row + 1 < self.lines.len() => {
                self.row += 1;
                self.column = self.column.min(self.line_length());
            }
            KeyCode::Home => self.column = 0,
            KeyCode::End => self.column = self.line_length(),
            _ => return false,
        }
        true
    }

    /// 描いて、カーソルを置く位置を返す。長い行は、カーソルが見えるよう左を切る。
    pub fn draw(&mut self, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
        if area.width == 0 || area.height == 0 {
            return None;
        }
        let height = usize::from(area.height);
        if self.row < self.top {
            self.top = self.row;
        } else if self.row >= self.top + height {
            self.top = self.row + 1 - height;
        }
        let width = usize::from(area.width);
        let mut cursor = None;
        for (y, (index, line)) in
            (area.y..area.bottom()).zip(self.lines.iter().enumerate().skip(self.top).take(height))
        {
            // カーソルのある行は、カーソルまでの幅が収まるだけ左を切る。
            let before: usize = match index == self.row {
                true => line
                    .chars()
                    .take(self.column)
                    .map(|c| c.width().unwrap_or(0))
                    .sum(),
                false => 0,
            };
            let mut skip = (before + 1).saturating_sub(width);
            let mut shown = String::new();
            // アプリと同じ目安: 件名は 50 文字、本文の行は 72 文字。超えた分を色で知らせる。
            let limit = match index {
                0 => Some(SUMMARY_GUIDE),
                1 => None,
                _ => Some(BODY_GUIDE),
            };
            let mut over_from: Option<usize> = None;
            for (i, ch) in line.chars().enumerate() {
                let w = ch.width().unwrap_or(0);
                if skip > 0 {
                    skip = skip.saturating_sub(w);
                    continue;
                }
                if over_from.is_none() && limit.is_some_and(|l| i >= l) {
                    over_from = Some(shown.width());
                }
                shown.push(ch);
            }
            buf.set_stringn(area.x, y, &shown, width, Style::new());
            if let Some(from) = over_from.filter(|&f| f < width) {
                let at = Rect::new(
                    area.x + from as u16,
                    y,
                    (shown.width().min(width) - from) as u16,
                    1,
                );
                buf.set_style(at, Style::new().fg(Color::Yellow));
            }
            if index == self.row {
                let hidden = line.width().saturating_sub(shown.width());
                let x = (before - hidden.min(before)).min(width - 1);
                cursor = Some((area.x + x as u16, y));
            }
        }
        cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Editor {
        let mut editor = Editor::default();
        for ch in text.chars() {
            editor.key(match ch {
                '\n' => KeyCode::Enter,
                ch => KeyCode::Char(ch),
            });
        }
        editor
    }

    /// 件名は 50 文字、本文の行は 72 文字を超えた分を黄色にする（アプリの 50/72 の目安）
    #[test]
    fn guides_mark_what_goes_past_fifty_and_seventy_two() {
        let summary = "s".repeat(55);
        let body = "b".repeat(75);
        let mut editor = typed(&format!("{summary}\n\n{body}"));
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 3));
        editor.draw(buf.area, &mut buf);
        let yellow = |x: u16, y: u16| buf[(x, y)].fg == Color::Yellow;
        assert!(!yellow(49, 0) && yellow(50, 0) && yellow(54, 0));
        assert!(!yellow(71, 2) && yellow(72, 2) && yellow(74, 2));
        // 目安を超えなければ、色は付けない。
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 3));
        typed("Short\n\nBody").draw(buf.area, &mut buf);
        assert!((0..100).all(|x| buf[(x, 0)].fg != Color::Yellow));
    }

    /// 打った文字が入り、Enter で行が分かれ、Backspace で行がつながる
    #[test]
    fn typing_and_joining_lines() {
        let mut editor = typed("Fix the graph\n\nBody");
        assert_eq!(editor.text(), "Fix the graph\n\nBody");
        assert_eq!(editor.summary_length(), 13);
        for _ in 0..6 {
            editor.key(KeyCode::Backspace);
        }
        assert_eq!(editor.text(), "Fix the graph");
        editor.key(KeyCode::Home);
        editor.key(KeyCode::Delete);
        editor.key(KeyCode::Char('f'));
        assert_eq!(editor.text(), "fix the graph");
    }

    /// 全角の文字でも、文字の単位で動いて消す
    #[test]
    fn wide_characters_move_by_character() {
        let mut editor = typed("日本語");
        editor.key(KeyCode::Left);
        editor.key(KeyCode::Backspace);
        assert_eq!(editor.text(), "日語");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        // 「日」の後ろ（2 セル目の次）にカーソル。
        assert_eq!(editor.draw(buf.area, &mut buf), Some((2, 0)));
    }

    /// 空白だけなら空
    #[test]
    fn blank_text_is_empty() {
        assert!(typed("  \n ").is_empty());
        assert!(!typed("a").is_empty());
        let mut editor = typed("a");
        editor.set_text("Subject\n\nBody\n");
        assert_eq!(editor.text(), "Subject\n\nBody");
        editor.clear();
        assert!(editor.is_empty());
    }
}
