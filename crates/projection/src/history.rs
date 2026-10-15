use amonhen_layout::StrRef;
use amonhen_model::CommitText;

use crate::{ColumnSet, FrontendProfile, StringTable, StringTableBuilder};

/// History の連続した行の、FE 向けの列。文字列置き場の 1 番以降は各コミットの本文。
pub struct HistoryChunk {
    pub start_row: u32,
    pub subjects: Vec<StrRef>,
    pub author_names: Vec<StrRef>,
    pub author_times: Vec<i64>,
    pub strings: StringTable,
}

impl HistoryChunk {
    pub fn new(start_row: u32, texts: &[CommitText], profile: &FrontendProfile) -> Self {
        let mut strings = StringTableBuilder::new(profile.string);
        for text in texts {
            strings.share(text.raw.clone());
        }
        let mut column = |wanted: ColumnSet, part: fn(&CommitText) -> &[u8]| -> Vec<StrRef> {
            if !profile.columns.contains(wanted) {
                return Vec::new();
            }
            texts
                .iter()
                .enumerate()
                .map(|(i, text)| strings.add(1 + i as u32, part(text)))
                .collect()
        };
        let subjects = column(ColumnSet::SUBJECT, CommitText::subject);
        let author_names = column(ColumnSet::AUTHOR, CommitText::author_name);
        let author_times = if profile.columns.contains(ColumnSet::COMMIT_TIME) {
            texts.iter().map(|t| t.author_time).collect()
        } else {
            Vec::new()
        };
        Self {
            start_row,
            subjects,
            author_names,
            author_times,
            strings: strings.finish(),
        }
    }

    pub fn len(&self) -> usize {
        self.strings.bases().len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bases(&self) -> &[*const u8] {
        self.strings.bases()
    }

    pub fn bytes(&self, s: StrRef) -> &[u8] {
        self.strings.bytes(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StringEncoding;
    use std::sync::Arc;

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
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_ne_bytes(c))
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
