//! 右クリックとドラッグのメニューの、対象の入った名前（macOS 版の `Titles`）。同じ操作はどこでも同じ言い方に
//! する。シートを開くものは「…」で終える。

use amonhen_model::tr;
use gtk::glib;

/// 短いコミット ID の桁（どこでも同じ）。
pub const SHORT_ID: usize = 8;

pub fn short(oid: &str) -> &str {
    &oid[..oid.len().min(SHORT_ID)]
}

pub fn checkout(name: &str) -> String {
    tr!("Checkout {name}…", name = name)
}

pub fn checkout_as_local(remote: &str) -> String {
    tr!("Checkout {name} as Local Branch…", name = remote)
}

pub fn checkout_detached(oid: &str) -> String {
    tr!("Checkout {id} (Detached)…", id = short(oid))
}

pub fn merge(rev: &str, into: &str) -> String {
    tr!("Merge {rev} into {target}…", rev = rev, target = into)
}

pub fn rebase(target: &str, onto: &str) -> String {
    tr!("Rebase {target} onto {rev}…", target = target, rev = onto)
}

pub fn push(branch: &str, to: &str) -> String {
    tr!("Push {branch} to {target}…", branch = branch, target = to)
}

pub fn force_push(branch: &str, to: &str) -> String {
    tr!(
        "Force Push {branch} to {target}…",
        branch = branch,
        target = to
    )
}

pub fn apply(stash: &str) -> String {
    tr!("Apply {name}…", name = stash)
}

pub fn pop(stash: &str) -> String {
    tr!("Pop {name}…", name = stash)
}

pub fn drop(stash: &str) -> String {
    tr!("Drop {name}…", name = stash)
}

pub fn new_branch_from(start: &str) -> String {
    tr!("New Branch from {name}…", name = start)
}

pub fn cherry_pick(oid: &str) -> String {
    tr!("Cherry-pick {id}…", id = short(oid))
}

pub fn revert(oid: &str) -> String {
    tr!("Revert {id}…", id = short(oid))
}

pub fn delete(name: &str) -> String {
    tr!("Delete {name}…", name = name)
}

/// 「Reset main to 1a2b3c4d…」（動かし方は確認のシートで選ぶ）。
pub fn reset(branch: &str, oid: &str) -> String {
    tr!("Reset {branch} to {id}…", branch = branch, id = short(oid))
}

/// Repositories の左の分類: 「すべて Fetch（3）」。
pub fn fetch_all(count: usize) -> String {
    tr!("Fetch All ({count})", count = count)
}

/// Repositories の行（いくつも選んだとき）: 「Fetch（3）」。
pub fn fetch_several(count: usize) -> String {
    tr!("Fetch ({count})", count = count)
}

/// Repositories の SSH のホスト: 「host を一覧から外す…」。
pub fn remove_from_list(name: &str) -> String {
    tr!("Remove {name} from List…", name = name)
}

/// 日付（地域の書式。macOS 版の DateFormatter の日付と同じく、言語の設定に従う）。
pub fn date(time: i64) -> String {
    format_time(time, "%x")
}

/// 日付と時刻（地域の書式。`seconds` なら秒まで。macOS 版の DateFormatter の時刻の .short・.medium）。
pub fn date_time(time: i64, seconds: bool) -> String {
    let Some(d) = glib::DateTime::from_unix_local(time).ok() else {
        return String::new();
    };
    // 地域の時刻の書式（`%X`）が午前・午後を使うなら 12 時間で。
    let twelve = match (d.format("%p"), d.format("%X")) {
        (Ok(p), Ok(x)) => !p.is_empty() && x.contains(p.as_str()),
        _ => false,
    };
    let time = match (twelve, seconds) {
        (true, false) => "%l:%M %p",
        (true, true) => "%r",
        (false, false) => "%H:%M",
        (false, true) => "%H:%M:%S",
    };
    format_time_of(&d, &format!("%x {time}"))
}

fn format_time(time: i64, format: &str) -> String {
    glib::DateTime::from_unix_local(time)
        .map(|d| format_time_of(&d, format))
        .unwrap_or_default()
}

fn format_time_of(d: &glib::DateTime, format: &str) -> String {
    d.format(format)
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}
