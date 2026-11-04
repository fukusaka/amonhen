//! `amonhen subtree`: subtree の一覧と、Add・Pull・Push・Split（`git subtree`）。Pull・Push は、URL と ref を
//! 省くと前に使った相手（`amonhen.subtree.<dir>.*`）に。

use std::path::PathBuf;
use std::process::ExitCode;

use amonhen_model::tr;
use serde_json::json;

use crate::{Args, Result, command, open_repository, usage};

/// `amonhen subtree [list|add|pull|push|split]`。
pub fn subtree(args: &[String]) -> Result<ExitCode> {
    let (action, args) = match args.first().map(String::as_str) {
        Some(a) if !a.starts_with('-') => (a, &args[1..]),
        _ => ("list", args),
    };
    let mut dir = PathBuf::from(".");
    let mut json_out = false;
    let mut squash: Option<bool> = None;
    let mut branch: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json_out = true,
            "--squash" => squash = Some(true),
            "--no-squash" => squash = Some(false),
            "-b" | "--branch" => branch = Some(a.value("-b")?.to_owned()),
            "--" => rest.extend(a.rest.by_ref().cloned()),
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            _ => rest.push(arg.clone()),
        }
    }
    let bad = || -> Box<dyn std::error::Error + Send + Sync> {
        format!(
            "{}\n\n{}",
            tr!("Unknown subtree command: {command}", command = action),
            usage()
        )
        .into()
    };
    let front = open_repository(&dir)?;
    let info = command(&*front, json!({"op": "subtrees"}))?["info"].clone();
    if info["available"] != true {
        return Err(tr!("This machine’s git has no git subtree").into());
    }
    let known = |path: &str| {
        info["subtrees"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|s| s["dir"] == path)
            .cloned()
            .unwrap_or_default()
    };
    // 省いた URL と ref は、前に使った相手（Push の ref は Push で使ったもの、なければ Pull の ref）。
    let target = |path: &str, given: &[String], push: bool| -> Result<(String, String)> {
        if let [url, rev] = given {
            return Ok((url.clone(), rev.clone()));
        }
        let s = known(path);
        let rev = if push {
            s["pushRef"].as_str().or(s["ref"].as_str())
        } else {
            s["ref"].as_str()
        };
        match (given, s["url"].as_str(), rev) {
            ([], Some(url), Some(rev)) => Ok((url.to_owned(), rev.to_owned())),
            _ => Err(tr!("Give the URL and the ref for {dir}", dir = path).into()),
        }
    };
    let op = match (action, &rest[..]) {
        ("list", []) => None,
        ("add", [path, url, rev]) => Some(json!({
            "kind": "add", "dir": path, "url": url, "ref": rev, "squash": squash.unwrap_or(false),
        })),
        ("pull", [path, given @ ..]) => {
            let (url, rev) = target(path, given, false)?;
            let squash = squash.or(known(path)["squash"].as_bool()).unwrap_or(false);
            Some(json!({"kind": "pull", "dir": path, "url": url, "ref": rev, "squash": squash}))
        }
        ("push", [path, given @ ..]) => {
            let (url, rev) = target(path, given, true)?;
            Some(json!({"kind": "push", "dir": path, "url": url, "ref": rev}))
        }
        ("split", [path]) if branch.is_some() => {
            Some(json!({"kind": "split", "dir": path, "branch": branch}))
        }
        _ => return Err(bad()),
    };
    if let Some(op) = op {
        let r = command(&*front, json!({"op": "subtree", "action": op}))?;
        if json_out {
            println!("{}", json!({"ok": true, "outcome": r["outcome"]}));
        } else if r["outcome"] == "conflict" || r["outcome"] == "stopped" {
            println!(
                "{}",
                tr!("Merged with conflicts. Resolve them, then commit")
            );
        }
        return Ok(if r["outcome"] == "conflict" || r["outcome"] == "stopped" {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        });
    }
    if json_out {
        println!("{info}");
        return Ok(ExitCode::SUCCESS);
    }
    for s in info["subtrees"].as_array().into_iter().flatten() {
        println!("{}", line(s));
    }
    Ok(ExitCode::SUCCESS)
}

/// 1 つの subtree の行: フォルダ、覚えている相手（URL と ref）、最新の取り込みが squash なら `(squash)`。
fn line(s: &serde_json::Value) -> String {
    let mut line = s["dir"].as_str().unwrap_or_default().to_owned();
    if let (Some(url), Some(rev)) = (s["url"].as_str(), s["ref"].as_str()) {
        line.push_str(&format!("  {url} {rev}"));
    }
    if s["squashed"] == true {
        line.push_str("  (squash)");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_the_folder_and_the_remembered_source() {
        assert_eq!(line(&json!({"dir": "lib"})), "lib");
        let s = json!({"dir": "vendor/x", "url": "u", "ref": "main", "squashed": true});
        assert_eq!(line(&s), "vendor/x  u main  (squash)");
    }
}
