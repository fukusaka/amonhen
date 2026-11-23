//! 参照を解いて、描くだけの木にする（vgc の `svg/resolver.dart` の `ResolvingVisitor`）。

use std::collections::HashSet;
use std::rc::Rc;

use super::dart::{self, Error, R};
use super::geometry::{AffineMatrix, Path, Rect};
use super::node::{
    Defs, FilterLayer, MAX_REFERENCE_EXPANSIONS, MAX_REFERENCE_EXPANSIONS_MESSAGE, Node,
    ParentKind, ParentNode, PathNode, SvgAttributes,
};
use super::paint::{Color, Fill, Gradient, MaskType, Paint, Stroke, TextConfig, TextPosition};

/// 解いた後の木（`ResolvedPathNode` など。親は子を並べるだけ）。
#[derive(Debug)]
pub enum RNode {
    Empty,
    Group(Vec<RNode>),
    Viewport {
        width: f64,
        height: f64,
        children: Vec<RNode>,
    },
    SaveLayer {
        paint: Paint,
        children: Vec<RNode>,
    },
    TextPosition {
        position: TextPosition,
        children: Vec<RNode>,
    },
    Text {
        config: TextConfig,
        paint: Paint,
    },
    Path {
        paint: Paint,
        path: Path,
    },
    Clip {
        clips: Vec<Path>,
        child: Box<RNode>,
    },
    Mask {
        child: Box<RNode>,
        mask: Box<RNode>,
        blend_mode: Option<u8>,
        mask_type: Option<MaskType>,
    },
    Image {
        data: Rc<Vec<u8>>,
        format: u8,
        rect: Rect,
        transform: Option<AffineMatrix>,
    },
    Pattern {
        child: Box<RNode>,
        pattern: Box<RNode>,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        transform: AffineMatrix,
        id: String,
    },
}

/// `ResolvingVisitor`。
pub struct Resolver<'d> {
    defs: &'d Defs,
    bounds: Rect,
    current_layer: Option<FilterLayer>,
    active_masks: HashSet<String>,
    active_deferred: HashSet<String>,
    active_patterns: HashSet<String>,
    expansions: usize,
    current_text_position: Option<TextPosition>,
}

impl<'d> Resolver<'d> {
    pub fn new(defs: &'d Defs) -> Self {
        Self {
            defs,
            bounds: Rect::ZERO,
            current_layer: None,
            active_masks: HashSet::new(),
            active_deferred: HashSet::new(),
            active_patterns: HashSet::new(),
            expansions: 0,
            current_text_position: None,
        }
    }

    fn count_expansion(&mut self) -> R<()> {
        self.expansions += 1;
        if self.expansions > MAX_REFERENCE_EXPANSIONS {
            return Err(Error::state(MAX_REFERENCE_EXPANSIONS_MESSAGE));
        }
        Ok(())
    }

    fn filter(&self, attributes: &SvgAttributes) -> Option<&'d Vec<FilterLayer>> {
        let id = attributes.filter_id.as_ref()?;
        self.defs.filters.get(id)
    }

    pub fn visit(&mut self, node: &Node, data: &AffineMatrix) -> R<RNode> {
        match node {
            Node::Clip(n) => {
                let child_transform = node.concat_transform(data);
                let clips: Vec<Path> = self
                    .defs
                    .get_clip_path(&n.clip_id)?
                    .iter()
                    .map(|c| c.transformed(&child_transform))
                    .collect();
                if clips.is_empty() {
                    return self.visit(&n.child, data);
                }
                Ok(RNode::Clip {
                    clips,
                    child: Box::new(self.visit(&n.child, data)?),
                })
            }
            Node::Mask(n) => {
                self.count_expansion()?;
                if !self.active_masks.insert(n.mask_id.clone()) {
                    return self.visit(&n.child, data);
                }
                let r = self.visit_mask(node, &n.mask_id, &n.child, n.blend_mode, data);
                self.active_masks.remove(&n.mask_id);
                r
            }
            Node::Parent(p) => match p.kind {
                ParentKind::Viewport { width, height } => {
                    self.bounds = Rect::from_ltwh(0.0, 0.0, width, height);
                    let transform = node.concat_transform(data);
                    let children = p.children.borrow().clone();
                    let mut out = Vec::with_capacity(children.len());
                    for child in &children {
                        out.push(
                            self.visit(&child.apply_attributes(&p.attributes, false), &transform)?,
                        );
                    }
                    Ok(RNode::Viewport {
                        width,
                        height,
                        children: out,
                    })
                }
                ParentKind::TextPosition { reset } => self.visit_text_position(p, reset, data),
                ParentKind::Plain => self.visit_parent(p, data),
            },
            Node::Path(p) => self.visit_path(p, data),
            Node::Deferred(d) => {
                self.count_expansion()?;
                if !self.active_deferred.insert(d.ref_id.clone()) {
                    return Ok(RNode::Empty);
                }
                let r = match self.defs.get_drawable(&d.ref_id) {
                    None => Ok(RNode::Empty),
                    Some(resolved) => {
                        let concrete = resolved.apply_attributes(&d.attributes, true);
                        self.visit(&concrete, data)
                    }
                };
                self.active_deferred.remove(&d.ref_id);
                r
            }
            Node::Text(t) => {
                let paint = t.compute_paint(self.defs, self.bounds, data)?;
                let config = t.compute_text_config();
                let Some(paint) = paint else {
                    return Ok(RNode::Empty);
                };
                if dart::trim(&config.text).is_empty() {
                    return Ok(RNode::Empty);
                }
                if let Some(layer) = self.current_layer
                    && (layer.is_source_alpha || layer.sigma_x > 0.0 || layer.sigma_y > 0.0)
                {
                    return Ok(RNode::Text {
                        config,
                        paint: self.layer_paint(&paint, &layer, data)?,
                    });
                }
                Ok(RNode::Text { config, paint })
            }
            Node::Image(i) => {
                let child_transform = node.concat_transform(data);
                let raw = &i.attributes.raw;
                let left = dart::parse_double(raw.get("x").unwrap_or("0"))?;
                let top = dart::parse_double(raw.get("y").unwrap_or("0"))?;
                let mut width = dart::try_parse_double(raw.get("width").unwrap_or(""));
                let mut height = dart::try_parse_double(raw.get("height").unwrap_or(""));
                if width.is_none() || height.is_none() {
                    let (w, h) = image_size(&i.data)?;
                    width = width.or(Some(w));
                    height = height.or(Some(h));
                }
                let rect = Rect::from_ltwh(
                    left,
                    top,
                    width.unwrap_or_default(),
                    height.unwrap_or_default(),
                );
                if child_transform.encodable_in_rect() {
                    return Ok(RNode::Image {
                        data: i.data.clone(),
                        format: i.format,
                        rect: child_transform.transform_rect(rect),
                        transform: None,
                    });
                }
                Ok(RNode::Image {
                    data: i.data.clone(),
                    format: i.format,
                    rect,
                    transform: Some(child_transform),
                })
            }
            Node::Pattern(n) => {
                self.count_expansion()?;
                if !self.active_patterns.insert(n.pattern_id.clone()) {
                    return self.visit(&n.child, data);
                }
                let r = self.visit_pattern(node, &n.pattern_id, &n.child, data);
                self.active_patterns.remove(&n.pattern_id);
                r
            }
        }
    }

    fn visit_mask(
        &mut self,
        node: &Node,
        mask_id: &str,
        child: &Node,
        blend_mode: Option<u8>,
        data: &AffineMatrix,
    ) -> R<RNode> {
        let Some(resolved) = self.defs.get_drawable(mask_id) else {
            return self.visit(child, data);
        };
        let child = self.visit(child, data)?;
        let child_transform = node.concat_transform(data);
        let mask = self.visit(&resolved, &child_transform)?;
        Ok(RNode::Mask {
            child: Box::new(child),
            mask: Box::new(mask),
            blend_mode,
            mask_type: resolved.attributes().and_then(|a| a.mask_type),
        })
    }

    fn visit_pattern(
        &mut self,
        node: &Node,
        pattern_id: &str,
        child: &Node,
        data: &AffineMatrix,
    ) -> R<RNode> {
        let Some(resolved) = self.defs.get_drawable(pattern_id) else {
            return self.visit(child, data);
        };
        let child = self.visit(child, data)?;
        let child_transform = node.concat_transform(data);
        let pattern = self.visit(&resolved, &child_transform)?;
        let a = resolved
            .attributes()
            .cloned()
            .unwrap_or_else(SvgAttributes::empty);
        Ok(RNode::Pattern {
            child: Box::new(child),
            pattern: Box::new(pattern),
            x: a.x
                .map_or(0.0, |v| v.calculate(0.0, super::node::DEFAULT_FONT_SIZE)),
            y: a.y
                .map_or(0.0, |v| v.calculate(0.0, super::node::DEFAULT_FONT_SIZE)),
            width: a.width.ok_or_else(Error::null_check)?,
            height: a.height.ok_or_else(Error::null_check)?,
            transform: *data,
            id: pattern_id.to_owned(),
        })
    }

    fn visit_parent(&mut self, p: &Rc<ParentNode>, data: &AffineMatrix) -> R<RNode> {
        let next = Node::Parent(p.clone()).concat_transform(data);
        if let Some(layers) = self.filter(&p.attributes)
            && !layers.is_empty()
        {
            if layers.len() == 1 {
                return self.parent_for_layer(p, &next, &layers[0], false);
            }
            let outer = p.create_layer_paint(self.defs, None, None)?;
            let mut children = Vec::new();
            for layer in layers {
                children.push(self.parent_for_layer(p, &next, layer, outer.is_some())?);
            }
            return Ok(match outer {
                Some(paint) => RNode::SaveLayer { paint, children },
                None => RNode::Group(children),
            });
        }
        let paint = p.create_layer_paint(self.defs, None, None)?;
        let children = p.children.borrow().clone();
        let mut out = Vec::with_capacity(children.len());
        match paint {
            None => {
                for child in &children {
                    out.push(self.visit(&child.apply_attributes(&p.attributes, false), &next)?);
                }
                Ok(RNode::Group(out))
            }
            Some(paint) => {
                let attributes = p.attributes.for_save_layer();
                for child in &children {
                    out.push(self.visit(&child.apply_attributes(&attributes, false), &next)?);
                }
                Ok(RNode::SaveLayer {
                    paint,
                    children: out,
                })
            }
        }
    }

    fn parent_for_layer(
        &mut self,
        p: &ParentNode,
        next: &AffineMatrix,
        layer: &FilterLayer,
        ignore_save_layer: bool,
    ) -> R<RNode> {
        let layer_transform = if layer.dx == 0.0 && layer.dy == 0.0 {
            *next
        } else {
            next.translated(layer.dx, layer.dy)
        };
        let has_blur = layer.sigma_x > 0.0 || layer.sigma_y > 0.0;
        let previous = self.current_layer;
        self.current_layer = Some(FilterLayer {
            is_source_alpha: layer.is_source_alpha || previous.is_some_and(|l| l.is_source_alpha),
            sigma_x: 0.0,
            sigma_y: 0.0,
            dx: 0.0,
            dy: 0.0,
        });
        let r = (|| {
            let effective = if ignore_save_layer {
                p.attributes.for_save_layer()
            } else {
                (*p.attributes).clone()
            };
            let paint = if ignore_save_layer {
                has_blur.then(|| {
                    let mut paint = Paint::new(None, Some(Fill::color(Color::OPAQUE_BLACK)), None);
                    paint.blur_x = Some(layer.sigma_x * next.x_scale());
                    paint.blur_y = Some(layer.sigma_y * next.y_scale());
                    paint
                })
            } else {
                p.create_layer_paint(
                    self.defs,
                    has_blur.then(|| layer.sigma_x * next.x_scale()),
                    has_blur.then(|| layer.sigma_y * next.y_scale()),
                )?
            };
            let children = p.children.borrow().clone();
            let mut out = Vec::with_capacity(children.len());
            match paint {
                None => {
                    for child in &children {
                        out.push(
                            self.visit(
                                &child.apply_attributes(&effective, false),
                                &layer_transform,
                            )?,
                        );
                    }
                    Ok(RNode::Group(out))
                }
                Some(paint) => {
                    let attributes = effective.for_save_layer();
                    for child in &children {
                        out.push(self.visit(
                            &child.apply_attributes(&attributes, false),
                            &layer_transform,
                        )?);
                    }
                    Ok(RNode::SaveLayer {
                        paint,
                        children: out,
                    })
                }
            }
        })();
        self.current_layer = previous;
        r
    }

    fn visit_path(&mut self, p: &PathNode, data: &AffineMatrix) -> R<RNode> {
        let transform = data.multiplied(&p.attributes.transform);
        let fill_type = p.attributes.fill_rule.unwrap_or(p.path.fill_type);
        let transformed = p.path.transformed(&transform).with_fill_type(fill_type);
        let original_bounds = p.path.bounds();
        let new_bounds = transformed.bounds();
        if let Some(layers) = self.filter(&p.attributes)
            && !layers.is_empty()
        {
            let Some(original) = p.compute_paint(self.defs, original_bounds, &transform)? else {
                return Ok(RNode::Empty);
            };
            if layers.len() == 1 {
                return self.path_for_layer(
                    p,
                    &original,
                    &transform,
                    &transformed,
                    new_bounds,
                    &layers[0],
                );
            }
            let mut children = Vec::new();
            for layer in layers {
                children.push(self.path_for_layer(
                    p,
                    &original,
                    &transform,
                    &transformed,
                    new_bounds,
                    layer,
                )?);
            }
            return Ok(RNode::Group(children));
        }
        if let Some(layer) = self.current_layer
            && (layer.is_source_alpha || layer.sigma_x > 0.0 || layer.sigma_y > 0.0)
        {
            let Some(original) = p.compute_paint(self.defs, original_bounds, &transform)? else {
                return Ok(RNode::Empty);
            };
            return self.path_for_layer(p, &original, &transform, &transformed, new_bounds, &layer);
        }
        match p.compute_paint(self.defs, original_bounds, &transform)? {
            Some(paint) => Ok(resolve_path(p, paint, transformed)),
            None => Ok(RNode::Empty),
        }
    }

    fn path_for_layer(
        &mut self,
        p: &PathNode,
        original: &Paint,
        transform: &AffineMatrix,
        transformed: &Path,
        _new_bounds: Rect,
        layer: &FilterLayer,
    ) -> R<RNode> {
        let path = if layer.dx == 0.0 && layer.dy == 0.0 {
            transformed.clone()
        } else {
            p.path
                .transformed(&transform.translated(layer.dx, layer.dy))
                .with_fill_type(p.attributes.fill_rule.unwrap_or(p.path.fill_type))
        };
        let paint = self.layer_paint(original, layer, transform)?;
        Ok(resolve_path(p, paint, path))
    }

    fn visit_text_position(
        &mut self,
        p: &Rc<ParentNode>,
        reset: bool,
        data: &AffineMatrix,
    ) -> R<RNode> {
        let node = Node::Parent(p.clone());
        let next = node.concat_transform(data);
        let parent_position = self.current_text_position.clone();
        if let Some(layers) = self.filter(&p.attributes)
            && !layers.is_empty()
        {
            let previous = self.current_layer;
            let one = layers.len() == 1;
            let r = (|| {
                let mut children = Vec::new();
                for layer in layers {
                    self.current_layer = Some(FilterLayer {
                        is_source_alpha: layer.is_source_alpha
                            || previous.is_some_and(|l| l.is_source_alpha),
                        sigma_x: layer.sigma_x,
                        sigma_y: layer.sigma_y,
                        dx: 0.0,
                        dy: 0.0,
                    });
                    let layer_data = data.translated(layer.dx, layer.dy);
                    let layer_next = node.concat_transform(&layer_data);
                    let position = p.compute_text_position(
                        reset,
                        self.bounds,
                        &layer_data,
                        parent_position.as_ref(),
                    );
                    self.current_text_position = Some(position.clone());
                    let kids = p.children.borrow().clone();
                    let mut out = Vec::with_capacity(kids.len());
                    for child in &kids {
                        out.push(
                            self.visit(&child.apply_attributes(&p.attributes, false), &layer_next)?,
                        );
                    }
                    let resolved = RNode::TextPosition {
                        position,
                        children: out,
                    };
                    if one {
                        return Ok(resolved);
                    }
                    children.push(resolved);
                }
                Ok(RNode::Group(children))
            })();
            self.current_layer = previous;
            self.current_text_position = parent_position;
            return r;
        }
        let position = p.compute_text_position(reset, self.bounds, data, parent_position.as_ref());
        self.current_text_position = Some(position.clone());
        let r = (|| {
            let kids = p.children.borrow().clone();
            let mut out = Vec::with_capacity(kids.len());
            for child in &kids {
                out.push(self.visit(&child.apply_attributes(&p.attributes, false), &next)?);
            }
            Ok(RNode::TextPosition {
                position,
                children: out,
            })
        })();
        self.current_text_position = parent_position;
        r
    }

    /// `_blackenGradient`（色を黒にして、不透明度だけを残す）。
    fn blacken(gradient: &Gradient) -> Gradient {
        let Some(colors) = &gradient.colors else {
            return gradient.clone();
        };
        Gradient {
            colors: Some(
                colors
                    .iter()
                    .map(|c| Color::from_argb(c.a(), 0, 0, 0))
                    .collect(),
            ),
            ..gradient.clone()
        }
    }

    /// `_createLayerPaint`。
    fn layer_paint(
        &self,
        original: &Paint,
        layer: &FilterLayer,
        transform: &AffineMatrix,
    ) -> R<Paint> {
        let has_blur = layer.sigma_x > 0.0 || layer.sigma_y > 0.0;
        let source_alpha =
            layer.is_source_alpha || self.current_layer.is_some_and(|l| l.is_source_alpha);
        let fill = if source_alpha {
            original.fill.as_ref().map(|f| Fill {
                color: Color::from_argb(f.color.a(), 0, 0, 0),
                shader: f.shader.as_ref().map(Self::blacken),
            })
        } else {
            original.fill.clone()
        };
        let stroke = if source_alpha {
            original.stroke.as_ref().map(|s| Stroke {
                color: Color::from_argb(s.color.a(), 0, 0, 0),
                shader: s.shader.as_ref().map(Self::blacken),
                width: s.width,
                cap: s.cap,
                join: s.join,
                miter_limit: s.miter_limit,
            })
        } else {
            original.stroke.clone()
        };
        let mut paint = Paint::new(Some(original.blend_mode), fill, stroke);
        paint.blur_x = has_blur.then(|| layer.sigma_x * transform.x_scale());
        paint.blur_y = has_blur.then(|| layer.sigma_y * transform.y_scale());
        Ok(paint)
    }
}

/// `_resolvePathNode`（破線なら塗りと線を分ける）。
fn resolve_path(p: &PathNode, paint: Paint, path: Path) -> RNode {
    let dash = p
        .attributes
        .stroke
        .as_ref()
        .and_then(|s| s.dash_array.as_ref());
    let Some(dash) = dash else {
        return RNode::Path { paint, path };
    };
    let mut children = Vec::new();
    if paint.fill.is_some() {
        let mut fill = Paint::new(Some(paint.blend_mode), paint.fill.clone(), None);
        fill.blur_x = paint.blur_x;
        fill.blur_y = paint.blur_y;
        children.push(RNode::Path {
            paint: fill,
            path: path.clone(),
        });
    }
    if paint.stroke.is_some() {
        let mut stroke = Paint::new(Some(paint.blend_mode), None, paint.stroke.clone());
        stroke.blur_x = paint.blur_x;
        stroke.blur_y = paint.blur_y;
        children.push(RNode::Path {
            paint: stroke,
            path: path.dashed(dash),
        });
    }
    RNode::Group(children)
}

/// `ImageSizeData.fromBytes`（幅・高さ）。
fn image_size(bytes: &[u8]) -> R<(f64, f64)> {
    if bytes.is_empty() {
        return Err(Error::argument("bytes was empty"));
    }
    let n = bytes.len();
    let u16be = |i: usize| -> R<f64> {
        bytes
            .get(i..i + 2)
            .map(|b| f64::from(u16::from_be_bytes([b[0], b[1]])))
            .ok_or_else(|| Error::index(i as i64, n as i64))
    };
    let u16le = |i: usize| -> R<f64> {
        bytes
            .get(i..i + 2)
            .map(|b| f64::from(u16::from_le_bytes([b[0], b[1]])))
            .ok_or_else(|| Error::index(i as i64, n as i64))
    };
    let u32be = |i: usize| -> R<f64> {
        bytes
            .get(i..i + 4)
            .map(|b| f64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])))
            .ok_or_else(|| Error::index(i as i64, n as i64))
    };
    let i32le = |i: usize| -> R<f64> {
        bytes
            .get(i..i + 4)
            .map(|b| f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])))
            .ok_or_else(|| Error::index(i as i64, n as i64))
    };
    if n > 20 && bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return Ok((u32be(16)?, u32be(20)?));
    }
    if n > 8 && &bytes[..4] == b"GIF8" && (bytes[4] == 0x37 || bytes[4] == 0x39) && bytes[5] == 0x61
    {
        return Ok((u16le(6)?, u16le(8)?));
    }
    if n > 12 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        let mut index = 4 + u16be(4)? as usize;
        while index < n {
            if bytes[index] != 0xFF {
                return Err(Error::state("Invalid JPEG file"));
            }
            let marker = *bytes
                .get(index + 1)
                .ok_or_else(|| Error::index(index as i64 + 1, n as i64))?;
            if matches!(marker, 0xC0..=0xC2) {
                return Ok((u16be(index + 7)?, u16be(index + 5)?));
            }
            index += 2;
            index += u16be(index)? as usize;
        }
        return Err(Error::state("Invalid JPEG"));
    }
    if n > 28 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok((u16le(26)?, u16le(28)?));
    }
    if n > 22 && bytes[0] == 0x42 && bytes[1] == 0x4D {
        return Ok((i32le(18)?, i32le(22)?));
    }
    Err(Error::argument("unknown image type"))
}
