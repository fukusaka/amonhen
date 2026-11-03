//! Quick Open（Ctrl+Shift+O。macOS 版の `QuickOpen.swift`）: ブックマーク・最近開いたもの・スキャンで見つけたものを、
//! あいまい検索して開く。Enter で新しいタブに（開いていればそのタブを前に）、Ctrl+Enter で新しい窓に開く。Esc か、
//! ほかの窓に移ると閉じる。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;

use crate::bookmarks;
use crate::repobrowser::{self, Open};

thread_local! {
    static WINDOW: RefCell<glib::WeakRef<gtk::Window>> = RefCell::default();
}

/// `query` の文字を `text` の中に順に（飛び飛びでよい）見つけられれば、点数（続いているほど・先頭や区切りの直後
/// ほど高い）。見つからなければ None。
pub fn score(query: &str, text: &str) -> Option<i64> {
    let t: Vec<char> = text.chars().collect();
    let (mut score, mut ti, mut previous) = (0i64, 0usize, -2i64);
    for c in query.chars() {
        while ti < t.len() && t[ti] != c {
            ti += 1;
        }
        if ti >= t.len() {
            return None;
        }
        if ti as i64 == previous + 1 {
            score += 5;
        }
        if ti == 0 || "/-_. ".contains(t[ti - 1]) {
            score += 8;
        }
        previous = ti as i64;
        ti += 1;
    }
    // 短いものほど少し高く。
    Some(score * 100 - t.len() as i64)
}

/// 候補（場所と、比べるための小文字の名前・場所）。候補を読み直すときに一度だけ作る（打つたび・候補ごとに
/// ブックマークを読むと、たくさんあるときに遅い）。
struct Candidate {
    location: String,
    name: String,
    lower: String,
}

fn read_candidates(data: &bookmarks::Data, locations: Vec<String>) -> Vec<Candidate> {
    locations
        .into_iter()
        .map(|location| Candidate {
            name: bookmarks::display_name_in(data, &location).to_lowercase(),
            lower: location.to_lowercase(),
            location,
        })
        .collect()
}

/// 問い合わせに合うものを、合い方の良い順に並べる（名前で合えば高く、パスでしか合わなければ低く）。
fn matches(candidates: &[Candidate], query: &str) -> Vec<String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return candidates.iter().map(|c| c.location.clone()).collect();
    }
    let mut scored: Vec<(&str, i64)> = candidates
        .iter()
        .filter_map(|c| {
            if let Some(s) = score(&query, &c.name) {
                return Some((c.location.as_str(), s + 1000));
            }
            score(&query, &c.lower).map(|s| (c.location.as_str(), s))
        })
        .collect();
    // 同じ点なら元の並び（お気に入り・ブックマーク・最近・スキャンの順）。
    scored.sort_by_key(|(_, s)| -s);
    scored.into_iter().map(|(p, _)| p.to_owned()).collect()
}

struct Row {
    name: gtk::Label,
    path: gtk::Label,
    branch: gtk::Label,
    state: gtk::Label,
    location: RefCell<String>,
    /// 読んだブックマーク（候補と一緒に読み直す。行ごとに読まない）。
    data: Rc<RefCell<bookmarks::Data>>,
}

impl Row {
    fn render(&self) {
        let location = self.location.borrow().clone();
        self.name
            .set_markup(&repobrowser::name_markup_in(&self.data.borrow(), &location));
        self.path.set_text(&repobrowser::path_text(&location));
        self.branch.set_text(&repobrowser::branch_text(&location));
        repobrowser::show_state(&self.state, &location, false);
    }
}

/// Quick Open を出す（`parent` の上に）。`query` は確認用に、最初から入れておく問い合わせ。
pub fn show(parent: Option<&gtk::Window>, open: Open) -> gtk::Window {
    show_with(parent, open, "")
}

pub fn show_with(parent: Option<&gtk::Window>, open: Open, query: &str) -> gtk::Window {
    if let Some(w) = WINDOW.with(|w| w.borrow().upgrade()) {
        w.close();
    }
    bookmarks::refresh();
    // スキャンの結果がまだなければ探し始める（見つかったら候補に足す）。
    if !bookmarks::has_scanned() && !bookmarks::data().scan_roots.is_empty() {
        bookmarks::rescan();
    }
    let field = gtk::SearchEntry::new();
    field.set_placeholder_text(Some(&tr!(
        "Open a repository (fuzzy search by name or path)"
    )));
    field.set_hexpand(true);
    let model = gtk::StringList::new(&[]);
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    let factory = gtk::SignalListItemFactory::new();
    let rows: Rc<RefCell<Vec<Rc<Row>>>> = Rc::default();
    let data = Rc::new(RefCell::new(bookmarks::data()));
    let (r, d) = (rows.clone(), data.clone());
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let label = |class: Option<&str>, xalign: f32| {
            let l = gtk::Label::new(None);
            l.set_xalign(xalign);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if let Some(c) = class {
                l.add_css_class(c);
            }
            l
        };
        let name = label(None, 0.0);
        let path = label(Some("caption"), 0.0);
        path.add_css_class("dim-label");
        path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let branch = label(Some("caption"), 1.0);
        branch.add_css_class("dim-label");
        let state = label(Some("caption"), 1.0);
        let left = gtk::Box::new(gtk::Orientation::Vertical, 1);
        left.set_hexpand(true);
        left.append(&name);
        left.append(&path);
        let right = gtk::Box::new(gtk::Orientation::Vertical, 1);
        right.append(&branch);
        right.append(&state);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(3);
        line.set_margin_bottom(3);
        line.set_margin_start(4);
        line.set_margin_end(4);
        line.append(&left);
        line.append(&right);
        item.set_child(Some(&line));
        let mut rows = r.borrow_mut();
        rows.retain(|row| row.name.parent().is_some());
        rows.push(Rc::new(Row {
            name,
            path,
            branch,
            state,
            location: RefCell::default(),
            data: d.clone(),
        }));
    });
    let r = rows.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let Some(location) = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .map(|s| s.string().to_string())
        else {
            return;
        };
        let name = item
            .child()
            .and_then(|c| c.first_child())
            .and_then(|c| c.first_child())
            .and_downcast::<gtk::Label>();
        let row = name.and_then(|n| r.borrow().iter().find(|row| row.name == n).cloned());
        if let Some(row) = row {
            *row.location.borrow_mut() = location;
            row.render();
        }
    });
    let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
    list.add_css_class("navigation-sidebar");
    list.set_single_click_activate(false);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let hint = gtk::Label::new(Some(&tr!(
        "Enter Open in Tab   Ctrl+Enter Open in New Window   Esc Close"
    )));
    hint.set_xalign(0.0);
    hint.add_css_class("caption");
    hint.add_css_class("dim-label");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_top(14);
    content.set_margin_bottom(10);
    content.set_margin_start(14);
    content.set_margin_end(14);
    content.append(&field);
    content.append(&scrolled);
    content.append(&hint);
    let window = gtk::Window::builder()
        .title("Quick Open")
        .default_width(560)
        .default_height(380)
        .child(&content)
        .build();
    // 見出しの帯は出さない（macOS 版の浮いた枠の形）。
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    // 親の窓の上に浮かせるが、塞がない（macOS 版の浮いたパネル。開いたまま親の窓も使える）。
    window.set_transient_for(parent);
    window.set_modal(false);
    if let Some(app) = parent.and_then(|p| p.application()) {
        window.set_application(Some(&app));
    }
    WINDOW.with(|w| w.borrow().set(Some(&window)));

    let candidates = Rc::new(RefCell::new(read_candidates(
        &data.borrow(),
        bookmarks::all_locations(),
    )));
    let update = {
        let (model, selection, field, candidates) = (
            model.clone(),
            selection.clone(),
            field.clone(),
            candidates.clone(),
        );
        Rc::new(move || {
            let shown = matches(&candidates.borrow(), &field.text());
            let refs: Vec<&str> = shown.iter().map(String::as_str).collect();
            model.splice(0, model.n_items(), &refs);
            if !shown.is_empty() {
                selection.set_selected(0);
            }
        })
    };
    field.set_text(query);
    update();
    let u = update.clone();
    field.connect_search_changed(move |_| u());
    // ブックマーク・スキャンの結果・行の状態が変わったら、出しているものを描き直す。
    let (w, c, u, r, d) = (
        window.downgrade(),
        candidates.clone(),
        update.clone(),
        rows.clone(),
        data.clone(),
    );
    bookmarks::connect(move |path| {
        if w.upgrade().is_none() {
            return false;
        }
        match path {
            Some(p) => {
                for row in r.borrow().iter() {
                    if *row.location.borrow() == p {
                        row.render();
                    }
                }
            }
            None => {
                *d.borrow_mut() = bookmarks::data();
                *c.borrow_mut() = read_candidates(&d.borrow(), bookmarks::all_locations());
                u();
            }
        }
        true
    });

    let go = {
        let (w, selection, open) = (window.downgrade(), selection.clone(), open.clone());
        Rc::new(move |row: u32, place: crate::tabs::Place| {
            let Some(location) = selection
                .item(row)
                .and_downcast::<gtk::StringObject>()
                .map(|s| s.string().to_string())
            else {
                return;
            };
            if let Some(w) = w.upgrade() {
                w.close();
            }
            open(PathBuf::from(location), place);
        })
    };
    let (g, s) = (go.clone(), selection.clone());
    field.connect_activate(move |_| {
        if s.selected() != gtk::INVALID_LIST_POSITION {
            g(s.selected(), crate::tabs::Place::Tab);
        }
    });
    // ダブルクリックで開く。Ctrl を押しながらなら新しい窓に（macOS 版の ⌘）。開くときの知らせには押していた
    // キーがないので、押したときに覚えておく。
    let ctrl = Rc::new(std::cell::Cell::new(false));
    let click = gtk::GestureClick::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let c = ctrl.clone();
    click.connect_pressed(move |g, _, _, _| {
        c.set(
            g.current_event_state()
                .contains(gtk::gdk::ModifierType::CONTROL_MASK),
        );
    });
    list.add_controller(click);
    let row_keys = gtk::EventControllerKey::new();
    row_keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let c = ctrl.clone();
    row_keys.connect_key_pressed(move |_, _, _, modifiers| {
        c.set(modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK));
        glib::Propagation::Proceed
    });
    list.add_controller(row_keys);
    let g = go.clone();
    list.connect_activate(move |_, row| {
        g(
            row,
            if ctrl.get() {
                crate::tabs::Place::NewWindow
            } else {
                crate::tabs::Place::Tab
            },
        )
    });
    let w = window.downgrade();
    field.connect_stop_search(move |_| {
        if let Some(w) = w.upgrade() {
            w.close();
        }
    });
    // 欄にいたまま ↑↓ で選ぶ。
    // Ctrl+Enter は新しい窓に（欄の Enter より先に受ける）。
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (s, l, g) = (selection.clone(), list.downgrade(), go.clone());
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let step: i64 = match key {
            gtk::gdk::Key::Down => 1,
            gtk::gdk::Key::Up => -1,
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter
                if modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK) =>
            {
                if s.selected() != gtk::INVALID_LIST_POSITION {
                    g(s.selected(), crate::tabs::Place::NewWindow);
                }
                return glib::Propagation::Stop;
            }
            _ => return glib::Propagation::Proceed,
        };
        let n = s.n_items() as i64;
        if n > 0 {
            let current = match s.selected() {
                gtk::INVALID_LIST_POSITION => -1,
                r => r as i64,
            };
            let next = (current + step).clamp(0, n - 1) as u32;
            s.set_selected(next);
            if let Some(l) = l.upgrade() {
                l.scroll_to(next, gtk::ListScrollFlags::NONE, None);
            }
        }
        glib::Propagation::Stop
    });
    field.add_controller(keys);
    // 前の窓でなくなったら閉じる（macOS 版の windowDidResignKey。親やこのアプリのほかの窓を押しても、ほかの
    // アプリに移っても）。窓から開いたときだけ（起動したときに単独で出したものは、閉じると何も残らない）。
    window.connect_is_active_notify(|w| {
        if w.transient_for().is_none() || w.is_active() {
            return;
        }
        let w = w.clone();
        glib::idle_add_local_once(move || {
            if !w.is_active() && gtk::Window::list_toplevels().iter().any(|t| t == &w) {
                w.close();
            }
        });
    });
    window.present();
    field.grab_focus();
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_names_before_paths() {
        let data = bookmarks::Data::default();
        let list = read_candidates(&data, vec!["/src/amonhen-x".into(), "/amonhen/zeta".into()]);
        assert_eq!(matches(&list, "zeta"), ["/amonhen/zeta"]);
        assert_eq!(
            matches(&list, "AMONHEN"),
            ["/src/amonhen-x", "/amonhen/zeta"]
        );
        assert_eq!(matches(&list, " ").len(), 2);
        assert!(matches(&list, "qqq").is_empty());
    }

    #[test]
    fn scores_in_order_and_prefers_word_starts() {
        assert!(score("amh", "amonhen").is_some());
        assert!(score("xyz", "amonhen").is_none());
        // 区切りの直後で続いて合うほど高い。
        assert!(score("hen", "amon-hen").unwrap() > score("hen", "amonxhxexn").unwrap_or(i64::MIN));
    }
}
