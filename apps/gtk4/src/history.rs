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

use amonhen_core::lineage::Lineage;
use amonhen_core::{HistorySnapshot, RefsSnapshot, Repository};
use amonhen_model::{Oid, RefKind, tr};
use amonhen_projection::{FrontendProfile, HistoryChunk};
use gtk::glib::Properties;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk, pango};

use crate::github::Ci;
use crate::graph::{self, Lanes, Metrics};
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
/// 持っておくチャンクの数（見えている所に近いものから）。
const KEPT_CHUNKS: usize = 24;
/// グラフの列の幅の上限。
const GRAPH_WIDTH: f32 = 160.0;
/// 文字の列の左右の余白。
const TEXT_INSET: f32 = 8.0;

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
    /// 実際に 1 行で出すか: 1 行にしたか、作者・日時の行に出す項目が 1 つもないとき。
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
    /// ほかの worktree（メインのものを除く）が checkout しているブランチ。
    elsewhere: HashSet<String>,
    label_layouts: HashMap<String, pango::Layout>,
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
    /// 検索に一致した行（昇順）。薄く塗る。
    matches: Rc<Vec<u32>>,
    lanes: Lanes,
    metrics: Metrics,
    display: Display,
    /// 選んだコミットの祖先（History の世代、行、結果）。
    lineage: Option<(u64, u32, Rc<Lineage>)>,
    lineage_request: u64,
    /// 署名の検証の結果（コミットごと）と、検証している途中のもの。
    signatures: HashMap<Oid, Status>,
    verifying: HashSet<Oid>,
    /// 描いたときに見つけた、まだ検証していない署名（少し待ってから検証する）。
    unverified: Vec<Oid>,
    verify_soon: Option<glib::SourceId>,
    /// GitHub の CI の状態（コミットごと）。窓の側が gh で読んで渡す。
    ci: HashMap<Oid, Ci>,
    /// 行の右端の CI の印を押したときに呼ぶ。
    on_ci: Option<CiCallback>,
    tick: Option<gtk::TickCallbackId>,
    /// 大きさが決まる前に見えるようにと頼まれた行（決まったらスクロールする）。
    pending_scroll: Option<u32>,
    /// 最後に描いたとき、見えている行の文字がすべてそろっていた。
    complete: bool,
    /// `--measure-scroll` の記録（1 回の `snapshot` にかかった時間）。
    snapshot_times: Option<Vec<f64>>,
    /// コミットを運んで落としたときに呼ぶ（運んだコミット、落とした所の下の行。最後の行より下なら行の数）。
    on_drop: Option<DropCallback>,
    /// 運んでいる間の、落とす所（その行の上に線を引く）。
    drop_row: Option<u32>,
}

/// コミットを落としたときに呼ぶもの。
type DropCallback = Rc<dyn Fn(Oid, u32)>;

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
        fn constructed(&self) {
            self.parent_constructed();
            let view = self.obj();
            view.set_focusable(true);
            view.set_overflow(gtk::Overflow::Hidden);
            // テーマの一覧の背景（暗いテーマでも文字と合う）。
            view.add_css_class("view");

            let click = gtk::GestureClick::new();
            let weak = view.downgrade();
            click.connect_pressed(move |gesture, _, x, y| {
                if let Some(view) = weak.upgrade() {
                    view.grab_focus();
                    // 行の右端の CI の印なら、行を選ばずに詳細の吹き出し。
                    if view.click_ci(x as f32, y as f32) {
                        return;
                    }
                    if let Some(row) = view.row_at(y as f32) {
                        // Ctrl で 1 行ずつ足す・外す、Shift で範囲（macOS の ⌘・⇧ と同じ）。
                        let state = gesture.current_event_state();
                        view.select_with(
                            row,
                            state.contains(gdk::ModifierType::SHIFT_MASK),
                            state.contains(gdk::ModifierType::CONTROL_MASK),
                        );
                    }
                }
            });
            view.add_controller(click);

            // 右クリック: その行を選んで、コミットのメニュー。
            let menu = gtk::GestureClick::new();
            menu.set_button(gdk::BUTTON_SECONDARY);
            let weak = view.downgrade();
            menu.connect_pressed(move |_, _, x, y| {
                let Some(view) = weak.upgrade() else { return };
                view.grab_focus();
                if let Some(row) = view.row_at(y as f32) {
                    // 複数選んでいる行の上なら、選んだまま（まとめての操作のメニュー）。
                    if !view.selection().contains(&row) {
                        view.select(row);
                    }
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
            let subject = Rc::new(RefCell::new(String::new()));
            let (weak, sub) = (view.downgrade(), subject.clone());
            drag.connect_prepare(move |_, _, y| {
                let view = weak.upgrade()?;
                let row = view.row_at(y as f32)?;
                let (_, history) = view.current()?;
                let id = *history.graph.ids().get(row as usize)?;
                // 運んでいる間は、件名を札にして出す。件名は読んである行のものを使う（ここで読むと、daemon・
                // SSH 先では 1 往復ぶん画面が止まる。macOS 版と同じ）。
                sub.replace(
                    view.loaded_subject(row)
                        .unwrap_or_else(|| id.to_string()[..8].to_owned()),
                );
                let value = format!("{DRAG_PREFIX}{id}");
                Some(gdk::ContentProvider::for_value(&value.to_value()))
            });
            drag.connect_drag_begin(move |_, drag| {
                let label = gtk::Label::new(Some(&subject.borrow()));
                label.set_max_width_chars(48);
                label.set_ellipsize(pango::EllipsizeMode::End);
                label.add_css_class("osd");
                label.add_css_class("card");
                label.set_margin_start(6);
                label.set_margin_end(6);
                gtk::DragIcon::for_drag(drag).set_child(Some(&label));
            });
            view.add_controller(drag);
            let target = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
            let weak = view.downgrade();
            target.connect_motion(move |_, _, y| {
                let Some(view) = weak.upgrade() else {
                    return gdk::DragAction::empty();
                };
                let row = view.drop_row_at(y as f32);
                if view.state().drop_row != Some(row) {
                    view.state().drop_row = Some(row);
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
                view.state().drop_row = None;
                view.queue_draw();
                let Some(id) = value
                    .get::<String>()
                    .ok()
                    .and_then(|v| v.strip_prefix(DRAG_PREFIX).and_then(Oid::from_hex))
                else {
                    return false;
                };
                let row = view.drop_row_at(y as f32);
                let f = view.state().on_drop.clone();
                match f {
                    Some(f) => {
                        f(id, row);
                        true
                    }
                    None => false,
                }
            });
            view.add_controller(target);

            let keys = gtk::EventControllerKey::new();
            let weak = view.downgrade();
            keys.connect_key_pressed(move |_, key, _, state| {
                let Some(view) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
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
            self.obj().draw(snapshot);
            self.obj().draw_drop(snapshot);
            if let Some(times) = self.state.borrow_mut().snapshot_times.as_mut() {
                times.push(started.elapsed().as_secs_f64() * 1000.0);
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

    pub fn row_height(&self) -> f32 {
        let mut st = self.state();
        self.fonts(&mut st);
        st.metrics.row_height
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
            let changed =
                st.selected.is_none() != selected.is_none() || st.extra.len() != extra.len();
            // 最後に選んだコミットが消えたら（ほかのプログラムが rebase・amend した）、残りのうち最後のものに
            // する。何も残らなければ先頭（いちばん新しいコミット）を選ぶ（macOS 版と同じ。表示の位置は動かさない）。
            if selected.is_some() && st.selected.is_none() {
                st.selected = st.extra.pop_last();
                if st.selected.is_none() && !history.graph.ids().is_empty() {
                    st.selected = Some(0);
                    st.anchor = Some(0);
                }
            }
            st.history = Some(history);
            st.chunks.clear();
            st.loading.clear();
            st.rows.clear();
            st.complete = false;
            // 行の番号が変わるので、検索の一致は捨てる（検索の欄が探し直す）。
            st.matches = Rc::default();
            changed
        };
        self.configure_adjustments();
        self.update_lineage();
        self.queue_draw();
        // 選んだコミットが変わったときだけ知らせる（詳細が消えたコミットのまま残らないように）。
        if changed {
            self.notify_selection();
        }
    }

    /// 行の見せ方を替える（1 行・項目・祖先の強調）。
    pub fn set_display(&self, display: Display) {
        {
            let mut st = self.state();
            if st.display == display {
                return;
            }
            st.display = display;
            st.rows.clear();
            st.fonts = None;
        }
        self.configure_adjustments();
        if let Some(row) = self.selected() {
            self.scroll_to_row(row);
        }
        self.update_lineage();
        self.queue_draw();
    }

    /// 選んだコミットの祖先を裏で求め、求まったらグラフを描き直す。
    fn update_lineage(&self) {
        let (history, row, request) = {
            let mut st = self.state();
            st.lineage_request += 1;
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
            (history, row, st.lineage_request)
        };
        let weak = self.downgrade();
        let generation = history.generation;
        glib::spawn_future_local(async move {
            let Ok(lineage) = gio::spawn_blocking(move || {
                amonhen_core::lineage::lineage(&history.graph, &history.layout, row)
            })
            .await
            else {
                return;
            };
            let Some(view) = weak.upgrade() else { return };
            {
                let mut st = view.state();
                if st.lineage_request != request {
                    return;
                }
                st.lineage = Some((generation, row, Rc::new(lineage)));
            }
            view.queue_draw();
        });
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
                let (repo, pending) = {
                    let mut st = view.state();
                    st.verify_soon = None;
                    let ids = std::mem::take(&mut st.unverified);
                    let mut pending: Vec<Oid> = Vec::new();
                    for id in ids {
                        if !st.signatures.contains_key(&id) && st.verifying.insert(id) {
                            pending.push(id);
                        }
                    }
                    (st.repo.clone(), pending)
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

    /// 読んである行の件名（まだ読んでいなければ `None`）。
    fn loaded_subject(&self, row: u32) -> Option<String> {
        let st = self.state();
        let chunk = st.chunks.get(&(row / CHUNK_ROWS))?;
        let i = chunk.index(row);
        Some(chunk.text(chunk.0.subjects[i]).to_owned())
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

    /// `row` だけを選ぶ。
    pub fn select(&self, row: u32) {
        let changed = {
            let mut st = self.state();
            let changed = st.selected != Some(row) || !st.extra.is_empty();
            st.selected = Some(row);
            st.extra.clear();
            st.anchor = Some(row);
            changed
        };
        self.scroll_to_row(row);
        self.queue_draw();
        if changed {
            self.notify_selection();
        }
    }

    /// クリックで選ぶ。`shift` なら起点からの範囲、`ctrl` なら 1 行ずつ足す・外す。
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
                    self.select(row);
                    return;
                }
            }
        }
        self.queue_draw();
        self.notify_selection();
    }

    fn notify_selection(&self) {
        self.update_lineage();
        let f = self.state().on_select.clone();
        if let Some(f) = f {
            f(&self.selection());
        }
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

    /// `row` が見えるようにスクロールする。
    pub fn scroll_to_row(&self, row: u32) {
        let Some(adj) = self.vadjustment() else {
            return;
        };
        if adj.page_size() <= 0.0 {
            self.state().pending_scroll = Some(row);
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

    /// コミットを運んで落としたら `f(運んだコミット, 落とした所の下の行)` を呼ぶ。
    pub fn connect_drop_commit(&self, f: impl Fn(Oid, u32) + 'static) {
        self.state().on_drop = Some(Rc::new(f));
    }

    /// 落とす所: `y` にいちばん近い行の境目（その下の行。いちばん下なら行の数）。
    fn drop_row_at(&self, y: f32) -> u32 {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let row = ((y + scroll) / self.row_height() + 0.5).floor().max(0.0) as u32;
        row.min(self.row_count())
    }

    /// 運んでいる間、落とす所に線を引く。
    fn draw_drop(&self, snapshot: &gtk::Snapshot) {
        let Some(row) = self.state().drop_row else {
            return;
        };
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let y = row as f32 * self.row_height() - scroll;
        snapshot.append_color(
            &gdk::RGBA::new(0.21, 0.52, 0.89, 1.0),
            &graphene::Rect::new(4.0, y - 1.0, self.width() as f32 - 8.0, 2.0),
        );
    }

    fn row_at(&self, y: f32) -> Option<u32> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let row = ((y + scroll) / self.row_height()).floor();
        (row >= 0.0 && (row as u32) < self.row_count()).then_some(row as u32)
    }

    /// 矢印などで選ぶ行を動かす。`extend`（Shift）なら起点からの範囲を選ぶ。
    fn move_selection(&self, key: gdk::Key, extend: bool) -> bool {
        let count = self.row_count();
        if count == 0 {
            return false;
        }
        let page = self.vadjustment().map_or(1.0, |a| a.page_size()) as f32 / self.row_height();
        let page = (page.floor() as u32).max(1);
        let current = self.selected();
        let row = match key {
            gdk::Key::Down => current.map_or(0, |r| (r + 1).min(count - 1)),
            gdk::Key::Up => current.map_or(0, |r| r.saturating_sub(1)),
            gdk::Key::Page_Down => current.map_or(0, |r| (r + page).min(count - 1)),
            gdk::Key::Page_Up => current.map_or(0, |r| r.saturating_sub(page)),
            gdk::Key::Home => 0,
            gdk::Key::End => count - 1,
            _ => return false,
        };
        if extend {
            self.select_with(row, true, false);
            self.scroll_to_row(row);
        } else {
            self.select(row);
        }
        true
    }

    fn configure_adjustments(&self) {
        let (width, height) = (self.width() as f64, self.height() as f64);
        let rows = self.row_count() as f64;
        let row_height = self.row_height() as f64;
        if let Some(adj) = self.vadjustment() {
            let upper = (rows * row_height).max(height);
            let value = adj.value().clamp(0.0, (upper - height).max(0.0));
            adj.configure(value, 0.0, upper, row_height * 3.0, height * 0.9, height);
        }
        if let Some(adj) = self.hadjustment() {
            adj.configure(0.0, 0.0, width, 10.0, width, width);
        }
        if height > 0.0 {
            let pending = self.state().pending_scroll.take();
            if let Some(row) = pending {
                self.scroll_to_row(row);
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
        // 行の高さは、件名と作者・日時の 2 段が収まるように（フォントの大きさは OS や設定で違う）。
        let height = |font: &pango::FontDescription| {
            let layout = self.create_pango_layout(Some("Ag"));
            layout.set_font_description(Some(font));
            layout.pixel_size().1 as f32
        };
        st.metrics.row_height = if st.display.compact() {
            (height(&subject) + 8.0).ceil().max(24.0)
        } else {
            (height(&subject) + 2.0 + height(&meta) + 8.0)
                .ceil()
                .max(Metrics::default().row_height)
        };
        st.fonts = Some(Fonts {
            subject,
            meta,
            label,
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
        let mut wanted = Vec::new();
        {
            let mut st = self.state();
            let st = &mut *st;
            let Some(history) = st.history.clone() else {
                return;
            };
            self.fonts(st);
            let rh = st.metrics.row_height;
            let count = history.layout.nodes().len() as u32;
            let first = ((scroll_y / rh).floor().max(0.0) as u32).min(count);
            let last = (((scroll_y + height) / rh).ceil() as u32 + 1).min(count);

            // 検索に一致した行の背景（選んでいる行は、選んだ色で上から塗る）。
            let from = st.matches.partition_point(|&r| r < first);
            for &row in st.matches[from..].iter().take_while(|&&r| r < last) {
                let y = row as f32 * rh - scroll_y;
                let rect = gsk::RoundedRect::from_rect(
                    graphene::Rect::new(2.0, y + 1.0, width - 4.0, rh - 2.0),
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
            // 選んでいる行の背景。
            let color = if focused {
                gdk::RGBA::new(0.21, 0.52, 0.89, 1.0)
            } else {
                gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.12)
            };
            let chosen: Vec<u32> = st
                .extra
                .range(first..last)
                .copied()
                .chain(st.selected.filter(|r| (first..last).contains(r)))
                .collect();
            for &row in &chosen {
                let y = row as f32 * rh - scroll_y;
                snapshot.append_color(&color, &graphene::Rect::new(0.0, y, width, rh));
            }

            // グラフ。
            // レーンが少なければ狭く（History 全体のレーンの数で決め、スクロールで変えない）。
            let m = &st.metrics;
            let lanes = history.layout.lane_count() as f32;
            let graph_width = (m.left_inset + lanes * m.lane_width)
                .clamp(40.0, GRAPH_WIDTH)
                .min(width * 0.4);
            st.lanes
                .update(&history, first..last, graph_width, &st.metrics, true);
            snapshot.push_clip(&graphene::Rect::new(0.0, 0.0, graph_width, height));
            graph::draw(
                snapshot,
                &history,
                &st.lanes,
                first..last,
                scroll_y,
                &st.metrics,
                st.lineage
                    .as_ref()
                    .filter(|(g, _, _)| *g == history.generation)
                    .map(|(_, _, l)| &**l),
            );
            snapshot.pop();
            if st.lanes.animating && st.tick.is_none() {
                let weak = self.downgrade();
                st.tick = Some(self.add_tick_callback(move |_, _| {
                    let Some(view) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    view.queue_draw();
                    let mut st = view.state();
                    if st.lanes.animating {
                        glib::ControlFlow::Continue
                    } else {
                        st.tick = None;
                        glib::ControlFlow::Break
                    }
                }));
            }

            // 文字。
            let text_x = graph_width + TEXT_INSET;
            let row_end = width - TEXT_INSET;
            let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6);
            let white = gdk::RGBA::WHITE;
            if st.rows.len() > 4096 {
                st.rows.clear();
            }
            let compact = st.display.compact();
            for row in first..last {
                let index = row / CHUNK_ROWS;
                let Some(chunk) = st.chunks.get(&index).cloned() else {
                    if !st.loading.contains(&index) {
                        wanted.push(index);
                    }
                    continue;
                };
                if !st.rows.contains_key(&row) {
                    let text = self.row_text(st, &history, &chunk, row);
                    st.rows.insert(row, text);
                }
                let selected = focused && (st.selected == Some(row) || st.extra.contains(&row));
                let top = row as f32 * rh - scroll_y;
                let mut x = text_x;
                let line = &st.rows[&row];
                let (_, subject_h) = line.subject.pixel_size();
                let (_, meta_h) = line.meta.pixel_size();
                let gap = 2.0;
                let text_top = ((rh - subject_h as f32 - gap - meta_h as f32) / 2.0).round();
                // 2 行なら行の高さの中ほどに 2 行を、1 行なら件名と同じ高さに作者・日時を置く。
                let text_top = if compact {
                    ((rh - subject_h as f32) / 2.0).round()
                } else {
                    text_top
                };
                let id = history.graph.ids()[row as usize];
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
                if st.display.items & items::REFS != 0
                    && let Some(labels) = st.labels.get(&id).cloned()
                {
                    x = self.draw_labels(
                        snapshot,
                        st,
                        &labels,
                        x,
                        text_end - 40.0,
                        top + text_top,
                        subject_h as f32,
                        selected,
                    );
                }
                // 署名があれば、作者・日時の行に検証の結果の色の丸を置く。結果をまだ知らなければ、裏で検証する。
                let signed = st.rows[&row].signed && st.display.items & items::SIGNATURE != 0;
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
                let line = &st.rows[&row];
                line.meta.set_width(-1);
                let meta_natural = line.meta.pixel_size().0 as f32;
                // 1 行なら、作者・日時・ID を右に寄せ、件名はその手前まで。
                let meta_width = if compact {
                    meta_natural.min((text_end - text_x) * 0.45)
                } else {
                    0.0
                };
                let subject_end = if compact {
                    text_end - meta_width - 12.0 - dot
                } else {
                    text_end
                };
                let meta_max = text_end - text_x - dot;
                line.subject
                    .set_width(((subject_end - x).max(0.0) * pango::SCALE as f32) as i32);
                line.meta.set_width(
                    ((if compact { meta_width } else { meta_max }).max(0.0) * pango::SCALE as f32)
                        as i32,
                );
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x, top + text_top));
                snapshot.append_layout(&line.subject, if selected { &white } else { &fg });
                snapshot.restore();
                let (meta_x, meta_y) = if compact {
                    // 件名と並べて、ベースラインをそろえる。
                    let shift = (line.subject.baseline() - line.meta.baseline()) as f32
                        / pango::SCALE as f32;
                    (text_end - meta_width, top + text_top + shift)
                } else {
                    (text_x, top + text_top + subject_h as f32 + gap)
                };
                snapshot.save();
                snapshot.translate(&graphene::Point::new(meta_x, meta_y));
                let meta_color = if selected {
                    gdk::RGBA::new(1.0, 1.0, 1.0, 0.8)
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
                    let cy = meta_y + meta_h as f32 / 2.0;
                    let path = gsk::PathBuilder::new();
                    path.add_circle(&graphene::Point::new(cx, cy), 3.5);
                    let color = if selected {
                        white
                    } else {
                        signature_color(mark, &fg)
                    };
                    snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, &color);
                }
            }
        }
        self.state().complete = wanted.is_empty() && width > 0.0;
        self.verify_soon();
        for index in wanted {
            self.load_chunk(index);
        }
        self.trim_chunks(scroll_y, height);
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
            let format = if compact {
                "%Y-%m-%d"
            } else {
                "%Y-%m-%d %H:%M"
            };
            parts.push(
                glib::DateTime::from_unix_local(chunk.0.author_times[i])
                    .and_then(|d| d.format(format))
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
            );
        }
        if shown & items::ID != 0 {
            parts.push(history.graph.ids()[row as usize].to_string()[..8].to_owned());
        }
        // 折りたたんだ merge は、隠しているコミットの数を添える。
        if let Some(&hidden) = history.folded.get(row as usize)
            && hidden > 0
        {
            parts.push(format!("⊕ {}", tr!("{count} folded", count = hidden)));
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

    /// ref のラベルを `x` から並べ、次に件名を置く位置を返す。
    #[allow(clippy::too_many_arguments)]
    fn draw_labels(
        &self,
        snapshot: &gtk::Snapshot,
        st: &mut State,
        labels: &[Label],
        start: f32,
        max_x: f32,
        top: f32,
        line_height: f32,
        selected: bool,
    ) -> f32 {
        let fg = self.color();
        let mut x = start;
        for (i, label) in labels.iter().enumerate() {
            let shown = if i < 3 {
                label.name.clone()
            } else {
                format!("+{}", labels.len() - i)
            };
            if !st.label_layouts.contains_key(&shown) {
                let font = &st.fonts.as_ref().expect("fonts").label;
                let layout = self.create_pango_layout(Some(&shown));
                layout.set_font_description(Some(font));
                st.label_layouts.insert(shown.clone(), layout);
            }
            let layout = &st.label_layouts[&shown];
            let (w, h) = layout.pixel_size();
            let width = (w as f32 + 10.0).round();
            let height = h as f32 + 2.0;
            if x + width > max_x && i > 0 {
                break;
            }
            let y = (top + (line_height - height) / 2.0).round();
            let (fill, ink) = label_colors(i < 3, label, selected, &fg);
            let rect = gsk::RoundedRect::from_rect(graphene::Rect::new(x, y, width, height), 4.0);
            let path = gsk::PathBuilder::new();
            path.add_rounded_rect(&rect);
            snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, &fill);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x + 5.0, y + 1.0));
            snapshot.append_layout(layout, &ink);
            snapshot.restore();
            x += width + 4.0;
            if i >= 3 {
                break;
            }
        }
        x + 2.0
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
        let generation = history.generation;
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                repo.history_chunk(
                    &history,
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
                if st.history.as_ref().map(|h| h.generation) != Some(generation) {
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

fn label_colors(
    named: bool,
    label: &Label,
    selected: bool,
    fg: &gdk::RGBA,
) -> (gdk::RGBA, gdk::RGBA) {
    if !named {
        return (gdk::RGBA::new(0.5, 0.5, 0.5, 0.25), *fg);
    }
    let ink = if selected { gdk::RGBA::WHITE } else { *fg };
    // worktree の HEAD の札と、ほかの worktree が checkout しているブランチは緑。
    let (r, g, b) = (0.20, 0.66, 0.30);
    if label.worktree && label.head {
        return (gdk::RGBA::new(r, g, b, 0.85), gdk::RGBA::WHITE);
    }
    if label.worktree || label.elsewhere {
        let alpha = if selected { 0.45 } else { 0.25 };
        return (gdk::RGBA::new(r, g, b, alpha), ink);
    }
    let (r, g, b) = match label.kind {
        RefKind::Local => (0.21, 0.52, 0.89),
        RefKind::Remote => (0.57, 0.38, 0.84),
        _ => (0.90, 0.55, 0.15),
    };
    if label.head {
        return (gdk::RGBA::new(r, g, b, 0.85), gdk::RGBA::WHITE);
    }
    let ink = if selected { gdk::RGBA::WHITE } else { *fg };
    (
        gdk::RGBA::new(r, g, b, if selected { 0.45 } else { 0.22 }),
        ink,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
