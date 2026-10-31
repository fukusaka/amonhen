//! Blame と File History の窓（macOS 版の `FileTools.swift` と同じ）。
//!
//! - Blame: ファイルの各行を最後に変えたコミット。同じコミットの行をまとめ、まとまりの 1 行目に ID・作者・日時、
//!   2 行目に件名を出し、まとまりごとに背景を交互に変える。行をダブルクリックすると、そのコミットを History で選ぶ。
//! - File History: 左にファイルを変えたコミット、右にそのコミットでのファイルの差分。名前の変更も追う。
//!
//! どちらも行は `GtkListView` で、見えている行の部品しか作らない（大きなファイルでも重くならない）。

use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::{Oid, tr};
use amonhen_projection::FrontendProfile;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::diff::{DiffWidget, View};

/// History でそのコミットを選ぶ。
pub type OnCommit = Rc<dyn Fn(Oid)>;

fn run(repo: &Repository, command: Value) -> Result<Value, String> {
    let reply: Value =
        serde_json::from_str(&repo.run_json(&command.to_string())).map_err(|e| e.to_string())?;
    match reply["error"].as_str() {
        Some(e) => Err(e.to_owned()),
        None => Ok(reply["info"].clone()),
    }
}

fn date(time: i64) -> String {
    glib::DateTime::from_unix_local(time)
        .and_then(|d| d.format("%Y-%m-%d %H:%M"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn new_window(parent: &gtk::ApplicationWindow, title: &str, child: &impl IsA<gtk::Widget>) {
    let window = gtk::Window::builder()
        .title(title)
        .default_width(1100)
        .default_height(760)
        .child(child)
        .build();
    window.set_transient_for(Some(parent));
    window.set_destroy_with_parent(true);
    window.present();
}

/// 失敗を知らせる。
fn fail(parent: &gtk::ApplicationWindow, e: &str) {
    gtk::AlertDialog::builder()
        .message(tr!("Can’t read the file’s information"))
        .detail(e)
        .modal(true)
        .build()
        .show(Some(parent));
}

/// `path` の blame を、`rev`（なければ作業ツリー）で開く。
pub fn blame(
    parent: &gtk::ApplicationWindow,
    repo: Arc<Repository>,
    path: &str,
    rev: Option<&str>,
    on_commit: OnCommit,
) {
    let mut title = format!("Blame — {path}");
    if let Some(rev) = rev {
        title.push_str(&format!(" @ {}", &rev[..8.min(rev.len())]));
    }
    let command = json!({ "op": "blame", "path": path, "rev": rev });
    let parent = parent.clone();
    glib::spawn_future_local(async move {
        let info = gio::spawn_blocking(move || run(&repo, command)).await;
        match info {
            Ok(Ok(info)) => show_blame(&parent, &title, &info, on_commit),
            Ok(Err(e)) => fail(&parent, &e),
            Err(_) => {}
        }
    });
}

/// 1 行分の中身。
struct BlameRow {
    /// まとまりの何行目か（0 が 1 行目）と、まとまりの番号。
    offset: usize,
    block: usize,
    commit: usize,
    text: String,
}

fn show_blame(parent: &gtk::ApplicationWindow, title: &str, info: &Value, on_commit: OnCommit) {
    let commits: Vec<(String, String, i64, String)> = info["commits"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            (
                c["oid"].as_str().unwrap_or_default().to_owned(),
                c["author"].as_str().unwrap_or_default().to_owned(),
                c["time"].as_i64().unwrap_or_default(),
                c["summary"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let mut rows = Vec::new();
    let (mut block, mut offset, mut previous) = (0usize, 0usize, usize::MAX);
    for (i, line) in info["lines"].as_array().into_iter().flatten().enumerate() {
        let commit = line["c"].as_u64().unwrap_or_default() as usize;
        if commit != previous {
            if i > 0 {
                block += 1;
            }
            offset = 0;
            previous = commit;
        } else {
            offset += 1;
        }
        rows.push(BlameRow {
            offset,
            block,
            commit,
            text: line["t"].as_str().unwrap_or_default().to_owned(),
        });
    }
    let rows = Rc::new(rows);
    let commits = Rc::new(commits);
    let digits = rows.len().to_string().len();

    let model = gtk::StringList::new(&[]);
    for i in 0..rows.len() {
        model.append(&i.to_string());
    }
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let info = gtk::Label::new(None);
        info.set_xalign(0.0);
        info.set_width_chars(36);
        info.set_max_width_chars(36);
        info.set_ellipsize(gtk::pango::EllipsizeMode::End);
        info.add_css_class("caption");
        let number = gtk::Label::new(None);
        number.set_xalign(1.0);
        number.add_css_class("monospace");
        number.add_css_class("dim-label");
        let code = gtk::Label::new(None);
        code.set_xalign(0.0);
        code.set_hexpand(true);
        code.add_css_class("monospace");
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        line.append(&info);
        line.append(&number);
        line.append(&code);
        item.set_child(Some(&line));
    });
    let (r, c) = (rows.clone(), commits.clone());
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let index = item.position() as usize;
        let (Some(row), Some(line)) = (r.get(index), item.child().and_downcast::<gtk::Box>())
        else {
            return;
        };
        let info = line
            .first_child()
            .and_downcast::<gtk::Label>()
            .expect("info");
        let number = info
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("number");
        let code = number
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("code");
        let (oid, author, time, summary) = &c[row.commit];
        let committed = oid.bytes().any(|b| b != b'0');
        // まとまりの 1 行目に ID・作者・日時、2 行目に件名。
        let text = match (committed, row.offset) {
            (false, 0) => tr!("Not committed yet"),
            (true, 0) => format!("{}  {author}  {}", &oid[..8], date(*time)),
            (true, 1) => summary.clone(),
            _ => String::new(),
        };
        info.set_text(&text);
        info.set_opacity(if row.offset == 0 { 0.75 } else { 0.5 });
        line.set_tooltip_text(
            committed
                .then(|| {
                    format!(
                        "{summary}\n{oid}\n{}",
                        tr!("Double-click to show in History.")
                    )
                })
                .as_deref(),
        );
        number.set_text(&format!("{:>digits$}", index + 1));
        code.set_text(&row.text);
        // まとまりごとに背景を交互に変える。
        if let Some(cell) = line.parent() {
            if row.block % 2 == 1 {
                cell.add_css_class("blame-shaded");
            } else {
                cell.remove_css_class("blame-shaded");
            }
        }
    });
    let selection = gtk::SingleSelection::new(Some(model));
    // 開いたときは何も選ばない。
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    selection.set_selected(gtk::INVALID_LIST_POSITION);
    let list = gtk::ListView::new(Some(selection), Some(factory));
    list.add_css_class("view");
    let (r, c) = (rows.clone(), commits.clone());
    list.connect_activate(move |_, position| {
        let Some(row) = r.get(position as usize) else {
            return;
        };
        if let Some(id) = Oid::from_hex(&c[row.commit].0) {
            on_commit(id);
        }
    });
    install_css();
    let scrolled = gtk::ScrolledWindow::builder().child(&list).build();
    new_window(parent, title, &scrolled);
}

/// Blame の交互の背景。
fn install_css() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(".blame-shaded { background-color: alpha(currentColor, 0.05); }");
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// `path` を変えたコミットと、それぞれでの差分の窓を開く。
pub fn file_history(
    parent: &gtk::ApplicationWindow,
    repo: Arc<Repository>,
    path: &str,
    on_commit: OnCommit,
) {
    let command = json!({ "op": "fileHistory", "path": path });
    let (parent, path) = (parent.clone(), path.to_owned());
    let r = repo.clone();
    glib::spawn_future_local(async move {
        let info = gio::spawn_blocking(move || run(&r, command)).await;
        match info {
            Ok(Ok(info)) => show_file_history(&parent, repo, &path, &info, on_commit),
            Ok(Err(e)) => fail(&parent, &e),
            Err(_) => {}
        }
    });
}

/// File History の 1 行（コミット、件名、作者、日時、その時点のパス）。
struct Entry {
    oid: String,
    subject: String,
    author: String,
    time: i64,
    path: String,
}

fn show_file_history(
    parent: &gtk::ApplicationWindow,
    repo: Arc<Repository>,
    path: &str,
    info: &Value,
    on_commit: OnCommit,
) {
    let entries: Vec<Entry> = info["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Entry {
            oid: e["oid"].as_str().unwrap_or_default().to_owned(),
            subject: e["subject"].as_str().unwrap_or_default().to_owned(),
            author: e["author"].as_str().unwrap_or_default().to_owned(),
            time: e["time"].as_i64().unwrap_or_default(),
            path: e["path"].as_str().unwrap_or_default().to_owned(),
        })
        .collect();
    let entries = Rc::new(entries);
    let title = tr!(
        "History — {path} ({count} commits)",
        path = path,
        count = entries.len()
    );

    let model = gtk::StringList::new(&[]);
    for i in 0..entries.len() {
        model.append(&i.to_string());
    }
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let subject = gtk::Label::new(None);
        subject.set_xalign(0.0);
        subject.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let meta = gtk::Label::new(None);
        meta.set_xalign(0.0);
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        meta.add_css_class("dim-label");
        meta.add_css_class("caption");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
        column.set_margin_top(4);
        column.set_margin_bottom(4);
        column.append(&subject);
        column.append(&meta);
        item.set_child(Some(&column));
    });
    let e = entries.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let (Some(entry), Some(column)) = (
            e.get(item.position() as usize),
            item.child().and_downcast::<gtk::Box>(),
        ) else {
            return;
        };
        let subject = column
            .first_child()
            .and_downcast::<gtk::Label>()
            .expect("subject");
        let meta = subject
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("meta");
        subject.set_text(&entry.subject);
        meta.set_text(&format!(
            "{}  ·  {}  ·  {}",
            entry.author,
            date(entry.time),
            &entry.oid[..8.min(entry.oid.len())]
        ));
    });
    let selection = gtk::SingleSelection::new(Some(model));
    let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
    list.add_css_class("navigation-sidebar");
    let e = entries.clone();
    let open = on_commit.clone();
    list.connect_activate(move |_, position| {
        if let Some(id) = e
            .get(position as usize)
            .and_then(|entry| Oid::from_hex(&entry.oid))
        {
            open(id);
        }
    });
    let left = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let diff = DiffWidget::default();
    let right = gtk::ScrolledWindow::builder()
        .child(&diff)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let split = gtk::Paned::new(gtk::Orientation::Horizontal);
    split.set_start_child(Some(&left));
    split.set_end_child(Some(&right));
    split.set_resize_start_child(false);
    split.set_position(360);

    // 選んだコミットでの、このファイルの差分。
    let generation = Rc::new(std::cell::Cell::new(0u64));
    let (e, d) = (entries.clone(), diff.downgrade());
    let show = move |position: u32| {
        let Some(entry) = e.get(position as usize) else {
            return;
        };
        let Some(id) = Oid::from_hex(&entry.oid) else {
            return;
        };
        let g = generation.get() + 1;
        generation.set(g);
        let (repo, path, d, generation) = (
            repo.clone(),
            entry.path.clone(),
            d.clone(),
            generation.clone(),
        );
        glib::spawn_future_local(async move {
            let view = gio::spawn_blocking(move || {
                let changes = repo.commit_changes(&id).map_err(|e| e.to_string())?;
                let change = changes
                    .iter()
                    .find(|c| c.path == path || c.old_path.as_deref() == Some(path.as_str()))
                    .ok_or_else(|| tr!("This commit has no changes to the file"))?;
                let view = repo
                    .file_diff(change, &FrontendProfile::GTK)
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>(View::new(view))
            })
            .await;
            let Some(diff) = d.upgrade() else { return };
            if generation.get() != g {
                return;
            }
            match view {
                Ok(Ok(view)) => diff.show_plain(view),
                Ok(Err(e)) => eprintln!("差分を読めない: {e}"),
                Err(_) => {}
            }
        });
    };
    if !entries.is_empty() {
        show(0);
    }
    selection.connect_selected_notify(move |s| show(s.selected()));
    new_window(parent, &title, &split);
}
