//! リポジトリを開いて History を読み、件名などのチャンクを裏のスレッドで読む。
//!
//! 結果は `mpsc` でメインスレッドに送り、`wake` で起こす（Win32 なら `PostMessage`。設計の `Platform` の
//! 「メインスレッドを起こす」に当たる）。

use std::collections::{HashMap, HashSet};

use amonhen_core::diff::Whitespace;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use amonhen_backend_gix::GixBackend;
use amonhen_core::diff::{DiffView, StackedDiff};
use amonhen_core::{HistorySnapshot, RefsSnapshot, Repository, StatusSnapshot};
use amonhen_layout::StrRef;
use amonhen_model::{RefKind, Refs};
use amonhen_projection::{ColumnSet, FrontendProfile, HistoryChunk, StringEncoding};

pub const CHUNK_ROWS: u32 = 256;
/// 持っておくチャンクの数（見えている所に近いものから）。
const KEPT_CHUNKS: usize = 24;
/// 「差分をすべて開く」で開くファイルの、足した・消した行の数の上限（macOS 版・GTK4 版と同じ）。
pub const EXPAND_ALL_LIMIT: u32 = 1000;

/// Windows の FE の形。文字列は ASCII ならそのまま、それ以外は UTF-16（DirectWrite に `PCWSTR` で渡す）。
pub(crate) const PROFILE: FrontendProfile = FrontendProfile {
    string: StringEncoding::Utf16,
    columns: ColumnSet::ALL,
    page_aligned: false,
};

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// 裏のスレッドから受け取るチャンク。文字列置き場は自分のバッファを持っているので、どのスレッドに渡してもよい。
pub struct Chunk(HistoryChunk);

// SAFETY: `HistoryChunk` のポインタは、自分が持つバッファの先頭を指すだけ。
unsafe impl Send for Chunk {}

impl Chunk {
    pub fn index(&self, row: u32) -> usize {
        (row - self.0.start_row) as usize
    }

    pub fn subject(&self, i: usize) -> StrRef {
        self.0.subjects[i]
    }

    pub fn author(&self, i: usize) -> StrRef {
        self.0.author_names[i]
    }

    pub fn time(&self, i: usize) -> i64 {
        self.0.author_times[i]
    }

    /// 署名のあるコミット（検証はまだ。History の行の印）。
    pub fn signed(&self, i: usize) -> bool {
        self.0.signatures.get(i).is_some_and(|&k| k != 0)
    }

    /// `s` を UTF-16 で `out` に足す。
    pub fn push_utf16(&self, s: StrRef, out: &mut Vec<u16>) {
        push_utf16(self.0.bytes(s), s.is_ascii(), out);
    }
}

/// コアが `PROFILE` で詰めた文字列を UTF-16 で `out` に足す。ASCII はここで広げ、それ以外はコアが詰めた UTF-16 を
/// そのまま写す。
pub fn push_utf16(bytes: &[u8], ascii: bool, out: &mut Vec<u16>) {
    if ascii {
        out.extend(bytes.iter().map(|&b| b as u16));
    } else if bytes.as_ptr().align_offset(2) == 0 {
        // SAFETY: ASCII 以外はコアの `Vec<u16>` の中を、偶数のバイト位置から指す。
        let units =
            unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u16>(), bytes.len() / 2) };
        out.extend_from_slice(units);
    } else {
        out.extend(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| u16::from_ne_bytes(c)),
        );
    }
}

/// 差分の欄に描くもの（コミットの全ファイルの差分か、Working Copy の 1 ファイルの差分）。
#[derive(Clone, Copy)]
pub struct DiffContent<'a> {
    pub view: &'a DiffView,
    pub highlights: &'a HashMap<u32, Vec<(u32, u32)>>,
    pub syntax: &'a HashMap<u32, Vec<(u32, u32, u8)>>,
    /// ファイルの見出しの行の番号（昇順。上に張り付かせる見出しを探す）。
    pub file_rows: &'a [u32],
    /// 行ごとに次のコミットに含めるかの印を出す（Working Copy の 1 本の差分）。
    pub marks: bool,
    /// ファイルごとの、足した（A）・消した（D）・変えた（M）の印（見出しに出す。`file_rows` の順）。
    pub file_marks: &'a [char],
    /// 変更ファイルを木で見せるときの、表示の行の並び（None なら差分の行がそのまま表示の行）。
    pub tree: Option<&'a DiffTree>,
    /// まだ差分を取っていないファイルの、足した・消した行の数（パスごと。コミットの詳細の `stats`）。
    pub counts: Option<&'a HashMap<String, (u32, u32)>>,
    /// hunk の見出しに、前後の行を広げる印を出す（History のコミットの差分）。
    pub widen: bool,
    /// 全ファイルの差分（画像の比べる欄の前と後の中身）と、欄ごとの見せ方。
    pub stacked: Option<&'a StackedDiff>,
    pub image_views: Option<&'a HashMap<usize, crate::imagediff::View>>,
    /// 差分の持ち主を見分ける番号（画像を読んだものを取っておく鍵）。
    pub key: u64,
    /// その時点のファイルとして見せる（行番号は 1 列、変えた行は青）。
    pub file_mode: bool,
    /// すべてのファイルで開いた画像（差分の代わりに 1 枚の絵）と、その見せ方。
    pub picture: Option<(&'a [u8], crate::imagediff::View)>,
    /// SVG をモバイルで見るとき、その部品での見え方（元の絵と並べる）。
    pub mobile: Option<&'a MobileCheck>,
    /// 絵の下の説明を替えるもの（Android の VectorDrawable の「24 × 24 dp · 1 kB」）。
    pub picture_caption: Option<&'a str>,
    /// 選んだ行（Working Copy の差分。地を塗る）。
    pub selection: Option<&'a std::collections::BTreeSet<u32>>,
}

/// SVG の見せ方（macOS 版の `SVGView`、GTK4 版の `SvgView`）: 絵・モバイル（絵と、モバイル向けの描画部品で
/// 崩れる機能の一覧）・テキスト。開いている間は覚える（覚え直さない）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SvgView {
    #[default]
    Picture,
    Mobile,
    Text,
}

/// SVG のモバイル向けの確認: 崩れる機能（`svglint`）と、見ている部品での見え方（`svgmobile`）。
pub struct MobileCheck {
    pub issues: Vec<amonhen_core::svglint::Issue>,
    pub preview: amonhen_core::svgmobile::Preview,
}

impl DiffContent<'_> {
    /// 表示の行の数。
    pub fn line_count(&self) -> usize {
        self.tree.map_or(self.view.rows.len(), |t| t.lines.len())
    }

    /// 表示の `n` 行目。
    pub fn line(&self, n: usize) -> Option<DiffLine> {
        match self.tree {
            Some(t) => t.lines.get(n).copied(),
            None => (n < self.view.rows.len()).then_some(DiffLine::Row(n as u32)),
        }
    }

    /// 表示の行の、ファイルの見出しの位置（昇順。上に張り付かせる見出しを探す）。
    pub fn file_lines(&self) -> &[u32] {
        self.tree
            .map_or(self.file_rows, |t| t.file_lines.as_slice())
    }

    /// 差分の `row` 行目の、表示の行（隠したファイルの見出しなら None）。
    pub fn line_of(&self, row: u32) -> Option<u32> {
        match self.tree {
            Some(t) => t
                .line_of_row
                .get(row as usize)
                .copied()
                .filter(|&l| l != u32::MAX),
            None => Some(row),
        }
    }

    /// `file` 番目のファイルの見出しの字下げの深さと、見出しに出す名前（リストなら 0 とパスのまま）。
    pub fn file_title(&self, file: usize) -> (u32, Option<&str>) {
        match self.tree {
            Some(t) => (
                t.depths.get(file).copied().unwrap_or(0),
                t.titles.get(file).map(String::as_str),
            ),
            None => (0, None),
        }
    }
}

/// 差分の欄の表示の 1 行: 差分の行か、変更ファイルを木で見せるときのフォルダの行（`DiffTree::folders` の番号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLine {
    Row(u32),
    Folder(u32),
    /// 開いた画像の比べる欄（ファイルの番号と、欄の中の何行目か）。
    Panel(u32, u32),
    /// その時点のファイルで、消した行の所の印（`DiffTree::deletions` の番号）。
    Deleted(u32),
    /// 折り返した行の続き（差分の行と、その行の文字の範囲。UTF-16 の位置）。
    Cont(u32, u32, u32),
}

/// 木で見せるときのフォルダの行（macOS 版の `DiffViewController.Folder`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderLine {
    pub name: String,
    pub depth: u32,
    /// リポジトリの根からのパス（畳んだフォルダを覚える鍵）。
    pub path: String,
    pub collapsed: bool,
}

/// 変更ファイルを木で見せるときの、表示の行の並び（macOS 版の `treeLayout` と `setFolders`）。ファイルの
/// 見出しの前にフォルダの行を差し込み、畳んだフォルダの中のファイルは見出しごと隠す。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiffTree {
    pub lines: Vec<DiffLine>,
    pub folders: Vec<FolderLine>,
    /// 差分の行ごとの表示の行（隠した行は `u32::MAX`）。
    line_of_row: Vec<u32>,
    /// 表示の行の、ファイルの見出しの位置（隠したファイルのものはない）。
    file_lines: Vec<u32>,
    /// ファイルごとの字下げの深さ（入っているフォルダの数）と、見出しに出す名前（パスの最後）。
    depths: Vec<u32>,
    titles: Vec<String>,
    /// 比べる欄を出している画像のファイル（見出しは開いた ▾ で描く）。
    pub panels: HashSet<usize>,
    /// その時点のファイルで見せるときの、変えた行（消した行に続けて足した行）と、消した行の数。
    pub modified: HashSet<u32>,
    pub deletions: Vec<u32>,
    /// 折り返した行の、最初の表示の行に出す文字の終わり（UTF-16 の位置。続きは `DiffLine::Cont`）。
    pub first_end: HashMap<u32, u32>,
}

/// 長い行を折り返す設定（アプリ全体。macOS 版・GTK4 版と同じ鍵 `wrapDiffLines`）。
pub const WRAP_LINES: &str = "wrapDiffLines";

pub fn wrap_lines() -> bool {
    crate::prefs::bool(WRAP_LINES)
}

/// 行の文字を、`cols` 桁（等幅の字の数。全角は 2、タブは 4）ごとに区切る（UTF-16 の位置の範囲）。区切りは、
/// 範囲の後ろ半分にある空白の後ろを選び、なければ桁で切る（GTK4 版の `rewrap`）。収まれば 1 つ。
pub fn wrap_breaks(text: &str, cols: usize) -> Vec<(u32, u32)> {
    let cols = cols.max(8);
    let mut out = Vec::new();
    let (mut start, mut at, mut width) = (0u32, 0u32, 0usize);
    // 範囲の中の、最後の空白の後ろ（位置と、そこまでの桁）。
    let mut space: Option<(u32, usize)> = None;
    for c in text.chars() {
        let w = match c {
            '\t' => 4,
            c if (c as u32) >= 0x1100 && unicode_wide(c) => 2,
            _ => 1,
        };
        if width + w > cols && at > start {
            let (end, used) = match space.filter(|&(_, used)| used * 2 >= cols) {
                Some(found) => found,
                None => (at, width),
            };
            out.push((start, end));
            start = end;
            width -= used;
            space = None;
        }
        at += c.len_utf16() as u32;
        width += w;
        if c == ' ' {
            space = Some((at, width));
        }
    }
    out.push((start, at));
    out
}

/// 全角（東アジアの広い字）か（おおまかに。CJK・かな・ハングル・全角の記号）。
fn unicode_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
}

/// 表示の行の並び `base`（なければ差分の行のまま）で、`cols` 桁より長い差分の行を折り返した並び（ファイルの
/// 見出しと hunk の見出しは折り返さない）。折り返す行がなく `base` もなければ None。
pub fn wrapped(view: &DiffView, base: Option<DiffTree>, cols: usize) -> Option<DiffTree> {
    use amonhen_layout::DiffRow;
    if cols == 0 {
        return base;
    }
    let mut breaks: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for (i, r) in view.rows.iter().enumerate() {
        if matches!(r.kind, DiffRow::FILE | DiffRow::HUNK) {
            continue;
        }
        let bytes = view.strings.bytes(r.text);
        // 桁の数を超えないほど短い行は見るまでもない（どの字もバイトの数より多くの桁は取らない）。
        if bytes.len() <= cols {
            continue;
        }
        // ASCII 以外の行は、コアが UTF-16 で持っている（`push_utf16`）。
        let mut units = Vec::with_capacity(bytes.len());
        push_utf16(bytes, r.text.is_ascii(), &mut units);
        let parts = wrap_breaks(&String::from_utf16_lossy(&units), cols);
        if parts.len() > 1 {
            breaks.insert(i as u32, parts);
        }
    }
    if breaks.is_empty() {
        return base;
    }
    let mut tree = base.unwrap_or_else(|| DiffTree {
        lines: (0..view.rows.len() as u32).map(DiffLine::Row).collect(),
        ..Default::default()
    });
    let mut lines = Vec::with_capacity(tree.lines.len() + breaks.len());
    for line in std::mem::take(&mut tree.lines) {
        lines.push(line);
        if let DiffLine::Row(i) = line
            && let Some(parts) = breaks.get(&i)
        {
            tree.first_end.insert(i, parts[0].1);
            lines.extend(parts[1..].iter().map(|&(s, e)| DiffLine::Cont(i, s, e)));
        }
    }
    tree.line_of_row = vec![u32::MAX; view.rows.len()];
    tree.file_lines.clear();
    for (n, line) in lines.iter().enumerate() {
        if let DiffLine::Row(i) = *line {
            tree.line_of_row[i as usize] = n as u32;
            if view.rows[i as usize].kind == DiffRow::FILE {
                tree.file_lines.push(n as u32);
            }
        }
    }
    tree.lines = lines;
    Some(tree)
}

/// 木の形（ファイルごとの前に差し込むフォルダの行、深さ、名前、畳んだフォルダの中で隠すファイル）。並びは
/// パスの順のまま（macOS 版の `treeLayout`）。
#[allow(clippy::type_complexity)]
fn tree_layout(
    paths: &[&str],
    collapsed_folders: &HashSet<String>,
) -> (
    HashMap<usize, Vec<FolderLine>>,
    Vec<u32>,
    Vec<String>,
    HashSet<usize>,
) {
    let mut groups = HashMap::new();
    let mut depths = Vec::with_capacity(paths.len());
    let mut titles = Vec::with_capacity(paths.len());
    let mut hidden = HashSet::new();
    let mut previous: Vec<&str> = Vec::new();
    for (file, path) in paths.iter().enumerate() {
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop().unwrap_or(path);
        let common = previous
            .iter()
            .zip(&parts)
            .take_while(|(a, b)| a == b)
            .count();
        // 畳んだフォルダ（いちばん外側）。その中は見出しを出さない。
        let outer = (0..parts.len()).find(|&l| collapsed_folders.contains(&parts[..=l].join("/")));
        let folders: Vec<FolderLine> = (common..parts.len())
            .filter(|&level| outer.is_none_or(|o| level <= o))
            .map(|level| {
                let folder = parts[..=level].join("/");
                FolderLine {
                    name: parts[level].to_owned(),
                    depth: level as u32,
                    collapsed: collapsed_folders.contains(&folder),
                    path: folder,
                }
            })
            .collect();
        if outer.is_some() {
            hidden.insert(file);
        }
        if !folders.is_empty() {
            groups.insert(file, folders);
        }
        depths.push(parts.len() as u32);
        titles.push(name.to_owned());
        previous = parts;
    }
    (groups, depths, titles, hidden)
}

/// 行ごとの、変わった単語の範囲。
fn highlights_of(view: &DiffView) -> HashMap<u32, Vec<(u32, u32)>> {
    let mut out: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for h in &view.highlights {
        out.entry(h.row).or_default().push((h.start, h.end));
    }
    out
}

/// 行ごとの構文の色（ファイル全体を解析するので、裏のスレッドで呼ぶ）。
fn syntax_of(view: &DiffView) -> HashMap<u32, Vec<(u32, u32, u8)>> {
    let mut out: HashMap<u32, Vec<(u32, u32, u8)>> = HashMap::new();
    for s in view.syntax() {
        out.entry(s.row).or_default().push((s.start, s.end, s.kind));
    }
    out
}

/// Working Copy で選んだファイルの差分（HEAD と作業ツリーの 1 本の差分か、片側の差分）。
pub struct WorkingShown {
    pub path: String,
    pub diff: Arc<amonhen_core::WorkingDiff>,
    pub highlights: HashMap<u32, Vec<(u32, u32)>>,
    pub syntax: HashMap<u32, Vec<(u32, u32, u8)>>,
    /// 長い行を折り返した表示の行の並びと、その桁の数。
    pub tree: Option<DiffTree>,
    pub wrap_cols: usize,
    /// 選んだ行（差分の行の番号）と、Shift で続けて選ぶ起点。差分を読み直したら外れる（macOS 版・GTK4 版と同じ。
    /// 読み直すと行の番号が別の行を指すことがある）。
    pub selection: std::collections::BTreeSet<u32>,
    pub anchor: Option<u32>,
}

impl WorkingShown {
    pub fn content(&self) -> DiffContent<'_> {
        DiffContent {
            view: &self.diff.view,
            highlights: &self.highlights,
            syntax: &self.syntax,
            file_rows: &[],
            marks: self.diff.combined.is_some(),
            file_marks: &[],
            tree: self.tree.as_ref(),
            counts: None,
            widen: false,
            stacked: None,
            image_views: None,
            key: 0,
            file_mode: false,
            picture: None,
            mobile: None,
            picture_caption: None,
            selection: Some(&self.selection),
        }
    }

    /// 行 `row` を選ぶ（macOS 版・GTK4 版と同じく前後の行も選べる）。hunk の見出しなら、その hunk の行をすべて。
    /// `shift` で起点からの続いた行、`ctrl` で足す・外す（すべて選んでいれば外す）。
    pub fn select(&mut self, row: u32, shift: bool, ctrl: bool) {
        use amonhen_layout::DiffRow;
        let rows = &self.diff.view.rows;
        let Some(kind) = rows.get(row as usize).map(|r| r.kind) else {
            return;
        };
        let picked: Vec<u32> = if kind == DiffRow::HUNK {
            std::iter::once(row)
                .chain((row + 1..rows.len() as u32).take_while(|&r| {
                    !matches!(rows[r as usize].kind, DiffRow::HUNK | DiffRow::FILE)
                }))
                .collect()
        } else {
            vec![row]
        };
        match (shift, self.anchor) {
            (true, Some(anchor)) => {
                self.selection = (anchor.min(row)..=anchor.max(row)).collect();
            }
            _ if ctrl => {
                if picked.iter().all(|r| self.selection.contains(r)) {
                    for r in &picked {
                        self.selection.remove(r);
                    }
                } else {
                    self.selection.extend(picked);
                }
                self.anchor = Some(row);
            }
            _ => {
                self.selection = picked.into_iter().collect();
                self.anchor = Some(row);
            }
        }
    }

    /// 選んだ行のうち、変更の行（捨てるのはこの行）。
    pub fn selected_changes(&self) -> Vec<usize> {
        use amonhen_layout::DiffRow;
        self.selection
            .iter()
            .filter(|&&r| {
                self.diff
                    .view
                    .rows
                    .get(r as usize)
                    .is_some_and(|row| matches!(row.kind, DiffRow::ADDED | DiffRow::REMOVED))
            })
            .map(|&r| r as usize)
            .collect()
    }
}

/// 「すべてのファイル」で開いたファイルの、その時点の中身（ファイル全体の差分を、ファイルとして見せる）。
pub struct FileShown {
    pub path: String,
    pub view: DiffView,
    syntax: HashMap<u32, Vec<(u32, u32, u8)>>,
    highlights: HashMap<u32, Vec<(u32, u32)>>,
    /// その時点のファイルとして見せる行の並びと、それを折り返したもの。
    base: DiffTree,
    tree: DiffTree,
    wrap_cols: usize,
    /// 画像なら、その時点の中身（差分の代わりに 1 枚の絵で見せる）と、見せ方（倍率）。
    pub picture: Option<Arc<Vec<u8>>>,
    pub picture_view: crate::imagediff::View,
    /// 絵を読んだものを取っておく鍵（開くたびに新しい）。
    pub key: u64,
    /// Markdown なら、整形して見せるブロックと、その中の図を絵にしたもの（出てくる順）。
    pub markdown: Option<Vec<amonhen_core::markdown::Block>>,
    pub diagrams: Arc<Vec<Option<crate::markdownview::DiagramPicture>>>,
    /// Markdown の中の、このコミットから読めた画像。
    pub images: Arc<crate::markdownview::MdImages>,
    /// SVG をモバイルで見ているなら、その確認。
    pub mobile: Option<Arc<MobileCheck>>,
    /// Android の VectorDrawable なら、絵の下の説明と添え書き（テキストで見ていても、切り替えを出すために持つ）。
    pub drawable: Option<Drawable>,
}

/// すべてのファイルで開いた Android の VectorDrawable（macOS 版・GTK4 版と同じく、SVG にして絵で見せる）。
pub struct Drawable {
    /// 「24 × 24 dp · 1 kB」。
    pub caption: String,
    /// 引けなかった色・SVG では同じに描けないもの（訳した文）。
    pub notes: Vec<String>,
}

impl FileShown {
    pub fn content(&self) -> DiffContent<'_> {
        DiffContent {
            view: &self.view,
            highlights: &self.highlights,
            syntax: &self.syntax,
            file_rows: &[],
            marks: false,
            file_marks: &[],
            tree: Some(&self.tree),
            counts: None,
            widen: false,
            stacked: None,
            image_views: None,
            key: self.key,
            file_mode: true,
            picture: self
                .picture
                .as_deref()
                .map(|p| (p.as_slice(), self.picture_view)),
            mobile: self.mobile.as_deref(),
            picture_caption: self.drawable.as_ref().map(|d| d.caption.as_str()),
            selection: None,
        }
    }
}

/// コミット `commit` の時点の `path` の中身（このコミットで変えたなら `change` の blob、変えていなければ木から
/// 引く。GTK4 版の `blob_id`・`tree_blob`）。
pub fn blob_at(
    repo: &Repository,
    commit: &amonhen_model::Oid,
    change: Option<&amonhen_model::FileChange>,
    path: &str,
) -> Option<Arc<Vec<u8>>> {
    let oid = match change {
        Some(c) if c.kind == amonhen_model::ChangeKind::Deleted => c.old_id,
        Some(c) => c.new_id,
        None => {
            let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
            let request = serde_json::json!({
                "op": "treeEntries",
                "oid": commit.to_string(),
                "path": folder,
            });
            let response: serde_json::Value =
                serde_json::from_str(&repo.run_json(&request.to_string())).ok()?;
            response["info"]["entries"]
                .as_array()?
                .iter()
                .find(|e| e["name"] == name && e["kind"] != "folder")
                .and_then(|e| e["oid"].as_str())
                .and_then(amonhen_model::Oid::from_hex)
        }
    }?;
    repo.blob(&oid).ok()
}

/// ファイル全体の差分を、その時点のファイルとして見せる行の並び（GTK4 版の `rebuild_extra` の `file_mode`）:
/// hunk の見出しと消した行は出さず、消した行に続けて足した行は「変えた行」、消しただけの所には「N 行消した」の
/// 行を差し込む。ファイルごと消したなら、消した行をそのまま出す。
fn file_lines(view: &DiffView) -> DiffTree {
    use amonhen_layout::DiffRow;
    let rows = &view.rows;
    let deleted_file = !rows
        .iter()
        .any(|r| matches!(r.kind, DiffRow::ADDED | DiffRow::CONTEXT));
    let mut tree = DiffTree {
        line_of_row: vec![u32::MAX; rows.len()],
        ..Default::default()
    };
    let mut row = 0;
    while row < rows.len() {
        let kind = rows[row].kind;
        if kind == DiffRow::HUNK {
            row += 1;
        } else if kind == DiffRow::REMOVED && !deleted_file {
            let start = row;
            while row < rows.len() && rows[row].kind == DiffRow::REMOVED {
                row += 1;
            }
            if row < rows.len() && rows[row].kind == DiffRow::ADDED {
                while row < rows.len() && rows[row].kind == DiffRow::ADDED {
                    tree.modified.insert(row as u32);
                    tree.line_of_row[row] = tree.lines.len() as u32;
                    tree.lines.push(DiffLine::Row(row as u32));
                    row += 1;
                }
            } else {
                tree.lines
                    .push(DiffLine::Deleted(tree.deletions.len() as u32));
                tree.deletions.push((row - start) as u32);
            }
        } else {
            tree.line_of_row[row] = tree.lines.len() as u32;
            tree.lines.push(DiffLine::Row(row as u32));
            row += 1;
        }
    }
    tree
}

/// 読んだ差分（空白の扱いを変えたら、同じ引数でもう一度読む）。
#[derive(Clone)]
enum Want {
    Commit(u32),
    Oid(amonhen_model::Oid, bool),
    FileChange(amonhen_model::Oid, String),
    Range(
        Option<amonhen_model::Oid>,
        amonhen_model::Oid,
        String,
        String,
        Vec<[String; 3]>,
    ),
    Selection(Vec<u32>),
}

/// 空白の扱いを変えて読み直す前の、開き方（macOS 版の `restoreAfterReload`）。
struct Restore {
    collapsed: Vec<bool>,
    contexts: HashMap<usize, u32>,
    whole: HashSet<usize>,
    folders: HashSet<String>,
}

/// 選んだコミットの詳細（`commit_info` の答え）と、全ファイルを並べた差分。
pub struct CommitShown {
    pub row: u32,
    pub info: serde_json::Value,
    pub diff: DiffView,
    /// 差分の行ごとの、変わった単語の範囲（UTF-16 の位置）。
    pub highlights: HashMap<u32, Vec<(u32, u32)>>,
    /// 差分の行ごとの構文の色（範囲と種類）。ファイル全体を解析するので、差分より後に届く。
    pub syntax: HashMap<u32, Vec<(u32, u32, u8)>>,
    /// 時間のかかる詳細（`commit_extras` の答え。署名・子・含むブランチとタグなど）。後から届く。
    pub extras: Option<serde_json::Value>,
    /// 全ファイルの差分（畳むファイルを変えたら、ここから表示を作り直す）。
    pub stacked: Arc<StackedDiff>,
    /// ファイルごとに畳んでいるか（コミットの変更ファイルの順）。
    pub collapsed: Vec<bool>,
    /// ファイルの見出しの行の番号（表示を作り直すたびに求める）。
    pub file_rows: Vec<u32>,
    /// ファイルごとの、足した（A）・消した（D）・変えた（M）の印。
    file_marks: Vec<char>,
    /// 変更ファイルを木で見せるか（帯のリスト／ツリー）と、畳んだフォルダ（パス）。
    tree_on: bool,
    pub collapsed_folders: HashSet<String>,
    /// 木で見せるときの表示の行の並び。
    tree: Option<DiffTree>,
    /// まだ差分を取っていないファイルの行の数（パスごと。詳細の `stats`）。
    counts: HashMap<String, (u32, u32)>,
    /// ファイルごとの前後の行の数（広げたものだけ）と、ファイル全体で見せているファイル。
    contexts: HashMap<usize, u32>,
    whole: HashSet<usize>,
    /// 画像の比べる欄ごとの見せ方（位置・倍率）。
    pub image_views: HashMap<usize, crate::imagediff::View>,
    /// 長い行を折り返す桁の数（0 なら折り返さない）。
    wrap_cols: usize,
    /// 「テキストで見る」にした SVG（絵の代わりに差分）。
    svg_text: HashSet<usize>,
    /// この差分を見分ける番号（読み直すたびに新しい。画像を読んだものを取っておく鍵）。
    id: u64,
    /// 表示を作り直した回数（古い表示の構文の色を捨てるため）。
    version: u32,
}

impl CommitShown {
    fn new(row: u32, info: serde_json::Value, stacked: Arc<StackedDiff>) -> Self {
        // 最初はすべて畳んで、見出しだけを並べる（macOS 版・GTK4 版と同じ）。
        let collapsed = vec![true; stacked.files.len()];
        let file_marks = stacked.files.iter().map(|(_, d)| change_mark(d)).collect();
        let mut shown = Self {
            row,
            info,
            diff: stacked.view(&collapsed, &PROFILE),
            highlights: HashMap::new(),
            syntax: HashMap::new(),
            extras: None,
            stacked,
            collapsed,
            file_rows: Vec::new(),
            file_marks,
            tree_on: crate::prefs::bool(crate::prefs::HISTORY_FILE_TREE),
            collapsed_folders: HashSet::new(),
            tree: None,
            counts: HashMap::new(),
            contexts: HashMap::new(),
            whole: HashSet::new(),
            image_views: HashMap::new(),
            wrap_cols: 0,
            svg_text: HashSet::new(),
            id: {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            version: 0,
        };
        // 最初の表示（構文の色は、読み込んだ所がこの版 0 の表示のものを送る。最初は隠すファイルがないので、
        // 畳み方は `collapsed` のまま）。
        shown.rebuild_view();
        shown.version = 0;
        shown
    }

    pub fn content(&self) -> DiffContent<'_> {
        DiffContent {
            view: &self.diff,
            highlights: &self.highlights,
            syntax: &self.syntax,
            file_rows: &self.file_rows,
            marks: false,
            file_marks: &self.file_marks,
            tree: self.tree.as_ref(),
            counts: Some(&self.counts),
            widen: true,
            stacked: Some(&self.stacked),
            image_views: Some(&self.image_views),
            key: self.id,
            file_mode: false,
            picture: None,
            mobile: None,
            picture_caption: None,
            selection: None,
        }
    }

    /// 時間のかかる詳細が届いた（まだ差分を取っていないファイルの行の数も、ここから引く）。
    fn set_extras(&mut self, extras: serde_json::Value) {
        self.counts = extras["stats"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let added = s["added"].as_u64().unwrap_or(0) as u32;
                let removed = s["removed"].as_u64().unwrap_or(0) as u32;
                Some((s["path"].as_str()?.to_owned(), (added, removed)))
            })
            .collect();
        self.extras = Some(extras);
    }

    /// `file` 番目のファイルの足した・消した行の数（まだ差分を取っていなければ、詳細の `stats` から）。
    fn file_counts(&self, file: usize) -> (u32, u32) {
        let (path, diff) = &self.stacked.files[file];
        if diff.pending.is_some() {
            self.counts.get(path).copied().unwrap_or((0, 0))
        } else {
            (diff.added, diff.removed)
        }
    }

    /// `file` 番目のファイルを前後 `context` 行で取り直す（まだ差分を取っていなければ、ここで取る。`u32::MAX` なら
    /// ファイル全体）。取れなければ false。
    fn load(&mut self, repo: &Repository, file: usize, context: u32) -> bool {
        if Arc::get_mut(&mut self.stacked).is_none() {
            // 構文の色を求めている裏の仕事が持っているので、写してから変える。
            self.stacked = Arc::new(StackedDiff {
                files: self.stacked.files.clone(),
            });
        }
        let Some(stacked) = Arc::get_mut(&mut self.stacked) else {
            return false;
        };
        match repo.set_stacked_context(stacked, file, context) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("差分を読めない（{}）: {e}", stacked.files[file].0);
                false
            }
        }
    }

    /// まだ差分を取っていないなら取る。
    fn load_pending(&mut self, repo: &Repository, file: usize) {
        if self
            .stacked
            .files
            .get(file)
            .is_some_and(|(_, d)| d.pending.is_some())
        {
            self.load(repo, file, 3);
        }
    }

    /// 「取ってくる」で取り直したファイルの差分（`files`。ファイルの番号と差分）を入れる。まだ手元になかった
    /// ファイルだけを入れ替え、広げた前後・ファイル全体にしていたものはその形で取り直す。変えたら true。
    fn take_fetched(
        &mut self,
        files: Vec<(usize, amonhen_core::diff::FileDiff)>,
        repo: Option<&Repository>,
    ) -> bool {
        if Arc::get_mut(&mut self.stacked).is_none() {
            // 構文の色を求めている裏の仕事が持っているので、写してから変える。
            self.stacked = Arc::new(StackedDiff {
                files: self.stacked.files.clone(),
            });
        }
        let Some(stacked) = Arc::get_mut(&mut self.stacked) else {
            return false;
        };
        let mut taken = Vec::new();
        for (file, diff) in files {
            let Some((_, old)) = stacked.files.get_mut(file) else {
                continue;
            };
            if old.missing.is_empty() {
                continue;
            }
            *old = diff;
            if let Some(mark) = self.file_marks.get_mut(file) {
                *mark = change_mark(old);
            }
            taken.push(file);
        }
        if let Some(repo) = repo {
            for file in &taken {
                let context = if self.whole.contains(file) {
                    Some(u32::MAX)
                } else {
                    self.contexts.get(file).copied()
                };
                if let Some(context) = context {
                    self.load(repo, *file, context);
                }
            }
        }
        !taken.is_empty()
    }

    /// `file` 番目のファイルをファイル全体で見せているか（見出しの ⋯ の Show Whole File）。
    pub fn shows_whole(&self, file: usize) -> bool {
        self.whole.contains(&file)
    }

    /// 変更ファイルを木で見せるか。
    pub fn tree_on(&self) -> bool {
        self.tree_on
    }

    /// 木で見せるときの、すべてのフォルダ（パス）。
    fn all_folders(&self) -> HashSet<String> {
        let mut all = HashSet::new();
        for (path, _) in &self.stacked.files {
            let parts: Vec<&str> = path.split('/').collect();
            for level in 0..parts.len().saturating_sub(1) {
                all.insert(parts[..=level].join("/"));
            }
        }
        all
    }

    /// 「差分をすべて開く」で開くファイル（生成物と、変更が多すぎるファイルは除く。macOS 版の
    /// `expandableFiles`）。
    pub fn expandable(&self) -> Vec<usize> {
        (0..self.stacked.files.len())
            .filter(|&i| {
                let (added, removed) = self.file_counts(i);
                !self.stacked.generated(i) && added + removed <= EXPAND_ALL_LIMIT
            })
            .collect()
    }

    /// 生成物などのほかは、すべて開いているか（帯の「すべて開く・畳む」の向き）。
    pub fn all_open(&self) -> bool {
        let targets = self.expandable();
        !targets.is_empty() && targets.iter().all(|&i| !self.collapsed[i])
    }

    /// 変更ファイルの数と、足した・消した行の数の合計（帯に出す）。
    pub fn totals(&self) -> (usize, u32, u32) {
        let count = self.stacked.files.len();
        let (added, removed) = (0..count)
            .map(|i| self.file_counts(i))
            .fold((0, 0), |(a, r), (x, y)| (a + x, r + y));
        (count, added, removed)
    }

    /// 表示を作り直す（構文の色は捨てる）。色を付け直すための、全ファイルと畳んでいるかを返す。
    fn rebuild_view(&mut self) -> (Arc<StackedDiff>, Vec<bool>) {
        let stacked = self.stacked.clone();
        // 木なら、畳んだフォルダの中のファイルも畳む（見出しごと隠す）。
        let layout = self.tree_on.then(|| {
            let paths: Vec<&str> = stacked.files.iter().map(|(p, _)| p.as_str()).collect();
            tree_layout(&paths, &self.collapsed_folders)
        });
        let mut collapsed = self.collapsed.clone();
        if let Some((_, _, _, hidden)) = &layout {
            for &file in hidden {
                collapsed[file] = true;
            }
        }
        // 開いた画像は、差分の行の代わりに見出しの下に比べる欄（差分は畳んでおく。macOS 版と同じ）。
        let panels: HashSet<usize> = stacked
            .files
            .iter()
            .enumerate()
            // 中身が手元にない画像は、比べる欄を作らない（帯で取ってくるまで。macOS 版と同じ）。
            .filter(|&(i, (path, diff))| {
                !collapsed[i]
                    && crate::imagediff::is_picture(path)
                    && !self.svg_text.contains(&i)
                    && diff.missing.is_empty()
            })
            .map(|(i, _)| i)
            .collect();
        for &file in &panels {
            collapsed[file] = true;
        }
        self.diff = stacked.view(&collapsed, &PROFILE);
        self.index_highlights();
        let layout = layout.or_else(|| {
            (!panels.is_empty()).then(|| (HashMap::new(), Vec::new(), Vec::new(), HashSet::new()))
        });
        self.tree = layout.map(|(groups, depths, titles, hidden)| {
            let mut tree = DiffTree {
                depths,
                titles,
                line_of_row: vec![u32::MAX; self.diff.rows.len()],
                panels: panels.clone(),
                ..Default::default()
            };
            let mut file = 0;
            for (row, r) in self.diff.rows.iter().enumerate() {
                if r.kind == amonhen_layout::DiffRow::FILE {
                    for folder in groups.get(&file).into_iter().flatten() {
                        tree.lines.push(DiffLine::Folder(tree.folders.len() as u32));
                        tree.folders.push(folder.clone());
                    }
                    let shown = !hidden.contains(&file);
                    file += 1;
                    if !shown {
                        continue;
                    }
                    tree.file_lines.push(tree.lines.len() as u32);
                }
                tree.line_of_row[row] = tree.lines.len() as u32;
                tree.lines.push(DiffLine::Row(row as u32));
                if r.kind == amonhen_layout::DiffRow::FILE && panels.contains(&(file - 1)) {
                    for k in 0..crate::imagediff::PANEL_LINES {
                        tree.lines
                            .push(DiffLine::Panel((file - 1) as u32, k as u32));
                    }
                }
            }
            tree
        });
        // 長い行を折り返す（桁の数が決まっていれば）。
        self.tree = wrapped(&self.diff, self.tree.take(), self.wrap_cols);
        self.syntax.clear();
        self.version += 1;
        (stacked, collapsed)
    }

    /// 単語の範囲とファイルの見出しの行を、表示から引けるようにする。
    fn index_highlights(&mut self) {
        self.highlights.clear();
        for h in &self.diff.highlights {
            self.highlights
                .entry(h.row)
                .or_default()
                .push((h.start, h.end));
        }
        // ファイルの見出しの行（描くたびに全部の行を見なくて済むように）。
        self.file_rows = self
            .diff
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind == amonhen_layout::DiffRow::FILE)
            .map(|(i, _)| i as u32)
            .collect();
    }
}

/// ファイルの変更の種類の印（前の中身がなければ足した A、後の中身がなければ消した D、ほかは変えた M）。
pub fn change_mark(diff: &amonhen_core::diff::FileDiff) -> char {
    // partial clone で中身が手元にないファイルは、中身がないので変更の種類から（macOS 版と同じく変更の一覧の種類）。
    if !diff.missing.is_empty()
        && let Some(change) = &diff.pending
    {
        return match change.kind {
            amonhen_model::ChangeKind::Added => 'A',
            amonhen_model::ChangeKind::Deleted => 'D',
            _ => 'M',
        };
    }
    if diff.old.is_empty() && !diff.new.is_empty() {
        'A'
    } else if diff.new.is_empty() && !diff.old.is_empty() {
        'D'
    } else {
        'M'
    }
}

/// 構文の色を裏で求めて送る（`version` の表示のもの）。
fn send_syntax(
    tx: Sender<Event>,
    wake: Wake,
    generation: u64,
    row: u32,
    version: u32,
    stacked: Arc<StackedDiff>,
    collapsed: Vec<bool>,
) {
    std::thread::spawn(move || {
        // 同じ行の並びの表示をもう一度作って解析する（ファイルの中身は共有したまま）。
        let mut syntax: HashMap<u32, Vec<(u32, u32, u8)>> = HashMap::new();
        for s in stacked.view(&collapsed, &PROFILE).syntax() {
            syntax
                .entry(s.row)
                .or_default()
                .push((s.start, s.end, s.kind));
        }
        if !syntax.is_empty() {
            let _ = tx.send(Event::Syntax(generation, row, version, syntax));
            wake();
        }
    });
}

/// `f` にかかった時間を Activity の Performance に `metric`（訳す前の名前）で記録する。
fn timed<T>(metric: &'static str, f: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    let value = f();
    crate::activity::record(metric, started.elapsed().as_secs_f64() * 1000.0);
    value
}

/// 届いた検索の答え。
pub enum SearchDone {
    Found(String),
    Failed(String, String),
}

/// 署名の検証の結果（History の行の印の色。GTK4 版の `signature::Status` をまとめたもの）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signature {
    Verified,
    /// 鍵の信用度が決まっていない・署名者がコミッターと違う（黄色）。
    Caution,
    Expired,
    /// 失効した鍵・正しくない署名（赤）。
    Bad,
    /// 鍵がない・検証できない。
    Unknown,
}

impl Signature {
    /// `verifySignatures` の答えの 1 件から決める（GTK4 版の `signature::Status::new` と同じ判定）。
    pub fn of(check: &serde_json::Value) -> Self {
        let status = check["status"].as_str().unwrap_or_default();
        match status {
            "G" | "U" => {
                let signer = check["signer"].as_str().unwrap_or_default();
                let key = check["key"].as_str().unwrap_or_default();
                let ssh = !signer.contains('<') && key.starts_with("SHA256:");
                let level = if ssh {
                    3
                } else {
                    match check["trust"]
                        .as_str()
                        .unwrap_or_default()
                        .to_lowercase()
                        .as_str()
                    {
                        "ultimate" | "full" | "fully" => 3,
                        "marginal" => 2,
                        "never" => 0,
                        _ => 1,
                    }
                };
                if status == "U" || level < 2 || signer_mismatch(check) {
                    Self::Caution
                } else {
                    Self::Verified
                }
            }
            "X" | "Y" => Self::Expired,
            "R" | "B" => Self::Bad,
            _ => Self::Unknown,
        }
    }
}

/// 鍵のメールにコミッターのメールが（有効なものとして）ない（GTK4 版の `CommitterMatch::Mismatched`）。
fn signer_mismatch(check: &serde_json::Value) -> bool {
    let committer = check["committerEmail"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let emails: Vec<(String, String)> = check["emails"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            (
                e["email"].as_str().unwrap_or_default().to_lowercase(),
                e["validity"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    if emails.is_empty() || committer.is_empty() {
        return false;
    }
    !emails
        .iter()
        .any(|(e, v)| *e == committer && !matches!(v.as_str(), "r" | "e" | "n"))
}

/// 差分の `hunk` 行目（hunk の見出し）に続く、変わった行（次の hunk かファイルの見出しまで）。見出しでなければ空。
pub fn hunk_rows(rows: &[amonhen_layout::DiffRow], hunk: usize) -> Vec<usize> {
    use amonhen_layout::DiffRow;
    if rows.get(hunk).is_none_or(|r| r.kind != DiffRow::HUNK) {
        return Vec::new();
    }
    rows.iter()
        .enumerate()
        .skip(hunk + 1)
        .take_while(|(_, r)| !matches!(r.kind, DiffRow::HUNK | DiffRow::FILE))
        .filter(|(_, r)| matches!(r.kind, DiffRow::ADDED | DiffRow::REMOVED))
        .map(|(i, _)| i)
        .collect()
}

/// History の差分のファイル（見出しの右クリックのメニュー）。
#[derive(Clone, Debug, PartialEq)]
pub struct DiffFile {
    /// コミットの変更ファイルの順の番号。
    pub index: usize,
    pub path: String,
    pub collapsed: bool,
    /// 1 つのコミットの差分ならその ID（複数選んだときは None）。
    pub commit: Option<String>,
    /// そのコミットで消したファイル（Blame できない）。
    pub deleted: bool,
    /// そのコミットで足したファイル（その前の版はない）。
    pub added: bool,
    /// ファイル全体で見せている（見出しの ⋯ の Show Whole File）。
    pub whole: bool,
}

/// History の行に付ける ref の名前（ブランチ・リモートブランチ・タグ）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
    /// チェックアウト中のブランチ（worktree の札なら、書き込む先の worktree）。
    pub head: bool,
    /// worktree の HEAD の札（✎ ⌂ ⑂ と名前と変更の数。GTK4 版・macOS 版と同じ）。
    pub worktree: bool,
}

/// 行ごとのラベル。行の番号は `generation` の History のもの。
pub type Labels = HashMap<u32, Vec<RefLabel>>;

enum Event {
    /// 開いている Markdown の図を、色を当て直して描いたもの（開いたファイルの鍵と、図の絵）。
    Diagrams(u64, Vec<Option<crate::markdownview::DiagramPicture>>),
    /// 公開した History（永続キャッシュから出した前回のものなら `true`）。
    History(Arc<Repository>, Arc<HistorySnapshot>, bool),
    Refs(u64, Arc<RefsSnapshot>, Labels),
    Commit(u64, Box<CommitShown>),
    Syntax(u64, u32, u32, HashMap<u32, Vec<(u32, u32, u8)>>),
    /// 「すべてのファイル」で開いたファイルの、その時点の中身。
    File(Box<FileShown>),
    Extras(u64, u32, serde_json::Value),
    Status(Arc<StatusSnapshot>),
    Working(Box<WorkingShown>),
    /// History の検索の答え（History の世代、問い合わせ、一致した行）。
    Search(u64, String, Result<Vec<u32>, String>),
    /// 一致したコミットだけの History（元の History の世代、作った History、その行の ref の印、元の一致）。
    Filtered(u64, Arc<HistorySnapshot>, Labels, Vec<u32>),
    /// 署名の検証の結果（History の世代）。
    Signatures(u64, Vec<(amonhen_model::Oid, Signature)>),
    /// History の行のチャンク（History の世代、チャンクの番号、読めた行か読めない理由）。
    Chunk(u64, u32, Result<Chunk, String>),
    /// すべてのファイルで開いたファイル（`want_file_at` の番号）の中身が、partial clone で手元にない（その中身）。
    FileMissing(u64, Vec<amonhen_model::Oid>),
    /// すべてのファイルで開いたファイルの、手元になかった中身を取ってきた（`want_file_at` の番号、失敗の文）。
    FileFetched(u64, Result<(), String>),
    /// コミットの差分の、手元になかった中身を取ってきた（差分の番号、取り直したファイルの番号と差分）。
    StackedFetched(
        u64,
        Result<Vec<(usize, amonhen_core::diff::FileDiff)>, String>,
    ),
    Failed(String),
}

/// partial clone で手元にない中身を取ってくる様子（「取ってくる」の帯）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Fetch {
    #[default]
    Idle,
    Fetching,
    /// 取れなかった（その理由。帯のボタンはまた押せる）。
    Failed(String),
}

pub struct Data {
    pub history: Option<Arc<HistorySnapshot>>,
    pub refs: Option<Arc<RefsSnapshot>>,
    /// Working Copy の変更（index と作業ツリー）。
    pub status: Option<Arc<StatusSnapshot>>,
    /// Working Copy で選んだファイルの差分（届いたもの）と、読んでいるファイル。
    pub working: Option<Box<WorkingShown>>,
    working_wanted: Option<String>,
    /// 差分が届いた（見たら `false` に戻す）。
    pub working_changed: bool,
    /// 「すべてのファイル」で開いたファイルの中身（届いたもの）と、読んでいるファイル、届いたか（見たら false）。
    pub file_view: Option<Box<FileShown>>,
    file_wanted: Option<String>,
    pub file_view_changed: bool,
    /// `want_file_at` の番号（読み直すたびに増やす。古い答えを捨てる）と、そのコミット・変更（取ってきた後に
    /// 読み直す）。
    file_seq: u64,
    file_request: Option<(amonhen_model::Oid, Option<amonhen_model::FileChange>)>,
    /// partial clone か（すべてのファイルで開く前に、中身が手元にあるかを確かめる。窓がリポジトリの形から渡す）。
    pub partial: bool,
    /// すべてのファイルで開いたファイルの、partial clone で手元にない中身（帯で取ってくるまで）と、取ってくる様子。
    pub file_missing: Option<Vec<amonhen_model::Oid>>,
    pub file_fetch: Fetch,
    /// コミットの差分の、手元にない中身を取ってくる様子。
    pub commit_fetch: Fetch,
    /// 「取ってくる」の帯の様子が変わった（帯を出し直す。見たら `false` に戻す）。
    pub missing_changed: bool,
    /// 取ってきた中身でコミットの差分を作り直した（同じ所のまま描き直す。見たら `false` に戻す）。
    pub commit_refetched: bool,
    /// 差分の欄に、すべてのファイルで開いたファイルを出しているか。
    pub show_file: bool,
    /// 長い行を折り返すときの、差分の欄の幅と等幅の字の幅（DIP。折り返さないなら None）。
    wrap: Option<(f32, f32)>,
    /// 今の History の行の、ref のラベル。
    pub labels: Labels,
    /// 一致したコミットだけを出している間の、元の History とその一致・ref の印（検索の欄の Show Only Matching
    /// Commits。GTK4 版の `search::apply_filter`）。探すのはいつも元の History。
    base: Option<(Arc<HistorySnapshot>, Vec<u32>, Labels)>,
    /// 一致したコミットだけを出したい（一致が届いたら作る）。
    matching_only: bool,
    /// 署名の検証の結果と、検証している・これから検証するコミット（見えている行のものだけ、裏で）。
    pub signatures: HashMap<amonhen_model::Oid, Signature>,
    /// GitHub の CI の状態（コミットごと。窓が gh で読んで渡す。History の行の右端の印）。
    pub ci: HashMap<amonhen_model::Oid, crate::github::Ci>,
    verifying: HashSet<amonhen_model::Oid>,
    unverified: Vec<amonhen_model::Oid>,
    /// 今の History の行の、worktree の HEAD の札（ref のラベルの前に出す）と、その元（コミット、札の文字、
    /// 書き込む先か）。
    pub worktree_labels: Labels,
    worktree_heads: Vec<(amonhen_model::Oid, String, bool)>,
    /// 書き込む先・メインでない worktree が checkout しているブランチ（札を緑にする）。
    pub elsewhere: HashSet<String>,
    /// History の検索で一致した行（昇順）。描くときに塗る。
    pub matches: Vec<u32>,
    /// History で複数選んでいる行（昇順。2 つ以上のときだけ）。描くときに塗る。
    pub multi: Vec<u32>,
    /// History でコミットを運んでいる（描くときに、運ぶ先の印とつかんだ行を描く）。
    pub drag: Option<RowDrag>,
    /// 複数選んだときの読み込みの番号（`commit_wanted` の鍵の上位ビットを立てて使う）。
    selection_seq: u32,
    /// 探している問い合わせ。
    searching: Option<String>,
    /// 届いた検索の答え（見たら取る）。
    pub search: Option<SearchDone>,
    /// 選んだコミットの詳細と差分（届いたもの）。
    pub commit: Option<Box<CommitShown>>,
    /// 読んでいるコミットの行（届いたものがこれと違えば捨てる）。
    commit_wanted: Option<u32>,
    /// 最後に読んだ差分（空白の扱いを変えたとき、同じものを読み直す）。
    last_want: Option<Want>,
    /// Working Copy で読んだファイルと、両側に変更があるとき stage 済みの側を読んだか（空白の扱いを変えたら読み直す）。
    working_entry: Option<(amonhen_model::StatusEntry, bool)>,
    /// 行を比べるときに無視する空白（差分の帯で選ぶ。覚えない。macOS 版・GTK4 版と同じ）。
    pub whitespace: Whitespace,
    /// すべてのファイルで開いた SVG の見せ方と、モバイルで見る部品（パスの帯で選ぶ。覚えない）。
    pub svg_view: SvgView,
    pub mobile_renderer: amonhen_core::svglint::Target,
    /// 空白の扱いを変えて読み直すとき、届いた差分に戻す開き方。
    restore: Option<Restore>,
    /// 空白の扱いを変えて読み直した差分が届いた（同じ行でも出し直す。見たら `false` に戻す）。
    pub commit_reloaded: bool,
    /// 出している差分に構文の色が届いた（描き直す。見たら `false` に戻す）。
    pub syntax_changed: bool,
    /// 時間のかかる詳細が届いた（見たら `false` に戻す）。
    pub extras_changed: bool,
    /// History が替わった（行の番号が変わる。見たら `false` に戻す）。
    pub history_changed: bool,
    /// 差分より先に届いた、時間のかかる詳細。
    early_extras: Option<(u32, serde_json::Value)>,
    pub chunks: HashMap<u32, Rc<Chunk>>,
    repo: Option<Arc<Repository>>,
    loading: HashSet<u32>,
    /// 永続キャッシュから出した前回の History の世代（最新の History が届くまで）。
    cached: Option<u64>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    wake: Wake,
    changed_tx: Sender<u32>,
    /// History を位相順に並べるか（裏のスレッドと共有）。
    topo: Arc<std::sync::atomic::AtomicBool>,
    /// マージ済みブランチを折りたたむか、取り込んだ側も見せる merge（裏のスレッドと共有）。
    fold: Arc<std::sync::atomic::AtomicBool>,
    expanded: Arc<std::sync::Mutex<Vec<amonhen_model::Oid>>>,
    /// History の範囲（裏のスレッドと共有）。
    scope: Arc<std::sync::Mutex<Scope>>,
    /// 範囲を変えた（作り直した History が届いたら、選んだ行を見せる）。
    pub scope_changed: bool,
    /// History の上で編集している範囲の描き方（編集モードの間だけ。`render.rs` が描く）。
    pub edit: Option<crate::historyedit::Overlay>,
    /// 編集を始める前の範囲・折りたたみ・一致だけの表示（終えたら戻す）。
    before_edit: Option<(Scope, bool, bool)>,
}

/// History でコミットを運んでいる間のこと（macOS 版・GTK4 版の、コミットを運んで落とす）。
#[derive(Clone, Debug)]
pub struct RowDrag {
    /// つかんだ行。
    pub row: u32,
    /// 押した所の、行の上端からの高さと、今の指の高さ（History の窓の中の DIP）。
    pub offset: f32,
    pub y: f32,
    /// 押した所（少し動かしたら運び始める）。
    pub start: f32,
    /// 運び始めた。
    pub moving: bool,
    /// History の窓の高さ（描く行の範囲）。
    pub height: f32,
    /// 落とす先（行と、その行に重ねるか。重ねないなら行の上の境目）。
    pub target: Option<(u32, bool)>,
    /// 編集できる並び（HEAD から first-parent で辿ったもの。読み終えるまでは None。落とせない行を薄くする）。
    pub chain: Option<Arc<HashSet<amonhen_model::Oid>>>,
}

/// History の範囲（GTK4 版の `historydisplay::Scope`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// 全ブランチ。
    #[default]
    All,
    /// 今のブランチ（HEAD から辿れるもの）。
    Current,
    /// サイドバーで選んだ ref（種類と名前）。それぞれから辿れるコミット。
    Refs(Vec<(RefKind, String)>),
}

impl Scope {
    /// 起点のコミット（ref がなくなっていれば、その分は除く）。
    pub fn tips(&self, refs: &Refs) -> Vec<amonhen_model::Oid> {
        match self {
            Scope::All => Vec::new(),
            Scope::Current => refs.head.into_iter().collect(),
            Scope::Refs(list) => list
                .iter()
                .filter_map(|(kind, name)| {
                    refs.entries
                        .iter()
                        .find(|e| e.kind == *kind && &e.name == name)
                        .and_then(|e| e.target)
                })
                .collect(),
        }
    }
}

/// 変化の知らせのビットのうち、History を作り直す頼み（`amonhen_watch::Changes` が使わない上のビット）。
const REBUILD: u32 = 1 << 31;
/// 並び順を変えて History を並べ直す頼み（並び順は `Data::topo`）。
const REORDER: u32 = 1 << 30;
/// 読み直しのスレッドを終える頼み（`Data` を捨てた。タブを替えたときなど）。見張りが送り口を持っているので、送り口が
/// なくなるのを待っても終わらない。
const STOP: u32 = 1 << 29;

impl Drop for Data {
    fn drop(&mut self) {
        let _ = self.changed_tx.send(STOP);
    }
}

impl Data {
    /// `path` を裏のスレッドで開き、前回の History をすぐ送ってから最新を読む。
    pub fn open(path: PathBuf, wake: Wake) -> Self {
        let (tx, rx) = channel();
        let (thread_tx, thread_wake) = (tx.clone(), wake.clone());
        // 変化の知らせ（`amonhen_watch::Changes` のビット）。見張りのほか、Refresh などで自分でも送る。
        let (changed_tx, changed_rx) = channel::<u32>();
        let watch_tx = changed_tx.clone();
        // History の並び順（View の Date Order・Topological Order。設定は UI のスレッドで読む）。
        let topo = Arc::new(std::sync::atomic::AtomicBool::new(
            crate::prefs::history_order() == amonhen_model::HistoryOrder::Topo,
        ));
        let thread_topo = topo.clone();
        // マージ済みブランチを折りたたむか（View の Fold Merged Branches）と、取り込んだ側も見せる merge。
        let fold = Arc::new(std::sync::atomic::AtomicBool::new(crate::prefs::bool(
            crate::prefs::FOLD_MERGED_BRANCHES,
        )));
        let expanded: Arc<std::sync::Mutex<Vec<amonhen_model::Oid>>> = Arc::default();
        let (thread_fold, thread_expanded) = (fold.clone(), expanded.clone());
        // History の範囲（全ブランチ・今のブランチ・選んだ ref）。
        let scope: Arc<std::sync::Mutex<Scope>> = Arc::default();
        let thread_scope = scope.clone();
        // SSH 先ごとの選択（設定は UI のスレッドで読む）。
        let location = path.to_string_lossy().into_owned();
        // Settings > Performance で共有の daemon 越しに開くなら、手元も SSH 先も `daemon:` に（GTK4 版と同じ）。
        let location = if crate::prefs::bool(crate::prefs::OPEN_THROUGH_DAEMON)
            && !location.starts_with("daemon:")
        {
            if location.starts_with("ssh://") {
                format!("daemon:{location}")
            } else {
                amonhen_remote::local::canonical_path(&location)
                    .map_or(location, |p| format!("daemon:{p}"))
            }
        } else {
            location
        };
        let host_options = crate::remote::is_remote(&location).then(|| {
            (
                crate::remote::options_for(&location),
                crate::remote::checks_signatures_locally(&location),
            )
        });
        let order_of = move || {
            if thread_topo.load(std::sync::atomic::Ordering::Relaxed) {
                amonhen_model::HistoryOrder::Topo
            } else {
                amonhen_model::HistoryOrder::Date
            }
        };
        std::thread::spawn(move || {
            let send = |e| {
                let _ = thread_tx.send(e);
                thread_wake();
            };
            let opened = match host_options {
                Some((options, local)) => {
                    crate::remote::open(&location, options, local, &*thread_wake)
                }
                None => open(&path),
            };
            let repo = match opened {
                Ok(repo) => Arc::new(repo),
                Err(e) => return send(Event::Failed(format!("リポジトリを開けない: {e}"))),
            };
            // 並び順は読む前にコアへ（既定の日付順なら何もしない。GTK4 版と同じ）。
            let order = order_of();
            if order != amonhen_model::HistoryOrder::Date
                && let Err(e) = repo.set_history_order(order)
            {
                eprintln!("History の並び順を変えられない: {e}");
            }
            // 出す History: 折りたたむなら、マージ済みブランチを折りたたんだもの（GTK4 版の `reshape`）。作れない・
            // 空になったときは全体のまま。
            let shape = |full: Arc<HistorySnapshot>| -> Arc<HistorySnapshot> {
                let fold = thread_fold.load(std::sync::atomic::Ordering::Relaxed);
                // 範囲を絞るなら、その起点（ref は今の指す先。ほかのプログラムで動いても付け替わる）。
                let scope = thread_scope.lock().unwrap().clone();
                let tips: Vec<amonhen_model::Oid> = match &scope {
                    Scope::All => Vec::new(),
                    Scope::Current | Scope::Refs(_) => match repo.load_refs() {
                        Ok(refs) => scope.tips(&refs.refs),
                        Err(e) => {
                            eprintln!("ref を読めない: {e}");
                            Vec::new()
                        }
                    },
                };
                if !fold && tips.is_empty() {
                    return full;
                }
                let expanded = thread_expanded.lock().unwrap().clone();
                let some = (!tips.is_empty()).then_some(&tips[..]);
                let shaped = if fold {
                    repo.history_folded(some, true, &expanded)
                } else {
                    repo.history_filtered(&tips)
                };
                match shaped {
                    Ok(folded) if !(folded.graph.is_empty() && !full.graph.is_empty()) => folded,
                    Ok(_) => full,
                    Err(e) => {
                        eprintln!("History を折りたためない: {e}");
                        full
                    }
                }
            };
            if let Some(cached) = repo.load_cached_history() {
                send(Event::History(repo.clone(), shape(cached), true));
            }
            let history = match repo.load_history() {
                Ok(history) => shape(history),
                Err(e) => return send(Event::Failed(format!("History を読めない: {e}"))),
            };
            send(Event::History(repo.clone(), history.clone(), false));
            let refs = |history: &HistorySnapshot| match repo.load_refs() {
                Ok(refs) => {
                    // 行の索引（`row_of`）もここで作っておく（150 万コミットで 80 ms ほど）。
                    let labels = labels(history, &refs.refs);
                    Some(Event::Refs(history.generation, refs, labels))
                }
                Err(e) => Some(Event::Failed(format!("ref を読めない: {e}"))),
            };
            if let Some(event) = refs(&history) {
                send(event);
            }
            // Activity の Events に出すリポジトリの名前（作業ツリーのフォルダ、なければ .git の場所）。
            let name = {
                let paths = repo.paths();
                paths
                    .workdir
                    .as_deref()
                    .unwrap_or(&paths.git_dir)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            };
            let status = || match timed("Reload status", || repo.load_status()) {
                Ok(status) => Some(Event::Status(status)),
                Err(e) => {
                    eprintln!("status を読めない: {e}");
                    None
                }
            };
            if let Some(event) = status() {
                send(event);
            }

            // ref が変わったら History と ref を、設定が変わったら ref を読み直す（GTK4 版の `watch` と同じ）。
            // 知らせは監視のスレッドから来るので、このスレッドで受けて読む。読み直しの失敗は出さない（macOS 版と
            // 同じく、開いて最初の読み込みだけ）。
            // daemon 越し・SSH 先なら、daemon の知らせを受ける（GTK4 版と同じ）。
            let remote_tx = watch_tx.clone();
            let remote = repo.subscribe_changes(Box::new(move |bits| {
                let _ = remote_tx.send(bits);
            }));
            let _watcher = if remote {
                None
            } else {
                match amonhen_watch::Watcher::new(repo.paths(), move |changes| {
                    let _ = watch_tx.send(changes.0);
                }) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        eprintln!("リポジトリを見張れない: {e}");
                        return;
                    }
                }
            };
            let mut history = history;
            while let Ok(mut bits) = changed_rx.recv() {
                // まとめて来た知らせは 1 回の読み直しにする。
                while let Ok(more) = changed_rx.try_recv() {
                    bits |= more;
                }
                if bits & STOP != 0 {
                    break;
                }
                if bits & REORDER != 0 {
                    if let Err(e) = repo.set_history_order(order_of()) {
                        eprintln!("History を並べ直せない: {e}");
                    }
                    // 並べ直した History と、その行の ref の印を読み直す。
                    bits |= amonhen_watch::Changes::REFS.0;
                }
                let changes = amonhen_watch::Changes(bits);
                // 何が変わったかを Activity の Events に（GTK4 版と同じ言い方）。
                let what: Vec<String> = [
                    (amonhen_watch::Changes::REFS, "ref".to_owned()),
                    (amonhen_watch::Changes::INDEX, "index".to_owned()),
                    (
                        amonhen_watch::Changes::WORKTREE,
                        amonhen_model::tr!("working tree"),
                    ),
                    (amonhen_watch::Changes::CONFIG, amonhen_model::tr!("config")),
                ]
                .into_iter()
                .filter(|(bit, _)| changes.contains(*bit))
                .map(|(_, name)| name)
                .collect();
                if !what.is_empty() {
                    crate::activity::event(
                        &name,
                        amonhen_model::tr!(
                            "Changed: {what}",
                            what = what.join(&amonhen_model::tr!(", "))
                        ),
                    );
                }
                if bits & REBUILD != 0 {
                    // History の永続キャッシュを捨てて作り直す（View > Rebuild History）。
                    match repo.rebuild_history() {
                        Ok(h) => {
                            history = shape(h);
                            send(Event::History(repo.clone(), history.clone(), false));
                        }
                        Err(e) => eprintln!("History を作り直せない: {e}"),
                    }
                } else if changes.contains(amonhen_watch::Changes::REFS) {
                    match timed("Reload History", || repo.load_history()) {
                        Ok(h) => {
                            history = shape(h);
                            send(Event::History(repo.clone(), history.clone(), false));
                        }
                        Err(e) => eprintln!("History を読み直せない: {e}"),
                    }
                }
                if (changes.contains(amonhen_watch::Changes::REFS)
                    || changes.contains(amonhen_watch::Changes::CONFIG))
                    && let Some(event) = refs(&history)
                {
                    match event {
                        Event::Failed(e) => eprintln!("{e}"),
                        event => send(event),
                    }
                }
                // ref・index・作業ツリーが変わったら、Working Copy の status も。
                if (changes.contains(amonhen_watch::Changes::REFS)
                    || changes.contains(amonhen_watch::Changes::INDEX)
                    || changes.contains(amonhen_watch::Changes::WORKTREE))
                    && let Some(event) = status()
                {
                    send(event);
                }
            }
        });
        Self {
            history: None,
            refs: None,
            status: None,
            working: None,
            working_wanted: None,
            working_changed: false,
            file_view: None,
            file_wanted: None,
            file_view_changed: false,
            show_file: false,
            wrap: None,
            labels: HashMap::new(),
            signatures: HashMap::new(),
            ci: HashMap::new(),
            verifying: HashSet::new(),
            unverified: Vec::new(),
            worktree_labels: HashMap::new(),
            worktree_heads: Vec::new(),
            elsewhere: HashSet::new(),
            matches: Vec::new(),
            base: None,
            matching_only: false,
            multi: Vec::new(),
            selection_seq: 0,
            searching: None,
            search: None,
            commit: None,
            commit_wanted: None,
            last_want: None,
            working_entry: None,
            whitespace: Whitespace::Exact,
            svg_view: SvgView::Picture,
            mobile_renderer: amonhen_core::svglint::Target::Android,
            restore: None,
            commit_reloaded: false,
            file_seq: 0,
            file_request: None,
            partial: false,
            file_missing: None,
            file_fetch: Fetch::Idle,
            commit_fetch: Fetch::Idle,
            missing_changed: false,
            commit_refetched: false,
            syntax_changed: false,
            extras_changed: false,
            history_changed: false,
            early_extras: None,
            chunks: HashMap::new(),
            repo: None,
            loading: HashSet::new(),
            cached: None,
            tx,
            rx,
            wake,
            changed_tx,
            topo,
            fold,
            expanded,
            scope,
            scope_changed: false,
            edit: None,
            before_edit: None,
            drag: None,
        }
    }

    /// History と ref を読み直す（View > Refresh）。
    pub fn reload(&self) {
        let all = amonhen_watch::Changes::REFS.0 | amonhen_watch::Changes::CONFIG.0;
        let _ = self.changed_tx.send(all);
    }

    /// 出す History を替える（行の番号が変わるので、行ごとのものは捨てる）。
    fn show(&mut self, history: Arc<HistorySnapshot>, labels: Labels, matches: Vec<u32>) {
        self.history = Some(history);
        self.history_changed = true;
        self.labels = labels;
        self.place_worktrees();
        // 署名の結果はコミットのものなので残すが、検証中のものは新しい世代で頼み直す。
        self.verifying.clear();
        self.matches = matches;
        self.multi.clear();
        self.commit = None;
        self.commit_wanted = None;
        self.chunks.clear();
        self.loading.clear();
    }

    /// 探す History（一致したコミットだけを出している間も、元の History）。
    fn searched(&self) -> Option<&Arc<HistorySnapshot>> {
        self.base
            .as_ref()
            .map(|(h, _, _)| h)
            .or(self.history.as_ref())
    }

    /// 一致したコミットだけを出しているか。
    pub fn is_filtered(&self) -> bool {
        self.base.is_some()
    }

    /// 一致したコミットだけを出すか（一致がなければ元の History のまま。GTK4 版と同じ）。
    pub fn set_matching_only(&mut self, on: bool) {
        self.matching_only = on;
        if !on {
            if let Some((history, matches, labels)) = self.base.take() {
                self.show(history, labels, matches);
            }
            return;
        }
        if self.base.is_none() {
            let matches = self.matches.clone();
            self.filter(matches);
        }
    }

    /// 一致したコミットだけの History を裏で作る（linux の 25 万件で 50 ms ほど。GTK4 版と同じく裏で）。
    fn filter(&mut self, matches: Vec<u32>) {
        let (Some(repo), Some(base)) = (self.repo.clone(), self.searched().cloned()) else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        let refs = self.refs.clone();
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let filtered = repo.history_matching(&base, &matches);
            let labels = refs.map_or_else(Labels::new, |r| labels(&filtered, &r.refs));
            let _ = tx.send(Event::Filtered(base.generation, filtered, labels, matches));
            wake();
        });
    }

    /// History を `query`（コアの問い合わせの書き方）で探す。答えは `search` に届く。
    pub fn search(&mut self, query: String) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.searched().cloned()) else {
            return;
        };
        self.searching = Some(query.clone());
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let rows = repo
                .search_history(&history, &query)
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Search(history.generation, query, rows));
            wake();
        });
    }

    /// Working Copy のファイルの差分を裏で読む。1 本の差分で表せれば HEAD と作業ツリーの差分、表せなければ変更の
    /// ある側（両方なら作業ツリーの側）。`None` なら出さない。
    /// `prefer_staged`: 両側に変更があって 1 本の差分で表せないとき、stage 済みの側を読むか。
    pub fn want_working(&mut self, entry: Option<amonhen_model::StatusEntry>, prefer_staged: bool) {
        let Some(entry) = entry else {
            self.working_wanted = None;
            self.working = None;
            self.working_entry = None;
            self.working_changed = true;
            return;
        };
        let Some(repo) = self.repo.clone() else {
            return;
        };
        self.working_wanted = Some(entry.path.clone());
        self.working_entry = Some((entry.clone(), prefer_staged));
        let whitespace = self.whitespace;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            // 空白を無視する間は 1 本の差分（行ごとのチェック）にせず、選んだ側の差分（macOS 版と同じ）。
            let combined = if whitespace == Whitespace::Exact {
                repo.working_diff_combined(&entry, &PROFILE).ok().flatten()
            } else {
                None
            };
            let diff = match combined {
                Some(diff) => Ok(diff),
                // 1 本の差分で表せなければ、選んだ側（片側にしか変更がなければその側。GTK4 版と同じ）。
                None => {
                    let staged = if entry.is_staged() && entry.is_unstaged() {
                        prefer_staged
                    } else {
                        !entry.is_unstaged()
                    };
                    repo.working_diff_ws(&entry, staged, whitespace, &PROFILE)
                }
            };
            let event = match diff {
                Ok(diff) => {
                    let (highlights, syntax) = (highlights_of(&diff.view), syntax_of(&diff.view));
                    Event::Working(Box::new(WorkingShown {
                        path: entry.path.clone(),
                        diff: Arc::new(diff),
                        highlights,
                        syntax,
                        tree: None,
                        wrap_cols: 0,
                        selection: Default::default(),
                        anchor: None,
                    }))
                }
                Err(e) => {
                    eprintln!("{} の差分を読めない: {e}", entry.path);
                    return;
                }
            };
            let _ = tx.send(event);
            wake();
        });
    }

    /// Working Copy の差分の `row` 行目（変更の行）を、次のコミットに含める・外す（今と逆にする）。終わったら status を
    /// 読み直し、差分も読み直される。
    pub fn toggle_working_row(&self, row: usize) {
        let Some(working) = &self.working else { return };
        let Some(staged) = working
            .diff
            .combined
            .as_ref()
            .and_then(|c| c.get(row).copied())
        else {
            return;
        };
        let (Some(repo), diff, changed) = (
            self.repo.clone(),
            working.diff.clone(),
            self.changed_tx.clone(),
        ) else {
            return;
        };
        std::thread::spawn(move || {
            if let Err(e) = repo.set_rows_staged(&diff, &[row], !staged) {
                eprintln!("行を含められない: {e}");
            }
            let _ = changed.send(amonhen_watch::Changes::INDEX.0);
        });
    }

    /// Working Copy の差分の `row` の行を含めているか（変わった行でなければ None）。
    pub fn working_row_staged(&self, row: usize) -> Option<bool> {
        use amonhen_layout::DiffRow;
        let working = self.working.as_ref()?;
        let kind = working.diff.view.rows.get(row)?.kind;
        if !matches!(kind, DiffRow::ADDED | DiffRow::REMOVED) {
            return None;
        }
        working.diff.combined.as_ref()?.get(row).copied()
    }

    /// Working Copy の差分の `a`〜`b` の変わった行を、まとめて `stage`（含める・外す）にする（印の上を引いた
    /// ときと Shift+クリック。GTK4 版の印の範囲）。
    pub fn set_working_range(&self, a: usize, b: usize, stage: bool) {
        use amonhen_layout::DiffRow;
        let Some(working) = &self.working else { return };
        let Some(combined) = working.diff.combined.as_ref() else {
            return;
        };
        let rows: Vec<usize> = (a.min(b)..=a.max(b))
            .filter(|&r| {
                working
                    .diff
                    .view
                    .rows
                    .get(r)
                    .is_some_and(|x| matches!(x.kind, DiffRow::ADDED | DiffRow::REMOVED))
                    && combined.get(r).copied() != Some(stage)
            })
            .collect();
        if rows.is_empty() {
            return;
        }
        let (Some(repo), diff, changed) = (
            self.repo.clone(),
            working.diff.clone(),
            self.changed_tx.clone(),
        ) else {
            return;
        };
        std::thread::spawn(move || {
            if let Err(e) = repo.set_rows_staged(&diff, &rows, stage) {
                eprintln!("行を含められない: {e}");
            }
            let _ = changed.send(amonhen_watch::Changes::INDEX.0);
        });
    }

    /// Working Copy の差分の hunk の見出し（`row`）の行をまとめて含める・外す（GTK4 版の hunk の印）。1 行でも
    /// 含めていなければすべて含め、すべて含めていればすべて外す。
    pub fn toggle_working_hunk(&self, row: usize) {
        let Some(working) = &self.working else { return };
        let Some(combined) = working.diff.combined.as_ref() else {
            return;
        };
        let rows = hunk_rows(&working.diff.view.rows, row);
        if rows.is_empty() {
            return;
        }
        let stage = rows
            .iter()
            .any(|&r| !combined.get(r).copied().unwrap_or(false));
        let (Some(repo), diff, changed) = (
            self.repo.clone(),
            working.diff.clone(),
            self.changed_tx.clone(),
        ) else {
            return;
        };
        std::thread::spawn(move || {
            if let Err(e) = repo.set_rows_staged(&diff, &rows, stage) {
                eprintln!("hunk を含められない: {e}");
            }
            let _ = changed.send(amonhen_watch::Changes::INDEX.0);
        });
    }

    /// Working Copy の差分で右クリックした `row` の行（変わった行ならその行、hunk の見出しならその hunk の行）と、
    /// そのうちに含めている行があるか（含めている行は捨てられない）。stage 済みの側の差分では None。
    pub fn working_rows_at(&self, row: usize) -> Option<(Vec<usize>, bool)> {
        // 空白を無視した差分の行は中身をそのまま表していないので、行ごとには捨てない（macOS 版と同じ）。
        if self.whitespace != Whitespace::Exact {
            return None;
        }
        use amonhen_layout::DiffRow;
        let working = self.working.as_ref()?;
        if working.diff.staged {
            return None;
        }
        // 選んだ行の中を押したら、選んだ変更の行すべて（macOS 版・GTK4 版の Discard Lines）。
        let selected = working.selected_changes();
        let rows = match working.diff.view.rows.get(row)?.kind {
            _ if working.selection.contains(&(row as u32)) && !selected.is_empty() => selected,
            DiffRow::ADDED | DiffRow::REMOVED => vec![row],
            DiffRow::HUNK => hunk_rows(&working.diff.view.rows, row),
            _ => return None,
        };
        let staged = working
            .diff
            .combined
            .as_ref()
            .is_some_and(|c| rows.iter().any(|&r| c.get(r).copied().unwrap_or(false)));
        (!rows.is_empty()).then_some((rows, staged))
    }

    /// Working Copy の今の差分（行を捨てるのに使う）。
    pub fn working_diff(&self) -> Option<(Arc<Repository>, Arc<amonhen_core::WorkingDiff>)> {
        Some((self.repo.clone()?, self.working.as_ref()?.diff.clone()))
    }

    /// Working Copy の status を読み直す（stage・コミットの後。見張りが拾う前に出す）。
    pub fn reload_status(&self) {
        let _ = self.changed_tx.send(amonhen_watch::Changes::INDEX.0);
    }

    /// History の並び順を変えて並べ直す（View の Date Order・Topological Order）。
    pub fn reorder(&self, order: amonhen_model::HistoryOrder) {
        self.topo.store(
            order == amonhen_model::HistoryOrder::Topo,
            std::sync::atomic::Ordering::Relaxed,
        );
        let _ = self.changed_tx.send(REORDER);
    }

    /// History の範囲。
    pub fn scope(&self) -> Scope {
        self.scope.lock().unwrap().clone()
    }

    /// History の範囲を変えて作り直す（GTK4 版の `set_scope`）。
    pub fn set_scope(&mut self, scope: Scope) {
        // 編集の間は、今のブランチの全部の行のまま（サイドバーで ref を選んでも範囲は変えない）。
        if self.before_edit.is_none() {
            self.apply_scope(scope);
        }
    }

    fn apply_scope(&mut self, scope: Scope) {
        {
            let mut current = self.scope.lock().unwrap();
            if *current == scope {
                return;
            }
            *current = scope;
        }
        self.scope_changed = true;
        let _ = self.changed_tx.send(REORDER);
    }

    /// マージ済みブランチを折りたたむかを変えて、出し直す（View の Fold Merged Branches）。
    pub fn set_fold(&self, value: bool) {
        self.fold.store(value, std::sync::atomic::Ordering::Relaxed);
        let _ = self.changed_tx.send(REORDER);
    }

    /// 折りたたんだ History での、コミット `id` の merge の隠しているコミットの数（取り込んだ側を見せていれば 0。
    /// 右クリックの Show n Merged Commits・Fold Merged Commits。どちらでもなければ None）。
    pub fn merge_fold(&self, id: &str) -> Option<u32> {
        if !self.fold.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        let oid = amonhen_model::Oid::from_hex(id)?;
        let history = self.history.as_ref()?;
        let row = history.row_of(&oid)?;
        let hidden = history.folded.get(row as usize).copied().unwrap_or(0);
        let open = self.expanded.lock().unwrap().contains(&oid);
        (hidden > 0 || open).then_some(hidden)
    }

    /// 折りたたんだ merge の取り込んだ側を見せる・たたむ。
    pub fn toggle_merge(&self, id: &str) {
        let Some(oid) = amonhen_model::Oid::from_hex(id) else {
            return;
        };
        {
            let mut expanded = self.expanded.lock().unwrap();
            match expanded.iter().position(|e| *e == oid) {
                Some(i) => {
                    expanded.remove(i);
                }
                None => expanded.push(oid),
            }
        }
        let _ = self.changed_tx.send(REORDER);
    }

    /// History の永続キャッシュを捨てて作り直し、ref も読み直す（View > Rebuild History）。
    pub fn rebuild(&self) {
        let _ = self
            .changed_tx
            .send(REBUILD | amonhen_watch::Changes::CONFIG.0);
    }

    /// 届いたものを取り込む。History が替わったか、チャンクが届いたら `true`。
    pub fn poll(&mut self) -> Result<bool, String> {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::History(repo, history, cached) => {
                    self.cached = cached.then_some(history.generation);
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| Arc::ptr_eq(h, &history))
                    {
                        continue;
                    }
                    self.repo = Some(repo);
                    // 新しい History では、一致したコミットだけの History も作り直す（探し直すのは FE）。
                    self.base = None;
                    self.show(history, Labels::new(), Vec::new());
                    changed = true;
                }
                Event::Chunk(generation, index, chunk) => {
                    let current = self.history.as_ref().map(|h| h.generation);
                    if let Some(chunk) = current_chunk(current, self.cached, generation, chunk)? {
                        self.loading.remove(&index);
                        self.chunks.insert(index, Rc::new(chunk));
                        changed = true;
                    }
                }
                Event::Signatures(generation, found) => {
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation)
                    {
                        for (id, signature) in found {
                            self.verifying.remove(&id);
                            self.signatures.insert(id, signature);
                        }
                        changed = true;
                    }
                }
                Event::Refs(generation, refs, labels) => {
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation)
                    {
                        self.labels = labels;
                    } else if let Some((h, _, base_labels)) = &mut self.base
                        && h.generation == generation
                    {
                        *base_labels = labels;
                    }
                    self.refs = Some(refs);
                    changed = true;
                }
                Event::Commit(generation, shown) => {
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation)
                        && self.commit_wanted == Some(shown.row)
                    {
                        let mut shown = shown;
                        if let Some((row, extras)) = self.early_extras.take()
                            && row == shown.row
                        {
                            shown.set_extras(extras);
                        }
                        self.commit = Some(shown);
                        // 次のコミットではまた聞く（覚えない。macOS 版と同じ）。
                        self.commit_fetch = Fetch::Idle;
                        self.apply_wrap();
                        changed = true;
                        // 空白の扱いを変えて読み直した: 開いていたファイル・広げた前後・ファイル全体・畳んだ
                        // フォルダを戻す。
                        if let Some(restore) = self.restore.take() {
                            self.restore_layout(restore);
                            self.commit_reloaded = true;
                        }
                    }
                }
                Event::Syntax(generation, row, version, syntax) => {
                    if let Some(commit) = &mut self.commit
                        && commit.row == row
                        && commit.version == version
                        && self
                            .history
                            .as_ref()
                            .is_some_and(|h| h.generation == generation)
                    {
                        commit.syntax = syntax;
                        self.syntax_changed = true;
                        changed = true;
                    }
                }
                Event::Extras(generation, row, extras) => {
                    let current = self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation);
                    if current && self.commit_wanted == Some(row) {
                        match &mut self.commit {
                            Some(commit) if commit.row == row => commit.set_extras(extras),
                            // 差分より先に届いた。差分が届いたときに足す。
                            _ => self.early_extras = Some((row, extras)),
                        }
                        self.extras_changed = true;
                        changed = true;
                    }
                }
                Event::Search(generation, query, rows) => {
                    let current = self.searched().is_some_and(|h| h.generation == generation);
                    if current && self.searching.as_deref() == Some(query.as_str()) {
                        self.searching = None;
                        // 一致したコミットだけを出している間に探し直したら、元の History に戻してから作り直す。
                        if let Some((history, _, labels)) = self.base.take() {
                            self.show(history, labels, Vec::new());
                        }
                        self.search = Some(match rows {
                            Ok(rows) => {
                                self.matches = rows;
                                if self.matching_only {
                                    self.filter(self.matches.clone());
                                }
                                SearchDone::Found(query)
                            }
                            Err(e) => {
                                self.matches.clear();
                                SearchDone::Failed(query, e)
                            }
                        });
                        changed = true;
                    }
                }
                Event::Filtered(generation, filtered, labels, matches) => {
                    let current = self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation);
                    if current && self.matching_only && self.base.is_none() {
                        let base = self.history.clone().expect("History");
                        let base_labels = std::mem::take(&mut self.labels);
                        self.base = Some((base, matches, base_labels));
                        // すべて一致なので塗らない（次・前は行を 1 つずつ。GTK4 版と同じ）。
                        let rows = (0..filtered.graph.len() as u32).collect();
                        self.show(filtered, labels, rows);
                        changed = true;
                    }
                }
                Event::Diagrams(key, diagrams) => {
                    if let Some(file) = self.file_view.as_mut().filter(|f| f.key == key) {
                        file.diagrams = Arc::new(diagrams);
                        self.file_view_changed = true;
                        changed = true;
                    }
                }
                Event::File(shown) => {
                    if self.file_wanted.as_deref() == Some(shown.path.as_str()) {
                        self.file_view = Some(shown);
                        self.apply_wrap();
                        self.file_view_changed = true;
                        changed = true;
                    }
                }
                Event::Working(shown) => {
                    if self.working_wanted.as_deref() == Some(shown.path.as_str()) {
                        self.working = Some(shown);
                        self.apply_wrap();
                        self.working_changed = true;
                        changed = true;
                    }
                }
                Event::Status(status) => {
                    if !self
                        .status
                        .as_ref()
                        .is_some_and(|s| Arc::ptr_eq(s, &status))
                    {
                        self.status = Some(status);
                        changed = true;
                    }
                }
                Event::FileMissing(seq, ids) => {
                    if seq == self.file_seq {
                        self.file_missing = Some(ids);
                        self.file_view_changed = true;
                        self.missing_changed = true;
                        changed = true;
                    }
                }
                Event::FileFetched(seq, result) => {
                    if seq == self.file_seq {
                        match result {
                            // 取れたら、そのファイルを読み直して出す。
                            Ok(()) => {
                                if let (Some((commit, change)), Some(path)) =
                                    (self.file_request.clone(), self.file_wanted.clone())
                                {
                                    self.want_file_at(commit, change, Some(path));
                                }
                            }
                            Err(e) => self.file_fetch = Fetch::Failed(e),
                        }
                        self.missing_changed = true;
                        changed = true;
                    }
                }
                Event::StackedFetched(id, result) => {
                    if self.commit.as_ref().is_some_and(|c| c.id == id) {
                        match result {
                            Ok(files) => {
                                self.commit_fetch = Fetch::Idle;
                                let repo = self.repo.clone();
                                self.relayout(|commit| commit.take_fetched(files, repo.as_deref()));
                                self.commit_refetched = true;
                            }
                            Err(e) => self.commit_fetch = Fetch::Failed(e),
                        }
                        self.missing_changed = true;
                        changed = true;
                    }
                }
                Event::Failed(e) => return Err(e),
            }
        }
        Ok(changed)
    }

    /// 長い行を折り返す（`wrap` は差分の欄の幅と等幅の字の幅、None なら折り返さない）。出している差分・
    /// Working Copy の差分・すべてのファイルの中身の表示の行を作り直す。変わったら true。
    pub fn set_wrap(&mut self, wrap: Option<(f32, f32)>) -> bool {
        if self.wrap == wrap {
            return false;
        }
        self.wrap = wrap;
        self.apply_wrap()
    }

    /// 今の折り返しの設定を、出しているものに当てる。変わったら true。
    fn apply_wrap(&mut self) -> bool {
        let wrap = self.wrap;
        let cols = move |marks: bool, file_mode: bool| match wrap {
            Some((width, char_width)) if char_width > 0.0 => {
                let left = crate::render::text_left(marks, file_mode);
                ((width - left - 12.0) / char_width).floor().max(0.0) as usize
            }
            _ => 0,
        };
        let mut changed = false;
        let commit_cols = cols(false, false);
        if self
            .commit
            .as_ref()
            .is_some_and(|c| c.wrap_cols != commit_cols)
        {
            changed |= self.relayout(|commit| {
                commit.wrap_cols = commit_cols;
                true
            });
        }
        let file_cols = cols(false, true);
        if let Some(file) = self.file_view.as_mut()
            && file.wrap_cols != file_cols
        {
            file.wrap_cols = file_cols;
            file.tree = wrapped(&file.view, Some(file.base.clone()), file_cols)
                .unwrap_or_else(|| file.base.clone());
            changed = true;
        }
        if let Some(working) = self.working.as_mut() {
            let working_cols = cols(working.diff.combined.is_some(), false);
            if working.wrap_cols != working_cols {
                working.wrap_cols = working_cols;
                working.tree = wrapped(&working.diff.view, None, working_cols);
                changed = true;
            }
        }
        changed
    }

    /// 出している差分が 1 つのコミットなら、その ID（すべてのファイルを見られる。stash・複数・範囲では None）。
    pub fn single_commit(&self) -> Option<amonhen_model::Oid> {
        match self.last_want {
            Some(Want::Commit(_)) | Some(Want::Oid(_, false)) | Some(Want::FileChange(..)) => {
                let oid = self.commit.as_ref()?.info["oid"].as_str()?;
                amonhen_model::Oid::from_hex(oid)
            }
            _ => None,
        }
    }

    /// 差分の欄に描くもの（History なら、すべてのファイルで開いたファイルか、コミットの差分）。
    pub fn shown_content(&self) -> Option<DiffContent<'_>> {
        if self.show_file {
            self.file_view.as_deref().map(|f| f.content())
        } else {
            self.commit.as_deref().map(|c| c.content())
        }
    }

    /// 「すべてのファイル」で、コミット `commit` の時点の `path` の中身を裏で読む。このコミットで変えたファイル
    /// （`change`）ならファイル全体を前後に持つ差分（変えた行に印を付ける）、変えていなければそのままの中身。
    /// `path` が None なら閉じる。
    pub fn want_file_at(
        &mut self,
        commit: amonhen_model::Oid,
        change: Option<amonhen_model::FileChange>,
        path: Option<String>,
    ) {
        self.file_view = None;
        self.file_view_changed = true;
        self.file_wanted = path.clone();
        self.file_seq += 1;
        self.file_request = path.as_ref().map(|_| (commit, change.clone()));
        if self.file_missing.take().is_some() || self.file_fetch != Fetch::Idle {
            self.missing_changed = true;
        }
        self.file_fetch = Fetch::Idle;
        let (Some(path), Some(repo)) = (path, self.repo.clone()) else {
            return;
        };
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let (seq, partial) = (self.file_seq, self.partial);
        let (svg_view, renderer) = (self.svg_view, self.mobile_renderer);
        // VectorDrawable の色は、暗い外観なら values-night などを先に引く
        let dark = crate::palette::dark();
        // Markdown の図に当てる色の元は、テーマを持つ UI のスレッドで取っておく。
        let diagram_theme =
            amonhen_core::markdown::is_markdown(&path).then(crate::markdownview::diagram_theme);
        // gantt の日付を数える時間帯（Settings > Theme。macOS 版・GTK4 版と同じ）。
        let zone = crate::timezone::gantt_zone();
        std::thread::spawn(move || {
            // partial clone で中身が手元になければ、取りに行かずに「取ってくる」の帯で聞く（押したときだけ取る。
            // macOS 版の `CommitFilesBrowser.open`）。
            if partial {
                match repo.file_missing(&commit, &path) {
                    Ok(ids) if !ids.is_empty() => {
                        let _ = tx.send(Event::FileMissing(seq, ids));
                        return wake();
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("中身が手元にあるかを確かめられない（{path}）: {e}"),
                }
            }
            // 画像は、その時点の中身を絵で見せる（読めなければ、ほかのファイルと同じく）。SVG はテキストでも
            // 見られる。
            let svg = crate::imagediff::is_svg(&path);
            let picture = (crate::imagediff::is_picture(&path)
                && !(svg && svg_view == SvgView::Text))
                .then(|| blob_at(&repo, &commit, change.as_ref(), &path))
                .flatten();
            // .xml は中身の頭を見て、Android の VectorDrawable なら SVG にして絵で見せる（色の参照は同じコミットの
            // res から引く。テキストで見るなら絵にしない。描けなければテキスト。GTK4 版の `show_drawable`）。
            let mut drawable = None;
            let mut picture = picture;
            if amonhen_core::vectordrawable::is_xml(&path)
                && let Some(data) = blob_at(&repo, &commit, change.as_ref(), &path)
                && amonhen_core::vectordrawable::is_vector_drawable(&path, &data)
            {
                let drawn = (svg_view != SvgView::Text)
                    .then(|| repo.vector_drawable(&commit, &path, &data, None, dark));
                match drawn {
                    Some(Ok(drawn)) => {
                        let (w, h) = crate::imagediff::svg_size(drawn.svg.as_bytes());
                        drawable = Some(Drawable {
                            caption: format!(
                                "{w} × {h} dp · {}",
                                crate::imagediff::format_size(data.len())
                            ),
                            notes: drawn.notes,
                        });
                        picture = Some(Arc::new(drawn.svg.into_bytes()));
                    }
                    Some(Err(e)) => eprintln!("VectorDrawable を描けない（{path}）: {e}"),
                    None => {
                        drawable = Some(Drawable {
                            caption: String::new(),
                            notes: Vec::new(),
                        })
                    }
                }
            }
            // モバイルで見る SVG は、崩れる機能と、その部品での見え方も（Android と flutter_svg は本物の取り込みを
            // 移したもの、ほかは近似。GTK4 版と同じ）。
            let mobile = picture
                .as_deref()
                .filter(|_| svg && svg_view == SvgView::Mobile)
                .map(|data| {
                    Arc::new(MobileCheck {
                        issues: amonhen_core::svglint::lint(data),
                        preview: amonhen_core::svgmobile::preview(data, renderer),
                    })
                });
            // Markdown は、整形して見せるブロックも読む（macOS 版の `MarkdownView`）。
            let markdown = amonhen_core::markdown::is_markdown(&path)
                .then(|| blob_at(&repo, &commit, change.as_ref(), &path))
                .flatten()
                .map(|bytes| {
                    // 図（DOT）の中の画像も、同じコミットから読む（GTK4 版と同じ）。
                    let images = amonhen_core::diagram::ImageSource {
                        from: path.clone(),
                        read: {
                            let repo = repo.clone();
                            Arc::new(move |p: &str| {
                                blob_at(&repo, &commit, None, p).map(|b| b.to_vec())
                            })
                        },
                    };
                    amonhen_core::markdown::parse_with_images(
                        &String::from_utf8_lossy(bytes.as_slice()),
                        None,
                        Some(zone.clone()),
                        Some(images),
                    )
                });
            // Markdown の中の画像は、このコミットの中のものだけを読んでおく（外へは読みに行かない）。
            let images = markdown.as_deref().map_or_else(Default::default, |blocks| {
                crate::markdownview::load_images(blocks, &path, &|p| {
                    blob_at(&repo, &commit, None, p)
                })
            });
            // 図は、ここ（裏）で色を当てて絵にしておく（大きな図は数十 ms かかる）。
            let diagrams = match (&markdown, &diagram_theme) {
                (Some(blocks), Some(colors)) => {
                    crate::markdownview::render_diagrams(blocks, colors)
                }
                _ => Vec::new(),
            };
            let view = match &change {
                Some(c) => repo.file_diff_with_context(c, u32::MAX, &PROFILE),
                None => repo.tree_file(&commit, &path, &PROFILE),
            };
            let view = match view {
                Ok(view) => view,
                // 確かめた後に中身がなかった（partial clone）: 同じく帯で聞く。
                Err(e) if amonhen_model::MissingContents::of(&*e).is_some() => {
                    let ids = amonhen_model::MissingContents::of(&*e)
                        .map(|m| m.ids.clone())
                        .unwrap_or_default();
                    let _ = tx.send(Event::FileMissing(seq, ids));
                    return wake();
                }
                Err(e) => {
                    eprintln!("その時点のファイルを読めない（{path}）: {e}");
                    return;
                }
            };
            let syntax = syntax_of(&view);
            let base = file_lines(&view);
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);
            let _ = tx.send(Event::File(Box::new(FileShown {
                path,
                view,
                syntax,
                tree: base.clone(),
                base,
                wrap_cols: 0,
                highlights: HashMap::new(),
                picture,
                picture_view: crate::imagediff::View::default(),
                key: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                markdown,
                diagrams: Arc::new(diagrams),
                images: Arc::new(images),
                mobile,
                drawable,
            })));
            wake();
        });
    }

    /// 開いている Markdown の図に、今の配色・外観・テーマで色を当て直す（裏で描き、届くまでは前の絵のまま。
    /// macOS 版・GTK4 版と同じく、図だけを描き直す）。
    pub fn recolor_diagrams(&mut self) {
        let Some(file) = self.file_view.as_ref() else {
            return;
        };
        let Some(blocks) = file.markdown.clone().filter(|_| !file.diagrams.is_empty()) else {
            return;
        };
        let (key, colors) = (file.key, crate::markdownview::diagram_theme());
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let diagrams = crate::markdownview::render_diagrams(&blocks, &colors);
            let _ = tx.send(Event::Diagrams(key, diagrams));
            wake();
        });
    }

    /// 出しているコミットの差分の、partial clone で中身が手元になく差分を取っていないファイルの数。
    pub fn missing_files(&self) -> usize {
        self.commit
            .as_ref()
            .map_or(0, |c| c.stacked.missing_files())
    }

    /// 帯の「取ってくる」（コミットの差分）: 手元になかった中身をまとめて 1 回で取り、それらのファイルの差分を
    /// 取る（裏で。届いたら同じ所のまま出し直す）。
    pub fn fetch_commit_missing(&mut self) {
        let (Some(repo), Some(commit)) = (self.repo.clone(), self.commit.as_ref()) else {
            return;
        };
        if self.commit_fetch == Fetch::Fetching || commit.stacked.missing_files() == 0 {
            return;
        }
        // 取っている間も今の差分を見せるので、写したものに取る（取り直したファイルだけを戻す）。
        let mut stacked = StackedDiff {
            files: commit.stacked.files.clone(),
        };
        let id = commit.id;
        self.commit_fetch = Fetch::Fetching;
        self.missing_changed = true;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let before: Vec<bool> = stacked
                .files
                .iter()
                .map(|(_, d)| !d.missing.is_empty())
                .collect();
            let result = repo
                .fetch_stacked_missing(&mut stacked)
                .map(|()| {
                    stacked
                        .files
                        .into_iter()
                        .enumerate()
                        .filter(|(i, (_, d))| before[*i] && d.missing.is_empty())
                        .map(|(i, (_, d))| (i, d))
                        .collect()
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::StackedFetched(id, result));
            wake();
        });
    }

    /// 帯の「取ってくる」（すべてのファイルで開いたファイル）: 手元になかった中身をまとめて 1 回で取り、取れたら
    /// そのファイルを読み直して出す（裏で）。
    pub fn fetch_file_missing(&mut self) {
        let (Some(repo), Some(ids)) = (self.repo.clone(), self.file_missing.clone()) else {
            return;
        };
        if self.file_fetch == Fetch::Fetching {
            return;
        }
        self.file_fetch = Fetch::Fetching;
        self.missing_changed = true;
        let (tx, wake, seq) = (self.tx.clone(), self.wake.clone(), self.file_seq);
        std::thread::spawn(move || {
            let result = repo.fetch_contents(&ids).map_err(|e| e.to_string());
            let _ = tx.send(Event::FileFetched(seq, result));
            wake();
        });
    }

    /// 行 `row` のコミットの詳細と差分を裏で読む（同じ行を読んでいれば何もしない）。
    pub fn want_commit(&mut self, row: u32) {
        if self.commit_wanted == Some(row) {
            return;
        }
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        let Some(&id) = history.graph.ids().get(row as usize) else {
            return;
        };
        self.commit_wanted = Some(row);
        self.early_extras = None;
        let generation = history.generation;
        // 時間のかかる詳細は別のスレッドで（差分を待たせない）。
        let (extras_repo, extras_tx, extras_wake) =
            (repo.clone(), self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            if let Ok(extras) = extras_repo.commit_extras(&id) {
                let _ = extras_tx.send(Event::Extras(generation, row, extras));
                extras_wake();
            }
        });
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let whitespace = self.whitespace;
        self.last_want = Some(Want::Commit(row));
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let info = repo.commit_info(&id).map_err(|e| e.to_string())?;
                let changes = repo.commit_changes(&id).map_err(|e| e.to_string())?;
                let stacked = repo
                    .stacked_diff_ws(&changes, whitespace)
                    .map_err(|e| e.to_string())?;
                Ok(CommitShown::new(row, info, Arc::new(stacked)))
            };
            let shown = match timed("Load diff", load) {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("コミットを読めない: {e}")));
                    return wake();
                }
            };
            // 差分を先に出し、構文の色は後から（ファイル全体を解析するので時間がかかることがある）。
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, row, 0, stacked, collapsed);
        });
    }

    /// stash `id` の詳細と中身（`stash_changes`: 作業ツリーと index の変更、untracked のファイル）を裏で読む
    /// （Stashes の画面。History の行ではないので、複数選んだときと同じ鍵を使う）。
    pub fn want_stash(&mut self, id: amonhen_model::Oid) {
        self.want_oid(id, true);
    }

    /// History の行ではないコミット `id` の詳細と差分を裏で読む（Reflog。どのブランチにもないコミットもある）。
    pub fn want_commit_id(&mut self, id: amonhen_model::Oid) {
        self.want_oid(id, false);
    }

    /// `want_stash`・`want_commit_id` の中身（`stash` なら stash の中身、でなければコミットの変更）。
    fn want_oid(&mut self, id: amonhen_model::Oid, stash: bool) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        self.selection_seq = self.selection_seq.wrapping_add(1) & 0x7fff_ffff;
        let key = 0x8000_0000 | self.selection_seq;
        self.commit_wanted = Some(key);
        self.early_extras = None;
        let generation = history.generation;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let whitespace = self.whitespace;
        self.last_want = Some(Want::Oid(id, stash));
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let info = repo.commit_info(&id).map_err(|e| e.to_string())?;
                let changes = if stash {
                    repo.stash_changes(&id)
                } else {
                    repo.commit_changes(&id)
                }
                .map_err(|e| e.to_string())?;
                let stacked = repo
                    .stacked_diff_ws(&changes, whitespace)
                    .map_err(|e| e.to_string())?;
                Ok(CommitShown::new(key, info, Arc::new(stacked)))
            };
            let shown = match load() {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("stash を読めない: {e}")));
                    return wake();
                }
            };
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, key, 0, stacked, collapsed);
        });
    }

    /// コミット `id` での、ファイル `path` の差分と詳細を裏で読む（File History。名前を変えたコミットなら変える前の
    /// 名前でも探す）。History の行ではないので、複数選んだときと同じ鍵を使う。
    pub fn want_file_change(&mut self, id: amonhen_model::Oid, path: String) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        self.selection_seq = self.selection_seq.wrapping_add(1) & 0x7fff_ffff;
        let key = 0x8000_0000 | self.selection_seq;
        self.commit_wanted = Some(key);
        self.early_extras = None;
        let generation = history.generation;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let whitespace = self.whitespace;
        self.last_want = Some(Want::FileChange(id, path.clone()));
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let info = repo.commit_info(&id).map_err(|e| e.to_string())?;
                let changes: Vec<_> = repo
                    .commit_changes(&id)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .filter(|c| c.path == path || c.old_path.as_deref() == Some(path.as_str()))
                    .collect();
                let stacked = repo
                    .stacked_diff_ws(&changes, whitespace)
                    .map_err(|e| e.to_string())?;
                Ok(CommitShown::new(key, info, Arc::new(stacked)))
            };
            let shown = match load() {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("差分を読めない: {e}")));
                    return wake();
                }
            };
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, key, 0, stacked, collapsed);
        });
    }

    /// `base` から `tip` までの変更すべてを裏で読む（Push で送る変更すべて。macOS 版の PushSheet の range）。
    /// 詳細の欄の見出しは `title`・`note` と、コミットの 1 行（短い ID・件名・作者）の `lines`。
    pub fn want_range(
        &mut self,
        base: Option<amonhen_model::Oid>,
        tip: amonhen_model::Oid,
        title: String,
        note: String,
        lines: Vec<[String; 3]>,
    ) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        self.selection_seq = self.selection_seq.wrapping_add(1) & 0x7fff_ffff;
        let key = 0x8000_0000 | self.selection_seq;
        self.commit_wanted = Some(key);
        self.early_extras = None;
        let generation = history.generation;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let whitespace = self.whitespace;
        self.last_want = Some(Want::Range(
            base,
            tip,
            title.clone(),
            note.clone(),
            lines.clone(),
        ));
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let changes = repo
                    .changes_between(base.as_ref(), &tip)
                    .map_err(|e| e.to_string())?;
                let stacked = repo
                    .stacked_diff_ws(&changes, whitespace)
                    .map_err(|e| e.to_string())?;
                let info = serde_json::json!({ "selection": {
                    "kind": "range", "count": lines.len(), "lines": lines,
                    "title": title, "note": note,
                }});
                Ok(CommitShown::new(key, info, Arc::new(stacked)))
            };
            let shown = match load() {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("差分を読めない: {e}")));
                    return wake();
                }
            };
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, key, 0, stacked, collapsed);
        });
    }

    /// 複数選んだ行（`rows`、昇順で 2 つ以上）の差分と、詳細の欄の見出しを裏で読む（GTK4 版と同じく、コアの
    /// `selection_diff` が範囲・2 つ・とびとびで差分の取り方を決める）。
    pub fn want_selection(&mut self, rows: Vec<u32>) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        self.selection_seq = self.selection_seq.wrapping_add(1) & 0x7fff_ffff;
        let key = 0x8000_0000 | self.selection_seq;
        self.commit_wanted = Some(key);
        self.early_extras = None;
        let generation = history.generation;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let whitespace = self.whitespace;
        self.last_want = Some(Want::Selection(rows.clone()));
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let (selection, stacked) = repo
                    .selection_diff_ws(&history, &rows, amonhen_core::PairMode::Forward, whitespace)
                    .map_err(|e| e.to_string())?;
                let kind = match selection {
                    amonhen_core::Selection::Range { .. } => "range",
                    amonhen_core::Selection::Pair { .. } => "pair",
                    amonhen_core::Selection::Each(_) => "each",
                };
                // 各コミットの 1 行（短い ID・件名・作者。多ければ 20 まで）。
                let ids = history.graph.ids();
                let lines: Vec<serde_json::Value> = rows
                    .iter()
                    .take(20)
                    .filter_map(|&r| ids.get(r as usize))
                    .map(|id| {
                        let info = repo.commit_info(id).unwrap_or_default();
                        let message = info["message"].as_str().unwrap_or_default();
                        let subject = message.lines().next().unwrap_or_default();
                        let hex = id.to_string();
                        serde_json::json!([&hex[..8], subject, info["author"]])
                    })
                    .collect();
                let info = serde_json::json!({ "selection": {
                    "kind": kind, "count": rows.len(), "lines": lines,
                }});
                Ok(CommitShown::new(key, info, Arc::new(stacked)))
            };
            let shown = match load() {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("差分を読めない: {e}")));
                    return wake();
                }
            };
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, key, 0, stacked, collapsed);
        });
    }

    /// 行のコミットの署名の印（署名がなければ None、まだ検証していなければ Some(None) で、裏で検証を頼む）。
    pub fn signature_mark(&mut self, id: amonhen_model::Oid) -> Option<Signature> {
        let known = self.signatures.get(&id).copied();
        if known.is_none() && !self.verifying.contains(&id) && !self.unverified.contains(&id) {
            self.unverified.push(id);
        }
        known
    }

    /// 頼まれた署名を裏でまとめて検証する（描いた後に呼ぶ。GTK4 版は 8 個ずつ）。
    pub fn verify_signatures(&mut self) {
        if self.unverified.is_empty() {
            return;
        }
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.as_ref()) else {
            return;
        };
        let generation = history.generation;
        let pending = std::mem::take(&mut self.unverified);
        self.verifying.extend(pending.iter().copied());
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            for batch in pending.chunks(8) {
                let oids: Vec<String> = batch.iter().map(ToString::to_string).collect();
                let command = serde_json::json!({ "op": "verifySignatures", "oids": oids });
                let reply: serde_json::Value =
                    serde_json::from_str(&repo.run_json(&command.to_string())).unwrap_or_default();
                let checks = reply["info"]["checks"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let found = batch
                    .iter()
                    .zip(checks.iter())
                    .map(|(id, c)| (*id, Signature::of(c)))
                    .collect();
                let _ = tx.send(Event::Signatures(generation, found));
                wake();
            }
        });
    }

    /// History の差分の `diff_row` 行目が入っているファイル（右クリックのメニュー）。
    pub fn file_at(&self, diff_row: usize) -> Option<DiffFile> {
        let commit = self.commit.as_ref()?;
        let rows = commit.diff.rows.get(..=diff_row)?;
        let index = rows
            .iter()
            .filter(|r| r.kind == amonhen_layout::DiffRow::FILE)
            .count()
            .checked_sub(1)?;
        self.file_of(index)
    }

    /// コミットの変更ファイルすべて（変更ファイルの木の行ごとの右クリック）。
    pub fn files(&self) -> Vec<DiffFile> {
        let count = self.commit.as_ref().map_or(0, |c| c.stacked.files.len());
        (0..count).filter_map(|i| self.file_of(i)).collect()
    }

    /// コミットの `index` 番目の変更ファイル（見出しの ⋯・右クリック）。
    pub fn file_of(&self, index: usize) -> Option<DiffFile> {
        let commit = self.commit.as_ref()?;
        let path = commit.stacked.files.get(index)?.0.clone();
        // 足した・消したファイルか（GTK4 版の `deleted` の手元の決め方と同じく、前後の中身が空か）。
        let diff = &commit.stacked.files.get(index)?.1;
        let deleted = diff.new.is_empty() && !diff.old.is_empty();
        let added = diff.old.is_empty() && !diff.new.is_empty();
        let oid = commit.info["oid"].as_str().map(str::to_owned);
        Some(DiffFile {
            index,
            collapsed: commit.collapsed.get(index).copied().unwrap_or(false),
            commit: oid,
            deleted,
            added,
            whole: commit.shows_whole(index),
            path,
        })
    }

    /// 差分の `diff_row` 行目（hunk の見出し）が入っているファイルの番号。
    pub fn file_index_at(&self, diff_row: usize) -> Option<usize> {
        let commit = self.commit.as_ref()?;
        let rows = commit.diff.rows.get(..=diff_row)?;
        rows.iter()
            .filter(|r| r.kind == amonhen_layout::DiffRow::FILE)
            .count()
            .checked_sub(1)
    }

    /// 差分の `diff_row` 行目（ファイルの見出し）のファイルを畳む・開く。見出しでなければ何もしない。
    pub fn toggle_file(&mut self, diff_row: usize) -> bool {
        let Some(commit) = self.commit.as_ref() else {
            return false;
        };
        let rows = &commit.diff.rows;
        if rows
            .get(diff_row)
            .is_none_or(|r| r.kind != amonhen_layout::DiffRow::FILE)
        {
            return false;
        }
        let file = rows[..diff_row]
            .iter()
            .filter(|r| r.kind == amonhen_layout::DiffRow::FILE)
            .count();
        self.toggle_file_index(file)
    }

    /// 出しているコミットの差分の畳み方を `change` で変え、表示を作り直す（構文の色も付け直す）。`change` が
    /// false を返したら何もしない。変えたら true。
    fn relayout(&mut self, change: impl FnOnce(&mut CommitShown) -> bool) -> bool {
        let Some(generation) = self.history.as_ref().map(|h| h.generation) else {
            return false;
        };
        let Some(commit) = self.commit.as_mut() else {
            return false;
        };
        if !change(commit) {
            return false;
        }
        let (stacked, collapsed) = commit.rebuild_view();
        send_syntax(
            self.tx.clone(),
            self.wake.clone(),
            generation,
            commit.row,
            commit.version,
            stacked,
            collapsed,
        );
        true
    }

    /// 差分の `file` 番目のファイルを畳む・開く。
    pub fn toggle_file_index(&mut self, file: usize) -> bool {
        let repo = self.repo.clone();
        self.relayout(|commit| {
            let Some(&collapsed) = commit.collapsed.get(file) else {
                return false;
            };
            // まだ差分を取っていないファイル（ファイルの多いコミット）は、開くときに取る。
            if collapsed && let Some(repo) = &repo {
                commit.load_pending(repo, file);
            }
            commit.collapsed[file] = !collapsed;
            true
        })
    }

    /// hunk の見出しの印: `file` 番目のファイルの前後の行を 20 行ずつ広げる（macOS 版の `widenContext`）。
    pub fn widen_context(&mut self, file: usize) -> bool {
        let Some(repo) = self.repo.clone() else {
            return false;
        };
        self.relayout(|commit| {
            if commit.whole.contains(&file) {
                return false;
            }
            let context = commit.contexts.get(&file).copied().unwrap_or(3) + 20;
            if !commit.load(&repo, file, context) {
                return false;
            }
            commit.contexts.insert(file, context);
            true
        })
    }

    /// 見出しの ⋯ の Show Whole File: `file` 番目のファイルの前後の行をファイル全体にする・戻す（開く）。
    pub fn toggle_whole_file(&mut self, file: usize) -> bool {
        let Some(repo) = self.repo.clone() else {
            return false;
        };
        self.relayout(|commit| {
            if file >= commit.collapsed.len() {
                return false;
            }
            commit.load_pending(&repo, file);
            let whole = !commit.whole.contains(&file);
            let context = if whole {
                u32::MAX
            } else {
                commit.contexts.get(&file).copied().unwrap_or(3)
            };
            if !commit.load(&repo, file, context) {
                return false;
            }
            if whole {
                commit.whole.insert(file);
            } else {
                commit.whole.remove(&file);
            }
            commit.collapsed[file] = false;
            true
        })
    }

    /// 生成物と変更の多いファイルのほかをすべて開く（すべて開いていれば、すべて畳む）。帯の「差分をすべて開く・
    /// 畳む」。変わったら true。
    pub fn toggle_all_files(&mut self) -> bool {
        let repo = self.repo.clone();
        self.relayout(|commit| {
            if commit.all_open() {
                commit.collapsed.fill(true);
            } else {
                for i in commit.expandable() {
                    if let Some(repo) = &repo {
                        commit.load_pending(repo, i);
                    }
                    commit.collapsed[i] = false;
                }
            }
            true
        })
    }

    /// 行を比べるときに無視する空白を変え、出している差分を開き方を保って読み直す（Working Copy の差分も）。
    pub fn set_whitespace(&mut self, whitespace: Whitespace) {
        if whitespace == self.whitespace {
            return;
        }
        self.whitespace = whitespace;
        self.restore = self.commit.as_ref().map(|c| Restore {
            collapsed: c.collapsed.clone(),
            contexts: c.contexts.clone(),
            whole: c.whole.clone(),
            folders: c.collapsed_folders.clone(),
        });
        self.commit_wanted = None;
        match self.last_want.clone() {
            Some(Want::Commit(row)) => self.want_commit(row),
            Some(Want::Oid(id, stash)) => self.want_oid(id, stash),
            Some(Want::FileChange(id, path)) => self.want_file_change(id, path),
            Some(Want::Range(base, tip, title, note, lines)) => {
                self.want_range(base, tip, title, note, lines)
            }
            Some(Want::Selection(rows)) => self.want_selection(rows),
            None => self.restore = None,
        }
        if let Some((entry, prefer_staged)) = self.working_entry.clone() {
            self.want_working(Some(entry), prefer_staged);
        }
    }

    /// 読み直した差分に、前の開き方を戻す（ファイルの数が同じときだけ）。
    fn restore_layout(&mut self, restore: Restore) {
        let repo = self.repo.clone();
        self.relayout(|commit| {
            if commit.collapsed.len() != restore.collapsed.len() {
                return false;
            }
            for (file, &collapsed) in restore.collapsed.iter().enumerate() {
                if !collapsed && let Some(repo) = &repo {
                    commit.load_pending(repo, file);
                }
            }
            if let Some(repo) = &repo {
                for (&file, &context) in &restore.contexts {
                    if !restore.whole.contains(&file) && commit.load(repo, file, context) {
                        commit.contexts.insert(file, context);
                    }
                }
                for &file in &restore.whole {
                    if commit.load(repo, file, u32::MAX) {
                        commit.whole.insert(file);
                    }
                }
            }
            commit.collapsed = restore.collapsed;
            commit.collapsed_folders = restore.folders;
            true
        });
    }

    /// 変更ファイルを木で見せる・リストに戻す（帯のリスト／ツリー）。
    pub fn set_file_tree(&mut self, on: bool) -> bool {
        self.relayout(|commit| {
            let changed = commit.tree_on != on;
            commit.tree_on = on;
            changed
        })
    }

    /// 画像の比べる欄の「テキストで見る」: その SVG を絵の代わりに差分で見せる（macOS 版と同じ）。
    pub fn show_svg_as_text(&mut self, file: usize) -> bool {
        self.relayout(|commit| commit.svg_text.insert(file))
    }

    /// 木のフォルダ `path` を畳む・開く。
    pub fn toggle_folder(&mut self, path: &str) -> bool {
        self.relayout(|commit| {
            if !commit.collapsed_folders.remove(path) {
                commit.collapsed_folders.insert(path.to_owned());
            }
            true
        })
    }

    /// 木のフォルダをすべて開く（どれか畳んでいれば）・すべて畳む（帯のフォルダのボタン）。
    pub fn toggle_all_folders(&mut self) -> bool {
        self.relayout(|commit| {
            if commit.collapsed_folders.is_empty() {
                commit.collapsed_folders = commit.all_folders();
            } else {
                commit.collapsed_folders.clear();
            }
            true
        })
    }

    pub fn repo(&self) -> Option<Arc<Repository>> {
        self.repo.clone()
    }

    /// 各 worktree の HEAD の札と、ほかの worktree が checkout しているブランチ（GTK4 版の
    /// `HistoryView::set_worktrees`）。worktree が 1 つなら空で呼ぶ。
    pub fn set_worktrees(
        &mut self,
        heads: Vec<(amonhen_model::Oid, String, bool)>,
        elsewhere: HashSet<String>,
    ) {
        self.worktree_heads = heads;
        self.elsewhere = elsewhere;
        self.place_worktrees();
    }

    /// worktree の札を今の History の行に置く。
    fn place_worktrees(&mut self) {
        let mut labels = Labels::new();
        for (head, name, editing) in &self.worktree_heads {
            if let Some(row) = self.row_of(head) {
                labels.entry(row).or_default().push(RefLabel {
                    name: name.clone(),
                    kind: RefKind::Local,
                    head: *editing,
                    worktree: true,
                });
            }
        }
        self.worktree_labels = labels;
    }

    /// History の行に描いているコミットの行（編集で並べ替えていなければ同じ行）。
    pub fn shown_row(&self, row: u32) -> u32 {
        self.edit.as_ref().map_or(row, |e| e.shown_row(row))
    }

    /// History の上で編集を始める: 今のブランチの全部の行（たたまない・一致だけにしない）にする（GTK4 版の
    /// `Shaper::begin_editing`）。
    pub fn begin_editing(&mut self) {
        if self.before_edit.is_some() {
            return;
        }
        let fold = self.fold.load(std::sync::atomic::Ordering::Relaxed);
        let (scope, only) = (self.scope(), self.matching_only);
        if only {
            self.set_matching_only(false);
        }
        if fold {
            self.set_fold(false);
        }
        self.apply_scope(Scope::Current);
        self.before_edit = Some((scope, fold, only));
    }

    /// 編集を終える（Apply・Cancel）: 範囲の描き方を消し、元の範囲・折りたたみ・一致だけの表示に戻す。
    pub fn end_editing(&mut self) {
        self.edit = None;
        let Some((scope, fold, only)) = self.before_edit.take() else {
            return;
        };
        if fold {
            self.set_fold(true);
        }
        self.apply_scope(scope);
        if only {
            self.set_matching_only(true);
        }
    }

    /// 編集している（範囲・折りたたみは選べない）。
    pub fn editing(&self) -> bool {
        self.before_edit.is_some()
    }

    /// コミットの行（今の History の）。
    pub fn row_of(&self, id: &amonhen_model::Oid) -> Option<u32> {
        self.history.as_ref()?.row_of(id)
    }

    pub fn row_count(&self) -> u32 {
        self.history
            .as_ref()
            .map_or(0, |h| h.layout.nodes().len() as u32)
    }

    /// `rows` を含むチャンクを読み始め、遠いチャンクを捨てる。
    pub fn want(&mut self, rows: std::ops::Range<u32>) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        let first = rows.start / CHUNK_ROWS;
        let last = rows.end.saturating_sub(1) / CHUNK_ROWS;
        for index in first..=last {
            if self.chunks.contains_key(&index) || !self.loading.insert(index) {
                continue;
            }
            let (tx, wake) = (self.tx.clone(), self.wake.clone());
            let (repo, history) = (repo.clone(), history.clone());
            std::thread::spawn(move || {
                let chunk = repo
                    .history_chunk(&history, index * CHUNK_ROWS, CHUNK_ROWS, &PROFILE)
                    .map(Chunk)
                    .map_err(|e| e.to_string());
                let _ = tx.send(Event::Chunk(history.generation, index, chunk));
                wake();
            });
        }
        if self.chunks.len() > KEPT_CHUNKS {
            let center = (first + last) / 2;
            let mut indices: Vec<u32> = self.chunks.keys().copied().collect();
            indices.sort_by_key(|&i| i.abs_diff(center));
            for i in &indices[KEPT_CHUNKS..] {
                self.chunks.remove(i);
            }
        }
    }

    /// 見えている行のチャンクがすべて揃っているか。
    pub fn has_rows(&self, rows: std::ops::Range<u32>) -> bool {
        if rows.is_empty() {
            return self.history.is_some();
        }
        let first = rows.start / CHUNK_ROWS;
        let last = (rows.end - 1) / CHUNK_ROWS;
        (first..=last).all(|i| self.chunks.contains_key(&i))
    }
}

fn open(path: &Path) -> Result<Repository, String> {
    let backend = GixBackend::open(path).map_err(|e| e.to_string())?;
    let key = std::fs::canonicalize(path)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
    Ok(match amonhen_remote::local::cache_dir() {
        Some(dir) => Repository::with_cache(backend, amonhen_core::cache::path_for(&dir, &key)),
        None => Repository::new(backend),
    })
}

/// ref を行ごとのラベルにする。チェックアウト中のブランチ、ローカルブランチ、リモートブランチ、タグの順。
fn labels(history: &HistorySnapshot, refs: &Refs) -> Labels {
    let mut labels = Labels::new();
    for entry in &refs.entries {
        if !matches!(entry.kind, RefKind::Local | RefKind::Remote | RefKind::Tag) {
            continue;
        }
        let Some(row) = entry.target.as_ref().and_then(|t| history.row_of(t)) else {
            continue;
        };
        let head = entry.kind == RefKind::Local && refs.head_branch.as_deref() == Some(&entry.name);
        labels.entry(row).or_default().push(RefLabel {
            name: entry.name.clone(),
            kind: entry.kind,
            head,
            worktree: false,
        });
    }
    for row in labels.values_mut() {
        row.sort_by_key(|l| (!l.head, l.kind as u8));
    }
    labels
}

/// 届いたチャンクのうち、今の History（世代 `current`）のもの。前の History のものは読めても読めなくても捨てる。
/// 永続キャッシュから出した前回の History（世代 `cached`）の行が読めないのも失敗にしない: 同じパスに作り直した
/// リポジトリでは、前回のコミットがもうない。すぐ後に届く最新の History で読み直す（GTK4 版・macOS 版と同じく
/// 読めない行は捨てる）。
fn current_chunk<T>(
    current: Option<u64>,
    cached: Option<u64>,
    generation: u64,
    chunk: Result<T, String>,
) -> Result<Option<T>, String> {
    if current != Some(generation) {
        return Ok(None);
    }
    match chunk {
        Ok(chunk) => Ok(Some(chunk)),
        Err(_) if cached == Some(generation) => Ok(None),
        Err(e) => Err(format!("件名を読めない: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_the_cached_history_cant_read_are_dropped() {
        // キャッシュから出した前回の History（世代 1）の行は、同じパスに作り直したリポジトリでは読めない。
        let gone = || Err::<u32, _>("commit not found".to_owned());
        assert_eq!(current_chunk(Some(1), Some(1), 1, gone()), Ok(None));
        // 最新の History（世代 2）に替わった後に届いたものは、読めても読めなくても捨てる。
        assert_eq!(current_chunk(Some(2), None, 1, gone()), Ok(None));
        assert_eq!(current_chunk(Some(2), None, 1, Ok(7)), Ok(None));
        assert_eq!(current_chunk(None, None, 1, gone()), Ok(None));
        // 最新の History の行が読めなければ、これまでどおり失敗。
        assert_eq!(
            current_chunk(Some(2), None, 2, gone()),
            Err("件名を読めない: commit not found".to_owned())
        );
        assert_eq!(current_chunk(Some(2), None, 2, Ok(7)), Ok(Some(7)));
        assert_eq!(current_chunk(Some(1), Some(1), 1, Ok(7)), Ok(Some(7)));
    }

    #[test]
    fn wrap_breaks_at_spaces_and_counts_wide_characters() {
        // 収まれば 1 つ。
        assert_eq!(wrap_breaks("short line", 20), [(0, 10)]);
        // 後ろ半分の空白の後ろで切る。
        assert_eq!(wrap_breaks("aaaa bbbb cccc dddd", 12), [(0, 10), (10, 19)]);
        // 空白がなければ桁で切る。
        assert_eq!(wrap_breaks(&"x".repeat(20), 8), [(0, 8), (8, 16), (16, 20)]);
        // 全角は 2 桁（UTF-16 の位置は 1 つずつ）。
        assert_eq!(wrap_breaks(&"あ".repeat(10), 8), [(0, 4), (4, 8), (8, 10)]);
    }

    #[test]
    fn tree_layout_puts_folders_before_their_first_file() {
        let paths = ["README.md", "src/a/b.rs", "src/a/c.rs", "src/d.rs"];
        let (groups, depths, titles, hidden) = tree_layout(&paths, &HashSet::new());
        let names = |f: usize| -> Vec<(String, u32)> {
            groups
                .get(&f)
                .map(|g| g.iter().map(|x| (x.name.clone(), x.depth)).collect())
                .unwrap_or_default()
        };
        assert!(names(0).is_empty());
        assert_eq!(names(1), [("src".into(), 0), ("a".into(), 1)]);
        assert!(names(2).is_empty());
        assert!(names(3).is_empty());
        assert_eq!(depths, [0, 2, 2, 1]);
        assert_eq!(titles, ["README.md", "b.rs", "c.rs", "d.rs"]);
        assert!(hidden.is_empty());
    }

    #[test]
    fn tree_layout_hides_files_in_a_folded_folder() {
        let paths = ["src/a/b.rs", "src/a/c.rs", "src/d.rs"];
        let folded = HashSet::from(["src/a".to_owned()]);
        let (groups, _, _, hidden) = tree_layout(&paths, &folded);
        // 畳んだフォルダの行は出し、その中のフォルダの行とファイルは出さない。
        let first = &groups[&0];
        assert_eq!(first.len(), 2);
        assert!(first[1].collapsed && first[1].path == "src/a");
        assert_eq!(hidden, HashSet::from([0, 1]));
        assert!(!groups.contains_key(&1));
    }
}
