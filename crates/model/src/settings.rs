//! この機械の Amon Hen の設定（`settings.json`）。アプリ・CLI・共有の daemon・VSCode 拡張が同じものに従う。
//!
//! 置き場所は `$XDG_CONFIG_HOME/amonhen/settings.json`（なければ `~/.config/amonhen/settings.json`、Windows は
//! `%APPDATA%\amonhen\settings.json`）。git を動かす機械に置く: この Mac の分はこの Mac、SSH 先の分はその SSH 先
//! （SSH 先のエンジン・CLI・Remote-SSH の VSCode 拡張が読む）。中身は JSON のオブジェクトで、今は使う git（`"git"`:
//! 実行ファイルのパス）だけ。知らないキーは書き換えるときも残す。
//!
//! 使うたびに更新時刻を確かめて読み直すので、アプリで変えれば、CLI・daemon にも再起動なしで効く。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::Result;

/// 設定のファイル。決められなければ（HOME もないとき）None。
pub fn path() -> Option<PathBuf> {
    #[cfg(windows)]
    let dir = crate::process::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let dir = crate::process::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    dir.map(|d| d.join("amonhen").join("settings.json"))
}

/// 設定で選んだ git（実行ファイルのパス）。決めていなければ `Ok(None)`。ファイルが読めない（JSON でない）ときは
/// その理由。
pub fn git() -> Result<Option<String>, String> {
    let Some(path) = path() else { return Ok(None) };
    let stamp = std::fs::metadata(&path)
        .ok()
        .map(|m| (m.modified().ok(), m.len()));
    let mut cached = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = cached.as_ref()
        && c.path == path
        && c.stamp == stamp
    {
        return c.git.clone();
    }
    let git = match stamp {
        None => Ok(None),
        Some(_) => read(&path).map(|object| {
            object
                .get("git")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        }),
    };
    *cached = Some(Cached {
        path,
        stamp,
        git: git.clone(),
    });
    git
}

/// 使う git を設定に書く。`git` が None・空なら消す（自動: PATH の git）。ほかのキーは残す。
pub fn set_git(git: Option<&str>) -> Result<()> {
    let path = path()
        .ok_or_else(|| crate::tr!("Can’t tell where Amon Hen’s settings go (HOME isn’t set)"))?;
    let mut object = if path.exists() {
        read(&path).map_err(|e| -> crate::Error { e.into() })?
    } else {
        serde_json::Map::new()
    };
    match git.filter(|g| !g.is_empty()) {
        Some(g) => object.insert("git".into(), g.into()),
        None => object.remove("git"),
    };
    let dir = path.parent().expect("settings.json is in a folder");
    std::fs::create_dir_all(dir)?;
    let mut text = serde_json::to_string_pretty(&object)?;
    text.push('\n');
    // 途中で読まれても半端な中身を見せないよう、隣に書いてから置き換える。
    let temp = dir.join(format!("settings.json.tmp.{}", std::process::id()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path)?;
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(())
}

/// この機械（このプロセスが git を動かすところ）で使う git と、それをどこで決めたか。
pub fn machine_git() -> crate::MachineGit {
    use crate::process::GitChoice;
    let (program, source) = match crate::process::git_choice() {
        GitChoice::Environment(p) => (p, "environment"),
        GitChoice::Settings(p) => (p, "settings"),
        GitChoice::Fallback(p) => (p, "fallback"),
        GitChoice::Path => ("git".to_owned(), "path"),
    };
    crate::MachineGit {
        chosen: git().ok().flatten(),
        file: path().map(|p| p.display().to_string()),
        program,
        source: source.to_owned(),
        problem: crate::process::missing_git(),
    }
}

struct Cached {
    path: PathBuf,
    /// 読んだときの更新時刻と大きさ（ファイルがなければ None）。
    stamp: Option<(Option<SystemTime>, u64)>,
    git: Result<Option<String>, String>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

/// 人に見せるパス（ホームの下なら `~/…`）。
pub fn shown(path: &std::path::Path) -> String {
    match std::env::var_os("HOME").and_then(|h| path.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) if !rest.as_os_str().is_empty() => {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
        }
        _ => path.display().to_string(),
    }
}

fn read(path: &std::path::Path) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let unreadable = |reason: String| {
        crate::tr!(
            "Amon Hen’s settings ({path}) can’t be read: {reason}",
            path = shown(path),
            reason = reason
        )
    };
    let text = std::fs::read_to_string(path).map_err(|e| unreadable(e.to_string()))?;
    if text.trim().is_empty() {
        return Ok(serde_json::Map::new());
    }
    match serde_json::from_str(&text).map_err(|e| unreadable(e.to_string()))? {
        serde_json::Value::Object(object) => Ok(object),
        _ => Err(unreadable(crate::tr!("not a JSON object"))),
    }
}

/// テストで設定の置き場所を `dir` に替える、FE の環境（Windows は `%APPDATA%` を `AMONHEN_PASS_ENV` で足して渡す。
/// ほかは `XDG_CONFIG_HOME`）。
#[doc(hidden)]
pub fn config_vars(dir: &str) -> Vec<(String, String)> {
    let vars: &[(&str, &str)] = if cfg!(windows) {
        &[(crate::process::PASS_ENV, "APPDATA"), ("APPDATA", dir)]
    } else {
        &[("XDG_CONFIG_HOME", dir)]
    };
    vars.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_git_keeping_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let env = crate::process::ClientEnv::from_vars(config_vars(dir.path().to_str().unwrap()));
        crate::process::with_client_env(&env, || {
            let file = dir.path().join("amonhen").join("settings.json");
            assert_eq!(path(), Some(file.clone()));
            assert_eq!(git(), Ok(None), "no file is automatic");

            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, r#"{"other": 1}"#).unwrap();
            set_git(Some("/opt/git/bin/git")).unwrap();
            assert_eq!(git(), Ok(Some("/opt/git/bin/git".into())));
            let written: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(written["other"], 1, "other keys stay");

            // ほかのプロセス（アプリ）が書き換えれば、次に読むときに替わる。
            std::fs::write(&file, r#"{"git": "/usr/local/bin/git", "other": 1}"#).unwrap();
            assert_eq!(git(), Ok(Some("/usr/local/bin/git".into())));

            set_git(None).unwrap();
            assert_eq!(git(), Ok(None));

            std::fs::write(&file, "{").unwrap();
            assert!(git().unwrap_err().contains("settings.json"));
            assert!(
                set_git(Some("/x")).is_err(),
                "a broken file isn't overwritten"
            );
        });
    }
}
