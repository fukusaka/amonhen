//! `amonhen open`: リポジトリを Amon Hen のタブで開く（`code .` のように）。
//!
//! 手元のリポジトリはフォルダとして、SSH 先は `amonhen://open?location=…` の URL としてアプリに渡す
//! （アプリが起動していれば、そのウィンドウのタブに入る）。macOS だけ。

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use crate::Result;

pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut bundle = std::env::var("AMONHEN_BUNDLE_ID").unwrap_or_else(|_| "io.github.fukusaka.amonhen".into());
    let mut target: Option<&str> = None;
    let mut through_daemon = false;
    for arg in args {
        match arg.as_str() {
            "--snapshot" => bundle = "io.github.fukusaka.amonhen.snapshot".into(),
            // 手元のリポジトリを、共有の daemon 越しに開く。
            "--daemon" => through_daemon = true,
            s if s.starts_with('-') => return Err(format!("知らないオプション: {s}").into()),
            s => target = Some(s),
        }
    }
    let target = target.unwrap_or(".");
    if !cfg!(target_os = "macos") {
        return Err("アプリで開くのは macOS だけです".into());
    }
    let status = if target.starts_with("ssh://") || target.starts_with("daemon:") {
        Command::new("open").args(["-b", &bundle, &url_for(target)]).status()?
    } else if through_daemon {
        let root = crate::repository_root(&PathBuf::from(target))?;
        Command::new("open").args(["-b", &bundle, &url_for(&format!("daemon:{}", root.display()))]).status()?
    } else {
        let root = crate::repository_root(&PathBuf::from(target))?;
        Command::new("open").arg("-b").arg(&bundle).arg(&root).status()?
    };
    if !status.success() {
        return Err(format!("Amon Hen（{bundle}）を起動できません。/Applications に入っているか確かめてください").into());
    }
    Ok(ExitCode::SUCCESS)
}

/// SSH 先などの場所を渡す URL。
pub fn url_for(location: &str) -> String {
    let mut encoded = String::new();
    for b in location.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("amonhen://open?location={encoded}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_the_location() {
        assert_eq!(
            super::url_for("ssh://luna.local/~/prj/a b"),
            "amonhen://open?location=ssh%3A//luna.local/~/prj/a%20b"
        );
    }
}
