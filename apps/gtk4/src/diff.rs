//! 差分の表示。コミットの全ファイルを、ファイルの見出し・hunk の見出し・行の順に並べる（`StackedDiff`）。
//!
//! History と同じく、見えている行だけを 1 つのウィジェットの `snapshot` で描く。変わった単語の強調と構文の
//! 色は、コアが FE の符号単位（UTF-8 のバイト位置）で渡すので、そのまま Pango の属性にする。
//! ファイルの見出しをクリックすると、そのファイルを畳む・開く。本文はふだん折り返さず、一番長い行まで横にスクロール
//! する（行番号・印・ファイルの見出しは動かない。macOS 版と同じ）。「長い行を折り返す」（`wrap_lines`。アプリ
//! 全体の設定）なら、差分の幅に収まらない行を同じ行の続きの行に折り返す。折り返す位置は差分全体について先に決め
//! （`State::rewrap`）、描くときは 1 行の Pango の配置を続きの行ごとにずらして切り取って描くので、構文の色・
//! 変わった単語の強調はそのまま。行番号・印・チェックは最初の行にだけ出す。
//!
//! History の右の列では、見出しが変更ファイルの一覧を兼ねる（種類の印・木の深さと名前・行数・いつも出す ⋯）。
//! 見出しの前にフォルダの行、見出しの下に画像の欄（子の部品）を置き、畳んだフォルダの中の見出しは出さない。
//! 行の位置は、その分だけずらす（`State::row_y`・`State::row_at`。macOS 版の `rowY`・`rowAt`）。hunk の見出しの
//! 左の印を押すと、そのファイルの前後を広げる。「すべてのファイル」の中身は、その時点のファイルとして見せる
//! （`set_file_mode`）。
//!
//! Working Copy では、変更の行をクリック（Shift で範囲、Ctrl で追加）して選び、行の左の印で
//! 次のコミットに含めるか（index に入っているか）を見せる。印のクリックでその行を含める・外す。
//!
//! 行を選ばない差分（History など）では、文字を選べる（ドラッグ、ダブルクリックで単語、トリプルクリックで行、
//! Shift で広げる、Ctrl+A ですべて、Esc でやめる）。Ctrl+C か右クリックの Copy で、選んだ文字（Working Copy では
//! 選んだ行の文字）をコピーする。+・− の印は付けない（macOS 版と同じ）。

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};

use amonhen_core::diff::{DiffView, StackedDiff};
use amonhen_layout::DiffRow;
use amonhen_model::tr;
use amonhen_projection::FrontendProfile;
use gtk::glib::Properties;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk, pango};

use crate::theme::DiffColor;

/// 左の行番号の列と本文の間の余白。
const GUTTER_GAP: f32 = 12.0;
const TEXT_INSET: f32 = 8.0;
/// 行ごとの stage の印の列の幅。
const MARK_WIDTH: f32 = 20.0;
/// 見出しの下に置く欄（画像の比較）の高さ。
pub const PANEL_HEIGHT: f32 = 300.0;
/// 見出しの右端の ⋯ を押せる幅。
const MORE_WIDTH: f32 = 30.0;

/// 文字の位置（表示の行と、その行の文字のバイト位置）。
type TextPos = (u32, u32);

/// 長い行を折り返す設定の鍵（アプリ全体。macOS 版の `wrapDiffLines`）。
pub const WRAP_LINES: &str = "wrapDiffLines";

/// 表示の設定のメニューの「長い行を折り返す」（History の右の列と Working Copy。アプリ全体の設定）。
pub fn wrap_item() -> crate::menu::Item {
    let on = wrap_lines();
    crate::menu::choice_with_tip(
        tr!("Wrap Long Lines"),
        on,
        tr!("Wrap lines wider than the diff instead of scrolling sideways (in every diff)"),
        move || crate::settings::set_bool(WRAP_LINES, !on),
    )
}

/// 差分の帯の、長い行を折り返す切り替えのボタン（空白のボタンの隣。アイコンだけで、折り返している間は強調の
/// 色。ほかの所で切り替えても合わせる。macOS 版と同じ）。
pub fn wrap_button() -> gtk::Button {
    let icon = crate::icon::Icon::default();
    let button = gtk::Button::new();
    button.set_child(Some(&icon));
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    let update = {
        let (button, icon) = (button.downgrade(), icon.downgrade());
        move || {
            let (Some(button), Some(icon)) = (button.upgrade(), icon.upgrade()) else {
                return;
            };
            let on = wrap_lines();
            icon.set(crate::icon::Kind::WrapLines, on);
            let title = if on {
                tr!("Wrapping long lines (Alt+Z)")
            } else {
                tr!("Wrap Long Lines (Alt+Z)")
            };
            button.set_tooltip_text(Some(&format!(
                "{title}\n{}",
                tr!("Wrap lines wider than the diff instead of scrolling sideways (in every diff)")
            )));
            button.update_property(&[gtk::accessible::Property::Label(&tr!("Wrap Long Lines"))]);
        }
    };
    update();
    button.connect_clicked(|_| crate::settings::set_bool(WRAP_LINES, !wrap_lines()));
    crate::settings::connect_changed_while(&button, move |key| {
        if key == WRAP_LINES {
            update();
        }
    });
    button
}

/// 長い行を折り返すか。確かめる用（`--snapshot`）に `AMH_WRAP_LINES=1` で入れられる。
pub fn wrap_lines() -> bool {
    std::env::var("AMH_WRAP_LINES").is_ok_and(|v| v == "1") || crate::settings::bool(WRAP_LINES)
}

thread_local! {
    /// `--measure-diff-scroll` の間の、差分を描いた時間（ms）。
    static DRAW_TIMES: RefCell<Option<Vec<f64>>> = const { RefCell::new(None) };
}

/// 差分を描いた時間を記録し始める（`on`）・やめて、それまでの時間を返す（`--measure-diff-scroll`）。
pub fn draw_times(on: bool) -> Vec<f64> {
    DRAW_TIMES.with(|t| {
        let old = t.replace(on.then(Vec::new));
        old.unwrap_or_default()
    })
}

/// 折り返しを作り直すのにかかった時間がこれを超えたら、幅を変えている間は作り直さず、止まってから作り直す（ms）。
const REWRAP_BUDGET_MS: f64 = 4.0;

/// 折り返す位置を探す文字（空白と区切りの記号。その後ろで折り返す）。
fn breaks_after(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || matches!(
            c,
            '、' | '。' | '，' | '．' | '・' | '）' | '」' | '』' | '】'
        )
}

/// 行の折り返す位置（2 行目から先の始まり）。`count` は文字の数、`x(i)` は i 番目の文字の左端（`x(count)` は
/// 行の幅）、`byte(i)` はそのバイト位置、`ch(i)` はその文字。後ろ半分に空白か区切りの記号があればその後ろで、
/// なければ幅いっぱいで折り返す（macOS 版と同じ）。
fn break_points(
    count: usize,
    avail: f32,
    x: impl Fn(usize) -> f32,
    byte: impl Fn(usize) -> u32,
    ch: impl Fn(usize) -> char,
) -> Vec<u32> {
    let mut out = Vec::new();
    let mut s = 0;
    while s < count && x(count) - x(s) > avail {
        let mut e = s + 1;
        while e < count && x(e + 1) - x(s) <= avail {
            e += 1;
        }
        let half = s + (e - s) / 2;
        let cut = (half.max(s + 1)..e)
            .rev()
            .find(|&j| breaks_after(ch(j)))
            .map_or(e, |j| j + 1);
        out.push(byte(cut));
        s = cut;
    }
    out
}

/// `text` を `font` で 1 行に並べたときの、文字ごとの（バイト位置、左端の x）と、終わり（長さ、幅）。
fn measure_chars(
    context: &pango::Context,
    font: &pango::FontDescription,
    text: &str,
) -> Vec<(u32, f32)> {
    let layout = pango::Layout::new(context);
    layout.set_font_description(Some(font));
    layout.set_single_paragraph_mode(true);
    layout.set_text(text);
    let scale = pango::SCALE as f32;
    let mut out = Vec::with_capacity(text.len() + 1);
    let mut iter = layout.iter();
    loop {
        let index = iter.index() as u32;
        if index as usize >= text.len() {
            break;
        }
        out.push((index, iter.char_extents().x() as f32 / scale));
        if !iter.next_char() {
            break;
        }
    }
    out.push((text.len() as u32, layout.pixel_size().0 as f32));
    out
}

/// 裏のスレッドで作った表示。文字列置き場は自分のバッファを持っているので、どのスレッドに渡してもよい。
pub struct View {
    view: DiffView,
    /// 行ごとの、強調する範囲（バイト位置）。
    highlights: HashMap<u32, Vec<(u32, u32)>>,
    /// 行ごとの、構文の色（バイト位置と種類）。
    syntax: HashMap<u32, Vec<(u32, u32, u8)>>,
    /// 行番号の最大の桁数。
    digits: usize,
    /// ファイルの見出しの行（全ファイルを並べた差分。そうでなければ空）。
    file_rows: Vec<u32>,
    /// 一番長い行の、等幅の文字での桁数（横にスクロールする幅）。
    columns: usize,
}

// SAFETY: `DiffView` のポインタは、自分が持つバッファの先頭を指すだけ。
unsafe impl Send for View {}

impl View {
    /// `stacked` の、`collapsed` のファイルを畳んだ表示を作る（裏のスレッドで呼ぶ）。
    pub fn build(stacked: &StackedDiff, collapsed: &[bool]) -> Self {
        Self::new(stacked.view(collapsed, &FrontendProfile::GTK))
    }

    /// 強調と構文の色を行ごとに分けておく（構文の色はファイル全体を解析するので、裏のスレッドで呼ぶ）。
    pub fn new(view: DiffView) -> Self {
        let mut highlights: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
        for h in &view.highlights {
            highlights.entry(h.row).or_default().push((h.start, h.end));
        }
        let mut syntax: HashMap<u32, Vec<(u32, u32, u8)>> = HashMap::new();
        for s in view.syntax() {
            syntax
                .entry(s.row)
                .or_default()
                .push((s.start, s.end, s.kind));
        }
        let max = view
            .rows
            .iter()
            .filter(|r| r.kind != DiffRow::FILE)
            .map(|r| r.old_no.max(r.new_no))
            .max()
            .unwrap_or(0);
        let file_rows = view
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind == DiffRow::FILE)
            .map(|(i, _)| i as u32)
            .collect();
        let columns = view
            .rows
            .iter()
            .filter(|r| r.kind != DiffRow::FILE)
            .map(|r| columns(std::str::from_utf8(view.strings.bytes(r.text)).unwrap_or("")))
            .max()
            .unwrap_or(0);
        Self {
            highlights,
            syntax,
            digits: max.to_string().len().max(3),
            file_rows,
            columns,
            view,
        }
    }

    fn text(&self, row: &DiffRow) -> &str {
        std::str::from_utf8(self.view.strings.bytes(row.text)).unwrap_or("")
    }

    /// ファイルごとの行数・バイナリか・まだ差分を取っていないか（見出しの行から。ファイルの順）。
    pub fn file_infos(&self) -> Vec<FileInfo> {
        self.file_rows
            .iter()
            .map(|&r| {
                let row = &self.view.rows[r as usize];
                FileInfo {
                    added: row.new_no,
                    removed: row.old_no,
                    binary: row.flags & DiffRow::BINARY != 0,
                    pending: row.flags & DiffRow::PENDING != 0,
                }
            })
            .collect()
    }

    /// コアの表示（確認用）。
    #[cfg(test)]
    pub fn diff_view(&self) -> &DiffView {
        &self.view
    }
}

/// 全ファイルを並べた差分の、1 ファイルの様子。
#[derive(Clone, Copy, Debug, Default)]
pub struct FileInfo {
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
    /// まだ差分を取っていない（ファイルの多いコミット。行数は 0）。
    pub pending: bool,
}

/// 差分の元（コミットの全ファイルの差分）。ファイルを畳む・開く・前後を広げるたびに、ここから表示を作り直す。
/// ファイルの多いコミットでは、まだ差分を取っていないファイル（`FileDiff::pending`）があり、開いたときに `repo`
/// で取る。
pub struct Source {
    stacked: Mutex<StackedDiff>,
    pub repo: Option<Arc<amonhen_core::Repository>>,
}

// SAFETY: `StackedDiff` の中身は共有の不変なバッファ（`Blob`）と、自分で持つ文字列と行の範囲だけ。
unsafe impl Send for Source {}
unsafe impl Sync for Source {}

impl Source {
    pub fn new(stacked: StackedDiff, repo: Option<Arc<amonhen_core::Repository>>) -> Self {
        Self {
            stacked: Mutex::new(stacked),
            repo,
        }
    }

    /// 差分（前後を広げる・まだ取っていない差分を取るときは、裏のスレッドで書き換える）。
    pub fn lock(&self) -> MutexGuard<'_, StackedDiff> {
        self.stacked.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// ファイルの見出しの名前（パス。とびとびに選んだコミットでは「パス — 短い ID」）。
    pub fn names(&self) -> Vec<String> {
        self.lock().files.iter().map(|(p, _)| p.clone()).collect()
    }

    /// 全ファイルの差分で `f` を呼ぶ（まだ取っていないファイルは、写しの中で取る）。裏のスレッドで呼ぶ。
    pub fn with_all<R>(&self, f: impl FnOnce(&StackedDiff) -> R) -> R {
        let stacked = self.lock();
        if stacked.files.iter().all(|(_, d)| d.pending.is_none()) {
            return f(&stacked);
        }
        let mut copy = StackedDiff {
            files: stacked
                .files
                .iter()
                .map(|(p, d)| (p.clone(), d.clone()))
                .collect(),
        };
        drop(stacked);
        if let Some(repo) = &self.repo {
            for i in 0..copy.files.len() {
                if copy.files[i].1.pending.is_some() {
                    let _ = repo.set_stacked_context(&mut copy, i, 3);
                }
            }
        }
        f(&copy)
    }
}

/// 木で見せるときに、ファイルの見出しの前に出すフォルダの行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub depth: usize,
    /// 根からのパス（押したときに渡す）。
    pub path: String,
    pub collapsed: bool,
}

/// ファイルの見出しの飾り（History の右の列。macOS 版の `fileMarks`・`fileNotes`・`fileCounts`・`openOverride`・
/// `fileDepths`・`fileTitles`・`folderGroups`・`hiddenFiles`・`filePanels`）。ファイルの番号ごと。
#[derive(Clone, Default)]
pub struct Decor {
    /// 種類の印（A・D・M・R・C）。
    pub marks: HashMap<usize, &'static str>,
    /// 行数の代わりに出す文字（「Image」）。
    pub notes: HashMap<usize, String>,
    /// まだ差分を取っていないファイルの行数（見出しの行は 0 なので、詳細の行数で出す）。
    pub counts: HashMap<usize, (u32, u32)>,
    /// 開いた形（▾）で描くファイル（中身は差分の行ではなく、下に置いた欄）。
    pub open: HashSet<usize>,
    /// 木の深さと、見出しに出す名前（なければパス）。
    pub depths: HashMap<usize, usize>,
    pub titles: HashMap<usize, String>,
    /// 見出しの前に出すフォルダの行。
    pub folders: HashMap<usize, Vec<Folder>>,
    /// 畳んだフォルダの中のファイル（見出しの行も出さない）。
    pub hidden: HashSet<usize>,
    /// 見出しの下に置く欄（高さは `PANEL_HEIGHT`）。
    pub panels: HashMap<usize, gtk::Widget>,
}

/// 差分の行の種類を、AI に渡す文字の行の種類にする。
fn ai_line(kind: u8) -> amonhen_assist::Line {
    use amonhen_assist::Line;
    match kind {
        DiffRow::FILE => Line::File,
        DiffRow::HUNK => Line::Hunk,
        DiffRow::ADDED => Line::Added,
        DiffRow::REMOVED => Line::Removed,
        _ => Line::Context,
    }
}

/// `view` の行を、AI に渡す差分の文字に足す（上限に届いたら止める）。`path` は見出しのない差分のパス。
pub fn ai_text(view: &DiffView, path: Option<&str>, out: &mut amonhen_assist::DiffText) {
    if let Some(path) = path {
        out.path(path);
    }
    for row in &view.rows {
        let text = std::str::from_utf8(view.strings.bytes(row.text)).unwrap_or("");
        if !out.push(ai_line(row.kind), text) {
            break;
        }
    }
}

struct RowText {
    gutter: Option<pango::Layout>,
    text: pango::Layout,
}

#[derive(Default)]
pub struct State {
    source: Option<Arc<Source>>,
    collapsed: Vec<bool>,
    view: Option<Arc<View>>,
    rows: HashMap<u32, RowText>,
    /// `rows` を作ったときの構文のテーマ（変えたら作り直す）。
    theme_generation: u32,
    font: Option<pango::FontDescription>,
    row_height: f32,
    /// 等幅の文字 1 つの幅。
    char_width: f32,
    /// 作り直している途中の表示の世代（古い結果を捨てる）。
    generation: u64,
    /// 行を選ぶ表示（Working Copy）。
    marks: bool,
    /// 変更の行と hunk の見出しに stage のチェックボックスを出す（HEAD と作業ツリーの 1 本の差分だけ。片側だけの
    /// 差分では出さない。macOS 版と同じ）。Space で含める・外すのも、このときだけ。
    checks: bool,
    /// 選んだ変更の行。
    selection: BTreeSet<u32>,
    /// Shift で範囲を選ぶときの起点。
    anchor: Option<u32>,
    /// 選んだ文字（行を選ばない表示。起点と今の位置）。
    text_selection: Option<(TextPos, TextPos)>,
    /// 単語・行の単位で選び始めたときの最初の範囲（ドラッグでその単位のまま広げる）と、クリックの回数。
    selection_unit: Option<((TextPos, TextPos), i32)>,
    /// 本文を押して、文字を選びながらドラッグしているか。
    dragging: bool,
    /// 印をクリックしたときに呼ぶ（変更の行。hunk の見出しなら、その hunk の変更の行すべて）。
    on_mark: Option<Rc<dyn Fn(Vec<usize>)>>,
    /// 選んだ行が変わったときに呼ぶ。
    on_selection: Option<Rc<dyn Fn()>>,
    /// ファイルの見出し・差分の行を右クリックしたときに呼ぶ。
    on_file_menu: Option<FileMenuCallback>,
    /// 見出しのない差分（Working Copy の 1 ファイル）の行を右クリックしたときに呼ぶ（行、x、y）。
    on_row_menu: Option<RowMenuCallback>,
    /// Space で、選んだ変更の行を含める・外すときに呼ぶ（行）。
    on_toggle_rows: Option<Rc<dyn Fn(Vec<usize>)>>,
    /// ⌥J・⌥K で、全ファイルを並べていない差分の次・前のファイルへ移るときに呼ぶ（+1・−1）。
    on_move_file: Option<Rc<dyn Fn(i32)>>,
    /// J・K で最後に移った hunk（選べない差分で、次に J・K を押したときの起点）。
    hunk_cursor: Option<usize>,
    /// 表示し直した後に選ぶ hunk の番号（含める・外すの後に次の hunk へ進むため）。
    hunk_after: Option<usize>,
    /// 見出しの飾り（History の右の列）。
    decor: Decor,
    /// 見出しを押したときに呼ぶ（ファイルの番号）。None なら、その場で畳む・開く。
    on_toggle_file: Option<Rc<dyn Fn(usize)>>,
    /// フォルダの行を押したときに呼ぶ（フォルダのパス）。
    on_toggle_folder: Option<Rc<dyn Fn(String)>>,
    /// hunk の見出しの左の、前後を広げる印を押したときに呼ぶ（ファイルの番号）。あれば印を出す。
    on_expand_hunk: Option<Rc<dyn Fn(usize)>>,
    /// マウスが乗っているファイルの見出し。
    hovered_file: Option<usize>,
    /// 行の位置のずれ（行、高さ。行の順）。その行の後ろに高さの分だけ空ける（負なら詰める）。
    extra: Vec<(i64, f32)>,
    /// `extra` の高さの累積（`extra_sum[i]` は先頭から i 個の和。`row_y` を二分探索で引く）。
    extra_sum: Vec<f32>,
    /// `extra` に詰める（負の）ものがあるか（なければ `row_at` も二分探索で引ける）。
    extra_shrinks: bool,
    /// 長い行を折り返すか（`wrap_lines`）。
    wrap: bool,
    /// 部品の幅（今のもの）と、折り返しに使う幅（幅を変えている間は、高くつくなら前のまま）。
    width: f32,
    wrap_width: f32,
    /// 折り返した行の、2 行目から先の始まり（バイト位置）。1 行に収まる行は入れない。
    breaks: HashMap<u32, Vec<u32>>,
    /// `breaks` を作ったときの表示と、本文の幅（同じなら作り直さない）。
    breaks_for: Option<(usize, u32)>,
    /// タブか ASCII でない文字のある長い行の、文字ごとの位置（測ったら、幅を変えても使い回す）と、その表示。
    measured: HashMap<u32, Rc<Vec<(u32, f32)>>>,
    measured_for: usize,
    /// 文字を測る Pango の文脈。
    pango: Option<pango::Context>,
    /// ASCII の文字ごとの幅（字体ごとに一度だけ測る）。
    ascii_widths: Option<Rc<Vec<f32>>>,
    /// 最後に折り返しを作り直すのにかかった時間（ms）。
    wrap_cost: f64,
    /// 幅が止まってから折り返しを作り直す待ち。
    rewrap_pending: Option<glib::SourceId>,
    /// 描かない行（畳んだフォルダの中の見出し、その時点のファイルの hunk の見出しと消した行）。
    hidden_rows: HashSet<u32>,
    /// その時点のファイルとして見せる（History の「すべてのファイル」）。hunk の見出しと消した行は出さず、消した
    /// ところに「ここで N 行を削除」の行を置く。足した行は緑、変えた行（消した行に続けて足した行）は青。行番号は
    /// 後の側だけ。ファイルごと消したなら、消した行をそのまま出す。
    file_mode: bool,
    /// 消したところの印（その前の最後の行と、消した行の数・中身）。
    deletion_marks: Vec<(u32, u32, String)>,
    modified_rows: HashSet<u32>,
    /// 差分の中を探している語と、一致（行、バイト位置の始めと終わり）、今の一致。
    find: String,
    matches: Vec<(u32, u32, u32)>,
    current_match: Option<usize>,
    /// 差分の代わりに出す知らせ（「Loading the diff…」など）。
    message: Option<String>,
    /// 変更のない所（隙間）を少しずつ開閉する差分（Working Copy）。hunk の見出しの行の代わりに隙間の行があり
    /// （`old_no` が隠している行数、`new_no` が見せている行数、文字は関数名）、破線と札で描く（macOS 版と同じ）。
    folds: bool,
    /// 隙間の開き方（隙間の番号 → 上・下から見せている行数）と、隙間の行（番号の順）。
    gap_openings: HashMap<u32, (u32, u32)>,
    gap_rows: Vec<u32>,
    /// 隙間の開き方を変えた（隙間の番号、新しい開き方。None なら畳む）。
    on_set_gap: Option<SetGapCallback>,
    /// 行・hunk の変更を捨てる（Unstaged の差分の行）。
    on_discard_rows: Option<Rc<dyn Fn(Vec<usize>)>>,
    /// Unstaged・Staged の差分か（Some(true) が Staged）。All（`checks`）とほかの差分では None。
    split_side: Option<bool>,
    /// 行の Stage の仕方が「変更の帯・行番号を押して」か（そうでなければ「選んでから hunk の見出しで」）。
    stage_bar: bool,
    /// マウスが乗っている隙間の行と、そのボタン。
    hovered_gap: Option<(u32, Option<GapAction>)>,
    /// 帯の上でマウスが乗っている変更のかたまり（始め、終わりの次）。
    hovered_block: Option<(u32, u32)>,
    /// 「変更の帯」の形で、行番号の上をドラッグしている行（押した行と、今の範囲の変更の行）。
    number_drag: Option<(u32, Vec<u32>)>,
    /// 行をドラッグで選んでいる（押した行と、その前に選んでいた行）。
    row_drag: Option<(u32, BTreeSet<u32>)>,
}

/// 隙間の開き方を変えたときに呼ぶ（隙間の番号、上・下から見せる行数。None なら畳む）。
type SetGapCallback = Rc<dyn Fn(u32, Option<(u32, u32)>)>;

/// 隙間の行の操作。開く・畳むは隙間に、stage・unstage・discard はその下の hunk に効く（macOS 版と同じ）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapAction {
    /// 上（前の変更に続く所）から開く。
    Down,
    /// 下（次の変更の手前）から開く。
    Up,
    All,
    Collapse,
    Stage,
    Unstage,
    Discard,
}

/// 1 度に開く行数。
const GAP_STEP: u32 = 20;

/// 隙間の行の置き場所（部品の中の座標）。
struct GapLayout {
    /// 中央の札（0 行の見出しの行にはない）と、選んだ行の数を出しているか。
    pill: Option<(graphene::Rect, pango::Layout)>,
    pill_selected: bool,
    /// 札の右の開く・畳む（マウスが乗っているときだけ）と、右端の hunk の操作。
    buttons: Vec<(GapAction, graphene::Rect, pango::Layout)>,
    /// 左の関数名。
    function: Option<(f32, pango::Layout)>,
    /// hunk の操作が効く行（選んだ行か、hunk の変更すべて）。
    target: Vec<usize>,
}

/// ファイルの見出し・差分の行を右クリックしたときに呼ぶ（見出しの上ならファイルの番号、パス、x、y）。
type FileMenuCallback = Rc<dyn Fn(Option<usize>, &str, f64, f64)>;

/// 見出しのない差分の行を右クリックしたときに呼ぶ（行、x、y）。
type RowMenuCallback = Rc<dyn Fn(usize, f64, f64)>;

impl State {
    /// 行の上端（文書の座標）。見出しの下の欄・フォルダの行・消したところの印の分だけ下がり、描かない行の分だけ
    /// 上がる（macOS 版の `rowY`）。
    fn row_y(&self, row: u32) -> f32 {
        let i = self.extra.partition_point(|&(r, _)| r < row as i64);
        row as f32 * self.row_height + self.extra_sum.get(i).copied().unwrap_or(0.0)
    }

    /// 文書の座標 `y` にある行（欄の中なら、その見出しの行。macOS 版の `rowAt`）。
    fn row_at(&self, y: f32) -> i64 {
        let rh = self.row_height;
        if !self.extra_shrinks && self.extra_sum.len() == self.extra.len() + 1 {
            // 空けるものだけなら、空けたところの始まりは行の順に並ぶので二分探索で引ける。
            let start = |i: usize| (self.extra[i].0 + 1) as f32 * rh + self.extra_sum[i];
            let k = {
                let (mut lo, mut hi) = (0, self.extra.len());
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if start(mid) <= y {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                lo
            };
            if k > 0 && y < start(k - 1) + self.extra[k - 1].1 {
                return self.extra[k - 1].0;
            }
            return ((y - self.extra_sum[k]) / rh).floor() as i64;
        }
        let mut offset = 0.0;
        for &(r, h) in &self.extra {
            let start = (r + 1) as f32 * rh + offset;
            if h < 0.0 {
                if y < start + h {
                    break;
                }
            } else {
                if y < start {
                    break;
                }
                if y < start + h {
                    return r;
                }
            }
            offset += h;
        }
        ((y - offset) / rh).floor() as i64
    }

    fn row_count(&self) -> u32 {
        self.view.as_ref().map_or(0, |v| v.view.rows.len() as u32)
    }

    /// 文書の高さ。
    fn content_height(&self) -> f32 {
        self.row_y(self.row_count())
    }

    /// ファイルの番号 → 見出しの行。
    fn file_row(&self, file: usize) -> Option<u32> {
        self.view.as_ref()?.file_rows.get(file).copied()
    }

    /// 行の折り返す位置を作り直す（表示か本文の幅が変わったときだけ）。ASCII だけでタブのない行は桁数で、ほかは
    /// 文字ごとの位置を測って（測ったものは幅を変えても使い回す）決める。ファイル・hunk の見出しは折り返さない。
    fn rewrap(&mut self) {
        let Some(view) = self
            .view
            .clone()
            .filter(|_| self.wrap && self.wrap_width > 0.0)
        else {
            self.breaks.clear();
            self.breaks_for = None;
            return;
        };
        let avail =
            (self.wrap_width - DiffWidget::text_left(self, &view) - 8.0).max(self.char_width * 8.0);
        let key = (Arc::as_ptr(&view) as usize, avail.to_bits());
        if self.breaks_for == Some(key) {
            return;
        }
        let started = std::time::Instant::now();
        if self.measured_for != key.0 {
            self.measured.clear();
            self.measured_for = key.0;
        }
        self.breaks.clear();
        let font = self.font.clone().unwrap_or_default();
        // 文字の幅は、描くときと同じ文脈で測り直す（部品が窓に入る前に測った `char_width` は、画面の解像度の
        // 設定を受ける前の値のことがある）。
        // ASCII の文字ごとの幅（描くときと同じ文脈で、字体ごとに一度だけ測る）。ASCII だけでタブのない行は、
        // これを足して位置を決める（等幅の名前でも数字だけ別の字体になることがあるので、字ごとに）。
        if self.ascii_widths.is_none()
            && let Some(context) = &self.pango
        {
            let layout = pango::Layout::new(context);
            layout.set_font_description(Some(&font));
            let widths: Vec<f32> = (0u8..128)
                .map(|b| {
                    let c = b as char;
                    if !(' '..='~').contains(&c) {
                        return 0.0;
                    }
                    // 端の空白を幅に数えないことがあるので、字で挟んで測る。
                    let width = |text: &str| {
                        layout.set_text(text);
                        layout.size().0 as f32 / pango::SCALE as f32
                    };
                    (width(&format!("x{}x", c.to_string().repeat(16))) - width("xx")) / 16.0
                })
                .collect();
            self.ascii_widths = Some(Rc::new(widths));
        }
        let ascii = self.ascii_widths.clone();
        // 一番広い ASCII の文字でも収まる長さの行は、足さずに収まるとみなす（ほとんどの行）。
        let widest = ascii
            .as_ref()
            .map_or(f32::MAX, |w| w.iter().copied().fold(0.0, f32::max));
        let cw = self.char_width;
        let mut xs: Vec<f32> = Vec::new();
        for (i, row) in view.view.rows.iter().enumerate() {
            if matches!(row.kind, DiffRow::FILE | DiffRow::HUNK) {
                continue;
            }
            let text = view.text(row);
            if let Some(widths) = &ascii
                && text.is_ascii()
                && !text.contains('\t')
            {
                if text.len() as f32 * widest <= avail {
                    continue;
                }
                let bytes = text.as_bytes();
                xs.clear();
                let mut x = 0.0;
                xs.push(0.0);
                for &b in bytes {
                    x += widths[b as usize];
                    xs.push(x);
                }
                if x <= avail {
                    continue;
                }
                let b = break_points(
                    bytes.len(),
                    avail,
                    |i| xs[i],
                    |i| i as u32,
                    |i| bytes[i] as char,
                );
                self.breaks.insert(i as u32, b);
                continue;
            }
            // 幅の見積もり（タブは 8 桁ごと、広い文字は 2 桁）に余裕を見ても収まるなら、測らない。
            if columns(text) as f32 * cw * 1.25 <= avail {
                continue;
            }
            let chars = match self.measured.get(&(i as u32)) {
                Some(chars) => chars.clone(),
                None => {
                    let Some(context) = &self.pango else { continue };
                    let chars = Rc::new(measure_chars(context, &font, text));
                    self.measured.insert(i as u32, chars.clone());
                    chars
                }
            };
            let count = chars.len() - 1;
            let b = break_points(
                count,
                avail,
                |i| chars[i].1,
                |i| chars[i].0,
                |i| text[chars[i].0 as usize..].chars().next().unwrap_or(' '),
            );
            if !b.is_empty() {
                self.breaks.insert(i as u32, b);
            }
        }
        self.breaks_for = Some(key);
        self.wrap_cost = started.elapsed().as_secs_f64() * 1000.0;
    }

    /// 行 `index` の見た目の行の数（折り返していなければ 1）。
    fn lines_of(&self, index: u32) -> usize {
        self.breaks.get(&index).map_or(1, |b| b.len() + 1)
    }

    /// 見出しの下の欄・フォルダの行・畳んだフォルダ・その時点のファイル・折り返した行から、行の位置のずれを作り
    /// 直す（macOS 版の `rebuildExtra`・`computeFileMode`）。
    fn rebuild_extra(&mut self) {
        self.rewrap();
        let rh = self.row_height;
        self.deletion_marks.clear();
        self.modified_rows.clear();
        self.hidden_rows.clear();
        let Some(view) = self.view.clone() else {
            self.set_extra(Vec::new());
            return;
        };
        let rows = &view.view.rows;
        if self.file_mode {
            let deleted_file = !rows
                .iter()
                .any(|r| matches!(r.kind, DiffRow::ADDED | DiffRow::CONTEXT));
            let mut row = 0;
            while row < rows.len() {
                let kind = rows[row].kind;
                if kind == DiffRow::HUNK {
                    self.hidden_rows.insert(row as u32);
                    row += 1;
                } else if kind == DiffRow::REMOVED && !deleted_file {
                    let start = row;
                    while row < rows.len() && rows[row].kind == DiffRow::REMOVED {
                        row += 1;
                    }
                    self.hidden_rows.extend(start as u32..row as u32);
                    if row < rows.len() && rows[row].kind == DiffRow::ADDED {
                        while row < rows.len() && rows[row].kind == DiffRow::ADDED {
                            self.modified_rows.insert(row as u32);
                            row += 1;
                        }
                    } else {
                        let text = rows[start..row]
                            .iter()
                            .map(|r| view.text(r))
                            .collect::<Vec<_>>()
                            .join("\n");
                        self.deletion_marks
                            .push(((row - 1) as u32, (row - start) as u32, text));
                    }
                } else {
                    row += 1;
                }
            }
        } else {
            for &file in &self.decor.hidden {
                if let Some(&r) = view.file_rows.get(file) {
                    self.hidden_rows.insert(r);
                }
            }
        }
        // （行、同じ行の中の順、高さ）。
        let mut entries: Vec<(i64, u8, f32)> = Vec::new();
        for &file in self.decor.panels.keys() {
            if let Some(&r) = view.file_rows.get(file)
                && !self.decor.hidden.contains(&file)
            {
                entries.push((r as i64, 0, PANEL_HEIGHT));
            }
        }
        for (&file, folders) in &self.decor.folders {
            if let Some(&r) = view.file_rows.get(file) {
                entries.push((r as i64 - 1, 1, folders.len() as f32 * rh));
            }
        }
        // 描かない行（続いた行ごと）は、その後ろの行を上に詰める。
        let mut hidden: Vec<u32> = self.hidden_rows.iter().copied().collect();
        hidden.sort_unstable();
        let mut run: Vec<u32> = Vec::new();
        let flush = |run: &mut Vec<u32>, entries: &mut Vec<(i64, u8, f32)>| {
            if let Some(&last) = run.last() {
                entries.push((last as i64, 2, -(run.len() as f32) * rh));
            }
            run.clear();
        };
        for row in hidden {
            if run.last().is_some_and(|&last| row != last + 1) {
                flush(&mut run, &mut entries);
            }
            run.push(row);
        }
        flush(&mut run, &mut entries);
        for &(after, _, _) in &self.deletion_marks {
            entries.push((after as i64, 3, rh));
        }
        // 折り返した行の続きの行（その行のすぐ後ろ。消したところの印より前）。
        for (&r, b) in &self.breaks {
            if !self.hidden_rows.contains(&r) {
                entries.push((r as i64, 0, b.len() as f32 * rh));
            }
        }
        entries.sort_by_key(|e| (e.0, e.1));
        self.set_extra(entries.into_iter().map(|(r, _, h)| (r, h)).collect());
    }

    /// 行の位置のずれを替え、高さの累積を作り直す。
    fn set_extra(&mut self, extra: Vec<(i64, f32)>) {
        self.extra = extra;
        self.extra_shrinks = self.extra.iter().any(|&(_, h)| h < 0.0);
        let mut sum = 0.0;
        self.extra_sum = std::iter::once(0.0)
            .chain(self.extra.iter().map(|&(_, h)| {
                sum += h;
                sum
            }))
            .collect();
    }
}

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::DiffWidget)]
    pub struct DiffWidget {
        #[property(override_interface = gtk::Scrollable, get, set = Self::set_hadjustment, nullable)]
        hadjustment: RefCell<Option<gtk::Adjustment>>,
        #[property(override_interface = gtk::Scrollable, get, set = Self::set_vadjustment, nullable)]
        vadjustment: RefCell<Option<gtk::Adjustment>>,
        #[property(override_interface = gtk::Scrollable, get, set, builder(gtk::ScrollablePolicy::Minimum))]
        hscroll_policy: Cell<gtk::ScrollablePolicy>,
        #[property(override_interface = gtk::Scrollable, get, set, builder(gtk::ScrollablePolicy::Minimum))]
        vscroll_policy: Cell<gtk::ScrollablePolicy>,
        pub state: RefCell<State>,
    }

    impl Default for DiffWidget {
        fn default() -> Self {
            Self {
                hadjustment: RefCell::default(),
                vadjustment: RefCell::default(),
                hscroll_policy: Cell::new(gtk::ScrollablePolicy::Minimum),
                vscroll_policy: Cell::new(gtk::ScrollablePolicy::Minimum),
                state: RefCell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DiffWidget {
        const NAME: &'static str = "AmonhenDiffWidget";
        type Type = super::DiffWidget;
        type ParentType = gtk::Widget;
        type Interfaces = (gtk::Scrollable,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for DiffWidget {
        fn constructed(&self) {
            self.parent_constructed();
            let view = self.obj();
            view.set_focusable(true);
            view.set_overflow(gtk::Overflow::Hidden);
            view.add_css_class("view");
            // 長い行を折り返す（アプリ全体の設定。変えたら、出している差分も折り返し直す）。
            view.state().wrap = wrap_lines();
            let weak = view.downgrade();
            crate::settings::connect_changed_while(&*view, move |key| {
                if key == WRAP_LINES
                    && let Some(view) = weak.upgrade()
                {
                    let on = wrap_lines();
                    {
                        let mut st = view.state();
                        st.wrap = on;
                        st.wrap_width = st.width;
                    }
                    view.rewrap_keeping_top();
                }
            });
            // Edit の Copy・Select All（キーは `key_pressed`）。
            for (prefix, name) in [("clipboard", "copy"), ("selection", "select-all")] {
                let group = gio::SimpleActionGroup::new();
                let action = gio::SimpleAction::new(name, None);
                let weak = view.downgrade();
                action.connect_activate(move |_, _| {
                    if let Some(view) = weak.upgrade() {
                        if prefix == "clipboard" {
                            view.copy_selection();
                        } else {
                            view.select_all();
                        }
                    }
                });
                group.add_action(&action);
                view.insert_action_group(prefix, Some(&group));
            }
            let click = gtk::GestureClick::new();
            let weak = view.downgrade();
            click.connect_pressed(move |gesture, clicks, x, y| {
                if let Some(view) = weak.upgrade() {
                    view.grab_focus();
                    let state = gesture.current_event_state();
                    view.clicked(
                        x as f32,
                        y as f32,
                        state.contains(gdk::ModifierType::SHIFT_MASK),
                        state.contains(gdk::ModifierType::CONTROL_MASK),
                        clicks,
                    );
                }
            });
            view.add_controller(click);
            // ドラッグで文字を選ぶ（行を選ばない表示）。
            let drag = gtk::GestureDrag::new();
            let weak = view.downgrade();
            drag.connect_drag_update(move |gesture, dx, dy| {
                if let (Some(view), Some((x, y))) = (weak.upgrade(), gesture.start_point()) {
                    view.drag_to((x + dx) as f32, (y + dy) as f32);
                }
            });
            let weak = view.downgrade();
            drag.connect_drag_end(move |_, _, _| {
                if let Some(view) = weak.upgrade() {
                    view.drag_end();
                }
            });
            view.add_controller(drag);
            // 選んだ文字の色は、フォーカスがあるかで変える。
            let focus = gtk::EventControllerFocus::new();
            for enter in [true, false] {
                let weak = view.downgrade();
                let redraw = move |_: &gtk::EventControllerFocus| {
                    if let Some(view) = weak.upgrade() {
                        view.queue_draw();
                    }
                };
                if enter {
                    focus.connect_enter(redraw);
                } else {
                    focus.connect_leave(redraw);
                }
            }
            view.add_controller(focus);
            // ファイルの見出しの右クリック（Blame・File History）。
            let menu = gtk::GestureClick::new();
            menu.set_button(gdk::BUTTON_SECONDARY);
            let weak = view.downgrade();
            menu.connect_pressed(move |_, _, x, y| {
                if let Some(view) = weak.upgrade() {
                    view.file_menu(x, y);
                }
            });
            view.add_controller(menu);
            // マウスが乗っているファイルの見出しを薄く塗る（macOS 版と同じ）。
            let motion = gtk::EventControllerMotion::new();
            let weak = view.downgrade();
            motion.connect_motion(move |_, x, y| {
                if let Some(view) = weak.upgrade() {
                    view.hover_gap(Some((x as f32, y as f32)));
                    view.hover(Some(y as f32));
                }
            });
            let weak = view.downgrade();
            motion.connect_leave(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.hover_gap(None);
                    view.hover(None);
                }
            });
            view.add_controller(motion);
            // その時点のファイルの、消したところの印のツールチップ（消した行の中身）。
            view.connect_query_tooltip(|view, _, y, _, tooltip| {
                match view.deleted_text(y as f32) {
                    Some(text) => {
                        tooltip.set_text(Some(&text));
                        true
                    }
                    None => false,
                }
            });
            // Space（選んだ行を含める・外す）、J・K（次・前の hunk）、⌥J・⌥K（次・前のファイル）、Ctrl+C・
            // Ctrl+A・Esc（コピー・すべて選ぶ・選ぶのをやめる）。
            let keys = gtk::EventControllerKey::new();
            let weak = view.downgrade();
            keys.connect_key_pressed(move |_, key, _, modifiers| {
                let Some(view) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                view.key_pressed(key, modifiers)
            });
            view.add_controller(keys);
        }

        fn dispose(&self) {
            let panels: Vec<gtk::Widget> = self
                .state
                .borrow_mut()
                .decor
                .panels
                .drain()
                .map(|(_, w)| w)
                .collect();
            for panel in panels {
                panel.unparent();
            }
        }
    }

    impl WidgetImpl for DiffWidget {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.obj().width_changed(width as f32);
            self.obj().configure_adjustments();
            self.obj().place_panels();
            // 右クリックのメニューは子なので、自分で置く（置かないと、最初に測った高さのまま項目が切れる）。
            let mut child = self.obj().first_child();
            while let Some(c) = child {
                if let Some(p) = c.downcast_ref::<gtk::Popover>() {
                    p.present();
                }
                child = c.next_sibling();
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let started = std::time::Instant::now();
            self.obj().draw(snapshot);
            DRAW_TIMES.with(|t| {
                if let Some(t) = t.borrow_mut().as_mut() {
                    t.push(started.elapsed().as_secs_f64() * 1000.0);
                }
            });
        }
    }

    impl ScrollableImpl for DiffWidget {}

    impl DiffWidget {
        fn set_hadjustment(&self, adjustment: Option<gtk::Adjustment>) {
            if let Some(adj) = &adjustment {
                let weak = self.obj().downgrade();
                adj.connect_value_changed(move |_| {
                    if let Some(view) = weak.upgrade() {
                        view.queue_draw();
                    }
                });
            }
            self.hadjustment.replace(adjustment);
            self.obj().configure_adjustments();
        }

        fn set_vadjustment(&self, adjustment: Option<gtk::Adjustment>) {
            if let Some(adj) = &adjustment {
                let weak = self.obj().downgrade();
                adj.connect_value_changed(move |_| {
                    if let Some(view) = weak.upgrade() {
                        // 見出しの下の欄も一緒に動かす。
                        if !view.state().decor.panels.is_empty() {
                            view.queue_allocate();
                        }
                        view.queue_draw();
                    }
                });
            }
            self.vadjustment.replace(adjustment);
            self.obj().configure_adjustments();
        }
    }
}

glib::wrapper! {
    pub struct DiffWidget(ObjectSubclass<imp::DiffWidget>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Default for DiffWidget {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl DiffWidget {
    fn state(&self) -> std::cell::RefMut<'_, State> {
        self.imp().state.borrow_mut()
    }

    /// 前の差分を消す（次を読んでいる間）。
    pub fn clear(&self) {
        self.replace_decor(Decor::default());
        {
            let mut st = self.state();
            st.generation += 1;
            st.message = None;
            st.source = None;
            st.view = None;
            st.rows.clear();
            st.selection.clear();
            st.anchor = None;
            st.text_selection = None;
            st.hunk_cursor = None;
            st.hovered_file = None;
            st.rebuild_extra();
        }
        self.configure_adjustments();
        self.queue_draw();
    }

    /// 差分の代わりに知らせ `text` を出す（読んでいる・読めなかったなど。macOS 版と同じ文）。
    pub fn show_message(&self, text: &str) {
        self.clear();
        self.state().message = Some(text.to_owned());
        self.queue_draw();
    }

    /// 全ファイルを並べた差分を、`collapsed` のファイルを畳み（`view` はそれで作ったもの）、見出しを `decor` で
    /// 飾って出す。`keep_scroll` ならスクロールの位置を保つ（開く・畳む・前後を広げたとき）。
    pub fn show_stacked(
        &self,
        source: Arc<Source>,
        view: View,
        collapsed: Vec<bool>,
        decor: Decor,
        keep_scroll: bool,
    ) {
        self.replace_decor(decor);
        {
            let mut st = self.state();
            self.ensure_font(&mut st);
            st.generation += 1;
            st.message = None;
            st.collapsed = collapsed;
            st.source = Some(source);
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = false;
            st.checks = false;
            st.selection.clear();
            st.anchor = None;
            st.text_selection = None;
            st.hunk_cursor = None;
            st.rebuild_extra();
        }
        if !keep_scroll {
            self.scroll_to_start();
        }
        self.configure_adjustments();
        self.queue_allocate();
        self.refresh_matches();
        self.queue_draw();
        self.selection_changed();
    }

    /// 見出しの飾りを替える（前の欄の部品は外し、新しい欄の部品を子にする）。
    fn replace_decor(&self, decor: Decor) {
        let old = std::mem::replace(&mut self.state().decor, decor);
        let new: Vec<gtk::Widget> = self.state().decor.panels.values().cloned().collect();
        for widget in old.panels.values() {
            if !new.contains(widget) && widget.parent().as_ref() == Some(self.upcast_ref()) {
                widget.unparent();
            }
        }
        for widget in new {
            if widget.parent().as_ref() != Some(self.upcast_ref()) {
                if widget.parent().is_some() {
                    widget.unparent();
                }
                widget.set_parent(self);
            }
        }
    }

    /// まだ差分を取っていないファイルの行数を替える（詳細の行数が届いたとき）。
    pub fn set_counts(&self, counts: HashMap<usize, (u32, u32)>) {
        self.state().decor.counts = counts;
        self.queue_draw();
    }

    /// 見出しを押したら `f(ファイルの番号)` を呼ぶ（その場で畳む・開く代わりに）。
    pub fn connect_toggle_file(&self, f: impl Fn(usize) + 'static) {
        self.state().on_toggle_file = Some(Rc::new(f));
    }

    /// フォルダの行を押したら `f(フォルダのパス)` を呼ぶ。
    pub fn connect_toggle_folder(&self, f: impl Fn(String) + 'static) {
        self.state().on_toggle_folder = Some(Rc::new(f));
    }

    /// hunk の見出しの左に前後を広げる印を出し、押したら `f(ファイルの番号)` を呼ぶ。
    pub fn connect_expand_hunk(&self, f: impl Fn(usize) + 'static) {
        self.state().on_expand_hunk = Some(Rc::new(f));
        self.queue_draw();
    }

    /// その時点のファイルとして見せる（「すべてのファイル」の中身）。
    pub fn set_file_mode(&self, on: bool) {
        self.state().file_mode = on;
        self.set_has_tooltip(on);
    }

    /// 見出しの下の欄を、スクロールに合わせて置く（`size_allocate` から）。
    fn place_panels(&self) {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let width = self.width();
        let placed: Vec<(gtk::Widget, Option<f32>)> = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            let rh = st.row_height;
            st.decor
                .panels
                .iter()
                .map(|(&file, widget)| {
                    let y = st
                        .file_row(file)
                        .filter(|_| !st.decor.hidden.contains(&file))
                        .map(|row| st.row_y(row) + rh - scroll);
                    (widget.clone(), y)
                })
                .collect()
        };
        for (widget, y) in placed {
            let shown = y.is_some_and(|y| y + PANEL_HEIGHT > 0.0 && y < self.height() as f32);
            widget.set_child_visible(shown);
            if let (true, Some(y)) = (shown, y) {
                widget.measure(gtk::Orientation::Horizontal, -1);
                widget.allocate(
                    width,
                    PANEL_HEIGHT as i32,
                    -1,
                    Some(gsk::Transform::new().translate(&graphene::Point::new(0.0, y))),
                );
            }
        }
    }

    /// `y`（部品の中の位置）が消したところの印なら、消した行の中身。
    fn deleted_text(&self, y: f32) -> Option<String> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let st = self.state();
        if !st.file_mode {
            return None;
        }
        let rh = st.row_height;
        st.deletion_marks.iter().find_map(|(after, _, text)| {
            let top = st.row_y(after + 1) - rh - scroll;
            (y >= top && y < top + rh).then(|| text.clone())
        })
    }

    /// マウスが `y` に乗った（None なら外れた）: 見出しの上なら、そのファイルを薄く塗る。
    fn hover(&self, y: Option<f32>) {
        if self
            .state()
            .view
            .as_ref()
            .is_none_or(|v| v.file_rows.is_empty())
        {
            return;
        }
        let file = y.and_then(|y| self.header_at(y));
        let changed = {
            let mut st = self.state();
            let changed = st.hovered_file != file;
            st.hovered_file = file;
            changed
        };
        if changed {
            self.queue_draw();
        }
    }

    /// `y`（部品の中の位置）のフォルダの行のパス。
    fn folder_at(&self, y: f32) -> Option<String> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let st = self.state();
        let rh = st.row_height;
        st.decor.folders.iter().find_map(|(&file, folders)| {
            let bottom = st.row_y(st.file_row(file)?);
            let top = bottom - folders.len() as f32 * rh;
            let i = ((y + scroll - top) / rh).floor();
            (i >= 0.0 && (i as usize) < folders.len()).then(|| folders[i as usize].path.clone())
        })
    }

    /// `y`（部品の中の位置）が見出しの下の欄の中か。
    fn in_panel(&self, y: f32) -> bool {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let st = self.state();
        let rh = st.row_height;
        st.decor.panels.keys().any(|&file| {
            st.file_row(file)
                .filter(|_| !st.decor.hidden.contains(&file))
                .is_some_and(|row| {
                    let top = st.row_y(row) + rh - scroll;
                    y >= top && y < top + PANEL_HEIGHT
                })
        })
    }

    /// ファイル `index` の見出しが見えている範囲より上にあれば、見出しが一番上に来るまで戻す。
    pub fn reveal_file_header(&self, index: usize) {
        let top = {
            let st = self.state();
            st.file_row(index).map(|row| st.row_y(row))
        };
        if let (Some(top), Some(adj)) = (top, self.vadjustment())
            && (top as f64) < adj.value()
        {
            self.scroll_to_file(index);
        }
    }

    /// `line` 行目（後の側の行番号。ファイルごと消したなら前の側）を、見えている範囲の上の方に出して選ぶ。
    pub fn scroll_to_line(&self, line: u32) {
        let target = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            let Some(view) = st.view.clone() else { return };
            let rows = &view.view.rows;
            let row = rows
                .iter()
                .position(|r| r.new_no == line && r.kind != DiffRow::HUNK)
                .or_else(|| {
                    rows.iter()
                        .position(|r| r.old_no == line && r.kind == DiffRow::REMOVED)
                });
            let Some(row) = row else { return };
            st.selection = std::iter::once(row as u32).collect();
            st.row_y(row as u32)
        };
        if let Some(adj) = self.vadjustment() {
            let page = adj.page_size();
            let y = (target as f64 - page / 3.0).clamp(0.0, (adj.upper() - page).max(0.0));
            adj.set_value(y);
        }
        self.queue_draw();
    }

    /// `y`（部品の中の位置）の差分の行の、ファイルの番号と後の側の行番号（0 ならない）。
    pub fn line_at(&self, y: f64) -> Option<(usize, u32)> {
        let row = self.row_at(y)?;
        let st = self.state();
        let view = st.view.as_ref()?;
        let r = view.view.rows.get(row)?;
        if r.kind == DiffRow::FILE {
            return None;
        }
        let file = view.file_rows.iter().rposition(|&f| f as usize <= row)?;
        Some((file, r.new_no))
    }

    /// Working Copy の 1 ファイルの差分を、行ごとの stage の印つきで出す。`keep` なら同じファイルを
    /// 読み直したとして、スクロールの位置を保つ。選んだ行はいつも外す（macOS 版と同じ。読み直すと行の番号が
    /// 別の行を指すことがあり、そのまま Stage Lines・Discard Lines を押すと違う行に効いてしまう）。
    pub fn show_working(&self, view: View, keep: bool, checks: bool) {
        self.replace_decor(Decor::default());
        {
            let mut st = self.state();
            self.ensure_font(&mut st);
            st.generation += 1;
            st.message = None;
            st.source = None;
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = true;
            st.checks = checks;
            st.gap_rows = if st.folds {
                st.view
                    .as_ref()
                    .map(|v| {
                        v.view
                            .rows
                            .iter()
                            .enumerate()
                            .filter(|(_, r)| r.kind == DiffRow::HUNK)
                            .map(|(i, _)| i as u32)
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            st.hovered_gap = None;
            st.hovered_block = None;
            st.number_drag = None;
            st.row_drag = None;
            st.selection.clear();
            st.anchor = None;
            st.text_selection = None;
            if !keep {
                st.hunk_after = None;
            }
            st.hunk_cursor = None;
            st.rebuild_extra();
        }
        if !keep {
            self.scroll_to_start();
        }
        self.configure_adjustments();
        self.refresh_matches();
        // 含める・外すの後は、次の hunk を選ぶ（macOS 版と同じ）。
        let after = self.state().hunk_after.take();
        if let Some(index) = after {
            self.select_hunk(index);
        }
        self.queue_draw();
        self.selection_changed();
    }

    /// 印も選択もない表示（コンフリクトのマーカー付きの中身、その時点のファイルなど）。
    pub fn show_plain(&self, view: View) {
        self.replace_decor(Decor::default());
        {
            let mut st = self.state();
            self.ensure_font(&mut st);
            st.generation += 1;
            st.message = None;
            st.source = None;
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = false;
            st.checks = false;
            st.selection.clear();
            st.anchor = None;
            st.text_selection = None;
            st.hunk_cursor = None;
            st.rebuild_extra();
        }
        self.scroll_to_start();
        self.configure_adjustments();
        self.refresh_matches();
        self.queue_draw();
        self.selection_changed();
    }

    /// 先頭（左上）までスクロールする。
    fn scroll_to_start(&self) {
        for adj in [self.vadjustment(), self.hadjustment()]
            .into_iter()
            .flatten()
        {
            adj.set_value(0.0);
        }
    }

    /// 選んだ変更の行（表示の行の番号。Working Copy の差分では、コアの差分の行の番号と同じ）。
    /// 選んだ行のうち、変更の行（含める・外す・捨てるのはこの行。選ぶだけなら前後の行や hunk の見出しも選べる）。
    pub fn selected_rows(&self) -> Vec<usize> {
        let st = self.state();
        let Some(view) = &st.view else {
            return Vec::new();
        };
        st.selection
            .iter()
            .map(|&r| r as usize)
            .filter(|&r| view.view.rows.get(r).is_some_and(Self::is_change))
            .collect()
    }

    /// 選んだ行のうち、次のコミットに含めているものの数と、全体の数。
    pub fn selected_staged(&self) -> (usize, usize) {
        let st = self.state();
        let Some(view) = &st.view else { return (0, 0) };
        let changes: Vec<&DiffRow> = st
            .selection
            .iter()
            .filter_map(|&r| view.view.rows.get(r as usize))
            .filter(|r| Self::is_change(r))
            .collect();
        let staged = changes
            .iter()
            .filter(|r| r.flags & DiffRow::STAGED != 0)
            .count();
        (staged, changes.len())
    }

    /// ファイルの見出しか差分の行を右クリックしたら `f(見出しの上ならファイルの番号, パス, x, y)` を呼ぶ。
    pub fn connect_file_menu(&self, f: impl Fn(Option<usize>, &str, f64, f64) + 'static) {
        self.state().on_file_menu = Some(Rc::new(f));
    }

    /// 見出しのない差分の行を右クリックしたら `f(行, x, y)` を呼ぶ。
    pub fn connect_row_menu(&self, f: impl Fn(usize, f64, f64) + 'static) {
        self.state().on_row_menu = Some(Rc::new(f));
    }

    /// `y`（部品の中の位置）の行の番号。
    pub fn row_at(&self, y: f64) -> Option<usize> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let mut st = self.state();
        self.ensure_font(&mut st);
        let index = st.row_at(y as f32 + scroll);
        (index >= 0 && (index as u32) < st.row_count() && !st.hidden_rows.contains(&(index as u32)))
            .then_some(index as usize)
    }

    /// 出している差分の元（コミットの差分。Working Copy の差分では None）。
    pub fn source(&self) -> Option<Arc<Source>> {
        self.state().source.clone()
    }

    /// AI に渡す、`row` 行目を含む hunk の差分の文字（見出しから次の hunk・ファイルの手前まで）と、そのファイルの
    /// パス（見出しのない差分では None）。hunk の中でなければ None。
    pub fn hunk_text(&self, row: usize) -> Option<(Option<String>, String)> {
        let view = self.state().view.clone()?;
        let rows = &view.view.rows;
        let upto = rows.get(..=row)?;
        let start = upto.iter().rposition(|r| r.kind == DiffRow::HUNK)?;
        // 見出しより後ろにファイルの見出しがあれば、その hunk は別のファイル。
        if upto[start..].iter().any(|r| r.kind == DiffRow::FILE) {
            return None;
        }
        let path = upto[..start]
            .iter()
            .rev()
            .find(|r| r.kind == DiffRow::FILE)
            .map(|r| {
                let text = view.text(r);
                text.split(" — ").next().unwrap_or(text).to_owned()
            });
        let mut out = amonhen_assist::DiffText::new();
        if let Some(path) = &path {
            out.path(path);
        }
        for (i, r) in rows[start..].iter().enumerate() {
            if i > 0 && matches!(r.kind, DiffRow::HUNK | DiffRow::FILE) {
                break;
            }
            if !out.push(ai_line(r.kind), view.text(r)) {
                break;
            }
        }
        Some((path, out.finish()))
    }

    /// 最初の hunk（確認用）。
    pub fn first_hunk(&self) -> Option<(Option<String>, String)> {
        let row = {
            let st = self.state();
            st.view
                .as_ref()?
                .view
                .rows
                .iter()
                .position(|r| r.kind == DiffRow::HUNK)?
        };
        self.hunk_text(row)
    }

    fn file_menu(&self, x: f64, y: f64) {
        let (view, f, rows_f, marks) = {
            let st = self.state();
            (
                st.view.clone(),
                st.on_file_menu.clone(),
                st.on_row_menu.clone(),
                st.marks,
            )
        };
        let index = self.row_at(y);
        // 行を選ぶ差分（Working Copy）では、見出しの上でなければ行のメニュー（いくつかのファイルを並べても）。
        if let (Some(view), Some(rows_f), Some(index)) = (&view, rows_f, index)
            && (view.file_rows.is_empty() || (marks && self.header_at(y as f32).is_none()))
        {
            rows_f(index, x, y);
            return;
        }
        // 右クリックのメニューを持ち主が作らない差分では、何も出さない（コピーは Ctrl+C。macOS 版と同じ）。
        let (Some(view), Some(f)) = (view, f) else {
            return;
        };
        // 見出し（上に張り付いたものを含む）の上ならそのファイル、下の行ならその行のファイル（フォルダの行・
        // 欄の上では出さない）。
        let header = self.header_at(y as f32);
        if header.is_none() && (self.folder_at(y as f32).is_some() || self.in_panel(y as f32)) {
            return;
        }
        let file = header.or_else(|| {
            let index = index?;
            view.file_rows.iter().rposition(|&r| r as usize <= index)
        });
        let Some(file) = file else { return };
        // とびとびに選んだときの見出しは「パス — 短い ID」。
        let text = view.text(&view.view.rows[view.file_rows[file] as usize]);
        let path = text.split(" — ").next().unwrap_or(text);
        f(header, path, x, y);
    }

    /// ファイル `file` を畳んでいるか。
    pub fn file_collapsed(&self, file: usize) -> bool {
        self.state().collapsed.get(file).copied().unwrap_or(false)
    }

    /// 上に張り付かせる見出し（ファイルの番号と、見えている範囲の上端からの位置）。見出しの行が見えている範囲より
    /// 上にあるファイルのものを出し、次のファイルの見出しが近づいたら押し上げる（macOS 版と同じ）。
    fn sticky(st: &State, view: &View, scroll: f32) -> Option<(usize, f32)> {
        let rh = st.row_height;
        let first = st.row_at(scroll).max(0) as u32;
        let file = view
            .file_rows
            .iter()
            .rposition(|&r| r <= first && !st.hidden_rows.contains(&r))?;
        if st.row_y(view.file_rows[file]) >= scroll {
            return None;
        }
        let top = view
            .file_rows
            .get(file + 1)
            .map_or(0.0, |&next| (st.row_y(next) - scroll - rh).min(0.0));
        Some((file, top))
    }

    /// `y`（部品の中の位置）が、ファイルの見出し（上に張り付いたものを含む）なら、そのファイルの番号。
    fn header_at(&self, y: f32) -> Option<usize> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let mut st = self.state();
        self.ensure_font(&mut st);
        let view = st.view.clone()?;
        let rh = st.row_height;
        if let Some((file, top)) = Self::sticky(&st, &view, scroll)
            && y >= top
            && y < top + rh
        {
            return Some(file);
        }
        let row = st.row_at(y + scroll);
        if row < 0 || st.hidden_rows.contains(&(row as u32)) {
            return None;
        }
        // 見出しの下の欄の中は、見出しではない。
        if y + scroll >= st.row_y(row as u32) + rh {
            return None;
        }
        view.file_rows.iter().position(|&r| r as i64 == row)
    }

    /// Space で、選んだ変更の行を含める・外すときに `f(行)` を呼ぶ（Working Copy）。
    pub fn connect_toggle_rows(&self, f: impl Fn(Vec<usize>) + 'static) {
        self.state().on_toggle_rows = Some(Rc::new(f));
    }

    /// ⌥J・⌥K で、全ファイルを並べていない差分の次・前のファイルへ移るときに `f(+1・−1)` を呼ぶ。
    pub fn connect_move_file(&self, f: impl Fn(i32) + 'static) {
        self.state().on_move_file = Some(Rc::new(f));
    }

    /// キーの操作（macOS 版の差分と同じ）。扱ったら Stop。
    pub fn key_pressed(&self, key: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        let modifiers = modifiers
            & (gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SHIFT_MASK
                | gdk::ModifierType::SUPER_MASK);
        let plain = modifiers.is_empty();
        let alt = modifiers == gdk::ModifierType::ALT_MASK;
        let ctrl = modifiers == gdk::ModifierType::CONTROL_MASK;
        match key.to_lower() {
            gdk::Key::c if ctrl => {
                if !self.copy_selection() {
                    return glib::Propagation::Proceed;
                }
            }
            gdk::Key::a if ctrl => self.select_all(),
            gdk::Key::Escape if plain => {
                if !self.clear_text_selection() {
                    return glib::Propagation::Proceed;
                }
            }
            gdk::Key::space if plain => {
                let (marks, rows, f) = {
                    let st = self.state();
                    (
                        st.checks,
                        st.view.as_ref().map_or_else(Vec::new, |view| {
                            st.selection
                                .iter()
                                .map(|&r| r as usize)
                                .filter(|&r| view.view.rows.get(r).is_some_and(Self::is_change))
                                .collect::<Vec<_>>()
                        }),
                        st.on_toggle_rows.clone(),
                    )
                };
                let Some(f) = f.filter(|_| marks && !rows.is_empty()) else {
                    return glib::Propagation::Proceed;
                };
                // 切り替えた後は、次の hunk へ進む。
                let hunks = self.hunk_rows();
                if let Some(&last) = rows.iter().max()
                    && let Some(i) = hunks.iter().rposition(|&h| h as usize <= last)
                {
                    self.state().hunk_after = Some((i + 1).min(hunks.len() - 1));
                }
                f(rows);
            }
            gdk::Key::j if plain => self.move_hunk(1),
            gdk::Key::k if plain => self.move_hunk(-1),
            gdk::Key::j if alt => self.move_file(1),
            gdk::Key::k if alt => self.move_file(-1),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// hunk の見出しの行。
    fn hunk_rows(&self) -> Vec<u32> {
        self.rows_of(DiffRow::HUNK)
    }

    /// ファイルの見出しの行。
    pub fn file_rows(&self) -> Vec<u32> {
        self.state()
            .view
            .as_ref()
            .map_or_else(Vec::new, |v| v.file_rows.clone())
    }

    fn rows_of(&self, kind: u8) -> Vec<u32> {
        let st = self.state();
        let Some(view) = &st.view else {
            return Vec::new();
        };
        view.view
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind == kind)
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// 見えている一番上の行。
    fn top_row(&self) -> u32 {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let mut st = self.state();
        self.ensure_font(&mut st);
        st.row_at(scroll).max(0) as u32
    }

    /// J・K: 次・前の hunk を選んで（選べる差分なら）、上から 3 分の 1 あたりに出す。
    fn move_hunk(&self, step: i32) {
        let hunks = self.hunk_rows();
        if hunks.is_empty() {
            return self.error_bell();
        }
        let (first_selected, cursor) = {
            let st = self.state();
            (st.selection.first().copied(), st.hunk_cursor)
        };
        let current = first_selected
            .and_then(|row| hunks.iter().rposition(|&h| h <= row))
            .or(cursor);
        let target = match current {
            Some(c) => c as i64 + step as i64,
            None => {
                // 選んでいなければ、見えている一番上から: J はそこ以降の最初、K はそれより前の最後。
                let top = self.top_row();
                if step > 0 {
                    hunks
                        .iter()
                        .position(|&h| h >= top)
                        .map_or(hunks.len() as i64, |i| i as i64)
                } else {
                    hunks
                        .iter()
                        .rposition(|&h| h < top)
                        .map_or(-1, |i| i as i64)
                }
            }
        };
        if target < 0 || target as usize >= hunks.len() {
            return self.error_bell();
        }
        self.select_hunk(target as usize);
    }

    /// `index` 番目の hunk を選び（選べる差分なら、見出しから次の hunk までの行すべて。前後の行も。macOS 版と同じ）、
    /// 見えるところへスクロールする。
    pub fn select_hunk(&self, index: usize) {
        let hunks = self.hunk_rows();
        let Some(&start) = hunks.get(index) else {
            return;
        };
        let end = hunks.get(index + 1).copied().unwrap_or(self.row_count());
        {
            let mut st = self.state();
            st.hunk_cursor = Some(index);
            if st.marks {
                st.selection = (start..end).collect();
                st.anchor = Some(start);
            }
        }
        self.reveal_rows(start, end);
        self.queue_draw();
        self.selection_changed();
    }

    /// `start..end` の行が見えていなければ、`start` が上から 3 分の 1 あたりに来るようにスクロールする。
    fn reveal_rows(&self, start: u32, end: u32) {
        let Some(adj) = self.vadjustment() else {
            return;
        };
        let (top, bottom) = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            (st.row_y(start) as f64, st.row_y(end) as f64)
        };
        let (value, page) = (adj.value(), adj.page_size());
        if top < value || bottom > value + page {
            let y = (top - page / 3.0).clamp(0.0, (adj.upper() - page).max(0.0));
            adj.set_value(y);
        }
    }

    /// ⌥J・⌥K: 全ファイルを並べた差分なら次・前の見出しへ、そうでなければ持ち主に頼む。
    fn move_file(&self, step: i32) {
        let files = self.file_rows();
        if files.is_empty() {
            let f = self.state().on_move_file.clone();
            match f {
                Some(f) => f(step),
                None => self.error_bell(),
            }
            return;
        }
        // 畳んだフォルダの中の見出しは飛ばす。
        let hidden = self.state().hidden_rows.clone();
        let shown: Vec<usize> = (0..files.len())
            .filter(|&i| !hidden.contains(&files[i]))
            .collect();
        let top = self.top_row();
        let current = shown.iter().rposition(|&i| files[i] <= top).unwrap_or(0) as i64;
        let target = current + step as i64;
        if target < 0 || target as usize >= shown.len() {
            return self.error_bell();
        }
        self.scroll_to_file(shown[target as usize]);
    }

    /// 見えている一番上の行が属するファイル（macOS 版の `visibleTopFile`）。
    pub fn visible_top_file(&self) -> Option<usize> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let mut st = self.state();
        self.ensure_font(&mut st);
        let first = st.row_at(scroll).max(0) as u32;
        st.view
            .as_ref()?
            .file_rows
            .iter()
            .rposition(|&f| f <= first)
    }

    /// `index` 番目のファイルの見出しが一番上に来るようにスクロールする。
    pub fn scroll_to_file(&self, index: usize) {
        let y = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            st.file_row(index).map(|row| st.row_y(row) as f64)
        };
        let (Some(y), Some(adj)) = (y, self.vadjustment()) else {
            return;
        };
        adj.set_value(y.min((adj.upper() - adj.page_size()).max(0.0)));
    }

    pub fn connect_mark(&self, f: impl Fn(Vec<usize>) + 'static) {
        self.state().on_mark = Some(Rc::new(f));
    }

    pub fn connect_selection(&self, f: impl Fn() + 'static) {
        self.state().on_selection = Some(Rc::new(f));
    }

    fn selection_changed(&self) {
        let f = self.state().on_selection.clone();
        if let Some(f) = f {
            f();
        }
    }

    /// 変更の行（選べる行）か。
    fn is_change(row: &DiffRow) -> bool {
        matches!(row.kind, DiffRow::ADDED | DiffRow::REMOVED)
    }

    fn row_height(&self) -> f32 {
        let mut st = self.state();
        self.ensure_font(&mut st);
        st.row_height
    }

    fn row_count(&self) -> u32 {
        self.state().row_count()
    }

    fn ensure_font(&self, st: &mut State) {
        if st.font.is_some() {
            return;
        }
        let base = self.pango_context().font_description().unwrap_or_default();
        let mut font = pango::FontDescription::from_string("Monospace");
        crate::scale_font(&mut font, &base, 0.90);
        let layout = self.create_pango_layout(Some("Mg"));
        layout.set_font_description(Some(&font));
        st.row_height = (layout.pixel_size().1 as f32 + 3.0).round();
        let probe = self.create_pango_layout(Some(&"0".repeat(20)));
        probe.set_font_description(Some(&font));
        st.char_width = probe.pixel_size().0 as f32 / 20.0;
        st.font = Some(font);
        st.pango = Some(self.pango_context());
    }

    /// 部品の幅が変わった: 折り返していれば折り返し直す（一番上の行を保つ）。作り直しが高くつく差分では、幅が
    /// 止まってから（macOS 版と同じ）。
    fn width_changed(&self, width: f32) {
        let (wrap, cheap) = {
            let mut st = self.state();
            if st.width == width {
                return;
            }
            st.width = width;
            (
                st.wrap,
                st.wrap_cost <= REWRAP_BUDGET_MS || st.breaks_for.is_none(),
            )
        };
        if !wrap {
            self.state().wrap_width = width;
            return;
        }
        if let Some(id) = self.state().rewrap_pending.take() {
            id.remove();
        }
        if cheap {
            self.state().wrap_width = width;
            self.rewrap_keeping_top();
            return;
        }
        let weak = self.downgrade();
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
            if let Some(view) = weak.upgrade() {
                {
                    let mut st = view.state();
                    st.rewrap_pending = None;
                    st.wrap_width = st.width;
                }
                view.rewrap_keeping_top();
            }
        });
        self.state().rewrap_pending = Some(id);
    }

    /// `--measure-diff-scroll`: 本文の幅 `widths` ごとに折り返しを作り直した時間（ms）と、折り返した行の数。
    /// 測ったら今の幅で作り直す。
    pub fn measure_rewrap(&self, widths: &[f32]) -> Vec<(f32, f64, usize)> {
        let mut out = Vec::new();
        for &w in widths {
            let started = std::time::Instant::now();
            let mut st = self.state();
            st.wrap_width = w;
            st.rebuild_extra();
            out.push((w, started.elapsed().as_secs_f64() * 1000.0, st.breaks.len()));
        }
        {
            let mut st = self.state();
            st.wrap_width = st.width;
        }
        self.rewrap_keeping_top();
        out
    }

    /// 行の数（`--measure-diff-scroll`）。
    pub fn row_total(&self) -> u32 {
        self.state().row_count()
    }

    /// 折り返しを作り直し、一番上に見えていた行を一番上に保つ。
    fn rewrap_keeping_top(&self) {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let (top, delta) = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            let top = st.row_at(scroll).max(0) as u32;
            let delta = scroll - st.row_y(top);
            st.rebuild_extra();
            (top, delta)
        };
        self.configure_adjustments();
        let y = {
            let st = self.state();
            // 同じ行の続きの行の中ほどにいたら、その行の中で近いところに（行より下にはみ出さない）。
            let height = st.row_y(top + 1) - st.row_y(top);
            st.row_y(top) + delta.clamp(0.0, (height - st.row_height).max(0.0))
        };
        if let Some(adj) = self.vadjustment() {
            adj.set_value((y as f64).min((adj.upper() - adj.page_size()).max(0.0)));
        }
        self.queue_allocate();
        self.queue_draw();
    }

    /// 本文の左端（印・行番号の列の右。行番号の列の幅は、等幅なので桁数から）。行番号と印は横にスクロールしない。
    fn text_left(st: &State, view: &View) -> f32 {
        let left = if Self::mark_column(st) {
            TEXT_INSET + MARK_WIDTH
        } else {
            TEXT_INSET
        };
        // その時点のファイルは、後の側の行番号だけ。
        let columns = if st.file_mode {
            view.digits
        } else {
            view.digits * 2 + 1
        };
        left + st.char_width * columns as f32 + GUTTER_GAP
    }

    /// 横にスクロールした位置。
    fn scroll_x(&self) -> f32 {
        self.hadjustment().map_or(0.0, |a| a.value()) as f32
    }

    fn configure_adjustments(&self) {
        let (width, height) = (self.width() as f64, self.height() as f64);
        let content = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            st.content_height() as f64
        };
        let upper = (content + 8.0).max(height);
        if let Some(adj) = self.vadjustment() {
            let value = adj.value().clamp(0.0, (upper - height).max(0.0));
            let step = self.row_height() as f64 * 3.0;
            adj.configure(value, 0.0, upper, step, height * 0.9, height);
        }
        // 横は一番長い行まで（折り返さない。macOS 版と同じ）。折り返すなら横にはスクロールしない。
        let (content, step) = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            let content = st.view.clone().filter(|_| !st.wrap).map_or(0.0, |view| {
                Self::text_left(&st, &view) + view.columns as f32 * st.char_width + 40.0
            });
            (content as f64, st.char_width as f64 * 4.0)
        };
        if let Some(adj) = self.hadjustment() {
            let upper = content.max(width);
            let value = adj.value().clamp(0.0, (upper - width).max(0.0));
            adj.configure(value, 0.0, upper, step, width * 0.9, width);
        }
    }

    /// ファイルの見出しなら畳む・開く。Working Copy では印なら含める・外す、ほかは行を選ぶ。行を選ばない表示では、
    /// 文字を選び始める（`clicks` が 2 なら単語、3 以上なら行）。
    fn clicked(&self, x: f32, y: f32, shift: bool, ctrl: bool, clicks: i32) {
        let (marks, checks, view) = {
            let mut st = self.state();
            st.dragging = false;
            (st.marks, st.checks, st.view.clone())
        };
        let Some(view) = view else { return };
        if !marks {
            // 見出しの下の欄の中は、欄の部品が受ける。
            if self.in_panel(y) && self.header_at(y).is_none() {
                return;
            }
            if let Some(file) = self.header_at(y) {
                // 右端の ⋯: 右クリックと同じメニュー。ほかは開く・畳む。
                if x > self.width() as f32 - MORE_WIDTH {
                    self.file_menu(x as f64, y as f64);
                    return;
                }
                let f = self.state().on_toggle_file.clone();
                match f {
                    Some(f) => f(file),
                    None => self.set_file_collapsed(file, !self.file_collapsed(file)),
                }
                return;
            }
            if let Some(folder) = self.folder_at(y) {
                let f = self.state().on_toggle_folder.clone();
                if let Some(f) = f {
                    f(folder);
                }
                return;
            }
            // hunk の見出しの左の印: そのファイルの前後の行を広げる。
            let expand = {
                let st = self.state();
                st.on_expand_hunk
                    .clone()
                    .filter(|_| x < Self::text_left(&st, &view) - GUTTER_GAP / 2.0)
            };
            if let Some(f) = expand
                && let Some(row) = self.row_at(y as f64)
                && view.view.rows[row].kind == DiffRow::HUNK
                && let Some(file) = view.file_rows.iter().rposition(|&r| r as usize <= row)
            {
                f(file);
                return;
            }
            self.begin_text_selection(x, y, shift, clicks);
            return;
        }
        // いくつかのファイルを並べた Working Copy の差分: 見出しを押すとそのファイルを畳む・開く、右端の ⋯ は
        // そのファイルのメニュー。
        if !view.file_rows.is_empty()
            && let Some(file) = self.header_at(y)
        {
            if x > self.width() as f32 - MORE_WIDTH {
                self.file_menu(x as f64, y as f64);
                return;
            }
            let f = self.state().on_toggle_file.clone();
            if let Some(f) = f {
                f(file);
            }
            return;
        }
        let Some(index) = self.row_at(y as f64) else {
            return;
        };
        let index = index as u32;
        let rows = &view.view.rows;
        let row = rows[index as usize];
        let (folds, bar, mark_column, text_x) = {
            let st = self.state();
            (
                st.folds,
                st.stage_bar && (st.checks || st.split_side.is_some()),
                Self::mark_column(&st),
                Self::text_left(&st, &view),
            )
        };
        // 隙間の行: ボタン・札で開く・畳む、hunk の操作。行のほかの所は何もしない（選ばない）。
        if folds && row.kind == DiffRow::HUNK {
            self.gap_click(x, y, index);
            return;
        }
        // 「変更の帯」の形: 帯を押すとかたまり、行番号を押す・ドラッグするとその行（離したとき）。
        if folds && bar && Self::is_change(&row) && x < text_x - GUTTER_GAP / 2.0 {
            if mark_column && x < TEXT_INSET + MARK_WIDTH {
                let (start, end) = Self::change_block(rows, index as usize);
                self.stage_rows((start..end).collect());
            } else {
                self.state().number_drag = Some((index, vec![index]));
                self.queue_draw();
            }
            return;
        }
        // 左端のチェックボックス: その行（hunk の見出しなら hunk の変更すべて）を含める・外す（macOS 版と同じ）。
        if checks
            && x < TEXT_INSET + MARK_WIDTH
            && (Self::is_change(&row) || row.kind == DiffRow::HUNK)
        {
            let picked: Vec<usize> = if row.kind == DiffRow::HUNK {
                Self::hunk_changes(rows, index as usize).collect()
            } else {
                vec![index as usize]
            };
            let f = self.state().on_mark.clone();
            if let (Some(f), Some(&last)) = (f, picked.iter().max()) {
                // 切り替えた後は、次の hunk へ進む（Space と同じ）。
                let hunks = self.hunk_rows();
                if let Some(i) = hunks.iter().rposition(|&h| h as usize <= last) {
                    self.state().hunk_after = Some((i + 1).min(hunks.len() - 1));
                }
                f(picked);
            }
            return;
        }
        let before = if ctrl {
            self.state().selection.clone()
        } else {
            BTreeSet::new()
        };
        self.select_rows(&view, index, shift, ctrl);
        // ドラッグで範囲を選ぶ（押した行から。Ctrl なら前に選んでいた行に足す。macOS 版と同じ）。
        if !shift {
            self.state().row_drag = Some((index, before));
        }
    }

    /// `(x, y)`（部品の中の位置）の文字の位置。上にはみ出せば先頭、下にはみ出せば末尾。
    fn text_position(&self, x: f32, y: f32) -> Option<TextPos> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let (view, left, row, hidden) = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            let view = st.view.clone()?;
            let left = Self::text_left(&st, &view);
            let row = st.row_at(y + scroll);
            let hidden = row >= 0 && st.hidden_rows.contains(&(row as u32));
            (view, left, row, hidden)
        };
        let rows = &view.view.rows;
        let last = rows.len().checked_sub(1)?;
        if y + scroll < 0.0 || row < 0 {
            return Some((0, 0));
        }
        let row = row as usize;
        if row > last {
            return Some((last as u32, view.text(&rows[last]).len() as u32));
        }
        if rows[row].kind == DiffRow::FILE || hidden {
            return Some((row as u32, 0));
        }
        let layout = self.layout_of(row as u32)?;
        let text = view.text(&rows[row]);
        // 折り返した行なら、押した見た目の行の始まりと終わり。
        let (start, end) = {
            let st = self.state();
            let breaks = st.breaks.get(&(row as u32)).map_or(&[][..], Vec::as_slice);
            let k = (((y + scroll - st.row_y(row as u32)) / st.row_height)
                .floor()
                .max(0.0) as usize)
                .min(breaks.len());
            (
                if k == 0 { 0 } else { breaks[k - 1] as usize },
                breaks.get(k).map_or(text.len(), |&b| b as usize),
            )
        };
        let xa = if start == 0 {
            0.0
        } else {
            layout.index_to_pos(start as i32).x() as f32 / pango::SCALE as f32
        };
        let offset = x - left + self.scroll_x() + xa;
        if offset <= xa {
            return Some((row as u32, start as u32));
        }
        let (_, index, trailing) = layout.xy_to_index((offset * pango::SCALE as f32) as i32, 0);
        let mut index = floor_char_boundary(text, index.max(0) as usize);
        // 文字の後ろ半分なら、その文字の後ろ。
        for _ in 0..trailing {
            index += text[index..].chars().next().map_or(0, char::len_utf8);
        }
        Some((row as u32, index.clamp(start, end) as u32))
    }

    /// `p` を含む、クリックの回数の単位の範囲（2 なら単語、3 以上なら行、ほかは `p` そのもの）。
    fn unit_range(&self, p: TextPos, clicks: i32) -> (TextPos, TextPos) {
        if clicks < 2 {
            return (p, p);
        }
        let text = {
            let st = self.state();
            st.view
                .as_ref()
                .and_then(|v| v.view.rows.get(p.0 as usize).map(|r| v.text(r).to_owned()))
                .unwrap_or_default()
        };
        let (start, end) = if clicks == 2 {
            word_range(&text, p.1 as usize)
        } else {
            (0, text.len())
        };
        ((p.0, start as u32), (p.0, end as u32))
    }

    fn begin_text_selection(&self, x: f32, y: f32, shift: bool, clicks: i32) {
        let Some(p) = self.text_position(x, y) else {
            return;
        };
        let unit = self.unit_range(p, clicks);
        {
            let mut st = self.state();
            st.dragging = true;
            match st.text_selection {
                Some((anchor, _)) if shift => st.text_selection = Some((anchor, p)),
                _ => {
                    st.selection_unit = (clicks > 1).then_some((unit, clicks));
                    st.text_selection = Some(unit);
                }
            }
        }
        self.queue_draw();
    }

    /// ドラッグした: 選ぶ文字を `(x, y)` まで広げる（上下にはみ出せば、その向きにスクロールする）。
    fn drag_to(&self, x: f32, y: f32) {
        if self.drag_rows(y) {
            return;
        }
        let (dragging, current, unit) = {
            let st = self.state();
            (
                st.dragging && !st.marks,
                st.text_selection,
                st.selection_unit,
            )
        };
        let (true, Some(current)) = (dragging, current) else {
            return;
        };
        if let Some(adj) = self.vadjustment() {
            let step = self.row_height() as f64;
            let max = (adj.upper() - adj.page_size()).max(0.0);
            if y < 0.0 {
                adj.set_value((adj.value() - step).clamp(0.0, max));
            } else if y > self.height() as f32 {
                adj.set_value((adj.value() + step).clamp(0.0, max));
            }
        }
        let Some(p) = self.text_position(x, y) else {
            return;
        };
        let selection = match unit {
            // 単語・行の単位で選び始めたら、その単位のまま広げる。
            Some(((start, end), clicks)) => {
                let extended = self.unit_range(p, clicks);
                if p < start {
                    (end, extended.0)
                } else {
                    (start, end.max(extended.1))
                }
            }
            None => (current.0, p),
        };
        self.state().text_selection = Some(selection);
        self.queue_draw();
    }

    /// 選んだ文字（Working Copy の行を選ぶ表示では、選んだ行の文字）。行は改行でつなぐ。
    pub fn selected_text(&self) -> Option<String> {
        let st = self.state();
        let view = st.view.clone()?;
        let rows = &view.view.rows;
        if st.marks {
            if st.selection.is_empty() {
                return None;
            }
            let lines: Vec<&str> = st
                .selection
                .iter()
                .filter_map(|&r| rows.get(r as usize).map(|row| view.text(row)))
                .collect();
            return Some(lines.join("\n"));
        }
        let (a, b) = st.text_selection?;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        if start == end || end.0 as usize >= rows.len() {
            return None;
        }
        let lines: Vec<&str> = (start.0..=end.0)
            .map(|r| {
                let text = view.text(&rows[r as usize]);
                let from = if r == start.0 { start.1 as usize } else { 0 }.min(text.len());
                let to = if r == end.0 {
                    end.1 as usize
                } else {
                    text.len()
                }
                .min(text.len());
                text.get(from..to.max(from)).unwrap_or("")
            })
            .collect();
        Some(lines.join("\n"))
    }

    /// 選んだ文字をコピーする。選んでいなければ false。
    pub fn copy_selection(&self) -> bool {
        match self.selected_text() {
            Some(text) => {
                self.clipboard().set_text(&text);
                true
            }
            None => false,
        }
    }

    /// Ctrl+A: すべての文字を選ぶ（Working Copy の行を選ぶ表示では、すべての行）。
    fn select_all(&self) {
        let marks = {
            let mut st = self.state();
            let Some(view) = st.view.clone() else { return };
            let rows = &view.view.rows;
            let Some(last) = rows.len().checked_sub(1) else {
                return;
            };
            // 選べる差分ならすべての行（前後の行も。macOS 版と同じ）。
            if st.marks {
                st.selection = (0..rows.len() as u32).collect();
                st.anchor = None;
            } else {
                let end = view.text(&rows[last]).len() as u32;
                st.text_selection = Some(((0, 0), (last as u32, end)));
                st.selection_unit = None;
            }
            st.marks
        };
        self.queue_draw();
        if marks {
            self.selection_changed();
        }
    }

    /// Esc: 文字を選ぶのをやめる。選んでいなければ false。
    fn clear_text_selection(&self) -> bool {
        let had = self.state().text_selection.take().is_some();
        if had {
            self.queue_draw();
        }
        had
    }

    /// `hunk`（見出しの行）の中の変更の行。
    fn hunk_changes(rows: &[DiffRow], hunk: usize) -> impl Iterator<Item = usize> + '_ {
        rows.iter()
            .enumerate()
            .skip(hunk + 1)
            .take_while(|(_, r)| !matches!(r.kind, DiffRow::HUNK | DiffRow::FILE))
            .filter(|(_, r)| Self::is_change(r))
            .map(|(i, _)| i)
    }

    /// 行を選ぶ（macOS 版と同じく、前後の行も選べる）。hunk の見出しなら、その hunk の行をすべて。Shift で
    /// 起点からの続いた行、Ctrl で足す・外す（すべて選んでいれば外す）。
    fn select_rows(&self, view: &View, index: u32, shift: bool, ctrl: bool) {
        let rows = &view.view.rows;
        let picked: Vec<u32> = if rows[index as usize].kind == DiffRow::HUNK {
            std::iter::once(index)
                .chain((index + 1..rows.len() as u32).take_while(|&r| {
                    !matches!(rows[r as usize].kind, DiffRow::HUNK | DiffRow::FILE)
                }))
                .collect()
        } else {
            vec![index]
        };
        {
            let mut st = self.state();
            match (shift, st.anchor) {
                (true, Some(anchor)) => {
                    let (a, b) = (anchor.min(index), anchor.max(index));
                    st.selection = (a..=b).collect();
                }
                _ if ctrl => {
                    if picked.iter().all(|r| st.selection.contains(r)) {
                        for r in &picked {
                            st.selection.remove(r);
                        }
                    } else {
                        st.selection.extend(picked.iter().copied());
                    }
                    st.anchor = Some(index);
                }
                _ => {
                    st.selection = picked.into_iter().collect();
                    st.anchor = Some(index);
                }
            }
        }
        self.queue_draw();
        self.selection_changed();
    }

    /// ファイル `file` を畳む・開く（表示を裏で作り直す。スクロールの位置は保ち、畳んだファイルの見出しが上に
    /// 隠れていたら、見出しが見えるところまで戻す）。
    pub fn set_file_collapsed(&self, file: usize, collapsed: bool) {
        let (source, all, generation) = {
            let mut st = self.state();
            let Some(source) = st.source.clone() else {
                return;
            };
            match st.collapsed.get_mut(file) {
                Some(c) if *c != collapsed => *c = collapsed,
                _ => return,
            }
            st.generation += 1;
            st.message = None;
            (source, st.collapsed.clone(), st.generation)
        };
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(view) = gio::spawn_blocking(move || View::build(&source.lock(), &all)).await
            else {
                return;
            };
            let Some(widget) = weak.upgrade() else { return };
            {
                let mut st = widget.state();
                if st.generation != generation {
                    return;
                }
                st.view = Some(Arc::new(view));
                st.rows.clear();
                st.text_selection = None;
                st.rebuild_extra();
            }
            widget.configure_adjustments();
            widget.reveal_file_header(file);
            widget.refresh_matches();
            widget.queue_draw();
        });
    }

    /// 差分の中を `query` で探す（大文字・小文字は区別しない）。今の一致は、見えている位置より後の最初のもの。
    /// （今の一致の番号、一致の数）を返す。
    pub fn find(&self, query: &str) -> (Option<usize>, usize) {
        self.find_with(query, true)
    }

    /// 出し直した差分で一致を数え直す（今の一致は見えている所から。選びもスクロールもしない。macOS 版の
    /// `updateMatches(scrollToCurrent: false)`）。（今の一致の番号、一致の数）を返す。
    pub fn recount(&self, query: &str) -> (Option<usize>, usize) {
        self.find_with(query, false)
    }

    fn find_with(&self, query: &str, reveal: bool) -> (Option<usize>, usize) {
        self.state().find = query.to_owned();
        self.refresh_matches();
        let top = self.top_row();
        let current = {
            let mut st = self.state();
            let current = (!st.matches.is_empty())
                .then(|| st.matches.iter().position(|m| m.0 >= top).unwrap_or(0));
            st.current_match = current;
            current
        };
        if reveal {
            self.reveal_match();
        }
        self.queue_draw();
        (current, self.state().matches.len())
    }

    /// 次・前の一致へ（端からは反対の端へ）。（今の一致の番号、一致の数）を返す。
    pub fn find_move(&self, step: i32) -> (Option<usize>, usize) {
        let (current, count) = {
            let mut st = self.state();
            let n = st.matches.len() as i64;
            if n == 0 {
                drop(st);
                self.error_bell();
                return (None, 0);
            }
            let next = match st.current_match {
                Some(c) => (c as i64 + step as i64).rem_euclid(n),
                None if step > 0 => 0,
                None => n - 1,
            } as usize;
            st.current_match = Some(next);
            (Some(next), n as usize)
        };
        self.reveal_match();
        self.queue_draw();
        (current, count)
    }

    /// 探すのをやめる（強調を消す）。
    pub fn clear_find(&self) {
        {
            let mut st = self.state();
            st.find.clear();
            st.matches.clear();
            st.current_match = None;
        }
        self.queue_draw();
    }

    /// 表示を作り直したので、一致を探し直す（今の一致は、同じ行にあれば保つ）。
    fn refresh_matches(&self) {
        let mut st = self.state();
        let previous = st.current_match.and_then(|c| st.matches.get(c).copied());
        st.matches.clear();
        st.current_match = None;
        let (Some(view), false) = (st.view.clone(), st.find.is_empty()) else {
            return;
        };
        let query = st.find.clone();
        // ファイルの見出し（パス）も含め、すべての行から探す（macOS 版と同じ）。
        for (i, row) in view.view.rows.iter().enumerate() {
            for (start, end) in find_all(view.text(row), &query) {
                st.matches.push((i as u32, start as u32, end as u32));
            }
        }
        st.current_match = previous.and_then(|p| st.matches.iter().position(|m| *m == p));
    }

    /// 今の一致を選び（行を選ばない表示なら。Ctrl+C でコピーできるように）、見える位置までスクロールする。
    fn reveal_match(&self) {
        let row = {
            let mut st = self.state();
            let m = st.current_match.and_then(|c| st.matches.get(c)).copied();
            if let Some((row, start, end)) = m
                && !st.marks
            {
                st.text_selection = Some(((row, start), (row, end)));
                st.selection_unit = None;
            }
            m.map(|m| m.0)
        };
        if let Some(row) = row {
            self.reveal_rows(row, row + 1);
            self.reveal_columns(row);
        }
    }

    /// 今の一致が横に隠れていれば、見えるところまで横にスクロールする（macOS 版と同じ）。
    fn reveal_columns(&self, row: u32) {
        let (Some(adj), Some(layout)) = (self.hadjustment(), self.layout_of(row)) else {
            return;
        };
        let range = {
            let st = self.state();
            st.current_match
                .and_then(|c| st.matches.get(c))
                .map(|m| (m.1, m.2))
        };
        let Some((start, end)) = range else { return };
        let x = |index: u32| layout.index_to_pos(index as i32).x() as f64 / pango::SCALE as f64;
        let (x0, x1) = (x(start), x(end));
        let visible = {
            let st = self.state();
            let left = st.view.as_ref().map_or(0.0, |v| Self::text_left(&st, v));
            self.width() as f64 - left as f64
        };
        let value = adj.value();
        if x0 < value || x1 > value + visible {
            let max = (adj.upper() - adj.page_size()).max(0.0);
            adj.set_value((x0 - 40.0).clamp(0.0, max));
        }
    }

    /// `index` 行目の本文の Pango の配置（なければ作って覚える）。
    fn layout_of(&self, index: u32) -> Option<pango::Layout> {
        let mut st = self.state();
        self.ensure_font(&mut st);
        let view = st.view.clone()?;
        if index as usize >= view.view.rows.len() {
            return None;
        }
        if !st.rows.contains_key(&index) {
            let text = self.row_text(&st, &view, index);
            st.rows.insert(index, text);
        }
        Some(st.rows[&index].text.clone())
    }

    fn row_text(&self, st: &State, view: &View, index: u32) -> RowText {
        let font = st.font.as_ref().expect("font");
        let row = &view.view.rows[index as usize];
        let layout = |text: &str| {
            let l = self.create_pango_layout(Some(text));
            l.set_font_description(Some(font));
            l.set_single_paragraph_mode(true);
            l
        };
        let fg = self.color();
        if row.kind == DiffRow::FILE {
            // 見出し: パス（開閉の印・種類の印・行数・⋯ は `draw_header` で描く）。パスはふつうの太さで（行数を
            // 太字にする。macOS 版と同じ）。
            return RowText {
                gutter: None,
                text: layout(view.text(row)),
            };
        }
        let gutter = (row.kind != DiffRow::HUNK).then(|| {
            let n = |v: u32| {
                if v == 0 { String::new() } else { v.to_string() }
            };
            let w = view.digits;
            // その時点のファイルは、後の側の行番号だけ（ファイルごと消したなら前の側）。
            if st.file_mode {
                let line = if row.new_no == 0 {
                    row.old_no
                } else {
                    row.new_no
                };
                return layout(&format!("{:>w$}", n(line)));
            }
            // コンフリクトの行は、作業ツリーのファイルの行番号だけ。
            let old = if row.kind >= DiffRow::OURS {
                0
            } else {
                row.old_no
            };
            layout(&format!("{:>w$} {:>w$}", n(old), n(row.new_no)))
        });
        let text = view.text(row);
        let l = layout(text);
        let attrs = pango::AttrList::new();
        if let Some(spans) = view.syntax.get(&index) {
            for &(start, end, kind) in spans {
                if let Some(color) = syntax_color(kind, &fg) {
                    let mut a = rgb_attr(color);
                    a.set_start_index(start);
                    a.set_end_index(end);
                    attrs.insert(a);
                }
            }
        }
        if let Some(ranges) = view.highlights.get(&index) {
            // 変わった単語の色は配色テーマから（`theme.rs`）。
            let word = crate::theme::diff_color(
                if row.kind == DiffRow::ADDED {
                    DiffColor::AddedWord
                } else {
                    DiffColor::RemovedWord
                },
                &fg,
            );
            let channel = |v: f32| (v.clamp(0.0, 1.0) * 65535.0) as u16;
            for &(start, end) in ranges {
                let mut a = pango::AttrColor::new_background(
                    channel(word.red()),
                    channel(word.green()),
                    channel(word.blue()),
                );
                a.set_start_index(start);
                a.set_end_index(end);
                attrs.insert(a);
                let mut alpha = pango::AttrInt::new_background_alpha(channel(word.alpha()));
                alpha.set_start_index(start);
                alpha.set_end_index(end);
                attrs.insert(alpha);
            }
        }
        l.set_attributes(Some(&attrs));
        RowText { gutter, text: l }
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let (width, height) = (self.width() as f32, self.height() as f32);
        let scroll_y = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let scroll_x = self.scroll_x();
        let fg = self.color();
        let mut st = self.state();
        let st = &mut *st;
        self.ensure_font(st);
        // 差分の代わりの知らせ: 頼まれたもの、1 ファイルのバイナリ、変わったファイルのない差分（macOS 版と同じ）。
        let message = st.message.clone().or_else(|| {
            let view = st.view.as_ref()?;
            if view.view.binary && view.file_rows.is_empty() {
                Some(tr!("Binary file"))
            } else if view.view.rows.is_empty() && st.source.is_some() {
                Some(tr!("No changes"))
            } else {
                None
            }
        });
        if let Some(message) = message {
            let layout = self.create_pango_layout(Some(&message));
            let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(12.0, 12.0));
            snapshot.append_layout(&layout, &secondary);
            snapshot.restore();
            return;
        }
        let Some(view) = st.view.clone() else {
            return;
        };
        let rh = st.row_height;
        let count = view.view.rows.len() as u32;
        let first = (st.row_at(scroll_y).max(0) as u32).min(count);
        let last = ((st.row_at(scroll_y + height).max(0) as u32).saturating_add(2)).min(count);
        if st.rows.len() > 4096 || st.theme_generation != crate::theme::generation() {
            st.rows.clear();
            st.theme_generation = crate::theme::generation();
        }
        let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.5);
        let left = if Self::mark_column(st) {
            TEXT_INSET + MARK_WIDTH
        } else {
            TEXT_INSET
        };
        let text_x = Self::text_left(st, &view);
        // 本文は横にスクロールする（行番号と印はしない）。
        let origin = text_x - scroll_x;
        // 一致の行の範囲（一致は行の順に並んでいる）。
        let match_from = st.matches.partition_point(|m| m.0 < first);
        // 選んだ文字（フォーカスがなければ薄く）。
        let text_range = st
            .text_selection
            .filter(|_| !st.marks)
            .map(|(a, b)| if a <= b { (a, b) } else { (b, a) })
            .filter(|(a, b)| a != b);
        let selected_color = if self.has_focus() {
            gdk::RGBA::new(0.21, 0.52, 0.89, 0.40)
        } else {
            gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.18)
        };
        for index in first..last {
            if st.hidden_rows.contains(&index) {
                continue;
            }
            if !st.rows.contains_key(&index) {
                let text = self.row_text(st, &view, index);
                st.rows.insert(index, text);
            }
            let row = &view.view.rows[index as usize];
            let top = st.row_y(index) - scroll_y;
            if row.kind == DiffRow::FILE {
                self.draw_header(snapshot, st, &view, index, top, width, false);
                continue;
            }
            // 折り返した行は、続きの行の分だけ高い（行番号・印は最初の行に）。
            let row_h = rh * st.lines_of(index) as f32;
            // 選んだ行は行番号の欄だけを選んだ色で塗り、行の追加・削除の色はそのまま見せる（macOS 版と同じ）。
            if st.selection.contains(&index) {
                snapshot.append_color(
                    &gdk::RGBA::new(0.21, 0.52, 0.89, 0.35),
                    &graphene::Rect::new(0.0, top, (text_x - 6.0).max(0.0), row_h),
                );
            }
            // 行の地は配色テーマの差分の色（`theme.rs`。標準はコンフリクトの Current が青・Incoming が緑・共通の
            // 祖先が灰色・マーカーが橙で、macOS 版と同じ）。
            let background = match row.kind {
                DiffRow::ADDED => Some(DiffColor::Added),
                DiffRow::REMOVED => Some(DiffColor::Removed),
                DiffRow::HUNK if !st.folds => Some(DiffColor::Hunk),
                DiffRow::OURS => Some(DiffColor::Current),
                DiffRow::THEIRS => Some(DiffColor::Incoming),
                DiffRow::BASE => Some(DiffColor::Base),
                DiffRow::MARKER => Some(DiffColor::Marker),
                _ => None,
            };
            if st.modified_rows.contains(&index) {
                // その時点のファイルの、変えた行（消した行に続けて足した行）は青（macOS 版と同じ）。
                snapshot.append_color(
                    &gdk::RGBA::new(0.21, 0.52, 0.89, 0.2),
                    &graphene::Rect::new(0.0, top, width, row_h),
                );
            } else if let Some(kind) = background {
                snapshot.append_color(
                    &crate::theme::diff_color(kind, &fg),
                    &graphene::Rect::new(0.0, top, width, row_h),
                );
            }
            if st.folds && row.kind == DiffRow::HUNK {
                self.draw_gap_row(snapshot, st, &view, index, top, width, text_x);
                continue;
            }
            if row.kind == DiffRow::HUNK && st.on_expand_hunk.is_some() {
                self.draw_expand_mark(snapshot, top, rh, text_x - GUTTER_GAP / 2.0);
            }
            let line = &st.rows[&index];
            let (_, h) = line.text.pixel_size();
            let y = top + ((rh - h as f32) / 2.0).round();
            // 変更の行と hunk の見出しのチェックボックス。hunk は中の変更がすべて含まれていれば ✓、一部なら −
            // （macOS 版と同じ）。
            if st.folds && Self::mark_column(st) && Self::is_change(row) {
                self.draw_bar(snapshot, st, index, row, top, row_h, text_x, &fg);
            } else if st.checks {
                let check = if Self::is_change(row) {
                    Some(if row.flags & DiffRow::STAGED != 0 {
                        Check::On
                    } else {
                        Check::Off
                    })
                } else if row.kind == DiffRow::HUNK {
                    let (mut staged, mut total) = (0, 0);
                    for r in Self::hunk_changes(&view.view.rows, index as usize) {
                        total += 1;
                        if view.view.rows[r].flags & DiffRow::STAGED != 0 {
                            staged += 1;
                        }
                    }
                    (total > 0).then_some(match staged {
                        0 => Check::Off,
                        n if n == total => Check::On,
                        _ => Check::Mixed,
                    })
                } else {
                    None
                };
                if let Some(check) = check {
                    draw_mark(snapshot, TEXT_INSET, top, rh, check, &fg);
                }
            }
            if let Some(gutter) = &line.gutter {
                snapshot.save();
                snapshot.translate(&graphene::Point::new(
                    left,
                    on_baseline(y, &line.text, gutter),
                ));
                snapshot.append_layout(gutter, &secondary);
                snapshot.restore();
            }
            // 本文を、見た目の行ごとに描く（折り返していなければ 1 行）。続きの行は、同じ配置を左にずらして切り取る。
            let len = view.text(row).len() as u32;
            let breaks = st.breaks.get(&index).map_or(&[][..], Vec::as_slice);
            let last_line = breaks.len();
            let x = |i: u32| line.text.index_to_pos(i as i32).x() as f32 / pango::SCALE as f32;
            // hunk の見出しとコンフリクトのマーカーの文字は薄く（macOS 版と同じ）。
            let color = if row.kind == DiffRow::HUNK || row.kind == DiffRow::MARKER {
                &secondary
            } else {
                &fg
            };
            for k in 0..=last_line {
                let s = if k == 0 { 0 } else { breaks[k - 1] };
                let e = breaks.get(k).copied().unwrap_or(len);
                let xa = if k == 0 { 0.0 } else { x(s) };
                let line_top = top + k as f32 * rh;
                let base = origin - xa;
                // 続きのある行は、その行の終わりで切る（後ろの文字は次の行に描く）。
                let clip_width = if k < last_line {
                    x(e) - xa
                } else {
                    width - text_x
                };
                snapshot.push_clip(&graphene::Rect::new(
                    text_x,
                    line_top,
                    clip_width.min(width - text_x).max(0.0),
                    rh,
                ));
                // 範囲 [a, b) のこの見た目の行の分（左端と右端。`on` なら行の終わりより先へ続く）。
                let piece = |a: u32, b: u32, on: bool| -> Option<(f32, f32)> {
                    if b < s || (b == s && k > 0) || a > e || (a == e && k < last_line) {
                        return None;
                    }
                    let (lo, hi) = (a.max(s), b.min(e));
                    let x0 = x(lo);
                    let x1 = if on && k == last_line {
                        x(e) + 6.0
                    } else if b > e {
                        x(e)
                    } else {
                        x(hi)
                    };
                    Some((base + x0, x1 - x0))
                };
                if let Some((start, end)) = text_range
                    && start.0 <= index
                    && index <= end.0
                {
                    let a = if index == start.0 { start.1 } else { 0 };
                    // 次の行に続くときは、改行の分として少し右まで塗る。
                    let (b, on) = if index == end.0 {
                        (end.1, false)
                    } else {
                        (len, true)
                    };
                    if let Some((x0, w)) = piece(a, b, on)
                        && w > 0.0
                    {
                        snapshot.append_color(
                            &selected_color,
                            &graphene::Rect::new(x0, line_top, w, rh),
                        );
                    }
                }
                // 探している語の一致（今の一致は濃く）。
                for (i, m) in st.matches[match_from..]
                    .iter()
                    .enumerate()
                    .skip_while(|(_, m)| m.0 < index)
                    .take_while(|(_, m)| m.0 == index)
                {
                    let Some((x0, w)) = piece(m.1, m.2, false) else {
                        continue;
                    };
                    let color = if st.current_match == Some(match_from + i) {
                        gdk::RGBA::new(1.0, 0.6, 0.0, 0.75)
                    } else {
                        gdk::RGBA::new(1.0, 0.85, 0.0, 0.45)
                    };
                    snapshot.append_color(
                        &color,
                        &graphene::Rect::new(x0, line_top + 1.0, w.max(2.0), rh - 2.0),
                    );
                }
                snapshot.save();
                snapshot.translate(&graphene::Point::new(base, y + k as f32 * rh));
                snapshot.append_layout(&line.text, color);
                snapshot.restore();
                snapshot.pop();
            }
        }
        if !st.decor.folders.is_empty() {
            self.draw_folders(snapshot, st, &view, scroll_y, width, height);
        }
        if !st.deletion_marks.is_empty() {
            self.draw_deletion_marks(snapshot, st, scroll_y, text_x - GUTTER_GAP, height);
        }
        // 見出しの下の欄（子の部品）。
        let panels: Vec<gtk::Widget> = st.decor.panels.values().cloned().collect();
        for panel in &panels {
            if panel.is_child_visible() {
                self.snapshot_child(panel, snapshot);
            }
        }
        // 見出しの行が上に隠れたファイルは、見出しを上に張り付かせる。
        if let Some((file, top)) = Self::sticky(st, &view, scroll_y) {
            let index = view.file_rows[file];
            if !st.rows.contains_key(&index) {
                let text = self.row_text(st, &view, index);
                st.rows.insert(index, text);
            }
            self.draw_header(snapshot, st, &view, index, top, width, true);
        }
    }

    /// hunk の見出しの左（行番号の欄）の、前後の行を広げる印。
    fn draw_expand_mark(&self, snapshot: &gtk::Snapshot, top: f32, rh: f32, gutter: f32) {
        let accent = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
        snapshot.append_color(
            &gdk::RGBA::new(0.21, 0.52, 0.89, 0.18),
            &graphene::Rect::new(0.0, top, gutter, rh),
        );
        let layout = self.create_pango_layout(Some("⤒  ⤓"));
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrInt::new_weight(pango::Weight::Semibold));
        layout.set_attributes(Some(&attrs));
        let (w, h) = layout.pixel_size();
        snapshot.save();
        snapshot.translate(&graphene::Point::new(
            ((gutter - w as f32) / 2.0).round(),
            top + ((rh - h as f32) / 2.0).round(),
        ));
        snapshot.append_layout(&layout, &accent);
        snapshot.restore();
    }

    /// 木で見せるときの、見出しの前のフォルダの行（「▾  name/」を深さだけ字下げ）。
    fn draw_folders(
        &self,
        snapshot: &gtk::Snapshot,
        st: &State,
        view: &View,
        scroll_y: f32,
        width: f32,
        height: f32,
    ) {
        let rh = st.row_height;
        let fg = self.color();
        let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6);
        let band = self.band_color(&fg);
        for (&file, folders) in &st.decor.folders {
            let Some(&row) = view.file_rows.get(file) else {
                continue;
            };
            let bottom = st.row_y(row) - scroll_y;
            let top = bottom - folders.len() as f32 * rh;
            if bottom <= 0.0 || top >= height {
                continue;
            }
            for (i, folder) in folders.iter().enumerate() {
                let y = top + i as f32 * rh;
                snapshot.append_color(&band, &graphene::Rect::new(0.0, y, width, rh));
                snapshot.append_color(
                    &gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.15),
                    &graphene::Rect::new(0.0, y + rh - 1.0, width, 1.0),
                );
                let mark = if folder.collapsed { "▸" } else { "▾" };
                let layout = self.create_pango_layout(Some(&format!("{mark}  {}/", folder.name)));
                let attrs = pango::AttrList::new();
                attrs.insert(pango::AttrInt::new_weight(pango::Weight::Semibold));
                layout.set_attributes(Some(&attrs));
                let (_, h) = layout.pixel_size();
                snapshot.save();
                snapshot.translate(&graphene::Point::new(
                    6.0 + folder.depth as f32 * 14.0,
                    y + ((rh - h as f32) / 2.0).round(),
                ));
                snapshot.append_layout(&layout, &secondary);
                snapshot.restore();
            }
        }
    }

    /// その時点のファイルの、消したところの印の行（「ここで N 行を削除」）。
    fn draw_deletion_marks(
        &self,
        snapshot: &gtk::Snapshot,
        st: &State,
        scroll_y: f32,
        gutter: f32,
        height: f32,
    ) {
        let rh = st.row_height;
        let red = gdk::RGBA::new(0.88, 0.11, 0.14, 1.0);
        for (after, count, _) in &st.deletion_marks {
            let top = st.row_y(after + 1) - rh - scroll_y;
            if top + rh <= 0.0 || top >= height {
                continue;
            }
            let layout =
                self.create_pango_layout(Some(&tr!("{count} lines deleted here", count = count)));
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Medium));
            attrs.insert(pango::AttrFloat::new_scale(0.85));
            layout.set_attributes(Some(&attrs));
            let (w, h) = layout.pixel_size();
            snapshot.append_color(
                &gdk::RGBA::new(red.red(), red.green(), red.blue(), 0.16),
                &graphene::Rect::new(gutter + 4.0, top + 2.0, w as f32 + 16.0, rh - 4.0),
            );
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                gutter + 12.0,
                top + ((rh - h as f32) / 2.0).round(),
            ));
            snapshot.append_layout(&layout, &red);
            snapshot.restore();
        }
    }

    /// 見出し・フォルダの行の地の色（窓に 1 枚の地を敷いていれば、地と同じ濃さ。`backdrop.rs`）。
    fn band_color(&self, fg: &gdk::RGBA) -> gdk::RGBA {
        let alpha = crate::backdrop::of(self).band_alpha().unwrap_or(1.0);
        if fg.red() > 0.5 {
            gdk::RGBA::new(0.17, 0.17, 0.18, alpha)
        } else {
            gdk::RGBA::new(0.96, 0.96, 0.96, alpha)
        }
    }

    /// ファイルの見出しの行（変更ファイルの一覧の行を兼ねる。macOS 版の `drawCompactHeader`）: ▸・▾、種類の印、
    /// パス（木なら名前）、右に行数と、いつも出す ⋯（右クリックと同じメニュー）。横にはスクロールしない。
    /// `opaque` なら下の行を隠す（上に張り付かせるとき）。
    #[allow(clippy::too_many_arguments)]
    fn draw_header(
        &self,
        snapshot: &gtk::Snapshot,
        st: &State,
        view: &View,
        index: u32,
        top: f32,
        width: f32,
        opaque: bool,
    ) {
        let rh = st.row_height;
        let fg = self.color();
        let dark = fg.red() > 0.5;
        let file = view.file_rows.iter().position(|&r| r == index).unwrap_or(0);
        let hovered = st.hovered_file == Some(file);
        if opaque {
            snapshot.append_color(
                &self.band_color(&fg),
                &graphene::Rect::new(0.0, top, width, rh),
            );
        }
        snapshot.append_color(
            &gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.08),
            &graphene::Rect::new(0.0, top, width, rh),
        );
        if hovered {
            snapshot.append_color(
                &gdk::RGBA::new(0.21, 0.52, 0.89, 0.12),
                &graphene::Rect::new(0.0, top, width, rh),
            );
        }
        snapshot.append_color(
            &gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.15),
            &graphene::Rect::new(0.0, top + rh - 1.0, width, 1.0),
        );
        let row = &view.view.rows[index as usize];
        let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6);
        let small = |text: &str, weight: Option<pango::Weight>| {
            let l = self.create_pango_layout(Some(text));
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrFloat::new_scale(0.9));
            if let Some(weight) = weight {
                attrs.insert(pango::AttrInt::new_weight(weight));
            }
            l.set_attributes(Some(&attrs));
            l
        };
        let large = |text: &str| {
            let l = self.create_pango_layout(Some(text));
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrFloat::new_scale(1.15));
            l.set_attributes(Some(&attrs));
            l
        };
        let middle = |l: &pango::Layout| top + ((rh - l.pixel_size().1 as f32) / 2.0).round();
        // 右から順に置く場所を決め、パスを描いてから、右の側を描く。
        let mut right = width - 12.0;
        let mut later: Vec<(pango::Layout, f32, gdk::RGBA)> = Vec::new();
        // よく使う操作は、いつも出す ⋯ のメニュー（右クリックと同じ）。
        let more = large("⋯");
        let more_width = more.pixel_size().0 as f32;
        later.push((
            more,
            right - more_width,
            if hovered { fg } else { secondary },
        ));
        right -= more_width + 14.0;
        let pending = row.flags & DiffRow::PENDING != 0;
        let (added, removed) = if pending {
            st.decor.counts.get(&file).copied().unwrap_or((0, 0))
        } else {
            (row.new_no, row.old_no)
        };
        let (green, red) = if dark {
            (
                gdk::RGBA::new(0.4, 0.8, 0.5, 1.0),
                gdk::RGBA::new(1.0, 0.45, 0.45, 1.0),
            )
        } else {
            (
                gdk::RGBA::new(0.2, 0.6, 0.3, 1.0),
                gdk::RGBA::new(0.85, 0.25, 0.25, 1.0),
            )
        };
        // 行数（0 なら出さない）。バイナリは「Binary」、画像は「Image」（macOS 版と同じ）。
        let note = st
            .decor
            .notes
            .get(&file)
            .cloned()
            .or_else(|| (row.flags & DiffRow::BINARY != 0).then(|| tr!("Binary")));
        let counts: Vec<(pango::Layout, gdk::RGBA)> = match note {
            Some(note) => vec![(small(&note, None), secondary)],
            None if added == 0 && removed == 0 => Vec::new(),
            None => vec![
                (
                    small(&format!("+{added}"), Some(pango::Weight::Bold)),
                    green,
                ),
                (
                    small(&format!("−{removed}"), Some(pango::Weight::Bold)),
                    red,
                ),
            ],
        };
        let counts_width: f32 = counts
            .iter()
            .map(|(l, _)| l.pixel_size().0 as f32)
            .sum::<f32>()
            + 6.0 * (counts.len().max(1) - 1) as f32;
        let mut x = right - counts_width;
        for (l, color) in counts {
            let w = l.pixel_size().0 as f32;
            later.push((l, x, color));
            x += w + 6.0;
        }
        right -= counts_width + 14.0;
        let indent = st.decor.depths.get(&file).copied().unwrap_or(0) as f32 * 14.0;
        let open = row.flags & DiffRow::COLLAPSED == 0 || st.decor.open.contains(&file);
        let chevron = large(if open { "▾" } else { "▸" });
        snapshot.save();
        snapshot.translate(&graphene::Point::new(
            (indent + 12.0 - chevron.pixel_size().0 as f32 / 2.0).round(),
            middle(&chevron),
        ));
        snapshot.append_layout(&chevron, &secondary);
        snapshot.restore();
        let mut path_x = 26.0 + indent;
        if let Some(mark) = st.decor.marks.get(&file) {
            let l = self.create_pango_layout(Some(mark));
            let mut font = st.font.clone().unwrap_or_default();
            font.set_weight(pango::Weight::Bold);
            l.set_font_description(Some(&font));
            let color = gdk::RGBA::parse(mark_color(mark)).unwrap_or(fg);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(path_x, middle(&l)));
            snapshot.append_layout(&l, &color);
            snapshot.restore();
            path_x += 16.0;
        }
        // パスは行数の手前までに収める。
        let title = st.decor.titles.get(&file).map(|t| {
            let l = self.create_pango_layout(Some(t));
            l.set_font_description(st.font.as_ref());
            l
        });
        let line = &st.rows[&index];
        let path = title.as_ref().unwrap_or(&line.text);
        let y = middle(path);
        snapshot.save();
        snapshot.push_clip(&graphene::Rect::new(
            path_x,
            top,
            (right - 8.0 - path_x).max(0.0),
            rh,
        ));
        snapshot.translate(&graphene::Point::new(path_x, y));
        // パスの中の、探している語の一致（今の一致は濃く）。木などで名前だけを出しているときは、一致した文字を
        // 見出しの中で探す（macOS 版と同じ）。
        {
            let row_text = view.text(row);
            let title_text = st.decor.titles.get(&file);
            let from = st.matches.partition_point(|m| m.0 < index);
            for (i, m) in st.matches[from..]
                .iter()
                .enumerate()
                .take_while(|(_, m)| m.0 == index)
            {
                let (start, end) = match title_text {
                    Some(t) => {
                        let Some(found) = row_text
                            .get(m.1 as usize..m.2 as usize)
                            .and_then(|needle| find_all(t, needle).first().copied())
                        else {
                            continue;
                        };
                        (found.0 as i32, found.1 as i32)
                    }
                    None => (m.1 as i32, m.2 as i32),
                };
                let x0 = path.index_to_pos(start).x() as f32 / pango::SCALE as f32;
                let x1 = path.index_to_pos(end).x() as f32 / pango::SCALE as f32;
                let color = if st.current_match == Some(from + i) {
                    gdk::RGBA::new(1.0, 0.6, 0.0, 0.75)
                } else {
                    gdk::RGBA::new(1.0, 0.85, 0.0, 0.45)
                };
                snapshot.append_color(
                    &color,
                    &graphene::Rect::new(x0, top - y + 1.0, (x1 - x0).max(2.0), rh - 2.0),
                );
            }
        }
        snapshot.append_layout(path, &fg);
        snapshot.pop();
        snapshot.restore();
        for (l, x, color) in later {
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x, middle(&l)));
            snapshot.append_layout(&l, &color);
            snapshot.restore();
        }
    }
}

/// 変更の種類の印の色（追加は緑、削除は赤、変更は橙、名前の変更・コピーは青。macOS 版と同じ）。
pub fn mark_color(mark: &str) -> &'static str {
    match mark {
        "A" => "#2ec27e",
        "D" => "#e01b24",
        "M" => "#e66100",
        _ => "#3584e4",
    }
}

/// `main` を `y` に置いた行で、`other` を置く高さ（文字の下端の線を `main` に揃える。字の違いで上下にずれない）。
fn on_baseline(y: f32, main: &pango::Layout, other: &pango::Layout) -> f32 {
    y + (main.baseline() - other.baseline()) as f32 / pango::SCALE as f32
}

/// `index` 以下で一番近い文字の境目（`str::floor_char_boundary` は MSRV より新しい）。
fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut i = index.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// `text` の `index`（バイト位置）の文字を含む単語の範囲（ダブルクリック）。英数字と `_` の並び、空白の並び、
/// それ以外は 1 文字。
fn word_range(text: &str, index: usize) -> (usize, usize) {
    // 文字の途中を指していたら、その文字の頭に（Pango の位置がずれても落ちないように）。
    let index = floor_char_boundary(text, index);
    // 行末なら、その前の文字。
    let at = if index < text.len() {
        index
    } else {
        text[..index].char_indices().last().map_or(0, |(i, _)| i)
    };
    let Some(c) = text[at..].chars().next() else {
        return (index, index);
    };
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let kind = class(c);
    if kind == 2 {
        return (at, at + c.len_utf8());
    }
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| class(*c) == kind)
        .last()
        .map_or(at, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .find(|(_, c)| class(*c) != kind)
        .map_or(text.len(), |(i, _)| at + i);
    (start, end)
}

/// `text` を等幅で並べたときの桁数（タブは 8 桁ごと。全角など幅の広い文字は 2 桁として、多めに見積もる）。
fn columns(text: &str) -> usize {
    text.chars().fold(0, |n, c| match c {
        '\t' => (n / 8 + 1) * 8,
        c if c as u32 >= 0x1100 => n + 2,
        _ => n + 1,
    })
}

/// `text` の中の `query` の位置（バイト）。大文字・小文字は区別しない（小文字にして長さが変わる文字（`İ`・
/// ケルビン記号など）があれば、1 文字ずつ比べる）。
fn find_all(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    // 文字ごとに長さが変わらなければ、小文字にした文字列のバイトの位置がそのまま使える。
    let same_len = |s: &str| {
        s.is_ascii()
            || s.chars()
                .all(|c| c.to_lowercase().map(char::len_utf8).sum::<usize>() == c.len_utf8())
    };
    if !same_len(text) || !same_len(query) {
        return crate::messagefind::find_all(text, query);
    }
    let (hay, needle) = (text.to_lowercase(), query.to_lowercase());
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = hay[from..].find(&needle) {
        let start = from + i;
        out.push((start, start + needle.len()));
        from = start + needle.len().max(1);
        if from >= hay.len() {
            break;
        }
    }
    out
}

/// 次のコミットに含めるかの印（含めていれば塗りつぶしにチェック、外していれば枠だけ）。
/// stage のチェックボックスの状態。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Check {
    Off,
    On,
    /// hunk の変更の一部だけを含めている。
    Mixed,
}

/// 変更のない所（隙間）の行と、行の stage（Working Copy。macOS 版と同じ）。
impl DiffWidget {
    /// 次の `show_working` の差分を、隙間を開閉する形で出す。`openings` は隙間の番号 → 上・下から見せている行数、
    /// `split_side` は Unstaged・Staged の差分なら Some（Staged なら true）、`stage_bar` は「変更の帯」の形か。
    pub fn set_folds(
        &self,
        folds: bool,
        openings: HashMap<u32, (u32, u32)>,
        split_side: Option<bool>,
        stage_bar: bool,
    ) {
        let mut st = self.state();
        st.folds = folds;
        st.gap_openings = openings;
        st.split_side = split_side;
        st.stage_bar = stage_bar;
    }

    /// いくつかのファイルを並べた Working Copy の差分の、見出しの飾りと畳んだファイル（`show_working` の後に）。
    pub fn set_working_files(&self, decor: Decor, collapsed: Vec<bool>) {
        self.replace_decor(decor);
        self.state().collapsed = collapsed;
        self.queue_draw();
    }

    pub fn connect_set_gap(&self, f: impl Fn(u32, Option<(u32, u32)>) + 'static) {
        self.state().on_set_gap = Some(Rc::new(f));
    }

    pub fn connect_discard_rows(&self, f: impl Fn(Vec<usize>) + 'static) {
        self.state().on_discard_rows = Some(Rc::new(f));
    }

    /// 行の左に帯（All）・チェックの欄を置くか。
    fn mark_column(st: &State) -> bool {
        st.checks || (st.folds && st.stage_bar && st.split_side.is_some())
    }

    /// `row` を含む変更のかたまり（続いた追加・削除の行。始め、終わりの次）。
    fn change_block(rows: &[DiffRow], row: usize) -> (usize, usize) {
        let mut start = row;
        while start > 0 && Self::is_change(&rows[start - 1]) {
            start -= 1;
        }
        let mut end = row + 1;
        while end < rows.len() && Self::is_change(&rows[end]) {
            end += 1;
        }
        (start, end)
    }

    /// `rows` の変更を stage・unstage する（Unstaged・Staged ならその側の操作、All なら入れ替える）。
    fn stage_rows(&self, rows: Vec<usize>) {
        let f = self.state().on_mark.clone();
        if let Some(f) = f.filter(|_| !rows.is_empty()) {
            f(rows);
        }
    }

    /// `rows` に効く操作と名前（Stage・Unstage・Discard。`lines` なら Lines、そうでなければ Hunk）。
    fn stage_actions(
        st: &State,
        view: &View,
        rows: &[usize],
        lines: bool,
    ) -> Vec<(GapAction, String)> {
        if rows.is_empty() || !(st.checks || st.split_side.is_some()) {
            return Vec::new();
        }
        let noun = if lines { "Lines" } else { "Hunk" };
        if st.checks {
            let all = rows
                .iter()
                .all(|&r| view.view.rows[r].flags & DiffRow::STAGED != 0);
            return vec![if all {
                (GapAction::Unstage, format!("Unstage {noun}"))
            } else {
                (GapAction::Stage, format!("Stage {noun}"))
            }];
        }
        if st.split_side == Some(true) {
            return vec![(GapAction::Unstage, format!("Unstage {noun}"))];
        }
        let mut out = vec![(GapAction::Stage, format!("Stage {noun}"))];
        if st.on_discard_rows.is_some() {
            out.push((GapAction::Discard, format!("Discard {noun}")));
        }
        out
    }

    /// 右クリックのメニューの、選んだ行・その hunk・その行の Stage（Unstage）と Discard（名前と行）。
    pub fn stage_menu(&self, row: usize) -> Vec<(String, Vec<usize>, bool)> {
        let st = self.state();
        let Some(view) = st.view.clone() else {
            return Vec::new();
        };
        if !st.folds || row >= view.view.rows.len() {
            return Vec::new();
        }
        let rows = &view.view.rows;
        let mut out = Vec::new();
        let mut add = |picked: Vec<usize>, lines: bool, what: String| {
            for (action, title) in Self::stage_actions(&st, &view, &picked, lines) {
                let name = if lines {
                    title.replace("Lines", &what)
                } else {
                    title
                };
                out.push((name, picked.clone(), action == GapAction::Discard));
            }
        };
        let selected: Vec<usize> = st
            .selection
            .iter()
            .map(|&r| r as usize)
            .filter(|&r| Self::is_change(&rows[r]))
            .collect();
        if !selected.is_empty() {
            let n = selected.len();
            add(selected, true, tr!("{n} Selected Lines", n = n));
        }
        let header = if rows[row].kind == DiffRow::HUNK {
            Some(row)
        } else {
            (0..row)
                .rev()
                .take_while(|&i| rows[i].kind != DiffRow::FILE)
                .find(|&i| rows[i].kind == DiffRow::HUNK)
        };
        if let Some(h) = header {
            let changes: Vec<usize> = Self::hunk_changes(rows, h).collect();
            add(changes, false, String::new());
        }
        if Self::is_change(&rows[row]) {
            add(vec![row], true, tr!("This Line"));
        }
        out
    }

    /// 小さめの字の配置（隙間の行の札・ボタン・関数名）。
    fn small_layout(&self, text: &str, bold: bool) -> pango::Layout {
        let layout = self.create_pango_layout(Some(text));
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrFloat::new_scale(0.85));
        if bold {
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Semibold));
        }
        layout.set_attributes(Some(&attrs));
        layout
    }

    /// 隙間の行の置き場所（部品の中の座標）。左に関数名、中央に札、札の右に開く・畳む（マウスが乗っているとき）、
    /// 右端にその下の hunk の操作（「選んでから」の形）。hunk の行を選んでいれば、札は選んだ行の数になり、操作は
    /// 選んだ行に効く。
    fn gap_layout(
        &self,
        st: &State,
        view: &View,
        index: u32,
        top: f32,
        width: f32,
        text_x: f32,
    ) -> GapLayout {
        let rows = &view.view.rows;
        let row = rows[index as usize];
        let (hidden, shown) = (row.old_no, row.new_no);
        let function = view.text(&row).to_owned();
        let size = |l: &pango::Layout| {
            let (w, h) = l.pixel_size();
            (w as f32, h as f32)
        };
        // ファイルの頭・末尾の隙間か（いくつかのファイルを並べた差分では、そのファイルの中で）。
        let edge = |range: &mut dyn Iterator<Item = usize>| {
            for i in range {
                if rows[i].kind == DiffRow::FILE {
                    return true;
                }
                if Self::is_change(&rows[i]) {
                    return false;
                }
            }
            true
        };
        let leading = edge(&mut (0..index as usize).rev());
        let trailing = edge(&mut (index as usize + 1..rows.len()));
        let changes: Vec<usize> = if !st.stage_bar {
            Self::hunk_changes(rows, index as usize).collect()
        } else {
            Vec::new()
        };
        let selected: Vec<usize> = changes
            .iter()
            .copied()
            .filter(|&r| st.selection.contains(&(r as u32)))
            .collect();
        let pill_text = if !selected.is_empty() {
            tr!("{n} lines selected", n = selected.len())
        } else if hidden > 0 {
            format!("⋯  {}", tr!("{n} unchanged lines", n = hidden))
        } else if shown > 0 {
            tr!("{n} unchanged lines shown", n = shown)
        } else {
            String::new()
        };
        let mut gap_actions: Vec<(GapAction, String)> = Vec::new();
        if st.hovered_gap.is_some_and(|(r, _)| r == index) && selected.is_empty() {
            if hidden > GAP_STEP {
                if !leading {
                    gap_actions.push((
                        GapAction::Down,
                        format!("↓ {}", tr!("{n} lines", n = GAP_STEP)),
                    ));
                }
                if !trailing {
                    gap_actions.push((
                        GapAction::Up,
                        format!("↑ {}", tr!("{n} lines", n = GAP_STEP)),
                    ));
                }
            }
            if hidden > 0 {
                gap_actions.push((GapAction::All, tr!("All")));
            }
            if shown > 0 {
                gap_actions.push((GapAction::Collapse, tr!("Collapse")));
            }
        }
        let target = if selected.is_empty() {
            changes
        } else {
            selected.clone()
        };
        let hunk_actions = Self::stage_actions(st, view, &target, !selected.is_empty());
        let rh = st.row_height;
        let height = (rh - 4.0).max(10.0);
        let y = top + (rh - height) / 2.0;
        let mut buttons = Vec::new();
        // 右端の hunk の操作（右から並べる）。
        let mut right = width - 10.0;
        for (action, title) in hunk_actions.iter().rev() {
            let layout = self.small_layout(title, true);
            let w = size(&layout).0 + 12.0;
            buttons.push((
                *action,
                graphene::Rect::new(right - w, y, w, height),
                layout,
            ));
            right -= w + 2.0;
        }
        // 中央の札（自分の幅だけで中央に置く。ボタンが出ても動かない）と、その右の開く・畳む。
        let mut pill = None;
        if !pill_text.is_empty() {
            let layout = self.small_layout(&pill_text, false);
            let w = size(&layout).0 + 20.0;
            let mut x = (text_x + (width - text_x - w) / 2.0).max(text_x + 8.0);
            pill = Some((graphene::Rect::new(x, y, w, height), layout));
            x += w + 6.0;
            for (action, title) in gap_actions {
                let layout = self.small_layout(&title, true);
                let cw = size(&layout).0 + 16.0;
                if x + cw >= right - 8.0 {
                    break;
                }
                buttons.push((action, graphene::Rect::new(x, y, cw, height), layout));
                x += cw + 6.0;
            }
        }
        // 左の関数名（札にかかるなら出さない）。
        let function = (!function.is_empty())
            .then(|| self.small_layout(&format!("ƒ {function}"), false))
            .filter(|l| {
                let end = text_x + 8.0 + size(l).0;
                end + 12.0 < pill.as_ref().map_or(right, |(r, _)| r.x())
            })
            .map(|l| (text_x + 8.0, l));
        GapLayout {
            pill,
            pill_selected: !selected.is_empty(),
            buttons,
            function,
            target,
        }
    }

    /// 隙間の行: 破線と、札・ボタン・関数名。
    #[allow(clippy::too_many_arguments)]
    fn draw_gap_row(
        &self,
        snapshot: &gtk::Snapshot,
        st: &State,
        view: &View,
        index: u32,
        top: f32,
        width: f32,
        text_x: f32,
    ) {
        let layout = self.gap_layout(st, view, index, top, width, text_x);
        let fg = self.color();
        let rh = st.row_height;
        let accent = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
        let red = gdk::RGBA::new(0.88, 0.27, 0.25, 1.0);
        let faint = |a: f32| gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), a);
        let with_alpha = |c: &gdk::RGBA, a: f32| gdk::RGBA::new(c.red(), c.green(), c.blue(), a);
        let mid = (top + rh / 2.0).floor();
        let mut holes: Vec<(f32, f32)> = layout
            .buttons
            .iter()
            .map(|(_, r, _)| (r.x() - 6.0, r.x() + r.width() + 6.0))
            .collect();
        if let Some((r, _)) = &layout.pill {
            holes.push((r.x() - 6.0, r.x() + r.width() + 6.0));
        }
        if let Some((x, l)) = &layout.function {
            holes.push((x - 4.0, x + l.pixel_size().0 as f32 + 8.0));
        }
        let mut x = text_x + 4.0;
        while x < width - 4.0 {
            if !holes
                .iter()
                .any(|&(a, b)| (x >= a && x <= b) || (x + 4.0 >= a && x + 4.0 <= b))
            {
                snapshot.append_color(&faint(0.25), &graphene::Rect::new(x, mid, 4.0, 1.0));
            }
            x += 8.0;
        }
        let rounded =
            |snapshot: &gtk::Snapshot, rect: &graphene::Rect, radius: f32, color: &gdk::RGBA| {
                let size = graphene::Size::new(radius, radius);
                snapshot.push_rounded_clip(&gsk::RoundedRect::new(*rect, size, size, size, size));
                snapshot.append_color(color, rect);
                snapshot.pop();
            };
        let put = |snapshot: &gtk::Snapshot,
                   l: &pango::Layout,
                   rect: &graphene::Rect,
                   color: &gdk::RGBA| {
            let (w, h) = l.pixel_size();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                rect.x() + (rect.width() - w as f32) / 2.0,
                rect.y() + (rect.height() - h as f32) / 2.0,
            ));
            snapshot.append_layout(l, color);
            snapshot.restore();
        };
        if let Some((rect, l)) = &layout.pill {
            let hovered = st.hovered_gap == Some((index, None));
            let fill = if layout.pill_selected {
                with_alpha(&accent, 0.18)
            } else {
                faint(if hovered { 0.22 } else { 0.12 })
            };
            rounded(snapshot, rect, rect.height() / 2.0, &fill);
            put(
                snapshot,
                l,
                rect,
                &if layout.pill_selected {
                    accent
                } else {
                    faint(0.7)
                },
            );
        }
        for (action, rect, l) in &layout.buttons {
            let hovered = st.hovered_gap == Some((index, Some(*action)));
            let color = if *action == GapAction::Discard {
                red
            } else {
                accent
            };
            let hunk = matches!(
                action,
                GapAction::Stage | GapAction::Unstage | GapAction::Discard
            );
            if hunk {
                if hovered {
                    rounded(snapshot, rect, 4.0, &with_alpha(&color, 0.14));
                }
            } else {
                rounded(
                    snapshot,
                    rect,
                    rect.height() / 2.0,
                    &with_alpha(&color, if hovered { 0.30 } else { 0.14 }),
                );
            }
            put(snapshot, l, rect, &color);
        }
        if let Some((x, l)) = &layout.function {
            let h = l.pixel_size().1 as f32;
            snapshot.save();
            snapshot.translate(&graphene::Point::new(*x, top + (rh - h) / 2.0));
            snapshot.append_layout(l, &faint(0.45));
            snapshot.restore();
        }
    }

    /// 変更の行の左の帯: All では staged なら塗り、まだなら枠だけ。Unstaged・Staged（「変更の帯」の形）では、
    /// かたまりの印（乗ると太く、行番号の欄も薄く塗る）。
    #[allow(clippy::too_many_arguments)]
    fn draw_bar(
        &self,
        snapshot: &gtk::Snapshot,
        st: &State,
        index: u32,
        row: &DiffRow,
        top: f32,
        row_h: f32,
        text_x: f32,
        fg: &gdk::RGBA,
    ) {
        let accent = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
        let hot = st
            .hovered_block
            .is_some_and(|(a, b)| index >= a && index < b)
            || st
                .number_drag
                .as_ref()
                .is_some_and(|(_, rows)| rows.contains(&index));
        // 左端から少し離す（差分の左端の数 px は、一覧との境の仕切りが押されたものとして受け取る）。
        let x = TEXT_INSET + 8.0;
        let bar = if hot {
            snapshot.append_color(
                &gdk::RGBA::new(0.21, 0.52, 0.89, 0.14),
                &graphene::Rect::new(
                    TEXT_INSET + MARK_WIDTH,
                    top,
                    text_x - GUTTER_GAP - TEXT_INSET - MARK_WIDTH,
                    row_h,
                ),
            );
            graphene::Rect::new(x - 1.0, top, 6.0, row_h)
        } else {
            graphene::Rect::new(x, top, 4.0, row_h)
        };
        if !st.checks {
            let a = if hot { 1.0 } else { 0.55 };
            snapshot.append_color(
                &gdk::RGBA::new(accent.red(), accent.green(), accent.blue(), a),
                &bar,
            );
        } else if row.flags & DiffRow::STAGED != 0 {
            snapshot.append_color(&accent, &bar);
        } else {
            let line = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.5);
            snapshot.append_color(
                &line,
                &graphene::Rect::new(bar.x(), bar.y(), 1.0, bar.height()),
            );
            snapshot.append_color(
                &line,
                &graphene::Rect::new(bar.x() + bar.width() - 1.0, bar.y(), 1.0, bar.height()),
            );
        }
    }

    /// 隙間の行を押した: ボタンならその操作、札ならすべて開く（すべて開いていれば畳む）。
    fn gap_click(&self, x: f32, y: f32, index: u32) {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let width = self.width() as f32;
        let (layout, gap, row, opening, on_set, on_discard) = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            let Some(view) = st.view.clone() else { return };
            let top = st.row_y(index) - scroll;
            let text_x = Self::text_left(&st, &view);
            let layout = self.gap_layout(&st, &view, index, top, width, text_x);
            let Ok(gap) = st.gap_rows.binary_search(&index) else {
                return;
            };
            let gap = gap as u32;
            (
                layout,
                gap,
                view.view.rows[index as usize],
                st.gap_openings.get(&gap).copied().unwrap_or((0, 0)),
                st.on_set_gap.clone(),
                st.on_discard_rows.clone(),
            )
        };
        let p = graphene::Point::new(x, y);
        let inside = |r: &graphene::Rect| {
            graphene::Rect::new(r.x() - 2.0, r.y() - 3.0, r.width() + 4.0, r.height() + 6.0)
                .contains_point(&p)
        };
        let action = if let Some((a, _, _)) = layout.buttons.iter().find(|(_, r, _)| inside(r)) {
            *a
        } else if let Some((r, _)) = layout.pill.as_ref().filter(|_| !layout.pill_selected)
            && inside(r)
            && (row.old_no > 0 || row.new_no > 0)
        {
            if row.old_no > 0 {
                GapAction::All
            } else {
                GapAction::Collapse
            }
        } else {
            return;
        };
        let grow = |v: u32| v.saturating_add(GAP_STEP);
        let (top, bottom) = opening;
        let new = match action {
            GapAction::Down => Some((grow(top), bottom)),
            GapAction::Up => Some((top, grow(bottom))),
            // すべて開くときは、隙間の行を開いた行の上に残す（そこから畳める）。
            GapAction::All => Some((top, u32::MAX)),
            GapAction::Collapse => None,
            GapAction::Stage | GapAction::Unstage => {
                self.stage_rows(layout.target);
                return;
            }
            GapAction::Discard => {
                if let Some(f) = on_discard {
                    f(layout.target);
                }
                return;
            }
        };
        if let Some(f) = on_set {
            f(gap, new);
        }
    }

    /// マウスの位置（None なら外れた）から、乗っている隙間の行・ボタンと、帯の上のかたまりを決める。
    fn hover_gap(&self, at: Option<(f32, f32)>) {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let width = self.width() as f32;
        let (before, gap, block) = {
            let mut st = self.state();
            if !st.folds {
                return;
            }
            // 比べるための前の値（下でボタンを並べるために行だけ先に書く）。
            let before = (st.hovered_gap, st.hovered_block);
            self.ensure_font(&mut st);
            let Some(view) = st.view.clone() else { return };
            let found = at.and_then(|(x, y)| {
                let index = st.row_at(y + scroll);
                (index >= 0 && (index as u32) < st.row_count()).then_some((x, y, index as u32))
            });
            let (gap, block) = match found {
                Some((x, y, index)) if view.view.rows[index as usize].kind == DiffRow::HUNK => {
                    // 先に行だけ決めて（ボタンを並べるため）、ボタンに乗っているかを調べる。
                    if st.hovered_gap.map(|(r, _)| r) != Some(index) {
                        st.hovered_gap = Some((index, None));
                    }
                    let top = st.row_y(index) - scroll;
                    let text_x = Self::text_left(&st, &view);
                    let layout = self.gap_layout(&st, &view, index, top, width, text_x);
                    let p = graphene::Point::new(x, y);
                    let action = layout
                        .buttons
                        .iter()
                        .find(|(_, r, _)| {
                            graphene::Rect::new(
                                r.x() - 2.0,
                                r.y() - 3.0,
                                r.width() + 4.0,
                                r.height() + 6.0,
                            )
                            .contains_point(&p)
                        })
                        .map(|(a, _, _)| *a);
                    (Some((index, action)), None)
                }
                Some((x, _, index))
                    if st.stage_bar
                        && Self::mark_column(&st)
                        && x < TEXT_INSET + MARK_WIDTH
                        && Self::is_change(&view.view.rows[index as usize]) =>
                {
                    let (a, b) = Self::change_block(&view.view.rows, index as usize);
                    (None, Some((a as u32, b as u32)))
                }
                _ => (None, None),
            };
            (before, gap, block)
        };
        let changed = {
            let mut st = self.state();
            let changed = before != (gap, block);
            st.hovered_gap = gap;
            st.hovered_block = block;
            changed
        };
        if changed {
            self.queue_draw();
        }
    }

    /// 行番号の上・行のドラッグ。扱ったら true。
    fn drag_rows(&self, y: f32) -> bool {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let mut st = self.state();
        let Some(view) = st.view.clone() else {
            return false;
        };
        let count = view.view.rows.len();
        if count == 0 {
            return false;
        }
        let row = (st.row_at(y + scroll).max(0) as usize).min(count - 1) as u32;
        if let Some((anchor, _)) = st.number_drag.clone() {
            let rows: Vec<u32> = (anchor.min(row)..=anchor.max(row))
                .filter(|&r| Self::is_change(&view.view.rows[r as usize]))
                .collect();
            st.number_drag = Some((anchor, rows));
            drop(st);
            self.queue_draw();
            return true;
        }
        if let Some((anchor, base)) = st.row_drag.clone().filter(|_| st.marks) {
            let mut next = base;
            next.extend(anchor.min(row)..=anchor.max(row));
            if next != st.selection {
                st.selection = next;
                drop(st);
                self.queue_draw();
                self.selection_changed();
            }
            return true;
        }
        false
    }

    /// ドラッグを離した: 行番号の上のドラッグなら、その範囲を stage する。
    fn drag_end(&self) {
        let drag = {
            let mut st = self.state();
            st.row_drag = None;
            st.number_drag.take()
        };
        if let Some((_, rows)) = drag {
            self.stage_rows(rows.into_iter().map(|r| r as usize).collect());
            self.queue_draw();
        }
    }
}

fn draw_mark(snapshot: &gtk::Snapshot, x: f32, top: f32, rh: f32, check: Check, fg: &gdk::RGBA) {
    let size = (rh - 6.0).clamp(8.0, 12.0);
    let rect = graphene::Rect::new(x + 2.0, top + (rh - size) / 2.0, size, size);
    let path = gsk::PathBuilder::new();
    path.add_rounded_rect(&gsk::RoundedRect::from_rect(rect, 3.0));
    let path = path.to_path();
    if check != Check::Off {
        snapshot.append_fill(
            &path,
            gsk::FillRule::Winding,
            &gdk::RGBA::new(0.21, 0.52, 0.89, 1.0),
        );
        let mark = gsk::PathBuilder::new();
        if check == Check::On {
            mark.move_to(rect.x() + size * 0.22, rect.y() + size * 0.52);
            mark.line_to(rect.x() + size * 0.42, rect.y() + size * 0.72);
            mark.line_to(rect.x() + size * 0.78, rect.y() + size * 0.3);
        } else {
            mark.move_to(rect.x() + size * 0.25, rect.y() + size * 0.5);
            mark.line_to(rect.x() + size * 0.75, rect.y() + size * 0.5);
        }
        let stroke = gsk::Stroke::new(1.6);
        stroke.set_line_cap(gsk::LineCap::Round);
        snapshot.append_stroke(&mark.to_path(), &stroke, &gdk::RGBA::WHITE);
    } else {
        let color = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.45);
        snapshot.append_stroke(&path, &gsk::Stroke::new(1.0), &color);
    }
}

fn rgb_attr((r, g, b): (f32, f32, f32)) -> pango::AttrColor {
    pango::AttrColor::new_foreground(
        (r * 65535.0) as u16,
        (g * 65535.0) as u16,
        (b * 65535.0) as u16,
    )
}

/// 構文の種類ごとの色（Settings > Theme で選んだテーマ）。`None` は本文の色のまま。
pub(crate) fn syntax_color(kind: u8, fg: &gdk::RGBA) -> Option<(f32, f32, f32)> {
    crate::theme::syntax_color(kind, fg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_without_case_when_lowercasing_changes_the_length() {
        assert_eq!(find_all("Fix the FIX", "fix"), vec![(0, 3), (8, 11)]);
        // `İ` は小文字で 3 バイト、ケルビン記号は 1 バイトになる。
        assert_eq!(find_all("İstanbul TODO", "todo"), vec![(10, 14)]);
        assert_eq!(find_all("İstanbul", "istanbul"), vec![(0, 9)]);
        assert_eq!(find_all("\u{212A}elvin todo", "TODO"), vec![(9, 13)]);
        assert_eq!(find_all("todo", "\u{212A}"), Vec::new());
        // 伸びる字と縮む字で長さの合計が変わらなくても、位置はずれない。
        assert_eq!(find_all("İİ\u{212A} todo", "todo"), vec![(8, 12)]);
        assert!(find_all("abc", "").is_empty());
    }

    #[test]
    fn rows_move_down_past_panels_and_folders_and_up_past_hidden_rows() {
        let mut st = State {
            row_height: 10.0,
            ..State::default()
        };
        // 行 0 の後ろに欄（100）、行 2 の前にフォルダの行 2 つ（行 1 の後ろ）、行 4・5 を隠す。
        st.set_extra(vec![(0, 100.0), (1, 20.0), (5, -20.0)]);
        assert_eq!(st.row_y(0), 0.0);
        assert_eq!(st.row_y(1), 110.0);
        assert_eq!(st.row_y(2), 140.0);
        assert_eq!(st.row_y(6), 160.0);
        // 欄の中は見出しの行、フォルダの行の中は前の行。
        assert_eq!(st.row_at(5.0), 0);
        assert_eq!(st.row_at(50.0), 0);
        assert_eq!(st.row_at(115.0), 1);
        assert_eq!(st.row_at(125.0), 1);
        assert_eq!(st.row_at(145.0), 2);
        assert_eq!(st.row_at(165.0), 6);
        st.set_extra(Vec::new());
        assert_eq!(st.row_at(25.0), 2);
    }

    #[test]
    fn wrapped_rows_are_found_by_binary_search() {
        let mut st = State {
            row_height: 10.0,
            ..State::default()
        };
        // 行 1 が 3 行に、行 4 が 2 行に折り返し、行 6 の後ろに欄（詰めるものがないので二分探索）。
        st.set_extra(vec![(1, 20.0), (4, 10.0), (6, 50.0)]);
        assert!(!st.extra_shrinks);
        assert_eq!(st.row_y(1), 10.0);
        assert_eq!(st.row_y(2), 40.0);
        assert_eq!(st.row_y(5), 80.0);
        assert_eq!(st.row_y(7), 150.0);
        assert_eq!(st.row_at(15.0), 1);
        assert_eq!(st.row_at(35.0), 1);
        assert_eq!(st.row_at(45.0), 2);
        assert_eq!(st.row_at(75.0), 4);
        assert_eq!(st.row_at(85.0), 5);
        assert_eq!(st.row_at(120.0), 6);
        assert_eq!(st.row_at(155.0), 7);
    }

    #[test]
    fn long_lines_break_after_spaces_in_the_latter_half() {
        let at = |text: &str, width: usize| {
            let b = text.as_bytes();
            break_points(
                text.len(),
                width as f32,
                |i| i as f32,
                |i| i as u32,
                |i| b[i] as char,
            )
        };
        // 収まれば折り返さない。
        assert!(at("short", 10).is_empty());
        // 後ろ半分の空白の後ろで。
        assert_eq!(at("aaaaa bbbbbbb", 8), [6]);
        // 前半にしか空白がなければ、幅いっぱいで。
        assert_eq!(at("a bbbbbbbbbbbb", 8), [8]);
        // 続けて何度も。
        assert_eq!(at(&"x".repeat(25), 10), [10, 20]);
    }

    #[test]
    fn columns_count_tabs_and_wide_characters() {
        assert_eq!(columns(""), 0);
        assert_eq!(columns("abc"), 3);
        assert_eq!(columns("\tx"), 9);
        assert_eq!(columns("ab\tx"), 9);
        assert_eq!(columns("日本"), 4);
    }

    #[test]
    fn word_range_picks_words_spaces_and_symbols() {
        let text = "let foo_bar = 日本語;";
        assert_eq!(word_range(text, 5), (4, 11));
        assert_eq!(word_range(text, 4), (4, 11));
        assert_eq!(word_range(text, 3), (3, 4));
        assert_eq!(word_range(text, 12), (12, 13));
        assert_eq!(word_range(text, 14), (14, 23));
        assert_eq!(word_range(text, text.len()), (23, 24));
        assert_eq!(word_range("", 0), (0, 0));
        // 文字の途中を指しても落ちず、その文字の単語に。
        assert_eq!(word_range(text, 15), (14, 23));
        assert_eq!(word_range("日本", 100), (0, 6));
    }
}
