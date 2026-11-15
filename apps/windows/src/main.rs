//! Windows の FE の試作（`docs/design.md` の 19）。History を Direct2D / DirectWrite で描き、ウィンドウの作り方
//! （素の Win32 と、WinUI 3 の SwapChainPanel）で、起動の速さと画面に出るまでの遅れを比べる。
//!
//! `amonhen-win <repo> [--host win32|winui] [--measure-scroll [--scroll-rows <n>] | --measure-input <n>]
//! [--draw-on-input] [--raw-input] [--child-hwnd] [--keep-presenting] [--no-waitable] [--sync <n>]
//! [--tearing] [--profile] [--out <file>]`、`amonhen-win --listen <file>`（実のマウスの基準、`listen.rs`）

mod data;
mod listen;
mod measure;
mod render;
mod win32;
mod winui;

use std::path::PathBuf;
use std::time::Instant;

use data::Data;
use measure::{InputLatency, Measure};
use render::{Options, ROW_HEIGHT, Renderer, Surface};

#[derive(Clone)]
pub struct Args {
    pub repo: PathBuf,
    pub host: String,
    pub measure: Option<f32>,
    pub measure_input: Option<usize>,
    /// WinUI 3: クリックを受けたら次の `Rendering` を待たずに描く。
    pub draw_on_input: bool,
    /// WinUI 3: クリックを XAML より前に、子ウィンドウ（HWND）のメッセージで受ける（`--draw-on-input` も兼ねる）。
    pub raw_input: bool,
    /// 変わらなくても毎フレーム描いて present し続ける（Independent Flip から落ちないように。GPUI と同じ出し方）。
    pub keep_presenting: bool,
    /// WinUI 3: SwapChainPanel の上に普通の子ウィンドウ（ほぼ透明）を重ね、クリックをそこで受ける。
    pub child_hwnd: bool,
    pub options: Options,
    pub out: Option<PathBuf>,
}

fn parse() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        repo: PathBuf::from("."),
        host: "win32".into(),
        measure: None,
        measure_input: None,
        draw_on_input: false,
        raw_input: false,
        keep_presenting: false,
        child_hwnd: false,
        options: Options {
            waitable: true,
            sync: 1,
            tearing: false,
        },
        out: None,
    };
    let mut rows = measure::ROWS_PER_FRAME;
    let mut measuring = false;
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} に値がない"));
        match a.as_str() {
            "--host" => parsed.host = value()?,
            "--measure-scroll" => measuring = true,
            "--scroll-rows" => rows = value()?.parse().map_err(|e| format!("{e}"))?,
            "--measure-input" => {
                parsed.measure_input = Some(value()?.parse().map_err(|e| format!("{e}"))?)
            }
            "--draw-on-input" => parsed.draw_on_input = true,
            "--keep-presenting" => parsed.keep_presenting = true,
            "--child-hwnd" => {
                parsed.child_hwnd = true;
                parsed.raw_input = true;
                parsed.draw_on_input = true;
            }
            "--raw-input" => {
                parsed.raw_input = true;
                parsed.draw_on_input = true;
            }
            "--no-waitable" => parsed.options.waitable = false,
            "--sync" => parsed.options.sync = value()?.parse().map_err(|e| format!("{e}"))?,
            "--tearing" => parsed.options.tearing = true,
            "--out" => parsed.out = Some(value()?.into()),
            // SAFETY: スレッドを作る前（引数を読んでいるところ）で、ほかに環境を読む者はいない。
            "--profile" => unsafe { std::env::set_var("AMONHEN_PROFILE", "1") },
            _ if a.starts_with("--") => return Err(format!("知らない引数: {a}")),
            _ => parsed.repo = a.into(),
        }
    }
    parsed.measure = measuring.then_some(rows);
    Ok(parsed)
}

fn main() {
    let started = Instant::now();
    let mut raw = std::env::args().skip(1);
    if raw.next().as_deref() == Some("--listen") {
        if let Err(e) = listen::run(raw.next().unwrap_or_else(|| "listen.txt".into()).into()) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let result = match args.host.as_str() {
        "win32" => win32::run(args, started),
        "winui" => winui::run(args, started),
        other => Err(format!("知らない --host: {other}")),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

enum Phase {
    /// History と最初の画面の件名を待っている。
    Waiting,
    /// 測る前に何フレームか描いて落ち着かせる。
    Settling(u32),
    Running,
    Done,
}

/// どのウィンドウにも共通の、描く中身と測り方。
pub struct Session {
    pub data: Data,
    pub renderer: Renderer,
    pub surface: Option<Surface>,
    pub scale: f32,
    scroll_y: f32,
    dirty: bool,
    measure: Option<Measure>,
    input: Option<InputLatency>,
    phase: Phase,
    started: Instant,
    first_paint: bool,
    host: &'static str,
    out: Option<PathBuf>,
    keep_presenting: bool,
}

impl Session {
    pub fn new(
        args: &Args,
        host: &'static str,
        started: Instant,
        wake: data::Wake,
    ) -> Result<Self, String> {
        let renderer = Renderer::new().map_err(|e| format!("Direct2D を用意できない: {e}"))?;
        let session = Self {
            data: Data::open(args.repo.clone(), wake),
            renderer,
            surface: None,
            scale: 1.0,
            scroll_y: 0.0,
            dirty: true,
            measure: args.measure.map(Measure::new),
            input: args.measure_input.map(InputLatency::new),
            phase: Phase::Waiting,
            started,
            first_paint: false,
            keep_presenting: args.keep_presenting,
            host,
            out: args.out.clone(),
        };
        session.report(&format!(
            "host {host}, waitable {}, sync {}, tearing {} (supported {})",
            args.options.waitable,
            args.options.sync,
            args.options.tearing,
            session.renderer.tearing_supported()
        ));
        Ok(session)
    }

    fn report(&self, line: &str) {
        println!("{line}");
        if let Some(out) = &self.out {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(out)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    pub fn needs_frame(&self) -> bool {
        self.dirty
            || self.keep_presenting
            || matches!(self.phase, Phase::Settling(_) | Phase::Running)
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    fn height(&self) -> f32 {
        self.surface
            .as_ref()
            .map_or(0.0, |s| s.height as f32 / self.scale)
    }

    fn max_scroll(&self) -> f32 {
        (self.data.row_count() as f32 * ROW_HEIGHT - self.height()).max(0.0)
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, self.max_scroll());
        self.dirty = true;
    }

    pub fn scroll_to(&mut self, y: f32) {
        self.scroll_y = y.clamp(0.0, self.max_scroll());
        self.dirty = true;
    }

    pub fn page(&self) -> f32 {
        self.height() * 0.9
    }

    fn visible(&self) -> std::ops::Range<u32> {
        let count = self.data.row_count();
        let first = ((self.scroll_y / ROW_HEIGHT) as u32).min(count);
        let last = (((self.scroll_y + self.height()) / ROW_HEIGHT).ceil() as u32).min(count);
        first..last
    }

    /// クリックで 1 画面分進める（`--measure-input` の入力）。`at` は受け取った時刻（QPC）。
    pub fn clicked(&mut self, at: i64) {
        if let Some(input) = &mut self.input {
            input.received(at);
        }
        let page = self.page();
        if self.scroll_y + page > self.max_scroll() {
            self.scroll_to(0.0);
        } else {
            self.scroll_by(page);
        }
    }

    /// 画面に出るのを待っている入力があるか（あれば `poll_input` を呼び続ける）。
    pub fn awaiting_display(&self) -> bool {
        self.input.as_ref().is_some_and(|i| i.waiting())
    }

    /// 画面に出た入力を書き出す。決めた回数を測り終えたら `true`。
    pub fn poll_input(&mut self) -> bool {
        let (Some(input), Some(surface)) = (&mut self.input, &self.surface) else {
            return false;
        };
        let found = input.poll(&surface.chain);
        let done = input.finished();
        let summary = done.then(|| input.summary());
        for (received, displayed) in found {
            self.report(&format!("input received={received} displayed={displayed}"));
        }
        if let Some(summary) = summary {
            self.report(&format!("[{}] {summary}", self.host));
        }
        done
    }

    /// 裏のスレッドから届いたものを取り込む。
    pub fn poll(&mut self) -> Result<(), String> {
        if self.data.poll()? {
            self.dirty = true;
        }
        Ok(())
    }

    /// 1 フレーム描いて出す。測り終えたら `true`。
    pub fn frame(&mut self) -> Result<bool, String> {
        if self.surface.is_none() {
            return Ok(false);
        }
        let start = match (&self.phase, &mut self.measure) {
            (Phase::Running, Some(m)) => {
                let (t, dy) = (m.frame_started(), m.rows_per_frame * ROW_HEIGHT);
                self.scroll_by(dy);
                t
            }
            _ => measure::now(),
        };
        let surface = self.surface.as_mut().expect("surface");
        self.renderer
            .draw(surface, &mut self.data, self.scroll_y, self.scale)
            .map_err(|e| format!("描けない: {e}"))?;
        surface.present().map_err(|e| format!("出せない: {e}"))?;
        if let Some(input) = &mut self.input {
            input.presented(&surface.chain);
        }
        self.dirty = false;

        let ready = self.data.has_rows(self.visible());
        if ready && !self.first_paint {
            self.first_paint = true;
            self.report(&format!(
                "first History paint: {:.0} ms after start ({} rows)",
                self.started.elapsed().as_secs_f64() * 1000.0,
                self.data.row_count()
            ));
        }
        match self.phase {
            Phase::Waiting if ready && self.measure.is_some() => self.phase = Phase::Settling(30),
            Phase::Settling(0) => self.phase = Phase::Running,
            Phase::Settling(n) => self.phase = Phase::Settling(n - 1),
            Phase::Running => {
                let at_end = self.scroll_y >= self.max_scroll();
                let m = self.measure.as_mut().expect("measure");
                m.frame_presented(start, &self.surface.as_ref().expect("surface").chain);
                if m.finished() || at_end {
                    self.phase = Phase::Done;
                    let summary = m.summary();
                    self.report(&format!("[{}] {summary}", self.host));
                    if let Some(p) = &self.renderer.profile {
                        self.report(&render::profile_summary(p));
                    }
                    return Ok(true);
                }
            }
            _ => {}
        }
        Ok(false)
    }
}
