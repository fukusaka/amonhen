//! AI（docs/ux.md の「AI」、macOS 版の `Assistant.swift`）。呼び出しは `amonhen-assist` で、ここは設定・送る前の
//! 確認・返事を流しながら見せる部品（[`ResultView`]・[`Panel`]）と、Settings の AI のタブ。
//!
//! 設定は `gtk.ini` の、macOS 版の UserDefaults と同じ名前の鍵（`assistant`・`assistantModel.<CLI>`・
//! `assistantLanguage`・`trustedPrompts`・`assistantRepositories`）。利用者のプロンプトは
//! `~/.config/amonhen/prompts/<用途>.md`、返事のキャッシュは `~/.cache/amonhen/assist`。

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use amonhen_assist::{Cache, Cli, Prompts, Purpose, Variables};
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::Value;

use crate::settings;

/// 使う CLI（`none` か CLI の名前。既定は使わない）。
pub const ASSISTANT: &str = "assistant";
const LANGUAGE: &str = "assistantLanguage";
const TRUSTED: &str = "trustedPrompts";
const REPOSITORIES: &str = "assistantRepositories";

fn model_key(cli: Cli) -> String {
    format!("assistantModel.{}", cli.id())
}

/// CLI の場所を決めて書く鍵（画面には出さない。よくある場所にない CLI と、確認用の偽の CLI のため）。
fn path_key(cli: Cli) -> String {
    format!("assistantPath.{}", cli.id())
}

thread_local! {
    /// 確認用に押すボタン（`--ai translate,explain` など）。
    static AUTO: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// 確認用に、リポジトリごとの確認を飛ばす（`--ai-allow`）。
    static ALLOW: Cell<bool> = const { Cell::new(false) };
    /// 動いている問い合わせの数（`--snapshot` は終わるまで待つ）。
    static BUSY: Cell<u32> = const { Cell::new(0) };
}

pub fn selected() -> Option<Cli> {
    settings::string(ASSISTANT).and_then(|s| Cli::from_id(&s))
}

pub fn set_selected(cli: Option<Cli>) {
    settings::set_string(ASSISTANT, cli.map_or("none", Cli::id));
}

/// CLI ごとのモデル（空なら CLI の既定）。
pub fn model(cli: Cli) -> String {
    settings::string(&model_key(cli)).unwrap_or_else(|| cli.default_model().to_owned())
}

pub fn set_model(cli: Cli, model: &str) {
    settings::set_string(&model_key(cli), model);
}

/// 翻訳と解説の言語。
pub fn language() -> String {
    settings::string(LANGUAGE)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "日本語".to_owned())
}

fn home() -> PathBuf {
    glib::home_dir()
}

/// CLI の実行ファイル（見つからなければ None）。
pub fn executable(cli: Cli) -> Option<PathBuf> {
    match settings::string(&path_key(cli)).filter(|p| !p.is_empty()) {
        Some(path) => Some(PathBuf::from(path)).filter(|p| p.is_file()),
        None => cli.find(&home()),
    }
}

pub fn prompts() -> Prompts {
    Prompts::new(glib::user_config_dir().join("amonhen").join("prompts"))
}

fn cache() -> Cache {
    Cache::new(glib::user_cache_dir().join("amonhen").join("assist"))
}

/// AI を使えるか。
pub enum Availability {
    /// CLI を選んでいない（ボタンを出さない）。
    Off,
    /// 選んだ CLI が見つからない（ボタンは出すが押せない。理由）。
    Missing(String),
    Ready(Cli, PathBuf),
}

pub fn availability() -> Availability {
    let Some(cli) = selected() else {
        return Availability::Off;
    };
    match executable(cli) {
        Some(path) => Availability::Ready(cli, path),
        None => Availability::Missing(not_found(cli)),
    }
}

pub fn is_ready() -> bool {
    matches!(availability(), Availability::Ready(..))
}

fn not_found(cli: Cli) -> String {
    tr!(
        "Not found. Install {name} ({program}) and sign in with the CLI.",
        name = cli.title(),
        program = cli.program()
    )
}

/// AI のボタンを設定に合わせる: CLI を選んでいなければ出さず、見つからなければ押せなくして理由を出す。
pub fn track(widget: &impl IsA<gtk::Widget>, tooltip: String) {
    let update = {
        let weak = widget.upcast_ref::<gtk::Widget>().downgrade();
        move || {
            let Some(w) = weak.upgrade() else { return };
            match availability() {
                Availability::Off => w.set_visible(false),
                Availability::Missing(reason) => {
                    w.set_visible(true);
                    w.set_sensitive(false);
                    w.set_tooltip_text(Some(&reason));
                }
                Availability::Ready(..) => {
                    w.set_visible(true);
                    w.set_sensitive(true);
                    w.set_tooltip_text(Some(&tooltip));
                }
            }
        }
    };
    update();
    settings::connect_changed(move |key| {
        if key == ASSISTANT || key.starts_with("assistantPath.") {
            update();
        }
    });
}

/// 確認用に押すボタンを決める（`--ai`）。
pub fn set_auto(names: &str) {
    AUTO.with(|a| {
        a.borrow_mut()
            .extend(names.split(',').map(|s| s.trim().to_owned()))
    });
}

/// 確認用に、リポジトリごとの確認を飛ばす（`--ai-allow`）。
pub fn allow_without_asking() {
    ALLOW.with(|a| a.set(true));
}

/// `name` を押すよう頼まれていれば（`--ai`）、`ready()` になったところで一度 `f()` を呼ぶ。
pub fn auto(name: &str, ready: impl Fn() -> bool + 'static, f: impl FnOnce() + 'static) {
    if !AUTO.with(|a| a.borrow().iter().any(|n| n == name)) {
        return;
    }
    let name = name.to_owned();
    let mut f = Some(f);
    let mut tries = 0;
    busy(1);
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        tries += 1;
        if !ready() && tries < 100 {
            return glib::ControlFlow::Continue;
        }
        AUTO.with(|a| a.borrow_mut().retain(|n| *n != name));
        if tries < 100
            && let Some(f) = f.take()
        {
            f();
        }
        // 押した後の確認や問い合わせが始まるまで、少し待ってもらう。
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), || busy(-1));
        glib::ControlFlow::Break
    });
}

/// `button` を押すよう頼まれていれば（`--ai`）、見えて押せるようになったところで押す。
pub fn auto_click(button: &gtk::Button, name: &str) {
    let (b, c) = (button.downgrade(), button.downgrade());
    auto(
        name,
        move || {
            b.upgrade()
                .is_some_and(|b| b.is_mapped() && b.is_sensitive())
        },
        move || {
            if let Some(b) = c.upgrade() {
                b.emit_clicked();
            }
        },
    );
}

fn busy(by: i32) {
    BUSY.with(|b| b.set((b.get() as i32 + by).max(0) as u32));
}

/// 動いている問い合わせがあるか（`--snapshot` は終わるまで待つ）。
pub fn is_busy() -> bool {
    BUSY.with(|b| b.get() > 0)
}

/// JSON の対応表の鍵（設定の文字列）。
fn table(key: &str) -> serde_json::Map<String, Value> {
    settings::string(key)
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn set_table(key: &str, map: serde_json::Map<String, Value>) {
    settings::set_string(key, &Value::Object(map).to_string());
}

/// リポジトリごとの許可の鍵（作業ツリーの場所。SSH 先などで分からなければ `remote`）。
pub fn repository_key(workdir: Option<&Path>) -> String {
    workdir.map_or_else(|| "remote".to_owned(), |p| p.to_string_lossy().into_owned())
}

/// このリポジトリで AI を使ってよいか。初めて使うときに、送る内容の大きさを見せて確かめる。
pub async fn confirm(parent: Option<&gtk::Window>, key: &str, summary: &str) -> bool {
    if ALLOW.with(Cell::get) {
        return true;
    }
    let mut allowed = table(REPOSITORIES);
    if let Some(decided) = allowed.get(key).and_then(Value::as_bool) {
        return decided;
    }
    let cli = selected().map_or("", Cli::title);
    let sheet = crate::sheets::Sheet::new(
        parent,
        &tr!("Use AI in this repository?"),
        &tr!(
            "Sends to {cli}: {summary}\nThis repository’s content (messages and diffs) goes to an external service through the CLI.\nYou can change this later with “Forget Per-Repository Permissions” in Settings > AI.",
            cli = cli,
            summary = summary
        ),
    );
    sheet.buttons(
        &tr!("Use"),
        false,
        Some(&tr!("Don’t Use in This Repository")),
    );
    let decision = match sheet.run().await {
        crate::sheets::Response::Ok => true,
        crate::sheets::Response::Other => false,
        crate::sheets::Response::Cancel => return false,
    };
    allowed.insert(key.to_owned(), Value::Bool(decision));
    set_table(REPOSITORIES, allowed);
    decision
}

pub fn forget_repository_decisions() {
    settings::set_string(REPOSITORIES, "{}");
}

/// 使うプロンプト。リポジトリのものは clone したものにも入っているので、初めて使うとき・中身が変わったときに
/// 見せて確かめる（許可はパスと中身のハッシュで覚える）。断られたら利用者のものを使う。
async fn template(
    parent: Option<&gtk::Window>,
    purpose: Purpose,
    workdir: Option<&Path>,
) -> String {
    let user = || prompts().user_prompt(purpose);
    let Some((path, text)) = workdir.and_then(|w| amonhen_assist::repository_prompt(w, purpose))
    else {
        return user();
    };
    let mut trusted = table(TRUSTED);
    let hash = amonhen_assist::sha256_hex(text.as_bytes());
    let path_text = path.to_string_lossy().into_owned();
    if trusted.get(&path_text).and_then(Value::as_str) == Some(hash.as_str()) {
        return text;
    }
    let shown: String = text.chars().take(1500).collect();
    let sheet = crate::sheets::Sheet::new(
        parent,
        &tr!("Use the repository’s prompt?"),
        &format!("{path_text}\n\n{shown}"),
    );
    sheet.message.set_selectable(true);
    sheet.button(
        &tr!("Don’t Use (Use My Prompt)"),
        crate::sheets::Response::Cancel,
        None,
    );
    sheet.button(
        &tr!("Use"),
        crate::sheets::Response::Ok,
        Some(crate::sheets::Style::Suggested),
    );
    if sheet.run().await != crate::sheets::Response::Ok {
        return user();
    }
    trusted.insert(path_text, Value::String(hash));
    set_table(TRUSTED, trusted);
    text
}

/// 問い合わせの途中の様子を受け取る。
pub type OnText = Rc<dyn Fn(&str)>;

/// `purpose` のプロンプトに `variables` を差し込んで送り、返事を流しながら `on_text(それまでの全文)` に渡す。
/// `use_cache` なら、同じ問い合わせの前の結果をすぐ返す。`cancel` が立ったら止める。
pub async fn run(
    parent: Option<gtk::Window>,
    purpose: Purpose,
    mut variables: Variables,
    workdir: Option<PathBuf>,
    use_cache: bool,
    cancel: Arc<AtomicBool>,
    on_text: OnText,
) -> Result<String, String> {
    let (cli, executable) = match availability() {
        Availability::Ready(cli, path) => (cli, path),
        Availability::Missing(reason) => return Err(reason),
        Availability::Off => {
            return Err(tr!(
                "No AI CLI is selected, or it can’t be found (Settings > AI)."
            ));
        }
    };
    variables.entry("language".into()).or_insert_with(language);
    let template = template(parent.as_ref(), purpose, workdir.as_deref()).await;
    let prompt = amonhen_assist::build(&template, &variables, purpose)?;
    let model = model(cli);
    let key = amonhen_assist::cache_key(cli.id(), &model, purpose, &prompt);
    if use_cache && let Some(cached) = cache().get(&key) {
        on_text(&cached);
        return Ok(cached);
    }
    let started = std::time::Instant::now();
    busy(1);
    let (tx, rx) = async_channel::unbounded::<String>();
    let task = gio::spawn_blocking(move || {
        amonhen_assist::stream(cli, &executable, &model, &prompt, &cancel, |text| {
            let _ = tx.send_blocking(text.to_owned());
        })
    });
    while let Ok(text) = rx.recv().await {
        // 溜まっていれば最後のものだけ出す。
        let mut latest = text;
        while let Ok(next) = rx.try_recv() {
            latest = next;
        }
        on_text(&latest);
    }
    let result = task.await.unwrap_or_else(|_| Err(tr!("Failed")));
    busy(-1);
    let result = result?;
    crate::activity::record(
        &format!("AI: {}", purpose.title()),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    cache().put(&key, &result);
    Ok(result)
}

/// 返事を作る仕事（`Run` を受け取り、変数を作って [`run`] を呼ぶ）。
pub type Work = Rc<dyn Fn(Run) -> Pin<Box<dyn Future<Output = Result<String, String>>>>>;

/// 1 回の問い合わせに渡すもの。
#[derive(Clone)]
pub struct Run {
    pub use_cache: bool,
    pub cancel: Arc<AtomicBool>,
    pub on_text: OnText,
}

/// 返事を流しながら見せる部品: 文と、状態・コピー・作り直し（キャッシュを使わない）・「使う」のボタンの帯。
pub struct ResultView {
    /// 文の欄。
    pub body: gtk::Label,
    /// 状態とボタンの帯。
    pub bar: gtk::Box,
    spinner: gtk::Spinner,
    status: gtk::Label,
    regenerate: gtk::Button,
    use_button: gtk::Button,
    work: Work,
    on_use: Option<OnText>,
    result: RefCell<String>,
    cancel: RefCell<Arc<AtomicBool>>,
    generation: Cell<u64>,
    /// 今の文（流れてきた途中の分を含む）。
    shown: RefCell<String>,
    /// 文が変わったら呼ぶ（詳細の欄の高さを合わせるなど）。
    pub on_change: RefCell<Option<Rc<dyn Fn()>>>,
}

impl ResultView {
    /// `use_title` と `on_use` を渡すと「使う」ボタンを出す（結果を画面に入れる）。
    pub fn new(use_title: Option<&str>, on_use: Option<OnText>, work: Work) -> Rc<Self> {
        let body = gtk::Label::new(None);
        body.set_xalign(0.0);
        body.set_yalign(0.0);
        body.set_wrap(true);
        body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        body.set_selectable(true);
        let spinner = gtk::Spinner::new();
        let status = gtk::Label::new(None);
        status.add_css_class("dim-label");
        status.add_css_class("caption");
        status.set_xalign(0.0);
        status.set_hexpand(true);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let copy = gtk::Button::with_label(&tr!("Copy"));
        let regenerate = gtk::Button::with_label(&tr!("Regenerate"));
        regenerate.set_tooltip_text(Some(&tr!("Ask again without using the previous result")));
        let use_button = gtk::Button::with_label(use_title.unwrap_or_default());
        use_button.add_css_class("suggested-action");
        use_button.set_visible(on_use.is_some());
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for w in [
            spinner.upcast_ref::<gtk::Widget>(),
            status.upcast_ref(),
            copy.upcast_ref(),
            regenerate.upcast_ref(),
            use_button.upcast_ref(),
        ] {
            bar.append(w);
        }
        let this = Rc::new(Self {
            body,
            bar,
            spinner,
            status,
            regenerate: regenerate.clone(),
            use_button: use_button.clone(),
            work,
            on_use,
            result: RefCell::default(),
            cancel: RefCell::new(Arc::new(AtomicBool::new(false))),
            generation: Cell::new(0),
            shown: RefCell::default(),
            on_change: RefCell::default(),
        });
        let weak = Rc::downgrade(&this);
        copy.connect_clicked(move |b| {
            if let Some(this) = weak.upgrade() {
                let result = this.result.borrow();
                let text = if result.is_empty() {
                    this.shown.borrow().clone()
                } else {
                    result.clone()
                };
                b.clipboard().set_text(&text);
            }
        });
        let weak = Rc::downgrade(&this);
        regenerate.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.start(false);
            }
        });
        let weak = Rc::downgrade(&this);
        use_button.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade()
                && let Some(f) = &this.on_use
            {
                f(&this.result.borrow());
            }
        });
        // 画面から外したら（コミットを選び直したなど）止める。
        let weak = Rc::downgrade(&this);
        this.bar.connect_unrealize(move |_| {
            if let Some(this) = weak.upgrade() {
                this.stop();
            }
        });
        this
    }

    /// 止める（窓を閉じたときなど）。
    pub fn stop(&self) {
        self.cancel.borrow().store(true, Ordering::Relaxed);
    }

    /// 頼む（`use_cache` なら前の結果を使う）。
    pub fn start(self: &Rc<Self>, use_cache: bool) {
        self.stop();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel.replace(cancel.clone());
        let g = self.generation.get() + 1;
        self.generation.set(g);
        self.result.replace(String::new());
        self.set_text("");
        self.status.set_text(&tr!("Waiting…"));
        self.spinner.start();
        self.regenerate.set_sensitive(false);
        self.use_button.set_sensitive(false);
        let weak = Rc::downgrade(self);
        let on_text: OnText = Rc::new(move |partial: &str| {
            if let Some(this) = weak.upgrade()
                && this.generation.get() == g
            {
                this.set_text(partial);
                this.status.set_text(&tr!("Receiving…"));
            }
        });
        let future = (self.work)(Run {
            use_cache,
            cancel,
            on_text,
        });
        let weak = Rc::downgrade(self);
        busy(1);
        glib::spawn_future_local(async move {
            let result = future.await;
            busy(-1);
            let Some(this) = weak.upgrade() else { return };
            if this.generation.get() != g {
                return;
            }
            match result {
                Ok(text) => {
                    this.set_text(&text);
                    this.result.replace(text);
                    let done = selected().map(|cli| {
                        let m = model(cli);
                        if m.is_empty() {
                            cli.title().to_owned()
                        } else {
                            m
                        }
                    });
                    this.status
                        .set_text(&tr!("Done ({model})", model = done.unwrap_or_default()));
                    this.use_button.set_sensitive(true);
                }
                Err(e) => {
                    this.status.set_text(&tr!("Failed"));
                    this.set_text(&e);
                }
            }
            this.spinner.stop();
            this.regenerate.set_sensitive(true);
        });
    }

    fn set_text(&self, text: &str) {
        self.shown.replace(text.to_owned());
        self.body.set_text(text);
        let f = self.on_change.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }
}

/// 返事を見せる窓（macOS 版の `AssistantPanel`）。`use_title` と `on_use` を渡すと「使う」ボタンを出し、押したら
/// 閉じる。
pub fn panel(
    parent: Option<&gtk::Window>,
    title: &str,
    use_title: Option<&str>,
    on_use: Option<OnText>,
    work: Work,
) -> Rc<ResultView> {
    let window = gtk::Window::builder()
        .title(format!("{title} — {}", selected().map_or("", Cli::title)))
        .default_width(620)
        .default_height(460)
        .destroy_with_parent(true)
        .build();
    window.set_transient_for(parent);
    let w = window.downgrade();
    let on_use = on_use.map(|f| -> Rc<dyn Fn(&str)> {
        Rc::new(move |text: &str| {
            f(text);
            if let Some(w) = w.upgrade() {
                w.close();
            }
        })
    });
    let view = ResultView::new(use_title, on_use, work);
    // 開いたときに文の欄に入力の印を出さない（マウスでは選べる）。
    view.body.set_focusable(false);
    view.body.set_margin_top(10);
    view.body.set_margin_bottom(10);
    view.body.set_margin_start(12);
    view.body.set_margin_end(12);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&view.body)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    // 流れてくる間は、いちばん下を見せる。
    let adj = scrolled.vadjustment();
    adj.connect_upper_notify(|a| a.set_value(a.upper() - a.page_size()));
    view.bar.set_margin_top(8);
    view.bar.set_margin_bottom(10);
    view.bar.set_margin_start(12);
    view.bar.set_margin_end(12);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&scrolled);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&view.bar);
    window.set_child(Some(&content));
    let v = view.clone();
    window.connect_close_request(move |_| {
        v.stop();
        glib::Propagation::Proceed
    });
    window.present();
    view.start(true);
    // 確認用（`--ai use`）: 返事が来たら「使う」を押す。
    let (a, b) = (view.use_button.downgrade(), view.use_button.downgrade());
    auto(
        "use",
        move || {
            a.upgrade()
                .is_some_and(|u| u.is_visible() && u.is_sensitive())
        },
        move || {
            if let Some(u) = b.upgrade() {
                u.emit_clicked();
            }
        },
    );
    view
}

/// 畳める見出しの下に返事を出す（History の詳細の翻訳・解説）。
pub fn inline(title: &str, work: Work) -> (gtk::Box, Rc<ResultView>) {
    install_css();
    let view = ResultView::new(None, None, work);
    let arrow = gtk::Button::from_icon_name("pan-down-symbolic");
    arrow.add_css_class("flat");
    arrow.set_tooltip_text(Some(&tr!("Collapse")));
    // 使った CLI とモデルは、終わったときの状態の文に出る。
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    head.append(&arrow);
    head.append(&heading);
    view.bar.set_hexpand(true);
    view.bar.set_margin_start(8);
    // 詳細の欄の中なので、ボタンは小さく。
    let mut child = view.bar.first_child();
    while let Some(c) = child {
        if c.is::<gtk::Button>() {
            c.add_css_class("flat");
            c.add_css_class("caption");
        }
        child = c.next_sibling();
    }
    head.append(&view.bar);
    view.body.set_margin_start(28);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.add_css_class("ai-result");
    root.append(&head);
    root.append(&view.body);
    let body = view.body.clone();
    arrow.connect_clicked(move |b| {
        let open = !body.is_visible();
        body.set_visible(open);
        b.set_icon_name(if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        });
        b.set_tooltip_text(Some(&if open { tr!("Collapse") } else { tr!("Expand") }));
    });
    // 部品があるうちは持っておく。
    // SAFETY: 同じ名前の値は、この型でだけ置く・読む。
    unsafe { root.set_data("assist-view", view.clone()) };
    view.start(true);
    (root, view)
}

/// `f` を [`Work`] にする。
pub fn work<F, Fut>(f: F) -> Work
where
    F: Fn(Run) -> Fut + 'static,
    Fut: Future<Output = Result<String, String>> + 'static,
{
    Rc::new(move |run| Box::pin(f(run)))
}

/// 差分の右クリックの「AI でこの hunk を解説」（使えなければ None）。
pub fn hunk_item(
    parent: Option<gtk::Window>,
    workdir: Option<PathBuf>,
    path: String,
    hunk: String,
    message: String,
) -> Option<crate::menu::Item> {
    if !is_ready() {
        return None;
    }
    Some(crate::menu::action(
        tr!("Explain This Hunk with AI"),
        move || {
            explain_hunk(
                parent.clone(),
                workdir.clone(),
                path.clone(),
                hunk.clone(),
                message.clone(),
            );
        },
    ))
}

/// hunk を解説する（確かめてから窓で）。
pub fn explain_hunk(
    parent: Option<gtk::Window>,
    workdir: Option<PathBuf>,
    path: String,
    hunk: String,
    message: String,
) {
    glib::spawn_future_local(async move {
        let key = repository_key(workdir.as_deref());
        let summary = tr!("One hunk of {path}", path = path);
        if !confirm(parent.as_ref(), &key, &summary).await {
            return;
        }
        let p = parent.clone();
        panel(
            parent.as_ref(),
            &tr!("Hunk Explanation"),
            None,
            None,
            work(move |run: Run| {
                let vars: Variables = [
                    ("path", path.clone()),
                    ("hunk", hunk.clone()),
                    ("message", message.clone()),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect();
                run_with(p.clone(), Purpose::ExplainHunk, vars, workdir.clone(), run)
            }),
        );
    });
}

/// [`run`] を [`Run`] で呼ぶ。
pub fn run_with(
    parent: Option<gtk::Window>,
    purpose: Purpose,
    variables: Variables,
    workdir: Option<PathBuf>,
    run: Run,
) -> impl Future<Output = Result<String, String>> {
    self::run(
        parent,
        purpose,
        variables,
        workdir,
        run.use_cache,
        run.cancel,
        run.on_text,
    )
}

/// AI の提案の紫の帯と、返事の欄の見た目。
pub fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".ai-suggestion { border-left: 4px solid #9141ac; background-color: alpha(#9141ac, 0.08); \
         padding: 8px 10px; border-radius: 4px; }
         .ai-result { border-left: 3px solid alpha(#9141ac, 0.6); padding: 4px 0 4px 6px; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 変数の表を作る。
pub fn variables<const N: usize>(pairs: [(&str, String); N]) -> Variables {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}
