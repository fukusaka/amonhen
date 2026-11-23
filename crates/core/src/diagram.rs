//! Markdown の図（` ```mermaid ` のブロック）を SVG に描く（History の「すべてのファイル」の整形表示）。描くのは
//! pele（Obsidian の Mermaid の描き手）を移した `amonhen-pele` で、`diagrams` フィーチャーのときだけ入る。
//!
//! 形と位置（pele の SVG）は Markdown を読むときに一度だけ作る。色は pele が CSS の変数（`--pele-*`）で決めるので、
//! 描くときに `colorize` で配色（構文の色・pele・Mermaid）の変数を当て、`svgcss` で属性に書き写して CoreSVG・
//! librsvg で描ける形にする（1 図 0.05〜0.3 ms）。テーマや明るい・暗いを切り替えても、色だけを当て直せばよい。

use std::collections::HashMap;
use std::sync::Arc;

/// 図（gantt）の地方時の時間帯。時間帯は OS のものなので FE が実装して渡す（None は UTC）。`diagrams` の
/// フィーチャーでは pele の `TimeZone` そのもので、ないときも同じ形の trait なので、FE はどちらでも同じに書ける。
#[cfg(feature = "diagrams")]
pub use amonhen_pele::TimeZone;

/// 時間帯（`diagrams` のないとき。pele の `TimeZone` と同じ形）。
#[cfg(not(feature = "diagrams"))]
pub trait TimeZone: Send + Sync {
    /// その時刻（1970 年からの UTC のミリ秒）での、地方時 − UTC（ミリ秒）。
    fn offset_ms(&self, utc_ms: f64) -> f64;
}

/// 描いた図。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drawn {
    /// pele の SVG（色は CSS の変数のまま。`colorize` で色を当てる）と、図の中で指定した Mermaid のテーマ
    /// （`%%{init: …}%%`・front matter の `config` の `theme`・`themeVariables`。なければ `None`）。
    Svg(String, Option<MermaidConfig>),
    /// 描けない（書き方の誤りなど）。理由は pele の言葉のまま（英語）。
    Error(String),
}

/// フェンスの言葉が `language` のブロックの中身 `text` を図にする。図の言葉でない・pele が読まない種類なら `None`
/// （FE はコードとして見せる）。
pub fn draw(language: &str, text: &str) -> Option<Drawn> {
    draw_in_zone(language, text, None)
}

/// `draw` の、gantt の日付をこの時間帯の地方時で数えるもの（None は UTC）。
pub fn draw_in_zone(language: &str, text: &str, zone: Option<Arc<dyn TimeZone>>) -> Option<Drawn> {
    if !language.eq_ignore_ascii_case("mermaid") {
        return None;
    }
    draw_mermaid(text, zone)
}

#[cfg(feature = "diagrams")]
fn draw_mermaid(text: &str, zone: Option<Arc<dyn TimeZone>>) -> Option<Drawn> {
    if !amonhen_pele::supports(text) {
        return None;
    }
    // 深い入れ子でスタックがあふれるとアプリごと落ちる（pele は JS の RangeError を「入れ子が深すぎる」と返すが、
    // Rust では捕まえられない）。図の文は 5 万文字までなので、大きなスタックの別スレッドで描けばあふれない。
    // スタックは使った分だけ取られる。pele の中で panic しても、アプリは落とさず図の誤りとして見せる。
    let source = text.to_owned();
    let rendered = std::thread::Builder::new()
        .name("pele".into())
        .stack_size(PELE_STACK)
        .spawn(move || {
            let options = amonhen_pele::RenderOptions {
                time_zone: zone,
                ..Default::default()
            };
            amonhen_pele::render_with(&source, &options)
                .map(|r| r.svg)
                .map_err(|e| e.message)
        })
        .ok()?
        .join();
    Some(match rendered {
        Ok(Ok(svg)) => Drawn::Svg(svg, own_config(text)),
        Ok(Err(message)) => Drawn::Error(message),
        Err(_) => Drawn::Error(amonhen_model::tr!("This diagram couldn’t be drawn")),
    })
}

/// pele を動かすスレッドのスタック（5 万文字の入れ子にも足りる大きさ）。
#[cfg(feature = "diagrams")]
const PELE_STACK: usize = 256 << 20;

/// 図の中で指定した Mermaid のテーマ（pele の前処理が読んだ config から）。
#[cfg(feature = "diagrams")]
fn own_config(text: &str) -> Option<MermaidConfig> {
    use amonhen_pele::preprocess::preprocess;
    use amonhen_pele::util::yaml::Value;
    let config = preprocess(text, None).ok()?.config;
    let theme = match config.get("theme") {
        Some(Value::Str(name)) => Some(name.trim().to_owned()),
        _ => None,
    };
    let mut variables = Vec::new();
    if let Some(Value::Obj(vars)) = config.get("themeVariables") {
        for (key, value) in vars.iter() {
            let value = match value {
                Value::Str(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Num(n) => n.to_string(),
                _ => continue,
            };
            variables.push((key.clone(), value));
        }
    }
    (theme.is_some() || !variables.is_empty()).then_some(MermaidConfig { theme, variables })
}

/// 設定の見本の図（入れ子の枠・図形・ラベルつきの矢印）。色は `colorize` で当てる。図を描けない作りなら `None`。
pub fn sample() -> Option<String> {
    match draw(
        "mermaid",
        "flowchart LR\n  subgraph Repository\n    A[Commit] --> B{Review}\n  end\n  B -->|merge| C[(main)]\n",
    )? {
        Drawn::Svg(svg, _) => Some(svg),
        Drawn::Error(_) => None,
    }
}

#[cfg(not(feature = "diagrams"))]
fn draw_mermaid(_text: &str, _zone: Option<Arc<dyn TimeZone>>) -> Option<Drawn> {
    None
}

/// Mermaid のテーマの指定（mermaid-cli の設定ファイル `{"theme": …, "themeVariables": {…}}` と同じ中身）。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MermaidConfig {
    /// 名前つきのテーマ（`default`・`base` など）。なければ `default`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// テーマ変数の上書き（名前 → 値）。
    #[serde(
        default,
        rename = "themeVariables",
        with = "pairs",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub variables: Vec<(String, String)>,
}

mod pairs {
    //! `themeVariables` を JSON のオブジェクトで読み書きする（値の文字以外は文字にする）。
    use serde::{Deserialize, Deserializer, Serializer, ser::SerializeMap};

    pub fn serialize<S: Serializer>(v: &[(String, String)], s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(v.len()))?;
        for (k, val) in v {
            map.serialize_entry(k, val)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
        let map = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        Ok(map
            .into_iter()
            .filter_map(|(k, v)| {
                let v = match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => return None,
                };
                Some((k, v))
            })
            .collect())
    }
}

impl MermaidConfig {
    /// mermaid-cli の設定ファイル（JSON）を読む。`themeVariables` だけのオブジェクトも受ける（テーマは base）。
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let obj = value
            .as_object()
            .ok_or_else(|| amonhen_model::tr!("A Mermaid theme file is a JSON object"))?;
        // 包みの形（{"mermaid": {...}} など）は見ない。theme・themeVariables がなければ、全体を変数とみる
        if obj.contains_key("theme") || obj.contains_key("themeVariables") {
            let mut config: Self = serde_json::from_value(serde_json::json!({
                "theme": obj.get("theme").cloned().unwrap_or(serde_json::Value::Null),
                "themeVariables": obj.get("themeVariables").cloned().unwrap_or_else(|| serde_json::json!({})),
            }))
            .map_err(|e| e.to_string())?;
            if config
                .theme
                .as_deref()
                .is_some_and(|t| MermaidTheme::from_name(t).is_none())
            {
                return Err(amonhen_model::tr!(
                    "Unknown Mermaid theme: {name}",
                    name = config.theme.take().unwrap_or_default()
                ));
            }
            Ok(config)
        } else {
            let config: Self =
                serde_json::from_value(serde_json::json!({ "themeVariables": value }))
                    .map_err(|e| e.to_string())?;
            Ok(Self {
                theme: Some("base".into()),
                ..config
            })
        }
    }

    /// pele の変数に。
    fn palette(&self) -> Vec<(String, String)> {
        let name = self
            .theme
            .as_deref()
            .and_then(MermaidTheme::from_name)
            .unwrap_or(MermaidTheme::Default);
        from_mermaid(
            &crate::mermaidtheme::variables_with(name.name(), &self.variables).unwrap_or_default(),
        )
    }
}

/// 図の配色（設定の「図の配色」）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    /// 構文の色と同じテーマから（既定）。
    #[default]
    Syntax,
    /// pele の既定の色（灰色を主にした色）。
    Pele,
    /// Mermaid 本家のテーマ。`None` は外観に合わせる（明るい表示は default、暗い表示は dark）。名前つきは外観に
    /// 関係なくそのテーマ（地も塗る）。
    Mermaid(Option<MermaidTheme>),
    /// 読み込んだ Mermaid のテーマ（設定の鍵は `mermaid-import:名前`、中身は FE が覚えている）。
    Imported,
}

/// 読み込んだ Mermaid のテーマを指す設定の鍵の接頭辞。
pub const IMPORTED_PREFIX: &str = "mermaid-import:";

/// Mermaid の名前つきテーマ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MermaidTheme {
    Default,
    Neutral,
    Dark,
    Forest,
    Base,
}

impl MermaidTheme {
    pub const ALL: [Self; 5] = [
        Self::Default,
        Self::Neutral,
        Self::Dark,
        Self::Forest,
        Self::Base,
    ];

    /// Mermaid での名前から。
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name.trim())
    }

    /// Mermaid での名前。
    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Neutral => "neutral",
            Self::Dark => "dark",
            Self::Forest => "forest",
            Self::Base => "base",
        }
    }
}

impl Scheme {
    /// 設定に書く名前。
    pub fn key(self) -> String {
        match self {
            Self::Syntax => "syntax".into(),
            Self::Pele => "pele".into(),
            Self::Mermaid(None) => "mermaid".into(),
            Self::Mermaid(Some(theme)) => format!("mermaid-{}", theme.name()),
            Self::Imported => IMPORTED_PREFIX.into(),
        }
    }

    /// 設定の名前から（知らない名前は既定）。
    pub fn from_key(key: &str) -> Self {
        match key {
            "pele" => Self::Pele,
            "mermaid" => Self::Mermaid(None),
            _ if key.starts_with(IMPORTED_PREFIX) => Self::Imported,
            _ => key
                .strip_prefix("mermaid-")
                .and_then(|name| MermaidTheme::ALL.into_iter().find(|t| t.name() == name))
                .map_or(Self::Syntax, |t| Self::Mermaid(Some(t))),
        }
    }

    /// 設定に出す順の、すべての配色。
    pub fn all() -> Vec<Self> {
        let mut all = vec![Self::Syntax, Self::Pele, Self::Mermaid(None)];
        all.extend(MermaidTheme::ALL.map(|t| Self::Mermaid(Some(t))));
        all
    }

    /// 図の地を塗るか（Mermaid のテーマは、外観と合わなくても読めるよう、テーマの地を敷く）。
    pub fn backdrop(self) -> bool {
        matches!(self, Self::Mermaid(_) | Self::Imported)
    }
}

/// 構文の色のテーマから配色を作るための色（`#rrggbb`）。`text`・`background` は画面の文字と地の色、ほかは構文の
/// 種類（`keyword` など、テーマの `colors` の鍵）→ 色。テーマが持たない種類は、FE がその外観の標準の色を入れる。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeColors {
    pub text: String,
    pub background: String,
    pub kinds: HashMap<String, String>,
}

/// pele の暗い表示の色（pele の既定は明るい表示のもの）。
const PELE_DARK: [(&str, &str); 9] = [
    ("--pele-bg", "#1e1e1e"),
    ("--pele-fg", "#e4e4e4"),
    ("--pele-muted", "#9a9a9a"),
    ("--pele-line", "#d0d0d0"),
    ("--pele-surface", "#2b2b2b"),
    ("--pele-surface-alt", "#363636"),
    ("--pele-border", "#6b6b6b"),
    ("--pele-accent", "#7b9be5"),
    ("--pele-fg-on-dark", "#ffffff"),
];

/// 系列（円グラフ・gitGraph などの 8 色）に使う構文の種類。
const SERIES: [&str; 8] = [
    "keyword",
    "string",
    "number",
    "function",
    "type",
    "constant",
    "attribute",
    "tag",
];

/// 配色の変数（`--pele-*` → 色）。`theme` は `Scheme::Syntax` のときだけ使う。
pub fn palette(scheme: Scheme, dark: bool, theme: &ThemeColors) -> Vec<(String, String)> {
    let own = |list: &[(&str, &str)]| {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    };
    match scheme {
        // 中身がわからない読み込んだテーマは、構文の色にする
        Scheme::Imported => palette(Scheme::Syntax, dark, theme),
        Scheme::Pele if dark => own(&PELE_DARK),
        Scheme::Pele => Vec::new(),
        Scheme::Mermaid(theme) => {
            let theme = theme.unwrap_or(if dark {
                MermaidTheme::Dark
            } else {
                MermaidTheme::Default
            });
            from_mermaid(&crate::mermaidtheme::variables(theme.name()).unwrap_or_default())
        }
        Scheme::Syntax => {
            let kind = |name: &str| {
                theme
                    .kinds
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| theme.text.clone())
            };
            let keyword = kind("keyword");
            let mut out = vec![
                ("--pele-bg".to_owned(), theme.background.clone()),
                ("--pele-fg".to_owned(), theme.text.clone()),
                ("--pele-line".to_owned(), theme.text.clone()),
                ("--pele-muted".to_owned(), kind("comment")),
                ("--pele-border".to_owned(), keyword.clone()),
                ("--pele-accent".to_owned(), kind("function")),
                (
                    "--pele-surface".to_owned(),
                    mix(&keyword, &theme.background, 0.12),
                ),
                (
                    "--pele-surface-alt".to_owned(),
                    mix(&kind("type"), &theme.background, 0.18),
                ),
                ("--pele-fg-on-dark".to_owned(), "#ffffff".to_owned()),
            ];
            for (i, name) in SERIES.iter().enumerate() {
                out.push((format!("--pele-series-{}", i + 1), kind(name)));
            }
            out
        }
    }
}

/// Mermaid のテーマの変数（`mermaidtheme::variables`）を pele の変数に。図形の面は mainBkg、枠は nodeBorder、
/// 入れ子の枠の面は clusterBkg、線は lineColor、文字は textColor、系列は pie1〜pie8。
fn from_mermaid(vars: &[(&'static str, String)]) -> Vec<(String, String)> {
    let get = |key: &str| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
    let background = get("background").unwrap_or_else(|| "#ffffff".into());
    // 半透明の色は地に重ねて不透明に（CoreSVG は #rrggbbaa を読まないことがある）
    let solid = |key: &str| get(key).map(|v| flatten(&v, &background));
    let text = solid("textColor").unwrap_or_else(|| "#333333".into());
    let mut out = vec![
        ("--pele-bg".to_owned(), flatten(&background, "#ffffff")),
        ("--pele-fg".to_owned(), text.clone()),
        ("--pele-muted".to_owned(), mix(&text, &background, 0.65)),
    ];
    for (pele, mermaid) in [
        ("--pele-line", "lineColor"),
        ("--pele-surface", "mainBkg"),
        ("--pele-surface-alt", "clusterBkg"),
        ("--pele-border", "nodeBorder"),
        ("--pele-accent", "primaryBorderColor"),
    ] {
        if let Some(v) = solid(mermaid) {
            out.push((pele.to_owned(), v));
        }
    }
    for i in 1..=8 {
        if let Some(v) = solid(&format!("pie{i}")) {
            out.push((format!("--pele-series-{i}"), v));
        }
    }
    out.push(("--pele-fg-on-dark".to_owned(), "#ffffff".to_owned()));
    out
}

/// `#rrggbbaa` を `base` に重ねた `#rrggbb`（不透明ならそのまま）。
fn flatten(color: &str, base: &str) -> String {
    let h = color.trim().trim_start_matches('#');
    if h.len() == 8
        && let Ok(alpha) = u8::from_str_radix(&h[6..8], 16)
    {
        return mix(&format!("#{}", &h[..6]), base, f64::from(alpha) / 255.0);
    }
    color.to_owned()
}

/// 図を描くときの配色の変数と、地を塗るか。図の中で Mermaid のテーマを指定していればそれ（設定より先）、なければ
/// 設定の配色 `scheme`。`imported` は `scheme` が読み込んだ Mermaid のテーマのとき、その中身。
pub fn colors_for(
    own: Option<&MermaidConfig>,
    scheme: Scheme,
    imported: Option<&MermaidConfig>,
    dark: bool,
    theme: &ThemeColors,
) -> (Vec<(String, String)>, bool) {
    if let Some(own) = own {
        return (own.palette(), true);
    }
    if let (Scheme::Imported, Some(imported)) = (scheme, imported) {
        return (imported.palette(), true);
    }
    (palette(scheme, dark, theme), scheme.backdrop())
}

/// pele の SVG `svg` に配色 `vars` を当て、CoreSVG・librsvg で描ける SVG にする。`backdrop` なら図の地を
/// `--pele-bg` で塗る。読めなければ `None`。
pub fn colorize(svg: &str, vars: &[(String, String)], backdrop: bool) -> Option<String> {
    let vars: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    if backdrop {
        let with = add_backdrop(svg)?;
        return crate::svgcss::inline_with(&with, &vars);
    }
    crate::svgcss::inline_with(svg, &vars)
}

/// 根の `<svg>` の中の先頭に、`viewBox` いっぱいの地の四角を足す。
fn add_backdrop(svg: &str) -> Option<String> {
    let start = svg.find("<svg")?;
    let end = start + svg[start..].find('>')?;
    let open = &svg[start..=end];
    let view_box = open.split("viewBox=\"").nth(1)?.split('"').next()?;
    let nums: Vec<&str> = view_box.split_ascii_whitespace().collect();
    let [x, y, w, h] = nums[..] else { return None };
    Some(format!(
        "{}<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"var(--pele-bg)\"/>{}",
        &svg[..=end],
        &svg[end + 1..]
    ))
}

/// `top` を `amount`（0〜1）だけ `base` に重ねた色。読めない色なら `top`。
fn mix(top: &str, base: &str, amount: f64) -> String {
    let (Some(a), Some(b)) = (rgb(top), rgb(base)) else {
        return top.to_owned();
    };
    let c = |i: usize| (f64::from(a[i]) * amount + f64::from(b[i]) * (1.0 - amount)).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(0), c(1), c(2))
}

fn rgb(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim().strip_prefix('#')?;
    let h = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 | 8 => h[..6].to_owned(),
        _ => return None,
    };
    let n = u32::from_str_radix(&h, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// いつも UTC+9 の時間帯（FE が渡すもののかわり）。
    #[cfg(feature = "diagrams")]
    struct Tokyo;

    #[cfg(feature = "diagrams")]
    impl TimeZone for Tokyo {
        fn offset_ms(&self, _utc_ms: f64) -> f64 {
            9.0 * 3_600_000.0
        }
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn counts_gantt_dates_in_the_given_zone() {
        let text = "gantt\n  dateFormat YYYY-MM-DD HH:mm\n  axisFormat %H:%M %Z\n  section S\n  Task :2024-01-01 22:00, 3h\n";
        let svg = |zone: Option<Arc<dyn TimeZone>>| match draw_in_zone("mermaid", text, zone) {
            Some(Drawn::Svg(svg, _)) => svg,
            other => panic!("{other:?}"),
        };
        let utc = svg(None);
        assert_eq!(utc, svg_of(draw("mermaid", text)));
        assert!(utc.contains("+0000"));
        assert!(svg(Some(Arc::new(Tokyo))).contains("+0900"));
        let blocks = crate::markdown::parse_in_zone(
            &format!("```mermaid\n{text}```\n"),
            None,
            Some(Arc::new(Tokyo)),
        );
        assert!(
            matches!(&blocks[0], crate::markdown::Block::Diagram { svg: Some(s), .. } if s.contains("+0900"))
        );
    }

    #[cfg(feature = "diagrams")]
    fn svg_of(drawn: Option<Drawn>) -> String {
        match drawn {
            Some(Drawn::Svg(svg, _)) => svg,
            other => panic!("{other:?}"),
        }
    }

    fn theme() -> ThemeColors {
        ThemeColors {
            text: "#202020".into(),
            background: "#ffffff".into(),
            kinds: [
                ("keyword", "#aa0000"),
                ("comment", "#808080"),
                ("function", "#0000aa"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        }
    }

    #[test]
    fn syntax_palette_comes_from_the_theme() {
        let vars: HashMap<_, _> = palette(Scheme::Syntax, false, &theme())
            .into_iter()
            .collect();
        assert_eq!(vars["--pele-border"], "#aa0000");
        assert_eq!(vars["--pele-muted"], "#808080");
        assert_eq!(vars["--pele-accent"], "#0000aa");
        assert_eq!(vars["--pele-surface"], "#f5e0e0");
        // テーマが持たない種類は文字の色
        assert_eq!(vars["--pele-series-2"], "#202020");
        assert!(palette(Scheme::Pele, false, &theme()).is_empty());
        assert_eq!(Scheme::from_key("mermaid"), Scheme::Mermaid(None));
        assert_eq!(
            Scheme::from_key("mermaid-forest"),
            Scheme::Mermaid(Some(MermaidTheme::Forest))
        );
        for scheme in Scheme::all() {
            assert_eq!(Scheme::from_key(&scheme.key()), scheme);
        }
        assert_eq!(Scheme::from_key("?"), Scheme::Syntax);
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn flowcharts_are_drawn_and_colored() {
        let Some(Drawn::Svg(svg, None)) = draw("mermaid", "flowchart LR\n  A[Start] --> B[End]\n")
        else {
            panic!("not drawn");
        };
        assert!(svg.contains("var(--"));
        let light = colorize(&svg, &palette(Scheme::Pele, false, &theme()), false).unwrap();
        let mermaid =
            colorize(&svg, &palette(Scheme::Mermaid(None), false, &theme()), true).unwrap();
        let syntax = colorize(&svg, &palette(Scheme::Syntax, false, &theme()), false).unwrap();
        assert!(!light.contains("var(") && light.contains("#f3f3f3"));
        assert!(mermaid.to_lowercase().contains("#ececff"), "{mermaid}");
        assert!(mermaid.contains("<rect x=\"0\" y=\"0\""), "{mermaid}");
        assert!(syntax.contains("#f5e0e0") && syntax.contains("#aa0000"));
        assert!(light.contains(">Start</text>"));
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn a_diagram_can_name_its_own_theme() {
        let text = "%%{init: {\"theme\": \"forest\", \"themeVariables\": {\"lineColor\": \"#ff0000\"}}}%%\nflowchart LR\n  A --> B\n";
        let Some(Drawn::Svg(svg, Some(own))) = draw("mermaid", text) else {
            panic!("no theme read");
        };
        assert_eq!(own.theme.as_deref(), Some("forest"));
        assert_eq!(
            own.variables,
            [("lineColor".to_owned(), "#ff0000".to_owned())]
        );
        let (vars, backdrop) = colors_for(Some(&own), Scheme::Syntax, None, true, &theme());
        assert!(backdrop);
        let vars: HashMap<_, _> = vars.into_iter().collect();
        assert_eq!(vars["--pele-line"], "#ff0000");
        assert_eq!(vars["--pele-surface"], "#cde498");
        let front = "---\nconfig:\n  theme: neutral\n---\nflowchart LR\n  A --> B\n";
        let Some(Drawn::Svg(_, Some(own))) = draw("mermaid", front) else {
            panic!("no front matter theme");
        };
        assert_eq!(own.theme.as_deref(), Some("neutral"));
        assert!(colorize(&svg, &vars.into_iter().collect::<Vec<_>>(), true).is_some());
    }

    #[test]
    fn theme_files_are_mermaid_cli_configs() {
        let c = MermaidConfig::parse(r##"{"theme": "base", "themeVariables": {"primaryColor": "#ff0000", "darkMode": true}}"##).unwrap();
        assert_eq!(c.theme.as_deref(), Some("base"));
        assert!(
            c.variables
                .contains(&("darkMode".to_owned(), "true".to_owned()))
        );
        let bare = MermaidConfig::parse(r##"{"primaryColor": "#00ff00"}"##).unwrap();
        assert_eq!(bare.theme.as_deref(), Some("base"));
        assert!(MermaidConfig::parse(r#"{"theme": "nope"}"#).is_err());
        assert!(MermaidConfig::parse("[1]").is_err());
        let round: MermaidConfig =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(round, c);
        let (vars, _) = colors_for(None, Scheme::Imported, Some(&c), false, &theme());
        let vars: HashMap<_, _> = vars.into_iter().collect();
        assert_eq!(vars["--pele-surface"], "#ff0000");
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn the_settings_sample_is_drawn() {
        let svg = sample().unwrap();
        assert!(
            svg.contains(">Repository</text>") && svg.contains(">merge</text>"),
            "{svg}"
        );
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn other_languages_and_unknown_kinds_stay_code() {
        assert_eq!(draw("rust", "fn main() {}"), None);
        // pele が読まない種類（zenuml など）はコードのまま
        assert_eq!(draw("mermaid", "zenuml\n  A.method()\n"), None);
        assert!(matches!(
            draw("mermaid", "sequenceDiagram\n  A->>B: hi\n"),
            Some(Drawn::Svg(_, None))
        ));
        assert!(matches!(
            draw("Mermaid", "flowchart LR\n  A -->"),
            Some(Drawn::Error(_))
        ));
    }
}
