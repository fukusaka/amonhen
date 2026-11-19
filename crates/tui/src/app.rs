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
}

/// 選択肢 1 つ: 押すキー、名前、頼む中身。
type Choice = (char, String, Request);

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
    /// 今の検索: 語と、動いている検索を取り消す印（結果が届いたら `None`）。
    search: Option<(String, Option<Arc<AtomicBool>>)>,
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
            search: None,
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
        self.send(Request::Open);
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

    /// 描き直す所があれば、その所（聞いたら、なかったことにする）。キーの一覧はペインに重なるので、出している
    /// 間はすべてを描き直す。
    pub fn take_damage(&mut self) -> Option<Damage> {
        let damage = std::mem::take(&mut self.damage);
        if std::mem::take(&mut self.dirty) || (self.help && damage != Damage::default()) {
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

    /// エディタを閉じた後のメッセージ。だめなら理由を出し、メッセージはそのまま。
    pub fn message_edited(&mut self, edited: Result<String>) {
        self.edit_message = false;
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
                    Loaded::Reflog(reply) => {
                        if let Some(error) = self.reflog.set(reply) {
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
        // 確かめている間は、`y`（か Enter）で行い、ほかのキーでやめる。
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
                KeyCode::Enter => {
                    if let Some((start, _, name)) = self.typing_branch.take()
                        && !name.trim().is_empty()
                    {
                        let name = name.trim();
                        let (op, label, command) = match self.naming {
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
            KeyCode::Char('b') if !control => self.ask_branch(),
            KeyCode::Char('T') => self.ask_name(Naming::Tag),
            KeyCode::Char('R') if self.view != View::Changes && self.focus == Focus::Main => {
                self.ask_reset();
            }
            // stash する前に、何を含めるかを選ばせる。
            KeyCode::Char('s') if !control && self.focus != Focus::Side => {
                let stash = |untracked: bool, keep_index: bool| Request::Run {
                    name: "stash",
                    command: json!({
                        "op": "stashPush", "message": null,
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

    /// メインの画面を替える。
    fn show(&mut self, view: View) {
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

    /// 命令が済んだ。だめなら理由を出し、どちらでも状態・ref・History を読み直す。
    fn ran(&mut self, name: &'static str, result: std::result::Result<Value, String>) {
        self.running = None;
        let reply = match result {
            Ok(reply) => reply,
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
            "merge" | "rebase" | "continue" | "skip" | "cherryPick" | "revert" | "rewrite" => {
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

    /// pull する。上流と分かれているときは、どう取り込むかを選ばせる（ほかは git の設定のとおり）。
    fn ask_pull(&mut self) {
        let pull = |mode: &str| Request::Run {
            name: "pull",
            command: json!({"op": "pull", "mode": mode}),
        };
        if self.ahead > 0 && self.behind > 0 {
            self.choices = Some((
                tr!("Pull: the branch and its upstream have diverged. How?"),
                vec![
                    ('y', tr!("as git is set"), pull("git")),
                    ('m', tr!("merge"), pull("merge")),
                    ('r', tr!("rebase"), pull("rebase")),
                ],
            ));
            self.dirty = true;
            return;
        }
        self.run("pull", tr!("Pulling…"), json!({"op": "pull"}));
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
                ],
            ));
            return;
        }
        self.confirm = Some((ask, tr!("push"), push(remote, remote_branch, false, false)));
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
        let branch = self.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
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
                "Reset {branch} to {commit}? Undo brings it back.",
                branch = branch,
                commit = &id[..id.len().min(8)]
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
        self.confirm = Some(if merge {
            (
                tr!("Merge {name} into {branch}?", name = name, branch = branch),
                tr!("merge"),
                Request::Run {
                    name: "merge",
                    command: json!({"op": "merge", "rev": name}),
                },
            )
        } else {
            (
                tr!("Rebase {branch} onto {name}?", name = name, branch = branch),
                tr!("rebase"),
                Request::Run {
                    name: "rebase",
                    command: json!({"op": "rebase", "onto": name}),
                },
            )
        });
        true
    }

    /// サイドバーで選んでいる stash を戻す（`pop` なら、戻して消す）か、消す（先に確かめる）。
    fn stash_key(&mut self, code: KeyCode) -> bool {
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
                ("n D", tr!("Rename a branch; delete a branch or a tag")),
                ("/", tr!("Filter branches and tags")),
                ("Esc", tr!("Clear the filter, or go back")),
            ],
            (View::History, Focus::Main) => vec![
                ("j k", tr!("Move")),
                ("g G", tr!("Top and bottom")),
                ("PgUp PgDn", tr!("Move by a page")),
                ("Enter Space", tr!("Go to the details")),
                ("/", tr!("Search")),
                ("n N", tr!("Next and previous match")),
                ("y v", tr!("Cherry-pick, revert the selected commit")),
                ("c", tr!("Check out the selected commit (detached HEAD)")),
                ("e", tr!("Edit the commit's message in git's editor")),
                ("F X", tr!("Fix up into the parent; drop the commit")),
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
                ("d", tr!("Discard the file's changes")),
                ("B L", tr!("Blame, File History of the file")),
                (
                    "o t",
                    tr!("Take the current or the incoming side of a conflicted file"),
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
                    "o t b",
                    tr!("In a conflict: take current, incoming, or both"),
                ),
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
            ("s S", tr!("Stash, and pop the newest stash")),
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
            // worktree は、その場所を知らせるだけ（開くのは、そこで `amonhen tui`）。
            Some(Picked::Commit(_))
                if self.sidebar.picked_ref().is_some_and(|r| r.0 == "worktree") =>
            {
                let path = self.sidebar.picked_ref().map_or("", |r| r.1).to_owned();
                self.notice = Some(tr!("Worktree at {path}", path = path));
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
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Sidebar;
                true
            }
            // 選んでいるファイルの Blame・File History を開く。
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
            // コンフリクトの中では、そのコンフリクトを Current・Incoming・両方のどれかにする（Undo できる）。
            KeyCode::Char('o' | 't' | 'b') if self.changes.conflict_at_cursor().is_some() => {
                let choice = match code {
                    KeyCode::Char('o') => "ours",
                    KeyCode::Char('t') => "theirs",
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
                    tr!("y {yes}   n cancel", yes = yes),
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
                    Naming::Rename => tr!("Rename {at} to: {name}", at = at, name = name),
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
                    (View::Reflog, Focus::Main) => {
                        tr!("j/k move   Enter details   b new branch   0 sidebar   q quit")
                    }
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
                tr!(" Keys "),
                usize::from(width.saturating_sub(4)),
                Style::new().add_modifier(Modifier::BOLD),
            );
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
        // 入力しているときだけ、カーソルを出す。
        cursor
    }
}
