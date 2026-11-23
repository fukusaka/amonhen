//! 目盛り（pele の `src/diagrams/xychart/ticks.ts`）。

use crate::js;

/// `10 ** e`。
fn pow10(e: f64) -> f64 {
    10f64.powf(e)
}

/// `span` をおよそ `count` に分ける、10 の冪の 1・2・5 倍の刻み。1 より小さい刻みは、整数になる逆数の負として
/// 返す。目盛りの値を割り算で求め、丸めの誤差なしに書けるように（0.30000000000000004 でなく 0.3）。
pub fn tick_step(span: f64, count: f64) -> f64 {
    let raw = span / js::max(count, 1.0);
    let power = raw.log10().floor();
    let unit = raw / pow10(power);
    let factor = if unit >= 7.07 {
        10.0
    } else if unit >= 3.16 {
        5.0
    } else if unit >= 1.41 {
        2.0
    } else {
        1.0
    };
    if power < 0.0 {
        -pow10(-power) / factor
    } else {
        factor * pow10(power)
    }
}

pub fn tick_index(step: f64, value: f64) -> f64 {
    if step > 0.0 {
        value / step
    } else {
        value * -step
    }
}

pub fn tick_value(step: f64, index: f64) -> f64 {
    if step > 0.0 {
        index * step
    } else {
        index / -step
    }
}

/// `lo` から `hi` まで（両端を含む）の刻みの倍数。
pub fn ticks_between(lo: f64, hi: f64, step: f64) -> Vec<f64> {
    let mut first = js::round(tick_index(step, lo));
    if tick_value(step, first) < lo {
        first += 1.0;
    }
    let mut last = js::round(tick_index(step, hi));
    if tick_value(step, last) > hi {
        last -= 1.0;
    }
    let mut out = Vec::new();
    let mut i = first;
    while i <= last && out.len() < 100 {
        out.push(tick_value(step, i));
        i += 1.0;
    }
    out
}
