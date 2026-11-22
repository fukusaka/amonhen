//! History の表（グラフ・ref のラベル・件名・作者と日時）。
//!
//! 行は `GtkListView` などの部品にせず、見えている行だけを 1 つのウィジェットの `snapshot` で描く
//! （150 万行でも部品を作らない）。`GtkScrollable` なので `GtkScrolledWindow` に入れればスクロールする。
//! 件名などは 256 行ずつ裏のスレッドで読み（`Repository::history_chunk`）、見えている所の近くだけを持つ。

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use amonhen_core::lineage::LineageWindow;
use amonhen_core::{HistorySnapshot, RefsSnapshot, Repository};
use amonhen_model::{Oid, RefKind, tr};
use amonhen_projection::{FrontendProfile, HistoryChunk};
use gtk::glib::Properties;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk, pango};

use crate::github::Ci;
use crate::graph::{self, Lanes, Metrics};
use crate::historyedit::Action;
use crate::settings::items;
use crate::signature::Status;

/// 行を右クリックしたときに呼ぶ（行、x、y）。
type MenuCallback = Rc<dyn Fn(u32, f64, f64)>;
/// 選んだ行が変わったときに呼ぶ（選んでいる行、昇順）。
type SelectCallback = Rc<dyn Fn(&[u32])>;
/// 行の右端の CI の印を押したときに呼ぶ（コミット、状態、印の位置）。
type CiCallback = Rc<dyn Fn(Oid, Ci, gdk::Rectangle)>;
/// CI の印に空ける幅（行の右端から）。
const CI_SPACE: f32 = 18.0;

const CHUNK_ROWS: u32 = 256;
/// 1 フレームに伸ばす祖先の行数（linux で中央値 約 2 ms、最大 約 4 ms。macOS 版と同じ）。
const LINEAGE_ROWS_PER_FRAME: u32 = 150_000;
/// 持っておくチャンクの数（見えている所に近いものから）。
const KEPT_CHUNKS: usize = 24;
/// グラフの列の幅の上限（macOS 版の `GraphRenderer.Metrics.maxWidth`）。
const GRAPH_WIDTH: f32 = 120.0;
/// 文字の列の左右の余白。
const TEXT_INSET: f32 = 8.0;
/// 行ごとのグラフの右端と、件名などの間（macOS 版の `graphGap`）。
const GRAPH_GAP: f32 = 10.0;
/// 件名と作者・日時の間（macOS 版の `gap`）。
const ROW_GAP: f32 = 2.0;

/// 裏のスレッドから受け取るチャンク。文字列置き場は自分のバッファ（`Blob`）を持っているので、
/// どのスレッドに渡してもよい。
struct Chunk(HistoryChunk);

// SAFETY: `HistoryChunk` のポインタは、自分が持つバッファの先頭を指すだけ。
unsafe impl Send for Chunk {}

impl Chunk {
    fn text(&self, s: amonhen_layout::StrRef) -> &str {
        // UTF-8 の FE には、不正なバイト列を置き換えた正しい UTF-8 しか渡されない。
        std::str::from_utf8(self.0.bytes(s)).unwrap_or("")
    }

    fn index(&self, row: u32) -> usize {
        (row - self.0.start_row) as usize
    }
}

/// 並べ終えた行の文字。
struct RowText {
    subject: pango::Layout,
    meta: pango::Layout,
    /// 署名がある（検証の結果の印を出す）。
    signed: bool,
}

#[derive(Clone)]
struct Label {
    name: String,
    kind: RefKind,
    head: bool,
    /// worktree の HEAD の札（緑。`head` なら書き込む先）。
    worktree: bool,
    /// ほかの worktree が checkout しているブランチ（worktree の札と同じ緑）。
    elsewhere: bool,
}

struct Fonts {
    subject: pango::FontDescription,
    meta: pango::FontDescription,
    label: pango::FontDescription,
    /// 件名・作者と日時・ラベルの字の上下（ベースラインから。px）。
    subject_line: Line,
    meta_line: Line,
    label_line: Line,
}

/// 字の高さ（ベースラインからの上と下）。行の高さと字を置く位置は、字体の行送り（日本語の字体や Ubuntu は
/// 字の大きさの 1.4 倍を超える）ではなく字の大きさから、macOS 版のシステムフォントの上下の割合で決める。
#[derive(Clone, Copy)]
struct Line {
    ascent: f32,
    descent: f32,
}

impl Line {
    fn height(self) -> f32 {
        self.ascent + self.descent
    }
}

/// History の行の見せ方（View の History の表示）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Display {
    /// 1 行で出す（件名の右に作者・日時）。
    pub one_line: bool,
    /// 行に出す項目（`settings::items` のビット）。
    pub items: u32,
    /// 選んだコミットの祖先を強調する。
    pub highlight_ancestors: bool,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            one_line: false,
            items: items::ALL,
            highlight_ancestors: true,
        }
    }
}

impl Display {
    /// 実際に 1 行で出すか: 1 行にしたか、作者・日時の行に出す項目が 1 つもないとき（macOS 版の
    /// `oneLineHistoryRows`）。
    fn compact(&self) -> bool {
        self.one_line || self.items & items::META == 0
    }
}

/// 署名の検証の結果の色（macOS 版の `SignatureStatus` の色）。状態は詳細の Signature の行と同じ
/// `signature::Status` で決める（行と詳細で食い違わないように）。`None` はまだ検証していない。
fn signature_color(status: Option<Status>, fg: &gdk::RGBA) -> gdk::RGBA {
    match status {
        Some(Status::Verified(_)) => gdk::RGBA::new(0.20, 0.70, 0.30, 1.0),
        Some(Status::Untrusted | Status::SignerMismatch) => gdk::RGBA::new(0.90, 0.75, 0.10, 1.0),
        Some(Status::Expired) => gdk::RGBA::new(0.95, 0.55, 0.15, 1.0),
        Some(Status::Revoked | Status::Bad) => gdk::RGBA::new(0.90, 0.25, 0.25, 1.0),
        Some(Status::NoKey | Status::Unsupported) => {
            gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.55)
        }
        Some(Status::Pending) | None => gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.25),
    }
}

/// `verifySignatures` の答えの 1 件から状態を決める（コアがコミッターのメールを添える。macOS 版と同じ）。
fn signature_status(check: &serde_json::Value) -> Status {
    let signer = check["signer"].as_str().unwrap_or_default();
    let key = check["key"].as_str().unwrap_or_default();
    let kind = if !signer.contains('<') && key.starts_with("SHA256:") {
        "ssh"
    } else {
        "gpg"
    };
    let committer = check["committerEmail"].as_str().unwrap_or_default();
    Status::new(Some(check), committer, Some(kind))
}

#[derive(Default)]
pub struct State {
    repo: Option<Arc<Repository>>,
    history: Option<Arc<HistorySnapshot>>,
    chunks: HashMap<u32, Rc<Chunk>>,
    loading: HashSet<u32>,
    rows: HashMap<u32, RowText>,
    /// 行に出す札（worktree の HEAD の札、ref の札の順）。
    labels: HashMap<Oid, Vec<Label>>,
    ref_labels: HashMap<Oid, Vec<Label>>,
    worktree_labels: HashMap<Oid, Vec<Label>>,
    /// 開いたリポジトリの HEAD と、書き込む先の worktree の HEAD（worktree が 2 つ以上のとき）。グラフでは後者を
    /// 優先して輪で描く（macOS 版の `head`）。
    ref_head: Option<Oid>,
    editing_head: Option<Oid>,
    /// ほかの worktree（メインのものを除く）が checkout しているブランチ。
    elsewhere: HashSet<String>,
    label_layouts: HashMap<String, pango::Layout>,
    /// 見えている行の、名前を全部は出していない札（中央を切った札・「+N」）の横の範囲。押すと名前の一覧を出す。
    clipped: HashMap<u32, Vec<(f32, f32)>>,
    /// 見えている行の、たたんだ merge の「+N」の札の横の範囲。押すと開く。
    fold_badges: HashMap<u32, (f32, f32)>,
    /// merge を開く・たたむ（コミット、開くなら Some(true)・たたむなら Some(false)・入れ替えるなら None）。
    /// したら true。
    on_fold: Option<FoldCallback>,
    fonts: Option<Fonts>,
    /// 選んでいる行（複数選んでいるときは、最後に選んだ行）。
    selected: Option<u32>,
    /// 複数選んでいるときの、`selected` 以外の行。
    extra: BTreeSet<u32>,
    /// Shift で範囲を選ぶときの起点。
    anchor: Option<u32>,
    /// 行を選んだときに呼ぶ。
    on_select: Option<SelectCallback>,
    /// 行を右クリックしたときに呼ぶ（行、x、y）。
    on_menu: Option<MenuCallback>,
    /// メニューを出している行（選びはせず、枠で示す。macOS 版の `clickedRow`）。
    menu_row: Option<u32>,
    /// 次に出す History で選ぶコミット（[`HistoryView::select_commit`]）。
    wanted: Option<Oid>,
    /// 検索に一致した行（昇順）。薄く塗る。
    matches: Rc<Vec<u32>>,
    lanes: Lanes,
    metrics: Metrics,
    display: Display,
    /// 選んだコミットの祖先（History の世代、行、結果）。
    /// 強調する祖先（History の世代、選んだ行、描く行まで求めたもの）。gitk と同じく、描く行の分だけ求め、
    /// スクロールしたら続きから伸ばす（macOS 版の `Lineage`）。
    lineage: Option<(u64, u32, LineageWindow)>,
    /// 祖先を描く行まで求め終わっていない（求め終わるまで次のフレームでも描く）。
    lineage_pending: bool,
    /// 署名の検証の結果（コミットごと）と、検証している途中のもの。
    signatures: HashMap<Oid, Status>,
    verifying: HashSet<Oid>,
    /// 描いたときに見つけた、まだ検証していない署名（少し待ってから検証する）。
    unverified: Vec<Oid>,
    verify_soon: Option<glib::SourceId>,
    /// 検証する側を変えた回数。変える前に始めた検証の結果は捨てる（macOS 版の `signatureGeneration`）。
    signature_generation: u64,
    /// GitHub の CI の状態（コミットごと）。窓の側が gh で読んで渡す。
    ci: HashMap<Oid, Ci>,
    /// 行の右端の CI の印を押したときに呼ぶ。
    on_ci: Option<CiCallback>,
    tick: Option<gtk::TickCallbackId>,
    /// 大きさが決まる前に見えるようにと頼まれた行と、真ん中に置くか（決まったらスクロールする）。
    pending_scroll: Option<(u32, bool)>,
    /// 最後に描いたとき、見えている行の文字がすべてそろっていた。
    complete: bool,
    /// `--measure-scroll` の記録（1 回の `snapshot` にかかった時間）。
    snapshot_times: Option<Vec<f64>>,
    /// コミットを運んで落としたときに呼ぶ（運んだコミット、落とした所）。
    on_drop: Option<DropCallback>,
    /// 運んでいる間の、落とす所（行の間ならその行の上に線、重ねるならその行を枠で囲む）。
    drop_row: Option<Dropped>,
    /// 運んでいる間に、ポインタが窓の外にいる（そこで落とすと drop にする）。
    drag_outside: bool,
    /// 窓に、運んでいる間に窓の外へ出たかを見る受け手を付けた。
    watching_window: bool,
    /// 運ぶのが取り消されたあとに Esc を離した（Esc でやめた。窓の外でも drop にしない）。
    drag_escaped: bool,
    /// この表から運んでいるもの（運び、運んでいるコミット）。ほかから運んできたものは落とさせない。
    dragging: Option<(gdk::Drag, Oid)>,
    /// 運んでいる間だけ覚えておく、編集できるコミット（HEAD から最初の merge まで）。読み終えるまでは None で、
    /// どこにでも落とせる（macOS 版の `dragChain`）。
    drag_chain: Option<HashSet<Oid>>,
    /// 運んでいる行（表の行）。その行は空けて描く（点も文字も描かない。macOS 版の `draggingRow`）。
    dragging_row: Option<u32>,
    /// 運ぶ絵を描いている（選んだ色にしない）。
    drawing_drag_image: bool,
    /// 運び始めたときに呼ぶ（編集できるコミットを読んで [`HistoryView::set_drag_chain`] で渡す）。
    on_drag_start: Option<Rc<dyn Fn()>>,
    /// 出す History が替わったときに呼ぶ（密度の帯が求め直す）。
    on_history: Option<Rc<dyn Fn()>>,
    /// 最後に描いたグラフの列の幅と、それが変わったときに呼ぶもの（列の見出しが合わせる）。
    graph_width: f32,
    on_graph_width: Option<Rc<dyn Fn()>>,
    /// 最後に描いたもの（読み直したとき、ここから新しい表示へ溶かす）。
    last_node: Option<gsk::RenderNode>,
    /// 溶かしている途中の前の表示と、始めた時刻。
    fade: Option<(gsk::RenderNode, Instant)>,
    /// テーマの選択の色（背景と、その上の文字）を読むための、見えない子（macOS 版は表の選択でアクセントカラー）。
    accent: Option<(gtk::Label, gtk::Label)>,
    /// 次に出す History は溶かして入れ替える（ほかのプログラムの変化で読み直した。[`HistoryView::fade_next`]）。
    fade_requested: Option<Instant>,
    /// History を入れ替えた直後、見えている行の文字を読み終えるまで出しておく前の表示と、入れ替えた時刻・
    /// 読み終えたら溶かすか（入れ替えの間に文字が空の行が出ないように。macOS 版の `present`）。
    held: Option<(gsk::RenderNode, Instant, bool)>,
    /// shallow clone の境界のコミット（親を取得していない）の行。グラフの点から下へ点線を描く。
    boundary: Rc<HashSet<u32>>,
    /// shallow clone の History の最後の行の下に置く部品（取得していない古い履歴を取り足す帯）。
    footer: Option<gtk::Widget>,
    /// 一致したコミットだけを出している（その終わりは History の終わりではないので、帯を出さない）。
    matching_only: bool,
    /// History の上で編集している範囲（なければ編集していない）。
    edit: Option<Edit>,
    /// 編集の間に表で起きたことを知らせる先。
    on_edit: Option<Rc<dyn Fn(EditEvent)>>,
    /// テーマの一覧の背景の色を読む見えない子（編集の範囲の外を薄くする）。
    base_probe: Option<gtk::Label>,
}

/// merge を開く・たたむ（コミット、開くか）。したら true。
type FoldCallback = Rc<dyn Fn(Oid, Option<bool>) -> bool>;

/// 最後の行の下に置く帯の高さ（macOS 版の `ShallowHistoryBand.height`）。
pub const FOOTER_HEIGHT: f32 = 44.0;

/// 前の表示を出しておく長さの上限（読むのが遅くても、これを過ぎたら新しい表示を出す）。
const HOLD: std::time::Duration = std::time::Duration::from_millis(300);

/// 読み直したときに、前の表示を新しい表示へ溶かす時間（macOS 版の 0.15 秒）。
const FADE: std::time::Duration = std::time::Duration::from_millis(150);

/// 選択の色を読む見えない子の色（テーマの選択の背景と文字。GTK の既定のテーマも libadwaita も、アクセント
/// カラーに合わせる）。
fn install_accent_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".history-accent-bg { color: @theme_selected_bg_color; } \
         .history-accent-fg { color: @theme_selected_fg_color; } \
         .history-base { color: @theme_base_color; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// コミットを落としたときに呼ぶもの。
type DropCallback = Rc<dyn Fn(Oid, Dropped)>;

/// 取り消されたドラッグが Esc によるものかを見分けるために待つ時間（Esc でやめると、取り消しのすぐあとに Esc を
/// 離した知らせが届く。落としたときは届かない）。
const DRAG_ESCAPE_WAIT: std::time::Duration = std::time::Duration::from_millis(300);

/// コミットを落とした所。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// 行の間（その下の行。最後の行より下なら行の数）: 並べ替える。
    Above(u32),
    /// 行の上に重ねた: その行のコミットにまとめる（squash）。
    Onto(u32),
    /// 窓の外: 取り除く（drop）。
    Outside,
}

/// History の上で編集している範囲（`historyedit` が決め、表が描く。macOS 版の `HistoryViewController` の
/// `editor`）。範囲は HEAD から first-parent で辿ったコミット（新しい順）。
pub struct EditMarks {
    /// 範囲の行（新しい順。並べ替えた後の並び）。
    pub rows: Vec<EditRow>,
    /// HEAD から first-parent で辿った並び（境目を引く先）。
    pub line: Rc<Vec<Oid>>,
    /// 境目を上へ引いて狭められる、いちばん少ない数（外れる行がそのままのときだけ狭められる）。
    pub min_count: usize,
    /// 並べ替えられる（merge を含まない）。
    pub reorderable: bool,
}

/// 範囲の 1 行。
#[derive(Clone)]
pub struct EditRow {
    pub oid: Oid,
    pub action: Action,
    /// merge（作り直すだけで、操作の印を出さない）。
    pub merge: bool,
    /// 直したメッセージの件名（直していなければ None）。
    pub subject: Option<String>,
    /// コンフリクトしそう（印を橙に）。
    pub conflict: bool,
}

/// History の上で編集しているときに、表で起きたこと（`historyedit` が受ける）。
pub enum EditEvent {
    /// 範囲の `0` 番目の行の操作の印を押した（することの一覧を (x, y) に出す）。
    Choices(usize, f64, f64),
    /// 範囲の行ですることをキーで替えた。
    Choose(usize, Action),
    /// 範囲の行をダブルクリックした（メッセージを直す）。
    Reword(usize),
    /// 範囲の `0` 番目の行を、`1` 番目の行の上へ運んだ（範囲の数なら最後の行の下）。
    Move(usize, usize),
    /// 範囲の `0` 番目の行を、`1` 番目の行に重ねた（そのすぐ上へ動かしてまとめる）。
    Squash(usize, usize),
    /// 境目を引いて、範囲を HEAD から `0` 個にした。
    Resize(usize),
}

/// 表の側で持つ、編集の範囲の描き方。
struct Edit {
    marks: EditMarks,
    /// 表の行 → 範囲の何番目（新しい順）。
    slots: HashMap<u32, usize>,
    /// 表の行 → そこに描くコミットの行（並べ替えた分）。
    source: HashMap<u32, u32>,
    /// first-parent の並びのうち表にあるもの（表の行、並びの何番目。行の順）。境目を引く先。
    line_rows: Vec<(u32, usize)>,
    /// `line_rows` を求めた History の世代と並び（変わらなければ求め直さない）。
    line_key: Option<(u64, *const Vec<Oid>)>,
    /// 見えている行の、操作の印の横の範囲。押すとすることの一覧を出す。
    chips: HashMap<u32, (f32, f32)>,
    /// 境目を引いている間の、範囲の数。
    preview: Option<usize>,
    /// 境目を引いている。
    dragging_boundary: bool,
}

/// 範囲のコミットの表の行（範囲の順。表になければ None）から、表の行 → 範囲の何番目と、表の行 → そこに描く
/// コミットの行を作る。範囲の行は表の上から順に並ぶ（並べ替えても行の場所は変わらず、描くコミットが替わる）。
fn edit_map(rows_of: &[Option<u32>]) -> (HashMap<u32, usize>, HashMap<u32, u32>) {
    let mut slots: Vec<u32> = rows_of.iter().flatten().copied().collect();
    slots.sort_unstable();
    let mut index = HashMap::new();
    let mut source = HashMap::new();
    for (i, slot) in slots.into_iter().enumerate() {
        index.insert(slot, i);
        if let Some(Some(row)) = rows_of.get(i) {
            source.insert(slot, *row);
        }
    }
    (index, source)
}

/// 境目の下端 `bottom(行)` が `y` にいちばん近い、並びの行（`line_rows` の 1 つ）の並びの何番目。
fn nearest_line_row(line_rows: &[(u32, usize)], y: f32, row_height: f32) -> Option<usize> {
    if line_rows.is_empty() || row_height <= 0.0 {
        return None;
    }
    let bottom = |row: u32| (row + 1) as f32 * row_height;
    let at = line_rows.partition_point(|&(row, _)| bottom(row) < y);
    [at.checked_sub(1), Some(at)]
        .into_iter()
        .flatten()
        .filter_map(|i| line_rows.get(i))
        .min_by(|a, b| (bottom(a.0) - y).abs().total_cmp(&(bottom(b.0) - y).abs()))
        .map(|&(_, k)| k)
}

/// History の行を運ぶときの中身の頭（サイドバーのブランチ・タグの `local:`・`other:` と分ける）。
pub const DRAG_PREFIX: &str = "commit:";

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::HistoryView)]
    pub struct HistoryView {
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

    impl Default for HistoryView {
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
    impl ObjectSubclass for HistoryView {
        const NAME: &'static str = "AmonhenHistoryView";
        type Type = super::HistoryView;
        type ParentType = gtk::Widget;
        type Interfaces = (gtk::Scrollable,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for HistoryView {
        fn dispose(&self) {
            if let Some((bg, fg)) = self.state.borrow_mut().accent.take() {
                bg.unparent();
                fg.unparent();
            }
            if let Some(footer) = self.state.borrow_mut().footer.take() {
                footer.unparent();
            }
            if let Some(base) = self.state.borrow_mut().base_probe.take() {
                base.unparent();
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            let view = self.obj();
            view.set_focusable(true);
            view.set_overflow(gtk::Overflow::Hidden);
            // テーマの一覧の背景（暗いテーマでも文字と合う）。
            view.add_css_class("view");
            // 選択の色はテーマ（アクセントカラー）から読む。
            install_accent_css();
            let probes = (gtk::Label::new(None), gtk::Label::new(None));
            probes.0.add_css_class("history-accent-bg");
            probes.1.add_css_class("history-accent-fg");
            for p in [&probes.0, &probes.1] {
                p.set_visible(false);
                p.set_parent(&*view);
            }
            self.state.borrow_mut().accent = Some(probes);
            let base = gtk::Label::new(None);
            base.add_css_class("history-base");
            base.set_visible(false);
            base.set_parent(&*view);
            self.state.borrow_mut().base_probe = Some(base);

            // いくつか選んでいる行を押しただけでは、選びを変えない。離したときに 1 行にする（ドラッグならそのまま。
            // macOS 版の表と同じ）。
            let on_release: Rc<Cell<Option<u32>>> = Rc::default();
            let click = gtk::GestureClick::new();
            let weak = view.downgrade();
            let later = on_release.clone();
            click.connect_pressed(move |gesture, n, x, y| {
                if let Some(view) = weak.upgrade() {
                    view.grab_focus();
                    // History の上で編集しているときの、操作の印・境目・ダブルクリック。
                    if view.edit_press(x as f32, y as f32, n) {
                        return;
                    }
                    // 行の右端の CI の印なら、行を選ばずに詳細の吹き出し。
                    if view.click_ci(x as f32, y as f32) {
                        return;
                    }
                    // たたんだ merge の「+N」なら、行を選ばずに開く。
                    if view.click_fold_badge(x as f32, y as f32) {
                        return;
                    }
                    // 名前を全部は出していない札なら、行を選ばずに名前の一覧の吹き出し。
                    if view.click_labels(x as f32, y as f32) {
                        return;
                    }
                    if let Some(row) = view.row_at(y as f32) {
                        // Ctrl で 1 行ずつ足す・外す、Shift で範囲（macOS の ⌘・⇧ と同じ）。
                        let state = gesture.current_event_state();
                        let (shift, ctrl) = (
                            state.contains(gdk::ModifierType::SHIFT_MASK),
                            state.contains(gdk::ModifierType::CONTROL_MASK),
                        );
                        if !shift && !ctrl && view.in_multiple_selection(row) {
                            later.set(Some(row));
                            return;
                        }
                        view.select_with(row, shift, ctrl);
                    } else if view.below_rows(y as f32) {
                        // 最後の行より下: 選択を外し、詳細も消す（macOS 版の表と同じ）。
                        view.clear_selection();
                    }
                }
            });
            // Edit の Select All（キーは下の `keys`）。
            let group = gio::SimpleActionGroup::new();
            let action = gio::SimpleAction::new("select-all", None);
            let weak = view.downgrade();
            action.connect_activate(move |_, _| {
                if let Some(view) = weak.upgrade() {
                    view.select_all();
                }
            });
            group.add_action(&action);
            view.insert_action_group("selection", Some(&group));
            let (weak, later) = (view.downgrade(), on_release.clone());
            click.connect_released(move |_, _, _, _| {
                if let (Some(view), Some(row)) = (weak.upgrade(), later.take()) {
                    view.select_with(row, false, false);
                }
            });
            // ドラッグになったら離しても選び直さない。
            click.connect_stopped(move |_| on_release.set(None));
            view.add_controller(click);

            // 編集の範囲の境目を、押したまま引いた先の first-parent のコミットまで動かす（離したら範囲を変える。
            // macOS 版の `dragBoundary`）。
            let boundary = gtk::GestureDrag::new();
            let weak = view.downgrade();
            boundary.connect_drag_update(move |gesture, _, dy| {
                if let Some(view) = weak.upgrade()
                    && let Some((_, y)) = gesture.start_point()
                {
                    view.drag_boundary_to((y + dy) as f32);
                }
            });
            let weak = view.downgrade();
            boundary.connect_drag_end(move |_, _, _| {
                if let Some(view) = weak.upgrade() {
                    view.end_boundary_drag();
                }
            });
            view.add_controller(boundary);

            // 右クリック: その行のコミットのメニュー。選びはせず（詳細はそのまま）、メニューの間は行を枠で示す
            // （macOS 版の `clickedRow`）。複数選んでいる行の上なら、まとめての操作のメニュー。
            let menu = gtk::GestureClick::new();
            menu.set_button(gdk::BUTTON_SECONDARY);
            let weak = view.downgrade();
            menu.connect_pressed(move |_, _, x, y| {
                let Some(view) = weak.upgrade() else { return };
                view.grab_focus();
                if let Some(row) = view.row_at(y as f32) {
                    let f = view.state().on_menu.clone();
                    if let Some(f) = f {
                        f(row, x, y);
                    }
                }
            });
            view.add_controller(menu);

            // コミットを運ぶ（History の中で落として並べ替える。macOS 版と同じ）。
            let drag = gtk::DragSource::new();
            drag.set_actions(gdk::DragAction::MOVE);
            let dragged: Rc<Cell<Option<Oid>>> = Rc::default();
            // つかんだ行と、押した所（運ぶ絵の中のどこを指で持つか）。
            let grabbed: Rc<Cell<Option<(u32, f64, f64)>>> = Rc::default();
            let (weak, prepared, grab) = (view.downgrade(), dragged.clone(), grabbed.clone());
            drag.connect_prepare(move |_, x, y| {
                let view = weak.upgrade()?;
                let row = view.row_at(y as f32)?;
                // 編集の間は、範囲の行だけを並べ替えられる（merge を含まないとき。境目を引いているときは運ばない）。
                if !view.edit_can_drag(row) {
                    return None;
                }
                let shown = view.shown_row(row);
                let (_, history) = view.current()?;
                let id = *history.graph.ids().get(shown as usize)?;
                view.state().drag_chain = None;
                view.state().dragging = None;
                prepared.set(Some(id));
                grab.set(Some((row, x, y)));
                let value = format!("{DRAG_PREFIX}{id}");
                Some(gdk::ContentProvider::for_value(&value.to_value()))
            });
            let (weak, from) = (view.downgrade(), dragged);
            drag.connect_drag_begin(move |_, drag| {
                if let Some(view) = weak.upgrade()
                    && let Some(id) = from.get()
                {
                    // 運ぶ絵は、その行と同じ絵（グラフの点と線・札・件名・作者）。行の上にそのまま重ねて持ち上げる。
                    if let Some((row, x, y)) = grabbed.get()
                        && let Some((paintable, top)) = view.row_image(row)
                    {
                        gtk::DragIcon::set_from_paintable(
                            drag,
                            &paintable,
                            x as i32,
                            (y - top) as i32,
                        );
                    }
                    {
                        let mut st = view.state();
                        st.dragging = Some((drag.clone(), id));
                        st.dragging_row = grabbed.get().map(|(row, _, _)| row);
                        st.drag_outside = false;
                    }
                    view.watch_window_for_drags();
                    view.queue_draw();
                    // 編集の間は、落とせる所を表が決める（編集できるコミットを読まない）。
                    let editing = view.state().edit.is_some();
                    let f = view.state().on_drag_start.clone().filter(|_| !editing);
                    if let Some(f) = f {
                        f();
                    }
                }
            });
            // 窓の外で落とした（Esc で取り消したのではない）なら、そのコミットを drop にする（Apply まで変えない）。
            // Wayland では、落とした先（デスクトップ・ほかのアプリの窓）が受け取らないと、理由によらず取り消しに
            // なる。Esc でやめたときは、取り消しのすぐあとに Esc を離した知らせが届くので、少し待って見分ける
            // （タブを窓の外へ引き出すときと同じ）。
            let weak = view.downgrade();
            drag.connect_drag_cancel(move |_, _, _| {
                let Some(view) = weak.upgrade() else {
                    return false;
                };
                let id = {
                    let mut st = view.state();
                    st.drag_escaped = false;
                    st.dragging
                        .as_ref()
                        .map(|(_, id)| *id)
                        .filter(|_| st.drag_outside)
                };
                let Some(id) = id.filter(|_| view.can_drop_outside()) else {
                    return false;
                };
                let weak = view.downgrade();
                glib::timeout_add_local_once(DRAG_ESCAPE_WAIT, move || {
                    if let Some(view) = weak.upgrade()
                        && !view.state().drag_escaped
                    {
                        view.dropped(id, Dropped::Outside);
                    }
                });
                false
            });
            let weak = view.downgrade();
            drag.connect_drag_end(move |_, _, _| {
                if let Some(view) = weak.upgrade() {
                    {
                        let mut st = view.state();
                        st.dragging = None;
                        st.drag_chain = None;
                        st.dragging_row = None;
                        st.drag_outside = false;
                    }
                    view.queue_draw();
                }
            });
            view.add_controller(drag);
            let target = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
            // この表から運んだコミットだけを受ける（サイドバーのブランチなどは受けない。macOS 版と同じ）。
            let weak = view.downgrade();
            target.connect_accept(move |_, drop| {
                let Some(view) = weak.upgrade() else {
                    return false;
                };
                let ours = drop.drag();
                drop.formats().contains_type(glib::Type::STRING)
                    && ours.is_some()
                    && view.state().dragging.as_ref().map(|(d, _)| d) == ours.as_ref()
            });
            let weak = view.downgrade();
            target.connect_motion(move |_, _, y| {
                let Some(view) = weak.upgrade() else {
                    return gdk::DragAction::empty();
                };
                // 行の真ん中に重ねたら、そのコミットにまとめる（まとめられない行なら、行の間へ並べ替える）。
                if let Some(row) = view.onto_row_at(y as f32)
                    && view.can_squash_onto(row)
                {
                    if view.state().drop_row != Some(Dropped::Onto(row)) {
                        view.state().drop_row = Some(Dropped::Onto(row));
                        view.queue_draw();
                    }
                    return gdk::DragAction::MOVE;
                }
                let row = view.drop_row_at(y as f32);
                // 編集できる並びの外（最初の merge より下、HEAD でない枝）には落とせない（印も出さない）。
                if !view.can_drop_at(row) {
                    if view.state().drop_row.take().is_some() {
                        view.queue_draw();
                    }
                    return gdk::DragAction::empty();
                }
                if view.state().drop_row != Some(Dropped::Above(row)) {
                    view.state().drop_row = Some(Dropped::Above(row));
                    view.queue_draw();
                }
                gdk::DragAction::MOVE
            });
            let weak = view.downgrade();
            target.connect_leave(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.state().drop_row = None;
                    view.queue_draw();
                }
            });
            let weak = view.downgrade();
            target.connect_drop(move |_, value, _, y| {
                let Some(view) = weak.upgrade() else {
                    return false;
                };
                let at = view.state().drop_row.take();
                view.queue_draw();
                let Some(id) = value
                    .get::<String>()
                    .ok()
                    .and_then(|v| v.strip_prefix(DRAG_PREFIX).and_then(Oid::from_hex))
                else {
                    return false;
                };
                let at = at.unwrap_or(Dropped::Above(view.drop_row_at(y as f32)));
                view.dropped(id, at)
            });
            view.add_controller(target);

            let keys = gtk::EventControllerKey::new();
            let weak = view.downgrade();
            keys.connect_key_pressed(move |_, key, _, state| {
                let Some(view) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                // Ctrl+A ですべて選ぶ、Menu・Shift+F10 で選んだ行のメニュー（macOS 版の表と同じ）。
                let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
                let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
                // 編集の間、選んだ範囲の行で p・r・s・f・d・e を押すと、することを替える（macOS 版と同じ）。
                if !state.intersects(
                    gdk::ModifierType::CONTROL_MASK
                        | gdk::ModifierType::ALT_MASK
                        | gdk::ModifierType::SUPER_MASK
                        | gdk::ModifierType::META_MASK,
                ) && view.edit_key(key)
                {
                    return glib::Propagation::Stop;
                }
                if ctrl && !shift && matches!(key, gdk::Key::a | gdk::Key::A) {
                    view.select_all();
                    return glib::Propagation::Stop;
                }
                // 選んだ merge を ← でたたみ、→ で開く（ツリーの開閉と同じ。macOS 版と同じ）。
                if state.is_empty()
                    && matches!(key, gdk::Key::Left | gdk::Key::Right)
                    && view.fold_key(key == gdk::Key::Right)
                {
                    return glib::Propagation::Stop;
                }
                if key == gdk::Key::Menu || (shift && !ctrl && key == gdk::Key::F10) {
                    view.popup_menu();
                    return glib::Propagation::Stop;
                }
                if view.move_selection(key, state.contains(gdk::ModifierType::SHIFT_MASK)) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            view.add_controller(keys);
        }
    }

    impl WidgetImpl for HistoryView {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.obj().configure_adjustments();
            self.obj().place_footer();
            // 吹き出し（CI の印のもの）は子なので、自分で置く。
            let mut child = self.obj().first_child();
            while let Some(c) = child {
                if let Some(p) = c.downcast_ref::<gtk::Popover>() {
                    p.present();
                }
                child = c.next_sibling();
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let started = Instant::now();
            // 描いたものを覚えておき、読み直したときに前の表示から溶かす（macOS 版と同じ）。
            let own = gtk::Snapshot::new();
            self.obj().draw(&own);
            self.obj().draw_drop(&own);
            let node = own.to_node();
            let mut st = self.state.borrow_mut();
            // 入れ替えた History の見えている行を読み終えるまでは、前の表示を出しておく。
            if let Some((old, at, _)) = st.held.as_ref()
                && !st.complete
                && at.elapsed() < HOLD
            {
                snapshot.append_node(old);
                return;
            }
            let released = st.held.take();
            drop(st);
            if let Some((old, _, true)) = released {
                self.obj().start_fade_from(old);
            }
            let mut st = self.state.borrow_mut();
            let fade = st
                .fade
                .as_ref()
                .filter(|(_, at)| at.elapsed() < FADE)
                .map(|(old, at)| (old.clone(), at.elapsed().as_secs_f32() / FADE.as_secs_f32()));
            match (&node, fade) {
                (Some(node), Some((old, progress))) => {
                    snapshot.push_cross_fade(f64::from(progress.clamp(0.0, 1.0)));
                    snapshot.append_node(&old);
                    snapshot.pop();
                    snapshot.append_node(node);
                    snapshot.pop();
                }
                (Some(node), None) => snapshot.append_node(node),
                (None, _) => {}
            }
            st.last_node = node;
            if let Some(times) = st.snapshot_times.as_mut() {
                times.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            let footer = st.footer.clone();
            drop(st);
            if let Some(footer) = footer.filter(|f| f.is_child_visible()) {
                self.obj().snapshot_child(&footer, snapshot);
            }
        }
    }

    impl ScrollableImpl for HistoryView {}

    impl HistoryView {
        fn set_hadjustment(&self, adjustment: Option<gtk::Adjustment>) {
            self.hadjustment.replace(adjustment);
            self.obj().configure_adjustments();
        }

        fn set_vadjustment(&self, adjustment: Option<gtk::Adjustment>) {
            if let Some(adj) = &adjustment {
                let weak = self.obj().downgrade();
                adj.connect_value_changed(move |_| {
                    if let Some(view) = weak.upgrade() {
                        view.queue_draw();
                        // 最後の行の下の帯は、スクロールに合わせて置き直す。
                        if view.footer_shown() {
                            view.queue_allocate();
                        }
                    }
                });
            }
            self.vadjustment.replace(adjustment);
            self.obj().configure_adjustments();
        }
    }
}

glib::wrapper! {
    pub struct HistoryView(ObjectSubclass<imp::HistoryView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Default for HistoryView {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl HistoryView {
    fn state(&self) -> std::cell::RefMut<'_, State> {
        self.imp().state.borrow_mut()
    }

    pub fn row_count(&self) -> u32 {
        self.state()
            .history
            .as_ref()
            .map_or(0, |h| h.layout.nodes().len() as u32)
    }

    /// `start` なら 1 回の `snapshot` の時間を記録し始め、そうでなければ記録をやめてそれを返す。
    pub fn imp_snapshot_times(&self, start: bool) -> Vec<f64> {
        let mut st = self.state();
        if start {
            st.snapshot_times = Some(Vec::new());
            Vec::new()
        } else {
            st.snapshot_times.take().unwrap_or_default()
        }
    }

    /// 選択の背景と、その上の文字の色（テーマのアクセントカラー）。
    fn accent(&self) -> (gdk::RGBA, gdk::RGBA) {
        let probes = self.state().accent.clone();
        probes.map_or(
            (gdk::RGBA::new(0.21, 0.52, 0.89, 1.0), gdk::RGBA::WHITE),
            |(bg, fg)| (bg.color(), fg.color()),
        )
    }

    pub fn row_height(&self) -> f32 {
        let mut st = self.state();
        self.fonts(&mut st);
        st.metrics.row_height
    }

    /// 次に出す History を、前の表示から短く溶かして入れ替える（ほかのプログラムの変化で読み直したとき。
    /// 自分で範囲を変えたときは待たせない。macOS 版と同じ）。少し待っても出さなければ忘れる。
    pub fn fade_next(&self) {
        self.state().fade_requested = Some(Instant::now());
    }

    /// 前の表示を、新しい History の見えている行を読み終えるまで出しておく（前に描いたものがあれば）。読み終えたら、
    /// 頼まれていれば溶かして入れ替える。
    fn hold_previous(&self) {
        let mut st = self.state();
        let fade = st
            .fade_requested
            .take()
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(2));
        // 前の表示を出している途中なら、それを出したまま（入れ替えの途中の表示は覚えない）。
        let old = match st.held.take() {
            Some((old, _, faded)) => Some((old, faded || fade)),
            None => st.last_node.clone().map(|old| (old, fade)),
        };
        let Some((old, fade)) = old else {
            return;
        };
        st.held = Some((old, Instant::now(), fade));
        drop(st);
        // 読むのが遅くても、上限を過ぎたら新しい表示を出す。
        let weak = self.downgrade();
        glib::timeout_add_local_once(HOLD, move || {
            if let Some(view) = weak.upgrade() {
                view.queue_draw();
            }
        });
    }

    /// 前の表示 `old` から溶かし始める。
    fn start_fade_from(&self, old: gsk::RenderNode) {
        self.state().fade = Some((old, Instant::now()));
        self.add_tick_callback(|view, _| {
            view.queue_draw();
            let mut st = view.state();
            if st.fade.as_ref().is_some_and(|(_, at)| at.elapsed() < FADE) {
                return glib::ControlFlow::Continue;
            }
            st.fade = None;
            glib::ControlFlow::Break
        });
    }

    /// 表示する History を替える。同じ History の新しい世代なら、選んでいたコミットを選び直す。
    pub fn show(&self, repo: Arc<Repository>, history: Arc<HistorySnapshot>) {
        // 選んでいたコミットが消えたか（ほかのプログラムが rebase・amend した）、選び直しで変わったか。
        let changed = {
            let mut st = self.state();
            // 変わっていなければ何もしない（読み直すたびに同じスナップショットが来る）。
            if st
                .history
                .as_ref()
                .is_some_and(|h| Arc::ptr_eq(h, &history))
            {
                st.repo = Some(repo);
                return;
            }
            // 選んでいたコミットを、新しい History の行で選び直す。
            let id_of = |row: u32| {
                let old = st.history.as_ref()?;
                old.graph.ids().get(row as usize).copied()
            };
            let selected = st.selected.and_then(id_of);
            let extra: Vec<_> = st.extra.iter().filter_map(|&r| id_of(r)).collect();
            let anchor = st.anchor.and_then(id_of);
            st.repo = Some(repo);
            st.selected = selected.and_then(|id| history.row_of(&id));
            st.extra = extra.iter().filter_map(|id| history.row_of(id)).collect();
            st.anchor = anchor.and_then(|id| history.row_of(&id));
            let mut changed =
                st.selected.is_none() != selected.is_none() || st.extra.len() != extra.len();
            if let Some(row) = st.wanted.take().and_then(|id| history.row_of(&id)) {
                st.selected = Some(row);
                st.extra.clear();
                st.anchor = Some(row);
                changed = true;
            }
            // 最後に選んだコミットが消えたら（ほかのプログラムが rebase・amend した）、残りのうち最後のものに
            // する。何も残らなければ先頭（いちばん新しいコミット）を選ぶ（macOS 版と同じ。表示の位置は動かさない）。
            if selected.is_some() && st.selected.is_none() {
                st.selected = st.extra.pop_last();
            }
            // 何も選んでいなければ（開いたばかりも）先頭を選び、詳細が空のままにならないようにする（macOS 版と同じ）。
            if st.selected.is_none() && !history.graph.ids().is_empty() {
                st.selected = Some(0);
                st.anchor = Some(0);
                changed = true;
            }
            // shallow clone の境界のコミットの行（境界はたいてい数個なので、全行を見ても数 ms）。
            st.boundary = Rc::new(history.boundary_rows().into_iter().collect());
            // 行の番号が変わるので、メニューの行の枠は外す。
            st.menu_row = None;
            st.history = Some(history);
            st.chunks.clear();
            st.loading.clear();
            st.rows.clear();
            st.complete = false;
            // 行の番号が変わるので、検索の一致は捨てる（検索の欄が探し直す）。
            st.matches = Rc::default();
            changed
        };
        self.hold_previous();
        self.configure_adjustments();
        // 編集の範囲の行を、新しい History の行で求め直す。
        self.rebuild_edit();
        self.update_lineage();
        self.queue_draw();
        self.queue_allocate();
        let f = self.state().on_history.clone();
        if let Some(f) = f {
            f();
        }
        // 選んだコミットが変わったときだけ知らせる（詳細が消えたコミットのまま残らないように）。
        if changed {
            self.notify_selection();
        }
    }

    /// 行の文字を読み直す（mailmap が変わって作者名が変わるときなど。History はそのまま）。
    pub fn reload_rows(&self) {
        {
            let mut st = self.state();
            st.chunks.clear();
            st.loading.clear();
            st.rows.clear();
            st.complete = false;
        }
        self.hold_previous();
        self.queue_draw();
    }

    /// 行の見せ方を替える（1 行・項目・祖先の強調）。
    pub fn set_display(&self, display: Display) {
        // 祖先の強調だけなら、行の高さは変わらないので動かさない（macOS 版と同じ）。
        let recenter = {
            let mut st = self.state();
            if st.display == display {
                return;
            }
            let recenter = Display {
                highlight_ancestors: st.display.highlight_ancestors,
                ..display
            } != st.display;
            st.display = display;
            st.rows.clear();
            st.fonts = None;
            recenter
        };
        self.configure_adjustments();
        // 行の高さが変わるので、選んでいる行を表の中ほどに置き直す（macOS 版の `rowStyleChanged`）。
        if recenter && let Some(row) = self.selected() {
            self.center_row(row);
        }
        self.update_lineage();
        self.queue_draw();
    }

    /// 選んだコミットの祖先を強調する。求めるのは描くときに、描く行の分だけ（選んだ入力と同じフレームに出る）。
    fn update_lineage(&self) {
        let mut st = self.state();
        let target = st
            .history
            .clone()
            .zip(st.selected)
            .filter(|_| st.display.highlight_ancestors);
        let Some((history, row)) = target else {
            if st.lineage.take().is_some() {
                drop(st);
                self.queue_draw();
            }
            return;
        };
        if st
            .lineage
            .as_ref()
            .is_some_and(|(g, r, _)| *g == history.generation && *r == row)
        {
            return;
        }
        st.lineage = Some((
            history.generation,
            row,
            LineageWindow::new(&history.graph, row),
        ));
        drop(st);
        self.queue_draw();
    }

    /// 見えている署名を、スクロールが落ち着いてから 8 件ずつ裏で検証し、終わった分から色を付ける
    /// （gpg を 1 件ずつ起動するので 1 件 15 ms ほどかかる）。
    fn verify_soon(&self) {
        let mut st = self.state();
        if st.unverified.is_empty() {
            return;
        }
        if let Some(id) = st.verify_soon.take() {
            id.remove();
        }
        let weak = self.downgrade();
        st.verify_soon = Some(glib::timeout_add_local_once(
            std::time::Duration::from_millis(150),
            move || {
                let Some(view) = weak.upgrade() else { return };
                let (repo, pending, generation) = {
                    let mut st = view.state();
                    st.verify_soon = None;
                    let ids = std::mem::take(&mut st.unverified);
                    let mut pending: Vec<Oid> = Vec::new();
                    for id in ids {
                        if !st.signatures.contains_key(&id) && st.verifying.insert(id) {
                            pending.push(id);
                        }
                    }
                    (st.repo.clone(), pending, st.signature_generation)
                };
                let Some(repo) = repo else { return };
                glib::spawn_future_local(async move {
                    for batch in pending.chunks(8) {
                        let (r, ids) = (repo.clone(), batch.to_vec());
                        let checks = gio::spawn_blocking(move || {
                            let oids: Vec<String> = ids.iter().map(Oid::to_string).collect();
                            let command =
                                serde_json::json!({ "op": "verifySignatures", "oids": oids });
                            let reply: serde_json::Value =
                                serde_json::from_str(&r.run_json(&command.to_string()))
                                    .unwrap_or_default();
                            reply["info"]["checks"]
                                .as_array()
                                .map(|c| c.iter().map(signature_status).collect::<Vec<_>>())
                                .unwrap_or_default()
                        })
                        .await
                        .unwrap_or_default();
                        let Some(view) = weak.upgrade() else { return };
                        {
                            let mut st = view.state();
                            if st.signature_generation != generation {
                                return;
                            }
                            for (id, status) in batch.iter().zip(checks) {
                                st.signatures.insert(*id, status);
                            }
                            for id in batch {
                                st.verifying.remove(id);
                            }
                        }
                        view.queue_draw();
                    }
                });
            },
        ));
    }

    /// 署名を検証する側が変わった: 覚えていた結果を捨て、見えている行を検証し直す（macOS 版の
    /// `forgetSignatures`）。
    pub fn forget_signatures(&self) {
        {
            let mut st = self.state();
            st.signature_generation += 1;
            if let Some(id) = st.verify_soon.take() {
                id.remove();
            }
            st.signatures.clear();
            st.verifying.clear();
            st.unverified.clear();
        }
        self.queue_draw();
    }

    /// ブランチ・タグの名前を、指すコミットの行に出す。
    pub fn show_refs(&self, refs: &RefsSnapshot) {
        let mut labels: HashMap<Oid, Vec<Label>> = HashMap::new();
        for e in &refs.refs.entries {
            let Some(target) = e.target else { continue };
            if !matches!(e.kind, RefKind::Local | RefKind::Remote | RefKind::Tag) {
                continue;
            }
            labels.entry(target).or_default().push(Label {
                name: e.name.clone(),
                kind: e.kind,
                head: e.kind == RefKind::Local
                    && Some(e.name.as_str()) == refs.refs.head_branch.as_deref(),
                worktree: false,
                elsewhere: false,
            });
        }
        // 今のブランチ、ローカル、リモート、タグの順。
        for list in labels.values_mut() {
            list.sort_by_key(|l| (!l.head, l.kind, l.name.clone()));
        }
        let mut st = self.state();
        st.ref_labels = labels;
        st.ref_head = refs.refs.head;
        Self::merge_labels(&mut st);
        drop(st);
        self.queue_draw();
    }

    /// `id` を指すブランチ・タグの名前と種類（行の札と同じ順。worktree の札は除く）。
    pub fn ref_names(&self, id: &Oid) -> Vec<(String, RefKind)> {
        self.state()
            .ref_labels
            .get(id)
            .map(|list| list.iter().map(|l| (l.name.clone(), l.kind)).collect())
            .unwrap_or_default()
    }

    /// 各 worktree の HEAD の札（コミット、札の文字、書き込む先か）と、ほかの worktree が checkout している
    /// ブランチ（緑の札にする）。worktree が 1 つなら空で呼ぶ。
    pub fn set_worktrees(&self, heads: Vec<(Oid, String, bool)>, elsewhere: HashSet<String>) {
        let editing_head = heads
            .iter()
            .find(|(_, _, editing)| *editing)
            .map(|(id, _, _)| *id);
        let mut labels: HashMap<Oid, Vec<Label>> = HashMap::new();
        for (head, name, editing) in heads {
            labels.entry(head).or_default().push(Label {
                name,
                kind: RefKind::Local,
                head: editing,
                worktree: true,
                elsewhere: false,
            });
        }
        let mut st = self.state();
        st.worktree_labels = labels;
        st.editing_head = editing_head;
        st.elsewhere = elsewhere;
        Self::merge_labels(&mut st);
        drop(st);
        self.queue_draw();
    }

    /// worktree の札を先に、ref の札を後に並べる（ほかの worktree のブランチに印を付ける）。
    fn merge_labels(st: &mut State) {
        let mut merged = st.worktree_labels.clone();
        for (id, list) in &st.ref_labels {
            let entry = merged.entry(*id).or_default();
            entry.extend(list.iter().map(|l| Label {
                elsewhere: l.kind == RefKind::Local && st.elsewhere.contains(&l.name),
                ..l.clone()
            }));
        }
        st.labels = merged;
    }

    /// 見えている行の件名などがすべて読めている（`--snapshot` で撮るのを待つ）。
    pub fn is_settled(&self) -> bool {
        let st = self.state();
        st.complete
            && st.loading.is_empty()
            && !st.lanes.animating
            && st.verifying.is_empty()
            && st.unverified.is_empty()
    }

    pub fn selected(&self) -> Option<u32> {
        self.state().selected
    }

    /// 選んでいる行（昇順）。
    pub fn selection(&self) -> Vec<u32> {
        let st = self.state();
        let mut rows: Vec<u32> = st.extra.iter().copied().chain(st.selected).collect();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// `row` だけを選び、表の中ほどに見えるようにする（検索・Go to HEAD・リンクなど、プログラムから選ぶとき。
    /// macOS 版の `select(row:)` と同じ）。
    pub fn select(&self, row: u32) {
        self.select_row(row, true);
    }

    /// コミット `id` を選ぶ。今の History になければ、次に出す History で選ぶ（絞り込みを作り直している間）。
    pub fn select_commit(&self, id: Oid) {
        let row = self.current().and_then(|(_, history)| history.row_of(&id));
        match row {
            Some(row) => self.select(row),
            None => self.state().wanted = Some(id),
        }
    }

    /// `row` だけを選び、見えるところまでだけスクロールする（クリック・矢印のキー）。
    fn select_visible(&self, row: u32) {
        self.select_row(row, false);
    }

    fn select_row(&self, row: u32, center: bool) {
        let changed = {
            let mut st = self.state();
            let changed = st.selected != Some(row) || !st.extra.is_empty();
            st.selected = Some(row);
            st.extra.clear();
            st.anchor = Some(row);
            changed
        };
        if center {
            self.center_row(row);
        } else {
            self.scroll_to_row(row);
        }
        self.queue_draw();
        if changed {
            self.notify_selection();
        }
    }

    /// クリックで選ぶ。`shift` なら起点からの範囲、`ctrl` なら 1 行ずつ足す・外す。
    /// `row` が、いくつか選んでいるうちの 1 つか。
    fn in_multiple_selection(&self, row: u32) -> bool {
        let st = self.state();
        !st.extra.is_empty() && (st.selected == Some(row) || st.extra.contains(&row))
    }

    pub fn select_with(&self, row: u32, shift: bool, ctrl: bool) {
        {
            let mut st = self.state();
            let anchor = st.anchor.or(st.selected);
            match (shift, ctrl, anchor) {
                (true, _, Some(anchor)) => {
                    let (a, b) = (anchor.min(row), anchor.max(row));
                    if !ctrl {
                        st.extra.clear();
                    }
                    if ctrl && let Some(old) = st.selected {
                        st.extra.insert(old);
                    }
                    st.extra.extend((a..=b).filter(|&r| r != row));
                    st.selected = Some(row);
                }
                (false, true, _) => {
                    let mut all: BTreeSet<u32> =
                        st.extra.iter().copied().chain(st.selected).collect();
                    if !all.remove(&row) {
                        all.insert(row);
                    }
                    // 外した行が最後に選んだ行なら、残りのうち最後のものにする。
                    st.selected = if all.contains(&row) {
                        Some(row)
                    } else {
                        all.iter().next_back().copied()
                    };
                    if let Some(s) = st.selected {
                        all.remove(&s);
                    }
                    st.extra = all;
                    st.anchor = Some(row);
                }
                _ => {
                    drop(st);
                    self.select_visible(row);
                    return;
                }
            }
        }
        self.queue_draw();
        self.notify_selection();
    }

    /// 裏のタブになった: 見えている所のほかのチャンクと並べた行を捨てる（前に戻ったら読み直す）。捨てた数を返す。
    /// macOS 版の `shrinkForBackground`。
    pub fn shrink_for_background(&self) -> usize {
        let rh = self.row_height().max(1.0);
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let height = self.height() as f32;
        let first = (scroll / rh) as u32 / CHUNK_ROWS;
        let last = (((scroll + height.max(rh)) / rh) as u32 / CHUNK_ROWS).max(first);
        let mut st = self.state();
        let before = st.chunks.len();
        st.chunks.retain(|i, _| (first..=last).contains(i));
        st.rows
            .retain(|r, _| (first..=last).contains(&(r / CHUNK_ROWS)));
        before - st.chunks.len()
    }

    /// すべての行を選ぶ（Ctrl+A）。
    fn select_all(&self) {
        let count = self.row_count();
        if count == 0 {
            return;
        }
        {
            let mut st = self.state();
            let last = st.selected.unwrap_or(0);
            st.extra = (0..count).filter(|&r| r != last).collect();
            st.selected = Some(last);
            st.anchor = Some(0);
        }
        self.queue_draw();
        self.notify_selection();
    }

    /// 選んでいる行のメニューを、その行の上に出す（Menu・Shift+F10）。
    fn popup_menu(&self) {
        let Some(row) = self.selected() else { return };
        self.scroll_to_row(row);
        let scroll = self.vadjustment().map_or(0.0, |a| a.value());
        let h = self.row_height() as f64;
        let y = row as f64 * h - scroll + h / 2.0;
        let x = self.width() as f64 / 3.0;
        let f = self.state().on_menu.clone();
        if let Some(f) = f {
            f(row, x, y);
        }
    }

    fn notify_selection(&self) {
        self.update_lineage();
        let f = self.state().on_select.clone();
        if let Some(f) = f {
            // 編集の間は、並べ替えて描いているコミットの行（詳細はその行に描いているコミット）。
            f(&self.shown_selection());
        }
    }

    /// 選んでいる行に描いているコミットの行（昇順。編集で並べ替えていなければ選んでいる行のまま）。
    fn shown_selection(&self) -> Vec<u32> {
        let rows = self.selection();
        let st = self.state();
        let Some(edit) = st.edit.as_ref() else {
            return rows;
        };
        let mut shown: Vec<u32> = rows
            .iter()
            .map(|r| edit.source.get(r).copied().unwrap_or(*r))
            .collect();
        shown.sort_unstable();
        shown.dedup();
        shown
    }

    /// 検索に一致した行（昇順）を塗る。
    pub fn set_matches(&self, rows: Rc<Vec<u32>>) {
        self.state().matches = rows;
        self.queue_draw();
    }

    /// 行を右クリックしたら `f(行, x, y)` を呼ぶ。
    pub fn connect_menu(&self, f: impl Fn(u32, f64, f64) + 'static) {
        self.state().on_menu = Some(Rc::new(f));
    }

    /// merge の「+N」を押した・←/→ を押したら `f(コミット, 開くか)` を呼ぶ（入れ替えるなら None）。したら true。
    pub fn connect_fold(&self, f: impl Fn(Oid, Option<bool>) -> bool + 'static) {
        self.state().on_fold = Some(Rc::new(f));
    }

    /// (x, y) がたたんだ merge の「+N」の上なら、開いて true。
    fn click_fold_badge(&self, x: f32, y: f32) -> bool {
        let Some(row) = self.row_at(y) else {
            return false;
        };
        let (id, f) = {
            let st = self.state();
            let Some(&(a, b)) = st.fold_badges.get(&row) else {
                return false;
            };
            if !(a..=b).contains(&x) {
                return false;
            }
            let id = st
                .history
                .as_ref()
                .and_then(|h| h.graph.ids().get(row as usize).copied());
            (id, st.on_fold.clone())
        };
        match (id, f) {
            (Some(id), Some(f)) => f(id, None),
            _ => false,
        }
    }

    /// 1 つだけ選んだ merge を開く（`open`）・たたむ。編集の間は何もしない。したら true。
    fn fold_key(&self, open: bool) -> bool {
        let (id, f) = {
            let st = self.state();
            if st.edit.is_some() || !st.extra.is_empty() {
                return false;
            }
            let (Some(row), Some(history)) = (st.selected, st.history.as_ref()) else {
                return false;
            };
            let merge = history
                .layout
                .nodes()
                .get(row as usize)
                .is_some_and(|n| n.flags & amonhen_layout::Node::MERGE != 0);
            if !merge {
                return false;
            }
            (
                history.graph.ids().get(row as usize).copied(),
                st.on_fold.clone(),
            )
        };
        match (id, f) {
            (Some(id), Some(f)) => f(id, Some(open)),
            _ => false,
        }
    }

    /// 最後に描いたグラフの列の幅（列の見出しの「Message」の位置）。
    pub fn graph_width(&self) -> f32 {
        self.state().graph_width
    }

    /// グラフの列の幅が変わったら `f()` を呼ぶ。
    pub fn connect_graph_width(&self, f: impl Fn() + 'static) {
        self.state().on_graph_width = Some(Rc::new(f));
    }

    /// 列の見出しの「Message」の左端（行の文字と同じく、グラフの右端から少し空ける）。
    pub fn message_x(&self) -> f32 {
        self.graph_width() + GRAPH_GAP
    }

    /// `row` のメニュー `popover` を出した。閉じるまで、その行を枠で示す（選んでいない行でも）。
    pub fn mark_menu_row(&self, row: u32, popover: &impl IsA<gtk::Popover>) {
        self.state().menu_row = Some(row);
        self.queue_draw();
        let weak = self.downgrade();
        popover.connect_closed(move |_| {
            if let Some(view) = weak.upgrade() {
                let mut st = view.state();
                if st.menu_row == Some(row) {
                    st.menu_row = None;
                    drop(st);
                    view.queue_draw();
                }
            }
        });
    }

    /// 選んだ行が変わったら `f(選んでいる行、昇順)` を呼ぶ。
    /// GitHub の CI の状態（コミットごと）を替える。変わったら描き直す。
    pub fn set_ci(&self, ci: HashMap<Oid, Ci>) {
        {
            let mut st = self.state();
            if st.ci == ci {
                return;
            }
            st.ci = ci;
        }
        self.queue_draw();
    }

    /// 行の右端の CI の印を押したら `f(コミット, 状態, 印の位置)` を呼ぶ。
    pub fn connect_ci(&self, f: impl Fn(Oid, Ci, gdk::Rectangle) + 'static) {
        self.state().on_ci = Some(Rc::new(f));
    }

    /// `row` の行の CI の印（出していなければ None）: コミット、状態、印の位置。
    pub fn ci_mark(&self, row: u32) -> Option<(Oid, Ci, gdk::Rectangle)> {
        let rh = self.row_height();
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let st = self.state();
        if st.display.items & items::CI == 0 {
            return None;
        }
        let id = *st.history.as_ref()?.graph.ids().get(row as usize)?;
        let ci = *st.ci.get(&id)?;
        // 描くのと同じ位置（文字の列の右端から 9 の所を中心に 14 の四角）。
        let end = self.width() as f32 - TEXT_INSET;
        let cy = row as f32 * rh - scroll + rh / 2.0;
        let rect = gdk::Rectangle::new((end - 16.0) as i32, (cy - 7.0) as i32, 14, 14);
        Some((id, ci, rect))
    }

    /// (x, y) が CI の印の上なら、知らせて true。
    fn click_ci(&self, x: f32, y: f32) -> bool {
        let Some(f) = self.state().on_ci.clone() else {
            return false;
        };
        let Some(row) = self.row_at(y) else {
            return false;
        };
        let end = self.width() as f32 - TEXT_INSET;
        if x < end - CI_SPACE || x > end + 2.0 {
            return false;
        }
        let Some((id, ci, rect)) = self.ci_mark(row) else {
            return false;
        };
        f(id, ci, rect);
        true
    }

    /// 確認用: `row` の行の CI の印を押したことにする。
    pub fn click_ci_row(&self, row: u32) -> bool {
        let Some(f) = self.state().on_ci.clone() else {
            return false;
        };
        match self.ci_mark(row) {
            Some((id, ci, rect)) => {
                f(id, ci, rect);
                true
            }
            None => false,
        }
    }

    pub fn connect_selected(&self, f: impl Fn(&[u32]) + 'static) {
        self.state().on_select = Some(Rc::new(f));
    }

    /// 表示している History と、そのリポジトリ。
    pub fn current(&self) -> Option<(Arc<Repository>, Arc<HistorySnapshot>)> {
        let st = self.state();
        Some((st.repo.clone()?, st.history.clone()?))
    }

    /// `row` を表の中ほどに置く（端では端まで）。
    pub fn center_row(&self, row: u32) {
        let Some(adj) = self.vadjustment() else {
            return;
        };
        if adj.page_size() <= 0.0 {
            self.state().pending_scroll = Some((row, true));
            return;
        }
        let h = self.row_height() as f64;
        let mid = row as f64 * h + h / 2.0 - adj.page_size() / 2.0;
        adj.set_value(mid.clamp(0.0, (adj.upper() - adj.page_size()).max(0.0)));
    }

    /// 先頭の行までスクロールする（範囲・折りたたみを変えて出し直したとき。macOS 版の `resetScroll`）。
    pub fn scroll_to_top(&self) {
        self.state().pending_scroll = None;
        if let Some(adj) = self.vadjustment() {
            adj.set_value(0.0);
        }
    }

    /// `row` が見えるようにスクロールする。
    pub fn scroll_to_row(&self, row: u32) {
        let Some(adj) = self.vadjustment() else {
            return;
        };
        if adj.page_size() <= 0.0 {
            self.state().pending_scroll = Some((row, false));
            return;
        }
        let h = self.row_height() as f64;
        let top = row as f64 * h;
        if top < adj.value() {
            adj.set_value(top);
        } else if top + h > adj.value() + adj.page_size() {
            adj.set_value(top + h - adj.page_size());
        }
    }

    /// コミットを運んで落としたら `f(運んだコミット, 落とした所)` を呼ぶ。
    pub fn connect_drop_commit(&self, f: impl Fn(Oid, Dropped) + 'static) {
        self.state().on_drop = Some(Rc::new(f));
    }

    /// 出す History が替わったら `f()` を呼ぶ。
    pub fn connect_history_changed(&self, f: impl Fn() + 'static) {
        self.state().on_history = Some(Rc::new(f));
    }

    /// 運び始めたら `f()` を呼ぶ（編集できるコミットを読んで [`HistoryView::set_drag_chain`] で渡す）。
    pub fn connect_drag_start(&self, f: impl Fn() + 'static) {
        self.state().on_drag_start = Some(Rc::new(f));
    }

    /// 運んでいる間の、編集できるコミット（落とせる所を決める）。運び終えていれば捨てる。
    pub fn set_drag_chain(&self, chain: HashSet<Oid>) {
        let mut st = self.state();
        if st.dragging.is_some() {
            st.drag_chain = Some(chain);
            drop(st);
            // 落とせない行を薄くする。
            self.queue_draw();
        }
    }

    /// `row` の行を、画面と同じ絵（選んでいない行の色）にする。行の上端（ビューの座標）も返す。
    fn row_image(&self, row: u32) -> Option<(gdk::Paintable, f64)> {
        let (width, rh) = (self.width() as f32, self.row_height());
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let top = row as f32 * rh - scroll;
        let whole = gtk::Snapshot::new();
        self.state().drawing_drag_image = true;
        self.draw(&whole);
        self.state().drawing_drag_image = false;
        let node = whole.to_node()?;
        // 次の描画で、選んだ色に戻す。
        self.queue_draw();
        let image = gtk::Snapshot::new();
        // 少し透かして、下の行（重ねる行の枠）が見えるようにする（macOS の運ぶ絵と同じ）。
        image.push_opacity(0.8);
        let bounds = graphene::Rect::new(0.0, 0.0, width, rh);
        let shape = gsk::RoundedRect::from_rect(bounds, 5.0);
        let path = gsk::PathBuilder::new();
        path.add_rounded_rect(&shape);
        let path = path.to_path();
        let base = self.base_color();
        image.append_fill(
            &path,
            gsk::FillRule::Winding,
            &gdk::RGBA::new(base.red(), base.green(), base.blue(), 0.92),
        );
        let fg = self.color();
        image.append_stroke(
            &path,
            &gsk::Stroke::new(1.0),
            &gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.18),
        );
        image.push_clip(&bounds);
        image.translate(&graphene::Point::new(0.0, -top));
        image.append_node(&node);
        image.pop();
        image.pop();
        let size = graphene::Size::new(width, rh);
        Some((image.to_paintable(Some(&size))?, f64::from(top)))
    }

    /// `row` の上（その行の上の境目）に落とせるか: 運んでいるコミットが編集できる並びにあり、境目の上か下の行も
    /// 並びにある（macOS 版の `validateDrop`）。並びを読み終えるまでは落とせる。
    fn can_drop_at(&self, row: u32) -> bool {
        if let Some(ok) = self.edit_can_drop(row) {
            return ok;
        }
        let st = self.state();
        let (Some(chain), Some((_, dragged)), Some(history)) = (
            st.drag_chain.as_ref(),
            st.dragging.as_ref(),
            st.history.as_ref(),
        ) else {
            return true;
        };
        let ids = history.graph.ids();
        let id = |r: u32| ids.get(r as usize);
        let below = id(row.min((ids.len() as u32).saturating_sub(1)));
        let above = row.checked_sub(1).and_then(id);
        chain.contains(dragged)
            && (below.is_some_and(|b| chain.contains(b))
                || above.is_some_and(|a| chain.contains(a)))
    }

    /// 落とす所: `y` にいちばん近い行の境目（その下の行。いちばん下なら行の数）。
    fn drop_row_at(&self, y: f32) -> u32 {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let row = ((y + scroll) / self.row_height() + 0.5).floor().max(0.0) as u32;
        row.min(self.row_count())
    }

    /// 重ねる所: `y` が行の真ん中（上と下の 4 分の 1 は行の間）にあれば、その行。
    fn onto_row_at(&self, y: f32) -> Option<u32> {
        let row = self.row_at(y)?;
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let within = (y + scroll) / self.row_height() - row as f32;
        (0.25..0.75).contains(&within).then_some(row)
    }

    /// 運んでいるコミットを、`row` の行に重ねてまとめられるか: 同じコミットでも merge でもなく、どちらも編集の
    /// 範囲（編集していなければ編集できる並び）にある（macOS 版の `squashTarget`）。並びを読み終えるまでは、落として
    /// から確かめる。
    fn can_squash_onto(&self, row: u32) -> bool {
        let st = self.state();
        let Some((_, dragged)) = st.dragging.as_ref() else {
            return false;
        };
        if let Some(edit) = st.edit.as_ref() {
            let Some(target) = edit.slots.get(&row).and_then(|&i| edit.marks.rows.get(i)) else {
                return false;
            };
            return target.oid != *dragged && !target.merge;
        }
        let Some(history) = st.history.as_ref() else {
            return false;
        };
        let Some(target) = history.graph.ids().get(row as usize) else {
            return false;
        };
        target != dragged
            && st
                .drag_chain
                .as_ref()
                .is_none_or(|c| c.contains(dragged) && c.contains(target))
    }

    /// 運んでいるコミットを、窓の外に落として drop にできるか（編集の間は範囲の merge でない行、外では編集できる
    /// 並びの行。並びを読み終えるまでは、落としてから確かめる）。
    fn can_drop_outside(&self) -> bool {
        let st = self.state();
        let Some((_, dragged)) = st.dragging.as_ref() else {
            return false;
        };
        if let Some(edit) = st.edit.as_ref() {
            return edit
                .marks
                .rows
                .iter()
                .any(|r| r.oid == *dragged && !r.merge);
        }
        st.drag_chain.as_ref().is_none_or(|c| c.contains(dragged))
    }

    /// 運んでいる間に窓の外へ出たかを見る（窓の外で落としたら drop にする）。窓ごとに一度だけ付ける。
    fn watch_window_for_drags(&self) {
        if self.state().watching_window {
            return;
        }
        let Some(window) = self.root().and_downcast::<gtk::Window>() else {
            return;
        };
        self.state().watching_window = true;
        let motion = gtk::DropControllerMotion::new();
        let weak = self.downgrade();
        motion.connect_enter(move |_, _, _| {
            if let Some(view) = weak.upgrade() {
                view.state().drag_outside = false;
            }
        });
        let weak = self.downgrade();
        motion.connect_leave(move |_| {
            if let Some(view) = weak.upgrade() {
                view.state().drag_outside = true;
            }
        });
        window.add_controller(motion);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        keys.connect_key_released(move |_, key, _, _| {
            if key == gdk::Key::Escape
                && let Some(view) = weak.upgrade()
            {
                view.state().drag_escaped = true;
            }
        });
        window.add_controller(keys);
    }

    /// `id` を `at` に落とした。編集の間は範囲を変え、そうでなければ窓に任せる（編集モードを開く）。扱ったら true。
    fn dropped(&self, id: Oid, at: Dropped) -> bool {
        if let Some(handled) = self.edit_dropped(id, at) {
            return handled;
        }
        let f = self.state().on_drop.clone();
        match f {
            Some(f) => {
                f(id, at);
                true
            }
            None => false,
        }
    }

    /// 運んでいる間、落とす所に印を付ける（行の間なら線、重ねるなら行を囲む枠）。
    fn draw_drop(&self, snapshot: &gtk::Snapshot) {
        let Some(at) = self.state().drop_row else {
            return;
        };
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let (accent, rh, width) = (self.accent().0, self.row_height(), self.width() as f32);
        match at {
            Dropped::Above(row) => {
                let y = row as f32 * rh - scroll;
                snapshot.append_color(
                    &accent,
                    &graphene::Rect::new(4.0, y - 1.0, width - 8.0, 2.0),
                );
            }
            Dropped::Onto(row) => {
                let rect =
                    graphene::Rect::new(3.0, row as f32 * rh - scroll + 1.0, width - 6.0, rh - 2.0);
                let shape = gsk::RoundedRect::from_rect(rect, 5.0);
                snapshot.push_rounded_clip(&shape);
                snapshot.append_color(
                    &gdk::RGBA::new(accent.red(), accent.green(), accent.blue(), 0.2),
                    &rect,
                );
                snapshot.pop();
                snapshot.append_border(
                    &gsk::RoundedRect::from_rect(rect, 5.0),
                    &[2.0; 4],
                    &[accent; 4],
                );
            }
            Dropped::Outside => {}
        }
    }

    fn row_at(&self, y: f32) -> Option<u32> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let row = ((y + scroll) / self.row_height()).floor();
        (row >= 0.0 && (row as u32) < self.row_count()).then_some(row as u32)
    }

    /// `y` が最後の行より下か（shallow clone の帯の上は除く）。
    fn below_rows(&self, y: f32) -> bool {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let end = self.row_count() as f32 * self.row_height();
        let footer = if self.footer_shown() {
            FOOTER_HEIGHT
        } else {
            0.0
        };
        y + scroll >= end + footer
    }

    /// 何も選んでいない状態にする。
    fn clear_selection(&self) {
        {
            let mut st = self.state();
            if st.selected.is_none() && st.extra.is_empty() {
                return;
            }
            st.selected = None;
            st.extra.clear();
            st.anchor = None;
        }
        self.queue_draw();
        self.notify_selection();
    }

    /// 矢印で選ぶ行を動かす。`extend`（Shift）なら起点からの範囲を選ぶ。Page Up・Page Down・Home・End は
    /// 選ばずにスクロールだけ（macOS 版の表と同じ）。
    fn move_selection(&self, key: gdk::Key, extend: bool) -> bool {
        let count = self.row_count();
        if count == 0 {
            return false;
        }
        if let Some(adj) = self.vadjustment() {
            let (value, page) = (adj.value(), adj.page_size());
            let target = match key {
                gdk::Key::Page_Down => Some(value + page),
                gdk::Key::Page_Up => Some(value - page),
                gdk::Key::Home => Some(adj.lower()),
                gdk::Key::End => Some(adj.upper()),
                _ => None,
            };
            if let Some(target) = target {
                adj.set_value(target.clamp(adj.lower(), (adj.upper() - page).max(adj.lower())));
                return true;
            }
        }
        let current = self.selected();
        let row = match key {
            gdk::Key::Down => current.map_or(0, |r| (r + 1).min(count - 1)),
            gdk::Key::Up => current.map_or(0, |r| r.saturating_sub(1)),
            _ => return false,
        };
        if extend {
            self.select_with(row, true, false);
            self.scroll_to_row(row);
        } else {
            self.select_visible(row);
        }
        true
    }

    /// 最後の行の下に置く部品（shallow clone の帯）を決める。出すのは shallow clone の History で、一致したコミット
    /// だけを出していないとき。
    pub fn set_footer(&self, footer: Option<&gtk::Widget>) {
        let old = self.state().footer.take();
        if let Some(old) = old {
            old.unparent();
        }
        if let Some(footer) = footer {
            footer.set_parent(self);
            self.state().footer = Some(footer.clone());
        }
        self.configure_adjustments();
        self.queue_allocate();
    }

    /// 一致したコミットだけを出しているか（検索の「一致だけ」。その間は帯を出さない。macOS 版の `baseHistory`）。
    pub fn set_matching_only(&self, matching_only: bool) {
        if std::mem::replace(&mut self.state().matching_only, matching_only) != matching_only {
            self.configure_adjustments();
            self.queue_allocate();
        }
    }

    /// shallow clone の境界のコミットの行（昇順）。
    pub fn boundary_rows(&self) -> Vec<u32> {
        let mut rows: Vec<u32> = self.state().boundary.iter().copied().collect();
        rows.sort_unstable();
        rows
    }

    /// 境界のコミットのうち、いちばん古いものの作者の日時（macOS 版の `oldestBoundaryDate`）。境界の行は表の
    /// 終わりのほうにあり、まだ読んでいないことが多い。読んでいなければその行だけ読む。
    pub fn oldest_boundary_time(&self) -> Option<i64> {
        let st = self.state();
        st.boundary
            .iter()
            .filter_map(|&row| match st.chunks.get(&(row / CHUNK_ROWS)) {
                Some(chunk) => chunk.0.author_times.get(chunk.index(row)).copied(),
                None => {
                    let (repo, history) = (st.repo.as_ref()?, st.history.as_ref()?);
                    let chunk = repo
                        .history_chunk(history, row, 1, &FrontendProfile::GTK)
                        .ok()?;
                    chunk.author_times.first().copied()
                }
            })
            .min()
    }

    /// 最後の行の下に帯を出すか。
    fn footer_shown(&self) -> bool {
        let st = self.state();
        st.footer.is_some()
            && !st.matching_only
            && st.history.as_ref().is_some_and(|h| !h.shallow.is_empty())
    }

    /// 帯を最後の行の下に置く（スクロールで行と一緒に動く）。出さないときは隠す。
    fn place_footer(&self) {
        let Some(footer) = self.state().footer.clone() else {
            return;
        };
        let shown = self.footer_shown();
        footer.set_child_visible(shown);
        if !shown {
            return;
        }
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let top = self.row_count() as f32 * self.row_height() - scroll;
        let (minimum, _, _, _) = footer.measure(gtk::Orientation::Horizontal, -1);
        let width = self.width().max(minimum);
        footer.size_allocate(
            &gtk::Allocation::new(0, top.round() as i32, width, FOOTER_HEIGHT as i32),
            -1,
        );
    }

    fn configure_adjustments(&self) {
        let (width, height) = (self.width() as f64, self.height() as f64);
        let rows = self.row_count() as f64;
        let row_height = self.row_height() as f64;
        let footer = if self.footer_shown() {
            f64::from(FOOTER_HEIGHT)
        } else {
            0.0
        };
        if let Some(adj) = self.vadjustment() {
            let upper = (rows * row_height + footer).max(height);
            let value = adj.value().clamp(0.0, (upper - height).max(0.0));
            adj.configure(value, 0.0, upper, row_height * 3.0, height * 0.9, height);
        }
        if let Some(adj) = self.hadjustment() {
            adj.configure(0.0, 0.0, width, 10.0, width, width);
        }
        if height > 0.0 {
            let pending = self.state().pending_scroll.take();
            match pending {
                Some((row, true)) => self.center_row(row),
                Some((row, false)) => self.scroll_to_row(row),
                None => {}
            }
        }
    }

    fn fonts(&self, st: &mut State) {
        if st.fonts.is_some() {
            return;
        }
        let subject = self.pango_context().font_description().unwrap_or_default();
        let mut meta = subject.clone();
        crate::scale_font(&mut meta, &subject, 0.85);
        let mut label = subject.clone();
        crate::scale_font(&mut label, &subject, 0.80);
        label.set_weight(pango::Weight::Medium);
        // 行の高さは、件名と作者・日時の 2 段が収まるように（フォントの大きさは OS や設定で違う）。余白は
        // macOS 版の行（38 pt に 13 pt と 11 pt の 2 段、1 行なら 24 pt）と同じ。
        let line = |font: &pango::FontDescription| {
            let size = self
                .pango_context()
                .load_font(font)
                .map_or(font.size(), |f| f.describe_with_absolute_size().size());
            let em = size as f32 / pango::SCALE as f32;
            Line {
                ascent: em * 0.95,
                descent: em * 0.24,
            }
        };
        let (subject_line, meta_line, label_line) = (line(&subject), line(&meta), line(&label));
        st.metrics.row_height = if st.display.compact() {
            (subject_line.height() + 8.5).ceil().max(24.0)
        } else {
            (subject_line.height() + ROW_GAP + meta_line.height() + 7.0)
                .ceil()
                .max(Metrics::default().row_height)
        };
        st.fonts = Some(Fonts {
            subject,
            meta,
            label,
            subject_line,
            meta_line,
            label_line,
        });
    }

    fn layout(&self, text: &str, font: &pango::FontDescription) -> pango::Layout {
        let layout = self.create_pango_layout(Some(text));
        layout.set_font_description(Some(font));
        layout.set_ellipsize(pango::EllipsizeMode::End);
        layout.set_single_paragraph_mode(true);
        layout
    }

    /// 見えている範囲を描く。
    fn draw(&self, snapshot: &gtk::Snapshot) {
        let (width, height) = (self.width() as f32, self.height() as f32);
        let scroll_y = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let focused = self.has_focus();
        let fg = self.color();
        let (accent, on_accent) = self.accent();
        let base = self.base_color();
        let mut wanted = Vec::new();
        {
            let mut st = self.state();
            let st = &mut *st;
            let Some(history) = st.history.clone() else {
                return;
            };
            self.fonts(st);
            st.clipped.clear();
            st.fold_badges.clear();
            if let Some(edit) = st.edit.as_mut() {
                edit.chips.clear();
            }
            let rh = st.metrics.row_height;
            let count = history.layout.nodes().len() as u32;
            let first = ((scroll_y / rh).floor().max(0.0) as u32).min(count);
            let last = (((scroll_y + height) / rh).ceil() as u32 + 1).min(count);

            // 選んでいる行の背景。
            let color = if focused {
                accent
            } else {
                gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.12)
            };
            // 運ぶ絵は選んでいない行の色で。運んでいる行は空けるので、選択の背景も描かない。
            let chosen: Vec<u32> = if st.drawing_drag_image {
                Vec::new()
            } else {
                st.extra
                    .range(first..last)
                    .copied()
                    .chain(st.selected.filter(|r| (first..last).contains(r)))
                    .filter(|&r| Some(r) != st.dragging_row)
                    .collect()
            };
            for &row in &chosen {
                let y = row as f32 * rh - scroll_y;
                snapshot.append_color(&color, &graphene::Rect::new(0.0, y, width, rh));
            }
            // メニューを出している行の枠（macOS 版の表の、右クリックした行の枠）。
            if let Some(row) = st.menu_row.filter(|r| (first..last).contains(r)) {
                let y = row as f32 * rh - scroll_y;
                let rect = gsk::RoundedRect::from_rect(
                    graphene::Rect::new(1.0, y + 1.0, width - 2.0, rh - 2.0),
                    4.0,
                );
                let path = gsk::PathBuilder::new();
                path.add_rounded_rect(&rect);
                let ring = if chosen.contains(&row) && focused {
                    on_accent
                } else {
                    accent
                };
                snapshot.append_stroke(&path.to_path(), &gsk::Stroke::new(2.0), &ring);
            }

            // グラフ。列は見えているレーンの分だけ取り、スクロールで見えるレーンが変われば伸び縮みする（macOS 版と
            // 同じ）。使えるのは上限まで。
            let max_graph = GRAPH_WIDTH;
            st.lanes
                .update(&history, first..last, max_graph, &st.metrics, true);
            let graph_width = st.lanes.width();
            if (graph_width - st.graph_width).abs() >= 0.5 {
                st.graph_width = graph_width;
                // 描いている間に見出しを描き直させない。次の描画の前に知らせる（ふつうの idle では、History を
                // 描き続けている間は遅れる）。
                if let Some(f) = st.on_graph_width.clone() {
                    glib::idle_add_local_full(glib::Priority::HIGH, move || {
                        f();
                        glib::ControlFlow::Break
                    });
                }
            }
            snapshot.push_clip(&graphene::Rect::new(0.0, 0.0, max_graph, height));
            // 祖先は描く行まで求める（スクロールしたら続きから）。遠くへ飛んだときは 1 フレームに伸ばす行数を
            // 抑え（linux の端まで一度に伸ばすと 20 ms かかる）、描く行まで届くまでは強調せずに描いて、次のフレームで
            // 続ける（macOS 版と同じ）。
            let requested = st.lineage.is_some();
            let lineage = match st
                .lineage
                .as_mut()
                .filter(|(g, _, _)| *g == history.generation)
            {
                Some((_, _, w)) => {
                    let end = last.min(w.end().saturating_add(LINEAGE_ROWS_PER_FRAME));
                    w.extend(&history.graph, &history.layout, end);
                    Some(&*w).filter(|w| w.end() >= last)
                }
                None => None,
            };
            st.lineage_pending = requested && lineage.is_none();
            let extents = graph::draw(
                snapshot,
                &history,
                &st.lanes,
                first..last,
                scroll_y,
                &st.metrics,
                lineage,
                &st.boundary,
                st.editing_head.or(st.ref_head),
                st.dragging_row,
            );
            snapshot.pop();
            if (st.lanes.animating || st.lineage_pending) && st.tick.is_none() {
                let weak = self.downgrade();
                st.tick = Some(self.add_tick_callback(move |_, _| {
                    let Some(view) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    view.queue_draw();
                    let mut st = view.state();
                    if st.lanes.animating || st.lineage_pending {
                        glib::ControlFlow::Continue
                    } else {
                        st.tick = None;
                        glib::ControlFlow::Break
                    }
                }));
            }

            // 文字。行ごとに、その行のグラフの右端のすぐ右から始める（macOS 版と同じ）。
            let text_x_of = |row: u32| {
                extents
                    .get((row - first) as usize)
                    .map_or(graph_width + TEXT_INSET, |x| (x + GRAPH_GAP).round())
            };
            let row_end = width - TEXT_INSET;
            // 検索に一致した行の背景。文字の所だけ（その行のグラフの右端から列の終わりまで）で、選んでいる行には
            // 塗らない（macOS 版と同じ）。
            let from = st.matches.partition_point(|&r| r < first);
            for &row in st.matches[from..].iter().take_while(|&&r| r < last) {
                if st.selected == Some(row) || st.extra.contains(&row) {
                    continue;
                }
                let y = row as f32 * rh - scroll_y;
                let start = text_x_of(row);
                let rect = gsk::RoundedRect::from_rect(
                    graphene::Rect::new(start - 3.0, y + 1.0, row_end - start + 6.0, rh - 2.0),
                    4.0,
                );
                let path = gsk::PathBuilder::new();
                path.add_rounded_rect(&rect);
                snapshot.append_fill(
                    &path.to_path(),
                    gsk::FillRule::Winding,
                    &gdk::RGBA::new(1.0, 0.8, 0.0, 0.14),
                );
            }
            let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6);
            // 選んだ行の文字は、選択の色の上の文字の色（macOS 版の alternateSelectedControlTextColor）。
            let white = on_accent;
            if st.rows.len() > 4096 {
                st.rows.clear();
            }
            let compact = st.display.compact();
            // 2 行なら行の高さの中ほどに 2 行を、1 行なら件名と同じ高さに作者・日時を置く（ベースラインで）。
            let (subject_line, meta_line) = {
                let fonts = st.fonts.as_ref().expect("fonts");
                (fonts.subject_line, fonts.meta_line)
            };
            let text_top = ((rh
                - subject_line.height()
                - if compact {
                    0.0
                } else {
                    ROW_GAP + meta_line.height()
                })
                / 2.0)
                .round();
            let subject_baseline = text_top + subject_line.ascent;
            let meta_baseline = if compact {
                subject_baseline
            } else {
                (subject_baseline + subject_line.descent + ROW_GAP + meta_line.ascent).round()
            };
            let baseline = |layout: &pango::Layout| layout.baseline() as f32 / pango::SCALE as f32;
            let subject_font = st.fonts.as_ref().expect("fonts").subject.clone();
            for row in first..last {
                // 運んでいる行は空けておく（つかんだ行は絵になって動いている）。
                if st.dragging_row == Some(row) {
                    continue;
                }
                // History の上で編集しているときは、範囲の行に並べ替えた後のコミットを描く（グラフはそのまま）。
                let (slot, src) = match st.edit.as_ref() {
                    Some(e) => (
                        e.slots.get(&row).copied(),
                        e.source.get(&row).copied().unwrap_or(row),
                    ),
                    None => (None, row),
                };
                let edit_row = slot.and_then(|i| st.edit.as_ref()?.marks.rows.get(i).cloned());
                let index = src / CHUNK_ROWS;
                let Some(chunk) = st.chunks.get(&index).cloned() else {
                    if !st.loading.contains(&index) {
                        wanted.push(index);
                    }
                    continue;
                };
                if !st.rows.contains_key(&src) {
                    let text = self.row_text(st, &history, &chunk, src);
                    st.rows.insert(src, text);
                }
                let selected = focused
                    && !st.drawing_drag_image
                    && (st.selected == Some(row) || st.extra.contains(&row));
                let top = row as f32 * rh - scroll_y;
                let text_x = text_x_of(row);
                let mut x = text_x;
                let id = history.graph.ids()[src as usize];
                // GitHub の CI の状態（知っていれば）。行の右端に置き、署名の印とは離す。
                let ci = if st.display.items & items::CI != 0 {
                    st.ci.get(&id).copied()
                } else {
                    None
                };
                let text_end = if ci.is_some() {
                    row_end - CI_SPACE
                } else {
                    row_end
                };
                if let Some(ci) = ci {
                    let color = if selected { white } else { ci_color(ci, &fg) };
                    draw_ci(snapshot, ci, row_end - 9.0, top + rh / 2.0, &color);
                }
                // 署名があれば、作者・日時の行に検証の結果の色の丸を置く。結果をまだ知らなければ、裏で検証する。
                let signed = st.rows[&src].signed && st.display.items & items::SIGNATURE != 0;
                let mark = if signed {
                    let known = st.signatures.get(&id).copied();
                    if known.is_none() && !st.verifying.contains(&id) {
                        st.unverified.push(id);
                    }
                    Some(known)
                } else {
                    None
                };
                let dot = if signed { 12.0 } else { 0.0 };
                let meta_natural = {
                    let meta = &st.rows[&src].meta;
                    meta.set_width(-1);
                    meta.pixel_size().0 as f32
                };
                // 1 行なら、作者・日時・ID を右に寄せ、件名はその手前まで。
                let meta_width = if compact {
                    meta_natural.min((text_end - text_x) * 0.45)
                } else {
                    0.0
                };
                let mut subject_end = if compact {
                    text_end - meta_width - 12.0 - dot
                } else {
                    text_end
                };
                // 操作の印（git の言葉）は、件名の場所の右端に（merge の行には出さない）。
                if let Some(r) = edit_row.as_ref().filter(|r| !r.merge) {
                    let fonts = st.fonts.as_ref().expect("fonts");
                    let w = self.draw_edit_chip(
                        snapshot,
                        &mut st.label_layouts,
                        fonts,
                        r,
                        subject_end,
                        top + subject_baseline,
                        (&fg, &base),
                    );
                    if let Some(edit) = st.edit.as_mut() {
                        edit.chips.insert(row, (subject_end - w, subject_end));
                    }
                    subject_end -= w + 8.0;
                }
                // たたんだ merge は、点の右に隠したコミットの数の札（押すと開く）。
                let hidden = if st.edit.is_none() {
                    history.folded.get(src as usize).copied().unwrap_or(0)
                } else {
                    0
                };
                if hidden > 0 {
                    let colors = if selected {
                        (on_accent, accent)
                    } else {
                        (
                            gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6),
                            base,
                        )
                    };
                    let fonts = st.fonts.as_ref().expect("fonts");
                    let w = self.draw_fold_badge(
                        snapshot,
                        &mut st.label_layouts,
                        fonts,
                        hidden,
                        x,
                        top + subject_baseline,
                        colors,
                    );
                    st.fold_badges.insert(row, (x, x + w));
                    x += w + 6.0;
                }
                // 札は件名の場所の 6 割まで（残りは件名に残す）。入りきらない札は「+N」にまとめる。
                let mut clipped_labels = Vec::new();
                if st.display.items & items::REFS != 0
                    && let Some(labels) = st.labels.get(&id).cloned()
                {
                    (x, clipped_labels) = self.draw_labels(
                        snapshot,
                        st,
                        &labels,
                        x,
                        text_x + (subject_end - text_x) * 0.6,
                        top + subject_baseline,
                        selected,
                    );
                }
                let meta_max = text_end - text_x - dot;
                let line = &st.rows[&src];
                // 直したメッセージは、その件名を描く。取り消す行は線で消す。
                let custom = edit_row
                    .as_ref()
                    .filter(|r| r.subject.is_some() || r.action == Action::Drop)
                    .map(|r| {
                        let text = r
                            .subject
                            .clone()
                            .unwrap_or_else(|| line.subject.text().to_string());
                        let layout = self.layout(&text, &subject_font);
                        if r.action == Action::Drop {
                            let attrs = pango::AttrList::new();
                            attrs.insert(pango::AttrInt::new_strikethrough(true));
                            layout.set_attributes(Some(&attrs));
                        }
                        layout
                    });
                let subject = custom.as_ref().unwrap_or(&line.subject);
                // まとめる行・取り消す行の件名は薄く。
                let faint = edit_row.as_ref().is_some_and(|r| {
                    matches!(r.action, Action::Drop | Action::Squash | Action::Fixup)
                });
                subject.set_width(((subject_end - x).max(0.0) * pango::SCALE as f32) as i32);
                // 名前を全部は出していない札は、押すと名前の一覧を出す。
                if !clipped_labels.is_empty() {
                    st.clipped.insert(row, clipped_labels);
                }
                line.meta.set_width(
                    ((if compact { meta_width } else { meta_max }).max(0.0) * pango::SCALE as f32)
                        as i32,
                );
                snapshot.save();
                snapshot.translate(&graphene::Point::new(
                    x,
                    top + subject_baseline - baseline(subject),
                ));
                snapshot.append_layout(
                    subject,
                    if selected {
                        &white
                    } else if faint {
                        &secondary
                    } else {
                        &fg
                    },
                );
                snapshot.restore();
                // 1 行なら件名と並べて、ベースラインをそろえる。
                let meta_x = if compact {
                    text_end - meta_width
                } else {
                    text_x
                };
                let meta_y = top + meta_baseline - baseline(&line.meta);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(meta_x, meta_y));
                let meta_color = if selected {
                    gdk::RGBA::new(
                        white.red(),
                        white.green(),
                        white.blue(),
                        white.alpha() * 0.8,
                    )
                } else {
                    secondary
                };
                snapshot.append_layout(&line.meta, &meta_color);
                snapshot.restore();
                if let Some(mark) = mark {
                    let cx = if compact {
                        text_end - meta_width - 7.0
                    } else {
                        text_x + meta_natural.min(meta_max) + 9.0
                    };
                    // 作者・日時の字の高さの中ほど（macOS 版は x-height の半分）。
                    let cy = top + meta_baseline - meta_line.ascent * 0.28;
                    let color = if selected {
                        white
                    } else {
                        signature_color(mark, &fg)
                    };
                    // 状態ごとの印（macOS 版の `symbol`。サイドバーのタグの印・吹き出しの見出しと同じ形）。
                    crate::icon::draw_at(
                        snapshot,
                        mark.map_or(crate::icon::Kind::Seal, Status::icon),
                        &color,
                        graphene::Point::new(cx, cy),
                        11.0,
                    );
                }
            }
            let veil = gdk::RGBA::new(base.red(), base.green(), base.blue(), 0.62);
            // 運んでいる間、落とせない行（編集できる並びの外）は、グラフごと薄くする（macOS 版と同じ）。並びを読み終える
            // までは薄くしない。つかんだコミットが並びの外なら、どこにも落とせないので全部。
            if st.edit.is_none()
                && st.dragging_row.is_some()
                && let (Some(chain), Some((_, dragged))) =
                    (st.drag_chain.as_ref(), st.dragging.as_ref())
            {
                let movable = chain.contains(dragged);
                let ids = history.graph.ids();
                for row in first..last {
                    if movable && ids.get(row as usize).is_some_and(|id| chain.contains(id)) {
                        continue;
                    }
                    let y = row as f32 * rh - scroll_y;
                    snapshot.append_color(&veil, &graphene::Rect::new(0.0, y, width, rh));
                }
            }
            if let Some(edit) = st.edit.as_ref() {
                // 編集の範囲の外の行は、グラフごと薄くする（上に背景の色を重ねる。macOS 版と同じ）。
                for row in (first..last).filter(|r| !edit.slots.contains_key(r)) {
                    let y = row as f32 * rh - scroll_y;
                    snapshot.append_color(&veil, &graphene::Rect::new(0.0, y, width, rh));
                }
                // 範囲のいちばん下の境目（下へ引くと広げ、上へ引くと狭める）。
                if let Some(row) =
                    Self::edit_boundary_row(edit).filter(|r| (first..last).contains(r))
                {
                    let y = (row + 1) as f32 * rh - scroll_y;
                    snapshot.append_color(&accent, &graphene::Rect::new(0.0, y - 1.0, width, 2.0));
                    let grip = gsk::RoundedRect::from_rect(
                        graphene::Rect::new(6.0, y - 4.0, 30.0, 8.0),
                        4.0,
                    );
                    let path = gsk::PathBuilder::new();
                    path.add_rounded_rect(&grip);
                    snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, &accent);
                }
            }
        }
        self.state().complete = wanted.is_empty() && width > 0.0;
        self.verify_soon();
        // 見えている行のチャンクを先に、前後のチャンクも先読みする（速くスクロールしても、新しいチャンクに入る
        // たびに行が空で出ないように。macOS 版と同じ）。
        let ahead = self.ahead_chunks(scroll_y, height);
        for index in wanted.into_iter().chain(ahead) {
            self.load_chunk(index);
        }
        self.trim_chunks(scroll_y, height);
    }

    /// 見えている所の前後で、まだ読んでいないチャンク（後ろ、前の順）。
    fn ahead_chunks(&self, scroll_y: f32, height: f32) -> Vec<u32> {
        let st = self.state();
        let Some(history) = st.history.as_ref() else {
            return Vec::new();
        };
        let count = history.layout.nodes().len() as u32;
        neighbor_chunks(scroll_y, height, st.metrics.row_height, count)
            .into_iter()
            .filter(|i| !st.chunks.contains_key(i) && !st.loading.contains(i))
            .collect()
    }

    fn row_text(&self, st: &State, history: &HistorySnapshot, chunk: &Chunk, row: u32) -> RowText {
        let fonts = st.fonts.as_ref().expect("fonts");
        let i = chunk.index(row);
        let subject = self.layout(chunk.text(chunk.0.subjects[i]), &fonts.subject);
        // 作者・日時・ID のうち出すもの。1 行では件名に幅を譲り、日付だけにして詰めて並べる。
        let shown = st.display.items;
        let compact = st.display.compact();
        let mut parts = Vec::new();
        if shown & items::AUTHOR != 0 {
            parts.push(chunk.text(chunk.0.author_names[i]).to_owned());
        }
        if shown & items::DATE != 0 {
            // 地域の書式（1 行では短い日付だけ、2 行では .medium の日付と時刻。macOS 版と同じ）。
            let time = chunk.0.author_times[i];
            parts.push(if compact {
                crate::titles::date(time)
            } else {
                crate::titles::medium_date_time(time)
            });
        }
        if shown & items::ID != 0 {
            parts.push(history.graph.ids()[row as usize].to_string()[..8].to_owned());
        }
        let meta = parts.join(if compact { "  " } else { "  ·  " });
        let meta = self.layout(&meta, &fonts.meta);
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrFontFeatures::new("tnum"));
        meta.set_attributes(Some(&attrs));
        let signed = chunk.0.signatures.get(i).is_some_and(|&k| k != 0);
        RowText {
            subject,
            meta,
            signed,
        }
    }

    /// たたんだ merge の札（枠だけの丸い札に「+N」。下を通る線は背景の色で隠す）。`colors` は文字・背景の色
    /// （選んだ行なら、選択の上の文字と選択の色）。幅を返す。
    #[allow(clippy::too_many_arguments)]
    fn draw_fold_badge(
        &self,
        snapshot: &gtk::Snapshot,
        layouts: &mut HashMap<String, pango::Layout>,
        fonts: &Fonts,
        count: u32,
        x: f32,
        baseline: f32,
        colors: (gdk::RGBA, gdk::RGBA),
    ) -> f32 {
        let (ink, fill) = colors;
        let line = fonts.label_line;
        let shown = format!("+{count}");
        let layout = layouts.entry(format!("\u{2}{shown}")).or_insert_with(|| {
            let layout = self.create_pango_layout(Some(&shown));
            layout.set_font_description(Some(&fonts.label));
            layout
        });
        let height = (line.height() + 4.0).round();
        let width = (layout.pixel_size().0 as f32 + 12.0).round().max(height);
        let y = (baseline - line.ascent - 2.0).round();
        let rect =
            gsk::RoundedRect::from_rect(graphene::Rect::new(x, y, width, height), height / 2.0);
        let path = gsk::PathBuilder::new();
        path.add_rounded_rect(&rect);
        let path = path.to_path();
        snapshot.append_fill(&path, gsk::FillRule::Winding, &fill);
        snapshot.append_stroke(&path, &gsk::Stroke::new(1.0), &ink);
        snapshot.save();
        let text_y = baseline - 1.0 - layout.baseline() as f32 / pango::SCALE as f32;
        let text_x = x + (width - layout.pixel_size().0 as f32) / 2.0;
        snapshot.translate(&graphene::Point::new(text_x, text_y));
        snapshot.append_layout(layout, &ink);
        snapshot.restore();
        width
    }

    /// 編集の操作の印（git の言葉の札）を、右端 `right` に描いて幅を返す。そのままは薄い灰色、取り消すは赤、
    /// ほかは濃い色、コンフリクトしそうなら橙（macOS 版の `drawEditChip`）。`colors` は文字の色と背景の色。
    #[allow(clippy::too_many_arguments)]
    fn draw_edit_chip(
        &self,
        snapshot: &gtk::Snapshot,
        layouts: &mut HashMap<String, pango::Layout>,
        fonts: &Fonts,
        row: &EditRow,
        right: f32,
        baseline: f32,
        colors: (&gdk::RGBA, &gdk::RGBA),
    ) -> f32 {
        let (fg, base) = colors;
        let name = row.action.name();
        // 札の名前の置き場を使う（ブランチの名前と混ざらないよう、頭に印を付ける）。
        let layout = layouts.entry(format!("\u{1}{name}")).or_insert_with(|| {
            let layout = self.create_pango_layout(Some(name));
            layout.set_font_description(Some(&fonts.label));
            layout
        });
        let width = (layout.pixel_size().0 as f32 + 12.0).round();
        let line = fonts.label_line;
        let (fill, ink) = if row.conflict {
            (gdk::RGBA::new(0.96, 0.55, 0.10, 0.85), gdk::RGBA::WHITE)
        } else {
            match row.action {
                Action::Pick => (
                    gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.08),
                    gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6),
                ),
                Action::Drop => (gdk::RGBA::new(0.90, 0.25, 0.25, 0.8), gdk::RGBA::WHITE),
                _ => (gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.7), *base),
            }
        };
        let x = right - width;
        let rect = gsk::RoundedRect::from_rect(
            graphene::Rect::new(
                x,
                (baseline - line.ascent - 2.0).round(),
                width,
                (line.height() + 4.0).round(),
            ),
            8.0,
        );
        let path = gsk::PathBuilder::new();
        path.add_rounded_rect(&rect);
        snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, &fill);
        snapshot.save();
        let text_y = baseline - 1.0 - layout.baseline() as f32 / pango::SCALE as f32;
        snapshot.translate(&graphene::Point::new(x + 6.0, text_y));
        snapshot.append_layout(layout, &ink);
        snapshot.restore();
        width
    }

    /// ref のラベルを `x` から並べ、次に件名を置く位置と、名前を全部は出していない札（中央を切った・
    /// 「+N」にまとめた）の横の範囲を返す。
    #[allow(clippy::too_many_arguments)]
    fn draw_labels(
        &self,
        snapshot: &gtk::Snapshot,
        st: &mut State,
        labels: &[Label],
        start: f32,
        max_x: f32,
        baseline: f32,
        selected: bool,
    ) -> (f32, Vec<(f32, f32)>) {
        let fg = self.color();
        // 字の上下に 2 px ずつ（macOS 版の `drawLabels`）。
        let line = st.fonts.as_ref().expect("fonts").label_line;
        let height = (line.height() + 4.0).round();
        let y = (baseline - line.ascent - 2.0).round();
        let font = st.fonts.as_ref().expect("fonts").label.clone();
        let layouts = &mut st.label_layouts;
        let mut width_of = |shown: &str| {
            let layout = layouts.entry(shown.to_string()).or_insert_with(|| {
                let layout = self.create_pango_layout(Some(shown));
                layout.set_font_description(Some(&font));
                // 入りきらない名前は中央を切る（頭と、版番号などの末尾を残す）。
                layout.set_ellipsize(pango::EllipsizeMode::Middle);
                layout
            });
            layout.set_width(-1);
            (layout.pixel_size().0 as f32 + 10.0).round()
        };
        let widths: Vec<f32> = labels
            .iter()
            .take(LABEL_NAMES)
            .map(|l| width_of(&l.name))
            .collect();
        let slots = place_labels(&widths, labels.len(), start, max_x, |n| {
            width_of(&format!("+{n}"))
        });
        // 選んだ行の文字の色（テーマの選択の上の文字）。st を借りているので、子から直接読む。
        let on_accent = st
            .accent
            .as_ref()
            .map_or(gdk::RGBA::WHITE, |(_, f)| f.color());
        let (mut x, mut clipped) = (start, Vec::new());
        for slot in slots {
            let (shown, label) = match slot.content {
                Slot::Name(i) => (labels[i].name.clone(), Some(&labels[i])),
                Slot::Rest(n) => (format!("+{n}"), None),
            };
            let layout = &st.label_layouts[&shown];
            // 幅が足りなければ、名前を「…」で切る。
            layout.set_width(((slot.width - 10.0).max(0.0) * pango::SCALE as f32) as i32);
            if label.is_none() || layout.is_ellipsized() {
                clipped.push((slot.x, slot.x + slot.width));
            }
            let (fill, ink) = label_colors(label, selected, &fg, &on_accent);
            let rect = gsk::RoundedRect::from_rect(
                graphene::Rect::new(slot.x, y, slot.width, height),
                4.0,
            );
            let path = gsk::PathBuilder::new();
            path.add_rounded_rect(&rect);
            snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, &fill);
            snapshot.save();
            let text_y = baseline - 1.0 - layout.baseline() as f32 / pango::SCALE as f32;
            snapshot.translate(&graphene::Point::new(slot.x + 5.0, text_y));
            snapshot.append_layout(layout, &ink);
            snapshot.restore();
            x = slot.x + slot.width + 4.0;
        }
        (x + 2.0, clipped)
    }

    /// (x, y) が名前を全部は出していない札（中央を切った札・「+N」）の上なら、そのコミットの ref の名前を
    /// すべて並べた吹き出しを札の下に出して true。
    fn click_labels(&self, x: f32, y: f32) -> bool {
        let Some(row) = self.row_at(y) else {
            return false;
        };
        let (range, labels) = {
            let st = self.state();
            let Some(&range) = st
                .clipped
                .get(&row)
                .and_then(|ranges| ranges.iter().find(|(a, b)| (*a..=*b).contains(&x)))
            else {
                return false;
            };
            let labels: Vec<(String, RefKind, bool, bool)> = st
                .history
                .as_ref()
                .and_then(|h| h.graph.ids().get(row as usize))
                .and_then(|id| st.labels.get(id))
                .map(|all| {
                    all.iter()
                        .map(|l| (l.name.clone(), l.kind, l.worktree || l.elsewhere, l.head))
                        .collect()
                })
                .unwrap_or_default();
            (range, labels)
        };
        if labels.is_empty() {
            return false;
        }
        // macOS 版と同じ組み方: 見出しと、1 行に 1 つの名前（札と同じ色の印つき、選んでコピーできる）。
        let list = gtk::Box::new(gtk::Orientation::Vertical, 5);
        list.set_margin_top(6);
        list.set_margin_bottom(6);
        list.set_margin_start(8);
        list.set_margin_end(8);
        let title = gtk::Label::new(Some(&tr!("Branches and Tags")));
        title.add_css_class("heading");
        title.set_xalign(0.0);
        title.set_margin_bottom(3);
        list.append(&title);
        for (name, kind, green, head) in labels {
            // 札と同じ色の印と、選んでコピーできる名前。
            let (r, g, b) = label_base(kind, green);
            let mark = gtk::DrawingArea::new();
            mark.set_content_width(8);
            mark.set_content_height(8);
            // 名前が折り返しても 1 行目に並ぶよう、上に寄せて 1 行目の中ほどに置く。
            mark.set_valign(gtk::Align::Start);
            mark.set_margin_top(5);
            mark.set_draw_func(move |_, cr, w, h| {
                cr.set_source_rgb(r as f64, g as f64, b as f64);
                cr.rectangle(0.0, 0.0, w as f64, h as f64);
                let _ = cr.fill();
            });
            // 名前は切らない。長い名前は折り返す（ブランチ名には空白がないので、どこででも）。
            let text = gtk::Label::new(Some(&name));
            text.set_selectable(true);
            // 開いたときに名前に文字カーソルを出さない（マウスでは選べる）。
            text.set_focusable(false);
            text.set_xalign(0.0);
            text.set_wrap(true);
            text.set_wrap_mode(pango::WrapMode::Char);
            text.set_max_width_chars(70);
            if head {
                text.add_css_class("heading");
            }
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            line.append(&mark);
            line.append(&text);
            list.append(&line);
        }
        let rh = self.row_height();
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let popover = gtk::Popover::new();
        popover.set_child(Some(&list));
        popover.set_parent(self);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(
            range.0 as i32,
            (row as f32 * rh - scroll) as i32 + 2,
            (range.1 - range.0) as i32,
            20,
        )));
        popover.connect_closed(|p| p.unparent());
        popover.popup();
        true
    }

    fn load_chunk(&self, index: u32) {
        let (repo, history) = {
            let mut st = self.state();
            let (Some(repo), Some(history)) = (st.repo.clone(), st.history.clone()) else {
                return;
            };
            st.loading.insert(index);
            (repo, history)
        };
        let weak = self.downgrade();
        let target = history.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                repo.history_chunk(
                    &target,
                    index * CHUNK_ROWS,
                    CHUNK_ROWS,
                    &FrontendProfile::GTK,
                )
                .map(Chunk)
                .map_err(|e| e.to_string())
            })
            .await;
            let Some(view) = weak.upgrade() else { return };
            {
                let mut st = view.state();
                // 読んでいる間に History が替わっていたら捨てる。
                if !st
                    .history
                    .as_ref()
                    .is_some_and(|h| Arc::ptr_eq(h, &history))
                {
                    return;
                }
                st.loading.remove(&index);
                match result {
                    Ok(Ok(chunk)) => {
                        st.chunks.insert(index, Rc::new(chunk));
                    }
                    Ok(Err(e)) => eprintln!("History の行を読めない: {e}"),
                    Err(_) => {}
                }
            }
            view.queue_draw();
        });
    }

    /// 見えている所から遠いチャンクを捨てる（linux ではすべて持つと数 GB になる）。
    fn trim_chunks(&self, scroll_y: f32, height: f32) {
        let mut st = self.state();
        if st.chunks.len() <= KEPT_CHUNKS {
            return;
        }
        let center = ((scroll_y + height / 2.0) / st.metrics.row_height) as i64 / CHUNK_ROWS as i64;
        let mut indices: Vec<u32> = st.chunks.keys().copied().collect();
        indices.sort_by_key(|&i| (i as i64 - center).abs());
        for &i in &indices[KEPT_CHUNKS..] {
            st.chunks.remove(&i);
            let rows = i * CHUNK_ROWS..(i + 1) * CHUNK_ROWS;
            st.rows.retain(|r, _| !rows.contains(r));
        }
    }
}

/// History の上で編集する（macOS 版の `HistoryViewController` の「History の上で編集する」）。範囲の行に操作の
/// 印を描き、外の行はグラフごと薄くし、範囲の下に境目を引く。することの選択・並べ替え・範囲の変更は
/// [`EditEvent`] で `historyedit` に知らせ、決めた結果を [`HistoryView::set_edit`] で受け取って描き直す。
impl HistoryView {
    /// 編集の範囲を替える（None で編集を終える）。
    pub fn set_edit(&self, marks: Option<EditMarks>) {
        let before = self.shown_ids();
        let ended = marks.is_none();
        {
            let mut st = self.state();
            match marks {
                Some(marks) => {
                    // 前の並び（境目の先）を求めたものは、並びが同じならそのまま使う。
                    let (line_rows, line_key) = st
                        .edit
                        .take()
                        .map(|e| (e.line_rows, e.line_key))
                        .unwrap_or_default();
                    st.edit = Some(Edit {
                        marks,
                        slots: HashMap::new(),
                        source: HashMap::new(),
                        line_rows,
                        line_key,
                        chips: HashMap::new(),
                        preview: None,
                        dragging_boundary: false,
                    });
                }
                None => st.edit = None,
            }
        }
        self.rebuild_edit();
        self.queue_draw();
        // 並べ替えて描いていた行から元の行に戻ったとき・描くコミットが替わったときは、詳細を選んでいる行に描く
        // コミットに合わせる（macOS 版と同じ）。
        if ended || self.shown_ids() != before {
            self.notify_selection();
        }
    }

    /// 編集の間に表で起きたことを `f` に知らせる。
    pub fn connect_edit(&self, f: impl Fn(EditEvent) + 'static) {
        self.state().on_edit = Some(Rc::new(f));
    }

    /// 編集の間の、表の行が範囲の何番目か（範囲の外なら None）。
    pub fn edit_index(&self, row: u32) -> Option<usize> {
        self.state().edit.as_ref()?.slots.get(&row).copied()
    }

    /// 表の行に描いているコミットの行（編集で並べ替えていなければ同じ行）。
    pub fn shown_row(&self, row: u32) -> u32 {
        self.state()
            .edit
            .as_ref()
            .and_then(|e| e.source.get(&row).copied())
            .unwrap_or(row)
    }

    /// 表の行に描いているコミット。
    pub fn shown_commit(&self, row: u32) -> Option<Oid> {
        let shown = self.shown_row(row);
        let (_, history) = self.current()?;
        history.graph.ids().get(shown as usize).copied()
    }

    /// 選んでいる行に描いているコミット（詳細に出すもの）。
    fn shown_ids(&self) -> Vec<Oid> {
        let rows = self.shown_selection();
        let Some((_, history)) = self.current() else {
            return Vec::new();
        };
        let ids = history.graph.ids();
        rows.iter()
            .filter_map(|&r| ids.get(r as usize).copied())
            .collect()
    }

    /// 範囲の行と描くコミット、境目を引く先を、今の History の行で求め直す。
    fn rebuild_edit(&self) {
        let mut st = self.state();
        let Some(history) = st.history.clone() else {
            return;
        };
        let Some(edit) = st.edit.as_mut() else {
            return;
        };
        let rows_of: Vec<Option<u32>> = edit
            .marks
            .rows
            .iter()
            .map(|r| history.row_of(&r.oid))
            .collect();
        (edit.slots, edit.source) = edit_map(&rows_of);
        // first-parent の並びは長いことがあるので、History か並びが替わったときだけ引き直す。
        let key = (history.generation, Rc::as_ptr(&edit.marks.line));
        if edit.line_key != Some(key) {
            edit.line_rows = edit
                .marks
                .line
                .iter()
                .enumerate()
                .filter_map(|(k, id)| Some((history.row_of(id)?, k)))
                .collect();
            edit.line_rows.sort_unstable();
            edit.line_key = Some(key);
        }
    }

    /// 範囲のいちばん下の行（その下に境目を引く）。
    fn edit_boundary_row(edit: &Edit) -> Option<u32> {
        let count = edit.preview.unwrap_or(edit.marks.rows.len());
        // 並びは History の上から順に出る（行の順に並べたものは、並びの順でもある）。
        edit.line_rows
            .binary_search_by_key(&count.wrapping_sub(1), |&(_, k)| k)
            .ok()
            .map(|i| edit.line_rows[i].0)
            .or_else(|| edit.slots.keys().max().copied())
    }

    /// 編集で起きたことを知らせる（知らせた先が表を描き直すので、状態を借りずに呼ぶ）。
    fn emit_edit(&self, event: EditEvent) {
        let f = self.state().on_edit.clone();
        if let Some(f) = f {
            f(event);
        }
    }

    /// 編集の間に (x, y) を押した: 境目ならそれを引き始め、操作の印ならすることの一覧、範囲の行のダブルクリックは
    /// メッセージを直す。扱ったら true。
    fn edit_press(&self, x: f32, y: f32, presses: i32) -> bool {
        let rh = self.row_height();
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let event = {
            let mut st = self.state();
            let Some(edit) = st.edit.as_mut() else {
                return false;
            };
            if let Some(boundary) = Self::edit_boundary_row(edit)
                && ((boundary + 1) as f32 * rh - scroll - y).abs() <= 4.0
            {
                edit.dragging_boundary = true;
                return true;
            }
            let row = ((y + scroll) / rh).floor();
            if row < 0.0 {
                return false;
            }
            let row = row as u32;
            let Some(&index) = edit.slots.get(&row) else {
                return false;
            };
            if let Some(&(a, _)) = edit.chips.get(&row).filter(|(a, b)| (*a..=*b).contains(&x)) {
                EditEvent::Choices(index, a as f64, ((row + 1) as f32 * rh - scroll) as f64)
            } else if presses == 2 && edit.marks.rows.get(index).is_some_and(|r| !r.merge) {
                EditEvent::Reword(index)
            } else {
                return false;
            }
        };
        self.emit_edit(event);
        true
    }

    /// 境目を引いている間: `y` にいちばん近い並びのコミットの下へ（狭められないところへは動かさない）。
    fn drag_boundary_to(&self, y: f32) {
        let rh = self.row_height();
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let mut st = self.state();
        let Some(edit) = st.edit.as_mut().filter(|e| e.dragging_boundary) else {
            return;
        };
        let Some(k) = nearest_line_row(&edit.line_rows, y + scroll, rh) else {
            return;
        };
        let count = k + 1;
        if (count >= edit.marks.rows.len() || count >= edit.marks.min_count)
            && edit.preview != Some(count)
        {
            edit.preview = Some(count);
            drop(st);
            self.queue_draw();
        }
    }

    /// 境目を離した: 範囲を変える（変わらなければ元の境目に戻す）。
    fn end_boundary_drag(&self) {
        let count = {
            let mut st = self.state();
            let Some(edit) = st.edit.as_mut().filter(|e| e.dragging_boundary) else {
                return;
            };
            edit.dragging_boundary = false;
            let count = edit.preview.filter(|&c| c != edit.marks.rows.len());
            if count.is_none() {
                edit.preview = None;
            }
            count
        };
        self.queue_draw();
        // 範囲を変え終えたら（読み終えて `set_edit` が来たら）、引いた境目を消す。
        if let Some(count) = count {
            self.emit_edit(EditEvent::Resize(count));
        }
    }

    /// 選んだ範囲の行で p・r・s・f・d・e を押した。扱ったら true。
    fn edit_key(&self, key: gdk::Key) -> bool {
        let Some(action) = key.to_unicode().and_then(Action::from_key) else {
            return false;
        };
        let index = {
            let st = self.state();
            let Some(edit) = st.edit.as_ref() else {
                return false;
            };
            let Some(index) = st.selected.and_then(|r| edit.slots.get(&r).copied()) else {
                return false;
            };
            index
        };
        self.emit_edit(EditEvent::Choose(index, action));
        true
    }

    /// `row` の行を運べるか（編集の間は、範囲の行で、並べ替えられるときだけ。境目を引いているときは運ばない）。
    fn edit_can_drag(&self, row: u32) -> bool {
        let st = self.state();
        st.edit.as_ref().is_none_or(|e| {
            !e.dragging_boundary && e.marks.reorderable && e.slots.contains_key(&row)
        })
    }

    /// 編集の間に、`row` の上（その行の上の境目）に落とせるか: 範囲の中か、範囲のすぐ下。編集していなければ None。
    fn edit_can_drop(&self, row: u32) -> Option<bool> {
        let st = self.state();
        let edit = st.edit.as_ref()?;
        let last = edit.slots.keys().max().copied();
        Some(edit.slots.contains_key(&row) || last.is_some_and(|l| row == l + 1))
    }

    /// 編集の間に `id` を `at` に落とした: 行の間なら並べ替え、重ねたらまとめ、窓の外なら drop にする。扱ったか
    /// （編集していなければ None）。
    fn edit_dropped(&self, id: Oid, at: Dropped) -> Option<bool> {
        let event = {
            let st = self.state();
            let edit = st.edit.as_ref()?;
            let Some(from) = edit.marks.rows.iter().position(|r| r.oid == id) else {
                return Some(true);
            };
            match at {
                Dropped::Above(row) => EditEvent::Move(
                    from,
                    edit.slots
                        .get(&row)
                        .copied()
                        .unwrap_or(edit.marks.rows.len()),
                ),
                Dropped::Onto(row) => {
                    let Some(&onto) = edit.slots.get(&row) else {
                        return Some(false);
                    };
                    EditEvent::Squash(from, onto)
                }
                Dropped::Outside => EditEvent::Choose(from, Action::Drop),
            }
        };
        self.emit_edit(event);
        Some(true)
    }

    /// テーマの一覧の背景の色（編集の範囲の外を薄くする）。
    fn base_color(&self) -> gdk::RGBA {
        let probe = self.state().base_probe.clone();
        probe.map_or(gdk::RGBA::WHITE, |p| p.color())
    }
}

/// 見えている行（上端 `scroll_y`、高さ `height`）のチャンクの、すぐ後とすぐ前のチャンク（History の中のもの）。
fn neighbor_chunks(scroll_y: f32, height: f32, row_height: f32, rows: u32) -> Vec<u32> {
    if rows == 0 || row_height <= 0.0 {
        return Vec::new();
    }
    let first = ((scroll_y / row_height).floor().max(0.0) as u32).min(rows - 1);
    let last = (((scroll_y + height) / row_height).ceil() as u32).clamp(first, rows - 1);
    let (first, last) = (first / CHUNK_ROWS, last / CHUNK_ROWS);
    let after = (last + 1) * CHUNK_ROWS < rows;
    after
        .then_some(last + 1)
        .into_iter()
        .chain(first.checked_sub(1))
        .collect()
}

/// CI の印の色（macOS 版と同じく、成功は緑・失敗は赤・途中は橙）。
fn ci_color(ci: Ci, fg: &gdk::RGBA) -> gdk::RGBA {
    match ci {
        Ci::Passing => gdk::RGBA::new(0.20, 0.70, 0.30, 1.0),
        Ci::Failing => gdk::RGBA::new(0.90, 0.25, 0.25, 1.0),
        Ci::Pending => gdk::RGBA::new(0.95, 0.55, 0.15, 1.0),
        Ci::None => gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.55),
    }
}

/// CI の印を (cx, cy) を中心に描く: 枠のないチェック・バツ・点（署名の丸と形で見分ける）。
fn draw_ci(snapshot: &gtk::Snapshot, ci: Ci, cx: f32, cy: f32, color: &gdk::RGBA) {
    let path = gsk::PathBuilder::new();
    let stroke = gsk::Stroke::new(1.6);
    stroke.set_line_cap(gsk::LineCap::Round);
    stroke.set_line_join(gsk::LineJoin::Round);
    match ci {
        Ci::Passing => {
            path.move_to(cx - 3.8, cy + 0.2);
            path.line_to(cx - 1.2, cy + 2.9);
            path.line_to(cx + 4.0, cy - 3.2);
            snapshot.append_stroke(&path.to_path(), &stroke, color);
        }
        Ci::Failing => {
            path.move_to(cx - 3.2, cy - 3.2);
            path.line_to(cx + 3.2, cy + 3.2);
            path.move_to(cx + 3.2, cy - 3.2);
            path.line_to(cx - 3.2, cy + 3.2);
            snapshot.append_stroke(&path.to_path(), &stroke, color);
        }
        Ci::Pending => {
            path.add_circle(&graphene::Point::new(cx, cy), 3.0);
            snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, color);
        }
        Ci::None => {
            path.add_circle(&graphene::Point::new(cx, cy), 3.0);
            snapshot.append_stroke(&path.to_path(), &gsk::Stroke::new(1.0), color);
        }
    }
}

/// 名前で出す札の数。残りと入りきらない分は「+N」にまとめる。
const LABEL_NAMES: usize = 3;
/// 切り詰めた札に残す幅（これより狭くはしない）。
const LABEL_MINIMUM: f32 = 40.0;
const LABEL_GAP: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    /// 札の並びの中の位置。
    Name(usize),
    /// まとめた札の数（「+N」）。
    Rest(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LabelSlot {
    content: Slot,
    x: f32,
    /// 札の幅。名前の幅より狭ければ、名前を「…」で切る。
    width: f32,
}

/// 札の置き方（macOS 版の `LabelFit.place`）。`widths` は先頭から `LABEL_NAMES` 個までの札の幅、`count` は
/// 札の総数、`rest` は「+N」の札の幅。札は `max_x` を越えない。1 つ目が入らなければ切り詰め、2 つ目からは
/// 入らなければ「+N」にまとめる（「+N」の分の幅は先に取っておくので、まとめた札は必ず出る）。
fn place_labels(
    widths: &[f32],
    count: usize,
    start: f32,
    max_x: f32,
    mut rest: impl FnMut(usize) -> f32,
) -> Vec<LabelSlot> {
    let mut slots = Vec::new();
    let mut x = start;
    let names = widths.len().min(LABEL_NAMES);
    for (i, &width) in widths.iter().take(names).enumerate() {
        let after = count - i - 1;
        let reserve = if after > 0 {
            LABEL_GAP + rest(after)
        } else {
            0.0
        };
        let room = max_x - x - reserve;
        if width > room {
            if i > 0 {
                let width = rest(count - i);
                slots.push(LabelSlot {
                    content: Slot::Rest(count - i),
                    x,
                    width,
                });
                return slots;
            }
            let clipped = width.min(room.max(LABEL_MINIMUM));
            slots.push(LabelSlot {
                content: Slot::Name(0),
                x,
                width: clipped,
            });
            if after > 0 {
                let width = rest(after);
                slots.push(LabelSlot {
                    content: Slot::Rest(after),
                    x: x + clipped + LABEL_GAP,
                    width,
                });
            }
            return slots;
        }
        slots.push(LabelSlot {
            content: Slot::Name(i),
            x,
            width,
        });
        x += width + LABEL_GAP;
    }
    if count > names {
        let width = rest(count - names);
        slots.push(LabelSlot {
            content: Slot::Rest(count - names),
            x,
            width,
        });
    }
    slots
}

/// 札の種類の色（worktree とほかの worktree のブランチは緑、ローカルは青、リモートは紫、タグは橙）。
fn label_base(kind: RefKind, worktree: bool) -> (f32, f32, f32) {
    if worktree {
        return (0.20, 0.66, 0.30);
    }
    match kind {
        RefKind::Local => (0.21, 0.52, 0.89),
        RefKind::Remote => (0.57, 0.38, 0.84),
        _ => (0.90, 0.55, 0.15),
    }
}

fn label_colors(
    label: Option<&Label>,
    selected: bool,
    fg: &gdk::RGBA,
    on_accent: &gdk::RGBA,
) -> (gdk::RGBA, gdk::RGBA) {
    let Some(label) = label else {
        return (gdk::RGBA::new(0.5, 0.5, 0.5, 0.25), *fg);
    };
    let ink = if selected { *on_accent } else { *fg };
    // worktree の HEAD の札と、ほかの worktree が checkout しているブランチは緑。
    let (r, g, b) = label_base(label.kind, true);
    if label.worktree && label.head {
        return (gdk::RGBA::new(r, g, b, 0.85), gdk::RGBA::WHITE);
    }
    // ほかの worktree が checkout しているブランチは、worktree の札より少し薄い（macOS 版と同じ）。
    if label.worktree || label.elsewhere {
        let alpha = match (selected, label.worktree) {
            (true, _) => 0.45,
            (false, true) => 0.25,
            (false, false) => 0.22,
        };
        return (gdk::RGBA::new(r, g, b, alpha), ink);
    }
    let (r, g, b) = label_base(label.kind, false);
    if label.head {
        return (gdk::RGBA::new(r, g, b, 0.85), gdk::RGBA::WHITE);
    }
    let ink = if selected { *on_accent } else { *fg };
    (
        gdk::RGBA::new(r, g, b, if selected { 0.45 } else { 0.22 }),
        ink,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_rows_keep_their_places_and_draw_the_reordered_commits() {
        // 範囲の 3 つのコミットは表の 0・2・5 行目（間は取り込んだ枝）。並べ替えて、元の 5・0・2 行目の順にした。
        let (slots, source) = edit_map(&[Some(5), Some(0), Some(2)]);
        assert_eq!(slots, HashMap::from([(0, 0), (2, 1), (5, 2)]));
        assert_eq!(source, HashMap::from([(0, 5), (2, 0), (5, 2)]));
        // 表にないコミットは描かない。
        let (slots, source) = edit_map(&[Some(3), None]);
        assert_eq!(slots, HashMap::from([(3, 0)]));
        assert_eq!(source, HashMap::from([(3, 3)]));
    }

    #[test]
    fn the_boundary_moves_to_the_nearest_first_parent_row() {
        // 並びの 0・1・2 番目が表の 0・3・4 行目（行の高さ 10、下端は 10・40・50）。
        let rows = [(0, 0), (3, 1), (4, 2)];
        assert_eq!(nearest_line_row(&rows, 0.0, 10.0), Some(0));
        assert_eq!(nearest_line_row(&rows, 24.0, 10.0), Some(0));
        assert_eq!(nearest_line_row(&rows, 26.0, 10.0), Some(1));
        assert_eq!(nearest_line_row(&rows, 46.0, 10.0), Some(2));
        assert_eq!(nearest_line_row(&rows, 500.0, 10.0), Some(2));
        assert_eq!(nearest_line_row(&[], 5.0, 10.0), None);
    }
    use serde_json::json;

    #[test]
    fn labels_fit_before_the_limit_and_fold_the_rest() {
        let name = |i, x, width| LabelSlot {
            content: Slot::Name(i),
            x,
            width,
        };
        let rest = |n, x| LabelSlot {
            content: Slot::Rest(n),
            x,
            width: 20.0,
        };
        let place =
            |widths: &[f32], count, max_x| place_labels(widths, count, 0.0, max_x, |_| 20.0);
        // 入れば、そのまま並べる。
        assert_eq!(
            place(&[30.0, 30.0], 2, 100.0),
            vec![name(0, 0.0, 30.0), name(1, 34.0, 30.0)]
        );
        // 4 つ目からは「+N」。
        assert_eq!(
            place(&[10.0, 10.0, 10.0], 5, 100.0),
            vec![
                name(0, 0.0, 10.0),
                name(1, 14.0, 10.0),
                name(2, 28.0, 10.0),
                rest(2, 42.0)
            ]
        );
        // 2 つ目が入らなければ、黙って落とさずに「+N」にまとめる。
        assert_eq!(
            place(&[30.0, 60.0], 2, 80.0),
            vec![name(0, 0.0, 30.0), rest(1, 34.0)]
        );
        // 1 つ目が長ければ、切り詰める（「+N」の場所は残す）。
        assert_eq!(place(&[300.0], 1, 100.0), vec![name(0, 0.0, 100.0)]);
        assert_eq!(
            place(&[300.0, 30.0], 2, 100.0),
            vec![name(0, 0.0, 76.0), rest(1, 80.0)]
        );
        // 場所がほとんどなくても、札は最小の幅で出す。
        assert_eq!(place(&[300.0], 1, 10.0), vec![name(0, 0.0, 40.0)]);
        // まとめた札は、限りを越えない。
        for max_x in [50.0, 80.0, 120.0, 200.0] {
            let slots = place(&[40.0, 40.0, 40.0], 6, max_x);
            let last = slots.last().unwrap();
            assert!(
                last.x + last.width <= max_x.max(40.0 + 4.0 + 20.0),
                "{slots:?}"
            );
        }
    }

    #[test]
    fn neighbors_are_the_chunks_around_the_visible_rows() {
        // 行の高さ 10、256 行のチャンク。
        assert_eq!(neighbor_chunks(0.0, 100.0, 10.0, 1000), vec![1]);
        assert_eq!(neighbor_chunks(2560.0, 100.0, 10.0, 1000), vec![2, 0]);
        // チャンクの境をまたいでいれば、その外側。
        assert_eq!(neighbor_chunks(2500.0, 200.0, 10.0, 1000), vec![2]);
        assert_eq!(neighbor_chunks(9000.0, 1000.0, 10.0, 1000), vec![2]);
        assert_eq!(neighbor_chunks(0.0, 100.0, 10.0, 100), Vec::<u32>::new());
        assert_eq!(neighbor_chunks(0.0, 100.0, 10.0, 0), Vec::<u32>::new());
    }

    #[test]
    fn rows_are_one_line_without_meta_items() {
        let display = |one_line, items| Display {
            one_line,
            items,
            highlight_ancestors: true,
        };
        assert!(!display(false, items::ALL).compact());
        assert!(display(true, items::ALL).compact());
        assert!(display(false, items::REFS | items::SIGNATURE | items::CI).compact());
        assert!(!display(false, items::DATE).compact());
    }

    #[test]
    fn signature_status_matches_the_details() {
        let check = |status: &str, email: &str, committer: &str| {
            json!({
                "status": status, "signer": "A <a@x>", "key": "ABCD", "fingerprint": "",
                "trust": "full", "emails": [{ "email": email, "validity": "u" }],
                "committerEmail": committer,
            })
        };
        assert_eq!(
            signature_status(&check("G", "a@x", "a@x")),
            Status::Verified(3)
        );
        // 署名した鍵のメールがコミッターと違う（行では緑にしない）。
        assert_eq!(
            signature_status(&check("G", "a@x", "b@x")),
            Status::SignerMismatch
        );
        assert_eq!(
            signature_status(&check("V", "a@x", "")),
            Status::Unsupported
        );
        assert_eq!(signature_status(&check("R", "a@x", "")), Status::Revoked);
    }
}
