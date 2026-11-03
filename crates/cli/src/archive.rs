//! `amonhen archive`: ある時点のファイルを zip・tar・tar.gz にまとめる（コアの archive を通すので、SSH 先の
//! リポジトリでも書くのは手元のファイル）。

use std::path::PathBuf;
use std::process::ExitCode;

use amonhen_model::tr;
use serde_json::json;

use crate::{Args, Result, command, open_repository, usage};

/// `-o` のファイル名から決める形（git archive と同じく、決まらなければ tar）。
fn format_of(file: &str) -> &'static str {
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        "zip"
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        "tar.gz"
    } else {
        "tar"
    }
}

/// `amonhen archive`。
pub fn archive(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut json_out = false;
    let mut format: Option<String> = None;
    let mut prefix = String::new();
    let mut output: Option<String> = None;
    let mut revs: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        if arg == "--" {
            // `--` の後はすべてパス（git archive と同じ）。
            paths.extend(a.rest.by_ref().cloned());
            break;
        }
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json_out = true,
            "--format" => format = Some(a.value("--format")?.to_owned()),
            "--prefix" => prefix = a.value("--prefix")?.to_owned(),
            "-o" | "--output" => output = Some(a.value("-o")?.to_owned()),
            s if s.starts_with("--format=") => format = Some(s["--format=".len()..].to_owned()),
            s if s.starts_with("--prefix=") => prefix = s["--prefix=".len()..].to_owned(),
            s if s.starts_with("--output=") => output = Some(s["--output=".len()..].to_owned()),
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
        return Err(format!("{}\n\n{}", tr!("Give one revision"), usage()).into());
    }
    let rev = revs.pop().unwrap_or_else(|| "HEAD".to_owned());
    let format = match (format, &output) {
        (Some(f), _) if f == "tgz" => "tar.gz".to_owned(),
        (Some(f), _) => f,
        (None, Some(file)) => format_of(file).to_owned(),
        (None, None) => "tar".to_owned(),
    };
    let front = open_repository(&dir)?;
    // partial clone なら、先に取る中身の数を言う（取るのはコアがまとめて）。
    let need = command(
        &*front,
        json!({"op": "archiveContents", "rev": rev, "paths": paths}),
    )?;
    let count = need["info"]["count"].as_u64().unwrap_or(0);
    if count > 0 {
        let more = if need["info"]["more"] == true {
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
    // 出力を選ばなければ git archive と同じく標準出力に出す。一時ファイルに書かせて出す（コアは daemon の中かもしれない）。
    let temp = output
        .is_none()
        .then(|| std::env::temp_dir().join(format!("amonhen-archive-{}", std::process::id())));
    let file = match (&temp, &output) {
        (Some(t), _) => t.to_string_lossy().into_owned(),
        (None, Some(o)) => std::path::absolute(o)?.to_string_lossy().into_owned(),
        (None, None) => unreachable!(),
    };
    let r = command(
        &*front,
        json!({
            "op": "archive",
            "rev": rev,
            "format": format,
            "prefix": prefix,
            "paths": paths,
            "file": file,
        }),
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
        println!("{}", r["info"]);
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_format_from_the_file_name_as_git_does() {
        assert_eq!(format_of("app.zip"), "zip");
        assert_eq!(format_of("app.TAR.GZ"), "tar.gz");
        assert_eq!(format_of("app.tgz"), "tar.gz");
        assert_eq!(format_of("app.tar"), "tar");
        assert_eq!(format_of("app"), "tar");
    }
}
