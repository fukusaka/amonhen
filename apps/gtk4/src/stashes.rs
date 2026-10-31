//! Stashes: 左に stash の一覧（新しい順）と Apply・Pop・Drop…・Branch…、右に選んだ stash の中身（History の
//! 詳細と同じ欄）。macOS 版の `StashesViewController` と同じ（docs/ux.md の「5a. Stashes」）。
//!
//! stash の数は少ないので、コミットの情報（日時・untracked を含むか・土台）は全部読む。一覧は `GtkListView`
//! で、行の中身は名前で引く。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::{RefsSnapshot, Repository};
use amonhen_model::{Oid, RefKind, tr};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::details::Details;
use crate::diff::{self, DiffWidget};
use crate::menu::{self, Item};
use crate::{sheets, titles};

/// 一覧の 1 行。
#[derive(Clone, PartialEq)]
struct Stash {
    /// `stash@{0}`。
    name: String,
    id: Oid,
    /// stash のメッセージ（`WIP on main: 1234567 件名`・`On main: メッセージ`）。
    detail: Option<String>,
    /// 作ったときのブランチ（メッセージの `WIP on main:`・`On main:` から。detached HEAD なら `(no branch)`）。
    branch: Option<String>,
    /// 見出し: `-m` のメッセージ。なければ WIP と、作ったときの HEAD の件名。
    title: String,
}

impl Stash {
    fn new(name: &str, id: Oid, detail: Option<&str>) -> Self {
        let message = detail.unwrap_or_default();
        let wip = message.strip_prefix("WIP on ");
        let parsed = wip
            .or_else(|| message.strip_prefix("On "))
            .and_then(|rest| rest.split_once(": "));
        let (branch, title) = match parsed {
            Some((branch, rest)) => {
                // WIP の残りは「短い ID 件名」なので、ID は外す（土台は 2 行目に出す）。
                let title = if wip.is_some() {
                    format!("WIP · {}", rest.split_once(' ').map_or("", |(_, s)| s))
                } else {
                    rest.to_owned()
                };
                (Some(branch.to_owned()), title)
            }
            None => (None, message.to_owned()),
        };
        Self {
            name: name.to_owned(),
            id,
            detail: detail.map(str::to_owned),
            branch,
            title,
        }
    }
}

/// stash のコミットから読む分。
#[derive(Clone)]
struct Info {
    time: i64,
    /// 3 つ目の親が untracked のファイル（`git stash -u`）。
    untracked: bool,
    /// 土台（1 つ目の親）。
    base: Option<String>,
}

/// 土台と、今の HEAD の離れ具合（`stashBases` の答え）。
#[derive(Clone, Copy, PartialEq)]
struct Base {
    /// HEAD にだけあるコミットの数。
    ahead: u64,
    /// 土台にだけあるコミットの数（0 でなければ、土台は HEAD の履歴にない）。
    behind: u64,
}

pub struct Stashes {
    pub root: gtk::Paned,
    inner: Rc<Inner>,
}

struct Inner {
    actions: RefCell<Option<Rc<Actions>>>,
    repo: RefCell<Option<Arc<Repository>>>,
    model: gtk::StringList,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    stashes: RefCell<Vec<Stash>>,
    infos: RefCell<HashMap<Oid, Info>>,
    bases: RefCell<HashMap<String, Base>>,
    bases_request: Cell<u64>,
    /// 今のブランチ。ほかのブランチで作った stash は、ブランチの名前を薄くして見分ける。
    current_branch: RefCell<Option<String>>,
    buttons: Vec<gtk::Button>,
    details: Rc<Details>,
    diff: DiffWidget,
    right: gtk::Paned,
    /// 裏で読んでいる数（`--snapshot` はなくなるまで待つ）。
    pending: Rc<Cell<u32>>,
    /// 選び直したら増やし、古い結果を捨てる。
    generation: Cell<u64>,
    /// 行を作り直している間は、選択の変化を知らせない。
    rebuilding: Cell<bool>,
}

impl Stashes {
    pub fn new(pending: Rc<Cell<u32>>) -> Self {
        let model = gtk::StringList::new(&[]);
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let list = gtk::ListView::new(Some(selection.clone()), None::<gtk::ListItemFactory>);
        list.add_css_class("navigation-sidebar");
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        // 一覧の上の帯: Apply・Pop・Drop…・Branch…。
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.set_margin_start(8);
        bar.set_margin_end(8);
        let mut buttons = Vec::new();
        for (title, tip) in [
            (
                "Apply".to_owned(),
                tr!("Apply the stash to the working tree and keep it in the list"),
            ),
            (
                "Pop".to_owned(),
                tr!("Apply the stash to the working tree and remove it from the list"),
            ),
            (
                tr!("Drop…"),
                tr!("Remove the stash without applying it (you can Undo it)"),
            ),
            (
                tr!("Branch…"),
                tr!(
                    "Create a branch at the commit the stash was made on, switch to it and apply the stash there (git stash branch)"
                ),
            ),
        ] {
            let button = gtk::Button::with_label(&title);
            button.set_tooltip_text(Some(&tip));
            bar.append(&button);
            buttons.push(button);
        }
        let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left.append(&bar);
        left.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        left.append(&scrolled);

        // 右の列: 選んだ stash の詳細と差分（History と同じ）。
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

        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.set_start_child(Some(&left));
        root.set_end_child(Some(&right));
        root.set_shrink_start_child(false);
        root.set_resize_start_child(false);
        root.set_position(360);

        let inner = Rc::new(Inner {
            actions: RefCell::default(),
            repo: RefCell::default(),
            model,
            selection,
            list,
            stashes: RefCell::default(),
            infos: RefCell::default(),
            bases: RefCell::default(),
            bases_request: Cell::new(0),
            current_branch: RefCell::default(),
            buttons,
            details,
            diff,
            right,
            pending,
            generation: Cell::new(0),
            rebuilding: Cell::new(false),
        });
        inner.list.set_factory(Some(&factory(&inner)));
        let weak = Rc::downgrade(&inner);
        inner.selection.connect_selected_notify(move |_| {
            if let Some(inner) = weak.upgrade()
                && !inner.rebuilding.get()
            {
                inner.show_selected();
            }
        });
        for (i, button) in inner.buttons.iter().enumerate() {
            let weak = Rc::downgrade(&inner);
            button.connect_clicked(move |_| {
                let Some(inner) = weak.upgrade() else { return };
                let (Some(stash), Some(a)) = (inner.selected(), inner.actions.borrow().clone())
                else {
                    return;
                };
                let (a, name, message) = (&a, &stash.name, stash.detail.as_deref());
                match i {
                    0 => sheets::pop(a, name, message, false),
                    1 => sheets::pop(a, name, message, true),
                    2 => confirm_drop(a, name, message),
                    _ => branch(a, name),
                }
            });
        }
        inner.update_buttons();
        Self { root, inner }
    }

    pub fn set_actions(&self, actions: Rc<Actions>) {
        self.inner.actions.replace(Some(actions));
    }

    pub fn set_repo(&self, repo: Arc<Repository>) {
        self.inner.repo.replace(Some(repo));
    }

    /// ref の一覧の stash（新しい順）を出す。選んでいた stash は ID で選び直す（番号は積むたびにずれる）。
    pub fn show(&self, refs: &RefsSnapshot) {
        let inner = &self.inner;
        let branch = refs.refs.head_branch.clone();
        let branch_changed = *inner.current_branch.borrow() != branch;
        inner.current_branch.replace(branch);
        let next: Vec<Stash> = refs
            .refs
            .entries
            .iter()
            .filter(|e| e.kind == RefKind::Stash)
            .filter_map(|e| Some(Stash::new(&e.name, e.target?, e.detail.as_deref())))
            .collect();
        if next == *inner.stashes.borrow() {
            if branch_changed {
                inner.refresh_rows();
            }
        } else {
            let selected = inner.selected().map(|s| s.id);
            let row = next
                .iter()
                .position(|s| Some(s.id) == selected)
                .or((!next.is_empty()).then_some(0));
            inner.stashes.replace(next);
            inner.refresh_rows();
            inner.rebuilding.set(true);
            inner
                .selection
                .set_selected(row.map_or(gtk::INVALID_LIST_POSITION, |r| r as u32));
            inner.rebuilding.set(false);
            // 選び直した行が前と同じ stash なら、中身は読み直さない。
            if inner.selected().map(|s| s.id) != selected {
                inner.show_selected();
            }
            inner.update_buttons();
            inner.load_infos();
        }
        // HEAD が動いたかもしれないので、土台との離れ具合は毎回読み直す。
        inner.reload_bases();
    }

    /// stash があるか（なければ History に移る）。
    pub fn is_empty(&self) -> bool {
        self.inner.stashes.borrow().is_empty()
    }

    /// この名前の stash を選ぶ（サイドバーで選んだとき）。
    pub fn select(&self, name: &str) {
        let at = self
            .inner
            .stashes
            .borrow()
            .iter()
            .position(|s| s.name == name);
        if let Some(at) = at {
            self.inner.selection.set_selected(at as u32);
        }
    }

    /// 選んでいる stash で、帯のボタン（0: Apply、1: Pop、2: Drop…、3: Branch…）を押す（確認用）。
    pub fn click(&self, button: usize) {
        if let Some(b) = self.inner.buttons.get(button) {
            b.emit_clicked();
        }
    }
}

impl Inner {
    fn selected(&self) -> Option<Stash> {
        let at = self.selection.selected();
        self.stashes.borrow().get(at as usize).cloned()
    }

    fn update_buttons(&self) {
        let any = self.selected().is_some();
        for button in &self.buttons {
            button.set_sensitive(any);
        }
    }

    /// 行を描き直させる（同じ名前を入れ替える。選択は保つ）。
    fn refresh_rows(&self) {
        let selected = self.selection.selected();
        let names: Vec<String> = self
            .stashes
            .borrow()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        self.rebuilding.set(true);
        self.model.splice(0, self.model.n_items(), &names);
        self.selection.set_selected(selected);
        self.rebuilding.set(false);
    }

    /// 選んだ stash の詳細（作者・メッセージ・土台）と中身（作業中の変更と untracked のファイル）を裏で読む。
    fn show_selected(self: &Rc<Self>) {
        self.update_buttons();
        let g = self.generation.get() + 1;
        self.generation.set(g);
        self.details.clear();
        self.diff.clear();
        let (Some(stash), Some(repo)) = (self.selected(), self.repo.borrow().clone()) else {
            return;
        };
        let id = stash.id;
        self.pending.set(self.pending.get() + 1);
        let (this, r) = (self.clone(), repo.clone());
        glib::spawn_future_local(async move {
            let info =
                gio::spawn_blocking(move || r.commit_info(&id).map_err(|e| e.to_string())).await;
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != g {
                return;
            }
            match info {
                Ok(Ok(info)) => {
                    this.details.show(&info);
                    crate::fit_details(&this.right, this.details.clone());
                }
                Ok(Err(e)) => eprintln!("stash の詳細を読めない: {e}"),
                Err(_) => {}
            }
        });
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let changes = repo.stash_changes(&id).map_err(|e| e.to_string())?;
                let stacked = repo.stacked_diff(&changes).map_err(|e| e.to_string())?;
                let view = diff::View::build(&stacked, &[]);
                Ok::<_, String>((Arc::new(diff::Source(stacked)), view))
            })
            .await;
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != g {
                return;
            }
            match result {
                Ok(Ok((source, view))) => this.diff.show(source, view),
                Ok(Err(e)) => eprintln!("stash の差分を読めない: {e}"),
                Err(_) => {}
            }
        });
    }

    /// まだ読んでいない stash のコミットの情報を裏で読む。
    fn load_infos(self: &Rc<Self>) {
        let missing: Vec<Oid> = {
            let infos = self.infos.borrow();
            self.stashes
                .borrow()
                .iter()
                .map(|s| s.id)
                .filter(|id| !infos.contains_key(id))
                .collect()
        };
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        if missing.is_empty() {
            return;
        }
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let loaded = gio::spawn_blocking(move || {
                missing
                    .into_iter()
                    .filter_map(|id| Some((id, info_of(&repo.commit_info(&id).ok()?))))
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            this.pending.set(this.pending.get() - 1);
            this.infos.borrow_mut().extend(loaded);
            this.refresh_rows();
        });
    }

    /// 土台と HEAD の離れ具合を読み直す。
    fn reload_bases(self: &Rc<Self>) {
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        if self.stashes.borrow().is_empty() {
            return;
        }
        let request = self.bases_request.get() + 1;
        self.bases_request.set(request);
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let reply = sheets::run(&repo, json!({ "op": "stashBases" })).await;
            this.pending.set(this.pending.get() - 1);
            if this.bases_request.get() != request {
                return;
            }
            let next: HashMap<String, Base> = reply["info"]["stashes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    let base = Base {
                        ahead: s["ahead"].as_u64()?,
                        behind: s["behind"].as_u64()?,
                    };
                    Some((s["name"].as_str()?.to_owned(), base))
                })
                .collect();
            if next != *this.bases.borrow() {
                this.bases.replace(next);
                this.refresh_rows();
            }
        });
    }

    /// 1 行目: `stash@{n}` とメッセージ。
    fn title_markup(stash: &Stash) -> String {
        format!(
            "<span alpha=\"60%\" font_features=\"tnum\">{}</span>  {}",
            glib::markup_escape_text(&stash.name),
            glib::markup_escape_text(&stash.title)
        )
    }

    /// 2 行目: 作ったブランチ・土台と HEAD の関係・いつ・untracked を含むか・土台の短い ID（狭ければ後ろから切れる）。
    fn subtitle_markup(&self, stash: &Stash) -> String {
        const SECONDARY: &str = "alpha=\"60%\"";
        const TERTIARY: &str = "alpha=\"38%\"";
        // 土台が今の HEAD の履歴にない（ほかの系統で作った）と、戻すときに重なりやすい。
        const ORANGE: &str = "foreground=\"#e66100\"";
        let mut parts: Vec<(String, &str)> = Vec::new();
        if let Some(branch) = &stash.branch {
            let current = self.current_branch.borrow().as_deref() == Some(branch.as_str());
            parts.push((branch.clone(), if current { SECONDARY } else { TERTIARY }));
        }
        if let Some(b) = self.bases.borrow().get(&stash.name) {
            if b.behind > 0 {
                parts.push((tr!("not in HEAD’s history"), ORANGE));
            } else if b.ahead == 0 {
                parts.push((tr!("at HEAD"), SECONDARY));
            } else {
                parts.push((tr!("{count} behind HEAD", count = b.ahead), SECONDARY));
            }
        }
        if let Some(info) = self.infos.borrow().get(&stash.id) {
            parts.push((ago(info.time), SECONDARY));
            if info.untracked {
                parts.push((tr!("with untracked files"), SECONDARY));
            }
            if let Some(base) = &info.base {
                parts.push((base[..base.len().min(7)].to_owned(), SECONDARY));
            }
        }
        parts
            .iter()
            .map(|(text, style)| format!("<span {style}>{}</span>", glib::markup_escape_text(text)))
            .collect::<Vec<_>>()
            .join(&format!("<span {TERTIARY}> · </span>"))
    }

    /// 行の右クリックのメニュー。
    fn menu_for(&self, name: &str) -> Vec<Item> {
        let stash = self
            .stashes
            .borrow()
            .iter()
            .find(|s| s.name == name)
            .cloned();
        match (stash, self.actions.borrow().as_ref()) {
            (Some(s), Some(a)) => menu(a, &s.name, s.detail.as_deref()),
            _ => Vec::new(),
        }
    }
}

/// 一覧の行を作る（見出しと、ブランチ・土台・いつの 2 行）。
fn factory(inner: &Rc<Inner>) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let weak = Rc::downgrade(inner);
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let label = |css: Option<&str>| {
            let l = gtk::Label::new(None);
            l.set_xalign(0.0);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_single_line_mode(true);
            if let Some(css) = css {
                l.add_css_class(css);
            }
            l
        };
        let title = label(None);
        let subtitle = label(Some("caption"));
        let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
        row.set_margin_top(3);
        row.set_margin_bottom(3);
        row.append(&title);
        row.append(&subtitle);
        item.set_child(Some(&row));

        // 右クリック: その行の stash の操作のメニュー。
        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_SECONDARY);
        let (w, list_item) = (weak.clone(), item.downgrade());
        click.connect_pressed(move |gesture, _, x, y| {
            let (Some(inner), Some(item)) = (w.upgrade(), list_item.upgrade()) else {
                return;
            };
            let Some(name) = item.item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            if let Some(widget) = gesture.widget() {
                menu::popup(&widget, x, y, inner.menu_for(&name.string()));
            }
        });
        row.add_controller(click);
    });
    let weak = Rc::downgrade(inner);
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let (Some(inner), Some(name)) = (
            weak.upgrade(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) else {
            return;
        };
        let row = item.child().and_downcast::<gtk::Box>().expect("row");
        let title = row
            .first_child()
            .and_downcast::<gtk::Label>()
            .expect("title");
        let subtitle = title
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("subtitle");
        let stash = inner
            .stashes
            .borrow()
            .iter()
            .find(|s| s.name == name.string())
            .cloned();
        let Some(stash) = stash else { return };
        title.set_markup(&Inner::title_markup(&stash));
        title.set_tooltip_text(stash.detail.as_deref());
        let sub = inner.subtitle_markup(&stash);
        subtitle.set_markup(&sub);
        subtitle.set_visible(!sub.is_empty());
    });
    factory
}

fn info_of(info: &Value) -> Info {
    let parents: Vec<&str> = info["parents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    Info {
        time: info["committerTime"].as_i64().unwrap_or_default(),
        untracked: parents.len() > 2,
        base: parents.first().map(|p| (*p).to_owned()),
    }
}

/// どのくらい前か（macOS 版の RelativeDateTimeFormatter の短い形）。
fn ago(time: i64) -> String {
    let now = glib::real_time() / 1_000_000;
    let s = (now - time).max(0);
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match s {
        _ if s < MIN => tr!("now"),
        _ if s < HOUR => tr!("{count} min. ago", count = s / MIN),
        _ if s < DAY => tr!("{count} hr. ago", count = s / HOUR),
        _ if s < 7 * DAY => tr!("{count} days ago", count = s / DAY),
        _ if s < 30 * DAY => tr!("{count} wk. ago", count = s / (7 * DAY)),
        _ if s < 365 * DAY => tr!("{count} mo. ago", count = s / (30 * DAY)),
        _ => tr!("{count} yr. ago", count = s / (365 * DAY)),
    }
}

/// stash の操作のメニュー（Stashes の一覧の行と、サイドバーの Stashes の項目）。
pub fn menu(actions: &Rc<Actions>, name: &str, message: Option<&str>) -> Vec<Item> {
    let mut items = Vec::new();
    for pop in [false, true] {
        let (a, n, m) = (actions.clone(), name.to_owned(), message.map(str::to_owned));
        let title = if pop {
            titles::pop(name)
        } else {
            titles::apply(name)
        };
        items.push(menu::action(title, move || {
            sheets::pop(&a, &n, m.as_deref(), pop)
        }));
    }
    let (a, n, m) = (actions.clone(), name.to_owned(), message.map(str::to_owned));
    items.push(menu::action(titles::drop(name), move || {
        confirm_drop(&a, &n, m.as_deref())
    }));
    let (a, n) = (actions.clone(), name.to_owned());
    items.push(menu::action(titles::new_branch_from(name), move || {
        branch(&a, &n)
    }));
    items.push(Item::Separator);
    let n = name.to_owned();
    items.push(menu::action(tr!("Copy Name"), move || {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(&n);
        }
    }));
    items
}

/// stash を消す前に確かめる（Undo で戻せる）。
pub fn confirm_drop(actions: &Rc<Actions>, name: &str, message: Option<&str>) {
    let mut text = message.map(|m| format!("“{m}”\n")).unwrap_or_default();
    text.push_str(&sheets::can_undo());
    sheets::confirm_then(
        actions,
        tr!("Drop {name}?", name = name),
        text,
        "Drop".to_owned(),
        false,
        vec![json!({ "op": "stashDrop", "name": name })],
    );
}

/// `git stash branch`: 土台から新しいブランチを作って切り替え、そこに戻す。HEAD が進んでいても重ならない。
pub fn branch(actions: &Rc<Actions>, name: &str) {
    let (actions, name) = (actions.clone(), name.to_owned());
    glib::spawn_future_local(async move {
        let Some(branch) = actions
            .ask(
                &tr!("New Branch from {name}", name = name),
                &tr!(
                    "Created where the stash was made, checked out, and the stash applied there (then dropped)."
                ),
                "",
                &tr!("Create"),
            )
            .await
        else {
            return;
        };
        actions.perform(json!({ "op": "stashBranch", "name": name, "branch": branch }));
    });
}
