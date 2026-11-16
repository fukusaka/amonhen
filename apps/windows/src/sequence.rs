//! 途中の操作（`sequence` の答え）の見出しと説明（GTK4 版の `statusbar::sequence_title`・`sequence_detail` の
//! うち、コンフリクトと編集で止まったときまで）。

use amonhen_model::tr;
use serde_json::Value;

/// 1 行目: 何をどこへしている途中か。
pub fn title(s: &Value) -> String {
    let get = |k: &str| s[k].as_str().map(str::to_owned);
    let from = s["merging"]
        .as_array()
        .map(|a| {
            amonhen_model::i18n::list(&a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        })
        .or_else(|| get("from"));
    let onto = get("onto").or(from);
    let title = match (s["kind"].as_str().unwrap_or_default(), get("branch"), onto) {
        ("rebase", Some(b), Some(o)) => tr!("Rebasing {branch} onto {onto}", branch = b, onto = o),
        ("merge", Some(b), Some(f)) => tr!("Merging {from} into {branch}", from = f, branch = b),
        ("cherry-pick", Some(b), _) => tr!("Cherry-picking into {branch}", branch = b),
        ("squash", Some(b), Some(f)) => tr!("Squashing {from} into {branch}", from = f, branch = b),
        ("squash", _, _) => tr!("Squash in progress"),
        ("revert", Some(b), _) => tr!("Reverting in {branch}", branch = b),
        ("am", Some(b), _) => tr!("Applying patches to {branch}", branch = b),
        ("am", _, _) => tr!("Applying patches"),
        ("rebase", _, _) => tr!("Rebase in progress"),
        ("merge", _, _) => tr!("Merge in progress"),
        ("cherry-pick", _, _) => tr!("Cherry-pick in progress"),
        _ => tr!("Revert in progress"),
    };
    match (s["current"].as_u64(), s["total"].as_u64()) {
        (Some(c), Some(t)) => format!(
            "{title} · {}",
            tr!("step {current} of {total}", current = c, total = t)
        ),
        _ => title,
    }
}

/// 2 行目: どこで止まっていて、次に何をすればよいか（コンフリクトの数は Working Copy の status から）。
pub fn detail(s: &Value, conflicts: usize) -> String {
    let subject = s["step"]["subject"].as_str().map(|t| format!("“{t}”"));
    if conflicts > 0 {
        return match &subject {
            Some(at) => tr!(
                "{count} files have conflicts at {subject}. Resolve them in Working Copy, then Continue.",
                count = conflicts,
                subject = at
            ),
            None => tr!(
                "{count} files have conflicts. Resolve them in Working Copy, then Continue.",
                count = conflicts
            ),
        };
    }
    if s["editing"].as_bool() == Some(true) {
        return tr!(
            "Stopped to edit {subject}. Change files or amend the commit, then Continue.",
            subject = subject.unwrap_or_default()
        );
    }
    String::new()
}

/// Skip できるか（1 つずつ当てていく操作だけ。merge・revert にはない）。
pub fn can_skip(s: &Value) -> bool {
    let kind = s["kind"].as_str().unwrap_or_default();
    kind.starts_with("rebase") || kind == "cherry-pick" || kind == "am"
}
