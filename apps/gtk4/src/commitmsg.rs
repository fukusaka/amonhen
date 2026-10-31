//! コミットのメッセージの入力の補助（macOS 版の `WorkingCopyViewController` の「コミットの入力の補助」）:
//! - 書きかけの下書き（コミットするまで、アプリを終えても残る）と、なければテンプレート
//! - 最近のコミットのメッセージ（ボタンのメニューと、件名の欄の Alt+↑・Alt+↓）
//! - Conventional Commits の型、Co-authored-by・Signed-off-by のトレーラー
//! - Commit ▾ の Fixup Commit For・Squash Commit For

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::menu::{Item, action};

/// 最近のコミット（メッセージ、作者、メール）。
type Recent = Vec<(String, String, String)>;

const TYPES: [&str; 11] = [
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

pub struct MessageAids {
    /// 最近のメッセージを入れるボタン。
    pub history: gtk::Button,
    /// 型・トレーラーを入れるボタン。
    pub insert: gtk::Button,
    subject: gtk::Entry,
    body: gtk::TextView,
    amend: gtk::CheckButton,
    repo: RefCell<Option<Arc<Repository>>>,
    recent: RefCell<Recent>,
    /// Alt+↑・Alt+↓ で呼び出している最近のメッセージ（-1 は呼び出していない）と、呼び出す前の下書き。
    cursor: Cell<isize>,
    before_recall: RefCell<Option<(String, String)>>,
    /// 下書きを覚えないでいる間（中身を入れ替えているとき）。
    quiet: Cell<bool>,
}

fn icon_button(icon: &str, fallback: &str, tip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    if gtk::IconTheme::for_display(&button.display()).has_icon(icon) {
        button.set_icon_name(icon);
    } else {
        button.set_label(fallback);
    }
    button.set_tooltip_text(Some(tip));
    button.add_css_class("flat");
    button
}

impl MessageAids {
    pub fn new(subject: &gtk::Entry, body: &gtk::TextView, amend: &gtk::CheckButton) -> Rc<Self> {
        let history = icon_button(
            "document-open-recent-symbolic",
            "⟲",
            &tr!(
                "Insert the message of a recent commit (Alt+Up and Alt+Down in the Summary field also work)"
            ),
        );
        let insert = icon_button(
            "list-add-symbolic",
            "+",
            &tr!("Insert a Conventional Commits type, Co-authored-by or Signed-off-by"),
        );
        let this = Rc::new(Self {
            history,
            insert,
            subject: subject.clone(),
            body: body.clone(),
            amend: amend.clone(),
            repo: RefCell::default(),
            recent: RefCell::default(),
            cursor: Cell::new(-1),
            before_recall: RefCell::default(),
            quiet: Cell::new(false),
        });
        let w = Rc::downgrade(&this);
        this.history.connect_clicked(move |b| {
            if let Some(this) = w.upgrade() {
                crate::menu::popup(
                    b.upcast_ref::<gtk::Widget>(),
                    0.0,
                    b.height() as f64,
                    this.history_menu(),
                );
            }
        });
        let w = Rc::downgrade(&this);
        this.insert.connect_clicked(move |b| {
            if let Some(this) = w.upgrade() {
                crate::menu::popup(
                    b.upcast_ref::<gtk::Widget>(),
                    0.0,
                    b.height() as f64,
                    this.insert_menu(),
                );
            }
        });
        // 打つたびに下書きを覚える。
        let w = Rc::downgrade(&this);
        subject.connect_changed(move |_| {
            if let Some(this) = w.upgrade() {
                this.save_draft();
            }
        });
        let w = Rc::downgrade(&this);
        body.buffer().connect_changed(move |_| {
            if let Some(this) = w.upgrade() {
                this.save_draft();
            }
        });
        // 件名の欄の Alt+↑・Alt+↓ で、最近のメッセージを順に呼び出す。
        let keys = gtk::EventControllerKey::new();
        let w = Rc::downgrade(&this);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(this) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if state != gtk::gdk::ModifierType::ALT_MASK {
                return glib::Propagation::Proceed;
            }
            let step = match key {
                gtk::gdk::Key::Up => 1,
                gtk::gdk::Key::Down => -1,
                _ => return glib::Propagation::Proceed,
            };
            this.recall(step);
            glib::Propagation::Stop
        });
        subject.add_controller(keys);
        this
    }

    /// 下書きを覚える鍵（リポジトリごと）。
    fn draft_key(&self) -> Option<String> {
        let repo = self.repo.borrow().clone()?;
        let place = repo
            .paths()
            .workdir
            .map_or_else(|| "remote".to_owned(), |p| p.display().to_string());
        // KeyFile の鍵に使えない文字を避けるため、16 進にする。
        let hex: String = place.bytes().map(|b| format!("{b:02x}")).collect();
        Some(format!("commitDraft.{hex}"))
    }

    fn texts(&self) -> (String, String) {
        let b = self.body.buffer();
        (
            self.subject.text().to_string(),
            b.text(&b.start_iter(), &b.end_iter(), false).to_string(),
        )
    }

    fn set_texts(&self, subject: &str, body: &str) {
        self.quiet.set(true);
        self.subject.set_text(subject);
        self.body.buffer().set_text(body);
        self.quiet.set(false);
    }

    /// 書きかけのメッセージを覚える（コミットするまで、アプリを終えても残る）。
    fn save_draft(&self) {
        if self.quiet.get() || self.cursor.get() >= 0 {
            return;
        }
        let Some(key) = self.draft_key() else { return };
        let (subject, body) = self.texts();
        let value = if subject.trim().is_empty() && body.trim().is_empty() {
            String::new()
        } else {
            json!([subject, body]).to_string()
        };
        if crate::settings::string(&key).unwrap_or_default() != value {
            crate::settings::set_string(&key, &value);
        }
    }

    /// コミットした: 下書きを消す。
    pub fn clear_draft(&self) {
        if let Some(key) = self.draft_key() {
            crate::settings::set_string(&key, "");
        }
        self.cursor.set(-1);
        self.before_recall.replace(None);
    }

    /// リポジトリを開いた（読み直した）: 最近のコミットを読み、下書きか、なければテンプレートを入れる。
    pub fn set_repo(self: &Rc<Self>, repo: Arc<Repository>) {
        let first = self.repo.borrow().is_none();
        self.repo.replace(Some(repo.clone()));
        if first {
            self.restore_draft_or_template();
        }
        // 最近のコミット 30 個のメッセージと作者（一覧と Co-authored-by の候補）。
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let recent = gio::spawn_blocking(move || {
                let Some(history) = repo.history() else {
                    return Vec::new();
                };
                history
                    .graph
                    .ids()
                    .iter()
                    .take(30)
                    .filter_map(|id| {
                        let info = repo.commit_info(id).ok()?;
                        Some((
                            info["message"].as_str()?.trim().to_owned(),
                            info["author"].as_str().unwrap_or_default().to_owned(),
                            info["authorEmail"].as_str().unwrap_or_default().to_owned(),
                        ))
                    })
                    .collect()
            })
            .await
            .unwrap_or_default();
            if let Some(this) = w.upgrade() {
                this.recent.replace(recent);
            }
        });
    }

    /// 下書きがあればそれを、なければテンプレートを入れる: `commit.template`（このリポジトリ専用のものはコアから
    /// 読むので daemon・SSH 越しでも入る。ほかのファイルは手元のリポジトリだけ）。どれも # の行は除く。
    fn restore_draft_or_template(self: &Rc<Self>) {
        if let Some(key) = self.draft_key()
            && let Some(draft) = crate::settings::string(&key)
            && let Ok(serde_json::Value::Array(parts)) = serde_json::from_str(&draft)
            && parts.len() == 2
        {
            self.set_texts(
                parts[0].as_str().unwrap_or_default(),
                parts[1].as_str().unwrap_or_default(),
            );
            return;
        }
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let config = crate::sheets::run(&repo, json!({ "op": "configList" })).await;
            let path = config["info"]["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .rev()
                .find(|e| {
                    e["key"]
                        .as_str()
                        .is_some_and(|k| k.eq_ignore_ascii_case("commit.template"))
                })
                .and_then(|e| e["value"].as_str().map(str::to_owned));
            let Some(path) = path.filter(|p| !p.is_empty()) else {
                return;
            };
            let text = if path.ends_with("/amonhen/commit-template") {
                crate::sheets::run(&repo, json!({ "op": "commitTemplate" })).await["info"]["text"]
                    .as_str()
                    .map(str::to_owned)
            } else {
                let workdir = repo.paths().workdir;
                let home = glib::home_dir();
                let file = match path.strip_prefix("~/") {
                    Some(rest) => home.join(rest),
                    None if path.starts_with('/') => path.into(),
                    None => match workdir {
                        Some(w) => w.join(&path),
                        None => return,
                    },
                };
                std::fs::read_to_string(file).ok()
            };
            let (Some(this), Some(text)) = (w.upgrade(), text) else {
                return;
            };
            let (subject, body) = this.texts();
            if !subject.is_empty() || !body.is_empty() {
                return;
            }
            let text: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
            let text = text.join("\n");
            let text = text.trim();
            let (s, b) = text.split_once('\n').unwrap_or((text, ""));
            this.set_texts(s, b.trim_matches('\n'));
        });
    }

    fn use_message(&self, text: &str) {
        let (s, b) = text.split_once('\n').unwrap_or((text, ""));
        self.subject.set_text(s);
        self.body.buffer().set_text(b.trim());
    }

    /// Alt+↑（`step` = 1、古い方へ）・Alt+↓ で、最近のメッセージを順に呼び出す。戻りきったら呼び出す前の下書き。
    fn recall(&self, step: isize) {
        let count = self.recent.borrow().len() as isize;
        if count == 0 {
            return;
        }
        if self.cursor.get() < 0 {
            self.before_recall.replace(Some(self.texts()));
        }
        let cursor = (self.cursor.get() + step).clamp(-1, count - 1);
        self.cursor.set(cursor);
        if cursor < 0 {
            if let Some((s, b)) = self.before_recall.borrow().clone() {
                self.set_texts(&s, &b);
            }
        } else {
            let text = self.recent.borrow()[cursor as usize].0.clone();
            self.quiet.set(true);
            self.use_message(&text);
            self.quiet.set(false);
        }
        self.subject.set_position(-1);
    }

    fn history_menu(self: &Rc<Self>) -> Vec<Item> {
        let mut seen = Vec::new();
        let mut items = Vec::new();
        for (message, _, _) in self.recent.borrow().iter() {
            if seen.contains(message) {
                continue;
            }
            seen.push(message.clone());
            let subject: String = message
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(80)
                .collect();
            let (w, m) = (Rc::downgrade(self), message.clone());
            items.push(action(subject, move || {
                if let Some(this) = w.upgrade() {
                    this.use_message(&m);
                }
            }));
        }
        if items.is_empty() {
            items.push(action(tr!("Not loaded yet"), || {}));
        }
        items
    }

    fn insert_menu(self: &Rc<Self>) -> Vec<Item> {
        let mut items = Vec::new();
        for t in TYPES {
            let w = Rc::downgrade(self);
            items.push(action(format!("{t}:"), move || {
                if let Some(this) = w.upgrade() {
                    this.set_type(t);
                }
            }));
        }
        items.push(Item::Separator);
        let mut seen: Vec<String> = Vec::new();
        for (_, author, email) in self.recent.borrow().iter() {
            if email.is_empty() || seen.contains(&email.to_lowercase()) {
                continue;
            }
            seen.push(email.to_lowercase());
            let (w, line) = (
                Rc::downgrade(self),
                format!("Co-authored-by: {author} <{email}>"),
            );
            items.push(action(
                format!("Co-authored-by: {author} <{email}>"),
                move || {
                    if let Some(this) = w.upgrade() {
                        this.add_trailer(&line);
                    }
                },
            ));
        }
        items.push(Item::Separator);
        let w = Rc::downgrade(self);
        items.push(action(tr!("Signed-off-by (Me)"), move || {
            if let Some(this) = w.upgrade() {
                this.add_sign_off();
            }
        }));
        items
    }

    /// 件名の先頭を `type: ` にする（もう型があれば置き換える）。
    fn set_type(&self, t: &str) {
        let s = self.subject.text().to_string();
        // `type(scope)!: ` を外す。
        let stripped = match s.split_once(':') {
            Some((head, rest))
                if !head.is_empty()
                    && head
                        .trim_end_matches('!')
                        .split('(')
                        .next()
                        .is_some_and(|w| {
                            w.chars().all(|c| c.is_ascii_lowercase()) && !w.is_empty()
                        }) =>
            {
                rest.trim_start().to_owned()
            }
            _ => s,
        };
        self.subject.set_text(&format!("{t}: {stripped}"));
        self.subject.grab_focus();
        self.subject.set_position(-1);
    }

    /// 本文の末尾にトレーラーを足す（もうあれば足さない）。
    fn add_trailer(&self, line: &str) {
        let (_, body) = self.texts();
        if body.contains(line) {
            return;
        }
        let body = body.trim().to_owned();
        let last = body.lines().last().unwrap_or_default();
        // トレーラーは本文と空行で分ける（前の行がトレーラーなら続ける）。
        let last_is_trailer = last.split_once(": ").is_some_and(|(k, _)| {
            !k.is_empty() && k.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
        });
        let text = if body.is_empty() {
            line.to_owned()
        } else if last_is_trailer {
            format!("{body}\n{line}")
        } else {
            format!("{body}\n\n{line}")
        };
        self.body.buffer().set_text(&text);
    }

    fn add_sign_off(self: &Rc<Self>) {
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let config = crate::sheets::run(&repo, json!({ "op": "configList" })).await;
            let value = |key: &str| {
                config["info"]["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .rev()
                    .find(|e| {
                        e["key"]
                            .as_str()
                            .is_some_and(|k| k.eq_ignore_ascii_case(key))
                    })
                    .and_then(|e| e["value"].as_str().map(str::to_owned))
            };
            let (Some(this), Some(name), Some(email)) =
                (w.upgrade(), value("user.name"), value("user.email"))
            else {
                return;
            };
            this.add_trailer(&format!("Signed-off-by: {name} <{email}>"));
        });
    }

    /// Commit ▾ のメニューに、Fixup Commit For・Squash Commit For（最近のコミットの件名）を、開くたびに作って
    /// 足す。`base` はもとの項目（Commit and Push・Amend Last Commit）。
    pub fn install_fixups(self: &Rc<Self>, button: &gtk::MenuButton, base: gio::Menu) {
        let group = gio::SimpleActionGroup::new();
        for (name, prefix, keep_body) in [("fixup", "fixup! ", false), ("squash", "squash! ", true)]
        {
            let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
            let w = Rc::downgrade(self);
            action.connect_activate(move |_, value| {
                let (Some(this), Some(subject)) =
                    (w.upgrade(), value.and_then(|v| v.get::<String>()))
                else {
                    return;
                };
                // fixup! は件名だけ（元のメッセージを残す）、squash! は本文に書き足せる。
                this.subject.set_text(&format!("{prefix}{subject}"));
                if !keep_body {
                    this.body.buffer().set_text("");
                }
                this.amend.set_active(false);
                if keep_body {
                    this.body.grab_focus();
                } else {
                    this.subject.grab_focus();
                }
            });
            group.add_action(&action);
        }
        button.insert_action_group("message", Some(&group));
        let w = Rc::downgrade(self);
        button.set_create_popup_func(move |button| {
            let Some(this) = w.upgrade() else { return };
            let menu = gio::Menu::new();
            menu.append_section(None, &base);
            let mut subjects: Vec<String> = Vec::new();
            for (message, _, _) in this.recent.borrow().iter() {
                let subject = message.lines().next().unwrap_or_default().to_owned();
                // fixup! を重ねない（rebase --autosquash は元のコミットの件名で探す）。
                if subject.is_empty()
                    || subject.starts_with("fixup! ")
                    || subject.starts_with("squash! ")
                    || subjects.contains(&subject)
                {
                    continue;
                }
                subjects.push(subject);
                if subjects.len() == 15 {
                    break;
                }
            }
            if !subjects.is_empty() {
                let section = gio::Menu::new();
                for (title, name) in [
                    ("Fixup Commit For", "message.fixup"),
                    ("Squash Commit For", "message.squash"),
                ] {
                    let sub = gio::Menu::new();
                    for subject in &subjects {
                        let item = gio::MenuItem::new(Some(subject), None);
                        item.set_action_and_target_value(Some(name), Some(&subject.to_variant()));
                        sub.append_item(&item);
                    }
                    section.append_submenu(Some(title), &sub);
                }
                menu.append_section(None, &section);
            }
            button.set_menu_model(Some(&menu));
        });
    }
}
