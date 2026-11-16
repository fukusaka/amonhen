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

use amonhen_core::{RefsSnapshot, Repository, StatusSnapshot};
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
    /// History を包む生の要素（右クリックのメニューを自分で開く）。
    static HISTORY_ELEMENT: RefCell<Option<xaml::UIElement>> = const { RefCell::new(None) };
    /// History から外した右クリックのメニュー（外している間、WinUI は自分では開かない）。
    static HISTORY_FLYOUT: RefCell<Option<xaml::FlyoutBase>> = const { RefCell::new(None) };
    /// サイドバーの木の生の要素と、右クリックで開く位置（キーボードで開いたときは項目そのものに出す）。
    static SIDEBAR_ELEMENT: RefCell<Option<xaml::UIElement>> = const { RefCell::new(None) };
    /// サイドバーのメニューを付けておく、押されない空の Grid。
    static SIDEBAR_MENU_HOST: RefCell<Option<xaml::UIElement>> = const { RefCell::new(None) };
    static SIDEBAR_AT: RefCell<Option<(xaml::UIElement, Option<xaml::Point>)>> =
        const { RefCell::new(None) };
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
    /// 窓の根の Grid（メニューのショートカットを、フォーカスのある部品より先に受ける）。
    root: ElementRef<Grid>,
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
    /// History で選んでいるコミット（ID と、指している ref）。右クリックのメニューを作る。
    selected: Option<(String, Vec<crate::data::RefLabel>)>,
    /// History の右クリックのメニューを開いた回数（変わったら、描き直した後に開く）。
    menu_request: u64,
    /// サイドバーの木と、右クリックした項目のキー・開いた回数（History と同じく、作り直してから開く）。
    sidebar_tree: ElementRef<Grid>,
    sidebar_menu_host: ElementRef<Grid>,
    sidebar_key: Option<String>,
    sidebar_request: u64,
    /// 出している画面（サイドバーの Workspace）。
    page: Page,
    repo: Option<Arc<Repository>>,
    status: Option<Arc<StatusSnapshot>>,
    /// Working Copy のコミットの件名と本文。
    summary: String,
    description: String,
    committing: bool,
    amend: bool,
    /// History の検索の欄（出しているか、問い合わせ、件数の文字）。
    search_open: bool,
    query: String,
    search_status: String,
    search_box: ElementRef<TextBox>,
    /// 検索の欄を開いた回数（開いた後にフォーカスとキーを付ける）。
    search_opened: u64,
    /// Working Copy の一覧で選んでいるファイル（パス）。
    working_file: Option<String>,
    /// 走っている操作（ステータスバーに出す）。
    busy: Option<String>,
    /// Stashes の画面で選んでいる stash の名前。
    stash: Option<String>,
    /// File History の画面（パス、読んだコミット、選んでいる行）。
    file_history: Option<crate::filehistory::State>,
    /// Blame の画面（パス、コミット、読んだ行）。閉じたら前の画面（File History なら File History）に戻る。
    blame: Option<crate::blame::State>,
    blame_from: Page,
    /// 途中の操作（`sequence` の答えの info）。なければ None。
    sequence: Option<Value>,
    /// ダイアログの入力の欄（Enter で既定のボタンを押す）と、開いた回数（開いた後にキーを付ける）。
    dialog_input: ElementRef<TextBox>,
    dialog_inputs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    History,
    WorkingCopy,
    Stashes,
    FileHistory,
    Blame,
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
    /// Fetch の確認（リモートが 2 つ以上なら、どこから取るかを選ぶ。0 はすべて）。
    Fetch {
        repo: Arc<Repository>,
        remotes: Vec<String>,
        from: usize,
    },
    /// Pull の確認（上流から取り込む。取り込み方を選ぶ）。
    Pull {
        repo: Arc<Repository>,
        title: String,
        mode: usize,
        /// 上流がなければ、取り込み元のリモートのブランチ（同じ名前のものから）と選んだもの。
        branch: String,
        sources: Vec<String>,
        source: usize,
    },
    /// Push の確認（上流へ送る）。`replace` なら Force Push（上流が `expect` のときだけ置き換える）。`diverged` は、
    /// 上流に手元にないコミットがあるので、Pull… か Force Push を選んでもらう。
    Push {
        repo: Arc<Repository>,
        title: String,
        message: String,
        remote: String,
        branch: String,
        remote_branch: String,
        replace: bool,
        diverged: bool,
        expect: Option<String>,
    },
    /// 上流のないブランチを初めて送る（Publish。GTK4 版の `sheets::publish_in`）。
    Publish {
        repo: Arc<Repository>,
        branch: String,
        remotes: Vec<String>,
        remote: usize,
        name: String,
    },
    /// 新しいブランチの名前を聞く（`start` から作って切り替える）。
    NewBranch {
        repo: Arc<Repository>,
        start: String,
        name: String,
        /// ブランチではなくタグを作る（New Tag at …）。
        tag: bool,
    },
    /// リモートの追加（`original` が None）・編集（GTK4 版の `remotesheet::present`）。接続の確かめはまだない。
    Remote {
        repo: Arc<Repository>,
        /// 編集するリモート（名前、fetch の URL、push の URL）。
        original: Option<(String, String, String)>,
        name: String,
        fetch: String,
        push: String,
    },
    /// stash から新しいブランチを作る（`git stash branch`。GTK4 版の `stashes::branch`）。
    StashBranch {
        repo: Arc<Repository>,
        stash: String,
        name: String,
    },
    /// コンフリクトが残っているファイルを解決済みにしてよいか。
    MarkResolved { path: String, count: u64 },
    /// ブランチの名前を変える（GTK4 版の `refops::ask_rename_branch`）。
    Rename {
        repo: Arc<Repository>,
        from: String,
        name: String,
    },
    /// 確かめてから命令を実行する（Cherry-pick・Revert など。GTK4 版の `sheets::pick` ほか）。
    Confirm {
        repo: Arc<Repository>,
        title: String,
        message: String,
        button: String,
        command: Value,
    },
    /// Checkout の確認（GTK4 版の `sheets::checkout`）。
    Checkout {
        repo: Arc<Repository>,
        title: String,
        message: String,
        command: Value,
    },
    /// 今のブランチを `oid` に動かす（GTK4 版の `commitops::ask_reset`。動かし方は Soft・Mixed・Hard、既定は
    /// Mixed）。前後の絵・今との違いの行き先・Force Push の注意はまだない。
    Reset {
        repo: Arc<Repository>,
        title: String,
        oid: String,
        mode: usize,
    },
    /// Merge・Rebase（GTK4 版の `sheets::integrate_sheet`）。`choices` は名前と rev。2 つ以上なら相手を選ぶ。
    /// merge なら取り込み方も選ぶ。前後の絵・コンフリクトの見込みはまだない。
    Integrate {
        repo: Arc<Repository>,
        rebase: bool,
        current: String,
        choices: Vec<(String, String)>,
        /// 相手を選ぶ欄を出す（メニューから）。でなければ `choices` の最初の相手を問う。
        chooser: bool,
        chosen: usize,
        mode: usize,
    },
    /// Working Copy のファイルの変更を捨てる（untracked なら消す）確認。
    Discard {
        repo: Arc<Repository>,
        path: String,
        untracked: bool,
    },
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
    /// History で選んでいるコミットが変わった。
    Selected(Option<(String, Vec<crate::data::RefLabel>)>),
    /// History の右クリックのメニューの項目を選んだ（項目のキー）。
    HistoryMenu(String),
    /// History の右クリックのメニューを開く（選んだ行のメニューに作り直してから）。
    OpenHistoryMenu,
    /// サイドバーの項目を右クリックした（項目のキー）。
    SidebarMenu(String),
    /// Stashes の一覧で選んだ行。
    StashSelected(Option<usize>),
    /// File History を開く（パス）・読み終えた・一覧で選んだ・閉じる。
    OpenFileHistory(String),
    FileHistoryLoaded(String, Result<Vec<crate::filehistory::Entry>, String>),
    FileHistorySelected(Option<usize>),
    CloseFileHistory,
    /// File History の行の右クリック（`blame:<oid>`）。
    FileHistoryMenu(String),
    /// Blame を開く（パス、コミット。なければ作業ツリー）・読み終えた・閉じる・行の右クリック。
    OpenBlame(String, Option<String>),
    BlameLoaded(String, Option<String>, Result<crate::blame::Data, String>),
    CloseBlame,
    BlameMenu(String),
    /// Working Copy のファイルの右クリック（パス、項目のキー）。
    WorkingFileMenu(String, String),
    /// 途中の操作を読んだ。
    Sequence(Option<Value>),
    /// 解決済みにする前に、残っているコンフリクトを数えた（パス、数）。
    ConflictCounted(String, u64),
    /// Pull の取り込み元・Publish の送り先のリモートを選んだ。
    PullSource(Option<usize>),
    PublishRemote(Option<usize>),
    /// Publish の送り先を調べ終えた（そのブランチがリモートにあれば、送るコミットと取り込んでいないコミットの数）。
    PublishChecked(Arc<Repository>, String, String, String, Option<(u64, u64)>),
    /// 編集するリモートを読んだ（なければ None）。
    RemoteLoaded(Arc<Repository>, Option<(String, String, String)>),
    /// リモートのシートの欄（0 は名前、1 は fetch、2 は push の URL）。
    RemoteField(usize, String),
    /// 消すブランチを調べ終えた（名前、ほかのブランチにないコミットの数と一覧、または調べられなかった理由）。
    DeleteChecked(Arc<Repository>, String, Result<(u64, Vec<String>), String>),
    Status(Arc<StatusSnapshot>),
    /// Working Copy の件名・本文が変わった。
    Summary(String),
    Description(String),
    /// ファイルを次のコミットに含める・外す。
    Stage(String, bool),
    /// Working Copy の Commit を押した。
    CommitChanges,
    /// コミットが終わった（失敗ならその文）。
    Committed(Option<String>),
    /// 検索の欄の問い合わせが変わった。
    Query(String),
    /// 検索の欄で Enter（`true` なら Shift+Enter で前へ）。
    SearchSubmit(bool),
    CloseSearch,
    /// 件数の文字（「N / M」「No Matches」）。
    SearchStatus(String),
    /// Amend を切り替えた。
    Amend(bool),
    /// Amend にしたときの、HEAD のメッセージ（件名と本文）。
    HeadCommitText(String),
    /// Working Copy のファイルの右クリックのメニューの Discard File（パス）。
    DiscardFile(String),
    /// Working Copy の一覧で選んだファイル（何番目か）。
    WorkingFile(Option<usize>),
    /// 新しいブランチの名前が変わった。
    BranchName(String),
    /// Pull のダイアログで、取り込み方を選んだ。
    PullMode(Option<usize>),
    /// Fetch のダイアログに出すリモート（なければ空）。
    Remotes(Arc<Repository>, Vec<String>),
    /// Fetch のダイアログで、取るリモートを選んだ（0 はすべて）。
    FetchFrom(Option<usize>),
    /// Reset の動かし方・Merge の相手・Merge の取り込み方を選んだ。
    ResetMode(Option<usize>),
    IntegrateRev(Option<usize>),
    MergeMode(Option<usize>),
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
            root: ElementRef::new(),
            panel: ElementRef::new(),
            area: ElementRef::new(),
            diff_panel: ElementRef::new(),
            diff_area: ElementRef::new(),
            refs: None,
            groups: None,
            info: None,
            error: None,
            dialog: None,
            selected: None,
            menu_request: 0,
            sidebar_tree: ElementRef::new(),
            sidebar_menu_host: ElementRef::new(),
            sidebar_key: None,
            stash: None,
            file_history: None,
            blame: None,
            blame_from: Page::History,
            sequence: None,
            sidebar_request: 0,
            page: Page::History,
            repo: None,
            status: None,
            summary: String::new(),
            description: String::new(),
            committing: false,
            amend: false,
            search_open: false,
            query: String::new(),
            search_status: String::new(),
            search_box: ElementRef::new(),
            search_opened: 0,
            working_file: None,
            busy: None,
            dialog_input: ElementRef::new(),
            dialog_inputs: 0,
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Refs(refs, repo) => {
                self.refs = Some(refs);
                self.repo = Some(repo.clone());
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
            Message::Command(id) if id == "pull" => self.pull(),
            Message::Command(id) if id == "new-branch" => {
                // 選んでいるコミットから（なければ今のブランチから。GTK4 版の `start_point`）。
                let start = self
                    .selected
                    .as_ref()
                    .map(|(id, _)| id.clone())
                    .or_else(|| self.refs.as_ref()?.refs.head_branch.clone())
                    .unwrap_or_else(|| "HEAD".to_owned());
                self.new_branch(start);
            }
            Message::BranchName(name) => match &mut self.dialog {
                Some(Dialog::NewBranch { name: current, .. })
                | Some(Dialog::Rename { name: current, .. })
                | Some(Dialog::StashBranch { name: current, .. })
                | Some(Dialog::Publish { name: current, .. }) => *current = name,
                _ => {}
            },
            Message::Command(id) if id == "push" => self.push(false),
            Message::Command(id) if id == "force-push" => self.push(true),
            Message::PullSource(Some(index)) => {
                if let Some(Dialog::Pull { source, .. }) = &mut self.dialog {
                    *source = index;
                }
            }
            Message::PublishRemote(Some(index)) => {
                if let Some(Dialog::Publish { remote, .. }) = &mut self.dialog {
                    *remote = index;
                }
            }
            Message::PullSource(None) | Message::PublishRemote(None) => {}
            Message::PublishChecked(repo, branch, remote, remote_branch, exists) => {
                let push = json!({
                    "op": "push", "remote": remote, "branch": branch, "remoteBranch": remote_branch,
                    "force": false, "setUpstream": true,
                });
                let there = format!("{remote}/{remote_branch}");
                let set_upstream = vec![
                    json!({ "op": "configSet", "key": format!("branch.{branch}.remote"), "value": remote }),
                    json!({ "op": "configSet", "key": format!("branch.{branch}.merge"), "value": format!("refs/heads/{remote_branch}") }),
                ];
                // GTK4 版と同じ: 送り先にもうそのブランチがあれば、作るのではなくそれを上流にする。
                match exists {
                    None => {
                        self.busy = Some("Publish…".to_owned());
                        perform(context, repo, push, "Publish".to_owned());
                    }
                    Some((_, incoming)) if incoming > 0 => {
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!("{target} already exists", target = &there),
                            message: tr!(
                                "It has {count} commits that {branch} doesn’t (as of the last Fetch), so pushing would be refused. Make it the upstream and Pull first.",
                                count = incoming,
                                branch = &branch
                            ),
                            button: tr!("Set Upstream"),
                            command: Value::Array(set_upstream),
                        })
                    }
                    Some((outgoing, _)) => {
                        let (message, button, command) = if outgoing > 0 {
                            (
                                tr!(
                                    "This pushes {count} commits to it and makes it the upstream.",
                                    count = outgoing
                                ),
                                "Push".to_owned(),
                                push,
                            )
                        } else {
                            (
                                tr!(
                                    "It is the same as {branch}. This makes it the upstream.",
                                    branch = &branch
                                ),
                                tr!("Set Upstream"),
                                Value::Array(set_upstream),
                            )
                        };
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!("{target} already exists", target = &there),
                            message,
                            button,
                            command,
                        })
                    }
                }
            }
            Message::PullMode(index) => {
                if let (Some(Dialog::Pull { mode, .. }), Some(index)) = (&mut self.dialog, index) {
                    *mode = index;
                }
            }
            Message::Command(id) if id == "fetch" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                context.spawn_background(move |_| {
                    let reply: Value = serde_json::from_str(&repo.run_json(r#"{"op":"remotes"}"#))
                        .unwrap_or_default();
                    let remotes = reply["info"]["remotes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|r| r["name"].as_str().map(str::to_owned))
                        .collect();
                    Message::Remotes(repo, remotes)
                });
            }
            Message::Remotes(repo, remotes) => {
                if remotes.is_empty() {
                    beep();
                } else {
                    self.dialog = Some(Dialog::Fetch {
                        repo,
                        remotes,
                        from: 0,
                    });
                }
            }
            Message::ResetMode(Some(index)) => {
                if let Some(Dialog::Reset { mode, .. }) = &mut self.dialog {
                    *mode = index;
                }
            }
            Message::IntegrateRev(Some(index)) => {
                if let Some(Dialog::Integrate { chosen, .. }) = &mut self.dialog {
                    *chosen = index;
                }
            }
            Message::MergeMode(Some(index)) => {
                if let Some(Dialog::Integrate { mode, .. }) = &mut self.dialog {
                    *mode = index;
                }
            }
            Message::ResetMode(None) | Message::IntegrateRev(None) | Message::MergeMode(None) => {}
            Message::Command(id) if id == "merge" || id == "rebase" => {
                self.integrate_choosing(id == "rebase")
            }
            Message::FetchFrom(index) => {
                if let (Some(Dialog::Fetch { from, .. }), Some(index)) = (&mut self.dialog, index) {
                    *from = index;
                }
            }
            Message::Command(id) if id == "show-working-copy" => self.show_page(Page::WorkingCopy),
            Message::Command(id) if id == "show-history" => self.show_page(Page::History),
            Message::Command(id) if id == "find-in-history" => {
                // History に移って、その検索の欄へ（GTK4 版・macOS 版と同じ）。
                self.show_page(Page::History);
                if self.search_open {
                    let _ = self.search_box.request_focus();
                } else {
                    self.search_open = true;
                    self.search_opened += 1;
                }
            }
            Message::Query(text) => self.query = text,
            Message::SearchSubmit(backwards) => {
                let query = self.query.trim().to_owned();
                with_history(|h| h.search(query, backwards));
            }
            Message::CloseSearch => {
                self.search_open = false;
                self.search_status.clear();
                with_history(|h| h.search(String::new(), false));
            }
            Message::SearchStatus(text) => self.search_status = text,
            Message::Command(id) if matches!(id.as_str(), "continue" | "skip" | "abort") => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                let title = match id.as_str() {
                    "continue" => "Continue",
                    "skip" => "Skip",
                    _ => "Abort",
                };
                self.busy = Some(format!("{title}…"));
                perform(context, repo, json!({ "op": id }), title.to_owned());
            }
            Message::Command(id) => with_history(|h| h.command(&id)),
            Message::UndoPeek(repo, peek) => match peek {
                Some((label, what)) => self.dialog = Some(Dialog::Undo { repo, label, what }),
                None => beep(),
            },
            Message::DialogClosed(result) => match (self.dialog.take(), result) {
                (Some(Dialog::Undo { repo, .. }), ContentDialogResult::Primary) => {
                    perform(context, repo, json!({ "op": "undo" }), tr!("Couldn’t Undo"));
                }
                (
                    Some(Dialog::NewBranch {
                        repo,
                        start,
                        name,
                        tag,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let name = name.trim().to_owned();
                    if !name.is_empty() {
                        let command = if tag {
                            json!({ "op": "createTag", "name": name, "target": start, "message": null })
                        } else {
                            json!({ "op": "createBranch", "name": name, "start": start, "checkout": true })
                        };
                        perform(context, repo, command, tr!("Create"));
                    }
                }
                (
                    Some(Dialog::Confirm {
                        repo,
                        command,
                        button,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => match command {
                    Value::Array(commands) => perform_all(context, repo, commands, button),
                    command => perform(context, repo, command, button),
                },
                (
                    Some(Dialog::Reset {
                        repo, oid, mode, ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let mode = RESET_MODES[mode];
                    perform(
                        context,
                        repo,
                        json!({ "op": "reset", "oid": oid, "mode": mode }),
                        "Reset".to_owned(),
                    );
                }
                (
                    Some(Dialog::Integrate {
                        repo,
                        rebase,
                        choices,
                        chosen,
                        mode,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let Some((_, rev)) = choices.get(chosen) else {
                        return;
                    };
                    let (command, button) = if rebase {
                        (json!({ "op": "rebase", "onto": rev }), "Rebase")
                    } else {
                        let mut command = json!({ "op": "merge", "rev": rev });
                        if MERGE_MODES[mode] != "git" {
                            command["mode"] = json!(MERGE_MODES[mode]);
                        }
                        (command, "Merge")
                    };
                    self.busy = Some(format!("{button}…"));
                    perform(context, repo, command, button.to_owned());
                }
                (
                    Some(Dialog::Remote {
                        repo,
                        original,
                        name,
                        fetch,
                        push,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let title = if original.is_some() {
                        tr!("Edit Remote")
                    } else {
                        tr!("Add Remote")
                    };
                    let commands =
                        remote_changes(original.as_ref(), name.trim(), fetch.trim(), push.trim());
                    perform_all(context, repo, commands, title);
                }
                (Some(Dialog::StashBranch { repo, stash, name }), ContentDialogResult::Primary) => {
                    let branch = name.trim().to_owned();
                    if !branch.is_empty() {
                        perform(
                            context,
                            repo,
                            json!({ "op": "stashBranch", "name": stash, "branch": branch }),
                            tr!("Create"),
                        );
                    }
                }
                (Some(Dialog::MarkResolved { path, .. }), ContentDialogResult::Primary) => {
                    self.update(Message::Stage(path, true), context);
                }
                (Some(Dialog::Rename { repo, from, name }), ContentDialogResult::Primary) => {
                    let to = name.trim().to_owned();
                    if !to.is_empty() && to != from {
                        perform(
                            context,
                            repo,
                            json!({ "op": "renameBranch", "from": from, "to": to }),
                            tr!("Rename"),
                        );
                    }
                }
                (Some(Dialog::Checkout { repo, command, .. }), ContentDialogResult::Primary) => {
                    perform(context, repo, command, "Checkout".to_owned());
                }
                (
                    Some(Dialog::Pull {
                        repo,
                        mode,
                        branch,
                        sources,
                        source,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    self.busy = Some("Pull…".to_owned());
                    let pull = json!({ "op": "pull", "mode": PULL_MODES[mode] });
                    // 上流がなければ、選んだリモートのブランチを上流にしてから取り込む（GTK4 版と同じ）。
                    let commands = match sources.get(source).and_then(|s| s.split_once('/')) {
                        Some((remote, name)) => vec![
                            json!({ "op": "configSet", "key": format!("branch.{branch}.remote"), "value": remote }),
                            json!({ "op": "configSet", "key": format!("branch.{branch}.merge"), "value": format!("refs/heads/{name}") }),
                            pull,
                        ],
                        None => vec![pull],
                    };
                    perform_all(context, repo, commands, "Pull".to_owned());
                }
                (
                    Some(Dialog::Publish {
                        repo,
                        branch,
                        remotes,
                        remote,
                        name,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let (Some(remote), remote_branch) =
                        (remotes.get(remote).cloned(), name.trim().to_owned())
                    else {
                        return;
                    };
                    if remote_branch.is_empty() {
                        return;
                    }
                    // その名前のブランチがもうリモートにあるか（最後の Fetch の時点）と、離れ具合を裏で調べる。
                    let there = format!("{remote}/{remote_branch}");
                    let exists = self.refs.as_ref().is_some_and(|r| {
                        r.refs
                            .entries
                            .iter()
                            .any(|e| e.kind == amonhen_model::RefKind::Remote && e.name == there)
                    });
                    context.spawn_background(move |_| {
                        let count = |base: &str, tip: &str| -> u64 {
                            let command =
                                json!({ "op": "between", "base": base, "tip": tip, "limit": 1 });
                            let reply: Value =
                                serde_json::from_str(&repo.run_json(&command.to_string()))
                                    .unwrap_or_default();
                            reply["info"]["count"].as_u64().unwrap_or(0)
                        };
                        let counts =
                            exists.then(|| (count(&there, &branch), count(&branch, &there)));
                        Message::PublishChecked(repo, branch, remote, remote_branch, counts)
                    });
                }
                (
                    Some(Dialog::Push {
                        repo,
                        diverged: true,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    // 上流に手元にないコミットがある: Pull… を選んだ。閉じたダイアログを同じ更新で開き直すと
                    // reactor は開いたままとみなして出さないので、閉じた後の知らせで開く。
                    let _ = repo;
                    context.spawn_background(|_| Message::Command("pull".to_owned()));
                }
                (
                    Some(Dialog::Push {
                        repo,
                        remote,
                        branch,
                        remote_branch,
                        replace,
                        diverged,
                        expect,
                        ..
                    }),
                    result,
                ) if (result == ContentDialogResult::Primary && !diverged)
                    || (result == ContentDialogResult::Secondary && diverged) =>
                {
                    // 上流が先に進んでいるときは、Force Push は 2 つ目のボタン（Enter では押せない）。
                    let replace = replace || diverged;
                    self.busy = Some(if replace { "Force Push…" } else { "Push…" }.to_owned());
                    perform(
                        context,
                        repo,
                        json!({
                            "op": "push", "remote": remote, "branch": branch,
                            "remoteBranch": remote_branch, "force": replace,
                            "expect": if replace { expect } else { None },
                            "setUpstream": false,
                        }),
                        if replace { "Force Push" } else { "Push" }.to_owned(),
                    );
                }
                (
                    Some(Dialog::Fetch {
                        repo,
                        remotes,
                        from,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let remote = from.checked_sub(1).and_then(|i| remotes.get(i).cloned());
                    self.busy = Some("Fetch…".to_owned());
                    perform(
                        context,
                        repo,
                        json!({ "op": "fetch", "remote": remote }),
                        "Fetch".to_owned(),
                    );
                }
                (
                    Some(Dialog::Discard {
                        repo,
                        path,
                        untracked,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let op = if untracked {
                        "deleteUntracked"
                    } else {
                        "discard"
                    };
                    perform(
                        context,
                        repo,
                        json!({ "op": op, "paths": [path] }),
                        "Discard File".to_owned(),
                    );
                }
                _ => {}
            },
            Message::Selected(selected) => self.selected = selected,
            Message::OpenHistoryMenu => self.menu_request += 1,
            Message::RemoteLoaded(repo, found) => match found {
                Some((name, fetch, push)) => {
                    // push の URL は、fetch と違うときだけ欄に出す（同じなら空。GTK4 版と同じ）。
                    let shown_push = if push != fetch {
                        push.clone()
                    } else {
                        String::new()
                    };
                    self.dialog = Some(Dialog::Remote {
                        repo,
                        original: Some((name.clone(), fetch.clone(), push)),
                        name,
                        fetch,
                        push: shown_push,
                    });
                    self.dialog_inputs += 1;
                }
                None => beep(),
            },
            Message::RemoteField(index, text) => {
                if let Some(Dialog::Remote {
                    name, fetch, push, ..
                }) = &mut self.dialog
                {
                    *[name, fetch, push].into_iter().nth(index).expect("field") = text;
                }
            }
            Message::DeleteChecked(repo, name, checked) => match checked {
                Err(message) => {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Couldn’t check the branches"),
                        message,
                    })
                }
                Ok((count, commits)) => {
                    // GTK4 版の `sheets::delete_branches`: ほかのブランチにないコミットがあれば並べ、Delete Anyway で消す。
                    let mut message = String::new();
                    if count > 0 {
                        message.push_str(&tr!(
                            "Some commits are on no other branch; they go with the branch."
                        ));
                        message.push(' ');
                    }
                    message.push_str(&tr!("You can Undo this (Ctrl+Alt+Z)."));
                    if count > 0 {
                        message.push_str(&format!(
                            "\n\n{}\n{}",
                            tr!("Commits only on {branch}", branch = &name),
                            commits.join("\n")
                        ));
                        if count > commits.len() as u64 {
                            message.push_str("\n…");
                        }
                    }
                    self.dialog = Some(Dialog::Confirm {
                        repo,
                        title: tr!("Delete {name}?", name = &name),
                        message,
                        button: if count > 0 {
                            tr!("Delete Anyway")
                        } else {
                            tr!("Delete")
                        },
                        command: json!({ "op": "deleteBranch", "name": name, "force": count > 0 }),
                    });
                }
            },
            Message::SidebarMenu(key) => {
                // 操作のない項目（見出し・Workspace のページ）では開かない。
                self.sidebar_key = Some(key);
                if self.sidebar_menu().is_empty() {
                    self.sidebar_key = None;
                } else {
                    self.sidebar_request += 1;
                }
            }
            Message::Status(status) => {
                self.status = Some(status);
                // 途中の操作も読み直す（merge・rebase などは status と一緒に変わる）。
                if let Some(repo) = self.repo.clone() {
                    context.spawn_background(move |_| {
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(r#"{"op":"sequence"}"#))
                                .unwrap_or_default();
                        Message::Sequence((!reply["text"].is_null()).then(|| reply["info"].clone()))
                    });
                }
                // 選んでいるファイルの差分も読み直す（stage で行の印が、外での変更で中身が変わる）。
                self.load_working_diff();
            }
            Message::Summary(text) => self.summary = text,
            Message::Description(text) => self.description = text,
            Message::Stage(path, include) => {
                let Some(repo) = self.repo.clone() else {
                    return;
                };
                context.spawn_background(move |_| {
                    let result = if include {
                        repo.stage(&[&path])
                    } else {
                        repo.unstage(&[&path])
                    };
                    with_status_reload();
                    Message::Performed(result.err().map(|e| (tr!("Couldn’t Stage"), e.to_string())))
                });
            }
            Message::CommitChanges => {
                let Some(repo) = self.repo.clone() else {
                    return;
                };
                let message = crate::workcopy::message(&self.summary, &self.description);
                let amend = self.amend;
                self.committing = true;
                context.spawn_background(move |_| {
                    Message::Committed(repo.commit(&message, amend).err().map(|e| e.to_string()))
                });
            }
            Message::Amend(amend) => {
                self.amend = amend;
                // Amend にして欄が空なら、HEAD のメッセージを入れる（GTK4 版の `fill_head_message`）。
                if amend
                    && self.summary.is_empty()
                    && self.description.is_empty()
                    && let Some(repo) = self.repo.clone()
                {
                    context.spawn_background(move |_| {
                        let reply: Value = serde_json::from_str(
                            &repo.run_json(r#"{"op":"commitMessage","oid":"HEAD"}"#),
                        )
                        .unwrap_or_default();
                        Message::HeadCommitText(
                            reply["text"].as_str().unwrap_or_default().to_owned(),
                        )
                    });
                }
            }
            Message::HeadCommitText(text) => {
                if self.amend && self.summary.is_empty() && self.description.is_empty() {
                    let text = text.trim();
                    let (summary, body) = text.split_once('\n').unwrap_or((text, ""));
                    self.summary = summary.trim().to_owned();
                    self.description = body.trim().to_owned();
                }
            }
            Message::DiscardFile(path) => {
                let (Some(repo), Some(status)) = (self.repo.clone(), self.status.as_ref()) else {
                    return;
                };
                let untracked = status
                    .status
                    .entries
                    .iter()
                    .any(|e| e.path == path && e.kind == amonhen_model::StatusKind::Untracked);
                self.dialog = Some(Dialog::Discard {
                    repo,
                    path,
                    untracked,
                });
            }
            Message::WorkingFile(index) => {
                // 一覧は status が替わるたびに作り直すので、選び直しの知らせ（None）は無視して前の選択を保つ。
                if let Some(index) = index {
                    let path = self
                        .status
                        .as_ref()
                        .and_then(|s| s.status.entries.get(index))
                        .map(|e| e.path.clone());
                    self.working_file = path;
                    self.load_working_diff();
                }
            }
            Message::Committed(failed) => {
                self.committing = false;
                match failed {
                    None => {
                        self.summary.clear();
                        self.description.clear();
                        self.amend = false;
                    }
                    Some(message) => {
                        self.dialog = Some(Dialog::Failed {
                            title: tr!("Couldn’t Commit"),
                            message,
                        })
                    }
                }
            }
            Message::HistoryMenu(key) => {
                if let Some(text) = key.strip_prefix("copy:") {
                    copy_text(text);
                } else if let Some(name) = key.strip_prefix("checkout:") {
                    self.checkout(CheckoutTarget::Branch(name.to_owned()));
                } else if let Some(name) = key.strip_prefix("track:") {
                    self.checkout(CheckoutTarget::Track(name.to_owned()));
                } else if let Some(oid) = key.strip_prefix("detach:") {
                    self.checkout(CheckoutTarget::Detach(oid.to_owned()));
                } else if let Some(oid) = key.strip_prefix("branch:") {
                    self.new_branch(oid.to_owned());
                } else if let Some(oid) = key.strip_prefix("tag:") {
                    self.ask_name(oid.to_owned(), true);
                } else if let Some(oid) = key.strip_prefix("cherry-pick:") {
                    self.pick(oid.to_owned(), false);
                } else if let Some(oid) = key.strip_prefix("revert:") {
                    self.pick(oid.to_owned(), true);
                } else if let Some(oid) = key.strip_prefix("reset:") {
                    self.reset(oid.to_owned());
                } else if let Some(name) = key.strip_prefix("delete-branch:") {
                    self.check_delete(context, name.to_owned());
                } else if let Some(name) = key.strip_prefix("rename:") {
                    if let Some(repo) = self.repo.clone() {
                        self.dialog = Some(Dialog::Rename {
                            repo,
                            from: name.to_owned(),
                            name: name.to_owned(),
                        });
                        self.dialog_inputs += 1;
                    }
                } else if let Some(name) = key.strip_prefix("delete-tag:") {
                    if let Some(repo) = self.repo.clone() {
                        // GTK4 版の `refops::tag_items` の Delete。リモートのタグは残る。
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!("Delete tag {name}?", name = name),
                            message: format!(
                                "{} {}",
                                tr!("The remotes keep theirs."),
                                tr!("You can Undo this (Ctrl+Alt+Z).")
                            ),
                            button: tr!("Delete"),
                            command: json!({ "op": "deleteTag", "name": name }),
                        });
                    }
                } else if key == "remote-add" {
                    if let Some(repo) = self.repo.clone() {
                        // 1 つもなければ origin から（GTK4 版と同じ）。
                        let none = !self.refs.as_ref().is_some_and(|r| {
                            r.refs
                                .entries
                                .iter()
                                .any(|e| e.kind == amonhen_model::RefKind::Remote)
                        });
                        self.dialog = Some(Dialog::Remote {
                            repo,
                            original: None,
                            name: if none {
                                "origin".to_owned()
                            } else {
                                String::new()
                            },
                            fetch: String::new(),
                            push: String::new(),
                        });
                        self.dialog_inputs += 1;
                    }
                } else if let Some(remote) = key.strip_prefix("remote-edit:") {
                    if let Some(repo) = self.repo.clone() {
                        let remote = remote.to_owned();
                        context.spawn_background(move |_| {
                            let reply: Value =
                                serde_json::from_str(&repo.run_json(r#"{"op":"remotes"}"#))
                                    .unwrap_or_default();
                            let found = reply["info"]["remotes"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .find(|r| r["name"].as_str() == Some(remote.as_str()))
                                .map(|r| {
                                    let s = |k: &str| r[k].as_str().unwrap_or_default().to_owned();
                                    (s("name"), s("fetchUrl"), s("pushUrl"))
                                });
                            Message::RemoteLoaded(repo, found)
                        });
                    }
                } else if let Some(remote) = key.strip_prefix("remote-prune:") {
                    if let Some(repo) = self.repo.clone() {
                        // GTK4 版の `refops::remote_menu` の Prune（リモートと話すので先に聞く）。
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!("Prune {remote}?", remote = remote),
                            message: format!(
                                "{} {}",
                                tr!(
                                    "Removes the remote-tracking branches whose branches are gone from {remote} (git remote prune).",
                                    remote = remote
                                ),
                                tr!("This can’t be undone.")
                            ),
                            button: "Prune".to_owned(),
                            command: json!({ "op": "changeRemote", "change": { "kind": "prune", "name": remote } }),
                        });
                    }
                } else if let Some(remote) = key.strip_prefix("remote-remove:") {
                    if let Some(repo) = self.repo.clone() {
                        // GTK4 版の `remotesheet::ask_remove`。
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!("Remove {name}?", name = remote),
                            message: format!(
                                "{} {}",
                                tr!(
                                    "This removes the remote’s settings and its remote-tracking branches ({name}/…). The remote repository isn’t touched.",
                                    name = remote
                                ),
                                tr!("This can’t be undone.")
                            ),
                            button: tr!("Remove"),
                            command: json!({ "op": "changeRemote", "change": { "kind": "remove", "name": remote } }),
                        });
                    }
                } else if let Some((op, name)) = key.split_once(':')
                    && let Some(op) = op.strip_prefix("stash-")
                {
                    self.stash_action(op, name.to_owned());
                } else if let Some(rev) = key.strip_prefix("merge:") {
                    self.integrate(rev.to_owned(), false);
                } else if let Some(rev) = key.strip_prefix("rebase:") {
                    self.integrate(rev.to_owned(), true);
                }
            }
            Message::Performed(failed) => {
                self.busy = None;
                if let Some((title, message)) = failed {
                    self.dialog = Some(Dialog::Failed { title, message });
                }
            }
            Message::Invoked(key) if key == "page:working-copy" => {
                self.show_page(Page::WorkingCopy)
            }
            Message::Invoked(key) if key == "page:history" => self.show_page(Page::History),
            Message::Invoked(key) if key == "page:stashes" => self.show_page(Page::Stashes),
            Message::Sequence(sequence) => self.sequence = sequence,
            Message::ConflictCounted(path, count) => {
                if count == 0 {
                    self.update(Message::Stage(path, true), context);
                } else {
                    self.dialog = Some(Dialog::MarkResolved { path, count });
                }
            }
            Message::WorkingFileMenu(path, key) if key == "take-ours" || key == "take-theirs" => {
                // 片側のファイルそのものを取る（git checkout --ours・--theirs と同じ。GTK4 版の `take_side`）。
                if let Some(repo) = self.repo.clone() {
                    let ours = key == "take-ours";
                    perform(
                        context,
                        repo,
                        json!({ "op": "takeSide", "path": path, "ours": ours }),
                        if ours {
                            tr!("Take Current File")
                        } else {
                            tr!("Take Incoming File")
                        },
                    );
                }
            }
            Message::WorkingFileMenu(path, key) if key == "resolved" => {
                // マーカーが残っていれば、確かめてから（GTK4 版の `mark_resolved`）。
                if let Some(repo) = self.repo.clone() {
                    context.spawn_background(move |_| {
                        let command = json!({ "op": "conflictCount", "path": &path });
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(&command.to_string()))
                                .unwrap_or_default();
                        Message::ConflictCounted(path, reply["info"]["count"].as_u64().unwrap_or(0))
                    });
                }
            }
            Message::WorkingFileMenu(path, key) => match key.as_str() {
                "history" => self.update(Message::OpenFileHistory(path), context),
                "blame" => self.update(Message::OpenBlame(path, None), context),
                _ => self.update(Message::DiscardFile(path), context),
            },
            Message::OpenFileHistory(path) => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                self.file_history = Some(crate::filehistory::State {
                    path: path.clone(),
                    entries: None,
                    selected: None,
                });
                self.show_page(Page::FileHistory);
                context.spawn_background(move |_| {
                    let command = json!({ "op": "fileHistory", "path": &path, "merges": false });
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                    let entries = match reply["error"].as_str() {
                        Some(e) => Err(e.to_owned()),
                        None => Ok(crate::filehistory::entries_of(&reply["info"])),
                    };
                    Message::FileHistoryLoaded(path, entries)
                });
            }
            Message::FileHistoryLoaded(path, entries) => {
                if let Some(state) = self.file_history.as_mut()
                    && state.path == path
                {
                    // 読んだら、いちばん新しいコミットを選ぶ（GTK4 版と同じ）。
                    let first = entries.as_ref().ok().and_then(|e| e.first().cloned());
                    state.entries = Some(entries);
                    state.selected = first.is_some().then_some(0);
                    if let Some(first) = first {
                        self.show_file_change(&first);
                    }
                }
            }
            Message::FileHistorySelected(index) => {
                let entry = self.file_history.as_mut().and_then(|state| {
                    state.selected = index;
                    state.entries.as_ref()?.as_ref().ok()?.get(index?).cloned()
                });
                if let Some(entry) = entry {
                    self.show_file_change(&entry);
                }
            }
            Message::FileHistoryMenu(key) => {
                if let Some(oid) = key.strip_prefix("blame:") {
                    let path = self.file_history.as_ref().and_then(|state| {
                        let entries = state.entries.as_ref()?.as_ref().ok()?;
                        Some(entries.iter().find(|e| e.oid == oid)?.path.clone())
                    });
                    if let Some(path) = path {
                        self.update(Message::OpenBlame(path, Some(oid.to_owned())), context);
                    }
                }
            }
            Message::OpenBlame(path, rev) => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                if self.page != Page::Blame {
                    self.blame_from = self.page;
                }
                self.blame = Some(crate::blame::State {
                    path: path.clone(),
                    rev: rev.clone(),
                    data: None,
                });
                self.page = Page::Blame;
                context.spawn_background(move |_| {
                    let command = json!({ "op": "blame", "path": &path, "rev": &rev });
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                    let data = match reply["error"].as_str() {
                        Some(e) => Err(e.to_owned()),
                        None => Ok(crate::blame::data_of(&reply["info"])),
                    };
                    Message::BlameLoaded(path, rev, data)
                });
            }
            Message::BlameLoaded(path, rev, data) => {
                if let Some(state) = self.blame.as_mut()
                    && state.path == path
                    && state.rev == rev
                {
                    state.data = Some(data);
                }
            }
            Message::CloseBlame => {
                self.blame = None;
                self.page = self.blame_from;
            }
            Message::BlameMenu(key) => {
                if let Some(oid) = key.strip_prefix("copy:") {
                    copy_text(oid);
                } else if let Some(oid) = key.strip_prefix("show:")
                    && let Some(id) = amonhen_model::Oid::from_hex(oid)
                {
                    // History に戻ってそのコミットを選ぶ。
                    self.blame = None;
                    self.show_page(Page::History);
                    with_history(|h| h.select_commit(&id));
                }
            }
            Message::CloseFileHistory => {
                self.file_history = None;
                self.show_page(Page::History);
            }
            Message::StashSelected(index) => {
                let entry = index.and_then(|i| self.stash_entries().get(i).map(|e| (*e).clone()));
                self.stash = entry.as_ref().map(|e| e.name.clone());
                if let Some(target) = entry.and_then(|e| e.target) {
                    with_history(|h| h.session.data.want_stash(target));
                }
            }
            Message::Invoked(key) => {
                // Stashes・File History の画面からは History に戻ってそのコミットへ。
                if matches!(self.page, Page::Stashes | Page::FileHistory | Page::Blame) {
                    self.show_page(Page::History);
                }
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
        let root = self.root.clone();
        let refs = (
            self.panel.clone(),
            self.area.clone(),
            self.diff_panel.clone(),
            self.diff_area.clone(),
            self.sidebar_tree.clone(),
            self.sidebar_menu_host.clone(),
        );
        let sender = context.sender();
        context.use_effect("history", (), move || {
            let (panel, area, diff_panel, diff_area, sidebar, sidebar_menu_host) = refs;
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
                root,
                root_input: false,
                sidebar,
                sidebar_menu_host,
                sidebar_input: false,
                sidebar_host_input: false,
                sidebar_menu_ready: false,
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
                status_sent: None,
                selected: None,
                shown: None,
                context_at: None,
                menu_requested: false,
                menu_ready: false,
                working_page: false,
                last_query: None,
                multi_sent: Vec::new(),
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

        // 右クリック: メニューを選んだ行のものに作り直した後（この描き直しの後）に、押した位置で開く。
        if self.menu_request > 0 {
            // effect の時点では WinUI のメニューはまだ入れ替わっていないので、次のフレームで開く。
            context.use_effect("history-menu", self.menu_request, || {
                with_history(|h| h.menu_ready = true);
                None
            });
        }

        // サイドバーの右クリック: メニューを押した項目のものに作り直した後、次のフレームで開く。
        if self.sidebar_request > 0 {
            context.use_effect("sidebar-menu", self.sidebar_request, || {
                with_history(|h| h.sidebar_menu_ready = true);
                None
            });
        }
        // 木（TreeView）には参照を付けられないので、包む Grid に右クリックを付ける。メニューは押されない空の
        // Grid に付けておき、そこから取って開く（右クリックした所に付けると、WinUI が作り直す前のメニューを開く）。
        let sidebar = Grid::new()
            .element_ref(&self.sidebar_tree)
            .grid_column(0)
            .children((
                TreeView::new()
                    .on_item_invoked(context.callback(Message::Invoked))
                    .nodes(sidebar::nodes(
                        self.refs.as_ref().map(|r| &r.refs),
                        self.groups.as_ref(),
                    )),
                Grid::new()
                    .element_ref(&self.sidebar_menu_host)
                    .width(0.0)
                    .height(0.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .vertical_alignment(VerticalAlignment::Top)
                    .menu(Menu::new(
                        self.sidebar_menu(),
                        context.callback(Message::HistoryMenu),
                    )),
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
            .rows([
                GridLength::Pixel(if self.page == Page::WorkingCopy {
                    0.0
                } else {
                    240.0
                }),
                GridLength::Star(1.0),
            ])
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
        // 検索の欄を開いたら、フォーカスを入れて Enter・Esc を受ける（描き直した後に）。
        if self.search_opened > 0 {
            let search_box = self.search_box.clone();
            context.use_effect("search-box", self.search_opened, move || {
                let _ = search_box.request_focus();
                let _ = search_box.request_native_element(|result| {
                    if let Ok(element) = result
                        && let Err(e) = attach_search_keys(&element)
                    {
                        eprintln!("検索の欄にキーを付けられない: {e}");
                    }
                });
                None
            });
        }
        let search_bar: View = if self.search_open {
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .margin(Thickness::new(8.0, 4.0, 8.0, 4.0))
                .grid_row(0)
                .children((
                    TextBox::new()
                        .element_ref(&self.search_box)
                        .text(self.query.clone())
                        .placeholder_text(tr!("Search"))
                        .on_text_changed(context.callback(Message::Query))
                        .grid_column(0),
                    TextBlock::new()
                        .text(self.search_status.clone())
                        .margin(Thickness::new(8.0, 0.0, 4.0, 0.0))
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(1),
                ))
        } else {
            View::empty()
        };
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
                    .rows([GridLength::Auto, GridLength::Star(1.0)])
                    .grid_column(1)
                    .children((
                        search_bar,
                        Grid::new()
                            .background(Brush::Solid(Color::transparent()))
                            .element_ref(&self.area)
                            .grid_row(1)
                            .children((history,))
                            .menu(Menu::new(
                                self.history_menu(),
                                context.callback(Message::HistoryMenu),
                            )),
                    )),
                right,
                // Working Copy は History の上に重ねる（History の描画は SwapChainPanel の毎フレームの知らせで回って
                // いて、外すと止まる）。
                self.working_copy_view(context),
                self.stashes_view(context),
                self.file_history_view(context),
                self.blame_view(context),
            ));
        // ダイアログの入力の欄: フォーカスを入れ、Enter で既定のボタン（ContentDialog は Enter を既定のボタンに
        // 回さない）。
        if self.dialog_inputs > 0 {
            let input = self.dialog_input.clone();
            context.use_effect("dialog-input", self.dialog_inputs, move || {
                let _ = input.request_focus();
                let _ = input.request_native_element(|result| {
                    if let Ok(element) = result
                        && let Err(e) = attach_enter(&element)
                    {
                        eprintln!("入力の欄にキーを付けられない: {e}");
                    }
                });
                None
            });
        }
        let dialog = self.dialog_view(context);
        Grid::new()
            .element_ref(&self.root)
            .rows([
                GridLength::Auto,
                GridLength::Auto,
                GridLength::Star(1.0),
                GridLength::Auto,
            ])
            .children((menu_bar, toolbar, body, self.status_bar(), dialog))
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
            Some(Dialog::Confirm {
                title,
                message,
                button,
                ..
            }) => dialog
                .is_open(true)
                .title(title.clone())
                .primary_button_text(button.clone())
                .close_button_text(tr!("Cancel"))
                .content(
                    TextBlock::new()
                        .text(message.clone())
                        .text_wrapping(TextWrapping::Wrap),
                ),
            Some(Dialog::NewBranch {
                start, name, tag, ..
            }) => dialog
                .is_open(true)
                .title(if *tag {
                    tr!("New Tag at {name}", name = shown_start(start))
                } else {
                    tr!("New Branch from {name}", name = shown_start(start))
                })
                .primary_button_text(tr!("Create"))
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!name.trim().is_empty())
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(if *tag {
                                tr!("Name of the tag.")
                            } else {
                                tr!("Name of the branch. It is checked out.")
                            })
                            .text_wrapping(TextWrapping::Wrap),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(name.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::Remote {
                original,
                name,
                fetch,
                push,
                ..
            }) => {
                let field = |label: String, text: &str, index: usize, placeholder: String| {
                    StackPanel::new().spacing(2.0).children((
                        TextBlock::new().text(label),
                        TextBox::new()
                            .text(text)
                            .placeholder_text(placeholder)
                            .min_width(360.0)
                            .on_text_changed(
                                context.callback(move |t| Message::RemoteField(index, t)),
                            ),
                    ))
                };
                dialog
                    .is_open(true)
                    .title(if original.is_some() {
                        tr!("Edit Remote")
                    } else {
                        tr!("Add Remote")
                    })
                    .primary_button_text(tr!("Save"))
                    .close_button_text(tr!("Cancel"))
                    .is_primary_button_enabled(!name.trim().is_empty() && !fetch.trim().is_empty())
                    .content(
                        StackPanel::new().spacing(8.0).children((
                            StackPanel::new().spacing(2.0).children((
                                TextBlock::new().text(tr!("Name")),
                                TextBox::new()
                                    .element_ref(&self.dialog_input)
                                    .text(name.clone())
                                    .min_width(360.0)
                                    .on_text_changed(
                                        context.callback(|t| Message::RemoteField(0, t)),
                                    ),
                            )),
                            field(tr!("Fetch URL"), fetch, 1, String::new()),
                            field(tr!("Push URL"), push, 2, tr!("Same as the fetch URL")),
                        )),
                    )
            }
            Some(Dialog::StashBranch { stash, name, .. }) => dialog
                .is_open(true)
                .title(tr!("New Branch from {name}", name = stash))
                .primary_button_text(tr!("Create"))
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!name.trim().is_empty())
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(tr!(
                                "Created where the stash was made, checked out, and the stash applied there (then dropped)."
                            ))
                            .text_wrapping(TextWrapping::Wrap),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(name.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::MarkResolved { path, count }) => dialog
                .is_open(true)
                .title(tr!(
                    "Mark {path} as resolved with {count} conflicts left?",
                    path = path,
                    count = count
                ))
                .primary_button_text(tr!("Mark Resolved"))
                .close_button_text(tr!("Cancel"))
                .content(
                    TextBlock::new()
                        .text(tr!("The conflict markers stay in the file."))
                        .text_wrapping(TextWrapping::Wrap),
                ),
            Some(Dialog::Rename { from, name, .. }) => dialog
                .is_open(true)
                .title(tr!("Rename {name}", name = from))
                .primary_button_text(tr!("Rename"))
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!name.trim().is_empty() && name.trim() != from)
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(tr!("New name of the branch."))
                            .text_wrapping(TextWrapping::Wrap),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(name.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::Checkout { title, message, .. }) => dialog
                .is_open(true)
                .title(title.clone())
                .primary_button_text("Checkout")
                .close_button_text(tr!("Cancel"))
                .content(
                    TextBlock::new()
                        .text(message.clone())
                        .text_wrapping(TextWrapping::Wrap),
                ),
            Some(Dialog::Pull {
                title,
                mode,
                sources,
                source,
                ..
            }) => {
                let modes: Vec<String> = PULL_MODES.iter().map(|m| pull_mode_title(m)).collect();
                // 上流がなければ、取り込み元を選ぶ欄を先に。
                let from: View = if sources.is_empty() {
                    View::empty()
                } else {
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new().text(tr!("From:")),
                        ComboBox::new()
                            .items_source(sources.clone())
                            .selected_index(*source)
                            .on_selection_changed(context.callback(Message::PullSource)),
                    ))
                };
                dialog
                    .is_open(true)
                    .title(title.clone())
                    .primary_button_text("Pull")
                    .close_button_text(tr!("Cancel"))
                    .content(
                        StackPanel::new().spacing(6.0).children((
                            from,
                            TextBlock::new().text(tr!("Integrate with:")),
                            ComboBox::new()
                                .items_source(modes)
                                .selected_index(*mode)
                                .on_selection_changed(context.callback(Message::PullMode)),
                        )),
                    )
            }
            Some(Dialog::Push {
                title,
                message,
                replace,
                diverged,
                ..
            }) => {
                let dialog = dialog
                    .is_open(true)
                    .title(title.clone())
                    .close_button_text(tr!("Cancel"));
                // 分かれていれば Pull…（Enter）と Force Push、でなければ Push か Force Push（GTK4 版と同じ並び）。
                let dialog = if *diverged {
                    dialog
                        .primary_button_text("Pull…")
                        .secondary_button_text("Force Push")
                } else {
                    dialog.primary_button_text(if *replace { "Force Push" } else { "Push" })
                };
                dialog.content(
                    TextBlock::new()
                        .text(message.clone())
                        .text_wrapping(TextWrapping::Wrap),
                )
            }
            Some(Dialog::Publish {
                branch,
                remotes,
                remote,
                name,
                ..
            }) => dialog
                .is_open(true)
                .title(tr!("Publish {branch}?", branch = branch))
                .primary_button_text("Publish")
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!name.trim().is_empty())
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(tr!(
                                "This creates the branch on the remote and makes it the upstream (where Pull and Push go from now on)."
                            ))
                            .text_wrapping(TextWrapping::Wrap),
                        TextBlock::new().text(tr!("Remote")),
                        ComboBox::new()
                            .items_source(remotes.clone())
                            .selected_index(*remote)
                            .on_selection_changed(context.callback(Message::PublishRemote)),
                        TextBlock::new().text(tr!("Name there")),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(name.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::Fetch { remotes, from, .. }) => {
                // GTK4 版の `sheets::fetch` と同じ: リモートが 1 つならその名前を見出しに、2 つ以上なら選ぶ。
                let title = match remotes.as_slice() {
                    [only] => tr!("Fetch from {remote}?", remote = only),
                    _ => tr!("Fetch?"),
                };
                let content: View = if remotes.len() > 1 {
                    let mut choices = vec![tr!(
                        "All Remotes ({names})",
                        names = amonhen_model::i18n::list(remotes)
                    )];
                    choices.extend(remotes.iter().cloned());
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new().text(tr!("From:")),
                        ComboBox::new()
                            .items_source(choices)
                            .selected_index(*from)
                            .on_selection_changed(context.callback(Message::FetchFrom)),
                    ))
                } else {
                    TextBlock::new().into()
                };
                dialog
                    .is_open(true)
                    .title(title)
                    .primary_button_text("Fetch")
                    .close_button_text(tr!("Cancel"))
                    .content(content)
            }
            Some(Dialog::Discard {
                path, untracked, ..
            }) => {
                // GTK4 版の `discard_file` と同じ文: untracked は消す（戻せない）、それ以外は Undo で戻せる。
                let (title, text, button) = if *untracked {
                    (
                        tr!("Delete {path}?", path = path),
                        format!(
                            "{} {}",
                            tr!("It’s untracked."),
                            tr!("This can’t be undone.")
                        ),
                        tr!("Delete"),
                    )
                } else {
                    (
                        tr!("Discard changes to {path}?", path = path),
                        tr!("You can Undo this (Ctrl+Alt+Z)."),
                        "Discard".to_owned(),
                    )
                };
                dialog
                    .is_open(true)
                    .title(title)
                    .primary_button_text(button)
                    .close_button_text(tr!("Cancel"))
                    .content(
                        TextBlock::new()
                            .text(text)
                            .text_wrapping(TextWrapping::Wrap),
                    )
            }
            Some(Dialog::Reset { title, mode, .. }) => {
                let modes = vec![
                    tr!("Soft: keep the changes staged"),
                    tr!("Mixed: keep the changes in the working tree"),
                    tr!("Hard: match the commit (uncommitted changes are stashed first)"),
                ];
                dialog
                    .is_open(true)
                    .title(title.clone())
                    .primary_button_text("Reset")
                    .close_button_text(tr!("Cancel"))
                    .content(
                        StackPanel::new().spacing(6.0).children((
                            TextBlock::new()
                                .text(format!(
                                    "{} {}",
                                    tr!("Commits taken off stay in Reflog."),
                                    tr!("You can Undo this (Ctrl+Alt+Z).")
                                ))
                                .text_wrapping(TextWrapping::Wrap),
                            RadioButtons::new()
                                .items_source(modes)
                                .selected_index(*mode)
                                .on_selection_changed(context.callback(Message::ResetMode)),
                        )),
                    )
            }
            Some(Dialog::Integrate {
                rebase,
                current,
                choices,
                chooser,
                chosen,
                mode,
                ..
            }) => {
                // 相手が決まっていれば問いの見出し、選ぶなら「X に merge する」（GTK4 版と同じ）。
                let chooser = *chooser;
                let title = match (chooser, *rebase) {
                    (true, true) => tr!("Rebase {name}", name = current),
                    (true, false) => tr!("Merge into {name}", name = current),
                    (false, true) => tr!(
                        "Rebase {target} onto {rev}?",
                        target = current,
                        rev = choices.first().map_or("", |c| c.0.as_str())
                    ),
                    (false, false) => tr!(
                        "Merge {rev} into {target}?",
                        rev = choices.first().map_or("", |c| c.0.as_str()),
                        target = current
                    ),
                };
                // 選ぶ欄（なければ Undo の文）と、merge なら取り込み方。
                let (head, picker): (View, View) = if chooser {
                    (
                        TextBlock::new()
                            .text(if *rebase {
                                tr!("The branch, tag or commit to rebase onto")
                            } else {
                                tr!("The branch, tag or commit to merge in")
                            })
                            .into(),
                        ComboBox::new()
                            .items_source(choices.iter().map(|c| c.0.clone()).collect::<Vec<_>>())
                            .selected_index(*chosen)
                            .min_width(320.0)
                            .on_selection_changed(context.callback(Message::IntegrateRev))
                            .into(),
                    )
                } else {
                    (
                        TextBlock::new()
                            .text(tr!("You can Undo this (Ctrl+Alt+Z)."))
                            .text_wrapping(TextWrapping::Wrap)
                            .into(),
                        View::empty(),
                    )
                };
                let modes: View = if *rebase {
                    View::empty()
                } else {
                    ComboBox::new()
                        .items_source(
                            MERGE_MODES
                                .iter()
                                .map(|m| merge_mode_title(m))
                                .collect::<Vec<_>>(),
                        )
                        .selected_index(*mode)
                        .on_selection_changed(context.callback(Message::MergeMode))
                        .into()
                };
                dialog
                    .is_open(true)
                    .title(title)
                    .primary_button_text(if *rebase { "Rebase" } else { "Merge" })
                    .close_button_text(tr!("Cancel"))
                    .content(
                        StackPanel::new()
                            .spacing(6.0)
                            .children((head, picker, modes)),
                    )
            }
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

    fn new_branch(&mut self, start: String) {
        self.ask_name(start, false);
    }

    /// 新しいブランチ（`tag` ならタグ）の名前を聞く。
    fn ask_name(&mut self, start: String, tag: bool) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        self.dialog = Some(Dialog::NewBranch {
            repo,
            start,
            name: String::new(),
            tag,
        });
        self.dialog_inputs += 1;
    }

    /// Cherry-pick・Revert の確認（GTK4 版の `sheets::pick`: 今のブランチに足す。前後の絵はまだない）。
    fn pick(&mut self, oid: String, revert: bool) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let onto = self
            .refs
            .as_ref()
            .and_then(|r| r.refs.head_branch.clone())
            .unwrap_or_else(|| "HEAD".to_owned());
        let what = oid[..8].to_owned();
        let title = if revert {
            tr!("Revert {what} on {onto}?", what = what, onto = onto)
        } else {
            tr!("Cherry-pick {what} onto {onto}?", what = what, onto = onto)
        };
        let message = format!(
            "{} {}",
            if revert {
                tr!("Adds commits that undo them.")
            } else {
                tr!("Adds copies of them.")
            },
            tr!("You can Undo this (Ctrl+Alt+Z).")
        );
        // マージかは、出している詳細の親の数で（同じコミットのとき）。
        let merge = self.info.as_ref().is_some_and(|(info, _)| {
            info["oid"].as_str() == Some(oid.as_str())
                && info["parents"].as_array().is_some_and(|p| p.len() > 1)
        });
        let op = if revert { "revert" } else { "cherryPick" };
        self.dialog = Some(Dialog::Confirm {
            repo,
            title,
            message,
            button: if revert { "Revert" } else { "Cherry-pick" }.to_owned(),
            command: json!({ "op": op, "oids": [oid], "isMerge": merge }),
        });
    }

    /// ブランチを消す前に、今のブランチ（上流があり、そちらの方が少なければ上流）にないコミットを調べる（GTK4 版の
    /// `sheets::unique_commits`）。プルリクエストでマージ済みかはまだ見ない。
    fn check_delete(&mut self, context: &ComponentContext<Self>, name: String) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let upstream = self
            .refs
            .as_ref()
            .and_then(|r| {
                r.refs
                    .entries
                    .iter()
                    .find(|e| e.kind == amonhen_model::RefKind::Local && e.name == name)
            })
            .and_then(|e| if e.gone { None } else { e.detail.clone() });
        context.spawn_background(move |_| {
            let between = |base: &str| -> Result<(u64, Vec<String>), String> {
                let command = json!({ "op": "between", "base": base, "tip": &name, "limit": 30 });
                let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                    .map_err(|e| e.to_string())?;
                if reply["ok"].as_bool() != Some(true) {
                    return Err(reply["error"]
                        .as_str()
                        .map_or_else(|| tr!("Couldn’t check the branches"), str::to_owned));
                }
                let info = &reply["info"];
                let commits = info["commits"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        let oid = c["oid"].as_str().unwrap_or_default();
                        format!(
                            "{}  {}",
                            &oid[..oid.len().min(8)],
                            c["subject"].as_str().unwrap_or_default()
                        )
                    })
                    .collect();
                Ok((info["count"].as_u64().unwrap_or(0), commits))
            };
            let checked = between("HEAD").and_then(|head| match upstream {
                Some(u) if head.0 > 0 => {
                    let up = between(&u)?;
                    Ok(if up.0 < head.0 { up } else { head })
                }
                _ => Ok(head),
            });
            Message::DeleteChecked(repo, name, checked)
        });
    }

    /// 決まった相手との Merge・Rebase の確認（サイドバーの右クリックから。GTK4 版の `sheets::integrate`）。
    fn integrate(&mut self, rev: String, rebase: bool) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        self.dialog = Some(Dialog::Integrate {
            repo,
            rebase,
            current: self.head_name(),
            choices: vec![(rev.clone(), rev)],
            chooser: false,
            chosen: 0,
            mode: 0,
        });
    }

    /// サイドバーの右クリックのメニュー（GTK4 版の `refops::menu` のうち、動く操作だけ）。項目のキーは History の
    /// 右クリックと同じ形（`Message::HistoryMenu` で受ける）。
    fn sidebar_menu(&self) -> Vec<MenuItem> {
        use amonhen_model::RefKind;
        // リモートと Remotes の見出し（GTK4 版の `refops::remote_menu`）。
        match self.sidebar_key.as_deref() {
            Some("group:remotes") => {
                return vec![MenuItem::item("remote-add", tr!("Add Remote…"))];
            }
            Some(key) if key.starts_with("folder:remotes/") => {
                let remote = &key["folder:remotes/".len()..];
                return vec![
                    MenuItem::item(format!("remote-edit:{remote}"), tr!("Edit…")),
                    MenuItem::item(format!("remote-prune:{remote}"), "Prune…"),
                    MenuItem::separator("-remove"),
                    MenuItem::item(format!("remote-remove:{remote}"), tr!("Remove…")),
                ];
            }
            _ => {}
        }
        let Some(entry) = self.sidebar_key.as_deref().and_then(|k| self.entry(k)) else {
            return Vec::new();
        };
        let name = entry.name.as_str();
        let current = self.head_name();
        let is_head = entry.kind == RefKind::Local
            && self
                .refs
                .as_ref()
                .and_then(|r| r.refs.head_branch.as_deref())
                == Some(name);
        let mut items = Vec::new();
        let copy = match entry.kind {
            RefKind::Local | RefKind::Remote => tr!("Copy Branch Name"),
            RefKind::Tag => tr!("Copy Tag Name"),
            RefKind::Stash => return items,
            _ => tr!("Copy Name"),
        };
        items.push(MenuItem::item(format!("copy:{name}"), copy));
        if !matches!(entry.kind, RefKind::Local | RefKind::Remote | RefKind::Tag) {
            return items;
        }
        items.push(MenuItem::separator("-copy"));
        let merge = MenuItem::item(
            format!("merge:{name}"),
            tr!("Merge {rev} into {target}…", rev = name, target = &current),
        );
        let rebase = MenuItem::item(
            format!("rebase:{name}"),
            tr!("Rebase {target} onto {rev}…", target = &current, rev = name),
        );
        match entry.kind {
            RefKind::Local if !is_head => {
                items.push(MenuItem::item(
                    format!("checkout:{name}"),
                    tr!("Checkout {name}…", name = name),
                ));
                items.push(merge);
                items.push(rebase);
                items.push(MenuItem::separator("-integrate"));
            }
            RefKind::Remote => {
                items.push(MenuItem::item(
                    format!("track:{name}"),
                    tr!("Checkout {name} as Local Branch…", name = name),
                ));
                items.push(merge);
                items.push(rebase);
                items.push(MenuItem::separator("-integrate"));
            }
            RefKind::Tag => items.push(merge),
            _ => {}
        }
        items.push(MenuItem::item(
            format!("branch:{name}"),
            tr!("New Branch from {name}…", name = name),
        ));
        if entry.kind == RefKind::Local {
            items.push(MenuItem::item(
                format!("tag:{name}"),
                tr!("New Tag at {name}…", name = name),
            ));
            items.push(MenuItem::item(format!("rename:{name}"), tr!("Rename…")));
            if !is_head {
                items.push(MenuItem::item(
                    format!("delete-branch:{name}"),
                    tr!("Delete {name}…", name = name),
                ));
            }
        }
        if entry.kind == RefKind::Tag {
            items.push(MenuItem::separator("-tag"));
            items.push(MenuItem::item(
                format!("delete-tag:{name}"),
                tr!("Delete {name}…", name = name),
            ));
        }
        items
    }

    /// 「Reset <今のブランチ> to <コミット>…」の確認（GTK4 版の `commitops::ask_reset`）。
    fn reset(&mut self, oid: String) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let branch = self.head_name();
        self.dialog = Some(Dialog::Reset {
            repo,
            title: tr!("Reset {branch} to {id}?", branch = branch, id = &oid[..8]),
            oid,
            mode: 1,
        });
    }

    /// 今のブランチの名前（detached なら HEAD）。
    fn head_name(&self) -> String {
        self.refs
            .as_ref()
            .and_then(|r| r.refs.head_branch.clone())
            .unwrap_or_else(|| "HEAD".to_owned())
    }

    /// Repository > Merge…・Rebase…: 相手を選んでから（GTK4 版の `appmenu::merge_or_rebase`）。選べるのは History
    /// で選んでいるコミット（HEAD でなければ）、ローカル・リモートのブランチ、タグ（今のブランチは除く）。
    fn integrate_choosing(&mut self, rebase: bool) {
        let (Some(repo), Some(refs)) = (self.repo.clone(), self.refs.clone()) else {
            return beep();
        };
        let mut choices = Vec::new();
        let head = refs.refs.head.map(|h| h.to_string());
        if let Some((id, _)) = &self.selected
            && Some(id) != head.as_ref()
        {
            choices.push((tr!("Selected Commit {id}", id = &id[..8]), id.clone()));
        }
        for kind in [
            amonhen_model::RefKind::Local,
            amonhen_model::RefKind::Remote,
            amonhen_model::RefKind::Tag,
        ] {
            for e in refs.refs.entries.iter().filter(|e| e.kind == kind) {
                if Some(&e.name) != refs.refs.head_branch.as_ref() {
                    choices.push((e.name.clone(), e.name.clone()));
                }
            }
        }
        if choices.is_empty() {
            return beep();
        }
        // メニューからは、相手が 1 つでも選ぶ欄を出す（GTK4 版と同じ）。
        self.dialog = Some(Dialog::Integrate {
            repo,
            rebase,
            current: self.head_name(),
            choices,
            chooser: true,
            chosen: 0,
            mode: 0,
        });
    }

    /// Checkout の確認を出す（GTK4 版の `sheets::checkout` の見出しと説明。作業中の変更も持って行くと添える）。
    fn checkout(&mut self, target: CheckoutTarget) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let (title, command, line) = match &target {
            CheckoutTarget::Branch(name) => (
                tr!("Checkout {name}?", name = name),
                json!({ "op": "checkout", "branch": name }),
                String::new(),
            ),
            CheckoutTarget::Track(remote) => (
                tr!("Checkout {name}?", name = remote),
                json!({ "op": "track", "remoteBranch": remote }),
                tr!(
                    "Creates {local} tracking {remote}.",
                    local = remote.split_once('/').map_or(remote.as_str(), |(_, b)| b),
                    remote = remote
                ),
            ),
            CheckoutTarget::Detach(oid) => (
                tr!("Checkout {name}?", name = &oid[..oid.len().min(8)]),
                json!({ "op": "detach", "oid": oid }),
                tr!("HEAD is detached (on no branch)."),
            ),
        };
        let changed = self.status.as_ref().map_or(0, |s| {
            s.status
                .entries
                .iter()
                .filter(|e| e.kind != amonhen_model::StatusKind::Untracked)
                .count()
        });
        let mut lines = vec![line];
        if changed > 0 {
            lines.push(tr!(
                "{count} files with uncommitted changes come along.",
                count = changed
            ));
        }
        let message = lines
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        self.dialog = Some(Dialog::Checkout {
            repo,
            title,
            message,
            command,
        });
    }

    /// 今のブランチの、ref の一覧の行（上流は `detail`、上流との差は `ahead`・`behind`）。
    fn head_entry(&self) -> Option<&RefEntry> {
        let refs = &self.refs.as_ref()?.refs;
        let head = refs.head_branch.as_deref()?;
        refs.entries
            .iter()
            .find(|e| e.kind == amonhen_model::RefKind::Local && e.name == head)
    }

    /// Repository > Pull…: 上流から取り込む（GTK4 版の `sheets::pull_in` の見出しと取り込み方）。上流がなければ、
    /// 取り込み元のリモートのブランチ（同じ名前のものから）を選び、それを上流にして取り込む。前後の絵はまだない。
    fn pull(&mut self) {
        let (Some(repo), Some(entry)) = (self.repo.clone(), self.head_entry().cloned()) else {
            return beep();
        };
        let branch = entry.name.clone();
        let (title, sources) = match entry.detail.clone().filter(|_| !entry.gone) {
            Some(upstream) if entry.behind > 0 => (
                tr!(
                    "Pull {count} commits from {upstream}?",
                    count = entry.behind,
                    upstream = upstream
                ),
                Vec::new(),
            ),
            Some(upstream) => (
                tr!("Pull from {upstream}?", upstream = upstream),
                Vec::new(),
            ),
            None => {
                let remotes: Vec<String> = self
                    .refs
                    .as_ref()
                    .map(|r| {
                        r.refs
                            .entries
                            .iter()
                            .filter(|e| {
                                e.kind == amonhen_model::RefKind::Remote
                                    && !e.name.ends_with("/HEAD")
                            })
                            .map(|e| e.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let same = |n: &String| n.split_once('/').is_some_and(|(_, b)| b == branch);
                let mut sources: Vec<String> =
                    remotes.iter().filter(|n| same(n)).cloned().collect();
                sources.extend(remotes.iter().filter(|n| !same(n)).cloned());
                if sources.is_empty() {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("No remote branch to pull from"),
                        message: tr!("Fetch, or add a remote in Repository Settings."),
                    });
                    return;
                }
                (tr!("Pull into {branch}?", branch = &branch), sources)
            }
        };
        self.dialog = Some(Dialog::Pull {
            repo,
            title,
            mode: 0,
            branch,
            sources,
            source: 0,
        });
    }

    /// Repository > Push…・Force Push…（GTK4 版の `sheets::push_in`）。上流がなければ Publish。上流が先に進んで
    /// いれば、Pull… か Force Push を選んでもらう。Force Push は、最後の Fetch の時点の上流のときだけ置き換える。
    /// 送るコミットの一覧・前後の絵・注意はまだない。
    fn push(&mut self, force: bool) {
        let (Some(repo), Some(entry)) = (self.repo.clone(), self.head_entry().cloned()) else {
            return beep();
        };
        let branch = entry.name.clone();
        let Some(upstream) = entry.detail.clone().filter(|_| !entry.gone) else {
            return self.publish(repo, branch);
        };
        let (outgoing, incoming) = (u64::from(entry.ahead), u64::from(entry.behind));
        let diverged = !force && incoming > 0;
        if !(force || diverged || outgoing > 0) || (diverged && outgoing == 0) {
            return beep();
        }
        let (title, message) = if diverged {
            (
                tr!(
                    "{upstream} has {count} commits you don’t have",
                    upstream = &upstream,
                    count = incoming
                ),
                tr!("Pull them first, or force push to replace them. Force Push can’t be undone."),
            )
        } else if force {
            (
                tr!("Force push {branch}?", branch = &branch),
                if incoming > 0 {
                    tr!(
                        "{upstream} loses {count} commits. This can’t be undone.",
                        upstream = &upstream,
                        count = incoming
                    )
                } else {
                    tr!(
                        "It fails if {upstream} has moved since you last fetched.",
                        upstream = &upstream
                    )
                },
            )
        } else {
            (
                tr!(
                    "Push {count} commits to {upstream}?",
                    count = outgoing,
                    upstream = &upstream
                ),
                String::new(),
            )
        };
        // 置き換えてよいのは、最後の Fetch の時点の上流（そのあと誰かが送っていれば断られる）。
        let expect = self.refs.as_ref().and_then(|r| {
            r.refs
                .entries
                .iter()
                .find(|e| e.kind == amonhen_model::RefKind::Remote && e.name == upstream)
                .and_then(|e| e.target)
                .map(|t| t.to_string())
        });
        let (remote, remote_branch) = upstream
            .split_once('/')
            .map(|(r, b)| (r.to_owned(), b.to_owned()))
            .unwrap_or_default();
        self.dialog = Some(Dialog::Push {
            repo,
            title,
            message,
            remote,
            branch,
            remote_branch,
            replace: force,
            diverged,
            expect,
        });
    }

    /// 上流のないブランチを初めて送る前に、送り先のリモートとそこでの名前を聞く（GTK4 版の `publish_in`。
    /// 既定のリモートは、1 つならそれ、なければ origin）。
    fn publish(&mut self, repo: Arc<Repository>, branch: String) {
        let mut remotes: Vec<String> = self
            .refs
            .as_ref()
            .map(|r| {
                r.refs
                    .entries
                    .iter()
                    .filter(|e| e.kind == amonhen_model::RefKind::Remote)
                    .filter_map(|e| e.name.split_once('/').map(|(r, _)| r.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        remotes.sort();
        remotes.dedup();
        if remotes.is_empty() {
            self.dialog = Some(Dialog::Failed {
                title: tr!("Can’t Publish"),
                message: tr!(
                    "This repository has no remotes. Add one in Repository > Add Remote…."
                ),
            });
            return;
        }
        let remote = remotes.iter().position(|r| r == "origin").unwrap_or(0);
        self.dialog = Some(Dialog::Publish {
            repo,
            name: branch.clone(),
            branch,
            remotes,
            remote,
        });
        self.dialog_inputs += 1;
    }

    /// 窓の下のステータスバー: 走っている操作（なければ今のブランチ）。
    fn status_bar(&self) -> View {
        let text = match &self.busy {
            Some(busy) => busy.clone(),
            None => self
                .refs
                .as_ref()
                .and_then(|r| r.refs.head_branch.clone())
                .unwrap_or_default(),
        };
        TextBlock::new()
            .text(text)
            .font_size(12.0)
            .margin(Thickness::new(12.0, 3.0, 12.0, 4.0))
            .grid_row(3)
            .into()
    }

    fn show_page(&mut self, page: Page) {
        let from = self.page;
        self.page = page;
        let stash = self
            .stash
            .as_deref()
            .and_then(|name| self.stash_entries().iter().find(|e| e.name == name)?.target);
        with_history(|h| {
            h.set_working_page(page == Page::WorkingCopy);
            if page == Page::Stashes {
                // 選んでいた stash を出し直す。
                if let Some(target) = stash {
                    h.session.data.want_stash(target);
                }
            } else if matches!(from, Page::Stashes | Page::FileHistory | Page::Blame) {
                // History で選んでいるコミットを読み直す。
                h.reload_selected();
            }
        });
    }

    /// File History で選んだコミットでの、そのファイルの差分を右の列に出す。
    fn show_file_change(&self, entry: &crate::filehistory::Entry) {
        if let Some(id) = amonhen_model::Oid::from_hex(&entry.oid) {
            let path = entry.path.clone();
            with_history(|h| h.session.data.want_file_change(id, path));
        }
    }

    fn file_history_view(&self, context: &ViewContext<Self>) -> View {
        let (Page::FileHistory, Some(state)) = (self.page, &self.file_history) else {
            return View::empty();
        };
        let view = crate::filehistory::view(
            state,
            crate::filehistory::Callbacks {
                selected: context.callback(Message::FileHistorySelected),
                close: context.callback(|()| Message::CloseFileHistory),
                menu: Box::new(|| context.callback(Message::FileHistoryMenu)),
            },
        );
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .content(view)
    }

    fn blame_view(&self, context: &ViewContext<Self>) -> View {
        let (Page::Blame, Some(state)) = (self.page, &self.blame) else {
            return View::empty();
        };
        let view = crate::blame::view(
            state,
            crate::blame::Callbacks {
                close: context.callback(|()| Message::CloseBlame),
                menu: Box::new(|| context.callback(Message::BlameMenu)),
            },
        );
        // コードの幅が要るので、History と右の列の両方を覆う。
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .grid_column_span(2)
            .content(view)
    }

    /// stash の一覧（新しい順）。
    fn stash_entries(&self) -> Vec<&RefEntry> {
        self.refs.as_ref().map_or_else(Vec::new, |r| {
            r.refs
                .entries
                .iter()
                .filter(|e| e.kind == amonhen_model::RefKind::Stash)
                .collect()
        })
    }

    /// stash の操作（GTK4 版の `sheets::pop`・`stashes::confirm_drop`・`stashes::branch`）。`op` は apply・pop・
    /// drop・branch。
    fn stash_action(&mut self, op: &str, name: String) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let message = self
            .stash_entries()
            .iter()
            .find(|e| e.name == name)
            .and_then(|e| e.detail.clone());
        let mut text = message.map(|m| format!("“{m}”\n")).unwrap_or_default();
        let undo = tr!("You can Undo this (Ctrl+Alt+Z).");
        self.dialog = Some(match op {
            "apply" | "pop" => {
                let pop = op == "pop";
                let what = if pop {
                    tr!("Applies it to the working tree and removes it from the list.")
                } else {
                    tr!("Applies it to the working tree. It stays in the list.")
                };
                text.push_str(&format!("{what} {undo}"));
                Dialog::Confirm {
                    repo,
                    title: if pop {
                        tr!("Pop {name}?", name = &name)
                    } else {
                        tr!("Apply the stash {name}?", name = &name)
                    },
                    message: text,
                    button: if pop { "Pop" } else { "Apply" }.to_owned(),
                    command: json!({ "op": "stashApply", "name": name, "pop": pop }),
                }
            }
            "drop" => {
                text.push_str(&undo);
                Dialog::Confirm {
                    repo,
                    title: tr!("Drop {name}?", name = &name),
                    message: text,
                    button: "Drop".to_owned(),
                    command: json!({ "op": "stashDrop", "name": name }),
                }
            }
            "branch" => {
                self.dialog_inputs += 1;
                Dialog::StashBranch {
                    repo,
                    stash: name,
                    name: String::new(),
                }
            }
            _ => return,
        });
    }

    fn stashes_view(&self, context: &ViewContext<Self>) -> View {
        if self.page != Page::Stashes {
            return View::empty();
        }
        let stashes = self.stash_entries();
        let selected = self.stash.as_deref();
        let callbacks = crate::stashes::Callbacks {
            selected: context.callback(Message::StashSelected),
            button: Box::new(move |op: &'static str| {
                let name = selected.unwrap_or_default().to_owned();
                context.callback(move |()| Message::HistoryMenu(format!("{op}:{name}")))
            }),
            menu: Box::new(|| context.callback(Message::HistoryMenu)),
        };
        let view = crate::stashes::view(
            &stashes,
            selected,
            self.refs
                .as_ref()
                .and_then(|r| r.refs.head_branch.as_deref()),
            callbacks,
        );
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .content(view)
    }

    /// Working Copy で選んでいるファイルの差分を読む（なくなっていれば消す）。
    fn load_working_diff(&self) {
        let entry = self.working_file.as_ref().and_then(|path| {
            self.status
                .as_ref()?
                .status
                .entries
                .iter()
                .find(|e| &e.path == path)
                .cloned()
        });
        with_history(|h| h.session.data.want_working(entry));
    }

    fn working_copy_view(&self, context: &ViewContext<Self>) -> View {
        if self.page != Page::WorkingCopy {
            return View::empty();
        }
        let callbacks = crate::workcopy::Callbacks {
            summary: context.callback(Message::Summary),
            description: context.callback(Message::Description),
            commit: context.callback(|()| Message::CommitChanges),
            stage: Box::new(|path: String| {
                context.callback(move |include| Message::Stage(path.clone(), include))
            }),
            selected: context.callback(Message::WorkingFile),
            amend: context.callback(Message::Amend),
            file_menu: Box::new(|path: String| {
                context.callback(move |key| Message::WorkingFileMenu(path.clone(), key))
            }),
            sequence: Box::new(|key: &'static str| {
                context.callback(move |()| Message::Command(key.to_owned()))
            }),
        };
        // 途中の操作の帯の見出しと説明（コンフリクトの数は status から）。
        let conflicts = self.status.as_ref().map_or(0, |s| {
            s.status
                .entries
                .iter()
                .filter(|e| e.kind == amonhen_model::StatusKind::Unmerged)
                .count()
        });
        let sequence = self.sequence.as_ref().map(|s| {
            (
                crate::sequence::title(s),
                crate::sequence::detail(s, conflicts),
                crate::sequence::can_skip(s),
            )
        });
        let view = crate::workcopy::view(
            crate::workcopy::Input {
                status: self.status.as_deref(),
                summary: &self.summary,
                description: &self.description,
                committing: self.committing,
                amend: self.amend,
                sequence: sequence
                    .as_ref()
                    .map(|(t, d, skip)| (t.as_str(), d.as_str(), *skip)),
            },
            callbacks,
        );
        // まん中の列だけを覆う。右の列は差分の欄（詳細の行は畳む）。
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .content(view)
    }

    /// History の右クリックのメニュー（GTK4 版の `commitops::menu`、macOS 版の `HistoryViewController` と同じ並び）。
    /// まだ動かない操作は出さない。右クリックは先に行を選ぶので、選んでいるコミットのメニューになる。
    fn history_menu(&self) -> Vec<MenuItem> {
        let Some((id, labels)) = &self.selected else {
            return Vec::new();
        };
        let head = self.refs.as_ref().and_then(|r| r.refs.head_branch.clone());
        let mut items = Vec::new();
        // Checkout: このコミットを指すローカルブランチ（今のブランチは除く）、リモートブランチ、なければコミット。
        let mut checkouts = 0;
        for label in labels.iter().take(5) {
            match label.kind {
                amonhen_model::RefKind::Local if Some(&label.name) != head.as_ref() => {
                    items.push(MenuItem::item(
                        format!("checkout:{}", label.name),
                        tr!("Checkout {name}…", name = label.name),
                    ));
                    checkouts += 1;
                }
                amonhen_model::RefKind::Remote => {
                    items.push(MenuItem::item(
                        format!("track:{}", label.name),
                        tr!("Checkout {name} as Local Branch…", name = label.name),
                    ));
                    checkouts += 1;
                }
                _ => {}
            }
        }
        if checkouts == 0 && !labels.iter().any(|l| Some(&l.name) == head.as_ref()) {
            items.push(MenuItem::item(
                format!("detach:{id}"),
                tr!("Checkout {name}…", name = &id[..8]),
            ));
        }
        items.push(MenuItem::item(
            format!("branch:{id}"),
            tr!("New Branch from {name}…", name = &id[..8]),
        ));
        items.push(MenuItem::item(
            format!("tag:{id}"),
            tr!("New Tag at {name}…", name = &id[..8]),
        ));
        items.push(MenuItem::item(
            format!("reset:{id}"),
            tr!(
                "Reset {branch} to {id}…",
                branch = self.head_name(),
                id = &id[..8]
            ),
        ));
        items.push(MenuItem::separator("-pick"));
        items.push(MenuItem::item(
            format!("cherry-pick:{id}"),
            tr!("Cherry-pick {id}…", id = &id[..8]),
        ));
        items.push(MenuItem::item(
            format!("revert:{id}"),
            tr!("Revert {id}…", id = &id[..8]),
        ));
        items.push(MenuItem::separator("-copy"));
        items.push(MenuItem::item(format!("copy:{id}"), tr!("Copy Commit ID")));
        // このコミットを指すブランチ・タグの名前（リモートブランチは origin/ を付けたまま。5 つまで）。
        for label in labels.iter().take(5) {
            let name = &label.name;
            let title = if label.kind == amonhen_model::RefKind::Tag {
                tr!("Copy Tag Name “{name}”", name = name)
            } else {
                tr!("Copy Branch Name “{name}”", name = name)
            };
            items.push(MenuItem::item(format!("copy:{name}"), title));
        }
        items
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

/// 切り替える先（GTK4 版の `sheets::CheckoutTarget`）。
enum CheckoutTarget {
    Branch(String),
    /// リモートのブランチを追跡するローカルブランチを作って切り替える。
    Track(String),
    /// コミットを detached HEAD で。
    Detach(String),
}

/// 見出しに出す起点（全桁のコミット ID なら 8 桁に。GTK4 版の `titles::shown_start`）。
fn shown_start(rev: &str) -> &str {
    let full = matches!(rev.len(), 40 | 64) && rev.bytes().all(|b| b.is_ascii_hexdigit());
    if full { &rev[..8] } else { rev }
}

/// Pull の取り込み方（GTK4 版の `PULL_MODES`。Settings の Pull の既定と同じ名前）。
/// Reset の動かし方（Soft・Mixed・Hard の順）。
const RESET_MODES: [&str; 3] = ["soft", "mixed", "hard"];

/// Merge の取り込み方（GTK4 版の `MERGE_MODES`。相手が 1 つなので Fast-Forward Only も選べる）。
const MERGE_MODES: [&str; 4] = ["git", "fastForwardOnly", "noFastForward", "squash"];

fn merge_mode_title(mode: &str) -> String {
    match mode {
        "fastForwardOnly" => "Fast-Forward Only".to_owned(),
        "noFastForward" => tr!("Always Create a Merge Commit"),
        "squash" => tr!("Squash into One Commit"),
        _ => tr!("Follow git’s settings"),
    }
}

const PULL_MODES: [&str; 4] = ["git", "merge", "rebase", "fastForwardOnly"];

fn pull_mode_title(mode: &str) -> String {
    match mode {
        "git" => tr!("Follow git’s settings"),
        "rebase" => "Rebase".to_owned(),
        "fastForwardOnly" => "Fast-Forward Only".to_owned(),
        _ => "Merge".to_owned(),
    }
}

/// Working Copy の status を読み直してもらう（裏のスレッドから。描き手は UI のスレッドにいるので、知らせだけ送る）。
fn with_status_reload() {
    STATUS_RELOAD.store(true, std::sync::atomic::Ordering::Release);
}

/// 裏のスレッドが頼んだ status の読み直し（UI のスレッドの描くついでに見る）。
static STATUS_RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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

/// 命令を順に裏で実行する（失敗したらそこで止める）。
fn perform_all(
    context: &ComponentContext<Root>,
    repo: Arc<Repository>,
    commands: Vec<Value>,
    title: String,
) {
    context.spawn_background(move |_| {
        for command in commands {
            let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
            if let Some(e) = reply["error"].as_str() {
                return Message::Performed(Some((title, e.to_owned())));
            }
        }
        Message::Performed(None)
    });
}

/// リモートのシートで書く命令（`changeRemote` の列。GTK4 版の `remotesheet::changes`）。`original` が None なら
/// 新しいリモート。
fn remote_changes(
    original: Option<&(String, String, String)>,
    name: &str,
    fetch: &str,
    push: &str,
) -> Vec<Value> {
    let mut out = Vec::new();
    match original {
        Some((old_name, old_fetch, old_push)) => {
            if old_name != name {
                out.push(json!({ "kind": "rename", "from": old_name, "to": name }));
            }
            if old_fetch != fetch {
                out.push(json!({ "kind": "setUrl", "name": name, "url": fetch }));
            }
            let had_push = if old_push != old_fetch {
                old_push.as_str()
            } else {
                ""
            };
            if had_push != push {
                let url = if push.is_empty() {
                    Value::Null
                } else {
                    json!(push)
                };
                out.push(json!({ "kind": "setPushUrl", "name": name, "url": url }));
            }
        }
        None => {
            out.push(json!({ "kind": "add", "name": name, "url": fetch }));
            if !push.is_empty() {
                out.push(json!({ "kind": "setPushUrl", "name": name, "url": push }));
            }
        }
    }
    out.into_iter()
        .map(|change| json!({ "op": "changeRemote", "change": change }))
        .collect()
}

/// 文字をクリップボードに入れる。
fn copy_text(text: &str) {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    const CF_UNICODETEXT: u32 = 13;
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: 確保した大きさの中に書き、クリップボードに渡したら持ち主はクリップボードになる。
    unsafe {
        // 窓を渡さずに開くと、空にしたときに持ち主がなくなり、`SetClipboardData` が失敗する。
        let window = windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow();
        if let Err(e) = OpenClipboard(Some(window)) {
            eprintln!("クリップボードを開けない: {e}");
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2) {
            let target = GlobalLock(memory).cast::<u16>();
            if !target.is_null() {
                std::ptr::copy_nonoverlapping(units.as_ptr(), target, units.len());
                let _ = GlobalUnlock(memory);
                if let Err(e) = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0))) {
                    eprintln!("クリップボードに入れられない: {e}");
                }
            }
            let _: HGLOBAL = memory;
        }
        let _ = CloseClipboard();
    }
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
    if !info["selection"].is_null() {
        return selection_details(&info["selection"]);
    }
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

/// 複数のコミットを選んだときの見出し（GTK4 版の `show_selection`）: 数、差分の取り方の説明、各コミットの 1 行。
fn selection_details(selection: &Value) -> View {
    let count = selection["count"].as_u64().unwrap_or(0);
    let note = match selection["kind"].as_str() {
        Some("range") => tr!(
            "They are consecutive, so this is the diff from the parent of the oldest to the newest (the same as squashing them into one)."
        ),
        Some("pair") => tr!(
            "Two non-consecutive commits, so this is the diff from the older to the newer (A..B)."
        ),
        _ => tr!(
            "They are not contiguous, so each commit’s diff is listed file by file (headed by the path and short ID)."
        ),
    };
    let secondary = Brush::Solid(Color::argb(0x99, 0x1c, 0x1c, 0x1f));
    let mut rows: Vec<View> = vec![
        TextBlock::new()
            .text(tr!("{count} commits selected", count = count))
            .font_size(14.0)
            .font_weight(FontWeight::SEMI_BOLD)
            .into(),
        TextBlock::new()
            .text(note)
            .font_size(12.0)
            .text_wrapping(TextWrapping::Wrap)
            .foreground(secondary)
            .into(),
    ];
    for line in selection["lines"].as_array().into_iter().flatten() {
        let text = format!(
            "{}  {}  — {}",
            line[0].as_str().unwrap_or_default(),
            line[1].as_str().unwrap_or_default(),
            line[2].as_str().unwrap_or_default()
        );
        rows.push(
            TextBlock::new()
                .text(text)
                .font_size(12.0)
                .text_trimming(TextTrimming::CharacterEllipsis)
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
    root: ElementRef<Grid>,
    /// 根にショートカットを付けた（付けるのを頼んだ）。
    root_input: bool,
    panel: ElementRef<SwapChainPanel>,
    area: ElementRef<Grid>,
    options: Options,
    attached: bool,
    input_requested: bool,
    diff: DiffPane,
    sender: LocalSender<Message>,
    /// サイドバーに送った ref（同じものは送り直さない）。
    refs_sent: Option<Arc<RefsSnapshot>>,
    /// Working Copy に送った status。
    status_sent: Option<Arc<StatusSnapshot>>,
    /// 詳細と差分を読みに行った選択。
    selected: Option<u32>,
    /// 詳細と差分を出しているコミットの行。
    shown: Option<u32>,
    /// 右クリックのメニューを開く位置（History の中の DIP）。
    context_at: Option<xaml::Point>,
    /// 開くのを頼んだ（開くまで頼み直さない）。
    menu_requested: bool,
    /// サイドバーの木（右クリックを付ける）と、付けたか、メニューを作り直し終えたか。
    sidebar: ElementRef<Grid>,
    /// サイドバーのメニューを付けておく、押されない空の Grid。
    sidebar_menu_host: ElementRef<Grid>,
    sidebar_input: bool,
    sidebar_host_input: bool,
    sidebar_menu_ready: bool,
    /// メニューを作り直した（次のフレームで開く）。
    menu_ready: bool,
    /// Working Copy を出している（差分の欄は選んだファイルの差分）。
    working_page: bool,
    /// 最後に探した問い合わせ（同じなら Enter で次の一致へ）。
    last_query: Option<String>,
    /// 差分を読みに行った複数選択。
    multi_sent: Vec<u32>,
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
        if !self.root_input {
            self.root_input = self.root.request_native_element(|result| match result {
                Ok(element) => {
                    if let Err(e) = attach_shortcuts(&element) {
                        eprintln!("ショートカットを付けられない: {e}");
                    }
                }
                Err(e) => eprintln!("窓の根を取れない: {e:?}"),
            });
        }
        if !self.diff.input_requested {
            self.diff.input_requested = request_input(&self.diff.area, Target::Diff);
        }
        if !self.sidebar_input {
            self.sidebar_input = self.sidebar.request_native_element(|result| match result {
                Ok(element) => {
                    if let Err(e) = attach_sidebar(&element) {
                        eprintln!("サイドバーに右クリックを付けられない: {e}");
                    }
                }
                Err(e) => eprintln!("サイドバーを取れない: {e:?}"),
            });
        }
        if !self.sidebar_host_input {
            self.sidebar_host_input =
                self.sidebar_menu_host
                    .request_native_element(|result| match result {
                        Ok(element) => {
                            if let Ok(ui) = element.cast::<xaml::UIElement>() {
                                SIDEBAR_MENU_HOST.with(|e| *e.borrow_mut() = Some(ui));
                            }
                        }
                        Err(e) => eprintln!("サイドバーのメニューの置き場を取れない: {e:?}"),
                    });
        }
        if std::mem::take(&mut self.sidebar_menu_ready) {
            open_sidebar_menu();
        }
        let s = &mut self.session;
        if STATUS_RELOAD.swap(false, std::sync::atomic::Ordering::AcqRel) {
            s.data.reload_status();
        }
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
        if let Some(status) = &s.data.status
            && !self
                .status_sent
                .as_ref()
                .is_some_and(|sent| Arc::ptr_eq(sent, status))
        {
            self.status_sent = Some(status.clone());
            self.sender.send(Message::Status(status.clone()));
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
        // 複数選んだ行が変わったら、まとめた差分を読む。
        if s.data.multi != self.multi_sent {
            self.multi_sent = s.data.multi.clone();
            if self.multi_sent.len() > 1 {
                s.data.want_selection(self.multi_sent.clone());
            } else {
                // 1 つに戻った: その行を読み直す。
                self.selected = None;
            }
        }
        // 選んだコミットが変わったら、詳細と差分を読む。右クリックのメニューにも知らせる。
        if s.selected != self.selected && s.data.multi.is_empty() {
            self.selected = s.selected;
            let selected = s.selected.and_then(|row| {
                let id = s
                    .data
                    .history
                    .as_ref()?
                    .graph
                    .ids()
                    .get(row as usize)?
                    .to_string();
                Some((id, s.data.labels.get(&row).cloned().unwrap_or_default()))
            });
            self.sender.send(Message::Selected(selected));
            if let Some(row) = s.selected {
                s.data.want_commit(row);
            }
        }
        // 検索の答え: 選んでいる行から下で最初の一致を選び、件数を出す（GTK4 版の `nearest_index`）。
        if let Some(done) = self.session.data.search.take() {
            let s = &mut self.session;
            let status = match done {
                crate::data::SearchDone::Found(_) if s.data.matches.is_empty() => {
                    tr!("No Matches")
                }
                crate::data::SearchDone::Found(_) => {
                    let from = s.selected.unwrap_or(0);
                    let matches = &s.data.matches;
                    let index = matches.iter().position(|&r| r >= from).unwrap_or(0);
                    let (row, total) = (matches[index], matches.len());
                    s.reveal(row);
                    format!("{} / {total}", index + 1)
                }
                crate::data::SearchDone::Failed(_, e) => e,
            };
            s.invalidate();
            self.sender.send(Message::SearchStatus(status));
        }
        if std::mem::take(&mut self.menu_ready) {
            self.open_context_menu();
        }
        let s = &mut self.session;
        // 右クリックで頼まれていれば、選んだ行を知らせた後に開いてもらう（知らせが先に届く）。
        if self.context_at.is_some() && !self.menu_requested {
            self.menu_requested = true;
            self.sender.send(Message::OpenHistoryMenu);
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
        if std::mem::take(&mut s.data.syntax_changed) && !self.working_page {
            s.renderer.forget_diff();
            self.diff.dirty = true;
        }
        if std::mem::take(&mut s.data.working_changed) && self.working_page {
            s.renderer.forget_diff();
            self.diff.scroll_y = 0.0;
            self.diff.dirty = true;
        }
        if self.diff.dirty
            && let Some(surface) = self.diff.surface.as_mut()
        {
            s.renderer
                .draw_diff(
                    surface,
                    if self.working_page {
                        s.data.working.as_deref().map(|w| w.content())
                    } else {
                        s.data.commit.as_deref().map(|c| c.content())
                    },
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

    /// History を探す。同じ問い合わせをもう探してあれば、選んでいる行から次（`backwards` なら前）の一致へ。空なら
    /// 一致を消す。
    fn search(&mut self, query: String, backwards: bool) {
        let s = &mut self.session;
        if query.is_empty() {
            self.last_query = None;
            s.data.matches.clear();
            s.invalidate();
            return;
        }
        if self.last_query.as_deref() == Some(query.as_str()) && !s.data.matches.is_empty() {
            let matches = &s.data.matches;
            let from = s.selected.unwrap_or(0);
            let index = if backwards {
                matches
                    .iter()
                    .rposition(|&r| r < from)
                    .unwrap_or(matches.len() - 1)
            } else {
                matches.iter().position(|&r| r > from).unwrap_or(0)
            };
            let (row, total) = (matches[index], matches.len());
            s.reveal(row);
            self.sender
                .send(Message::SearchStatus(format!("{} / {total}", index + 1)));
            return;
        }
        self.last_query = Some(query.clone());
        s.data.search(query);
    }

    /// 頼まれた右クリックのメニューを、押した位置で開く（メニューは選んだ行のものに作り直してある）。
    fn open_context_menu(&mut self) {
        self.menu_requested = false;
        let Some(at) = self.context_at.take() else {
            return;
        };
        let Some(element) = HISTORY_ELEMENT.with(|e| e.borrow().clone()) else {
            return;
        };
        let open = || -> windows_core_reactor::Result<()> {
            // reactor が作り直したメニューは要素に付き直しているので、外してから開く（付いていなければ前のもの）。
            let flyout = match element.ContextFlyout() {
                Ok(flyout) => {
                    element.SetContextFlyout(None::<&xaml::FlyoutBase>)?;
                    HISTORY_FLYOUT.with(|f| *f.borrow_mut() = Some(flyout.clone()));
                    flyout
                }
                Err(_) => match HISTORY_FLYOUT.with(|f| f.borrow().clone()) {
                    Some(flyout) => flyout,
                    None => return Ok(()),
                },
            };
            let options = xaml::FlyoutShowOptions::new()?;
            options.SetPosition(Some(at))?;
            flyout.ShowAtWithOptions(&element, &options)
        };
        if let Err(e) = open() {
            eprintln!("右クリックのメニューを開けない: {e}");
        }
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
        let data = &self.session.data;
        if self.working_page {
            data.working.as_ref().map_or(0, |w| w.diff.view.rows.len())
        } else {
            data.commit.as_ref().map_or(0, |c| c.diff.rows.len())
        }
    }

    /// 選んでいるコミットの詳細と差分を読み直す（Stashes の画面から戻ったとき）。
    fn reload_selected(&mut self) {
        self.selected = None;
        self.multi_sent.clear();
    }

    /// 差分の欄に出すもの（History の選んだコミットか、Working Copy の選んだファイル）を替える。
    fn set_working_page(&mut self, working: bool) {
        if self.working_page != working {
            self.working_page = working;
            self.session.renderer.forget_diff();
            self.diff.scroll_y = 0.0;
            self.diff.dirty = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    History,
    Diff,
}

/// 検索の欄のキー: Enter で探す（同じ問い合わせなら次へ）、Shift+Enter で前へ、Esc で閉じる（GTK4 版と同じ）。
fn attach_search_keys(element: &IInspectable) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    let revoker = ui.PreviewKeyDown(|_, args: Ref<xaml::KeyRoutedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        let Ok(key) = args.Key() else { return };
        let message = match key {
            xaml::VirtualKey::Enter => {
                let shift =
                    xaml::InputKeyboardSource::GetKeyStateForCurrentThread(xaml::VirtualKey::Shift)
                        .is_ok_and(|s| s.contains(xaml::CoreVirtualKeyStates::Down));
                Message::SearchSubmit(shift)
            }
            xaml::VirtualKey::Escape => Message::CloseSearch,
            _ => return,
        };
        let _ = args.SetHandled(true);
        with_history(|h| {
            h.sender.send(message);
        });
    })?;
    REVOKERS.with(|r| r.borrow_mut().push(revoker));
    Ok(())
}

/// ダイアログの入力の欄の Enter で、既定のボタンを押したことにする。
fn attach_enter(element: &IInspectable) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    let revoker = ui.PreviewKeyDown(|_, args: Ref<xaml::KeyRoutedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        if args.Key().ok() != Some(xaml::VirtualKey::Enter) {
            return;
        }
        let _ = args.SetHandled(true);
        with_history(|h| {
            h.sender
                .send(Message::DialogClosed(ContentDialogResult::Primary));
        });
    })?;
    REVOKERS.with(|r| r.borrow_mut().push(revoker));
    Ok(())
}

/// 窓の根に、メニューのショートカットを付ける。メニューの項目の KeyboardAccelerator は、TreeView・ListView に
/// フォーカスがあると届かない（その部品がキーを先に使う）ので、根の PreviewKeyDown で先に受けて操作を送る。
fn attach_shortcuts(element: &IInspectable) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    let revoker = ui.PreviewKeyDown(|_, args: Ref<xaml::KeyRoutedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        let Ok(key) = args.Key() else { return };
        let down = |key: xaml::VirtualKey| {
            xaml::InputKeyboardSource::GetKeyStateForCurrentThread(key)
                .is_ok_and(|s| s.contains(xaml::CoreVirtualKeyStates::Down))
        };
        let pressed = MenuShortcut {
            key: key.0 as u16,
            control: down(xaml::VirtualKey::Control),
            shift: down(xaml::VirtualKey::Shift),
            alt: down(xaml::VirtualKey::Menu),
        };
        // 修飾キーなしのキー（文字の入力・矢印）は部品に任せる。
        if !pressed.control && !pressed.alt {
            return;
        }
        if let Some(id) = crate::menus::command_for(pressed) {
            let _ = args.SetHandled(true);
            with_history(|h| {
                h.sender.send(Message::Command(id.to_owned()));
            });
        }
    })?;
    REVOKERS.with(|r| r.borrow_mut().push(revoker));
    Ok(())
}

/// サイドバーの木に右クリックを付ける。押した所から親をたどって木の項目（`DataContext` が TreeViewNode）を探し、
/// そのキーを知らせる。メニューは押した項目のものに作り直してから開く（WinUI が今のメニューを開かないよう、
/// 扱ったことにする）。
fn attach_sidebar(element: &IInspectable) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    SIDEBAR_ELEMENT.with(|e| *e.borrow_mut() = Some(ui.clone()));
    let source = ui.clone();
    let revoker = ui.ContextRequested(move |_, args: Ref<xaml::ContextRequestedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        let _ = args.SetHandled(true);
        let Ok(original) = args.OriginalSource() else {
            return;
        };
        let Some((item, key)) = tree_item_key(&original) else {
            return;
        };
        let mut at = xaml::Point::default();
        let place = if args.TryGetPosition(&source, &mut at).unwrap_or(false) {
            (source.clone(), Some(at))
        } else {
            (item, None)
        };
        SIDEBAR_AT.with(|a| *a.borrow_mut() = Some(place));
        with_history(|h| {
            h.sender.send(Message::SidebarMenu(key));
        });
    })?;
    REVOKERS.with(|r| r.borrow_mut().push(revoker));
    Ok(())
}

/// `from` から親へたどり、木の項目の要素とそのキーを返す。
fn tree_item_key(from: &IInspectable) -> Option<(xaml::UIElement, String)> {
    let mut node: xaml::DependencyObject = from.cast().ok()?;
    for _ in 0..32 {
        if let Ok(element) = node.cast::<xaml::FrameworkElement>()
            && let Ok(context) = element.DataContext()
            && let Ok(unknown) = context.cast::<windows_core_reactor::IUnknown>()
            && let Some(key) = windows_reactor::tree_node_key_by_identity(unknown.as_raw() as usize)
        {
            return Some((element.cast().ok()?, key));
        }
        node = xaml::VisualTreeHelper::GetParent(&node).ok()?;
    }
    None
}

/// 作り直したサイドバーのメニューを、右クリックした所で開く。
fn open_sidebar_menu() {
    let (Some(element), Some((target, at))) = (
        SIDEBAR_MENU_HOST.with(|e| e.borrow().clone()),
        SIDEBAR_AT.with(|a| a.borrow_mut().take()),
    ) else {
        return;
    };
    let open = || -> windows_core_reactor::Result<()> {
        let flyout = element.ContextFlyout()?;
        let options = xaml::FlyoutShowOptions::new()?;
        if let Some(at) = at {
            options.SetPosition(Some(at))?;
        }
        flyout.ShowAtWithOptions(&target, &options)
    };
    if let Err(e) = open() {
        eprintln!("サイドバーのメニューを開けない: {e}");
    }
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
            let left = properties.IsLeftButtonPressed().unwrap_or(false);
            let right = properties.IsRightButtonPressed().unwrap_or(false);
            if !left && !right {
                return;
            }
            let _ = source.Focus(xaml::FocusState::Pointer);
            // 右クリック: WinUI が今のメニュー（選び直す前の行のもの）を開かないよう、要素から外しておく。
            if right
                && target == Target::History
                && let Ok(flyout) = source.ContextFlyout()
            {
                let _ = source.SetContextFlyout(None::<&xaml::FlyoutBase>);
                HISTORY_FLYOUT.with(|f| *f.borrow_mut() = Some(flyout));
            }
            with_history(|h| match target {
                // 右クリックも、先にその行を選ぶ（メニューはそのコミットのもの）。
                Target::History => {
                    if let Some(row) = h.session.row_at(position.Y) {
                        // Ctrl+クリックで足す・外す、Shift+クリックで範囲（右クリックは 1 つを選び直さない）。
                        let down = |key: xaml::VirtualKey| {
                            xaml::InputKeyboardSource::GetKeyStateForCurrentThread(key)
                                .is_ok_and(|s| s.contains(xaml::CoreVirtualKeyStates::Down))
                        };
                        let ctrl = down(xaml::VirtualKey::Control);
                        let shift = down(xaml::VirtualKey::Shift);
                        if right && h.session.data.multi.contains(&row) {
                        } else if left && ctrl {
                            h.session.toggle_select(row);
                        } else if left && shift {
                            h.session.select_range(row);
                        } else {
                            h.session.select(row);
                        }
                    }
                }
                Target::Diff if !left => {}
                // Working Copy の差分: 左の印を押すと、その行を含める・外す。
                Target::Diff if h.working_page => {
                    let row = ((h.diff.scroll_y + position.Y) / DIFF_ROW_HEIGHT).floor();
                    if row >= 0.0 && position.X < crate::render::DIFF_MARK_WIDTH {
                        let row = row as usize;
                        let change = h.session.data.working.as_ref().is_some_and(|w| {
                            w.diff.view.rows.get(row).is_some_and(|r| {
                                matches!(
                                    r.kind,
                                    amonhen_layout::DiffRow::ADDED
                                        | amonhen_layout::DiffRow::REMOVED
                                )
                            })
                        });
                        if change {
                            h.session.data.toggle_working_row(row);
                        }
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
            // 右クリックは扱ったことにしない（WinUI が右クリックのメニューを開く）。
            if left {
                let _ = args.SetHandled(true);
            }
        })?,
    );

    if target == Target::History {
        HISTORY_ELEMENT.with(|e| *e.borrow_mut() = Some(ui.clone()));
        // 右クリックで選び直す前のメニューが開かないよう、押したときにメニューを外してある。ここで位置を覚え、選んだ
        // 行のメニューに作り直してから開く（キーボードで開くときは、付いたままのメニューを WinUI が開く）。
        let source = ui.clone();
        revokers.push(ui.ContextRequested(
            move |_, args: Ref<xaml::ContextRequestedEventArgs>| {
                let Some(args) = args.as_ref() else { return };
                let _ = args.SetHandled(true);
                let mut at = xaml::Point::default();
                let pointer = args.TryGetPosition(&source, &mut at).unwrap_or(false);
                with_history(|h| {
                    if !pointer {
                        // キーボード（アプリケーションキー・Shift+F10）: 選んでいる行の左の辺り。
                        let s = &h.session;
                        let Some(row) = s.selected else { return };
                        at = xaml::Point {
                            X: 40.0,
                            Y: s.row_top(row) + ROW_HEIGHT / 2.0,
                        };
                    }
                    h.context_at = Some(at);
                });
            },
        )?);
    }

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
