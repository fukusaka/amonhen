//! つなぐ必要のあるリポジトリ（daemon 越し・SSH 先）のタブの中身（macOS 版の `Connecting.swift`）。開いてすぐ、
//! リポジトリが入る場所に置いておき、つないでいる間はそのことと Cancel を、つなげなかったら（開けなかったら）
//! エラーと Close Tab・Try Again を出す。つながったら、タブの中身をリポジトリの画面に替える。
//! 起動時に開けなかった手元のリポジトリのタブにも使う。

use std::rc::Rc;

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;

/// 場所の接続先（`ssh://user@host/…` なら `user@host`、手元の共有の daemon なら `daemon`）。手元なら None。
pub fn host(location: &str) -> Option<String> {
    if let Some(rest) = location.strip_prefix("daemon:") {
        return Some(host(rest).map_or_else(|| "daemon".to_owned(), |h| format!("{h} · daemon")));
    }
    let rest = location.strip_prefix("ssh://")?;
    Some(rest.split('/').next().unwrap_or_default().to_owned())
}

/// リポジトリのパス（daemon 越しなら、その向こうのパス）。
pub fn path(location: &str) -> String {
    if let Some(rest) = location.strip_prefix("daemon:") {
        return path(rest);
    }
    if let Some(rest) = location.strip_prefix("ssh://") {
        return rest
            .find('/')
            .map_or_else(String::new, |i| rest[i..].to_owned());
    }
    location.to_owned()
}

/// つないでいる間・つなげなかったときのタブの中身。
pub struct Connecting {
    /// 真ん中に縦に並べる。
    pub root: gtk::Box,
    stack: gtk::Box,
    location: String,
    /// リポジトリの名前（開けなかった手元のリポジトリの見出しに出す）。
    name: String,
    on_close: Rc<dyn Fn()>,
    on_retry: Rc<dyn Fn()>,
}

impl Connecting {
    /// `on_close` は Cancel・Close Tab、`on_retry` は Try Again。
    pub fn new(
        location: &str,
        name: &str,
        on_close: impl Fn() + 'static,
        on_retry: impl Fn() + 'static,
    ) -> Self {
        let stack = gtk::Box::new(gtk::Orientation::Vertical, 10);
        stack.set_halign(gtk::Align::Center);
        stack.set_valign(gtk::Align::Center);
        stack.set_margin_start(20);
        stack.set_margin_end(20);
        stack.set_width_request(-1);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.append(&stack);
        stack.set_vexpand(true);
        // つないでいる間は Esc で Cancel、つなげなかったら Return で Try Again（macOS 版と同じ）。どちらも出す
        // ときにボタンに入るので、この中のキーとして受ける。
        let keys = gtk::EventControllerKey::new();
        // ボタンより先に受ける（Close Tab に入っていても Return は Try Again。macOS 版と同じ）。
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let close: Rc<dyn Fn()> = Rc::new(on_close);
        let retry: Rc<dyn Fn()> = Rc::new(on_retry);
        let s = stack.downgrade();
        let (c, r) = (close.clone(), retry.clone());
        keys.connect_key_pressed(move |_, key, _, _| {
            let failed = s.upgrade().is_some_and(|s| s.widget_name() == "failed");
            match key {
                gtk::gdk::Key::Escape if !failed => c(),
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter if failed => r(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        root.add_controller(keys);
        Self {
            root,
            stack,
            location: location.to_owned(),
            name: name.to_owned(),
            on_close: close,
            on_retry: retry,
        }
    }

    fn host(&self) -> String {
        host(&self.location).unwrap_or_else(|| self.location.clone())
    }

    fn clear(&self) {
        while let Some(child) = self.stack.first_child() {
            self.stack.remove(&child);
        }
    }

    fn path_label(&self) -> gtk::Label {
        let label = gtk::Label::new(Some(&path(&self.location)));
        label.add_css_class("dim-label");
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.set_max_width_chars(70);
        label
    }

    fn title(text: &str) -> gtk::Label {
        let label = gtk::Label::new(Some(text));
        label.add_css_class("heading");
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_justify(gtk::Justification::Center);
        label.set_max_width_chars(60);
        label
    }

    /// つないでいる間（SSH の接続と daemon の送り込みに数秒かかる）。
    pub fn show_connecting(&self) {
        self.clear();
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        heading.set_halign(gtk::Align::Center);
        heading.append(&spinner);
        heading.append(&Self::title(&tr!(
            "Connecting to {host}…",
            host = self.host()
        )));
        let cancel = gtk::Button::with_label(&tr!("Cancel"));
        cancel.set_halign(gtk::Align::Center);
        let f = self.on_close.clone();
        cancel.connect_clicked(move |_| f());
        self.stack.append(&heading);
        self.stack.append(&self.path_label());
        self.stack.append(&cancel);
        self.stack.set_widget_name("connecting");
        cancel.grab_focus();
    }

    /// つなげなかった（開けなかった）。タブは残し、閉じるまで次の起動でも開き直す。
    pub fn show_error(&self, error: &str) {
        self.clear();
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Warning, false);
        let icon_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        icon_box.set_halign(gtk::Align::Center);
        icon_box.append(&icon);
        let title = if amonhen_remote::location::is_remote(&self.location) {
            tr!("Couldn’t connect to {host}", host = self.host())
        } else {
            tr!("Couldn’t open {name}", name = self.name)
        };
        let detail = gtk::Label::new(Some(error));
        detail.add_css_class("dim-label");
        detail.set_wrap(true);
        detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        detail.set_justify(gtk::Justification::Center);
        detail.set_selectable(true);
        detail.set_max_width_chars(65);
        let close = gtk::Button::with_label(&tr!("Close Tab"));
        let f = self.on_close.clone();
        close.connect_clicked(move |_| f());
        let retry = gtk::Button::with_label(&tr!("Try Again"));
        retry.add_css_class("suggested-action");
        let f = self.on_retry.clone();
        retry.connect_clicked(move |_| f());
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::Center);
        buttons.set_margin_top(4);
        buttons.append(&close);
        buttons.append(&retry);
        self.stack.append(&icon_box);
        self.stack.append(&Self::title(&title));
        self.stack.append(&self.path_label());
        self.stack.append(&detail);
        self.stack.append(&buttons);
        self.stack.set_widget_name("failed");
        retry.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_path_of_locations() {
        assert_eq!(
            host("ssh://me@box:22/srv/repo").as_deref(),
            Some("me@box:22")
        );
        assert_eq!(host("daemon:/home/me/repo").as_deref(), Some("daemon"));
        assert_eq!(
            host("daemon:ssh://box/srv/repo").as_deref(),
            Some("box · daemon")
        );
        assert_eq!(host("/home/me/repo"), None);
        assert_eq!(path("ssh://box/srv/repo"), "/srv/repo");
        assert_eq!(path("daemon:ssh://box/srv/repo"), "/srv/repo");
        assert_eq!(path("/home/me/repo"), "/home/me/repo");
    }
}
