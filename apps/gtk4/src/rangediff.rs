//! Range Diff: 同じ一連のコミットの 2 つの版（rebase の前と後など）を比べる（`git range-diff`）。macOS 版の
//! `RangeDiffViewController` と同じ並び: 左の上に比べる範囲（書き直して比べ直せる）、その下に組（同じ・変わった・
//! 消えた・足された）の一覧、右に選んだ組の差分の差分（変わった組）か、そのコミットの中身（History の詳細の列と
//! 同じ）。開いている間だけ、サイドバーの Workspace に Range Diff の行を出す。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_core::diff::DiffView;
use amonhen_model::{Oid, tr};
use amonhen_projection::FrontendProfile;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::details::Details;
use crate::diff::{self, DiffWidget};
use crate::history::HistoryView;
use crate::menu::{self, Item};
use crate::sidebar::{Page, Sidebar};

/// 組の片側（範囲の中の 1 始まりの番号と、コミット ID）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct Side {
    index: u64,
    oid: String,
}

/// 組 1 つ（`rangeDiff` の答えの `pairs` の 1 つ）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pair {
    /// `same`・`changed`・`removed`・`added`。
    status: String,
    old: Option<Side>,
    new: Option<Side>,
    subject: String,
    /// 差分の差分（changed のときだけ）。
    diff: Vec<String>,
}

impl Pair {
    fn parse(v: &Value) -> Self {
        let side = |v: &Value| {
            Some(Side {
                index: v["index"].as_u64()?,
                oid: v["oid"].as_str()?.to_owned(),
            })
        };
        Self {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            old: side(&v["old"]),
            new: side(&v["new"]),
            subject: v["subject"].as_str().unwrap_or_default().to_owned(),
            diff: v["diff"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l.as_str().map(str::to_owned))
                .collect(),
        }
    }
}

/// 組の印（git range-diff と同じ記号）、色、どうなったか。
fn mark(status: &str) -> (&'static str, Option<&'static str>, String) {
    match status {
        "changed" => ("!", Some("#e66100"), tr!("changed")),
        "removed" => ("<", Some("#c01c28"), tr!("removed")),
        "added" => (">", Some("#26a269"), tr!("added")),
        _ => ("=", None, tr!("same")),
    }
}

/// 40 桁の ID は短くして入力欄に出す（そのまま比べ直せる長さに）。
fn shorten(range: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.len() == 40 {
            out.push_str(&run[..12]);
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in range.chars() {
        if c.is_ascii_hexdigit() && !c.is_ascii_uppercase() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

fn date(time: i64) -> String {
    glib::DateTime::from_unix_local(time)
        .and_then(|d| d.format("%Y-%m-%d %H:%M"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn short(oid: &str) -> &str {
    crate::titles::short(oid)
}

pub struct RangeDiff {
    pub root: gtk::Paned,
    view: glib::WeakRef<HistoryView>,
    /// 裏で読んでいる数（`--snapshot` はなくなるまで待つ）。
    pending: Rc<Cell<u32>>,
    entry: gtk::Entry,
    summary: gtk::Label,
    counts: gtk::Label,
    changes_only: gtk::CheckButton,
    /// 一覧の行（中身は `shown` の同じ位置）。
    model: gtk::StringList,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    pairs: RefCell<Vec<Pair>>,
    /// 一覧に出している組（`changes_only` なら同じものを除く）。
    shown: Rc<RefCell<Vec<Pair>>>,
    /// 比べ直したら増やし、古い答えを捨てる。
    request: Cell<u64>,
    /// 右の欄（`pair`: 差分の差分、`commit`: コミットの中身）。
    right: gtk::Stack,
    pair_title: gtk::Label,
    pair_note: gtk::Label,
    pair_diff: DiffWidget,
    details: Rc<Details>,
    commit_diff: DiffWidget,
    commit_column: gtk::Paned,
    /// 選び直したら増やし、古い結果を捨てる。
    shown_generation: Rc<Cell<u64>>,
    /// 一覧を作り直している間は、選択の変化で右の欄を読み直さない。
    filling: Cell<bool>,
    on_show_in_history: RefCell<Option<crate::filetools::OnCommit>>,
    on_close: RefCell<Option<Rc<dyn Fn()>>>,
    /// 次に比べたら、この行を選ぶ・この行の右クリックのメニューを出す（確認用）。
    select_once: Cell<Option<u32>>,
    menu_once: Cell<Option<u32>>,
}

impl RangeDiff {
    pub fn new(view: &HistoryView, pending: Rc<Cell<u32>>) -> Rc<Self> {
        // 左: 範囲、説明、数と Changes Only、組の一覧。
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some(&tr!("old...new  or  base old new")));
        entry.set_tooltip_text(Some(&tr!(
            "The ranges to compare, written as for git range-diff"
        )));
        entry.add_css_class("monospace");
        entry.set_hexpand(true);
        let compare = gtk::Button::with_label(&tr!("Compare"));
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.set_tooltip_text(Some(&tr!("Close Range Diff")));
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.append(&entry);
        top.append(&compare);
        top.append(&close);

        let small = |css: &[&str]| {
            let l = gtk::Label::new(None);
            l.set_xalign(0.0);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            for c in css {
                l.add_css_class(c);
            }
            l
        };
        let summary = small(&["dim-label", "caption"]);
        // 失敗の文は長いことがあるので、3 行まで折り返す。
        summary.set_wrap(true);
        summary.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        summary.set_lines(3);
        summary.set_visible(false);
        let counts = small(&["dim-label", "caption"]);
        counts.set_hexpand(true);
        let changes_only = gtk::CheckButton::with_label(&tr!("Changes Only"));
        changes_only.set_tooltip_text(Some(&tr!(
            "Hide the commits that are the same in both versions"
        )));
        changes_only.add_css_class("caption");
        let second = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        second.append(&counts);
        second.append(&changes_only);

        let bar = gtk::Box::new(gtk::Orientation::Vertical, 4);
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.set_margin_start(8);
        bar.set_margin_end(8);
        bar.append(&top);
        bar.append(&summary);
        bar.append(&second);

        let model = gtk::StringList::new(&[]);
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let shown: Rc<RefCell<Vec<Pair>>> = Rc::default();
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

        // 右: 変わった組は件名と差分の差分、ほかはコミットの詳細と差分（History の右の列と同じ）。
        let pair_title = small(&["heading"]);
        let pair_note = small(&["dim-label", "caption"]);
        let header = gtk::Box::new(gtk::Orientation::Vertical, 2);
        header.set_margin_top(8);
        header.set_margin_bottom(8);
        header.set_margin_start(12);
        header.set_margin_end(12);
        header.append(&pair_title);
        header.append(&pair_note);
        let pair_diff = DiffWidget::default();
        let pair_scrolled = gtk::ScrolledWindow::builder()
            .child(&pair_diff)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let pair_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        pair_page.append(&header);
        pair_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        pair_page.append(&pair_scrolled);

        let details = Rc::new(Details::new());
        let commit_diff = DiffWidget::default();
        let commit_scrolled = gtk::ScrolledWindow::builder()
            .child(&commit_diff)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let commit_column = gtk::Paned::new(gtk::Orientation::Vertical);
        commit_column.set_start_child(Some(&details.root));
        commit_column.set_end_child(Some(&commit_scrolled));
        commit_column.set_resize_start_child(false);
        commit_column.set_position(170);

        let right = gtk::Stack::new();
        right.set_transition_type(gtk::StackTransitionType::None);
        right.add_named(&pair_page, Some("pair"));
        right.add_named(&commit_column, Some("commit"));

        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.set_start_child(Some(&left));
        root.set_end_child(Some(&right));
        root.set_shrink_start_child(false);
        root.set_resize_start_child(false);
        root.set_position(380);

        let this = Rc::new(Self {
            root,
            view: view.downgrade(),
            pending,
            entry,
            summary,
            counts,
            changes_only,
            model,
            selection,
            list,
            pairs: RefCell::default(),
            shown,
            request: Cell::new(0),
            right,
            pair_title,
            pair_note,
            pair_diff,
            details,
            commit_diff,
            commit_column,
            shown_generation: Rc::default(),
            filling: Cell::new(false),
            on_show_in_history: RefCell::default(),
            on_close: RefCell::default(),
            select_once: Cell::new(None),
            menu_once: Cell::new(None),
        });
        this.setup_rows(&factory);

        let weak = Rc::downgrade(&this);
        let compare_now = move || {
            if let Some(this) = weak.upgrade() {
                this.compare();
            }
        };
        let c = compare_now.clone();
        compare.connect_clicked(move |_| c());
        this.entry.connect_activate(move |_| compare_now());
        let weak = Rc::downgrade(&this);
        close.connect_clicked(move |_| {
            if let Some(f) = weak.upgrade().and_then(|t| t.on_close.borrow().clone()) {
                f();
            }
        });
        let weak = Rc::downgrade(&this);
        this.changes_only.connect_toggled(move |_| {
            if let Some(this) = weak.upgrade() {
                this.filter();
            }
        });
        let weak = Rc::downgrade(&this);
        this.selection.connect_selected_notify(move |_| {
            if let Some(this) = weak.upgrade()
                && !this.filling.get()
            {
                this.show_selected();
            }
        });
        this.show_pair(None);
        this
    }

    /// 組の行: 1 行目に印・古い番号 → 新しい番号・件名、2 行目にどうなったかと短い ID。右クリックで
    /// History で見る・ID をコピー。
    fn setup_rows(self: &Rc<Self>, factory: &gtk::SignalListItemFactory) {
        let weak = Rc::downgrade(self);
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let title = gtk::Label::new(None);
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let sub = gtk::Label::new(None);
            sub.set_xalign(0.0);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            sub.add_css_class("caption");
            sub.set_margin_start(16);
            let row = gtk::Box::new(gtk::Orientation::Vertical, 1);
            row.set_margin_top(2);
            row.set_margin_bottom(2);
            row.append(&title);
            row.append(&sub);
            item.set_child(Some(&row));

            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            let (weak, list_item) = (weak.clone(), item.downgrade());
            let row_weak = row.downgrade();
            click.connect_pressed(move |_, _, x, y| {
                let (Some(this), Some(item), Some(row)) =
                    (weak.upgrade(), list_item.upgrade(), row_weak.upgrade())
                else {
                    return;
                };
                let pair = this.shown.borrow().get(item.position() as usize).cloned();
                if let Some(pair) = pair {
                    menu::popup(&row, x, y, this.pair_menu(&pair, row.upcast_ref()));
                }
            });
            row.add_controller(click);
        });
        let shown = self.shown.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(row) = item.child() else { return };
            let title = row.first_child().and_downcast::<gtk::Label>().expect("title");
            let sub = title
                .next_sibling()
                .and_downcast::<gtk::Label>()
                .expect("sub");
            let shown = shown.borrow();
            let Some(pair) = shown.get(item.position() as usize) else {
                return;
            };
            let (symbol, color, word) = mark(&pair.status);
            let esc = |s: &str| glib::markup_escape_text(s).to_string();
            let colored = |text: &str| match color {
                Some(c) => format!("<span foreground=\"{c}\">{}</span>", esc(text)),
                None => format!("<span fgalpha=\"55%\">{}</span>", esc(text)),
            };
            let index = |s: &Option<Side>| s.as_ref().map_or("–".into(), |s| s.index.to_string());
            let subject = match pair.status.as_str() {
                "same" => format!("<span fgalpha=\"60%\">{}</span>", esc(&pair.subject)),
                "removed" => format!("<s>{}</s>", esc(&pair.subject)),
                _ => esc(&pair.subject),
            };
            title.set_markup(&format!(
                "<b><tt>{}</tt></b> <span fgalpha=\"60%\" font_features=\"tnum\">{} → {}</span>  {subject}",
                colored(symbol),
                index(&pair.old),
                index(&pair.new),
            ));
            title.set_tooltip_text(Some(&pair.subject));
            let id = |s: &Option<Side>| s.as_ref().map_or("–".to_owned(), |s| short(&s.oid).to_owned());
            sub.set_markup(&format!(
                "{} <span fgalpha=\"60%\">· <tt>{} → {}</tt></span>",
                colored(&word),
                id(&pair.old),
                id(&pair.new),
            ));
        });
    }

    /// 組の右クリックのメニュー（macOS 版と同じ）。
    fn pair_menu(&self, pair: &Pair, widget: &gtk::Widget) -> Vec<Item> {
        let mut items = Vec::new();
        for (side, title) in [
            (&pair.old, tr!("Show Old Version in History")),
            (&pair.new, tr!("Show New Version in History")),
        ] {
            let Some(id) = side.as_ref().and_then(|s| Oid::from_hex(&s.oid)) else {
                continue;
            };
            let f = self.on_show_in_history.borrow().clone();
            items.push(menu::action(title, move || {
                if let Some(f) = &f {
                    f(id);
                }
            }));
        }
        items.push(Item::Separator);
        for (side, title) in [
            (&pair.old, tr!("Copy Old Commit ID")),
            (&pair.new, tr!("Copy New Commit ID")),
        ] {
            let Some(oid) = side.as_ref().map(|s| s.oid.clone()) else {
                continue;
            };
            let w = widget.clone();
            items.push(menu::action(title, move || w.clipboard().set_text(&oid)));
        }
        items
    }

    /// 組の右クリックで「History で見る」を選んだら `f(コミット)` を呼ぶ。
    pub fn connect_show_in_history(&self, f: impl Fn(Oid) + 'static) {
        self.on_show_in_history.replace(Some(Rc::new(f)));
    }

    /// 次に比べたら `select` の行を選び、`menu` の行の右クリックのメニューを出す（確認用。メニューの項目は
    /// 標準エラーに書く）。
    pub fn debug_once(&self, select: Option<u32>, menu: Option<u32>) {
        self.select_once.set(select);
        self.menu_once.set(menu);
    }

    /// × を押したら `f()` を呼ぶ。
    pub fn connect_close(&self, f: impl Fn() + 'static) {
        self.on_close.replace(Some(Rc::new(f)));
    }

    fn repo(&self) -> Option<Arc<Repository>> {
        self.view.upgrade()?.current().map(|(r, _)| r)
    }

    fn next_request(&self) -> u64 {
        let r = self.request.get() + 1;
        self.request.set(r);
        r
    }

    /// `branch`（None なら今のブランチ）を、最後に書き換える前の版と比べる。
    pub fn open_previous(self: &Rc<Self>, branch: Option<String>) {
        let Some(repo) = self.repo() else { return };
        let request = self.next_request();
        self.set_summary(&tr!("Looking for the previous version…"));
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let reply = crate::sheets::run(
                &repo,
                json!({ "op": "previousVersion", "branch": branch.clone() }),
            )
            .await;
            if request == this.request.get() {
                let previous = &reply["info"]["previous"];
                if let Some(error) = reply["error"].as_str() {
                    this.show(Vec::new(), &[], Some(error.to_owned()));
                } else if previous.is_object() {
                    let ranges: Vec<String> = previous["ranges"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|r| r.as_str().map(str::to_owned))
                        .collect();
                    let text = tr!(
                        "{selector} → {branch} · {message} · {time}",
                        selector = previous["selector"].as_str().unwrap_or_default(),
                        branch = previous["branch"].as_str().unwrap_or_default(),
                        message = previous["message"].as_str().unwrap_or_default(),
                        time = date(previous["time"].as_i64().unwrap_or_default()),
                    );
                    this.open_ranges(ranges, Some(text));
                } else {
                    let text = tr!(
                        "{branch} hasn’t been rewritten (nothing in its reflog)",
                        branch = branch.as_deref().unwrap_or("HEAD")
                    );
                    this.show(Vec::new(), &[], Some(text));
                }
            }
            this.pending.set(this.pending.get() - 1);
        });
    }

    /// `ranges`（git range-diff と同じ形）で比べる。`summary` は範囲の下に出す説明。
    pub fn open_ranges(self: &Rc<Self>, ranges: Vec<String>, summary: Option<String>) {
        let Some(repo) = self.repo() else { return };
        let request = self.next_request();
        let text: Vec<String> = ranges.iter().map(|r| shorten(r)).collect();
        self.entry.set_text(&text.join(" "));
        self.set_summary(summary.as_deref().unwrap_or_default());
        self.counts.set_text(&tr!("Comparing…"));
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let reply =
                crate::sheets::run(&repo, json!({ "op": "rangeDiff", "ranges": ranges })).await;
            if request == this.request.get() {
                match reply["error"].as_str() {
                    Some(error) => this.show(Vec::new(), &ranges, Some(error.to_owned())),
                    None => {
                        let pairs = reply["info"]["pairs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(Pair::parse)
                            .collect();
                        this.show(pairs, &ranges, summary);
                    }
                }
            }
            this.pending.set(this.pending.get() - 1);
        });
    }

    /// 入力欄の範囲で比べ直す。
    fn compare(self: &Rc<Self>) {
        let ranges: Vec<String> = self
            .entry
            .text()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if !ranges.is_empty() {
            self.open_ranges(ranges, None);
        }
    }

    /// 範囲の下の説明（前の版の見つけ方・失敗）。なければ行を詰める。
    fn set_summary(&self, text: &str) {
        self.summary.set_text(text);
        self.summary.set_visible(!text.is_empty());
    }

    fn show(&self, pairs: Vec<Pair>, ranges: &[String], summary: Option<String>) {
        self.set_summary(summary.as_deref().unwrap_or_default());
        self.summary.set_tooltip_text(
            (!ranges.is_empty())
                .then(|| format!("git range-diff {}", ranges.join(" ")))
                .as_deref(),
        );
        let count = |status: &str| pairs.iter().filter(|p| p.status == status).count();
        let counts = if pairs.is_empty() && !ranges.is_empty() {
            tr!("No commits to compare")
        } else {
            [
                ("same", tr!("{count} same", count = count("same"))),
                ("changed", tr!("{count} changed", count = count("changed"))),
                ("removed", tr!("{count} removed", count = count("removed"))),
                ("added", tr!("{count} added", count = count("added"))),
            ]
            .into_iter()
            .filter(|(status, _)| count(status) > 0)
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join(" · ")
        };
        self.counts.set_text(&counts);
        self.pairs.replace(pairs);
        // 前に選んでいた組は選び直さない（比べ直したら最初の変わった組から）。
        self.shown.borrow_mut().clear();
        self.filter();
        if let Some(row) = self.select_once.take()
            && row < self.model.n_items()
        {
            self.selection.set_selected(row);
        }
        if let Some(row) = self.menu_once.take()
            && let Some(pair) = self.shown.borrow().get(row as usize).cloned()
        {
            let items = self.pair_menu(&pair, self.list.upcast_ref());
            eprintln!("{}", menu::titles(&items).join("\n"));
            self.pending.set(self.pending.get() + 1);
            let (list, pending) = (self.list.clone(), self.pending.clone());
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                menu::popup(&list, 160.0, 24.0 + row as f64 * 44.0, items);
                pending.set(pending.get() - 1);
            });
        }
    }

    /// Changes Only に合わせて一覧を作り直し、同じ組か最初の変わった組を選ぶ。
    fn filter(&self) {
        let selected = self
            .shown
            .borrow()
            .get(self.selection.selected() as usize)
            .cloned();
        let only = self.changes_only.is_active();
        let shown: Vec<Pair> = self
            .pairs
            .borrow()
            .iter()
            .filter(|p| !only || p.status != "same")
            .cloned()
            .collect();
        let row = selected
            .and_then(|s| shown.iter().position(|p| *p == s))
            .or_else(|| shown.iter().position(|p| p.status != "same"))
            .or((!shown.is_empty()).then_some(0));
        let n = shown.len();
        self.shown.replace(shown);
        self.filling.set(true);
        let rows: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &rows);
        match row {
            Some(r) => {
                self.selection.set_selected(r as u32);
                self.list
                    .scroll_to(r as u32, gtk::ListScrollFlags::NONE, None);
            }
            None => self.selection.set_selected(gtk::INVALID_LIST_POSITION),
        }
        self.filling.set(false);
        self.show_selected();
    }

    fn show_selected(&self) {
        let pair = self
            .shown
            .borrow()
            .get(self.selection.selected() as usize)
            .cloned();
        self.show_pair(pair.as_ref());
    }

    /// 右の欄: 変わった組は差分の差分、ほかはコミットそのもの（消えたものは古い版、ほかは新しい版）。
    fn show_pair(&self, pair: Option<&Pair>) {
        let g = self.shown_generation.get() + 1;
        self.shown_generation.set(g);
        self.pair_diff.clear();
        self.commit_diff.clear();
        self.details.clear();
        let Some(pair) = pair else {
            self.right.set_visible_child_name("pair");
            self.pair_title
                .set_text(&tr!("Choose two versions to compare"));
            self.pair_note.set_text("");
            return;
        };
        if pair.status != "changed" {
            self.right.set_visible_child_name("commit");
            let side = pair.new.as_ref().or(pair.old.as_ref());
            if let Some(id) = side.and_then(|s| Oid::from_hex(&s.oid)) {
                self.load_commit(id, g);
            }
            return;
        }
        self.right.set_visible_child_name("pair");
        self.pair_title.set_text(&pair.subject);
        let id = |s: &Option<Side>| s.as_ref().map_or("", |s| short(&s.oid)).to_owned();
        self.pair_note.set_text(&tr!(
            "{old} → {new} · the colored lines changed between the two versions",
            old = id(&pair.old),
            new = id(&pair.new),
        ));
        let lines = pair.diff.clone();
        self.pending.set(self.pending.get() + 1);
        let (pending, weak) = (self.pending.clone(), self.pair_diff.downgrade());
        let generation = self.shown_generation_ref();
        glib::spawn_future_local(async move {
            let view = gio::spawn_blocking(move || {
                let file = amonhen_core::diff::range_diff_file(&lines);
                diff::View::new(DiffView::new(&file, &FrontendProfile::GTK))
            })
            .await;
            pending.set(pending.get() - 1);
            if let (Ok(view), Some(diff)) = (view, weak.upgrade())
                && generation.upgrade().is_some_and(|c| c.get() == g)
            {
                diff.show_plain(view);
            }
        });
    }

    /// 選び直しの世代を、裏の読み込みから確かめるための弱い参照（画面を持ち続けない）。
    fn shown_generation_ref(&self) -> std::rc::Weak<Cell<u64>> {
        Rc::downgrade(&self.shown_generation)
    }

    /// コミットの詳細（すぐ読める分・時間のかかる分）と差分を読んで出す（History の右の列と同じ）。
    fn load_commit(&self, id: Oid, g: u64) {
        let Some(repo) = self.repo() else { return };
        let details = self.details.clone();
        let generation = self.shown_generation_ref();
        let column = self.commit_column.downgrade();
        let pending = self.pending.clone();
        let current = move || generation.upgrade().is_some_and(|c| c.get() == g);
        let fit = {
            let (column, details) = (column.clone(), details.clone());
            move || {
                if let Some(column) = column.upgrade() {
                    crate::fit_details(&column, details.clone());
                }
            }
        };
        pending.set(pending.get() + 3);
        let (r, p, d, c, f) = (
            repo.clone(),
            pending.clone(),
            details.clone(),
            current.clone(),
            fit.clone(),
        );
        glib::spawn_future_local(async move {
            let info =
                gio::spawn_blocking(move || r.commit_info(&id).map_err(|e| e.to_string())).await;
            p.set(p.get() - 1);
            match info {
                Ok(Ok(info)) if c() => {
                    d.show(&info);
                    f();
                }
                Ok(Err(e)) => eprintln!("コミットの詳細を読めない: {e}"),
                _ => {}
            }
        });
        let (r, p, d, c) = (repo.clone(), pending.clone(), details, current.clone());
        glib::spawn_future_local(async move {
            let extras =
                gio::spawn_blocking(move || r.commit_extras(&id).map_err(|e| e.to_string())).await;
            p.set(p.get() - 1);
            match extras {
                Ok(Ok(extras)) if c() => {
                    d.show_extras(&extras);
                    fit();
                }
                Ok(Err(e)) => eprintln!("コミットの詳細を読めない: {e}"),
                _ => {}
            }
        });
        let weak = self.commit_diff.downgrade();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let changes = repo.commit_changes(&id).map_err(|e| e.to_string())?;
                let stacked = repo.stacked_diff(&changes).map_err(|e| e.to_string())?;
                let view = diff::View::build(&stacked, &[]);
                Ok::<_, String>((Arc::new(diff::Source(stacked)), view))
            })
            .await;
            pending.set(pending.get() - 1);
            match result {
                Ok(Ok((source, view))) if current() => {
                    if let Some(diff) = weak.upgrade() {
                        diff.show(source, view);
                    }
                }
                Ok(Err(e)) => eprintln!("差分を読めない: {e}"),
                _ => {}
            }
        });
    }

    /// サイドバーの Range Diff の行の右クリックのメニュー。
    pub fn sidebar_menu(&self) -> Vec<Item> {
        let f = self.on_close.borrow().clone();
        vec![menu::action(tr!("Close Range Diff"), move || {
            if let Some(f) = &f {
                f();
            }
        })]
    }
}

/// ページの `gtk::Stack` での名前。
pub const PAGE: &str = "range-diff";

/// Range Diff を開く・閉じる（サイドバーの行を出し、ページを切り替える）。History・サイドバー・メニューのほか、
/// Reflog などの画面からも使う。窓の部品は弱い参照で持つ（閉包に入れても窓を生かし続けない）。
#[derive(Clone)]
pub struct Opener {
    view: std::rc::Weak<RangeDiff>,
    pages: glib::WeakRef<gtk::Stack>,
    sidebar: std::rc::Weak<Sidebar>,
}

impl Opener {
    pub fn new(view: &Rc<RangeDiff>, pages: &gtk::Stack, sidebar: &Rc<Sidebar>) -> Self {
        Self {
            view: Rc::downgrade(view),
            pages: pages.downgrade(),
            sidebar: Rc::downgrade(sidebar),
        }
    }

    fn present(&self) -> Option<Rc<RangeDiff>> {
        let (view, pages, sidebar) = (
            self.view.upgrade()?,
            self.pages.upgrade()?,
            self.sidebar.upgrade()?,
        );
        sidebar.set_range_diff_shown(true);
        sidebar.select_page(Page::RangeDiff);
        pages.set_visible_child_name(PAGE);
        Some(view)
    }

    /// `branch`（None なら今のブランチ）を、最後に書き換える前の版と比べる。
    pub fn previous(&self, branch: Option<String>) {
        if let Some(view) = self.present() {
            view.open_previous(branch);
        }
    }

    /// `ranges`（git range-diff と同じ形。`["A...B"]`・`["<base>", "A", "B"]` など）で比べる。
    pub fn ranges(&self, ranges: Vec<String>) {
        if let Some(view) = self.present() {
            view.open_ranges(ranges, None);
        }
    }

    /// 閉じて History に戻る（サイドバーから Range Diff の行を消す）。
    pub fn close(&self) {
        let (Some(pages), Some(sidebar)) = (self.pages.upgrade(), self.sidebar.upgrade()) else {
            return;
        };
        pages.set_visible_child_name("history");
        sidebar.select_page(Page::History);
        sidebar.set_range_diff_shown(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_full_ids_in_ranges() {
        let a = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            shorten(&format!("{a}...{a}")),
            "0123456789ab...0123456789ab"
        );
        assert_eq!(shorten("main...topic"), "main...topic");
        assert_eq!(shorten("abc1234"), "abc1234");
    }
}
