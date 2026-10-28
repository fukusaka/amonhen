//! テストの手助け（各クレートのテストが使う）。

use std::path::PathBuf;
use std::sync::Once;
use std::time::{Duration, SystemTime};

/// このプロセスと、ここから起動する git・CLI・daemon が、利用者の設定を読まないようにする: git の global・system の
/// 設定と、Amon Hen の設定（`settings.json`）。HOME・XDG_CONFIG_HOME（Windows は APPDATA も）をプロセスごとの空の
/// フォルダにし、`GIT_CONFIG_NOSYSTEM` を立て、`GIT_CONFIG_GLOBAL` を消す（`GIT_CONFIG_GLOBAL` で外すのは git 2.32
/// からなので、HOME で外す）。何度呼んでも 1 回だけ。テストが git を動かす前（テストの `git` の手助けなど）に呼ぶ。
/// HOME を自分で決めて起動するもの（SSH 先のつもりのエンジン）は、そちらが勝つ。
pub fn isolate_from_user_settings() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let home = home();
        let config = home.join(".config");
        std::fs::create_dir_all(&config).expect("the test home can be made");
        // SAFETY: 環境を変えるのはここだけで、1 回だけ。Rust の環境の読み書き（Command の起動も）はロックで
        // 守られている。テストは C の getenv を並べて呼ばない。
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config);
            #[cfg(windows)]
            std::env::set_var("APPDATA", &config);
            std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
            std::env::remove_var("GIT_CONFIG_GLOBAL");
        }
    });
}

/// `amonhen-test-home-<pid>`。前のテストが残した 1 日より古いものは消す。
fn home() -> PathBuf {
    let temp = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(&temp) {
        let day_ago = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
        for entry in entries.flatten() {
            let stale = entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("amonhen-test-home-"))
                && entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t < day_ago);
            if stale {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    temp.join(format!("amonhen-test-home-{}", std::process::id()))
}
