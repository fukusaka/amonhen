//! History の見せ方（macOS 版の `HistoryDisplayMenu`）: 並び順・行に出す項目・1 行表示・マージ済みブランチの
//! 折りたたみ・祖先の強調。History のバーの右端のボタンと、主メニューの View に同じものを出し（docs/ux.md の
//! 「View menu is History's display menu」）、`crate::settings` に置く（Settings > Appearance と同じ鍵）。
//! 項目は窓のアクション（`win.history-order` など）で、状態は設定から作る。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::{HistorySnapshot, Repository};
use amonhen_model::{HistoryOrder, Oid, RefKind, Refs, tr};
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
    /// サイドバーで複数選んだブランチ・タグ。空なら全ブランチ（macOS 版の `Scope.refs`）。
    scope: RefCell<Vec<ScopeRef>>,
    /// 絞り込んでいる間だけ History のバーの左に出す、ブランチの数と All Branches のボタン。
    pub scope_bar: gtk::Box,
    scope_label: gtk::Label,
}

/// 作った History の形（全体の History の世代、開いた merge、絞り込む起点、折りたたむか）。
type Shape = (u64, Vec<Oid>, Vec<Oid>, bool);

/// 絞り込む ref の 1 つ（種類, 名前, コミット）。ref が動いたら、コミットを付け替える。
pub type ScopeRef = (RefKind, String, Oid);

impl Shaper {
    pub fn new(view: &HistoryView, search: Rc<Search>, pending: Rc<Cell<u32>>) -> Rc<Self> {
        view.set_display(display());
        let scope_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let scope_label = gtk::Label::new(None);
        scope_label.add_css_class("dim-label");
        scope_label.set_margin_start(4);
        let all = gtk::Button::with_label(&tr!("All Branches"));
        all.add_css_class("flat");
        scope_bar.append(&scope_label);
        scope_bar.append(&all);
        scope_bar.set_visible(false);
        let shaper = Rc::new(Self {
            view: view.clone(),
            search,
            pending,
            full: RefCell::default(),
            expanded: RefCell::default(),
            shaped: RefCell::default(),
            request: Rc::default(),
            scope: RefCell::default(),
            scope_bar,
            scope_label,
        });
        let weak = Rc::downgrade(&shaper);
        all.connect_clicked(move |_| {
            if let Some(s) = weak.upgrade() {
                s.set_scope(Vec::new(), None);
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

    /// 複数選んだブランチ・タグから辿れる History にする（空なら全ブランチに戻す）。変えたら true。`select` は
    /// 作り直したら選ぶコミット。
    pub fn set_scope(&self, refs: Vec<ScopeRef>, select: Option<Oid>) -> bool {
        if *self.scope.borrow() == refs {
            return false;
        }
        self.scope_label
            .set_text(&tr!("{count} branches", count = refs.len()));
        self.scope_bar.set_visible(!refs.is_empty());
        self.scope.replace(refs);
        self.reshape(select);
        true
    }

    /// ref の一覧が変わった。絞り込んだ ref がほかのプログラムで動いた（rebase・commit・fetch）なら、新しい
    /// コミットに付け替えて絞り込み直す（前のコミットのままだと、消えたコミットから辿ることになる）。
    pub fn show_refs(&self, refs: &Refs) {
        let moved: Vec<ScopeRef> = self
            .scope
            .borrow()
            .iter()
            .map(|(kind, name, target)| {
                let now = refs
                    .entries
                    .iter()
                    .find(|e| e.kind == *kind && e.name == *name)
                    .and_then(|e| e.target)
                    .unwrap_or(*target);
                (*kind, name.clone(), now)
            })
            .collect();
        if moved != *self.scope.borrow() {
            self.scope.replace(moved);
            self.reshape(None);
        }
    }

    fn reshape(&self, select: Option<Oid>) {
        let Some((repo, full)) = self.full.borrow().clone() else {
            return;
        };
        let fold = settings::bool(settings::FOLD_MERGED_BRANCHES);
        let tips: Vec<Oid> = self.scope.borrow().iter().map(|(_, _, t)| *t).collect();
        if !fold && tips.is_empty() {
            // 作っている途中の折りたたみ・絞り込みは捨てる。
            self.request.set(self.request.get() + 1);
            self.shaped.replace(None);
            self.search.show_history(repo, full);
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
        shaper.set_scope(scope, None);
    });
}
