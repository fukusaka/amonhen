//! 手元の共有の daemon のコアに任せる処理。つないだ側のコアが要求を作って `GitBackend::delegate` で送り、daemon の
//! コアが `Repository::serve_delegated` で実行して返す。History は daemon が永続キャッシュのファイルで渡す
//! （`GitBackend::prepared_history`）。
//!
//! - 命令（操作・Undo・設定など）は daemon のコアで実行し、操作と記録を 1 か所にする。
//! - 差分は daemon で blob を読んで計算し、ファイルの数に関わらず 1 回の往復で返す。ファイルの中身は答えの
//!   `parts` に分け、共有メモリで渡す（コピーしない。ソケットで送ると、大きなファイルでかえって遅くなった）。

use amonhen_model::{FileChange, Result};
use serde::{Deserialize, Serialize};

use crate::diff::{FileDiff, Line, Source};

#[derive(Serialize, Deserialize)]
pub(crate) enum Request {
    /// `Repository::run_json` の命令（JSON）。
    Command(String),
    /// 変更ごとの差分（`Repository::change_diff`）。
    ChangeDiffs(Vec<FileChange>),
}

#[derive(Serialize, Deserialize)]
pub(crate) enum Reply {
    Command(String),
    /// 差分の中身以外。i 番目の差分の前後の中身は、`parts` の 2i・2i+1 番。
    ChangeDiffs(Vec<DiffMeta>),
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DiffMeta {
    headers: String,
    /// kind・old_no・new_no・source・範囲の始め・終わり。
    lines: Vec<(u8, u32, u32, u8, u32, u32)>,
    binary: bool,
    added: u32,
    removed: u32,
}

impl From<&FileDiff> for DiffMeta {
    fn from(d: &FileDiff) -> Self {
        Self {
            headers: d.headers.clone(),
            lines: d
                .lines
                .iter()
                .map(|l| (l.kind, l.old_no, l.new_no, l.source as u8, l.range.start, l.range.end))
                .collect(),
            binary: d.binary,
            added: d.added,
            removed: d.removed,
        }
    }
}

impl DiffMeta {
    pub(crate) fn into_diff(self, old: amonhen_layout::Blob, new: amonhen_layout::Blob) -> FileDiff {
        FileDiff {
            old,
            new,
            headers: self.headers,
            lines: self
                .lines
                .into_iter()
                .map(|(kind, old_no, new_no, source, start, end)| Line {
                    kind,
                    old_no,
                    new_no,
                    source: match source {
                        0 => Source::Old,
                        1 => Source::New,
                        _ => Source::Header,
                    },
                    range: start..end,
                })
                .collect(),
            binary: self.binary,
            added: self.added,
            removed: self.removed,
        }
    }
}

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(postcard::to_allocvec(value)?)
}

pub(crate) fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T> {
    Ok(postcard::from_bytes(bytes)?)
}
