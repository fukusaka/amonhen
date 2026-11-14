//! コミットの作者・コミッターのアバター（macOS 版の `Avatars`）。GitHub で分かったアバターの URL があればそれを、
//! なければ Gravatar（メールアドレスの SHA-256 で引く。送るのはハッシュだけ）を読む。読んだ画像はアプリが動いている
//! 間だけ覚え（macOS 版と同じ）、取れなかったものは何度も取りに行かない。読むのは出すときだけ（詳細の欄）、裏で
//! （`curl`）。読み終えたら知らせ、出している所が描き直す。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

/// 画像を読み終えたとき（どのメールアドレスか。小文字）。
type Watcher = Rc<dyn Fn(&str)>;

#[derive(Default)]
struct Cache {
    images: HashMap<String, gdk::Texture>,
    /// GitHub のコミットの作者のアバター（メールアドレス → URL）。Gravatar より優先する。
    github: HashMap<String, String>,
    loading: HashSet<String>,
    /// 取れなかったもの（ネットワークがないなど）。
    failed: HashSet<String>,
    /// 読み終えたことを聞く所（持ち主がなくなったら外す）。
    watchers: Vec<(glib::WeakRef<glib::Object>, Watcher)>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::default();
}

fn key_of(email: &str) -> String {
    email.trim().to_lowercase()
}

/// 画像を読み終えたら `f(メールアドレス)` を呼ぶ（`owner` がある間だけ。なくなったら外す）。
fn connect_loaded_while(owner: &impl IsA<glib::Object>, f: impl Fn(&str) + 'static) {
    let owner = owner.upcast_ref::<glib::Object>().downgrade();
    CACHE.with(|c| c.borrow_mut().watchers.push((owner, Rc::new(f))));
}

fn notify(keys: &[String]) {
    let watchers: Vec<Watcher> = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.watchers.retain(|(owner, _)| owner.upgrade().is_some());
        c.watchers.iter().map(|(_, f)| f.clone()).collect()
    });
    for key in keys {
        for f in &watchers {
            f(key);
        }
    }
}

/// GitHub で分かったアバターを使う。Gravatar で読んでいたものは読み直す。
pub fn use_github(urls: HashMap<String, String>) {
    let changed: Vec<String> = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let mut changed = Vec::new();
        for (key, url) in urls {
            if c.github.get(&key) != Some(&url) {
                c.images.remove(&key);
                c.failed.remove(&key);
                c.github.insert(key.clone(), url);
                changed.push(key);
            }
        }
        changed
    });
    // 描き直すと texture が呼ばれ、GitHub から読み始める。
    notify(&changed);
}

/// GitHub のアバターを使うのをやめる（Gravatar に戻す）。
pub fn forget_github() {
    let keys: Vec<String> = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let keys: Vec<String> = c.github.drain().map(|(k, _)| k).collect();
        for key in &keys {
            c.images.remove(key);
            c.failed.remove(key);
        }
        keys
    });
    notify(&keys);
}

/// 覚えていれば画像を返す。まだなら取りに行き（Settings > Appearance の Show avatars のときだけ）、None を返す。
pub fn texture(email: &str) -> Option<gdk::Texture> {
    let key = key_of(email);
    let url = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(t) = c.images.get(&key) {
            return Err(Some(t.clone()));
        }
        if !shown() || key.is_empty() || c.loading.contains(&key) || c.failed.contains(&key) {
            return Err(None);
        }
        c.loading.insert(key.clone());
        // GitHub のアバターを知っていればそれを、なければ Gravatar（登録していない人は、ハッシュから作る模様）。
        // 高解像度の画面のため 2 倍の大きさで取る。
        Ok(c.github.get(&key).cloned().unwrap_or_else(|| {
            let hash = glib::compute_checksum_for_string(glib::ChecksumType::Sha256, &key)
                .map(|h| h.to_string())
                .unwrap_or_default();
            format!("https://www.gravatar.com/avatar/{hash}?s=96&d=identicon")
        }))
    });
    let url = match url {
        Ok(url) => url,
        Err(found) => return found,
    };
    glib::spawn_future_local(async move {
        let bytes = gio::spawn_blocking(move || fetch(&url))
            .await
            .ok()
            .flatten();
        let texture =
            bytes.and_then(|b| gdk::Texture::from_bytes(&glib::Bytes::from_owned(b)).ok());
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            c.loading.remove(&key);
            match &texture {
                Some(t) => {
                    c.images.insert(key.clone(), t.clone());
                }
                None => {
                    c.failed.insert(key.clone());
                }
            }
        });
        if texture.is_some() {
            notify(std::slice::from_ref(&key));
        }
    });
    None
}

/// アバターを出すか（Settings > Appearance）。
pub fn shown() -> bool {
    crate::settings::bool(crate::settings::SHOW_AVATARS)
}

/// `url` の中身（裏の糸で）。取れなければ None。`curl` で取り、`curl` がなければ GIO（gvfs）で取る
/// （`curl` のない環境で、何も言わずにどのアバターも出なくならないように）。
fn fetch(url: &str) -> Option<Vec<u8>> {
    match amonhen_model::process::command("curl")
        .args(["-fsSL", "--max-time", "15", url])
        .output()
    {
        Ok(out) => (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout),
        Err(_) => {
            let (bytes, _) = gio::File::for_uri(url)
                .load_contents(None::<&gio::Cancellable>)
                .ok()?;
            (!bytes.is_empty()).then(|| bytes.to_vec())
        }
    }
}

fn install_css() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".amonhen-avatar { border-radius: 9999px; background-color: alpha(gray, 0.35); }
         .amonhen-avatar label { color: white; font-weight: 600; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 丸いアバターの部品。まだ読んでいなければ、名前の頭文字の丸。
pub struct Avatar {
    pub root: gtk::Box,
    size: i32,
    image: gtk::Image,
    initial: gtk::Label,
    person: RefCell<(String, String)>,
}

impl Avatar {
    pub fn new(size: i32) -> Rc<Self> {
        install_css();
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("amonhen-avatar");
        root.set_overflow(gtk::Overflow::Hidden);
        root.set_size_request(size, size);
        root.set_valign(gtk::Align::Start);
        root.set_halign(gtk::Align::Start);
        let image = gtk::Image::new();
        image.set_pixel_size(size);
        let initial = gtk::Label::new(None);
        initial.set_halign(gtk::Align::Center);
        initial.set_size_request(size, size);
        let attrs = gtk::pango::AttrList::new();
        attrs.insert(gtk::pango::AttrSize::new_size_absolute(
            size * gtk::pango::SCALE / 2,
        ));
        initial.set_attributes(Some(&attrs));
        root.append(&image);
        root.append(&initial);
        let avatar = Rc::new(Self {
            root,
            size,
            image,
            initial,
            person: RefCell::default(),
        });
        // 読み終えたら、出している人なら描き直す。
        let weak = Rc::downgrade(&avatar);
        connect_loaded_while(&avatar.root, move |key| {
            if let Some(a) = weak.upgrade()
                && key_of(&a.person.borrow().1) == key
            {
                a.update();
            }
        });
        let weak = Rc::downgrade(&avatar);
        crate::settings::connect_changed_while(&avatar.root, move |key| {
            if key == crate::settings::SHOW_AVATARS
                && let Some(a) = weak.upgrade()
            {
                a.update();
            }
        });
        avatar
    }

    /// `name`・`email` の人を出す。
    pub fn show(&self, name: &str, email: &str) {
        self.person.replace((name.to_owned(), email.to_owned()));
        self.update();
    }

    fn update(&self) {
        self.root.set_visible(shown());
        if !shown() {
            return;
        }
        let (name, email) = self.person.borrow().clone();
        match texture(&email) {
            Some(t) => {
                self.image.set_paintable(Some(&t));
                self.image.set_pixel_size(self.size);
                self.image.set_visible(true);
                self.initial.set_visible(false);
            }
            None => {
                let initial: String = name
                    .trim()
                    .chars()
                    .take(1)
                    .flat_map(char::to_uppercase)
                    .collect();
                self.initial.set_text(&initial);
                self.image.set_visible(false);
                self.initial.set_visible(true);
            }
        }
    }
}
