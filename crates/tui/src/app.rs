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

use crate::history::{History, WHEEL_ROWS};
use crate::loader::{Loaded, Loader, Request};

pub enum Event {
    Input(Input),
    Loaded(Loaded),
    Failed(String),
}

pub struct App {
    loader: Loader,
    history: History,
    /// リポジトリの名前（作業ツリーのフォルダ名）。
    name: String,
    branch: Option<String>,
    /// 「2d」などの時刻の基準。
    now: i64,
    size: (u16, u16),
    /// ワーカーに頼んで、まだ答えが来ていない数。
    pending: usize,
    error: Option<String>,
    dirty: bool,
    quit: bool,
}

/// 一番上の行（リポジトリとブランチ）と一番下の行（案内）を除いた所が History。
const BARS: u16 = 2;

impl App {
    pub fn new(front: Arc<dyn Front>, events: Sender<Event>, now: i64) -> Self {
        Self {
            loader: Loader::start(front, events),
            history: History::default(),
            name: String::new(),
            branch: None,
            now,
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
        self.dirty = true;
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
    }

    fn key(&mut self, key: KeyEvent) {
        // Windows は離したときも知らせる。
        if key.kind == KeyEventKind::Release {
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.history.page();
        let changed = match key.code {
            KeyCode::Char('c') if control => {
                self.quit = true;
                false
            }
            KeyCode::Char('q') => {
                self.quit = true;
                false
            }
            KeyCode::Char('d') if control => self.history.move_selection(page / 2),
            KeyCode::Char('u') if control => self.history.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.history.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.history.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.history.move_selection(page),
            KeyCode::PageUp => self.history.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.history.select(0),
            KeyCode::Char('G') | KeyCode::End => self.history.select(u32::MAX),
            _ => false,
        };
        self.dirty |= changed;
    }

    fn mouse(&mut self, mouse: MouseEvent) {
        let changed = match mouse.kind {
            MouseEventKind::ScrollDown => self.history.scroll(WHEEL_ROWS),
            MouseEventKind::ScrollUp => self.history.scroll(-WHEEL_ROWS),
            // 一番上の行は History の外。
            MouseEventKind::Down(MouseButton::Left) if mouse.row >= 1 => {
                self.history.click(mouse.row - 1)
            }
            _ => false,
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

        self.history.draw(
            Rect::new(area.x, area.y + 1, area.width, area.height - BARS),
            buf,
            self.now,
        );

        // 一番下: エラーか、キーの案内。右に、選んでいる行と全体の行数。
        let y = area.bottom() - 1;
        let position = format!(
            "{}/{}",
            (self.history.selected() + 1).min(self.history.total()),
            self.history.total()
        );
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
                let hints = tr!("j/k move   g/G top/bottom   q quit");
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
