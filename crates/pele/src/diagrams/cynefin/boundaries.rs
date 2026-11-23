//! Mermaid の Cynefin の境の形（pele の `src/diagrams/cynefin/boundaries.ts`）: 領域の間の種つきの波線と、Clear と
//! Chaotic の間の S 字の崖。曲線は数の平らな並び: 始まりの点と、3 次の区間ごとに 6 つの数（制御点 2 つと終わりの点）。

use crate::js;
use crate::svg::builder::num;

fn imul(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

fn ushr(a: i32, n: u32) -> i32 {
    ((a as u32) >> n) as i32
}

/// mulberry32 の 1 歩。
pub fn seeded_random(seed: f64) -> f64 {
    let mut t = js::to_int32(seed + 0x6d2b79f5 as f64);
    t = imul(t ^ ushr(t, 15), t | 1);
    t ^= t.wrapping_add(imul(t ^ ushr(t, 7), t | 61));
    ((t ^ ushr(t, 14)) as u32) as f64 / 4294967296.0
}

pub fn hash_string(s: &str) -> i32 {
    let mut hash: i32 = 0;
    for c in s.encode_utf16() {
        hash = (hash << 5).wrapping_sub(hash).wrapping_add(c as i32);
    }
    hash
}

/// 0 でない設定の種が勝つ。ほかは図の id から種を作る。
pub fn resolve_seed(configured: f64, id: &str) -> f64 {
    if configured.is_finite() && configured != 0.0 {
        configured
    } else {
        hash_string(id) as f64
    }
}

#[derive(Clone, Copy)]
pub struct WaveOptions {
    pub segments: usize,
    pub vertical: bool,
    /// Mermaid が点ごと・制御点ごとに種を変える掛け数。
    pub point_step: f64,
    pub control_step: f64,
    pub control_shift: f64,
    pub pin_start: bool,
    pub pin_end: bool,
}

pub const FOLD: WaveOptions = WaveOptions {
    segments: 7,
    vertical: true,
    point_step: 17.0,
    control_step: 31.0,
    control_shift: 7.0,
    pin_start: false,
    pin_end: false,
};
pub const HORIZON: WaveOptions = WaveOptions {
    segments: 7,
    vertical: false,
    point_step: 23.0,
    control_step: 37.0,
    control_shift: 11.0,
    pin_start: false,
    pin_end: false,
};

/// 1 つの軸に沿って `from` から `to` まで、もう 1 つの軸の `center` を中心にした波線。点は振れ幅まで揺れ、
/// 留めた端は中心の線に残る。
pub fn wave(
    from: f64,
    to: f64,
    center: f64,
    seed: f64,
    amplitude: f64,
    o: WaveOptions,
) -> Vec<f64> {
    let step = (to - from) / o.segments as f64;
    let mut along = Vec::new();
    let mut across = Vec::new();
    for i in 0..=o.segments {
        let pinned = (i == 0 && o.pin_start) || (i == o.segments && o.pin_end);
        along.push(from + i as f64 * step);
        across.push(if pinned {
            center
        } else {
            center + seeded_random(seed + i as f64 * o.point_step) * amplitude * 2.0 - amplitude
        });
    }
    let mut out = if o.vertical {
        vec![across[0], along[0]]
    } else {
        vec![along[0], across[0]]
    };
    for i in 0..o.segments {
        let mid = (along[i] + along[i + 1]) / 2.0;
        let offset = amplitude
            * 1.5
            * (if i % 2 == 0 { 1.0 } else { -1.0 })
            * seeded_random(seed + i as f64 * o.control_step + o.control_shift);
        if o.vertical {
            out.extend([
                across[i] + offset,
                mid,
                across[i + 1] - offset,
                mid,
                across[i + 1],
                along[i + 1],
            ]);
        } else {
            out.extend([
                mid,
                across[i] + offset,
                mid,
                across[i + 1] - offset,
                along[i + 1],
                across[i + 1],
            ]);
        }
    }
    out
}

/// 崖: (cx, top) から (cx, bottom) へ下りる急な S。
pub fn cliff(cx: f64, top: f64, bottom: f64, amplitude: f64) -> Vec<f64> {
    let h = bottom - top;
    vec![
        cx,
        top,
        cx + amplitude,
        top + h * 0.2,
        cx - amplitude * 1.5,
        top + h * 0.55,
        cx + amplitude * 0.5,
        top + h * 0.75,
        cx - amplitude,
        top + h * 0.85,
        cx + amplitude * 0.3,
        top + h * 0.95,
        cx,
        bottom,
    ]
}

/// 曲線の始まりの点から終わりへ続く道の命令。
pub fn forward(curve: &[f64]) -> String {
    let mut d = String::new();
    let mut i = 2;
    while i < curve.len() {
        d.push_str(&format!(
            "C{},{} {},{} {},{}",
            num(curve[i]),
            num(curve[i + 1]),
            num(curve[i + 2]),
            num(curve[i + 3]),
            num(curve[i + 4]),
            num(curve[i + 5])
        ));
        i += 6;
    }
    d
}

/// 曲線の終わりの点から始まりへ戻る道の命令。
pub fn backward(curve: &[f64]) -> String {
    let mut d = String::new();
    let mut i = curve.len() as isize - 6;
    while i >= 2 {
        let k = i as usize;
        d.push_str(&format!(
            "C{},{} {},{} {},{}",
            num(curve[k + 2]),
            num(curve[k + 3]),
            num(curve[k]),
            num(curve[k + 1]),
            num(curve[k - 2]),
            num(curve[k - 1])
        ));
        i -= 6;
    }
    d
}

pub fn curve_path(curve: &[f64]) -> String {
    format!("M{},{}{}", num(curve[0]), num(curve[1]), forward(curve))
}

pub fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> String {
    let r = format!("{},{}", num(rx), num(ry));
    format!(
        "M{},{}A{r} 0 1,1 {},{}A{r} 0 1,1 {},{}Z",
        num(cx - rx),
        num(cy),
        num(cx + rx),
        num(cy),
        num(cx - rx),
        num(cy)
    )
}
