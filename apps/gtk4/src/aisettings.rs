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

/// 枠で囲んだ行の一覧（macOS 版の `.grouped` の Form の Section。Settings のほかのタブと同じ見た目）。行は
/// `add` で足す。
fn boxed_list() -> (gtk::ListBox, gtk::Frame) {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_show_separators(true);
    list.add_css_class("settings-list");
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&list));
    (list, frame)
}

/// 枠の一覧に 1 行足す（隠すときは返した行を隠す。中身だけ隠すと空の行と線が残る）。
fn add(list: &gtk::ListBox, widget: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_child(Some(widget));
    list.append(&row);
    row
}

/// AI のタブの中身。
pub fn page() -> gtk::Widget {
    // 余白は Settings のほかのタブと同じ。
    let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
    column.set_margin_top(18);
    column.set_margin_bottom(18);
    column.set_margin_start(20);
    column.set_margin_end(20);

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
    // 1 つ目の節: 使う CLI と、選んだ CLI の場所・モデル・言語（CLI を変えたら 2 行目から作り直す）。枠の下に
    // 送る前に聞くことの説明（macOS 版の Section の footer）。
    let top = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let (details, frame) = boxed_list();
    add(&details, &row(&tr!("AI to use"), &choice));
    top.append(&frame);
    let footer = note(&tr!(
        "Asks before sending anything from a repository the first time."
    ));
    footer.set_margin_start(8);
    top.append(&footer);
    column.append(&top);

    // 用途ごとのプロンプト（見出しと枠の節。印が変わったら作り直す）。
    let prompts = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.append(&prompts);

    let forget = gtk::Button::with_label(&tr!("Forget Per-Repository Permissions"));
    forget.set_halign(gtk::Align::Start);
    forget.connect_clicked(|_| assist::forget_repository_decisions());
    // 説明は付けない（macOS 版と同じ）。枠の節に 1 行で。
    let (list, frame) = boxed_list();
    add(&list, &forget);
    column.append(&frame);

    let refresh = {
        let (details, prompts, footer) =
            (details.downgrade(), prompts.downgrade(), footer.downgrade());
        Rc::new(move || {
            if let Some(d) = details.upgrade() {
                fill_details(&d);
            }
            if let Some(f) = footer.upgrade() {
                f.set_visible(assist::selected().is_some());
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
    // 外でプロンプトのファイルを作ったり消したりしたら、戻ってきたときに「書き換えた」の印を読み直す。作り直すのは
    // 用途ごとのプロンプトの行だけで、印が変わったときだけ（モデルの名前・言語の欄は作り直さない。macOS 版と
    // 同じく、打っている途中の欄から入力を取らない）。
    let p = prompts.downgrade();
    column.connect_realize(move |c| {
        if let Some(window) = c.root().and_downcast::<gtk::Window>() {
            let p = p.clone();
            window.connect_is_active_notify(move |w| {
                if w.is_active()
                    && let Some(p) = p.upgrade()
                    && shown_customized(&p) != customized()
                {
                    fill_prompts(&p);
                }
            });
        }
    });
    // スクロールは設定の窓のもの（macOS 版と同じく、ページ全体が窓と一緒にスクロールする）。
    column.upcast()
}

/// 選んだ CLI の場所・モデル・言語（枠の一覧の 2 行目から。1 行目は使う CLI）。
fn fill_details(b: &gtk::ListBox) {
    while let Some(c) = b.row_at_index(1) {
        b.remove(&c);
    }
    let Some(cli) = assist::selected() else {
        return;
    };
    match assist::executable(cli) {
        Some(path) => {
            let l = note(&path.to_string_lossy());
            l.set_selectable(true);
            add(b, &l);
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
            add(b, &l);
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
    // Other… の上に線（macOS 版の Divider）。
    crate::settings::separate_items(
        &picker,
        std::rc::Rc::new(std::cell::RefCell::new(vec![other])),
    );
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
    name.set_width_chars(28);
    add(b, &row(&tr!("Model"), &picker));
    let name_row = add(b, &row(&tr!("Model name"), &name));
    name_row.set_visible(custom);
    let (nr, nm) = (name_row.downgrade(), name.downgrade());
    picker.connect_selected_notify(move |d| {
        let i = d.selected();
        if let Some(nr) = nr.upgrade() {
            nr.set_visible(i == other);
        }
        // 欄には今のモデルの名前を入れておく（macOS 版は欄が設定そのもの）。
        if i == other
            && let Some(nm) = nm.upgrade()
        {
            nm.set_text(&assist::model(cli));
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
    add(
        b,
        &row(
            &tr!("Language for translations and explanations"),
            &language,
        ),
    );
}

/// 書き換えた（ファイルのある）用途。
fn customized() -> Vec<bool> {
    let store = assist::prompts();
    Purpose::ALL
        .into_iter()
        .map(|p| store.is_customized(p))
        .collect()
}

/// 用途ごとのプロンプトの行で出している「書き換えた」の印。
fn shown_customized(b: &gtk::Box) -> Vec<bool> {
    // SAFETY: 印は `fill_prompts` が `Vec<bool>` で付けたものだけ。
    unsafe {
        b.data::<Vec<bool>>(CUSTOMIZED)
            .map(|v| v.as_ref().clone())
            .unwrap_or_default()
    }
}

const CUSTOMIZED: &str = "amonhen-customized";

/// 用途ごとのプロンプトの行。
fn fill_prompts(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
    // SAFETY: 読むのは `shown_customized` だけで、同じ型。
    unsafe {
        b.set_data(CUSTOMIZED, customized());
    }
    let heading = gtk::Label::new(Some(&tr!("Prompts by Purpose")));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    b.append(&heading);
    let (list, frame) = boxed_list();
    b.append(&frame);
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
        add(&list, &line);
    }
    // macOS 版と同じ文（置き場所はこの FE のもの）。節の中の 1 行。
    add(
        &list,
        &note(&tr!("Stored in {dir}.", dir = home_relative(&store.dir))),
    );
}

/// 家のディレクトリの下なら `~/…` にする。
fn home_relative(path: &std::path::Path) -> String {
    match path.strip_prefix(glib::home_dir()) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}
