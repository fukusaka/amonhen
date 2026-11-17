//! 窓のメニューバー。操作の名前とメニューの区切りは、macOS 版・GTK4 版と共通の操作の表（`amonhen-commands`）から
//! 作る。この FE にまだ機能のない操作は出さない（GTK4 版の `PENDING` と同じ考え）。

use amonhen_commands::{Command, Menu};
use windows_reactor::{
    Callback, CommandBar, CommandBarCommand, GridChildExt, MenuBar, MenuBarItem, MenuBarSlot,
    MenuExt, MenuItem, MenuShortcut, SlotsControl, Symbol, View,
};

/// この FE で動く操作。
pub const IMPLEMENTED: &[&str] = &[
    "show-working-copy",
    "show-history",
    "refresh",
    "rebuild-history",
    "go-to-head",
    "go-to-parent",
    "go-to-child",
    "undo",
    "find-in-history",
    "fetch",
    "pull",
    "push",
    "force-push",
    "new-branch",
    "merge",
    "rebase",
    "continue",
    "skip",
    "abort",
    "settings",
    "activity",
    "repositories",
    "quick-open",
    "add-submodule",
    "update-submodules",
    "add-subtree",
    "repository-settings",
    "stash",
    "pop-stash",
    "delete-ignored-files",
    "new-orphan-branch",
    "switch-branch",
    "compare-with-previous",
    "apply-patch",
    "save-bundle",
    "fetch-bundle",
    "new-worktree",
    "prune-worktrees",
    "open-in-terminal",
    "open-in-editor",
    "reveal-in-files",
];

/// 表にない、この FE が File メニューの先頭に置く項目（GTK4 版の `appmenu` の Open…・Clone…。macOS 版の
/// ⌘O・⌘⌃C）。
const FILE_ITEMS: &[(&str, &str)] = &[
    ("new-window", "Primary+N"),
    ("open", "Primary+O"),
    ("clone", "Primary+Alt+C"),
];

fn file_item_title(id: &str) -> String {
    match id {
        "new-window" => amonhen_model::tr!("New Window"),
        "open" => amonhen_model::tr!("Open…"),
        _ => "Clone…".to_owned(),
    }
}

/// 項目の名前（GTK4 版の `appmenu::title` と同じ）。
fn title(c: &Command) -> String {
    match c.id {
        "reveal-in-files" => amonhen_model::tr!("Show in Files"),
        _ if c.localized => amonhen_model::i18n::translate(c.title).into_owned(),
        _ => c.title.to_owned(),
    }
}

/// メニューバー。項目を選ぶと `on_click` が操作の id を受ける。
pub fn menu_bar(on_click: impl Fn() -> Callback<String>) -> View {
    let menus: [(Menu, &'static str); 7] = [
        (Menu::File, "File"),
        (Menu::Edit, "Edit"),
        (Menu::View, "View"),
        (Menu::Navigate, "Navigate"),
        (Menu::Repository, "Repository"),
        (Menu::Commit, "Commit"),
        (Menu::Window, "Window"),
    ];
    let mut bar: Vec<(String, View)> = Vec::new();
    for (menu, name) in menus {
        let mut items = Vec::new();
        if menu == Menu::File {
            items.extend(FILE_ITEMS.iter().map(|(id, key)| {
                let item = MenuItem::item(*id, file_item_title(id));
                match shortcut(key) {
                    Some(s) => item.with_shortcut(s),
                    None => item,
                }
            }));
        }
        for (i, section) in amonhen_commands::menu(menu).into_iter().enumerate() {
            let shown: Vec<&Command> = section
                .into_iter()
                .filter(|c| IMPLEMENTED.contains(&c.id))
                .collect();
            if shown.is_empty() {
                continue;
            }
            if !items.is_empty() {
                items.push(MenuItem::separator(format!("-{i}")));
            }
            items.extend(shown.into_iter().map(|c| {
                let item = MenuItem::item(c.id, title(c));
                match c.shortcut.and_then(shortcut) {
                    Some(s) => item.with_shortcut(s),
                    None => item,
                }
            }));
        }
        // View の終わりに History の並び順（GTK4 版の View の History の見せ方。選んでいる方に ✓）と
        // Customize Toolbar…（GTK4 版の View のメニュー）。
        if menu == Menu::View {
            if !items.is_empty() {
                items.push(MenuItem::separator("-order"));
            }
            let order = crate::prefs::history_order();
            for (id, value, title) in [
                (
                    "history-order-date",
                    amonhen_model::HistoryOrder::Date,
                    amonhen_model::tr!("Date Order"),
                ),
                (
                    "history-order-topo",
                    amonhen_model::HistoryOrder::Topo,
                    amonhen_model::tr!("Topological Order"),
                ),
            ] {
                let mark = if order == value { "✓ " } else { "" };
                items.push(MenuItem::item(id, format!("{mark}{title}")));
            }
            // 行に出す項目（GTK4 版の View の History の見せ方。CI の状態はこの FE にない）。
            items.push(MenuItem::separator("-items"));
            let shown = crate::render::history_items();
            for (id, bit, title) in [
                (
                    "history-item-refs",
                    crate::render::items::REFS,
                    "Branches and Tags",
                ),
                (
                    "history-item-author",
                    crate::render::items::AUTHOR,
                    "Author",
                ),
                ("history-item-date", crate::render::items::DATE, "Date"),
                ("history-item-id", crate::render::items::ID, "Commit ID"),
                (
                    "history-item-signature",
                    crate::render::items::SIGNATURE,
                    "Signature",
                ),
            ] {
                let mark = if shown & bit != 0 { "✓ " } else { "" };
                let title = amonhen_model::i18n::translate(title);
                items.push(MenuItem::item(id, format!("{mark}{title}")));
            }
            items.push(MenuItem::separator("-one-line"));
            let mark = if crate::render::one_line() {
                "✓ "
            } else {
                ""
            };
            items.push(MenuItem::item(
                "one-line-history",
                format!("{mark}{}", amonhen_model::tr!("One Line per Commit")),
            ));
            items.push(MenuItem::separator("-toolbar"));
            items.push(MenuItem::item(
                "customize-toolbar",
                amonhen_model::tr!("Customize Toolbar…"),
            ));
        }
        if items.is_empty() {
            continue;
        }
        let title = amonhen_model::i18n::translate(name).into_owned();
        let item = MenuBarItem::new()
            .title(title)
            .menu(windows_reactor::Menu::new(items, on_click()));
        bar.push((name.to_owned(), item));
    }
    // 窓の上の行（根の Grid の 0 行目）に置く。
    MenuBar::new()
        .grid_row(0)
        .collection_slot(MenuBarSlot::Items, bar)
}

/// ツールバーの並び（macOS 版の既定の並び、GTK4 版の `toolbar::DEFAULT`）と印。`None` は区切り。
const TOOLBAR: &[Option<(&str, Symbol)>] = &[
    Some(("sidebar", Symbol::OpenPane)),
    Some(("repositories", Symbol::Library)),
    None,
    Some(("branch", Symbol::Switch)),
    Some(("fetch", Symbol::Download)),
    Some(("pull", Symbol::Import)),
    Some(("push", Symbol::Upload)),
    None,
    Some(("merge", Symbol::Shuffle)),
    Some(("rebase", Symbol::Sort)),
    None,
    Some(("stash", Symbol::Save)),
    Some(("pop", Symbol::OpenFile)),
    None,
    Some(("activity", Symbol::Clock)),
    Some(("refresh", Symbol::Refresh)),
    Some(("undo", Symbol::Undo)),
    Some(("search", Symbol::Find)),
];

/// 既定では出さないが、足せるボタン（GTK4 版・macOS 版の `OPTIONAL`）。
const OPTIONAL: &[(&str, Symbol)] = &[
    ("new-branch", Symbol::Add),
    ("terminal", Symbol::Keyboard),
    ("editor", Symbol::Edit),
    ("files", Symbol::Document),
];

/// 並びを置く設定の鍵（GTK4 版と同じ形: `,` でつなぎ、出さないものは `-` を前に付ける。`space` はスペース）。
pub const TOOLBAR_KEY: &str = "toolbarItemsV3";

/// ツールバーの並びの 1 つ（GTK4 版の `toolbar::Entry` のうち、CommandBar で表せるもの）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarEntry {
    Item(String, bool),
    Space,
}

fn icon_of(id: &str) -> Option<Symbol> {
    TOOLBAR
        .iter()
        .flatten()
        .chain(OPTIONAL)
        .find(|(i, _)| *i == id)
        .map(|(_, s)| *s)
}

/// 既定の並び（足せるボタンは出さずに後ろへ）。
pub fn default_toolbar() -> Vec<ToolbarEntry> {
    TOOLBAR
        .iter()
        .map(|e| match e {
            Some((id, _)) => ToolbarEntry::Item((*id).to_owned(), true),
            None => ToolbarEntry::Space,
        })
        .chain(
            OPTIONAL
                .iter()
                .map(|(id, _)| ToolbarEntry::Item((*id).to_owned(), false)),
        )
        .collect()
}

/// 保存した並び（なければ既定）。知らないボタンは除き、並びにないボタンは出さずに後ろへ足す。
pub fn toolbar_entries() -> Vec<ToolbarEntry> {
    let Some(text) = crate::prefs::string(TOOLBAR_KEY).filter(|t| !t.is_empty()) else {
        return default_toolbar();
    };
    let mut entries = Vec::new();
    for part in text.split(',').map(str::trim) {
        let (id, shown) = match part.strip_prefix('-') {
            Some(id) => (id, false),
            None => (part, true),
        };
        match id {
            // 伸縮するスペースも、ここではスペース。境目は置かない。
            "space" | "~" => entries.push(ToolbarEntry::Space),
            _ if icon_of(id).is_some() => entries.push(ToolbarEntry::Item(id.to_owned(), shown)),
            _ => {}
        }
    }
    for (id, _) in TOOLBAR.iter().flatten().chain(OPTIONAL) {
        if !entries
            .iter()
            .any(|e| matches!(e, ToolbarEntry::Item(i, _) if i == id))
        {
            entries.push(ToolbarEntry::Item((*id).to_owned(), false));
        }
    }
    entries
}

/// 並びを保存する。
pub fn save_toolbar(entries: &[ToolbarEntry]) {
    let text: Vec<String> = entries
        .iter()
        .map(|e| match e {
            ToolbarEntry::Item(id, true) => id.clone(),
            ToolbarEntry::Item(id, false) => format!("-{id}"),
            ToolbarEntry::Space => "space".to_owned(),
        })
        .collect();
    let _ = crate::prefs::set_string(TOOLBAR_KEY, &text.join(","));
}

/// ツールバーのボタンの名前（GTK4 版の `toolbar::name`。操作の名前は英語のまま）。
pub fn toolbar_name(id: &str) -> String {
    match id {
        "sidebar" => amonhen_model::tr!("Show Sidebar"),
        "search" => amonhen_model::tr!("Search"),
        "refresh" => amonhen_model::tr!("Refresh"),
        "terminal" => amonhen_model::tr!("Terminal"),
        "editor" => amonhen_model::tr!("Editor"),
        "files" => amonhen_model::tr!("Files"),
        "new-branch" => "New Branch".to_owned(),
        "repositories" => "Repositories".to_owned(),
        _ => {
            let mut name = id.to_owned();
            name[..1].make_ascii_uppercase();
            name
        }
    }
}

/// ツールバーのボタンが押す操作（表の id）。
fn toolbar_command(id: &str) -> &str {
    match id {
        "branch" => "switch-branch",
        "pop" => "pop-stash",
        "sidebar" => "show-sidebar",
        "search" => "find-in-history",
        "terminal" => "open-in-terminal",
        "editor" => "open-in-editor",
        "files" => "reveal-in-files",
        _ => id,
    }
}

/// ツールバー（CommandBar）。押すと `on_click` が操作の id を受ける。まだ動かない操作のボタンは押せない。
pub fn toolbar(on_click: Callback<String>) -> View {
    // 並びは View > Customize Toolbar… で変えられる（GTK4 版の `toolbar.rs`）。
    let commands = toolbar_entries()
        .into_iter()
        .enumerate()
        .filter_map(|(i, entry)| match entry {
            ToolbarEntry::Space => Some(CommandBarCommand::separator(format!("-{i}"))),
            ToolbarEntry::Item(_, false) => None,
            ToolbarEntry::Item(id, true) => {
                let icon = icon_of(&id)?;
                let command = toolbar_command(&id);
                let enabled = IMPLEMENTED.contains(&command);
                Some(
                    CommandBarCommand::button_with_icon(
                        command.to_owned(),
                        toolbar_name(&id),
                        icon,
                    )
                    .enabled(enabled),
                )
            }
        })
        .collect::<Vec<_>>();
    CommandBar::new()
        .grid_row(1)
        .owned_commands(commands, [], on_click)
}

/// 押したキーに当たる、この FE で動く操作（メニューの項目のショートカット）。
pub fn command_for(pressed: MenuShortcut) -> Option<&'static str> {
    if let Some((id, _)) = FILE_ITEMS
        .iter()
        .find(|(_, key)| shortcut(key) == Some(pressed))
    {
        return Some(id);
    }
    amonhen_commands::COMMANDS
        .iter()
        .filter(|c| IMPLEMENTED.contains(&c.id))
        .find(|c| c.shortcut.and_then(shortcut) == Some(pressed))
        .map(|c| c.id)
}

/// 表のショートカット（`Primary+Shift+B` の形）を Windows のキーにする。`Primary` は Ctrl、macOS の ⌃（`Control`）
/// と ⌥（`Alt`）はどちらも Alt（GTK4 版と同じ読み替え）。知らないキーなら `None`。
pub fn shortcut(text: &str) -> Option<MenuShortcut> {
    let mut parts: Vec<&str> = text.split('+').collect();
    let key = parts.pop()?;
    let mut out = MenuShortcut {
        key: virtual_key(key)?,
        ..MenuShortcut::default()
    };
    for m in parts {
        match m {
            "Primary" => out.control = true,
            "Shift" => out.shift = true,
            "Control" | "Alt" => out.alt = true,
            _ => return None,
        }
    }
    Some(out)
}

/// キーの名前の仮想キー（`VK_*`）。
fn virtual_key(key: &str) -> Option<u16> {
    let bytes = key.as_bytes();
    Some(match key {
        "Return" => 0x0D,
        "Left" => 0x25,
        "Up" => 0x26,
        "Right" => 0x27,
        "Down" => 0x28,
        "Comma" => 0xBC,
        "Semicolon" => 0xBA,
        _ if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() => {
            u16::from(bytes[0].to_ascii_uppercase())
        }
        _ if key.starts_with('F') => {
            let n: u16 = key[1..].parse().ok()?;
            if !(1..=24).contains(&n) {
                return None;
            }
            0x70 + n - 1
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_table_shortcuts() {
        let s = shortcut("Primary+Shift+H").unwrap();
        assert_eq!(
            (s.key, s.control, s.shift, s.alt),
            (0x48, true, true, false)
        );
        let s = shortcut("Primary+Alt+Down").unwrap();
        assert_eq!((s.key, s.control, s.alt), (0x28, true, true));
        assert_eq!(shortcut("Primary+2").unwrap().key, 0x32);
        assert_eq!(shortcut("F9").unwrap().key, 0x78);
        assert!(shortcut("Primary+Tab").is_none());
    }
}
