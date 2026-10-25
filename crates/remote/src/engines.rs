//! 手元のエンジンの在りか: SSH 先へ送るエンジン（`amonhen-<os>-<arch>`）の置き場所と、開発中に共有の daemon に
//! するエンジン。アプリ（FFI 越し）と、エンジン（CLI・daemon）が同じ探し方をする。
//!
//! 開発中は、実行ファイルを作ったチェックアウト（workspace の根）の中だけを探す。上へたどり続けると、worktree
//! （`.claude/worktrees/<name>`）は本体のチェックアウトの、別のビルドのエンジンを拾ってしまう。

use std::path::{Path, PathBuf};

/// SSH 先へ送るエンジンの置き場所。`exe` は今動いている実行ファイル（アプリ・エンジン）。
/// `AMONHEN_ENGINES` → `.app` の `Contents/Resources/engines` → エンジンの置き場所の隣の `engines`（`amonhen install`
/// が置く）→ 開発中は、チェックアウトの `dist/engine`（`scripts/build-engine.sh` が作る。まだなくても、そこ）。
pub fn bundled_dir(exe: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("AMONHEN_ENGINES") {
        return PathBuf::from(dir);
    }
    let resolved = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let beside = ["../Resources/engines", "engines"].into_iter();
    if let Some(bundled) = beside
        .filter_map(|p| resolved.parent().map(|d| d.join(p)))
        .find(|d| d.is_dir())
    {
        return bundled;
    }
    checkout_root(&resolved)
        .map(|root| root.join("dist/engine"))
        .unwrap_or_else(|| PathBuf::from("dist/engine"))
}

/// 開発中に共有の daemon にするエンジン（チェックアウトの `target/release/amonhen`）。チェックアウトの中で
/// 動いていなければ・まだ作っていなければ None。
pub fn development_engine(exe: &Path) -> Option<PathBuf> {
    let resolved = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let name = if cfg!(windows) {
        "amonhen.exe"
    } else {
        "amonhen"
    };
    let engine = checkout_root(&resolved)?.join("target/release").join(name);
    engine.is_file().then_some(engine)
}

/// `path` を含むチェックアウト（`Cargo.lock` と `crates` のある、いちばん近いフォルダ）。`apps/gtk4` は
/// `Cargo.lock` を別に持つが `crates` がないので、その上の根まで進む。
pub fn checkout_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|d| d.join("Cargo.lock").is_file() && d.join("crates").is_dir())
        .map(Path::to_path_buf)
}

/// `path` のエンジンがこのビルドのものか（中に `BUILD_MARK` があるか）。
pub fn is_this_build(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|data| has_build_mark(&data))
}

/// `data`（エンジンの中身）に、このビルドの印があるか。
pub fn has_build_mark(data: &[u8]) -> bool {
    let mark = crate::BUILD_MARK.as_bytes();
    data.windows(mark.len()).any(|w| w == mark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_inside_the_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path();
        // 本体のチェックアウトと、その中の worktree（どちらも workspace の根）。
        for root in [main.to_path_buf(), main.join(".claude/worktrees/w")] {
            std::fs::create_dir_all(root.join("crates")).unwrap();
            std::fs::write(root.join("Cargo.lock"), "").unwrap();
        }
        std::fs::create_dir_all(main.join("dist/engine")).unwrap();
        std::fs::create_dir_all(main.join("target/release")).unwrap();
        std::fs::write(main.join("target/release/amonhen"), "").unwrap();
        let app = main.join(".claude/worktrees/w/apps/macos/.build/release/AmonHen");
        std::fs::create_dir_all(app.parent().unwrap()).unwrap();
        std::fs::write(&app, "").unwrap();

        let worktree = main.join(".claude/worktrees/w").canonicalize().unwrap();
        assert_eq!(
            checkout_root(&app.canonicalize().unwrap()),
            Some(worktree.clone())
        );
        // 本体の dist/engine・target/release/amonhen は拾わない。
        assert_eq!(bundled_dir(&app), worktree.join("dist/engine"));
        assert_eq!(development_engine(&app), None);
        let gtk = main.join("apps/gtk4");
        std::fs::create_dir_all(&gtk).unwrap();
        std::fs::write(gtk.join("Cargo.lock"), "").unwrap();
        assert_eq!(
            checkout_root(&gtk),
            Some(main.to_path_buf()),
            "apps/gtk4 has a Cargo.lock but no crates"
        );
    }

    #[test]
    fn finds_the_mark_of_this_build() {
        let mut data = b"\x7fELF...".to_vec();
        assert!(!has_build_mark(&data));
        data.extend_from_slice(crate::BUILD_MARK.as_bytes());
        assert!(has_build_mark(&data));
    }
}
