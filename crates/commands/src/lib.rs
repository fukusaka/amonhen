//! 操作の表: メニュー・ツールバー・ショートカットに出す操作の、FE に共通の元（docs/ux.md の「Menus」）。
//!
//! 各操作は id・英語の名前・説明・OS によらない形のショートカットと、メニューのどの区切りに属するかを持つ。
//! id は GTK4 の FE の action 名（`win.<id>`）と同じ。macOS の FE には、この表から作った Swift のファイル
//! （`apps/macos/Sources/AmonHen/CommandTable.swift`、[`swift_source`]）を渡す。呼び先と、押せるかどうかの判定、
//! 区切りを画面のどこに置くかは FE が決める。
//!
//! 名前の決まり: git の操作と画面の名前（Fetch・Push・Working Copy など）は英語のまま（`localized: false`）、それ
//! 以外は英語の名前を鍵にして訳す（macOS は `Localizable.strings`、GTK4 はコアの `ja.strings`）。シートを開くもの
//! だけ「…」で終える。
//!
//! ショートカットは `Primary+Shift+B` の形。`Primary` は macOS で ⌘、ほかで Ctrl、`Control` は macOS で ⌃、`Alt` は
//! macOS で ⌥。GTK では `Control` と `Alt` をどちらも Alt に読み替えるので、ぶつかるものや、デスクトップが全体で
//! 使うキーは `gtk` で替える。

/// 操作を受ける所。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// 前にあるリポジトリの窓。
    Window,
    /// アプリ（窓がなくても使える）。
    App,
}

/// GTK でのショートカット。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gtk {
    /// 表のものを読み替える。
    Same,
    /// 付けない。
    None,
    /// これにする（`Primary+F9` の形）。
    Key(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub id: &'static str,
    /// 英語の名前（訳の鍵）。
    pub title: &'static str,
    /// 名前を訳すか（git の操作と画面の名前は訳さない）。
    pub localized: bool,
    /// 説明（訳す）。
    pub tooltip: Option<&'static str>,
    /// ショートカット（`Primary+Shift+B` の形）。
    pub shortcut: Option<&'static str>,
    pub gtk: Gtk,
    pub scope: Scope,
}

const fn command(id: &'static str, title: &'static str) -> Command {
    Command {
        id,
        title,
        localized: false,
        tooltip: None,
        shortcut: None,
        gtk: Gtk::Same,
        scope: Scope::Window,
    }
}

impl Command {
    const fn localized(mut self) -> Self {
        self.localized = true;
        self
    }
    const fn key(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
    const fn tip(mut self, tooltip: &'static str) -> Self {
        self.tooltip = Some(tooltip);
        self
    }
    const fn gtk(mut self, gtk: Gtk) -> Self {
        self.gtk = gtk;
        self
    }
    const fn app(mut self) -> Self {
        self.scope = Scope::App;
        self
    }

    /// シートを開くか（名前が「…」で終わる）。
    pub fn opens_sheet(&self) -> bool {
        self.title.ends_with('…')
    }

    /// GTK のショートカット（`<Control><Alt>f` の形）。
    pub fn gtk_accel(&self) -> Option<String> {
        let shortcut = match self.gtk {
            Gtk::Same => self.shortcut?,
            Gtk::None => return None,
            Gtk::Key(key) => key,
        };
        let (mods, key) = split(shortcut);
        let mut accel = String::new();
        let mut alt = false;
        for m in mods {
            match m {
                "Primary" => accel.push_str("<Control>"),
                "Shift" => accel.push_str("<Shift>"),
                // ⌃ と ⌥ は、どちらも Alt（GTK では Ctrl が Primary なので）。
                "Control" | "Alt" if !alt => {
                    accel.push_str("<Alt>");
                    alt = true;
                }
                _ => {}
            }
        }
        accel.push_str(&gtk_key(key));
        Some(accel)
    }
}

/// `Primary+Shift+B` を修飾キーとキーに分ける。
fn split(shortcut: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = shortcut.split('+').collect();
    let key = parts.pop().unwrap_or_default();
    (parts, key)
}

fn gtk_key(key: &str) -> String {
    match key {
        "Return" | "Up" | "Down" | "Left" | "Right" => key.to_owned(),
        "Comma" => "comma".to_owned(),
        "Semicolon" => "semicolon".to_owned(),
        _ if key.starts_with('F') && key.len() > 1 => key.to_owned(),
        _ => key.to_lowercase(),
    }
}

/// 操作の表（メニューに出す順ではない。並びは [`SECTIONS`]）。
pub static COMMANDS: &[Command] = &[
    // リモート
    command("fetch", "Fetch…").key("Primary+Control+F"),
    command("pull", "Pull…").key("Primary+Control+L"),
    command("push", "Push…").key("Primary+Control+P"),
    command("force-push", "Force Push…"),
    command("set-upstream", "Set Upstream…").localized(),
    command("add-remote", "Add Remote…").localized(),
    command("create-pull-request", "Create Pull Request…").localized(),
    command("open-on-github", "Open on GitHub").localized(),
    // ブランチ・タグ・worktree
    command("switch-branch", "Switch Branch…")
        .localized()
        .key("Primary+Shift+B"),
    command("new-branch", "New Branch…").key("Primary+Alt+B"),
    command("rename-branch", "Rename Branch…").localized(),
    // Ctrl+Alt+T は、Ubuntu などで端末を開くキー。
    command("new-tag", "New Tag…")
        .key("Primary+Alt+T")
        .gtk(Gtk::None),
    command("new-worktree", "New Worktree…"),
    command("prune-worktrees", "Prune Worktrees").localized(),
    // 取り込み・書き換え
    command("merge", "Merge…").key("Primary+Alt+M"),
    command("rebase", "Rebase…").key("Primary+Alt+R"),
    command("compare-with-previous", "Compare with Previous Version")
        .localized()
        .tip("Compare the current branch with its version before it was last rewritten (a rebase, History edit, amend or reset), like git range-diff"),
    command("apply-patch", "Apply Patch…").localized(),
    command("continue", "Continue"),
    command("skip", "Skip"),
    command("abort", "Abort"),
    command("stash", "Stash…").key("Primary+Alt+S"),
    command("pop-stash", "Pop…").key("Primary+Alt+Shift+S"),
    command("undo", "Undo…").key("Primary+Alt+Z"),
    // コミット
    command("commit", "Commit").key("Primary+Return"),
    command("commit-and-push", "Commit and Push").key("Primary+Shift+Return"),
    command("stage-all", "Stage All"),
    command("save-patch", "Save as Patch…").localized(),
    // 画面・移動
    command("show-working-copy", "Working Copy").key("Primary+1"),
    command("show-history", "History").key("Primary+2"),
    command("show-stashes", "Stashes").key("Primary+4"),
    command("show-reflog", "Reflog").key("Primary+5"),
    command("refresh", "Refresh")
        .localized()
        .key("Primary+R")
        .tip("Reload History, branches and Working Copy (they normally reload on their own when something changes)"),
    command("rebuild-history", "Rebuild History")
        .localized()
        .tip("Discard the History cache and rebuild it from scratch (over the shared daemon, the daemon’s too)"),
    // GNOME では F9 がサイドバー（Ctrl+Alt+S は Stash とぶつかる）。
    command("show-sidebar", "Show Sidebar")
        .localized()
        .key("Primary+Control+S")
        .gtk(Gtk::Key("F9")),
    // GTK では Fetch（Ctrl+Alt+F）とぶつかる。
    command("filter-branches", "Filter Branches…")
        .localized()
        .key("Primary+Alt+F")
        .gtk(Gtk::Key("Primary+Alt+Shift+F")),
    command("go-to-head", "Go to HEAD")
        .localized()
        .key("Primary+Shift+H"),
    command("go-to-parent", "Go to Parent")
        .localized()
        .key("Primary+Alt+Down")
        .gtk(Gtk::Key("Alt+Down")),
    command("go-to-child", "Go to Child")
        .localized()
        .key("Primary+Alt+Up")
        .gtk(Gtk::Key("Alt+Up")),
    command("find-in-history", "Find in History…")
        .localized()
        .key("Primary+Shift+F"),
    command("repository-settings", "Repository Settings")
        .localized()
        .key("Primary+Semicolon"),
    command("open-in-terminal", "Open in Terminal").localized(),
    // GTK の FE では「Show in Files」と呼ぶ（名前は FE が替える。id は同じ）。
    command("reveal-in-files", "Show in Finder").localized(),
    // アプリ
    command("quick-open", "Quick Open…")
        .key("Primary+Shift+O")
        .app(),
    command("repositories", "Repositories")
        .key("Primary+Shift+R")
        .app(),
    command("activity", "Activity").key("Primary+Alt+A").app(),
    command("settings", "Settings…")
        .localized()
        .key("Primary+Comma")
        .app(),
];

/// メニュー（区切りを置く所の目安。どこに出すかは FE が決める）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    App,
    File,
    Edit,
    View,
    Navigate,
    Repository,
    Commit,
    Window,
}

/// メニューの中の、区切りで囲まれたひとまとまり。
#[derive(Clone, Copy, Debug)]
pub struct Section {
    pub menu: Menu,
    pub ids: &'static [&'static str],
}

const fn section(menu: Menu, ids: &'static [&'static str]) -> Section {
    Section { menu, ids }
}

/// メニューの並び（上から）。表のどの操作も、どれか 1 つに入る。
pub static SECTIONS: &[Section] = &[
    section(Menu::App, &["settings"]),
    section(Menu::File, &["quick-open"]),
    section(Menu::Edit, &["find-in-history"]),
    section(
        Menu::View,
        &[
            "show-working-copy",
            "show-history",
            "show-stashes",
            "show-reflog",
        ],
    ),
    section(Menu::View, &["refresh", "rebuild-history"]),
    section(Menu::View, &["show-sidebar"]),
    section(
        Menu::Navigate,
        &[
            "switch-branch",
            "filter-branches",
            "go-to-head",
            "go-to-parent",
            "go-to-child",
        ],
    ),
    section(Menu::Repository, &["fetch", "pull", "push", "force-push"]),
    section(
        Menu::Repository,
        &["new-branch", "rename-branch", "set-upstream", "new-tag"],
    ),
    section(
        Menu::Repository,
        &["new-worktree", "prune-worktrees", "add-remote"],
    ),
    section(
        Menu::Repository,
        &["merge", "rebase", "compare-with-previous", "apply-patch"],
    ),
    section(Menu::Repository, &["continue", "skip", "abort"]),
    section(Menu::Repository, &["stash", "pop-stash"]),
    section(Menu::Repository, &["undo"]),
    section(Menu::Repository, &["repository-settings"]),
    section(Menu::Repository, &["create-pull-request", "open-on-github"]),
    section(Menu::Repository, &["open-in-terminal", "reveal-in-files"]),
    section(Menu::Commit, &["commit", "commit-and-push", "stage-all"]),
    section(Menu::Commit, &["save-patch"]),
    section(Menu::Window, &["repositories", "activity"]),
];

/// id の操作。
pub fn find(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// `menu` の区切りごとの操作（上から）。
pub fn menu(menu: Menu) -> Vec<Vec<&'static Command>> {
    SECTIONS
        .iter()
        .filter(|s| s.menu == menu)
        .map(|s| s.ids.iter().filter_map(|id| find(id)).collect())
        .collect()
}

/// Swift の識別子（`force-push` → `forcePush`。予約語は `` ` `` で囲む）。
fn swift_name(id: &str) -> String {
    let mut out = String::new();
    for (i, part) in id.split('-').enumerate() {
        let mut chars = part.chars();
        if i == 0 {
            out.push_str(part);
        } else if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if matches!(out.as_str(), "continue" | "default" | "repeat" | "switch") {
        format!("`{out}`")
    } else {
        out
    }
}

fn swift_string(s: &str) -> String {
    format!("{s:?}")
}

/// macOS の `keyEquivalent` と修飾キー。
fn swift_shortcut(shortcut: Option<&str>) -> (String, String) {
    let Some(shortcut) = shortcut else {
        return ("\"\"".to_owned(), "[.command]".to_owned());
    };
    let (mods, key) = split(shortcut);
    let key = match key {
        "Return" => "\"\\r\"".to_owned(),
        "Down" => "String(UnicodeScalar(NSDownArrowFunctionKey)!)".to_owned(),
        "Up" => "String(UnicodeScalar(NSUpArrowFunctionKey)!)".to_owned(),
        "Comma" => "\",\"".to_owned(),
        "Semicolon" => "\";\"".to_owned(),
        _ => swift_string(&key.to_lowercase()),
    };
    let mods: Vec<&str> = mods
        .iter()
        .filter_map(|m| match *m {
            "Primary" => Some(".command"),
            "Control" => Some(".control"),
            "Alt" => Some(".option"),
            "Shift" => Some(".shift"),
            _ => None,
        })
        .collect();
    (key, format!("[{}]", mods.join(", ")))
}

/// macOS の FE に渡す Swift のファイル（`CommandTable.swift`）の中身。
pub fn swift_source() -> String {
    let mut out = String::from(
        "// 生成したファイル。手で直さない: 元は crates/commands（`AMONHEN_UPDATE=1 cargo test -p amonhen-commands` で作り直す）。\n\
         import AppKit\n\n\
         /// 操作の表（crates/commands の `COMMANDS`）。呼び先は `Commands` が結びつける。\n\
         enum CommandTable {\n    \
         struct Entry {\n        \
         let id: String\n        \
         /// 英語の名前（`localized` なら訳の鍵）。\n        \
         let title: String\n        \
         let localized: Bool\n        \
         let key: String\n        \
         let modifiers: NSEvent.ModifierFlags\n        \
         /// 説明（訳の鍵）。\n        \
         let toolTip: String?\n        \
         /// アプリ（`AppDelegate`）が受けるか。\n        \
         let app: Bool\n    \
         }\n\n",
    );
    for c in COMMANDS {
        let (key, mods) = swift_shortcut(c.shortcut);
        let tip = c.tooltip.map_or("nil".to_owned(), swift_string);
        out.push_str(&format!(
            "    static let {} = Entry(\n        id: {}, title: {}, localized: {}, key: {}, modifiers: {},\n        toolTip: {}, app: {})\n",
            swift_name(c.id),
            swift_string(c.id),
            swift_string(c.title),
            c.localized,
            key,
            mods,
            tip,
            c.scope == Scope::App,
        ));
    }
    out.push_str("\n    /// メニューの区切り（上から）。\n");
    for (name, menu) in [
        ("view", Menu::View),
        ("navigate", Menu::Navigate),
        ("repository", Menu::Repository),
        ("commit", Menu::Commit),
    ] {
        let sections: Vec<String> = SECTIONS
            .iter()
            .filter(|s| s.menu == menu)
            .map(|s| {
                let ids: Vec<String> = s.ids.iter().map(|id| swift_name(id)).collect();
                format!("[{}]", ids.join(", "))
            })
            .collect();
        out.push_str(&format!(
            "    static let {name}Menu: [[Entry]] = [\n        {},\n    ]\n",
            sections.join(",\n        ")
        ));
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// `"鍵" = "値";` の形の訳（引用符のエスケープを解く）。
    fn strings(path: &str) -> HashMap<String, String> {
        let text = std::fs::read_to_string(root().join(path)).unwrap();
        let mut table = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if !line.starts_with('"') {
                continue;
            }
            let mut parts = Vec::new();
            let mut current = String::new();
            let mut inside = false;
            let mut chars = line.chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' if inside => {
                        parts.push(std::mem::take(&mut current));
                        inside = false;
                    }
                    '"' => inside = true,
                    '\\' if inside => {
                        if let Some(n) = chars.next() {
                            current.push(if n == 'n' { '\n' } else { n });
                        }
                    }
                    _ if inside => current.push(c),
                    _ => {}
                }
            }
            if parts.len() == 2 {
                table.insert(parts[0].clone(), parts[1].clone());
            }
        }
        table
    }

    #[test]
    fn ids_are_unique_and_every_command_is_in_one_section() {
        let mut ids = HashSet::new();
        for c in COMMANDS {
            assert!(ids.insert(c.id), "id が重なっている: {}", c.id);
        }
        let mut placed = HashSet::new();
        for s in SECTIONS {
            for id in s.ids {
                assert!(find(id).is_some(), "表にない id: {id}");
                assert!(placed.insert(*id), "2 つの区切りにある: {id}");
            }
        }
        assert_eq!(placed, ids, "どの区切りにもない操作がある");
    }

    #[test]
    fn shortcuts_do_not_collide() {
        let mut mac = HashMap::new();
        let mut gtk = HashMap::new();
        for c in COMMANDS {
            if let Some(s) = c.shortcut {
                let (mut mods, key) = split(s);
                mods.sort();
                let normal = format!("{}+{key}", mods.join("+"));
                if let Some(other) = mac.insert(normal, c.id) {
                    panic!("macOS のショートカットがぶつかる: {other} と {}", c.id);
                }
            }
            if let Some(accel) = c.gtk_accel()
                && let Some(other) = gtk.insert(accel.clone(), c.id)
            {
                panic!(
                    "GTK のショートカットがぶつかる: {other} と {}（{accel}）",
                    c.id
                );
            }
        }
        // GTK の FE が自分で使うキー。
        for reserved in [
            "<Control>q",
            "<Control>w",
            "<Control>n",
            "<Control>o",
            "<Control>f",
            "<Control>question",
        ] {
            assert!(
                !gtk.contains_key(reserved),
                "GTK の FE のキーとぶつかる: {reserved}"
            );
        }
    }

    #[test]
    fn titles_follow_the_naming_rules() {
        for c in COMMANDS {
            assert!(!c.title.contains("..."), "「…」を使う: {}", c.title);
            if let Some(tip) = c.tooltip {
                assert!(!tip.is_empty() && !tip.ends_with('.'), "説明の形: {tip}");
            }
        }
    }

    #[test]
    fn every_localized_title_has_both_translations() {
        let mac = strings("apps/macos/Localization/ja.lproj/Localizable.strings");
        let core = strings("crates/model/src/i18n/ja.strings");
        for c in COMMANDS {
            let keys = c.localized.then_some(c.title).into_iter().chain(c.tooltip);
            for key in keys {
                assert!(mac.contains_key(key), "macOS の訳がない: {key}");
                assert!(core.contains_key(key), "コアの訳（GTK4）がない: {key}");
            }
        }
    }

    #[test]
    fn swift_table_is_up_to_date() {
        let path = root().join("apps/macos/Sources/AmonHen/CommandTable.swift");
        let source = swift_source();
        if std::env::var_os("AMONHEN_UPDATE").is_some() {
            std::fs::write(&path, &source).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == source,
            "CommandTable.swift が表と違う。AMONHEN_UPDATE=1 cargo test -p amonhen-commands で作り直す"
        );
    }

    #[test]
    fn gtk_accels() {
        assert_eq!(
            find("fetch").unwrap().gtk_accel().as_deref(),
            Some("<Control><Alt>f")
        );
        assert_eq!(
            find("pop-stash").unwrap().gtk_accel().as_deref(),
            Some("<Control><Alt><Shift>s")
        );
        assert_eq!(
            find("commit").unwrap().gtk_accel().as_deref(),
            Some("<Control>Return")
        );
        assert_eq!(
            find("settings").unwrap().gtk_accel().as_deref(),
            Some("<Control>comma")
        );
        assert_eq!(
            find("show-sidebar").unwrap().gtk_accel().as_deref(),
            Some("F9")
        );
        assert_eq!(find("new-tag").unwrap().gtk_accel(), None);
    }
}
