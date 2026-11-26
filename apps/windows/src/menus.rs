//! 窓のメニュー（タイトルバーの ≡）とツールバー。操作の名前とメニューの区切りは、macOS 版・GTK4 版と共通の
//! 操作の表（`amonhen-commands`）から作る。この FE にまだ機能のない操作は出さない（GTK4 版の `PENDING` と同じ
//! 考え）。

use amonhen_commands::{Command, Menu};
use windows_reactor::*;

/// この FE で動く操作。
pub const IMPLEMENTED: &[&str] = &[
    "show-sidebar",
    "wrap-long-lines",
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
    "export-archive",
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
        // 折り返しているときは ✓（ほかの切り替えと同じ）。
        "wrap-long-lines" => {
            let mark = if crate::data::wrap_lines() {
                "✓ "
            } else {
                ""
            };
            format!("{mark}{}", amonhen_model::i18n::translate(c.title))
        }
        _ if c.localized => amonhen_model::i18n::translate(c.title).into_owned(),
        _ => c.title.to_owned(),
    }
}

/// メニューの見出しと項目（タイトルバーの ≡ に入れ子で入れる）。項目を選ぶと、メニューの `on_click` が操作の
/// id を受ける。
pub fn menus() -> Vec<(String, Vec<MenuItem>)> {
    let menus: [(Menu, &'static str); 7] = [
        (Menu::File, "File"),
        (Menu::Edit, "Edit"),
        (Menu::View, "View"),
        (Menu::Navigate, "Navigate"),
        (Menu::Repository, "Repository"),
        (Menu::Commit, "Commit"),
        (Menu::Window, "Window"),
    ];
    let mut out = Vec::new();
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
            // 行に出す項目（GTK4 版の View の History の見せ方）。
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
                ("history-item-ci", crate::render::items::CI, "CI Status"),
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
            // 変更ファイルを差分の左に木で出すか（切ると、差分の見出しだけ）。
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
            // その前に Settings…（macOS 版・GTK4 版はアプリのメニューに置くが、Windows にはアプリのメニューが
            // ないので、Windows のアプリの習いどおり File の終わりの方に置く）。
            if let Some(c) = amonhen_commands::find("settings") {
                items.push(MenuItem::separator("-settings"));
                let item = MenuItem::item(c.id, title(c));
                items.push(match c.shortcut.and_then(shortcut) {
                    Some(s) => item.with_shortcut(s),
                    None => item,
                });
            }
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
        // Window の終わりに、開いている窓（macOS 版の Window メニューの窓の並び。選ぶとその窓を前に出す）。
        if menu == Menu::Window {
            let windows = crate::tabs::windows();
            if windows.len() > 1 {
                items.push(MenuItem::separator("-windows"));
                let focused = crate::tabs::focused();
                for (id, tab) in windows {
                    let Some(tab) = tab else { continue };
                    let name = crate::tabs::title(tab.id)
                        .unwrap_or_else(|| crate::tabs::name(&tab.location));
                    let label = if id == focused {
                        format!("✓ {name}")
                    } else {
                        name
                    };
                    items.push(MenuItem::item(format!("focus-window:{id}"), label));
                }
            }
        }
        if items.is_empty() {
            continue;
        }
        out.push((amonhen_model::i18n::translate(name).into_owned(), items));
    }
    out
}

/// タイトルバーの右の ≡。メニューバーの中身をすべて、メニューごとに入れ子で入れる（Windows にはアプリの外に
/// 出すメニューバーがないので、窓の中の 1 つのボタンにまとめる）。
pub fn menu_button(on_click: Callback<String>) -> View {
    let items: Vec<MenuItem> = menus()
        .into_iter()
        .enumerate()
        .map(|(i, (title, items))| MenuItem::submenu(format!("menu-{i}"), title, items))
        .collect();
    Button::new()
        .vertical_alignment(VerticalAlignment::Center)
        .content(TextBlock::new().text("\u{2261}").font_size(18.0))
        .tooltip(amonhen_model::tr!("Commands"))
        .menu(windows_reactor::Menu::new(items, on_click))
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
/// ツールバーの見せ方を覚える鍵（`icon-text`・`icon`・`text`）。
pub const TOOLBAR_DISPLAY_KEY: &str = "toolbarDisplay";

/// ツールバーの見せ方（macOS 版の NSToolbar のカスタマイズのシートの「表示」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    IconAndText,
    Icon,
    Text,
}

impl Display {
    pub const ALL: [Display; 3] = [Display::IconAndText, Display::Icon, Display::Text];

    fn key(self) -> &'static str {
        match self {
            Display::IconAndText => "icon-text",
            Display::Icon => "icon",
            Display::Text => "text",
        }
    }

    fn title(self) -> String {
        match self {
            Display::IconAndText => amonhen_model::tr!("Icon and Text"),
            Display::Icon => amonhen_model::tr!("Icon Only"),
            Display::Text => amonhen_model::tr!("Text Only"),
        }
    }
}

/// 覚えたツールバーの見せ方（なければアイコンとテキスト。macOS 版の既定と同じ）。
pub fn toolbar_display() -> Display {
    let key = crate::prefs::string(TOOLBAR_DISPLAY_KEY);
    Display::ALL
        .into_iter()
        .find(|d| Some(d.key()) == key.as_deref())
        .unwrap_or(Display::IconAndText)
}

pub fn set_toolbar_display(display: Display) {
    if let Err(e) = crate::prefs::set_string(TOOLBAR_DISPLAY_KEY, display.key()) {
        eprintln!("ツールバーの見せ方を覚えられない: {e}");
    }
}

/// ツールバーの並びの 1 つ（GTK4 版の `toolbar::Entry` のうち、CommandBar で表せるもの）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarEntry {
    Item(String, bool),
    Space,
    /// 伸縮するスペース（macOS 版の flexibleSpace。余った幅を等分する）。
    Flex,
}

fn icon_of(id: &str) -> Option<Symbol> {
    TOOLBAR
        .iter()
        .flatten()
        .chain(OPTIONAL)
        .find(|(i, _)| *i == id)
        .map(|(_, s)| *s)
}

/// 既定の並び（足せるボタンは出さずに後ろへ）。最後の区切りは、macOS 版と同じく伸縮するスペース（Activity から右を
/// 右の端に寄せる）。
pub fn default_toolbar() -> Vec<ToolbarEntry> {
    let last = TOOLBAR.iter().rposition(Option::is_none);
    TOOLBAR
        .iter()
        .enumerate()
        .map(|(i, e)| match e {
            Some((id, _)) => ToolbarEntry::Item((*id).to_owned(), true),
            None if Some(i) == last => ToolbarEntry::Flex,
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
            "space" => entries.push(ToolbarEntry::Space),
            "~" => entries.push(ToolbarEntry::Flex),
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
            ToolbarEntry::Flex => "~".to_owned(),
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
    /// 走っている操作の数（Activity のボタンが回る。macOS 版の Activity の項目の回る印）。
    pub running: usize,
    /// History の上でコミットを編集している（HEAD を動かす操作は押せないように見せ、わけをツールチップに）。
    pub editing: bool,
}

/// Push で送れるものがあるか（上流がなければ、送り先のリモートがあれば Publish できる）。なければツールバーの
/// Push を押せないように見せる（GTK4 版・macOS 版はボタンを押せなくする）。
fn can_push(t: Tracking) -> bool {
    if t.published {
        t.ahead > 0
    } else {
        !t.no_remote
    }
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

/// ツールバーのボタンの字形（Segoe Fluent Icons。macOS 版の SF Symbols に近いもの）。
/// ツールバーのアイコンの大きさ（Segoe Fluent Icons の既定のアイコンと同じ 20）。
const TOOLBAR_ICON_SIZE: f32 = 20.0;

/// ツールバーのボタンのアイコン（`id` は `publish` も）。どの FE でも同じ形の共通のアイコン（`crates/icons`、
/// `docs/icons.md`）を、線ごとに WinUI の `Path`（線を引く図形）で重ねて描く。色は文字の色（テーマに従う）。Files は
/// Explorer のフォルダ（Segoe）のまま。
fn toolbar_icon(id: &str) -> View {
    if id == "files" {
        return FontIcon::new().glyph(glyph_of(id)).into();
    }
    crate::icons::icon(id, TOOLBAR_ICON_SIZE)
}

fn glyph_of(id: &str) -> &'static str {
    match id {
        "files" => "\u{E8B7}",
        _ => "\u{E712}",
    }
}

/// ツールバー（macOS 版と同じく、アイコンの下に名前を出す平らなボタンの並び）。押すと `on_click` が操作の id を
/// 受ける。まだ動かない操作のボタンは押せない。Pull・Push は `tracking` で、上流との差の数をアイコンのバッジに、
/// 送れないわけをツールチップに出す。
pub fn toolbar(on_click: Callback<String>, tracking: Tracking) -> View {
    // 並びは View > Customize Toolbar… で変えられる（GTK4 版の `toolbar.rs`）。
    let buttons: Vec<(String, View, bool)> = toolbar_entries()
        .into_iter()
        .enumerate()
        .filter_map(|(i, entry)| match entry {
            ToolbarEntry::Space => {
                Some((format!("-{i}"), View::from(Grid::new().width(12.0)), false))
            }
            ToolbarEntry::Flex => Some((format!("~{i}"), View::from(Grid::new()), true)),
            ToolbarEntry::Item(_, false) => None,
            ToolbarEntry::Item(id, true) => {
                icon_of(&id)?;
                let command = toolbar_command(&id).to_owned();
                let enabled = IMPLEMENTED.contains(&command.as_str());
                let held =
                    tracking.editing && crate::historyedit::MOVES_HEAD.contains(&command.as_str());
                let (tooltip, count) = match id.as_str() {
                    _ if held => (crate::historyedit::editing_hint(), 0),
                    "pull" => (pull_tooltip(tracking), tracking.behind),
                    "push" => (push_tooltip(tracking), tracking.ahead),
                    "activity" if tracking.running > 0 => (
                        amonhen_model::tr!(
                            "{count} operations running (click for Activity)",
                            count = tracking.running
                        ),
                        0,
                    ),
                    _ => (toolbar_name(&id), 0),
                };
                // 送れないときは押せないように見せるだけにする（WinUI は押せない部品のツールチップを出さないので、
                // 本当に押せなくするとわけが読めない。押せばベルが鳴る）。
                let faded = !enabled || held || (id == "push" && !can_push(tracking));
                // アイコンと、上流との差の数のバッジ（macOS 版の `setTracking` の `badge`。0 なら出さない）。
                let badge: View = if count > 0 {
                    InfoBadge::new()
                        .value(i32::try_from(count).unwrap_or(i32::MAX))
                        .horizontal_alignment(HorizontalAlignment::Right)
                        .vertical_alignment(VerticalAlignment::Top)
                        .margin(Thickness::new(0.0, -6.0, -14.0, 0.0))
                        .into()
                } else {
                    View::empty()
                };
                // Activity は、操作が走っている間はアイコンの左で回る（macOS 版と同じ）。
                let spinner: View = if id == "activity" && tracking.running > 0 {
                    ProgressRing::new()
                        .is_active(true)
                        .width(14.0)
                        .height(14.0)
                        .into()
                } else {
                    View::empty()
                };
                // 上流がなければ Push は Publish（送り先を選んで初めて送る）。名前もアイコンも変わる（macOS 版の
                // `setTracking` と同じ）。
                let shown = if id == "push" && !tracking.published {
                    "publish"
                } else {
                    id.as_str()
                };
                let display = toolbar_display();
                let icon = StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(4.0)
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .children((spinner, Grid::new().children((toolbar_icon(shown), badge))));
                let click = on_click.clone();
                let key = command.clone();
                let button = Button::new()
                    .style(ButtonStyle::Subtle)
                    .is_enabled(enabled)
                    .min_width(56.0)
                    .opacity(if faded { 0.4 } else { 1.0 })
                    .on_click(move || {
                        let _ = click.call(key.clone());
                    })
                    .content(
                        StackPanel::new()
                            .spacing(2.0)
                            .horizontal_alignment(HorizontalAlignment::Center)
                            .children((
                                if display == Display::Text {
                                    View::empty()
                                } else {
                                    icon
                                },
                                if display == Display::Icon {
                                    View::empty()
                                } else {
                                    TextBlock::new()
                                        .text(toolbar_name(shown))
                                        .font_size(if display == Display::Text {
                                            13.0
                                        } else {
                                            11.0
                                        })
                                        .horizontal_alignment(HorizontalAlignment::Center)
                                        .into()
                                },
                            )),
                    )
                    .tooltip(tooltip);
                Some((command, button, false))
            }
        })
        .collect();
    // 伸縮するスペースは余った幅を等分する（macOS 版と同じ）。ないときは左に寄せる。
    let columns: Vec<GridLength> = buttons
        .iter()
        .map(|(_, _, flex)| {
            if *flex {
                GridLength::Star(1.0)
            } else {
                GridLength::Auto
            }
        })
        .collect();
    let cells = buttons
        .into_iter()
        .enumerate()
        .map(|(i, (key, view, _))| (key, Grid::new().grid_column(i as i32).children((view,))));
    Grid::new()
        .columns(columns)
        .vertical_alignment(VerticalAlignment::Center)
        .children((View::keyed_fragment(cells),))
}

/// カスタマイズの帯とパレットのタイル（アイコンと名前。スペースは枠だけ）。
fn tile(entry: &ToolbarEntry) -> View {
    let (id, name, width) = match entry {
        ToolbarEntry::Item(id, _) => (id.as_str(), toolbar_name(id), None),
        ToolbarEntry::Space => ("", amonhen_model::tr!("Space"), Some(64.0)),
        ToolbarEntry::Flex => ("", amonhen_model::tr!("Flexible Space"), Some(120.0)),
    };
    let icon: View = if id.is_empty() {
        Border::new()
            .height(16.0)
            .border_thickness(Thickness::uniform(1.0))
            .border_brush(Brush::Solid(Color::argb(0x80, 0x80, 0x80, 0x80)))
            .corner_radius(3.0)
            .into()
    } else {
        toolbar_icon(id)
    };
    let tile = StackPanel::new()
        .spacing(2.0)
        .min_width(48.0)
        .margin(Thickness::new(2.0, 0.0, 2.0, 0.0))
        .children((
            icon,
            TextBlock::new()
                .text(name)
                .font_size(11.0)
                .horizontal_alignment(HorizontalAlignment::Center),
        ));
    match width {
        Some(w) => StackPanel::new().width(w).children((tile,)),
        None => tile,
    }
}

/// 帯の項目の Tag（並びの中の位置）。最後の `end` は、帯の終わりに落とす所。
pub fn strip_tag(index: usize) -> String {
    format!("t{index}")
}

/// カスタマイズしている間のツールバー（macOS 版の NSToolbar のカスタマイズ中）: ボタンは押せず、引いて並べ替え、
/// パレットへ引いて外す。パレットから引いてきた項目は、落としたタイルの前か後ろ（右半分なら後ろ）に入る。
pub fn toolbar_strip(entries: &[ToolbarEntry], on_reordered: Callback<Vec<String>>) -> View {
    let items = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !matches!(e, ToolbarEntry::Item(_, false)))
        .map(|(i, entry)| {
            let tag = strip_tag(i);
            (
                tag.clone(),
                GridViewItem::new().tag(tag).content(tile(entry)),
            )
        })
        .chain(std::iter::once((
            "end".to_owned(),
            GridViewItem::new()
                .tag("end")
                .content(Grid::new().width(56.0).height(40.0)),
        )));
    // 並びが変わるたびに一覧ごと作り直す（一覧が自分で並べ替えた後に項目を差し替えると、Tag と並びがずれる）。
    let key: String = entries.iter().map(|e| format!("{e:?};")).collect();
    // 1 列に並べ、入りきらなければ横にスクロールする（GridView は幅が足りないと折り返す）。
    let strip = ScrollViewer::new()
        .horizontal_scroll_bar_visibility(ScrollBarVisibility::Auto)
        .vertical_scroll_bar_visibility(ScrollBarVisibility::Disabled)
        .content(
            GridView::new()
                .can_drag_items(true)
                .can_reorder_items(true)
                .allow_drop(true)
                .on_reordered(on_reordered)
                .collection_slot(GridViewSlot::Items, items),
        );
    View::keyed_fragment([(key, strip)])
}

/// パレットから引いてきた項目（`ids`）を、帯のタイル `target`（`strip_tag` か `end`）の前か後ろ（`after`）に入れた
/// 並び。ボタンは 1 つずつなので、もうあれば動かす。スペースと伸縮するスペースは足す。`default` は既定の並びにする。
pub fn dropped_on_toolbar(
    entries: Vec<ToolbarEntry>,
    target: &str,
    ids: &[String],
    after: bool,
) -> Vec<ToolbarEntry> {
    if ids.iter().any(|id| id == "default") {
        return default_toolbar();
    }
    // 入れる所を、タイルの印（入れる前の並びの位置）で覚えておき、動かすボタンを抜いてから数え直す。
    let mut marked: Vec<(Option<usize>, ToolbarEntry)> = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| (Some(i), e))
        .collect();
    let added: Vec<ToolbarEntry> = ids
        .iter()
        .map(|id| match id.as_str() {
            "space" => ToolbarEntry::Space,
            "~" => ToolbarEntry::Flex,
            id => ToolbarEntry::Item(id.to_owned(), true),
        })
        .collect();
    marked.retain(|(_, e)| {
        !matches!(e, ToolbarEntry::Item(id, _) if added.iter().any(|a| matches!(a, ToolbarEntry::Item(b, _) if b == id)))
    });
    let shown = |e: &ToolbarEntry| !matches!(e, ToolbarEntry::Item(_, false));
    let at = match target
        .strip_prefix('t')
        .and_then(|t| t.parse::<usize>().ok())
    {
        Some(index) => marked
            .iter()
            .position(|(i, _)| *i == Some(index))
            .map(|p| p + usize::from(after)),
        None => None,
    }
    // 帯の終わり（出しているものの後ろ）。
    .unwrap_or_else(|| {
        marked
            .iter()
            .rposition(|(_, e)| shown(e))
            .map_or(0, |p| p + 1)
    });
    for (k, entry) in added.into_iter().enumerate() {
        marked.insert(at + k, (None, entry));
    }
    marked.into_iter().map(|(_, e)| e).collect()
}

/// 並べ替えた帯のタイルの Tag（`strip_tag`、`end`）が、今の並びの帯のタイルと同じ組か（違えば前の並びの知らせ）。
pub fn same_strip(entries: &[ToolbarEntry], tags: &[String]) -> bool {
    let mut now: Vec<String> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !matches!(e, ToolbarEntry::Item(_, false)))
        .map(|(i, _)| strip_tag(i))
        .collect();
    let mut got: Vec<String> = tags.iter().filter(|t| *t != "end").cloned().collect();
    now.sort();
    got.sort();
    now == got
}

/// 帯のタイル（`strip_tag` の Tag）をパレットへ引いて外した並び。ボタンは出さないで後ろへ、スペースは消す。
pub fn removed_from_toolbar(entries: Vec<ToolbarEntry>, tags: &[String]) -> Vec<ToolbarEntry> {
    let gone: Vec<usize> = tags
        .iter()
        .filter_map(|t| t.strip_prefix('t')?.parse().ok())
        .collect();
    let mut kept = Vec::new();
    let mut hidden = Vec::new();
    for (i, entry) in entries.into_iter().enumerate() {
        match entry {
            ToolbarEntry::Item(id, true) if gone.contains(&i) => {
                hidden.push(ToolbarEntry::Item(id, false))
            }
            ToolbarEntry::Space | ToolbarEntry::Flex if gone.contains(&i) => {}
            entry => kept.push(entry),
        }
    }
    kept.extend(hidden);
    kept
}

/// パレットの項目の Tag（ボタンの id、`space`、`~`、既定のセットは `default`）。
pub fn palette_tags() -> Vec<String> {
    TOOLBAR
        .iter()
        .flatten()
        .chain(OPTIONAL)
        .map(|(id, _)| (*id).to_owned())
        .chain(["space".to_owned(), "~".to_owned()])
        .collect()
}

/// カスタマイズのパレット（macOS 版の NSToolbar のシート）: ツールバーに引いて足す項目と、既定のセット。
/// `palette` と `defaults` は、引き始め・落とし先を付ける生の要素の置き場。
/// `strip` は編集する並び（`toolbar_strip`）。窓の見出しの上では項目を引けないので、シートの一番上に置く。
pub fn toolbar_palette(
    strip: impl Into<View>,
    palette: &ElementRef<Grid>,
    defaults: &ElementRef<Grid>,
    on_display: Callback<Option<usize>>,
    on_done: Callback<()>,
) -> View {
    let tags = palette_tags();
    let items = tags.into_iter().map(|tag| {
        let entry = match tag.as_str() {
            "space" => ToolbarEntry::Space,
            "~" => ToolbarEntry::Flex,
            id => ToolbarEntry::Item(id.to_owned(), true),
        };
        (
            tag.clone(),
            GridViewItem::new().tag(tag).content(tile(&entry)),
        )
    });
    let default_set: Vec<(String, View)> = default_toolbar()
        .iter()
        .filter(|e| !matches!(e, ToolbarEntry::Item(_, false)))
        .enumerate()
        .map(|(i, e)| (format!("{i}"), tile(e)))
        .collect();
    let default_item = GridViewItem::new().tag("default").content(
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .children((View::keyed_fragment(default_set),)),
    );
    Grid::new().children((StackPanel::new()
        .spacing(8.0)
        .margin(Thickness::new(16.0, 12.0, 16.0, 12.0))
        .children((
            Border::new()
                .padding(Thickness::new(6.0, 4.0, 6.0, 4.0))
                .corner_radius(6.0)
                .border_thickness(Thickness::uniform(1.0))
                .border_brush(Brush::Solid(Color::argb(0x80, 0x80, 0x80, 0x80)))
                .content(strip),
            TextBlock::new().text(amonhen_model::tr!(
                "Drag your favorite items into the toolbar…"
            )),
            Grid::new().element_ref(palette).children((GridView::new()
                .can_drag_items(true)
                .collection_slot(GridViewSlot::Items, items),)),
            TextBlock::new().text(amonhen_model::tr!(
                "…or drag the default set into the toolbar."
            )),
            Grid::new().element_ref(defaults).children((GridView::new()
                .can_drag_items(true)
                .collection_slot(GridViewSlot::Items, [("default".to_owned(), default_item)]),)),
            // 下の行: 左に見せ方（macOS 版のシートの「表示」）、右に Done。
            Grid::new()
                .columns([
                    GridLength::Auto,
                    GridLength::Auto,
                    GridLength::Star(1.0),
                    GridLength::Auto,
                ])
                .column_spacing(8.0)
                .children((
                    TextBlock::new()
                        .text(amonhen_model::tr!("Show"))
                        .vertical_alignment(VerticalAlignment::Center),
                    ComboBox::new()
                        .grid_column(1)
                        .items_source(
                            Display::ALL
                                .into_iter()
                                .map(Display::title)
                                .collect::<Vec<_>>(),
                        )
                        .selected_index(
                            Display::ALL
                                .iter()
                                .position(|d| *d == toolbar_display())
                                .unwrap_or(0),
                        )
                        .on_selection_changed(on_display),
                    Button::new()
                        .grid_column(3)
                        .style(ButtonStyle::Accent)
                        .on_click(on_done)
                        .content(amonhen_model::tr!("Done")),
                )),
        )),))
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

#[cfg(test)]
mod toolbar_tests {
    use super::*;

    fn item(id: &str) -> ToolbarEntry {
        ToolbarEntry::Item(id.to_owned(), true)
    }

    #[test]
    fn dropping_inserts_moves_and_restores() {
        let entries = vec![
            item("fetch"),
            ToolbarEntry::Space,
            item("push"),
            ToolbarEntry::Item("files".into(), false),
        ];
        // push の右半分に伸縮するスペース。
        assert_eq!(
            dropped_on_toolbar(entries.clone(), "t2", &["~".into()], true),
            [
                item("fetch"),
                ToolbarEntry::Space,
                item("push"),
                ToolbarEntry::Flex,
                ToolbarEntry::Item("files".into(), false)
            ]
        );
        // もうある push を fetch の前へ動かす。
        assert_eq!(
            dropped_on_toolbar(entries.clone(), "t0", &["push".into()], false),
            [
                item("push"),
                item("fetch"),
                ToolbarEntry::Space,
                ToolbarEntry::Item("files".into(), false)
            ]
        );
        // 出していない files を帯の終わりへ。
        assert_eq!(
            dropped_on_toolbar(entries.clone(), "end", &["files".into()], false),
            [
                item("fetch"),
                ToolbarEntry::Space,
                item("push"),
                item("files")
            ]
        );
        assert_eq!(
            dropped_on_toolbar(entries, "t0", &["default".into()], false),
            default_toolbar()
        );
    }

    #[test]
    fn a_reorder_of_the_previous_strip_is_stale() {
        let entries = vec![
            ToolbarEntry::Item("fetch".into(), true),
            ToolbarEntry::Space,
            ToolbarEntry::Item("pull".into(), true),
            ToolbarEntry::Item("push".into(), false),
        ];
        let tags = |t: &[&str]| t.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(same_strip(&entries, &tags(&["t2", "t0", "t1", "end"])));
        // 外した後の並びに、外す前の帯の知らせが来た。
        assert!(!same_strip(&entries, &tags(&["t0", "t1", "t2", "t3"])));
        assert!(!same_strip(&entries, &tags(&["t0", "t2"])));
    }

    #[test]
    fn removing_hides_buttons_and_drops_spaces() {
        let entries = vec![item("fetch"), ToolbarEntry::Space, item("push")];
        assert_eq!(
            removed_from_toolbar(entries, &["t0".into(), "t1".into()]),
            [item("push"), ToolbarEntry::Item("fetch".into(), false)]
        );
    }

    #[test]
    fn the_default_set_ends_with_a_flexible_space_before_activity() {
        let entries = default_toolbar();
        let activity = entries.iter().position(|e| *e == item("activity")).unwrap();
        assert_eq!(entries[activity - 1], ToolbarEntry::Flex);
    }
}
