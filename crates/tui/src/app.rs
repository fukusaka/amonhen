//! 状態と、イベントの処理と、画面の組み立て。

use std::sync::Arc;
use std::sync::mpsc::Sender;

use amonhen_front::Front;
use amonhen_model::tr;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::terminal::Frame;
use ratatui_crossterm::crossterm::event::{
    Event as Input, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use unicode_width::UnicodeWidthStr;

use crate::Clock;
use crate::changes::Changes;
use crate::details::Details;
use crate::diff::{PAN, Theme};
use crate::history::{History, WHEEL_ROWS};
use crate::loader::{Loaded, Loader, Request};

pub enum Event {
    Input(Input),
    /// リポジトリが変わった（`amonhen_watch::Changes` のビット）。
    Changed(u32),
    Loaded(Loaded),
    Failed(String),
}

/// メインの画面。
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    /// Working Copy。
    Changes,
    History,
}

/// キーが届く所: メインの画面の一覧（History・変わったファイル）か、その横（詳細・差分）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Main,
    Side,
    /// コミットメッセージの欄（Working Copy の一覧の下）。
    Message,
}

/// 画面の割り方（一番上と一番下の行を除いた所）。
struct Layout {
    main: Option<Rect>,
    side: Option<Rect>,
}

/// 変化の知らせのビット（`amonhen_watch::Changes`）。
const CHANGED_REFS: u32 = 1;
const CHANGED_INDEX: u32 = 2;
const CHANGED_WORKTREE: u32 = 4;

pub struct App {
    loader: Loader,
    history: History,
    details: Details,
    changes: Changes,
    view: View,
    focus: Focus,
    /// リポジトリ・ref・History を読んでいる。
    opening: bool,
    /// 読んでいる間に、また ref が変わった。
    reopen: bool,
    /// 確かめてから行う操作: 尋ねる文と、`y` で頼む中身。
    confirm: Option<(String, Request)>,
    /// HEAD のコミット（amend で、そのメッセージを欄に入れる）。
    head: Option<String>,
    /// コミットを作っている。
    committing: bool,
    /// 済んだことの知らせ（次のキーまで出す）。
    notice: Option<String>,
    theme: Theme,
    /// リポジトリの名前（作業ツリーのフォルダ名）。
    name: String,
    branch: Option<String>,
    clock: Clock,
    size: (u16, u16),
    /// ワーカーに頼んで、まだ答えが来ていない数。
    pending: usize,
    error: Option<String>,
    dirty: bool,
    quit: bool,
}

/// 一番上の行（リポジトリとブランチ）と一番下の行（案内）を除いた所に、History と詳細を出す。
const BARS: u16 = 2;
/// これ以上の幅なら、History の右に詳細を並べる。狭ければ、詳細は History の所に開く（Enter）。
const SIDE_BY_SIDE: u16 = 120;
/// 並べたときの History の幅は全体の 5 分の 2（少なくともこの幅）。
const HISTORY_MIN: u16 = 60;

impl App {
    pub fn new(front: Arc<dyn Front>, events: Sender<Event>, clock: Clock) -> Self {
        Self {
            loader: Loader::start(front, events),
            history: History::default(),
            details: Details::default(),
            changes: Changes::default(),
            view: View::History,
            focus: Focus::Main,
            opening: false,
            reopen: false,
            confirm: None,
            head: None,
            committing: false,
            notice: None,
            theme: Theme::detect(),
            name: String::new(),
            branch: None,
            clock,
            size: (0, 0),
            pending: 0,
            error: None,
            dirty: true,
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
        if cols >= SIDE_BY_SIDE {
            let left = (cols / 5 * 2).max(HISTORY_MIN);
            // 間に区切りの 1 列。
            return Layout {
                main: Some(Rect::new(0, 1, left, height)),
                side: Some(Rect::new(left + 1, 1, cols - left - 1, height)),
            };
        }
        let all = Some(Rect::new(0, 1, cols, height));
        match self.focus {
            Focus::Main | Focus::Message => Layout {
                main: all,
                side: None,
            },
            Focus::Side => Layout {
                main: None,
                side: all,
            },
        }
    }

    /// 描き直すことがあれば true（聞いたら、なかったことにする）。
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn quit(&self) -> bool {
        self.quit
    }

    /// まだ読んでいる。
    pub fn busy(&self) -> bool {
        self.pending > 0
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn history_rows(&self) -> u32 {
        self.history.total()
    }

    pub fn history_at_end(&self, down: bool) -> bool {
        self.history.at_end(down)
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
                self.dirty = true;
                match loaded {
                    Loaded::Opened {
                        workdir,
                        refs,
                        history,
                    } => {
                        self.name = workdir
                            .as_deref()
                            .and_then(|w| std::path::Path::new(w).file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.branch = refs["headBranch"].as_str().map(str::to_owned);
                        self.head = refs["head"].as_str().map(str::to_owned);
                        self.history.opened(&refs, &history);
                        self.opening = false;
                        if std::mem::take(&mut self.reopen) {
                            self.open();
                        }
                    }
                    Loaded::Rows { start, reply } => self.history.loaded(start, &reply),
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
                    Loaded::Status(Ok(status)) => self.changes.status(&status),
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
                            Err(error) => self.error = Some(error),
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
        let side = self.layout().side.is_some();
        // 詳細が見えているなら、選んでいるコミットのものにする。
        if side
            && self.view == View::History
            && let Some(id) = self.history.selected_id()
        {
            self.dirty |= self.details.want(id);
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
        // 詳細・差分が、一覧の所に開いている（並んでいない）。
        let covering = self.layout().main.is_none();
        // エラーは、次のキーまで出しておく。
        self.dirty |= self.error.take().is_some() | self.notice.take().is_some();
        // 確かめている間は、`y`（か Enter）で行い、ほかのキーでやめる。
        if let Some((_, request)) = self.confirm.take() {
            if matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter) {
                self.send(request);
            }
            self.dirty = true;
            return;
        }
        // メッセージを書いている間は、文字のキーはすべて入力。
        if self.focus == Focus::Message && !(control && key.code == KeyCode::Char('c')) {
            self.message_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('c') if control => self.quit = true,
            KeyCode::Char('q') if !covering => self.quit = true,
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Main | Focus::Message => Focus::Side,
                    Focus::Side => Focus::Main,
                };
                self.dirty = true;
            }
            KeyCode::Char('1') => self.show(View::Changes),
            KeyCode::Char('2') => self.show(View::History),
            _ => {
                self.dirty |= match (self.view, self.focus) {
                    (View::History, Focus::Main) => self.history_key(key.code, control),
                    (View::History, Focus::Side) => self.details_key(key.code, control),
                    (View::Changes, Focus::Main) => self.changes_key(key.code, control),
                    (View::Changes, Focus::Side) => self.changes_diff_key(key.code, control),
                    (_, Focus::Message) => false,
                }
            }
        }
    }

    /// メインの画面を替える。
    fn show(&mut self, view: View) {
        self.dirty |= self.view != view || self.focus != Focus::Main;
        self.view = view;
        self.focus = Focus::Main;
    }

    /// メッセージの欄でのキー: Esc と Tab で一覧へ、`Ctrl-s`（か Alt-Enter）でコミット、ほかは入力。
    fn message_key(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        self.dirty = true;
        match key.code {
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => self.focus = Focus::Main,
            KeyCode::Char('s') if control => self.commit(),
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
            KeyCode::Char('d') => {
                self.confirm = self.changes.discard_file().map(|(path, request)| {
                    let ask = tr!(
                        "Discard the changes in {path}? Undo brings them back.",
                        path = path
                    );
                    (ask, request)
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
        match code {
            KeyCode::Char('d') if control => self.changes.move_cursor(page / 2),
            KeyCode::Char('u') if control => self.changes.move_cursor(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.changes.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.changes.move_cursor(-1),
            KeyCode::PageDown => self.changes.move_cursor(page),
            KeyCode::PageUp => self.changes.move_cursor(-page),
            KeyCode::Char('g') | KeyCode::Home => self.changes.cursor_to(0),
            KeyCode::Char('G') | KeyCode::End => self.changes.cursor_to(u32::MAX),
            KeyCode::Char('l') | KeyCode::Right => self.changes.pan(PAN),
            KeyCode::Char('h') | KeyCode::Left => self.changes.pan(-PAN),
            KeyCode::Char(']') => self.changes.jump_hunk(true),
            KeyCode::Char('[') => self.changes.jump_hunk(false),
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
                    self.confirm = Some((ask, request));
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
        let page = self.history.page();
        match code {
            KeyCode::Char('d') if control => self.history.move_selection(page / 2),
            KeyCode::Char('u') if control => self.history.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.history.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.history.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.history.move_selection(page),
            KeyCode::PageUp => self.history.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.history.select(0),
            KeyCode::Char('G') | KeyCode::End => self.history.select(u32::MAX),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.focus = Focus::Side;
                true
            }
            _ => false,
        }
    }

    fn details_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.details.page();
        match code {
            KeyCode::Char('d') if control => self.details.scroll(page / 2),
            KeyCode::Char('u') if control => self.details.scroll(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.details.scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.details.scroll(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.details.scroll(page),
            KeyCode::PageUp => self.details.scroll(-page),
            KeyCode::Char('g') | KeyCode::Home => self.details.scroll_to(0),
            KeyCode::Char('G') | KeyCode::End => self.details.scroll_to(u32::MAX),
            KeyCode::Char('l') | KeyCode::Right => self.details.pan(PAN),
            KeyCode::Char('h') | KeyCode::Left => self.details.pan(-PAN),
            KeyCode::Char(']') => self.details.jump_file(true),
            KeyCode::Char('[') => self.details.jump_file(false),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.focus = Focus::Main;
                true
            }
            _ => false,
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) {
        // 確かめている間は、キーで答えるまで動かさない。
        if self.confirm.is_some() {
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
        let history = self.view == View::History;
        let changed = if let Some(area) = inside(layout.main) {
            match wheel {
                Some(rows) if history => self.history.scroll(rows),
                Some(rows) => self.changes.scroll_list(rows),
                None if click => {
                    let focused = std::mem::replace(&mut self.focus, Focus::Main);
                    let line = mouse.row - area.y;
                    let selected = if history {
                        self.history.click(line)
                    } else if line >= self.changes.list_height() {
                        // 一覧の下は、メッセージの欄。
                        self.focus = Focus::Message;
                        false
                    } else {
                        self.changes.click(line)
                    };
                    selected || focused != self.focus
                }
                None => false,
            }
        } else if let Some(area) = inside(layout.side) {
            match wheel {
                Some(rows) if history => self.details.scroll(rows),
                Some(rows) => self.changes.scroll(rows),
                None if click => {
                    let focused = std::mem::replace(&mut self.focus, Focus::Side);
                    let selected = !history && self.changes.click_diff(mouse.row - area.y);
                    selected || focused != Focus::Side
                }
                None => false,
            }
        } else {
            false
        };
        self.dirty |= changed;
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.height < BARS + 1 {
            return;
        }
        let mut cursor = None;
        let buf = frame.buffer_mut();
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
        }
        // メインの画面の切り替え（数字のキー）。今の画面は太字。
        let changes = match self.changes.count() {
            0 => tr!("1 Changes"),
            count => tr!("1 Changes ({count})", count = count),
        };
        for (view, label) in [(View::Changes, changes), (View::History, tr!("2 History"))] {
            if x + 3 >= area.right() {
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
            let loading = tr!("Loading…");
            let width = loading.width() as u16 + 1;
            if width < area.width {
                buf.set_string(area.right() - width, area.y, loading, bar);
            }
        }

        let layout = self.layout();
        let focused = self.focus == Focus::Main;
        if let Some(at) = layout.main {
            match self.view {
                View::History => self.history.draw(at, buf, self.clock.now, focused),
                View::Changes => {
                    cursor = self
                        .changes
                        .draw_main(at, buf, focused, self.focus == Focus::Message);
                }
            }
        }
        if let Some(at) = layout.side {
            // 一覧と並べたときは、間に区切りの線。
            if at.x > area.x {
                let line = Style::new().fg(Color::Indexed(if focused { 238 } else { 244 }));
                for y in at.y..at.bottom() {
                    buf[(at.x - 1, y)].set_char('│').set_style(line);
                }
            }
            match self.view {
                View::History => self.details.draw(at, buf, &self.clock, &self.theme),
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
            (View::History, Focus::Side) => self.details.position(),
            (View::Changes, Focus::Main) => self.changes.list_position(),
            (View::Changes, Focus::Side) => self.changes.diff_position(),
            (_, Focus::Message) => self.changes.list_position(),
        };
        let position = format!("{at}/{of}");
        let position_x = area
            .right()
            .saturating_sub(position.width() as u16 + 1)
            .max(area.x);
        let room = usize::from(position_x.saturating_sub(area.x + 2));
        match (&self.confirm, &self.error) {
            (Some((ask, _)), _) => {
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
                    tr!("y discard   n cancel"),
                    usize::from(position_x.saturating_sub(x + 3)),
                    Style::new(),
                );
            }
            (None, Some(error)) => {
                buf.set_stringn(area.x + 1, y, error, room, Style::new().fg(Color::Red));
            }
            (None, None) if self.notice.is_some() => {
                let notice = self.notice.as_deref().unwrap_or("");
                buf.set_stringn(area.x + 1, y, notice, room, Style::new().fg(Color::Green));
            }
            (None, None) => {
                let hints = match (self.view, self.focus) {
                    (View::History, Focus::Main) => {
                        tr!("j/k move   g/G top/bottom   Enter details   q quit")
                    }
                    (View::History, Focus::Side) => {
                        tr!("j/k scroll   [/] file   h/l pan   Esc back")
                    }
                    (View::Changes, Focus::Main) => {
                        tr!(
                            "j/k move   Space stage/unstage   c message   a amend   d discard   q quit"
                        )
                    }
                    (View::Changes, Focus::Side) => {
                        tr!("j/k line   [/] hunk   Space stage/unstage   d discard   Esc back")
                    }
                    (_, Focus::Message) => tr!("Ctrl-s commit   Esc back"),
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
        // 入力しているときだけ、カーソルを出す。
        if let Some(at) = cursor {
            frame.set_cursor_position(at);
        }
    }
}
