//! 右クリックなどで出す小さなメニュー（項目を選んだら閉包を呼ぶ）。

use gtk::prelude::*;
use gtk::{gdk, glib};

pub enum Item {
    Action(String, Box<dyn Fn()>),
    Separator,
}

/// `title` の項目を作る。
pub fn action(title: impl Into<String>, f: impl Fn() + 'static) -> Item {
    Item::Action(title.into(), Box::new(f))
}

/// `parent` の `(x, y)` に `items` のメニューを出す。
pub fn popup(parent: &impl IsA<gtk::Widget>, x: f64, y: f64, items: Vec<Item>) {
    if items.is_empty() {
        return;
    }
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.add_css_class("menu");
    for item in items {
        match item {
            Item::Separator => column.append(&gtk::Separator::new(gtk::Orientation::Horizontal)),
            Item::Action(title, f) => {
                let label = gtk::Label::new(Some(&title));
                label.set_xalign(0.0);
                let button = gtk::Button::new();
                button.set_child(Some(&label));
                button.add_css_class("flat");
                let p = popover.downgrade();
                button.connect_clicked(move |_| {
                    if let Some(p) = p.upgrade() {
                        p.popdown();
                    }
                    f();
                });
                column.append(&button);
            }
        }
    }
    popover.set_child(Some(&column));
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    // 閉じたら外す（次は作り直す）。
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}
