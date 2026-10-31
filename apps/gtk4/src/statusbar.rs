//! 窓の下のステータスバー（macOS 版の `StatusBar.swift`・`Failures.swift`。docs/ux.md の「Status bar」）。
//! 「今どうなっているか」を 1 か所に集める: merge・rebase の途中（Continue・Skip・Abort）、失敗（何が失敗したか・
//! なぜか・次の一手）、接続が切れた、終わった操作の一言。行は上から重要な順に積み、何もなければ隠れる。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;
use serde_json::Value;

/// 行の種類（上から並ぶ順）。同じ種類は 1 つだけ（出し直せば置き換える）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Row {
    Sequence,
    Failure,
    Message,
}

/// 失敗した操作。帯に、何が失敗したか・なぜか・次に何ができるかを出すための材料。
#[derive(Clone)]
pub struct Failure {
    /// git やコアの文（全文。「詳細」とツールチップに出す）。
    pub message: String,
    /// 失敗の種類（`crates/core/src/failure.rs`）。
    pub kind: Option<String>,
    /// 失敗に関わるパス（ほかの worktree の場所、ロックファイル）。
    pub path: Option<String>,
    /// 失敗した命令（`op` で何の操作か分かる。「もう一度」で同じものを実行する）。
    pub command: Option<Value>,
    /// 見出し（なければ命令から決める）。
    pub heading: Option<String>,
}

/// 帯に出す次の一手。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Remedy {
    OpenWorktree(String),
    OpenTerminal,
    Retry,
    Pull,
    WorkingCopy,
    SetUpstream,
}

impl Remedy {
    fn title(&self) -> String {
        match self {
            Remedy::OpenWorktree(_) => tr!("Open Worktree"),
            Remedy::OpenTerminal => tr!("Open in Terminal"),
            Remedy::Retry => tr!("Try Again"),
            Remedy::Pull => "Pull…".to_owned(),
            Remedy::WorkingCopy => tr!("Open Working Copy"),
            Remedy::SetUpstream => tr!("Set Upstream…"),
        }
    }
}

impl Failure {
    /// コアの答え（`error`・`errorKind`・`errorPath`）から。
    pub fn from_reply(reply: &Value, command: Option<Value>) -> Self {
        Self {
            message: reply["error"].as_str().unwrap_or_default().to_owned(),
            kind: reply["errorKind"].as_str().map(str::to_owned),
            path: reply["errorPath"].as_str().map(str::to_owned),
            command,
            heading: None,
        }
    }

    fn op(&self) -> Option<&str> {
        self.command.as_ref()?["op"].as_str()
    }

    /// 帯の見出し。何の操作が失敗したか。
    pub fn title(&self) -> String {
        if let Some(h) = &self.heading {
            return h.clone();
        }
        match self.op() {
            Some("checkout" | "track" | "detach") => tr!("Couldn’t Checkout"),
            Some("pull") => tr!("Couldn’t Pull"),
            Some("push" | "pushTags") => tr!("Couldn’t Push"),
            Some("fetch") => tr!("Couldn’t Fetch"),
            Some("merge" | "mergeInto") => tr!("Couldn’t Merge"),
            Some("rebase") => tr!("Couldn’t Rebase"),
            Some("cherryPick") => tr!("Couldn’t Cherry-pick"),
            Some("revert") => tr!("Couldn’t Revert"),
            Some("formatPatch") => tr!("Couldn’t Save the Patches"),
            Some("am" | "amThreeWay" | "applyPatch") => tr!("Couldn’t Apply the Patch"),
            Some("commit") => tr!("Couldn’t Commit"),
            Some("continue") => tr!("Couldn’t Continue"),
            Some("deleteBranch") => tr!("Couldn’t Delete the Branch"),
            Some("stashPush") => tr!("Couldn’t Stash"),
            Some("stashApply") => tr!("Couldn’t Apply the Stash"),
            Some("discard" | "deleteUntracked") => tr!("Couldn’t Discard"),
            Some("undo") => tr!("Couldn’t Undo"),
            _ => tr!("Couldn’t Finish"),
        }
    }

    /// 要点（1 行）。種類が分かれば言い直し、分からなければ git の fatal:・error: の行（なければ最初の行）。
    pub fn summary(&self) -> String {
        let place = self
            .path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match self.kind.as_deref() {
            Some("worktreeInUse") => match self.command.as_ref().and_then(|c| c["branch"].as_str())
            {
                Some(branch) => tr!(
                    "{branch} is checked out in the worktree {place}. A branch can be checked out in only one worktree.",
                    branch = branch,
                    place = place
                ),
                None => tr!(
                    "The branch is checked out in the worktree {place}. A branch can be checked out in only one worktree.",
                    place = place
                ),
            },
            Some("auth") => {
                tr!("The remote didn’t accept the sign-in (password, token or SSH key).")
            }
            Some("hostKey") => {
                tr!("The SSH host key of the remote isn’t known yet, or it has changed.")
            }
            Some("network") => {
                tr!("Can’t reach the remote. Check the network and the remote’s address.")
            }
            Some("rejected") => {
                tr!("The remote has commits you don’t have yet. Pull them first, then Push.")
            }
            Some("diverged") => tr!(
                "Both your branch and its upstream have new commits, so it can’t just move forward."
            ),
            Some("localChanges") => {
                tr!("Uncommitted changes would be overwritten. Commit or stash them first.")
            }
            Some("locked") => tr!(
                "Another git command is running in this repository, or one stopped and left a lock file."
            ),
            Some("noUpstream") => tr!("This branch has no upstream to pull from or push to."),
            _ => self.git_line(),
        }
    }

    /// 次にどうすればよいか（「詳細」に出す。分からなければ None）。
    pub fn advice(&self) -> Option<String> {
        match self.kind.as_deref() {
            Some("auth") => Some(tr!(
                "The remote refused the password, token or SSH key. If a dialog asked for a password or passphrase, check what you typed (GitHub needs a token, not the account password). To stop being asked, store the credentials with a credential helper (for example `gh auth setup-git` or `git config --global credential.helper libsecret`), or add the SSH key to the agent (`ssh-add`)."
            )),
            Some("hostKey") => Some(tr!(
                "Connect once in a terminal (for example `ssh -T git@github.com`) and confirm the host key, then try again."
            )),
            Some("locked") => self.path.as_ref().map(|p| {
                tr!(
                    "If no other git command is running, delete the lock file and try again:\n{path}",
                    path = p
                )
            }),
            Some("diverged") => Some(tr!(
                "Pull with Merge or Rebase instead of Fast-Forward Only."
            )),
            _ => None,
        }
    }

    /// git の文のうち、帯に出す 1 行。
    fn git_line(&self) -> String {
        let lines: Vec<&str> = self
            .message
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("hint:"))
            .collect();
        lines
            .iter()
            .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
            .or(lines.first())
            .map_or(String::new(), |l| l.to_string())
    }

    pub fn remedies(&self) -> Vec<Remedy> {
        let retry = if self.command.is_some() {
            vec![Remedy::Retry]
        } else {
            vec![]
        };
        match self.kind.as_deref() {
            Some("worktreeInUse") => self
                .path
                .clone()
                .map(|p| vec![Remedy::OpenWorktree(p)])
                .unwrap_or_default(),
            Some("auth" | "hostKey") => [vec![Remedy::OpenTerminal], retry].concat(),
            Some("network" | "locked") => retry,
            Some("rejected" | "diverged") => vec![Remedy::Pull],
            Some("localChanges") => vec![Remedy::WorkingCopy],
            Some("noUpstream") => vec![Remedy::SetUpstream],
            _ => vec![],
        }
    }

    /// 取り消した（失敗ではない）。
    pub fn cancelled(&self) -> bool {
        amonhen_model::i18n::is_cancelled(&self.message)
    }
}

/// 操作が終わったときの一言（知らせる値打ちのある操作だけ）。
pub fn done_text(command: &Value, head: Option<&str>) -> Option<String> {
    let s = |key: &str| command[key].as_str().unwrap_or_default().to_owned();
    let head = head.unwrap_or("HEAD");
    Some(match command["op"].as_str()? {
        "fetch" => tr!("Fetched"),
        "pull" => tr!("Pulled into {branch}", branch = head),
        "push" => tr!(
            "Pushed {branch} to {target}",
            branch = s("branch"),
            target = format!("{}/{}", s("remote"), s("remoteBranch"))
        ),
        "merge" => tr!("Merged into {branch}", branch = head),
        "mergeInto" => tr!(
            "Merged {rev} into {target}",
            rev = s("rev"),
            target = s("target")
        ),
        "rebase" => tr!("Rebased onto {onto}", onto = s("onto")),
        "checkout" => tr!("Checked out {name}", name = s("branch")),
        "track" => tr!("Checked out {name}", name = s("remoteBranch")),
        "stashPush" => tr!("Stashed the changes"),
        "stashApply" => tr!("Applied {name}", name = s("name")),
        "deleteBranch" => tr!("Deleted {name}", name = s("name")),
        "undo" => tr!("Undone"),
        _ => return None,
    })
}

/// 途中の操作（`sequence` の答え）の 1 行目: 何をどこへしている途中か。
pub fn sequence_title(s: &Value) -> String {
    let get = |k: &str| s[k].as_str().map(str::to_owned);
    let from = s["merging"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .or_else(|| get("from"));
    let onto = get("onto").or(from);
    let title = match (s["kind"].as_str().unwrap_or_default(), get("branch"), onto) {
        ("rebase", Some(b), Some(o)) => tr!("Rebasing {branch} onto {onto}", branch = b, onto = o),
        ("merge", Some(b), Some(f)) => tr!("Merging {from} into {branch}", from = f, branch = b),
        ("cherry-pick", Some(b), _) => tr!("Cherry-picking into {branch}", branch = b),
        ("revert", Some(b), _) => tr!("Reverting in {branch}", branch = b),
        ("am", Some(b), _) => tr!("Applying patches to {branch}", branch = b),
        ("am", _, _) => tr!("Applying patches"),
        ("rebase", _, _) => tr!("Rebase in progress"),
        ("merge", _, _) => tr!("Merge in progress"),
        ("cherry-pick", _, _) => tr!("Cherry-pick in progress"),
        _ => tr!("Revert in progress"),
    };
    match (s["current"].as_u64(), s["total"].as_u64()) {
        (Some(c), Some(t)) => format!(
            "{title} · {}",
            tr!("step {current} of {total}", current = c, total = t)
        ),
        _ => title,
    }
}

/// 途中の操作の 2 行目: どこで・なぜ止まっていて、次に何をすればよいか。
pub fn sequence_detail(s: &Value, conflicts: usize, without_markers: usize) -> String {
    let subject = s["step"]["subject"].as_str().map(|t| format!("“{t}”"));
    let subj = subject.clone().unwrap_or_default();
    if s["step"]["action"].as_str() == Some("exec") && conflicts == 0 {
        return tr!(
            "The command {subject} failed. Fix the problem, then Continue.",
            subject = subj
        );
    }
    if conflicts > 0 && without_markers > 0 {
        return match &subject {
            Some(at) => tr!(
                "{count} files have conflicts at {subject} ({without} without markers). Resolve them in Working Copy, then Continue.",
                count = conflicts,
                subject = at,
                without = without_markers
            ),
            None => tr!(
                "{count} files have conflicts ({without} without markers). Resolve them in Working Copy, then Continue.",
                count = conflicts,
                without = without_markers
            ),
        };
    }
    if conflicts > 0 {
        return match &subject {
            Some(at) => tr!(
                "{count} files have conflicts at {subject}. Resolve them in Working Copy, then Continue.",
                count = conflicts,
                subject = at
            ),
            None => tr!(
                "{count} files have conflicts. Resolve them in Working Copy, then Continue.",
                count = conflicts
            ),
        };
    }
    if s["editing"].as_bool() == Some(true) {
        return tr!(
            "Stopped to edit {subject}. Change files or amend the commit, then Continue.",
            subject = subj
        );
    }
    if s["kind"].as_str() == Some("am") && s["retried"].as_bool() != Some(true) {
        if s["empty"].as_bool() == Some(true) {
            return tr!(
                "{subject} has no changes (a cover letter?). Skip it.",
                subject = subj
            );
        }
        return if s["threeWay"].as_bool() == Some(true) {
            tr!(
                "{subject} doesn’t apply. Apply it by hand and Continue, or Skip it.",
                subject = subj
            )
        } else {
            tr!(
                "{subject} doesn’t apply. Retry it with a 3-way merge to resolve conflicts, apply it by hand and Continue, or Skip it.",
                subject = subj
            )
        };
    }
    tr!("Conflicts resolved. Continue to finish, or Abort to go back to where you started.")
}

/// 窓の下のステータスバー。作るときに、文字の色（`label.warning` など）も決める。
pub struct StatusBar {
    pub root: gtk::Box,
    rows: RefCell<Vec<(Row, gtk::Widget)>>,
    /// 終わった操作の一言を消すまでの時計（出し直したら前のものは効かせない）。
    flash_generation: Cell<u64>,
}

impl StatusBar {
    pub fn new() -> Rc<Self> {
        install_css();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_visible(false);
        Rc::new(Self {
            root,
            rows: RefCell::default(),
            flash_generation: Cell::new(0),
        })
    }

    /// `row` の行を出す（同じ種類の前の行は置き換える）。
    pub fn show(&self, row: Row, content: &impl IsA<gtk::Widget>) {
        self.hide(row);
        let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
        wrapper.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        wrapper.append(content);
        let mut rows = self.rows.borrow_mut();
        let index = rows
            .iter()
            .position(|(r, _)| *r > row)
            .unwrap_or(rows.len());
        // 決まった順に差し込む。
        let before = index.checked_sub(1).map(|i| rows[i].1.clone());
        self.root.insert_child_after(&wrapper, before.as_ref());
        rows.insert(index, (row, wrapper.upcast()));
        self.root.set_visible(true);
    }

    pub fn hide(&self, row: Row) {
        let mut rows = self.rows.borrow_mut();
        if let Some(i) = rows.iter().position(|(r, _)| *r == row) {
            let (_, w) = rows.remove(i);
            self.root.remove(&w);
        }
        self.root.set_visible(!rows.is_empty());
    }

    /// 終わった操作の一言を、しばらく出して消す。
    pub fn flash(self: &Rc<Self>, text: &str) {
        let icon = gtk::Label::new(Some("✓"));
        icon.add_css_class("success");
        let label = gtk::Label::new(Some(text));
        label.add_css_class("dim-label");
        label.add_css_class("caption");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        self.show(Row::Message, &line(&[icon.upcast(), label.upcast()], None));
        let generation = self.flash_generation.get() + 1;
        self.flash_generation.set(generation);
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local_once(5, move || {
            if let Some(this) = weak.upgrade()
                && this.flash_generation.get() == generation
            {
                this.hide(Row::Message);
            }
        });
    }
}

/// 行の並び（左から右）。`tint` は敷く色の CSS のクラス。
pub fn line(children: &[gtk::Widget], tint: Option<&str>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.set_margin_top(4);
    b.set_margin_bottom(4);
    b.set_margin_start(12);
    b.set_margin_end(12);
    for c in children {
        b.append(c);
    }
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.append(&b);
    if let Some(tint) = tint {
        outer.add_css_class(tint);
    }
    outer
}

/// 小さなボタン。
pub fn button(label: &str, suggested: bool) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    if suggested {
        b.add_css_class("suggested-action");
    }
    b.set_valign(gtk::Align::Center);
    b
}

fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".status-sequence { background-color: alpha(@warning_color, 0.14); } \
         .status-failure { background-color: alpha(@error_color, 0.08); } \
         label.warning { color: @warning_color; } \
         label.error { color: @error_color; } \
         label.success { color: @success_color; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

impl StatusBar {
    /// 操作の失敗を、ステータスバーで知らせる（作業を止めない）。閉じるか、次の失敗・成功で消えるまで出す。
    /// 帯には、何が失敗したか（見出し）、なぜか（要点）、次にできること（ボタン）を出す。git の全文は「詳細」と
    /// ツールチップ。`open` は worktree を開く。
    pub fn show_failure(
        self: &Rc<Self>,
        failure: Failure,
        actions: &Rc<crate::actions::Actions>,
        open: Rc<dyn Fn(std::path::PathBuf)>,
    ) {
        let cancelled = failure.cancelled();
        let icon = gtk::Label::new(Some(if cancelled { "■" } else { "✕" }));
        if !cancelled {
            icon.add_css_class("error");
        }
        let title = gtk::Label::new(Some(&if cancelled {
            tr!("Operation Cancelled")
        } else {
            failure.title()
        }));
        title.add_css_class("heading");
        let detail = gtk::Label::new(Some(&if cancelled {
            String::new()
        } else {
            failure.summary()
        }));
        detail.add_css_class("caption");
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        detail.set_xalign(0.0);
        detail.set_hexpand(true);
        detail.set_tooltip_text(Some(&failure.message));
        let mut children: Vec<gtk::Widget> = vec![icon.upcast(), title.upcast(), detail.upcast()];
        if !cancelled {
            for (i, remedy) in failure.remedies().into_iter().enumerate() {
                let b = button(&remedy.title(), i == 0);
                let (this, actions, open, command) = (
                    Rc::downgrade(self),
                    actions.clone(),
                    open.clone(),
                    failure.command.clone(),
                );
                b.connect_clicked(move |b| {
                    if let Some(this) = this.upgrade() {
                        this.hide(Row::Failure);
                    }
                    let activate = |name: &str| {
                        let _ = b.activate_action(name, None);
                    };
                    match &remedy {
                        Remedy::OpenWorktree(path) => open(path.into()),
                        Remedy::OpenTerminal => activate("win.open-in-terminal"),
                        Remedy::Retry => {
                            if let Some(c) = &command {
                                actions.perform(c.clone());
                            }
                        }
                        Remedy::Pull => activate("win.pull"),
                        Remedy::WorkingCopy => activate("win.show-working-copy"),
                        Remedy::SetUpstream => activate("win.set-upstream"),
                    }
                });
                children.push(b.upcast());
            }
            let more = button(&tr!("Details"), false);
            let (f, a) = (failure.clone(), actions.clone());
            more.connect_clicked(move |_| {
                let text = [
                    f.kind.is_some().then(|| f.summary()),
                    f.advice(),
                    Some(f.message.clone()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n\n");
                let (title, parent) = (f.title(), crate::sheets::parent(&a));
                glib::spawn_future_local(async move {
                    crate::sheets::inform(parent.as_ref(), &title, &text).await;
                });
            });
            children.push(more.upcast());
            let activity = button("Activity", false);
            activity.set_action_name(Some("win.activity"));
            children.push(activity.upcast());
        }
        let close = button(&tr!("Close"), false);
        let this = Rc::downgrade(self);
        close.connect_clicked(move |_| {
            if let Some(this) = this.upgrade() {
                this.hide(Row::Failure);
            }
        });
        children.push(close.upcast());
        self.show(
            Row::Failure,
            &line(&children, (!cancelled).then_some("status-failure")),
        );
        // 取り消したことは、しばらくしたら消す。
        if cancelled {
            let this = Rc::downgrade(self);
            glib::timeout_add_seconds_local_once(5, move || {
                if let Some(this) = this.upgrade() {
                    this.hide(Row::Failure);
                }
            });
        }
    }

    /// merge・rebase・cherry-pick・revert・am の途中なら、色の付いた行を出す。1 行目に何をどこへしている途中か
    /// （何番目か）、2 行目にどこで・なぜ止まっていて次に何をすればよいか。`without` はマーカーの残っていない
    /// コンフリクトのファイル。
    pub fn show_sequence(
        self: &Rc<Self>,
        info: Option<&Value>,
        conflicts: usize,
        without: Vec<String>,
        actions: &Rc<crate::actions::Actions>,
    ) {
        let Some(s) = info else {
            self.hide(Row::Sequence);
            return;
        };
        let icon = gtk::Label::new(Some("⇄"));
        icon.add_css_class("warning");
        icon.add_css_class("title-3");
        let title = gtk::Label::new(Some(&sequence_title(s)));
        title.add_css_class("heading");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let detail_text = sequence_detail(s, conflicts, without.len());
        let detail = gtk::Label::new(Some(&detail_text));
        detail.add_css_class("caption");
        detail.set_xalign(0.0);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        detail.set_tooltip_text(Some(&detail_text));
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 1);
        texts.set_hexpand(true);
        texts.append(&title);
        texts.append(&detail);
        let mut children: Vec<gtk::Widget> = vec![icon.upcast(), texts.upcast()];
        if !without.is_empty() {
            let b = button(
                &tr!(
                    "Mark {count} Without Markers Resolved",
                    count = without.len()
                ),
                false,
            );
            b.set_tooltip_text(Some(&tr!("Mark the conflicted files that have no conflict markers left as resolved (rerere or another editor resolved them).")));
            let a = actions.clone();
            b.connect_clicked(move |_| {
                let paths = without.clone();
                a.perform_work(move |repo| {
                    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
                    repo.stage(&refs)
                });
            });
            children.push(b.upcast());
        }
        let show = button(&tr!("Open Working Copy"), false);
        show.set_action_name(Some("win.show-working-copy"));
        children.push(show.upcast());
        let kind = s["kind"].as_str().unwrap_or_default();
        let retry = kind == "am"
            && conflicts == 0
            && s["threeWay"].as_bool() != Some(true)
            && s["retried"].as_bool() != Some(true)
            && s["empty"].as_bool() != Some(true);
        if retry {
            let b = button(&tr!("Retry with 3-Way Merge"), true);
            let a = actions.clone();
            b.connect_clicked(move |_| a.perform(serde_json::json!({ "op": "amThreeWay" })));
            children.push(b.upcast());
        }
        let cont = button("Continue", conflicts == 0 && !retry);
        cont.set_action_name(Some("win.continue"));
        children.push(cont.upcast());
        if matches!(kind, "rebase" | "cherry-pick" | "am") {
            let skip = button("Skip", false);
            skip.set_action_name(Some("win.skip"));
            children.push(skip.upcast());
        }
        let abort = button("Abort", false);
        abort.set_action_name(Some("win.abort"));
        children.push(abort.upcast());
        self.show(Row::Sequence, &line(&children, Some("status-sequence")));
    }
}
