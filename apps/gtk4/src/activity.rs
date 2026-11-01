//! Activity の窓（macOS 版の `Activity.swift` と同じタブ）: Operations（実行した git とコアの命令）、History
//! （Undo できる操作と「ここまで戻す」）、Performance（画面の速さの記録）、Events（ファイルの変化の知らせ・
//! 読み直し）、Daemon（共有の daemon の様子と、起動・停止）。開いている間は 1 秒ごとに読み直す。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;

const KEEP: usize = 500;

/// 実行している操作の数の知らせ先（false を返したら外す）。
type RunningListener = Box<dyn Fn(u32) -> bool>;

thread_local! {
    /// 画面の速さの記録（項目ごとの直近の値、ms）。
    static PERFORMANCE: RefCell<HashMap<String, VecDeque<f64>>> = RefCell::default();
    /// 内部の出来事（時刻、リポジトリ、内容）。
    static EVENTS: RefCell<VecDeque<(i64, String, String)>> = RefCell::default();
    /// 開いている Activity の窓（元の窓ごとに 1 つ）。
    static OPEN: RefCell<Vec<(glib::WeakRef<gtk::ApplicationWindow>, Activity)>> = RefCell::default();
    /// 実行している操作の数（見出しの Activity に出す。macOS 版の `RunningOperations`）。
    static RUNNING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// 数が変わったら知らせる先（false を返したものは外す）。
    static ON_RUNNING: RefCell<Vec<RunningListener>> = RefCell::default();
}

/// 操作を始めた（終わったら [`end`]）。
pub fn begin() {
    set_running(RUNNING.with(|r| r.get()) + 1);
}

/// 操作が終わった。
pub fn end() {
    set_running(RUNNING.with(|r| r.get()).saturating_sub(1));
}

fn set_running(count: u32) {
    RUNNING.with(|r| r.set(count));
    // 知らせる間に足されてもよいように、いったん取り出す。
    let mut listeners = ON_RUNNING.with(|l| l.take());
    listeners.retain(|f| f(count));
    ON_RUNNING.with(|l| {
        let mut l = l.borrow_mut();
        listeners.append(&mut l);
        *l = listeners;
    });
}

/// 実行している操作の数が変わったら `f(数)` を呼ぶ（今の数でもすぐ呼ぶ）。`f` が false を返したら外す。
pub fn connect_running(f: impl Fn(u32) -> bool + 'static) {
    if f(RUNNING.with(|r| r.get())) {
        ON_RUNNING.with(|l| l.borrow_mut().push(Box::new(f)));
    }
}

/// 見出しの Activity のボタン: 操作を実行している間は回り、押すと Activity の窓（macOS 版のツールバーと同じ）。
pub fn toolbar_button() -> gtk::Button {
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    let icon = crate::icon::Icon::default();
    icon.set(crate::icon::Kind::Activity, false);
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    inner.append(&spinner);
    inner.append(&icon);
    let button = gtk::Button::new();
    button.set_child(Some(&inner));
    button.set_action_name(Some("win.activity"));
    let b = button.downgrade();
    connect_running(move |count| {
        let Some(button) = b.upgrade() else {
            return false;
        };
        spinner.set_visible(count > 0);
        spinner.set_spinning(count > 0);
        button.set_tooltip_text(Some(&if count > 0 {
            tr!(
                "{count} operations running (click for Activity)",
                count = count
            )
        } else {
            tr!("Git and commands run, the operation log, and UI speed (Ctrl+Alt+A)")
        }));
        true
    });
    button
}

/// 画面の速さを 1 つ記録する（メインスレッドから）。
pub fn record(metric: &str, ms: f64) {
    PERFORMANCE.with(|p| {
        let mut p = p.borrow_mut();
        let values = p.entry(metric.to_owned()).or_default();
        values.push_back(ms);
        if values.len() > KEEP {
            values.pop_front();
        }
    });
}

/// 内部の出来事を 1 つ記録する（メインスレッドから）。
pub fn event(repo: &str, text: impl Into<String>) {
    let now = glib::DateTime::now_local()
        .map_or(0, |d| d.to_unix() * 1000 + d.microsecond() as i64 / 1000);
    EVENTS.with(|e| {
        let mut e = e.borrow_mut();
        e.push_back((now, repo.to_owned(), text.into()));
        if e.len() > KEEP {
            e.pop_front();
        }
    });
}

fn time(ms: i64) -> String {
    glib::DateTime::from_unix_local(ms / 1000)
        .and_then(|d| d.format("%H:%M:%S"))
        .map(|s| format!("{s}.{:03}", ms % 1000))
        .unwrap_or_default()
}

fn ms(value: f64) -> String {
    if value < 10.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.0}")
    }
}

/// 並べた `len` 個のうち、割合 `p` の位置（macOS 版と同じく `len × p` を切り捨て、最後を超えない）。
fn quantile_index(len: usize, p: f64) -> usize {
    ((len as f64 * p) as usize).min(len.saturating_sub(1))
}

fn label(text: &str, css: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in css {
        l.add_css_class(c);
    }
    l
}

/// 成功・失敗・実行中の色（テーマの色を使う）。
fn install_css() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".activity-success { color: @success_color; } \
         .activity-error { color: @error_color; } \
         .activity-warning { color: @warning_color; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 前に出したときと同じ中身か（同じなら作り直さない。スクロールや説明がそのまま残る）。
fn unchanged(last: &std::cell::Cell<u64>, key: impl std::hash::Hash) -> bool {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    let hash = BuildHasherDefault::<DefaultHasher>::default().hash_one(key);
    last.replace(hash) == hash
}

/// 中身を作り直す一覧（行は少ないので ListBox で足りる）。
fn list() -> (gtk::ScrolledWindow, gtk::ListBox) {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("view");
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .build();
    (scrolled, list)
}

/// 行（列の文字を左から並べる。`widths` は文字数の幅、0 は残りすべて）。
fn row(cells: &[(&str, i32, &[&str])]) -> gtk::Box {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    line.set_margin_top(3);
    line.set_margin_bottom(3);
    line.set_margin_start(6);
    line.set_margin_end(6);
    for (text, width, css) in cells {
        let l = label(text, css);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        if *width > 0 {
            l.set_width_chars(*width);
            l.set_max_width_chars(*width);
        } else {
            l.set_hexpand(true);
        }
        line.append(&l);
    }
    line
}

/// Activity が見ているリポジトリ（元の窓の前のタブのもの。タブを替えたら付け替える）。
struct Target {
    /// 開いたリポジトリ（daemon 越しなら、その記録を読む）。
    repo: Option<Arc<Repository>>,
    /// History の Undo の先（書き込む先の worktree。Undo の記録もここから読む）。
    actions: Rc<Actions>,
}

#[derive(Clone)]
pub struct Activity {
    window: gtk::Window,
    stack: gtk::Stack,
    target: Rc<RefCell<Target>>,
    /// 付け替えたときに読み直す。
    retargeted: Rc<dyn Fn()>,
}

impl Activity {
    /// `parent` の Activity を開く（開いていれば前に出す）。
    pub fn show(
        parent: &gtk::ApplicationWindow,
        repo: Option<Arc<Repository>>,
        actions: Rc<Actions>,
        engine: std::path::PathBuf,
    ) {
        if let Some(a) = Self::find(parent) {
            a.retarget(repo, actions);
            a.window.present();
            return;
        }
        let activity = Self::new(parent, repo, actions, engine);
        OPEN.with(|o| {
            let mut o = o.borrow_mut();
            o.retain(|(p, a)| p.upgrade().is_some() && a.window.is_visible());
            o.push((parent.downgrade(), activity.clone()));
        });
        activity.window.present();
    }

    /// `parent` の Activity が開いていれば、`repo` のもの（前に出たタブ）に付け替える。
    pub fn follow(
        parent: &gtk::ApplicationWindow,
        repo: Option<Arc<Repository>>,
        actions: Rc<Actions>,
    ) {
        if let Some(a) = Self::find(parent) {
            a.retarget(repo, actions);
        }
    }

    fn retarget(&self, repo: Option<Arc<Repository>>, actions: Rc<Actions>) {
        {
            let t = self.target.borrow();
            let same_repo = match (&t.repo, &repo) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same_repo && Rc::ptr_eq(&t.actions, &actions) {
                return;
            }
        }
        self.target.replace(Target { repo, actions });
        (self.retargeted)();
    }

    /// 開いている Activity のタブ（`operations` など）を選ぶ。
    pub fn select(parent: &gtk::ApplicationWindow, tab: &str) {
        if let Some(a) = Self::find(parent) {
            a.stack.set_visible_child_name(tab);
        }
    }

    fn find(parent: &gtk::ApplicationWindow) -> Option<Self> {
        OPEN.with(|o| {
            o.borrow()
                .iter()
                .find(|(p, a)| p.upgrade().as_ref() == Some(parent) && a.window.is_visible())
                .map(|(_, a)| a.clone())
        })
    }

    fn new(
        parent: &gtk::ApplicationWindow,
        repo: Option<Arc<Repository>>,
        actions: Rc<Actions>,
        engine: std::path::PathBuf,
    ) -> Self {
        install_css();
        let stack = gtk::Stack::new();
        let switcher = gtk::StackSwitcher::builder().stack(&stack).build();
        let header = gtk::HeaderBar::new();
        header.set_title_widget(Some(&switcher));
        let window = gtk::Window::builder()
            .title("Activity")
            .default_width(980)
            .default_height(640)
            .child(&stack)
            .build();
        window.set_titlebar(Some(&header));
        window.set_transient_for(Some(parent));
        window.set_destroy_with_parent(true);

        let target = Rc::new(RefCell::new(Target { repo, actions }));
        let mut refreshers: Vec<Box<dyn Fn()>> = Vec::new();
        let margin = |w: &gtk::Box| {
            w.set_margin_top(10);
            w.set_margin_bottom(10);
            w.set_margin_start(12);
            w.set_margin_end(12);
        };

        // Operations: git の起動とコアの命令。
        let ops = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&ops);
        let kinds =
            gtk::DropDown::from_strings(&[tr!("All").as_str(), "git", tr!("Commands").as_str()]);
        let failures = gtk::CheckButton::with_label(&tr!("Failures Only"));
        let filter = gtk::SearchEntry::builder()
            .placeholder_text(tr!("Filter"))
            .hexpand(true)
            .build();
        let export = gtk::Button::with_label(&tr!("Export JSON…"));
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.append(&kinds);
        bar.append(&failures);
        bar.append(&filter);
        bar.append(&export);
        let (scrolled, op_list) = list();
        ops.append(&bar);
        ops.append(&scrolled);
        ops.append(&label(
            &tr!("git runs and core commands, newest first, up to 1,000 (for repositories through the daemon, also the git it starts). ● means running; fetch, pull, push and clone show their progress and can be canceled."),
            &["dim-label", "caption"],
        ));
        stack.add_titled(&ops, Some("operations"), &tr!("Operations"));
        // daemon の記録（裏で 2 秒ごとに読む。付け替えたら捨てる）。
        let remote_records: Rc<RefCell<Vec<Value>>> = Rc::default();
        let refresh_ops = {
            let (op_list, kinds, failures, filter, target, remote_records) = (
                op_list.clone(),
                kinds.clone(),
                failures.clone(),
                filter.clone(),
                target.clone(),
                remote_records.clone(),
            );
            let loading = Rc::new(std::cell::Cell::new(false));
            let tick = std::cell::Cell::new(0u32);
            let last = std::cell::Cell::new(0);
            move || {
                tick.set(tick.get() + 1);
                let repo = target.borrow().repo.clone();
                let remote = repo
                    .as_ref()
                    .is_some_and(|r| r.connection_state().is_some());
                if remote
                    && !loading.get()
                    && tick.get() % 2 == 1
                    && let Some(repo) = repo.clone()
                {
                    loading.set(true);
                    let (remote_records, loading, target) =
                        (remote_records.clone(), loading.clone(), target.clone());
                    glib::spawn_future_local(async move {
                        let asked = repo.clone();
                        let reply: Value = gio::spawn_blocking(move || {
                            serde_json::from_str(&asked.run_json(r#"{"op":"remoteActivity"}"#))
                                .unwrap_or_default()
                        })
                        .await
                        .unwrap_or_default();
                        loading.set(false);
                        // 読んでいる間に付け替えたら捨てる。
                        if !target
                            .borrow()
                            .repo
                            .as_ref()
                            .is_some_and(|r| Arc::ptr_eq(r, &repo))
                        {
                            return;
                        }
                        let mut records = reply["info"]["records"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        for r in &mut records {
                            r["remote"] = json!(true);
                        }
                        remote_records.replace(records);
                    });
                }
                let mut records: Vec<Value> = amonhen_model::activity::entries()
                    .into_iter()
                    .map(|e| {
                        json!({
                            "seq": e.seq, "kind": e.kind, "repo": e.repo, "label": e.label,
                            "started": e.started, "duration": e.duration, "ok": e.ok,
                            "error": e.error, "progress": e.progress, "cancellable": e.cancellable,
                        })
                    })
                    .collect();
                records.extend(remote_records.borrow().iter().cloned());
                records.sort_by_key(|r| std::cmp::Reverse(r["started"].as_u64().unwrap_or(0)));
                let kind = match kinds.selected() {
                    1 => Some("git"),
                    2 => Some("command"),
                    _ => None,
                };
                let text = filter.text().to_lowercase();
                let key = (
                    Value::Array(records.clone()).to_string(),
                    kind,
                    failures.is_active(),
                    text.clone(),
                );
                if unchanged(&last, key) {
                    return;
                }
                op_list.remove_all();
                for r in records.iter().take(1000) {
                    let ok = r["ok"].as_bool();
                    if kind.is_some_and(|k| r["kind"].as_str() != Some(k))
                        || (failures.is_active() && ok != Some(false))
                    {
                        continue;
                    }
                    let label_text = r["label"].as_str().unwrap_or_default();
                    let repo_path = r["repo"].as_str().unwrap_or_default();
                    if !text.is_empty()
                        && !label_text.to_lowercase().contains(&text)
                        && !repo_path.to_lowercase().contains(&text)
                    {
                        continue;
                    }
                    let mark = match ok {
                        None => ("●", "activity-warning"),
                        Some(true) => ("✓", "activity-success"),
                        Some(false) => ("✗", "activity-error"),
                    };
                    let name = repo_path.rsplit('/').next().unwrap_or_default();
                    let name = if r["remote"].as_bool() == Some(true) {
                        tr!("{name} (daemon)", name = name)
                    } else {
                        name.to_owned()
                    };
                    let mut content = label_text.to_owned();
                    if ok.is_none()
                        && let Some(p) = r["progress"].as_str()
                    {
                        content.push_str(&format!("  {p}"));
                    }
                    let duration = r["duration"].as_f64().map_or("…".to_owned(), ms);
                    let line = row(&[
                        (
                            &time(r["started"].as_i64().unwrap_or(0)),
                            12,
                            &["numeric", "dim-label"],
                        ),
                        (mark.0, 2, &[mark.1]),
                        (&name, 14, &[]),
                        (&content, 0, &[]),
                        (&duration, 6, &["numeric"]),
                    ]);
                    let mut tip = format!("{repo_path}\n{label_text}");
                    if let Some(e) = r["error"].as_str() {
                        tip.push_str(&format!("\n\n{e}"));
                    }
                    line.set_tooltip_text(Some(&tip));
                    // 走っていて取り消せる操作には「取り消す」。
                    if ok.is_none() && r["cancellable"].as_bool() == Some(true) {
                        let cancel = gtk::Button::with_label(&tr!("Cancel Operation"));
                        cancel.add_css_class("flat");
                        let seq = r["seq"].as_u64().unwrap_or(0);
                        let is_remote = r["remote"].as_bool() == Some(true);
                        let repo = repo.clone();
                        cancel.connect_clicked(move |_| {
                            if is_remote {
                                // daemon への往復なので、裏の糸で（macOS 版と同じ）。
                                if let Some(repo) = repo.clone() {
                                    let command = json!({ "op": "cancelActivity", "seq": seq, "remote": true });
                                    gio::spawn_blocking(move || {
                                        let _ = repo.run_json(&command.to_string());
                                    });
                                }
                            } else {
                                amonhen_model::activity::cancel(seq);
                            }
                        });
                        line.append(&cancel);
                    }
                    op_list.append(&line);
                }
            }
        };
        // 絞り込みを変えたら、すぐに出し直す。
        let refresh_ops = Rc::new(refresh_ops);
        let r = refresh_ops.clone();
        kinds.connect_selected_notify(move |_| r());
        let r = refresh_ops.clone();
        failures.connect_toggled(move |_| r());
        let r = refresh_ops.clone();
        filter.connect_search_changed(move |_| r());
        refreshers.push(Box::new(move || refresh_ops()));
        let w = window.downgrade();
        export.connect_clicked(move |_| {
            let Some(window) = w.upgrade() else { return };
            let dialog = gtk::FileDialog::builder()
                .title(tr!("Export JSON"))
                .initial_name("amonhen-activity.json")
                .modal(true)
                .build();
            glib::spawn_future_local(async move {
                let Ok(file) = dialog.save_future(Some(&window)).await else {
                    return;
                };
                let records: Vec<Value> = amonhen_model::activity::entries()
                    .into_iter()
                    .map(|e| {
                        json!({
                            "seq": e.seq, "kind": e.kind, "repo": e.repo, "label": e.label,
                            "started": e.started, "duration": e.duration, "ok": e.ok, "error": e.error,
                        })
                    })
                    .collect();
                if let Some(path) = file.path() {
                    let text = serde_json::to_string_pretty(&records).unwrap_or_default();
                    let _ = std::fs::write(path, text);
                }
            });
        });

        // History: Undo できる操作（新しい順）と「ここまで戻す」。
        let history = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&history);
        let (scrolled, undo_list) = list();
        history.append(&scrolled);
        history.append(&label(
            &tr!("Operations you can Undo (newest first). It stops where another tool has moved a ref."),
            &["dim-label", "caption"],
        ));
        stack.add_titled(&history, Some("history"), "History");
        let refresh_undo: Rc<dyn Fn()> = {
            let (undo_list, target, w) = (undo_list.clone(), target.clone(), window.downgrade());
            Rc::new(move || {
                // Undo する先（書き込む先の worktree）の記録を出す（一覧と Undo の先を取り違えない）。
                let actions = target.borrow().actions.clone();
                let Some(repo) = actions.repo().or_else(|| target.borrow().repo.clone()) else {
                    undo_list.remove_all();
                    return;
                };
                let (undo_list, target, w) = (undo_list.clone(), target.clone(), w.clone());
                glib::spawn_future_local(async move {
                    let reply: Value = gio::spawn_blocking(move || {
                        serde_json::from_str(&repo.run_json(r#"{"op":"undoLog"}"#))
                            .unwrap_or_default()
                    })
                    .await
                    .unwrap_or_default();
                    // 読んでいる間に付け替えたら捨てる（付け替えたほうで読み直す）。
                    if !Rc::ptr_eq(&target.borrow().actions, &actions) {
                        return;
                    }
                    undo_list.remove_all();
                    let entries = reply["info"]["entries"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    for (i, e) in entries.iter().enumerate() {
                        let line = row(&[
                            (
                                &time(e["time"].as_f64().unwrap_or(0.0) as i64),
                                12,
                                &["numeric", "dim-label"],
                            ),
                            (e["label"].as_str().unwrap_or_default(), 0, &[]),
                        ]);
                        let button = gtk::Button::with_label(&if i == 0 {
                            "Undo".to_owned()
                        } else {
                            tr!("Undo to Here")
                        });
                        button.add_css_class("flat");
                        button.set_tooltip_text(Some(&if i == 0 {
                            tr!("Undo this operation.")
                        } else {
                            tr!(
                                "Undo the {count} latest operations, down to and including this one.",
                                count = i + 1
                            )
                        }));
                        let (a, w) = (actions.clone(), w.clone());
                        let title = if i == 0 {
                            tr!(
                                "Undo “{label}”?",
                                label = e["label"].as_str().unwrap_or_default()
                            )
                        } else {
                            tr!("Undo the last {count} operations?", count = i + 1)
                        };
                        // 戻す前に確かめる（macOS 版と同じ）。
                        button.connect_clicked(move |_| {
                            let (a, w, title) = (a.clone(), w.clone(), title.clone());
                            glib::spawn_future_local(async move {
                                let sheet = crate::sheets::Sheet::new(
                                    w.upgrade().as_ref(),
                                    &title,
                                    &tr!("Returns HEAD and the branches to where they were."),
                                );
                                sheet.buttons("Undo", false, None);
                                if sheet.run().await == crate::sheets::Response::Ok {
                                    a.perform_sequence(vec![json!({ "op": "undo" }); i + 1]);
                                }
                            });
                        });
                        line.append(&button);
                        undo_list.append(&line);
                    }
                });
            })
        };
        refresh_undo();

        // Performance: 画面の速さの記録。
        let perf = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&perf);
        let (scrolled, perf_list) = list();
        perf.append(&scrolled);
        perf.append(&label(
            &tr!("The last 500 of each since the app started. Orange when p95 exceeds twice a frame (16.7 ms)."),
            &["dim-label", "caption"],
        ));
        stack.add_titled(&perf, Some("performance"), &tr!("Performance"));
        let refresh_perf = {
            let perf_list = perf_list.clone();
            let last = std::cell::Cell::new(0);
            move || {
                let metrics: Vec<(String, Vec<f64>)> = PERFORMANCE.with(|p| {
                    p.borrow()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.iter().copied().collect()))
                        .collect()
                });
                if unchanged(&last, format!("{metrics:?}")) {
                    return;
                }
                let mut metrics = metrics;
                perf_list.remove_all();
                perf_list.append(&row(&[
                    (&tr!("Item"), 0, &["dim-label"]),
                    (&tr!("Count"), 6, &["dim-label"]),
                    ("p50 ms", 7, &["dim-label"]),
                    ("p95 ms", 7, &["dim-label"]),
                    (&tr!("Max ms"), 7, &["dim-label"]),
                    (&tr!("Last ms"), 7, &["dim-label"]),
                ]));
                metrics.sort_by(|a, b| a.0.cmp(&b.0));
                for (name, values) in metrics {
                    let last = *values.last().unwrap_or(&0.0);
                    let mut sorted = values.clone();
                    sorted.sort_by(f64::total_cmp);
                    let at = |p: f64| sorted[quantile_index(sorted.len(), p)];
                    let p95 = at(0.95);
                    let slow: &[&str] = if p95 > 16.7 {
                        &["activity-warning", "numeric"]
                    } else {
                        &["numeric"]
                    };
                    perf_list.append(&row(&[
                        (&name, 0, &[]),
                        (&values.len().to_string(), 6, &["numeric"]),
                        (&ms(at(0.5)), 7, &["numeric"]),
                        (&ms(p95), 7, slow),
                        (&ms(at(1.0)), 7, &["numeric"]),
                        (&ms(last), 7, &["numeric"]),
                    ]));
                }
            }
        };
        refreshers.push(Box::new(refresh_perf));

        // Events: 内部の出来事。
        let events = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&events);
        let (scrolled, event_list) = list();
        events.append(&scrolled);
        events.append(&label(
            &tr!("File change notices and the reloads they cause (newest first, up to 500)."),
            &["dim-label", "caption"],
        ));
        stack.add_titled(&events, Some("events"), &tr!("Events"));
        let refresh_events = {
            let event_list = event_list.clone();
            let last = std::cell::Cell::new(0);
            move || {
                let items: Vec<(i64, String, String)> =
                    EVENTS.with(|e| e.borrow().iter().rev().cloned().collect());
                if unchanged(&last, &items) {
                    return;
                }
                event_list.remove_all();
                for (t, repo, text) in items {
                    event_list.append(&row(&[
                        (&time(t), 12, &["numeric", "dim-label"]),
                        (&repo, 14, &[]),
                        (&text, 0, &[]),
                    ]));
                }
            }
        };
        refreshers.push(Box::new(refresh_events));

        // Daemon: 共有の daemon の様子と、起動・停止。
        let daemon = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&daemon);
        let state = label(&tr!("Checking…"), &["heading"]);
        let detail = label("", &["caption", "numeric"]);
        detail.set_selectable(true);
        let toggle = gtk::Button::with_label(&tr!("Start"));
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_hexpand(true);
        texts.append(&state);
        texts.append(&detail);
        top.append(&texts);
        top.append(&toggle);
        let (scrolled, repo_list) = list();
        daemon.append(&top);
        daemon.append(&label(&tr!("Open Repositories"), &["heading"]));
        daemon.append(&scrolled);
        stack.add_titled(&daemon, Some("daemon"), "Daemon");
        let running = Rc::new(std::cell::Cell::new(false));
        let refresh_daemon = {
            let (state, detail, toggle, repo_list, running) = (
                state.clone(),
                detail.clone(),
                toggle.clone(),
                repo_list.clone(),
                running.clone(),
            );
            // 2 秒ごとに、前の問い合わせが返ってから（macOS 版と同じ。遅いソケットで積み重ねない）。
            let loading = Rc::new(std::cell::Cell::new(false));
            let tick = std::cell::Cell::new(0u32);
            let last = Rc::new(std::cell::Cell::new(0));
            move || {
                tick.set(tick.get() + 1);
                if loading.get() || tick.get().is_multiple_of(2) {
                    return;
                }
                loading.set(true);
                let (state, detail, toggle, repo_list, running, loading, last) = (
                    state.clone(),
                    detail.clone(),
                    toggle.clone(),
                    repo_list.clone(),
                    running.clone(),
                    loading.clone(),
                    last.clone(),
                );
                glib::spawn_future_local(async move {
                    let status = gio::spawn_blocking(|| {
                        amonhen_remote::local::control("status")
                            .ok()
                            .flatten()
                            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    })
                    .await
                    .ok()
                    .flatten();
                    loading.set(false);
                    running.set(status.is_some());
                    // 一覧は、中身が変わったときだけ作り直す（ちらつかせず、スクロールの位置も保つ）。
                    let repos = status.as_ref().map(|d| d["repos"].to_string());
                    let same_repos = unchanged(&last, &repos);
                    if !same_repos {
                        repo_list.remove_all();
                    }
                    let Some(d) = status else {
                        state.set_text(&tr!("The shared daemon isn’t running"));
                        detail.set_text(&tr!("It starts automatically when you open a repository through the daemon (right-click in the repository list, or Settings > Performance)."));
                        toggle.set_label(&tr!("Start"));
                        return;
                    };
                    state.set_text(&tr!("The shared daemon is running"));
                    let uptime = d["uptimeMs"].as_u64().unwrap_or(0) / 1000;
                    detail.set_text(&tr!(
                        "pid {pid} · version {version} (protocol {protocol}) · up {minutes} min {seconds} s · connections {connections}\n{socket}",
                        pid = d["pid"],
                        version = d["version"].as_str().unwrap_or_default(),
                        protocol = d["protocol"],
                        minutes = uptime / 60,
                        seconds = uptime % 60,
                        connections = d["connections"],
                        socket = d["socket"].as_str().unwrap_or_default(),
                    ));
                    toggle.set_label(&tr!("Stop"));
                    if same_repos {
                        return;
                    }
                    for r in d["repos"].as_array().into_iter().flatten() {
                        let yes = |key: &str| {
                            if r[key].as_bool() == Some(true) {
                                tr!("Yes")
                            } else {
                                "—".to_owned()
                            }
                        };
                        let clients = r["clients"].as_u64().unwrap_or(0);
                        let clients = if clients > 0 {
                            tr!("{clients} connections", clients = clients)
                        } else {
                            tr!(
                                "No connections (for {seconds} s)",
                                seconds = r["idleMs"].as_u64().unwrap_or(0) / 1000
                            )
                        };
                        repo_list.append(&row(&[
                            (r["path"].as_str().unwrap_or_default(), 0, &[]),
                            (&clients, 18, &["numeric"]),
                            (&format!("History {}", yes("history")), 10, &[]),
                            (&tr!("Watching {state}", state = yes("watching")), 8, &[]),
                        ]));
                    }
                });
            }
        };
        refreshers.push(Box::new(refresh_daemon));
        toggle.connect_clicked(move |_| {
            let (running, engine) = (running.get(), engine.clone());
            glib::spawn_future_local(async move {
                let _ = gio::spawn_blocking(move || {
                    if running {
                        let _ = amonhen_remote::local::control("stop");
                    } else {
                        let _ = amonhen_remote::local::start(&engine.to_string_lossy());
                    }
                })
                .await;
            });
        });

        // 開いている間は 1 秒ごとに読み直す（History のタブは、見せたときと操作の後に）。
        let refresh_all = {
            let refreshers = refreshers;
            move || {
                for f in refreshers.iter() {
                    f();
                }
            }
        };
        refresh_all();
        let w = window.downgrade();
        glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            if w.upgrade().is_none_or(|w| !w.is_visible()) {
                return glib::ControlFlow::Break;
            }
            refresh_all();
            glib::ControlFlow::Continue
        });
        let r = refresh_undo.clone();
        stack.connect_visible_child_name_notify(move |s| {
            if s.visible_child_name().as_deref() == Some("history") {
                r();
            }
        });
        // 操作の後と、同じタブで書き込む先の worktree を替えたら読み直す（付け替えた先の操作でも。同じものには
        // 1 度だけつなぐ）。
        let connected: Rc<RefCell<Vec<std::rc::Weak<Actions>>>> = Rc::default();
        let connect_after = {
            let r = refresh_undo.clone();
            move |actions: &Rc<Actions>| {
                let mut c = connected.borrow_mut();
                c.retain(|w| w.strong_count() > 0);
                if c.iter().any(|w| w.as_ptr() == Rc::as_ptr(actions)) {
                    return;
                }
                c.push(Rc::downgrade(actions));
                let r1 = r.clone();
                actions.connect_after(move || r1());
                let r = r.clone();
                actions.connect_repo_changed(move || r());
            }
        };
        let first = target.borrow().actions.clone();
        connect_after(&first);
        let retargeted: Rc<dyn Fn()> = {
            let (target, r) = (target.clone(), refresh_undo);
            Rc::new(move || {
                remote_records.borrow_mut().clear();
                let actions = target.borrow().actions.clone();
                connect_after(&actions);
                r();
            })
        };
        Self {
            window,
            stack,
            target,
            retargeted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// p50・p95 の位置は macOS 版と同じ（`len × p` を切り捨て）。
    #[test]
    fn quantiles_match_macos() {
        assert_eq!(quantile_index(1, 0.95), 0);
        assert_eq!(quantile_index(2, 0.5), 1);
        assert_eq!(quantile_index(10, 0.5), 5);
        assert_eq!(quantile_index(10, 0.95), 9);
        assert_eq!(quantile_index(100, 0.95), 95);
        assert_eq!(quantile_index(10, 1.0), 9);
    }
}
