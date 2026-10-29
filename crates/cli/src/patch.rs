//! パッチ: format-patch・am・apply。コアの命令（formatPatch・readPatch・am・amThreeWay・applyPatch）を通すので、
//! SSH 先のリポジトリでもパッチのファイルは手元に置いたまま。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use amonhen_front::Front;
use amonhen_model::tr;
use serde_json::{Value, json};

use crate::{Args, Result, command, open_repository, usage};

/// 一度に辿るコミットの数の上限（`A..B` が first-parent でつながっていないときに止めるため）。
const WALK_LIMIT: usize = 10_000;

fn absolute(path: &str) -> Result<String> {
    Ok(std::path::absolute(path)?.to_string_lossy().into_owned())
}

fn resolve(front: &dyn Front, rev: &str) -> Result<String> {
    let r = command(front, json!({"op": "resolveCommit", "rev": rev}))?;
    r["text"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| tr!("{rev} not found", rev = rev).into())
}

fn first_parent(front: &dyn Front, oid: &str) -> Result<Option<String>> {
    let r = command(front, json!({"op": "commitInfo", "oid": oid}))?;
    Ok(r["info"]["parents"][0].as_str().map(str::to_owned))
}

/// 書き出すコミット（古い順）。`git format-patch` と同じく、`<rev>` 1 つなら `<rev>..HEAD`、`-<n>` なら `<rev>`
/// （既定は HEAD）から n 個。first-parent で辿る。
fn commits(front: &dyn Front, rev: Option<&str>, count: Option<usize>) -> Result<Vec<String>> {
    // git と同じく、範囲も数も選ばなければ何も書き出さない。
    if rev.is_none() && count.is_none() {
        return Ok(Vec::new());
    }
    let (from, to) = match (rev, count) {
        (Some(r), None) => match r.split_once("..") {
            Some((a, b)) => (Some(a), if b.is_empty() { "HEAD" } else { b }),
            None => (Some(r), "HEAD"),
        },
        (r, _) => (None, r.unwrap_or("HEAD")),
    };
    // `<from>..<to>` は git と同じく、<from> から届かない（<to> から届く）コミット。そのうち first-parent の線の上の
    // ものを <to> から辿る。<from> が線の上になくても（枝分かれしていても）、分かれたところで止まる。
    let range = from
        .map(|f| -> Result<HashSet<String>> {
            let base = if f.is_empty() { "HEAD" } else { f };
            let r = command(
                front,
                json!({"op": "between", "base": base, "tip": to, "limit": WALK_LIMIT + 1}),
            )?;
            let commits = r["info"]["commits"].as_array().cloned().unwrap_or_default();
            if commits.len() > WALK_LIMIT {
                return Err(
                    tr!("Too many commits from {from} to {to}", from = base, to = to).into(),
                );
            }
            Ok(commits
                .iter()
                .filter_map(|c| c["oid"].as_str().map(str::to_owned))
                .collect())
        })
        .transpose()?;
    let mut out = Vec::new();
    let mut next = Some(resolve(front, to)?);
    while let Some(oid) = next {
        if range.as_ref().is_some_and(|r| !r.contains(&oid))
            || count.is_some_and(|n| out.len() >= n)
            || out.len() >= WALK_LIMIT
        {
            break;
        }
        next = first_parent(front, &oid)?;
        out.push(oid);
    }
    out.reverse();
    Ok(out)
}

/// `amonhen format-patch`。
pub fn format_patch(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut json_out = false;
    let mut output: Option<String> = None;
    let mut stdout = false;
    let mut mbox: Option<String> = None;
    let mut cover_letter = false;
    let mut reroll: Option<u32> = None;
    let mut count: Option<usize> = None;
    let mut revs: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json_out = true,
            "-o" | "--output-directory" => output = Some(a.value("-o")?.to_owned()),
            "--stdout" => stdout = true,
            "--mbox" => mbox = Some(a.value("--mbox")?.to_owned()),
            "--cover-letter" => cover_letter = true,
            "-v" | "--reroll-count" => {
                reroll = Some(
                    a.value("-v")?
                        .parse()
                        .map_err(|_| tr!("-v needs a number"))?,
                )
            }
            "-n" => {
                count = Some(
                    a.value("-n")?
                        .parse()
                        .map_err(|_| tr!("-n needs a number"))?,
                )
            }
            s if s.len() > 1
                && s.starts_with('-')
                && s[1..].bytes().all(|b| b.is_ascii_digit()) =>
            {
                count = s[1..].parse().ok();
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            _ => revs.push(arg.clone()),
        }
    }
    if revs.len() > 1 {
        return Err(format!("{}\n\n{}", tr!("Give one revision or range"), usage()).into());
    }
    let front = open_repository(&dir)?;
    let oids = commits(&*front, revs.first().map(String::as_str), count)?;
    if oids.is_empty() {
        return Err(tr!("No commits to save as patches").into());
    }
    let mut request = json!({
        "op": "formatPatch",
        "oids": oids,
        "coverLetter": cover_letter,
        "reroll": reroll,
    });
    // --stdout は一時ファイルに 1 つの mbox を書かせて、それを出す（コアは daemon の中かもしれない）。
    let temp = stdout.then(|| {
        std::env::temp_dir().join(format!("amonhen-format-patch-{}.mbox", std::process::id()))
    });
    match (&temp, &mbox) {
        (Some(t), _) => request["file"] = t.to_string_lossy().into_owned().into(),
        (None, Some(m)) => request["file"] = absolute(m)?.into(),
        (None, None) => request["dir"] = absolute(output.as_deref().unwrap_or("."))?.into(),
    }
    let r = command(&*front, request);
    if let Some(t) = &temp {
        let data = std::fs::read(t);
        let _ = std::fs::remove_file(t);
        r?;
        use std::io::Write;
        std::io::stdout().write_all(&data?)?;
        return Ok(ExitCode::SUCCESS);
    }
    let r = r?;
    if json_out {
        println!("{}", r["info"]);
    } else {
        // git と同じく、書いたファイルを 1 行に 1 つ（今のフォルダからの相対）。
        let here = std::env::current_dir().ok();
        for file in r["info"]["files"].as_array().into_iter().flatten() {
            let path = Path::new(file.as_str().unwrap_or_default());
            let shown = here
                .as_ref()
                .and_then(|h| path.strip_prefix(h).ok())
                .unwrap_or(path);
            println!("{}", shown.display());
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `amonhen am`。
pub fn am(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut json_out = false;
    let mut three_way: Option<bool> = None;
    let mut check = false;
    let mut step: Option<&str> = None;
    let mut files: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json_out = true,
            "-3" | "--3way" => three_way = Some(true),
            "--no-3way" => three_way = Some(false),
            "--check" => check = true,
            "--continue" => step = Some("continue"),
            "--skip" => step = Some("skip"),
            "--abort" => step = Some("abort"),
            "--retry-3way" => step = Some("amThreeWay"),
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            _ => files.push(absolute(arg)?),
        }
    }
    let front = open_repository(&dir)?;
    if check {
        return check_patches(&*front, &files, json_out);
    }
    let r = match step {
        Some(op) => command(&*front, json!({"op": op}))?,
        None if files.is_empty() => {
            return Err(format!("{}\n\n{}", tr!("Give the patch files"), usage()).into());
        }
        None => command(
            &*front,
            json!({"op": "am", "paths": files, "threeWay": three_way}),
        )?,
    };
    if step == Some("abort") {
        if !json_out {
            println!(
                "{}",
                tr!("Stopped applying the patches and went back to where it started")
            );
        }
        return Ok(ExitCode::SUCCESS);
    }
    report(&*front, &r, json_out)
}

/// `amonhen apply`。
pub fn apply(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut json_out = false;
    let mut index = false;
    let mut three_way = false;
    let mut check = false;
    let mut files: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json_out = true,
            "--index" => index = true,
            "-3" | "--3way" => three_way = true,
            "--check" => check = true,
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            _ => files.push(absolute(arg)?),
        }
    }
    if files.is_empty() {
        return Err(format!("{}\n\n{}", tr!("Give the patch files"), usage()).into());
    }
    let front = open_repository(&dir)?;
    if check {
        return check_patches(&*front, &files, json_out);
    }
    let r = command(
        &*front,
        json!({"op": "applyPatch", "paths": files, "index": index, "threeWay": three_way}),
    )?;
    if json_out {
        println!("{}", json!({"ok": true, "outcome": r["outcome"]}));
    } else if r["outcome"] == "conflict" {
        println!(
            "{}",
            tr!("Applied with conflicts. Resolve them, then stage the files")
        );
    } else {
        println!("{}", tr!("Applied (amonhen undo takes it back)"));
    }
    Ok(if r["outcome"] == "conflict" {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// パッチの中身と、そのまま当たるか。当たらなければ終了コードは 1。
fn check_patches(front: &dyn Front, files: &[String], json_out: bool) -> Result<ExitCode> {
    if files.is_empty() {
        return Err(format!("{}\n\n{}", tr!("Give the patch files"), usage()).into());
    }
    let r = command(front, json!({"op": "readPatch", "paths": files}))?;
    let info = &r["info"];
    if json_out {
        println!("{info}");
    } else {
        for patch in info["patches"].as_array().into_iter().flatten() {
            let subject = patch["subject"].as_str().unwrap_or_default();
            if !subject.is_empty() {
                let author = patch["author"].as_str().unwrap_or_default();
                println!("{subject}  ({author})");
            }
            for file in patch["files"].as_array().into_iter().flatten() {
                let path = file["path"].as_str().unwrap_or_default();
                let change = if file["binary"] == true {
                    "bin".to_owned()
                } else {
                    format!("+{} -{}", file["added"], file["removed"])
                };
                match file["oldPath"].as_str() {
                    Some(old) => println!("  {old} → {path}  {change}"),
                    None => println!("  {path}  {change}"),
                }
            }
        }
        match info["error"].as_str() {
            None => println!("{}", tr!("Applies cleanly")),
            Some(e) => println!("{}\n{e}", tr!("Doesn’t apply cleanly:")),
        }
    }
    Ok(if info["applies"] == true {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// am の結果。止まったら、どのパッチでなぜ止まったかと、次にできること。
fn report(front: &dyn Front, r: &Value, json_out: bool) -> Result<ExitCode> {
    let stopped = r["outcome"] != "done" && r["outcome"].is_string();
    let sequence = if stopped {
        Some(command(front, json!({"op": "sequence"}))?)
    } else {
        None
    };
    if json_out {
        let mut out = json!({"ok": true, "outcome": r["outcome"]});
        if let Some(s) = &sequence {
            out["sequence"] = s["info"].clone();
        }
        println!("{out}");
    } else if let Some(s) = &sequence {
        let info = &s["info"];
        let subject = info["step"]["subject"].as_str().unwrap_or_default();
        let current = info["current"].as_u64().unwrap_or(0);
        let total = info["total"].as_u64().unwrap_or(0);
        let why = if info["empty"] == true {
            tr!("The patch is empty. Skip it with amonhen am --skip")
        } else if r["outcome"] == "conflict" && info["threeWay"] != true && info["retried"] != true
        {
            tr!(
                "It doesn’t apply. amonhen am --retry-3way merges it with conflicts to resolve; or apply it by hand, then amonhen am --continue (or --skip, --abort)"
            )
        } else {
            tr!(
                "Resolve the conflicts and stage the files, then amonhen am --continue (or --skip, --abort)"
            )
        };
        println!(
            "{}\n{why}",
            tr!(
                "Stopped at patch {current} of {total}, “{subject}”.",
                current = current,
                total = total,
                subject = subject
            )
        );
    } else if r["outcome"].is_string() {
        println!("{}", tr!("Applied (amonhen undo takes it back)"));
    }
    Ok(if stopped {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
