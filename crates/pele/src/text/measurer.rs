//! 文字の幅（pele の `src/text/measurer.ts`）。canvas はないので、Arial の送り幅の表から見積もる
//! `metricsMeasurer`（node で pele を動かしたときと同じ）を既定にする。

use std::sync::OnceLock;

use super::metrics::{BOLD, REGULAR};

/// 文字の飾り（ビットの組み合わせ）。
pub mod style {
    pub const BOLD: u32 = 1;
    pub const ITALIC: u32 = 2;
    pub const MONO: u32 = 4;
    pub const SUB: u32 = 8;
    pub const SUP: u32 = 16;
    pub const UNDERLINE: u32 = 32;
}

/// CSS の `font-size="smaller"` のおおよその割る数。
const SMALLER: f64 = 1.2;

/// 文字の幅を測るもの。
pub trait TextMeasurer {
    fn width(&self, text: &str, size: f64, style: u32) -> f64;
}

fn unpack(packed: &str) -> [u16; 95] {
    let mut out = [0u16; 95];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u16::from_str_radix(&packed[i * 2..i * 2 + 2], 36).unwrap();
    }
    out
}

fn regular() -> &'static [u16; 95] {
    static T: OnceLock<[u16; 95]> = OnceLock::new();
    T.get_or_init(|| unpack(REGULAR))
}

fn bold() -> &'static [u16; 95] {
    static T: OnceLock<[u16; 95]> = OnceLock::new();
    T.get_or_init(|| unpack(BOLD))
}

fn is_wide(c: u32) -> bool {
    (0x1100..=0x115f).contains(&c)
        || (0x2e80..=0xa4cf).contains(&c)
        || (0xac00..=0xd7a3).contains(&c)
        || (0xf900..=0xfaff).contains(&c)
        || (0xfe30..=0xfe4f).contains(&c)
        || (0xff00..=0xff60).contains(&c)
        || (0xffe0..=0xffe6).contains(&c)
        || (0xd800..=0xdbff).contains(&c)
}

fn is_zero_width(c: u32) -> bool {
    c == 0xad
        || (0x300..=0x36f).contains(&c)
        || (0x200b..=0x200f).contains(&c)
        || (0xdc00..=0xdfff).contains(&c)
        || (0xfe00..=0xfe0f).contains(&c)
        || c == 0xfeff
}

/// Arial の送り幅の表から幅を見積もる。UTF-16 の符号単位ごとに数える（JS の `charCodeAt` と同じ）。
pub struct MetricsMeasurer;

impl TextMeasurer for MetricsMeasurer {
    fn width(&self, text: &str, size: f64, st: u32) -> f64 {
        let mono = st & style::MONO != 0;
        let eff = if st & (style::SUB | style::SUP) != 0 {
            size / SMALLER
        } else {
            size
        };
        let table = if st & style::BOLD != 0 {
            bold()
        } else {
            regular()
        };
        let mut units = 0.0f64;
        for c in text.encode_utf16() {
            let c = c as u32;
            if c < 32 {
                continue;
            }
            if c < 127 {
                units += if mono {
                    600.0
                } else {
                    table[(c - 32) as usize] as f64
                };
            } else if is_zero_width(c) {
                continue;
            } else if is_wide(c) {
                units += if mono { 1200.0 } else { 1000.0 };
            } else {
                units += if mono { 600.0 } else { 580.0 };
            }
        }
        (units * eff) / 1000.0
    }
}

pub static METRICS_MEASURER: MetricsMeasurer = MetricsMeasurer;

/// 既定の測り手。canvas がないので、いつも `metricsMeasurer`。
pub fn default_measurer() -> &'static dyn TextMeasurer {
    &METRICS_MEASURER
}
