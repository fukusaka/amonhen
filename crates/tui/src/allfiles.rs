//! History の All Files（アプリのコミットの「すべてのファイル」）: コミットの時点の木（開いたフォルダだけ
//! 読む）と、選んだファイルの中身。そのコミットで変わったファイルは、変わった行に印をつけたファイル全体。

use std::collections::HashMap;

use amonhen_front::Reply;
use amonhen_model::tr;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;

use crate::diff::{DIM, Diff, Scroll, Theme, draw_row, fit_path};

/// 木の 1 行。
struct Node {
    /// 根からのパス（`/` 区切り、先頭の `/` なし）。
    path: String,
    name: String,
    depth: usize,
    folder: bool,
    open: bool,
}

pub struct AllFiles {
    /// コミット（16 進）と、題に出す短い ID と件名。
    pub oid: String,
    title: String,
    /// そのコミットで変わったファイル（パス → 「A」「M」などの 1 文字）。
    changed: HashMap<String, char>,
    rows: Vec<Node>,
    selected: usize,
    top: usize,
    /// 読んでいるフォルダ（答えが届く順）。
    pub loading: std::collections::VecDeque<String>,
    /// 右の中身: パスと差分（読めなければ理由）。
    shown: Option<(String, Result<Diff, String>)>,
    scroll: Scroll,
    /// 右の中身を動かしている。
    pub focus_content: bool,
}

impl AllFiles {
    pub fn new(oid: String, title: String, changed: HashMap<String, char>) -> Self {
        Self {
            oid,
            title,
            changed,
            rows: Vec::new(),
            selected: 0,
            top: 0,
            loading: std::collections::VecDeque::from([String::new()]),
            shown: None,
            scroll: Scroll::default(),
            focus_content: false,
        }
    }

    /// そのコミットで変わったファイル（`commitExtras` の `stats` のパス）。詳細から分かっている印はそのまま。
    pub fn set_changed(&mut self, stats: &Value) {
        for path in (stats.as_array().into_iter().flatten()).filter_map(|s| s["path"].as_str()) {
            self.changed.entry(path.to_owned()).or_insert('M');
        }
    }

    /// フォルダ `path` の中身（`treeEntries` の `info.entries`）が届いた。木に差し込む。
    pub fn entries(&mut self, path: &str, entries: &Value) {
        let (at, depth) = match path {
            "" => (0, 0),
            _ => match self.rows.iter().position(|n| n.path == path) {
                Some(i) => (i + 1, self.rows[i].depth + 1),
                None => return,
            },
        };
        let nodes: Vec<Node> = (entries.as_array().into_iter().flatten())
            .filter_map(|e| {
                let name = e["name"].as_str()?.to_owned();
                Some(Node {
                    path: match path {
                        "" => name.clone(),
                        _ => format!("{path}/{name}"),
                    },
                    name,
                    depth,
                    folder: e["kind"] == "folder",
                    open: false,
                })
            })
            .collect();
        self.rows.splice(at..at, nodes);
    }

    /// 選んでいる行のパスとフォルダか。
    pub fn selected(&self) -> Option<(&str, bool)> {
        self.rows
            .get(self.selected)
            .map(|n| (n.path.as_str(), n.folder))
    }

    /// 選んでいるフォルダを開く（中身を読むならそのパス）・畳む。
    pub fn toggle(&mut self) -> Option<String> {
        let node = self.rows.get_mut(self.selected)?;
        if !node.folder {
            return None;
        }
        node.open = !node.open;
        let (path, depth, open) = (node.path.clone(), node.depth, node.open);
        if open {
            return Some(path);
        }
        // 畳む: 下にある深い行を外す。
        let end = (self.selected + 1..self.rows.len())
            .find(|&i| self.rows[i].depth <= depth)
            .unwrap_or(self.rows.len());
        self.rows.drain(self.selected + 1..end);
        None
    }

    pub fn move_selection(&mut self, delta: i64, height: usize) {
        let last = self.rows.len().saturating_sub(1) as i64;
        self.selected = (self.selected as i64 + delta).clamp(0, last.max(0)) as usize;
        if self.selected < self.top {
            self.top = self.selected;
        } else if height > 0 && self.selected >= self.top + height {
            self.top = self.selected + 1 - height;
        }
    }

    /// 右の中身（`treeFile` の答え）。
    pub fn set_content(&mut self, path: String, reply: Result<Reply, String>) {
        let diff = reply.and_then(|r| match r.json["binary"] == true {
            true => Err(tr!("A binary file")),
            false => Ok(Diff::new(&r, false)),
        });
        self.shown = Some((path, diff));
        self.scroll = Scroll::default();
    }

    /// 右の中身を動かす。
    pub fn scroll(&mut self, delta: i64) {
        if let Some((_, Ok(diff))) = &self.shown {
            self.scroll.by(delta, diff.len());
        }
    }

    pub fn pan(&mut self, delta: i32) {
        self.scroll.pan(delta);
    }

    pub fn page(&self) -> i64 {
        self.scroll.page()
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, Style::new());
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                buf[(x, y)].reset();
            }
        }
        let title = tr!(" All Files of {commit} ", commit = self.title);
        buf.set_stringn(
            area.x + 1,
            area.y,
            &title,
            usize::from(area.width.saturating_sub(2)),
            Style::new().add_modifier(Modifier::BOLD),
        );
        let body = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(2),
        );
        let left = (area.width / 3).clamp(20.min(area.width), 50);
        let tree = Rect::new(body.x, body.y, left, body.height);
        let content = Rect::new(
            body.x + left + 1,
            body.y,
            body.width.saturating_sub(left + 1),
            body.height,
        );
        for y in body.y..body.bottom() {
            buf[(body.x + left, y)].set_char('│').set_fg(DIM);
        }
        let height = usize::from(tree.height);
        if self.selected >= self.top + height.max(1) {
            self.top = self.selected + 1 - height.max(1);
        }
        for (y, (i, node)) in
            (tree.y..tree.bottom()).zip(self.rows.iter().enumerate().skip(self.top))
        {
            let indent = (node.depth * 2) as u16;
            let mark = match (node.folder, node.open) {
                (true, true) => "▾ ",
                (true, false) => "▸ ",
                (false, _) => "  ",
            };
            let (letter, style) = match self.changed.get(&node.path) {
                Some(&c) => (c, Style::new().fg(Color::Yellow)),
                // 変わったファイルのあるフォルダも、その色で。
                None if node.folder
                    && self
                        .changed
                        .keys()
                        .any(|p| p.starts_with(&format!("{}/", node.path))) =>
                {
                    (' ', Style::new().fg(Color::Yellow))
                }
                None => (' ', Style::new()),
            };
            let text = format!("{mark}{}{}", node.name, if node.folder { "/" } else { "" });
            let x = tree.x + indent.min(tree.width.saturating_sub(4));
            buf.set_stringn(
                x,
                y,
                text,
                usize::from(tree.right().saturating_sub(x + 2)),
                style,
            );
            if letter != ' ' {
                buf[(tree.right() - 1, y)].set_char(letter).set_style(style);
            }
            if i == self.selected {
                let bg = match self.focus_content {
                    true => Color::Indexed(236),
                    false => Color::Indexed(238),
                };
                buf.set_style(Rect::new(tree.x, y, tree.width, 1), Style::new().bg(bg));
            }
        }
        match &self.shown {
            None => {
                buf.set_stringn(
                    content.x + 1,
                    content.y,
                    tr!("Enter on a file shows its contents"),
                    usize::from(content.width.saturating_sub(1)),
                    Style::new().fg(DIM),
                );
            }
            Some((path, Err(error))) => {
                buf.set_stringn(
                    content.x + 1,
                    content.y,
                    format!("{path}: {error}"),
                    usize::from(content.width.saturating_sub(1)),
                    Style::new().fg(DIM),
                );
            }
            Some((path, Ok(diff))) => {
                buf.set_stringn(
                    content.x + 1,
                    content.y,
                    fit_path(path, usize::from(content.width.saturating_sub(1))),
                    usize::from(content.width.saturating_sub(1)),
                    Style::new().add_modifier(Modifier::BOLD),
                );
                let rows = Rect::new(
                    content.x,
                    content.y + 1,
                    content.width,
                    content.height.saturating_sub(1),
                );
                self.scroll.height = rows.height;
                self.scroll.clamp(diff.len());
                for (y, row) in (rows.y..rows.bottom()).zip(self.scroll.top..diff.len()) {
                    draw_row(
                        Rect::new(rows.x, y, rows.width, 1),
                        buf,
                        diff,
                        row as usize,
                        self.scroll.left,
                        theme,
                    );
                }
            }
        }
        let hints = tr!("j/k move   Enter open   Tab contents   h/l pan   Esc close");
        buf.set_stringn(
            area.x + 1,
            area.bottom().saturating_sub(1),
            hints,
            usize::from(area.width.saturating_sub(2)),
            Style::new().fg(DIM),
        );
    }
}
