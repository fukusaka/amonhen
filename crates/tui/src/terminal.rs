//! 本物の端末: 代替画面・raw モード・マウスの取り込みと、その後始末。入力を読むスレッド。

use std::io::{BufWriter, Stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use ratatui_crossterm::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use ratatui_crossterm::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui_crossterm::crossterm::{cursor, execute};

use crate::Result;
use crate::app::Event;
use crate::canvas::Canvas;

/// 1 フレームを 1 回の書き込みで出せる大きさ（200 × 60 の全面の描き直しでも収まる）。
const OUTPUT_BUFFER: usize = 1 << 16;

pub struct Screen {
    pub canvas: Canvas<BufWriter<Stdout>>,
}

/// 端末の大きさ（列、行）。
pub fn size() -> Result<(u16, u16)> {
    Ok(ratatui_crossterm::crossterm::terminal::size()?)
}

impl Screen {
    /// 端末を取る。終わるとき（panic でも）に戻す。
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut out = BufWriter::with_capacity(OUTPUT_BUFFER, std::io::stdout());
        execute!(out, EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;
        // panic の文は、端末を戻してから出す。
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        Ok(Self {
            canvas: Canvas::new(out),
        })
    }
}

impl Screen {
    /// マウスの取り込みを替える（切ると、端末が文字の選択を受け持つ）。
    pub fn set_mouse(&mut self, capture: bool) -> Result<()> {
        let out = self.canvas.out();
        match capture {
            true => execute!(out, EnableMouseCapture)?,
            false => execute!(out, DisableMouseCapture)?,
        }
        Ok(())
    }

    /// 端末を戻してプロセスを止め（`Ctrl-z`。シェルの `fg` で続く）、続いたら端末を取り直す。
    #[cfg(unix)]
    pub fn suspend(&mut self, mouse: bool) -> Result<()> {
        self.outside(mouse, || {
            // 自分に SIGTSTP を送る（ここで止まり、SIGCONT で次の行から続く）。
            let _ = amonhen_model::process::command("kill")
                .args(["-TSTP", &std::process::id().to_string()])
                .status();
        })
    }

    /// 端末を戻して `f` を動かし（エディタなど、端末を使うもの）、終わったら端末を取り直す。
    pub fn outside<T>(&mut self, mouse: bool, f: impl FnOnce() -> T) -> Result<T> {
        restore();
        let result = f();
        enable_raw_mode()?;
        let mut out = BufWriter::with_capacity(OUTPUT_BUFFER, std::io::stdout());
        execute!(out, EnterAlternateScreen, cursor::Hide)?;
        if mouse {
            execute!(out, EnableMouseCapture)?;
        }
        // 代替画面は空なので、前のフレームを持たないものに取り替える（次のフレームはすべて書く）。
        self.canvas = Canvas::new(out);
        Ok(result)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        restore();
    }
}

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(
        std::io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        cursor::Show
    );
}

/// 入力を待つ 1 回の長さ。エディタに端末を渡すときは、これだけ待てば読むのをやめている。
pub const INPUT_POLL: Duration = Duration::from_millis(50);

/// 入力を読んでチャネルに送り続ける（読むのは待つので、スレッドを分ける）。返した印を true にしている間は
/// 読まない（エディタが端末を使う間、キーを横取りしない）。
pub fn read_input(tx: Sender<Event>) -> Arc<AtomicBool> {
    let paused = Arc::new(AtomicBool::new(false));
    let flag = paused.clone();
    let _ = std::thread::Builder::new()
        .name("amonhen-tui-input".into())
        .spawn(move || {
            loop {
                if flag.load(Ordering::Relaxed) {
                    std::thread::sleep(INPUT_POLL);
                    continue;
                }
                match event::poll(INPUT_POLL) {
                    Ok(false) => continue,
                    Ok(true) => {}
                    Err(_) => break,
                }
                let Ok(input) = event::read() else { break };
                if tx.send(Event::Input(input)).is_err() {
                    break;
                }
            }
        });
    paused
}
