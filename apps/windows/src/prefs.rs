//! 利用者の設定（GTK4 版の `settings.rs` の鍵のうち、Windows 版に機能のあるもの）。`%APPDATA%\amonhen\windows.json`
//! に置く（エンジンの `settings.json` の隣。GTK4 版の `gtk.ini` に当たる）。鍵の名前は macOS 版の UserDefaults と同じ。

use std::cell::RefCell;
use std::path::PathBuf;

use serde_json::{Map, Value};

/// マージエディタを開いたときに、間違えようのないコンフリクトを解く（既定は解く）。
pub const AUTO_MAGIC_MERGE: &str = "autoMagicMerge";
/// リポジトリを開いたときに commit-graph を書き出す・書き足す（既定はしない。GTK4 版・macOS 版と同じ鍵）。
pub const WRITE_COMMIT_GRAPH: &str = "writeCommitGraph";
/// リポジトリを共有の daemon 越しに開く（SSH 先も。GTK4 版と同じ鍵）。
pub const OPEN_THROUGH_DAEMON: &str = "openThroughDaemon";
/// status で git の fsmonitor を使う（既定は使わない。GTK4 版・macOS 版と同じ鍵）。
pub const USE_FSMONITOR: &str = "useFSMonitor";
/// 自動の fetch の間隔（分。0 はしない。GTK4 版・macOS 版と同じ鍵）。
pub const AUTO_FETCH_MINUTES: &str = "autoFetchMinutes";
/// Working Copy の一覧の見せ方（`list`・`tree`・`flat`）。
pub const WORKING_COPY_LAYOUT: &str = "workingCopyLayout";
/// コミットの入力を件名の 1 行に畳んでいるか（macOS 版・GTK4 版と同じ鍵）。
pub const WORKING_COPY_COMMIT_COLLAPSED: &str = "workingCopyCommitCollapsed";
/// 一覧を Staged・Unstaged の節に分けるか（macOS 版・GTK4 版と同じ鍵）。
pub const WORKING_COPY_SECTIONS: &str = "workingCopySections";
/// Working Copy の差分で行を stage する形: "select"（選んでから hunk の見出しで）か "bar"（変更の帯・行番号を
/// 押して）。GTK4 版・macOS 版と同じ鍵。
pub const LINE_STAGE_STYLE: &str = "lineStageStyle";

/// 複数のファイルを並べた差分（History のコミット、Working Copy で選んだファイル）で、先頭から開いて並べる
/// ファイルの数（GTK4 版・macOS 版と同じ鍵と選べる数）。
pub const DIFF_OPEN_FILES: &str = "diffOpenFiles";
pub const DIFF_OPEN_FILES_CHOICES: [usize; 5] = [5, 10, 20, 50, 100];

pub fn diff_open_files() -> usize {
    string(DIFF_OPEN_FILES)
        .and_then(|s| s.parse().ok())
        .filter(|n| DIFF_OPEN_FILES_CHOICES.contains(n))
        .unwrap_or(20)
}

/// 行の Stage の仕方が「変更の帯・行番号を押して」か。
pub fn line_stage_bar() -> bool {
    string(LINE_STAGE_STYLE).as_deref() == Some("bar")
}
/// File History に merge コミットも出す（既定は出さない）。
pub const FILE_HISTORY_MERGES: &str = "fileHistoryMerges";
/// 開いたリポジトリを Repositories のブックマークに入れる（既定は入れる。GTK4 版・macOS 版と同じ鍵）。
pub const AUTO_BOOKMARK: &str = "autoBookmark";
/// History を 1 行で出す（View の One Line per Commit。GTK4 版・macOS 版と同じ鍵）。
pub const COMPACT_HISTORY_ROWS: &str = "compactHistoryRows";
/// 最後に閉じた窓のリポジトリ（引数なしで起動したら開く。GTK4 版・macOS 版の、開いていた窓を戻すのにあたる）。
pub const LAST_REPOSITORY: &str = "lastRepository";
/// 最後に閉じた窓のタブ（場所の並びと前のタブ。JSON）。
pub const LAST_TABS: &str = "lastTabs";
/// History でマージ済みブランチを折りたたむ（既定は折りたたまない。GTK4 版・macOS 版と同じ鍵）。
pub const FOLD_MERGED_BRANCHES: &str = "foldMergedBranches";
/// コミットの変更ファイルを差分の左に木で出す（既定は差分の見出しだけ。macOS 版・GTK4 版と同じ鍵）。
pub const HISTORY_FILE_TREE: &str = "historyFileTree";
/// 選んだコミットの祖先を強調する（既定は強調する。GTK4 版・macOS 版と同じ鍵）。
pub const HIGHLIGHT_ANCESTORS: &str = "highlightAncestors";
/// History の行に出す項目（`render::items` のビットを 10 進の文字で。GTK4 版・macOS 版と同じ鍵）。
pub const HISTORY_ITEMS: &str = "historyItems";

pub fn history_items() -> u32 {
    string(HISTORY_ITEMS)
        .and_then(|s| s.parse().ok())
        .unwrap_or(crate::render::items::ALL)
}
/// コミットの詳細に作者・コミッターのアバターを出す（既定は出す。Gravatar にメールアドレスのハッシュを送る。
/// GTK4 版・macOS 版と同じ鍵）。
pub const SHOW_AVATARS: &str = "showAvatars";
/// GitHub（gh で読む。Settings > Accounts）: History のコミットごとに CI の状態を出す（既定は出す）。
pub const GITHUB_CI_STATUS: &str = "gitHubCIStatus";
/// コミットの作者の GitHub のアバターを使う（既定は使う。使わなければ Gravatar だけ）。
pub const GITHUB_AVATARS: &str = "gitHubAvatars";
/// CI を読むブランチに、既定のブランチも含める（既定は含める。外すと今のブランチだけ）。
pub const GITHUB_CI_DEFAULT_BRANCH: &str = "gitHubCIDefaultBranch";
/// ブランチごとに読む直近のコミットの数（30・50・100。既定は 100）。
pub const GITHUB_CI_COMMITS: &str = "gitHubCICommits";
/// アバターの URL は、窓を開いて最初の 1 回だけ読む（既定はそうする）。
pub const GITHUB_AVATARS_ONCE: &str = "gitHubAvatarsOnce";

/// GitHub で、ブランチごとに読む直近のコミットの数。
pub fn github_ci_commits() -> u32 {
    string(GITHUB_CI_COMMITS)
        .and_then(|s| s.parse().ok())
        .filter(|n| [30, 50, 100].contains(n))
        .unwrap_or(100)
}

/// History の並び順（`date`・`topo`。GTK4 版・macOS 版と同じ鍵）。
pub const HISTORY_ORDER: &str = "historyOrder";

/// History の並び順（既定は日付順）。
pub fn history_order() -> amonhen_model::HistoryOrder {
    string(HISTORY_ORDER)
        .and_then(|s| amonhen_model::HistoryOrder::from_name(&s))
        .unwrap_or_default()
}

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
        .unwrap_or(
            key == AUTO_MAGIC_MERGE
                || key == SHOW_AVATARS
                || key == GITHUB_CI_STATUS
                || key == GITHUB_AVATARS
                || key == GITHUB_CI_DEFAULT_BRANCH
                || key == GITHUB_AVATARS_ONCE
                || key == AUTO_BOOKMARK
                || key == HIGHLIGHT_ANCESTORS,
        )
}

pub fn string(key: &str) -> Option<String> {
    VALUES.with(|v| {
        v.borrow()
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

/// 数（なければ・読めなければ None）。
pub fn number(key: &str) -> Option<f64> {
    string(key)?.parse().ok()
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
        // 窓ごとに別のプロセスなので、ほかの窓が書いた分を消さないよう、書く前に読み直す。
        *v = load();
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
