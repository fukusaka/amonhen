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
    /// 署名の種類（`SIGNED_*`）。署名がなければ 0。検証の結果ではない（`GitBackend::verify_signatures`）。
    pub signature: u8,
}

impl CommitText {
    pub const SIGNED_GPG: u8 = 1;
    pub const SIGNED_SSH: u8 = 2;
    pub const SIGNED_X509: u8 = 3;

    /// `gpgsig` ヘッダーの値（の 1 行目）から署名の種類を決める。
    pub fn signature_kind(gpgsig: &[u8]) -> u8 {
        let has = |needle: &[u8]| gpgsig.windows(needle.len()).any(|w| w == needle);
        if has(b"SSH SIGNATURE") {
            Self::SIGNED_SSH
        } else if has(b"SIGNED MESSAGE") {
            Self::SIGNED_X509
        } else {
            Self::SIGNED_GPG
        }
    }

    pub fn subject(&self) -> &[u8] {
        &self.raw[self.subject.start as usize..self.subject.end as usize]
    }

    pub fn author_name(&self) -> &[u8] {
        &self.raw[self.author_name.start as usize..self.author_name.end as usize]
    }
}
