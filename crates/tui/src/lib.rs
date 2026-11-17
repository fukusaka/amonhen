//! 端末で動く FE（`amonhen tui`）。設計は `docs/tui.md`。
//!
//! コアへは FE 向けの窓口（`amonhen_front::Front`）だけを通る。窓口の呼び出しは答えを待つので、すべて
//! ワーカー（`loader`）で行い、メインスレッドは 1 本のチャネルで入力・答えを待つ。状態（`app`）はただのデータで、
//! 描画はその関数。何も変わらなければ描かない。

mod app;
mod changes;
mod details;
mod diff;
mod history;
mod loader;
mod terminal;

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use amonhen_front::Front;
use ratatui_core::backend::TestBackend;
use ratatui_core::layout::Rect;
use ratatui_core::terminal::{Terminal, TerminalOptions, Viewport};
use ratatui_crossterm::CrosstermBackend;
use ratatui_crossterm::crossterm::event::{
    Event as Input, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
};

use unicode_width::UnicodeWidthStr;

use app::{App, Event};

pub type Result<T> = amonhen_model::Result<T>;

/// 時刻の出し方。
#[derive(Clone, Copy)]
pub struct Clock {
    /// 「2d」などの基準（UNIX 時刻）。
    pub now: i64,
    /// UNIX 時刻を、手元の日付と時刻の文字に。
    pub local: fn(i64) -> String,
}

/// 端末を取って、終わる（`q`）まで動かす。
pub fn run(front: Arc<dyn Front>, local: fn(i64) -> String) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut screen = terminal::Screen::enter()?;
    terminal::read_input(tx.clone());
    // リポジトリの変化の知らせ（受けられなくても、画面は動く）。
    let changed = std::sync::Mutex::new(tx.clone());
    let _ = front.subscribe(Box::new(move |bits| {
        if let Ok(tx) = changed.lock() {
            let _ = tx.send(Event::Changed(bits));
        }
    }));
    let mut app = App::new(front, tx, Clock { now: now(), local });
    let size = screen.terminal.size()?;
    app.resize(size.width, size.height);
    app.start();
    loop {
        if app.take_dirty() {
            screen.draw(|frame| app.draw(frame))?;
        }
        // 1 つ待ち、たまっている分はまとめて片づけてから描く。
        let Ok(event) = rx.recv() else { break };
        app.handle(event);
        while let Ok(event) = rx.try_recv() {
            app.handle(event);
        }
        if app.quit() {
            break;
        }
    }
    Ok(())
}

/// 読み込みが落ち着くまで、答えを受け取る。
fn settle(app: &mut App, rx: &mpsc::Receiver<Event>) -> Result<()> {
    while app.busy() {
        let event = rx
            .recv_timeout(Duration::from_secs(60))
            .map_err(|_| "the repository took too long to load")?;
        app.handle(event);
    }
    match app.error() {
        Some(e) => Err(e.to_owned().into()),
        None => Ok(()),
    }
}

/// 1 フレームを文字で（`--snapshot`。テストと、見た目の確認に）。`keys` は、描く前に押すキー（1 文字ずつ。
/// 改行は Enter、タブは Tab、`\x1b` は Esc）。
pub fn snapshot(
    front: Arc<dyn Front>,
    cols: u16,
    rows: u16,
    clock: Clock,
    keys: &str,
) -> Result<String> {
    let (tx, rx) = mpsc::channel();
    let mut app = App::new(front, tx, clock);
    app.resize(cols, rows);
    app.start();
    settle(&mut app, &rx)?;
    for ch in keys.chars() {
        let code = match ch {
            '\n' => KeyCode::Enter,
            '\t' => KeyCode::Tab,
            '\x1b' => KeyCode::Esc,
            ch => KeyCode::Char(ch),
        };
        app.handle(Event::Input(Input::Key(KeyEvent::new(
            code,
            KeyModifiers::NONE,
        ))));
        settle(&mut app, &rx)?;
    }
    let mut terminal = Terminal::new(TestBackend::new(cols, rows))?;
    terminal.draw(|frame| app.draw(frame))?;
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..rows {
        let mut line = String::new();
        // 全角の文字は 2 セルを使う（後ろのセルは空）。
        let mut x = 0;
        while x < cols {
            let symbol = buffer[(x, y)].symbol();
            line.push_str(symbol);
            x += (symbol.width() as u16).max(1);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// `--measure-scroll` の結果。
#[derive(Debug)]
pub struct Measure {
    pub frames: usize,
    pub rows: u32,
    /// イベントを受けてからフレームのバイト列を出し終えるまで（µs）。
    pub median_us: u64,
    pub p95_us: u64,
    pub max_us: u64,
    /// 1 フレームのバイト数。
    pub mean_bytes: u64,
    pub max_bytes: u64,
    /// 文字がまだ届いていない行を含んだフレームの数。
    pub incomplete_frames: usize,
}

/// 出したバイト数を数えるだけの出力先。
struct Counter(Arc<AtomicU64>);

impl Write for Counter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.fetch_add(buf.len() as u64, Ordering::Relaxed);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// History を `frames` 回スクロールし（ホイール 1 目盛り、`interval` おき）、1 フレームにかかる時間とバイト数を
/// 測る。入力から出力までは本物と同じ道を通し、出力先だけを数えるものに替える。
pub fn measure_scroll(
    front: Arc<dyn Front>,
    cols: u16,
    rows: u16,
    frames: usize,
    interval: Duration,
    local: fn(i64) -> String,
) -> Result<Measure> {
    let (tx, rx) = mpsc::channel();
    let mut app = App::new(front, tx, Clock { now: now(), local });
    app.resize(cols, rows);
    app.start();
    settle(&mut app, &rx)?;
    let written = Arc::new(AtomicU64::new(0));
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(Counter(written.clone())),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, cols, rows)),
        },
    )?;
    terminal.draw(|frame| app.draw(frame))?;
    let wheel = |kind| {
        Event::Input(Input::Mouse(MouseEvent {
            kind,
            // History の上（詳細と並んでいても、左が History）。
            column: 1,
            row: rows / 2,
            modifiers: KeyModifiers::NONE,
        }))
    };
    let mut times = Vec::with_capacity(frames);
    let mut sizes = Vec::with_capacity(frames);
    let mut incomplete = 0;
    let mut down = true;
    for _ in 0..frames {
        let before = written.load(Ordering::Relaxed);
        let start = Instant::now();
        // 端に着いたら折り返す。
        if app.history_at_end(down) {
            down = !down;
        }
        app.handle(wheel(if down {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        }));
        while let Ok(event) = rx.try_recv() {
            app.handle(event);
        }
        if app.take_dirty() {
            terminal.draw(|frame| app.draw(frame))?;
        }
        let spent = start.elapsed();
        times.push(spent.as_micros() as u64);
        sizes.push(written.load(Ordering::Relaxed) - before);
        incomplete += usize::from(app.last_frame_incomplete());
        std::thread::sleep(interval.saturating_sub(spent));
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    Ok(Measure {
        frames,
        rows: app.history_rows(),
        median_us: at(0.5),
        p95_us: at(0.95),
        max_us: at(1.0),
        mean_bytes: sizes.iter().sum::<u64>() / sizes.len().max(1) as u64,
        max_bytes: sizes.iter().copied().max().unwrap_or(0),
        incomplete_frames: incomplete,
    })
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
