//! 操作の実行。コアの命令（`crates/core/src/command.rs` の `Command`、JSON）を裏で実行し、失敗や
//! コンフリクトを知らせ、終わったら画面を読み直す。Undo の記録はコアが書く。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

type Callback = Rc<dyn Fn()>;
/// 失敗を知らせる（ステータスバー）。
type FailureCallback = Rc<dyn Fn(crate::statusbar::Failure)>;
/// 操作が終わった（ステータスバーの一言）。
type DoneCallback = Rc<dyn Fn(&Value)>;

pub struct Actions {
    repo: RefCell<Option<Arc<Repository>>>,
    window: glib::WeakRef<gtk::ApplicationWindow>,
    /// 見出しの「取り消す」。次に取り消す操作の名前を説明に出す。
    pub undo: gtk::Button,
    /// 操作の後に、History・サイドバー・Working Copy を読み直す。
    on_change: RefCell<Option<Callback>>,
    /// 操作の後に呼ぶ（Activity の History など）。
    on_after: RefCell<Vec<Callback>>,
    on_failure: RefCell<Option<FailureCallback>>,
    on_done: RefCell<Option<DoneCallback>>,
    /// 今のブランチ（detached HEAD なら None）。Reset の名前と確認に出す。
    head: RefCell<Option<String>>,
    /// merge・rebase・cherry-pick・revert の途中か（途中なら Reset を出さない）。
    in_sequence: Cell<bool>,
}

impl Actions {
    pub fn new(window: &gtk::ApplicationWindow) -> Rc<Self> {
        // 見出しの Undo（メニューの Undo と同じアクション。使えるかはアクションで決まる）。
        let undo = gtk::Button::new();
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Undo, false);
        undo.set_child(Some(&icon));
        undo.set_action_name(Some("win.undo"));
        let actions = Rc::new(Self {
            repo: RefCell::default(),
            window: window.downgrade(),
            undo,
            on_change: RefCell::default(),
            on_after: RefCell::default(),
            on_failure: RefCell::default(),
            on_done: RefCell::default(),
            head: RefCell::default(),
            in_sequence: Cell::new(false),
        });
        // git の Undo は確かめてから（Ctrl+Z は入力欄の取り消しに残す。docs/ux.md の「Menus」）。
        let weak = Rc::downgrade(&actions);
        actions.undo.connect_clicked(move |_| {
            if let Some(actions) = weak.upgrade() {
                crate::sheets::undo(&actions);
            }
        });
        actions
    }

    pub fn set_repo(&self, repo: Arc<Repository>) {
        let first = self.repo.borrow().is_none();
        self.repo.replace(Some(repo));
        if first {
            self.refresh_undo();
        }
    }

    pub fn repo(&self) -> Option<Arc<Repository>> {
        self.repo.borrow().clone()
    }

    /// 今のブランチ（ref を読むたびに入れる）。
    pub fn set_head(&self, branch: Option<String>) {
        self.head.replace(branch);
    }

    pub fn head(&self) -> Option<String> {
        self.head.borrow().clone()
    }

    /// merge・rebase などの途中か（status を読むたびに入れる）。
    pub fn set_in_sequence(&self, yes: bool) {
        self.in_sequence.set(yes);
    }

    pub fn in_sequence(&self) -> bool {
        self.in_sequence.get()
    }

    pub fn window(&self) -> Option<gtk::ApplicationWindow> {
        self.window.upgrade()
    }

    /// 失敗したら `f` で知らせる（なければ窓で知らせる）。
    pub fn connect_failure(&self, f: impl Fn(crate::statusbar::Failure) + 'static) {
        self.on_failure.replace(Some(Rc::new(f)));
    }

    /// 操作が終わったら `f(命令)` を呼ぶ（失敗・コンフリクトでは呼ばない）。
    pub fn connect_done(&self, f: impl Fn(&Value) + 'static) {
        self.on_done.replace(Some(Rc::new(f)));
    }

    pub fn connect_change(&self, f: impl Fn() + 'static) {
        self.on_change.replace(Some(Rc::new(f)));
    }

    /// 操作の後に `f` を呼ぶ。
    pub fn connect_after(&self, f: impl Fn() + 'static) {
        self.on_after.borrow_mut().push(Rc::new(f));
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
        self.perform_with(commands, |_| done());
    }

    /// 命令を順に実行し、知らせと読み直しを頼んだ後で、最後の答えを `done` に渡す。
    pub fn perform_with(
        self: &Rc<Self>,
        commands: Vec<Value>,
        done: impl FnOnce(&Value) + 'static,
    ) {
        let Some(repo) = self.repo.borrow().clone() else {
            done(&Value::Null);
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let (result, last_command) = gio::spawn_blocking(move || {
                let mut last = Value::Null;
                let mut last_command = Value::Null;
                for command in commands {
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_default();
                    let done = reply["ok"].as_bool() == Some(true)
                        && reply["outcome"].as_str().is_none_or(|o| o == "done");
                    last = reply;
                    last_command = command;
                    if !done {
                        break;
                    }
                }
                (last, last_command)
            })
            .await
            .unwrap_or_default();
            // 失敗はステータスバーに（作業を止めない）。コンフリクトや途中で止まったことは、読み直したときに
            // ステータスバーの途中の操作の行が知らせる。
            if result["error"].is_string() {
                let failure =
                    crate::statusbar::Failure::from_reply(&result, Some(last_command.clone()));
                let on_failure = this.on_failure.borrow().clone();
                match on_failure {
                    Some(f) => f(failure),
                    None => this.alert(&failure.title(), &failure.message).await,
                }
            } else if result["outcome"].as_str().is_none_or(|o| o == "done")
                && let Some(f) = this.on_done.borrow().clone()
            {
                f(&last_command);
            }
            if let Some(f) = this.on_change.borrow().clone() {
                f();
            }
            this.refresh_undo();
            let after: Vec<Callback> = this.on_after.borrow().clone();
            for f in after {
                f();
            }
            done(&result);
        });
    }

    /// 作業中の変更を stash する（メッセージと、untracked のファイルを含めるかを聞く）。
    pub fn stash(self: &Rc<Self>) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Some((message, untracked)) = this
                .ask_with(
                    "Stash Changes",
                    &tr!("Message (optional)"),
                    "",
                    "Stash",
                    Some(tr!("Include untracked files").as_str()),
                )
                .await
            else {
                return;
            };
            let message = (!message.is_empty()).then_some(message);
            this.perform(
                json!({ "op": "stashPush", "message": message, "includeUntracked": untracked }),
            );
        });
    }

    /// コミットの編集（`HistoryEdit`）。
    pub fn rewrite(self: &Rc<Self>, label: &str, edit: Value) {
        self.perform(json!({ "op": "rewrite", "label": label, "edit": edit }));
    }

    /// JSON の命令にない操作を裏で行い、終わったら読み直す。
    pub fn perform_work(
        self: &Rc<Self>,
        work: impl FnOnce(&Repository) -> amonhen_model::Result<()> + Send + 'static,
    ) {
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || work(&repo).map_err(|e| e.to_string())).await;
            if let Ok(Err(e)) = result {
                this.alert(&tr!("The operation failed"), &e).await;
            }
            if let Some(f) = this.on_change.borrow().clone() {
                f();
            }
            this.refresh_undo();
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
            if let Some(action) = button
                .root()
                .and_downcast::<gtk::ApplicationWindow>()
                .and_then(|w| w.lookup_action("undo"))
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(label.is_some());
            }
            let tip = match label {
                Some(l) => tr!("Undo: {label} (Ctrl+Z)", label = l),
                None => tr!("Nothing to Undo"),
            };
            button.set_tooltip_text(Some(&tip));
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
            .buttons([tr!("Cancel").as_str(), ok])
            .cancel_button(0)
            .default_button(1)
            .modal(true)
            .build();
        let window = self.window.upgrade();
        dialog.choose_future(window.as_ref()).await == Ok(1)
    }

    /// 文字を入れてもらう（macOS 版の `ask`）。キャンセルか空なら None。
    pub async fn ask(&self, title: &str, message: &str, initial: &str, ok: &str) -> Option<String> {
        self.ask_with(title, message, initial, ok, None)
            .await
            .map(|(text, _)| text)
            .filter(|s| !s.is_empty())
    }

    /// 文字を入れてもらい、`check` があればそのチェックの状態も返す。キャンセルなら None。
    pub async fn ask_with(
        &self,
        title: &str,
        message: &str,
        initial: &str,
        ok: &str,
        check: Option<&str>,
    ) -> Option<(String, bool)> {
        let entry = gtk::Entry::builder()
            .text(initial)
            .activates_default(true)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.append(&entry);
        let checkbox = check.map(gtk::CheckButton::with_label);
        if let Some(c) = &checkbox {
            content.append(c);
        }
        if !self
            .run_dialog(title, message, content.upcast_ref(), entry.upcast_ref(), ok)
            .await
        {
            return None;
        }
        let checked = checkbox.as_ref().is_some_and(|c| c.is_active());
        Some((entry.text().trim().to_owned(), checked))
    }

    /// 複数行のメッセージを編集してもらう。キャンセルなら None。
    pub async fn edit_message(&self, title: &str, initial: &str, ok: &str) -> Option<String> {
        let text = gtk::TextView::builder()
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(6)
            .bottom_margin(6)
            .left_margin(6)
            .right_margin(6)
            .build();
        text.buffer().set_text(initial);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&text)
            .min_content_width(460)
            .min_content_height(180)
            .has_frame(true)
            .build();
        if !self
            .run_dialog(title, "", scrolled.upcast_ref(), text.upcast_ref(), ok)
            .await
        {
            return None;
        }
        let buffer = text.buffer();
        let message = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_owned();
        (!message.is_empty()).then_some(message)
    }

    /// 一覧から選んでもらう（`choices` は表示と値の組）。キャンセルなら None。
    pub async fn choose(
        &self,
        title: &str,
        message: &str,
        choices: &[(String, String)],
        selected: usize,
        ok: &str,
    ) -> Option<String> {
        let titles: Vec<&str> = choices.iter().map(|(t, _)| t.as_str()).collect();
        let list = gtk::StringList::new(&titles);
        let dropdown = gtk::DropDown::new(Some(list), gtk::Expression::NONE);
        // ブランチやタグが多くても探せるように。
        dropdown.set_expression(Some(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            gtk::Expression::NONE,
            "string",
        )));
        dropdown.set_enable_search(true);
        dropdown.set_selected(selected as u32);
        if !self
            .run_dialog(
                title,
                message,
                dropdown.upcast_ref(),
                dropdown.upcast_ref(),
                ok,
            )
            .await
        {
            return None;
        }
        choices
            .get(dropdown.selected() as usize)
            .map(|(_, value)| value.clone())
    }

    /// macOS 版の確認の窓（見出し・説明・部品・ボタン）に当たる窓を出す。`ok` を押したら true。
    async fn run_dialog(
        &self,
        title: &str,
        message: &str,
        content: &gtk::Widget,
        focus: &gtk::Widget,
        ok: &str,
    ) -> bool {
        let parent = self.window.upgrade();
        let dialog = gtk::Window::builder()
            .title(title)
            .modal(true)
            .resizable(false)
            .default_width(380)
            .build();
        dialog.set_transient_for(parent.as_ref());
        let heading = gtk::Label::new(Some(title));
        heading.add_css_class("title-4");
        heading.set_xalign(0.0);
        heading.set_wrap(true);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
        column.set_margin_top(16);
        column.set_margin_bottom(16);
        column.set_margin_start(16);
        column.set_margin_end(16);
        column.append(&heading);
        if !message.is_empty() {
            let detail = gtk::Label::new(Some(message));
            detail.set_xalign(0.0);
            detail.set_wrap(true);
            column.append(&detail);
        }
        column.append(content);
        let cancel = gtk::Button::with_label(&tr!("Cancel"));
        let accept = gtk::Button::with_label(ok);
        accept.add_css_class("suggested-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        buttons.set_margin_top(4);
        buttons.append(&cancel);
        buttons.append(&accept);
        column.append(&buttons);
        dialog.set_child(Some(&column));
        dialog.set_default_widget(Some(&accept));

        let (tx, rx) = async_channel::bounded::<bool>(1);
        let t = tx.clone();
        accept.connect_clicked(move |_| {
            let _ = t.try_send(true);
        });
        let t = tx.clone();
        cancel.connect_clicked(move |_| {
            let _ = t.try_send(false);
        });
        dialog.connect_close_request(move |_| {
            let _ = tx.try_send(false);
            glib::Propagation::Proceed
        });
        dialog.present();
        focus.grab_focus();
        let answer = rx.recv().await.unwrap_or(false);
        dialog.destroy();
        answer
    }
}
