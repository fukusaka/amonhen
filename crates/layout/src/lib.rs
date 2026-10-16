//! FE 間で共通の `#[repr(C)]` レコード（数値の列、グラフの Node / Edge、文字列の参照）。
//!
//! ここにある型はメモリ上の形・ファイルの形・通信の形を兼ねる。フィールドの並びとサイズを変えたら、
//! 永続キャッシュの形式のバージョンを上げること。参照はポインタではなく、領域の先頭からのオフセットで持つ。

mod blob;
mod slab;

pub use blob::{Blob, ByteSource};
pub use slab::{Pod, Slab};

/// 文字列置き場の中の 1 つの文字列。
///
/// ASCII の文字列は全 FE で共通のバイト列を指し、`len` はバイト数。
/// ASCII 以外は FE の好む符号化（UTF-8 か UTF-16）の領域を指し、`len` はその符号単位の数。
///
/// `buf` は文字列置き場の番号、`off` はその中のバイト位置。同じプロセス内では、ASCII の文字列は
/// git オブジェクトを展開したバッファをそのまま指す（集め直さない）。共有メモリ・ファイル・
/// ネットワークに出すときは 1 つの領域に集め、`buf` はすべて 0 にする。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StrRef {
    pub buf: u32,
    pub off: u32,
    /// 最上位ビットが立っていれば ASCII 以外。残りの 31 ビットが長さ。
    len_and_kind: u32,
}

impl StrRef {
    const NON_ASCII: u32 = 1 << 31;
    pub const MAX_LEN: u32 = Self::NON_ASCII - 1;

    pub const fn ascii(buf: u32, off: u32, len: u32) -> Self {
        assert!(len <= Self::MAX_LEN);
        Self {
            buf,
            off,
            len_and_kind: len,
        }
    }

    pub const fn non_ascii(buf: u32, off: u32, len: u32) -> Self {
        assert!(len <= Self::MAX_LEN);
        Self {
            buf,
            off,
            len_and_kind: len | Self::NON_ASCII,
        }
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
    /// この線がつなぐ子と親の行（祖先の強調に使う）。縦線と終わりの曲線はレーンの持ち主の子のもの。
    pub child: u32,
    pub parent: u32,
}

impl Edge {
    /// first-parent の線（幹線として太く描く）。
    pub const FIRST_PARENT: u16 = 1 << 0;
    /// ほかの線が待っている親に合流する線の、始まりの曲線。そこから先の縦線と終わりの曲線は、
    /// 合流先のレーンの持ち主が描く。
    pub const RIDER: u16 = 1 << 1;
}

/// 差分の 1 行。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: u8,
    /// `STAGED` など。
    pub flags: u8,
    pub _pad: [u8; 2],
    /// 変更前・変更後の行番号（1 始まり）。その側にない行は 0。
    pub old_no: u32,
    pub new_no: u32,
    pub text: StrRef,
}

impl DiffRow {
    pub const CONTEXT: u8 = 0;
    pub const ADDED: u8 = 1;
    pub const REMOVED: u8 = 2;
    /// `@@ -1,3 +1,4 @@`
    pub const HUNK: u8 = 3;
    /// コンフリクトの Current 側。
    pub const OURS: u8 = 4;
    /// コンフリクトの Incoming 側。
    pub const THEIRS: u8 = 5;
    /// コンフリクトの共通の祖先（diff3）。
    pub const BASE: u8 = 6;
    /// `<<<<<<<` などのマーカーの行。
    pub const MARKER: u8 = 7;
    /// コミットの全ファイルを並べた差分の、ファイルの見出し（文字列はパス、`old_no`・`new_no` は削除・追加の行数）。
    pub const FILE: u8 = 8;

    /// `flags`: Working Copy の HEAD と作業ツリーの差分で、この行の変更が index に入っている（次のコミットに含める）。
    pub const STAGED: u8 = 1 << 0;
    /// `flags`: ファイルの見出しで、そのファイルの行を畳んでいる。
    pub const COLLAPSED: u8 = 1 << 1;
    /// `flags`: ファイルの見出しで、バイナリのファイル（行の差分はない）。
    pub const BINARY: u8 = 1 << 2;
}

/// 差分の行の中で強調する範囲（単語単位の変更）。位置は FE の文字列の符号単位。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Highlight {
    pub row: u32,
    pub start: u32,
    pub end: u32,
}

/// 差分の行の中の、構文の色付けの範囲。位置は FE の文字列の符号単位。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyntaxSpan {
    pub row: u32,
    pub start: u32,
    pub end: u32,
    pub kind: u8,
    pub _pad: [u8; 3],
}

impl SyntaxSpan {
    pub const KEYWORD: u8 = 1;
    pub const STRING: u8 = 2;
    pub const COMMENT: u8 = 3;
    pub const NUMBER: u8 = 4;
    pub const FUNCTION: u8 = 5;
    pub const TYPE: u8 = 6;
    pub const CONSTANT: u8 = 7;
    pub const PROPERTY: u8 = 8;
    pub const ATTRIBUTE: u8 = 9;
    pub const TAG: u8 = 10;
    pub const OPERATOR: u8 = 11;
    /// `self`・`this` などの組み込みの変数や、引数。
    pub const VARIABLE: u8 = 12;
    /// 文字列の中のエスケープや正規表現など。
    pub const ESCAPE: u8 = 13;
    pub const MODULE: u8 = 14;
}

unsafe impl Pod for StrRef {}
unsafe impl Pod for Node {}
unsafe impl Pod for Edge {}
unsafe impl Pod for DiffRow {}
unsafe impl Pod for Highlight {}
unsafe impl Pod for SyntaxSpan {}

const _: () = {
    assert!(size_of::<StrRef>() == 12 && align_of::<StrRef>() == 4);
    assert!(size_of::<Node>() == 12 && align_of::<Node>() == 4);
    assert!(size_of::<Edge>() == 24 && align_of::<Edge>() == 4);
    assert!(size_of::<DiffRow>() == 24 && align_of::<DiffRow>() == 4);
    assert!(size_of::<SyntaxSpan>() == 16 && align_of::<SyntaxSpan>() == 4);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_ref_keeps_length_and_kind_apart() {
        let a = StrRef::ascii(2, 10, 5);
        assert!(a.is_ascii());
        assert_eq!((a.buf, a.off, a.len()), (2, 10, 5));

        let n = StrRef::non_ascii(0, 3, StrRef::MAX_LEN);
        assert!(!n.is_ascii());
        assert_eq!(n.len(), StrRef::MAX_LEN);
    }
}
