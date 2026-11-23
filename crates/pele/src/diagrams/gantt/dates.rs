//! Mermaid の gantt が使う dayjs 1.11 の部分（pele の `src/diagrams/gantt/dates.ts`）: 書式に合わせた厳しい読み、
//! 書式での書き出し、暦の足し算。読み込むプラグイン（custom parse format、advanced format、ISO week）と英語の
//! ロケールのふるまいで、地方時で数える（地方時は `js_date` のとおり UTC）。日付は、読んだものを書き出して元の
//! 文字列に戻るときだけ受けるので、癖も残す。
//!
//! 日付は時刻の値（`f64`、不正なら NaN）。正規表現は手で書いた走査に置き換えた。文字列は UTF-16 で扱う。

use super::js_date as d;
use crate::js;
use crate::util::chars::is_space;

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

const DAY: f64 = 86_400_000.0;
const WEEK: f64 = 7.0 * DAY;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Ms,
    S,
    M,
    H,
    D,
    W,
    Month,
    Y,
}

pub fn is_valid(date: f64) -> bool {
    !date.is_nan()
}

/// `String(value)` を `length` 桁まで頭に 0 を足す（負の数は `-` の前に足す）。
fn pad(value: f64, length: usize) -> String {
    let s = js::fmt(value);
    let n = js::len16(&s);
    if n >= length {
        s
    } else {
        "0".repeat(length - n) + &s
    }
}

fn ordinal(n: f64) -> String {
    let s = ["th", "st", "nd", "rd"];
    let v = n % 100.0;
    let pick = |i: f64| -> Option<&str> {
        if (0.0..4.0).contains(&i) && i.fract() == 0.0 {
            Some(s[i as usize])
        } else {
            None
        }
    };
    let suffix = pick((v - 20.0) % 10.0).or(pick(v)).unwrap_or(s[0]);
    js::fmt(n) + suffix
}

fn utc_offset(date: f64) -> f64 {
    let offset = -js::round(d::get_timezone_offset(date) / 15.0) * 15.0;
    if offset == 0.0 { 0.0 } else { offset }
}

fn zone(date: f64) -> String {
    let offset = utc_offset(date);
    let minutes = offset.abs();
    format!(
        "{}{}:{}",
        if offset >= 0.0 { '+' } else { '-' },
        pad((minutes / 60.0).floor(), 2),
        pad(minutes % 60.0, 2)
    )
}

pub fn iso_weekday(date: f64) -> f64 {
    let day = d::get_day(date);
    if js::truthy(day) { day } else { 7.0 }
}

pub fn add_days(date: f64, days: f64) -> f64 {
    d::set_date(date, d::get_date(date) + days)
}

fn days_in_month(date: f64) -> f64 {
    let last = d::local(
        d::get_full_year(date),
        d::get_month(date) + 1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    );
    let last = d::set_hours(last, 23.0, 59.0, 59.0, 999.0);
    d::get_date(last)
}

/// 月か年を変える。日はそのまま、新しい月が短ければその月の終わりに。
fn set_calendar(date: f64, apply: impl FnOnce(f64) -> f64) -> f64 {
    let next = d::set_date(date, 1.0);
    let next = apply(next);
    d::set_date(next, js::min(d::get_date(date), days_in_month(next)))
}

pub fn add(date: f64, amount: f64, unit: Unit) -> f64 {
    match unit {
        Unit::Month => set_calendar(date, |n| d::set_month(n, d::get_month(date) + amount, None)),
        Unit::Y => set_calendar(date, |n| {
            d::set_full_year(n, d::get_full_year(date) + amount)
        }),
        Unit::D => add_days(date, js::round(amount)),
        Unit::W => add_days(date, js::round(7.0 * amount)),
        Unit::H => d::time_clip(date + amount * 3_600_000.0),
        Unit::M => d::time_clip(date + amount * 60_000.0),
        Unit::S => d::time_clip(date + amount * 1000.0),
        Unit::Ms => d::time_clip(date + amount),
    }
}

fn week_thursday(date: f64) -> f64 {
    add_days(date, 4.0 - iso_weekday(date))
}

fn iso_week(date: f64) -> f64 {
    let thursday = week_thursday(date);
    let jan1 = d::local(d::get_full_year(thursday), 0.0, 1.0, 0.0, 0.0, 0.0, 0.0);
    let weekday = iso_weekday(jan1);
    let first = add_days(jan1, 4.0 - weekday + if weekday > 4.0 { 7.0 } else { 0.0 });
    let zone_delta = (utc_offset(first) - utc_offset(thursday)) * 60000.0;
    let weeks = (thursday - first - zone_delta) / WEEK;
    let whole = if weeks < 0.0 {
        let c = weeks.ceil();
        if js::truthy(c) { c } else { 0.0 }
    } else {
        weeks.floor()
    };
    whole + 1.0
}

/// `\[([^\]]+)]` が `at` に合えば、終わりの位置。`closes[k]` は `k` から先で最初の `]` の位置（なければ長さ）。
fn bracket_at(s: &[u16], closes: &[usize], at: usize) -> Option<usize> {
    if s.get(at) != Some(&(b'[' as u16)) {
        return None;
    }
    let k = closes.get(at + 1).copied().unwrap_or(s.len());
    if k < s.len() && k > at + 1 {
        Some(k + 1)
    } else {
        None
    }
}

/// 各位置から先で最初の `]` の位置（なければ長さ）。閉じない `[` の続く書式でも、位置ごとに終わりまで探さない。
fn closing_brackets(s: &[u16]) -> Vec<usize> {
    let mut closes = vec![s.len(); s.len()];
    let mut next = s.len();
    for k in (0..s.len()).rev() {
        if s[k] == b']' as u16 {
            next = k;
        }
        closes[k] = next;
    }
    closes
}

fn starts(s: &[u16], at: usize, word: &str) -> bool {
    let w = word.as_bytes();
    s.len() >= at + w.len()
        && s[at..at + w.len()]
            .iter()
            .zip(w)
            .all(|(&c, &b)| c == b as u16)
}

/// `at` から同じ文字が続く数（`max` まで）。
fn run(s: &[u16], at: usize, c: u8, max: usize) -> usize {
    let mut n = 0;
    while n < max && s.get(at + n) == Some(&(c as u16)) {
        n += 1;
    }
    n
}

/// RE_ADVANCED の字句: `\[([^\]]+)]|Q|wo|ww|w|WW|W|zzz|z|gggg|GGGG|Do|X|x|k{1,2}|S`。
fn advanced_at(s: &[u16], closes: &[usize], at: usize) -> Option<usize> {
    if let Some(e) = bracket_at(s, closes, at) {
        return Some(e);
    }
    for word in [
        "Q", "wo", "ww", "w", "WW", "W", "zzz", "z", "gggg", "GGGG", "Do", "X", "x",
    ] {
        if starts(s, at, word) {
            return Some(at + word.len());
        }
    }
    let k = run(s, at, b'k', 2);
    if k > 0 {
        return Some(at + k);
    }
    if starts(s, at, "S") {
        return Some(at + 1);
    }
    None
}

/// RE_FORMAT の字句: `\[([^\]]+)]|YYYY|YY|M{1,4}|D{1,2}|d{1,4}|H{1,2}|h{1,2}|a|A|m{1,2}|s{1,2}|Z{1,2}|SSS`。
fn format_at(s: &[u16], closes: &[usize], at: usize) -> Option<usize> {
    if let Some(e) = bracket_at(s, closes, at) {
        return Some(e);
    }
    if starts(s, at, "YYYY") {
        return Some(at + 4);
    }
    if starts(s, at, "YY") {
        return Some(at + 2);
    }
    for (c, max) in [(b'M', 4), (b'D', 2), (b'd', 4), (b'H', 2), (b'h', 2)] {
        let k = run(s, at, c, max);
        if k > 0 {
            return Some(at + k);
        }
    }
    if starts(s, at, "a") || starts(s, at, "A") {
        return Some(at + 1);
    }
    for (c, max) in [(b'm', 2), (b's', 2), (b'Z', 2)] {
        let k = run(s, at, c, max);
        if k > 0 {
            return Some(at + k);
        }
    }
    if starts(s, at, "SSS") {
        return Some(at + 3);
    }
    None
}

/// 全体に正規表現を当てて置き換える（`String.prototype.replace` の `g`）。
/// `find` が合う所を `with` が書いたものに置き換える（`with` は合った文字列と書く先を受ける）。
fn replace_all(
    s: &[u16],
    find: impl Fn(&[u16], &[usize], usize) -> Option<usize>,
    mut with: impl FnMut(&[u16], &mut Vec<u16>),
) -> Vec<u16> {
    let closes = closing_brackets(s);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match find(s, &closes, i) {
            Some(e) => {
                with(&s[i..e], &mut out);
                i = e;
            }
            None => {
                out.push(s[i]);
                i += 1;
            }
        }
    }
    out
}

const DEFAULT_FORMAT: &str = "YYYY-MM-DDTHH:mm:ssZ";

/// 書式の字句（名前は字句そのまま。小文字は `Lower` を付ける）。
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    // 1 度目（advanced format）。`Do` は角括弧の中身だけ。
    Q,
    Do,
    GGGG,
    W,
    WW,
    K,
    KK,
    X,
    LowerX,
    // 2 度目（dayjs）。
    YY,
    YYYY,
    M,
    MM,
    MMM,
    MMMM,
    D,
    DD,
    LowerD,
    LowerDD,
    LowerDDD,
    LowerDDDD,
    H,
    HH,
    LowerH,
    LowerHH,
    LowerA,
    A,
    LowerM,
    LowerMM,
    S,
    SS,
    SSS,
    Z,
    ZZ,
}

const ADVANCED: [(&str, Tok); 9] = [
    ("Q", Tok::Q),
    ("Do", Tok::Do),
    ("GGGG", Tok::GGGG),
    ("W", Tok::W),
    ("WW", Tok::WW),
    ("k", Tok::K),
    ("kk", Tok::KK),
    ("X", Tok::X),
    ("x", Tok::LowerX),
];

const TOKENS: [(&str, Tok); 25] = [
    ("YY", Tok::YY),
    ("YYYY", Tok::YYYY),
    ("M", Tok::M),
    ("MM", Tok::MM),
    ("MMM", Tok::MMM),
    ("MMMM", Tok::MMMM),
    ("D", Tok::D),
    ("DD", Tok::DD),
    ("d", Tok::LowerD),
    ("dd", Tok::LowerDD),
    ("ddd", Tok::LowerDDD),
    ("dddd", Tok::LowerDDDD),
    ("H", Tok::H),
    ("HH", Tok::HH),
    ("h", Tok::LowerH),
    ("hh", Tok::LowerHH),
    ("a", Tok::LowerA),
    ("A", Tok::A),
    ("m", Tok::LowerM),
    ("mm", Tok::LowerMM),
    ("s", Tok::S),
    ("ss", Tok::SS),
    ("SSS", Tok::SSS),
    ("Z", Tok::Z),
    ("ZZ", Tok::ZZ),
];

fn named(list: &[(&str, Tok)], m: &[u16]) -> Option<Tok> {
    list.iter()
        .find(|(w, _)| w.len() == m.len() && w.bytes().zip(m).all(|(b, &c)| c == b as u16))
        .map(|&(_, t)| t)
}

/// 書式を 2 度の置き換え（advanced format の字句、dayjs の字句）で読んだ一片。
#[derive(Clone, Debug)]
enum Piece {
    Text(String),
    Tok(Tok),
}

/// 書式を一片ずつに分ける（週の番号と時間帯の名前の字句があれば None）。1 度目に日付から書くのは数か、角括弧で
/// 囲んだ序数なので、2 度目の字句の切れ目は日付によらない: 数は字句にも角括弧にもならない 1 文字として、2 度目の
/// 置き換えを書式だけで済ませておく（序数は必ず角括弧の中に入る）。日ごとに書くとき、書式を読み直さない。
fn compile_format(template: &str) -> Option<Vec<Piece>> {
    const HOLE: u16 = 1;
    let src: Vec<u16> = template.encode_utf16().collect();
    let closes = closing_brackets(&src);
    // 1 度目: 文字と、日付から書く字句。
    let mut flat: Vec<u16> = Vec::with_capacity(src.len());
    let mut holes: Vec<Option<Tok>> = Vec::with_capacity(src.len());
    let mut unsupported = false;
    let mut i = 0;
    while i < src.len() {
        let Some(e) = advanced_at(&src, &closes, i) else {
            flat.push(src[i]);
            holes.push(None);
            i += 1;
            continue;
        };
        let m = &src[i..e];
        i = e;
        match named(&ADVANCED, m) {
            Some(Tok::Do) => {
                flat.extend([b'[' as u16, HOLE, b']' as u16]);
                holes.extend([None, Some(Tok::Do), None]);
            }
            Some(t) => {
                flat.push(HOLE);
                holes.push(Some(t));
            }
            None => {
                let words = ["gggg", "wo", "w", "ww", "z", "zzz"];
                if m[0] != b'[' as u16
                    && words
                        .iter()
                        .any(|w| w.bytes().map(u16::from).eq(m.iter().copied()))
                {
                    unsupported = true;
                }
                flat.extend_from_slice(m);
                holes.extend(m.iter().map(|_| None));
            }
        }
    }
    if unsupported {
        return None;
    }
    // 2 度目: dayjs の字句と、角括弧の中身。
    let closes = closing_brackets(&flat);
    let mut pieces: Vec<Piece> = Vec::new();
    let mut text: Vec<u16> = Vec::new();
    let flush = |pieces: &mut Vec<Piece>, text: &mut Vec<u16>| {
        if !text.is_empty() {
            pieces.push(Piece::Text(js::from16(text)));
            text.clear();
        }
    };
    let put = |pieces: &mut Vec<Piece>, text: &mut Vec<u16>, k: usize| match holes[k] {
        Some(t) => {
            flush(pieces, text);
            pieces.push(Piece::Tok(t));
        }
        None => text.push(flat[k]),
    };
    let mut i = 0;
    while i < flat.len() {
        match format_at(&flat, &closes, i) {
            Some(e) if e - i > 1 && flat[i] == b'[' as u16 => {
                for k in i + 1..e - 1 {
                    put(&mut pieces, &mut text, k);
                }
                i = e;
            }
            Some(e) => {
                flush(&mut pieces, &mut text);
                pieces.push(Piece::Tok(named(&TOKENS, &flat[i..e]).unwrap_or(Tok::ZZ)));
                i = e;
            }
            None => {
                put(&mut pieces, &mut text, i);
                i += 1;
            }
        }
    }
    flush(&mut pieces, &mut text);
    Some(pieces)
}

type Compiled = std::rc::Rc<Option<Vec<Piece>>>;

thread_local! {
    /// 読んだ書式（最近のいくつか）。
    static FORMATS: std::cell::RefCell<Vec<(String, Compiled)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn compiled_format(template: &str) -> Compiled {
    FORMATS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(k) = cache.iter().position(|(t, _)| t == template) {
            return cache[k].1.clone();
        }
        let pieces = std::rc::Rc::new(compile_format(template));
        if cache.len() >= 4 {
            cache.remove(0);
        }
        cache.push((template.to_string(), pieces.clone()));
        pieces
    })
}

/// 週の番号と時間帯の名前の字句は None（Mermaid の読み込まないプラグインが要り、そこでは失敗する）。
pub fn format(date: f64, template: &str) -> Option<String> {
    if !is_valid(date) {
        return Some("Invalid Date".into());
    }
    let str = if template.is_empty() {
        DEFAULT_FORMAT
    } else {
        template
    };
    if str == "YYYY-MM-DD" {
        return Some(iso_date(date));
    }
    let pieces = compiled_format(str);
    let pieces = pieces.as_ref().as_ref()?;
    let hours = d::get_hours(date);
    let (year, month, day) = d::ymd(date);
    let weekday = || d::get_day(date);
    let mut out = String::with_capacity(str.len() + 16);
    for piece in pieces {
        let tok = match piece {
            Piece::Text(t) => {
                out.push_str(t);
                continue;
            }
            Piece::Tok(tok) => *tok,
        };
        let fixed = match tok {
            Tok::MMM => Some(&MONTHS[month as usize][..3]),
            Tok::MMMM => Some(MONTHS[month as usize]),
            Tok::LowerDD => Some(&WEEKDAYS[weekday() as usize][..2]),
            Tok::LowerDDD => Some(&WEEKDAYS[weekday() as usize][..3]),
            Tok::LowerDDDD => Some(WEEKDAYS[weekday() as usize]),
            Tok::LowerA => Some(if hours < 12.0 { "am" } else { "pm" }),
            Tok::A => Some(if hours < 12.0 { "AM" } else { "PM" }),
            _ => None,
        };
        if let Some(fixed) = fixed {
            out.push_str(fixed);
            continue;
        }
        let text = match tok {
            Tok::Q => js::fmt(((month + 1.0) / 3.0).ceil()),
            Tok::Do => ordinal(day),
            Tok::GGGG => js::fmt(d::get_full_year(week_thursday(date))),
            Tok::W => js::fmt(iso_week(date)),
            Tok::WW => pad(iso_week(date), 2),
            Tok::K => js::fmt(hours_or_24(date)),
            Tok::KK => pad(hours_or_24(date), 2),
            Tok::X => js::fmt((date / 1000.0).floor()),
            Tok::LowerX => js::fmt(date),
            Tok::YY => {
                let y: Vec<u16> = js::fmt(year).encode_utf16().collect();
                js::from16(&y[y.len().saturating_sub(2)..])
            }
            Tok::YYYY => pad(year, 4),
            Tok::M => js::fmt(month + 1.0),
            Tok::MM => pad(month + 1.0, 2),
            Tok::D => js::fmt(day),
            Tok::DD => pad(day, 2),
            Tok::LowerD => js::fmt(weekday()),
            Tok::H => js::fmt(hours),
            Tok::HH => pad(hours, 2),
            Tok::LowerH => js::fmt(hours12(hours)),
            Tok::LowerHH => pad(hours12(hours), 2),
            Tok::LowerM => js::fmt(d::get_minutes(date)),
            Tok::LowerMM => pad(d::get_minutes(date), 2),
            Tok::S => js::fmt(d::get_seconds(date)),
            Tok::SS => pad(d::get_seconds(date), 2),
            Tok::SSS => pad(d::get_milliseconds(date), 3),
            Tok::Z => zone(date),
            _ => zone(date).replace(':', ""),
        };
        out.push_str(&text);
    }
    Some(out)
}

fn hours_or_24(date: f64) -> f64 {
    let h = d::get_hours(date);
    if js::truthy(h) { h } else { 24.0 }
}

fn hours12(hours: f64) -> f64 {
    let h = hours % 12.0;
    if js::truthy(h) { h } else { 12.0 }
}

pub fn iso_date(date: f64) -> String {
    let (year, month, day) = d::ymd(date);
    let month = month + 1.0;
    if !date.is_nan() {
        // 日付があれば年・月・日は整数（`String(n)` は整数の書き方）。
        use std::fmt::Write;
        let mut out = String::with_capacity(10);
        if (1000.0..=9999.0).contains(&year) {
            let _ = write!(out, "{}", year as i64);
        } else {
            out.push_str(&pad(year, 4));
        }
        let _ = write!(
            out,
            "{}{}{}{}",
            if month < 10.0 { "-0" } else { "-" },
            month as i64,
            if day < 10.0 { "-0" } else { "-" },
            day as i64
        );
        return out;
    }
    let y = if (1000.0..=9999.0).contains(&year) {
        js::fmt(year)
    } else {
        pad(year, 4)
    };
    format!(
        "{y}{}{}{}{}",
        if month < 10.0 { "-0" } else { "-" },
        js::fmt(month),
        if day < 10.0 { "-0" } else { "-" },
        js::fmt(day)
    )
}

#[derive(Default)]
struct Fields {
    year: Option<f64>,
    month: Option<f64>,
    day: Option<f64>,
    day_given: bool,
    hours: Option<f64>,
    minutes: Option<f64>,
    seconds: Option<f64>,
    milliseconds: Option<f64>,
    week: Option<f64>,
    offset: Option<f64>,
    afternoon: Option<bool>,
}

/// 読み手が値を探す正規表現。
#[derive(Clone, Copy)]
enum Re {
    /// `\d` を n 個。
    Digits(usize),
    /// `\d\d?`
    OneToTwo,
    /// `[+-]?\d+`
    Signed,
    /// `[+-]\d\d:?(\d\d)?|Z`
    Offset,
    /// `\d*[^-_:/,()\s\d]+`
    Word,
}

fn is_d(c: u16) -> bool {
    (48..=57).contains(&c)
}

fn digits(s: &[u16], at: usize) -> usize {
    s[at..].iter().take_while(|&&c| is_d(c)).count()
}

impl Re {
    /// `at` で合えば終わり。
    fn at(self, s: &[u16], at: usize) -> Option<usize> {
        match self {
            Re::Digits(n) => (digits(s, at) >= n).then_some(at + n),
            Re::OneToTwo => match digits(s, at) {
                0 => None,
                1 => Some(at + 1),
                _ => Some(at + 2),
            },
            Re::Signed => {
                let from = if matches!(s.get(at), Some(&43) | Some(&45)) {
                    at + 1
                } else {
                    at
                };
                let n = digits(s, from);
                (n > 0).then_some(from + n)
            }
            Re::Offset => {
                if matches!(s.get(at), Some(&43) | Some(&45)) && digits(s, at + 1) >= 2 {
                    let mut e = at + 3;
                    if s.get(e) == Some(&58) {
                        e += 1;
                    }
                    if digits(s, e) >= 2 {
                        e += 2;
                    }
                    return Some(e);
                }
                (s.get(at) == Some(&90)).then_some(at + 1)
            }
            Re::Word => {
                let n = digits(s, at);
                let word = |c: u16| {
                    !is_d(c)
                        && !is_space(c as u32)
                        && !matches!(c, 45 | 95 | 58 | 47 | 44 | 40 | 41)
                };
                let k = s[at + n..].iter().take_while(|&&c| word(c)).count();
                (k > 0).then_some(at + n + k)
            }
        }
    }

    /// `re.exec(s)`: 最初に合う所。
    fn exec(self, s: &[u16]) -> Option<(usize, usize)> {
        (0..s.len()).find_map(|i| self.at(s, i).map(|e| (i, e)))
    }
}

#[derive(Clone, Copy)]
enum Read {
    Afternoon(bool),
    Quarter,
    Millis(f64),
    Seconds,
    Minutes,
    Hours,
    Day,
    Ordinal,
    Week,
    Month,
    MonthName(bool),
    Year,
    ShortYear,
    Offset,
}

fn reader(token: &str) -> Option<(Re, Read)> {
    use Re::*;
    use Read as R;
    Some(match token {
        "A" => (Word, R::Afternoon(true)),
        "a" => (Word, R::Afternoon(false)),
        "Q" => (Digits(1), R::Quarter),
        "S" => (Digits(1), R::Millis(100.0)),
        "SS" => (Digits(2), R::Millis(10.0)),
        "SSS" => (Digits(3), R::Millis(1.0)),
        "s" | "ss" => (OneToTwo, R::Seconds),
        "m" | "mm" => (OneToTwo, R::Minutes),
        "H" | "h" | "HH" | "hh" => (OneToTwo, R::Hours),
        "D" => (OneToTwo, R::Day),
        "DD" => (Digits(2), R::Day),
        "Do" => (Word, R::Ordinal),
        "w" => (OneToTwo, R::Week),
        "ww" => (Digits(2), R::Week),
        "M" => (OneToTwo, R::Month),
        "MM" => (Digits(2), R::Month),
        "MMM" => (Word, R::MonthName(true)),
        "MMMM" => (Word, R::MonthName(false)),
        "Y" => (Signed, R::Year),
        "YY" => (Digits(2), R::ShortYear),
        "YYYY" => (Digits(4), R::Year),
        "Z" | "ZZ" => (Offset, R::Offset),
        _ => return None,
    })
}

fn apply(read: Read, f: &mut Fields, input: &str) -> bool {
    let n = || js::number(input);
    match read {
        Read::Afternoon(upper) => f.afternoon = Some(input == if upper { "PM" } else { "pm" }),
        Read::Quarter => f.month = Some((n() - 1.0) * 3.0 + 1.0),
        Read::Millis(k) => f.milliseconds = Some(n() * k),
        Read::Seconds => f.seconds = Some(n()),
        Read::Minutes => f.minutes = Some(n()),
        Read::Hours => f.hours = Some(n()),
        Read::Day => f.day = Some(n()),
        Read::Week => f.week = Some(n()),
        Read::Month => f.month = Some(n()),
        Read::Year => f.year = Some(n()),
        Read::ShortYear => {
            let v = n();
            f.year = Some(v + if v > 68.0 { 1900.0 } else { 2000.0 });
        }
        Read::Ordinal => {
            let s: Vec<u16> = input.encode_utf16().collect();
            let Some(start) = s.iter().position(|&c| is_d(c)) else {
                return false;
            };
            let k = digits(&s, start);
            f.day = Some(js::number(&js::from16(&s[start..start + k])));
            f.day_given = true;
            for i in 1..=31 {
                if ordinal(i as f64) == input {
                    f.day = Some(i as f64);
                }
            }
        }
        Read::MonthName(short) => {
            let index = MONTHS
                .iter()
                .position(|m| (if short { &m[..3] } else { m }) == input)
                .map_or(0, |i| i + 1);
            if index < 1 {
                return false;
            }
            f.month = Some(if index % 12 != 0 { index % 12 } else { index } as f64);
        }
        Read::Offset => {
            if input == "Z" {
                f.offset = Some(0.0);
                return true;
            }
            // input.match(/([+-]|\d\d)/g)
            let s: Vec<u16> = input.encode_utf16().collect();
            let mut parts: Vec<String> = Vec::new();
            let mut i = 0;
            while i < s.len() {
                if s[i] == 43 || s[i] == 45 {
                    parts.push(js::from16(&s[i..i + 1]));
                    i += 1;
                } else if i + 1 < s.len() && is_d(s[i]) && is_d(s[i + 1]) {
                    parts.push(js::from16(&s[i..i + 2]));
                    i += 2;
                } else {
                    i += 1;
                }
            }
            let part = |k: usize| parts.get(k).map_or(f64::NAN, |p| js::number(p));
            let second = part(2);
            let minutes = part(1) * 60.0 + if js::truthy(second) { second } else { 0.0 };
            f.offset = Some(if minutes == 0.0 {
                0.0
            } else if parts[0] == "+" {
                -minutes
            } else {
                minutes
            });
        }
    }
    true
}

enum Step {
    Text(usize),
    Read(Re, Read),
}

/// RE_TOKENS の 3 つ目の組: `A|a|Q|YYYY|YY?|ww?|MM?M?M?|Do|DD?|hh?|HH?|mm?|ss?|S{1,3}|z|ZZ?`。
fn token_at(s: &[u16], at: usize) -> Option<usize> {
    for word in ["A", "a", "Q", "YYYY"] {
        if starts(s, at, word) {
            return Some(at + word.len());
        }
    }
    let runs: [(u8, usize); 3] = [(b'Y', 2), (b'w', 2), (b'M', 4)];
    for (c, max) in runs {
        let k = run(s, at, c, max);
        if k > 0 {
            return Some(at + k);
        }
    }
    if starts(s, at, "Do") {
        return Some(at + 2);
    }
    for (c, max) in [
        (b'D', 2),
        (b'h', 2),
        (b'H', 2),
        (b'm', 2),
        (b's', 2),
        (b'S', 3),
        (b'z', 1),
        (b'Z', 2),
    ] {
        let k = run(s, at, c, max);
        if k > 0 {
            return Some(at + k);
        }
    }
    None
}

fn is_separator(c: u16) -> bool {
    matches!(c, 45 | 95 | 58 | 47 | 46 | 44 | 40 | 41) || is_space(c as u32)
}

/// `template.match(RE_TOKENS)`。
fn tokens(s: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        // (\[[^[]*\])
        if s[i] == b'[' as u16 {
            let mut stop = i + 1;
            while stop < s.len() && s[stop] != b'[' as u16 {
                stop += 1;
            }
            if let Some(k) = (i + 1..stop).rev().find(|&k| s[k] == b']' as u16) {
                out.push(s[i..k + 1].to_vec());
                i = k + 1;
                continue;
            }
        }
        let n = s[i..].iter().take_while(|&&c| is_separator(c)).count();
        if n > 0 {
            out.push(s[i..i + n].to_vec());
            i += n;
            continue;
        }
        if let Some(e) = token_at(s, i) {
            out.push(s[i..e].to_vec());
            i = e;
            continue;
        }
        i += 1;
    }
    out
}

fn compile(template: &str) -> Option<Vec<Step>> {
    let src: Vec<u16> = template.encode_utf16().collect();
    // dayjs は地域の字句を英語のロケールにない表で広げようとして、失敗する。
    let bare = replace_all(&src, bracket_at, |_, _| {});
    if bare.iter().any(|&c| c == b'L' as u16 || c == b'l' as u16) {
        return None;
    }
    let found = tokens(&src);
    if found.is_empty() {
        return None;
    }
    Some(
        found
            .into_iter()
            .map(|t| match reader(&js::from16(&t)) {
                Some((re, read)) => Step::Read(re, read),
                None => {
                    let mut t = &t[..];
                    if t.first() == Some(&(b'[' as u16)) {
                        t = &t[1..];
                    }
                    if t.last() == Some(&(b']' as u16)) {
                        t = &t[..t.len() - 1];
                    }
                    Step::Text(t.len())
                }
            })
            .collect(),
    )
}

fn or_zero(v: Option<f64>) -> f64 {
    match v {
        Some(x) if js::truthy(x) => x,
        _ => 0.0,
    }
}

fn loose(input: &str, template: &str, now: f64) -> Option<f64> {
    if template == "x" || template == "X" {
        let k = if template == "X" { 1000.0 } else { 1.0 };
        return Some(d::time_clip(k * js::number(input)));
    }
    let steps = compile(template)?;
    let mut f = Fields::default();
    let mut rest: Vec<u16> = input.encode_utf16().collect();
    let mut start = 0usize;
    for step in &steps {
        match *step {
            Step::Text(n) => start += n,
            Step::Read(re, read) => {
                let from = start.min(rest.len());
                let (a, b) = re.exec(&rest[from..])?;
                let found = rest[from + a..from + b].to_vec();
                if !apply(read, &mut f, &js::from16(&found)) {
                    return None;
                }
                // dayjs は合った文字列が最初に現れる所を、どこであれ消し、`start` は動かさない。
                if let Some(k) =
                    (0..=rest.len() - found.len()).find(|&k| rest[k..k + found.len()] == found[..])
                {
                    rest.drain(k..k + found.len());
                }
            }
        }
    }
    if let (Some(afternoon), Some(hours)) = (f.afternoon, f.hours) {
        if afternoon {
            if hours < 12.0 {
                f.hours = Some(hours + 12.0);
            }
        } else if hours == 12.0 {
            f.hours = Some(0.0);
        }
    }
    let truthy = |v: Option<f64>| v.is_some_and(js::truthy);
    let day = if f.day_given || truthy(f.day) {
        f.day.unwrap_or(f64::NAN)
    } else if !truthy(f.year) && !truthy(f.month) {
        d::get_date(now)
    } else {
        1.0
    };
    let year = if truthy(f.year) {
        f.year.unwrap()
    } else {
        d::get_full_year(now)
    };
    let mut month = 0.0;
    if !(truthy(f.year) && !truthy(f.month)) {
        month = match f.month {
            Some(m) if m > 0.0 => m - 1.0,
            _ => d::get_month(now),
        };
    }
    let hours = or_zero(f.hours);
    let minutes = or_zero(f.minutes);
    let seconds = or_zero(f.seconds);
    let ms = or_zero(f.milliseconds);
    if let Some(offset) = f.offset {
        let t = d::utc(
            year,
            month,
            day,
            hours,
            minutes,
            seconds,
            ms + offset * 60000.0,
        );
        return Some(d::time_clip(t));
    }
    // 週の番号を当てるには、Mermaid の読み込まないプラグインが要る。
    if truthy(f.week) {
        return None;
    }
    Some(d::local(year, month, day, hours, minutes, seconds, ms))
}

/// `dayjs(input, format, true)` と同じく読む。書式にない値は `now` から取る。
pub fn parse_strict(input: &str, template: &str, now: f64) -> Option<f64> {
    let date = loose(input, template, now)?;
    (is_valid(date) && format(date, template).as_deref() == Some(input)).then_some(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_what_formats_back() {
        let now = 0.0;
        let t = parse_strict("2014-01-06", "YYYY-MM-DD", now).unwrap();
        assert_eq!(iso_date(t), "2014-01-06");
        assert!(parse_strict("2014-1-6", "YYYY-MM-DD", now).is_none());
        let t = parse_strict("06 Jan 2014", "DD MMM YYYY", now).unwrap();
        assert_eq!(format(t, "Do MMMM YY").unwrap(), "6th January 14");
        assert_eq!(format(t, "wo"), None);
    }

    #[test]
    fn adds_months_like_dayjs() {
        let t = parse_strict("2014-01-31", "YYYY-MM-DD", 0.0).unwrap();
        assert_eq!(iso_date(add(t, 1.0, Unit::Month)), "2014-02-28");
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn format_takes_linear_time() {
        // 閉じない `[` は位置ごとに終わりまで `]` を探していた（10 万字で 50 億回）。
        let unclosed = "[".repeat(100_000);
        assert_eq!(format(0.0, &unclosed).as_deref(), Some(unclosed.as_str()));
        let mixed = "[a".repeat(50_000);
        assert_eq!(format(0.0, &mixed), Some("[am".repeat(50_000)));
    }

    #[test]
    fn format_reads_both_passes() {
        let date = 946_771_200_000.0; // 2000-01-02T00:00:00Z（日曜）
        assert_eq!(
            format(date, "[Q] Q Do [Do] [abc Do").as_deref(),
            Some("Q 1 2nd Do abc [2nd")
        );
        assert_eq!(
            format(date, "dddd, MMMM Do YYYY h:mm a").as_deref(),
            Some("Sunday, January 2nd 2000 12:00 am")
        );
        assert_eq!(format(date, "YYQYY MQM"), Some("00100 111".into()));
        assert_eq!(format(date, "w"), None);
    }
}
