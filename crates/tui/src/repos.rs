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
    /// 足す探すフォルダ（この機械のパスか `ssh://host/path`）。
    AddRoot(String),
    /// 探すフォルダの深さ。
    Depth(String),
    /// 探すのをやめるか（`y`）。
    RemoveRoot,
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
    /// 場所（この機械なら None、SSH 先ならそのホスト）の下のフォルダ。`scan` は探すフォルダ（🔍）。
    Folder {
        host: Option<String>,
        path: String,
        label: String,
        depth: u16,
        scan: bool,
    },
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
    /// 探すフォルダ（この機械のものと SSH 先のもの）。
    roots: Vec<amonhen_model::bookmarks::ScanRoot>,
    /// 左の分類の一番上に見えているもの。
    category_top: usize,
    /// 印をつけた行（`v`。Fetch をまとめてする）。
    marked: std::collections::HashSet<String>,
    /// Fetch している行と、失敗した行の理由。
    fetching: std::collections::HashSet<String>,
    fetch_errors: HashMap<String, String>,
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
            roots: Vec::new(),
            category_top: 0,
            marked: Default::default(),
            fetching: Default::default(),
            fetch_errors: HashMap::new(),
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

    /// 選んでいる行に印をつける・外す（`v`）。
    pub fn toggle_mark(&mut self) {
        if let Some(path) = self.selected_path()
            && !self.marked.remove(&path)
        {
            self.marked.insert(path);
        }
    }

    /// Fetch するもの: 印をつけた行（なければ選んでいる行）。
    pub fn to_fetch(&self) -> Vec<String> {
        match self.marked.is_empty() {
            true => self.selected_path().into_iter().collect(),
            false => {
                let mut paths: Vec<String> = self.marked.iter().cloned().collect();
                paths.sort();
                paths
            }
        }
    }

    /// Fetch を始めた（印は外す）。
    pub fn fetch_started(&mut self, paths: &[String]) {
        for path in paths {
            self.fetching.insert(path.clone());
            self.fetch_errors.remove(path);
        }
        self.marked.clear();
    }

    /// Fetch が終わった。行の状態は読み直す（呼ぶ側）。
    pub fn fetched(&mut self, path: &str, error: Option<String>) {
        self.fetching.remove(path);
        self.peeks.remove(path);
        if let Some(error) = error {
            self.fetch_errors.insert(path.to_owned(), error);
        }
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

    /// 探すフォルダを替える（分類を作り直す）。
    pub fn set_roots(&mut self, roots: Vec<amonhen_model::bookmarks::ScanRoot>) {
        self.roots = roots;
        self.rebuild_categories();
    }

    /// 場所（None はこの機械）の下の分類: 探すフォルダ（🔍）と、ほかのリポジトリのあるフォルダの木（アプリと
    /// 同じ。子が 1 つだけでリポジトリのないフォルダは 1 つにまとめる）。
    fn folders(&self, host: Option<&str>) -> Vec<Category> {
        let mut out = Vec::new();
        let roots: Vec<String> = (self.roots.iter())
            .filter(|r| place_of(&r.path).0.as_deref() == host)
            .map(|r| place_of(&r.path).1)
            .collect();
        for root in &roots {
            out.push(Category::Folder {
                host: host.map(str::to_owned),
                path: root.clone(),
                label: format!("🔍 {}", short_home(root)),
                depth: 0,
                scan: true,
            });
        }
        // 探すフォルダの下にないリポジトリの、親のフォルダの木。
        let dirs: Vec<String> = (self.entries.iter())
            .filter(|e| e.host() == host)
            .map(|e| place_of(&e.path).1)
            .filter(|p| !roots.iter().any(|r| under(p, r)))
            .filter_map(|p| parent(&p))
            .collect();
        for (path, label, depth) in folder_tree(&dirs) {
            out.push(Category::Folder {
                host: host.map(str::to_owned),
                label: match depth {
                    0 => short_home(&label),
                    _ => label,
                },
                path,
                depth,
                scan: false,
            });
        }
        out
    }

    fn rebuild_categories(&mut self) {
        let current = self.categories.get(self.category).cloned();
        let mut categories = vec![Category::All, Category::Favorites, Category::ThisMachine];
        categories.extend(self.folders(None));
        let mut hosts: Vec<String> = (self.entries.iter())
            .filter_map(|e| e.host().map(str::to_owned))
            .chain(self.roots.iter().filter_map(|r| place_of(&r.path).0))
            .collect();
        hosts.sort();
        hosts.dedup();
        for host in hosts {
            let folders = self.folders(Some(&host));
            categories.push(Category::Host(host));
            categories.extend(folders);
        }
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

    /// 左の分類の数と、`index` 番目の名前（テストで並びを見る）。
    #[cfg(test)]
    pub fn category_count(&self) -> usize {
        self.categories.len()
    }

    #[cfg(test)]
    pub fn category_label(&self, index: usize) -> String {
        self.category_name(&self.categories[index])
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
            Category::Folder { host, path, .. } => {
                entry.host() == host.as_deref() && under(&place_of(&entry.path).1, path)
            }
        }
    }

    /// 選んでいる分類が探すフォルダなら、その場所（`ssh://` を付けた形）。
    pub fn selected_root(&self) -> Option<String> {
        match self.categories.get(self.category)? {
            Category::Folder {
                host,
                path,
                scan: true,
                ..
            } => Some(match host {
                Some(host) => self
                    .roots
                    .iter()
                    .find(|r| place_of(&r.path) == (Some(host.clone()), path.clone()))
                    .map_or_else(|| format!("ssh://{host}{path}"), |r| r.path.clone()),
                None => path.clone(),
            }),
            _ => None,
        }
    }

    /// 選んでいる分類の場所（探すフォルダを足すとき、初めに入れる）。
    pub fn selected_place(&self) -> String {
        match self.categories.get(self.category) {
            Some(Category::Host(host)) => format!("ssh://{host}/"),
            Some(Category::Folder {
                host: Some(host), ..
            }) => format!("ssh://{host}/"),
            _ => String::new(),
        }
    }

    /// 探すフォルダ `location` の深さ。
    pub fn root_depth(&self, location: &str) -> Option<u32> {
        self.roots
            .iter()
            .find(|r| r.path == location)
            .map(|r| r.depth)
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
        if self.fetching.contains(&entry.path) {
            return (tr!("fetching…"), Style::new().fg(Color::Cyan));
        }
        if self.fetch_errors.contains_key(&entry.path) {
            return (tr!("fetch failed"), Style::new().fg(Color::Red));
        }
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
            Category::Folder { label, .. } => label.clone(),
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
        // 分類が多ければ、選んでいるものが見えるよう動かす。
        let rows = usize::from(body.height.max(1));
        if self.category < self.category_top {
            self.category_top = self.category;
        } else if self.category >= self.category_top + rows {
            self.category_top = self.category + 1 - rows;
        }
        for (i, (category, y)) in self
            .categories
            .iter()
            .enumerate()
            .skip(self.category_top)
            .map(|(i, c)| (c, i))
            .zip(body.y..body.bottom())
            .map(|((c, i), y)| (i, (c, y)))
        {
            let indent = match category {
                Category::Host(_) => 2,
                Category::Folder { host, depth, .. } => {
                    2 + u16::from(host.is_some()) * 2 + depth * 2
                }
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
            // 印をつけた行（History の印と同じ色）。
            if self.marked.contains(&entry.path) {
                buf.set_style(
                    Rect::new(list.x, y, list.width, 1),
                    Style::new().bg(Color::Indexed(24)),
                );
            }
            if i == self.selected {
                let bg = match (self.focus_categories, self.marked.contains(&entry.path)) {
                    (_, true) => Color::Indexed(31),
                    (true, false) => Color::Indexed(236),
                    (false, false) => Color::Indexed(238),
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
            (Some(Prompt::AddRoot(text)), _) => tr!(
                "Add a folder to scan (a path on this machine or ssh://host/path): {text}",
                text = text
            ),
            (Some(Prompt::Depth(text)), _) => {
                tr!("How many levels down to look: {text}", text = text)
            }
            (Some(Prompt::RemoveRoot), _) => tr!(
                "Stop scanning this folder? Its repositories leave the list unless added.   y stop   n cancel"
            ),
            (None, true) => format!("/{}", self.filter),
            (None, false) if self.focus_categories => tr!(
                "j/k move   Enter list   a add a folder to scan   on 🔍: d depth  r rescan  x stop   Esc close"
            ),
            (None, false) => tr!(
                "j/k move   Enter open   v mark   f fetch   t shell   e editor   o files   * favorite   n name   c color   g group   a add   x remove   / filter   s sort   Esc close"
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

/// 場所を、ホスト（この機械なら None）とその上のパスに分ける。
fn place_of(location: &str) -> (Option<String>, String) {
    let plain = location.strip_prefix("daemon:").unwrap_or(location);
    match plain.strip_prefix("ssh://") {
        Some(rest) => {
            let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            let host = host.rsplit('@').next().unwrap_or(host);
            (Some(host.to_owned()), path.to_owned())
        }
        None => (None, plain.to_owned()),
    }
}

/// `path` が `folder` か、その下か。
fn under(path: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches(['/', '\\']);
    path == folder || (path.starts_with(folder) && path[folder.len()..].starts_with(['/', '\\']))
}

/// 親のフォルダ。
fn parent(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let at = trimmed.rfind(['/', '\\'])?;
    Some(match at {
        0 => trimmed[..1].to_owned(),
        _ => trimmed[..at].to_owned(),
    })
}

/// 家のフォルダを `~` に。
fn short_home(path: &str) -> String {
    match std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
        Some(home) if under(path, &home) => {
            format!("~{}", &path[home.trim_end_matches('/').len()..])
        }
        _ => path.to_owned(),
    }
}

/// リポジトリのあるフォルダ `dirs` の木（パス、出す名前、深さ）。子が 1 つだけで、それ自体はリポジトリのある
/// フォルダでないものは、子と 1 つにまとめる（`~/prj/oss` のように）。
fn folder_tree(dirs: &[String]) -> Vec<(String, String, u16)> {
    #[derive(Default)]
    struct Node {
        children: std::collections::BTreeMap<String, Node>,
        holds: bool,
    }
    let mut root = Node::default();
    let separator = if dirs.iter().any(|d| d.contains('\\') && !d.contains('/')) {
        '\\'
    } else {
        '/'
    };
    for dir in dirs {
        let mut node = &mut root;
        for part in dir.split(['/', '\\']) {
            node = node.children.entry(part.to_owned()).or_default();
        }
        node.holds = true;
    }
    fn emit(
        node: &Node,
        prefix: &str,
        depth: u16,
        sep: char,
        out: &mut Vec<(String, String, u16)>,
    ) {
        for (name, child) in &node.children {
            let mut path = match prefix.is_empty() && name.is_empty() {
                true => sep.to_string(),
                false => match prefix.is_empty() {
                    true => name.clone(),
                    false => format!("{}{sep}{name}", prefix.trim_end_matches(sep)),
                },
            };
            let mut label = path.clone();
            if depth > 0 {
                label = name.clone();
            }
            let mut child = child;
            while !child.holds && child.children.len() == 1 {
                let (next, grand) = child.children.iter().next().expect("one child");
                path = format!("{}{sep}{next}", path.trim_end_matches(sep));
                label = format!("{}{sep}{next}", label.trim_end_matches(sep));
                child = grand;
            }
            out.push((path.clone(), label, depth));
            emit(child, &path, depth + 1, sep, out);
        }
    }
    let mut out = Vec::new();
    emit(&root, "", 0, separator, &mut out);
    out
}
