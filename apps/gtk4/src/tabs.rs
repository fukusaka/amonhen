//! タブ: 1 つの窓で複数のリポジトリを扱う（macOS 版の窓のタブ、docs/ux.md の 1）。窓は libadwaita の `TabView`
//! と `TabBar` を持ち（GtkNotebook は Wayland で窓の外に落としたタブを窓にできない）、
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
    /// タブの中身（窓の TabView のページ）。
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
    /// タブの見出しと、その説明（印を添えたもの。ページに置く）。
    title: RefCell<(String, Option<String>)>,
    /// 未コミットの変更の数と、上流との差（↑ 先・↓ 後ろ）。
    marks: Cell<(u32, u32, u32)>,
    closed: Cell<bool>,
    on_close: RefCell<Vec<Box<dyn FnOnce()>>>,
    on_front: RefCell<Vec<FrontCallback>>,
    /// 前のタブでなくなったら呼ぶ（裏になった History のメモリを絞る）。
    on_back: RefCell<Vec<FrontCallback>>,
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
        let tab = Rc::new(Self {
            root,
            header,
            group,
            name: name.to_owned(),
            location: location.to_owned(),
            pending,
            title: RefCell::new((name.to_owned(), None)),
            marks: Cell::new((0, 0, 0)),
            closed: Cell::new(false),
            on_close: RefCell::default(),
            on_front: RefCell::default(),
            on_back: RefCell::default(),
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

    /// 窓の TabView の中の、このタブのページ。
    fn page(&self) -> Option<adw::TabPage> {
        let host = self.host()?;
        page_of(&host.view, &self.root)
    }

    /// 見出しと説明をページに置く。
    fn apply_title(&self) {
        if let Some(page) = self.page() {
            let (title, tip) = &*self.title.borrow();
            page.set_title(title);
            page.set_tooltip(tip.as_deref().unwrap_or(""));
        }
    }

    pub fn closed(&self) -> bool {
        self.closed.get()
    }

    /// リポジトリのページ（リポジトリを開いていないタブ）か。
    pub fn is_home(&self) -> bool {
        self.location.is_empty()
    }

    /// `key`（`repos::normalize` した場所）を開いているタブか（場所は開いた形なので、リンクを解いて比べる）。
    pub fn is_at(&self, key: &str) -> bool {
        !self.is_home() && crate::repos::normalize(std::path::Path::new(&self.location)) == key
    }

    /// タブを閉じたら `f` を呼ぶ（裏の糸や見張りを止める）。
    pub fn connect_close(&self, f: impl FnOnce() + 'static) {
        self.on_close.borrow_mut().push(Box::new(f));
    }

    /// タブが前に出たら（窓が前に来たときも）`f` を呼ぶ。
    pub fn connect_front(&self, f: impl Fn() + 'static) {
        self.on_front.borrow_mut().push(Rc::new(f));
    }

    /// 窓の中でほかのタブが前に出て、このタブが後ろになったら `f` を呼ぶ。
    pub fn connect_back(&self, f: impl Fn() + 'static) {
        self.on_back.borrow_mut().push(Rc::new(f));
    }

    fn back(&self) {
        let callbacks: Vec<FrontCallback> = self.on_back.borrow().clone();
        for f in callbacks {
            f();
        }
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
        let tip = tips.join(&tr!(", "));
        self.title
            .replace((title, (!tip.is_empty()).then_some(tip)));
        self.apply_title();
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
    /// 窓の番号（Window > Move Tab to Window の移す先を指す）。
    id: u32,
    view: adw::TabView,
    bar: adw::TabBar,
    /// 閉じるのを認めたページ（`close-page` で、こちらから閉じたものは止めない）。
    closing: RefCell<Option<adw::TabPage>>,
    tabs: RefCell<Vec<Rc<Tab>>>,
    /// 今、窓に入れているアクションの名前（前のタブを替えたら外す）。
    installed: RefCell<Vec<String>>,
    /// 窓に出しているタブ。
    shown: RefCell<Weak<Tab>>,
    /// タブが 1 つでもタブの帯を出す（View > Show Tab Bar。帯のタブをつかんでほかの窓に移せる。macOS 版と同じ）。
    tab_bar: Cell<bool>,
}

thread_local! {
    /// 開いているリポジトリの窓（開いた順）。
    static WINDOWS: RefCell<Vec<Rc<RepoWindow>>> = RefCell::default();
    /// 次の窓の番号。
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
    /// Window > Move Tab to Window の中身（メニューのある部品と、その中身。窓とタブが変わったら作り直す）。
    static MOVE_MENUS: RefCell<Vec<(glib::WeakRef<gtk::Widget>, gio::Menu)>> = RefCell::default();
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

/// 前回の窓とタブを開き直している途中か。
pub fn is_restoring() -> bool {
    RESTORING.with(Cell::get)
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
        let view = adw::TabView::new();
        view.set_vexpand(true);
        keep_text_shortcuts(&view);
        let bar = adw::TabBar::new();
        bar.set_view(Some(&view));
        // タブが 1 つなら帯を隠す（View > Show Tab Bar で出す）。
        bar.set_autohide(true);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&bar);
        body.append(&view);
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
        bar.set_end_action_widget(Some(&plus));
        // タブはつかんでほかの窓の帯に落とせる。窓の外に落とすと新しい窓になる（macOS 版のタブと同じ。TabView は
        // どの窓のものとも運び合える）。
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Amon Hen")
            .default_width(1280)
            .default_height(760)
            .child(&body)
            .build();
        let id = NEXT_ID.with(|n| n.replace(n.get() + 1));
        let this = Rc::new(Self {
            window: window.clone(),
            id,
            view: view.clone(),
            bar: bar.clone(),
            closing: RefCell::default(),
            tabs: RefCell::default(),
            installed: RefCell::default(),
            shown: RefCell::default(),
            tab_bar: Cell::new(false),
        });
        WINDOWS.with(|w| w.borrow_mut().push(this.clone()));
        // 窓の大きさは、そのリポジトリごとに覚える（macOS 版の窓の frame と同じ。GNOME では位置は決められない
        // ので大きさだけ）。変えている間は書かず、止まってから。
        let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        let weak = Rc::downgrade(&this);
        let remember = move |_: &gtk::ApplicationWindow| {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let (w, p) = (weak.clone(), pending.clone());
            let id =
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    p.borrow_mut().take();
                    if let Some(this) = w.upgrade() {
                        this.save_size();
                    }
                });
            pending.borrow_mut().replace(id);
        };
        let r = remember.clone();
        window.connect_default_width_notify(move |w| r(w));
        window.connect_default_height_notify(move |w| remember(w));
        let weak = Rc::downgrade(&this);
        view.connect_selected_page_notify(move |view| {
            if let Some(this) = weak.upgrade()
                && let Some(page) = view.selected_page()
                && let Some(tab) = this.tab_of(&page.child())
            {
                this.show(&tab);
            }
        });
        view.connect_page_reordered(|_, _, _| save());
        // ほかの窓からタブを落とされた（つかんで運ばれた）: そのタブを、元の窓からこの窓のものにする。
        let weak = Rc::downgrade(&this);
        view.connect_page_attached(move |_, page, _| {
            if let Some(this) = weak.upgrade() {
                this.adopt(&page.child());
            }
        });
        // 帯の × と Ctrl+W: こちらの閉じ方で閉じる（最後のタブなら窓を閉じる・リポジトリのページを出す）。
        let weak = Rc::downgrade(&this);
        view.connect_close_page(move |view, page| {
            let Some(this) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if this.closing.borrow().as_ref() == Some(page) {
                return glib::Propagation::Proceed;
            }
            view.close_page_finish(page, false);
            if let Some(tab) = this.tab_of(&page.child()) {
                glib::idle_add_local_once(move || close_tab(&tab));
            }
            glib::Propagation::Stop
        });
        // 窓の外に落とされた: 新しい窓を作り、そこに入れる。
        view.connect_create_window(|view| {
            let app = view
                .root()
                .and_downcast::<gtk::ApplicationWindow>()
                .and_then(|w| w.application())?;
            let target = RepoWindow::new(&app);
            target.window.present();
            Some(target.view.clone())
        });
        let weak = Rc::downgrade(&this);
        window.connect_close_request(move |_| {
            if let Some(this) = weak.upgrade() {
                // 閉じる窓のタブは、どれも閉じる（裏の糸と見張りを止める）。
                let tabs: Vec<Rc<Tab>> = this.tabs.borrow_mut().drain(..).collect();
                for tab in tabs {
                    teardown(&tab);
                }
                WINDOWS.with(|w| w.borrow_mut().retain(|w| !Rc::ptr_eq(w, &this)));
                // 最後の窓を閉じるのはアプリを終えること（GNOME のふつうの終え方）。開いていた並びは消さずに残す
                // （macOS 版も終えるときは記録を消さない）。
                if WINDOWS.with(|w| !w.borrow().is_empty()) {
                    save();
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(&this);
        window.connect_is_active_notify(move |window| {
            if !window.is_active() {
                return;
            }
            LAST.with(|l| l.borrow().set(Some(window)));
            // ほかのプロセスがブックマークを書き換えていたら読み直す（変わっていたときだけ一覧が描き直す。
            // macOS 版はアプリが前に来たときに読み直す）。
            crate::bookmarks::refresh();
            if let Some(tab) = weak.upgrade().and_then(|t| t.current()) {
                tab.front();
            }
        });
        this
    }

    /// 窓の大きさを、窓のタブのリポジトリごとに覚える（最大化しているときは覚えない）。
    fn save_size(&self) {
        if CHECK_RUN.with(Cell::get) || self.window.is_maximized() {
            return;
        }
        let (width, height) = self.window.default_size();
        if width <= 0 || height <= 0 {
            return;
        }
        let mut sizes = window_sizes();
        for tab in self.tabs() {
            if !tab.location.is_empty() {
                let key = crate::repos::normalize(std::path::Path::new(&tab.location));
                sizes.insert(key, serde_json::json!([width, height]));
            }
        }
        crate::settings::set_string(WINDOW_SIZES, &serde_json::Value::Object(sizes).to_string());
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
        let page = self.view.selected_page()?;
        self.tab_of(&page.child())
    }

    /// タブの並び（TabView の順）。
    pub fn tabs(&self) -> Vec<Rc<Tab>> {
        (0..self.view.n_pages())
            .filter_map(|i| self.tab_of(&self.view.nth_page(i).child()))
            .collect()
    }

    /// `tab` を前のタブにする。
    fn select(&self, tab: &Tab) {
        if let Some(page) = page_of(&self.view, &tab.root) {
            self.view.set_selected_page(&page);
        }
    }

    /// `tab` の位置。
    fn position(&self, tab: &Tab) -> Option<i32> {
        page_of(&self.view, &tab.root).map(|p| self.view.page_position(&p))
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
        let old = self.shown.replace(Rc::downgrade(tab)).upgrade();
        // 移す先の名前は前のタブの名前（Window > Move Tab to Window）。
        glib::idle_add_local_once(refresh_move_menus);
        // 後ろになったタブ（少し後で。切り替えの途中で重い仕事をしない）。
        if let Some(old) = old.filter(|t| !t.closed()) {
            glib::idle_add_local_once(move || {
                if !old.is_shown() && !old.closed() {
                    old.back();
                }
            });
        }
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

    /// タブの帯は、タブが 2 つ以上のときか、View > Show Tab Bar で出したときだけ出す（macOS 版と同じ）。帯を
    /// 隠せるのはタブが 1 つのときだけ。
    fn update_tabs_bar(&self) {
        let alone = self.view.n_pages() <= 1;
        self.bar.set_autohide(!self.tab_bar.get());
        for tab in self.tabs.borrow().iter() {
            if let Some(action) = tab
                .group
                .lookup_action("toggle-tab-bar")
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(alone);
            }
        }
    }

    /// ほかの窓から運ばれてきたページを、この窓のタブにする（元の窓からは外す。空になった窓は閉じる）。
    fn adopt(self: &Rc<Self>, page: &gtk::Widget) {
        if self.tab_of(page).is_some() {
            return;
        }
        let Some((source, tab)) = WINDOWS.with(|w| {
            w.borrow().iter().find_map(|h| {
                let tab = h.tab_of(page)?;
                Some((h.clone(), tab))
            })
        }) else {
            return;
        };
        source.tabs.borrow_mut().retain(|t| !Rc::ptr_eq(t, &tab));
        if source
            .shown
            .borrow()
            .upgrade()
            .is_some_and(|t| Rc::ptr_eq(&t, &tab))
        {
            source.shown.replace(Weak::new());
            if let Some(next) = source.current() {
                source.show(&next);
            }
        }
        source.update_tabs_bar();
        self.tabs.borrow_mut().push(tab.clone());
        tab.apply_title();
        self.select(&tab);
        self.show(&tab);
        self.update_tabs_bar();
        self.window.present();
        if source.tabs.borrow().is_empty() {
            source.window.close();
        }
        save();
    }

    /// 窓のすべてのタブで、裏で読んでいるものがないか（`--snapshot` はそれまで待つ）。
    pub fn idle(&self) -> bool {
        self.tabs.borrow().iter().all(|t| t.pending.get() == 0)
    }
}

/// `tab` を `host` の窓に足す。`position` がなければ前のタブの右に。`select` なら前に出す。
pub fn add(host: &Rc<RepoWindow>, tab: &Rc<Tab>, select: bool, position: Option<u32>) {
    let position = position
        .map(|p| p as i32)
        .or_else(|| {
            host.view
                .selected_page()
                .map(|p| host.view.page_position(&p) + 1)
        })
        .unwrap_or(host.view.n_pages())
        .min(host.view.n_pages());
    host.tabs.borrow_mut().push(tab.clone());
    let page = host.view.insert(&tab.root, position);
    tab.apply_title();
    host.update_tabs_bar();
    if select || host.view.n_pages() == 1 {
        host.view.set_selected_page(&page);
        // 最初のページは選んだ知らせが来ないことがある。
        host.show(tab);
    }
    save();
}

/// Window > Move Tab to Window ▸ の中身を作る（`anchor` はメニューを出すタブの部品。その窓は移す先から除く）。
/// 窓とタブが変わるたびに作り直す。
pub fn move_menu(anchor: &impl IsA<gtk::Widget>) -> gio::Menu {
    let menu = gio::Menu::new();
    let anchor = anchor.upcast_ref::<gtk::Widget>().downgrade();
    fill_move_menu(&anchor, &menu);
    MOVE_MENUS.with(|m| m.borrow_mut().push((anchor, menu.clone())));
    menu
}

/// 移す先: 前の窓とは別の窓ごとに 1 つ。その窓で前に出ているタブの名前と、ほかのタブの数で出す（macOS 版と同じ）。
fn fill_move_menu(anchor: &glib::WeakRef<gtk::Widget>, menu: &gio::Menu) {
    menu.remove_all();
    let own = anchor
        .upgrade()
        .and_then(|a| a.root())
        .and_downcast::<gtk::ApplicationWindow>()
        .and_then(|w| host_of(&w));
    let others: Vec<Rc<RepoWindow>> = WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .filter(|h| own.as_ref().is_none_or(|o| !Rc::ptr_eq(o, h)))
            .cloned()
            .collect()
    });
    for host in others {
        let Some(shown) = host.current() else {
            continue;
        };
        let n = host.tabs.borrow().len();
        let title = if n > 1 {
            tr!("{name} (+{count})", name = shown.name, count = n - 1)
        } else {
            shown.name.clone()
        };
        let item = gio::MenuItem::new(Some(&title), None);
        item.set_action_and_target_value(
            Some("win.move-tab-to-window"),
            Some(&host.id.to_variant()),
        );
        menu.append_item(&item);
    }
    if menu.n_items() == 0 {
        // 押せない「None」（アクションのない項目）。
        menu.append(Some(&tr!("None")), Some("win.move-tab-to-window-none"));
    }
}

/// どの Move Tab to Window ▸ も作り直す（部品がなくなったものは外す）。
fn refresh_move_menus() {
    let menus: Vec<(glib::WeakRef<gtk::Widget>, gio::Menu)> = MOVE_MENUS.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|(a, _)| a.upgrade().is_some());
        m.clone()
    });
    for (anchor, menu) in menus {
        fill_move_menu(&anchor, &menu);
    }
}

/// View > Show Tab Bar の今の名前（帯を出していれば Hide Tab Bar。macOS 版と同じく、メニューを開くときに合わせる）。
pub fn tab_bar_title(window: &gtk::ApplicationWindow) -> String {
    let shown = host_of(window).is_some_and(|h| h.bar.is_tabs_revealed());
    if shown {
        tr!("Hide Tab Bar")
    } else {
        tr!("Show Tab Bar")
    }
}

/// 窓の大きさ（場所 → `[幅, 高さ]` の JSON）。
const WINDOW_SIZES: &str = "windowSizes";

fn window_sizes() -> serde_json::Map<String, serde_json::Value> {
    crate::settings::string(WINDOW_SIZES)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 窓を作って、`location` を開いたタブを置く。前にその場所の窓を開いていたら、その大きさで（macOS 版と同じ）。
pub fn new_window_with(app: &gtk::Application, location: PathBuf) -> Rc<RepoWindow> {
    let host = RepoWindow::new(app);
    let key = crate::repos::normalize(&location);
    // 確認用の起動（撮る・計測する）は、いつも同じ大きさで。
    if !CHECK_RUN.with(Cell::get)
        && let Some(size) = window_sizes().get(&key).and_then(|v| v.as_array().cloned())
        && let [w, h] = size.as_slice()
        && let (Some(w), Some(h)) = (w.as_i64(), h.as_i64())
    {
        host.window
            .set_default_size(w.max(400) as i32, h.max(300) as i32);
    }
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

/// `location` を開いているタブ（どの窓でも。追加した worktree なら、メインのリポジトリのタブ）。
fn find_open(location: &std::path::Path) -> Option<(Rc<RepoWindow>, Rc<Tab>)> {
    let worktree = crate::repos::main_of_worktree(location);
    let key = crate::repos::normalize(&crate::repos::through_daemon(
        worktree.as_deref().unwrap_or(location),
    ));
    WINDOWS.with(|w| {
        w.borrow().iter().find_map(|h| {
            h.tabs
                .borrow()
                .iter()
                .find(|t| t.is_at(&key))
                .map(|t| (h.clone(), t.clone()))
        })
    })
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
    if let Some((host, tab)) = find_open(&location) {
        host.select(&tab);
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
            open_in(app, &host, location, None, None);
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
    page: Option<crate::sidebar::Page>,
) {
    crate::build_tab(
        app,
        host,
        crate::Args {
            path: Some(location),
            page,
            ..crate::Args::default()
        },
        true,
        position,
    );
}

/// 確認用（`--tab`）: `host` の窓に、後ろのタブとして足す。
pub fn add_background(app: &gtk::Application, host: &Rc<RepoWindow>, location: PathBuf) {
    // 追加した worktree のフォルダーはメインのリポジトリとして開くので、そのタブがもうあれば足さない（前の窓を
    // 戻すとき、worktree とメインの両方が記録されていることがある）。
    let worktree = crate::repos::main_of_worktree(&location);
    let key = crate::repos::normalize(&crate::repos::through_daemon(
        worktree.as_deref().unwrap_or(&location),
    ));
    let open = WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .any(|h| h.tabs.borrow().iter().any(|t| t.is_at(&key)))
    });
    if open {
        return;
    }
    let position = Some(host.view.n_pages() as u32);
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
        host.select(&home);
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
    // 見出しはリポジトリのタブと同じ Repository のメニュー・ツールバー・主メニュー（リポジトリへの操作は、このタブに
    // アクションがないので押せない。macOS 版で Repositories の窓が前にあっても、メニューバーとツールバーはそのままなの
    // と同じ）。History の表示は設定なので、ここでも変えられる。
    crate::historydisplay::install_actions(&tab.group);
    let primary =
        crate::appmenu::install_home(&tab.header, &tab.group, &tab.root, titles.upcast_ref());
    // 開くときに、View > Show Tab Bar の名前を合わせる（リポジトリのタブと同じく、帯を出していれば Hide Tab Bar）。
    crate::appmenu::retitle_on_show_with(&primary, |button, model| {
        let Some(window) = button.root().and_downcast::<gtk::ApplicationWindow>() else {
            return;
        };
        crate::menu::retitle(model, &|action| {
            (action == "win.toggle-tab-bar").then(|| tab_bar_title(&window))
        });
    });
    // 開いたリポジトリは、このタブの場所に置いて、このタブを閉じる（ページがそのリポジトリになる）。
    let (a, t) = (app.clone(), Rc::downgrade(&tab));
    let open: crate::repobrowser::Open = Rc::new(move |location, place| {
        let (Some(tab), Place::Tab) = (t.upgrade(), place) else {
            return self::open(&a, location, place);
        };
        let Some(host) = tab.host() else {
            return self::open(&a, location, place);
        };
        // もう開いていれば（ほかの窓でも）そのタブを前に出し、このページは残す。追加した worktree はメインの
        // リポジトリのタブで開く（`open` と同じ。macOS 版と同じ）。
        if find_open(&location).is_some() {
            return self::open(&a, location, place);
        }
        let position = host.position(&tab).map(|p| p as u32);
        open_in(&a, &host, location, position, None);
        close_tab(&tab);
        host.window.present();
    });
    // Open Recent の項目。ページは残して、新しいタブに開く（macOS 版は File > Open Recent からページを置き換えない）。
    let recent = gio::SimpleAction::new("open-recent", Some(glib::VariantTy::STRING));
    let a = app.clone();
    recent.connect_activate(move |_, value| {
        if let Some(location) = value.and_then(|v| v.get::<String>()) {
            self::open(&a, PathBuf::from(location), Place::Tab);
        }
    });
    tab.group.add_action(&recent);
    // File > New Window・Open…・Keyboard Shortcuts・About（macOS 版ではどの窓からも効く）。New Window は新しい窓に
    // このページを、Open… はこのページを出す（macOS 版と同じ）。
    let add_action = |name: &str, f: fn(&Rc<Tab>, &gtk::Application)| {
        let action = gio::SimpleAction::new(name, None);
        let (t, a) = (Rc::downgrade(&tab), app.downgrade());
        action.connect_activate(move |_, _| {
            if let (Some(tab), Some(app)) = (t.upgrade(), a.upgrade()) {
                f(&tab, &app);
            }
        });
        tab.group.add_action(&action);
    };
    add_action("new-window", |_, app| {
        show_home(app, None);
    });
    add_action("open", |tab, app| {
        show_home(app, tab.host());
    });
    add_action("shortcuts", |tab, _| {
        if let Some(window) = tab.window() {
            crate::appmenu::show_shortcuts(&window);
        }
    });
    add_action("about", |tab, _| {
        if let Some(window) = tab.window() {
            crate::appmenu::show_about(&window);
        }
    });
    // Activity（Ctrl+Alt+A。macOS 版ではどの窓からも開ける）。このページにはリポジトリがないので、History の
    // タブは開いているリポジトリの最初のものを選ぶ。
    let activity = gio::SimpleAction::new("activity", None);
    let t = Rc::downgrade(&tab);
    activity.connect_activate(move |_, _| {
        if let Some(window) = t.upgrade().and_then(|t| t.window()) {
            crate::activity::Activity::show(&window, None, crate::engine());
        }
    });
    tab.group.add_action(&activity);
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
        // 前に出すのは最後のタブ（macOS 版はタブを順に開いて前に出していくので、最後のものが残る）。
        let n = host.view.n_pages();
        if n > 1 {
            host.view.set_selected_page(&host.view.nth_page(n - 1));
        }
    }
    set_restoring(false);
}

/// 前のタブを、同じ場所に開き直したタブに替える（daemon・SSH 越しのつながりが切れたとき）。
/// `page` は開き直したタブで出すページ（つなぎ直すときの、それまで出していたページ。None なら History）。
pub fn reopen(tab: &Rc<Tab>, location: PathBuf, page: Option<crate::sidebar::Page>) {
    let (Some(host), Some(window)) = (tab.host(), tab.window()) else {
        return;
    };
    let Some(app) = window.application() else {
        return;
    };
    let position = host.position(tab).map(|n| n as u32 + 1);
    open_in(&app, &host, location, position, page);
    close_tab(tab);
}

/// ツールバーのリポジトリ名から: `tab` を `location` のリポジトリに切り替える（同じ場所に開き直して、`tab` を閉じる）。
/// もうどこかのタブで開いていれば、そのタブを前に出す（`tab` は残す。macOS 版と同じ）。
pub fn switch(tab: &Rc<Tab>, location: PathBuf) {
    let key = crate::repos::normalize(&crate::repos::through_daemon(&location));
    let existing = WINDOWS.with(|w| {
        w.borrow().iter().find_map(|h| {
            h.tabs
                .borrow()
                .iter()
                .find(|t| t.is_at(&key))
                .map(|t| (h.clone(), t.clone()))
        })
    });
    match existing {
        Some((host, t)) => {
            host.select(&t);
            host.window.present();
        }
        None => reopen(tab, location, None),
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

/// タブを閉じる。窓の最後のタブなら窓を閉じる。ただし、ほかに窓がなくて最後のリポジトリを閉じるときは、終わらずに
/// リポジトリのページを出す（macOS 版と同じ）。
pub fn close_tab(tab: &Rc<Tab>) {
    let Some(host) = tab.host() else { return };
    if host.tabs.borrow().len() <= 1 {
        let alone = WINDOWS.with(|w| w.borrow().len() <= 1);
        match tab.window().and_then(|w| w.application()) {
            Some(app) if alone && !tab.is_home() => {
                show_home(&app, Some(host.clone()));
            }
            _ => {
                host.window.close();
                return;
            }
        }
    }
    detach(&host, tab);
    teardown(tab);
    save();
}

/// `tab` を `host` の窓から外す（閉じない）。
fn detach(host: &Rc<RepoWindow>, tab: &Rc<Tab>) {
    host.tabs.borrow_mut().retain(|t| !Rc::ptr_eq(t, tab));
    if let Some(page) = page_of(&host.view, &tab.root) {
        host.closing.replace(Some(page.clone()));
        host.view.close_page(&page);
        host.closing.replace(None);
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
    // ページを運ぶ（中身を作り直さない）。運ばれた先の窓が、`page-attached` で自分のタブにする。
    let Some(page) = page_of(&source.view, &tab.root) else {
        return;
    };
    source
        .view
        .transfer_page(&page, &target.view, target.view.n_pages());
    if select {
        target.select(tab);
        target.show(tab);
    }
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
    // File > Close（Ctrl+W）: タブを閉じる（最後のタブなら窓を閉じる。最後のリポジトリならリポジトリのページを出す。
    // macOS 版と同じ）。
    add("close", close_tab);
    add("close-window", |tab| {
        if let Some(window) = tab.window() {
            window.close();
        }
    });
    // View > Show Tab Bar（Ctrl+Shift+T）: タブが 1 つでも帯を出す・隠す。
    add("toggle-tab-bar", |tab| {
        let Some(host) = tab.host() else { return };
        if host.view.n_pages() > 1 {
            return;
        }
        host.tab_bar.set(!host.tab_bar.get());
        host.update_tabs_bar();
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
    // Window > Move Tab to Window ▸ の項目（移す先の窓の番号）。
    let to_window = gio::SimpleAction::new("move-tab-to-window", Some(glib::VariantTy::UINT32));
    let weak = Rc::downgrade(tab);
    to_window.connect_activate(move |_, value| {
        let (Some(tab), Some(id)) = (weak.upgrade(), value.and_then(|v| v.get::<u32>())) else {
            return;
        };
        let target = WINDOWS.with(|w| w.borrow().iter().find(|h| h.id == id).cloned());
        if let Some(target) = target {
            move_tab(&tab, &target, true);
            target.window.present();
            save();
        }
    });
    tab.group.add_action(&to_window);
    add("merge-all-windows", |tab| {
        let Some(host) = tab.host() else { return };
        let others: Vec<Rc<RepoWindow>> = WINDOWS.with(|w| {
            w.borrow()
                .iter()
                .filter(|w| !Rc::ptr_eq(w, &host))
                .cloned()
                .collect()
        });
        // 運んだタブは前に出るので、前にあったタブに戻す。
        let current = host.current();
        for other in others {
            for t in other.tabs() {
                move_tab(&t, &host, false);
            }
        }
        if let Some(current) = current {
            host.select(&current);
        }
        host.window.present();
        save();
    });
}

/// 前のタブを `delta` だけ隣に（端からは反対の端へ）。
fn step(tab: &Rc<Tab>, delta: i32) {
    let Some(host) = tab.host() else { return };
    let n = host.view.n_pages();
    if n <= 1 {
        return;
    }
    let current = host
        .view
        .selected_page()
        .map_or(0, |p| host.view.page_position(&p));
    let next = (current + delta).rem_euclid(n);
    host.view.set_selected_page(&host.view.nth_page(next));
}

/// `view` の中の、中身が `child` のページ。
/// Ctrl+Home・Ctrl+End（Shift 付きも）は、文字の欄で先頭・末尾へ動く（選ぶ）キー。TabView の既定では窓のどこでも
/// 最初・最後のタブへの切り替えに取られ、コミットメッセージの編集中に押すとタブが変わる。macOS 版のタブにもないので
/// 外す（`shortcuts` は libadwaita 1.2 から。なければそのまま）。
fn keep_text_shortcuts(view: &adw::TabView) {
    let Some(spec) = view.find_property("shortcuts") else {
        return;
    };
    let Some(class) = glib::FlagsClass::with_type(spec.value_type()) else {
        return;
    };
    let value = class
        .builder_with_value(view.property_value("shortcuts"))
        .and_then(|b| {
            b.unset_by_nick("control-home")
                .unset_by_nick("control-end")
                .unset_by_nick("control-shift-home")
                .unset_by_nick("control-shift-end")
                .build()
        });
    if let Some(value) = value {
        view.set_property_from_value("shortcuts", &value);
    }
}

fn page_of(view: &adw::TabView, child: &impl IsA<gtk::Widget>) -> Option<adw::TabPage> {
    let child = child.upcast_ref::<gtk::Widget>();
    (0..view.n_pages())
        .map(|i| view.nth_page(i))
        .find(|p| p.child() == *child)
}

/// 開いている窓とタブの並びを覚える（次の起動で開き直す）。
fn save() {
    refresh_move_menus();
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
        ("win.toggle-tab-bar", &["<Control><Shift>t"][..]),
        ("win.next-tab", &["<Control>Page_Down", "<Control>Tab"][..]),
        (
            "win.previous-tab",
            &["<Control>Page_Up", "<Control><Shift>Tab"][..],
        ),
    ] {
        app.set_accels_for_action(action, accels);
    }
}
