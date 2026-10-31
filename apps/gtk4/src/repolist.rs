//! ツールバーのリポジトリ名（macOS 版の `RepositoryBrowser` の `compact` と `RepositoryWindow.showRepositoryList`）:
//! 押すとリポジトリの一覧を小さく出し、選んだものにこのタブを切り替える（Ctrl+Enter なら新しい窓）。お気に入り、
//! そのあと最後に開いた順に、名前と状態だけを出す。下の「すべてのリポジトリ…」はリポジトリのページを出す。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;

use crate::bookmarks;
use crate::repobrowser::{self, Open};
use crate::tabs::Place;

/// 一覧の 1 行（名前と状態）。
struct Row {
    name: gtk::Label,
    state: gtk::Label,
    location: RefCell<String>,
}

impl Row {
    fn render(&self) {
        let location = self.location.borrow().clone();
        self.name.set_markup(&repobrowser::name_markup(&location));
        self.name
            .set_tooltip_text(Some(&repobrowser::path_text(&location)));
        let (markup, tip) = repobrowser::state_markup(&location, false);
        self.state.set_markup(&markup);
        self.state.set_tooltip_text(tip.as_deref());
    }
}

/// 一覧の並び: お気に入り、そのあと最後に開いた順（開いたことのないものは後ろに、名前の順）。
fn ordered(query: &str) -> Vec<String> {
    let favorites: Vec<String> = bookmarks::data()
        .items
        .iter()
        .filter(|b| b.favorite)
        .map(|b| b.path.clone())
        .collect();
    let query = query.trim().to_lowercase();
    let mut list: Vec<String> = bookmarks::all_locations()
        .into_iter()
        .filter(|l| {
            query.is_empty()
                || bookmarks::display_name(l).to_lowercase().contains(&query)
                || l.to_lowercase().contains(&query)
        })
        .collect();
    list.sort_by_cached_key(|l| {
        (
            !favorites.contains(l),
            std::cmp::Reverse(crate::repos::opened_at(l).unwrap_or(i64::MIN)),
            bookmarks::display_name(l).to_lowercase(),
        )
    });
    list
}

/// リポジトリ名のボタン。`titles` を中身にし、押すと一覧を出す。`open` は選んだもの（`Place::Tab` ならこのタブを
/// 切り替える）、`show_all` はリポジトリのページを出す。
pub fn button(
    titles: &gtk::Box,
    name: &str,
    open: Open,
    show_all: Rc<dyn Fn()>,
) -> gtk::MenuButton {
    let popover = gtk::Popover::new();
    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some(&tr!("Filter by name or path")));
    let model = gtk::StringList::new(&[]);
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    let factory = gtk::SignalListItemFactory::new();
    let rows: Rc<RefCell<Vec<Rc<Row>>>> = Rc::default();
    let r = rows.clone();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let state = gtk::Label::new(None);
        state.set_xalign(1.0);
        state.add_css_class("caption");
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(2);
        line.set_margin_bottom(2);
        line.set_margin_start(2);
        line.set_margin_end(2);
        line.append(&name);
        line.append(&state);
        item.set_child(Some(&line));
        let mut rows = r.borrow_mut();
        rows.retain(|row| row.name.parent().is_some());
        rows.push(Rc::new(Row {
            name,
            state,
            location: RefCell::default(),
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
            .and_downcast::<gtk::Label>();
        let row = name.and_then(|n| r.borrow().iter().find(|row| row.name == n).cloned());
        if let Some(row) = row {
            *row.location.borrow_mut() = location;
            row.render();
        }
    });
    let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
    list.add_css_class("navigation-sidebar");
    list.set_single_click_activate(true);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(360)
        .vexpand(true)
        .build();
    let scanning = gtk::Spinner::new();
    scanning.set_visible(false);
    let all = gtk::Button::with_label(&tr!("All Repositories…"));
    all.set_tooltip_text(Some(&tr!("Open the repository page (Ctrl+Shift+R)")));
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bottom.append(&scanning);
    bottom.append(&spacer);
    bottom.append(&all);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_size_request(420, -1);
    content.set_margin_top(4);
    content.set_margin_bottom(4);
    content.set_margin_start(4);
    content.set_margin_end(4);
    content.append(&filter);
    content.append(&scrolled);
    content.append(&bottom);
    popover.set_child(Some(&content));

    let update = {
        let (model, selection, filter) = (model.clone(), selection.clone(), filter.clone());
        Rc::new(move || {
            let shown = ordered(&filter.text());
            let refs: Vec<&str> = shown.iter().map(String::as_str).collect();
            model.splice(0, model.n_items(), &refs);
            if !shown.is_empty() {
                selection.set_selected(0);
            }
        })
    };
    let u = update.clone();
    filter.connect_search_changed(move |_| u());
    // 出すたびに読み直す（ブックマーク・スキャンの結果・行の状態が変わったら、出している間は描き直す）。
    let (u, f, sc) = (update.clone(), filter.clone(), scanning.clone());
    let (r, p) = (rows.clone(), popover.downgrade());
    popover.connect_show(move |_| {
        f.set_text("");
        bookmarks::refresh();
        if !bookmarks::has_scanned() && !bookmarks::data().scan_roots.is_empty() {
            bookmarks::rescan();
        }
        u();
        f.grab_focus();
        let (u, sc, r, p) = (u.clone(), sc.clone(), r.clone(), p.clone());
        bookmarks::connect(move |path| {
            if !p.upgrade().is_some_and(|p| p.is_visible()) {
                return false;
            }
            match path {
                Some(path) => {
                    for row in r.borrow().iter() {
                        if *row.location.borrow() == path {
                            row.render();
                        }
                    }
                }
                None => {
                    sc.set_visible(bookmarks::is_scanning());
                    sc.set_spinning(bookmarks::is_scanning());
                    u();
                }
            }
            true
        });
    });
    let go = {
        let (p, selection) = (popover.downgrade(), selection.clone());
        Rc::new(move |row: u32, place: Place| {
            let Some(location) = selection
                .item(row)
                .and_downcast::<gtk::StringObject>()
                .map(|s| s.string().to_string())
            else {
                return;
            };
            if let Some(p) = p.upgrade() {
                p.popdown();
            }
            open(PathBuf::from(location), place);
        })
    };
    let g = go.clone();
    list.connect_activate(move |_, row| g(row, Place::Tab));
    // 確認用（`--open-menu repository-name` と）: 出したら、この表示名の行を選ぶ。
    if let Ok(name) = std::env::var("AMONHEN_CHOOSE_REPOSITORY") {
        let (g, m) = (go.clone(), model.clone());
        popover.connect_show(move |_| {
            let (g, m, name) = (g.clone(), m.clone(), name.clone());
            glib::idle_add_local_once(move || {
                let row = (0..m.n_items()).find(|&i| {
                    m.string(i)
                        .is_some_and(|l| bookmarks::display_name(&l) == name)
                });
                match row {
                    Some(row) => g(row, Place::Tab),
                    None => eprintln!("一覧にない: {name}"),
                }
            });
        });
    }
    let (g, s) = (go.clone(), selection.clone());
    filter.connect_activate(move |_| {
        if s.selected() != gtk::INVALID_LIST_POSITION {
            g(s.selected(), Place::Tab);
        }
    });
    let p = popover.downgrade();
    all.connect_clicked(move |_| {
        if let Some(p) = p.upgrade() {
            p.popdown();
        }
        show_all();
    });
    // 欄にいたまま ↑↓ で選ぶ。Ctrl+Enter は新しい窓に（欄の Enter より先に受ける）。
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
                    g(s.selected(), Place::NewWindow);
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
    filter.add_controller(keys);

    let button = gtk::MenuButton::builder()
        .child(titles)
        .popover(&popover)
        .has_frame(false)
        .tooltip_text(tr!("{name} — click to switch repository", name = name))
        .build();
    button.set_always_show_arrow(false);
    button
}
