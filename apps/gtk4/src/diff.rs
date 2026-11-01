//! 差分の表示。コミットの全ファイルを、ファイルの見出し・hunk の見出し・行の順に並べる（`StackedDiff`）。
//!
//! History と同じく、見えている行だけを 1 つのウィジェットの `snapshot` で描く。変わった単語の強調と構文の
//! 色は、コアが FE の符号単位（UTF-8 のバイト位置）で渡すので、そのまま Pango の属性にする。
//! ファイルの見出しをクリックすると、そのファイルを畳む・開く。
//!
//! Working Copy では、変更の行をクリック（Shift で範囲、Ctrl で追加）して選び、行の左の印で
//! 次のコミットに含めるか（index に入っているか）を見せる。印のクリックでその行を含める・外す。

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
        Self {
            highlights,
            syntax,
            digits: max.to_string().len().max(3),
            file_rows,
            view,
        }
    }

    fn text(&self, row: &DiffRow) -> &str {
        std::str::from_utf8(self.view.strings.bytes(row.text)).unwrap_or("")
    }
}

/// 差分の元（ファイルを畳んだとき、ここから作り直す）。
pub struct Source(pub StackedDiff);

// SAFETY: `StackedDiff` の中身は共有の不変なバッファ（`Blob`）と、自分で持つ文字列と行の範囲だけ。
unsafe impl Send for Source {}
unsafe impl Sync for Source {}

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
    /// 作り直している途中の表示の世代（古い結果を捨てる）。
    generation: u64,
    /// 行ごとの stage の印を出す（Working Copy）。
    marks: bool,
    /// 選んだ変更の行。
    selection: BTreeSet<u32>,
    /// Shift で範囲を選ぶときの起点。
    anchor: Option<u32>,
    /// 印をクリックしたときに呼ぶ（行）。
    on_mark: Option<Rc<dyn Fn(u32)>>,
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
            click.connect_pressed(move |gesture, _, x, y| {
                if let Some(view) = weak.upgrade() {
                    view.grab_focus();
                    let state = gesture.current_event_state();
                    view.clicked(
                        x as f32,
                        y as f32,
                        state.contains(gdk::ModifierType::SHIFT_MASK),
                        state.contains(gdk::ModifierType::CONTROL_MASK),
                    );
                }
            });
            view.add_controller(click);
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
            // Space（選んだ行を含める・外す）、J・K（次・前の hunk）、⌥J・⌥K（次・前のファイル）。
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
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }

    impl ScrollableImpl for DiffWidget {}

    impl DiffWidget {
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
            st.source = None;
            st.view = None;
            st.rows.clear();
            st.selection.clear();
            st.anchor = None;
            st.hunk_cursor = None;
        }
        self.configure_adjustments();
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
            st.collapsed = collapsed;
            st.viewed = viewed;
            st.top_file = None;
            st.source = Some(source);
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = false;
            st.selection.clear();
            st.anchor = None;
            st.hunk_cursor = None;
        }
        if let Some(adj) = self.vadjustment() {
            adj.set_value(0.0);
        }
        self.configure_adjustments();
        self.refresh_matches();
        self.queue_draw();
        self.selection_changed();
    }

    /// Working Copy の 1 ファイルの差分を、行ごとの stage の印つきで出す。`keep` なら同じファイルを
    /// 読み直したとして、スクロールの位置を保つ。選んだ行はいつも外す（macOS 版と同じ。読み直すと行の番号が
    /// 別の行を指すことがあり、そのまま Stage Lines・Discard Lines を押すと違う行に効いてしまう）。
    pub fn show_working(&self, view: View, keep: bool) {
        {
            let mut st = self.state();
            st.generation += 1;
            st.source = None;
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = true;
            st.selection.clear();
            st.anchor = None;
            if !keep {
                st.hunk_after = None;
            }
            st.hunk_cursor = None;
        }
        if !keep && let Some(adj) = self.vadjustment() {
            adj.set_value(0.0);
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
            st.source = None;
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = false;
            st.selection.clear();
            st.anchor = None;
            st.hunk_cursor = None;
        }
        if let Some(adj) = self.vadjustment() {
            adj.set_value(0.0);
        }
        self.configure_adjustments();
        self.refresh_matches();
        self.queue_draw();
        self.selection_changed();
    }

    /// 選んだ変更の行（表示の行の番号。Working Copy の差分では、コアの差分の行の番号と同じ）。
    pub fn selected_rows(&self) -> Vec<usize> {
        self.state().selection.iter().map(|&r| r as usize).collect()
    }

    /// 選んだ行のうち、次のコミットに含めているものの数と、全体の数。
    pub fn selected_staged(&self) -> (usize, usize) {
        let st = self.state();
        let Some(view) = &st.view else { return (0, 0) };
        let staged = st
            .selection
            .iter()
            .filter(|&&r| view.view.rows[r as usize].flags & DiffRow::STAGED != 0)
            .count();
        (staged, st.selection.len())
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
        match key.to_lower() {
            gdk::Key::space if plain => {
                let (marks, rows, f) = {
                    let st = self.state();
                    (
                        st.marks,
                        st.selection.iter().map(|&r| r as usize).collect::<Vec<_>>(),
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

    /// `index` 番目の hunk を選び（選べる差分なら、その変更の行）、見えるところへスクロールする。
    pub fn select_hunk(&self, index: usize) {
        let hunks = self.hunk_rows();
        let Some(&start) = hunks.get(index) else {
            return;
        };
        let end = hunks.get(index + 1).copied().unwrap_or(self.row_count());
        {
            let mut st = self.state();
            st.hunk_cursor = Some(index);
            if st.marks
                && let Some(view) = st.view.clone()
            {
                st.selection = (start..end)
                    .filter(|&r| Self::is_change(&view.view.rows[r as usize]))
                    .collect();
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

    pub fn connect_mark(&self, f: impl Fn(u32) + 'static) {
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
        st.font = Some(font);
    }

    fn configure_adjustments(&self) {
        let (width, height) = (self.width() as f64, self.height() as f64);
        let upper = (self.row_count() as f64 * self.row_height() as f64 + 8.0).max(height);
        if let Some(adj) = self.vadjustment() {
            let value = adj.value().clamp(0.0, (upper - height).max(0.0));
            let step = self.row_height() as f64 * 3.0;
            adj.configure(value, 0.0, upper, step, height * 0.9, height);
        }
        if let Some(adj) = self.hadjustment() {
            adj.configure(0.0, 0.0, width, 10.0, width, width);
        }
    }

    /// ファイルの見出しなら畳む・開く。Working Copy では印なら含める・外す、ほかは行を選ぶ。
    fn clicked(&self, x: f32, y: f32, shift: bool, ctrl: bool) {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let rh = self.row_height();
        let index = ((y + scroll) / rh).floor();
        let (marks, view) = {
            let st = self.state();
            (st.marks, st.view.clone())
        };
        let Some(view) = view else { return };
        if index < 0.0 || index as usize >= view.view.rows.len() {
            return;
        }
        let index = index as u32;
        let row = view.view.rows[index as usize];
        if marks {
            if Self::is_change(&row) && x < TEXT_INSET + MARK_WIDTH {
                let f = self.state().on_mark.clone();
                if let Some(f) = f {
                    f(index);
                }
                return;
            }
            self.select_rows(&view, index, shift, ctrl);
            return;
        }
        let Some(file) = self.header_at(y) else {
            return;
        };
        let on_viewed = self.state().on_toggle_viewed.clone();
        match on_viewed {
            Some(f) if x >= self.viewed_x() - 4.0 => f(file),
            _ => self.set_file_collapsed(file, !self.file_collapsed(file)),
        }
    }

    /// 行を選ぶ。hunk の見出しなら、その hunk の変更の行をすべて。
    fn select_rows(&self, view: &View, index: u32, shift: bool, ctrl: bool) {
        let rows = &view.view.rows;
        let picked: Vec<u32> = if rows[index as usize].kind == DiffRow::HUNK {
            (index + 1..rows.len() as u32)
                .take_while(|&r| rows[r as usize].kind != DiffRow::HUNK)
                .filter(|&r| Self::is_change(&rows[r as usize]))
                .collect()
        } else if Self::is_change(&rows[index as usize]) {
            vec![index]
        } else {
            Vec::new()
        };
        {
            let mut st = self.state();
            match (shift, st.anchor) {
                (true, Some(anchor)) => {
                    let (a, b) = (anchor.min(index), anchor.max(index));
                    if !ctrl {
                        st.selection.clear();
                    }
                    st.selection
                        .extend((a..=b).filter(|&r| Self::is_change(&rows[r as usize])));
                }
                _ if ctrl => {
                    for r in &picked {
                        if !st.selection.remove(r) {
                            st.selection.insert(*r);
                        }
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
        self.reveal_match();
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
        for (i, row) in view.view.rows.iter().enumerate() {
            if row.kind == DiffRow::FILE {
                continue;
            }
            for (start, end) in find_all(view.text(row), &query) {
                st.matches.push((i as u32, start as u32, end as u32));
            }
        }
        st.current_match = previous.and_then(|p| st.matches.iter().position(|m| *m == p));
    }

    /// 今の一致が見える位置までスクロールする。
    fn reveal_match(&self) {
        let row = {
            let st = self.state();
            st.current_match
                .and_then(|c| st.matches.get(c))
                .map(|m| m.0)
        };
        if let Some(row) = row {
            self.reveal_rows(row, row + 1);
        }
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
        let fg = self.color();
        let viewed_x = self.viewed_x();
        let mut st = self.state();
        let st = &mut *st;
        self.ensure_font(st);
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
        // 行番号の列の幅（等幅なので桁数から）。
        let gutter_width = {
            let probe = self.create_pango_layout(Some(&"0".repeat(view.digits * 2 + 1)));
            probe.set_font_description(st.font.as_ref());
            probe.pixel_size().0 as f32
        };
        let left = if st.marks {
            TEXT_INSET + MARK_WIDTH
        } else {
            TEXT_INSET
        };
        // 一致の行の範囲（一致は行の順に並んでいる）。
        let match_from = st.matches.partition_point(|m| m.0 < first);
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
            if st.selection.contains(&index) {
                snapshot.append_color(
                    &gdk::RGBA::new(0.21, 0.52, 0.89, 0.28),
                    &graphene::Rect::new(0.0, top, width, rh),
                );
            }
            if st.marks && Self::is_change(row) {
                draw_mark(
                    snapshot,
                    TEXT_INSET,
                    top,
                    rh,
                    row.flags & DiffRow::STAGED != 0,
                    &fg,
                );
            }
            let text_x = left + gutter_width + GUTTER_GAP;
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
                    &graphene::Rect::new(text_x + x0, top + 1.0, (x1 - x0).max(2.0), rh - 2.0),
                );
            }
            if let Some(gutter) = &line.gutter {
                snapshot.save();
                snapshot.translate(&graphene::Point::new(left, y));
                snapshot.append_layout(gutter, &secondary);
                snapshot.restore();
            }
            let color = if row.kind == DiffRow::HUNK {
                &secondary
            } else {
                &fg
            };
            snapshot.save();
            snapshot.push_clip(&graphene::Rect::new(
                text_x,
                top,
                (width - text_x).max(0.0),
                rh,
            ));
            snapshot.translate(&graphene::Point::new(text_x, y));
            snapshot.append_layout(&line.text, color);
            snapshot.pop();
            snapshot.restore();
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
        let left = if st.marks {
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
        snapshot.append_layout(&line.text, &fg);
        snapshot.pop();
        snapshot.restore();
        let mut x = counts_x;
        for (l, color) in &counts {
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x, y));
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
            snapshot.translate(&graphene::Point::new(viewed_x + size + 5.0, y));
            snapshot.append_layout(&label, &secondary);
            snapshot.restore();
        }
    }
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
fn draw_mark(snapshot: &gtk::Snapshot, x: f32, top: f32, rh: f32, staged: bool, fg: &gdk::RGBA) {
    let size = (rh - 6.0).clamp(8.0, 12.0);
    let rect = graphene::Rect::new(x + 2.0, top + (rh - size) / 2.0, size, size);
    let path = gsk::PathBuilder::new();
    path.add_rounded_rect(&gsk::RoundedRect::from_rect(rect, 3.0));
    let path = path.to_path();
    if staged {
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
