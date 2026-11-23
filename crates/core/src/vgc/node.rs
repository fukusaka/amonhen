//! 読んだ SVG の木（vgc の `svg/node.dart`）と、要素の属性（`parser.dart` の `SvgAttributes` など）、id の参照の
//! 解決（`parser.dart` の `_Resolver`）。
//!
//! 親の要素（`ParentNode`）は、読んでいる途中で id の表（`Defs`）にも入り、その後も子が足される（Dart の参照の
//! 共有と同じ）ので、`Rc` と `RefCell` で持つ。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::dart::{Error, R};
use super::geometry::{AffineMatrix, FillType, Path, PathBuilder, Point, Rect};
use super::paint::{
    BlendMode, Color, Fill, Gradient, MaskType, NORMAL_FONT_WEIGHT, Paint, Stroke, TextConfig,
    TextPosition,
};

/// 参照の展開の上限（`kMaxReferenceExpansions`）。
pub const MAX_REFERENCE_EXPANSIONS: usize = 1000;
pub const MAX_REFERENCE_EXPANSIONS_MESSAGE: &str =
    "SVG contains too many nested reference expansions (possible Denial of Service exploit).";
/// 文字の大きさの既定（`kDefaultFontSize`）。
pub const DEFAULT_FONT_SIZE: f64 = 16.0;

/// 属性の表（Dart の `Map<String, String>`。入れた順を保ち、同じ鍵は元の位置で置き換える）。
#[derive(Clone, Debug, Default)]
pub struct AttrMap(pub Vec<(String, String)>);

impl AttrMap {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn insert(&mut self, key: &str, value: String) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
    }

    /// `Map.toString()`。
    pub fn to_dart_string(&self) -> String {
        let items: Vec<String> = self.0.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        format!("{{{}}}", items.join(", "))
    }
}

/// 親から受け継ぐ属性（SVG 1.1）。
const HERITABLE: [&str; 41] = [
    "writing-mode",
    "glyph-orientation-vertical",
    "glyph-orientation-horizontal",
    "direction",
    "text-anchor",
    "font-family",
    "font-style",
    "font-variant",
    "font-weight",
    "font-stretch",
    "font-size",
    "font-size-adjust",
    "font",
    "kerning",
    "letter-spacing",
    "word-spacing",
    "fill",
    "fill-rule",
    "fill-opacity",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-opacity",
    "visibility",
    "marker-start",
    "marker",
    "color-interpolation",
    "color-interpolation-filters",
    "color-rendering",
    "shape-rendering",
    "text-rendering",
    "image-rendering",
    "color",
    "color-profile",
    "clip-rule",
    "pointer-events",
    "cursor",
];

/// `ColorOrNone`。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorOrNone {
    pub is_none: bool,
    pub color: Option<Color>,
}

impl ColorOrNone {
    pub const NONE: ColorOrNone = ColorOrNone {
        is_none: true,
        color: None,
    };

    pub fn color(color: Option<Color>) -> Self {
        Self {
            is_none: false,
            color,
        }
    }

    fn apply_parent(self, parent: Option<ColorOrNone>) -> Self {
        let Some(parent) = parent else {
            return self;
        };
        if self.is_none {
            return self;
        }
        if parent.is_none && self.color.is_none() {
            return Self::NONE;
        }
        Self::color(self.color.or(parent.color))
    }
}

/// `DoubleOrPercentage`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DoubleOrPercentage {
    Pixels(f64),
    Percentage(f64),
    Em(f64),
}

impl DoubleOrPercentage {
    pub fn calculate(self, bound: f64, font_size: f64) -> f64 {
        match self {
            Self::Pixels(v) => v,
            Self::Percentage(v) => v * bound,
            Self::Em(v) => v * font_size,
        }
    }
}

/// `SvgStrokeAttributes`。
#[derive(Clone, Debug, Default)]
pub struct StrokeAttributes {
    pub color: ColorOrNone,
    pub shader_id: Option<String>,
    pub join: Option<u8>,
    pub cap: Option<u8>,
    pub miter_limit: Option<f64>,
    pub width: Option<f64>,
    pub dash_array: Option<Vec<f64>>,
    pub dash_offset: Option<f64>,
    pub has_pattern: Option<bool>,
    pub opacity: Option<f64>,
}

impl StrokeAttributes {
    fn for_save_layer(&self) -> Self {
        Self {
            opacity: None,
            ..self.clone()
        }
    }

    fn apply_parent(&self, parent: Option<&StrokeAttributes>) -> Self {
        Self {
            color: self.color.apply_parent(parent.map(|p| p.color)),
            shader_id: self
                .shader_id
                .clone()
                .or_else(|| parent.and_then(|p| p.shader_id.clone())),
            join: self.join.or(parent.and_then(|p| p.join)),
            cap: self.cap.or(parent.and_then(|p| p.cap)),
            miter_limit: self.miter_limit.or(parent.and_then(|p| p.miter_limit)),
            width: self.width.or(parent.and_then(|p| p.width)),
            dash_array: self
                .dash_array
                .clone()
                .or_else(|| parent.and_then(|p| p.dash_array.clone())),
            dash_offset: self.dash_offset.or(parent.and_then(|p| p.dash_offset)),
            has_pattern: self.has_pattern.or(parent.and_then(|p| p.has_pattern)),
            opacity: self.opacity.or(parent.and_then(|p| p.opacity)),
        }
    }

    /// `toStroke`。
    pub fn to_stroke(
        &self,
        defs: &Defs,
        bounds: Rect,
        transform: &AffineMatrix,
    ) -> R<Option<Stroke>> {
        // 太さ 0 の線は Flutter では細線だが SVG では描かない
        if self.color.is_none
            || (self.color.color.is_none()
                && self.has_pattern.is_none()
                && self.shader_id.is_none()
                || self.width == Some(0.0))
        {
            return Ok(None);
        }
        if self.has_pattern.unwrap_or(false) {
            return Ok(Some(Stroke {
                color: Color::OPAQUE_BLACK,
                shader: None,
                cap: self.cap,
                join: self.join,
                miter_limit: self.miter_limit,
                width: self.width,
            }));
        }
        let mut shader = None;
        if let Some(id) = &self.shader_id {
            match defs.shaders.get(id) {
                Some(g) => shader = Some(g.apply_bounds(bounds, transform)),
                None => return Ok(None),
            }
        }
        let color = match self.opacity {
            None => self.color.color.unwrap_or(Color::OPAQUE_BLACK),
            Some(o) => self
                .color
                .color
                .ok_or_else(Error::null_check)?
                .with_opacity(o)?,
        };
        Ok(Some(Stroke {
            color,
            shader,
            cap: self.cap,
            join: self.join,
            miter_limit: self.miter_limit,
            width: transform.scale_stroke_width(self.width),
        }))
    }
}

/// `SvgFillAttributes`。
#[derive(Clone, Debug, Default)]
pub struct FillAttributes {
    pub color: ColorOrNone,
    pub opacity: Option<f64>,
    pub shader_id: Option<String>,
    pub has_pattern: Option<bool>,
}

impl FillAttributes {
    fn for_save_layer(&self) -> Self {
        Self {
            opacity: None,
            ..self.clone()
        }
    }

    fn apply_parent(&self, parent: Option<&FillAttributes>) -> Self {
        Self {
            color: self.color.apply_parent(parent.map(|p| p.color)),
            shader_id: self
                .shader_id
                .clone()
                .or_else(|| parent.and_then(|p| p.shader_id.clone())),
            has_pattern: self.has_pattern.or(parent.and_then(|p| p.has_pattern)),
            opacity: self.opacity.or(parent.and_then(|p| p.opacity)),
        }
    }

    /// `toFill`。
    pub fn to_fill(
        &self,
        defs: &Defs,
        bounds: Rect,
        transform: &AffineMatrix,
        default_color: Option<Color>,
    ) -> R<Option<Fill>> {
        if self.color.is_none {
            return Ok(None);
        }
        let opacity = self.opacity.unwrap_or(1.0);
        let resolved = match self.color.color {
            Some(c) => Some(c.with_opacity(opacity)?),
            None => match default_color {
                Some(c) => Some(c.with_opacity(opacity)?),
                None => None,
            },
        };
        let Some(resolved) = resolved else {
            return Ok(None);
        };
        if self.has_pattern.unwrap_or(false) {
            return Ok(Some(Fill::color(resolved)));
        }
        let mut shader = None;
        if let Some(id) = &self.shader_id {
            match defs.shaders.get(id) {
                Some(g) => shader = Some(g.apply_bounds(bounds, transform)),
                None => return Ok(None),
            }
        }
        Ok(Some(Fill {
            color: resolved,
            shader,
        }))
    }
}

/// `SvgAttributes`。
#[derive(Clone, Debug)]
pub struct SvgAttributes {
    pub raw: Rc<AttrMap>,
    pub id: Option<String>,
    pub href: Option<String>,
    pub transform: AffineMatrix,
    pub color: ColorOrNone,
    pub stroke: Option<StrokeAttributes>,
    pub fill: Option<FillAttributes>,
    pub fill_rule: Option<FillType>,
    pub clip_rule: Option<FillType>,
    pub clip_path_id: Option<String>,
    pub blend_mode: Option<BlendMode>,
    pub font_family: Option<String>,
    pub font_weight: Option<u8>,
    pub font_size: Option<f64>,
    pub text_decoration: Option<u8>,
    pub text_decoration_style: Option<u8>,
    pub text_decoration_color: Option<Color>,
    pub x: Option<DoubleOrPercentage>,
    pub dx: Option<DoubleOrPercentage>,
    pub text_anchor_multiplier: Option<f64>,
    pub y: Option<DoubleOrPercentage>,
    pub dy: Option<DoubleOrPercentage>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub mask_type: Option<MaskType>,
    pub opacity: Option<f64>,
    pub filter_id: Option<String>,
}

impl Default for SvgAttributes {
    fn default() -> Self {
        Self {
            raw: Rc::default(),
            id: None,
            href: None,
            transform: AffineMatrix::IDENTITY,
            color: ColorOrNone::default(),
            stroke: None,
            fill: None,
            fill_rule: None,
            clip_rule: None,
            clip_path_id: None,
            blend_mode: None,
            font_family: None,
            font_weight: None,
            font_size: None,
            text_decoration: None,
            text_decoration_style: None,
            text_decoration_color: None,
            x: None,
            dx: None,
            text_anchor_multiplier: None,
            y: None,
            dy: None,
            width: None,
            height: None,
            mask_type: None,
            opacity: None,
            filter_id: None,
        }
    }
}

impl SvgAttributes {
    /// `SvgAttributes.empty`。
    pub fn empty() -> Rc<SvgAttributes> {
        thread_local! {
            static EMPTY: Rc<SvgAttributes> = Rc::new(SvgAttributes::default());
        }
        EMPTY.with(Rc::clone)
    }

    /// `forSaveLayer`（不透明度は saveLayer が受け持つので外す）。
    pub fn for_save_layer(&self) -> SvgAttributes {
        SvgAttributes {
            stroke: self.stroke.as_ref().map(StrokeAttributes::for_save_layer),
            fill: self.fill.as_ref().map(FillAttributes::for_save_layer),
            dx: None,
            dy: None,
            opacity: None,
            filter_id: None,
            ..self.clone()
        }
    }

    /// `applyParent`。
    pub fn apply_parent(
        &self,
        parent: &SvgAttributes,
        transform_override: Option<AffineMatrix>,
        filter_id_override: Option<String>,
    ) -> SvgAttributes {
        let mut raw = AttrMap::default();
        for (k, v) in &parent.raw.0 {
            if HERITABLE.contains(&k.as_str()) {
                raw.insert(k, v.clone());
            }
        }
        for (k, v) in &self.raw.0 {
            raw.insert(k, v.clone());
        }
        let id = raw.get("id").map(str::to_owned);
        let href = raw.get("href").map(str::to_owned);
        SvgAttributes {
            raw: Rc::new(raw),
            id,
            href,
            transform: transform_override.unwrap_or(self.transform),
            color: self.color.apply_parent(Some(parent.color)),
            stroke: match &self.stroke {
                Some(s) => Some(s.apply_parent(parent.stroke.as_ref())),
                None => parent.stroke.clone(),
            },
            fill: match &self.fill {
                Some(f) => Some(f.apply_parent(parent.fill.as_ref())),
                None => parent.fill.clone(),
            },
            fill_rule: self.fill_rule.or(parent.fill_rule),
            clip_rule: self.clip_rule.or(parent.clip_rule),
            clip_path_id: self
                .clip_path_id
                .clone()
                .or_else(|| parent.clip_path_id.clone()),
            blend_mode: self.blend_mode.or(parent.blend_mode),
            font_family: self
                .font_family
                .clone()
                .or_else(|| parent.font_family.clone()),
            font_weight: self.font_weight.or(parent.font_weight),
            font_size: self.font_size.or(parent.font_size),
            text_decoration: self.text_decoration.or(parent.text_decoration),
            text_decoration_style: self.text_decoration_style.or(parent.text_decoration_style),
            text_decoration_color: self.text_decoration_color.or(parent.text_decoration_color),
            text_anchor_multiplier: self
                .text_anchor_multiplier
                .or(parent.text_anchor_multiplier),
            height: self.height.or(parent.height),
            width: self.width.or(parent.width),
            x: self.x,
            y: self.y,
            dx: self.dx,
            dy: self.dy,
            mask_type: self.mask_type.or(parent.mask_type),
            opacity: self.opacity,
            filter_id: filter_id_override.or_else(|| self.filter_id.clone()),
        }
    }
}

/// `SvgFilterLayer`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterLayer {
    pub is_source_alpha: bool,
    pub sigma_x: f64,
    pub sigma_y: f64,
    pub dx: f64,
    pub dy: f64,
}

impl FilterLayer {
    pub fn has_effect(&self) -> bool {
        self.sigma_x > 0.0 || self.sigma_y > 0.0 || self.dx != 0.0 || self.dy != 0.0
    }
}

/// 親の要素の種類（`ViewportNode`・`TextPositionNode` も `ParentNode`）。
#[derive(Clone, Copy, Debug)]
pub enum ParentKind {
    Plain,
    Viewport { width: f64, height: f64 },
    TextPosition { reset: bool },
}

/// `ParentNode`。
#[derive(Debug)]
pub struct ParentNode {
    pub attributes: Rc<SvgAttributes>,
    pub transform: AffineMatrix,
    pub kind: ParentKind,
    pub children: RefCell<Vec<Node>>,
}

#[derive(Debug)]
pub struct ClipNode {
    pub clip_id: String,
    pub child: Node,
    pub transform: AffineMatrix,
}

#[derive(Debug)]
pub struct MaskNode {
    pub mask_id: String,
    pub child: Node,
    pub blend_mode: Option<BlendMode>,
    pub transform: AffineMatrix,
}

#[derive(Debug)]
pub struct PatternNode {
    pub pattern_id: String,
    pub child: Node,
    pub transform: AffineMatrix,
}

#[derive(Debug)]
pub struct PathNode {
    pub path: Path,
    pub attributes: Rc<SvgAttributes>,
}

#[derive(Debug)]
pub struct DeferredNode {
    pub ref_id: String,
    pub attributes: Rc<SvgAttributes>,
}

#[derive(Debug)]
pub struct TextNode {
    pub text: String,
    pub attributes: Rc<SvgAttributes>,
}

#[derive(Debug)]
pub struct ImageNode {
    pub data: Rc<Vec<u8>>,
    pub format: u8,
    pub attributes: Rc<SvgAttributes>,
}

/// `Node`（解決の前）。
#[derive(Clone, Debug)]
pub enum Node {
    Parent(Rc<ParentNode>),
    Clip(Rc<ClipNode>),
    Mask(Rc<MaskNode>),
    Pattern(Rc<PatternNode>),
    Path(Rc<PathNode>),
    Deferred(Rc<DeferredNode>),
    Text(Rc<TextNode>),
    Image(Rc<ImageNode>),
}

impl Node {
    pub fn parent(
        attributes: Rc<SvgAttributes>,
        transform: Option<AffineMatrix>,
        kind: ParentKind,
    ) -> Rc<ParentNode> {
        let transform = transform.unwrap_or(attributes.transform);
        Rc::new(ParentNode {
            attributes,
            transform,
            kind,
            children: RefCell::new(Vec::new()),
        })
    }

    /// 属性を持つ要素なら、その属性。
    pub fn attributes(&self) -> Option<&Rc<SvgAttributes>> {
        match self {
            Node::Parent(n) => Some(&n.attributes),
            Node::Path(n) => Some(&n.attributes),
            Node::Deferred(n) => Some(&n.attributes),
            Node::Text(n) => Some(&n.attributes),
            Node::Image(n) => Some(&n.attributes),
            _ => None,
        }
    }

    /// `transform`（`TransformableNode`）。
    pub fn transform(&self) -> AffineMatrix {
        match self {
            Node::Parent(n) => n.transform,
            Node::Clip(n) => n.transform,
            Node::Mask(n) => n.transform,
            Node::Pattern(n) => n.transform,
            Node::Path(n) => n.attributes.transform,
            Node::Deferred(n) => n.attributes.transform,
            Node::Text(n) => n.attributes.transform,
            Node::Image(n) => n.attributes.transform,
        }
    }

    /// `concatTransform`。
    pub fn concat_transform(&self, current: &AffineMatrix) -> AffineMatrix {
        let t = self.transform();
        if t == AffineMatrix::IDENTITY {
            return *current;
        }
        current.multiplied(&t)
    }

    /// `applyAttributes`。
    pub fn apply_attributes(&self, new: &SvgAttributes, replace: bool) -> Node {
        match self {
            Node::Parent(n) => {
                let filter = if replace {
                    new.filter_id
                        .clone()
                        .or_else(|| n.attributes.filter_id.clone())
                } else {
                    None
                };
                let attributes = Rc::new(n.attributes.apply_parent(new, None, filter));
                let (transform, kind) = match n.kind {
                    ParentKind::TextPosition { reset } => {
                        (attributes.transform, ParentKind::TextPosition { reset })
                    }
                    _ => (n.transform, ParentKind::Plain),
                };
                Node::Parent(Rc::new(ParentNode {
                    attributes,
                    transform,
                    kind,
                    children: RefCell::new(n.children.borrow().clone()),
                }))
            }
            Node::Clip(n) => Node::Clip(Rc::new(ClipNode {
                clip_id: n.clip_id.clone(),
                child: n.child.apply_attributes(new, replace),
                transform: n.transform,
            })),
            Node::Mask(n) => Node::Mask(Rc::new(MaskNode {
                mask_id: n.mask_id.clone(),
                child: n.child.apply_attributes(new, replace),
                blend_mode: n.blend_mode,
                transform: n.transform,
            })),
            Node::Pattern(n) => Node::Pattern(Rc::new(PatternNode {
                pattern_id: n.pattern_id.clone(),
                child: n.child.apply_attributes(new, replace),
                transform: n.transform,
            })),
            Node::Path(n) => {
                let attributes = if replace {
                    new.apply_parent(
                        &n.attributes,
                        Some(n.attributes.transform),
                        new.filter_id
                            .clone()
                            .or_else(|| n.attributes.filter_id.clone()),
                    )
                } else {
                    n.attributes.apply_parent(new, None, None)
                };
                Node::Path(Rc::new(PathNode {
                    path: n.path.clone(),
                    attributes: Rc::new(attributes),
                }))
            }
            Node::Deferred(n) => Node::Deferred(Rc::new(DeferredNode {
                ref_id: n.ref_id.clone(),
                attributes: Rc::new(replaced(&n.attributes, new, replace)),
            })),
            Node::Text(n) => Node::Text(Rc::new(TextNode {
                text: n.text.clone(),
                attributes: Rc::new(replaced(&n.attributes, new, replace)),
            })),
            Node::Image(n) => Node::Image(Rc::new(ImageNode {
                data: n.data.clone(),
                format: n.format,
                attributes: Rc::new(replaced(&n.attributes, new, replace)),
            })),
        }
    }
}

fn replaced(own: &SvgAttributes, new: &SvgAttributes, replace: bool) -> SvgAttributes {
    if replace {
        new.apply_parent(own, Some(own.transform), None)
    } else {
        own.apply_parent(new, None, None)
    }
}

impl ParentNode {
    /// `addChild`（clip・mask・pattern で包む）。
    pub fn add_child(
        &self,
        child: Node,
        clip_id: Option<String>,
        mask_id: Option<String>,
        pattern_id: Option<String>,
    ) {
        let attributes = child
            .attributes()
            .cloned()
            .unwrap_or_else(SvgAttributes::empty);
        let mut wrapped = child;
        if let Some(clip_id) = clip_id {
            wrapped = Node::Clip(Rc::new(ClipNode {
                clip_id,
                child: wrapped,
                transform: attributes.transform,
            }));
        }
        if let Some(mask_id) = mask_id {
            wrapped = Node::Mask(Rc::new(MaskNode {
                mask_id,
                child: wrapped,
                blend_mode: attributes.blend_mode,
                transform: attributes.transform,
            }));
        }
        if let Some(pattern_id) = pattern_id {
            wrapped = Node::Pattern(Rc::new(PatternNode {
                pattern_id,
                child: wrapped,
                transform: attributes.transform,
            }));
        }
        self.children.borrow_mut().push(wrapped);
    }

    /// `createLayerPaint`。
    pub fn create_layer_paint(
        &self,
        defs: &Defs,
        blur_x: Option<f64>,
        blur_y: Option<f64>,
    ) -> R<Option<Paint>> {
        let a = &self.attributes;
        let layer_opacity = a.fill.as_ref().and_then(|f| f.opacity).or(a.opacity);
        let needs_layer = a.blend_mode.is_some()
            || (blur_x.is_some() || blur_y.is_some())
            || layer_opacity.is_some_and(|o| o != 1.0 && o != 0.0);
        if !needs_layer {
            return Ok(None);
        }
        let fill = match &a.fill {
            Some(f) => f.to_fill(defs, Rect::LARGEST, &self.transform, None)?,
            None => None,
        };
        let fill = match fill {
            Some(f) => f,
            None => Fill::color(Color::OPAQUE_BLACK.with_opacity(layer_opacity.unwrap_or(1.0))?),
        };
        let mut paint = Paint::new(a.blend_mode, Some(fill), None);
        paint.blur_x = blur_x;
        paint.blur_y = blur_y;
        Ok(Some(paint))
    }

    /// `TextPositionNode.computeTextPosition`。
    pub fn compute_text_position(
        &self,
        reset: bool,
        bounds: Rect,
        transform: &AffineMatrix,
        parent: Option<&TextPosition>,
    ) -> TextPosition {
        let computed = if self.transform == AffineMatrix::IDENTITY {
            *transform
        } else {
            transform.multiplied(&self.transform)
        };
        let a = &self.attributes;
        let font_size = a.font_size.unwrap_or(DEFAULT_FONT_SIZE);
        let mut x = a.x.map(|v| v.calculate(bounds.width(), font_size));
        let mut y = a.y.map(|v| v.calculate(bounds.height(), font_size));
        let mut dx = a.dx.map(|v| v.calculate(bounds.width(), font_size));
        let mut dy = a.dy.map(|v| v.calculate(bounds.height(), font_size));
        let has_xy = x.is_some() && y.is_some();
        let has_dxdy = dx.is_some() && dy.is_some();
        let inherits_consumed = parent.is_some_and(|p| p.transform.is_none())
            && x.is_none()
            && y.is_none()
            && dx.is_none()
            && dy.is_none();
        let consume = computed == AffineMatrix::IDENTITY
            || (computed.encodable_in_rect() && (has_xy || has_dxdy));
        if let (true, Some(px), Some(py)) = (has_xy, x, y) {
            let p = if consume {
                computed.transform_point(Point::new(px, py))
            } else {
                Point::new(px, py)
            };
            x = Some(p.x);
            y = Some(p.y);
        }
        if let (true, Some(px), Some(py)) = (has_dxdy, dx, dy) {
            let p = if consume {
                computed.transform_point(Point::new(px, py))
            } else {
                Point::new(px, py)
            };
            dx = Some(p.x);
            dy = Some(p.y);
        }
        TextPosition {
            x,
            y,
            dx,
            dy,
            reset,
            transform: if inherits_consumed || consume {
                None
            } else {
                Some(computed)
            },
        }
    }
}

impl PathNode {
    /// `computePaint`。
    pub fn compute_paint(
        &self,
        defs: &Defs,
        bounds: Rect,
        transform: &AffineMatrix,
    ) -> R<Option<Paint>> {
        let a = &self.attributes;
        let stroke = match &a.stroke {
            Some(s) => s.to_stroke(defs, bounds, transform)?,
            None => None,
        };
        let fill = match &a.fill {
            Some(f) => f.to_fill(defs, bounds, transform, Some(Color::OPAQUE_BLACK))?,
            None => None,
        };
        if fill.is_none() && stroke.is_none() {
            return Ok(None);
        }
        Ok(Some(Paint::new(a.blend_mode, fill, stroke)))
    }
}

impl TextNode {
    /// `computePaint`（塗りが先）。
    pub fn compute_paint(
        &self,
        defs: &Defs,
        bounds: Rect,
        transform: &AffineMatrix,
    ) -> R<Option<Paint>> {
        let a = &self.attributes;
        let fill = match &a.fill {
            Some(f) => f.to_fill(defs, bounds, transform, Some(Color::OPAQUE_BLACK))?,
            None => None,
        };
        let stroke = match &a.stroke {
            Some(s) => s.to_stroke(defs, bounds, transform)?,
            None => None,
        };
        if fill.is_none() && stroke.is_none() {
            return Ok(None);
        }
        Ok(Some(Paint::new(a.blend_mode, fill, stroke)))
    }

    /// `computeTextConfig`。
    pub fn compute_text_config(&self) -> TextConfig {
        let a = &self.attributes;
        TextConfig {
            text: self.text.clone(),
            x_anchor_multiplier: a.text_anchor_multiplier.unwrap_or(0.0),
            font_family: a.font_family.clone(),
            font_weight: a.font_weight.unwrap_or(NORMAL_FONT_WEIGHT),
            font_size: a.font_size.unwrap_or(DEFAULT_FONT_SIZE),
            decoration: a.text_decoration.unwrap_or(0),
            decoration_style: a.text_decoration_style.unwrap_or(0),
            decoration_color: a.text_decoration_color.unwrap_or(Color::OPAQUE_BLACK),
        }
    }
}

/// id の参照（`_Resolver`）。
#[derive(Default)]
pub struct Defs {
    pub drawables: HashMap<String, Node>,
    pub shaders: HashMap<String, Gradient>,
    pub clips: HashMap<String, Vec<Node>>,
    pub filters: HashMap<String, Vec<FilterLayer>>,
    pub deferred_shaders: HashMap<String, Vec<Gradient>>,
    pub deferred_expansion_count: Cell<usize>,
}

impl Defs {
    pub fn get_drawable(&self, r: &str) -> Option<Node> {
        self.drawables.get(r).cloned()
    }

    pub fn add_drawable(&mut self, r: String, node: Node) {
        self.drawables.entry(r).or_insert(node);
    }

    pub fn add_clip_path(&mut self, r: String, nodes: Vec<Node>) {
        self.clips.entry(r).or_insert(nodes);
    }

    /// `addGradient`（`href` の先が後に出てくるときは、出てきたときに埋める）。
    pub fn add_gradient(&mut self, gradient: Gradient, href: Option<&str>) {
        if self.shaders.contains_key(&gradient.id) {
            return;
        }
        let id = gradient.id.clone();
        match href {
            Some(href) => {
                let href = format!("url({href})");
                self.shaders.insert(id.clone(), gradient.clone());
                match self.shaders.get(&href) {
                    Some(r) => {
                        let applied = gradient.apply_properties(r);
                        self.shaders.insert(id, applied);
                    }
                    None => self
                        .deferred_shaders
                        .entry(href)
                        .or_default()
                        .push(gradient),
                }
            }
            None => {
                self.shaders.insert(id.clone(), gradient.clone());
                for deferred in self.deferred_shaders.remove(&id).unwrap_or_default() {
                    let applied = deferred.apply_properties(&gradient);
                    self.shaders.insert(deferred.id.clone(), applied);
                }
            }
        }
    }

    /// `getClipPath`（clip の子のパスを、規則が変わるたびに分けて集める）。
    pub fn get_clip_path(&self, r: &str) -> R<Vec<Path>> {
        let Some(nodes) = self.clips.get(r) else {
            return Ok(Vec::new());
        };
        let mut builders: Vec<PathBuilder> = Vec::new();
        let mut current: Option<usize> = None;
        let mut active: Vec<String> = Vec::new();
        for node in nodes {
            self.extract_clip(Some(node.clone()), &mut builders, &mut current, &mut active)?;
        }
        Ok(builders.iter_mut().map(PathBuilder::take_path).collect())
    }

    fn extract_clip(
        &self,
        target: Option<Node>,
        builders: &mut Vec<PathBuilder>,
        current: &mut Option<usize>,
        active: &mut Vec<String>,
    ) -> R<()> {
        match target {
            Some(Node::Path(p)) => {
                let mut next = PathBuilder::from_path(&p.path);
                next.fill_type = p.attributes.clip_rule.unwrap_or(FillType::NonZero);
                match *current {
                    Some(i) if next.fill_type != builders[i].fill_type => {
                        builders.push(next);
                        *current = Some(builders.len() - 1);
                    }
                    None => {
                        builders.push(next);
                        *current = Some(builders.len() - 1);
                    }
                    Some(i) => {
                        let path = next.snapshot();
                        builders[i].add_path(&path);
                    }
                }
            }
            Some(Node::Deferred(d)) => {
                let count = self.deferred_expansion_count.get() + 1;
                self.deferred_expansion_count.set(count);
                if count > MAX_REFERENCE_EXPANSIONS {
                    return Err(Error::state(MAX_REFERENCE_EXPANSIONS_MESSAGE));
                }
                if active.contains(&d.ref_id) {
                    return Ok(());
                }
                active.push(d.ref_id.clone());
                let r = self.extract_clip(self.get_drawable(&d.ref_id), builders, current, active);
                active.retain(|id| id != &d.ref_id);
                r?;
            }
            Some(Node::Parent(p)) => {
                let children = p.children.borrow().clone();
                for child in children {
                    self.extract_clip(Some(child), builders, current, active)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
