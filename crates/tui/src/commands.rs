//! コマンドの一覧（`:`）: 名前で操作を探して動かす。
//!
//! 名前は、アプリのメニューと同じ操作の表（`amonhen-commands`）から取る。動かすときは、その操作のキーを押した
//! のと同じことをする（操作の中身は 1 か所）。

use amonhen_model::i18n::translate;
use ratatui_crossterm::crossterm::event::KeyCode;

/// その操作を動かせる所。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// どこでも。
    Anywhere,
    /// 右の枠（詳細・差分）にいないとき。
    NotSide,
    /// サイドバーで、選んでいる行に。
    Sidebar,
    /// History に移ってから。
    History,
    /// Working Copy で。
    Changes,
    /// 途中の操作（merge・rebase など）があるとき。
    Sequence,
}

pub struct Entry {
    /// 操作の表の id。
    pub id: &'static str,
    pub key: KeyCode,
    pub control: bool,
    pub place: Place,
}

const fn entry(id: &'static str, key: char, place: Place) -> Entry {
    Entry {
        id,
        key: KeyCode::Char(key),
        control: false,
        place,
    }
}

/// キーのない操作（一覧からだけ動かす。中身は `App::run_keyless`）。
const fn keyless(id: &'static str, place: Place) -> Entry {
    Entry {
        id,
        key: KeyCode::Null,
        control: false,
        place,
    }
}

const fn control(id: &'static str, key: char, place: Place) -> Entry {
    Entry {
        id,
        key: KeyCode::Char(key),
        control: true,
        place,
    }
}

/// 一覧に出す操作（メニューの並び）。
pub static ENTRIES: &[Entry] = &[
    entry("fetch", 'f', Place::Anywhere),
    entry("pull", 'p', Place::Anywhere),
    entry("push", 'P', Place::Anywhere),
    entry("new-branch", 'b', Place::Anywhere),
    entry("rename-branch", 'n', Place::Sidebar),
    entry("new-tag", 'T', Place::Anywhere),
    entry("new-worktree", 'w', Place::Sidebar),
    entry("set-upstream", 'U', Place::Sidebar),
    keyless("new-orphan-branch", Place::Anywhere),
    keyless("clean-up-branches", Place::Anywhere),
    keyless("add-remote", Place::Anywhere),
    keyless("update-submodules", Place::Anywhere),
    keyless("delete-ignored-files", Place::Anywhere),
    keyless("export-archive", Place::History),
    keyless("save-bundle", Place::Anywhere),
    keyless("prune-worktrees", Place::Anywhere),
    entry("merge", 'm', Place::Sidebar),
    entry("rebase", 'r', Place::Sidebar),
    entry("continue", 'C', Place::Sequence),
    entry("skip", 'K', Place::Sequence),
    entry("abort", 'A', Place::Sequence),
    entry("stash", 's', Place::NotSide),
    entry("pop-stash", 'S', Place::NotSide),
    entry("undo", 'u', Place::Anywhere),
    entry("stage-all", '+', Place::Changes),
    control("commit", 's', Place::Changes),
    control("commit-and-push", 'p', Place::Changes),
    entry("show-working-copy", '1', Place::Anywhere),
    entry("show-history", '2', Place::Anywhere),
    entry("show-reflog", '3', Place::Anywhere),
    entry("show-sidebar", '0', Place::Anywhere),
    entry("filter-branches", '/', Place::Sidebar),
    entry("find-in-history", '/', Place::History),
    entry("go-to-head", '@', Place::History),
    entry("go-to-parent", ']', Place::History),
    entry("go-to-child", '[', Place::History),
];

impl Entry {
    /// 一覧に出す名前（アプリのメニューと同じ。git の操作と画面の名前は訳さない）。
    pub fn title(&self) -> String {
        // メニューでは Stash の下に「Pop…」とある。一覧では、何を pop するのかを足す。
        if self.id == "pop-stash" {
            return "Pop Stash…".to_owned();
        }
        match amonhen_commands::find(self.id) {
            Some(command) if command.localized => translate(command.title).into_owned(),
            Some(command) => command.title.to_owned(),
            None => self.id.to_owned(),
        }
    }

    /// キーの書き方（キーの一覧と同じ）。
    pub fn key_label(&self) -> String {
        let KeyCode::Char(ch) = self.key else {
            return String::new();
        };
        match self.control {
            true => format!("Ctrl-{ch}"),
            false => ch.to_string(),
        }
    }
}

/// 名前が、打った語のすべてを含むか（大文字と小文字は区別しない）。
pub fn matches(title: &str, query: &str) -> bool {
    let title = title.to_lowercase();
    query
        .split_whitespace()
        .all(|word| title.contains(&word.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_in_the_command_table() {
        for e in ENTRIES {
            assert!(amonhen_commands::find(e.id).is_some(), "{}", e.id);
        }
    }

    #[test]
    fn every_word_must_match() {
        assert!(matches("Find in History…", "hist find"));
        assert!(matches("Find in History…", ""));
        assert!(!matches("Find in History…", "hist push"));
    }

    #[test]
    fn titles_follow_the_language() {
        use amonhen_model::i18n::{Lang, with};
        let find = ENTRIES.iter().find(|e| e.id == "find-in-history").unwrap();
        let fetch = ENTRIES.iter().find(|e| e.id == "fetch").unwrap();
        assert_eq!(with(Lang::parse("en"), || find.title()), "Find in History…");
        assert_eq!(with(Lang::parse("ja"), || find.title()), "History を検索…");
        // git の操作の名前は訳さない。
        assert_eq!(with(Lang::parse("ja"), || fetch.title()), "Fetch…");
    }
}
