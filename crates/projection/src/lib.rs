//! 内部モデルから FE ごとの形への変換。
//!
//! FE は接続時に `FrontendProfile` を宣言し、この crate がその形にチャンク単位で変換する。
//! 形が同じ部分（数値の列、グラフの Node / Edge、ASCII の文字列）は変換せず参照で渡す。

mod history;
mod strings;

pub use history::HistoryChunk;
pub use strings::{StringTable, StringTableBuilder};

/// ASCII 以外の文字列の符号化。ASCII の文字列はどれでも共通のバイト列を参照する。
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringEncoding {
    /// Linux（GTK / Pango）、VSCode など。
    Utf8,
    /// macOS / iOS（CFString）、Windows（DirectWrite）、Android（Java の文字列）。
    Utf16,
}

/// FE が受け取る列。列ごとの配列で持つので、選ぶのはポインタを選ぶだけで詰め直さない。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnSet(u32);

impl ColumnSet {
    pub const GRAPH: Self = Self(1 << 0);
    pub const SUBJECT: Self = Self(1 << 1);
    pub const AUTHOR: Self = Self(1 << 2);
    pub const COMMIT_TIME: Self = Self(1 << 3);
    pub const REFS: Self = Self(1 << 4);
    pub const ALL: Self = Self((1 << 5) - 1);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrontendProfile {
    pub string: StringEncoding,
    pub columns: ColumnSet,
    /// チャンクの領域をページ境界に揃える（`MTLBuffer(bytesNoCopy:)` などでそのまま GPU に見せるため）。
    pub page_aligned: bool,
}

impl FrontendProfile {
    pub const MACOS: Self = Self {
        string: StringEncoding::Utf16,
        columns: ColumnSet::ALL,
        page_aligned: true,
    };

    /// daemon の FE 向けの窓口（`amonhen-front`）。VSCode 拡張・CLI。文字列は UTF-8 で、TS の `TextDecoder` で読む。
    pub const WEB: Self = Self {
        string: StringEncoding::Utf8,
        columns: ColumnSet::ALL,
        page_aligned: false,
    };
}
