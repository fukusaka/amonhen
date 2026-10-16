//! エンジンの置き場所（どの OS でも、どの FE でも同じ）。
//!
//! ```text
//! ${XDG_DATA_HOME:-~/.local/share}/amonhen/engine/<中身のハッシュ>/amonhen   エンジン（版ごとに並べて置ける）
//! ${XDG_DATA_HOME:-~/.local/share}/amonhen/current -> engine/<ハッシュ>       いちばん新しい版
//! ~/.local/bin/amonhen -> .../amonhen/current/amonhen                        端末の CLI
//! ```
//!
//! アプリ・VSCode 拡張・tar.gz を展開した人は `amonhen install` で自分を置き、共有の daemon は `amonhen engine` が
//! 出すもの（`current` の通信の形の版が同じならそれ）で起動する。どの FE が先に起動しても、同じ daemon になる。
//! SSH 先にも同じ形で送る（`amonhen_remote::ssh`）。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::Result;

fn data_dir() -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(d).join("amonhen"));
    }
    let home = std::env::var_os("HOME").ok_or("HOME がありません")?;
    Ok(PathBuf::from(home).join(".local/share/amonhen"))
}

fn bin_link() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or("HOME がありません")?;
    Ok(PathBuf::from(home).join(".local/bin/amonhen"))
}

/// `amonhen 0.1.1-snapshot.72+abc (protocol 16)` を（版, 通信の形の版）に。
fn parse_version(line: &str) -> Option<(String, u32)> {
    let rest = line.trim().strip_prefix("amonhen ")?;
    let (version, protocol) = rest.split_once(" (protocol ")?;
    Some((version.to_owned(), protocol.strip_suffix(')')?.parse().ok()?))
}

/// 版の大小。`0.1.0` < `0.1.1-snapshot.3` < `0.1.1-snapshot.12` < `0.1.1`（`+` の後ろは見ない）。
fn version_key(version: &str) -> (Vec<u64>, u8, u64) {
    let version = version.split('+').next().unwrap_or_default();
    let (core, pre) = match version.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (version, None),
    };
    let numbers = core.split('.').map(|n| n.parse().unwrap_or(0)).collect();
    match pre {
        None => (numbers, 1, 0),
        Some(p) => (numbers, 0, p.rsplit('.').next().and_then(|n| n.parse().ok()).unwrap_or(0)),
    }
}

/// `path` のエンジンの版（動かして聞く）。
fn version_of(path: &Path) -> Option<(String, u32)> {
    let out = std::process::Command::new(path).arg("--version").output().ok()?;
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

fn own_version() -> (String, u32) {
    parse_version(&amonhen_daemon::version()).expect("自分の版")
}

/// 共有の daemon として起動するエンジン: `current` の通信の形の版が自分と同じならそれ、なければ自分。
pub fn engine_for_daemon() -> Result<PathBuf> {
    let me = std::env::current_exe()?.canonicalize()?;
    let current = data_dir()?.join("current/amonhen");
    if current.is_file()
        && let Some((_, protocol)) = version_of(&current)
        && protocol == own_version().1
    {
        return Ok(current);
    }
    Ok(me)
}

/// `amonhen engine`: 共有の daemon として起動するエンジンの場所を出す（アプリ・VSCode 拡張が使う）。
pub fn engine(_args: &[String]) -> Result<ExitCode> {
    println!("{}", engine_for_daemon()?.display());
    Ok(ExitCode::SUCCESS)
}

/// `amonhen install [--quiet] [--no-link] [--force]`: 自分をエンジンの置き場所に置き、今の `current` より古くなければ
/// `current` を自分に向け、`~/.local/bin/amonhen` を張る。
pub fn install(args: &[String]) -> Result<ExitCode> {
    let (mut quiet, mut link, mut force) = (false, true, false);
    for a in args {
        match a.as_str() {
            "--quiet" | "-q" => quiet = true,
            "--no-link" => link = false,
            "--force" => force = true,
            other => return Err(format!("知らない引数: {other}\n\n{}", crate::USAGE).into()),
        }
    }
    let say = |s: String| {
        if !quiet {
            println!("{s}");
        }
    };
    let me = std::env::current_exe()?.canonicalize()?;
    let data = std::fs::read(&me)?;
    let hash = sha1_smol::Sha1::from(&data).digest().to_string()[..16].to_owned();
    let root = data_dir()?;
    let dir = root.join("engine").join(&hash);
    let target = dir.join("amonhen");
    if !target.is_file() {
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!("amonhen.tmp.{}", std::process::id()));
        std::fs::write(&tmp, &data)?;
        set_executable(&tmp)?;
        std::fs::rename(&tmp, &target)?;
        say(format!("置きました: {}", target.display()));
    }
    let _ = std::fs::write(dir.join("used"), b"");
    // SSH 先に送るほかの OS と CPU のエンジン（.app の Resources/engines、開発中は dist/engine）も一緒に置く。ここから
    // 起動した daemon が、SSH 先に送れるように。
    if let Some(source) = bundled_engines(&me) {
        let to = dir.join("engines");
        for entry in std::fs::read_dir(&source)?.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("amonhen-") || to.join(&name).is_file() {
                continue;
            }
            std::fs::create_dir_all(&to)?;
            let tmp = to.join(format!("{}.tmp.{}", name.to_string_lossy(), std::process::id()));
            std::fs::copy(entry.path(), &tmp)?;
            set_executable(&tmp)?;
            std::fs::rename(&tmp, to.join(&name))?;
        }
    }

    // current を向けるか: なければ向ける。あれば、そちらが新しいときだけ残す（同じ版の別ビルドなら新しく置いたほう）。
    let current = root.join("current");
    let (mine, _) = own_version();
    let installed = std::fs::read_link(&current).ok().and_then(|_| version_of(&current.join("amonhen")));
    let points_here = std::fs::read_link(&current).is_ok_and(|p| root.join(&p) == dir || p == dir);
    let newer_installed = installed
        .as_ref()
        .is_some_and(|(v, _)| version_key(v) > version_key(&mine));
    if points_here {
        say(format!("current はこの版です（{mine}）"));
    } else if newer_installed && !force {
        let (v, _) = installed.unwrap();
        say(format!("current は新しい版（{v}）のままにします（この版は {mine}。--force で入れ替える）"));
    } else {
        let tmp = root.join(format!("current.tmp.{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        symlink(&PathBuf::from("engine").join(&hash), &tmp)?;
        std::fs::rename(&tmp, &current)?;
        say(format!("current をこの版（{mine}）にしました"));
    }

    if link {
        let bin = bin_link()?;
        let wanted = current.join("amonhen");
        match std::fs::symlink_metadata(&bin) {
            Ok(m) if !m.file_type().is_symlink() => {
                say(format!("{} はリンクではないので、そのままにします", bin.display()));
            }
            Ok(_) if std::fs::read_link(&bin).is_ok_and(|p| p == wanted) => {}
            _ => {
                std::fs::create_dir_all(bin.parent().unwrap())?;
                let _ = std::fs::remove_file(&bin);
                symlink(&wanted, &bin)?;
                say(format!("{} → {}", bin.display(), wanted.display()));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// 自分と一緒に配られた、SSH 先に送るエンジンの置き場所（`.app` の `Contents/Resources/engines`、VSCode 拡張の
/// `bin`（自分も `amonhen-<os>-<arch>` の名前で並んでいる）、開発中はリポジトリの `dist/engine`）。
fn bundled_engines(me: &Path) -> Option<PathBuf> {
    let dir = me.parent()?;
    if me.file_name().is_some_and(|n| n.to_string_lossy().starts_with("amonhen-")) {
        return Some(dir.to_path_buf());
    }
    if let Some(bundled) = Some(dir.join("../Resources/engines")).filter(|d| d.is_dir()) {
        return Some(bundled);
    }
    dir.ancestors().map(|d| d.join("dist/engine")).find(|d| d.is_dir())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn symlink(original: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(original, link)?;
    Ok(())
}

#[cfg(not(unix))]
fn symlink(_original: &Path, _link: &Path) -> Result<()> {
    Err("リンクは Unix だけです".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_versions() {
        assert_eq!(parse_version("amonhen 0.1.1-snapshot.72+5de3fb4 (protocol 16)\n"), Some(("0.1.1-snapshot.72+5de3fb4".into(), 16)));
        let k = version_key;
        assert!(k("0.1.0") < k("0.1.1-snapshot.3"));
        assert!(k("0.1.1-snapshot.3") < k("0.1.1-snapshot.12"));
        assert!(k("0.1.1-snapshot.12+abc") < k("0.1.1"));
        assert!(k("0.1.1") < k("0.2.0"));
        assert_eq!(k("0.1.0+a"), k("0.1.0+b"));
    }
}
