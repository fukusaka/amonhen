//! Amon Hen の GTK4 の FE。Linux・BSD が本命で、GTK の動く Windows・macOS でも動く。
//!
//! `amonhen-gtk [<path>] [--snapshot <png>] [--measure-scroll [--scroll-rows <n>]] [--scroll-to <row> | --select-rows <row,...> | --select-ref <name>] [--menu-ref <name> | --menu-row <row>] [--search <query>] [--open-menu repository|primary] [--perform <json>] [--working-copy [--select-path <path>] [--toggle-row <row>]]`

mod actions;
mod appmenu;
mod commitops;
mod details;
mod diff;
mod graph;
mod history;
mod icon;
mod measure;
mod menu;
mod refops;
mod search;
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

use crate::actions::Actions;
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
    /// `--measure-scroll` で 1 フレームにスクロールする行数。
    scroll_rows: Option<f64>,
    /// Working Copy を開いた状態で始める。
    working_copy: bool,
    /// Working Copy で選んでおくファイル。
    select_path: Option<String>,
    /// 最初に ref を読んだら、この名前の ref をサイドバーで選んだことにする（確認用）。
    select_ref: Option<String>,
    /// 最初に ref を読んだら、この名前の ref の右クリックのメニューを出す（確認用）。
    menu_ref: Option<String>,
    /// History でこれらの行を選んでおく（確認用。`1,3` のように）。
    select_rows: Vec<u32>,
    /// History をこの問い合わせで探しておく（確認用）。
    search: Option<String>,
    /// History のこの行の右クリックのメニューを開いておく（確認用）。
    menu_row: Option<u32>,
    /// 見出しのメニュー（`repository` か `primary`）を開いておく（確認用）。
    open_menu: Option<String>,
    /// 最初に History を出したら、この命令（JSON）を一度実行する（確認用）。
    perform: Option<String>,
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
            "--scroll-rows" => {
                let rows = it.next().ok_or("--scroll-rows <n>")?;
                args.scroll_rows = Some(rows.parse().map_err(|_| "--scroll-rows <n>")?);
            }
            "--working-copy" => args.working_copy = true,
            "--select-ref" => args.select_ref = Some(it.next().ok_or("--select-ref <name>")?),
            "--menu-ref" => args.menu_ref = Some(it.next().ok_or("--menu-ref <name>")?),
            "--select-rows" => {
                let rows = it.next().ok_or("--select-rows <row,row,...>")?;
                args.select_rows = rows
                    .split(',')
                    .map(|r| r.trim().parse().map_err(|_| "--select-rows <row,row,...>"))
                    .collect::<Result<_, _>>()?;
            }
            "--search" => args.search = Some(it.next().ok_or("--search <query>")?),
            "--menu-row" => {
                let row = it.next().ok_or("--menu-row <row>")?;
                args.menu_row = Some(row.parse().map_err(|_| "--menu-row <row>")?);
            }
            "--open-menu" => args.open_menu = Some(it.next().ok_or("--open-menu <name>")?),
            "--perform" => args.perform = Some(it.next().ok_or("--perform <json>")?),
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
    // History の検索の欄（History の上に出す）。
    let search = Rc::new(search::Search::new(&view));
    let history_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    history_column.append(&search.bar);
    history_column.append(&overlay);

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
    columns.set_start_child(Some(&history_column));
    columns.set_end_child(Some(&right));
    columns.set_position(560);
    // 裏で読んでいる詳細と差分の数（`--snapshot` はなくなるまで待つ）。
    let pending = Rc::new(Cell::new(0u32));
    connect_selection(&view, &details, &diff, &right, &pending);

    // History と Working Copy は、サイドバーで行き来する。
    let working = Rc::new(WorkingCopy::new(pending.clone()));
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
    let mut menu_ref = args.menu_ref.clone();
    let mut perform = args.perform.clone();
    if perform.is_some() {
        pending.set(pending.get() + 1);
    }
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
    // 操作は裏で実行し、終わったら History・ref・status を読み直す（ファイルの変化の知らせも来るが、
    // 待たずに出す）。
    let actions = Actions::new(&window);
    let reload_tx = tx.clone();
    let weak_view = view.downgrade();
    let reload: Rc<dyn Fn()> = Rc::new(move || {
        let Some((repo, _)) = weak_view.upgrade().and_then(|v| v.current()) else {
            return;
        };
        let tx = reload_tx.clone();
        std::thread::spawn(move || {
            reload_history(&repo, &tx);
            reload_status(&repo, &tx);
        });
    });
    let r = reload.clone();
    actions.connect_change(move || r());
    // History のコミットの右クリック。
    let (a, v) = (actions.clone(), view.downgrade());
    view.connect_menu(move |row, x, y| {
        let Some(view) = v.upgrade() else { return };
        if let Some((_, history)) = view.current() {
            let rows = view.selection();
            let items = if rows.len() > 1 && rows.contains(&row) {
                commitops::menu_many(&a, &history, &rows, view.upcast_ref())
            } else {
                commitops::menu(&a, &history, row, view.upcast_ref())
            };
            menu::popup(&view, x, y, items);
        }
    });
    // 最後に読んだ ref（メニューにはチェックアウト中のブランチや最新の stash が要る）。
    let refs: Rc<std::cell::RefCell<Option<Arc<amonhen_core::RefsSnapshot>>>> = Rc::default();
    let head = {
        let refs = refs.clone();
        move || {
            refs.borrow()
                .as_ref()
                .and_then(|r| r.refs.head_branch.clone())
        }
    };
    // 別のリポジトリを新しい窓で開く。
    let app_for_open = app.clone();
    let open: Rc<dyn Fn(PathBuf)> = Rc::new(move |path| {
        build_window(
            &app_for_open,
            Args {
                path: Some(path),
                ..Args::default()
            },
        );
    });
    // サブモジュールは、作業ツリーからのパスで開く。
    let open_submodule: refops::OpenSubmodule = {
        let (open, view) = (open.clone(), view.downgrade());
        Rc::new(move |name: &str| {
            let workdir = view
                .upgrade()
                .and_then(|v| v.current())
                .and_then(|(repo, _)| repo.paths().workdir);
            if let Some(dir) = workdir {
                open(dir.join(name));
            }
        })
    };
    let (a, h, o) = (actions.clone(), head.clone(), open_submodule.clone());
    sidebar.connect_menu(move |data, widget, x, y| {
        menu::popup(widget, x, y, refops::menu(&a, data, h().as_deref(), &o));
    });
    let (a, h) = (actions.clone(), head.clone());
    sidebar.connect_drop(move |dragged, data, widget, x, y| {
        menu::popup(
            widget,
            x,
            y,
            refops::drop_menu(&a, dragged, data, h().as_deref()),
        );
    });
    // 見出しのメニュー（主メニューが右端、その左に Undo）。
    let go_to: Rc<dyn Fn(amonhen_model::Oid)> = {
        let (pages, view) = (pages.clone(), view.clone());
        Rc::new(move |id| jump_to(&pages, &view, id, &Rc::new(Cell::new(0))))
    };
    let buttons = appmenu::install(appmenu::Parts {
        app: app.clone(),
        window: window.clone(),
        header: header.clone(),
        actions: actions.clone(),
        pages: pages.clone(),
        sidebar: sidebar.clone(),
        working: working.clone(),
        view: view.clone(),
        refs: refs.clone(),
        reload,
        open,
        go_to,
    });
    let latest_refs = refs;
    search.install(&window, app);
    header.pack_end(&search.toggle);
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
                        actions.set_repo(repo.clone());
                        view.show(repo, history);
                        search.history_changed();
                        if let Some(command) = perform.take() {
                            match serde_json::from_str(&command) {
                                Ok(command) => {
                                    let p = pending.clone();
                                    actions.perform_then(vec![command], move || {
                                        // 読み直しの知らせが届くまで、少し待ってから撮る。
                                        glib::timeout_add_local_once(
                                            std::time::Duration::from_millis(500),
                                            move || p.set(p.get() - 1),
                                        );
                                    });
                                }
                                Err(e) => {
                                    eprintln!("--perform の JSON を読めない: {e}");
                                    pending.set(pending.get() - 1);
                                }
                            }
                        }
                        if first {
                            first = false;
                            // 窓が出てから開く（出る前に開くと、出ないことがある）。
                            let button = match args.open_menu.as_deref() {
                                Some("repository") => Some(buttons.repository.clone()),
                                Some("primary") => Some(buttons.primary.clone()),
                                _ => None,
                            };
                            if let Some((&first, rest)) = args.select_rows.split_first() {
                                view.select(first);
                                for &row in rest {
                                    view.select_with(row, false, true);
                                }
                            }
                            if let Some(query) = &args.search {
                                search.search(query, pending.clone());
                            }
                            if let Some(row) = args.menu_row {
                                view.select(row);
                                pending.set(pending.get() + 1);
                                let (p, v, a) = (pending.clone(), view.clone(), actions.clone());
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(300),
                                    move || {
                                        if let Some((_, history)) = v.current() {
                                            let y = (row as f32 + 0.5) * v.row_height();
                                            let items =
                                                commitops::menu(&a, &history, row, v.upcast_ref());
                                            eprintln!("{}", menu::titles(&items).join("\n"));
                                            menu::popup(&v, 380.0, y as f64, items);
                                        }
                                        p.set(p.get() - 1);
                                    },
                                );
                            }
                            if let Some(button) = button {
                                pending.set(pending.get() + 1);
                                let p = pending.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(300),
                                    move || {
                                        button.popup();
                                        p.set(p.get() - 1);
                                    },
                                );
                            }
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
                        latest_refs.replace(Some(refs.clone()));
                        buttons.show_refs(&refs);
                        if let Some(name) = menu_ref.take()
                            && let Some(entry) = refs.refs.entries.iter().find(|e| e.name == name)
                        {
                            let data = sidebar::Data {
                                title: entry.name.clone(),
                                item: sidebar::Item::Ref(entry.clone()),
                                ..sidebar::Data::default()
                            };
                            let items = refops::menu(
                                &actions,
                                &data,
                                refs.refs.head_branch.as_deref(),
                                &open_submodule,
                            );
                            eprintln!("{}", menu::titles(&items).join("\n"));
                            menu::popup(&sidebar.root, 24.0, 120.0, items);
                        }
                        actions.refresh_undo();
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
                        // Working Copy の操作（破棄・コミット）も Undo の記録を書く。
                        actions.refresh_undo();
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

/// 詳細の欄をメッセージの高さに合わせる（右の列の半分まで。残りは欄の中でスクロール）。
/// 窓が開いた直後は大きさがまだ決まっていないので、決まったフレームで合わせる。
fn fit_details(right: &gtk::Paned, details: Rc<Details>) {
    right.add_tick_callback(move |right, _| {
        if right.width() <= 0 {
            return glib::ControlFlow::Continue;
        }
        let height = details.natural_height(right.width());
        right.set_position(height.min(right.height() / 2).max(60));
        glib::ControlFlow::Break
    });
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
    let loader = Rc::new(Loader {
        details: details.clone(),
        diff: diff.downgrade(),
        right: right.downgrade(),
        pending: pending.clone(),
        generation: Cell::new(0),
    });
    let weak_view = view.downgrade();
    view.connect_selected(move |rows| {
        let Some(view) = weak_view.upgrade() else {
            return;
        };
        let Some((repo, history)) = view.current() else {
            return;
        };
        match rows {
            [] => {}
            [row] => loader.one(repo, history, *row),
            _ => loader.many(repo, history, rows.to_vec(), PairMode::Forward),
        }
    });
}

/// 選んだコミットの詳細と差分を読む。
struct Loader {
    details: Rc<Details>,
    diff: glib::WeakRef<DiffWidget>,
    right: glib::WeakRef<gtk::Paned>,
    /// 裏で読んでいる数（`--snapshot` はなくなるまで待つ）。
    pending: Rc<Cell<u32>>,
    /// 選び直したら増やし、古い結果を捨てる。
    generation: Cell<u64>,
}

impl Loader {
    fn start(&self) -> u64 {
        let g = self.generation.get() + 1;
        self.generation.set(g);
        self.details.clear();
        if let Some(diff) = self.diff.upgrade() {
            diff.clear();
        }
        g
    }

    /// 1 つのコミット: 詳細はすぐ読めるので先に出し、時間のかかる分と差分はその後に。
    fn one(
        self: &Rc<Self>,
        repo: Arc<Repository>,
        history: Arc<amonhen_core::HistorySnapshot>,
        row: u32,
    ) {
        let Some(&id) = history.graph.ids().get(row as usize) else {
            return;
        };
        let g = self.start();
        self.pending.set(self.pending.get() + 1);
        let (this, repo_i) = (self.clone(), repo.clone());
        glib::spawn_future_local(async move {
            let info =
                gio::spawn_blocking(move || repo_i.commit_info(&id).map_err(|e| e.to_string()))
                    .await;
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != g {
                return;
            }
            match info {
                Ok(Ok(info)) => {
                    this.details.show(&info);
                    this.fit();
                }
                Ok(Err(e)) => eprintln!("コミットの詳細を読めない: {e}"),
                Err(_) => {}
            }
        });
        // 時間のかかる分（署名の検証・子・含むブランチとタグなど）。
        self.pending.set(self.pending.get() + 1);
        let (this, repo_e) = (self.clone(), repo.clone());
        glib::spawn_future_local(async move {
            let extras =
                gio::spawn_blocking(move || repo_e.commit_extras(&id).map_err(|e| e.to_string()))
                    .await;
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != g {
                return;
            }
            match extras {
                Ok(Ok(extras)) => {
                    // 詳細より先に届いたら、詳細が出たときに足す。
                    this.details.show_extras(&extras);
                    this.fit();
                }
                Ok(Err(e)) => eprintln!("コミットの詳細を読めない: {e}"),
                Err(_) => {}
            }
        });
        self.diff(repo, history, vec![row], PairMode::Forward, g, |_| {});
    }

    /// 複数のコミット: 選び方の見出しを出し、`selection_diff` の差分を読む。
    fn many(
        self: &Rc<Self>,
        repo: Arc<Repository>,
        history: Arc<amonhen_core::HistorySnapshot>,
        rows: Vec<u32>,
        pair: PairMode,
    ) {
        let g = self.start();
        let weak = Rc::downgrade(self);
        let (r, h, rs) = (repo.clone(), history.clone(), rows.clone());
        let on_pair: Rc<dyn Fn(PairMode)> = Rc::new(move |mode| {
            if let Some(this) = weak.upgrade() {
                this.many(r.clone(), h.clone(), rs.clone(), mode);
            }
        });
        self.details
            .show_selection(rows.len(), None, pair, &[], on_pair.clone());
        // 各コミットの 1 行（短い ID・件名・作者）。
        let ids: Vec<amonhen_model::Oid> = rows
            .iter()
            .filter_map(|&r| history.graph.ids().get(r as usize).copied())
            .collect();
        let count = rows.len();
        let this = self.clone();
        let repo_t = repo.clone();
        self.diff(repo, history, rows, pair, g, move |kind| {
            let (this, repo_t, ids, on_pair) =
                (this.clone(), repo_t.clone(), ids.clone(), on_pair.clone());
            let kind = kind.to_owned();
            glib::spawn_future_local(async move {
                let texts = gio::spawn_blocking(move || {
                    let texts = repo_t.commit_texts(&ids).unwrap_or_default();
                    ids.iter()
                        .zip(&texts)
                        .map(|(id, t)| {
                            (
                                id.to_string()[..8].to_owned(),
                                String::from_utf8_lossy(t.subject()).into_owned(),
                                String::from_utf8_lossy(t.author_name()).into_owned(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default();
                if this.generation.get() == g {
                    this.details
                        .show_selection(count, Some(&kind), pair, &texts, on_pair);
                    this.fit();
                }
            });
        });
    }

    /// 選んだ行の差分を読んで出す。読めたら `then(選び方)` を呼ぶ（`range`・`pair`・`each`）。
    fn diff(
        self: &Rc<Self>,
        repo: Arc<Repository>,
        history: Arc<amonhen_core::HistorySnapshot>,
        rows: Vec<u32>,
        pair: PairMode,
        g: u64,
        then: impl FnOnce(&str) + 'static,
    ) {
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let (selection, stacked) = repo
                    .selection_diff(&history, &rows, pair)
                    .map_err(|e| e.to_string())?;
                let kind = match selection {
                    amonhen_core::Selection::Range { .. } => "range",
                    amonhen_core::Selection::Pair { .. } => "pair",
                    amonhen_core::Selection::Each(_) => "each",
                };
                let view = diff::View::build(&stacked, &[]);
                Ok::<_, String>((kind, Arc::new(diff::Source(stacked)), view))
            })
            .await;
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != g {
                return;
            }
            match result {
                Ok(Ok((kind, source, view))) => {
                    if let Some(diff) = this.diff.upgrade() {
                        diff.show(source, view);
                    }
                    then(kind);
                }
                Ok(Err(e)) => eprintln!("差分を読めない: {e}"),
                Err(_) => {}
            }
        });
    }

    fn fit(&self) {
        if let Some(right) = self.right.upgrade() {
            fit_details(&right, self.details.clone());
        }
    }
}

enum Event {
    History(Arc<Repository>, Arc<amonhen_core::HistorySnapshot>),
    Refs(Arc<amonhen_core::RefsSnapshot>),
    Status(Arc<amonhen_core::StatusSnapshot>),
    Failed(String),
}

/// History と ref を読み直して知らせる（裏のスレッドで呼ぶ）。
fn reload_history(repo: &Arc<Repository>, tx: &async_channel::Sender<Event>) {
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
}

/// status を読み直して知らせる（裏のスレッドで呼ぶ）。
fn reload_status(repo: &Arc<Repository>, tx: &async_channel::Sender<Event>) {
    match repo.load_status() {
        Ok(snapshot) => {
            let _ = tx.send_blocking(Event::Status(snapshot));
        }
        Err(e) => eprintln!("status を読めない: {e}"),
    }
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
    let status = |repo: &Arc<Repository>| reload_status(repo, &tx);
    let reload = |repo: &Arc<Repository>| reload_history(repo, &tx);
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
        let rows = args.scroll_rows.unwrap_or(measure::ROWS_PER_FRAME);
        measure::scroll(window, view, rows);
    }
}

/// 窓の背景の色（テーマの色の名前はテーマによって違う。どれもなければ白）。
#[allow(deprecated)]
fn window_background(window: &gtk::ApplicationWindow) -> gtk::gdk::RGBA {
    let style = window.style_context();
    ["window_bg_color", "theme_bg_color", "bg_color"]
        .iter()
        .find_map(|name| style.lookup_color(name))
        .unwrap_or(gtk::gdk::RGBA::WHITE)
}

/// `font` を `base` の大きさの `factor` 倍にする。大きさがピクセルで決まっている（Ubuntu など）なら、
/// ポイントに読み替えずにピクセルのまま縮める。
pub fn scale_font(
    font: &mut gtk::pango::FontDescription,
    base: &gtk::pango::FontDescription,
    factor: f64,
) {
    let size = base.size() as f64 * factor;
    if base.is_size_absolute() {
        font.set_absolute_size(size);
    } else {
        font.set_size(size as i32);
    }
}

/// `widget` の下の、見えている popover。
fn collect_popovers(widget: &gtk::Widget, out: &mut Vec<gtk::Popover>) {
    let mut child = widget.first_child();
    while let Some(c) = child {
        match c.downcast_ref::<gtk::Popover>() {
            Some(p) if p.is_visible() => out.push(p.clone()),
            _ => collect_popovers(&c, out),
        }
        child = c.next_sibling();
    }
}

/// 窓の中身を PNG にする。
fn snapshot(window: &gtk::ApplicationWindow, png: &Path) -> Result<(), String> {
    let (w, h) = (window.width() as f32, window.height() as f32);
    let scale = window.scale_factor() as f32;
    // 窓そのものの paintable は空になるので、窓の部品（見出しと中身）を順に描く。
    let snap = gtk::Snapshot::new();
    snap.scale(scale, scale);
    snap.append_color(
        &window_background(window),
        &graphene::Rect::new(0.0, 0.0, w, h),
    );
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
    // 開いているメニュー（popover）は別の面に描かれるので、上に重ねる。
    let mut popovers = Vec::new();
    collect_popovers(window.upcast_ref(), &mut popovers);
    for popover in popovers {
        if let Some(bounds) = popover.compute_bounds(window) {
            let paintable = gtk::WidgetPaintable::new(Some(&popover));
            snap.save();
            snap.translate(&bounds.top_left());
            paintable.snapshot(&snap, bounds.width() as f64, bounds.height() as f64);
            snap.restore();
        }
    }
    let node = snap.to_node().ok_or("描くものがない")?;
    let renderer = window.renderer().ok_or("renderer がない")?;
    let texture = renderer.render_texture(
        node,
        Some(&graphene::Rect::new(0.0, 0.0, w * scale, h * scale)),
    );
    texture.save_to_png(png).map_err(|e| e.to_string())
}
