//! 共有のコミットのメッセージのテンプレートの一覧（macOS 版の `TemplateLibrary`・`TemplateLibrarySection`）。
//! この機械のもので、設定（`gtk.ini`）の `commitTemplates` に JSON（id・name・text の並び。macOS 版の UserDefaults と
//! 同じ形）で置く。Settings > Git > Commits で編集し、リポジトリは Settings > Commits で 1 つを選ぶ
//! （`amonhen.commitTemplate` に名前を書く。Amon Hen のコミット欄だけが使う）。

use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::repopages::Table;
use crate::sheets::{Response, Sheet};

/// 設定の鍵（macOS 版の UserDefaults と同じ）。
pub const KEY: &str = "commitTemplates";

/// テンプレートの 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub text: String,
}

/// 一覧（並べた順）。
pub fn all() -> Vec<Template> {
    crate::settings::string(KEY)
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|t| Template {
            id: t["id"].as_str().unwrap_or_default().to_owned(),
            name: t["name"].as_str().unwrap_or_default().to_owned(),
            text: t["text"].as_str().unwrap_or_default().to_owned(),
        })
        .collect()
}

/// 一覧を書く（変わったことは `crate::settings::connect_changed` で知らせる）。
fn save(templates: &[Template]) {
    let list: Vec<Value> = templates
        .iter()
        .map(|t| json!({ "id": t.id, "name": t.name, "text": t.text }))
        .collect();
    crate::settings::set_string(KEY, &Value::Array(list).to_string());
}

/// 名前のテンプレートの本文（なければ None）。
pub fn text_of(name: &str) -> Option<String> {
    all().into_iter().find(|t| t.name == name).map(|t| t.text)
}

/// 一覧の名前（同じ名前が 2 つあれば最初のもの）。
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for t in all() {
        if !names.contains(&t.name) {
            names.push(t.name);
        }
    }
    names
}

fn title_of(t: &Template) -> String {
    if t.name.is_empty() {
        tr!("Untitled")
    } else {
        t.name.clone()
    }
}

/// Settings > Git > Commits の「Shared Templates」: 名前の一覧と ＋／−／Edit…。本文はシートで書く。ボタンの操作
/// （`templates.*`）は、返す部品を含むページに置く。
pub fn section() -> (gtk::Box, gio::SimpleActionGroup) {
    let table = Rc::new(Table::new());
    table.text_column(&tr!("Name"), true, |id, l| {
        let name = all().iter().find(|t| t.id == id).map(title_of);
        l.set_text(&name.unwrap_or_default());
    });
    // 名前だけの一覧なので見出しは出さない（macOS 版の List と同じ）。
    table.hide_header();
    let fill = {
        let t = Rc::downgrade(&table);
        move || {
            if let Some(t) = t.upgrade() {
                let ids: Vec<String> = all().into_iter().map(|t| t.id).collect();
                t.set_ids(&ids);
            }
        }
    };
    fill();
    crate::settings::connect_changed_while(&table.view, move |key| {
        if key == KEY {
            fill();
        }
    });

    let group = gio::SimpleActionGroup::new();
    let edit = gio::SimpleAction::new("edit", None);
    let t = table.clone();
    edit.connect_activate(move |_, _| {
        if let Some(id) = t.selected() {
            edit_sheet(t.view.upcast_ref(), id);
        }
    });
    group.add_action(&edit);
    let remove = gio::SimpleAction::new("remove", None);
    let t = table.clone();
    remove.connect_activate(move |_, _| {
        if let Some(id) = t.selected() {
            let rest: Vec<Template> = all().into_iter().filter(|x| x.id != id).collect();
            save(&rest);
        }
    });
    group.add_action(&remove);
    let add = gio::SimpleAction::new("add", None);
    let t = table.clone();
    add.connect_activate(move |_, _| {
        let new = Template {
            id: glib::uuid_string_random().to_uppercase(),
            name: tr!("Untitled"),
            text: String::new(),
        };
        let mut list = all();
        list.push(new.clone());
        save(&list);
        t.select(&new.id);
        edit_sheet(t.view.upcast_ref(), new.id);
    });
    group.add_action(&add);
    let enable = {
        let (e, r) = (edit.clone(), remove.clone());
        move |on: bool| {
            e.set_enabled(on);
            r.set_enabled(on);
        }
    };
    enable(false);
    table.connect_selected(move |id| enable(id.is_some()));
    let menu = gio::Menu::new();
    menu.append(Some(&tr!("Edit…")), Some("templates.edit"));
    // 行のないところでは出さない（macOS 版は選びが空ならメニューに項目がない）。
    table.row_menu(&menu);
    let e = edit.clone();
    table.view.connect_activate(move |_, _| e.activate(None));

    let plus = gtk::Button::from_icon_name("list-add-symbolic");
    plus.set_action_name(Some("templates.add"));
    let minus = gtk::Button::from_icon_name("list-remove-symbolic");
    minus.set_action_name(Some("templates.remove"));
    let edit_button = gtk::Button::with_label(&tr!("Edit…"));
    edit_button.set_action_name(Some("templates.edit"));
    for b in [&plus, &minus, &edit_button] {
        b.add_css_class("flat");
    }
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    buttons.append(&plus);
    buttons.append(&minus);
    buttons.append(&spacer);
    buttons.append(&edit_button);

    let heading = gtk::Label::new(Some(&tr!("Shared Templates")));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    heading.set_margin_top(12);
    let frame = table.scrolled(140);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.append(&heading);
    column.append(&frame);
    column.append(&buttons);
    (column, group)
}

/// テンプレートの名前と本文を書くシート（見出しはない。書くたびに残し、Done で閉じる。macOS 版と同じ）。
fn edit_sheet(anchor: &gtk::Widget, id: String) {
    let Some(template) = all().into_iter().find(|t| t.id == id) else {
        return;
    };
    let parent = anchor.root().and_downcast::<gtk::Window>();
    glib::spawn_future_local(async move {
        let sheet = Sheet::unnamed(parent.as_ref(), "", "");
        let name = gtk::Entry::builder()
            .text(&template.name)
            .placeholder_text(tr!("Name"))
            .build();
        let text = gtk::TextView::builder()
            .monospace(true)
            .left_margin(6)
            .right_margin(6)
            .top_margin(6)
            .bottom_margin(6)
            .build();
        text.buffer().set_text(&template.text);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&text)
            .min_content_width(480)
            .min_content_height(240)
            .has_frame(true)
            .build();
        sheet.add(&name);
        sheet.add(&scrolled);
        sheet.button(
            &tr!("Done"),
            Response::Ok,
            Some(crate::sheets::Style::Suggested),
        );
        let (n, b) = (name.clone(), text.buffer());
        let write = Rc::new(move || {
            let mut list = all();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.name = n.text().to_string();
                t.text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
                save(&list);
            }
        });
        // 書くたびに残す（一覧の名前もすぐ変わる）。
        let w = write.clone();
        name.connect_changed(move |_| w());
        let w = write.clone();
        text.buffer().connect_changed(move |_| w());
        let _ = sheet.run().await;
    });
}
