//! History の検索（macOS 版のツールバーの検索の欄と同じ）。見出しの検索のボタンか Ctrl+F で、History の上に
//! 欄を出す（GNOME の検索バー）。Enter で探し、一致した行を塗って件数を出し、選んでいる行より後の最初の一致を
//! 選ぶ。同じ問い合わせなら Enter で次へ、Shift+Enter で前へ（Ctrl+G・Ctrl+Shift+G でも）。書き方はコアの
//! `query`（`author:`・`subject:`・`commit:`・`path:`・`contents:`、AND・OR・括弧）。
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
    /// 一致した行（昇順）と、それを探した問い合わせと History の世代。
    matches: RefCell<Rc<Vec<u32>>>,
    searched: RefCell<Option<(String, u64)>>,
    generation: Cell<u64>,
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
        let count = gtk::Label::new(None);
        count.add_css_class("dim-label");
        count.add_css_class("numeric");
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
        line.append(&entry);
        line.append(&count);
        line.append(&steps);
        let bar = gtk::SearchBar::builder()
            .child(&line)
            .show_close_button(true)
            .build();
        bar.connect_entry(&entry);
        let toggle = gtk::ToggleButton::new();
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Search, false);
        toggle.set_child(Some(&icon));
        toggle.set_tooltip_text(Some(&tr!("Search History (Ctrl+F)")));
        bar.bind_property("search-mode-enabled", &toggle, "active")
            .bidirectional()
            .sync_create()
            .build();

        let inner = Rc::new(Inner {
            view: view.clone(),
            entry: entry.clone(),
            count,
            matches: RefCell::default(),
            searched: RefCell::default(),
            generation: Cell::new(0),
            filter_generation: Cell::new(0),
            filter_started: Cell::new(0),
            pending: RefCell::default(),
            base: RefCell::default(),
            only: only.clone(),
            previous: previous.clone(),
            next: next.clone(),
            on_query: RefCell::default(),
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

    /// タブのアクション（`win.find`・`win.find-next`・`win.find-previous`）を足す。
    pub fn install(&self, group: &gio::SimpleActionGroup, app: &gtk::Application) {
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
            bar.set_search_mode(true);
            entry.grab_focus();
        });
        group.add_action(&find);
        for (name, step) in [("find-next", 1), ("find-previous", -1)] {
            let action = gio::SimpleAction::new(name, None);
            let (i, bar) = (self.inner.clone(), self.bar.clone());
            action.connect_activate(move |_, _| {
                // 差分の中を探しているなら、その一致へ。
                if let Some(w) = bar.root().and_downcast::<gtk::Window>()
                    && (crate::messagefind::find_next_in_focused(&w, step)
                        || crate::commitfiles::find_next_in_focused(&w, step)
                        || crate::difffind::find_next_in_focused(&w, step))
                {
                    return;
                }
                Inner::submit(&i, step)
            });
            group.add_action(&action);
        }
        app.set_accels_for_action("win.find", &["<Control>f"]);
        app.set_accels_for_action("win.find-next", &["<Control>g"]);
        app.set_accels_for_action("win.find-previous", &["<Control><Shift>g"]);
    }

    /// 新しく読んだ History を出す。行が変わるので、検索の結果は捨てる（問い合わせは欄に残し、Enter で探し直せる。
    /// macOS 版と同じ）。一致したコミットだけを出していたなら、元の History に戻す。
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
        if !self.inner.only.is_active() {
            self.inner.view.show(repo, history);
        }
        if !same && self.inner.searched.borrow().is_some() {
            // 絞り込みをやめると、元の History（今読んだもの）を出す。
            self.inner.clear();
        }
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
            let f = self.on_query.borrow().clone();
            if let Some(f) = f {
                f(now.as_deref());
            }
        }
    }

    fn clear(&self) {
        self.generation.set(self.generation.get() + 1);
        // 走っている検索をやめさせる（`contents:` などは、次の検索を始めるまで裏で読み続けてしまう）。
        if let Some((repo, _)) = self.base.borrow().as_ref() {
            repo.cancel_search();
        }
        self.set_matches(Rc::default());
        self.set_searched(None);
        self.count.set_text("");
        self.view.set_matches(Rc::default());
        // 絞り込みもやめる（元の History に戻す）。
        self.only.set_sensitive(false);
        if self.only.is_active() {
            self.only.set_active(false);
        }
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
            this.view.show(repo, base);
            this.view.set_matches(this.matches.borrow().clone());
            if let Some(row) = this.view.selected() {
                this.view.scroll_to_row(row);
            }
            this.update_count();
            return;
        }
        // 一致したコミットだけ（すべて一致なので塗らない）。作るのは linux の 25 万件で 50 ms ほどで、
        // History の大きさに比例するので裏で作る（macOS 版と同じ）。
        let rows = this.matches.borrow().clone();
        this.filter_started.set(generation);
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let matched: Vec<u32> = rows.as_ref().clone();
            let built = gio::spawn_blocking(move || {
                let filtered = repo.history_matching(&base, &matched);
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
            if this.only.is_active()
                && let Ok((repo, filtered)) = built
            {
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
        } else {
            Self::run(this, query, step.signum());
        }
    }

    /// 裏で探す（すべてのコミットを読むので、linux では初回に 1 秒ほど）。`step` が 0 なら選び直さない。
    fn run(this: &Rc<Self>, query: String, step: i32) {
        let Some((repo, history)) = this.base.borrow().clone() else {
            return;
        };
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        this.count.set_text(&tr!("Searching…"));
        let snapshot_generation = history.generation;
        let q = query.clone();
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            // 前の検索はやめさせる（コアは新しい検索を始めると古いものを止める）。
            let rows = gio::spawn_blocking(move || {
                repo.search_history(&history, &q).map_err(|e| e.to_string())
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            if this.generation.get() != generation {
                return;
            }
            match rows {
                Ok(Ok(rows)) => {
                    let rows = Rc::new(rows);
                    this.set_searched(Some((query, snapshot_generation)));
                    this.set_matches(rows.clone());
                    this.only.set_sensitive(true);
                    if this.only.is_active() {
                        // 絞った History を出し終えてから `pending` を減らす。
                        Inner::apply_filter(&this);
                        return;
                    } else {
                        this.view.set_matches(rows);
                        if step != 0 {
                            this.first_match();
                        } else {
                            this.update_count();
                        }
                    }
                }
                Ok(Err(e)) => this.count.set_text(&e),
                Err(_) => {}
            }
            if let Some(p) = this.pending.take() {
                p.set(p.get() - 1);
            }
        });
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

    /// 選んでいる行から `step` 方向の、次の一致を選ぶ（端では反対の端へ回る）。
    fn step(&self, step: i32) {
        // 一致したものだけを出しているなら、どの行も一致なので隣の行へ。
        if self.only.is_active() {
            let count = self.matches.borrow().len() as u32;
            if count == 0 {
                self.view.display().beep();
            }
            if count > 0 {
                let row = match self.view.selected() {
                    Some(r) if step >= 0 => (r + 1) % count,
                    Some(r) => (r + count - 1) % count,
                    None => 0,
                };
                self.view.select(row);
            }
            self.update_count();
            return;
        }
        let matches = self.matches.borrow().clone();
        if matches.is_empty() {
            // 一致がなければ鳴らす（macOS 版と同じ。探す前なら鳴らさない）。
            if self.searched.borrow().is_some() {
                self.view.display().beep();
            }
            self.update_count();
            return;
        }
        let row = match (self.view.selected(), step >= 0) {
            (Some(cur), true) => matches
                .iter()
                .copied()
                .find(|&r| r > cur)
                .unwrap_or(matches[0]),
            (Some(cur), false) => matches
                .iter()
                .rev()
                .copied()
                .find(|&r| r < cur)
                .unwrap_or(*matches.last().unwrap()),
            (None, _) => matches[0],
        };
        self.view.select(row);
        self.update_count();
    }

    /// 「何番目 / 全体」。
    fn update_count(&self) {
        let matches = self.matches.borrow();
        let total = matches.len();
        if self.only.is_active() {
            let text = match self.view.selected() {
                Some(r) => format!("{} / {total}", r + 1),
                None => tr!("{count} matches", count = total),
            };
            self.count.set_text(&text);
            return;
        }
        let text = match self
            .view
            .selected()
            .and_then(|r| matches.binary_search(&r).ok())
        {
            Some(i) => format!("{} / {total}", i + 1),
            None if total == 0 => tr!("No matches"),
            None => tr!("{count} matches", count = total),
        };
        self.count.set_text(&text);
    }
}
