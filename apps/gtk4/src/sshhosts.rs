//! Settings > SSH Hosts（macOS 版の `SSHHostsSettings.swift`・`SSHHosts.swift`。docs/ux.md の 3 章「SSH Hosts」）:
//! SSH 先ごとの設定。3 列: 左に SSH 先の一覧（＋／−）、真ん中に選んだ SSH 先のページ（接続と、そこにあるすべての
//! リポジトリに効く Settings > Git と同じページ）、右にその中身。git のページは、SSH 先でリポジトリなしに動かした
//! エンジン（`ssh://host`）を通して SSH 先の `--global` を読み書きする。
//!
//! SSH 先ごとの選択（エンジン・送り込むか・ログインシェル。コアの `HostOptions`）と、署名をどちらの鍵で検証するかは
//! アプリの設定に置き、リポジトリを開くとき（`main.rs` の `open_remote`）に使う。開くのは裏の糸なので、設定の写しを
//! 糸をまたいで読める形でも持つ。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

use amonhen_core::Repository;
use amonhen_model::tr;
use amonhen_remote::ssh::HostOptions;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::reposettings::{
    Level, Picker, RepoSettings, label, page, page_title, pages, run_one, section,
};
use crate::sheets::{Response, Sheet};

/// ＋ で加えた SSH 先（JSON の配列。macOS 版と同じ鍵）。
const SSH_HOSTS: &str = "sshHosts";
/// SSH 先ごとの選択（SSH 先 → `HostOptions` の JSON の、JSON の対応）。
const HOST_OPTIONS: &str = "sshHostOptions";
/// SSH 先ごとの、署名を手元の鍵で検証するか（SSH 先 → bool の JSON）。
pub const SIGNATURE_BY_HOST: &str = "checkRemoteSignaturesLocallyByHost";
/// 「その他…」の項目の値。
const OTHER: &str = "\u{1}other";

/// 裏の糸から読む設定の写し（リポジトリを開くとき）。
#[derive(Default)]
struct Mirror {
    options: HashMap<String, HostOptions>,
    local_signatures: HashMap<String, bool>,
}

static MIRROR: Mutex<Option<Mirror>> = Mutex::new(None);

/// 設定の JSON の対応（SSH 先 → 値）。読めなければ空。
fn string_map(key: &str) -> HashMap<String, String> {
    crate::settings::string(key)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn bool_map(key: &str) -> HashMap<String, bool> {
    crate::settings::string(key)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 設定から写しを作る（起動の初めと、変えたとき）。
pub fn load() {
    let options = string_map(HOST_OPTIONS)
        .into_iter()
        .map(|(host, json)| (host, HostOptions::from_json(&json)))
        .collect();
    *MIRROR.lock().unwrap() = Some(Mirror {
        options,
        local_signatures: bool_map(SIGNATURE_BY_HOST),
    });
}

/// 場所の SSH 先（`ssh://host/path`・`daemon:ssh://host/path` の host。`user@`・`:port` も含めて ssh に渡す形）。
/// 手元なら None。
pub fn host_of(location: &str) -> Option<String> {
    let rest = location.strip_prefix("daemon:").unwrap_or(location);
    let rest = rest.strip_prefix("ssh://")?;
    let host = rest.split('/').next().unwrap_or_default();
    (!host.is_empty()).then(|| host.to_owned())
}

/// `location` の SSH 先ごとの選択（手元・選んでいなければ既定）。どの糸からも読める。
pub fn options_for(location: &str) -> HostOptions {
    let Some(host) = host_of(location) else {
        return HostOptions::default();
    };
    MIRROR
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.options.get(&host).cloned())
        .unwrap_or_default()
}

/// `location`（SSH 先のリポジトリ）の署名を手元の鍵で検証するか（既定は手元）。どの糸からも読める。
pub fn checks_signatures_locally(location: &str) -> bool {
    let Some(host) = host_of(location) else {
        return true;
    };
    MIRROR
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.local_signatures.get(&host).copied())
        .unwrap_or(true)
}

/// daemon 越しのリポジトリの署名を検証する側を決める（macOS 版の `amonhen_repo_set_local_signature_check`）。手元の
/// 鍵なら、コミットのオブジェクトだけを書く作業用のリポジトリで検証する。手元のリポジトリには効かない。
pub fn apply_signature_check(repo: &Repository, location: &str) {
    static VERIFIER: std::sync::OnceLock<Arc<amonhen_backend_cli::LocalVerifier>> =
        std::sync::OnceLock::new();
    if repo.connection_state().is_none() {
        return;
    }
    let verifier = checks_signatures_locally(location).then(|| {
        VERIFIER
            .get_or_init(|| {
                Arc::new(amonhen_backend_cli::LocalVerifier::new(
                    std::env::temp_dir().join("amonhen-verify.git"),
                ))
            })
            .clone() as Arc<dyn amonhen_model::SignatureVerifier>
    });
    repo.set_signature_verifier(verifier);
}

fn host_options(host: &str) -> HostOptions {
    string_map(HOST_OPTIONS)
        .get(host)
        .map(|j| HostOptions::from_json(j))
        .unwrap_or_default()
}

/// SSH 先ごとの選択を変える（次につなぐときから効く）。
fn set_host_options(host: &str, options: &HostOptions) {
    let mut all = string_map(HOST_OPTIONS);
    if options.is_empty() {
        all.remove(host);
    } else {
        all.insert(host.to_owned(), options.to_json());
    }
    crate::settings::set_string(HOST_OPTIONS, &json!(all).to_string());
    load();
}

fn checks_locally(host: &str) -> bool {
    bool_map(SIGNATURE_BY_HOST)
        .get(host)
        .copied()
        .unwrap_or(true)
}

fn set_checks_locally(host: &str, local: bool) {
    let mut all = bool_map(SIGNATURE_BY_HOST);
    all.insert(host.to_owned(), local);
    crate::settings::set_string(SIGNATURE_BY_HOST, &json!(all).to_string());
    load();
}

fn added_hosts() -> Vec<String> {
    crate::settings::string(SSH_HOSTS)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn set_added_hosts(hosts: &[String]) {
    crate::settings::set_string(SSH_HOSTS, &json!(hosts).to_string());
}

/// Amon Hen が知っている SSH 先: Places にあるもの（リポジトリかスキャンするフォルダー）と、＋ で加えたもの
/// （macOS 版と同じ）。
fn known_hosts() -> Vec<String> {
    let mut hosts = added_hosts();
    hosts.extend(
        crate::bookmarks::all_locations()
            .iter()
            .filter_map(|l| host_of(l)),
    );
    hosts.extend(
        crate::bookmarks::data()
            .scan_roots
            .iter()
            .filter_map(|r| host_of(&r.path)),
    );
    hosts.sort_by_key(|h| h.to_lowercase());
    hosts.dedup();
    hosts
}

/// Places のリポジトリとスキャンするフォルダーのうち、`host` のもの。
fn places_on(host: &str) -> (Vec<String>, Vec<String>) {
    let paths = crate::bookmarks::all_locations()
        .into_iter()
        .filter(|l| host_of(l).as_deref() == Some(host))
        .collect();
    let roots = crate::bookmarks::data()
        .scan_roots
        .into_iter()
        .map(|r| r.path)
        .filter(|p| host_of(p).as_deref() == Some(host))
        .collect();
    (paths, roots)
}

/// SSH の接続先の候補: `~/.ssh/config` の `Host`（`*`・`?` のない名前）と `~/.ssh/known_hosts` のホスト（ハッシュに
/// したものは読めないので出ない。ポートがあれば `host:port`）。名前を先に、IP アドレスは後に。
pub fn candidates() -> Vec<String> {
    let ssh = glib::home_dir().join(".ssh");
    let config = std::fs::read_to_string(ssh.join("config")).unwrap_or_default();
    let known = std::fs::read_to_string(ssh.join("known_hosts")).unwrap_or_default();
    parse_candidates(&config, &known)
}

fn parse_candidates(config: &str, known: &str) -> Vec<String> {
    let wild = |s: &str| s.contains(['*', '?', '!']);
    let mut names: Vec<String> = Vec::new();
    for line in config.lines() {
        let words: Vec<&str> = line
            .split([' ', '\t', '='])
            .filter(|w| !w.is_empty())
            .collect();
        if words.len() > 1 && words[0].eq_ignore_ascii_case("host") {
            names.extend(
                words[1..]
                    .iter()
                    .filter(|w| !wild(w))
                    .map(|w| (*w).to_owned()),
            );
        }
    }
    for line in known.lines() {
        let mut fields = line.split(' ').filter(|f| !f.is_empty());
        let mut first = fields.next();
        // @cert-authority・@revoked
        if first.is_some_and(|f| f.starts_with('@')) {
            first = fields.next();
        }
        let Some(hosts) = first.filter(|h| !h.starts_with('#') && !h.starts_with('|')) else {
            continue;
        };
        for host in hosts.split(',') {
            // [host]:port
            if let Some(rest) = host.strip_prefix('[')
                && let Some((name, port)) = rest.split_once(']')
            {
                let port = port.trim_start_matches(':');
                names.push(if port.is_empty() || port == "22" {
                    name.to_owned()
                } else {
                    format!("{name}:{port}")
                });
            } else if !wild(host) {
                names.push(host.to_owned());
            }
        }
    }
    let is_address = |s: &str| {
        (s.starts_with(|c: char| c.is_ascii_digit())
            && s.chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == ':'))
            || s.contains("::")
    };
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.clone()));
    names.sort_by_key(|n| (is_address(n), n.to_lowercase()));
    names
}

/// 1 つの SSH 先へのつながり: リポジトリなしで動かしたエンジン（`ssh://host`、共有の daemon 越しなら
/// `daemon:ssh://host`）と、それを通してその SSH 先の `--global` を読み書きする設定の部品。
enum State {
    Connecting,
    Connected(Rc<RepoSettings>),
    Failed(String),
}

/// つながりの知らせ（状態が変わったら true、設定を読み直したら false）。
type OnChange = Rc<dyn Fn(bool)>;

struct HostSession {
    host: String,
    state: RefCell<State>,
    /// SSH 先で使っているエンジンと、その用意のしかた（つながったとき。共有の daemon 越しなら None）。
    engine: RefCell<Option<String>>,
    /// 共有の daemon 越しにつないだか。
    through_daemon: Cell<bool>,
    /// Windows の SSH 先か（エンジンの環境変数の `OS` で見る。ログインシェルがない）。
    windows: Cell<bool>,
    /// 何回目のつなぎか（古いつなぎの結果は捨てる）。
    attempt: Cell<u64>,
    /// 状態が変わったら（true）、読み直したら（false）呼ぶ。
    on_change: RefCell<Option<OnChange>>,
}

impl HostSession {
    fn new(host: &str) -> Rc<Self> {
        Rc::new(Self {
            host: host.to_owned(),
            state: RefCell::new(State::Connecting),
            engine: RefCell::default(),
            through_daemon: Cell::new(false),
            windows: Cell::new(false),
            attempt: Cell::new(0),
            on_change: RefCell::default(),
        })
    }

    fn changed(&self, state: bool) {
        let f = self.on_change.borrow().clone();
        if let Some(f) = f {
            f(state);
        }
    }

    fn settings(&self) -> Option<Rc<RepoSettings>> {
        match &*self.state.borrow() {
            State::Connected(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn failed(&self) -> bool {
        matches!(*self.state.borrow(), State::Failed(_))
    }

    /// つなぐ（つなぎ直す）。SSH でつないでエンジンを用意するので数秒かかる。
    fn connect(self: &Rc<Self>) {
        let attempt = self.attempt.get() + 1;
        self.attempt.set(attempt);
        self.state.replace(State::Connecting);
        self.engine.replace(None);
        let through = crate::settings::bool(crate::settings::OPEN_THROUGH_DAEMON);
        self.through_daemon.set(through);
        self.changed(true);
        let location = format!(
            "{}ssh://{}",
            if through { "daemon:" } else { "" },
            self.host
        );
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let repo = crate::open(std::path::Path::new(&location))?;
                let engine = repo.connection_state().and_then(|c| c.engine);
                let windows =
                    crate::reposettings::call(&repo, &json!({"op": "processEnvironment"}))
                        .ok()
                        .and_then(|info| {
                            info["vars"].as_array().map(|vars| {
                                vars.iter()
                                    .any(|v| v["name"] == "OS" && v["value"] == "Windows_NT")
                            })
                        })
                        .unwrap_or(false);
                Ok::<_, String>((repo, engine, windows))
            })
            .await
            .unwrap_or_else(|_| Err("panic".into()));
            let Some(s) = w.upgrade() else { return };
            if s.attempt.get() != attempt {
                return;
            }
            match result {
                Ok((repo, engine, windows)) => {
                    s.engine.replace(engine);
                    s.windows.set(windows);
                    let settings = RepoSettings::new(
                        Arc::new(repo),
                        &format!("ssh://{}", s.host),
                        Level::AllRepositories,
                    );
                    // ページの一覧は真ん中の列に出すので、部品の左の一覧と区切りは隠す。
                    if let Some(left) = settings.widget.first_child() {
                        left.set_visible(false);
                        if let Some(sep) = left.next_sibling() {
                            sep.set_visible(false);
                        }
                    }
                    let w = Rc::downgrade(&s);
                    settings.on_refresh(move |_| {
                        if let Some(s) = w.upgrade() {
                            s.changed(false);
                        }
                    });
                    // 確認用の手順（`--settings ssh-hosts/<host>/<page>` と一緒に。初めてつながったときに 1 回）。
                    thread_local! {
                        static SCRIPT_RUN: Cell<bool> = const { Cell::new(false) };
                    }
                    if let Ok(script) = std::env::var("AMONHEN_SETTINGS_SCRIPT")
                        && !SCRIPT_RUN.with(|r| r.replace(true))
                    {
                        settings.run_script(&script);
                    }
                    s.state.replace(State::Connected(settings));
                }
                Err(e) => {
                    s.state.replace(State::Failed(e));
                }
            }
            s.changed(true);
        });
    }
}

/// Settings > SSH Hosts のタブ。
pub struct SshHosts {
    pub widget: gtk::Box,
    hosts: RefCell<Vec<String>>,
    selected: RefCell<Option<String>>,
    /// 選んだページ（`connection` か Settings > Git のページの名前）。
    page: RefCell<String>,
    sessions: RefCell<HashMap<String, Rc<HostSession>>>,
    host_list: gtk::ListBox,
    page_list: gtk::ListBox,
    footer: gtk::Label,
    content: gtk::Box,
    remove: gtk::Button,
    /// 一覧を作り直している間（選択の知らせを無視する）。
    filling: Cell<bool>,
}

thread_local! {
    /// 開いたら選ぶ SSH 先とページ（`--settings ssh-hosts/<host>/<page>`）。
    static PENDING: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    /// 作ってあるタブ（出している間に選ばれたら、すぐ選ぶ）。
    static CURRENT: RefCell<std::rc::Weak<SshHosts>> = RefCell::default();
}

/// 選ぶ SSH 先とページ（なければ加える）。タブがもうあればすぐ、なければ次に出すときに選ぶ。
pub fn select_pending(host: &str, page: &str) {
    PENDING.with(|p| p.replace(Some((host.to_owned(), page.to_owned()))));
    if let Some(current) = CURRENT.with(|c| c.borrow().upgrade()) {
        current.reload();
    }
}

/// 出してあるタブの SSH 先の一覧を読み直す（Settings の窓を出し直したとき。macOS 版と同じ）。
pub fn reload_shown() {
    if let Some(current) = CURRENT.with(|c| c.borrow().upgrade()) {
        current.reload();
    }
}

impl SshHosts {
    pub fn new() -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        widget.set_size_request(960, 560);

        // 左: SSH 先。
        let host_list = gtk::ListBox::new();
        host_list.add_css_class("navigation-sidebar");
        let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left.set_size_request(160, -1);
        left.append(
            &gtk::ScrolledWindow::builder()
                .child(&host_list)
                .vexpand(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .build(),
        );
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        buttons.set_margin_start(8);
        buttons.set_margin_end(8);
        buttons.set_margin_top(6);
        buttons.set_margin_bottom(6);
        let add = gtk::Button::with_label("+");
        add.add_css_class("flat");
        add.set_tooltip_text(Some(&tr!("Add an SSH host")));
        let remove = gtk::Button::with_label("−");
        remove.add_css_class("flat");
        remove.set_tooltip_text(Some(&tr!("Forget this host")));
        buttons.append(&add);
        buttons.append(&remove);
        left.append(&buttons);
        widget.append(&left);
        widget.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        // 真ん中: ページ。
        let page_list = gtk::ListBox::new();
        page_list.add_css_class("navigation-sidebar");
        for name in std::iter::once("connection").chain(pages(Level::AllRepositories)) {
            let title = if name == "connection" {
                tr!("Connection")
            } else {
                page_title(name)
            };
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(name);
            let l = label(&title, &[]);
            l.set_margin_start(6);
            l.set_margin_top(4);
            l.set_margin_bottom(4);
            row.set_child(Some(&l));
            page_list.append(&row);
        }
        let middle = gtk::Box::new(gtk::Orientation::Vertical, 0);
        middle.set_size_request(180, -1);
        middle.append(
            &gtk::ScrolledWindow::builder()
                .child(&page_list)
                .vexpand(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .build(),
        );
        let footer = label("", &["dim-label", "caption"]);
        footer.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        footer.set_margin_start(14);
        footer.set_margin_end(14);
        footer.set_margin_top(8);
        footer.set_margin_bottom(8);
        middle.append(&footer);
        widget.append(&middle);
        widget.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        // 右: 中身。
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_hexpand(true);
        content.set_vexpand(true);
        widget.append(&content);

        let this = Rc::new(Self {
            widget,
            hosts: RefCell::default(),
            selected: RefCell::default(),
            page: RefCell::new("connection".into()),
            sessions: RefCell::default(),
            host_list,
            page_list,
            footer,
            content,
            remove,
            filling: Cell::new(false),
        });
        let w = Rc::downgrade(&this);
        this.host_list.connect_row_selected(move |_, row| {
            let Some(s) = w.upgrade() else { return };
            if s.filling.get() {
                return;
            }
            let host = row.map(|r| r.widget_name().to_string());
            s.selected.replace(host);
            s.select_changed();
        });
        let w = Rc::downgrade(&this);
        this.page_list.connect_row_selected(move |_, row| {
            let Some(s) = w.upgrade() else { return };
            if s.filling.get() {
                return;
            }
            if let Some(row) = row {
                s.page.replace(row.widget_name().to_string());
                s.render();
            }
        });
        let w = Rc::downgrade(&this);
        add.connect_clicked(move |_| {
            if let Some(s) = w.upgrade() {
                s.add_host();
            }
        });
        let w = Rc::downgrade(&this);
        this.remove.connect_clicked(move |_| {
            if let Some(s) = w.upgrade() {
                s.remove_selected();
            }
        });
        CURRENT.with(|c| c.replace(Rc::downgrade(&this)));
        this.reload();
        this
    }

    /// 一覧を読み直す。選ぶものがなければ先頭。
    fn reload(self: &Rc<Self>) {
        if let Some((host, page)) = PENDING.with(|p| p.take()) {
            let mut added = added_hosts();
            if !known_hosts().contains(&host) {
                added.push(host.clone());
                set_added_hosts(&added);
            }
            self.selected.replace(Some(host));
            self.page.replace(if page.is_empty() {
                "connection".into()
            } else {
                page
            });
        }
        let hosts = known_hosts();
        let selected = self.selected.borrow().clone();
        if selected.as_ref().is_none_or(|h| !hosts.contains(h)) {
            self.selected.replace(hosts.first().cloned());
        }
        self.hosts.replace(hosts);
        self.fill_hosts();
        self.select_changed();
    }

    /// 左の一覧を作り直す（つながらない SSH 先は名前の下に小さく）。
    fn fill_hosts(&self) {
        self.filling.set(true);
        while let Some(row) = self.host_list.first_child() {
            self.host_list.remove(&row);
        }
        let selected = self.selected.borrow().clone();
        for host in self.hosts.borrow().iter() {
            let unreachable = self.sessions.borrow().get(host).is_some_and(|s| s.failed());
            let b = gtk::Box::new(gtk::Orientation::Vertical, 1);
            b.set_margin_start(6);
            b.set_margin_top(3);
            b.set_margin_bottom(3);
            let name = label(host, &[]);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            b.append(&name);
            if unreachable {
                name.add_css_class("dim-label");
                b.append(&label(&tr!("unreachable"), &["dim-label", "caption"]));
            }
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(host);
            row.set_child(Some(&b));
            self.host_list.append(&row);
            if selected.as_deref() == Some(host.as_str()) {
                self.host_list.select_row(Some(&row));
            }
        }
        self.filling.set(false);
    }

    /// 選んだ SSH 先が変わった（初めて選んだならつなぐ）。
    fn select_changed(self: &Rc<Self>) {
        let host = self.selected.borrow().clone();
        self.remove.set_sensitive(host.is_some());
        self.page_list.set_sensitive(host.is_some());
        if let Some(host) = &host {
            self.session(host);
        }
        self.filling.set(true);
        let page = self.page.borrow().clone();
        let mut child = self.page_list.first_child();
        while let Some(c) = child {
            if let Some(row) = c.downcast_ref::<gtk::ListBoxRow>()
                && row.widget_name() == page
            {
                self.page_list.select_row(Some(row));
            }
            child = c.next_sibling();
        }
        self.filling.set(false);
        self.render();
    }

    /// その SSH 先のつながり（なければつなぐ）。
    fn session(self: &Rc<Self>, host: &str) -> Rc<HostSession> {
        if let Some(s) = self.sessions.borrow().get(host) {
            return s.clone();
        }
        let session = HostSession::new(host);
        self.sessions
            .borrow_mut()
            .insert(host.to_owned(), session.clone());
        // 初めのつなぎの「つないでいます」は、呼んだ側が出す（出している途中に出し直さない）。
        session.connect();
        let w: Weak<Self> = Rc::downgrade(self);
        let h = host.to_owned();
        session.on_change.replace(Some(Rc::new(move |state| {
            if let Some(s) = w.upgrade() {
                s.session_changed(&h, state);
            }
        })));
        session
    }

    /// つながりの状態が変わった（一覧の「つながらない」と、選んでいれば中身）か、設定を読み直した（下の git の版と、
    /// 接続のページ。git のページは部品が自分で出し直す）。
    fn session_changed(self: &Rc<Self>, host: &str, state: bool) {
        if state {
            self.fill_hosts();
        }
        if self.selected.borrow().as_deref() == Some(host)
            && (state || *self.page.borrow() == "connection")
        {
            self.render();
        } else if self.selected.borrow().as_deref() == Some(host) {
            self.update_footer(host);
        }
    }

    /// 真ん中の列の下: この SSH 先の git。
    fn update_footer(&self, host: &str) {
        let version = self
            .sessions
            .borrow()
            .get(host)
            .and_then(|s| s.settings())
            .and_then(|s| s.state.borrow().git_version.clone())
            .unwrap_or_else(|| "?".into());
        self.footer.set_text(&format!("git {version} · {host}"));
    }

    /// 選んだ SSH 先の、選んだページを出す。
    fn render(self: &Rc<Self>) {
        while let Some(c) = self.content.first_child() {
            self.content.remove(&c);
        }
        let Some(host) = self.selected.borrow().clone() else {
            self.footer.set_text("");
            let l = label(
                &tr!("Add an SSH host with +, or open one from Places."),
                &["dim-label"],
            );
            l.set_margin_top(20);
            l.set_margin_start(20);
            self.content.append(&l);
            return;
        };
        let session = self.session(&host);
        self.update_footer(&host);
        let page = self.page.borrow().clone();
        if page == "connection" {
            self.content.append(&self.connection_page(&session));
            return;
        }
        let state = session.state.borrow();
        match &*state {
            State::Connected(settings) => {
                // 環境変数は ~/.gitconfig の設定ではない（ページの中で、何の環境かを書く）。
                if page == "environment" {
                    if !session.windows.get() {
                        self.content.append(&login_shell_rows(&session));
                    }
                } else {
                    let l = label(
                        &tr!(
                            "Applies to every repository on {host} (its ~/.gitconfig). git in a terminal there follows these too.",
                            host = host
                        ),
                        &["dim-label", "caption"],
                    );
                    l.set_wrap(true);
                    l.set_margin_start(20);
                    l.set_margin_end(20);
                    l.set_margin_top(10);
                    self.content.append(&l);
                }
                if let Some(parent) = settings.widget.parent()
                    && let Some(parent) = parent.downcast_ref::<gtk::Box>()
                {
                    parent.remove(&settings.widget);
                }
                settings.widget.set_vexpand(true);
                settings.select(&page);
                self.content.append(&settings.widget);
            }
            State::Connecting => {
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                b.set_margin_top(20);
                b.set_margin_start(20);
                let spinner = gtk::Spinner::new();
                spinner.set_spinning(true);
                b.append(&spinner);
                b.append(&label(&tr!("Connecting to {host}…", host = host), &[]));
                self.content.append(&b);
            }
            State::Failed(reason) => {
                let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
                b.set_margin_top(20);
                b.set_margin_start(20);
                b.set_margin_end(20);
                b.append(&label(&tr!("Couldn’t connect to {host}", host = host), &[]));
                let r = label(reason, &["settings-error"]);
                r.set_wrap(true);
                r.set_selectable(true);
                b.append(&r);
                let again = gtk::Button::with_label(&tr!("Check Connection"));
                again.set_halign(gtk::Align::Start);
                let w = Rc::downgrade(&session);
                again.connect_clicked(move |_| {
                    if let Some(s) = w.upgrade() {
                        s.connect();
                    }
                });
                b.append(&again);
                self.content.append(&b);
            }
        }
    }

    /// 「接続」のページ: SSH 先と状態、使っているエンジン、署名の検証に使う鍵。
    fn connection_page(&self, session: &Rc<HostSession>) -> gtk::ScrolledWindow {
        let host = session.host.clone();
        // 状態と Check Connection。
        let state = gtk::Box::new(gtk::Orientation::Vertical, 2);
        state.set_hexpand(true);
        let connecting = matches!(*session.state.borrow(), State::Connecting);
        match &*session.state.borrow() {
            State::Connecting => {
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let spinner = gtk::Spinner::new();
                spinner.set_spinning(true);
                b.append(&spinner);
                b.append(&label(&tr!("Connecting…"), &[]));
                state.append(&b);
            }
            State::Connected(settings) => {
                state.append(&label(&format!("● {}", tr!("Connected")), &["settings-ok"]));
                let version = settings.state.borrow().git_version.clone();
                state.append(&label(
                    &version.map_or_else(
                        || tr!("git wasn’t found on the host"),
                        |v| format!("git {v}"),
                    ),
                    &["dim-label", "caption"],
                ));
                if settings.state.borrow().loaded && settings.state.borrow().git_version.is_none() {
                    state.append(&chosen_git_warning(settings));
                }
            }
            State::Failed(reason) => {
                state.append(&label(
                    &format!("● {}", tr!("unreachable")),
                    &["settings-error"],
                ));
                let r = label(reason, &["dim-label", "caption"]);
                r.set_wrap(true);
                r.set_selectable(true);
                state.append(&r);
            }
        }
        let check = gtk::Button::with_label(&tr!("Check Connection"));
        check.set_valign(gtk::Align::Center);
        check.set_sensitive(!connecting);
        let w = Rc::downgrade(session);
        check.connect_clicked(move |_| {
            if let Some(s) = w.upgrade() {
                s.connect();
            }
        });
        let state_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        state_row.append(&state);
        state_row.append(&check);

        // 使っているエンジン。
        let in_use = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let engine = session.engine.borrow().clone();
        let connected = session.settings().is_some();
        let used = label(
            &engine.clone().unwrap_or_else(|| {
                if connected {
                    tr!("Through the shared daemon")
                } else {
                    tr!("Not connected")
                }
            }),
            &["dim-label"],
        );
        used.set_selectable(true);
        used.set_wrap(true);
        used.set_xalign(1.0);
        in_use.append(&used);
        // daemon 越しなら、エンジンは daemon が起動したもの（同じ SSH 先を開いたほかの相手と共有する）。
        if engine.is_some() && session.through_daemon.get() {
            let l = label(
                &tr!("Started by the shared daemon"),
                &["dim-label", "caption"],
            );
            l.set_xalign(1.0);
            in_use.append(&l);
        }
        let mut engine_rows = engine_choice(session);
        engine_rows.push(labeled(&tr!("In use"), &in_use));
        // 環境変数で決めてあれば、そちらが優先される。
        if let Some(name) = ["AMONHEN_REMOTE_ENGINE", "AMONHEN_PUSH"]
            .into_iter()
            .find(|n| std::env::var(n).is_ok_and(|v| !v.is_empty()))
        {
            engine_rows.push(note(&tr!(
                "{name} is set in the environment, so it decides instead.",
                name = name
            )));
        }

        // 署名の検証。
        let h = host.clone();
        let verify = Picker::new(move |value| {
            let local = value == "local";
            if local != checks_locally(&h) {
                set_checks_locally(&h, local);
            }
        });
        verify.set(
            &vec![(
                None,
                vec![
                    ("local".into(), tr!("This computer’s keys")),
                    ("host".into(), tr!("The host’s git and keys")),
                ],
            )],
            &[],
            if checks_locally(&host) {
                "local"
            } else {
                "host"
            },
        );

        let host_label = label(&host, &[]);
        host_label.set_selectable(true);
        page(&[
            section(
                Some(&tr!("Connection")),
                &[
                    labeled(&tr!("Host"), &host_label),
                    labeled(&tr!("State"), &state_row),
                ],
            ),
            section(Some(&tr!("Engine")), &engine_rows),
            section(
                Some(&tr!("Signature Verification")),
                &[
                    labeled(&tr!("Verify with"), &verify.button),
                    note(&tr!(
                        "Signatures of commits in repositories on this host are checked with these."
                    )),
                ],
            ),
            scan_folders(session),
        ])
    }

    /// 名前を聞いて SSH 先を加える（候補は ~/.ssh/config と known_hosts のもの）。
    fn add_host(self: &Rc<Self>) {
        let parent = self.widget.root().and_downcast::<gtk::Window>();
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let entry = gtk::Entry::builder()
                .activates_default(true)
                .hexpand(true)
                .build();
            let list = gtk::ListBox::new();
            let known = known_hosts();
            for c in candidates().into_iter().filter(|c| !known.contains(c)) {
                let row = gtk::ListBoxRow::new();
                let l = label(&c, &[]);
                l.set_margin_start(6);
                l.set_margin_top(2);
                l.set_margin_bottom(2);
                row.set_child(Some(&l));
                row.set_widget_name(&c);
                list.append(&row);
            }
            let e = entry.clone();
            list.connect_row_selected(move |_, row| {
                if let Some(row) = row {
                    e.set_text(&row.widget_name());
                }
            });
            let sheet = Sheet::new(
                parent.as_ref(),
                &tr!("Add an SSH host"),
                &tr!("Write it as ssh knows it: user@host:port, or a name in ~/.ssh/config."),
            );
            sheet.add(&entry);
            if list.first_child().is_some() {
                sheet.add(
                    &gtk::ScrolledWindow::builder()
                        .child(&list)
                        .min_content_height(160)
                        .max_content_height(160)
                        .hscrollbar_policy(gtk::PolicyType::Never)
                        .has_frame(true)
                        .build(),
                );
            }
            sheet.buttons(&tr!("Add"), false, None);
            if sheet.run().await != Response::Ok {
                return;
            }
            let host = entry.text().trim().to_owned();
            if host.is_empty() || host.contains(|c: char| c == '/' || c.is_whitespace()) {
                return;
            }
            let Some(s) = w.upgrade() else { return };
            let mut added = added_hosts();
            if !added.contains(&host) {
                added.push(host.clone());
                set_added_hosts(&added);
            }
            s.selected.replace(Some(host));
            s.page.replace("connection".into());
            s.reload();
        });
    }

    /// 選んだ SSH 先を忘れる。リポジトリやスキャンするフォルダーがあれば、Places から外すのと同じ確認をして同じ
    /// ように外す（SSH 先には触れない。macOS 版と同じ）。
    fn remove_selected(self: &Rc<Self>) {
        let Some(host) = self.selected.borrow().clone() else {
            return;
        };
        let parent = self.widget.root().and_downcast::<gtk::Window>();
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let (paths, roots) = places_on(&host);
            if !paths.is_empty() || !roots.is_empty() {
                let sheet = Sheet::new(
                    parent.as_ref(),
                    &tr!("Remove {host}?", host = host),
                    &tr!(
                        "This removes its {repositories} repositories and {folders} folders to scan from the list. Nothing on the host is touched.",
                        repositories = paths.len(),
                        folders = roots.len()
                    ),
                );
                sheet.buttons(&tr!("Delete"), true, None);
                if sheet.run().await != Response::Ok {
                    return;
                }
                for p in &paths {
                    crate::repos::forget(p);
                    crate::bookmarks::remove(p);
                }
                for r in &roots {
                    crate::bookmarks::remove_scan_root(r);
                }
            }
            let mut added = added_hosts();
            added.retain(|h| *h != host);
            set_added_hosts(&added);
            let Some(s) = w.upgrade() else { return };
            s.sessions.borrow_mut().remove(&host);
            s.selected.replace(None);
            s.reload();
        });
    }
}

/// 「探すフォルダ」: この SSH 先でスキャンするフォルダ（Repositories の Places の、この SSH 先の下と同じ設定）。
/// 深さを選べ、− で外し、＋ で SSH 先のフォルダを列で見て加える。
fn scan_folders(session: &Rc<HostSession>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    b.append(&label(&tr!("Folders to Scan"), &["heading"]));
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_show_separators(true);
    list.add_css_class("settings-list");
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&list));
    b.append(&frame);
    fill_scan_folders(&list, session);
    b
}

/// 探すフォルダの行を作り直す（最後の行は ＋ と説明）。
fn fill_scan_folders(list: &gtk::ListBox, session: &Rc<HostSession>) {
    list.remove_all();
    let host = session.host.clone();
    let roots: Vec<crate::bookmarks::ScanRoot> = crate::bookmarks::data()
        .scan_roots
        .into_iter()
        .filter(|r| crate::bookmarks::ssh_host(&r.path).as_deref() == Some(host.as_str()))
        .collect();
    let row = |child: &gtk::Box| {
        child.set_margin_top(4);
        child.set_margin_bottom(4);
        child.set_margin_start(4);
        child.set_margin_end(4);
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.set_child(Some(child));
        list.append(&row);
    };
    let titles: Vec<String> = (1..=6u32)
        .map(|d| tr!("{count} levels", count = d))
        .collect();
    for root in roots {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let path = label(
            &crate::repobrowser::remote_title(&crate::bookmarks::path_part(&root.path)),
            &["monospace"],
        );
        path.set_hexpand(true);
        path.set_selectable(true);
        line.append(&path);
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        let depth = gtk::DropDown::from_strings(&refs);
        depth.set_selected(root.depth.clamp(1, 6) - 1);
        let p = root.path.clone();
        depth.connect_selected_notify(move |d| {
            crate::bookmarks::set_depth(&p, d.selected() + 1);
        });
        line.append(&depth);
        let remove = gtk::Button::from_icon_name("list-remove-symbolic");
        remove.set_tooltip_text(Some(&tr!("Stop Scanning")));
        remove.add_css_class("flat");
        let (p, l, s) = (root.path.clone(), list.downgrade(), Rc::downgrade(session));
        remove.connect_clicked(move |_| {
            crate::bookmarks::remove_scan_root(&p);
            if let (Some(l), Some(s)) = (l.upgrade(), s.upgrade()) {
                fill_scan_folders(&l, &s);
            }
        });
        line.append(&remove);
        row(&line);
    }
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.set_tooltip_text(Some(&tr!("Add a Folder to Scan on {host}", host = host)));
    add.add_css_class("flat");
    let (l, s) = (list.downgrade(), Rc::downgrade(session));
    add.connect_clicked(move |button| {
        let (Some(l), Some(s)) = (l.upgrade(), s.upgrade()) else {
            return;
        };
        let Some(parent) = button.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let repo = s.settings().map(|settings| settings.repo.clone());
        glib::spawn_future_local(async move {
            if let Some(location) =
                crate::repobrowser::choose_remote_scan_folder(&parent, Some(s.host.clone()), repo)
                    .await
            {
                crate::bookmarks::add_scan_root(&location);
                fill_scan_folders(&l, &s);
            }
        });
    });
    line.append(&add);
    // 確認用: `AMONHEN_SCAN_ADD` なら、窓に出ている ＋ を一度だけ押す（つなぎ終えて作り直したものを）。
    thread_local! {
        static ADD_PRESSED: Cell<bool> = const { Cell::new(false) };
    }
    if std::env::var_os("AMONHEN_SCAN_ADD").is_some() && !ADD_PRESSED.with(Cell::get) {
        let a = add.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            if let Some(a) = a.upgrade().filter(|a| a.root().is_some())
                && !ADD_PRESSED.with(|p| p.replace(true))
            {
                a.emit_clicked();
            }
        });
    }
    let note = label(
        &tr!("The same list as under the host in Places."),
        &["dim-label", "caption"],
    );
    note.set_wrap(true);
    note.set_hexpand(true);
    note.set_xalign(0.0);
    line.append(&note);
    row(&line);
}

/// 「名前: 部品」の行（名前は左、部品は右）。
fn labeled(name: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    b.set_margin_top(4);
    b.set_margin_bottom(4);
    b.set_margin_start(4);
    b.set_margin_end(4);
    let l = label(name, &[]);
    let widget = widget.as_ref();
    // 何行かある部品（状態・使っているもの）なら名前は上に、1 行の欄なら真ん中に揃える。
    if widget.is::<gtk::Box>() || widget.is::<gtk::Label>() {
        l.set_valign(gtk::Align::Start);
        l.set_margin_top(4);
    } else {
        l.set_valign(gtk::Align::Center);
    }
    b.append(&l);
    widget.set_hexpand(true);
    // 箱（状態と Check Connection など）は幅いっぱいに、ほかは右に寄せる。
    if !widget.is::<gtk::Box>() {
        widget.set_halign(gtk::Align::End);
    }
    b.append(widget);
    b.upcast()
}

/// 説明の文（小さく、薄く）。
fn note(text: &str) -> gtk::Widget {
    let l = label(text, &["dim-label", "caption"]);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_margin_start(4);
    l.set_margin_end(4);
    l.upcast()
}

/// パスで指定したエンジンか（`/…`、Windows の `C:\…`・`\\server\…`）。
fn is_engine_path(value: &str) -> bool {
    let b = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

/// エンジンの選択: 自動（PATH のこのビルドのもの → 前に送ったもの → 送り込む）・SSH 先の PATH のもの・Amon Hen が
/// 送り込むもの・その他…（SSH 先のパス）と、合うものがなければ送り込むか。変えると、つなぎ直す。
fn engine_choice(session: &Rc<HostSession>) -> Vec<gtk::Widget> {
    let host = session.host.clone();
    let options = host_options(&host);
    let save = {
        let (host, w) = (host.clone(), Rc::downgrade(session));
        Rc::new(move |new: HostOptions| {
            if new != host_options(&host) {
                set_host_options(&host, &new);
                // 新しい選択で SSH 先のエンジンを起動し直す。
                if let Some(s) = w.upgrade() {
                    s.connect();
                }
            }
        })
    };
    let (h, sv, w) = (host.clone(), save.clone(), Rc::downgrade(session));
    // 選ぶ欄（つながっていないときに、パスを選ぶ窓を出す親を探す）。
    let anchor: Rc<RefCell<glib::WeakRef<gtk::Widget>>> = Rc::default();
    let a = anchor.clone();
    let picker = Picker::new(move |value| {
        let mut new = host_options(&h);
        if value != OTHER {
            new.engine = (!value.is_empty()).then_some(value);
            sv(new);
            return;
        }
        // その他…: SSH 先のパスを入れてもらう（つながっていれば、そのリポジトリの窓口で）。
        let current = new
            .engine
            .clone()
            .filter(|e| is_engine_path(e))
            .unwrap_or_else(|| "~/.local/share/amonhen".into());
        let sv = sv.clone();
        let Some(settings) = w.upgrade().and_then(|s| s.settings()) else {
            // つないでいる途中か、つながらなかった: 選ぶ窓が自分で SSH 先につなぐ（エンジンが動かない SSH 先も
            // パスを直せるように。macOS 版と同じ）。
            let request = crate::filepicker::Request {
                title: tr!("Engine on {host}", host = h),
                message: tr!("Choose amonhen on the host. It must be this build."),
                kind: crate::filepicker::Kind::Executable,
                initial: current,
                places: crate::filepicker::host_program_places(),
            };
            let parent = a.borrow().upgrade().and_then(|b| b.root()).and_downcast::<gtk::Window>();
            let source = crate::filepicker::Source::Host(h.clone());
            glib::spawn_future_local(async move {
                if let Some(chosen) =
                    crate::filepicker::choose(parent.as_ref(), request, source).await
                    && is_engine_path(&chosen.path)
                {
                    new.engine = Some(chosen.path);
                    sv(new);
                }
            });
            return;
        };
        settings.choose_path(
            crate::filepicker::Kind::Executable,
            &tr!("Engine on {host}", host = h),
            &tr!("Choose amonhen on the host. It must be this build."),
            current,
            move |path| {
                if is_engine_path(&path) {
                    new.engine = Some(path);
                    sv(new);
                }
            },
        );
    });
    let mut items = vec![
        (
            "installed".to_owned(),
            tr!("The amonhen on the host’s PATH"),
        ),
        ("sent".to_owned(), tr!("The one Amon Hen sends")),
    ];
    if let Some(path) = options.engine.clone().filter(|e| is_engine_path(e)) {
        items.push((path.clone(), path));
    }
    anchor.replace(picker.button.upcast_ref::<gtk::Widget>().downgrade());
    picker.set(
        &vec![
            (None, vec![(String::new(), tr!("Automatic"))]),
            (None, items),
        ],
        &[(OTHER.to_owned(), tr!("Other…"))],
        options.engine.as_deref().unwrap_or_default(),
    );
    let help = match options.engine.as_deref() {
        Some("installed") => tr!("Uses the amonhen installed on the host. It must be this build."),
        Some("sent") => {
            tr!("Uses the one Amon Hen sent to the host, sending it first when there is none.")
        }
        Some(p) if is_engine_path(p) => tr!("Uses this path on the host."),
        _ => tr!(
            "The amonhen on the host’s PATH if it is this build, else one sent earlier, else Amon Hen sends one."
        ),
    };
    let push = gtk::Switch::new();
    push.set_active(options.push.unwrap_or(true));
    push.set_sensitive(
        options.engine.as_deref() != Some("installed")
            && !options.engine.as_deref().is_some_and(is_engine_path),
    );
    let h = host.clone();
    push.connect_active_notify(move |sw| {
        let mut new = host_options(&h);
        new.push = (!sw.is_active()).then_some(false);
        save(new);
    });
    vec![
        labeled(&tr!("Engine to use"), &picker.button),
        note(&help),
        labeled(&tr!("Send one when none fits"), &push),
    ]
}

/// SSH 先の git が動かないときに、プログラムで選んだ git が理由ならそれを言い、PATH の git に戻せるようにする。
fn chosen_git_warning(settings: &Rc<RepoSettings>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let (repo, w, bw) = (settings.repo.clone(), settings.weak(), b.downgrade());
    glib::spawn_future_local(async move {
        let info = run_one(&repo, json!({"op": "machineGit"}))
            .await
            .unwrap_or_default();
        let git = crate::programs::MachineGit::from_info(&info);
        let (Some(chosen), Some(b)) = (git.chosen, bw.upgrade()) else {
            return;
        };
        let l = label(
            &tr!("The git chosen in Programs is {git}.", git = chosen),
            &["settings-warning", "caption"],
        );
        l.set_selectable(true);
        l.set_wrap(true);
        b.append(&l);
        let button = gtk::Button::with_label(&tr!("Use the git on the PATH"));
        button.add_css_class("flat");
        button.connect_clicked(move |_| {
            let Some(s) = w.upgrade() else { return };
            let (repo, w) = (s.repo.clone(), s.weak());
            glib::spawn_future_local(async move {
                let result = run_one(&repo, json!({"op": "setMachineGit"})).await;
                if let Some(s) = w.upgrade() {
                    s.show_error(None, result.err().as_deref());
                    s.load();
                }
            });
        });
        b.append(&button);
    });
    b
}

/// SSH 先の「環境変数」の上: エンジンを SSH 先のログインシェル経由で起動するか（SSH 先ごと。変えるとつなぎ直す）。
fn login_shell_rows(session: &Rc<HostSession>) -> gtk::Box {
    let host = session.host.clone();
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    b.set_margin_top(14);
    b.set_margin_start(20);
    b.set_margin_end(20);
    let check = gtk::CheckButton::with_label(&tr!("Use the login shell’s environment variables"));
    check.set_active(host_options(&host).login_shell == Some(true));
    let (h, w) = (host.clone(), Rc::downgrade(session));
    check.connect_toggled(move |c| {
        let mut options = host_options(&h);
        options.login_shell = c.is_active().then_some(true);
        set_host_options(&h, &options);
        if let Some(s) = w.upgrade() {
            s.connect();
        }
    });
    b.append(&check);
    let n = label(
        &tr!(
            "Starts the engine on {host} through its login shell, so it and git get the PATH and GIT_* set in ~/.profile and the like, which a command run over ssh lacks. Changing it reconnects.",
            host = host
        ),
        &["dim-label", "caption"],
    );
    n.set_wrap(true);
    n.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    n.set_margin_start(28);
    b.append(&n);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hosts_from_ssh_config_and_known_hosts() {
        let config = "Host build-1 dev-*\n  HostName 10.0.0.1\nHost=old-box\n";
        let known = "@cert-authority *.example ssh-ed25519 AAA\n\
                     |1|hashed= ssh-ed25519 AAA\n\
                     [box.example]:2222,10.0.0.9 ssh-ed25519 AAA\n\
                     [plain.example]:22 ssh-ed25519 AAA\n\
                     # comment\n";
        assert_eq!(
            parse_candidates(config, known),
            vec![
                "box.example:2222",
                "build-1",
                "old-box",
                "plain.example",
                "10.0.0.9"
            ]
        );
    }

    #[test]
    fn finds_the_host_of_a_location() {
        assert_eq!(
            host_of("ssh://me@box:2222/~/x").as_deref(),
            Some("me@box:2222")
        );
        assert_eq!(host_of("daemon:ssh://box/x").as_deref(), Some("box"));
        assert_eq!(host_of("ssh://box").as_deref(), Some("box"));
        assert_eq!(host_of("daemon:/x"), None);
        assert_eq!(host_of("/x"), None);
    }
}
