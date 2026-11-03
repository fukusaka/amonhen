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
    if args.first().map(String::as_str) != Some("create") {
        return Err(format!("{}\n\n{}", tr!("Only bundle create is supported"), usage()).into());
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
