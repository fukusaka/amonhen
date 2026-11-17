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
use crate::details::{Details, PAN, Theme};
use crate::history::{History, WHEEL_ROWS};
use crate::loader::{Loaded, Loader, Request};

pub enum Event {
    Input(Input),
    Loaded(Loaded),
    Failed(String),
}

/// キーが届く所。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    History,
    Details,
}

/// 画面の割り方（一番上と一番下の行を除いた所）。
struct Layout {
    history: Option<Rect>,
    details: Option<Rect>,
}

pub struct App {
    loader: Loader,
    history: History,
    details: Details,
    focus: Focus,
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
            focus: Focus::History,
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
        self.send(Request::Open);
    }

    fn send(&mut self, request: Request) {
        self.pending += 1;
        self.loader.send(request);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = (cols, rows);
        self.history.set_height(rows.saturating_sub(BARS));
        // 詳細の幅は、並べても開いても、キーの届く所によらない。
        let layout = self.layout();
        let details = layout.details.or(layout.history).unwrap_or_default();
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
                history: Some(Rect::new(0, 1, left, height)),
                details: Some(Rect::new(left + 1, 1, cols - left - 1, height)),
            };
        }
        let all = Some(Rect::new(0, 1, cols, height));
        match self.focus {
            Focus::History => Layout {
                history: all,
                details: None,
            },
            Focus::Details => Layout {
                history: None,
                details: all,
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
                        self.history.opened(&refs, &history);
                    }
                    Loaded::Rows { start, reply } => self.history.loaded(start, &reply),
                    Loaded::Commit { id, result } => {
                        if let Some(request) = self.details.loaded(&id, result, &self.clock) {
                            self.send(request);
                        }
                    }
                    Loaded::Syntax { id, reply } => self.details.syntax(&id, reply.as_ref()),
                }
            }
            Event::Failed(error) => {
                self.pending = self.pending.saturating_sub(1);
                self.history.failed();
                self.error = Some(error);
                self.dirty = true;
            }
        }
        // 見える範囲が動いたら、足りない行を頼む。
        if let Some(request) = self.history.request() {
            self.send(request);
        }
        // 詳細が見えているなら、選んでいるコミットのものにする。
        if self.layout().details.is_some()
            && let Some(id) = self.history.selected_id()
        {
            self.dirty |= self.details.want(id);
        }
        if let Some(request) = self.details.request() {
            self.send(request);
        }
    }

    fn key(&mut self, key: KeyEvent) {
        // Windows は離したときも知らせる。
        if key.kind == KeyEventKind::Release {
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        // 詳細が History の所に開いている（並んでいない）。
        let covering = self.layout().history.is_none();
        match key.code {
            KeyCode::Char('c') if control => self.quit = true,
            KeyCode::Char('q') if !covering => self.quit = true,
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::History => Focus::Details,
                    Focus::Details => Focus::History,
                };
                self.dirty = true;
            }
            _ => {
                self.dirty |= match self.focus {
                    Focus::History => self.history_key(key.code, control),
                    Focus::Details => self.details_key(key.code, control),
                }
            }
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
                self.focus = Focus::Details;
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
                self.focus = Focus::History;
                true
            }
            _ => false,
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) {
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
        let changed = if let Some(area) = inside(layout.history) {
            match wheel {
                Some(rows) => self.history.scroll(rows),
                None if click => {
                    let focused = std::mem::replace(&mut self.focus, Focus::History);
                    self.history.click(mouse.row - area.y) || focused != Focus::History
                }
                None => false,
            }
        } else if inside(layout.details).is_some() {
            match wheel {
                Some(rows) => self.details.scroll(rows),
                None if click => {
                    std::mem::replace(&mut self.focus, Focus::Details) != Focus::Details
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
        if let Some(branch) = &self.branch {
            buf.set_stringn(
                x + 2,
                area.y,
                branch,
                usize::from(area.right().saturating_sub(x + 3)),
                bar,
            );
        }
        if self.busy() {
            let loading = tr!("Loading…");
            let width = loading.width() as u16 + 1;
            if width < area.width {
                buf.set_string(area.right() - width, area.y, loading, bar);
            }
        }

        let layout = self.layout();
        if let Some(at) = layout.history {
            self.history
                .draw(at, buf, self.clock.now, self.focus == Focus::History);
        }
        if let Some(at) = layout.details {
            // History と並べたときは、間に区切りの線。
            if at.x > area.x {
                let line = Style::new().fg(Color::Indexed(if self.focus == Focus::Details {
                    244
                } else {
                    238
                }));
                for y in at.y..at.bottom() {
                    buf[(at.x - 1, y)].set_char('│').set_style(line);
                }
            }
            self.details.draw(at, buf, &self.clock, &self.theme);
        }

        // 一番下: エラーか、キーの案内。右に、今の位置（History は選んでいる行、詳細は見えている最後の行）。
        let y = area.bottom() - 1;
        let (at, of) = match self.focus {
            Focus::History => (
                (self.history.selected() + 1).min(self.history.total()),
                self.history.total(),
            ),
            Focus::Details => self.details.position(),
        };
        let position = format!("{at}/{of}");
        let position_x = area
            .right()
            .saturating_sub(position.width() as u16 + 1)
            .max(area.x);
        let room = usize::from(position_x.saturating_sub(area.x + 2));
        match &self.error {
            Some(error) => {
                buf.set_stringn(area.x + 1, y, error, room, Style::new().fg(Color::Red));
            }
            None => {
                let hints = match self.focus {
                    Focus::History => tr!("j/k move   g/G top/bottom   Enter details   q quit"),
                    Focus::Details => {
                        tr!("j/k scroll   [/] file   h/l pan   Esc back")
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
    }
}
