//! 構文の色のテーマ（macOS 版の `SyntaxColors.swift`）。明るい外観と暗い外観で別々に選ぶ（Settings > Theme）。
//! System（これまでの色）、内蔵のテーマ、VSCode の配色テーマ（JSON）から読み込んだテーマ。読み込んだものは
//! 設定の `importedSyntaxThemes` に JSON で置く（形は macOS 版と同じ）。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;

use amonhen_layout::SyntaxSpan;
use gtk::gdk;
use gtk::prelude::*;
use serde_json::{Map, Value, json};

use crate::settings;

pub const SYNTAX_THEME_LIGHT: &str = "syntaxThemeLight";
pub const SYNTAX_THEME_DARK: &str = "syntaxThemeDark";
pub const IMPORTED_SYNTAX_THEMES: &str = "importedSyntaxThemes";
/// 差分の色は、テーマに関係なく標準の色を使う（構文の色はテーマのまま）。
pub const DIFF_STANDARD_COLORS: &str = "diffStandardColors";

/// 差分の色の種類（macOS 版の `DiffColor`）。テーマの `colors` の鍵（`#rrggbbaa`）と、テーマが持たないときの
/// 標準の色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffColor {
    /// 追加・削除の行の地。
    Added,
    Removed,
    /// 変わった単語の地。
    AddedWord,
    RemovedWord,
    /// hunk の見出しの行の地。
    Hunk,
    /// コンフリクトの Current・Incoming・共通の祖先の行と、マーカーの行の地。マージエディタも同じ色合い。
    Current,
    Incoming,
    Base,
    Marker,
}

impl DiffColor {
    pub fn key(self) -> &'static str {
        match self {
            DiffColor::Added => "diffAdded",
            DiffColor::Removed => "diffRemoved",
            DiffColor::AddedWord => "diffAddedWord",
            DiffColor::RemovedWord => "diffRemovedWord",
            DiffColor::Hunk => "diffHunk",
            DiffColor::Current => "conflictCurrent",
            DiffColor::Incoming => "conflictIncoming",
            DiffColor::Base => "conflictBase",
            DiffColor::Marker => "conflictMarker",
        }
    }

    /// これまでの色。
    pub fn standard(self) -> gdk::RGBA {
        let (r, g, b, a) = match self {
            DiffColor::Added => (0.2, 0.7, 0.3, 0.16),
            DiffColor::Removed => (0.9, 0.25, 0.25, 0.16),
            DiffColor::AddedWord => (0.2, 0.7, 0.3, 0.35),
            DiffColor::RemovedWord => (0.9, 0.25, 0.25, 0.35),
            DiffColor::Hunk => (0.3, 0.5, 0.9, 0.10),
            DiffColor::Current => (0.3, 0.5, 0.9, 0.18),
            DiffColor::Incoming => (0.2, 0.7, 0.3, 0.18),
            DiffColor::Base => (0.5, 0.5, 0.5, 0.18),
            DiffColor::Marker => (1.0, 0.58, 0.0, 0.14),
        };
        gdk::RGBA::new(r, g, b, a)
    }
}

/// System（これまでの色）の名前。
pub const SYSTEM: &str = "System";

/// 構文の色のテーマ。種類（`keyword` など）→ `#rrggbb`。ない種類は本文の色。差分の色（[`DiffColor`] の鍵 →
/// `#rrggbbaa`）も同じ表に入れ、ないものは標準の色。
#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxTheme {
    pub name: String,
    /// 暗い外観向けか（一覧で明るい・暗いに分けて出す）。
    pub dark: bool,
    pub colors: HashMap<String, String>,
}

/// 種類の名前と `SyntaxSpan` の定数（macOS 版の `SyntaxTheme.kinds` と同じ並び）。
pub const KINDS: [(&str, u8); 14] = [
    ("keyword", SyntaxSpan::KEYWORD),
    ("string", SyntaxSpan::STRING),
    ("comment", SyntaxSpan::COMMENT),
    ("number", SyntaxSpan::NUMBER),
    ("function", SyntaxSpan::FUNCTION),
    ("type", SyntaxSpan::TYPE),
    ("constant", SyntaxSpan::CONSTANT),
    ("property", SyntaxSpan::PROPERTY),
    ("attribute", SyntaxSpan::ATTRIBUTE),
    ("tag", SyntaxSpan::TAG),
    ("operator", SyntaxSpan::OPERATOR),
    ("variable", SyntaxSpan::VARIABLE),
    ("escape", SyntaxSpan::ESCAPE),
    ("module", SyntaxSpan::MODULE),
];

impl SyntaxTheme {
    fn new(name: &str, dark: bool, hex: [&str; 14], diff: [&str; 5]) -> Self {
        let mut colors: HashMap<String, String> = KINDS
            .iter()
            .zip(hex)
            .map(|((kind, _), hex)| ((*kind).into(), hex.into()))
            .collect();
        colors.extend(diff_colors(diff));
        Self {
            name: name.into(),
            dark,
            colors,
        }
    }

    fn system() -> Self {
        Self {
            name: SYSTEM.into(),
            dark: false,
            colors: HashMap::new(),
        }
    }

    /// 種類の色（`None` は本文の色のまま）。`dark` は今の外観が暗いか（System の色を選ぶため）。
    /// 差分の色（テーマが持たなければ `None`）。
    pub fn diff_color(&self, kind: DiffColor) -> Option<gdk::RGBA> {
        self.colors.get(kind.key()).and_then(|hex| parse_rgba(hex))
    }

    pub fn color(&self, kind: u8, dark: bool) -> Option<(f32, f32, f32)> {
        if self.name == SYSTEM && self.colors.is_empty() {
            return system_color(kind, dark);
        }
        let name = KINDS.iter().find(|(_, k)| *k == kind)?.0;
        self.colors.get(name).and_then(|hex| parse_hex(hex))
    }

    fn to_json(&self) -> Value {
        let colors: Map<String, Value> = self
            .colors
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect();
        json!({"name": self.name, "dark": self.dark, "colors": colors})
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            name: v["name"].as_str()?.into(),
            dark: v["dark"].as_bool().unwrap_or(false),
            colors: v["colors"]
                .as_object()?
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect(),
        })
    }
}

/// System の色（macOS 版のシステムの色に近いもの。これまでの `diff::syntax_color` と同じ）。
fn system_color(kind: u8, dark: bool) -> Option<(f32, f32, f32)> {
    let pick =
        |light: (f32, f32, f32), dark_: (f32, f32, f32)| Some(if dark { dark_ } else { light });
    let pink = pick((0.80, 0.18, 0.45), (1.0, 0.45, 0.65));
    let red = pick((0.77, 0.16, 0.16), (1.0, 0.45, 0.40));
    let gray = pick((0.45, 0.45, 0.48), (0.60, 0.60, 0.64));
    let purple = pick((0.50, 0.25, 0.75), (0.78, 0.55, 1.0));
    let blue = pick((0.10, 0.40, 0.85), (0.40, 0.65, 1.0));
    let teal = pick((0.10, 0.55, 0.60), (0.35, 0.80, 0.85));
    let cyan = pick((0.10, 0.50, 0.70), (0.40, 0.80, 0.95));
    let orange = pick((0.80, 0.45, 0.05), (1.0, 0.65, 0.30));
    let brown = pick((0.55, 0.40, 0.25), (0.80, 0.65, 0.45));
    match kind {
        SyntaxSpan::KEYWORD | SyntaxSpan::TAG | SyntaxSpan::VARIABLE => pink,
        SyntaxSpan::STRING => red,
        SyntaxSpan::COMMENT => gray,
        SyntaxSpan::NUMBER | SyntaxSpan::CONSTANT => purple,
        SyntaxSpan::FUNCTION => blue,
        SyntaxSpan::TYPE => teal,
        SyntaxSpan::PROPERTY => cyan,
        SyntaxSpan::ATTRIBUTE | SyntaxSpan::ESCAPE => orange,
        SyntaxSpan::MODULE => brown,
        _ => None,
    }
}

/// `#rrggbb`（`#rrggbbaa` の透明度は使わない）から。
pub fn parse_hex(hex: &str) -> Option<(f32, f32, f32)> {
    let s = hex.trim().trim_start_matches('#');
    if !(s.len() == 6 || s.len() == 8) {
        return None;
    }
    let v = u32::from_str_radix(&s[..6], 16).ok()?;
    let c = |shift: u32| ((v >> shift) & 0xff) as f32 / 255.0;
    Some((c(16), c(8), c(0)))
}

/// `#rrggbb` か `#rrggbbaa`（透明度も）から。
pub fn parse_rgba(hex: &str) -> Option<gdk::RGBA> {
    let s = hex.trim().trim_start_matches('#');
    let (r, g, b) = parse_hex(hex)?;
    let a = match s.len() {
        8 => u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0,
        _ => 1.0,
    };
    Some(gdk::RGBA::new(r, g, b, a))
}

/// 内蔵のテーマの差分の色: そのテーマの緑・赤・青・灰・橙（`[added, removed, blue, gray, orange]`）に、標準の色と同じ
/// 濃さを付ける（macOS 版の `diffColors` と同じ）。
fn diff_colors([added, removed, blue, gray, orange]: [&str; 5]) -> Vec<(String, String)> {
    [
        (DiffColor::Added, added, "29"),
        (DiffColor::Removed, removed, "29"),
        (DiffColor::AddedWord, added, "61"),
        (DiffColor::RemovedWord, removed, "61"),
        (DiffColor::Hunk, blue, "1a"),
        (DiffColor::Current, blue, "2e"),
        (DiffColor::Incoming, added, "2e"),
        (DiffColor::Base, gray, "2e"),
        (DiffColor::Marker, orange, "24"),
    ]
    .into_iter()
    .map(|(kind, hex, alpha)| (kind.key().to_owned(), format!("{hex}{alpha}")))
    .collect()
}

/// VSCode の色（`#rgb`・`#rgba`・`#rrggbb`・`#rrggbbaa`）を `#rrggbb` か `#rrggbbaa` に。読めなければ `None`。
pub fn normalized_hex(value: &str) -> Option<String> {
    let s = value.trim().strip_prefix('#')?;
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match s.len() {
        3 | 4 => Some(format!(
            "#{}",
            s.chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_lowercase()
        )),
        6 | 8 => Some(format!("#{}", s.to_lowercase())),
        _ => None,
    }
}

/// `#rrggbb` に。
pub fn to_hex(rgba: &gdk::RGBA) -> String {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        c(rgba.red()),
        c(rgba.green()),
        c(rgba.blue())
    )
}

/// 内蔵のテーマ（どれも MIT などで公開された配色。macOS 版と同じ色）。
pub fn built_in() -> Vec<SyntaxTheme> {
    vec![
        SyntaxTheme::new(
            "Solarized Light",
            false,
            [
                "#859900", "#2aa198", "#93a1a1", "#d33682", "#268bd2", "#b58900", "#cb4b16",
                "#268bd2", "#b58900", "#268bd2", "#657b83", "#b58900", "#dc322f", "#6c71c4",
            ],
            ["#859900", "#dc322f", "#268bd2", "#93a1a1", "#cb4b16"],
        ),
        SyntaxTheme::new(
            "Solarized Dark",
            true,
            [
                "#859900", "#2aa198", "#586e75", "#d33682", "#268bd2", "#b58900", "#cb4b16",
                "#268bd2", "#b58900", "#268bd2", "#839496", "#b58900", "#dc322f", "#6c71c4",
            ],
            ["#859900", "#dc322f", "#268bd2", "#586e75", "#cb4b16"],
        ),
        SyntaxTheme::new(
            "Gruvbox Light",
            false,
            [
                "#9d0006", "#79740e", "#928374", "#8f3f71", "#79740e", "#b57614", "#8f3f71",
                "#076678", "#427b58", "#9d0006", "#af3a03", "#076678", "#af3a03", "#427b58",
            ],
            ["#79740e", "#9d0006", "#076678", "#928374", "#af3a03"],
        ),
        SyntaxTheme::new(
            "Gruvbox Dark",
            true,
            [
                "#fb4934", "#b8bb26", "#928374", "#d3869b", "#b8bb26", "#fabd2f", "#d3869b",
                "#83a598", "#8ec07c", "#fb4934", "#fe8019", "#83a598", "#fe8019", "#8ec07c",
            ],
            ["#b8bb26", "#fb4934", "#83a598", "#928374", "#fe8019"],
        ),
        SyntaxTheme::new(
            "Tomorrow",
            false,
            [
                "#8959a8", "#718c00", "#8e908c", "#f5871f", "#4271ae", "#c99e00", "#f5871f",
                "#c82829", "#f5871f", "#c82829", "#3e999f", "#c82829", "#f5871f", "#3e999f",
            ],
            ["#718c00", "#c82829", "#4271ae", "#8e908c", "#f5871f"],
        ),
        SyntaxTheme::new(
            "Tomorrow Night",
            true,
            [
                "#b294bb", "#b5bd68", "#969896", "#de935f", "#81a2be", "#f0c674", "#de935f",
                "#cc6666", "#de935f", "#cc6666", "#8abeb7", "#cc6666", "#de935f", "#8abeb7",
            ],
            ["#b5bd68", "#cc6666", "#81a2be", "#969896", "#de935f"],
        ),
        SyntaxTheme::new(
            "Nord",
            true,
            [
                "#81a1c1", "#a3be8c", "#616e88", "#b48ead", "#88c0d0", "#8fbcbb", "#b48ead",
                "#88c0d0", "#d08770", "#81a1c1", "#81a1c1", "#d8dee9", "#ebcb8b", "#8fbcbb",
            ],
            ["#a3be8c", "#bf616a", "#81a1c1", "#616e88", "#d08770"],
        ),
    ]
}

/// 読み込んだテーマ。
pub fn imported() -> Vec<SyntaxTheme> {
    settings::string(IMPORTED_SYNTAX_THEMES)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| {
            v.as_array()
                .map(|a| a.iter().filter_map(SyntaxTheme::from_json).collect())
        })
        .unwrap_or_default()
}

pub fn set_imported(themes: &[SyntaxTheme]) {
    let v = Value::Array(themes.iter().map(SyntaxTheme::to_json).collect());
    settings::set_string(IMPORTED_SYNTAX_THEMES, &v.to_string());
}

/// System、内蔵、読み込んだものの順。
pub fn all() -> Vec<SyntaxTheme> {
    let mut themes = vec![SyntaxTheme::system()];
    themes.extend(built_in());
    themes.extend(imported());
    themes
}

/// 外観ごとに選んだテーマの名前。
pub fn selected(dark: bool) -> String {
    settings::string(if dark {
        SYNTAX_THEME_DARK
    } else {
        SYNTAX_THEME_LIGHT
    })
    .unwrap_or_else(|| SYSTEM.into())
}

thread_local! {
    /// 選んだテーマ（明るい・暗い）と、差分を標準の色にするか。設定が変わるまで覚えておく（描くたびに JSON を
    /// 読まない）。
    static CURRENT: RefCell<Option<([SyntaxTheme; 2], bool)>> = const { RefCell::new(None) };
    /// テーマを変えた回数（差分の色付けした行のキャッシュを捨てる目印）。
    static GENERATION: Cell<u32> = const { Cell::new(0) };
}

/// テーマを変えた回数。色を付けた文字を覚えている部品は、変わっていたら作り直す。
pub fn generation() -> u32 {
    GENERATION.with(Cell::get)
}

/// 今の外観で選んだテーマで `f` を呼ぶ。`fg` は本文の色（明るければ暗い外観）。
fn with_current<T>(fg: &gdk::RGBA, f: impl FnOnce(&SyntaxTheme, bool, bool) -> T) -> T {
    let dark = fg.red() > 0.5;
    CURRENT.with(|c| {
        let mut c = c.borrow_mut();
        let (themes, standard) = c.get_or_insert_with(|| {
            let find = |dark: bool| {
                let name = selected(dark);
                all()
                    .into_iter()
                    .find(|t| t.name == name)
                    .unwrap_or_else(SyntaxTheme::system)
            };
            (
                [find(false), find(true)],
                settings::bool(DIFF_STANDARD_COLORS),
            )
        });
        f(&themes[usize::from(dark)], dark, *standard)
    })
}

/// 種類の色（`None` は本文の色のまま）。`fg` は本文の色（明るければ暗い外観）。
pub fn syntax_color(kind: u8, fg: &gdk::RGBA) -> Option<(f32, f32, f32)> {
    with_current(fg, |theme, dark, _| theme.color(kind, dark))
}

/// 差分の色を、アプリの今の外観（明るい・暗い）で（本文の色を知らない所から。マージエディタの欄など）。
pub fn diff_color_now(kind: DiffColor) -> gdk::RGBA {
    let fg = if adw::StyleManager::default().is_dark() {
        gdk::RGBA::WHITE
    } else {
        gdk::RGBA::BLACK
    };
    diff_color(kind, &fg)
}

/// 差分の色。今の外観で選んだテーマの色で、テーマが持たない色と、設定で標準の色にしたときは標準の色。
pub fn diff_color(kind: DiffColor, fg: &gdk::RGBA) -> gdk::RGBA {
    with_current(fg, |theme, _, standard| {
        (!standard)
            .then(|| theme.diff_color(kind))
            .flatten()
            .unwrap_or_else(|| kind.standard())
    })
}

/// 設定が変わったら、覚えたテーマを捨てて、すべての窓を描き直す（起動したときに一度呼ぶ）。
pub fn watch_settings() {
    settings::connect_changed(|key| {
        if matches!(
            key,
            SYNTAX_THEME_LIGHT | SYNTAX_THEME_DARK | IMPORTED_SYNTAX_THEMES | DIFF_STANDARD_COLORS
        ) {
            CURRENT.with(|c| c.borrow_mut().take());
            GENERATION.with(|g| g.set(g.get() + 1));
            for window in gtk::Window::list_toplevels() {
                redraw(&window);
            }
        }
    });
}

fn redraw(widget: &gtk::Widget) {
    widget.queue_draw();
    let mut child = widget.first_child();
    while let Some(c) = child {
        redraw(&c);
        child = c.next_sibling();
    }
}

/// VSCode の配色テーマの JSON（コメント・末尾のカンマあり、`include` で親を読む）から、種類ごとの色を拾う。
/// TextMate のスコープを種類に当てはめるので近似になる（macOS 版の `importVSCode` と同じ当てはめ）。
pub fn import_vscode(path: &Path) -> Result<SyntaxTheme, String> {
    let json = load_vscode(path, 0)?;
    let background = json["colors"]["editor.background"].as_str();
    let dark = match json["type"].as_str() {
        Some(t) => t != "light" && t != "hcLight",
        None => background
            .and_then(parse_hex)
            .map(|(r, g, b)| r.max(g).max(b) < 0.5)
            .unwrap_or(true),
    };
    // スコープ → 色（後に書いたものが勝つ）。
    let mut scopes: HashMap<String, String> = HashMap::new();
    for entry in json["tokenColors"].as_array().into_iter().flatten() {
        let Some(color) = entry["settings"]["foreground"].as_str() else {
            continue;
        };
        let names: Vec<String> = match &entry["scope"] {
            Value::Array(a) => a
                .iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect(),
            Value::String(s) => s.split(',').map(|s| s.trim().to_owned()).collect(),
            _ => Vec::new(),
        };
        // 「a b」（子孫の指定）は最後のスコープだけで見る。
        for name in names {
            let last = name.split(' ').next_back().unwrap_or("").to_owned();
            scopes.insert(last, color.chars().take(7).collect());
        }
    }
    // 種類ごとに、当てはめるスコープを優先の順に。
    let targets: [(&str, &[&str]); 14] = [
        (
            "keyword",
            &["keyword.control", "keyword", "storage.modifier", "storage"],
        ),
        ("string", &["string.quoted", "string"]),
        ("comment", &["comment.line", "comment"]),
        ("number", &["constant.numeric"]),
        (
            "function",
            &[
                "entity.name.function",
                "support.function",
                "meta.function-call",
            ],
        ),
        (
            "type",
            &[
                "entity.name.type",
                "support.type",
                "storage.type",
                "entity.name.class",
            ],
        ),
        (
            "constant",
            &["constant.language", "variable.other.constant", "constant"],
        ),
        (
            "property",
            &[
                "variable.other.property",
                "support.type.property-name",
                "variable.other.object.property",
            ],
        ),
        (
            "attribute",
            &["entity.other.attribute-name", "meta.attribute"],
        ),
        ("tag", &["entity.name.tag"]),
        ("operator", &["keyword.operator"]),
        ("variable", &["variable.other", "variable"]),
        ("escape", &["constant.character.escape"]),
        (
            "module",
            &[
                "entity.name.namespace",
                "entity.name.module",
                "support.module",
                "entity.name.type.module",
            ],
        ),
    ];
    let mut colors = HashMap::new();
    for (kind, wanted) in targets {
        for target in wanted {
            // いちばん詳しく当たるスコープ（target そのものか、その親）。
            let hit = scopes
                .keys()
                .filter(|s| *target == s.as_str() || target.starts_with(&format!("{s}.")))
                .max_by_key(|s| s.len());
            if let Some(hit) = hit {
                colors.insert(kind.to_owned(), scopes[hit].clone());
                break;
            }
        }
    }
    // 差分の色（`colors` にあるものだけ。なければ標準の色）。行の地がなければ、変わった文字の地を使う（VSCode と
    // 同じ）。
    let ui = &json["colors"];
    let hex = |key: &str| ui[key].as_str().and_then(normalized_hex);
    let diff = [
        (
            DiffColor::Added,
            hex("diffEditor.insertedLineBackground")
                .or_else(|| hex("diffEditor.insertedTextBackground")),
        ),
        (
            DiffColor::Removed,
            hex("diffEditor.removedLineBackground")
                .or_else(|| hex("diffEditor.removedTextBackground")),
        ),
        (
            DiffColor::AddedWord,
            hex("diffEditor.insertedTextBackground"),
        ),
        (
            DiffColor::RemovedWord,
            hex("diffEditor.removedTextBackground"),
        ),
        (DiffColor::Current, hex("merge.currentContentBackground")),
        (DiffColor::Incoming, hex("merge.incomingContentBackground")),
        (DiffColor::Base, hex("merge.commonContentBackground")),
    ];
    for (kind, value) in diff {
        if let Some(value) = value {
            colors.insert(kind.key().to_owned(), value);
        }
    }
    let name = json["name"].as_str().map(str::to_owned).unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    Ok(SyntaxTheme { name, dark, colors })
}

fn load_vscode(path: &Path, depth: u32) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let object: Value = serde_json::from_str(&strip_jsonc(&text)).map_err(|e| e.to_string())?;
    let Value::Object(object) = object else {
        return Err("not a JSON object".into());
    };
    let Some(include) = object.get("include").and_then(Value::as_str) else {
        return Ok(Value::Object(object));
    };
    if depth >= 4 {
        return Ok(Value::Object(object));
    }
    // 親を先に、子で上書きする（tokenColors はつなげ、colors は合わせる）。
    let parent = path.parent().unwrap_or(Path::new(".")).join(include);
    let mut merged = match load_vscode(&parent, depth + 1)? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    for (key, value) in object {
        if key == "include" {
            continue;
        }
        match (key.as_str(), merged.get_mut(&key), value) {
            ("tokenColors", Some(Value::Array(parent)), Value::Array(mine)) => parent.extend(mine),
            ("colors", Some(Value::Object(parent)), Value::Object(mine)) => parent.extend(mine),
            (_, _, value) => {
                merged.insert(key, value);
            }
        }
    }
    Ok(Value::Object(merged))
}

/// JSONC（`//`・`/* */` のコメントと、閉じる前のカンマ）をふつうの JSON にする。文字列の中は触らない。
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if in_string {
            out.push(c);
            if c == '\\' && i < chars.len() {
                out.push(chars[i]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.get(i) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i) == Some(&'*') => {
                i += 1;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                // 次の空白でない文字が閉じかっこなら落とす。
                let next = chars[i..].iter().find(|c| !c.is_whitespace());
                if !matches!(next, Some('}' | ']')) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_trailing_commas_outside_strings() {
        let text = "{\n // c\n \"a\": \"x // y\", /* b */ \"c\": [1, 2,],\n}";
        let v: Value = serde_json::from_str(&strip_jsonc(text)).unwrap();
        assert_eq!(v["a"], "x // y");
        assert_eq!(v["c"], json!([1, 2]));
    }

    #[test]
    fn imports_scopes_with_include() {
        let dir = std::env::temp_dir().join(format!("amonhen-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("base.json"),
            r##"{"type": "dark", "tokenColors": [
                {"scope": "keyword", "settings": {"foreground": "#111111"}},
                {"scope": ["string", "comment"], "settings": {"foreground": "#222222"}},
            ]}"##,
        )
        .unwrap();
        std::fs::write(
            dir.join("child.json"),
            r##"{"name": "Child", "include": "./base.json", // parent
                "tokenColors": [{"scope": "keyword.control, entity.name.function", "settings": {"foreground": "#333333ff"}}]}"##,
        )
        .unwrap();
        let theme = import_vscode(&dir.join("child.json")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(theme.name, "Child");
        assert!(theme.dark);
        assert_eq!(theme.colors["keyword"], "#333333");
        assert_eq!(theme.colors["function"], "#333333");
        assert_eq!(theme.colors["string"], "#222222");
        assert_eq!(theme.colors["comment"], "#222222");
        assert!(!theme.colors.contains_key("number"));
    }

    #[test]
    fn built_in_themes_cover_every_kind() {
        let diff = [
            DiffColor::Added,
            DiffColor::Removed,
            DiffColor::AddedWord,
            DiffColor::RemovedWord,
            DiffColor::Hunk,
            DiffColor::Current,
            DiffColor::Incoming,
            DiffColor::Base,
            DiffColor::Marker,
        ];
        for theme in built_in() {
            for (_, kind) in KINDS {
                assert!(theme.color(kind, theme.dark).is_some(), "{}", theme.name);
            }
            // 差分の色も、標準の色と同じ濃さで持つ。
            for kind in diff {
                let color = theme
                    .diff_color(kind)
                    .unwrap_or_else(|| panic!("{} {:?}", theme.name, kind));
                assert!(
                    (color.alpha() - kind.standard().alpha()).abs() < 0.05,
                    "{} {:?}",
                    theme.name,
                    kind
                );
            }
        }
    }

    #[test]
    fn imports_diff_colors_and_falls_back_to_the_text_background() {
        let dir = std::env::temp_dir().join(format!("amonhen-theme-diff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("t.json"),
            r##"{"type": "light", "colors": {
                "diffEditor.insertedTextBackground": "#9bb95533",
                "diffEditor.removedLineBackground": "#f00a",
                "diffEditor.removedTextBackground": "#ff000066",
                "merge.currentContentBackground": "#40c8ae80",
                "merge.incomingContentBackground": "not a color"
            }}"##,
        )
        .unwrap();
        let theme = import_vscode(&dir.join("t.json")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        // 行の地がなければ、変わった文字の地。短い書き方は伸ばす。
        assert_eq!(theme.colors["diffAdded"], "#9bb95533");
        assert_eq!(theme.colors["diffAddedWord"], "#9bb95533");
        assert_eq!(theme.colors["diffRemoved"], "#ff0000aa");
        assert_eq!(theme.colors["diffRemovedWord"], "#ff000066");
        assert_eq!(theme.colors["conflictCurrent"], "#40c8ae80");
        // 読めない色と、ない色は持たない（標準の色になる）。
        assert!(!theme.colors.contains_key("conflictIncoming"));
        assert!(!theme.colors.contains_key("diffHunk"));
        let alpha = theme.diff_color(DiffColor::Removed).unwrap().alpha();
        assert!((alpha - 0xaa as f32 / 255.0).abs() < 0.001);
        assert_eq!(theme.diff_color(DiffColor::Marker), None);
    }
}
