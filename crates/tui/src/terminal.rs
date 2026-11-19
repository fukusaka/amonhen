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
    /// 端末が左右の余白（DECSLRM）を持つ（取るときに問い合わせた）。
    margins: bool,
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
        // 入力を読むスレッドを始める前に、端末に聞く。
        let margins = has_margins(&mut out);
        let mut canvas = Canvas::new(out);
        canvas.set_margins(margins);
        Ok(Self { canvas, margins })
    }
}

/// 端末が答えるのを待つ長さ。答えない端末では、起動がこれだけ遅れる。
#[cfg(unix)]
const ANSWER_TIMEOUT: Duration = Duration::from_millis(300);

/// 端末が左右の余白（DECSLRM）を持つか。モード 69（DECLRMM）を知っているかを聞き（DECRQM）、続けて、どの端末も
/// 答える問い（DA1）を送る。DA1 の答えが先に来たら、聞いたことを知らない端末。raw モードで、入力を読む
/// スレッドを始める前に呼ぶ（答えは、入力として届く）。
#[cfg(unix)]
fn has_margins(out: &mut impl std::io::Write) -> bool {
    // 入力が端末でなければ、答えを読めない。
    if unsafe { libc::isatty(libc::STDIN_FILENO) } != 1 {
        return false;
    }
    if out
        .write_all(b"\x1b[?69$p\x1b[c")
        .and_then(|()| out.flush())
        .is_err()
    {
        return false;
    }
    let deadline = std::time::Instant::now() + ANSWER_TIMEOUT;
    let mut answer = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let mut fd = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `fd` と `byte` は、呼び出しの間だけ使う手元の値。
        if left.is_zero() || unsafe { libc::poll(&mut fd, 1, left.as_millis() as libc::c_int) } <= 0
        {
            return false;
        }
        let mut byte = 0u8;
        if unsafe { libc::read(libc::STDIN_FILENO, (&raw mut byte).cast(), 1) } != 1 {
            return false;
        }
        answer.push(byte);
        // DA1 の答え（`CSI ? … c`）まで読んだら終わり。
        if byte == b'c' && answer.windows(3).any(|w| w == b"\x1b[?") {
            return margins_answer(&answer);
        }
    }
}

#[cfg(not(unix))]
fn has_margins(_: &mut impl std::io::Write) -> bool {
    false
}

/// DECRQM の答え（`CSI ? 69 ; Ps $ y`）が、モード 69 を知っていると言っているか: `Ps` が 1（入っている）か
/// 2（切ってある）。0 は知らない、3 と 4 は変えられない。
#[cfg_attr(not(unix), allow(dead_code))]
fn margins_answer(answer: &[u8]) -> bool {
    let start = b"\x1b[?69;";
    answer.windows(start.len() + 3).any(|w| {
        w.starts_with(start) && matches!(w[start.len()], b'1' | b'2') && w.ends_with(b"$y")
    })
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
        self.canvas.set_margins(self.margins);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 端末の答えから、左右の余白を持つかを読む
    #[test]
    fn margins_are_read_from_the_answer() {
        // 知っている（切ってある）。後ろは DA1 の答え。
        assert!(margins_answer(b"\x1b[?69;2$y\x1b[?62;4c"));
        assert!(margins_answer(b"\x1b[?69;1$y\x1b[?1;2c"));
        // 知らない。変えられない。答えない。
        assert!(!margins_answer(b"\x1b[?69;0$y\x1b[?1;2;4c"));
        assert!(!margins_answer(b"\x1b[?69;4$y\x1b[?1;2c"));
        assert!(!margins_answer(b"\x1b[?1;2c"));
    }
}
