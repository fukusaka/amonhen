//! Markdown の整形表示（History の「すべてのファイル」）の中を探す帯。macOS 版は整形表示が差分と同じ検索の帯を
//! 持ち、⌘F で見えている文字を探す。ここでも、フォーカスが整形表示の中にあれば Ctrl+F でこの帯（差分の帯と同じ形で、
//! 下に置く）を開き、Ctrl+G・Ctrl+Shift+G で次・前の一致へ移る（帯の欄からは History の検索の一致へ。差分と同じ）。
//!
//! 探すのは見えている文字: TextView の文字と、文字の間に埋め込んだラベル（`<kbd>` の札・表のマス）。絵（画像・図・
//! 数式）は文字を持たないので当たらない。閉じた `<details>` の中は組み立てていないので探さない。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{glib, pango};

/// 一致の地の色（差分・詳細の検索と同じ。今の一致は橙、ほかは黄）。
const MATCH: &str = "#ffe040";
const CURRENT: &str = "#ff9400";
/// 一致に付ける文字の印の名前。
const MATCH_TAG: &str = "amonhen-find";
const CURRENT_TAG: &str = "amonhen-find-current";

/// 一致の場所。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Spot {
    /// TextView の文字（文字の位置の始まりと終わり）。
    Text(i32, i32),
    /// 埋め込んだラベル（`labels` の番号, バイトの位置の始まりと終わり）。
    Label(usize, u32, u32),
}

pub struct MarkdownFind {
    bar: gtk::Box,
    entry: gtk::SearchEntry,
    count: gtk::Label,
    text: gtk::TextView,
    /// 最後に探した、埋め込んだラベル（`Spot::Label` の番号はこの並び）。
    labels: RefCell<Vec<gtk::Label>>,
    matches: RefCell<Vec<Spot>>,
    current: Cell<Option<usize>>,
}

thread_local! {
    /// 開いている帯と、その整形表示（帯を含む部品。Ctrl+F を、フォーカスのある整形表示の帯に回すため）。
    static OPEN: RefCell<Vec<(glib::WeakRef<gtk::Widget>, Weak<MarkdownFind>)>> = RefCell::default();
    /// 確認用（`--find-markdown <語>`）: 最初に整形表示を出したら、この語で探す。
    static FIND_ONCE: RefCell<Option<String>> = RefCell::default();
}

/// 確認用（`--find-markdown <語>`）: 最初に Markdown の整形表示を出したら、帯を開いてこの語を探す。
pub fn find_once(query: &str) {
    FIND_ONCE.with(|q| q.replace(Some(query.to_owned())));
}

/// `window` でフォーカスが整形表示の中にあれば、その検索の帯を開く（Ctrl+F。開いたら true）。
pub fn find_in_focused(window: &gtk::Window) -> bool {
    match focused(window) {
        Some(find) => {
            find.show();
            true
        }
        None => false,
    }
}

/// `window` でフォーカスのある整形表示の帯が開いていれば、次・前の一致へ（Ctrl+G。移ったら true）。フォーカスが
/// 帯の中なら移らない（差分と同じく、帯の欄からは History の検索の一致へ）。
pub fn find_next_in_focused(window: &gtk::Window, step: i32) -> bool {
    match focused(window).filter(|f| f.bar.is_visible() && !f.in_bar(window)) {
        Some(find) => {
            find.move_match(step);
            true
        }
        None => false,
    }
}

/// `window` でフォーカスのある整形表示の帯が閉じていれば、帯を開いて次・前の一致へ（History を探していない
/// ときの Ctrl+G）。移ったら true。
pub fn find_next_opening(window: &gtk::Window, step: i32) -> bool {
    match focused(window).filter(|f| !f.in_bar(window)) {
        Some(find) => {
            find.move_match(step);
            true
        }
        None => false,
    }
}

/// `window` で Ctrl+G が整形表示の一致に移れるか（フォーカスが整形表示にあり、検索の帯の中ではない）。
pub fn find_next_possible(window: &gtk::Window) -> bool {
    focused(window).is_some_and(|f| !f.in_bar(window))
}

fn focused(window: &gtk::Window) -> Option<Rc<MarkdownFind>> {
    let focus = gtk::prelude::GtkWindowExt::focus(window)?;
    OPEN.with(|open| {
        let mut open = open.borrow_mut();
        open.retain(|(root, f)| root.upgrade().is_some() && f.strong_count() > 0);
        open.iter().find_map(|(root, find)| {
            let root = root.upgrade()?;
            (root.is_mapped() && (focus == root || focus.is_ancestor(&root)))
                .then(|| find.upgrade())
                .flatten()
        })
    })
}

impl MarkdownFind {
    /// `text`（整形表示の文字）を探す帯。`root` は整形表示と帯を入れる部品（フォーカスがこの中にあれば Ctrl+F で
    /// 開く）。帯は持ち主が `root` の下に置く。
    pub fn new(text: &gtk::TextView, root: &impl IsA<gtk::Widget>) -> Rc<Self> {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr!("Search"))
            .hexpand(true)
            .build();
        let count = gtk::Label::new(None);
        count.add_css_class("dim-label");
        count.add_css_class("caption");
        count.add_css_class("numeric");
        let previous = gtk::Button::from_icon_name("go-up-symbolic");
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        if !gtk::IconTheme::for_display(&text.display()).has_icon("go-up-symbolic") {
            previous.set_label("∧");
            next.set_label("∨");
        }
        // ツールチップは出さず、読み上げの名前だけ（差分の帯と同じ）。
        previous.update_property(&[gtk::accessible::Property::Label(&tr!("Previous"))]);
        next.update_property(&[gtk::accessible::Property::Label(&tr!("Next"))]);
        let done = gtk::Button::with_label(&tr!("Done"));
        for b in [&previous, &next, &done] {
            b.add_css_class("flat");
        }
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(4);
        bar.set_margin_bottom(4);
        bar.set_margin_start(8);
        bar.set_margin_end(8);
        bar.append(&entry);
        bar.append(&count);
        bar.append(&previous);
        bar.append(&next);
        bar.append(&done);
        bar.set_visible(false);
        let find = Rc::new(Self {
            bar,
            entry,
            count,
            text: text.clone(),
            labels: RefCell::default(),
            matches: RefCell::default(),
            current: Cell::default(),
        });
        OPEN.with(|open| {
            open.borrow_mut().push((
                root.upcast_ref::<gtk::Widget>().downgrade(),
                Rc::downgrade(&find),
            ))
        });
        let weak = Rc::downgrade(&find);
        find.entry.connect_search_changed(move |_| {
            if let Some(f) = weak.upgrade() {
                f.update(true);
            }
        });
        let weak = Rc::downgrade(&find);
        find.entry.connect_activate(move |_| {
            if let Some(f) = weak.upgrade() {
                f.move_match(1);
            }
        });
        let weak = Rc::downgrade(&find);
        find.entry.connect_stop_search(move |_| {
            if let Some(f) = weak.upgrade() {
                f.hide();
            }
        });
        // Shift+Enter: 前の一致。
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&find);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(f) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
                && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK)
            {
                f.move_match(-1);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        find.entry.add_controller(keys);
        for (button, step) in [(previous, -1), (next, 1)] {
            let weak = Rc::downgrade(&find);
            button.connect_clicked(move |_| {
                if let Some(f) = weak.upgrade() {
                    f.move_match(step);
                }
            });
        }
        let weak = Rc::downgrade(&find);
        done.connect_clicked(move |_| {
            if let Some(f) = weak.upgrade() {
                f.hide();
            }
        });
        if let Some(query) = FIND_ONCE.with(|q| q.take()) {
            find.bar.set_visible(true);
            find.entry.set_text(&query);
            find.update(true);
        }
        find
    }

    /// 帯（持ち主が整形表示の下に置く）。
    pub fn bar(&self) -> &gtk::Box {
        &self.bar
    }

    fn in_bar(&self, window: &gtk::Window) -> bool {
        gtk::prelude::GtkWindowExt::focus(window).is_some_and(|f| f.is_ancestor(&self.bar))
    }

    /// 帯を開いて検索の欄へ。選んだ文字（1 行）があれば、それを探す（差分と同じ）。
    pub fn show(&self) {
        let buffer = self.text.buffer();
        if let Some((start, end)) = buffer.selection_bounds() {
            let selected = buffer.text(&start, &end, false);
            if !selected.is_empty() && !selected.contains('\n') {
                self.entry.set_text(&selected);
            }
        }
        self.bar.set_visible(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        self.update(true);
    }

    fn hide(&self) {
        self.bar.set_visible(false);
        self.matches.borrow_mut().clear();
        self.current.set(None);
        self.paint();
        self.text.grab_focus();
    }

    /// 組み直した（`<details>` を開け閉めした・外観が変わった）。帯が開いていれば探し直す。
    pub fn refresh(&self) {
        if self.bar.is_visible() {
            self.update(false);
        }
    }

    /// 探し直す。今の一致は最初のもの。
    fn update(&self, reveal: bool) {
        for label in self.labels.take() {
            label.set_attributes(None);
        }
        let query = self.entry.text();
        let (matches, labels) = if query.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            search(&self.text, &query)
        };
        self.labels.replace(labels);
        self.current.set((!matches.is_empty()).then_some(0));
        self.matches.replace(matches);
        self.paint();
        self.show_count();
        if reveal {
            self.reveal();
        }
    }

    fn move_match(&self, step: i32) {
        if !self.bar.is_visible() {
            self.show();
        }
        let count = self.matches.borrow().len() as i32;
        if count == 0 {
            self.bar.error_bell();
            return;
        }
        let at = self
            .current
            .get()
            .map_or(if step > 0 { -1 } else { 0 }, |c| c as i32);
        self.current
            .set(Some((at + step).rem_euclid(count) as usize));
        self.paint();
        self.show_count();
        self.reveal();
    }

    /// 一致に色を付ける（今の一致は濃く）。文字は黒に（暗い見た目でも読めるよう。詳細の検索と同じ）。
    fn paint(&self) {
        let buffer = self.text.buffer();
        let table = buffer.tag_table();
        let tag = |name: &str, color: &str| {
            let tag = table.lookup(name).unwrap_or_else(|| {
                let tag = gtk::TextTag::builder()
                    .name(name)
                    .background(color)
                    .foreground("black")
                    .build();
                table.add(&tag);
                tag
            });
            // 組み立てるときに作った印（コードの背景など）より上に。
            tag.set_priority(table.size() - 1);
            tag
        };
        let (plain, strong) = (tag(MATCH_TAG, MATCH), tag(CURRENT_TAG, CURRENT));
        let (start, end) = buffer.bounds();
        buffer.remove_tag(&plain, &start, &end);
        buffer.remove_tag(&strong, &start, &end);
        let matches = self.matches.borrow();
        let current = self.current.get();
        let labels = self.labels.borrow();
        let mut lists: Vec<Option<pango::AttrList>> = vec![None; labels.len()];
        for (n, spot) in matches.iter().enumerate() {
            let now = current == Some(n);
            match *spot {
                Spot::Text(from, to) => {
                    let tag = if now { &strong } else { &plain };
                    buffer.apply_tag(
                        tag,
                        &buffer.iter_at_offset(from),
                        &buffer.iter_at_offset(to),
                    );
                }
                Spot::Label(i, from, to) => {
                    let list = lists[i].get_or_insert_with(pango::AttrList::new);
                    let (r, g, b) = if now {
                        (0xffff, 0x9400, 0x0000)
                    } else {
                        (0xffff, 0xe000, 0x4000)
                    };
                    let mut bg = pango::AttrColor::new_background(r, g, b);
                    bg.set_start_index(from);
                    bg.set_end_index(to);
                    list.insert(bg);
                    let mut fg = pango::AttrColor::new_foreground(0, 0, 0);
                    fg.set_start_index(from);
                    fg.set_end_index(to);
                    list.insert(fg);
                }
            }
        }
        for (label, list) in labels.iter().zip(&lists) {
            label.set_attributes(list.as_ref());
        }
    }

    fn show_count(&self) {
        let count = self.matches.borrow().len();
        let text = if self.entry.text().is_empty() {
            String::new()
        } else if let Some(c) = self.current.get() {
            format!("{} / {count}", c + 1)
        } else {
            tr!("Not found")
        };
        self.count.set_text(&text);
    }

    /// 今の一致が見える位置までスクロールする（大きさが決まってから）。
    fn reveal(&self) {
        let Some(spot) = self
            .current
            .get()
            .and_then(|n| self.matches.borrow().get(n).copied())
        else {
            return;
        };
        let text = self.text.clone();
        let label = match spot {
            Spot::Label(i, ..) => self.labels.borrow().get(i).cloned(),
            Spot::Text(..) => None,
        };
        glib::idle_add_local_once(move || {
            let Some(adj) = text.vadjustment() else {
                return;
            };
            // 一致の上の端と高さ（文字の座標）。
            let (top, height) = match (spot, label) {
                (Spot::Text(from, _), _) => {
                    let rect = text.iter_location(&text.buffer().iter_at_offset(from));
                    (rect.y() as f64, rect.height().max(1) as f64)
                }
                (Spot::Label(_, from, _), Some(label)) => {
                    let rect = label.layout().index_to_pos(from as i32);
                    let (_, offset_y) = label.layout_offsets();
                    let y = offset_y as f32 + rect.y() as f32 / pango::SCALE as f32;
                    let Some(point) =
                        label.compute_point(&text, &gtk::graphene::Point::new(0.0, y))
                    else {
                        return;
                    };
                    let (_, by) = text.window_to_buffer_coords(
                        gtk::TextWindowType::Widget,
                        0,
                        point.y() as i32,
                    );
                    (by as f64, (rect.height() / pango::SCALE).max(1) as f64)
                }
                _ => return,
            };
            if top < adj.value() || top + height > adj.value() + adj.page_size() {
                adj.set_value((top - adj.page_size() / 3.0).max(0.0));
            }
        });
    }
}

/// `text` の見えている文字から `query` を探す（文書の順）。埋め込んだラベルは、その場所の順に並べる。
fn search(text: &gtk::TextView, query: &str) -> (Vec<Spot>, Vec<gtk::Label>) {
    let buffer = text.buffer();
    let (start, end) = buffer.bounds();
    // 埋め込んだもの（部品・絵）は U+FFFC になる。
    let all = buffer.slice(&start, &end, true);
    let mut matches = Vec::new();
    let mut labels = Vec::new();
    let mut piece = String::new();
    let mut piece_start = 0i32;
    let flush = |piece: &str, piece_start: i32, matches: &mut Vec<Spot>| {
        for (from, to) in crate::messagefind::find_all(piece, query) {
            let a = piece_start + piece[..from].chars().count() as i32;
            let b = a + piece[from..to].chars().count() as i32;
            matches.push(Spot::Text(a, b));
        }
    };
    for (offset, c) in all.chars().enumerate() {
        let offset = offset as i32;
        if c != '\u{fffc}' {
            if piece.is_empty() {
                piece_start = offset;
            }
            piece.push(c);
            continue;
        }
        flush(&piece, piece_start, &mut matches);
        piece.clear();
        let Some(anchor) = buffer.iter_at_offset(offset).child_anchor() else {
            continue;
        };
        for widget in anchor.widgets() {
            let mut found = Vec::new();
            collect_labels(&widget, &mut found);
            for label in found {
                let spots: Vec<(usize, usize)> = crate::messagefind::find_all(&label.text(), query);
                if spots.is_empty() {
                    continue;
                }
                let i = labels.len();
                labels.push(label);
                matches.extend(
                    spots
                        .into_iter()
                        .map(|(from, to)| Spot::Label(i, from as u32, to as u32)),
                );
            }
        }
    }
    flush(&piece, piece_start, &mut matches);
    (matches, labels)
}

/// `widget` とその中の、見えているラベル（上から順）。
fn collect_labels(widget: &gtk::Widget, out: &mut Vec<gtk::Label>) {
    if !widget.is_visible() {
        return;
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        out.push(label.clone());
        return;
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        collect_labels(&c, out);
        child = c.next_sibling();
    }
}
