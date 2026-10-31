//! 見出しのツールバーのボタンの並び（macOS 版の `MainToolbar` のカスタマイズ）。どのボタンを出すかと順番を、
//! View > Customize Toolbar… の窓で決める（GNOME の見出しには並べ替えの仕組みがないので、一覧で上下に動かす）。
//! 見出しの左（Repository のメニューの右）と右（主メニューの左）に分けて並べ、間に空きを入れられる。
//! 並びはアプリ全体で 1 つ（設定の `toolbarItems`）で、変えるとすべての窓のタブに効く。
//!
//! Repository のメニュー・リポジトリの名前・検索・主メニュー（≡）は動かさない。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;

use crate::icon;

/// 設定の鍵。並びを `,` でつなぐ（出さないものは `-` を前に付ける）。
const KEY: &str = "toolbarItems";
/// 空き（いくつでも入れられる）。
const SPACE: &str = "space";
/// ここから右は見出しの右に並べる。
const SPLIT: &str = "|";

/// macOS 版の既定の並び（Branch・Fetch・Pull・Push、Merge・Rebase、Stash・Pop、右に Activity・Refresh・Undo）。
const DEFAULT: &[&str] = &[
    "branch", "fetch", "pull", "push", SPACE, "merge", "rebase", SPACE, "stash", "pop", SPLIT,
    "activity", "refresh", "undo",
];
/// 既定では出さないボタン（macOS 版のツールバーに足せるもの）。
const OPTIONAL: &[&str] = &["new-branch", "repositories", "terminal", "files"];

/// 並びの 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Item { id: String, shown: bool },
    Space,
    Split,
}

/// ボタンの名前（一覧に出す。操作の名前は英語のまま）。
fn name(id: &str) -> String {
    match id {
        "branch" => "Branch".to_owned(),
        "fetch" => "Fetch".to_owned(),
        "pull" => "Pull".to_owned(),
        "push" => "Push".to_owned(),
        "merge" => "Merge".to_owned(),
        "rebase" => "Rebase".to_owned(),
        "stash" => "Stash".to_owned(),
        "pop" => "Pop".to_owned(),
        "new-branch" => "New Branch".to_owned(),
        "repositories" => "Repositories".to_owned(),
        "terminal" => tr!("Open in Terminal"),
        "files" => tr!("Show in Files"),
        "activity" => "Activity".to_owned(),
        "refresh" => tr!("Refresh"),
        "undo" => "Undo".to_owned(),
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

fn entries() -> Vec<Entry> {
    crate::settings::string(KEY).map_or_else(default_entries, |s| parse(&s))
}

fn save(entries: &[Entry]) {
    crate::settings::set_string(KEY, &format(entries));
}

/// タブごとに作ってあるボタン（ほかの所でも中身を変えるもの）。
pub struct Given {
    pub branch: gtk::Button,
    pub pull: gtk::Button,
    pub push: gtk::Button,
    pub undo: gtk::Button,
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
    buttons.insert(
        "refresh",
        icon_button(
            icon::Kind::Refresh,
            "win.refresh",
            &tr!("Reload History, branches and Working Copy (Ctrl+R)"),
        )
        .upcast(),
    );
    buttons.insert("undo", given.undo.upcast());

    let fill = {
        let (start, end) = (start.downgrade(), end.downgrade());
        move || {
            let (Some(start), Some(end)) = (start.upgrade(), end.upgrade()) else {
                return;
            };
            for b in [&start, &end] {
                while let Some(child) = b.first_child() {
                    b.remove(&child);
                }
            }
            let mut target = &start;
            for entry in entries() {
                match entry {
                    Entry::Split => target = &end,
                    Entry::Space => {
                        let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        space.set_width_request(8);
                        target.append(&space);
                    }
                    Entry::Item { id, shown: true } => {
                        if let Some(w) = buttons.get(id.as_str()) {
                            target.append(w);
                        }
                    }
                    Entry::Item { shown: false, .. } => {}
                }
            }
        }
    };
    fill();
    crate::settings::connect_changed(move |key| {
        if key == KEY {
            fill();
        }
    });
}

thread_local! {
    static WINDOW: RefCell<glib::WeakRef<gtk::Window>> = RefCell::default();
}

/// View > Customize Toolbar…: どのボタンを出すかと順番を決める窓（開いていれば前に出す）。
pub fn customize(parent: &gtk::Window) {
    if let Some(w) = WINDOW.with(|w| w.borrow().upgrade()) {
        w.present();
        return;
    }
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    refill(&list);
    let refill = {
        let list = list.downgrade();
        Rc::new(move || {
            if let Some(list) = list.upgrade() {
                refill(&list);
            }
        })
    };

    let add_space = gtk::Button::with_label(&tr!("Add Space"));
    add_space.connect_clicked({
        let refill = refill.clone();
        move |_| {
            let mut all = entries();
            // 左の終わり（右との境目の前）に足す。
            let at = all
                .iter()
                .position(|e| *e == Entry::Split)
                .unwrap_or(all.len());
            all.insert(at, Entry::Space);
            save(&all);
            refill();
        }
    });
    let restore = gtk::Button::with_label(&tr!("Restore Default"));
    restore.connect_clicked({
        let refill = refill.clone();
        move |_| {
            save(&default_entries());
            refill();
        }
    });
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.append(&add_space);
    let gap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    gap.set_hexpand(true);
    buttons.append(&gap);
    buttons.append(&restore);

    let note = gtk::Label::new(Some(&tr!(
        "The Repository menu, search and the main menu stay where they are."
    )));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    note.add_css_class("caption");

    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    column.set_margin_top(18);
    column.set_margin_bottom(18);
    column.set_margin_start(18);
    column.set_margin_end(18);
    column.append(&list);
    column.append(&note);
    column.append(&buttons);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&column)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(720)
        .build();
    let window = gtk::Window::builder()
        .title(tr!("Customize Toolbar"))
        .default_width(440)
        .child(&scrolled)
        .transient_for(parent)
        .destroy_with_parent(true)
        .build();
    window.set_titlebar(Some(&gtk::HeaderBar::new()));
    WINDOW.with(|w| w.replace(window.downgrade()));
    window.present();
}

/// 一覧を今の並びで作り直す。
fn refill(list: &gtk::ListBox) {
    while let Some(row) = list.first_child() {
        list.remove(&row);
    }
    let all = entries();
    list.append(&heading_row(&tr!("Left of the header bar")));
    for (i, entry) in all.iter().enumerate() {
        list.append(&entry_row(&all, i, entry, list.downgrade()));
    }
}

/// 一覧の見出しの行（見出しの左・右）。
fn heading_row(title: &str) -> gtk::ListBoxRow {
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("heading");
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    label.set_margin_start(12);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&label));
    row.set_activatable(false);
    row
}

/// 並びの 1 行: 出すか（空きは外す）、名前、上下に動かす。
fn entry_row(
    all: &[Entry],
    index: usize,
    entry: &Entry,
    list: glib::WeakRef<gtk::ListBox>,
) -> gtk::ListBoxRow {
    // 変えたら保存し（ツールバーは設定の知らせで並べ直す）、一覧を作り直す（押したボタンの外から）。
    let changed = Rc::new(move |all: Vec<Entry>| {
        save(&all);
        let list = list.clone();
        glib::idle_add_local_once(move || {
            if let Some(list) = list.upgrade() {
                refill(&list);
            }
        });
    });
    if *entry == Entry::Split {
        // 境目は見出しとして出す（ボタンを上下に動かして越えさせる）。
        return heading_row(&tr!("Right of the header bar"));
    }
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.set_margin_top(4);
    b.set_margin_bottom(4);
    b.set_margin_start(12);
    b.set_margin_end(8);
    match entry {
        Entry::Item { id, shown } => {
            let check = gtk::CheckButton::with_label(&name(id));
            check.set_active(*shown);
            check.set_hexpand(true);
            let (all, changed) = (all.to_vec(), changed.clone());
            check.connect_toggled(move |c| {
                let mut all = all.clone();
                if let Entry::Item { shown, .. } = &mut all[index] {
                    *shown = c.is_active();
                }
                changed(all);
            });
            b.append(&check);
        }
        Entry::Space => {
            let label = gtk::Label::new(Some(&tr!("Space")));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_margin_start(28);
            label.add_css_class("dim-label");
            b.append(&label);
            let remove = gtk::Button::with_label("−");
            remove.add_css_class("flat");
            remove.set_tooltip_text(Some(&tr!("Remove")));
            let (all, changed) = (all.to_vec(), changed.clone());
            remove.connect_clicked(move |_| {
                let mut all = all.clone();
                all.remove(index);
                changed(all);
            });
            b.append(&remove);
        }
        Entry::Split => {}
    }
    for (label, tip, step) in [("↑", tr!("Move Up"), -1isize), ("↓", tr!("Move Down"), 1)] {
        let button = gtk::Button::with_label(label);
        button.add_css_class("flat");
        button.set_tooltip_text(Some(&tip));
        let to = index as isize + step;
        let possible = to >= 0 && (to as usize) < all.len();
        button.set_sensitive(possible);
        let (all, changed) = (all.to_vec(), changed.clone());
        button.connect_clicked(move |_| {
            if !possible {
                return;
            }
            let mut all = all.clone();
            all.swap(index, to as usize);
            changed(all);
        });
        b.append(&button);
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&b));
    row.set_activatable(false);
    row
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
}
