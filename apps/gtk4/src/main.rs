//! Amon Hen の GTK4 の FE。Linux・BSD が本命で、GTK の動く Windows・macOS でも動く。
//!
//! `amonhen-gtk [<path>] [--snapshot <png>] [--measure-scroll [--scroll-rows <n>]] [--scroll-to <row> | --select-rows <row,...> | --select-ref <name>] [--menu-ref <name> | --menu-row <row> [--menu-item <title>]] [--search <query> [--only-matching]] [--blame <path> | --file-history <path> | --activity <tab> | --settings <tab> | --merge-editor <path> [--accept ours|theirs|both|base]] [--reflog] [--apply-patch <path,...>] [--open-menu repository|primary] [--action <id>] [--print-file-menu <path>] [--perform <json>] [--stashes [--stash-button apply|pop|drop|branch]] [--working-copy [--select-path <path>] [--toggle-row <row>] [--layout list|tree|flat]] [--range-diff[=<ranges>] [--select-pair <n>] [--menu-pair <n>]] [--repositories | --quick-open <query>]`

mod actions;
mod activity;
mod appmenu;
mod askpass;
mod bookmarks;
mod commitmsg;
mod commitops;
mod configtable;
mod details;
mod diff;
mod filetools;
mod github;
mod graph;
mod history;
mod historydisplay;
mod historyedit;
mod icon;
mod measure;
mod menu;
mod merge;
mod mergeseveral;
mod patches;
mod quickopen;
mod rangediff;
mod reffacts;
mod reflog;
mod refmarks;
mod refops;
mod remotesheet;
mod repobrowser;
mod repopages;
mod repos;
mod reposettings;
mod rewritedraw;
mod search;
mod settings;
mod sheets;
mod sidebar;
mod stashes;
mod statusbar;
mod titles;
mod workcopy;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_backend_gix::GixBackend;
use amonhen_core::{PairMode, Repository, cache};
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib, graphene};

use crate::actions::Actions;
use crate::details::Details;
use crate::diff::DiffWidget;
use crate::history::HistoryView;
use crate::sidebar::{Page, Sidebar};
use crate::stashes::Stashes;
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
    /// `--search` で、一致したコミットだけを出す（確認用）。
    only_matching: bool,
    /// このファイルの Blame・File History の窓を開いておく（確認用。`--snapshot` はその窓を撮る）。
    blame: Option<String>,
    /// Activity の窓をこのタブで開いておく（確認用）。
    activity: Option<String>,
    /// 設定の窓をこのタブで開いておく（確認用）。
    settings: Option<String>,
    /// リポジトリの設定の窓をこのページで開いておく（確認用）。
    repository_settings: Option<String>,
    /// このファイルのマージエディタを開いておく（確認用）。
    merge_editor: Option<String>,
    file_history: Option<String>,
    /// History のこの行の右クリックのメニューを開いておく（確認用）。
    menu_row: Option<u32>,
    /// `--menu-row`・`--reflog-row` のメニューを出す代わりに、この項目を選ぶ（確認用。下に開くメニューの中は
    /// `Reset main to 1a2b3c4d > Hard…` のように ` > ` でつなぐ）。
    menu_item: Option<String>,
    /// Reflog の画面を開いておく（確認用）。`--select-rows`・`--menu-row` は Reflog の行のこと。
    reflog: bool,
    /// 見出しのメニュー（`repository` か `primary`）を開いておく（確認用）。
    open_menu: Option<String>,
    /// 最初に History を出したら、この命令（JSON）を一度実行する（確認用）。
    perform: Option<String>,
    /// Working Copy のこのファイルの右クリックのメニューを、文字で書き出す（確認用）。
    print_file_menu: Option<String>,
    /// 最初に History を出したら、この窓のアクション（`win.<id>`）を一度実行する（確認用。`--snapshot` は
    /// 開いたシートを撮る）。
    action: Option<String>,
    /// Working Copy の一覧の見せ方（`list`・`tree`・`flat`。確認用）。
    layout: Option<String>,
    /// Working Copy の差分で、この行の印を一度クリックしたことにする（確認用）。
    toggle_row: Option<u32>,
    scroll_to: Option<u32>,
    /// 最初に ref を読んだら、Stashes の画面を開く（確認用）。
    stashes: bool,
    /// `--stashes` で、選んだ stash の帯のボタン（`apply`・`pop`・`drop`・`branch`）を一度押す（確認用。
    /// `--snapshot` は開いたシートを撮る）。
    stash_button: Option<usize>,
    /// Range Diff を開いておく（確認用）。`Some(None)` は今のブランチを書き換える前の版と、`Some(Some(範囲))` は
    /// その範囲（空白で区切る）で比べる。
    range_diff: Option<Option<String>>,
    /// Range Diff でこの組を選んでおく・この組の右クリックのメニューを出す（確認用）。
    select_pair: Option<u32>,
    menu_pair: Option<u32>,
    /// これらのパッチのファイル（`,` で区切る）を当てる前の確認を出す（確認用）。
    apply_patch: Vec<PathBuf>,
    /// History の編集モードをこのコミットから開いておく（確認用）。`edit_plan` は行ですること（新しい順に
    /// `pick,squash,drop` など）、`edit_move` は `from:above` で行を動かし、`edit_apply` なら Apply Changes を押す。
    edit_commits: Option<String>,
    edit_plan: Option<String>,
    edit_move: Option<(usize, usize)>,
    edit_apply: bool,
    /// サイドバーのこの行（ref・リモート・見出しの名前）の右クリックのメニューを出す（確認用。`--menu-item` で
    /// 項目を選ぶ）・右端の印の吹き出しを出す。
    sidebar_menu: Option<String>,
    sidebar_mark: Option<String>,
    /// サイドバーをこの語で絞り込んでおく（確認用）。
    filter_sidebar: Option<String>,
    /// リポジトリの窓の代わりに Repositories の窓を出す（確認用。`--snapshot` はその窓を撮る）。
    repositories: bool,
    /// Quick Open を、この問い合わせを入れて出す（確認用。`--snapshot` はその窓を撮る）。
    quick_open: Option<String>,
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
            "--stashes" => args.stashes = true,
            "--stash-button" => {
                let usage = "--stash-button apply|pop|drop|branch";
                let name = it.next().ok_or(usage)?;
                args.stash_button = Some(
                    ["apply", "pop", "drop", "branch"]
                        .iter()
                        .position(|b| *b == name)
                        .ok_or(usage)?,
                );
            }
            "--select-ref" => args.select_ref = Some(it.next().ok_or("--select-ref <name>")?),
            "--menu-ref" => args.menu_ref = Some(it.next().ok_or("--menu-ref <name>")?),
            "--sidebar-menu" => args.sidebar_menu = Some(it.next().ok_or("--sidebar-menu <name>")?),
            "--filter-sidebar" => {
                args.filter_sidebar = Some(it.next().ok_or("--filter-sidebar <words>")?)
            }
            "--sidebar-mark" => args.sidebar_mark = Some(it.next().ok_or("--sidebar-mark <name>")?),
            "--select-rows" => {
                let rows = it.next().ok_or("--select-rows <row,row,...>")?;
                args.select_rows = rows
                    .split(',')
                    .map(|r| r.trim().parse().map_err(|_| "--select-rows <row,row,...>"))
                    .collect::<Result<_, _>>()?;
            }
            "--only-matching" => args.only_matching = true,
            "--blame" => args.blame = Some(it.next().ok_or("--blame <path>")?),
            "--activity" => args.activity = Some(it.next().ok_or("--activity <tab>")?),
            "--settings" => args.settings = Some(it.next().ok_or("--settings <tab>")?),
            "--repository-settings" => {
                args.repository_settings = Some(it.next().ok_or("--repository-settings <page>")?)
            }
            "--merge-editor" => args.merge_editor = Some(it.next().ok_or("--merge-editor <path>")?),
            "--accept" => merge::accept_once(&it.next().ok_or("--accept ours|theirs|both|base")?),
            "--file-history" => args.file_history = Some(it.next().ok_or("--file-history <path>")?),
            "--search" => args.search = Some(it.next().ok_or("--search <query>")?),
            "--menu-row" => {
                let row = it.next().ok_or("--menu-row <row>")?;
                args.menu_row = Some(row.parse().map_err(|_| "--menu-row <row>")?);
            }
            "--reflog" => args.reflog = true,
            "--repositories" => args.repositories = true,
            "--quick-open" => args.quick_open = Some(it.next().ok_or("--quick-open <query>")?),
            "--apply-patch" => {
                let paths = it.next().ok_or("--apply-patch <path,...>")?;
                args.apply_patch = paths.split(',').map(PathBuf::from).collect();
            }
            "--edit-commits" => {
                args.edit_commits = Some(it.next().ok_or("--edit-commits <commit>")?)
            }
            "--edit-plan" => args.edit_plan = Some(it.next().ok_or("--edit-plan <actions>")?),
            "--edit-move" => {
                let spec = it.next().ok_or("--edit-move <from>:<above>")?;
                let (a, b) = spec.split_once(':').ok_or("--edit-move <from>:<above>")?;
                args.edit_move = Some((
                    a.parse().map_err(|_| "--edit-move <from>:<above>")?,
                    b.parse().map_err(|_| "--edit-move <from>:<above>")?,
                ));
            }
            "--edit-apply" => args.edit_apply = true,
            "--menu-item" => args.menu_item = Some(it.next().ok_or("--menu-item <title>")?),
            "--open-menu" => args.open_menu = Some(it.next().ok_or("--open-menu <name>")?),
            "--perform" => args.perform = Some(it.next().ok_or("--perform <json>")?),
            "--action" => args.action = Some(it.next().ok_or("--action <id>")?),
            "--print-file-menu" => {
                args.print_file_menu = Some(it.next().ok_or("--print-file-menu <path>")?)
            }
            "--layout" => args.layout = Some(it.next().ok_or("--layout list|tree|flat")?),
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
            "--range-diff" => args.range_diff = Some(None),
            _ if a.starts_with("--range-diff=") => {
                args.range_diff = Some(Some(a["--range-diff=".len()..].to_owned()));
            }
            "--select-pair" => {
                let row = it.next().ok_or("--select-pair <n>")?;
                args.select_pair = Some(row.parse().map_err(|_| "--select-pair <n>")?);
            }
            "--menu-pair" => {
                let row = it.next().ok_or("--menu-pair <n>")?;
                args.menu_pair = Some(row.parse().map_err(|_| "--menu-pair <n>")?);
            }
            _ if a.starts_with('-') => return Err(tr!("Unknown option: {option}", option = a)),
            _ => args.path = Some(a.into()),
        }
    }
    Ok(args)
}

fn main() -> glib::ExitCode {
    // git・ssh が資格情報を尋ねてきた（`askpass` のスクリプトから呼ばれた）。
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).map(String::as_str) == Some("--askpass") {
        std::process::exit(askpass::run(raw.get(2).map_or("", String::as_str)));
    }
    // 資格情報を尋ねる先を決める（git・daemon を動かす前に）。
    askpass::install();
    // コアが作る文（エラーなど）の言語。Linux の慣習どおり LC_ALL・LC_MESSAGES・LANG から。
    amonhen_model::i18n::set_default(amonhen_model::i18n::Lang::from_env());
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
    // 窓のアイコン（配るときに hicolor のテーマに置く。.desktop の Icon と同じ名前）。GTK を初期化した後に。
    app.connect_startup(|_| gtk::Window::set_default_icon_name(APP_ID));
    app.connect_activate(move |app| launch(app, &args));
    // 引数は自分で読んだので、GTK には渡さない。
    app.run_with_args::<&str>(&[])
}

/// リポジトリを開く。`location` は手元のパス、`daemon:/path`（手元の共有の daemon。いなければ起動する）、
/// `ssh://[user@]host[:port]/path`（SSH 先の daemon。なければエンジンを送り込む）のどれか。History は CLI・アプリと
/// 同じ永続キャッシュに置く（手元の共有の daemon なら、daemon が用意したものを読む）。
fn open(location: &Path) -> Result<Repository, String> {
    let text = location.to_string_lossy();
    if amonhen_remote::location::is_remote(&text) {
        return open_remote(&text).map_err(|e| e.to_string());
    }
    let path = location;
    // Settings > Advanced: 手元のリポジトリも共有の daemon 越しに開く。
    if settings::bool(settings::OPEN_THROUGH_DAEMON) {
        let path = amonhen_remote::local::canonical_path(&text).map_err(|e| e.to_string())?;
        return open_remote(&format!("daemon:{path}")).map_err(|e| e.to_string());
    }
    let backend = GixBackend::open(path).map_err(|e| e.to_string())?;
    let key = std::fs::canonicalize(path)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
    Ok(match amonhen_remote::local::cache_dir() {
        Some(dir) => Repository::with_cache(backend, cache::path_for(&dir, &key)),
        None => Repository::new(backend),
    })
}

/// daemon 越しに開く（macOS 版の `amonhen_repo_open_location` と同じ）。
fn open_remote(location: &str) -> amonhen_model::Result<Repository> {
    use amonhen_remote::ssh;
    let daemon = engine();
    let conn = if location.starts_with("ssh://") {
        // SSH 先に送るエンジン（`amonhen-<os>-<arch>`）の置き場所: `AMONHEN_ENGINES`、なければ実行ファイルの隣の
        // `engines`（配るときに同梱する。macOS 版の Resources/engines と同じ）。SSH 先が手元と同じ OS と CPU なら
        // 手元のエンジンを送る。
        let engines = std::env::var_os("AMONHEN_ENGINES")
            .map(PathBuf::from)
            .or_else(|| {
                let exe = std::env::current_exe().ok()?;
                Some(exe.parent()?.join("engines"))
            })
            .unwrap_or_default();
        let daemons = ssh::Daemons::new(engines, Some((ssh::local_platform(), daemon.clone())));
        let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
        // SSH 先ごとの選択（macOS の Settings > SSH Hosts）は、GTK4 のアプリにはまだない。
        ssh::connect(
            &program,
            &ssh::Target::parse(location)?,
            &daemons,
            &ssh::HostOptions::default(),
        )?
    } else {
        let path = amonhen_remote::location::local_path(location)?;
        amonhen_remote::local::connect(path, &daemon.to_string_lossy())?
    };
    let backend = amonhen_remote::RemoteBackend::connect(conn)?;
    // SSH 先の History は手元にキャッシュする（つなぎ直したら新しいコミットだけを受け取る）。手元の共有の daemon
    // なら、daemon が用意したファイルを直接読むので持たない。
    Ok(match amonhen_remote::local::cache_dir() {
        Some(dir) if location.starts_with("ssh://") => {
            Repository::with_cache(backend, cache::path_for(&dir, location))
        }
        _ => Repository::new(backend),
    })
}

/// 共有の daemon として起動するエンジン: 置き場所の `current`（`amonhen install` が置く）か、PATH の `amonhen`。
fn engine() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(current) = data.map(|d| d.join("amonhen/current/amonhen"))
        && current.is_file()
    {
        return current;
    }
    PathBuf::from("amonhen")
}

/// リポジトリを新しい窓で開く。
fn opener(app: &gtk::Application) -> Rc<dyn Fn(PathBuf)> {
    let app = app.clone();
    Rc::new(move |path| {
        build_window(
            &app,
            Args {
                path: Some(path),
                ..Args::default()
            },
        )
    })
}

/// 起動したとき: 引数のリポジトリ、なければ今のフォルダのリポジトリを開く。
fn launch(app: &gtk::Application, args: &Args) {
    let window = if args.repositories {
        Some(repobrowser::show(app, opener(app)).upcast::<gtk::Window>())
    } else if let Some(query) = &args.quick_open {
        let window = quickopen::show_with(None, opener(app), query);
        window.set_application(Some(app));
        Some(window)
    } else {
        build_window(app, args.clone());
        None
    };
    if args.repositories
        && let Some(row) = args.menu_row
    {
        let item = args.menu_item.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            repobrowser::debug_menu(row, item)
        });
    }
    if let (Some(window), Some(png)) = (window, args.snapshot.clone()) {
        repobrowser::snapshot_when_ready(&window, png);
    }
}

fn build_window(app: &gtk::Application, args: Args) {
    let path = args
        .path
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    // File > Open Recent に出す（確認用の撮影では覚えない）。
    if args.snapshot.is_none() && !args.measure_scroll {
        repos::record(&path);
    }
    // 窓の名前: リポジトリのフォルダーの名前（daemon・SSH 越しなら、場所の最後の部分と接続先）。
    let text = path.to_string_lossy().into_owned();
    let name = if amonhen_remote::location::is_remote(&text) {
        let last = text
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&text);
        match text.strip_prefix("ssh://") {
            Some(rest) => format!("{last} — {}", rest.split('/').next().unwrap_or_default()),
            None => last.to_owned(),
        }
    } else {
        std::fs::canonicalize(&path)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| text.clone())
    };

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
    let status = gtk::Label::new(Some(&tr!("Loading History…")));
    status.add_css_class("dim-label");
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&scrolled));
    overlay.add_overlay(&status);
    // History の検索の欄（History の上に出す）。
    let search = Rc::new(search::Search::new(&view));
    let history_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let (history_bar, display_button) = historydisplay::bar();
    history_column.append(&history_bar);
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
    // 並び順・折りたたみ（View の History の表示）に合わせて History を作る。
    let shaper = historydisplay::Shaper::new(&view, search.clone(), pending.clone());

    // History と Working Copy は、サイドバーで行き来する。
    let working = Rc::new(WorkingCopy::new(pending.clone()));
    if let Some(path) = &args.select_path {
        working.select_path(path);
    }
    if let Some(row) = args.toggle_row {
        working.toggle_row_once(row);
    }
    if let Some(layout) = &args.layout {
        working.set_layout(layout);
    }
    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::None);
    pages.add_named(&columns, Some("history"));
    pages.add_named(&working.root, Some("working"));
    // Stashes の画面（stash の一覧と、選んだ stash の中身）。
    let stashes = Rc::new(Stashes::new(pending.clone()));
    pages.add_named(&stashes.root, Some("stashes"));
    // Range Diff（開いている間だけサイドバーに出す）。
    let range_diff = rangediff::RangeDiff::new(&view, pending.clone());
    pages.add_named(&range_diff.root, Some(rangediff::PAGE));
    let sidebar = Rc::new(Sidebar::new());
    let range_diff_opener = rangediff::Opener::new(&range_diff, &pages, &sidebar);
    let o = range_diff_opener.clone();
    range_diff.connect_close(move || o.close());
    let weak_sidebar = Rc::downgrade(&sidebar);
    working.connect_count(move |count| {
        if let Some(sidebar) = weak_sidebar.upgrade() {
            sidebar.set_working_count(count);
        }
    });
    connect_sidebar(&sidebar, &pages, &view, &stashes);
    if let Some(words) = &args.filter_sidebar {
        sidebar.filter(words);
    }
    sidebar.debug_once(
        args.sidebar_menu.clone(),
        args.sidebar_mark.clone(),
        args.menu_item.clone(),
        pending.clone(),
    );
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
    // 窓の下のステータスバー（途中の操作・失敗・終わった操作の一言）。
    let statusbar = statusbar::StatusBar::new();
    body.set_vexpand(true);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&body);
    content.append(&statusbar.root);
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
    let mut open_stashes = args.stashes;
    if open_stashes {
        pending.set(pending.get() + 1);
    }

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(format!("{name} — Amon Hen"))
        .default_width(1280)
        .default_height(760)
        .child(&content)
        .build();
    window.set_titlebar(Some(&header));
    historydisplay::install_actions(&window);
    window.present();
    view.grab_focus();

    let (tx, rx) = async_channel::unbounded::<Event>();
    // 操作は裏で実行し、終わったら History・ref・status を読み直す（ファイルの変化の知らせも来るが、
    // 待たずに出す）。
    let actions = Actions::new(&window);
    // パッチのファイルを窓に落としたら当てる。
    patches::install_drop(&window, &actions);
    working.set_actions(actions.clone());
    stashes.set_actions(actions.clone());
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
    // Blame・File History（History の差分のファイルの見出しと、Working Copy のファイルの右クリック）。
    let on_commit: filetools::OnCommit = {
        let (pages, view, sidebar) = (pages.clone(), view.clone(), sidebar.clone());
        Rc::new(move |id| {
            sidebar.select_page(Page::History);
            jump_to(&pages, &view, id, &Rc::new(Cell::new(0)));
        })
    };
    // Reflog の画面（HEAD の reflog と、選んだ行のコミットの中身）。
    let reflog = Rc::new(reflog::ReflogView::new(
        actions.clone(),
        pending.clone(),
        on_commit.clone(),
    ));
    pages.add_named(&reflog.root, Some("reflog"));
    reflog.set_range_diff(range_diff_opener.clone());
    let (v, w, oc) = (view.downgrade(), window.downgrade(), on_commit.clone());
    diff.connect_file_menu(move |path, x, y| {
        let (Some(view), Some(window)) = (v.upgrade(), w.upgrade()) else {
            return;
        };
        let Some((repo, history)) = view.current() else {
            return;
        };
        let mut items = Vec::new();
        // 1 つのコミットを選んでいるときだけ、そのコミットでの blame。
        if let [row] = view.selection()[..]
            && let Some(id) = history.graph.ids().get(row as usize)
        {
            let hex = id.to_string();
            let (repo, window, path, oc) =
                (repo.clone(), window.clone(), path.to_owned(), oc.clone());
            items.push(menu::action(
                tr!("Blame at {commit}", commit = &hex[..8]),
                move || {
                    filetools::blame(&window, repo.clone(), &path, Some(&hex), oc.clone());
                },
            ));
        }
        let (window, path, oc) = (window.clone(), path.to_owned(), oc.clone());
        items.push(menu::action("File History", move || {
            filetools::file_history(&window, repo.clone(), &path, oc.clone());
        }));
        menu::popup(&view, x, y, items);
    });
    let (v, w, oc) = (view.downgrade(), window.downgrade(), on_commit.clone());
    working.connect_file_menu(move |path, can_blame, can_history| {
        let (Some(view), Some(window)) = (v.upgrade(), w.upgrade()) else {
            return Vec::new();
        };
        let Some((repo, _)) = view.current() else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if can_blame {
            let (repo, window, path, oc) =
                (repo.clone(), window.clone(), path.to_owned(), oc.clone());
            items.push(menu::action("Blame", move || {
                filetools::blame(&window, repo.clone(), &path, None, oc.clone());
            }));
        }
        if can_history {
            let (window, path, oc) = (window.clone(), path.to_owned(), oc.clone());
            items.push(menu::action("File History", move || {
                filetools::file_history(&window, repo.clone(), &path, oc.clone());
            }));
        }
        items
    });
    range_diff.connect_show_in_history({
        let on_commit = on_commit.clone();
        move |id| on_commit(id)
    });
    // History の編集モード（History の画面と入れ替える）。
    let edit_opener = historyedit::Opener::new(
        actions.clone(),
        &pages,
        &columns,
        sidebar.clone(),
        statusbar.clone(),
        pending.clone(),
    );
    // History のコミットの右クリック。
    let (a, v, rd) = (actions.clone(), view.downgrade(), range_diff_opener.clone());
    let (sh, eo) = (shaper.clone(), edit_opener.clone());
    view.connect_menu(move |row, x, y| {
        let Some(view) = v.upgrade() else { return };
        if let Some((_, history)) = view.current() {
            let rows = view.selection();
            let items = if rows.len() > 1 && rows.contains(&row) {
                commitops::menu_many(&a, &history, &rows, view.upcast_ref(), &rd, &eo)
            } else {
                // 折りたたんだ merge なら、取り込んだ側を開く・たたむ項目を先に。
                let mut items = sh.merge_items(&history, row);
                items.extend(commitops::menu(&a, &history, row, view.upcast_ref(), &eo));
                items
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
    // 失敗はステータスバーに、終わった操作の一言も（前の失敗は消す）。
    let (bar, a, o) = (statusbar.clone(), actions.clone(), open.clone());
    actions.connect_failure(move |failure| bar.show_failure(failure, &a, o.clone()));
    let (bar, h) = (statusbar.clone(), head.clone());
    actions.connect_done(move |command| {
        bar.hide(statusbar::Row::Failure);
        if let Some(text) = statusbar::done_text(command, h().as_deref()) {
            bar.flash(&text);
        }
    });
    // daemon・SSH 越しのつながりが切れたら、ステータスバーで知らせ、同じ場所につなぎ直せるようにする
    // （新しい窓で開き直し、この窓を閉じる）。
    if let Some(location) = args.path.clone() {
        let (v, bar, w, app) = (
            view.downgrade(),
            statusbar.clone(),
            window.downgrade(),
            app.clone(),
        );
        glib::timeout_add_seconds_local(3, move || {
            let (Some(view), Some(window)) = (v.upgrade(), w.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            let closed = view
                .current()
                .and_then(|(repo, _)| repo.connection_state())
                .and_then(|c| c.closed);
            if let Some(reason) = closed {
                let text = location.to_string_lossy().into_owned();
                let host = text
                    .trim_start_matches("daemon:")
                    .strip_prefix("ssh://")
                    .and_then(|rest| rest.split('/').next())
                    .map_or_else(|| "daemon".to_owned(), str::to_owned);
                let (app, location, window) = (app.clone(), location.clone(), window.downgrade());
                bar.show_disconnected(&host, &reason, move || {
                    build_window(
                        &app,
                        Args {
                            path: Some(location.clone()),
                            ..Args::default()
                        },
                    );
                    if let Some(w) = window.upgrade() {
                        w.close();
                    }
                });
            }
            glib::ControlFlow::Continue
        });
    }
    // 自動の fetch（Settings > General で決めた間隔。聞かずにすべてのリモートから。失敗はステータスバーに）。
    let elapsed = Rc::new(Cell::new(0u32));
    let (v, bar, a, o, r) = (
        view.downgrade(),
        statusbar.clone(),
        actions.clone(),
        open.clone(),
        reload.clone(),
    );
    glib::timeout_add_seconds_local(60, move || {
        let Some(view) = v.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let minutes = settings::auto_fetch_minutes();
        elapsed.set(elapsed.get() + 1);
        if minutes == 0 || elapsed.get() < minutes {
            return glib::ControlFlow::Continue;
        }
        elapsed.set(0);
        let Some((repo, _)) = view.current() else {
            return glib::ControlFlow::Continue;
        };
        let (bar, a, o, r) = (bar.clone(), a.clone(), o.clone(), r.clone());
        let name = repo_name(&repo);
        glib::spawn_future_local(async move {
            let command = serde_json::json!({ "op": "fetch" });
            let reply = sheets::run(&repo, command.clone()).await;
            if reply["error"].is_string() {
                let mut failure = statusbar::Failure::from_reply(&reply, Some(command));
                failure.heading = Some(amonhen_model::tr!("Couldn’t Fetch Automatically"));
                activity::event(
                    &name,
                    amonhen_model::tr!("Automatic fetch failed: {error}", error = failure.message),
                );
                bar.show_failure(failure, &a, o);
            } else {
                activity::event(&name, amonhen_model::tr!("Automatic fetch"));
            }
            r();
        });
        glib::ControlFlow::Continue
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
    let (rd, rdo) = (Rc::downgrade(&range_diff), range_diff_opener.clone());
    sidebar.set_actions(actions.clone());
    sidebar.connect_menu(move |data, known| match (&data.item, rd.upgrade()) {
        (sidebar::Item::Page(Page::RangeDiff), Some(range_diff)) => range_diff.sidebar_menu(),
        _ => refops::menu(&a, data, known, h().as_deref(), &o, &rdo),
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
        range_diff: range_diff_opener.clone(),
        location: text.clone(),
    });
    let latest_refs = refs;
    search.install(&window, app);
    header.pack_end(&search.toggle);
    let order = settings::history_order();
    std::thread::spawn(move || load(path, order, tx));
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
                        stashes.set_repo(repo.clone());
                        reflog.set_repo(repo.clone());
                        actions.set_repo(repo.clone());
                        shaper.present(repo, history);
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
                                Some("history-display") => Some(display_button.clone()),
                                _ => None,
                            };
                            if args.reflog {
                                // 読み終えてから行を選ぶ・メニューを出す。
                                pending.set(pending.get() + 1);
                                let (r, p) = (reflog.clone(), pending.clone());
                                let (select, menu_row, item) = (
                                    args.select_rows.first().copied(),
                                    args.menu_row,
                                    args.menu_item.clone(),
                                );
                                reflog.after_load(move || {
                                    if let Some(row) = select.or(menu_row) {
                                        r.select(row);
                                    }
                                    if let Some(row) = menu_row {
                                        let items = r.menu(row);
                                        eprintln!("{}", menu::titles(&items).join("\n"));
                                        match &item {
                                            Some(path) => {
                                                if !menu::activate(&items, path) {
                                                    eprintln!("メニューの項目がない: {path}");
                                                }
                                            }
                                            None => r.popup_menu(row),
                                        }
                                    }
                                    // 選んだ行の詳細と差分を読み始めてから減らす。
                                    glib::idle_add_local_once(move || p.set(p.get() - 1));
                                });
                                sidebar.select_page(Page::Reflog);
                                pages.set_visible_child_name("reflog");
                            } else if let Some((&first, rest)) = args.select_rows.split_first() {
                                view.select(first);
                                for &row in rest {
                                    view.select_with(row, false, true);
                                }
                            }
                            if let Some(path) = &args.merge_editor {
                                let (repo, _) = view.current().expect("history");
                                merge::open(
                                    window.upcast_ref(),
                                    repo,
                                    actions.clone(),
                                    path,
                                    || {},
                                );
                                pending.set(pending.get() + 1);
                                let p = pending.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(1500),
                                    move || p.set(p.get() - 1),
                                );
                            }
                            if let Some(id) = args.action.clone() {
                                // ref を読むまで待ってから（Merge・Rebase・Pop などは ref から候補を作る）。
                                pending.set(pending.get() + 1);
                                let (p, w, r) =
                                    (pending.clone(), window.downgrade(), latest_refs.clone());
                                glib::timeout_add_local(
                                    std::time::Duration::from_millis(100),
                                    move || {
                                        let Some(window) = w.upgrade() else {
                                            return glib::ControlFlow::Break;
                                        };
                                        if r.borrow().is_none() {
                                            return glib::ControlFlow::Continue;
                                        }
                                        let _ = WidgetExt::activate_action(
                                            &window,
                                            &format!("win.{id}"),
                                            None,
                                        );
                                        let p = p.clone();
                                        glib::timeout_add_local_once(
                                            std::time::Duration::from_millis(2000),
                                            move || p.set(p.get() - 1),
                                        );
                                        glib::ControlFlow::Break
                                    },
                                );
                            }
                            if let Some(tab) = &args.settings {
                                settings::show(&window);
                                settings::select(tab);
                                pending.set(pending.get() + 1);
                                let p = pending.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(500),
                                    move || p.set(p.get() - 1),
                                );
                            }
                            if !args.apply_patch.is_empty() {
                                patches::apply(&actions, args.apply_patch.clone());
                                pending.set(pending.get() + 1);
                                let p = pending.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(1500),
                                    move || p.set(p.get() - 1),
                                );
                            }
                            if let Some(page) = &args.repository_settings {
                                let _ = WidgetExt::activate_action(
                                    &window,
                                    "win.repository-settings",
                                    None,
                                );
                                reposettings::select(&window, page);
                                pending.set(pending.get() + 1);
                                let p = pending.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(
                                        1500 + reposettings::script_millis(),
                                    ),
                                    move || p.set(p.get() - 1),
                                );
                            }
                            if let Some(tab) = &args.activity {
                                let repo = view.current().map(|(r, _)| r);
                                activity::Activity::show(&window, repo, actions.clone(), engine());
                                activity::Activity::select(&window, tab);
                                pending.set(pending.get() + 1);
                                let p = pending.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(1500),
                                    move || p.set(p.get() - 1),
                                );
                            }
                            if args.blame.is_some() || args.file_history.is_some() {
                                let (repo, _) = view.current().expect("history");
                                let oc: filetools::OnCommit = Rc::new(|_| {});
                                match (&args.blame, &args.file_history) {
                                    (Some(path), _) => {
                                        filetools::blame(&window, repo, path, None, oc)
                                    }
                                    (_, Some(path)) => {
                                        filetools::file_history(&window, repo, path, oc)
                                    }
                                    _ => {}
                                }
                                // 窓が開いて中身が読めるまで待つ（裏で読むので少しかかる）。
                                pending.set(pending.get() + 1);
                                let p = pending.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(1500),
                                    move || p.set(p.get() - 1),
                                );
                            }
                            if let Some(query) = &args.search {
                                search.search(query, args.only_matching, pending.clone());
                            }
                            if let Some(row) = args.menu_row.filter(|_| !args.reflog) {
                                // 複数選んだ行の上なら、選んだまま（まとめての操作のメニュー）。
                                let many =
                                    args.select_rows.len() > 1 && args.select_rows.contains(&row);
                                if !many {
                                    view.select(row);
                                }
                                let rd = range_diff_opener.clone();
                                pending.set(pending.get() + 1);
                                let (p, v, a) = (pending.clone(), view.clone(), actions.clone());
                                let (sh, eo) = (shaper.clone(), edit_opener.clone());
                                let item = args.menu_item.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(300),
                                    move || {
                                        if let Some((_, history)) = v.current() {
                                            let y = (row as f32 + 0.5) * v.row_height();
                                            let mut items = sh.merge_items(&history, row);
                                            items.extend(if many {
                                                let rows = v.selection();
                                                commitops::menu_many(
                                                    &a,
                                                    &history,
                                                    &rows,
                                                    v.upcast_ref(),
                                                    &rd,
                                                    &eo,
                                                )
                                            } else {
                                                commitops::menu(
                                                    &a,
                                                    &history,
                                                    row,
                                                    v.upcast_ref(),
                                                    &eo,
                                                )
                                            });
                                            eprintln!("{}", menu::titles(&items).join("\n"));
                                            match &item {
                                                Some(path) => {
                                                    if !menu::activate(&items, path) {
                                                        eprintln!("メニューの項目がない: {path}");
                                                    }
                                                }
                                                None => menu::popup(&v, 380.0, y as f64, items),
                                            }
                                        }
                                        // 項目が開いたシートが出て、中身を調べ終えるまで待つ。
                                        let wait = if item.is_some() { 1500 } else { 0 };
                                        glib::timeout_add_local_once(
                                            std::time::Duration::from_millis(wait),
                                            move || p.set(p.get() - 1),
                                        );
                                    },
                                );
                            }
                            if let Some(start) = &args.edit_commits {
                                edit_opener.open(start, None);
                                pending.set(pending.get() + 1);
                                let (p, eo) = (pending.clone(), edit_opener.clone());
                                let (plan, moved, apply) =
                                    (args.edit_plan.clone(), args.edit_move, args.edit_apply);
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(800),
                                    move || {
                                        if let Some(plan) = &plan {
                                            eo.set_plan(plan);
                                        }
                                        if let Some((from, above)) = moved {
                                            eo.move_row(from, above);
                                        }
                                        if apply {
                                            eo.apply();
                                        }
                                        // 見通し（rewritePreview）が届くのを待ってから撮る。
                                        glib::timeout_add_local_once(
                                            std::time::Duration::from_millis(1500),
                                            move || p.set(p.get() - 1),
                                        );
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
                            if let Some(ranges) = &args.range_diff {
                                range_diff.debug_once(args.select_pair, args.menu_pair);
                                match ranges {
                                    Some(r) => range_diff_opener
                                        .ranges(r.split_whitespace().map(str::to_owned).collect()),
                                    None => range_diff_opener.previous(None),
                                }
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
                        stashes.show(&refs);
                        // 最後の stash を戻した・消したら、空の Stashes の画面から History に移る。
                        if stashes.is_empty()
                            && pages.visible_child_name().as_deref() == Some("stashes")
                        {
                            pages.set_visible_child_name("history");
                            sidebar.select_page(Page::History);
                        }
                        if open_stashes && view.current().is_some() {
                            open_stashes = false;
                            if !stashes.is_empty() {
                                pages.set_visible_child_name("stashes");
                                sidebar.select_page(Page::Stashes);
                                if let Some(button) = args.stash_button {
                                    stashes.click(button);
                                    // シートが出るまで待つ。
                                    pending.set(pending.get() + 1);
                                    let p = pending.clone();
                                    glib::timeout_add_local_once(
                                        std::time::Duration::from_millis(2000),
                                        move || p.set(p.get() - 1),
                                    );
                                }
                            }
                            pending.set(pending.get() - 1);
                        }
                        latest_refs.replace(Some(refs.clone()));
                        actions.set_head(refs.refs.head_branch.clone());
                        // HEAD が動いたら Reflog も読み直す（見えていなければ、次に出すときに）。
                        reflog.reload();
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
                                &sidebar.known(),
                                refs.refs.head_branch.as_deref(),
                                &open_submodule,
                                &range_diff_opener,
                            );
                            eprintln!("{}", menu::titles(&items).join("\n"));
                            menu::popup(&sidebar.root, 24.0, 120.0, items);
                        }
                        actions.refresh_undo();
                        if let Some(name) = select_ref.take() {
                            pending.set(pending.get() - 1);
                            // stash は Stashes の画面で選ぶ。
                            if name.starts_with("stash@{") {
                                stashes.select(&name);
                                pages.set_visible_child_name("stashes");
                                sidebar.select_page(Page::Stashes);
                                continue;
                            }
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
                        // merge・rebase などの途中なら、ステータスバーに行を出す。
                        let conflicts = snapshot
                            .status
                            .entries
                            .iter()
                            .filter(|e| e.kind == amonhen_model::StatusKind::Unmerged)
                            .count();
                        if let Some((repo, _)) = view.current() {
                            let (bar, a) = (statusbar.clone(), actions.clone());
                            glib::spawn_future_local(async move {
                                let sequence =
                                    sheets::run(&repo, serde_json::json!({ "op": "sequence" }))
                                        .await;
                                let without = if conflicts > 0 {
                                    sheets::run(&repo, serde_json::json!({ "op": "conflictFiles" }))
                                        .await["info"]["files"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .filter(|f| f["markers"].as_u64() == Some(0))
                                        .filter_map(|f| f["path"].as_str().map(str::to_owned))
                                        .collect()
                                } else {
                                    Vec::new()
                                };
                                let info =
                                    sequence["info"].is_object().then_some(&sequence["info"]);
                                a.set_in_sequence(info.is_some());
                                bar.show_sequence(info, conflicts, without, &a);
                            });
                        }
                        working.show(snapshot);
                        if let Some(path) = &args.print_file_menu
                            && let Some(titles) = working.file_menu_titles(path)
                        {
                            println!("{}", titles.join("\n"));
                        }
                        // Working Copy の操作（破棄・コミット）も Undo の記録を書く。
                        actions.refresh_undo();
                        if waiting_status {
                            waiting_status = false;
                            pending.set(pending.get() - 1);
                        }
                    }
                    Event::Note(repo, text) => activity::event(&repo, text),
                    Event::Timing(metric, ms) => {
                        activity::record(&amonhen_model::i18n::translate(metric), ms)
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
fn connect_sidebar(
    sidebar: &Sidebar,
    pages: &gtk::Stack,
    view: &HistoryView,
    stashes: &Rc<Stashes>,
) {
    let (pages, view, stashes) = (pages.downgrade(), view.downgrade(), Rc::downgrade(stashes));
    sidebar.connect_select(move |data| {
        let (Some(pages), Some(view)) = (pages.upgrade(), view.upgrade()) else {
            return;
        };
        match &data.item {
            sidebar::Item::Page(Page::WorkingCopy) => pages.set_visible_child_name("working"),
            sidebar::Item::Page(Page::History) => pages.set_visible_child_name("history"),
            sidebar::Item::Page(Page::Stashes) => pages.set_visible_child_name("stashes"),
            // Stashes の項目の stash は、Stashes の画面で選ぶ（stash のコミットは History にない）。
            sidebar::Item::Ref(e) if e.kind == amonhen_model::RefKind::Stash => {
                if let Some(stashes) = stashes.upgrade() {
                    stashes.select(&e.name);
                    pages.set_visible_child_name("stashes");
                }
                return;
            }
            sidebar::Item::Page(Page::RangeDiff) => pages.set_visible_child_name(rangediff::PAGE),
            sidebar::Item::Page(Page::Reflog) => pages.set_visible_child_name("reflog"),
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
                    let mailmap = repo_t.mailmap();
                    ids.iter()
                        .zip(&texts)
                        .map(|(id, t)| {
                            (
                                id.to_string()[..8].to_owned(),
                                String::from_utf8_lossy(t.subject()).into_owned(),
                                String::from_utf8_lossy(&t.author(mailmap.as_deref()).name)
                                    .into_owned(),
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
            let started = std::time::Instant::now();
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
                    activity::record(&tr!("Load diff"), started.elapsed().as_secs_f64() * 1000.0);
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
    /// Activity の Events に出す出来事（リポジトリの名前、内容）。
    Note(String, String),
    /// Activity の Performance に出す時間（項目、ms）。
    Timing(&'static str, f64),
    Failed(String),
}

fn repo_name(repo: &Repository) -> String {
    repo.paths()
        .workdir
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// History と ref を読み直して知らせる（裏のスレッドで呼ぶ）。
fn reload_history(repo: &Arc<Repository>, tx: &async_channel::Sender<Event>) {
    let started = std::time::Instant::now();
    let loaded = repo.load_history();
    let _ = tx.send_blocking(Event::Timing(
        "Reload History",
        started.elapsed().as_secs_f64() * 1000.0,
    ));
    match loaded {
        Ok(history) => {
            let _ = tx.send_blocking(Event::History(repo.clone(), history));
        }
        Err(e) => {
            let _ = tx.send_blocking(Event::Failed(tr!("Can’t read History: {e}", e = e)));
        }
    }
    if let Ok(refs) = repo.load_refs() {
        let _ = tx.send_blocking(Event::Refs(refs));
    }
}

/// status を読み直して知らせる（裏のスレッドで呼ぶ）。
fn reload_status(repo: &Arc<Repository>, tx: &async_channel::Sender<Event>) {
    let started = std::time::Instant::now();
    let loaded = repo.load_status();
    let _ = tx.send_blocking(Event::Timing(
        "Reload status",
        started.elapsed().as_secs_f64() * 1000.0,
    ));
    match loaded {
        Ok(snapshot) => {
            let _ = tx.send_blocking(Event::Status(snapshot));
        }
        Err(e) => eprintln!("status を読めない: {e}"),
    }
}

/// 裏のスレッドで開いて読み、変化を見張って読み直す。
fn load(path: PathBuf, order: amonhen_model::HistoryOrder, tx: async_channel::Sender<Event>) {
    let repo = match open(&path) {
        Ok(repo) => Arc::new(repo),
        Err(e) => {
            let _ = tx.send_blocking(Event::Failed(tr!("Can’t open the repository: {e}", e = e)));
            return;
        }
    };
    // Settings > Performance: commit-graph がなければ書き出し、あれば増えた分を書き足す（手元のリポジトリだけ）。
    if settings::bool(settings::WRITE_COMMIT_GRAPH) && repo.connection_state().is_none() {
        let repo = repo.clone();
        std::thread::spawn(move || match repo.update_commit_graph() {
            Ok(true) => eprintln!("commit-graph を書き出した"),
            Ok(false) => {}
            Err(e) => eprintln!("commit-graph を書き出せない: {e}"),
        });
    }
    // 並び順は読む前にコアへ（既定の日付順なら何もしない）。
    if order != amonhen_model::HistoryOrder::Date
        && let Err(e) = repo.set_history_order(order)
    {
        eprintln!("History の並び順を変えられない: {e}");
    }
    // 前回の History をすぐ出してから、最新にする。
    if let Some(cached) = repo.load_cached_history() {
        let _ = tx.send_blocking(Event::History(repo.clone(), cached));
    }
    let status = |repo: &Arc<Repository>| reload_status(repo, &tx);
    let reload = |repo: &Arc<Repository>| reload_history(repo, &tx);
    reload(&repo);
    status(&repo);

    // ref が変わったら History を、index か作業ツリーが変わったら status を読み直す。知らせは監視の
    // スレッド（daemon 越しなら daemon の知らせ）から来るので、このスレッドで受けて読む。
    let (changed_tx, changed_rx) = std::sync::mpsc::channel();
    let remote_tx = changed_tx.clone();
    let remote = repo.subscribe_changes(Box::new(move |bits| {
        let _ = remote_tx.send(bits);
    }));
    let _watcher = if remote {
        None
    } else {
        match amonhen_watch::Watcher::new(repo.paths(), move |changes| {
            let _ = changed_tx.send(changes.0);
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("リポジトリを見張れない: {e}");
                return;
            }
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
        let mut what = Vec::new();
        for (bit, name) in [
            (Changes::REFS, "ref".to_owned()),
            (Changes::INDEX, "index".to_owned()),
            (Changes::WORKTREE, tr!("working tree")),
        ] {
            if changes.contains(bit) {
                what.push(name);
            }
        }
        if !what.is_empty() {
            let text = tr!("{what} changed (reloading)", what = what.join(&tr!(", ")));
            let _ = tx.send_blocking(Event::Note(repo_name(&repo), text));
        }
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
                    // 別の窓（Blame など）を開いていれば、そちらを撮る。
                    // その窓にさらにシートが付いていれば、いちばん前のものを撮る。
                    let on = |parent: &gtk::Window| {
                        gtk::Window::list_toplevels()
                            .into_iter()
                            .filter_map(|w| w.downcast::<gtk::Window>().ok())
                            .find(|w| w.is_visible() && w.transient_for().as_ref() == Some(parent))
                    };
                    let mut target: gtk::Window = window.clone().upcast();
                    while let Some(front) = on(&target) {
                        target = front;
                    }
                    match snapshot(&target, &png) {
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
fn window_background(window: &gtk::Window) -> gtk::gdk::RGBA {
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
            Some(p) if p.is_visible() && p.is_mapped() => out.push(p.clone()),
            _ => collect_popovers(&c, out),
        }
        child = c.next_sibling();
    }
}

/// 窓の中身を PNG にする。
fn snapshot(window: &gtk::Window, png: &Path) -> Result<(), String> {
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
