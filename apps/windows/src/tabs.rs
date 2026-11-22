//! 窓のタブ（macOS 版の窓のタブ、GTK4 版の `tabs.rs`）。タブはリポジトリの場所と、出す画面だけを持つ。前に出して
//! いるタブのリポジトリだけを開いておき、タブを替えると根の部品を作り直して、そのタブのリポジトリを開く（History は
//! 永続のキャッシュから読むので速い）。

use std::cell::{Cell, RefCell};

use serde_json::{Value, json};

/// 1 つのタブ。
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    /// 並べ替え・閉じるで指す番号（窓の中で重ならない）。
    pub id: u64,
    pub location: String,
    /// Repositories の画面から出す（New Tab）。
    pub repositories: bool,
    /// Working Copy から出す（前に出していたとき Working Copy だった）。
    pub working_copy: bool,
    /// File History のタブ（そのファイルのパス。macOS 版・GTK4 版と同じく、リポジトリのタブの隣に開く）。
    pub file_history: Option<String>,
}

thread_local! {
    static TABS: RefCell<Vec<Tab>> = const { RefCell::new(Vec::new()) };
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
    /// 前のタブを替えたときに、根の部品を作り直させる。
    static REMOUNT: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

fn new_id() -> u64 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

fn tab(location: &str, repositories: bool, working_copy: bool) -> Tab {
    Tab {
        id: new_id(),
        location: location.to_owned(),
        repositories,
        working_copy,
        file_history: None,
    }
}

/// 起動のときのタブ（`locations` が空なら `first` だけ）。
pub fn init(
    locations: &[String],
    active: usize,
    first: &str,
    repositories: bool,
    working_copy: bool,
) {
    let tabs: Vec<Tab> = if locations.is_empty() {
        vec![tab(first, repositories, working_copy)]
    } else {
        locations.iter().map(|l| tab(l, false, false)).collect()
    };
    let active = active.min(tabs.len() - 1);
    TABS.with(|t| *t.borrow_mut() = tabs);
    ACTIVE.with(|a| a.set(active));
}

/// タブを替えたときに呼ぶもの（窓の外側の部品が渡す）。
pub fn set_remount(f: impl Fn() + 'static) {
    REMOUNT.with(|r| *r.borrow_mut() = Some(Box::new(f)));
}

pub fn tabs() -> Vec<Tab> {
    TABS.with(|t| t.borrow().clone())
}

pub fn active() -> usize {
    ACTIVE.with(Cell::get)
}

/// 前に出しているタブ。
pub fn current() -> Option<Tab> {
    TABS.with(|t| t.borrow().get(active()).cloned())
}

/// タブの名前（フォルダの名前）。
pub fn name(location: &str) -> String {
    let trimmed = location.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(trimmed)
        .to_owned()
}

/// `index` のタブを前に出し、そのリポジトリを開き直す。
fn activate(index: usize) {
    ACTIVE.with(|a| a.set(index));
    if let Some(tab) = current() {
        crate::app::set_target(&tab);
    }
    REMOUNT.with(|r| {
        if let Some(f) = r.borrow().as_ref() {
            f();
        }
    });
}

/// 前のタブが今どの画面か（替えて戻ったときに同じ画面から出す）。
pub fn note_page(working_copy: bool) {
    TABS.with(|t| {
        if let Some(tab) = t.borrow_mut().get_mut(active()) {
            tab.working_copy = working_copy;
        }
    });
}

/// Repositories のタブを前に出す（macOS 版の Repositories のウインドウのタブ）。この窓にあればそれを、なければ
/// 今のタブの右に新しく開く。そこで選んだリポジトリは、そのタブに開く。
pub fn show_repositories() {
    let found = TABS.with(|t| t.borrow().iter().position(|tab| tab.repositories));
    match found {
        Some(index) => select(index),
        None => new_tab(),
    }
}

/// 今のタブが Repositories のタブか。
pub fn current_is_repositories() -> bool {
    current().is_some_and(|tab| tab.repositories)
}

/// `index` のタブを前に出す（もう前なら何もしない）。
pub fn select(index: usize) {
    if index != active() && index < TABS.with(|t| t.borrow().len()) {
        activate(index);
    }
}

/// 次（`1`）・前（`-1`）のタブ（端から反対の端へ回る）。
pub fn step(delta: isize) {
    let len = TABS.with(|t| t.borrow().len()) as isize;
    if len > 1 {
        select((active() as isize + delta).rem_euclid(len) as usize);
    }
}

/// 新しいタブ（今のリポジトリの Repositories の画面。そこで選んだリポジトリがこのタブに開く）を、今のタブの右に。
pub fn new_tab() {
    let Some(now) = current() else { return };
    insert(tab(&now.location, true, false));
}

fn insert(tab: Tab) {
    let index = active() + 1;
    TABS.with(|t| {
        let mut tabs = t.borrow_mut();
        let index = index.min(tabs.len());
        tabs.insert(index, tab);
    });
    activate(index);
}

/// `location` のリポジトリの `path` の File History を、今のタブの右のタブで開く（もう開いていればそのタブを前に）。
pub fn open_file_history(location: &str, path: &str) {
    let found = TABS.with(|t| {
        t.borrow()
            .iter()
            .position(|tab| tab.file_history.as_deref() == Some(path) && tab.location == location)
    });
    match found {
        Some(index) => select(index),
        None => insert(Tab {
            file_history: Some(path.to_owned()),
            ..tab(location, false, false)
        }),
    }
}

/// リポジトリを新しいタブで開く。
pub fn open(location: &str) -> Result<(), String> {
    check(location)?;
    insert(tab(location, false, false));
    Ok(())
}

/// 今のタブのリポジトリを替える（Repositories の Open、worktree の切り替え）。
pub fn replace(location: &str, working_copy: bool) -> Result<(), String> {
    check(location)?;
    let index = active();
    TABS.with(|t| {
        if let Some(tab) = t.borrow_mut().get_mut(index) {
            *tab = self::tab(location, false, working_copy);
        }
    });
    activate(index);
    Ok(())
}

fn check(location: &str) -> Result<(), String> {
    if crate::repositories::is_repository(location) {
        Ok(())
    } else {
        Err(amonhen_model::tr!("Not found — {path}", path = location))
    }
}

/// タブを閉じる。最後のタブなら閉じずに true（窓を閉じる）。
pub fn close(id: u64) -> bool {
    let Some(index) = TABS.with(|t| t.borrow().iter().position(|tab| tab.id == id)) else {
        return false;
    };
    let len = TABS.with(|t| t.borrow().len());
    if len <= 1 {
        return true;
    }
    TABS.with(|t| t.borrow_mut().remove(index));
    // 後ろに置いていた、そのタブのリポジトリを閉じる。
    crate::app::forget_parked(id);
    let active = active();
    if index == active {
        activate(index.min(len - 2));
    } else if index < active {
        ACTIVE.with(|a| a.set(active - 1));
    }
    false
}

/// 前のタブを閉じる。最後のタブなら true（窓を閉じる）。
pub fn close_current() -> bool {
    current().is_none_or(|tab| close(tab.id))
}

/// タブを引いて並べ替えた（`order` は番号の並び）。前のタブは同じタブのまま。
pub fn reorder(order: &[String]) {
    let active_id = current().map(|t| t.id);
    TABS.with(|t| {
        let mut tabs = t.borrow_mut();
        let mut sorted: Vec<Tab> = order
            .iter()
            .filter_map(|id| id.parse::<u64>().ok())
            .filter_map(|id| tabs.iter().find(|tab| tab.id == id).cloned())
            .collect();
        // 並びに出てこなかったもの（ないはず）は後ろに残す。
        for tab in tabs.iter() {
            if !sorted.iter().any(|s| s.id == tab.id) {
                sorted.push(tab.clone());
            }
        }
        *tabs = sorted;
        if let Some(index) = tabs.iter().position(|tab| Some(tab.id) == active_id) {
            ACTIVE.with(|a| a.set(index));
        }
    });
}

/// 覚えておく形（次に引数なしで起動したときに開き直す）。
pub fn saved() -> String {
    // File History のタブは覚えない（リポジトリのタブだけを戻す）。前のタブがそれなら、その左のリポジトリのタブ。
    let now = active();
    let (locations, active) = TABS.with(|t| {
        let tabs = t.borrow();
        let before = tabs[..now.min(tabs.len())]
            .iter()
            .filter(|tab| tab.file_history.is_none())
            .count();
        let kept = tabs.get(now).is_some_and(|tab| tab.file_history.is_none());
        let locations: Vec<String> = tabs
            .iter()
            .filter(|tab| tab.file_history.is_none())
            .map(|tab| tab.location.clone())
            .collect();
        (
            locations,
            if kept {
                before
            } else {
                before.saturating_sub(1)
            },
        )
    });
    json!({ "tabs": locations, "active": active }).to_string()
}

/// 覚えていたタブ（場所と前のタブ。なくなったリポジトリは除く）。
pub fn parse_saved(text: &str, exists: impl Fn(&str) -> bool) -> (Vec<String>, usize) {
    let value: Value = serde_json::from_str(text).unwrap_or_default();
    let all: Vec<String> = value["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let active = value["active"].as_u64().unwrap_or(0) as usize;
    let active_location = all.get(active).cloned();
    let kept: Vec<String> = all.into_iter().filter(|l| exists(l)).collect();
    let active = active_location
        .and_then(|l| kept.iter().position(|k| *k == l))
        .unwrap_or(0);
    (kept, active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_the_folder() {
        assert_eq!(name(r"C:\src\amonhen"), "amonhen");
        assert_eq!(name("C:/src/amonhen/"), "amonhen");
        assert_eq!(name("repo"), "repo");
    }

    #[test]
    fn saved_tabs_skip_missing_and_keep_the_active_one() {
        let text = r#"{"tabs":["a","gone","b"],"active":2}"#;
        let (tabs, active) = parse_saved(text, |l| l != "gone");
        assert_eq!(tabs, ["a", "b"]);
        assert_eq!(active, 1);
        // 前のタブがなくなっていれば先頭。
        let (tabs, active) = parse_saved(r#"{"tabs":["a","gone"],"active":1}"#, |l| l != "gone");
        assert_eq!(tabs, ["a"]);
        assert_eq!(active, 0);
        assert_eq!(parse_saved("", |_| true), (Vec::new(), 0));
    }

    #[test]
    fn file_history_tabs_open_beside_and_are_not_remembered() {
        init(&["a".into(), "b".into()], 0, "", false, false);
        open_file_history("a", "src/x.rs");
        let shown: Vec<(String, Option<String>)> = tabs()
            .iter()
            .map(|t| (t.location.clone(), t.file_history.clone()))
            .collect();
        assert_eq!(
            shown,
            [
                ("a".into(), None),
                ("a".into(), Some("src/x.rs".into())),
                ("b".into(), None),
            ]
        );
        assert_eq!(active(), 1);
        // 同じファイルをもう一度開けば、そのタブを前に出すだけ。
        select(2);
        open_file_history("a", "src/x.rs");
        assert_eq!((tabs().len(), active()), (3, 1));
        // 覚えるのはリポジトリのタブだけ（前のタブが File History なら、その左のタブ）。
        let saved: Value = serde_json::from_str(&saved()).unwrap();
        assert_eq!(saved, json!({ "tabs": ["a", "b"], "active": 0 }));
    }

    #[test]
    fn closing_moves_to_the_neighbour() {
        init(&["a".into(), "b".into(), "c".into()], 1, "", false, false);
        let ids: Vec<u64> = tabs().iter().map(|t| t.id).collect();
        assert!(!close(ids[1]));
        assert_eq!(
            tabs()
                .iter()
                .map(|t| t.location.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(current().unwrap().location, "c");
        // 前より左を閉じると、前のタブは同じまま。
        assert!(!close(ids[0]));
        assert_eq!(current().unwrap().location, "c");
        // 最後の 1 つは閉じない（窓を閉じる）。
        assert!(close(ids[2]));
        assert_eq!(tabs().len(), 1);
    }

    #[test]
    fn reordering_keeps_the_active_tab() {
        init(&["a".into(), "b".into(), "c".into()], 0, "", false, false);
        let ids: Vec<String> = tabs().iter().map(|t| t.id.to_string()).collect();
        reorder(&[ids[2].clone(), ids[1].clone(), ids[0].clone()]);
        assert_eq!(
            tabs()
                .iter()
                .map(|t| t.location.as_str())
                .collect::<Vec<_>>(),
            ["c", "b", "a"]
        );
        assert_eq!(current().unwrap().location, "a");
        assert_eq!(active(), 2);
    }
}
