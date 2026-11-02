//! ブランチの切り替え（Switch Branch…・見出しのブランチ、Ctrl+Shift+B。macOS 版の `BranchSwitcher.swift`）:
//! ブランチをあいまい検索して切り替える。最近切り替えたものを上に出す。リモートのブランチは、追跡するローカル
//! ブランチを作って切り替える。一致するものがなければ、打った名前で新しいブランチを作れる。Esc か、ほかの窓に
//! 移ると閉じる。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use amonhen_model::{RefKind, Refs, tr};
use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

use crate::actions::Actions;
use crate::icon::{self, Icon};
use crate::quickopen;

/// 最近切り替えたブランチの記録（場所 → ブランチの名前、新しい順）。macOS 版の `recentBranches.<場所>`。
const RECENT: &str = "recentBranches";
/// 覚えておく数。
const KEEP: usize = 10;

thread_local! {
    static WINDOW: RefCell<glib::WeakRef<gtk::Window>> = RefCell::default();
    /// 確認用: 次に出したら、すぐ選んでいる行で切り替える（`--switch-enter`）。
    static ENTER_ONCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 確認用: 次に出したら、すぐ選んでいる行で切り替える。
pub fn enter_once() {
    ENTER_ONCE.with(|e| e.set(true));
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    Local(String),
    /// リモートのブランチ（追跡するローカルブランチを作って切り替える）。
    Remote(String),
    /// 打った名前で新しいブランチを作って切り替える。
    Create(String),
}

impl Choice {
    /// 一覧の行の文字列（`StringList` に入れる形）。
    fn encode(&self) -> String {
        match self {
            Choice::Local(n) => format!("l:{n}"),
            Choice::Remote(n) => format!("r:{n}"),
            Choice::Create(n) => format!("c:{n}"),
        }
    }

    fn decode(text: &str) -> Option<Self> {
        let (kind, name) = text.split_once(':')?;
        let name = name.to_owned();
        Some(match kind {
            "l" => Choice::Local(name),
            "r" => Choice::Remote(name),
            "c" => Choice::Create(name),
            _ => return None,
        })
    }
}

fn all_recent() -> serde_json::Map<String, serde_json::Value> {
    crate::settings::string(RECENT)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// `location` のリポジトリで最近切り替えたブランチ（新しい順）。
pub fn recent(location: &str) -> Vec<String> {
    all_recent()
        .get(location)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 今のブランチを覚える（ref を読むたびに。先頭と同じなら何もしない）。
pub fn remember(location: &str, branch: &str) {
    let mut list = recent(location);
    if list.first().map(String::as_str) == Some(branch) {
        return;
    }
    list.retain(|b| b != branch);
    list.insert(0, branch.to_owned());
    list.truncate(KEEP);
    let mut all = all_recent();
    all.insert(location.to_owned(), json!(list));
    crate::settings::set_string(RECENT, &serde_json::Value::Object(all).to_string());
}

/// 問い合わせ `query` に合う候補を、出す順に。最近切り替えたものを先に、残りは名前の順。問い合わせがあれば
/// 合い方の良い順（ローカルのブランチを少し上に）。ちょうど同じ名前のローカルブランチがなく、ブランチの名前として
/// 使えれば、最後に作る行を足す。
fn candidates(refs: &Refs, recent: &[String], query: &str) -> Vec<Choice> {
    let query = query.trim();
    let lower = query.to_lowercase();
    let head = refs.head_branch.as_deref();
    let locals: Vec<&str> = refs
        .entries
        .iter()
        .filter(|e| e.kind == RefKind::Local && Some(e.name.as_str()) != head)
        .map(|e| e.name.as_str())
        .collect();
    let local_set: HashSet<&str> = locals.iter().copied().chain(head).collect();
    // ローカルにもう同じ名前があるリモートのブランチ（origin/main など）は出さない。
    let remotes: Vec<&str> = refs
        .entries
        .iter()
        .filter(|e| e.kind == RefKind::Remote && !e.name.ends_with("/HEAD"))
        .filter(|e| {
            let short = e.name.split_once('/').map_or(e.name.as_str(), |(_, b)| b);
            !local_set.contains(short)
        })
        .map(|e| e.name.as_str())
        .collect();
    let rank: HashMap<&str, usize> = recent
        .iter()
        .enumerate()
        .rev()
        .map(|(i, b)| (b.as_str(), i))
        .collect();
    let mut ordered = locals.clone();
    ordered.sort_by_key(|n| (rank.get(n).copied().unwrap_or(usize::MAX), *n));
    let mut list: Vec<(Choice, &str, i64)> = ordered
        .iter()
        .enumerate()
        .map(|(i, n)| (Choice::Local((*n).to_owned()), *n, 2000 - i as i64))
        .chain(
            remotes
                .iter()
                .enumerate()
                .map(|(i, n)| (Choice::Remote((*n).to_owned()), *n, 1000 - i as i64)),
        )
        .collect();
    if !lower.is_empty() {
        list = list
            .into_iter()
            .filter_map(|(c, name, base)| {
                let bonus = if base > 1000 { 500 } else { 0 };
                quickopen::score(&lower, &name.to_lowercase()).map(|s| (c, name, s + bonus))
            })
            .collect();
        list.sort_by_key(|(_, _, s)| -s);
    }
    let mut shown: Vec<Choice> = list.into_iter().map(|(c, _, _)| c).collect();
    if !query.is_empty()
        && !local_set.contains(query)
        && !query.contains(' ')
        && !query.starts_with('-')
    {
        shown.push(Choice::Create(query.to_owned()));
    }
    shown
}

/// 行の部品（アイコンと名前）。
struct Row {
    icon: Icon,
    label: gtk::Label,
}

/// ブランチの切り替えを出す（`parent` の上に）。`refs` のブランチから選び、`recent` は最近切り替えたもの
/// （新しい順）。`query` は確認用に、最初から入れておく問い合わせ。
pub fn show(
    parent: &gtk::ApplicationWindow,
    refs: Refs,
    recent: Vec<String>,
    actions: Rc<Actions>,
    query: &str,
) -> gtk::Window {
    if let Some(w) = WINDOW.with(|w| w.borrow().upgrade()) {
        w.close();
    }
    let field = gtk::SearchEntry::new();
    field.set_placeholder_text(Some(&tr!("Find a branch or type a new name")));
    field.set_hexpand(true);
    let model = gtk::StringList::new(&[]);
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let icon = Icon::default();
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(3);
        line.set_margin_bottom(3);
        line.set_margin_start(4);
        line.set_margin_end(4);
        line.append(&icon);
        line.append(&label);
        item.set_child(Some(&line));
    });
    let recent = Rc::new(recent);
    let r = recent.clone();
    let a = actions.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let Some(choice) = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .and_then(|s| Choice::decode(&s.string()))
        else {
            return;
        };
        let Some(line) = item.child() else { return };
        let (Some(icon), Some(label)) = (
            line.first_child().and_downcast::<Icon>(),
            line.last_child().and_downcast::<gtk::Label>(),
        ) else {
            return;
        };
        let row = Row { icon, label };
        render(&row, &choice, &r, &a);
    });
    let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
    list.add_css_class("navigation-sidebar");
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let hint = gtk::Label::new(Some(&tr!(
        "Enter Switch   Esc Close   (a remote branch gets a tracking branch)"
    )));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
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
        .title("Switch Branch")
        .default_width(480)
        .default_height(400)
        .child(&content)
        .transient_for(parent)
        .modal(true)
        .build();
    // 見出しの帯は出さない（macOS 版の浮いた枠の形）。
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    // 元の窓と一緒に閉じる（アプリの窓には数えない）。
    window.set_destroy_with_parent(true);
    WINDOW.with(|w| w.borrow().set(Some(&window)));

    let refs = Rc::new(refs);
    let update = {
        let (model, selection, field, refs, recent) = (
            model.clone(),
            selection.clone(),
            field.clone(),
            refs.clone(),
            recent.clone(),
        );
        Rc::new(move || {
            let shown: Vec<String> = candidates(&refs, &recent, &field.text())
                .iter()
                .map(Choice::encode)
                .collect();
            let strs: Vec<&str> = shown.iter().map(String::as_str).collect();
            model.splice(0, model.n_items(), &strs);
            if !shown.is_empty() {
                selection.set_selected(0);
            }
        })
    };
    field.set_text(query);
    update();
    let u = update.clone();
    field.connect_search_changed(move |_| u());

    let go = {
        let (w, selection) = (window.downgrade(), selection.clone());
        Rc::new(move |row: u32| {
            let Some(choice) = selection
                .item(row)
                .and_downcast::<gtk::StringObject>()
                .and_then(|s| Choice::decode(&s.string()))
            else {
                return;
            };
            if let Some(w) = w.upgrade() {
                w.close();
            }
            choose(&actions, choice);
        })
    };
    let (g, s) = (go.clone(), selection.clone());
    field.connect_activate(move |_| {
        if s.selected() != gtk::INVALID_LIST_POSITION {
            g(s.selected());
        }
    });
    let g = go.clone();
    list.connect_activate(move |_, row| g(row));
    let w = window.downgrade();
    field.connect_stop_search(move |_| {
        if let Some(w) = w.upgrade() {
            w.close();
        }
    });
    // 欄にいたまま ↑↓ で選ぶ。
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (s, l) = (selection.clone(), list.downgrade());
    keys.connect_key_pressed(move |_, key, _, _| {
        let step: i64 = match key {
            gtk::gdk::Key::Down => 1,
            gtk::gdk::Key::Up => -1,
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
    // ほかの窓に移ったら閉じる（macOS 版の hidesOnDeactivate）。撮る確認用の起動では、前に出る窓が定まらない
    // ので閉じない。
    window.connect_is_active_notify(move |w| {
        if !w.is_active() && !crate::tabs::is_check_run() {
            let w = w.clone();
            glib::idle_add_local_once(move || {
                if !w.is_active() && gtk::Window::list_toplevels().iter().any(|t| t == &w) {
                    w.close();
                }
            });
        }
    });
    window.present();
    field.grab_focus();
    if ENTER_ONCE.with(|e| e.replace(false)) {
        field.emit_activate();
    }
    window
}

/// 行を描く: ローカルのブランチ（ほかの worktree が使っていれば緑のフォルダー、最近のものには印）、リモートの
/// ブランチ（薄く）、作る行（強調の色）。
fn render(row: &Row, choice: &Choice, recent: &[String], actions: &Actions) {
    for css in ["success", "dim-label", "accent"] {
        row.icon.remove_css_class(css);
        row.label.remove_css_class(css);
    }
    let (kind, text, css, accent) = match choice {
        Choice::Local(name) => match actions.worktree_using(name) {
            Some((worktree, _)) => (
                icon::Kind::Folder,
                format!(
                    "{name}{}",
                    tr!("  · in the worktree {name}", name = worktree)
                ),
                Some("success"),
                false,
            ),
            None if recent.contains(name) => (
                icon::Kind::Branch,
                format!("{name}{}", tr!("  · recent")),
                None,
                false,
            ),
            None => (icon::Kind::Branch, name.clone(), None, false),
        },
        Choice::Remote(name) => (
            icon::Kind::Remote,
            format!("{name}{}", tr!("  · creates a tracking branch")),
            Some("dim-label"),
            false,
        ),
        Choice::Create(name) => (
            icon::Kind::PlusCircle,
            tr!("Create branch {name} and switch to it", name = name),
            Some("accent"),
            true,
        ),
    };
    row.icon.set(kind, accent);
    row.label.set_text(&text);
    if let Some(css) = css {
        row.icon.add_css_class(css);
        row.label.add_css_class(css);
    }
}

/// 選んだものに切り替える。選んだこと自体を確かめとし、シートは出さない（macOS 版と同じ）。ローカルのブランチは
/// checkout（ほかの worktree が使っていれば、git と同じく押し通さず、その worktree に移る）、リモートのブランチは
/// 追跡ブランチを作って、作る行はそのまま作って切り替える。
fn choose(actions: &Rc<Actions>, choice: Choice) {
    match choice {
        Choice::Local(name) => match actions.worktree_using(&name) {
            Some((_, path)) => actions.edit_worktree(&path),
            None => actions.perform(json!({ "op": "checkout", "branch": name })),
        },
        Choice::Remote(name) => actions.perform(json!({ "op": "track", "remoteBranch": name })),
        Choice::Create(name) => actions.perform(
            json!({ "op": "createBranch", "name": name, "start": "HEAD", "checkout": true }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_model::RefEntry;

    fn entry(kind: RefKind, name: &str) -> RefEntry {
        RefEntry {
            kind,
            name: name.into(),
            target: None,
            detail: None,
            ahead: 0,
            behind: 0,
            gone: false,
        }
    }

    fn refs() -> Refs {
        Refs {
            head_branch: Some("main".into()),
            head: None,
            entries: vec![
                entry(RefKind::Local, "alpha"),
                entry(RefKind::Local, "main"),
                entry(RefKind::Local, "zeta"),
                entry(RefKind::Remote, "origin/HEAD"),
                entry(RefKind::Remote, "origin/main"),
                entry(RefKind::Remote, "origin/topic"),
            ],
        }
    }

    #[test]
    fn recent_first_then_names_then_remotes() {
        let shown = candidates(&refs(), &["zeta".into()], "");
        assert_eq!(
            shown,
            vec![
                Choice::Local("zeta".into()),
                Choice::Local("alpha".into()),
                Choice::Remote("origin/topic".into()),
            ]
        );
    }

    #[test]
    fn query_filters_and_offers_a_new_branch() {
        let shown = candidates(&refs(), &[], "top");
        assert_eq!(
            shown,
            vec![
                Choice::Remote("origin/topic".into()),
                Choice::Create("top".into()),
            ]
        );
        // 今のブランチの名前や、空白を含む名前では作る行を出さない。
        assert!(!candidates(&refs(), &[], "main").contains(&Choice::Create("main".into())));
        assert!(
            candidates(&refs(), &[], "a b")
                .iter()
                .all(|c| !matches!(c, Choice::Create(_)))
        );
    }
}
