//! 右クリックなどで出すメニュー。GTK 標準の `GtkPopoverMenu`（見た目とキーボードでの操作がほかの
//! アプリと同じ）で、項目を選んだら閉包を呼ぶ。項目ごとのアクションは、出すたびに作り直す。

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

pub enum Item {
    Action(String, Box<dyn Fn()>),
    /// ここで区切る（GNOME のメニューでは、区切りは節の境目）。
    Separator,
}

/// `title` の項目を作る。
pub fn action(title: impl Into<String>, f: impl Fn() + 'static) -> Item {
    Item::Action(title.into(), Box::new(f))
}

/// 項目の名前（区切りは `---`）。確認用に、出したメニューを文字で書き出すのに使う。
pub fn titles(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|i| match i {
            Item::Action(title, _) => title.clone(),
            Item::Separator => "---".into(),
        })
        .collect()
}

/// `parent` の `(x, y)` に `items` のメニューを出す。
pub fn popup(parent: &impl IsA<gtk::Widget>, x: f64, y: f64, items: Vec<Item>) {
    if items.is_empty() {
        return;
    }
    let model = gio::Menu::new();
    let mut section = gio::Menu::new();
    let group = gio::SimpleActionGroup::new();
    for (i, item) in items.into_iter().enumerate() {
        match item {
            Item::Separator => {
                if section.n_items() > 0 {
                    model.append_section(None, &section);
                    section = gio::Menu::new();
                }
            }
            Item::Action(title, f) => {
                let name = format!("item{i}");
                let action = gio::SimpleAction::new(&name, None);
                action.connect_activate(move |_, _| f());
                group.add_action(&action);
                section.append(Some(&title), Some(&format!("context.{name}")));
            }
        }
    }
    if section.n_items() > 0 {
        model.append_section(None, &section);
    }
    // メニューの項目のアクションは、出す先の部品に付ける（前のメニューの分は置き換わる）。
    parent.insert_action_group("context", Some(&group));
    let popover = gtk::PopoverMenu::from_model(Some(&model));
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    // 閉じたら外す（次は作り直す）。項目のアクションは閉じた後に呼ばれるので、外すのは少し後。
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}
