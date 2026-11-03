//! 見出しのツールバーのボタンの並び（macOS 版の `MainToolbar` と、NSToolbar のカスタマイズ）。View > Customize
//! Toolbar… で見出しの下にパレットを出し、出している間は見出しのボタンを押す代わりにドラッグで並べ替える: パレットの項目を
//! 見出しにドラッグして足し、見出しの項目を外にドラッグして外し、既定のセットをドラッグして元に戻す（macOS 版と同じ）。
//! 見出しの左（Repository のメニューの右）と右（主メニューの左）に分けて並べ、境目は伸縮するスペース（macOS 版の
//! flexibleSpace。見出しには 1 つだけ）。間にスペースを入れられる。並びはアプリ全体で 1 つ（設定の
//! `toolbarItemsV3`）で、変えるとすべての窓のタブに効く。
//!
//! Repository のメニューと主メニュー（≡）は、macOS 版のメニューバーの代わりなので動かさない。リポジトリの名前は
//! 並びの 1 つで、並びから外すと見出しの真ん中に出す。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::icon;

/// 設定の鍵。並びを `,` でつなぐ（出さないものは `-` を前に付ける）。既定の並びを変えたら名前を変える（macOS 版の
/// ツールバーの名前と同じく、前に保存した並びに新しいボタンが出てこないため）。
const KEY: &str = "toolbarItemsV3";
/// 前の版の鍵（検索がまだ並びになかった）。読むときに検索を右の終わりに足す。
const KEY_V2: &str = "toolbarItemsV2";
/// スペース（いくつでも入れられる）。
const SPACE: &str = "space";
/// 伸縮するスペース。ここから右は見出しの右に並べる。
const SPLIT: &str = "|";

/// macOS 版の既定の並び（サイドバーの切り替え・Repositories・リポジトリの名前・Branch・Fetch・Pull・Push、
/// Merge・Rebase、Stash・Pop、右に Activity・Refresh・Undo・検索）。
const DEFAULT: &[&str] = &[
    "sidebar",
    "repositories",
    "repository",
    "branch",
    "fetch",
    "pull",
    "push",
    SPACE,
    "merge",
    "rebase",
    SPACE,
    "stash",
    "pop",
    SPLIT,
    "activity",
    "refresh",
    "undo",
    "search",
];
/// 既定では出さないボタン（macOS 版のツールバーに足せるもの）。
const OPTIONAL: &[&str] = &["new-branch", "terminal", "files"];
/// パレットの並び（macOS 版の `toolbarAllowedItemIdentifiers` の順。≡ は動かさないので除く）。
const PALETTE: &[&str] = &[
    "sidebar",
    "repositories",
    "repository",
    "branch",
    "search",
    "fetch",
    "pull",
    "push",
    "merge",
    "rebase",
    "stash",
    "pop",
    "new-branch",
    "activity",
    "refresh",
    "undo",
    "terminal",
    "files",
    SPACE,
    SPLIT,
];

/// 並びの 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Item { id: String, shown: bool },
    Space,
    Split,
}

/// パレットに出す名前（macOS 版の paletteLabel。操作の名前は英語のまま）。
fn name(id: &str) -> String {
    match id {
        "sidebar" => tr!("Show Sidebar"),
        "repository" => "Repository".to_owned(),
        // macOS 版と同じく、新しいブランチも「Branch」（印で見分ける）。
        "branch" | "new-branch" => "Branch".to_owned(),
        "search" => tr!("Search"),
        "fetch" => "Fetch".to_owned(),
        "pull" => "Pull".to_owned(),
        "push" => "Push".to_owned(),
        "merge" => "Merge".to_owned(),
        "rebase" => "Rebase".to_owned(),
        "stash" => "Stash".to_owned(),
        "pop" => "Pop".to_owned(),
        "repositories" => "Repositories".to_owned(),
        "terminal" => tr!("Terminal"),
        "files" => tr!("Files"),
        "activity" => "Activity".to_owned(),
        "refresh" => tr!("Refresh"),
        "undo" => "Undo".to_owned(),
        SPACE => tr!("Space"),
        SPLIT => tr!("Flexible Space"),
        _ => id.to_owned(),
    }
}

fn known(id: &str) -> bool {
    DEFAULT.contains(&id) && id != SPACE && id != SPLIT || OPTIONAL.contains(&id)
}

/// 既定の並び（足せるボタンは、出さないものとして後ろに）。
fn default_entries() -> Vec<Entry> {
    parse(&DEFAULT.join(","))
}

/// 設定の文字列を読む。知らない名前は捨て、載っていないボタンは出さないものとして後ろに足す。
fn parse(text: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut split = false;
    for part in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match part {
            SPACE => entries.push(Entry::Space),
            SPLIT if !split => {
                split = true;
                entries.push(Entry::Split);
            }
            _ => {
                let (id, shown) = match part.strip_prefix('-') {
                    Some(id) => (id, false),
                    None => (part, true),
                };
                let seen = entries
                    .iter()
                    .any(|e| matches!(e, Entry::Item { id: i, .. } if i == id));
                if known(id) && !seen {
                    entries.push(Entry::Item {
                        id: id.to_owned(),
                        shown,
                    });
                }
            }
        }
    }
    if !split {
        entries.push(Entry::Split);
    }
    for id in DEFAULT
        .iter()
        .chain(OPTIONAL)
        .filter(|id| **id != SPACE && **id != SPLIT)
    {
        if !entries
            .iter()
            .any(|e| matches!(e, Entry::Item { id: i, .. } if i == id))
        {
            entries.push(Entry::Item {
                id: (*id).to_owned(),
                shown: false,
            });
        }
    }
    entries
}

fn format(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|e| match e {
            Entry::Item { id, shown: true } => id.clone(),
            Entry::Item { id, shown: false } => format!("-{id}"),
            Entry::Space => SPACE.to_owned(),
            Entry::Split => SPLIT.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// ツールバーに `id` のボタンを出しているか。
pub fn shows(id: &str) -> bool {
    entries()
        .iter()
        .any(|e| matches!(e, Entry::Item { id: i, shown: true } if i == id))
}

/// 並びを覚える設定の鍵（変わったら `crate::settings::connect_changed` で知らせる）。
pub const ITEMS_KEY: &str = KEY;

fn entries() -> Vec<Entry> {
    if let Some(s) = crate::settings::string(KEY) {
        return parse(&s);
    }
    crate::settings::string(KEY_V2).map_or_else(default_entries, |s| from_v2(&s))
}

/// 前の版の並びを読む（並べたものはそのままに、検索を右の終わりに足す。前の版では検索はそこに決まっていた）。
fn from_v2(text: &str) -> Vec<Entry> {
    let mut all = parse(text);
    all.retain(|e| !matches!(e, Entry::Item { id, .. } if id == "search"));
    insert_at(
        &mut all,
        Side::End,
        usize::MAX,
        Entry::Item {
            id: "search".to_owned(),
            shown: true,
        },
    );
    all
}

/// 見出しの左か右か。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Start,
    End,
}

/// ドラッグしているもの（パレット・見出しから、見出し・パレットへ）。
#[derive(Clone, Debug, PartialEq, Eq, glib::Boxed)]
#[boxed_type(name = "AmonhenToolbarDrag")]
enum Dragged {
    /// 見出しにある `n` 番目の並び。
    Move(usize),
    /// パレットの項目（ボタン・スペース・伸縮するスペース）。
    Add(String),
    /// 既定のセット。
    Default,
}

/// `side` に出ているもの（出すボタンとスペース）の、並びの中の位置。
fn shown_on(all: &[Entry], side: Side) -> Vec<usize> {
    let split = all
        .iter()
        .position(|e| *e == Entry::Split)
        .unwrap_or(all.len());
    all.iter()
        .enumerate()
        .filter(|(i, e)| {
            (side == Side::Start) == (*i < split)
                && matches!(e, Entry::Space | Entry::Item { shown: true, .. })
        })
        .map(|(i, _)| i)
        .collect()
}

/// `side` の `slot` 番目（出ているものの中の位置。多すぎれば終わり）に `entry` を入れる。
fn insert_at(all: &mut Vec<Entry>, side: Side, slot: usize, entry: Entry) {
    let shown = shown_on(all, side);
    let split = all
        .iter()
        .position(|e| *e == Entry::Split)
        .unwrap_or(all.len());
    let at = match (shown.get(slot), shown.last(), side) {
        (Some(&i), _, _) => i,
        (None, Some(&last), _) => last + 1,
        (None, None, Side::Start) => 0,
        (None, None, Side::End) => (split + 1).min(all.len()),
    };
    all.insert(at, entry);
}

/// `dragged` を `side` の `slot` 番目に落としたあとの並び。
fn dropped(all: &[Entry], dragged: &Dragged, side: Side, slot: usize) -> Vec<Entry> {
    let mut all = all.to_vec();
    // 同じ側の前から動かすなら、取り除いた分だけ詰める。
    let take = |all: &mut Vec<Entry>, index: usize, slot: usize| -> usize {
        let before = shown_on(all, side).iter().take(slot).any(|&i| i == index);
        all.remove(index);
        if before { slot - 1 } else { slot }
    };
    match dragged {
        Dragged::Default => return default_entries(),
        Dragged::Move(index) => {
            let Some(entry) = all.get(*index).cloned() else {
                return all;
            };
            if entry == Entry::Split {
                return all;
            }
            let slot = take(&mut all, *index, slot);
            insert_at(&mut all, side, slot, entry);
        }
        Dragged::Add(id) if id == SPACE => insert_at(&mut all, side, slot, Entry::Space),
        Dragged::Add(id) if id == SPLIT => {
            // 左右の出ているものを通して数え、その前に境目を移す。
            let starts = shown_on(&all, Side::Start).len();
            let at = if side == Side::Start {
                slot.min(starts)
            } else {
                starts + slot
            };
            all.retain(|e| *e != Entry::Split);
            // 境目がなければ、出ているものはみな左に数える。
            let combined = shown_on(&all, Side::Start);
            match combined.get(at) {
                Some(&i) => all.insert(i, Entry::Split),
                None => {
                    // 出ているものの終わりの後（出していないものより前）に。
                    let after = combined.last().map_or(0, |&i| i + 1);
                    all.insert(after, Entry::Split);
                }
            }
        }
        Dragged::Add(id) => {
            if !known(id) {
                return all;
            }
            let mut slot = slot;
            if let Some(index) = all
                .iter()
                .position(|e| matches!(e, Entry::Item { id: i, .. } if i == id))
            {
                slot = take(&mut all, index, slot);
            }
            insert_at(
                &mut all,
                side,
                slot,
                Entry::Item {
                    id: id.clone(),
                    shown: true,
                },
            );
        }
    }
    all
}

/// 見出しの外にドラッグして外したあとの並び（スペースは消し、ボタンは出さないものにする）。
fn removed(all: &[Entry], index: usize) -> Vec<Entry> {
    let mut all = all.to_vec();
    match all.get_mut(index) {
        Some(Entry::Space) => {
            all.remove(index);
        }
        Some(Entry::Item { shown, .. }) => *shown = false,
        _ => {}
    }
    all
}

fn save(entries: &[Entry]) {
    crate::settings::set_string(KEY, &format(entries));
}

/// タブごとに作ってあるボタン（ほかの所でも中身を変えるもの）。
pub struct Given {
    /// 見出し（リポジトリの名前を並びから外したら、真ん中に戻す）。
    pub header: gtk::HeaderBar,
    /// リポジトリの名前（押すとリポジトリの一覧）。
    pub repository: gtk::Widget,
    /// 見出しの真ん中にいつも出すもの（リポジトリのページの「Repositories」と説明。macOS 版の窓の名前）。なければ
    /// 真ん中はリポジトリの名前を並びから外したときだけ使う。
    pub title: Option<gtk::Widget>,
    pub branch: gtk::Button,
    pub pull: gtk::Button,
    pub push: gtk::Button,
    pub undo: gtk::Button,
    /// History の検索のボタン（macOS 版の検索欄）。
    pub search: gtk::Widget,
}

fn icon_button(kind: icon::Kind, action: &str, tip: &str) -> gtk::Button {
    let icon = icon::Icon::default();
    icon.set(kind, false);
    let button = gtk::Button::new();
    button.set_child(Some(&icon));
    button.set_action_name(Some(action));
    button.set_tooltip_text(Some(tip));
    button
}

/// 文字のボタン（テーマにアイコンがあればアイコン）。
fn text_button(icon_name: &str, label: &str, action: &str, tip: &str) -> gtk::Button {
    let has_icon = gtk::gdk::Display::default()
        .is_some_and(|d| gtk::IconTheme::for_display(&d).has_icon(icon_name));
    let button = if has_icon {
        gtk::Button::from_icon_name(icon_name)
    } else {
        gtk::Button::with_label(label)
    };
    button.set_action_name(Some(action));
    button.set_tooltip_text(Some(tip));
    button
}

/// ボタンを作り、`start`（見出しの左）と `end`（右）に設定の並びで入れる。並びが変わったら入れ直す。
pub fn install(start: &gtk::Box, end: &gtk::Box, given: Given) {
    let mut buttons: HashMap<&'static str, gtk::Widget> = HashMap::new();
    // サイドバーの切り替え（F9。macOS 版のツールバーの toggleSidebar）。状態はアクションと揃う。
    let sidebar = gtk::ToggleButton::new();
    let sidebar_icon = icon::Icon::default();
    sidebar_icon.set(icon::Kind::Sidebar, false);
    sidebar.set_child(Some(&sidebar_icon));
    sidebar.set_action_name(Some("win.show-sidebar"));
    sidebar.set_tooltip_text(Some(&format!("{} (F9)", tr!("Show Sidebar"))));
    buttons.insert("sidebar", sidebar.upcast());
    buttons.insert("repository", given.repository.clone());
    buttons.insert("branch", given.branch.upcast());
    buttons.insert(
        "fetch",
        icon_button(
            icon::Kind::Fetch,
            "win.fetch",
            &tr!("Fetch all remotes (Ctrl+Alt+F)"),
        )
        .upcast(),
    );
    buttons.insert("pull", given.pull.upcast());
    buttons.insert("push", given.push.upcast());
    buttons.insert(
        "merge",
        icon_button(
            icon::Kind::Merge,
            "win.merge",
            &tr!("Merge a branch, tag or the selected commit into the current branch (Ctrl+Alt+M)"),
        )
        .upcast(),
    );
    buttons.insert(
        "rebase",
        icon_button(
            icon::Kind::Rebase,
            "win.rebase",
            &tr!(
                "Rebase the current branch onto a branch, tag or the selected commit (Ctrl+Alt+R)"
            ),
        )
        .upcast(),
    );
    buttons.insert(
        "stash",
        icon_button(
            icon::Kind::StashPush,
            "win.stash",
            &tr!("Stash your uncommitted changes (Ctrl+Alt+S)"),
        )
        .upcast(),
    );
    buttons.insert(
        "pop",
        icon_button(
            icon::Kind::StashPop,
            "win.pop-stash",
            &tr!("Apply the latest stash and remove it (Ctrl+Alt+Shift+S)"),
        )
        .upcast(),
    );
    // 新しいブランチ: ブランチの印に ＋ を添える。
    let new_branch = icon_button(
        icon::Kind::Branch,
        "win.new-branch",
        &tr!("New branch from HEAD (Ctrl+Alt+B)"),
    );
    if let Some(icon) = new_branch.child() {
        new_branch.set_child(None::<&gtk::Widget>);
        let inner = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        inner.append(&icon);
        inner.append(&gtk::Label::new(Some("+")));
        new_branch.set_child(Some(&inner));
    }
    buttons.insert("new-branch", new_branch.upcast());
    buttons.insert(
        "repositories",
        text_button(
            "view-grid-symbolic",
            "Repositories",
            "win.repositories",
            &tr!("Show the Repositories page (Ctrl+Shift+R)"),
        )
        .upcast(),
    );
    buttons.insert(
        "terminal",
        text_button(
            "utilities-terminal-symbolic",
            &tr!("Terminal"),
            "win.open-in-terminal",
            &tr!("Open in Terminal"),
        )
        .upcast(),
    );
    buttons.insert(
        "files",
        icon_button(
            icon::Kind::Folder,
            "win.reveal-in-files",
            &tr!("Show in Files"),
        )
        .upcast(),
    );
    buttons.insert("activity", crate::activity::toolbar_button().upcast());
    // Refresh: 押せば読み直し、▾ で History を作り直す（macOS 版と同じ）。
    let refresh = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    refresh.add_css_class("linked");
    let reload = icon_button(
        icon::Kind::Refresh,
        "win.refresh",
        &tr!("Reload History, branches and Working Copy (Ctrl+R)"),
    );
    refresh.append(&reload);
    let more = gio::Menu::new();
    crate::menu::append_with_tip(
        &more,
        &tr!("Rebuild History"),
        "win.rebuild-history",
        Some(&tr!(
            "Discard the History cache and rebuild it from scratch (over the shared daemon, the daemon’s too)"
        )),
    );
    let arrow = gtk::MenuButton::builder()
        .menu_model(&more)
        .tooltip_text(tr!("Refresh"))
        .build();
    crate::menu::tooltips_on_show(&arrow);
    // ▾ は Refresh と一緒に押せなくする（リポジトリのページでは、macOS 版と同じく Refresh ごと押せない）。
    // （アクションがないボタンは、部品の sensitive ではなく状態の印で押せなくなるので、印を見る）。
    let follow = {
        let arrow = arrow.downgrade();
        move |reload: &gtk::Button| {
            if let Some(arrow) = arrow.upgrade() {
                arrow.set_sensitive(!reload.state_flags().contains(gtk::StateFlags::INSENSITIVE));
            }
        }
    };
    follow(&reload);
    reload.connect_state_flags_changed(move |reload, _| follow(reload));
    refresh.append(&arrow);
    buttons.insert("refresh", refresh.upcast());
    buttons.insert("undo", given.undo.upcast());
    buttons.insert("search", given.search.clone());

    // リポジトリの名前を並べるときの、見出しの真ん中（空けておく。窓の名前を重ねて出さない）。
    let center = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    // 見出しに入れた部品と、その並びの中の位置（カスタマイズの間、どれをドラッグしたかを見分ける）。
    let placed: Placed = Rc::default();
    let fill = {
        let (start, end) = (start.downgrade(), end.downgrade());
        let (header, repository) = (given.header.downgrade(), given.repository.clone());
        let placed = placed.clone();
        let title = given.title.clone();
        move || {
            let (Some(start), Some(end), Some(header)) =
                (start.upgrade(), end.upgrade(), header.upgrade())
            else {
                return;
            };
            let customizing = customizing();
            for b in [&start, &end] {
                while let Some(child) = b.first_child() {
                    child.set_can_target(true);
                    b.remove(&child);
                }
                // カスタマイズの間は、空でも落とせるよう幅を取り、枠を出す。
                if customizing {
                    b.add_css_class("toolbar-customizing");
                } else {
                    b.remove_css_class("toolbar-customizing");
                }
            }
            let all = entries();
            let shown = all
                .iter()
                .any(|e| matches!(e, Entry::Item { id, shown: true } if id == "repository"));
            match &title {
                Some(title) => header.set_title_widget(Some(title)),
                None if shown => header.set_title_widget(Some(&center)),
                None => header.set_title_widget(Some(&repository)),
            }
            let mut target = (&start, Side::Start);
            let mut now = Vec::new();
            for (index, entry) in all.iter().enumerate() {
                let widget = match entry {
                    Entry::Split => {
                        target = (&end, Side::End);
                        continue;
                    }
                    Entry::Space => {
                        let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        space.set_width_request(8);
                        if customizing {
                            space.add_css_class("toolbar-space");
                        }
                        space.upcast()
                    }
                    Entry::Item { id, shown: true } => match buttons.get(id.as_str()) {
                        Some(w) => w.clone(),
                        None => continue,
                    },
                    Entry::Item { shown: false, .. } => continue,
                };
                // カスタマイズの間は押せない（見出しの箱がドラッグを受ける）。
                widget.set_can_target(!customizing);
                target.0.append(&widget);
                now.push((target.1, widget, index));
            }
            placed.replace(now);
        }
    };
    fill();
    let fill = Rc::new(fill);
    for (b, side) in [(start, Side::Start), (end, Side::End)] {
        drag_from(b, side, &placed);
        drop_on(b.upcast_ref(), Some(side), &placed);
    }
    // 見出しの隙間（真ん中など）に落としたら、近い側の端に入れる。
    drop_on(given.header.upcast_ref(), None, &placed);
    // タブの見出しがある間だけ見る（閉じたタブのボタンを持ち続けない）。
    let f = fill.clone();
    crate::settings::connect_changed_while(&given.header, move |key| {
        if key == KEY {
            f();
        }
    });
    let header = given.header.downgrade();
    TOOLBARS.with(|t| t.borrow_mut().push((header, Rc::new(move || fill()))));
}

/// ツールバーを入れ直すもの。
type Refill = Rc<dyn Fn()>;

/// 見出しに入れた部品と、その側・並びの中の位置。
type Placed = Rc<RefCell<Vec<(Side, gtk::Widget, usize)>>>;

thread_local! {
    /// 窓に出しているパレット（開いている間はカスタマイズ中）。
    static PALETTE_SHOWN: RefCell<glib::WeakRef<gtk::Revealer>> = RefCell::default();
    static CUSTOMIZING: Cell<bool> = const { Cell::new(false) };
    /// 作ったツールバー（見出しがなくなったら外す）と、入れ直すもの。
    static TOOLBARS: RefCell<Vec<(glib::WeakRef<gtk::HeaderBar>, Refill)>> = RefCell::default();
}

fn customizing() -> bool {
    CUSTOMIZING.with(Cell::get)
}

/// カスタマイズを始める・終える（すべての窓のツールバーを入れ直す）。
fn set_customizing(on: bool) {
    CUSTOMIZING.with(|c| c.set(on));
    let fills: Vec<Refill> = TOOLBARS.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|(h, _)| h.upgrade().is_some());
        t.iter().map(|(_, f)| f.clone()).collect()
    });
    for f in fills {
        f();
    }
}

/// `b` の中の `x` にある部品（`placed` の 1 つ）と、その左端。
fn placed_at(
    b: &gtk::Widget,
    placed: &Placed,
    side: Side,
    x: f64,
) -> Option<(gtk::Widget, usize, f64)> {
    placed
        .borrow()
        .iter()
        .filter(|(s, ..)| *s == side)
        .find_map(|(_, w, index)| {
            let r = w.compute_bounds(b)?;
            let (left, right) = (f64::from(r.x()), f64::from(r.x() + r.width()));
            (x >= left && x < right).then(|| (w.clone(), *index, left))
        })
}

/// `b` の `x` に落としたときの位置（出ているものの中で、真ん中が `x` より左にあるものの数）。
fn slot_at(b: &gtk::Widget, placed: &Placed, side: Side, x: f64) -> usize {
    placed
        .borrow()
        .iter()
        .filter(|(s, ..)| *s == side)
        .filter(|(_, w, _)| {
            w.compute_bounds(b)
                .is_some_and(|r| f64::from(r.x() + r.width() / 2.0) < x)
        })
        .count()
}

/// カスタマイズの間、見出しの項目をドラッグできるようにする。見出しの外（パレットでもなく、落とせる所でもない所）
/// に落としたら外す（macOS 版と同じ）。
fn drag_from(b: &gtk::Box, side: Side, placed: &Placed) {
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    let dragging = Rc::new(Cell::new(None::<usize>));
    let (w, p, d) = (b.downgrade(), placed.clone(), dragging.clone());
    source.connect_prepare(move |source, x, y| {
        if !customizing() {
            return None;
        }
        let b = w.upgrade()?;
        let (widget, index, left) = placed_at(b.upcast_ref(), &p, side, x)?;
        d.set(Some(index));
        let paintable = gtk::WidgetPaintable::new(Some(&widget));
        source.set_icon(Some(&paintable), (x - left) as i32, y as i32);
        Some(gdk::ContentProvider::for_value(
            &Dragged::Move(index).to_value(),
        ))
    });
    source.connect_drag_cancel(move |_, _, reason| {
        let Some(index) = dragging.take() else {
            return false;
        };
        if reason != gdk::DragCancelReason::NoTarget {
            return false;
        }
        save(&removed(&entries(), index));
        true
    });
    b.add_controller(source);
}

/// `w` に落とせるようにする。`side` がなければ見出しの隙間で、`x` が真ん中より左なら左の終わり、右なら右の初め。
fn drop_on(w: &gtk::Widget, side: Option<Side>, placed: &Placed) {
    let target = gtk::DropTarget::new(
        Dragged::static_type(),
        gdk::DragAction::MOVE | gdk::DragAction::COPY,
    );
    let p = placed.clone();
    target.connect_accept(|_, _| customizing());
    target.connect_drop(move |target, value, x, _| {
        let (Ok(dragged), Some(w)) = (value.get::<Dragged>(), target.widget()) else {
            return false;
        };
        let (side, slot) = match side {
            Some(side) => (side, slot_at(&w, &p, side, x)),
            None if x < f64::from(w.width()) / 2.0 => (Side::Start, usize::MAX),
            None => (Side::End, 0),
        };
        save(&dropped(&entries(), &dragged, side, slot));
        true
    });
    w.add_controller(target);
}

/// カスタマイズの見た目（一度だけ入れる）。
fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".toolbar-customizing { min-width: 40px; min-height: 30px; border-radius: 6px; \
           outline: 1px dashed alpha(currentColor, 0.35); outline-offset: 2px; } \
         .toolbar-customizing:drop(active) { background: alpha(@theme_selected_bg_color, 0.25); } \
         .toolbar-space { min-width: 20px; border-radius: 4px; border: 1px dashed alpha(currentColor, 0.45); } \
         .toolbar-palette-tile { padding: 6px; border-radius: 6px; } \
         .toolbar-palette-tile:hover { background: alpha(currentColor, 0.06); } \
         .toolbar-palette { padding: 14px 18px; background: @window_bg_color; \
           border-bottom: 1px solid alpha(currentColor, 0.15); \
           box-shadow: 0 2px 6px rgba(0, 0, 0, 0.12); } \
         .toolbar-default-set { padding: 8px; border-radius: 6px; border: 1px solid alpha(currentColor, 0.15); }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// パレットに出す項目の形（押せない、見出しのボタンと同じ見た目）。
fn preview(id: &str) -> gtk::Widget {
    if id == SPACE || id == SPLIT {
        let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        space.add_css_class("toolbar-space");
        space.set_size_request(if id == SPLIT { 64 } else { 20 }, 30);
        space.set_halign(gtk::Align::Center);
        space.set_valign(gtk::Align::Center);
        return space.upcast();
    }
    let kind = match id {
        "sidebar" => Some(icon::Kind::Sidebar),
        "branch" | "new-branch" => Some(icon::Kind::Branch),
        "search" => Some(icon::Kind::Search),
        "fetch" => Some(icon::Kind::Fetch),
        "pull" => Some(icon::Kind::Pull),
        "push" => Some(icon::Kind::Push),
        "merge" => Some(icon::Kind::Merge),
        "rebase" => Some(icon::Kind::Rebase),
        "stash" => Some(icon::Kind::StashPush),
        "pop" => Some(icon::Kind::StashPop),
        "files" => Some(icon::Kind::Folder),
        "activity" => Some(icon::Kind::Activity),
        "refresh" => Some(icon::Kind::Refresh),
        "undo" => Some(icon::Kind::Undo),
        _ => None,
    };
    let button = gtk::Button::new();
    match (kind, id) {
        (Some(kind), _) => {
            let icon = icon::Icon::default();
            icon.set(kind, false);
            if id == "new-branch" {
                let inner = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                inner.append(&icon);
                inner.append(&gtk::Label::new(Some("+")));
                button.set_child(Some(&inner));
            } else {
                button.set_child(Some(&icon));
            }
        }
        (None, "repositories") => set_icon_or_label(&button, "view-grid-symbolic", "Repositories"),
        (None, "terminal") => {
            set_icon_or_label(&button, "utilities-terminal-symbolic", &tr!("Terminal"));
        }
        // リポジトリの名前。
        _ => {
            let label = gtk::Label::new(Some("Repository"));
            label.add_css_class("heading");
            button.set_child(Some(&label));
        }
    }
    button.set_can_target(false);
    button.set_focusable(false);
    button.set_halign(gtk::Align::Center);
    button.set_valign(gtk::Align::Center);
    button.upcast()
}

fn set_icon_or_label(button: &gtk::Button, icon_name: &str, label: &str) {
    if gdk::Display::default().is_some_and(|d| gtk::IconTheme::for_display(&d).has_icon(icon_name))
    {
        button.set_icon_name(icon_name);
    } else {
        button.set_label(label);
    }
}

/// パレットからドラッグできるようにする（`w` の見た目をドラッグの絵に）。
fn drag_out(tile: &gtk::Widget, image: &gtk::Widget, dragged: Dragged) {
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::COPY);
    let image = image.downgrade();
    source.connect_prepare(move |source, _, _| {
        if let Some(image) = image.upgrade() {
            let paintable = gtk::WidgetPaintable::new(Some(&image));
            source.set_icon(Some(&paintable), image.width() / 2, image.height() / 2);
        }
        Some(gdk::ContentProvider::for_value(&dragged.to_value()))
    });
    tile.add_controller(source);
}

/// View > Customize Toolbar…: ツールバーのパレット（macOS 版の NSToolbar のカスタマイズのシート）。窓の見出しの
/// すぐ下に滑り出させ（見出しは隠さず、落とせるまま）、窓の中身はその下に押し下げる。開いている間は、すべての窓の
/// ツールバーをドラッグで並べ替えられる（並びはアプリ全体で 1 つ）。ほかの窓で開いていれば、そちらを閉じてこの窓に
/// 出す（macOS 版でも、シートは開いた窓に付く）。
pub fn customize(parent: &gtk::Window) {
    if let Some(open) = PALETTE_SHOWN.with(|p| p.borrow().upgrade()) {
        if open
            .root()
            .is_some_and(|r| r == *parent.upcast_ref::<gtk::Root>())
        {
            return;
        }
        close_palette(&open);
    }
    let Some(body) = body_of(parent) else {
        return;
    };
    install_css();
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .child(&palette())
        .build();
    body.prepend(&revealer);
    // 窓を閉じたら（Done を押さずに）カスタマイズを終える。
    revealer.connect_unrealize(|r| {
        if PALETTE_SHOWN.with(|p| p.borrow().upgrade()).as_ref() == Some(r) {
            PALETTE_SHOWN.with(|p| p.replace(glib::WeakRef::new()));
            set_customizing(false);
        }
    });
    PALETTE_SHOWN.with(|p| p.replace(revealer.downgrade()));
    set_customizing(true);
    revealer.set_reveal_child(true);
}

/// 窓の中身の縦の箱（パレットをその頭に入れる）。中身が箱でなければ、箱で包む。
fn body_of(window: &gtk::Window) -> Option<gtk::Box> {
    let child = window.child()?;
    if let Some(body) = child
        .downcast_ref::<gtk::Box>()
        .filter(|b| b.orientation() == gtk::Orientation::Vertical)
    {
        return Some(body.clone());
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    window.set_child(None::<&gtk::Widget>);
    child.set_vexpand(true);
    body.append(&child);
    window.set_child(Some(&body));
    Some(body)
}

/// パレットを畳んで外し、カスタマイズを終える。
fn close_palette(revealer: &gtk::Revealer) {
    PALETTE_SHOWN.with(|p| p.replace(glib::WeakRef::new()));
    set_customizing(false);
    revealer.set_reveal_child(false);
    // 畳み終えてから外す。
    revealer.connect_child_revealed_notify(|r| {
        if !r.is_child_revealed()
            && let Some(parent) = r.parent().and_downcast::<gtk::Box>()
        {
            parent.remove(r);
        }
    });
}

/// パレットの中身: 足せる項目・既定のセット・Done。
fn palette() -> gtk::Widget {
    let caption = |text: &str| {
        let l = gtk::Label::new(Some(text));
        l.set_xalign(0.0);
        l.set_wrap(true);
        l
    };

    // 足せる項目（見出しにあるものも出す。ドラッグすれば、その位置に移る）。
    let items = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(4)
        .max_children_per_line(10)
        .row_spacing(6)
        .column_spacing(6)
        .build();
    for id in PALETTE {
        let image = preview(id);
        let label = gtk::Label::new(Some(&name(id)));
        label.add_css_class("caption");
        label.set_wrap(true);
        label.set_justify(gtk::Justification::Center);
        label.set_max_width_chars(12);
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
        tile.add_css_class("toolbar-palette-tile");
        tile.set_size_request(-1, 64);
        tile.append(&image);
        tile.append(&label);
        drag_out(tile.upcast_ref(), &image, Dragged::Add((*id).to_owned()));
        items.append(&tile);
    }

    // 既定のセット（ひとまとまりでドラッグする）。
    let set = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    set.add_css_class("toolbar-default-set");
    for entry in default_entries() {
        let id = match &entry {
            Entry::Item { id, shown: true } => id.as_str(),
            Entry::Space => SPACE,
            Entry::Split => SPLIT,
            Entry::Item { shown: false, .. } => continue,
        };
        set.append(&preview(id));
    }
    let set_scroll = gtk::ScrolledWindow::builder()
        .child(&set)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .build();
    drag_out(set_scroll.upcast_ref(), set.upcast_ref(), Dragged::Default);

    let note = caption(&tr!(
        "The Repository menu and the main menu stay where they are."
    ));
    note.add_css_class("dim-label");
    note.add_css_class("caption");
    note.set_hexpand(true);
    let done = gtk::Button::with_label(&tr!("Done"));
    done.add_css_class("suggested-action");
    done.set_halign(gtk::Align::End);
    done.connect_clicked(|b| {
        if let Some(r) = b.ancestor(gtk::Revealer::static_type()).and_downcast() {
            close_palette(&r);
        }
    });
    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    bottom.append(&note);
    bottom.append(&done);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    column.add_css_class("toolbar-palette");
    column.append(&caption(&tr!("Drag your favorite items into the toolbar…")));
    column.append(&items);
    column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    column.append(&caption(&tr!("…or drag the default set into the toolbar.")));
    column.append(&set_scroll);
    column.append(&bottom);
    // 見出しの項目をパレットに落としたら外す（macOS 版と同じ）。
    let target = gtk::DropTarget::new(Dragged::static_type(), gdk::DragAction::MOVE);
    target.connect_drop(|_, value, _, _| {
        if let Ok(Dragged::Move(index)) = value.get::<Dragged>() {
            save(&removed(&entries(), index));
        }
        true
    });
    column.add_controller(target);
    // Esc でも閉じる（パレットにフォーカスがあるとき）。
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(|k, key, _, _| {
        if key != gdk::Key::Escape {
            return glib::Propagation::Proceed;
        }
        if let Some(r) = k
            .widget()
            .and_then(|w| w.ancestor(gtk::Revealer::static_type()))
            .and_downcast()
        {
            close_palette(&r);
        }
        glib::Propagation::Stop
    });
    column.add_controller(keys);
    column.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_round_trips() {
        let d = default_entries();
        assert_eq!(parse(&format(&d)), d);
        assert_eq!(d.iter().filter(|e| **e == Entry::Split).count(), 1);
    }

    #[test]
    fn unknown_and_missing_items_are_fixed() {
        let e = parse("push,nothing,push,space");
        assert_eq!(
            e[0],
            Entry::Item {
                id: "push".into(),
                shown: true
            }
        );
        assert_eq!(e[1], Entry::Space);
        assert_eq!(e[2], Entry::Split);
        // 載っていないボタンは、出さないものとして後ろに。
        assert!(
            e[3..]
                .iter()
                .all(|x| matches!(x, Entry::Item { shown: false, .. }))
        );
        assert_eq!(e.len(), 3 + (DEFAULT.len() - 3 - 1) + OPTIONAL.len());
    }

    #[test]
    fn the_default_starts_like_macos() {
        let ids: Vec<String> = default_entries()
            .into_iter()
            .take(4)
            .filter_map(|e| match e {
                Entry::Item { id, shown: true } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["sidebar", "repositories", "repository", "branch"]);
    }

    /// 見出しに出ている並び（左と右）。
    fn shown(all: &[Entry]) -> (Vec<String>, Vec<String>) {
        let ids = |side| {
            shown_on(all, side)
                .into_iter()
                .map(|i| match &all[i] {
                    Entry::Item { id, .. } => id.clone(),
                    _ => SPACE.to_owned(),
                })
                .collect::<Vec<_>>()
        };
        (ids(Side::Start), ids(Side::End))
    }

    #[test]
    fn dropping_from_the_palette_adds_or_moves() {
        let all = parse("fetch,pull,|,undo");
        // 足す（左の 1 番目）。
        let (l, r) = shown(&dropped(
            &all,
            &Dragged::Add("terminal".into()),
            Side::Start,
            1,
        ));
        assert_eq!(l, ["fetch", "terminal", "pull"]);
        assert_eq!(r, ["undo"]);
        // 見出しにあるものは、そこから移る。
        let (l, r) = shown(&dropped(&all, &Dragged::Add("fetch".into()), Side::End, 1));
        assert_eq!(l, ["pull"]);
        assert_eq!(r, ["undo", "fetch"]);
        // スペースはいくつでも。
        let (l, _) = shown(&dropped(&all, &Dragged::Add(SPACE.into()), Side::Start, 0));
        assert_eq!(l, [SPACE, "fetch", "pull"]);
        // 知らないものは入れない。
        assert_eq!(
            dropped(&all, &Dragged::Add("x".into()), Side::Start, 0),
            all
        );
        // 既定のセット。
        assert_eq!(
            dropped(&all, &Dragged::Default, Side::End, 0),
            default_entries()
        );
    }

    #[test]
    fn moving_within_the_toolbar_counts_the_slot_without_the_item() {
        let all = parse("fetch,pull,push,|,undo");
        // fetch を pull と push の間（出ているものの 2 番目の前）へ。
        let (l, _) = shown(&dropped(&all, &Dragged::Move(0), Side::Start, 2));
        assert_eq!(l, ["pull", "fetch", "push"]);
        // 右の初めへ。
        let (l, r) = shown(&dropped(&all, &Dragged::Move(2), Side::End, 0));
        assert_eq!(l, ["fetch", "pull"]);
        assert_eq!(r, ["push", "undo"]);
        // 終わりより先は終わりに。空の側にも入る。
        let empty = parse("fetch,|");
        let (l, r) = shown(&dropped(&empty, &Dragged::Move(0), Side::End, usize::MAX));
        assert!(l.is_empty());
        assert_eq!(r, ["fetch"]);
    }

    #[test]
    fn the_flexible_space_moves_the_split() {
        let all = parse("fetch,pull,|,undo");
        let (l, r) = shown(&dropped(&all, &Dragged::Add(SPLIT.into()), Side::Start, 1));
        assert_eq!(l, ["fetch"]);
        assert_eq!(r, ["pull", "undo"]);
        let (l, r) = shown(&dropped(&all, &Dragged::Add(SPLIT.into()), Side::End, 1));
        assert_eq!(l, ["fetch", "pull", "undo"]);
        assert!(r.is_empty());
        let moved = dropped(&all, &Dragged::Add(SPLIT.into()), Side::Start, 0);
        assert_eq!(moved.iter().filter(|e| **e == Entry::Split).count(), 1);
    }

    #[test]
    fn dragging_out_removes() {
        let all = parse("fetch,space,pull,|");
        let (l, _) = shown(&removed(&all, 1));
        assert_eq!(l, ["fetch", "pull"]);
        let after = removed(&all, 0);
        assert_eq!(shown(&after).0, [SPACE, "pull"]);
        // 外したボタンは、出さないものとして残る（パレットからまた足せる）。
        assert!(after.contains(&Entry::Item {
            id: "fetch".into(),
            shown: false
        }));
        // 境目は外せない。
        assert_eq!(removed(&all, 3), all);
    }

    #[test]
    fn the_old_order_gets_search_at_the_right_end() {
        let (l, r) = shown(&from_v2("fetch,|,refresh,-undo"));
        assert_eq!(l, ["fetch"]);
        assert_eq!(r, ["refresh", "search"]);
        let (_, r) = shown(&default_entries());
        assert_eq!(r, ["activity", "refresh", "undo", "search"]);
    }
}
