//! SVG を読んで木にする（vgc の `svg/parser.dart`）。XML のイベントを先頭から読み、要素ごとに木を組む。
//! `_readSubtree` は Dart の generator で、入れ子で同じイベントの列を読む。ここでは「yield の後」の処理を次に
//! 呼ばれたときに回す `Subtree` で同じにする（自己終了の要素を開いたままと見なす vgc の癖も同じになる）。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::dart::{self, Error, R};
use super::geometry::{AffineMatrix, FillType, Path, PathBuilder, Point, Rect};
use super::node::{
    AttrMap, ColorOrNone, DeferredNode, Defs, DoubleOrPercentage, FillAttributes, FilterLayer,
    ImageNode, Node, ParentKind, ParentNode, PathNode, StrokeAttributes, SvgAttributes, TextNode,
};
use super::paint::{
    BOLD_FONT_WEIGHT, Color, Gradient, GradientKind, MaskType, NORMAL_FONT_WEIGHT, TileMode,
    UnitMode,
};
use super::parsers::{
    self, is_em, is_percentage, parse_decimal_or_percentage, parse_double, parse_double_with_units,
    parse_em, parse_percentage, parse_transform, split_list,
};
use super::path_parsing::parse_svg_path_data;
use super::xml::{Event, Events, local_name};

/// 解析の結果。
pub struct Parsed {
    pub root: Rc<ParentNode>,
    pub defs: Defs,
}

/// 開いた要素。
#[derive(Clone)]
struct Start {
    name: String,
    self_closing: bool,
}

/// `_readSubtree` の 1 回分。
struct Subtree {
    start: i64,
    /// 前に返したイベントの「yield の後」の処理（自己終了か終了タグなら深さを戻す）。
    pending: Option<bool>,
    done: bool,
}

/// `SvgParser`。
pub struct SvgParser<'a> {
    events: Events<'a>,
    pub defs: Defs,
    parents: Vec<(String, Rc<ParentNode>)>,
    pattern_ids: HashSet<String>,
    root: Option<Rc<ParentNode>>,
    attributes: Rc<SvgAttributes>,
    start: Option<Start>,
    depth: i64,
    last_end: Option<String>,
    last_text_ended_with_space: bool,
    pub warnings: Vec<String>,
}

/// テーマの currentColor（vgc の `--current-color` の既定）。
const CURRENT_COLOR: Color = Color::OPAQUE_BLACK;

const EMPTY_URL_IRI: &str = "url(#)";

fn is_path_element(name: &str) -> bool {
    matches!(
        name,
        "circle" | "path" | "rect" | "polygon" | "polyline" | "ellipse" | "line"
    )
}

impl<'a> SvgParser<'a> {
    pub fn new(xml: &'a str) -> Self {
        Self {
            events: Events::new(xml),
            defs: Defs::default(),
            parents: Vec::new(),
            pattern_ids: HashSet::new(),
            root: None,
            attributes: SvgAttributes::empty(),
            start: None,
            depth: 0,
            last_end: None,
            last_text_ended_with_space: false,
            warnings: Vec::new(),
        }
    }

    fn subtree(&self) -> Subtree {
        Subtree {
            start: self.depth,
            pending: None,
            done: false,
        }
    }

    fn clear_current(&mut self) {
        self.attributes = SvgAttributes::empty();
        self.start = None;
    }

    /// `_readSubtree` の次のイベント。
    fn next_in(&mut self, st: &mut Subtree) -> R<Option<Event<'a>>> {
        if st.done {
            return Ok(None);
        }
        if let Some(closes) = st.pending.take() {
            if closes {
                self.depth -= 1;
                self.clear_current();
            }
            if self.depth < st.start {
                st.done = true;
                return Ok(None);
            }
        }
        loop {
            let Some(event) = self.events.next_event()? else {
                st.done = true;
                return Ok(None);
            };
            let mut self_closing = false;
            if let Event::Start {
                name,
                attributes,
                self_closing: closes,
            } = &event
            {
                let map = create_attribute_map(attributes)?;
                if !is_visible(&map) {
                    if !closes {
                        self.depth += 1;
                        self.discard_subtree()?;
                    }
                    continue;
                }
                self.start = Some(Start {
                    name: (*name).to_owned(),
                    self_closing: *closes,
                });
                let current_color = (self.depth == 0).then_some(CURRENT_COLOR);
                self.attributes = Rc::new(self.create_svg_attributes(map, current_color)?);
                self.depth += 1;
                self_closing = *closes;
            }
            st.pending = Some(self_closing || matches!(event, Event::End { .. }));
            return Ok(Some(event));
        }
    }

    /// `_discardSubtree`。
    fn discard_subtree(&mut self) -> R<()> {
        let start = self.depth;
        while let Some(event) = self.events.next_event()? {
            match event {
                Event::Start {
                    self_closing: false,
                    ..
                } => self.depth += 1,
                Event::End { .. } => self.depth -= 1,
                _ => {}
            }
            self.clear_current();
            if self.depth < start {
                return Ok(());
            }
        }
        Ok(())
    }

    fn attribute(&self, name: &str) -> Option<String> {
        self.attributes.raw.get(name).map(str::to_owned)
    }

    fn attribute_or(&self, name: &str, def: &str) -> String {
        self.attribute(name).unwrap_or_else(|| def.to_owned())
    }

    fn self_closing(&self) -> bool {
        self.start.as_ref().is_some_and(|s| s.self_closing)
    }

    fn current_group(&self) -> R<Rc<ParentNode>> {
        self.parents
            .last()
            .map(|(_, p)| p.clone())
            .ok_or_else(Error::no_element)
    }

    fn viewport_width(&self) -> Option<f64> {
        self.root.as_ref().and_then(|r| match r.kind {
            ParentKind::Viewport { width, .. } => Some(width),
            _ => None,
        })
    }

    fn viewport_height(&self) -> Option<f64> {
        self.root.as_ref().and_then(|r| match r.kind {
            ParentKind::Viewport { height, .. } => Some(height),
            _ => None,
        })
    }

    fn units(&self, raw: Option<&str>, percentage_ref: Option<f64>) -> R<Option<f64>> {
        parse_double_with_units(raw, false, percentage_ref)
    }

    /// 単位付きの数（`!` 付き。null になれば失敗）。
    fn units_req(&self, raw: &str, percentage_ref: Option<f64>) -> R<f64> {
        self.units(Some(raw), percentage_ref)?
            .ok_or_else(Error::null_check)
    }

    /// `buildUrlIri`（id がなければ `url(#null)`。Dart の文字列の埋め込みと同じ）。
    fn build_url_iri(&self) -> String {
        format!("url(#{})", self.attributes.id.as_deref().unwrap_or("null"))
    }

    fn check_for_iri(&mut self, node: Node) {
        let iri = self.build_url_iri();
        if iri != EMPTY_URL_IRI {
            self.defs.add_drawable(iri, node);
        }
    }

    fn add_group(&mut self, name: String, drawable: Rc<ParentNode>) {
        self.parents.push((name, drawable.clone()));
        self.check_for_iri(Node::Parent(drawable));
    }

    /// `getPattern`。
    fn get_pattern(&self) -> Option<String> {
        if let Some(fill) = self.attribute("fill")
            && fill.starts_with("url")
            && self.pattern_ids.contains(&fill)
        {
            return Some(fill);
        }
        if let Some(stroke) = self.attribute("stroke")
            && stroke.starts_with("url")
            && self.pattern_ids.contains(&stroke)
        {
            return Some(stroke);
        }
        None
    }

    /// 子として足す（clip・mask・pattern の参照付き）。
    fn add_with_refs(&self, parent: &ParentNode, child: Node) {
        parent.add_child(
            child,
            self.attributes.clip_path_id.clone(),
            self.attribute("mask"),
            self.get_pattern(),
        );
    }

    /// `parse`: 木を読み切る。
    pub fn parse(mut self) -> R<(Parsed, Vec<String>)> {
        let mut st = self.subtree();
        while let Some(event) = self.next_in(&mut st)? {
            match event {
                Event::Start {
                    name, self_closing, ..
                } => {
                    if self.start_element(name, self_closing)? {
                        continue;
                    }
                    match name {
                        "svg" => self.el_svg()?,
                        "g" | "a" => self.el_g()?,
                        "use" => self.el_use()?,
                        "symbol" | "mask" => self.el_symbol()?,
                        "pattern" => self.el_pattern()?,
                        "radialGradient" => self.el_radial_gradient()?,
                        "linearGradient" => self.el_linear_gradient()?,
                        "clipPath" => self.el_clip_path()?,
                        "image" => self.el_image()?,
                        "text" | "tspan" => self.el_text()?,
                        "filter" => self.el_filter()?,
                        _ => {
                            if !self_closing {
                                self.discard_subtree()?;
                            }
                        }
                    }
                }
                Event::End { name } => self.end_element(name)?,
                Event::Text(text) if self.in_text() => self.append_text(&text)?,
                Event::CData(text) if self.in_text() => self.append_text(text)?,
                _ => {}
            }
        }
        let Some(root) = self.root.clone() else {
            return Err(Error::state("Invalid SVG data"));
        };
        Ok((
            Parsed {
                root,
                defs: self.defs,
            },
            self.warnings,
        ))
    }

    fn in_text(&self) -> bool {
        self.parents
            .last()
            .is_some_and(|(n, _)| n == "text" || n == "tspan")
    }

    /// `_appendText`。
    fn append_text(&mut self, text: &str) -> R<()> {
        let has_non_ws = !dart::trim(text).is_empty();
        let leading_ws = text.chars().next().is_some_and(dart::is_js_space);
        let follows_tspan = self.last_end.as_deref() == Some("tspan");
        let prepend = self.attributes.x.is_none()
            && (self.last_text_ended_with_space || (follows_tspan && leading_ws));
        self.last_text_ended_with_space =
            has_non_ws && text.chars().last().is_some_and(dart::is_js_space);
        let replaced = text.replace('\n', "").replace('\t', " ");
        let trimmed = dart::trim(&replaced);
        let mut collapsed = String::with_capacity(trimmed.len());
        let mut previous_space = false;
        for c in trimmed.chars() {
            if c == ' ' {
                if !previous_space {
                    collapsed.push(' ');
                }
                previous_space = true;
            } else {
                collapsed.push(c);
                previous_space = false;
            }
        }
        if collapsed.is_empty() {
            if leading_ws && follows_tspan {
                self.last_text_ended_with_space = true;
            }
            return Ok(());
        }
        let text = if prepend {
            format!(" {collapsed}")
        } else {
            collapsed
        };
        if let Some((_, group)) = self.parents.last() {
            group.add_child(
                Node::Text(Rc::new(TextNode {
                    text,
                    attributes: self.attributes.clone(),
                })),
                None,
                None,
                None,
            );
        }
        Ok(())
    }

    /// `startElement`（`defs` と図形）。
    fn start_element(&mut self, name: &str, self_closing: bool) -> R<bool> {
        if name == "defs" && !self_closing {
            let group = Node::parent(self.attributes.clone(), None, ParentKind::Plain);
            self.add_group(name.to_owned(), group);
            return Ok(true);
        }
        self.add_shape(name)
    }

    /// `addShape`。
    fn add_shape(&mut self, name: &str) -> R<bool> {
        if !is_path_element(name) {
            return Ok(false);
        }
        let parent = self.current_group()?;
        let Some(path) = self.path_for(name)? else {
            return Ok(false);
        };
        let drawable = Node::Path(Rc::new(PathNode {
            path,
            attributes: self.attributes.clone(),
        }));
        self.check_for_iri(drawable.clone());
        self.add_with_refs(&parent, drawable);
        Ok(true)
    }

    /// `endElement`。
    fn end_element(&mut self, name: &str) -> R<()> {
        let last = self.parents.last().ok_or_else(Error::no_element)?;
        if last.0 == name {
            self.parents.pop();
        }
        self.last_end = Some(name.to_owned());
        if name == "text" {
            self.last_text_ended_with_space = false;
        }
        Ok(())
    }

    // --- 要素 ---

    fn start_name(&self) -> String {
        self.start
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default()
    }

    fn el_svg(&mut self) -> R<()> {
        let (width, height, transform) = self.parse_view_box()?;
        let node = Node::parent(
            self.attributes.clone(),
            Some(transform),
            ParentKind::Viewport { width, height },
        );
        if self.root.is_some() {
            // 入れ子の <svg> は描かない（親の子にならない）
            self.parents.push(("svg".into(), node));
            return Ok(());
        }
        self.root = Some(node.clone());
        let name = self.start_name();
        self.add_group(name, node);
        Ok(())
    }

    fn el_g(&mut self) -> R<()> {
        if self.self_closing() {
            return Ok(());
        }
        let parent = self.current_group()?;
        let group = Node::parent(self.attributes.clone(), None, ParentKind::Plain);
        self.add_with_refs(&parent, Node::Parent(group.clone()));
        let name = self.start_name();
        self.add_group(name, group);
        Ok(())
    }

    fn el_text(&mut self) -> R<()> {
        if self.self_closing() {
            return Ok(());
        }
        let parent = self.current_group()?;
        let name = self.start_name();
        let reset = local_name(&name) == "text";
        let group = Node::parent(
            self.attributes.clone(),
            None,
            ParentKind::TextPosition { reset },
        );
        self.add_with_refs(&parent, Node::Parent(group.clone()));
        self.add_group(name, group);
        Ok(())
    }

    fn el_symbol(&mut self) -> R<()> {
        let group = Node::parent(self.attributes.clone(), None, ParentKind::Plain);
        let name = self.start_name();
        self.add_group(name, group);
        Ok(())
    }

    fn el_filter(&mut self) -> R<()> {
        if self.self_closing() {
            return Ok(());
        }
        let id = self.build_url_iri();
        let mut primitives: HashMap<String, Primitive> = HashMap::new();
        let mut last_result: Option<String> = None;
        let mut count = 0;
        let mut merge_inputs: Option<Vec<String>> = None;
        let mut merge_result: Option<String> = None;
        let mut unsupported = false;
        let mut st = self.subtree();
        while let Some(event) = self.next_in(&mut st)? {
            match event {
                Event::End { name } => {
                    if local_name(name) == "feMerge"
                        && merge_result.is_some()
                        && merge_inputs.is_some()
                    {
                        let result = merge_result.take().unwrap_or_default();
                        let inputs = merge_inputs.take().unwrap_or_default();
                        primitives.insert(result.clone(), Primitive::merge(inputs));
                        last_result = Some(result);
                    }
                }
                Event::Start {
                    name, self_closing, ..
                } => {
                    let local = local_name(name);
                    if matches!(local, "feGaussianBlur" | "feOffset" | "feMerge") {
                        let result = match self.attribute("result") {
                            Some(r) => r,
                            None => {
                                let r = format!("implicit_result_{count}");
                                count += 1;
                                r
                            }
                        };
                        let default_input = last_result
                            .clone()
                            .unwrap_or_else(|| "SourceGraphic".into());
                        let input = self.attribute("in").unwrap_or(default_input);
                        if local == "feGaussianBlur" {
                            let mut px = Some(0.0);
                            let mut py = Some(0.0);
                            if let Some(raw) = self.attribute("stdDeviation") {
                                let parts: Vec<String> = split_list(dart::trim(&raw))
                                    .into_iter()
                                    .filter(|s| !s.is_empty())
                                    .collect();
                                if let Some(first) = parts.first() {
                                    px = parse_double(Some(first), true)?;
                                    py = match parts.get(1) {
                                        Some(second) => parse_double(Some(second), true)?,
                                        None => px,
                                    };
                                }
                            }
                            if let (Some(x), Some(y)) = (px, py)
                                && x >= 0.0
                                && y >= 0.0
                            {
                                primitives.insert(
                                    result.clone(),
                                    Primitive {
                                        kind: PrimitiveKind::Blur,
                                        input,
                                        inputs: Vec::new(),
                                        sigma_x: x,
                                        sigma_y: y,
                                        dx: 0.0,
                                        dy: 0.0,
                                    },
                                );
                                last_result = Some(result);
                            }
                        } else if local == "feOffset" {
                            let dx = match self.attribute("dx") {
                                Some(raw) => parse_double(Some(&raw), true)?.unwrap_or(0.0),
                                None => 0.0,
                            };
                            let dy = match self.attribute("dy") {
                                Some(raw) => parse_double(Some(&raw), true)?.unwrap_or(0.0),
                                None => 0.0,
                            };
                            primitives.insert(
                                result.clone(),
                                Primitive {
                                    kind: PrimitiveKind::Offset,
                                    input,
                                    inputs: Vec::new(),
                                    sigma_x: 0.0,
                                    sigma_y: 0.0,
                                    dx,
                                    dy,
                                },
                            );
                            last_result = Some(result);
                        } else if self_closing {
                            primitives.insert(result.clone(), Primitive::merge(Vec::new()));
                            last_result = Some(result);
                        } else {
                            merge_result = Some(result);
                            merge_inputs = Some(Vec::new());
                        }
                    } else if local == "feMergeNode" {
                        let input = self
                            .attribute("in")
                            .or_else(|| last_result.clone())
                            .unwrap_or_else(|| "SourceGraphic".into());
                        if let Some(inputs) = merge_inputs.as_mut() {
                            inputs.push(input);
                        }
                    } else {
                        if !self_closing {
                            self.discard_subtree()?;
                        }
                        unsupported = true;
                    }
                }
                _ => {}
            }
        }
        if unsupported {
            return Ok(());
        }
        let Some(last_result) = last_result else {
            return Ok(());
        };
        let mut visiting = HashSet::new();
        let Some(layers) = trace(&primitives, &mut visiting, &last_result) else {
            return Ok(());
        };
        if layers.is_empty() {
            return Ok(());
        }
        if layers.len() == 1 && !layers[0].is_source_alpha && !layers[0].has_effect() {
            return Ok(());
        }
        self.defs.filters.insert(id, layers);
        Ok(())
    }

    fn el_pattern(&mut self) -> R<()> {
        if self.self_closing() {
            return Ok(());
        }
        let attributes = self.attributes.clone();
        let raw_width = self.attribute_or("width", "");
        let raw_height = self.attribute_or("height", "");
        let mut width = parsers::parse_pattern_unit(&raw_width, self.viewport_width())?;
        let mut height = parsers::parse_pattern_unit(&raw_height, self.viewport_height())?;
        if width.is_none() || height.is_none() {
            let (w, h, _) = self.parse_view_box()?;
            width = Some(w);
            height = Some(h);
        }
        let raw_x = attributes.raw.get("x").map(str::to_owned);
        let raw_y = attributes.raw.get("y").map(str::to_owned);
        let id = self.build_url_iri();
        self.pattern_ids.insert(id);
        let new = SvgAttributes {
            raw: attributes.raw.clone(),
            id: attributes.id.clone(),
            href: attributes.href.clone(),
            transform: attributes.transform,
            color: attributes.color,
            stroke: attributes.stroke.clone(),
            fill: attributes.fill.clone(),
            fill_rule: attributes.fill_rule,
            clip_rule: attributes.clip_rule,
            clip_path_id: attributes.clip_path_id.clone(),
            blend_mode: attributes.blend_mode,
            font_family: attributes.font_family.clone(),
            font_weight: attributes.font_weight,
            font_size: attributes.font_size,
            x: double_or_percentage(raw_x.as_deref())?,
            y: double_or_percentage(raw_y.as_deref())?,
            width,
            height,
            ..SvgAttributes::default()
        };
        let group = Node::parent(Rc::new(new), None, ParentKind::Plain);
        let name = self.start_name();
        self.add_group(name, group);
        Ok(())
    }

    fn el_use(&mut self) -> R<()> {
        let parent = self.current_group()?;
        let Some(href) = self.attributes.href.clone().filter(|h| !h.is_empty()) else {
            return Ok(());
        };
        let base = match parse_transform(self.attribute("transform").as_deref())? {
            Some(t) => t,
            None => AffineMatrix::IDENTITY,
        };
        let x = self.units_req(&self.attribute_or("x", "0"), self.viewport_width())?;
        let y = self.units_req(&self.attribute_or("y", "0"), self.viewport_height())?;
        let transform = base.translated(x, y);
        let group = Node::parent(SvgAttributes::empty(), Some(transform), ParentKind::Plain);
        group.add_child(
            Node::Deferred(Rc::new(DeferredNode {
                ref_id: format!("url({href})"),
                attributes: self.attributes.clone(),
            })),
            None,
            None,
            None,
        );
        let own = format!("#{}", self.attributes.id.as_deref().unwrap_or("null"));
        if own != href {
            self.check_for_iri(Node::Parent(group.clone()));
        }
        self.add_with_refs(&parent, Node::Parent(group));
        Ok(())
    }

    /// `parseStops`（グラデーションの中の開始タグはどれも stop として読む）。
    fn parse_stops(&mut self) -> R<(Vec<Color>, Vec<f64>)> {
        let mut colors = Vec::new();
        let mut offsets = Vec::new();
        let mut st = self.subtree();
        while let Some(event) = self.next_in(&mut st)? {
            if let Event::Start { .. } = event {
                let raw_opacity = self.attribute_or("stop-opacity", "1");
                let stop_color = self
                    .parse_color(self.attribute("stop-color").as_deref())?
                    .unwrap_or(Color::OPAQUE_BLACK);
                let opacity =
                    parse_double(Some(&raw_opacity), false)?.ok_or_else(Error::null_check)?;
                colors.push(stop_color.with_opacity(opacity)?);
                let raw_offset = self.attribute_or("offset", "0%");
                offsets.push(parse_decimal_or_percentage(&raw_offset)?);
            }
        }
        Ok((colors, offsets))
    }

    fn gradient_unit_mode(&self) -> Option<UnitMode> {
        match self.attribute("gradientUnits").as_deref() {
            Some("userSpaceOnUse") => Some(UnitMode::UserSpaceOnUse),
            Some("objectBoundingBox") => Some(UnitMode::ObjectBoundingBox),
            _ => None,
        }
    }

    fn tile_mode(&self) -> Option<TileMode> {
        match self.attribute("spreadMethod").as_deref() {
            Some("pad") => Some(TileMode::Clamp),
            Some("repeat") => Some(TileMode::Repeated),
            Some("reflect") => Some(TileMode::Mirror),
            _ => None,
        }
    }

    fn el_radial_gradient(&mut self) -> R<()> {
        let unit_mode = self.gradient_unit_mode();
        let raw_cx = self.attribute_or("cx", "50%");
        let raw_cy = self.attribute_or("cy", "50%");
        let raw_r = self.attribute_or("r", "50%");
        let raw_fx = self.attribute("fx").unwrap_or_else(|| raw_cx.clone());
        let raw_fy = self.attribute("fy").unwrap_or_else(|| raw_cy.clone());
        let tile_mode = self.tile_mode();
        let id = self.build_url_iri();
        let transform = parse_transform(self.attribute("gradientTransform").as_deref())?;
        let href = self.attributes.href.clone();
        let (colors, offsets) = if self.self_closing() {
            (None, None)
        } else {
            let (c, o) = self.parse_stops()?;
            (Some(c), Some(o))
        };
        let cx = parse_decimal_or_percentage(&raw_cx)?;
        let cy = parse_decimal_or_percentage(&raw_cy)?;
        let r = parse_decimal_or_percentage(&raw_r)?;
        let fx = parse_decimal_or_percentage(&raw_fx)?;
        let fy = parse_decimal_or_percentage(&raw_fy)?;
        self.defs.add_gradient(
            Gradient {
                id,
                kind: GradientKind::Radial {
                    center: Point::new(cx, cy),
                    radius: r,
                    focal: (fx != cx || fy != cy).then_some(Point::new(fx, fy)),
                },
                colors,
                offsets,
                tile_mode,
                unit_mode,
                transform,
            },
            href.as_deref(),
        );
        Ok(())
    }

    fn el_linear_gradient(&mut self) -> R<()> {
        let unit_mode = self.gradient_unit_mode();
        let x1 = self.attribute_or("x1", "0%");
        let x2 = self.attribute_or("x2", "100%");
        let y1 = self.attribute_or("y1", "0%");
        let y2 = self.attribute_or("y2", "0%");
        let id = self.build_url_iri();
        let transform = parse_transform(self.attribute("gradientTransform").as_deref())?;
        let tile_mode = self.tile_mode();
        let href = self.attributes.href.clone();
        let (colors, offsets) = if self.self_closing() {
            (None, None)
        } else {
            let (c, o) = self.parse_stops()?;
            (Some(c), Some(o))
        };
        let from = Point::new(
            parse_decimal_or_percentage(&x1)?,
            parse_decimal_or_percentage(&y1)?,
        );
        let to = Point::new(
            parse_decimal_or_percentage(&x2)?,
            parse_decimal_or_percentage(&y2)?,
        );
        self.defs.add_gradient(
            Gradient {
                id,
                kind: GradientKind::Linear { from, to },
                colors,
                offsets,
                tile_mode,
                unit_mode,
                transform,
            },
            href.as_deref(),
        );
        Ok(())
    }

    fn el_clip_path(&mut self) -> R<()> {
        let id = self.build_url_iri();
        let mut nodes = Vec::new();
        let mut st = self.subtree();
        while let Some(event) = self.next_in(&mut st)? {
            if let Event::Start { name, .. } = event {
                if is_path_element(name) {
                    let path = self.path_for(name)?.ok_or_else(Error::null_check)?;
                    self.current_group()?;
                    let path = match parse_transform(self.attribute("transform").as_deref())? {
                        Some(t) => path.transformed(&t),
                        None => path,
                    };
                    nodes.push(Node::Path(Rc::new(PathNode {
                        path: Path::new(
                            path.commands,
                            self.attributes.clip_rule.unwrap_or(FillType::NonZero),
                        ),
                        attributes: self.attributes.clone(),
                    })));
                } else if name == "use" {
                    let href = self.attributes.href.as_deref().unwrap_or("null");
                    nodes.push(Node::Deferred(Rc::new(DeferredNode {
                        ref_id: format!("url({href})"),
                        attributes: self.attributes.clone(),
                    })));
                }
            }
        }
        self.defs.add_clip_path(id, nodes);
        Ok(())
    }

    fn el_image(&mut self) -> R<()> {
        let Some(href) = self.attributes.href.clone() else {
            return Ok(());
        };
        if !href.starts_with("data:") {
            return Ok(());
        }
        // 位置は UTF-16 の単位（data: URI はふつう ASCII）
        let units: Vec<u16> = href.encode_utf16().collect();
        let len = units.len() as i64;
        let find = |c: u16, from: usize| -> i64 {
            units
                .iter()
                .skip(from)
                .position(|&u| u == c)
                .map_or(-1, |i| (i + from) as i64)
        };
        let semicolon = find(u16::from(b';'), 0) + 1;
        let comma = find(u16::from(b','), semicolon.max(0) as usize) + 1;
        let slash = find(u16::from(b'/'), 0) + 1;
        let end = semicolon - 1;
        if slash > len {
            return Err(Error::range(slash, 0, len, "start"));
        }
        if slash > end || end > len {
            return Err(Error::range(end, slash, len, "end"));
        }
        let mime: String = String::from_utf16_lossy(&units[slash as usize..end as usize])
            .chars()
            .filter(|c| !dart::is_js_space(*c))
            .collect::<String>()
            .to_lowercase();
        let format = match mime.as_str() {
            "png" => 0,
            "jpeg" | "jpg" => 1,
            "webp" => 2,
            "gif" => 3,
            "bmp" => 4,
            _ => {
                self.warnings
                    .push(format!("Warning: Unsupported image format {mime}"));
                return Ok(());
            }
        };
        let data: String = String::from_utf16_lossy(&units[comma as usize..])
            .chars()
            .filter(|c| !dart::is_js_space(*c))
            .collect();
        let bytes = parsers::base64_decode(&data)?;
        let image = Node::Image(Rc::new(ImageNode {
            data: Rc::new(bytes),
            format,
            attributes: self.attributes.clone(),
        }));
        let group = self.current_group()?;
        group.add_child(image.clone(), None, None, None);
        self.check_for_iri(image);
        Ok(())
    }

    // --- 図形 ---

    fn path_for(&self, name: &str) -> R<Option<Path>> {
        let fill_rule = self.attributes.fill_rule;
        let vw = self.viewport_width();
        let vh = self.viewport_height();
        Ok(Some(match name {
            "circle" => {
                let cx = self.units_req(&self.attribute_or("cx", "0"), vw)?;
                let cy = self.units_req(&self.attribute_or("cy", "0"), vh)?;
                let diag = match (vw, vh) {
                    (Some(w), Some(h)) => Some((w * w + h * h).sqrt() / 2f64.sqrt()),
                    _ => None,
                };
                let r = self.units_req(&self.attribute_or("r", "0"), diag)?;
                PathBuilder::new(fill_rule)
                    .add_oval(Rect::from_circle(cx, cy, r))
                    .take_path()
            }
            "path" => parse_svg_path_data(&self.attribute_or("d", ""), fill_rule)?,
            "rect" => {
                let x = self.units_req(&self.attribute_or("x", "0"), vw)?;
                let y = self.units_req(&self.attribute_or("y", "0"), vh)?;
                let w = self.units_req(&self.attribute_or("width", "0"), vw)?;
                let h = self.units_req(&self.attribute_or("height", "0"), vh)?;
                let mut rx_raw = self.attribute("rx");
                let mut ry_raw = self.attribute("ry");
                if rx_raw.is_none() {
                    rx_raw = ry_raw.clone();
                }
                if ry_raw.is_none() {
                    ry_raw = rx_raw.clone();
                }
                match rx_raw {
                    Some(rx_raw) if !rx_raw.is_empty() => {
                        let rx = self.units_req(&rx_raw, vw)?;
                        let ry = self.units_req(ry_raw.as_deref().unwrap_or(""), vh)?;
                        PathBuilder::new(fill_rule)
                            .add_rrect(Rect::from_ltwh(x, y, w, h), rx, ry)
                            .take_path()
                    }
                    _ => PathBuilder::new(fill_rule)
                        .add_rect(Rect::from_ltwh(x, y, w, h))
                        .take_path(),
                }
            }
            "polygon" | "polyline" => {
                let points = self.attribute_or("points", "");
                if points.is_empty() {
                    return Ok(None);
                }
                let close = if name == "polygon" { "z" } else { "" };
                parse_svg_path_data(&format!("M{points}{close}"), fill_rule)?
            }
            "ellipse" => {
                let cx = self.units_req(&self.attribute_or("cx", "0"), vw)?;
                let cy = self.units_req(&self.attribute_or("cy", "0"), vh)?;
                let rx = self.units_req(&self.attribute_or("rx", "0"), vw)?;
                let ry = self.units_req(&self.attribute_or("ry", "0"), vh)?;
                let r = Rect::from_ltwh(cx - rx, cy - ry, rx * 2.0, ry * 2.0);
                PathBuilder::new(fill_rule).add_oval(r).take_path()
            }
            "line" => {
                let x1 = self.units_req(&self.attribute_or("x1", "0"), vw)?;
                let x2 = self.units_req(&self.attribute_or("x2", "0"), vw)?;
                let y1 = self.units_req(&self.attribute_or("y1", "0"), vh)?;
                let y2 = self.units_req(&self.attribute_or("y2", "0"), vh)?;
                let mut b = PathBuilder::new(fill_rule);
                b.move_to(x1, y1).line_to(x2, y2);
                b.take_path()
            }
            _ => return Ok(None),
        }))
    }

    // --- 属性 ---

    /// `_parseViewBox`（幅・高さ・平行移動）。
    fn parse_view_box(&self) -> R<(f64, f64, AffineMatrix)> {
        let view_box = self.attribute_or("viewBox", "");
        let raw_width = self.attribute_or("width", "");
        let raw_height = self.attribute_or("height", "");
        if view_box.is_empty() && raw_width.is_empty() && raw_height.is_empty() {
            return Err(Error::state(format!(
                "SVG did not specify dimensions\n\n\
                 The SVG library looks for a `viewBox` or `width` and `height` attribute \
                 to determine the viewport boundary of the SVG.  Note that these attributes, \
                 as with all SVG attributes, are case sensitive.\n\
                 During processing, the following attributes were found:\n  {}",
                self.attributes.raw.to_dart_string()
            )));
        }
        if view_box.is_empty() {
            let width = raw_width_height(&raw_width)?;
            let height = raw_width_height(&raw_height)?;
            return Ok((width, height, AffineMatrix::IDENTITY));
        }
        let parts = split_space_comma(&view_box);
        if parts.len() < 4 {
            return Err(Error::state("viewBox element must be 4 elements long"));
        }
        let req =
            |s: &str| -> R<f64> { parse_double(Some(s), false)?.ok_or_else(Error::null_check) };
        let width = req(&parts[2])?;
        let height = req(&parts[3])?;
        let tx = -req(&parts[0])?;
        let ty = -req(&parts[1])?;
        Ok((width, height, AffineMatrix::IDENTITY.translated(tx, ty)))
    }

    fn parse_color(&self, s: Option<&str>) -> R<Option<Color>> {
        super::colors::parse_color(s)
    }

    /// `_createSvgAttributes`。
    fn create_svg_attributes(
        &mut self,
        map: AttrMap,
        current_color: Option<Color>,
    ) -> R<SvgAttributes> {
        let id = map.get("id").map(str::to_owned);
        let opacity = match map.get("opacity") {
            Some(raw) => Some(dart::clamp(
                parse_double(Some(raw), false)?.ok_or_else(Error::null_check)?,
                0.0,
                1.0,
            )),
            None => None,
        };
        let color = self.parse_color(map.get("color"))?.or(current_color);
        let raw_x = single_coordinate("x", map.get("x"))?;
        let raw_y = single_coordinate("y", map.get("y"))?;
        let raw_dx = single_coordinate("dx", map.get("dx"))?;
        let raw_dy = single_coordinate("dy", map.get("dy"))?;
        let x = double_or_percentage(raw_x.as_deref())?;
        let y = double_or_percentage(raw_y.as_deref())?;
        let dx = double_or_percentage(raw_dx.as_deref())?;
        let dy = double_or_percentage(raw_dy.as_deref())?;
        let href = map.get("href").map(str::to_owned);
        let color_or_none = if map.get("color").map(str::to_lowercase).as_deref() == Some("none") {
            ColorOrNone::NONE
        } else {
            ColorOrNone::color(color)
        };
        let stroke = self.parse_stroke_attributes(&map, opacity)?;
        let fill = self.parse_fill_attributes(&map, opacity)?;
        let fill_rule = parsers::parse_fill_rule(map.get("fill-rule"));
        let clip_rule = parsers::parse_fill_rule(map.get("clip-rule"));
        let clip_path_id = map.get("clip-path").map(str::to_owned);
        let blend_mode = map.get("mix-blend-mode").and_then(blend_mode);
        let transform = parse_transform(map.get("transform"))?.unwrap_or(AffineMatrix::IDENTITY);
        let font_family = map.get("font-family").map(str::to_owned);
        let font_weight = self.parse_font_weight(map.get("font-weight"));
        let font_size = parse_font_size(map.get("font-size"))?;
        let text_decoration = parse_text_decoration(map.get("text-decoration"))?;
        let text_decoration_style = parse_text_decoration_style(map.get("text-decoration-style"))?;
        let text_decoration_color = self.parse_color(map.get("text-decoration-color"))?;
        let text_anchor_multiplier = match map.get("text-anchor") {
            Some("end") => Some(1.0),
            Some("middle") => Some(0.5),
            Some("start") => Some(0.0),
            _ => None,
        };
        let mask_type = match map.get("mask-type").map(|m| dart::trim(m).to_lowercase()) {
            Some(m) if m == "alpha" => Some(MaskType::Alpha),
            Some(m) if m == "luminance" => Some(MaskType::Luminance),
            _ => None,
        };
        let filter_id = map.get("filter").map(str::to_owned);
        Ok(SvgAttributes {
            raw: Rc::new(map),
            id,
            href,
            transform,
            color: color_or_none,
            stroke,
            fill,
            fill_rule,
            clip_rule,
            clip_path_id,
            blend_mode,
            font_family,
            font_weight,
            font_size,
            text_decoration,
            text_decoration_style,
            text_decoration_color,
            x,
            dx,
            text_anchor_multiplier,
            y,
            dy,
            width: None,
            height: None,
            mask_type,
            opacity,
            filter_id,
        })
    }

    fn parse_stroke_attributes(
        &self,
        map: &AttrMap,
        uniform_opacity: Option<f64>,
    ) -> R<Option<StrokeAttributes>> {
        let raw_stroke = map.get("stroke");
        let mut opacity = match map.get("stroke-opacity") {
            Some(raw) => Some(dart::clamp(
                parse_double(Some(raw), false)?.ok_or_else(Error::null_check)?,
                0.0,
                1.0,
            )),
            None => None,
        };
        if let Some(u) = uniform_opacity {
            opacity = Some(match opacity {
                None => u,
                Some(o) => o * u,
            });
        }
        let raw_cap = map.get("stroke-linecap");
        let raw_join = map.get("stroke-linejoin");
        let raw_miter = map.get("stroke-miterlimit");
        let raw_width = map.get("stroke-width");
        let raw_dash = map.get("stroke-dasharray");
        let raw_dash_offset = map.get("stroke-dashoffset");
        let any = raw_stroke
            .or(raw_cap)
            .or(raw_join)
            .or(raw_miter)
            .or(raw_width)
            .or(raw_dash)
            .or(raw_dash_offset);
        if any.is_none() {
            return Ok(None);
        }
        let mut shader_id = None;
        let mut has_pattern = None;
        let color = if raw_stroke.is_some_and(|s| s.starts_with("url")) {
            shader_id = raw_stroke.map(str::to_owned);
            if raw_stroke.is_some_and(|s| self.pattern_ids.contains(s)) {
                has_pattern = Some(true);
            }
            Some(Color(0xFFFF_FFFF))
        } else {
            self.parse_color(raw_stroke)?
        };
        let cap = match raw_cap {
            Some("butt") => Some(0),
            Some("round") => Some(1),
            Some("square") => Some(2),
            _ => None,
        };
        let join = match raw_join {
            Some("miter") => Some(0),
            Some("bevel") => Some(2),
            Some("round") => Some(1),
            _ => None,
        };
        Ok(Some(StrokeAttributes {
            color: if raw_stroke == Some("none") {
                ColorOrNone::NONE
            } else {
                ColorOrNone::color(color)
            },
            shader_id,
            join,
            cap,
            miter_limit: parse_double(raw_miter, false)?,
            width: parse_double_with_units(raw_width, true, None)?,
            dash_array: parse_dash_array(raw_dash)?,
            dash_offset: parse_double_with_units(raw_dash_offset, false, None)?,
            has_pattern,
            opacity,
        }))
    }

    fn parse_fill_attributes(
        &self,
        map: &AttrMap,
        uniform_opacity: Option<f64>,
    ) -> R<Option<FillAttributes>> {
        let raw_fill = map.get("fill").unwrap_or("");
        let mut opacity = match map.get("fill-opacity") {
            Some(raw) => Some(dart::clamp(
                parse_double(Some(raw), false)?.ok_or_else(Error::null_check)?,
                0.0,
                1.0,
            )),
            None => None,
        };
        if let Some(u) = uniform_opacity {
            opacity = Some(match opacity {
                None => u,
                Some(o) => o * u,
            });
        }
        if raw_fill.starts_with("url") {
            let has_pattern = self.pattern_ids.contains(raw_fill).then_some(true);
            return Ok(Some(FillAttributes {
                color: ColorOrNone::color(Some(Color(0xFFFF_FFFF))),
                shader_id: Some(raw_fill.to_owned()),
                has_pattern,
                opacity,
            }));
        }
        let mut fill_color = self.parse_color(Some(raw_fill))?;
        if let Some(c) = fill_color
            && c.a() != 255
        {
            opacity = Some(c.a() as f64 / 255.0);
            fill_color = Some(c.with_opacity(1.0)?);
        }
        Ok(Some(FillAttributes {
            color: if raw_fill == "none" {
                ColorOrNone::NONE
            } else {
                ColorOrNone::color(fill_color)
            },
            opacity,
            shader_id: None,
            has_pattern: None,
        }))
    }

    /// `parseFontWeight`（知らない値は警告して捨てる）。
    fn parse_font_weight(&mut self, raw: Option<&str>) -> Option<u8> {
        let raw = raw?;
        let w = match raw {
            "normal" => NORMAL_FONT_WEIGHT,
            "bold" => BOLD_FONT_WEIGHT,
            "100" => 0,
            "200" => 1,
            "300" => 2,
            "400" => 3,
            "500" => 4,
            "600" => 5,
            "700" => 6,
            "800" => 7,
            "900" => 8,
            _ => {
                self.warnings.push(format!(
                    "Warning: Invalid \"font-weight\": {raw}. Ignoring invalid value."
                ));
                return None;
            }
        };
        Some(w)
    }
}

/// `split(RegExp('[ ,]+'))` の結果（連なった区切りを 1 つと見なす）。
fn split_space_comma(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_sep = false;
    for c in s.chars() {
        if c == ' ' || c == ',' {
            if !in_sep {
                out.push(std::mem::take(&mut current));
                in_sep = true;
            }
        } else {
            in_sep = false;
            current.push(c);
        }
    }
    out.push(current);
    out
}

/// `_parseRawWidthHeight`。
fn raw_width_height(raw: &str) -> R<f64> {
    if raw == "100%" || raw.is_empty() {
        return Ok(f64::INFINITY);
    }
    Ok(parse_double_with_units(Some(raw), true, None)?.unwrap_or(f64::INFINITY))
}

fn is_visible(map: &AttrMap) -> bool {
    map.get("display") != Some("none") && map.get("visibility") != Some("hidden")
}

/// `_createAttributeMap`（`style` を属性に開く。値は前後の空白を落とし、`inherit` は捨てる）。
fn create_attribute_map(attributes: &[(&str, String)]) -> R<AttrMap> {
    let mut map = AttrMap::default();
    for (name, value) in attributes {
        let value = dart::trim(value);
        let local = local_name(name);
        if local == "style" {
            for style in value.split(';') {
                if style.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = style.split(':').collect();
                if parts.len() < 2 {
                    return Err(Error::index(1, parts.len() as i64));
                }
                let v = dart::trim(parts[1]);
                if v == "inherit" {
                    continue;
                }
                map.insert(dart::trim(parts[0]), v.to_owned());
            }
        } else if value != "inherit" {
            map.insert(local, value.to_owned());
        }
    }
    Ok(map)
}

/// `_singleCoordinate`（文字ごとの座標の並びは扱わない）。
fn single_coordinate(name: &str, raw: Option<&str>) -> R<Option<String>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = dart::trim(raw);
    if trimmed.chars().any(|c| c == ',' || dart::is_js_space(c)) {
        return Err(Error::unimplemented(format!(
            "A list of values for \"{name}\" is not supported: \"{trimmed}\""
        )));
    }
    Ok(Some(trimmed.to_owned()))
}

/// `DoubleOrPercentage.fromString`。
fn double_or_percentage(raw: Option<&str>) -> R<Option<DoubleOrPercentage>> {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else {
        return Ok(None);
    };
    if is_percentage(raw) {
        return Ok(Some(DoubleOrPercentage::Percentage(parse_percentage(raw)?)));
    }
    if is_em(raw) {
        return Ok(Some(DoubleOrPercentage::Em(parse_em(raw)?)));
    }
    Ok(Some(DoubleOrPercentage::Pixels(
        parse_double(Some(raw), false)?.ok_or_else(Error::null_check)?,
    )))
}

fn blend_mode(raw: &str) -> Option<u8> {
    Some(match raw {
        "multiply" => 24,
        "screen" => 14,
        "overlay" => 15,
        "darken" => 16,
        "lighten" => 17,
        "color-dodge" => 18,
        "color-burn" => 19,
        "hard-light" => 20,
        "soft-light" => 21,
        "difference" => 22,
        "exclusion" => 23,
        "hue" => 25,
        "saturation" => 26,
        "color" => 27,
        "luminosity" => 28,
        _ => return None,
    })
}

/// `parseFontSize`。
fn parse_font_size(raw: Option<&str>) -> R<Option<f64>> {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else {
        return Ok(None);
    };
    if let Some(v) = parse_double_with_units(Some(raw), true, None)? {
        return Ok(Some(v));
    }
    let lower = dart::trim(&raw.to_lowercase()).to_owned();
    let v = match lower.as_str() {
        "xx-small" => 10.0,
        "x-small" => 12.0,
        "small" => 14.0,
        "medium" => 18.0,
        "large" => 22.0,
        "x-large" => 26.0,
        "xx-large" => 32.0,
        _ => return Err(Error::state(format!("Could not parse font-size: {lower}"))),
    };
    Ok(Some(v))
}

fn parse_text_decoration(raw: Option<&str>) -> R<Option<u8>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    Ok(Some(match raw {
        "none" => 0,
        "underline" => 1,
        "overline" => 2,
        "line-through" => 4,
        _ => {
            return Err(Error::unsupported(format!(
                "Attribute value for text-decoration=\"{raw}\" is not supported"
            )));
        }
    }))
}

fn parse_text_decoration_style(raw: Option<&str>) -> R<Option<u8>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    Ok(Some(match raw {
        "solid" => 0,
        "double" => 1,
        "dotted" => 2,
        "dashed" => 3,
        "wavy" => 4,
        _ => {
            return Err(Error::unsupported(format!(
                "Attribute value for text-decoration-style=\"{raw}\" is not supported"
            )));
        }
    }))
}

/// `_parseDashArray`。
fn parse_dash_array(raw: Option<&str>) -> R<Option<Vec<f64>>> {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else {
        return Ok(None);
    };
    if raw == "none" {
        return Ok(Some(Vec::new()));
    }
    let mut doubles = Vec::new();
    let mut non_zero = false;
    for part in split_space_comma(raw) {
        let v = parse_double_with_units(Some(&part), false, None)?.ok_or_else(Error::null_check)?;
        if v != 0.0 {
            non_zero = true;
        }
        doubles.push(v);
    }
    if doubles.is_empty() || !non_zero {
        return Ok(None);
    }
    Ok(Some(doubles))
}

#[derive(Clone, Copy, PartialEq)]
enum PrimitiveKind {
    Blur,
    Offset,
    Merge,
}

struct Primitive {
    kind: PrimitiveKind,
    input: String,
    inputs: Vec<String>,
    sigma_x: f64,
    sigma_y: f64,
    dx: f64,
    dy: f64,
}

impl Primitive {
    fn merge(inputs: Vec<String>) -> Self {
        Self {
            kind: PrimitiveKind::Merge,
            input: String::new(),
            inputs,
            sigma_x: 0.0,
            sigma_y: 0.0,
            dx: 0.0,
            dy: 0.0,
        }
    }
}

/// フィルターの入力をたどって、重ねる層にする。
fn trace(
    primitives: &HashMap<String, Primitive>,
    visiting: &mut HashSet<String>,
    input: &str,
) -> Option<Vec<FilterLayer>> {
    let layer = |is_source_alpha| FilterLayer {
        is_source_alpha,
        sigma_x: 0.0,
        sigma_y: 0.0,
        dx: 0.0,
        dy: 0.0,
    };
    if input == "SourceGraphic" {
        return Some(vec![layer(false)]);
    }
    if input == "SourceAlpha" {
        return Some(vec![layer(true)]);
    }
    if visiting.contains(input) {
        return None;
    }
    let primitive = primitives.get(input)?;
    visiting.insert(input.to_owned());
    if primitive.kind == PrimitiveKind::Merge {
        let mut merged = Vec::new();
        for i in &primitive.inputs {
            let Some(parent) = trace(primitives, visiting, i) else {
                visiting.remove(input);
                return None;
            };
            merged.extend(parent);
        }
        visiting.remove(input);
        return Some(merged);
    }
    let parent = trace(primitives, visiting, &primitive.input);
    visiting.remove(input);
    let parent = parent?;
    Some(
        parent
            .into_iter()
            .map(|p| {
                if primitive.kind == PrimitiveKind::Blur {
                    FilterLayer {
                        is_source_alpha: p.is_source_alpha,
                        sigma_x: (p.sigma_x * p.sigma_x + primitive.sigma_x * primitive.sigma_x)
                            .sqrt(),
                        sigma_y: (p.sigma_y * p.sigma_y + primitive.sigma_y * primitive.sigma_y)
                            .sqrt(),
                        dx: p.dx,
                        dy: p.dy,
                    }
                } else {
                    FilterLayer {
                        is_source_alpha: p.is_source_alpha,
                        sigma_x: p.sigma_x,
                        sigma_y: p.sigma_y,
                        dx: p.dx + primitive.dx,
                        dy: p.dy + primitive.dy,
                    }
                }
            })
            .collect(),
    )
}
