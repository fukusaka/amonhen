//! 構文と差分の色のテーマ（GTK4 版の `theme.rs`、macOS 版の `SyntaxColors.swift`）。Settings > Theme で選ぶ。
//! 外観（明るい・暗い）ごとに選んだテーマ（`syntaxThemeLight`・`syntaxThemeDark`）を使う。System はこれまでの色。
//! 差分の色は「差分は標準の色」（`diffStandardColors`）なら、テーマによらず標準の色。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use amonhen_layout::SyntaxSpan;
use serde_json::{Map, Value, json};

pub const SYNTAX_THEME_LIGHT: &str = "syntaxThemeLight";
pub const SYNTAX_THEME_DARK: &str = "syntaxThemeDark";
pub const DIFF_STANDARD_COLORS: &str = "diffStandardColors";
/// 読み込んだ VSCode のテーマ（JSON。形は GTK4 版・macOS 版と同じ）。
pub const IMPORTED_SYNTAX_THEMES: &str = "importedSyntaxThemes";
/// System（これまでの色）の名前。
pub const SYSTEM: &str = "System";

/// 差分の色の種類（GTK4 版の `DiffColor` のうち、この FE が描くもの）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffColor {
    Added,
    Removed,
    AddedWord,
    RemovedWord,
    Hunk,
}

impl DiffColor {
    fn key(self) -> &'static str {
        match self {
            DiffColor::Added => "diffAdded",
            DiffColor::Removed => "diffRemoved",
            DiffColor::AddedWord => "diffAddedWord",
            DiffColor::RemovedWord => "diffRemovedWord",
            DiffColor::Hunk => "diffHunk",
        }
    }

    /// 標準の色（これまでの色）。
    fn standard(self) -> (f32, f32, f32, f32) {
        match self {
            DiffColor::Added => (0.2, 0.7, 0.3, 0.16),
            DiffColor::Removed => (0.9, 0.25, 0.25, 0.16),
            DiffColor::AddedWord => (0.2, 0.7, 0.3, 0.35),
            DiffColor::RemovedWord => (0.9, 0.25, 0.25, 0.35),
            DiffColor::Hunk => (0.3, 0.5, 0.9, 0.10),
        }
    }
}

/// 構文の色のテーマ。種類（`keyword` など）→ `#rrggbb`、差分の色（鍵 → `#rrggbbaa`）。
#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxTheme {
    pub name: String,
    pub dark: bool,
    pub colors: HashMap<String, String>,
}

/// 種類の名前と `SyntaxSpan` の定数（GTK4 版の `KINDS` と同じ並び）。
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

/// 読み込んだテーマ。
pub fn imported() -> Vec<SyntaxTheme> {
    crate::prefs::string(IMPORTED_SYNTAX_THEMES)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| {
            v.as_array()
                .map(|a| a.iter().filter_map(SyntaxTheme::from_json).collect())
        })
        .unwrap_or_default()
}

pub fn set_imported(themes: &[SyntaxTheme]) {
    let v = Value::Array(themes.iter().map(SyntaxTheme::to_json).collect());
    let _ = crate::prefs::set_string(IMPORTED_SYNTAX_THEMES, &v.to_string());
}

/// 内蔵と読み込んだテーマ（System を除く）。
fn all() -> Vec<SyntaxTheme> {
    let mut themes = built_in();
    themes.extend(imported());
    themes
}

/// 内蔵のテーマの差分の色: 緑・赤・青・灰・橙（`[added, removed, blue, gray, orange]`）に標準の色と同じ濃さを
/// 付ける（GTK4 版の `diff_colors` と同じ）。
fn diff_colors([added, removed, blue, _gray, _orange]: [&str; 5]) -> Vec<(String, String)> {
    [
        (DiffColor::Added, added, "29"),
        (DiffColor::Removed, removed, "29"),
        (DiffColor::AddedWord, added, "61"),
        (DiffColor::RemovedWord, removed, "61"),
        (DiffColor::Hunk, blue, "1a"),
    ]
    .into_iter()
    .map(|(kind, hex, alpha)| (kind.key().to_owned(), format!("{hex}{alpha}")))
    .collect()
}

/// `#rrggbb` か `#rrggbbaa` から（透明度がなければ 1）。
fn parse(hex: &str) -> Option<(f32, f32, f32, f32)> {
    let s = hex.trim().trim_start_matches('#');
    if !(s.len() == 6 || s.len() == 8) {
        return None;
    }
    let v = u32::from_str_radix(&s[..6], 16).ok()?;
    let c = |shift: u32| ((v >> shift) & 0xff) as f32 / 255.0;
    let a = if s.len() == 8 {
        f32::from(u8::from_str_radix(&s[6..8], 16).ok()?) / 255.0
    } else {
        1.0
    };
    Some((c(16), c(8), c(0), a))
}

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

/// 選べるテーマの名前（System、内蔵、読み込んだものの順）。
pub fn names() -> Vec<String> {
    std::iter::once(SYSTEM.to_owned())
        .chain(all().into_iter().map(|t| t.name))
        .collect()
}

/// 外観ごとに選んだテーマの名前。
pub fn selected(dark: bool) -> String {
    crate::prefs::string(key(dark)).unwrap_or_else(|| SYSTEM.into())
}

/// 外観ごとのテーマの設定の鍵。
pub fn key(dark: bool) -> &'static str {
    if dark {
        SYNTAX_THEME_DARK
    } else {
        SYNTAX_THEME_LIGHT
    }
}

/// 今の外観で選んだテーマ（System なら None）と、差分を標準の色にするか。設定か外観が変わるまで覚えておく。
static CURRENT: Mutex<Option<(Option<SyntaxTheme>, bool)>> = Mutex::new(None);
/// テーマを変えた回数（色を付けて取っておいた差分の行を捨てる目印）。
static GENERATION: AtomicU32 = AtomicU32::new(0);

pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

/// 設定を変えた: 覚えたテーマを捨てる。
pub fn reload() {
    if let Ok(mut c) = CURRENT.lock() {
        *c = None;
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

fn with_current<T>(f: impl FnOnce(Option<&SyntaxTheme>, bool) -> T) -> T {
    let mut c = CURRENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (theme, standard) = c.get_or_insert_with(|| {
        let name = selected(crate::palette::dark());
        (
            all().into_iter().find(|t| t.name == name),
            crate::prefs::bool(DIFF_STANDARD_COLORS),
        )
    });
    f(theme.as_ref(), *standard)
}

/// 構文の種類の色（テーマが System なら `system`、テーマがその種類を持たなければ None＝本文の色）。
pub fn syntax(
    kind: u8,
    system: impl FnOnce(u8) -> Option<(f32, f32, f32)>,
) -> Option<(f32, f32, f32)> {
    with_current(|theme, _| match theme {
        None => system(kind),
        Some(t) => {
            let name = KINDS.iter().find(|(_, k)| *k == kind)?.0;
            t.colors
                .get(name)
                .and_then(|h| parse(h))
                .map(|(r, g, b, _)| (r, g, b))
        }
    })
}

/// 差分の色（テーマが持たないときと、標準の色にしたときは標準の色）。
pub fn diff(kind: DiffColor) -> (f32, f32, f32, f32) {
    with_current(|theme, standard| {
        theme
            .filter(|_| !standard)
            .and_then(|t| t.colors.get(kind.key()))
            .and_then(|h| parse(h))
            .unwrap_or_else(|| kind.standard())
    })
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

/// VSCode の配色テーマの JSON（コメント・末尾のカンマあり、`include` で親を読む）から、種類ごとの色を拾う。
/// TextMate のスコープを種類に当てはめるので近似になる（macOS 版の `importVSCode` と同じ当てはめ）。
pub fn import_vscode(path: &Path) -> Result<SyntaxTheme, String> {
    let json = load_vscode(path, 0)?;
    let background = json["colors"]["editor.background"].as_str();
    let dark = match json["type"].as_str() {
        Some(t) => t != "light" && t != "hcLight",
        None => background
            .and_then(parse)
            .map(|(r, g, b, _)| r.max(g).max(b) < 0.5)
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
    fn built_in_themes_have_every_color() {
        for t in built_in() {
            for (kind, _) in KINDS {
                assert!(parse(&t.colors[kind]).is_some(), "{} {kind}", t.name);
            }
            let (_, _, _, a) = parse(&t.colors["diffAdded"]).unwrap();
            assert!((a - 0x29 as f32 / 255.0).abs() < 1e-6);
        }
        assert_eq!(names()[0], SYSTEM);
    }

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
        // ない色は持たない（標準の色になる）。
        assert!(!theme.colors.contains_key("diffHunk"));
        let (_, _, _, alpha) = parse(&theme.colors["diffRemoved"]).unwrap();
        assert!((alpha - 0xaa as f32 / 255.0).abs() < 0.001);
    }
}
