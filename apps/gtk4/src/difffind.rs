//! 差分の中を探す帯（Working Copy・File History の差分。History の差分は `commitfiles` が自分で持つ）。macOS 版は
//! 検索の帯が差分の部品（`DiffViewController`）にあり、どの差分でも ⌘F で探せる。ここでも、フォーカスが差分の
//! 中にあれば Ctrl+F でこの帯を開き、Ctrl+G・Ctrl+Shift+G で次・前の一致へ移る。

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;

use crate::diff::DiffWidget;

/// 差分の検索の帯。部品は弱く持つ（帯の部品の閉包がこれを持つので、循環しないように）。
#[derive(Clone)]
pub struct DiffFind(Rc<Inner>);

struct Inner {
    diff: glib::WeakRef<DiffWidget>,
    /// フォーカスがこの中にあれば、Ctrl+F でこの帯を開く。
    area: glib::WeakRef<gtk::Widget>,
    bar: glib::WeakRef<gtk::Box>,
    entry: glib::WeakRef<gtk::SearchEntry>,
    count: glib::WeakRef<gtk::Label>,
}

thread_local! {
    /// 開いている帯（Ctrl+F を、フォーカスのある差分の帯に回すため）。
    static OPEN: RefCell<Vec<Weak<Inner>>> = RefCell::default();
}

/// `window` でフォーカスのある差分に帯があれば、それを開く（Ctrl+F。開いたら true）。
pub fn find_in_focused(window: &gtk::Window) -> bool {
    match focused(window) {
        Some(find) => {
            find.show();
            true
        }
        None => false,
    }
}

/// `window` でフォーカスのある差分の帯が開いていれば、次・前の一致へ（Ctrl+G。移ったら true）。
pub fn find_next_in_focused(window: &gtk::Window, step: i32) -> bool {
    match focused(window).filter(|f| f.0.bar.upgrade().is_some_and(|b| b.is_visible())) {
        Some(find) => {
            find.move_match(step);
            true
        }
        None => false,
    }
}

fn focused(window: &gtk::Window) -> Option<DiffFind> {
    let focus = gtk::prelude::GtkWindowExt::focus(window)?;
    OPEN.with(|open| {
        let mut open = open.borrow_mut();
        open.retain(|w| w.upgrade().is_some_and(|i| i.area.upgrade().is_some()));
        open.iter().filter_map(Weak::upgrade).find_map(|inner| {
            let area = inner.area.upgrade()?;
            (area.is_mapped() && (focus == area || focus.is_ancestor(&area)))
                .then(|| DiffFind(inner))
        })
    })
}

impl DiffFind {
    /// `diff` の検索の帯を作る。`area`（差分とこの帯を入れる部品）にフォーカスがあれば Ctrl+F で開く。帯は
    /// 差分の下に置く（History の差分と同じ）。
    pub fn new(diff: &DiffWidget, area: &impl IsA<gtk::Widget>) -> (DiffFind, gtk::Box) {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr!("Search Diff"))
            .hexpand(true)
            .build();
        let count = gtk::Label::new(None);
        count.add_css_class("dim-label");
        count.add_css_class("caption");
        count.add_css_class("numeric");
        let previous = gtk::Button::from_icon_name("go-up-symbolic");
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        if !gtk::IconTheme::for_display(&diff.display()).has_icon("go-up-symbolic") {
            previous.set_label("∧");
            next.set_label("∨");
        }
        // ツールチップは出さず、読み上げの名前だけ（macOS 版と同じ）。
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
        let find = DiffFind(Rc::new(Inner {
            diff: diff.downgrade(),
            area: area.upcast_ref::<gtk::Widget>().downgrade(),
            bar: bar.downgrade(),
            entry: entry.downgrade(),
            count: count.downgrade(),
        }));
        OPEN.with(|open| open.borrow_mut().push(Rc::downgrade(&find.0)));

        let f = find.clone();
        entry.connect_search_changed(move |entry| {
            if let Some(diff) = f.0.diff.upgrade() {
                let (current, count) = diff.find(&entry.text());
                f.show_count(current, count);
            }
        });
        let f = find.clone();
        entry.connect_activate(move |_| f.move_match(1));
        let f = find.clone();
        entry.connect_stop_search(move |_| f.hide());
        // Shift+Enter: 前の一致。
        let keys = gtk::EventControllerKey::new();
        let f = find.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
                && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK)
            {
                f.move_match(-1);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        entry.add_controller(keys);
        for (button, step) in [(previous, -1), (next, 1)] {
            let f = find.clone();
            button.connect_clicked(move |_| f.move_match(step));
        }
        let f = find.clone();
        done.connect_clicked(move |_| f.hide());
        (find, bar)
    }

    /// 帯を開いて検索の欄へ。選んだ文字（1 行）があれば、それを探す（macOS 版と同じ）。
    pub fn show(&self) {
        let (Some(diff), Some(bar), Some(entry)) = (
            self.0.diff.upgrade(),
            self.0.bar.upgrade(),
            self.0.entry.upgrade(),
        ) else {
            return;
        };
        if let Some(selected) = diff
            .selected_text()
            .filter(|t| !t.is_empty() && !t.contains('\n'))
        {
            entry.set_text(&selected);
        }
        bar.set_visible(true);
        entry.grab_focus();
        entry.select_region(0, -1);
        let (current, count) = diff.find(&entry.text());
        self.show_count(current, count);
    }

    fn hide(&self) {
        if let Some(bar) = self.0.bar.upgrade() {
            bar.set_visible(false);
        }
        if let Some(diff) = self.0.diff.upgrade() {
            diff.clear_find();
            diff.grab_focus();
        }
    }

    fn move_match(&self, step: i32) {
        if !self.0.bar.upgrade().is_some_and(|b| b.is_visible()) {
            self.show();
        }
        if let Some(diff) = self.0.diff.upgrade() {
            let (current, count) = diff.find_move(step);
            self.show_count(current, count);
        }
    }

    /// 差分を出し直したら、一致を探し直して数を直す（帯が開いているときだけ）。
    pub fn refresh(&self) {
        let (Some(diff), Some(bar), Some(entry)) = (
            self.0.diff.upgrade(),
            self.0.bar.upgrade(),
            self.0.entry.upgrade(),
        ) else {
            return;
        };
        if bar.is_visible() {
            let (current, count) = diff.recount(&entry.text());
            self.show_count(current, count);
        }
    }

    fn show_count(&self, current: Option<usize>, count: usize) {
        let (Some(label), Some(entry)) = (self.0.count.upgrade(), self.0.entry.upgrade()) else {
            return;
        };
        let text = if entry.text().is_empty() {
            String::new()
        } else if let Some(c) = current {
            format!("{} / {count}", c + 1)
        } else {
            tr!("Not found")
        };
        label.set_text(&text);
    }
}
