//! Settings > SSH Hosts（GTK4 版の `sshhosts.rs`、macOS 版の `SSHHostsSettings.swift`）: SSH 先ごとの設定。左に
//! SSH 先の一覧（＋／−、つながらないものは薄く）、右に選んだ SSH 先の接続のページ（状態と Check Connection、使う
//! エンジンと合うものがないときの送り込み、署名を検証する鍵）。選んだ SSH 先には、リポジトリなしで動かしたエンジン
//! （`ssh://host`、共有の daemon 越しなら `daemon:ssh://host`）でつなぐ。
//!
//! SSH 先ごとの選択（コアの `HostOptions`）と署名を検証する鍵は、リポジトリを開くとき（`remote::open`）に使う。

use std::collections::HashMap;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use amonhen_remote::ssh::HostOptions;
use serde_json::Value;
use windows_reactor::*;

/// ＋ で加えた SSH 先（JSON の配列。GTK4 版・macOS 版と同じ鍵）。
const SSH_HOSTS: &str = "sshHosts";

/// SSH 先へのつながり。
pub enum Conn {
    Connecting,
    Connected {
        /// SSH 先でリポジトリなしに動かしたエンジン（Git のページはこれで SSH 先の `--global` を読み書きする）。
        repo: Arc<Repository>,
        /// SSH 先で使っているエンジンと、その用意のしかた（共有の daemon 越しなら None）。
        engine: Option<String>,
        /// SSH 先の git の版（読めなければ None）。
        git: Option<String>,
        /// 共有の daemon 越しにつないだか。
        through_daemon: bool,
        /// エンジンが持っている環境（名前の順。そのまま SSH 先の git に渡る）。
        env: Vec<(String, String)>,
    },
    Failed(String),
}

pub struct Session {
    pub conn: Conn,
    /// 何回目のつなぎか（古いつなぎの結果は捨てる）。
    pub attempt: u64,
}

/// つないだ結果（裏のスレッドから）。
pub type Connected = Result<Reached, String>;

/// つながった SSH 先から読んだもの。
pub struct Reached {
    repo: Arc<Repository>,
    engine: Option<String>,
    git: Option<String>,
    env: Vec<(String, String)>,
}

pub enum Event {
    Select(Option<usize>),
    /// 真ん中の列のページ（[`PAGES`] の番号）を選んだ。
    Page(Option<usize>),
    /// 接続のページのスキャンするフォルダ: 深さ（その場所と、`levels` の番号）、外す、足す（列で選ぶ）。
    ScanDepth(String, Option<usize>),
    ScanRemove(String),
    ScanAdd,
    /// Environment Variables: 探す文字、ログインシェル経由で起動するか。
    EnvQuery(String),
    LoginShell(bool),
    /// ＋（加える欄を出す）・打った名前・候補を選んだ・加える・やめる。
    Add,
    AddText(String),
    AddPick(Option<usize>),
    AddDone(bool),
    /// −（Places にリポジトリがあれば確かめる）・確かめた答え。
    Remove,
    RemoveDone(bool),
    Check,
    Connected(String, u64, bool, Connected),
    Engine(Option<usize>),
    EnginePath(String),
    EnginePathDone(bool),
    Push(Option<usize>),
    Verify(Option<usize>),
}

/// 裏のスレッドでつなぐ仕事（`run` を裏で呼ぶ）。
pub struct Job {
    pub host: String,
    pub attempt: u64,
    location: String,
    options: HostOptions,
    local_signatures: bool,
    through_daemon: bool,
}

impl Job {
    pub fn run(self) -> Event {
        // エンジンを送ってよいかの問いは、窓の時計が拾って聞く（`remote::take_ask`）。
        let result =
            crate::remote::open(&self.location, self.options, self.local_signatures, &|| {}).map(
                |repo| {
                    let call = |command: &str| -> Value {
                        serde_json::from_str(&repo.run_json(command)).unwrap_or_default()
                    };
                    let engine = repo.connection_state().and_then(|c| c.engine);
                    let git = call(r#"{"op":"gitVersion"}"#)["info"]["version"]
                        .as_str()
                        .map(str::to_owned);
                    let mut env: Vec<(String, String)> =
                        call(r#"{"op":"processEnvironment"}"#)["info"]["vars"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|v| {
                                Some((
                                    v["name"].as_str()?.to_owned(),
                                    v["value"].as_str()?.to_owned(),
                                ))
                            })
                            .collect();
                    env.sort_by_key(|(n, _)| n.to_lowercase());
                    Reached {
                        repo: Arc::new(repo),
                        engine,
                        git,
                        env,
                    }
                },
            );
        Event::Connected(self.host, self.attempt, self.through_daemon, result)
    }
}

pub struct State {
    pub hosts: Vec<String>,
    pub selected: Option<String>,
    pub sessions: HashMap<String, Session>,
    /// ＋ で加えている名前（出していなければ None）。
    pub adding: Option<String>,
    /// − で外すのを確かめている（Places のリポジトリとスキャンするフォルダの数）。
    pub removing: Option<(usize, usize)>,
    /// エンジンの「その他…」で打っているパス（出していなければ None）。
    pub engine_path: Option<String>,
    /// 選ぶ欄を作り直す数（取りやめたとき、欄を今の値に戻す）。
    pub epoch: u32,
    /// 出しているページ（[`PAGES`] の番号）。
    pub page: usize,
    /// Environment Variables で探す文字。
    pub env_query: String,
    /// スキャンするフォルダを足すために、列で選ぶ画面を開く SSH 先（窓が見て開く）。
    pub pick_scan_root: Option<String>,
}

/// SSH 先ごとのページ（真ん中の列。訳は `tr!` と同じ表から）: 接続と、そこにあるすべてのリポジトリに効く
/// Settings > Git と同じページ。
pub const PAGES: [&str; 3] = ["Connection", "Git", "Environment Variables"];
const GIT_PAGE: usize = 1;
const ENV_PAGE: usize = 2;

impl State {
    pub fn new() -> (Self, Vec<Job>) {
        let hosts = known_hosts();
        let mut state = Self {
            selected: hosts.first().cloned(),
            hosts,
            sessions: HashMap::new(),
            adding: None,
            removing: None,
            engine_path: None,
            epoch: 0,
            page: 0,
            env_query: String::new(),
            pick_scan_root: None,
        };
        let jobs = state.session_for_selected().into_iter().collect();
        (state, jobs)
    }

    /// 選んだ SSH 先にまだつないでいなければつなぐ。
    fn session_for_selected(&mut self) -> Option<Job> {
        let host = self.selected.clone()?;
        if self.sessions.contains_key(&host) {
            return None;
        }
        Some(self.connect(&host))
    }

    fn connect(&mut self, host: &str) -> Job {
        let attempt = self.sessions.get(host).map_or(1, |s| s.attempt + 1);
        self.sessions.insert(
            host.to_owned(),
            Session {
                conn: Conn::Connecting,
                attempt,
            },
        );
        let through_daemon = crate::prefs::bool(crate::prefs::OPEN_THROUGH_DAEMON);
        let location = format!(
            "{}ssh://{host}",
            if through_daemon { "daemon:" } else { "" }
        );
        Job {
            host: host.to_owned(),
            attempt,
            options: crate::remote::options_for(&location),
            local_signatures: crate::remote::checks_signatures_locally(&location),
            location,
            through_daemon,
        }
    }

    fn reload(&mut self) {
        self.hosts = known_hosts();
        if self
            .selected
            .as_ref()
            .is_none_or(|h| !self.hosts.contains(h))
        {
            self.selected = self.hosts.first().cloned();
        }
    }

    /// 選んだ SSH 先の選択を変え、新しい選択でつなぎ直す。
    fn save_options(&mut self, change: impl FnOnce(&mut HostOptions)) -> Option<Job> {
        let host = self.selected.clone()?;
        let mut options = crate::remote::host_options(&host);
        let before = options.clone();
        change(&mut options);
        if options == before {
            return None;
        }
        crate::remote::set_host_options(&host, &options);
        Some(self.connect(&host))
    }

    /// Git のページで選んでいる SSH 先（Settings > Git の部品をそこに向ける）。
    pub fn git_host(&self) -> Option<String> {
        (self.page == GIT_PAGE)
            .then(|| self.selected.clone())
            .flatten()
    }

    /// つながっている SSH 先のエンジン。
    pub fn repo_of(&self, host: &str) -> Option<Arc<Repository>> {
        match &self.sessions.get(host)?.conn {
            Conn::Connected { repo, .. } => Some(repo.clone()),
            _ => None,
        }
    }

    pub fn handle(&mut self, event: Event) -> Vec<Job> {
        let mut jobs = Vec::new();
        match event {
            Event::Page(Some(i)) => {
                if i < PAGES.len() {
                    self.page = i;
                }
            }
            Event::Page(None) => {}
            Event::ScanDepth(path, Some(i)) => {
                crate::repositories::set_scan_depth(&path, i as u32 + 1);
            }
            Event::ScanDepth(_, None) => {}
            Event::ScanRemove(path) => crate::repositories::remove_scan_root(&path),
            Event::ScanAdd => self.pick_scan_root = self.selected.clone(),
            Event::EnvQuery(text) => self.env_query = text,
            Event::LoginShell(on) => {
                jobs.extend(self.save_options(|o| o.login_shell = on.then_some(true)));
            }
            Event::Select(Some(i)) => {
                if let Some(host) = self.hosts.get(i).cloned()
                    && self.selected.as_ref() != Some(&host)
                {
                    self.selected = Some(host);
                    self.engine_path = None;
                    self.removing = None;
                    jobs.extend(self.session_for_selected());
                }
            }
            Event::Select(None) => {}
            Event::Add => self.adding = Some(String::new()),
            Event::AddText(text) => {
                if self.adding.is_some() {
                    self.adding = Some(text);
                }
            }
            Event::AddPick(Some(i)) => {
                if let Some(typed) = &self.adding
                    && let Some(name) = add_candidates(typed, &self.hosts).get(i)
                {
                    self.adding = Some(name.clone());
                    self.epoch += 1;
                }
            }
            Event::AddPick(None) => {}
            Event::AddDone(add) => {
                let host = self.adding.take().unwrap_or_default();
                let host = host.trim();
                if add && valid_host(host) {
                    let mut added = added_hosts();
                    if !added.iter().any(|h| h == host) {
                        added.push(host.to_owned());
                        set_added_hosts(&added);
                    }
                    self.reload();
                    self.selected = Some(host.to_owned());
                    jobs.extend(self.session_for_selected());
                }
            }
            Event::Remove => {
                if let Some(host) = &self.selected {
                    let places = crate::repositories::places_on_host(host);
                    if places != (0, 0) {
                        self.removing = Some(places);
                    } else {
                        self.forget_selected();
                        jobs.extend(self.session_for_selected());
                    }
                }
            }
            Event::RemoveDone(remove) => {
                if self.removing.take().is_some() && remove {
                    if let Some(host) = &self.selected {
                        crate::repositories::forget_host(host);
                    }
                    self.forget_selected();
                    jobs.extend(self.session_for_selected());
                }
            }
            Event::Check => {
                if let Some(host) = self.selected.clone() {
                    jobs.push(self.connect(&host));
                }
            }
            Event::Connected(host, attempt, through_daemon, result) => {
                if let Some(session) = self.sessions.get_mut(&host)
                    && session.attempt == attempt
                {
                    session.conn = match result {
                        Ok(Reached {
                            repo,
                            engine,
                            git,
                            env,
                        }) => Conn::Connected {
                            repo,
                            engine,
                            git,
                            through_daemon,
                            env,
                        },
                        Err(e) => Conn::Failed(e),
                    };
                }
            }
            Event::Engine(Some(i)) => {
                let Some(host) = self.selected.clone() else {
                    return jobs;
                };
                let choices = engine_choices(&crate::remote::host_options(&host));
                match choices.get(i) {
                    Some((value, _)) if value == OTHER => {
                        let current = crate::remote::host_options(&host)
                            .engine
                            .filter(|e| is_engine_path(e))
                            .unwrap_or_else(|| "~/.local/share/amonhen".into());
                        self.engine_path = Some(current);
                    }
                    Some((value, _)) => {
                        let value = value.clone();
                        self.engine_path = None;
                        jobs.extend(self.save_options(|o| {
                            o.engine = (!value.is_empty()).then_some(value);
                        }));
                    }
                    None => {}
                }
            }
            Event::Engine(None) => {}
            Event::EnginePath(text) => {
                if self.engine_path.is_some() {
                    self.engine_path = Some(text);
                }
            }
            Event::EnginePathDone(apply) => {
                let path = self.engine_path.take().unwrap_or_default();
                let path = path.trim().to_owned();
                if apply && is_engine_path(&path) {
                    jobs.extend(self.save_options(|o| o.engine = Some(path)));
                }
                // 取りやめたら、欄を今の値に戻す。
                self.epoch += 1;
            }
            Event::Push(Some(i)) => {
                jobs.extend(self.save_options(|o| {
                    o.push = match i {
                        1 => Some(true),
                        2 => Some(false),
                        _ => None,
                    };
                }));
            }
            Event::Push(None) => {}
            Event::Verify(Some(i)) => {
                if let Some(host) = &self.selected {
                    crate::remote::set_checks_locally(host, i == 0);
                }
            }
            Event::Verify(None) => {}
        }
        jobs
    }

    /// 選んだ SSH 先を一覧から外す（＋ で加えたものと、つながり）。
    fn forget_selected(&mut self) {
        let Some(host) = self.selected.take() else {
            return;
        };
        let mut added = added_hosts();
        added.retain(|h| *h != host);
        set_added_hosts(&added);
        self.sessions.remove(&host);
        self.engine_path = None;
        self.reload();
    }
}

/// 「その他…」の項目の値。
const OTHER: &str = "\u{1}other";

fn added_hosts() -> Vec<String> {
    crate::prefs::string(SSH_HOSTS)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn set_added_hosts(hosts: &[String]) {
    let text = serde_json::to_string(hosts).unwrap_or_default();
    if let Err(e) = crate::prefs::set_string(SSH_HOSTS, &text) {
        eprintln!("SSH 先の一覧を書けない: {e}");
    }
}

/// Amon Hen が知っている SSH 先: Places にあるもの（リポジトリかスキャンするフォルダ）と、＋ で加えたもの
/// （GTK4 版・macOS 版と同じ）。
fn known_hosts() -> Vec<String> {
    let mut hosts = added_hosts();
    hosts.extend(crate::repositories::remote_hosts());
    hosts.sort_by_key(|h| h.to_lowercase());
    hosts.dedup();
    hosts
}

/// ssh に渡せる名前か（`/`・空白を含まない）。
fn valid_host(host: &str) -> bool {
    !host.is_empty() && !host.contains(|c: char| c == '/' || c.is_whitespace())
}

/// SSH の接続先の候補: `~/.ssh/config` の `Host` と `~/.ssh/known_hosts` のホスト（GTK4 版の `candidates`）。
fn candidates() -> Vec<String> {
    let Some(home) = std::env::var_os("USERPROFILE") else {
        return Vec::new();
    };
    let ssh = std::path::PathBuf::from(home).join(".ssh");
    let config = std::fs::read_to_string(ssh.join("config")).unwrap_or_default();
    let known = std::fs::read_to_string(ssh.join("known_hosts")).unwrap_or_default();
    parse_candidates(&config, &known)
}

/// ＋ の欄の下に出す候補（まだ一覧にないもので、打った文字で始まるもの。大文字小文字は区別しない）。
fn add_candidates(typed: &str, known: &[String]) -> Vec<String> {
    let lower = typed.trim().to_lowercase();
    candidates()
        .into_iter()
        .filter(|c| !known.contains(c) && c.to_lowercase().starts_with(&lower))
        .take(12)
        .collect()
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

/// パスで指定したエンジンか（`/…`、Windows の `C:\…`・`\\server\…`。GTK4 版と同じ）。
fn is_engine_path(value: &str) -> bool {
    let b = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

/// エンジンの選ぶ欄の項目（値と見出し）: 自動・SSH 先の PATH のもの・Amon Hen が送り込むもの・選んだパス・
/// その他…。
fn engine_choices(options: &HostOptions) -> Vec<(String, String)> {
    let mut items = vec![
        (String::new(), tr!("Automatic")),
        (
            "installed".to_owned(),
            tr!("The amonhen on the host’s PATH"),
        ),
        ("sent".to_owned(), tr!("The one Amon Hen sends")),
    ];
    if let Some(path) = options.engine.clone().filter(|e| is_engine_path(e)) {
        items.push((path.clone(), path));
    }
    items.push((OTHER.to_owned(), tr!("Other…")));
    items
}

/// 部品の知らせを [`Event`] にする関数から、部品に渡す Callback を作る。
type Make<'a, T, F> = Box<dyn Fn(F) -> Callback<T> + 'a>;
/// 選ぶ部品の知らせ（選んだ番号）を [`Event`] にする関数。
type OnPick = fn(Option<usize>) -> Event;

pub struct Callbacks<'a> {
    pub event: Make<'a, Option<usize>, OnPick>,
    pub text: Make<'a, String, fn(String) -> Event>,
    pub button: Make<'a, (), fn() -> Event>,
    pub check: Make<'a, bool, fn(bool) -> Event>,
    /// スキャンするフォルダの行（場所ごと）: 深さ、外す。
    pub scan_depth: Box<dyn Fn(String) -> Callback<Option<usize>> + 'a>,
    pub scan_remove: Box<dyn Fn(String) -> Callback<()> + 'a>,
}

fn heading(text: String) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(13.0)
        .opacity(0.8)
        .margin(Thickness::new(0.0, 14.0, 0.0, 4.0))
}

fn note(text: String) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(12.0)
        .opacity(0.6)
        .text_wrapping(TextWrapping::Wrap)
}

/// 「名前: 部品」の行。
fn labeled(name: String, content: impl Into<View>) -> View {
    Grid::new()
        .columns([GridLength::Pixel(140.0), GridLength::Star(1.0)])
        .margin(Thickness::new(0.0, 3.0, 0.0, 3.0))
        .children((
            TextBlock::new()
                .text(name)
                .grid_column(0)
                .margin(Thickness::new(0.0, 4.0, 8.0, 0.0)),
            Border::new().grid_column(1).content(content),
        ))
}

/// ボタン（`accent` なら既定の操作の色）。
fn button(text: String, on: Callback<()>, enabled: bool, accent: bool) -> View {
    let b = Button::new().on_click(on).is_enabled(enabled);
    let b = if accent {
        b.style(ButtonStyle::Accent)
    } else {
        b
    };
    b.content(TextBlock::new().text(text))
}

/// `git` は、Git のページで出す Settings > Git の部品（SSH 先に向けたもの。読み終えていなければ None）。
pub fn view(state: &State, callbacks: &Callbacks<'_>, git: Option<View>) -> View {
    let ok = Color::rgb(0x2e, 0x9e, 0x4f);
    let error = Color::rgb(0xd0, 0x3a, 0x3a);
    // 左: SSH 先の一覧と ＋／−。
    let rows = state.hosts.iter().map(|host| {
        let unreachable = state
            .sessions
            .get(host)
            .is_some_and(|s| matches!(s.conn, Conn::Failed(_)));
        let mut lines: Vec<(String, View)> = vec![(
            "name".into(),
            TextBlock::new()
                .text(host.clone())
                .opacity(if unreachable { 0.5 } else { 1.0 })
                .into(),
        )];
        if unreachable {
            lines.push(("unreachable".into(), note(tr!("unreachable")).into()));
        }
        (
            // 鍵は名前だけ（つながらない印で作り直すと、選んだ行の印が消える）。
            host.clone(),
            ListViewItem::new()
                .min_height(crate::small::ROW_HEIGHT)
                .content(
                    StackPanel::new()
                        .margin(Thickness::new(0.0, 3.0, 0.0, 3.0))
                        .children([View::keyed_fragment(lines)]),
                ),
        )
    });
    let selected = state
        .selected
        .as_ref()
        .and_then(|h| state.hosts.iter().position(|x| x == h));
    let list = ListView::new()
        .selected_index(selected)
        .on_selection_changed((callbacks.event)(Event::Select))
        .collection_slot(ListViewSlot::Items, rows);
    let mut left: Vec<(String, View)> =
        vec![("list".into(), Border::new().height(240.0).content(list))];
    left.push((
        "buttons".into(),
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(4.0)
            .margin(Thickness::new(0.0, 6.0, 0.0, 0.0))
            .children((
                crate::small::button("+", |b| b.on_click((callbacks.button)(|| Event::Add)))
                    .tooltip(tr!("Add an SSH host")),
                crate::small::button("−", |b| {
                    b.on_click((callbacks.button)(|| Event::Remove))
                        .is_enabled(state.selected.is_some())
                })
                .tooltip(tr!("Forget this host")),
            )),
    ));
    if let Some(typed) = &state.adding {
        left.push(("adding".into(), add_view(state, typed, callbacks)));
    }
    let left = StackPanel::new()
        .grid_column(0)
        .children([View::keyed_fragment(left)]);

    let right: View = match &state.selected {
        None => note(tr!("Add an SSH host with +, or open one from Places.")).into(),
        Some(host) if state.page == GIT_PAGE => git_view(state, host, callbacks, git, error),
        Some(host) if state.page == ENV_PAGE => env_view(state, host, callbacks, error),
        Some(host) => connection_view(state, host, callbacks, ok, error),
    };
    // 真ん中: 選んだ SSH 先のページ。
    let pages = ListView::new()
        .selected_index(Some(state.page))
        .on_selection_changed((callbacks.event)(Event::Page))
        .collection_slot(
            ListViewSlot::Items,
            PAGES.iter().map(|title| {
                (
                    (*title).to_owned(),
                    ListViewItem::new()
                        .min_height(crate::small::ROW_HEIGHT)
                        .content(
                            TextBlock::new()
                                .text(amonhen_model::i18n::translate(title).into_owned()),
                        ),
                )
            }),
        );
    StackPanel::new().children((
        TextBlock::new()
            .text(tr!("SSH Hosts"))
            .font_size(20.0)
            .margin(Thickness::new(0.0, 16.0, 0.0, 10.0)),
        Grid::new()
            .columns([
                GridLength::Pixel(160.0),
                GridLength::Pixel(110.0),
                GridLength::Star(1.0),
            ])
            .column_spacing(16.0)
            .children((
                left,
                Border::new().grid_column(1).content(pages),
                // 選んだ SSH 先・ページ・つながりの状態が変わったら作り直す（同じ形の部品を使い回すと、つながった
                // ときに中身が出し直されないことがある）。
                Border::new()
                    .grid_column(2)
                    .content(View::keyed_fragment([(right_key(state), right)])),
            )),
    ))
}

/// 「スキャンするフォルダ」: この SSH 先でスキャンするフォルダ（Places の、この SSH 先の下と同じ設定）。深さを
/// 選べ、− で外し、＋ で SSH 先のフォルダを列で見て足す（GTK4 版の `scan_folders`）。
fn scan_folders(host: &str, callbacks: &Callbacks<'_>) -> View {
    let levels: Vec<String> = (1..=6u32)
        .map(|d| tr!("{count} levels", count = d))
        .collect();
    let rows = crate::repositories::host_scan_roots(host)
        .into_iter()
        .map(|(path, depth)| {
            let shown = amonhen_remote::ssh::Target::parse(&path).map_or_else(
                |_| path.clone(),
                |t| t.path.trim_start_matches('/').to_owned(),
            );
            (
                format!("{path}|{depth}"),
                Grid::new()
                    .columns([GridLength::Star(1.0), GridLength::Auto, GridLength::Auto])
                    .column_spacing(6.0)
                    .margin(Thickness::new(0.0, 2.0, 0.0, 2.0))
                    .children((
                        TextBlock::new()
                            .text(shown)
                            .is_text_selection_enabled(true)
                            .vertical_alignment(VerticalAlignment::Center)
                            .text_trimming(TextTrimming::CharacterEllipsis)
                            .grid_column(0),
                        ComboBox::new()
                            .grid_column(1)
                            .items_source(levels.clone())
                            .selected_index(depth.clamp(1, 6) as usize - 1)
                            .on_selection_changed((callbacks.scan_depth)(path.clone())),
                        crate::small::button("−", |b| {
                            b.grid_column(2).on_click((callbacks.scan_remove)(path))
                        }),
                    )),
            )
        });
    StackPanel::new().children((
        heading(tr!("Folders to Scan")),
        StackPanel::new().children([View::keyed_fragment(rows)]),
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .margin(Thickness::new(0.0, 4.0, 0.0, 0.0))
            .children((
                button(
                    "+".into(),
                    (callbacks.button)(|| Event::ScanAdd),
                    true,
                    false,
                )
                .tooltip(tr!("Add a Folder to Scan")),
                note(tr!("The same list as under the host in Places."))
                    .vertical_alignment(VerticalAlignment::Center),
            )),
    ))
}

/// 右の列の中身の鍵。
fn right_key(state: &State) -> String {
    let conn = state
        .selected
        .as_ref()
        .and_then(|h| state.sessions.get(h))
        .map_or("none", |s| match s.conn {
            Conn::Connecting => "connecting",
            Conn::Connected { .. } => "connected",
            Conn::Failed(_) => "failed",
        });
    format!(
        "{}|{}|{conn}",
        state.selected.as_deref().unwrap_or_default(),
        state.page
    )
}

/// Git のページ: SSH 先の `--global`（Settings > Git と同じ部品）。つないでいる途中・つながらないなら、そう出す。
fn git_view(
    state: &State,
    host: &str,
    callbacks: &Callbacks<'_>,
    git: Option<View>,
    error: Color,
) -> View {
    let Some(Conn::Connected { .. }) = state.sessions.get(host).map(|s| &s.conn) else {
        return not_connected(state, host, callbacks, error);
    };
    StackPanel::new().children((
        note(tr!(
            "Applies to every repository on {host} (its ~/.gitconfig). git in a terminal there follows these too.",
            host = host
        ))
        .margin(Thickness::new(0.0, 0.0, 0.0, 8.0)),
        git.unwrap_or_else(|| note(tr!("Loading…")).into()),
    ))
}

/// Environment Variables のページ: エンジンを SSH 先のログインシェル経由で起動するか（Windows の SSH 先には
/// ログインシェルがないので出さない）と、エンジンが実際に持っている環境（そのまま git に渡る。GTK4 版の
/// `environment::host_page`）。
fn env_view(state: &State, host: &str, callbacks: &Callbacks<'_>, error: Color) -> View {
    let Some(Conn::Connected { env, .. }) = state.sessions.get(host).map(|s| &s.conn) else {
        return not_connected(state, host, callbacks, error);
    };
    let windows = env.iter().any(|(n, v)| n == "OS" && v == "Windows_NT");
    let query = state.env_query.to_lowercase();
    let secret = crate::settingsview::is_secret;
    let rows = env
        .iter()
        .filter(|(name, value)| {
            query.is_empty()
                || name.to_lowercase().contains(&query)
                || (!secret(name) && value.to_lowercase().contains(&query))
        })
        .map(|(name, value)| {
            (
                name.clone(),
                Grid::new()
                    .columns([GridLength::Pixel(200.0), GridLength::Star(1.0)])
                    .margin(Thickness::new(0.0, 1.0, 0.0, 1.0))
                    .children((
                        TextBlock::new()
                            .text(name.clone())
                            .font_size(12.0)
                            .is_text_selection_enabled(true)
                            .text_trimming(TextTrimming::CharacterEllipsis)
                            .grid_column(0),
                        TextBlock::new()
                            .text(if secret(name) {
                                "••••••••".to_owned()
                            } else {
                                value.clone()
                            })
                            .font_size(12.0)
                            .opacity(0.7)
                            .is_text_selection_enabled(!secret(name))
                            .text_trimming(TextTrimming::CharacterEllipsis)
                            .grid_column(1),
                    )),
            )
        });
    let mut parts: Vec<(String, View)> = Vec::new();
    if !windows {
        let login = crate::remote::host_options(host).login_shell == Some(true);
        parts.push((
            format!("login-{login}"),
            CheckBox::new()
                .is_checked(login)
                .on_is_checked_changed((callbacks.check)(Event::LoginShell))
                .content(TextBlock::new().text(tr!("Use the login shell’s environment variables"))),
        ));
        parts.push((
            "login-note".into(),
            note(tr!(
                "Starts the engine on {host} through its login shell, so it and git get the PATH and GIT_* set in ~/.profile and the like, which a command run over ssh lacks. Changing it reconnects.",
                host = host
            ))
            .margin(Thickness::new(28.0, 0.0, 0.0, 10.0))
            .into(),
        ));
    }
    parts.extend([
        (
            "caption".into(),
            note(tr!(
                "The engine’s environment, which git on {host} gets",
                host = host
            ))
            .into(),
        ),
        (
            "query".into(),
            TextBox::new()
                .text(state.env_query.clone())
                .placeholder_text(tr!("Search by name or value"))
                .margin(Thickness::new(0.0, 4.0, 0.0, 4.0))
                .on_text_changed((callbacks.text)(Event::EnvQuery))
                .into(),
        ),
        (
            "rows".into(),
            ScrollViewer::new()
                .max_height(360.0)
                .content(StackPanel::new().children([View::keyed_fragment(rows)])),
        ),
    ]);
    StackPanel::new().children([View::keyed_fragment(parts)])
}

/// つないでいる途中・つながらない SSH 先のページ。
fn not_connected(state: &State, host: &str, callbacks: &Callbacks<'_>, error: Color) -> View {
    match state.sessions.get(host).map(|s| &s.conn) {
        Some(Conn::Failed(reason)) => StackPanel::new().spacing(8.0).children((
            TextBlock::new().text(tr!("Couldn’t connect to {host}", host = host)),
            note(reason.clone())
                .foreground(Brush::Solid(error))
                .is_text_selection_enabled(true),
            button(
                tr!("Check Connection"),
                (callbacks.button)(|| Event::Check),
                true,
                false,
            ),
        )),
        _ => StackPanel::new()
            .orientation(Orientation::Horizontal)
            .vertical_alignment(VerticalAlignment::Top)
            .spacing(8.0)
            .children((
                ProgressRing::new().is_active(true).width(16.0).height(16.0),
                TextBlock::new().text(tr!("Connecting to {host}…", host = host)),
            )),
    }
}

/// ＋ の欄: 名前と、~/.ssh/config・known_hosts の候補。
fn add_view(state: &State, typed: &str, callbacks: &Callbacks<'_>) -> View {
    let names = add_candidates(typed, &state.hosts);
    let list = ListView::new()
        .on_selection_changed((callbacks.event)(Event::AddPick))
        .collection_slot(
            ListViewSlot::Items,
            names.iter().map(|n| {
                (
                    n.clone(),
                    ListViewItem::new()
                        .min_height(crate::small::ROW_HEIGHT)
                        .content(TextBlock::new().text(n.clone()).font_size(12.0)),
                )
            }),
        );
    StackPanel::new()
        .spacing(4.0)
        .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
        .children((
            TextBlock::new().text(tr!("Add an SSH host")),
            note(tr!(
                "Write it as ssh knows it: user@host:port, or a name in ~/.ssh/config."
            )),
            View::keyed_fragment([(
                format!("add-{}", state.epoch),
                TextBox::new()
                    .text(typed.to_owned())
                    .on_text_changed((callbacks.text)(Event::AddText)),
            )]),
            Border::new().max_height(160.0).content(list),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(6.0)
                .children((
                    button(
                        tr!("Add"),
                        (callbacks.button)(|| Event::AddDone(true)),
                        valid_host(typed.trim()),
                        true,
                    ),
                    button(
                        tr!("Cancel"),
                        (callbacks.button)(|| Event::AddDone(false)),
                        true,
                        false,
                    ),
                )),
        ))
}

/// 「接続」のページ: SSH 先と状態、使うエンジン、署名の検証に使う鍵。
fn connection_view(
    state: &State,
    host: &str,
    callbacks: &Callbacks<'_>,
    ok: Color,
    error: Color,
) -> View {
    let session = state.sessions.get(host);
    let connecting = session.is_none_or(|s| matches!(s.conn, Conn::Connecting));
    let status: View = match session.map(|s| &s.conn) {
        None | Some(Conn::Connecting) => StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(6.0)
            .children((
                ProgressRing::new().is_active(true).width(16.0).height(16.0),
                TextBlock::new().text(tr!("Connecting…")),
            )),
        Some(Conn::Connected { git, .. }) => StackPanel::new().children((
            TextBlock::new()
                .text(format!("● {}", tr!("Connected")))
                .foreground(Brush::Solid(ok)),
            note(git.as_ref().map_or_else(
                || tr!("git wasn’t found on the host"),
                |v| format!("git {v}"),
            )),
        )),
        Some(Conn::Failed(reason)) => StackPanel::new().children((
            TextBlock::new()
                .text(format!("● {}", tr!("unreachable")))
                .foreground(Brush::Solid(error)),
            note(reason.clone()).is_text_selection_enabled(true),
        )),
    };
    let state_row = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .children((
            Border::new().grid_column(0).content(status),
            Border::new()
                .grid_column(1)
                .vertical_alignment(VerticalAlignment::Top)
                .content(button(
                    tr!("Check Connection"),
                    (callbacks.button)(|| Event::Check),
                    !connecting,
                    false,
                )),
        ));

    // 使っているエンジン。
    let (engine, through_daemon, connected) = match session.map(|s| &s.conn) {
        Some(Conn::Connected {
            engine,
            through_daemon,
            ..
        }) => (engine.clone(), *through_daemon, true),
        _ => (None, false, false),
    };
    let mut in_use: Vec<(String, View)> = vec![(
        "engine".into(),
        TextBlock::new()
            .text(engine.clone().unwrap_or_else(|| {
                if connected {
                    tr!("Through the shared daemon")
                } else {
                    tr!("Not connected")
                }
            }))
            .opacity(0.75)
            .is_text_selection_enabled(true)
            .text_wrapping(TextWrapping::Wrap)
            .into(),
    )];
    // daemon 越しなら、エンジンは daemon が起動したもの（同じ SSH 先を開いたほかの相手と共有する）。
    if engine.is_some() && through_daemon {
        in_use.push((
            "daemon".into(),
            note(tr!("Started by the shared daemon")).into(),
        ));
    }

    let options = crate::remote::host_options(host);
    let choices = engine_choices(&options);
    let current = options.engine.as_deref().unwrap_or_default();
    let engine_index = choices.iter().position(|(v, _)| v == current).unwrap_or(0);
    let engine_box = View::keyed_fragment([(
        format!("engine-{host}-{}", state.epoch),
        ComboBox::new()
            .items_source(choices.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>())
            .selected_index(engine_index)
            .on_selection_changed((callbacks.event)(Event::Engine)),
    )]);
    let help = match options.engine.as_deref() {
        Some("installed") => tr!("Uses the amonhen installed on the host. It must be this build."),
        Some("sent") => tr!(
            "Uses the one Amon Hen sent to the host, sending one when there is none (whether to ask first is chosen below)."
        ),
        Some(p) if is_engine_path(p) => tr!("Uses this path on the host."),
        _ => tr!(
            "The amonhen on the host’s PATH if it is this build, else one sent earlier, else the one Amon Hen sends."
        ),
    };
    let mut engine_rows: Vec<(String, View)> = vec![
        ("choice".into(), labeled(tr!("Engine to use"), engine_box)),
        ("help".into(), note(help).into()),
    ];
    if let Some(path) = &state.engine_path {
        engine_rows.push((
            "path".into(),
            StackPanel::new()
                .spacing(4.0)
                .margin(Thickness::new(0.0, 6.0, 0.0, 6.0))
                .children((
                    TextBlock::new().text(tr!("Engine on {host}", host = host)),
                    note(tr!("Choose amonhen on the host. It must be this build.")),
                    TextBox::new()
                        .text(path.clone())
                        .on_text_changed((callbacks.text)(Event::EnginePath)),
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(6.0)
                        .children((
                            button(
                                tr!("Use"),
                                (callbacks.button)(|| Event::EnginePathDone(true)),
                                is_engine_path(path.trim()),
                                true,
                            ),
                            button(
                                tr!("Cancel"),
                                (callbacks.button)(|| Event::EnginePathDone(false)),
                                true,
                                false,
                            ),
                        )),
                )),
        ));
    }
    // 合うものがないとき: 送る前に聞く（既定）・聞かずに送る・送らない。
    let push_index = match options.push {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    };
    engine_rows.push((
        "push".into(),
        labeled(
            tr!("When none fits"),
            View::keyed_fragment([(
                format!("push-{host}-{push_index}"),
                ComboBox::new()
                    .items_source(vec![
                        tr!("Ask before sending one"),
                        tr!("Send one without asking"),
                        tr!("Don’t send one"),
                    ])
                    .selected_index(push_index)
                    .is_enabled(
                        options.engine.as_deref() != Some("installed")
                            && !options.engine.as_deref().is_some_and(is_engine_path),
                    )
                    .on_selection_changed((callbacks.event)(Event::Push)),
            )]),
        ),
    ));
    engine_rows.push((
        "in-use".into(),
        labeled(
            tr!("In use"),
            StackPanel::new().children([View::keyed_fragment(in_use)]),
        ),
    ));
    // 環境変数で決めてあれば、そちらが優先される。
    if let Some(name) = ["AMONHEN_REMOTE_ENGINE", "AMONHEN_PUSH"]
        .into_iter()
        .find(|n| std::env::var(n).is_ok_and(|v| !v.is_empty()))
    {
        engine_rows.push((
            "env".into(),
            note(tr!(
                "{name} is set in the environment, so it decides instead.",
                name = name
            ))
            .into(),
        ));
    }

    let local = crate::remote::checks_signatures_locally(&format!("ssh://{host}"));
    let mut rows: Vec<(String, View)> = Vec::new();
    if let Some((repositories, folders)) = state.removing {
        rows.push((
            "removing".into(),
            Border::new()
                .background(Brush::Solid(crate::palette::note_band()))
                .padding(Thickness::new(10.0, 8.0, 10.0, 8.0))
                .margin(Thickness::new(0.0, 0.0, 0.0, 8.0))
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new().text(tr!("Remove {host} from List?", host = host)),
                        note(tr!(
                            "This removes its {repositories} repositories and {folders} folders to scan from the list. Nothing on the host is touched.",
                            repositories = repositories,
                            folders = folders
                        )),
                        StackPanel::new()
                            .orientation(Orientation::Horizontal)
                            .spacing(6.0)
                            .children((
                                button(
                                    tr!("Remove from List"),
                                    (callbacks.button)(|| Event::RemoveDone(true)),
                                    true,
                                    true,
                                ),
                                button(
                                    tr!("Cancel"),
                                    (callbacks.button)(|| Event::RemoveDone(false)),
                                    true,
                                    false,
                                ),
                            )),
                    )),
                )
                ,
        ));
    }
    rows.extend([
        ("h-connection".into(), heading(tr!("Connection")).into()),
        (
            "host".into(),
            labeled(
                tr!("Host"),
                TextBlock::new()
                    .text(host.to_owned())
                    .is_text_selection_enabled(true),
            ),
        ),
        ("state".into(), labeled(tr!("State"), state_row)),
        ("h-engine".into(), heading(tr!("Engine")).into()),
        (
            "engine".into(),
            StackPanel::new().children([View::keyed_fragment(engine_rows)]),
        ),
        (
            "h-signature".into(),
            heading(tr!("Signature Verification")).into(),
        ),
        (
            "verify".into(),
            labeled(
                tr!("Verify with"),
                View::keyed_fragment([(
                    format!("verify-{host}-{local}"),
                    ComboBox::new()
                        .items_source(vec![
                            tr!("This computer’s keys"),
                            tr!("The host’s git and keys"),
                        ])
                        .selected_index(if local { 0 } else { 1 })
                        .on_selection_changed((callbacks.event)(Event::Verify)),
                )]),
            ),
        ),
        (
            "verify-note".into(),
            note(tr!(
                "Signatures of commits in repositories on this host are checked with these."
            ))
            .into(),
        ),
        ("scan".into(), scan_folders(host, callbacks)),
    ]);
    StackPanel::new().children([View::keyed_fragment(rows)])
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
    fn engine_paths() {
        assert!(is_engine_path("/usr/local/bin/amonhen"));
        assert!(is_engine_path(r"C:\tools\amonhen.exe"));
        assert!(is_engine_path(r"\\server\share\amonhen.exe"));
        assert!(!is_engine_path("~/.local/share/amonhen"));
        assert!(!is_engine_path("installed"));
    }

    #[test]
    fn host_names() {
        assert!(valid_host("me@box:2222"));
        assert!(!valid_host("box/path"));
        assert!(!valid_host("my box"));
        assert!(!valid_host(""));
    }
}
