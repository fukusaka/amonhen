//! gantt が頼る JS の `Date`（V8 のふるまい）。日付は時刻の値（1970 年からのミリ秒、`f64`、不正なら NaN）で表し、
//! 取り出しと書き換えは関数にする。
//!
//! pele は「地方時」で数えるので、出力はホストの時間帯で変わる。ここでは地方時を UTC とし、ホストに関わらず同じ
//! 図を描く（node を `TZ=UTC` で動かした pele と同じ）。そのため `getTimezoneOffset` はいつも 0 で、地方時と UTC の
//! 取り出しは同じになる。
//!
//! `parse` は V8 の `Date.parse`（ES5 の ISO の形と、それ以外の古い形の読み）。

use std::time::{SystemTime, UNIX_EPOCH};

const MS_PER_DAY: f64 = 86_400_000.0;
/// ±1e8 日。
const MAX_TIME: f64 = 8.64e15;
const MIN_YEAR: f64 = -1_000_000.0;
const MAX_YEAR: f64 = 1_000_000.0;
const MIN_MONTH: f64 = -10_000_000.0;
const MAX_MONTH: f64 = 10_000_000.0;

/// `Date.now()`。
pub fn now() -> f64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as f64,
        Err(e) => -(e.duration().as_millis() as f64),
    }
}

/// V8 の `DoubleToInteger`。
fn integer(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x.trunc() + 0.0 }
}

/// `TimeClip`。
pub fn time_clip(t: f64) -> f64 {
    if (-MAX_TIME..=MAX_TIME).contains(&t) {
        integer(t)
    } else {
        f64::NAN
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// V8 の `MakeDay`。月は 0 から。
pub fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if (MIN_YEAR..=MAX_YEAR).contains(&year)
        && (MIN_MONTH..=MAX_MONTH).contains(&month)
        && date.is_finite()
    {
        let mut y = year.trunc() as i64;
        let mut m = month.trunc() as i64;
        y += m / 12;
        m %= 12;
        if m < 0 {
            m += 12;
            y -= 1;
        }
        days_from_civil(y, m + 1, 1) as f64 + integer(date) - 1.0
    } else {
        f64::NAN
    }
}

/// V8 の `MakeTime`。
pub fn make_time(h: f64, m: f64, s: f64, ms: f64) -> f64 {
    if h.is_finite() && m.is_finite() && s.is_finite() && ms.is_finite() {
        integer(h) * 3_600_000.0 + integer(m) * 60_000.0 + integer(s) * 1000.0 + integer(ms)
    } else {
        f64::NAN
    }
}

pub fn make_date(day: f64, time: f64) -> f64 {
    if day.is_finite() && time.is_finite() {
        time + day * MS_PER_DAY
    } else {
        f64::NAN
    }
}

/// 0〜99 の年は 1900 年代（`new Date(y, …)` と `Date.UTC`）。
fn full_year(year: f64) -> f64 {
    if year.is_nan() {
        return year;
    }
    let y = integer(year);
    if (0.0..=99.0).contains(&y) {
        1900.0 + y
    } else {
        year
    }
}

/// `new Date(y, m, d, h, mi, s, ms)`（地方時）。
pub fn local(y: f64, m: f64, d: f64, h: f64, mi: f64, s: f64, ms: f64) -> f64 {
    time_clip(make_date(
        make_day(full_year(y), m, d),
        make_time(h, mi, s, ms),
    ))
}

/// `Date.UTC(y, m, d, h, mi, s, ms)`。
pub fn utc(y: f64, m: f64, d: f64, h: f64, mi: f64, s: f64, ms: f64) -> f64 {
    local(y, m, d, h, mi, s, ms)
}

struct Fields {
    year: f64,
    month: f64,
    date: f64,
    time_in_day: f64,
}

fn fields(t: f64) -> Fields {
    let days = (t / MS_PER_DAY).floor();
    let (y, m, d) = civil_from_days(days as i64);
    Fields {
        year: y as f64,
        month: (m - 1) as f64,
        date: d as f64,
        time_in_day: t - days * MS_PER_DAY,
    }
}

pub fn get_full_year(t: f64) -> f64 {
    if t.is_nan() { t } else { fields(t).year }
}

pub fn get_month(t: f64) -> f64 {
    if t.is_nan() { t } else { fields(t).month }
}

pub fn get_date(t: f64) -> f64 {
    if t.is_nan() { t } else { fields(t).date }
}

/// `getDay()`（0 が日曜）。
pub fn get_day(t: f64) -> f64 {
    if t.is_nan() {
        t
    } else {
        ((t / MS_PER_DAY).floor() + 4.0).rem_euclid(7.0)
    }
}

fn in_day(t: f64) -> f64 {
    t.rem_euclid(MS_PER_DAY)
}

pub fn get_hours(t: f64) -> f64 {
    if t.is_nan() {
        t
    } else {
        (in_day(t) / 3_600_000.0).floor()
    }
}

pub fn get_minutes(t: f64) -> f64 {
    if t.is_nan() {
        t
    } else {
        (in_day(t) / 60_000.0).floor() % 60.0
    }
}

pub fn get_seconds(t: f64) -> f64 {
    if t.is_nan() {
        t
    } else {
        (in_day(t) / 1000.0).floor() % 60.0
    }
}

pub fn get_milliseconds(t: f64) -> f64 {
    if t.is_nan() { t } else { in_day(t) % 1000.0 }
}

/// `getTimezoneOffset()`（地方時は UTC）。
pub fn get_timezone_offset(t: f64) -> f64 {
    if t.is_nan() { t } else { 0.0 }
}

/// `setDate(dt)`。
pub fn set_date(t: f64, dt: f64) -> f64 {
    if t.is_nan() {
        return t;
    }
    let f = fields(t);
    time_clip(make_date(make_day(f.year, f.month, dt), f.time_in_day))
}

/// `setMonth(m)`・`setMonth(m, dt)`。
pub fn set_month(t: f64, m: f64, dt: Option<f64>) -> f64 {
    if t.is_nan() {
        return t;
    }
    let f = fields(t);
    time_clip(make_date(
        make_day(f.year, m, dt.unwrap_or(f.date)),
        f.time_in_day,
    ))
}

/// `setFullYear(y)`。不正な日付は 1970 年 1 月 1 日 0 時として書き換える。
pub fn set_full_year(t: f64, y: f64) -> f64 {
    let (m, dt, time) = if t.is_nan() {
        (0.0, 1.0, 0.0)
    } else {
        let f = fields(t);
        (f.month, f.date, f.time_in_day)
    };
    time_clip(make_date(make_day(y, m, dt), time))
}

/// `setHours(h, m, s, ms)`。
pub fn set_hours(t: f64, h: f64, m: f64, s: f64, ms: f64) -> f64 {
    if t.is_nan() {
        return t;
    }
    let days = (t / MS_PER_DAY).floor();
    time_clip(make_date(days, make_time(h, m, s, ms)))
}

// --- Date.parse（V8 の dateparser） ---

const NONE: i64 = i64::MIN;

#[derive(Clone, Copy, PartialEq, Debug)]
enum KeywordType {
    Invalid,
    MonthName,
    TimeZoneName,
    TimeSeparator,
    AmPm,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Token {
    Invalid,
    Unknown,
    Number {
        value: i64,
        length: usize,
    },
    Symbol(u16),
    WhiteSpace,
    Keyword {
        kind: KeywordType,
        value: i64,
        length: usize,
    },
    End,
}

impl Token {
    fn is_number(&self) -> bool {
        matches!(self, Token::Number { .. })
    }

    fn number(&self) -> i64 {
        match self {
            Token::Number { value, .. } => *value,
            _ => 0,
        }
    }

    fn is_fixed_length_number(&self, n: usize) -> bool {
        matches!(self, Token::Number { length, .. } if *length == n)
    }

    fn is_symbol(&self, c: u8) -> bool {
        *self == Token::Symbol(c as u16)
    }

    fn is_ascii_sign(&self) -> bool {
        self.is_symbol(b'+') || self.is_symbol(b'-')
    }

    fn ascii_sign(&self) -> i64 {
        if self.is_symbol(b'+') { 1 } else { -1 }
    }

    fn is_keyword_z(&self) -> bool {
        matches!(
            self,
            Token::Keyword {
                kind: KeywordType::TimeZoneName,
                value: 0,
                length: 1
            }
        )
    }

    fn is_keyword_type(&self, k: KeywordType) -> bool {
        matches!(self, Token::Keyword { kind, .. } if *kind == k)
    }
}

const KEYWORDS: &[(&[u8; 3], KeywordType, i64)] = &[
    (b"jan", KeywordType::MonthName, 1),
    (b"feb", KeywordType::MonthName, 2),
    (b"mar", KeywordType::MonthName, 3),
    (b"apr", KeywordType::MonthName, 4),
    (b"may", KeywordType::MonthName, 5),
    (b"jun", KeywordType::MonthName, 6),
    (b"jul", KeywordType::MonthName, 7),
    (b"aug", KeywordType::MonthName, 8),
    (b"sep", KeywordType::MonthName, 9),
    (b"oct", KeywordType::MonthName, 10),
    (b"nov", KeywordType::MonthName, 11),
    (b"dec", KeywordType::MonthName, 12),
    (b"am\0", KeywordType::AmPm, 0),
    (b"pm\0", KeywordType::AmPm, 12),
    (b"ut\0", KeywordType::TimeZoneName, 0),
    (b"utc", KeywordType::TimeZoneName, 0),
    (b"z\0\0", KeywordType::TimeZoneName, 0),
    (b"gmt", KeywordType::TimeZoneName, 0),
    (b"cdt", KeywordType::TimeZoneName, -5),
    (b"cst", KeywordType::TimeZoneName, -6),
    (b"edt", KeywordType::TimeZoneName, -4),
    (b"est", KeywordType::TimeZoneName, -5),
    (b"mdt", KeywordType::TimeZoneName, -6),
    (b"mst", KeywordType::TimeZoneName, -7),
    (b"pdt", KeywordType::TimeZoneName, -7),
    (b"pst", KeywordType::TimeZoneName, -8),
    (b"t\0\0", KeywordType::TimeSeparator, 0),
];

fn lookup(prefix: [u32; 3], len: usize) -> (KeywordType, i64) {
    for (word, kind, value) in KEYWORDS {
        if (0..3).all(|j| prefix[j] == word[j] as u32)
            && (len <= 3 || *kind == KeywordType::MonthName)
        {
            return (*kind, *value);
        }
    }
    (KeywordType::Invalid, 0)
}

/// JS の WhiteSpace と LineTerminator。
fn is_white(c: u32) -> bool {
    matches!(
        c,
        9 | 10
            | 11
            | 12
            | 13
            | 32
            | 0xa0
            | 0x1680
            | 0x2028
            | 0x2029
            | 0x202f
            | 0x205f
            | 0x3000
            | 0xfeff
    ) || (0x2000..=0x200a).contains(&c)
}

struct Scanner<'a> {
    s: &'a [u16],
    p: usize,
    next: Token,
}

impl<'a> Scanner<'a> {
    fn new(s: &'a [u16]) -> Self {
        let mut sc = Scanner {
            s,
            p: 0,
            next: Token::End,
        };
        sc.next = sc.scan();
        sc
    }

    /// 終わりは 0（V8 と同じく、NUL も終わりのように見える所がある）。
    fn ch(&self) -> u32 {
        self.s.get(self.p).map_or(0, |&c| c as u32)
    }

    fn at_end(&self) -> bool {
        self.p >= self.s.len()
    }

    fn scan(&mut self) -> Token {
        let pre = self.p;
        if self.at_end() {
            return Token::End;
        }
        let c = self.ch();
        if (48..=57).contains(&c) {
            let mut n: i64 = 0;
            let mut i = 0;
            // 頭の 0 は桁に数えない。
            while !self.at_end() && self.ch() == 48 {
                self.p += 1;
            }
            while !self.at_end() && (48..=57).contains(&self.ch()) {
                if i < 9 {
                    n = n * 10 + (self.ch() - 48) as i64;
                }
                i += 1;
                self.p += 1;
            }
            return Token::Number {
                value: n,
                length: self.p - pre,
            };
        }
        for sym in *b":-+.)" {
            if c == sym as u32 {
                self.p += 1;
                return Token::Symbol(sym as u16);
            }
        }
        if c >= 65 && !is_white(c) {
            let mut prefix = [0u32; 3];
            let mut i = 0;
            while !self.at_end() && self.ch() >= 65 && !is_white(self.ch()) {
                if i < 3 {
                    prefix[i] = self.ch() | 0x20;
                }
                i += 1;
                self.p += 1;
            }
            let (kind, value) = lookup(prefix, i);
            return Token::Keyword {
                kind,
                value,
                length: i,
            };
        }
        if is_white(c) {
            self.p += 1;
            return Token::WhiteSpace;
        }
        if c == 40 {
            let mut balance = 0;
            loop {
                let ch = self.ch();
                if ch == 41 {
                    balance -= 1;
                } else if ch == 40 {
                    balance += 1;
                }
                self.p += 1;
                if !(balance > 0 && self.ch() != 0) {
                    break;
                }
            }
            return Token::Unknown;
        }
        self.p += 1;
        Token::Unknown
    }

    fn next(&mut self) -> Token {
        let t = self.next;
        self.next = self.scan();
        t
    }

    fn peek(&self) -> Token {
        self.next
    }

    fn skip_symbol(&mut self, c: u8) -> bool {
        if self.next.is_symbol(c) {
            self.next();
            true
        } else {
            false
        }
    }
}

fn between(x: i64, lo: i64, hi: i64) -> bool {
    lo <= x && x <= hi
}

#[derive(Default)]
struct Day {
    comp: [i64; 3],
    index: usize,
    named_month: Option<i64>,
    iso: bool,
}

impl Day {
    fn add(&mut self, n: i64) -> bool {
        if self.index < 3 {
            self.comp[self.index] = n;
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn write(&mut self) -> Option<(i64, i64, i64)> {
        if self.index < 1 {
            return None;
        }
        while self.index < 3 {
            self.comp[self.index] = 1;
            self.index += 1;
        }
        let is_day = |n: i64| between(n, 1, 31);
        let (mut year, month, day) = match self.named_month {
            None => {
                if self.iso || !is_day(self.comp[0]) {
                    (self.comp[0], self.comp[1], self.comp[2])
                } else {
                    (self.comp[2], self.comp[0], self.comp[1])
                }
            }
            Some(named) => {
                if !is_day(self.comp[0]) {
                    (self.comp[0], named, self.comp[1])
                } else {
                    (self.comp[1], named, self.comp[0])
                }
            }
        };
        if !self.iso {
            if between(year, 0, 49) {
                year += 2000;
            } else if between(year, 50, 99) {
                year += 1900;
            }
        }
        if !between(year, i32::MIN as i64, i32::MAX as i64)
            || !between(month, 1, 12)
            || !is_day(day)
        {
            return None;
        }
        Some((year, month - 1, day))
    }
}

struct Time {
    comp: [i64; 4],
    index: usize,
    hour_offset: i64,
}

impl Time {
    fn new() -> Self {
        Time {
            comp: [0; 4],
            index: 0,
            hour_offset: NONE,
        }
    }

    fn is_empty(&self) -> bool {
        self.index == 0
    }

    fn is_expecting(&self, n: i64) -> bool {
        (self.index == 1 && between(n, 0, 59))
            || (self.index == 2 && between(n, 0, 59))
            || (self.index == 3 && between(n, 0, 999))
    }

    fn add(&mut self, n: i64) -> bool {
        if self.index < 4 {
            self.comp[self.index] = n;
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn add_final(&mut self, n: i64) -> bool {
        if !self.add(n) {
            return false;
        }
        while self.index < 4 {
            self.comp[self.index] = 0;
            self.index += 1;
        }
        true
    }

    fn write(&mut self) -> Option<[i64; 4]> {
        while self.index < 4 {
            self.comp[self.index] = 0;
            self.index += 1;
        }
        let [mut hour, minute, second, ms] = self.comp;
        if self.hour_offset != NONE {
            if !between(hour, 0, 12) {
                return None;
            }
            hour %= 12;
            hour += self.hour_offset;
        }
        if (!between(hour, 0, 23)
            || !between(minute, 0, 59)
            || !between(second, 0, 59)
            || !between(ms, 0, 999))
            && (hour != 24 || minute != 0 || second != 0 || ms != 0)
        {
            return None;
        }
        Some([hour, minute, second, ms])
    }
}

struct Zone {
    sign: i64,
    hour: i64,
    minute: i64,
}

impl Zone {
    fn new() -> Self {
        Zone {
            sign: NONE,
            hour: NONE,
            minute: NONE,
        }
    }

    fn set(&mut self, offset_hours: i64) {
        self.sign = if offset_hours < 0 { -1 } else { 1 };
        self.hour = offset_hours * self.sign;
        self.minute = 0;
    }

    fn is_expecting(&self, n: i64) -> bool {
        self.hour != NONE && self.minute == NONE && between(n, 0, 59)
    }

    fn is_utc(&self) -> bool {
        self.hour == 0 && self.minute == 0
    }

    fn is_empty(&self) -> bool {
        self.hour == NONE
    }

    /// UTC からのずれ（秒）。ないときは None、読めないときは Err。
    fn write(&mut self) -> Result<Option<i64>, ()> {
        if self.sign == NONE {
            return Ok(None);
        }
        if self.hour == NONE {
            self.hour = 0;
        }
        if self.minute == NONE {
            self.minute = 0;
        }
        let total = (self.hour as u32 as u64)
            .wrapping_mul(3600)
            .wrapping_add((self.minute as u32 as u64).wrapping_mul(60))
            & 0xffff_ffff;
        if total > i32::MAX as u64 {
            return Err(());
        }
        let total = total as i64;
        Ok(Some(if self.sign < 0 { -total } else { total }))
    }
}

fn read_milliseconds(token: Token) -> i64 {
    let (mut number, mut length) = match token {
        Token::Number { value, length } => (value, length),
        _ => (0, 0),
    };
    if length < 3 {
        if length == 1 {
            number *= 100;
        } else if length == 2 {
            number *= 10;
        }
    } else if length > 3 {
        if length > 9 {
            length = 9;
        }
        let mut factor = 1;
        loop {
            factor *= 10;
            length -= 1;
            if length <= 3 {
                break;
            }
        }
        number /= factor;
    }
    number
}

/// ES5 の ISO の形を読む。読み終えたら `End`、残りがあれば古い形の読み手に渡す字句、だめなら `Invalid`。
fn parse_es5(sc: &mut Scanner, day: &mut Day, time: &mut Time, tz: &mut Zone) -> Token {
    if sc.peek().is_ascii_sign() {
        let sign_token = sc.next();
        if !sc.peek().is_fixed_length_number(6) {
            return sign_token;
        }
        let sign = sign_token.ascii_sign();
        let year = sc.next().number();
        if sign < 0 && year == 0 {
            return sign_token;
        }
        day.add(sign * year);
    } else if sc.peek().is_fixed_length_number(4) {
        let n = sc.next().number();
        day.add(n);
    } else {
        return sc.next();
    }
    if sc.skip_symbol(b'-') {
        if !sc.peek().is_fixed_length_number(2) || !between(sc.peek().number(), 1, 12) {
            return sc.next();
        }
        let n = sc.next().number();
        day.add(n);
        if sc.skip_symbol(b'-') {
            if !sc.peek().is_fixed_length_number(2) || !between(sc.peek().number(), 1, 31) {
                return sc.next();
            }
            let n = sc.next().number();
            day.add(n);
        }
    }
    if !sc.peek().is_keyword_type(KeywordType::TimeSeparator) {
        if sc.peek() != Token::End {
            return sc.next();
        }
    } else {
        sc.next();
        if !sc.peek().is_fixed_length_number(2) || !between(sc.peek().number(), 0, 24) {
            return Token::Invalid;
        }
        let hour_is_24 = sc.peek().number() == 24;
        let n = sc.next().number();
        time.add(n);
        if !sc.skip_symbol(b':') {
            return Token::Invalid;
        }
        if !sc.peek().is_fixed_length_number(2)
            || !between(sc.peek().number(), 0, 59)
            || (hour_is_24 && sc.peek().number() > 0)
        {
            return Token::Invalid;
        }
        let n = sc.next().number();
        time.add(n);
        if sc.skip_symbol(b':') {
            if !sc.peek().is_fixed_length_number(2)
                || !between(sc.peek().number(), 0, 59)
                || (hour_is_24 && sc.peek().number() > 0)
            {
                return Token::Invalid;
            }
            let n = sc.next().number();
            time.add(n);
            if sc.skip_symbol(b'.') {
                if !sc.peek().is_number() || (hour_is_24 && sc.peek().number() > 0) {
                    return Token::Invalid;
                }
                let t = sc.next();
                time.add(read_milliseconds(t));
            }
        }
        if sc.peek().is_keyword_z() {
            sc.next();
            tz.set(0);
        } else if sc.peek().is_symbol(b'+') || sc.peek().is_symbol(b'-') {
            tz.sign = if sc.next().is_symbol(b'+') { 1 } else { -1 };
            if sc.peek().is_fixed_length_number(4) {
                let hourmin = sc.next().number();
                let (hour, min) = (hourmin / 100, hourmin % 100);
                if !between(hour, 0, 23) || !between(min, 0, 59) {
                    return Token::Invalid;
                }
                tz.hour = hour;
                tz.minute = min;
            } else {
                if !sc.peek().is_fixed_length_number(2) || !between(sc.peek().number(), 0, 23) {
                    return Token::Invalid;
                }
                tz.hour = sc.next().number();
                if !sc.skip_symbol(b':') {
                    return Token::Invalid;
                }
                if !sc.peek().is_fixed_length_number(2) || !between(sc.peek().number(), 0, 59) {
                    return Token::Invalid;
                }
                tz.minute = sc.next().number();
            }
        }
        if sc.peek() != Token::End {
            return Token::Invalid;
        }
    }
    if tz.is_empty() && time.is_empty() {
        tz.set(0);
    }
    day.iso = true;
    Token::End
}

fn parse_fields(s: &[u16]) -> Option<([i64; 3], [i64; 4], Option<i64>)> {
    let mut sc = Scanner::new(s);
    let mut day = Day::default();
    let mut time = Time::new();
    let mut tz = Zone::new();
    let next = parse_es5(&mut sc, &mut day, &mut time, &mut tz);
    if next == Token::Invalid {
        return None;
    }
    let mut has_read_number = day.index > 0;
    let mut token = next;
    while token != Token::End {
        if let Token::Number { value: n, .. } = token {
            has_read_number = true;
            if sc.skip_symbol(b':') {
                if sc.skip_symbol(b':') {
                    if !time.is_empty() {
                        return None;
                    }
                    time.add(n);
                    time.add(0);
                } else {
                    if !time.add(n) {
                        return None;
                    }
                    if sc.peek().is_symbol(b'.') {
                        sc.next();
                    }
                }
            } else if sc.skip_symbol(b'.') && time.is_expecting(n) {
                time.add(n);
                if !sc.peek().is_number() {
                    return None;
                }
                let value = read_milliseconds(sc.next());
                if value < 0 {
                    return None;
                }
                time.add_final(value);
            } else if tz.is_expecting(n) {
                tz.minute = n;
            } else if time.is_expecting(n) {
                time.add_final(n);
                let peek = sc.peek();
                if peek != Token::End
                    && peek != Token::WhiteSpace
                    && !peek.is_keyword_z()
                    && !peek.is_ascii_sign()
                {
                    return None;
                }
            } else {
                if !day.add(n) {
                    return None;
                }
                sc.skip_symbol(b'-');
            }
        } else if let Token::Keyword { kind, value, .. } = token {
            if kind == KeywordType::AmPm && !time.is_empty() {
                time.hour_offset = value;
            } else if kind == KeywordType::MonthName {
                day.named_month = Some(value);
                sc.skip_symbol(b'-');
            } else if kind == KeywordType::TimeZoneName && has_read_number {
                tz.set(value);
            } else {
                if has_read_number {
                    return None;
                }
                if sc.peek().is_number() {
                    return None;
                }
            }
        } else if token.is_ascii_sign() && (tz.is_utc() || !time.is_empty()) {
            tz.sign = token.ascii_sign();
            let mut n = 0;
            let mut length = 0;
            if sc.peek().is_number() {
                let t = sc.next();
                if let Token::Number { value, length: l } = t {
                    n = value;
                    length = l;
                }
            }
            has_read_number = true;
            if sc.peek().is_symbol(b':') {
                tz.hour = n;
                tz.minute = NONE;
            } else if length == 2 || length == 1 {
                tz.hour = n;
                tz.minute = 0;
            } else if length == 4 || length == 3 {
                tz.hour = n / 100;
                tz.minute = n % 100;
            } else {
                return None;
            }
        } else if (token.is_ascii_sign() || token.is_symbol(b')')) && has_read_number {
            return None;
        }
        token = sc.next();
    }
    let d = day.write()?;
    let t = time.write()?;
    let z = tz.write().ok()?;
    Some(([d.0, d.1, d.2], t, z))
}

/// `Date.parse(s)`（`new Date(s)` の時刻の値）。
pub fn parse(s: &str) -> f64 {
    let units: Vec<u16> = s.encode_utf16().collect();
    let Some((d, t, zone)) = parse_fields(&units) else {
        return f64::NAN;
    };
    let day = make_day(d[0] as f64, d[1] as f64, d[2] as f64);
    let time = make_time(t[0] as f64, t[1] as f64, t[2] as f64, t[3] as f64);
    let mut date = make_date(day, time);
    match zone {
        None => {
            if !(-MAX_TIME - 864e6..=MAX_TIME + 864e6).contains(&date) {
                return f64::NAN;
            }
        }
        Some(offset) => {
            date -= offset as f64 * 1000.0;
            if !(-MAX_TIME..=MAX_TIME).contains(&date) {
                return f64::NAN;
            }
        }
    }
    time_clip(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_iso_and_legacy_dates() {
        assert_eq!(parse("2014-01-06"), 1388966400000.0);
        assert_eq!(parse("2014-01-06T10:00"), 1389002400000.0);
        assert_eq!(parse("Jan 6 2014"), 1388966400000.0);
        assert_eq!(parse("1/6/2014 10:00 PM"), 1389045600000.0);
        assert_eq!(parse("nonsense 5"), 988675200000.0);
        assert!(parse("5 nonsense").is_nan());
    }

    #[test]
    fn calendar_arithmetic_follows_js() {
        let t = local(2014.0, 0.0, 31.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(get_month(set_month(t, 1.0, None)), 2.0);
        assert_eq!(
            get_full_year(local(50.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0)),
            1950.0
        );
        assert_eq!(get_day(0.0), 4.0);
        assert!(set_date(f64::NAN, 1.0).is_nan());
        assert_eq!(set_full_year(f64::NAN, 2000.0), 946684800000.0);
    }
}
