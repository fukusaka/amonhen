//! 手元の共有の daemon のコアに任せる処理。つないだ側のコアが要求を作って `GitBackend::delegate` で送り、daemon の
//! コアが `Repository::serve_delegated` で実行して返す。History は daemon が永続キャッシュのファイルで渡す
//! （`GitBackend::prepared_history`）。
//!
//! 命令（操作・Undo・設定など）は daemon のコアで実行し、操作と記録を 1 か所にする。差分は任せない: アプリは
//! 色付けのためにファイルの中身を前後とも使うので、daemon で計算しても中身を全部送ることになり、linux で測ると
//! 1 回にまとめる分だけ遅いときがかえって遅くなった（p95 8.3 → 9.2 ms、最大 27 → 34 ms）。

use amonhen_model::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) enum Request {
    /// `Repository::run_json` の命令（JSON）。
    Command(String),
}

#[derive(Serialize, Deserialize)]
pub(crate) enum Reply {
    Command(String),
}

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(postcard::to_allocvec(value)?)
}

pub(crate) fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T> {
    Ok(postcard::from_bytes(bytes)?)
}
