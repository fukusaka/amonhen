//! 時の軸（pele の `src/diagrams/gantt/axis.ts`）。Mermaid の使う d3-time と d3-time-format（英語のロケール、
//! 地方時）に合わせる: `axisFormat` の指定子、d3 の `every` の決まりでの目盛りの間隔、自動の目盛りの選び方。

use super::dates::{MONTHS, WEEKDAYS};
use super::js_date as d;
use crate::js;

const SECOND: f64 = 1000.0;
const MINUTE: f64 = 60.0 * SECOND;
const HOUR: f64 = 60.0 * MINUTE;
const DAY: f64 = 24.0 * HOUR;
const WEEK: f64 = 7.0 * DAY;
const MONTH: f64 = 30.0 * DAY;
const YEAR: f64 = 365.0 * DAY;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TickUnit {
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

/// 間隔。`step` が 1 なら単位そのもの。
#[derive(Clone, Copy)]
struct Calendar {
    unit: TickUnit,
    week_start: f64,
    step: f64,
    /// 週を `step` ごとに数える起点（1970 年 1 月 1 日を含む週の始まり）。
    epoch: f64,
}

fn start_of_day(t: f64) -> f64 {
    d::set_hours(t, 0.0, 0.0, 0.0, 0.0)
}

fn base_floor(unit: TickUnit, week_start: f64, t: f64) -> f64 {
    match unit {
        TickUnit::Millisecond => t,
        TickUnit::Second => d::time_clip(t - d::get_milliseconds(t)),
        TickUnit::Minute => d::time_clip(t - d::get_milliseconds(t) - d::get_seconds(t) * SECOND),
        TickUnit::Hour => d::time_clip(
            t - d::get_milliseconds(t) - d::get_seconds(t) * SECOND - d::get_minutes(t) * MINUTE,
        ),
        TickUnit::Day => start_of_day(t),
        TickUnit::Week => {
            let back = (d::get_day(t) + 7.0 - week_start) % 7.0;
            start_of_day(d::set_date(t, d::get_date(t) - back))
        }
        TickUnit::Month => start_of_day(d::set_date(t, 1.0)),
        TickUnit::Year => start_of_day(d::set_month(t, 0.0, Some(1.0))),
    }
}

fn base_offset(unit: TickUnit, t: f64, step: f64) -> f64 {
    match unit {
        TickUnit::Millisecond => d::time_clip(t + step),
        TickUnit::Second => d::time_clip(t + step * SECOND),
        TickUnit::Minute => d::time_clip(t + step * MINUTE),
        TickUnit::Hour => d::time_clip(t + step * HOUR),
        TickUnit::Day => d::set_date(t, d::get_date(t) + step),
        TickUnit::Week => d::set_date(t, d::get_date(t) + step * 7.0),
        TickUnit::Month => d::set_month(t, d::get_month(t) + step, None),
        TickUnit::Year => d::set_full_year(t, d::get_full_year(t) + step),
    }
}

/// d3 が間隔と比べる値: 目盛りは、これが間隔の倍数になる所に来る。
fn field(unit: TickUnit, t: f64) -> f64 {
    match unit {
        TickUnit::Second => d::get_utc_seconds(t),
        TickUnit::Minute => d::get_minutes(t),
        TickUnit::Hour => d::get_hours(t),
        TickUnit::Day => d::get_date(t) - 1.0,
        TickUnit::Month => d::get_month(t),
        _ => f64::NAN,
    }
}

/// `field` のとる値の数。間を抜いた目盛りの離れうる幅を抑える。
fn cycle(unit: TickUnit) -> f64 {
    match unit {
        TickUnit::Second | TickUnit::Minute => 60.0,
        TickUnit::Hour => 24.0,
        TickUnit::Day => 31.0,
        TickUnit::Month => 12.0,
        _ => f64::INFINITY,
    }
}

fn unit_ms(unit: TickUnit) -> f64 {
    match unit {
        TickUnit::Millisecond => 1.0,
        TickUnit::Second => SECOND,
        TickUnit::Minute => MINUTE,
        TickUnit::Hour => HOUR,
        TickUnit::Day => DAY,
        TickUnit::Week => WEEK,
        TickUnit::Month => MONTH,
        TickUnit::Year => YEAR,
    }
}

/// 揃えた 2 つの日付の間の単位の数（UTC からのずれの変わりは除く）。
fn span(start: f64, end: f64, unit: f64) -> f64 {
    ((end - start - (d::get_timezone_offset(end) - d::get_timezone_offset(start)) * MINUTE) / unit)
        .floor()
}

fn modulo(a: f64, b: f64) -> f64 {
    ((a % b) + b) % b
}

impl Calendar {
    fn floor(&self, t: f64) -> f64 {
        let (unit, ws, step) = (self.unit, self.week_start, self.step);
        if step <= 1.0 {
            return base_floor(unit, ws, t);
        }
        match unit {
            TickUnit::Millisecond => d::time_clip((t / step).floor() * step),
            TickUnit::Year => {
                let t = d::set_full_year(t, (d::get_full_year(t) / step).floor() * step);
                base_floor(unit, ws, t)
            }
            TickUnit::Week => {
                let t = base_floor(unit, ws, t);
                base_offset(unit, t, -modulo(span(self.epoch, t, WEEK), step))
            }
            _ => {
                let mut t = base_floor(unit, ws, t);
                // 日付があふれると field が NaN になり、間隔に合うことはない。
                while t.is_finite() && field(unit, t) % step != 0.0 {
                    t = base_floor(unit, ws, d::time_clip(t - 1.0));
                }
                t
            }
        }
    }

    fn offset(&self, t: f64, n: f64) -> f64 {
        let (unit, step) = (self.unit, self.step);
        if step <= 1.0 {
            return base_offset(unit, t, n);
        }
        match unit {
            TickUnit::Millisecond => d::time_clip(t + n * step),
            TickUnit::Year => d::set_full_year(t, d::get_full_year(t) + n * step),
            TickUnit::Week => base_offset(unit, t, n * step),
            _ => {
                let mut t = t;
                let mut i = 0.0;
                while i < n && t.is_finite() {
                    loop {
                        t = base_offset(unit, t, 1.0);
                        if !(t.is_finite() && field(unit, t) % step != 0.0) {
                            break;
                        }
                    }
                    i += 1.0;
                }
                t
            }
        }
    }

    fn cycle(&self) -> f64 {
        cycle(self.unit)
    }

    fn ms(&self) -> f64 {
        unit_ms(self.unit)
    }
}

/// d3 の `unit.every(step)` の間隔。
fn every(unit: TickUnit, step: f64, week_start: f64) -> Calendar {
    let epoch = if unit == TickUnit::Week {
        base_floor(unit, week_start, 0.0)
    } else {
        0.0
    };
    Calendar {
        unit,
        week_start,
        step,
        epoch,
    }
}

/// start から stop まで（両端を含む）の目盛り。`limit` を超えるなら None。
fn range(calendar: &Calendar, start: f64, stop: f64, limit: usize) -> Option<Vec<f64>> {
    let mut t = d::time_clip(start - 1.0);
    t = calendar.floor(t);
    t = calendar.offset(t, 1.0);
    t = calendar.floor(t);
    let mut out = Vec::new();
    let mut previous = f64::NEG_INFINITY;
    while t <= stop && t > previous {
        if out.len() >= limit {
            return None;
        }
        previous = t;
        out.push(previous);
        t = calendar.offset(t, 1.0);
        t = calendar.floor(t);
    }
    Some(out)
}

fn tick_step(start: f64, stop: f64, count: f64) -> f64 {
    let step = (stop - start) / js::max(0.0, count);
    let power = step.log10().floor();
    let error = step / 10f64.powf(power);
    let factor = if error >= 50f64.sqrt() {
        10.0
    } else if error >= 10f64.sqrt() {
        5.0
    } else if error >= std::f64::consts::SQRT_2 {
        2.0
    } else {
        1.0
    };
    10f64.powf(power) * factor
}

const AUTO: [(TickUnit, f64, f64); 18] = [
    (TickUnit::Second, 1.0, SECOND),
    (TickUnit::Second, 5.0, 5.0 * SECOND),
    (TickUnit::Second, 15.0, 15.0 * SECOND),
    (TickUnit::Second, 30.0, 30.0 * SECOND),
    (TickUnit::Minute, 1.0, MINUTE),
    (TickUnit::Minute, 5.0, 5.0 * MINUTE),
    (TickUnit::Minute, 15.0, 15.0 * MINUTE),
    (TickUnit::Minute, 30.0, 30.0 * MINUTE),
    (TickUnit::Hour, 1.0, HOUR),
    (TickUnit::Hour, 3.0, 3.0 * HOUR),
    (TickUnit::Hour, 6.0, 6.0 * HOUR),
    (TickUnit::Hour, 12.0, 12.0 * HOUR),
    (TickUnit::Day, 1.0, DAY),
    (TickUnit::Day, 2.0, 2.0 * DAY),
    (TickUnit::Week, 1.0, WEEK),
    (TickUnit::Month, 1.0, MONTH),
    (TickUnit::Month, 3.0, 3.0 * MONTH),
    (TickUnit::Year, 1.0, YEAR),
];

/// d3 の時の目盛りと同じ選び方で、おおよそ `count` 個のきりのよい時刻の目盛り。
pub fn auto_ticks(start: f64, stop: f64, count: f64) -> Vec<f64> {
    let target = (stop - start) / count;
    let mut i = 0;
    while i < AUTO.len() && AUTO[i].2 <= target {
        i += 1;
    }
    let calendar = if i == AUTO.len() {
        every(
            TickUnit::Year,
            js::max(1.0, tick_step(start / YEAR, stop / YEAR, count)),
            0.0,
        )
    } else if i == 0 {
        every(
            TickUnit::Millisecond,
            js::max(tick_step(start, stop, count), 1.0),
            0.0,
        )
    } else {
        let k = if target / AUTO[i - 1].2 < AUTO[i].2 / target {
            i - 1
        } else {
            i
        };
        every(AUTO[k].0, AUTO[k].1, 0.0)
    };
    range(&calendar, start, stop, (4.0 * count + 8.0) as usize).unwrap_or_default()
}

const WEEK_STARTS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

/// `^([1-9]\d*)(millisecond|second|minute|hour|day|week|month)$`
fn interval(s: &str) -> Option<(f64, TickUnit)> {
    let b = s.as_bytes();
    if !b.first().is_some_and(|c| (b'1'..=b'9').contains(c)) {
        return None;
    }
    let n = b.iter().take_while(|c| c.is_ascii_digit()).count();
    let unit = match &s[n..] {
        "millisecond" => TickUnit::Millisecond,
        "second" => TickUnit::Second,
        "minute" => TickUnit::Minute,
        "hour" => TickUnit::Hour,
        "day" => TickUnit::Day,
        "week" => TickUnit::Week,
        "month" => TickUnit::Month,
        _ => return None,
    };
    Some((js::parse_int(&s[..n], 10), unit))
}

/// `1week` のような `tickInterval` の目盛り。そうでないか、`limit` より多くなるなら None。
pub fn interval_ticks(
    interval_text: &str,
    weekday: &str,
    start: f64,
    stop: f64,
    limit: f64,
) -> Option<Vec<f64>> {
    let (step, unit) = interval(interval_text)?;
    let week_start = WEEK_STARTS
        .iter()
        .position(|w| *w == weekday)
        .map_or(0.0, |k| k as f64);
    let calendar = every(unit, step, week_start);
    if (stop - start) / (js::min(step, calendar.cycle()) * calendar.ms()) > limit {
        return None;
    }
    range(&calendar, start, stop, limit as usize)
}

fn pad(value: f64, fill: &str, width: usize) -> String {
    let neg = value < 0.0;
    let digits = js::fmt(if neg { -value } else { value });
    let n = js::len16(&digits);
    let body = if n < width {
        fill.repeat(width - n) + &digits
    } else {
        digits
    };
    if neg { format!("-{body}") } else { body }
}

fn floored(t: f64, unit: TickUnit, week_start: f64) -> f64 {
    base_floor(unit, week_start, t)
}

/// 年の始まりの前の最後の `week_start` の曜日からの週の数（d3 の %U と %W）。
fn week_of_year(t: f64, week_start: f64) -> f64 {
    let year_start = floored(t, TickUnit::Year, 0.0);
    span(
        floored(d::time_clip(year_start - 1.0), TickUnit::Week, week_start),
        floored(t, TickUnit::Week, week_start),
        WEEK,
    )
}

/// 日付の ISO の週と年を決める木曜日。
fn iso_thursday(t: f64) -> f64 {
    let day = d::get_day(t);
    let late = day >= 4.0 || day == 0.0;
    let mut r = floored(
        if late { t } else { d::time_clip(t - 1.0) },
        TickUnit::Week,
        4.0,
    );
    if !late {
        r = base_offset(TickUnit::Week, r, 1.0);
        r = base_floor(TickUnit::Week, 4.0, r);
    }
    r
}

/// `%Y-%m-%d` のような d3 の時の書式で日付を書く。
pub fn time_format(specifier: &str, t: f64) -> String {
    let s: Vec<u16> = specifier.encode_utf16().collect();
    let mut out: Vec<u16> = Vec::new();
    let mut from = 0;
    let n = s.len();
    let mut i = 0;
    while i < n {
        if s[i] != 37 {
            i += 1;
            continue;
        }
        out.extend_from_slice(&s[from..i]);
        i += 1;
        let mut c: &[u16] = s.get(i..i + 1).unwrap_or(&[]);
        let fill = match c {
            [45] => Some(""),
            [95] => Some(" "),
            [48] => Some("0"),
            _ => None,
        };
        let fill = match fill {
            Some(f) => {
                i += 1;
                c = s.get(i..i + 1).unwrap_or(&[]);
                f
            }
            None => {
                if c == [101] {
                    " "
                } else {
                    "0"
                }
            }
        };
        match directive(c, t, fill) {
            Some(text) => out.extend(text.encode_utf16()),
            None => out.extend_from_slice(c),
        }
        from = i + 1;
        i += 1;
    }
    if from < n {
        out.extend_from_slice(&s[from..]);
    }
    js::from16(&out)
}

/// 指定子の文字。分からない文字は None（そのまま書く）。
fn directive(c: &[u16], t: f64, p: &str) -> Option<String> {
    let c = match c {
        [c] if *c < 128 => *c as u8,
        _ => return None,
    };
    let weekday = d::get_day(t) as usize;
    let month = d::get_month(t) as usize;
    Some(match c {
        b'a' => WEEKDAYS[weekday][..3].to_string(),
        b'A' => WEEKDAYS[weekday].to_string(),
        b'b' => MONTHS[month][..3].to_string(),
        b'B' => MONTHS[month].to_string(),
        b'c' => time_format("%x, %X", t),
        b'd' | b'e' => pad(d::get_date(t), p, 2),
        b'f' => pad(d::get_milliseconds(t), p, 3) + "000",
        b'g' => pad(d::get_full_year(iso_thursday(t)) % 100.0, p, 2),
        b'G' => pad(d::get_full_year(iso_thursday(t)) % 10000.0, p, 4),
        b'H' => pad(d::get_hours(t), p, 2),
        b'I' => {
            let h = d::get_hours(t) % 12.0;
            pad(if js::truthy(h) { h } else { 12.0 }, p, 2)
        }
        b'j' => pad(
            1.0 + span(
                floored(t, TickUnit::Year, 0.0),
                floored(t, TickUnit::Day, 0.0),
                DAY,
            ),
            p,
            3,
        ),
        b'L' => pad(d::get_milliseconds(t), p, 3),
        b'm' => pad(d::get_month(t) + 1.0, p, 2),
        b'M' => pad(d::get_minutes(t), p, 2),
        b'p' => (if d::get_hours(t) >= 12.0 { "PM" } else { "AM" }).to_string(),
        b'q' => js::fmt(1.0 + (d::get_month(t) / 3.0).floor()),
        b'Q' => js::fmt(t),
        b's' => js::fmt((t / 1000.0).floor()),
        b'S' => pad(d::get_seconds(t), p, 2),
        b'u' => {
            let day = d::get_day(t);
            js::fmt(if js::truthy(day) { day } else { 7.0 })
        }
        b'U' => pad(week_of_year(t, 0.0), p, 2),
        b'V' => {
            let thursday = iso_thursday(t);
            let year_start = floored(thursday, TickUnit::Year, 0.0);
            let first = floored(year_start, TickUnit::Week, 4.0);
            let extra = if d::get_day(year_start) == 4.0 {
                1.0
            } else {
                0.0
            };
            pad(span(first, thursday, WEEK) + extra, p, 2)
        }
        b'w' => js::fmt(d::get_day(t)),
        b'W' => pad(week_of_year(t, 1.0), p, 2),
        b'x' => time_format("%-m/%-d/%Y", t),
        b'X' => time_format("%-I:%M:%S %p", t),
        b'y' => pad(d::get_full_year(t) % 100.0, p, 2),
        b'Y' => pad(d::get_full_year(t) % 10000.0, p, 4),
        b'Z' => {
            let z = d::get_timezone_offset(t);
            let abs = z.abs();
            format!(
                "{}{}{}",
                if z > 0.0 { '-' } else { '+' },
                pad((abs / 60.0).floor(), "0", 2),
                pad(abs % 60.0, "0", 2)
            )
        }
        b'%' => "%".to_string(),
        _ => return None,
    })
}
