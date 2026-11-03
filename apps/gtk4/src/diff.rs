//! 差分の表示。コミットの全ファイルを、ファイルの見出し・hunk の見出し・行の順に並べる（`StackedDiff`）。
//!
//! History と同じく、見えている行だけを 1 つのウィジェットの `snapshot` で描く。変わった単語の強調と構文の
//! 色は、コアが FE の符号単位（UTF-8 のバイト位置）で渡すので、そのまま Pango の属性にする。
//! ファイルの見出しをクリックすると、そのファイルを畳む・開く。本文は折り返さず、一番長い行まで横にスクロール
//! する（行番号・印・ファイルの見出しは動かない。macOS 版と同じ）。
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
use std::sync::Arc;

use amonhen_core::diff::{DiffView, StackedDiff};
use amonhen_layout::{DiffRow, SyntaxSpan};
use amonhen_model::tr;
use amonhen_projection::FrontendProfile;
use gtk::glib::Properties;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk, pango};

/// 左の行番号の列と本文の間の余白。
const GUTTER_GAP: f32 = 12.0;
const TEXT_INSET: f32 = 8.0;
/// 行ごとの stage の印の列の幅。
const MARK_WIDTH: f32 = 20.0;

/// 文字の位置（表示の行と、その行の文字のバイト位置）。
type TextPos = (u32, u32);

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

    /// コアの表示（確認用）。
    #[cfg(test)]
    pub fn diff_view(&self) -> &DiffView {
        &self.view
    }
}

/// 差分の元（ファイルを畳んだとき、ここから作り直す）。2 つめがあれば、選んだファイルの差分だけを読む元で、
/// 1 つめの各ファイルの差分は空（名前だけが正しい）。
pub struct Source(pub StackedDiff, pub Option<Lazy>);

// SAFETY: `StackedDiff` の中身は共有の不変なバッファ（`Blob`）と、自分で持つ文字列と行の範囲だけ。
unsafe impl Send for Source {}
unsafe impl Sync for Source {}

/// 選んだファイルの差分だけを読む元（全ファイルを並べないコミット。macOS 版と同じ）。
pub struct Lazy {
    pub repo: Arc<amonhen_core::Repository>,
    pub changes: Vec<amonhen_model::FileChange>,
}

impl Source {
    /// 全ファイルの差分で `f` を呼ぶ（選んだファイルだけを読む元なら、ここで全部読む）。裏のスレッドで呼ぶ。
    pub fn with_all<R>(&self, f: impl FnOnce(&StackedDiff) -> R) -> R {
        match &self.1 {
            Some(lazy) => {
                let all = lazy
                    .repo
                    .stacked_diff(&lazy.changes)
                    .unwrap_or(StackedDiff { files: Vec::new() });
                f(&all)
            }
            None => f(&self.0),
        }
    }
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
    /// Viewed にしたファイル（番号）。
    viewed: HashSet<usize>,
    /// 見出しの Viewed を押したときに呼ぶ（ファイルの番号）。None なら Viewed を出さない。
    on_toggle_viewed: Option<Rc<dyn Fn(usize)>>,
    /// 一番上に見えているファイルが変わったときに呼ぶ（ファイルの番号）。
    on_top_file: Option<Rc<dyn Fn(usize)>>,
    top_file: Option<usize>,
    /// 差分の中を探している語と、一致（行、バイト位置の始めと終わり）、今の一致。
    find: String,
    matches: Vec<(u32, u32, u32)>,
    current_match: Option<usize>,
    /// 差分の代わりに出す知らせ（「Loading the diff…」など）。
    message: Option<String>,
}

/// ファイルの見出し・差分の行を右クリックしたときに呼ぶ（見出しの上ならファイルの番号、パス、x、y）。
type FileMenuCallback = Rc<dyn Fn(Option<usize>, &str, f64, f64)>;

/// 見出しのない差分の行を右クリックしたときに呼ぶ（行、x、y）。
type RowMenuCallback = Rc<dyn Fn(usize, f64, f64)>;

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
    }

    impl WidgetImpl for DiffWidget {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.obj().configure_adjustments();
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
            self.obj().draw(snapshot);
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

    /// 新しい差分を出す（先頭から）。
    pub fn show(&self, source: Arc<Source>, view: View) {
        let collapsed = vec![false; source.0.files.len()];
        self.show_files(source, view, collapsed, HashSet::new());
    }

    /// 新しい差分を、`collapsed` のファイルを畳み（`view` はそれで作ったもの）、`viewed` を Viewed にして出す。
    pub fn show_files(
        &self,
        source: Arc<Source>,
        view: View,
        collapsed: Vec<bool>,
        viewed: HashSet<usize>,
    ) {
        {
            let mut st = self.state();
            st.generation += 1;
            st.message = None;
            st.collapsed = collapsed;
            st.viewed = viewed;
            st.top_file = None;
            st.source = Some(source);
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = false;
            st.checks = false;
            st.selection.clear();
            st.anchor = None;
            st.text_selection = None;
            st.hunk_cursor = None;
        }
        self.scroll_to_start();
        self.configure_adjustments();
        self.refresh_matches();
        self.queue_draw();
        self.selection_changed();
    }

    /// Working Copy の 1 ファイルの差分を、行ごとの stage の印つきで出す。`keep` なら同じファイルを
    /// 読み直したとして、スクロールの位置を保つ。選んだ行はいつも外す（macOS 版と同じ。読み直すと行の番号が
    /// 別の行を指すことがあり、そのまま Stage Lines・Discard Lines を押すと違う行に効いてしまう）。
    pub fn show_working(&self, view: View, keep: bool, checks: bool) {
        {
            let mut st = self.state();
            st.generation += 1;
            st.message = None;
            st.source = None;
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = true;
            st.checks = checks;
            st.selection.clear();
            st.anchor = None;
            st.text_selection = None;
            if !keep {
                st.hunk_after = None;
            }
            st.hunk_cursor = None;
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

    /// 印も選択もない表示（コンフリクトのマーカー付きの中身など）。
    pub fn show_plain(&self, view: View) {
        {
            let mut st = self.state();
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
        let index = ((y as f32 + scroll) / self.row_height()).floor();
        (index >= 0.0 && (index as u32) < self.row_count()).then_some(index as usize)
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
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let rh = self.row_height();
        let (view, f, rows_f) = {
            let st = self.state();
            (
                st.view.clone(),
                st.on_file_menu.clone(),
                st.on_row_menu.clone(),
            )
        };
        let index = ((y as f32 + scroll) / rh).floor();
        if let (Some(view), Some(rows_f)) = (&view, rows_f)
            && index >= 0.0
            && (index as usize) < view.view.rows.len()
            && view.file_rows.is_empty()
        {
            rows_f(index as usize, x, y);
            return;
        }
        // 右クリックのメニューを持ち主が作らない差分では、何も出さない（コピーは Ctrl+C。macOS 版と同じ）。
        let (Some(view), Some(f)) = (view, f) else {
            return;
        };
        if index < 0.0 {
            return;
        }
        // 見出し（上に張り付いたものを含む）の上ならそのファイル、下の行ならその行のファイル。
        let header = self.header_at(y as f32);
        let file = header.or_else(|| {
            view.file_rows
                .iter()
                .rposition(|&r| r as f32 <= index)
                .filter(|_| (index as usize) < view.view.rows.len())
        });
        let Some(file) = file else { return };
        // とびとびに選んだときの見出しは「パス — 短い ID」。
        let text = view.text(&view.view.rows[view.file_rows[file] as usize]);
        let path = text.split(" — ").next().unwrap_or(text);
        f(header, path, x, y);
    }

    /// 見出しの Viewed を押したら `f(ファイルの番号)` を呼ぶ（呼ぶ先があるときだけ Viewed を出す）。
    pub fn connect_toggle_viewed(&self, f: impl Fn(usize) + 'static) {
        self.state().on_toggle_viewed = Some(Rc::new(f));
        self.queue_draw();
    }

    /// 一番上に見えているファイルが変わったら `f(ファイルの番号)` を呼ぶ。
    pub fn connect_top_file(&self, f: impl Fn(usize) + 'static) {
        self.state().on_top_file = Some(Rc::new(f));
    }

    /// Viewed にしたファイルを替える。
    pub fn set_viewed(&self, viewed: HashSet<usize>) {
        self.state().viewed = viewed;
        self.queue_draw();
    }

    /// ファイル `file` を畳んでいるか。
    pub fn file_collapsed(&self, file: usize) -> bool {
        self.state().collapsed.get(file).copied().unwrap_or(false)
    }

    /// 上に張り付かせる見出し（ファイルの番号と、見えている範囲の上端からの位置）。見出しの行が見えている範囲より
    /// 上にあるファイルのものを出し、次のファイルの見出しが近づいたら押し上げる（macOS 版と同じ）。
    fn sticky(view: &View, scroll: f32, rh: f32) -> Option<(usize, f32)> {
        let first = (scroll / rh).floor().max(0.0) as u32;
        let file = view.file_rows.iter().rposition(|&r| r <= first)?;
        if view.file_rows[file] as f32 * rh >= scroll {
            return None;
        }
        let top = view
            .file_rows
            .get(file + 1)
            .map_or(0.0, |&next| (next as f32 * rh - scroll - rh).min(0.0));
        Some((file, top))
    }

    /// `y`（部品の中の位置）が、ファイルの見出し（上に張り付いたものを含む）なら、そのファイルの番号。
    fn header_at(&self, y: f32) -> Option<usize> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let rh = self.row_height();
        let view = self.state().view.clone()?;
        if let Some((file, top)) = Self::sticky(&view, scroll, rh)
            && y >= top
            && y < top + rh
        {
            return Some(file);
        }
        let row = ((y + scroll) / rh).floor();
        if row < 0.0 {
            return None;
        }
        view.file_rows.iter().position(|&r| r == row as u32)
    }

    /// 見出しの Viewed のチェックボックスの左端。
    fn viewed_x(&self) -> f32 {
        self.width() as f32 - 76.0
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
    fn file_rows(&self) -> Vec<u32> {
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
        (scroll / self.row_height()).floor().max(0.0) as u32
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
        let rh = self.row_height() as f64;
        let (top, bottom) = (start as f64 * rh, end as f64 * rh);
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
        let top = self.top_row();
        let current = files.iter().rposition(|&r| r <= top).unwrap_or(0) as i64;
        let target = current + step as i64;
        if target < 0 || target as usize >= files.len() {
            return self.error_bell();
        }
        self.scroll_to_file(target as usize);
    }

    /// `index` 番目のファイルの見出しが一番上に来るようにスクロールする。
    pub fn scroll_to_file(&self, index: usize) {
        let files = self.file_rows();
        let (Some(&row), Some(adj)) = (files.get(index), self.vadjustment()) else {
            return;
        };
        let y = row as f64 * self.row_height() as f64;
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
        self.state()
            .view
            .as_ref()
            .map_or(0, |v| v.view.rows.len() as u32)
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
    }

    /// 本文の左端（印・行番号の列の右。行番号の列の幅は、等幅なので桁数から）。行番号と印は横にスクロールしない。
    fn text_left(st: &State, view: &View) -> f32 {
        let left = if st.checks {
            TEXT_INSET + MARK_WIDTH
        } else {
            TEXT_INSET
        };
        left + st.char_width * (view.digits * 2 + 1) as f32 + GUTTER_GAP
    }

    /// 横にスクロールした位置。
    fn scroll_x(&self) -> f32 {
        self.hadjustment().map_or(0.0, |a| a.value()) as f32
    }

    fn configure_adjustments(&self) {
        let (width, height) = (self.width() as f64, self.height() as f64);
        let upper = (self.row_count() as f64 * self.row_height() as f64 + 8.0).max(height);
        if let Some(adj) = self.vadjustment() {
            let value = adj.value().clamp(0.0, (upper - height).max(0.0));
            let step = self.row_height() as f64 * 3.0;
            adj.configure(value, 0.0, upper, step, height * 0.9, height);
        }
        // 横は一番長い行まで（折り返さない。macOS 版と同じ）。
        let (content, step) = {
            let mut st = self.state();
            self.ensure_font(&mut st);
            let content = st.view.clone().map_or(0.0, |view| {
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
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let rh = self.row_height();
        let index = ((y + scroll) / rh).floor();
        let (marks, checks, view) = {
            let mut st = self.state();
            st.dragging = false;
            (st.marks, st.checks, st.view.clone())
        };
        let Some(view) = view else { return };
        if !marks {
            let Some(file) = self.header_at(y) else {
                self.begin_text_selection(x, y, shift, clicks);
                return;
            };
            let on_viewed = self.state().on_toggle_viewed.clone();
            match on_viewed {
                Some(f) if x >= self.viewed_x() - 4.0 => f(file),
                _ => self.set_file_collapsed(file, !self.file_collapsed(file)),
            }
            return;
        }
        if index < 0.0 || index as usize >= view.view.rows.len() {
            return;
        }
        let index = index as u32;
        let rows = &view.view.rows;
        let row = rows[index as usize];
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
        self.select_rows(&view, index, shift, ctrl);
    }

    /// `(x, y)`（部品の中の位置）の文字の位置。上にはみ出せば先頭、下にはみ出せば末尾。
    fn text_position(&self, x: f32, y: f32) -> Option<TextPos> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let rh = self.row_height();
        let (view, left) = {
            let st = self.state();
            let view = st.view.clone()?;
            let left = Self::text_left(&st, &view);
            (view, left)
        };
        let rows = &view.view.rows;
        let last = rows.len().checked_sub(1)?;
        if y + scroll < 0.0 {
            return Some((0, 0));
        }
        let row = ((y + scroll) / rh).floor() as usize;
        if row > last {
            return Some((last as u32, view.text(&rows[last]).len() as u32));
        }
        if rows[row].kind == DiffRow::FILE {
            return Some((row as u32, 0));
        }
        let layout = self.layout_of(row as u32)?;
        let offset = x - left + self.scroll_x();
        if offset <= 0.0 {
            return Some((row as u32, 0));
        }
        let (_, index, trailing) = layout.xy_to_index((offset * pango::SCALE as f32) as i32, 0);
        let text = view.text(&rows[row]);
        let mut index = (index.max(0) as usize).min(text.len());
        // 文字の後ろ半分なら、その文字の後ろ。
        for _ in 0..trailing {
            index += text[index..].chars().next().map_or(0, char::len_utf8);
        }
        Some((row as u32, index as u32))
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
            let Ok(view) = gio::spawn_blocking(move || View::build(&source.0, &all)).await else {
                return;
            };
            let Some(widget) = weak.upgrade() else { return };
            let row = {
                let mut st = widget.state();
                if st.generation != generation {
                    return;
                }
                let row = view.file_rows.get(file).copied();
                st.view = Some(Arc::new(view));
                st.rows.clear();
                st.text_selection = None;
                row
            };
            widget.configure_adjustments();
            if let (Some(row), Some(adj)) = (row, widget.vadjustment()) {
                let top = row as f64 * widget.row_height() as f64;
                if top < adj.value() {
                    adj.set_value(top);
                }
            }
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
            // 見出し: 開閉の印とパス（行数と Viewed は `draw_header` で右に描く）。
            let mark = if row.flags & DiffRow::COLLAPSED != 0 {
                "▸"
            } else {
                "▾"
            };
            let path = view.text(row);
            let text = format!("{mark} {path}");
            let l = layout(&text);
            let attrs = pango::AttrList::new();
            let mut bold = pango::AttrInt::new_weight(pango::Weight::Bold);
            bold.set_start_index((mark.len() + 1) as u32);
            bold.set_end_index(text.len() as u32);
            attrs.insert(bold);
            l.set_attributes(Some(&attrs));
            return RowText {
                gutter: None,
                text: l,
            };
        }
        let gutter = (row.kind != DiffRow::HUNK).then(|| {
            let n = |v: u32| {
                if v == 0 { String::new() } else { v.to_string() }
            };
            let w = view.digits;
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
            let (r, g, b) = if row.kind == DiffRow::ADDED {
                (0.2, 0.7, 0.3)
            } else {
                (0.9, 0.25, 0.25)
            };
            for &(start, end) in ranges {
                let mut a = pango::AttrColor::new_background(
                    (r * 65535.0) as u16,
                    (g * 65535.0) as u16,
                    (b * 65535.0) as u16,
                );
                a.set_start_index(start);
                a.set_end_index(end);
                attrs.insert(a);
                let mut alpha = pango::AttrInt::new_background_alpha((0.35 * 65535.0) as u16);
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
        let viewed_x = self.viewed_x();
        let mut st = self.state();
        let st = &mut *st;
        self.ensure_font(st);
        // 差分の代わりの知らせ: 頼まれたもの、1 ファイルのバイナリ、変わったファイルのない差分（macOS 版と同じ）。
        let message = st.message.clone().or_else(|| {
            let view = st.view.as_ref()?;
            if view.view.binary && view.file_rows.is_empty() {
                Some(tr!("Binary file"))
            } else if view.view.rows.is_empty()
                && st.source.as_ref().is_some_and(|s| s.0.files.is_empty())
            {
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
        let first = ((scroll_y / rh).floor().max(0.0) as u32).min(count);
        let last = (((scroll_y + height) / rh).ceil() as u32 + 1).min(count);
        if st.rows.len() > 4096 {
            st.rows.clear();
        }
        let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.5);
        let left = if st.checks {
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
            if !st.rows.contains_key(&index) {
                let text = self.row_text(st, &view, index);
                st.rows.insert(index, text);
            }
            let row = &view.view.rows[index as usize];
            let top = index as f32 * rh - scroll_y;
            if row.kind == DiffRow::FILE {
                self.draw_header(snapshot, st, &view, index, top, width, viewed_x, false);
                continue;
            }
            // 選んだ行は行番号の欄だけを選んだ色で塗り、行の追加・削除の色はそのまま見せる（macOS 版と同じ）。
            if st.selection.contains(&index) {
                snapshot.append_color(
                    &gdk::RGBA::new(0.21, 0.52, 0.89, 0.35),
                    &graphene::Rect::new(0.0, top, (text_x - 6.0).max(0.0), rh),
                );
            }
            let background = match row.kind {
                DiffRow::ADDED => Some(gdk::RGBA::new(0.2, 0.7, 0.3, 0.16)),
                DiffRow::REMOVED => Some(gdk::RGBA::new(0.9, 0.25, 0.25, 0.16)),
                DiffRow::HUNK => Some(gdk::RGBA::new(0.3, 0.5, 0.9, 0.10)),
                // コンフリクト: Current（自分の側）・Incoming（取り込む側）・共通の祖先・マーカー。
                DiffRow::OURS => Some(gdk::RGBA::new(0.2, 0.55, 0.9, 0.16)),
                DiffRow::THEIRS => Some(gdk::RGBA::new(0.6, 0.4, 0.85, 0.16)),
                DiffRow::BASE => Some(gdk::RGBA::new(0.5, 0.5, 0.5, 0.10)),
                DiffRow::MARKER => Some(gdk::RGBA::new(0.5, 0.5, 0.5, 0.22)),
                _ => None,
            };
            if let Some(bg) = background {
                snapshot.append_color(&bg, &graphene::Rect::new(0.0, top, width, rh));
            }
            let line = &st.rows[&index];
            let (_, h) = line.text.pixel_size();
            let y = top + ((rh - h as f32) / 2.0).round();
            // 変更の行と hunk の見出しのチェックボックス。hunk は中の変更がすべて含まれていれば ✓、一部なら −
            // （macOS 版と同じ）。
            if st.checks {
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
            snapshot.push_clip(&graphene::Rect::new(
                text_x,
                top,
                (width - text_x).max(0.0),
                rh,
            ));
            if let Some((start, end)) = text_range
                && start.0 <= index
                && index <= end.0
            {
                let x = |i: u32| line.text.index_to_pos(i as i32).x() as f32 / pango::SCALE as f32;
                let x0 = if index == start.0 { x(start.1) } else { 0.0 };
                // 次の行に続くときは、改行の分として少し右まで塗る。
                let x1 = if index == end.0 {
                    x(end.1)
                } else {
                    line.text.pixel_size().0 as f32 + 6.0
                };
                if x1 > x0 {
                    snapshot.append_color(
                        &selected_color,
                        &graphene::Rect::new(origin + x0, top, x1 - x0, rh),
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
                let x0 = line.text.index_to_pos(m.1 as i32).x() as f32 / pango::SCALE as f32;
                let x1 = line.text.index_to_pos(m.2 as i32).x() as f32 / pango::SCALE as f32;
                let color = if st.current_match == Some(match_from + i) {
                    gdk::RGBA::new(1.0, 0.6, 0.0, 0.75)
                } else {
                    gdk::RGBA::new(1.0, 0.85, 0.0, 0.45)
                };
                snapshot.append_color(
                    &color,
                    &graphene::Rect::new(origin + x0, top + 1.0, (x1 - x0).max(2.0), rh - 2.0),
                );
            }
            let color = if row.kind == DiffRow::HUNK {
                &secondary
            } else {
                &fg
            };
            snapshot.save();
            snapshot.translate(&graphene::Point::new(origin, y));
            snapshot.append_layout(&line.text, color);
            snapshot.restore();
            snapshot.pop();
        }
        // 見出しの行が上に隠れたファイルは、見出しを上に張り付かせる。
        if let Some((file, top)) = Self::sticky(&view, scroll_y, rh) {
            let index = view.file_rows[file];
            if !st.rows.contains_key(&index) {
                let text = self.row_text(st, &view, index);
                st.rows.insert(index, text);
            }
            self.draw_header(snapshot, st, &view, index, top, width, viewed_x, true);
        }
        // 一番上に見えているファイル（一覧の選択を合わせる）。
        if !view.file_rows.is_empty() {
            let file = view
                .file_rows
                .iter()
                .rposition(|&r| r as f32 * rh <= scroll_y + rh / 2.0)
                .unwrap_or(0);
            if st.top_file != Some(file) {
                st.top_file = Some(file);
                if let Some(f) = st.on_top_file.clone() {
                    glib::idle_add_local_once(move || f(file));
                }
            }
        }
    }

    /// ファイルの見出しの行: 開閉の印とパス、右に追加・削除の行数と Viewed のチェックボックス（Viewed を扱う
    /// ときだけ）。`opaque` なら下の行を隠す（上に張り付かせるとき）。
    #[allow(clippy::too_many_arguments)]
    fn draw_header(
        &self,
        snapshot: &gtk::Snapshot,
        st: &State,
        view: &View,
        index: u32,
        top: f32,
        width: f32,
        viewed_x: f32,
        opaque: bool,
    ) {
        let rh = st.row_height;
        let fg = self.color();
        let dark = fg.red() > 0.5;
        if opaque {
            let base = if dark {
                gdk::RGBA::new(0.14, 0.14, 0.15, 1.0)
            } else {
                gdk::RGBA::WHITE
            };
            snapshot.append_color(&base, &graphene::Rect::new(0.0, top, width, rh));
        }
        snapshot.append_color(
            &gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.08),
            &graphene::Rect::new(0.0, top, width, rh),
        );
        snapshot.append_color(
            &gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.15),
            &graphene::Rect::new(0.0, top + rh - 1.0, width, 1.0),
        );
        let row = &view.view.rows[index as usize];
        let file = view.file_rows.iter().position(|&r| r == index).unwrap_or(0);
        let layout = |text: &str| {
            let l = self.create_pango_layout(Some(text));
            l.set_font_description(st.font.as_ref());
            l
        };
        // 右から: Viewed、行数。
        let show_viewed = st.on_toggle_viewed.is_some();
        let right = if show_viewed { viewed_x } else { width - 4.0 };
        let counts: Vec<(pango::Layout, gdk::RGBA)> = if row.flags & DiffRow::BINARY != 0 {
            vec![(layout(&tr!("Binary")), fg)]
        } else {
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
            vec![
                (layout(&format!("+{}", row.new_no)), green),
                (layout(&format!("−{}", row.old_no)), red),
            ]
        };
        let counts_width: f32 = counts
            .iter()
            .map(|(l, _)| l.pixel_size().0 as f32)
            .sum::<f32>()
            + 8.0 * (counts.len() as f32 - 1.0);
        let counts_x = right - 14.0 - counts_width;
        let line = &st.rows[&index];
        let (_, h) = line.text.pixel_size();
        let y = top + ((rh - h as f32) / 2.0).round();
        let left = if st.checks {
            TEXT_INSET + MARK_WIDTH
        } else {
            TEXT_INSET
        };
        // パスは行数の手前までに収める。
        snapshot.save();
        snapshot.push_clip(&graphene::Rect::new(
            left,
            top,
            (counts_x - 8.0 - left).max(0.0),
            rh,
        ));
        snapshot.translate(&graphene::Point::new(left, y));
        // パスの中の、探している語の一致（今の一致は濃く）。
        let from = st.matches.partition_point(|m| m.0 < index);
        for (i, m) in st.matches[from..]
            .iter()
            .enumerate()
            .take_while(|(_, m)| m.0 == index)
        {
            let x0 = line.text.index_to_pos(m.1 as i32).x() as f32 / pango::SCALE as f32;
            let x1 = line.text.index_to_pos(m.2 as i32).x() as f32 / pango::SCALE as f32;
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
        snapshot.append_layout(&line.text, &fg);
        snapshot.pop();
        snapshot.restore();
        let mut x = counts_x;
        for (l, color) in &counts {
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x, on_baseline(y, &line.text, l)));
            snapshot.append_layout(l, color);
            snapshot.restore();
            x += l.pixel_size().0 as f32 + 8.0;
        }
        if show_viewed {
            let size = 12.0;
            let rect = graphene::Rect::new(viewed_x, top + (rh - size) / 2.0, size, size);
            let path = gsk::PathBuilder::new();
            path.add_rounded_rect(&gsk::RoundedRect::from_rect(rect, 3.0));
            let path = path.to_path();
            if st.viewed.contains(&file) {
                snapshot.append_fill(
                    &path,
                    gsk::FillRule::Winding,
                    &gdk::RGBA::new(0.21, 0.52, 0.89, 1.0),
                );
                let check = gsk::PathBuilder::new();
                check.move_to(rect.x() + size * 0.22, rect.y() + size * 0.52);
                check.line_to(rect.x() + size * 0.42, rect.y() + size * 0.72);
                check.line_to(rect.x() + size * 0.78, rect.y() + size * 0.3);
                let stroke = gsk::Stroke::new(1.6);
                stroke.set_line_cap(gsk::LineCap::Round);
                snapshot.append_stroke(&check.to_path(), &stroke, &gdk::RGBA::WHITE);
            } else {
                let color = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.5);
                snapshot.append_stroke(&path, &gsk::Stroke::new(1.0), &color);
            }
            let label = layout("Viewed");
            let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.6);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                viewed_x + size + 5.0,
                on_baseline(y, &line.text, &label),
            ));
            snapshot.append_layout(&label, &secondary);
            snapshot.restore();
        }
    }
}

/// `main` を `y` に置いた行で、`other` を置く高さ（文字の下端の線を `main` に揃える。字の違いで上下にずれない）。
fn on_baseline(y: f32, main: &pango::Layout, other: &pango::Layout) -> f32 {
    y + (main.baseline() - other.baseline()) as f32 / pango::SCALE as f32
}

/// `text` の `index`（バイト位置）の文字を含む単語の範囲（ダブルクリック）。英数字と `_` の並び、空白の並び、
/// それ以外は 1 文字。
fn word_range(text: &str, index: usize) -> (usize, usize) {
    let index = index.min(text.len());
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

/// `text` の中の `query` の位置（バイト）。大文字・小文字は区別しない（小文字にして長さが変わる文字があれば、
/// 区別して探す）。
fn find_all(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let (hay, needle) = (text.to_lowercase(), query.to_lowercase());
    let (hay, needle) = if hay.len() == text.len() && needle.len() == query.len() {
        (hay, needle)
    } else {
        (text.to_owned(), query.to_owned())
    };
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

/// 構文の種類ごとの色（macOS 版の `SyntaxColors` と同じ割り当て）。`None` は本文の色のまま。
pub(crate) fn syntax_color(kind: u8, fg: &gdk::RGBA) -> Option<(f32, f32, f32)> {
    let dark = fg.red() > 0.5;
    let pick =
        |light: (f32, f32, f32), dark_: (f32, f32, f32)| Some(if dark { dark_ } else { light });
    let pink = pick((0.80, 0.18, 0.45), (1.0, 0.45, 0.65));
    let red = pick((0.77, 0.16, 0.16), (1.0, 0.45, 0.40));
    let gray = pick((0.45, 0.45, 0.48), (0.60, 0.60, 0.64));
    let purple = pick((0.50, 0.25, 0.75), (0.78, 0.55, 1.0));
    let blue = pick((0.10, 0.40, 0.85), (0.40, 0.65, 1.0));
    let teal = pick((0.10, 0.55, 0.60), (0.35, 0.80, 0.85));
    let cyan = pick((0.10, 0.50, 0.70), (0.40, 0.80, 0.95));
    let orange = pick((0.80, 0.45, 0.05), (1.0, 0.65, 0.30));
    let brown = pick((0.55, 0.40, 0.25), (0.80, 0.65, 0.45));
    match kind {
        SyntaxSpan::KEYWORD | SyntaxSpan::TAG | SyntaxSpan::VARIABLE => pink,
        SyntaxSpan::STRING => red,
        SyntaxSpan::COMMENT => gray,
        SyntaxSpan::NUMBER | SyntaxSpan::CONSTANT => purple,
        SyntaxSpan::FUNCTION => blue,
        SyntaxSpan::TYPE => teal,
        SyntaxSpan::PROPERTY => cyan,
        SyntaxSpan::ATTRIBUTE | SyntaxSpan::ESCAPE => orange,
        SyntaxSpan::MODULE => brown,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
