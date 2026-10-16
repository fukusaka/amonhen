//! SSH 越し（や別プロセス）のリポジトリ。daemon（`amonhend`）は手元の `GitBackend` を要求に応じて実行し、
//! 手元は `RemoteBackend` を通してそれを使う。コアのレイアウト・キャッシュ・差分・色付けは手元で動くので、
//! 通信に乗るのは git に触れる所（新しいコミット・本文・blob・操作）だけになる。

mod client;
pub mod frame;
pub mod local;
pub mod location;
mod server;
#[cfg(unix)]
pub mod shm;
pub mod ssh;
pub mod wire;

pub use client::{Connection, RemoteBackend};
pub use server::{Host, Server};
pub use wire::{Notification, PROTOCOL_VERSION};
