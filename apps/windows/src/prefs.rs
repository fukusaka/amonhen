//! 利用者の設定（GTK4 版の `settings.rs` の鍵のうち、Windows 版に機能のあるもの）。`%APPDATA%\amonhen\windows.json`
//! に置く（エンジンの `settings.json` の隣。GTK4 版の `gtk.ini` に当たる）。鍵の名前は macOS 版の UserDefaults と同じ。

use std::cell::RefCell;
use std::path::PathBuf;

use serde_json::{Map, Value};

/// マージエディタを開いたときに、間違えようのないコンフリクトを解く（既定は解く）。
pub const AUTO_MAGIC_MERGE: &str = "autoMagicMerge";
/// 自動の fetch の間隔（分。0 はしない。GTK4 版・macOS 版と同じ鍵）。
pub const AUTO_FETCH_MINUTES: &str = "autoFetchMinutes";
/// Working Copy の一覧の見せ方（`list`・`tree`・`flat`）。
pub const WORKING_COPY_LAYOUT: &str = "workingCopyLayout";
/// File History に merge コミットも出す（既定は出さない）。
pub const FILE_HISTORY_MERGES: &str = "fileHistoryMerges";

thread_local! {
    static VALUES: RefCell<Map<String, Value>> = RefCell::new(load());
}

fn path() -> Option<PathBuf> {
    amonhen_model::settings::path().map(|p| p.with_file_name("windows.json"))
}

fn load() -> Map<String, Value> {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

pub fn bool(key: &str) -> bool {
    VALUES
        .with(|v| v.borrow().get(key).and_then(Value::as_bool))
        .unwrap_or(key == AUTO_MAGIC_MERGE)
}

pub fn string(key: &str) -> Option<String> {
    VALUES.with(|v| {
        v.borrow()
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

/// 書いて保存する（書けなければ理由）。
pub fn set_bool(key: &str, value: bool) -> Result<(), String> {
    set(key, Value::Bool(value))
}

pub fn set_string(key: &str, value: &str) -> Result<(), String> {
    set(key, Value::String(value.to_owned()))
}

/// 鍵ごと消す（なければ何もしない）。
pub fn remove(key: &str) -> Result<(), String> {
    if VALUES.with(|v| !v.borrow().contains_key(key)) {
        return Ok(());
    }
    write(|v| {
        v.remove(key);
    })
}

fn set(key: &str, value: Value) -> Result<(), String> {
    write(|v| {
        v.insert(key.to_owned(), value);
    })
}

fn write(change: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let text = VALUES.with(|v| {
        let mut v = v.borrow_mut();
        change(&mut v);
        serde_json::to_string_pretty(&*v).unwrap_or_default()
    });
    let path = path().ok_or("no settings folder")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// コミットのメッセージの下書きを覚える鍵（worktree ごと。GTK4 版と同じ形）。
pub fn draft_key(workdir: &std::path::Path) -> String {
    let hex: String = workdir
        .display()
        .to_string()
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("commitDraft.{hex}")
}

/// 自動の fetch の間隔（分。0 はしない）。
pub fn auto_fetch_minutes() -> u32 {
    string(AUTO_FETCH_MINUTES)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}
