//! History の右の列の「すべてのファイル」（macOS 版の `CommitFilesBrowser`、GTK4 版の `commitbrowser.rs`）: その
//! 時点のリポジトリ全体の木（ツリーかリスト）。フォルダの中は開いたときに読む（`treeEntries`）。変更ファイルには
//! 種類の印、変更を含むフォルダには橙の点を付け、このコミットで消したファイルは赤で足す。ファイルを押すと、その
//! 時点の中身（コミットで変えた行に印）を差分の欄に出し、上にパスの帯（戻る・この変更を見る）を出す。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::{ChangeKind, FileChange, Oid, tr};
use windows_reactor::*;

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

fn name_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, n)| n)
}

/// 木の 1 つ（フォルダかファイル）。
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub kind: Option<ChangeKind>,
    pub contains_changes: bool,
}

/// 一覧の 1 行（木の深さと、その木の 1 つ）。
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub depth: usize,
    pub node: Node,
}

/// その時点の木と、開いているフォルダ・ファイル。
pub struct Browser {
    repo: Arc<Repository>,
    pub commit: Oid,
    changes: Vec<FileChange>,
    /// 変更ファイル（パス → 番号）と、変更を含むフォルダ、このコミットで消したファイル（親のフォルダ → パス）。
    changed: HashMap<String, usize>,
    changed_folders: HashSet<String>,
    deleted: HashMap<String, Vec<String>>,
    /// 読んだフォルダの中身（フォルダのパス → 子。根は ""）。
    children: HashMap<String, Vec<Node>>,
    pub expanded: HashSet<String>,
    /// 開いているファイル。
    pub open: Option<String>,
    /// 今の一覧の行（`refresh` で作り直す）。
    pub rows: Vec<Row>,
}

impl Browser {
    /// コミット `commit`（変更 `changes`）の木。変更を含むフォルダは開いておく（macOS 版と同じ）。
    pub fn new(repo: Arc<Repository>, commit: Oid, changes: Vec<FileChange>) -> Self {
        let mut changed = HashMap::new();
        let mut changed_folders = HashSet::new();
        let mut deleted: HashMap<String, Vec<String>> = HashMap::new();
        for (i, c) in changes.iter().enumerate() {
            changed.insert(c.path.clone(), i);
            let mut folder = parent_of(&c.path);
            loop {
                changed_folders.insert(folder.to_owned());
                if folder.is_empty() {
                    break;
                }
                folder = parent_of(folder);
            }
            if c.kind == ChangeKind::Deleted {
                deleted
                    .entry(parent_of(&c.path).to_owned())
                    .or_default()
                    .push(c.path.clone());
            }
        }
        let mut browser = Self {
            repo,
            commit,
            changes,
            changed,
            changed_folders,
            deleted,
            children: HashMap::new(),
            expanded: HashSet::new(),
            open: None,
            rows: Vec::new(),
        };
        browser.expanded = browser
            .changed_folders
            .iter()
            .filter(|f| !f.is_empty())
            .cloned()
            .collect();
        browser
    }

    /// `path` の変更（このコミットで変えたファイルなら）。
    pub fn change_of(&self, path: &str) -> Option<&FileChange> {
        self.changes.get(*self.changed.get(path)?)
    }

    /// 変更ファイルの中での番号（この変更を見るとき）。
    pub fn change_index(&self, path: &str) -> Option<usize> {
        self.changed.get(path).copied()
    }

    /// フォルダの中を読む（まだなら）。このコミットで消したファイル・フォルダも足す。
    fn load(&mut self, folder: &str) {
        if self.children.contains_key(folder) {
            return;
        }
        let request = serde_json::json!({
            "op": "treeEntries",
            "oid": self.commit.to_string(),
            "path": folder,
        });
        let response: serde_json::Value =
            serde_json::from_str(&self.repo.run_json(&request.to_string())).unwrap_or_default();
        let mut nodes: Vec<Node> = response["info"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let name = e["name"].as_str().unwrap_or_default().to_owned();
                let path = if folder.is_empty() {
                    name.clone()
                } else {
                    format!("{folder}/{name}")
                };
                let is_dir = e["kind"] == "folder";
                let kind = self.change_of(&path).map(|c| c.kind);
                let contains_changes =
                    kind.is_some() || (is_dir && self.changed_folders.contains(&path));
                Node {
                    name,
                    path,
                    is_dir,
                    kind,
                    contains_changes,
                }
            })
            .collect();
        for path in self.deleted.get(folder).into_iter().flatten() {
            nodes.push(Node {
                name: name_of(path).to_owned(),
                path: path.clone(),
                is_dir: false,
                kind: Some(ChangeKind::Deleted),
                contains_changes: true,
            });
        }
        // 消したフォルダ（このコミットで中のファイルをすべて消した）も、変更を含むフォルダとして足す。
        let names: HashSet<String> = nodes.iter().map(|n| n.name.clone()).collect();
        for f in &self.changed_folders {
            if !f.is_empty() && parent_of(f) == folder && !names.contains(name_of(f)) {
                nodes.push(Node {
                    name: name_of(f).to_owned(),
                    path: f.clone(),
                    is_dir: true,
                    kind: None,
                    contains_changes: true,
                });
            }
        }
        nodes.sort_by(|a, b| {
            (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase()))
        });
        self.children.insert(folder.to_owned(), nodes);
    }

    /// 一覧の行を作り直す（木かリストか）。
    pub fn refresh(&mut self, flat: bool) {
        self.rows = self.make_rows(flat);
    }

    /// 一覧の行: 木なら開いているフォルダの中まで、リストならすべてのファイル（パスの順）。
    fn make_rows(&mut self, flat: bool) -> Vec<Row> {
        let mut out = Vec::new();
        if flat {
            let mut stack = vec![String::new()];
            while let Some(folder) = stack.pop() {
                self.load(&folder);
                for node in self.children[&folder].clone() {
                    if node.is_dir {
                        stack.push(node.path.clone());
                    } else {
                        out.push(Row { depth: 0, node });
                    }
                }
            }
            out.sort_by_key(|r| r.node.path.to_lowercase());
            return out;
        }
        self.walk("", 0, &mut out);
        out
    }

    fn walk(&mut self, folder: &str, depth: usize, out: &mut Vec<Row>) {
        self.load(folder);
        for node in self.children[folder].clone() {
            let open = node.is_dir && self.expanded.contains(&node.path);
            let path = node.path.clone();
            out.push(Row { depth, node });
            if open {
                self.walk(&path, depth + 1, out);
            }
        }
    }

    /// フォルダを開く・畳む。
    pub fn toggle(&mut self, folder: &str) {
        if !self.expanded.remove(folder) {
            self.expanded.insert(folder.to_owned());
        }
    }

    /// どれかのフォルダを開いているか（帯のフォルダのボタンの向き）。
    pub fn any_expanded(&self) -> bool {
        !self.expanded.is_empty()
    }

    /// すべてのフォルダを開く（中を読みながら）・畳む。
    pub fn set_all(&mut self, expanded: bool) {
        self.expanded.clear();
        if !expanded {
            return;
        }
        let mut stack = vec![String::new()];
        while let Some(folder) = stack.pop() {
            self.load(&folder);
            for node in self.children[&folder].clone() {
                if node.is_dir {
                    self.expanded.insert(node.path.clone());
                    stack.push(node.path);
                }
            }
        }
    }
}

/// 変更の種類の印と色（macOS 版の `FileCell.mark`）。
fn kind_mark(kind: ChangeKind) -> (&'static str, Color) {
    match kind {
        ChangeKind::Added => ("A", Color::rgb(0x2e, 0xa0, 0x43)),
        ChangeKind::Deleted => ("D", Color::rgb(0xdc, 0x38, 0x33)),
        ChangeKind::Modified => ("M", Color::rgb(0xed, 0x8c, 0x1a)),
        ChangeKind::Renamed => ("R", Color::rgb(0x35, 0x85, 0xe4)),
        ChangeKind::Copied => ("C", Color::rgb(0x35, 0x85, 0xe4)),
    }
}

/// 木・リストの一覧。行を選ぶと `selected`（`rows` の番号）。
pub fn list(
    rows: &[Row],
    flat: bool,
    expanded: &HashSet<String>,
    selected: Callback<Option<usize>>,
) -> View {
    let items = rows.iter().map(|r| {
        let n = &r.node;
        let chevron: View = if n.is_dir {
            FontIcon::new()
                .glyph(if expanded.contains(&n.path) {
                    "\u{E70D}"
                } else {
                    "\u{E76C}"
                })
                .into()
        } else {
            View::empty()
        };
        // リストならフォルダを薄く添える。変更を含まないものは薄く、消したものは赤。
        let folder: View = if flat && !parent_of(&n.path).is_empty() {
            TextBlock::new()
                .text(format!("{}/", parent_of(&n.path)))
                .opacity(0.45)
                .into()
        } else {
            View::empty()
        };
        // このコミットで消したファイルは、赤の取り消し線（macOS 版・GTK4 版と同じ）。
        let name: View = if n.kind == Some(ChangeKind::Deleted) {
            RichTextBlock::new()
                .paragraphs(RichText::single_paragraph([RichTextInline::Run(
                    RichTextRun {
                        text: n.name.clone(),
                        foreground: Some(Color::rgb(0xdc, 0x38, 0x33)),
                        is_strikethrough: true,
                        ..RichTextRun::default()
                    },
                )]))
                .into()
        } else {
            let name = TextBlock::new()
                .text(n.name.clone())
                .text_trimming(TextTrimming::CharacterEllipsis);
            if n.contains_changes {
                name.into()
            } else {
                name.opacity(0.6).into()
            }
        };
        let mark: View = match n.kind {
            Some(kind) => {
                let (mark, color) = kind_mark(kind);
                TextBlock::new()
                    .text(mark)
                    .font_weight(FontWeight::BOLD)
                    .font_size(11.0)
                    .foreground(Brush::Solid(color))
                    .margin(Thickness::new(6.0, 0.0, 0.0, 0.0))
                    .into()
            }
            None if n.is_dir && n.contains_changes => TextBlock::new()
                .text("●")
                .font_size(8.0)
                .foreground(Brush::Solid(Color::rgb(0xe6, 0x61, 0x00)))
                .vertical_alignment(VerticalAlignment::Center)
                .margin(Thickness::new(6.0, 0.0, 0.0, 0.0))
                .into(),
            None => View::empty(),
        };
        let row = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(5.0)
            .margin(Thickness::new(r.depth as f64 * 14.0, 0.0, 0.0, 0.0))
            .children((
                Border::new().width(14.0).content(chevron),
                Grid::new()
                    .opacity(if n.is_dir { 1.0 } else { 0.6 })
                    .children((crate::icons::icon(
                        if n.is_dir { "folder" } else { "files" },
                        16.0,
                    ),)),
                folder,
                name,
                mark,
            ));
        (
            n.path.clone(),
            ListViewItem::new().min_height(26.0).content(row),
        )
    });
    let key: String = rows
        .iter()
        .map(|r| r.node.path.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let list = ListView::new()
        .selected_index(None)
        .on_selection_changed(selected)
        .collection_slot(ListViewSlot::Items, items);
    View::keyed_fragment([(format!("{}{}", flat, key.len()), list)])
}

/// 開いたファイルのパスの帯: 戻る、フォルダ / 名前 @ 短い ID（消したなら赤の注記）、この変更を見る。
pub fn path_bar(
    browser: &Browser,
    back: Callback<()>,
    show_change: Callback<()>,
    markdown: Option<(bool, Callback<bool>, Callback<bool>)>,
    svg: View,
    preview: Callback<()>,
) -> View {
    let Some(path) = &browser.open else {
        return View::empty();
    };
    let folder = parent_of(path);
    let hex = browser.commit.to_string();
    let deleted = browser
        .change_of(path)
        .is_some_and(|c| c.kind == ChangeKind::Deleted);
    let changed = browser.change_of(path).is_some();
    // Markdown は、整形（プレビュー）とソース（テキスト）を切り替える。押して外したときはそのまま。
    let switch: View = match markdown {
        Some((text, preview, source)) => StackPanel::new()
            .orientation(Orientation::Horizontal)
            .children((
                ToggleButton::new()
                    .is_checked(!text)
                    .on_is_checked_changed(preview)
                    .content(TextBlock::new().text(tr!("Preview")).font_size(12.0))
                    .tooltip(tr!("Show the Markdown formatted")),
                ToggleButton::new()
                    .is_checked(text)
                    .on_is_checked_changed(source)
                    .content(TextBlock::new().text(tr!("Text")).font_size(12.0))
                    .tooltip(tr!("Show the Markdown source")),
            )),
        None => View::empty(),
    };
    Grid::new()
        .columns([
            GridLength::Auto,
            GridLength::Star(1.0),
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Auto,
        ])
        .column_spacing(8.0)
        .margin(Thickness::new(8.0, 3.0, 8.0, 3.0))
        .children((
            Button::new()
                .grid_column(0)
                .style(ButtonStyle::Subtle)
                .on_click(back)
                .content(crate::icons::icon("back", 16.0))
                .tooltip(tr!("Back to the files")),
            StackPanel::new()
                .grid_column(1)
                .orientation(Orientation::Horizontal)
                .vertical_alignment(VerticalAlignment::Center)
                .spacing(4.0)
                .children((
                    if folder.is_empty() {
                        View::empty()
                    } else {
                        TextBlock::new()
                            .text(format!("{} /", folder.replace('/', " / ")))
                            .opacity(0.6)
                            .into()
                    },
                    TextBlock::new()
                        .text(name_of(path).to_owned())
                        .font_weight(FontWeight::SEMI_BOLD),
                    TextBlock::new()
                        .text(format!("@ {}", &hex[..8]))
                        .font_size(11.0)
                        .opacity(0.6)
                        .vertical_alignment(VerticalAlignment::Center),
                    if deleted {
                        TextBlock::new()
                            .text(tr!("(deleted in this commit)"))
                            .font_size(11.0)
                            .foreground(Brush::Solid(Color::rgb(0xe0, 0x1b, 0x24)))
                            .vertical_alignment(VerticalAlignment::Center)
                            .into()
                    } else {
                        View::empty()
                    },
                )),
            StackPanel::new()
                .grid_column(2)
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((switch, svg)),
            if changed {
                Button::new()
                    .grid_column(3)
                    .on_click(show_change)
                    .content(TextBlock::new().text(tr!("Show This Change")))
            } else {
                View::empty()
            },
            // アプリの外で見る（macOS 版の Quick Look、GTK4 版のプレビューのボタン。消したファイルは前の中身）。
            Button::new()
                .grid_column(4)
                .style(ButtonStyle::Subtle)
                .on_click(preview)
                .content(crate::icons::icon("preview", 16.0))
                .tooltip(tr!("Open this file at the commit in a preview")),
        ))
}
