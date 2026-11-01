//! 右クリックなどで出すメニュー。GTK 標準の `GtkPopoverMenu`（見た目とキーボードでの操作がほかの
//! アプリと同じ）で、項目を選んだら閉包を呼ぶ。項目ごとのアクションは、出すたびに作り直す。

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

pub enum Item {
    Action(String, Box<dyn Fn()>),
    /// 値を選ぶ項目（今の値に印を付ける。macOS 版の `state = .on`）。`color`（`#rrggbb`）があれば、名前の前に
    /// その色の丸を出す（色ラベル）。
    Choice {
        title: String,
        color: Option<&'static str>,
        on: bool,
        f: Box<dyn Fn()>,
    },
    /// 下に開くメニュー（「Reset … ▸」の Soft・Mixed・Hard など）。
    Submenu(String, Vec<Item>),
    /// ここで区切る（GNOME のメニューでは、区切りは節の境目）。
    Separator,
}

/// `title` の項目を作る。
pub fn action(title: impl Into<String>, f: impl Fn() + 'static) -> Item {
    Item::Action(title.into(), Box::new(f))
}

/// 値を選ぶ項目を作る（`on` なら印を付ける）。
pub fn choice(title: impl Into<String>, on: bool, f: impl Fn() + 'static) -> Item {
    Item::Choice {
        title: title.into(),
        color: None,
        on,
        f: Box::new(f),
    }
}

/// 項目の名前（区切りは `---`、下に開くメニューは `▸` と、字下げした中身）。確認用に、出したメニューを文字で
/// 書き出すのに使う。
pub fn titles(items: &[Item]) -> Vec<String> {
    let mut out = Vec::new();
    for i in items {
        match i {
            Item::Action(title, _) | Item::Choice { title, .. } => out.push(title.clone()),
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
        Item::Action(title, f) | Item::Choice { title, f, .. }
            if title == first && rest.is_empty() =>
        {
            f();
            true
        }
        Item::Submenu(title, children) if title == first => activate(children, rest),
        _ => false,
    })
}

/// 色の丸の付いた選ぶ項目（GTK の縦のメニューは項目の名前に色もアイコンも出せないので、自前の部品にする）。
/// 印・色の丸・名前の順に並べ、押したらメニューを閉じて `f` を呼ぶ。
fn swatch_row(title: &str, color: Option<&str>, on: bool, f: Box<dyn Fn()>) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let check = gtk::Label::new(Some(if on { "✓" } else { "" }));
    check.set_width_chars(2);
    row.append(&check);
    let dot = gtk::Label::new(None);
    dot.set_width_chars(2);
    if let Some(color) = color {
        dot.set_markup(&format!("<span foreground=\"{color}\">●</span>"));
    }
    row.append(&dot);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);
    let button = gtk::Button::builder().child(&row).build();
    button.add_css_class("flat");
    button.add_css_class("swatch-row");
    button.connect_clicked(move |b| {
        if let Some(p) = b.ancestor(gtk::Popover::static_type()) {
            p.downcast::<gtk::Popover>().expect("popover").popdown();
        }
        f();
    });
    button.upcast()
}

/// `items` を節に分けた `GMenu` にする。項目のアクションは `group` に足す（名前は `n` から順に振る）。
/// 自前の部品で出す項目は、その部品と名前を `custom` に足す（メニューを作った後に置く）。
fn build(
    items: Vec<Item>,
    group: &gio::SimpleActionGroup,
    n: &mut usize,
    custom: &mut Vec<(String, gtk::Widget)>,
) -> gio::Menu {
    // 色の丸のある段は、そろえるために、選ぶ項目をみな自前の部品で出す。
    let swatches = items
        .iter()
        .any(|i| matches!(i, Item::Choice { color: Some(_), .. }));
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
            Item::Choice {
                title,
                color,
                on,
                f,
            } => {
                let name = format!("item{n}");
                *n += 1;
                if swatches {
                    let item = gio::MenuItem::new(None, None);
                    item.set_attribute_value("custom", Some(&name.to_variant()));
                    section.append_item(&item);
                    custom.push((name, swatch_row(&title, color, on, f)));
                    continue;
                }
                // 真偽の状態を持つアクションの項目には、GTK が印を付ける。macOS 版と同じく、印は今の値にだけ
                // （ほかの項目は状態を持たせず、空の印も出さない）。
                let action = if on {
                    gio::SimpleAction::new_stateful(&name, None, &true.to_variant())
                } else {
                    gio::SimpleAction::new(&name, None)
                };
                action.connect_activate(move |_, _| f());
                group.add_action(&action);
                section.append(Some(&title), Some(&format!("context.{name}")));
            }
            Item::Submenu(title, children) => {
                let sub = build(children, group, n, custom);
                section.append_submenu(Some(&title), &sub);
            }
        }
    }
    if section.n_items() > 0 {
        model.append_section(None, &section);
    }
    model
}

/// メニューの見た目の直し。GTK のメニューは中身をスクロールの枠に入れていて、その縦のスクロールバーの最小の
/// 高さ（46）が枠の高さになるので、項目が 1 つだと下が空く。メニューの中では、スクロールバーの最小の高さをなくす。
pub fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "popover.menu scrollbar.vertical, popover.menu scrollbar.vertical range, \
         popover.menu scrollbar.vertical trough, popover.menu scrollbar.vertical slider { min-height: 0; } \
         popover.menu button.swatch-row { padding: 0 8px; min-height: 30px; font-weight: normal; } \
         popover.menu button.swatch-row label { color: @theme_fg_color; opacity: 1; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// `parent` の `(x, y)` に `items` のメニューを出す。
pub fn popup(parent: &impl IsA<gtk::Widget>, x: f64, y: f64, items: Vec<Item>) {
    if items.is_empty() {
        return;
    }
    let group = gio::SimpleActionGroup::new();
    let mut custom = Vec::new();
    let model = build(items, &group, &mut 0, &mut custom);
    // メニューの項目のアクションは、出す先の部品に付ける（前のメニューの分は置き換わる）。
    parent.insert_action_group("context", Some(&group));
    let popover = gtk::PopoverMenu::from_model(Some(&model));
    for (name, widget) in custom {
        popover.add_child(&widget, &name);
    }
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    // 子の吹き出しを置くのは親の配置（layout manager）なので、それがない親（ListBox など）では最初に測った
    // 大きさのままになり、項目が切れたり下が空いたりする。出ている間は自分で置く。
    if parent.layout_manager().is_none() {
        popover.add_tick_callback(|p, _| {
            p.present();
            glib::ControlFlow::Continue
        });
    }
    // 閉じたら外す（次は作り直す）。項目のアクションは閉じた後に呼ばれるので、外すのは少し後。
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}
