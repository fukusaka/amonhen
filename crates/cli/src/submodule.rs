//! `amonhen submodule`: submodule の一覧と、足す・取り出す・外す・URL や追うブランチを変える・中のリポジトリの
//! データを `.git/modules` に移す・それぞれの中で命令を動かす（`git submodule`）。

use std::path::PathBuf;
use std::process::ExitCode;

use amonhen_model::tr;
use serde_json::json;

use crate::{Args, Result, command, open_repository, usage};

/// `amonhen submodule [list|add|update|remove|set-url|set-branch|sync]`。
pub fn submodule(args: &[String]) -> Result<ExitCode> {
    let (action, args) = match args.first().map(String::as_str) {
        Some(a) if !a.starts_with('-') => (a, &args[1..]),
        _ => ("list", args),
    };
    let mut dir = PathBuf::from(".");
    let (mut json_out, mut recursive, mut remote, mut delete_git_dir, mut default) =
        (false, false, false, false, false);
    let mut branch: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json_out = true,
            "--recursive" => recursive = true,
            "--remote" => remote = true,
            "--delete-git-dir" => delete_git_dir = true,
            "--default" => default = true,
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
            tr!("Unknown submodule command: {command}", command = action),
            usage()
        )
        .into()
    };
    let change = match (action, &rest[..]) {
        ("list", []) => None,
        ("update", _) => {
            let front = open_repository(&dir)?;
            command(
                &*front,
                json!({"op": "updateSubmodules", "paths": rest, "recursive": recursive, "remote": remote}),
            )?;
            return Ok(ExitCode::SUCCESS);
        }
        ("add", [url, path]) => {
            Some(json!({"kind": "add", "url": url, "path": path, "branch": branch}))
        }
        ("remove", [path]) => {
            Some(json!({"kind": "remove", "path": path, "deleteGitDir": delete_git_dir}))
        }
        ("set-url", [path, url]) => Some(json!({"kind": "setUrl", "path": path, "url": url})),
        ("set-branch", [path]) if branch.is_some() != default => {
            Some(json!({"kind": "setBranch", "path": path, "branch": branch}))
        }
        ("sync", []) => Some(json!({"kind": "sync", "recursive": recursive})),
        ("absorb", paths) => Some(json!({"kind": "absorb", "paths": paths})),
        ("foreach", words) if !words.is_empty() => {
            Some(json!({"kind": "foreach", "command": words.join(" "), "recursive": recursive}))
        }
        _ => return Err(bad()),
    };
    let front = open_repository(&dir)?;
    if let Some(change) = change {
        command(&*front, json!({"op": "changeSubmodule", "change": change}))?;
        return Ok(ExitCode::SUCCESS);
    }
    let r = command(&*front, json!({"op": "submodules"}))?;
    if json_out {
        println!("{}", r["info"]);
        return Ok(ExitCode::SUCCESS);
    }
    for s in r["info"]["submodules"].as_array().into_iter().flatten() {
        println!("{}", line(s));
    }
    Ok(ExitCode::SUCCESS)
}

/// 1 つの submodule の行: `git submodule status` と同じ印（`-`・`+`・`U`）、中に変更があれば `*`、場所、URL、追うブランチ。
fn line(s: &serde_json::Value) -> String {
    let mark = if s["conflict"] == true {
        'U'
    } else if s["initialized"] != true {
        '-'
    } else if s["moved"] == true {
        '+'
    } else {
        ' '
    };
    let changed = if s["changed"] == true { '*' } else { ' ' };
    let mut line = format!(
        "{mark}{changed} {}  {}",
        s["path"].as_str().unwrap_or_default(),
        s["url"].as_str().unwrap_or_default()
    );
    if let Some(branch) = s["branch"].as_str() {
        line.push_str(&format!(" ({branch})"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_submodules_as_git_submodule_status_does() {
        let s = json!({"path": "lib", "url": "u", "initialized": false});
        assert_eq!(line(&s), "-  lib  u");
        let s = json!({"path": "lib", "url": "u", "initialized": true, "moved": true, "changed": true, "branch": "main"});
        assert_eq!(line(&s), "+* lib  u (main)");
    }
}
