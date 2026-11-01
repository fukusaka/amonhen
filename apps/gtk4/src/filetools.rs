//! Blame と File History の窓（macOS 版の `FileTools.swift` と同じ）。
//!
//! - Blame: ファイルの各行を最後に変えたコミット。同じコミットの行をまとめ、まとまりの 1 行目に ID・作者・日時、
//!   2 行目に件名を出し、まとまりごとに背景を交互に変える。行をダブルクリックすると、そのコミットを History で選ぶ。
//! - File History: 左にファイルを変えたコミット、右にそのコミットでのファイルの差分。名前の変更も追う。
//!
//! どちらも行は `GtkListView` で、見えている行の部品しか作らない（大きなファイルでも重くならない）。

use std::cell::{Cell, RefCell};
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
    crate::titles::date_time(time, false)
}

thread_local! {
    /// 開いている Blame・File History の窓（何の窓か → 窓）。
    static TOOL_WINDOWS: std::cell::RefCell<std::collections::HashMap<String, glib::WeakRef<gtk::Window>>> =
        std::cell::RefCell::default();
}

/// 何の窓か（同じリポジトリの同じファイル、Blame なら同じコミット。macOS 版のタブの鍵と同じ）。
fn tool_key(kind: &str, repo: &Arc<Repository>, path: &str, rev: Option<&str>) -> String {
    format!(
        "{kind}\0{}\0{path}\0{}",
        Arc::as_ptr(repo) as usize,
        rev.unwrap_or("")
    )
}

/// `key` の窓。もう開いていれば、それを前に出して返す（中身は呼んだ側が入れ直す。macOS 版は前のタブを
/// 読み直したものに入れ替える）。なければ新しく開く。
fn tool_window(parent: &gtk::ApplicationWindow, key: String, title: &str) -> gtk::Window {
    let open = TOOL_WINDOWS.with(|w| {
        let mut w = w.borrow_mut();
        w.retain(|_, window| window.upgrade().is_some());
        w.get(&key).and_then(|window| window.upgrade())
    });
    if let Some(window) = open {
        window.set_title(Some(title));
        window.present();
        return window;
    }
    let window = gtk::Window::builder()
        .title(title)
        .default_width(1100)
        .default_height(760)
        .build();
    window.set_transient_for(Some(parent));
    window.set_destroy_with_parent(true);
    TOOL_WINDOWS.with(|w| w.borrow_mut().insert(key, window.downgrade()));
    window.present();
    window
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
    let key = tool_key("blame", &repo, path, rev);
    let command = json!({ "op": "blame", "path": path, "rev": rev });
    // 窓は先に出し、読んでいる間は「Loading…」（macOS 版と同じ）。
    let window = tool_window(parent, key, &title);
    window.set_child(Some(&note(&tr!("Loading…"))));
    let path = path.to_owned();
    glib::spawn_future_local(async move {
        let info = gio::spawn_blocking(move || run(&repo, command)).await;
        match info {
            Ok(Ok(info)) => {
                let spans: Rc<std::cell::RefCell<LineSpans>> = Rc::default();
                let (model, selection) = show_blame(&window, &info, spans.clone(), on_commit);
                // 構文の色は、行を出してから裏で付ける（読めたら描き直す）。
                let colored = gio::spawn_blocking(move || blame_syntax(&path, &info)).await;
                if let Ok(colored) = colored
                    && colored.iter().any(|l| !l.is_empty())
                {
                    spans.replace(colored);
                    let selected = selection.selected();
                    let n = model.n_items();
                    model.items_changed(0, n, n);
                    selection.set_selected(selected);
                }
            }
            Ok(Err(e)) => window.set_child(Some(&note(&e))),
            Err(_) => {}
        }
    });
}

/// 窓の中身の代わりの知らせ（読んでいる・読めなかった理由）。
fn note(text: &str) -> gtk::Label {
    let note = gtk::Label::new(Some(text));
    note.add_css_class("dim-label");
    note.set_wrap(true);
    note.set_selectable(true);
    note
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

/// Blame の行を窓に出す。構文の色（`spans`）は後から入れられるよう、一覧の中身と選択を返す。
fn show_blame(
    window: &gtk::Window,
    info: &Value,
    spans: Rc<std::cell::RefCell<LineSpans>>,
    on_commit: OnCommit,
) -> (gtk::StringList, gtk::SingleSelection) {
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
        for &(start, end, kind) in s.borrow().get(index).into_iter().flatten() {
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
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    // 開いたときは何も選ばない。
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    selection.set_selected(gtk::INVALID_LIST_POSITION);
    let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
    list.add_css_class("view");
    let (r, c) = (rows.clone(), commits.clone());
    list.connect_activate(move |_, position| {
        let Some(row) = r.get(position as usize) else {
            return;
        };
        // まだコミットしていない行（ID がすべて 0）は History に出せない（macOS 版と同じく何もしない）。
        let oid = &c[row.commit].0;
        if oid.bytes().any(|b| b != b'0')
            && let Some(id) = Oid::from_hex(oid)
        {
            on_commit(id);
        }
    });
    install_css();
    let scrolled = gtk::ScrolledWindow::builder().child(&list).build();
    window.set_child(Some(&scrolled));
    (model, selection)
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
/// `fileHistoryMerges`（右クリックの Show Merge Commits で切り替え、開いているほかの File History も読み直す）。
/// 窓は先に出し、新しい 100 コミットを読んだら出して、残りを読み続ける（macOS 版と同じ）。
pub fn file_history(
    parent: &gtk::ApplicationWindow,
    repo: Arc<Repository>,
    path: &str,
    on_commit: OnCommit,
) {
    // 同じファイルの File History がもう開いていれば、それを読み直す（macOS 版と同じ）。
    let key = tool_key("fileHistory", &repo, path, None);
    let window = tool_window(parent, key, &format!("File History — {path}"));
    if let Some(view) = FileHistoryView::of(&window) {
        view.reload();
        return;
    }
    // タブの窓ではないので、Ctrl+F・Ctrl+G は自分で差分の検索の帯に回す。
    crate::difffind::add_shortcuts(&window);
    let view = FileHistoryView::new(&window, parent, repo, path.to_owned(), on_commit);
    view.load();
}

/// 先に読むコミットの数（`git log -n` は Bloom フィルタがなくても早く返る。macOS 版と同じ）。
const FIRST_PAGE: usize = 100;

/// File History の 1 行（コミット、件名、作者、日時、その時点のパス）。
#[derive(Clone)]
struct Entry {
    oid: String,
    subject: String,
    author: String,
    time: i64,
    path: String,
}

/// 一覧の行: 名前が変わったところの見出し（パス、その下の最初の行）か、コミットか、残りを読んでいる印。
#[derive(Clone)]
enum HistoryRow {
    Path(String, usize),
    Entry(usize),
    Loading,
}

/// 各コミットの時点のパスから、一覧の行を作る。名前が 1 つだけなら見出しは入れない（窓の題にある）。
/// 読み終えていなければ、最後に「Loading…」の行。
fn history_rows(entries: &[Entry], complete: bool) -> Vec<HistoryRow> {
    let mut rows = Vec::new();
    let renamed = entries.iter().any(|e| e.path != entries[0].path);
    for (i, e) in entries.iter().enumerate() {
        if renamed && (i == 0 || entries[i - 1].path != e.path) {
            rows.push(HistoryRow::Path(e.path.clone(), i));
        }
        rows.push(HistoryRow::Entry(i));
    }
    if !complete {
        rows.push(HistoryRow::Loading);
    }
    rows
}

thread_local! {
    /// 開いている File History の窓と、その中身。
    static FILE_HISTORIES: RefCell<Vec<(glib::WeakRef<gtk::Window>, Rc<FileHistoryView>)>> =
        RefCell::default();
}

/// File History の窓の中身。読み直しても部品は作り直さず、一覧の中身だけを入れ替える（選んでいたコミットは、
/// まだあれば選んだまま）。
struct FileHistoryView {
    window: glib::WeakRef<gtk::Window>,
    parent: glib::WeakRef<gtk::ApplicationWindow>,
    repo: Arc<Repository>,
    path: String,
    on_commit: OnCommit,
    /// merge のコミットも出しているか（Settings の File History）。
    merges: Cell<bool>,
    entries: RefCell<Vec<Entry>>,
    rows: RefCell<Vec<HistoryRow>>,
    complete: Cell<bool>,
    /// 読み直した回数（前の読み込みの結果を捨てる）。
    generation: Cell<u64>,
    /// 読み直す前に選んでいたコミット（読み直した一覧にあれば選び直す）。
    kept: RefCell<Option<String>>,
    /// 差分に出しているコミットと、差分を読んだ回数。
    shown: RefCell<Option<String>>,
    diff_generation: Cell<u64>,
    /// 一覧を入れ替えている間は、選択の変化で差分を読まない。
    applying: Cell<bool>,
    model: gtk::StringList,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    diff: DiffWidget,
    find: crate::difffind::DiffFind,
}

impl FileHistoryView {
    fn of(window: &gtk::Window) -> Option<Rc<Self>> {
        FILE_HISTORIES.with(|v| {
            v.borrow()
                .iter()
                .find(|(w, _)| w.upgrade().as_ref() == Some(window))
                .map(|(_, view)| view.clone())
        })
    }

    fn new(
        window: &gtk::Window,
        parent: &gtk::ApplicationWindow,
        repo: Arc<Repository>,
        path: String,
        on_commit: OnCommit,
    ) -> Rc<Self> {
        let model = gtk::StringList::new(&[]);
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
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        let list = gtk::ListView::new(Some(selection.clone()), Some(factory.clone()));
        list.add_css_class("navigation-sidebar");
        let left = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let diff = DiffWidget::default();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&diff)
            .vexpand(true)
            .build();
        // 差分の下に検索の帯（Ctrl+F。macOS 版と同じく、どの差分でも探せる）。
        let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
        right.append(&scrolled);
        let (find, find_bar) = crate::difffind::DiffFind::new(&diff, &right);
        right.append(&find_bar);
        let split = gtk::Paned::new(gtk::Orientation::Horizontal);
        split.set_start_child(Some(&left));
        split.set_end_child(Some(&right));
        split.set_resize_start_child(false);
        split.set_position(360);
        window.set_child(Some(&split));
        diff.show_message(&tr!("Loading…"));

        let view = Rc::new(Self {
            window: window.downgrade(),
            parent: parent.downgrade(),
            repo,
            path,
            on_commit,
            merges: Cell::new(crate::settings::bool(crate::settings::FILE_HISTORY_MERGES)),
            entries: RefCell::default(),
            rows: RefCell::new(vec![HistoryRow::Loading]),
            complete: Cell::new(false),
            generation: Cell::new(0),
            kept: RefCell::default(),
            shown: RefCell::default(),
            diff_generation: Cell::new(0),
            applying: Cell::new(false),
            model,
            selection,
            list,
            diff,
            find,
        });
        FILE_HISTORIES.with(|v| v.borrow_mut().push((window.downgrade(), view.clone())));
        // 窓を閉じたら、中身を手放す。
        window.connect_destroy(|window| {
            let window = window.clone();
            FILE_HISTORIES.with(|v| {
                v.borrow_mut()
                    .retain(|(w, _)| w.upgrade().is_some_and(|w| w != window))
            });
        });
        view.update_model();
        view.update_title();
        Self::connect(&view, &factory);
        view
    }

    fn connect(this: &Rc<Self>, factory: &gtk::SignalListItemFactory) {
        let weak = Rc::downgrade(this);
        factory.connect_bind(move |_, item| {
            let Some(this) = weak.upgrade() else { return };
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let rows = this.rows.borrow();
            let entries = this.entries.borrow();
            let (Some(row), Some(column)) = (
                rows.get(item.position() as usize),
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
                    let tip = match index.checked_sub(1).and_then(|i| entries.get(i)) {
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
                    let entry = &entries[*i];
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
                // 残りを読んでいる。
                HistoryRow::Loading => {
                    subject.set_markup(&format!(
                        "<span alpha=\"55%\">{}</span>",
                        glib::markup_escape_text(&tr!("Loading…"))
                    ));
                    meta.set_visible(false);
                    item.set_selectable(false);
                    item.set_activatable(false);
                    column.set_tooltip_text(None);
                }
            }
        });
        let weak = Rc::downgrade(this);
        this.list.connect_activate(move |_, position| {
            let Some(this) = weak.upgrade() else { return };
            if let Some(id) = this.entry_at(position).and_then(|e| Oid::from_hex(&e.oid)) {
                (this.on_commit)(id);
            }
        });
        let weak = Rc::downgrade(this);
        this.selection.connect_selected_notify(move |s| {
            let Some(this) = weak.upgrade() else { return };
            if !this.applying.get() {
                Self::show_diff(&this, s.selected());
            }
        });
        // 右クリック: Show in History、Blame at、コピー、Show Merge Commits（既定も変え、開いているほかの
        // File History も読み直す）。
        let menu_click = gtk::GestureClick::new();
        menu_click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let weak = Rc::downgrade(this);
        menu_click.connect_pressed(move |_, _, x, y| {
            if let Some(this) = weak.upgrade() {
                Self::popup_menu(&this, x, y);
            }
        });
        this.list.add_controller(menu_click);
        // Ctrl+C: 選んだコミットの ID をコピーする（macOS 版と同じ）。
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(this);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(this) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if key.to_lower() == gtk::gdk::Key::c
                && modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK)
                && let Some(oid) = this.selected_oid()
            {
                this.list.clipboard().set_text(&oid);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        this.list.add_controller(keys);
        // Show Merge Commits を変えたら（どの窓からでも）、読み直す（macOS 版と同じ）。
        if let Some(window) = this.window.upgrade() {
            let weak = Rc::downgrade(this);
            crate::settings::connect_changed_while(&window, move |key| {
                let Some(this) = weak.upgrade() else { return };
                let merges = crate::settings::bool(crate::settings::FILE_HISTORY_MERGES);
                if key == crate::settings::FILE_HISTORY_MERGES && merges != this.merges.get() {
                    this.merges.set(merges);
                    this.reload();
                }
            });
        }
    }

    fn entry_at(&self, position: u32) -> Option<Entry> {
        match self.rows.borrow().get(position as usize)? {
            HistoryRow::Entry(i) => self.entries.borrow().get(*i).cloned(),
            _ => None,
        }
    }

    fn selected_oid(&self) -> Option<String> {
        self.entry_at(self.selection.selected()).map(|e| e.oid)
    }

    fn popup_menu(this: &Rc<Self>, x: f64, y: f64) {
        let list = &this.list;
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
            && let Some(entry) = this.entry_at(position)
        {
            this.selection.set_selected(position);
            let short = entry.oid[..8.min(entry.oid.len())].to_owned();
            if let Some(id) = Oid::from_hex(&entry.oid) {
                let open = this.on_commit.clone();
                items.push(crate::menu::action(tr!("Show in History"), move || {
                    open(id)
                }));
            }
            if let Some(parent) = this.parent.upgrade() {
                let (repo, path, oid, oc) = (
                    this.repo.clone(),
                    entry.path.clone(),
                    entry.oid.clone(),
                    this.on_commit.clone(),
                );
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
        let merges = this.merges.get();
        items.push(crate::menu::choice(
            tr!("Show Merge Commits"),
            merges,
            move || {
                crate::settings::set_bool(crate::settings::FILE_HISTORY_MERGES, !merges);
            },
        ));
        crate::menu::popup(list.upcast_ref::<gtk::Widget>(), x, y, items);
    }

    /// 初めから読み直す（選んでいたコミットは、まだあれば選んだまま）。
    fn reload(self: &Rc<Self>) {
        let selected = self.selected_oid().or_else(|| self.kept.borrow().clone());
        self.kept.replace(selected);
        self.entries.borrow_mut().clear();
        self.complete.set(false);
        self.rows.replace(vec![HistoryRow::Loading]);
        self.update_model();
        self.update_title();
        self.load();
    }

    /// 新しい 100 コミットを読んで出し、続きがあれば全部を読んで入れ替える。
    fn load(self: &Rc<Self>) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let weak = Rc::downgrade(self);
        let (repo, path, merges) = (self.repo.clone(), self.path.clone(), self.merges.get());
        glib::spawn_future_local(async move {
            let read = |limit: Option<usize>| {
                let (repo, path) = (repo.clone(), path.clone());
                async move {
                    let mut command =
                        json!({ "op": "fileHistory", "path": path, "merges": merges });
                    if let Some(limit) = limit {
                        command["limit"] = json!(limit);
                    }
                    gio::spawn_blocking(move || run(&repo, command))
                        .await
                        .unwrap_or_else(|_| Err(tr!("Cancelled")))
                }
            };
            let mut limit = Some(FIRST_PAGE);
            loop {
                let info = read(limit).await;
                let Some(this) = weak.upgrade() else { return };
                if this.generation.get() != generation {
                    return;
                }
                match info {
                    Ok(info) => {
                        let complete = limit.is_none() || info["more"].as_bool() != Some(true);
                        this.apply(entries_of(&info), complete);
                        if complete {
                            return;
                        }
                        limit = None;
                    }
                    Err(e) => {
                        this.fail(&e);
                        return;
                    }
                }
            }
        });
    }

    /// 読めなかった: 読めた分は出したまま、差分の場所に理由を出す（macOS 版と同じ）。
    fn fail(self: &Rc<Self>, e: &str) {
        eprintln!("ファイルの履歴を読めない: {e}");
        self.complete.set(true);
        let entries = self.entries.borrow().clone();
        self.apply(entries, true);
        self.diff.show_message(e);
    }

    /// 一覧を入れ替える。選んでいたコミットは選んだまま（見出しが増えて行がずれても）。
    fn apply(self: &Rc<Self>, entries: Vec<Entry>, complete: bool) {
        let selected = self.selected_oid().or_else(|| self.kept.borrow().clone());
        if complete {
            self.kept.replace(None);
        }
        let rows = history_rows(&entries, complete);
        let first = entries.first().map(|e| e.oid.clone());
        let row_of = |oid: &Option<String>| {
            rows.iter().position(
                |r| matches!(r, HistoryRow::Entry(i) if Some(&entries[*i].oid) == oid.as_ref()),
            )
        };
        // 選んでいたコミットが読み直した一覧になければ、読み終えたところで先頭を選ぶ。
        let target = match selected {
            Some(_) => row_of(&selected).or_else(|| complete.then(|| row_of(&first)).flatten()),
            None => row_of(&first),
        };
        let empty = entries.is_empty();
        self.entries.replace(entries);
        self.rows.replace(rows);
        self.complete.set(complete);
        self.update_model();
        self.update_title();
        match target {
            Some(row) => {
                self.applying.set(true);
                self.selection.set_selected(row as u32);
                self.applying.set(false);
                Self::show_diff(self, row as u32);
            }
            None if complete && empty => {
                self.shown.replace(None);
                self.diff
                    .show_message(&tr!("No commits changed this file."));
            }
            None => {}
        }
    }

    /// 一覧の中身（行の数）を入れ直す。
    fn update_model(&self) {
        self.applying.set(true);
        let n = self.rows.borrow().len();
        let items: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        let items: Vec<&str> = items.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &items);
        self.selection.set_selected(gtk::INVALID_LIST_POSITION);
        self.applying.set(false);
    }

    fn update_title(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let count = self.entries.borrow().len();
        let title = if self.complete.get() {
            tr!(
                "File History — {path} ({count} commits)",
                path = self.path,
                count = count
            )
        } else if count == 0 {
            tr!("File History — {path} (loading…)", path = self.path)
        } else {
            tr!(
                "File History — {path} ({count}+ commits)",
                path = self.path,
                count = count
            )
        };
        window.set_title(Some(&title));
    }

    /// 選んだコミットでの、このファイルの差分（出しているコミットなら読み直さない）。
    fn show_diff(this: &Rc<Self>, position: u32) {
        let Some(entry) = this.entry_at(position) else {
            return;
        };
        if this.shown.borrow().as_deref() == Some(entry.oid.as_str()) {
            return;
        }
        let Some(id) = Oid::from_hex(&entry.oid) else {
            return;
        };
        this.shown.replace(Some(entry.oid.clone()));
        let g = this.diff_generation.get() + 1;
        this.diff_generation.set(g);
        let (repo, path) = (this.repo.clone(), entry.path.clone());
        let weak = Rc::downgrade(this);
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
            let Some(this) = weak.upgrade() else { return };
            if this.diff_generation.get() != g {
                return;
            }
            match view {
                Ok(Ok(view)) => {
                    this.diff.show_plain(view);
                    this.find.refresh();
                }
                Ok(Err(e)) => {
                    eprintln!("差分を読めない: {e}");
                    this.diff.show_message(&tr!("Can’t read the diff"));
                }
                Err(_) => {}
            }
        });
    }
}

/// `fileHistory` の答えのコミット。
fn entries_of(info: &Value) -> Vec<Entry> {
    info["entries"]
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
        .collect()
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

    /// 名前が変わったところに見出し、読み終えていなければ最後に「Loading…」の行。
    #[test]
    fn history_rows_add_paths_and_loading() {
        let entry = |path: &str| Entry {
            oid: "0".repeat(40),
            subject: String::new(),
            author: String::new(),
            time: 0,
            path: path.to_owned(),
        };
        let kinds = |rows: Vec<HistoryRow>| -> Vec<String> {
            rows.into_iter()
                .map(|r| match r {
                    HistoryRow::Path(p, i) => format!("{p}@{i}"),
                    HistoryRow::Entry(i) => i.to_string(),
                    HistoryRow::Loading => "…".to_owned(),
                })
                .collect()
        };
        let same = [entry("a"), entry("a")];
        assert_eq!(kinds(history_rows(&same, true)), ["0", "1"]);
        assert_eq!(kinds(history_rows(&same, false)), ["0", "1", "…"]);
        let renamed = [entry("b"), entry("a")];
        assert_eq!(
            kinds(history_rows(&renamed, true)),
            ["b@0", "0", "a@1", "1"]
        );
        assert_eq!(kinds(history_rows(&[], false)), ["…"]);
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
