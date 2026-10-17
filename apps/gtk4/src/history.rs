//! History の表（グラフ・ref のラベル・件名・作者と日時）。
//!
//! 行は `GtkListView` などの部品にせず、見えている行だけを 1 つのウィジェットの `snapshot` で描く
//! （150 万行でも部品を作らない）。`GtkScrollable` なので `GtkScrolledWindow` に入れればスクロールする。
//! 件名などは 256 行ずつ裏のスレッドで読み（`Repository::history_chunk`）、見えている所の近くだけを持つ。

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use amonhen_core::{HistorySnapshot, RefsSnapshot, Repository};
use amonhen_model::{Oid, RefKind};
use amonhen_projection::{FrontendProfile, HistoryChunk};
use gtk::glib::Properties;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk, pango};

use crate::graph::{self, Lanes, Metrics};

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
}

#[derive(Clone)]
struct Label {
    name: String,
    kind: RefKind,
    head: bool,
}

struct Fonts {
    subject: pango::FontDescription,
    meta: pango::FontDescription,
    label: pango::FontDescription,
}

#[derive(Default)]
pub struct State {
    repo: Option<Arc<Repository>>,
    history: Option<Arc<HistorySnapshot>>,
    chunks: HashMap<u32, Rc<Chunk>>,
    loading: HashSet<u32>,
    rows: HashMap<u32, RowText>,
    labels: HashMap<Oid, Vec<Label>>,
    label_layouts: HashMap<String, pango::Layout>,
    fonts: Option<Fonts>,
    selected: Option<u32>,
    lanes: Lanes,
    metrics: Metrics,
    tick: Option<gtk::TickCallbackId>,
    /// 大きさが決まる前に見えるようにと頼まれた行（決まったらスクロールする）。
    pending_scroll: Option<u32>,
    /// 最後に描いたとき、見えている行の文字がすべてそろっていた。
    complete: bool,
    /// `--measure-scroll` の記録（1 回の `snapshot` にかかった時間）。
    snapshot_times: Option<Vec<f64>>,
}

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

            let click = gtk::GestureClick::new();
            let weak = view.downgrade();
            click.connect_pressed(move |_, _, _, y| {
                if let Some(view) = weak.upgrade() {
                    view.grab_focus();
                    if let Some(row) = view.row_at(y as f32) {
                        view.select(row);
                    }
                }
            });
            view.add_controller(click);

            let keys = gtk::EventControllerKey::new();
            let weak = view.downgrade();
            keys.connect_key_pressed(move |_, key, _, _| {
                let Some(view) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if view.move_selection(key) {
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
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let started = Instant::now();
            self.obj().draw(snapshot);
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
        self.state().metrics.row_height
    }

    /// 表示する History を替える。同じ History の新しい世代なら、選んでいたコミットを選び直す。
    pub fn show(&self, repo: Arc<Repository>, history: Arc<HistorySnapshot>) {
        {
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
            let selected = st.selected.and_then(|row| {
                let old = st.history.as_ref()?;
                old.graph.ids().get(row as usize).copied()
            });
            st.repo = Some(repo);
            st.selected = selected.and_then(|id| history.row_of(&id));
            st.history = Some(history);
            st.chunks.clear();
            st.loading.clear();
            st.rows.clear();
            st.complete = false;
        }
        self.configure_adjustments();
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
            });
        }
        // 今のブランチ、ローカル、リモート、タグの順。
        for list in labels.values_mut() {
            list.sort_by_key(|l| (!l.head, l.kind, l.name.clone()));
        }
        self.state().labels = labels;
        self.queue_draw();
    }

    /// 見えている行の件名などがすべて読めている（`--snapshot` で撮るのを待つ）。
    pub fn is_settled(&self) -> bool {
        let st = self.state();
        st.complete && st.loading.is_empty() && !st.lanes.animating
    }

    pub fn selected(&self) -> Option<u32> {
        self.state().selected
    }

    pub fn select(&self, row: u32) {
        self.state().selected = Some(row);
        self.scroll_to_row(row);
        self.queue_draw();
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

    fn row_at(&self, y: f32) -> Option<u32> {
        let scroll = self.vadjustment().map_or(0.0, |a| a.value()) as f32;
        let row = ((y + scroll) / self.row_height()).floor();
        (row >= 0.0 && (row as u32) < self.row_count()).then_some(row as u32)
    }

    fn move_selection(&self, key: gdk::Key) -> bool {
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
        self.select(row);
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
        let size = subject.size();
        let mut meta = subject.clone();
        meta.set_size(size * 85 / 100);
        let mut label = subject.clone();
        label.set_size(size * 80 / 100);
        label.set_weight(pango::Weight::Medium);
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

            // 選んでいる行の背景。
            if let Some(row) = st.selected.filter(|r| (first..last).contains(r)) {
                let y = row as f32 * rh - scroll_y;
                let color = if focused {
                    gdk::RGBA::new(0.21, 0.52, 0.89, 1.0)
                } else {
                    gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.12)
                };
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
            let text_end = width - TEXT_INSET;
            let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6);
            let white = gdk::RGBA::WHITE;
            if st.rows.len() > 4096 {
                st.rows.clear();
            }
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
                let selected = focused && st.selected == Some(row);
                let top = row as f32 * rh - scroll_y;
                let mut x = text_x;
                let line = &st.rows[&row];
                let (_, subject_h) = line.subject.pixel_size();
                let (_, meta_h) = line.meta.pixel_size();
                let gap = 2.0;
                let text_top = ((rh - subject_h as f32 - gap - meta_h as f32) / 2.0).round();
                let id = history.graph.ids()[row as usize];
                if let Some(labels) = st.labels.get(&id).cloned() {
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
                let line = &st.rows[&row];
                line.subject
                    .set_width(((text_end - x).max(0.0) * pango::SCALE as f32) as i32);
                line.meta
                    .set_width(((text_end - text_x).max(0.0) * pango::SCALE as f32) as i32);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x, top + text_top));
                snapshot.append_layout(&line.subject, if selected { &white } else { &fg });
                snapshot.restore();
                snapshot.save();
                snapshot.translate(&graphene::Point::new(
                    text_x,
                    top + text_top + subject_h as f32 + gap,
                ));
                let meta_color = if selected {
                    gdk::RGBA::new(1.0, 1.0, 1.0, 0.8)
                } else {
                    secondary
                };
                snapshot.append_layout(&line.meta, &meta_color);
                snapshot.restore();
            }
        }
        self.state().complete = wanted.is_empty() && width > 0.0;
        for index in wanted {
            self.load_chunk(index);
        }
        self.trim_chunks(scroll_y, height);
    }

    fn row_text(&self, st: &State, history: &HistorySnapshot, chunk: &Chunk, row: u32) -> RowText {
        let fonts = st.fonts.as_ref().expect("fonts");
        let i = chunk.index(row);
        let subject = self.layout(chunk.text(chunk.0.subjects[i]), &fonts.subject);
        let author = chunk.text(chunk.0.author_names[i]);
        let date = glib::DateTime::from_unix_local(chunk.0.author_times[i])
            .and_then(|d| d.format("%Y-%m-%d %H:%M"))
            .map(|s| s.to_string())
            .unwrap_or_default();
        let id = history.graph.ids()[row as usize].to_string();
        let mut meta = format!("{author}  ·  {date}  ·  {}", &id[..8]);
        // 折りたたんだ merge は、隠しているコミットの数を添える。
        if let Some(&hidden) = history.folded.get(row as usize)
            && hidden > 0
        {
            meta.push_str(&format!("  ·  ⊕ {hidden} 件を折りたたみ"));
        }
        let meta = self.layout(&meta, &fonts.meta);
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrFontFeatures::new("tnum"));
        meta.set_attributes(Some(&attrs));
        RowText { subject, meta }
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

fn label_colors(
    named: bool,
    label: &Label,
    selected: bool,
    fg: &gdk::RGBA,
) -> (gdk::RGBA, gdk::RGBA) {
    if !named {
        return (gdk::RGBA::new(0.5, 0.5, 0.5, 0.25), *fg);
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
