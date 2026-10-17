//! 差分の表示。コミットの全ファイルを、ファイルの見出し・hunk の見出し・行の順に並べる（`StackedDiff`）。
//!
//! History と同じく、見えている行だけを 1 つのウィジェットの `snapshot` で描く。変わった単語の強調と構文の
//! 色は、コアが FE の符号単位（UTF-8 のバイト位置）で渡すので、そのまま Pango の属性にする。
//! ファイルの見出しをクリックすると、そのファイルを畳む・開く。
//!
//! Working Copy では、変更の行をクリック（Shift で範囲、Ctrl で追加）して選び、行の左の印で
//! 次のコミットに含めるか（index に入っているか）を見せる。印のクリックでその行を含める・外す。

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::diff::{DiffView, StackedDiff};
use amonhen_layout::{DiffRow, SyntaxSpan};
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
        Self {
            highlights,
            syntax,
            digits: max.to_string().len().max(3),
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
        }
        self.configure_adjustments();
        self.queue_draw();
    }

    /// 新しい差分を出す（先頭から）。
    pub fn show(&self, source: Arc<Source>, view: View) {
        {
            let mut st = self.state();
            st.generation += 1;
            st.collapsed = vec![false; source.0.files.len()];
            st.source = Some(source);
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = false;
            st.selection.clear();
            st.anchor = None;
        }
        if let Some(adj) = self.vadjustment() {
            adj.set_value(0.0);
        }
        self.configure_adjustments();
        self.queue_draw();
        self.selection_changed();
    }

    /// Working Copy の 1 ファイルの差分を、行ごとの stage の印つきで出す。`keep` なら同じファイルを
    /// 読み直したとして、スクロールの位置と選んだ行を保つ。
    pub fn show_working(&self, view: View, keep: bool) {
        {
            let mut st = self.state();
            st.generation += 1;
            st.source = None;
            let rows = view.view.rows.len() as u32;
            st.view = Some(Arc::new(view));
            st.rows.clear();
            st.marks = true;
            if keep {
                st.selection.retain(|&r| r < rows);
            } else {
                st.selection.clear();
                st.anchor = None;
            }
        }
        if !keep && let Some(adj) = self.vadjustment() {
            adj.set_value(0.0);
        }
        self.configure_adjustments();
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
        self.toggle_file(y);
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

    /// ファイルの見出しをクリックしたら、そのファイルを畳む・開く。
    fn toggle_file(&self, y: f32) {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let rh = self.row_height();
        let (source, collapsed, generation) = {
            let mut st = self.state();
            let Some(view) = st.view.clone() else { return };
            let row = ((y + scroll) / rh).floor();
            if row < 0.0 {
                return;
            }
            let Some(target) = view.view.rows.get(row as usize) else {
                return;
            };
            if target.kind != DiffRow::FILE {
                return;
            }
            // 何番目のファイルか（見出しの行を数える）。
            let index = view.view.rows[..row as usize]
                .iter()
                .filter(|r| r.kind == DiffRow::FILE)
                .count();
            let Some(source) = st.source.clone() else {
                return;
            };
            if let Some(c) = st.collapsed.get_mut(index) {
                *c = !*c;
            }
            st.generation += 1;
            (source, st.collapsed.clone(), st.generation)
        };
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(view) = gio::spawn_blocking(move || View::build(&source.0, &collapsed)).await
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
            }
            widget.configure_adjustments();
            widget.queue_draw();
        });
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
            // 見出し: 開閉の印、パス、行数。
            let mark = if row.flags & DiffRow::COLLAPSED != 0 {
                "▸"
            } else {
                "▾"
            };
            let path = view.text(row);
            let counts = if row.flags & DiffRow::BINARY != 0 {
                "バイナリ".to_owned()
            } else {
                format!("+{}  −{}", row.new_no, row.old_no)
            };
            let text = format!("{mark} {path}   {counts}");
            let l = layout(&text);
            let attrs = pango::AttrList::new();
            let mut bold = pango::AttrInt::new_weight(pango::Weight::Bold);
            bold.set_start_index(0);
            bold.set_end_index((mark.len() + 1 + path.len()) as u32);
            attrs.insert(bold);
            let counts_at = (text.len() - counts.len()) as u32;
            if row.flags & DiffRow::BINARY == 0 {
                let plus_end = counts_at + format!("+{}", row.new_no).len() as u32;
                let mut green = rgb_attr((0.2, 0.6, 0.3));
                green.set_start_index(counts_at);
                green.set_end_index(plus_end);
                attrs.insert(green);
                let mut red = rgb_attr((0.85, 0.25, 0.25));
                red.set_start_index(plus_end + 2);
                red.set_end_index(text.len() as u32);
                attrs.insert(red);
            }
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
            layout(&format!("{:>w$} {:>w$}", n(row.old_no), n(row.new_no)))
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
        for index in first..last {
            if !st.rows.contains_key(&index) {
                let text = self.row_text(st, &view, index);
                st.rows.insert(index, text);
            }
            let row = &view.view.rows[index as usize];
            let top = index as f32 * rh - scroll_y;
            let background = match row.kind {
                DiffRow::ADDED => Some(gdk::RGBA::new(0.2, 0.7, 0.3, 0.16)),
                DiffRow::REMOVED => Some(gdk::RGBA::new(0.9, 0.25, 0.25, 0.16)),
                DiffRow::HUNK => Some(gdk::RGBA::new(0.3, 0.5, 0.9, 0.10)),
                DiffRow::FILE => Some(gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.08)),
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
            let left = if st.marks {
                TEXT_INSET + MARK_WIDTH
            } else {
                TEXT_INSET
            };
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
            let text_x = if row.kind == DiffRow::FILE {
                left
            } else {
                left + gutter_width + GUTTER_GAP
            };
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
    }
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
fn syntax_color(kind: u8, fg: &gdk::RGBA) -> Option<(f32, f32, f32)> {
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
