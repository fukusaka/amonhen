use std::sync::Arc;

use amonhen_layout::StrRef;
use amonhen_model::CommitText;

use crate::{ColumnSet, FrontendProfile, StringEncoding};

enum Buffer {
    Utf8(Vec<u8>),
    Utf16(Vec<u16>),
    Commit(Arc<Vec<u8>>),
}

impl Buffer {
    fn as_ptr(&self) -> *const u8 {
        match self {
            Buffer::Utf8(v) => v.as_ptr(),
            Buffer::Utf16(v) => v.as_ptr().cast(),
            Buffer::Commit(v) => v.as_ptr(),
        }
    }
}

/// History の連続した行の、FE 向けの列。
///
/// 文字列置き場の 0 番は変換した文字列（UTF-16 の FE なら ASCII 以外、UTF-8 の FE なら
/// 不正なバイト列を置き換えたもの）。1 番以降は各コミットの本文のバッファで、ASCII の文字列
/// （UTF-8 の FE なら正しい UTF-8 の文字列すべて）はここを直接指す。
pub struct HistoryChunk {
    pub start_row: u32,
    pub subjects: Vec<StrRef>,
    pub author_names: Vec<StrRef>,
    pub author_times: Vec<i64>,
    buffers: Vec<Buffer>,
    bases: Vec<*const u8>,
}

// `bases` は `buffers` が持つ、変更されないメモリを指すだけ。
unsafe impl Send for HistoryChunk {}
unsafe impl Sync for HistoryChunk {}

impl HistoryChunk {
    pub fn new(start_row: u32, texts: &[CommitText], profile: &FrontendProfile) -> Self {
        let mut converted = Converted::new(profile.string);
        let mut buffers = Vec::with_capacity(texts.len() + 1);
        buffers.push(Buffer::Utf8(Vec::new())); // 0 番は最後に差し替える
        let mut column = |wanted: ColumnSet, part: fn(&CommitText) -> &[u8]| -> Vec<StrRef> {
            if !profile.columns.contains(wanted) {
                return Vec::new();
            }
            texts
                .iter()
                .enumerate()
                .map(|(i, text)| converted.add(1 + i as u32, &text.raw, part(text)))
                .collect()
        };
        let subjects = column(ColumnSet::SUBJECT, CommitText::subject);
        let author_names = column(ColumnSet::AUTHOR, CommitText::author_name);
        let author_times = if profile.columns.contains(ColumnSet::COMMIT_TIME) {
            texts.iter().map(|t| t.author_time).collect()
        } else {
            Vec::new()
        };

        buffers[0] = converted.into_buffer();
        buffers.extend(texts.iter().map(|t| Buffer::Commit(t.raw.clone())));
        let bases = buffers.iter().map(Buffer::as_ptr).collect();
        Self {
            start_row,
            subjects,
            author_names,
            author_times,
            buffers,
            bases,
        }
    }

    pub fn len(&self) -> usize {
        self.buffers.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 文字列置き場の先頭アドレス。`StrRef::buf` で引く。
    pub fn bases(&self) -> &[*const u8] {
        &self.bases
    }

    /// `s` のバイト列。ASCII なら 1 バイト、ASCII 以外は FE の符号化の符号単位で `len()` 個。
    pub fn bytes(&self, s: StrRef) -> &[u8] {
        let unit = match (&self.buffers[s.buf as usize], s.is_ascii()) {
            (Buffer::Utf16(_), false) => 2,
            _ => 1,
        };
        let base = self.bases[s.buf as usize];
        // `bases` は `buffers` の中身を指し、`s` はその範囲内で作ったもの。
        unsafe { std::slice::from_raw_parts(base.add(s.off as usize), s.len() as usize * unit) }
    }
}

/// 変換が要る文字列を詰める 0 番の文字列置き場。
enum Converted {
    Utf8(Vec<u8>),
    Utf16(Vec<u16>),
}

impl Converted {
    fn new(encoding: StringEncoding) -> Self {
        match encoding {
            StringEncoding::Utf8 => Self::Utf8(Vec::new()),
            StringEncoding::Utf16 => Self::Utf16(Vec::new()),
        }
    }

    fn add(&mut self, buf: u32, raw: &[u8], part: &[u8]) -> StrRef {
        let off = (part.as_ptr() as usize - raw.as_ptr() as usize) as u32;
        if part.is_ascii() {
            return StrRef::ascii(buf, off, part.len() as u32);
        }
        match self {
            Self::Utf8(out) => match std::str::from_utf8(part) {
                Ok(_) => StrRef::non_ascii(buf, off, part.len() as u32),
                Err(_) => {
                    let start = out.len() as u32;
                    out.extend_from_slice(String::from_utf8_lossy(part).as_bytes());
                    StrRef::non_ascii(0, start, out.len() as u32 - start)
                }
            },
            Self::Utf16(out) => {
                let start = out.len();
                out.extend(String::from_utf8_lossy(part).encode_utf16());
                StrRef::non_ascii(0, (start * 2) as u32, (out.len() - start) as u32)
            }
        }
    }

    fn into_buffer(self) -> Buffer {
        match self {
            Self::Utf8(v) => Buffer::Utf8(v),
            Self::Utf16(v) => Buffer::Utf16(v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(raw: &[u8], subject: std::ops::Range<u32>, author: std::ops::Range<u32>) -> CommitText {
        CommitText {
            raw: Arc::new(raw.to_vec()),
            subject,
            author_name: author,
            author_time: 42,
        }
    }

    fn utf16(chunk: &HistoryChunk, s: StrRef) -> String {
        let units: Vec<u16> = chunk
            .bytes(s)
            .chunks_exact(2)
            .map(|c| u16::from_ne_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).unwrap()
    }

    #[test]
    fn ascii_points_into_the_commit_and_the_rest_is_converted_once() {
        let a = text(b"xx hello yy Alice", 3..8, 12..17);
        let jp = "件名です".as_bytes();
        let mut raw = b"zz ".to_vec();
        raw.extend_from_slice(jp);
        raw.extend_from_slice(b" Bob");
        let b = text(
            &raw,
            3..3 + jp.len() as u32,
            raw.len() as u32 - 3..raw.len() as u32,
        );
        let texts = [a, b];

        let chunk = HistoryChunk::new(100, &texts, &FrontendProfile::MACOS);
        assert_eq!(chunk.len(), 2);
        assert_eq!(chunk.author_times, [42, 42]);

        let s0 = chunk.subjects[0];
        assert!(s0.is_ascii());
        assert_eq!(s0.buf, 1);
        assert_eq!(chunk.bytes(s0), b"hello");
        // コピーせずにコミットのバッファを指している。
        assert_eq!(chunk.bytes(s0).as_ptr(), texts[0].subject().as_ptr());

        let s1 = chunk.subjects[1];
        assert!(!s1.is_ascii());
        assert_eq!(s1.buf, 0);
        assert_eq!(utf16(&chunk, s1), "件名です");
        assert_eq!(chunk.bytes(chunk.author_names[1]), b"Bob");
    }

    #[test]
    fn utf8_frontends_get_every_valid_string_without_copying() {
        let jp = "件名".as_bytes();
        let t = text(jp, 0..jp.len() as u32, 0..0);
        let profile = FrontendProfile {
            string: StringEncoding::Utf8,
            ..FrontendProfile::MACOS
        };
        let chunk = HistoryChunk::new(0, std::slice::from_ref(&t), &profile);
        let s = chunk.subjects[0];
        assert_eq!(s.buf, 1);
        assert_eq!(chunk.bytes(s).as_ptr(), t.subject().as_ptr());
    }

    #[test]
    fn unselected_columns_are_left_empty() {
        let t = text(b"a b", 0..1, 2..3);
        let profile = FrontendProfile {
            columns: ColumnSet::SUBJECT,
            ..FrontendProfile::MACOS
        };
        let chunk = HistoryChunk::new(0, std::slice::from_ref(&t), &profile);
        assert_eq!(chunk.subjects.len(), 1);
        assert!(chunk.author_names.is_empty() && chunk.author_times.is_empty());
    }
}
