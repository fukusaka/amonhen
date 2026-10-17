//! 操作の実行。コアの命令（`crates/core/src/command.rs` の `Command`、JSON）を裏で実行し、失敗や
//! コンフリクトを知らせ、終わったら画面を読み直す。Undo の記録はコアが書く。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

type Callback = Rc<dyn Fn()>;

pub struct Actions {
    repo: RefCell<Option<Arc<Repository>>>,
    window: glib::WeakRef<gtk::ApplicationWindow>,
    /// 見出しの「取り消す」。次に取り消す操作の名前を説明に出す。
    pub undo: gtk::Button,
    /// 操作の後に、History・サイドバー・Working Copy を読み直す。
    on_change: RefCell<Option<Callback>>,
}

impl Actions {
    pub fn new(window: &gtk::ApplicationWindow) -> Rc<Self> {
        let undo = gtk::Button::with_label("取り消す");
        undo.set_sensitive(false);
        let actions = Rc::new(Self {
            repo: RefCell::default(),
            window: window.downgrade(),
            undo,
            on_change: RefCell::default(),
        });
        let weak = Rc::downgrade(&actions);
        actions.undo.connect_clicked(move |_| {
            if let Some(actions) = weak.upgrade() {
                actions.perform(json!({ "op": "undo" }));
            }
        });
        // Ctrl+Z: 入力欄が自分の Undo に使わなかったときだけ（窓で最後に受ける）。
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Bubble);
        let weak = Rc::downgrade(&actions);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(actions) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if key == gtk::gdk::Key::z
                && state.contains(gtk::gdk::ModifierType::CONTROL_MASK)
                && actions.undo.is_sensitive()
            {
                actions.perform(json!({ "op": "undo" }));
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        actions
    }

    pub fn set_repo(&self, repo: Arc<Repository>) {
        let first = self.repo.borrow().is_none();
        self.repo.replace(Some(repo));
        if first {
            self.refresh_undo();
        }
    }

    pub fn connect_change(&self, f: impl Fn() + 'static) {
        self.on_change.replace(Some(Rc::new(f)));
    }

    /// `command` を実行する。
    pub fn perform(self: &Rc<Self>, command: Value) {
        self.perform_sequence(vec![command]);
    }

    /// 命令を順に実行する。失敗したり、途中で止まったりしたら、そこでやめる。
    pub fn perform_sequence(self: &Rc<Self>, commands: Vec<Value>) {
        self.perform_then(commands, || {});
    }

    /// 命令を順に実行し、知らせと読み直しを頼んだ後で `done` を呼ぶ。
    pub fn perform_then(self: &Rc<Self>, commands: Vec<Value>, done: impl FnOnce() + 'static) {
        let Some(repo) = self.repo.borrow().clone() else {
            done();
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let mut last = Value::Null;
                for command in commands {
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_default();
                    let done = reply["ok"].as_bool() == Some(true)
                        && reply["outcome"].as_str().is_none_or(|o| o == "done");
                    last = reply;
                    if !done {
                        break;
                    }
                }
                last
            })
            .await
            .unwrap_or_default();
            if let Some(error) = result["error"].as_str() {
                this.alert("操作に失敗しました", error).await;
            } else {
                match result["outcome"].as_str() {
                    Some("conflict") => {
                        this.alert(
                            "コンフリクトがあります",
                            "Working Copy で解決してから Continue するか、Abort してください。",
                        )
                        .await;
                    }
                    Some("stopped") => {
                        this.alert(
                            "途中で止まりました",
                            "Working Copy で確かめてから Continue してください。",
                        )
                        .await;
                    }
                    _ => {}
                }
            }
            if let Some(f) = this.on_change.borrow().clone() {
                f();
            }
            this.refresh_undo();
            done();
        });
    }

    /// 次に取り消せる操作の名前を読み、「取り消す」に出す。
    pub fn refresh_undo(&self) {
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        let button = self.undo.downgrade();
        glib::spawn_future_local(async move {
            let reply = gio::spawn_blocking(move || {
                serde_json::from_str::<Value>(&repo.run_json(r#"{"op":"undoPeek"}"#))
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            let Some(button) = button.upgrade() else {
                return;
            };
            let label = reply["label"].as_str();
            button.set_sensitive(label.is_some());
            button.set_tooltip_text(label.map(|l| format!("取り消す: {l}")).as_deref());
        });
    }

    async fn alert(&self, title: &str, message: &str) {
        let dialog = gtk::AlertDialog::builder()
            .message(title)
            .detail(message)
            .modal(true)
            .build();
        let window = self.window.upgrade();
        let _ = dialog.choose_future(window.as_ref()).await;
    }

    /// 取り消せない操作の前に確かめる。
    pub async fn confirm(&self, title: &str, message: &str, ok: &str) -> bool {
        let dialog = gtk::AlertDialog::builder()
            .message(title)
            .detail(message)
            .buttons(["キャンセル", ok])
            .cancel_button(0)
            .default_button(1)
            .modal(true)
            .build();
        let window = self.window.upgrade();
        dialog.choose_future(window.as_ref()).await == Ok(1)
    }

    /// 名前を入れてもらう。キャンセルなら None。
    pub async fn ask(&self, title: &str, initial: &str, ok: &str) -> Option<String> {
        let parent = self.window.upgrade();
        let dialog = gtk::Window::builder()
            .title(title)
            .modal(true)
            .resizable(false)
            .default_width(360)
            .build();
        dialog.set_transient_for(parent.as_ref());
        let entry = gtk::Entry::builder()
            .text(initial)
            .activates_default(true)
            .build();
        let cancel = gtk::Button::with_label("キャンセル");
        let accept = gtk::Button::with_label(ok);
        accept.add_css_class("suggested-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&cancel);
        buttons.append(&accept);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(16);
        column.set_margin_bottom(16);
        column.set_margin_start(16);
        column.set_margin_end(16);
        column.append(&entry);
        column.append(&buttons);
        dialog.set_child(Some(&column));
        dialog.set_default_widget(Some(&accept));

        let (tx, rx) = async_channel::bounded::<Option<String>>(1);
        let (t, e) = (tx.clone(), entry.clone());
        accept.connect_clicked(move |_| {
            let _ = t.try_send(Some(e.text().trim().to_owned()));
        });
        let t = tx.clone();
        cancel.connect_clicked(move |_| {
            let _ = t.try_send(None);
        });
        dialog.connect_close_request(move |_| {
            let _ = tx.try_send(None);
            glib::Propagation::Proceed
        });
        dialog.present();
        entry.grab_focus();
        let answer = rx.recv().await.ok().flatten();
        dialog.destroy();
        answer.filter(|s| !s.is_empty())
    }
}
