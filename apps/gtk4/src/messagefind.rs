//! 詳細の中（件名・本文・notes）を探す帯。macOS 版はメッセージの欄が `NSTextView` の検索の帯を持ち、フォーカスが
//! そこにあれば ⌘F でメッセージの中を探す。ここでも、フォーカスが詳細の中にあれば Ctrl+F でこの帯を開き、
//! 大文字・小文字を区別せずにすべての一致に色を付け、今の一致を見える位置までスクロールする。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use amonhen_model::tr;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

pub struct MessageFind {
    pub bar: gtk::Box,
    entry: gtk::SearchEntry,
    count: gtk::Label,
    /// 探す欄（上から順。表は選び直すたびに作り直すので、そのつど求める）。
    labels: Box<dyn Fn() -> Vec<gtk::Label>>,
    /// 最後に探した、見えている欄（一致の欄の番号はこの並び）。
    shown: RefCell<Vec<gtk::Label>>,
    scrolled: gtk::ScrolledWindow,
    /// 一致（欄の番号, 文字の始まり, 終わり。バイトの位置）。
    matches: RefCell<Vec<(usize, u32, u32)>>,
    current: Cell<Option<usize>>,
}

thread_local! {
    /// 開いている詳細の帯（Ctrl+F で、フォーカスのある詳細を探す）。
    static OPEN: RefCell<Vec<(glib::WeakRef<gtk::Widget>, Weak<MessageFind>)>> = RefCell::default();
    /// 確認用（`--find-message <語>`）: 最初に詳細を出したら、この語で探す。
    static FIND_ONCE: RefCell<Option<String>> = RefCell::default();
}

/// 確認用（`--find-message <語>`）: 最初にコミットの詳細を出したら、帯を開いてこの語を探す。
pub fn find_once(query: &str) {
    FIND_ONCE.with(|q| q.replace(Some(query.to_owned())));
}

/// `window` でフォーカスが詳細の中にあれば、その検索の帯を開く（Ctrl+F。開いたら true）。
pub fn find_in_focused(window: &gtk::Window) -> bool {
    match focused(window) {
        Some(find) => {
            find.show();
            true
        }
        None => false,
    }
}

/// `window` でフォーカスのある詳細の検索の帯が開いていれば、次・前の一致へ（Ctrl+G。移ったら true）。
pub fn find_next_in_focused(window: &gtk::Window, step: i32) -> bool {
    match focused(window).filter(|f| f.bar.is_visible()) {
        Some(find) => {
            find.move_match(step);
            true
        }
        None => false,
    }
}

/// `window` でフォーカスのある詳細の検索の帯が閉じていれば、帯を開いて次・前の一致へ（History を探していない
/// ときの Ctrl+G。macOS 版のメッセージの欄の `findNext`）。移ったら true。
pub fn find_next_opening(window: &gtk::Window, step: i32) -> bool {
    match focused(window) {
        Some(find) => {
            find.open_and_move(step);
            true
        }
        None => false,
    }
}

/// `window` で Ctrl+G が詳細の一致に移れるか（フォーカスが詳細の中にある。帯が閉じていれば開いて移る）。
pub fn find_next_possible(window: &gtk::Window) -> bool {
    focused(window).is_some()
}

/// `window` で、フォーカスが中にある詳細の帯。
fn focused(window: &gtk::Window) -> Option<Rc<MessageFind>> {
    let focus = gtk::prelude::GtkWindowExt::focus(window)?;
    OPEN.with(|open| {
        let mut open = open.borrow_mut();
        open.retain(|(_, f)| f.strong_count() > 0);
        open.iter().find_map(|(root, find)| {
            let root = root.upgrade()?;
            (root.is_mapped() && (focus == root || focus.is_ancestor(&root)))
                .then(|| find.upgrade())
                .flatten()
        })
    })
}

impl MessageFind {
    /// `root`（詳細の全体）の中の `labels` を探す帯。`scrolled` は一致を見せるためにスクロールする所。
    pub fn new(
        root: &gtk::Box,
        scrolled: &gtk::ScrolledWindow,
        labels: impl Fn() -> Vec<gtk::Label> + 'static,
    ) -> Rc<Self> {
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
        if !gtk::IconTheme::for_display(&entry.display()).has_icon("go-up-symbolic") {
            previous.set_label("∧");
            next.set_label("∨");
        }
        previous.set_tooltip_text(Some(&tr!("Previous")));
        next.set_tooltip_text(Some(&tr!("Next")));
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
            labels: Box::new(labels),
            shown: RefCell::default(),
            scrolled: scrolled.clone(),
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
        find
    }

    pub fn show(&self) {
        self.bar.set_visible(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        self.update(true);
    }

    /// 帯を開き（前に探した語のまま）、次なら最初の一致、前なら最後の一致へ。語がなければ開くだけ。
    fn open_and_move(&self, step: i32) {
        if self.bar.is_visible() {
            self.move_match(step);
            return;
        }
        self.show();
        if self.entry.text().is_empty() {
            return;
        }
        self.current.set(None);
        self.move_match(step);
    }

    fn hide(&self) {
        self.bar.set_visible(false);
        self.matches.borrow_mut().clear();
        self.current.set(None);
        self.paint();
        if let Some(label) = self.shown.borrow().first() {
            label.grab_focus();
        }
    }

    /// 中身を出し直した（別のコミットを選んだ）。帯が開いていれば探し直す。確認用の語があれば帯を開く。
    pub fn refresh(&self) {
        if (self.labels)().iter().all(|l| l.text().is_empty()) {
            return;
        }
        if let Some(query) = FIND_ONCE.with(|q| q.take()) {
            self.bar.set_visible(true);
            self.entry.set_text(&query);
            self.update(true);
        } else if self.bar.is_visible() {
            self.update(false);
        }
    }

    /// すべての欄から探し直す。今の一致は最初のもの。
    fn update(&self, reveal: bool) {
        // 前に色を付けた欄は消しておく。
        for label in self.shown.take() {
            label.set_attributes(None);
        }
        let shown: Vec<gtk::Label> = (self.labels)()
            .into_iter()
            .filter(|l| l.is_visible() && l.parent().is_none_or(|p| p.is_visible()))
            .collect();
        let query = self.entry.text();
        let mut matches = Vec::new();
        if !query.is_empty() {
            for (i, label) in shown.iter().enumerate() {
                for (start, end) in find_all(&label.text(), &query) {
                    matches.push((i, start as u32, end as u32));
                }
            }
        }
        self.shown.replace(shown);
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

    /// 一致に色を付ける（今の一致は濃く）。
    fn paint(&self) {
        let matches = self.matches.borrow();
        let current = self.current.get();
        for (i, label) in self.shown.borrow().iter().enumerate() {
            let list = pango::AttrList::new();
            let mut any = false;
            for (n, &(_, start, end)) in matches.iter().enumerate().filter(|(_, m)| m.0 == i) {
                // 今の一致は橙、ほかは黄（macOS の検索の色に近い）。
                let (r, g, b) = if current == Some(n) {
                    (0xffff, 0x9400, 0x0000)
                } else {
                    (0xffff, 0xe000, 0x4000)
                };
                let mut bg = pango::AttrColor::new_background(r, g, b);
                bg.set_start_index(start);
                bg.set_end_index(end);
                list.insert(bg);
                // 暗い見た目でも読めるよう、文字は黒に。
                let mut fg = pango::AttrColor::new_foreground(0, 0, 0);
                fg.set_start_index(start);
                fg.set_end_index(end);
                list.insert(fg);
                any = true;
            }
            label.set_attributes(any.then_some(&list));
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
        let Some(n) = self.current.get() else { return };
        let Some(&(i, start, _)) = self.matches.borrow().get(n) else {
            return;
        };
        let Some(label) = self.shown.borrow().get(i).cloned() else {
            return;
        };
        let scrolled = self.scrolled.clone();
        glib::idle_add_local_once(move || {
            let Some(child) = scrolled.child() else {
                return;
            };
            let rect = label.layout().index_to_pos(start as i32);
            let (_, offset_y) = label.layout_offsets();
            let y = offset_y as f32 + rect.y() as f32 / pango::SCALE as f32;
            let height = (rect.height() / pango::SCALE).max(1) as f64;
            let Some(point) = label.compute_point(&child, &gtk::graphene::Point::new(0.0, y))
            else {
                return;
            };
            let adj = scrolled.vadjustment();
            let top = point.y() as f64;
            if top < adj.value() || top + height > adj.value() + adj.page_size() {
                adj.set_value((top - adj.page_size() / 3.0).max(0.0));
            }
        });
    }
}

/// `text` の中の `query` の一致（大文字・小文字を区別しない。バイトの位置の始まりと終わり）。
pub(crate) fn find_all(text: &str, query: &str) -> Vec<(usize, usize)> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let hay: Vec<(usize, char)> = text.char_indices().map(|(i, c)| (i, fold(c))).collect();
    let needle: Vec<char> = query.chars().map(fold).collect();
    let mut out = Vec::new();
    if needle.is_empty() || needle.len() > hay.len() {
        return out;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()]
            .iter()
            .zip(&needle)
            .all(|((_, a), b)| a == b)
        {
            let end = hay.get(i + needle.len()).map_or(text.len(), |(at, _)| *at);
            out.push((hay[i].0, end));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::find_all;

    #[test]
    fn finds_without_case() {
        assert_eq!(
            find_all("Fix the fix. FIX", "fix"),
            vec![(0, 3), (8, 11), (13, 16)]
        );
        assert_eq!(find_all("日本語の本", "本"), vec![(3, 6), (12, 15)]);
        assert!(find_all("abc", "").is_empty());
        assert!(find_all("ab", "abc").is_empty());
    }
}
