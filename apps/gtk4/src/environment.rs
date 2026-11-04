//! Settings > Git > Environment Variables（macOS 版の `MachineSettings.swift` の `EnvironmentPage`・`EnvironmentTable`・
//! `EnvironmentValueView`）: このアプリが動かす git と ssh に渡す環境変数。変数を値と一緒に並べ、渡すものをチェックで
//! 選ぶ。git と ssh がいつも使うもの（コアの `ALWAYS_PASSED` と `GIT_*`）は外せず、渡せないもの（`env_refusal`）は
//! 選べない。選んだ名前はアプリの設定に置き、`AMONHEN_PASS_ENV` にしてすぐ効かせる（共有の daemon への今のつながりにも）。
//!
//! macOS 版にある「ログインシェルの環境変数を使う」は置かない。Finder から起動したアプリが launchd の素の環境
//! （PATH も SSH の agent もない）しか持たないことへの手当てで、Linux のデスクトップから起動したアプリは、ログインの
//! ときに作られたセッションの環境（`~/.profile` の PATH・agent）を引き継ぐ。表には、このプロセスの環境を並べる。
//!
//! SSH 先のページ（Settings > SSH Hosts）は、SSH 先のエンジンの環境（コアの `processEnvironment`）を読んで並べる。
//! SSH 先では、エンジンの環境がすべて git に渡るので、渡すかどうかを選ぶ列はない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use amonhen_model::process::{ALWAYS_PASSED, PASS_ENV, always_passed, env_refusal};
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::reposettings::{RepoSettings, label, run_one};

/// 足した名前（コンマ区切り。macOS 版の `addedEnvironmentNames` と同じ鍵）。
const ADDED: &str = "addedEnvironmentNames";
/// 表で前に選んだ行。
const SELECTION: &str = "environmentTableSelection";

/// 足した名前（渡せないものは除く）。
pub fn added_names() -> Vec<String> {
    crate::settings::string(ADDED)
        .unwrap_or_default()
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|n| !n.is_empty() && env_refusal(n).is_none())
        .map(str::to_owned)
        .collect()
}

/// 足した名前を `AMONHEN_PASS_ENV` に置く（コアが渡す名前を決め、共有の daemon にも伝わる）。起動の初め（ほかの
/// スレッドを動かす前）に呼ぶ。
pub fn apply_added_names() {
    let mut names = added_names();
    names.sort();
    names.dedup();
    // SAFETY: 起動の初めは、まだほかのスレッドがない。設定を変えたときは macOS 版の `setenv` と同じく動いている
    // 間に変える。環境を読むのはコア（std の `env::var`・`Command`）で、std の中の鍵で守られている。
    unsafe {
        if names.is_empty() {
            std::env::remove_var(PASS_ENV);
        } else {
            std::env::set_var(PASS_ENV, names.join(","));
        }
    }
}

/// 足した名前の設定の字（Undo の記録に使う）。
fn added_text() -> String {
    crate::settings::string(ADDED).unwrap_or_default()
}

/// 足した名前の字を `added_names` の形にする（並べ替えて重なりを除く）。
fn added_text_of(names: &[String]) -> String {
    let mut names = names.to_vec();
    names.sort();
    names.dedup();
    names.join(", ")
}

/// 足した名前（設定の字）を変えて、すぐ効かせる（共有の daemon への今のつながりにも送る）。Repository Settings の
/// Undo からも呼ぶ。
pub(crate) fn set_added_text(text: &str) {
    crate::settings::set_string(ADDED, text);
    apply_added_names();
    glib::spawn_future_local(async {
        let _ = gio::spawn_blocking(amonhen_remote::local::update_environment).await;
    });
}

/// トークン・パスワード・鍵らしい名前か（値を伏せる。macOS 版の `SecretNames`）。名前を英数字以外で語に区切り、
/// どれかの語が TOKEN・SECRET・PASSWORD・PASSWD・CREDENTIAL・PRIVATE を含むか、AUTH・PASS そのものか、KEY で
/// 終われば伏せる。SSH の agent の場所と、渡す変数の一覧は出す。
pub fn is_secret(name: &str) -> bool {
    let upper = name.to_uppercase();
    if upper == "SSH_AUTH_SOCK" || upper == PASS_ENV {
        return false;
    }
    upper
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|word| {
            [
                "TOKEN",
                "SECRET",
                "PASSWORD",
                "PASSWD",
                "CREDENTIAL",
                "PRIVATE",
            ]
            .iter()
            .any(|c| word.contains(c))
                || word == "AUTH"
                || word == "PASS"
                || word.ends_with("KEY")
        })
}

/// 検索に合うか。伏せた値は探さない（一部を入れて当たるかで、値を当てられてしまう）。名前では探せる。
fn matches(name: &str, value: &str, filter: &str) -> bool {
    let f = filter.to_lowercase();
    name.to_lowercase().contains(&f) || (!is_secret(name) && value.to_lowercase().contains(&f))
}

/// 行の扱い。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    /// いつも渡す（外せない）。
    Always,
    /// 足したもの。
    Chosen,
    Other,
    /// 渡せない（理由）。
    Refused(String),
}

impl Kind {
    fn of(name: &str, added: &[String]) -> Self {
        if let Some(reason) = env_refusal(name) {
            Kind::Refused(reason)
        } else if always_passed(name) {
            Kind::Always
        } else if added.iter().any(|a| a == name) {
            Kind::Chosen
        } else {
            Kind::Other
        }
    }

    fn passed(&self) -> bool {
        matches!(self, Kind::Always | Kind::Chosen)
    }

    fn help(&self) -> String {
        match self {
            Kind::Always => tr!("git and ssh always get this"),
            Kind::Refused(reason) => reason.clone(),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct Row {
    name: String,
    value: String,
    /// この機械の表なら、その扱い（SSH 先の表は None。すべて git に渡る）。
    kind: Option<Kind>,
}

/// PATH のように場所を並べた変数か。
fn is_list(name: &str) -> bool {
    name.ends_with("PATH") || name.ends_with("_DIRS") || name.ends_with("PATHS")
}

fn places(value: &str, windows: bool) -> Vec<String> {
    value
        .split(if windows { ';' } else { ':' })
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 表と、選んだ変数の値の全体。
struct Table {
    /// 表に出す変数（名前の順）。
    vars: RefCell<Vec<(String, String)>>,
    /// この機械の表か（渡すかどうかの列がある）。
    local: bool,
    added: RefCell<Vec<String>>,
    show_all: Cell<bool>,
    filter: RefCell<String>,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    /// 選んだ行の名前と、伏せた値を見せたか。
    selected: RefCell<String>,
    revealed: Cell<bool>,
    /// 値の全体の欄。
    detail: gtk::Box,
    /// PATH で選んだ git（PATH のものなら None）と、SSH 先の PATH で見つかる git のフォルダ。
    chosen_git: RefCell<Option<String>>,
    git_directory: RefCell<Option<String>>,
    /// 欄に値を入れている間（チェックの知らせで書かない）。
    binding: Cell<bool>,
    /// Windows の SSH 先か（場所の区切りが `;`）。
    windows: Cell<bool>,
    /// 渡すかの書き換えを記録する設定の窓（この機械の表だけ）。
    settings: RefCell<std::rc::Weak<RepoSettings>>,
    /// 表の部品（作り直しでフォーカスを失ったら戻す先）。
    column_view: glib::WeakRef<gtk::ColumnView>,
}

fn row_of(o: &glib::Object) -> Option<Row> {
    o.downcast_ref::<glib::BoxedAnyObject>()
        .map(|b| b.borrow::<Row>().clone())
}

impl Table {
    fn new(local: bool) -> Rc<Self> {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let detail = gtk::Box::new(gtk::Orientation::Vertical, 4);
        Rc::new(Self {
            vars: RefCell::default(),
            local,
            added: RefCell::new(added_names()),
            show_all: Cell::new(true),
            filter: RefCell::default(),
            store,
            selection,
            selected: RefCell::new(if local {
                crate::settings::string(SELECTION).unwrap_or_default()
            } else {
                String::new()
            }),
            revealed: Cell::new(false),
            detail,
            chosen_git: RefCell::default(),
            git_directory: RefCell::default(),
            binding: Cell::new(false),
            windows: Cell::new(false),
            settings: RefCell::default(),
            column_view: glib::WeakRef::new(),
        })
    }

    /// 並べ直す（渡すものを先に、そのあとにほかの変数。どちらも名前の順）。
    fn rebuild(self: &Rc<Self>) {
        let added = self.added.borrow().clone();
        let filter = self.filter.borrow().trim().to_owned();
        let mut rows: Vec<Row> = self
            .vars
            .borrow()
            .iter()
            .map(|(name, value)| Row {
                name: name.clone(),
                value: value.clone(),
                kind: self.local.then(|| Kind::of(name, &added)),
            })
            .filter(|r| self.show_all.get() || r.kind.as_ref().is_none_or(Kind::passed))
            .filter(|r| filter.is_empty() || matches(&r.name, &r.value, &filter))
            .collect();
        rows.sort_by_key(|r| !r.kind.as_ref().is_some_and(Kind::passed));
        let items: Vec<glib::BoxedAnyObject> = rows
            .iter()
            .cloned()
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.binding.set(true);
        self.store.splice(0, self.store.n_items(), &items);
        let selected = self.selected.borrow().clone();
        let position = rows.iter().position(|r| r.name == selected);
        self.selection
            .set_selected(position.map_or(gtk::INVALID_LIST_POSITION, |p| p as u32));
        self.binding.set(false);
        self.show_detail();
    }

    /// 渡すかを変える（足した名前に入れる・外す）。Undo できるように、設定の窓の書き換えとして記録する（macOS
    /// 版と同じ）。表は設定の知らせで作り直す（`page`）。
    fn set_passed(self: &Rc<Self>, name: &str, on: bool) {
        let mut names = self.added.borrow().clone();
        names.retain(|n| n != name);
        if on {
            names.push(name.to_owned());
        }
        let (old, new) = (added_text(), added_text_of(&names));
        if old == new {
            return;
        }
        match self.settings.borrow().upgrade() {
            Some(s) => s.apply_changes(
                vec![crate::reposettings::Change {
                    key: crate::reposettings::ADDED_ENVIRONMENT.to_owned(),
                    old: Some(old),
                    new: Some(new),
                    ..Default::default()
                }],
                crate::reposettings::Record::New,
            ),
            None => set_added_text(&new),
        }
    }

    /// 足した名前が変わった（ここで変えたか Undo）: 読み直して作り直す（チェックの知らせの中では作り直さない）。
    fn added_changed(self: &Rc<Self>) {
        self.added.replace(added_names());
        let w = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(t) = w.upgrade() {
                // 表の中（押したチェック）にフォーカスがあったなら、作り直した後も表に置く。チェックの部品が外れて
                // フォーカスがなくなると、Ctrl+Z（Undo）が設定のページに届かない。
                let view = t.column_view.upgrade();
                let inside = |view: &gtk::ColumnView| {
                    view.root().and_then(|r| r.focus()).is_some_and(|f| {
                        &f == view.upcast_ref::<gtk::Widget>() || f.is_ancestor(view)
                    })
                };
                let lost =
                    |view: &gtk::ColumnView| view.root().is_some_and(|r| r.focus().is_none());
                let had = view.as_ref().is_some_and(|v| inside(v) || lost(v));
                t.rebuild();
                if let Some(view) = view
                    && had
                    && !inside(&view)
                {
                    view.grab_focus();
                }
            }
        });
    }

    /// 選んだ変数の値の全体。PATH のように場所を並べたものは 1 行に 1 か所に分け、その場所があるか、前に同じ場所が
    /// あるか、PATH ならどこの git が使われるかを添える。伏せた値は「表示」で見せる。
    fn show_detail(self: &Rc<Self>) {
        while let Some(c) = self.detail.first_child() {
            self.detail.remove(&c);
        }
        let name = self.selected.borrow().clone();
        let value = self
            .vars
            .borrow()
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone());
        let Some(value) = value.filter(|_| !name.is_empty()) else {
            let l = label(
                &tr!("Select a variable to see its whole value."),
                &["dim-label", "caption"],
            );
            l.set_size_request(-1, 40);
            l.set_valign(gtk::Align::Start);
            self.detail.append(&l);
            return;
        };
        let masked = is_secret(&name) && !self.revealed.get();
        let windows = self.windows.get();
        let list = is_list(&name);
        let places = places(&value, windows);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = label(
            &if list {
                tr!("{name} ({count} places)", name = name, count = places.len())
            } else {
                name.clone()
            },
            &["monospace", "heading"],
        );
        title.set_hexpand(true);
        head.append(&title);
        if masked {
            let show = gtk::Button::with_label(&tr!("Show"));
            show.add_css_class("flat");
            let w = Rc::downgrade(self);
            show.connect_clicked(move |_| {
                if let Some(t) = w.upgrade() {
                    t.revealed.set(true);
                    t.show_detail();
                }
            });
            head.append(&show);
        }
        self.detail.append(&head);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
        if masked {
            body.append(&label("••••••••", &["monospace", "dim-label"]));
        } else if list {
            self.place_list(&body, &name, &places);
        } else {
            let l = label(&value, &["monospace"]);
            l.set_selectable(true);
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::Char);
            body.append(&l);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .child(&body)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(120)
            .max_content_height(120)
            .build();
        self.detail.append(&scroll);
    }

    /// 1 行に 1 か所。番号、場所、添え書き（ない・前と同じ・ここの git を使う）。git を選んでいれば、PATH ではなく
    /// それを使うと書く。SSH 先の場所はこの機械では調べられないので、「ない」は出さない。
    fn place_list(&self, body: &gtk::Box, name: &str, places: &[String]) {
        let chosen = self.chosen_git.borrow().clone();
        let git = if name != "PATH" || chosen.is_some() {
            None
        } else if self.local {
            let file = if cfg!(windows) { "git.exe" } else { "git" };
            places.iter().position(|p| {
                std::fs::metadata(std::path::Path::new(p).join(file)).is_ok_and(|m| m.is_file())
            })
        } else {
            let dir = self.git_directory.borrow().clone();
            dir.and_then(|d| places.iter().position(|p| *p == d))
        };
        if name == "PATH"
            && let Some(chosen) = chosen
        {
            body.append(&label(
                &if self.local {
                    tr!(
                        "git is the one chosen in Settings > Git: {git}",
                        git = chosen
                    )
                } else {
                    tr!("git is the one chosen in Programs: {git}", git = chosen)
                },
                &["accent", "caption"],
            ));
        }
        let grid = gtk::Grid::new();
        grid.set_column_spacing(8);
        grid.set_row_spacing(2);
        for (i, place) in places.iter().enumerate() {
            let missing = self.local && !std::path::Path::new(place).exists();
            let repeated = places[..i].contains(place);
            let n = label(&(i + 1).to_string(), &["dim-label", "caption", "numeric"]);
            n.set_xalign(1.0);
            n.set_width_chars(3);
            grid.attach(&n, 0, i as i32, 1, 1);
            let p = label(place, &["monospace"]);
            p.set_selectable(true);
            if missing || repeated {
                p.add_css_class("dim-label");
            }
            if repeated {
                p.set_attributes(Some(&{
                    let a = gtk::pango::AttrList::new();
                    a.insert(gtk::pango::AttrInt::new_strikethrough(true));
                    a
                }));
            }
            grid.attach(&p, 1, i as i32, 1, 1);
            let mut notes = Vec::new();
            if git == Some(i) {
                notes.push((tr!("git used from here"), "accent"));
            }
            if missing {
                notes.push((tr!("doesn’t exist"), "dim-label"));
            }
            if repeated {
                notes.push((tr!("already above"), "dim-label"));
            }
            for (j, (text, css)) in notes.into_iter().enumerate() {
                grid.attach(
                    &label(&text, &[css, "caption"]),
                    2 + j as i32,
                    i as i32,
                    1,
                    1,
                );
            }
        }
        body.append(&grid);
    }

    /// 表の部品（ColumnView。見えている行だけに部品を作る）。
    fn view(self: &Rc<Self>) -> gtk::ScrolledWindow {
        let view = gtk::ColumnView::new(Some(self.selection.clone()));
        self.column_view.set(Some(&view));
        view.set_show_column_separators(false);
        view.set_reorderable(false);
        let w = Rc::downgrade(self);
        self.selection.connect_selected_item_notify(move |sel| {
            let Some(t) = w.upgrade() else { return };
            if t.binding.get() {
                return;
            }
            let name = sel
                .selected_item()
                .and_then(|o| row_of(&o))
                .map(|r| r.name)
                .unwrap_or_default();
            if t.local {
                crate::settings::set_string(SELECTION, &name);
            }
            t.selected.replace(name);
            t.revealed.set(false);
            t.show_detail();
        });

        if self.local {
            // 渡すか（外せないもの・渡せないものは選べない）。
            let factory = gtk::SignalListItemFactory::new();
            let w = Rc::downgrade(self);
            factory.connect_setup(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let check = gtk::CheckButton::new();
                check.set_halign(gtk::Align::Center);
                let w = w.clone();
                check.connect_toggled(move |c| {
                    if let Some(t) = w.upgrade()
                        && !t.binding.get()
                    {
                        t.set_passed(&c.widget_name(), c.is_active());
                    }
                });
                item.set_child(Some(&check));
            });
            let w = Rc::downgrade(self);
            factory.connect_bind(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let (Some(check), Some(row), Some(t)) = (
                    item.child().and_downcast::<gtk::CheckButton>(),
                    item.item().and_then(|o| row_of(&o)),
                    w.upgrade(),
                ) else {
                    return;
                };
                let kind = row.kind.unwrap_or(Kind::Other);
                let was = t.binding.replace(true);
                check.set_widget_name(&row.name);
                check.set_active(kind.passed());
                check.set_sensitive(matches!(kind, Kind::Chosen | Kind::Other));
                let help = kind.help();
                check.set_tooltip_text((!help.is_empty()).then_some(help.as_str()));
                t.binding.set(was);
            });
            let column = gtk::ColumnViewColumn::new(Some(&tr!("Pass")), Some(factory));
            column.set_fixed_width(56);
            view.append_column(&column);
        }

        // 名前（いつも渡すものには印）。
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let name = label("", &["monospace"]);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            b.append(&name);
            let badge = label("Always", &["dim-label", "caption", "env-badge"]);
            b.append(&badge);
            item.set_child(Some(&b));
        });
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(b), Some(row)) = (
                item.child().and_downcast::<gtk::Box>(),
                item.item().and_then(|o| row_of(&o)),
            ) else {
                return;
            };
            let (Some(name), Some(badge)) = (
                b.first_child().and_downcast::<gtk::Label>(),
                b.last_child().and_downcast::<gtk::Label>(),
            ) else {
                return;
            };
            name.set_text(&row.name);
            let refused = matches!(row.kind, Some(Kind::Refused(_)));
            if refused {
                name.add_css_class("dim-label");
            } else {
                name.remove_css_class("dim-label");
            }
            badge.set_visible(row.kind == Some(Kind::Always));
            let help = row.kind.map(|k| k.help()).unwrap_or_default();
            b.set_tooltip_text((!help.is_empty()).then_some(help.as_str()));
        });
        let column = gtk::ColumnViewColumn::new(Some(&tr!("Name")), Some(factory));
        column.set_resizable(true);
        column.set_fixed_width(240);
        view.append_column(&column);

        // 値（伏せるものは伏せる）。
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let l = label("", &["monospace", "dim-label"]);
            l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            item.set_child(Some(&l));
        });
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(l), Some(row)) = (
                item.child().and_downcast::<gtk::Label>(),
                item.item().and_then(|o| row_of(&o)),
            ) else {
                return;
            };
            let shown = if is_secret(&row.name) {
                "••••••••".to_owned()
            } else {
                row.value
            };
            l.set_text(&shown);
            l.set_tooltip_text(Some(&shown));
        });
        let column = gtk::ColumnViewColumn::new(Some(&tr!("Value")), Some(factory));
        column.set_expand(true);
        view.append_column(&column);

        gtk::ScrolledWindow::builder()
            .child(&view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .min_content_height(100)
            .has_frame(true)
            .build()
    }

    /// 検索の欄（名前か値で探す）。
    fn search(self: &Rc<Self>) -> gtk::SearchEntry {
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some(&tr!("Search by name or value")));
        search.set_width_chars(24);
        let w = Rc::downgrade(self);
        search.connect_search_changed(move |e| {
            if let Some(t) = w.upgrade() {
                t.filter.replace(e.text().to_string());
                t.rebuild();
            }
        });
        search
    }
}

fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".env-badge { border: 1px solid alpha(currentColor, 0.5); border-radius: 4px; \
         padding: 0 5px; } .accent { color: @theme_selected_bg_color; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// いつも渡す名前（説明に出す。Amon Hen のものは除く。macOS 版の `PassedEnvironment.alwaysNames`）。
fn always_names() -> String {
    ALWAYS_PASSED
        .iter()
        .copied()
        .filter(|n| !n.starts_with("AMONHEN_"))
        .chain(["GIT_*"])
        .collect::<Vec<_>>()
        .join(", ")
}

/// 余白をとったページ（表が残りの高さを使う）。
fn padded(child: &gtk::Box) -> gtk::Box {
    child.set_margin_top(18);
    child.set_margin_bottom(18);
    child.set_margin_start(20);
    child.set_margin_end(20);
    child.set_vexpand(true);
    child.clone()
}

/// Environment Variables のページ（SSH 先なら、そのエンジンの環境）。
pub fn page(s: &Rc<RepoSettings>) -> gtk::Widget {
    install_css();
    if s.host.is_some() {
        return host_page(s).upcast();
    }
    let table = Table::new(true);
    let mut vars: Vec<(String, String)> = std::env::vars().collect();
    vars.sort();
    table.vars.replace(vars);

    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let switch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    switch.add_css_class("linked");
    let passed = gtk::ToggleButton::with_label(&tr!("Passed"));
    let all = gtk::ToggleButton::with_label(&tr!("All"));
    all.set_group(Some(&passed));
    all.set_active(true);
    switch.append(&passed);
    switch.append(&all);
    let w = Rc::downgrade(&table);
    all.connect_toggled(move |a| {
        if let Some(t) = w.upgrade() {
            t.show_all.set(a.is_active());
            t.rebuild();
        }
    });
    top.append(&switch);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    top.append(&spacer);
    top.append(&table.search());
    // SSH の agent のソケットがあるか（SSH_AUTH_SOCK があるときだけ）。macOS 版と同じく表の上に、線で分けて。
    if let Some(socket) = std::env::var_os("SSH_AUTH_SOCK") {
        #[cfg(unix)]
        let found = {
            use std::os::unix::fs::FileTypeExt;
            std::fs::metadata(&socket).is_ok_and(|m| m.file_type().is_socket())
        };
        // Windows の agent は名前付きパイプ。
        #[cfg(not(unix))]
        let found = std::path::Path::new(&socket).exists();
        let agent = if found {
            label(&tr!("SSH agent found"), &["dim-label", "caption"])
        } else {
            label(
                &tr!("No SSH agent here, so keys that need a passphrase can’t be used"),
                &["settings-warning", "caption"],
            )
        };
        agent.set_wrap(true);
        b.append(&agent);
        b.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    b.append(&top);
    b.append(&table.view());
    b.append(&table.detail);
    let note = label(
        &tr!(
            "Checked variables go to the git and ssh this app runs, through the shared daemon too, but not to SSH hosts. Always passed, when the shell sets them: {names}.",
            names = always_names()
        ),
        &["dim-label", "caption"],
    );
    note.set_wrap(true);
    note.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    b.append(&note);
    // この機械で選んだ git（PATH のものでなければ、PATH の行にそれを書く）。
    let (repo, w) = (s.repo.clone(), Rc::downgrade(&table));
    glib::spawn_future_local(async move {
        let info = run_one(&repo, json!({"op": "machineGit"}))
            .await
            .unwrap_or_default();
        let git = crate::programs::MachineGit::from_info(&info);
        if let Some(t) = w.upgrade() {
            t.chosen_git
                .replace((git.source != "path" && !git.source.is_empty()).then_some(git.program));
            t.rebuild();
        }
    });
    table.settings.replace(Rc::downgrade(s));
    let w = Rc::downgrade(&table);
    crate::settings::connect_changed_while(&b, move |key| {
        if key == ADDED
            && let Some(t) = w.upgrade()
        {
            t.added_changed();
        }
    });
    table.rebuild();
    // ページが表を持っておく。
    unsafe { b.set_data("table", table) };
    padded(&b).upcast()
}

/// SSH 先の「環境変数」: エンジンが実際に持っている環境（そのまま git に渡る）。
fn host_page(s: &Rc<RepoSettings>) -> gtk::Box {
    let host = s.host.clone().unwrap_or_default();
    let table = Table::new(false);
    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let caption = label(
        &tr!(
            "The engine’s environment, which git on {host} gets",
            host = host
        ),
        &["dim-label", "caption"],
    );
    caption.set_hexpand(true);
    caption.set_wrap(true);
    top.append(&caption);
    top.append(&table.search());
    b.append(&top);
    let view = table.view();
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&view));
    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk::Align::Center);
    spinner.set_valign(gtk::Align::Center);
    overlay.add_overlay(&spinner);
    overlay.set_vexpand(true);
    b.append(&overlay);
    b.append(&table.detail);

    // エンジンの環境と、PATH で見つかる git の場所、SSH 先の設定で選んだ git を SSH 先から読む。
    let (repo, w) = (s.repo.clone(), Rc::downgrade(&table));
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || {
            let call = |c| crate::reposettings::call(&repo, &c).unwrap_or_default();
            let vars: Vec<(String, String)> = call(json!({"op": "processEnvironment"}))["vars"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| {
                            Some((
                                v["name"].as_str()?.to_owned(),
                                v["value"].as_str()?.to_owned(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let found =
                crate::programs::candidates(&call(json!({"op": "programs", "name": "git"})));
            let git = crate::programs::MachineGit::from_info(&call(json!({"op": "machineGit"})));
            (vars, found, git)
        })
        .await;
        spinner.set_visible(false);
        let (Ok((vars, found, git)), Some(t)) = (result, w.upgrade()) else {
            return;
        };
        t.windows
            .set(vars.iter().any(|(n, v)| n == "OS" && v == "Windows_NT"));
        let windows = t.windows.get();
        t.git_directory.replace(found.first().map(|c| {
            let i = c.path.rfind(if windows { '\\' } else { '/' }).unwrap_or(0);
            c.path[..i].to_owned()
        }));
        t.chosen_git
            .replace((git.source != "path" && !git.source.is_empty()).then_some(git.program));
        t.vars.replace(vars);
        t.rebuild();
    });
    table.rebuild();
    unsafe { b.set_data("table", table) };
    padded(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names_are_masked() {
        for name in [
            "GITHUB_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "API_KEY",
            "APIKEY",
            "MY_PASS",
            "NPM_AUTH",
            "DB_PASSWORD",
        ] {
            assert!(is_secret(name), "{name}");
        }
        for name in [
            "SSH_AUTH_SOCK",
            "AMONHEN_PASS_ENV",
            "GIT_ASKPASS",
            "AUTHOR",
            "KEYMAP",
            "GPG_KEYID",
            "PATH",
        ] {
            assert!(!is_secret(name), "{name}");
        }
        assert!(matches("GITHUB_TOKEN", "abc", "github"));
        assert!(!matches("GITHUB_TOKEN", "abc", "abc"));
        assert!(matches("HOME", "/home/me", "me"));
    }

    #[test]
    fn rows_say_whether_they_are_passed() {
        let added = vec!["HTTPS_PROXY".to_owned()];
        assert_eq!(Kind::of("PATH", &added), Kind::Always);
        assert_eq!(Kind::of("GIT_TRACE", &added), Kind::Always);
        assert_eq!(Kind::of("HTTPS_PROXY", &added), Kind::Chosen);
        assert_eq!(Kind::of("LANG", &added), Kind::Other);
        assert!(matches!(Kind::of("GIT_DIR", &added), Kind::Refused(_)));
        assert_eq!(
            places("/a::/b:/a", false),
            vec!["/a".to_owned(), "/b".into(), "/a".into()]
        );
    }
}
