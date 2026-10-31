//! 操作の前の確認（macOS 版の `SyncSheets.swift`。docs/ux.md の「Confirmations and wording」）。外と話す・HEAD や
//! ブランチや作業ツリーを動かす・消す操作は、何が起きるかを窓に付いたシートで見せてから実行する。題は動詞で
//! 始まる問い 1 つ、説明は 2 文まで、ボタンは題の動詞（OK にしない）。取り消せないものだけ赤くし、Return で押さない。
//!
//! GTK にはシートがないので、窓に付いた modal の小さな窓（[`Sheet`]）で出す。

use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;

/// シートの答え。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Ok,
    /// 3 つ目のボタン（Push のシートの Pull… など）。
    Other,
    Cancel,
}

/// ボタンの色。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// 既定（Return で押す）。
    Suggested,
    /// 取り消せない（赤。Return では押さない）。
    Destructive,
}

/// 窓に付いた確認（題・説明・中身・ボタン）。
pub struct Sheet {
    window: gtk::Window,
    pub heading: gtk::Label,
    pub message: gtk::Label,
    body: gtk::Box,
    buttons: gtk::Box,
    tx: async_channel::Sender<Response>,
    rx: async_channel::Receiver<Response>,
}

impl Sheet {
    pub fn new(parent: Option<&gtk::Window>, title: &str, message: &str) -> Self {
        let heading = gtk::Label::new(Some(title));
        heading.add_css_class("title-3");
        heading.set_wrap(true);
        heading.set_max_width_chars(48);
        heading.set_xalign(0.0);
        let label = gtk::Label::new(Some(message));
        label.set_wrap(true);
        label.set_max_width_chars(60);
        label.set_xalign(0.0);
        label.set_visible(!message.is_empty());
        let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        body.append(&heading);
        body.append(&label);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        buttons.set_margin_top(6);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_top(18);
        content.set_margin_bottom(18);
        content.set_margin_start(20);
        content.set_margin_end(20);
        content.append(&body);
        content.append(&buttons);
        let window = gtk::Window::builder()
            .modal(true)
            .resizable(false)
            .default_width(480)
            .child(&content)
            .build();
        // 見出しの帯は出さない（シートの形）。
        window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
        window.set_transient_for(parent);
        let (tx, rx) = async_channel::bounded(1);
        let t = tx.clone();
        window.connect_close_request(move |_| {
            let _ = t.try_send(Response::Cancel);
            glib::Propagation::Proceed
        });
        let keys = gtk::EventControllerKey::new();
        let w = window.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape
                && let Some(w) = w.upgrade()
            {
                w.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        Self {
            window,
            heading,
            message: label,
            body,
            buttons,
            tx,
            rx,
        }
    }

    /// 説明の下に部品を足す。
    pub fn add(&self, widget: &impl IsA<gtk::Widget>) {
        self.body.append(widget);
    }

    /// ボタンを足す（右から Ok、その左に Cancel、さらに左に Other の順に並ぶよう、足した順に左から並べる）。
    pub fn button(&self, label: &str, response: Response, style: Option<Style>) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        match style {
            Some(Style::Suggested) => {
                button.add_css_class("suggested-action");
                self.window.set_default_widget(Some(&button));
            }
            Some(Style::Destructive) => button.add_css_class("destructive-action"),
            None => {}
        }
        let (tx, w) = (self.tx.clone(), self.window.downgrade());
        button.connect_clicked(move |_| {
            let _ = tx.try_send(response);
            if let Some(w) = w.upgrade() {
                w.destroy();
            }
        });
        self.buttons.append(&button);
        button
    }

    /// よくある形: [Other] Cancel <ok>。
    pub fn buttons(&self, ok: &str, destructive: bool, other: Option<&str>) -> gtk::Button {
        if let Some(other) = other {
            let b = self.button(other, Response::Other, None);
            b.set_hexpand(true);
            b.set_halign(gtk::Align::Start);
        }
        self.button(&tr!("Cancel"), Response::Cancel, None);
        self.button(
            ok,
            Response::Ok,
            Some(if destructive {
                Style::Destructive
            } else {
                Style::Suggested
            }),
        )
    }

    /// 出して、答えを待つ。
    pub async fn run(self) -> Response {
        self.window.present();
        self.rx.recv().await.unwrap_or(Response::Cancel)
    }
}

/// 説明の文の行（小さく、薄く）。`lines` 行分の高さを取っておく（答えが来ても大きさを変えない）。
pub fn note(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_wrap(true);
    l.set_max_width_chars(60);
    l.set_xalign(0.0);
    l.add_css_class("dim-label");
    l.add_css_class("caption");
    l
}

/// 「名前: 部品」の行。
pub fn row(name: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let l = gtk::Label::new(Some(name));
    l.set_xalign(1.0);
    l.set_width_chars(12);
    b.append(&l);
    b.append(widget);
    b
}

pub const CAN_UNDO_KEY: &str = "You can Undo this (Ctrl+Alt+Z).";

/// 「Undo で戻せます」の文。
pub fn can_undo() -> String {
    amonhen_model::i18n::translate(CAN_UNDO_KEY).into_owned()
}

/// 「取り消せません」の文。
pub fn cant_undo() -> String {
    tr!("This can’t be undone.")
}

/// コアの命令を裏で実行し、答え（JSON）を返す。
pub async fn run(repo: &Arc<Repository>, command: Value) -> Value {
    let repo = repo.clone();
    gio::spawn_blocking(move || {
        serde_json::from_str(&repo.run_json(&command.to_string())).unwrap_or_default()
    })
    .await
    .unwrap_or_default()
}

/// `between` の答え: 片側にだけあるコミット（新しい順）。
#[derive(Clone, Default)]
pub struct CommitRange {
    pub count: u64,
    /// (oid, 件名, 作者, 時刻)。
    pub commits: Vec<(String, String, String, i64)>,
}

impl CommitRange {
    fn from(info: &Value) -> Self {
        Self {
            count: info["count"].as_u64().unwrap_or(0),
            commits: info["commits"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| {
                    (
                        c["oid"].as_str().unwrap_or_default().to_owned(),
                        c["subject"].as_str().unwrap_or_default().to_owned(),
                        c["author"].as_str().unwrap_or_default().to_owned(),
                        c["time"].as_i64().unwrap_or(0),
                    )
                })
                .collect(),
        }
    }
}

/// `tip` にあって `base` にないコミット。
pub async fn between(repo: &Arc<Repository>, base: &str, tip: &str, limit: u32) -> CommitRange {
    let reply = run(
        repo,
        json!({ "op": "between", "base": base, "tip": tip, "limit": limit }),
    )
    .await;
    CommitRange::from(&reply["info"])
}

fn time(t: i64) -> String {
    glib::DateTime::from_unix_local(t)
        .and_then(|d| d.format("%Y-%m-%d %H:%M"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// コミットの一覧（見出しと、短い ID・件名・作者・日時）。空の一覧は出さない。`warn` は消えるもの（赤）。
pub fn commit_lists(lists: &[(String, &CommitRange, bool)]) -> gtk::Box {
    let out = gtk::Box::new(gtk::Orientation::Vertical, 4);
    for (title, range, warn) in lists {
        if range.count == 0 {
            continue;
        }
        let header = gtk::Label::new(Some(&format!("{title} ({})", range.count)));
        header.set_xalign(0.0);
        header.add_css_class("heading");
        if *warn {
            header.add_css_class("error");
        }
        out.append(&header);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        list.set_margin_start(4);
        for (oid, subject, author, t) in &range.commits {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let id = gtk::Label::new(Some(&oid[..oid.len().min(8)]));
            id.add_css_class("monospace");
            id.add_css_class("dim-label");
            let s = gtk::Label::new(Some(subject));
            s.set_ellipsize(gtk::pango::EllipsizeMode::End);
            s.set_xalign(0.0);
            s.set_hexpand(true);
            if *warn {
                s.add_css_class("error");
            }
            let who = gtk::Label::new(Some(&format!("{author} · {}", time(*t))));
            who.add_css_class("dim-label");
            who.add_css_class("caption");
            line.append(&id);
            line.append(&s);
            line.append(&who);
            list.append(&line);
        }
        if range.count > range.commits.len() as u64 {
            let more = note(&tr!(
                "and {count} more",
                count = range.count - range.commits.len() as u64
            ));
            list.append(&more);
        }
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_width(440)
            .min_content_height((range.commits.len().min(8) as i32) * 22 + 6)
            .max_content_height(8 * 22 + 6)
            .propagate_natural_height(true)
            .build();
        scrolled.add_css_class("frame");
        out.append(&scrolled);
    }
    out
}

/// `predictMerge` の答え（コンフリクトするファイル。調べられなければ None）。
pub struct MergeForecast {
    pub conflicts: Option<Vec<String>>,
}

impl MergeForecast {
    pub fn has_conflicts(&self) -> bool {
        self.conflicts.as_ref().is_some_and(|c| !c.is_empty())
    }

    /// 見せる文。`estimate` なら、rebase の目安だと書き添える。
    pub fn text(&self, estimate: bool) -> String {
        let Some(conflicts) = &self.conflicts else {
            return tr!("Can’t check for conflicts ahead (needs git 2.38 or newer).");
        };
        let mut names = conflicts
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if conflicts.len() > 3 {
            names.push_str(", …");
        }
        let line = if conflicts.is_empty() {
            tr!("No conflicts expected.")
        } else {
            tr!(
                "⚠︎ Conflicts expected in {count} files: {names}",
                count = conflicts.len(),
                names = names
            )
        };
        if estimate {
            format!(
                "{line} {}",
                tr!("Rebase applies the commits one at a time, so this is only an estimate.")
            )
        } else {
            line
        }
    }
}

/// `theirs` を `ours` にマージしたらコンフリクトするか（作業ツリーと index には触らない）。
pub async fn predict_merge(repo: &Arc<Repository>, ours: &str, theirs: &[String]) -> MergeForecast {
    let theirs: Value = if theirs.len() == 1 {
        json!(theirs[0])
    } else {
        json!(theirs)
    };
    let reply = run(
        repo,
        json!({ "op": "predictMerge", "ours": ours, "theirs": theirs }),
    )
    .await;
    MergeForecast {
        conflicts: reply["info"]["conflicts"].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        }),
    }
}

/// コンフリクトの見込みの行（最初は「調べています」）。
pub fn forecast_label() -> gtk::Label {
    let l = note(&tr!("Checking for conflicts…"));
    l.set_lines(3);
    l
}

pub fn show_forecast(label: &gtk::Label, forecast: &MergeForecast, estimate: bool) {
    label.set_text(&forecast.text(estimate));
    if forecast.has_conflicts() {
        label.add_css_class("warning");
    } else {
        label.remove_css_class("warning");
    }
}

/// `rewritePreview` の答えを、確認に出す行にする（動く・残るブランチ、push 済み、コンフリクト）。
pub fn rewrite_lines(info: &Value) -> (Vec<String>, bool) {
    let names = |list: &[Value]| -> String {
        list.iter()
            .filter_map(|b| b["name"].as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let moving = info["moving"].as_array().cloned().unwrap_or_default();
    let staying = info["staying"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    if !moving.is_empty() {
        out.push(tr!("Also moves {names}.", names = names(&moving)));
    }
    let stay = |reason: &str| -> Vec<Value> {
        staying
            .iter()
            .filter(|b| b["reason"].as_str() == Some(reason))
            .cloned()
            .collect()
    };
    for (reason, key) in [
        (
            "setting",
            "{names} stays where it is (rebase.updateRefs is off).",
        ),
        (
            "git",
            "{names} stays where it is (moving it needs git 2.38).",
        ),
        (
            "checkedOut",
            "{names} stays where it is (checked out in another worktree).",
        ),
        ("off", "{names} stays where it is (not moved this time)."),
    ] {
        let list = stay(reason);
        if !list.is_empty() {
            out.push(amonhen_model::i18n::fill(
                &amonhen_model::i18n::translate(key),
                &[("names", &names(&list))],
            ));
        }
    }
    let mut pushed: Vec<String> = Vec::new();
    if info["currentUpstream"].is_string()
        && let Some(c) = info["current"].as_str()
    {
        pushed.push(c.to_owned());
    }
    pushed.extend(
        moving
            .iter()
            .filter(|b| b["upstream"].is_string())
            .filter_map(|b| b["name"].as_str().map(str::to_owned)),
    );
    if !pushed.is_empty() {
        let list = pushed.join(", ");
        out.push(if pushed.len() == 1 {
            tr!(
                "{names} is pushed, so pushing afterwards needs Force Push.",
                names = list
            )
        } else {
            tr!(
                "{names} are pushed, so pushing afterwards needs Force Push.",
                names = list
            )
        });
    }
    let mut conflicts = false;
    if let Some(list) = info["conflicts"].as_array() {
        if let Some(first) = list.first() {
            conflicts = true;
            let oid = first["oid"].as_str().unwrap_or_default();
            let files: Vec<&str> = first["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str())
                .collect();
            let mut shown = files.iter().take(3).copied().collect::<Vec<_>>().join(", ");
            if files.len() > 3 {
                shown.push_str(", …");
            }
            let short = &oid[..oid.len().min(8)];
            out.push(if list.len() == 1 {
                tr!(
                    "⚠︎ Conflicts expected at {commit}: {files}",
                    commit = short,
                    files = shown
                )
            } else {
                tr!(
                    "⚠︎ Conflicts expected at {count} commits, first at {commit}: {files}",
                    count = list.len(),
                    commit = short,
                    files = shown
                )
            });
        } else {
            out.push(tr!("No conflicts expected."));
        }
    }
    (out, conflicts)
}

/// 今のブランチとその上流、上流との差（push・pull の確認に使う）。
pub struct SyncState {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    /// 上流になくて手元にある（push で送る）。
    pub outgoing: CommitRange,
    /// 上流にあって手元にない（pull で取り込む）。
    pub incoming: CommitRange,
    /// 上流のコミット（16 進、最後の Fetch の時点）。Force Push はリモートがこれのときだけ置き換える。
    pub upstream_target: Option<String>,
    /// コミットしていない変更のあるファイルの数（untracked を除く）。
    pub changed: usize,
}

/// `branch`（なければ今のブランチ）の同期の状態。
pub async fn sync_state(repo: &Arc<Repository>, branch: Option<String>) -> Option<SyncState> {
    let r = repo.clone();
    let b = branch.clone();
    let (status, refs) = gio::spawn_blocking(move || {
        let status = r.load_status().ok()?;
        let refs = r.load_refs().ok()?;
        Some((status, refs))
    })
    .await
    .ok()
    .flatten()?;
    let local = b.as_deref().and_then(|name| {
        refs.refs
            .entries
            .iter()
            .find(|i| i.kind == amonhen_model::RefKind::Local && i.name == name)
    });
    let name = local
        .map(|l| l.name.clone())
        .or_else(|| status.status.branch.clone());
    let upstream = match local {
        Some(l) if l.gone => None,
        Some(l) => l.detail.clone(),
        None => status.status.upstream.clone(),
    };
    let changed = status
        .status
        .entries
        .iter()
        .filter(|e| e.kind != amonhen_model::StatusKind::Untracked)
        .count();
    let tip = local.map_or("HEAD".to_owned(), |l| l.name.clone());
    let Some(upstream) = upstream else {
        return Some(SyncState {
            branch: name,
            upstream: None,
            outgoing: CommitRange::default(),
            incoming: CommitRange::default(),
            upstream_target: None,
            changed,
        });
    };
    let upstream_target = refs
        .refs
        .entries
        .iter()
        .find(|i| i.kind == amonhen_model::RefKind::Remote && i.name == upstream)
        .and_then(|i| i.target.map(|t| t.to_string()));
    let outgoing = between(repo, &upstream, &tip, 500).await;
    let incoming = between(repo, &tip, &upstream, 500).await;
    Some(SyncState {
        branch: name,
        upstream: Some(upstream),
        outgoing,
        incoming,
        upstream_target,
        changed,
    })
}

/// 知らせる（ボタンは閉じるだけ）。
pub async fn inform(parent: Option<&gtk::Window>, title: &str, message: &str) {
    let sheet = Sheet::new(parent, title, message);
    sheet.button(&tr!("Close"), Response::Ok, Some(Style::Suggested));
    sheet.run().await;
}

/// 確認の窓の親（操作の窓）。
pub fn parent(actions: &Actions) -> Option<gtk::Window> {
    actions.window().map(|w| w.upcast())
}

/// Fetch の前に、どこから取るかを見せる。リモートがいくつもあれば選べる（最初はすべて）。
pub fn fetch(actions: &Rc<Actions>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let remotes: Vec<String> = run(&repo, json!({ "op": "remotes" })).await["info"]["remotes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["name"].as_str().map(str::to_owned))
            .collect();
        if remotes.is_empty() {
            return;
        }
        let config = run(&repo, json!({ "op": "configList" })).await;
        let entries: Vec<(String, String)> = config["info"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some((
                    e["key"].as_str()?.to_lowercase(),
                    e["value"].as_str()?.to_lowercase(),
                ))
            })
            .collect();
        let value = move |key: &str| {
            entries
                .iter()
                .rev()
                .find(|(k, _)| *k == key.to_lowercase())
                .map(|(_, v)| v.clone())
        };
        // 片づけるかは git と同じく、remote.<name>.prune、なければ fetch.prune。効いている設定の名前。
        let prune_key = move |remote: &str| -> Option<String> {
            let own = format!("remote.{remote}.prune");
            let key = if value(&own).is_some() {
                own
            } else {
                "fetch.prune".to_owned()
            };
            (value(&key).as_deref() == Some("true")).then_some(key)
        };
        let prune_line = move |chosen: &[String]| -> String {
            let names: Vec<&String> = chosen.iter().filter(|r| prune_key(r).is_some()).collect();
            if names.is_empty() {
                return String::new();
            }
            let mut keys: Vec<String> = Vec::new();
            for key in names.iter().filter_map(|r| prune_key(r)) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            if names.len() == chosen.len() {
                tr!(
                    "Remote branches deleted there are removed here too ({settings}).",
                    settings = keys.join(", ")
                )
            } else {
                tr!(
                    "Remote branches deleted on {remotes} are removed here too ({settings}).",
                    remotes = names
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    settings = keys.join(", ")
                )
            }
        };
        let title = if remotes.len() == 1 {
            tr!("Fetch from {remote}?", remote = remotes[0])
        } else {
            tr!("Fetch?")
        };
        let sheet = Sheet::new(parent(&actions).as_ref(), &title, &prune_line(&remotes));
        let mut choices = vec![tr!("All Remotes ({names})", names = remotes.join(", "))];
        choices.extend(remotes.iter().cloned());
        let refs: Vec<&str> = choices.iter().map(String::as_str).collect();
        let from = gtk::DropDown::from_strings(&refs);
        if remotes.len() > 1 {
            sheet.add(&row(&tr!("From:"), &from));
            let (message, remotes, prune_line) =
                (sheet.message.clone(), remotes.clone(), prune_line.clone());
            from.connect_selected_notify(move |d| {
                let chosen = match d.selected() {
                    0 => remotes.clone(),
                    i => vec![remotes[i as usize - 1].clone()],
                };
                let text = prune_line(&chosen);
                message.set_text(&text);
                message.set_visible(!text.is_empty());
            });
        }
        sheet.buttons("Fetch", false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let remote = match from.selected() {
            0 => None,
            i => remotes.get(i as usize - 1).cloned(),
        };
        actions.perform(json!({ "op": "fetch", "remote": remote }));
    });
}

/// 取り込み方（Settings の Pull の既定と同じ名前）。
const PULL_MODES: [&str; 4] = ["git", "merge", "rebase", "fastForwardOnly"];

fn pull_mode_title(mode: &str) -> String {
    match mode {
        "git" => tr!("Follow git’s settings"),
        "rebase" => "Rebase".to_owned(),
        "fastForwardOnly" => "Fast-Forward Only".to_owned(),
        _ => "Merge".to_owned(),
    }
}

/// Pull の前に、取り込むコミットと取り込み方を見せる。上流がなければ、取り込み元のリモートのブランチを選び、それを
/// 上流にして取り込む。
pub fn pull(actions: &Rc<Actions>, mode: &str) {
    let (actions, mode) = (actions.clone(), mode.to_owned());
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let Some(state) = sync_state(&repo, None).await else {
            return;
        };
        let Some(branch) = state.branch.clone() else {
            return;
        };
        // 上流がなければ、取り込み元を選ぶ（同じ名前のリモートのブランチを先に）。
        let mut sources: Vec<String> = Vec::new();
        if state.upstream.is_none() {
            let r = repo.clone();
            let refs = gio::spawn_blocking(move || r.load_refs().ok())
                .await
                .ok()
                .flatten();
            let remotes: Vec<String> = refs
                .iter()
                .flat_map(|r| r.refs.entries.iter())
                .filter(|i| i.kind == amonhen_model::RefKind::Remote && !i.name.ends_with("/HEAD"))
                .map(|i| i.name.clone())
                .collect();
            let same = |n: &String| n.split_once('/').is_some_and(|(_, b)| b == branch);
            sources = remotes.iter().filter(|n| same(n)).cloned().collect();
            sources.extend(remotes.iter().filter(|n| !same(n)).cloned());
            if sources.is_empty() {
                inform(
                    parent(&actions).as_ref(),
                    &tr!("No remote branch to pull from"),
                    &tr!("Fetch, or add a remote with Repository > Add Remote…."),
                )
                .await;
                return;
            }
        }
        let by_git = run(&repo, json!({ "op": "syncDefaults" })).await["info"]["pull"]
            .as_str()
            .unwrap_or("merge")
            .to_owned();
        let (incoming, outgoing) = (state.incoming.count, state.outgoing.count);
        let title = match &state.upstream {
            Some(u) if incoming > 0 => {
                tr!(
                    "Pull {count} commits from {upstream}?",
                    count = incoming,
                    upstream = u
                )
            }
            Some(u) => tr!("Pull from {upstream}?", upstream = u),
            None => tr!("Pull into {branch}?", branch = branch),
        };
        let sheet = Sheet::new(parent(&actions).as_ref(), &title, "");
        let source_refs: Vec<&str> = sources.iter().map(String::as_str).collect();
        let source = gtk::DropDown::from_strings(&source_refs);
        if !sources.is_empty() {
            sheet.add(&row(&tr!("From:"), &source));
        }
        let mode_titles: Vec<String> = PULL_MODES
            .iter()
            .map(|m| {
                if *m == "git" {
                    format!("{} ({})", pull_mode_title(m), pull_mode_title(&by_git))
                } else {
                    pull_mode_title(m)
                }
            })
            .collect();
        let mode_refs: Vec<&str> = mode_titles.iter().map(String::as_str).collect();
        let modes = gtk::DropDown::from_strings(&mode_refs);
        modes.set_selected(PULL_MODES.iter().position(|m| *m == mode).unwrap_or(0) as u32);
        sheet.add(&row(&tr!("Integrate with:"), &modes));
        let explain = note("");
        sheet.add(&explain);
        let forecast =
            (state.upstream.is_some() && incoming > 0 && outgoing > 0).then(forecast_label);
        if let Some(f) = &forecast {
            sheet.add(f);
        }
        sheet.add(&commit_lists(&[
            (tr!("Commits to pull"), &state.incoming, false),
            (tr!("Commits only here"), &state.outgoing, false),
        ]));
        let ok = sheet.buttons("Pull", false, None);
        let chosen = {
            let modes = modes.clone();
            move || PULL_MODES[modes.selected() as usize].to_owned()
        };
        let method = {
            let by_git = by_git.clone();
            move |m: &str| {
                if m == "git" {
                    by_git.clone()
                } else {
                    m.to_owned()
                }
            }
        };
        // 今の状態で、選んだ取り込み方が何をするか。
        let describe = {
            let (branch, upstream, source, method) = (
                branch.clone(),
                state.upstream.clone(),
                source.clone(),
                method.clone(),
            );
            let changed = state.changed;
            move |m: &str| -> (String, bool) {
                let mut lines = Vec::new();
                let mut blocked = false;
                if upstream.is_none() {
                    let picked = source
                        .selected_item()
                        .and_downcast::<gtk::StringObject>()
                        .map(|s| s.string().to_string())
                        .unwrap_or_default();
                    lines.push(tr!(
                        "Sets {source} as the upstream, then pulls.",
                        source = picked
                    ));
                } else if incoming == 0 {
                    lines.push(tr!(
                        "Nothing new as of the last Fetch (Pull fetches first)."
                    ));
                } else if outgoing == 0 {
                    lines.push(tr!(
                        "Moves {branch} forward (fast-forward).",
                        branch = branch
                    ));
                } else {
                    match method(m).as_str() {
                        "rebase" => lines.push(tr!(
                            "Your {count} local commits are replayed on top (their IDs change).",
                            count = outgoing
                        )),
                        "fastForwardOnly" => {
                            lines.push(tr!(
                                "⚠︎ Fast-Forward Only can’t pull these: you have {count} local commits. Choose Merge or Rebase.",
                                count = outgoing
                            ));
                            blocked = true;
                        }
                        _ => lines.push(tr!(
                            "You have {count} local commits, so this makes a merge commit.",
                            count = outgoing
                        )),
                    }
                }
                if changed > 0 {
                    lines.push(tr!(
                        "{count} files have uncommitted changes. Pull stops if the incoming changes touch them.",
                        count = changed
                    ));
                }
                lines.push(can_undo());
                (lines.join("\n"), blocked)
            }
        };
        let forecast_answer: Rc<std::cell::RefCell<Option<MergeForecast>>> = Rc::default();
        let update = {
            let (explain, ok, forecast, answer, chosen, method, describe) = (
                explain.clone(),
                ok.clone(),
                forecast.clone(),
                forecast_answer.clone(),
                chosen.clone(),
                method.clone(),
                describe.clone(),
            );
            Rc::new(move || {
                let (text, blocked) = describe(&chosen());
                explain.set_text(&text);
                ok.set_sensitive(!blocked);
                if let Some(label) = &forecast {
                    let m = method(&chosen());
                    if m == "fastForwardOnly" {
                        label.set_text("");
                    } else if let Some(f) = answer.borrow().as_ref() {
                        show_forecast(label, f, m == "rebase");
                    }
                }
            })
        };
        update();
        let u = update.clone();
        modes.connect_selected_notify(move |_| u());
        let u = update.clone();
        source.connect_selected_notify(move |_| u());
        if forecast.is_some()
            && let Some(upstream) = state.upstream.clone()
        {
            let (repo, answer, update) = (repo.clone(), forecast_answer.clone(), update.clone());
            glib::spawn_future_local(async move {
                let f = predict_merge(&repo, "HEAD", &[upstream]).await;
                answer.replace(Some(f));
                update();
            });
        }
        if sheet.run().await != Response::Ok {
            return;
        }
        let pull = json!({ "op": "pull", "mode": chosen() });
        let picked = source
            .selected_item()
            .and_downcast::<gtk::StringObject>()
            .map(|s| s.string().to_string());
        match (state.upstream.is_none(), picked.as_deref().and_then(|p| p.split_once('/'))) {
            (true, Some((remote, name))) => actions.perform_sequence(vec![
                json!({ "op": "configSet", "key": format!("branch.{branch}.remote"), "value": remote }),
                json!({ "op": "configSet", "key": format!("branch.{branch}.merge"), "value": format!("refs/heads/{name}") }),
                pull,
            ]),
            _ => actions.perform(pull),
        }
    });
}

/// Push の前に、送るコミットを見せる（1 つのシート）。上流が先に進んでいれば、同じシートで消えるコミットを見せて
/// Pull… か Force Push を選ばせる。Force Push は、シートで見せた上流のコミットのときだけ置き換える。上流がなければ
/// Publish。`branch` を渡せば今のブランチ以外も送れる。送り終えたら `then`。
pub fn push(
    actions: &Rc<Actions>,
    branch: Option<String>,
    force: bool,
    then: Option<Box<dyn FnOnce()>>,
) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let Some(state) = sync_state(&repo, branch).await else {
            return;
        };
        let Some(branch) = state.branch.clone() else {
            return;
        };
        let Some(upstream) = state.upstream.clone() else {
            return publish(&actions, &branch, then);
        };
        let (outgoing, incoming) = (state.outgoing.count, state.incoming.count);
        let diverged = !force && incoming > 0;
        if !(force || diverged || outgoing > 0) || (diverged && outgoing == 0) {
            return;
        }
        let (title, message) = if diverged {
            (
                tr!(
                    "{upstream} has {count} commits you don’t have",
                    upstream = upstream,
                    count = incoming
                ),
                tr!("Pull them first, or force push to replace them. Force Push can’t be undone."),
            )
        } else if force {
            (
                tr!("Force push {branch}?", branch = branch),
                if incoming > 0 {
                    tr!(
                        "{upstream} loses {count} commits. This can’t be undone.",
                        upstream = upstream,
                        count = incoming
                    )
                } else {
                    tr!(
                        "It fails if {upstream} has moved since you last fetched.",
                        upstream = upstream
                    )
                },
            )
        } else {
            (
                tr!(
                    "Push {count} commits to {upstream}?",
                    count = outgoing,
                    upstream = upstream
                ),
                String::new(),
            )
        };
        let sheet = Sheet::new(parent(&actions).as_ref(), &title, &message);
        let lost = if force || diverged {
            state.incoming.clone()
        } else {
            CommitRange::default()
        };
        sheet.add(&commit_lists(&[
            (tr!("Commits to send"), &state.outgoing, false),
            (
                tr!("Commits {upstream} will lose", upstream = upstream),
                &lost,
                true,
            ),
        ]));
        let replace = force || diverged;
        sheet.buttons(
            if replace { "Force Push" } else { "Push" },
            replace,
            diverged.then_some("Pull…"),
        );
        match sheet.run().await {
            Response::Ok => {
                let (remote, remote_branch) = upstream
                    .split_once('/')
                    .map(|(r, b)| (r.to_owned(), b.to_owned()))
                    .unwrap_or_default();
                let command = json!({
                    "op": "push", "remote": remote, "branch": branch, "remoteBranch": remote_branch,
                    "force": replace, "expect": if replace { state.upstream_target.clone() } else { None },
                    "setUpstream": false,
                });
                actions.perform_with(vec![command], move |reply| {
                    if reply["ok"].as_bool() == Some(true)
                        && let Some(then) = then
                    {
                        then();
                    }
                });
            }
            Response::Other => pull(&actions, "git"),
            Response::Cancel => {}
        }
    });
}

/// 上流のないブランチを初めて送る（Publish）。送り先のリモートとそこでの名前を選ばせ、送ったらそれを上流にする。
pub fn publish(actions: &Rc<Actions>, branch: &str, then: Option<Box<dyn FnOnce()>>) {
    let (actions, branch) = (actions.clone(), branch.to_owned());
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let remotes: Vec<String> = run(&repo, json!({ "op": "remotes" })).await["info"]["remotes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["name"].as_str().map(str::to_owned))
            .collect();
        let window = parent(&actions);
        if remotes.is_empty() {
            inform(
                window.as_ref(),
                &tr!("Can’t Publish"),
                &tr!("This repository has no remotes. Add one with Repository > Add Remote…."),
            )
            .await;
            return;
        }
        let config = run(&repo, json!({ "op": "configList" })).await;
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
        let preferred = [
            value(&format!("branch.{branch}.pushRemote")),
            value("remote.pushDefault"),
            Some(if remotes.len() == 1 {
                remotes[0].clone()
            } else {
                "origin".to_owned()
            }),
        ]
        .into_iter()
        .flatten()
        .find(|r| remotes.contains(r));
        let sheet = Sheet::new(
            window.as_ref(),
            &tr!("Publish {branch}?", branch = branch),
            &tr!(
                "This creates the branch on the remote and makes it the upstream (where Pull and Push go from now on)."
            ),
        );
        let refs: Vec<&str> = remotes.iter().map(String::as_str).collect();
        let remote = gtk::DropDown::from_strings(&refs);
        if let Some(p) = &preferred {
            remote.set_selected(remotes.iter().position(|r| r == p).unwrap_or(0) as u32);
        }
        let name = gtk::Entry::builder().text(&branch).hexpand(true).build();
        sheet.add(&row(&tr!("Remote"), &remote));
        sheet.add(&row(&tr!("Name there"), &name));
        sheet.buttons("Publish", false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let remote = remotes[remote.selected() as usize].clone();
        let remote_branch = name.text().trim().to_owned();
        if remote_branch.is_empty() {
            return;
        }
        let target = format!("{remote}/{remote_branch}");
        let r = repo.clone();
        let exists = gio::spawn_blocking(move || r.load_refs().ok())
            .await
            .ok()
            .flatten()
            .is_some_and(|refs| {
                refs.refs
                    .entries
                    .iter()
                    .any(|i| i.kind == amonhen_model::RefKind::Remote && i.name == target)
            });
        // その名前のブランチがもうリモートにある（最後の Fetch の時点）。作るのではなく、それを上流にして送る。
        if exists {
            let outgoing = between(&repo, &target, &branch, 1).await.count;
            let incoming = between(&repo, &branch, &target, 1).await.count;
            let set_upstream = vec![
                json!({ "op": "configSet", "key": format!("branch.{branch}.remote"), "value": remote }),
                json!({ "op": "configSet", "key": format!("branch.{branch}.merge"), "value": format!("refs/heads/{remote_branch}") }),
            ];
            let title = tr!("{target} already exists", target = target);
            if incoming > 0 {
                let sheet = Sheet::new(
                    window.as_ref(),
                    &title,
                    &tr!(
                        "It has {count} commits that {branch} doesn’t (as of the last Fetch), so pushing would be refused. Make it the upstream and Pull first.",
                        count = incoming,
                        branch = branch
                    ),
                );
                sheet.buttons(&tr!("Set Upstream"), false, None);
                if sheet.run().await == Response::Ok {
                    actions.perform_sequence(set_upstream);
                }
                return;
            }
            let sheet = Sheet::new(
                window.as_ref(),
                &title,
                &if outgoing > 0 {
                    tr!(
                        "This pushes {count} commits to it and makes it the upstream.",
                        count = outgoing
                    )
                } else {
                    tr!(
                        "It is the same as {branch}. This makes it the upstream.",
                        branch = branch
                    )
                },
            );
            let ok = if outgoing > 0 {
                "Push".to_owned()
            } else {
                tr!("Set Upstream")
            };
            sheet.buttons(&ok, false, None);
            if sheet.run().await != Response::Ok {
                return;
            }
        }
        actions.perform_with(
            vec![json!({
                "op": "push", "remote": remote, "branch": branch, "remoteBranch": remote_branch,
                "force": false, "setUpstream": true,
            })],
            move |reply| {
                if reply["ok"].as_bool() == Some(true)
                    && let Some(then) = then
                {
                    then();
                }
            },
        );
    });
}

/// 切り替える先。
pub enum CheckoutTarget {
    Branch(String),
    /// リモートのブランチを追跡するローカルブランチを作って切り替える。
    Track(String),
    /// コミットを detached HEAD で。
    Detach(String),
}

/// Checkout の前に、何が起きるか（作業中の変更はそのまま持って行く、追跡ブランチを作る、detached になる）を見せる。
pub fn checkout(actions: &Rc<Actions>, target: CheckoutTarget) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let r = repo.clone();
        let changed = gio::spawn_blocking(move || {
            r.load_status().map_or(0, |s| {
                s.status
                    .entries
                    .iter()
                    .filter(|e| e.kind != amonhen_model::StatusKind::Untracked)
                    .count()
            })
        })
        .await
        .unwrap_or(0);
        let (title, command, line) = match &target {
            CheckoutTarget::Branch(name) => (
                tr!("Checkout {name}?", name = name),
                json!({ "op": "checkout", "branch": name }),
                String::new(),
            ),
            CheckoutTarget::Track(remote) => (
                tr!("Checkout {name}?", name = remote),
                json!({ "op": "track", "remoteBranch": remote }),
                tr!(
                    "Creates {local} tracking {remote}.",
                    local = remote.split_once('/').map_or(remote.as_str(), |(_, b)| b),
                    remote = remote
                ),
            ),
            CheckoutTarget::Detach(oid) => (
                tr!("Checkout {name}?", name = &oid[..oid.len().min(8)]),
                json!({ "op": "detach", "oid": oid }),
                tr!("HEAD is detached (on no branch)."),
            ),
        };
        let mut lines = vec![line];
        if changed > 0 {
            lines.push(tr!(
                "{count} files with uncommitted changes come along.",
                count = changed
            ));
        }
        let message = lines
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let sheet = Sheet::new(parent(&actions).as_ref(), &title, &message);
        sheet.buttons("Checkout", false, None);
        if sheet.run().await == Response::Ok {
            actions.perform(command);
        }
    });
}

/// 確かめてから `commands` を順に実行する（Undo できる操作）。
pub fn confirm_then(
    actions: &Rc<Actions>,
    title: String,
    message: String,
    ok: String,
    destructive: bool,
    commands: Vec<Value>,
) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let sheet = Sheet::new(parent(&actions).as_ref(), &title, &message);
        sheet.buttons(&ok, destructive, None);
        if sheet.run().await == Response::Ok {
            actions.perform_sequence(commands);
        }
    });
}

/// Cherry-pick・Revert の前に確かめる。`commits` は（16 進, merge か）を、実行する順に。
pub fn pick(actions: &Rc<Actions>, revert: bool, commits: Vec<(String, bool)>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let onto = gio::spawn_blocking(move || repo.load_status().ok()?.status.branch.clone())
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "HEAD".to_owned());
        let what = if commits.len() == 1 {
            crate::titles::short(&commits[0].0).to_owned()
        } else {
            tr!("{count} commits", count = commits.len())
        };
        let title = if revert {
            tr!("Revert {what} on {onto}?", what = what, onto = onto)
        } else {
            tr!("Cherry-pick {what} onto {onto}?", what = what, onto = onto)
        };
        let message = format!(
            "{} {}",
            if revert {
                tr!("Adds commits that undo them.")
            } else {
                tr!("Adds copies of them.")
            },
            can_undo()
        );
        let op = if revert { "revert" } else { "cherryPick" };
        let commands = commits
            .into_iter()
            .map(|(oid, merge)| json!({ "op": op, "oid": oid, "isMerge": merge }))
            .collect();
        let ok = if revert { "Revert" } else { "Cherry-pick" };
        confirm_then(&actions, title, message, ok.to_owned(), false, commands);
    });
}

/// stash を作業ツリーに戻す前に、どの stash かを見せる。`pop` なら一覧から消す（コンフリクトしたら残す）。
pub fn pop(actions: &Rc<Actions>, name: &str, message: Option<&str>, pop: bool) {
    let what = if pop {
        tr!("Applies it to the working tree and removes it from the list.")
    } else {
        tr!("Applies it to the working tree. It stays in the list.")
    };
    let mut text = message.map(|m| format!("“{m}”\n")).unwrap_or_default();
    text.push_str(&format!("{what} {}", can_undo()));
    let title = if pop {
        tr!("Pop {name}?", name = name)
    } else {
        tr!("Apply {name}?", name = name)
    };
    confirm_then(
        actions,
        title,
        text,
        if pop { "Pop" } else { "Apply" }.to_owned(),
        false,
        vec![json!({ "op": "stashApply", "name": name, "pop": pop })],
    );
}

/// Undo の前に、何を戻すかを見せる。
pub fn undo(actions: &Rc<Actions>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let peek = run(&repo, json!({ "op": "undoPeek" })).await;
        let Some(label) = peek["label"].as_str() else {
            return;
        };
        let what = match peek["info"]["kind"].as_str() {
            Some("files") => tr!("Puts the files back as they were. HEAD and the index stay."),
            Some("stash") => tr!("Brings back the dropped stash."),
            Some("rename") => tr!("Gives the branch its earlier name back."),
            _ => tr!("Returns HEAD and the branches to where they were."),
        };
        let sheet = Sheet::new(
            parent(&actions).as_ref(),
            &tr!("Undo “{label}”?", label = label),
            &what,
        );
        sheet.buttons("Undo", false, None);
        if sheet.run().await == Response::Ok {
            actions.perform(json!({ "op": "undo" }));
        }
    });
}

/// 相手の決まった Merge・Rebase の前に確かめる。
pub fn integrate(actions: &Rc<Actions>, rev: &str, current: &str, rebase: bool) {
    integrate_choosing(
        actions,
        vec![(rev.to_owned(), rev.to_owned())],
        current,
        rebase,
    );
}

/// Merge・Rebase の前に確かめる。`choices`（名前, rev）がいくつもあれば、同じシートで相手を選ぶ（最初のもの
/// から）。コンフリクトの見込み（rebase なら書き換えの見通し: 一緒に動くブランチ、push 済み、コミットごとの
/// コンフリクト）を裏で調べて見せ、rebase なら積み重ねたブランチを動かすかをこの操作だけ選べる。
pub fn integrate_choosing(
    actions: &Rc<Actions>,
    choices: Vec<(String, String)>,
    current: &str,
    rebase: bool,
) {
    let (actions, current) = (actions.clone(), current.to_owned());
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        if choices.is_empty() {
            return;
        }
        let title_for = {
            let current = current.clone();
            move |rev: &str| {
                if rebase {
                    tr!("Rebase {target} onto {rev}?", target = current, rev = rev)
                } else {
                    tr!("Merge {rev} into {target}?", rev = rev, target = current)
                }
            }
        };
        let sheet = Sheet::new(
            parent(&actions).as_ref(),
            &title_for(&choices[0].0),
            &can_undo(),
        );
        let labels: Vec<&str> = choices.iter().map(|(l, _)| l.as_str()).collect();
        let picker = gtk::DropDown::from_strings(&labels);
        picker.set_enable_search(true);
        if choices.len() > 1 {
            sheet.add(&row(
                &if rebase { tr!("Onto:") } else { tr!("Merge:") },
                &picker,
            ));
        }
        let forecast = forecast_label();
        sheet.add(&forecast);
        let update_refs = gtk::CheckButton::with_label(&tr!("Move stacked branches too"));
        update_refs.set_visible(false);
        if rebase {
            sheet.add(&update_refs);
        }
        sheet.buttons(if rebase { "Rebase" } else { "Merge" }, false, None);
        let chosen = {
            let (picker, choices) = (picker.clone(), choices.clone());
            move || choices[picker.selected() as usize].clone()
        };
        // 見込みを読む（相手や、積み重ねたブランチを動かすかを変えたら読み直す。古い答えは捨てる）。
        let generation = Rc::new(std::cell::Cell::new(0u32));
        let load = {
            let (repo, forecast, update_refs, chosen, generation) = (
                repo.clone(),
                forecast.clone(),
                update_refs.clone(),
                chosen.clone(),
                generation.clone(),
            );
            Rc::new(move |choice: Option<bool>| {
                let g = generation.get() + 1;
                generation.set(g);
                let (repo, forecast, update_refs, generation) = (
                    repo.clone(),
                    forecast.clone(),
                    update_refs.clone(),
                    generation.clone(),
                );
                let rev = chosen().1;
                forecast.set_text(&tr!("Checking for conflicts…"));
                forecast.remove_css_class("warning");
                glib::spawn_future_local(async move {
                    if !rebase {
                        let f = predict_merge(&repo, "HEAD", &[rev]).await;
                        if generation.get() == g {
                            show_forecast(&forecast, &f, false);
                        }
                        return;
                    }
                    let mut command = json!({ "op": "rewritePreview", "onto": rev });
                    if let Some(c) = choice {
                        command["updateRefs"] = json!(c);
                    }
                    let info = run(&repo, command).await["info"].clone();
                    if generation.get() != g {
                        return;
                    }
                    let (lines, conflicts) = rewrite_lines(&info);
                    forecast.set_text(&lines.join("\n"));
                    if conflicts {
                        forecast.add_css_class("warning");
                    }
                    // 積み重ねたブランチがあり、git が動かせるときだけ選べる（最初は git の設定のとおり）。
                    let stacked = info["moving"].as_array().is_some_and(|m| !m.is_empty())
                        || info["staying"].as_array().is_some_and(|s| {
                            s.iter()
                                .any(|b| matches!(b["reason"].as_str(), Some("setting" | "off")))
                        });
                    let available = info["updateRefs"]["available"].as_bool() == Some(true);
                    if choice.is_none() {
                        update_refs
                            .set_active(info["updateRefs"]["configured"].as_bool() == Some(true));
                    }
                    update_refs.set_visible(stacked && available);
                });
            })
        };
        load(None);
        let l = load.clone();
        update_refs.connect_toggled(move |b| l(Some(b.is_active())));
        let (l, heading, title_for, c) = (
            load.clone(),
            sheet.heading.clone(),
            title_for.clone(),
            chosen.clone(),
        );
        picker.connect_selected_notify(move |_| {
            heading.set_text(&title_for(&c().0));
            l(None);
        });
        if sheet.run().await != Response::Ok {
            return;
        }
        let rev = chosen().1;
        let command = if rebase {
            let mut c = json!({ "op": "rebase", "onto": rev });
            if update_refs.is_visible() {
                c["updateRefs"] = json!(update_refs.is_active());
            }
            c
        } else {
            json!({ "op": "merge", "rev": rev })
        };
        actions.perform(command);
    });
}

/// Commit and Push のあとの push: 上流があって先に進んでいなければ、聞かずに送る。上流がない・上流が先に
/// 進んでいるときは Push のシート（docs/ux.md の「Toolbar」）。
pub fn push_after_commit(actions: &Rc<Actions>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let Some(state) = sync_state(&repo, None).await else {
            return;
        };
        match (&state.branch, &state.upstream) {
            (Some(branch), Some(upstream)) if state.incoming.count == 0 => {
                let Some((remote, remote_branch)) = upstream.split_once('/') else {
                    return;
                };
                actions.perform(json!({
                    "op": "push", "remote": remote, "branch": branch, "remoteBranch": remote_branch,
                    "force": false, "setUpstream": false,
                }));
            }
            _ => push(&actions, None, false, None),
        }
    });
}

/// ブランチを消す前に確かめる。今のブランチにも上流にも取り込まれていないコミットがあれば、それを並べて見せ、
/// Delete Anyway（`-D`）で消す。どちらも Undo で戻せる。
pub fn delete_branches(actions: &Rc<Actions>, names: Vec<String>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let r = repo.clone();
        let refs = gio::spawn_blocking(move || r.load_refs().ok())
            .await
            .ok()
            .flatten();
        let mut unique: Vec<(String, CommitRange)> = Vec::new();
        for name in &names {
            // git と同じく、今のブランチか上流のどちらかに取り込まれていれば消せる。
            let item = refs.as_ref().and_then(|r| {
                r.refs
                    .entries
                    .iter()
                    .find(|i| i.kind == amonhen_model::RefKind::Local && &i.name == name)
            });
            let upstream = item.and_then(|i| if i.gone { None } else { i.detail.clone() });
            let from_head = between(&repo, "HEAD", name, 30).await;
            let only = match upstream {
                Some(u) if from_head.count > 0 => {
                    let from_upstream = between(&repo, &u, name, 30).await;
                    if from_upstream.count < from_head.count {
                        from_upstream
                    } else {
                        from_head
                    }
                }
                _ => from_head,
            };
            unique.push((name.clone(), only));
        }
        let unmerged = unique.iter().any(|(_, r)| r.count > 0);
        let title = if names.len() == 1 {
            tr!("Delete {name}?", name = names[0])
        } else {
            tr!("Delete {count} branches?", count = names.len())
        };
        let mut message = if names.len() == 1 {
            String::new()
        } else {
            format!("{}\n", names.join(", "))
        };
        if unmerged {
            message.push_str(&tr!(
                "Some commits are on no other branch; they go with the branch."
            ));
            message.push(' ');
        }
        message.push_str(&can_undo());
        let sheet = Sheet::new(parent(&actions).as_ref(), &title, &message);
        if unmerged {
            let lists: Vec<(String, &CommitRange, bool)> = unique
                .iter()
                .filter(|(_, r)| r.count > 0)
                .map(|(n, r)| (tr!("Commits only on {branch}", branch = n), r, true))
                .collect();
            sheet.add(&commit_lists(&lists));
        }
        let ok = if unmerged {
            tr!("Delete Anyway")
        } else {
            tr!("Delete")
        };
        sheet.buttons(&ok, false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        actions.perform_sequence(
            unique
                .into_iter()
                .map(
                    |(name, r)| json!({ "op": "deleteBranch", "name": name, "force": r.count > 0 }),
                )
                .collect(),
        );
    });
}

/// サイドバーにドラッグしたブランチを、リモートのブランチに送る前に確かめる。送り先が上流なら Push のシート。
/// `force` なら、そのリモートのブランチが最後の Fetch の時点のコミットのときだけ置き換える。
pub fn push_to(actions: &Rc<Actions>, branch: &str, target: &str, command: Value, force: bool) {
    let (actions, branch, target) = (actions.clone(), branch.to_owned(), target.to_owned());
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let r = repo.clone();
        let refs = gio::spawn_blocking(move || r.load_refs().ok())
            .await
            .ok()
            .flatten();
        let find = |kind: amonhen_model::RefKind, name: &str| {
            refs.as_ref().and_then(|r| {
                r.refs
                    .entries
                    .iter()
                    .find(|e| e.kind == kind && e.name == name)
                    .cloned()
            })
        };
        let local = find(amonhen_model::RefKind::Local, &branch);
        if local
            .as_ref()
            .is_some_and(|l| !l.gone && l.detail.as_deref() == Some(target.as_str()))
        {
            return push(&actions, Some(branch), force, None);
        }
        let there = find(amonhen_model::RefKind::Remote, &target).and_then(|e| e.target);
        let title = if force {
            tr!(
                "Force push {branch} to {target}?",
                branch = branch,
                target = target
            )
        } else {
            tr!(
                "Push {branch} to {target}?",
                branch = branch,
                target = target
            )
        };
        let message = if force {
            format!(
                "{} {}",
                tr!(
                    "{target} loses the commits {branch} doesn’t have.",
                    target = target,
                    branch = branch
                ),
                cant_undo()
            )
        } else if there.is_none() {
            tr!("Creates {target}.", target = target)
        } else {
            String::new()
        };
        let sheet = Sheet::new(parent(&actions).as_ref(), &title, &message);
        sheet.buttons(if force { "Force Push" } else { "Push" }, force, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let mut command = command;
        if force && let Some(t) = there {
            command["expect"] = json!(t.to_string());
        }
        actions.perform(command);
    });
}
