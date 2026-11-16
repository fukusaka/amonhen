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
    /// 記録された作者のメール（`<` と `>` の間）。mailmap で名前を置き換えるのに使う。
    pub author_email: Range<u32>,
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

    /// タグのオブジェクトの署名の種類（`SIGNED_*`）。署名がなければ 0。タグの署名はメッセージの後ろに付く
    /// （git と同じく、行の頭の `-----BEGIN … SIGNATURE-----` などを探す）。
    pub fn tag_signature_kind(raw: &[u8]) -> u8 {
        let mut kind = 0;
        for line in raw.split(|&b| b == b'\n') {
            kind = match line {
                b"-----BEGIN PGP SIGNATURE-----" | b"-----BEGIN PGP MESSAGE-----" => {
                    Self::SIGNED_GPG
                }
                b"-----BEGIN SSH SIGNATURE-----" => Self::SIGNED_SSH,
                b"-----BEGIN SIGNED MESSAGE-----" => Self::SIGNED_X509,
                _ => continue,
            };
        }
        kind
    }

    pub fn subject(&self) -> &[u8] {
        &self.raw[self.subject.start as usize..self.subject.end as usize]
    }

    pub fn author_name(&self) -> &[u8] {
        &self.raw[self.author_name.start as usize..self.author_name.end as usize]
    }

    pub fn author_email(&self) -> &[u8] {
        &self.raw[self.author_email.start as usize..self.author_email.end as usize]
    }

    /// コミットのオブジェクトのヘッダー（メッセージの前の空行まで）とメッセージ。`raw` がコミットのオブジェクト
    /// でなければ（daemon から History の行の分だけを受け取ったもの）None。
    fn split_object(&self) -> Option<(&[u8], &[u8])> {
        let raw = self.raw.as_slice();
        if !raw.starts_with(b"tree ") {
            return None;
        }
        // 署名などの複数行のヘッダーは続きの行が空白で始まるので、最初の空行がヘッダーの終わり。
        match raw.windows(2).position(|w| w == b"\n\n") {
            Some(end) => Some((&raw[..end], &raw[end + 2..])),
            None => Some((raw, &[])),
        }
    }

    /// メッセージの全体（件名と本文）。`raw` がコミットのオブジェクトでなければ空。
    pub fn message(&self) -> &[u8] {
        self.split_object().map_or(&[], |(_, message)| message)
    }

    /// 記録されたコミッターの名前とメール。`raw` がコミットのオブジェクトでなければ None。
    pub fn committer_name_email(&self) -> Option<(&[u8], &[u8])> {
        let (headers, _) = self.split_object()?;
        let line = headers
            .split(|&b| b == b'\n')
            .find_map(|line| line.strip_prefix(b"committer "))?;
        // `名前 <メール> 時刻 時差`。
        let close = line.iter().rposition(|&b| b == b'>')?;
        let open = line[..close].iter().rposition(|&b| b == b'<')?;
        Some((line[..open].trim_ascii(), &line[open + 1..close]))
    }

    /// 作者の名前とメール。mailmap があれば置き換えたもの（`Mailmap::map`）。
    pub fn author<'a>(
        &'a self,
        mailmap: Option<&'a crate::Mailmap>,
    ) -> crate::mailmap::Identity<'a> {
        match mailmap {
            Some(m) => m.map(self.author_name(), self.author_email()),
            None => crate::mailmap::Identity {
                name: self.author_name().into(),
                email: self.author_email().into(),
            },
        }
    }
}
