use std::ops::Range;
use std::sync::Arc;

/// コミットの本文。git オブジェクトを展開したバッファをそのまま持ち、件名や作者は位置で指す。
///
/// FE に渡すときも、ASCII の文字列はこのバッファを直接指す（コピーしない）。
#[derive(Clone, Debug)]
pub struct CommitText {
    pub raw: Arc<Vec<u8>>,
    /// メッセージの 1 行目（前後の空白を除く）。
    pub subject: Range<u32>,
    pub author_name: Range<u32>,
    pub author_time: i64,
}

impl CommitText {
    pub fn subject(&self) -> &[u8] {
        &self.raw[self.subject.start as usize..self.subject.end as usize]
    }

    pub fn author_name(&self) -> &[u8] {
        &self.raw[self.author_name.start as usize..self.author_name.end as usize]
    }
}
