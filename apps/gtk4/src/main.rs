//! Amon Hen の GTK4 の FE。Linux・BSD が本命で、GTK の動く Windows・macOS でも動く。
//!
//! `amonhen-gtk [<path>] [--snapshot <png>] [--measure-scroll] [--scroll-to <row> | --select-ref <name>] [--working-copy [--select-path <path>] [--toggle-row <row>]]`

mod details;
mod diff;
mod graph;
mod history;
mod icon;
mod measure;
mod sidebar;
mod workcopy;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_backend_gix::GixBackend;
use amonhen_core::{PairMode, Repository, cache};
use gtk::prelude::*;
use gtk::{gio, glib, graphene};

use crate::details::Details;
use crate::diff::DiffWidget;
use crate::history::HistoryView;
use crate::sidebar::{Page, Sidebar};
use crate::workcopy::WorkingCopy;

const APP_ID: &str = "io.github.fukusaka.amonhen";

#[derive(Clone, Default)]
struct Args {
    path: Option<PathBuf>,
    /// 見えている行がすべて読めたら、窓を PNG にして終わる。
    snapshot: Option<PathBuf>,
    /// 自動でスクロールして、1 フレームの時間を測って終わる。
    measure_scroll: bool,
    /// Working Copy を開いた状態で始める。
    working_copy: bool,
    /// Working Copy で選んでおくファイル。
    select_path: Option<String>,
    /// 最初に ref を読んだら、この名前の ref をサイドバーで選んだことにする（確認用）。
    select_ref: Option<String>,
    /// Working Copy の差分で、この行の印を一度クリックしたことにする（確認用）。
    toggle_row: Option<u32>,
    scroll_to: Option<u32>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--snapshot" => args.snapshot = Some(it.next().ok_or("--snapshot <png>")?.into()),
            "--measure-scroll" => args.measure_scroll = true,
            "--working-copy" => args.working_copy = true,
            "--select-ref" => args.select_ref = Some(it.next().ok_or("--select-ref <name>")?),
            "--toggle-row" => {
                let row = it.next().ok_or("--toggle-row <row>")?;
                args.toggle_row = Some(row.parse().map_err(|_| "--toggle-row <row>")?);
            }
            "--select-path" => {
                args.select_path = Some(it.next().ok_or("--select-path <path>")?);
            }
            "--scroll-to" => {
                let row = it.next().ok_or("--scroll-to <row>")?;
                args.scroll_to = Some(row.parse().map_err(|_| "--scroll-to <row>")?);
            }
            _ if a.starts_with('-') => return Err(format!("知らないオプション: {a}")),
            _ => args.path = Some(a.into()),
        }
    }
    Ok(args)
}

fn main() -> glib::ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}");
            return glib::ExitCode::FAILURE;
        }
    };
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| build_window(app, args.clone()));
    // 引数は自分で読んだので、GTK には渡さない。
    app.run_with_args::<&str>(&[])
}

/// `path` を含むリポジトリを開く。History は CLI・アプリと同じ永続キャッシュに置く。
fn open(path: &Path) -> Result<Repository, String> {
    let backend = GixBackend::open(path).map_err(|e| e.to_string())?;
    let key = std::fs::canonicalize(path)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
    Ok(match amonhen_remote::local::cache_dir() {
        Some(dir) => Repository::with_cache(backend, cache::path_for(&dir, &key)),
        None => Repository::new(backend),
    })
}

fn build_window(app: &gtk::Application, args: Args) {
    let path = args
        .path
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let name = std::fs::canonicalize(&path)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| path.display().to_string());

    let title = gtk::Label::new(Some(&name));
    title.add_css_class("title");
    let subtitle = gtk::Label::new(None);
    subtitle.add_css_class("subtitle");
    subtitle.set_visible(false);
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 0);
    titles.set_valign(gtk::Align::Center);
    titles.append(&title);
    titles.append(&subtitle);
    let header = gtk::HeaderBar::new();
    header.set_title_widget(Some(&titles));

    let view = HistoryView::default();
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let status = gtk::Label::new(Some("History を読み込んでいます…"));
    status.add_css_class("dim-label");
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&scrolled));
    overlay.add_overlay(&status);

    // 右の列: 選んだコミットの詳細と差分。
    let details = Rc::new(Details::new());
    let diff = DiffWidget::default();
    let diff_scrolled = gtk::ScrolledWindow::builder()
        .child(&diff)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let right = gtk::Paned::new(gtk::Orientation::Vertical);
    right.set_start_child(Some(&details.root));
    right.set_end_child(Some(&diff_scrolled));
    right.set_resize_start_child(false);
    right.set_position(170);
    let columns = gtk::Paned::new(gtk::Orientation::Horizontal);
    columns.set_start_child(Some(&overlay));
    columns.set_end_child(Some(&right));
    columns.set_position(560);
    // 裏で読んでいる詳細と差分の数（`--snapshot` はなくなるまで待つ）。
    let pending = Rc::new(Cell::new(0u32));
    connect_selection(&view, &details, &diff, &right, &pending);

    // History と Working Copy は、サイドバーで行き来する。
    let working = WorkingCopy::new(pending.clone());
    if let Some(path) = &args.select_path {
        working.select_path(path);
    }
    if let Some(row) = args.toggle_row {
        working.toggle_row_once(row);
    }
    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::None);
    pages.add_named(&columns, Some("history"));
    pages.add_named(&working.root, Some("working"));
    let sidebar = Rc::new(Sidebar::new());
    let weak_sidebar = Rc::downgrade(&sidebar);
    working.connect_count(move |count| {
        if let Some(sidebar) = weak_sidebar.upgrade() {
            sidebar.set_working_count(count);
        }
    });
    connect_sidebar(&sidebar, &pages, &view);
    if args.working_copy {
        pages.set_visible_child_name("working");
        sidebar.select_page(Page::WorkingCopy);
    }
    let body = gtk::Paned::new(gtk::Orientation::Horizontal);
    body.set_start_child(Some(&sidebar.root));
    body.set_end_child(Some(&pages));
    body.set_resize_start_child(false);
    body.set_shrink_start_child(false);
    body.set_position(200);
    // 最初の status が届くまでは、撮るのを待つ。
    pending.set(pending.get() + 1);
    let mut waiting_status = true;
    let mut select_ref = args.select_ref.clone();
    if select_ref.is_some() {
        pending.set(pending.get() + 1);
    }

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(format!("{name} — Amon Hen"))
        .default_width(1280)
        .default_height(760)
        .child(&body)
        .build();
    window.set_titlebar(Some(&header));
    window.present();
    view.grab_focus();

    let (tx, rx) = async_channel::unbounded::<Event>();
    std::thread::spawn(move || load(path, tx));
    glib::spawn_future_local(glib::clone!(
        #[weak]
        view,
        #[weak]
        window,
        #[weak]
        status,
        #[weak]
        subtitle,
        async move {
            let mut first = true;
            while let Ok(event) = rx.recv().await {
                match event {
                    Event::History(repo, history) => {
                        status.set_visible(false);
                        working.set_repo(repo.clone());
                        view.show(repo, history);
                        if first {
                            first = false;
                            if let Some(row) = args.scroll_to {
                                view.select(row);
                            }
                            after_first_show(&window, &view, &pending, &args);
                        }
                    }
                    Event::Refs(refs) => {
                        match &refs.refs.head_branch {
                            Some(branch) => subtitle.set_text(branch),
                            None => subtitle.set_text("detached HEAD"),
                        }
                        subtitle.set_visible(true);
                        view.show_refs(&refs);
                        sidebar.show(&refs);
                        if let Some(name) = select_ref.take() {
                            pending.set(pending.get() - 1);
                            let target = refs
                                .refs
                                .entries
                                .iter()
                                .find(|e| e.name == name)
                                .and_then(|e| e.target);
                            match target {
                                Some(target) => jump_to(&pages, &view, target, &pending),
                                None => eprintln!("ref がない: {name}"),
                            }
                        }
                    }
                    Event::Status(snapshot) => {
                        working.show(snapshot);
                        if waiting_status {
                            waiting_status = false;
                            pending.set(pending.get() - 1);
                        }
                    }
                    Event::Failed(e) => {
                        status.set_text(&e);
                        status.set_visible(true);
                        if args.snapshot.is_some() || args.measure_scroll {
                            eprintln!("{e}");
                            window.close();
                        }
                    }
                }
            }
        }
    ));
}

/// サイドバーで選んだ項目: ページなら切り替え、ref なら History でそのコミットを選ぶ。
fn connect_sidebar(sidebar: &Sidebar, pages: &gtk::Stack, view: &HistoryView) {
    let (pages, view) = (pages.downgrade(), view.downgrade());
    sidebar.connect_select(move |data| {
        let (Some(pages), Some(view)) = (pages.upgrade(), view.upgrade()) else {
            return;
        };
        match data.item {
            sidebar::Item::Page(Page::WorkingCopy) => pages.set_visible_child_name("working"),
            sidebar::Item::Page(Page::History) => pages.set_visible_child_name("history"),
            _ => {}
        }
        if let Some(target) = sidebar::target(data) {
            jump_to(&pages, &view, target, &Rc::new(Cell::new(0)));
        }
    });
}

/// History に切り替えて、`target` のコミットを選ぶ。`pending` は終わるまで 1 つ増やしておく。
fn jump_to(
    pages: &gtk::Stack,
    view: &HistoryView,
    target: amonhen_model::Oid,
    pending: &Rc<Cell<u32>>,
) {
    pages.set_visible_child_name("history");
    let Some((_, history)) = view.current() else {
        return;
    };
    // 行の索引は初回に作る（150 万コミットで 80 ms ほど）ので、裏で引く。
    let weak = view.downgrade();
    let pending = pending.clone();
    pending.set(pending.get() + 1);
    glib::spawn_future_local(async move {
        let generation = history.generation;
        let row = gio::spawn_blocking(move || history.row_of(&target)).await;
        pending.set(pending.get() - 1);
        let (Some(view), Ok(Some(row))) = (weak.upgrade(), row) else {
            return;
        };
        if view.current().map(|(_, h)| h.generation) == Some(generation) {
            view.select(row);
            view.grab_focus();
        }
    });
}

/// History で選んだコミットの詳細と差分を、裏で読んで出す。選び直したら古い結果は捨てる。
fn connect_selection(
    view: &HistoryView,
    details: &Rc<Details>,
    diff: &DiffWidget,
    right: &gtk::Paned,
    pending: &Rc<Cell<u32>>,
) {
    let weak_right = right.downgrade();
    let generation = Rc::new(Cell::new(0u64));
    let weak_view = view.downgrade();
    let weak_diff = diff.downgrade();
    let details = details.clone();
    let pending = pending.clone();
    view.connect_selected(move |row| {
        let (Some(view), Some(diff)) = (weak_view.upgrade(), weak_diff.upgrade()) else {
            return;
        };
        let Some((repo, history)) = view.current() else {
            return;
        };
        let Some(&id) = history.graph.ids().get(row as usize) else {
            return;
        };
        let g = generation.get() + 1;
        generation.set(g);
        details.clear();
        diff.clear();

        // 詳細はすぐ読めるので先に出し、差分はその後に。
        pending.set(pending.get() + 2);
        let (details, generation_d, pending_d, repo_d) = (
            details.clone(),
            generation.clone(),
            pending.clone(),
            repo.clone(),
        );
        let weak_right = weak_right.clone();
        glib::spawn_future_local(async move {
            let info =
                gio::spawn_blocking(move || repo_d.commit_info(&id).map_err(|e| e.to_string()))
                    .await;
            pending_d.set(pending_d.get() - 1);
            if generation_d.get() != g {
                return;
            }
            match info {
                Ok(Ok(info)) => {
                    details.show(&info);
                    // 詳細の欄をメッセージの高さに合わせる（右の列の半分まで。残りは欄の中でスクロール）。
                    // 窓が開いた直後は大きさがまだ決まっていないので、決まったフレームで合わせる。
                    if let Some(right) = weak_right.upgrade() {
                        right.add_tick_callback(move |right, _| {
                            if right.width() <= 0 {
                                return glib::ControlFlow::Continue;
                            }
                            let height = details.natural_height(right.width());
                            right.set_position(height.min(right.height() / 2).max(60));
                            glib::ControlFlow::Break
                        });
                    }
                }
                Ok(Err(e)) => eprintln!("コミットの詳細を読めない: {e}"),
                Err(_) => {}
            }
        });
        let (generation, pending) = (generation.clone(), pending.clone());
        let weak_diff = diff.downgrade();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let (_, stacked) = repo
                    .selection_diff(&history, &[row], PairMode::Forward)
                    .map_err(|e| e.to_string())?;
                let view = diff::View::build(&stacked, &[]);
                Ok::<_, String>((Arc::new(diff::Source(stacked)), view))
            })
            .await;
            pending.set(pending.get() - 1);
            let Some(diff) = weak_diff.upgrade() else {
                return;
            };
            if generation.get() != g {
                return;
            }
            match result {
                Ok(Ok((source, view))) => diff.show(source, view),
                Ok(Err(e)) => eprintln!("差分を読めない: {e}"),
                Err(_) => {}
            }
        });
    });
}

enum Event {
    History(Arc<Repository>, Arc<amonhen_core::HistorySnapshot>),
    Refs(Arc<amonhen_core::RefsSnapshot>),
    Status(Arc<amonhen_core::StatusSnapshot>),
    Failed(String),
}

/// 裏のスレッドで開いて読み、変化を見張って読み直す。
fn load(path: PathBuf, tx: async_channel::Sender<Event>) {
    let repo = match open(&path) {
        Ok(repo) => Arc::new(repo),
        Err(e) => {
            let _ = tx.send_blocking(Event::Failed(format!("リポジトリを開けない: {e}")));
            return;
        }
    };
    // 前回の History をすぐ出してから、最新にする。
    if let Some(cached) = repo.load_cached_history() {
        let _ = tx.send_blocking(Event::History(repo.clone(), cached));
    }
    let status = |repo: &Arc<Repository>| match repo.load_status() {
        Ok(snapshot) => {
            let _ = tx.send_blocking(Event::Status(snapshot));
        }
        Err(e) => eprintln!("status を読めない: {e}"),
    };
    let reload = |repo: &Arc<Repository>| {
        match repo.load_history() {
            Ok(history) => {
                let _ = tx.send_blocking(Event::History(repo.clone(), history));
            }
            Err(e) => {
                let _ = tx.send_blocking(Event::Failed(format!("History を読めない: {e}")));
            }
        }
        if let Ok(refs) = repo.load_refs() {
            let _ = tx.send_blocking(Event::Refs(refs));
        }
    };
    reload(&repo);
    status(&repo);

    // ref が変わったら History を、index か作業ツリーが変わったら status を読み直す。知らせは監視の
    // スレッドから来るので、このスレッドで受けて読む。
    let (changed_tx, changed_rx) = std::sync::mpsc::channel();
    let _watcher = match amonhen_watch::Watcher::new(repo.paths(), move |changes| {
        let _ = changed_tx.send(changes.0);
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("リポジトリを見張れない: {e}");
            return;
        }
    };
    use amonhen_watch::Changes;
    while let Ok(mut bits) = changed_rx.recv() {
        // まとめて来た知らせは 1 回の読み直しにする。
        while let Ok(more) = changed_rx.try_recv() {
            bits |= more;
        }
        if tx.is_closed() {
            return;
        }
        let changes = Changes(bits);
        if changes.contains(Changes::REFS) {
            reload(&repo);
        }
        if changes.contains(Changes::REFS)
            || changes.contains(Changes::INDEX)
            || changes.contains(Changes::WORKTREE)
        {
            status(&repo);
        }
    }
}

fn after_first_show(
    window: &gtk::ApplicationWindow,
    view: &HistoryView,
    pending: &Rc<Cell<u32>>,
    args: &Args,
) {
    let pending = pending.clone();
    if let Some(png) = args.snapshot.clone() {
        // 見えている行が読めて、レーンが落ち着くまで待ってから撮る。
        glib::timeout_add_local(
            std::time::Duration::from_millis(50),
            glib::clone!(
                #[weak]
                window,
                #[weak]
                view,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    if (view.is_mapped() && !view.is_settled()) || pending.get() > 0 {
                        return glib::ControlFlow::Continue;
                    }
                    match snapshot(&window, &png) {
                        Ok(()) => eprintln!("{}", png.display()),
                        Err(e) => eprintln!("撮れない: {e}"),
                    }
                    window.close();
                    glib::ControlFlow::Break
                }
            ),
        );
    } else if args.measure_scroll {
        measure::scroll(window, view);
    }
}

/// 窓の中身を PNG にする。
fn snapshot(window: &gtk::ApplicationWindow, png: &Path) -> Result<(), String> {
    let (w, h) = (window.width() as f32, window.height() as f32);
    let scale = window.scale_factor() as f32;
    // 窓そのものの paintable は空になるので、窓の部品（見出しと中身）を順に描く。
    let snap = gtk::Snapshot::new();
    snap.scale(scale, scale);
    snap.append_color(&gtk::gdk::RGBA::WHITE, &graphene::Rect::new(0.0, 0.0, w, h));
    let mut child = window.first_child();
    while let Some(widget) = child {
        if let Some(bounds) = widget.compute_bounds(window) {
            let paintable = gtk::WidgetPaintable::new(Some(&widget));
            snap.save();
            snap.translate(&bounds.top_left());
            paintable.snapshot(&snap, bounds.width() as f64, bounds.height() as f64);
            snap.restore();
        }
        child = widget.next_sibling();
    }
    let node = snap.to_node().ok_or("描くものがない")?;
    let renderer = window.renderer().ok_or("renderer がない")?;
    let texture = renderer.render_texture(
        node,
        Some(&graphene::Rect::new(0.0, 0.0, w * scale, h * scale)),
    );
    texture.save_to_png(png).map_err(|e| e.to_string())
}
