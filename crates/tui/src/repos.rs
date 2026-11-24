//! リポジトリのページ（アプリの Repositories）: 左に分類（All・Favorites・この機械・SSH 先・グループ）、右に
//! その分類のリポジトリと状態の列（名前と場所、ブランチ、状態、最後のコミット、開いた日時）。中身は
//! アプリと同じ `bookmarks.json`・`recent.json` と、探すフォルダの下で見つけたもの。行の状態は裏で読む。

use std::collections::HashMap;

use amonhen_backend_cli::Peek;
use amonhen_model::tr;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::diff::DIM;
use crate::history::{ago, fit};

/// 一覧の 1 つのリポジトリ。
#[derive(Clone, Debug)]
pub struct Entry {
    /// 場所（この機械のパス・`ssh://`・`daemon:`）。
    pub path: String,
    pub name: String,
    pub favorite: bool,
    pub group: Option<String>,
    /// 探すフォルダの下で見つけたもの（登録も、開いた記録もない）。
    pub scanned: bool,
    /// 色ラベル（`amonhen_model::bookmarks::COLORS` の名前）。
    pub color: Option<String>,
    /// 最後に開いた日時（UNIX 時間の秒）。
    pub opened: Option<i64>,
}

/// 一番下の行で聞いていること（打っている語）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// 表示名（空なら場所の最後の部分に戻す）。
    Name(String),
    /// グループ（空ならグループから外す）。
    Group(String),
    /// 足すリポジトリの場所（この機械のパスか `ssh://host/path`）。
    Add(String),
    /// 色ラベル（`1`〜`7`、`0` で外す）。
    Color,
    /// 一覧から外すか（`y`）。
    Remove,
}

impl Entry {
    /// SSH 先なら、そのホスト。
    fn host(&self) -> Option<&str> {
        let path = self.path.strip_prefix("daemon:").unwrap_or(&self.path);
        let rest = path.strip_prefix("ssh://")?;
        let host = rest.split('/').next()?;
        Some(host.rsplit('@').next().unwrap_or(host))
    }

    /// 状態を手元で読めるか（SSH 先・daemon 越しのものは、開いたときだけ読む。アプリと同じ）。
    pub fn is_local(&self) -> bool {
        !self.path.starts_with("ssh://") && !self.path.starts_with("daemon:")
    }
}

/// 左の分類。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Category {
    All,
    Favorites,
    ThisMachine,
    Host(String),
    Group(String),
}

/// 並べる列（`s` で順に替える）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sort {
    #[default]
    Opened,
    Name,
    Branch,
    Status,
    LastCommit,
}

impl Sort {
    fn next(self) -> Self {
        match self {
            Sort::Opened => Sort::Name,
            Sort::Name => Sort::Branch,
            Sort::Branch => Sort::Status,
            Sort::Status => Sort::LastCommit,
            Sort::LastCommit => Sort::Opened,
        }
    }
}

pub struct Repos {
    entries: Vec<Entry>,
    /// 行の状態（読めなかったものは None）。まだ読んでいないものはない。
    peeks: HashMap<String, Option<Peek>>,
    categories: Vec<Category>,
    category: usize,
    selected: usize,
    top: usize,
    sort: Sort,
    /// 左の分類を動かしている。
    pub focus_categories: bool,
    /// 名前と場所で絞る語（`/`）。
    pub filter: String,
    pub typing_filter: bool,
    /// 一番下の行で聞いていること。
    pub prompt: Option<Prompt>,
}

impl Repos {
    pub fn new(entries: Vec<Entry>) -> Self {
        let mut repos = Self {
            entries: Vec::new(),
            peeks: HashMap::new(),
            categories: Vec::new(),
            category: 0,
            selected: 0,
            top: 0,
            sort: Sort::default(),
            focus_categories: false,
            filter: String::new(),
            typing_filter: false,
            prompt: None,
        };
        repos.add(entries);
        repos
    }

    /// ファイルから読み直した一覧に替える（行の状態・分類・選んでいる行はそのまま。探して見つけたものは残す）。
    pub fn replace(&mut self, entries: Vec<Entry>) {
        let scanned: Vec<Entry> = (self.entries.iter())
            .filter(|e| e.scanned && !entries.iter().any(|n| n.path == e.path))
            .cloned()
            .collect();
        self.entries.clear();
        self.add(entries);
        self.add(scanned);
        let last = self.rows().len().saturating_sub(1);
        self.selected = self.selected.min(last);
    }

    /// 選んでいる行。
    pub fn selected_entry(&self) -> Option<Entry> {
        self.rows().get(self.selected).map(|e| (*e).clone())
    }

    /// リポジトリを足す（もうある場所は足さない。`daemon:` の付いた同じ場所も）。
    pub fn add(&mut self, entries: Vec<Entry>) {
        for entry in entries {
            let plain = entry.path.strip_prefix("daemon:").unwrap_or(&entry.path);
            if self.entries.iter().any(|e| {
                e.path == entry.path
                    || e.path == plain
                    || e.path.strip_prefix("daemon:") == Some(entry.path.as_str())
            }) {
                continue;
            }
            self.entries.push(entry);
        }
        self.rebuild_categories();
    }

    fn rebuild_categories(&mut self) {
        let current = self.categories.get(self.category).cloned();
        let mut categories = vec![Category::All, Category::Favorites, Category::ThisMachine];
        let mut hosts: Vec<String> = (self.entries.iter())
            .filter_map(|e| e.host().map(str::to_owned))
            .collect();
        hosts.sort();
        hosts.dedup();
        categories.extend(hosts.into_iter().map(Category::Host));
        let mut groups: Vec<String> = (self.entries.iter())
            .filter_map(|e| e.group.clone())
            .collect();
        groups.sort();
        groups.dedup();
        categories.extend(groups.into_iter().map(Category::Group));
        self.categories = categories;
        self.category = current
            .and_then(|c| self.categories.iter().position(|x| *x == c))
            .unwrap_or(0);
    }

    /// 状態を手元で読むリポジトリの場所。
    pub fn local_paths(&self) -> Vec<String> {
        (self.entries.iter())
            .filter(|e| e.is_local())
            .map(|e| e.path.clone())
            .collect()
    }

    /// 行の状態が届いた。
    pub fn set_peek(&mut self, path: String, peek: Option<Peek>) {
        self.peeks.insert(path, peek);
    }

    fn in_category(&self, entry: &Entry, category: &Category) -> bool {
        match category {
            Category::All => true,
            Category::Favorites => entry.favorite,
            Category::ThisMachine => entry.host().is_none(),
            Category::Host(host) => entry.host() == Some(host.as_str()),
            Category::Group(group) => entry.group.as_deref() == Some(group.as_str()),
        }
    }

    /// 今の分類と絞り込みの行（並べた順）。
    fn rows(&self) -> Vec<&Entry> {
        let Some(category) = self.categories.get(self.category) else {
            return Vec::new();
        };
        let filter = self.filter.to_lowercase();
        let mut rows: Vec<&Entry> = (self.entries.iter())
            .filter(|e| self.in_category(e, category))
            .filter(|e| {
                filter.is_empty()
                    || e.name.to_lowercase().contains(&filter)
                    || e.path.to_lowercase().contains(&filter)
            })
            .collect();
        let peek = |e: &Entry| self.peeks.get(&e.path).cloned().flatten();
        match self.sort {
            // 開いた順（新しいものを先に。開いたことのないものは後ろ）。アプリの既定。
            Sort::Opened => rows.sort_by_key(|e| std::cmp::Reverse(e.opened.unwrap_or(i64::MIN))),
            Sort::Name => rows.sort_by_key(|e| e.name.to_lowercase()),
            Sort::Branch => {
                rows.sort_by_key(|e| peek(e).and_then(|p| p.branch).unwrap_or_default())
            }
            // 変更の多いもの、上流との差の大きいものを先に。
            Sort::Status => rows.sort_by_key(|e| {
                std::cmp::Reverse(peek(e).map_or(0, |p| p.changed + p.ahead + p.behind))
            }),
            Sort::LastCommit => rows.sort_by_key(|e| {
                std::cmp::Reverse(
                    peek(e)
                        .and_then(|p| p.last_commit)
                        .map_or(i64::MIN, |c| c.0),
                )
            }),
        }
        rows
    }

    /// 選んでいるリポジトリの場所。
    pub fn selected_path(&self) -> Option<String> {
        self.rows().get(self.selected).map(|e| e.path.clone())
    }

    pub fn move_selection(&mut self, delta: i64) {
        if self.focus_categories {
            let last = self.categories.len().saturating_sub(1) as i64;
            self.category = (self.category as i64 + delta).clamp(0, last) as usize;
            self.selected = 0;
            self.top = 0;
            return;
        }
        let last = self.rows().len().saturating_sub(1) as i64;
        self.selected = (self.selected as i64 + delta).clamp(0, last.max(0)) as usize;
    }

    pub fn next_sort(&mut self) {
        self.sort = self.sort.next();
        self.selected = 0;
        self.top = 0;
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.selected = 0;
        self.top = 0;
    }

    /// 状態の列の文字（アプリと同じ: 変更の数、上流との差、何もなければ ✓。SSH 先は SSH）。
    fn status(&self, entry: &Entry) -> (String, Style) {
        if !entry.is_local() {
            return ("SSH".to_owned(), Style::new().fg(DIM));
        }
        match self.peeks.get(&entry.path) {
            None => ("…".to_owned(), Style::new().fg(DIM)),
            Some(None) => ("?".to_owned(), Style::new().fg(DIM)),
            Some(Some(p)) => {
                let mut parts = Vec::new();
                if p.changed > 0 {
                    parts.push(tr!("changes {count}", count = p.changed));
                }
                if p.ahead > 0 {
                    parts.push(format!("↑{}", p.ahead));
                }
                if p.behind > 0 {
                    parts.push(format!("↓{}", p.behind));
                }
                match parts.is_empty() {
                    true => ("✓".to_owned(), Style::new().fg(Color::Green)),
                    false => (parts.join(" "), Style::new().fg(Color::Yellow)),
                }
            }
        }
    }

    fn category_name(&self, category: &Category) -> String {
        match category {
            Category::All => tr!("All"),
            Category::Favorites => tr!("Favorites"),
            Category::ThisMachine => tr!("This Machine"),
            Category::Host(host) => host.clone(),
            Category::Group(group) => group.clone(),
        }
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, now: i64) {
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                buf[(x, y)].reset();
            }
        }
        buf.set_stringn(
            area.x + 1,
            area.y,
            tr!(" Repositories "),
            usize::from(area.width.saturating_sub(2)),
            Style::new().add_modifier(Modifier::BOLD),
        );
        let body = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(2),
        );
        let left = 24.min(area.width / 3);
        for y in body.y..body.bottom() {
            buf[(body.x + left, y)].set_char('│').set_fg(DIM);
        }
        // 左: 分類と数。
        let counts: Vec<usize> = (self.categories.iter())
            .map(|c| {
                self.entries
                    .iter()
                    .filter(|e| self.in_category(e, c))
                    .count()
            })
            .collect();
        for (i, (category, y)) in self
            .categories
            .iter()
            .zip(body.y..body.bottom())
            .enumerate()
        {
            let indent = match category {
                Category::Host(_) => 2,
                _ => 0,
            };
            let name = match category {
                Category::Group(_) => format!("▪ {}", self.category_name(category)),
                _ => self.category_name(category),
            };
            let count = counts[i].to_string();
            let room = usize::from(left.saturating_sub(indent + count.len() as u16 + 2));
            buf.set_stringn(body.x + 1 + indent, y, fit(&name, room), room, Style::new());
            buf.set_string(
                body.x + left - 1 - count.len() as u16,
                y,
                &count,
                Style::new().fg(DIM),
            );
            if i == self.category {
                let bg = match self.focus_categories {
                    true => Color::Indexed(238),
                    false => Color::Indexed(236),
                };
                buf.set_style(Rect::new(body.x, y, left, 1), Style::new().bg(bg));
            }
        }
        // 右: 見出しと行。
        let list = Rect::new(
            body.x + left + 1,
            body.y,
            body.width.saturating_sub(left + 1),
            body.height,
        );
        let height = usize::from(list.height.saturating_sub(1));
        if self.selected >= self.top + height.max(1) {
            self.top = self.selected + 1 - height.max(1);
        } else if self.selected < self.top {
            self.top = self.selected;
        }
        let rows = self.rows();
        let width = list.width;
        // 列の幅: 開いた日時 8（見出しの分）、最後のコミット 24（日時と件名）、状態 16、ブランチ 16、残りを名前と場所。
        let (opened_w, last_w, status_w, branch_w) = (8u16, 24u16, 16u16, 16u16);
        let fixed = opened_w + last_w + status_w + branch_w + 4;
        let name_w = width.saturating_sub(fixed + 1).max(10);
        let x_branch = list.x + 1 + name_w + 1;
        let x_status = x_branch + branch_w + 1;
        let x_last = x_status + status_w + 1;
        let x_opened = x_last + last_w + 1;
        let header = Style::new().fg(DIM).add_modifier(Modifier::BOLD);
        let mark = |sort: Sort, text: String| match self.sort == sort {
            true => format!("{text} ▾"),
            false => text,
        };
        for (x, sort, text) in [
            (list.x + 1, Sort::Name, tr!("Name")),
            (x_branch, Sort::Branch, tr!("Branch")),
            (x_status, Sort::Status, tr!("Status")),
            (x_last, Sort::LastCommit, tr!("Last Commit")),
            (x_opened, Sort::Opened, tr!("Opened")),
        ] {
            if x < list.right() {
                let room = usize::from(list.right() - x);
                buf.set_stringn(x, list.y, mark(sort, text), room, header);
            }
        }
        if rows.is_empty() {
            buf.set_stringn(
                list.x + 1,
                list.y + 1,
                tr!("No repositories here"),
                usize::from(width.saturating_sub(1)),
                Style::new().fg(DIM),
            );
        }
        for ((i, entry), y) in rows
            .iter()
            .enumerate()
            .skip(self.top)
            .zip(list.y + 1..list.bottom())
        {
            let peek = self.peeks.get(&entry.path).cloned().flatten();
            let star = if entry.favorite { "★ " } else { "" };
            let name = format!("{star}{}", entry.name);
            // 色ラベルは、名前の前に色の点。
            let mut x = list.x + 1;
            if let Some(color) = entry.color.as_deref().and_then(label_color) {
                buf.set_string(x, y, "●", Style::new().fg(color));
                x += 2;
            }
            let room = usize::from((list.x + 1 + name_w).saturating_sub(x));
            let end = buf
                .set_stringn(x, y, fit(&name, room), room, Style::new())
                .0;
            // 名前の後ろに、薄く場所。
            if end + 2 < list.x + 1 + name_w {
                let room = usize::from(list.x + 1 + name_w - end - 2);
                buf.set_stringn(
                    end + 2,
                    y,
                    fit(&entry.path, room),
                    room,
                    Style::new().fg(DIM),
                );
            }
            let mut put = |x: u16, w: u16, text: &str, style: Style| {
                if x < list.right() {
                    let room = usize::from(w.min(list.right() - x));
                    buf.set_stringn(x, y, fit(text, room), room, style);
                }
            };
            let branch = peek
                .as_ref()
                .and_then(|p| p.branch.clone())
                .unwrap_or_default();
            put(
                x_branch,
                branch_w,
                &branch,
                Style::new().fg(Color::Indexed(114)),
            );
            let (status, style) = self.status(entry);
            put(x_status, status_w, &status, style);
            if let Some((time, subject)) = peek.as_ref().and_then(|p| p.last_commit.clone()) {
                let when = ago(now, time);
                let text = format!("{when:>4} {subject}");
                put(x_last, last_w, &text, Style::new());
            }
            let opened = entry.opened.map_or("—".to_owned(), |t| ago(now, t));
            let pad = 4u16.saturating_sub(opened.width() as u16);
            put(x_opened + pad, opened_w, &opened, Style::new().fg(DIM));
            if i == self.selected {
                let bg = match self.focus_categories {
                    true => Color::Indexed(236),
                    false => Color::Indexed(238),
                };
                buf.set_style(Rect::new(list.x, y, list.width, 1), Style::new().bg(bg));
            }
        }
        let hints = match (&self.prompt, self.typing_filter) {
            (Some(Prompt::Name(text)), _) => {
                tr!("Display name (empty for the folder’s): {text}", text = text)
            }
            (Some(Prompt::Group(text)), _) => tr!("Group (empty for none): {text}", text = text),
            (Some(Prompt::Add(text)), _) => tr!(
                "Add a repository (a path on this machine or ssh://host/path): {text}",
                text = text
            ),
            (Some(Prompt::Color), _) => tr!(
                "Color label: 1 red  2 orange  3 yellow  4 green  5 blue  6 purple  7 gray  0 none"
            ),
            (Some(Prompt::Remove), _) => {
                tr!("Remove it from the list? The repository itself stays.   y remove   n cancel")
            }
            (None, true) => format!("/{}", self.filter),
            (None, false) => tr!(
                "j/k move   Enter open   * favorite   n name   c color   g group   a add   x remove   / filter   s sort   Esc close"
            ),
        };
        buf.set_stringn(
            area.x + 1,
            area.bottom().saturating_sub(1),
            hints,
            usize::from(area.width.saturating_sub(2)),
            Style::new().fg(DIM),
        );
    }
}

/// 色ラベルの端末の色。
fn label_color(name: &str) -> Option<Color> {
    Some(match name {
        "red" => Color::Red,
        "orange" => Color::Indexed(208),
        "yellow" => Color::Yellow,
        "green" => Color::Green,
        "blue" => Color::Blue,
        "purple" => Color::Magenta,
        "gray" => Color::Indexed(244),
        _ => return None,
    })
}
