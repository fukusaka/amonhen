//! 塗りと文字の設定（vgc の `paint.dart`）。等しさ（`==`）は Dart と同じにする（描画命令の重複をまとめるのに使う）。

use std::hash::{Hash, Hasher};

use super::dart::{self, R};
use super::geometry::{AffineMatrix, Point, Rect, hash_f64, hash_opt_f64};

/// `Color`（ARGB。Dart の int なので負にもなりうる）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color(pub i64);

impl Color {
    pub const OPAQUE_BLACK: Color = Color(0xFF00_0000);

    pub fn from_argb(a: i64, r: i64, g: i64, b: i64) -> Self {
        Color(
            (((a & 0xff) << 24) | ((r & 0xff) << 16) | ((g & 0xff) << 8) | (b & 0xff))
                & 0xFFFF_FFFF,
        )
    }

    /// `Color.fromRGBO`（不透明度は 255 倍して 0 の方へ切る）。
    pub fn from_rgbo(r: i64, g: i64, b: i64, opacity: f64) -> R<Self> {
        let a = dart::to_int(opacity * 255.0)?;
        Ok(Color(
            (((a & 0xff) << 24) | ((r & 0xff) << 16) | ((g & 0xff) << 8) | (b & 0xff))
                & 0xFFFF_FFFF,
        ))
    }

    pub fn with_opacity(self, opacity: f64) -> R<Self> {
        Self::from_rgbo(self.r(), self.g(), self.b(), opacity)
    }

    pub fn r(self) -> i64 {
        (0x00ff_0000 & self.0) >> 16
    }

    pub fn g(self) -> i64 {
        (0x0000_ff00 & self.0) >> 8
    }

    pub fn b(self) -> i64 {
        0x0000_00ff & self.0
    }

    pub fn a(self) -> i64 {
        self.0 >> 24
    }
}

/// `TileMode`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TileMode {
    Clamp = 0,
    Repeated = 1,
    Mirror = 2,
}

/// `GradientUnitMode`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnitMode {
    ObjectBoundingBox,
    UserSpaceOnUse,
    Transformed,
}

/// 線形・放射の違い。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Linear {
        from: Point,
        to: Point,
    },
    Radial {
        center: Point,
        radius: f64,
        focal: Option<Point>,
    },
}

/// `Gradient`（`LinearGradient`・`RadialGradient`）。
#[derive(Clone, Debug)]
pub struct Gradient {
    pub id: String,
    pub kind: GradientKind,
    pub colors: Option<Vec<Color>>,
    pub offsets: Option<Vec<f64>>,
    pub tile_mode: Option<TileMode>,
    pub unit_mode: Option<UnitMode>,
    pub transform: Option<AffineMatrix>,
}

impl PartialEq for Gradient {
    fn eq(&self, o: &Self) -> bool {
        let common = self.id == o.id
            && self.colors == o.colors
            && self.offsets == o.offsets
            && self.tile_mode == o.tile_mode
            && self.unit_mode == o.unit_mode;
        match (self.kind, o.kind) {
            (GradientKind::Linear { from, to }, GradientKind::Linear { from: f2, to: t2 }) => {
                // 線形は transform を比べない（Dart と同じ）
                common && from == f2 && to == t2
            }
            (
                GradientKind::Radial {
                    center,
                    radius,
                    focal,
                },
                GradientKind::Radial {
                    center: c2,
                    radius: r2,
                    focal: f2,
                },
            ) => {
                common
                    && center == c2
                    && radius == r2
                    && focal == f2
                    && self.transform == o.transform
            }
            _ => false,
        }
    }
}

impl Hash for Gradient {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.colors.hash(state);
        if let Some(offsets) = &self.offsets {
            for v in offsets {
                hash_f64(*v, state);
            }
        }
        self.tile_mode.hash(state);
        self.unit_mode.hash(state);
        match self.kind {
            GradientKind::Linear { from, to } => {
                0u8.hash(state);
                for v in [from.x, from.y, to.x, to.y] {
                    hash_f64(v, state);
                }
            }
            GradientKind::Radial {
                center,
                radius,
                focal,
            } => {
                1u8.hash(state);
                for v in [center.x, center.y, radius] {
                    hash_f64(v, state);
                }
                hash_opt_f64(focal.map(|p| p.x), state);
                hash_opt_f64(focal.map(|p| p.y), state);
            }
        }
    }
}

impl Gradient {
    /// `applyBounds`。
    pub fn apply_bounds(&self, bounds: Rect, transform: &AffineMatrix) -> Gradient {
        let mut acc = self.transform.unwrap_or(AffineMatrix::IDENTITY);
        match self.unit_mode.unwrap_or(UnitMode::ObjectBoundingBox) {
            UnitMode::ObjectBoundingBox => {
                acc = transform
                    .translated(bounds.left, bounds.top)
                    .scaled(bounds.width(), Some(bounds.height()))
                    .multiplied(&acc);
            }
            UnitMode::UserSpaceOnUse => acc = transform.multiplied(&acc),
            UnitMode::Transformed => {}
        }
        match self.kind {
            GradientKind::Linear { from, to } => Gradient {
                id: self.id.clone(),
                kind: GradientKind::Linear {
                    from: acc.transform_point(from),
                    to: acc.transform_point(to),
                },
                colors: self.colors.clone(),
                offsets: self.offsets.clone(),
                tile_mode: Some(self.tile_mode.unwrap_or(TileMode::Clamp)),
                unit_mode: Some(UnitMode::Transformed),
                transform: None,
            },
            GradientKind::Radial { .. } => Gradient {
                id: self.id.clone(),
                kind: self.kind,
                colors: self.colors.clone(),
                offsets: self.offsets.clone(),
                tile_mode: Some(self.tile_mode.unwrap_or(TileMode::Clamp)),
                unit_mode: Some(UnitMode::Transformed),
                transform: Some(acc),
            },
        }
    }

    /// `applyProperties`（`href` の先から足りないものを取る）。
    pub fn apply_properties(&self, r: &Gradient) -> Gradient {
        Gradient {
            id: self.id.clone(),
            kind: self.kind,
            colors: self.colors.clone().or_else(|| r.colors.clone()),
            offsets: self.offsets.clone().or_else(|| r.offsets.clone()),
            tile_mode: self.tile_mode.or(r.tile_mode),
            unit_mode: self.unit_mode.or(r.unit_mode),
            transform: self.transform.or(r.transform),
        }
    }
}

/// `BlendMode`（dart:ui の並び）。
pub type BlendMode = u8;
pub const SRC_OVER: BlendMode = 3;
pub const DST_IN: BlendMode = 6;

/// `StrokeCap`・`StrokeJoin` の番号。
pub type Cap = u8;
pub type Join = u8;

/// `Fill`。
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Fill {
    pub color: Color,
    pub shader: Option<Gradient>,
}

impl Fill {
    pub fn color(color: Color) -> Self {
        Self {
            color,
            shader: None,
        }
    }
}

/// `Stroke`。
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub shader: Option<Gradient>,
    pub cap: Option<Cap>,
    pub join: Option<Join>,
    pub miter_limit: Option<f64>,
    pub width: Option<f64>,
}

impl Hash for Stroke {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.color.hash(state);
        self.shader.hash(state);
        self.cap.hash(state);
        self.join.hash(state);
        hash_opt_f64(self.miter_limit, state);
        hash_opt_f64(self.width, state);
    }
}

/// `Paint`。
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    pub blend_mode: BlendMode,
    pub stroke: Option<Stroke>,
    pub fill: Option<Fill>,
    pub blur_x: Option<f64>,
    pub blur_y: Option<f64>,
}

impl Hash for Paint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.blend_mode.hash(state);
        self.stroke.hash(state);
        self.fill.hash(state);
        hash_opt_f64(self.blur_x, state);
        hash_opt_f64(self.blur_y, state);
    }
}

impl Paint {
    pub fn new(blend_mode: Option<BlendMode>, fill: Option<Fill>, stroke: Option<Stroke>) -> Self {
        Self {
            blend_mode: blend_mode.unwrap_or(SRC_OVER),
            stroke,
            fill,
            blur_x: None,
            blur_y: None,
        }
    }
}

/// `TextPosition`。
#[derive(Clone, Debug, PartialEq)]
pub struct TextPosition {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub dx: Option<f64>,
    pub dy: Option<f64>,
    pub reset: bool,
    pub transform: Option<AffineMatrix>,
}

impl Hash for TextPosition {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for v in [self.x, self.y, self.dx, self.dy] {
            hash_opt_f64(v, state);
        }
        self.reset.hash(state);
        self.transform.hash(state);
    }
}

/// `TextConfig`。
#[derive(Clone, Debug, PartialEq)]
pub struct TextConfig {
    pub text: String,
    pub x_anchor_multiplier: f64,
    pub font_family: Option<String>,
    pub font_weight: u8,
    pub font_size: f64,
    pub decoration: u8,
    pub decoration_style: u8,
    pub decoration_color: Color,
}

impl Hash for TextConfig {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
        hash_f64(self.x_anchor_multiplier, state);
        self.font_family.hash(state);
        self.font_weight.hash(state);
        hash_f64(self.font_size, state);
        self.decoration.hash(state);
        self.decoration_style.hash(state);
        self.decoration_color.hash(state);
    }
}

/// `FontWeight` の番号（w100 = 0 … w900 = 8）。
pub const NORMAL_FONT_WEIGHT: u8 = 3;
pub const BOLD_FONT_WEIGHT: u8 = 6;

/// `MaskType`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskType {
    Alpha,
    Luminance,
}
