use std::sync::Arc;

use amonhen_layout::StrRef;

use crate::StringEncoding;

enum Buffer {
    Utf8(Vec<u8>),
    Utf16(Vec<u16>),
    Shared(Arc<Vec<u8>>),
}

impl Buffer {
    fn as_ptr(&self) -> *const u8 {
        match self {
            Buffer::Utf8(v) => v.as_ptr(),
            Buffer::Utf16(v) => v.as_ptr().cast(),
            Buffer::Shared(v) => v.as_ptr(),
        }
    }
}

/// FE に渡す文字列置き場を組み立てる。
///
/// 0 番は変換した文字列（UTF-16 の FE なら ASCII 以外、UTF-8 の FE なら不正なバイト列を置き換えた
/// もの）。1 番以降は `share` で登録したバッファ（コミットの本文や blob）で、ASCII の文字列（UTF-8
/// の FE なら正しい UTF-8 の文字列すべて）はここを直接指す。
pub struct StringTableBuilder {
    converted: Converted,
    shared: Vec<Arc<Vec<u8>>>,
}

impl StringTableBuilder {
    pub fn new(encoding: StringEncoding) -> Self {
        Self {
            converted: Converted::new(encoding),
            shared: Vec::new(),
        }
    }

    /// バッファを登録し、その番号を返す。
    pub fn share(&mut self, buffer: Arc<Vec<u8>>) -> u32 {
        self.shared.push(buffer);
        self.shared.len() as u32
    }

    /// `buf` 番のバッファの中の `part` を指す。変換が要るときだけ 0 番に書く。
    pub fn add(&mut self, buf: u32, part: &[u8]) -> StrRef {
        let raw = &self.shared[buf as usize - 1];
        let off = (part.as_ptr() as usize - raw.as_ptr() as usize) as u32;
        debug_assert!(off as usize + part.len() <= raw.len());
        self.converted.add(buf, off, part)
    }

    pub fn finish(self) -> StringTable {
        let mut buffers = vec![self.converted.into_buffer()];
        buffers.extend(self.shared.into_iter().map(Buffer::Shared));
        let bases = buffers.iter().map(Buffer::as_ptr).collect();
        StringTable { buffers, bases }
    }
}

/// 不変の文字列置き場。`StrRef` の `buf` で引く。
pub struct StringTable {
    buffers: Vec<Buffer>,
    bases: Vec<*const u8>,
}

// `bases` は `buffers` が持つ、変更されないメモリを指すだけ。
unsafe impl Send for StringTable {}
unsafe impl Sync for StringTable {}

impl StringTable {
    /// 文字列置き場の先頭アドレス。
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

    fn add(&mut self, buf: u32, off: u32, part: &[u8]) -> StrRef {
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
