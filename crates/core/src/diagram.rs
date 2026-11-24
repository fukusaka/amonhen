//! Markdown の図（` ```mermaid `・` ```dot ` のブロック）を SVG に描く（History の「すべてのファイル」の整形表示）。
//! Mermaid を描くのは pele（Obsidian の Mermaid の描き手）を移した `amonhen-pele`、Graphviz の DOT（` ```dot `・
//! ` ```graphviz `・` ```gv `）は `amonhen-dot` で、どちらも `diagrams` フィーチャーのときだけ入る。DOT の図も pele と
//! 同じ CSS の変数で色を書くので、`colorize` で同じように色を当てる。
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
    /// 近いもので描いた図（DOT の HTML の表など）と、そのことを伝える一文（訳した文）。色は `Svg` と同じ。
    Noted(String, String),
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
    draw_with(language, text, zone, None)
}

/// `draw_in_zone` の、DOT の図の画像（ノードの `image`・表の `<IMG>`）を `images` から読むもの。
pub fn draw_with(
    language: &str,
    text: &str,
    zone: Option<Arc<dyn TimeZone>>,
    images: Option<&ImageSource>,
) -> Option<Drawn> {
    if language.eq_ignore_ascii_case("mermaid") {
        return draw_mermaid(text, zone);
    }
    // dot・graphviz・gv は `layout` の属性で配置を選ぶ。neato などの名前のフェンスはその配置が既定
    if DOT_FENCES.iter().any(|l| language.eq_ignore_ascii_case(l)) {
        return draw_dot(text, language, images);
    }
    None
}

/// Markdown のファイルの図が読む画像の出どころ: そのファイルのパス（リポジトリの根から、`docs/README.md`）と、
/// リポジトリの根からのパスで、そのファイルと同じコミットの中身を読む読み手。読めなければ None。図の中の画像は
/// `resolve_image` の決まりでパスにしてから読む（外へは読みに行かない）。
#[derive(Clone)]
pub struct ImageSource {
    pub from: String,
    pub read: Arc<ReadFile>,
}

/// リポジトリの根からのパスで、ファイルの中身を読む（読めなければ None）。
pub type ReadFile = dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync;

impl ImageSource {
    /// 1 つの Markdown のファイルで読むバイト数・回数の上限をつけたもの（図ごとの上限は `amonhen-dot`）。
    pub(crate) fn bounded(&self) -> ImageSource {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let read = self.read.clone();
        let bytes = Arc::new(AtomicUsize::new(0));
        let count = Arc::new(AtomicUsize::new(0));
        ImageSource {
            from: self.from.clone(),
            read: Arc::new(move |path: &str| {
                if count.fetch_add(1, Ordering::Relaxed) >= MAX_FILE_IMAGES
                    || bytes.load(Ordering::Relaxed) >= MAX_FILE_IMAGE_BYTES
                {
                    return None;
                }
                let data = read(path)?;
                bytes.fetch_add(data.len(), Ordering::Relaxed);
                Some(data)
            }),
        }
    }
}

/// 図に描く画像 1 枚のバイト数の上限（これより大きいものは読み手が渡さなくてよい）。
pub const MAX_IMAGE_BYTES: usize = 1024 * 1024;
#[cfg(feature = "diagrams")]
const _: () = assert!(MAX_IMAGE_BYTES == amonhen_dot::image::MAX_BYTES);

/// 1 つの Markdown のファイルの図が読む画像の回数とバイト数の上限。
const MAX_FILE_IMAGES: usize = 128;
const MAX_FILE_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// 図の中の画像のパス `src`（`img/a.png`）を、Markdown のファイル `from`（リポジトリの根から）のフォルダからの
/// 相対パスとして、リポジトリの根からのパスにする。読まないもの: 空・URL（`https://`・`file:`・`data:` など
/// 名前と `:` で始まるもの、Windows のドライブも）・絶対パス（`/`・`\`）・`\` を含むもの・根より上（`..` で
/// 出るもの）・制御文字を含むもの。
pub fn resolve_image(src: &str, from: &str) -> Option<String> {
    let src = src.trim();
    if src.is_empty()
        || src.starts_with('/')
        || src.contains('\\')
        || src.chars().any(char::is_control)
    {
        return None;
    }
    // `名前:` で始まるもの（URL の scheme・ドライブ）
    if let Some(i) = src.find(':') {
        let scheme = &src[..i];
        if !scheme.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return None;
        }
    }
    let mut parts: Vec<&str> = match from.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').filter(|p| !p.is_empty()).collect(),
        None => Vec::new(),
    };
    for part in src.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Graphviz の DOT として描くフェンスの言葉。
const DOT_FENCES: &[&str] = &[
    "dot",
    "graphviz",
    "gv",
    "neato",
    "fdp",
    "sfdp",
    "twopi",
    "circo",
    "osage",
    "patchwork",
];

/// Graphviz の DOT の図。Mermaid と同じく大きなスタックの別スレッドで描き、panic は図の誤りにする。
#[cfg(feature = "diagrams")]
fn draw_dot(text: &str, language: &str, images: Option<&ImageSource>) -> Option<Drawn> {
    let source = text.to_owned();
    let engine = amonhen_dot::engines::Engine::from_name(language)
        .unwrap_or(amonhen_dot::engines::Engine::Dot);
    let images = images.cloned();
    let drawn = std::thread::Builder::new()
        .name("dot".into())
        .stack_size(PELE_STACK)
        .spawn(move || {
            let loader = images.map(|images| {
                move |src: &str| {
                    let path = resolve_image(src, &images.from)?;
                    (images.read)(&path)
                }
            });
            amonhen_dot::render_with_images(
                &source,
                engine,
                loader.as_ref().map(|l| l as &amonhen_dot::image::Loader),
            )
        })
        .ok()?
        .join();
    Some(match drawn {
        Ok(Ok(d)) if d.notes.is_empty() => Drawn::Svg(d.svg, None),
        Ok(Ok(d)) => Drawn::Noted(d.svg, approximation_note(&d.notes)),
        Ok(Err(e)) => Drawn::Error(e.message),
        Err(_) => Drawn::Error(amonhen_model::tr!("This diagram couldn’t be drawn")),
    })
}

#[cfg(not(feature = "diagrams"))]
fn draw_dot(_text: &str, _language: &str, _images: Option<&ImageSource>) -> Option<Drawn> {
    None
}

/// 近いもので描いたことを伝える一文。
#[cfg(feature = "diagrams")]
fn approximation_note(notes: &[amonhen_dot::Note]) -> String {
    use amonhen_dot::Note;
    let reasons: Vec<String> = notes
        .iter()
        .map(|n| match n {
            Note::Images => {
                amonhen_model::tr!("images that can’t be read are drawn as empty frames")
            }
            Note::UnknownShapes => amonhen_model::tr!("unknown shapes are drawn as boxes"),
            Note::Ortho => amonhen_model::tr!("orthogonal edges are drawn as polylines"),
            Note::UnknownColors => {
                amonhen_model::tr!("unknown colors are drawn in the default color")
            }
            Note::ColorSchemes => {
                amonhen_model::tr!("unknown color schemes are drawn in the default color")
            }
            Note::OtherLayouts => {
                amonhen_model::tr!("unknown layout engines are drawn with the dot layout")
            }
            Note::LayoutOptions => amonhen_model::tr!("some layout options are approximated"),
        })
        .collect();
    amonhen_model::tr!(
        "This picture is an approximation: {reasons}.",
        reasons = amonhen_model::i18n::list(&reasons)
    )
}

#[cfg(feature = "diagrams")]
fn draw_mermaid(text: &str, zone: Option<Arc<dyn TimeZone>>) -> Option<Drawn> {
    // pele は 5 万文字（UTF-16）を超える文を読まずに断る。`supports` は前処理を長さの上限なしに通すので、
    // 何 MB ものコードブロックで止まらないよう、先に長さを見て pele の断りをそのまま見せる。
    // UTF-16 の数はバイト数の 3 分の 1 より少なくならない（3 バイトの文字が 1 つ）ので、上限の 3 倍を超えるバイト数
    // なら数えずに断る（何 MB もの文を数えると、デバッグビルドでは数百ミリ秒かかる）。
    let too_long = text.len() > MAX_SOURCE
        && (text.len() > MAX_SOURCE * 3 || text.encode_utf16().count() > MAX_SOURCE);
    if !too_long && !amonhen_pele::supports(text) {
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

/// pele が読む文の長さの上限（Mermaid の既定の maxTextSize。UTF-16 の数）。
#[cfg(feature = "diagrams")]
const MAX_SOURCE: usize = 50_000;

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
        Drawn::Svg(svg, _) | Drawn::Noted(svg, _) => Some(svg),
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
    colorize_impl(svg, vars, backdrop, false)
}

/// `colorize` と同じで、CoreSVG（macOS）向けに書く: 斜体の文字を傾け（CoreSVG は `font-style` を読まない）、
/// DOT の図の飾りの混ざる行を span ごとの `<text>` に分け、飾りの線を描く（`svgcss::inline_slanted`）。
pub fn colorize_for_coresvg(
    svg: &str,
    vars: &[(String, String)],
    backdrop: bool,
) -> Option<String> {
    colorize_impl(svg, vars, backdrop, true)
}

fn colorize_impl(
    svg: &str,
    vars: &[(String, String)],
    backdrop: bool,
    coresvg: bool,
) -> Option<String> {
    let vars: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let inline = if coresvg {
        crate::svgcss::inline_slanted
    } else {
        crate::svgcss::inline_with
    };
    if backdrop {
        let with = add_backdrop(svg)?;
        return inline(&with, &vars);
    }
    inline(svg, &vars)
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
    fn refuses_long_sources_without_reading_them() {
        // 5 MB の flowchart も、知らない種類も、読む前に長さで断る（時間は前処理を通さないので数ミリ秒）。
        for head in ["flowchart LR\n", "zenuml\n"] {
            let text = format!("{head}{}", "  A --> B\n".repeat(500_000));
            let started = std::time::Instant::now();
            match draw("mermaid", &text) {
                Some(Drawn::Error(message)) => assert!(message.contains("limit"), "{message}"),
                other => panic!("{other:?}"),
            }
            assert!(started.elapsed() < std::time::Duration::from_millis(500));
        }
        // ちょうど上限までは描く
        let text = format!("flowchart LR\n  A --> B\n%%{}\n", "x".repeat(50_000 - 26));
        assert_eq!(text.encode_utf16().count(), 50_000);
        assert!(matches!(draw("mermaid", &text), Some(Drawn::Svg(..))));
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

    /// 太字・斜体と、下線・色の混ざる DOT のラベル（どちらも 1 行）。
    #[cfg(feature = "diagrams")]
    const MIXED_DOT: [&str; 2] = [
        "digraph { a [label=<<b>bold</b> and <i>italic</i>>] }",
        "digraph { a [label=<plain <u>under</u> <font color=\"red\">red</font>>] }",
    ];

    #[cfg(feature = "diagrams")]
    #[test]
    fn dot_labels_flow_for_librsvg() {
        // librsvg は `<tspan>` の飾りも text-decoration も読むので、span の並ぶ行は 1 つの `<text>` に流す（書体が
        // 測りより広くても、span の間の空白はつぶれない）。CoreSVG だけの線と手がかりは渡さない
        for (src, words) in MIXED_DOT.into_iter().zip([
            &["bold", " and ", "italic"][..],
            &["plain ", "under", " ", "red"],
        ]) {
            let Some(Drawn::Svg(svg, None)) = draw("dot", src) else {
                panic!("not drawn");
            };
            let flat = colorize(&svg, &palette(Scheme::Pele, false, &theme()), false).unwrap();
            assert!(
                !flat.contains("data-coresvg-only") && !flat.contains("data-x"),
                "{flat}"
            );
            assert!(!flat.contains("skewX"), "{flat}");
            let labels: Vec<&str> = flat
                .split("<text")
                .skip(1)
                .filter(|t| t.contains("<tspan"))
                .collect();
            assert_eq!(labels.len(), 1, "{flat}");
            let label = labels[0].split("</text>").next().unwrap();
            // librsvg は `<tspan>` の両端の空白を縮めるので、空白を残させる
            assert!(label.contains("text-anchor=\"middle\""), "{flat}");
            assert!(label.contains("xml:space=\"preserve\""), "{flat}");
            assert!(!label.contains("<tspan x="), "{flat}");
            let spans: Vec<&str> = label
                .split("<tspan")
                .skip(1)
                .map(|t| t.split('>').nth(1).unwrap().split('<').next().unwrap())
                .collect();
            assert_eq!(spans, words, "{flat}");
        }
        let flat = |src: &str| {
            let Some(Drawn::Svg(svg, None)) = draw("dot", src) else {
                panic!("not drawn");
            };
            colorize(&svg, &palette(Scheme::Pele, false, &theme()), false).unwrap()
        };
        let a = flat(MIXED_DOT[0]);
        assert!(a.contains("font-weight=\"700\">bold</tspan>"), "{a}");
        assert!(a.contains("font-style=\"italic\">italic</tspan>"), "{a}");
        let b = flat(MIXED_DOT[1]);
        assert!(
            b.contains("text-decoration=\"underline\">under</tspan>"),
            "{b}"
        );
        assert!(b.contains("fill=\"#ff0000\">red</tspan>"), "{b}");
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn dot_labels_split_for_coresvg() {
        // CoreSVG は `<tspan>` の飾りも text-decoration も読まないので、span ごとの `<text>` に分け（左端は
        // `data-x`）、下線は線で描く。分けた `<text>` の両端の空白は描き手によって縮んだり残ったりするので、空白は
        // 描かずに位置の間に置く: どの `<text>` も空白で始まり・終わらず、間はちょうど空白 1 つ分
        use amonhen_pele::text::measurer::{default_measurer, style};
        let measure = |t: &str, st: u32| default_measurer().width(t, 14.0, st);
        for (src, words) in MIXED_DOT.into_iter().zip([
            [("bold", style::BOLD), ("and", 0), ("italic", 0)],
            [("plain", 0), ("under", 0), ("red", 0)],
        ]) {
            let Some(Drawn::Svg(svg, None)) = draw("dot", src) else {
                panic!("not drawn");
            };
            let flat =
                colorize_for_coresvg(&svg, &palette(Scheme::Pele, false, &theme()), false).unwrap();
            assert!(!flat.contains("<tspan"), "{flat}");
            assert!(
                !flat.contains("text-decoration") && !flat.contains("data-x"),
                "{flat}"
            );
            let texts: Vec<(&str, f64, &str)> = flat
                .split("<text")
                .skip(1)
                .map(|t| {
                    let x = t.split("x=\"").nth(1).unwrap().split('"').next().unwrap();
                    let body = t.split('>').nth(1).unwrap().split('<').next().unwrap();
                    (t.split('>').next().unwrap(), x.parse().unwrap(), body)
                })
                .collect();
            assert_eq!(
                texts.iter().map(|t| t.2).collect::<Vec<_>>(),
                words.iter().map(|w| w.0).collect::<Vec<_>>(),
                "{flat}"
            );
            for k in 1..texts.len() {
                let (word, st) = words[k - 1];
                let gap = texts[k].1 - texts[k - 1].1 - measure(word, st);
                assert!((gap - measure(" ", 0)).abs() < 0.05, "{gap} {flat}");
            }
            for (open, x, word) in &texts {
                assert!(open.contains("text-anchor=\"start\""), "{flat}");
                assert_eq!(
                    open.contains("font-weight=\"700\""),
                    *word == "bold",
                    "{flat}"
                );
                assert_eq!(open.contains("skewX"), *word == "italic", "{flat}");
                // 下線は文字の左端から文字の幅だけ
                let line = format!("<path d=\"M{x},");
                assert_eq!(flat.contains(&line), *word == "under", "{flat}");
            }
        }
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
    fn dot_blocks_are_drawn_and_colored() {
        let md = "```dot\ndigraph { a [label=\"A  b\"]; a -> b [color=red]; b -> c }\n```\n\n```GV\ngraph { x -- y }\n```\n";
        let blocks = crate::markdown::parse_in_zone(md, None, None);
        let crate::markdown::Block::Diagram {
            language,
            svg: Some(svg),
            note: None,
            error: None,
            ..
        } = &blocks[0]
        else {
            panic!("{blocks:?}");
        };
        assert_eq!(language, "dot");
        assert!(matches!(
            &blocks[1],
            crate::markdown::Block::Diagram { svg: Some(_), .. }
        ));
        // テーマの色は配色で、文が決めた色はそのまま
        let syntax = colorize(svg, &palette(Scheme::Syntax, false, &theme()), false).unwrap();
        assert!(!syntax.contains("var("), "{syntax}");
        assert!(syntax.contains("#aa0000") && syntax.contains("#ff0000"));
        let dark = colorize(svg, &palette(Scheme::Pele, true, &theme()), false).unwrap();
        assert!(dark.contains("#d0d0d0") && dark.contains("#ff0000"));
        // 続く空白を保つ
        assert!(syntax.contains("xml:space=\"preserve\""), "{syntax}");
        for language in ["graphviz", "Dot", "neato", "Circo"] {
            assert!(matches!(
                draw(language, "digraph { a -> b }"),
                Some(Drawn::Svg(_, None))
            ));
        }
        // フェンスの言葉は既定の配置で、`layout` が勝つ
        let Some(Drawn::Svg(lr, _)) = draw("dot", "digraph { rankdir=LR; a -> b }") else {
            panic!("not drawn");
        };
        let Some(Drawn::Svg(free, _)) = draw("neato", "digraph { rankdir=LR; a -> b }") else {
            panic!("not drawn");
        };
        let Some(Drawn::Svg(back, _)) = draw("neato", "digraph { layout=dot; rankdir=LR; a -> b }")
        else {
            panic!("not drawn");
        };
        assert_ne!(lr, free);
        assert_eq!(lr, back);
        assert!(matches!(draw("dot", "digraph {"), Some(Drawn::Error(_))));
        // グラデーションの塗りも残る
        let Some(Drawn::Svg(svg, _)) = draw(
            "dot",
            "digraph { a [style=filled, fillcolor=\"red:blue\"] }",
        ) else {
            panic!("not drawn");
        };
        let colored = colorize(&svg, &palette(Scheme::Pele, false, &theme()), false).unwrap();
        assert!(
            colored.contains("<linearGradient") && colored.contains("url(#"),
            "{colored}"
        );
    }

    #[test]
    fn image_paths_stay_in_the_repository() {
        let r = |src: &str, from: &str| resolve_image(src, from);
        assert_eq!(
            r("img/a.png", "docs/README.md").as_deref(),
            Some("docs/img/a.png")
        );
        assert_eq!(r("./a.png", "README.md").as_deref(), Some("a.png"));
        assert_eq!(
            r("../x/a.png", "docs/README.md").as_deref(),
            Some("x/a.png")
        );
        assert_eq!(r("a//b/./c.svg", "README.md").as_deref(), Some("a/b/c.svg"));
        // 根より上・絶対パス・URL・ドライブ・\\・制御文字・空
        assert_eq!(r("../a.png", "README.md"), None);
        assert_eq!(r("docs/../../a.png", "README.md"), None);
        assert_eq!(r("/etc/passwd", "README.md"), None);
        assert_eq!(r("https://example.com/a.png", "README.md"), None);
        assert_eq!(r("file:a.png", "README.md"), None);
        assert_eq!(r("data:image/png;base64,AAAA", "README.md"), None);
        assert_eq!(r("C:/a.png", "README.md"), None);
        assert_eq!(r("img\\a.png", "README.md"), None);
        assert_eq!(r("a\npng", "README.md"), None);
        assert_eq!(r("  ", "README.md"), None);
        assert_eq!(r(".", "README.md"), None);
        // `:` が名前の後でなければパス
        assert_eq!(
            r("img/a:b.png", "README.md").as_deref(),
            Some("img/a:b.png")
        );
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn dot_images_come_from_the_markdown_files_commit() {
        // 2×1 画素の PNG（頭だけ）
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&2u32.to_be_bytes());
        png.extend_from_slice(&1u32.to_be_bytes());
        let asked = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = asked.clone();
        let source = ImageSource {
            from: "docs/README.md".into(),
            read: Arc::new(move |path: &str| {
                seen.lock().unwrap().push(path.to_owned());
                (path == "docs/img/a.png").then(|| png.clone())
            }),
        };
        let md = "```dot\ndigraph { a [image=\"img/a.png\", label=\"\"]; b [image=\"../../secret.png\"]; c [image=\"/abs.png\"] }\n```\n";
        let blocks = crate::markdown::parse_with_images(md, None, None, Some(source.clone()));
        let crate::markdown::Block::Diagram {
            svg: Some(svg),
            note: Some(note),
            ..
        } = &blocks[0]
        else {
            panic!("{blocks:?}");
        };
        // 根より上・絶対パスは読み手に渡さない
        assert_eq!(*asked.lock().unwrap(), ["docs/img/a.png"]);
        assert!(note.contains("images"), "{note}");
        assert!(
            svg.contains("href=\"data:image/png;base64,iVBORw0KGgo"),
            "{svg}"
        );
        // 色を当てても残り、CoreSVG 向けには xlink:href も書く
        let vars = palette(Scheme::Pele, false, &theme());
        let colored = colorize(svg, &vars, false).unwrap();
        assert!(colored.contains("<image ") && colored.contains("href=\"data:image/png"));
        let core = colorize_for_coresvg(svg, &vars, false).unwrap();
        assert!(core.contains("xlink:href=\"data:image/png"), "{core}");
        // 読めれば注記はない。読み手がなければ今までどおり枠と注記
        let ok = "```dot\ndigraph { a [image=\"img/a.png\"] }\n```\n";
        let blocks = crate::markdown::parse_with_images(ok, None, None, Some(source));
        assert!(matches!(
            &blocks[0],
            crate::markdown::Block::Diagram {
                svg: Some(_),
                note: None,
                ..
            }
        ));
        let blocks = crate::markdown::parse_in_zone(ok, None, None);
        assert!(matches!(
            &blocks[0],
            crate::markdown::Block::Diagram { note: Some(_), .. }
        ));
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn dot_approximations_carry_a_note() {
        let md = "```dot\ndigraph { a [shape=none, label=<<table><tr><td><img src=\"x.png\"/></td></tr></table>>, color=nosuch] }\n```\n";
        let blocks = crate::markdown::parse_in_zone(md, None, None);
        let crate::markdown::Block::Diagram {
            svg: Some(_),
            note: Some(note),
            ..
        } = &blocks[0]
        else {
            panic!("{blocks:?}");
        };
        assert!(
            note.contains("images") && note.contains("unknown colors"),
            "{note}"
        );
        let json = serde_json::to_string(&blocks[0]).unwrap();
        assert!(json.contains("\"note\":"), "{json}");
        amonhen_model::i18n::with(amonhen_model::i18n::Lang::Ja, || {
            let Some(Drawn::Noted(_, note)) = draw("dot", "digraph { a [shape=blob] }") else {
                panic!("no note");
            };
            assert!(note.contains("箱"), "{note}");
        });
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
