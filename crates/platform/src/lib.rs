//! FE から注入される OS の機能（メインスレッドの起こし方・認証情報・SSH・保存先）。
//!
//! コアは OS 固有のコードを持たず、必要な機能はすべてこのトレイト経由で使う。
//! 認証情報と SSH は、書き込み系の操作を実装するときに加える。

use std::path::PathBuf;
use std::sync::Arc;

/// FE のメインスレッドを起こす。何度呼ばれても、FE 側で 1 回の更新にまとめてよい。
///
/// macOS / iOS は `DispatchSource`（`.data_add`）、Windows は `PostMessage`、
/// Linux は GLib のメインループ、Android は Looper / Handler で実装する。
pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub trait Platform: Send + Sync {
    fn waker(&self) -> Waker;

    /// 永続キャッシュの置き場所（macOS なら `~/Library/Caches/<バンドル ID>`）。
    fn cache_dir(&self) -> PathBuf;
}
