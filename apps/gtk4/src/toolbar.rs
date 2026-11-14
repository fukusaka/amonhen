//! 見出しのツールバーのボタンの並び（macOS 版の `MainToolbar` と、NSToolbar のカスタマイズ）。View > Customize
//! Toolbar… で見出しの下にパレットを出し、出している間は見出しのボタンを押す代わりにドラッグで並べ替える: パレットの項目を
//! 見出しにドラッグして足し、見出しの項目を外にドラッグして外し、既定のセットをドラッグして元に戻す（macOS 版と同じ）。
//! Repository のメニューと主メニュー（≡）の間に 1 列に並べる。スペースと伸縮するスペース（macOS 版の flexibleSpace。
//! 余った幅を等分する）をいくつでも、どこにでも入れられる。並びはアプリ全体で 1 つ（設定の `toolbarItemsV3`）で、変えると
//! すべての窓のタブに効く。
//!
//! Repository のメニューと主メニュー（≡）は、macOS 版のメニューバーの代わりなので動かさない。リポジトリの名前は
//! 並びの 1 つで、並びから外すと最初の伸縮するスペースの所に出す。
//!
//! GtkHeaderBar の真ん中の部品は窓の中心に揃うので、左右の幅が違うと片側が空く。そこで見出しの左右は空にして、窓のボタンも
//! 含めて真ん中の 1 列（[`Bar`]）に入れる。

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
/// 前の版の、左右の境目の伸縮するスペース（保存した並びを読むときだけ）。
const SPLIT: &str = "|";
/// 伸縮するスペース（いくつでも入れられる）。
const FLEX: &str = "~";
/// サイドバーの上と中身の上の境目（macOS 版の sidebarTrackingSeparator）。前のものはサイドバーの上の見出しに置く。
/// 1 つだけで、外せない。
const EDGE: &str = "|sidebar";

/// macOS 版の既定の並び（サイドバーの切り替え・Repositories・リポジトリの名前・Branch・Fetch・Pull・Push、
/// Merge・Rebase、Stash・Pop、右に Activity・Refresh・Undo・検索）。
const DEFAULT: &[&str] = &[
    "sidebar",
    "repositories",
    EDGE,
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
    FLEX,
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
    FLEX,
];

/// 並びの 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Item {
        id: String,
        shown: bool,
    },
    Space,
    Flex,
    /// サイドバーの上と中身の上の境目。
    Edge,
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
        FLEX => tr!("Flexible Space"),
        _ => id.to_owned(),
    }
}

fn known(id: &str) -> bool {
    DEFAULT.contains(&id) && id != SPACE && id != FLEX && id != EDGE || OPTIONAL.contains(&id)
}

/// 既定の並び（足せるボタンは、出さないものとして後ろに）。
fn default_entries() -> Vec<Entry> {
    parse(&DEFAULT.join(","))
}

/// 設定の文字列を読む。知らない名前は捨て、載っていないボタンは出さないものとして後ろに足す。
fn parse(text: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    for part in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match part {
            SPACE => entries.push(Entry::Space),
            FLEX | SPLIT => entries.push(Entry::Flex),
            EDGE if !entries.contains(&Entry::Edge) => entries.push(Entry::Edge),
            EDGE => {}
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
    // 境目のない並び（前の版）は、先頭のサイドバーの切り替えと Repositories の後に境目を置く（macOS 版の既定と同じ）。
    if !entries.contains(&Entry::Edge) {
        let at = entries
            .iter()
            .take_while(
                |e| matches!(e, Entry::Item { id, .. } if id == "sidebar" || id == "repositories"),
            )
            .count();
        entries.insert(at, Entry::Edge);
    }
    for id in DEFAULT
        .iter()
        .chain(OPTIONAL)
        .filter(|id| **id != SPACE && **id != FLEX && **id != EDGE)
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
            Entry::Flex => FLEX.to_owned(),
            Entry::Edge => EDGE.to_owned(),
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

/// 前の版の並びを読む（並べたものはそのままに、検索を終わりに足す。前の版では検索はそこに決まっていた）。
fn from_v2(text: &str) -> Vec<Entry> {
    let mut all = parse(text);
    all.retain(|e| !matches!(e, Entry::Item { id, .. } if id == "search"));
    insert_at(
        &mut all,
        usize::MAX,
        Entry::Item {
            id: "search".to_owned(),
            shown: true,
        },
    );
    all
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

/// 見出しに出ているもの（出すボタンとスペース、境目）の、並びの中の位置。
fn shown_on(all: &[Entry]) -> Vec<usize> {
    all.iter()
        .enumerate()
        .filter(|(_, e)| {
            matches!(
                e,
                Entry::Space | Entry::Flex | Entry::Edge | Entry::Item { shown: true, .. }
            )
        })
        .map(|(i, _)| i)
        .collect()
}

/// `slot` 番目（出ているものの中の位置。多すぎれば終わり）に `entry` を入れる。
fn insert_at(all: &mut Vec<Entry>, slot: usize, entry: Entry) {
    let shown = shown_on(all);
    let at = match (shown.get(slot), shown.last()) {
        (Some(&i), _) => i,
        (None, Some(&last)) => last + 1,
        (None, None) => 0,
    };
    all.insert(at, entry);
}

/// `dragged` を `slot` 番目に落としたあとの並び。
fn dropped(all: &[Entry], dragged: &Dragged, slot: usize) -> Vec<Entry> {
    let mut all = all.to_vec();
    // 前から動かすなら、取り除いた分だけ詰める。
    let take = |all: &mut Vec<Entry>, index: usize, slot: usize| -> usize {
        let before = shown_on(all).iter().take(slot).any(|&i| i == index);
        all.remove(index);
        if before { slot - 1 } else { slot }
    };
    match dragged {
        Dragged::Default => return default_entries(),
        Dragged::Move(index) => {
            let Some(entry) = all.get(*index).cloned() else {
                return all;
            };
            let slot = take(&mut all, *index, slot);
            insert_at(&mut all, slot, entry);
        }
        Dragged::Add(id) if id == SPACE => insert_at(&mut all, slot, Entry::Space),
        Dragged::Add(id) if id == FLEX => insert_at(&mut all, slot, Entry::Flex),
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
        Some(Entry::Space | Entry::Flex) => {
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
    /// リポジトリの名前（押すとリポジトリの一覧）。
    pub repository: gtk::Widget,
    /// 最初の伸縮するスペースの所にいつも出すもの（リポジトリのページの「Repositories」と説明。macOS 版の窓の名前）。
    /// なければ、リポジトリの名前を並びから外したときだけ出す。
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

/// 見出しの列: サイドバーの上の見出しに、動かさないメニュー（`lead`）と境目より前の項目（`side`）。中身の上の
/// 見出しに、境目より後の項目（`items`）と動かさないメニュー（`trail`）。
pub struct Bar {
    pub lead: gtk::Box,
    pub side: gtk::Box,
    pub items: gtk::Box,
    pub trail: gtk::Box,
}

impl Bar {
    /// サイドバーの上の見出し（`side_header`）と中身の上の見出し（`header`）の真ん中を、窓のボタンも含めたそれぞれ
    /// 1 列にする（ドラッグと右クリックは見出しのまま）。GNOME の分けた見出しの形（サイドバーが窓の上端まで伸びる）。
    pub fn new(side_header: &gtk::HeaderBar, header: &gtk::HeaderBar) -> Self {
        let horizontal = || gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let (side_row, lead, side) = (horizontal(), horizontal(), horizontal());
        let (row, items, trail) = (horizontal(), horizontal(), horizontal());
        side_row.set_hexpand(true);
        // 項目のない所にも落とせるように、左の列も見出しの残りの幅いっぱいにする（右の列と同じ）。
        side.set_hexpand(true);
        row.set_hexpand(true);
        items.set_hexpand(true);
        let controls = |side| {
            let controls = gtk::WindowControls::new(side);
            controls
                .bind_property("empty", &controls, "visible")
                .invert_boolean()
                .sync_create()
                .build();
            controls
        };
        side_row.append(&controls(gtk::PackType::Start));
        side_row.append(&lead);
        side_row.append(&side);
        row.append(&items);
        row.append(&trail);
        row.append(&controls(gtk::PackType::End));
        side_header.set_show_title_buttons(false);
        side_header.set_title_widget(Some(&side_row));
        header.set_show_title_buttons(false);
        header.set_title_widget(Some(&row));
        Self {
            lead,
            side,
            items,
            trail,
        }
    }
}

/// ボタンを作り、設定の並びで入れる（境目より前は `side`、後は `row`）。並びが変わったら入れ直す。
pub fn install(side: &gtk::Box, row: &gtk::Box, given: Given) {
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

    // 見出しに入れた部品と、その並びの中の位置（カスタマイズの間、どれをドラッグしたかを見分ける）。
    let placed: Placed = Rc::default();
    let flexible = || {
        let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        space.set_hexpand(true);
        space.set_width_request(8);
        space
    };
    let fill = {
        let (side, row) = (side.downgrade(), row.downgrade());
        let repository = given.repository.clone();
        let placed = placed.clone();
        let title = given.title.clone();
        move || {
            let (Some(side), Some(row)) = (side.upgrade(), row.upgrade()) else {
                return;
            };
            let customizing = customizing();
            for row in [&side, &row] {
                while let Some(child) = row.first_child() {
                    child.set_can_target(true);
                    row.remove(&child);
                }
                // カスタマイズの間は、枠を出す。
                if customizing {
                    row.add_css_class("toolbar-customizing");
                } else {
                    row.remove_css_class("toolbar-customizing");
                }
            }
            let all = entries();
            let shown = all
                .iter()
                .any(|e| matches!(e, Entry::Item { id, shown: true } if id == "repository"));
            // 並びにない、いつも出すもの（最初の伸縮するスペースの所に。両側にスペースを置いて、間に出す）。
            let mut middle = title
                .clone()
                .or_else(|| (!shown).then(|| repository.clone()));
            let mut now = Vec::new();
            // 境目までは、サイドバーの上の見出しに入れる。
            let mut target = side.clone();
            for (index, entry) in all.iter().enumerate() {
                let widget: gtk::Widget = match entry {
                    Entry::Edge => {
                        target = row.clone();
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
                    Entry::Flex => {
                        let space = flexible();
                        if customizing {
                            space.set_width_request(64);
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
                // カスタマイズの間は押せない（見出しの列がドラッグを受ける）。
                widget.set_can_target(!customizing);
                target.append(&widget);
                now.push((widget, index));
                if *entry == Entry::Flex
                    && target == row
                    && let Some(middle) = middle.take()
                {
                    row.append(&middle);
                    row.append(&flexible());
                }
            }
            if let Some(middle) = middle {
                row.append(&flexible());
                row.append(&middle);
                row.append(&flexible());
            }
            placed.replace(now);
        }
    };
    fill();
    let fill = Rc::new(fill);
    // 2 つの列は左右に並ぶので、位置は列をまたいで数える（中身の上の列は、境目の分 1 つ後ろ）。
    for (b, offset) in [(side, 0), (row, 1)] {
        drag_from(b, &placed);
        drop_on(b.upcast_ref(), &placed, offset);
    }
    // タブの見出しがある間だけ見る（閉じたタブのボタンを持ち続けない）。
    let f = fill.clone();
    crate::settings::connect_changed_while(row, move |key| {
        if key == KEY {
            f();
        }
    });
    let row = row.downgrade();
    TOOLBARS.with(|t| t.borrow_mut().push((row, Rc::new(move || fill()))));
}

/// ツールバーを入れ直すもの。
type Refill = Rc<dyn Fn()>;

/// 見出しに入れた部品と、その並びの中の位置。
type Placed = Rc<RefCell<Vec<(gtk::Widget, usize)>>>;

thread_local! {
    /// 窓に出しているパレット（開いている間はカスタマイズ中）。
    static PALETTE_SHOWN: RefCell<glib::WeakRef<gtk::Revealer>> = RefCell::default();
    static CUSTOMIZING: Cell<bool> = const { Cell::new(false) };
    /// 作ったツールバー（見出しの列がなくなったら外す）と、入れ直すもの。
    static TOOLBARS: RefCell<Vec<(glib::WeakRef<gtk::Box>, Refill)>> = RefCell::default();
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
fn placed_at(b: &gtk::Widget, placed: &Placed, x: f64) -> Option<(gtk::Widget, usize, f64)> {
    placed.borrow().iter().find_map(|(w, index)| {
        let r = w.compute_bounds(b)?;
        let (left, right) = (f64::from(r.x()), f64::from(r.x() + r.width()));
        (x >= left && x < right).then(|| (w.clone(), *index, left))
    })
}

/// `b` の `x` に落としたときの位置（出ているものの中で、真ん中が `x` より左にあるものの数）。
fn slot_at(b: &gtk::Widget, placed: &Placed, x: f64) -> usize {
    placed
        .borrow()
        .iter()
        .filter(|(w, _)| {
            w.compute_bounds(b)
                .is_some_and(|r| f64::from(r.x() + r.width() / 2.0) < x)
        })
        .count()
}

/// カスタマイズの間、見出しの項目をドラッグできるようにする。見出しの外（パレットでもなく、落とせる所でもない所）
/// に落としたら外す（macOS 版と同じ）。
fn drag_from(b: &gtk::Box, placed: &Placed) {
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    let dragging = Rc::new(Cell::new(None::<usize>));
    let (w, p, d) = (b.downgrade(), placed.clone(), dragging.clone());
    source.connect_prepare(move |source, x, y| {
        if !customizing() {
            return None;
        }
        let b = w.upgrade()?;
        let (widget, index, left) = placed_at(b.upcast_ref(), &p, x)?;
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

/// `w` に落とせるようにする。`offset` は落とした位置に足す数（境目より後の列は 1）。
fn drop_on(w: &gtk::Widget, placed: &Placed, offset: usize) {
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
        save(&dropped(&entries(), &dragged, slot_at(&w, &p, x) + offset));
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
    if id == SPACE || id == FLEX {
        let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        space.add_css_class("toolbar-space");
        space.set_size_request(if id == FLEX { 64 } else { 20 }, 30);
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
/// すぐ下に、中身に重ねて滑り出させる（見出しは隠さず、落とせるまま）。macOS 版のシートと同じく窓の大きさも中身の
/// 並びも変えず、閉じれば元のとおりに戻る。開いている間は、すべての窓のツールバーをドラッグで並べ替えられる（並びは
/// アプリ全体で 1 つ）。ほかの窓で開いていれば、そちらを閉じてこの窓に出す（macOS 版でも、シートは開いた窓に付く）。
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
    let Some(overlay) = overlay_of(parent) else {
        return;
    };
    install_css();
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .valign(gtk::Align::Start)
        .child(&palette())
        .build();
    overlay.add_overlay(&revealer);
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

/// 窓の中身に重ねる層（パレットをそこに載せる）。中身を一度だけ `gtk::Overlay` で包む。重ねた子は大きさの
/// 見積もりに入らないので、パレットを出しても窓の大きさは変わらない。
fn overlay_of(window: &gtk::Window) -> Option<gtk::Overlay> {
    let child = window.child()?;
    if let Some(overlay) = child
        .downcast_ref::<gtk::Overlay>()
        .filter(|o| o.widget_name() == OVERLAY_NAME)
    {
        return Some(overlay.clone());
    }
    let overlay = gtk::Overlay::new();
    overlay.set_widget_name(OVERLAY_NAME);
    window.set_child(None::<&gtk::Widget>);
    overlay.set_child(Some(&child));
    window.set_child(Some(&overlay));
    Some(overlay)
}

const OVERLAY_NAME: &str = "toolbar-palette-layer";

/// パレットを畳んで外し、カスタマイズを終える。
fn close_palette(revealer: &gtk::Revealer) {
    PALETTE_SHOWN.with(|p| p.replace(glib::WeakRef::new()));
    set_customizing(false);
    revealer.set_reveal_child(false);
    // 畳み終えてから外す。
    revealer.connect_child_revealed_notify(|r| {
        if !r.is_child_revealed()
            && let Some(parent) = r.parent().and_downcast::<gtk::Overlay>()
        {
            parent.remove_overlay(r);
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
            Entry::Flex => FLEX,
            Entry::Item { shown: false, .. } | Entry::Edge => continue,
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
        assert_eq!(d.iter().filter(|e| **e == Entry::Flex).count(), 1);
    }

    #[test]
    fn an_order_saved_with_the_old_boundary_keeps_its_place() {
        let e = parse("fetch,|,~,undo");
        assert_eq!(shown(&e), [EDGE, "fetch", FLEX, FLEX, "undo"]);
        assert_eq!(
            shown(&parse(&format(&e))),
            [EDGE, "fetch", FLEX, FLEX, "undo"]
        );
    }

    #[test]
    fn an_order_without_the_sidebar_edge_gets_it_after_the_sidebar_buttons() {
        // 境目を足す前の版の既定の並び: サイドバーの切り替えと Repositories の後に入る（macOS 版の既定と同じ）。
        let e = parse("sidebar,repositories,repository,branch,~,undo");
        assert_eq!(
            shown(&e),
            [
                "sidebar",
                "repositories",
                EDGE,
                "repository",
                "branch",
                FLEX,
                "undo"
            ]
        );
        // 境目は 1 つだけ。
        let twice = parse("fetch,|sidebar,pull,|sidebar");
        assert_eq!(twice.iter().filter(|x| **x == Entry::Edge).count(), 1);
        assert_eq!(shown(&twice), ["fetch", EDGE, "pull"]);
    }

    #[test]
    fn unknown_and_missing_items_are_fixed() {
        let e = parse("push,nothing,push,space");
        assert_eq!(e[0], Entry::Edge);
        assert_eq!(
            e[1],
            Entry::Item {
                id: "push".into(),
                shown: true
            }
        );
        assert_eq!(e[2], Entry::Space);
        // 載っていないボタンは、出さないものとして後ろに。
        assert!(
            e[3..]
                .iter()
                .all(|x| matches!(x, Entry::Item { shown: false, .. }))
        );
        // 既定の並びのうち、スペース 2 つ・伸縮するスペース・境目・push の分を除いたもの。
        assert_eq!(e.len(), 3 + (DEFAULT.len() - 4 - 1) + OPTIONAL.len());
    }

    #[test]
    fn the_default_starts_like_macos() {
        let all = default_entries();
        assert_eq!(
            &shown(&all)[..5],
            ["sidebar", "repositories", EDGE, "repository", "branch"]
        );
    }

    /// 見出しに出ている並び。
    fn shown(all: &[Entry]) -> Vec<String> {
        shown_on(all)
            .into_iter()
            .map(|i| match &all[i] {
                Entry::Item { id, .. } => id.clone(),
                Entry::Flex => FLEX.to_owned(),
                Entry::Edge => EDGE.to_owned(),
                Entry::Space => SPACE.to_owned(),
            })
            .collect()
    }

    #[test]
    fn dropping_from_the_palette_adds_or_moves() {
        let all = parse("fetch,pull,~,undo");
        // 足す（境目の後の 2 番目）。
        let after = dropped(&all, &Dragged::Add("terminal".into()), 2);
        assert_eq!(
            shown(&after),
            [EDGE, "fetch", "terminal", "pull", FLEX, "undo"]
        );
        // 見出しにあるものは、そこから移る。
        let after = dropped(&all, &Dragged::Add("fetch".into()), 5);
        assert_eq!(shown(&after), [EDGE, "pull", FLEX, "undo", "fetch"]);
        // スペースはいくつでも。
        let after = dropped(&all, &Dragged::Add(SPACE.into()), 1);
        assert_eq!(shown(&after), [EDGE, SPACE, "fetch", "pull", FLEX, "undo"]);
        // 境目より前（サイドバーの上）にも入れられる。
        let after = dropped(&all, &Dragged::Add("terminal".into()), 0);
        assert_eq!(
            shown(&after),
            ["terminal", EDGE, "fetch", "pull", FLEX, "undo"]
        );
        // 知らないものは入れない。
        assert_eq!(dropped(&all, &Dragged::Add("x".into()), 0), all);
        // 既定のセット。
        assert_eq!(dropped(&all, &Dragged::Default, 0), default_entries());
    }

    #[test]
    fn moving_within_the_toolbar_counts_the_slot_without_the_item() {
        let all = parse("fetch,pull,push,~,undo");
        let after = dropped(&all, &Dragged::Move(1), 3);
        assert_eq!(shown(&after), [EDGE, "pull", "fetch", "push", FLEX, "undo"]);
        // 終わりより先は終わりに。
        let after = dropped(&all, &Dragged::Move(1), usize::MAX);
        assert_eq!(shown(&after), [EDGE, "pull", "push", FLEX, "undo", "fetch"]);
        // 境目をまたいで、サイドバーの上へ。
        let after = dropped(&all, &Dragged::Move(3), 0);
        assert_eq!(shown(&after), ["push", EDGE, "fetch", "pull", FLEX, "undo"]);
    }

    #[test]
    fn flexible_spaces_go_anywhere_and_can_all_be_removed() {
        let all = parse("fetch,pull,~,undo");
        let flexes = |e: &[Entry]| e.iter().filter(|e| **e == Entry::Flex).count();
        let added = dropped(&all, &Dragged::Add(FLEX.into()), 0);
        assert_eq!(added[0], Entry::Flex);
        let added = dropped(&added, &Dragged::Add(FLEX.into()), usize::MAX);
        assert_eq!(flexes(&added), 3);
        assert_eq!(parse(&format(&added)), added);
        // どれでも動かせて、どれでも外せる。
        let moved = dropped(&added, &Dragged::Move(4), 1);
        assert_eq!(
            shown(&moved),
            [FLEX, FLEX, EDGE, "fetch", "pull", "undo", FLEX]
        );
        let none = removed(&removed(&removed(&added, 6), 4), 0);
        assert_eq!(flexes(&none), 0);
    }

    #[test]
    fn dragging_out_removes() {
        let all = parse("fetch,space,pull,~");
        assert_eq!(shown(&removed(&all, 2)), [EDGE, "fetch", "pull", FLEX]);
        let after = removed(&all, 1);
        assert_eq!(shown(&after), [EDGE, SPACE, "pull", FLEX]);
        // 外したボタンは、出さないものとして残る（パレットからまた足せる）。
        assert!(after.contains(&Entry::Item {
            id: "fetch".into(),
            shown: false
        }));
    }

    #[test]
    fn the_old_order_gets_search_at_the_end() {
        assert_eq!(
            shown(&from_v2("fetch,|,refresh,-undo")),
            [EDGE, "fetch", FLEX, "refresh", "search"]
        );
        assert_eq!(
            shown(&default_entries())[14..],
            [FLEX, "activity", "refresh", "undo", "search"]
        );
    }
}
