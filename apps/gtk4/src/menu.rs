//! 右クリックなどで出すメニュー。GTK 標準の `GtkPopoverMenu`（見た目とキーボードでの操作がほかの
//! アプリと同じ）で、項目を選んだら閉包を呼ぶ。項目ごとのアクションは、出すたびに作り直す。

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

pub enum Item {
    Action(String, Box<dyn Fn()>),
    /// 下に開くメニュー（「Reset … ▸」の Soft・Mixed・Hard など）。
    Submenu(String, Vec<Item>),
    /// ここで区切る（GNOME のメニューでは、区切りは節の境目）。
    Separator,
}

/// `title` の項目を作る。
pub fn action(title: impl Into<String>, f: impl Fn() + 'static) -> Item {
    Item::Action(title.into(), Box::new(f))
}

/// 項目の名前（区切りは `---`、下に開くメニューは `▸` と、字下げした中身）。確認用に、出したメニューを文字で
/// 書き出すのに使う。
pub fn titles(items: &[Item]) -> Vec<String> {
    let mut out = Vec::new();
    for i in items {
        match i {
            Item::Action(title, _) => out.push(title.clone()),
            Item::Submenu(title, children) => {
                out.push(format!("{title} ▸"));
                out.extend(titles(children).into_iter().map(|t| format!("    {t}")));
            }
            Item::Separator => out.push("---".into()),
        }
    }
    out
}

/// `path`（項目の名前。下に開くメニューの中は ` > ` でつなぐ）の項目を選んだことにする（確認用）。
pub fn activate(items: &[Item], path: &str) -> bool {
    let (first, rest) = path.split_once(" > ").unwrap_or((path, ""));
    items.iter().any(|item| match item {
        Item::Action(title, f) if title == first && rest.is_empty() => {
            f();
            true
        }
        Item::Submenu(title, children) if title == first => activate(children, rest),
        _ => false,
    })
}

/// `items` を節に分けた `GMenu` にする。項目のアクションは `group` に足す（名前は `n` から順に振る）。
fn build(items: Vec<Item>, group: &gio::SimpleActionGroup, n: &mut usize) -> gio::Menu {
    let model = gio::Menu::new();
    let mut section = gio::Menu::new();
    for item in items {
        match item {
            Item::Separator => {
                if section.n_items() > 0 {
                    model.append_section(None, &section);
                    section = gio::Menu::new();
                }
            }
            Item::Action(title, f) => {
                let name = format!("item{n}");
                *n += 1;
                let action = gio::SimpleAction::new(&name, None);
                action.connect_activate(move |_, _| f());
                group.add_action(&action);
                section.append(Some(&title), Some(&format!("context.{name}")));
            }
            Item::Submenu(title, children) => {
                let sub = build(children, group, n);
                section.append_submenu(Some(&title), &sub);
            }
        }
    }
    if section.n_items() > 0 {
        model.append_section(None, &section);
    }
    model
}

/// `parent` の `(x, y)` に `items` のメニューを出す。
pub fn popup(parent: &impl IsA<gtk::Widget>, x: f64, y: f64, items: Vec<Item>) {
    if items.is_empty() {
        return;
    }
    let group = gio::SimpleActionGroup::new();
    let model = build(items, &group, &mut 0);
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
