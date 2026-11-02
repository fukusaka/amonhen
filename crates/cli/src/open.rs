//! `amonhen open`: リポジトリを Amon Hen のタブで開く（`code .` のように）。
//!
//! macOS では、手元のリポジトリはフォルダとして、SSH 先は `amonhen://open?location=…` の URL としてアプリに渡す
//! （アプリが起動していれば、そのウィンドウのタブに入る）。ほかの Unix では GTK4 のアプリ（`amonhen-gtk`）に場所を
//! 引数で渡す（アプリは 1 つだけ動き、起動していればそのアプリの窓のタブに入る）。

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use amonhen_model::tr;

use crate::Result;

// macOS の open を呼ぶだけ（Windows では呼ばない）なので、process::command は使わない
#[allow(clippy::disallowed_methods)]
pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut bundle =
        std::env::var("AMONHEN_BUNDLE_ID").unwrap_or_else(|_| "io.github.fukusaka.amonhen".into());
    let mut target: Option<&str> = None;
    let mut through_daemon = false;
    for arg in args {
        match arg.as_str() {
            "--snapshot" => bundle = "io.github.fukusaka.amonhen.snapshot".into(),
            // 手元のリポジトリを、共有の daemon 越しに開く。
            "--daemon" => through_daemon = true,
            s if s.starts_with('-') => {
                return Err(tr!("Unknown option: {option}", option = s).into());
            }
            s => target = Some(s),
        }
    }
    let target = target.unwrap_or(".");
    if !cfg!(target_os = "macos") {
        return open_gtk(target, through_daemon);
    }
    let status = if target.starts_with("ssh://") || target.starts_with("daemon:") {
        Command::new("open")
            .args(["-b", &bundle, &url_for(target)])
            .status()?
    } else if through_daemon {
        let root = crate::repository_root(&PathBuf::from(target))?;
        Command::new("open")
            .args([
                "-b",
                &bundle,
                &url_for(&format!("daemon:{}", root.display())),
            ])
            .status()?
    } else {
        let root = crate::repository_root(&PathBuf::from(target))?;
        Command::new("open")
            .arg("-b")
            .arg(&bundle)
            .arg(&root)
            .status()?
    };
    if !status.success() {
        return Err(tr!(
            "Can’t start Amon Hen ({bundle}). Check that it’s in /Applications",
            bundle = bundle
        )
        .into());
    }
    Ok(ExitCode::SUCCESS)
}

/// macOS 以外の Unix: GTK4 のアプリ（PATH の `amonhen-gtk`。`install.sh` が置く）を、開く場所を引数にして起動する。
/// アプリが動いていれば、その場所はそのアプリに渡り、起動したものはすぐ終わる。待たずに戻る。
fn open_gtk(target: &str, through_daemon: bool) -> Result<ExitCode> {
    if cfg!(windows) {
        return Err(tr!("Opening in the app works only on macOS").into());
    }
    let location = if target.starts_with("ssh://") || target.starts_with("daemon:") {
        target.to_owned()
    } else {
        let root = crate::repository_root(&PathBuf::from(target))?;
        if through_daemon {
            format!("daemon:{}", root.display())
        } else {
            root.display().to_string()
        }
    };
    amonhen_model::process::command("amonhen-gtk")
        .arg(&location)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| {
            tr!(
                "Can’t start Amon Hen (amonhen-gtk): {e}. Check that it’s installed",
                e = e
            )
        })?;
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
            super::url_for("ssh://dev.example/~/src/a b"),
            "amonhen://open?location=ssh%3A//dev.example/~/src/a%20b"
        );
    }
}
