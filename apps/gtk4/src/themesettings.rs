//! Settings > Theme（macOS 版の `ThemeSettingsView.swift`）: 構文の色。

use std::cell::RefCell;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::settings;
use crate::theme::{self, SyntaxTheme};

/// 欄の行を隠す・出す（節の一覧の行ごと。行の中身だけ隠すと空の行が残る）。
fn show_row(widget: &impl IsA<gtk::Widget>, visible: bool) {
    let widget = widget.upcast_ref::<gtk::Widget>();
    match widget.parent().and_downcast::<gtk::ListBoxRow>() {
        Some(row) => row.set_visible(visible),
        None => widget.set_visible(visible),
    }
}

/// 名前を左、`widget` を右に置いた行。
fn row(title: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = gtk::Label::new(Some(title));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    b.append(&l);
    b.append(widget);
    b
}

/// Settings > Theme のページの節。
pub fn sections() -> Vec<gtk::Box> {
    vec![syntax_section()]
}

/// 構文の色の欄: 明るい外観・暗い外観のテーマ、見本、VSCode のテーマの読み込みと、読み込んだものの一覧。
fn syntax_section() -> gtk::Box {
    let light = gtk::DropDown::from_strings(&[]);
    let dark = gtk::DropDown::from_strings(&[]);
    let preview = gtk::Label::new(None);
    preview.set_xalign(0.0);
    preview.set_selectable(false);
    preview.add_css_class("monospace");
    preview.add_css_class("view");
    preview.set_margin_top(4);
    preview.set_margin_bottom(4);
    let import = gtk::Button::with_label(&tr!("Import VSCode Theme…"));
    import.set_halign(gtk::Align::Start);
    let failure = settings::note("");
    failure.add_css_class("settings-error");
    failure.set_visible(false);
    let imported = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let section = crate::reposettings::section(
        Some(&tr!("Syntax Colors")),
        &[
            row(&tr!("Light appearance"), &light).upcast(),
            row(&tr!("Dark appearance"), &dark).upcast(),
            preview.clone().upcast(),
            import.clone().upcast(),
            settings::note(&tr!(
                "Reads tokenColors from a VSCode color theme (.json). Scopes map to Amon Hen’s 14 kinds, so colors are close but not exact."
            ))
            .upcast(),
            failure.clone().upcast(),
            imported.clone().upcast(),
        ],
    );
    // 一覧の並び（外観ごと）: System、合う外観のテーマ、ほかの外観のテーマ（macOS 版と同じ。間に線）。
    let names: Rc<RefCell<[Vec<String>; 2]>> = Rc::default();
    let lines: [Rc<RefCell<Vec<u32>>>; 2] = [Rc::default(), Rc::default()];
    settings::separate_items(&light, lines[0].clone());
    settings::separate_items(&dark, lines[1].clone());
    let syncing = Rc::new(std::cell::Cell::new(false));
    let refresh: Rc<dyn Fn()> = {
        let (light, dark, preview, imported) = (
            light.clone(),
            dark.clone(),
            preview.clone(),
            imported.clone(),
        );
        let (names, lines, syncing) = (names.clone(), lines.clone(), syncing.clone());
        Rc::new(move || {
            syncing.set(true);
            let themes = theme::all();
            for (i, (dropdown, is_dark)) in [(&light, false), (&dark, true)].into_iter().enumerate()
            {
                let matching: Vec<&SyntaxTheme> =
                    themes[1..].iter().filter(|t| t.dark == is_dark).collect();
                let other: Vec<&SyntaxTheme> =
                    themes[1..].iter().filter(|t| t.dark != is_dark).collect();
                let mut list = vec![theme::SYSTEM.to_owned()];
                list.extend(matching.iter().map(|t| t.name.clone()));
                list.extend(other.iter().map(|t| t.name.clone()));
                *lines[i].borrow_mut() = vec![1, 1 + matching.len() as u32];
                // 一覧が変わったときだけ作り直す（選んだ直後に作り直すと、開いている一覧が選んだものを出せない）。
                if names.borrow()[i] != list {
                    let refs: Vec<&str> = list.iter().map(String::as_str).collect();
                    dropdown.set_model(Some(&gtk::StringList::new(&refs)));
                }
                let chosen = theme::selected(is_dark);
                let index = list.iter().position(|n| *n == chosen).unwrap_or(0) as u32;
                if dropdown.selected() != index {
                    dropdown.set_selected(index);
                }
                names.borrow_mut()[i] = list;
            }
            preview.set_markup(&preview_markup());
            while let Some(child) = imported.first_child() {
                imported.remove(&child);
            }
            for t in theme::imported() {
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                line.append(&gtk::Label::new(Some(&t.name)));
                let kind = gtk::Label::new(Some(&if t.dark { tr!("Dark") } else { tr!("Light") }));
                kind.add_css_class("dim-label");
                kind.set_hexpand(true);
                kind.set_xalign(0.0);
                line.append(&kind);
                let remove = gtk::Button::with_label(&tr!("Remove"));
                let name = t.name.clone();
                remove.connect_clicked(move |_| {
                    let mut themes = theme::imported();
                    themes.retain(|t| t.name != name);
                    theme::set_imported(&themes);
                });
                line.append(&remove);
                imported.append(&line);
            }
            show_row(&imported, imported.first_child().is_some());
            syncing.set(false);
        })
    };
    for (i, (dropdown, key)) in [
        (&light, theme::SYNTAX_THEME_LIGHT),
        (&dark, theme::SYNTAX_THEME_DARK),
    ]
    .into_iter()
    .enumerate()
    {
        let (names, syncing) = (names.clone(), syncing.clone());
        dropdown.connect_selected_notify(move |d| {
            if syncing.get() {
                return;
            }
            // 書くと欄を作り直す（`names` を書き換える）ので、名前を写してから書く。
            let name = names.borrow()[i].get(d.selected() as usize).cloned();
            if let Some(name) = name {
                settings::set_string(key, &name);
            }
        });
    }
    let f = failure.clone();
    import.connect_clicked(move |button| {
        let parent = button.root().and_downcast::<gtk::Window>();
        let failure = f.clone();
        glib::spawn_future_local(async move {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("JSON"));
            filter.add_suffix("json");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder()
                .title(tr!("Import VSCode Theme…"))
                .filters(&filters)
                .modal(true)
                .build();
            let Ok(files) = dialog.open_multiple_future(parent.as_ref()).await else {
                return;
            };
            failure.set_visible(false);
            let mut themes = theme::imported();
            let mut chosen = Vec::new();
            for file in files.iter::<gio::File>().flatten() {
                let Some(path) = file.path() else { continue };
                match theme::import_vscode(&path) {
                    Ok(t) => {
                        themes.retain(|x| x.name != t.name);
                        chosen.push((t.dark, t.name.clone()));
                        themes.push(t);
                    }
                    Err(e) => {
                        failure.set_text(&tr!(
                            "Couldn’t read {file}: {error}",
                            file = path.file_name().unwrap_or_default().to_string_lossy(),
                            error = e
                        ));
                        failure.set_visible(true);
                    }
                }
            }
            theme::set_imported(&themes);
            // 読み込んだテーマを、その外観のテーマにする（macOS 版と同じ）。
            for (dark, name) in chosen {
                settings::set_string(
                    if dark {
                        theme::SYNTAX_THEME_DARK
                    } else {
                        theme::SYNTAX_THEME_LIGHT
                    },
                    &name,
                );
            }
        });
    });
    refresh();
    let r = refresh.clone();
    settings::connect_changed_while(&section, move |key| {
        if matches!(
            key,
            theme::SYNTAX_THEME_LIGHT | theme::SYNTAX_THEME_DARK | theme::IMPORTED_SYNTAX_THEMES
        ) {
            r();
        }
    });
    // 外観（明るい・暗い）を変えたら、見本の色を合わせる。
    let p = preview.downgrade();
    adw::StyleManager::default().connect_dark_notify(move |_| {
        if let Some(p) = p.upgrade() {
            p.set_markup(&preview_markup());
        }
    });
    section
}

/// 見本（短い Rust のコード）を、今の外観のテーマの色で。
fn preview_markup() -> String {
    let dark = adw::StyleManager::default().is_dark();
    let fg = if dark {
        gdk::RGBA::WHITE
    } else {
        gdk::RGBA::BLACK
    };
    use amonhen_layout::SyntaxSpan as K;
    let parts: [(&str, Option<u8>); 23] = [
        ("// Rank the results\n", Some(K::COMMENT)),
        ("pub fn ", Some(K::KEYWORD)),
        ("rank", Some(K::FUNCTION)),
        ("(", None),
        ("hits", Some(K::VARIABLE)),
        (": &[", None),
        ("Hit", Some(K::TYPE)),
        ("]) -> ", None),
        ("usize", Some(K::TYPE)),
        (" {\n    ", None),
        ("let ", Some(K::KEYWORD)),
        ("limit", Some(K::VARIABLE)),
        (" = ", Some(K::OPERATOR)),
        ("10", Some(K::NUMBER)),
        (";\n    ", None),
        ("println!", Some(K::FUNCTION)),
        ("(", None),
        ("\"top {}\\n\"", Some(K::STRING)),
        (", ", None),
        ("self", Some(K::CONSTANT)),
        (".", None),
        ("count", Some(K::PROPERTY)),
        (")\n}", None),
    ];
    let mut out = String::new();
    for (text, kind) in parts {
        let text = glib::markup_escape_text(text);
        match kind.and_then(|k| theme::syntax_color(k, &fg)) {
            Some((r, g, b)) => out.push_str(&format!(
                "<span foreground=\"#{:02x}{:02x}{:02x}\">{text}</span>",
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8
            )),
            None => out.push_str(&text),
        }
    }
    out
}
