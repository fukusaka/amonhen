//! GPUI（Zed の UI）で、Win32・WinUI 3 の試作と同じ History を描く。窓・入力・フレームの出し方は GPUI に
//! 任せ（DirectComposition の swap chain、垂直同期のスレッドで描く）、同じ方法で測って比べる。
//!
//! `amonhen-win-gpui <repo> [--measure-scroll [--scroll-rows <n>] | --measure-input <n>] [--out <file>]`
//!
//! GPUI は swap chain を見せないので、画面に出た時刻は PresentMon で読む（`measure-input.ps1` が
//! `input received=<QPC>` と PresentMon の `--qpc_time` の記録を突き合わせる）。

#[path = "../../src/data.rs"]
mod data;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use amonhen_core::HistorySnapshot;
use amonhen_core::lanes::{LaneMetrics, lane_positions};
use amonhen_layout::Edge;
use futures::StreamExt as _;
use gpui::{
    App, Application, Bounds, ContentMask, Context, Hsla, MouseButton, MouseDownEvent, PathBuilder,
    Pixels, Point, Render, Rgba, ScrollWheelEvent, SharedString, TextRun, Window, WindowBounds,
    WindowOptions, canvas, div, fill, font, point, prelude::*, px, size,
};

use data::{Chunk, Data};

const ROW_HEIGHT: f32 = 38.0;
const LANE_WIDTH: f32 = 14.0;
const LEFT_INSET: f32 = 10.0;
const NODE_RADIUS: f32 = 4.0;
const LINE_WIDTH: f32 = 1.6;
const FIRST_PARENT_WIDTH: f32 = 2.4;
const GRAPH_WIDTH: f32 = 160.0;
const TEXT_INSET: f32 = 8.0;
const SIDEBAR: f32 = 220.0;
const FRAMES: usize = 600;

const PALETTE: [[f32; 3]; 8] = [
    [0.30, 0.55, 0.95],
    [0.93, 0.45, 0.30],
    [0.35, 0.75, 0.45],
    [0.75, 0.45, 0.85],
    [0.95, 0.70, 0.25],
    [0.30, 0.75, 0.80],
    [0.90, 0.40, 0.60],
    [0.60, 0.60, 0.65],
];

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    Rgba { r, g, b, a }.into()
}

fn palette(index: u16) -> Hsla {
    let [r, g, b] = PALETTE[index as usize % PALETTE.len()];
    rgba(r, g, b, 1.0)
}

fn now() -> i64 {
    let mut t = 0;
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut t);
    }
    t
}

fn frequency() -> f64 {
    let mut f = 0;
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f);
    }
    f as f64
}

#[derive(Clone)]
struct Args {
    repo: PathBuf,
    measure: Option<f32>,
    measure_input: Option<usize>,
    out: Option<PathBuf>,
}

fn parse() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        repo: ".".into(),
        measure: None,
        measure_input: None,
        out: None,
    };
    let mut rows = 40.0;
    let mut measuring = false;
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} に値がない"));
        match a.as_str() {
            "--measure-scroll" => measuring = true,
            "--scroll-rows" => rows = value()?.parse().map_err(|e| format!("{e}"))?,
            "--measure-input" => {
                parsed.measure_input = Some(value()?.parse().map_err(|e| format!("{e}"))?)
            }
            "--out" => parsed.out = Some(value()?.into()),
            // HWND の swap chain にする（GPUI の既定は DirectComposition）。
            // SAFETY: スレッドを作る前（引数を読んでいるところ）で、ほかに環境を読む者はいない。
            "--no-dcomp" => unsafe { std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1") },
            _ if a.starts_with("--") => return Err(format!("知らない引数: {a}")),
            _ => parsed.repo = a.into(),
        }
    }
    parsed.measure = measuring.then_some(rows);
    Ok(parsed)
}

fn report(out: &Option<PathBuf>, line: &str) {
    println!("{line}");
    if let Some(out) = out {
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

enum Phase {
    Waiting,
    Settling(u32),
    Running,
    Done,
}

/// フレームごとの記録。描く処理（canvas の paint）からも書くので共有する。
#[derive(Default)]
struct Stats {
    /// render の始まりから canvas を描き終えるまで（GPUI の layout・prepaint・paint を含む。GPU に出す分は含まない）。
    work: Vec<f64>,
    intervals: Vec<f64>,
    last_start: Option<i64>,
}

struct HistoryView {
    data: Data,
    args: Args,
    started: Instant,
    scroll_y: f32,
    height: f32,
    phase: Phase,
    first_paint: bool,
    stats: Rc<RefCell<Stats>>,
    freq: f64,
    clicks: usize,
    rows: HashMap<u32, (SharedString, SharedString)>,
}

impl HistoryView {
    fn max_scroll(&self) -> f32 {
        (self.data.row_count() as f32 * ROW_HEIGHT - self.height).max(0.0)
    }

    fn scroll_by(&mut self, dy: f32) {
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, self.max_scroll());
    }

    fn visible(&self) -> std::ops::Range<u32> {
        let count = self.data.row_count();
        let first = ((self.scroll_y / ROW_HEIGHT).floor().max(0.0) as u32).min(count);
        let last = (((self.scroll_y + self.height) / ROW_HEIGHT).ceil() as u32 + 1).min(count);
        first..last
    }

    fn clicked(&mut self, at: i64, cx: &mut Context<Self>) {
        let Some(target) = self.args.measure_input else {
            return;
        };
        report(&self.args.out, &format!("input received={at}"));
        // 1 画面ずつ送り、終わりまで来たら先頭へ。
        if self.scroll_y >= self.max_scroll() {
            self.scroll_y = 0.0;
        } else {
            self.scroll_by(self.height);
        }
        self.clicks += 1;
        if self.clicks >= target {
            let out = self.args.out.clone();
            cx.spawn(async move |_, cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                report(&out, &format!("input clicks {target} done"));
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        }
        cx.notify();
    }

    fn row_text(&mut self, history: &HistorySnapshot, chunk: &Chunk, row: u32) {
        if self.rows.contains_key(&row) {
            return;
        }
        if self.rows.len() > 4096 {
            self.rows.clear();
        }
        let i = chunk.index(row);
        let mut subject = Vec::with_capacity(96);
        chunk.push_utf16(chunk.subject(i), &mut subject);
        let mut author = Vec::with_capacity(32);
        chunk.push_utf16(chunk.author(i), &mut author);
        let id = history.graph.ids()[row as usize].to_string();
        let meta = format!(
            "{}  ·  {}  ·  {}",
            String::from_utf16_lossy(&author),
            date(chunk.time(i)),
            &id[..8]
        );
        self.rows.insert(
            row,
            (String::from_utf16_lossy(&subject).into(), meta.into()),
        );
    }
}

/// 1 フレームに描くもの（render で集め、canvas の paint で描く）。
struct Frame {
    history: Arc<HistorySnapshot>,
    rows: std::ops::Range<u32>,
    texts: Vec<(u32, SharedString, SharedString)>,
    scroll_y: f32,
    start: i64,
    stats: Rc<RefCell<Stats>>,
    freq: f64,
    measuring: bool,
}

impl Render for HistoryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut start = now();
        if let Err(e) = self.data.poll() {
            eprintln!("{e}");
            std::process::exit(1);
        }
        self.height = f32::from(window.viewport_size().height);
        let measuring = matches!(self.phase, Phase::Running);
        if measuring {
            let mut stats = self.stats.borrow_mut();
            start = now();
            if let Some(last) = stats.last_start {
                stats
                    .intervals
                    .push((start - last) as f64 * 1000.0 / self.freq);
            }
            stats.last_start = Some(start);
            drop(stats);
            let dy = self.args.measure.unwrap_or(40.0) * ROW_HEIGHT;
            self.scroll_by(dy);
        }

        let rows = self.visible();
        let screen = rows.end - rows.start;
        let count = self.data.row_count();
        self.data
            .want(rows.start.saturating_sub(screen)..(rows.end + screen).min(count));
        let ready = self.data.has_rows(rows.clone());
        let mut texts = Vec::new();
        if let Some(history) = self.data.history.clone() {
            for row in rows.clone() {
                if let Some(chunk) = self.data.chunks.get(&(row / data::CHUNK_ROWS)).cloned() {
                    self.row_text(&history, &chunk, row);
                    let (s, m) = &self.rows[&row];
                    texts.push((row, s.clone(), m.clone()));
                }
            }
        }

        if ready && !self.first_paint && self.data.history.is_some() {
            self.first_paint = true;
            report(
                &self.args.out,
                &format!(
                    "first History paint: {:.0} ms after start ({} rows)",
                    self.started.elapsed().as_secs_f64() * 1000.0,
                    count
                ),
            );
        }
        match self.phase {
            Phase::Waiting if ready && self.args.measure.is_some() => {
                self.phase = Phase::Settling(30)
            }
            Phase::Settling(0) => self.phase = Phase::Running,
            Phase::Settling(n) => self.phase = Phase::Settling(n - 1),
            Phase::Running => {
                let done =
                    self.stats.borrow().work.len() >= FRAMES || self.scroll_y >= self.max_scroll();
                if done {
                    self.phase = Phase::Done;
                    report(
                        &self.args.out,
                        &summary(&self.stats.borrow(), self.args.measure),
                    );
                    cx.spawn(async move |_, cx| {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        let _ = cx.update(|cx| cx.quit());
                    })
                    .detach();
                }
            }
            _ => {}
        }
        if matches!(self.phase, Phase::Settling(_) | Phase::Running) {
            window.request_animation_frame();
        }

        let frame = self.data.history.clone().map(|history| Frame {
            history,
            rows,
            texts,
            scroll_y: self.scroll_y,
            start,
            stats: self.stats.clone(),
            freq: self.freq,
            measuring,
        });
        let sidebar = div()
            .w(px(SIDEBAR))
            .h_full()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .bg(rgba(0.95, 0.95, 0.96, 1.0))
            .child(div().text_size(px(12.0)).child("Workspace"))
            .child("Working Copy")
            .child("History")
            .child(div().text_size(px(12.0)).child("Branches"))
            .child("main");
        let list = div()
            .id("history")
            .flex_1()
            .h_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                    let at = now();
                    this.clicked(at, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _window, cx| {
                let d = e.delta.pixel_delta(px(ROW_HEIGHT));
                this.scroll_by(-f32::from(d.y));
                cx.notify();
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        if let Some(frame) = frame {
                            paint(bounds, frame, window, cx);
                        }
                    },
                )
                .size_full(),
            );
        div()
            .flex()
            .size_full()
            .bg(rgba(1.0, 1.0, 1.0, 1.0))
            .text_color(rgba(0.11, 0.11, 0.12, 1.0))
            .font_family("Segoe UI")
            .text_size(px(13.0))
            .child(sidebar)
            .child(list)
    }
}

fn paint(bounds: Bounds<Pixels>, frame: Frame, window: &mut Window, cx: &mut App) {
    let history = &frame.history;
    let origin = bounds.origin;
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let lanes = history.layout.lane_count() as f32;
    let graph_width = (LEFT_INSET + lanes * LANE_WIDTH)
        .clamp(40.0, GRAPH_WIDTH)
        .min(width * 0.4);
    let at = |x: f32, y: f32| -> Point<Pixels> { point(origin.x + px(x), origin.y + px(y)) };

    // グラフ。
    let metrics = LaneMetrics {
        full: LANE_WIDTH,
        thin: 3.0,
        min_full: 7.0,
        min_thin: 0.0,
        available: graph_width - (LEFT_INSET - LANE_WIDTH / 2.0) - 4.0,
    };
    let count = history.layout.nodes().len() as u32;
    let rows = frame.rows.clone();
    let around = rows.start.saturating_sub(4)..(rows.end + 4).min(count);
    let centers = lane_positions(&history.layout, around, &metrics).0;
    let x = |lane: u16| {
        let c = centers
            .get(lane as usize)
            .copied()
            .unwrap_or(lane as f32 * LANE_WIDTH + LANE_WIDTH / 2.0);
        LEFT_INSET - LANE_WIDTH / 2.0 + c
    };
    let row_y = |row: u32| (row as f32 + 0.5) * ROW_HEIGHT - frame.scroll_y;
    let mut groups: HashMap<(u16, bool), Vec<&Edge>> = HashMap::new();
    let edges = history.layout.edges();
    for i in history.layout.edges_in(rows.clone()) {
        let e = &edges[i as usize];
        let first_parent = e.kind & Edge::FIRST_PARENT != 0;
        groups
            .entry((e.color % PALETTE.len() as u16, first_parent))
            .or_default()
            .push(e);
    }
    let graph_mask = ContentMask {
        bounds: Bounds::new(origin, size(px(graph_width), bounds.size.height)),
    };
    window.with_content_mask(Some(graph_mask), |window| {
        for ((color, first_parent), edges) in groups {
            let w = if first_parent {
                FIRST_PARENT_WIDTH
            } else {
                LINE_WIDTH
            };
            let mut b = PathBuilder::stroke(px(w));
            for e in edges {
                let (ax, ay) = (x(e.lane_start), row_y(e.row_start));
                let (bx, by) = (x(e.lane_end), row_y(e.row_end));
                if e.lane_start == e.lane_end {
                    b.move_to(at(ax, ay.max(-ROW_HEIGHT)));
                    b.line_to(at(bx, by.min(height + ROW_HEIGHT)));
                } else {
                    let my = (ay + by) / 2.0;
                    b.move_to(at(ax, ay));
                    b.cubic_bezier_to(at(bx, by), at(ax, my), at(bx, my));
                }
            }
            if let Ok(path) = b.build() {
                window.paint_path(path, palette(color));
            }
        }
        let nodes = history.layout.nodes();
        for n in &nodes[rows.start as usize..rows.end as usize] {
            let c = at(x(n.lane) - NODE_RADIUS, row_y(n.row) - NODE_RADIUS);
            window.paint_quad(
                fill(
                    Bounds::new(c, size(px(NODE_RADIUS * 2.0), px(NODE_RADIUS * 2.0))),
                    palette(n.color),
                )
                .corner_radii(px(NODE_RADIUS)),
            );
        }
    });

    // 文字。
    let text_x = graph_width + TEXT_INSET;
    let text_mask = ContentMask {
        bounds: Bounds::new(
            at(text_x, 0.0),
            size(
                px((width - TEXT_INSET - text_x).max(0.0)),
                bounds.size.height,
            ),
        ),
    };
    let foreground = rgba(0.11, 0.11, 0.12, 1.0);
    let secondary = rgba(0.11, 0.11, 0.12, 0.6);
    window.with_content_mask(Some(text_mask), |window| {
        for (row, subject, meta) in frame.texts {
            let top = row as f32 * ROW_HEIGHT - frame.scroll_y;
            for (text, size, color, dy) in [
                (subject, 13.0, foreground, 2.0),
                (meta, 11.0, secondary, 20.0),
            ] {
                let run = TextRun {
                    len: text.len(),
                    font: font("Segoe UI"),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window
                    .text_system()
                    .shape_line(text, px(size), &[run], None);
                let _ = line.paint(at(text_x, top + dy), px(size * 1.3), window, cx);
            }
        }
    });

    if frame.measuring {
        frame
            .stats
            .borrow_mut()
            .work
            .push((now() - frame.start) as f64 * 1000.0 / frame.freq);
    }
}

fn percentile(values: &[f64], p: f64) -> String {
    if values.is_empty() {
        return "-".into();
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    format!("{:.2}", v[((v.len() - 1) as f64 * p).round() as usize])
}

fn summary(stats: &Stats, rows: Option<f32>) -> String {
    let mut v = stats.intervals.clone();
    v.sort_by(f64::total_cmp);
    let refresh = v.get(v.len() / 2).copied().unwrap_or(0.0);
    let dropped = stats
        .intervals
        .iter()
        .filter(|&&i| refresh > 0.0 && i > refresh * 1.5)
        .count();
    format!(
        "[gpui] frames {} ({:.0} rows/frame), refresh ~{:.2} ms\n\
         main thread per frame (render to end of paint): p50 {} / p95 {} / max {} ms\n\
         frame interval: p50 {} / p95 {} ms, dropped {} of {}",
        stats.work.len(),
        rows.unwrap_or(0.0),
        refresh,
        percentile(&stats.work, 0.5),
        percentile(&stats.work, 0.95),
        percentile(&stats.work, 1.0),
        percentile(&stats.intervals, 0.5),
        percentile(&stats.intervals, 0.95),
        dropped,
        stats.intervals.len(),
    )
}

/// UTC の秒を `YYYY-MM-DD HH:MM` にする（試作なので地方時に直さない）。
fn date(utc: i64) -> String {
    let days = utc.div_euclid(86400);
    let secs = utc.rem_euclid(86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60
    )
}

fn main() {
    let started = Instant::now();
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Amon Hen (GPUI)".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let window = cx
            .open_window(options, |_, cx| {
                let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
                let wake: data::Wake = Arc::new(move || {
                    let _ = tx.unbounded_send(());
                });
                let view = cx.new(|_| HistoryView {
                    data: Data::open(args.repo.clone(), wake),
                    args: args.clone(),
                    started,
                    scroll_y: 0.0,
                    height: 800.0,
                    phase: Phase::Waiting,
                    first_paint: false,
                    stats: Rc::new(RefCell::new(Stats::default())),
                    freq: frequency(),
                    clicks: 0,
                    rows: HashMap::new(),
                });
                // 裏のスレッドから届いたら描き直す。
                let weak = view.downgrade();
                cx.spawn(async move |cx| {
                    while rx.next().await.is_some() {
                        if weak.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                })
                .detach();
                view
            })
            .expect("窓を開けない");
        let _ = window;
        cx.activate(true);
    });
}
