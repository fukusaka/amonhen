//! Merge・Pull・Rebase の確認に出す、コンフリクトの見込み（GTK4 版の `sheets::MergeForecast`・`rewrite_lines`）。

use amonhen_model::tr;
use serde_json::Value;

/// `predictMerge` の答えを、確認に出す行と、コンフリクトがありそうかにする。`estimate` なら rebase の目安だと
/// 書き添える。失敗した答え（rev がないなど）なら None（何も出さない。GTK4 版と同じ）。
pub fn merge_text(reply: &Value, estimate: bool) -> Option<(String, bool)> {
    if reply["ok"].as_bool() != Some(true) {
        return None;
    }
    let Some(conflicts) = reply["info"]["conflicts"].as_array() else {
        return Some((
            tr!("Can’t check for conflicts ahead (needs git 2.38 or newer)."),
            false,
        ));
    };
    let conflicts: Vec<&str> = conflicts.iter().filter_map(Value::as_str).collect();
    let line = if conflicts.is_empty() {
        tr!("No conflicts expected.")
    } else {
        tr!(
            "⚠︎ Conflicts expected in {count} files: {names}",
            count = conflicts.len(),
            names = first_three(&conflicts)
        )
    };
    let line = if estimate {
        format!(
            "{line} {}",
            tr!("Rebase applies the commits one at a time, so this is only an estimate.")
        )
    } else {
        line
    };
    Some((line, !conflicts.is_empty()))
}

/// 最初の 3 つ（それより多ければ「, …」を足す）。
fn first_three(names: &[&str]) -> String {
    let mut shown = names.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        shown.push_str(", …");
    }
    shown
}

/// `rewritePreview` の答えを、確認に出す行にする（一緒に動く・残るブランチ、push 済み、コンフリクト）。2 つ目は
/// コンフリクトがありそうか。
pub fn rewrite_lines(info: &Value) -> (Vec<String>, bool) {
    let names = |list: &[&Value]| -> String {
        amonhen_model::i18n::list(
            &list
                .iter()
                .filter_map(|b| b["name"].as_str())
                .collect::<Vec<_>>(),
        )
    };
    let moving: Vec<&Value> = info["moving"].as_array().into_iter().flatten().collect();
    let staying: Vec<&Value> = info["staying"].as_array().into_iter().flatten().collect();
    let mut out = Vec::new();
    if !moving.is_empty() {
        out.push(tr!("Also moves {names}.", names = names(&moving)));
    }
    for (reason, key) in [
        (
            "setting",
            "{names} stays where it is (rebase.updateRefs is off).",
        ),
        (
            "git",
            "{names} stays where it is (moving it needs git 2.38).",
        ),
        (
            "checkedOut",
            "{names} stays where it is (checked out in another worktree).",
        ),
        ("off", "{names} stays where it is (not moved this time)."),
    ] {
        let list: Vec<&Value> = staying
            .iter()
            .copied()
            .filter(|b| b["reason"].as_str() == Some(reason))
            .collect();
        if !list.is_empty() {
            out.push(amonhen_model::i18n::fill(
                &amonhen_model::i18n::translate(key),
                &[("names", &names(&list))],
            ));
        }
    }
    let mut pushed: Vec<String> = Vec::new();
    if info["currentPushed"] == true
        && let Some(c) = info["current"].as_str()
    {
        pushed.push(c.to_owned());
    }
    pushed.extend(
        moving
            .iter()
            .filter(|b| b["pushed"] == true)
            .filter_map(|b| b["name"].as_str().map(str::to_owned)),
    );
    if !pushed.is_empty() {
        let list = amonhen_model::i18n::list(&pushed);
        out.push(if pushed.len() == 1 {
            tr!(
                "{names} is pushed, so pushing afterwards needs Force Push.",
                names = list
            )
        } else {
            tr!(
                "{names} are pushed, so pushing afterwards needs Force Push.",
                names = list
            )
        });
    }
    let mut conflicts = false;
    if let Some(list) = info["conflicts"].as_array() {
        if let Some(first) = list.first() {
            conflicts = true;
            let oid = first["oid"].as_str().unwrap_or_default();
            let files: Vec<&str> = first["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let short = &oid[..oid.len().min(8)];
            out.push(if list.len() == 1 {
                tr!(
                    "⚠︎ Conflicts expected at {commit}: {files}",
                    commit = short,
                    files = first_three(&files)
                )
            } else {
                tr!(
                    "⚠︎ Conflicts expected at {count} commits, first at {commit}: {files}",
                    count = list.len(),
                    commit = short,
                    files = first_three(&files)
                )
            });
        } else {
            out.push(tr!("No conflicts expected."));
        }
    }
    (out, conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_forecast_from_reply() {
        // 失敗した答えは何も出さない。
        assert!(merge_text(&json!({ "ok": false, "error": "x" }), false).is_none());
        let (line, conflicts) = merge_text(
            &json!({ "ok": true, "info": { "conflicts": ["a", "b", "c", "d"] } }),
            false,
        )
        .unwrap();
        assert!(conflicts);
        assert!(line.contains("a, b, c, …"));
        // 版が古くて調べられない。
        let (_, conflicts) = merge_text(&json!({ "ok": true, "info": {} }), false).unwrap();
        assert!(!conflicts);
    }

    #[test]
    fn rewrite_lines_list_the_branches() {
        let info = json!({
            "moving": [{ "name": "a" }, { "name": "b" }, { "name": "c" }],
            "staying": [],
        });
        assert_eq!(rewrite_lines(&info).0[0], "Also moves a, b, and c.");
    }
}
