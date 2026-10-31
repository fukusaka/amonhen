//! Settings の AI のタブ（macOS 版の `AISettings`）: 使う CLI（既定は使わない）、CLI ごとのモデル、翻訳・解説の言語、
//! 用途ごとのプロンプト（編集・既定に戻す・見本で試す）、リポジトリごとの許可を消す。

use std::rc::Rc;

use amonhen_assist::{Cli, Purpose};
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::assist;

/// 項目の下に添える説明。
fn note(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_max_width_chars(64);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.add_css_class("dim-label");
    l.add_css_class("caption");
    l
}

/// 「名前 … 部品」の行。
fn row(title: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = gtk::Label::new(Some(title));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    b.append(&l);
    b.append(widget);
    b
}

fn heading(title: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(title));
    l.set_xalign(0.0);
    l.add_css_class("heading");
    l
}

/// AI のタブの中身。
pub fn page() -> gtk::Widget {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
    column.set_margin_top(18);
    column.set_margin_bottom(18);
    column.set_margin_start(24);
    column.set_margin_end(24);

    // 使う CLI。
    let choices: Vec<Option<Cli>> = std::iter::once(None)
        .chain(Cli::ALL.into_iter().map(Some))
        .collect();
    let titles: Vec<String> = choices
        .iter()
        .map(|c| c.map_or_else(|| tr!("Don’t Use"), |c| c.title().to_owned()))
        .collect();
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let choice = gtk::DropDown::from_strings(&refs);
    choice.set_selected(
        choices
            .iter()
            .position(|c| *c == assist::selected())
            .unwrap_or(0) as u32,
    );
    let top = gtk::Box::new(gtk::Orientation::Vertical, 6);
    top.append(&row(&tr!("AI to use"), &choice));
    // 選んだ CLI の場所・モデル・言語（CLI を変えたら作り直す）。
    let details = gtk::Box::new(gtk::Orientation::Vertical, 6);
    top.append(&details);
    column.append(&top);

    // 用途ごとのプロンプト。
    let prompts = gtk::Box::new(gtk::Orientation::Vertical, 10);
    column.append(&prompts);

    let forget = gtk::Button::with_label(&tr!("Forget Per-Repository Permissions"));
    forget.set_halign(gtk::Align::Start);
    forget.connect_clicked(|_| assist::forget_repository_decisions());
    let bottom = gtk::Box::new(gtk::Orientation::Vertical, 4);
    bottom.append(&forget);
    bottom.append(&note(&tr!(
        "Forgets the answers to “Use AI in this repository?” and asks again next time."
    )));
    column.append(&bottom);

    let refresh = {
        let (details, prompts) = (details.downgrade(), prompts.downgrade());
        Rc::new(move || {
            if let Some(d) = details.upgrade() {
                fill_details(&d);
            }
            if let Some(p) = prompts.upgrade() {
                fill_prompts(&p);
            }
        })
    };
    refresh();
    let r = refresh.clone();
    choice.connect_selected_notify(move |d| {
        let cli = choices[d.selected() as usize];
        if cli != assist::selected() {
            assist::set_selected(cli);
            r();
        }
    });
    // 外でプロンプトのファイルを書き換えたら、戻ってきたときに読み直す。
    let r = refresh.clone();
    column.connect_realize(move |c| {
        if let Some(window) = c.root().and_downcast::<gtk::Window>() {
            let r = r.clone();
            window.connect_is_active_notify(move |w| {
                if w.is_active() {
                    r();
                }
            });
        }
    });
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&column)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(640)
        .build();
    scrolled.upcast()
}

/// 選んだ CLI の場所・モデル・言語。
fn fill_details(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
    let Some(cli) = assist::selected() else {
        return;
    };
    match assist::executable(cli) {
        Some(path) => {
            let l = note(&path.to_string_lossy());
            l.set_selectable(true);
            b.append(&l);
            // 版は裏で聞く。
            let weak = l.downgrade();
            glib::spawn_future_local(async move {
                let p = path.clone();
                let version = gio::spawn_blocking(move || amonhen_assist::version(&p))
                    .await
                    .ok()
                    .flatten();
                if let (Some(l), Some(v)) = (weak.upgrade(), version) {
                    l.set_text(&tr!(
                        "{path} ({version})",
                        path = path.to_string_lossy(),
                        version = v
                    ));
                }
            });
        }
        None => {
            let l = note(&tr!(
                "Not found. Install {name} ({program}) and sign in with the CLI.",
                name = cli.title(),
                program = cli.program()
            ));
            l.remove_css_class("dim-label");
            l.add_css_class("error");
            b.append(&l);
        }
    }

    // モデル: 一覧から選ぶ。一覧にない名前は「その他…」で書く。
    let models = cli.models(&glib::home_dir());
    let current = assist::model(cli);
    let mut ids: Vec<String> = vec![String::new()];
    let mut titles: Vec<String> = vec![tr!("CLI default")];
    for (id, title) in &models {
        ids.push(id.clone());
        titles.push(format!("{title}  ({id})"));
    }
    titles.push(tr!("Other…"));
    let other = ids.len() as u32;
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let picker = gtk::DropDown::from_strings(&refs);
    let custom = !ids.contains(&current);
    picker.set_selected(if custom {
        other
    } else {
        ids.iter().position(|i| *i == current).unwrap_or(0) as u32
    });
    let name = gtk::Entry::builder()
        .placeholder_text(tr!("CLI default"))
        .text(if custom { current.as_str() } else { "" })
        .hexpand(true)
        .build();
    let name_row = row(&tr!("Model name"), &name);
    name.set_width_chars(28);
    name_row.set_visible(custom);
    b.append(&row(&tr!("Model"), &picker));
    b.append(&name_row);
    let nr = name_row.downgrade();
    picker.connect_selected_notify(move |d| {
        let i = d.selected();
        if let Some(nr) = nr.upgrade() {
            nr.set_visible(i == other);
        }
        if i != other {
            assist::set_model(cli, &ids[i as usize]);
        }
    });
    name.connect_changed(move |e| assist::set_model(cli, e.text().trim()));

    let language = gtk::Entry::builder()
        .text(assist::language())
        .width_chars(16)
        .build();
    language.connect_changed(|e| {
        crate::settings::set_string("assistantLanguage", e.text().trim());
    });
    b.append(&row(
        &tr!("Language for translations and explanations"),
        &language,
    ));
    b.append(&note(&tr!(
        "Asks before sending anything from a repository the first time."
    )));
}

/// 用途ごとのプロンプトの行。
fn fill_prompts(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
    b.append(&heading(&tr!("Prompts by Purpose")));
    let store = assist::prompts();
    let ready = assist::is_ready();
    for purpose in Purpose::ALL {
        let customized = store.is_customized(purpose);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        let title = gtk::Label::new(Some(&purpose.title()));
        title.set_xalign(0.0);
        text.append(&title);
        let vars: Vec<String> = purpose
            .variables()
            .iter()
            .map(|v| format!("{{{{{v}}}}}"))
            .collect();
        text.append(&note(&tr!(
            "Variables: {variables}",
            variables = vars.join(" ")
        )));
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.append(&text);
        if customized {
            let tag = gtk::Label::new(Some(&tr!("Customized")));
            tag.add_css_class("caption");
            tag.add_css_class("warning");
            line.append(&tag);
        }
        let edit = gtk::Button::with_label(&tr!("Edit…"));
        edit.connect_clicked(move |b| {
            let store = assist::prompts();
            match store.editable_file(purpose) {
                Ok(file) => {
                    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&file)));
                    let parent = b.root().and_downcast::<gtk::Window>();
                    launcher.launch(parent.as_ref(), None::<&gio::Cancellable>, |_| {});
                }
                Err(e) => eprintln!("プロンプトを書けない: {e}"),
            }
        });
        let reset = gtk::Button::with_label(&tr!("Restore Default"));
        reset.set_sensitive(customized);
        let parent_box = b.downgrade();
        reset.connect_clicked(move |_| {
            assist::prompts().reset(purpose);
            if let Some(b) = parent_box.upgrade() {
                fill_prompts(&b);
            }
        });
        let try_button = gtk::Button::with_label(&tr!("Try"));
        try_button.set_sensitive(ready);
        try_button.set_tooltip_text(Some(&tr!(
            "Send the current prompt with a sample diff and message, and see the reply."
        )));
        try_button.connect_clicked(move |b| {
            let parent = b.root().and_downcast::<gtk::Window>();
            let p = parent.clone();
            // 見本の値で、今のプロンプト（書き換えていればそれ）を送る。キャッシュは使わない。
            assist::panel(
                parent.as_ref(),
                &tr!("{purpose} (Try with Sample)", purpose = purpose.title()),
                None,
                None,
                assist::work(move |run: assist::Run| {
                    let run = assist::Run {
                        use_cache: false,
                        ..run
                    };
                    assist::run_with(p.clone(), purpose, purpose.sample_variables(), None, run)
                }),
            );
        });
        for w in [&edit, &reset, &try_button] {
            w.set_valign(gtk::Align::Center);
            line.append(w);
        }
        b.append(&line);
    }
    b.append(&note(&tr!(
        "Stored in {dir}/<purpose>.md. A repository’s .amonhen/prompts/<purpose>.md takes precedence (you’re asked to check it the first time). The app appends the reply format (the JSON for resolving conflicts).",
        dir = home_relative(&store.dir)
    )));
}

/// 家のディレクトリの下なら `~/…` にする。
fn home_relative(path: &std::path::Path) -> String {
    match path.strip_prefix(glib::home_dir()) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}
