//! FE 間で共通の `#[repr(C)]` レコード（数値の列、グラフの Node / Edge、文字列の参照）。
//!
//! ここにある型はメモリ上の形・ファイルの形・通信の形を兼ねる。フィールドの並びとサイズを変えたら、
//! 永続キャッシュの形式のバージョンを上げること。参照はポインタではなく、領域の先頭からのオフセットで持つ。

/// 文字列置き場の中の 1 つの文字列。
///
/// ASCII の文字列は全 FE で共通のバイト列を指し、`len` はバイト数。
/// ASCII 以外は FE の好む符号化（UTF-8 か UTF-16）の領域を指し、`len` はその符号単位の数。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StrRef {
    pub off: u32,
    /// 最上位ビットが立っていれば ASCII 以外。残りの 31 ビットが長さ。
    len_and_kind: u32,
}

impl StrRef {
    const NON_ASCII: u32 = 1 << 31;
    pub const MAX_LEN: u32 = Self::NON_ASCII - 1;

    pub const fn ascii(off: u32, len: u32) -> Self {
        assert!(len <= Self::MAX_LEN);
        Self { off, len_and_kind: len }
    }

    pub const fn non_ascii(off: u32, len: u32) -> Self {
        assert!(len <= Self::MAX_LEN);
        Self { off, len_and_kind: len | Self::NON_ASCII }
    }

    pub const fn len(self) -> u32 {
        self.len_and_kind & Self::MAX_LEN
    }

    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub const fn is_ascii(self) -> bool {
        self.len_and_kind & Self::NON_ASCII == 0
    }
}

/// グラフの点（コミット 1 つ）。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub row: u32,
    pub lane: u16,
    pub color: u16,
    pub flags: u16,
    pub _pad: u16,
}

impl Node {
    pub const MERGE: u16 = 1 << 0;
    pub const HEAD: u16 = 1 << 1;
    pub const STASH: u16 = 1 << 2;
    /// 未コミットの変更を表す仮の点。
    pub const WORKING_COPY: u16 = 1 << 3;
}

/// グラフの線 1 本。長い縦線も行ごとに分けず、区間 1 つで表す。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    pub row_start: u32,
    pub row_end: u32,
    pub lane_start: u16,
    pub lane_end: u16,
    pub color: u16,
    pub kind: u16,
}

impl Edge {
    /// first-parent の線（幹線として太く描く）。
    pub const FIRST_PARENT: u16 = 1 << 0;
}

const _: () = {
    assert!(size_of::<StrRef>() == 8 && align_of::<StrRef>() == 4);
    assert!(size_of::<Node>() == 12 && align_of::<Node>() == 4);
    assert!(size_of::<Edge>() == 16 && align_of::<Edge>() == 4);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_ref_keeps_length_and_kind_apart() {
        let a = StrRef::ascii(10, 5);
        assert!(a.is_ascii());
        assert_eq!((a.off, a.len()), (10, 5));

        let n = StrRef::non_ascii(3, StrRef::MAX_LEN);
        assert!(!n.is_ascii());
        assert_eq!(n.len(), StrRef::MAX_LEN);
    }
}
