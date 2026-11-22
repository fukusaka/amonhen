//! 窓のタブ（macOS 版の窓のタブ、GTK4 版の `tabs.rs`）。タブはリポジトリの場所と、出す画面を持つ。どのタブも
//! そのリポジトリを開いたまま持ち（窓の根の部品が、タブごとに 1 つ）、前のタブだけを見せる。
//!
//! 窓はどれも同じプロセスにある。前に出ている窓のタブの並びを `TABS`・`ACTIVE`・`REMOUNT` に置き（タブの操作は
//! その窓に効く）、ほかの窓のものは `WINDOWS` に取っておく。窓が前に出たら入れ替える（[`focus`]）。

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
    /// 別の窓に出したマージエディタ（そのファイルと、打ちかけの Result を入れた一時ファイル）。
    pub merge: Option<String>,
    pub merge_text: Option<String>,
}

thread_local! {
    static TABS: RefCell<Vec<Tab>> = const { RefCell::new(Vec::new()) };
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
    /// タブの並び・前のタブ・名前が変わったときに、窓の外側の部品を描き直させる。
    static REMOUNT: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    /// タブごとの名前（● と ↑↓ を添えたもの。それぞれのタブの根の部品が知らせる）と、変わった回数。
    static TITLES: RefCell<std::collections::HashMap<u64, String>> = RefCell::new(Default::default());
    static TITLES_CHANGED: Cell<u64> = const { Cell::new(0) };
    /// 前に出ている窓の番号と、ほかの窓のタブの並び（前のタブ、描き直させるもの）。
    static FOCUSED: Cell<u64> = const { Cell::new(1) };
    static WINDOWS: RefCell<std::collections::HashMap<u64, Window>> = RefCell::new(Default::default());
    static NEXT_WINDOW: Cell<u64> = const { Cell::new(2) };
}

/// 後ろの窓のタブの並び。
#[derive(Default)]
struct Window {
    tabs: Vec<Tab>,
    active: usize,
    remount: Option<Box<dyn Fn()>>,
}

/// 前に出ている窓の番号。
pub fn focused() -> u64 {
    FOCUSED.with(Cell::get)
}

/// 新しい窓のタブの並び（窓はまだ後ろ。開いて前に出たら [`focus`] で入れ替わる）。窓の番号を返す。
pub fn new_window(locations: &[String], active: usize) -> u64 {
    let tabs: Vec<Tab> = locations.iter().map(|l| tab(l, false, false)).collect();
    new_window_with(tabs, active)
}

/// 新しい窓（タブをそのまま渡す。Repositories の画面から出すタブなど）。
pub fn new_window_with(tabs: Vec<Tab>, active: usize) -> u64 {
    let id = NEXT_WINDOW.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let active = active.min(tabs.len().saturating_sub(1));
    WINDOWS.with(|w| {
        w.borrow_mut().insert(
            id,
            Window {
                tabs,
                active,
                remount: None,
            },
        );
    });
    id
}

/// 窓 `id` が前に出た。前の窓のタブの並びを取っておき、`id` のものを今のものにする。
pub fn focus(id: u64) {
    let now = focused();
    if now == id {
        return;
    }
    let Some(next) = WINDOWS.with(|w| w.borrow_mut().remove(&id)) else {
        return;
    };
    let previous = Window {
        tabs: TABS.with(|t| std::mem::replace(&mut *t.borrow_mut(), next.tabs)),
        active: ACTIVE.with(|a| a.replace(next.active)),
        remount: REMOUNT.with(|r| std::mem::replace(&mut *r.borrow_mut(), next.remount)),
    };
    // 閉じた窓のもの（タブがない）は取っておかない。
    if !previous.tabs.is_empty() {
        WINDOWS.with(|w| w.borrow_mut().insert(now, previous));
    }
    FOCUSED.with(|f| f.set(id));
}

/// 窓 `id` を閉じた（タブの並びを捨てる）。前の窓なら、ほかの窓が前に出るまで空にしておく。
pub fn forget_window(id: u64) {
    if id == focused() {
        TABS.with(|t| t.borrow_mut().clear());
        REMOUNT.with(|r| *r.borrow_mut() = None);
    } else {
        WINDOWS.with(|w| w.borrow_mut().remove(&id));
    }
}

/// 窓 `window` のタブの並び。
pub fn tabs_of(window: u64) -> Vec<Tab> {
    if window == focused() {
        tabs()
    } else {
        WINDOWS.with(|w| {
            w.borrow()
                .get(&window)
                .map(|w| w.tabs.clone())
                .unwrap_or_default()
        })
    }
}

/// 窓 `window` の前のタブの位置。
pub fn active_of(window: u64) -> usize {
    if window == focused() {
        active()
    } else {
        WINDOWS.with(|w| w.borrow().get(&window).map_or(0, |w| w.active))
    }
}

/// 開いている窓（前に出ている窓が先）の番号と、その前のタブ。
pub fn windows() -> Vec<(u64, Option<Tab>)> {
    let mut out = vec![(focused(), current())];
    WINDOWS.with(|w| {
        let w = w.borrow();
        let mut ids: Vec<u64> = w.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let window = &w[&id];
            out.push((id, window.tabs.get(window.active).cloned()));
        }
    });
    out
}

/// タブ `id` のある窓。
pub fn window_of_tab(id: u64) -> Option<u64> {
    if tabs().iter().any(|t| t.id == id) {
        return Some(focused());
    }
    WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .find(|(_, w)| w.tabs.iter().any(|t| t.id == id))
            .map(|(id, _)| *id)
    })
}

/// タブ `id` を窓から取り出す（ほかの窓へ移すため）。取り出した窓を描き直す。タブと、取り出した窓と、その窓に
/// タブが残っていないかを返す。
pub fn take_tab(id: u64) -> Option<(Tab, u64, bool)> {
    let window = window_of_tab(id)?;
    let take = |tabs: &mut Vec<Tab>, active: &mut usize| {
        let index = tabs.iter().position(|t| t.id == id)?;
        let tab = tabs.remove(index);
        if index < *active || *active >= tabs.len() {
            *active = active.saturating_sub(1);
        }
        Some((tab, tabs.is_empty()))
    };
    let taken = if window == focused() {
        // 前の窓なら、前のタブが変わる前に、窓の入力の部品を取っておく。
        crate::app::leave_front();
        let mut active = active();
        let taken = TABS.with(|t| take(&mut t.borrow_mut(), &mut active));
        ACTIVE.with(|a| a.set(active));
        taken
    } else {
        WINDOWS.with(|w| {
            let mut w = w.borrow_mut();
            let w = w.get_mut(&window)?;
            take(&mut w.tabs, &mut w.active)
        })
    }?;
    refresh_window(window);
    Some((taken.0, window, taken.1))
}

/// タブを窓 `window` の `place`（タブの番号と、その後ろか）に入れて、前のタブにする。`place` が None なら終わり。
pub fn insert_tab(window: u64, tab: Tab, place: Option<(u64, bool)>) {
    // 落とした所のタブの前か後ろ（なければ最後）。
    let at = |tabs: &[Tab]| {
        place
            .and_then(|(id, after)| {
                let i = tabs.iter().position(|t| t.id == id)?;
                Some(if after { i + 1 } else { i })
            })
            .unwrap_or(tabs.len())
    };
    if window == focused() {
        crate::app::leave_front();
        let index = TABS.with(|t| {
            let mut t = t.borrow_mut();
            let index = at(&t);
            t.insert(index, tab);
            index
        });
        ACTIVE.with(|a| a.set(index));
    } else {
        WINDOWS.with(|w| {
            if let Some(w) = w.borrow_mut().get_mut(&window) {
                let index = at(&w.tabs);
                w.tabs.insert(index, tab);
                w.active = index;
            }
        });
    }
    refresh_window(window);
}

/// 窓 `window` を描き直す。
fn refresh_window(window: u64) {
    if window == focused() {
        refresh();
    } else {
        WINDOWS.with(|w| {
            if let Some(f) = w.borrow().get(&window).and_then(|w| w.remount.as_ref()) {
                f();
            }
        });
    }
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
        merge: None,
        merge_text: None,
    }
}

/// 新しい窓に出すタブ（`repositories` なら Repositories の画面から）。
pub fn make(location: &str, repositories: bool) -> Tab {
    tab(location, repositories, false)
}

/// 起動したときの前のタブに、別の窓に出したマージエディタを渡す（`--merge`）。
pub fn set_merge(merge: Option<String>, text: Option<String>) {
    TABS.with(|t| {
        if let Some(tab) = t.borrow_mut().get_mut(active()) {
            tab.merge = merge;
            tab.merge_text = text;
        }
    });
}

/// 窓 `window` のタブを覚える形（[`saved`] と同じ）。
pub fn saved_of(window: u64) -> String {
    if window == focused() {
        saved()
    } else {
        let (tabs, active) = WINDOWS.with(|w| {
            w.borrow()
                .get(&window)
                .map(|w| (w.tabs.clone(), w.active))
                .unwrap_or_default()
        });
        saved_from(&tabs, active)
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

/// 窓 `window` のタブが変わったときに呼ぶもの（その窓の外側の部品が渡す）。
pub fn set_remount(window: u64, f: impl Fn() + 'static) {
    if window == focused() {
        REMOUNT.with(|r| *r.borrow_mut() = Some(Box::new(f)));
    } else {
        WINDOWS.with(|w| {
            if let Some(w) = w.borrow_mut().get_mut(&window) {
                w.remount = Some(Box::new(f));
            }
        });
    }
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

/// `index` のタブを前に出す（どのタブもリポジトリを開いたままなので、見せるものを替えるだけ）。
fn activate(index: usize) {
    // 今の前のタブの、窓の入力の部品を取っておく（前に戻したら戻す）。
    crate::app::leave_front();
    ACTIVE.with(|a| a.set(index));
    refresh();
}

/// 窓の外側の部品を描き直す。
fn refresh() {
    REMOUNT.with(|r| {
        if let Some(f) = r.borrow().as_ref() {
            f();
        }
    });
}

/// タブ `id` の名前を知らせる（変わったときだけ描き直す）。
pub fn set_title(id: u64, title: &str) {
    let changed = TITLES.with(|t| {
        let mut titles = t.borrow_mut();
        if titles.get(&id).map(String::as_str) == Some(title) {
            false
        } else {
            titles.insert(id, title.to_owned());
            true
        }
    });
    if changed {
        TITLES_CHANGED.with(|c| c.set(c.get() + 1));
        // そのタブのある窓を描き直す。
        let window = if tabs().iter().any(|t| t.id == id) {
            focused()
        } else {
            WINDOWS.with(|w| {
                w.borrow()
                    .iter()
                    .find(|(_, w)| w.tabs.iter().any(|t| t.id == id))
                    .map_or(0, |(id, _)| *id)
            })
        };
        refresh_window(window);
    }
}

/// タブ `id` の名前（まだ知らされていなければ None）。
pub fn title(id: u64) -> Option<String> {
    TITLES.with(|t| t.borrow().get(&id).cloned())
}

/// 名前が変わった回数（窓の外側の部品が、根の部品に描き直させるために渡す）。
pub fn titles_changed() -> u64 {
    TITLES_CHANGED.with(Cell::get)
}

/// 前のタブが今どの画面か（替えて戻ったときに同じ画面から出す）。
pub fn note_page(working_copy: bool) {
    TABS.with(|t| {
        if let Some(tab) = t.borrow_mut().get_mut(active()) {
            tab.working_copy = working_copy;
            tab.repositories = false;
        }
    });
}

/// `index` のタブを前に出す（もう前なら何もしない）。
pub fn select(index: usize) {
    if index != active() && index < TABS.with(|t| t.borrow().len()) {
        activate(index);
    }
}

/// 窓 `window` の前のタブを `id` のタブにして、タブの並びを描き直させる（TabView が別のタブを選んで見せて
/// いても、こちらの前のタブに戻す）。
pub fn reselect(window: u64, id: u64) {
    if window != focused() {
        return;
    }
    match TABS.with(|t| t.borrow().iter().position(|tab| tab.id == id)) {
        Some(index) if index != active() => activate(index),
        _ => refresh(),
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
    TITLES.with(|t| t.borrow_mut().remove(&id));
    let active = active();
    if index == active {
        activate(index.min(len - 2));
    } else {
        if index < active {
            ACTIVE.with(|a| a.set(active - 1));
        }
        // 閉じたタブの根の部品を外す。
        refresh();
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
    TABS.with(|t| saved_from(&t.borrow(), active()))
}

fn saved_from(tabs: &[Tab], now: usize) -> String {
    // File History のタブは覚えない（リポジトリのタブだけを戻す）。前のタブがそれなら、その左のリポジトリのタブ。
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
    let active = if kept {
        before
    } else {
        before.saturating_sub(1)
    };
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

    #[test]
    fn moved_tabs_go_where_they_were_dropped() {
        init(&["a".into(), "b".into(), "c".into()], 0, "", false, false);
        let ids: Vec<u64> = tabs().iter().map(|t| t.id).collect();
        let window = focused();
        let locations = || {
            tabs()
                .iter()
                .map(|t| t.location.clone())
                .collect::<Vec<_>>()
        };
        // b の左半分に落とせば b の前、c の右半分なら c の後ろ、タブの上でなければ最後。どれも前のタブになる。
        insert_tab(window, make("x", false), Some((ids[1], false)));
        assert_eq!(locations(), ["a", "x", "b", "c"]);
        assert_eq!(active(), 1);
        insert_tab(window, make("y", false), Some((ids[2], true)));
        assert_eq!(locations(), ["a", "x", "b", "c", "y"]);
        assert_eq!(active(), 4);
        insert_tab(window, make("z", false), None);
        assert_eq!(locations(), ["a", "x", "b", "c", "y", "z"]);
        assert_eq!(active(), 5);
    }
}
