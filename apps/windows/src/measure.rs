//! `--measure-scroll`: History を毎フレーム決まった行数だけスクロールし、メインスレッドの時間と、フレームの
//! 始まり（スクロールを進めた時点）から画面に出るまでを測る（GTK4 版・macOS 版の `--measure-scroll` と同じ考え）。
//!
//! 画面に出た時刻は DXGI のフレームの記録（`GetFrameStatistics` の `SyncQPCTime`、その present が出た垂直同期の
//! 時刻）。記録は最後に出た present の分しかないので、毎フレーム読む。どの経路で出たか（Independent Flip か
//! DWM の合成か）は分からないので、PresentMon で見る。

use std::collections::VecDeque;

use windows::Win32::Graphics::Dxgi::{DXGI_FRAME_STATISTICS, IDXGISwapChain2};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub const FRAMES: usize = 600;
/// 1 フレームにスクロールする行数の既定（`--scroll-rows`）。
pub const ROWS_PER_FRAME: f32 = 40.0;

pub fn now() -> i64 {
    let mut t = 0;
    unsafe {
        let _ = QueryPerformanceCounter(&mut t);
    }
    t
}

fn frequency() -> f64 {
    let mut f = 0;
    unsafe {
        let _ = QueryPerformanceFrequency(&mut f);
    }
    f as f64
}

pub struct Measure {
    pub rows_per_frame: f32,
    freq: f64,
    /// フレームの始まりから present を返すまで。
    work: Vec<f64>,
    /// フレームの始まりの間隔。
    intervals: Vec<f64>,
    /// フレームの始まりから画面に出るまで。
    presented: Vec<f64>,
    /// 画面に出た時刻が分からなかったフレームの数。
    unknown: usize,
    /// フレームの記録が読めなかった回数。
    no_statistics: usize,
    pending: VecDeque<(u32, i64)>,
    last_start: Option<i64>,
}

impl Measure {
    pub fn new(rows_per_frame: f32) -> Self {
        Self {
            rows_per_frame,
            freq: frequency(),
            work: Vec::new(),
            intervals: Vec::new(),
            presented: Vec::new(),
            unknown: 0,
            no_statistics: 0,
            pending: VecDeque::new(),
            last_start: None,
        }
    }

    fn ms(&self, ticks: i64) -> f64 {
        ticks as f64 * 1000.0 / self.freq
    }

    pub fn frame_started(&mut self) -> i64 {
        let t = now();
        if let Some(last) = self.last_start {
            self.intervals.push(self.ms(t - last));
        }
        self.last_start = Some(t);
        t
    }

    pub fn frame_presented(&mut self, start: i64, chain: &IDXGISwapChain2) {
        self.work.push(self.ms(now() - start));
        let Ok(count) = (unsafe { chain.GetLastPresentCount() }) else {
            return;
        };
        self.pending.push_back((count, start));
        let mut stats = DXGI_FRAME_STATISTICS::default();
        if unsafe { chain.GetFrameStatistics(&mut stats) }.is_err() || stats.PresentCount == 0 {
            self.no_statistics += 1;
            // 古いものは諦める。
            while self.pending.len() > 8 {
                self.pending.pop_front();
                self.unknown += 1;
            }
            return;
        }
        while let Some(&(id, start)) = self.pending.front() {
            if id > stats.PresentCount {
                break;
            }
            self.pending.pop_front();
            if id == stats.PresentCount {
                self.presented.push(self.ms(stats.SyncQPCTime - start));
            } else {
                // 間に読めなかった（2 つ以上まとめて出た、または飛ばされた）。
                self.unknown += 1;
            }
        }
    }

    pub fn finished(&self) -> bool {
        self.work.len() >= FRAMES
    }

    pub fn summary(&self) -> String {
        let refresh = median(&self.intervals);
        let dropped = self
            .intervals
            .iter()
            .filter(|&&i| refresh > 0.0 && i > refresh * 1.5)
            .count();
        format!(
            "frames {} ({:.0} rows/frame), refresh ~{:.2} ms\n\
             main thread per frame: p50 {} / p95 {} / max {} ms\n\
             frame interval: p50 {} / p95 {} ms, dropped {} of {}\n\
             frame start to display (DXGI SyncQPCTime): p50 {} / p95 {} ms ({} known, {} unknown, {} without statistics)",
            self.work.len(),
            self.rows_per_frame,
            refresh,
            percentile(&self.work, 0.5),
            percentile(&self.work, 0.95),
            percentile(&self.work, 1.0),
            percentile(&self.intervals, 0.5),
            percentile(&self.intervals, 0.95),
            dropped,
            self.intervals.len(),
            percentile(&self.presented, 0.5),
            percentile(&self.presented, 0.95),
            self.presented.len(),
            self.unknown,
            self.no_statistics,
        )
    }
}

fn sorted(values: &[f64]) -> Vec<f64> {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v
}

fn median(values: &[f64]) -> f64 {
    let v = sorted(values);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

fn percentile(values: &[f64], p: f64) -> String {
    if values.is_empty() {
        return "-".into();
    }
    let v = sorted(values);
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    format!("{:.2}", v[i])
}

/// `--measure-input <n>`: クリックを受け取ってから、それを映したフレームが画面に出るまで。送る側（`input.ps1`）が
/// `SendInput` の前に記録した時刻と合わせるため、1 回ごとに受け取った時刻と出た時刻（どちらも QPC）を書き出す。
pub struct InputLatency {
    target: usize,
    freq: f64,
    /// まだ描いていない入力を受け取った時刻。
    pending: Vec<i64>,
    /// 出した present の番号と、それが映す入力。
    inflight: VecDeque<(u32, Vec<i64>)>,
    /// （受け取った、画面に出た）。
    pub results: Vec<(i64, i64)>,
    pub unknown: usize,
}

impl InputLatency {
    pub fn new(target: usize) -> Self {
        Self {
            target,
            freq: frequency(),
            pending: Vec::new(),
            inflight: VecDeque::new(),
            results: Vec::new(),
            unknown: 0,
        }
    }

    pub fn received(&mut self, at: i64) {
        self.pending.push(at);
    }

    /// 出した present が、受け取ったままの入力を映している。
    pub fn presented(&mut self, chain: &IDXGISwapChain2) {
        if self.pending.is_empty() {
            return;
        }
        if let Ok(count) = unsafe { chain.GetLastPresentCount() } {
            self.inflight
                .push_back((count, std::mem::take(&mut self.pending)));
        }
    }

    pub fn waiting(&self) -> bool {
        !self.inflight.is_empty()
    }

    /// 画面に出たものを読み、新しく分かった（受け取った、出た）を返す。
    pub fn poll(&mut self, chain: &IDXGISwapChain2) -> Vec<(i64, i64)> {
        let mut stats = DXGI_FRAME_STATISTICS::default();
        if unsafe { chain.GetFrameStatistics(&mut stats) }.is_err() || stats.PresentCount == 0 {
            return Vec::new();
        }
        let mut found = Vec::new();
        while let Some((id, _)) = self.inflight.front() {
            if *id > stats.PresentCount {
                break;
            }
            let (id, inputs) = self.inflight.pop_front().expect("front");
            if id == stats.PresentCount {
                found.extend(inputs.into_iter().map(|r| (r, stats.SyncQPCTime)));
            } else {
                self.unknown += inputs.len();
            }
        }
        self.results.extend(&found);
        found
    }

    pub fn finished(&self) -> bool {
        self.results.len() + self.unknown >= self.target
    }

    pub fn summary(&self) -> String {
        let ms: Vec<f64> = self
            .results
            .iter()
            .map(|(r, d)| (d - r) as f64 * 1000.0 / self.freq)
            .collect();
        format!(
            "input received to display: p50 {} / p95 {} / max {} ms ({} known, {} unknown)",
            percentile(&ms, 0.5),
            percentile(&ms, 0.95),
            percentile(&ms, 1.0),
            ms.len(),
            self.unknown
        )
    }
}
