//! Keys and Values: git の設定をキーと値で見る表（macOS 版の `AllSettingsTab`。docs/ux.md の 4.5）。
//!
//! Xcode の Build Settings と同じ形: 列はキー、効いている値、そして設定元ごと（システム全体・すべてのリポジトリ・
//! このリポジトリ・この worktree）の値。効いている段の値は太字。行はキーのドットで開閉できる組にする（section、
//! その下に subsection。初めはすべて開いている）。書ける列（このリポジトリ。すべてのリポジトリの段ならすべての
//! リポジトリ。Combined なら効いている値）は、マウスを載せると欄の枠が出て、1 回のクリックで編集する。空にすると
//! その段から消す。「+」でキーと値を足し、「−」と Delete で選んだキーを消す。表からの書き換えは Ctrl+Z／
//! Ctrl+Shift+Z で何段でも行き来でき、最後の削除は下の 1 行にも出て、その「戻す」は 1 段の Ctrl+Z と同じ。
//! 上の切り替えは Xcode の語のまま（訳さない）で、アプリ全体で覚える: 行は Basic／Customized／All、列は
//! Combined／Levels。

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::{Rc, Weak};

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::reposettings::{Level, RepoSettings, config_file, label, scope_label};

/// 「Basic」のキー: ページが尋ねるもの（docs/ux.md の 4.5）。git の文書の綴りで（比べるときは大文字小文字を
/// 区別しない）。
const BASIC_KEYS: &[&str] = &[
    "user.name",
    "user.email",
    "commit.gpgSign",
    "gpg.format",
    "user.signingKey",
    "commit.template",
    "amonhen.defaultBranch",
    "pull.rebase",
    "pull.ff",
    "fetch.prune",
    "rebase.autoStash",
    "rebase.updateRefs",
    "merge.conflictStyle",
    "rerere.enabled",
    "rerere.autoUpdate",
    "core.autocrlf",
    "core.fileMode",
];

/// 段（列の順）。
const SCOPES: &[&str] = &["system", "global", "local", "worktree"];

/// 切り替えの覚え場所（アプリの設定。macOS 版の UserDefaults と同じ名前）。
const ROWS_KEY: &str = "configTableRows";
const COLUMNS_KEY: &str = "configTableColumns";

/// 表の 1 行: 1 つのキーと、段ごとの値。同じ段に同じキーが複数あれば（`remote.origin.fetch` など）改行で並べる。
#[derive(Clone, Default, Debug)]
struct Row {
    key: String,
    /// キーの列に出す字（組の中では最後の部分、組はその名前）。
    label: String,
    /// 今効いている値と、その段。どの段にもなければ空。
    effective: String,
    scope: String,
    values: HashMap<String, String>,
    files: HashMap<String, String>,
    /// 組（section か subsection）の見出しの行。
    group: bool,
    children: Vec<Row>,
}

/// キーの 3 つの部分。`section.name` か `section.subsection.name` で、subsection にはドットが入りうる
/// （`url.https://….insteadOf`）ので、最初と最後のドットで切る。
fn parts(key: &str) -> (String, Option<String>, String) {
    match (key.find('.'), key.rfind('.')) {
        (Some(first), Some(last)) => (
            key[..first].to_owned(),
            (first != last).then(|| key[first + 1..last].to_owned()),
            key[last + 1..].to_owned(),
        ),
        _ => (key.to_owned(), None, String::new()),
    }
}

/// git が知っている変数の綴り（`core.fileMode`）を小文字のキーから引く。`git config --list` は小文字で出す。
fn canonical_keys(known: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for k in known {
        map.entry(k.to_lowercase()).or_insert_with(|| k.clone());
    }
    map
}

/// 設定をキーごとに 1 行にまとめる（キーは git が知っている綴りにそろえる）。
fn config_rows(s: &RepoSettings) -> Vec<Row> {
    let st = s.state.borrow();
    let canonical = canonical_keys(&st.known_keys);
    let mut order: Vec<String> = Vec::new();
    let mut by_key: HashMap<String, Row> = HashMap::new();
    for e in &st.entries {
        let key = canonical
            .get(&e.key.to_lowercase())
            .cloned()
            .unwrap_or_else(|| e.key.clone());
        let row = by_key.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Row {
                key: key.clone(),
                ..Default::default()
            }
        });
        row.values
            .entry(e.scope.clone())
            .and_modify(|v| {
                v.push('\n');
                v.push_str(&e.value);
            })
            .or_insert_with(|| e.value.clone());
        row.files.insert(e.scope.clone(), config_file(e));
        row.scope = e.scope.clone();
    }
    order
        .into_iter()
        .filter_map(|k| by_key.remove(&k))
        .map(|mut r| {
            r.effective = r.values.get(&r.scope).cloned().unwrap_or_default();
            r
        })
        .collect()
}

fn blank(key: &str) -> Row {
    Row {
        key: key.to_owned(),
        ..Default::default()
    }
}

/// 値の見せ方（markup）: 同じ段に複数あれば 1 行ずつ、明示的に空の値は「(empty)」。
fn value_markup(value: &str) -> String {
    value
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                format!(
                    "<span alpha=\"55%\">{}</span>",
                    glib::markup_escape_text(&tr!("(empty)"))
                )
            } else {
                glib::markup_escape_text(line).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn row_of(object: &glib::Object) -> Option<Row> {
    let object = match object.downcast_ref::<gtk::TreeListRow>() {
        Some(r) => r.item()?,
        None => object.clone(),
    };
    object
        .downcast_ref::<glib::BoxedAnyObject>()
        .map(|b| b.borrow::<Row>().clone())
}

/// 表の行のキー（組の見出しは None。確認用の手順が行を選ぶのに使う）。
pub(crate) fn row_key(object: &glib::Object) -> Option<String> {
    row_of(object).filter(|r| !r.group).map(|r| r.key)
}

/// 消したキーと値。
type Removed = Vec<(String, String)>;

/// 読むだけのセルに出すもの: 値（なければ空）、太字か、ツールチップ。
type CellText = Rc<dyn Fn(&Row) -> (Option<String>, bool, String)>;

/// 表の状態。
struct Table {
    settings: Weak<RepoSettings>,
    view: gtk::ColumnView,
    sort_model: gtk::SortListModel,
    selection: gtk::MultiSelection,
    rows_mode: RefCell<String>,
    columns_mode: RefCell<String>,
    filter: RefCell<String>,
    /// 閉じた組（初めはすべて開いている）。
    collapsed: RefCell<HashSet<String>>,
    /// 出ている書けるセル（キーごと。右クリックの「編集」がそのセルで編集を始める）。
    cells: RefCell<HashMap<String, Vec<Weak<EditCell>>>>,
    /// 作った書けるセルのすべて（部品からセルを引く）。
    all_cells: RefCell<Vec<Weak<EditCell>>>,
    /// 最後に消したキーと値と、それを書いたあとの書き換えの数（それより後に書き換えがあれば出さない）。
    removed: RefCell<Option<(u64, Removed)>>,
    removed_label: gtk::Label,
    restore: gtk::Button,
    /// 列: 効いている値・設定元・段ごと。
    effective_column: RefCell<Option<gtk::ColumnViewColumn>>,
    source_column: RefCell<Option<gtk::ColumnViewColumn>>,
    scope_columns: RefCell<Vec<(String, gtk::ColumnViewColumn)>>,
    /// 足している途中の行。
    adding: gtk::Box,
    new_key: gtk::Entry,
    new_value: gtk::Entry,
}

/// 書けるセル: ふだんは文字だけ（マウスを載せると欄の枠）。1 回のクリックで欄になり、Return か離れたら書く。
struct EditCell {
    stack: gtk::Stack,
    text: gtk::Label,
    entry: gtk::Entry,
    key: RefCell<String>,
    editing: Cell<bool>,
    table: Weak<Table>,
}

impl EditCell {
    fn new(table: &Rc<Table>) -> Rc<Self> {
        let text = label("", &[]);
        text.set_hexpand(true);
        text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let shown = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        shown.add_css_class("config-editable");
        shown.append(&text);
        shown.set_cursor_from_name(Some("text"));
        let entry = gtk::Entry::new();
        entry.add_css_class("config-entry");
        let stack = gtk::Stack::new();
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);
        stack.add_named(&shown, Some("shown"));
        stack.add_named(&entry, Some("edit"));
        let cell = Rc::new(Self {
            stack,
            text,
            entry,
            key: RefCell::default(),
            editing: Cell::new(false),
            table: Rc::downgrade(table),
        });
        let click = gtk::GestureClick::new();
        let c = Rc::downgrade(&cell);
        click.connect_released(move |_, n, _, _| {
            if n == 1
                && let Some(c) = c.upgrade()
            {
                c.begin();
            }
        });
        shown.add_controller(click);
        let c = Rc::downgrade(&cell);
        cell.entry.connect_activate(move |_| {
            if let Some(c) = c.upgrade() {
                c.commit();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let c = Rc::downgrade(&cell);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape
                && let Some(c) = c.upgrade()
            {
                c.end();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        cell.entry.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        let c = Rc::downgrade(&cell);
        focus.connect_leave(move |_| {
            if let Some(c) = c.upgrade() {
                c.commit();
            }
        });
        cell.entry.add_controller(focus);
        cell
    }

    /// 編集を始める（欄にはこの段の値）。
    fn begin(&self) {
        let (Some(table), key) = (self.table.upgrade(), self.key.borrow().clone()) else {
            return;
        };
        if key.is_empty() {
            return;
        }
        let Some(s) = table.settings.upgrade() else {
            return;
        };
        self.entry.set_text(&s.local(&key).unwrap_or_default());
        self.editing.set(true);
        self.stack.set_visible_child_name("edit");
        self.entry.grab_focus();
        table.select_key(&key);
    }

    fn end(&self) {
        self.editing.set(false);
        self.stack.set_visible_child_name("shown");
    }

    /// 欄の値を書く（変わっていれば）。空ならこの段から消す。
    fn commit(&self) {
        if !self.editing.replace(false) {
            return;
        }
        self.stack.set_visible_child_name("shown");
        let key = self.key.borrow().clone();
        let Some(table) = self.table.upgrade() else {
            return;
        };
        let value = self.entry.text().trim().to_owned();
        table.write(vec![(key, (!value.is_empty()).then_some(value))], false);
    }
}

impl Table {
    fn settings(&self) -> Option<Rc<RepoSettings>> {
        self.settings.upgrade()
    }

    /// 部品 `widget` が書けるセルなら、そのセル。
    fn cell_of(&self, widget: &gtk::Widget) -> Option<Rc<EditCell>> {
        let mut cells = self.all_cells.borrow_mut();
        cells.retain(|c| c.strong_count() > 0);
        cells
            .iter()
            .filter_map(Weak::upgrade)
            .find(|c| c.stack.upcast_ref::<gtk::Widget>() == widget)
    }

    /// 表からの書き換えはすべてここを通す（Undo に 1 段として積む）。`removal` なら消したものを下の 1 行に出す
    /// （それ以外の書き換えのあとは出さない）。
    fn write(&self, changes: Vec<(String, Option<String>)>, removal: bool) {
        let Some(s) = self.settings() else { return };
        let gone: Vec<(String, String)> = changes
            .iter()
            .filter(|(_, v)| v.is_none())
            .filter_map(|(k, _)| s.local(k).map(|old| (k.clone(), old)))
            .collect();
        let changes: Vec<_> = changes
            .into_iter()
            .filter(|(k, v)| s.local(k) != *v)
            .collect();
        if changes.is_empty() {
            return;
        }
        // 書けたら書き換えの数が 1 つ増える。そのときだけ出す。
        self.removed
            .replace(removal.then(|| (s.writes() + 1, gone)));
        s.write(changes, None);
    }

    /// 選んだ行のこの段の値を消す。
    fn remove_selected(&self) {
        let Some(s) = self.settings() else { return };
        let keys: Vec<(String, Option<String>)> = self
            .selected_keys()
            .into_iter()
            .filter(|k| s.local(k).is_some())
            .map(|k| (k, None))
            .collect();
        self.write(keys, true);
    }

    fn selected_keys(&self) -> Vec<String> {
        let bits = self.selection.selection();
        let mut keys = Vec::new();
        for i in 0..bits.size() {
            let pos = bits.nth(i as u32);
            if let Some(row) = self.sort_model.item(pos).and_then(|o| row_of(&o))
                && !row.group
            {
                keys.push(row.key);
            }
        }
        keys.sort();
        keys
    }

    fn select_key(&self, key: &str) {
        for i in 0..self.sort_model.n_items() {
            if self
                .sort_model
                .item(i)
                .and_then(|o| row_of(&o))
                .is_some_and(|r| r.key == key && !r.group)
            {
                self.selection.select_item(i, true);
                return;
            }
        }
    }

    /// 選んだ行を `key = value` の形に（効いている値。複数あれば 1 行ずつ。どこにも設定のない行は飛ばす）。
    fn copy_text(&self) -> String {
        let bits = self.selection.selection();
        let mut lines = Vec::new();
        for i in 0..bits.size() {
            let pos = bits.nth(i as u32);
            if let Some(row) = self.sort_model.item(pos).and_then(|o| row_of(&o))
                && !row.group
                && !row.scope.is_empty()
            {
                for v in row.effective.split('\n') {
                    lines.push(format!("{} = {v}", row.key));
                }
            }
        }
        lines.join("\n")
    }

    fn copy(&self) {
        let text = self.copy_text();
        if !text.is_empty() {
            self.view.clipboard().set_text(&text);
        }
    }

    /// 出す行を作り直す（切り替え・検索・読み直しのたび）。開閉の状態は残す。
    fn rebuild(&self) {
        let Some(s) = self.settings() else { return };
        // 今閉じている組を覚える。
        if let Some(tree) = self.sort_model.model().and_downcast::<gtk::TreeListModel>() {
            let mut collapsed = self.collapsed.borrow_mut();
            for i in 0..tree.n_items() {
                if let Some(r) = tree.row(i)
                    && r.is_expandable()
                    && let Some(row) = r.item().and_then(|o| row_of(&o))
                {
                    if r.is_expanded() {
                        collapsed.remove(&row.key);
                    } else {
                        collapsed.insert(row.key);
                    }
                }
            }
        }
        let own = s.own_scope();
        let repository = s.level == Level::Repository;
        let mut all = config_rows(&s);
        match self.rows_mode.borrow().as_str() {
            "all" => {
                let set: HashSet<String> = all.iter().map(|r| r.key.to_lowercase()).collect();
                let known = s.state.borrow().known_keys.clone();
                all.extend(
                    known
                        .iter()
                        .filter(|k| !set.contains(&k.to_lowercase()))
                        .map(|k| blank(k)),
                );
            }
            "basic" => {
                let canonical = canonical_keys(&s.state.borrow().known_keys);
                let mut by_key: HashMap<String, Row> =
                    all.into_iter().map(|r| (r.key.to_lowercase(), r)).collect();
                all = BASIC_KEYS
                    .iter()
                    .map(|k| {
                        let lower = k.to_lowercase();
                        by_key.remove(&lower).unwrap_or_else(|| {
                            blank(canonical.get(&lower).map_or(*k, String::as_str))
                        })
                    })
                    .collect();
            }
            _ => all.retain(|r| {
                r.values.contains_key(own) || (repository && r.values.contains_key("worktree"))
            }),
        }
        let f = self.filter.borrow().trim().to_lowercase();
        if !f.is_empty() {
            all.retain(|r| {
                r.key.to_lowercase().contains(&f)
                    || r.values.values().any(|v| v.to_lowercase().contains(&f))
            });
        }
        // section、その下に subsection の組にする。
        let mut sections: BTreeMap<String, Row> = BTreeMap::new();
        for mut row in all {
            let (section, sub, name) = parts(&row.key);
            row.label = name;
            let group = sections.entry(section.clone()).or_insert_with(|| Row {
                key: section.clone(),
                label: section.clone(),
                group: true,
                ..Default::default()
            });
            match sub {
                Some(sub) => {
                    let key = format!("{section}.{sub}");
                    match group.children.iter_mut().find(|c| c.group && c.key == key) {
                        Some(g) => g.children.push(row),
                        None => group.children.push(Row {
                            key,
                            label: sub,
                            group: true,
                            children: vec![row],
                            ..Default::default()
                        }),
                    }
                }
                None => group.children.push(row),
            }
        }
        let root = gio::ListStore::new::<glib::BoxedAnyObject>();
        for (_, section) in sections {
            root.append(&glib::BoxedAnyObject::new(section));
        }
        let tree = gtk::TreeListModel::new(root, false, false, |item| {
            let row = row_of(item)?;
            if !row.group {
                return None;
            }
            let store = gio::ListStore::new::<glib::BoxedAnyObject>();
            for c in row.children {
                store.append(&glib::BoxedAnyObject::new(c));
            }
            Some(store.upcast())
        });
        // 閉じていなかった組を開く（開くと中の行が後ろに入るので、数え直しながら進む）。
        let collapsed = self.collapsed.borrow();
        let mut i = 0;
        while i < tree.n_items() {
            if let Some(r) = tree.row(i)
                && r.is_expandable()
                && r.item()
                    .and_then(|o| row_of(&o))
                    .is_some_and(|row| !collapsed.contains(&row.key))
            {
                r.set_expanded(true);
            }
            i += 1;
        }
        drop(collapsed);
        self.sort_model.set_model(Some(&tree));
        self.update_removed();
    }

    /// 列の切り替え（Combined: 効いている値と設定元。Levels: 段ごと）。
    fn show_columns(&self) {
        let Some(s) = self.settings() else { return };
        let levels = self.columns_mode.borrow().as_str() == "levels";
        if let Some(c) = self.source_column.borrow().as_ref() {
            c.set_visible(!levels);
        }
        for (scope, c) in self.scope_columns.borrow().iter() {
            let shown = match scope.as_str() {
                "local" | "worktree" => s.level == Level::Repository,
                _ => true,
            };
            c.set_visible(levels && shown);
        }
    }

    /// 下の 1 行（最後の削除）。そのあとに書き換えがあれば出さない。
    fn update_removed(&self) {
        let Some(s) = self.settings() else { return };
        let removed = self.removed.borrow();
        let shown = removed
            .as_ref()
            .filter(|(at, gone)| *at == s.writes() && !gone.is_empty());
        let repository = s.level == Level::Repository;
        if let Some((_, gone)) = shown {
            let text = if gone.len() == 1 {
                let what = format!("{} = {}", gone[0].0, gone[0].1);
                if repository {
                    tr!("Removed {what} from this repository", what = what)
                } else {
                    tr!("Removed {what} from all repositories", what = what)
                }
            } else if repository {
                tr!(
                    "Removed {count} keys from this repository",
                    count = gone.len()
                )
            } else {
                tr!(
                    "Removed {count} keys from all repositories",
                    count = gone.len()
                )
            };
            self.removed_label.set_text(&text);
        }
        self.removed_label.set_visible(shown.is_some());
        self.restore.set_visible(shown.is_some());
    }

    /// 下の 1 行の「戻す」: 最後の削除を 1 段戻す（Ctrl+Z と同じ）。
    fn restore(&self) {
        if let Some(s) = self.settings() {
            self.removed.replace(None);
            s.undo();
        }
    }

    /// 「+」: 表の下にキーと値の欄を出す（キーと値がそろわないまま離れたら消える）。
    fn begin_adding(&self) {
        self.new_key.set_text("");
        self.new_value.set_text("");
        self.adding.set_visible(true);
        self.new_key.grab_focus();
    }

    fn commit_adding(&self) {
        if !self.adding.is_visible() {
            return;
        }
        let key = self.new_key.text().trim().to_owned();
        let value = self.new_value.text().trim().to_owned();
        self.adding.set_visible(false);
        if key.is_empty() || value.is_empty() {
            return;
        }
        self.write(vec![(key, Some(value))], false);
    }
}

/// 色・枠（書けるセルは、マウスを載せたときだけ欄の枠）。
fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".config-editable { padding: 1px 4px; border-radius: 4px; \
           box-shadow: inset 0 0 0 1px transparent; } \
         .config-editable:hover { background-color: alpha(@theme_base_color, 0.6); \
           box-shadow: inset 0 0 0 1px alpha(currentColor, 0.35); } \
         .config-entry { min-height: 0; padding-top: 0; padding-bottom: 0; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 2 つか 3 つの切り替え（Xcode の segmented control）。`on_change` に選んだ値。
fn segmented(
    items: &[(&str, &str)],
    current: &str,
    on_change: impl Fn(&str) + 'static,
) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("linked");
    let on_change = Rc::new(on_change);
    let mut first: Option<gtk::ToggleButton> = None;
    for (value, text) in items {
        let t = gtk::ToggleButton::with_label(text);
        if let Some(f) = &first {
            t.set_group(Some(f));
        } else {
            first = Some(t.clone());
        }
        t.set_active(*value == current);
        let (f, value) = (on_change.clone(), (*value).to_owned());
        t.connect_toggled(move |t| {
            if t.is_active() {
                f(&value);
            }
        });
        b.append(&t);
    }
    b
}

/// Keys and Values のページ。
pub(crate) fn page(s: &Rc<RepoSettings>) -> gtk::Box {
    install_css();
    let repository = s.level == Level::Repository;
    let rows_mode = crate::settings::string(ROWS_KEY)
        .filter(|m| ["basic", "customized", "all"].contains(&m.as_str()))
        .unwrap_or_else(|| "customized".into());
    let columns_mode = crate::settings::string(COLUMNS_KEY)
        .filter(|m| ["combined", "levels"].contains(&m.as_str()))
        .unwrap_or_else(|| "levels".into());

    let sort_model = gtk::SortListModel::new(None::<gio::ListModel>, None::<gtk::Sorter>);
    let selection = gtk::MultiSelection::new(Some(sort_model.clone()));
    let view = gtk::ColumnView::new(Some(selection.clone()));
    view.set_show_row_separators(true);
    view.set_show_column_separators(true);
    let row_sorter = gtk::TreeListRowSorter::new(view.sorter());
    sort_model.set_sorter(Some(&row_sorter));

    let removed_label = label("", &["dim-label"]);
    removed_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    removed_label.set_margin_start(8);
    let restore = gtk::Button::with_label(&tr!("Restore"));
    let new_key = gtk::Entry::builder()
        .placeholder_text(tr!("Key"))
        .hexpand(true)
        .build();
    let new_value = gtk::Entry::builder()
        .placeholder_text(tr!("Value"))
        .hexpand(true)
        .build();
    let adding = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    adding.append(&new_key);
    adding.append(&new_value);
    adding.set_visible(false);

    let table = Rc::new(Table {
        settings: s.weak(),
        view: view.clone(),
        sort_model: sort_model.clone(),
        selection: selection.clone(),
        rows_mode: RefCell::new(rows_mode.clone()),
        columns_mode: RefCell::new(columns_mode.clone()),
        filter: RefCell::default(),
        collapsed: RefCell::default(),
        cells: RefCell::default(),
        all_cells: RefCell::default(),
        removed: RefCell::default(),
        removed_label: removed_label.clone(),
        restore: restore.clone(),
        effective_column: RefCell::default(),
        source_column: RefCell::default(),
        scope_columns: RefCell::default(),
        adding: adding.clone(),
        new_key: new_key.clone(),
        new_value: new_value.clone(),
    });

    // 列の並べ替え（組の見出しはキーで並べる）。
    let sorter = |f: fn(&Row) -> String| {
        gtk::CustomSorter::new(move |a, b| {
            let (Some(a), Some(b)) = (row_of(a), row_of(b)) else {
                return gtk::Ordering::Equal;
            };
            let (x, y) = if a.group || b.group {
                (a.key.to_lowercase(), b.key.to_lowercase())
            } else {
                (f(&a).to_lowercase(), f(&b).to_lowercase())
            };
            x.cmp(&y).into()
        })
    };
    let host = s.host.clone();

    // キー（組は太字の見出しと開閉の三角）。
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let expander = gtk::TreeExpander::new();
        let l = label("", &[]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        expander.set_child(Some(&l));
        item.set_child(Some(&expander));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(expander), Some(tree_row)) = (
            item.child().and_downcast::<gtk::TreeExpander>(),
            item.item().and_downcast::<gtk::TreeListRow>(),
        ) else {
            return;
        };
        expander.set_list_row(Some(&tree_row));
        let Some(row) = tree_row.item().and_then(|o| row_of(&o)) else {
            return;
        };
        if let Some(l) = expander.child().and_downcast::<gtk::Label>() {
            if row.group {
                l.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(&row.label)));
                l.set_tooltip_text(None);
            } else {
                l.set_text(&row.label);
                l.set_tooltip_text(Some(&row.key));
            }
        }
    });
    let key_column = gtk::ColumnViewColumn::new(Some(&tr!("Key")), Some(factory));
    key_column.set_resizable(true);
    key_column.set_fixed_width(200);
    key_column.set_sorter(Some(&sorter(|r| r.key.clone())));
    view.append_column(&key_column);

    // 書けるセルの列（`shown` がセルに出す値）。
    let editable_factory = |shown: fn(&Row, &str) -> Option<(String, bool)>| {
        let factory = gtk::SignalListItemFactory::new();
        let t = Rc::downgrade(&table);
        factory.connect_setup(move |_, item| {
            let (Some(item), Some(table)) = (item.downcast_ref::<gtk::ListItem>(), t.upgrade())
            else {
                return;
            };
            let cell = EditCell::new(&table);
            item.set_child(Some(&cell.stack));
            table.all_cells.borrow_mut().push(Rc::downgrade(&cell));
            // セルは部品が消えるまで持つ。
            let keep = cell.clone();
            cell.stack.connect_destroy(move |_| {
                let _ = &keep;
            });
        });
        let t = Rc::downgrade(&table);
        factory.connect_bind(move |_, item| {
            let (Some(item), Some(table)) = (item.downcast_ref::<gtk::ListItem>(), t.upgrade())
            else {
                return;
            };
            let Some(stack) = item.child().and_downcast::<gtk::Stack>() else {
                return;
            };
            let Some(cell) = table.cell_of(stack.upcast_ref()) else {
                return;
            };
            let Some(row) = item
                .item()
                .and_downcast::<gtk::TreeListRow>()
                .and_then(|r| r.item())
                .and_then(|o| row_of(&o))
            else {
                return;
            };
            let Some(s) = table.settings() else { return };
            cell.end();
            if row.group {
                cell.key.replace(String::new());
                cell.text.set_text("");
                cell.stack.set_visible(false);
                return;
            }
            cell.stack.set_visible(true);
            cell.key.replace(row.key.clone());
            match shown(&row, s.own_scope()) {
                Some((value, bold)) => {
                    let markup = value_markup(&value);
                    cell.text.set_markup(&if bold {
                        format!("<b>{markup}</b>")
                    } else {
                        markup
                    });
                }
                None => cell.text.set_text(""),
            }
            cell.stack
                .set_tooltip_text(Some(&if s.level == Level::Repository {
                    tr!("Click to edit in this repository")
                } else {
                    tr!("Click to edit for all repositories")
                }));
            let mut cells = table.cells.borrow_mut();
            let list = cells.entry(row.key.clone()).or_default();
            list.retain(|c| c.upgrade().is_some_and(|c| *c.key.borrow() == row.key));
            list.push(Rc::downgrade(&cell));
        });
        factory
    };
    // 読むだけのセルの列。
    let text_factory = |text: CellText| {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                let l = label("", &[]);
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                item.set_child(Some(&l));
            }
        });
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(l), Some(row)) = (
                item.child().and_downcast::<gtk::Label>(),
                item.item()
                    .and_downcast::<gtk::TreeListRow>()
                    .and_then(|r| r.item())
                    .and_then(|o| row_of(&o)),
            ) else {
                return;
            };
            if row.group {
                l.set_text("");
                l.set_tooltip_text(None);
                return;
            }
            let (value, bold, tooltip) = text(&row);
            match value {
                Some(v) => {
                    let markup = value_markup(&v);
                    l.set_markup(&if bold {
                        format!("<b>{markup}</b>")
                    } else {
                        markup
                    });
                }
                None => l.set_text(""),
            }
            l.set_tooltip_text((!tooltip.is_empty()).then_some(tooltip.as_str()));
        });
        factory
    };

    // 効いている値: Combined なら書ける欄（ここが効いている値の欄）、Levels なら読むだけ。2 つの列を用意して
    // 切り替える。
    let effective_edit = gtk::ColumnViewColumn::new(
        Some(&tr!("Value in effect")),
        Some(editable_factory(|r, _| {
            (!r.scope.is_empty()).then(|| (r.effective.clone(), false))
        })),
    );
    let h = host.clone();
    let effective_read = gtk::ColumnViewColumn::new(
        Some(&tr!("Value in effect")),
        Some(text_factory(Rc::new(move |r| {
            if r.scope.is_empty() {
                // どこにも設定がない（Basic の未設定のキー）: 空の値とは違うので、何も出さない。
                return (None, false, String::new());
            }
            let tip = format!(
                "{} · {}",
                scope_label(&r.scope, h.as_deref()),
                r.files.get(&r.scope).cloned().unwrap_or_default()
            );
            (Some(r.effective.clone()), false, tip)
        }))),
    );
    for c in [&effective_edit, &effective_read] {
        c.set_resizable(true);
        c.set_fixed_width(130);
        c.set_sorter(Some(&sorter(|r| r.effective.clone())));
        view.append_column(c);
    }
    let h = host.clone();
    let source = gtk::ColumnViewColumn::new(
        Some(&tr!("Source")),
        Some(text_factory(Rc::new(move |r| {
            if r.scope.is_empty() {
                (Some(tr!("git’s default")), false, String::new())
            } else {
                (
                    Some(scope_label(&r.scope, h.as_deref())),
                    false,
                    format!(
                        "{} · {}",
                        r.scope,
                        r.files.get(&r.scope).cloned().unwrap_or_default()
                    ),
                )
            }
        }))),
    );
    source.set_resizable(true);
    source.set_fixed_width(150);
    source.set_sorter(Some(&sorter(|r| r.scope.clone())));
    view.append_column(&source);
    table.source_column.replace(Some(source));

    // 段ごとの列。書けるのはこの段だけ。見出しは 4.3 の設定元の言葉を、文の頭だけ大文字に。
    let own = s.own_scope();
    for scope in SCOPES {
        let text = scope_label(scope, host.as_deref());
        let header = match &host {
            Some(h) if text.starts_with(h.as_str()) => text,
            _ => {
                let mut c = text.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
        };
        let factory = if *scope == own {
            editable_factory(|r, own| {
                r.values
                    .get(own)
                    .map(|v| (v.clone(), r.scope.as_str() == own))
            })
        } else {
            let scope = (*scope).to_owned();
            text_factory(Rc::new(move |r| match r.values.get(&scope) {
                Some(v) => (
                    Some(v.clone()),
                    r.scope == scope,
                    format!(
                        "{scope} · {}",
                        r.files.get(&scope).cloned().unwrap_or_default()
                    ),
                ),
                None => (None, false, String::new()),
            }))
        };
        let column = gtk::ColumnViewColumn::new(Some(&header), Some(factory));
        column.set_resizable(true);
        column.set_fixed_width(match *scope {
            "system" => 90,
            "worktree" => 100,
            _ => 115,
        });
        column.set_expand(*scope == own);
        let sc = (*scope).to_owned();
        column.set_sorter(Some(&gtk::CustomSorter::new(move |a, b| {
            let (Some(a), Some(b)) = (row_of(a), row_of(b)) else {
                return gtk::Ordering::Equal;
            };
            if a.group || b.group {
                return a.key.to_lowercase().cmp(&b.key.to_lowercase()).into();
            }
            let v = |r: &Row| {
                r.values
                    .get(&sc)
                    .cloned()
                    .unwrap_or_default()
                    .to_lowercase()
            };
            v(&a).cmp(&v(&b)).into()
        })));
        view.append_column(&column);
        table
            .scope_columns
            .borrow_mut()
            .push(((*scope).to_owned(), column));
    }
    table.effective_column.replace(Some(effective_edit.clone()));
    view.sort_by_column(Some(&key_column), gtk::SortType::Ascending);

    // 列の切り替えで、効いている値の書ける列と読むだけの列を入れ替える。
    let show_effective = {
        let (t, edit, read) = (
            table.clone(),
            effective_edit.clone(),
            effective_read.clone(),
        );
        move || {
            let levels = t.columns_mode.borrow().as_str() == "levels";
            edit.set_visible(!levels);
            edit.set_expand(!levels);
            read.set_visible(levels);
            t.show_columns();
        }
    };
    show_effective();

    // 上: 行と列の切り替え、検索。
    let t = table.clone();
    let rows_switch = segmented(
        &[
            ("basic", "Basic"),
            ("customized", "Customized"),
            ("all", "All"),
        ],
        &rows_mode,
        move |m| {
            crate::settings::set_string(ROWS_KEY, m);
            t.rows_mode.replace(m.to_owned());
            t.rebuild();
        },
    );
    rows_switch.set_tooltip_text(Some(&if repository {
        tr!("Basic: the settings the pages here ask about, set or not. Customized: the keys set in this repository or this worktree. All: every key git reads.")
    } else {
        tr!("Basic: the settings the pages here ask about, set or not. Customized: the keys set for all repositories. All: every key git reads.")
    }));
    let t = table.clone();
    let se = show_effective.clone();
    let columns_switch = segmented(
        &[("combined", "Combined"), ("levels", "Levels")],
        &columns_mode,
        move |m| {
            crate::settings::set_string(COLUMNS_KEY, m);
            t.columns_mode.replace(m.to_owned());
            se();
        },
    );
    columns_switch.set_tooltip_text(Some(&tr!(
        "Combined: the value in effect and where it comes from. Levels: a column per source, the one in effect in bold."
    )));
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(&tr!("Search by key or value")));
    search.set_hexpand(true);
    search.set_halign(gtk::Align::End);
    search.set_width_chars(24);
    let t = table.clone();
    search.connect_search_changed(move |e| {
        t.filter.replace(e.text().to_string());
        t.rebuild();
    });
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.append(&rows_switch);
    top.append(&columns_switch);
    top.append(&search);

    // 右クリックのメニュー・Delete・Ctrl+C。
    let group = gio::SimpleActionGroup::new();
    let edit = gio::SimpleAction::new("edit", None);
    let t = table.clone();
    edit.connect_activate(move |_, _| {
        let keys = t.selected_keys();
        if keys.len() != 1 {
            return;
        }
        // その行を見えるところへ出してから（出ていない行にはセルがない）、書けるセルで始める。
        let key = keys[0].clone();
        if let Some(i) = (0..t.sort_model.n_items()).find(|i| {
            t.sort_model
                .item(*i)
                .and_then(|o| row_of(&o))
                .is_some_and(|r| r.key == key && !r.group)
        }) {
            t.view.scroll_to(i, None, gtk::ListScrollFlags::NONE, None);
        }
        let t = t.clone();
        glib::idle_add_local_once(move || {
            // 出ている列のセル（Combined なら効いている値、Levels なら書ける段）。
            let cell = t.cells.borrow().get(&key).and_then(|list| {
                list.iter()
                    .filter_map(Weak::upgrade)
                    .find(|c| *c.key.borrow() == key && c.stack.is_mapped())
            });
            if let Some(cell) = cell {
                cell.begin();
            }
        });
    });
    group.add_action(&edit);
    let remove = gio::SimpleAction::new("remove", None);
    let t = table.clone();
    remove.connect_activate(move |_, _| t.remove_selected());
    group.add_action(&remove);
    let copy = gio::SimpleAction::new("copy", None);
    let t = table.clone();
    copy.connect_activate(move |_, _| t.copy());
    group.add_action(&copy);
    let add = gio::SimpleAction::new("add", None);
    let t = table.clone();
    add.connect_activate(move |_, _| t.begin_adding());
    group.add_action(&add);
    let menu = gio::Menu::new();
    menu.append(
        Some(&if repository {
            tr!("Edit in This Repository")
        } else {
            tr!("Edit for All Repositories")
        }),
        Some("config.edit"),
    );
    menu.append(
        Some(&if repository {
            tr!("Remove from This Repository")
        } else {
            tr!("Remove from All Repositories")
        }),
        Some("config.remove"),
    );
    let copy_section = gio::Menu::new();
    copy_section.append(Some(&tr!("Copy")), Some("config.copy"));
    menu.append_section(None, &copy_section);
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(&view);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let (v, sel, sm, t) = (
        view.clone(),
        selection.clone(),
        sort_model.clone(),
        table.clone(),
    );
    click.connect_pressed(move |_, _, x, y| {
        // 押した行が選ばれていなければ、その行だけを選ぶ（セルの部品から行を探す）。
        let mut widget = v.pick(x, y, gtk::PickFlags::DEFAULT);
        while let Some(w) = widget {
            if let Some(cell) = t.cell_of(&w) {
                let key = cell.key.borrow().clone();
                let n = sm.n_items();
                if let Some(i) = (0..n).find(|i| {
                    sm.item(*i)
                        .and_then(|o| row_of(&o))
                        .is_some_and(|r| r.key == key && !r.group)
                }) && !sel.is_selected(i)
                {
                    sel.select_item(i, true);
                }
                break;
            }
            widget = w.parent();
        }
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
    });
    view.add_controller(click);
    let shortcuts = gtk::ShortcutController::new();
    for (accel, action) in [
        ("Delete", "config.remove"),
        ("BackSpace", "config.remove"),
        ("<Control>c", "config.copy"),
    ] {
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(accel),
            Some(gtk::NamedAction::new(action)),
        ));
    }
    view.add_controller(shortcuts);

    // 足している途中の行: Return で値の欄へ、値で Return なら書く。Escape でやめる。両方から離れたら書くか消す。
    let nv = new_value.clone();
    new_key.connect_activate(move |_| {
        nv.grab_focus();
    });
    let t = table.clone();
    new_value.connect_activate(move |_| t.commit_adding());
    for entry in [&new_key, &new_value] {
        let keys = gtk::EventControllerKey::new();
        let a = adding.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                a.set_visible(false);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        entry.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        let (t, k, v) = (table.clone(), new_key.clone(), new_value.clone());
        focus.connect_leave(move |_| {
            // 隣の欄へ移っただけなら続ける。
            let (t, k, v) = (t.clone(), k.clone(), v.clone());
            glib::idle_add_local_once(move || {
                if !k.has_focus() && !v.has_focus() && !k.is_focus() && !v.is_focus() {
                    let inside = k
                        .root()
                        .and_then(|r| r.focus())
                        .is_some_and(|f| f.is_ancestor(&k) || f.is_ancestor(&v));
                    if !inside {
                        t.commit_adding();
                    }
                }
            });
        });
        entry.add_controller(focus);
    }

    let scrolled = gtk::ScrolledWindow::builder()
        .child(&view)
        .vexpand(true)
        .build();
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&scrolled));
    let plus = gtk::Button::with_label("+");
    plus.set_action_name(Some("config.add"));
    plus.set_tooltip_text(Some(&if repository {
        tr!("Add a key to this repository")
    } else {
        tr!("Add a key for all repositories")
    }));
    let minus = gtk::Button::with_label("−");
    minus.set_action_name(Some("config.remove"));
    minus.set_tooltip_text(Some(&if repository {
        tr!("Remove the selected keys from this repository (Delete)")
    } else {
        tr!("Remove the selected keys from all repositories (Delete)")
    }));
    restore.set_action_name(Some("config.restore"));
    let restore_action = gio::SimpleAction::new("restore", None);
    let t = table.clone();
    restore_action.connect_activate(move |_, _| t.restore());
    group.add_action(&restore_action);
    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let pm = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pm.add_css_class("linked");
    pm.append(&plus);
    pm.append(&minus);
    bottom.append(&pm);
    bottom.append(&removed_label);
    bottom.append(&restore);

    // 消せるのは、選んだ行のどれかにこの段の値があるとき。
    let enable = {
        let (t, remove, edit) = (table.clone(), remove.clone(), edit.clone());
        move || {
            let keys = t.selected_keys();
            let has = t
                .settings()
                .is_some_and(|s| keys.iter().any(|k| s.local(k).is_some()));
            remove.set_enabled(has);
            edit.set_enabled(keys.len() == 1);
        }
    };
    enable();
    let en = enable.clone();
    selection.connect_selection_changed(move |_, _, _| en());

    let t = table.clone();
    s.on_refresh(move |_| {
        t.rebuild();
        enable();
    });

    let page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    page.set_margin_top(18);
    page.set_margin_bottom(18);
    page.set_margin_start(20);
    page.set_margin_end(20);
    page.append(&top);
    page.append(&frame);
    page.append(&adding);
    page.append(&bottom);
    page.insert_action_group("config", Some(&group));
    page
}
