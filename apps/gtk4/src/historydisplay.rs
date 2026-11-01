//! History の見せ方（macOS 版の `HistoryDisplayMenu`）: 並び順・行に出す項目・1 行表示・マージ済みブランチの
//! 折りたたみ・祖先の強調。History のバーの右端のボタンと、主メニューの View に同じものを出し（docs/ux.md の
//! 「View menu is History's display menu」）、`crate::settings` に置く（Settings > Appearance と同じ鍵）。
//! 項目は窓のアクション（`win.history-order` など）で、状態は設定から作る。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::{HistorySnapshot, Repository};
use amonhen_model::{HistoryOrder, Oid, RefEntry, RefKind, Refs, tr};
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::history::{Display, HistoryView};
use crate::menu::{self, Item};
use crate::search::Search;
use crate::settings::{self, items};

/// 行に出す項目（アクションの名前、ビット、見出し）。
fn item_entries() -> [(&'static str, u32, String); 6] {
    [
        ("history-item-refs", items::REFS, tr!("Branches and Tags")),
        ("history-item-author", items::AUTHOR, tr!("Author")),
        ("history-item-date", items::DATE, tr!("Date")),
        ("history-item-id", items::ID, tr!("Commit ID")),
        ("history-item-signature", items::SIGNATURE, tr!("Signature")),
        ("history-item-ci", items::CI, tr!("CI Status")),
    ]
}

/// 切り替えの項目（アクションの名前、設定の鍵）。
const TOGGLES: [(&str, &str); 6] = [
    ("one-line-history", settings::COMPACT_HISTORY_ROWS),
    ("fold-merged-branches", settings::FOLD_MERGED_BRANCHES),
    ("highlight-ancestors", settings::HIGHLIGHT_ANCESTORS),
    (
        "monospaced-commit-message",
        settings::MONOSPACED_COMMIT_MESSAGE,
    ),
    ("all-files-in-one-diff", crate::commitfiles::STACKED),
    ("changed-files-as-tree", crate::commitfiles::FILE_TREE),
];

/// 切り替えの今の値（全ファイルを 1 つの差分に並べるかは、既定が入）。
fn toggle_value(key: &str) -> bool {
    if key == crate::commitfiles::STACKED {
        crate::commitfiles::stacked()
    } else {
        settings::bool(key)
    }
}

/// 設定から作る行の見せ方。
pub fn display() -> Display {
    Display {
        one_line: settings::bool(settings::COMPACT_HISTORY_ROWS),
        items: settings::history_items(),
        highlight_ancestors: settings::bool(settings::HIGHLIGHT_ANCESTORS),
    }
}

/// 並び順・項目と、1 行表示・折りたたみ・祖先の強調、コミットの詳細の見せ方をまとめた節（バーのボタンと、主メニューの
/// View の後ろ）。
pub fn sections() -> Vec<(Option<String>, gio::Menu)> {
    let order = gio::Menu::new();
    for (title, value) in [
        (tr!("Date Order"), HistoryOrder::Date),
        (tr!("Topological Order"), HistoryOrder::Topo),
    ] {
        let item = gio::MenuItem::new(Some(&title), None);
        item.set_action_and_target_value(
            Some("win.history-order"),
            Some(&value.name().to_variant()),
        );
        order.append_item(&item);
    }
    let shown = gio::Menu::new();
    for (action, _, title) in item_entries() {
        shown.append(Some(&title), Some(&format!("win.{action}")));
    }
    let toggles = gio::Menu::new();
    toggles.append(
        Some(&tr!("One Line per Commit")),
        Some("win.one-line-history"),
    );
    toggles.append(
        Some(&tr!("Fold Merged Branches")),
        Some("win.fold-merged-branches"),
    );
    toggles.append(
        Some(&tr!("Highlight Ancestors")),
        Some("win.highlight-ancestors"),
    );
    let details = gio::Menu::new();
    details.append(
        Some(&tr!("Monospaced Commit Message")),
        Some("win.monospaced-commit-message"),
    );
    details.append(
        Some(&tr!("All Files in One Diff")),
        Some("win.all-files-in-one-diff"),
    );
    details.append(
        Some(&tr!("Changed Files as Tree")),
        Some("win.changed-files-as-tree"),
    );
    vec![
        (Some(tr!("Order")), order),
        (Some(tr!("Items")), shown),
        (None, toggles),
        (Some(tr!("Commit Details")), details),
    ]
}

/// `menu` の後ろに [`sections`] を足す。
pub fn append_sections(menu: &gio::Menu) {
    for (label, section) in sections() {
        menu.append_section(label.as_deref(), &section);
    }
}

/// タブのアクション（`win.*`）を足す。設定が変わったら（Settings の窓で変えても）状態を合わせる。
pub fn install_actions(group: &gio::SimpleActionGroup) {
    let order = gio::SimpleAction::new_stateful(
        "history-order",
        Some(glib::VariantTy::STRING),
        &settings::history_order().name().to_variant(),
    );
    order.connect_activate(|_, value| {
        let Some(name) = value.and_then(|v| v.get::<String>()) else {
            return;
        };
        if HistoryOrder::from_name(&name).is_some_and(|o| o != settings::history_order()) {
            settings::set_string(settings::HISTORY_ORDER, &name);
        }
    });
    group.add_action(&order);
    for (name, bit, _) in item_entries() {
        let action = gio::SimpleAction::new_stateful(
            name,
            None,
            &(settings::history_items() & bit != 0).to_variant(),
        );
        action.connect_activate(move |_, _| {
            settings::set_history_items(settings::history_items() ^ bit);
        });
        group.add_action(&action);
    }
    for (name, key) in TOGGLES {
        let action = gio::SimpleAction::new_stateful(name, None, &toggle_value(key).to_variant());
        action.connect_activate(move |_, _| settings::set_bool(key, !toggle_value(key)));
        group.add_action(&action);
    }
    let weak = group.downgrade();
    settings::connect_changed(move |key| {
        let Some(group) = weak.upgrade() else { return };
        let set = |name: &str, state: glib::Variant| {
            if let Some(a) = group
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_state(&state);
            }
        };
        match key {
            settings::HISTORY_ORDER => set(
                "history-order",
                settings::history_order().name().to_variant(),
            ),
            settings::HISTORY_ITEMS => {
                for (name, bit, _) in item_entries() {
                    set(name, (settings::history_items() & bit != 0).to_variant());
                }
            }
            _ => {
                for (name, k) in TOGGLES {
                    if k == key {
                        set(name, toggle_value(k).to_variant());
                    }
                }
            }
        }
    });
}

/// History のバー（右端に Go to HEAD と、見せ方のメニューのボタン）と、そのメニューのボタン。
pub fn bar() -> (gtk::Box, gtk::MenuButton) {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    bar.set_margin_start(6);
    bar.set_margin_end(6);
    bar.set_margin_top(2);
    bar.set_margin_bottom(2);
    let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    space.set_hexpand(true);
    bar.append(&space);
    let head_icon = crate::icon::Icon::default();
    head_icon.set(crate::icon::Kind::Location, false);
    let head = gtk::Button::new();
    head.set_child(Some(&head_icon));
    head.add_css_class("flat");
    head.set_action_name(Some("win.go-to-head"));
    head.set_tooltip_text(Some(&tr!("Go to HEAD (Ctrl+Shift+H)")));
    bar.append(&head);
    let model = gio::Menu::new();
    append_sections(&model);
    let icon = crate::icon::Icon::default();
    icon.set(crate::icon::Kind::Sliders, false);
    let button = gtk::MenuButton::builder()
        .child(&icon)
        .menu_model(&model)
        .tooltip_text(tr!("History Display: order, items, one line per commit"))
        .build();
    button.add_css_class("flat");
    bar.append(&button);
    (bar, button)
}

/// 表示する History を作る: 折りたたむなら、マージ済みブランチを折りたたんだ History（`history_folded`）。
/// 並び順が変わったらコアに並べ直させる（git は読み直さない）。
pub struct Shaper {
    view: HistoryView,
    search: Rc<Search>,
    /// 裏で作っている数（`--snapshot` はなくなるまで待つ）。
    pending: Rc<Cell<u32>>,
    /// 最後に受け取った、全体の History。
    full: RefCell<Option<(Arc<Repository>, Arc<HistorySnapshot>)>>,
    /// 折りたたんだ History で開いた merge。
    expanded: RefCell<HashSet<Oid>>,
    /// 最後に出した形。同じなら作り直さない。
    shaped: RefCell<Option<Shape>>,
    /// 頼んだ回数（古い結果を捨てるため）。
    request: Rc<Cell<u64>>,
    /// 表示する範囲（macOS 版の `HistoryViewController.Scope`）。
    scope: RefCell<Scope>,
    /// `Scope::Ref` で一緒に辿る起点（選んだブランチの上流。先にある・遅れているコミットを両方見せる）。
    companions: RefCell<Vec<Oid>>,
    /// HEAD のコミット（`Scope::Current` で使う）。
    head: Cell<Option<Oid>>,
    /// 最後に受け取った ref の一覧（選んだブランチの上流を引く）。
    refs: RefCell<Refs>,
    /// History のバーの左に出す、範囲を選ぶもの（All Branches・Current Branch・絞り込んだ ref）。
    pub scope_bar: gtk::DropDown,
    /// 範囲を選ぶものを作り直している間（選んだ知らせを無視する）。
    updating_scope: Cell<bool>,
}

/// 表示する範囲（macOS 版の `HistoryViewController.Scope`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// 全ブランチ。
    #[default]
    All,
    /// 今のブランチ（HEAD から辿れるもの）。HEAD が動いたら絞り込み直す。
    Current,
    /// サイドバーで選んだ ref。ref が動いたら、コミットを付け替える。
    Ref(ScopeRef),
    /// サイドバーで複数選んだ ref。それぞれから辿れるコミットをまとめて出す。
    Refs(Vec<ScopeRef>),
}

/// 作った History の形（全体の History の世代、開いた merge、絞り込む起点、折りたたむか）。
type Shape = (u64, Vec<Oid>, Vec<Oid>, bool);

/// 絞り込む ref の 1 つ（種類, 名前, コミット）。ref が動いたら、コミットを付け替える。
pub type ScopeRef = (RefKind, String, Oid);

impl Shaper {
    pub fn new(view: &HistoryView, search: Rc<Search>, pending: Rc<Cell<u32>>) -> Rc<Self> {
        view.set_display(display());
        let scope_bar = gtk::DropDown::from_strings(&[]);
        scope_bar.set_margin_start(4);
        let shaper = Rc::new(Self {
            view: view.clone(),
            search,
            pending,
            full: RefCell::default(),
            expanded: RefCell::default(),
            shaped: RefCell::default(),
            request: Rc::default(),
            scope: RefCell::default(),
            companions: RefCell::default(),
            head: Cell::default(),
            refs: RefCell::default(),
            scope_bar,
            updating_scope: Cell::new(false),
        });
        shaper.update_scope_bar();
        let weak = Rc::downgrade(&shaper);
        shaper.scope_bar.connect_selected_notify(move |bar| {
            let Some(s) = weak.upgrade() else { return };
            if s.updating_scope.get() {
                return;
            }
            match bar.selected() {
                0 => s.set_scope(Scope::All, None, Vec::new()),
                1 => s.set_scope(Scope::Current, None, Vec::new()),
                _ => {}
            }
        });
        let weak = Rc::downgrade(&shaper);
        settings::connect_changed(move |key| {
            let Some(s) = weak.upgrade() else { return };
            match key {
                settings::COMPACT_HISTORY_ROWS
                | settings::HISTORY_ITEMS
                | settings::HIGHLIGHT_ANCESTORS => s.view.set_display(display()),
                settings::FOLD_MERGED_BRANCHES => {
                    s.expanded.borrow_mut().clear();
                    s.reshape(None);
                }
                settings::HISTORY_ORDER => s.reorder(),
                _ => {}
            }
        });
        shaper
    }

    /// 読み直した（全体の）History を出す。
    pub fn present(&self, repo: Arc<Repository>, full: Arc<HistorySnapshot>) {
        self.full.replace(Some((repo, full)));
        self.reshape(None);
    }

    /// 範囲を選ぶものを今の範囲に合わせる（3 つめは絞り込んだ ref の名前か、ブランチの数。macOS 版の
    /// `updateScopePopup`）。
    fn update_scope_bar(&self) {
        let mut titles = vec![tr!("All Branches"), tr!("Current Branch")];
        let selected = match &*self.scope.borrow() {
            Scope::All => 0,
            Scope::Current => 1,
            Scope::Ref((_, name, _)) => {
                titles.push(name.clone());
                2
            }
            Scope::Refs(list) => {
                titles.push(tr!("{count} branches", count = list.len()));
                2
            }
        };
        let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        self.updating_scope.set(true);
        self.scope_bar
            .set_model(Some(&gtk::StringList::new(&titles)));
        self.scope_bar.set_selected(selected);
        self.updating_scope.set(false);
    }

    /// 表示する範囲を変えて作り直す。`select` は作り直したら選ぶコミット、`companions` は `Scope::Ref` のときに
    /// 一緒に辿る起点（ブランチの上流）。
    pub fn set_scope(&self, scope: Scope, select: Option<Oid>, companions: Vec<Oid>) {
        self.scope.replace(scope);
        self.companions.replace(companions);
        self.update_scope_bar();
        self.reshape(select);
    }

    /// 全ブランチでなければ全ブランチに戻して `id` を選ぶ（今の範囲にないコミットへ移るとき。macOS 版の
    /// `set(scope: .all, select:)`）。戻したら true。
    pub fn show_all_selecting(&self, id: Oid) -> bool {
        if *self.scope.borrow() == Scope::All {
            return false;
        }
        self.set_scope(Scope::All, Some(id), Vec::new());
        true
    }

    /// サイドバー・詳細で選んだブランチ・タグの History に絞り込み、指すコミットを選ぶ（macOS 版の
    /// `sidebar.onSelect`・`selectRef(named:)`）。`with_upstream` なら、上流と分かれたローカルブランチは上流も
    /// 一緒に辿る。ブランチ・リモートブランチ・タグでなければ false。
    pub fn scope_to_ref(&self, entry: &RefEntry, with_upstream: bool) -> bool {
        let Some(target) = entry.target else {
            return false;
        };
        if !matches!(entry.kind, RefKind::Local | RefKind::Remote | RefKind::Tag) {
            return false;
        }
        let upstream = (with_upstream
            && entry.kind == RefKind::Local
            && (entry.ahead > 0 || entry.behind > 0))
            .then(|| {
                self.refs
                    .borrow()
                    .entries
                    .iter()
                    .find(|e| e.kind == RefKind::Remote && Some(&e.name) == entry.detail.as_ref())
                    .and_then(|e| e.target)
            })
            .flatten();
        self.set_scope(
            Scope::Ref((entry.kind, entry.name.clone(), target)),
            Some(target),
            upstream.into_iter().collect(),
        );
        true
    }

    /// 名前の ref（ブランチ・リモートブランチ・タグ）の History に絞り込む（詳細の欄の ref のリンク）。なければ
    /// false。
    pub fn scope_to_ref_named(&self, name: &str) -> bool {
        let entry = self
            .refs
            .borrow()
            .entries
            .iter()
            .find(|e| {
                e.name == name && matches!(e.kind, RefKind::Local | RefKind::Remote | RefKind::Tag)
            })
            .cloned();
        entry.is_some_and(|e| self.scope_to_ref(&e, false))
    }

    /// ref の一覧が変わった。絞り込んだ ref がほかのプログラムで動いた（rebase・commit・fetch）なら、新しい
    /// コミットに付け替えて絞り込み直す（前のコミットのままだと、消えたコミットから辿ることになる）。今の
    /// ブランチに絞っていて HEAD が動いたら、絞り込み直す。
    pub fn show_refs(&self, refs: &Refs) {
        let now = |kind: RefKind, name: &str, target: Oid| {
            refs.entries
                .iter()
                .find(|e| e.kind == kind && e.name == name)
                .and_then(|e| e.target)
                .unwrap_or(target)
        };
        let moved = match &*self.scope.borrow() {
            Scope::Ref((kind, name, target)) => {
                Scope::Ref((*kind, name.clone(), now(*kind, name, *target)))
            }
            Scope::Refs(list) => Scope::Refs(
                list.iter()
                    .map(|(kind, name, target)| (*kind, name.clone(), now(*kind, name, *target)))
                    .collect(),
            ),
            other => other.clone(),
        };
        self.refs.replace(refs.clone());
        let head_moved = self.head.replace(refs.head) != refs.head;
        let scope_moved = moved != *self.scope.borrow();
        if scope_moved {
            self.scope.replace(moved);
        }
        if scope_moved || (head_moved && *self.scope.borrow() == Scope::Current) {
            self.reshape(None);
        }
    }

    /// 絞り込む起点（空なら全ブランチ）。
    fn tips(&self) -> Vec<Oid> {
        match &*self.scope.borrow() {
            Scope::All => Vec::new(),
            Scope::Current => self.head.get().into_iter().collect(),
            Scope::Ref((_, _, target)) => std::iter::once(*target)
                .chain(self.companions.borrow().iter().copied())
                .collect(),
            Scope::Refs(list) => list.iter().map(|(_, _, t)| *t).collect(),
        }
    }

    fn reshape(&self, select: Option<Oid>) {
        let Some((repo, full)) = self.full.borrow().clone() else {
            return;
        };
        let fold = settings::bool(settings::FOLD_MERGED_BRANCHES);
        let tips = self.tips();
        if !fold && tips.is_empty() {
            // 作っている途中の折りたたみ・絞り込みは捨てる。
            self.request.set(self.request.get() + 1);
            self.shaped.replace(None);
            self.search.show_history(repo, full.clone());
            if let Some(id) = select {
                select_when_found(&self.view, full, id, self.pending.clone());
            }
            return;
        }
        let mut expanded: Vec<Oid> = if fold {
            self.expanded.borrow().iter().copied().collect()
        } else {
            Vec::new()
        };
        expanded.sort_unstable();
        let key = (full.generation, expanded.clone(), tips.clone(), fold);
        // 同じ History を同じ形で作っている・作ったなら、そのまま（読み直すたびに同じ History が来る）。
        if select.is_none() && self.shaped.borrow().as_ref() == Some(&key) {
            return;
        }
        self.shaped.replace(Some(key));
        // まだ何も出していなければ、作り終えるまでは全体を出しておく（macOS 版も前回の History をすぐ出す）。
        if self.view.current().is_none() {
            self.search.show_history(repo.clone(), full.clone());
        }
        self.request.set(self.request.get() + 1);
        let request = self.request.get();
        let (search, view, pending) =
            (self.search.clone(), self.view.clone(), self.pending.clone());
        pending.set(pending.get() + 1);
        let r = repo.clone();
        let current = self.request.clone();
        let whole = full.graph.len();
        glib::spawn_future_local(async move {
            let shaped = gio::spawn_blocking(move || {
                let some = (!tips.is_empty()).then_some(&tips[..]);
                if fold {
                    r.history_folded(some, &expanded)
                } else {
                    r.history_filtered(&tips)
                }
                .map_err(|e| e.to_string())
            })
            .await;
            pending.set(pending.get() - 1);
            if current.get() != request {
                return;
            }
            match shaped {
                // 起点がもう History にない（ほかのプログラムの rebase で消えたコミットを、ref を読み直す前に
                // 使った）と空になる。空にはせず今の表示を残し、ref を読み直して付け替えたときに出し直す。
                Ok(Ok(shaped)) if shaped.graph.is_empty() && whole > 0 => {}
                Ok(Ok(shaped)) => {
                    let row = select.and_then(|id| shaped.row_of(&id));
                    search.show_history(repo, shaped);
                    if let Some(row) = row {
                        view.select(row);
                    }
                }
                Ok(Err(e)) => eprintln!("History を作れない: {e}"),
                Err(_) => {}
            }
        });
    }

    /// 並び順が変わった。コアに並べ直させて、出し直す。
    fn reorder(self: &Rc<Self>) {
        let Some((repo, _)) = self.full.borrow().clone() else {
            return;
        };
        let order = settings::history_order();
        let pending = self.pending.clone();
        pending.set(pending.get() + 1);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let r = repo.clone();
            let latest = gio::spawn_blocking(move || {
                r.set_history_order(order)?;
                r.load_history()
            })
            .await;
            pending.set(pending.get() - 1);
            let Some(s) = weak.upgrade() else { return };
            match latest {
                Ok(Ok(history)) => s.present(repo, history),
                Ok(Err(e)) => eprintln!("History を並べ直せない: {e}"),
                Err(_) => {}
            }
        });
    }

    /// 折りたたんだ merge の右クリックの項目（取り込んだ側を開く・たたむ）。なければ空。
    pub fn merge_items(self: &Rc<Self>, history: &HistorySnapshot, row: u32) -> Vec<Item> {
        let Some(&id) = history.graph.ids().get(row as usize) else {
            return Vec::new();
        };
        let hidden = history.folded.get(row as usize).copied().unwrap_or(0);
        if hidden == 0 && !self.expanded.borrow().contains(&id) {
            return Vec::new();
        }
        let title = if hidden > 0 {
            tr!("Show {count} Merged Commits", count = hidden)
        } else {
            tr!("Fold Merged Commits")
        };
        let weak = Rc::downgrade(self);
        vec![
            menu::action(title, move || {
                if let Some(s) = weak.upgrade() {
                    s.toggle_merge(id);
                }
            }),
            Item::Separator,
        ]
    }

    /// 折りたたんだ merge を開く・たたむ。
    fn toggle_merge(&self, id: Oid) {
        {
            let mut expanded = self.expanded.borrow_mut();
            if !expanded.remove(&id) {
                expanded.insert(id);
            }
        }
        self.reshape(Some(id));
    }
}

/// サイドバーでブランチ・タグを 2 つ以上選んだら、History に切り替えて、それぞれから辿れる履歴をまとめて出す
/// （macOS 版の `onSelectMany`）。
pub fn connect_select_many(
    shaper: &Rc<Shaper>,
    sidebar: &crate::sidebar::Sidebar,
    pages: &gtk::Stack,
) {
    let (shaper, pages) = (Rc::downgrade(shaper), pages.downgrade());
    sidebar.connect_select_many(move |refs| {
        let (Some(shaper), Some(pages)) = (shaper.upgrade(), pages.upgrade()) else {
            return;
        };
        pages.set_visible_child_name("history");
        let scope = refs
            .iter()
            .filter_map(|e| Some((e.kind, e.name.clone(), e.target?)))
            .collect();
        shaper.set_scope(Scope::Refs(scope), None, Vec::new());
    });
}

/// `history` で `id` の行を裏で引き、見つかったら（まだ同じ History を出していれば）選ぶ。
fn select_when_found(
    view: &HistoryView,
    history: Arc<HistorySnapshot>,
    id: Oid,
    pending: Rc<Cell<u32>>,
) {
    // 行の索引は初回に作る（150 万コミットで 80 ms ほど）ので、裏で引く。
    let weak = view.downgrade();
    let generation = history.generation;
    pending.set(pending.get() + 1);
    glib::spawn_future_local(async move {
        let row = gio::spawn_blocking(move || history.row_of(&id)).await;
        pending.set(pending.get() - 1);
        let (Some(view), Ok(Some(row))) = (weak.upgrade(), row) else {
            return;
        };
        if view.current().map(|(_, h)| h.generation) == Some(generation) {
            view.select(row);
        }
    });
}
