//! Mermaid の名前つきテーマ（default・neutral・dark・forest・base）の色を、Mermaid と同じ計算で出す。図を
//! Mermaid のテーマの配色で塗るのに使う。
//!
//! 移したもと:
//! - Mermaid（develop ブランチ）の `packages/mermaid/src/themes/theme-{default,neutral,dark,forest,base}.js`・
//!   `theme-helpers.js`（`mkBorder`）。Copyright (c) 2014 - 2022 Knut Sveidqvist
//! - 色の計算は khroma 2.1.0（Mermaid が使う色の小さなライブラリ）の、テーマが使う分（読み取り・RGB と HSL の
//!   変換・`change`・`adjust`・`lighten`・`darken`・`invert`・`rgba`）。Copyright (c) 2019-present Fabio
//!   Spampinato, Andrew Maney
//!
//! どちらも MIT ライセンス:
//!
//! > Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
//! > associated documentation files (the "Software"), to deal in the Software without restriction, including
//! > without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! > copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the
//! > following conditions:
//! >
//! > The above copyright notice and this permission notice shall be included in all copies or substantial
//! > portions of the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
//! > LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
//! > EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
//! > IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
//! > USE OR OTHER DEALINGS IN THE SOFTWARE.
//!
//! khroma は途中の色を文字（`#rrggbb`・`rgb(…)`・`hsl(…)`）で受け渡し、文字にするときに小数を 10 桁で丸める。
//! 丸めの境目で結果が変わらないように、ここでも同じ形の文字を作って読み直す。テーマの計算は Mermaid の
//! 順番どおり（後の代入が勝ち、`x || y` は先の値を残す）。利用者の上書きはなく、`darkMode` はどのテーマも
//! 既定の未設定（偽）。

use std::collections::HashMap;

/// 返す変数（Mermaid の名前）。この順に並べる。
const KEYS: &[&str] = &[
    "background",
    "primaryColor",
    "secondaryColor",
    "tertiaryColor",
    "primaryBorderColor",
    "secondaryBorderColor",
    "tertiaryBorderColor",
    "primaryTextColor",
    "secondaryTextColor",
    "tertiaryTextColor",
    "textColor",
    "titleColor",
    "lineColor",
    "arrowheadColor",
    "mainBkg",
    "nodeBorder",
    "clusterBkg",
    "clusterBorder",
    "edgeLabelBackground",
    "noteBkgColor",
    "noteBorderColor",
    "noteTextColor",
    "pie1",
    "pie2",
    "pie3",
    "pie4",
    "pie5",
    "pie6",
    "pie7",
    "pie8",
    "pieTitleTextColor",
    "pieSectionTextColor",
    "pieStrokeColor",
    "pieOuterStrokeColor",
    "git0",
    "git1",
    "git2",
    "git3",
    "git4",
    "git5",
    "git6",
    "git7",
];

/// Mermaid の名前つきテーマ（default・neutral・dark・forest・base）の、計算したあとの色。知らない名前なら None。
/// 色は `#rrggbb`（不透明でなければ `#rrggbbaa`）。
pub fn variables(theme: &str) -> Option<Vec<(&'static str, String)>> {
    variables_with(theme, &[])
}

/// `theme` に、Mermaid の themeVariables の上書き `overrides`（名前 → 値。色は Mermaid が読む書き方のまま）を当てたあとの色。
///
/// Mermaid の `getThemeVariables(overrides)` → `calculate(overrides)` と同じく、テーマを作ってから上書きを置き、
/// `updateColors` を回し、もう一度上書きを置く（上書きした変数は必ずその値になる）。`darkMode` は `"true"` で真。
/// khroma が読めない値（フォントの名前など）と、ここで扱わない名前は読み捨てる。
pub fn variables_with(
    theme: &str,
    overrides: &[(String, String)],
) -> Option<Vec<(&'static str, String)>> {
    let (constructor, update): (Step, Step) = match theme {
        "default" => (default_constructor, default_update),
        "neutral" => (neutral_constructor, neutral_update),
        "dark" => (dark_constructor, dark_update),
        "forest" => (forest_constructor, forest_update),
        "base" => (base_constructor, base_update),
        _ => return None,
    };
    let overrides: Vec<(&str, &str)> = overrides
        .iter()
        .map(|(key, value)| (key.as_str(), value.trim()))
        .filter(|(key, value)| *key == "darkMode" || khroma::is_valid(value))
        .collect();
    let mut t = Theme::default();
    constructor(&mut t);
    // calculate(overrides)
    for (key, value) in &overrides {
        t.set(key, *value);
    }
    update(&mut t);
    for (key, value) in &overrides {
        t.set(key, *value);
    }
    Some(
        KEYS.iter()
            .map(|&key| {
                let value = t.vars.get(key).map_or_else(
                    || panic!("mermaid theme {theme}: {key} is not set"),
                    |value| khroma::to_hex(value),
                );
                (key, value)
            })
            .collect(),
    )
}

use khroma::Ch::{H, L, S};
use khroma::{adjust, darken, invert, lighten};

/// テーマのコンストラクタ・`updateColors`。
type Step = fn(&mut Theme);

/// テーマの `this`（変数の名前 → khroma の文字）。ない変数は JavaScript の undefined。
#[derive(Default)]
struct Theme {
    vars: HashMap<String, String>,
}

impl Theme {
    /// `this.key`（なければ空。空は JavaScript で偽）。
    fn get(&self, key: &str) -> String {
        self.vars.get(key).cloned().unwrap_or_default()
    }

    fn set(&mut self, key: &str, value: impl Into<String>) {
        self.vars.insert(key.to_string(), value.into());
    }

    /// `this.key = this.key || value(this)`。
    fn or(&mut self, key: &str, value: impl FnOnce(&Self) -> String) {
        if self.get(key).is_empty() {
            let value = value(self);
            self.set(key, value);
        }
    }

    /// `this.darkMode`（上書きの `"true"` だけが真。どのテーマも既定では置かない）。
    fn dark(&self) -> bool {
        self.get("darkMode").eq_ignore_ascii_case("true")
    }
}

/// `theme-helpers.js` の `mkBorder`。
fn mk_border(color: &str, dark: bool) -> String {
    if dark {
        adjust(color, &[(S, -40.0), (L, 10.0)])
    } else {
        adjust(color, &[(S, -40.0), (L, -10.0)])
    }
}

const GITS: [&str; 8] = [
    "git0", "git1", "git2", "git3", "git4", "git5", "git6", "git7",
];

/// default・forest・base の git: `this.gitN = this.gitN || …` のあと、`darkMode` なら lighten、でなければ darken。
fn git_colors(t: &mut Theme) {
    let seed: [fn(&Theme) -> String; 8] = [
        |t| t.get("primaryColor"),
        |t| t.get("secondaryColor"),
        |t| t.get("tertiaryColor"),
        |t| adjust(&t.get("primaryColor"), &[(H, -30.0)]),
        |t| adjust(&t.get("primaryColor"), &[(H, -60.0)]),
        |t| adjust(&t.get("primaryColor"), &[(H, -90.0)]),
        |t| adjust(&t.get("primaryColor"), &[(H, 60.0)]),
        |t| adjust(&t.get("primaryColor"), &[(H, 120.0)]),
    ];
    for (key, seed) in GITS.iter().zip(seed) {
        t.or(key, seed);
    }
    let dark = t.dark();
    for key in GITS {
        let color = t.get(key);
        t.set(
            key,
            if dark {
                lighten(&color, 25.0)
            } else {
                darken(&color, 25.0)
            },
        );
    }
}

/// pie の文字と線（`this.pieX = this.pieX || …`）。
fn pie_text(t: &mut Theme, title: &str) {
    t.or("pieTitleTextColor", |t| t.get(title));
    t.or("pieSectionTextColor", |t| t.get("textColor"));
    t.or("pieStrokeColor", |_| "black".into());
    t.or("pieOuterStrokeColor", |_| "black".into());
}

// 下のテーマは、返す変数とそれが読む変数の行だけを、Mermaid と同じ順に写す。コンストラクタは上書きより前に
// 走るので、そこで `darkMode` を読む `mkBorder` はいつも偽で計算する（Mermaid も同じ）。

/// `theme-default.js` のコンストラクタ。終わりで `updateColors` を一度呼ぶ。`calculate` はこのあと上書きを
/// 置いてもう一度呼ぶので、git0〜git7 は `darken(…, 25)` を 2 回受ける（Mermaid の既定の gitGraph の色）。
///
/// secondaryColor・tertiaryColor・枠と文字の色・mainBkg はここでしか計算しないので、primaryColor などを
/// 上書きしても変わらない。pie・git は updateColors で `||` だが、上書きより前の 1 回目で埋まるので、これも
/// 基の色の上書きには付いてこない（Mermaid も同じ。基の色から作り直すのは base だけ）。上書きした変数そのものは
/// 最後に置き直すので、その値になる。darkMode の上書きは 2 回目の git の lighten・darken にだけ効く。
fn default_constructor(t: &mut Theme) {
    t.set("background", "#f4f4f4");
    t.set("primaryColor", "#ECECFF");
    t.set(
        "secondaryColor",
        adjust(&t.get("primaryColor"), &[(H, 120.0)]),
    );
    t.set("secondaryColor", "#ffffde");
    t.set(
        "tertiaryColor",
        adjust(&t.get("primaryColor"), &[(H, -160.0)]),
    );
    t.set(
        "primaryBorderColor",
        mk_border(&t.get("primaryColor"), false),
    );
    t.set(
        "secondaryBorderColor",
        mk_border(&t.get("secondaryColor"), false),
    );
    t.set(
        "tertiaryBorderColor",
        mk_border(&t.get("tertiaryColor"), false),
    );
    t.set("primaryTextColor", invert(&t.get("primaryColor")));
    t.set("secondaryTextColor", invert(&t.get("secondaryColor")));
    t.set("tertiaryTextColor", invert(&t.get("tertiaryColor")));
    t.set("lineColor", invert(&t.get("background")));
    t.set("textColor", invert(&t.get("background")));
    t.set("background", "white");
    t.set("mainBkg", "#ECECFF");
    t.set("secondBkg", "#ffffde");
    t.set("lineColor", "#333333");
    t.set("border1", "#9370DB");
    t.set("border2", "#aaaa33");
    t.set("arrowheadColor", "#333333");
    t.set("labelBackground", "rgba(232,232,232, 0.8)");
    t.set("textColor", "#333");
    t.set("actorTextColor", "black");
    t.set("noteBkgColor", "#fff5ad");
    t.set("clusterBkg", "#FBFBFF");
    t.set("taskTextDarkColor", "black");
    default_update(t);
}

/// `theme-default.js` の `updateColors`。nodeBorder・clusterBkg・titleColor などは直に代入するので、それらを
/// 上書きしても値が勝つのは最後に置き直すから。
fn default_update(t: &mut Theme) {
    t.set("nodeBorder", t.get("border1"));
    t.set("clusterBkg", t.get("secondBkg"));
    t.set("clusterBorder", t.get("border2"));
    t.set("titleColor", t.get("textColor"));
    t.set("edgeLabelBackground", t.get("labelBackground"));
    t.set("noteBorderColor", t.get("border2"));
    t.set("noteTextColor", t.get("actorTextColor"));
    t.or("pie1", |t| t.get("primaryColor"));
    t.or("pie2", |t| t.get("secondaryColor"));
    t.or("pie3", |t| adjust(&t.get("tertiaryColor"), &[(L, -40.0)]));
    t.or("pie4", |t| adjust(&t.get("primaryColor"), &[(L, -10.0)]));
    t.or("pie5", |t| adjust(&t.get("secondaryColor"), &[(L, -30.0)]));
    t.or("pie6", |t| adjust(&t.get("tertiaryColor"), &[(L, -20.0)]));
    t.or("pie7", |t| {
        adjust(&t.get("primaryColor"), &[(H, 60.0), (L, -20.0)])
    });
    t.or("pie8", |t| {
        adjust(&t.get("primaryColor"), &[(H, -60.0), (L, -40.0)])
    });
    pie_text(t, "taskTextDarkColor");
    git_colors(t);
}

/// `theme-neutral.js` のコンストラクタ。
fn neutral_constructor(t: &mut Theme) {
    t.set("primaryColor", "#eee");
    t.set("contrast", "#707070");
    t.set("secondaryColor", lighten(&t.get("contrast"), 55.0));
    t.set("background", "#ffffff");
    t.set(
        "tertiaryColor",
        adjust(&t.get("primaryColor"), &[(H, -160.0)]),
    );
    t.set(
        "primaryBorderColor",
        mk_border(&t.get("primaryColor"), false),
    );
    t.set(
        "secondaryBorderColor",
        mk_border(&t.get("secondaryColor"), false),
    );
    t.set(
        "tertiaryBorderColor",
        mk_border(&t.get("tertiaryColor"), false),
    );
    t.set("primaryTextColor", invert(&t.get("primaryColor")));
    t.set("secondaryTextColor", invert(&t.get("secondaryColor")));
    t.set("tertiaryTextColor", invert(&t.get("tertiaryColor")));
    t.set("lineColor", invert(&t.get("background")));
    t.set("textColor", invert(&t.get("background")));
    t.set("mainBkg", "#eee");
    t.set("lineColor", "#666");
    t.set("border1", "#999");
    t.set("text", "#333");
    t.set("arrowheadColor", "#333333");
    t.set("edgeLabelBackground", "white");
}

/// `theme-neutral.js` の `updateColors`。note の 3 色・nodeBorder・clusterBkg・clusterBorder・titleColor は
/// 直に代入する（上書きは最後に置き直して勝つ。clusterBkg・clusterBorder は contrast の上書きに付いてくる）。
/// pieN は cScaleN（`||` なので cScaleN の上書きが pie と git に届く）。
fn neutral_update(t: &mut Theme) {
    t.set("secondBkg", lighten(&t.get("contrast"), 55.0));
    t.set("border2", t.get("contrast"));
    t.set("noteBorderColor", "#999");
    t.set("noteBkgColor", "#666");
    t.set("noteTextColor", "#fff");
    let c_scale = [
        "#555", "#F4F4F4", "#555", "#BBB", "#777", "#999", "#DDD", "#FFF", "#DDD", "#BBB", "#999",
        "#777",
    ];
    for (i, color) in c_scale.iter().enumerate() {
        t.or(&format!("cScale{i}"), |_| color.to_string());
    }
    t.set("nodeBorder", t.get("border1"));
    t.set("clusterBkg", t.get("secondBkg"));
    t.set("clusterBorder", t.get("border2"));
    t.set("titleColor", t.get("text"));
    t.set("taskTextDarkColor", t.get("text"));
    for i in 0..12 {
        t.set(&format!("pie{i}"), t.get(&format!("cScale{i}")));
    }
    pie_text(t, "taskTextDarkColor");
    // git0 = darken(pie1, 25) || primaryColor、gitN = pie(N+1) || …（pie はいつも真）。
    t.set("git0", darken(&t.get("pie1"), 25.0));
    for i in 1..8 {
        t.set(&format!("git{i}"), t.get(&format!("pie{}", i + 1)));
    }
}

/// `theme-dark.js` のコンストラクタ。
fn dark_constructor(t: &mut Theme) {
    t.set("background", "#333");
    t.set("primaryColor", "#1f2020");
    t.set("secondaryColor", lighten(&t.get("primaryColor"), 16.0));
    t.set(
        "tertiaryColor",
        adjust(&t.get("primaryColor"), &[(H, -160.0)]),
    );
    t.set("primaryBorderColor", invert(&t.get("background")));
    t.set(
        "secondaryBorderColor",
        mk_border(&t.get("secondaryColor"), false),
    );
    t.set(
        "tertiaryBorderColor",
        mk_border(&t.get("tertiaryColor"), false),
    );
    t.set("primaryTextColor", invert(&t.get("primaryColor")));
    t.set("secondaryTextColor", invert(&t.get("secondaryColor")));
    t.set("tertiaryTextColor", invert(&t.get("tertiaryColor")));
    t.set("lineColor", invert(&t.get("background")));
    t.set("textColor", invert(&t.get("background")));
    t.set("mainBkg", "#1f2020");
    t.set("mainContrastColor", "lightgrey");
    t.set("border1", "#ccc");
    t.set("border2", khroma::rgba(255.0, 255.0, 255.0, 0.25));
    t.set("labelBackground", "#181818");
    t.set("textColor", "#ccc");
    t.set("titleColor", "#F9FFFE");
    t.set("noteBkgColor", "#fff5ad");
}

/// `theme-dark.js` の `updateColors`。lineColor・arrowheadColor は mainContrastColor、note の 3 色は secondBkg・
/// secondaryBorderColor・secondaryTextColor を直に代入する（上書きは最後に置き直して勝つ）。pieN は cScaleN、
/// git は secondaryColor と pie から。
fn dark_update(t: &mut Theme) {
    t.set("secondBkg", lighten(&t.get("mainBkg"), 16.0));
    t.set("lineColor", t.get("mainContrastColor"));
    t.set("arrowheadColor", t.get("mainContrastColor"));
    t.set("nodeBorder", t.get("border1"));
    t.set("clusterBkg", t.get("secondBkg"));
    t.set("clusterBorder", t.get("border2"));
    t.set(
        "edgeLabelBackground",
        lighten(&t.get("labelBackground"), 25.0),
    );
    t.set("noteBorderColor", t.get("secondaryBorderColor"));
    t.set("noteBkgColor", t.get("secondBkg"));
    t.set("noteTextColor", t.get("secondaryTextColor"));
    let c_scale = [
        "#0b0000", "#4d1037", "#3f5258", "#4f2f1b", "#6e0a0a", "#3b0048", "#995a01", "#154706",
        "#161722", "#00296f", "#01629c", "#010029",
    ];
    for (i, color) in c_scale.iter().enumerate() {
        t.or(&format!("cScale{}", i + 1), |_| color.to_string());
    }
    t.or("cScale0", |t| t.get("primaryColor"));
    for i in 0..12 {
        t.set(&format!("pie{i}"), t.get(&format!("cScale{i}")));
    }
    t.or("pieTitleTextColor", |t| t.get("mainContrastColor"));
    t.or("pieSectionTextColor", |t| t.get("textColor"));
    t.or("pieStrokeColor", |_| "black".into());
    t.or("pieOuterStrokeColor", |_| "black".into());
    t.set("git0", lighten(&t.get("secondaryColor"), 20.0));
    // gitN = lighten(pie(N+1) || …, 20)（git5・git6 は 10。pie はいつも真）。
    for i in 1..8 {
        let amount = if i == 5 || i == 6 { 10.0 } else { 20.0 };
        t.set(
            &format!("git{i}"),
            lighten(&t.get(&format!("pie{}", i + 1)), amount),
        );
    }
    t.or("nodeBorder", |_| "#999".into());
}

/// `theme-forest.js` のコンストラクタ。
fn forest_constructor(t: &mut Theme) {
    t.set("background", "#f4f4f4");
    t.set("primaryColor", "#cde498");
    t.set("secondaryColor", "#cdffb2");
    t.set("background", "white");
    t.set("mainBkg", "#cde498");
    t.set("secondBkg", "#cdffb2");
    t.set("lineColor", "green");
    t.set("border1", "#13540c");
    t.set("border2", "#6eaa49");
    t.set("arrowheadColor", "green");
    t.set("tertiaryColor", lighten("#cde498", 10.0));
    t.set(
        "primaryBorderColor",
        mk_border(&t.get("primaryColor"), false),
    );
    t.set(
        "secondaryBorderColor",
        mk_border(&t.get("secondaryColor"), false),
    );
    t.set(
        "tertiaryBorderColor",
        mk_border(&t.get("tertiaryColor"), false),
    );
    t.set("primaryTextColor", invert(&t.get("primaryColor")));
    t.set("secondaryTextColor", invert(&t.get("secondaryColor")));
    // tertiaryTextColor は primaryColor から（もとのまま）。
    t.set("tertiaryTextColor", invert(&t.get("primaryColor")));
    t.set("lineColor", invert(&t.get("background")));
    t.set("textColor", invert(&t.get("background")));
    t.set("titleColor", "#333");
    t.set("edgeLabelBackground", "#e8e8e8");
    t.set("actorTextColor", "black");
    t.set("noteBkgColor", "#fff5ad");
    t.set("taskTextDarkColor", "black");
}

/// `theme-forest.js` の `updateColors`。noteBorderColor・noteTextColor・nodeBorder・clusterBkg・clusterBorder は
/// 直に代入する（上書きは最後に置き直して勝つ）。
fn forest_update(t: &mut Theme) {
    t.set("noteBorderColor", t.get("border2"));
    t.set("noteTextColor", t.get("actorTextColor"));
    t.set("nodeBorder", t.get("border1"));
    t.set("clusterBkg", t.get("secondBkg"));
    t.set("clusterBorder", t.get("border2"));
    t.or("pie1", |t| t.get("primaryColor"));
    t.or("pie2", |t| t.get("secondaryColor"));
    t.or("pie3", |t| t.get("tertiaryColor"));
    t.or("pie4", |t| adjust(&t.get("primaryColor"), &[(L, -30.0)]));
    t.or("pie5", |t| adjust(&t.get("secondaryColor"), &[(L, -30.0)]));
    t.or("pie6", |t| {
        adjust(&t.get("tertiaryColor"), &[(H, 40.0), (L, -40.0)])
    });
    t.or("pie7", |t| {
        adjust(&t.get("primaryColor"), &[(H, 60.0), (L, -10.0)])
    });
    t.or("pie8", |t| {
        adjust(&t.get("primaryColor"), &[(H, -60.0), (L, -10.0)])
    });
    pie_text(t, "taskTextDarkColor");
    git_colors(t);
}

/// `theme-base.js` のコンストラクタ。基の色だけを置く。
fn base_constructor(t: &mut Theme) {
    t.set("background", "#f4f4f4");
    t.set("primaryColor", "#fff4dd");
    t.set("noteBkgColor", "#fff5ad");
    t.set("noteTextColor", "#333");
}

/// `theme-base.js` の `updateColors`。すべて `x || y` なので、上書きした基の色は派生する色すべてに届き、上書きした
/// 派生の色はそのまま残る。`darkMode` もここで読む。
fn base_update(t: &mut Theme) {
    let dark = t.dark();
    t.or("primaryTextColor", |_| {
        if dark { "#eee" } else { "#333" }.into()
    });
    t.or("secondaryColor", |t| {
        adjust(&t.get("primaryColor"), &[(H, -120.0)])
    });
    t.or("tertiaryColor", |t| {
        adjust(&t.get("primaryColor"), &[(H, 180.0), (L, 5.0)])
    });
    t.or("primaryBorderColor", |t| {
        mk_border(&t.get("primaryColor"), dark)
    });
    t.or("secondaryBorderColor", |t| {
        mk_border(&t.get("secondaryColor"), dark)
    });
    t.or("tertiaryBorderColor", |t| {
        mk_border(&t.get("tertiaryColor"), dark)
    });
    t.or("noteBorderColor", |t| {
        mk_border(&t.get("noteBkgColor"), dark)
    });
    t.or("noteBkgColor", |_| "#fff5ad".into());
    t.or("noteTextColor", |_| "#333".into());
    t.or("secondaryTextColor", |t| invert(&t.get("secondaryColor")));
    t.or("tertiaryTextColor", |t| invert(&t.get("tertiaryColor")));
    t.or("lineColor", |t| invert(&t.get("background")));
    t.or("arrowheadColor", |t| invert(&t.get("background")));
    t.or("textColor", |t| t.get("primaryTextColor"));
    t.or("mainBkg", |t| t.get("primaryColor"));
    t.or("nodeBorder", |t| t.get("primaryBorderColor"));
    t.or("clusterBkg", |t| t.get("tertiaryColor"));
    t.or("clusterBorder", |t| t.get("tertiaryBorderColor"));
    t.or("titleColor", |t| t.get("tertiaryTextColor"));
    t.or("edgeLabelBackground", |t| {
        let secondary = t.get("secondaryColor");
        if dark {
            darken(&secondary, 30.0)
        } else {
            secondary
        }
    });
    t.or("taskTextDarkColor", |t| t.get("textColor"));
    t.or("pie1", |t| t.get("primaryColor"));
    t.or("pie2", |t| t.get("secondaryColor"));
    t.or("pie3", |t| t.get("tertiaryColor"));
    t.or("pie4", |t| adjust(&t.get("primaryColor"), &[(L, -10.0)]));
    t.or("pie5", |t| adjust(&t.get("secondaryColor"), &[(L, -10.0)]));
    t.or("pie6", |t| adjust(&t.get("tertiaryColor"), &[(L, -10.0)]));
    t.or("pie7", |t| {
        adjust(&t.get("primaryColor"), &[(H, 60.0), (L, -10.0)])
    });
    t.or("pie8", |t| {
        adjust(&t.get("primaryColor"), &[(H, -60.0), (L, -10.0)])
    });
    pie_text(t, "taskTextDarkColor");
    git_colors(t);
}

/// khroma 2.1.0 の、テーマが使う分。色は文字で受け取り、文字で返す（khroma と同じ形）。
mod khroma {
    /// 変えた成分の種類（khroma の `TYPE`）。RGB と HSL を同時には変えない。
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Kind {
        All,
        Rgb,
        Hsl,
    }

    /// 色の成分。
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum Ch {
        R,
        G,
        B,
        H,
        S,
        L,
        A,
    }

    /// khroma の `Channels`。読んだ形の成分だけを持ち、ほかの形の成分は求めるたびに計算する。
    struct Channels {
        r: Option<f64>,
        g: Option<f64>,
        b: Option<f64>,
        h: Option<f64>,
        s: Option<f64>,
        l: Option<f64>,
        a: f64,
        kind: Kind,
        changed: bool,
        /// 読んだもとの文字（変えていなければ、文字にするときにそのまま返す）。
        color: Option<String>,
    }

    impl Channels {
        fn rgb(r: f64, g: f64, b: f64, a: f64, color: Option<String>) -> Self {
            Channels {
                r: Some(r),
                g: Some(g),
                b: Some(b),
                h: None,
                s: None,
                l: None,
                a,
                kind: Kind::All,
                changed: false,
                color,
            }
        }

        fn hsl(h: f64, s: f64, l: f64, a: f64, color: String) -> Self {
            Channels {
                r: None,
                g: None,
                b: None,
                h: Some(h),
                s: Some(s),
                l: Some(l),
                a,
                kind: Kind::All,
                changed: false,
                color: Some(color),
            }
        }

        fn ensure_hsl(&mut self) {
            let (r, g, b) = (self.r, self.g, self.b);
            let rgb = || (r.unwrap_or(0.0), g.unwrap_or(0.0), b.unwrap_or(0.0));
            if self.h.is_none() {
                self.h = Some(rgb2hsl(rgb(), Ch::H));
            }
            if self.s.is_none() {
                self.s = Some(rgb2hsl(rgb(), Ch::S));
            }
            if self.l.is_none() {
                self.l = Some(rgb2hsl(rgb(), Ch::L));
            }
        }

        fn ensure_rgb(&mut self) {
            let (h, s, l) = (self.h, self.s, self.l);
            let hsl = || (h.unwrap_or(0.0), s.unwrap_or(0.0), l.unwrap_or(0.0));
            if self.r.is_none() {
                self.r = Some(hsl2rgb(hsl(), Ch::R));
            }
            if self.g.is_none() {
                self.g = Some(hsl2rgb(hsl(), Ch::G));
            }
            if self.b.is_none() {
                self.b = Some(hsl2rgb(hsl(), Ch::B));
            }
        }

        fn field(&self, ch: Ch) -> Option<f64> {
            match ch {
                Ch::R => self.r,
                Ch::G => self.g,
                Ch::B => self.b,
                Ch::H => self.h,
                Ch::S => self.s,
                Ch::L => self.l,
                Ch::A => Some(self.a),
            }
        }

        /// khroma の getter。
        fn get(&mut self, ch: Ch) -> f64 {
            match ch {
                Ch::R | Ch::G | Ch::B => {
                    if let Some(value) = self.field(ch)
                        && self.kind != Kind::Hsl
                    {
                        return value;
                    }
                    self.ensure_hsl();
                    let hsl = (
                        self.h.unwrap_or(0.0),
                        self.s.unwrap_or(0.0),
                        self.l.unwrap_or(0.0),
                    );
                    hsl2rgb(hsl, ch)
                }
                Ch::H | Ch::S | Ch::L => {
                    if let Some(value) = self.field(ch)
                        && self.kind != Kind::Rgb
                    {
                        return value;
                    }
                    self.ensure_rgb();
                    let rgb = (
                        self.r.unwrap_or(0.0),
                        self.g.unwrap_or(0.0),
                        self.b.unwrap_or(0.0),
                    );
                    rgb2hsl(rgb, ch)
                }
                Ch::A => self.a,
            }
        }

        /// khroma の setter。
        fn set(&mut self, ch: Ch, value: f64) {
            let kind = match ch {
                Ch::R | Ch::G | Ch::B => Some(Kind::Rgb),
                Ch::H | Ch::S | Ch::L => Some(Kind::Hsl),
                Ch::A => None,
            };
            if let Some(kind) = kind {
                assert!(
                    self.kind == Kind::All || self.kind == kind,
                    "Cannot change both RGB and HSL channels at the same time"
                );
                self.kind = kind;
            }
            self.changed = true;
            match ch {
                Ch::R => self.r = Some(value),
                Ch::G => self.g = Some(value),
                Ch::B => self.b = Some(value),
                Ch::H => self.h = Some(value),
                Ch::S => self.s = Some(value),
                Ch::L => self.l = Some(value),
                Ch::A => self.a = value,
            }
        }
    }

    /// JavaScript の `Math.round`（.5 は大きいほうへ）。
    fn js_round(x: f64) -> f64 {
        let floor = x.floor();
        if x - floor >= 0.5 { floor + 1.0 } else { floor }
    }

    /// khroma の `lang.round`（小数 10 桁）。
    fn round10(x: f64) -> f64 {
        js_round(x * 10_000_000_000.0) / 10_000_000_000.0
    }

    fn clamp(ch: Ch, value: f64) -> f64 {
        let (low, high) = match ch {
            Ch::R | Ch::G | Ch::B => (0.0, 255.0),
            Ch::S | Ch::L => (0.0, 100.0),
            Ch::A => (0.0, 1.0),
            // 色相は 360 の剰余だけ（負のままのこともある）。
            Ch::H => return value % 360.0,
        };
        if value >= high {
            high
        } else if value < low {
            low
        } else {
            value
        }
    }

    fn hue2rgb(p: f64, q: f64, mut t: f64) -> f64 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 1.0 / 2.0 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    }

    fn hsl2rgb((h, s, l): (f64, f64, f64), ch: Ch) -> f64 {
        // `if (!s)`: 0 と NaN は無彩色
        if s == 0.0 || s.is_nan() {
            return l * 2.55;
        }
        let h = h / 360.0;
        let s = s / 100.0;
        let l = l / 100.0;
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            (l + s) - (l * s)
        };
        let p = 2.0 * l - q;
        match ch {
            Ch::R => hue2rgb(p, q, h + 1.0 / 3.0) * 255.0,
            Ch::G => hue2rgb(p, q, h) * 255.0,
            _ => hue2rgb(p, q, h - 1.0 / 3.0) * 255.0,
        }
    }

    fn rgb2hsl((r, g, b): (f64, f64, f64), ch: Ch) -> f64 {
        let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        if ch == Ch::L {
            return l * 100.0;
        }
        if max == min {
            return 0.0;
        }
        let d = max - min;
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        if ch == Ch::S {
            return s * 100.0;
        }
        if max == r {
            ((g - b) / d + if g < b { 6.0 } else { 0.0 }) * 60.0
        } else if max == g {
            ((b - r) / d + 2.0) * 60.0
        } else {
            ((r - g) / d + 4.0) * 60.0
        }
    }

    /// CSS の色の名前（khroma の表のまま。khroma は 16 進に直してから読む）。
    const KEYWORDS: &[(&str, &str)] = &[
        ("aliceblue", "#f0f8ff"),
        ("antiquewhite", "#faebd7"),
        ("aqua", "#00ffff"),
        ("aquamarine", "#7fffd4"),
        ("azure", "#f0ffff"),
        ("beige", "#f5f5dc"),
        ("bisque", "#ffe4c4"),
        ("black", "#000000"),
        ("blanchedalmond", "#ffebcd"),
        ("blue", "#0000ff"),
        ("blueviolet", "#8a2be2"),
        ("brown", "#a52a2a"),
        ("burlywood", "#deb887"),
        ("cadetblue", "#5f9ea0"),
        ("chartreuse", "#7fff00"),
        ("chocolate", "#d2691e"),
        ("coral", "#ff7f50"),
        ("cornflowerblue", "#6495ed"),
        ("cornsilk", "#fff8dc"),
        ("crimson", "#dc143c"),
        ("cyanaqua", "#00ffff"),
        ("darkblue", "#00008b"),
        ("darkcyan", "#008b8b"),
        ("darkgoldenrod", "#b8860b"),
        ("darkgray", "#a9a9a9"),
        ("darkgreen", "#006400"),
        ("darkgrey", "#a9a9a9"),
        ("darkkhaki", "#bdb76b"),
        ("darkmagenta", "#8b008b"),
        ("darkolivegreen", "#556b2f"),
        ("darkorange", "#ff8c00"),
        ("darkorchid", "#9932cc"),
        ("darkred", "#8b0000"),
        ("darksalmon", "#e9967a"),
        ("darkseagreen", "#8fbc8f"),
        ("darkslateblue", "#483d8b"),
        ("darkslategray", "#2f4f4f"),
        ("darkslategrey", "#2f4f4f"),
        ("darkturquoise", "#00ced1"),
        ("darkviolet", "#9400d3"),
        ("deeppink", "#ff1493"),
        ("deepskyblue", "#00bfff"),
        ("dimgray", "#696969"),
        ("dimgrey", "#696969"),
        ("dodgerblue", "#1e90ff"),
        ("firebrick", "#b22222"),
        ("floralwhite", "#fffaf0"),
        ("forestgreen", "#228b22"),
        ("fuchsia", "#ff00ff"),
        ("gainsboro", "#dcdcdc"),
        ("ghostwhite", "#f8f8ff"),
        ("gold", "#ffd700"),
        ("goldenrod", "#daa520"),
        ("gray", "#808080"),
        ("green", "#008000"),
        ("greenyellow", "#adff2f"),
        ("grey", "#808080"),
        ("honeydew", "#f0fff0"),
        ("hotpink", "#ff69b4"),
        ("indianred", "#cd5c5c"),
        ("indigo", "#4b0082"),
        ("ivory", "#fffff0"),
        ("khaki", "#f0e68c"),
        ("lavender", "#e6e6fa"),
        ("lavenderblush", "#fff0f5"),
        ("lawngreen", "#7cfc00"),
        ("lemonchiffon", "#fffacd"),
        ("lightblue", "#add8e6"),
        ("lightcoral", "#f08080"),
        ("lightcyan", "#e0ffff"),
        ("lightgoldenrodyellow", "#fafad2"),
        ("lightgray", "#d3d3d3"),
        ("lightgreen", "#90ee90"),
        ("lightgrey", "#d3d3d3"),
        ("lightpink", "#ffb6c1"),
        ("lightsalmon", "#ffa07a"),
        ("lightseagreen", "#20b2aa"),
        ("lightskyblue", "#87cefa"),
        ("lightslategray", "#778899"),
        ("lightslategrey", "#778899"),
        ("lightsteelblue", "#b0c4de"),
        ("lightyellow", "#ffffe0"),
        ("lime", "#00ff00"),
        ("limegreen", "#32cd32"),
        ("linen", "#faf0e6"),
        ("magenta", "#ff00ff"),
        ("maroon", "#800000"),
        ("mediumaquamarine", "#66cdaa"),
        ("mediumblue", "#0000cd"),
        ("mediumorchid", "#ba55d3"),
        ("mediumpurple", "#9370db"),
        ("mediumseagreen", "#3cb371"),
        ("mediumslateblue", "#7b68ee"),
        ("mediumspringgreen", "#00fa9a"),
        ("mediumturquoise", "#48d1cc"),
        ("mediumvioletred", "#c71585"),
        ("midnightblue", "#191970"),
        ("mintcream", "#f5fffa"),
        ("mistyrose", "#ffe4e1"),
        ("moccasin", "#ffe4b5"),
        ("navajowhite", "#ffdead"),
        ("navy", "#000080"),
        ("oldlace", "#fdf5e6"),
        ("olive", "#808000"),
        ("olivedrab", "#6b8e23"),
        ("orange", "#ffa500"),
        ("orangered", "#ff4500"),
        ("orchid", "#da70d6"),
        ("palegoldenrod", "#eee8aa"),
        ("palegreen", "#98fb98"),
        ("paleturquoise", "#afeeee"),
        ("palevioletred", "#db7093"),
        ("papayawhip", "#ffefd5"),
        ("peachpuff", "#ffdab9"),
        ("peru", "#cd853f"),
        ("pink", "#ffc0cb"),
        ("plum", "#dda0dd"),
        ("powderblue", "#b0e0e6"),
        ("purple", "#800080"),
        ("rebeccapurple", "#663399"),
        ("red", "#ff0000"),
        ("rosybrown", "#bc8f8f"),
        ("royalblue", "#4169e1"),
        ("saddlebrown", "#8b4513"),
        ("salmon", "#fa8072"),
        ("sandybrown", "#f4a460"),
        ("seagreen", "#2e8b57"),
        ("seashell", "#fff5ee"),
        ("sienna", "#a0522d"),
        ("silver", "#c0c0c0"),
        ("skyblue", "#87ceeb"),
        ("slateblue", "#6a5acd"),
        ("slategray", "#708090"),
        ("slategrey", "#708090"),
        ("snow", "#fffafa"),
        ("springgreen", "#00ff7f"),
        ("tan", "#d2b48c"),
        ("teal", "#008080"),
        ("thistle", "#d8bfd8"),
        ("transparent", "#00000000"),
        ("turquoise", "#40e0d0"),
        ("violet", "#ee82ee"),
        ("wheat", "#f5deb3"),
        ("white", "#ffffff"),
        ("whitesmoke", "#f5f5f5"),
        ("yellow", "#ffff00"),
        ("yellowgreen", "#9acd32"),
    ];

    fn parse_hex(color: &str) -> Option<Channels> {
        let hex = color.strip_prefix('#')?;
        if !matches!(hex.len(), 3 | 4 | 6 | 8) || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let dec = u32::from_str_radix(hex, 16).ok()?;
        let length = hex.len();
        let has_alpha = length % 4 == 0;
        let full = length > 4;
        let (multiplier, bits, mask) = if full { (1, 8, 255) } else { (17, 4, 15) };
        let offset = if has_alpha { 0 } else { -1 };
        let part = |n: i32| f64::from(((dec >> (bits * (offset + n))) & mask) * multiplier);
        let a = if has_alpha {
            f64::from((dec & mask) * multiplier) / 255.0
        } else {
            1.0
        };
        Some(Channels::rgb(
            part(3),
            part(2),
            part(1),
            a,
            Some(color.to_string()),
        ))
    }

    /// `rgb(…)`・`hsl(…)` の中の数（`,`・空白・`/` で区切る）。単位は外して、`%` かどうかを返す。
    fn parse_args<'a>(color: &'a str, names: &[&str]) -> Option<Vec<&'a str>> {
        let lower = color.to_ascii_lowercase();
        let open = names
            .iter()
            .find(|name| lower.starts_with(&format!("{name}(")))?
            .len()
            + 1;
        let inner = color.get(open..)?.trim_end().strip_suffix(')')?;
        let args: Vec<&str> = inner
            .split([',', '/', ' ', '\t'])
            .map(str::trim)
            .filter(|arg| !arg.is_empty())
            .collect();
        matches!(args.len(), 3 | 4).then_some(args)
    }

    fn number(arg: &str) -> Option<(f64, bool)> {
        let arg = arg.strip_prefix('+').unwrap_or(arg);
        match arg.strip_suffix('%') {
            Some(n) => Some((n.parse().ok()?, true)),
            None => Some((arg.parse().ok()?, false)),
        }
    }

    fn parse_alpha(args: &[&str]) -> Option<f64> {
        match args.get(3) {
            Some(arg) => {
                let (a, percent) = number(arg)?;
                Some(clamp(Ch::A, if percent { a / 100.0 } else { a }))
            }
            None => Some(1.0),
        }
    }

    fn parse_rgb(color: &str) -> Option<Channels> {
        let args = parse_args(color, &["rgba", "rgb"])?;
        let channel = |arg: &str, ch: Ch| {
            let (n, percent) = number(arg)?;
            Some(clamp(ch, if percent { n * 2.55 } else { n }))
        };
        Some(Channels::rgb(
            channel(args[0], Ch::R)?,
            channel(args[1], Ch::G)?,
            channel(args[2], Ch::B)?,
            parse_alpha(&args)?,
            Some(color.to_string()),
        ))
    }

    fn parse_hsl(color: &str) -> Option<Channels> {
        let args = parse_args(color, &["hsla", "hsl"])?;
        let hue = args[0].to_ascii_lowercase();
        let h = if let Some(n) = hue.strip_suffix("grad") {
            n.parse::<f64>().ok()? * 0.9
        } else if let Some(n) = hue.strip_suffix("rad") {
            n.parse::<f64>().ok()? * 180.0 / std::f64::consts::PI
        } else if let Some(n) = hue.strip_suffix("turn") {
            n.parse::<f64>().ok()? * 360.0
        } else {
            hue.strip_suffix("deg").unwrap_or(&hue).parse().ok()?
        };
        let percent = |arg: &str| match number(arg)? {
            (n, true) => Some(n),
            (_, false) => None,
        };
        Some(Channels::hsl(
            clamp(Ch::H, h),
            clamp(Ch::S, percent(args[1])?),
            clamp(Ch::L, percent(args[2])?),
            parse_alpha(&args)?,
            color.to_string(),
        ))
    }

    fn try_parse(color: &str) -> Option<Channels> {
        parse_hex(color)
            .or_else(|| parse_rgb(color))
            .or_else(|| parse_hsl(color))
            .or_else(|| {
                let lower = color.to_ascii_lowercase();
                let (_, hex) = KEYWORDS.iter().find(|(name, _)| *name == lower)?;
                parse_hex(hex)
            })
    }

    fn parse(color: &str) -> Channels {
        try_parse(color).unwrap_or_else(|| panic!("Unsupported color format: \"{color}\""))
    }

    /// khroma が色として読めるか。
    pub(super) fn is_valid(color: &str) -> bool {
        try_parse(color).is_some()
    }

    fn hex_byte(x: f64) -> String {
        format!("{:02x}", js_round(x) as i64)
    }

    fn stringify_hex(ch: &mut Channels) -> String {
        let (r, g, b, a) = (ch.get(Ch::R), ch.get(Ch::G), ch.get(Ch::B), ch.a);
        let mut out = format!("#{}{}{}", hex_byte(r), hex_byte(g), hex_byte(b));
        if a < 1.0 {
            out.push_str(&hex_byte(a * 255.0));
        }
        out
    }

    fn stringify(ch: &mut Channels) -> String {
        if !ch.changed
            && let Some(color) = &ch.color
        {
            return color.clone();
        }
        if ch.kind == Kind::Hsl || ch.r.is_none() {
            let (h, s, l, a) = (ch.get(Ch::H), ch.get(Ch::S), ch.get(Ch::L), ch.a);
            let (h, s, l) = (round10(h), round10(s), round10(l));
            return if a < 1.0 {
                format!("hsla({h}, {s}%, {l}%, {a})")
            } else {
                format!("hsl({h}, {s}%, {l}%)")
            };
        }
        let (r, g, b, a) = (ch.get(Ch::R), ch.get(Ch::G), ch.get(Ch::B), ch.a);
        let integer = |x: f64| x.fract() == 0.0;
        if a < 1.0 || !integer(r) || !integer(g) || !integer(b) {
            let (r, g, b) = (round10(r), round10(g), round10(b));
            if a < 1.0 {
                format!("rgba({r}, {g}, {b}, {})", round10(a))
            } else {
                format!("rgb({r}, {g}, {b})")
            }
        } else {
            stringify_hex(ch)
        }
    }

    /// `change`: 成分を置き換える。
    fn change(color: &str, channels: &[(Ch, f64)]) -> String {
        let mut ch = parse(color);
        for &(c, value) in channels {
            ch.set(c, clamp(c, value));
        }
        stringify(&mut ch)
    }

    /// `adjust`: 成分に足す（0 の成分は飛ばす）。
    pub(super) fn adjust(color: &str, channels: &[(Ch, f64)]) -> String {
        let mut ch = parse(color);
        let mut changes = Vec::new();
        for &(c, amount) in channels {
            // `if (!channels[c]) continue;`
            if amount == 0.0 || amount.is_nan() {
                continue;
            }
            changes.push((c, ch.get(c) + amount));
        }
        change(color, &changes)
    }

    /// `adjustChannel`: 変わらなければ成分を置かない（もとの文字のまま返る）。
    fn adjust_channel(color: &str, c: Ch, amount: f64) -> String {
        let mut ch = parse(color);
        let current = ch.get(c);
        let next = clamp(c, current + amount);
        if current != next {
            ch.set(c, next);
        }
        stringify(&mut ch)
    }

    pub(super) fn lighten(color: &str, amount: f64) -> String {
        adjust_channel(color, Ch::L, amount)
    }

    pub(super) fn darken(color: &str, amount: f64) -> String {
        adjust_channel(color, Ch::L, -amount)
    }

    /// `rgba(r, g, b, a)`（数で受け取る形）。
    pub(super) fn rgba(r: f64, g: f64, b: f64, a: f64) -> String {
        let mut ch = Channels::rgb(
            clamp(Ch::R, r),
            clamp(Ch::G, g),
            clamp(Ch::B, b),
            clamp(Ch::A, a),
            None,
        );
        stringify(&mut ch)
    }

    /// `invert(color)`（重みは既定の 100）。反転した色と元の色を `mix` で混ぜる。
    pub(super) fn invert(color: &str) -> String {
        let mut inverse = parse(color);
        for c in [Ch::R, Ch::G, Ch::B] {
            let value = 255.0 - inverse.get(c);
            inverse.set(c, value);
        }
        // mix(inverse, color, 100)
        let (r1, g1, b1, a1) = (
            inverse.get(Ch::R),
            inverse.get(Ch::G),
            inverse.get(Ch::B),
            inverse.a,
        );
        let mut original = parse(color);
        let (r2, g2, b2, a2) = (
            original.get(Ch::R),
            original.get(Ch::G),
            original.get(Ch::B),
            original.a,
        );
        // weight = 100
        let weight_scale = 1.0;
        let weight_normalized = (weight_scale * 2.0) - 1.0;
        let alpha_delta = a1 - a2;
        let weight1_combined = if weight_normalized * alpha_delta == -1.0 {
            weight_normalized
        } else {
            (weight_normalized + alpha_delta) / (1.0 + weight_normalized * alpha_delta)
        };
        let weight1 = (weight1_combined + 1.0) / 2.0;
        let weight2 = 1.0 - weight1;
        rgba(
            (r1 * weight1) + (r2 * weight2),
            (g1 * weight1) + (g2 * weight2),
            (b1 * weight1) + (b2 * weight2),
            (a1 * weight_scale) + (a2 * (1.0 - weight_scale)),
        )
    }

    /// khroma の文字を `#rrggbb`（不透明でなければ `#rrggbbaa`）に。
    pub(super) fn to_hex(color: &str) -> String {
        stringify_hex(&mut parse(color))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn keeps_khromas_strings() {
            assert_eq!(
                adjust("#ECECFF", &[(Ch::S, -40.0), (Ch::L, -10.0)]),
                "hsl(240, 60%, 86.2745098039%)"
            );
            assert_eq!(
                adjust("#ECECFF", &[(Ch::H, -160.0)]),
                "hsl(80, 100%, 96.2745098039%)"
            );
            assert_eq!(
                lighten("#1f2020", 16.0),
                "hsl(180, 1.5873015873%, 28.3529411765%)"
            );
            assert_eq!(invert("#ECECFF"), "#131300");
            assert_eq!(invert("white"), "#000000");
            assert_eq!(rgba(255.0, 255.0, 255.0, 0.25), "rgba(255, 255, 255, 0.25)");
            // 変わらなければ、もとの文字のまま
            assert_eq!(lighten("#ffffff", 10.0), "#ffffff");
            assert_eq!(adjust("#eee", &[(Ch::L, 0.0)]), "#eee");
            // 無彩色の色相は 0 から回す（負のまま残る）
            assert_eq!(
                adjust("#eee", &[(Ch::H, -160.0)]),
                "hsl(-160, 0%, 93.3333333333%)"
            );
        }

        #[test]
        fn hex_of_each_format() {
            assert_eq!(to_hex("#abc"), "#aabbcc");
            assert_eq!(to_hex("#abcd"), "#aabbccdd");
            assert_eq!(to_hex("lightgrey"), "#d3d3d3");
            assert_eq!(to_hex("transparent"), "#00000000");
            assert_eq!(to_hex("rgba(232,232,232, 0.8)"), "#e8e8e8cc");
            assert_eq!(to_hex("hsl(240, 100%, 46.2745098039%)"), "#0000ec");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(theme: &str, key: &str) -> String {
        variables(theme)
            .unwrap()
            .into_iter()
            .find(|(k, _)| *k == key)
            .unwrap()
            .1
    }

    fn with(theme: &str, overrides: &[(&str, &str)], key: &str) -> String {
        let overrides: Vec<(String, String)> = overrides
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        variables_with(theme, &overrides)
            .unwrap()
            .into_iter()
            .find(|(k, _)| *k == key)
            .unwrap()
            .1
    }

    #[test]
    fn base_derives_from_an_overridden_primary_color() {
        let red = [("primaryColor", "#ff0000")];
        assert_eq!(with("base", &red, "mainBkg"), "#ff0000");
        assert_eq!(with("base", &red, "pie1"), "#ff0000");
        // adjust('#ff0000', {h: -120}) = hsl(-120, 100%, 50%)
        assert_eq!(with("base", &red, "secondaryColor"), "#0000ff");
        assert_eq!(with("base", &red, "pie2"), "#0000ff");
        // adjust('#ff0000', {h: 180, l: 5}) = hsl(180, 100%, 55%)
        assert_eq!(with("base", &red, "clusterBkg"), "#1affff");
        // mkBorder('#ff0000') = hsl(0, 60%, 40%)
        assert_eq!(with("base", &red, "nodeBorder"), "#a32929");
        // adjust('#ff0000', {l: -10}) = hsl(0, 100%, 40%)
        assert_eq!(with("base", &red, "pie4"), "#cc0000");
        // background は変えていない
        assert_eq!(with("base", &red, "lineColor"), "#0b0b0b");
    }

    #[test]
    fn base_in_dark_mode() {
        let dark = [("darkMode", "true")];
        assert_eq!(with("base", &dark, "primaryTextColor"), "#eeeeee");
        assert_eq!(with("base", &dark, "textColor"), "#eeeeee");
        // mkBorder は lighten 側: adjust('#fff4dd', {s: -40, l: 10}) は l が 100 で白
        assert_eq!(with("base", &dark, "primaryBorderColor"), "#ffffff");
        // adjust('#fff5ad', {s: -40, l: 10}) = hsl(52.68…, 60%, 93.92…%)
        assert_eq!(with("base", &dark, "noteBorderColor"), "#f9f7e6");
        // git は lighten(…, 25)
        assert_eq!(with("base", &dark, "git0"), "#ffffff");
        assert_eq!(with("base", &[("darkMode", "false")], "git0"), "#ffcb5d");
    }

    #[test]
    fn an_overridden_derived_color_wins() {
        let line = [("lineColor", "#123456")];
        assert_eq!(with("base", &line, "lineColor"), "#123456");
        assert_eq!(with("base", &line, "arrowheadColor"), "#0b0b0b");
        // default の updateColors は titleColor を直に代入するが、最後に置き直す
        assert_eq!(
            with("default", &[("titleColor", "red")], "titleColor"),
            "#ff0000"
        );
        assert_eq!(
            with(
                "dark",
                &[("noteBkgColor", "hsl(0, 0%, 50%)")],
                "noteBkgColor"
            ),
            // khroma: 50 * 2.55 = 127.49999…
            "#7f7f7f"
        );
        // 色でない値・知らない名前は読み捨てる
        let junk = [("fontFamily", "trebuchet ms"), ("primaryColor", "nope")];
        assert_eq!(with("base", &junk, "mainBkg"), "#fff4dd");
    }

    #[test]
    fn default_with_an_overridden_primary_color() {
        let red = [("primaryColor", "#ff0000")];
        assert_eq!(with("default", &red, "primaryColor"), "#ff0000");
        // pie・git は `||` で、1 回目（上書き前）の updateColors の値が残る
        assert_eq!(with("default", &red, "pie1"), "#ececff");
        assert_eq!(with("default", &red, "pie4"), "#b9b9ff");
        // git0 は 1 回目（上書き前）に darken した '#ECECFF' のまま、2 回目にもう一度 darken
        assert_eq!(with("default", &red, "git0"), "#0000ec");
        // pie3 = adjust(tertiaryColor, {l: -40})、tertiaryColor はコンストラクタで計算済み
        assert_eq!(with("default", &red, "pie3"), "#b5ff20");
        // コンストラクタだけで計算する色は変わらない（Mermaid も同じ）
        assert_eq!(with("default", &red, "primaryBorderColor"), "#c7c7f1");
        assert_eq!(with("default", &red, "mainBkg"), "#ececff");
    }

    #[test]
    fn unknown_theme() {
        assert!(variables("neo").is_none());
    }

    #[test]
    fn every_key_is_a_hex_colour() {
        for theme in ["default", "neutral", "dark", "forest", "base"] {
            let vars = variables(theme).unwrap();
            assert_eq!(vars.len(), KEYS.len());
            for (key, value) in vars {
                assert!(
                    value.starts_with('#') && matches!(value.len(), 7 | 9),
                    "{theme} {key} {value}"
                );
            }
        }
    }

    // 期待の値は手で計算した（コメントは khroma の途中の文字）。

    #[test]
    fn default_theme_colours() {
        // mkBorder('#ECECFF') = hsl(240, 60%, 86.2745098039%)
        assert_eq!(get("default", "primaryBorderColor"), "#c7c7f1");
        // adjust('#ECECFF', {h: -160}) = hsl(80, 100%, 96.2745098039%)
        assert_eq!(get("default", "tertiaryColor"), "#f9ffec");
        assert_eq!(get("default", "primaryTextColor"), "#131300");
        assert_eq!(get("default", "clusterBkg"), "#ffffde");
        assert_eq!(get("default", "nodeBorder"), "#9370db");
        assert_eq!(get("default", "edgeLabelBackground"), "#e8e8e8cc");
        // darken を 2 回: hsl(240, 100%, 46.2745098039%)
        assert_eq!(get("default", "git0"), "#0000ec");
        // pie4 = adjust('#ECECFF', {l: -10}) = hsl(240, 100%, 86.2745098039%)
        assert_eq!(get("default", "pie4"), "#b9b9ff");
        // pie3 = adjust(tertiaryColor, {l: -40}) = hsl(80, 100%, 56.2745098039%)
        assert_eq!(get("default", "pie3"), "#b5ff20");
    }

    #[test]
    fn dark_theme_colours() {
        // secondBkg = lighten('#1f2020', 16) = hsl(180, 1.5873015873%, 28.3529411765%)
        assert_eq!(get("dark", "clusterBkg"), "#474949");
        assert_eq!(get("dark", "noteBkgColor"), "#474949");
        assert_eq!(get("dark", "primaryBorderColor"), "#cccccc");
        assert_eq!(get("dark", "primaryTextColor"), "#e0dfdf");
        assert_eq!(get("dark", "clusterBorder"), "#ffffff40");
        assert_eq!(get("dark", "lineColor"), "#d3d3d3");
        // lighten('#181818', 25) = hsl(0, 0%, 34.4117647059%)
        assert_eq!(get("dark", "edgeLabelBackground"), "#585858");
        assert_eq!(get("dark", "pie1"), "#0b0000");
        // git1 = lighten(pie2 = '#4d1037', 20)
        assert_eq!(get("dark", "git1"), "#a12273");
    }

    #[test]
    fn neutral_theme_colours() {
        // lighten('#707070', 55) = hsl(0, 0%, 98.9215686275%)
        assert_eq!(get("neutral", "secondaryColor"), "#fcfcfc");
        // mkBorder('#eee') = hsl(0, 0%, 83.3333333333%)
        assert_eq!(get("neutral", "primaryBorderColor"), "#d4d4d4");
        assert_eq!(get("neutral", "textColor"), "#000000");
        // darken('#F4F4F4', 25) = hsl(0, 0%, 70.6862745098%)
        assert_eq!(get("neutral", "git0"), "#b4b4b4");
        assert_eq!(get("neutral", "pie2"), "#555555");
    }

    #[test]
    fn forest_and_base_theme_colours() {
        // lighten('#cde498', 10) = hsl(78.1578947368, 58.4615384615%, 84.5098039216%)
        assert_eq!(get("forest", "tertiaryColor"), "#e1efc0");
        assert_eq!(get("forest", "primaryTextColor"), "#321b67");
        assert_eq!(get("forest", "lineColor"), "#000000");
        // base: adjust('#fff4dd', {h: -120}) = hsl(-79.4117647059, 100%, 93.3333333333%)
        assert_eq!(get("base", "secondaryColor"), "#f4ddff");
        assert_eq!(get("base", "lineColor"), "#0b0b0b");
    }
}
