//! Reflog の画面（macOS 版の `ReflogViewController`。docs/ux.md の「5b. Reflog」）。左に HEAD の reflog（新しい
//! 順）、右に選んだ行のコミットの中身（History の詳細と同じ欄）。reset・rebase・amend で外れたコミットを見つけて、
//! ブランチを作って取り戻すのに使う。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::{Oid, tr};
use gtk::glib;
use gtk::prelude::*;
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::commitpane::CommitPane;
use crate::menu::{self, Item};

/// reflog の 1 行（コアの `reflog` の答えの `entries` の 1 つ）。
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    /// `HEAD@{n}`。
    selector: String,
    /// この動きの後に HEAD が指したコミット。
    new: String,
    time: i64,
    /// git の言葉（`commit: …`・`checkout: moving from a to b`・`reset: moving to …`）。
    message: String,
    subject: String,
    /// ref から辿れる（History にある）。
    reachable: bool,
}

impl Entry {
    fn same(&self, other: &Entry) -> bool {
        // 番号は動くたびにずれるので、コミットと日時とメッセージで同じ動きとみなす。
        self.new == other.new && self.time == other.time && self.message == other.message
    }
}

/// History でそのコミットを選ぶ。
pub type OnShowInHistory = Rc<dyn Fn(Oid)>;

pub struct ReflogView {
    pub root: gtk::Paned,
    inner: Rc<Inner>,
    target: gtk::Label,
}

struct Inner {
    actions: Rc<Actions>,
    repo: RefCell<Option<Arc<Repository>>>,
    entries: RefCell<Rc<Vec<Entry>>>,
    model: gtk::StringList,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    branch_button: gtk::Button,
    checkout_button: gtk::Button,
    /// 右の列（History の詳細と同じ）。
    pane: Rc<CommitPane>,
    /// 読み直しの番号（古い答えを捨てる）。
    request: Cell<u64>,
    /// 見えていない間に変わった（次に出すときに読み直す）。
    stale: Cell<bool>,
    /// 行を入れ替えている間（選択の変化を知らせない）。
    replacing: Cell<bool>,
    /// 裏で読んでいる数（`--snapshot` はなくなるまで待つ）。
    pending: Rc<Cell<u32>>,
    on_show_in_history: OnShowInHistory,
    /// Range Diff を開く（行の「今の HEAD と比べる」）。
    range_diff: RefCell<Option<crate::rangediff::Opener>>,
    /// 次に読み終えたら呼ぶ（確認用のフラグ）。
    after_load: RefCell<Vec<Box<dyn FnOnce()>>>,
}

impl ReflogView {
    /// 開いたリポジトリの場所（詳細の署名の吹き出しに、検証した場所を出す。History の詳細と同じ）。
    pub fn set_location(&self, location: &str) {
        self.inner.pane.details.set_location(location);
    }

    pub fn new(
        actions: Rc<Actions>,
        pending: Rc<Cell<u32>>,
        on_show_in_history: OnShowInHistory,
    ) -> Self {
        // 上の帯: 選んだ行のコミットにブランチを作る・detached でチェックアウトする（macOS 版と同じ言葉）。
        let branch_button = gtk::Button::with_label("New Branch…");
        branch_button.set_tooltip_text(Some(&tr!(
            "Create a branch at this commit and switch to it"
        )));
        let checkout_button = gtk::Button::with_label("Checkout");
        checkout_button.set_tooltip_text(Some(&tr!("Check out this commit as a detached HEAD")));
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.set_margin_start(8);
        bar.set_margin_end(8);
        for button in [&branch_button, &checkout_button] {
            button.set_sensitive(false);
            bar.append(button);
        }

        let model = gtk::StringList::new(&[]);
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let factory = gtk::SignalListItemFactory::new();
        let list = gtk::ListView::new(Some(selection.clone()), Some(factory.clone()));
        list.add_css_class("navigation-sidebar");
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        // worktree が 2 つ以上なら、どの worktree に効くかをボタンの後ろに小さく出す（長ければ真ん中を切る。
        // macOS 版と同じ）。
        let target = gtk::Label::new(None);
        target.set_hexpand(true);
        target.set_xalign(0.0);
        target.set_margin_start(6);
        target.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        target.add_css_class("dim-label");
        target.add_css_class("caption");
        bar.append(&target);
        let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left.append(&bar);
        left.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        left.append(&scrolled);

        // 右の列: History の詳細と同じ欄（詳細と、変更ファイルと差分）。
        let pane = CommitPane::new(pending.clone());
        pane.clear();
        pane.connect_restore(&actions);

        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.set_start_child(Some(&left));
        root.set_end_child(Some(&pane.root));
        root.set_resize_start_child(false);
        root.set_shrink_start_child(false);
        root.set_position(420);

        let inner = Rc::new(Inner {
            actions,
            repo: RefCell::default(),
            entries: RefCell::default(),
            model,
            selection,
            list,
            branch_button,
            checkout_button,
            pane,
            request: Cell::new(0),
            stale: Cell::new(true),
            replacing: Cell::new(false),
            pending,
            on_show_in_history,
            range_diff: RefCell::default(),
            after_load: RefCell::default(),
        });
        inner.connect_factory(&factory);
        // 詳細の親・子・メッセージの中の ID は、History でそのコミットを選ぶ（macOS 版と同じ）。
        inner
            .pane
            .connect_select_commit(inner.on_show_in_history.clone());
        let weak = Rc::downgrade(&inner);
        inner.selection.connect_selected_notify(move |_| {
            if let Some(inner) = weak.upgrade()
                && !inner.replacing.get()
            {
                inner.show_selected();
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.branch_button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade()
                && let Some(entry) = inner.selected()
            {
                crate::refops::ask_new_branch(&inner.actions, &entry.new);
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.checkout_button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade()
                && let Some(entry) = inner.selected()
            {
                crate::sheets::checkout(
                    &inner.actions,
                    crate::sheets::CheckoutTarget::Detach(entry.new),
                );
            }
        });
        // 出したときに、見えていない間の変化を読む。
        let weak = Rc::downgrade(&inner);
        root.connect_map(move |_| {
            if let Some(inner) = weak.upgrade()
                && inner.stale.get()
            {
                inner.reload();
            }
        });
        Self {
            root,
            inner,
            target,
        }
    }

    /// 書き込む先の worktree の名前（worktree が 2 つ以上あるときだけ）。どの HEAD の reflog かを帯に出す。
    pub fn set_worktree(&self, name: Option<&str>) {
        self.target.set_text(
            &name
                .map(|n| tr!("HEAD of ✎ {name}", name = n))
                .unwrap_or_default(),
        );
    }

    pub fn set_repo(&self, repo: Arc<Repository>) {
        self.inner.repo.replace(Some(repo));
    }

    /// reflog を読み直す（HEAD が動いたとき）。見えていなければ、次に出すときに読む。
    pub fn reload(&self) {
        self.inner.reload();
    }

    /// 次に読み終えたら `f` を呼ぶ（確認用のフラグ）。
    pub fn after_load(&self, f: impl FnOnce() + 'static) {
        self.inner.after_load.borrow_mut().push(Box::new(f));
    }

    /// `row` 行目を選ぶ（確認用）。
    pub fn select(&self, row: u32) {
        self.inner.selection.set_selected(row);
    }

    /// 行の「Compare with Current HEAD (Range Diff)」で使う。
    pub fn set_range_diff(&self, opener: crate::rangediff::Opener) {
        self.inner.range_diff.replace(Some(opener));
    }

    /// `row` 行目の右クリックのメニュー。
    pub fn menu(&self, row: u32) -> Vec<Item> {
        self.inner.menu(row)
    }

    /// `row` 行目の右クリックのメニューを出す（確認用）。
    pub fn popup_menu(&self, row: u32) {
        let items = self.inner.menu(row);
        let y = 50.0 + row as f64 * 52.0;
        menu::popup(&self.inner.list, 120.0, y, items);
    }
}

impl Inner {
    fn connect_factory(self: &Rc<Self>, factory: &gtk::SignalListItemFactory) {
        let weak = Rc::downgrade(self);
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            // 1 行目: 番号と、何をしたか（git reflog と同じ）。2 行目: どのブランチにも残っていないか・いつ・
            // 短い ID・件名（狭ければ後ろから切れる）。
            let title = gtk::Label::new(None);
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let sub = gtk::Label::new(None);
            sub.set_xalign(0.0);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            sub.add_css_class("caption");
            let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
            column.set_margin_top(4);
            column.set_margin_bottom(4);
            column.append(&title);
            column.append(&sub);
            // 右クリックで、その行のメニュー（選択は動かさない。macOS 版の clickedRow と同じ）。
            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            let (weak, list_item) = (weak.clone(), item.downgrade());
            click.connect_pressed(move |gesture, _, x, y| {
                let (Some(inner), Some(item)) = (weak.upgrade(), list_item.upgrade()) else {
                    return;
                };
                let row = item.position();
                if row == gtk::INVALID_LIST_POSITION {
                    return;
                }
                if let Some(widget) = gesture.widget() {
                    menu::popup(&widget, x, y, inner.menu(row));
                }
            });
            column.add_controller(click);
            item.set_child(Some(&column));
        });
        let weak = Rc::downgrade(self);
        factory.connect_bind(move |_, item| {
            let Some(inner) = weak.upgrade() else { return };
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let entries = inner.entries.borrow().clone();
            let (Some(entry), Some(column)) = (
                entries.get(item.position() as usize),
                item.child().and_downcast::<gtk::Box>(),
            ) else {
                return;
            };
            let title = column
                .first_child()
                .and_downcast::<gtk::Label>()
                .expect("title");
            let sub = title
                .next_sibling()
                .and_downcast::<gtk::Label>()
                .expect("sub");
            let esc = |s: &str| glib::markup_escape_text(s).to_string();
            let message = if entry.message.is_empty() {
                "—"
            } else {
                &entry.message
            };
            title.set_markup(&format!(
                "<span font_features=\"tnum\" fgalpha=\"60%\">{}</span>  {}",
                esc(&entry.selector),
                esc(message)
            ));
            title.set_tooltip_text(Some(&entry.message));
            let mut parts = Vec::new();
            // ブランチから外れたコミットは、ここからしか辿れない（reflog が切れると gc で消える）。
            if !entry.reachable {
                parts.push(format!(
                    "<span foreground=\"#e66100\">{}</span>",
                    esc(&tr!("not on any branch"))
                ));
            }
            parts.push(format!(
                "<span fgalpha=\"60%\">{}</span>",
                esc(&crate::stashes::ago(entry.time))
            ));
            parts.push(format!(
                "<span fgalpha=\"60%\">{}</span>",
                esc(&entry.new[..entry.new.len().min(7)])
            ));
            if !entry.subject.is_empty() {
                parts.push(format!(
                    "<span fgalpha=\"60%\">{}</span>",
                    esc(&entry.subject)
                ));
            }
            sub.set_markup(&parts.join("<span fgalpha=\"40%\"> · </span>"));
            sub.set_tooltip_text(Some(&entry.subject));
        });
    }

    fn reload(self: &Rc<Self>) {
        let Some(repo) = self.repo.borrow().clone() else {
            return;
        };
        let root = self.list.ancestor(gtk::Paned::static_type());
        if !root.is_some_and(|r| r.is_mapped()) {
            self.stale.set(true);
            return;
        }
        self.stale.set(false);
        let request = self.request.get() + 1;
        self.request.set(request);
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let reply = crate::sheets::run(&repo, json!({ "op": "reflog", "limit": 500 })).await;
            this.pending.set(this.pending.get() - 1);
            if this.request.get() != request {
                return;
            }
            if reply["ok"].as_bool() != Some(true) {
                eprintln!("reflog を読めない: {}", reply["error"]);
            }
            this.show(entries_of(&reply["info"]));
            for f in this.after_load.take() {
                f();
            }
        });
    }

    /// 行を入れ替える。選んでいた行は、同じ動きを選び直す（なければ先頭）。
    fn show(self: &Rc<Self>, next: Vec<Entry>) {
        if **self.entries.borrow() == next {
            return;
        }
        let previous = self.selected();
        let row = previous
            .as_ref()
            .and_then(|p| next.iter().position(|e| e.same(p)))
            .or((!next.is_empty()).then_some(0));
        let changed = match (&previous, row) {
            (Some(p), Some(r)) => !next[r].same(p),
            _ => true,
        };
        let count = next.len();
        self.entries.replace(Rc::new(next));
        self.replacing.set(true);
        let indices: Vec<String> = (0..count).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = indices.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &refs);
        self.selection
            .set_selected(row.map_or(gtk::INVALID_LIST_POSITION, |r| r as u32));
        self.replacing.set(false);
        // 選び直した行が前と同じ動きなら、詳細はそのまま。
        if changed {
            self.show_selected();
        }
        self.update_buttons();
    }

    fn selected(&self) -> Option<Entry> {
        self.entries
            .borrow()
            .get(self.selection.selected() as usize)
            .cloned()
    }

    fn update_buttons(&self) {
        let any = self.selected().is_some();
        self.branch_button.set_sensitive(any);
        self.checkout_button.set_sensitive(any);
    }

    /// 選んだ行のコミットの詳細と差分を、裏で読んで出す。
    fn show_selected(self: &Rc<Self>) {
        self.update_buttons();
        match (
            self.repo.borrow().clone(),
            self.selected().and_then(|e| Oid::from_hex(&e.new)),
        ) {
            (Some(repo), Some(id)) => self.pane.show(repo, id, false),
            _ => self.pane.clear(),
        }
    }

    /// 行の右クリックのメニュー（macOS 版の `menuNeedsUpdate` と同じ並び）。
    fn menu(self: &Rc<Self>, row: u32) -> Vec<Item> {
        let Some(entry) = self.entries.borrow().get(row as usize).cloned() else {
            return Vec::new();
        };
        let hex = entry.new.clone();
        let short = crate::titles::short(&hex).to_owned();
        let actions = &self.actions;
        let mut items = Vec::new();
        let (a, h) = (actions.clone(), hex.clone());
        items.push(menu::action(
            crate::titles::new_branch_from(&short),
            move || {
                crate::refops::ask_new_branch(&a, &h);
            },
        ));
        let (a, h) = (actions.clone(), hex.clone());
        items.push(menu::action(
            tr!("New Tag at {name}…", name = short),
            move || crate::refops::ask_new_tag(&a, &h),
        ));
        let (a, h) = (actions.clone(), hex.clone());
        items.push(menu::action(
            crate::titles::checkout_detached(&hex),
            move || {
                crate::sheets::checkout(&a, crate::sheets::CheckoutTarget::Detach(h.clone()));
            },
        ));
        items.push(crate::commitops::reset(actions, &hex));
        // 今の HEAD と比べる（Range Diff）。今の HEAD は一番上の行のコミット（Range Diff は開いたリポジトリで動くので、
        // `HEAD` と書くと、ほかの worktree を選んでいるときに別の HEAD になる。macOS 版と同じ）。
        if row > 0
            && let Some(opener) = self.range_diff.borrow().clone()
            && let Some(head) = self.entries.borrow().first().map(|e| e.new.clone())
        {
            items.push(Item::Separator);
            let range = format!("{hex}...{head}");
            items.push(menu::action(
                tr!("Compare with Current HEAD (Range Diff)"),
                move || opener.ranges(vec![range.clone()]),
            ));
        }
        if entry.reachable
            && let Some(id) = Oid::from_hex(&hex)
        {
            items.push(Item::Separator);
            let show = self.on_show_in_history.clone();
            items.push(menu::action(tr!("Show in History"), move || show(id)));
        }
        items.push(Item::Separator);
        let (w, h) = (self.list.clone(), hex);
        items.push(menu::action(tr!("Copy Commit ID"), move || {
            w.clipboard().set_text(&h);
        }));
        let (w, name) = (self.list.clone(), entry.selector);
        items.push(menu::action(tr!("Copy Name"), move || {
            w.clipboard().set_text(&name);
        }));
        items
    }
}

/// コアの `reflog` の答えの行。
fn entries_of(info: &Value) -> Vec<Entry> {
    info["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Entry {
            selector: e["selector"].as_str().unwrap_or_default().to_owned(),
            new: e["new"].as_str().unwrap_or_default().to_owned(),
            time: e["time"].as_i64().unwrap_or_default(),
            message: e["message"].as_str().unwrap_or_default().to_owned(),
            subject: e["subject"].as_str().unwrap_or_default().to_owned(),
            reachable: e["reachable"].as_bool().unwrap_or(true),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// コアの答えの行を読み、どのブランチにもない行は reachable が false になる。
    #[test]
    fn reads_the_core_reply() {
        let info = json!({ "entries": [
            { "selector": "HEAD@{0}", "old": "a", "new": "b", "time": 1, "message": "reset: moving to HEAD~1",
              "subject": "s", "reachable": true },
            { "selector": "HEAD@{1}", "old": null, "new": "c", "time": 0, "message": "commit: x",
              "subject": "", "reachable": false },
        ]});
        let entries = entries_of(&info);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].selector, "HEAD@{0}");
        assert!(!entries[1].reachable);
        assert!(entries[0].same(&Entry {
            selector: "HEAD@{3}".into(),
            ..entries[0].clone()
        }));
    }
}
