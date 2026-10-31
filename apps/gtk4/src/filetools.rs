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
    let path = path.to_owned();
    glib::spawn_future_local(async move {
        // 構文の色も、読むのと同じく裏で。
        let info = gio::spawn_blocking(move || {
            run(&repo, command).map(|info| {
                let spans = blame_syntax(&path, &info);
                (info, spans)
            })
        })
        .await;
        match info {
            Ok(Ok((info, spans))) => show_blame(&parent, &title, &info, spans, on_commit),
            Ok(Err(e)) => fail(&parent, &e),
            Err(_) => {}
        }
    });
}

/// Blame の各行の構文の色（行の中のバイト位置と種類。macOS 版と同じく、ファイル全体で色を付けて行に分ける）。
type LineSpans = Vec<Vec<(u32, u32, u8)>>;

fn blame_syntax(path: &str, info: &Value) -> LineSpans {
    let lines: Vec<&str> = info["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| l["t"].as_str().unwrap_or_default())
        .collect();
    let spans = amonhen_core::diff::text_syntax(path, lines.join("\n").as_bytes());
    line_spans(&lines, &spans)
}

/// テキスト全体の UTF-16 の位置の色を、行ごとのバイト位置に分ける（複数行にまたがるものは行ごとに切る）。
fn line_spans(lines: &[&str], spans: &[amonhen_layout::SyntaxSpan]) -> LineSpans {
    let mut out = vec![Vec::new(); lines.len()];
    if spans.is_empty() {
        return out;
    }
    let lens: Vec<u32> = lines
        .iter()
        .map(|l| l.encode_utf16().count() as u32)
        .collect();
    let mut starts = Vec::with_capacity(lines.len());
    let mut unit = 0u32;
    for len in &lens {
        starts.push(unit);
        unit += len + 1;
    }
    let byte_at = |line: &str, unit: u32| {
        let mut u = 0u32;
        for (b, c) in line.char_indices() {
            if u >= unit {
                return b as u32;
            }
            u += c.len_utf16() as u32;
        }
        line.len() as u32
    };
    for span in spans {
        let mut i = starts
            .partition_point(|&s| s <= span.start)
            .saturating_sub(1);
        while i < lines.len() && starts[i] < span.end {
            let a = span.start.saturating_sub(starts[i]).min(lens[i]);
            let b = (span.end - starts[i]).min(lens[i]);
            if a < b {
                out[i].push((byte_at(lines[i], a), byte_at(lines[i], b), span.kind));
            }
            i += 1;
        }
    }
    out
}

/// 1 行分の中身。
struct BlameRow {
    /// まとまりの何行目か（0 が 1 行目）と、まとまりの番号。
    offset: usize,
    block: usize,
    commit: usize,
    text: String,
}

fn show_blame(
    parent: &gtk::ApplicationWindow,
    title: &str,
    info: &Value,
    spans: LineSpans,
    on_commit: OnCommit,
) {
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
    let spans = Rc::new(spans);
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
    let (r, c, s) = (rows.clone(), commits.clone(), spans.clone());
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
        let attrs = gtk::pango::AttrList::new();
        let fg = code.color();
        for &(start, end, kind) in s.get(index).into_iter().flatten() {
            if let Some((r, g, b)) = crate::diff::syntax_color(kind, &fg) {
                let mut attr = gtk::pango::AttrColor::new_foreground(
                    (r * 65535.0) as u16,
                    (g * 65535.0) as u16,
                    (b * 65535.0) as u16,
                );
                attr.set_start_index(start);
                attr.set_end_index(end);
                attrs.insert(attr);
            }
        }
        code.set_attributes(Some(&attrs));
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

/// `path` を変えたコミットと、それぞれでの差分の窓を開く。merge のコミットを出すかは Settings の
/// `fileHistoryMerges`（右クリックの Show Merge Commits で切り替え、開いている窓も読み直す）。
pub fn file_history(
    parent: &gtk::ApplicationWindow,
    repo: Arc<Repository>,
    path: &str,
    on_commit: OnCommit,
) {
    let window = gtk::Window::builder()
        .title(format!("File History — {path}"))
        .default_width(1100)
        .default_height(760)
        .build();
    window.set_transient_for(Some(parent));
    window.set_destroy_with_parent(true);
    window.present();
    load_file_history(&window, parent, repo, path.to_owned(), on_commit);
}

fn load_file_history(
    window: &gtk::Window,
    parent: &gtk::ApplicationWindow,
    repo: Arc<Repository>,
    path: String,
    on_commit: OnCommit,
) {
    let merges = crate::settings::bool(crate::settings::FILE_HISTORY_MERGES);
    let command = json!({ "op": "fileHistory", "path": path, "merges": merges });
    let (window, parent) = (window.clone(), parent.clone());
    let r = repo.clone();
    glib::spawn_future_local(async move {
        let info = gio::spawn_blocking(move || run(&r, command)).await;
        match info {
            Ok(Ok(info)) => show_file_history(&window, &parent, repo, &path, &info, on_commit),
            Ok(Err(e)) => fail(&parent, &e),
            Err(_) => {}
        }
    });
}

/// File History の 1 行（コミット、件名、作者、日時、その時点のパス）。
#[derive(Clone)]
struct Entry {
    oid: String,
    subject: String,
    author: String,
    time: i64,
    path: String,
}

/// 一覧の行: 名前が変わったところの見出し（パス、その下の最初の行）か、コミット。
#[derive(Clone)]
enum HistoryRow {
    Path(String, usize),
    Entry(usize),
}

/// 各コミットの時点のパスから、一覧の行を作る。名前が 1 つだけなら見出しは入れない（窓の題にある）。
fn history_rows(entries: &[Entry]) -> Vec<HistoryRow> {
    let mut rows = Vec::new();
    let renamed = entries.iter().any(|e| e.path != entries[0].path);
    for (i, e) in entries.iter().enumerate() {
        if renamed && (i == 0 || entries[i - 1].path != e.path) {
            rows.push(HistoryRow::Path(e.path.clone(), i));
        }
        rows.push(HistoryRow::Entry(i));
    }
    rows
}

fn show_file_history(
    window: &gtk::Window,
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
    window.set_title(Some(&tr!(
        "File History — {path} ({count} commits)",
        path = path,
        count = entries.len()
    )));
    let rows = Rc::new(history_rows(&entries));
    let entries = Rc::new(entries);

    let model = gtk::StringList::new(&[]);
    for i in 0..rows.len() {
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
    let (e, r) = (entries.clone(), rows.clone());
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let (Some(row), Some(column)) = (
            r.get(item.position() as usize),
            item.child().and_downcast::<gtk::Box>(),
        ) else {
            return;
        };
        // 右クリックで、どの行かを分かるようにする。
        column.set_widget_name(&format!("row:{}", item.position()));
        let subject = column
            .first_child()
            .and_downcast::<gtk::Label>()
            .expect("subject");
        let meta = subject
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("meta");
        match row {
            // ファイル名を先に（狭くても切れない）、ディレクトリを薄く後ろに。説明に、名前が変わったコミット。
            HistoryRow::Path(path, index) => {
                let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
                subject.set_markup(&format!(
                    "<b>{}</b>  <span alpha=\"60%\">{}</span>",
                    glib::markup_escape_text(name),
                    glib::markup_escape_text(dir)
                ));
                meta.set_visible(false);
                item.set_selectable(false);
                item.set_activatable(false);
                let tip = match index.checked_sub(1).and_then(|i| e.get(i)) {
                    Some(newer) => format!(
                        "{path}\n{}",
                        tr!(
                            "Renamed or copied to {path} in {commit}",
                            path = newer.path,
                            commit = &newer.oid[..8.min(newer.oid.len())]
                        )
                    ),
                    None => path.clone(),
                };
                column.set_tooltip_text(Some(&tip));
            }
            HistoryRow::Entry(i) => {
                let entry = &e[*i];
                subject.set_text(&entry.subject);
                meta.set_visible(true);
                meta.set_text(&format!(
                    "{}  ·  {}  ·  {}",
                    entry.author,
                    date(entry.time),
                    &entry.oid[..8.min(entry.oid.len())]
                ));
                item.set_selectable(true);
                item.set_activatable(true);
                column.set_tooltip_text(Some(&entry.path));
            }
        }
    });
    let selection = gtk::SingleSelection::new(Some(model));
    let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
    list.add_css_class("navigation-sidebar");
    let entry_at = {
        let (e, r) = (entries.clone(), rows.clone());
        move |position: u32| -> Option<Entry> {
            match r.get(position as usize)? {
                HistoryRow::Entry(i) => e.get(*i).cloned(),
                HistoryRow::Path(..) => None,
            }
        }
    };
    let open = on_commit.clone();
    let at = entry_at.clone();
    list.connect_activate(move |_, position| {
        if let Some(id) = at(position).and_then(|entry| Oid::from_hex(&entry.oid)) {
            open(id);
        }
    });
    // 右クリック: Show in History、Blame at、コピー、Show Merge Commits（既定も変え、この窓を読み直す）。
    let menu_click = gtk::GestureClick::new();
    menu_click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let (l, at, sel) = (list.downgrade(), entry_at.clone(), selection.clone());
    let (w, p, r, pa, oc) = (
        window.downgrade(),
        parent.downgrade(),
        repo.clone(),
        path.to_owned(),
        on_commit.clone(),
    );
    menu_click.connect_pressed(move |_, _, x, y| {
        let Some(list) = l.upgrade() else { return };
        let mut items = Vec::new();
        // 押した行（ListView の中の行の部品から位置を探す）。
        let picked = list.pick(x, y, gtk::PickFlags::DEFAULT).and_then(|w| {
            let mut cur = Some(w);
            while let Some(c) = cur {
                if let Some(pos) = c.widget_name().strip_prefix("row:") {
                    return pos.parse().ok();
                }
                cur = c.parent();
            }
            None
        });
        if let Some(position) = picked
            && let Some(entry) = at(position)
        {
            sel.set_selected(position);
            let short = entry.oid[..8.min(entry.oid.len())].to_owned();
            if let Some(id) = Oid::from_hex(&entry.oid) {
                let open = oc.clone();
                items.push(crate::menu::action(tr!("Show in History"), move || {
                    open(id)
                }));
            }
            if let Some(parent) = p.upgrade() {
                let (repo, path, oid, oc) =
                    (r.clone(), entry.path.clone(), entry.oid.clone(), oc.clone());
                items.push(crate::menu::action(
                    tr!("Blame at {id}", id = short),
                    move || blame(&parent, repo.clone(), &path, Some(&oid), oc.clone()),
                ));
            }
            items.push(crate::menu::Item::Separator);
            for (title, text) in [
                (tr!("Copy Commit ID"), entry.oid.clone()),
                (tr!("Copy Short ID"), short.clone()),
                (tr!("Copy Path"), entry.path.clone()),
            ] {
                let l = list.downgrade();
                items.push(crate::menu::action(title, move || {
                    if let Some(l) = l.upgrade() {
                        l.clipboard().set_text(&text);
                    }
                }));
            }
            items.push(crate::menu::Item::Separator);
        }
        let merges = crate::settings::bool(crate::settings::FILE_HISTORY_MERGES);
        let (w, p, r, pa, oc) = (w.clone(), p.clone(), r.clone(), pa.clone(), oc.clone());
        items.push(crate::menu::action(
            format!(
                "{}{}",
                if merges { "✓ " } else { "" },
                tr!("Show Merge Commits")
            ),
            move || {
                crate::settings::set_bool(crate::settings::FILE_HISTORY_MERGES, !merges);
                if let (Some(w), Some(p)) = (w.upgrade(), p.upgrade()) {
                    load_file_history(&w, &p, r.clone(), pa.clone(), oc.clone());
                }
            },
        ));
        crate::menu::popup(list.upcast_ref::<gtk::Widget>(), x, y, items);
    });
    list.add_controller(menu_click);
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
    let d = diff.downgrade();
    let at = entry_at.clone();
    let show = move |position: u32| {
        let Some(entry) = at(position) else {
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
    // 最初は一番上のコミット（見出しなら、その下）。
    let first = rows
        .iter()
        .position(|r| matches!(r, HistoryRow::Entry(_)))
        .unwrap_or(0) as u32;
    selection.set_selected(first);
    show(first);
    selection.connect_selected_notify(move |s| show(s.selected()));
    if entries.is_empty() {
        let note = gtk::Label::new(Some(&tr!("No commits changed this file.")));
        note.add_css_class("dim-label");
        window.set_child(Some(&note));
    } else {
        window.set_child(Some(&split));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32, kind: u8) -> amonhen_layout::SyntaxSpan {
        amonhen_layout::SyntaxSpan {
            row: 0,
            start,
            end,
            kind,
            _pad: [0; 3],
        }
    }

    #[test]
    fn splits_syntax_into_lines() {
        // "aé b" / "/* c" / "d */": é は UTF-16 で 1、UTF-8 で 2 バイト。
        let lines = ["aé b", "/* c", "d */"];
        let spans = [span(3, 4, 1), span(5, 14, 2)];
        assert_eq!(
            line_spans(&lines, &spans),
            vec![vec![(4, 5, 1)], vec![(0, 4, 2)], vec![(0, 4, 2)]]
        );
    }
}
