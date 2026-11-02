//! ツールバーのリポジトリ名（macOS 版の `RepositoryBrowser` の `compact` と `RepositoryWindow.showRepositoryList`）:
//! 押すとリポジトリの一覧を小さく出し、選んだものにこのタブを切り替える（Ctrl+Enter なら新しい窓）。お気に入り、
//! そのあと最後に開いた順に、名前・パス・ブランチ・状態を出す（ダブルクリックか Enter で開く）。下の「すべての
//! リポジトリ…」はリポジトリのページを出す。

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;

use crate::bookmarks;
use crate::menu;
use crate::repobrowser::{self, Open, RowOpen};
use crate::tabs::Place;

/// 一覧の 1 行（名前・パス・ブランチ・状態。macOS 版の `RepositoryCell` の小さい形）。
struct Row {
    name: gtk::Label,
    path: gtk::Label,
    branch: gtk::Label,
    state: gtk::Label,
    location: RefCell<String>,
}

impl Row {
    fn render(&self) {
        let location = self.location.borrow().clone();
        self.name.set_markup(&repobrowser::name_markup(&location));
        let path = repobrowser::path_text(&location);
        self.path.set_text(&path);
        self.path.set_tooltip_text(Some(&path));
        self.branch.set_text(&repobrowser::branch_text(&location));
        repobrowser::show_state(&self.state, &location, false);
    }
}

/// 選んでいる行の場所（並びの順）。
fn selected_locations(selection: &gtk::MultiSelection) -> Vec<String> {
    let set = selection.selection();
    (0..set.size())
        .filter_map(|i| {
            selection
                .item(set.nth(i as u32))
                .and_downcast::<gtk::StringObject>()
                .map(|s| s.string().to_string())
        })
        .collect()
}

/// 選んでいる最初の行（Enter で開くもの・↑↓ の起点）。
fn first_selected(selection: &gtk::MultiSelection) -> Option<u32> {
    let set = selection.selection();
    (!set.is_empty()).then(|| set.minimum())
}

/// 一覧の並び: お気に入り、そのあと最後に開いた順（開いたことのないものは後ろに、名前の順）。
fn ordered(query: &str) -> Vec<String> {
    // ブックマークと開いた日時は一度だけ読む（行ごとに読むと、数百行で目に見えて遅い）。
    let data = bookmarks::data();
    let opened = crate::repos::opened_all_at();
    let favorites: std::collections::HashSet<String> = data
        .items
        .iter()
        .filter(|b| b.favorite)
        .map(|b| b.path.clone())
        .collect();
    let query = query.trim().to_lowercase();
    // 追加した worktree は、メインのリポジトリも一覧にあれば出さない（リポジトリのページと同じ）。
    let all = bookmarks::all_locations();
    let hidden = repobrowser::hidden_worktrees(&all);
    let mut list: Vec<String> = all
        .into_iter()
        .filter(|l| !hidden.contains(l))
        .filter(|l| {
            query.is_empty()
                || bookmarks::display_name_in(&data, l)
                    .to_lowercase()
                    .contains(&query)
                || l.to_lowercase().contains(&query)
        })
        .collect();
    list.sort_by_cached_key(|l| {
        (
            !favorites.contains(l),
            std::cmp::Reverse(opened.get(l).copied().unwrap_or(i64::MIN)),
            bookmarks::display_name_in(&data, l).to_lowercase(),
        )
    });
    list
}

/// リポジトリ名のボタン。`titles` を中身にし、押すと一覧を出す。`open` は選んだもの（`Place::Tab` ならこのタブを
/// 切り替える）、`beside` は行の右クリックの「新しいタブで開く」（このタブはそのまま）、`show_all` はリポジトリの
/// ページを出す。
pub fn button(
    titles: &gtk::Box,
    name: &str,
    open: Open,
    beside: Open,
    show_all: Rc<dyn Fn()>,
) -> gtk::MenuButton {
    let popover = gtk::Popover::new();
    // 行の右クリックのメニュー（リポジトリのページと同じもの。macOS 版と同じ）から開くとき。一覧は閉じる。
    let menu_open: repobrowser::RowOpener = {
        let (p, open) = (popover.downgrade(), open.clone());
        Rc::new(move |location, how| {
            let Some(p) = p.upgrade() else { return };
            let window = p.root().and_downcast::<gtk::Window>();
            p.popdown();
            // 見つからなければ、一覧から外すか聞く（リポジトリのページと同じ。macOS 版の `open`）。
            if let Some(window) = window
                && !repobrowser::found_or_offer_removal(&window, &location, || {})
            {
                return;
            }
            let location = PathBuf::from(location);
            match how {
                RowOpen::Here => open(location, Place::Tab),
                RowOpen::NewTab => beside(location, Place::Tab),
                RowOpen::NewWindow => open(location, Place::NewWindow),
            }
        })
    };
    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some(&tr!("Switch repository")));
    let model = gtk::StringList::new(&[]);
    // Ctrl・Shift+クリックでいくつも選べる（macOS 版と同じ。右クリックの Fetch・Group などは選んだものすべてに）。
    let selection = gtk::MultiSelection::new(Some(model.clone()));
    let factory = gtk::SignalListItemFactory::new();
    let rows: Rc<RefCell<Vec<Rc<Row>>>> = Rc::default();
    let (r, s) = (rows.clone(), selection.downgrade());
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        // 左に名前と、その下にパス。右にブランチと、その下に状態（macOS 版と同じ）。
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let path = gtk::Label::new(None);
        path.set_xalign(0.0);
        path.set_hexpand(true);
        path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        path.add_css_class("dim-label");
        path.add_css_class("caption");
        let branch = gtk::Label::new(None);
        branch.set_xalign(1.0);
        branch.set_ellipsize(gtk::pango::EllipsizeMode::End);
        branch.set_max_width_chars(24);
        branch.add_css_class("dim-label");
        let state = gtk::Label::new(None);
        state.set_xalign(1.0);
        state.add_css_class("caption");
        let line = gtk::Grid::new();
        line.set_column_spacing(8);
        line.set_margin_top(2);
        line.set_margin_bottom(2);
        line.set_margin_start(2);
        line.set_margin_end(2);
        line.attach(&name, 0, 0, 1, 1);
        line.attach(&path, 0, 1, 1, 1);
        line.attach(&branch, 1, 0, 1, 1);
        line.attach(&state, 1, 1, 1, 1);
        item.set_child(Some(&line));
        let row = Rc::new(Row {
            name,
            path,
            branch,
            state,
            location: RefCell::default(),
        });
        // 右クリックで、その行を選んでメニュー（macOS 版と同じ）。
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (w, list_item, s, o) = (
            Rc::downgrade(&row),
            item.downgrade(),
            s.clone(),
            menu_open.clone(),
        );
        click.connect_pressed(move |gesture, _, x, y| {
            let (Some(row), Some(item), Some(widget)) =
                (w.upgrade(), list_item.upgrade(), gesture.widget())
            else {
                return;
            };
            let location = row.location.borrow().clone();
            let Some(window) = widget.root().and_downcast::<gtk::Window>() else {
                return;
            };
            // 選んでいる行の上なら選んだものすべて、そうでなければその行だけを選んで（macOS 版と同じ）。
            let mut targets = vec![location.clone()];
            if let Some(s) = s.upgrade()
                && item.position() != gtk::INVALID_LIST_POSITION
            {
                if s.is_selected(item.position()) {
                    targets = selected_locations(&s);
                } else {
                    s.select_item(item.position(), true);
                }
            }
            let items = repobrowser::row_items(&window, &location, &targets, false, o.clone());
            menu::popup(&widget, x, y, items);
        });
        line.add_controller(click);
        let mut rows = r.borrow_mut();
        rows.retain(|row| row.name.parent().is_some());
        rows.push(row);
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
    // クリックで選び、ダブルクリック（か Enter）で開く（macOS 版と同じ）。
    list.set_single_click_activate(false);
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
                selection.select_item(0, true);
            }
        })
    };
    let u = update.clone();
    filter.connect_search_changed(move |_| u());
    // 出すたびに読み直す（ブックマーク・スキャンの結果・行の状態が変わったら、出している間は描き直す）。
    let (u, f, sc) = (update.clone(), filter.clone(), scanning.clone());
    let (r, p) = (rows.clone(), popover.downgrade());
    // 出すたびに聞き手を足すので、前に出したときの分は外す（隠れている間に知らせが来なかったときも残らない）。
    let shows = Rc::new(Cell::new(0u32));
    popover.connect_show(move |_| {
        shows.set(shows.get() + 1);
        let (shows, show) = (shows.clone(), shows.get());
        f.set_text("");
        // 出すたびに、行の状態を読み直し、スキャンするフォルダーを探し直す（macOS 版の `prepare`）。
        bookmarks::refresh();
        bookmarks::forget_peeks();
        bookmarks::rescan();
        u();
        f.grab_focus();
        let (u, sc, r, p) = (u.clone(), sc.clone(), r.clone(), p.clone());
        bookmarks::connect(move |path| {
            if shows.get() != show || !p.upgrade().is_some_and(|p| p.is_visible()) {
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
            let Some(p) = p.upgrade() else { return };
            let window = p.root().and_downcast::<gtk::Window>();
            p.popdown();
            // 見つからなければ、一覧から外すか聞く（リポジトリのページと同じ。macOS 版の `open`）。
            if let Some(window) = window
                && !repobrowser::found_or_offer_removal(&window, &location, || {})
            {
                return;
            }
            open(PathBuf::from(location), place);
        })
    };
    // ダブルクリック・Enter で開く。Ctrl を押しながらなら新しい窓に（macOS 版の ⌘。リポジトリのページと同じ）。
    // 開くときの知らせには押していたキーがないので、押したときに覚えておく。
    let ctrl = Rc::new(Cell::new(false));
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
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let c = ctrl.clone();
    keys.connect_key_pressed(move |_, _, _, modifiers| {
        c.set(modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK));
        glib::Propagation::Proceed
    });
    list.add_controller(keys);
    let g = go.clone();
    list.connect_activate(move |_, row| {
        g(
            row,
            if ctrl.get() {
                Place::NewWindow
            } else {
                Place::Tab
            },
        )
    });
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
        if let Some(row) = first_selected(&s) {
            g(row, Place::Tab);
        }
    });
    // Esc: 欄（SearchEntry）が先に受けて popover まで届かないので、ここで閉じる（Quick Open・ブランチの切り替えと同じ）。
    // 打ってあれば、まず消す（もう一度で閉じる。macOS 版と同じ）。
    let p = popover.downgrade();
    filter.connect_stop_search(move |f| {
        if !f.text().is_empty() {
            f.set_text("");
        } else if let Some(p) = p.upgrade() {
            p.popdown();
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
                if let Some(row) = first_selected(&s) {
                    g(row, Place::NewWindow);
                }
                return glib::Propagation::Stop;
            }
            _ => return glib::Propagation::Proceed,
        };
        let n = s.n_items() as i64;
        if n > 0 {
            let current = first_selected(&s).map_or(-1, i64::from);
            let next = (current + step).clamp(0, n - 1) as u32;
            s.select_item(next, true);
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
