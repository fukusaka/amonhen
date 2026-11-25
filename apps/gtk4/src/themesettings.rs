//! Settings > Theme（macOS 版の `ThemeSettingsView.swift`）: 窓の背景と構文の色。窓の背景の欄は、リポジトリの
//! 一覧の右クリック「Window Background…」のシート（そのリポジトリだけの上書き）でも同じものを使う。

use std::cell::RefCell;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};

use crate::backdrop::{Backdrop, Placement, Style};
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

/// `min`〜`max` の割合を選ぶ横棒と、その %。
fn percent_scale(min: f64, max: f64, value: f64) -> (gtk::Box, gtk::Scale) {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, 0.05);
    scale.set_value(value);
    scale.set_width_request(200);
    scale.set_draw_value(false);
    let label = gtk::Label::new(None);
    label.set_width_chars(5);
    label.set_xalign(1.0);
    let update = {
        let label = label.clone();
        move |s: &gtk::Scale| label.set_text(&format!("{}%", (s.value() * 100.0).round()))
    };
    update(&scale);
    scale.connect_value_changed(update);
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.append(&scale);
    b.append(&label);
    (b, scale)
}

/// 窓の背景の欄。macOS 版と同じく互いに独立した 3 つの節（窓の透過・背景画像・ツールバー）を縦に並べる。
/// `get` で今の設定を読み、`set` で書く。返す `refresh` は、ほかの所で設定が変わったときに欄を合わせ直す。
pub fn backdrop_section(
    get: Rc<dyn Fn() -> Backdrop>,
    set: Rc<dyn Fn(Backdrop)>,
) -> (gtk::Box, Rc<dyn Fn()>) {
    let styles: Vec<String> = Style::ALL.iter().map(|s| s.title()).collect();
    let refs: Vec<&str> = styles.iter().map(String::as_str).collect();
    let style = gtk::DropDown::from_strings(&refs);
    let style_row = row(&tr!("See through in"), &style);
    let (transparency_box, transparency) = percent_scale(0.1, 0.9, get().transparency);
    let transparency_row = row(&tr!("Transparency"), &transparency_box);

    let image_name = gtk::Label::new(None);
    image_name.add_css_class("dim-label");
    image_name.set_ellipsize(pango::EllipsizeMode::Middle);
    image_name.set_max_width_chars(28);
    let choose = gtk::Button::with_label(&tr!("Choose…"));
    let remove = gtk::Button::with_label(&tr!("Remove"));
    let image_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    image_box.append(&image_name);
    image_box.append(&choose);
    image_box.append(&remove);
    let image_row = row(&tr!("Image"), &image_box);
    let placements: Vec<String> = Placement::ALL.iter().map(|p| p.title()).collect();
    let refs: Vec<&str> = placements.iter().map(String::as_str).collect();
    let placement = gtk::DropDown::from_strings(&refs);
    let placement_row = row(&tr!("Position"), &placement);
    let (image_transparency_box, image_transparency) =
        percent_scale(0.5, 0.95, get().image_transparency);
    let image_transparency_row = row(&tr!("Transparency"), &image_transparency_box);

    let tint = gtk::Switch::new();
    tint.set_valign(gtk::Align::Center);
    let tint_row = row(&tr!("Tint the toolbar"), &tint);
    let color = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    color.dialog().unwrap().set_with_alpha(false);
    let color_row = row(&tr!("Toolbar color"), &color);
    let (tint_box, tint_opacity) = percent_scale(0.1, 1.0, get().toolbar_opacity);
    let tint_opacity_row = row(&tr!("Toolbar tint"), &tint_box);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
    column.append(&crate::reposettings::section(
        Some(&tr!("Window Transparency")),
        &[
            style_row.clone().upcast(),
            transparency_row.clone().upcast(),
        ],
    ));
    column.append(&crate::reposettings::section(
        Some(&tr!("Background Image")),
        &[
            image_row.clone().upcast(),
            placement_row.clone().upcast(),
            image_transparency_row.clone().upcast(),
        ],
    ));
    column.append(&crate::reposettings::section(
        Some(&tr!("Toolbar")),
        &[
            tint_row.clone().upcast(),
            color_row.clone().upcast(),
            tint_opacity_row.clone().upcast(),
        ],
    ));

    // 欄を今の設定に合わせる（合わせている間の変化は、設定に書き戻さない）。
    let syncing = Rc::new(std::cell::Cell::new(false));
    let refresh: Rc<dyn Fn()> = {
        let (style, transparency, image_name, remove, placement) = (
            style.clone(),
            transparency.clone(),
            image_name.clone(),
            remove.clone(),
            placement.clone(),
        );
        let (image_transparency, tint, color, tint_opacity) = (
            image_transparency.clone(),
            tint.clone(),
            color.clone(),
            tint_opacity.clone(),
        );
        let (transparency_row, placement_row, image_transparency_row) = (
            transparency_row.clone(),
            placement_row.clone(),
            image_transparency_row.clone(),
        );
        let (color_row, tint_opacity_row) = (color_row.clone(), tint_opacity_row.clone());
        let (get, syncing) = (get.clone(), syncing.clone());
        Rc::new(move || {
            let b = get();
            syncing.set(true);
            style.set_selected(Style::ALL.iter().position(|s| *s == b.style).unwrap_or(0) as u32);
            if (transparency.value() - b.transparency).abs() > 1e-6 {
                transparency.set_value(b.transparency);
            }
            image_name.set_text(&match &b.image {
                Some(path) => std::path::Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone()),
                None => tr!("None"),
            });
            placement.set_selected(
                Placement::ALL
                    .iter()
                    .position(|p| *p == b.placement)
                    .unwrap_or(0) as u32,
            );
            if (image_transparency.value() - b.image_transparency).abs() > 1e-6 {
                image_transparency.set_value(b.image_transparency);
            }
            tint.set_active(b.toolbar_color.is_some());
            if let Some(rgba) = b
                .toolbar_color
                .as_deref()
                .and_then(theme::parse_hex)
                .map(|(r, g, bl)| gdk::RGBA::new(r, g, bl, 1.0))
                && color.rgba() != rgba
            {
                color.set_rgba(&rgba);
            }
            if (tint_opacity.value() - b.toolbar_opacity).abs() > 1e-6 {
                tint_opacity.set_value(b.toolbar_opacity);
            }
            show_row(&transparency_row, b.style != Style::System);
            remove.set_visible(b.image.is_some());
            show_row(&placement_row, b.image.is_some());
            show_row(&image_transparency_row, b.image.is_some());
            show_row(&color_row, b.toolbar_color.is_some());
            show_row(&tint_opacity_row, b.toolbar_color.is_some());
            syncing.set(false);
        })
    };
    // 欄を変えたら、今の設定に当てて書く。
    let change = {
        let (get, set, refresh, syncing) =
            (get.clone(), set.clone(), refresh.clone(), syncing.clone());
        Rc::new(move |f: &dyn Fn(&mut Backdrop)| {
            if syncing.get() {
                return;
            }
            let mut b = get();
            f(&mut b);
            set(b);
            refresh();
        })
    };
    let c = change.clone();
    style.connect_selected_notify(move |d| {
        let s = Style::ALL[d.selected() as usize];
        c(&|b| b.style = s);
    });
    let c = change.clone();
    transparency.connect_value_changed(move |s| {
        let v = (s.value() * 20.0).round() / 20.0;
        c(&|b| b.transparency = v);
    });
    let c = change.clone();
    placement.connect_selected_notify(move |d| {
        let p = Placement::ALL[d.selected() as usize];
        c(&|b| b.placement = p);
    });
    let c = change.clone();
    image_transparency.connect_value_changed(move |s| {
        let v = (s.value() * 20.0).round() / 20.0;
        c(&|b| b.image_transparency = v);
    });
    let c = change.clone();
    tint_opacity.connect_value_changed(move |s| {
        let v = (s.value() * 20.0).round() / 20.0;
        c(&|b| b.toolbar_opacity = v);
    });
    let c = change.clone();
    tint.connect_active_notify(move |t| {
        let on = t.is_active();
        c(&|b| {
            if on != b.toolbar_color.is_some() {
                // 入れたら、最初は強調の色（macOS 版と同じ）。
                b.toolbar_color = on.then(|| "#3584e4".to_owned());
            }
        });
    });
    let c = change.clone();
    color.connect_rgba_notify(move |button| {
        let hex = theme::to_hex(&button.rgba());
        c(&|b| {
            if b.toolbar_color.is_some() {
                b.toolbar_color = Some(hex.clone());
            }
        });
    });
    let c = change.clone();
    remove.connect_clicked(move |_| c(&|b| b.image = None));
    let c = change.clone();
    choose.connect_clicked(move |button| {
        let parent = button.root().and_downcast::<gtk::Window>();
        let c = c.clone();
        glib::spawn_future_local(async move {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&tr!("Image")));
            filter.add_pixbuf_formats();
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder()
                .title(tr!("Image"))
                .filters(&filters)
                .modal(true)
                .build();
            let Ok(file) = dialog.open_future(parent.as_ref()).await else {
                return;
            };
            if let Some(path) = file.path() {
                let path = path.to_string_lossy().into_owned();
                c(&move |b| b.image = Some(path.clone()));
            }
        });
    });
    refresh();
    (column, refresh)
}

/// Settings > Theme のページの節（窓の背景、構文の色）。
pub fn sections() -> Vec<gtk::Box> {
    let (backdrop, refresh) =
        backdrop_section(Rc::new(Backdrop::app), Rc::new(|b: Backdrop| b.set_app()));
    let weak = backdrop.downgrade();
    settings::connect_changed_while(&backdrop, move |key| {
        if key.starts_with("backdrop") && weak.upgrade().is_some() {
            refresh();
        }
    });
    let (colors, mermaid) = syntax_section();
    vec![backdrop, colors, mermaid, math_section()]
}

/// 数式の欄（式の大きさ・1 つのファイルで描く時間の上限。macOS 版と同じ選択肢）。
fn math_section() -> gtk::Box {
    use amonhen_core::math::{FONT_SCALE, FONT_SCALES, TIME_LIMITS};
    let scale_labels: Vec<String> = FONT_SCALES
        .iter()
        .map(|&scale| {
            let text = format!("{}%", (scale * 100.0).round() as i64);
            if scale == FONT_SCALE {
                tr!("{scale} (KaTeX)", scale = text)
            } else {
                text
            }
        })
        .collect();
    let size =
        gtk::DropDown::from_strings(&scale_labels.iter().map(String::as_str).collect::<Vec<_>>());
    size.set_valign(gtk::Align::Center);
    let current = theme::math_scale();
    size.set_selected(
        FONT_SCALES
            .iter()
            .position(|&s| (s - current).abs() < 1e-9)
            .unwrap_or(2) as u32,
    );
    size.connect_selected_notify(|d| {
        if let Some(&scale) = FONT_SCALES.get(d.selected() as usize)
            && (scale - theme::math_scale()).abs() > 1e-9
        {
            settings::set_string(theme::MATH_SCALE, &scale.to_string());
        }
    });
    let limit_labels: Vec<String> = TIME_LIMITS
        .iter()
        .map(|&s| {
            if s == 0 {
                tr!("No limit")
            } else {
                tr!("{s} s", s = s)
            }
        })
        .collect();
    let limit =
        gtk::DropDown::from_strings(&limit_labels.iter().map(String::as_str).collect::<Vec<_>>());
    limit.set_valign(gtk::Align::Center);
    limit.set_selected(
        TIME_LIMITS
            .iter()
            .position(|&s| s == theme::math_time_limit())
            .unwrap_or(1) as u32,
    );
    limit.connect_selected_notify(|d| {
        if let Some(&seconds) = TIME_LIMITS.get(d.selected() as usize)
            && seconds != theme::math_time_limit()
        {
            settings::set_string(theme::MATH_TIME_LIMIT, &seconds.to_string());
        }
    });
    // Markdown の数式
    crate::reposettings::section(
        Some(&tr!("Math")),
        &[
            row(&tr!("Formula size"), &size).upcast(),
            row(&tr!("Time limit per file"), &limit).upcast(),
            settings::note(&tr!("Formulas past the limit show their TeX.")).upcast(),
        ],
    )
}

/// 構文の色の欄（明るい外観・暗い外観のテーマ、見本、VSCode のテーマの読み込みと、読み込んだものの一覧）と、
/// Mermaid の欄（図の配色・gantt の時間帯・見本・Mermaid のテーマの読み込み）。
fn syntax_section() -> (gtk::Box, gtk::Box) {
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
    let imported = gtk::Box::new(gtk::Orientation::Vertical, 6);
    // 差分は、テーマに関係なく標準の色を使う（構文の色はテーマのまま）。
    let standard = gtk::Switch::new();
    standard.set_valign(gtk::Align::Center);
    standard.set_active(settings::bool(theme::DIFF_STANDARD_COLORS));
    standard.connect_active_notify(|s| {
        if s.is_active() != settings::bool(theme::DIFF_STANDARD_COLORS) {
            settings::set_bool(theme::DIFF_STANDARD_COLORS, s.is_active());
        }
    });
    // Markdown の図の配色（構文の色・pele・Mermaid のテーマ・読み込んだ Mermaid のテーマ）
    let diagram = gtk::DropDown::from_strings(&[]);
    diagram.set_valign(gtk::Align::Center);
    let diagram_keys: Rc<RefCell<Vec<String>>> = Rc::default();
    let diagram_syncing = Rc::new(std::cell::Cell::new(false));
    let mermaid_failure = settings::note("");
    mermaid_failure.add_css_class("settings-error");
    let mermaid_import = gtk::Button::with_label(&tr!("Import Mermaid Theme…"));
    mermaid_import.set_halign(gtk::Align::Start);
    let mermaid_imported = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let diagram_refresh: Rc<dyn Fn()> = {
        let (diagram, keys, syncing, list) = (
            diagram.clone(),
            diagram_keys.clone(),
            diagram_syncing.clone(),
            mermaid_imported.clone(),
        );
        Rc::new(move || {
            use amonhen_core::diagram::{IMPORTED_PREFIX, Scheme};
            syncing.set(true);
            let imported = theme::imported_mermaid();
            let mut entries: Vec<(String, String)> = Scheme::all()
                .into_iter()
                .map(|s| (s.key(), diagram_scheme_label(s)))
                .collect();
            entries.extend(
                imported
                    .iter()
                    .map(|(name, _)| (format!("{IMPORTED_PREFIX}{name}"), name.clone())),
            );
            let new_keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
            if *keys.borrow() != new_keys {
                let labels: Vec<&str> = entries.iter().map(|(_, l)| l.as_str()).collect();
                diagram.set_model(Some(&gtk::StringList::new(&labels)));
                *keys.borrow_mut() = new_keys;
            }
            let current =
                settings::string(theme::DIAGRAM_COLORS).unwrap_or_else(|| "syntax".into());
            let index = keys
                .borrow()
                .iter()
                .position(|k| *k == current)
                .unwrap_or(0) as u32;
            if diagram.selected() != index {
                diagram.set_selected(index);
            }
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            for (name, _) in imported {
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                line.append(&gtk::Label::new(Some(&name)));
                let kind = gtk::Label::new(Some("Mermaid"));
                kind.add_css_class("dim-label");
                kind.set_hexpand(true);
                kind.set_xalign(0.0);
                line.append(&kind);
                let remove = gtk::Button::with_label(&tr!("Remove"));
                remove.connect_clicked(move |_| {
                    let mut themes = theme::imported_mermaid();
                    themes.retain(|(n, _)| *n != name);
                    if settings::string(theme::DIAGRAM_COLORS).as_deref()
                        == Some(&format!("{IMPORTED_PREFIX}{name}"))
                    {
                        settings::set_string(theme::DIAGRAM_COLORS, "syntax");
                    }
                    theme::set_imported_mermaid(&themes);
                });
                line.append(&remove);
                list.append(&line);
            }
            show_row(&list, list.first_child().is_some());
            syncing.set(false);
        })
    };
    {
        let (keys, syncing) = (diagram_keys.clone(), diagram_syncing.clone());
        diagram.connect_selected_notify(move |d| {
            if syncing.get() {
                return;
            }
            let key = keys.borrow().get(d.selected() as usize).cloned();
            if let Some(key) = key
                && settings::string(theme::DIAGRAM_COLORS)
                    .as_deref()
                    .unwrap_or("syntax")
                    != key
            {
                settings::set_string(theme::DIAGRAM_COLORS, &key);
            }
        });
    }
    {
        let failure = mermaid_failure.clone();
        mermaid_import.connect_clicked(move |button| {
            let parent = button.root().and_downcast::<gtk::Window>();
            let failure = failure.clone();
            glib::spawn_future_local(async move {
                let filter = gtk::FileFilter::new();
                filter.set_name(Some("JSON"));
                filter.add_suffix("json");
                let filters = gio::ListStore::new::<gtk::FileFilter>();
                filters.append(&filter);
                let dialog = gtk::FileDialog::builder()
                    .title(tr!("Import Mermaid Theme…"))
                    .filters(&filters)
                    .modal(true)
                    .build();
                let Ok(files) = dialog.open_multiple_future(parent.as_ref()).await else {
                    return;
                };
                show_row(&failure, false);
                let mut themes = theme::imported_mermaid();
                let mut chosen = None;
                for file in files.iter::<gio::File>().flatten() {
                    let Some(path) = file.path() else { continue };
                    let read = std::fs::read_to_string(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|text| amonhen_core::diagram::MermaidConfig::parse(&text));
                    match read {
                        Ok(config) => {
                            let name = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            themes.retain(|(n, _)| *n != name);
                            themes.push((name.clone(), config));
                            chosen = Some(name);
                        }
                        Err(e) => {
                            failure.set_text(&tr!(
                                "Couldn’t read {file}: {error}",
                                file = path.file_name().unwrap_or_default().to_string_lossy(),
                                error = e
                            ));
                            show_row(&failure, true);
                        }
                    }
                }
                theme::set_imported_mermaid(&themes);
                if let Some(name) = chosen {
                    settings::set_string(
                        theme::DIAGRAM_COLORS,
                        &format!("{}{name}", amonhen_core::diagram::IMPORTED_PREFIX),
                    );
                }
            });
        });
    }
    // 図の見本（「図の配色」と今の外観・テーマで色を当てる。変わったら当て直す）
    let diagram_sample = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    diagram_sample.set_margin_start(6);
    diagram_sample.set_margin_bottom(6);
    if let Some(picture) = amonhen_core::diagram::sample()
        .and_then(|svg| crate::markdownview::diagram_picture(svg, None, "flowchart"))
    {
        diagram_sample.append(&picture);
    }
    // gantt の時間帯（OS の時間帯・UTC・IANA の名前。名前で絞り込める）
    let zone_keys: Vec<String> = ["".to_owned(), "UTC".to_owned()]
        .into_iter()
        .chain(theme::zone_names())
        .collect();
    let zone_labels: Vec<String> = zone_keys
        .iter()
        .map(|k| {
            if k.is_empty() {
                tr!("System time zone")
            } else {
                k.clone()
            }
        })
        .collect();
    let zone =
        gtk::DropDown::from_strings(&zone_labels.iter().map(String::as_str).collect::<Vec<_>>());
    zone.set_enable_search(true);
    zone.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        gtk::Expression::NONE,
        "string",
    )));
    zone.set_valign(gtk::Align::Center);
    let current_zone = settings::string(theme::GANTT_TIME_ZONE).unwrap_or_default();
    zone.set_selected(
        zone_keys
            .iter()
            .position(|k| *k == current_zone)
            .unwrap_or(0) as u32,
    );
    zone.connect_selected_notify(move |d| {
        if let Some(key) = zone_keys.get(d.selected() as usize)
            && settings::string(theme::GANTT_TIME_ZONE).unwrap_or_default() != *key
        {
            settings::set_string(theme::GANTT_TIME_ZONE, key);
        }
    });
    let section = crate::reposettings::section(
        Some(&tr!("Color Theme")),
        &[
            row(&tr!("Light appearance"), &light).upcast(),
            row(&tr!("Dark appearance"), &dark).upcast(),
            row(&tr!("Use standard colors for diffs"), &standard).upcast(),
            preview.clone().upcast(),
            import.clone().upcast(),
            failure.clone().upcast(),
            imported.clone().upcast(),
        ],
    );
    // Markdown の Mermaid の図
    let mermaid_section = crate::reposettings::section(
        Some("Mermaid"),
        &[
            row(&tr!("Colors"), &diagram).upcast(),
            diagram_sample.clone().upcast(),
            mermaid_import.clone().upcast(),
            mermaid_failure.clone().upcast(),
            mermaid_imported.clone().upcast(),
            // gantt の日付を数える時間帯
            row(&tr!("Time zone"), &zone).upcast(),
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
            show_row(&failure, false);
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
                        show_row(&failure, true);
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
    diagram_refresh();
    // 知らせは出すまで行ごと隠す（隠した部品の行が空いて見えないように）。
    show_row(&failure, false);
    show_row(&mermaid_failure, false);
    let d = diagram_refresh.clone();
    settings::connect_changed_while(&section, move |key| {
        if matches!(key, theme::DIAGRAM_COLORS | theme::IMPORTED_MERMAID_THEMES) {
            d();
        }
    });
    let r = refresh.clone();
    settings::connect_changed_while(&section, move |key| {
        if matches!(
            key,
            theme::SYNTAX_THEME_LIGHT
                | theme::SYNTAX_THEME_DARK
                | theme::IMPORTED_SYNTAX_THEMES
                | theme::DIFF_STANDARD_COLORS
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
    (section, mermaid_section)
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
    // 差分の色の見本（削除と追加の行、変わった単語）。
    let span = |kind: theme::DiffColor, text: &str| {
        let c = theme::diff_color(kind, &fg);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!(
            "<span background=\"#{:02x}{:02x}{:02x}\" bgalpha=\"{}%\">{text}</span>",
            byte(c.red()),
            byte(c.green()),
            byte(c.blue()),
            ((c.alpha() * 100.0).round() as u32).max(1)
        )
    };
    for (sign, number, line, word) in [
        (
            "-",
            "10",
            theme::DiffColor::Removed,
            theme::DiffColor::RemovedWord,
        ),
        (
            "+",
            "25",
            theme::DiffColor::Added,
            theme::DiffColor::AddedWord,
        ),
    ] {
        let inner = format!("{sign}     let limit = {};          ", span(word, number));
        out.push('\n');
        out.push_str(&span(line, &inner));
    }
    out
}

/// このリポジトリの窓の背景（リポジトリの一覧の右クリック「Window Background…」）。アプリの設定を使うか、上書きするか。
pub fn show_repository(parent: &gtk::Window, location: &str) {
    // 行の余白など、Settings の窓と同じ見た目の規則（Settings を開く前でも）。
    crate::reposettings::install_css();
    let name = crate::bookmarks::display_name(location);
    let sheet = crate::sheets::Sheet::unnamed(Some(parent), &tr!("Window Background"), "");
    let own = Rc::new(RefCell::new(crate::bookmarks::backdrop(location)));
    let toggle = gtk::Switch::new();
    toggle.set_valign(gtk::Align::Center);
    toggle.set_active(own.borrow().is_some());
    let top = crate::reposettings::section(
        None,
        &[
            row(
                &tr!("Use a different background for {name}", name = name),
                &toggle,
            )
            .upcast(),
            settings::note(&tr!("Otherwise this window follows Settings > Theme.")).upcast(),
        ],
    );
    let location = location.to_owned();
    let (backdrop, _) = {
        let (o, l) = (own.clone(), location.clone());
        backdrop_section(
            Rc::new({
                let o = own.clone();
                move || o.borrow().clone().unwrap_or_else(Backdrop::app)
            }),
            Rc::new(move |b: Backdrop| {
                o.replace(Some(b.clone()));
                crate::bookmarks::set_backdrop(&l, Some(b));
            }),
        )
    };
    backdrop.set_visible(toggle.is_active());
    let b = backdrop.downgrade();
    toggle.connect_active_notify(move |t| {
        let value = t.is_active().then(Backdrop::app);
        own.replace(value.clone());
        crate::bookmarks::set_backdrop(&location, value);
        if let Some(b) = b.upgrade() {
            b.set_visible(t.is_active());
        }
    });
    let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
    column.set_width_request(480);
    column.append(&top);
    column.append(&backdrop);
    sheet.add(&column);
    sheet.button(
        &tr!("Done"),
        crate::sheets::Response::Ok,
        Some(crate::sheets::Style::Suggested),
    );
    glib::spawn_future_local(async move {
        sheet.run().await;
    });
}

/// 「図の配色」の項目の名前（macOS 版と同じ）。
fn diagram_scheme_label(scheme: amonhen_core::diagram::Scheme) -> String {
    use amonhen_core::diagram::Scheme;
    match scheme {
        Scheme::Syntax => tr!("Syntax colors"),
        Scheme::Pele => "pele".into(),
        Scheme::Mermaid(None) => tr!("Mermaid (follows the appearance)"),
        Scheme::Mermaid(Some(theme)) => format!("Mermaid {}", theme.name()),
        // 読み込んだテーマは名前で出す（ここには来ない）
        Scheme::Imported => "Mermaid".into(),
    }
}
