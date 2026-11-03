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
    /// ツールチップの付いた項目（macOS 版の `toolTip`。GMenu の項目には出せないので、自前の部品で出す）。
    Hint {
        title: String,
        tip: String,
        f: Box<dyn Fn()>,
    },
    /// ショートカットを添えた項目（macOS 版の `keyEquivalent`。キーは別のところで効くもので、ここでは見せるだけ）。
    /// `accel` は GTK の形（`<Control>c`・`Return`）。
    Keyed {
        title: String,
        accel: String,
        f: Box<dyn Fn()>,
    },
    /// 押せない項目（説明だけ。macOS 版の `isEnabled = false`）。
    Disabled(String),
    /// 押せない項目で、押せないわけをツールチップに出すもの（macOS 版の `isEnabled = false` と `toolTip`）。
    DisabledHint {
        title: String,
        tip: String,
    },
    /// 下に開くメニュー（Repositories の行の Group・Color Label など）。
    Submenu(String, Vec<Item>),
    /// ここで区切る（GNOME のメニューでは、区切りは節の境目）。
    Separator,
}

/// 押せない `title` の項目に、押せないわけ `tip` をツールチップで添える。
pub fn disabled_hint(title: impl Into<String>, tip: impl Into<String>) -> Item {
    Item::DisabledHint {
        title: title.into(),
        tip: tip.into(),
    }
}

/// `title` の項目を作る。
pub fn action(title: impl Into<String>, f: impl Fn() + 'static) -> Item {
    Item::Action(title.into(), Box::new(f))
}

/// ショートカット `accel`（GTK の形）を添えた `title` の項目を作る。
pub fn keyed(title: impl Into<String>, accel: &str, f: impl Fn() + 'static) -> Item {
    Item::Keyed {
        title: title.into(),
        accel: accel.to_owned(),
        f: Box::new(f),
    }
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

/// ツールチップ `tip` の付いた `title` の項目を作る。
pub fn hint(title: impl Into<String>, tip: impl Into<String>, f: impl Fn() + 'static) -> Item {
    Item::Hint {
        title: title.into(),
        tip: tip.into(),
        f: Box::new(f),
    }
}

/// 項目の名前（区切りは `---`、下に開くメニューは `▸` と、字下げした中身）。確認用に、出したメニューを文字で
/// 書き出すのに使う。
pub fn titles(items: &[Item]) -> Vec<String> {
    let mut out = Vec::new();
    for i in items {
        match i {
            Item::Action(title, _)
            | Item::Choice { title, .. }
            | Item::Hint { title, .. }
            | Item::Keyed { title, .. } => out.push(title.clone()),
            Item::Disabled(title) | Item::DisabledHint { title, .. } => {
                out.push(format!("({title})"))
            }
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
        Item::Action(title, f)
        | Item::Choice { title, f, .. }
        | Item::Hint { title, f, .. }
        | Item::Keyed { title, f, .. }
            if title == first && rest.is_empty() =>
        {
            f();
            true
        }
        Item::Submenu(title, children) if title == first => activate(children, rest),
        _ => false,
    })
}

/// 選ぶ項目のある段の項目（macOS 版のように、印の欄を空けて今の値にだけ ✓ を出す。GTK の印は四角の中の ✓
/// で、項目の名前に色もアイコンも出せないので、自前の部品にする）。印・色の丸（`dot` が Some のとき、その欄を
/// 空ける）・名前の順に並べ、押したらメニューを閉じて `f` を呼ぶ。`on` が None なら印の欄を置かない（選ぶ項目の
/// ない段のツールチップ付きの項目。名前は GTK の項目と同じ位置）。`tip` はツールチップ、`accel` は右に添える
/// ショートカット（GTK の形）。
fn choice_row(
    title: &str,
    on: Option<bool>,
    dot: Option<Option<&str>>,
    tip: Option<&str>,
    accel: Option<&str>,
    f: Box<dyn Fn()>,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if let Some(on) = on {
        let check = gtk::Label::new(Some(if on { "✓" } else { "" }));
        check.set_width_chars(2);
        row.append(&check);
    }
    if let Some(color) = dot {
        let dot = gtk::Label::new(None);
        dot.set_width_chars(2);
        if let Some(color) = color {
            dot.set_markup(&format!("<span foreground=\"{color}\">●</span>"));
        }
        row.append(&dot);
    }
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);
    if let Some((key, mods)) = accel.and_then(gtk::accelerator_parse) {
        let accel = gtk::Label::new(Some(&gtk::accelerator_get_label(key, mods)));
        accel.add_css_class("dim-label");
        accel.set_margin_start(24);
        row.append(&accel);
    }
    let button = gtk::Button::builder().child(&row).build();
    button.add_css_class("flat");
    button.add_css_class(if on.is_some() {
        "choice-row"
    } else {
        "hint-row"
    });
    button.set_tooltip_text(tip);
    button.connect_clicked(move |b| {
        if let Some(p) = b.ancestor(gtk::Popover::static_type()) {
            p.downcast::<gtk::Popover>().expect("popover").popdown();
        }
        f();
    });
    button.upcast()
}

/// 自前の部品の項目を `section` に足す（部品は `custom` に。メニューを作った後に置く）。
fn add_custom(
    section: &gio::Menu,
    n: &mut usize,
    custom: &mut Vec<(String, gtk::Widget)>,
    widget: gtk::Widget,
) {
    let name = format!("item{n}");
    *n += 1;
    let item = gio::MenuItem::new(None, None);
    item.set_attribute_value("custom", Some(&name.to_variant()));
    section.append_item(&item);
    custom.push((name, widget));
}

/// `items` を節に分けた `GMenu` にする。項目のアクションは `group` に足す（名前は `n` から順に振る）。
/// 自前の部品で出す項目は、その部品と名前を `custom` に足す（メニューを作った後に置く）。
fn build(
    items: Vec<Item>,
    group: &gio::SimpleActionGroup,
    n: &mut usize,
    custom: &mut Vec<(String, gtk::Widget)>,
) -> gio::Menu {
    // 選ぶ項目のある段は、名前の位置をそろえるために、項目をみな自前の部品で出す。色の丸があれば、その欄も。
    let choices = items.iter().any(|i| matches!(i, Item::Choice { .. }));
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
            Item::Action(title, f) if choices => {
                let dot = swatches.then_some(None);
                add_custom(
                    &section,
                    n,
                    custom,
                    choice_row(&title, Some(false), dot, None, None, f),
                );
            }
            Item::Keyed { title, accel, f } if choices => {
                let dot = swatches.then_some(None);
                add_custom(
                    &section,
                    n,
                    custom,
                    choice_row(&title, Some(false), dot, None, Some(&accel), f),
                );
            }
            Item::Keyed { title, accel, f } => {
                let name = format!("item{n}");
                *n += 1;
                let action = gio::SimpleAction::new(&name, None);
                action.connect_activate(move |_, _| f());
                group.add_action(&action);
                let item = gio::MenuItem::new(Some(&title), Some(&format!("context.{name}")));
                item.set_attribute_value("accel", Some(&accel.to_variant()));
                section.append_item(&item);
            }
            Item::Hint { title, tip, f } => {
                let (on, dot) = if choices {
                    (Some(false), swatches.then_some(None))
                } else {
                    (None, None)
                };
                add_custom(
                    &section,
                    n,
                    custom,
                    choice_row(&title, on, dot, Some(&tip), None, f),
                );
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
                let dot = swatches.then_some(color);
                add_custom(
                    &section,
                    n,
                    custom,
                    choice_row(&title, Some(on), dot, None, None, f),
                );
            }
            Item::Disabled(title) => {
                // グループにないアクションの項目は、GTK が押せない見た目にする。
                section.append(Some(&title), Some("context.disabled"));
            }
            Item::DisabledHint { title, tip } => {
                // GMenu の項目にはツールチップを出せないので、自前の部品を押せなくして出す。
                let (on, dot) = if choices {
                    (Some(false), swatches.then_some(None))
                } else {
                    (None, None)
                };
                let row = choice_row(&title, on, dot, Some(&tip), None, Box::new(|| {}));
                row.set_sensitive(false);
                add_custom(&section, n, custom, row);
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
/// 等幅の入力欄の説明（プレースホルダー）は、ふつうの字で（等幅だと横に長く、切れる）。
pub fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "popover.menu scrollbar.vertical, popover.menu scrollbar.vertical range, \
         popover.menu scrollbar.vertical trough, popover.menu scrollbar.vertical slider { min-height: 0; } \
         popover.menu button.choice-row { padding: 0 8px; min-height: 30px; font-weight: normal; } \
         popover.menu button.choice-row label { color: @theme_fg_color; opacity: 1; } \
         popover.menu button.hint-row { padding: 0 12px; min-height: 30px; font-weight: normal; } \
         popover.menu button.hint-row label { color: @theme_fg_color; opacity: 1; } \
         entry.monospace text placeholder { font-family: sans-serif; } \
         .drop-above { box-shadow: inset 0 2px @accent_color; } \
         .drop-below { box-shadow: inset 0 -2px @accent_color; } \
         .drop-above:drop(active), .drop-below:drop(active) { background: none; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// `menu`（区切りと下のメニューの中も）の項目の名前を `title(アクションの名前)` に替える（None なら替えない）。
pub fn retitle(menu: &gio::MenuModel, title: &dyn Fn(&str) -> Option<String>) {
    for i in 0..menu.n_items() {
        for link in [gio::MENU_LINK_SECTION, gio::MENU_LINK_SUBMENU] {
            if let Some(child) = menu.item_link(i, link) {
                retitle(&child, title);
            }
        }
        let Some(menu) = menu.downcast_ref::<gio::Menu>() else {
            continue;
        };
        let action = menu
            .item_attribute_value(i, gio::MENU_ATTRIBUTE_ACTION, Some(glib::VariantTy::STRING))
            .and_then(|v| v.get::<String>());
        let label = menu
            .item_attribute_value(i, gio::MENU_ATTRIBUTE_LABEL, Some(glib::VariantTy::STRING))
            .and_then(|v| v.get::<String>());
        if let Some(new) = action.as_deref().and_then(title)
            && label.as_deref() != Some(new.as_str())
        {
            let item = gio::MenuItem::from_model(menu, i);
            item.set_label(Some(&new));
            menu.remove(i);
            menu.insert_item(i, &item);
        }
    }
}

/// GMenu の項目のツールチップを入れておく属性（macOS 版の項目の `toolTip`）。GTK の `GtkPopoverMenu` は項目に
/// ツールチップを出さないので、[`tooltips_on_show`] が出すたびに項目の部品に付ける。
pub const TOOLTIP: &str = "amonhen-tooltip";

/// `title` の項目（アクション `action`）にツールチップ `tip` を付けて `menu` に足す。
pub fn append_with_tip(menu: &gio::Menu, title: &str, action: &str, tip: Option<&str>) {
    let item = gio::MenuItem::new(Some(title), Some(action));
    if let Some(tip) = tip {
        item.set_attribute_value(TOOLTIP, Some(&tip.to_variant()));
    }
    menu.append_item(&item);
}

/// 項目の名前とツールチップ（`menu` の節・下のメニューの中も）。
fn collect_tips(menu: &gio::MenuModel, out: &mut std::collections::HashMap<String, String>) {
    for i in 0..menu.n_items() {
        for link in [gio::MENU_LINK_SECTION, gio::MENU_LINK_SUBMENU] {
            if let Some(child) = menu.item_link(i, link) {
                collect_tips(&child, out);
            }
        }
        let get = |name: &str| {
            menu.item_attribute_value(i, name, Some(glib::VariantTy::STRING))
                .and_then(|v| v.get::<String>())
        };
        if let (Some(label), Some(tip)) = (get(gio::MENU_ATTRIBUTE_LABEL), get(TOOLTIP)) {
            out.insert(label, tip);
        }
    }
}

/// `widget` の下の項目の部品（`GtkModelButton`。名前は `text`）のうち、`tips` にある名前のものにツールチップを
/// 付ける（下のメニューの段も、GTK は最初に作って Stack に入れている）。
fn apply_tips(widget: &gtk::Widget, tips: &std::collections::HashMap<String, String>) {
    let mut child = widget.first_child();
    while let Some(c) = child {
        if c.type_().name() == "GtkModelButton" && c.find_property("text").is_some() {
            let text = c.property::<Option<String>>("text");
            if let Some(tip) = text.and_then(|t| tips.get(&t)) {
                c.set_tooltip_text(Some(tip));
            }
        }
        apply_tips(&c, tips);
        child = c.next_sibling();
    }
}

/// `button` のメニューを出すたびに、[`append_with_tip`] で付けたツールチップを項目に付ける（項目の部品は、
/// メニューの中身が変わると作り直されるので、そのたび）。GTK の内側の部品の形に頼るので、形が違えば何も付かない。
pub fn tooltips_on_show(button: &gtk::MenuButton) {
    let (Some(popover), Some(model)) = (button.popover(), button.menu_model()) else {
        return;
    };
    popover.connect_map(move |p| {
        let p = p.clone().upcast::<gtk::Widget>();
        let model = model.clone();
        // 出すときに名前を合わせる処理（`appmenu::retitle_on_show`）で作り直された後に付ける。
        glib::idle_add_local_once(move || {
            let mut tips = std::collections::HashMap::new();
            collect_tips(&model, &mut tips);
            if !tips.is_empty() {
                apply_tips(&p, &tips);
            }
        });
    });
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
