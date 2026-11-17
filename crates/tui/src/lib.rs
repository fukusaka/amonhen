//! 端末で動く FE（`amonhen tui`）。設計は `docs/tui.md`。
//!
//! コアへは FE 向けの窓口（`amonhen_front::Front`）だけを通る。窓口の呼び出しは答えを待つので、すべて
//! ワーカー（`loader`）で行い、メインスレッドは 1 本のチャネルで入力・答えを待つ。状態（`app`）はただのデータで、
//! 描画はその関数。何も変わらなければ描かない。

mod app;
#[cfg(unix)]
pub mod askpass;
mod canvas;
mod changes;
mod details;
mod diff;
mod editor;
mod fileview;
mod history;
mod loader;
mod reflog;
mod sidebar;
mod terminal;

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use amonhen_front::Front;
use ratatui_core::buffer::Buffer;
use ratatui_crossterm::crossterm::event::{
    Event as Input, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
};

use unicode_width::UnicodeWidthStr;

use app::{App, Damage, Event};
use canvas::Canvas;

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
    run_with(front, local, |_| Ok(()))
}

/// `run` と同じ。`askpass` があれば、git や ssh が尋ねてきたことを画面で聞く。
#[cfg(unix)]
pub fn run_asking(
    front: Arc<dyn Front>,
    local: fn(i64) -> String,
    askpass: &askpass::Askpass,
) -> Result<()> {
    run_with(front, local, |events| askpass.serve(events))
}

fn run_with(
    front: Arc<dyn Front>,
    local: fn(i64) -> String,
    serve: impl FnOnce(mpsc::Sender<Event>) -> Result<()>,
) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    serve(tx.clone())?;
    let mut screen = terminal::Screen::enter()?;
    let input_paused = terminal::read_input(tx.clone());
    // リポジトリの変化の知らせ（受けられなくても、画面は動く）。
    let changed = std::sync::Mutex::new(tx.clone());
    let _ = front.subscribe(Box::new(move |bits| {
        if let Ok(tx) = changed.lock() {
            let _ = tx.send(Event::Changed(bits));
        }
    }));
    let mut app = App::new(front, tx, Clock { now: now(), local });
    let (cols, rows) = terminal::size()?;
    app.resize(cols, rows);
    app.start();
    loop {
        if let Some(damage) = app.take_damage() {
            paint(&mut screen.canvas, &mut app, damage)?;
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
        if let Some(capture) = app.take_mouse_change() {
            screen.set_mouse(capture)?;
        }
        if app.take_redraw() {
            screen.canvas.forget();
            // 大きさの知らせを取りこぼしていても、ここで合う。
            let (cols, rows) = terminal::size()?;
            app.resize(cols, rows);
        }
        #[cfg(unix)]
        if app.take_suspend() {
            screen.suspend(app.mouse_captured())?;
            // 止まっている間に端末の大きさが変わっていても、知らせは届かない。
            let (cols, rows) = terminal::size()?;
            app.resize(cols, rows);
        }
        // メッセージを git のエディタで開く。その間は、入力を読むのをやめて端末を渡す。
        if let Some((message, workdir)) = app.take_edit_message() {
            input_paused.store(true, Ordering::Relaxed);
            std::thread::sleep(terminal::INPUT_POLL * 2);
            let edited =
                screen.outside(app.mouse_captured(), || edit(&message, workdir.as_deref()))?;
            input_paused.store(false, Ordering::Relaxed);
            let (cols, rows) = terminal::size()?;
            app.resize(cols, rows);
            app.message_edited(edited);
        }
    }
    Ok(())
}

/// `message` を一時ファイルに書いて git のエディタ（`git var GIT_EDITOR`: `GIT_EDITOR`・`core.editor`・
/// `VISUAL`・`EDITOR` の順）で開き、閉じた後の中身を返す。
fn edit(message: &str, workdir: Option<&str>) -> Result<String> {
    let dir = workdir
        .map(std::path::Path::new)
        .filter(|d| d.is_dir())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| ".".into());
    let out = amonhen_model::process::command("git")
        .args(["var", "GIT_EDITOR"])
        .current_dir(&dir)
        .output()?;
    let editor = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() || editor.is_empty() {
        return Err(amonhen_model::tr!(
            "git has no editor set (GIT_EDITOR, core.editor, VISUAL, EDITOR)"
        )
        .into());
    }
    let file =
        std::env::temp_dir().join(format!("amonhen-tui-{}-COMMIT_EDITMSG", std::process::id()));
    std::fs::write(&file, format!("{message}\n"))?;
    // git と同じく、シェルに渡す（エディタの指定に引数があってもよい）。
    let status = amonhen_model::process::command("sh")
        .args(["-c", &format!("{editor} \"$@\""), "sh"])
        .arg(&file)
        .current_dir(&dir)
        .status();
    let text = std::fs::read_to_string(&file);
    let _ = std::fs::remove_file(&file);
    match status {
        Ok(status) if status.success() => Ok(text?),
        Ok(_) => Err(amonhen_model::tr!(
            "The editor ended with an error; the message is unchanged"
        )
        .into()),
        Err(e) => Err(e.into()),
    }
}

/// 1 フレームを出す: `damage` の所だけを描き直す。
fn paint<W: Write>(canvas: &mut Canvas<W>, app: &mut App, damage: Damage) -> Result<()> {
    let areas = app.areas(damage);
    canvas.draw(app.size(), &areas, |buf, all| {
        app.draw(buf, if all { Damage::ALL } else { damage })
    })?;
    Ok(())
}

/// 頼んだものの答えがそろうまで受け取る。
fn wait(app: &mut App, rx: &mpsc::Receiver<Event>) -> Result<()> {
    while app.busy() {
        let event = rx
            .recv_timeout(Duration::from_secs(60))
            .map_err(|_| "the repository took too long to answer")?;
        app.handle(event);
    }
    Ok(())
}

/// 読み込みが落ち着くまで、答えを受け取る。読めなければ失敗。
fn settle(app: &mut App, rx: &mpsc::Receiver<Event>) -> Result<()> {
    wait(app, rx)?;
    match app.error() {
        Some(e) => Err(e.to_owned().into()),
        None => Ok(()),
    }
}

/// 1 フレームを文字で（`--snapshot`。テストと、見た目の確認に）。`keys` は、描く前に押すキー（1 文字ずつ。
/// 改行は Enter、タブは Tab、`\x1b` は Esc、`\x7f` は Backspace、ほかの制御文字は Ctrl と文字）。
pub fn snapshot(
    front: Arc<dyn Front>,
    cols: u16,
    rows: u16,
    clock: Clock,
    keys: &str,
) -> Result<String> {
    let buffer = frame(front, cols, rows, clock, keys)?;
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

/// 1 フレームをセルのまま（文字と、色などの見た目。テストで見た目を確かめるのに）。`keys` は `snapshot` と同じ。
pub fn frame(
    front: Arc<dyn Front>,
    cols: u16,
    rows: u16,
    clock: Clock,
    keys: &str,
) -> Result<Buffer> {
    let (tx, rx) = mpsc::channel();
    let mut app = App::new(front, tx, clock);
    app.resize(cols, rows);
    app.start();
    settle(&mut app, &rx)?;
    // 本物と同じに、キーごとに変わった所だけを描き直していく。
    let mut canvas = Canvas::new(std::io::sink());
    if let Some(damage) = app.take_damage() {
        paint(&mut canvas, &mut app, damage)?;
    }
    for ch in keys.chars() {
        let (code, modifiers) = match ch {
            '\n' => (KeyCode::Enter, KeyModifiers::NONE),
            '\t' => (KeyCode::Tab, KeyModifiers::NONE),
            '\x1b' => (KeyCode::Esc, KeyModifiers::NONE),
            '\x7f' => (KeyCode::Backspace, KeyModifiers::NONE),
            // ほかの制御文字は Ctrl と文字（`\x13` は Ctrl-s）。
            '\x01'..='\x1a' => (
                KeyCode::Char((b'a' + ch as u8 - 1) as char),
                KeyModifiers::CONTROL,
            ),
            ch => (KeyCode::Char(ch), KeyModifiers::NONE),
        };
        app.handle(Event::Input(Input::Key(KeyEvent::new(code, modifiers))));
        // 操作がだめだったときの理由は、画面に出る（スナップショットは続ける）。
        wait(&mut app, &rx)?;
        if let Some(damage) = app.take_damage() {
            paint(&mut canvas, &mut app, damage)?;
        }
    }
    whole(&mut app, canvas.shown())
}

/// すべてを描いたフレーム。変わった所だけを描き直してきた画面（`shown`）がこれと違えば、失敗。
fn whole(app: &mut App, shown: &Buffer) -> Result<Buffer> {
    let mut whole = Buffer::empty(shown.area);
    // 前のフレームから写さずに、すべてを組み立て直したものと比べる。
    app.forget_drawn();
    app.draw(&mut whole, Damage::ALL);
    let width = usize::from(shown.area.width);
    match (0..whole.content.len()).find(|&i| whole.content[i] != shown.content[i]) {
        Some(at) => Err(format!(
            "a partial redraw left a stale cell at column {}, line {}: {:?} where a full redraw has {:?}",
            at % width,
            at / width,
            shown.content[at].symbol(),
            whole.content[at].symbol()
        )
        .into()),
        None => Ok(whole),
    }
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
    /// 時間の内訳（µs）: イベントの処理（届いた答えを入れる所を含む）、セルを組み立てる、前のフレームとの差を
    /// 取って書き出す。それぞれの中央値と、最も遅いフレームでの値。
    pub median_parts_us: [u64; 3],
    pub slowest_parts_us: [u64; 3],
    /// 最も遅いフレームで、ワーカーから届いた答えの数。
    pub slowest_replies: usize,
    /// 最も遅いフレームで、このスレッドが CPU を使っていた時間（µs。読めない OS では `None`）。かかった時間より
    /// ずっと短ければ、その間スレッドは止められていた。
    pub slowest_cpu_us: Option<u64>,
}

/// このスレッドが、ここまでに CPU を使った時間。
#[cfg(unix)]
fn thread_cpu() -> Option<Duration> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `time` は、呼び出しの間だけ使う手元の値。
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) } == 0;
    ok.then(|| Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
}

#[cfg(not(unix))]
fn thread_cpu() -> Option<Duration> {
    None
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
/// 測る。`keys` なら、ホイールの代わりに `j`（端に着いたら `k`）を押して選択を動かす。入力から出力までは本物と
/// 同じ道を通し、出力先だけを数えるものに替える。
pub fn measure_scroll(
    front: Arc<dyn Front>,
    cols: u16,
    rows: u16,
    frames: usize,
    interval: Duration,
    keys: bool,
    local: fn(i64) -> String,
) -> Result<Measure> {
    let (tx, rx) = mpsc::channel();
    let mut app = App::new(front, tx, Clock { now: now(), local });
    app.resize(cols, rows);
    app.start();
    settle(&mut app, &rx)?;
    let written = Arc::new(AtomicU64::new(0));
    let mut canvas = Canvas::new(Counter(written.clone()));
    if let Some(damage) = app.take_damage() {
        paint(&mut canvas, &mut app, damage)?;
    }
    let column = app.main_column();
    let wheel = |kind| {
        Event::Input(Input::Mouse(MouseEvent {
            kind,
            // History の上（サイドバーの右、詳細の左）。
            column,
            row: rows / 2,
            modifiers: KeyModifiers::NONE,
        }))
    };
    let mut times = Vec::with_capacity(frames);
    let mut parts: Vec<([u64; 3], usize)> = Vec::with_capacity(frames);
    let mut cpu: Vec<Option<u64>> = Vec::with_capacity(frames);
    let mut sizes = Vec::with_capacity(frames);
    let mut incomplete = 0;
    let mut down = true;
    for _ in 0..frames {
        let before = written.load(Ordering::Relaxed);
        let cpu_start = thread_cpu();
        let start = Instant::now();
        // 端に着いたら折り返す。
        if app.history_at_end(down, keys) {
            down = !down;
        }
        app.handle(match (keys, down) {
            (true, _) => Event::Input(Input::Key(KeyEvent::new(
                KeyCode::Char(if down { 'j' } else { 'k' }),
                KeyModifiers::NONE,
            ))),
            (false, true) => wheel(MouseEventKind::ScrollDown),
            (false, false) => wheel(MouseEventKind::ScrollUp),
        });
        let mut replies = 0;
        while let Ok(event) = rx.try_recv() {
            app.handle(event);
            replies += 1;
        }
        let handled = start.elapsed();
        let mut built = Duration::ZERO;
        if let Some(damage) = app.take_damage() {
            let areas = app.areas(damage);
            canvas.draw(app.size(), &areas, |buf, all| {
                let before = Instant::now();
                let cursor = app.draw(buf, if all { Damage::ALL } else { damage });
                built = before.elapsed();
                cursor
            })?;
        }
        let spent = start.elapsed();
        cpu.push(
            thread_cpu()
                .zip(cpu_start)
                .map(|(end, start)| end.saturating_sub(start).as_micros() as u64),
        );
        let micros = |d: Duration| d.as_micros() as u64;
        parts.push((
            [
                micros(handled),
                micros(built),
                micros(spent.saturating_sub(handled + built)),
            ],
            replies,
        ));
        times.push(spent.as_micros() as u64);
        sizes.push(written.load(Ordering::Relaxed) - before);
        incomplete += usize::from(app.last_frame_incomplete());
        std::thread::sleep(interval.saturating_sub(spent));
    }
    // 測った後の画面が、すべてを描いたものと同じか確かめる。
    whole(&mut app, canvas.shown())?;
    // 最も遅いフレームの内訳（並べ替える前に探す）。
    let slowest = (0..times.len()).max_by_key(|&i| times[i]).unwrap_or(0);
    let (slowest_parts_us, slowest_replies) = parts.get(slowest).copied().unwrap_or_default();
    let median_of = |part: usize| {
        let mut values: Vec<u64> = parts.iter().map(|p| p.0[part]).collect();
        values.sort_unstable();
        values.get(values.len() / 2).copied().unwrap_or(0)
    };
    let median_parts_us = [median_of(0), median_of(1), median_of(2)];
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
        median_parts_us,
        slowest_parts_us,
        slowest_replies,
        slowest_cpu_us: cpu.get(slowest).copied().flatten(),
    })
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
