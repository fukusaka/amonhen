//! 状態と、イベントの処理と、画面の組み立て。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use amonhen_front::Front;
use amonhen_model::{Oid, tr};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_crossterm::crossterm::event::{
    Event as Input, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

use crate::Clock;
use crate::Result;
use crate::changes::Changes;
use crate::commands::{self, Place};
use crate::details::Details;
use crate::diff::{PAN, Theme};
use crate::fileview::FileView;
use crate::history::{History, WHEEL_ROWS};
use crate::loader::{Loaded, Loader, Request};
use crate::reflog::Reflog;
use crate::sidebar::{self, Picked, Sidebar};

pub enum Event {
    Input(Input),
    /// リポジトリが変わった（`amonhen_watch::Changes` のビット）。
    Changed(u32),
    /// git や ssh が、パスワードなどを尋ねてきた。
    #[cfg(unix)]
    Ask(crate::askpass::Question),
    Loaded(Loaded),
    /// 動いている検索の、ここまでに合った行と、調べ終えた割合（0〜1。内容の検索でなければ 0 のまま）。
    Partial {
        query: String,
        rows: Vec<u32>,
        done: f32,
    },
    Failed(String),
}

/// 描き直す所。一番上と一番下の行は、どれかを描き直すたびに描き直す。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Damage(u8);

impl Damage {
    const SIDEBAR: Self = Self(1);
    const MAIN: Self = Self(2);
    const SIDE: Self = Self(4);
    /// すべて（ペインの間の線と、重ねて出すものも）。
    pub const ALL: Self = Self(u8::MAX);

    fn has(self, pane: Self) -> bool {
        self.0 & pane.0 == pane.0
    }
}

impl std::ops::BitOr for Damage {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Damage {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// メインの画面。
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    /// Working Copy。
    Changes,
    History,
    Reflog,
    /// 1 つのファイルの Blame か File History（Working Copy の一覧から開く）。
    File,
}

impl View {
    fn picked(self) -> Picked {
        match self {
            View::Changes => Picked::Changes,
            View::History => Picked::History,
            View::Reflog => Picked::Reflog,
            // Working Copy から開く画面。
            View::File => Picked::Changes,
        }
    }
}

/// キーが届く所: メインの画面の一覧（History・変わったファイル）か、その横（詳細・差分）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Sidebar,
    Main,
    Side,
    /// コミットメッセージの欄（Working Copy の一覧の下）。
    Message,
}

/// 画面の割り方（一番上と一番下の行を除いた所）。
struct Layout {
    sidebar: Option<Rect>,
    main: Option<Rect>,
    side: Option<Rect>,
}

/// 変化の知らせのビット（`amonhen_watch::Changes`）。
const CHANGED_REFS: u32 = 1;
const CHANGED_INDEX: u32 = 2;
const CHANGED_WORKTREE: u32 = 4;

/// 一番下の行で打っている名前の使い道。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Naming {
    /// 新しいブランチ（始まりのコミットか ref に作って、切り替える）。
    Branch,
    /// 新しいタグ（コミットか ref に）。
    Tag,
    /// ブランチの新しい名前。
    Rename,
    /// stash のメッセージ（空でもよい）。
    Stash,
    /// stash を戻す新しいブランチ（stash を作ったところに作る。git stash branch）。
    StashBranch,
    /// ブランチを出す新しい worktree の場所。
    Worktree,
    /// push の送り先（`<remote>/<branch>`）。
    PushTo,
    /// 履歴のない新しいブランチの名前。
    Orphan,
    /// 足すリモートの名前（次に URL を聞く）。
    RemoteName,
    /// 足すリモートの URL（名前は聞いてある）。
    RemoteUrl,
    /// 選んでいるコミットを書き出すアーカイブのファイル（この機械の）。
    Archive,
    /// リポジトリをまとめる bundle のファイル（この機械の）。
    Bundle,
    /// 取り込む bundle のファイル（この機械の）。
    BundleFile,
    /// bundle のブランチを取り込む名前（`refs/remotes/<名前>/*`。ファイルは聞いてある）。
    BundleName,
    /// 当てるパッチのファイル（この機械の）。
    Patch,
    /// 選んでいるコミットのパッチを書くフォルダ（この機械の）。
    SavePatch,
}

/// 選択肢 1 つ: 押すキー、名前、頼む中身。
type Choice = (char, String, Request);

/// コマンドの一覧（`:`）で打っている語と、選んでいる行。
#[derive(Default)]
struct Palette {
    query: String,
    selected: usize,
}

/// 操作ログ（アプリの Activity の Operation Log）: Undo の記録を新しい順に。選んだところまで戻せる。
struct OperationLog {
    /// 名前・時刻（UNIX 時刻の秒）。
    entries: Vec<(String, i64)>,
    selected: usize,
}

pub struct App {
    loader: Loader,
    history: History,
    details: Details,
    changes: Changes,
    sidebar: Sidebar,
    /// サイドバーで開いた stash（詳細の枠に、History の選択の代わりに出す）。
    pinned: Option<Oid>,
    reflog: Reflog,
    file: FileView,
    /// ファイルの画面を閉じたときに戻る画面。
    file_back: View,
    view: View,
    focus: Focus,
    /// リポジトリ・ref・History を読んでいる。
    opening: bool,
    /// 読んでいる間に、また ref が変わった。
    reopen: bool,
    /// 確かめてから行う操作: 尋ねる文と、`y` で頼む中身。
    confirm: Option<(String, String, Request)>,
    /// push を確かめている: `t` で、送り先のブランチを打てる。
    push_to_offered: bool,
    /// 読んでいる bundle のファイル（取り込む名前を聞く間、覚えておく）。
    bundle_file: Option<String>,
    /// 読んでいるパッチのファイル（当てるかを聞く間、覚えておく）。
    patch_file: Option<String>,
    /// 前の版と比べている間の題（比べた結果が届くまで覚えておく）。
    range_title: Option<String>,
    /// merge か rebase を確かめる前に、見通しを読んでいる: 何を（ref の名前）、どのブランチに、merge か。merge は
    /// 見通し（`mergePreview`）を読んだ後に、コンフリクトの見込みも読む。
    preview: Option<(String, String, bool, Value)>,
    /// reset する先（見通しを読んでいる間、覚えておく）。
    reset_to: Option<String>,
    /// History でマージ済みのブランチを折りたたんでいる（アプリの Fold Merged Branches。この TUI の間だけ）。
    fold: bool,
    /// 折りたたんでいる間に、取り込んだ側を開いたマージ（16 進の ID）。
    expanded: Vec<String>,
    /// History の並び順を、トポロジー順（ブランチのコミットをそのマージの下にまとめる）にしている。この TUI の
    /// 間だけ（アプリの設定には書かない）。
    topo_order: bool,
    /// 作業ツリーのファイルがこの機械にある（`ssh://` の先でない）。エディタで開けるか。
    files_here: bool,
    /// エディタで開く作業ツリーのファイル（絶対パス）。
    open_file: Option<String>,
    /// 開き直す worktree（サイドバーの Enter。TUI を閉じて、そこで開き直す）。
    open_worktree: Option<String>,
    /// いくつかから選んで行う操作: 尋ねる文と、キー・名前・頼む中身の組。
    choices: Option<(String, Vec<Choice>)>,
    /// マウスを取り込んでいる（切ると、端末で文字を選べる）。
    mouse: bool,
    /// マウスの取り込みを替えた（端末に伝えるまで）。
    mouse_changed: bool,
    /// `Ctrl-z` で止める（端末を戻してから）。
    #[cfg(unix)]
    suspend: bool,
    /// `Ctrl-l` で、端末に出ているものを当てにせずにすべて書き直す。
    redraw: bool,
    /// 新しいブランチの名前を打っている: 始まり（ref かコミット）、その出し方、打った名前。
    typing_branch: Option<(String, String, String)>,
    /// 打っている名前が何のものか。
    naming: Naming,
    /// git や ssh から尋ねられていること（先頭を画面で聞いている）と、打った答え。
    #[cfg(unix)]
    questions: std::collections::VecDeque<crate::askpass::Question>,
    #[cfg(unix)]
    answer: String,
    /// 動いている操作（fetch・push など）の、一番上の行に出す名前。
    running: Option<String>,
    /// 今のブランチの上流（`origin/main`）と、ahead・behind。
    upstream: Option<String>,
    ahead: u64,
    behind: u64,
    /// 途中の操作（merge・rebase・cherry-pick・revert）: 種類と、何番目・全部でいくつ（rebase）。
    sequence: Option<(String, Option<(u64, u64)>)>,
    /// 一番新しい stash の名前（`stash@{0}`）。
    stash: Option<String>,
    /// リモートの名前（上流のないブランチを publish する先）。
    remotes: Vec<String>,
    /// HEAD のコミット（amend で、そのメッセージを欄に入れる）。
    head: Option<String>,
    /// コミットを作っている。
    committing: bool,
    /// コミットができたら push する（Commit and Push。状態を読み直してから、push を確かめる）。
    push_after_commit: bool,
    /// 済んだことの知らせ（次のキーまで出す）。
    notice: Option<String>,
    /// 検索の語を打っている（`/`）。
    typing_query: Option<String>,
    /// サイドバーの絞り込みの語を打っている（サイドバーで `/`）。
    typing_filter: Option<String>,
    /// キーの一覧を出している（`?`）。
    help: bool,
    /// キーの一覧の、一番上に見えている行（収まらないときに動かす）。
    help_top: usize,
    /// コマンドの一覧を出している（`:`）。
    palette: Option<Palette>,
    operation_log: Option<OperationLog>,
    /// History の編集モード（`E`）。
    edit_mode: Option<crate::editmode::EditMode>,
    /// 編集モードで、変更があるのに Esc を 1 度押した（もう 1 度で捨てる）。
    edit_discard: bool,
    /// 編集モードの行のメッセージを、エディタで直す（開く中身。開いたら `editing_plan` が立つ）。
    plan_message: Option<String>,
    editing_plan: bool,
    /// 確かめている間に重ねる、前と後の絵（枠の題と、`drawing` の形の絵。編集モードの `editDrawing`、reset の
    /// `movePreview` から作ったもの）。答えたら消える。
    question_drawing: Option<(String, Value)>,
    /// cherry-pick・revert の確かめる文と頼むこと（前と後の絵を読んでから確かめる）。
    pick_question: Option<(String, String, Request, String)>,
    /// 今の検索: 語と、動いている検索を取り消す印（結果が届いたら `None`）。
    search: Option<(String, Option<Arc<AtomicBool>>)>,
    /// 動いている検索の、調べ終えた割合（0〜1。内容の検索でなければ 0 のまま）。
    search_done: f32,
    theme: Theme,
    /// リポジトリの名前（作業ツリーのフォルダ名）。
    name: String,
    /// 作業ツリーの場所。
    workdir: Option<String>,
    /// メッセージをエディタで開く（`Ctrl-e`。端末を渡してから）。
    edit_message: bool,
    /// メッセージを書き換えるコミット（16 進の ID）と、届いたら今のメッセージ（届くまで `None`）。届いたら
    /// エディタで開き、閉じたら書き換える。
    rewording: Option<(String, Option<String>)>,
    branch: Option<String>,
    clock: Clock,
    size: (u16, u16),
    /// ワーカーに頼んで、まだ答えが来ていない数。
    pending: usize,
    error: Option<String>,
    /// すべてを描き直す。
    dirty: bool,
    /// 描き直すペイン（そのペインの中だけが変わったとき。ほかは `dirty`）。
    damage: Damage,
    /// メインの画面が最後に置いたカーソル（メインを描き直さないフレームで使う）。
    main_cursor: Option<(u16, u16)>,
    quit: bool,
}

/// 一番上の行（リポジトリとブランチ）と一番下の行（案内）を除いた所に、History と詳細を出す。
const BARS: u16 = 2;
/// これ以上の幅なら、History の右に詳細を並べる。狭ければ、詳細は History の所に開く（Enter）。
const SIDE_BY_SIDE: u16 = 116;
/// これ以上の幅なら、左にサイドバーを出す。狭ければ、呼んだとき（`0`）だけメインの画面の所に出す。
const SIDEBAR_FROM: u16 = 100;
/// 並べたときの History の幅は全体の 5 分の 2（少なくともこの幅）。
const HISTORY_MIN: u16 = 60;

impl App {
    pub fn new(front: Arc<dyn Front>, events: Sender<Event>, clock: Clock) -> Self {
        Self {
            loader: Loader::start(front, events),
            history: History::default(),
            details: Details::default(),
            changes: Changes::default(),
            sidebar: Sidebar::default(),
            pinned: None,
            reflog: Reflog::default(),
            file: FileView::default(),
            file_back: View::Changes,
            view: View::History,
            focus: Focus::Main,
            opening: false,
            reopen: false,
            confirm: None,
            choices: None,
            mouse: true,
            mouse_changed: false,
            #[cfg(unix)]
            suspend: false,
            redraw: false,
            typing_branch: None,
            naming: Naming::Branch,
            #[cfg(unix)]
            questions: std::collections::VecDeque::new(),
            #[cfg(unix)]
            answer: String::new(),
            running: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            sequence: None,
            stash: None,
            remotes: Vec::new(),
            head: None,
            committing: false,
            push_after_commit: false,
            notice: None,
            typing_query: None,
            typing_filter: None,
            help: false,
            help_top: 0,
            palette: None,
            operation_log: None,
            edit_mode: None,
            edit_discard: false,
            plan_message: None,
            editing_plan: false,
            question_drawing: None,
            pick_question: None,
            push_to_offered: false,
            bundle_file: None,
            patch_file: None,
            range_title: None,
            topo_order: false,
            fold: false,
            preview: None,
            reset_to: None,
            expanded: Vec::new(),
            files_here: true,
            open_file: None,
            open_worktree: None,
            search: None,
            search_done: 0.0,
            theme: Theme::detect(),
            name: String::new(),
            workdir: None,
            edit_message: false,
            rewording: None,
            branch: None,
            clock,
            size: (0, 0),
            pending: 0,
            error: None,
            dirty: true,
            damage: Damage::default(),
            main_cursor: None,
            quit: false,
        }
    }

    /// リポジトリを読み始める。
    pub fn start(&mut self) {
        self.open();
        self.changes.refresh();
        self.request();
    }

    /// リポジトリ・ref・History を読む。読んでいる間に頼まれた分は、終わってからもう一度。
    fn open(&mut self) {
        if self.opening {
            self.reopen = true;
            return;
        }
        self.opening = true;
        // HEAD が動いたかもしれない。
        self.reflog.refresh();
        self.send(Request::Open {
            fold: self.fold,
            expanded: self.expanded.clone(),
        });
    }

    fn send(&mut self, request: Request) {
        self.pending += 1;
        self.loader.send(request);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = (cols, rows);
        self.history.set_height(rows.saturating_sub(BARS));
        self.changes.set_height(rows.saturating_sub(BARS));
        self.sidebar.set_height(rows.saturating_sub(BARS));
        self.reflog.set_height(rows.saturating_sub(BARS));
        self.file.set_height(rows.saturating_sub(BARS));
        // 詳細の幅は、並べても開いても、キーの届く所によらない。
        let layout = self.layout();
        let details = layout.side.or(layout.main).unwrap_or_default();
        self.details
            .set_size(details.width, details.height, &self.clock);
        self.dirty = true;
    }

    fn layout(&self) -> Layout {
        let (cols, rows) = self.size;
        let height = rows.saturating_sub(BARS);
        // 狭ければ、サイドバーは呼んだときだけ、メインの画面の所に出す。
        if cols < SIDEBAR_FROM && self.focus == Focus::Sidebar {
            return Layout {
                sidebar: Some(Rect::new(0, 1, cols, height)),
                main: None,
                side: None,
            };
        }
        let (sidebar, x) = match cols >= SIDEBAR_FROM {
            // サイドバーの右に、区切りの 1 列。
            true => (
                Some(Rect::new(0, 1, sidebar::WIDTH, height)),
                sidebar::WIDTH + 1,
            ),
            false => (None, 0),
        };
        let rest = cols - x;
        if rest >= SIDE_BY_SIDE {
            let left = (rest / 5 * 2).max(HISTORY_MIN);
            // 間に区切りの 1 列。
            return Layout {
                sidebar,
                main: Some(Rect::new(x, 1, left, height)),
                side: Some(Rect::new(x + left + 1, 1, rest - left - 1, height)),
            };
        }
        let all = Some(Rect::new(x, 1, rest, height));
        match self.focus {
            Focus::Sidebar | Focus::Main | Focus::Message => Layout {
                sidebar,
                main: all,
                side: None,
            },
            Focus::Side => Layout {
                sidebar,
                main: None,
                side: all,
            },
        }
    }

    /// 描き直す所があれば、その所（聞いたら、なかったことにする）。キーの一覧とコマンドの一覧はペインに重なる
    /// ので、出している間はすべてを描き直す。
    pub fn take_damage(&mut self) -> Option<Damage> {
        let damage = std::mem::take(&mut self.damage);
        let over = self.help
            || self.palette.is_some()
            || self.operation_log.is_some()
            || self.edit_mode.is_some();
        if std::mem::take(&mut self.dirty) || (over && damage != Damage::default()) {
            return Some(Damage::ALL);
        }
        (damage != Damage::default()).then_some(damage)
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// `damage` で描き直すセルの範囲（`draw` はこの中だけに描く）。
    pub fn areas(&self, damage: Damage) -> Vec<Rect> {
        let (cols, rows) = self.size;
        if damage == Damage::ALL || rows < BARS + 1 {
            return vec![Rect::new(0, 0, cols, rows)];
        }
        let layout = self.layout();
        let mut areas = vec![Rect::new(0, 0, cols, 1), Rect::new(0, rows - 1, cols, 1)];
        for (pane, area) in [
            (Damage::SIDEBAR, layout.sidebar),
            (Damage::MAIN, layout.main),
            (Damage::SIDE, layout.side),
        ] {
            if damage.has(pane) {
                areas.extend(area);
            }
        }
        areas
    }

    /// `damage` で描き直すペインのうち、最後に描いてから中身が縦にずれたものと、その行数（正なら上へ）。
    /// フレームの出力が、端末のスクロールに使う。当てにならなくてもよい（中身が替わっていても、違うセルは書く）。
    pub fn scrolls(&self, damage: Damage) -> Vec<(Rect, i32)> {
        let layout = self.layout();
        let main = match self.view {
            View::History => self.history.scrolled(),
            _ => 0,
        };
        let side = match self.view {
            View::Changes => self.changes.diff_scrolled(),
            _ => self.details.scrolled(),
        };
        [
            (Damage::MAIN, layout.main, main),
            (Damage::SIDE, layout.side, side),
        ]
        .into_iter()
        .filter(|&(pane, _, lines)| damage.has(pane) && lines != 0)
        .filter_map(|(_, area, lines)| Some((area?, lines)))
        .collect()
    }

    pub fn quit(&self) -> bool {
        self.quit
    }

    pub fn mouse_captured(&self) -> bool {
        self.mouse
    }

    /// マウスの取り込みを替えたなら、その値（聞いたら、なかったことにする）。
    pub fn take_mouse_change(&mut self) -> Option<bool> {
        std::mem::take(&mut self.mouse_changed).then_some(self.mouse)
    }

    /// `Ctrl-z` を押した（聞いたら、なかったことにする）。止まって続いた後は、描き直す。
    #[cfg(unix)]
    pub fn take_suspend(&mut self) -> bool {
        let suspend = std::mem::take(&mut self.suspend);
        self.dirty |= suspend;
        suspend
    }

    /// `Ctrl-l` を押した（聞いたら、なかったことにする）。
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// メッセージをエディタで開くなら、今のメッセージと作業ツリーの場所（聞いたら、なかったことにする）。
    pub fn take_edit_message(&mut self) -> Option<(String, Option<String>)> {
        // 編集モードの行のメッセージ。
        if let Some(text) = self.plan_message.take() {
            self.editing_plan = true;
            return Some((text, self.workdir.clone()));
        }
        // コミットのメッセージの書き換えは、今のメッセージが届いてから。
        if let Some((_, Some(message))) = &self.rewording
            && !self.edit_message
        {
            self.edit_message = true;
            return Some((message.clone(), self.workdir.clone()));
        }
        if self.rewording.is_some() {
            return None;
        }
        std::mem::take(&mut self.edit_message)
            .then(|| (self.changes.message.text(), self.workdir.clone()))
    }

    /// 作業ツリーのファイルがこの機械にあるか（`ssh://` の先なら false）。
    pub fn set_files_here(&mut self, here: bool) {
        self.files_here = here;
    }

    /// 開き直す worktree のパス（選んだときだけ。そのとき `quit` も立っている）。
    pub fn take_open_worktree(&mut self) -> Option<String> {
        self.open_worktree.take()
    }

    /// エディタで開くファイルと、作業ツリーの場所（聞いたら、なかったことにする）。
    pub fn take_open_file(&mut self) -> Option<(String, Option<String>)> {
        let file = self.open_file.take()?;
        Some((file, self.workdir.clone()))
    }

    /// エディタを閉じた。だめなら理由を出す。中身が変わったかもしれないので、状態を読み直す。
    pub fn file_opened(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.error = Some(error.to_string());
        }
        self.changes.refresh();
        self.dirty = true;
    }

    /// エディタを閉じた後のメッセージ。だめなら理由を出し、メッセージはそのまま。
    pub fn message_edited(&mut self, edited: Result<String>) {
        self.edit_message = false;
        // 編集モードの行のメッセージ（当てるまで、何も変わらない）。
        if std::mem::take(&mut self.editing_plan) {
            self.dirty = true;
            match edited {
                Ok(text) => {
                    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
                    let message = lines.join("\n").trim().to_owned();
                    match (message.is_empty(), self.edit_mode.as_mut()) {
                        (true, _) => {
                            self.error = Some(tr!("The message is empty; it is unchanged"))
                        }
                        (false, Some(mode)) => mode.set_message(message),
                        (false, None) => {}
                    }
                }
                Err(error) => self.error = Some(error.to_string()),
            }
            return;
        }
        // コミットのメッセージを書き換えていたなら、変わっていれば書き換える。
        if let Some((oid, before)) = self.rewording.take() {
            self.dirty = true;
            match edited {
                Ok(text) => {
                    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
                    let message = lines.join("\n").trim().to_owned();
                    if message.is_empty() {
                        self.error = Some(tr!("The message is empty; the commit is unchanged"));
                    } else if Some(message.as_str()) == before.as_deref().map(str::trim) {
                        self.notice = Some(tr!("The message is unchanged"));
                    } else {
                        self.run(
                            "rewrite",
                            tr!("Editing the message…"),
                            json!({
                                "op": "rewrite", "label": "Edit Message",
                                "edit": {"kind": "reword", "oid": oid, "message": message},
                            }),
                        );
                    }
                }
                Err(error) => self.error = Some(error.to_string()),
            }
            return;
        }
        match edited {
            // `#` で始まる行は、git と同じくコメント。
            Ok(text) => {
                let text: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
                self.changes.message.set_text(&text.join("\n"));
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        self.dirty = true;
    }

    /// まだ読んでいる。
    pub fn busy(&self) -> bool {
        self.pending > 0
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// メインの画面の中の 1 列（計測で、そこにホイールを送る）。
    /// 前のフレームで組み立てたものを捨てる（次は、すべてを組み立て直す）。
    pub fn forget_drawn(&mut self) {
        self.history.forget_drawn();
        self.changes.forget_drawn();
        self.details.forget_drawn();
    }

    pub fn main_column(&self) -> u16 {
        self.layout().main.map_or(0, |at| at.x + 1)
    }

    pub fn history_rows(&self) -> u32 {
        self.history.total()
    }

    /// 計測の `step` を、`down` の向きにはもう進められない。
    pub fn at_end(&self, down: bool, step: crate::Step) -> bool {
        match step {
            crate::Step::Wheel => self.history.at_end(down),
            crate::Step::Move => {
                let last = self.history.total().saturating_sub(1);
                self.history.selected() == if down { last } else { 0 }
            }
            crate::Step::Diff => self.details.at_end(down),
            crate::Step::Changes => self.changes.at_end(down),
            // 回数で折り返す。
            crate::Step::DiffPan | crate::Step::ChangesPan => false,
        }
    }

    /// 詳細に出ている行の数（差分を含む）。
    pub fn details_lines(&self, step: crate::Step) -> u32 {
        match step {
            crate::Step::Changes | crate::Step::ChangesPan => self.changes.diff_position().1,
            _ => self.details.position().1,
        }
    }

    pub fn last_frame_incomplete(&self) -> bool {
        self.history.incomplete()
    }

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Input(Input::Key(key)) => self.key(key),
            Event::Input(Input::Mouse(mouse)) => self.mouse(mouse),
            Event::Input(Input::Resize(cols, rows)) => self.resize(cols, rows),
            Event::Input(_) => {}
            #[cfg(unix)]
            Event::Ask(question) => {
                self.questions.push_back(question);
                self.dirty = true;
            }
            Event::Changed(bits) => {
                if bits & (CHANGED_INDEX | CHANGED_WORKTREE) != 0 {
                    self.changes.refresh();
                }
                if bits & CHANGED_REFS != 0 {
                    self.open();
                }
            }
            Event::Loaded(loaded) => {
                self.pending = self.pending.saturating_sub(1);
                // 行と祖先は History の中だけ、コミットと構文の色は詳細・差分の中だけが変わる。
                match &loaded {
                    Loaded::Rows { .. } | Loaded::Lineage { .. } => self.damage |= Damage::MAIN,
                    Loaded::Commit { .. } | Loaded::Syntax { .. } => self.damage |= Damage::SIDE,
                    _ => self.dirty = true,
                }
                match loaded {
                    Loaded::Opened {
                        workdir,
                        refs,
                        history,
                    } => {
                        self.workdir = workdir.clone();
                        self.name = workdir
                            .as_deref()
                            .and_then(|w| std::path::Path::new(w).file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.branch = refs["headBranch"].as_str().map(str::to_owned);
                        self.head = refs["head"].as_str().map(str::to_owned);
                        // 読み直したら、選んでいたコミットを選び直す。
                        let selected = self.history.selected_id();
                        self.history.opened(&refs, &history);
                        self.sidebar.set_refs(&refs);
                        self.remotes = refs["remotes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|r| r["name"].as_str().or(r.as_str()))
                            .map(str::to_owned)
                            .collect();
                        self.stash = refs["entries"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|e| e["kind"] == "stash")
                            .and_then(|e| e["name"].as_str())
                            .map(str::to_owned);
                        // 検索していたら、新しい History で検索し直す。
                        if let Some((query, _)) = self.search.clone() {
                            self.start_search(query);
                        }
                        if let Some(id) = selected {
                            self.send(Request::RowOf {
                                history: self.history.id().clone(),
                                id: id.to_string(),
                            });
                        }
                        self.opening = false;
                        if std::mem::take(&mut self.reopen) {
                            self.open();
                        }
                    }
                    Loaded::Rows { start, reply } => self.history.loaded(start, &reply),
                    Loaded::Search { query, result } => {
                        // 取り消した検索（語を替えた・やめた）の答えは捨てる。
                        if let Some((current, running)) = &mut self.search
                            && *current == query
                            && running.take().is_some()
                        {
                            match result {
                                Ok(rows) => self.history.set_matches(rows),
                                Err(error) => {
                                    self.search = None;
                                    self.error = Some(error);
                                }
                            }
                        }
                    }
                    Loaded::File {
                        path,
                        blame,
                        result,
                    } => {
                        if let Some(error) = self.file.set(&path, blame, result) {
                            self.error = Some(error);
                        }
                    }
                    Loaded::Reflog(name, reply) => {
                        if let Some(error) = self.reflog.set(name, reply) {
                            self.error = Some(error);
                        }
                    }
                    Loaded::Lineage { row, bits } => self.history.set_lineage(row, bits),
                    Loaded::RowOf(row) => {
                        if let Some(row) = row {
                            self.history.select(row);
                        }
                    }
                    Loaded::Commit { id, result } => {
                        if let Some(request) = self.details.loaded(&id, result, &self.clock) {
                            self.send(request);
                        }
                    }
                    Loaded::Syntax { id, reply } => {
                        if let Some(reply) = &reply {
                            self.changes.syntax(&id, reply);
                        }
                        self.details.syntax(&id, reply.as_ref());
                    }
                    Loaded::Status(Ok(status)) => {
                        self.upstream = status["upstream"].as_str().map(str::to_owned);
                        self.ahead = status["ahead"].as_u64().unwrap_or(0);
                        self.behind = status["behind"].as_u64().unwrap_or(0);
                        let sequence = &status["sequence"];
                        self.sequence = sequence["kind"].as_str().map(|kind| {
                            let progress =
                                sequence["current"].as_u64().zip(sequence["total"].as_u64());
                            (kind.to_owned(), progress)
                        });
                        self.changes.status(&status);
                        // Commit and Push: コミットの後の状態（上流より進んだ数）が届いたら、push を確かめる。
                        if !self.committing && std::mem::take(&mut self.push_after_commit) {
                            self.ask_push();
                        }
                    }
                    Loaded::Ran { name, result } => self.ran(name, result),
                    Loaded::Status(Err(error)) => {
                        self.changes.status_failed();
                        self.error = Some(error);
                    }
                    Loaded::WorkingDiff {
                        path,
                        serial,
                        result,
                    } => {
                        if let Some(request) = self.changes.working_diff(path, serial, result) {
                            self.send(request);
                        }
                    }
                    Loaded::Committed(result) => {
                        self.committing = false;
                        match result {
                            Ok(output) => {
                                self.changes.message.clear();
                                self.changes.amend = false;
                                self.focus = Focus::Main;
                                // hooks が何か書いていれば、その 1 行目を出す。
                                self.notice =
                                    Some(match output.lines().find(|l| !l.trim().is_empty()) {
                                        Some(line) => {
                                            tr!("Committed: {output}", output = line.trim())
                                        }
                                        None => tr!("Committed"),
                                    });
                                self.changes.refresh();
                                self.open();
                            }
                            Err(error) => {
                                self.push_after_commit = false;
                                self.error = Some(error);
                            }
                        }
                    }
                    Loaded::Message(message) if self.rewording.is_some() => {
                        // 書き換えるコミットの今のメッセージ: エディタで開く（`take_edit_message`）。
                        match message {
                            Some(message) => {
                                if let Some(rewording) = &mut self.rewording {
                                    rewording.1 = Some(message);
                                }
                            }
                            None => {
                                self.rewording = None;
                                self.error = Some(tr!("Can’t read the commit’s message"));
                            }
                        }
                    }
                    Loaded::Message(message) => {
                        // 待つ間に書き始めていたら、そのままにする。
                        if let Some(message) = message
                            && self.changes.amend
                            && self.changes.message.is_empty()
                        {
                            self.changes.message.set_text(&message);
                        }
                    }
                    Loaded::Staged(result) => {
                        self.changes.clear_mark();
                        if let Err(error) = result {
                            self.error = Some(error);
                        }
                        // 変化の知らせを待たずに読み直す。
                        self.changes.refresh();
                    }
                }
            }
            // 取り消した検索（語を替えた・やめた）のものは捨てる。
            Event::Partial { query, rows, done } => {
                if let Some((current, Some(_))) = &self.search
                    && *current == query
                {
                    self.search_done = done;
                    self.history.set_partial_matches(rows);
                    self.dirty = true;
                }
            }
            Event::Failed(error) => {
                self.pending = self.pending.saturating_sub(1);
                self.history.failed();
                self.opening = false;
                self.error = Some(error);
                self.dirty = true;
            }
        }
        self.request();
    }

    /// 画面に出すのに足りないものを頼む。
    fn request(&mut self) {
        // 見える範囲が動いたら、足りない行を頼む。
        if let Some(request) = self.history.request() {
            self.send(request);
        }
        if let Some(request) = self.history.lineage_request() {
            self.send(request);
        }
        let side = self.layout().side.is_some();
        // 詳細が見えているなら、選んでいるコミットのものにする。
        if side
            && let Some(id) = match self.view {
                View::History => self.pinned.or(self.history.selected_id()),
                View::Reflog => self.reflog.selected_id(),
                View::File => self.file.selected_id(),
                View::Changes => None,
            }
        {
            let stash = self.view == View::History && self.pinned == Some(id);
            // 詳細の中だけが変わる。
            if self.details.want(id, stash) {
                self.damage |= Damage::SIDE;
            }
        }
        if let Some(request) = self.file.request() {
            self.send(request);
        }
        if let Some(request) = self.reflog.request(self.view == View::Reflog) {
            self.send(request);
        }
        if let Some(request) = self.details.request() {
            self.send(request);
        }
        while let Some(request) = self.changes.request(side && self.view == View::Changes) {
            self.send(request);
        }
    }

    fn key(&mut self, key: KeyEvent) {
        // Windows は離したときも知らせる。
        if key.kind == KeyEventKind::Release {
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        // 画面が乱れたときに、どこからでもすべて書き直す（中身は読み直さない）。
        if control && key.code == KeyCode::Char('l') {
            self.redraw = true;
            self.dirty = true;
            return;
        }
        // 詳細・差分が、一覧の所に開いている（並んでいない）。
        let covering = self.focus == Focus::Side && self.layout().main.is_none();
        // エラーは、次のキーまで出しておく。
        self.dirty |= self.error.take().is_some() | self.notice.take().is_some();
        // push を確かめている間の `t`: 送り先のブランチを打つ。
        if std::mem::take(&mut self.push_to_offered) && key.code == KeyCode::Char('t') {
            self.confirm = None;
            self.choices = None;
            self.ask_push_to();
            self.dirty = true;
            return;
        }
        // 確かめている間は、`y`（か Enter）で行い、ほかのキーでやめる。答えたら、重ねていた絵も消す。
        if self.choices.is_some() || self.confirm.is_some() {
            self.question_drawing = None;
        }
        if let Some((_, options)) = self.choices.take() {
            if let KeyCode::Char(pressed) = key.code
                && let Some((_, _, request)) = options.into_iter().find(|o| o.0 == pressed)
            {
                self.send(request);
            }
            self.dirty = true;
            return;
        }
        if let Some((_, _, request)) = self.confirm.take() {
            if matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter) {
                self.send(request);
            }
            self.dirty = true;
            return;
        }
        // 尋ねられている間は、Enter で答える、Esc でやめる（尋ねた操作は失敗する）。
        #[cfg(unix)]
        if let Some(question) = self.questions.front() {
            self.dirty = true;
            match key.code {
                KeyCode::Enter => {
                    let _ = question.reply.send(Some(std::mem::take(&mut self.answer)));
                    self.questions.pop_front();
                }
                KeyCode::Esc => {
                    let _ = question.reply.send(None);
                    self.answer.clear();
                    self.questions.pop_front();
                }
                KeyCode::Backspace => {
                    self.answer.pop();
                }
                KeyCode::Char(ch) if !control => self.answer.push(ch),
                _ => {}
            }
            return;
        }
        // キーの一覧は、`j` `k`（と矢印）で動かせる。ほかのキーで閉じる。
        if self.help {
            self.dirty = true;
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.help_top += 1,
                KeyCode::Char('k') | KeyCode::Up => self.help_top = self.help_top.saturating_sub(1),
                _ => self.help = false,
            }
            return;
        }
        // 操作ログ: 選んでいる記録まで（新しいものから順に）戻す。確かめてから。ほかのキーで閉じる。
        if let Some(mut log) = self.operation_log.take() {
            self.dirty = true;
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    log.selected = (log.selected + 1).min(log.entries.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up => log.selected = log.selected.saturating_sub(1),
                KeyCode::Char('n') if control => {
                    log.selected = (log.selected + 1).min(log.entries.len().saturating_sub(1));
                }
                KeyCode::Char('p') if control => log.selected = log.selected.saturating_sub(1),
                KeyCode::Enter => {
                    let count = log.selected + 1;
                    let ask = match count {
                        1 => tr!("Undo “{label}”?", label = log.entries[0].0),
                        _ => tr!("Undo the last {count} operations?", count = count),
                    };
                    self.confirm = Some((
                        ask,
                        tr!("undo"),
                        Request::Run {
                            name: "undo",
                            command: Value::Array(vec![json!({"op": "undo"}); count]),
                        },
                    ));
                    return;
                }
                _ => return,
            }
            self.operation_log = Some(log);
            return;
        }
        if self.edit_mode.is_some() {
            self.edit_mode_key(key.code);
            return;
        }
        // コマンドの一覧は、打った語で絞る。Enter で選んでいる行を動かす、Esc でやめる。
        if let Some(mut palette) = self.palette.take() {
            self.dirty = true;
            match key.code {
                KeyCode::Esc => return,
                KeyCode::Enter => {
                    let entries = self.palette_entries(&palette.query);
                    if let Some((entry, _)) = entries.get(palette.selected) {
                        self.run_entry(entry);
                    }
                    return;
                }
                KeyCode::Down => palette.selected += 1,
                KeyCode::Char('n') if control => palette.selected += 1,
                KeyCode::Up => palette.selected = palette.selected.saturating_sub(1),
                KeyCode::Char('p') if control => {
                    palette.selected = palette.selected.saturating_sub(1);
                }
                KeyCode::Backspace => {
                    palette.query.pop();
                    palette.selected = 0;
                }
                KeyCode::Char(ch) if !control => {
                    palette.query.push(ch);
                    palette.selected = 0;
                }
                _ => {}
            }
            let count = self.palette_entries(&palette.query).len();
            palette.selected = palette.selected.min(count.saturating_sub(1));
            self.palette = Some(palette);
            return;
        }
        // 絞り込みの語は、打つたびに効く。Enter で確定、Esc で絞り込みをやめる。
        if let Some(filter) = &mut self.typing_filter {
            self.dirty = true;
            match key.code {
                KeyCode::Enter => self.typing_filter = None,
                KeyCode::Esc => {
                    self.typing_filter = None;
                    self.sidebar.set_filter("");
                }
                KeyCode::Backspace => {
                    filter.pop();
                    self.sidebar.set_filter(filter);
                }
                KeyCode::Char(ch) if !control => {
                    filter.push(ch);
                    self.sidebar.set_filter(filter);
                }
                _ => {}
            }
            return;
        }
        // 新しいブランチの名前を打っている間は、Enter で作って切り替える、Esc でやめる。
        if let Some((_, _, name)) = &mut self.typing_branch {
            self.dirty = true;
            match key.code {
                KeyCode::Enter if matches!(self.naming, Naming::Stash) => {
                    if let Some((_, _, message)) = self.typing_branch.take() {
                        self.ask_stash(message.trim());
                    }
                }
                KeyCode::Enter => {
                    if let Some((start, _, name)) = self.typing_branch.take()
                        && !name.trim().is_empty()
                    {
                        let name = name.trim();
                        let (op, label, command) = match self.naming {
                            Naming::Stash => return,
                            Naming::PushTo => {
                                self.push_to(&start, name);
                                return;
                            }
                            Naming::Orphan => {
                                self.ask_orphan(name);
                                return;
                            }
                            Naming::RemoteName => {
                                if self.remotes.iter().any(|r| r == name) {
                                    self.error =
                                        Some(tr!("There is a remote named {name}", name = name));
                                    return;
                                }
                                self.naming = Naming::RemoteUrl;
                                self.typing_branch =
                                    Some((name.to_owned(), name.to_owned(), String::new()));
                                return;
                            }
                            Naming::Archive => {
                                self.export_archive(&start, name);
                                return;
                            }
                            Naming::BundleFile => {
                                let file = local_path(name);
                                self.bundle_file = Some(file.clone());
                                (
                                    "bundleCheck",
                                    tr!("Reading the bundle…"),
                                    json!({"op": "bundleCheck", "file": file}),
                                )
                            }
                            Naming::SavePatch => (
                                "savePatch",
                                tr!("Saving the patch…"),
                                json!({"op": "formatPatch", "oids": [start], "dir": local_path(name)}),
                            ),
                            Naming::Patch => {
                                let file = local_path(name);
                                self.patch_file = Some(file.clone());
                                (
                                    "readPatch",
                                    tr!("Reading the patch…"),
                                    json!({"op": "readPatch", "paths": [file]}),
                                )
                            }
                            Naming::BundleName => (
                                "fetchBundle",
                                tr!("Fetching from the bundle…"),
                                json!({"op": "fetchBundle", "file": start, "name": name}),
                            ),
                            Naming::Bundle => (
                                "bundle",
                                tr!("Saving the bundle…"),
                                json!({"op": "bundle", "refs": [], "bases": [], "file": local_path(name)}),
                            ),
                            Naming::RemoteUrl => (
                                "addRemote",
                                tr!("Adding {name}…", name = start),
                                json!({
                                    "op": "changeRemote",
                                    "change": {"kind": "add", "name": start, "url": name},
                                }),
                            ),
                            Naming::Worktree => (
                                "addWorktree",
                                tr!("Adding the worktree…"),
                                json!({"op": "worktreeAdd", "path": name, "start": start}),
                            ),
                            Naming::StashBranch => (
                                "stashBranch",
                                tr!("Creating {name}…", name = name),
                                json!({"op": "stashBranch", "name": start, "branch": name}),
                            ),
                            Naming::Branch => (
                                "createBranch",
                                tr!("Creating {name}…", name = name),
                                json!({
                                    "op": "createBranch", "name": name, "start": start,
                                    "checkout": true,
                                }),
                            ),
                            Naming::Tag => (
                                "createTag",
                                tr!("Tagging {name}…", name = name),
                                json!({
                                    "op": "createTag", "name": name, "target": start,
                                    "message": null,
                                }),
                            ),
                            Naming::Rename => (
                                "renameBranch",
                                tr!("Renaming to {name}…", name = name),
                                json!({"op": "renameBranch", "from": start, "to": name}),
                            ),
                        };
                        self.run(op, label, command);
                    }
                }
                KeyCode::Esc => self.typing_branch = None,
                KeyCode::Backspace => {
                    name.pop();
                }
                // 端末の行と同じく、Ctrl-u で打ったものを消す（初めに入れてあるパスを打ち直すときなど）。
                KeyCode::Char('u') if control => name.clear(),
                KeyCode::Char(ch) if !control => name.push(ch),
                _ => {}
            }
            return;
        }
        // 検索の語を打っている間は、Enter で検索、Esc でやめる。
        if let Some(query) = &mut self.typing_query {
            self.dirty = true;
            match key.code {
                KeyCode::Enter => {
                    let query = self.typing_query.take().unwrap_or_default();
                    if query.trim().is_empty() {
                        self.clear_search();
                    } else {
                        self.start_search(query);
                    }
                }
                KeyCode::Esc => self.typing_query = None,
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Char(ch) if !control => query.push(ch),
                _ => {}
            }
            return;
        }
        // メッセージを書いている間は、文字のキーはすべて入力。
        if self.focus == Focus::Message && !(control && key.code == KeyCode::Char('c')) {
            self.message_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('c') if control => self.quit = true,
            #[cfg(unix)]
            KeyCode::Char('z') if control => self.suspend = true,
            KeyCode::Char('M') => {
                self.mouse = !self.mouse;
                self.mouse_changed = true;
                self.notice = Some(match self.mouse {
                    true => tr!("Mouse on"),
                    false => tr!("Mouse off: the terminal selects text"),
                });
                self.dirty = true;
            }
            KeyCode::Char('q') if !covering => self.quit = true,
            KeyCode::Tab | KeyCode::BackTab => {
                // サイドバーが出ていれば、そこも回る。
                let sidebar = self.size.0 >= SIDEBAR_FROM;
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::Main,
                    Focus::Main | Focus::Message => Focus::Side,
                    Focus::Side if sidebar => Focus::Sidebar,
                    Focus::Side => Focus::Main,
                };
                self.dirty = true;
            }
            KeyCode::Char('f') if !control => {
                self.run("fetch", tr!("Fetching…"), json!({"op": "fetch"}))
            }
            KeyCode::Char('p') if !control => self.ask_pull(),
            KeyCode::Char('P') if !control => self.ask_push(),
            // 次に戻す操作の名前を聞いてから、確かめる。
            KeyCode::Char('u') if !control => {
                self.run("undoPeek", tr!("Undo…"), json!({"op": "undoPeek"}))
            }
            // サイドバーの stash なら、そこにブランチを作って戻す（`stash_key`）。
            KeyCode::Char('b') if !control => {
                if !(self.focus == Focus::Sidebar && self.stash_key(KeyCode::Char('b'))) {
                    self.ask_branch();
                }
            }
            KeyCode::Char('T') => self.ask_name(Naming::Tag),
            KeyCode::Char('R') if self.view != View::Changes && self.focus == Focus::Main => {
                self.ask_reset();
            }
            // stash する前に、メッセージ（なくてもよい）を聞き、何を含めるかを選ばせる。
            KeyCode::Char('s') if !control && self.focus != Focus::Side => {
                self.naming = Naming::Stash;
                self.typing_branch = Some((String::new(), String::new(), String::new()));
                self.dirty = true;
            }
            KeyCode::Char('S') if self.focus != Focus::Side => match self.stash.clone() {
                Some(name) => self.run(
                    "pop",
                    tr!("Popping the stash…"),
                    json!({"op": "stashApply", "name": name, "pop": true}),
                ),
                None => {
                    self.error = Some(tr!("No stash to pop"));
                    self.dirty = true;
                }
            },
            // 途中の操作があるときだけ: 続ける、飛ばす、やめる（やめる前に確かめる）。
            KeyCode::Char('C') if self.sequence.is_some() => {
                self.run("continue", tr!("Continuing…"), json!({"op": "continue"}));
            }
            KeyCode::Char('K') if self.sequence.is_some() => {
                self.run("skip", tr!("Skipping…"), json!({"op": "skip"}));
            }
            KeyCode::Char('A') if self.sequence.is_some() => {
                let kind = self.sequence.as_ref().map_or("", |s| s.0.as_str());
                self.confirm = Some((
                    tr!("Abort the {kind}?", kind = kind),
                    tr!("abort"),
                    Request::Run {
                        name: "abort",
                        command: json!({"op": "abort"}),
                    },
                ));
                self.dirty = true;
            }
            KeyCode::Char('?') => {
                self.help = true;
                self.help_top = 0;
                self.dirty = true;
            }
            KeyCode::Char(':') => {
                self.palette = Some(Palette::default());
                self.dirty = true;
            }
            KeyCode::Char('0') => {
                self.dirty |= self.focus != Focus::Sidebar;
                self.focus = Focus::Sidebar;
            }
            KeyCode::Char('1') => self.show(View::Changes),
            KeyCode::Char('2') => self.show(View::History),
            KeyCode::Char('3') => self.show(View::Reflog),
            _ => {
                self.dirty |= match (self.view, self.focus) {
                    (View::History, Focus::Main) => self.history_key(key.code, control),
                    (View::History | View::Reflog | View::File, Focus::Side) => {
                        self.details_key(key.code, control)
                    }
                    (View::File, Focus::Main) => self.file_key(key.code, control),
                    (View::Reflog, Focus::Main) => self.reflog_key(key.code, control),
                    (View::Changes, Focus::Main) => self.changes_key(key.code, control),
                    (View::Changes, Focus::Side) => self.changes_diff_key(key.code, control),
                    (_, Focus::Message) => false,
                    (_, Focus::Sidebar) => self.sidebar_key(key.code, control),
                }
            }
        }
    }

    /// コマンドの一覧に出す操作: 今いる所で動かせて、名前が `query` に合うもの。
    fn palette_entries(&self, query: &str) -> Vec<(&'static commands::Entry, String)> {
        commands::ENTRIES
            .iter()
            .filter(|e| match e.place {
                Place::Anywhere | Place::History => true,
                Place::NotSide => self.focus != Focus::Side,
                Place::Sidebar => self.focus == Focus::Sidebar,
                Place::Changes => self.view == View::Changes,
                Place::Sequence => self.sequence.is_some(),
            })
            .map(|e| (e, e.title()))
            .filter(|(_, title)| commands::matches(title, query))
            .collect()
    }

    /// コマンドの一覧で選んだ操作を動かす: その操作のキーを押したのと同じ。
    fn run_entry(&mut self, entry: &commands::Entry) {
        match entry.place {
            Place::History => self.show(View::History),
            Place::Changes if self.focus == Focus::Side => self.focus = Focus::Main,
            _ => {}
        }
        if entry.key == KeyCode::Null {
            self.run_keyless(entry.id);
            return;
        }
        let modifiers = match entry.control {
            true => KeyModifiers::CONTROL,
            false => KeyModifiers::NONE,
        };
        self.key(KeyEvent::new(entry.key, modifiers));
    }

    /// Working Copy で選んでいるコンフリクトのファイルに、Magic Merge を当てる（決まった規則で解けるものを解く。
    /// Undo できる）。差分で開いていれば、そのファイルに。
    fn magic_merge(&mut self) {
        match self.changes.conflicted_path().map(str::to_owned) {
            Some(path) => self.run(
                "magicMerge",
                tr!("Magic Merge…"),
                json!({"op": "magicMergeFile", "path": path}),
            ),
            None => {
                self.error = Some(tr!("Select a conflicted file in the Working Copy"));
                self.dirty = true;
            }
        }
    }

    /// コマンドの一覧からだけ動かす操作（キーを持たない）。
    fn run_keyless(&mut self, id: &str) {
        if id == "activity" {
            // アプリの Activity のうち、このリポジトリの操作ログ（git の記録・画面の速さは端末では出さない）。
            self.run(
                "undoLog",
                tr!("Reading the operation log…"),
                json!({"op": "undoLog"}),
            );
            return;
        }
        if id == "compare-with-previous" {
            self.run(
                "previousVersion",
                tr!("Finding the previous version…"),
                json!({"op": "previousVersion"}),
            );
        }
        if id == "save-patch" {
            // History で選んでいるコミットを、git の名前のファイルにして、今のフォルダ（この機械）に。
            let Some(id) = self.history.selected_id() else {
                return;
            };
            let id = id.to_string();
            let short = id[..id.len().min(8)].to_owned();
            self.naming = Naming::SavePatch;
            self.typing_branch = Some((id, short, local_path("")));
            self.dirty = true;
        }
        if id == "apply-patch" {
            self.naming = Naming::Patch;
            self.typing_branch = Some((String::new(), String::new(), local_path("")));
            self.dirty = true;
        }
        if id == "fetch-bundle" {
            self.naming = Naming::BundleFile;
            self.typing_branch = Some((String::new(), String::new(), local_path("")));
            self.dirty = true;
        }
        if id == "save-bundle" {
            // HEAD とすべてのブランチ・タグを、今のフォルダ（この機械）の 1 つのファイルに。
            let name = match self.name.is_empty() {
                true => "repository".to_owned(),
                false => self.name.clone(),
            };
            self.naming = Naming::Bundle;
            let file = local_path(&format!("{name}.bundle"));
            self.typing_branch = Some((String::new(), String::new(), file));
            self.dirty = true;
        }
        if id == "export-archive" {
            // History で選んでいるコミットを、今のフォルダ（TUI を始めた所。SSH 先でも、この機械）に書き出す。
            let Some(id) = self.history.selected_id() else {
                return;
            };
            let id = id.to_string();
            let short = id[..id.len().min(8)].to_owned();
            let name = match self.name.is_empty() {
                true => short.clone(),
                false => format!("{}-{short}", self.name),
            };
            let file = local_path(&format!("{name}.zip"));
            self.naming = Naming::Archive;
            self.typing_branch = Some((id, short, file));
            self.dirty = true;
        }
        if id == "delete-ignored-files" {
            self.run(
                "ignoredFiles",
                tr!("Finding ignored files…"),
                json!({"op": "ignoredFiles"}),
            );
        }
        if id == "update-submodules" {
            self.run(
                "submodules",
                tr!("Reading the submodules…"),
                json!({"op": "submodules"}),
            );
        }
        if id == "add-remote" {
            self.naming = Naming::RemoteName;
            // 初めは、まだリモートがなければ origin。
            let name = match self.remotes.is_empty() {
                true => "origin".to_owned(),
                false => String::new(),
            };
            self.typing_branch = Some((String::new(), String::new(), name));
            self.dirty = true;
        }
        if id == "clean-up-branches" {
            self.run(
                "cleanupList",
                tr!("Finding branches to clean up…"),
                json!({"op": "branchCleanup"}),
            );
        }
        if id == "new-orphan-branch" {
            self.naming = Naming::Orphan;
            self.typing_branch = Some((String::new(), String::new(), String::new()));
            self.dirty = true;
        }
        if id == "prune-worktrees" {
            // フォルダのなくなった worktree の記録を片付ける（フォルダには触らない）。
            self.run(
                "pruneWorktrees",
                tr!("Pruning worktrees…"),
                json!({"op": "worktreePrune"}),
            );
        }
    }

    /// メインの画面を替える。
    fn show(&mut self, view: View) {
        // Reflog を開き直すと、HEAD のものに戻る（ブランチのものは、サイドバーの `L` で）。
        if view == View::Reflog {
            self.dirty |= self.reflog.name().is_some();
            self.reflog.show(None);
        }
        self.dirty |= self.view != view || self.focus != Focus::Main;
        self.pinned = None;
        self.view = view;
        self.focus = Focus::Main;
        self.sidebar.show_view(view.picked());
    }

    /// コアの命令を動かす。1 つずつ（動いている間は、次を受けない）。
    fn run(&mut self, name: &'static str, label: String, command: Value) {
        self.dirty = true;
        if self.running.is_some() {
            self.error = Some(tr!("Another operation is still running"));
            return;
        }
        self.running = Some(label);
        self.send(Request::Run { name, command });
    }

    /// History の並び順を、日付順とトポロジー順で替える（この TUI の間だけ）。
    fn toggle_history_order(&mut self) {
        self.dirty = true;
        if self.running.is_some() {
            self.error = Some(tr!("Another operation is still running"));
            return;
        }
        let order = match self.topo_order {
            true => "date",
            false => "topo",
        };
        self.running = Some(tr!("Reordering History…"));
        self.send(Request::Call {
            name: "historyOrder",
            request: json!({"op": "historyOrder", "order": order}),
        });
    }

    /// 命令が済んだ。だめなら理由を出し、どちらでも状態・ref・History を読み直す。
    fn ran(&mut self, name: &'static str, result: std::result::Result<Value, String>) {
        self.running = None;
        let reply = match result {
            Ok(reply) => reply,
            // 絵が読めなくても、cherry-pick・revert は確かめる（絵なしで）。
            Err(_) if name == "pickPreview" => {
                if let Some((ask, yes, request, _)) = self.pick_question.take() {
                    self.confirm = Some((ask, yes, request));
                    self.dirty = true;
                }
                return;
            }
            Err(error) => {
                // 長い出力は、最初の行だけ。
                let line = error.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                self.error = Some(line.trim().to_owned());
                self.changes.refresh();
                self.open();
                return;
            }
        };
        match name {
            "pickPreview" => {
                if let Some((ask, yes, request, title)) = self.pick_question.take() {
                    let branch = self.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
                    self.question_drawing = Some(&reply["info"]["drawing"])
                        .filter(|d| d.is_object())
                        .map(|d| (title, crate::drawing::from_pick(d, &branch)));
                    self.confirm = Some((ask, yes, request));
                    self.dirty = true;
                }
                return;
            }
            "undoLog" => {
                let entries: Vec<(String, i64)> = reply["info"]["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|e| {
                        let label = e["label"].as_str().unwrap_or_default().to_owned();
                        (label, e["time"].as_f64().unwrap_or(0.0) as i64 / 1000)
                    })
                    .collect();
                match entries.is_empty() {
                    true => self.notice = Some(tr!("Nothing to undo")),
                    false => {
                        self.operation_log = Some(OperationLog {
                            entries,
                            selected: 0,
                        })
                    }
                }
                return;
            }
            // 戻す操作があれば、名前を出して確かめる。
            "undoPeek" => {
                match reply["label"].as_str() {
                    Some(label) => {
                        self.confirm = Some((
                            tr!("Undo “{label}”?", label = label),
                            tr!("undo"),
                            Request::Run {
                                name: "undo",
                                command: json!({"op": "undo"}),
                            },
                        ));
                    }
                    None => self.error = Some(tr!("Nothing to undo")),
                }
                return;
            }
            "fetch" => self.notice = Some(tr!("Fetched")),
            "pull" => self.notice = Some(tr!("Pulled")),
            "push" => self.notice = Some(tr!("Pushed")),
            "undo" => self.notice = Some(tr!("Undone")),
            "checkout" => self.notice = Some(tr!("Checked out")),
            "createBranch" => self.notice = Some(tr!("Created the branch")),
            "createTag" => self.notice = Some(tr!("Created the tag")),
            "renameBranch" => self.notice = Some(tr!("Renamed")),
            "deleteBranch" | "deleteTag" => self.notice = Some(tr!("Deleted")),
            "stash" => self.notice = Some(tr!("Stashed")),
            "pop" => self.notice = Some(tr!("Popped the stash")),
            "abort" => self.notice = Some(tr!("Aborted")),
            "resolve" => self.notice = Some(tr!("Resolved")),
            "reset" => {
                // hard で作業中の変更があれば、消す前に stash に残っている。
                self.notice = Some(match reply["info"]["stashed"] == true {
                    true => tr!("Reset (the working changes are in a stash)"),
                    false => tr!("Reset"),
                });
            }
            "apply" => self.notice = Some(tr!("Applied the stash")),
            "drop" => self.notice = Some(tr!("Dropped the stash")),
            "stashBranch" => self.notice = Some(tr!("Applied the stash on the new branch")),
            "removeWorktree" => self.notice = Some(tr!("Removed the worktree")),
            "restoreFile" => self.notice = Some(tr!("Restored the version")),
            "addWorktree" => self.notice = Some(tr!("Added the worktree")),
            "setUpstream" => self.notice = Some(tr!("Set the upstream")),
            "pruneWorktrees" => self.notice = Some(tr!("Pruned the worktrees")),
            "orphan" => self.notice = Some(tr!("Created the branch")),
            // 片付けてよいブランチが分かった: 選んでおくもの（取り込み済み）を消すか、上流の消えたものも含めるか。
            "cleanupList" => {
                self.ask_clean_up(&reply["info"]);
                return;
            }
            "cleanUp" => self.notice = Some(tr!("Deleted the branches")),
            "addRemote" => self.notice = Some(tr!("Added the remote")),
            "submodules" => {
                self.ask_update_submodules(&reply["info"]);
                return;
            }
            "updateSubmodules" => self.notice = Some(tr!("Updated the submodules")),
            // ignore されたものが分かった: 数と最初のいくつかを出して、確かめてから消す（Undo では戻らない）。
            "ignoredFiles" => {
                let paths: Vec<String> = (reply["info"]["paths"].as_array().into_iter().flatten())
                    .filter_map(|p| p.as_str().map(str::to_owned))
                    .collect();
                self.dirty = true;
                if paths.is_empty() {
                    self.notice = Some(tr!(
                        "No ignored files: nothing in the working tree matches .gitignore"
                    ));
                    return;
                }
                const SHOWN: usize = 3;
                let mut names = amonhen_model::i18n::list(&paths[..paths.len().min(SHOWN)]);
                if paths.len() > SHOWN {
                    names = tr!(
                        "{names} and {count} more",
                        names = names,
                        count = paths.len() - SHOWN
                    );
                }
                self.confirm = Some((
                    tr!(
                        "Delete {count} ignored items ({names})? Undo can’t bring them back.",
                        count = paths.len(),
                        names = names
                    ),
                    tr!("delete"),
                    Request::Run {
                        name: "deleteIgnored",
                        command: json!({"op": "deleteIgnored", "paths": paths}),
                    },
                ));
                return;
            }
            "deleteIgnored" => self.notice = Some(tr!("Deleted the ignored files")),
            // パッチの中身が分かった: 当たらなければ git の答えを出す。メールの形ならコミットとして（git am）、
            // ただの diff なら作業ツリーに（git apply）当てるかを確かめる。
            "readPatch" => {
                self.dirty = true;
                let Some(file) = self.patch_file.take() else {
                    return;
                };
                let info = &reply["info"];
                if info["applies"] != true {
                    let error = info["error"].as_str().unwrap_or("");
                    let line = error
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or(error);
                    self.error = Some(tr!(
                        "The patch doesn’t apply: {reason}",
                        reason = line.trim()
                    ));
                    return;
                }
                let patches = info["patches"].as_array().map_or(0, Vec::len);
                let (ask, request) = match info["kind"].as_str() {
                    Some("mailbox") => (
                        match patches {
                            1 => tr!("Apply the patch to the current branch as a commit?"),
                            count => tr!(
                                "Apply {count} patches to the current branch as commits?",
                                count = count
                            ),
                        },
                        Request::Run {
                            name: "am",
                            command: json!({"op": "am", "paths": [file], "threeWay": null}),
                        },
                    ),
                    _ => (
                        tr!("Apply the diff to the working tree? Undo brings the files back."),
                        Request::Run {
                            name: "applyPatch",
                            command: json!({"op": "applyPatch", "paths": [file]}),
                        },
                    ),
                };
                self.confirm = Some((ask, tr!("apply"), request));
                return;
            }
            "applyPatch" => self.notice = Some(tr!("Applied the patch")),
            "magicMerge" => {
                let resolved = reply["info"]["resolved"].as_u64().unwrap_or(0);
                let remaining = reply["info"]["remaining"].as_u64().unwrap_or(0);
                self.notice = Some(match (resolved, remaining) {
                    (0, _) => tr!("Magic Merge found nothing it can resolve on its own"),
                    (_, 0) => tr!("Magic Merge resolved {count}; none left", count = resolved),
                    _ => tr!(
                        "Magic Merge resolved {count}; {left} left",
                        count = resolved,
                        left = remaining
                    ),
                });
            }
            // 今のブランチの、最後に書き換える前の版が分かった: その範囲で range-diff を頼む。
            "previousVersion" => {
                let previous = &reply["info"]["previous"];
                if previous.is_null() {
                    self.notice = Some(tr!(
                        "The branch hasn’t been rewritten (its reflog has no previous version)"
                    ));
                    self.dirty = true;
                    return;
                }
                self.range_title = Some(tr!(
                    "{branch} compared with {selector}",
                    branch = previous["branch"].as_str().unwrap_or(""),
                    selector = previous["selector"].as_str().unwrap_or("")
                ));
                self.run(
                    "rangeDiff",
                    tr!("Comparing…"),
                    json!({"op": "rangeDiff", "ranges": previous["ranges"], "creationFactor": null}),
                );
                return;
            }
            // 比べた結果は、Blame・File History と同じ画面に（選んだ組のコミットの詳細も同じ）。
            "rangeDiff" => {
                let title = self.range_title.take().unwrap_or_default();
                self.file.show_range_diff(title, &reply["info"]["pairs"]);
                if self.view != View::File {
                    self.file_back = self.view;
                }
                self.view = View::File;
                self.focus = Focus::Main;
                self.dirty = true;
                return;
            }
            // merge の見通しが分かった: コンフリクトの見込みも読んでから確かめる（fast-forward・取り込むものなしは
            // コンフリクトしない）。
            "mergePreview" => {
                let Some(preview) = self.preview.as_mut() else {
                    return;
                };
                preview.3 = reply["info"].clone();
                if reply["info"]["fastForward"] == true || reply["info"]["upToDate"] == true {
                    self.ask_with_preview(None);
                    return;
                }
                let name = preview.0.clone();
                self.run(
                    "mergeConflicts",
                    tr!("Looking ahead…"),
                    json!({"op": "predictMerge", "ours": null, "theirs": name}),
                );
                return;
            }
            "mergeConflicts" => {
                let conflicts = conflict_files(&reply["info"]["conflicts"]);
                self.ask_with_preview(conflicts);
                return;
            }
            "pullConflicts" => {
                let conflicts = conflict_files(&reply["info"]["conflicts"]);
                self.ask_pull_with(conflicts);
                return;
            }
            "resetPreview" => {
                let info = reply["info"].clone();
                self.ask_reset_with(&info);
                return;
            }
            "editRows" => {
                let rows = (reply["rows"].as_array().into_iter().flatten())
                    .map(|r| {
                        crate::editmode::Row::new(
                            r["oid"].as_str().unwrap_or("").to_owned(),
                            r["merge"] == true,
                            r["message"].as_str().unwrap_or("").to_owned(),
                        )
                    })
                    .collect();
                self.edit_mode = Some(crate::editmode::EditMode::new(rows));
                self.edit_discard = false;
                self.dirty = true;
                return;
            }
            "editPreview" => {
                let Some(mode) = &self.edit_mode else {
                    return;
                };
                let info = &reply["info"];
                // merge を越える編集では描けない（アプリと同じく、そのときは文だけ）。
                self.question_drawing = Some(info["editDrawing"].clone())
                    .filter(Value::is_object)
                    .map(|d| (tr!(" Apply These Changes? "), d));
                let ask = tr!(
                    "Apply these changes? {summary}{risk}{notes} Undo brings it all back.",
                    summary = mode.summary(),
                    risk = risk_text(rewrite_conflicts(info)),
                    notes = rewrite_notes(info)
                );
                let label = format!("Edit {} Commits", mode.rows.len());
                self.confirm = Some((
                    ask,
                    tr!("apply"),
                    Request::Run {
                        name: "rewrite",
                        command: json!({"op": "rewrite", "label": label, "edit": mode.plan()}),
                    },
                ));
                self.dirty = true;
                return;
            }
            "rebasePreview" => {
                let info = reply["info"].clone();
                let conflicts = rewrite_conflicts(&info);
                if let Some(preview) = self.preview.as_mut() {
                    preview.3 = info;
                }
                self.ask_with_preview(conflicts);
                return;
            }
            // 並び順が替わった: History を読み直す（ref と状態も、ほかの操作と同じく下で読み直す）。
            "historyOrder" => {
                self.topo_order = reply["order"] == "topo";
                self.notice = Some(match self.topo_order {
                    true => tr!(
                        "History in topological order: a branch's commits stay together under its merge"
                    ),
                    false => tr!("History in date order"),
                });
            }
            "savePatch" => {
                let file = (reply["info"]["files"].as_array())
                    .and_then(|f| f.first())
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.notice = Some(tr!("Saved {file}", file = file));
            }
            // bundle の中身が分かった: 足りないコミットがあれば断り、なければ取り込む名前を聞く。
            "bundleCheck" => {
                let info = &reply["info"];
                let missing = info["missing"].as_u64().unwrap_or(0);
                self.dirty = true;
                if missing > 0 {
                    self.error = Some(tr!(
                        "The bundle needs {count} commits this repository doesn’t have",
                        count = missing
                    ));
                    return;
                }
                let Some(file) = self.bundle_file.take() else {
                    return;
                };
                let name = info["name"].as_str().unwrap_or("bundle").to_owned();
                self.naming = Naming::BundleName;
                self.typing_branch = Some((file.clone(), file, name));
                return;
            }
            "fetchBundle" => {
                let count = reply["info"]["refs"].as_array().map_or(0, Vec::len);
                self.notice = Some(tr!(
                    "Fetched {count} branches from the bundle",
                    count = count
                ));
            }
            "bundle" => {
                let file = reply["info"]["file"].as_str().unwrap_or("");
                self.notice = Some(tr!("Saved the bundle to {file}", file = file));
            }
            "archive" => {
                let file = reply["info"]["file"].as_str().unwrap_or("");
                self.notice = Some(tr!("Exported to {file}", file = file));
            }
            // git am も、当たらなければ merge と同じく止まる（Continue・Skip・Abort）。
            "merge" | "rebase" | "continue" | "skip" | "cherryPick" | "revert" | "rewrite"
            | "am" => {
                // 編集モードは、書き換えたら閉じる（書き換えられなければ、選んだものごと残る）。
                if name == "rewrite" {
                    self.edit_mode = None;
                }
                // コンフリクトで止まったら、Working Copy で解決する。
                match reply["outcome"].as_str() {
                    Some("conflict") => {
                        self.error = Some(tr!("Stopped at conflicts: resolve them, then continue"));
                        self.view = View::Changes;
                        self.focus = Focus::Main;
                        self.sidebar.show_view(Picked::Changes);
                    }
                    Some("stopped") => self.notice = Some(tr!("Stopped")),
                    _ => self.notice = Some(tr!("Done")),
                }
            }
            _ => {}
        }
        self.changes.refresh();
        self.open();
    }

    /// pull する。上流と分かれているときは、コンフリクトの見込みを読んでから、どう取り込むかを選ばせる（ほかは
    /// git の設定のとおり）。
    fn ask_pull(&mut self) {
        if self.ahead > 0 && self.behind > 0 {
            if let Some(upstream) = self.upstream.clone() {
                self.run(
                    "pullConflicts",
                    tr!("Looking ahead…"),
                    json!({"op": "predictMerge", "ours": null, "theirs": upstream}),
                );
            } else {
                self.ask_pull_with(None);
            }
            return;
        }
        self.run("pull", tr!("Pulling…"), json!({"op": "pull"}));
    }

    /// 分かれているときの pull の選択。`conflicts` は上流を merge したときにコンフリクトしそうなファイル（分から
    /// なければ `None`。rebase でも最後の結果は同じなので目安になる）。
    fn ask_pull_with(&mut self, conflicts: Option<Vec<String>>) {
        let pull = |mode: &str| Request::Run {
            name: "pull",
            command: json!({"op": "pull", "mode": mode}),
        };
        self.choices = Some((
            tr!(
                "Pull: the branch and its upstream have diverged.{risk} How?",
                risk = risk_text(conflicts)
            ),
            vec![
                ('y', tr!("as git is set"), pull("git")),
                ('m', tr!("merge"), pull("merge")),
                ('r', tr!("rebase"), pull("rebase")),
            ],
        ));
        self.dirty = true;
    }

    /// 今のブランチを上流へ push する前に確かめる。
    fn ask_push(&mut self) {
        self.dirty = true;
        let Some(branch) = self.branch.clone() else {
            self.error = Some(tr!("No branch is checked out"));
            return;
        };
        let push =
            |remote: &str, remote_branch: &str, force: bool, set_upstream: bool| Request::Run {
                name: "push",
                command: json!({
                    "op": "push",
                    "remote": remote,
                    "branch": branch,
                    "remoteBranch": remote_branch,
                    "force": force,
                    "setUpstream": set_upstream,
                }),
            };
        let Some((remote, remote_branch)) =
            self.upstream.as_deref().and_then(|u| u.split_once('/'))
        else {
            // 上流がなければ、リモートを選んで publish する（同じ名前のブランチを作り、上流にする）。
            match self.remotes.as_slice() {
                [] => {
                    self.error = Some(tr!(
                        "There is no remote to publish {branch} to",
                        branch = branch
                    ))
                }
                [remote] => {
                    self.confirm = Some((
                        tr!(
                            "Publish {branch} to {remote}?",
                            branch = branch,
                            remote = remote
                        ),
                        tr!("publish"),
                        push(remote, &branch, false, true),
                    ));
                }
                remotes => {
                    let options = remotes
                        .iter()
                        .zip('1'..='9')
                        .map(|(remote, key)| {
                            (key, remote.clone(), push(remote, &branch, false, true))
                        })
                        .collect();
                    self.choices = Some((
                        tr!("Publish {branch} to which remote?", branch = branch),
                        options,
                    ));
                }
            }
            return;
        };
        let ask = match self.ahead {
            1 => tr!(
                "Push {branch} to {remote}/{remote_branch} (1 commit)?",
                branch = branch,
                remote = remote,
                remote_branch = remote_branch
            ),
            count => tr!(
                "Push {branch} to {remote}/{remote_branch} ({count} commits)?",
                branch = branch,
                remote = remote,
                remote_branch = remote_branch,
                count = count
            ),
        };
        // 上流に手元にないコミットがあれば、ふつうの push は断られる。置き換える push（上流が最後に見たときの
        // ままのときだけ）も選べるようにする。
        // `t` で、ほかのブランチへ送れる（`ask_push_to`。選ぶ前に横取りするので、中身は送らない）。
        self.push_to_offered = true;
        if self.behind > 0 {
            self.choices = Some((
                ask,
                vec![
                    ('y', tr!("push"), push(remote, remote_branch, false, false)),
                    (
                        'f',
                        tr!(
                            "force push (with lease), replacing {count} on the remote",
                            count = self.behind
                        ),
                        push(remote, remote_branch, true, false),
                    ),
                    ('t', tr!("another branch…"), Request::Status),
                ],
            ));
            return;
        }
        self.confirm = Some((ask, tr!("push"), push(remote, remote_branch, false, false)));
    }

    /// サイドバーで選んでいるリモートのブランチを、今のブランチの上流にする（確かめてから）。
    fn ask_set_upstream(&mut self) {
        let Some(branch) = self.branch.clone() else {
            self.error = Some(tr!("No branch is checked out"));
            return;
        };
        let picked = (self.sidebar.picked_ref())
            .filter(|r| r.0 == "remote")
            .map(|r| r.1.to_owned());
        // リモートの名前は、知っているもののうち合うもの（名前に `/` があってもよいよう、長いほうから）。
        let split = picked.as_deref().and_then(|name| {
            let mut remotes: Vec<&String> = self.remotes.iter().collect();
            remotes.sort_by_key(|r| std::cmp::Reverse(r.len()));
            remotes.into_iter().find_map(|r| {
                let rest = name.strip_prefix(r.as_str())?.strip_prefix('/')?;
                (!rest.is_empty()).then(|| (r.clone(), rest.to_owned()))
            })
        });
        let Some((remote, remote_branch)) = split else {
            self.error = Some(tr!("Select a remote branch to be the upstream"));
            return;
        };
        let set = |key: &str, value: String| json!({"op": "configSet", "key": key, "value": value});
        self.confirm = Some((
            tr!(
                "Set the upstream of {branch} to {remote}/{remote_branch}?",
                branch = branch,
                remote = remote,
                remote_branch = remote_branch
            ),
            tr!("set"),
            Request::Run {
                name: "setUpstream",
                command: json!([
                    set(&format!("branch.{branch}.remote"), remote),
                    set(
                        &format!("branch.{branch}.merge"),
                        format!("refs/heads/{remote_branch}")
                    ),
                ]),
            },
        ));
    }

    /// `rev` の時点のファイルを `file` に書き出す（`git archive`）。形はファイルの拡張子で決め、中身はファイルの名前の
    /// フォルダに入れる（アプリと同じ）。相対のパスは、TUI を始めたフォルダから。
    fn export_archive(&mut self, rev: &str, file: &str) {
        let format = [
            (".zip", "zip"),
            (".tar.gz", "tar.gz"),
            (".tgz", "tar.gz"),
            (".tar", "tar"),
        ]
        .into_iter()
        .find(|(ext, _)| file.ends_with(ext));
        let Some((ext, format)) = format else {
            self.error = Some(tr!("End the file name with .zip, .tar.gz or .tar"));
            return;
        };
        let path = std::path::PathBuf::from(local_path(file));
        let stem = path
            .file_name()
            .map(|n| n.to_string_lossy().trim_end_matches(ext).to_owned())
            .unwrap_or_default();
        let prefix = match stem.is_empty() {
            true => String::new(),
            false => format!("{stem}/"),
        };
        self.run(
            "archive",
            tr!("Exporting…"),
            json!({
                "op": "archive", "rev": rev, "format": format, "prefix": prefix, "paths": [],
                "file": path.to_string_lossy(),
            }),
        );
    }

    /// Update Submodules: `submodules` の答えから、すべての submodule を取り出して記録したコミットにするか、中の
    /// submodule までか、追うブランチの最新にするかを選ばせる（アプリのシートの選び方と同じ）。
    fn ask_update_submodules(&mut self, info: &Value) {
        let paths: Vec<String> = (info["submodules"].as_array().into_iter().flatten())
            .filter_map(|s| s["path"].as_str().map(str::to_owned))
            .collect();
        self.dirty = true;
        if paths.is_empty() {
            self.notice = Some(tr!("There are no submodules"));
            return;
        }
        let update = |recursive: bool, remote: bool| Request::Run {
            name: "updateSubmodules",
            command: json!({
                "op": "updateSubmodules", "paths": paths, "recursive": recursive, "remote": remote,
            }),
        };
        self.choices = Some((
            tr!("Update {names}?", names = amonhen_model::i18n::list(&paths)),
            vec![
                ('y', tr!("to the recorded commits"), update(false, false)),
                ('r', tr!("recursively"), update(true, false)),
                ('t', tr!("to their branches' latest"), update(false, true)),
            ],
        ));
    }

    /// Clean Up Branches: `branchCleanup` の答えから、消すブランチを確かめる。最初に選んでおくもの（既定の
    /// ブランチに取り込み済み）を消すか、上流の消えたものも含めるかを選ばせる（アプリのシートと同じく、上流が消えた
    /// だけのものは最初は選ばない）。
    fn ask_clean_up(&mut self, info: &Value) {
        let branches: Vec<&Value> = info["branches"].as_array().into_iter().flatten().collect();
        let pick = |all: bool| -> Vec<Value> {
            (branches.iter())
                .filter(|b| all || b["checked"] == true)
                .map(|b| json!({"name": b["name"], "force": b["force"]}))
                .collect()
        };
        let names = |picked: &[Value]| -> String {
            let names: Vec<&str> = picked.iter().filter_map(|b| b["name"].as_str()).collect();
            amonhen_model::i18n::list(&names)
        };
        let (checked, all) = (pick(false), pick(true));
        let delete = |branches: Vec<Value>| Request::Run {
            name: "cleanUp",
            command: json!({"op": "deleteBranches", "branches": branches}),
        };
        self.dirty = true;
        if all.is_empty() {
            self.notice = Some(tr!("No branches to clean up"));
            return;
        }
        let base = info["base"].as_str().unwrap_or("");
        let mut options = Vec::new();
        if !checked.is_empty() {
            options.push((
                'y',
                tr!("delete those merged into {base}", base = base),
                delete(checked.clone()),
            ));
        }
        if all.len() > checked.len() {
            let gone = all.len() - checked.len();
            let label = match checked.is_empty() {
                true => tr!(
                    "delete those whose upstream is gone ({count})",
                    count = gone
                ),
                false => tr!("also those whose upstream is gone ({count})", count = gone),
            };
            options.push(('a', label, delete(all.clone())));
        }
        let shown = match checked.is_empty() {
            true => names(&all),
            false => names(&checked),
        };
        self.choices = Some((
            tr!("Clean up {names}? Undo brings them back.", names = shown),
            options,
        ));
    }

    /// 履歴のない新しいブランチ `name` を、空から作るか、今のファイルを stage したまま作るかを選ばせる。
    fn ask_orphan(&mut self, name: &str) {
        let orphan = |keep_files: bool| Request::Run {
            name: "orphan",
            command: json!({"op": "createOrphanBranch", "name": name, "keepFiles": keep_files}),
        };
        self.choices = Some((
            tr!("Start {name} with no history?", name = name),
            vec![
                ('e', tr!("empty"), orphan(false)),
                ('k', tr!("keeping the current files staged"), orphan(true)),
            ],
        ));
        self.dirty = true;
    }

    /// push の送り先を打たせる（初めは上流）。
    fn ask_push_to(&mut self) {
        let Some(branch) = self.branch.clone() else {
            return;
        };
        let start = self.upstream.clone().unwrap_or_default();
        self.naming = Naming::PushTo;
        self.typing_branch = Some((branch.clone(), branch, start));
    }

    /// `branch` を `target`（`<remote>/<branch>`）へ push する。上流は変えない。
    fn push_to(&mut self, branch: &str, target: &str) {
        let parsed = target
            .split_once('/')
            .filter(|(remote, to)| !to.is_empty() && self.remotes.iter().any(|r| r == remote));
        let Some((remote, to)) = parsed else {
            self.error = Some(tr!(
                "Write the remote and the branch, like {example}",
                example = format!("{}/{branch}", self.remotes.first().map_or("origin", |r| r))
            ));
            return;
        };
        self.run(
            "push",
            tr!("Pushing to {target}…", target = target),
            json!({
                "op": "push", "remote": remote, "branch": branch, "remoteBranch": to,
                "force": false, "setUpstream": false,
            }),
        );
    }

    /// 編集モードのキー。`j` `k` で選び、`J` `K` で動かし、行ですることを選ぶ。Enter で見通しを読んでから確かめて
    /// 当てる。Esc でやめる（変更があれば、もう 1 度押してから）。
    fn edit_mode_key(&mut self, code: KeyCode) {
        use crate::editmode::Action;
        let Some(mode) = self.edit_mode.as_mut() else {
            return;
        };
        self.dirty = true;
        let last = mode.rows.len().saturating_sub(1);
        let result = match code {
            KeyCode::Char('j') | KeyCode::Down => {
                mode.selected = (mode.selected + 1).min(last);
                Ok(())
            }
            KeyCode::Char('k') | KeyCode::Up => {
                mode.selected = mode.selected.saturating_sub(1);
                Ok(())
            }
            KeyCode::Char('J') => mode.move_selected(1),
            KeyCode::Char('K') => mode.move_selected(-1),
            KeyCode::Char('p') => mode.set(Action::Pick),
            KeyCode::Char('s') => mode.set(Action::Squash),
            KeyCode::Char('f') => mode.set(Action::Fixup),
            KeyCode::Char('d') => mode.set(Action::Drop),
            KeyCode::Char('e') => mode.set(Action::Edit),
            KeyCode::Char('r') => match mode.message_to_edit() {
                Some(text) => {
                    self.plan_message = Some(text);
                    Ok(())
                }
                None => Err(tr!("A merge is recreated as it is")),
            },
            KeyCode::Enter => {
                if !mode.has_changes() {
                    self.edit_mode = None;
                    self.notice = Some(tr!("Nothing was changed"));
                    return;
                }
                let edit = mode.plan();
                self.run(
                    "editPreview",
                    tr!("Looking ahead…"),
                    json!({"op": "rewritePreview", "edit": edit, "onto": null}),
                );
                Ok(())
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if mode.has_changes() && !self.edit_discard {
                    self.edit_discard = true;
                    self.notice = Some(tr!("Press Esc again to discard these changes"));
                    return;
                }
                self.edit_mode = None;
                Ok(())
            }
            _ => Ok(()),
        };
        self.edit_discard = false;
        if let Err(reason) = result {
            self.error = Some(reason);
        }
    }

    /// History で選んでいるコミットへの操作: `y` cherry-pick、`v` revert、`c` そのコミットをチェックアウト
    /// （detached HEAD）。
    fn ask_commit_operation(&mut self, code: KeyCode) -> bool {
        let Some((id, is_merge, subject)) = self.history.selected_commit() else {
            return false;
        };
        let (id, subject) = (id.to_string(), subject.to_owned());
        let short = id[..id.len().min(8)].to_owned();
        // メッセージの書き換えは、今のメッセージを読んでからエディタで開く。
        if code == KeyCode::Char('e') {
            self.rewording = Some((id.clone(), None));
            self.send(Request::Message { id });
            return true;
        }
        let rewrite = |label: &str, kind: &str| Request::Run {
            name: "rewrite",
            command: json!({"op": "rewrite", "label": label, "edit": {"kind": kind, "oid": id}}),
        };
        if code == KeyCode::Char('F') {
            self.confirm = Some((
                tr!(
                    "Fix up {commit} “{subject}” into its parent? Undo brings it back.",
                    commit = short,
                    subject = subject
                ),
                tr!("fix up"),
                rewrite("Fixup into Parent", "fixup"),
            ));
            return true;
        }
        if code == KeyCode::Char('X') {
            self.confirm = Some((
                tr!(
                    "Drop {commit} “{subject}”? Undo brings it back.",
                    commit = short,
                    subject = subject
                ),
                tr!("drop"),
                rewrite("Drop", "drop"),
            ));
            return true;
        }
        // cherry-pick・revert は、前と後の絵を読んでから確かめる（アプリのシートと同じ）。
        if matches!(code, KeyCode::Char('y' | 'v')) && !is_merge {
            let revert = code == KeyCode::Char('v');
            let (ask, yes, name, op, title) = match revert {
                false => (
                    tr!(
                        "Cherry-pick {commit} “{subject}”?",
                        commit = short,
                        subject = subject
                    ),
                    tr!("cherry-pick"),
                    "cherryPick",
                    "cherryPick",
                    tr!(" Cherry-pick {commit} ", commit = short),
                ),
                true => (
                    tr!(
                        "Revert {commit} “{subject}”?",
                        commit = short,
                        subject = subject
                    ),
                    tr!("revert"),
                    "revert",
                    "revert",
                    tr!(" Revert {commit} ", commit = short),
                ),
            };
            let request = Request::Run {
                name,
                command: json!({"op": op, "oid": id, "isMerge": false}),
            };
            self.pick_question = Some((ask, yes, request, title));
            self.run(
                "pickPreview",
                tr!("Looking ahead…"),
                json!({"op": "pickPreview", "oids": [id], "revert": revert}),
            );
            return true;
        }
        self.confirm = Some(match code {
            KeyCode::Char('y') => (
                tr!(
                    "Cherry-pick {commit} “{subject}”?",
                    commit = short,
                    subject = subject
                ),
                tr!("cherry-pick"),
                Request::Run {
                    name: "cherryPick",
                    command: json!({"op": "cherryPick", "oid": id, "isMerge": is_merge}),
                },
            ),
            KeyCode::Char('v') => (
                tr!(
                    "Revert {commit} “{subject}”?",
                    commit = short,
                    subject = subject
                ),
                tr!("revert"),
                Request::Run {
                    name: "revert",
                    command: json!({"op": "revert", "oid": id, "isMerge": is_merge}),
                },
            ),
            _ => (
                tr!("Check out {commit} (detached HEAD)?", commit = short),
                tr!("check out"),
                Request::Run {
                    name: "checkout",
                    command: json!({"op": "detach", "oid": id}),
                },
            ),
        });
        true
    }

    /// 今のブランチを、選んでいるコミット（History・Reflog）に動かす前に、どう動かすかを選ばせる。
    fn ask_reset(&mut self) {
        let id = match self.view {
            View::History => self.history.selected_id(),
            View::Reflog => self.reflog.selected_id(),
            View::Changes | View::File => None,
        };
        let Some(id) = id.map(|id| id.to_string()) else {
            return;
        };
        // 先に見通し（外れるコミット・加わるコミット・上流が持っているか）を読み、確かめる文に書く。
        self.reset_to = Some(id.clone());
        self.run(
            "resetPreview",
            tr!("Looking ahead…"),
            json!({"op": "movePreview", "from": "HEAD", "to": id, "branch": self.branch}),
        );
    }

    /// reset の見通し（`movePreview` の `info`）から、どう reset するかを選ばせる。
    fn ask_reset_with(&mut self, preview: &Value) {
        let Some(id) = self.reset_to.take() else {
            return;
        };
        let branch = self.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
        // アプリのシートの前と後の絵（同じなら、絵はない）。
        self.question_drawing = preview["drawing"].as_object().map(|_| {
            (
                tr!(" Reset {branch} ", branch = branch),
                crate::drawing::from_move(&preview["drawing"], &branch),
            )
        });
        let lost = preview["lostCount"].as_u64().unwrap_or(0);
        let gained = preview["gainedCount"].as_u64().unwrap_or(0);
        let pushed = preview["upstreamHas"].as_u64().unwrap_or(0);
        let force = match (pushed, preview["upstream"].as_str()) {
            (1.., Some(upstream)) => tr!(
                " {upstream} has {count} of the commits it leaves, so pushing will need Force Push.",
                upstream = upstream,
                count = pushed
            ),
            _ => String::new(),
        };
        let option = |key: char, mode: &'static str, label: String| {
            (
                key,
                label,
                Request::Run {
                    name: "reset",
                    command: json!({"op": "reset", "oid": id, "mode": mode}),
                },
            )
        };
        self.choices = Some((
            tr!(
                "Reset {branch} to {commit} (−{lost} +{gained} commits)? Undo brings it back.{force}",
                branch = branch,
                commit = &id[..id.len().min(8)],
                lost = lost,
                gained = gained,
                force = force
            ),
            vec![
                option('s', "soft", tr!("soft (keep the index and files)")),
                option('m', "mixed", tr!("mixed (keep the files)")),
                option(
                    'h',
                    "hard",
                    tr!("hard (stash the changes, then discard them)"),
                ),
            ],
        ));
        self.dirty = true;
    }

    /// 新しいブランチの名前を聞く。始まりは、サイドバーなら選んでいる ref、ほかは History で選んでいるコミット
    /// （History を見ていなければ HEAD）。
    fn ask_branch(&mut self) {
        self.ask_name(Naming::Branch);
    }

    /// 新しいブランチかタグの名前を聞く（始まりは `ask_branch` のとおり）。
    /// stash に何を含めるかを選ばせる（メッセージは聞いてある。空なら付けない）。
    fn ask_stash(&mut self, message: &str) {
        let message = match message.is_empty() {
            true => Value::Null,
            false => json!(message),
        };
        let stash = |untracked: bool, keep_index: bool| Request::Run {
            name: "stash",
            command: json!({
                "op": "stashPush", "message": message,
                "includeUntracked": untracked, "keepIndex": keep_index,
            }),
        };
        self.choices = Some((
            tr!("Stash the working changes?"),
            vec![
                ('y', tr!("tracked files"), stash(false, false)),
                ('u', tr!("with untracked files"), stash(true, false)),
                ('k', tr!("keep what is staged"), stash(false, true)),
            ],
        ));
        self.dirty = true;
    }

    fn ask_name(&mut self, naming: Naming) {
        let picked = match self.focus {
            Focus::Sidebar => self
                .sidebar
                .picked_ref()
                .filter(|r| matches!(r.0, "branch" | "remote" | "tag"))
                .map(|(_, name, _)| (name.to_owned(), name.to_owned())),
            _ => None,
        };
        let start = picked.or_else(|| {
            let id = match self.view {
                View::History => self.history.selected_id()?.to_string(),
                View::Reflog => self.reflog.selected_id()?.to_string(),
                View::File => self.file.selected_id()?.to_string(),
                View::Changes => self.head.clone()?,
            };
            let short = id[..id.len().min(8)].to_owned();
            Some((id, short))
        });
        if let Some((start, shown)) = start {
            self.naming = naming;
            self.typing_branch = Some((start, shown, String::new()));
            self.dirty = true;
        }
    }

    /// サイドバーで選んでいるブランチかタグを消す前に確かめる（Undo で戻せる）。今のブランチは消せない。
    fn ask_delete(&mut self) -> bool {
        let Some((kind, name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let (op, command) = match kind {
            "branch" if Some(name) == self.branch.as_deref() => {
                self.error = Some(tr!("The current branch can’t be deleted"));
                return true;
            }
            "branch" => (
                "deleteBranch",
                json!({"op": "deleteBranch", "name": name, "force": false}),
            ),
            "tag" => ("deleteTag", json!({"op": "deleteTag", "name": name})),
            _ => return false,
        };
        self.confirm = Some((
            tr!("Delete {name}? Undo brings it back.", name = name),
            tr!("delete"),
            Request::Run { name: op, command },
        ));
        true
    }

    /// サイドバーで選んでいるブランチの、新しい名前を聞く。
    fn ask_rename(&mut self) -> bool {
        let Some(("branch", name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let name = name.to_owned();
        self.naming = Naming::Rename;
        self.typing_branch = Some((name.clone(), name.clone(), name));
        true
    }

    /// サイドバーで選んでいる ref をチェックアウトする: ローカルのブランチはそのまま、リモートのブランチは追跡する
    /// ローカルのブランチを作って。
    fn checkout_picked(&mut self) -> bool {
        let Some((kind, name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let command = match kind {
            "branch" => json!({"op": "checkout", "branch": name}),
            "remote" => json!({"op": "track", "remoteBranch": name}),
            _ => {
                self.error = Some(tr!("Only branches can be checked out here"));
                return true;
            }
        };
        let label = tr!("Checking out {name}…", name = name);
        self.run("checkout", label, command);
        true
    }

    /// サイドバーで選んでいる ref を今のブランチに merge する（`merge`）か、今のブランチをその上に rebase する
    /// 前に確かめる。
    fn ask_merge_or_rebase(&mut self, merge: bool) -> bool {
        let Some((kind, name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        if kind == "stash" || kind == "worktree" {
            return false;
        }
        let Some(branch) = self.branch.clone() else {
            self.error = Some(tr!("No branch is checked out"));
            return true;
        };
        // アプリが確かめるときに描く前と後の代わりに、エンジンの見通しを読んで、確かめる文に書く。
        let name = name.to_owned();
        self.preview = Some((name.clone(), branch, merge, Value::Null));
        match merge {
            true => self.run(
                "mergePreview",
                tr!("Looking ahead…"),
                json!({"op": "mergePreview", "theirs": name}),
            ),
            false => self.run(
                "rebasePreview",
                tr!("Looking ahead…"),
                json!({"op": "rewritePreview", "onto": name}),
            ),
        }
        true
    }

    /// merge・rebase の見通しから、確かめる文を作る。`conflicts` はコンフリクトしそうなファイル（分からなければ
    /// `None`）。
    fn ask_with_preview(&mut self, conflicts: Option<Vec<String>>) {
        let Some((name, branch, merge, preview)) = self.preview.take() else {
            return;
        };
        self.dirty = true;
        let count = |shown: &Value, more: &Value| {
            shown.as_array().map_or(0, Vec::len) as u64 + more.as_u64().unwrap_or(0)
        };
        let risk = risk_text(conflicts);
        // アプリのシートと同じく、前と後の絵を重ねる（取り込むものがなければ、絵はない）。
        self.question_drawing = match (merge, preview["fastForward"] == true) {
            (true, true) => Some(&preview["move"]["drawing"])
                .filter(|d| d.is_object())
                .map(|d| {
                    (
                        tr!(" Fast-forward {branch} ", branch = branch),
                        crate::drawing::from_move(d, &branch),
                    )
                }),
            (true, false) => Some(&preview["drawing"])
                .filter(|d| d.is_object())
                .map(|d| {
                    (
                        tr!(" Merge {name} into {branch} ", name = name, branch = branch),
                        crate::drawing::from_merge(d, &branch, &name),
                    )
                }),
            (false, _) => Some(&preview["drawing"])
                .filter(|d| d.is_object())
                .map(|d| {
                    (
                        tr!(
                            " Rebase {branch} onto {name} ",
                            branch = branch,
                            name = name
                        ),
                        crate::drawing::from_rebase(d, &branch, &name),
                    )
                }),
        };
        let (ask, yes, request) = if merge {
            let request = Request::Run {
                name: "merge",
                command: json!({"op": "merge", "rev": name}),
            };
            if preview["upToDate"] == true {
                self.notice = Some(tr!(
                    "{branch} already has {name}",
                    branch = branch,
                    name = name
                ));
                return;
            }
            if preview["fastForward"] == true {
                let drawing = &preview["move"]["drawing"];
                let n = count(&drawing["gained"], &drawing["gainedMore"]);
                let ask = tr!(
                    "Fast-forward {branch} to {name} (+{count})?",
                    branch = branch,
                    name = name,
                    count = n
                );
                (ask, tr!("merge"), request)
            } else {
                let drawing = &preview["drawing"];
                let theirs = count(&drawing["theirs"], &drawing["theirsMore"]);
                let ours = count(&drawing["ours"], &drawing["oursMore"]);
                let ask = tr!(
                    "Merge {name} into {branch} as “{subject}” (+{theirs} in, {ours} on {branch} since they split).{risk}",
                    name = name,
                    branch = branch,
                    theirs = theirs,
                    ours = ours,
                    subject = drawing["subject"].as_str().unwrap_or(""),
                    risk = risk
                );
                (ask, tr!("merge"), request)
            }
        } else {
            let ask = tr!(
                "Rebase {branch} onto {name}?{risk}{notes}",
                branch = branch,
                name = name,
                risk = risk,
                notes = rewrite_notes(&preview)
            );
            let request = Request::Run {
                name: "rebase",
                command: json!({"op": "rebase", "onto": name}),
            };
            (ask, tr!("rebase"), request)
        };
        self.confirm = Some((ask, yes, request));
    }

    /// サイドバーで選んでいる stash を戻す（`pop` なら、戻して消す）か、消す（先に確かめる）。
    fn stash_key(&mut self, code: KeyCode) -> bool {
        // worktree の行の `x` は、その worktree を消す（確かめてから。変更が残っていれば git が断る）。
        if let (KeyCode::Char('x'), Some(("worktree", path, _))) = (code, self.sidebar.picked_ref())
        {
            let path = path.to_owned();
            if self.sidebar.picked_is_current() {
                self.error = Some(tr!("This worktree is the one that is open"));
                return true;
            }
            self.confirm = Some((
                tr!(
                    "Remove the worktree at {path}? Undo can’t bring it back.",
                    path = path
                ),
                tr!("remove"),
                Request::Run {
                    name: "removeWorktree",
                    command: json!({"op": "worktreeRemove", "path": path}),
                },
            ));
            return true;
        }
        let Some(("stash", name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let name = name.to_owned();
        match code {
            KeyCode::Char('a') => self.run(
                "apply",
                tr!("Applying the stash…"),
                json!({"op": "stashApply", "name": name, "pop": false}),
            ),
            KeyCode::Char('o') => self.run(
                "pop",
                tr!("Popping the stash…"),
                json!({"op": "stashApply", "name": name, "pop": true}),
            ),
            // stash を作ったところに新しいブランチを作って切り替え、そこに戻す（アプリの Branch…）。
            KeyCode::Char('b') => {
                self.naming = Naming::StashBranch;
                self.typing_branch = Some((name.clone(), name, String::new()));
                self.dirty = true;
            }
            KeyCode::Char('x') => {
                self.confirm = Some((
                    tr!("Drop {name}? Undo brings it back.", name = name),
                    tr!("drop"),
                    Request::Run {
                        name: "drop",
                        command: json!({"op": "stashDrop", "name": name}),
                    },
                ));
            }
            _ => return false,
        }
        true
    }

    /// History を検索する。動いている検索は取り消す。
    fn start_search(&mut self, query: String) {
        self.cancel_search();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.send(Request::Search {
            history: self.history.id().clone(),
            query: query.clone(),
            cancelled: cancelled.clone(),
        });
        self.search = Some((query, Some(cancelled)));
        self.search_done = 0.0;
        self.history.begin_search();
    }

    fn cancel_search(&mut self) {
        if let Some((_, Some(running))) = &self.search {
            running.store(true, Ordering::Relaxed);
        }
    }

    fn clear_search(&mut self) -> bool {
        self.cancel_search();
        let had = self.search.take().is_some();
        self.history.clear_matches() || had
    }

    fn file_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.file.page();
        match code {
            KeyCode::Char('d') if control => self.file.move_selection(page / 2),
            KeyCode::Char('u') if control => self.file.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.file.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.file.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.file.move_selection(page),
            KeyCode::PageUp => self.file.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.file.select(0),
            KeyCode::Char('G') | KeyCode::End => self.file.select(usize::MAX),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.focus = Focus::Side;
                true
            }
            // File History で、作業ツリーのファイルをその版に戻す（確かめてから。Undo で元に戻る）。
            KeyCode::Char('V') => {
                let version = (self.file.selected_version())
                    .map(|(id, path, then)| (id.to_string(), path.to_owned(), then.to_owned()));
                match version {
                    Some((_, path, then)) if path != then => {
                        self.error = Some(tr!(
                            "The file was {then} at that commit; a version from before a rename can’t be restored here",
                            then = then
                        ));
                    }
                    Some((id, path, _)) => {
                        self.confirm = Some((
                            tr!(
                                "Restore {path} to its version at {commit}? Undo brings back what it has now.",
                                path = path,
                                commit = &id[..id.len().min(8)]
                            ),
                            tr!("restore"),
                            Request::Run {
                                name: "restoreFile",
                                command: json!({"op": "restoreFiles", "rev": id, "paths": [path]}),
                            },
                        ));
                    }
                    None => self.error = Some(tr!("Open File History (L) to restore a version")),
                }
                true
            }
            // その行のコミットを、History で選ぶ。
            KeyCode::Char('H') => {
                if let Some(id) = self.file.selected_id() {
                    self.view = View::History;
                    self.sidebar.show_view(Picked::History);
                    self.send(Request::RowOf {
                        history: self.history.id().clone(),
                        id: id.to_string(),
                    });
                }
                true
            }
            KeyCode::Esc | KeyCode::Backspace => {
                self.show(self.file_back);
                true
            }
            _ => false,
        }
    }

    fn reflog_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.reflog.page();
        match code {
            KeyCode::Char('d') if control => self.reflog.move_selection(page / 2),
            KeyCode::Char('u') if control => self.reflog.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.reflog.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.reflog.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.reflog.move_selection(page),
            KeyCode::PageUp => self.reflog.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.reflog.select(0),
            KeyCode::Char('G') | KeyCode::End => self.reflog.select(usize::MAX),
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Sidebar;
                true
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.focus = Focus::Side;
                true
            }
            _ => false,
        }
    }

    fn sidebar_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.sidebar.page();
        match code {
            KeyCode::Char('d') if control => self.sidebar.move_selection(page / 2),
            KeyCode::Char('u') if control => self.sidebar.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.sidebar.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.sidebar.move_selection(-1),
            KeyCode::PageDown => self.sidebar.move_selection(page),
            KeyCode::PageUp => self.sidebar.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.sidebar.move_selection(i64::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => self.sidebar.move_selection(i64::MAX / 2),
            KeyCode::Enter | KeyCode::Char(' ' | 'l') | KeyCode::Right => {
                self.pick();
                true
            }
            KeyCode::Char('/') => {
                self.typing_filter = Some(self.sidebar.filter().to_owned());
                true
            }
            KeyCode::Char('c') => self.checkout_picked(),
            KeyCode::Char('a' | 'o' | 'x') => self.stash_key(code),
            KeyCode::Char('D') => self.ask_delete(),
            KeyCode::Char('n') => self.ask_rename(),
            KeyCode::Char('m') => self.ask_merge_or_rebase(true),
            KeyCode::Char('r') => self.ask_merge_or_rebase(false),
            // 今のブランチの上流を、選んでいるリモートのブランチにする（アプリの Set Upstream）。
            KeyCode::Char('U') => {
                self.ask_set_upstream();
                true
            }
            // ブランチを、新しい worktree に出す。場所を聞く（今の作業ツリーの隣を、初めに入れておく）。
            KeyCode::Char('w') => {
                let picked = (self.sidebar.picked_ref())
                    .filter(|r| r.0 == "branch")
                    .map(|r| r.1.to_owned());
                match picked {
                    Some(_) if self.sidebar.picked_is_current() => {
                        self.error = Some(tr!("This branch is checked out here"));
                    }
                    Some(branch) => {
                        let path = self.workdir.as_deref().map_or(String::new(), |workdir| {
                            let workdir = workdir.trim_end_matches(['/', '\\']);
                            format!("{workdir}-{}", branch.replace(['/', '\\'], "-"))
                        });
                        self.naming = Naming::Worktree;
                        self.typing_branch = Some((branch.clone(), branch, path));
                    }
                    None => self.error = Some(tr!("Select a local branch for the new worktree")),
                }
                true
            }
            // ブランチの reflog（そのブランチが動いた記録）を、Reflog の画面に出す。
            KeyCode::Char('L') => {
                let picked = (self.sidebar.picked_ref())
                    .filter(|r| matches!(r.0, "branch" | "remote"))
                    .map(|r| r.1.to_owned());
                match picked {
                    Some(name) => {
                        self.show(View::Reflog);
                        self.reflog.show(Some(name));
                    }
                    None => self.error = Some(tr!("Select a branch to see its reflog")),
                }
                true
            }
            // 絞り込んでいれば、まずそれをやめる。
            KeyCode::Esc if !self.sidebar.filter().is_empty() => {
                self.sidebar.set_filter("");
                true
            }
            KeyCode::Esc => {
                self.focus = Focus::Main;
                true
            }
            _ => false,
        }
    }

    /// 今の枠で使えるキー（`?` で出す一覧）。
    fn help_lines(&self) -> Vec<(&'static str, String)> {
        let mut lines = match (self.view, self.focus) {
            (_, Focus::Sidebar) => vec![
                ("j k", tr!("Move")),
                (
                    "Enter Space",
                    tr!("Open the view, or select the ref's commit in History"),
                ),
                ("c", tr!("Check out the branch")),
                ("m", tr!("Merge it into the current branch")),
                ("r", tr!("Rebase the current branch onto it")),
                ("a o x", tr!("On a stash: apply, pop, drop")),
                (
                    "b",
                    tr!("On a stash: a branch where it was made, with it applied"),
                ),
                ("x", tr!("On a worktree: remove it")),
                ("w", tr!("New worktree for the branch")),
                (
                    "U",
                    tr!("On a remote branch: make it the upstream of the current branch"),
                ),
                ("n D", tr!("Rename a branch; delete a branch or a tag")),
                ("L", tr!("The branch's reflog")),
                ("/", tr!("Filter branches and tags")),
                ("Esc", tr!("Clear the filter, or go back")),
            ],
            (View::History, Focus::Main) => vec![
                ("j k", tr!("Move")),
                ("g G", tr!("Top and bottom")),
                ("PgUp PgDn", tr!("Move by a page")),
                ("@", tr!("Go to HEAD")),
                ("o", tr!("Date or topological order")),
                (
                    "z Z",
                    tr!("Fold merged branches; open or close the selected merge"),
                ),
                ("] [", tr!("Go to the parent, the child")),
                ("Enter Space", tr!("Go to the details")),
                ("/", tr!("Search")),
                ("n N", tr!("Next and previous match")),
                ("y v", tr!("Cherry-pick, revert the selected commit")),
                ("c", tr!("Check out the selected commit (detached HEAD)")),
                ("e", tr!("Edit the commit's message in git's editor")),
                ("F X", tr!("Fix up into the parent; drop the commit")),
                ("E", tr!("Edit the commits from here to HEAD at once")),
                ("Esc", tr!("Clear the search")),
            ],
            (View::Reflog, Focus::Main) => vec![
                ("j k", tr!("Move")),
                ("g G", tr!("Top and bottom")),
                ("Enter", tr!("Go to the details")),
            ],
            (View::File, Focus::Main) => vec![
                ("j k", tr!("Move")),
                ("g G", tr!("Top and bottom")),
                ("Enter", tr!("Go to the commit's details")),
                ("H", tr!("Select the commit in History")),
                (
                    "V",
                    tr!("In File History: restore the file to that version"),
                ),
                ("Esc", tr!("Go back")),
            ],
            (View::History | View::Reflog | View::File, Focus::Side) => vec![
                ("j k", tr!("Scroll")),
                ("[ ]", tr!("Previous and next file")),
                ("B L", tr!("Blame, File History of the file in view")),
                ("h l", tr!("Pan long lines; h at the left edge goes back")),
                ("Esc", tr!("Go back to History")),
            ],
            (View::Changes, Focus::Main) | (_, Focus::Message) => vec![
                ("j k", tr!("Move")),
                ("Space", tr!("Stage or unstage the file")),
                ("+", tr!("Stage all (not conflicted files)")),
                ("d", tr!("Discard the file's changes")),
                ("B L", tr!("Blame, File History of the file")),
                ("e", tr!("Open the file in git's editor")),
                (
                    "o t",
                    tr!("Take the current or the incoming side of a conflicted file"),
                ),
                (
                    "m",
                    tr!("Magic Merge: resolve what rules can in a conflicted file"),
                ),
                ("Enter", tr!("Go to the diff")),
                (
                    "c",
                    tr!("Write the commit message (Ctrl-e there opens git's editor)"),
                ),
                ("a", tr!("Amend the last commit")),
                ("Ctrl-s", tr!("Commit")),
                ("Ctrl-p", tr!("Commit, then push")),
            ],
            (View::Changes, Focus::Side) => vec![
                ("j k", tr!("Move by line")),
                ("[ ]", tr!("Previous and next hunk")),
                (
                    "Space",
                    tr!("Stage or unstage the line, or the hunk on its header"),
                ),
                ("d", tr!("Discard the line or the hunk")),
                (
                    "v",
                    tr!("Start a range of lines; Space and d then act on all of it"),
                ),
                (
                    "o t b a",
                    tr!("In a conflict: take current, incoming, both, or the base"),
                ),
                ("m", tr!("Magic Merge on the whole file")),
                ("h l", tr!("Pan long lines; h at the left edge goes back")),
                ("Esc", tr!("Go back to the list")),
            ],
        };
        lines.extend([
            ("", String::new()),
            ("f p P", tr!("Fetch, pull, push")),
            (
                "b T",
                tr!("New branch, new tag at the selected commit or ref"),
            ),
            (
                "s S",
                tr!("Stash (asks for a message), and pop the newest stash"),
            ),
            (
                "C K A",
                tr!("Continue, skip, abort a merge or rebase in progress"),
            ),
            ("u", tr!("Undo the last operation")),
            (
                "R",
                tr!("In History or Reflog: reset the branch to the selected commit"),
            ),
            (
                "M",
                tr!("Mouse on or off (off lets the terminal select text)"),
            ),
            (":", tr!("Commands by name")),
            ("Ctrl-l", tr!("Redraw the screen")),
            #[cfg(unix)]
            ("Ctrl-z", tr!("Suspend (fg in the shell comes back)")),
            ("1 2 3", tr!("Changes, History and Reflog")),
            ("0", tr!("Sidebar")),
            ("Tab", tr!("Next pane")),
            ("q", tr!("Quit")),
        ]);
        lines
    }

    /// サイドバーで選んでいる行を開く: メインの画面を替えるか、ref のコミットを History で選ぶ。
    fn pick(&mut self) {
        match self.sidebar.picked() {
            Some(Picked::Changes) => self.show(View::Changes),
            Some(Picked::History) => self.show(View::History),
            Some(Picked::Reflog) => self.show(View::Reflog),
            // stash は History の行ではないので、その差分を詳細の枠に出す。
            Some(Picked::Commit(id))
                if self.sidebar.picked_ref().is_some_and(|r| r.0 == "stash") =>
            {
                self.view = View::History;
                self.focus = Focus::Side;
                self.pinned = Oid::from_hex(&id);
            }
            // ほかの worktree は、TUI をそこで開き直す（アプリが新しい窓で開くのの代わり）。開いているものは、
            // その場所を知らせるだけ。
            Some(Picked::Commit(_))
                if self.sidebar.picked_ref().is_some_and(|r| r.0 == "worktree") =>
            {
                let path = self.sidebar.picked_ref().map_or("", |r| r.1).to_owned();
                if self.sidebar.picked_is_current() {
                    self.notice = Some(tr!(
                        "This is the worktree that is open, at {path}",
                        path = path
                    ));
                } else {
                    self.open_worktree = Some(path);
                    self.quit = true;
                }
            }
            Some(Picked::Commit(id)) => {
                self.pinned = None;
                self.view = View::History;
                self.focus = Focus::Main;
                self.send(Request::RowOf {
                    history: self.history.id().clone(),
                    id,
                });
            }
            None => {}
        }
        self.dirty = true;
    }

    /// メッセージの欄でのキー: Esc と Tab で一覧へ、`Ctrl-s`（か Alt-Enter）でコミット、ほかは入力。
    fn message_key(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        self.dirty = true;
        match key.code {
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => self.focus = Focus::Main,
            KeyCode::Char('s') if control => self.commit(),
            KeyCode::Char('p') if control => {
                self.push_after_commit = true;
                self.commit();
                self.push_after_commit &= self.committing;
            }
            KeyCode::Char('e') if control => self.edit_message = true,
            KeyCode::Enter if alt => self.commit(),
            KeyCode::Char(_) if control || alt => {}
            code => {
                self.changes.message.key(code);
            }
        }
    }

    /// 欄のメッセージでコミットを作る。できないときは、理由を出す。
    fn commit(&mut self) {
        if self.committing {
            return;
        }
        if self.changes.message.is_empty() {
            self.error = Some(tr!("Write a commit message first"));
        } else if !self.changes.amend && self.changes.staged_count() == 0 {
            self.error = Some(tr!("Nothing is staged (Space stages a file)"));
        } else {
            self.committing = true;
            self.send(Request::MakeCommit {
                message: self.changes.message.text(),
                amend: self.changes.amend,
            });
        }
    }

    fn changes_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.changes.page();
        match code {
            KeyCode::Char('d') if control => self.changes.move_selection(page / 2),
            KeyCode::Char('u') if control => self.changes.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.changes.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.changes.move_selection(-1),
            KeyCode::PageDown => self.changes.move_selection(page),
            KeyCode::PageUp => self.changes.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.changes.select(0),
            KeyCode::Char('G') | KeyCode::End => self.changes.select(usize::MAX),
            KeyCode::Char(' ') => {
                if let Some(request) = self.changes.toggle() {
                    self.send(request);
                }
                false
            }
            // Stage All（コンフリクトのファイルは除く）。
            KeyCode::Char('+') => {
                match self.changes.stage_all() {
                    Some(request) => self.send(request),
                    None => self.notice = Some(tr!("Nothing to stage")),
                }
                true
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Sidebar;
                true
            }
            // 選んでいるファイルの Blame・File History を開く。
            // 作業ツリーのファイルを、git のエディタで開く（端末を渡す）。
            KeyCode::Char('e') => {
                match (self.changes.selected_path(), self.workdir.as_deref()) {
                    _ if !self.files_here => {
                        self.error = Some(tr!(
                            "The files are on another machine; open them in a terminal there"
                        ));
                    }
                    (Some(path), Some(workdir)) => {
                        let file = std::path::Path::new(workdir).join(path);
                        match file.is_file() {
                            true => self.open_file = Some(file.to_string_lossy().into_owned()),
                            false => {
                                self.error = Some(tr!(
                                    "{path} isn’t a file in the working tree",
                                    path = path
                                ))
                            }
                        }
                    }
                    _ => {}
                }
                true
            }
            KeyCode::Char('B' | 'L') => match self.changes.selected_path() {
                Some(path) => {
                    self.file.open(path, code == KeyCode::Char('B'), None);
                    self.file_back = View::Changes;
                    self.view = View::File;
                    true
                }
                None => false,
            },
            // コンフリクトのあるファイルを、片側にして解決済みにする（先に確かめる）。
            // コンフリクトのファイルで `m`: Magic Merge（決まった規則で解けるものを解く）。
            KeyCode::Char('m') if self.changes.conflicted_path().is_some() => {
                self.magic_merge();
                true
            }
            KeyCode::Char('o' | 't') if self.changes.conflicted_path().is_some() => {
                let ours = code == KeyCode::Char('o');
                let path = self.changes.conflicted_path().unwrap_or("").to_owned();
                let ask = match ours {
                    true => tr!("Take the current side of {path}?", path = path),
                    false => tr!("Take the incoming side of {path}?", path = path),
                };
                self.confirm = Some((
                    ask,
                    tr!("take"),
                    Request::Run {
                        name: "resolve",
                        command: json!({"op": "takeSide", "path": path, "ours": ours}),
                    },
                ));
                true
            }
            KeyCode::Char('c') if self.changes.has_message_field() => {
                self.focus = Focus::Message;
                true
            }
            // 直前のコミットを作り直す。欄が空なら、そのメッセージを入れる。
            KeyCode::Char('a') => {
                self.changes.amend = !self.changes.amend;
                if self.changes.amend
                    && self.changes.message.is_empty()
                    && let Some(id) = self.head.clone()
                {
                    self.send(Request::Message { id });
                }
                true
            }
            KeyCode::Char('s') if control => {
                self.commit();
                true
            }
            KeyCode::Char('p') if control => {
                self.push_after_commit = true;
                self.commit();
                // コミットできなかったら、push もしない。
                self.push_after_commit &= self.committing;
                true
            }
            KeyCode::Char('d') => {
                self.confirm = self.changes.discard_file().map(|(path, request)| {
                    let ask = tr!(
                        "Discard the changes in {path}? Undo brings them back.",
                        path = path
                    );
                    (ask, tr!("discard"), request)
                });
                self.confirm.is_some()
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.focus = Focus::Side;
                true
            }
            _ => false,
        }
    }

    fn changes_diff_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.changes.diff_page();
        // 選んでいる行を動かすだけなら、差分の中だけが変わる。
        let moved = match code {
            KeyCode::Char('d') if control => Some(self.changes.move_cursor(page / 2)),
            KeyCode::Char('u') if control => Some(self.changes.move_cursor(-page / 2)),
            KeyCode::Char('j') | KeyCode::Down => Some(self.changes.move_cursor(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(self.changes.move_cursor(-1)),
            KeyCode::PageDown => Some(self.changes.move_cursor(page)),
            KeyCode::PageUp => Some(self.changes.move_cursor(-page)),
            KeyCode::Char('g') | KeyCode::Home => Some(self.changes.cursor_to(0)),
            KeyCode::Char('G') | KeyCode::End => Some(self.changes.cursor_to(u32::MAX)),
            KeyCode::Char(']') => Some(self.changes.jump_hunk(true)),
            KeyCode::Char('[') => Some(self.changes.jump_hunk(false)),
            _ => None,
        };
        if let Some(moved) = moved {
            if moved {
                self.damage |= Damage::SIDE;
            }
            return false;
        }
        match code {
            // 横に動かすのも、差分の中だけが変わる。
            KeyCode::Char('l') | KeyCode::Right => {
                if self.changes.pan(PAN) {
                    self.damage |= Damage::SIDE;
                }
                false
            }
            // 左の端にいるなら、動かさずに一覧へ戻る。
            KeyCode::Char('h') | KeyCode::Left => {
                if self.changes.pan(-PAN) {
                    self.damage |= Damage::SIDE;
                    false
                } else {
                    self.focus = Focus::Main;
                    true
                }
            }
            // 行の範囲: `v` で始まりを決め、動かした所までを選ぶ。もう一度で、やめる。
            KeyCode::Char('v') => self.changes.toggle_mark(),
            KeyCode::Esc if self.changes.has_mark() => self.changes.toggle_mark(),
            // 共通の祖先の側は、コンフリクトに入っているとき（diff3 の形）だけ選べる。
            KeyCode::Char('a')
                if self.changes.conflict_at_cursor().is_some()
                    && !self.changes.conflict_has_base() =>
            {
                self.error = Some(tr!(
                    "This conflict has no base side (git writes it with merge.conflictStyle diff3 or zdiff3)"
                ));
                true
            }
            // コンフリクトの中では、そのコンフリクトを Current・Incoming・両方・共通の祖先のどれかにする（Undo
            // できる）。
            // コンフリクトのファイルの差分でも `m` で Magic Merge（ファイルのすべてのコンフリクトに）。
            KeyCode::Char('m') if self.changes.conflicted_path().is_some() => {
                self.magic_merge();
                true
            }
            KeyCode::Char('o' | 't' | 'b' | 'a') if self.changes.conflict_at_cursor().is_some() => {
                let choice = match code {
                    KeyCode::Char('o') => "ours",
                    KeyCode::Char('t') => "theirs",
                    KeyCode::Char('a') => "base",
                    _ => "both",
                };
                if let Some((path, index)) = self.changes.conflict_at_cursor() {
                    let command = json!({
                        "op": "resolveConflict", "path": path, "index": index, "choice": choice,
                    });
                    self.run("resolve", tr!("Resolving…"), command);
                }
                true
            }
            // 行（hunk の見出しなら、その hunk）の stage を切り替える。
            KeyCode::Char(' ') => match self.changes.toggle_rows() {
                Ok(Some(request)) => {
                    self.send(request);
                    false
                }
                Ok(None) => false,
                Err(error) => {
                    self.error = Some(error);
                    true
                }
            },
            KeyCode::Char('d') => match self.changes.discard_rows() {
                Ok(Some((target, request))) => {
                    let ask = match (target.hunk, target.rows.len()) {
                        (false, _) => tr!("Discard this line? Undo brings it back."),
                        (true, 1) => tr!("Discard this hunk (1 line)? Undo brings it back."),
                        (true, count) => tr!(
                            "Discard this hunk ({count} lines)? Undo brings it back.",
                            count = count
                        ),
                    };
                    self.confirm = Some((ask, tr!("discard"), request));
                    true
                }
                Ok(None) => false,
                Err(error) => {
                    self.error = Some(error);
                    true
                }
            },
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.focus = Focus::Main;
                true
            }
            _ => false,
        }
    }

    fn history_key(&mut self, code: KeyCode, control: bool) -> bool {
        // History を操作したら、開いていた stash ではなく選んでいるコミットを出す。
        self.pinned = None;
        let page = self.history.page();
        // 選択を動かすだけなら、History の中だけが変わる（詳細は、頼むときに印をつける）。
        let moved = match code {
            KeyCode::Char('d') if control => Some(self.history.move_selection(page / 2)),
            KeyCode::Char('u') if control => Some(self.history.move_selection(-page / 2)),
            KeyCode::Char('j') | KeyCode::Down => Some(self.history.move_selection(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(self.history.move_selection(-1)),
            KeyCode::PageDown => Some(self.history.move_selection(page)),
            KeyCode::PageUp => Some(self.history.move_selection(-page)),
            KeyCode::Char('g') | KeyCode::Home => Some(self.history.select(0)),
            KeyCode::Char('G') | KeyCode::End => Some(self.history.select(u32::MAX)),
            // HEAD・親・子へ（アプリの Navigate）。`]` は下（古い方）、`[` は上へ動く。
            KeyCode::Char('@') => Some(self.history.go_to_head()),
            KeyCode::Char('o') => {
                self.toggle_history_order();
                None
            }
            // マージ済みのブランチを折りたたむ・戻す。
            KeyCode::Char('z') => {
                self.fold = !self.fold;
                self.expanded.clear();
                self.notice = Some(match self.fold {
                    true => tr!(
                        "Merged branches folded: a merge shows how many commits it brought in; Z opens one"
                    ),
                    false => tr!("Merged branches shown"),
                });
                self.dirty = true;
                self.open();
                None
            }
            // 折りたたんだマージの、取り込んだ側を開く・閉じる。
            KeyCode::Char('Z') if self.fold => {
                if let Some(id) = self.history.selected_id() {
                    let id = id.to_string();
                    match self.expanded.iter().position(|e| *e == id) {
                        Some(i) => {
                            self.expanded.remove(i);
                        }
                        None if self.history.selected_folded() > 0 => self.expanded.push(id),
                        None => self.notice = Some(tr!("This commit has nothing folded")),
                    }
                    self.dirty = true;
                    self.open();
                }
                None
            }
            KeyCode::Char(']') => Some(self.history.go_to_parent()),
            KeyCode::Char('[') => Some(self.history.go_to_child()),
            _ => None,
        };
        if let Some(moved) = moved {
            if moved {
                self.damage |= Damage::MAIN;
            }
            return false;
        }
        match code {
            KeyCode::Char('/') => {
                // 前の語から直せるよう、入れておく。
                self.typing_query =
                    Some(self.search.as_ref().map_or(String::new(), |s| s.0.clone()));
                true
            }
            // 選んでいるコミットを今のブランチに持ってくる・打ち消す・そこをチェックアウトする（先に確かめる）。
            KeyCode::Char('y' | 'v' | 'c' | 'e' | 'F' | 'X') => self.ask_commit_operation(code),
            // 選んでいるコミットから HEAD までを、編集モードで。
            KeyCode::Char('E') => {
                match self.history.selected_id() {
                    Some(id) => self.send(Request::EditRows {
                        from: id.to_string(),
                    }),
                    None => return false,
                }
                true
            }
            KeyCode::Char('n') => self.history.next_match(true),
            KeyCode::Char('N') => self.history.next_match(false),
            KeyCode::Esc => self.clear_search(),
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Sidebar;
                true
            }
            KeyCode::Enter | KeyCode::Char(' ' | 'l') | KeyCode::Right => {
                self.focus = Focus::Side;
                true
            }
            _ => false,
        }
    }

    fn details_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.details.page();
        // 動かすだけなら、詳細の中だけが変わる。
        let scrolled = match code {
            KeyCode::Char('d') if control => Some(self.details.scroll(page / 2)),
            KeyCode::Char('u') if control => Some(self.details.scroll(-page / 2)),
            KeyCode::Char('j') | KeyCode::Down => Some(self.details.scroll(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(self.details.scroll(-1)),
            KeyCode::PageDown | KeyCode::Char(' ') => Some(self.details.scroll(page)),
            KeyCode::PageUp => Some(self.details.scroll(-page)),
            KeyCode::Char('g') | KeyCode::Home => Some(self.details.scroll_to(0)),
            KeyCode::Char('G') | KeyCode::End => Some(self.details.scroll_to(u32::MAX)),
            KeyCode::Char('l') | KeyCode::Right => Some(self.details.pan(PAN)),
            // 左の端にいるなら、動かさずに一覧へ戻る（下で）。
            KeyCode::Char('h') | KeyCode::Left if self.details.panned() => {
                Some(self.details.pan(-PAN))
            }
            KeyCode::Char(']') => Some(self.details.jump_file(true)),
            KeyCode::Char('[') => Some(self.details.jump_file(false)),
            _ => None,
        };
        if let Some(scrolled) = scrolled {
            if scrolled {
                self.damage |= Damage::SIDE;
            }
            return false;
        }
        match code {
            // 今見ているファイルの Blame（そのコミットの時点の）・File History を開く。
            KeyCode::Char('B' | 'L') if self.view != View::File => {
                match self.details.current_file() {
                    Some((path, id)) => {
                        let blame = code == KeyCode::Char('B');
                        let path = path.to_owned();
                        self.file.open(&path, blame, blame.then(|| id.to_string()));
                        self.file_back = self.view;
                        self.view = View::File;
                        self.focus = Focus::Main;
                        true
                    }
                    None => false,
                }
            }
            KeyCode::Esc | KeyCode::Char('q' | 'h') | KeyCode::Backspace | KeyCode::Left => {
                self.focus = Focus::Main;
                true
            }
            _ => false,
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) {
        // 確かめている間は、キーで答えるまで動かさない。
        if self.confirm.is_some() || self.choices.is_some() || !self.mouse {
            return;
        }
        let layout = self.layout();
        let inside = |area: Option<Rect>| {
            area.filter(|a| {
                a.x <= mouse.column
                    && mouse.column < a.right()
                    && a.y <= mouse.row
                    && mouse.row < a.bottom()
            })
        };
        let wheel = match mouse.kind {
            MouseEventKind::ScrollDown => Some(WHEEL_ROWS),
            MouseEventKind::ScrollUp => Some(-WHEEL_ROWS),
            _ => None,
        };
        let click = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
        let view = self.view;
        let pane = if inside(layout.sidebar).is_some() {
            Damage::SIDEBAR
        } else if inside(layout.main).is_some() {
            Damage::MAIN
        } else {
            Damage::SIDE
        };
        let changed = if let Some(area) = inside(layout.sidebar) {
            match wheel {
                Some(rows) => self.sidebar.scroll(rows),
                None if click => {
                    self.focus = Focus::Sidebar;
                    if self.sidebar.click(mouse.row - area.y) {
                        self.pick();
                    }
                    true
                }
                None => false,
            }
        } else if let Some(area) = inside(layout.main) {
            match wheel {
                Some(rows) => match view {
                    View::History => self.history.scroll(rows),
                    View::Reflog => self.reflog.scroll(rows),
                    View::File => self.file.scroll(rows),
                    View::Changes => self.changes.scroll_list(rows),
                },
                None if click => {
                    let focused = std::mem::replace(&mut self.focus, Focus::Main);
                    let line = mouse.row - area.y;
                    let selected = match view {
                        View::History => self.history.click(line),
                        View::Reflog => self.reflog.click(line),
                        View::File => self.file.click(line),
                        // 一覧の下は、メッセージの欄。
                        View::Changes if line >= self.changes.list_height() => {
                            self.focus = Focus::Message;
                            false
                        }
                        View::Changes => self.changes.click(line),
                    };
                    selected || focused != self.focus
                }
                None => false,
            }
        } else if let Some(area) = inside(layout.side) {
            match wheel {
                Some(rows) if view == View::Changes => self.changes.scroll(rows),
                Some(rows) => self.details.scroll(rows),
                None if click => {
                    let focused = std::mem::replace(&mut self.focus, Focus::Side);
                    let selected =
                        view == View::Changes && self.changes.click_diff(mouse.row - area.y);
                    selected || focused != Focus::Side
                }
                None => false,
            }
        } else {
            false
        };
        // ホイールで動くのは、そのペインの中だけ。
        match wheel {
            Some(_) if changed => self.damage |= pane,
            _ => self.dirty |= changed,
        }
    }

    /// `damage` の所を描き（`areas` が返す範囲。呼ぶ側が消してある）、カーソルを置く位置を返す。
    pub fn draw(&mut self, buf: &mut Buffer, damage: Damage) -> Option<(u16, u16)> {
        let area = buf.area;
        if area.height < BARS + 1 {
            return None;
        }
        let all = damage == Damage::ALL;
        let mut cursor = None;
        let bar = Style::new().add_modifier(Modifier::REVERSED);

        // 一番上: リポジトリ、ブランチ、読んでいる間の印。
        let top = Rect::new(area.x, area.y, area.width, 1);
        buf.set_style(top, bar);
        let x = buf
            .set_stringn(
                area.x + 1,
                area.y,
                &self.name,
                usize::from(area.width.saturating_sub(2)),
                bar.add_modifier(Modifier::BOLD),
            )
            .0;
        let mut x = x;
        if let Some(branch) = &self.branch {
            x = buf
                .set_stringn(
                    x + 2,
                    area.y,
                    branch,
                    usize::from(area.right().saturating_sub(x + 3)),
                    bar,
                )
                .0;
            // 上流より進んでいる数と、遅れている数。
            let mut counts = String::new();
            if self.ahead > 0 {
                counts.push_str(&format!(" ↑{}", self.ahead));
            }
            if self.behind > 0 {
                counts.push_str(&format!(" ↓{}", self.behind));
            }
            if !counts.is_empty() && x + 1 < area.right() {
                x = buf
                    .set_stringn(x, area.y, counts, usize::from(area.right() - x - 1), bar)
                    .0;
            }
        }
        // サイドバーが出ていなければ、メインの画面の切り替え（数字のキー）を並べる。今の画面は太字。
        let tabs = self.layout().sidebar.is_none();
        let changes = match self.changes.count() {
            0 => tr!("1 Changes"),
            count => tr!("1 Changes ({count})", count = count),
        };
        for (view, label) in [
            (View::Changes, changes),
            (View::History, tr!("2 History")),
            (View::Reflog, tr!("3 Reflog")),
        ] {
            if !tabs || x + 3 >= area.right() {
                break;
            }
            let style = match self.view == view {
                true => bar.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                false => bar,
            };
            x = buf
                .set_stringn(
                    x + 3,
                    area.y,
                    label,
                    usize::from(area.right().saturating_sub(x + 4)),
                    style,
                )
                .0;
        }
        if self.busy() {
            // 動いている操作があれば、その名前。
            let loading = self.running.clone().unwrap_or_else(|| tr!("Loading…"));
            let width = loading.width() as u16 + 1;
            if width < area.width {
                buf.set_string(area.right() - width, area.y, loading, bar);
            }
        }

        let layout = self.layout();
        if let Some(at) = layout.sidebar.filter(|_| damage.has(Damage::SIDEBAR)) {
            self.sidebar
                .draw(at, buf, self.focus == Focus::Sidebar, self.changes.count());
            // メインの画面と並べたときは、間に区切りの線。
            if all && at.right() < area.right() {
                let line = Style::new().fg(Color::Indexed(238));
                for y in at.y..at.bottom() {
                    buf[(at.right(), y)].set_char('│').set_style(line);
                }
            }
        }
        let focused = self.focus == Focus::Main;
        if layout.main.is_none() {
            self.main_cursor = None;
        }
        if let Some(at) = layout.main.filter(|_| damage.has(Damage::MAIN)) {
            self.main_cursor = None;
            match self.view {
                View::History => self.history.draw(at, buf, self.clock.now, focused),
                View::Reflog => self.reflog.draw(at, buf, self.clock.now, focused),
                View::File => self.file.draw(at, buf, self.clock.now, focused),
                View::Changes => {
                    self.main_cursor =
                        self.changes
                            .draw_main(at, buf, focused, self.focus == Focus::Message);
                }
            }
        }
        cursor = cursor.or(self.main_cursor);
        if let Some(at) = layout.side.filter(|_| damage.has(Damage::SIDE)) {
            // 一覧と並べたときは、間に区切りの線。
            if all && layout.main.is_some() {
                let line = Style::new().fg(Color::Indexed(if focused { 238 } else { 244 }));
                for y in at.y..at.bottom() {
                    buf[(at.x - 1, y)].set_char('│').set_style(line);
                }
            }
            match self.view {
                View::History | View::Reflog | View::File => {
                    self.details.draw(at, buf, &self.clock, &self.theme);
                }
                View::Changes => self.changes.draw_diff(at, buf, &self.theme, !focused),
            }
        }

        // 一番下: エラーか、キーの案内。右に、今の位置（一覧は選んでいる行、詳細と差分は見えている最後の行）。
        let y = area.bottom() - 1;
        let (at, of) = match (self.view, self.focus) {
            (View::History, Focus::Main) => (
                (self.history.selected() + 1).min(self.history.total()),
                self.history.total(),
            ),
            (View::History | View::Reflog | View::File, Focus::Side) => self.details.position(),
            (View::File, Focus::Main | Focus::Sidebar) => self.file.position(),
            (View::Reflog, Focus::Main | Focus::Sidebar) => self.reflog.position(),
            (View::Changes, Focus::Main) => self.changes.list_position(),
            (View::Changes, Focus::Side) => self.changes.diff_position(),
            (_, Focus::Message) => self.changes.list_position(),
            (View::History, Focus::Sidebar) => (
                (self.history.selected() + 1).min(self.history.total()),
                self.history.total(),
            ),
            (View::Changes, Focus::Sidebar) => self.changes.list_position(),
        };
        let position = format!("{at}/{of}");
        let position_x = area
            .right()
            .saturating_sub(position.width() as u16 + 1)
            .max(area.x);
        let room = usize::from(position_x.saturating_sub(area.x + 2));
        #[cfg(unix)]
        let asked = self.questions.front();
        match (&self.confirm, &self.error) {
            // いくつかから選ぶ: 文と、選択肢のキーと名前。
            _ if self.choices.is_some() => {
                let (ask, options) = self.choices.as_ref().unwrap();
                let mut x =
                    buf.set_stringn(
                        area.x + 1,
                        y,
                        ask,
                        room,
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                    .0 + 3;
                let cancel = ('n', tr!("cancel"));
                for (key, label) in options
                    .iter()
                    .map(|o| (o.0, o.1.as_str()))
                    .chain([(cancel.0, cancel.1.as_str())])
                {
                    if x >= position_x {
                        break;
                    }
                    x =
                        buf.set_stringn(
                            x,
                            y,
                            format!("{key} {label}"),
                            usize::from(position_x - x),
                            Style::new(),
                        )
                        .0 + 3;
                }
            }
            // 尋ねられていること: 文と、打った答え（パスワードは隠す）。
            #[cfg(unix)]
            _ if asked.is_some() => {
                let question = asked.unwrap();
                let shown = match question.secret() {
                    true => "•".repeat(self.answer.chars().count()),
                    false => self.answer.clone(),
                };
                let x = buf
                    .set_stringn(
                        area.x + 1,
                        y,
                        question.prompt.trim(),
                        room,
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                    .0;
                let x = buf
                    .set_stringn(
                        x + 1,
                        y,
                        shown,
                        usize::from(position_x.saturating_sub(x + 2)),
                        Style::new(),
                    )
                    .0;
                cursor = Some((x.min(position_x.saturating_sub(1)), y));
            }
            (Some((ask, yes, _)), _) => {
                let x = buf
                    .set_stringn(
                        area.x + 1,
                        y,
                        ask,
                        room,
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                    .0;
                buf.set_stringn(
                    x + 3,
                    y,
                    match self.push_to_offered {
                        true => tr!("y {yes}   t another branch…   n cancel", yes = yes),
                        false => tr!("y {yes}   n cancel", yes = yes),
                    },
                    usize::from(position_x.saturating_sub(x + 3)),
                    Style::new(),
                );
            }
            (None, Some(error)) => {
                buf.set_stringn(area.x + 1, y, error, room, Style::new().fg(Color::Red));
            }
            (None, None) if self.typing_branch.is_some() => {
                let (_, at, name) = self.typing_branch.as_ref().unwrap();
                let text = match self.naming {
                    Naming::Branch => tr!("New branch at {at}: {name}", at = at, name = name),
                    Naming::Tag => tr!("New tag at {at}: {name}", at = at, name = name),
                    Naming::StashBranch => tr!(
                        "New branch where {at} was made, with it applied: {name}",
                        at = at,
                        name = name
                    ),
                    Naming::Rename => tr!("Rename {at} to: {name}", at = at, name = name),
                    Naming::Stash => tr!("Stash message (Enter for none): {name}", name = name),
                    Naming::Worktree => {
                        tr!("New worktree for {at} at: {name}", at = at, name = name)
                    }
                    Naming::PushTo => {
                        tr!("Push {at} to (remote/branch): {name}", at = at, name = name)
                    }
                    Naming::Orphan => tr!("New orphan branch: {name}", name = name),
                    Naming::RemoteName => tr!("New remote's name: {name}", name = name),
                    Naming::RemoteUrl => tr!("URL of {at}: {name}", at = at, name = name),
                    Naming::BundleFile => tr!("Fetch from the bundle at: {name}", name = name),
                    Naming::Patch => tr!("Apply the patch at: {name}", name = name),
                    Naming::SavePatch => {
                        tr!(
                            "Save {at} as a patch in the folder: {name}",
                            at = at,
                            name = name
                        )
                    }
                    Naming::BundleName => {
                        tr!("Fetch its branches as remote: {name}", name = name)
                    }
                    Naming::Bundle => {
                        tr!("Save the repository as a bundle at: {name}", name = name)
                    }
                    Naming::Archive => {
                        tr!(
                            "Export {at} to (.zip, .tar.gz or .tar): {name}",
                            at = at,
                            name = name
                        )
                    }
                };
                let x = buf.set_stringn(area.x + 1, y, text, room, Style::new()).0;
                cursor = Some((x.min(position_x.saturating_sub(1)), y));
            }
            (None, None) if self.typing_filter.is_some() => {
                let filter = self.typing_filter.as_deref().unwrap_or("");
                let text = tr!("Filter: {filter}", filter = filter);
                let x = buf.set_stringn(area.x + 1, y, text, room, Style::new()).0;
                cursor = Some((x.min(position_x.saturating_sub(1)), y));
            }
            (None, None) if self.typing_query.is_some() => {
                let query = self.typing_query.as_deref().unwrap_or("");
                let x = buf
                    .set_stringn(area.x + 1, y, format!("/{query}"), room, Style::new())
                    .0;
                cursor = Some((x.min(position_x.saturating_sub(1)), y));
            }
            (None, None) if self.notice.is_some() => {
                let notice = self.notice.as_deref().unwrap_or("");
                buf.set_stringn(area.x + 1, y, notice, room, Style::new().fg(Color::Green));
            }
            // 途中の操作: 「Rebase 3/7 · 2 conflicts」と、その操作のキー。
            (None, None) if self.sequence.is_some() => {
                let (kind, progress) = self.sequence.as_ref().unwrap();
                let mut state = kind.clone();
                if let Some((current, total)) = progress {
                    state.push_str(&format!(" {current}/{total}"));
                }
                match self.changes.conflicts() {
                    0 => {}
                    1 => state.push_str(&tr!(" · 1 conflict")),
                    count => state.push_str(&tr!(" · {count} conflicts", count = count)),
                }
                let x = buf
                    .set_stringn(
                        area.x + 1,
                        y,
                        state,
                        room,
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                    .0;
                buf.set_stringn(
                    x + 3,
                    y,
                    tr!("C continue   K skip   A abort"),
                    usize::from(position_x.saturating_sub(x + 3)),
                    Style::new(),
                );
            }
            (None, None) => {
                let hints = match (self.view, self.focus) {
                    (View::History, Focus::Main) => match &self.search {
                        // 検索している間は、語と、合った行のうち何番目か。
                        Some((query, running)) => {
                            let (at, of) = self.history.match_position();
                            let found = match (running, at) {
                                // 動いている間は、ここまでに合った数（と、内容の検索なら調べ終えた割合）。
                                (Some(_), _) if self.search_done > 0.0 => tr!(
                                    "searching… {percent}%, {count} so far",
                                    percent = (self.search_done * 100.0) as u32,
                                    count = of
                                ),
                                (Some(_), _) if of > 0 => {
                                    tr!("searching… {count} so far", count = of)
                                }
                                (Some(_), _) => tr!("searching…"),
                                (None, Some(at)) => format!("{at}/{of}"),
                                (None, None) => of.to_string(),
                            };
                            tr!(
                                "/{query}: {found}   n/N next/previous   Esc clear",
                                query = query,
                                found = found
                            )
                        }
                        None => tr!("j/k move   / search   Enter details   0 sidebar   q quit"),
                    },
                    (View::File, Focus::Main) => {
                        tr!(
                            "{title}   j/k move   Enter details   H in History   Esc back",
                            title = self.file.title()
                        )
                    }
                    (View::History | View::Reflog | View::File, Focus::Side) => {
                        tr!("j/k scroll   [/] file   h/l pan   Esc back")
                    }
                    (View::Reflog, Focus::Main) => match self.reflog.name() {
                        Some(name) => tr!(
                            "Reflog of {name}   j/k move   Enter details   3 HEAD’s reflog",
                            name = name
                        ),
                        None => {
                            tr!("j/k move   Enter details   b new branch   0 sidebar   q quit")
                        }
                    },
                    (View::Changes, Focus::Main) => {
                        tr!(
                            "j/k move   Space stage/unstage   c message   a amend   d discard   q quit"
                        )
                    }
                    (View::Changes, Focus::Side) => {
                        tr!("j/k line   [/] hunk   Space stage/unstage   d discard   Esc back")
                    }
                    (_, Focus::Message) => {
                        tr!("Ctrl-s commit   Ctrl-p commit and push   Ctrl-e editor   Esc back")
                    }
                    (_, Focus::Sidebar) => {
                        tr!("j/k move   Enter open   / filter   ? keys   Esc back   q quit")
                    }
                };
                buf.set_stringn(
                    area.x + 1,
                    y,
                    hints,
                    room,
                    Style::new().fg(Color::Indexed(244)),
                );
            }
        }
        buf.set_string(position_x, y, position, Style::new());
        if self.help {
            cursor = None;
            let lines = self.help_lines();
            let keys = lines.iter().map(|l| l.0.width()).max().unwrap_or(0) as u16;
            let text = lines.iter().map(|l| l.1.width()).max().unwrap_or(0) as u16;
            // 枠は画面の真ん中に。収まらない分は切る。
            let width = (keys + text + 7).min(area.width);
            let height = (lines.len() as u16 + 2).min(area.height);
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            frame_box(buf, area, at, &tr!(" Keys "));
            // 収まらない分は、`j` `k` で動かす。
            let inside = usize::from(height.saturating_sub(2));
            self.help_top = self.help_top.min(lines.len().saturating_sub(inside));
            if lines.len() > inside {
                let more = tr!(" j/k more ");
                let x = at.right().saturating_sub(more.width() as u16 + 2);
                if x > at.x + 8 {
                    buf.set_string(
                        x,
                        at.bottom() - 1,
                        more,
                        Style::new().fg(Color::Indexed(244)),
                    );
                }
            }
            for (y, (key, text)) in
                (at.y + 1..at.bottom().saturating_sub(1)).zip(lines.iter().skip(self.help_top))
            {
                buf.set_stringn(
                    at.x + 2,
                    y,
                    key,
                    usize::from(width.saturating_sub(4)),
                    Style::new().fg(Color::Cyan),
                );
                if width > keys + 6 {
                    buf.set_stringn(
                        at.x + 4 + keys,
                        y,
                        text,
                        usize::from(width - keys - 6),
                        Style::new(),
                    );
                }
            }
        }
        if let Some(mode) = &self.edit_mode {
            // 一番下の行を残して、画面を覆う枠。
            let at = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
            frame_box(buf, area, at, &tr!(" Edit Commits "));
            if at.width > 4 && at.height > 2 {
                let inner = Rect::new(at.x + 2, at.y + 1, at.width - 4, at.height - 2);
                for y in inner.y..inner.bottom() {
                    for x in inner.x..inner.right() {
                        buf[(x, y)].reset();
                    }
                }
                mode.draw(inner, buf);
            }
        }
        // 確かめている間は、前と後の絵を重ねる。
        if let Some((title, drawing)) = self
            .question_drawing
            .as_ref()
            .filter(|_| self.confirm.is_some() || self.choices.is_some())
        {
            let width = (crate::drawing::width(drawing) + 4).max(48).min(area.width);
            let height = (crate::drawing::height(drawing) + 2).min(area.height.saturating_sub(1));
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height.saturating_sub(1) - height) / 2,
                width,
                height,
            );
            frame_box(buf, area, at, title);
            if at.width > 4 && at.height > 2 {
                let inner = Rect::new(at.x + 2, at.y + 1, at.width - 4, at.height - 2);
                for y in inner.y..inner.bottom() {
                    for x in inner.x..inner.right() {
                        buf[(x, y)].reset();
                    }
                }
                crate::drawing::draw(drawing, inner, buf);
            }
        }
        if let Some(log) = &self.operation_log {
            let times: Vec<String> = log
                .entries
                .iter()
                .map(|(_, time)| crate::history::ago(self.clock.now, *time))
                .collect();
            let label_width = log
                .entries
                .iter()
                .map(|(l, _)| l.width())
                .max()
                .unwrap_or(0) as u16;
            let hint = tr!("Enter undoes back to the selected one");
            let width = (label_width + 10)
                .max(hint.width() as u16 + 4)
                .max(36)
                .min(area.width);
            // 枠・記録の行・案内の行。
            let height = (log.entries.len() as u16 + 3).min(area.height);
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            frame_box(buf, area, at, &tr!(" Operation Log "));
            let inner = usize::from(width.saturating_sub(4));
            let rows = usize::from(height.saturating_sub(3));
            let top = (log.selected + 1).saturating_sub(rows);
            for (y, i) in (at.y + 1..at.bottom().saturating_sub(2)).zip(top..log.entries.len()) {
                // 選んでいる記録まで（戻るもの）を、すべて反転する。
                let undone = i <= log.selected;
                let style = match undone {
                    true => Style::new().add_modifier(Modifier::REVERSED),
                    false => Style::new(),
                };
                if undone {
                    for x in at.x + 1..at.right() - 1 {
                        buf[(x, y)].set_style(style);
                    }
                }
                let time_style = match undone {
                    true => style,
                    false => Style::new().fg(Color::Indexed(244)),
                };
                buf.set_stringn(at.x + 2, y, &times[i], 4, time_style);
                buf.set_stringn(
                    at.x + 7,
                    y,
                    &log.entries[i].0,
                    inner.saturating_sub(5),
                    style,
                );
            }
            if height >= 3 {
                buf.set_stringn(
                    at.x + 2,
                    at.bottom() - 2,
                    &hint,
                    inner,
                    Style::new().fg(Color::Indexed(244)),
                );
            }
        }
        if let Some(palette) = &self.palette {
            let entries = self.palette_entries(&palette.query);
            let keys: Vec<String> = entries.iter().map(|(e, _)| e.key_label()).collect();
            let key_width = keys.iter().map(|k| k.width()).max().unwrap_or(0) as u16;
            let title_width = entries.iter().map(|(_, t)| t.width()).max().unwrap_or(0) as u16;
            let none = tr!("No command matches");
            let wanted = (title_width + key_width + 7)
                .max(none.width() as u16 + 4)
                .max(palette.query.width() as u16 + 7)
                .max(36);
            let width = wanted.min(area.width);
            // 枠・打っている語の行・操作の行（なければ、ないと書く 1 行）。
            let height = (entries.len().max(1) as u16 + 3).min(area.height);
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            frame_box(buf, area, at, &tr!(" Commands "));
            let inner = usize::from(width.saturating_sub(4));
            if height >= 3 {
                let x = buf
                    .set_stringn(at.x + 2, at.y + 1, ":", inner, Style::new())
                    .0;
                let x = buf
                    .set_stringn(
                        x,
                        at.y + 1,
                        &palette.query,
                        inner.saturating_sub(1),
                        Style::new(),
                    )
                    .0;
                cursor = Some((x.min(at.right().saturating_sub(2)), at.y + 1));
            }
            // 収まらない分は、選んでいる行が見えるように送る。
            let rows = usize::from(height.saturating_sub(3));
            let top = (palette.selected + 1).saturating_sub(rows);
            if entries.is_empty() && rows > 0 {
                buf.set_stringn(
                    at.x + 2,
                    at.y + 2,
                    none,
                    inner,
                    Style::new().fg(Color::Indexed(244)),
                );
            }
            for (y, i) in (at.y + 2..at.bottom().saturating_sub(1)).zip(top..entries.len()) {
                let selected = i == palette.selected;
                let style = match selected {
                    true => Style::new().add_modifier(Modifier::REVERSED),
                    false => Style::new(),
                };
                if selected {
                    for x in at.x + 1..at.right() - 1 {
                        buf[(x, y)].set_style(style);
                    }
                }
                buf.set_stringn(at.x + 2, y, &entries[i].1, inner, style);
                let key = &keys[i];
                let key_x = at.right().saturating_sub(key.width() as u16 + 2);
                if key_x > at.x + 2 + entries[i].1.width() as u16 {
                    let style = match selected {
                        true => style,
                        false => Style::new().fg(Color::Cyan),
                    };
                    buf.set_string(key_x, y, key, style);
                }
            }
        }
        // 入力しているときだけ、カーソルを出す。
        cursor
    }
}

/// この機械のファイルのパス: 相対なら、TUI を始めたフォルダから。
fn local_path(file: &str) -> String {
    let path = std::path::Path::new(file);
    match path.is_absolute() {
        true => file.to_owned(),
        false => (std::env::current_dir().unwrap_or_default().join(path))
            .to_string_lossy()
            .into_owned(),
    }
}

/// 画面に重ねる枠を描く（中は空に。題は上の線に）。
fn frame_box(buf: &mut Buffer, area: Rect, at: Rect, title: &str) {
    for y in at.y..at.bottom() {
        // 枠の左端が全角の文字の後ろ半分に重なるなら、その文字を消す。
        if at.x > area.x && buf[(at.x - 1, y)].symbol().width() > 1 {
            buf[(at.x - 1, y)].set_char(' ');
        }
        for x in at.x..at.right() {
            let edge_y = y == at.y || y == at.bottom() - 1;
            let edge_x = x == at.x || x == at.right() - 1;
            let ch = match (edge_y, edge_x) {
                (true, true) if y == at.y && x == at.x => '┌',
                (true, true) if y == at.y => '┐',
                (true, true) if x == at.x => '└',
                (true, true) => '┘',
                (true, false) => '─',
                (false, true) => '│',
                (false, false) => ' ',
            };
            buf[(x, y)].reset();
            buf[(x, y)].set_char(ch);
        }
    }
    buf.set_stringn(
        at.x + 2,
        at.y,
        title,
        usize::from(at.width.saturating_sub(4)),
        Style::new().add_modifier(Modifier::BOLD),
    );
}

/// `predictMerge` の `conflicts`（ファイルの配列。git が古くて調べられなければ null）。
fn conflict_files(conflicts: &Value) -> Option<Vec<String>> {
    conflicts.as_array().map(|files| {
        files
            .iter()
            .filter_map(|f| f.as_str().map(str::to_owned))
            .collect()
    })
}

/// 書き換えの見通し（`rewritePreview` の `info`）から、コンフリクトしそうなファイルをまとめて（分からなければ
/// `None`）。
fn rewrite_conflicts(info: &Value) -> Option<Vec<String>> {
    info["conflicts"].as_array().map(|commits| {
        let mut files: Vec<String> = commits
            .iter()
            .flat_map(|c| c["files"].as_array().cloned().unwrap_or_default())
            .filter_map(|f| f.as_str().map(str::to_owned))
            .collect();
        files.sort();
        files.dedup();
        files
    })
}

/// 書き換えの見通しから、一緒に動くブランチと、push に Force Push が要るかを、確かめる文に足す文にする。
fn rewrite_notes(info: &Value) -> String {
    let moving: Vec<&str> = (info["moving"].as_array().into_iter().flatten())
        .filter_map(|b| b["name"].as_str())
        .collect();
    let mut notes = String::new();
    if !moving.is_empty() {
        notes += &tr!(
            " {names} move along.",
            names = amonhen_model::i18n::list(&moving)
        );
    }
    if info["currentPushed"] == true {
        notes +=
            &tr!(" Its upstream has the commits it rewrites, so pushing will need Force Push.");
    }
    notes
}

/// コンフリクトの見込みを、確かめる文に足す一文にする（分からなければ空）。
fn risk_text(conflicts: Option<Vec<String>>) -> String {
    match conflicts {
        Some(files) if !files.is_empty() => {
            let shown: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
            let mut names = amonhen_model::i18n::list(&shown);
            if files.len() > 3 {
                names = tr!(
                    "{names} and {count} more",
                    names = names,
                    count = files.len() - 3
                );
            }
            tr!(" Conflicts are likely in {files}.", files = names)
        }
        Some(_) => tr!(" No conflicts are expected."),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use amonhen_front::InProcess;

    use super::*;

    /// コミットが 6 つの History を開いたアプリ。
    fn app_with_history() -> (tempfile::TempDir, App, std::sync::mpsc::Receiver<Event>) {
        amonhen_model::testing::isolate_from_user_settings();
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = amonhen_model::process::command("git")
                .args(["-c", "user.name=Ann", "-c", "user.email=ann@example.com"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        for i in 0..6 {
            git(&[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("Commit {i}"),
            ]);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let clock = Clock {
            now: 0,
            local: |t| t.to_string(),
        };
        let mut app = App::new(
            Arc::new(InProcess::open(dir.path(), None).unwrap()),
            tx,
            clock,
        );
        app.resize(100, 12);
        app.start();
        crate::settle(&mut app, &rx).unwrap();
        (dir, app, rx)
    }

    fn bottom_line(app: &mut App) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 12));
        app.draw(&mut buf, Damage::ALL);
        (0..100).map(|x| buf[(x, 11)].symbol()).collect()
    }

    /// 編集モードの `r`: 行のメッセージを git のエディタで直す（`#` の行は捨てる）。当てるまで何も変わらない。
    #[test]
    fn edit_mode_rewords_through_the_editor() {
        let (_dir, mut app, rx) = app_with_history();
        let key = |app: &mut App, ch: char| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            ))));
        };
        key(&mut app, '2');
        key(&mut app, 'j');
        key(&mut app, 'E');
        crate::settle(&mut app, &rx).unwrap();
        assert_eq!(app.edit_mode.as_ref().map(|m| m.rows.len()), Some(2));
        key(&mut app, 'r');
        let (text, _) = app.take_edit_message().expect("the message to edit");
        assert_eq!(text.trim(), "Commit 5");
        app.message_edited(Ok("Better\n# a comment\n".to_owned()));
        let mode = app.edit_mode.as_ref().unwrap();
        assert_eq!(mode.rows[0].action, crate::editmode::Action::Reword);
        assert_eq!(mode.rows[0].new_message.as_deref(), Some("Better"));
        assert!(app.take_edit_message().is_none());
    }

    /// サイドバーの worktree で Enter: ほかの worktree なら TUI を閉じてそのパスを返し、開いているものなら場所を
    /// 知らせるだけ。
    #[test]
    fn enter_on_another_worktree_reopens_there() {
        let (dir, mut app, rx) = app_with_history();
        let other = tempfile::tempdir().unwrap();
        let path = other.path().join("other");
        let out = amonhen_model::process::command("git")
            .args(["worktree", "add", "-q", "-b", "topic"])
            .arg(&path)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        app.open();
        crate::settle(&mut app, &rx).unwrap();
        let key = |app: &mut App, code: KeyCode| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                code,
                KeyModifiers::NONE,
            ))));
        };
        // 一番下の行が other。その上が、開いているもの。
        for code in [
            KeyCode::Char('0'),
            KeyCode::Char('G'),
            KeyCode::Char('k'),
            KeyCode::Enter,
        ] {
            key(&mut app, code);
        }
        assert!(app.take_open_worktree().is_none());
        assert!(!app.quit());
        assert!(bottom_line(&mut app).contains("This is the worktree that is open"));
        for code in [KeyCode::Char('j'), KeyCode::Enter] {
            key(&mut app, code);
        }
        let opened = app.take_open_worktree().expect("a worktree to open");
        assert_eq!(
            std::fs::canonicalize(opened).unwrap(),
            std::fs::canonicalize(&path).unwrap()
        );
        assert!(app.quit());
    }

    /// Working Copy の `e`: 選んでいるファイルを、作業ツリーの絶対パスでエディタに渡す。`ssh://` の先なら断る。
    #[test]
    fn e_opens_the_selected_file_only_when_the_files_are_here() {
        let (dir, mut app, rx) = app_with_history();
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        // 変化の知らせは来ないので、読み直させる。
        app.changes.refresh();
        let key = |app: &mut App, ch: char| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            ))));
        };
        key(&mut app, '1');
        crate::settle(&mut app, &rx).unwrap();
        key(&mut app, 'e');
        let (file, workdir) = app.take_open_file().expect("a file to open");
        assert_eq!(
            std::fs::canonicalize(&file).unwrap(),
            std::fs::canonicalize(dir.path().join("a.txt")).unwrap()
        );
        assert!(workdir.is_some());
        assert!(app.take_open_file().is_none());
        // ほかの機械のファイルは開かない。
        app.set_files_here(false);
        key(&mut app, 'e');
        assert!(app.take_open_file().is_none());
        assert!(bottom_line(&mut app).contains("The files are on another machine"));
    }

    /// 動いている検索の途中の結果: 最初に届いた分で合った行へ移り、後から届く分では動かさない。一番下の行に、
    /// ここまでの数と割合を出す。
    #[test]
    fn partial_matches_show_while_the_search_runs() {
        let (_dir, mut app, _rx) = app_with_history();
        let partial = |query: &str, rows: &[u32], done: f32| Event::Partial {
            query: query.to_owned(),
            rows: rows.to_vec(),
            done,
        };
        app.search = Some(("x".to_owned(), Some(Arc::new(AtomicBool::new(false)))));
        app.history.begin_search();
        assert!(bottom_line(&mut app).contains("/x: searching…   n/N"));

        app.handle(partial("x", &[2, 4], 0.0));
        assert_eq!(app.history.selected(), 2);
        assert!(bottom_line(&mut app).contains("/x: searching… 2 so far"));

        // 見ている所を動かしたら、後から届く分はそのまま。
        app.handle(Event::Input(Input::Key(KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.history.selected(), 3);
        app.handle(partial("x", &[1, 2, 4], 0.5));
        assert_eq!(app.history.selected(), 3);
        assert_eq!(app.history.match_position(), (None, 3));
        assert!(bottom_line(&mut app).contains("/x: searching… 50%, 3 so far"));

        // ほかの検索のものは捨てる。
        app.handle(partial("y", &[0], 0.9));
        assert_eq!(app.history.match_position(), (None, 3));
    }
}
