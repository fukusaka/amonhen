//! 図のテーマの変数が決める系列の色（pele の `src/svg/palette.ts`）。

use std::cell::RefCell;

use crate::preprocess::Config;
use crate::svg::builder::esc;
use crate::svg::ink::{ink, plain_color};
use crate::util::chars::{is_space_char, is_word_char};
use crate::util::yaml::Value;

/// Mermaid のパレットのテーマ変数。`first` は最初の系列の変数の番号。mindmap の枝は cScale1 から（cScale0 は
/// Mermaid のスタイルで根のもの）。venn のパレットは、Mermaid と同じく決めた色を繰り返す。
fn palettes(type_: &str) -> Option<(&'static str, usize, bool)> {
    Some(match type_ {
        "pie" => ("pie", 1, false),
        "gitGraph" => ("git", 0, false),
        "timeline" | "radar" | "treemap" => ("cScale", 0, false),
        "mindmap" => ("cScale", 1, false),
        "venn" => ("venn", 1, true),
        _ => return None,
    })
}

const SLOTS: usize = 8;

const FUNCTIONS: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "hwb",
    "lab",
    "lch",
    "oklab",
    "oklch",
    "color",
    "color-mix",
    "light-dark",
    "var",
];

/// 資源を読まず、宣言を終わらせず、ページの変数を読まない色。関数の外の値は色として確かめ、打ち間違いは
/// テーマの色のままにする。
fn safe_color(value: Option<&Value>) -> Option<String> {
    let Some(Value::Str(value)) = value else {
        return None;
    };
    let color = crate::js::trim(value);
    // `/^[\w\s#%.,+\-/()]{1,200}$/`
    let n = color.encode_utf16().count();
    if !(1..=200).contains(&n)
        || !color
            .chars()
            .all(|c| is_word_char(c) || is_space_char(c) || "#%.,+-/()".contains(c))
    {
        return None;
    }
    if !color.contains('(') {
        return plain_color(color).then(|| color.to_string());
    }
    let mut depth = 0i32;
    for c in color.chars() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth < 0 {
                return None;
            }
        }
    }
    if depth != 0 {
        return None;
    }
    // `/([\w-]*)\s*\(/g`
    let ch: Vec<char> = color.chars().collect();
    let mut i = 0;
    while i < ch.len() {
        let start = i;
        let mut j = i;
        while j < ch.len() && (is_word_char(ch[j]) || ch[j] == '-') {
            j += 1;
        }
        let mut k = j;
        while k < ch.len() && is_space_char(ch[k]) {
            k += 1;
        }
        if k < ch.len() && ch[k] == '(' {
            let name: String = ch[start..j].iter().collect();
            if !FUNCTIONS.contains(&name.to_lowercase().as_str()) {
                return None;
            }
            i = k + 1;
        } else {
            i += 1;
        }
    }
    // `/var\(\s*([\w-]*)/gi`
    let lower = color.to_lowercase();
    let mut from = 0;
    while let Some(k) = lower[from..].find("var(") {
        let at = from + k + 4;
        let rest = color[at..].trim_start_matches(is_space_char);
        let name: String = rest
            .chars()
            .take_while(|c| is_word_char(*c) || *c == '-')
            .collect();
        if !name.starts_with("--pele-") {
            return None;
        }
        from = at;
    }
    Some(color.to_string())
}

/// 関数の中でない、色の間のカンマで分ける。
fn split_list(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in value.chars() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
        } else if c == ',' && depth == 0 {
            parts.push(crate::js::trim(&cur).to_string());
            cur.clear();
            continue;
        }
        cur.push(c);
    }
    parts.push(crate::js::trim(&cur).to_string());
    parts
}

fn entries(type_: &str, vars: Option<&Config>) -> Option<(Vec<Option<Value>>, bool)> {
    let empty = Config::new();
    let vars = vars.unwrap_or(&empty);
    if type_ == "xychart" {
        let list = vars
            .get("xyChart")
            .and_then(Value::as_obj)
            .and_then(|x| x.get("plotColorPalette"));
        return match list {
            Some(Value::Str(s)) => Some((
                split_list(s)
                    .into_iter()
                    .map(|v| Some(Value::Str(v)))
                    .collect(),
                true,
            )),
            _ => None,
        };
    }
    let (prefix, first, repeat) = palettes(type_)?;
    let mut list: Vec<Option<Value>> = (0..SLOTS)
        .map(|i| vars.get(&format!("{prefix}{}", first + i)).cloned())
        .collect();
    if repeat {
        list.retain(|v| {
            !matches!(v, None | Some(Value::Null)) && v.as_ref() != Some(&Value::Str(String::new()))
        });
    }
    Some((list, repeat))
}

#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub style: String,
    /// 系列の色の上の文字の色（色の見やすさが分かるとき）。
    pub inks: Vec<Option<&'static str>>,
}

/// 図のテーマ変数が決めるパレットの `--pele-series-*` の宣言。使える色でないものは、その系列をテーマの色のままにする。
pub fn palette(type_: &str, config: &Config) -> Palette {
    let vars = config.get("themeVariables").and_then(Value::as_obj);
    let Some((list, repeat)) = entries(type_, vars) else {
        return Palette::default();
    };
    if list.is_empty() {
        return Palette::default();
    }
    let mut style = String::new();
    let mut inks = vec![None; SLOTS];
    for i in 0..SLOTS {
        if !repeat && i >= list.len() {
            break;
        }
        let Some(color) = safe_color(list[i % list.len()].as_ref()) else {
            continue;
        };
        style.push_str(&format!(";--pele-series-{}:{}", i + 1, color));
        inks[i] = ink(&format!(";fill:{color}"), "");
    }
    if style.is_empty() {
        Palette::default()
    } else {
        Palette {
            style: esc(&style),
            inks,
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Palette> = RefCell::new(Palette::default());
}

pub fn with_palette<T>(next: Palette, run: impl FnOnce() -> T) -> T {
    let outer = CURRENT.with(|c| c.replace(next));
    let out = run();
    CURRENT.with(|c| *c.borrow_mut() = outer);
    out
}

pub fn palette_style() -> String {
    CURRENT.with(|c| c.borrow().style.clone())
}

/// 系列の色の上のラベルの塗り（パレットがそれを要る色を決めるとき）。
pub fn series_ink_fill(index: i64) -> String {
    let k = index.rem_euclid(SLOTS as i64) as usize;
    CURRENT.with(|c| match c.borrow().inks.get(k).copied().flatten() {
        Some(color) => format!(" fill=\"{color}\""),
        None => String::new(),
    })
}
