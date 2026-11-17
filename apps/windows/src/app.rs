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
    /// ほかの worktree の変更を最後に数えた時刻と、全部を数え直した時刻（10 秒ごと・5 分ごと。GTK4 版と同じ）。
    static WORKTREE_TICK: std::cell::Cell<(Instant, Instant)> =
        std::cell::Cell::new((Instant::now(), Instant::now()));
    /// Activity を開いている間、読み直しを頼んだ最後の時刻（0.5 秒ごとに頼む）。
    static ACTIVITY_TICK: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    /// 自動の fetch の間隔（分。0 はしない）と、最後に数え始めた時。
    static AUTO_FETCH: std::cell::Cell<(u32, Instant)> =
        std::cell::Cell::new((crate::prefs::auto_fetch_minutes(), Instant::now()));
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
    // Repositories の「開いた日時」・すべての一覧に出す（まだどこにもなければブックマークにも入れる）。
    if let Ok(path) = std::path::absolute(&args.repo)
        && path.join(".git").exists()
    {
        crate::repositories::record_opened(&path);
    }
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
    /// タグのリモートでの様子（コアの `tagRemotes` の info。リモートと読んだ時刻、タグごとの same・different・
    /// missing）。
    tag_remotes: Value,
    /// Pull・Push のダイアログのコミットの一覧（見出し・数・(oid, 件名, 作者, 時刻)・消えるものか）。
    sync_lists: Vec<SyncList>,
    /// History で複数選んでいるコミット（上から。右クリックのメニューを複数のもの用にする）。
    multi: Vec<(String, bool)>,
    /// Push のダイアログの注意（GTK4 版の `push_warnings`）。
    sync_warnings: Vec<String>,
    /// stash の詳しい中身（名前 → 中身。ref が変わるたびに読み直す）。
    stash_details: std::collections::HashMap<String, crate::stashes::Detail>,
    /// ほかの worktree の変更の数（パス → 数）。
    worktree_counts: std::collections::HashMap<String, u64>,
    /// submodule の様子（コアの `submodules` の info の `submodules`）。
    submodules: Value,
    /// subtree（コアの `subtrees` の info。`available` と `subtrees`）。
    subtrees: Value,
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
    /// 下書きを覚える鍵（開いている worktree のもの）。
    draft_key: Option<String>,
    /// コミットできたら、ref を読み直したところで push する（Commit and Push）。
    push_after_commit: bool,
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
    /// Range Diff の画面（開いている間だけ。サイドバーに行を出す）と、比べた回数（古い答えを捨てる）。
    range_diff: Option<crate::rangediff::State>,
    range_request: u64,
    /// Reflog の画面の行（読むまでは None）と、選んでいる行。
    reflog: Option<Vec<crate::reflog::Entry>>,
    reflog_selected: Option<usize>,
    /// worktree（`worktrees` の答え。開いたリポジトリのもの）。
    worktrees: Vec<WorktreeInfo>,
    /// リポジトリの形（shallow・partial clone、sparse checkout の印）。
    form: sidebar::RepositoryForm,
    /// サイドバーの絞り込み（Branches・Remotes・Tags）。
    sidebar_filter: String,
    /// ステータスバーに添える知らせ（自動の fetch の失敗など）。
    status_note: Option<String>,
    /// Working Copy のパスの絞り込み。
    working_filter: String,
    /// Working Copy の木で畳んでいるディレクトリ。
    working_collapsed: std::collections::HashSet<String>,
    /// Working Copy の一覧の見せ方と、切り替えのボタンを押した回数。
    working_layout: crate::workcopy::Layout,
    layout_clicks: u64,
    /// Working Copy のファイルごとの行数（`workingStats`。status が替わるたびに読み直す）。
    working_stats: std::collections::HashMap<String, (Option<u64>, Option<u64>)>,
    /// 最近のコミットの（メッセージ, 作者, メール）。最近のメッセージと Co-authored-by の候補。
    recent_messages: Vec<(String, String, String)>,
    /// Stashes の画面で選んでいる stash の名前。
    stash: Option<String>,
    /// File History の画面（パス、読んだコミット、選んでいる行）。
    file_history: Option<crate::filehistory::State>,
    /// Blame の画面（パス、コミット、読んだ行）。閉じたら前の画面（File History なら File History）に戻る。
    blame: Option<crate::blame::State>,
    blame_from: Page,
    /// Repository Settings の画面と、閉じたら戻る画面。
    repo_settings: Option<crate::reposettings::State>,
    repo_settings_from: Page,
    /// Repositories の画面と、閉じたら戻る画面・リポジトリの一覧を包む Grid（ダブルクリック・Enter を付ける）。
    repositories: Option<crate::repositories::State>,
    repositories_from: Page,
    repositories_list: ElementRef<Grid>,
    /// Activity の画面と、閉じたら戻る画面。
    activity: Option<crate::activity::State>,
    activity_from: Page,
    /// Settings の画面と、閉じたら戻る画面。
    settings: Option<crate::settingsview::State>,
    settings_from: Page,
    /// マージエディタ（閉じたら Working Copy に戻る）。
    merge: Option<crate::merge::State>,
    /// 途中の操作（`sequence` の答えの info）。なければ None。
    sequence: Option<Value>,
    /// ダイアログの入力の欄（Enter で既定のボタンを押す）と、開いた回数（開いた後にキーを付ける）。
    dialog_input: ElementRef<TextBox>,
    dialog_inputs: u64,
    /// Working Copy の一覧を包む Grid（Space で含める・外す、Delete・Backspace で捨てる）。
    working_list: ElementRef<Grid>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    History,
    WorkingCopy,
    Stashes,
    FileHistory,
    Blame,
    Merge,
    Settings,
    Activity,
    RepoSettings,
    Repositories,
    Reflog,
    RangeDiff,
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
    /// ファイルをある版に戻す（GTK4 版の `restoresheet::ask`）。
    Restore {
        repo: Arc<Repository>,
        title: String,
        rev: String,
        paths: Vec<String>,
        staged: bool,
    },
    /// ブランチの切り替え（GTK4 版の `branchswitcher`）: 打った問い合わせと、選んでいる候補。
    /// File > Clone…（GTK4 版の `repos::clone`）。欄は URL・置き場所・フォルダの名前、取得する範囲
    /// （すべて・中身は必要なときに・最近の履歴だけ）と最近の履歴の数、一部のフォルダだけ・submodule も。
    Clone {
        url: String,
        folder: String,
        name: String,
        download: usize,
        depth: usize,
        sparse: bool,
        submodules: bool,
    },
    /// Quick Open（リポジトリをあいまい検索して新しい窓で開く）。
    QuickOpen {
        query: String,
        selected: usize,
        candidates: Vec<crate::repositories::Candidate>,
    },
    Switcher {
        repo: Arc<Repository>,
        query: String,
        selected: usize,
        recent: Vec<String>,
    },
    /// 履歴のない新しいブランチ（GTK4 版の `new_orphan_branch`）。`keep` は今のファイルを残すか。
    NewOrphan {
        repo: Arc<Repository>,
        name: String,
        keep: bool,
    },
    /// ignore されたファイルを消す（git clean -X）。消すものを先に一覧で見せる（GTK4 版の `delete_ignored_files`）。
    DeleteIgnored {
        repo: Arc<Repository>,
        paths: Vec<String>,
    },
    /// Clean Up Branches（GTK4 版の `clean_up_branches`）: `branchCleanup` の答えのブランチと、消すか。
    CleanUp {
        repo: Arc<Repository>,
        base: String,
        branches: Vec<(Value, bool)>,
    },
    /// Fetch の確認（リモートが 2 つ以上なら、どこから取るかを選ぶ。0 はすべて）。
    Fetch {
        repo: Arc<Repository>,
        remotes: Vec<String>,
        from: usize,
        /// リモートごとの、消えたブランチを片づける設定（`remote.<name>.prune`、なければ `fetch.prune`）が
        /// true ならその名前。
        prune: Vec<Option<String>>,
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
    /// コミットのメッセージを書き直す（Edit Message…）か、1 つ古いコミットとまとめる（Combine with Older
    /// Commit…）。`squash` なら後者。GTK4 版は編集モードを開くが、ここではすぐ書き換える。
    Reword {
        repo: Arc<Repository>,
        oid: String,
        message: String,
        squash: bool,
    },
    /// コミット（古い順）をパッチにする（GTK4 版の `patches::save`）。形・送り状・版を選んでから、書き出す場所を選ぶ。
    Patch {
        repo: Arc<Repository>,
        oids: Vec<String>,
        /// 1 つの mbox にまとめる（でなければ 1 コミット 1 ファイル）。
        single: bool,
        cover: bool,
        version: String,
    },
    /// `rev` の時点のファイルを書き出す（GTK4 版の `archive::export`）。形と先頭のフォルダを選んでから、保存する
    /// ファイルを選ぶ。1 つのフォルダだけを書き出すこと、partial clone の中身を先に取る確認はまだない。
    Archive {
        repo: Arc<Repository>,
        rev: String,
        format: usize,
        top: String,
    },
    /// `refs`（空なら HEAD とすべてのブランチ・タグ）を bundle に書く（GTK4 版の `bundle::save`）。入れる範囲を
    /// 選んでから、コミットの数を確かめ、保存するファイルを選ぶ。partial clone の中身を先に取る確認はまだない。
    Bundle {
        repo: Arc<Repository>,
        refs: Vec<String>,
        /// 見せる名前と、既定のファイル名に使う名前。
        what: String,
        label: String,
        /// ある基点より後だけ（`base`）。
        newer: bool,
        base: String,
    },
    /// bundle のブランチを `refs/remotes/<名前>/*` に取り込む（GTK4 版の `bundle::fetch`）。
    FetchBundle {
        repo: Arc<Repository>,
        file: String,
        message: String,
        name: String,
    },
    /// 新しい worktree（GTK4 版の `worktrees::new_worktree`）: `start` からブランチを作って新しいフォルダに checkout
    /// する。フォルダの既定は、リポジトリの隣の「<名前>-<ブランチ>」。
    NewWorktree {
        repo: Arc<Repository>,
        start: String,
        branch: String,
        folder: String,
    },
    /// worktree をロックする（理由は任意）。
    LockWorktree {
        repo: Arc<Repository>,
        path: String,
        name: String,
        reason: String,
    },
    /// worktree のフォルダを移す（`repair` なら、手で移した先を教えてつなぎ直す）。GTK4 版の `move_to`・`locate`。
    WorktreeFolder {
        repo: Arc<Repository>,
        path: String,
        name: String,
        to: String,
        repair: bool,
    },
    /// パッチを当てる前の確認（GTK4 版の `patches::confirm`）。mbox ならパッチごとにコミットにする（git am）、
    /// でなければ作業ツリーに入れる（git apply）。前後の絵はまだない。
    ApplyPatch {
        repo: Arc<Repository>,
        paths: Vec<String>,
        mailbox: bool,
        title: String,
        what: String,
        /// パッチとファイルの一覧（件名・作者、ファイルごとの印と増減）。
        list: String,
        /// そのまま当たるか（当たらなければその理由）。
        applies: Result<(), String>,
        three_way: bool,
        index: bool,
    },
    /// 作業中の変更を stash する（GTK4 版の `Actions::stash_paths`）。`paths` があればそのファイルだけ。
    Stash {
        repo: Arc<Repository>,
        paths: Vec<String>,
        message: String,
        staged: bool,
        keep_index: bool,
        untracked: bool,
    },
    /// 保存していない Result を捨てて、マージエディタを閉じてよいか。
    CloseMerge,
    /// コンフリクトが残っているファイルを解決済みにしてよいか。
    MarkResolved { path: String, count: u64 },
    /// 入力の欄（見出し・値・薄い文字）とチェック（名前・値）のダイアログ（submodule のシートなど）。押すと
    /// `kind` で命令にする。
    Form {
        repo: Arc<Repository>,
        title: String,
        message: String,
        button: String,
        fields: Vec<(String, String, String)>,
        checks: Vec<(String, bool)>,
        kind: FormKind,
    },
    /// Repositories の行の表示名を変える。
    RenameRepository { location: String, name: String },
    /// Repositories のグループを作る（リポジトリがあればそこに移す）。
    NewGroup {
        location: Option<String>,
        name: String,
    },
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
    /// Range Diff: 前の版を探した（範囲と説明、なければ説明だけ）・比べた・選んだ・範囲を書き換えた・比べ直す・閉じる。
    RangePrevious(u64, Result<(Vec<String>, String), String>),
    RangeDiffLoaded(u64, Result<Vec<crate::rangediff::Pair>, String>),
    RangeDiffSelected(Option<usize>),
    /// 一覧の行を選んだ（一覧に出している組の番号）と、Changes Only を切り替えた。
    RangeDiffRow(Option<usize>),
    RangeDiffChangesOnly(bool),
    RangeDiffRanges(String),
    RangeDiffCompare,
    CloseRangeDiff,
    /// worktree を読んだ。
    Worktrees(Vec<WorktreeInfo>),
    /// 裏で調べてから確かめる（題、説明、ボタン、命令）。
    Confirm(Arc<Repository>, String, String, String, Value),
    /// Working Copy のパスの絞り込み。
    WorkingFilter(String),
    /// Repository Settings の Operations の選ぶ欄（行の id と、選んだ番号）。
    RepoSettingsChoice(&'static str, Option<usize>),
    /// サイドバーの絞り込み。
    SidebarFilter(String),
    /// リポジトリの形を読んだ。
    Form(sidebar::RepositoryForm),
    /// 戻したファイルを stage もする。
    RestoreStaged(bool),
    /// ブランチの切り替えで候補を選んだ。
    SwitcherSelect(Option<usize>),
    /// Clone の欄（0 は URL、1 は置き場所、2 はフォルダの名前）・取得する範囲・数・チェック（0 は一部のフォルダ
    /// だけ、1 は submodule も）・置き場所を選ぶ。
    CloneField(usize, String),
    CloneDownload(Option<usize>),
    CloneDepth(Option<usize>),
    CloneCheck(usize, bool),
    CloneChoose,
    /// clone が終わった（作ったフォルダと、失敗なら理由）。
    Cloned(std::path::PathBuf, Option<String>),
    /// Quick Open の候補を、スキャンで見つけたものも入れて読み直した。
    QuickOpenCandidates(Vec<crate::repositories::Candidate>),
    /// New Orphan Branch の「今のファイルを残す」。
    OrphanKeep(bool),
    /// ignore されたファイルを読んだ（`ignoredFiles` の答え）。
    IgnoredLoaded(Arc<Repository>, Value),
    /// Clean Up Branches の候補を読んだ（`branchCleanup` の答え）。
    CleanUpLoaded(Arc<Repository>, Value),
    /// Clean Up Branches のブランチのチェック。
    CleanUpCheck(usize, bool),
    /// Working Copy の一覧の見せ方を替える。
    WorkingLayout(crate::workcopy::Layout),
    /// ディレクトリの中のファイルをまとめて含める・外す。
    StageAll(Vec<String>, bool),
    /// Working Copy のファイルごとの行数を読んだ（読んだ status の generation と、パスごとの行数）。
    WorkingStats(
        u64,
        std::collections::HashMap<String, (Option<u64>, Option<u64>)>,
    ),
    /// Working Copy の一覧のキー（true は Delete・Backspace で捨てる、false は Space で含める・外す）。
    WorkingKey(bool),
    /// 下書きのない worktree を開いたときに読んだテンプレート（下書きの鍵と中身）。
    CommitTemplate(String, String),
    /// 最近のコミットのメッセージを読んだ。
    RecentMessages(Vec<(String, String, String)>),
    /// 最近のメッセージ・「+」のメニューの項目。
    CommitTextMenu(String),
    /// Reflog を読んだ・一覧で選んだ。
    ReflogLoaded(Vec<crate::reflog::Entry>),
    ReflogSelected(Option<usize>),
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
    /// マージエディタを開く（パス）・読み終えた・ボタン・Result を書き換えた。
    OpenMerge(String),
    MergeLoaded(String, Result<Value, String>),
    MergeButton(&'static str),
    MergeResult(String),
    /// Settings: git の名前とメールを読んだ・欄を書き換えた・保存・チェックを替えた・閉じる。
    SettingsGit(String, String),
    SettingsField(usize, String),
    SettingsSaveGit,
    SettingsToggle(&'static str, bool),
    CloseSettings,
    /// Activity: タブを選んだ・Undo の記録を読んだ・戻す数を選んだ・読み直す時刻・閉じる。
    ActivityTab(Option<usize>),
    ActivityUndoLog(Vec<Value>),
    ActivityUndo(usize),
    ActivityTick,
    /// 自動の fetch の間隔を選んだ（Settings）。
    SettingsAutoFetch(Option<usize>),
    /// Settings > General の外のエディタを選んだ（項目の番号）。
    SettingsEditor(Option<usize>),
    /// History のキャッシュの大きさを読んだ。
    CacheSize(u64),
    /// History のキャッシュを消す。
    ClearCache,
    /// 自動の fetch の時が来た。
    AutoFetch,
    /// 自動の fetch が終わった（失敗なら理由）。
    AutoFetched(Option<String>),
    /// Activity の Operations の記録を JSON に書き出す。
    ActivityExport,
    /// Activity の History のキャッシュを読んだ（リポジトリの名前と `historyCache` の info）。
    ActivityCache(String, Value),
    CloseActivity,
    /// Repositories の操作。
    Repos(crate::repositories::Event),
    /// History で複数選んだコミット（上から。1 つ以下なら空）。
    MultiSelected(Vec<(String, bool)>),
    /// Pull・Push のダイアログのコミットの一覧を読んだ。
    SyncLists(Vec<SyncList>, Vec<String>),
    /// stash の詳しい中身を読んだ。
    StashDetails(std::collections::HashMap<String, crate::stashes::Detail>),
    /// ほかの worktree の変更を数える（true なら全部を数え直す）と、その答え（パスと数）。
    CountWorktrees(bool),
    WorktreeCounts(Vec<(String, u64)>),
    /// タグのリモートでの様子を読んだ（`tagRemotes` の info）。
    TagRemotes(Value),
    /// submodule の様子を読んだ。
    Submodules(Value),
    /// subtree を読んだ（`subtrees` の info）。
    Subtrees(Value),
    /// 入力の欄とチェックのダイアログ（`Dialog::Form`）の欄・チェックを変えた。
    FormField(usize, String),
    FormCheck(usize, bool),
    /// まとめるときのメッセージの初期値を読んだ（コミット、メッセージ）。
    SquashText(String, Result<String, String>),
    /// bundle に入るコミットを数えた（`bundlePreview` の答えと、書く命令）。
    BundlePreview(Arc<Repository>, String, Value, Value),
    /// 取り込む bundle を調べた（ファイル、`bundleCheck` の答え）。
    BundleChecked(Arc<Repository>, String, Value),
    /// 新しい worktree の欄（0 はブランチ、1 はフォルダ）。
    WorktreeField(usize, String),
    /// bundle の範囲（全体か、基点より後か）を選んだ。
    BundleNewer(bool),
    /// 当てるパッチを読んだ（パス、`readPatch` の答え）。
    PatchRead(Arc<Repository>, Vec<String>, Value),
    /// パッチを当てるダイアログの選択（`three-way`・`index`）。
    ApplyOption(&'static str, bool),
    /// 書き出す形を選んだ。
    ArchiveFormat(Option<usize>),
    /// パッチのダイアログの選択（`single`・`cover`）。
    PatchOption(&'static str, bool),
    /// Stash のダイアログの選択（`all`・`staged`・`keep-index`・`untracked`）。
    StashOption(&'static str, bool),
    /// Repository Settings: 設定を読んだ・欄を書き換えた・ボタン。
    RepoSettingsLoaded(Vec<crate::reposettings::Entry>),
    /// Repository Settings のフックの有効を切り替えた。
    HookEnabled(String, bool),
    /// Repository Settings の専用のテンプレートを読んだ（中身、置き場所）。
    RepoTemplateLoaded(Option<String>, String),
    /// Repository Settings の既定のブランチを判定した。
    RepoDetectedBase(Option<String>),
    /// Repository Settings のリポジトリの形を読んだ（`repositoryForm` の info）。
    RepoFormLoaded(Value),
    /// Repository Settings のフック・LFS を読んだ（`hooksAndLfs` の info）。
    RepoHooksLoaded(Value),
    /// Repository Settings で見るフックの中身を読んだ（名前と中身）。
    HookScript(String, String),
    /// Repository Settings の `.gitignore` と `.git/info/exclude` を読んだ。
    RepoIgnoreLoaded(String, String),
    RepoSettingsField(&'static str, String),
    RepoSettingsButton(String),
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
    /// Fetch の相手（リモートの名前と、片づける設定が効いていればその設定の名前）。
    Remotes(Arc<Repository>, Vec<(String, Option<String>)>),
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
            worktrees: Vec::new(),
            working_filter: String::new(),
            status_note: None,
            sidebar_filter: String::new(),
            form: Default::default(),
            working_layout: crate::workcopy::Layout::from_name(
                &crate::prefs::string(crate::prefs::WORKING_COPY_LAYOUT).unwrap_or_default(),
            ),
            layout_clicks: 0,
            working_collapsed: Default::default(),
            working_stats: Default::default(),
            draft_key: None,
            push_after_commit: false,
            recent_messages: Vec::new(),
            reflog: None,
            range_diff: None,
            range_request: 0,
            reflog_selected: None,
            file_history: None,
            blame: None,
            blame_from: Page::History,
            sequence: None,
            merge: None,
            settings: None,
            activity: None,
            repo_settings: None,
            repo_settings_from: Page::History,
            activity_from: Page::History,
            tag_remotes: Value::Null,
            worktree_counts: std::collections::HashMap::new(),
            stash_details: std::collections::HashMap::new(),
            sync_lists: Vec::new(),
            multi: Vec::new(),
            sync_warnings: Vec::new(),
            submodules: Value::Null,
            subtrees: Value::Null,
            repositories: None,
            repositories_from: Page::History,
            repositories_list: ElementRef::new(),
            settings_from: Page::History,
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
            working_list: ElementRef::new(),
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Refs(refs, repo) => {
                self.refs = Some(refs);
                self.repo = Some(repo.clone());
                self.switch_draft(&repo, context);
                // 最近切り替えたブランチを覚える（ブランチの切り替えの並び）。
                if let (Some(workdir), Some(head)) = (
                    repo.paths().workdir,
                    self.refs.as_ref().and_then(|r| r.refs.head_branch.clone()),
                ) {
                    crate::branchswitcher::remember(&workdir.display().to_string(), &head);
                }
                // Commit and Push: コミットした後の ref で push する。
                if std::mem::take(&mut self.push_after_commit) {
                    self.push_committed(context);
                }
                // worktree も読み直す（作る・消す・ブランチを替えると ref も変わる）。
                let r = repo.clone();
                // ブランチの分け方は ref が変わるたびに読み直す（数十 ms かかるので裏で）。
                load_groups(repo, context);
                // タグのリモートでの様子も（fetch でリモートのタグを読み直す）。
                load_tag_remotes(r.clone(), context);
                // stash の詳しい中身（作った時刻・untracked・土台と HEAD の離れ具合）。
                let stashes: Vec<(String, amonhen_model::Oid)> = self
                    .stash_entries()
                    .into_iter()
                    .filter_map(|e| Some((e.name.clone(), e.target?)))
                    .collect();
                if !stashes.is_empty() {
                    let s = r.clone();
                    context.spawn_background(move |_| {
                        let bases: Value =
                            serde_json::from_str(&s.run_json(r#"{"op":"stashBases"}"#))
                                .unwrap_or_default();
                        let details = stashes
                            .into_iter()
                            .map(|(name, id)| {
                                let info = s.commit_info(&id).unwrap_or_default();
                                let parents: Vec<&str> = info["parents"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_str)
                                    .collect();
                                let base = bases["info"]["stashes"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .find(|b| b["name"].as_str() == Some(&name));
                                let detail = crate::stashes::Detail {
                                    time: info["committerTime"].as_i64().unwrap_or_default(),
                                    untracked: parents.len() > 2,
                                    base: parents.first().map(|p| (*p).to_owned()),
                                    ahead_behind: base.and_then(|b| {
                                        Some((b["ahead"].as_u64()?, b["behind"].as_u64()?))
                                    }),
                                };
                                (name, detail)
                            })
                            .collect();
                        Message::StashDetails(details)
                    });
                }
                load_recent_messages(r.clone(), context);
                // リポジトリの形（unshallow・sparse checkout の切り替えも ref・status の変化のときに読み直す）。
                let f = r.clone();
                context.spawn_background(move |_| {
                    let reply: Value =
                        serde_json::from_str(&f.run_json(r#"{"op":"repositoryForm"}"#))
                            .unwrap_or_default();
                    Message::Form(sidebar::RepositoryForm::from_info(&reply["info"]))
                });
                load_worktrees(r, context);
                // Reflog を出していれば読み直す（HEAD が動くと ref も変わる）。
                if self.page == Page::Reflog {
                    self.load_reflog(context);
                }
            }
            Message::TagRemotes(info) => self.tag_remotes = info,
            Message::StashDetails(details) => self.stash_details = details,
            Message::MultiSelected(ids) => {
                self.multi = if ids.len() > 1 { ids } else { Vec::new() }
            }
            Message::SyncLists(lists, warnings) => {
                if matches!(self.dialog, Some(Dialog::Pull { .. } | Dialog::Push { .. })) {
                    self.sync_lists = lists;
                    self.sync_warnings = warnings;
                }
            }
            Message::Submodules(states) => self.submodules = states,
            Message::Subtrees(info) => self.subtrees = info,
            Message::FormField(index, text) => {
                if let Some(Dialog::Form { fields, .. }) = &mut self.dialog
                    && let Some(field) = fields.get_mut(index)
                {
                    field.1 = text;
                }
            }
            Message::FormCheck(index, on) => {
                if let Some(Dialog::Form { checks, .. }) = &mut self.dialog
                    && let Some(check) = checks.get_mut(index)
                {
                    check.1 = on;
                }
            }
            Message::Groups(groups) => {
                if groups.is_some() {
                    self.groups = groups;
                }
            }
            Message::Commit(info, extras) => self.info = Some((info, extras)),
            Message::Failed(e) => self.error = Some(e),
            Message::Command(id) if id == "undo" => self.undo(context),
            Message::Command(id) if id == "pull" => {
                self.pull();
                self.load_sync_lists(context);
            }
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
                | Some(Dialog::NewOrphan { name: current, .. })
                | Some(Dialog::Switcher { query: current, .. })
                | Some(Dialog::QuickOpen { query: current, .. })
                | Some(Dialog::Rename { name: current, .. })
                | Some(Dialog::NewGroup { name: current, .. })
                | Some(Dialog::RenameRepository { name: current, .. })
                | Some(Dialog::StashBranch { name: current, .. })
                | Some(Dialog::Publish { name: current, .. })
                | Some(Dialog::Stash {
                    message: current, ..
                })
                | Some(Dialog::Reword {
                    message: current, ..
                })
                | Some(Dialog::Patch {
                    version: current, ..
                })
                | Some(Dialog::Archive { top: current, .. })
                | Some(Dialog::Bundle { base: current, .. })
                | Some(Dialog::FetchBundle { name: current, .. })
                | Some(Dialog::LockWorktree {
                    reason: current, ..
                })
                | Some(Dialog::WorktreeFolder { to: current, .. }) => {
                    *current = name.replace("\r\n", "\n").replace('\r', "\n")
                }
                _ => {}
            },
            Message::Command(id) if id == "push" => {
                self.push(false);
                self.load_sync_lists(context);
            }
            Message::Command(id) if id == "force-push" => {
                self.push(true);
                self.load_sync_lists(context);
            }
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
                    // 片づけるかは git と同じく、remote.<name>.prune、なければ fetch.prune（GTK4 版と同じ）。
                    let config: Value =
                        serde_json::from_str(&repo.run_json(r#"{"op":"configList"}"#))
                            .unwrap_or_default();
                    let entries: Vec<(String, String)> = config["info"]["entries"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|e| {
                            Some((
                                e["key"].as_str()?.to_lowercase(),
                                e["value"].as_str()?.to_lowercase(),
                            ))
                        })
                        .collect();
                    let value = |key: &str| {
                        entries
                            .iter()
                            .rev()
                            .find(|(k, _)| *k == key.to_lowercase())
                            .map(|(_, v)| v.clone())
                    };
                    let remotes = reply["info"]["remotes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|r| r["name"].as_str().map(str::to_owned))
                        .map(|name| {
                            let own = format!("remote.{name}.prune");
                            let key = if value(&own).is_some() {
                                own
                            } else {
                                "fetch.prune".to_owned()
                            };
                            let prune = (value(&key).as_deref() == Some("true")).then_some(key);
                            (name, prune)
                        })
                        .collect();
                    Message::Remotes(repo, remotes)
                });
            }
            Message::Remotes(repo, remotes) => {
                if remotes.is_empty() {
                    beep();
                } else {
                    let (remotes, prune) = remotes.into_iter().unzip();
                    self.dialog = Some(Dialog::Fetch {
                        repo,
                        remotes,
                        from: 0,
                        prune,
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
            Message::Command(id) if id == "repository-settings" => {
                if self.page != Page::RepoSettings {
                    self.repo_settings_from = self.page;
                }
                self.repo_settings = Some(crate::reposettings::State::new());
                self.page = Page::RepoSettings;
                self.write_repo_settings(Vec::new(), context);
            }
            Message::Command(id) if id == "stash" => self.ask_stash(Vec::new()),
            Message::Command(id) if id == "am-three-way" => {
                if let Some(repo) = self.repo.clone() {
                    perform(
                        context,
                        repo,
                        json!({ "op": "amThreeWay" }),
                        tr!("Retry with 3-Way Merge"),
                    );
                }
            }
            Message::Command(id)
                if matches!(
                    id.as_str(),
                    "open-in-terminal" | "open-in-editor" | "reveal-in-files"
                ) =>
            {
                let Some(dir) = self.repo.as_ref().and_then(|r| r.paths().workdir) else {
                    return beep();
                };
                // エディタは起動を待つので裏で（code.cmd がすぐ返す）。
                if id == "open-in-editor" {
                    context.spawn_background(move |_| {
                        Message::Performed(
                            crate::editor::open_folder(&dir)
                                .err()
                                .map(|e| (tr!("Couldn’t Open in Editor"), e)),
                        )
                    });
                    return;
                }
                let opened = if id == "open-in-terminal" {
                    crate::files::open_terminal(&dir)
                } else {
                    crate::files::open_folder(&dir)
                };
                if !opened {
                    beep();
                }
            }
            Message::Command(id) if id == "delete-ignored-files" => {
                if let Some(repo) = self.repo.clone() {
                    context.spawn_background(move |_| {
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(r#"{"op":"ignoredFiles"}"#))
                                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                        Message::IgnoredLoaded(repo, reply)
                    });
                }
            }
            Message::Command(id) if id == "pop-stash" => {
                // いちばん新しい stash を Pop（確かめてから。GTK4 版の `pop_stash`）。
                match self.stash_entries().first().map(|e| e.name.clone()) {
                    Some(name) => self.stash_action("pop", name),
                    None => beep(),
                }
            }
            Message::Command(id) if id == "new-worktree" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                // 選んでいるコミットから（なければ今のブランチから。New Branch と同じ）。
                let start = self
                    .selected
                    .as_ref()
                    .map(|(id, _)| id.clone())
                    .or_else(|| self.refs.as_ref()?.refs.head_branch.clone())
                    .unwrap_or_else(|| "HEAD".to_owned());
                self.dialog = Some(Dialog::NewWorktree {
                    repo,
                    start,
                    branch: String::new(),
                    folder: String::new(),
                });
                self.dialog_inputs += 1;
            }
            Message::Command(id) if id == "switch-branch" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                let recent = repo
                    .paths()
                    .workdir
                    .map(|w| crate::branchswitcher::recent(&w.display().to_string()))
                    .unwrap_or_default();
                self.dialog = Some(Dialog::Switcher {
                    repo,
                    query: String::new(),
                    selected: 0,
                    recent,
                });
                self.dialog_inputs += 1;
            }
            Message::SwitcherSelect(index) => {
                if let (
                    Some(Dialog::Switcher { selected, .. } | Dialog::QuickOpen { selected, .. }),
                    Some(index),
                ) = (&mut self.dialog, index)
                {
                    *selected = index;
                }
            }
            Message::Command(id) if id == "open" => {
                // 開いているものとは別の窓で開く（窓にタブがない）。
                let Some(path) = crate::files::pick_folder(&tr!("Open…"), &tr!("Open")) else {
                    return;
                };
                if let Err(e) = crate::repositories::open_window(&path.display().to_string()) {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Open…"),
                        message: e,
                    });
                }
            }
            Message::Command(id) if id == "clone" => {
                self.dialog = Some(Dialog::Clone {
                    url: String::new(),
                    folder: crate::repositories::clone_folder(),
                    name: String::new(),
                    download: 0,
                    depth: 1,
                    sparse: false,
                    submodules: false,
                });
                self.dialog_inputs += 1;
            }
            Message::CloneField(index, text) => {
                if let Some(Dialog::Clone {
                    url, folder, name, ..
                }) = &mut self.dialog
                {
                    *[url, folder, name].into_iter().nth(index).expect("field") = text;
                }
            }
            Message::CloneDownload(Some(index)) => {
                if let Some(Dialog::Clone { download, .. }) = &mut self.dialog {
                    *download = index;
                }
            }
            Message::CloneDepth(Some(index)) => {
                if let Some(Dialog::Clone { depth, .. }) = &mut self.dialog {
                    *depth = index;
                }
            }
            Message::CloneDownload(None) | Message::CloneDepth(None) => {}
            Message::CloneCheck(index, on) => {
                if let Some(Dialog::Clone {
                    sparse, submodules, ..
                }) = &mut self.dialog
                {
                    *[sparse, submodules].into_iter().nth(index).expect("check") = on;
                }
            }
            Message::CloneChoose => {
                if let Some(path) =
                    crate::files::pick_folder(&tr!("Clone Location"), &tr!("Choose"))
                    && let Some(Dialog::Clone { folder, .. }) = &mut self.dialog
                {
                    *folder = path.display().to_string();
                }
            }
            Message::Cloned(dest, failed) => {
                self.busy = None;
                match failed {
                    Some(message) => {
                        self.dialog = Some(Dialog::Failed {
                            title: tr!("Couldn’t clone"),
                            message,
                        })
                    }
                    None => {
                        let dest = dest.display().to_string();
                        crate::repositories::add_bookmark(&dest);
                        if let Err(message) = crate::repositories::open_window(&dest) {
                            self.dialog = Some(Dialog::Failed {
                                title: tr!("Couldn’t clone"),
                                message,
                            });
                        }
                    }
                }
            }
            // Repository > Add Submodule…・Update Submodules…（サイドバーの Submodules の見出しと同じ）。
            Message::Command(id) if id == "add-submodule" => self.submodule_menu("sm-add", context),
            Message::Command(id) if id == "add-subtree" => self.submodule_menu("st-add", context),
            Message::Command(id) if id == "update-submodules" => {
                self.submodule_menu("sm-update:", context)
            }
            Message::Command(id) if id == "quick-open" => {
                self.dialog = Some(Dialog::QuickOpen {
                    query: String::new(),
                    selected: 0,
                    candidates: crate::repositories::candidates(Vec::new()),
                });
                self.dialog_inputs += 1;
                // スキャンするフォルダの下のリポジトリは、探し終えてから足す。
                let roots = crate::repositories::scan_roots();
                if !roots.is_empty() {
                    context.spawn_background(move |_| {
                        let found = crate::repositories::scan(roots);
                        Message::QuickOpenCandidates(crate::repositories::candidates(found))
                    });
                }
            }
            Message::QuickOpenCandidates(found) => {
                if let Some(Dialog::QuickOpen { candidates, .. }) = &mut self.dialog {
                    *candidates = found;
                }
            }
            Message::Command(id) if id == "new-orphan-branch" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                self.dialog = Some(Dialog::NewOrphan {
                    repo,
                    name: String::new(),
                    keep: false,
                });
                self.dialog_inputs += 1;
            }
            Message::OrphanKeep(on) => {
                if let Some(Dialog::NewOrphan { keep, .. }) = &mut self.dialog {
                    *keep = on;
                }
            }
            Message::Command(id) if id == "prune-worktrees" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                perform(
                    context,
                    repo,
                    json!({ "op": "worktreePrune" }),
                    tr!("Prune Worktrees"),
                );
            }
            Message::Command(id) if id == "save-bundle" => {
                self.ask_bundle(Vec::new(), String::new(), "all".to_owned())
            }
            Message::Command(id) if id == "fetch-bundle" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                let Some(file) = crate::files::open_files(&tr!(
                    "Choose a bundle file (git bundle) to fetch from."
                ))
                .into_iter()
                .next()
                .map(|p| p.to_string_lossy().into_owned()) else {
                    return;
                };
                context.spawn_background(move |_| {
                    let command = json!({ "op": "bundleCheck", "file": &file });
                    let check: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                    Message::BundleChecked(repo, file, check)
                });
            }
            Message::Command(id) if id == "apply-patch" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                let paths: Vec<String> = crate::files::open_files(&tr!(
                    "Choose patch files (.patch, .diff, .mbox, .eml)."
                ))
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
                if paths.is_empty() {
                    return;
                }
                context.spawn_background(move |_| {
                    let command = json!({ "op": "readPatch", "paths": &paths });
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                    Message::PatchRead(repo, paths, reply)
                });
            }
            Message::Command(id) if id == "repositories" => {
                if self.page != Page::Repositories {
                    self.repositories_from = self.page;
                }
                let (state, roots) = crate::repositories::State::new();
                self.repositories = Some(state);
                self.page = Page::Repositories;
                context.spawn_background(move |_| {
                    Message::Repos(crate::repositories::Event::Scanned(
                        crate::repositories::scan(roots),
                    ))
                });
            }
            Message::Command(id) if id == "activity" => {
                if self.page != Page::Activity {
                    self.activity_from = self.page;
                }
                self.activity = Some(crate::activity::State::new());
                self.page = Page::Activity;
                ACTIVITY_TICK.with(|t| t.set(Some(Instant::now())));
                self.load_undo_log(context);
            }
            Message::Command(id) if id == "settings" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                if self.page != Page::Settings {
                    self.settings_from = self.page;
                }
                let editor = crate::prefs::string(crate::editor::KEY).unwrap_or_default();
                self.settings = Some(crate::settingsview::State {
                    git: None,
                    auto_magic_merge: crate::prefs::bool(crate::prefs::AUTO_MAGIC_MERGE),
                    file_history_merges: crate::prefs::bool(crate::prefs::FILE_HISTORY_MERGES),
                    auto_fetch: crate::prefs::auto_fetch_minutes(),
                    write_commit_graph: crate::prefs::bool(crate::prefs::WRITE_COMMIT_GRAPH),
                    use_fsmonitor: crate::prefs::bool(crate::prefs::USE_FSMONITOR),
                    cache_size: None,
                    editors: crate::settingsview::editor_choices(&editor),
                    editor,
                    editor_epoch: 0,
                });
                context.spawn_background(|_| {
                    Message::CacheSize(
                        amonhen_remote::local::cache_dir()
                            .map_or(0, |d| amonhen_core::cache::total_size(&d)),
                    )
                });
                self.page = Page::Settings;
                // `--global` の段の値（いくつもあれば最後のもの。git と同じ）。
                context.spawn_background(move |_| {
                    let reply: Value =
                        serde_json::from_str(&repo.run_json(r#"{"op":"configList"}"#))
                            .unwrap_or_default();
                    let value = |key: &str| {
                        reply["info"]["entries"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .rev()
                            .find(|e| {
                                e["scope"].as_str() == Some("global")
                                    && e["key"]
                                        .as_str()
                                        .is_some_and(|k| k.eq_ignore_ascii_case(key))
                            })
                            .and_then(|e| e["value"].as_str())
                            .unwrap_or_default()
                            .to_owned()
                    };
                    Message::SettingsGit(value("user.name"), value("user.email"))
                });
            }
            Message::Command(id) if id == "compare-with-previous" => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                let branch = self.refs.as_ref().and_then(|r| r.refs.head_branch.clone());
                self.range_request += 1;
                let request = self.range_request;
                let mut state = crate::rangediff::State::new();
                state.summary = tr!("Looking for the previous version…");
                self.range_diff = Some(state);
                self.show_page(Page::RangeDiff);
                context.spawn_background(move |_| {
                    let command = json!({ "op": "previousVersion", "branch": &branch });
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_default();
                    let previous = &reply["info"]["previous"];
                    let found = if let Some(e) = reply["error"].as_str() {
                        Err(e.to_owned())
                    } else if previous.is_object() {
                        let ranges = previous["ranges"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|r| r.as_str().map(str::to_owned))
                            .collect();
                        let time = previous["time"].as_i64().unwrap_or_default();
                        Ok((
                            ranges,
                            tr!(
                                "{selector} → {branch} · {message} · {time}",
                                selector = previous["selector"].as_str().unwrap_or_default(),
                                branch = previous["branch"].as_str().unwrap_or_default(),
                                message = previous["message"].as_str().unwrap_or_default(),
                                time = crate::render::local_date(time),
                            ),
                        ))
                    } else {
                        Ok((
                            Vec::new(),
                            tr!(
                                "{branch} hasn’t been rewritten (nothing in its reflog)",
                                branch = branch.as_deref().unwrap_or("HEAD")
                            ),
                        ))
                    };
                    Message::RangePrevious(request, found)
                });
            }
            // Refresh: History と一緒に、worktree・ブランチの分け方も読み直す（フォルダを手で動かした worktree など、
            // ref の変わらない変化）。
            Message::Command(id) if id == "refresh" => {
                if let Some(repo) = self.repo.clone() {
                    load_groups(repo.clone(), context);
                    // タグのリモートでの様子も（タグを push・削除しても手元の ref は変わらない）。
                    load_tag_remotes(repo.clone(), context);
                    load_worktrees(repo, context);
                }
                with_history(|h| h.command(&id));
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
                (
                    Some(Dialog::Clone {
                        url,
                        folder,
                        name,
                        download,
                        depth,
                        sparse,
                        submodules,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let url = url.trim().to_owned();
                    let name = match name.trim() {
                        "" => crate::repositories::name_from_url(&url),
                        n => n.to_owned(),
                    };
                    let parent = crate::repositories::path_of(&folder);
                    if url.is_empty() || name.is_empty() || folder.trim().is_empty() {
                        return beep();
                    }
                    let dest = parent.join(&name);
                    let options = amonhen_backend_cli::CloneOptions {
                        filter: (download == 1).then(|| "blob:none".to_owned()),
                        depth: (download == 2).then(|| CLONE_DEPTHS[depth.min(3)]),
                        sparse,
                        recurse_submodules: submodules,
                        ..Default::default()
                    };
                    if dest.exists() {
                        // ダイアログを閉じた同じ更新では次のダイアログを出せないので、知らせで出す。
                        let title = tr!("{path} already exists", path = dest.display());
                        context.spawn_background(move |_| {
                            Message::Performed(Some((
                                title,
                                tr!("Choose a different name or location."),
                            )))
                        });
                        return;
                    }
                    crate::repositories::remember_clone_folder(folder.trim());
                    self.busy = Some(tr!("Cloning {url}…", url = url));
                    context.spawn_background(move |_| {
                        let result = amonhen_backend_cli::clone(&url, &dest, &options);
                        Message::Cloned(dest, result.err().map(|e| e.to_string()))
                    });
                }
                (
                    Some(Dialog::QuickOpen {
                        query,
                        selected,
                        candidates,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let shown = crate::repositories::matches(&candidates, &query);
                    // 打ち替えて候補が減っていれば、先頭。
                    match shown.get(selected).or(shown.first()) {
                        Some(c) => {
                            if let Err(e) = crate::repositories::open_window(&c.location) {
                                self.error = Some(e);
                            }
                        }
                        None => beep(),
                    }
                }
                (
                    Some(Dialog::Switcher {
                        repo,
                        query,
                        selected,
                        recent,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let Some(refs) = self.refs.as_ref() else {
                        return beep();
                    };
                    let shown = crate::branchswitcher::candidates(&refs.refs, &recent, &query);
                    // 打ち替えて候補が減っていれば、先頭。
                    match shown.get(selected).or(shown.first()) {
                        Some(choice) => {
                            perform(context, repo, choice.command(), tr!("Switch Branch"))
                        }
                        None => beep(),
                    }
                }
                (
                    Some(Dialog::Restore {
                        repo,
                        rev,
                        paths,
                        staged,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    perform(
                        context,
                        repo,
                        json!({ "op": "restoreFiles", "rev": rev, "paths": paths, "staged": staged }),
                        tr!("Restore"),
                    );
                }
                (Some(Dialog::NewOrphan { repo, name, keep }), ContentDialogResult::Primary) => {
                    let name = name.trim().to_owned();
                    if name.is_empty() {
                        return beep();
                    }
                    perform(
                        context,
                        repo,
                        json!({ "op": "createOrphanBranch", "name": name, "keepFiles": keep }),
                        tr!("Create"),
                    );
                }
                (Some(Dialog::DeleteIgnored { repo, paths }), ContentDialogResult::Primary) => {
                    perform(
                        context,
                        repo,
                        json!({ "op": "deleteIgnored", "paths": paths }),
                        tr!("Delete"),
                    );
                }
                (Some(Dialog::CleanUp { repo, branches, .. }), ContentDialogResult::Primary) => {
                    // まとめて消す（Undo は 1 回でまとめて戻す）。
                    let chosen: Vec<Value> = branches
                        .iter()
                        .filter(|(_, on)| *on)
                        .map(|(b, _)| json!({ "name": b["name"], "force": b["force"] }))
                        .collect();
                    if !chosen.is_empty() {
                        perform(
                            context,
                            repo,
                            json!({ "op": "deleteBranches", "branches": chosen }),
                            tr!("Clean Up Branches"),
                        );
                    }
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
                (
                    Some(Dialog::Stash {
                        repo,
                        paths,
                        message,
                        staged,
                        keep_index,
                        untracked,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let text = message.trim().to_owned();
                    perform(
                        context,
                        repo,
                        json!({
                            "op": "stashPush",
                            "message": (!text.is_empty()).then_some(text),
                            "includeUntracked": untracked,
                            "keepIndex": keep_index,
                            "staged": staged,
                            "paths": paths,
                        }),
                        "Stash".to_owned(),
                    );
                }
                (
                    Some(Dialog::Reword {
                        repo,
                        oid,
                        message,
                        squash,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let (kind, label) = if squash {
                        ("squash", tr!("Combine with Older"))
                    } else {
                        ("reword", tr!("Edit Message"))
                    };
                    perform(
                        context,
                        repo,
                        json!({
                            "op": "rewrite", "label": label,
                            "edit": { "kind": kind, "oid": oid, "message": message },
                        }),
                        label.clone(),
                    );
                }
                (
                    Some(Dialog::Patch {
                        repo,
                        oids,
                        single,
                        cover,
                        version,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let _ = crate::prefs::set_bool(PATCH_SINGLE_FILE, single);
                    let _ = crate::prefs::set_bool(PATCH_COVER_LETTER, cover);
                    let title = if oids.len() == 1 {
                        tr!("Save as Patch")
                    } else {
                        tr!("Save {count} Commits as Patches", count = oids.len())
                    };
                    let mut command =
                        json!({ "op": "formatPatch", "oids": oids, "coverLetter": cover });
                    if let Ok(v) = version.trim().parse::<u32>()
                        && v > 1
                    {
                        command["reroll"] = json!(v);
                    }
                    if single {
                        // ファイルの名前は最初のコミットの件名から（git format-patch と同じ考え）。
                        let subject = self
                            .info
                            .as_ref()
                            .filter(|(info, _)| {
                                info["oid"].as_str() == oids.first().map(String::as_str)
                            })
                            .and_then(|(info, _)| {
                                info["message"].as_str()?.lines().next().map(str::to_owned)
                            })
                            .unwrap_or_else(|| "patches".to_owned());
                        let Some(path) = crate::files::save_file(
                            &title,
                            &format!("{}.mbox", patch_file_name(&subject)),
                            "mbox",
                            "mbox",
                        ) else {
                            return;
                        };
                        command["file"] = json!(path.to_string_lossy());
                    } else {
                        let Some(path) = crate::files::pick_folder(
                            &tr!(
                                "Choose the folder for the patches. Files with the same names are replaced."
                            ),
                            &tr!("Save"),
                        ) else {
                            return;
                        };
                        command["dir"] = json!(path.to_string_lossy());
                    }
                    // 書いたら、最初のファイルをエクスプローラーで見せる（GTK4 版と同じ）。
                    context.spawn_background(move |_| {
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(&command.to_string()))
                                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                        if let Some(e) = reply["error"].as_str() {
                            return Message::Performed(Some((title, e.to_owned())));
                        }
                        if let Some(first) = reply["info"]["files"][0].as_str() {
                            crate::files::reveal(std::path::Path::new(first));
                        }
                        Message::Performed(None)
                    });
                }
                (
                    Some(Dialog::Archive {
                        repo,
                        rev,
                        format,
                        top,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let picked = ARCHIVE_FORMATS[format.min(ARCHIVE_FORMATS.len() - 1)];
                    let _ = crate::prefs::set_string(ARCHIVE_FORMAT, picked);
                    let prefix = top.trim().trim_matches('/').to_owned();
                    let stem = if prefix.is_empty() {
                        "archive"
                    } else {
                        prefix.as_str()
                    };
                    let Some(path) = crate::files::save_file(
                        &tr!("Export Archive"),
                        &format!("{stem}.{picked}"),
                        picked,
                        picked,
                    ) else {
                        return;
                    };
                    let command = json!({
                        "op": "archive",
                        "rev": rev,
                        "format": picked,
                        "prefix": if prefix.is_empty() { String::new() } else { format!("{prefix}/") },
                        "paths": [],
                        "file": path.to_string_lossy(),
                    });
                    // 書けたら、そのファイルをエクスプローラーで見せる（GTK4 版と同じ）。
                    context.spawn_background(move |_| {
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(&command.to_string()))
                                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                        if let Some(e) = reply["error"].as_str() {
                            return Message::Performed(Some((tr!("Export Archive"), e.to_owned())));
                        }
                        crate::files::reveal(&path);
                        Message::Performed(None)
                    });
                }
                (
                    Some(Dialog::ApplyPatch {
                        repo,
                        paths,
                        mailbox,
                        three_way,
                        index,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let command = if mailbox {
                        json!({ "op": "am", "paths": paths, "threeWay": three_way })
                    } else {
                        json!({
                            "op": "applyPatch", "paths": paths,
                            "index": index, "threeWay": three_way,
                        })
                    };
                    perform(context, repo, command, "Apply".to_owned());
                }
                (
                    Some(Dialog::Bundle {
                        repo,
                        refs,
                        label,
                        newer,
                        base,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let base = base.trim().to_owned();
                    let bases: Vec<String> = if newer && !base.is_empty() {
                        vec![base]
                    } else {
                        Vec::new()
                    };
                    let command = json!({ "op": "bundlePreview", "refs": refs, "bases": bases });
                    context.spawn_background(move |_| {
                        let preview: Value =
                            serde_json::from_str(&repo.run_json(&command.to_string()))
                                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                        Message::BundlePreview(repo, label, preview, command)
                    });
                }
                (
                    Some(Dialog::FetchBundle {
                        repo, file, name, ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let name = name.trim().to_owned();
                    if !name.is_empty() {
                        perform(
                            context,
                            repo,
                            json!({ "op": "fetchBundle", "file": file, "name": name }),
                            "Fetch".to_owned(),
                        );
                    }
                }
                (
                    Some(Dialog::NewWorktree {
                        repo,
                        start,
                        branch,
                        folder,
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let name = branch.trim().to_owned();
                    if name.is_empty() {
                        return beep();
                    }
                    let mut path = folder.trim().to_owned();
                    if path.is_empty() {
                        path = default_worktree_folder(&repo, &name);
                    }
                    perform(
                        context,
                        repo,
                        json!({ "op": "worktreeAdd", "path": path, "newBranch": name, "start": start }),
                        tr!("New Worktree"),
                    );
                }
                (
                    Some(Dialog::LockWorktree {
                        repo, path, reason, ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let text = reason.trim().to_owned();
                    perform(
                        context,
                        repo,
                        json!({ "op": "worktreeLock", "path": path, "reason": (!text.is_empty()).then_some(text) }),
                        tr!("Lock"),
                    );
                }
                (
                    Some(Dialog::WorktreeFolder {
                        repo,
                        path,
                        to,
                        repair,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => {
                    let to = to.trim().to_owned();
                    if to.is_empty() {
                        return beep();
                    }
                    let (command, title) = if repair {
                        (
                            json!({ "op": "worktreeRepair", "paths": [to] }),
                            tr!("Reconnect"),
                        )
                    } else {
                        (
                            json!({ "op": "worktreeMove", "path": path, "to": to }),
                            tr!("Move"),
                        )
                    };
                    perform(context, repo, command, title);
                }
                (Some(Dialog::CloseMerge), ContentDialogResult::Primary) => {
                    self.merge = None;
                    self.page = Page::WorkingCopy;
                }
                (Some(Dialog::MarkResolved { path, .. }), ContentDialogResult::Primary) => {
                    self.update(Message::Stage(path, true), context);
                }
                (
                    Some(Dialog::Form {
                        repo,
                        title,
                        fields,
                        checks,
                        kind,
                        ..
                    }),
                    ContentDialogResult::Primary,
                ) => match kind.command(&fields, &checks) {
                    Some(command) => perform(context, repo, command, title),
                    None => beep(),
                },
                (
                    Some(Dialog::RenameRepository { location, name }),
                    ContentDialogResult::Primary,
                ) => {
                    if let Some(state) = self.repositories.as_mut() {
                        state.rename(&location, &name);
                    }
                }
                (Some(Dialog::NewGroup { location, name }), ContentDialogResult::Primary) => {
                    if let Some(state) = self.repositories.as_mut() {
                        state.add_group(&name, location.as_deref());
                    }
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
                        ..
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
                // submodule の中の変更（✎）も status の変化で読み直す（submodule があるときだけ）。
                if let Some(repo) = self.repo.clone()
                    && self.submodules.as_array().is_some_and(|s| !s.is_empty())
                {
                    load_submodules(repo, context);
                }
                // 行数は裏で読む（数がなくても一覧は先に出す。GTK4 版の `load_stats`）。
                if let Some(repo) = self.repo.clone() {
                    let generation = status.generation;
                    context.spawn_background(move |_| {
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(r#"{"op":"workingStats"}"#))
                                .unwrap_or_default();
                        let stats = reply["info"]["stats"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|s| {
                                Some((
                                    s["path"].as_str()?.to_owned(),
                                    (s["added"].as_u64(), s["removed"].as_u64()),
                                ))
                            })
                            .collect();
                        Message::WorkingStats(generation, stats)
                    });
                }
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
            Message::Summary(text) => {
                if text != self.summary {
                    self.summary = text;
                    self.store_draft();
                }
            }
            Message::Description(text) => {
                if text != self.description {
                    self.description = text;
                    self.store_draft();
                }
            }
            Message::CommitTemplate(key, text) => {
                // 読む間にほかの worktree に替えた・打ち始めた・Amend にしたなら入れない。
                if self.draft_key.as_deref() == Some(key.as_str())
                    && self.summary.is_empty()
                    && self.description.is_empty()
                    && !self.amend
                {
                    let (s, b) = text.split_once('\n').unwrap_or((&text, ""));
                    self.summary = s.to_owned();
                    self.description = b.trim_matches('\n').to_owned();
                }
            }
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
                    let path = self.status.as_ref().and_then(|s| {
                        let visible =
                            crate::workcopy::visible(&s.status.entries, &self.working_filter);
                        match crate::workcopy::rows(
                            &visible,
                            self.working_layout,
                            &self.working_collapsed,
                        )
                        .get(index)
                        {
                            Some(crate::workcopy::Row::File { entry, .. }) => {
                                Some(Ok(entry.path.clone()))
                            }
                            Some(crate::workcopy::Row::Dir { path, .. }) => Some(Err(path.clone())),
                            None => None,
                        }
                    });
                    // ディレクトリの行を選んだら、畳む・開く（差分は前のファイルのまま）。
                    let path = match path {
                        Some(Err(dir)) => {
                            if !self.working_collapsed.remove(&dir) {
                                self.working_collapsed.insert(dir);
                            }
                            return;
                        }
                        Some(Ok(path)) => Some(path),
                        None => None,
                    };
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
                        if let Some(key) = &self.draft_key {
                            let _ = crate::prefs::remove(key);
                        }
                        // push は、コミットで動いた ref を読み直してから（`Message::Refs`）。
                    }
                    Some(message) => {
                        self.push_after_commit = false;
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
                } else if let Some(oids) = key.strip_prefix("cherry-pick:") {
                    self.pick(oids.split(',').map(str::to_owned).collect(), false);
                } else if let Some(oids) = key.strip_prefix("revert:") {
                    self.pick(oids.split(',').map(str::to_owned).collect(), true);
                } else if let Some(oid) = key.strip_prefix("reset:") {
                    self.reset(oid.to_owned());
                } else if let Some(name) = key.strip_prefix("delete-branch:") {
                    self.check_delete(context, name.to_owned());
                } else if let Some(name) = key.strip_prefix("favorite:") {
                    if let Some(repo) = self.repo.clone() {
                        let commands = self.favorite_commands(name);
                        perform_all(context, repo, commands, tr!("Add to Favorites"));
                    }
                } else if let Some(name) = key.strip_prefix("archive:") {
                    if let Some(repo) = self.repo.clone() {
                        let archived = self
                            .groups
                            .as_ref()
                            .and_then(|g| g.branches.get(name))
                            .is_some_and(|b| b.archived);
                        let value = if archived { json!(null) } else { json!("true") };
                        perform_all(
                            context,
                            repo,
                            vec![json!({
                                "op": "configSet",
                                "key": format!("branch.{name}.amonhenArchived"),
                                "value": value,
                            })],
                            tr!("Archive"),
                        );
                    }
                } else if let Some(name) = key.strip_prefix("push-branch:") {
                    let entry = self.refs.as_ref().and_then(|r| {
                        r.refs
                            .entries
                            .iter()
                            .find(|e| e.kind == amonhen_model::RefKind::Local && e.name == name)
                            .cloned()
                    });
                    match entry {
                        Some(entry) => {
                            self.push_branch(entry, false);
                            self.load_sync_lists(context);
                        }
                        None => beep(),
                    }
                } else if let Some(name) = key.strip_prefix("fast-forward:") {
                    // チェックアウトしないで上流まで進める（GTK4 版の `sheets::fast_forward`。前後の絵はまだない）。
                    let upstream =
                        self.refs
                            .as_ref()
                            .and_then(|r| {
                                r.refs.entries.iter().find(|e| {
                                    e.kind == amonhen_model::RefKind::Local && e.name == name
                                })
                            })
                            .and_then(|e| e.detail.clone())
                            .unwrap_or_default();
                    if let Some(repo) = self.repo.clone() {
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!(
                                "Fast-forward {branch} to {upstream}?",
                                branch = name,
                                upstream = &upstream
                            ),
                            message: tr!("You can Undo this (Ctrl+Alt+Z)."),
                            button: "Fast-Forward".to_owned(),
                            command: json!({ "op": "fastForward", "branch": name }),
                        });
                    }
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
                } else if let Some(rest) = key
                    .strip_prefix("push-tag:")
                    .or_else(|| key.strip_prefix("replace-tag:"))
                    .or_else(|| key.strip_prefix("delete-remote-tag:"))
                {
                    // GTK4 版の `confirm_push_tags`・`confirm_replace`・リモートから消す確認。どれも戻せない。
                    let mut parts = rest.split('|');
                    let (Some(remote), Some(tag), Some(repo)) =
                        (parts.next(), parts.next(), self.repo.clone())
                    else {
                        return;
                    };
                    let unchecked = parts.next() == Some("1");
                    let cant_undo = tr!("This can’t be undone.");
                    let (title, message, button, command) = if key.starts_with("push-tag:") {
                        (
                            tr!("Push {branch} to {target}?", branch = tag, target = remote),
                            format!(
                                "{tag}\n\n{} {cant_undo}",
                                tr!(
                                    "Everyone who fetches from {remote} gets them.",
                                    remote = remote
                                )
                            ),
                            "Push".to_owned(),
                            json!({ "op": "pushTags", "remote": remote, "names": [tag] }),
                        )
                    } else if key.starts_with("replace-tag:") {
                        (
                            tr!("Replace {tag} on {remote}?", tag = tag, remote = remote),
                            format!(
                                "{tag}\n{} {cant_undo}",
                                tr!(
                                    "{remote} has a different tag with this name; yours replaces it for everyone who fetches.",
                                    remote = remote
                                )
                            ),
                            tr!("Replace"),
                            json!({ "op": "pushTags", "remote": remote, "names": [tag], "force": true }),
                        )
                    } else {
                        let mut message = format!(
                            "{tag}\n\n{} {cant_undo}",
                            tr!(
                                "Everyone who fetches from {remote} loses them; yours stay.",
                                remote = remote
                            )
                        );
                        if unchecked {
                            message.push(' ');
                            message.push_str(&tr!(
                                "Whether {remote} has them hasn’t been checked yet (Fetch reads it).",
                                remote = remote
                            ));
                        }
                        (
                            tr!("Delete {tag} from {remote}?", tag = tag, remote = remote),
                            message,
                            tr!("Delete"),
                            json!({ "op": "pushTags", "remote": remote, "names": [tag], "delete": true }),
                        )
                    };
                    self.dialog = Some(Dialog::Confirm {
                        repo,
                        title,
                        message,
                        button,
                        command,
                    });
                } else if key.starts_with("sm-") || key.starts_with("st-") {
                    self.submodule_menu(&key, context);
                } else if key == "clean-up-branches" {
                    // 消してよいブランチの候補（GitHub のプルリクエストはまだ読まない）。
                    if let Some(repo) = self.repo.clone() {
                        context.spawn_background(move |_| {
                            let command = json!({ "op": "branchCleanup", "pullRequests": [] });
                            let reply: Value =
                                serde_json::from_str(&repo.run_json(&command.to_string()))
                                    .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                            Message::CleanUpLoaded(repo, reply)
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
                } else if let Some(oid) = key.strip_prefix("show:")
                    && let Some(id) = amonhen_model::Oid::from_hex(oid)
                {
                    // History に戻ってそのコミットを選ぶ（Reflog から）。
                    self.show_page(Page::History);
                    with_history(|h| h.select_commit(&id));
                } else if let Some(oid) = key.strip_prefix("reword:") {
                    // 出している詳細のメッセージから（右クリックは先にその行を選ぶ）。
                    let message = self
                        .info
                        .as_ref()
                        .filter(|(info, _)| info["oid"].as_str() == Some(oid))
                        .and_then(|(info, _)| info["message"].as_str().map(str::to_owned));
                    match (self.repo.clone(), message) {
                        (Some(repo), Some(message)) => {
                            self.dialog = Some(Dialog::Reword {
                                repo,
                                oid: oid.to_owned(),
                                message: message.trim_end().to_owned(),
                                squash: false,
                            });
                            self.dialog_inputs += 1;
                        }
                        _ => beep(),
                    }
                } else if let Some(oid) = key.strip_prefix("squash:") {
                    if let Some(repo) = self.repo.clone() {
                        let oid = oid.to_owned();
                        context.spawn_background(move |_| {
                            let command = json!({ "op": "squashMessage", "oid": &oid });
                            let reply: Value =
                                serde_json::from_str(&repo.run_json(&command.to_string()))
                                    .unwrap_or_default();
                            let message = match reply["error"].as_str() {
                                Some(e) => Err(e.to_owned()),
                                None => Ok(reply["text"].as_str().unwrap_or_default().to_owned()),
                            };
                            Message::SquashText(oid, message)
                        });
                    }
                } else if let Some(oid) = key.strip_prefix("patch:") {
                    if let Some(repo) = self.repo.clone() {
                        self.dialog = Some(Dialog::Patch {
                            repo,
                            oids: vec![oid.to_owned()],
                            single: crate::prefs::bool(PATCH_SINGLE_FILE),
                            cover: crate::prefs::bool(PATCH_COVER_LETTER),
                            version: String::new(),
                        });
                    }
                } else if let Some(path) = key.strip_prefix("wt-remove:") {
                    self.ask_remove_worktree(path.to_owned(), context);
                } else if let Some(path) = key.strip_prefix("wt-lock:") {
                    if let Some(repo) = self.repo.clone() {
                        let name = worktree_name(path);
                        self.dialog = Some(Dialog::LockWorktree {
                            repo,
                            path: path.to_owned(),
                            name,
                            reason: String::new(),
                        });
                        self.dialog_inputs += 1;
                    }
                } else if let Some(rest) = key
                    .strip_prefix("wt-move:")
                    .map(|p| (p, false))
                    .or_else(|| key.strip_prefix("wt-locate:").map(|p| (p, true)))
                {
                    let (path, repair) = rest;
                    if let Some(repo) = self.repo.clone() {
                        self.dialog = Some(Dialog::WorktreeFolder {
                            repo,
                            path: path.to_owned(),
                            name: worktree_name(path),
                            to: path.to_owned(),
                            repair,
                        });
                        self.dialog_inputs += 1;
                    }
                } else if let Some(path) = key.strip_prefix("wt-unlock:") {
                    if let Some(repo) = self.repo.clone() {
                        perform(
                            context,
                            repo,
                            json!({ "op": "worktreeUnlock", "path": path }),
                            tr!("Unlock"),
                        );
                    }
                } else if key == "prune-worktrees" {
                    self.update(Message::Command(key), context);
                } else if let Some(rest) = key.strip_prefix("bundle:") {
                    // `bundle:<完全な ref の名前>:<名前>`（サイドバーのブランチ・タグ）。
                    if let Some((full, name)) = rest.split_once(':') {
                        self.ask_bundle(vec![full.to_owned()], name.to_owned(), name.to_owned());
                    }
                } else if let Some(rest) = key.strip_prefix("archive:") {
                    // `archive:<rev>:<既定の名前>`（タグ・ブランチの名前、なければ短い ID）。
                    if let (Some(repo), Some((rev, label))) =
                        (self.repo.clone(), rest.split_once(':'))
                    {
                        let base = repo
                            .paths()
                            .workdir
                            .and_then(|w| w.file_name().map(|n| n.to_string_lossy().into_owned()))
                            .unwrap_or_default();
                        let top = archive_name(&if base.is_empty() {
                            label.to_owned()
                        } else {
                            format!("{base}-{label}")
                        });
                        let remembered = crate::prefs::string(ARCHIVE_FORMAT).unwrap_or_default();
                        self.dialog = Some(Dialog::Archive {
                            repo,
                            rev: rev.to_owned(),
                            format: ARCHIVE_FORMATS
                                .iter()
                                .position(|f| *f == remembered)
                                .unwrap_or(0),
                            top,
                        });
                    }
                } else if let Some(oid) = key.strip_prefix("drop:") {
                    if let Some(repo) = self.repo.clone() {
                        let short = &oid[..oid.len().min(8)];
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!("Drop {name}?", name = short),
                            message: tr!("You can Undo this (Ctrl+Alt+Z)."),
                            button: "Drop".to_owned(),
                            command: json!({
                                "op": "rewrite", "label": format!("Drop {short}"),
                                "edit": { "kind": "drop", "oid": oid },
                            }),
                        });
                    }
                } else if let Some(oid) = key.strip_prefix("split:") {
                    if let Some(repo) = self.repo.clone() {
                        // 止めてから Working Copy で分けてコミットする（GTK4 版の Split…）。
                        let short = &oid[..oid.len().min(8)];
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!("Split {commit}?", commit = short),
                            message: format!(
                                "{} {}",
                                tr!(
                                    "Its changes go back to the working tree; commit them in parts, then Continue."
                                ),
                                tr!("You can Undo this (Ctrl+Alt+Z).")
                            ),
                            button: "Split".to_owned(),
                            command: json!({
                                "op": "rewrite", "label": format!("Split {short}"),
                                "edit": { "kind": "split", "oid": oid },
                            }),
                        });
                    }
                } else if let Some(range) = key.strip_prefix("rangediff:") {
                    self.open_ranges(vec![range.to_owned()], String::new(), context);
                } else if let Some(rev) = key.strip_prefix("merge:") {
                    self.integrate(rev.to_owned(), false);
                } else if let Some(rev) = key.strip_prefix("rebase:") {
                    self.integrate(rev.to_owned(), true);
                }
            }
            Message::Performed(failed) => {
                self.busy = None;
                // Repository Settings を出していれば読み直す（リモートの編集などは設定を書く）。
                if self.repo_settings.is_some() {
                    self.write_repo_settings(Vec::new(), context);
                }
                // worktree も読み直す（`git worktree add` は作っている間ロックするので、ref が変わったときに読んだ
                // ものはロック中のことがある）。
                // ブランチの分け方も（お気に入り・アーカイブは git の設定だけが変わり、ref は変わらない）。
                if let Some(repo) = self.repo.clone() {
                    load_groups(repo.clone(), context);
                    // タグのリモートでの様子も（タグを push・削除しても手元の ref は変わらない）。
                    load_tag_remotes(repo.clone(), context);
                    load_worktrees(repo, context);
                }
                if let Some((title, message)) = failed {
                    self.dialog = Some(Dialog::Failed { title, message });
                }
            }
            Message::Invoked(key) if key == "page:working-copy" => {
                self.show_page(Page::WorkingCopy)
            }
            Message::Invoked(key) if key == "page:history" => self.show_page(Page::History),
            Message::Invoked(key) if key == "page:stashes" => self.show_page(Page::Stashes),
            // Conflicts: Working Copy で最初のコンフリクトのファイルを開く（GTK4 版と同じ）。
            Message::Invoked(key) if key == "page:conflicts" => {
                self.show_page(Page::WorkingCopy);
                let first = self.status.as_ref().and_then(|s| {
                    s.status
                        .entries
                        .iter()
                        .find(|e| e.kind == amonhen_model::StatusKind::Unmerged)
                        .map(|e| e.path.clone())
                });
                if let Some(path) = first {
                    self.working_file = Some(path);
                    self.load_working_diff();
                }
            }
            Message::Invoked(key) if key == "page:range-diff" => self.show_page(Page::RangeDiff),
            Message::RangePrevious(request, found) => {
                if request != self.range_request {
                    return;
                }
                match found {
                    Ok((ranges, summary)) if !ranges.is_empty() => {
                        self.open_ranges(ranges, summary, context)
                    }
                    Ok((_, summary)) | Err(summary) => {
                        if let Some(state) = self.range_diff.as_mut() {
                            state.error = Some(summary);
                        }
                    }
                }
            }
            Message::RangeDiffLoaded(request, pairs) => {
                if request != self.range_request {
                    return;
                }
                if let Some(state) = self.range_diff.as_mut() {
                    match pairs {
                        Ok(pairs) => {
                            // 最初の変わった組を選ぶ（なければ先頭）。
                            let first = pairs
                                .iter()
                                .position(|p| p.status == "changed")
                                .or((!pairs.is_empty()).then_some(0));
                            state.pairs = Some(pairs);
                            state.error = None;
                            self.update(Message::RangeDiffSelected(first), context);
                        }
                        Err(e) => state.error = Some(e),
                    }
                }
            }
            Message::RangeDiffRow(row) => {
                let index = self
                    .range_diff
                    .as_ref()
                    .and_then(|s| s.shown().get(row?).copied());
                if index.is_some() {
                    self.update(Message::RangeDiffSelected(index), context);
                }
            }
            Message::RangeDiffChangesOnly(on) => {
                let Some(state) = self.range_diff.as_mut() else {
                    return;
                };
                state.changes_only = on;
                // 選んでいた組が隠れたら、最初の変わった組を選ぶ（GTK4 版の `filter`）。
                let shown = state.shown();
                if state.selected.is_some_and(|s| !shown.contains(&s)) {
                    let first = shown.first().copied();
                    self.update(Message::RangeDiffSelected(first), context);
                }
            }
            Message::RangeDiffSelected(index) => {
                let Some(state) = self.range_diff.as_mut() else {
                    return;
                };
                state.selected = index;
                // 変わった組は差分の差分を重ねて出す。ほかはそのコミットの中身を右の列に。
                let oid = state
                    .selected_pair()
                    .filter(|p| p.status != "changed")
                    .and_then(|p| p.new.as_ref().or(p.old.as_ref()))
                    .and_then(|s| amonhen_model::Oid::from_hex(&s.oid));
                if let Some(id) = oid {
                    with_history(|h| h.session.data.want_commit_id(id));
                }
            }
            Message::RangeDiffRanges(text) => {
                if let Some(state) = self.range_diff.as_mut() {
                    state.ranges = text;
                }
            }
            Message::RangeDiffCompare => {
                let ranges: Vec<String> = self.range_diff.as_ref().map_or_else(Vec::new, |s| {
                    s.ranges.split_whitespace().map(str::to_owned).collect()
                });
                if !ranges.is_empty() {
                    self.open_ranges(ranges, String::new(), context);
                }
            }
            Message::CloseRangeDiff => {
                self.range_diff = None;
                self.show_page(Page::History);
            }
            Message::Invoked(key) if key == "page:reflog" => {
                self.show_page(Page::Reflog);
                self.load_reflog(context);
            }
            Message::ReflogLoaded(entries) => {
                // 選んでいた行は、同じ動きを選び直す（なければ先頭。GTK4 版の `show`）。
                let previous = self
                    .reflog_selected
                    .and_then(|i| self.reflog.as_ref()?.get(i).cloned());
                let row = previous
                    .and_then(|p| {
                        entries.iter().position(|e| {
                            e.new == p.new && e.time == p.time && e.message == p.message
                        })
                    })
                    .or((!entries.is_empty()).then_some(0));
                self.reflog = Some(entries);
                self.update(Message::ReflogSelected(row), context);
            }
            Message::ReflogSelected(index) => {
                self.reflog_selected = index;
                let id = index
                    .and_then(|i| self.reflog.as_ref()?.get(i))
                    .and_then(|e| amonhen_model::Oid::from_hex(&e.new));
                if let Some(id) = id
                    && self.page == Page::Reflog
                {
                    with_history(|h| h.session.data.want_commit_id(id));
                }
            }
            Message::Sequence(sequence) => self.sequence = sequence,
            Message::OpenMerge(path) => {
                let Some(repo) = self.repo.clone() else {
                    return beep();
                };
                self.merge = Some(crate::merge::State::new(path.clone()));
                self.page = Page::Merge;
                context.spawn_background(move |_| {
                    let command = json!({ "op": "mergeSides", "path": &path });
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                    let info = match reply["error"].as_str() {
                        Some(e) => Err(e.to_owned()),
                        None => Ok(reply["info"].clone()),
                    };
                    Message::MergeLoaded(path, info)
                });
            }
            Message::MergeLoaded(path, info) => {
                if let Some(state) = self.merge.as_mut()
                    && state.path == path
                {
                    state.load(info);
                }
            }
            Message::MergeResult(text) => {
                if let Some(state) = self.merge.as_mut() {
                    state.edit(&text);
                }
            }
            Message::MergeButton(key) => self.merge_button(key, context),
            Message::ActivityTab(Some(tab)) => {
                if let Some(state) = self.activity.as_mut() {
                    state.tab = tab;
                }
                self.load_undo_log(context);
                if tab == 2
                    && let Some(repo) = self.repo.clone()
                {
                    let name = repo
                        .paths()
                        .workdir
                        .and_then(|w| w.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .unwrap_or_default();
                    context.spawn_background(move |_| {
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(r#"{"op":"historyCache"}"#))
                                .unwrap_or_default();
                        Message::ActivityCache(name, reply["info"].clone())
                    });
                }
            }
            Message::ActivityCache(name, info) => {
                if let Some(state) = self.activity.as_mut() {
                    state.cache = Some((name, info));
                }
            }
            Message::ActivityExport => {
                // 題は付けない（GTK4 版・macOS 版と同じ）。
                let Some(path) =
                    crate::files::save_file("", "amonhen-activity.json", "JSON", "json")
                else {
                    return;
                };
                let records: Vec<Value> = amonhen_model::activity::entries()
                    .into_iter()
                    .map(|e| {
                        json!({
                            "seq": e.seq, "kind": e.kind, "repo": e.repo, "label": e.label,
                            "started": e.started, "duration": e.duration, "ok": e.ok, "error": e.error,
                        })
                    })
                    .collect();
                let text = serde_json::to_string_pretty(&records).unwrap_or_default();
                if let Err(e) = std::fs::write(&path, text) {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Export JSON…").trim_end_matches('…').to_owned(),
                        message: e.to_string(),
                    });
                }
            }
            Message::ActivityTab(None) => {}
            Message::SquashText(oid, message) => match (self.repo.clone(), message) {
                (Some(repo), Ok(message)) => {
                    self.dialog = Some(Dialog::Reword {
                        repo,
                        oid,
                        message,
                        squash: true,
                    });
                    self.dialog_inputs += 1;
                }
                (_, Err(message)) => {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Combine with Older"),
                        message,
                    })
                }
                _ => {}
            },
            Message::ArchiveFormat(Some(index)) => {
                if let Some(Dialog::Archive { format, .. }) = &mut self.dialog {
                    *format = index;
                }
            }
            Message::ArchiveFormat(None) => {}
            Message::BundleChecked(repo, file, check) => {
                if check["ok"] != true {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Can’t Fetch the Bundle"),
                        message: check["error"].as_str().unwrap_or_default().to_owned(),
                    });
                    return;
                }
                let info = &check["info"];
                let missing = info["missing"].as_u64().unwrap_or(0);
                if missing > 0 {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Can’t Fetch the Bundle"),
                        message: tr!(
                            "This repository lacks {count} commits the bundle needs (it has only the commits after them).",
                            count = missing
                        ),
                    });
                    return;
                }
                let branches: Vec<String> = info["refs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r.as_str()?.strip_prefix("refs/heads/").map(str::to_owned))
                    .collect();
                if branches.is_empty() {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Nothing to Fetch"),
                        message: tr!("The bundle has no branches."),
                    });
                    return;
                }
                let mut shown = branches
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                if branches.len() > 5 {
                    shown.push_str(", …");
                }
                let message = tr!(
                    "{file} has {count} branches ({names}). They are fetched under the name below, like a remote’s branches; tags aren’t fetched.",
                    file = std::path::Path::new(&file)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    count = branches.len(),
                    names = shown
                );
                self.dialog = Some(Dialog::FetchBundle {
                    repo,
                    file,
                    message,
                    name: info["name"].as_str().unwrap_or("bundle").to_owned(),
                });
                self.dialog_inputs += 1;
            }
            Message::Worktrees(list) => {
                self.worktrees = list;
                self.update(Message::CountWorktrees(true), context);
            }
            Message::CountWorktrees(fresh) => {
                // 開いている worktree は status で見ているので数えない。消えかけ（prunable）も。
                let Some(repo) = self.repo.clone() else {
                    return;
                };
                let open = repo.paths().workdir.map(|d| d.display().to_string());
                let paths: Vec<String> = self
                    .worktrees
                    .iter()
                    .filter(|w| !w.prunable)
                    .filter(|w| !open.as_deref().is_some_and(|o| same_path(o, &w.path)))
                    .map(|w| w.path.clone())
                    .collect();
                if self.worktrees.len() < 2 || paths.is_empty() {
                    return;
                }
                context.spawn_background(move |_| {
                    let command =
                        json!({ "op": "worktreeChanges", "paths": paths, "fresh": fresh });
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_default();
                    Message::WorktreeCounts(
                        reply["info"]["worktrees"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|w| {
                                Some((w["path"].as_str()?.to_owned(), w["count"].as_u64()?))
                            })
                            .collect(),
                    )
                });
            }
            Message::WorktreeCounts(counts) => {
                for (path, count) in counts {
                    if let Some(w) = self.worktrees.iter().find(|w| same_path(&w.path, &path)) {
                        self.worktree_counts.insert(w.path.clone(), count);
                    }
                }
            }
            Message::Confirm(repo, title, message, button, command) => {
                self.dialog = Some(Dialog::Confirm {
                    repo,
                    title,
                    message,
                    button,
                    command,
                })
            }
            Message::WorktreeField(index, text) => {
                if let Some(Dialog::NewWorktree { branch, folder, .. }) = &mut self.dialog {
                    *[branch, folder].into_iter().nth(index).expect("field") = text;
                }
            }
            Message::BundleNewer(on) => {
                if let Some(Dialog::Bundle { newer, .. }) = &mut self.dialog {
                    *newer = on;
                }
            }
            Message::BundlePreview(repo, label, preview, command) => {
                if preview["ok"] != true {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Can’t Save the Bundle"),
                        message: preview["error"].as_str().unwrap_or_default().to_owned(),
                    });
                    return;
                }
                let info = &preview["info"];
                let commits = info["commits"].as_u64().unwrap_or(0);
                if commits == 0 {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Nothing to Save"),
                        message: tr!("There are no commits to put in the bundle."),
                    });
                    return;
                }
                if info["beyondShallow"] == true {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Can’t Save the Bundle"),
                        message: tr!(
                            "This shallow clone doesn’t have all the history the bundle needs. Fetch more history, or choose a newer base."
                        ),
                    });
                    return;
                }
                // 入るコミットの数は、保存する場所を選ぶ窓の題に（GTK4 版と同じ）。
                let Some(path) = crate::files::save_file(
                    &tr!("{count} commits", count = commits),
                    &format!("{label}.bundle"),
                    "bundle",
                    "bundle",
                ) else {
                    return;
                };
                let mut command = command;
                command["op"] = json!("bundle");
                command["file"] = json!(path.to_string_lossy());
                context.spawn_background(move |_| {
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                    if let Some(e) = reply["error"].as_str() {
                        return Message::Performed(Some((tr!("Save as Bundle"), e.to_owned())));
                    }
                    crate::files::reveal(&path);
                    Message::Performed(None)
                });
            }
            Message::PatchRead(repo, paths, reply) => {
                let summary = &reply["info"];
                let patches = summary["patches"].as_array().cloned().unwrap_or_default();
                let empty = patches
                    .iter()
                    .all(|p| p["files"].as_array().is_none_or(|f| f.is_empty()));
                if reply["ok"].as_bool() != Some(true) || empty {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Can’t Read the Patch"),
                        message: reply["error"].as_str().map_or_else(
                            || tr!("There are no changes in the patch."),
                            str::to_owned,
                        ),
                    });
                    return;
                }
                let mailbox = summary["kind"].as_str() == Some("mailbox");
                let branch = self.head_name();
                let (title, what) = if mailbox {
                    let title = match &patches[..] {
                        [one] => tr!(
                            "Apply “{subject}”?",
                            subject = one["subject"].as_str().unwrap_or_default()
                        ),
                        _ => tr!("Apply {count} Patches?", count = patches.len()),
                    };
                    (
                        title,
                        tr!(
                            "Each patch becomes a commit on {branch} (git am).",
                            branch = branch
                        ),
                    )
                } else {
                    let names: Vec<String> = paths
                        .iter()
                        .map(|p| {
                            std::path::Path::new(p)
                                .file_name()
                                .map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned())
                        })
                        .collect();
                    let title = match &names[..] {
                        [one] => tr!("Apply {name}?", name = one),
                        _ => tr!("Apply {count} Patches?", count = names.len()),
                    };
                    (
                        title,
                        tr!("The changes go into the working tree (git apply)."),
                    )
                };
                let applies = if summary["applies"].as_bool() == Some(true) {
                    Ok(())
                } else {
                    Err(patch_error_line(
                        summary["error"].as_str().unwrap_or_default(),
                    ))
                };
                self.dialog = Some(Dialog::ApplyPatch {
                    repo,
                    paths,
                    mailbox,
                    title,
                    what,
                    list: patch_list(&patches),
                    applies,
                    three_way: mailbox && summary["threeWay"].as_bool() == Some(true),
                    index: false,
                });
            }
            Message::ApplyOption(option, on) => {
                if let Some(Dialog::ApplyPatch {
                    three_way, index, ..
                }) = &mut self.dialog
                {
                    match option {
                        "three-way" => *three_way = on,
                        _ => *index = on,
                    }
                }
            }
            Message::PatchOption(option, on) => {
                if let Some(Dialog::Patch { single, cover, .. }) = &mut self.dialog {
                    match option {
                        "single" => *single = on,
                        _ => *cover = on,
                    }
                }
            }
            Message::StashOption(option, on) => {
                if let Some(Dialog::Stash {
                    staged,
                    keep_index,
                    untracked,
                    ..
                }) = &mut self.dialog
                {
                    match option {
                        "all" if on => *staged = false,
                        "staged" if on => {
                            // stage した変更だけのときは、ほかの選び方は git が受けない。
                            *staged = true;
                            *keep_index = false;
                            *untracked = false;
                        }
                        "keep-index" => *keep_index = on,
                        "untracked" => *untracked = on,
                        _ => {}
                    }
                }
            }
            Message::RepoSettingsLoaded(entries) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    state.load(entries);
                    let mut locals: Vec<String> = self
                        .refs
                        .as_ref()
                        .map(|r| {
                            r.refs
                                .entries
                                .iter()
                                .filter(|e| e.kind == amonhen_model::RefKind::Local)
                                .map(|e| e.name.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    locals.sort();
                    state.local_branches = locals;
                }
                // フック・LFS・exclude も、書いた後に読み直す（同時に読むと、書く前のものが届く）。
                self.load_repo_files(context);
            }
            Message::RepoHooksLoaded(info) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    state.load_hooks(&info);
                }
            }
            Message::HookScript(name, text) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    state.hook_script = Some((name, text));
                }
            }
            Message::HookEnabled(name, on) if name == "maintenance" => {
                let now = self
                    .repo_settings
                    .as_ref()
                    .and_then(|s| s.form.as_ref())
                    .is_some_and(|f| f["maintenance"].as_bool() == Some(true));
                if now != on {
                    self.write_repo_settings(
                        vec![json!({ "op": "maintenance", "on": on })],
                        context,
                    );
                }
            }
            Message::RepoTemplateLoaded(text, path) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    let text = text.unwrap_or_default();
                    // 書きかけなら欄はそのまま。
                    let editing = state.template.as_deref().is_some_and(|t| {
                        crate::reposettings::lf(t) != state.template_saved
                            && crate::reposettings::lf(t) != text
                    });
                    state.template_saved = text.clone();
                    state.template_path = path;
                    if !editing {
                        state.template = Some(text);
                    }
                }
            }
            Message::RepoDetectedBase(base) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    state.detected_base = base;
                }
            }
            Message::RepoFormLoaded(info) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    state.form = Some(info);
                }
            }
            Message::HookEnabled(name, on) => {
                let current = self
                    .repo_settings
                    .as_ref()
                    .and_then(|s| s.hooks.as_ref())
                    .and_then(|h| h.iter().find(|(n, _)| *n == name))
                    .map(|(_, e)| *e);
                if current != Some(on) {
                    self.write_repo_settings(
                        vec![json!({ "op": "setHookEnabled", "name": name, "enabled": on })],
                        context,
                    );
                }
            }
            Message::RepoIgnoreLoaded(gitignore, exclude) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    // 書きかけ（読んだものと違う）なら、欄はそのまま。
                    let editing = state.exclude.as_deref().is_some_and(|t| {
                        crate::reposettings::lf(t) != state.exclude_saved
                            && crate::reposettings::lf(t) != exclude
                    });
                    state.gitignore = gitignore;
                    state.exclude_saved = exclude.clone();
                    if !editing {
                        state.exclude = Some(exclude);
                    }
                }
            }
            Message::RepoSettingsField(field, text) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    match field {
                        "name" => state.name = text,
                        "email" => state.email = text,
                        "query" => state.query = text,
                        "key" => state.new_key = text,
                        // 打ったまま持つ（揃えた値で欄を書き直すと、カーソルが先頭に戻る）。改行は書くときに揃える。
                        "exclude" => state.exclude = Some(text),
                        "lfs" => state.new_pattern = text,
                        "template" => state.template = Some(text),
                        _ => state.new_value = text,
                    }
                }
            }
            Message::RepoSettingsButton(key) => {
                let Some(state) = self.repo_settings.as_mut() else {
                    return;
                };
                // 空にした値は、この段から消す（GTK4 版と同じ）。
                let set = |key: &str, value: &str| {
                    let value = value.trim();
                    json!({
                        "op": "configSet", "key": key,
                        "value": (!value.is_empty()).then_some(value),
                    })
                };
                if let Some(name) = key.strip_prefix("hook-show:") {
                    let (Some(repo), name) = (self.repo.clone(), name.to_owned()) else {
                        return;
                    };
                    context.spawn_background(move |_| {
                        let command = json!({ "op": "hook", "name": &name });
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(&command.to_string()))
                                .unwrap_or_default();
                        let text = reply["info"]["text"]
                            .as_str()
                            .or(reply["text"].as_str())
                            .unwrap_or_default()
                            .to_owned();
                        Message::HookScript(name, text)
                    });
                    return;
                }
                // リモートの操作は、サイドバーのリモートの右クリックと同じ（`Message::HistoryMenu`）。
                if key.starts_with("remote-") {
                    self.update(Message::HistoryMenu(key), context);
                    return;
                }
                if key == "template-revert" {
                    state.template = Some(state.template_saved.clone());
                    return;
                }
                if key == "exclude-revert" {
                    state.exclude = Some(state.exclude_saved.clone());
                    return;
                }
                if key == "fetch-all-contents" {
                    // 大きなリポジトリでは長くかかるので、先に聞く（GTK4 版と同じ）。
                    let remote = state
                        .form
                        .as_ref()
                        .and_then(|f| f["partial"]["remote"].as_str())
                        .unwrap_or("origin")
                        .to_owned();
                    if let Some(repo) = self.repo.clone() {
                        self.dialog = Some(Dialog::Confirm {
                            repo,
                            title: tr!("Fetch All Contents?"),
                            message: tr!(
                                "Fetches the contents of every file in the history from {remote} (git fetch --refetch). In a large repository this can take long and use a lot of disk. Afterwards it’s no longer a partial clone.",
                                remote = remote
                            ),
                            button: tr!("Fetch All Contents"),
                            command: json!({ "op": "fetchAllContents" }),
                        });
                    }
                    return;
                }
                let commands = if key == "template-save" {
                    let text =
                        crate::reposettings::lf(state.template.as_deref().unwrap_or_default());
                    vec![json!({ "op": "saveCommitTemplate", "text": text })]
                } else if let Some(n) = key.strip_prefix("deepen:") {
                    vec![json!({ "op": "deepen", "commits": n.parse::<u64>().unwrap_or(100) })]
                } else if key == "unshallow" {
                    vec![json!({ "op": "unshallow" })]
                } else if key == "hook-create" {
                    let missing = state.missing_hooks();
                    let Some(name) =
                        missing.get(state.new_hook.min(missing.len().saturating_sub(1)))
                    else {
                        return;
                    };
                    vec![json!({ "op": "createHook", "name": name })]
                } else if let Some(pattern) = key.strip_prefix("lfs-untrack:") {
                    vec![json!({ "op": "lfsTrack", "pattern": pattern, "track": false })]
                } else if key == "lfs-track" {
                    let pattern = state.new_pattern.trim().to_owned();
                    state.new_pattern.clear();
                    vec![json!({ "op": "lfsTrack", "pattern": pattern, "track": true })]
                } else if key == "exclude-save" {
                    // TextBox は改行を `\r` で返すので、ファイルの改行（LF）に揃える。
                    let text =
                        crate::reposettings::lf(state.exclude.as_deref().unwrap_or_default());
                    vec![json!({
                        "op": "saveIgnoreFiles",
                        "gitignore": state.gitignore,
                        "exclude": text,
                    })]
                } else if key == "close" {
                    self.repo_settings = None;
                    self.page = self.repo_settings_from;
                    return;
                } else if key == "author" {
                    let mut commands = Vec::new();
                    for (k, now) in [("user.name", &state.name), ("user.email", &state.email)] {
                        if state.at("local", k).unwrap_or_default() != now.as_str() {
                            commands.push(set(k, now));
                        }
                    }
                    commands
                } else if key == "add" {
                    let command = set(state.new_key.trim(), &state.new_value);
                    state.new_key.clear();
                    state.new_value.clear();
                    vec![command]
                } else if let Some(k) = key.strip_prefix("remove:") {
                    vec![json!({ "op": "configSet", "key": k, "value": null })]
                } else {
                    return;
                };
                self.write_repo_settings(commands, context);
            }
            Message::ActivityUndoLog(entries) => {
                if let Some(state) = self.activity.as_mut() {
                    state.undo = entries;
                }
            }
            Message::ActivityTick => {
                if let Some(state) = self.activity.as_mut() {
                    state.refresh();
                    if state.tab == 1 {
                        self.load_undo_log(context);
                    }
                }
            }
            Message::ActivityUndo(count) => {
                let Some(repo) = self.repo.clone() else {
                    return;
                };
                let label = self
                    .activity
                    .as_ref()
                    .and_then(|s| s.undo.first())
                    .and_then(|e| e["label"].as_str())
                    .unwrap_or_default()
                    .to_owned();
                // 戻す前に確かめる（GTK4 版と同じ）。
                self.dialog = Some(Dialog::Confirm {
                    repo,
                    title: if count == 1 {
                        tr!("Undo “{label}”?", label = label)
                    } else {
                        tr!("Undo the last {count} operations?", count = count)
                    },
                    message: tr!("Returns HEAD and the branches to where they were."),
                    button: "Undo".to_owned(),
                    command: Value::Array(vec![json!({ "op": "undo" }); count]),
                });
            }
            Message::Repos(event) => {
                use crate::repositories::Action;
                let Some(state) = self.repositories.as_mut() else {
                    return;
                };
                match state.handle(event) {
                    Action::None => {}
                    Action::Close => {
                        self.repositories = None;
                        self.page = self.repositories_from;
                    }
                    Action::Scan(roots) => {
                        context.spawn_background(move |_| {
                            Message::Repos(crate::repositories::Event::Scanned(
                                crate::repositories::scan(roots),
                            ))
                        });
                    }
                    Action::Peek(locations) if locations.is_empty() => {}
                    Action::Peek(locations) => {
                        context.spawn_background(move |_| {
                            Message::Repos(crate::repositories::Event::Peeked(
                                crate::repositories::peek(locations),
                            ))
                        });
                    }
                    Action::Rename(location, name) => {
                        self.dialog = Some(Dialog::RenameRepository { location, name });
                        self.dialog_inputs += 1;
                    }
                    Action::Fetch(location) => {
                        context.spawn_background(move |_| {
                            let error = crate::repositories::fetch(&location);
                            Message::Repos(crate::repositories::Event::Fetched(location, error))
                        });
                    }
                    Action::NewGroup(location) => {
                        self.dialog = Some(Dialog::NewGroup {
                            location,
                            name: String::new(),
                        });
                        self.dialog_inputs += 1;
                    }
                }
            }
            Message::CloseActivity => {
                self.activity = None;
                ACTIVITY_TICK.with(|t| t.set(None));
                self.page = self.activity_from;
            }
            Message::SettingsGit(name, email) => {
                if let Some(state) = self.settings.as_mut() {
                    state.git = Some([(name.clone(), name), (email.clone(), email)]);
                }
            }
            Message::SettingsField(index, text) => {
                if let Some(field) = self
                    .settings
                    .as_mut()
                    .and_then(|s| s.git.as_mut())
                    .and_then(|g| g.get_mut(index))
                {
                    field.1 = text;
                }
            }
            Message::SettingsSaveGit => {
                let (Some(repo), Some(git)) = (
                    self.repo.clone(),
                    self.settings.as_mut().and_then(|s| s.git.as_mut()),
                ) else {
                    return;
                };
                // 変えた項目だけを書く（空にしたら消す）。
                let commands: Vec<Value> = ["user.name", "user.email"]
                    .iter()
                    .zip(git.iter_mut())
                    .filter(|(_, (read, now))| read != now)
                    .map(|(key, (read, now))| {
                        let value = now.trim().to_owned();
                        *read = now.clone();
                        json!({
                            "op": "configSet", "key": key, "scope": "global",
                            "value": (!value.is_empty()).then_some(value),
                        })
                    })
                    .collect();
                perform_all(context, repo, commands, tr!("Save"));
            }
            Message::SettingsAutoFetch(Some(index)) => {
                let minutes = crate::settingsview::AUTO_FETCH_CHOICES
                    .get(index)
                    .copied()
                    .unwrap_or(0);
                if self.settings.as_ref().map(|s| s.auto_fetch) == Some(minutes) {
                    return;
                }
                if let Err(e) =
                    crate::prefs::set_string(crate::prefs::AUTO_FETCH_MINUTES, &minutes.to_string())
                {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Save"),
                        message: e,
                    });
                }
                if let Some(state) = self.settings.as_mut() {
                    state.auto_fetch = minutes;
                }
                // 間隔を変えたら、そこから数え直す（GTK4 版・macOS 版と同じ）。
                AUTO_FETCH.with(|a| a.set((minutes, Instant::now())));
            }
            Message::SettingsAutoFetch(None) => {}
            Message::SettingsEditor(Some(index)) => {
                let Some(state) = self.settings.as_mut() else {
                    return;
                };
                let (items, selected) = state.editor_items();
                if index == selected {
                    return;
                }
                let value = if index == 0 {
                    Some(String::new())
                } else if index + 1 == items.len() {
                    // Other…: 実行ファイルを選ぶ（取り消したら元のまま）。
                    crate::files::open_files(&tr!("Choose the app to open files in."))
                        .into_iter()
                        .next()
                        .map(|p| p.display().to_string())
                } else {
                    state.editors.get(index - 1).map(|a| a.id.clone())
                };
                if let Some(value) = value {
                    let saved = if value.is_empty() {
                        crate::prefs::remove(crate::editor::KEY)
                    } else {
                        crate::prefs::set_string(crate::editor::KEY, &value)
                    };
                    if let Err(e) = saved {
                        self.dialog = Some(Dialog::Failed {
                            title: tr!("Save"),
                            message: e,
                        });
                    }
                    state.editors = crate::settingsview::editor_choices(&value);
                    state.editor = value;
                }
                state.editor_epoch += 1;
            }
            Message::SettingsEditor(None) => {}
            Message::CacheSize(bytes) => {
                if let Some(state) = self.settings.as_mut() {
                    state.cache_size = Some(bytes);
                }
            }
            Message::ClearCache => {
                // 開いているリポジトリの分も消える（次に開くとき git から読み直す。GTK4 版と同じ）。
                context.spawn_background(|_| {
                    if let Some(dir) = amonhen_remote::local::cache_dir()
                        && let Err(e) = amonhen_core::cache::remove_all(&dir)
                    {
                        return Message::Performed(Some((tr!("Clear"), e.to_string())));
                    }
                    Message::CacheSize(
                        amonhen_remote::local::cache_dir()
                            .map_or(0, |d| amonhen_core::cache::total_size(&d)),
                    )
                });
            }
            Message::AutoFetch => {
                // 聞かずにすべてのリモートから（GTK4 版と同じ）。ほかの操作の最中なら見送る。
                if let (Some(repo), None) = (self.repo.clone(), &self.busy) {
                    context.spawn_background(move |_| {
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(r#"{"op":"fetch"}"#))
                                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                        Message::AutoFetched(reply["error"].as_str().map(str::to_owned))
                    });
                }
            }
            Message::AutoFetched(error) => {
                // 失敗はステータスバーに（次に成功するまで。GTK4 版はステータスバーの失敗の知らせ）。
                self.status_note = error.map(|e| {
                    format!(
                        "{}: {}",
                        tr!("Couldn’t Fetch Automatically"),
                        e.lines().next().unwrap_or_default()
                    )
                });
            }
            Message::SettingsToggle(key, value) => {
                if key == crate::prefs::USE_FSMONITOR {
                    amonhen_backend_cli::set_fsmonitor(value);
                }
                if let Err(e) = crate::prefs::set_bool(key, value) {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Save"),
                        message: e,
                    });
                }
                if let Some(state) = self.settings.as_mut() {
                    match key {
                        crate::prefs::AUTO_MAGIC_MERGE => state.auto_magic_merge = value,
                        crate::prefs::WRITE_COMMIT_GRAPH => state.write_commit_graph = value,
                        crate::prefs::USE_FSMONITOR => state.use_fsmonitor = value,
                        _ => state.file_history_merges = value,
                    }
                }
            }
            Message::CloseSettings => {
                self.settings = None;
                self.page = self.settings_from;
            }
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
            Message::WorkingFileMenu(_, key) if key.starts_with("ignore:") => {
                if let Some(repo) = self.repo.clone() {
                    add_to_gitignore(context, repo, key["ignore:".len()..].to_owned());
                }
            }
            Message::WorkingFilter(text) => self.working_filter = text,
            Message::RepoSettingsChoice("template", Some(index)) => {
                let Some(state) = self.repo_settings.as_ref() else {
                    return;
                };
                if index == crate::reposettings::template_choice(state) && index != 2 {
                    return;
                }
                let key = crate::reposettings::TEMPLATE_KEY;
                let commands = match index {
                    0 => vec![json!({ "op": "configSet", "key": key, "value": null })],
                    // 専用のファイルがまだなければ空で作る（commit.template もそれを向く）。あれば向け直すだけ。
                    1 if state.template_path.is_empty() => {
                        vec![json!({ "op": "saveCommitTemplate", "text": "" })]
                    }
                    1 => {
                        vec![json!({ "op": "configSet", "key": key, "value": state.template_path })]
                    }
                    _ => {
                        // 作り直した欄が今の値を知らせてきたときは、選び直さない。
                        if crate::reposettings::template_choice(state) == 2 {
                            return;
                        }
                        let Some(path) =
                            crate::files::open_files(&tr!("Choose a commit message template file"))
                                .into_iter()
                                .next()
                        else {
                            return;
                        };
                        vec![json!({
                            "op": "configSet",
                            "key": key,
                            "value": path.display().to_string().replace('\\', "/"),
                        })]
                    }
                };
                self.write_repo_settings(commands, context);
            }
            Message::RepoSettingsChoice("default-branch", Some(index)) => {
                let Some(state) = self.repo_settings.as_ref() else {
                    return;
                };
                let values = crate::reposettings::default_branch_values(state);
                let Some(value) = values.get(index) else {
                    return;
                };
                let now = state
                    .at("local", crate::reposettings::DEFAULT_BRANCH)
                    .unwrap_or_default();
                if value != now {
                    self.write_repo_settings(
                        vec![json!({
                            "op": "configSet",
                            "key": crate::reposettings::DEFAULT_BRANCH,
                            "value": (!value.is_empty()).then_some(value),
                        })],
                        context,
                    );
                }
            }
            Message::RepoSettingsChoice("hook-new", Some(index)) => {
                if let Some(state) = self.repo_settings.as_mut() {
                    state.new_hook = index;
                }
            }
            Message::RepoSettingsChoice(id, Some(index)) => {
                let Some(state) = self.repo_settings.as_ref() else {
                    return;
                };
                let commands = if id == "pull" {
                    let method = ["merge", "rebase", "ffOnly"][index.min(2)];
                    // 作り直した欄が今の値を知らせてきたときは書かない。
                    if method == crate::reposettings::pull_method(state) {
                        return;
                    }
                    crate::reposettings::pull_commands(state, method)
                } else {
                    let Some((_, _, _, key, options, default)) =
                        crate::reposettings::operation_rows()
                            .into_iter()
                            .chain(crate::reposettings::working_copy_rows())
                            .find(|r| r.0 == id)
                    else {
                        return;
                    };
                    let Some((value, _)) = options.get(index) else {
                        return;
                    };
                    let now = state
                        .effective(key)
                        .map_or(default, |(v, _)| v)
                        .to_lowercase();
                    let same = now == *value
                        || (*value == "true" && ["yes", "on", "1"].contains(&now.as_str()))
                        || (*value == "false" && ["no", "off", "0", ""].contains(&now.as_str()));
                    if same {
                        return;
                    }
                    vec![json!({ "op": "configSet", "key": key, "value": value })]
                };
                self.write_repo_settings(commands, context);
            }
            Message::RepoSettingsChoice(_, None) => {}
            Message::SidebarFilter(text) => self.sidebar_filter = text,
            Message::Form(form) => self.form = form,
            Message::IgnoredLoaded(repo, reply) => {
                let paths: Vec<String> = reply["info"]["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str().map(str::to_owned))
                    .collect();
                self.dialog = Some(if let Some(error) = reply["error"].as_str() {
                    Dialog::Failed {
                        title: tr!("Delete Ignored Files…")
                            .trim_end_matches('…')
                            .to_owned(),
                        message: error.to_owned(),
                    }
                } else if paths.is_empty() {
                    Dialog::Failed {
                        title: tr!("No Ignored Files"),
                        message: tr!("Nothing in the working tree matches .gitignore."),
                    }
                } else {
                    Dialog::DeleteIgnored { repo, paths }
                });
            }
            Message::CleanUpLoaded(repo, reply) => {
                if let Some(error) = reply["error"].as_str() {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Clean Up Branches"),
                        message: error.to_owned(),
                    });
                    return;
                }
                let info = &reply["info"];
                let base = info["base"].as_str().unwrap_or("main").to_owned();
                let branches: Vec<(Value, bool)> = info["branches"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|b| (b.clone(), b["checked"] == true))
                    .collect();
                self.dialog = Some(if branches.is_empty() {
                    Dialog::Failed {
                        title: tr!("No Branches to Clean Up"),
                        message: tr!(
                            "No branch is merged into {base}, merged by a pull request, or left without its upstream.",
                            base = base
                        ),
                    }
                } else {
                    Dialog::CleanUp {
                        repo,
                        base,
                        branches,
                    }
                });
            }
            Message::CleanUpCheck(index, on) => {
                if let Some(Dialog::CleanUp { branches, .. }) = &mut self.dialog
                    && let Some(b) = branches.get_mut(index)
                {
                    b.1 = on;
                }
            }
            Message::WorkingLayout(layout) => {
                self.layout_clicks += 1;
                if layout != self.working_layout {
                    self.working_layout = layout;
                    let _ =
                        crate::prefs::set_string(crate::prefs::WORKING_COPY_LAYOUT, layout.name());
                }
            }
            Message::StageAll(paths, include) => {
                let Some(repo) = self.repo.clone() else {
                    return;
                };
                context.spawn_background(move |_| {
                    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                    let result = if include {
                        repo.stage(&paths)
                    } else {
                        repo.unstage(&paths)
                    };
                    with_status_reload();
                    Message::Performed(result.err().map(|e| (tr!("Couldn’t Stage"), e.to_string())))
                });
            }
            Message::WorkingStats(generation, stats) => {
                // 読む間に status が替わっていれば捨てる（替わった分が別に届く）。
                if self.status.as_ref().map(|s| s.generation) == Some(generation) {
                    self.working_stats = stats;
                }
            }
            Message::WorkingKey(discard) => {
                // 選んでいるファイル（GTK4 版・macOS 版の一覧のキーと同じ）。
                let entry = self.working_file.as_ref().and_then(|path| {
                    self.status
                        .as_ref()?
                        .status
                        .entries
                        .iter()
                        .find(|e| &e.path == path)
                        .cloned()
                });
                let Some(entry) = entry else {
                    return beep();
                };
                if !discard {
                    let include = !crate::workcopy::included(&entry);
                    self.update(Message::Stage(entry.path, include), context);
                } else if entry.kind != amonhen_model::StatusKind::Unmerged && entry.is_unstaged() {
                    self.update(Message::DiscardFile(entry.path), context);
                } else {
                    beep();
                }
            }
            Message::RecentMessages(recent) => self.recent_messages = recent,
            Message::CommitTextMenu(key) => self.message_menu(&key, context),
            Message::WorkingFileMenu(path, key) => match key.as_str() {
                "history" => self.update(Message::OpenFileHistory(path), context),
                "blame" => self.update(Message::OpenBlame(path, None), context),
                "merge-editor" => self.update(Message::OpenMerge(path), context),
                "stash" => self.ask_stash(vec![path]),
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
                let merges = crate::prefs::bool(crate::prefs::FILE_HISTORY_MERGES);
                context.spawn_background(move |_| {
                    let command = json!({ "op": "fileHistory", "path": &path, "merges": merges });
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
            Message::FileHistoryMenu(key) if key.starts_with("restore") => {
                // ファイルをその版（その前の版）に戻す（GTK4 版の `restoresheet::ask`）。
                let before = key.starts_with("restore-before:");
                let oid = key.split_once(':').map_or("", |(_, o)| o).to_owned();
                let (Some(repo), Some(state)) = (self.repo.clone(), self.file_history.as_ref())
                else {
                    return beep();
                };
                let at = state
                    .entries
                    .as_ref()
                    .and_then(|e| e.as_ref().ok())
                    .and_then(|e| e.iter().find(|e| e.oid == oid))
                    .map_or_else(|| state.path.clone(), |e| e.path.clone());
                // 前の版に戻すときは、そのコミットでの名前と今の名前の両方（名前を変えていれば今の名前は消える）。
                let mut paths = vec![at];
                if before && !paths.contains(&state.path) {
                    paths.push(state.path.clone());
                }
                let short = &oid[..oid.len().min(8)];
                let title = if before {
                    tr!(
                        "Restore {path} as It Was Before {commit}?",
                        path = &state.path,
                        commit = short
                    )
                } else {
                    tr!(
                        "Restore {path} as It Is in {commit}?",
                        path = &state.path,
                        commit = short
                    )
                };
                let rev = if before {
                    format!("{oid}^")
                } else {
                    oid.clone()
                };
                self.dialog = Some(Dialog::Restore {
                    repo,
                    title,
                    rev,
                    paths,
                    staged: false,
                });
            }
            Message::RestoreStaged(on) => {
                if let Some(Dialog::Restore { staged, .. }) = &mut self.dialog {
                    *staged = on;
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
                if matches!(
                    self.page,
                    Page::Stashes
                        | Page::FileHistory
                        | Page::Blame
                        | Page::Reflog
                        | Page::RangeDiff
                ) {
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
        // 未コミットの変更（●）と上流との差（↑↓）を添える（GTK4 版・macOS 版はタブの名前に。ここはタブがないので窓の
        // 名前に）。
        let mut title = name;
        if self
            .status
            .as_ref()
            .is_some_and(|s| !s.status.entries.is_empty())
        {
            title.push_str(" ●");
        }
        if let Some(head) = self.head_entry() {
            if head.ahead > 0 {
                title.push_str(&format!(" ↑{}", head.ahead));
            }
            if head.behind > 0 {
                title.push_str(&format!(" ↓{}", head.behind));
            }
        }
        context.window_title(title);
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
        // 絞り込みの欄（Branches・Remotes・Tags。GTK4 版の Filter branches）。
        let conflicts = self.status.as_ref().map_or(0, |s| {
            s.status
                .entries
                .iter()
                .filter(|e| e.kind == amonhen_model::StatusKind::Unmerged)
                .count()
        });
        let sidebar = Grid::new()
            .element_ref(&self.sidebar_tree)
            .rows([GridLength::Auto, GridLength::Star(1.0)])
            .grid_column(0)
            .children((
                TextBox::new()
                    .text(self.sidebar_filter.clone())
                    .placeholder_text(tr!("Filter branches"))
                    .margin(Thickness::new(8.0, 6.0, 8.0, 4.0))
                    .grid_row(0)
                    .on_text_changed(context.callback(Message::SidebarFilter)),
                TreeView::new()
                    .grid_row(1)
                    .on_item_invoked(context.callback(Message::Invoked))
                    .nodes(sidebar::nodes(
                        self.refs.as_ref().map(|r| &r.refs),
                        self.groups.as_ref(),
                        self.range_diff.is_some(),
                        &self
                            .worktrees
                            .iter()
                            .map(|w| {
                                let editing = self
                                    .repo
                                    .as_ref()
                                    .and_then(|r| r.paths().workdir)
                                    .is_some_and(|d| same_path(&d.display().to_string(), &w.path));
                                let changes =
                                    self.worktree_counts.get(&w.path).copied().unwrap_or(0);
                                (format!("worktree:{}", w.path), w.title(editing, changes))
                            })
                            .collect::<Vec<_>>(),
                        sidebar::Marks {
                            form: &self.form,
                            conflicts,
                            tag_remotes: &self.tag_remotes,
                            submodules: &self.submodules,
                            subtrees: &self.subtrees["subtrees"],
                        },
                        &self.sidebar_filter,
                    )),
                Grid::new()
                    .element_ref(&self.sidebar_menu_host)
                    .grid_row(1)
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
                self.reflog_view(context),
                self.range_diff_view(context),
                self.file_history_view(context),
                self.blame_view(context),
                self.merge_view(context),
                self.settings_view(context),
                self.activity_view(context),
                self.repositories_view(context),
                self.repo_settings_view(context),
            ));
        // Working Copy の一覧のキー: 画面を出すたびに Grid が作り直されるので、そのたびに付ける。
        let host = self.working_list.clone();
        context.use_effect("working-keys", self.page == Page::WorkingCopy, move || {
            let _ = host.request_native_element(|result| {
                if let Ok(element) = result
                    && let Err(e) = attach_working_keys(&element)
                {
                    eprintln!("Working Copy の一覧にキーを付けられない: {e}");
                }
            });
            None
        });
        // Repositories の一覧のダブルクリック・Enter で開く（画面を出すたびに Grid が作り直される）。
        let host = self.repositories_list.clone();
        context.use_effect(
            "repositories-open",
            self.page == Page::Repositories,
            move || {
                let _ = host.request_native_element(|result| {
                    if let Ok(element) = result
                        && let Err(e) = attach_repositories_open(&element)
                    {
                        eprintln!("Repositories の一覧に開く操作を付けられない: {e}");
                    }
                });
                None
            },
        );
        // ダイアログの入力の欄: フォーカスを入れ、Enter で既定のボタン（ContentDialog は Enter を既定のボタンに
        // 回さない）。
        if self.dialog_inputs > 0 {
            let input = self.dialog_input.clone();
            context.use_effect("dialog-input", self.dialog_inputs, move || {
                let _ = input.request_focus();
                let _ = input.request_native_element(|result| {
                    if let Ok(element) = result {
                        if let Err(e) = attach_enter(&element) {
                            eprintln!("入力の欄にキーを付けられない: {e}");
                        }
                        // 入れてある文字（Rename の今の名前など）は選んでおき、打てば置き換わるように。
                        let _ = xaml::select_all(&element);
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
            Some(Dialog::Stash {
                paths,
                message,
                staged,
                keep_index,
                untracked,
                ..
            }) => {
                let title = match &paths[..] {
                    [] => tr!("Stash Changes"),
                    [one] => tr!(
                        "Stash Changes in {name}",
                        name = one.rsplit('/').next().unwrap_or(one)
                    ),
                    many => tr!("Stash Changes in {count} Files", count = many.len()),
                };
                let check = |key: &'static str, label: String, on: bool, enabled: bool| {
                    CheckBox::new()
                        .is_checked(on)
                        .is_enabled(enabled)
                        .on_is_checked_changed(context.callback(move |v| Message::StashOption(key, v)))
                        .content(TextBlock::new().text(label))
                };
                dialog
                    .is_open(true)
                    .title(title)
                    .primary_button_text("Stash")
                    .close_button_text(tr!("Cancel"))
                    .content(
                        StackPanel::new().spacing(6.0).children((
                            TextBlock::new().text(tr!("Message (optional)")),
                            TextBox::new()
                                .element_ref(&self.dialog_input)
                                .text(message.clone())
                                .on_text_changed(context.callback(Message::BranchName)),
                            // すべてか stage した変更だけか（GTK4 版はラジオボタン）。
                            RadioButtons::new()
                                .items_source(vec![
                                    tr!("All changes"),
                                    tr!("Only the staged changes"),
                                ])
                                .selected_index(usize::from(*staged))
                                .on_selection_changed(context.callback(|i: Option<usize>| {
                                    Message::StashOption(
                                        if i == Some(1) { "staged" } else { "all" },
                                        true,
                                    )
                                })),
                            check(
                                "keep-index",
                                tr!("Keep the staged changes in the working tree (--keep-index)"),
                                *keep_index,
                                !*staged,
                            ),
                            check(
                                "untracked",
                                tr!("Include untracked files"),
                                *untracked,
                                !*staged,
                            ),
                        )),
                    )
            }
            Some(Dialog::Reword {
                oid,
                message,
                squash,
                ..
            }) => dialog
                .is_open(true)
                .title(if *squash {
                    tr!("Combine with Older Commit…").trim_end_matches('…').to_owned()
                } else {
                    tr!("Edit Message")
                })
                .primary_button_text(if *squash { "Combine" } else { "Save" })
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!message.trim().is_empty())
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(format!(
                                "{} · {}",
                                &oid[..oid.len().min(8)],
                                tr!("You can Undo this (Ctrl+Alt+Z).")
                            ))
                            .opacity(0.7)
                            .text_wrapping(TextWrapping::Wrap),
                        TextBox::new()
                            .accepts_return(true)
                            .text_wrapping(TextWrapping::Wrap)
                            .height(200.0)
                            .text(message.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::Patch {
                oids,
                single,
                cover,
                version,
                ..
            }) => dialog
                .is_open(true)
                .title(if oids.len() == 1 {
                    tr!("Save as Patch")
                } else {
                    tr!("Save {count} Commits as Patches", count = oids.len())
                })
                .primary_button_text(tr!("Save…"))
                .close_button_text(tr!("Cancel"))
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(tr!("Numbering, recipients and sign-offs follow git’s format.* settings."))
                            .text_wrapping(TextWrapping::Wrap),
                        RadioButtons::new()
                            .items_source(vec![tr!("One file per commit"), tr!("One mbox file")])
                            .selected_index(usize::from(*single))
                            .on_selection_changed(context.callback(|i: Option<usize>| {
                                Message::PatchOption("single", i == Some(1))
                            })),
                        CheckBox::new()
                            .is_checked(*cover)
                            .on_is_checked_changed(
                                context.callback(|v| Message::PatchOption("cover", v)),
                            )
                            .content(TextBlock::new().text(tr!("Cover letter"))),
                        TextBlock::new().text(tr!("Version (-v):")),
                        TextBox::new()
                            .text(version.clone())
                            .placeholder_text("1")
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::Archive { format, top, .. }) => dialog
                .is_open(true)
                .title(tr!("Export Archive"))
                .primary_button_text(tr!("Export…"))
                .close_button_text(tr!("Cancel"))
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(tr!(
                                "The files of this commit, as git archive writes them; .gitattributes decides what is left out (export-ignore)."
                            ))
                            .text_wrapping(TextWrapping::Wrap),
                        TextBlock::new().text(tr!("Format:")),
                        ComboBox::new()
                            .items_source(ARCHIVE_FORMATS.to_vec())
                            .selected_index(*format)
                            .on_selection_changed(context.callback(Message::ArchiveFormat)),
                        TextBlock::new().text(tr!("Top folder:")),
                        TextBox::new()
                            .text(top.clone())
                            .placeholder_text(tr!("None"))
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::ApplyPatch {
                mailbox,
                title,
                what,
                list,
                applies,
                three_way,
                index,
                ..
            }) => {
                let status = match applies {
                    Ok(()) => tr!("Applies cleanly."),
                    Err(reason) => tr!("Doesn’t apply cleanly: {reason}", reason = reason),
                };
                let check = |key: &'static str, label: String, on: bool| -> View {
                    CheckBox::new()
                        .is_checked(on)
                        .on_is_checked_changed(context.callback(move |v| Message::ApplyOption(key, v)))
                        .content(TextBlock::new().text(label))
                };
                dialog
                    .is_open(true)
                    .title(title.clone())
                    .primary_button_text(tr!("Apply"))
                    .close_button_text(tr!("Cancel"))
                    .content(
                        StackPanel::new().spacing(6.0).children((
                            TextBlock::new()
                                .text(format!("{what} {}", tr!("You can Undo this (Ctrl+Alt+Z).")))
                                .text_wrapping(TextWrapping::Wrap),
                            ScrollViewer::new().max_height(180.0).content(
                                TextBlock::new()
                                    .text(list.clone())
                                    .font_size(12.0)
                                    .is_text_selection_enabled(true),
                            ),
                            TextBlock::new()
                                .text(status)
                                .font_size(12.0)
                                .foreground_optional(
                                    applies
                                        .is_err()
                                        .then_some(Brush::Solid(Color::rgb(0xc4, 0x6a, 0x00))),
                                )
                                .text_wrapping(TextWrapping::Wrap),
                            if *mailbox {
                                View::empty()
                            } else {
                                check("index", tr!("Stage the changes too (--index)"), *index)
                            },
                            check(
                                "three-way",
                                tr!("3-way merge where it doesn’t apply"),
                                *three_way,
                            ),
                        )),
                    )
            }
            Some(Dialog::Bundle {
                what, newer, base, ..
            }) => dialog
                .is_open(true)
                .title(tr!("Save as Bundle"))
                .primary_button_text(tr!("Save…"))
                .close_button_text(tr!("Cancel"))
                // 「後だけ」を選んで基点が空なら押せない（黙って全体を保存しない。GTK4 版と同じ）。
                .is_primary_button_enabled(!*newer || !base.trim().is_empty())
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(tr!(
                                "{what}, in one file that git can clone or fetch from (git bundle).",
                                what = what
                            ))
                            .text_wrapping(TextWrapping::Wrap),
                        RadioButtons::new()
                            .items_source(vec![tr!("All history"), tr!("Only commits after:")])
                            .selected_index(usize::from(*newer))
                            .on_selection_changed(
                                context.callback(|i: Option<usize>| Message::BundleNewer(i == Some(1))),
                            ),
                        TextBox::new()
                            .text(base.clone())
                            .placeholder_text(tr!("Tag, branch or commit"))
                            .is_enabled(*newer)
                            .on_text_changed(context.callback(Message::BranchName)),
                        TextBlock::new()
                            .text(tr!("Whoever receives the bundle needs that commit already."))
                            .font_size(12.0)
                            .opacity(0.6)
                            .text_wrapping(TextWrapping::Wrap),
                    )),
                ),
            Some(Dialog::FetchBundle { message, name, .. }) => dialog
                .is_open(true)
                .title(tr!("Fetch from Bundle"))
                .primary_button_text("Fetch")
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!name.trim().is_empty())
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(message.clone())
                            .text_wrapping(TextWrapping::Wrap),
                        TextBlock::new().text(tr!("Name:")),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(name.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::NewWorktree {
                repo,
                start,
                branch,
                folder,
            }) => {
                let field = |label: String, text: &str, index: usize, placeholder: String| {
                    StackPanel::new().spacing(2.0).children((
                        TextBlock::new().text(label),
                        TextBox::new()
                            .text(text)
                            .placeholder_text(placeholder)
                            .on_text_changed(context.callback(move |t| Message::WorktreeField(index, t))),
                    ))
                };
                let hint = if branch.trim().is_empty() {
                    String::new()
                } else {
                    default_worktree_folder(repo, branch.trim())
                };
                dialog
                    .is_open(true)
                    .title(tr!("New Worktree"))
                    .primary_button_text(tr!("Create"))
                    .close_button_text(tr!("Cancel"))
                    .is_primary_button_enabled(!branch.trim().is_empty())
                    .content(
                        StackPanel::new().spacing(8.0).children((
                            TextBlock::new()
                                .text(tr!(
                                    "Creates a branch from {start} and checks it out in a new folder.",
                                    start = shown_start(start)
                                ))
                                .text_wrapping(TextWrapping::Wrap),
                            StackPanel::new().spacing(2.0).children((
                                TextBlock::new().text(tr!("Branch name")),
                                TextBox::new()
                                    .element_ref(&self.dialog_input)
                                    .text(branch.clone())
                                    .on_text_changed(context.callback(|t| Message::WorktreeField(0, t))),
                            )),
                            field(tr!("Folder"), folder, 1, hint),
                        )),
                    )
            }
            Some(Dialog::LockWorktree { name, reason, .. }) => dialog
                .is_open(true)
                .title(tr!("Lock the Worktree “{name}”?", name = name))
                .primary_button_text(tr!("Lock"))
                .close_button_text(tr!("Cancel"))
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(tr!(
                                "git then won’t prune, remove or move it, for example while it is on a disk that isn’t always connected (git worktree lock)."
                            ))
                            .text_wrapping(TextWrapping::Wrap),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(reason.clone())
                            .placeholder_text(tr!("Reason (optional)"))
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::WorktreeFolder {
                name, to, repair, ..
            }) => dialog
                .is_open(true)
                .title(if *repair {
                    tr!("Locate the Worktree “{name}”", name = name)
                } else {
                    tr!("Move the Worktree “{name}”", name = name)
                })
                .primary_button_text(if *repair { tr!("Reconnect") } else { tr!("Move") })
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!to.trim().is_empty())
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(if *repair {
                                tr!(
                                    "If you moved its folder yourself, give where it is now, and git reconnects it (git worktree repair)."
                                )
                            } else {
                                tr!("Moves the folder and tells git where it is now (git worktree move).")
                            })
                            .text_wrapping(TextWrapping::Wrap),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(to.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::CloseMerge) => dialog
                .is_open(true)
                .title(tr!("Close without saving?"))
                .primary_button_text(tr!("Close"))
                .close_button_text(tr!("Cancel"))
                .content(
                    TextBlock::new()
                        .text(tr!("Your changes to Result are lost."))
                        .text_wrapping(TextWrapping::Wrap),
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
            Some(Dialog::Form {
                title,
                message,
                button,
                fields,
                checks,
                ..
            }) => {
                // 欄（見出しがあれば上に）とチェックを縦に並べる。最初の欄にフォーカスと Enter。
                let mut rows: Vec<(String, View)> = vec![(
                    "message".to_owned(),
                    TextBlock::new()
                        .text(message.clone())
                        .text_wrapping(TextWrapping::Wrap)
                        .into(),
                )];
                for (i, (label, value, placeholder)) in fields.iter().enumerate() {
                    if !label.is_empty() {
                        rows.push((format!("label{i}"), TextBlock::new().text(label.clone()).into()));
                    }
                    let field = TextBox::new()
                        .text(value.clone())
                        .placeholder_text(placeholder.clone())
                        .on_text_changed(context.callback(move |t| Message::FormField(i, t)));
                    let field: View = if i == 0 {
                        field.element_ref(&self.dialog_input).into()
                    } else {
                        field.into()
                    };
                    rows.push((format!("field{i}"), field));
                }
                for (i, (label, on)) in checks.iter().enumerate() {
                    rows.push((
                        format!("check{i}"),
                        CheckBox::new()
                            .is_checked(*on)
                            .on_is_checked_changed(context.callback(move |v| Message::FormCheck(i, v)))
                            .content(TextBlock::new().text(label.clone()).text_wrapping(TextWrapping::Wrap)),
                    ));
                }
                dialog
                    .is_open(true)
                    .title(title.clone())
                    .primary_button_text(button.clone())
                    .close_button_text(tr!("Cancel"))
                    .content(
                        StackPanel::new()
                            .spacing(6.0)
                            .width(440.0)
                            .children((View::keyed_fragment(rows),)),
                    )
            }
            Some(Dialog::RenameRepository { name, .. }) => dialog
                .is_open(true)
                .title(tr!("Rename"))
                .primary_button_text(tr!("Rename"))
                .close_button_text(tr!("Cancel"))
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(tr!("Name shown in the list (leave empty for the folder name)"))
                            .text_wrapping(TextWrapping::Wrap),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(name.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
                ),
            Some(Dialog::NewGroup { name, .. }) => dialog
                .is_open(true)
                .title(tr!("New Group"))
                .primary_button_text(tr!("Add"))
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!name.trim().is_empty())
                .content(
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new().text(tr!("Group name")),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(name.clone())
                            .on_text_changed(context.callback(Message::BranchName)),
                    )),
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
                            self.sync_lists_view(),
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
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(message.clone())
                            .text_wrapping(TextWrapping::Wrap),
                        self.sync_lists_view(),
                    )),
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
            Some(Dialog::Fetch {
                remotes,
                from,
                prune,
                ..
            }) => {
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
                // 選んだリモートで、消えたブランチを片づける設定が効いていれば知らせる（GTK4 版の `prune_line`）。
                let chosen: Vec<usize> = match from.checked_sub(1) {
                    Some(i) => vec![i],
                    None => (0..remotes.len()).collect(),
                };
                let pruned: Vec<&String> = chosen
                    .iter()
                    .filter(|&&i| prune.get(i).is_some_and(Option::is_some))
                    .map(|&i| &remotes[i])
                    .collect();
                let mut keys: Vec<String> = Vec::new();
                for key in chosen.iter().filter_map(|&i| prune.get(i).cloned().flatten()) {
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                }
                let note = if pruned.is_empty() {
                    String::new()
                } else if pruned.len() == chosen.len() {
                    tr!(
                        "Remote branches deleted there are removed here too ({settings}).",
                        settings = amonhen_model::i18n::list(&keys)
                    )
                } else {
                    tr!(
                        "Remote branches deleted on {remotes} are removed here too ({settings}).",
                        remotes = amonhen_model::i18n::list(&pruned),
                        settings = amonhen_model::i18n::list(&keys)
                    )
                };
                let content = StackPanel::new().spacing(8.0).children((
                    TextBlock::new()
                        .text(note)
                        .text_wrapping(TextWrapping::Wrap),
                    content,
                ));
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
            Some(Dialog::Clone {
                url,
                folder,
                name,
                download,
                depth,
                sparse,
                submodules,
            }) => {
                let field = |index: usize, text: &str, placeholder: String| {
                    TextBox::new()
                        .text(text.to_owned())
                        .placeholder_text(placeholder)
                        .on_text_changed(context.callback(move |t| Message::CloneField(index, t)))
                };
                let label = |text: String| TextBlock::new().text(text).margin(Thickness::new(0.0, 4.0, 0.0, 0.0));
                let downloads = vec![
                    tr!("Everything"),
                    tr!("File contents when needed"),
                    tr!("Recent history only"),
                ];
                let help = match download {
                    1 => tr!(
                        "Good for large repositories: file contents are fetched when a diff or file needs them."
                    ),
                    2 => tr!("Older history can be fetched later from the end of History."),
                    _ => tr!(
                        "The whole history and the contents of every file, as git does by default."
                    ),
                };
                let depths: Vec<String> = CLONE_DEPTHS
                    .iter()
                    .map(|&d| match d {
                        1 => tr!("1 Commit"),
                        100 => tr!("100 Commits"),
                        1_000 => tr!("1,000 Commits"),
                        _ => tr!("10,000 Commits"),
                    })
                    .collect();
                let depth_box: View = if *download == 2 {
                    ComboBox::new()
                        .items_source(depths)
                        .selected_index(*depth)
                        .on_selection_changed(context.callback(Message::CloneDepth))
                        .into()
                } else {
                    View::empty()
                };
                let ready = !url.trim().is_empty() && !folder.trim().is_empty();
                dialog
                    .is_open(true)
                    .title(tr!("Clone Repository"))
                    .primary_button_text("Clone")
                    .close_button_text(tr!("Cancel"))
                    .is_primary_button_enabled(ready)
                    .content(
                        StackPanel::new().spacing(4.0).width(480.0).children((
                            field(
                                0,
                                url,
                                tr!(
                                    "https://github.com/owner/repo.git, git@host:owner/repo.git or a bundle file"
                                ),
                            )
                            .element_ref(&self.dialog_input),
                            label(tr!("Location:")),
                            Grid::new()
                                .columns([GridLength::Star(1.0), GridLength::Auto])
                                .column_spacing(6.0)
                                .children((
                                    field(1, folder, String::new()).grid_column(0),
                                    Button::new()
                                        .on_click(context.callback(|()| Message::CloneChoose))
                                        .grid_column(1)
                                        .content(TextBlock::new().text(tr!("Choose…"))),
                                )),
                            label(tr!("Folder Name:")),
                            field(2, name, tr!("Taken from the URL")),
                            label(tr!("Download:")),
                            StackPanel::new()
                                .orientation(Orientation::Horizontal)
                                .spacing(6.0)
                                .children((
                                    ComboBox::new()
                                        .items_source(downloads)
                                        .selected_index(*download)
                                        .on_selection_changed(
                                            context.callback(Message::CloneDownload),
                                        ),
                                    depth_box,
                                )),
                            TextBlock::new()
                                .text(help)
                                .font_size(12.0)
                                .opacity(0.65)
                                .text_wrapping(TextWrapping::Wrap),
                            CheckBox::new()
                                .is_checked(*sparse)
                                .on_is_checked_changed(
                                    context.callback(|on| Message::CloneCheck(0, on)),
                                )
                                .content(TextBlock::new().text(tr!("Check out only some folders"))),
                            CheckBox::new()
                                .is_checked(*submodules)
                                .on_is_checked_changed(
                                    context.callback(|on| Message::CloneCheck(1, on)),
                                )
                                .content(TextBlock::new().text(tr!("Check out submodules too"))),
                        )),
                    )
            }
            Some(Dialog::QuickOpen {
                query,
                selected,
                candidates,
            }) => {
                let shown = crate::repositories::matches(candidates, query);
                let selected = (*selected).min(shown.len().saturating_sub(1));
                let rows = shown.iter().map(|c| {
                    (
                        c.location.clone(),
                        ListViewItem::new().tag(c.location.clone()).content(
                            StackPanel::new().spacing(1.0).children((
                                TextBlock::new().text(c.name.clone()),
                                TextBlock::new()
                                    .text(crate::repositories::shown_path(&c.location))
                                    .font_size(11.0)
                                    .opacity(0.6)
                                    .text_trimming(TextTrimming::CharacterEllipsis),
                            )),
                        ),
                    )
                });
                // 打つたび・候補が増えるたびに一覧ごと作り直す（reactor の ListView は消した項目の場所が空いた
                // まま残る）。
                let key = format!("{query}\n{}", shown.len());
                let list = ListView::new()
                    .selected_index((!shown.is_empty()).then_some(selected))
                    .on_selection_changed(context.callback(Message::SwitcherSelect))
                    .collection_slot(ListViewSlot::Items, rows);
                dialog
                    .is_open(true)
                    .title("Quick Open")
                    .primary_button_text(tr!("Open"))
                    .close_button_text(tr!("Cancel"))
                    .is_primary_button_enabled(!shown.is_empty())
                    .content(
                        StackPanel::new().spacing(6.0).width(480.0).children((
                            TextBox::new()
                                .element_ref(&self.dialog_input)
                                .text(query.clone())
                                .placeholder_text(tr!(
                                    "Open a repository (fuzzy search by name or path)"
                                ))
                                .on_text_changed(context.callback(Message::BranchName)),
                            ScrollViewer::new()
                                .height(320.0)
                                .content(View::keyed_fragment([(key, list)])),
                        )),
                    )
            }
            Some(Dialog::Switcher {
                query,
                selected,
                recent,
                ..
            }) => {
                let shown = self
                    .refs
                    .as_ref()
                    .map(|r| crate::branchswitcher::candidates(&r.refs, recent, query))
                    .unwrap_or_default();
                let selected = (*selected).min(shown.len().saturating_sub(1));
                let rows = shown.iter().map(|c| {
                    let key = c.label(&[]);
                    (
                        key.clone(),
                        ListViewItem::new()
                            .tag(key)
                            .content(TextBlock::new().text(c.label(recent))),
                    )
                });
                // 打つたびに一覧ごと作り直す（reactor の ListView は消した項目の場所が空いたまま残る）。
                let list = ListView::new()
                    .selected_index((!shown.is_empty()).then_some(selected))
                    .on_selection_changed(context.callback(Message::SwitcherSelect))
                    .collection_slot(ListViewSlot::Items, rows);
                dialog
                    .is_open(true)
                    .title(tr!("Switch Branch"))
                    .primary_button_text(tr!("Switch"))
                    .close_button_text(tr!("Cancel"))
                    .is_primary_button_enabled(!shown.is_empty())
                    .content(
                        StackPanel::new().spacing(6.0).width(420.0).children((
                            TextBox::new()
                                .element_ref(&self.dialog_input)
                                .text(query.clone())
                                .placeholder_text(tr!("Branch name"))
                                .on_text_changed(context.callback(Message::BranchName)),
                            ScrollViewer::new().height(300.0).content(View::keyed_fragment([(
                                query.clone(),
                                list,
                            )])),
                        )),
                    )
            }
            Some(Dialog::Restore { title, staged, .. }) => dialog
                .is_open(true)
                .title(title.clone())
                .primary_button_text(tr!("Restore"))
                .close_button_text(tr!("Cancel"))
                .content(
                    StackPanel::new().spacing(8.0).children((
                        TextBlock::new()
                            .text(tr!(
                                "Replaces the file in the working tree with that version (git restore); a file that version doesn’t have is deleted. Undo brings back the file as it is now."
                            ))
                            .text_wrapping(TextWrapping::Wrap),
                        CheckBox::new()
                            .is_checked(*staged)
                            .on_is_checked_changed(context.callback(Message::RestoreStaged))
                            .content(TextBlock::new().text(tr!("Stage it too"))),
                    )),
                ),
            Some(Dialog::NewOrphan { name, keep, .. }) => dialog
                .is_open(true)
                .title(tr!("New Orphan Branch"))
                .primary_button_text(tr!("Create"))
                .close_button_text(tr!("Cancel"))
                .is_primary_button_enabled(!name.trim().is_empty())
                .content(
                    StackPanel::new().spacing(8.0).children((
                        TextBlock::new()
                            .text(tr!(
                                "A branch with no history. It is checked out, and is created by its first commit."
                            ))
                            .text_wrapping(TextWrapping::Wrap),
                        TextBox::new()
                            .element_ref(&self.dialog_input)
                            .text(name.clone())
                            .placeholder_text(tr!("Name of the branch"))
                            .on_text_changed(context.callback(Message::BranchName)),
                        CheckBox::new()
                            .is_checked(*keep)
                            .on_is_checked_changed(context.callback(Message::OrphanKeep))
                            .content(TextBlock::new().text(tr!(
                                "Keep the current files, staged for the first commit"
                            ))),
                    )),
                ),
            Some(Dialog::DeleteIgnored { paths, .. }) => dialog
                .is_open(true)
                .title(tr!("Delete {count} Ignored Items?", count = paths.len()))
                .primary_button_text(tr!("Delete"))
                .close_button_text(tr!("Cancel"))
                .content(
                    StackPanel::new().spacing(8.0).children((
                        TextBlock::new()
                            .text(format!(
                                "{} {}",
                                tr!(
                                    "These are removed from the working tree (git clean -X); folders go with everything in them."
                                ),
                                tr!("This can’t be undone.")
                            ))
                            .text_wrapping(TextWrapping::Wrap),
                        // パスを選んで写せる（長いパスは折り返す。GTK4 版と同じ）。
                        ScrollViewer::new().max_height(160.0).content(
                            TextBlock::new()
                                .text(paths.join("\n"))
                                .font_size(12.0)
                                .text_wrapping(TextWrapping::Wrap)
                                .is_text_selection_enabled(true),
                        ),
                    )),
                ),
            Some(Dialog::CleanUp {
                base, branches, ..
            }) => {
                // 理由ごとに見出しを付けて並べる（GTK4 版と同じ）。
                let mut rows: Vec<(String, View)> = Vec::new();
                for (reason, title) in [
                    ("merged", tr!("Merged into {base}", base = base)),
                    (
                        "pullRequest",
                        tr!("Merged by a pull request (squash or rebase)"),
                    ),
                    (
                        "gone",
                        tr!("Upstream gone, not merged (their own commits go with them)"),
                    ),
                ] {
                    let group: Vec<(usize, &(Value, bool))> = branches
                        .iter()
                        .enumerate()
                        .filter(|(_, (b, _))| b["reason"] == reason)
                        .collect();
                    if group.is_empty() {
                        continue;
                    }
                    rows.push((
                        format!("h:{reason}"),
                        TextBlock::new()
                            .text(title)
                            .font_size(12.0)
                            .opacity(0.6)
                            .margin(Thickness::new(0.0, 6.0, 0.0, 0.0))
                            .into(),
                    ));
                    for (i, (b, on)) in group {
                        let mut label = b["name"].as_str().unwrap_or_default().to_owned();
                        if let Some(n) = b["pullRequest"].as_u64() {
                            label.push_str(&format!("  #{n}"));
                        }
                        if reason == "gone"
                            && let Some(u) = b["upstream"].as_str()
                        {
                            label.push_str(&format!("  ({u})"));
                        }
                        rows.push((
                            format!("b:{i}"),
                            CheckBox::new()
                                .is_checked(*on)
                                .on_is_checked_changed(
                                    context.callback(move |v| Message::CleanUpCheck(i, v)),
                                )
                                .content(TextBlock::new().text(label)),
                        ));
                    }
                }
                let count = branches.iter().filter(|(_, on)| *on).count();
                let button = if count == 1 {
                    tr!("Delete 1 Branch")
                } else {
                    tr!("Delete {count} Branches", count = count)
                };
                dialog
                    .is_open(true)
                    .title(tr!("Clean Up Branches"))
                    .primary_button_text(button)
                    .is_primary_button_enabled(count > 0)
                    .close_button_text(tr!("Cancel"))
                    .content(
                        StackPanel::new().spacing(4.0).children((
                            TextBlock::new()
                                .text(format!(
                                    "{} {}",
                                    tr!(
                                        "Delete the checked branches. The current branch, favorites and branches used by a worktree aren’t listed."
                                    ),
                                    tr!("You can Undo this (Ctrl+Alt+Z).")
                                ))
                                .text_wrapping(TextWrapping::Wrap),
                            ScrollViewer::new().max_height(280.0).content(
                                StackPanel::new()
                                    .spacing(2.0)
                                    .children((View::keyed_fragment(rows),)),
                            ),
                        )),
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
    /// Cherry-pick・Revert（GTK4 版の `sheets::pick`）。いくつでも 1 つの命令で（コンフリクトで止まったら、残りは
    /// git が覚えていて Continue・Skip で続く）。
    fn pick(&mut self, oids: Vec<String>, revert: bool) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let onto = self
            .refs
            .as_ref()
            .and_then(|r| r.refs.head_branch.clone())
            .unwrap_or_else(|| "HEAD".to_owned());
        let what = match &oids[..] {
            [one] => one[..8.min(one.len())].to_owned(),
            _ => tr!("{count} commits", count = oids.len()),
        };
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
        // マージかは、複数なら選んだ行の印で、1 つなら出している詳細の親の数で（同じコミットのとき）。
        let merge = oids.iter().any(|oid| {
            self.multi.iter().any(|(id, merge)| id == oid && *merge)
                || self.info.as_ref().is_some_and(|(info, _)| {
                    info["oid"].as_str() == Some(oid.as_str())
                        && info["parents"].as_array().is_some_and(|p| p.len() > 1)
                })
        });
        let op = if revert { "revert" } else { "cherryPick" };
        self.dialog = Some(Dialog::Confirm {
            repo,
            title,
            message,
            button: if revert { "Revert" } else { "Cherry-pick" }.to_owned(),
            command: json!({ "op": op, "oids": oids, "isMerge": merge }),
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
        // worktree の行（GTK4 版の `worktrees::menu` のうち、編集する worktree を替える以外）。
        if let Some(path) = self
            .sidebar_key
            .as_deref()
            .and_then(|k| k.strip_prefix("worktree:"))
        {
            let Some(w) = self.worktrees.iter().find(|w| w.path == path) else {
                return Vec::new();
            };
            let mut items = vec![MenuItem::item(format!("copy:{path}"), tr!("Copy Path"))];
            if w.prunable {
                items.push(MenuItem::separator("-prune"));
                // フォルダを手で移したなら、つながりを直せる（git worktree repair。GTK4 版と同じ）。
                items.push(MenuItem::item(
                    format!("wt-locate:{path}"),
                    tr!("Locate Moved Folder…"),
                ));
                items.push(MenuItem::item("prune-worktrees", tr!("Prune Worktrees")));
            } else if !w.main {
                items.push(MenuItem::separator("-lock"));
                if w.locked {
                    items.push(MenuItem::item(format!("wt-unlock:{path}"), tr!("Unlock")));
                } else {
                    items.push(MenuItem::item(format!("wt-lock:{path}"), tr!("Lock…")));
                    // 開いているリポジトリの worktree は移さない（窓が古いパスを指したままになる）。
                    let opened = self
                        .repo
                        .as_ref()
                        .and_then(|r| r.paths().workdir)
                        .is_some_and(|d| same_path(&d.display().to_string(), path));
                    if !opened {
                        items.push(MenuItem::item(format!("wt-move:{path}"), tr!("Move…")));
                    }
                }
                items.push(MenuItem::separator("-remove"));
                items.push(MenuItem::item(
                    format!("wt-remove:{path}"),
                    tr!("Remove Worktree…"),
                ));
            }
            return items;
        }
        // リモートと Remotes の見出し（GTK4 版の `refops::remote_menu`）。
        match self.sidebar_key.as_deref() {
            Some("group:branches") => {
                return vec![MenuItem::item(
                    "clean-up-branches",
                    tr!("Clean Up Branches…"),
                )];
            }
            Some("group:remotes") => {
                return vec![MenuItem::item("remote-add", tr!("Add Remote…"))];
            }
            // subtree と Subtrees の見出し（GTK4 版の `remote_menu`）。
            Some("group:subtrees") => {
                return vec![MenuItem::item("st-add", tr!("Add Subtree…"))];
            }
            Some(key) if key.starts_with("subtree:") => {
                let dir = &key["subtree:".len()..];
                return vec![
                    MenuItem::item(format!("st-pull:{dir}"), "Pull…"),
                    MenuItem::item(format!("st-push:{dir}"), "Push…"),
                    MenuItem::item(format!("st-split:{dir}"), "Split…"),
                ];
            }
            // Submodules の見出し（GTK4 版の `remote_menu` の Submodules）。
            Some("group:submodules") => {
                return vec![
                    MenuItem::item("sm-update-all", "Update All"),
                    MenuItem::item("sm-update:", "Update Submodules…"),
                    MenuItem::item("sm-sync", "Sync URLs"),
                    MenuItem::item("sm-foreach", tr!("Run in Each Submodule…")),
                    MenuItem::separator("-add"),
                    MenuItem::item("sm-add", tr!("Add Submodule…")),
                ];
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
        // submodule（GTK4 版の `refops::items` の Submodule）。
        if entry.kind == RefKind::Submodule {
            let state = crate::sidebar::submodule_state(&self.submodules, name);
            let on = |k: &str| state.is_some_and(|s| s[k].as_bool() == Some(true));
            items.push(MenuItem::separator("-copy"));
            items.push(MenuItem::item(
                format!("sm-update1:{name}"),
                format!("Update {name}"),
            ));
            items.push(MenuItem::item(format!("sm-update:{name}"), "Update…"));
            if state.is_none() || on("initialized") {
                items.push(MenuItem::item(
                    format!("sm-open:{name}"),
                    tr!("Open in New Window"),
                ));
            }
            items.push(MenuItem::separator("-edit"));
            items.push(MenuItem::item(format!("sm-url:{name}"), tr!("Edit URL…")));
            items.push(MenuItem::item(
                format!("sm-branch:{name}"),
                tr!("Branch to Follow…"),
            ));
            if on("ownGitDir") {
                items.push(MenuItem::item(
                    format!("sm-absorb:{name}"),
                    tr!("Move Repository Data into .git/modules"),
                ));
            }
            items.push(MenuItem::separator("-remove"));
            items.push(MenuItem::item(format!("sm-remove:{name}"), tr!("Remove…")));
            return items;
        }
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
                items.extend(self.sync_items(entry));
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
        // bundle に書く（GTK4 版の `save_bundle`。ブランチ・リモートのブランチ・タグ 1 つ）。
        let full = match entry.kind {
            RefKind::Local => format!("refs/heads/{name}"),
            RefKind::Remote => format!("refs/remotes/{name}"),
            _ => format!("refs/tags/{name}"),
        };
        items.push(MenuItem::item(
            format!("bundle:{full}:{name}"),
            tr!("Save as Bundle…"),
        ));
        if entry.kind == RefKind::Local {
            items.push(MenuItem::item(
                format!("tag:{name}"),
                tr!("New Tag at {name}…", name = name),
            ));
            items.push(MenuItem::item(format!("rename:{name}"), tr!("Rename…")));
            // お気に入りとアーカイブ（git の設定に書く。GTK4 版の `favorite_items`）。
            let branch = self.groups.as_ref().and_then(|g| g.branches.get(name));
            items.push(MenuItem::item(
                format!("favorite:{name}"),
                if branch.is_some_and(|b| b.pinned.is_some()) {
                    tr!("Remove from Favorites")
                } else {
                    tr!("Add to Favorites")
                },
            ));
            items.push(MenuItem::item(
                format!("archive:{name}"),
                if branch.is_some_and(|b| b.archived) {
                    tr!("Unarchive")
                } else {
                    tr!("Archive")
                },
            ));
            if !is_head {
                items.push(MenuItem::item(
                    format!("delete-branch:{name}"),
                    tr!("Delete {name}…", name = name),
                ));
            }
        }
        if entry.kind == RefKind::Tag {
            items.push(MenuItem::separator("-tag"));
            items.extend(self.tag_items(name));
        }
        items
    }

    /// submodule と Submodules の見出しのメニュー（GTK4 版の `refops` と `submodulesheet`）。
    fn submodule_menu(&mut self, key: &str, context: &ComponentContext<Self>) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let (what, path) = key.split_once(':').unwrap_or((key, ""));
        let path = path.to_owned();
        let state = crate::sidebar::submodule_state(&self.submodules, &path).cloned();
        let text = |k: &str| {
            state
                .as_ref()
                .and_then(|s| s[k].as_str())
                .unwrap_or_default()
                .to_owned()
        };
        let form =
            |title: String, message: String, button: String, fields, checks, kind| Dialog::Form {
                repo: repo.clone(),
                title,
                message,
                button,
                fields,
                checks,
                kind,
            };
        let dialog = match what {
            "sm-update-all" => {
                return perform(
                    context,
                    repo,
                    json!({ "op": "updateSubmodules", "paths": [] }),
                    "Update All".to_owned(),
                );
            }
            "sm-update1" => {
                return perform(
                    context,
                    repo,
                    json!({ "op": "updateSubmodules", "paths": [path] }),
                    format!("Update {path}"),
                );
            }
            "sm-sync" => {
                return perform(
                    context,
                    repo,
                    json!({ "op": "changeSubmodule", "change": { "kind": "sync", "recursive": true } }),
                    "Sync URLs".to_owned(),
                );
            }
            "sm-absorb" => {
                return perform(
                    context,
                    repo,
                    json!({ "op": "changeSubmodule", "change": { "kind": "absorb", "paths": [path] } }),
                    tr!("Move Repository Data into .git/modules"),
                );
            }
            "sm-open" => {
                if let Some(dir) = repo.paths().workdir
                    && let Err(e) =
                        crate::repositories::open_window(&dir.join(&path).display().to_string())
                {
                    self.dialog = Some(Dialog::Failed {
                        title: tr!("Open in New Window"),
                        message: e,
                    });
                }
                return;
            }
            "sm-update" => {
                let paths: Vec<String> = (!path.is_empty())
                    .then(|| path.clone())
                    .into_iter()
                    .collect();
                form(
                    if path.is_empty() {
                        tr!("Update All Submodules")
                    } else {
                        tr!("Update {path}", path = &path)
                    },
                    tr!(
                        "Checks out the commit recorded in this repository, unless you choose otherwise."
                    ),
                    "Update".to_owned(),
                    Vec::new(),
                    vec![
                        (
                            tr!("Also update the submodules inside it (--recursive)"),
                            false,
                        ),
                        (
                            tr!("Use the latest commit of the branch it follows (--remote)"),
                            false,
                        ),
                    ],
                    FormKind::UpdateSubmodules(paths),
                )
            }
            "sm-foreach" => form(
                tr!("Run in Each Submodule"),
                tr!(
                    "Runs the command with the shell in each submodule’s folder (git submodule foreach). Its output is in Activity."
                ),
                tr!("Run"),
                vec![(String::new(), String::new(), tr!("Command"))],
                vec![(
                    tr!("Also in the submodules inside them (--recursive)"),
                    false,
                )],
                FormKind::RunInEach,
            ),
            "sm-add" => form(
                tr!("Add Submodule"),
                tr!(
                    "Clones the repository into a folder of this one and records it in .gitmodules (git submodule add). Commit to keep it."
                ),
                tr!("Add"),
                vec![
                    (tr!("Repository URL"), String::new(), String::new()),
                    (tr!("Folder"), String::new(), String::new()),
                    (tr!("Branch to follow"), String::new(), tr!("(optional)")),
                ],
                Vec::new(),
                FormKind::AddSubmodule,
            ),
            "sm-url" => form(
                tr!("Edit URL of {path}", path = &path),
                tr!(
                    "Changes the URL in .gitmodules and copies it to this repository’s settings (git submodule set-url). Commit .gitmodules to share it."
                ),
                tr!("Save"),
                vec![(String::new(), text("url"), tr!("Repository URL"))],
                Vec::new(),
                FormKind::SetUrl(path.clone()),
            ),
            "sm-branch" => form(
                tr!("Branch to Follow for {path}", path = &path),
                tr!(
                    "Updating with “the latest commit of the branch it follows” moves to this branch’s latest commit. Leave it empty for the remote’s default branch."
                ),
                tr!("Save"),
                vec![(String::new(), text("branch"), tr!("Default branch"))],
                Vec::new(),
                FormKind::SetBranch(path.clone()),
            ),
            // subtree（GTK4 版の `subtreesheet`）。この機械の git に subtree がなければ鳴らす。
            "st-add" | "st-pull" | "st-push" | "st-split"
                if self.subtrees["available"].as_bool() != Some(true) =>
            {
                return beep();
            }
            "st-add" => form(
                tr!("Add Subtree"),
                tr!(
                    "Brings another repository’s branch into a folder of this one as a commit (git subtree add). Pull and Push later use the same repository and branch."
                ),
                tr!("Add"),
                vec![
                    (tr!("Repository URL"), String::new(), String::new()),
                    (tr!("Branch or tag"), String::new(), String::new()),
                    (tr!("Folder"), String::new(), String::new()),
                ],
                vec![(tr!("Squash its history into one commit (--squash)"), false)],
                FormKind::AddSubtree,
            ),
            "st-pull" | "st-push" => {
                let push = what == "st-push";
                let subtree = self.subtrees["subtrees"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|s| s["dir"].as_str() == Some(&path))
                    .cloned()
                    .unwrap_or_default();
                let known = |k: &str| subtree[k].as_str().unwrap_or_default().to_owned();
                let rev = if push && !known("pushRef").is_empty() {
                    known("pushRef")
                } else {
                    known("ref")
                };
                let (title, message) = if push {
                    (
                        tr!("Push {dir}", dir = &path),
                        tr!(
                            "Sends the commits that changed this folder to the repository’s branch, as that repository’s own history (git subtree push)."
                        ),
                    )
                } else {
                    (
                        tr!("Pull {dir}", dir = &path),
                        tr!(
                            "Merges the repository’s branch into this folder (git subtree pull). Conflicts stop it like a merge."
                        ),
                    )
                };
                let checks = if push {
                    Vec::new()
                } else {
                    vec![(
                        tr!("Squash its history into one commit (--squash)"),
                        subtree["squash"].as_bool() == Some(true),
                    )]
                };
                form(
                    title,
                    message,
                    if push { "Push" } else { "Pull" }.to_owned(),
                    vec![
                        (tr!("Repository URL"), known("url"), String::new()),
                        (tr!("Branch or tag"), rev, String::new()),
                    ],
                    checks,
                    FormKind::PullOrPushSubtree(path.clone(), push),
                )
            }
            "st-split" => {
                let name = path.rsplit('/').next().unwrap_or(&path);
                form(
                    tr!("Split {dir}", dir = &path),
                    tr!(
                        "Creates a branch with only the history of this folder, its files at the top (git subtree split)."
                    ),
                    tr!("Create"),
                    vec![(String::new(), String::new(), format!("{name}-split"))],
                    Vec::new(),
                    FormKind::SplitSubtree(path.clone()),
                )
            }
            "sm-remove" => {
                let name = match text("name") {
                    n if n.is_empty() => path.clone(),
                    n => n,
                };
                form(
                    tr!("Remove {name}?", name = &path),
                    format!(
                        "{} {}\n\n{}",
                        tr!(
                            "Removes the folder and its entry in .gitmodules, and stages that. Commit to keep it."
                        ),
                        tr!("This can’t be undone."),
                        tr!(
                            "Kept, it brings back the submodule’s commits and stashes when you add it again, including ones not pushed."
                        )
                    ),
                    tr!("Remove"),
                    Vec::new(),
                    vec![(
                        tr!(
                            "Also delete its repository data (.git/modules/{name})",
                            name = &name
                        ),
                        false,
                    )],
                    FormKind::RemoveSubmodule(path.clone()),
                )
            }
            _ => return,
        };
        self.dialog = Some(dialog);
        self.dialog_inputs += 1;
    }

    /// タグのメニュー（GTK4 版の `refops::tag_items`）: リモートごとに Push（ないもの）・Replace（同じ名前の別の
    /// タグがあるもの）、Delete、リモートごとの Delete from（あるもの）。まだ読んでいないリモートは、どれも出す。
    fn tag_items(&self, name: &str) -> Vec<MenuItem> {
        let remotes: Vec<(String, bool)> = self.tag_remotes["remotes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| Some((r["name"].as_str()?.to_owned(), r["checked"].is_i64())))
            .collect();
        let state = |remote: &str| self.tag_remotes["tags"][name][remote].as_str();
        let (mut pushes, mut deletes) = (Vec::new(), Vec::new());
        for (remote, checked) in &remotes {
            let st = state(remote);
            let missing = *checked && st.is_none_or(|s| s == "missing");
            if !*checked || missing {
                pushes.push(MenuItem::item(
                    format!("push-tag:{remote}|{name}"),
                    tr!("Push {branch} to {target}…", branch = name, target = remote),
                ));
            }
            if *checked && st == Some("different") {
                pushes.push(MenuItem::item(
                    format!("replace-tag:{remote}|{name}"),
                    tr!("Replace {tag} on {remote}…", tag = name, remote = remote),
                ));
            }
            if !missing {
                deletes.push(MenuItem::item(
                    format!("delete-remote-tag:{remote}|{name}|{}", u8::from(!*checked)),
                    tr!("Delete {tag} from {remote}…", tag = name, remote = remote),
                ));
            }
        }
        // リモートごとの様子（GTK4 版はタグの印のポップオーバーに出す）。
        let mut items: Vec<MenuItem> = remotes
            .iter()
            .map(|(remote, _)| {
                MenuItem::disabled(
                    format!("-state:{remote}"),
                    crate::sidebar::tag_state_text(&self.tag_remotes, name, remote),
                )
            })
            .collect();
        if !items.is_empty() {
            items.push(MenuItem::separator("-states"));
        }
        let pushed = !pushes.is_empty();
        items.extend(pushes);
        if pushed {
            items.push(MenuItem::separator("-push-tag"));
        }
        items.push(MenuItem::item(
            format!("delete-tag:{name}"),
            tr!("Delete {name}…", name = name),
        ));
        items.extend(deletes);
        items
    }

    /// `name` をお気に入りに入れる・外す命令（GTK4 版の `favorite_commands`。外すときは残りの順を詰める）。
    fn favorite_commands(&self, name: &str) -> Vec<Value> {
        let mut pinned: Vec<(&String, i64)> = self
            .groups
            .as_ref()
            .map(|g| {
                g.branches
                    .iter()
                    .filter_map(|(n, b)| Some((n, b.pinned?)))
                    .collect()
            })
            .unwrap_or_default();
        pinned.sort_by_key(|(n, rank)| (*rank, n.to_string()));
        let mut order: Vec<String> = pinned.into_iter().map(|(n, _)| n.clone()).collect();
        let order_commands = |order: &[String]| -> Vec<Value> {
            order
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    json!({
                        "op": "configSet",
                        "key": format!("branch.{n}.amonhenPinned"),
                        "value": (i + 1).to_string(),
                    })
                })
                .collect()
        };
        if order.iter().any(|n| n == name) {
            order.retain(|n| n != name);
            let mut commands = order_commands(&order);
            commands.push(json!({
                "op": "configSet",
                "key": format!("branch.{name}.amonhenPinned"),
                "value": null,
            }));
            commands
        } else {
            order.push(name.to_owned());
            order_commands(&order)
        }
    }

    /// チェックアウトしていないブランチを上流と合わせる項目（GTK4 版の `refops::sync_items`。最後の Fetch の
    /// 時点）: 上流がなければ Publish、遅れているだけなら Fast-Forward（チェックアウトしないで進める）、先にある
    /// だけなら Push。分かれていればチェックアウトして merge か rebase をする（上の項目）。
    fn sync_items(&self, entry: &RefEntry) -> Vec<MenuItem> {
        let name = &entry.name;
        let mut items = Vec::new();
        match &entry.detail {
            Some(upstream) if !entry.gone => {
                if (entry.ahead == 0) == (entry.behind == 0) {
                    return items;
                }
                if entry.ahead > 0 {
                    items.push(MenuItem::item(
                        format!("push-branch:{name}"),
                        tr!(
                            "Push {branch} to {target}…",
                            branch = name,
                            target = upstream
                        ),
                    ));
                } else if !self
                    .worktrees
                    .iter()
                    .any(|w| w.branch.as_deref() == Some(name.as_str()))
                {
                    // ほかの worktree が checkout しているなら動かせない（その作業ツリーとずれる）。
                    items.push(MenuItem::item(
                        format!("fast-forward:{name}"),
                        tr!(
                            "Fast-Forward {name} to {upstream}…",
                            name = name,
                            upstream = upstream
                        ),
                    ));
                }
            }
            _ => {
                // リモートがなければ Publish の先がないので出さない（GTK4 版と同じ）。
                let has_remote = self.refs.as_ref().is_some_and(|r| {
                    r.refs
                        .entries
                        .iter()
                        .any(|e| e.kind == amonhen_model::RefKind::Remote)
                });
                if !has_remote {
                    return items;
                }
                items.push(MenuItem::item(
                    format!("push-branch:{name}"),
                    tr!("Publish {name}…", name = name),
                ));
            }
        }
        items.push(MenuItem::separator("-sync"));
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

    /// 出した Pull・Push のダイアログに並べるコミットの一覧を裏で読む（GTK4 版の `commit_lists`。取り込むもの・
    /// 手元にだけあるもの、送るもの・上流が失うもの）。
    fn load_sync_lists(&mut self, context: &ComponentContext<Self>) {
        self.sync_lists.clear();
        self.sync_warnings.clear();
        // Push の注意を出すための、上流と送るブランチのコミット。
        let mut push_range: Option<(String, amonhen_model::Oid, amonhen_model::Oid)> = None;
        let (repo, lists) = match &self.dialog {
            Some(Dialog::Pull { repo, sources, .. }) if sources.is_empty() => {
                let Some(upstream) = self.head_entry().and_then(|e| e.detail.clone()) else {
                    return;
                };
                (
                    repo.clone(),
                    vec![
                        (
                            tr!("Commits to pull"),
                            "HEAD".to_owned(),
                            upstream.clone(),
                            false,
                        ),
                        (tr!("Commits only here"), upstream, "HEAD".to_owned(), false),
                    ],
                )
            }
            Some(Dialog::Push {
                repo,
                remote,
                branch,
                remote_branch,
                replace,
                diverged,
                ..
            }) => {
                let upstream = format!("{remote}/{remote_branch}");
                let target = |kind: amonhen_model::RefKind, name: &str| {
                    self.refs
                        .as_ref()?
                        .refs
                        .entries
                        .iter()
                        .find(|e| e.kind == kind && e.name == name)?
                        .target
                };
                if let (Some(base), Some(tip)) = (
                    target(amonhen_model::RefKind::Remote, &upstream),
                    target(amonhen_model::RefKind::Local, branch),
                ) {
                    push_range = Some((upstream.clone(), base, tip));
                }
                let mut lists = vec![(
                    tr!("Commits to send"),
                    upstream.clone(),
                    branch.clone(),
                    false,
                )];
                if *replace || *diverged {
                    lists.push((
                        tr!("Commits {upstream} will lose", upstream = &upstream),
                        branch.clone(),
                        upstream,
                        true,
                    ));
                }
                (repo.clone(), lists)
            }
            _ => return,
        };
        context.spawn_background(move |_| {
            let lists = lists
                .into_iter()
                .map(|(title, base, tip, warn)| {
                    let command = json!({ "op": "between", "base": base, "tip": tip, "limit": 20 });
                    let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                        .unwrap_or_default();
                    let info = &reply["info"];
                    SyncList {
                        title,
                        count: info["count"].as_u64().unwrap_or(0),
                        commits: info["commits"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|c| {
                                (
                                    c["oid"].as_str().unwrap_or_default().to_owned(),
                                    c["subject"].as_str().unwrap_or_default().to_owned(),
                                    c["author"].as_str().unwrap_or_default().to_owned(),
                                    c["time"].as_i64().unwrap_or(0),
                                )
                            })
                            .collect(),
                        warn,
                    }
                })
                .collect::<Vec<SyncList>>();
            // 送る範囲があれば注意（送るものがない Force Push などには出さない。GTK4 版と同じ）。
            let warnings = match push_range {
                Some((upstream, base, tip)) if lists.first().is_some_and(|l| l.count > 0) => {
                    push_warnings(&repo, &upstream, &lists[0].commits, &base, &tip)
                }
                _ => Vec::new(),
            };
            Message::SyncLists(lists, warnings)
        });
    }

    /// Pull・Push のダイアログのコミットの一覧（空の一覧は出さない。消えるものは赤）と、Push の注意。
    fn sync_lists_view(&self) -> View {
        let warnings: Vec<(String, View)> = self
            .sync_warnings
            .iter()
            .enumerate()
            .map(|(i, w)| {
                (
                    format!("warning{i}"),
                    TextBlock::new()
                        .text(format!("⚠︎ {w}"))
                        .text_wrapping(TextWrapping::Wrap)
                        .foreground(Brush::Solid(Color::rgb(0x9c, 0x6a, 0x00)))
                        .into(),
                )
            })
            .collect();
        let lists = self.sync_lists.iter().filter(|l| l.count > 0).map(|list| {
            let red = Color::rgb(0xc0, 0x1c, 0x28);
            let header = TextBlock::new()
                .text(format!("{} ({})", list.title, list.count))
                .font_size(13.0)
                .margin(Thickness::new(0.0, 6.0, 0.0, 2.0));
            let header = if list.warn {
                header.foreground(Brush::Solid(red))
            } else {
                header
            };
            let rows = list.commits.iter().map(|(oid, subject, author, time)| {
                let subject = TextBlock::new()
                    .text(subject.clone())
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .grid_column(1);
                let subject = if list.warn {
                    subject.foreground(Brush::Solid(red))
                } else {
                    subject
                };
                (
                    oid.clone(),
                    Grid::new()
                        .columns([
                            GridLength::Pixel(70.0),
                            GridLength::Star(1.0),
                            GridLength::Auto,
                        ])
                        .children((
                            TextBlock::new()
                                .text(oid[..oid.len().min(8)].to_owned())
                                .font_size(12.0)
                                .opacity(0.6)
                                .grid_column(0),
                            subject,
                            TextBlock::new()
                                .text(format!("{author} · {}", crate::stashes::ago(*time)))
                                .font_size(11.0)
                                .opacity(0.6)
                                .margin(Thickness::new(8.0, 0.0, 0.0, 0.0))
                                .grid_column(2),
                        )),
                )
            });
            let mut items: Vec<(String, View)> = rows.collect();
            let more = list.count.saturating_sub(list.commits.len() as u64);
            if more > 0 {
                items.push((
                    "more".to_owned(),
                    TextBlock::new()
                        .text(tr!("and {count} more", count = more))
                        .font_size(11.0)
                        .opacity(0.6)
                        .into(),
                ));
            }
            (
                list.title.clone(),
                StackPanel::new().children((
                    header,
                    ScrollViewer::new().max_height(8.0 * 22.0).content(
                        StackPanel::new()
                            .spacing(2.0)
                            .children((View::keyed_fragment(items),)),
                    ),
                )),
            )
        });
        let lists: Vec<(String, View)> = lists.collect();
        StackPanel::new()
            .width(460.0)
            .spacing(4.0)
            .children((View::keyed_fragment(warnings), View::keyed_fragment(lists)))
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
        match self.head_entry().cloned() {
            Some(entry) => self.push_branch(entry, force),
            None => beep(),
        }
    }

    /// ブランチ `entry` を上流へ送る（上流がなければ Publish）。チェックアウトしていないブランチもサイドバーから。
    fn push_branch(&mut self, entry: RefEntry, force: bool) {
        let Some(repo) = self.repo.clone() else {
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

    /// Commit and Push のあとの push（GTK4 版の `push_after_commit`）: 上流があって先に進んでいなければ、聞かずに
    /// 送る。上流がない・上流が先に進んでいるときは、Push と同じく聞く。
    fn push_committed(&mut self, context: &ComponentContext<Root>) {
        let (Some(repo), Some(entry)) = (self.repo.clone(), self.head_entry().cloned()) else {
            return beep();
        };
        let upstream = entry.detail.clone().filter(|_| !entry.gone);
        let Some((remote, remote_branch)) = upstream
            .as_deref()
            .filter(|_| entry.behind == 0 && entry.ahead > 0)
            .and_then(|u| u.split_once('/'))
        else {
            return self.push(false);
        };
        self.busy = Some("Push…".to_owned());
        perform(
            context,
            repo,
            json!({
                "op": "push", "remote": remote, "branch": entry.name,
                "remoteBranch": remote_branch, "force": false, "setUpstream": false,
            }),
            "Push".to_owned(),
        );
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
        let mut text = match &self.busy {
            Some(busy) => busy.clone(),
            None => self
                .refs
                .as_ref()
                .and_then(|r| r.refs.head_branch.clone())
                .unwrap_or_default(),
        };
        if let Some(note) = &self.status_note {
            text.push_str(&format!("  ·  {note}"));
        }
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
            } else if matches!(
                from,
                Page::Stashes | Page::FileHistory | Page::Blame | Page::Reflog | Page::RangeDiff
            ) {
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

    /// マージエディタのボタン（GTK4 版の `merge::Editor` の帯と同じ）。
    fn merge_button(&mut self, key: &str, context: &ComponentContext<Self>) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        let Some(state) = self.merge.as_mut() else {
            return;
        };
        match key {
            "accept-ours" => state.accept(amonhen_core::conflict::Choice::Ours),
            "accept-theirs" => state.accept(amonhen_core::conflict::Choice::Theirs),
            "accept-both" => state.accept(amonhen_core::conflict::Choice::Both),
            "magic" => state.magic(),
            "previous" => state.step(false),
            "next" => state.step(true),
            "save" => {
                state.saved = state.result.clone();
                let command = json!({
                    "op": "saveMerge", "path": state.path, "text": state.result, "stage": false,
                });
                perform(context, repo, command, tr!("Save"));
            }
            "resolve" => {
                let command = json!({
                    "op": "saveMerge", "path": state.path, "text": state.result, "stage": true,
                });
                let count = state.conflicts();
                if count == 0 {
                    // 保存して解決済みにしたら閉じる。
                    perform(context, repo, command, tr!("Save & Mark Resolved"));
                    self.merge = None;
                    self.page = Page::WorkingCopy;
                } else {
                    // コンフリクトが残っていれば確かめる（GTK4 版の `save_and_resolve`）。
                    state.saved = state.result.clone();
                    self.dialog = Some(Dialog::Confirm {
                        repo,
                        title: tr!(
                            "Mark {path} as resolved with {count} conflicts left?",
                            path = &state.path,
                            count = count
                        ),
                        message: tr!("The conflict markers stay in the file."),
                        button: tr!("Mark Resolved"),
                        command,
                    });
                }
            }
            "close" => {
                if state.dirty() {
                    self.dialog = Some(Dialog::CloseMerge);
                } else {
                    self.merge = None;
                    self.page = Page::WorkingCopy;
                }
            }
            _ => {}
        }
    }

    /// Repository Settings: `commands` を順に書いてから（失敗したらそこで止めて知らせる）、設定を読み直す。
    fn write_repo_settings(&self, commands: Vec<Value>, context: &ComponentContext<Self>) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        context.spawn_background(move |_| {
            for command in commands {
                let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                    .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                if let Some(e) = reply["error"].as_str() {
                    return Message::Performed(Some((tr!("Repository Settings"), e.to_owned())));
                }
            }
            let reply: Value =
                serde_json::from_str(&repo.run_json(r#"{"op":"configList"}"#)).unwrap_or_default();
            Message::RepoSettingsLoaded(crate::reposettings::entries_of(&reply["info"]))
        });
    }

    /// Repository Settings のフック・LFS と、`.gitignore`・`.git/info/exclude` を裏で読む。
    fn load_repo_files(&self, context: &ComponentContext<Self>) {
        // このリポジトリ専用のテンプレート。
        if let Some(repo) = self.repo.clone() {
            context.spawn_background(move |_| {
                let reply: Value =
                    serde_json::from_str(&repo.run_json(r#"{"op":"commitTemplate"}"#))
                        .unwrap_or_default();
                Message::RepoTemplateLoaded(
                    reply["info"]["text"].as_str().map(str::to_owned),
                    reply["info"]["path"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
            });
        }
        // 判定した既定のブランチ（`branchGroups` の base）。
        if let Some(repo) = self.repo.clone() {
            context.spawn_background(move |_| {
                let reply: Value = serde_json::from_str(&repo.run_json(r#"{"op":"branchGroups"}"#))
                    .unwrap_or_default();
                Message::RepoDetectedBase(reply["info"]["base"].as_str().map(str::to_owned))
            });
        }
        // リポジトリの形（shallow・partial・maintenance）も。
        if let Some(repo) = self.repo.clone() {
            context.spawn_background(move |_| {
                let reply: Value =
                    serde_json::from_str(&repo.run_json(r#"{"op":"repositoryForm"}"#))
                        .unwrap_or_default();
                Message::RepoFormLoaded(reply["info"].clone())
            });
        }
        // フックと LFS も。
        if let Some(repo) = self.repo.clone() {
            context.spawn_background(move |_| {
                let reply: Value = serde_json::from_str(&repo.run_json(r#"{"op":"hooksAndLfs"}"#))
                    .unwrap_or_default();
                Message::RepoHooksLoaded(reply["info"].clone())
            });
        }
        // .git/info/exclude も（読めなければ欄を出さない。空として書くと中身を失う）。
        if let Some(repo) = self.repo.clone() {
            context.spawn_background(move |_| {
                let files: Value = serde_json::from_str(&repo.run_json(r#"{"op":"ignoreFiles"}"#))
                    .unwrap_or_default();
                match (
                    files["info"]["gitignore"].as_str(),
                    files["info"]["exclude"].as_str(),
                ) {
                    (Some(g), Some(e)) => Message::RepoIgnoreLoaded(g.to_owned(), e.to_owned()),
                    _ => Message::Performed(Some((
                        tr!("Repository Settings"),
                        files["error"].as_str().unwrap_or("ignoreFiles").to_owned(),
                    ))),
                }
            });
        }
    }

    fn repo_settings_view(&self, context: &ViewContext<Self>) -> View {
        let (Page::RepoSettings, Some(state)) = (self.page, &self.repo_settings) else {
            return View::empty();
        };
        let view = crate::reposettings::view(
            state,
            crate::reposettings::Callbacks {
                field: Box::new(|f| context.callback(move |t| Message::RepoSettingsField(f, t))),
                button: Box::new(|k: String| {
                    context.callback(move |()| Message::RepoSettingsButton(k.clone()))
                }),
                choice: Box::new(|id: &'static str| {
                    context.callback(move |i: Option<usize>| Message::RepoSettingsChoice(id, i))
                }),
                toggle: Box::new(|name: String| {
                    context.callback(move |on| Message::HookEnabled(name.clone(), on))
                }),
            },
        );
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .grid_column_span(2)
            .content(view)
    }

    /// Activity の History のタブの中身（Undo できる操作）を裏で読む。
    fn load_undo_log(&self, context: &ComponentContext<Self>) {
        let (Some(repo), Some(1)) = (self.repo.clone(), self.activity.as_ref().map(|s| s.tab))
        else {
            return;
        };
        context.spawn_background(move |_| {
            let reply: Value =
                serde_json::from_str(&repo.run_json(r#"{"op":"undoLog"}"#)).unwrap_or_default();
            Message::ActivityUndoLog(
                reply["info"]["entries"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            )
        });
    }

    fn repositories_view(&self, context: &ViewContext<Self>) -> View {
        use crate::repositories::Event;
        let (Page::Repositories, Some(state)) = (self.page, &self.repositories) else {
            return View::empty();
        };
        let view = crate::repositories::view(
            state,
            crate::repositories::Callbacks {
                category: context.callback(|i| Message::Repos(Event::Category(i))),
                selected: context.callback(|i| Message::Repos(Event::Selected(i))),
                filter: context.callback(|t| Message::Repos(Event::Filter(t))),
                menu: Box::new(|| context.callback(|id| Message::Repos(Event::Menu(id)))),
                button: Box::new(|key| {
                    context.callback(move |()| {
                        Message::Repos(match key {
                            "add-repository" => Event::AddRepository,
                            "add-scan-folder" => Event::AddScanFolder,
                            _ => match key.strip_prefix("sort:") {
                                Some(column) => Event::Sort(column),
                                None => Event::Close,
                            },
                        })
                    })
                }),
                list_host: &self.repositories_list,
            },
        );
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .grid_column_span(2)
            .content(view)
    }

    fn activity_view(&self, context: &ViewContext<Self>) -> View {
        let (Page::Activity, Some(state)) = (self.page, &self.activity) else {
            return View::empty();
        };
        let view = crate::activity::view(
            state,
            crate::activity::Callbacks {
                tab: context.callback(Message::ActivityTab),
                undo: Box::new(|n| context.callback(move |()| Message::ActivityUndo(n))),
                close: context.callback(|()| Message::CloseActivity),
                export: context.callback(|()| Message::ActivityExport),
            },
        );
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .grid_column_span(2)
            .content(view)
    }

    fn settings_view(&self, context: &ViewContext<Self>) -> View {
        let (Page::Settings, Some(state)) = (self.page, &self.settings) else {
            return View::empty();
        };
        let view = crate::settingsview::view(
            state,
            crate::settingsview::Callbacks {
                field: Box::new(|i| context.callback(move |t| Message::SettingsField(i, t))),
                save_git: context.callback(|()| Message::SettingsSaveGit),
                toggle: Box::new(|key| context.callback(move |v| Message::SettingsToggle(key, v))),
                close: context.callback(|()| Message::CloseSettings),
                auto_fetch: context.callback(Message::SettingsAutoFetch),
                clear_cache: context.callback(|()| Message::ClearCache),
                editor: context.callback(Message::SettingsEditor),
            },
        );
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .grid_column_span(2)
            .content(view)
    }

    fn merge_view(&self, context: &ViewContext<Self>) -> View {
        let (Page::Merge, Some(state)) = (self.page, &self.merge) else {
            return View::empty();
        };
        let view = crate::merge::view(
            state,
            crate::merge::Callbacks {
                button: Box::new(|key| context.callback(move |()| Message::MergeButton(key))),
                result: context.callback(Message::MergeResult),
            },
        );
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .grid_column_span(2)
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

    /// worktree を消す前に、残っている変更を数えてから確かめる（GTK4 版の `confirm_remove`）。
    fn ask_remove_worktree(&mut self, path: String, context: &ComponentContext<Self>) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let locked = self.worktrees.iter().any(|w| w.path == path && w.locked);
        context.spawn_background(move |_| {
            let command = json!({ "op": "worktreeChanges", "paths": [&path], "fresh": true });
            let reply: Value =
                serde_json::from_str(&repo.run_json(&command.to_string())).unwrap_or_default();
            let changes = reply["info"]["worktrees"][0]["count"].as_u64().unwrap_or(0);
            let name = worktree_name(&path);
            let message = if changes > 0 {
                format!(
                    "{} {}",
                    tr!(
                        "{path} has uncommitted changes in {count} files. They are lost with the folder. The branch stays.",
                        path = &path,
                        count = changes
                    ),
                    tr!("This can’t be undone.")
                )
            } else {
                format!(
                    "{} {}",
                    tr!("Removes the folder {path}. The branch stays.", path = &path),
                    tr!("This can’t be undone.")
                )
            };
            Message::Confirm(
                repo,
                tr!("Remove the worktree “{name}”?", name = name),
                message,
                tr!("Remove Worktree"),
                json!({ "op": "worktreeRemove", "path": path, "force": changes > 0 || locked }),
            )
        });
    }

    /// Save as Bundle のダイアログを出す（`refs` が空なら HEAD とすべてのブランチ・タグ。`names` は見せる名前、
    /// `label` は既定のファイル名に使う）。
    fn ask_bundle(&mut self, refs: Vec<String>, names: String, label: String) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let base = repo
            .paths()
            .workdir
            .and_then(|w| w.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let label = archive_name(&if base.is_empty() {
            label
        } else {
            format!("{base}-{label}")
        });
        self.dialog = Some(Dialog::Bundle {
            repo,
            refs,
            what: if names.is_empty() {
                tr!("HEAD and all branches and tags")
            } else {
                names
            },
            label,
            newer: false,
            base: String::new(),
        });
    }

    /// Stash のダイアログを出す（`paths` があればそのファイルだけ。untracked のファイルを選んでいれば、最初から
    /// 含める。含めないと git が断る。GTK4 版と同じ）。
    fn ask_stash(&mut self, paths: Vec<String>) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        let untracked = self.status.as_ref().is_some_and(|s| {
            s.status
                .entries
                .iter()
                .any(|e| e.kind == amonhen_model::StatusKind::Untracked && paths.contains(&e.path))
        });
        self.dialog = Some(Dialog::Stash {
            repo,
            paths,
            message: String::new(),
            staged: false,
            keep_index: false,
            untracked,
        });
        self.dialog_inputs += 1;
    }

    /// `ranges`（git range-diff と同じ形）で比べる。`summary` は範囲の下に出す説明。
    fn open_ranges(
        &mut self,
        ranges: Vec<String>,
        summary: String,
        context: &ComponentContext<Self>,
    ) {
        let Some(repo) = self.repo.clone() else {
            return beep();
        };
        self.range_request += 1;
        let request = self.range_request;
        let state = self
            .range_diff
            .get_or_insert_with(crate::rangediff::State::new);
        state.ranges = ranges
            .iter()
            .map(|r| crate::rangediff::shorten(r))
            .collect::<Vec<_>>()
            .join(" ");
        state.summary = summary;
        state.pairs = None;
        state.error = None;
        state.selected = None;
        if self.page != Page::RangeDiff {
            self.show_page(Page::RangeDiff);
        }
        context.spawn_background(move |_| {
            let command = json!({ "op": "rangeDiff", "ranges": ranges });
            let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
            let pairs = match reply["error"].as_str() {
                Some(e) => Err(e.to_owned()),
                None => Ok(crate::rangediff::pairs_of(&reply["info"])),
            };
            Message::RangeDiffLoaded(request, pairs)
        });
    }

    fn range_diff_view(&self, context: &ViewContext<Self>) -> View {
        let (Page::RangeDiff, Some(state)) = (self.page, &self.range_diff) else {
            return View::empty();
        };
        let list = Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
            .content(crate::rangediff::view(
                state,
                crate::rangediff::Callbacks {
                    ranges: context.callback(Message::RangeDiffRanges),
                    compare: context.callback(|()| Message::RangeDiffCompare),
                    selected: context.callback(Message::RangeDiffRow),
                    close: context.callback(|()| Message::CloseRangeDiff),
                    changes_only: context.callback(Message::RangeDiffChangesOnly),
                },
            ));
        // 変わった組は、右の列に差分の差分を重ねる。
        let diff: View = match state.selected_pair().filter(|p| p.status == "changed") {
            Some(pair) => Border::new()
                .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
                .grid_column(2)
                .content(crate::rangediff::diff_view(pair)),
            None => View::empty(),
        };
        View::fragment((list, diff))
    }

    /// HEAD の reflog を裏で読む（GTK4 版と同じく 500 行まで）。
    fn load_reflog(&self, context: &ComponentContext<Self>) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        context.spawn_background(move |_| {
            let reply: Value =
                serde_json::from_str(&repo.run_json(r#"{"op":"reflog","limit":500}"#))
                    .unwrap_or_default();
            Message::ReflogLoaded(crate::reflog::entries_of(&reply["info"]))
        });
    }

    fn reflog_view(&self, context: &ViewContext<Self>) -> View {
        let (Page::Reflog, Some(entries)) = (self.page, &self.reflog) else {
            return View::empty();
        };
        let view = crate::reflog::view(
            entries,
            self.reflog_selected,
            &self.head_name(),
            crate::reflog::Callbacks {
                selected: context.callback(Message::ReflogSelected),
                menu: Box::new(|| context.callback(Message::HistoryMenu)),
                button: Box::new(|key: String| {
                    context.callback(move |()| Message::HistoryMenu(key.clone()))
                }),
            },
        );
        Border::new()
            .background(Brush::Solid(Color::rgb(0xff, 0xff, 0xff)))
            .grid_column(1)
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
            &self.stash_details,
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

    /// 欄の中身を、開いている worktree の下書きとして覚える（どちらも空なら消す。GTK4 版の `save_draft`）。
    fn store_draft(&self) {
        let Some(key) = &self.draft_key else {
            return;
        };
        let result = if self.summary.trim().is_empty() && self.description.trim().is_empty() {
            crate::prefs::remove(key)
        } else {
            crate::prefs::set_string(key, &json!([self.summary, self.description]).to_string())
        };
        if let Err(e) = result {
            eprintln!("下書きを覚えられない: {e}");
        }
    }

    /// 開いた worktree が替わったら、その下書きを入れる。なければ欄を空にして、テンプレート（`commit.template`）を
    /// 裏で読んで入れる（GTK4 版の `restore_draft_or_template`）。前の worktree の下書きは打つたびに覚えてある。
    fn switch_draft(&mut self, repo: &Arc<Repository>, context: &ComponentContext<Root>) {
        let Some(workdir) = repo.paths().workdir else {
            return;
        };
        let key = crate::prefs::draft_key(&workdir);
        if self.draft_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.draft_key = Some(key.clone());
        self.amend = false;
        // Settings > Performance: commit-graph がなければ書き出し、あれば増えた分を書き足す（開いた worktree が
        // 替わったとき。GTK4 版はリポジトリを開いたとき）。
        if crate::prefs::bool(crate::prefs::WRITE_COMMIT_GRAPH) {
            let repo = repo.clone();
            std::thread::spawn(move || {
                if let Err(e) = repo.update_commit_graph() {
                    eprintln!("commit-graph を書き出せない: {e}");
                }
            });
        }
        let draft = crate::prefs::string(&key)
            .and_then(|d| serde_json::from_str::<(String, String)>(&d).ok());
        if let Some((summary, description)) = draft {
            self.summary = summary;
            self.description = description;
            return;
        }
        self.summary.clear();
        self.description.clear();
        let repo = repo.clone();
        context.spawn_background(move |_| {
            Message::CommitTemplate(key, commit_template(&repo, &workdir).unwrap_or_default())
        });
    }

    /// 最近のメッセージ・「+」のメニューの項目を選んだ（GTK4 版の `MessageAids`）。
    fn message_menu(&mut self, key: &str, context: &ComponentContext<Root>) {
        if key == "commit-push" {
            self.push_after_commit = true;
            self.update(Message::CommitChanges, context);
        } else if key == "amend" {
            self.update(Message::Amend(true), context);
        } else if let Some(subject) = key.strip_prefix("fixup:") {
            // fixup! は件名だけ（元のメッセージを残す）、squash! は本文に書き足せる（GTK4 版と同じ）。
            self.summary = format!("fixup! {subject}");
            self.description.clear();
            self.amend = false;
        } else if let Some(subject) = key.strip_prefix("squash:") {
            self.summary = format!("squash! {subject}");
            self.amend = false;
        } else if let Some(t) = key.strip_prefix("type:") {
            // 件名の先頭を `type: ` にする（もう型があれば置き換える）。
            self.summary = format!(
                "{t}: {}",
                crate::workcopy::without_commit_type(&self.summary)
            );
        } else if let Some(line) = key.strip_prefix("trailer:") {
            if let Some(body) = crate::workcopy::with_trailer(&self.description, line) {
                self.description = body;
            }
        } else if key == "signoff" {
            let Some(repo) = self.repo.clone() else {
                return beep();
            };
            context.spawn_background(move |_| {
                let config: Value = serde_json::from_str(&repo.run_json(r#"{"op":"configList"}"#))
                    .unwrap_or_default();
                let value = |key: &str| {
                    config["info"]["entries"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .rev()
                        .find(|e| {
                            e["key"]
                                .as_str()
                                .is_some_and(|k| k.eq_ignore_ascii_case(key))
                        })
                        .and_then(|e| e["value"].as_str().map(str::to_owned))
                };
                match (value("user.name"), value("user.email")) {
                    (Some(name), Some(email)) => {
                        Message::CommitTextMenu(format!("trailer:Signed-off-by: {name} <{email}>"))
                    }
                    // user.name か user.email がなければ、音で知らせる（GTK4 版・macOS 版と同じ）。
                    _ => Message::CommitTextMenu("beep".to_owned()),
                }
            });
        } else if let Some(index) = key.strip_prefix("recent:") {
            if let Some((message, _, _)) = index
                .parse::<usize>()
                .ok()
                .and_then(|i| self.recent_messages.get(i))
            {
                let (s, b) = message.split_once('\n').unwrap_or((message, ""));
                self.summary = s.to_owned();
                self.description = b.trim().to_owned();
            }
        } else if key == "beep" {
            beep();
        }
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
            filter: context.callback(Message::WorkingFilter),
            layout: Box::new(|layout| context.callback(move |_| Message::WorkingLayout(layout))),
            stage_all: Box::new(|paths: Vec<String>, include: bool| {
                context.callback(move |_| Message::StageAll(paths.clone(), include))
            }),
            message_menu: context.callback(Message::CommitTextMenu),
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
                filter: &self.working_filter,
                sparse: self.form.sparse.as_deref(),
                layout: self.working_layout,
                collapsed: &self.working_collapsed,
                layout_clicks: self.layout_clicks,
                stats: &self.working_stats,
                list_host: &self.working_list,
                recent: &self.recent_messages,
                summary: &self.summary,
                description: &self.description,
                committing: self.committing,
                amend: self.amend,
                sequence: sequence
                    .as_ref()
                    .map(|(t, d, skip)| (t.as_str(), d.as_str(), *skip)),
                // 止まった git am: コンフリクトがなく、まだ 3-way でやり直していなければ（GTK4 版の `retry`）。
                retry_three_way: self.sequence.as_ref().is_some_and(|s| {
                    s["kind"] == "am"
                        && conflicts == 0
                        && s["threeWay"].as_bool() != Some(true)
                        && s["retried"].as_bool() != Some(true)
                        && s["empty"].as_bool() != Some(true)
                }),
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
        // 複数選んでいれば、まとめて使える操作だけ（GTK4 版の `commitops::multi_menu` のうち、この FE にあるもの）。
        if self.multi.len() > 1 {
            let n = self.multi.len();
            let ids: Vec<&str> = self.multi.iter().map(|(id, _)| id.as_str()).collect();
            // Cherry-pick は古い順に、Revert は新しい順に（GTK4 版の `menu_many` と同じ）。
            let oldest_first: Vec<&str> = ids.iter().rev().copied().collect();
            let mut items = vec![
                MenuItem::item(
                    format!("cherry-pick:{}", oldest_first.join(",")),
                    tr!("Cherry-pick {count} Commits…", count = n),
                ),
                MenuItem::item(
                    format!("revert:{}", ids.join(",")),
                    tr!("Revert {count} Commits…", count = n),
                ),
                MenuItem::separator("-pick"),
            ];
            // 2 つなら、それぞれを先端とする 2 つの版として比べられる（rebase の前と後など）。
            if let [newer, older] = &ids[..] {
                items.push(MenuItem::item(
                    format!("rangediff:{older}...{newer}"),
                    tr!("Compare as Two Versions (Range Diff)"),
                ));
                items.push(MenuItem::separator("-copy"));
            }
            items.push(MenuItem::item(
                format!("copy:{}", ids.join("\n")),
                tr!("Copy Commit IDs"),
            ));
            items.push(MenuItem::item(
                format!(
                    "copy:{}",
                    ids.iter()
                        .map(|id| &id[..id.len().min(8)])
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                tr!("Copy Short IDs"),
            ));
            return items;
        }
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
        // コミットの書き換え（GTK4 版の編集モードへの近道。マージには出さない）。
        let merge = self.info.as_ref().is_some_and(|(info, _)| {
            info["oid"].as_str() == Some(id.as_str())
                && info["parents"].as_array().is_some_and(|p| p.len() > 1)
        });
        if !merge {
            items.push(MenuItem::separator("-edit"));
            items.push(MenuItem::item(format!("reword:{id}"), tr!("Edit Message…")));
            items.push(MenuItem::item(
                format!("squash:{id}"),
                tr!("Combine with Older Commit…"),
            ));
            items.push(MenuItem::item(format!("drop:{id}"), "Drop…"));
            items.push(MenuItem::item(format!("split:{id}"), "Split…"));
        }
        // マージはパッチにできない（git format-patch は飛ばす）。
        items.push(MenuItem::separator("-patch"));
        if !merge {
            items.push(MenuItem::item(format!("patch:{id}"), tr!("Save as Patch…")));
        }
        // 既定の名前はタグ、なければローカルブランチ、なければ短い ID（GTK4 版の `archive_label`）。
        let label = labels
            .iter()
            .find(|l| l.kind == amonhen_model::RefKind::Tag)
            .or_else(|| {
                labels
                    .iter()
                    .find(|l| l.kind == amonhen_model::RefKind::Local)
            })
            .map_or_else(|| id[..8].to_owned(), |l| l.name.clone());
        items.push(MenuItem::item(
            format!("archive:{id}:{label}"),
            tr!("Export Archive…"),
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
/// 書き出せる形（コアの `archive` の `format`）。先頭が既定（GTK4 版と同じ）。
const ARCHIVE_FORMATS: [&str; 3] = ["zip", "tar.gz", "tar"];
/// 前の形を覚えておく鍵（GTK4 版と同じ）。
const ARCHIVE_FORMAT: &str = "archiveFormat";

/// ファイル名と先頭のフォルダに使える名前（`/` と空白などは `-` に。GTK4 版の `archive::file_name`）。
fn archive_name(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "archive".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// 当てるパッチの一覧（件名・作者、その下にファイルごとの印と増減。GTK4 版の `list_markup` の文字だけの形）。
fn patch_list(patches: &[Value]) -> String {
    let mut out = Vec::new();
    for patch in patches {
        let subject = patch["subject"].as_str().unwrap_or_default();
        let files = patch["files"].as_array().cloned().unwrap_or_default();
        if subject.is_empty() && files.is_empty() {
            continue;
        }
        if !subject.is_empty() {
            let mut line = subject.to_owned();
            if let Some(author) = patch["author"]
                .as_str()
                .or_else(|| patch["authorEmail"].as_str())
            {
                line.push_str(&format!("  ({author})"));
            }
            out.push(line);
        }
        for file in &files {
            let path = file["path"].as_str().unwrap_or_default();
            let name = match file["oldPath"].as_str() {
                Some(old) => format!("{old} → {path}"),
                None => path.to_owned(),
            };
            let change = if file["binary"].as_bool() == Some(true) {
                "bin".to_owned()
            } else {
                format!(
                    "+{} −{}",
                    file["added"].as_u64().unwrap_or(0),
                    file["removed"].as_u64().unwrap_or(0)
                )
            };
            let mark = match file["status"].as_str() {
                Some("added") => "A",
                Some("deleted") => "D",
                Some("renamed") => "R",
                Some("copied") => "C",
                _ => "M",
            };
            out.push(format!("  {mark} {name}  {change}"));
        }
    }
    out.join("\n")
}

/// git の答えの最初の `error:` の行（なければ最初の行。GTK4 版の `first_line`）。
fn patch_error_line(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines
        .iter()
        .find_map(|l| l.strip_prefix("error:").map(str::trim))
        .or_else(|| lines.first().copied())
        .unwrap_or_default()
        .to_owned()
}

/// worktree 1 つ（`worktrees` の答えの 1 つ）。
#[derive(Clone)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub main: bool,
}

impl WorktreeInfo {
    fn list(info: &Value) -> Vec<Self> {
        info["worktrees"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|w| w["bare"].as_bool() != Some(true))
            .map(|w| Self {
                path: w["path"].as_str().unwrap_or_default().to_owned(),
                branch: w["branch"].as_str().map(str::to_owned),
                locked: !w["locked"].is_null() && w["locked"] != false,
                prunable: !w["prunable"].is_null() && w["prunable"] != false,
                main: w["main"].as_bool() == Some(true),
            })
            .collect()
    }

    /// サイドバーの見出し（フォルダの名前とブランチ。ロック・なくなったフォルダは、細いサイドバーでも切れない
    /// よう前に印をつける）。
    /// サイドバーの行の見出し。`editing` は開いている worktree か、`changes` は変更の数（GTK4 版と同じく、
    /// ✎ 開いているもの・⌂ メイン・⑂ ほかの印と、` · n`）。
    fn title(&self, editing: bool, changes: u64) -> String {
        let mut text = String::new();
        text.push_str(if editing {
            "✎ "
        } else if self.main {
            "⌂ "
        } else {
            "⑂ "
        });
        if self.prunable {
            text.push_str("⚠ ");
        }
        if self.locked {
            text.push_str("🔒 ");
        }
        text.push_str(&worktree_name(&self.path));
        if let Some(branch) = &self.branch {
            text.push_str(&format!("  {branch}"));
        }
        if changes > 0 {
            text.push_str(&format!(" · {changes}"));
        }
        text
    }
}

/// `commit.template` の中身（# の行を除く）。このリポジトリ専用のもの（Settings で書いたもの）はコアから、
/// ほかは手元で読む。
fn commit_template(repo: &Repository, workdir: &std::path::Path) -> Option<String> {
    let config: Value =
        serde_json::from_str(&repo.run_json(r#"{"op":"configList"}"#)).unwrap_or_default();
    let path = config["info"]["entries"]
        .as_array()?
        .iter()
        .rev()
        .find(|e| {
            e["key"]
                .as_str()
                .is_some_and(|k| k.eq_ignore_ascii_case("commit.template"))
        })?["value"]
        .as_str()?
        .to_owned();
    if path.is_empty() {
        return None;
    }
    let text = if path
        .replace('\\', "/")
        .ends_with("/amonhen/commit-template")
    {
        let reply: Value =
            serde_json::from_str(&repo.run_json(r#"{"op":"commitTemplate"}"#)).ok()?;
        reply["info"]["text"].as_str()?.to_owned()
    } else {
        let file = match path.strip_prefix("~/") {
            Some(rest) => std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)?
                .join(rest),
            None if std::path::Path::new(&path).is_absolute() => path.into(),
            None => workdir.join(&path),
        };
        std::fs::read_to_string(file).ok()?
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let text = lines.join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// 最近のコミット 30 個のメッセージと作者を裏で読む（最近のメッセージと Co-authored-by の候補。GTK4 版と同じ）。
fn load_recent_messages(repo: Arc<Repository>, context: &ComponentContext<Root>) {
    context.spawn_background(move |_| {
        let recent = repo.history().map_or_else(Vec::new, |history| {
            history
                .graph
                .ids()
                .iter()
                .take(30)
                .filter_map(|id| {
                    let info = repo.commit_info(id).ok()?;
                    Some((
                        info["message"].as_str()?.trim().to_owned(),
                        info["author"].as_str().unwrap_or_default().to_owned(),
                        info["authorEmail"].as_str().unwrap_or_default().to_owned(),
                    ))
                })
                .collect()
        });
        Message::RecentMessages(recent)
    });
}

/// ブランチの分け方（`branchGroups`）を裏で読む。
fn load_groups(repo: Arc<Repository>, context: &ComponentContext<Root>) {
    context.spawn_background(move |_| {
        let reply = repo.run_json(r#"{"op":"branchGroups"}"#);
        let reply = serde_json::from_str(&reply).unwrap_or_default();
        Message::Groups(BranchGroups::from_reply(&reply))
    });
}

/// 同じパスか（区切りの向きと大文字小文字を区別しない。Windows のパス）。
fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(a) == norm(b)
}

/// タグのリモートでの様子（`tagRemotes`）と submodule の様子（`submodules`）を裏で読む。
fn load_tag_remotes(repo: Arc<Repository>, context: &ComponentContext<Root>) {
    let r = repo.clone();
    context.spawn_background(move |_| {
        let reply: Value =
            serde_json::from_str(&r.run_json(r#"{"op":"tagRemotes"}"#)).unwrap_or_default();
        Message::TagRemotes(reply["info"].clone())
    });
    let r = repo.clone();
    context.spawn_background(move |_| {
        let reply: Value =
            serde_json::from_str(&r.run_json(r#"{"op":"subtrees"}"#)).unwrap_or_default();
        Message::Subtrees(reply["info"].clone())
    });
    load_submodules(repo, context);
}

/// submodule の様子（`submodules`）を裏で読む。
fn load_submodules(repo: Arc<Repository>, context: &ComponentContext<Root>) {
    context.spawn_background(move |_| {
        let reply: Value =
            serde_json::from_str(&repo.run_json(r#"{"op":"submodules"}"#)).unwrap_or_default();
        Message::Submodules(reply["info"]["submodules"].clone())
    });
}

/// worktree の一覧を裏で読む。
fn load_worktrees(repo: Arc<Repository>, context: &ComponentContext<Root>) {
    context.spawn_background(move |_| {
        let reply: Value =
            serde_json::from_str(&repo.run_json(r#"{"op":"worktrees"}"#)).unwrap_or_default();
        Message::Worktrees(WorktreeInfo::list(&reply["info"]))
    });
}

/// worktree のフォルダの名前。
fn worktree_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_owned()
}

/// 新しい worktree のフォルダの既定: リポジトリの隣の「<名前>-<ブランチ>」（GTK4 版と同じ）。
fn default_worktree_folder(repo: &Repository, branch: &str) -> String {
    let Some(workdir) = repo.paths().workdir else {
        return String::new();
    };
    let name = workdir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = workdir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    parent
        .join(format!("{name}-{}", branch.replace('/', "-")))
        .to_string_lossy()
        .into_owned()
}

/// パッチの形と送り状を覚えておく鍵（GTK4 版と同じ）。
const PATCH_SINGLE_FILE: &str = "patchSingleFile";
const PATCH_COVER_LETTER: &str = "patchCoverLetter";

/// git format-patch と同じ考えのファイル名（英数字と . _ のほかは - にまとめる。GTK4 版の `file_name`）。
fn patch_file_name(subject: &str) -> String {
    let mut out = String::new();
    for c in subject.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "patches".to_owned()
    } else {
        trimmed.chars().take(52).collect()
    }
}

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
/// `pattern` を作業ツリーの直下の .gitignore の最後に足す（コアで読み書きするので、SSH 先でも同じ。GTK4 版の
/// `add_to_gitignore`）。
fn add_to_gitignore(context: &ComponentContext<Root>, repo: Arc<Repository>, pattern: String) {
    context.spawn_background(move |_| {
        let title = tr!("Add “{pattern}” to .gitignore", pattern = &pattern);
        let files: Value =
            serde_json::from_str(&repo.run_json(r#"{"op":"ignoreFiles"}"#)).unwrap_or_default();
        // 読めなければ書かない（空として書くと、.gitignore と info/exclude の中身を失う）。
        let (Some(current), Some(exclude)) = (
            files["info"]["gitignore"].as_str(),
            files["info"]["exclude"].as_str(),
        ) else {
            let e = files["error"]
                .as_str()
                .map_or_else(|| tr!("The operation failed"), str::to_owned);
            return Message::Performed(Some((title, e)));
        };
        let Some(gitignore) = crate::workcopy::with_ignore_pattern(current, &pattern) else {
            return Message::Performed(None);
        };
        let command =
            json!({ "op": "saveIgnoreFiles", "gitignore": gitignore, "exclude": exclude });
        let reply: Value = serde_json::from_str(&repo.run_json(&command.to_string()))
            .unwrap_or_else(|e| json!({ "error": e.to_string() }));
        Message::Performed(reply["error"].as_str().map(|e| (title, e.to_owned())))
    });
}

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
pub(crate) fn copy_text(text: &str) {
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
        // 自動の fetch（Settings > General で決めた間隔）。
        let (minutes, since) = AUTO_FETCH.with(|a| a.get());
        if minutes > 0 && since.elapsed() >= std::time::Duration::from_secs(u64::from(minutes) * 60)
        {
            AUTO_FETCH.with(|a| a.set((minutes, Instant::now())));
            self.sender.send(Message::AutoFetch);
        }
        // ほかの worktree の変更の数（10 秒ごと、5 分ごとに全部を数え直す。変わっていなければエンジンは git を
        // 動かさない）。
        let (counted, fresh) = WORKTREE_TICK.with(|t| t.get());
        if counted.elapsed() >= std::time::Duration::from_secs(10) {
            let all = fresh.elapsed() >= std::time::Duration::from_secs(300);
            WORKTREE_TICK
                .with(|t| t.set((Instant::now(), if all { Instant::now() } else { fresh })));
            self.sender.send(Message::CountWorktrees(all));
        }
        // Activity を開いている間は 0.5 秒ごとに読み直してもらう（GTK4 版と同じ間隔）。
        if let Some(last) = ACTIVITY_TICK.with(|t| t.get())
            && last.elapsed() >= std::time::Duration::from_millis(500)
        {
            ACTIVITY_TICK.with(|t| t.set(Some(Instant::now())));
            self.sender.send(Message::ActivityTick);
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
            // 右クリックのメニュー（2 つなら Range Diff で比べる）のために、選んだコミットを知らせる（上から）。
            let mut rows = self.multi_sent.clone();
            rows.sort_unstable();
            let ids: Vec<(String, bool)> = s.data.history.as_ref().map_or_else(Vec::new, |h| {
                rows.iter()
                    .filter_map(|&r| {
                        let id = h.graph.ids().get(r as usize)?.to_string();
                        let merge = h
                            .layout
                            .nodes()
                            .get(r as usize)
                            .is_some_and(|n| n.flags & amonhen_layout::Node::MERGE != 0);
                        Some((id, merge))
                    })
                    .collect()
            });
            self.sender.send(Message::MultiSelected(ids));
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

/// Working Copy の一覧のキー（GTK4 版・macOS 版と同じ）: Space で含める・外す、Delete・Backspace で捨てる。
/// ListView が先に使うキーなので、包んだ Grid の PreviewKeyDown で受ける。
fn attach_working_keys(element: &IInspectable) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    let revoker = ui.PreviewKeyDown(|_, args: Ref<xaml::KeyRoutedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        let discard = match args.Key() {
            Ok(xaml::VirtualKey::Space) => false,
            Ok(xaml::VirtualKey::Delete | xaml::VirtualKey::Back) => true,
            _ => return,
        };
        let _ = args.SetHandled(true);
        with_history(|h| {
            h.sender.send(Message::WorkingKey(discard));
        });
    })?;
    REVOKERS.with(|r| r.borrow_mut().push(revoker));
    Ok(())
}

/// Push の前の注意（GTK4 版の `push_warnings`）: 既定のブランチへ直接、整理し忘れのコミット、秘密らしきファイル・
/// バイナリの追加（`base`〜`tip` で足すファイルから。バイナリは先頭の 100 ファイルまで数える）。
fn push_warnings(
    repo: &Repository,
    upstream: &str,
    outgoing: &[(String, String, String, i64)],
    base: &amonhen_model::Oid,
    tip: &amonhen_model::Oid,
) -> Vec<String> {
    use amonhen_model::ChangeKind;
    let mut out = Vec::new();
    let remote_branch = upstream.split_once('/').map_or(upstream, |(_, b)| b);
    if ["main", "master", "trunk", "develop"].contains(&remote_branch) {
        out.push(tr!(
            "This pushes straight to {upstream}.",
            upstream = upstream
        ));
    }
    let unfinished = outgoing
        .iter()
        .filter(|(_, subject, _, _)| {
            let s = subject.to_lowercase();
            s.starts_with("fixup!")
                || s.starts_with("squash!")
                || s.starts_with("amend!")
                || s.starts_with("wip")
                || s.contains(" wip ")
                || s.ends_with(" wip")
        })
        .count();
    if unfinished > 0 {
        out.push(tr!(
            "{count} commits look unfinished (fixup!, squash! or WIP).",
            count = unfinished
        ));
    }
    let changes = repo.changes_between(Some(base), tip).unwrap_or_default();
    let secret_like: Vec<&str> = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Added)
        .map(|c| c.path.as_str())
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
            name == ".env"
                || name.starts_with(".env.")
                || [".pem", ".key", ".p12", ".pfx"]
                    .iter()
                    .any(|e| name.ends_with(e))
                || name.starts_with("id_rsa")
                || name.starts_with("id_ed25519")
                || name.contains("credential")
                || name.contains("secret")
        })
        .take(3)
        .collect();
    if !secret_like.is_empty() {
        out.push(tr!(
            "Adds files that may hold secrets: {files}",
            files = secret_like.join(", ")
        ));
    }
    let binaries = changes
        .iter()
        .take(100)
        .filter(|c| c.kind == ChangeKind::Added)
        .filter(|c| {
            repo.file_diff(c, &crate::data::PROFILE)
                .is_ok_and(|d| d.binary)
        })
        .count();
    if binaries > 0 {
        out.push(tr!("Adds {count} binary files.", count = binaries));
    }
    out
}

/// Pull・Push のダイアログのコミットの一覧の 1 つ（コアの `between` の答え）。
#[derive(Clone)]
struct SyncList {
    title: String,
    count: u64,
    /// (oid, 件名, 作者, 時刻)。新しい順。
    commits: Vec<(String, String, String, i64)>,
    /// 消えるもの（赤で出す）。
    warn: bool,
}

/// `Dialog::Form` の答えをどの命令にするか（GTK4 版の `submodulesheet`）。
#[derive(Clone)]
enum FormKind {
    AddSubmodule,
    UpdateSubmodules(Vec<String>),
    RunInEach,
    SetUrl(String),
    SetBranch(String),
    RemoveSubmodule(String),
    AddSubtree,
    /// subtree の場所と、Push か（でなければ Pull）。
    PullOrPushSubtree(String, bool),
    SplitSubtree(String),
}

impl FormKind {
    /// 欄とチェックの値からコアの命令（欄が足りなければ None）。
    fn command(
        &self,
        fields: &[(String, String, String)],
        checks: &[(String, bool)],
    ) -> Option<Value> {
        let field = |i: usize| fields.get(i).map_or("", |f| f.1.trim()).to_owned();
        let check = |i: usize| checks.get(i).is_some_and(|c| c.1);
        let optional = |t: String| (!t.is_empty()).then_some(t);
        Some(match self {
            FormKind::AddSubmodule => {
                let url = field(0);
                let path = optional(field(1)).unwrap_or_else(|| submodule_path(&url));
                if url.is_empty() || path.is_empty() {
                    return None;
                }
                json!({
                    "op": "changeSubmodule",
                    "change": { "kind": "add", "url": url, "path": path, "branch": optional(field(2)) },
                })
            }
            FormKind::UpdateSubmodules(paths) => json!({
                "op": "updateSubmodules",
                "paths": paths,
                "recursive": check(0),
                "remote": check(1),
            }),
            FormKind::RunInEach => {
                let command = optional(field(0))?;
                json!({
                    "op": "changeSubmodule",
                    "change": { "kind": "foreach", "command": command, "recursive": check(0) },
                })
            }
            FormKind::SetUrl(path) => json!({
                "op": "changeSubmodule",
                "change": { "kind": "setUrl", "path": path, "url": optional(field(0))? },
            }),
            FormKind::SetBranch(path) => json!({
                "op": "changeSubmodule",
                "change": { "kind": "setBranch", "path": path, "branch": optional(field(0)) },
            }),
            FormKind::AddSubtree => {
                let (url, rev) = (field(0), field(1));
                let dir = optional(field(2)).unwrap_or_else(|| submodule_path(&url));
                if url.is_empty() || rev.is_empty() || dir.is_empty() {
                    return None;
                }
                json!({
                    "op": "subtree",
                    "action": { "kind": "add", "dir": dir, "url": url, "ref": rev, "squash": check(0) },
                })
            }
            FormKind::PullOrPushSubtree(dir, push) => {
                let (url, rev) = (optional(field(0))?, optional(field(1))?);
                let mut action = json!({
                    "kind": if *push { "push" } else { "pull" },
                    "dir": dir,
                    "url": url,
                    "ref": rev,
                });
                if !*push {
                    action["squash"] = json!(check(0));
                }
                json!({ "op": "subtree", "action": action })
            }
            FormKind::SplitSubtree(dir) => {
                let name = dir.rsplit('/').next().unwrap_or(dir);
                let branch = optional(field(0)).unwrap_or_else(|| format!("{name}-split"));
                json!({
                    "op": "subtree",
                    "action": { "kind": "split", "dir": dir, "branch": branch },
                })
            }
            FormKind::RemoveSubmodule(path) => json!({
                "op": "changeSubmodule",
                "change": { "kind": "remove", "path": path, "deleteGitDir": check(0) },
            }),
        })
    }
}

/// URL の最後の部分から `.git` を除いたもの（git が submodule の場所の既定にするもの。GTK4 版の `default_path`）。
fn submodule_path(url: &str) -> String {
    let last = url
        .trim()
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default();
    last.strip_suffix(".git").unwrap_or(last).to_owned()
}

/// Clone の「最近の履歴だけ」で選べるコミットの数（GTK4 版の `shallow::more_choices` と 1）。
const CLONE_DEPTHS: [u32; 4] = [1, 100, 1_000, 10_000];

/// Repositories の一覧: ダブルクリックと Enter で、選んでいるリポジトリを開く（GTK4 版と同じ）。
fn attach_repositories_open(element: &IInspectable) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    let open = || {
        with_history(|h| {
            h.sender
                .send(Message::Repos(crate::repositories::Event::Open));
        });
    };
    let keys = ui.PreviewKeyDown(move |_, args: Ref<xaml::KeyRoutedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        if args.Key().ok() != Some(xaml::VirtualKey::Enter) {
            return;
        }
        let _ = args.SetHandled(true);
        open();
    })?;
    let taps = ui.DoubleTapped(move |_, _| open())?;
    REVOKERS.with(|r| r.borrow_mut().extend([keys, taps]));
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
