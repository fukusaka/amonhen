//! Windows のアプリ（WinUI 3）。左にサイドバー（`sidebar.rs`）、まん中に History、右に選んだコミットの詳細（上）と
//! 全ファイルを並べた差分（下）。GTK4 版・macOS 版の History の画面と同じ並び。History と差分は SwapChainPanel に
//! Direct2D で描く。
//!
//! 部品は windows-reactor で宣言的に作る。reactor が出していない入力（ホイール・キー・押した位置）は、History と
//! 差分を包む Grid の生の WinUI のオブジェクト（`request_native_element`）に、`amonhen-win-xaml` の型で直に付ける。
//! WinUI のハンドラーは `Send` でなければならないので何も持たず、UI のスレッドの `HISTORY` から描き手を取る。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use amonhen_core::{RefsSnapshot, Repository};
use amonhen_model::RefEntry;
use amonhen_model::tr;
use amonhen_win_xaml as xaml;
use serde_json::{Value, json};
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::Threading::WaitForSingleObjectEx;
use windows_core_reactor::{IInspectable, Interface as _, Ref};
use windows_reactor::*;

use crate::render::{DIFF_ROW_HEIGHT, Options, ROW_HEIGHT, Renderer, Surface};
use crate::sidebar::{self, BranchGroups};
use crate::{Args, Session};

thread_local! {
    /// 根の部品に渡す引数（`Component::Input` は比べられる値でなければならないので、ここに置く）。
    static ARGS: RefCell<Option<(Args, Instant)>> = const { RefCell::new(None) };
    /// History と差分の描き手。
    static HISTORY: RefCell<Option<Rc<RefCell<HistoryView>>>> = const { RefCell::new(None) };
    /// History と差分に付けたイベント（持っている間だけ届く）。
    static REVOKERS: RefCell<Vec<windows_core_reactor::EventRevoker>> =
        const { RefCell::new(Vec::new()) };
}

/// ホイールの 1 目盛り（120）で進む行の数（Windows の既定の 3 行）。
const WHEEL_ROWS: f32 = 3.0;

pub fn run(args: Args, started: Instant) -> Result<(), String> {
    ARGS.with(|a| *a.borrow_mut() = Some((args, started)));
    App::run_component::<Root>(()).map_err(|e| format!("WinUI を始められない: {e}"))
}

fn with_history(f: impl FnOnce(&mut HistoryView)) {
    if let Some(view) = HISTORY.with(|h| h.borrow().clone()) {
        f(&mut view.borrow_mut());
    }
}

struct Root {
    panel: ElementRef<SwapChainPanel>,
    area: ElementRef<Grid>,
    diff_panel: ElementRef<SwapChainPanel>,
    diff_area: ElementRef<Grid>,
    refs: Option<Arc<RefsSnapshot>>,
    /// ローカルブランチの分け方（コアの `branchGroups`）。
    groups: Option<BranchGroups>,
    /// 選んだコミットの詳細（`commit_info` の答え）と、時間のかかる分（`commit_extras`）。
    info: Option<(Value, Option<Value>)>,
    error: Option<String>,
    /// 出している確認・知らせのダイアログ。
    dialog: Option<Dialog>,
}

/// 窓に重ねるダイアログ（macOS 版のシート、GTK4 版の `sheets`）。
#[derive(Clone)]
enum Dialog {
    /// Undo の確認（`undoPeek` の名前と、何が戻るか）。
    Undo {
        repo: Arc<Repository>,
        label: String,
        what: String,
    },
    /// 操作の失敗。
    Failed { title: String, message: String },
}

enum Message {
    Refs(Arc<RefsSnapshot>, Arc<Repository>),
    Groups(Option<BranchGroups>),
    Commit(Value, Option<Value>),
    /// サイドバーの項目を選んだ（項目のキー）。
    Invoked(String),
    /// メニューの操作を選んだ（操作の id）。
    Command(String),
    /// `undoPeek` の答え（戻せるものがなければ `None`）。
    UndoPeek(Arc<Repository>, Option<(String, String)>),
    /// ダイアログを閉じた（押したボタン）。
    DialogClosed(ContentDialogResult),
    /// 書き込む操作が終わった（失敗ならそのダイアログの見出しと文）。
    Performed(Option<(String, String)>),
    Failed(String),
}

impl Component for Root {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        Self {
            panel: ElementRef::new(),
            area: ElementRef::new(),
            diff_panel: ElementRef::new(),
            diff_area: ElementRef::new(),
            refs: None,
            groups: None,
            info: None,
            error: None,
            dialog: None,
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Refs(refs, repo) => {
                self.refs = Some(refs);
                // ブランチの分け方は ref が変わるたびに読み直す（数十 ms かかるので裏で）。
                context.spawn_background(move |_| {
                    let reply = repo.run_json(r#"{"op":"branchGroups"}"#);
                    let reply = serde_json::from_str(&reply).unwrap_or_default();
                    Message::Groups(BranchGroups::from_reply(&reply))
                });
            }
            Message::Groups(groups) => {
                if groups.is_some() {
                    self.groups = groups;
                }
            }
            Message::Commit(info, extras) => self.info = Some((info, extras)),
            Message::Failed(e) => self.error = Some(e),
            Message::Command(id) if id == "undo" => self.undo(context),
            Message::Command(id) => with_history(|h| h.command(&id)),
            Message::UndoPeek(repo, peek) => match peek {
                Some((label, what)) => self.dialog = Some(Dialog::Undo { repo, label, what }),
                None => beep(),
            },
            Message::DialogClosed(result) => {
                if let (Some(Dialog::Undo { repo, .. }), ContentDialogResult::Primary) =
                    (self.dialog.take(), result)
                {
                    perform(context, repo, json!({ "op": "undo" }), tr!("Couldn’t Undo"));
                }
            }
            Message::Performed(failed) => {
                if let Some((title, message)) = failed {
                    self.dialog = Some(Dialog::Failed { title, message });
                }
            }
            Message::Invoked(key) => {
                if let Some(entry) = self.entry(&key)
                    && let Some(target) = entry.target
                {
                    with_history(|h| h.select_commit(&target));
                }
            }
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        let (args, _) = ARGS.with(|a| a.borrow().clone()).expect("args");
        let name = std::fs::canonicalize(&args.repo)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| args.repo.display().to_string());
        context.window_title(name);
        let refs = (
            self.panel.clone(),
            self.area.clone(),
            self.diff_panel.clone(),
            self.diff_area.clone(),
        );
        let sender = context.sender();
        context.use_effect("history", (), move || {
            let (panel, area, diff_panel, diff_area) = refs;
            let (args, started) = ARGS.with(|a| a.borrow().clone()).expect("args");
            let session = match Session::new(&args, "app", started, Arc::new(|| {})) {
                Ok(s) => s,
                Err(e) => {
                    sender.send(Message::Failed(e));
                    return None;
                }
            };
            let view = Rc::new(RefCell::new(HistoryView {
                session,
                panel: panel.clone(),
                area,
                options: args.options,
                attached: false,
                input_requested: false,
                diff: DiffPane {
                    panel: diff_panel.clone(),
                    area: diff_area,
                    surface: None,
                    scale: 1.0,
                    attached: false,
                    input_requested: false,
                    scroll_y: 0.0,
                    dirty: true,
                },
                sender,
                refs_sent: None,
                selected: None,
                shown: None,
            }));
            HISTORY.with(|h| *h.borrow_mut() = Some(view.clone()));
            let history = panel.observe_surface(|event| {
                with_history(|h| {
                    if let Err(e) = h.on_event(event) {
                        h.sender.send(Message::Failed(e));
                    }
                })
            });
            let diff = diff_panel.observe_surface(|event| {
                with_history(|h| {
                    if let SwapChainPanelEvent::Metrics {
                        width,
                        height,
                        scale_x,
                        ..
                    } = event
                        && let Err(e) = h.diff.resize(&h.session.renderer, width, height, scale_x)
                    {
                        h.sender.send(Message::Failed(e));
                    }
                })
            });
            Some(Box::new(move || {
                drop(history);
                drop(diff);
                REVOKERS.with(|r| r.borrow_mut().clear());
                HISTORY.with(|h| h.borrow_mut().take());
            }))
        });

        let sidebar = TreeView::new()
            .on_item_invoked(context.callback(Message::Invoked))
            .grid_column(0)
            .nodes(sidebar::nodes(
                self.refs.as_ref().map(|r| &r.refs),
                self.groups.as_ref(),
            ));
        let history: View = match &self.error {
            Some(e) => TextBlock::new()
                .text(e.clone())
                .text_wrapping(TextWrapping::Wrap)
                .margin(16.0)
                .into(),
            None => SwapChainPanel::new().element_ref(&self.panel).into(),
        };
        let right = Grid::new()
            .rows([GridLength::Pixel(240.0), GridLength::Star(1.0)])
            .grid_column(2)
            .children((
                ScrollViewer::new()
                    .grid_row(0)
                    .content(details(self.info.as_ref().map(|(i, e)| (i, e.as_ref())))),
                Grid::new()
                    .background(Brush::Solid(Color::transparent()))
                    .element_ref(&self.diff_area)
                    .grid_row(1)
                    .children((SwapChainPanel::new().element_ref(&self.diff_panel),)),
            ));
        let menu_bar = crate::menus::menu_bar(|| context.callback(Message::Command));
        let toolbar = crate::menus::toolbar(context.callback(Message::Command));
        let body = Grid::new()
            .grid_row(2)
            .columns([
                GridLength::Pixel(260.0),
                GridLength::Star(1.0),
                GridLength::Star(1.0),
            ])
            .children((
                sidebar,
                Grid::new()
                    .background(Brush::Solid(Color::transparent()))
                    .element_ref(&self.area)
                    .grid_column(1)
                    .children((history,)),
                right,
            ));
        let dialog = self.dialog_view(context);
        Grid::new()
            .rows([GridLength::Auto, GridLength::Auto, GridLength::Star(1.0)])
            .children((menu_bar, toolbar, body, dialog))
    }
}

impl Root {
    /// Repository > Undo…: 最後の操作を見せて、確かめてから戻す（GTK4 版の `sheets::undo`）。
    fn undo(&self, context: &ComponentContext<Self>) {
        let mut repo = None;
        with_history(|h| repo = h.session.data.repo());
        let Some(repo) = repo else { return beep() };
        context.spawn_background(move |_| {
            let peek: Value =
                serde_json::from_str(&repo.run_json(r#"{"op":"undoPeek"}"#)).unwrap_or_default();
            let found = peek["label"].as_str().map(|label| {
                let what = match peek["info"]["kind"].as_str() {
                    Some("files") => {
                        tr!("Puts the files back as they were. HEAD and the index stay.")
                    }
                    Some("stash") => tr!("Brings back the dropped stash."),
                    Some("rename") => tr!("Gives the branch its earlier name back."),
                    _ => tr!("Returns HEAD and the branches to where they were."),
                };
                (label.to_owned(), what)
            });
            Message::UndoPeek(repo, found)
        });
    }

    fn dialog_view(&self, context: &ViewContext<Self>) -> View {
        let dialog = ContentDialog::new().on_closed(context.callback(Message::DialogClosed));
        match &self.dialog {
            None => dialog.is_open(false).into(),
            Some(Dialog::Undo { label, what, .. }) => dialog
                .is_open(true)
                .title(tr!("Undo “{label}”?", label = label))
                .primary_button_text("Undo")
                .close_button_text(tr!("Cancel"))
                .content(
                    TextBlock::new()
                        .text(what.clone())
                        .text_wrapping(TextWrapping::Wrap),
                ),
            Some(Dialog::Failed { title, message }) => dialog
                .is_open(true)
                .title(title.clone())
                .close_button_text("OK")
                .content(
                    TextBlock::new()
                        .text(message.clone())
                        .text_wrapping(TextWrapping::Wrap)
                        .is_text_selection_enabled(true),
                ),
        }
    }

    /// サイドバーの項目のキーが指す ref。
    fn entry(&self, key: &str) -> Option<&RefEntry> {
        let (kind, name) = sidebar::parse_ref_key(key)?;
        self.refs
            .as_ref()?
            .refs
            .entries
            .iter()
            .find(|e| e.kind as u8 == kind && e.name == name)
    }
}

/// 押せない・戻すものがないときの音（macOS 版の `NSSound.beep`、GTK4 版の `error_bell`）。
fn beep() {
    use windows::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows::Win32::UI::WindowsAndMessaging::MB_OK;
    // SAFETY: 音を鳴らすだけ。
    let _ = unsafe { MessageBeep(MB_OK) };
}

/// 書き込む操作を裏で実行する。終わると `Performed`（変化は見張りが拾って読み直す）。`title` は失敗の見出し。
fn perform(context: &ComponentContext<Root>, repo: Arc<Repository>, command: Value, title: String) {
    context.spawn_background(move |_| {
        let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
            .unwrap_or_else(|e| json!({ "error": e.to_string() }));
        Message::Performed(reply["error"].as_str().map(|e| (title, e.to_owned())))
    });
}

/// `+0900` のようなタイムゾーンを秒にする。
fn tz_seconds(tz: &str) -> i64 {
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let digits = tz.trim_start_matches(['+', '-']);
    let (h, m) = (
        digits
            .get(..2)
            .and_then(|h| h.parse::<i64>().ok())
            .unwrap_or(0),
        digits
            .get(2..4)
            .and_then(|m| m.parse::<i64>().ok())
            .unwrap_or(0),
    );
    sign * (h * 3600 + m * 60)
}

/// 作者・コミッターの 1 行（名前 <メール>  日時 タイムゾーン）。
fn person(info: &Value, role: &str) -> String {
    let name = info[role].as_str().unwrap_or_default();
    let email = info[format!("{role}Email")].as_str().unwrap_or_default();
    let tz = info[format!("{role}Tz")].as_str().unwrap_or_default();
    let time = info[format!("{role}Time")].as_i64().unwrap_or(0);
    format!(
        "{name} <{email}>  {} {tz}",
        crate::render::date(time + tz_seconds(tz))
    )
}

/// コミットの詳細（作者、コミッター、Commit・Parent・Refs の表、件名と本文）。
fn details(info: Option<(&Value, Option<&Value>)>) -> View {
    let Some((info, extras)) = info else {
        return TextBlock::new().margin(12.0).into();
    };
    let secondary = Brush::Solid(Color::argb(0x99, 0x1c, 0x1c, 0x1f));
    let line = |label: &str, value: String| {
        TextBlock::new()
            .text(format!("{label}  {value}"))
            .font_size(12.0)
            .is_text_selection_enabled(true)
            .text_wrapping(TextWrapping::Wrap)
            .foreground(secondary)
    };
    let mut rows: Vec<View> = vec![line("Author", person(info, "author")).into()];
    if info["committer"] != info["author"] || info["committerTime"] != info["authorTime"] {
        rows.push(line("Committer", person(info, "committer")).into());
    }
    rows.push(
        line(
            "Commit",
            info["oid"].as_str().unwrap_or_default().to_owned(),
        )
        .into(),
    );
    let list = |key: &str| -> Vec<String> {
        info[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let parents = list("parents");
    if !parents.is_empty() {
        let short: Vec<String> = parents
            .iter()
            .map(|p| p[..p.len().min(10)].to_owned())
            .collect();
        let label = if parents.len() > 1 {
            "Parents"
        } else {
            "Parent"
        };
        rows.push(line(label, short.join("  ")).into());
    }
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    if let Some(extras) = extras {
        let children = strings(&extras["children"]);
        if !children.is_empty() {
            let short: Vec<String> = children
                .iter()
                .map(|c| c[..c.len().min(10)].to_owned())
                .collect();
            let label = if children.len() > 1 {
                "Children"
            } else {
                "Child"
            };
            rows.push(line(label, short.join("  ")).into());
        }
        if let Some(rev) = extras["headRev"].as_str().filter(|r| !r.is_empty()) {
            rows.push(line("Rev", rev.to_owned()).into());
        }
    }
    if let Some(status) = signature_status(info, extras) {
        rows.push(line("Signature", status).into());
    }
    let refs = list("refs");
    if !refs.is_empty() {
        rows.push(line("Refs", refs.join(", ")).into());
    }
    if let Some(extras) = extras {
        for tag in extras["tagObjects"].as_array().into_iter().flatten() {
            let name = tag["name"].as_str().unwrap_or_default();
            let text = match tag["subject"].as_str().filter(|s| !s.is_empty()) {
                Some(subject) => format!("{name} — {subject}"),
                None if tag["id"].is_null() => format!("{name} (lightweight tag)"),
                None => name.to_owned(),
            };
            rows.push(line("Tag", text).into());
        }
        let branches = strings(&extras["branches"]);
        if !branches.is_empty() {
            rows.push(line("Branches", branches.join(", ")).into());
        }
        let count = extras["tagCount"].as_u64().unwrap_or(0) as usize;
        if count > 0 {
            let tags = strings(&extras["tags"]);
            let mut text = tags.iter().take(12).cloned().collect::<Vec<_>>().join(", ");
            if count > 12 {
                text.push_str(&format!(" and {} more tags", count - 12));
            }
            rows.push(line("Tags", text).into());
        }
        if let Some(previous) = extras["previousTag"].as_str() {
            rows.push(line("Previous Tag", previous.to_owned()).into());
        }
        if !extras["originUrl"].is_null() {
            let remote = strings(&extras["remoteBranches"]);
            let text = if remote.is_empty() {
                "Not pushed (not on any remote branch)".to_owned()
            } else {
                remote.join(", ")
            };
            rows.push(line("Pushed", text).into());
        }
    }
    let message = info["message"].as_str().unwrap_or_default().trim_end();
    let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
    rows.push(
        TextBlock::new()
            .text(subject)
            .font_size(14.0)
            .font_weight(FontWeight::SEMI_BOLD)
            .is_text_selection_enabled(true)
            .text_wrapping(TextWrapping::Wrap)
            .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
            .into(),
    );
    let body = body.trim();
    if !body.is_empty() {
        rows.push(
            TextBlock::new()
                .text(body)
                .font_size(13.0)
                .is_text_selection_enabled(true)
                .text_wrapping(TextWrapping::Wrap)
                .into(),
        );
    }
    StackPanel::new()
        .spacing(4.0)
        .margin(Thickness::new(14.0, 10.0, 14.0, 10.0))
        .children((View::keyed_fragment(rows.into_iter().enumerate()),))
}

/// 署名の行の文字（署名がなければ `None`）。言葉は GTK4 版の `signature::Status::label` と同じ（署名者の
/// メールとコミッターの照合はまだしない）。検証の結果は時間のかかる分で届く。
fn signature_status(info: &Value, extras: Option<&Value>) -> Option<String> {
    let kind = info["signed"].as_str();
    let check = extras.map(|e| &e["signature"]).filter(|s| !s.is_null());
    if kind.is_none() && check.is_none() {
        return None;
    }
    let label = match (extras, check) {
        (None, _) => "Checking…",
        (Some(_), None) => "No key",
        (Some(_), Some(check)) => match check["status"].as_str().unwrap_or_default() {
            "G" | "U" => {
                let trust = check["trust"].as_str().unwrap_or_default().to_lowercase();
                let trusted = kind == Some("ssh")
                    || matches!(trust.as_str(), "ultimate" | "full" | "fully" | "marginal");
                if check["status"] == "U" || !trusted {
                    "Untrusted key"
                } else {
                    "Verified"
                }
            }
            "X" | "Y" => "Expired",
            "R" => "Revoked key",
            "B" => "Bad signature",
            "V" => "Can’t verify",
            _ => "No key",
        },
    };
    let signer = check
        .and_then(|c| c["signer"].as_str())
        .filter(|s| !s.is_empty());
    Some(match signer {
        Some(signer) => format!("{label} — {signer}"),
        None => label.to_owned(),
    })
}

/// SwapChainPanel に swap chain をつなぐ（reactor が使う windows-core の型で渡す）。受け付けられたら `true`。
fn attach_chain(panel: &ElementRef<SwapChainPanel>, surface: &Surface) -> Result<bool, String> {
    use windows::core::Interface as _;
    let raw = surface
        .chain
        .cast::<windows::core::IUnknown>()
        .map_err(|e| e.to_string())?
        .into_raw();
    // SAFETY: 同じ COM のオブジェクトを、reactor が使う windows-core の型で持ち直すだけ。
    let unknown = unsafe { windows_core_reactor::IUnknown::from_raw(raw) };
    Ok(panel.request_set_swap_chain(unknown, |result| {
        if let Err(e) = result {
            eprintln!("SwapChainPanel に swap chain をつなげない: {e:?}");
        }
    }))
}

/// 大きさが変わった swap chain を作り直す（なければ作る）。
fn resize_surface(
    surface: &mut Option<Surface>,
    renderer: &Renderer,
    (width, height, scale): (f64, f64, f32),
    options: Options,
) -> Result<(), String> {
    let (w, h) = (
        (width * scale as f64).round() as u32,
        (height * scale as f64).round() as u32,
    );
    match surface.as_mut() {
        Some(s) => s.resize(renderer, w, h).map_err(|e| e.to_string())?,
        None => {
            *surface = Some(
                Surface::for_composition(renderer, w, h, options)
                    .map_err(|e| format!("swap chain を作れない: {e}"))?,
            );
        }
    }
    surface
        .as_ref()
        .expect("surface")
        .set_scale(scale)
        .map_err(|e| e.to_string())
}

/// 差分の欄。
struct DiffPane {
    panel: ElementRef<SwapChainPanel>,
    area: ElementRef<Grid>,
    surface: Option<Surface>,
    scale: f32,
    attached: bool,
    input_requested: bool,
    scroll_y: f32,
    dirty: bool,
}

impl DiffPane {
    fn resize(
        &mut self,
        renderer: &Renderer,
        width: f64,
        height: f64,
        scale: f32,
    ) -> Result<(), String> {
        self.scale = scale;
        // 差分は History の後に描くので、前のフレームを待たない。
        let options = Options {
            waitable: false,
            sync: 1,
            tearing: false,
        };
        resize_surface(&mut self.surface, renderer, (width, height, scale), options)?;
        if !self.attached {
            self.attached = attach_chain(&self.panel, self.surface.as_ref().expect("surface"))?;
        }
        self.dirty = true;
        Ok(())
    }

    fn height(&self) -> f32 {
        self.surface
            .as_ref()
            .map_or(0.0, |s| s.height as f32 / self.scale)
    }

    fn scroll_by(&mut self, dy: f32, rows: usize) {
        let max = (rows as f32 * DIFF_ROW_HEIGHT - self.height()).max(0.0);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max);
        self.dirty = true;
    }
}

struct HistoryView {
    session: Session,
    panel: ElementRef<SwapChainPanel>,
    area: ElementRef<Grid>,
    options: Options,
    attached: bool,
    input_requested: bool,
    diff: DiffPane,
    sender: LocalSender<Message>,
    /// サイドバーに送った ref（同じものは送り直さない）。
    refs_sent: Option<Arc<RefsSnapshot>>,
    /// 詳細と差分を読みに行った選択。
    selected: Option<u32>,
    /// 詳細と差分を出しているコミットの行。
    shown: Option<u32>,
}

impl HistoryView {
    fn on_event(&mut self, event: SwapChainPanelEvent) -> Result<(), String> {
        match event {
            SwapChainPanelEvent::Metrics {
                width,
                height,
                scale_x,
                ..
            } => {
                let s = &mut self.session;
                s.scale = scale_x;
                resize_surface(
                    &mut s.surface,
                    &s.renderer,
                    (width, height, scale_x),
                    self.options,
                )?;
                if !self.attached {
                    self.attached =
                        attach_chain(&self.panel, s.surface.as_ref().expect("surface"))?;
                }
                s.invalidate();
            }
            SwapChainPanelEvent::Rendering => self.render()?,
        }
        Ok(())
    }

    /// 届いたものを取り込み、描くものがあれば描く。
    fn render(&mut self) -> Result<(), String> {
        if !self.input_requested {
            self.input_requested = request_input(&self.area, Target::History);
        }
        if !self.diff.input_requested {
            self.diff.input_requested = request_input(&self.diff.area, Target::Diff);
        }
        let s = &mut self.session;
        s.poll()?;
        if let (Some(refs), Some(repo)) = (&s.data.refs, s.data.repo())
            && !self
                .refs_sent
                .as_ref()
                .is_some_and(|sent| Arc::ptr_eq(sent, refs))
        {
            self.refs_sent = Some(refs.clone());
            self.sender.send(Message::Refs(refs.clone(), repo));
        }
        // History が替わったら、選んでいる行の番号も変わる（同じコミットでも読み直す。差分も変わりうる）。
        if s.history_replaced {
            self.selected = None;
            self.shown = None;
        }
        // 開いて最初は HEAD のコミットを選ぶ（ref が届いてから）。
        if s.selected.is_none()
            && let Some(row) = s
                .data
                .refs
                .as_ref()
                .and_then(|r| r.refs.head)
                .and_then(|head| s.data.row_of(&head))
        {
            s.reveal(row);
        }
        // 選んだコミットが変わったら、詳細と差分を読む。
        if s.selected != self.selected {
            self.selected = s.selected;
            if let Some(row) = s.selected {
                s.data.want_commit(row);
            }
        }
        if let Some(commit) = &s.data.commit
            && self.shown != Some(commit.row)
        {
            self.shown = Some(commit.row);
            s.data.extras_changed = false;
            self.sender
                .send(Message::Commit(commit.info.clone(), commit.extras.clone()));
            s.renderer.forget_diff();
            self.diff.scroll_y = 0.0;
            self.diff.dirty = true;
        }
        if std::mem::take(&mut s.data.extras_changed)
            && let Some(commit) = &s.data.commit
        {
            self.sender
                .send(Message::Commit(commit.info.clone(), commit.extras.clone()));
        }
        if std::mem::take(&mut s.data.syntax_changed) {
            s.renderer.forget_diff();
            self.diff.dirty = true;
        }
        if self.diff.dirty
            && let Some(surface) = self.diff.surface.as_mut()
        {
            s.renderer
                .draw_diff(
                    surface,
                    s.data.commit.as_deref(),
                    self.diff.scroll_y,
                    self.diff.scale,
                )
                .map_err(|e| format!("差分を描けない: {e}"))?;
            surface.present().map_err(|e| format!("出せない: {e}"))?;
            self.diff.dirty = false;
        }
        if !s.needs_frame() {
            return Ok(());
        }
        // 前のフレームが出るのを待つ（最大 1 フレーム。UI のスレッドなので長くは待たない）。
        if let Some(h) = s.surface.as_ref().and_then(|s| s.waitable)
            && unsafe { WaitForSingleObjectEx(h, 20, false) } != WAIT_OBJECT_0
        {
            return Ok(());
        }
        if s.frame()? {
            std::process::exit(0);
        }
        Ok(())
    }

    fn select_commit(&mut self, id: &amonhen_model::Oid) {
        if let Some(row) = self.session.data.row_of(id) {
            self.session.reveal(row);
        }
    }

    /// メニューの操作（`menus::IMPLEMENTED` のもの）。
    fn command(&mut self, id: &str) {
        let s = &mut self.session;
        match id {
            "refresh" => s.data.reload(),
            "rebuild-history" => s.data.rebuild(),
            "go-to-head" => {
                if let Some(head) = s.data.refs.as_ref().and_then(|r| r.refs.head) {
                    self.select_commit(&head);
                }
            }
            // 選んでいるコミットの最初の親・最初の子へ（GTK4 版の `go_to_relative`）。
            "go-to-parent" => {
                let parent = s.selected.and_then(|row| {
                    let history = s.data.history.as_ref()?;
                    history.graph.parents(row as usize).first().copied()
                });
                if let Some(row) = parent {
                    s.reveal(row);
                }
            }
            "go-to-child" => {
                let child = s
                    .data
                    .commit
                    .as_ref()
                    .and_then(|c| c.extras.as_ref())
                    .and_then(|e| e["children"][0].as_str())
                    .and_then(amonhen_model::Oid::from_hex);
                if let Some(child) = child {
                    self.select_commit(&child);
                }
            }
            // History しかまだないので、出しているまま。
            "show-history" => {}
            _ => {}
        }
    }

    fn diff_rows(&self) -> usize {
        self.session
            .data
            .commit
            .as_ref()
            .map_or(0, |c| c.diff.rows.len())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    History,
    Diff,
}

/// `area` の生の要素に入力を付ける（まだ載っていなければ `false`、次のフレームでまた試す）。
fn request_input(area: &ElementRef<Grid>, target: Target) -> bool {
    area.request_native_element(move |result| match result {
        Ok(element) => {
            if let Err(e) = attach_input(&element, target) {
                eprintln!("入力を付けられない: {e}");
            }
        }
        Err(e) => eprintln!("部品を取れない: {e:?}"),
    })
}

/// History か差分を包む Grid に、押す・ホイール・キーを付け、キーを受けられるようにする。
fn attach_input(element: &IInspectable, target: Target) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    ui.SetIsTabStop(true)?;
    let mut revokers = Vec::new();

    let source = ui.clone();
    revokers.push(
        ui.PointerPressed(move |_, args: Ref<xaml::PointerRoutedEventArgs>| {
            let Some(args) = args.as_ref() else { return };
            let Ok(point) = args.GetCurrentPoint(&source) else {
                return;
            };
            let (Ok(position), Ok(properties)) = (point.Position(), point.Properties()) else {
                return;
            };
            if !properties.IsLeftButtonPressed().unwrap_or(false) {
                return;
            }
            let _ = source.Focus(xaml::FocusState::Pointer);
            with_history(|h| match target {
                Target::History => {
                    if let Some(row) = h.session.row_at(position.Y) {
                        h.session.select(row);
                    }
                }
                // ファイルの見出しを押すと、そのファイルを畳む・開く。
                Target::Diff => {
                    let row = ((h.diff.scroll_y + position.Y) / DIFF_ROW_HEIGHT).floor();
                    if row >= 0.0 && h.session.data.toggle_file(row as usize) {
                        h.session.renderer.forget_diff();
                        let rows = h.diff_rows();
                        h.diff.scroll_by(0.0, rows);
                    }
                }
            });
            let _ = args.SetHandled(true);
        })?,
    );

    revokers.push(
        ui.PointerWheelChanged(move |_, args: Ref<xaml::PointerRoutedEventArgs>| {
            let Some(args) = args.as_ref() else { return };
            let Ok(properties) = args
                .GetCurrentPoint(None::<&xaml::UIElement>)
                .and_then(|p| p.Properties())
            else {
                return;
            };
            if properties.IsHorizontalMouseWheel().unwrap_or(false) {
                return;
            }
            let notches = -(properties.MouseWheelDelta().unwrap_or(0) as f32) / 120.0;
            with_history(|h| match target {
                Target::History => h.session.scroll_by(notches * WHEEL_ROWS * ROW_HEIGHT),
                Target::Diff => {
                    let rows = h.diff_rows();
                    h.diff
                        .scroll_by(notches * WHEEL_ROWS * DIFF_ROW_HEIGHT, rows);
                }
            });
            let _ = args.SetHandled(true);
        })?,
    );

    revokers.push(ui.KeyDown(move |_, args: Ref<xaml::KeyRoutedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        let Ok(key) = args.Key() else { return };
        let handled = std::cell::Cell::new(true);
        with_history(|h| match target {
            Target::History => {
                let s = &mut h.session;
                match key {
                    xaml::VirtualKey::Up => s.move_selection(-1),
                    xaml::VirtualKey::Down => s.move_selection(1),
                    xaml::VirtualKey::PageUp => s.move_selection(-s.page_rows()),
                    xaml::VirtualKey::PageDown => s.move_selection(s.page_rows()),
                    xaml::VirtualKey::Home => s.move_selection(-(s.data.row_count() as i64)),
                    xaml::VirtualKey::End => s.move_selection(s.data.row_count() as i64),
                    _ => handled.set(false),
                }
            }
            Target::Diff => {
                let rows = h.diff_rows();
                let page = (h.diff.height() - DIFF_ROW_HEIGHT).max(DIFF_ROW_HEIGHT);
                let total = rows as f32 * DIFF_ROW_HEIGHT;
                let dy = match key {
                    xaml::VirtualKey::Up => -DIFF_ROW_HEIGHT,
                    xaml::VirtualKey::Down => DIFF_ROW_HEIGHT,
                    xaml::VirtualKey::PageUp => -page,
                    xaml::VirtualKey::PageDown | xaml::VirtualKey::Space => page,
                    xaml::VirtualKey::Home => -total,
                    xaml::VirtualKey::End => total,
                    _ => {
                        handled.set(false);
                        0.0
                    }
                };
                h.diff.scroll_by(dy, rows);
            }
        });
        if handled.get() {
            let _ = args.SetHandled(true);
        }
    })?);

    REVOKERS.with(|r| r.borrow_mut().extend(revokers));
    Ok(())
}
