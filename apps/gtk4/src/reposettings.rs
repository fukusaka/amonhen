//! リポジトリの設定（macOS 版の `RepositorySettings.swift`。docs/ux.md の 4 章）。
//!
//! 左にページの一覧、右に選んだページ。行は「質問とその下に 1 文の説明、効いている値の欄（どこにもなければ git の
//! 既定）、この段が上書きしている間だけ ↩︎、設定元、ⓘ（キーと値とファイル、上書きしなければ効く値）」。値を選ぶと
//! すぐ書き、失敗したら行の下に赤で出して欄を前の値に戻す（読み直すと元の値が出る）。書き換えは Ctrl+Z／
//! Ctrl+Shift+Z で何段でも行き来できる（ページを出している間。リポジトリの Undo とは別の記録）。
//!
//! 同じ部品を 2 つの段で使う: このリポジトリ（`--local` に書く）と、すべてのリポジトリ（`--global` に書く。アプリの
//! Settings > Git が使う。握るリポジトリは空のもので、その local の値は見ない）。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

/// どの段の設定か。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// このリポジトリ（`--local` に書く）。
    Repository,
    /// その機械のすべてのリポジトリ（`--global` に書く）。
    AllRepositories,
}

/// git の設定の 1 項目と、どこで決まっているか（コアの `configList`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub scope: String,
    pub origin: String,
    pub key: String,
    pub value: String,
}

/// 署名に使える鍵（コアの `signingKeys`）。
#[derive(Clone, Debug)]
struct SigningKey {
    id: String,
    label: String,
    emails: Vec<String>,
}

/// リモート（コアの `remotes`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Remote {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

/// フック（コアの `hooksAndLfs`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hook {
    pub name: String,
    pub enabled: bool,
}

/// 読んだもの。
#[derive(Default)]
pub(crate) struct State {
    pub entries: Vec<Entry>,
    /// git が知っている設定の変数の名前（設定の有無は問わない。読めなければ空）。
    pub known_keys: Vec<String>,
    pub remotes: Vec<Remote>,
    pub gitignore: String,
    /// `.git/info/exclude`（すべてのリポジトリの段では global excludes のファイル）の中身。
    pub exclude: String,
    /// すべてのリポジトリの段での、無視するパターンのファイルのパス。
    pub excludes_path: String,
    pub hooks_path: Option<String>,
    pub hooks: Vec<Hook>,
    pub lfs: bool,
    pub lfs_patterns: Vec<String>,
    /// LFS にあるファイルの数（git-lfs がなければ None）。
    pub lfs_files: Option<u64>,
    /// このリポジトリを扱う git の版（SSH 先なら SSH 先の git）。
    pub git_version: Option<String>,
    /// rebase で積み重ねたブランチを一緒に動かせるか（git 2.38 から）。
    pub update_refs: bool,
    /// `rr-cache` があるか（`rerere.enabled` が書かれていなければ、git はこれがあるとき rerere を使う）。
    pub rerere_cache: bool,
    pub local_branches: Vec<String>,
    /// 判定した既定のブランチ。
    pub detected_base: Option<String>,
    /// このリポジトリ専用のコミットのテンプレート: 中身（ファイルがなければ None）と、`commit.template` に書くパス。
    pub commit_template: Option<(Option<String>, String)>,
    /// 署名の形式ごとに集めた鍵（`user.email` の鍵が先）。
    signing_keys: HashMap<String, Vec<SigningKey>>,
    pub loaded: bool,
}

/// 設定の書き換え 1 つ（`new` が None なら消す。`Some("")` は空の値を書く）。
#[derive(Clone, Debug)]
pub(crate) struct Change {
    pub key: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

impl Change {
    fn inverse(&self) -> Self {
        Self {
            key: self.key.clone(),
            old: self.new.clone(),
            new: self.old.clone(),
        }
    }
}

/// 書き換えの記録の向き。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Record {
    /// ふつうの書き換え（Undo に積み、やり直しの記録は捨てる）。
    New,
    Undo,
    Redo,
}

type Refresher = Rc<dyn Fn(&Rc<RepoSettings>)>;

/// ページ（macOS 版の `SettingsPage`）。名前は `--repository-settings <page>` でも使う。
pub const PAGES: &[&str] = &[
    "author",
    "commits",
    "operations",
    "branches",
    "workingCopy",
    "remotes",
    "hooks",
    "config",
];

fn page_title(page: &str) -> String {
    match page {
        "author" => tr!("Author and Signing"),
        "commits" => tr!("Commits"),
        "operations" => tr!("Operations"),
        "branches" => tr!("Branches"),
        "workingCopy" => "Working Copy".into(),
        "remotes" => tr!("Remotes"),
        "hooks" => "Hooks · LFS".into(),
        "config" => tr!("Keys and Values"),
        _ => page.into(),
    }
}

/// その段にあるページ。リポジトリにしかないものは、このリポジトリの段だけ。
fn pages(level: Level) -> Vec<&'static str> {
    match level {
        Level::Repository => PAGES.to_vec(),
        Level::AllRepositories => vec!["author", "commits", "operations", "workingCopy", "config"],
    }
}

/// リポジトリの設定の部品。
pub struct RepoSettings {
    pub(crate) repo: Arc<Repository>,
    pub(crate) level: Level,
    /// SSH 先のリポジトリなら、その SSH 先の名前。
    pub(crate) host: Option<String>,
    pub(crate) state: RefCell<State>,
    refreshers: RefCell<Vec<Refresher>>,
    /// 読んだ値を欄に入れている間（欄の知らせで書かない）。
    quiet: Cell<bool>,
    undo: RefCell<Vec<Vec<Change>>>,
    redo: RefCell<Vec<Vec<Change>>>,
    /// git の設定・リモートなどを変えた（窓を読み直す）。
    on_change: RefCell<Option<Rc<dyn Fn()>>>,
    /// 部品のいちばん外（左の一覧と右のページ）。
    pub widget: gtk::Box,
    stack: gtk::Stack,
    /// ページの下の、行に付かないエラー。
    error: gtk::Label,
    /// 読み込みの世代（古い読み込みの答えは捨てる）。
    generation: Cell<u64>,
    signing_loaded: Cell<bool>,
    /// 書けた書き換えの数（Keys and Values が、最後の削除のあとに書き換えがあったかを見る）。
    writes: Cell<u64>,
    /// 行の質問と、その欄・↩︎（確認用の `AMONHEN_SETTINGS_SCRIPT` が使う）。
    controls: RefCell<Vec<(String, gtk::Box, gtk::Button)>>,
}

/// コアの命令を実行し、`info` を返す（失敗ならエラーの文）。
pub(crate) fn call(repo: &Repository, command: &Value) -> Result<Value, String> {
    let reply: Value =
        serde_json::from_str(&repo.run_json(&command.to_string())).map_err(|e| e.to_string())?;
    match reply["error"].as_str() {
        Some(e) => Err(e.to_owned()),
        None => Ok(reply["info"].clone()),
    }
}

/// 裏で `commands` を順に実行する。失敗したらそこで止めて、エラーの文を返す。
pub(crate) async fn run_all(repo: &Arc<Repository>, commands: Vec<Value>) -> Result<(), String> {
    let repo = repo.clone();
    gio::spawn_blocking(move || {
        for c in &commands {
            call(&repo, c)?;
        }
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("panic".into()))
}

/// 裏で 1 つの命令を実行し、`info` を返す。
pub(crate) async fn run_one(repo: &Arc<Repository>, command: Value) -> Result<Value, String> {
    let repo = repo.clone();
    gio::spawn_blocking(move || call(&repo, &command))
        .await
        .unwrap_or_else(|_| Err("panic".into()))
}

/// 読んだもの（裏の糸で作る）。
struct Loaded {
    entries: Vec<Entry>,
    known_keys: Vec<String>,
    remotes: Vec<Remote>,
    gitignore: String,
    exclude: String,
    excludes_path: String,
    hooks: Value,
    version: Value,
    groups: Option<Value>,
    template: Option<Value>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

impl Loaded {
    fn read(repo: &Repository, level: Level) -> Result<Self, String> {
        let global = level == Level::AllRepositories;
        let config = call(repo, &json!({"op": "configList"}))?;
        let mut entries: Vec<Entry> = config["entries"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|e| Entry {
                        scope: e["scope"].as_str().unwrap_or_default().to_owned(),
                        origin: e["origin"].as_str().unwrap_or_default().to_owned(),
                        key: e["key"].as_str().unwrap_or_default().to_owned(),
                        value: e["value"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        // `command` は Amon Hen が git を起動するときの `-c` で、リポジトリの設定ではない。`unknown` は git が
        // 組み込みで読むファイル（Apple の git の Xcode の gitconfig など）で、機械全体のもの。
        entries.retain(|e| e.scope != "command");
        for e in &mut entries {
            if e.scope == "unknown" {
                e.scope = "system".into();
            }
        }
        // すべてのリポジトリの段では、握っている空のリポジトリの local は見ない。
        if global {
            entries.retain(|e| e.scope != "local" && e.scope != "worktree");
        }
        let known_keys = call(repo, &json!({"op": "configKeys"}))
            .map(|k| strings(&k["keys"]))
            .unwrap_or_default();
        let remotes = call(repo, &json!({"op": "remotes"}))?["remotes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|r| Remote {
                        name: r["name"].as_str().unwrap_or_default().to_owned(),
                        fetch_url: r["fetchUrl"].as_str().unwrap_or_default().to_owned(),
                        push_url: r["pushUrl"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ignore = call(repo, &json!({"op": "ignoreFiles"}))?;
        let (exclude, excludes_path) = if global {
            let x = call(repo, &json!({"op": "excludesFile"}))?;
            (
                x["text"].as_str().unwrap_or_default().to_owned(),
                x["path"].as_str().unwrap_or_default().to_owned(),
            )
        } else {
            (
                ignore["exclude"].as_str().unwrap_or_default().to_owned(),
                String::new(),
            )
        };
        Ok(Self {
            entries,
            known_keys,
            remotes,
            gitignore: ignore["gitignore"].as_str().unwrap_or_default().to_owned(),
            exclude,
            excludes_path,
            hooks: call(repo, &json!({"op": "hooksAndLfs"}))?,
            version: call(repo, &json!({"op": "gitVersion"}))?,
            groups: call(repo, &json!({"op": "branchGroups"})).ok(),
            template: call(repo, &json!({"op": "commitTemplate"})).ok(),
        })
    }
}

/// git の真偽値を `true`・`false` にそろえる（`yes`・`on`・`1`、値のない項目も true）。
pub(crate) fn normalized_bool(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" | "" => Some("true"),
        "false" | "no" | "off" | "0" => Some("false"),
        _ => None,
    }
}

/// 設定元（`git config` の scope）の言い方。行の右と Keys and Values で同じ言葉を使う。
pub(crate) fn scope_label(scope: &str, host: Option<&str>) -> String {
    match scope {
        "local" => tr!("this repository"),
        "global" => match host {
            Some(host) => tr!("all repositories on {host}", host = host),
            None => tr!("all repositories"),
        },
        "worktree" => tr!("this worktree"),
        "system" => tr!("system-wide"),
        other => other.to_owned(),
    }
}

/// ホームを `~` にする。
pub(crate) fn tilde(path: &str) -> String {
    if let Some(home) = glib::home_dir().to_str().filter(|h| !h.is_empty())
        && let Some(rest) = path.strip_prefix(home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    path.to_owned()
}

/// 設定が書かれているファイル（`file:` を取り、ホームは ~ にする）。
pub(crate) fn config_file(entry: &Entry) -> String {
    tilde(entry.origin.strip_prefix("file:").unwrap_or(&entry.origin))
}

/// 場所（`ssh://[user@]host[:port]/path`）の SSH 先の名前。
pub fn host_of(location: &str) -> Option<String> {
    let rest = location.strip_prefix("ssh://")?;
    let authority = rest.split('/').next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_owned())
}

/// 色（テーマの色を使う）。
fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".settings-error { color: @error_color; } \
         .settings-warning { color: @warning_color; } \
         .settings-ok { color: @success_color; } \
         .settings-list > row { padding: 4px 6px; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub(crate) fn label(text: &str, css: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in css {
        l.add_css_class(c);
    }
    l
}

/// 説明の文（本文の大きさ、薄い色。質問から少し下げる）。
pub(crate) fn help_label(text: &str) -> gtk::Label {
    let l = label(text, &["dim-label"]);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
    l.set_margin_start(8);
    l.set_visible(!text.is_empty());
    l
}

/// 行の下のエラー（赤。初めは隠す）。
pub(crate) fn error_label() -> gtk::Label {
    let l = label("", &["settings-error"]);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_margin_start(8);
    l.set_selectable(true);
    l.set_visible(false);
    l
}

/// 見出しと、枠で囲んだ行の一覧（macOS のグループ化した Form の節）。
pub(crate) fn section(title: Option<&str>, rows: &[gtk::Widget]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    if let Some(title) = title {
        b.append(&label(title, &["heading"]));
    }
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_show_separators(true);
    list.add_css_class("settings-list");
    for r in rows {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.set_child(Some(r));
        list.append(&row);
    }
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&list));
    b.append(&frame);
    b
}

/// 節を縦に並べたページ（スクロールする）。
pub(crate) fn page(sections: &[gtk::Box]) -> gtk::ScrolledWindow {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 18);
    b.set_margin_top(18);
    b.set_margin_bottom(18);
    b.set_margin_start(20);
    b.set_margin_end(20);
    for s in sections {
        b.append(s);
    }
    gtk::ScrolledWindow::builder()
        .child(&b)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build()
}

/// 選ぶ欄（macOS の Picker）: 押すと、節と区切りのあるメニューが出る。今の値に印が付く。値でない項目
/// （「その他…」「ファイル…」）は `special` に回す。
#[derive(Clone)]
pub(crate) struct Picker {
    pub button: gtk::MenuButton,
    label: gtk::Label,
    action: gio::SimpleAction,
}

/// メニューの節: 見出し（なければ区切りだけ）と、値と言い方の組。
pub(crate) type Options = Vec<(Option<String>, Vec<(String, String)>)>;

impl Picker {
    pub fn new(on_pick: impl Fn(String) + 'static) -> Self {
        let action = gio::SimpleAction::new_stateful(
            "pick",
            Some(glib::VariantTy::STRING),
            &"".to_variant(),
        );
        let on_pick = Rc::new(on_pick);
        let f = on_pick.clone();
        action.connect_activate(move |_, v| {
            if let Some(v) = v.and_then(|v| v.get::<String>()) {
                f(v);
            }
        });
        let special = gio::SimpleAction::new("special", Some(glib::VariantTy::STRING));
        special.connect_activate(move |_, v| {
            if let Some(v) = v.and_then(|v| v.get::<String>()) {
                on_pick(v);
            }
        });
        let group = gio::SimpleActionGroup::new();
        group.add_action(&action);
        group.add_action(&special);
        let label = gtk::Label::new(None);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(28);
        label.set_xalign(0.0);
        let button = gtk::MenuButton::new();
        button.set_child(Some(&label));
        button.set_always_show_arrow(true);
        button.set_valign(gtk::Align::Center);
        button.insert_action_group("picker", Some(&group));
        Self {
            button,
            label,
            action,
        }
    }

    /// 項目と今の値を入れる。`special` は値でない項目（最後に区切って出す）。
    pub fn set(&self, options: &Options, special: &[(String, String)], current: &str) {
        let menu = gio::Menu::new();
        let mut shown = None;
        for (title, items) in options {
            let section = gio::Menu::new();
            for (value, text) in items {
                let item = gio::MenuItem::new(Some(text), None);
                item.set_action_and_target_value(Some("picker.pick"), Some(&value.to_variant()));
                section.append_item(&item);
                if value == current && shown.is_none() {
                    shown = Some(text.clone());
                }
            }
            menu.append_section(title.as_deref(), &section);
        }
        if !special.is_empty() {
            let section = gio::Menu::new();
            for (value, text) in special {
                let item = gio::MenuItem::new(Some(text), None);
                item.set_action_and_target_value(Some("picker.special"), Some(&value.to_variant()));
                section.append_item(&item);
            }
            menu.append_section(None, &section);
        }
        self.button.set_menu_model(Some(&menu));
        self.action.set_state(&current.to_variant());
        let text = shown.unwrap_or_else(|| current.to_owned());
        self.label.set_text(&text);
        self.button.set_tooltip_text(Some(&text));
    }
}

/// 行の部品（質問・欄・↩︎・設定元・ⓘ・説明・警告・エラー）。
pub(crate) struct Row {
    pub widget: gtk::Box,
    /// 欄を置く場所。
    pub control: gtk::Box,
    /// 行の下に足す部品（「その他…」の欄など）の置き場所。
    pub below: gtk::Box,
    pub error: gtk::Label,
    pub warning: gtk::Label,
    pub revert: gtk::Button,
    pub source: gtk::Label,
    pub info: gtk::MenuButton,
}

/// 行が読み書きするキー（設定元は先頭のキーのもの。↩︎ はすべての上書きを消す）。
pub(crate) type Keys = Rc<dyn Fn(&RepoSettings) -> Vec<String>>;
/// 生の値を人の言葉にする（ⓘ の「上書きしなければ」に使う）。
pub(crate) type Describe = Rc<dyn Fn(&RepoSettings, &str) -> String>;
/// 使えないときの理由（古い git など）。
pub(crate) type Disabled = Rc<dyn Fn(&RepoSettings) -> Option<String>>;

pub(crate) fn fixed_keys(keys: &[&str]) -> Keys {
    let keys: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
    Rc::new(move |_| keys.clone())
}

fn same() -> Describe {
    Rc::new(|_, v| v.to_owned())
}

fn on_off() -> Describe {
    Rc::new(|_, v| {
        if normalized_bool(v) == Some("true") {
            tr!("On")
        } else {
            tr!("Off")
        }
    })
}

impl RepoSettings {
    /// 部品を作って読み込む。`location` はリポジトリの場所（SSH 先かどうかを見る）。
    pub fn new(repo: Arc<Repository>, location: &str, level: Level) -> Rc<Self> {
        install_css();
        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        let error = error_label();
        error.set_margin_start(20);
        error.set_margin_end(20);
        error.set_margin_bottom(10);
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let this = Rc::new(Self {
            repo,
            level,
            host: host_of(location),
            state: RefCell::default(),
            refreshers: RefCell::default(),
            quiet: Cell::new(false),
            undo: RefCell::default(),
            redo: RefCell::default(),
            on_change: RefCell::default(),
            widget: widget.clone(),
            stack: stack.clone(),
            error: error.clone(),
            generation: Cell::new(0),
            signing_loaded: Cell::new(false),
            controls: RefCell::default(),
            writes: Cell::new(0),
        });

        // 左: ページの一覧と、その下にこのリポジトリを動かしている git。
        let sidebar = gtk::StackSidebar::new();
        sidebar.set_stack(&stack);
        sidebar.set_vexpand(true);
        let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left.set_size_request(200, -1);
        left.append(&sidebar);
        if level == Level::Repository {
            let git = gtk::Button::new();
            git.add_css_class("flat");
            let text = label("git ?", &["dim-label", "caption"]);
            git.set_child(Some(&text));
            git.set_margin_start(6);
            git.set_margin_end(6);
            git.set_margin_top(6);
            git.set_margin_bottom(6);
            git.set_tooltip_text(Some(&match &this.host {
                Some(host) => tr!(
                    "The git on {host} runs this repository. Every repository on that host uses it.",
                    host = host
                ),
                None => tr!("This computer’s git runs this repository. Choose it in Settings > Git."),
            }));
            git.connect_clicked(|b| {
                // その機械（この機械）の git を選ぶのは Settings > Git。
                if let Some(parent) = b
                    .root()
                    .and_downcast::<gtk::Window>()
                    .and_then(|w| w.transient_for())
                    .and_downcast::<gtk::ApplicationWindow>()
                {
                    crate::settings::show(&parent);
                    crate::settings::select("git");
                }
            });
            left.append(&git);
            this.on_refresh(move |s| {
                let st = s.state.borrow();
                let machine = s.host.clone().unwrap_or_else(|| tr!("This computer"));
                text.set_text(&format!(
                    "git {} · {machine}",
                    st.git_version.as_deref().unwrap_or("?")
                ));
            });
        }
        widget.append(&left);
        widget.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
        right.append(&stack);
        right.append(&error);
        widget.append(&right);

        for name in pages(level) {
            let child: gtk::Widget = match name {
                "author" => this.author_page().upcast(),
                "commits" => crate::repopages::commits_page(&this).upcast(),
                "operations" => this.operations_page().upcast(),
                "branches" => this.branches_page().upcast(),
                "workingCopy" => this.working_copy_page().upcast(),
                "remotes" => crate::repopages::remotes_page(&this).upcast(),
                "hooks" => crate::repopages::hooks_page(&this).upcast(),
                "config" => crate::configtable::page(&this).upcast(),
                _ => continue,
            };
            stack.add_titled(&child, Some(name), &page_title(name));
        }

        // 設定の書き換えの Undo・やり直し（ページを出している間。欄の中では欄の Undo が先に効く）。
        let shortcuts = gtk::ShortcutController::new();
        for (accel, redo) in [
            ("<Control>z", false),
            ("<Control><Shift>z", true),
            ("<Control>y", true),
        ] {
            let w = Rc::downgrade(&this);
            shortcuts.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(accel),
                Some(gtk::CallbackAction::new(move |_, _| {
                    if let Some(s) = w.upgrade() {
                        if redo { s.redo() } else { s.undo() }
                    }
                    glib::Propagation::Stop
                })),
            ));
        }
        widget.add_controller(shortcuts);
        this.load();
        this
    }

    /// 設定やリモートを変えたら呼ぶ（窓の History などを読み直す）。
    pub fn connect_change(&self, f: impl Fn() + 'static) {
        self.on_change.replace(Some(Rc::new(f)));
    }

    /// ページを選ぶ（`author` など）。
    pub fn select(&self, page: &str) {
        self.stack.set_visible_child_name(page);
    }

    pub(crate) fn weak(self: &Rc<Self>) -> Weak<Self> {
        Rc::downgrade(self)
    }

    /// 読み直したら `f` を呼ぶ（欄に今の値を入れる）。この間の欄の知らせでは書かない。
    pub(crate) fn on_refresh(self: &Rc<Self>, f: impl Fn(&Rc<RepoSettings>) + 'static) {
        self.refreshers.borrow_mut().push(Rc::new(f));
    }

    /// 欄に値を入れている途中か（その知らせでは書かない）。
    pub(crate) fn quiet(&self) -> bool {
        self.quiet.get()
    }

    fn refresh(self: &Rc<Self>) {
        let refreshers: Vec<Refresher> = self.refreshers.borrow().clone();
        self.quiet.set(true);
        for f in refreshers {
            f(self);
        }
        self.quiet.set(false);
    }

    /// 読み直す（開くたび、書いたあと）。
    pub fn load(self: &Rc<Self>) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let (repo, level, weak) = (self.repo.clone(), self.level, self.weak());
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || Loaded::read(&repo, level))
                .await
                .unwrap_or_else(|_| Err("panic".into()));
            let Some(s) = weak.upgrade() else { return };
            if s.generation.get() != generation {
                return;
            }
            match result {
                Ok(loaded) => s.apply(loaded),
                Err(e) => s.show_error(None, Some(&e)),
            }
        });
    }

    fn apply(self: &Rc<Self>, l: Loaded) {
        {
            let mut st = self.state.borrow_mut();
            st.entries = l.entries;
            st.known_keys = l.known_keys;
            st.remotes = l.remotes;
            st.gitignore = l.gitignore;
            st.exclude = l.exclude;
            st.excludes_path = l.excludes_path;
            st.hooks_path = l.hooks["hooksPath"].as_str().map(str::to_owned);
            st.hooks = l.hooks["hooks"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|h| Hook {
                            name: h["name"].as_str().unwrap_or_default().to_owned(),
                            enabled: h["enabled"].as_bool().unwrap_or(false),
                        })
                        .collect()
                })
                .unwrap_or_default();
            st.lfs = l.hooks["lfs"].as_bool().unwrap_or(false);
            st.lfs_patterns = strings(&l.hooks["lfsPatterns"]);
            st.lfs_files = l.hooks["lfsFiles"].as_u64();
            st.git_version = l.version["version"].as_str().map(str::to_owned);
            st.update_refs = l.version["updateRefs"].as_bool().unwrap_or(true);
            st.rerere_cache =
                self.level == Level::Repository && l.version["rerereCache"].as_bool() == Some(true);
            let mut branches: Vec<String> = l
                .groups
                .as_ref()
                .and_then(|g| g["branches"].as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|b| b["name"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            branches.sort();
            st.local_branches = branches;
            st.detected_base = l
                .groups
                .as_ref()
                .and_then(|g| g["base"].as_str().map(str::to_owned));
            st.commit_template = l.template.map(|t| {
                (
                    t["text"].as_str().map(str::to_owned),
                    t["path"].as_str().unwrap_or_default().to_owned(),
                )
            });
            st.loaded = true;
        }
        self.refresh();
        if !self.signing_loaded.replace(true) {
            for f in SIGNING_FORMATS {
                self.load_signing_keys(f);
            }
        }
    }

    /// `format` の鍵を集める（裏で。SSH 先のリポジトリなら SSH 先で）。集められなければ空。
    fn load_signing_keys(self: &Rc<Self>, format: &'static str) {
        let (repo, weak) = (self.repo.clone(), self.weak());
        glib::spawn_future_local(async move {
            let keys = run_one(&repo, json!({"op": "signingKeys", "format": format}))
                .await
                .ok()
                .and_then(|v| v["keys"].as_array().cloned())
                .unwrap_or_default();
            let Some(s) = weak.upgrade() else { return };
            let email = s
                .effective("user.email")
                .map(|e| e.value.to_lowercase())
                .unwrap_or_default();
            let mut keys: Vec<SigningKey> = keys
                .iter()
                .map(|k| SigningKey {
                    id: k["id"].as_str().unwrap_or_default().to_owned(),
                    label: k["label"].as_str().unwrap_or_default().to_owned(),
                    emails: strings(&k["emails"]),
                })
                .collect();
            // メールアドレスの合う鍵を先に（ほかは集めた順のまま）。
            keys.sort_by_key(|k| {
                email.is_empty() || !k.emails.iter().any(|e| e.to_lowercase() == email)
            });
            s.state
                .borrow_mut()
                .signing_keys
                .insert(format.to_owned(), keys);
            s.refresh();
        });
    }

    /// この段で書く scope。
    pub(crate) fn own_scope(&self) -> &'static str {
        match self.level {
            Level::Repository => "local",
            Level::AllRepositories => "global",
        }
    }

    /// 今効いている値（後に出るものほど優先）と、その設定元。
    pub(crate) fn effective(&self, key: &str) -> Option<Entry> {
        self.state
            .borrow()
            .entries
            .iter()
            .rev()
            .find(|e| e.key.eq_ignore_ascii_case(key))
            .cloned()
    }

    /// この段での値。
    pub(crate) fn local(&self, key: &str) -> Option<String> {
        let scope = self.own_scope();
        self.state
            .borrow()
            .entries
            .iter()
            .rev()
            .find(|e| e.key.eq_ignore_ascii_case(key) && e.scope == scope)
            .map(|e| e.value.clone())
    }

    /// この段以外で決まっている値（この段の値を消したときに効く値）。
    pub(crate) fn inherited(&self, key: &str) -> Option<Entry> {
        let scope = self.own_scope();
        self.state
            .borrow()
            .entries
            .iter()
            .rev()
            .find(|e| e.key.eq_ignore_ascii_case(key) && e.scope != scope)
            .cloned()
    }

    /// `key` の今効いている値の設定元の言い方と、そのファイル。どこにも書かれていなければ「git の既定」。
    pub(crate) fn origin(&self, key: &str) -> (String, Option<String>) {
        match self.effective(key) {
            Some(e) => (
                scope_label(&e.scope, self.host.as_deref()),
                Some(config_file(&e)),
            ),
            None => (tr!("git’s default"), None),
        }
    }

    /// この段の値を書き換える（`(キー, 値)`。値が None なら消す）。Undo に 1 段として積む。失敗したら
    /// `error`（なければページの下）に出す。
    pub(crate) fn write(
        self: &Rc<Self>,
        changes: Vec<(String, Option<String>)>,
        error: Option<&gtk::Label>,
    ) {
        let changes: Vec<Change> = changes
            .into_iter()
            .map(|(key, new)| Change {
                old: self.local(&key),
                key,
                new,
            })
            .filter(|c| c.old != c.new)
            .collect();
        if changes.is_empty() {
            return;
        }
        self.apply_changes(changes, error.cloned(), Record::New);
    }

    /// 書き換えを書く。`record` の向きに記録する（New なら Undo に積んでやり直しを捨てる、Undo ならやり直しに、
    /// Redo なら Undo に）。
    pub(crate) fn apply_changes(
        self: &Rc<Self>,
        changes: Vec<Change>,
        error: Option<gtk::Label>,
        record: Record,
    ) {
        let global = self.level == Level::AllRepositories;
        let commands: Vec<Value> = changes
            .iter()
            .map(|c| {
                let mut command = json!({"op": "configSet", "key": c.key, "value": c.new});
                if global {
                    command["scope"] = json!("global");
                }
                command
            })
            .collect();
        let (repo, weak) = (self.repo.clone(), self.weak());
        glib::spawn_future_local(async move {
            let result = run_all(&repo, commands).await;
            let Some(s) = weak.upgrade() else { return };
            match &result {
                Ok(()) => {
                    s.writes.set(s.writes.get() + 1);
                    // 記録は書けたものだけ（失敗した書き換えを Undo に残さない）。Undo で書いたものは、元に
                    // 戻した向き（もとの書き換え）をやり直しに積む。
                    match record {
                        Record::New => {
                            s.redo.borrow_mut().clear();
                            s.undo.borrow_mut().push(changes);
                        }
                        Record::Undo => s
                            .redo
                            .borrow_mut()
                            .push(changes.iter().map(Change::inverse).collect()),
                        Record::Redo => s.undo.borrow_mut().push(changes),
                    }
                    s.show_error(error.as_ref(), None);
                }
                Err(e) => s.show_error(error.as_ref(), Some(e)),
            }
            s.load();
            s.changed();
        });
    }

    /// 設定の書き換えを 1 段戻す。
    pub fn undo(self: &Rc<Self>) {
        let Some(group) = self.undo.borrow_mut().pop() else {
            return;
        };
        let inverse = group.iter().rev().map(Change::inverse).collect();
        self.apply_changes(inverse, None, Record::Undo);
    }

    /// 戻した書き換えをやり直す。
    pub fn redo(self: &Rc<Self>) {
        let Some(group) = self.redo.borrow_mut().pop() else {
            return;
        };
        self.apply_changes(group, None, Record::Redo);
    }

    /// 書けた書き換えの数。
    pub(crate) fn writes(&self) -> u64 {
        self.writes.get()
    }

    /// 窓に知らせる（History などを読み直す）。
    pub(crate) fn changed(&self) {
        let f = self.on_change.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }

    /// エラーを出す（`message` が None なら消す）。`row` がなければページの下に。
    pub(crate) fn show_error(&self, row: Option<&gtk::Label>, message: Option<&str>) {
        let target = row.unwrap_or(&self.error);
        target.set_text(message.unwrap_or_default());
        target.set_visible(message.is_some());
        if row.is_some() && message.is_none() {
            self.error.set_visible(false);
        }
    }

    /// 命令を順に実行し（リモート・フックなど、設定の書き換えでないもの。Undo には積まない）、読み直す。
    /// 失敗したらページの下に出す。できたら `then`。
    pub(crate) fn perform(
        self: &Rc<Self>,
        commands: Vec<Value>,
        then: impl FnOnce(&Rc<RepoSettings>) + 'static,
    ) {
        let (repo, weak) = (self.repo.clone(), self.weak());
        glib::spawn_future_local(async move {
            let result = run_all(&repo, commands).await;
            let Some(s) = weak.upgrade() else { return };
            s.show_error(None, result.as_ref().err().map(String::as_str));
            s.load();
            s.changed();
            if result.is_ok() {
                then(&s);
            }
        });
    }

    /// 部品の窓（シートの親）。
    pub(crate) fn window(&self) -> Option<gtk::Window> {
        self.widget.root().and_downcast()
    }

    // MARK: 行の部品

    /// 行の共通の形を作る。欄は `row.control` に入れる。
    pub(crate) fn row(
        self: &Rc<Self>,
        question: &str,
        help: &str,
        keys: Keys,
        describe: Describe,
        disabled: Option<Disabled>,
    ) -> Row {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 4);
        widget.set_margin_top(4);
        widget.set_margin_bottom(4);
        widget.set_margin_start(4);
        widget.set_margin_end(4);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let q = label(question, &[]);
        q.set_hexpand(true);
        q.set_wrap(true);
        q.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        line.append(&q);
        let control = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        control.set_valign(gtk::Align::Center);
        line.append(&control);
        // アイコンのテーマに頼らない（macOS の GTK などは GNOME のアイコンを持たない）。
        let revert = gtk::Button::new();
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Undo, false);
        revert.set_child(Some(&icon));
        revert.add_css_class("flat");
        revert.set_valign(gtk::Align::Center);
        revert.set_tooltip_text(Some(&tr!(
            "Stop overriding here. The value above takes over."
        )));
        line.append(&revert);
        let source = label("", &["dim-label"]);
        source.set_xalign(1.0);
        source.set_size_request(150, -1);
        source.set_ellipsize(gtk::pango::EllipsizeMode::End);
        line.append(&source);
        let info = gtk::MenuButton::new();
        info.set_child(Some(&gtk::Label::new(Some("ⓘ"))));
        info.set_has_frame(false);
        info.set_valign(gtk::Align::Center);
        info.set_tooltip_text(Some(&tr!("Where the value comes from")));
        let popover = gtk::Popover::new();
        info.set_popover(Some(&popover));
        line.append(&info);
        widget.append(&line);
        widget.append(&help_label(help));
        let warning = label("", &["settings-warning"]);
        warning.set_wrap(true);
        warning.set_margin_start(8);
        warning.set_visible(false);
        widget.append(&warning);
        let below = gtk::Box::new(gtk::Orientation::Vertical, 4);
        widget.append(&below);
        let error = error_label();
        widget.append(&error);

        self.controls
            .borrow_mut()
            .push((question.to_owned(), control.clone(), revert.clone()));
        let w = self.weak();
        let k = keys.clone();
        let e = error.clone();
        revert.connect_clicked(move |_| {
            if let Some(s) = w.upgrade() {
                let keys: Vec<(String, Option<String>)> = k(&s)
                    .into_iter()
                    .filter(|key| s.local(key).is_some())
                    .map(|key| (key, None))
                    .collect();
                s.write(keys, Some(&e));
            }
        });
        let (rv, src, c, warn) = (
            revert.clone(),
            source.clone(),
            control.clone(),
            warning.clone(),
        );
        self.on_refresh(move |s| {
            let keys = keys(s);
            let overriding = keys.iter().any(|k| s.local(k).is_some());
            // ↩︎ は上書きしている間だけ（場所は取っておく）。
            rv.set_opacity(if overriding { 1.0 } else { 0.0 });
            rv.set_can_target(overriding);
            rv.set_can_focus(overriding);
            let (text, file) = s.origin(&keys[0]);
            src.set_text(&text);
            src.set_tooltip_text(Some(&file.unwrap_or_else(|| {
                tr!("Nothing is set anywhere, so git’s own default applies.")
            })));
            popover.set_child(Some(&s.details(&keys, &describe)));
            let reason = disabled.as_ref().and_then(|d| d(s));
            c.set_sensitive(reason.is_none());
            warn.set_text(
                &reason
                    .as_ref()
                    .map(|r| format!("⚠ {r}"))
                    .unwrap_or_default(),
            );
            warn.set_visible(reason.is_some());
        });
        Row {
            widget,
            control,
            below,
            error,
            warning,
            revert,
            source,
            info,
        }
    }

    /// ⓘ の吹き出しの中身: キーごとに `key = value`、設定元とファイル、上書きしなければ効く値。
    fn details(&self, keys: &[String], describe: &Describe) -> gtk::Box {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
        b.set_margin_top(6);
        b.set_margin_bottom(6);
        b.set_margin_start(6);
        b.set_margin_end(6);
        for key in keys {
            let item = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let mono = |text: &str| {
                let l = label(text, &["monospace"]);
                l.set_selectable(true);
                l
            };
            let dim = |text: &str| {
                let l = label(text, &["dim-label"]);
                l.set_selectable(true);
                l.set_wrap(true);
                l.set_max_width_chars(60);
                l
            };
            match self.effective(key) {
                Some(e) => {
                    item.append(&mono(&format!("{key} = {}", e.value)));
                    let (source, file) = self.origin(key);
                    item.append(&dim(&match file {
                        Some(file) => tr!("{source} ({file})", source = source, file = file),
                        None => source,
                    }));
                }
                None => {
                    item.append(&mono(&tr!("{key} is not set", key = key)));
                    item.append(&dim(&tr!("git’s default")));
                }
            }
            if self.local(key).is_some()
                && let Some(above) = self.inherited(key)
            {
                item.append(&dim(&tr!(
                    "Without the override here: {value}",
                    value = describe(self, &above.value)
                )));
            }
            b.append(&item);
        }
        b
    }

    /// 選ぶ設定（欄には効いている値。どこにもなければ git の既定）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn choice_row(
        self: &Rc<Self>,
        question: &str,
        help: &str,
        key: &'static str,
        options: Vec<(&'static str, String)>,
        default: &'static str,
        is_bool: bool,
        disabled: Option<Disabled>,
    ) -> gtk::Widget {
        let options = Rc::new(options);
        let normalize = move |value: &str| -> String {
            if is_bool {
                normalized_bool(value).map_or_else(|| value.to_owned(), str::to_owned)
            } else {
                value.to_lowercase()
            }
        };
        let o = options.clone();
        let describe: Describe = Rc::new(move |_, v| {
            let v = normalize(v);
            o.iter()
                .find(|(value, _)| *value == v)
                .map_or(v, |(_, l)| l.clone())
        });
        let row = self.row(question, help, fixed_keys(&[key]), describe, disabled);
        let (w, e) = (self.weak(), row.error.clone());
        let picker = Picker::new(move |value| {
            if let Some(s) = w.upgrade()
                && !s.quiet()
            {
                let current = s
                    .effective(key)
                    .map_or_else(|| default.to_owned(), |e| normalize(&e.value));
                if value != current {
                    s.write(vec![(key.to_owned(), Some(value))], Some(&e));
                }
            }
        });
        row.control.append(&picker.button);
        self.on_refresh(move |s| {
            let current = s
                .effective(key)
                .map_or_else(|| default.to_owned(), |e| normalize(&e.value));
            let mut items: Vec<(String, String)> = options
                .iter()
                .map(|(v, l)| ((*v).to_owned(), l.clone()))
                .collect();
            if !items.iter().any(|(v, _)| *v == current) {
                items.push((current.clone(), current.clone()));
            }
            picker.set(&vec![(None, items)], &[], &current);
        });
        row.widget.upcast()
    }

    /// はい／いいえの設定。
    pub(crate) fn switch_row(
        self: &Rc<Self>,
        question: &str,
        help: &str,
        key: &'static str,
        default_on: Rc<dyn Fn(&RepoSettings) -> bool>,
        disabled: Option<Disabled>,
    ) -> gtk::Widget {
        let row = self.row(question, help, fixed_keys(&[key]), on_off(), disabled);
        let switch = gtk::Switch::new();
        switch.set_valign(gtk::Align::Center);
        row.control.append(&switch);
        let (w, e) = (self.weak(), row.error.clone());
        switch.connect_active_notify(move |sw| {
            if let Some(s) = w.upgrade()
                && !s.quiet()
            {
                s.write(
                    vec![(key.to_owned(), Some(sw.is_active().to_string()))],
                    Some(&e),
                );
            }
        });
        self.on_refresh(move |s| {
            let on = s.effective(key).map_or_else(
                || default_on(s),
                |e| normalized_bool(&e.value) == Some("true"),
            );
            switch.set_active(on);
        });
        row.widget.upcast()
    }

    /// 文字の設定。Return か欄から離れたときに書く。空にすると上書きを消す。
    pub(crate) fn text_row(self: &Rc<Self>, question: &str, key: &'static str) -> gtk::Widget {
        let row = self.row(question, "", fixed_keys(&[key]), same(), None);
        let entry = gtk::Entry::new();
        entry.set_width_chars(30);
        entry.set_valign(gtk::Align::Center);
        row.control.append(&entry);
        let commit = {
            let (w, e) = (self.weak(), row.error.clone());
            Rc::new(move |entry: &gtk::Entry| {
                if let Some(s) = w.upgrade()
                    && !s.quiet()
                {
                    let value = entry.text().trim().to_owned();
                    if value != s.local(key).unwrap_or_default() {
                        s.write(
                            vec![(key.to_owned(), (!value.is_empty()).then_some(value))],
                            Some(&e),
                        );
                    }
                }
            })
        };
        let c = commit.clone();
        entry.connect_activate(move |entry| c(entry));
        let focus = gtk::EventControllerFocus::new();
        let en = entry.clone();
        focus.connect_leave(move |_| commit(&en));
        entry.add_controller(focus);
        self.on_refresh(move |s| {
            entry.set_placeholder_text(s.effective(key).map(|e| e.value).as_deref());
            let local = s.local(key).unwrap_or_default();
            if entry.text() != local {
                entry.set_text(&local);
            }
        });
        row.widget.upcast()
    }

    // MARK: ページ

    /// Pull・Fetch・Rebase・Merge の設定。
    fn operations_page(self: &Rc<Self>) -> gtk::ScrolledWindow {
        let off: Rc<dyn Fn(&RepoSettings) -> bool> = Rc::new(|_| false);
        let too_old: Disabled = Rc::new(|s| {
            let st = s.state.borrow();
            (!st.update_refs).then(|| {
                tr!(
                    "Needs git 2.38 or later. This repository’s git is {version}{host}.",
                    version = st.git_version.as_deref().unwrap_or("?"),
                    host = s
                        .host
                        .as_ref()
                        .map(|h| format!(" ({h})"))
                        .unwrap_or_default()
                )
            })
        });
        page(&[
            section(Some("Pull"), &[self.pull_row()]),
            section(
                Some("Fetch"),
                &[self.switch_row(
                    &tr!("Remove remote-tracking branches deleted on the remote"),
                    "",
                    "fetch.prune",
                    off.clone(),
                    None,
                )],
            ),
            section(
                Some("Rebase"),
                &[
                    self.switch_row(
                        &tr!("Stash changes before rebasing"),
                        &tr!("History edits always stash, whatever this says."),
                        "rebase.autoStash",
                        off.clone(),
                        None,
                    ),
                    self.choice_row(
                        &tr!("Move stacked branches along when rebasing or editing History"),
                        &tr!("Branches whose tips are among the rewritten commits. Undo puts them back."),
                        "rebase.updateRefs",
                        vec![("false", tr!("Leave them")), ("true", tr!("Move them along"))],
                        "false",
                        true,
                        Some(too_old),
                    ),
                ],
            ),
            section(
                Some("Merge"),
                &[
                    self.choice_row(
                        &tr!("How conflicts are written in files"),
                        &tr!("With the base (diff3, zdiff3), the merge editor resolves one-sided conflicts by itself."),
                        "merge.conflictStyle",
                        vec![
                            ("merge", tr!("Both sides")),
                            ("diff3", tr!("Both sides and the common base")),
                            ("zdiff3", tr!("Both sides and the base, shortened")),
                        ],
                        "merge",
                        false,
                        None,
                    ),
                    self.switch_row(
                        &tr!("Reuse how conflicts were resolved before"),
                        &tr!("When the same conflict comes back, git puts the earlier resolution in the file (rerere)."),
                        "rerere.enabled",
                        Rc::new(|s| s.state.borrow().rerere_cache),
                        None,
                    ),
                    self.switch_row(
                        &tr!("Stage files resolved that way"),
                        &tr!("Only while the switch above is on."),
                        "rerere.autoUpdate",
                        off,
                        None,
                    ),
                ],
            ),
        ])
    }

    /// pull のとき、どう取り込むか。`pull.rebase` と `pull.ff` の 2 つで決まる（コアの `syncDefaults` と同じ読み方）。
    fn pull_row(self: &Rc<Self>) -> gtk::Widget {
        fn method(s: &RepoSettings) -> &'static str {
            if s.effective("pull.ff")
                .is_some_and(|e| e.value.eq_ignore_ascii_case("only"))
            {
                return "ffOnly";
            }
            match s.effective("pull.rebase").map(|e| e.value.to_lowercase()) {
                Some(r)
                    if ["true", "yes", "on", "1", "merges", "interactive"]
                        .contains(&r.as_str()) =>
                {
                    "rebase"
                }
                _ => "merge",
            }
        }
        // 設定元に出すのは、取り込み方を決めているほうのキー。
        let keys: Keys = Rc::new(|s| {
            if s.effective("pull.ff")
                .is_some_and(|e| e.value.eq_ignore_ascii_case("only"))
            {
                vec!["pull.ff".into(), "pull.rebase".into()]
            } else {
                vec!["pull.rebase".into(), "pull.ff".into()]
            }
        });
        let describe: Describe = Rc::new(|_, v| match v.to_lowercase().as_str() {
            "only" => tr!("Fast-forward"),
            v if ["true", "yes", "on", "1", "merges", "interactive"].contains(&v) => tr!("Rebase"),
            _ => tr!("Merge"),
        });
        let row = self.row(
            &tr!("When pulling, how are the changes brought in?"),
            "",
            keys,
            describe,
            None,
        );
        let (w, e) = (self.weak(), row.error.clone());
        let picker = Picker::new(move |m| {
            let Some(s) = w.upgrade() else { return };
            if s.quiet() || m == method(&s) {
                return;
            }
            // fast-forward だけ → pull.ff=only。それ以外は pull.ff の only を消してから pull.rebase を書く。
            // pull.rebase=true が残っていると、どちらが勝つかが git の版で変わるので、fast-forward でも
            // pull.rebase=false を書いて曖昧さをなくす。
            let mut changes = Vec::new();
            if s.local("pull.ff")
                .is_some_and(|v| v.eq_ignore_ascii_case("only"))
            {
                changes.push(("pull.ff".to_owned(), None));
            }
            match m.as_str() {
                "rebase" => changes.push(("pull.rebase".into(), Some("true".into()))),
                "ffOnly" => {
                    changes.push(("pull.rebase".into(), Some("false".into())));
                    changes.retain(|(k, _)| k != "pull.ff");
                    changes.push(("pull.ff".into(), Some("only".into())));
                }
                _ => changes.push(("pull.rebase".into(), Some("false".into()))),
            }
            s.write(changes, Some(&e));
        });
        row.control.append(&picker.button);
        self.on_refresh(move |s| {
            let items = vec![
                ("merge".to_owned(), tr!("Merge")),
                ("rebase".to_owned(), tr!("Rebase")),
                ("ffOnly".to_owned(), tr!("Fast-forward")),
            ];
            picker.set(&vec![(None, items)], &[], method(s));
        });
        row.widget.upcast()
    }

    /// 改行・実行の許可と、このリポジトリだけで無視するパターン。
    fn working_copy_page(self: &Rc<Self>) -> gtk::Box {
        let rows = section(
            None,
            &[
                self.choice_row(
                    &tr!("Line endings"),
                    "",
                    "core.autocrlf",
                    vec![
                        ("false", tr!("Keep as committed")),
                        ("true", tr!("CRLF in the working tree, LF in commits")),
                        ("input", tr!("LF in commits, no change on checkout")),
                    ],
                    "false",
                    false,
                    None,
                ),
                self.switch_row(
                    &tr!("Track changes to the executable bit"),
                    "",
                    "core.fileMode",
                    Rc::new(|_| true),
                    None,
                ),
            ],
        );
        let repository = self.level == Level::Repository;
        let heading = label(
            &if repository {
                tr!("Ignore in this repository only")
            } else {
                tr!("Ignore everywhere")
            },
            &["heading"],
        );
        let note = help_label(&if repository {
            tr!(".git/info/exclude, one pattern per line. Shared patterns go in .gitignore.")
        } else {
            String::new()
        });
        note.set_margin_start(0);
        let editor = gtk::TextView::new();
        editor.set_monospace(true);
        editor.set_top_margin(6);
        editor.set_bottom_margin(6);
        editor.set_left_margin(6);
        editor.set_right_margin(6);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&editor)
            .vexpand(true)
            .min_content_height(160)
            .build();
        let frame = gtk::Frame::new(None);
        frame.set_child(Some(&scrolled));
        let revert = gtk::Button::with_label(&tr!("Revert"));
        let save = gtk::Button::with_label(&tr!("Save"));
        save.add_css_class("suggested-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&revert);
        buttons.append(&save);
        let error = error_label();
        error.set_margin_start(0);

        // 書きかけかどうか（読んだ中身と比べる）。
        let saved = Rc::new(RefCell::new(String::new()));
        let buffer = editor.buffer();
        let dirty = {
            let (saved, revert, save) = (saved.clone(), revert.clone(), save.clone());
            move |b: &gtk::TextBuffer| {
                let text = b.text(&b.start_iter(), &b.end_iter(), false);
                let changed = text.as_str() != saved.borrow().as_str();
                revert.set_sensitive(changed);
                save.set_sensitive(changed);
            }
        };
        let d = dirty.clone();
        buffer.connect_changed(move |b| d(b));
        let do_save = {
            let (w, buffer, e) = (self.weak(), buffer.clone(), error.clone());
            Rc::new(move || {
                let Some(s) = w.upgrade() else { return };
                let text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
                let command = if s.level == Level::AllRepositories {
                    json!({"op": "saveExcludesFile", "text": text})
                } else {
                    let gitignore = s.state.borrow().gitignore.clone();
                    json!({"op": "saveIgnoreFiles", "gitignore": gitignore, "exclude": text})
                };
                let (w, e) = (s.weak(), e.clone());
                glib::spawn_future_local(async move {
                    let Some(s) = w.upgrade() else { return };
                    let result = run_all(&s.repo, vec![command]).await;
                    s.show_error(Some(&e), result.as_ref().err().map(String::as_str));
                    s.load();
                    s.changed();
                });
            })
        };
        let f = do_save.clone();
        save.connect_clicked(move |_| f());
        let shortcuts = gtk::ShortcutController::new();
        let f = do_save.clone();
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>s"),
            Some(gtk::CallbackAction::new(move |_, _| {
                f();
                glib::Propagation::Stop
            })),
        ));
        editor.add_controller(shortcuts);
        let (s2, b2) = (saved.clone(), buffer.clone());
        revert.connect_clicked(move |_| b2.set_text(&s2.borrow()));
        let n = note.clone();
        self.on_refresh(move |s| {
            let st = s.state.borrow();
            if s.level == Level::AllRepositories {
                n.set_text(&tilde(&st.excludes_path));
                n.set_visible(!st.excludes_path.is_empty());
            }
            // 書きかけでなければ、読んだ中身に入れ替える。
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            if text.as_str() == saved.borrow().as_str() && text.as_str() != st.exclude {
                buffer.set_text(&st.exclude);
            }
            saved.replace(st.exclude.clone());
            dirty(&buffer);
        });

        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.set_vexpand(true);
        column.append(&heading);
        column.append(&note);
        column.append(&frame);
        column.append(&buttons);
        column.append(&error);
        let b = gtk::Box::new(gtk::Orientation::Vertical, 18);
        b.set_margin_top(18);
        b.set_margin_bottom(18);
        b.set_margin_start(20);
        b.set_margin_end(20);
        b.append(&rows);
        b.append(&column);
        b
    }

    /// 既定のブランチ（Amon Hen の `amonhen.defaultBranch`。なければ判定したもの）。
    fn branches_page(self: &Rc<Self>) -> gtk::ScrolledWindow {
        const KEY: &str = "amonhen.defaultBranch";
        let row = self.row(
            &tr!("Default branch"),
            &tr!("Branches measures the others against it. Detected from origin/HEAD, main or master; choose one when there is none."),
            fixed_keys(&[KEY]),
            same(),
            None,
        );
        // ⓘ はない（Amon Hen のキーで、設定元はこのリポジトリか判定したものだけ）。場所は取っておく。
        row.info.set_opacity(0.0);
        row.info.set_can_target(false);
        row.info.set_can_focus(false);
        row.revert
            .set_tooltip_text(Some(&tr!("Go back to the detected branch.")));
        let (w, e) = (self.weak(), row.error.clone());
        let picker = Picker::new(move |value| {
            if let Some(s) = w.upgrade()
                && !s.quiet()
                && value != s.local(KEY).unwrap_or_default()
            {
                s.write(
                    vec![(KEY.to_owned(), (!value.is_empty()).then_some(value))],
                    Some(&e),
                );
            }
        });
        row.control.append(&picker.button);
        let (source, warning) = (row.source.clone(), row.warning.clone());
        self.on_refresh(move |s| {
            let st = s.state.borrow();
            let chosen = s.local(KEY);
            let mut items = vec![(
                String::new(),
                match &st.detected_base {
                    Some(b) => tr!("Detected ({branch})", branch = b),
                    None => tr!("Detected (none)"),
                },
            )];
            items.extend(st.local_branches.iter().map(|b| (b.clone(), b.clone())));
            if let Some(c) = &chosen
                && !st.local_branches.contains(c)
            {
                items.push((c.clone(), c.clone()));
            }
            picker.set(&vec![(None, items)], &[], chosen.as_deref().unwrap_or_default());
            source.set_text(&if chosen.is_none() {
                tr!("detected")
            } else {
                tr!("this repository")
            });
            source.set_tooltip_text(None);
            let missing = chosen
                .as_ref()
                .filter(|c| !st.local_branches.contains(c));
            warning.set_text(
                &missing
                    .map(|c| {
                        format!(
                            "⚠ {}",
                            tr!(
                                "There is no branch named {branch}, so the detected branch is used.",
                                branch = c
                            )
                        )
                    })
                    .unwrap_or_default(),
            );
            warning.set_visible(missing.is_some());
        });
        page(&[section(None, &[row.widget.upcast()])])
    }

    /// 作者と署名、mailmap。
    fn author_page(self: &Rc<Self>) -> gtk::ScrolledWindow {
        page(&[
            section(
                Some(&tr!("Author")),
                &[
                    self.text_row(&tr!("Name"), "user.name"),
                    self.text_row(&tr!("Email"), "user.email"),
                ],
            ),
            section(
                Some(&tr!("Signing")),
                &[self.signing_row(), self.signing_key_row()],
            ),
            section(
                Some("mailmap"),
                &[
                    self.switch_row(
                        &tr!("Use the mailmap"),
                        &tr!("Shows the names and emails from the mailmap instead of the recorded ones, like git log (Blame always does)."),
                        "log.mailmap",
                        Rc::new(|_| true),
                        None,
                    ),
                    self.mailmap_file_row(),
                ],
            ),
        ])
    }

    /// 今の署名の形式（`gpg.format`。知らないものは OpenPGP）。
    fn signing_format(&self) -> &'static str {
        let f = self
            .effective("gpg.format")
            .map(|e| e.value.to_lowercase())
            .unwrap_or_default();
        SIGNING_FORMATS
            .iter()
            .find(|x| **x == f)
            .copied()
            .unwrap_or("openpgp")
    }

    /// 署名: 1 つの選択（署名しない／OpenPGP／SSH／X.509）で、`commit.gpgsign` と `gpg.format` を一緒に書く。
    fn signing_row(self: &Rc<Self>) -> gtk::Widget {
        let label_of = |v: &str| {
            if v.is_empty() {
                tr!("No signing")
            } else {
                format_name(v).to_owned()
            }
        };
        let describe: Describe = Rc::new(move |_, v| match normalized_bool(v) {
            Some("true") => tr!("On"),
            Some("false") => tr!("Off"),
            _ => label_of(&v.to_lowercase()),
        });
        let current = |s: &RepoSettings| -> String {
            let signing = s
                .effective("commit.gpgsign")
                .is_some_and(|e| normalized_bool(&e.value) == Some("true"));
            if signing {
                s.signing_format().to_owned()
            } else {
                String::new()
            }
        };
        let row = self.row(
            &tr!("Signing"),
            "",
            fixed_keys(&["commit.gpgsign", "gpg.format"]),
            describe,
            None,
        );
        let (w, e) = (self.weak(), row.error.clone());
        let picker = Picker::new(move |value| {
            let Some(s) = w.upgrade() else { return };
            if s.quiet() || value == current(&s) {
                return;
            }
            let mut changes = Vec::new();
            if value.is_empty() {
                changes.push(("commit.gpgsign".to_owned(), Some("false".to_owned())));
            } else {
                changes.push(("commit.gpgsign".to_owned(), Some("true".to_owned())));
                if s.signing_format() != value || s.local("gpg.format").is_some() {
                    changes.push(("gpg.format".to_owned(), Some(value)));
                }
            }
            s.write(changes, Some(&e));
        });
        row.control.append(&picker.button);
        self.on_refresh(move |s| {
            let items = std::iter::once(String::new())
                .chain(SIGNING_FORMATS.iter().map(|f| (*f).to_owned()))
                .map(|v| (v.clone(), label_of(&v)))
                .collect();
            picker.set(&vec![(None, items)], &[], &current(s));
        });
        row.widget.upcast()
    }

    /// 鍵: コアが集めた鍵（形式ごと。`user.email` の鍵が先）から選ぶ。鍵を選ぶとその形式を `gpg.format` にも
    /// 書く。一覧にないものは「その他…」で入れる。
    fn signing_key_row(self: &Rc<Self>) -> gtk::Widget {
        const KEY: &str = "user.signingkey";
        const OTHER: &str = "other";
        let kind_of = |s: &RepoSettings, id: &str| -> Option<&'static str> {
            let st = s.state.borrow();
            SIGNING_FORMATS.iter().copied().find(|f| {
                st.signing_keys
                    .get(*f)
                    .is_some_and(|keys| keys.iter().any(|k| k.id == id))
            })
        };
        let label_of = move |s: &RepoSettings, value: &str| -> String {
            if value.is_empty() {
                return if s.signing_format() == "openpgp" {
                    tr!("Key for your email")
                } else {
                    tr!("None")
                };
            }
            kind_of(s, value)
                .and_then(|f| {
                    s.state.borrow().signing_keys[f]
                        .iter()
                        .find(|k| k.id == value)
                        .map(|k| k.label.clone())
                })
                .unwrap_or_else(|| value.to_owned())
        };
        let row = self.row(
            &tr!("Key"),
            "",
            fixed_keys(&[KEY]),
            Rc::new(move |s, v| label_of(s, v)),
            None,
        );
        // 「その他…」の欄（行の下に出す）。
        let entry = gtk::Entry::new();
        entry.set_visible(false);
        entry.set_margin_start(8);
        row.below.append(&entry);
        let commit = {
            let (w, e) = (self.weak(), row.error.clone());
            move |entry: &gtk::Entry| {
                let Some(s) = w.upgrade() else { return };
                entry.set_visible(false);
                let value = entry.text().trim().to_owned();
                let current = s.effective(KEY).map(|e| e.value).unwrap_or_default();
                if value != current {
                    s.write(
                        vec![(KEY.to_owned(), (!value.is_empty()).then_some(value))],
                        Some(&e),
                    );
                }
            }
        };
        entry.connect_activate(commit);
        let keys = gtk::EventControllerKey::new();
        let en = entry.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                en.set_visible(false);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        entry.add_controller(keys);
        let (w, e, en) = (self.weak(), row.error.clone(), entry.clone());
        let picker = Picker::new(move |value| {
            let Some(s) = w.upgrade() else { return };
            if s.quiet() {
                return;
            }
            if value == OTHER {
                en.set_text(&s.effective(KEY).map(|e| e.value).unwrap_or_default());
                en.set_placeholder_text(Some(&if s.signing_format() == "ssh" {
                    "~/.ssh/id_ed25519.pub".to_owned()
                } else {
                    tr!("Fingerprint or key ID")
                }));
                en.set_visible(true);
                en.grab_focus();
                return;
            }
            en.set_visible(false);
            let mut changes = vec![(KEY.to_owned(), (!value.is_empty()).then(|| value.clone()))];
            // 選んだ鍵の形式が今の形式と違えば、形式も合わせる。
            if let Some(f) = kind_of(&s, &value)
                && f != s.signing_format()
            {
                changes.push(("gpg.format".to_owned(), Some(f.to_owned())));
            }
            s.write(changes, Some(&e));
        });
        row.control.append(&picker.button);
        let warning = row.warning.clone();
        self.on_refresh(move |s| {
            let current = s.effective(KEY).map(|e| e.value).unwrap_or_default();
            let mut options: Options = vec![(None, vec![(String::new(), label_of(s, ""))])];
            {
                let st = s.state.borrow();
                for f in SIGNING_FORMATS {
                    if let Some(keys) = st.signing_keys.get(*f).filter(|k| !k.is_empty()) {
                        options.push((
                            Some(format_name(f).to_owned()),
                            keys.iter()
                                .map(|k| (k.id.clone(), k.label.clone()))
                                .collect(),
                        ));
                    }
                }
            }
            if !current.is_empty() && kind_of(s, &current).is_none() {
                options.push((None, vec![(current.clone(), current.clone())]));
            }
            picker.set(&options, &[(OTHER.to_owned(), tr!("Other…"))], &current);
            let format = s.signing_format();
            let mismatch = kind_of(s, &current).filter(|k| *k != format);
            warning.set_text(
                &mismatch
                    .map(|k| {
                        format!(
                            "⚠ {}",
                            tr!(
                                "This is an {kind} key, but the format is {format}.",
                                kind = format_name(k),
                                format = format_name(format)
                            )
                        )
                    })
                    .unwrap_or_default(),
            );
            warning.set_visible(mismatch.is_some());
        });
        row.widget.upcast()
    }

    /// mailmap のファイル（`mailmap.file`）。リポジトリの `.mailmap` の後に読む。git が読むファイルなので、SSH 先の
    /// リポジトリなら SSH 先のパスを入れる。
    fn mailmap_file_row(self: &Rc<Self>) -> gtk::Widget {
        const KEY: &str = "mailmap.file";
        const PICK: &str = "file";
        let label_of = |path: &str| {
            if path.is_empty() {
                tr!("None")
            } else {
                tr!("File: {path}", path = tilde(path))
            }
        };
        let row = self.row(
            &tr!("Mailmap file"),
            &tr!("Read after the repository’s .mailmap."),
            fixed_keys(&[KEY]),
            Rc::new(move |_, v| label_of(v)),
            None,
        );
        let (w, e) = (self.weak(), row.error.clone());
        let picker = Picker::new(move |value| {
            let Some(s) = w.upgrade() else { return };
            let current = s.effective(KEY).map(|e| e.value).unwrap_or_default();
            if s.quiet() || value == current {
                return;
            }
            if value == PICK {
                let initial = if current.is_empty() {
                    "~".to_owned()
                } else {
                    current
                };
                let (w, e) = (s.weak(), e.clone());
                s.choose_file(
                    &tr!("Mailmap file"),
                    &tr!("Choose a mailmap file"),
                    initial,
                    move |path| {
                        if let Some(s) = w.upgrade() {
                            s.write(vec![(KEY.to_owned(), Some(path))], Some(&e));
                        }
                    },
                );
            } else if value.is_empty() {
                // 上の段で決めたものを消すことはできないので、ここでは上書きを消す（上の段にあればそれが効く）。
                s.write(vec![(KEY.to_owned(), None)], Some(&e));
            }
        });
        row.control.append(&picker.button);
        self.on_refresh(move |s| {
            let current = s.effective(KEY).map(|e| e.value).unwrap_or_default();
            let mut items = vec![(String::new(), tr!("None"))];
            if !current.is_empty() {
                items.push((current.clone(), label_of(&current)));
            }
            picker.set(
                &vec![(None, items)],
                &[(PICK.to_owned(), tr!("A file…"))],
                &current,
            );
        });
        row.widget.upcast()
    }

    /// ファイルを選んでもらう。手元のリポジトリならファイルの選択の窓、SSH 先のリポジトリなら SSH 先のパスを
    /// 入れてもらう（git が読むファイルなので、その機械のもの）。
    pub(crate) fn choose_file(
        &self,
        title: &str,
        message: &str,
        initial: String,
        done: impl FnOnce(String) + 'static,
    ) {
        let parent = self.window();
        if let Some(host) = self.host.clone() {
            let (title, message) = (title.to_owned(), message.to_owned());
            glib::spawn_future_local(async move {
                let entry = gtk::Entry::builder()
                    .text(&initial)
                    .activates_default(true)
                    .hexpand(true)
                    .build();
                let sheet = crate::sheets::Sheet::new(
                    parent.as_ref(),
                    &title,
                    &tr!(
                        "{message} (a path on {host})",
                        message = message,
                        host = host
                    ),
                );
                sheet.add(&entry);
                sheet.buttons(&tr!("Choose"), false, None);
                if sheet.run().await == crate::sheets::Response::Ok {
                    let path = entry.text().trim().to_owned();
                    if !path.is_empty() {
                        done(path);
                    }
                }
            });
            return;
        }
        let dialog = gtk::FileDialog::builder().title(title).modal(true).build();
        let start = if initial == "~" || initial.is_empty() {
            glib::home_dir()
        } else {
            std::path::PathBuf::from(initial.replacen('~', &glib::home_dir().to_string_lossy(), 1))
        };
        if start.is_dir() {
            dialog.set_initial_folder(Some(&gio::File::for_path(&start)));
        } else if start.is_file() {
            dialog.set_initial_file(Some(&gio::File::for_path(&start)));
        }
        glib::spawn_future_local(async move {
            if let Ok(file) = dialog.open_future(parent.as_ref()).await
                && let Some(path) = file.path()
            {
                done(path.to_string_lossy().into_owned());
            }
        });
    }
}

/// 確認用の手順（`AMONHEN_SETTINGS_SCRIPT`）を終えるまでの時間（`--snapshot` はそれを待つ）。
pub fn script_millis() -> u64 {
    std::env::var("AMONHEN_SETTINGS_SCRIPT")
        .map(|s| 800 * (s.split(';').filter(|s| !s.trim().is_empty()).count() as u64 + 1))
        .unwrap_or(0)
}

impl RepoSettings {
    /// 確認用: `;` で区切った手順を、間を空けて順に行う。`<質問>=<値>`（選ぶ欄は値を選ぶ。`!` で始めれば
    /// 「その他…」などの項目。スイッチは on・off。文字の欄は入れて Return）、`revert:<質問>`（↩︎）、`undo`、
    /// `redo`、`page:<ページ>`。
    pub fn run_script(self: &Rc<Self>, script: &str) {
        let steps: Vec<String> = script
            .split(';')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        for (i, step) in steps.into_iter().enumerate() {
            let w = self.weak();
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(800 * (i as u64 + 1)),
                move || {
                    if let Some(s) = w.upgrade() {
                        s.step(&step);
                    }
                },
            );
        }
    }

    fn step(self: &Rc<Self>, step: &str) {
        eprintln!("settings script: {step}");
        match step {
            "undo" => return self.undo(),
            "redo" => return self.redo(),
            _ => {}
        }
        if let Some(page) = step.strip_prefix("page:") {
            return self.select(page);
        }
        if let Some(name) = step.strip_prefix("action:") {
            if let Some(page) = self.stack.visible_child() {
                let _ = page.activate_action(name, None);
            }
            return;
        }
        if let Some(id) = step.strip_prefix("select:") {
            // 出しているページの表で、その行を選ぶ。
            let mut views = Vec::new();
            if let Some(page) = self.stack.visible_child() {
                collect_widgets::<gtk::ColumnView>(&page, &mut views);
            }
            for view in views {
                let Some(selection) = view.model() else {
                    continue;
                };
                for i in 0..selection.n_items() {
                    let Some(item) = selection.item(i) else {
                        continue;
                    };
                    let key = item
                        .downcast_ref::<gtk::StringObject>()
                        .map(|o| o.string().to_string())
                        .or_else(|| crate::configtable::row_key(&item));
                    if key.as_deref() == Some(id) {
                        selection.select_item(i, true);
                    }
                }
            }
            return;
        }
        if let Some((name, value)) = step.strip_prefix("switch:").and_then(|r| r.split_once('=')) {
            // 出しているページの表の、その行のスイッチ。
            if let Some(switch) = self
                .stack
                .visible_child()
                .and_then(|p| find_widget::<gtk::Switch>(&p, &|s| s.widget_name() == name))
            {
                switch.set_active(value == "on");
            }
            return;
        }
        // 前に出ているシート（設定の窓に付いた窓）の欄・ボタン。
        let sheet = || {
            let parent = self.window()?;
            gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Window>().ok())
                .find(|w| w.is_visible() && w.transient_for().as_ref() == Some(&parent))
        };
        if let Some(rest) = step.strip_prefix("fill:") {
            if let (Some(sheet), Some((n, text))) = (sheet(), rest.split_once('=')) {
                let mut entries = Vec::new();
                collect_widgets::<gtk::Entry>(sheet.upcast_ref(), &mut entries);
                if let Some(e) = n.parse::<usize>().ok().and_then(|n| entries.get(n)) {
                    e.set_text(text);
                }
            }
            return;
        }
        if let Some(text) = step.strip_prefix("toggle:") {
            // 出しているページの切り替え（Basic など）。
            if let Some(t) = self.stack.visible_child().and_then(|p| {
                find_widget::<gtk::ToggleButton>(&p, &|t| t.label().as_deref() == Some(text))
            }) {
                t.set_active(true);
            }
            return;
        }
        if let Some(text) = step.strip_prefix("type:") {
            // 設定の窓で今入力している欄に入れて Return。
            let entry = self
                .window()
                .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
                .and_then(|f| {
                    f.downcast_ref::<gtk::Entry>()
                        .cloned()
                        .or_else(|| f.parent().and_downcast::<gtk::Entry>())
                });
            if let Some(entry) = entry {
                entry.set_text(text);
                entry.emit_activate();
            }
            return;
        }
        if let Some(text) = step.strip_prefix("click:") {
            if let Some(button) = sheet().and_then(|w| {
                find_widget::<gtk::Button>(w.upcast_ref(), &|b| b.label().as_deref() == Some(text))
            }) {
                button.emit_clicked();
            }
            return;
        }
        let find = |q: &str| {
            self.controls
                .borrow()
                .iter()
                .find(|(question, _, _)| question == q)
                .map(|(_, c, r)| (c.clone(), r.clone()))
        };
        if let Some(q) = step.strip_prefix("revert:") {
            if let Some((_, revert)) = find(q) {
                revert.emit_clicked();
            }
            return;
        }
        let Some((q, value)) = step.split_once('=') else {
            return;
        };
        let Some((control, _)) = find(q) else {
            eprintln!("settings script: no row {q}");
            return;
        };
        let Some(child) = control.first_child() else {
            return;
        };
        if let Some(switch) = child.downcast_ref::<gtk::Switch>() {
            switch.set_active(value == "on");
        } else if let Some(entry) = child.downcast_ref::<gtk::Entry>() {
            entry.set_text(value);
            entry.emit_activate();
        } else if let Some(special) = value.strip_prefix('!') {
            let _ = child.activate_action("picker.special", Some(&special.to_variant()));
        } else {
            let _ = child.activate_action("picker.pick", Some(&value.to_variant()));
        }
    }
}

/// `root` の下で、`pred` に合う最初の `W`。
fn find_widget<W: IsA<gtk::Widget>>(root: &gtk::Widget, pred: &dyn Fn(&W) -> bool) -> Option<W> {
    let mut all = Vec::new();
    collect_widgets::<W>(root, &mut all);
    all.into_iter().find(|w| pred(w))
}

/// `root` の下の `W` をすべて（`root` も含む）。
fn collect_widgets<W: IsA<gtk::Widget>>(root: &gtk::Widget, out: &mut Vec<W>) {
    if let Some(w) = root.downcast_ref::<W>() {
        out.push(w.clone());
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        collect_widgets(&c, out);
        child = c.next_sibling();
    }
}

/// 署名の形式（`gpg.format` の値）。
pub(crate) const SIGNING_FORMATS: &[&str] = &["openpgp", "ssh", "x509"];

fn format_name(format: &str) -> &str {
    match format {
        "openpgp" => "OpenPGP",
        "ssh" => "SSH",
        "x509" => "X.509",
        other => other,
    }
}

/// 開いている窓: 親の窓、設定の窓、中身。
type Open = (
    glib::WeakRef<gtk::ApplicationWindow>,
    glib::WeakRef<gtk::Window>,
    Rc<RepoSettings>,
);

thread_local! {
    /// 開いているリポジトリの設定の窓（親の窓ごとに 1 つ）。
    static OPEN: RefCell<Vec<Open>> = RefCell::default();
}

/// 開いているリポジトリの設定の窓のページ（`author` など）を選ぶ（確認用）。
pub fn select(parent: &gtk::ApplicationWindow, page: &str) {
    OPEN.with(|o| {
        if let Some((_, _, s)) = o
            .borrow()
            .iter()
            .find(|(p, _, _)| p.upgrade().as_ref() == Some(parent))
        {
            s.select(page);
        }
    });
}

/// `parent` のリポジトリの設定の窓を開く（開いていれば前に出して読み直す）。`page` を選ぶ。
pub fn show(
    parent: &gtk::ApplicationWindow,
    repo: Arc<Repository>,
    location: &str,
    on_change: Rc<dyn Fn()>,
    page: Option<&str>,
) {
    let found = OPEN.with(|o| {
        let mut o = o.borrow_mut();
        o.retain(|(p, w, _)| p.upgrade().is_some() && w.upgrade().is_some());
        o.iter()
            .find(|(p, _, _)| p.upgrade().as_ref() == Some(parent))
            .and_then(|(_, w, s)| Some((w.upgrade()?, s.clone())))
    });
    if let Some((window, settings)) = found {
        settings.load();
        if let Some(page) = page {
            settings.select(page);
        }
        window.present();
        return;
    }
    let settings = RepoSettings::new(repo, location, Level::Repository);
    settings.connect_change(move || on_change());
    if let Some(page) = page {
        settings.select(page);
    }
    if let Ok(script) = std::env::var("AMONHEN_SETTINGS_SCRIPT") {
        settings.run_script(&script);
    }
    let header = gtk::HeaderBar::new();
    let window = gtk::Window::builder()
        .title(tr!("Repository Settings"))
        .default_width(1000)
        .default_height(660)
        .child(&settings.widget)
        .transient_for(parent)
        .destroy_with_parent(true)
        .build();
    window.set_titlebar(Some(&header));
    OPEN.with(|o| {
        o.borrow_mut()
            .push((parent.downgrade(), window.downgrade(), settings.clone()))
    });
    let w = window.downgrade();
    window.connect_close_request(move |_| {
        OPEN.with(|o| {
            o.borrow_mut()
                .retain(|(_, win, _)| win.upgrade().is_some() && win.upgrade() != w.upgrade())
        });
        glib::Propagation::Proceed
    });
    window.present();
}
