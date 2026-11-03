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

/// 項目を文として並べる（macOS 版の `ListFormatter`: 英語は「a, b, and c」、日本語は「a、b、c」）。
pub fn list(parts: &[String]) -> String {
    list_in(amonhen_model::i18n::current(), parts)
}

fn list_in(lang: amonhen_model::i18n::Lang, parts: &[String]) -> String {
    use amonhen_model::i18n::Lang;
    match (lang, parts) {
        (_, []) => String::new(),
        (_, [one]) => one.clone(),
        (Lang::Ja, _) => parts.join("、"),
        (Lang::En, [a, b]) => format!("{a} and {b}"),
        (Lang::En, [head @ .., last]) => format!("{}, and {last}", head.join(", ")),
    }
}

/// 日付（地域の書式。macOS 版の DateFormatter の日付と同じく、言語の設定に従う）。
pub fn date(time: i64) -> String {
    format_time(time, "%x")
}

/// History の 2 行の行の日時（macOS 版の DateFormatter の日付 .medium・時刻 .short: 英語は
/// 「Nov 3, 2026 at 10:00 AM」、日本語は「2026/11/03 10:00」）。
pub fn medium_date_time(time: i64) -> String {
    let Some(d) = glib::DateTime::from_unix_local(time).ok() else {
        return String::new();
    };
    let clock = format_time_of(&d, clock_format(&d, false));
    medium_date(
        amonhen_model::i18n::current(),
        d.year(),
        d.month(),
        d.day_of_month(),
        &clock,
    )
}

/// [`medium_date_time`] の組み立て。
fn medium_date(
    lang: amonhen_model::i18n::Lang,
    year: i32,
    month: i32,
    day: i32,
    clock: &str,
) -> String {
    use amonhen_model::i18n::Lang;
    match lang {
        Lang::Ja => format!("{year}/{month:02}/{day:02} {clock}"),
        Lang::En => {
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let name = MONTHS[(month.clamp(1, 12) - 1) as usize];
            format!("{name} {day}, {year} at {clock}")
        }
    }
}

/// 時刻の書式（地域の書式が午前・午後を使うなら 12 時間で。`seconds` なら秒まで）。
fn clock_format(d: &glib::DateTime, seconds: bool) -> &'static str {
    let twelve = match (d.format("%p"), d.format("%X")) {
        (Ok(p), Ok(x)) => !p.is_empty() && x.contains(p.as_str()),
        _ => false,
    };
    match (twelve, seconds) {
        (true, false) => "%l:%M %p",
        (true, true) => "%r",
        (false, false) => "%H:%M",
        (false, true) => "%H:%M:%S",
    }
}

/// 日付と時刻（地域の書式。`seconds` なら秒まで。macOS 版の DateFormatter の時刻の .short・.medium）。
pub fn date_time(time: i64, seconds: bool) -> String {
    let Some(d) = glib::DateTime::from_unix_local(time).ok() else {
        return String::new();
    };
    format_time_of(&d, &format!("%x {}", clock_format(&d, seconds)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_model::i18n::Lang;

    #[test]
    fn medium_dates_follow_the_language() {
        assert_eq!(
            medium_date(Lang::En, 2026, 11, 3, "10:00 AM"),
            "Nov 3, 2026 at 10:00 AM"
        );
        assert_eq!(
            medium_date(Lang::Ja, 2026, 11, 3, "10:00"),
            "2026/11/03 10:00"
        );
    }

    #[test]
    fn lists_join_like_list_formatter() {
        let parts = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(list_in(Lang::En, &parts(&["a"])), "a");
        assert_eq!(list_in(Lang::En, &parts(&["a", "b"])), "a and b");
        assert_eq!(list_in(Lang::En, &parts(&["a", "b", "c"])), "a, b, and c");
        assert_eq!(list_in(Lang::Ja, &parts(&["a", "b", "c"])), "a、b、c");
    }
}
