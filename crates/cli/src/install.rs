//! エンジンの置き場所（どの OS でも、どの FE でも同じ）。
//!
//! ```text
//! ${XDG_DATA_HOME:-~/.local/share}/amonhen/engine/<中身のハッシュ>/amonhen   エンジン（版ごとに並べて置ける）
//! ${XDG_DATA_HOME:-~/.local/share}/amonhen/current -> engine/<ハッシュ>       いちばん新しい版
//! ~/.local/bin/amonhen -> .../amonhen/current/amonhen                        端末の CLI
//! ```
//!
//! Windows では `%LOCALAPPDATA%\amonhen` に置き、エンジンは `amonhen.exe`。シンボリックリンクは権限が要るので、
//! `current` は `engine\<ハッシュ>` を書いたファイルにし、CLI は `bin\amonhen.exe` に写す（`bin` を PATH に足す）。
//!
//! アプリ・VSCode 拡張・tar.gz を展開した人は `amonhen install` で自分を置き、共有の daemon は `amonhen engine` が
//! 出すもの（`current` の通信の形の版が同じならそれ）で起動する。どの FE が先に起動しても、同じ daemon になる。
//! SSH 先にも同じ形で送る（`amonhen_remote::ssh`）。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use amonhen_model::tr;

use crate::Result;

/// エンジンの実行ファイルの名前（Windows は `amonhen.exe`）。
fn exe_name() -> String {
    format!("amonhen{}", std::env::consts::EXE_SUFFIX)
}

fn data_dir() -> Result<PathBuf> {
    if cfg!(windows) {
        let local =
            std::env::var_os("LOCALAPPDATA").ok_or_else(|| tr!("LOCALAPPDATA isn’t set"))?;
        return Ok(PathBuf::from(local).join("amonhen"));
    }
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(d).join("amonhen"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| tr!("HOME isn’t set"))?;
    Ok(PathBuf::from(home).join(".local/share/amonhen"))
}

fn bin_link() -> Result<PathBuf> {
    if cfg!(windows) {
        return Ok(data_dir()?.join("bin").join(exe_name()));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| tr!("HOME isn’t set"))?;
    Ok(PathBuf::from(home).join(".local/bin/amonhen"))
}

/// `current` が指すエンジンのフォルダー（Unix はリンク、Windows は `engine\<ハッシュ>` を書いたファイル）。
fn current_dir(root: &Path) -> Option<PathBuf> {
    let current = root.join("current");
    let target = if cfg!(windows) {
        PathBuf::from(std::fs::read_to_string(&current).ok()?.trim())
    } else {
        std::fs::read_link(&current).ok()?
    };
    Some(root.join(target))
}

/// `current` を `engine/<hash>` に向ける（書いてから名前を付け替えるので、途中で読まれても壊れない）。
fn set_current(root: &Path, hash: &str) -> Result<()> {
    let relative = PathBuf::from("engine").join(hash);
    let tmp = root.join(format!("current.tmp.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    if cfg!(windows) {
        std::fs::write(&tmp, relative.display().to_string())?;
    } else {
        symlink(&relative, &tmp)?;
    }
    std::fs::rename(&tmp, root.join("current"))?;
    Ok(())
}

/// `amonhen 0.1.1-snapshot.72+abc (build 0123456789abcdef)` を（版, ビルドの指紋）に。前の版の形
/// （`(protocol 16)`）は読まない（ビルドの指紋がないので、どのビルドとも違う）。
fn parse_version(line: &str) -> Option<(String, String)> {
    let rest = line.trim().strip_prefix("amonhen ")?;
    let (version, build) = rest.split_once(" (build ")?;
    Some((version.to_owned(), build.strip_suffix(')')?.to_owned()))
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
        Some(p) => (
            numbers,
            0,
            p.rsplit('.')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
        ),
    }
}

/// `path` のエンジンの版（動かして聞く）。
fn version_of(path: &Path) -> Option<(String, String)> {
    let out = amonhen_model::process::command(path)
        .arg("--version")
        .output()
        .ok()?;
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

fn own_version() -> (String, String) {
    parse_version(&amonhen_daemon::version()).expect("自分の版")
}

/// 共有の daemon として起動するエンジン: `current` が自分と同じビルドならそれ、なければ自分。
pub fn engine_for_daemon() -> Result<PathBuf> {
    let me = std::env::current_exe()?.canonicalize()?;
    // Unix は current のリンクを通したパスにする（リンクを向け直せば、次に起動する daemon も新しい版になる）。
    let root = data_dir()?;
    let current = if cfg!(windows) {
        current_dir(&root)
            .map(|d| d.join(exe_name()))
            .unwrap_or_default()
    } else {
        root.join("current").join(exe_name())
    };
    if current.is_file()
        && let Some((_, build)) = version_of(&current)
        && build == own_version().1
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
            other => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown argument: {argument}", argument = other),
                    crate::usage()
                )
                .into());
            }
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
    let target = dir.join(exe_name());
    if !target.is_file() {
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!("amonhen.tmp.{}", std::process::id()));
        std::fs::write(&tmp, &data)?;
        set_executable(&tmp)?;
        std::fs::rename(&tmp, &target)?;
        say(tr!("Installed: {path}", path = target.display()));
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
            let tmp = to.join(format!(
                "{}.tmp.{}",
                name.to_string_lossy(),
                std::process::id()
            ));
            std::fs::copy(entry.path(), &tmp)?;
            set_executable(&tmp)?;
            std::fs::rename(&tmp, to.join(&name))?;
        }
    }

    // current を向けるか: なければ向ける。あれば、そちらが新しいときだけ残す（同じ版の別ビルドなら新しく置いたほう）。
    let (mine, _) = own_version();
    let installed_dir = current_dir(&root);
    let installed = installed_dir
        .as_ref()
        .and_then(|d| version_of(&d.join(exe_name())));
    let points_here = installed_dir.as_ref().is_some_and(|d| *d == dir);
    let newer_installed = installed
        .as_ref()
        .is_some_and(|(v, _)| version_key(v) > version_key(&mine));
    if points_here {
        say(tr!(
            "current is already this version ({version})",
            version = mine
        ));
    } else if newer_installed && !force {
        let (v, _) = installed.unwrap();
        say(tr!(
            "Keeping current at the newer version {installed} (this is {version}; --force replaces it)",
            installed = v,
            version = mine
        ));
    } else {
        set_current(&root, &hash)?;
        say(tr!(
            "Set current to this version ({version})",
            version = mine
        ));
    }

    if link {
        link_cli(&root, &say)?;
    }
    Ok(ExitCode::SUCCESS)
}

/// 端末の CLI を current のエンジンに向ける。Unix は `~/.local/bin/amonhen` をリンクに、Windows は `bin\amonhen.exe` に
/// 写す（動いている間は上書きできないので、そのときはそのままにする）。
fn link_cli(root: &Path, say: &dyn Fn(String)) -> Result<()> {
    let bin = bin_link()?;
    let Some(engine) = current_dir(root).map(|d| d.join(exe_name())) else {
        return Ok(());
    };
    if cfg!(windows) {
        std::fs::create_dir_all(bin.parent().unwrap())?;
        let same = std::fs::read(&bin).ok() == std::fs::read(&engine).ok();
        if !same {
            match std::fs::copy(&engine, &bin) {
                Ok(_) => say(tr!(
                    "Copied to {path} (add {dir} to PATH to use it from a terminal)",
                    path = bin.display(),
                    dir = bin.parent().unwrap().display()
                )),
                Err(e) => say(tr!(
                    "Can’t write {path} (it may be running): {e}",
                    path = bin.display(),
                    e = e
                )),
            }
        }
        return Ok(());
    }
    let wanted = root.join("current").join(exe_name());
    match std::fs::symlink_metadata(&bin) {
        Ok(m) if !m.file_type().is_symlink() => {
            say(tr!(
                "Leaving {path} alone because it isn’t a link",
                path = bin.display()
            ));
        }
        Ok(_) if std::fs::read_link(&bin).is_ok_and(|p| p == wanted) => {}
        _ => {
            std::fs::create_dir_all(bin.parent().unwrap())?;
            let _ = std::fs::remove_file(&bin);
            symlink(&wanted, &bin)?;
            say(format!("{} → {}", bin.display(), wanted.display()));
        }
    }
    Ok(())
}

/// 自分と一緒に配られた、SSH 先に送るエンジンの置き場所（`.app` の `Contents/Resources/engines`、VSCode 拡張の
/// `bin`（自分も `amonhen-<os>-<arch>` の名前で並んでいる）、開発中はリポジトリの `dist/engine`）。
fn bundled_engines(me: &Path) -> Option<PathBuf> {
    let dir = me.parent()?;
    if me
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("amonhen-"))
    {
        return Some(dir.to_path_buf());
    }
    if let Some(bundled) = Some(dir.join("../Resources/engines")).filter(|d| d.is_dir()) {
        return Some(bundled);
    }
    dir.ancestors()
        .map(|d| d.join("dist/engine"))
        .find(|d| d.is_dir())
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
    Err(tr!("Links are Unix only").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_versions() {
        assert_eq!(
            parse_version("amonhen 0.1.1-snapshot.72+5de3fb4 (build 0123456789abcdef)\n"),
            Some((
                "0.1.1-snapshot.72+5de3fb4".into(),
                "0123456789abcdef".into()
            ))
        );
        assert_eq!(parse_version("amonhen 0.1.0 (protocol 16)\n"), None);
        let k = version_key;
        assert!(k("0.1.0") < k("0.1.1-snapshot.3"));
        assert!(k("0.1.1-snapshot.3") < k("0.1.1-snapshot.12"));
        assert!(k("0.1.1-snapshot.12+abc") < k("0.1.1"));
        assert!(k("0.1.1") < k("0.2.0"));
        assert_eq!(k("0.1.0+a"), k("0.1.0+b"));
    }
}
