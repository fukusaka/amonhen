//! `--measure-scroll`: History を毎フレーム決まった行数だけスクロールし、メインスレッドの時間と
//! 落としたフレームを数える（macOS 版の `--measure-scroll` と同じ考え）。

use std::cell::RefCell;
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
         frame interval: p50 {} / p95 {} ms",
        percentile(&r.work, 0.5),
        percentile(&r.work, 0.95),
        percentile(&r.work, 1.0),
        percentile(snapshots, 0.5),
        percentile(snapshots, 0.95),
        percentile(snapshots, 1.0),
        percentile(&r.intervals, 0.5),
        percentile(&r.intervals, 0.95),
    )
}
