//! 操作の実行。コアの命令（`crates/core/src/command.rs` の `Command`、JSON）を裏で実行し、失敗や
//! コンフリクトを知らせ、終わったら画面を読み直す。Undo の記録はコアが書く。

use std::cell::RefCell;
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
    /// タブの中身（今どの窓にあるかは、ここから辿る。タブは窓を移れる）。
    anchor: glib::WeakRef<gtk::Widget>,
    /// タブのアクション（`win.undo` などの使えるかどうかを、前にないタブでもそのタブのものに入れる）。
    group: glib::WeakRef<gio::SimpleActionGroup>,
    /// 見出しの「取り消す」。次に取り消す操作の名前を説明に出す。
    pub undo: gtk::Button,
    /// 操作の後に、History・サイドバー・Working Copy を読み直す。
    on_change: RefCell<Option<Callback>>,
    /// 操作の後に呼ぶ（Activity の History など）。
    on_after: RefCell<Vec<Callback>>,
    /// fetch・pull の後（失敗しなければ。macOS 版の `onFetch`）。
    on_fetch: RefCell<Vec<Callback>>,
    /// 書き込む先の worktree を替えたら呼ぶ（Activity の History など、Undo の先が変わるもの）。
    on_repo: RefCell<Vec<Callback>>,
    /// 今のブランチを送るのが Publish かどうかが変わったら呼ぶ（Commit ▾ のツールチップ）。
    on_publishes: RefCell<Vec<Callback>>,
    on_failure: RefCell<Option<FailureCallback>>,
    on_done: RefCell<Option<DoneCallback>>,
    /// 操作がコンフリクトや途中で止まった（Working Copy で最初のコンフリクトを開く）。
    on_stop: RefCell<Option<Callback>>,
    /// 今のブランチ（detached HEAD なら None）。Reset の名前と確認に出す。
    head: RefCell<Option<String>>,
    /// メニューの項目とツールバーのボタンを使えるかを決める状態（変えたら [`enabled`] でアクションに入れる）。
    facts: Rc<RefCell<Facts>>,
    /// ほかの worktree が checkout しているブランチ → その worktree（名前、パス）。
    worktree_branches: RefCell<std::collections::HashMap<String, (String, String)>>,
    /// そのうち、メインの worktree が checkout しているブランチ（印のアイコンを家にする）。
    main_worktree_branch: RefCell<Option<String>>,
    /// 次に取り消せる操作の名前（メニューの「Undo <名前>…」）。
    undo_label: Rc<RefCell<Option<String>>>,
    /// 今のブランチを送るのが Publish か（まだリモートにない。メニューの「Publish…」）。
    publishes: std::cell::Cell<bool>,
}

impl Actions {
    /// `anchor` はタブの中身、`group` はタブのアクション。
    pub fn new(anchor: &impl IsA<gtk::Widget>, group: &gio::SimpleActionGroup) -> Rc<Self> {
        // 見出しの Undo（メニューの Undo と同じアクション。使えるかはアクションで決まり、押すとアクションが確かめて
        // から戻す。Ctrl+Z は入力欄の取り消しに残す。docs/ux.md の「Menus」）。
        let undo = gtk::Button::new();
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Undo, false);
        undo.set_child(Some(&icon));
        undo.set_action_name(Some("win.undo"));
        Rc::new(Self {
            repo: RefCell::default(),
            anchor: anchor.upcast_ref::<gtk::Widget>().downgrade(),
            group: group.downgrade(),
            undo,
            on_change: RefCell::default(),
            on_after: RefCell::default(),
            on_fetch: RefCell::default(),
            on_repo: RefCell::default(),
            on_publishes: RefCell::default(),
            on_failure: RefCell::default(),
            on_done: RefCell::default(),
            on_stop: RefCell::default(),
            head: RefCell::default(),
            facts: Rc::default(),
            worktree_branches: RefCell::default(),
            main_worktree_branch: RefCell::default(),
            undo_label: Rc::default(),
            publishes: std::cell::Cell::default(),
        })
    }

    pub fn set_repo(&self, repo: Arc<Repository>) {
        let workdir = repo.paths().workdir;
        let old = self.repo.replace(Some(repo));
        let first = old.is_none();
        // 同じ worktree を読み直しただけなら何もしない。
        let moved = old.is_some_and(|old| old.paths().workdir != workdir);
        if first || moved {
            self.refresh_undo();
        }
        if moved {
            let callbacks: Vec<Callback> = self.on_repo.borrow().clone();
            for f in callbacks {
                f();
            }
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

    /// ほかの worktree が checkout しているブランチ（worktree の一覧を読むたびに入れる）。`main` はそのうち
    /// メインの worktree が checkout しているもの。
    pub fn set_worktree_branches(
        &self,
        map: std::collections::HashMap<String, (String, String)>,
        main: Option<String>,
    ) {
        self.worktree_branches.replace(map);
        self.main_worktree_branch.replace(main);
    }

    /// `branch` を checkout しているのが、書き込む先でないメインの worktree か。
    pub fn is_main_worktree_branch(&self, branch: &str) -> bool {
        self.main_worktree_branch.borrow().as_deref() == Some(branch)
    }

    /// `branch` を checkout している、書き込む先でない worktree（名前、パス）。
    pub fn worktree_using(&self, branch: &str) -> Option<(String, String)> {
        self.worktree_branches.borrow().get(branch).cloned()
    }

    /// `path` の worktree を書き込む先にする（窓の `win.edit-worktree`）。
    pub fn edit_worktree(&self, path: &str) {
        if let Some(group) = self.group.upgrade() {
            group.activate_action("edit-worktree", Some(&path.to_variant()));
        }
    }

    /// merge・rebase・cherry-pick・revert・am の途中か（途中なら Reset を出さない）。
    pub fn in_sequence(&self) -> bool {
        self.facts.borrow().sequence.is_some()
    }

    /// 使えるかを決める状態を `f` で変え、タブのアクションに入れる（ref・status・画面が変わるたびに。macOS 版で
    /// メニューを開くたびに `validate` が見るものと同じ）。
    /// 手元のリポジトリか（daemon・SSH 先でない。端末・ファイルの窓で開ける）。
    pub fn is_local(&self) -> bool {
        self.facts.borrow().local
    }

    pub fn update_facts(&self, f: impl FnOnce(&mut Facts)) {
        f(&mut self.facts.borrow_mut());
        apply_facts(&self.group, &self.facts.borrow());
    }

    /// タブのある窓。
    pub fn window(&self) -> Option<gtk::ApplicationWindow> {
        self.anchor.upgrade()?.root().and_downcast()
    }

    /// 失敗したら `f` で知らせる（なければ窓で知らせる）。
    pub fn connect_failure(&self, f: impl Fn(crate::statusbar::Failure) + 'static) {
        self.on_failure.replace(Some(Rc::new(f)));
    }

    /// 操作が終わったら `f(命令)` を呼ぶ（失敗・コンフリクトでは呼ばない）。
    pub fn connect_done(&self, f: impl Fn(&Value) + 'static) {
        self.on_done.replace(Some(Rc::new(f)));
    }

    /// 操作がコンフリクトや途中で止まったら `f` を呼ぶ（merge・rebase・cherry-pick・revert・pull・stash の
    /// 適用など。macOS 版の `onStop`）。
    pub fn connect_stop(&self, f: impl Fn() + 'static) {
        self.on_stop.replace(Some(Rc::new(f)));
    }

    pub fn connect_change(&self, f: impl Fn() + 'static) {
        self.on_change.replace(Some(Rc::new(f)));
    }

    /// 操作の後に `f` を呼ぶ。
    pub fn connect_after(&self, f: impl Fn() + 'static) {
        self.on_after.borrow_mut().push(Rc::new(f));
    }

    /// fetch か pull を終えたら（失敗でなければ。コンフリクトで止まっても）`f` を呼ぶ。
    pub fn connect_fetch(&self, f: impl Fn() + 'static) {
        self.on_fetch.borrow_mut().push(Rc::new(f));
    }

    /// 書き込む先の worktree を替えたら `f` を呼ぶ。
    pub fn connect_repo_changed(&self, f: impl Fn() + 'static) {
        self.on_repo.borrow_mut().push(Rc::new(f));
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
        self.perform_recovering(commands, |_| false, done);
    }

    /// [`Self::perform_with`] と同じで、失敗の答えを `handled` が引き受けたら（`done` で別に尋ねるなど）、失敗を
    /// 知らせない（同じことを、失敗とその問いの 2 つで見せないように。macOS 版の `recover`）。
    pub fn perform_recovering(
        self: &Rc<Self>,
        commands: Vec<Value>,
        handled: impl FnOnce(&Value) -> bool + 'static,
        done: impl FnOnce(&Value) + 'static,
    ) {
        let Some(repo) = self.repo.borrow().clone() else {
            done(&Value::Null);
            return;
        };
        let this = self.clone();
        crate::activity::begin();
        glib::spawn_future_local(async move {
            let mut result = Value::Null;
            let mut last_command = Value::Null;
            let count = commands.len();
            for (i, command) in commands.into_iter().enumerate() {
                let (r, c) = (repo.clone(), command.to_string());
                let reply: Value = gio::spawn_blocking(move || {
                    serde_json::from_str(&r.run_json(&c)).unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                let done = reply["ok"].as_bool() == Some(true)
                    && reply["outcome"].as_str().is_none_or(|o| o == "done");
                result = reply;
                last_command = command;
                if !done || i + 1 == count {
                    break;
                }
                // 途中の命令が済んだ。macOS 版と同じく 1 つごとに知らせて読み直す（いくつかのブランチの削除や
                // Cherry-pick で、途中の状態も画面に出す）。最後の 1 つは下でまとめて扱う。
                if let Some(f) = this.on_done.borrow().clone() {
                    f(&last_command);
                }
                if matches!(last_command["op"].as_str(), Some("fetch" | "pull")) {
                    let fetched: Vec<Callback> = this.on_fetch.borrow().clone();
                    for f in fetched {
                        f();
                    }
                }
                if let Some(f) = this.on_change.borrow().clone() {
                    f();
                }
                this.refresh_undo();
            }
            crate::activity::end();
            // 失敗はステータスバーに（作業を止めない）。コンフリクトや途中で止まったことは、読み直したときに
            // ステータスバーの途中の操作の行が知らせる。
            if result["error"].is_string() {
                if handled(&result) {
                    // 知らせは `done` に任せる。
                } else {
                    let failure =
                        crate::statusbar::Failure::from_reply(&result, Some(last_command.clone()));
                    this.fail(failure).await;
                }
            } else if result["outcome"].as_str().is_none_or(|o| o == "done")
                && let Some(f) = this.on_done.borrow().clone()
            {
                f(&last_command);
            }
            // どこで止まったか・次に何をするかはステータスバーの行に出る。Working Copy でコンフリクトを開く。
            let stopped = !result["error"].is_string()
                && matches!(result["outcome"].as_str(), Some("conflict" | "stopped"));
            if stopped && let Some(f) = this.on_stop.borrow().clone() {
                f();
            }
            if let Some(f) = this.on_change.borrow().clone() {
                f();
            }
            this.refresh_undo();
            if let Some(names) = not_merged_yet(&result) {
                // 1 つずつの merge が途中で止まった。まだ merge していないものは、ここでしか言えない。
                this.alert(
                    &tr!("Not merged yet: {names}", names = names),
                    &tr!("Merge them again after resolving these conflicts."),
                )
                .await;
            }
            if !result["error"].is_string()
                && matches!(last_command["op"].as_str(), Some("fetch" | "pull"))
            {
                let fetched: Vec<Callback> = this.on_fetch.borrow().clone();
                for f in fetched {
                    f();
                }
            }
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
        self.perform_work_titled(None, work);
    }

    /// `perform_work` の、失敗の見出しを決める形（Rebuild History なら「Couldn’t Rebuild History」）。失敗は
    /// 命令の失敗と同じくステータスバーに出す（macOS 版と同じ）。
    pub fn perform_work_titled(
        self: &Rc<Self>,
        heading: Option<String>,
        work: impl FnOnce(&Repository) -> amonhen_model::Result<()> + Send + 'static,
    ) {
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        let this = self.clone();
        crate::activity::begin();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || work(&repo).map_err(|e| e.to_string())).await;
            crate::activity::end();
            if let Ok(Err(e)) = result {
                this.fail(crate::statusbar::Failure {
                    message: e,
                    kind: None,
                    path: None,
                    command: None,
                    heading: Some(heading.unwrap_or_else(|| tr!("The operation failed"))),
                })
                .await;
            }
            if let Some(f) = this.on_change.borrow().clone() {
                f();
            }
            this.refresh_undo();
        });
    }

    /// 今のブランチを送るのが Publish か（ref を読んだときに決める）。
    pub fn set_publishes(&self, publishes: bool) {
        if self.publishes.replace(publishes) == publishes {
            return;
        }
        let callbacks: Vec<Callback> = self.on_publishes.borrow().clone();
        for f in callbacks {
            f();
        }
    }

    /// 今のブランチを送るのが Publish か。
    pub fn publishes(&self) -> bool {
        self.publishes.get()
    }

    /// [`Self::publishes`] が変わったら `f` を呼ぶ。
    pub fn connect_publishes_changed(&self, f: impl Fn() + 'static) {
        self.on_publishes.borrow_mut().push(Rc::new(f));
    }

    /// 状態で変わるメニューの項目の名前（macOS 版の `validateMenuItem` と同じ）。`action` は `win.push` など。
    /// 変わらない項目は None。
    pub fn menu_title(&self, action: &str) -> Option<String> {
        let publishes = self.publishes.get();
        Some(match action {
            "win.push" if publishes => "Publish…".to_owned(),
            "win.push" => "Push…".to_owned(),
            "win.commit-and-push" if publishes => "Commit and Publish…".to_owned(),
            "win.commit-and-push" => "Commit and Push".to_owned(),
            "win.undo" => match self.undo_label.borrow().as_deref() {
                Some(label) => format!("Undo {label}…"),
                None => "Undo…".to_owned(),
            },
            "win.toggle-tab-bar" => crate::tabs::tab_bar_title(&self.window()?),
            _ => return None,
        })
    }

    /// `menu` の項目の名前を、今の状態に合わせる（開くときに呼ぶ）。
    pub fn retitle(&self, menu: &gio::MenuModel) {
        crate::menu::retitle(menu, &|action| self.menu_title(action));
    }

    /// 操作の後と同じく読み直す（命令を通さずに変えたとき。gh pr checkout など）。
    pub fn reload(&self) {
        if let Some(f) = self.on_change.borrow().clone() {
            f();
        }
        self.refresh_undo();
    }

    /// 次に取り消せる操作の名前を読み、「取り消す」に出す。
    pub fn refresh_undo(&self) {
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        let button = self.undo.downgrade();
        let (group, facts) = (self.group.clone(), self.facts.clone());
        let undo_label = self.undo_label.clone();
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
            undo_label.replace(label.map(str::to_owned));
            facts.borrow_mut().undo = label.is_some();
            apply_facts(&group, &facts.borrow());
            let tip = match label {
                Some(l) => tr!("Undo: {label} (Ctrl+Alt+Z)", label = l),
                None => tr!("Nothing to Undo"),
            };
            button.set_tooltip_text(Some(&tip));
        });
    }

    /// 失敗をステータスバーに（つないでいなければ、要点・次の一手・git の文を並べた知らせで。macOS 版の `fail`）。
    pub async fn fail(&self, failure: crate::statusbar::Failure) {
        let on_failure = self.on_failure.borrow().clone();
        match on_failure {
            Some(f) => f(failure),
            None => {
                let text = [
                    Some(failure.summary()),
                    failure.advice(),
                    failure.kind.is_some().then(|| failure.message.clone()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n\n");
                self.alert(&failure.title(), &text).await
            }
        }
    }

    pub async fn alert(&self, title: &str, message: &str) {
        let dialog = gtk::AlertDialog::builder()
            .message(title)
            .detail(message)
            .modal(true)
            .build();
        let window = self.window();
        let _ = dialog.choose_future(window.as_ref()).await;
    }

    /// 実行の前に確かめる（macOS 版の `confirm`。ほかの確認と同じシートで）。
    pub async fn confirm(&self, title: &str, message: &str, ok: &str) -> bool {
        let parent = self.window();
        let sheet =
            crate::sheets::Sheet::unnamed(parent.as_ref().map(|w| w.upcast_ref()), title, message);
        sheet.buttons(ok, false, None);
        sheet.run().await == crate::sheets::Response::Ok
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

    /// macOS 版の確認の窓（見出し・説明・部品・ボタン）に当たるシートを出す。`ok` を押したら true。ほかの確認と
    /// 同じシート（[`Sheet`]）なので、見た目と Return・Esc の扱いも同じ。
    async fn run_dialog(
        &self,
        title: &str,
        message: &str,
        content: &gtk::Widget,
        focus: &gtk::Widget,
        ok: &str,
    ) -> bool {
        let parent = self.window();
        let sheet =
            crate::sheets::Sheet::unnamed(parent.as_ref().map(|w| w.upcast_ref()), title, message);
        sheet.add(content);
        sheet.buttons(ok, false, None);
        sheet.window.present();
        focus.grab_focus();
        sheet.run().await == crate::sheets::Response::Ok
    }
}

/// 1 つずつの merge がコンフリクトで止まったときの、まだ merge していないもの（`info.remaining`。今の言語の
/// 並べ方でつなぐ）。止まっていない・残りがなければ None。
fn not_merged_yet(reply: &Value) -> Option<String> {
    if reply["error"].is_string() || reply["outcome"].as_str() != Some("conflict") {
        return None;
    }
    let names: Vec<&str> = reply["info"]["remaining"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    (!names.is_empty()).then(|| amonhen_model::i18n::list(&names))
}

/// メニューの項目とツールバーのボタンを使えるかを決める、タブの状態（macOS 版の `validate` が見るもの）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    /// ref を読んだ。
    pub refs: bool,
    /// HEAD がコミットを指している（まだコミットのないブランチでない）。
    pub head: bool,
    /// ブランチを checkout している（detached HEAD でない）。
    pub head_branch: bool,
    /// 今のブランチの（上流に送れるか、上流より先にあるコミットの数）。ブランチの項目がなければ None。
    pub tracking: Option<(bool, u32)>,
    /// stash がある。
    pub stash: bool,
    /// 途中の操作（`sequence` の `kind`: merge・rebase・cherry-pick・revert・am）。
    pub sequence: Option<String>,
    /// 取り消せる操作がある。
    pub undo: bool,
    /// Working Copy を出している。
    pub working_copy: bool,
    /// Working Copy の Commit を押せる。
    pub can_commit: bool,
    /// コンフリクト以外に、stage していない変更がある。
    pub can_stage_all: bool,
    /// History を出していて、パッチにできるコミット（merge でないもの）だけを選んでいる。
    pub patch: bool,
    /// 手元のリポジトリ（daemon・SSH 越しでない。端末・ファイルマネージャーは手元のものしか開けない）。
    pub local: bool,
    /// origin（なければ最初のリモート）が GitHub（gh があるときだけ調べる）。
    pub github: bool,
    /// gh が使える（Create Pull Request… に要る。macOS 版の `GitHub.isAvailable`）。
    pub gh: bool,
    /// まだ開けていないタブ（つないでいる途中・つなげなかった）。リポジトリの操作はどれも使えない（macOS 版の
    /// つないでいるタブは、リポジトリの窓でないので、その操作がない）。
    pub unopened: bool,
    /// Find Next・Find Previous が効く（History を探した、またはフォーカスが差分・History にある。macOS 版で
    /// ⌘G を受ける部品があるか）。
    pub find_next: bool,
}

/// リポジトリを開いていないと使えない、表の操作のメニュー（File・Window・アプリのものは開く前でも使える）。
const REPOSITORY_MENUS: &[amonhen_commands::Menu] = &[
    amonhen_commands::Menu::Edit,
    amonhen_commands::Menu::View,
    amonhen_commands::Menu::Navigate,
    amonhen_commands::Menu::Repository,
    amonhen_commands::Menu::Commit,
];

/// 表にない、リポジトリを開いていないと使えないタブのアクション（Find Next・Find Previous は、そのうえ
/// `Facts::find_next` のときだけ）。
const REPOSITORY_ONLY: &[&str] = &["find", "search-history"];

/// 状態から、タブのアクション（`win.<id>`）ごとに使えるか（macOS 版の `RepositoryWindow.validate` と同じ）。
/// ここにないアクションはいつも使える。
pub fn enabled(f: &Facts) -> Vec<(&'static str, bool)> {
    let sequence = f.sequence.as_deref();
    let idle = sequence.is_none();
    // Skip は 1 つずつ当てていく操作だけ（merge・revert にはない）。
    let skip = sequence.is_some_and(|s| s.starts_with("rebase") || s == "cherry-pick" || s == "am");
    let commit = f.working_copy && f.can_commit;
    let mut list = vec![
        ("continue", !idle),
        ("abort", !idle),
        ("skip", skip),
        ("save-patch", f.patch),
        ("apply-patch", idle),
        // 送るものがなければ押せない（理由はツールバーのツールチップ）。上流のないブランチは Publish。
        (
            "push",
            f.tracking
                .is_some_and(|(published, ahead)| !published || ahead > 0),
        ),
        (
            "force-push",
            f.tracking.is_some_and(|(published, _)| published),
        ),
        ("pull", f.head_branch && idle),
        ("merge", f.refs && idle),
        ("rebase", f.refs && idle),
        ("pop-stash", f.stash),
        ("show-stashes", f.stash),
        ("undo", f.undo),
        ("go-to-head", f.head),
        ("switch-branch", f.refs),
        ("rename-branch", f.head_branch),
        ("set-upstream", f.head_branch),
        ("compare-with-previous", f.head_branch),
        ("commit", commit),
        ("commit-and-push", commit),
        ("stage-all", f.working_copy && f.can_stage_all),
        ("open-in-terminal", f.local),
        ("reveal-in-files", f.local),
        ("open-on-github", f.github),
        ("create-pull-request", f.github && f.gh && f.head_branch),
    ];
    // 開けていないタブでは、リポジトリの操作をどれも使えなくする。
    let opened = !f.unopened;
    for (_, yes) in &mut list {
        *yes &= opened;
    }
    for section in amonhen_commands::SECTIONS {
        if !REPOSITORY_MENUS.contains(&section.menu) {
            continue;
        }
        for id in section.ids {
            if !list.iter().any(|(n, _)| n == id) {
                list.push((id, opened));
            }
        }
    }
    list
}

/// [`enabled`] の答えを、タブのアクションに入れる。
fn apply_facts(group: &glib::WeakRef<gio::SimpleActionGroup>, facts: &Facts) {
    let Some(group) = group.upgrade() else {
        return;
    };
    for (name, yes) in enabled(facts) {
        if let Some(action) = group
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(yes);
        }
    }
    for name in REPOSITORY_ONLY {
        if let Some(action) = group
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(!facts.unopened);
        }
    }
    for name in ["find-next", "find-previous"] {
        if let Some(action) = group
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(!facts.unopened && facts.find_next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(facts: &Facts, name: &str) -> bool {
        enabled(facts)
            .into_iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} がない"))
            .1
    }

    /// ブランチを checkout していて、ref を読み終え、何の途中でもない。
    fn idle() -> Facts {
        Facts {
            refs: true,
            head: true,
            head_branch: true,
            tracking: Some((true, 0)),
            local: true,
            ..Facts::default()
        }
    }

    #[test]
    fn names_what_a_stopped_merge_left() {
        let reply = |v: Value| not_merged_yet(&v);
        assert_eq!(
            reply(
                json!({ "ok": true, "outcome": "conflict", "info": { "remaining": ["b", "c"] } })
            ),
            Some("b and c".into())
        );
        assert_eq!(reply(json!({ "ok": true, "outcome": "conflict" })), None);
        assert_eq!(
            reply(json!({ "ok": true, "outcome": "conflict", "info": { "remaining": [] } })),
            None
        );
        assert_eq!(
            reply(json!({ "ok": true, "outcome": "done", "info": { "remaining": ["b"] } })),
            None
        );
    }

    #[test]
    fn an_unopened_tab_has_no_repository_actions() {
        let facts = Facts {
            unopened: true,
            ..idle()
        };
        for name in [
            "refresh",
            "fetch",
            "show-history",
            "go-to-parent",
            "commit",
            "find-in-history",
        ] {
            assert!(!of(&facts, name), "{name}");
        }
        // File・Window・アプリの操作は表に入れない（いつも使える）。
        for name in ["quick-open", "repositories", "activity", "settings"] {
            assert!(enabled(&facts).iter().all(|(n, _)| *n != name), "{name}");
        }
        // 開けたら戻る。
        assert!(of(&idle(), "refresh"));
        assert!(of(&idle(), "fetch"));
    }

    #[test]
    fn every_name_is_a_command_of_the_table() {
        for (name, _) in enabled(&Facts::default()) {
            assert!(amonhen_commands::find(name).is_some(), "表にない: {name}");
        }
    }

    #[test]
    fn nothing_that_needs_refs_is_enabled_before_they_load() {
        let facts = Facts::default();
        for name in [
            "pull",
            "push",
            "force-push",
            "merge",
            "rebase",
            "switch-branch",
            "go-to-head",
            "rename-branch",
            "create-pull-request",
        ] {
            assert!(!of(&facts, name), "{name}");
        }
    }

    #[test]
    fn continue_abort_and_skip_only_during_an_operation() {
        let mut facts = idle();
        for name in ["continue", "abort", "skip"] {
            assert!(!of(&facts, name), "{name}");
        }
        assert!(of(&facts, "apply-patch"));
        for (kind, skip) in [
            ("merge", false),
            ("revert", false),
            ("rebase", true),
            ("cherry-pick", true),
            ("am", true),
        ] {
            facts.sequence = Some(kind.into());
            assert!(of(&facts, "continue") && of(&facts, "abort"), "{kind}");
            assert_eq!(of(&facts, "skip"), skip, "{kind}");
            // 途中では、新しく取り込む操作とパッチを当てる操作はできない。
            for name in ["pull", "merge", "rebase", "apply-patch"] {
                assert!(!of(&facts, name), "{kind}: {name}");
            }
        }
    }

    #[test]
    fn pull_and_branch_operations_need_a_branch() {
        let mut facts = idle();
        for name in [
            "pull",
            "rename-branch",
            "set-upstream",
            "compare-with-previous",
        ] {
            assert!(of(&facts, name), "{name}");
        }
        // detached HEAD: 切り替えや Merge・Rebase はできる。
        facts.head_branch = false;
        facts.tracking = None;
        for name in [
            "pull",
            "push",
            "force-push",
            "rename-branch",
            "set-upstream",
            "compare-with-previous",
        ] {
            assert!(!of(&facts, name), "{name}");
        }
        for name in ["merge", "rebase", "switch-branch", "go-to-head"] {
            assert!(of(&facts, name), "{name}");
        }
    }

    #[test]
    fn push_needs_something_to_send() {
        let mut facts = idle();
        assert!(!of(&facts, "push"));
        assert!(of(&facts, "force-push"));
        facts.tracking = Some((true, 2));
        assert!(of(&facts, "push"));
        // 上流がない: Publish（Force Push はない）。
        facts.tracking = Some((false, 0));
        assert!(of(&facts, "push"));
        assert!(!of(&facts, "force-push"));
    }

    #[test]
    fn commit_and_stage_all_only_in_working_copy() {
        let mut facts = Facts {
            can_commit: true,
            can_stage_all: true,
            ..idle()
        };
        for name in ["commit", "commit-and-push", "stage-all"] {
            assert!(!of(&facts, name), "{name}");
        }
        facts.working_copy = true;
        for name in ["commit", "commit-and-push", "stage-all"] {
            assert!(of(&facts, name), "{name}");
        }
        facts.can_commit = false;
        facts.can_stage_all = false;
        for name in ["commit", "commit-and-push", "stage-all"] {
            assert!(!of(&facts, name), "{name}");
        }
    }

    #[test]
    fn local_github_stash_undo_and_patch() {
        let names = [
            "open-on-github",
            "create-pull-request",
            "pop-stash",
            "show-stashes",
            "undo",
            "save-patch",
        ];
        let mut facts = idle();
        for name in names {
            assert!(!of(&facts, name), "{name}");
        }
        assert!(of(&facts, "open-in-terminal") && of(&facts, "reveal-in-files"));
        facts.local = false;
        assert!(!of(&facts, "open-in-terminal") && !of(&facts, "reveal-in-files"));
        facts.github = true;
        facts.gh = true;
        facts.stash = true;
        facts.undo = true;
        facts.patch = true;
        for name in names {
            assert!(of(&facts, name), "{name}");
        }
        // プルリクエストは今のブランチから作る。
        facts.head_branch = false;
        assert!(!of(&facts, "create-pull-request"));
        assert!(of(&facts, "open-on-github"));
        // gh がなければ作れない（Open on GitHub はブラウザで開くので使える）。
        facts.head_branch = true;
        facts.gh = false;
        assert!(!of(&facts, "create-pull-request"));
        assert!(of(&facts, "open-on-github"));
    }
}
