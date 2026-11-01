//! マージエディタ（macOS 版の `MergeEditor.swift`）: コンフリクトのあるファイル 1 つを解決する。左に Current、
//! 真ん中に編集できる Result（今の作業ツリーの内容から始める）、右に Incoming（左右は、すべてのコンフリクトを
//! その側にした内容）。共通の祖先（diff3）があれば、Base を左端に出せる。Working Copy の右に埋め込むほか、
//! 別の窓（[`open`]・埋め込んだものを移す [`show_in_window`]）でも開ける。
//!
//! 列の間の帯は、左右の欄のコンフリクトと Result のコンフリクトをつなぎ、≫・≪ を押すとその側で解決する。
//!
//! 選んだコンフリクトを Accept Current・Incoming・Both・Base で解き、✨ Magic Merge は決まった規則で解ける
//! コンフリクトをまとめて解く（どれも Result の変わった所だけを置き換えるので、Ctrl+Z で 1 つずつ戻せる）。
//! 4 つの欄のスクロールは、コンフリクトの位置で揃える。解決の計算はコアの `amonhen_core::conflict`（文字だけを
//! 扱うので、daemon 越しのリポジトリでも手元で計算する）、読むのは `mergeSides`、書くのは `saveMerge`。

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
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

/// 帯の矢印の押せる範囲（x, y, 幅, 高さ）と、そのコンフリクト。
type Arrow = (f64, f64, f64, f64, usize);

pub struct Editor {
    /// エディタの中身（Working Copy に埋め込むか、別の窓に入れる）。
    root: gtk::Box,
    /// 別の窓に入れたとき、その窓。
    window: RefCell<Option<glib::WeakRef<gtk::Window>>>,
    /// 別の窓で開くボタン（埋め込んだときだけ出す）。
    popout: gtk::Button,
    /// Current と Result、Result と Incoming の間の帯と、帯の矢印の押せる範囲（x, y, 幅, 高さ, コンフリクト）。
    bands: [gtk::DrawingArea; 2],
    arrows: [RefCell<Vec<Arrow>>; 2],
    actions: Rc<Actions>,
    /// コンフリクトのある worktree の作業ツリー（同じパスでも、worktree が違えば別のエディタ）。
    workdir: Option<PathBuf>,
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
    on_resolved: RefCell<Rc<dyn Fn()>>,
    /// 別の窓で開く（埋め込んだ側が決める）。
    on_pop_out: RefCell<Option<Rc<dyn Fn()>>>,
}

thread_local! {
    /// 開いているマージエディタ（パスごとに 1 つ）。
    static OPEN: RefCell<Vec<(WindowKey, glib::WeakRef<gtk::Window>)>> = RefCell::default();
    /// AI の返事が使えなかった（読めない・マーカーが残る）問い合わせ（worktree と中身）。次はキャッシュを使わずに
    /// 聞き直す（同じ返事を出し続けないように。macOS 版の Regenerate にあたる）。エディタを作り直しても覚えておく。
    static AI_FAILED: RefCell<std::collections::HashSet<String>> = RefCell::default();
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

/// `path` のマージエディタを別の窓で開く（開いていれば前に出す）。保存して解決済みにしたら `on_resolved` を呼ぶ。
pub fn open(
    parent: &gtk::Window,
    repo: Arc<Repository>,
    actions: Rc<Actions>,
    path: &str,
    on_resolved: impl Fn() + 'static,
) {
    if let Some(w) = open_window(repo.paths().workdir.as_deref(), path) {
        w.present();
        return;
    }
    let (parent, path) = (parent.clone(), path.to_owned());
    glib::spawn_future_local(async move {
        match load(repo, actions, &path).await {
            Ok(editor) => {
                editor.set_on_resolved(on_resolved);
                show_in_window(&editor, &parent);
                Editor::opened(&editor);
            }
            Err(error) => {
                let dialog = gtk::AlertDialog::builder()
                    .message(tr!("Can’t open the merge editor"))
                    .detail(error)
                    .modal(true)
                    .build();
                let _ = dialog.choose_future(Some(&parent)).await;
            }
        }
    });
}

/// 別の窓で開いているマージエディタの鍵（worktree の作業ツリーとパス）。
type WindowKey = (Option<PathBuf>, String);

/// `workdir` の worktree の `path` のマージエディタを開いている別の窓（なければ None）。
pub fn open_window(workdir: Option<&Path>, path: &str) -> Option<gtk::Window> {
    OPEN.with(|o| {
        o.borrow()
            .iter()
            .find(|((w, p), _)| w.as_deref() == workdir && p == path)
            .and_then(|(_, w)| w.upgrade())
    })
}

/// `path` の両側と Result を裏で読んで、エディタを作る（まだどこにも入れない）。失敗はその理由。
pub async fn load(
    repo: Arc<Repository>,
    actions: Rc<Actions>,
    path: &str,
) -> Result<Rc<Editor>, String> {
    let p = path.to_owned();
    let workdir = repo.paths().workdir;
    let reply: Value = gio::spawn_blocking(move || {
        let command = json!({ "op": "mergeSides", "path": p });
        serde_json::from_str(&repo.run_json(&command.to_string())).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    if let Some(error) = reply["error"].as_str() {
        return Err(error.to_owned());
    }
    let info = &reply["info"];
    let sides = [
        Side::from(&info["current"]),
        Side::from(&info["incoming"]),
        Side::from(&info["base"]),
    ];
    let result = info["result"].as_str().unwrap_or_default().to_owned();
    // マーカーが残っていなければ（rerere が解決を当てた、ほかのエディタで直した）、その案内。
    let without_markers = (info["markers"].as_u64() == Some(0)).then(|| {
        if info["rerere"].as_bool() == Some(true) {
            tr!("git rerere applied the earlier resolution. Check the result, then Mark Resolved.")
        } else {
            tr!("No conflict markers are left. Check the result, then Mark Resolved.")
        }
    });
    Ok(Editor::new(
        actions,
        workdir,
        path,
        sides,
        result,
        without_markers,
    ))
}

/// エディタを（編集中の内容のまま）別の窓に入れて出す。埋め込んでいた側は、先に外しておく。
pub fn show_in_window(editor: &Rc<Editor>, parent: &gtk::Window) {
    editor.set_on_pop_out(None);
    let window = gtk::Window::builder()
        .title(format!("Merge — {}", editor.path))
        .default_width(1400)
        .default_height(800)
        .child(&editor.root)
        .transient_for(parent)
        .destroy_with_parent(true)
        .build();
    editor.window.replace(Some(window.downgrade()));
    OPEN.with(|o| {
        let mut o = o.borrow_mut();
        o.retain(|(_, w)| w.upgrade().is_some());
        o.push((
            (editor.workdir.clone(), editor.path.clone()),
            window.downgrade(),
        ));
    });
    // 窓がある間は持っておく（ボタンなどは弱い参照で呼ぶので、持たなければ開いた直後に消える）。
    KEEP.with(|k| k.borrow_mut().push(editor.clone()));
    let weak = Rc::downgrade(editor);
    window.connect_destroy(move |_| {
        KEEP.with(|k| {
            k.borrow_mut()
                .retain(|e| !std::ptr::eq(Rc::as_ptr(e), weak.as_ptr()))
        });
    });
    // 解決済みにしたら閉じる（その前に、渡されていた知らせを呼ぶ）。
    let previous = editor.on_resolved.borrow().clone();
    let w = window.downgrade();
    editor.set_on_resolved(move || {
        previous();
        if let Some(w) = w.upgrade() {
            w.destroy();
        }
    });
    // 保存していない変更があれば、閉じる前に確かめる。
    let weak = Rc::downgrade(editor);
    window.connect_close_request(move |window| {
        let Some(this) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if !this.has_unsaved_changes() {
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
    window.present();
}

impl Editor {
    /// エディタの中身（埋め込む側が入れる）。
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Result を保存していない変更があるか。
    pub fn has_unsaved_changes(&self) -> bool {
        self.result.text() != *self.saved.borrow()
    }

    /// 保存して解決済みにしたときの知らせ。
    pub fn set_on_resolved(&self, f: impl Fn() + 'static) {
        self.on_resolved.replace(Rc::new(f));
    }

    /// 別の窓で開くボタンを押したとき（None ならボタンを出さない）。
    pub fn set_on_pop_out(&self, f: Option<Rc<dyn Fn()>>) {
        self.popout.set_visible(f.is_some());
        self.on_pop_out.replace(f);
    }

    /// ダイアログを出す窓（埋め込んだ窓か、別の窓）。
    fn parent_window(&self) -> Option<gtk::Window> {
        self.root.root().and_downcast()
    }
}

impl Editor {
    fn new(
        actions: Rc<Actions>,
        workdir: Option<PathBuf>,
        path: &str,
        sides: [Option<Side>; 3],
        result: String,
        without_markers: Option<String>,
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

        // テーマにアイコンがなければ（macOS の GTK など）、文字にする。
        let has_icon = gdk::Display::default()
            .is_some_and(|d| gtk::IconTheme::for_display(&d).has_icon("window-new-symbolic"));
        let popout = if has_icon {
            gtk::Button::from_icon_name("window-new-symbolic")
        } else {
            gtk::Button::with_label("⧉")
        };
        popout.set_tooltip_text(Some(&tr!("Open in a Separate Window")));
        popout.set_visible(false);

        // 操作の帯は 2 段（Working Copy の右に埋め込んでも収まるように）: 上にコンフリクトの選択と解決、下に
        // Magic Merge・AI・知らせと保存。
        let row = || {
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            b.set_margin_start(10);
            b.set_margin_end(10);
            b
        };
        let top = row();
        top.set_margin_top(6);
        top.append(&position);
        top.append(&previous);
        top.append(&next);
        for (b, _) in &accept_buttons {
            top.append(b);
        }
        top.append(&base_toggle);
        let bottom = row();
        bottom.set_margin_top(6);
        bottom.set_margin_bottom(6);
        bottom.append(&magic);
        bottom.append(&ai_resolve);
        bottom.append(&ai_explain);
        bottom.append(&note);
        bottom.append(&popout);
        bottom.append(&save);
        bottom.append(&resolve);

        // 列: Base（出すとき）・Current と帯・Result・帯と Incoming（同じ幅に分ける）。
        let columns = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        columns.set_homogeneous(true);
        columns.set_margin_start(6);
        columns.set_margin_end(6);
        columns.set_margin_bottom(6);
        base.root.set_visible(false);
        base.root.set_margin_end(6);
        let bands = [Self::band_area(), Self::band_area()];
        let pair = |a: &gtk::Widget, b: &gtk::Widget| {
            let p = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            p.append(a);
            p.append(b);
            p
        };
        columns.append(&base.root);
        columns.append(&pair(
            current.root.upcast_ref(),
            Self::band_column(&bands[0]).upcast_ref(),
        ));
        columns.append(&result_pane.root);
        columns.append(&pair(
            Self::band_column(&bands[1]).upcast_ref(),
            incoming.root.upcast_ref(),
        ));
        let band = Band::new();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&top);
        root.append(&bottom);
        // マーカーの残っていないファイル（rerere が解決を当てた、ほかのエディタで直した）の案内。
        let banner_resolve = without_markers.as_ref().map(|text| {
            let (banner, button) = Self::without_markers_banner(text);
            root.append(&banner);
            button
        });
        root.append(&band.root);
        root.append(&columns);

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
            root,
            window: RefCell::default(),
            popout: popout.clone(),
            bands,
            arrows: Default::default(),
            actions,
            workdir,
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
            on_resolved: RefCell::new(Rc::new(|| {})),
            on_pop_out: RefCell::default(),
        });
        this.update_conflicts();
        // AI の設定が変わると `assist::track` がボタンを押せるようにするので、その後でコンフリクトの有無に
        // 合わせ直す（残っていなければ押せない）。エディタがなくなったら外す。
        let weak = Rc::downgrade(&this);
        crate::settings::connect_changed_while(&this.root, move |key| {
            if (key == crate::assist::ASSISTANT || key.starts_with("assistantPath."))
                && let Some(this) = weak.upgrade()
            {
                this.refresh();
            }
        });

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
        if let Some(button) = banner_resolve {
            let f = with(Self::save_and_resolve);
            button.connect_clicked(move |_| f());
        }

        let weak = Rc::downgrade(&this);
        popout.connect_clicked(move |_| {
            let f = weak.upgrade().and_then(|t| t.on_pop_out.borrow().clone());
            if let Some(f) = f {
                f();
            }
        });

        // キー: Ctrl+Alt+↑↓ でコンフリクトの間を移り、Ctrl+S で保存、Ctrl+Return で保存して解決済みにする。
        // エディタの中にフォーカスがあるときだけ（埋め込んだ Working Copy では、外の Ctrl+Return はコミット）。
        // Result の欄より先に受ける。
        let keys = gtk::ShortcutController::new();
        keys.set_scope(gtk::ShortcutScope::Local);
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
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
        this.root.add_controller(keys);

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
        // どれかの欄をスクロールしたら、ほかの欄を揃え、帯を描き直す。
        for side in 0..4 {
            let weak = Rc::downgrade(&this);
            this.pane(side)
                .scrolled
                .vadjustment()
                .connect_value_changed(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.scrolled(side);
                        this.redraw_bands();
                    }
                });
        }
        Self::connect_bands(&this);
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

    /// 開いたとき（作ったあと一度だけ）: 間違えようのないコンフリクトを解き（設定で切れる）、最初のコンフリクトを
    /// 見せる。
    pub fn opened(this: &Rc<Self>) {
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
        if let Some(choice) = ACCEPT_ONCE.with(|a| a.take()) {
            this.resolve(this.selected.get(), choice);
        }
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
        drop((conflicts, original_of));
        self.redraw_bands();
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
            if dialog.choose_future(this.parent_window().as_ref()).await == Ok(1) {
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
                let f = this.on_resolved.borrow().clone();
                f();
            }
        });
    }
}

// 列の間の帯（macOS 版の `ConflictBand`）: コンフリクトごとに、左右の欄の行範囲と Result の行範囲を曲線で
// つなぎ、欄の側にその側で解決する矢印（≫・≪）を出す。
impl Editor {
    fn band_area() -> gtk::DrawingArea {
        let area = gtk::DrawingArea::new();
        area.set_content_width(40);
        area.set_vexpand(true);
        area
    }

    /// 帯と、欄の見出しと同じ高さの空き（帯の上端を文字の欄の上端に揃える）。
    fn band_column(area: &gtk::DrawingArea) -> gtk::Box {
        let spacer = gtk::Label::new(Some(" "));
        spacer.add_css_class("heading");
        spacer.add_css_class("caption");
        let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
        b.append(&spacer);
        b.append(area);
        b
    }

    /// マーカーの残っていないファイルの案内の帯と、その Mark Resolved のボタン（macOS 版と同じ）。
    fn without_markers_banner(text: &str) -> (gtk::Box, gtk::Button) {
        let label = gtk::Label::new(Some(&format!("✓ {text}")));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_tooltip_text(Some(text));
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        b.add_css_class("merge-without-markers");
        b.set_margin_start(6);
        b.set_margin_end(6);
        b.set_margin_bottom(6);
        b.append(&label);
        let resolve = gtk::Button::with_label(&tr!("Mark Resolved"));
        resolve.set_valign(gtk::Align::Center);
        b.append(&resolve);
        let css = gtk::CssProvider::new();
        css.load_from_string(
            ".merge-without-markers { background: alpha(#e66100, 0.14); border-radius: 6px; padding: 6px 10px; }",
        );
        gtk::style_context_add_provider_for_display(
            &b.display(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        (b, resolve)
    }

    fn redraw_bands(&self) {
        for b in &self.bands {
            b.queue_draw();
        }
    }

    /// 欄 `pane` の行 `line` の上端の、帯 `band` の中の y（見えている範囲の外なら外の値）。
    fn band_y(pane: &Pane, line: usize, band: &gtk::DrawingArea) -> Option<f64> {
        let buffer = pane.view.buffer();
        let lines = buffer.line_count().max(1) as usize;
        let y = if line < lines {
            pane.view.line_yrange(&pane.line_iter(line)).0
        } else {
            let (top, height) = pane.view.line_yrange(&buffer.end_iter());
            top + height
        };
        let (_, wy) = pane
            .view
            .buffer_to_window_coords(gtk::TextWindowType::Widget, 0, y);
        pane.view
            .compute_point(band, &gtk::graphene::Point::new(0.0, wy as f32))
            .map(|p| p.y() as f64)
    }

    /// 帯 `index`（0 左、1 右）を描く。
    fn draw_band(&self, index: usize, cr: &gtk::cairo::Context, width: f64, height: f64) {
        let band = &self.bands[index];
        let ours = index == 0;
        let (pane, side) = if ours {
            (&self.current, &self.sides[0])
        } else {
            (&self.incoming, &self.sides[1])
        };
        let mut arrows = self.arrows[index].borrow_mut();
        arrows.clear();
        let Some(side) = side else { return };
        let color = if ours {
            (0.21, 0.52, 0.89)
        } else {
            (0.18, 0.76, 0.49)
        };
        let conflicts = self.conflicts.borrow();
        let original_of = self.original_of.borrow();
        let layout = pangocairo::functions::create_layout(cr);
        let mut font = gtk::pango::FontDescription::new();
        font.set_weight(gtk::pango::Weight::Bold);
        font.set_absolute_size(13.0 * gtk::pango::SCALE as f64);
        layout.set_font_description(Some(&font));
        layout.set_text(if ours { "≫" } else { "≪" });
        let (aw, ah) = layout.pixel_size();
        let (aw, ah) = (aw as f64, ah as f64);
        for (i, c) in conflicts.iter().enumerate() {
            let Some(Some(j)) = original_of.get(i) else {
                continue;
            };
            let Some(&(s0, s1)) = side.regions.get(*j) else {
                continue;
            };
            let (Some(sy0), Some(sy1), Some(ry0), Some(ry1)) = (
                Self::band_y(pane, s0, band),
                Self::band_y(pane, s1, band),
                Self::band_y(&self.result, c.lines.0, band),
                Self::band_y(&self.result, c.lines.1, band),
            ) else {
                continue;
            };
            let sy1 = sy1.max(sy0 + 2.0);
            // 左の帯は左が欄・右が Result、右の帯はその逆。
            let (l0, l1, r0, r1) = if ours {
                (sy0, sy1, ry0, ry1)
            } else {
                (ry0, ry1, sy0, sy1)
            };
            if l1.max(r1) < 0.0 || l0.min(r0) > height {
                continue;
            }
            let w = width;
            cr.move_to(0.0, l0);
            cr.curve_to(w / 2.0, l0, w / 2.0, r0, w, r0);
            cr.line_to(w, r1);
            cr.curve_to(w / 2.0, r1, w / 2.0, l1, 0.0, l1);
            cr.close_path();
            let selected = i == self.selected.get();
            cr.set_source_rgba(
                color.0,
                color.1,
                color.2,
                if selected { 0.35 } else { 0.16 },
            );
            let _ = cr.fill_preserve();
            cr.set_source_rgba(color.0, color.1, color.2, 0.5);
            cr.set_line_width(1.0);
            let _ = cr.stroke();
            // 欄の側の上端に、その側で解決する矢印。
            let x = if ours { 2.0 } else { w - aw - 2.0 };
            cr.set_source_rgb(color.0, color.1, color.2);
            cr.move_to(x, sy0);
            pangocairo::functions::show_layout(cr, &layout);
            arrows.push((x - 4.0, sy0 - 2.0, aw + 8.0, ah + 4.0, i));
        }
    }

    /// 帯 `index` の (x, y) にある矢印のコンフリクト。
    fn arrow_at(&self, index: usize, x: f64, y: f64) -> Option<usize> {
        self.arrows[index]
            .borrow()
            .iter()
            .find(|(ax, ay, w, h, _)| x >= *ax && x < ax + w && y >= *ay && y < ay + h)
            .map(|a| a.4)
    }

    fn connect_bands(this: &Rc<Self>) {
        for index in 0..2 {
            let band = &this.bands[index];
            let weak = Rc::downgrade(this);
            band.set_draw_func(move |_, cr, w, h| {
                if let Some(this) = weak.upgrade() {
                    this.draw_band(index, cr, w as f64, h as f64);
                }
            });
            // ≫・≪ を押したら、その側で解決する。
            let click = gtk::GestureClick::new();
            let weak = Rc::downgrade(this);
            click.connect_pressed(move |_, _, x, y| {
                let Some(this) = weak.upgrade() else { return };
                if let Some(i) = this.arrow_at(index, x, y) {
                    this.resolve(
                        i,
                        if index == 0 {
                            Choice::Ours
                        } else {
                            Choice::Theirs
                        },
                    );
                }
            });
            band.add_controller(click);
            let motion = gtk::EventControllerMotion::new();
            let weak = Rc::downgrade(this);
            motion.connect_motion(move |m, x, y| {
                let Some(this) = weak.upgrade() else { return };
                let over = this.arrow_at(index, x, y).is_some();
                if let Some(w) = m.widget() {
                    w.set_cursor_from_name(over.then_some("pointer"));
                }
            });
            band.add_controller(motion);
            // 欄の大きさが変わったら（Base を出す・窓の大きさ）描き直す。
            let weak = Rc::downgrade(this);
            band.connect_resize(move |_, _, _| {
                if let Some(this) = weak.upgrade() {
                    this.redraw_bands();
                }
            });
        }
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
            this.root.error_bell();
            return;
        };
        let vars = this.ai_variables(&c, base);
        let (workdir, path) = (this.workdir(), this.path.clone());
        let parent = this.parent_window();
        glib::spawn_future_local(async move {
            let key = crate::assist::repository_key(workdir.as_deref());
            let summary = tr!("One conflict in {path}", path = path);
            if !crate::assist::confirm(parent.as_ref(), &key, &summary).await {
                return;
            }
            let p = parent.clone();
            crate::assist::panel(
                parent.as_ref(),
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
            this.root.error_bell();
            return;
        };
        let vars = this.ai_variables(&c, base);
        let workdir = this.workdir();
        let asked = format!("{workdir:?}\0{vars:?}");
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let Some(this) = weak.upgrade() else { return };
            let key = crate::assist::repository_key(workdir.as_deref());
            let summary = tr!("One conflict in {path}", path = this.path);
            if !crate::assist::confirm(this.parent_window().as_ref(), &key, &summary).await {
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
            let parent = this.parent_window();
            let use_cache = !AI_FAILED.with(|f| f.borrow().contains(&asked));
            drop(this);
            let result = crate::assist::run(
                parent,
                amonhen_assist::Purpose::ResolveConflict,
                vars,
                workdir,
                use_cache,
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
                    AI_FAILED.with(|f| f.borrow_mut().remove(&asked));
                    this.band.show(&tr!("AI Suggestion"), Some(&proposal), None);
                    this.proposal.replace(Some((c, proposal)));
                }
                Err(e) => {
                    AI_FAILED.with(|f| f.borrow_mut().insert(asked));
                    this.band.show(&failed, None, Some(&e));
                }
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
            self.root.error_bell();
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
