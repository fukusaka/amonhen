//! 本物の端末: 代替画面・raw モード・マウスの取り込みと、その後始末。入力を読むスレッド。

use std::io::{BufWriter, Stdout, Write};
use std::sync::mpsc::Sender;

use ratatui_core::terminal::{Frame, Terminal};
use ratatui_crossterm::CrosstermBackend;
use ratatui_crossterm::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use ratatui_crossterm::crossterm::terminal::{
    BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use ratatui_crossterm::crossterm::{cursor, execute, queue};

use crate::Result;
use crate::app::Event;

/// 1 フレームを 1 回の書き込みで出せる大きさ（200 × 60 の全面の描き直しでも収まる）。
const OUTPUT_BUFFER: usize = 1 << 16;

pub struct Screen {
    pub terminal: Terminal<CrosstermBackend<BufWriter<Stdout>>>,
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
            terminal: Terminal::new(CrosstermBackend::new(out))?,
        })
    }

    /// 1 フレーム描く。対応する端末ではまとめて表示されるよう、synchronized update で包む。
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> Result<()> {
        queue!(self.terminal.backend_mut(), BeginSynchronizedUpdate)?;
        self.terminal.draw(render)?;
        queue!(self.terminal.backend_mut(), EndSynchronizedUpdate)?;
        self.terminal.backend_mut().flush()?;
        Ok(())
    }
}

impl Screen {
    /// マウスの取り込みを替える（切ると、端末が文字の選択を受け持つ）。
    pub fn set_mouse(&mut self, capture: bool) -> Result<()> {
        let out = self.terminal.backend_mut();
        match capture {
            true => execute!(out, EnableMouseCapture)?,
            false => execute!(out, DisableMouseCapture)?,
        }
        Ok(())
    }

    /// 端末を戻してプロセスを止め（`Ctrl-z`。シェルの `fg` で続く）、続いたら端末を取り直す。
    #[cfg(unix)]
    pub fn suspend(&mut self, mouse: bool) -> Result<()> {
        restore();
        // 自分に SIGTSTP を送る（ここで止まり、SIGCONT で次の行から続く）。
        let _ = amonhen_model::process::command("kill")
            .args(["-TSTP", &std::process::id().to_string()])
            .status();
        enable_raw_mode()?;
        let mut out = BufWriter::with_capacity(OUTPUT_BUFFER, std::io::stdout());
        execute!(out, EnterAlternateScreen, cursor::Hide)?;
        if mouse {
            execute!(out, EnableMouseCapture)?;
        }
        // 代替画面は空なので、前のフレームを持たない端末に取り替える（次のフレームはすべて書く）。
        self.terminal = Terminal::new(CrosstermBackend::new(out))?;
        Ok(())
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

/// 入力を読んでチャネルに送り続ける（読むのは待つので、スレッドを分ける）。
pub fn read_input(tx: Sender<Event>) {
    let _ = std::thread::Builder::new()
        .name("amonhen-tui-input".into())
        .spawn(move || {
            while let Ok(input) = event::read() {
                if tx.send(Event::Input(input)).is_err() {
                    break;
                }
            }
        });
}
