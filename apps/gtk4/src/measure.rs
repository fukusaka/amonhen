//! `--measure-scroll`: History を毎フレーム決まった行数だけスクロールし、メインスレッドの時間と
//! 落としたフレームを数える（macOS 版の `--measure-scroll` と同じ考え）。
//!
//! `--measure-select <n>`: History の行を n 個順に選び、選んでから詳細・差分が出るまでの段階ごとの時間を測る
//! （macOS 版の `--measure-select` と `FrameProfile`）。
//!
//! 画面に出るまでの遅れは、GTK のフレームの記録（`GdkFrameTimings`）の、フレームの始まり（スクロールを
//! 進めた時点）から、コンポジタが画面に出したと知らせた時刻（presentation time）まで。GPU の仕事や
//! コンポジタの合成も含む。知らせはフレームを描いた後で届くので、数フレーム後に読む。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::Instant;

use gtk::glib;
use gtk::prelude::*;

use crate::history::HistoryView;

const FRAMES: usize = 600;
/// 1 フレームにスクロールする行数の既定（`--scroll-rows`）。
pub const ROWS_PER_FRAME: f64 = 40.0;

#[derive(Default)]
struct Record {
    /// フレームの始まり（update）から描き終わり（after-paint）まで。
    work: Vec<f64>,
    /// フレームの時刻の間隔。
    intervals: Vec<f64>,
    started: Option<Instant>,
    last_frame: Option<i64>,
    refresh: Option<f64>,
    /// 画面に出た時刻をまだ読んでいないフレームの番号。
    unpresented: VecDeque<i64>,
    /// フレームの始まりから画面に出るまで。
    presented: Vec<f64>,
    /// 画面に出た時刻が分からなかったフレームの数。
    unknown: usize,
}

impl Record {
    /// 記録の揃ったフレームの、画面に出るまでの時間を読む（GTK は最近の十数フレームしか持たない）。
    fn read_presented(&mut self, clock: &gtk::gdk::FrameClock, current: i64) {
        while let Some(&counter) = self.unpresented.front() {
            let timings = clock.timings(counter);
            match timings {
                Some(t) if t.is_complete() => {
                    let (start, shown) = (t.frame_time(), t.presentation_time());
                    if shown > 0 {
                        self.presented.push((shown - start) as f64 / 1000.0);
                    } else {
                        self.unknown += 1;
                    }
                }
                // まだ揃っていない。古くなりすぎたら諦める。
                Some(_) if current - counter < 12 => return,
                _ => self.unknown += 1,
            }
            self.unpresented.pop_front();
        }
    }
}

pub fn scroll(window: &gtk::ApplicationWindow, view: &HistoryView, rows_per_frame: f64) {
    let Some(clock) = view.frame_clock() else {
        eprintln!("frame clock がない");
        return;
    };
    view.imp_snapshot_times(true);
    let record = Rc::new(RefCell::new(Record::default()));
    let r = record.clone();
    let update = clock.connect_update(move |_| {
        r.borrow_mut().started = Some(Instant::now());
    });
    let r = record.clone();
    let after = clock.connect_after_paint(move |clock| {
        let mut r = r.borrow_mut();
        if let Some(t) = r.started.take() {
            r.work.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let time = clock.frame_time();
        let current = clock.frame_counter();
        r.unpresented.push_back(current);
        r.read_presented(clock, current);
        if let Some(last) = r.last_frame {
            r.intervals.push((time - last) as f64 / 1000.0);
        }
        r.last_frame = Some(time);
        if r.refresh.is_none() {
            let (refresh, _) = clock.refresh_info(0);
            if refresh > 0 {
                r.refresh = Some(refresh as f64 / 1000.0);
            }
        }
    });
    let frame = std::cell::Cell::new(0);
    let window = window.downgrade();
    let handlers = RefCell::new(Some((update, after)));
    view.add_tick_callback(move |view, clock| {
        frame.set(frame.get() + 1);
        let frame = frame.get();
        let adj = view.vadjustment().expect("vadjustment");
        let max = adj.upper() - adj.page_size();
        adj.set_value((adj.value() + rows_per_frame * view.row_height() as f64).min(max));
        if frame < FRAMES && adj.value() < max {
            return glib::ControlFlow::Continue;
        }
        if let Some((update, after)) = handlers.take() {
            clock.disconnect(update);
            clock.disconnect(after);
        }
        let snapshots = view.imp_snapshot_times(false);
        println!("{}", summary(&record.borrow(), &snapshots, frame));
        if let Some(w) = window.upgrade() {
            w.close();
        }
        glib::ControlFlow::Break
    });
}

fn percentile(values: &[f64], p: f64) -> String {
    if values.is_empty() {
        return "-".into();
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    format!("{:.2}", v[i])
}

fn summary(r: &Record, snapshots: &[f64], frames: usize) -> String {
    let refresh = r.refresh.unwrap_or(1000.0 / 60.0);
    // 間隔がリフレッシュの 1.5 倍を超えたら、その分のフレームを落としたとみなす。
    let dropped: usize = r
        .intervals
        .iter()
        .map(|&i| ((i / refresh).round() as usize).saturating_sub(1))
        .sum();
    format!(
        "frames {frames} (refresh {refresh:.2} ms, dropped {dropped})\n\
         main thread per frame: p50 {} / p95 {} / max {} ms\n\
         history snapshot: p50 {} / p95 {} / max {} ms\n\
         frame interval: p50 {} / p95 {} ms\n\
         frame start to presented: p50 {} / p95 {} / max {} ms ({} frames, {} unknown)",
        percentile(&r.work, 0.5),
        percentile(&r.work, 0.95),
        percentile(&r.work, 1.0),
        percentile(snapshots, 0.5),
        percentile(snapshots, 0.95),
        percentile(snapshots, 1.0),
        percentile(&r.intervals, 0.5),
        percentile(&r.intervals, 0.95),
        percentile(&r.presented, 0.5),
        percentile(&r.presented, 0.95),
        percentile(&r.presented, 1.0),
        r.presented.len(),
        r.unknown + r.unpresented.len(),
    )
}

/// `--measure-select` の記録（選ぶたびに 1 つ。macOS 版の `FrameProfile` の frames）。
#[derive(Default)]
struct Selections {
    /// 今の選択を始めた時刻。
    selected_at: Option<Instant>,
    current: HashMap<&'static str, f64>,
    /// 段階の名前（出てきた順）。
    order: Vec<&'static str>,
    done: Vec<HashMap<&'static str, f64>>,
    /// 差分が出たら次を選ぶ。
    next: Option<Rc<dyn Fn()>>,
}

thread_local! {
    static SELECTIONS: RefCell<Option<Selections>> = const { RefCell::new(None) };
}

/// `--measure-select` の間: 選んでから今までの時間を `name` に足す。
pub fn select_mark(name: &'static str) {
    SELECTIONS.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut()
            && let Some(at) = s.selected_at
        {
            s.add(name, at.elapsed().as_secs_f64() * 1000.0);
        }
    });
}

/// `--measure-select` の間: 裏で測った時間を `name` に足す。
pub fn select_add(name: &'static str, ms: f64) {
    SELECTIONS.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut()
            && s.selected_at.is_some()
        {
            s.add(name, ms);
        }
    });
}

/// `--measure-select` の間: 差分を出した（読めなかったときも）。この選択を締めて、次を選ぶ。
pub fn select_shown() {
    select_mark("select: → diff shown");
    let next = SELECTIONS.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut()?;
        s.selected_at?;
        s.selected_at = None;
        let current = std::mem::take(&mut s.current);
        s.done.push(current);
        s.next.clone()
    });
    if let Some(next) = next {
        glib::idle_add_local_once(move || next());
    }
}

impl Selections {
    fn add(&mut self, name: &'static str, ms: f64) {
        if !self.order.contains(&name) {
            self.order.push(name);
        }
        *self.current.entry(name).or_default() += ms;
    }

    /// 段階ごとの p50 / p95（ミリ秒）。
    fn summary(&self) -> String {
        self.order
            .iter()
            .map(|name| {
                let values: Vec<f64> = self
                    .done
                    .iter()
                    .map(|f| f.get(name).copied().unwrap_or(0.0))
                    .collect();
                format!(
                    "  {name:<36} p50 {}  p95 {} ms",
                    percentile(&values, 0.5),
                    percentile(&values, 0.95)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// History の行を `count` 個順に選び（前の差分が出てから次を）、段階ごとの時間を出して終わる。行は History 全体に
/// 散らす（キャッシュに乗らないように。macOS 版と同じ選び方）。
pub fn select(window: &gtk::ApplicationWindow, view: &HistoryView, count: usize) {
    let total = view
        .current()
        .map_or(0, |(_, history)| history.graph.ids().len());
    if total < 2 {
        eprintln!("History のコミットが 2 つより少ない");
        window.close();
        return;
    }
    let done = Rc::new(std::cell::Cell::new(0usize));
    let (window, view) = (window.downgrade(), view.downgrade());
    let next: Rc<dyn Fn()> = Rc::new(move || {
        let n = done.get();
        if n >= count {
            let summary = SELECTIONS.with(|s| s.borrow_mut().take().map(|s| s.summary()));
            println!("selections: {count}\n{}", summary.unwrap_or_default());
            if let Some(w) = window.upgrade() {
                w.close();
            }
            return;
        }
        // 起動時に選んである先頭の行は避ける（選び直しても読み込みが起きない）。
        let mut row = 1 + (n * 7919) % (total - 1).max(1);
        done.set(n + 1);
        // 今選んでいる行と同じなら、隣に（選び直しにならず、差分を読まないので止まってしまう）。
        if let Some(view) = view.upgrade()
            && view.selected() == Some(row as u32)
        {
            row = (row + 1) % total;
        }
        SELECTIONS.with(|s| {
            if let Some(s) = s.borrow_mut().as_mut() {
                s.selected_at = Some(Instant::now());
            }
        });
        if let Some(view) = view.upgrade() {
            view.select(row as u32);
        }
    });
    SELECTIONS.with(|s| {
        s.replace(Some(Selections {
            next: Some(next.clone()),
            ..Selections::default()
        }))
    });
    next();
}
