//! Reflog の画面（macOS 版の `ReflogViewController`。docs/ux.md の「5b. Reflog」）。左に HEAD の reflog（新しい
//! 順）、右に選んだ行のコミットの中身（History の詳細と同じ欄）。reset・rebase・amend で外れたコミットを見つけて、
//! ブランチを作って取り戻すのに使う。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::{Oid, tr};
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::details::Details;
use crate::diff::{self, DiffWidget};
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
    details: Rc<Details>,
    diff: DiffWidget,
    right: gtk::Paned,
    /// 読み直しの番号（古い答えを捨てる）。
    request: Cell<u64>,
    /// 詳細と差分の番号（選び直したら古い結果を捨てる）。
    generation: Cell<u64>,
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
        let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left.append(&bar);
        left.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        left.append(&scrolled);

        // 右の列: History の詳細と同じ欄（詳細と差分）。
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
            details,
            diff,
            right,
            request: Cell::new(0),
            generation: Cell::new(0),
            stale: Cell::new(true),
            replacing: Cell::new(false),
            pending,
            on_show_in_history,
            range_diff: RefCell::default(),
            after_load: RefCell::default(),
        });
        inner.connect_factory(&factory);
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
        Self { root, inner }
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
            // 右クリックで、その行のメニュー（選択も移す）。
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
                inner.selection.set_selected(row);
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
            parts.push(format!("<span fgalpha=\"60%\">{}</span>", date(entry.time)));
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
        let g = self.generation.get() + 1;
        self.generation.set(g);
        self.details.clear();
        self.diff.clear();
        let (Some(repo), Some(id)) = (
            self.repo.borrow().clone(),
            self.selected().and_then(|e| Oid::from_hex(&e.new)),
        ) else {
            return;
        };
        // すぐ読める分を先に、時間のかかる分（署名の検証・含むブランチなど）と差分はその後に。
        for extras in [false, true] {
            self.pending.set(self.pending.get() + 1);
            let (this, repo) = (self.clone(), repo.clone());
            glib::spawn_future_local(async move {
                let value = gio::spawn_blocking(move || {
                    if extras {
                        repo.commit_extras(&id)
                    } else {
                        repo.commit_info(&id)
                    }
                    .map_err(|e| e.to_string())
                })
                .await;
                this.pending.set(this.pending.get() - 1);
                if this.generation.get() != g {
                    return;
                }
                match value {
                    Ok(Ok(value)) if extras => this.details.show_extras(&value),
                    Ok(Ok(value)) => this.details.show(&value),
                    Ok(Err(e)) => eprintln!("コミットの詳細を読めない: {e}"),
                    Err(_) => return,
                }
                crate::fit_details(&this.right, this.details.clone());
            });
        }
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            // History にないコミット（どのブランチにもない）もあるので、History の行ではなくコミットから読む。
            let result = gio::spawn_blocking(move || {
                let changes = repo.commit_changes(&id).map_err(|e| e.to_string())?;
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
                Ok(Err(e)) => eprintln!("差分を読めない: {e}"),
                Err(_) => {}
            }
        });
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
            format!("New Branch at {short}…"),
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
        items.extend(crate::commitops::reset(actions, &hex));
        // 今の HEAD と比べる（`<hex>...HEAD` の範囲で Range Diff。一番上の行は今の HEAD そのもの）。
        if row > 0
            && let Some(opener) = self.range_diff.borrow().clone()
        {
            items.push(Item::Separator);
            let range = format!("{hex}...HEAD");
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

fn date(time: i64) -> String {
    glib::DateTime::from_unix_local(time)
        .and_then(|d| d.format("%Y-%m-%d %H:%M"))
        .map(|s| s.to_string())
        .unwrap_or_default()
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
