//! 構文と差分の色のテーマ（GTK4 版の `theme.rs`、macOS 版の `SyntaxColors.swift`）。Settings > Theme で選ぶ。
//! この FE は明るい外観だけなので、明るい外観のテーマ（`syntaxThemeLight`）を使う。System はこれまでの色。
//! 差分の色は「差分は標準の色」（`diffStandardColors`）なら、テーマによらず標準の色。

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use amonhen_layout::SyntaxSpan;

pub const SYNTAX_THEME_LIGHT: &str = "syntaxThemeLight";
pub const DIFF_STANDARD_COLORS: &str = "diffStandardColors";
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
const KINDS: [(&str, u8); 14] = [
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

/// 選べるテーマの名前（System、内蔵の順）。
pub fn names() -> Vec<String> {
    std::iter::once(SYSTEM.to_owned())
        .chain(built_in().into_iter().map(|t| t.name))
        .collect()
}

/// 選んだテーマの名前。
pub fn selected() -> String {
    crate::prefs::string(SYNTAX_THEME_LIGHT).unwrap_or_else(|| SYSTEM.into())
}

/// 選んだテーマ（System なら None）と、差分を標準の色にするか。設定を変えるまで覚えておく。
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
        let name = selected();
        (
            built_in().into_iter().find(|t| t.name == name),
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
}
