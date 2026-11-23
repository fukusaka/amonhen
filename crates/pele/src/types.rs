//! 描く手の選びと結果の形（pele の `src/types.ts`）。

use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::text::measurer::TextMeasurer;

/// ホストのアイコン。名前から SVG の中身を返す。
pub type IconResolver = Rc<dyn Fn(&str) -> Option<String>>;

/// 組版した数式。大きさは画素。HTML は組版の CSS が要る。
#[derive(Clone, Debug, PartialEq)]
pub enum MathMarkup {
    Svg {
        svg: String,
        width: f64,
        height: f64,
    },
    Html {
        html: String,
        width: f64,
        height: f64,
    },
}

impl MathMarkup {
    pub fn width(&self) -> f64 {
        match self {
            MathMarkup::Svg { width, .. } | MathMarkup::Html { width, .. } => *width,
        }
    }

    pub fn height(&self) -> f64 {
        match self {
            MathMarkup::Svg { height, .. } | MathMarkup::Html { height, .. } => *height,
        }
    }
}

/// TeX（`$$` の囲みは除く）と文字の大きさ（画素）から数式を組む。
pub type MathResolver = Rc<dyn Fn(&str, f64) -> Option<MathMarkup>>;

/// 時間帯。gantt の地方時を決める（JS の `Date` がホストの時間帯で数えるのと同じ）。
pub trait TimeZone: Send + Sync {
    /// その時刻（1970 年からの UTC のミリ秒）での、地方時と UTC の差（ミリ秒、地方時 − UTC）。
    fn offset_ms(&self, utc_ms: f64) -> f64;
}

/// `RenderOptions`。どれも pele と同じく省ける（None で pele の既定）。
#[derive(Clone, Default)]
pub struct RenderOptions {
    pub measurer: Option<Rc<dyn TextMeasurer>>,
    /// ラベルを測る書体。ページが --pele-font・--pele-font-mono を決めないときは、この書体で描く。
    pub font_family: Option<String>,
    pub font_family_mono: Option<String>,
    pub monospace: Option<Vec<String>>,
    pub font_size: Option<f64>,
    pub id_prefix: Option<String>,
    pub responsive: Option<bool>,
    /// ホストが図に使える幅（画素）。狭く描ける種類は狭く描き、文字が一緒に縮まないようにする。
    pub max_width: Option<f64>,
    /// 横に流れる図を、`max_width` に収まらないとき縦に流して描くか、とその幅の境（既定は描く、640 画素未満）。
    pub auto_direction: Option<bool>,
    pub direction_breakpoint: Option<f64>,
    pub padding: Option<f64>,
    pub limit: Option<usize>,
    pub output_limit: Option<usize>,
    /// flowchart の辺の数の上限。短い 1 行が何百万もの辺を求めうる。
    pub max_edges: Option<usize>,
    /// リンクや画像に、相対の住所のほかに使えるスキーム。
    pub link_schemes: Option<Vec<String>>,
    pub image_schemes: Option<Vec<String>>,
    pub links: Option<bool>,
    pub images: Option<bool>,
    /// リンクの `rel`。既定は `noopener`、空の文字列なら書かない。
    pub link_rel: Option<String>,
    /// gantt が今とみなす時刻（1970 年からのミリ秒）。既定は今の時刻。
    pub now: Option<f64>,
    /// gantt が日付を数える地方時の時間帯。None は UTC（どの機械でも同じ図）。
    pub time_zone: Option<Arc<dyn TimeZone>>,
    pub icons: Option<IconResolver>,
    pub math: Option<MathResolver>,
    pub config: Option<Config>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkInfo {
    pub id: String,
    pub href: String,
    pub internal: bool,
}

/// 図の種類が描いたもの。
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub links: Vec<LinkInfo>,
}

/// ホストの決める上限。図の設定では変えられない。
#[derive(Clone, Copy, Debug)]
pub struct ParseLimits {
    pub max_edges: usize,
}

/// 図の種類が備えるもの（pele の `Diagram`）。`source` は前処理と符号の隠しを通り、Mermaid の読み手が待つとおり
/// 改行で終わる。読んだ形は種類ごとなので `Any` で渡す。
pub struct Diagram {
    pub type_: &'static str,
    /// Mermaid の設定でのこの種類の節の名前（種類の名前と違うとき）。
    pub section: Option<&'static str>,
    /// 元の位置を知らせる種類には、`%%` のコメントの行を残した文字列を渡す。
    pub keep_comments: bool,
    /// ラベルで `$$…$$` を使える。
    pub math: bool,
    #[allow(clippy::type_complexity)]
    pub parse: fn(&str, &Config, Option<&str>, usize, &ParseLimits) -> Result<Box<dyn Any>>,
    pub render: fn(&dyn Any, &Config, &RenderOptions) -> Result<Rendered>,
}
