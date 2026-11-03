//! `amonhen bundle create`: コミットを 1 つのファイルにまとめる（コアの bundle を通すので、SSH 先のリポジトリでも
//! 書くのは手元のファイル）。

use std::path::PathBuf;
use std::process::ExitCode;

use amonhen_model::tr;
use serde_json::json;

use crate::{Args, Result, command, open_repository, usage};

/// 入れる ref と、受け取る側が持っているはずの基点（`git bundle create` と同じく `<base>..<ref>` と `^<base>`）。
fn split_revs(revs: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut refs, mut bases) = (Vec::new(), Vec::new());
    for rev in revs {
        if let Some(base) = rev.strip_prefix('^') {
            bases.push(base.to_owned());
        } else if let Some((base, tip)) = rev.split_once("..") {
            bases.push(if base.is_empty() { "HEAD" } else { base }.to_owned());
            refs.push(if tip.is_empty() { "HEAD" } else { tip }.to_owned());
        } else {
            refs.push(rev.clone());
        }
    }
    (refs, bases)
}

/// `amonhen bundle`。
pub fn bundle(args: &[String]) -> Result<ExitCode> {
    match args.first().map(String::as_str) {
        Some("create") => {}
        Some("fetch") => return fetch(&args[1..]),
        _ => {
            return Err(format!(
                "{}\n\n{}",
                tr!("Use bundle create or bundle fetch"),
                usage()
            )
            .into());
        }
    }
    let mut dir = PathBuf::from(".");
    let mut json_out = false;
    let mut all = false;
    let mut file: Option<String> = None;
    let mut revs: Vec<String> = Vec::new();
    let mut a = Args::new(&args[1..]);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json_out = true,
            "--all" => all = true,
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            _ if file.is_none() => file = Some(arg.clone()),
            _ => revs.push(arg.clone()),
        }
    }
    let Some(file) = file else {
        return Err(format!("{}\n\n{}", tr!("Give the bundle file"), usage()).into());
    };
    let (mut refs, bases) = split_revs(&revs);
    if all {
        refs.clear();
    } else if refs.is_empty() {
        return Err(format!(
            "{}\n\n{}",
            tr!("Give the branches or tags to put in the bundle, or --all"),
            usage()
        )
        .into());
    }
    let front = open_repository(&dir)?;
    let preview = command(
        &*front,
        json!({"op": "bundlePreview", "refs": refs, "bases": bases}),
    )?;
    let count = preview["info"]["contents"].as_u64().unwrap_or(0);
    if count > 0 && preview["info"]["beyondShallow"] != true {
        let more = if preview["info"]["more"] == true {
            "+"
        } else {
            ""
        };
        eprintln!(
            "{}",
            tr!(
                "Fetching {count}{more} file contents missing from this partial clone…",
                count = count,
                more = more
            )
        );
    }
    // `-` なら標準出力に出す。一時ファイルに書かせて出す（コアは daemon の中かもしれない）。
    let temp = (file == "-")
        .then(|| std::env::temp_dir().join(format!("amonhen-bundle-{}", std::process::id())));
    let path = match &temp {
        Some(t) => t.to_string_lossy().into_owned(),
        None => std::path::absolute(&file)?.to_string_lossy().into_owned(),
    };
    let r = command(
        &*front,
        json!({"op": "bundle", "refs": refs, "bases": bases, "file": path}),
    );
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
        let mut info = r["info"].clone();
        info["commits"] = preview["info"]["commits"].clone();
        println!("{info}");
    }
    Ok(ExitCode::SUCCESS)
}

/// `amonhen bundle fetch [--json] <file> [<name>]`: bundle のブランチを `refs/remotes/<name>/*` に取り込む（name の
/// 既定はファイルの名前から）。SSH 先のリポジトリにも、中身を送って取り込む。
fn fetch(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut json_out = false;
    let mut rest: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json_out = true,
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
    let (file, name) = match &rest[..] {
        [file] => (file, None),
        [file, name] => (file, Some(name.clone())),
        _ => return Err(format!("{}\n\n{}", tr!("Give the bundle file"), usage()).into()),
    };
    let file = std::path::absolute(file)?.to_string_lossy().into_owned();
    let front = open_repository(&dir)?;
    let check = command(&*front, json!({"op": "bundleCheck", "file": file}))?;
    let missing = check["info"]["missing"].as_u64().unwrap_or(0);
    if missing > 0 {
        return Err(tr!(
            "This repository lacks {count} commits the bundle needs (it has only the commits after them)",
            count = missing
        )
        .into());
    }
    let name = name.unwrap_or_else(|| {
        check["info"]["name"]
            .as_str()
            .unwrap_or("bundle")
            .to_owned()
    });
    let r = command(
        &*front,
        json!({"op": "fetchBundle", "file": file, "name": name}),
    )?;
    if json_out {
        println!("{}", r["info"]);
    } else {
        for r in r["info"]["refs"].as_array().into_iter().flatten() {
            println!("{}", r.as_str().unwrap_or_default());
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ranges_and_bases_as_git_bundle_does() {
        let revs = ["main", "v1..topic", "^old", "..next"].map(String::from);
        let (refs, bases) = split_revs(&revs);
        assert_eq!(refs, ["main", "topic", "next"]);
        assert_eq!(bases, ["v1", "old", "HEAD"]);
    }
}
