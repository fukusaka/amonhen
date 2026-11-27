//! partial clone で中身が手元にないときの帯（macOS 版の `MissingContentsBand`）:「N 個のファイルの中身がまだ手元に
//! ありません［取ってくる］」。読む画面は勝手に取りに行かず、押したときだけ `fetch` で取り、取れたら `done` で出し
//! 直す。次のコミットではまた聞く（取った中身は手元に残るので、同じものでは二度と出ない）。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

/// 中身を取ってくる（裏のスレッドで呼ぶ）。失敗したらその文。
pub type Fetch = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    // macOS 版と同じく、黄色を 12% 敷く。
    provider.load_from_string(".missing-band { background-color: alpha(@warning_color, 0.12); }");
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 「N 個のファイルの中身がまだ手元にありません」の帯（`files` は中身が手元にないファイルの数）。
pub fn files(files: usize, fetch: Fetch, done: impl Fn() + 'static) -> gtk::Widget {
    band(
        &tr!(
            "The contents of {count} files aren’t here yet (partial clone).",
            count = files
        ),
        fetch,
        done,
    )
}

/// 帯の文を `message` にしたもの（1 つのファイルを開いたとき）。下に区切りの線を付けて返す。
pub fn band(message: &str, fetch: Fetch, done: impl Fn() + 'static) -> gtk::Widget {
    install_css();
    let icon = crate::icon::Icon::default();
    icon.set(crate::icon::Kind::CloudDown, false);
    icon.add_css_class("dim-label");
    icon.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(Some(message));
    label.add_css_class("caption");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let button = gtk::Button::with_label(&tr!("Fetch Contents"));
    button.add_css_class("small");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(&tr!(
        "Fetch just these contents from the remote, in one go, and show them."
    )));
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.set_margin_start(10);
    line.set_margin_end(10);
    line.set_margin_top(4);
    line.set_margin_bottom(4);
    line.append(&icon);
    line.append(&label);
    line.append(&button);
    let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
    widget.add_css_class("missing-band");
    widget.append(&line);
    widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    widget.set_accessible_role(gtk::AccessibleRole::Group);
    widget.update_property(&[gtk::accessible::Property::Label(message)]);
    let done: Rc<dyn Fn()> = Rc::new(done);
    let shown = message.to_owned();
    button.connect_clicked(move |button| {
        button.set_sensitive(false);
        label.set_text(&tr!("Fetching the contents…"));
        label.set_tooltip_text(None);
        crate::activity::begin();
        let (fetch, done, shown) = (fetch.clone(), done.clone(), shown.clone());
        let (button, label) = (button.downgrade(), label.downgrade());
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || fetch())
                .await
                .unwrap_or_else(|_| Err(tr!("Couldn’t load the diff")));
            crate::activity::end();
            match result {
                Ok(()) => done(),
                Err(e) => {
                    let (Some(button), Some(label)) = (button.upgrade(), label.upgrade()) else {
                        return;
                    };
                    button.set_sensitive(true);
                    label.set_text(&tr!("Couldn’t fetch the contents: {error}", error = e));
                    label.set_tooltip_text(Some(&shown));
                }
            }
        });
    });
    widget.upcast()
}
