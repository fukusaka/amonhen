//! マージエディタ（macOS 版の `MergeEditor.swift`）: コンフリクトのあるファイル 1 つを解決する窓。左に Current、
//! 真ん中に編集できる Result（今の作業ツリーの内容から始める）、右に Incoming（左右は、すべてのコンフリクトを
//! その側にした内容）。共通の祖先（diff3）があれば、Base を左端に出せる。
//!
//! 選んだコンフリクトを Accept Current・Incoming・Both・Base で解き、✨ Magic Merge は決まった規則で解ける
//! コンフリクトをまとめて解く（どれも Result の変わった所だけを置き換えるので、Ctrl+Z で 1 つずつ戻せる）。
//! 4 つの欄のスクロールは、コンフリクトの位置で揃える。解決の計算はコアの `amonhen_core::conflict`（文字だけを
//! 扱うので、daemon 越しのリポジトリでも手元で計算する）、読むのは `mergeSides`、書くのは `saveMerge`。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use amonhen_core::Repository;
use amonhen_core::conflict::{self, Choice, MagicRules};
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;

/// 欄の一方の中身（すべてのコンフリクトをその側にした文章と、各コンフリクトの行範囲）。
#[derive(Default)]
struct Side {
    text: String,
    regions: Vec<(usize, usize)>,
}

impl Side {
    fn from(v: &Value) -> Option<Self> {
        let text = v["text"].as_str()?.to_owned();
        let regions = v["regions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| Some((r[0].as_u64()? as usize, r[1].as_u64()? as usize)))
            .collect();
        Some(Self { text, regions })
    }
}

/// Result に残っているコンフリクト（マーカーを含む行範囲と、両側の中身）。
#[derive(Clone, PartialEq)]
struct Region {
    lines: (usize, usize),
    current: String,
    incoming: String,
}

fn regions(text: &str) -> Vec<Region> {
    let data = text.as_bytes();
    let ls = conflict::line_ranges(data);
    let join = |r: &std::ops::Range<usize>| -> String {
        r.clone()
            .map(|i| String::from_utf8_lossy(&data[ls[i].clone()]))
            .collect()
    };
    conflict::parse(data)
        .iter()
        .map(|c| Region {
            lines: (c.lines.start, c.lines.end),
            current: join(&c.ours),
            incoming: join(&c.theirs),
        })
        .collect()
}

/// 1 つの欄（見出しと、折り返さない等幅の文字の欄）。
struct Pane {
    root: gtk::Box,
    view: gtk::TextView,
    scrolled: gtk::ScrolledWindow,
}

impl Pane {
    fn new(title: &str, editable: bool, color: &str) -> Self {
        let label = gtk::Label::new(Some(title));
        label.set_xalign(0.0);
        label.add_css_class("heading");
        label.add_css_class("caption");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_margin_start(6);
        let view = gtk::TextView::builder()
            .monospace(true)
            .editable(editable)
            .cursor_visible(true)
            .wrap_mode(gtk::WrapMode::None)
            .left_margin(4)
            .build();
        let buffer = view.buffer();
        let rgba = |alpha: f32| {
            let mut c = gdk::RGBA::parse(color).unwrap_or(gdk::RGBA::BLACK);
            c.set_alpha(alpha);
            c
        };
        buffer.create_tag(
            Some("conflict"),
            &[("paragraph-background-rgba", &rgba(0.12))],
        );
        buffer.create_tag(
            Some("selected"),
            &[("paragraph-background-rgba", &rgba(0.32))],
        );
        buffer.create_tag(Some("resolved"), &[("foreground-rgba", &rgba(0.45))]);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.append(&label);
        root.append(&scrolled);
        Self {
            root,
            view,
            scrolled,
        }
    }

    fn text(&self) -> String {
        let b = self.view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), true).into()
    }

    fn line_iter(&self, line: usize) -> gtk::TextIter {
        let b = self.view.buffer();
        b.iter_at_line(line as i32).unwrap_or_else(|| b.end_iter())
    }

    /// 行範囲 `[start, end)` に `tag` を付ける（空の範囲は、その位置の 1 行に付ける）。
    fn tag(&self, tag: &str, (start, end): (usize, usize)) {
        let end = end.max(start + 1);
        self.view
            .buffer()
            .apply_tag_by_name(tag, &self.line_iter(start), &self.line_iter(end));
    }

    fn clear(&self) {
        let b = self.view.buffer();
        b.remove_all_tags(&b.start_iter(), &b.end_iter());
    }

    /// 一番上に見えている行（小数は行の中の位置）。
    fn top_line(&self) -> f64 {
        let y = self.scrolled.vadjustment().value();
        let (iter, top) = self.view.line_at_y(y as i32);
        let (_, height) = self.view.line_yrange(&iter);
        iter.line() as f64
            + if height > 0 {
                (y - top as f64) / height as f64
            } else {
                0.0
            }
    }

    /// 行 `line`（小数は行の中の位置）が一番上に来るようにスクロールする。
    fn scroll_to_line(&self, line: f64) {
        let iter = self.line_iter(line.max(0.0).floor() as usize);
        let (top, height) = self.view.line_yrange(&iter);
        let adj = self.scrolled.vadjustment();
        let y = top as f64 + line.fract().max(0.0) * height as f64;
        adj.set_value(y.clamp(0.0, (adj.upper() - adj.page_size()).max(0.0)));
    }

    fn cursor_line(&self) -> usize {
        let b = self.view.buffer();
        b.iter_at_offset(b.cursor_position()).line() as usize
    }
}

/// 区切り（`from` の行, `to` の行）で、`from` の行 `line` に当たる `to` の行（間は比例で割り当てる）。
fn map(line: f64, from: &[usize], to: &[usize]) -> f64 {
    if from.is_empty() || from.len() != to.len() {
        return line;
    }
    if line <= from[0] as f64 {
        return line - from[0] as f64 + to[0] as f64;
    }
    for k in 1..from.len() {
        if line <= from[k] as f64 {
            let (f0, f1, t0, t1) = (
                from[k - 1] as f64,
                from[k] as f64,
                to[k - 1] as f64,
                to[k] as f64,
            );
            return if f1 == f0 {
                t1
            } else {
                t0 + (line - f0) * (t1 - t0) / (f1 - f0)
            };
        }
    }
    line - *from.last().unwrap() as f64 + *to.last().unwrap() as f64
}

struct Editor {
    window: gtk::Window,
    actions: Rc<Actions>,
    path: String,
    current: Pane,
    result: Pane,
    incoming: Pane,
    base: Pane,
    sides: [Option<Side>; 3],
    /// 最初の Result に入っていたコンフリクト（両側の中身）。添字が左右の欄の `regions` の添字に当たる。
    originals: Vec<(String, String)>,
    /// 今の Result に残っているコンフリクトと、それぞれが `originals` の何番目か。
    conflicts: RefCell<Vec<Region>>,
    original_of: RefCell<Vec<Option<usize>>>,
    selected: Cell<usize>,
    /// 最後に保存した Result（閉じるときに、保存していない変更があるか確かめる）。
    saved: RefCell<String>,
    /// ほかの欄をスクロールで揃えている間（揃えた結果で揃え返さない）。
    syncing: Cell<bool>,
    /// 打つたびに読み直さないよう、少し待つ。
    reparse: RefCell<Option<glib::SourceId>>,
    position: gtk::Label,
    note: gtk::Label,
    buttons: Vec<gtk::Button>,
    accept_base: Option<gtk::Button>,
    /// AI の解消・解説のボタン（コンフリクトがあり、AI を使えるときだけ押せる）。
    ai_buttons: Vec<gtk::Button>,
    /// AI の解消の提案の帯と、出している提案（そのときのコンフリクトと、提案）。
    band: Band,
    proposal: RefCell<Option<(Region, amonhen_assist::Proposal)>>,
    ai_cancel: RefCell<Arc<AtomicBool>>,
    on_resolved: Box<dyn Fn()>,
}

thread_local! {
    /// 開いているマージエディタ（パスごとに 1 つ）。
    static OPEN: RefCell<Vec<(String, glib::WeakRef<gtk::Window>)>> = RefCell::default();
    /// 次に開いたとき、最初のコンフリクトをこれで解く（`--accept`。確認用）。
    static ACCEPT_ONCE: Cell<Option<Choice>> = const { Cell::new(None) };
    /// 開いているマージエディタ（窓を閉じたら外す）。
    static KEEP: RefCell<Vec<Rc<Editor>>> = RefCell::default();
}

/// 次に開いたとき、最初のコンフリクトを `choice`（`ours`・`theirs`・`both`・`base`）で解く（確認用）。
pub fn accept_once(choice: &str) {
    let choice = match choice {
        "theirs" => Choice::Theirs,
        "both" => Choice::Both,
        "base" => Choice::Base,
        _ => Choice::Ours,
    };
    ACCEPT_ONCE.with(|a| a.set(Some(choice)));
}

/// `path` のマージエディタを開く（開いていれば前に出す）。保存して解決済みにしたら `on_resolved` を呼ぶ。
pub fn open(
    parent: &gtk::Window,
    repo: Arc<Repository>,
    actions: Rc<Actions>,
    path: &str,
    on_resolved: impl Fn() + 'static,
) {
    let existing = OPEN.with(|o| {
        o.borrow()
            .iter()
            .find(|(p, _)| p == path)
            .and_then(|(_, w)| w.upgrade())
    });
    if let Some(w) = existing {
        w.present();
        return;
    }
    let (parent, path) = (parent.clone(), path.to_owned());
    glib::spawn_future_local(async move {
        let (r, p) = (repo.clone(), path.clone());
        let reply: Value = gio::spawn_blocking(move || {
            let command = json!({ "op": "mergeSides", "path": p });
            serde_json::from_str(&r.run_json(&command.to_string())).unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        if let Some(error) = reply["error"].as_str() {
            let dialog = gtk::AlertDialog::builder()
                .message(tr!("Can’t open the merge editor"))
                .detail(error)
                .modal(true)
                .build();
            let _ = dialog.choose_future(Some(&parent)).await;
            return;
        }
        let info = &reply["info"];
        let sides = [
            Side::from(&info["current"]),
            Side::from(&info["incoming"]),
            Side::from(&info["base"]),
        ];
        let result = info["result"].as_str().unwrap_or_default().to_owned();
        let editor = Editor::new(
            &parent,
            actions,
            &path,
            sides,
            result,
            Box::new(on_resolved),
        );
        OPEN.with(|o| {
            let mut o = o.borrow_mut();
            o.retain(|(_, w)| w.upgrade().is_some());
            o.push((path, editor.window.downgrade()));
        });
        // 窓がある間は持っておく（ボタンなどは弱い参照で呼ぶので、持たなければ開いた直後に消える）。
        KEEP.with(|k| k.borrow_mut().push(editor.clone()));
        let weak = Rc::downgrade(&editor);
        editor.window.connect_destroy(move |_| {
            KEEP.with(|k| {
                k.borrow_mut()
                    .retain(|e| !std::ptr::eq(Rc::as_ptr(e), weak.as_ptr()))
            });
        });
        editor.window.present();
        Editor::opened(&editor);
        if let Some(choice) = ACCEPT_ONCE.with(|a| a.take()) {
            editor.resolve(editor.selected.get(), choice);
        }
    });
}

impl Editor {
    fn new(
        parent: &gtk::Window,
        actions: Rc<Actions>,
        path: &str,
        sides: [Option<Side>; 3],
        result: String,
        on_resolved: Box<dyn Fn()>,
    ) -> Rc<Self> {
        let current = Pane::new(&tr!("Current (your branch)"), false, "#3584e4");
        let result_pane = Pane::new(&tr!("Result (editable)"), true, "#e01b24");
        let incoming = Pane::new(&tr!("Incoming (being merged in)"), false, "#2ec27e");
        let base = Pane::new(&tr!("Base (common ancestor)"), false, "#9a9996");
        let has_base = sides[2].is_some();

        let position = gtk::Label::new(None);
        position.add_css_class("numeric");
        let previous = gtk::Button::from_icon_name("go-up-symbolic");
        previous.set_tooltip_text(Some(&tr!("Previous Conflict (Ctrl+Alt+↑)")));
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        next.set_tooltip_text(Some(&tr!("Next Conflict (Ctrl+Alt+↓)")));
        let magic = gtk::Button::with_label("✨ Magic Merge");
        magic.set_tooltip_text(Some(&tr!(
            "Resolve all conflicts that fixed rules can solve: both sides are the same, only one side changed (diff3), only whitespace differs, or different words on the same line changed (diff3). You can Undo it."
        )));
        let ai_resolve = gtk::Button::with_label(&tr!("Resolve with AI"));
        let ai_explain = gtk::Button::with_label(&tr!("Explain with AI"));
        crate::assist::track(
            &ai_resolve,
            tr!(
                "AI: Suggest a resolution for the selected conflict (Result doesn’t change until you accept it)."
            ),
        );
        crate::assist::track(
            &ai_explain,
            tr!("AI: Explain what each side of the selected conflict is trying to do."),
        );
        let note = gtk::Label::new(None);
        note.add_css_class("dim-label");
        note.set_ellipsize(gtk::pango::EllipsizeMode::End);
        note.set_hexpand(true);
        note.set_xalign(0.0);
        let mut accepts = vec![
            ("Accept Current", Choice::Ours),
            ("Accept Incoming", Choice::Theirs),
            ("Accept Both", Choice::Both),
        ];
        if has_base {
            accepts.push(("Accept Base", Choice::Base));
        }
        let accept_buttons: Vec<(gtk::Button, Choice)> = accepts
            .into_iter()
            .map(|(title, choice)| {
                (
                    gtk::Button::with_label(&amonhen_model::i18n::translate(title)),
                    choice,
                )
            })
            .collect();
        let base_toggle = gtk::CheckButton::with_label("Base");
        base_toggle.set_tooltip_text(Some(&tr!("Show the common ancestor on the left.")));
        base_toggle.set_visible(has_base);
        let save = gtk::Button::with_label(&tr!("Save"));
        save.set_tooltip_text(Some(&tr!("Save (Ctrl+S)")));
        let resolve = gtk::Button::with_label(&tr!("Save & Mark Resolved"));
        resolve.add_css_class("suggested-action");
        resolve.set_tooltip_text(Some(&tr!("Save and mark as resolved (Ctrl+Return).")));

        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.set_margin_start(10);
        bar.set_margin_end(10);
        bar.append(&position);
        bar.append(&previous);
        bar.append(&next);
        bar.append(&magic);
        bar.append(&ai_resolve);
        bar.append(&ai_explain);
        bar.append(&note);
        for (b, _) in &accept_buttons {
            bar.append(b);
        }
        bar.append(&base_toggle);
        bar.append(&save);
        bar.append(&resolve);

        let columns = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        columns.set_homogeneous(true);
        columns.set_margin_start(6);
        columns.set_margin_end(6);
        columns.set_margin_bottom(6);
        base.root.set_visible(false);
        for p in [&base, &current, &result_pane, &incoming] {
            columns.append(&p.root);
        }
        let band = Band::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&bar);
        content.append(&band.root);
        content.append(&columns);
        let window = gtk::Window::builder()
            .title(format!("Merge — {path}"))
            .default_width(1400)
            .default_height(800)
            .child(&content)
            .transient_for(parent)
            .destroy_with_parent(true)
            .build();

        let mut buttons = vec![previous.clone(), next.clone(), magic.clone()];
        buttons.extend(accept_buttons.iter().map(|(b, _)| b.clone()));
        let accept_base = accept_buttons
            .iter()
            .find(|(_, c)| *c == Choice::Base)
            .map(|(b, _)| b.clone());

        // 中身。Result の最初の中身は Undo で消せないようにする。
        for (pane, side) in [
            (&current, &sides[0]),
            (&incoming, &sides[1]),
            (&base, &sides[2]),
        ] {
            pane.view
                .buffer()
                .set_text(side.as_ref().map_or("", |s| s.text.as_str()));
        }
        let buffer = result_pane.view.buffer();
        buffer.begin_irreversible_action();
        buffer.set_text(&result);
        buffer.end_irreversible_action();
        buffer.place_cursor(&buffer.start_iter());
        let originals = regions(&result)
            .into_iter()
            .map(|r| (r.current, r.incoming))
            .collect();

        let this = Rc::new(Self {
            window,
            actions,
            path: path.to_owned(),
            current,
            result: result_pane,
            incoming,
            base,
            sides,
            originals,
            conflicts: RefCell::default(),
            original_of: RefCell::default(),
            selected: Cell::new(0),
            saved: RefCell::new(result),
            syncing: Cell::new(false),
            reparse: RefCell::default(),
            position,
            note,
            buttons,
            accept_base,
            ai_buttons: vec![ai_resolve.clone(), ai_explain.clone()],
            band,
            proposal: RefCell::default(),
            ai_cancel: RefCell::new(Arc::new(AtomicBool::new(false))),
            on_resolved,
        });
        this.update_conflicts();

        let weak = Rc::downgrade(&this);
        let with = move |f: fn(&Rc<Self>)| {
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    f(&this);
                }
            }
        };
        let f = with(|t| t.step(-1));
        previous.connect_clicked(move |_| f());
        let f = with(|t| t.step(1));
        next.connect_clicked(move |_| f());
        let f = with(Self::magic_merge);
        magic.connect_clicked(move |_| f());
        let f = with(Self::ai_resolve);
        ai_resolve.connect_clicked(move |_| f());
        let f = with(Self::ai_explain);
        ai_explain.connect_clicked(move |_| f());
        let f = with(|t| t.accept_proposal(false));
        this.band.accept.connect_clicked(move |_| f());
        let f = with(|t| t.accept_proposal(true));
        this.band.edit.connect_clicked(move |_| f());
        let f = with(|t| t.reject_proposal());
        this.band.reject.connect_clicked(move |_| f());
        crate::assist::auto_click(&ai_resolve, "resolve");
        crate::assist::auto_click(&ai_explain, "explain-conflict");
        crate::assist::auto_click(&this.band.accept, "accept");
        for (button, choice) in accept_buttons {
            let weak = Rc::downgrade(&this);
            button.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.resolve(this.selected.get(), choice);
                }
            });
        }
        let weak = Rc::downgrade(&this);
        base_toggle.connect_toggled(move |b| {
            if let Some(this) = weak.upgrade() {
                this.base.root.set_visible(b.is_active());
            }
        });
        let f = with(|t| t.save(false));
        save.connect_clicked(move |_| f());
        let f = with(Self::save_and_resolve);
        resolve.connect_clicked(move |_| f());

        // キー: Ctrl+Alt+↑↓ でコンフリクトの間を移り、Ctrl+S で保存、Ctrl+Return で保存して解決済みにする。
        let keys = gtk::ShortcutController::new();
        keys.set_scope(gtk::ShortcutScope::Managed);
        let add = |trigger: &str, f: Box<dyn Fn()>| {
            keys.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(trigger),
                Some(gtk::CallbackAction::new(move |_, _| {
                    f();
                    glib::Propagation::Stop
                })),
            ));
        };
        add("<Control><Alt>Up", Box::new(with(|t| t.step(-1))));
        add("<Control><Alt>Down", Box::new(with(|t| t.step(1))));
        add("<Control>s", Box::new(with(|t| t.save(false))));
        add("<Control>Return", Box::new(with(Self::save_and_resolve)));
        this.window.add_controller(keys);

        // Result を書き換えたら、少し待ってからコンフリクトを読み直す。
        let weak = Rc::downgrade(&this);
        this.result.view.buffer().connect_changed(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if let Some(id) = this.reparse.take() {
                id.remove();
            }
            let weak = Rc::downgrade(&this);
            let id =
                glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                    if let Some(this) = weak.upgrade() {
                        this.reparse.take();
                        this.update_conflicts();
                    }
                });
            this.reparse.replace(Some(id));
        });
        // 欄でクリックした位置のコンフリクトを選ぶ。
        for side in 0..4 {
            let weak = Rc::downgrade(&this);
            this.pane(side)
                .view
                .buffer()
                .connect_cursor_position_notify(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.select_at_cursor(side);
                    }
                });
        }
        // どれかの欄をスクロールしたら、ほかの欄を揃える。
        for side in 0..4 {
            let weak = Rc::downgrade(&this);
            this.pane(side)
                .scrolled
                .vadjustment()
                .connect_value_changed(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.scrolled(side);
                    }
                });
        }
        // 保存していない変更があれば、閉じる前に確かめる。
        let weak = Rc::downgrade(&this);
        this.window.connect_close_request(move |window| {
            let Some(this) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if this.result.text() == *this.saved.borrow() {
                return glib::Propagation::Proceed;
            }
            let window = window.clone();
            glib::spawn_future_local(async move {
                let dialog = gtk::AlertDialog::builder()
                    .message(tr!("Close without saving?"))
                    .detail(tr!("Your changes to Result are lost."))
                    .buttons([tr!("Cancel"), tr!("Close Without Saving")])
                    .cancel_button(0)
                    .default_button(0)
                    .modal(true)
                    .build();
                if dialog.choose_future(Some(&window)).await == Ok(1) {
                    window.destroy();
                }
            });
            glib::Propagation::Stop
        });
        this
    }

    /// 欄の番号: 0 Current、1 Incoming、2 Base、3 Result。
    fn pane(&self, side: usize) -> &Pane {
        match side {
            0 => &self.current,
            1 => &self.incoming,
            2 => &self.base,
            _ => &self.result,
        }
    }

    /// 開いたとき: 間違えようのないコンフリクトを解き（設定で切れる）、最初のコンフリクトを見せる。
    fn opened(this: &Rc<Self>) {
        if crate::settings::bool(crate::settings::AUTO_MAGIC_MERGE) {
            let solved = this.apply_magic(MagicRules::Safe);
            if solved > 0 {
                this.note.set_text(&tr!(
                    "Resolved {count} when opened (Ctrl+Z to undo).",
                    count = solved
                ));
            }
        }
        // 大きさが決まってから見せる。
        let weak = Rc::downgrade(this);
        glib::idle_add_local_once(move || {
            if let Some(this) = weak.upgrade() {
                this.reveal();
            }
        });
    }

    /// Result を読み直して、残っているコンフリクトと左右の欄との対応を決め、強調し直す。
    fn update_conflicts(&self) {
        let found = regions(&self.result.text());
        // 両側の中身が同じ最初のコンフリクトに対応させる（手で直したものは対応がなくなる）。
        let mut used = vec![false; self.originals.len()];
        let original_of = found
            .iter()
            .map(|c| {
                let j = (0..self.originals.len()).find(|&j| {
                    !used[j] && self.originals[j] == (c.current.clone(), c.incoming.clone())
                });
                if let Some(j) = j {
                    used[j] = true;
                }
                j
            })
            .collect();
        let count = found.len();
        self.conflicts.replace(found);
        self.original_of.replace(original_of);
        self.selected
            .set(self.selected.get().min(count.saturating_sub(1)));
        self.refresh();
    }

    fn refresh(&self) {
        let conflicts = self.conflicts.borrow();
        let original_of = self.original_of.borrow();
        let selected = self.selected.get();
        self.position.set_text(&if conflicts.is_empty() {
            tr!("No conflicts")
        } else {
            tr!(
                "Conflict {index} of {count}",
                index = selected + 1,
                count = conflicts.len()
            )
        });
        for b in &self.buttons {
            b.set_sensitive(!conflicts.is_empty());
        }
        for b in &self.ai_buttons {
            b.set_sensitive(!conflicts.is_empty() && crate::assist::is_ready());
        }
        // 共通の祖先は、対応のわかるコンフリクトでだけ選べる。
        if let Some(b) = &self.accept_base {
            b.set_sensitive(!conflicts.is_empty() && original_of[selected].is_some());
        }
        // 解決済み（Result に残っていない）のコンフリクトは、左右の欄で薄くする。
        let chosen = original_of.get(selected).copied().flatten();
        for (side, pane) in [&self.current, &self.incoming, &self.base]
            .into_iter()
            .enumerate()
        {
            pane.clear();
            let Some(s) = &self.sides[side] else { continue };
            for (j, &r) in s.regions.iter().enumerate() {
                let live = original_of.contains(&Some(j));
                pane.tag(
                    if chosen == Some(j) {
                        "selected"
                    } else {
                        "conflict"
                    },
                    r,
                );
                if !live {
                    pane.tag("resolved", r);
                }
            }
        }
        self.result.clear();
        for (i, c) in conflicts.iter().enumerate() {
            self.result.tag(
                if i == selected {
                    "selected"
                } else {
                    "conflict"
                },
                c.lines,
            );
        }
    }

    /// Result と欄 `side` の、対応する行の区切り（Result の行, 欄の行）。
    fn anchors(&self, side: usize) -> (Vec<usize>, Vec<usize>) {
        let (mut r, mut s) = (vec![0], vec![0]);
        let Some(regions) = self
            .sides
            .get(side)
            .and_then(|s| s.as_ref())
            .map(|s| &s.regions)
        else {
            return (r, s);
        };
        let conflicts = self.conflicts.borrow();
        for (i, c) in conflicts.iter().enumerate() {
            let Some(j) = self.original_of.borrow()[i] else {
                continue;
            };
            let Some(&(s0, s1)) = regions.get(j) else {
                continue;
            };
            // 前の区切りより後ろのものだけ（順が入れ替わった対応は使わない）。
            if c.lines.0 < *r.last().unwrap() || s0 < *s.last().unwrap() {
                continue;
            }
            r.extend([c.lines.0, c.lines.1]);
            s.extend([s0, s1]);
        }
        (r, s)
    }

    fn scrolled(&self, source: usize) {
        if self.syncing.replace(true) {
            return;
        }
        let top = self.pane(source).top_line();
        // 一度 Result の行に直してから、ほかの欄の行にする。
        let line = if source == 3 {
            top
        } else {
            let (r, s) = self.anchors(source);
            map(top, &s, &r)
        };
        for side in 0..3 {
            if side != source {
                let (r, s) = self.anchors(side);
                self.pane(side).scroll_to_line(map(line, &r, &s));
            }
        }
        if source != 3 {
            self.result.scroll_to_line(line);
        }
        self.syncing.set(false);
    }

    /// 選んだコンフリクトが Result の上から 3 分の 1 あたりに来るようにスクロールする（ほかの欄は揃う）。
    fn reveal(&self) {
        let Some(c) = self.conflicts.borrow().get(self.selected.get()).cloned() else {
            return;
        };
        let iter = self.result.line_iter(c.lines.0);
        let (top, _) = self.result.view.line_yrange(&iter);
        let adj = self.result.scrolled.vadjustment();
        let y = top as f64 - adj.page_size() / 3.0;
        adj.set_value(y.clamp(0.0, (adj.upper() - adj.page_size()).max(0.0)));
        self.scrolled(3);
    }

    fn step(&self, by: isize) {
        let count = self.conflicts.borrow().len() as isize;
        if count == 0 {
            return;
        }
        self.selected
            .set((self.selected.get() as isize + by).rem_euclid(count) as usize);
        self.refresh();
        self.reveal();
    }

    fn select_at_cursor(&self, side: usize) {
        if self.syncing.get() {
            return;
        }
        let line = self.pane(side).cursor_line();
        let index = if side == 3 {
            self.conflicts
                .borrow()
                .iter()
                .position(|c| c.lines.0 <= line && line < c.lines.1)
        } else {
            self.sides[side].as_ref().and_then(|s| {
                let j = s
                    .regions
                    .iter()
                    .position(|&(a, b)| a <= line && line < b.max(a + 1))?;
                self.original_of.borrow().iter().position(|&o| o == Some(j))
            })
        };
        if let Some(index) = index
            && index != self.selected.get()
        {
            self.selected.set(index);
            self.refresh();
        }
    }

    /// Result の変わった所だけを置き換える（Undo の 1 歩になる）。
    fn replace(&self, new: &str) {
        let buffer = self.result.view.buffer();
        let old = self.result.text();
        let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
        let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let suffix = a[prefix..]
            .iter()
            .rev()
            .zip(b[prefix..].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        let replacement: String = b[prefix..b.len() - suffix].iter().collect();
        buffer.begin_user_action();
        let mut start = buffer.iter_at_offset(prefix as i32);
        let mut end = buffer.iter_at_offset((a.len() - suffix) as i32);
        buffer.delete(&mut start, &mut end);
        buffer.insert(&mut start, &replacement);
        buffer.end_user_action();
    }

    /// `index` 番目のコンフリクトを `choice` で解決する。
    fn resolve(&self, index: usize, choice: Choice) {
        if index >= self.conflicts.borrow().len() {
            return;
        }
        let text = self.result.text();
        let Some(resolved) = conflict::resolve(text.as_bytes(), index, choice) else {
            return;
        };
        self.replace(&String::from_utf8_lossy(&resolved));
        self.selected.set(index);
        self.update_conflicts();
        self.reveal();
    }

    /// ✨ Magic Merge: 決まった規則で解けるコンフリクトをまとめて解く。
    fn magic_merge(this: &Rc<Self>) {
        let solved = this.apply_magic(MagicRules::All);
        this.note.set_text(&if solved == 0 {
            tr!("Nothing to resolve")
        } else {
            tr!(
                "Resolved {count} (Ctrl+Z undoes them one at a time).",
                count = solved
            )
        });
        if solved > 0 {
            this.reveal();
        }
    }

    /// Magic Merge をコンフリクト 1 つずつかける（それぞれ別の Undo にする）。後ろから解くので、前の番号は
    /// 変わらない。解いた数を返す。
    fn apply_magic(&self, rules: MagicRules) -> usize {
        let count = self.conflicts.borrow().len();
        let mut solved = 0;
        for index in (0..count).rev() {
            let text = self.result.text();
            let (merged, resolved) = conflict::magic(text.as_bytes(), rules, Some(index));
            if resolved > 0 {
                self.replace(&String::from_utf8_lossy(&merged));
                solved += 1;
            }
        }
        if solved > 0 {
            self.update_conflicts();
        }
        solved
    }

    fn save_and_resolve(this: &Rc<Self>) {
        let remaining = this.conflicts.borrow().len();
        if remaining == 0 {
            this.save(true);
            return;
        }
        let this = this.clone();
        glib::spawn_future_local(async move {
            let dialog = gtk::AlertDialog::builder()
                .message(tr!(
                    "Mark as resolved with {count} conflicts left?",
                    count = remaining
                ))
                .detail(tr!("The conflict markers stay in the file."))
                .buttons([tr!("Cancel"), tr!("Mark Resolved")])
                .cancel_button(0)
                .default_button(1)
                .modal(true)
                .build();
            if dialog.choose_future(Some(&this.window)).await == Ok(1) {
                this.save(true);
            }
        });
    }

    /// Result を書く（Undo できる）。`stage` なら解決済みにして閉じる。失敗は `Actions` が知らせる。
    fn save(self: &Rc<Self>, stage: bool) {
        let text = self.result.text();
        let command = json!({ "op": "saveMerge", "path": self.path, "text": text, "stage": stage });
        let weak = Rc::downgrade(self);
        self.actions.perform_with(vec![command], move |reply| {
            let Some(this) = weak.upgrade() else { return };
            if reply["ok"].as_bool() != Some(true) {
                return;
            }
            this.saved.replace(text);
            this.note.set_text(&tr!("Saved"));
            if stage {
                (this.on_resolved)();
                this.window.destroy();
            }
        });
    }
}

/// AI の解消の提案を見せる帯（紫の帯と理由。採用するまで Result は変わらない）。
struct Band {
    root: gtk::Box,
    spinner: gtk::Spinner,
    status: gtk::Label,
    reason: gtk::Label,
    text: gtk::Label,
    accept: gtk::Button,
    edit: gtk::Button,
    reject: gtk::Button,
}

impl Band {
    fn new() -> Self {
        crate::assist::install_css();
        let spinner = gtk::Spinner::new();
        let status = gtk::Label::new(None);
        status.add_css_class("heading");
        status.set_xalign(0.0);
        status.set_hexpand(true);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let accept = gtk::Button::with_label(&tr!("Accept"));
        accept.add_css_class("suggested-action");
        let edit = gtk::Button::with_label(&tr!("Edit and Accept"));
        let reject = gtk::Button::with_label(&tr!("Reject"));
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for w in [
            spinner.upcast_ref::<gtk::Widget>(),
            status.upcast_ref(),
            reject.upcast_ref(),
            edit.upcast_ref(),
            accept.upcast_ref(),
        ] {
            head.append(w);
        }
        let reason = gtk::Label::new(None);
        reason.set_xalign(0.0);
        reason.set_wrap(true);
        reason.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        reason.set_selectable(true);
        let text = gtk::Label::new(None);
        text.set_xalign(0.0);
        text.set_yalign(0.0);
        text.set_selectable(true);
        text.add_css_class("monospace");
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&text)
            .propagate_natural_height(true)
            .max_content_height(180)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("ai-suggestion");
        root.set_margin_start(6);
        root.set_margin_end(6);
        root.set_margin_bottom(6);
        root.append(&head);
        root.append(&reason);
        root.append(&scrolled);
        root.set_visible(false);
        Self {
            root,
            spinner,
            status,
            reason,
            text,
            accept,
            edit,
            reject,
        }
    }

    /// 問い合わせ中（`status`）か、提案（理由と中身）を見せる。
    fn show(&self, status: &str, proposal: Option<&amonhen_assist::Proposal>, error: Option<&str>) {
        self.root.set_visible(true);
        self.status.set_text(status);
        let waiting = proposal.is_none() && error.is_none();
        self.spinner.set_visible(waiting);
        if waiting {
            self.spinner.start();
        } else {
            self.spinner.stop();
        }
        for b in [&self.accept, &self.edit] {
            b.set_visible(proposal.is_some());
        }
        self.reject.set_label(&if proposal.is_some() {
            tr!("Reject")
        } else {
            tr!("Close")
        });
        let reason = match (proposal, error) {
            (Some(p), _) => tr!(
                "Reason: {reason}",
                reason = p.reason.clone().unwrap_or_else(|| tr!("(none)"))
            ),
            (None, Some(e)) => e.to_owned(),
            (None, None) => String::new(),
        };
        self.reason.set_text(&reason);
        self.reason.set_visible(!reason.is_empty());
        let text = proposal.map(|p| p.text.as_str()).unwrap_or_default();
        self.text.set_text(text);
        self.text.set_visible(!text.is_empty());
    }
}

// AI: 選んだコンフリクトの解消の提案（帯で見せ、採用したら Result に入れる）と、両側の意図の解説（窓で）。
impl Editor {
    /// 選んだコンフリクトの両側と、共通の祖先の中身（と、その時の Result のコンフリクト）。
    fn selected_texts(&self) -> Option<(Region, String)> {
        let conflicts = self.conflicts.borrow();
        let index = self.selected.get();
        let c = conflicts.get(index)?.clone();
        let mut base = String::new();
        if let (Some(Some(j)), Some(side)) = (self.original_of.borrow().get(index), &self.sides[2])
            && let Some(&(start, end)) = side.regions.get(*j)
        {
            let lines: Vec<&str> = side.text.split('\n').collect();
            let (start, end) = (start.min(lines.len()), end.min(lines.len()));
            base = lines[start..end].join("\n");
        }
        Some((c, base))
    }

    fn workdir(&self) -> Option<std::path::PathBuf> {
        self.actions.repo().and_then(|r| r.paths().workdir)
    }

    fn ai_variables(&self, c: &Region, base: String) -> amonhen_assist::Variables {
        crate::assist::variables([
            ("path", self.path.clone()),
            ("ours", c.current.clone()),
            ("theirs", c.incoming.clone()),
            ("base", base),
        ])
    }

    /// 両側が何をしようとしているかを解説する（窓で）。
    fn ai_explain(this: &Rc<Self>) {
        let Some((c, base)) = this.selected_texts() else {
            this.window.error_bell();
            return;
        };
        let vars = this.ai_variables(&c, base);
        let (workdir, path) = (this.workdir(), this.path.clone());
        let parent: gtk::Window = this.window.clone();
        glib::spawn_future_local(async move {
            let key = crate::assist::repository_key(workdir.as_deref());
            let summary = tr!("One conflict in {path}", path = path);
            if !crate::assist::confirm(Some(&parent), &key, &summary).await {
                return;
            }
            let p = Some(parent.clone());
            crate::assist::panel(
                Some(&parent),
                &tr!("Conflict Explanation"),
                None,
                None,
                crate::assist::work(move |run: crate::assist::Run| {
                    crate::assist::run_with(
                        p.clone(),
                        amonhen_assist::Purpose::ExplainConflict,
                        vars.clone(),
                        workdir.clone(),
                        run,
                    )
                }),
            );
        });
    }

    /// 解消の提案を頼み、帯で見せる。形の違う返事・マーカーの残る提案は失敗として見せる。
    fn ai_resolve(this: &Rc<Self>) {
        let Some((c, base)) = this.selected_texts() else {
            this.window.error_bell();
            return;
        };
        let vars = this.ai_variables(&c, base);
        let workdir = this.workdir();
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let Some(this) = weak.upgrade() else { return };
            let key = crate::assist::repository_key(workdir.as_deref());
            let summary = tr!("One conflict in {path}", path = this.path);
            if !crate::assist::confirm(Some(&this.window), &key, &summary).await {
                return;
            }
            this.ai_cancel.borrow().store(true, Ordering::Relaxed);
            let cancel = Arc::new(AtomicBool::new(false));
            this.ai_cancel.replace(cancel.clone());
            this.proposal.replace(None);
            this.band.show(&tr!("Waiting…"), None, None);
            let w = Rc::downgrade(&this);
            let on_text: crate::assist::OnText = Rc::new(move |_| {
                if let Some(this) = w.upgrade() {
                    this.band.status.set_text(&tr!("Receiving…"));
                }
            });
            let parent = Some(this.window.clone());
            drop(this);
            let result = crate::assist::run(
                parent,
                amonhen_assist::Purpose::ResolveConflict,
                vars,
                workdir,
                true,
                cancel.clone(),
                on_text,
            )
            .await;
            let Some(this) = weak.upgrade() else { return };
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let failed = tr!("Failed");
            match result.and_then(|raw| {
                let proposal = amonhen_assist::Proposal::parse(&raw).ok_or_else(|| {
                    format!(
                        "{}\n\n{raw}",
                        tr!("Couldn’t read the reply (not JSON in the expected shape):")
                    )
                })?;
                if proposal.has_markers() {
                    return Err(format!(
                        "{}\n\n{}",
                        tr!("The suggestion still has conflict markers. Try again."),
                        proposal.text
                    ));
                }
                Ok(proposal)
            }) {
                Ok(proposal) => {
                    this.band.show(&tr!("AI Suggestion"), Some(&proposal), None);
                    this.proposal.replace(Some((c, proposal)));
                }
                Err(e) => this.band.show(&failed, None, Some(&e)),
            }
        });
    }

    /// 提案で、そのコンフリクト（マーカーを含む行）を置き換える（Undo で戻せる）。コンフリクトが動いていれば
    /// 中身で探し直す。`edit` なら、入れた所を選んで書き換えられるようにする。
    fn accept_proposal(&self, edit: bool) {
        let Some((original, proposal)) = self.proposal.take() else {
            return;
        };
        self.band.root.set_visible(false);
        let found = {
            let conflicts = self.conflicts.borrow();
            conflicts
                .iter()
                .find(|c| c.current == original.current && c.incoming == original.incoming)
                .or_else(|| conflicts.iter().find(|c| c.lines == original.lines))
                .map(|c| c.lines)
        };
        let Some((start, end)) = found else {
            self.window.error_bell();
            return;
        };
        let old = self.result.text();
        let mut lines: Vec<&str> = old.split('\n').collect();
        let (start, end) = (start.min(lines.len()), end.min(lines.len()));
        let mut replacement: Vec<&str> = proposal.text.split('\n').collect();
        if replacement.last() == Some(&"") {
            replacement.pop();
        }
        let count = replacement.len();
        lines.splice(start..end, replacement);
        self.replace(&lines.join("\n"));
        self.update_conflicts();
        self.note
            .set_text(&tr!("Put in the AI’s suggestion (Ctrl+Z to undo)."));
        if edit {
            let buffer = self.result.view.buffer();
            let from = self.result.line_iter(start);
            let mut to = self.result.line_iter(start + count);
            if count > 0 && start + count < lines.len() {
                to.backward_char();
            }
            buffer.select_range(&from, &to);
            self.result.view.grab_focus();
            self.result
                .view
                .scroll_to_iter(&mut from.clone(), 0.1, false, 0.0, 0.0);
        }
    }

    fn reject_proposal(&self) {
        self.ai_cancel.borrow().store(true, Ordering::Relaxed);
        self.proposal.replace(None);
        self.band.root.set_visible(false);
    }
}
