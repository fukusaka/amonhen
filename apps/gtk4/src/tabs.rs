//! タブ: 1 つの窓で複数のリポジトリを扱う（macOS 版の窓のタブ、docs/ux.md の 1）。窓は `gtk::Notebook` を持ち、
//! タブごとにリポジトリの画面のすべて（サイドバー・ページ・ステータスバー）と見出しの帯を持つ。見出しの帯は前にある
//! タブのものを窓に付け替え、窓のアクション（`win.*`）も前にあるタブのものを窓に入れる（ショートカットとメニューは
//! 前のタブに効く）。タブの中の部品は、自分のタブのアクションを引く（後ろのタブでも取り違えない）。
//!
//! 開いた窓とタブの並びは、開く・閉じる・並べ替えるたびに覚え、次の起動で同じ並びに開き直す（`repos::save_open`）。

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

/// リポジトリを開く先。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// 前にあるリポジトリの窓の、新しいタブ（なければ新しい窓）。
    Tab,
    NewWindow,
}

type FrontCallback = Rc<dyn Fn()>;

/// 1 つのリポジトリのタブ。
pub struct Tab {
    /// タブの中身（窓の Notebook のページ）。
    pub root: gtk::Box,
    /// このタブの見出しの帯（前にあるときだけ窓に付ける）。
    pub header: gtk::HeaderBar,
    /// このタブのアクション（`win.*`）。
    pub group: gio::SimpleActionGroup,
    /// リポジトリの名前（タブと窓の名前）。
    pub name: String,
    /// 開いた場所（覚える形。手元のパスは正規化したもの）。
    pub location: String,
    /// 裏で読んでいる数（`--snapshot` は窓のすべてのタブでなくなるまで待つ）。
    pub pending: Rc<Cell<u32>>,
    label: gtk::Label,
    handle: gtk::Box,
    /// 未コミットの変更の数と、上流との差（↑ 先・↓ 後ろ）。
    marks: Cell<(u32, u32, u32)>,
    closed: Cell<bool>,
    on_close: RefCell<Vec<Box<dyn FnOnce()>>>,
    on_front: RefCell<Vec<FrontCallback>>,
    /// 書き込む先の worktree の名前（確認のシートに出す。前にあるときに窓に入れる）。
    worktree_name: RefCell<Option<String>>,
}

impl Tab {
    pub fn new(name: &str, location: &str, pending: Rc<Cell<u32>>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        let header = gtk::HeaderBar::new();
        let group = gio::SimpleActionGroup::new();
        root.insert_action_group("win", Some(&group));
        header.insert_action_group("win", Some(&group));
        let label = gtk::Label::new(Some(name));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_hexpand(true);
        let close = gtk::Button::new();
        close.add_css_class("flat");
        close.add_css_class("circular");
        close.set_tooltip_text(Some(&tr!("Close Tab")));
        close.set_focus_on_click(false);
        if gtk::IconTheme::for_display(&close.display()).has_icon("window-close-symbolic") {
            close.set_icon_name("window-close-symbolic");
        } else {
            close.set_label("×");
        }
        let handle = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        handle.append(&label);
        handle.append(&close);
        let tab = Rc::new(Self {
            root,
            header,
            group,
            name: name.to_owned(),
            location: location.to_owned(),
            pending,
            label,
            handle,
            marks: Cell::new((0, 0, 0)),
            closed: Cell::new(false),
            on_close: RefCell::default(),
            on_front: RefCell::default(),
            worktree_name: RefCell::default(),
        });
        // 前に出した後で足したアクション（メニューの操作は、タブを窓に置いてから入れる）も窓に入れる。
        let weak = Rc::downgrade(&tab);
        tab.group.connect_action_added(None, move |group, name| {
            let Some(tab) = weak.upgrade() else { return };
            let Some(host) = tab.host() else { return };
            let shown = host.shown.borrow().upgrade();
            if shown.is_some_and(|t| Rc::ptr_eq(&t, &tab))
                && let Some(action) = group.lookup_action(name)
            {
                host.window.add_action(&action);
                host.installed.borrow_mut().push(name.to_owned());
            }
        });
        let weak = Rc::downgrade(&tab);
        close.connect_clicked(move |_| {
            if let Some(tab) = weak.upgrade() {
                close_tab(&tab);
            }
        });
        install_actions(&tab);
        tab
    }

    /// タブのある窓。
    pub fn window(&self) -> Option<gtk::ApplicationWindow> {
        self.root.root().and_downcast()
    }

    fn host(&self) -> Option<Rc<RepoWindow>> {
        WINDOWS.with(|w| {
            w.borrow()
                .iter()
                .find(|w| w.tabs.borrow().iter().any(|t| t.root == self.root))
                .cloned()
        })
    }

    /// 窓の前のタブか。
    pub fn is_shown(&self) -> bool {
        self.host()
            .is_some_and(|h| h.current().is_some_and(|t| t.root == self.root))
    }

    /// 前のタブで、窓も前にあるか（後ろのタブ・窓は数え直しや読み直しを後回しにする）。
    pub fn is_front(&self) -> bool {
        self.is_shown() && self.window().is_some_and(|w| w.is_active())
    }

    pub fn closed(&self) -> bool {
        self.closed.get()
    }

    /// リポジトリのページ（リポジトリを開いていないタブ）か。
    pub fn is_home(&self) -> bool {
        self.location.is_empty()
    }

    /// タブを閉じたら `f` を呼ぶ（裏の糸や見張りを止める）。
    pub fn connect_close(&self, f: impl FnOnce() + 'static) {
        self.on_close.borrow_mut().push(Box::new(f));
    }

    /// タブが前に出たら（窓が前に来たときも）`f` を呼ぶ。
    pub fn connect_front(&self, f: impl Fn() + 'static) {
        self.on_front.borrow_mut().push(Rc::new(f));
    }

    fn front(&self) {
        let callbacks: Vec<FrontCallback> = self.on_front.borrow().clone();
        for f in callbacks {
            f();
        }
    }

    /// 未コミットの変更の数（タブの名前に ● を添える）。
    pub fn set_changes(&self, count: u32) {
        let (_, ahead, behind) = self.marks.get();
        self.marks.set((count, ahead, behind));
        self.show_marks();
    }

    /// 上流との差（タブの名前に ↑↓ を添える）。
    pub fn set_tracking(&self, ahead: u32, behind: u32) {
        let (count, _, _) = self.marks.get();
        self.marks.set((count, ahead, behind));
        self.show_marks();
    }

    /// タブの名前に、未コミットの変更（●）と上流との差（↑↓）を添える（macOS 版と同じ）。窓の名前は変えない。
    fn show_marks(&self) {
        let (count, ahead, behind) = self.marks.get();
        let mut title = self.name.clone();
        let mut tips = Vec::new();
        if count > 0 {
            title.push_str(" ●");
            tips.push(tr!("{count} changed files", count = count));
        }
        if ahead > 0 {
            title.push_str(&format!(" ↑{ahead}"));
            tips.push(tr!("{count} commits ahead of upstream", count = ahead));
        }
        if behind > 0 {
            title.push_str(&format!(" ↓{behind}"));
            tips.push(tr!("{count} commits behind upstream", count = behind));
        }
        self.label.set_text(&title);
        let tip = tips.join(&tr!(", "));
        self.label
            .set_tooltip_text((!tip.is_empty()).then_some(tip.as_str()));
    }

    /// 書き込む先の worktree の名前（確認のシートに出す）。
    pub fn set_worktree_name(&self, name: Option<String>) {
        self.worktree_name.replace(name.clone());
        if self.is_shown()
            && let Some(window) = self.window()
        {
            crate::sheets::set_worktree_name(window.upcast_ref(), name);
        }
    }
}

/// リポジトリのタブを並べる窓。
pub struct RepoWindow {
    pub window: gtk::ApplicationWindow,
    notebook: gtk::Notebook,
    tabs: RefCell<Vec<Rc<Tab>>>,
    /// 今、窓に入れているアクションの名前（前のタブを替えたら外す）。
    installed: RefCell<Vec<String>>,
    /// 窓に出しているタブ。
    shown: RefCell<Weak<Tab>>,
}

thread_local! {
    /// 開いているリポジトリの窓（開いた順）。
    static WINDOWS: RefCell<Vec<Rc<RepoWindow>>> = RefCell::default();
    /// 最後に前にあったリポジトリの窓（Repositories・Quick Open から開く先）。
    static LAST: RefCell<glib::WeakRef<gtk::ApplicationWindow>> = RefCell::default();
    /// 確認用の起動（撮る・測る）か、前回のものを開き直している途中: 開いている窓の記録を書き換えない。
    static CHECK_RUN: Cell<bool> = const { Cell::new(false) };
    static RESTORING: Cell<bool> = const { Cell::new(false) };
}

/// 確認用の起動（`--snapshot`・`--measure-scroll`）では、開いている窓とタブを覚えない。
pub fn set_check_run() {
    CHECK_RUN.with(|c| c.set(true));
}

/// 確認用の起動か（開いたリポジトリを Open Recent・ブックマークに覚えない）。
pub fn is_check_run() -> bool {
    CHECK_RUN.with(Cell::get)
}

/// 前回の窓とタブを開き直している間は覚えない（終わったら `save` する）。
pub fn set_restoring(yes: bool) {
    RESTORING.with(|r| r.set(yes));
    if !yes {
        save();
    }
}

impl RepoWindow {
    /// 空の窓を作る（タブは [`add`] で足す）。
    pub fn new(app: &gtk::Application) -> Rc<Self> {
        let notebook = gtk::Notebook::new();
        notebook.set_show_border(false);
        notebook.set_scrollable(true);
        notebook.set_show_tabs(false);
        // タブの帯の「+」（macOS 版と同じく、開くリポジトリを選んで新しいタブに）。
        let plus = gtk::Button::new();
        plus.add_css_class("flat");
        plus.set_tooltip_text(Some(&tr!("New Tab (Ctrl+T)")));
        if gtk::IconTheme::for_display(&plus.display()).has_icon("list-add-symbolic") {
            plus.set_icon_name("list-add-symbolic");
        } else {
            plus.set_label("+");
        }
        plus.set_action_name(Some("win.new-tab"));
        notebook.set_action_widget(&plus, gtk::PackType::End);
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Amon Hen")
            .default_width(1280)
            .default_height(760)
            .child(&notebook)
            .build();
        let this = Rc::new(Self {
            window: window.clone(),
            notebook: notebook.clone(),
            tabs: RefCell::default(),
            installed: RefCell::default(),
            shown: RefCell::default(),
        });
        WINDOWS.with(|w| w.borrow_mut().push(this.clone()));
        let weak = Rc::downgrade(&this);
        notebook.connect_switch_page(move |_, page, _| {
            if let Some(this) = weak.upgrade()
                && let Some(tab) = this.tab_of(page)
            {
                this.show(&tab);
            }
        });
        notebook.connect_page_reordered(|_, _, _| save());
        let weak = Rc::downgrade(&this);
        window.connect_close_request(move |_| {
            if let Some(this) = weak.upgrade() {
                // 閉じる窓のタブは、どれも閉じる（裏の糸と見張りを止める）。
                let tabs: Vec<Rc<Tab>> = this.tabs.borrow_mut().drain(..).collect();
                for tab in tabs {
                    teardown(&tab);
                }
                WINDOWS.with(|w| w.borrow_mut().retain(|w| !Rc::ptr_eq(w, &this)));
                save();
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(&this);
        window.connect_is_active_notify(move |window| {
            if !window.is_active() {
                return;
            }
            LAST.with(|l| l.borrow().set(Some(window)));
            if let Some(tab) = weak.upgrade().and_then(|t| t.current()) {
                tab.front();
            }
        });
        this
    }

    fn tab_of(&self, page: &gtk::Widget) -> Option<Rc<Tab>> {
        self.tabs
            .borrow()
            .iter()
            .find(|t| t.root.upcast_ref::<gtk::Widget>() == page)
            .cloned()
    }

    /// 前のタブ。
    pub fn current(&self) -> Option<Rc<Tab>> {
        let page = self.notebook.nth_page(self.notebook.current_page())?;
        self.tab_of(&page)
    }

    /// タブの並び（Notebook の順）。
    pub fn tabs(&self) -> Vec<Rc<Tab>> {
        (0..self.notebook.n_pages())
            .filter_map(|i| self.notebook.nth_page(Some(i)))
            .filter_map(|page| self.tab_of(&page))
            .collect()
    }

    /// `tab` を前のタブとして窓に出す: 見出しの帯・窓の名前・アクションを、そのタブのものにする。
    fn show(&self, tab: &Rc<Tab>) {
        if self
            .shown
            .borrow()
            .upgrade()
            .is_some_and(|t| Rc::ptr_eq(&t, tab))
        {
            return;
        }
        self.shown.replace(Rc::downgrade(tab));
        // ほかの窓の見出しになっていれば外す（タブを窓の間で移したとき）。
        if let Some(other) = tab.header.root().and_downcast::<gtk::Window>()
            && other != *self.window.upcast_ref::<gtk::Window>()
        {
            other.set_titlebar(None::<&gtk::Widget>);
        }
        self.window.set_titlebar(Some(&tab.header));
        self.window
            .set_title(Some(&format!("{} — Amon Hen", tab.name)));
        let names: Vec<String> = tab
            .group
            .list_actions()
            .iter()
            .map(|n| n.to_string())
            .collect();
        for old in self.installed.borrow().iter() {
            if !names.contains(old) {
                self.window.remove_action(old);
            }
        }
        for name in &names {
            if let Some(action) = tab.group.lookup_action(name) {
                self.window.add_action(&action);
            }
        }
        self.installed.replace(names);
        crate::sheets::set_worktree_name(
            self.window.upcast_ref(),
            tab.worktree_name.borrow().clone(),
        );
        if self.window.is_active() {
            tab.front();
        }
    }

    /// タブの帯は、タブが 2 つ以上のときだけ出す（macOS 版と同じ）。
    fn update_tabs_bar(&self) {
        self.notebook.set_show_tabs(self.notebook.n_pages() > 1);
    }

    /// 窓のすべてのタブで、裏で読んでいるものがないか（`--snapshot` はそれまで待つ）。
    pub fn idle(&self) -> bool {
        self.tabs.borrow().iter().all(|t| t.pending.get() == 0)
    }
}

/// `tab` を `host` の窓に足す。`position` がなければ前のタブの右に。`select` なら前に出す。
pub fn add(host: &Rc<RepoWindow>, tab: &Rc<Tab>, select: bool, position: Option<u32>) {
    let position = position.or_else(|| host.notebook.current_page().map(|p| p + 1));
    host.tabs.borrow_mut().push(tab.clone());
    let index = host
        .notebook
        .insert_page(&tab.root, Some(&tab.handle), position);
    host.notebook.set_tab_reorderable(&tab.root, true);
    // タブは帯の幅を分け合う（macOS 版・GNOME のタブと同じ）。
    host.notebook.page(&tab.root).set_tab_expand(true);
    host.update_tabs_bar();
    if select || host.notebook.n_pages() == 1 {
        host.notebook.set_current_page(Some(index));
        // 最初のページは switch-page が来ないことがある。
        host.show(tab);
    }
    save();
}

/// 窓を作って、`location` を開いたタブを置く。
pub fn new_window_with(app: &gtk::Application, location: PathBuf) -> Rc<RepoWindow> {
    let host = RepoWindow::new(app);
    crate::build_tab(
        app,
        &host,
        crate::Args {
            path: Some(location),
            ..crate::Args::default()
        },
        true,
        None,
    );
    host.window.present();
    host
}

/// リポジトリを開く。`place` が `Tab` なら最後に前にあった窓の新しいタブ（なければ新しい窓）に。すでに開いて
/// いればそのタブを前に出す。`NewWindow` なら、開いていても新しい窓に開く（File > New Window）。
pub fn open(app: &gtk::Application, location: PathBuf, place: Place) {
    if place == Place::NewWindow {
        new_window_with(app, location);
        return;
    }
    // 追加した worktree のフォルダー: メインのリポジトリのタブがあれば、それを前に出してその worktree を書き込む
    // 先にする（なければ、開くときにメインのリポジトリに替える。`build_tab`）。
    let worktree = crate::repos::main_of_worktree(&location);
    let key = crate::repos::normalize(worktree.as_deref().unwrap_or(&location));
    let existing = WINDOWS.with(|w| {
        w.borrow().iter().find_map(|h| {
            h.tabs
                .borrow()
                .iter()
                .find(|t| t.location == key)
                .map(|t| (h.clone(), t.clone()))
        })
    });
    if let Some((host, tab)) = existing {
        if let Some(n) = host.notebook.page_num(&tab.root) {
            host.notebook.set_current_page(Some(n));
        }
        host.window.present();
        if worktree.is_some() {
            let path = location.to_string_lossy().into_owned();
            tab.group
                .activate_action("edit-worktree", Some(&path.to_variant()));
        }
        return;
    }
    match (place, front_window()) {
        (Place::Tab, Some(host)) => {
            open_in(app, &host, location, None);
            host.window.present();
        }
        _ => {
            new_window_with(app, location);
        }
    }
}

/// `host` の窓の新しいタブで開いて、前に出す。
fn open_in(
    app: &gtk::Application,
    host: &Rc<RepoWindow>,
    location: PathBuf,
    position: Option<u32>,
) {
    crate::build_tab(
        app,
        host,
        crate::Args {
            path: Some(location),
            ..crate::Args::default()
        },
        true,
        position,
    );
}

/// 確認用（`--tab`）: `host` の窓に、後ろのタブとして足す。
pub fn add_background(app: &gtk::Application, host: &Rc<RepoWindow>, location: PathBuf) {
    let position = Some(host.notebook.n_pages());
    crate::build_tab(
        app,
        host,
        crate::Args {
            path: Some(location),
            ..crate::Args::default()
        },
        false,
        position,
    );
}

/// リポジトリのページ（リポジトリを開いていないタブ。macOS 版の `RepositoryHome.swift`）を出す。`host` の窓に
/// もうあれば、それを前に出す。`host` がなければ新しい窓に出す。
pub fn show_home(app: &gtk::Application, host: Option<Rc<RepoWindow>>) -> Rc<RepoWindow> {
    if let Some(host) = &host
        && let Some(home) = host.tabs().into_iter().find(|t| t.is_home())
    {
        if let Some(n) = host.notebook.page_num(&home.root) {
            host.notebook.set_current_page(Some(n));
        }
        host.window.present();
        home.front();
        return host.clone();
    }
    let host = host.unwrap_or_else(|| RepoWindow::new(app));
    let tab = Tab::new("Repositories", "", Rc::default());
    let title = gtk::Label::new(Some("Repositories"));
    title.add_css_class("title");
    let subtitle = gtk::Label::new(Some(&tr!("Choose a repository to open")));
    subtitle.add_css_class("subtitle");
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 0);
    titles.set_valign(gtk::Align::Center);
    titles.append(&title);
    titles.append(&subtitle);
    tab.header.set_title_widget(Some(&titles));
    // 主メニュー（リポジトリへの操作はないので、開く・設定・閉じるだけ）。
    let menu = gio::Menu::new();
    let open = gio::Menu::new();
    open.append(Some("Quick Open…"), Some("win.quick-open"));
    open.append(Some("Clone…"), Some("win.clone"));
    open.append_submenu(Some(&tr!("Open Recent")), &crate::repos::recent_menu());
    menu.append_section(None, &open);
    let rest = gio::Menu::new();
    rest.append(Some(&tr!("Settings…")), Some("win.settings"));
    rest.append(Some(&tr!("Close")), Some("win.close"));
    menu.append_section(None, &rest);
    let primary = gtk::MenuButton::builder()
        .menu_model(&menu)
        .tooltip_text("Main Menu")
        .primary(true)
        .build();
    if gtk::IconTheme::for_display(&primary.display()).has_icon("open-menu-symbolic") {
        primary.set_icon_name("open-menu-symbolic");
    } else {
        primary.set_label("≡");
    }
    tab.header.pack_end(&primary);
    // 開いたリポジトリは、このタブの場所に置いて、このタブを閉じる（ページがそのリポジトリになる）。
    let (a, t) = (app.clone(), Rc::downgrade(&tab));
    let open: crate::repobrowser::Open = Rc::new(move |location, place| {
        let (Some(tab), Place::Tab) = (t.upgrade(), place) else {
            return self::open(&a, location, place);
        };
        let Some(host) = tab.host() else {
            return self::open(&a, location, place);
        };
        let key = crate::repos::normalize(&location);
        match host.tabs().into_iter().find(|t| t.location == key) {
            Some(existing) => {
                if let Some(n) = host.notebook.page_num(&existing.root) {
                    host.notebook.set_current_page(Some(n));
                }
            }
            None => {
                let position = host.notebook.page_num(&tab.root);
                open_in(&a, &host, location, position);
            }
        }
        close_tab(&tab);
        host.window.present();
    });
    // Open Recent の項目。
    let recent = gio::SimpleAction::new("open-recent", Some(glib::VariantTy::STRING));
    let o = open.clone();
    recent.connect_activate(move |_, value| {
        if let Some(location) = value.and_then(|v| v.get::<String>()) {
            o(PathBuf::from(location), Place::Tab);
        }
    });
    tab.group.add_action(&recent);
    let browser = crate::repobrowser::page(app, open, &tab.group);
    tab.root.append(&browser.root);
    let b = Rc::downgrade(&browser);
    tab.connect_front(move || {
        if let Some(b) = b.upgrade() {
            b.focus_filter();
        }
    });
    // タブを閉じるまでページを持つ。
    let keep = RefCell::new(Some(browser));
    tab.connect_close(move || {
        keep.borrow_mut().take();
    });
    add(&host, &tab, true, None);
    host.window.present();
    host
}

/// 前回の窓を開き直す: 窓ごとに、最初の場所で窓を作り、残りをタブで足す（前のタブは最初のもの）。
pub fn restore(app: &gtk::Application, groups: Vec<Vec<String>>) {
    set_restoring(true);
    for group in groups {
        let mut it = group.into_iter();
        let Some(first) = it.next() else { continue };
        let host = new_window_with(app, first.into());
        for location in it {
            add_background(app, &host, location.into());
        }
    }
    set_restoring(false);
}

/// 前のタブを、同じ場所に開き直したタブに替える（daemon・SSH 越しのつながりが切れたとき）。
pub fn reopen(tab: &Rc<Tab>, location: PathBuf) {
    let (Some(host), Some(window)) = (tab.host(), tab.window()) else {
        return;
    };
    let Some(app) = window.application() else {
        return;
    };
    let position = host.notebook.page_num(&tab.root).map(|n| n + 1);
    open_in(&app, &host, location, position);
    close_tab(tab);
}

/// ツールバーのリポジトリ名から: `tab` を `location` のリポジトリに切り替える（同じ場所に開き直して、`tab` を閉じる）。
/// もうどこかのタブで開いていれば、そのタブを前に出す（`tab` は残す。macOS 版と同じ）。
pub fn switch(tab: &Rc<Tab>, location: PathBuf) {
    let key = crate::repos::normalize(&location);
    let existing = WINDOWS.with(|w| {
        w.borrow().iter().find_map(|h| {
            h.tabs
                .borrow()
                .iter()
                .find(|t| t.location == key)
                .map(|t| (h.clone(), t.clone()))
        })
    });
    match existing {
        Some((host, t)) => {
            if let Some(n) = host.notebook.page_num(&t.root) {
                host.notebook.set_current_page(Some(n));
            }
            host.window.present();
        }
        None => reopen(tab, location),
    }
}

/// 最後に前にあったリポジトリの窓（なければ最初の窓）。
pub fn front_window() -> Option<Rc<RepoWindow>> {
    let last = LAST.with(|l| l.borrow().upgrade());
    WINDOWS.with(|w| {
        let w = w.borrow();
        last.and_then(|last| w.iter().find(|h| h.window == last).cloned())
            .or_else(|| w.first().cloned())
    })
}

/// `window` の窓（リポジトリの窓なら）。
pub fn host_of(window: &gtk::ApplicationWindow) -> Option<Rc<RepoWindow>> {
    WINDOWS.with(|w| w.borrow().iter().find(|h| h.window == *window).cloned())
}

/// 閉じたタブの後始末: 裏の糸と見張りを止め、アクションを手放す（閉じたタブの部品が互いを持ち合って残らない
/// ように）。
fn teardown(tab: &Rc<Tab>) {
    if tab.closed.replace(true) {
        return;
    }
    let callbacks: Vec<Box<dyn FnOnce()>> = tab.on_close.borrow_mut().drain(..).collect();
    for f in callbacks {
        f();
    }
    tab.on_front.borrow_mut().clear();
    for name in tab.group.list_actions() {
        tab.group.remove_action(&name);
    }
}

/// タブを閉じる。窓の最後のタブなら窓を閉じる。
pub fn close_tab(tab: &Rc<Tab>) {
    let Some(host) = tab.host() else { return };
    if host.tabs.borrow().len() <= 1 {
        host.window.close();
        return;
    }
    detach(&host, tab);
    teardown(tab);
    save();
}

/// `tab` を `host` の窓から外す（閉じない）。
fn detach(host: &Rc<RepoWindow>, tab: &Rc<Tab>) {
    host.tabs.borrow_mut().retain(|t| !Rc::ptr_eq(t, tab));
    if let Some(n) = host.notebook.page_num(&tab.root) {
        host.notebook.remove_page(Some(n));
    }
    if host
        .shown
        .borrow()
        .upgrade()
        .is_some_and(|t| Rc::ptr_eq(&t, tab))
    {
        host.shown.replace(Weak::new());
        if let Some(next) = host.current() {
            host.show(&next);
        }
    }
    host.update_tabs_bar();
}

/// タブを `target` の窓に移す（後ろに足して前に出す）。
fn move_tab(tab: &Rc<Tab>, target: &Rc<RepoWindow>, select: bool) {
    let Some(source) = tab.host() else { return };
    if Rc::ptr_eq(&source, target) {
        return;
    }
    detach(&source, tab);
    let position = Some(target.notebook.n_pages());
    add(target, tab, select, position);
    if source.tabs.borrow().is_empty() {
        source.window.close();
    }
}

/// 窓の Window のタブの操作（`win.*`）。どのタブにも同じ名前で入れ、前のタブのものが窓に入る。
fn install_actions(tab: &Rc<Tab>) {
    let add = |name: &str, f: fn(&Rc<Tab>)| {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(tab);
        action.connect_activate(move |_, _| {
            if let Some(tab) = weak.upgrade() {
                f(&tab);
            }
        });
        tab.group.add_action(&action);
    };
    // File > New Tab（Ctrl+T）・タブの帯の「+」: この窓に、リポジトリのページのタブを出す（あればそれを前に。
    // macOS 版と同じ）。
    add("new-tab", |tab| {
        let (Some(window), Some(host)) = (tab.window(), tab.host()) else {
            return;
        };
        if let Some(app) = window.application() {
            show_home(&app, Some(host));
        }
    });
    // File > Close（Ctrl+W）: タブを閉じる（最後のタブなら窓を閉じる。macOS 版と同じ）。
    add("close", close_tab);
    add("close-window", |tab| {
        if let Some(window) = tab.window() {
            window.close();
        }
    });
    add("next-tab", |tab| step(tab, 1));
    add("previous-tab", |tab| step(tab, -1));
    add("move-tab-to-new-window", |tab| {
        let (Some(host), Some(window)) = (tab.host(), tab.window()) else {
            return;
        };
        let Some(app) = window.application() else {
            return;
        };
        if host.tabs.borrow().len() <= 1 {
            return window.error_bell();
        }
        let target = RepoWindow::new(&app);
        move_tab(tab, &target, true);
        target.window.present();
        save();
    });
    add("merge-all-windows", |tab| {
        let Some(host) = tab.host() else { return };
        let others: Vec<Rc<RepoWindow>> = WINDOWS.with(|w| {
            w.borrow()
                .iter()
                .filter(|w| !Rc::ptr_eq(w, &host))
                .cloned()
                .collect()
        });
        for other in others {
            for t in other.tabs() {
                move_tab(&t, &host, false);
            }
        }
        host.window.present();
        save();
    });
}

/// 前のタブを `delta` だけ隣に（端からは反対の端へ）。
fn step(tab: &Rc<Tab>, delta: i32) {
    let Some(host) = tab.host() else { return };
    let n = host.notebook.n_pages() as i32;
    if n <= 1 {
        return;
    }
    let current = host.notebook.current_page().unwrap_or(0) as i32;
    let next = (current + delta).rem_euclid(n) as u32;
    host.notebook.set_current_page(Some(next));
}

/// 開いている窓とタブの並びを覚える（次の起動で開き直す）。
fn save() {
    if CHECK_RUN.with(Cell::get) || RESTORING.with(Cell::get) {
        return;
    }
    let groups: Vec<Vec<String>> = WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .map(|h| {
                h.tabs()
                    .iter()
                    .map(|t| t.location.clone())
                    // リポジトリのページは覚えない。
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|g| !g.is_empty())
            .collect()
    });
    crate::repos::save_open(&groups);
}

/// 窓のタブの操作のショートカット（アプリに 1 度だけ）。
pub fn install_accels(app: &gtk::Application) {
    for (action, accels) in [
        ("win.new-tab", &["<Control>t"][..]),
        ("win.close-window", &["<Control><Shift>w"][..]),
        ("win.next-tab", &["<Control>Page_Down", "<Control>Tab"][..]),
        (
            "win.previous-tab",
            &["<Control>Page_Up", "<Control><Shift>Tab"][..],
        ),
    ] {
        app.set_accels_for_action(action, accels);
    }
}
