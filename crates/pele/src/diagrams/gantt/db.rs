//! gantt の db（pele の `src/diagrams/gantt/db.ts`）。読んだものを溜め、描く前に予定を組む。

use std::collections::{HashMap, HashSet};

use super::dates::{Unit, WEEKDAYS, add, add_days, format, is_valid, iso_date, parse_strict};
use super::js_date as d;
use super::types::{GanttTask, Raw};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;
use crate::util::chars::is_space;
use crate::util::url::safe_url;

/// 予定は日ごとにタスクをたどり、図の決める書式で日付を読む。その合計（おおよその文字の操作の数）を抑え、
/// 悪意のある図が止まらずに失敗するように。
const WORK_LIMIT: f64 = 4e8;
const DAY_STEP: f64 = 400.0;
const DATED_STEP: f64 = 200.0;
const FORMAT_STEP: f64 = 400.0;

/// 今の時刻を読むただ一つの所。
pub fn clock(now: Option<f64>) -> f64 {
    match now {
        Some(n) if n.is_finite() => n,
        _ => d::now(),
    }
}

/// `^(\d+(?:\.\d+)?)([Mdhmswy]|ms)$`（前後の空白を除いて）。
pub fn parse_duration(str: &str) -> (f64, Unit) {
    let s = js::trim(str);
    let b = s.as_bytes();
    let mut i = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if i == 0 {
        return (f64::NAN, Unit::Ms);
    }
    if b.get(i) == Some(&b'.') {
        let k = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if k > 0 {
            i += 1 + k;
        }
    }
    let unit = match &s[i..] {
        "M" => Unit::Month,
        "d" => Unit::D,
        "h" => Unit::H,
        "m" => Unit::M,
        "s" => Unit::S,
        "w" => Unit::W,
        "y" => Unit::Y,
        "ms" => Unit::Ms,
        _ => return (f64::NAN, Unit::Ms),
    };
    (js::parse_float(&s[..i]), unit)
}

/// `split(/[\s,]+/)`
fn split_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut sep = false;
    for c in s.chars() {
        if c == ',' || is_space(c as u32) {
            sep = true;
        } else {
            if sep {
                out.push(std::mem::take(&mut cur));
                sep = false;
            }
            cur.push(c);
        }
    }
    if sep {
        out.push(std::mem::take(&mut cur));
    }
    out.push(cur);
    out
}

fn merge_tokens(existing: &[String], txt: &str) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    for t in existing {
        if !merged.contains(t) {
            merged.push(t.clone());
        }
    }
    for token in split_tokens(&txt.to_lowercase()) {
        if !token.is_empty() && !merged.contains(&token) {
            merged.push(token);
        }
    }
    merged
}

/// `^\s*(active|done|crit|milestone|vert)\s*$`
fn tag(s: &str) -> Option<&str> {
    let t = s.trim_matches(|c: char| is_space(c as u32));
    ["active", "done", "crit", "milestone", "vert"]
        .into_iter()
        .find(|&w| w == t)
}

/// `^<word>\s+([\d\w- ]+)` の組。
fn reference<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(word)?;
    let body = rest.trim_start_matches(|c: char| is_space(c as u32));
    if body.len() == rest.len() {
        return None;
    }
    let n = body
        .bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b' '))
        .count();
    if n > 0 {
        return Some(&body[..n]);
    }
    // 空白の後に合う文字がなければ、正規表現は `\s+` を戻し、空白の中の ' ' を組に取る。
    let run = &rest[..rest.len() - body.len()];
    let first = run.chars().next().map_or(0, char::len_utf8);
    run[first..].contains(' ').then_some(" ")
}

/// `^[YMDHhmsSZXx\-/.:, T]*$`
fn numeric_format(s: &str) -> bool {
    s.bytes().all(|c| b"YMDHhmsSZXx-/.:, T".contains(&c))
}

fn semantic(message: String) -> PeleError {
    PeleError::typed(message, ErrorCode::Semantic, "gantt")
}

#[derive(Clone, Default)]
pub struct GanttDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub date_format: String,
    pub axis_format: String,
    pub tick_interval: Option<String>,
    pub today_marker: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub inclusive_end_dates: bool,
    pub top_axis: bool,
    pub display_mode: String,
    /// 週ごとの目盛りが始まる曜日（図が決めたとき）。Mermaid の既定は日曜。
    pub weekday: Option<String>,
    pub weekend: String,
    pub sections: Vec<String>,
    /// 入れた順を保つ（JS の `Map`）。
    pub links: Vec<(String, String)>,
    pub tasks: Vec<GanttTask>,

    by_id: HashMap<String, usize>,
    current_section: String,
    task_count: usize,
    last_order: i64,
    last_task_id: Option<String>,
    now: f64,
    work: f64,
    include_set: HashSet<String>,
    exclude_set: HashSet<String>,
    excluded_days: [bool; 7],
    dated_tokens: bool,
    /// 除く日かどうか（日付の値ごと）。`get_tasks` のたびに一から。
    invalid_days: HashMap<u64, bool>,
    /// 書式（前後の空白を除いた範囲）と、それが `YYYY-MM-DD` か、日ごとに書き出す仕事。`get_tasks` が決める。
    format_at: std::ops::Range<usize>,
    format_cost: (bool, f64),
}

impl GanttDb {
    pub fn new() -> Self {
        GanttDb {
            weekend: "saturday".into(),
            ..Default::default()
        }
    }

    pub fn set_date_format(&mut self, txt: &str) {
        self.date_format = txt.to_string();
    }

    pub fn set_includes(&mut self, txt: &str) {
        self.includes = merge_tokens(&self.includes, txt);
    }

    pub fn set_excludes(&mut self, txt: &str) {
        self.excludes = merge_tokens(&self.excludes, txt);
    }

    pub fn set_acc_title(&mut self, txt: &str) {
        self.acc_title = Some(js::trim_start(txt).to_string());
    }

    /// `txt.replace(/\n\s+/g, '\n')`
    pub fn set_acc_description(&mut self, txt: &str) {
        let mut out = String::new();
        let mut chars = txt.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' && chars.peek().is_some_and(|&n| is_space(n as u32)) {
                while chars.peek().is_some_and(|&n| is_space(n as u32)) {
                    chars.next();
                }
            }
        }
        self.acc_descr = Some(out);
    }

    pub fn add_section(&mut self, txt: &str) {
        self.current_section = txt.to_string();
        self.sections.push(txt.to_string());
    }

    pub fn add_task(&mut self, descr: &str, data_str: &str) -> Result<()> {
        let body = data_str.strip_prefix(':').unwrap_or(data_str);
        let mut data: Vec<String> = body.split(',').map(str::to_string).collect();
        let mut task = GanttTask {
            id: String::new(),
            task: descr.to_string(),
            section: self.current_section.clone(),
            start_time: None,
            end_time: None,
            render_end_time: None,
            manual_end_time: false,
            processed: false,
            active: false,
            done: false,
            crit: false,
            milestone: false,
            vert: false,
            order: -1,
            classes: Vec::new(),
            prev_task_id: self.last_task_id.clone(),
            raw: Raw {
                data: data_str.to_string(),
                start: None,
                end: String::new(),
            },
        };

        while let Some(t) = data.first().and_then(|first| tag(first)) {
            match t {
                "active" => task.active = true,
                "done" => task.done = true,
                "crit" => task.crit = true,
                "milestone" => task.milestone = true,
                _ => task.vert = true,
            }
            data.remove(0);
        }
        // Mermaid はどちらも TypeError で失敗する。
        if data.is_empty() {
            return Err(semantic(format!(
                "Task \"{}\" needs a duration or an end date.",
                js::trim(descr)
            )));
        }
        if data.len() > 3 {
            return Err(semantic(format!(
                "Task \"{}\" has too many fields. A task takes an id, a start, and an end or duration.",
                js::trim(descr)
            )));
        }
        for item in data.iter_mut() {
            *item = js::trim(item).to_string();
        }

        if data.len() == 3 {
            task.id = data[0].clone();
        } else {
            self.task_count += 1;
            task.id = format!("task{}", self.task_count);
        }
        if data.len() > 1 {
            task.raw.start = Some(data[data.len() - 2].clone());
        }
        task.raw.end = data[data.len() - 1].clone();

        if !task.vert {
            task.order = self.last_order;
            self.last_order += 1;
        }
        self.last_task_id = Some(task.id.clone());
        self.by_id.insert(task.id.clone(), self.tasks.len());
        self.tasks.push(task);
        Ok(())
    }

    pub fn set_link(&mut self, ids: &str, link: &str) {
        let href = safe_url(link);
        for id in ids.split(',') {
            if self.by_id.contains_key(id) {
                match self.links.iter_mut().find(|(k, _)| k == id) {
                    Some(slot) => slot.1 = href.clone(),
                    None => self.links.push((id.to_string(), href.clone())),
                }
            }
        }
        self.set_class(ids, "clickable");
    }

    /// 呼び出しは読み取るが動かさない。Mermaid と同じく、タスクに押せる印をつけるだけ。
    pub fn set_click_event(&mut self, ids: &str) {
        self.set_class(ids, "clickable");
    }

    pub fn set_class(&mut self, ids: &str, class_name: &str) {
        for id in ids.split(',') {
            if let Some(&k) = self.by_id.get(id) {
                self.tasks[k].classes.push(class_name.to_string());
            }
        }
    }

    /// すべてのタスクの始まりと終わりを、呼ぶたびに一から解く。タスクは後で定めたタスクを指せるので、Mermaid と
    /// 同じく 11 回まで、そのたびにすべてのタスクを組み直す。
    pub fn get_tasks(&mut self, now: f64) -> Result<()> {
        self.now = now;
        self.work = 0.0;
        self.invalid_days.clear();
        let template = js::trim(&self.date_format);
        let start = template.as_ptr() as usize - self.date_format.as_ptr() as usize;
        self.format_at = start..start + template.len();
        self.format_cost = (
            template == "YYYY-MM-DD",
            FORMAT_STEP + js::len16(template) as f64 * 8.0,
        );
        self.include_set = self.includes.iter().cloned().collect();
        self.exclude_set = self.excludes.iter().cloned().collect();
        let weekend_start = match self.weekend.as_str() {
            "friday" => 5.0,
            "saturday" => 6.0,
            _ => f64::NAN,
        };
        let weekends = self.exclude_set.contains("weekends");
        for (day, name) in WEEKDAYS.iter().enumerate() {
            let iso = if day == 0 { 7.0 } else { day as f64 };
            self.excluded_days[day] = (weekends
                && (iso == weekend_start || iso == weekend_start + 1.0))
                || self.exclude_set.contains(&name.to_lowercase());
        }
        // この字句と区切りだけで書いた日付は曜日の名前に読めないので、ほかに除くものも含めるものもなければ、
        // 日ごとに書き出さなくてよい。
        self.dated_tokens =
            !self.includes.is_empty() || !numeric_format(js::trim(&self.date_format));
        for token in &self.excludes {
            if token != "weekends" && !WEEKDAYS.iter().any(|w| w.to_lowercase() == *token) {
                self.dated_tokens = true;
            }
        }

        for task in &mut self.tasks {
            task.start_time = None;
            task.end_time = None;
            task.render_end_time = None;
            task.processed = false;
            task.manual_end_time = false;
        }

        let mut all = self.compile_tasks()?;
        let mut i = 0;
        while !all && i < 10 {
            all = self.compile_tasks()?;
            i += 1;
        }
        Ok(())
    }

    /// 除く日か。日は曜日の名前、YYYY-MM-DD の日付、図の書式で書いた日付で字句に合う。
    pub fn is_invalid_date(&mut self, date: f64) -> Result<bool> {
        let day = d::get_day(date);
        let weekday = !day.is_nan() && self.excluded_days[day as usize];
        if !self.dated_tokens {
            return Ok(weekday);
        }
        let (iso, format_step) = self.format_cost;
        self.spend(DATED_STEP)?;
        if !iso {
            self.spend(format_step)?;
        }
        // 答えは日付だけで決まる（予定を組み直すたびに、重なるタスクが同じ日を何度も見る）。
        if let Some(&known) = self.invalid_days.get(&date.to_bits()) {
            return Ok(known);
        }
        let date_only = iso_date(date);
        let formatted = if iso {
            None
        } else {
            format(date, &self.date_format[self.format_at.clone()])
        };
        let listed = |set: &HashSet<String>| {
            set.contains(&date_only) || formatted.as_ref().is_some_and(|f| set.contains(f))
        };
        let invalid = !listed(&self.include_set) && (weekday || listed(&self.exclude_set));
        self.invalid_days.insert(date.to_bits(), invalid);
        Ok(invalid)
    }

    fn spend(&mut self, units: f64) -> Result<()> {
        self.work += units;
        if self.work > WORK_LIMIT {
            return Err(PeleError::typed(
                "The gantt chart takes too much work to schedule.",
                ErrorCode::Limit,
                "gantt",
            ));
        }
        Ok(())
    }

    fn parse(&mut self, str: &str, template: &str) -> Result<Option<f64>> {
        self.spend((js::len16(template) as f64 + 1.0) * (js::len16(str) as f64 + 1.0))?;
        Ok(parse_strict(str, template, self.now))
    }

    fn today(&self) -> f64 {
        d::set_hours(d::time_clip(self.now), 0.0, 0.0, 0.0, 0.0)
    }

    fn compile_tasks(&mut self) -> Result<bool> {
        let mut all = true;
        for k in 0..self.tasks.len() {
            self.compile_task(k)?;
            all = all && self.tasks[k].processed;
        }
        Ok(all)
    }

    fn compile_task(&mut self, k: usize) -> Result<()> {
        match self.tasks[k].raw.start.clone() {
            None => {
                // Mermaid は始まりのない最初のタスクで TypeError になる。ここでは今日に始める。
                let previous = self.tasks[k]
                    .prev_task_id
                    .as_ref()
                    .and_then(|id| self.by_id.get(id));
                self.tasks[k].start_time = match previous {
                    Some(&p) => self.tasks[p].end_time,
                    None => Some(self.today()),
                };
            }
            Some(start) => {
                if let Some(t) = self.get_start_date(&start)? {
                    self.tasks[k].start_time = Some(t);
                }
            }
        }
        let Some(start) = self.tasks[k].start_time else {
            return Ok(());
        };
        let raw_end = self.tasks[k].raw.end.clone();
        let end = self.get_end_date(start, &raw_end)?;
        self.tasks[k].end_time = end;
        if end.is_none() {
            return Ok(());
        }
        self.tasks[k].processed = true;
        self.tasks[k].manual_end_time = self.parse(&raw_end, "YYYY-MM-DD")?.is_some();
        self.check_task_dates(k)
    }

    fn references(&self, list: &str) -> Vec<usize> {
        list.split(' ')
            .filter(|id| !id.is_empty())
            .filter_map(|id| self.by_id.get(id).copied())
            .collect()
    }

    fn get_start_date(&mut self, raw: &str) -> Result<Option<f64>> {
        let str = js::trim(raw);
        let template = js::trim(&self.date_format).to_string();

        if (template == "x" || template == "X")
            && !str.is_empty()
            && str.bytes().all(|c| c.is_ascii_digit())
        {
            let date = d::time_clip(js::number(str));
            if !is_valid(date) {
                return Err(semantic(format!("Invalid date:{str}")));
            }
            return Ok(Some(date));
        }

        if let Some(list) = reference(str, "after") {
            let mut latest: Option<usize> = None;
            for t in self.references(list) {
                match latest {
                    None => latest = Some(t),
                    Some(l) => {
                        if gt(self.tasks[t].end_time, self.tasks[l].end_time) {
                            latest = Some(t);
                        }
                    }
                }
            }
            return Ok(match latest {
                Some(l) => self.tasks[l].end_time,
                None => Some(self.today()),
            });
        }

        if let Some(parsed) = self.parse(str, &template)? {
            return Ok(Some(parsed));
        }

        // 書式に合わないものは、Mermaid と同じく JS の日付の読み手でもう一度試す。
        let date = d::parse(str);
        let year = d::get_full_year(date);
        if !is_valid(date) || year < -10000.0 || year > 10000.0 {
            return Err(semantic(format!("Invalid date:{str}")));
        }
        Ok(Some(date))
    }

    fn get_end_date(&mut self, start_time: f64, raw: &str) -> Result<Option<f64>> {
        let str = js::trim(raw);

        if let Some(list) = reference(str, "until") {
            let mut earliest: Option<usize> = None;
            for t in self.references(list) {
                match earliest {
                    None => earliest = Some(t),
                    Some(e) => {
                        if gt(self.tasks[e].start_time, self.tasks[t].start_time) {
                            earliest = Some(t);
                        }
                    }
                }
            }
            return Ok(match earliest {
                Some(e) => self.tasks[e].start_time,
                None => Some(self.today()),
            });
        }

        let template = js::trim(&self.date_format).to_string();
        if let Some(parsed) = self.parse(str, &template)? {
            return Ok(Some(if self.inclusive_end_dates {
                add_days(parsed, 1.0)
            } else {
                parsed
            }));
        }

        let (value, unit) = parse_duration(str);
        if value.is_nan() {
            return Ok(Some(start_time));
        }
        let end = add(start_time, value, unit);
        Ok(Some(if is_valid(end) { end } else { start_time }))
    }

    /// 除く日をまたぐたびに、タスクの終わりを 1 日ずつ押し出す。
    fn check_task_dates(&mut self, k: usize) -> Result<()> {
        if self.excludes.is_empty() || self.tasks[k].manual_end_time {
            return Ok(());
        }
        let start = self.tasks[k].start_time.unwrap_or(f64::NAN);
        let mut cursor = add_days(start, 1.0);
        let mut end_time = d::time_clip(self.tasks[k].end_time.unwrap_or(f64::NAN));
        let max_end_time = add_days(end_time, 10000.0);
        let mut invalid = false;
        let mut render_end_time: Option<f64> = None;
        while cursor <= end_time {
            self.spend(DAY_STEP)?;
            if !invalid {
                render_end_time = Some(end_time);
            }
            invalid = self.is_invalid_date(cursor)?;
            if invalid {
                end_time = d::set_date(end_time, d::get_date(end_time) + 1.0);
                if end_time > max_end_time {
                    return Err(semantic(
                        "Failed to find a valid date that was not excluded by `excludes` after 10,000 iterations."
                            .into(),
                    ));
                }
            }
            cursor = d::set_date(cursor, d::get_date(cursor) + 1.0);
        }
        self.tasks[k].end_time = Some(end_time);
        self.tasks[k].render_end_time = render_end_time;
        Ok(())
    }
}

/// `a > b`（`undefined` との比べは偽）。
fn gt(a: Option<f64>, b: Option<f64>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a > b)
}
