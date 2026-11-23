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
    let vars = match theme {
        "default" => default_theme(),
        "neutral" => neutral_theme(),
        "dark" => dark_theme(),
        "forest" => forest_theme(),
        "base" => base_theme(),
        _ => return None,
    };
    Some(
        KEYS.iter()
            .map(|&key| {
                let value = vars
                    .get(key)
                    .unwrap_or_else(|| panic!("mermaid theme {theme}: {key} is not set"));
                (key, khroma::to_hex(value))
            })
            .collect(),
    )
}

/// テーマの変数（khroma の文字のまま）。
type Vars = HashMap<&'static str, String>;

use khroma::Ch::{H, L, S};
use khroma::{adjust, darken, invert, lighten};

/// `theme-helpers.js` の `mkBorder`（`darkMode` は偽）。
fn mk_border(color: &str) -> String {
    adjust(color, &[(S, -40.0), (L, -10.0)])
}

/// git0〜git7 のもと（default・forest・base の `this.gitN || …`）。
fn git_seed(primary: &str, secondary: &str, tertiary: &str) -> Vec<String> {
    vec![
        primary.to_string(),
        secondary.to_string(),
        tertiary.to_string(),
        adjust(primary, &[(H, -30.0)]),
        adjust(primary, &[(H, -60.0)]),
        adjust(primary, &[(H, -90.0)]),
        adjust(primary, &[(H, 60.0)]),
        adjust(primary, &[(H, 120.0)]),
    ]
}

fn set_all(vars: &mut Vars, names: &[&'static str], values: Vec<String>) {
    for (name, value) in names.iter().zip(values) {
        vars.insert(name, value);
    }
}

const PIES: [&str; 8] = [
    "pie1", "pie2", "pie3", "pie4", "pie5", "pie6", "pie7", "pie8",
];
const GITS: [&str; 8] = [
    "git0", "git1", "git2", "git3", "git4", "git5", "git6", "git7",
];

/// `theme-default.js`。コンストラクタの終わりで `updateColors` を呼び、`calculate` でもう一度呼ぶ。2 回目は
/// `x || y` の値はそのままだが、git0〜git7 は `darken(…, 25)` をもう一度受ける（Mermaid の既定の gitGraph の
/// 色はこの 2 回分）。
fn default_theme() -> Vars {
    let mut v = Vars::new();
    // コンストラクタ
    let primary = "#ECECFF";
    // secondaryColor は adjust(primary, {h: 120}) のすぐ後で '#ffffde' に置き換わる。
    let secondary = "#ffffde";
    let tertiary = adjust(primary, &[(H, -160.0)]);
    v.insert("primaryColor", primary.into());
    v.insert("secondaryColor", secondary.into());
    v.insert("tertiaryColor", tertiary.clone());
    v.insert("primaryBorderColor", mk_border(primary));
    v.insert("secondaryBorderColor", mk_border(secondary));
    v.insert("tertiaryBorderColor", mk_border(&tertiary));
    v.insert("primaryTextColor", invert(primary));
    v.insert("secondaryTextColor", invert(secondary));
    v.insert("tertiaryTextColor", invert(&tertiary));
    // background は '#f4f4f4' から 'white' に、lineColor・textColor は invert(background) から置き換わる。
    v.insert("background", "white".into());
    v.insert("mainBkg", "#ECECFF".into());
    let second_bkg = "#ffffde";
    v.insert("lineColor", "#333333".into());
    let border1 = "#9370DB";
    let border2 = "#aaaa33";
    v.insert("arrowheadColor", "#333333".into());
    let label_background = "rgba(232,232,232, 0.8)";
    let text_color = "#333";
    v.insert("textColor", text_color.into());
    let actor_text_color = "black";
    v.insert("noteBkgColor", "#fff5ad".into());
    let task_text_dark_color = "black";

    // updateColors（2 回とも同じ値になるもの）
    v.insert("nodeBorder", border1.into());
    v.insert("clusterBkg", second_bkg.into());
    v.insert("clusterBorder", border2.into());
    v.insert("titleColor", text_color.into());
    v.insert("edgeLabelBackground", label_background.into());
    v.insert("noteBorderColor", border2.into());
    v.insert("noteTextColor", actor_text_color.into());
    let pies = vec![
        primary.to_string(),
        secondary.to_string(),
        adjust(&tertiary, &[(L, -40.0)]),
        adjust(primary, &[(L, -10.0)]),
        adjust(secondary, &[(L, -30.0)]),
        adjust(&tertiary, &[(L, -20.0)]),
        adjust(primary, &[(H, 60.0), (L, -20.0)]),
        adjust(primary, &[(H, -60.0), (L, -40.0)]),
    ];
    set_all(&mut v, &PIES, pies);
    v.insert("pieTitleTextColor", task_text_dark_color.into());
    v.insert("pieSectionTextColor", text_color.into());
    v.insert("pieStrokeColor", "black".into());
    v.insert("pieOuterStrokeColor", "black".into());

    // git: 1 回目に darken(…, 25)、2 回目は `this.gitN || …` で 1 回目の値を残してから、もう一度 darken(…, 25)。
    let gits = git_seed(primary, secondary, &tertiary)
        .iter()
        .map(|c| darken(&darken(c, 25.0), 25.0))
        .collect();
    set_all(&mut v, &GITS, gits);
    v
}

/// `theme-neutral.js`。`updateColors` は `calculate` の 1 回だけ。
fn neutral_theme() -> Vars {
    let mut v = Vars::new();
    // コンストラクタ
    let primary = "#eee";
    let contrast = "#707070";
    let secondary = lighten(contrast, 55.0);
    let background = "#ffffff";
    let tertiary = adjust(primary, &[(H, -160.0)]);
    v.insert("primaryColor", primary.into());
    v.insert("secondaryColor", secondary.clone());
    v.insert("background", background.into());
    v.insert("tertiaryColor", tertiary.clone());
    v.insert("primaryBorderColor", mk_border(primary));
    v.insert("secondaryBorderColor", mk_border(&secondary));
    v.insert("tertiaryBorderColor", mk_border(&tertiary));
    v.insert("primaryTextColor", invert(primary));
    v.insert("secondaryTextColor", invert(&secondary));
    v.insert("tertiaryTextColor", invert(&tertiary));
    let text_color = invert(background);
    v.insert("textColor", text_color.clone());
    v.insert("mainBkg", "#eee".into());
    // lineColor は invert(background) から置き換わる。
    v.insert("lineColor", "#666".into());
    let border1 = "#999";
    let text = "#333";
    v.insert("arrowheadColor", "#333333".into());
    v.insert("edgeLabelBackground", "white".into());

    // updateColors
    let second_bkg = lighten(contrast, 55.0);
    let border2 = contrast;
    v.insert("noteBorderColor", "#999".into());
    v.insert("noteBkgColor", "#666".into());
    v.insert("noteTextColor", "#fff".into());
    // cScale1〜cScale8（pieN = cScaleN）
    let c_scale = [
        "#F4F4F4", "#555", "#BBB", "#777", "#999", "#DDD", "#FFF", "#DDD",
    ];
    v.insert("nodeBorder", border1.into());
    v.insert("clusterBkg", second_bkg);
    v.insert("clusterBorder", border2.into());
    v.insert("titleColor", text.into());
    let task_text_dark_color = text;
    set_all(
        &mut v,
        &PIES,
        c_scale.iter().map(|c| c.to_string()).collect(),
    );
    v.insert("pieTitleTextColor", task_text_dark_color.into());
    v.insert("pieSectionTextColor", text_color);
    v.insert("pieStrokeColor", "black".into());
    v.insert("pieOuterStrokeColor", "black".into());
    // git0 = darken(pie1, 25)、git1〜git7 = pie2〜pie8（どれも真なので `||` の右は使わない）。
    let mut gits = vec![darken(c_scale[0], 25.0)];
    gits.extend(c_scale[1..].iter().map(|c| c.to_string()));
    set_all(&mut v, &GITS, gits);
    v
}

/// `theme-dark.js`。`updateColors` は `calculate` の 1 回だけ。
fn dark_theme() -> Vars {
    let mut v = Vars::new();
    // コンストラクタ
    let background = "#333";
    let primary = "#1f2020";
    let secondary = lighten(primary, 16.0);
    let tertiary = adjust(primary, &[(H, -160.0)]);
    v.insert("background", background.into());
    v.insert("primaryColor", primary.into());
    v.insert("secondaryColor", secondary.clone());
    v.insert("tertiaryColor", tertiary.clone());
    v.insert("primaryBorderColor", invert(background));
    let secondary_border = mk_border(&secondary);
    v.insert("secondaryBorderColor", secondary_border.clone());
    v.insert("tertiaryBorderColor", mk_border(&tertiary));
    v.insert("primaryTextColor", invert(primary));
    let secondary_text = invert(&secondary);
    v.insert("secondaryTextColor", secondary_text.clone());
    v.insert("tertiaryTextColor", invert(&tertiary));
    let main_bkg = "#1f2020";
    v.insert("mainBkg", main_bkg.into());
    let main_contrast_color = "lightgrey";
    let border1 = "#ccc";
    let border2 = khroma::rgba(255.0, 255.0, 255.0, 0.25);
    let label_background = "#181818";
    // textColor は invert(background) から置き換わる。
    let text_color = "#ccc";
    v.insert("textColor", text_color.into());
    v.insert("titleColor", "#F9FFFE".into());

    // updateColors
    let second_bkg = lighten(main_bkg, 16.0);
    v.insert("lineColor", main_contrast_color.into());
    v.insert("arrowheadColor", main_contrast_color.into());
    // 終わりの `this.nodeBorder || '#999'` は border1 を残す。
    v.insert("nodeBorder", border1.into());
    v.insert("clusterBkg", second_bkg.clone());
    v.insert("clusterBorder", border2);
    v.insert("edgeLabelBackground", lighten(label_background, 25.0));
    v.insert("noteBorderColor", secondary_border);
    v.insert("noteBkgColor", second_bkg);
    v.insert("noteTextColor", secondary_text);
    // cScale1〜cScale8 の固定の色（pieN = cScaleN）。
    let c_scale = [
        "#0b0000", "#4d1037", "#3f5258", "#4f2f1b", "#6e0a0a", "#3b0048", "#995a01", "#154706",
    ];
    set_all(
        &mut v,
        &PIES,
        c_scale.iter().map(|c| c.to_string()).collect(),
    );
    v.insert("pieTitleTextColor", main_contrast_color.into());
    v.insert("pieSectionTextColor", text_color.into());
    v.insert("pieStrokeColor", "black".into());
    v.insert("pieOuterStrokeColor", "black".into());
    // git0 = lighten(secondaryColor, 20)、gitN = lighten(pie(N+1), 20)（git5・git6 は 10）。
    let mut gits = vec![lighten(&secondary, 20.0)];
    for (i, pie) in c_scale[1..].iter().enumerate() {
        let amount = if i == 4 || i == 5 { 10.0 } else { 20.0 };
        gits.push(lighten(pie, amount));
    }
    set_all(&mut v, &GITS, gits);
    v
}

/// `theme-forest.js`。`updateColors` は `calculate` の 1 回だけ。
fn forest_theme() -> Vars {
    let mut v = Vars::new();
    // コンストラクタ
    let primary = "#cde498";
    let secondary = "#cdffb2";
    // background は '#f4f4f4' から 'white' に置き換わる。
    let background = "white";
    v.insert("background", background.into());
    v.insert("primaryColor", primary.into());
    v.insert("secondaryColor", secondary.into());
    v.insert("mainBkg", "#cde498".into());
    let second_bkg = "#cdffb2";
    let border1 = "#13540c";
    let border2 = "#6eaa49";
    v.insert("arrowheadColor", "green".into());
    let tertiary = lighten("#cde498", 10.0);
    v.insert("tertiaryColor", tertiary.clone());
    v.insert("primaryBorderColor", mk_border(primary));
    v.insert("secondaryBorderColor", mk_border(secondary));
    v.insert("tertiaryBorderColor", mk_border(&tertiary));
    v.insert("primaryTextColor", invert(primary));
    v.insert("secondaryTextColor", invert(secondary));
    // tertiaryTextColor は primaryColor から（もとのまま）。
    v.insert("tertiaryTextColor", invert(primary));
    // lineColor は 'green' から invert(background) に置き換わる。
    v.insert("lineColor", invert(background));
    let text_color = invert(background);
    v.insert("textColor", text_color.clone());
    v.insert("titleColor", "#333".into());
    v.insert("edgeLabelBackground", "#e8e8e8".into());
    let actor_text_color = "black";
    v.insert("noteBkgColor", "#fff5ad".into());
    let task_text_dark_color = "black";

    // updateColors
    v.insert("noteBorderColor", border2.into());
    v.insert("noteTextColor", actor_text_color.into());
    v.insert("nodeBorder", border1.into());
    v.insert("clusterBkg", second_bkg.into());
    v.insert("clusterBorder", border2.into());
    let pies = vec![
        primary.to_string(),
        secondary.to_string(),
        tertiary.clone(),
        adjust(primary, &[(L, -30.0)]),
        adjust(secondary, &[(L, -30.0)]),
        adjust(&tertiary, &[(H, 40.0), (L, -40.0)]),
        adjust(primary, &[(H, 60.0), (L, -10.0)]),
        adjust(primary, &[(H, -60.0), (L, -10.0)]),
    ];
    set_all(&mut v, &PIES, pies);
    v.insert("pieTitleTextColor", task_text_dark_color.into());
    v.insert("pieSectionTextColor", text_color);
    v.insert("pieStrokeColor", "black".into());
    v.insert("pieOuterStrokeColor", "black".into());
    let gits = git_seed(primary, secondary, &tertiary)
        .iter()
        .map(|c| darken(c, 25.0))
        .collect();
    set_all(&mut v, &GITS, gits);
    v
}

/// `theme-base.js`。コンストラクタは基の色だけを置き、`updateColors`（`calculate` の 1 回）がすべてを
/// `x || y` で埋める。
fn base_theme() -> Vars {
    let mut v = Vars::new();
    // コンストラクタ
    let background = "#f4f4f4";
    let primary = "#fff4dd";
    let note_bkg = "#fff5ad";
    v.insert("background", background.into());
    v.insert("primaryColor", primary.into());
    v.insert("noteBkgColor", note_bkg.into());
    v.insert("noteTextColor", "#333".into());

    // updateColors
    let primary_text = "#333";
    v.insert("primaryTextColor", primary_text.into());
    let secondary = adjust(primary, &[(H, -120.0)]);
    let tertiary = adjust(primary, &[(H, 180.0), (L, 5.0)]);
    v.insert("secondaryColor", secondary.clone());
    v.insert("tertiaryColor", tertiary.clone());
    let primary_border = mk_border(primary);
    let tertiary_border = mk_border(&tertiary);
    v.insert("primaryBorderColor", primary_border.clone());
    v.insert("secondaryBorderColor", mk_border(&secondary));
    v.insert("tertiaryBorderColor", tertiary_border.clone());
    v.insert("noteBorderColor", mk_border(note_bkg));
    v.insert("secondaryTextColor", invert(&secondary));
    let tertiary_text = invert(&tertiary);
    v.insert("tertiaryTextColor", tertiary_text.clone());
    v.insert("lineColor", invert(background));
    v.insert("arrowheadColor", invert(background));
    let text_color = primary_text;
    v.insert("textColor", text_color.into());
    v.insert("mainBkg", primary.into());
    v.insert("nodeBorder", primary_border);
    v.insert("clusterBkg", tertiary.clone());
    v.insert("clusterBorder", tertiary_border);
    v.insert("titleColor", tertiary_text);
    v.insert("edgeLabelBackground", secondary.clone());
    // taskTextDarkColor = textColor
    let task_text_dark_color = text_color;
    let pies = vec![
        primary.to_string(),
        secondary.clone(),
        tertiary.clone(),
        adjust(primary, &[(L, -10.0)]),
        adjust(&secondary, &[(L, -10.0)]),
        adjust(&tertiary, &[(L, -10.0)]),
        adjust(primary, &[(H, 60.0), (L, -10.0)]),
        adjust(primary, &[(H, -60.0), (L, -10.0)]),
    ];
    set_all(&mut v, &PIES, pies);
    v.insert("pieTitleTextColor", task_text_dark_color.into());
    v.insert("pieSectionTextColor", text_color.into());
    v.insert("pieStrokeColor", "black".into());
    v.insert("pieOuterStrokeColor", "black".into());
    let gits = git_seed(primary, &secondary, &tertiary)
        .iter()
        .map(|c| darken(c, 25.0))
        .collect();
    set_all(&mut v, &GITS, gits);
    v
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

    /// テーマが使う CSS の色の名前（khroma は 16 進に直してから読む）。
    const KEYWORDS: &[(&str, &str)] = &[
        ("black", "#000000"),
        ("gray", "#808080"),
        ("green", "#008000"),
        ("grey", "#808080"),
        ("lightgray", "#d3d3d3"),
        ("lightgrey", "#d3d3d3"),
        ("navy", "#000080"),
        ("red", "#ff0000"),
        ("transparent", "#00000000"),
        ("white", "#ffffff"),
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

    fn parse(color: &str) -> Channels {
        parse_hex(color)
            .or_else(|| parse_rgb(color))
            .or_else(|| parse_hsl(color))
            .or_else(|| {
                let lower = color.to_ascii_lowercase();
                let (_, hex) = KEYWORDS.iter().find(|(name, _)| *name == lower)?;
                parse_hex(hex)
            })
            .unwrap_or_else(|| panic!("Unsupported color format: \"{color}\""))
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
