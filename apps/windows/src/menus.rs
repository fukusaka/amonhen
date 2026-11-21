//! 窓のメニューバー。操作の名前とメニューの区切りは、macOS 版・GTK4 版と共通の操作の表（`amonhen-commands`）から
//! 作る。この FE にまだ機能のない操作は出さない（GTK4 版の `PENDING` と同じ考え）。

use amonhen_commands::{Command, Menu};
use windows_reactor::{
    Callback, CommandBar, CommandBarCommand, GridChildExt, MenuBar, MenuBarItem, MenuBarSlot,
    MenuExt, MenuItem, MenuShortcut, SlotsControl, Symbol, View,
};

/// この FE で動く操作。
pub const IMPLEMENTED: &[&str] = &[
    "open-on-github",
    "create-pull-request",
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
    ("new-tab", "Primary+T"),
    ("new-window", "Primary+N"),
    ("open", "Primary+O"),
    ("clone", "Primary+Alt+C"),
];

fn file_item_title(id: &str) -> String {
    match id {
        "new-tab" => amonhen_model::tr!("New Tab"),
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
            // History の範囲（GTK4 版の History のバーの範囲。ref はサイドバーで選ぶ）。
            items.push(MenuItem::separator("-scope"));
            items.push(MenuItem::item(
                "scope-all",
                amonhen_model::tr!("All Branches"),
            ));
            items.push(MenuItem::item(
                "scope-current",
                amonhen_model::tr!("Current Branch"),
            ));
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
            let mark = if crate::prefs::bool(crate::prefs::FOLD_MERGED_BRANCHES) {
                "✓ "
            } else {
                ""
            };
            items.push(MenuItem::item(
                "fold-merged-branches",
                format!("{mark}{}", amonhen_model::tr!("Fold Merged Branches")),
            ));
            let mark = if crate::render::highlight_ancestors() {
                "✓ "
            } else {
                ""
            };
            items.push(MenuItem::item(
                "highlight-ancestors",
                format!("{mark}{}", amonhen_model::tr!("Highlight Ancestors")),
            ));
            // コミットの差分で全ファイルを並べるか（切ると、変更ファイルの一覧で選んだファイルだけ）。
            let mark = if crate::prefs::bool(crate::prefs::STACKED_COMMIT_DIFF) {
                "✓ "
            } else {
                ""
            };
            items.push(MenuItem::item(
                "all-files-in-one-diff",
                format!("{mark}{}", amonhen_model::tr!("All Files in One Diff")),
            ));
            // 変更ファイルを差分の左に木で出すか（切ると、差分の上に一覧）。
            let mark = if crate::prefs::bool(crate::prefs::HISTORY_FILE_TREE) {
                "✓ "
            } else {
                ""
            };
            items.push(MenuItem::item(
                "changed-files-as-tree",
                format!("{mark}{}", amonhen_model::tr!("Changed Files as Tree")),
            ));
            items.push(MenuItem::separator("-toolbar"));
            items.push(MenuItem::item(
                "customize-toolbar",
                amonhen_model::tr!("Customize Toolbar…"),
            ));
        }
        // File の終わりに Close Tab（GTK4 版の Close。最後のタブなら窓を閉じる）。
        if menu == Menu::File {
            items.push(MenuItem::separator("-close"));
            let item = MenuItem::item("close-tab", amonhen_model::tr!("Close Tab"));
            items.push(match shortcut("Primary+W") {
                Some(s) => item.with_shortcut(s),
                None => item,
            });
            // すべての窓を閉じて終わる（macOS 版・GTK4 版の Quit。開いていた窓は次に起動したときに戻る）。
            let item = MenuItem::item("quit", amonhen_model::tr!("Quit Amon Hen"));
            items.push(match shortcut("Primary+Q") {
                Some(s) => item.with_shortcut(s),
                None => item,
            });
        }
        // Window の先頭に、タブを替える項目（macOS 版の Show Next Tab・Show Previous Tab）。
        if menu == Menu::Window {
            let mut tabs = Vec::new();
            for (id, key, title) in [
                (
                    "next-tab",
                    "Primary+Tab",
                    amonhen_model::tr!("Show Next Tab"),
                ),
                (
                    "previous-tab",
                    "Primary+Shift+Tab",
                    amonhen_model::tr!("Show Previous Tab"),
                ),
            ] {
                let item = MenuItem::item(id, title);
                tabs.push(match shortcut(key) {
                    Some(s) => item.with_shortcut(s),
                    None => item,
                });
            }
            if !items.is_empty() {
                tabs.push(MenuItem::separator("-tabs"));
            }
            items.splice(0..0, tabs);
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

/// Pull・Push のボタンの様子（今のブランチの上流との差、上流があるか、リモートがないか）。
#[derive(Clone, Copy, Default)]
pub struct Tracking {
    pub ahead: u32,
    pub behind: u32,
    pub published: bool,
    pub no_remote: bool,
}

/// Pull のツールチップ（上流より後ろならその数。macOS 版の `setTracking` と同じ）。
fn pull_tooltip(t: Tracking) -> String {
    if t.behind > 0 {
        amonhen_model::tr!(
            "{count} commits behind upstream (Ctrl+Alt+L)",
            count = t.behind
        )
    } else {
        amonhen_model::tr!("Pull (Ctrl+Alt+L)")
    }
}

/// Push のツールチップ（押しても送れないときはその理由。macOS 版の `setTracking`、GTK4 版の `push_tooltip` と
/// 同じ文）。上流がなければ Publish（送り先を選ぶ）になる。
fn push_tooltip(t: Tracking) -> String {
    use amonhen_model::tr;
    if !t.published && t.no_remote {
        tr!("No remote to publish to. Add one in Repository > Add Remote….")
    } else if !t.published {
        tr!("Publish — choose the remote to create this branch on first (Ctrl+Alt+P)")
    } else if t.ahead > 0 {
        tr!(
            "{count} commits ahead of upstream (Ctrl+Alt+P)",
            count = t.ahead
        )
    } else if t.behind > 0 {
        tr!(
            "Nothing to push. Pull the {count} new commits first.",
            count = t.behind
        )
    } else {
        tr!("Nothing to push")
    }
}

/// ツールバー（CommandBar）。押すと `on_click` が操作の id を受ける。まだ動かない操作のボタンは押せない。
/// Pull・Push は `tracking` で、上流との差と、送れないわけをツールチップに出す。
pub fn toolbar(on_click: Callback<String>, tracking: Tracking) -> View {
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
                let button = CommandBarCommand::button_with_icon(
                    command.to_owned(),
                    toolbar_name(&id),
                    icon,
                );
                Some(match id.as_str() {
                    "pull" => button.enabled(enabled).tooltip(pull_tooltip(tracking)),
                    // 送れないときも押せるままにする（WinUI は押せない部品のツールチップを出さないので、わけが
                    // 読めなくなる。押せばベルが鳴る）。
                    "push" => button.enabled(enabled).tooltip(push_tooltip(tracking)),
                    _ => button.enabled(enabled),
                })
            }
        })
        .collect::<Vec<_>>();
    CommandBar::new().owned_commands(commands, [], on_click)
}

/// ツールバーの Pull・Push のボタンに、上流との差の数のバッジを付ける（macOS 版の `setTracking` の `badge`。
/// Push は先にある数、Pull は後ろの数。0 なら隠す）。`root` はツールバーを包む部品。
pub fn put_badges(
    root: &windows_core_reactor::IInspectable,
    tracking: Tracking,
) -> windows_core_reactor::Result<()> {
    amonhen_win_xaml::put_badges(
        root,
        &[
            (toolbar_name("pull"), tracking.behind),
            (toolbar_name("push"), tracking.ahead),
        ],
    )
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
        "Tab" => 0x09,
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
        assert_eq!(shortcut("Primary+Shift+Tab").unwrap().key, 0x09);
        assert!(shortcut("Primary+Escape").is_none());
    }
}
