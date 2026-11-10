//! History の検索（macOS 版のツールバーの検索の欄と同じ）。見出しの検索のボタンか Ctrl+F で、History の上に
//! 欄を出す（GNOME の検索バー）。Enter で探し、一致した行を塗って件数を出し、選んでいる行より後の最初の一致を
//! 選ぶ。同じ問い合わせなら Enter で次へ、Shift+Enter で前へ（Ctrl+G・Ctrl+Shift+G でも）。書き方はコアの
//! `query`（`author:`・`subject:`・`commit:`・`path:`・`contents:`・`after:`・`before:`、AND・OR・括弧）。
//! 欄の中の虫眼鏡（▾ 付き）のメニューで、作者・パス・内容・日時で探す書き方を入れる（macOS 版と同じ）。
//! partial clone で中身を探す前は、手元にない中身の数を出して断る。探している間は、分かった分から塗って件数と
//! 進み具合を出す（macOS 版と同じ）。
//! 一致したコミットだけを出す切り替え（macOS 版と同じ）を入れると、一致したものだけの History
//! （`history_matching`。間を飛ばして親につなぐ）に替える。探すのはいつも元の History。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use amonhen_core::{HistorySnapshot, Repository};
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::sync::Arc;

use crate::history::HistoryView;

/// 探した問い合わせが変わったときに呼ぶもの。
type OnQuery = Rc<dyn Fn(Option<&str>)>;

pub struct Search {
    pub bar: gtk::SearchBar,
    pub toggle: gtk::ToggleButton,
    inner: Rc<Inner>,
}

struct Inner {
    view: HistoryView,
    entry: gtk::SearchEntry,
    count: gtk::Label,
    /// 探している間に回す（macOS 版と同じ）。
    spinner: gtk::Spinner,
    /// 一致した行（昇順）と、それを探した問い合わせと History の世代。
    matches: RefCell<Rc<Vec<u32>>>,
    searched: RefCell<Option<(String, u64)>>,
    /// 探した問い合わせが決まった・消えた（Find Next を使えるかを決め直す）。
    on_find_state: RefCell<Option<Rc<dyn Fn()>>>,
    /// タブの中で最後にフォーカスがあった部品（メニューや見出しの帯を押してフォーカスが移っても、Find Next は
    /// その部品に効かせる。macOS 版のメニューは first responder を変えない）。
    last_focus: RefCell<glib::WeakRef<gtk::Widget>>,
    /// タブの中身（`last_focus` を覚える範囲）。
    tab_root: RefCell<glib::WeakRef<gtk::Widget>>,
    generation: Cell<u64>,
    /// 最後に探し始めた問い合わせ（History を読み直したとき、絞り込んだままにするなら探し直す）。
    requested: RefCell<Option<String>>,
    /// 最後に移った一致の番目（選んだ行が一致でなくても、件数の欄はこれを出す。macOS 版と同じ）。
    index: Cell<Option<usize>>,
    /// 一致したコミットだけの History を出している（元の History ではない）。
    filtered: Cell<bool>,
    /// 一致したコミットだけの History を作る世代（作り終えたとき、古いものなら捨てる）。
    filter_generation: Cell<u64>,
    /// 裏で作り始めた最後の世代（それより後の呼び出しが裏で作らなかったら、古いものを捨てるときに
    /// `pending` を減らす。ほかに減らすものがないため）。
    filter_started: Cell<u64>,
    /// `--search` のとき、終わったら 1 つ減らす。
    pending: RefCell<Option<Rc<Cell<u32>>>>,
    /// 元の History（探すのはいつもこれ）。
    base: RefCell<Option<(Arc<Repository>, Arc<HistorySnapshot>)>>,
    /// 一致したコミットだけを出す。
    only: gtk::ToggleButton,
    /// 前・次の一致へ（一致があるときだけ押せる。macOS 版と同じ）。
    previous: gtk::Button,
    next: gtk::Button,
    /// 探した問い合わせが決まった・消えたとき（詳細の欄で一致を強調する）。
    on_query: RefCell<Option<OnQuery>>,
    /// partial clone か（中身を探す前に、取る中身の数を出して断る）。リポジトリの形をまだ読んでいなければ None。
    partial: Cell<Option<bool>>,
    /// Repository Settings の「取得とフォルダ」を開く（中身の検索に要る中身が多すぎるとき）。
    on_show_fetching: RefCell<Option<Rc<dyn Fn()>>>,
}

/// `entry` の虫眼鏡を `button` に替える: 欄の虫眼鏡を隠し、空けた欄の左にボタンを重ねる（欄そのものは
/// GtkSearchEntry のまま。Esc・入力の待ち・消すボタンはそのまま働く）。
fn put_in_field(entry: &gtk::SearchEntry, button: &impl IsA<gtk::Widget>) -> gtk::Overlay {
    if let Some(icon) = entry.first_child().and_downcast::<gtk::Image>() {
        icon.set_visible(false);
    }
    entry.add_css_class("scoped");
    button.set_halign(gtk::Align::Start);
    button.set_valign(gtk::Align::Center);
    button.set_margin_start(4);
    let overlay = gtk::Overlay::builder().child(entry).hexpand(true).build();
    overlay.add_overlay(button);
    overlay
}

/// 欄の中のボタンの見た目（一度だけ入れる）。
fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "entry.search.scoped { padding-left: 34px; } \
         menubutton.search-scope > button { min-height: 0; min-width: 0; padding: 2px 4px; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

impl Search {
    pub fn new(view: &HistoryView) -> Self {
        // 書き方が分かるように、欄の中と吹き出しに説明を出す（macOS 版と同じ）。
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr!("Search (author:, path:, contents: also work)"))
            .tooltip_text(tr!(
                "Words match the subject, author and ID. Spaces mean AND, “or” means OR, and parentheses group.\nauthor:name  subject:text  commit:ID  path:path  contents:\"diff text\"\nExample: path:fs/ (author:linus or author:greg)"
            ))
            .hexpand(true)
            .build();
        // ▾: 作者・パス・内容・日時で探す書き方を入れる（macOS 版の検索の欄の ▾ のメニューと同じ）。
        let scopes = gio::Menu::new();
        for (title, prefix) in SCOPES {
            scopes.append(
                Some(&amonhen_model::i18n::translate(title)),
                Some(&gio::Action::print_detailed_name(
                    "search.insert-prefix",
                    Some(&prefix.to_variant()),
                )),
            );
        }
        // 欄の虫眼鏡を ▾ 付きのボタンに替える（NSSearchField と同じく、欄の中の虫眼鏡は 1 つ）。
        let scope_icon = crate::icon::Icon::default();
        scope_icon.set(crate::icon::Kind::Search, false);
        let arrow: gtk::Widget =
            if gtk::IconTheme::for_display(&view.display()).has_icon("pan-down-symbolic") {
                let arrow = gtk::Image::from_icon_name("pan-down-symbolic");
                arrow.set_pixel_size(8);
                arrow.upcast()
            } else {
                let arrow = gtk::Label::new(Some("▾"));
                arrow.add_css_class("caption");
                arrow.upcast()
            };
        let scope_inner = gtk::Box::new(gtk::Orientation::Horizontal, 1);
        scope_inner.append(&scope_icon);
        scope_inner.append(&arrow);
        let scope = gtk::MenuButton::builder()
            .menu_model(&scopes)
            .child(&scope_inner)
            .has_frame(false)
            .build();
        scope.add_css_class("search-scope");
        install_css();
        let field = put_in_field(&entry, &scope);
        let insert = gio::SimpleAction::new("insert-prefix", Some(glib::VariantTy::STRING));
        let e = entry.clone();
        insert.connect_activate(move |_, prefix| {
            let Some(prefix) = prefix.and_then(|p| p.str()) else {
                return;
            };
            e.set_text(&with_prefix(&e.text(), prefix));
            e.grab_focus();
            e.set_position(-1);
        });
        let search_actions = gio::SimpleActionGroup::new();
        search_actions.add_action(&insert);
        let count = gtk::Label::new(None);
        count.add_css_class("dim-label");
        count.add_css_class("numeric");
        let spinner = gtk::Spinner::new();
        spinner.set_visible(false);
        let previous = gtk::Button::from_icon_name("go-up-symbolic");
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        if !gtk::IconTheme::for_display(&view.display()).has_icon("go-up-symbolic") {
            previous.set_label("∧");
            next.set_label("∨");
        }
        previous.set_tooltip_text(Some(&tr!("Previous Match (Shift+Enter, Ctrl+Shift+G)")));
        next.set_tooltip_text(Some(&tr!("Next Match (Enter, Ctrl+G)")));
        let steps = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        steps.add_css_class("linked");
        steps.append(&previous);
        steps.append(&next);
        let only = gtk::ToggleButton::new();
        let filter_icon = crate::icon::Icon::default();
        filter_icon.set(crate::icon::Kind::Filter, false);
        only.set_child(Some(&filter_icon));
        only.set_tooltip_text(Some(&tr!("Show Only Matching Commits")));
        only.set_sensitive(false);
        steps.append(&only);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.insert_action_group("search", Some(&search_actions));
        line.append(&field);
        line.append(&spinner);
        line.append(&count);
        line.append(&steps);
        let bar = gtk::SearchBar::builder()
            .child(&line)
            .show_close_button(true)
            .build();
        bar.connect_entry(&entry);
        // History で文字を打つと、欄を開いてその文字から探す（GNOME の type-to-search。macOS 版にはない。矢印・
        // スペース・Ctrl や Alt との組み合わせ・Menu は GtkSearchEntry が拾わず、History のキーのまま）。
        bar.set_key_capture_widget(Some(view));
        let toggle = gtk::ToggleButton::new();
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Search, false);
        toggle.set_child(Some(&icon));
        toggle.set_tooltip_text(Some(&tr!("Search History (Ctrl+F)")));
        // ボタンを切っても、欄が見えていない（ほかの画面にいる）ときは欄を閉じない。`install` の clicked が History に
        // 移る（閉じると問い合わせ・一致・絞り込みが消える。macOS 版の検索欄はそのまま History で探す）。
        bar.bind_property("search-mode-enabled", &toggle, "active")
            .bidirectional()
            .sync_create()
            .transform_from(|binding, active: bool| {
                let shown = binding.source().is_some_and(|bar| {
                    bar.downcast_ref::<gtk::Widget>()
                        .is_some_and(|b| b.is_mapped())
                });
                (active || shown).then_some(active)
            })
            .build();

        let inner = Rc::new(Inner {
            view: view.clone(),
            entry: entry.clone(),
            count,
            spinner,
            matches: RefCell::default(),
            searched: RefCell::default(),
            on_find_state: RefCell::default(),
            last_focus: RefCell::default(),
            tab_root: RefCell::default(),
            generation: Cell::new(0),
            requested: RefCell::default(),
            index: Cell::new(None),
            filtered: Cell::new(false),
            filter_generation: Cell::new(0),
            filter_started: Cell::new(0),
            pending: RefCell::default(),
            base: RefCell::default(),
            only: only.clone(),
            previous: previous.clone(),
            next: next.clone(),
            on_query: RefCell::default(),
            partial: Cell::new(None),
            on_show_fetching: RefCell::default(),
        });
        inner.set_matches(Rc::default());
        let i = inner.clone();
        only.connect_toggled(move |_| Inner::apply_filter(&i));
        let i = inner.clone();
        entry.connect_activate(move |_| Inner::submit(&i, 1));
        // Shift+Enter: 前の一致へ。
        let keys = gtk::EventControllerKey::new();
        let i = inner.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
                && state.contains(gdk::ModifierType::SHIFT_MASK)
            {
                Inner::submit(&i, -1);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        entry.add_controller(keys);
        let i = inner.clone();
        entry.connect_search_changed(move |e| {
            // 空にしたら一致を消す。
            if e.text().is_empty() {
                i.clear();
            }
        });
        let i = inner.clone();
        previous.connect_clicked(move |_| Inner::submit(&i, -1));
        let i = inner.clone();
        next.connect_clicked(move |_| Inner::submit(&i, 1));
        let i = inner.clone();
        bar.connect_search_mode_enabled_notify(move |bar| {
            if !bar.is_search_mode() {
                i.clear();
                i.view.grab_focus();
            }
        });
        Self { bar, toggle, inner }
    }

    /// タブのアクション（`win.find`・`win.find-next`・`win.find-previous`）を足す（ショートカットは
    /// `crate::appmenu` が入れる）。
    pub fn install(&self, group: &gio::SimpleActionGroup) {
        // 見出しの検索のボタン・Ctrl+F で欄を開いたとき、ほかの画面にいれば History に移る（欄は History の上に
        // しかない。macOS 版の検索欄も History を選んでから探す）。
        let g = group.downgrade();
        self.bar.connect_search_mode_enabled_notify(move |bar| {
            if bar.is_search_mode()
                && !bar.is_mapped()
                && let Some(group) = g.upgrade()
            {
                group.activate_action("show-history", None);
            }
        });
        // 欄を開いたまま History を離れていれば、押しても閉じずに History に移る。
        let (g, bar, entry) = (
            group.downgrade(),
            self.bar.clone(),
            self.inner.entry.clone(),
        );
        self.toggle.connect_clicked(move |toggle| {
            if !bar.is_mapped()
                && !toggle.is_active()
                && let Some(group) = g.upgrade()
            {
                // 欄は開いたまま（上の bind_property が閉じない）。ボタンを戻して History に移る。
                toggle.set_active(true);
                bar.set_search_mode(true);
                group.activate_action("show-history", None);
                entry.grab_focus();
            }
        });
        let find = gio::SimpleAction::new("find", None);
        let (bar, entry) = (self.bar.clone(), self.inner.entry.clone());
        find.connect_activate(move |_, _| {
            // フォーカスが History の差分にあれば、その差分の中を探す（macOS 版と同じ）。
            if let Some(w) = bar.root().and_downcast::<gtk::Window>()
                && (crate::messagefind::find_in_focused(&w)
                    || crate::commitfiles::find_in_focused(&w)
                    || crate::difffind::find_in_focused(&w))
            {
                return;
            }
            // History でなければ探すものがない（macOS 版の Find は押せない）。
            if !bar.is_mapped() {
                if let Some(w) = bar.root().and_downcast::<gtk::Window>() {
                    w.error_bell();
                }
                return;
            }
            bar.set_search_mode(true);
            entry.grab_focus();
        });
        group.add_action(&find);
        // Edit > Find in History…: フォーカスがどこにあっても History の検索欄へ（macOS 版と同じ。`win.find` と違い、
        // 差分の中は探さない）。
        let history = gio::SimpleAction::new("search-history", None);
        let (bar, entry) = (self.bar.clone(), self.inner.entry.clone());
        history.connect_activate(move |_, _| {
            bar.set_search_mode(true);
            entry.grab_focus();
        });
        group.add_action(&history);
        for (name, step) in [("find-next", 1), ("find-previous", -1)] {
            let action = gio::SimpleAction::new(name, None);
            let (i, bar) = (self.inner.clone(), self.bar.clone());
            action.connect_activate(move |_, _| {
                // メニューから押してフォーカスがタブの外（メニュー・見出しの帯）にあれば、タブの中で最後にフォーカスが
                // あった部品に戻してから決める（macOS 版のメニューは first responder を変えない）。
                if let Some(w) = bar.root().and_downcast::<gtk::Window>() {
                    i.restore_focus(&w);
                }
                // 差分の中を探しているなら、その一致へ。
                if let Some(w) = bar.root().and_downcast::<gtk::Window>()
                    && (crate::messagefind::find_next_in_focused(&w, step)
                        || crate::commitfiles::find_next_in_focused(&w, step)
                        || crate::difffind::find_next_in_focused(&w, step))
                {
                    return;
                }
                // まだ探していないとき: History を見ていれば欄を開き、ほかの所にいれば鳴らす。探していれば
                // History に移って次の一致へ（macOS 版の History の moveSearch と、窓の findNext と同じ）。
                if i.searched.borrow().is_none() {
                    // 詳細・差分にフォーカスがあれば、その検索の帯を開いて次の一致へ（macOS 版と同じ）。
                    if let Some(w) = bar.root().and_downcast::<gtk::Window>()
                        && (crate::messagefind::find_next_opening(&w, step)
                            || crate::commitfiles::find_next_opening(&w, step)
                            || crate::difffind::find_next_opening(&w, step))
                    {
                        return;
                    }
                    // History の欄を開くのは、フォーカスが History の中にあるときだけ（macOS 版の History の
                    // findNext。ほかの所では窓の findNext に行き、History を探していなければ何もしない）。
                    let in_history = bar
                        .root()
                        .and_downcast::<gtk::Window>()
                        .is_some_and(|w| focus_in_history(&w, &bar));
                    if bar.is_mapped() && in_history {
                        bar.set_search_mode(true);
                        i.entry.grab_focus();
                    } else if let Some(w) = bar.root().and_downcast::<gtk::Window>() {
                        w.error_bell();
                    }
                    return;
                }
                let _ = WidgetExt::activate_action(&bar, "win.show-history", None);
                Inner::submit(&i, step)
            });
            group.add_action(&action);
        }
    }

    /// Find Next・Find Previous が効くか（macOS 版で ⌘G を受ける部品があるか）を、変わるたびに `f(効くか)` で
    /// 知らせる: History を探した、またはフォーカスが差分（検索の帯の欄は除く）か詳細か History の
    /// 中にある。`root`（タブの中身）が前にある間、窓のフォーカスの移りを見る。フォーカスがタブの外（メニュー・
    /// 見出しの帯）に移っても、タブの中で最後にフォーカスがあった所で決める。
    pub fn track_focus(&self, root: &gtk::Widget, f: impl Fn(bool) + 'static) {
        self.inner.tab_root.replace(root.downgrade());
        let last = Rc::new(Cell::new(false));
        // タブの中のフォーカスで決めた分。
        let by_focus = Rc::new(Cell::new(false));
        let refresh: Rc<dyn Fn()> = {
            // 探した状態から呼ばれるので、循環しないよう弱く持つ。
            let (i, bar, root) = (
                Rc::downgrade(&self.inner),
                self.bar.downgrade(),
                root.downgrade(),
            );
            Rc::new(move || {
                let (Some(root), Some(i), Some(bar)) = (root.upgrade(), i.upgrade(), bar.upgrade())
                else {
                    return;
                };
                if let Some(window) = root.root().and_downcast::<gtk::Window>()
                    && let Some(focus) = gtk::prelude::GtkWindowExt::focus(&window)
                    && (focus == root || focus.is_ancestor(&root))
                {
                    i.last_focus.replace(focus.downgrade());
                    by_focus.set(
                        crate::messagefind::find_next_possible(&window)
                            || crate::commitfiles::find_next_possible(&window)
                            || crate::difffind::find_next_possible(&window)
                            || (bar.is_mapped() && focus_in_history(&window, &bar)),
                    );
                }
                let available = i.searched.borrow().is_some() || by_focus.get();
                if last.replace(available) != available {
                    f(available);
                }
            })
        };
        let r = refresh.clone();
        self.inner.on_find_state.replace(Some(Rc::new(move || r())));
        type Watch = Option<(glib::WeakRef<gtk::Window>, glib::SignalHandlerId)>;
        let watch: Rc<RefCell<Watch>> = Rc::default();
        let w = watch.clone();
        root.connect_map(move |root| {
            if let Some((window, id)) = w.take()
                && let Some(window) = window.upgrade()
            {
                window.disconnect(id);
            }
            if let Some(window) = root.root().and_downcast::<gtk::Window>() {
                let r = refresh.clone();
                let id = window.connect_focus_widget_notify(move |_| r());
                w.replace(Some((window.downgrade(), id)));
            }
            refresh();
        });
        root.connect_unmap(move |_| {
            if let Some((window, id)) = watch.take()
                && let Some(window) = window.upgrade()
            {
                window.disconnect(id);
            }
        });
    }

    /// 一致したコミットだけを出しているか。
    pub fn is_filtered(&self) -> bool {
        self.inner.filtered.get()
    }

    /// 新しく読んだ History を出す。行が変わるので、検索の結果は捨てる（問い合わせは欄に残し、Enter で探し直せる。
    /// macOS 版と同じ）。一致したコミットだけを出しているなら、新しい History で探し直して絞り込んだままにする
    /// （それまでは今の絞り込みを出しておく。macOS 版の `showBase` と同じ）。
    pub fn show_history(&self, repo: Arc<Repository>, history: Arc<HistorySnapshot>) {
        let same = self
            .inner
            .base
            .borrow()
            .as_ref()
            .is_some_and(|(_, h)| Arc::ptr_eq(h, &history));
        self.inner
            .base
            .replace(Some((repo.clone(), history.clone())));
        if same {
            if !self.inner.filtered.get() {
                self.inner.view.show(repo, history);
            }
            return;
        }
        let query = self.inner.requested.borrow().clone();
        if self.inner.only.is_active()
            && self.inner.filtered.get()
            && let Some(query) = query
        {
            Inner::run(&self.inner, query, 0);
            return;
        }
        self.inner.view.set_matching_only(false);
        self.inner.view.show(repo, history);
        // 走っている検索・絞り込みは古い History のものなので捨てる。
        self.inner.clear();
    }

    /// partial clone か（リポジトリの形を読んだとき）。
    pub fn set_partial(&self, partial: bool) {
        self.inner.partial.set(Some(partial));
    }

    /// 中身の検索に要る中身が多すぎるとき、「取得とフォルダ」を開くもの（macOS 版の `onShowFetchingSettings`）。
    pub fn connect_show_fetching(&self, f: impl Fn() + 'static) {
        self.inner.on_show_fetching.replace(Some(Rc::new(f)));
    }

    /// 探した問い合わせが決まった・消えたら `f(問い合わせ)` を呼ぶ（macOS 版の `onSearchChanged`）。
    pub fn connect_query(&self, f: impl Fn(Option<&str>) + 'static) {
        self.inner.on_query.replace(Some(Rc::new(f)));
    }

    /// 欄を開いて `query` を探す（`--search`）。終わるまで `pending` を 1 つ増やしておく。`only` なら、
    /// 一致したコミットだけを出す。
    pub fn search(&self, query: &str, only: bool, pending: Rc<Cell<u32>>) {
        if only {
            // 探し終わったら絞る（今はまだ一致がないので、切り替えの知らせでは何もしない）。
            self.inner.only.set_sensitive(true);
            self.inner.only.set_active(true);
        }
        self.bar.set_search_mode(true);
        self.inner.entry.set_text(query);
        pending.set(pending.get() + 1);
        self.inner.pending.replace(Some(pending));
        Inner::submit(&self.inner, 1);
    }
}

impl Inner {
    /// フォーカスがタブの外にあれば、タブの中で最後にフォーカスがあった部品に戻す。
    fn restore_focus(&self, window: &gtk::Window) {
        let Some(root) = self.tab_root.borrow().upgrade() else {
            return;
        };
        let inside = gtk::prelude::GtkWindowExt::focus(window)
            .is_some_and(|f| f == root || f.is_ancestor(&root));
        if !inside
            && let Some(last) = self.last_focus.borrow().upgrade()
            && last.is_mapped()
        {
            last.grab_focus();
        }
    }

    /// 一致した行を入れ、前・次のボタンを一致があるときだけ押せるようにする。
    fn set_matches(&self, rows: Rc<Vec<u32>>) {
        let any = !rows.is_empty();
        self.matches.replace(rows);
        self.previous.set_sensitive(any);
        self.next.set_sensitive(any);
    }

    /// 探した問い合わせを入れ替え、変わったら知らせる。
    fn set_searched(&self, searched: Option<(String, u64)>) {
        let before = self.searched.borrow().as_ref().map(|(q, _)| q.clone());
        let now = searched.as_ref().map(|(q, _)| q.clone());
        self.searched.replace(searched);
        if before != now {
            let f = self.on_find_state.borrow().clone();
            if let Some(f) = f {
                f();
            }
            let f = self.on_query.borrow().clone();
            if let Some(f) = f {
                f(now.as_deref());
            }
        }
    }

    fn clear(&self) {
        self.generation.set(self.generation.get() + 1);
        self.filter_generation.set(self.filter_generation.get() + 1);
        self.requested.replace(None);
        self.index.set(None);
        self.spinner.stop();
        self.spinner.set_visible(false);
        self.count.set_tooltip_text(None);
        // `--search` の途中なら、やめたので待たせない。
        if let Some(p) = self.pending.take() {
            p.set(p.get() - 1);
        }
        // 走っている検索をやめさせる（`contents:` などは、次の検索を始めるまで裏で読み続けてしまう）。
        if let Some((repo, _)) = self.base.borrow().as_ref() {
            repo.cancel_search();
        }
        // 絞り込みもやめて元の History に戻す。切り替えは押せなくするだけで、入れたままにする（次に探したときも
        // 一致だけを出す。macOS 版と同じ）。
        self.only.set_sensitive(false);
        if self.filtered.replace(false)
            && let Some((repo, base)) = self.base.borrow().clone()
        {
            self.view.set_matching_only(false);
            self.view.show(repo, base);
        }
        self.set_matches(Rc::default());
        self.set_searched(None);
        self.count.set_text("");
        self.view.set_matches(Rc::default());
    }

    /// 切り替えに合わせて、一致したコミットだけの History か、元の History を出す。
    fn apply_filter(this: &Rc<Self>) {
        let generation = this.filter_generation.get() + 1;
        this.filter_generation.set(generation);
        let Some((repo, base)) = this.base.borrow().clone() else {
            return;
        };
        if !this.only.is_active() {
            // 選んでいたコミットは、元の History でも選んだまま（`show` が ID で選び直す。macOS 版と同じ）。
            if this.filtered.replace(false) {
                this.view.set_matching_only(false);
                this.view.show(repo, base);
            }
            this.view.set_matches(this.matches.borrow().clone());
            if let Some(row) = this.view.selected() {
                this.view.scroll_to_row(row);
            }
            this.update_count();
            return;
        }
        // 一致がなければ、元の History のまま（空にしない。macOS 版と同じ）。
        let rows = this.matches.borrow().clone();
        if rows.is_empty() {
            if this.filtered.replace(false) {
                this.view.set_matching_only(false);
                this.view.show(repo, base);
            }
            this.update_count();
            return;
        }
        // 一致したコミットだけ（すべて一致なので塗らない）。作るのは linux の 25 万件で 50 ms ほどで、
        // History の大きさに比例するので裏で作る（macOS 版と同じ）。
        this.filter_started.set(generation);
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let matched: Vec<u32> = rows.as_ref().clone();
            let source = base.clone();
            let built = gio::spawn_blocking(move || {
                let filtered = repo.history_matching(&source, &matched);
                (repo, filtered)
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            if this.filter_generation.get() != generation {
                // 後の呼び出しが裏で作っていれば、それが減らす。
                if this.filter_started.get() == generation
                    && let Some(p) = this.pending.take()
                {
                    p.set(p.get() - 1);
                }
                return;
            }
            // 作っている間に History が替わったら捨てる（新しい History で探し直している）。
            let current = this
                .base
                .borrow()
                .as_ref()
                .is_some_and(|(_, h)| Arc::ptr_eq(h, &base));
            if current
                && this.only.is_active()
                && let Ok((repo, filtered)) = built
            {
                this.filtered.set(true);
                this.view.set_matching_only(true);
                // 選んでいたコミットは、絞り込んだ History でも選んだまま（`show` が ID で選び直し、なければ
                // 先頭を選ぶ。macOS 版と同じ）。
                this.view.show(repo, filtered);
                if let Some(row) = this.view.selected() {
                    this.view.scroll_to_row(row);
                }
                this.update_count();
            }
            if let Some(p) = this.pending.take() {
                p.set(p.get() - 1);
            }
        });
    }

    /// 問い合わせが前と同じなら `step` だけ一致を移り、違えば探す。
    fn submit(this: &Rc<Self>, step: i32) {
        let query = this.entry.text().trim().to_owned();
        if query.is_empty() {
            return;
        }
        let generation = this.base.borrow().as_ref().map(|(_, h)| h.generation);
        let same = this
            .searched
            .borrow()
            .as_ref()
            .is_some_and(|(q, g)| *q == query && Some(*g) == generation);
        if same {
            this.step(step);
        } else if this.partial.get() != Some(false) && searches_contents(&query) {
            Self::confirm_contents(this, query, step.signum());
        } else {
            Self::run(this, query, step.signum());
        }
    }

    /// partial clone で中身を探す前に、取る中身の数を数えて断る。上限を超えるなら探さず、絞るか中身をすべて取るよう
    /// 頼む（macOS 版の `confirmContentsSearch`）。
    fn confirm_contents(this: &Rc<Self>, query: String, step: i32) {
        let Some((repo, history)) = this.base.borrow().clone() else {
            return;
        };
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        this.count.set_text("");
        this.count.set_tooltip_text(None);
        this.spinner.set_visible(true);
        this.spinner.start();
        let (q, counted) = (query.clone(), history.clone());
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            // リポジトリの形をまだ読んでいなければ（開いてすぐの検索・`--search`）、ここで読む。
            let partial = match weak.upgrade().and_then(|this| this.partial.get()) {
                Some(partial) => partial,
                None => crate::repoform::load(&repo)
                    .await
                    .is_some_and(|form| form.partial.is_some()),
            };
            if !partial {
                if let Some(this) = weak.upgrade()
                    && this.generation.get() == generation
                {
                    Self::run(&this, query, step);
                }
                return;
            }
            let need = gio::spawn_blocking(move || {
                repo.count_contents_to_fetch(&counted, &q)
                    .map_err(|e| e.to_string())
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            let current = this
                .base
                .borrow()
                .as_ref()
                .is_some_and(|(_, h)| Arc::ptr_eq(h, &history));
            if this.generation.get() != generation || !current {
                return;
            }
            this.spinner.stop();
            this.spinner.set_visible(false);
            let need = match need {
                Ok(Ok(need)) => need,
                Ok(Err(e)) => {
                    this.count.set_text(&e);
                    this.count.set_tooltip_text(Some(&e));
                    this.done_pending();
                    return;
                }
                Err(_) => {
                    this.done_pending();
                    return;
                }
            };
            let Some(need) = need else {
                Self::run(&this, query, step);
                return;
            };
            // `--search` では、尋ねている窓を撮れるように、窓を出してから終わりにする。
            {
                let weak = Rc::downgrade(&this);
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    if let Some(this) = weak.upgrade() {
                        this.done_pending();
                    }
                });
            }
            let window = this.entry.root().and_downcast::<gtk::Window>();
            let count = grouped(need.count);
            if need.more {
                // ほかの確認と同じシートで（macOS 版と同じ）。
                let sheet = crate::sheets::Sheet::unnamed(
                    window.as_ref(),
                    &tr!("Too Many Contents to Fetch"),
                    &tr!(
                        "This search needs more than {count} file contents not fetched yet. Narrow it with path:, after: or before:, or fetch all contents in Fetching and Folders.",
                        count = count
                    ),
                );
                sheet.buttons(&tr!("Open Fetching and Folders"), false, None);
                if sheet.run().await == crate::sheets::Response::Ok
                    && let Some(f) = this.on_show_fetching.borrow().clone()
                {
                    f();
                }
                return;
            }
            let sheet = crate::sheets::Sheet::unnamed(
                window.as_ref(),
                &tr!("Search the Contents?"),
                &tr!(
                    "This search fetches {count} file contents not fetched yet.",
                    count = count
                ),
            );
            sheet.buttons(&tr!("Search"), false, None);
            let chosen = sheet.run().await == crate::sheets::Response::Ok;
            // 尋ねている間に History が替わったら探さない（macOS 版と同じ）。
            let current = this
                .base
                .borrow()
                .as_ref()
                .is_some_and(|(_, h)| Arc::ptr_eq(h, &history));
            if chosen && current && this.generation.get() == generation {
                Self::run(&this, query, step);
            }
        });
    }

    /// 裏で探す（すべてのコミットを読むので、linux では初回に 1 秒ほど）。`step` が 0 なら選び直さない。
    fn run(this: &Rc<Self>, query: String, step: i32) {
        let Some((repo, history)) = this.base.borrow().clone() else {
            return;
        };
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        this.requested.replace(Some(query.clone()));
        this.count.set_text("");
        this.count.set_tooltip_text(None);
        this.spinner.set_visible(true);
        this.spinner.start();
        let snapshot_generation = history.generation;
        let q = query.clone();
        let searched = history.clone();
        // 内容の検索は大きな履歴では数分かかるので、分かった分から塗り、件数と進み具合を出す（macOS 版と同じ）。
        {
            let (weak, repo) = (Rc::downgrade(this), repo.clone());
            glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                let Some(this) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if this.generation.get() != generation || !this.spinner.is_visible() {
                    return glib::ControlFlow::Break;
                }
                let (rows, done) = repo.search_partial();
                if done > 0.0 || !rows.is_empty() {
                    this.count.set_text(&tr!(
                        "{count} found · {percent}%",
                        count = rows.len(),
                        percent = (done * 100.0) as u32
                    ));
                    // 絞り込みを出したまま元の History で探しているときは、行の番号が合わないので塗らない。
                    if !this.filtered.get() {
                        this.view.set_matches(Rc::new(rows));
                    }
                }
                glib::ControlFlow::Continue
            });
        }
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            // 前の検索はやめさせる（コアは新しい検索を始めると古いものを止める）。
            let rows = gio::spawn_blocking(move || {
                repo.search_history(&searched, &q)
                    .map_err(|e| e.to_string())
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            // 探している間に History が替わったら、行番号が合わないので捨てる（macOS 版と同じ）。
            let current = this
                .base
                .borrow()
                .as_ref()
                .is_some_and(|(_, h)| Arc::ptr_eq(h, &history));
            if this.generation.get() != generation || !current {
                return;
            }
            this.spinner.stop();
            this.spinner.set_visible(false);
            match rows {
                Ok(Ok(rows)) => {
                    let rows = Rc::new(rows);
                    this.set_searched(Some((query, snapshot_generation)));
                    this.set_matches(rows.clone());
                    this.only.set_sensitive(true);
                    if this.only.is_active() && !rows.is_empty() {
                        // 絞った History を出し終えてから `pending` を減らす。
                        Inner::apply_filter(&this);
                        return;
                    } else {
                        // 一致がなければ、絞り込まずに元の History を出す（macOS 版と同じ）。
                        if this.filtered.replace(false)
                            && let Some((repo, base)) = this.base.borrow().clone()
                        {
                            this.view.set_matching_only(false);
                            this.view.show(repo, base);
                        }
                        this.view.set_matches(rows);
                        if step != 0 {
                            this.first_match();
                        } else {
                            this.update_count();
                        }
                    }
                }
                Ok(Err(e)) => {
                    // 構文の誤りなどは件数の欄に出す（長いのでツールチップにも。macOS 版と同じ）。
                    this.count.set_text(&e);
                    this.count.set_tooltip_text(Some(&e));
                }
                Err(_) => {}
            }
            if let Some(p) = this.pending.take() {
                p.set(p.get() - 1);
            }
        });
    }

    /// `--search` の終わり（`pending` を 1 つ減らす）。
    fn done_pending(&self) {
        if let Some(p) = self.pending.take() {
            p.set(p.get() - 1);
        }
    }

    /// 新しく探したとき: 選んでいる行から下で、いちばん近い一致（選んでいる行が一致ならそれ）を選ぶ。なければ
    /// 先頭の一致へ回る（macOS 版と同じ）。
    fn first_match(&self) {
        let matches = self.matches.borrow().clone();
        let from = self.view.selected().unwrap_or(0);
        if let Some(&row) = matches.iter().find(|&&r| r >= from).or(matches.first()) {
            self.view.select(row);
        }
        self.update_count();
    }

    /// 最後に移った一致から `step` 方向の、次の一致を選ぶ（端では反対の端へ回る）。ほかの行を押して選んでも、
    /// 移る先は変わらない（macOS 版の `moveSearch` と同じ）。
    fn step(&self, step: i32) {
        let count = self.matches.borrow().len();
        if count == 0 {
            // 一致がなければ鳴らす（macOS 版と同じ。探す前なら鳴らさない）。
            if self.filtered.get() || self.searched.borrow().is_some() {
                self.view.display().beep();
            }
            self.update_count();
            return;
        }
        let index = next_index(self.index.get().filter(|&i| i < count), step, count);
        self.index.set(Some(index));
        // 一致したものだけを出しているなら、どの行も一致なので、番目がそのまま行。
        let row = if self.filtered.get() {
            index as u32
        } else {
            self.matches.borrow()[index]
        };
        self.view.select(row);
        self.update_count();
    }

    /// 「何番目 / 全体」。選んだ行が一致でなければ、最後に移った一致の番目を出したままにする（macOS 版と同じ）。
    fn update_count(&self) {
        // 探す前・結果を捨てた後（History を読み直したとき）は何も出さない（「一致なし」ではない）。
        if self.searched.borrow().is_none() {
            self.count.set_text("");
            return;
        }
        let matches = self.matches.borrow();
        let total = matches.len();
        let selected = if self.filtered.get() {
            self.view.selected().map(|r| r as usize)
        } else {
            self.view
                .selected()
                .and_then(|r| matches.binary_search(&r).ok())
        };
        if selected.is_some() {
            self.index.set(selected);
        }
        let index = self.index.get().filter(|&i| i < total);
        self.count.set_text(&count_text(index, total));
    }
}

/// 一致の `index` 番目から `step` だけ動いた番目（端では反対の端へ。まだどこにも移っていなければ先頭）。
fn next_index(index: Option<usize>, step: i32, count: usize) -> usize {
    let count = count as i64;
    let from = index.map_or(-i64::from(step), |i| i as i64);
    (from + i64::from(step)).rem_euclid(count) as usize
}

/// 件数の欄の文字（macOS 版と同じく「N / M」か「No Matches」）。
fn count_text(index: Option<usize>, total: usize) -> String {
    match index {
        _ if total == 0 => tr!("No Matches"),
        Some(i) => format!("{} / {total}", i + 1),
        None => tr!("{count} matches", count = total),
    }
}

/// 問い合わせに `contents:`（`content:`）があるか（引用の中の語も数えるが、尋ねすぎるだけ。macOS 版の
/// `searchesContents`）。
fn searches_contents(query: &str) -> bool {
    let lower = query.to_lowercase();
    ["contents:", "content:"].iter().any(|key| {
        lower.match_indices(key).any(|(at, _)| {
            let before = lower[..at].chars().next_back();
            let after = lower[at + key.len()..].chars().next();
            before.is_none_or(|c| c.is_whitespace() || c == '(')
                && after.is_some_and(|c| !c.is_whitespace())
        })
    })
}

/// 3 桁ごとに区切った数（macOS 版の `NumberFormatter` の `.decimal`）。
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 検索の欄の ▾ のメニュー（題と入れる書き方。macOS 版と同じ順）。
const SCOPES: [(&str, &str); 5] = [
    ("Search by Author (author:)", "author:"),
    ("Search by Path (path:)", "path:"),
    ("Search by Contents (contents:)", "contents:"),
    ("Committed After (after:)", "after:"),
    ("Committed Before (before:)", "before:"),
];

/// 欄の `current` に書き方 `prefix` を足したもの（空なら `prefix` だけ、あれば空白で区切って後ろに。macOS 版と同じ）。
fn with_prefix(current: &str, prefix: &str) -> String {
    let current = current.trim();
    if current.is_empty() {
        prefix.to_string()
    } else {
        format!("{current} {prefix}")
    }
}

/// `window` でフォーカスが History の列（検索の欄・表示の帯・一覧。`bar` の親）の中にあるか。
fn focus_in_history(window: &gtk::Window, bar: &gtk::SearchBar) -> bool {
    let (Some(focus), Some(column)) = (gtk::prelude::GtkWindowExt::focus(window), bar.parent())
    else {
        return false;
    };
    focus == column || focus.is_ancestor(&column)
}

#[cfg(test)]
mod tests {
    use super::{SCOPES, count_text, grouped, next_index, searches_contents, with_prefix};

    #[test]
    fn steps_from_the_last_visited_match() {
        assert_eq!(next_index(None, 1, 3), 0);
        assert_eq!(next_index(None, -1, 3), 0);
        assert_eq!(next_index(Some(0), 1, 3), 1);
        assert_eq!(next_index(Some(2), 1, 3), 0);
        assert_eq!(next_index(Some(0), -1, 3), 2);
    }

    #[test]
    fn inserts_a_scope_prefix() {
        assert_eq!(with_prefix("", "author:"), "author:");
        assert_eq!(with_prefix("  ", "path:"), "path:");
        assert_eq!(with_prefix(" fix ", "after:"), "fix after:");
    }

    #[test]
    fn scope_titles_are_translated() {
        amonhen_model::i18n::with(amonhen_model::i18n::Lang::Ja, || {
            for (title, _) in SCOPES {
                assert_ne!(amonhen_model::i18n::translate(title), title);
            }
        });
    }

    #[test]
    fn finds_a_contents_search() {
        assert!(searches_contents("contents:foo"));
        assert!(searches_contents("author:a (content:bar)"));
        assert!(searches_contents("Contents:x"));
        assert!(!searches_contents("contents: foo"));
        assert!(!searches_contents("mycontents:foo"));
        assert!(!searches_contents("path:fs/"));
    }

    #[test]
    fn groups_digits() {
        assert_eq!(grouped(7), "7");
        assert_eq!(grouped(1234), "1,234");
        assert_eq!(grouped(50000), "50,000");
        assert_eq!(grouped(1234567), "1,234,567");
    }

    #[test]
    fn count_shows_position_or_no_matches() {
        amonhen_model::i18n::with(amonhen_model::i18n::Lang::En, || {
            assert_eq!(count_text(Some(0), 3), "1 / 3");
            assert_eq!(count_text(Some(2), 3), "3 / 3");
            assert_eq!(count_text(None, 0), "No Matches");
            assert_eq!(count_text(Some(0), 0), "No Matches");
        });
    }
}
