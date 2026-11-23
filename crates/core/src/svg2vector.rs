//! SVG を VectorDrawable に変える。Android Studio の取り込み（AOSP の sdk-common にある `Svg2Vector`、
//! 版 32.1.0）を Rust に移したもの。元は The Android Open Source Project の Apache License 2.0
//! （http://www.apache.org/licenses/LICENSE-2.0）のコードで、Java を動かさずに同じ XML と同じエラー・警告を出す。
//!
//! 後の版を追えるよう、つくりは Java に近くしてある: `Svg2Vector`（このファイルの前半）、`SvgTree`（`Tree`）、
//! `SvgNode` とその子（`Node` と `Kind`。継承は `match` で分ける）、`SvgColor`・`VdUtil`（このファイルの後半）、
//! `PathParser`・`PathBuilder`・`VdPath.Node`・`EllipseSolver`・`VdNodeRender`（`path`）。出力に出る Java の
//! ふるまい（数の書き方、`HashMap` の並び、`AffineTransform` の計算の道）は `java` にまとめた。読むのは
//! `PositionXmlParser` に合わせた木（`dom`）。
//!
//! Java で例外になるところ（`NumberFormatException` など）は、`Throwable.toString()` と同じ文のエラーにする。
//! Java の `HashSet` を識別子のハッシュで回すところ（並びが毎回変わる）は、入れた順で回す。

mod dom;
mod java;
mod path;

use std::collections::{HashMap as StdHashMap, HashSet, VecDeque};

use dom::{Child, Dom};
use java::{AffineTransform, HashMap, J, Path2D, Thrown};
use path::{ParseMode, PathBuilder};

/// 移した Svg2Vector の版。
pub const VERSION: &str = "32.1.0";

/// 変換の結果（`Svg2Vector.parseSvgToXml`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converted {
    /// VectorDrawable の XML。描くものがなければ空。
    pub xml: String,
    /// エラー・警告（"ERROR @ line N: …" を改行でつないだもの）。なければ空。
    pub messages: String,
}

/// `svg` を VectorDrawable に変える。Java の取り込みが例外で止まるときは、その例外の 1 行目をエラーにする。
pub fn convert(svg: &[u8]) -> Result<Converted, String> {
    let dom = match Dom::parse(svg) {
        Ok(dom) => dom,
        // Java の木に要素がひとつもなければ、<svg> がないという例外になる
        Err(e) if e.no_elements => {
            return Err(Thrown::new(java::ILLEGAL_STATE, "No <svg> tags found.").to_string());
        }
        Err(e) => return Err(format!("XML parse error: {}", e.message)),
    };
    let mut tree = Tree::new(&dom);
    let result = (|| -> J<String> {
        parse(&mut tree)?;
        if tree.has_leaf_node {
            tree.write_xml()
        } else {
            Ok(String::new())
        }
    })();
    match result {
        Ok(xml) => Ok(Converted {
            xml,
            messages: tree.error_message(),
        }),
        Err(e) => Err(e.to_string()),
    }
}

// ---- Svg2Vector ----

const SVG_DEFS: &str = "defs";
const SVG_USE: &str = "use";
const SVG_HREF: &str = "href";
const SVG_XLINK_HREF: &str = "xlink:href";
const SVG_POLYGON: &str = "polygon";
const SVG_POLYLINE: &str = "polyline";
const SVG_RECT: &str = "rect";
const SVG_CIRCLE: &str = "circle";
const SVG_LINE: &str = "line";
const SVG_PATH: &str = "path";
const SVG_ELLIPSE: &str = "ellipse";
const SVG_GROUP: &str = "g";
const SVG_STYLE: &str = "style";
const SVG_DISPLAY: &str = "display";
const SVG_CLIP_PATH_ELEMENT: &str = "clipPath";
const SVG_D: &str = "d";
const SVG_CLIP_PATH: &str = "clip-path";
const SVG_CLIP_RULE: &str = "clip-rule";
const SVG_FILL: &str = "fill";
const SVG_FILL_OPACITY: &str = "fill-opacity";
const SVG_FILL_RULE: &str = "fill-rule";
const SVG_OPACITY: &str = "opacity";
const SVG_PAINT_ORDER: &str = "paint-order";
const SVG_STROKE: &str = "stroke";
const SVG_STROKE_OPACITY: &str = "stroke-opacity";
const SVG_STROKE_WIDTH: &str = "stroke-width";
const SVG_MASK: &str = "mask";
const SVG_POINTS: &str = "points";

/// `presentationMap`（SVG の属性 → VectorDrawable の属性。空は別に扱うもの）。
const PRESENTATION_MAP: [(&str, &str); 12] = [
    ("clip", "android:clip"),
    (SVG_CLIP_RULE, ""),
    (SVG_FILL, "android:fillColor"),
    (SVG_FILL_OPACITY, "android:fillAlpha"),
    (SVG_FILL_RULE, "android:fillType"),
    (SVG_OPACITY, ""),
    (SVG_PAINT_ORDER, ""),
    (SVG_STROKE, "android:strokeColor"),
    ("stroke-linecap", "android:strokeLineCap"),
    ("stroke-linejoin", "android:strokeLineJoin"),
    (SVG_STROKE_OPACITY, "android:strokeAlpha"),
    (SVG_STROKE_WIDTH, "android:strokeWidth"),
];

fn presentation(name: &str) -> Option<&'static str> {
    PRESENTATION_MAP
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
}

/// `gradientMap`（並びは Java の `ImmutableMap` と同じ）。
const GRADIENT_MAP: [(&str, &str); 11] = [
    ("x1", "android:startX"),
    ("y1", "android:startY"),
    ("x2", "android:endX"),
    ("y2", "android:endY"),
    ("cx", "android:centerX"),
    ("cy", "android:centerY"),
    ("r", "android:gradientRadius"),
    ("spreadMethod", "android:tileMode"),
    ("gradientUnits", ""),
    ("gradientTransform", ""),
    ("gradientType", "android:type"),
];

const UNSUPPORTED_SVG_NODES: [&str; 62] = [
    // アニメーション
    "animate",
    "animateColor",
    "animateMotion",
    "animateTransform",
    "mpath",
    "set",
    // 入れ物
    "a",
    "marker",
    "missing-glyph",
    "pattern",
    "switch",
    // フィルター
    "feBlend",
    "feColorMatrix",
    "feComponentTransfer",
    "feComposite",
    "feConvolveMatrix",
    "feDiffuseLighting",
    "feDisplacementMap",
    "feFlood",
    "feFuncA",
    "feFuncB",
    "feFuncG",
    "feFuncR",
    "feGaussianBlur",
    "feImage",
    "feMerge",
    "feMergeNode",
    "feMorphology",
    "feOffset",
    "feSpecularLighting",
    "feTile",
    "feTurbulence",
    // 字形
    "font",
    "font-face",
    "font-face-format",
    "font-face-name",
    "font-face-src",
    "font-face-uri",
    "hkern",
    "vkern",
    // グラデーション
    "stop",
    // 図形
    "ellipse",
    "image",
    // 光源
    "feDistantLight",
    "fePointLight",
    "feSpotLight",
    // 構造
    "symbol",
    // 文
    "altGlyphDef",
    "altGlyphItem",
    "glyph",
    "glyphRef",
    "text",
    "altGlyph",
    "textPath",
    "tref",
    "tspan",
    // そのほか
    "color-profile",
    "cursor",
    "filter",
    "foreignObject",
    "script",
    "view",
];

fn is_unsupported(tag: &str) -> bool {
    UNSUPPORTED_SVG_NODES.contains(&tag)
}

fn is_space_or_comma(c: char) -> bool {
    c == ',' || java::is_regex_space(c)
}

fn parse(svg: &mut Tree) -> J<()> {
    let svg_nodes = svg.dom.elements_by_tag_name("svg");
    if svg_nodes.len() != 1 {
        let message = if svg_nodes.is_empty() {
            "No <svg> tags found."
        } else {
            "Multiple <svg> tags are not supported."
        };
        return Err(Thrown::new(java::ILLEGAL_STATE, message));
    }
    let root_element = svg_nodes[0];
    svg.parse_dimension(root_element)?;
    if svg.view_box.is_none() {
        svg.log_error("Missing \"viewBox\" in <svg> element", Some(root_element));
        return Ok(());
    }
    let root = svg.new_node(root_element, "root".into(), Kind::group())?;
    svg.root = Some(root);

    traverse_svg_and_extract(svg, root, root_element)?;

    resolve_use_nodes(svg)?;
    resolve_gradient_references(svg);

    for (class, nodes) in svg.style_affected_nodes.to_vec() {
        let style = svg.style_class_attribute_map.get(&class).cloned();
        for n in nodes {
            add_style_to_path(svg, n, style.as_deref());
        }
    }

    for (child, group, value) in svg.clip_path_affected_nodes.clone() {
        handle_clip_path(svg, child, group, &value)?;
    }

    svg.flatten();
    svg.validate()?;
    Ok(())
}

/// `<use>` を埋める。
fn resolve_use_nodes(svg: &mut Tree) -> J<()> {
    let pending_use_set = svg.pending_use_set.clone();
    while !svg.pending_use_set.is_empty() {
        // removeIf: 回しながら消すので、後の要素の判定には消えた分が反映される
        let mut changed = false;
        let mut i = 0;
        while i < svg.pending_use_set.len() {
            let node = svg.pending_use_set[i];
            if resolve_use_href(svg, node) {
                svg.pending_use_set.remove(i);
                changed = true;
            } else {
                i += 1;
            }
        }
        if !changed {
            let nodes = svg.pending_use_set.clone();
            report_cycles(svg, &nodes);
            return Ok(());
        }
    }
    let ordering = use_node_topological_ordering(svg, pending_use_set);
    for node in ordering {
        handle_use(svg, node)?;
    }
    Ok(())
}

/// 前の `use` が後の `use` を含むところを参照しない順。
fn use_node_topological_ordering(svg: &Tree, mut pending_use_set: Vec<NodeId>) -> Vec<NodeId> {
    let mut queue: VecDeque<NodeId> = VecDeque::new();
    // 子 → 親と、参照される → 参照する use
    let mut reverse_graph = OrderedMap::<Vec<NodeId>>::default();
    let mut in_degrees = OrderedMap::<i32>::default();
    let Some(root) = svg.root else {
        return pending_use_set;
    };
    queue.push_back(root);
    reverse_graph.insert(root, Vec::new());
    in_degrees.insert(root, 0);
    while let Some(current) = queue.pop_front() {
        if svg.nodes[current].kind.is_group() {
            let mut add = |child: NodeId, queue: &mut VecDeque<NodeId>| {
                let parents = reverse_graph.entry(child);
                if !parents.contains(&current) {
                    parents.push(current);
                }
                if !in_degrees.contains(child) {
                    queue.push_back(child);
                    in_degrees.insert(child, 0);
                }
            };
            for &child in svg.nodes[current].kind.children() {
                add(child, &mut queue);
            }
            if let Kind::Group {
                use_ref: Some(r), ..
            } = svg.nodes[current].kind
            {
                add(r, &mut queue);
            }
        }
    }
    for node in reverse_graph.keys() {
        for &child in reverse_graph.get(node).unwrap() {
            *in_degrees.entry(child) += 1;
        }
    }
    for node in in_degrees.keys() {
        if *in_degrees.get(node).unwrap() == 0 {
            queue.push_back(node);
        }
    }
    let mut ordering = Vec::new();
    while let Some(current) = queue.pop_front() {
        if let Kind::Group {
            use_ref: Some(_), ..
        } = svg.nodes[current].kind
        {
            ordering.push(current);
            pending_use_set.retain(|&n| n != current);
        }
        for &child in reverse_graph.get(current).map(Vec::as_slice).unwrap_or(&[]) {
            let d = in_degrees.entry(child);
            *d -= 1;
            if *d == 0 {
                queue.push_back(child);
            }
        }
    }
    ordering.extend(pending_use_set);
    ordering
}

/// 入れた順の表（Java の識別子のハッシュの表の代わり）。
struct OrderedMap<V> {
    keys: Vec<NodeId>,
    values: StdHashMap<NodeId, V>,
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            values: StdHashMap::new(),
        }
    }
}

impl<V: Default> OrderedMap<V> {
    fn insert(&mut self, k: NodeId, v: V) {
        if self.values.insert(k, v).is_none() {
            self.keys.push(k);
        }
    }

    fn entry(&mut self, k: NodeId) -> &mut V {
        if !self.values.contains_key(&k) {
            self.keys.push(k);
        }
        self.values.entry(k).or_default()
    }

    fn get(&self, k: NodeId) -> Option<&V> {
        self.values.get(&k)
    }

    fn contains(&self, k: NodeId) -> bool {
        self.values.contains_key(&k)
    }

    fn keys(&self) -> Vec<NodeId> {
        self.keys.clone()
    }
}

/// グラデーションの `href` を解く。
fn resolve_gradient_references(svg: &mut Tree) {
    let nodes = std::mem::take(&mut svg.pending_gradient_ref_set);
    // resolveHref はいつも true を返すので 1 回りで済む
    for node in nodes {
        resolve_gradient_href(svg, node);
    }
}

fn report_cycles(svg: &mut Tree, svg_nodes: &[NodeId]) {
    let mut edges: HashMap<String> = HashMap::default();
    let mut nodes_by_id: StdHashMap<String, usize> = StdHashMap::new();
    for &n in svg_nodes {
        let element = svg.nodes[n].element;
        let id = svg.dom.elements[element].get_attribute("id").to_owned();
        if !id.is_empty() {
            let target = svg.href_id(n);
            if !target.is_empty() {
                edges.put(&id, target);
                nodes_by_id.insert(id, element);
            }
        }
    }
    loop {
        let first_entry = edges.iter().next().map(|(k, v)| (k.to_owned(), v.clone()));
        let Some((first, target)) = first_entry else {
            break;
        };
        let mut visited: Vec<String> = Vec::new();
        let mut id = first;
        let mut target_id = Some(target);
        while let Some(t) = target_id.clone() {
            if visited.contains(&id) {
                break;
            }
            visited.push(id.clone());
            id = t;
            target_id = edges.get(&id).cloned();
        }
        if target_id.is_some() {
            let node = nodes_by_id.get(&id).copied();
            let cycle = cycle_starting_at(svg, &id, &edges, &nodes_by_id);
            svg.log_error(
                &format!("Circular dependency of <use> nodes: {cycle}"),
                node,
            );
        }
        for v in &visited {
            edges.remove(v);
        }
    }
}

fn cycle_starting_at(
    svg: &Tree,
    start: &str,
    edges: &HashMap<String>,
    nodes_by_id: &StdHashMap<String, usize>,
) -> String {
    let mut buf = start.to_owned();
    let mut id = start.to_owned();
    loop {
        let Some(next) = edges.get(&id) else {
            buf.push_str(" -> null");
            break;
        };
        id = next.clone();
        buf.push_str(" -> ");
        buf.push_str(&id);
        if id == start {
            break;
        }
        let line = nodes_by_id
            .get(&id)
            .map(|&e| svg.dom.elements[e].line)
            .unwrap_or(0);
        buf.push_str(&format!(" (line {line})"));
    }
    buf
}

/// 木を前順にたどる。
fn traverse_svg_and_extract(svg: &mut Tree, current_group: NodeId, item: usize) -> J<()> {
    let dom = svg.dom;
    let children = &dom.elements[item].children;
    for (i, child) in children.iter().enumerate() {
        let &Child::Element(child_element) = child else {
            continue;
        };
        let e = &svg.dom.elements[child_element];
        if e.children.is_empty() && e.attrs.is_empty() {
            continue; // 何もない要素
        }
        let tag = e.tag.clone();
        match tag.as_str() {
            SVG_PATH | SVG_RECT | SVG_CIRCLE | SVG_ELLIPSE | SVG_POLYGON | SVG_POLYLINE
            | SVG_LINE => {
                let child = svg.new_node(child_element, format!("{tag}{i}"), Kind::leaf())?;
                process_id_name(svg, child);
                svg.add_child(current_group, child);
                extract_all_items_as(svg, child, child_element, current_group)?;
                svg.has_leaf_node = true;
            }
            SVG_GROUP => {
                let child_group =
                    svg.new_node(child_element, format!("child{i}"), Kind::group())?;
                svg.add_child(current_group, child_group);
                process_id_name(svg, child_group);
                extract_group_node(svg, child_group, current_group);
                traverse_svg_and_extract(svg, child_group, child_element)?;
            }
            SVG_USE => {
                let child_group =
                    svg.new_node(child_element, format!("child{i}"), Kind::group())?;
                process_id_name(svg, child_group);
                svg.add_child(current_group, child_group);
                if !svg.pending_use_set.contains(&child_group) {
                    svg.pending_use_set.push(child_group);
                }
            }
            SVG_DEFS => {
                let child_group =
                    svg.new_node(child_element, format!("child{i}"), Kind::group())?;
                traverse_svg_and_extract(svg, child_group, child_element)?;
            }
            SVG_CLIP_PATH_ELEMENT | SVG_MASK => {
                let clip_path =
                    svg.new_node(child_element, format!("{tag}{i}"), Kind::clip_path())?;
                process_id_name(svg, clip_path);
                traverse_svg_and_extract(svg, clip_path, child_element)?;
            }
            SVG_STYLE => extract_style_node(svg, child_element),
            "linearGradient" | "radialGradient" => {
                let gradient =
                    svg.new_node(child_element, format!("{tag}{i}"), Kind::gradient())?;
                process_id_name(svg, gradient);
                extract_gradient_node(svg, gradient);
                let kind = if tag == "linearGradient" {
                    "linear"
                } else {
                    "radial"
                };
                svg.fill_presentation_attributes(gradient, "gradientType", kind);
                svg.has_gradient = true;
            }
            _ => {
                let id = svg.dom.elements[child_element].get_attribute("id");
                if !id.is_empty() {
                    svg.ignored_ids.insert(id.to_owned());
                }
                if is_unsupported(&tag) {
                    svg.log_error(&format!("<{tag}> is not supported"), Some(child_element));
                }
                traverse_svg_and_extract(svg, current_group, child_element)?;
            }
        }
    }
    Ok(())
}

/// グラデーションの要素から属性と色の点を読む。
fn extract_gradient_node(svg: &mut Tree, gradient: NodeId) {
    let element = svg.nodes[gradient].element;
    let dom = svg.dom;
    let e = &dom.elements[element];
    if e.attr(SVG_HREF).is_some() || e.attr(SVG_XLINK_HREF).is_some() {
        svg.pending_gradient_ref_set.push(gradient);
    }
    for a in &e.attrs {
        if GRADIENT_MAP.iter().any(|(k, _)| *k == a.name) {
            svg.fill_presentation_attributes(gradient, &a.name, &a.value);
        }
    }
    // 位置がなければ前の最大の位置
    let mut greatest_offset = 0f64;
    for child in &e.children {
        let &Child::Element(stop) = child else {
            continue;
        };
        let dom = svg.dom;
        let s = &dom.elements[stop];
        if s.tag != "stop" {
            continue;
        }
        let mut color = "rgb(0,0,0)".to_owned();
        let mut opacity = "1".to_owned();
        for a in &s.attrs {
            let (name, value) = (a.name.as_str(), a.value.as_str());
            let result: J<()> = (|| {
                match name {
                    "offset" => greatest_offset = extract_offset(value, greatest_offset)?,
                    "stop-color" => color = value.to_owned(),
                    "stop-opacity" => opacity = value.to_owned(),
                    "style" => {
                        for attr in java::split_char(value, ';') {
                            let split = java::split_char(attr, ':');
                            if split.len() == 2 {
                                if attr.starts_with("stop-color") {
                                    color = split[1].to_owned();
                                } else if attr.starts_with("stop-opacity") {
                                    opacity = split[1].to_owned();
                                }
                            }
                        }
                    }
                    _ => {}
                }
                Ok(())
            })();
            if let Err(e) = result
                && e.is_illegal_argument()
            {
                svg.log_error(
                    &format!("Invalid attribute value: {name}=\"{value}\""),
                    Some(stop),
                );
            }
        }
        let offset = svg.format_coordinate(greatest_offset);
        if let Some(vd_color) = svg.node_color_svg2vd(gradient, &color, "#000000") {
            color = vd_color;
        }
        if let Kind::Gradient { stops, .. } = &mut svg.nodes[gradient].kind {
            stops.push(GradientStop {
                color,
                offset,
                opacity,
            });
        }
    }
}

/// 位置（数か百分率）と前の最大から、新しい最大。
fn extract_offset(offset: &str, greatest_offset: f64) -> J<f64> {
    let x = if let Some(p) = offset.strip_suffix('%') {
        java::parse_double(p)? / 100.0
    } else {
        java::parse_double(offset)?
    };
    let x = java::min(1.0, java::max(x, 0.0));
    Ok(java::max(x, greatest_offset))
}

/// グループの clip-path・mask・class を覚える。
fn extract_group_node(svg: &mut Tree, child_group: NodeId, current_group: NodeId) {
    let element = svg.nodes[child_group].element;
    let dom = svg.dom;
    let attrs = &dom.elements[element].attrs;
    for a in attrs {
        if a.name == SVG_CLIP_PATH || a.name == SVG_MASK {
            if !a.value.is_empty() {
                svg.add_clip_path_affected_node(child_group, current_group, &a.value);
            }
        } else if a.name == "class" && !a.value.is_empty() {
            svg.add_affected_node_to_style_class(&format!(".{}", a.value), child_group);
        }
    }
}

/// `<style>` のクラスを読む。
fn extract_style_node(svg: &mut Tree, element: usize) {
    let children = svg.dom.raw_children(element);
    let len = children.len();
    let mut style_data = String::new();
    for n in &children {
        let cdata = matches!(n, Child::Text { cdata: true, .. });
        if cdata || len == 1 {
            style_data = match n {
                Child::Text { value, .. } => value.clone(),
                Child::Comment(c) => c.clone(),
                Child::Element(_) => String::new(),
            };
        }
    }
    if style_data.is_empty() {
        return;
    }
    for class_data in java::split_char(&style_data, '}') {
        let split = java::split_char(class_data, '{');
        if split.len() < 2 {
            continue;
        }
        let class_name = java::trim(split[0]);
        let style_attr = java::trim(split[1]);
        for name in java::split_char(class_name, ',') {
            let mut attr = style_attr.to_owned();
            let name = java::trim(name);
            if let Some(existing) = svg.style_class_attribute_map.get(name) {
                attr.push(';');
                attr.push_str(existing);
            }
            svg.style_class_attribute_map.insert(name.to_owned(), attr);
        }
    }
}

fn process_id_name(svg: &mut Tree, node: NodeId) {
    let element = svg.nodes[node].element;
    let id = svg.dom.elements[element].get_attribute("id");
    if !id.is_empty() {
        svg.id_map.insert(id.to_owned(), node);
    }
}

/// clip-path を参照するノードを、そのクリップのノードの写しで置き換える。
fn handle_clip_path(svg: &mut Tree, child: NodeId, current_group: NodeId, value: &str) -> J<()> {
    let clip_name = clip_path_name(value);
    let Some(&clip_node) = svg.id_map.get(&clip_name) else {
        return Ok(());
    };
    if !matches!(svg.nodes[clip_node].kind, Kind::ClipPath { .. }) {
        let class = svg.nodes[clip_node].kind.java_class();
        let target = "com.android.ide.common.vectordrawable.SvgClipPathNode";
        return Err(Thrown::new(
            java::CLASS_CAST,
            format!(
                "class {class} cannot be cast to class {target} ({class} and {target} are in \
                 unnamed module of loader 'app')"
            ),
        ));
    }
    let clip_copy = svg.deep_copy(clip_node)?;
    svg.replace_child(current_group, child, clip_copy)?;
    svg.add_affected_node(clip_copy, child);
    // 影響を受けるノードの変換をクリップに重ねる
    let affected = svg.nodes[clip_copy].kind.affected().to_vec();
    for n in affected {
        let t = svg.nodes[n].local_transform;
        svg.nodes[clip_copy].local_transform.concatenate(&t);
    }
    Ok(())
}

/// "url(#clip-path)" から "clip-path"。
fn clip_path_name(s: &str) -> String {
    let start = s.find('#').map(|i| i as isize).unwrap_or(-1);
    let from = (start + 1) as usize;
    let end = s[from..].find(')').map(|i| i + from).unwrap_or(s.len());
    java::trim(&s[from..end]).to_owned()
}

/// 葉の要素の中身を読む。
fn extract_all_items_as(
    svg: &mut Tree,
    child: NodeId,
    current_item: usize,
    current_group: NodeId,
) -> J<()> {
    let mut parent = svg.dom.elements[current_item].parent;
    let mut has_node_attr = false;
    let mut style_content = String::new();
    let mut builder = String::new();
    while let Some(p) = parent {
        let pe = &svg.dom.elements[p];
        if pe.tag != SVG_GROUP {
            break;
        }
        if let Some(style) = pe.attr(SVG_STYLE) {
            builder.push_str(style);
            builder.push(';');
            style_content = builder.clone();
            if style_content.contains("display:none") {
                return Ok(()); // グループごと描かない
            }
            has_node_attr = true;
        }
        if pe.attr(SVG_DISPLAY) == Some("none") {
            return Ok(());
        }
        parent = pe.parent;
    }
    if has_node_attr && !style_content.is_empty() {
        add_style_to_path(svg, child, Some(&style_content));
    }
    let tag = svg.dom.elements[current_item].tag.clone();
    match tag.as_str() {
        SVG_PATH => extract_path_item(svg, child, current_item, current_group),
        SVG_RECT => extract_rect_item(svg, child, current_item, current_group)?,
        SVG_CIRCLE => extract_circle_item(svg, child, current_item, current_group)?,
        SVG_POLYGON | SVG_POLYLINE => extract_poly_item(svg, child, current_item, current_group)?,
        SVG_LINE => extract_line_item(svg, child, current_item, current_group)?,
        SVG_ELLIPSE => extract_ellipse_item(svg, child, current_item, current_group)?,
        _ => {}
    }
    svg.add_affected_node_to_style_class(&tag, child);
    Ok(())
}

/// polygon・polyline をパスに。
fn extract_poly_item(
    svg: &mut Tree,
    child: NodeId,
    element: usize,
    current_group: NodeId,
) -> J<()> {
    let dom = svg.dom;
    let e = &dom.elements[element];
    for a in &e.attrs {
        let (name, value) = (a.name.as_str(), a.value.as_str());
        let result: J<()> = (|| {
            if name == SVG_STYLE {
                add_style_to_path(svg, child, Some(value));
            } else if presentation(name).is_some() {
                svg.fill_presentation_attributes(child, name, value);
            } else if name == SVG_CLIP_PATH || name == SVG_MASK {
                svg.add_clip_path_affected_node(child, current_group, value);
            } else if name == SVG_POINTS {
                let mut builder = PathBuilder::default();
                let split = java::split(value, true, is_space_or_comma);
                let mut base_x = java::parse_float(java::at(&split, 0)?)?;
                let mut base_y = java::parse_float(java::at(&split, 1)?)?;
                builder.absolute_move_to(base_x as f64, base_y as f64)?;
                let mut j = 2;
                while j < split.len() {
                    let x = java::parse_float(split[j])?;
                    let y = java::parse_float(java::at(&split, j as isize + 1)?)?;
                    builder.relative_line_to((x - base_x) as f64, (y - base_y) as f64)?;
                    base_x = x;
                    base_y = y;
                    j += 2;
                }
                if e.tag == SVG_POLYGON {
                    builder.relative_close();
                }
                svg.set_path_data(child, builder.finish());
            } else if name == "class" {
                svg.add_affected_node_to_style_class(&format!(".{value}"), child);
                svg.add_affected_node_to_style_class(&format!("{}.{value}", e.tag), child);
            }
            Ok(())
        })();
        match result {
            Err(err) if err.is_number_format() || err.is_index_out_of_bounds() => {
                svg.log_error_at_line(&format!("Invalid value of \"{name}\" attribute"), a.line);
            }
            // formatFloatValue の IllegalArgumentException は捕まえない
            Err(err) => return Err(err),
            Ok(()) => {}
        }
    }
    Ok(())
}

/// rect をパスに。
fn extract_rect_item(
    svg: &mut Tree,
    child: NodeId,
    element: usize,
    current_group: NodeId,
) -> J<()> {
    let dom = svg.dom;
    let e = &dom.elements[element];
    let (mut x, mut y) = (0f64, 0f64);
    let (mut width, mut height) = (f64::NAN, f64::NAN);
    let (mut rx, mut ry) = (0f64, 0f64);
    let mut pure_transparent = false;
    for a in &e.attrs {
        let (name, value) = (a.name.as_str(), a.value.as_str());
        let result: J<()> = (|| {
            if name == SVG_STYLE {
                add_style_to_path(svg, child, Some(value));
                if value.contains("opacity:0;") {
                    pure_transparent = true;
                }
            } else if presentation(name).is_some() {
                svg.fill_presentation_attributes(child, name, value);
            } else if name == SVG_CLIP_PATH || name == SVG_MASK {
                svg.add_clip_path_affected_node(child, current_group, value);
            } else if name == "x" {
                x = svg.parse_x_value(value)?;
            } else if name == "y" {
                y = svg.parse_y_value(value)?;
            } else if name == "rx" {
                rx = svg.parse_x_value(value)?;
            } else if name == "ry" {
                ry = svg.parse_y_value(value)?;
            } else if name == "width" {
                width = svg.parse_x_value(value)?;
            } else if name == "height" {
                height = svg.parse_y_value(value)?;
            } else if name == "class" {
                svg.add_affected_node_to_style_class(&format!("rect.{value}"), child);
                svg.add_affected_node_to_style_class(&format!(".{value}"), child);
            }
            Ok(())
        })();
        if let Err(err) = result
            && err.is_illegal_argument()
        {
            svg.log_error(
                &format!("Invalid attribute value: {name}=\"{value}\""),
                Some(element),
            );
        }
    }
    if !pure_transparent && !x.is_nan() && !y.is_nan() && !width.is_nan() && !height.is_nan() {
        let path = (|| -> J<String> {
            let mut b = PathBuilder::default();
            if rx <= 0.0 && ry <= 0.0 {
                b.absolute_move_to(x, y)?;
                b.relative_horizontal_to(width)?;
                b.relative_vertical_to(height)?;
                b.relative_horizontal_to(-width)?;
            } else {
                if ry == 0.0 {
                    ry = rx;
                } else if rx == 0.0 {
                    rx = ry;
                }
                if rx > width / 2.0 {
                    rx = width / 2.0;
                }
                if ry > height / 2.0 {
                    ry = height / 2.0;
                }
                b.absolute_move_to(x + rx, y)?;
                b.absolute_line_to(x + width - rx, y)?;
                b.absolute_arc_to(rx, ry, false, false, true, x + width, y + ry)?;
                b.absolute_line_to(x + width, y + height - ry)?;
                b.absolute_arc_to(rx, ry, false, false, true, x + width - rx, y + height)?;
                b.absolute_line_to(x + rx, y + height)?;
                b.absolute_arc_to(rx, ry, false, false, true, x, y + height - ry)?;
                b.absolute_line_to(x, y + ry)?;
                b.absolute_arc_to(rx, ry, false, false, true, x + rx, y)?;
            }
            b.relative_close();
            Ok(b.finish())
        })();
        // 無限の値なら formatFloatValue が投げ、Java ではここで止まる
        svg.set_path_data(child, path?);
    }
    Ok(())
}

/// 葉の属性のうち、図形ごとに同じ扱いのもの。図形の値なら `false` を返す。
fn common_leaf_attribute(
    svg: &mut Tree,
    child: NodeId,
    current_group: NodeId,
    tag: &str,
    name: &str,
    value: &str,
    pure_transparent: &mut bool,
) -> bool {
    if name == SVG_STYLE {
        add_style_to_path(svg, child, Some(value));
        if value.contains("opacity:0;") {
            *pure_transparent = true;
        }
    } else if presentation(name).is_some() {
        svg.fill_presentation_attributes(child, name, value);
    } else if name == SVG_CLIP_PATH || name == SVG_MASK {
        svg.add_clip_path_affected_node(child, current_group, value);
    } else if name == "class" {
        svg.add_affected_node_to_style_class(&format!("{tag}.{value}"), child);
        svg.add_affected_node_to_style_class(&format!(".{value}"), child);
    } else {
        return false;
    }
    true
}

/// circle をパスに。
fn extract_circle_item(
    svg: &mut Tree,
    child: NodeId,
    element: usize,
    current_group: NodeId,
) -> J<()> {
    let dom = svg.dom;
    let e = &dom.elements[element];
    let (mut cx, mut cy, mut radius) = (0f32, 0f32, 0f32);
    let mut pure_transparent = false;
    for a in &e.attrs {
        let (name, value) = (a.name.as_str(), a.value.as_str());
        if common_leaf_attribute(
            svg,
            child,
            current_group,
            SVG_CIRCLE,
            name,
            value,
            &mut pure_transparent,
        ) {
            continue;
        }
        match name {
            "cx" => cx = java::parse_float(value)?,
            "cy" => cy = java::parse_float(value)?,
            "r" => radius = java::parse_float(value)?,
            _ => {}
        }
    }
    if !pure_transparent && !cx.is_nan() && !cy.is_nan() {
        let mut b = PathBuilder::default();
        b.absolute_move_to(cx as f64, cy as f64)?;
        b.relative_move_to(-radius as f64, 0.0)?;
        b.relative_arc_to(
            radius as f64,
            radius as f64,
            false,
            true,
            true,
            (2.0 * radius) as f64,
            0.0,
        )?;
        b.relative_arc_to(
            radius as f64,
            radius as f64,
            false,
            true,
            true,
            (-2.0 * radius) as f64,
            0.0,
        )?;
        svg.set_path_data(child, b.finish());
    }
    Ok(())
}

/// ellipse をパスに。
fn extract_ellipse_item(
    svg: &mut Tree,
    child: NodeId,
    element: usize,
    current_group: NodeId,
) -> J<()> {
    let dom = svg.dom;
    let e = &dom.elements[element];
    let (mut cx, mut cy, mut rx, mut ry) = (0f32, 0f32, 0f32, 0f32);
    let mut pure_transparent = false;
    for a in &e.attrs {
        let (name, value) = (a.name.as_str(), a.value.as_str());
        if common_leaf_attribute(
            svg,
            child,
            current_group,
            SVG_ELLIPSE,
            name,
            value,
            &mut pure_transparent,
        ) {
            continue;
        }
        match name {
            "cx" => cx = java::parse_float(value)?,
            "cy" => cy = java::parse_float(value)?,
            "rx" => rx = java::parse_float(value)?,
            "ry" => ry = java::parse_float(value)?,
            _ => {}
        }
    }
    if !pure_transparent && !cx.is_nan() && !cy.is_nan() && rx > 0.0 && ry > 0.0 {
        let mut b = PathBuilder::default();
        b.absolute_move_to((cx - rx) as f64, cy as f64)?;
        b.relative_arc_to(
            rx as f64,
            ry as f64,
            false,
            true,
            false,
            (2.0 * rx) as f64,
            0.0,
        )?;
        b.relative_arc_to(
            rx as f64,
            ry as f64,
            false,
            true,
            false,
            (-2.0 * rx) as f64,
            0.0,
        )?;
        b.relative_close();
        svg.set_path_data(child, b.finish());
    }
    Ok(())
}

/// line をパスに。
fn extract_line_item(
    svg: &mut Tree,
    child: NodeId,
    element: usize,
    current_group: NodeId,
) -> J<()> {
    let dom = svg.dom;
    let e = &dom.elements[element];
    let (mut x1, mut y1, mut x2, mut y2) = (0f32, 0f32, 0f32, 0f32);
    let mut pure_transparent = false;
    for a in &e.attrs {
        let (name, value) = (a.name.as_str(), a.value.as_str());
        if common_leaf_attribute(
            svg,
            child,
            current_group,
            SVG_LINE,
            name,
            value,
            &mut pure_transparent,
        ) {
            continue;
        }
        match name {
            "x1" => x1 = java::parse_float(value)?,
            "y1" => y1 = java::parse_float(value)?,
            "x2" => x2 = java::parse_float(value)?,
            "y2" => y2 = java::parse_float(value)?,
            _ => {}
        }
    }
    if !pure_transparent && !x1.is_nan() && !y1.is_nan() && !x2.is_nan() && !y2.is_nan() {
        let mut b = PathBuilder::default();
        b.absolute_move_to(x1 as f64, y1 as f64)?;
        b.absolute_line_to(x2 as f64, y2 as f64)?;
        svg.set_path_data(child, b.finish());
    }
    Ok(())
}

fn extract_path_item(svg: &mut Tree, child: NodeId, element: usize, current_group: NodeId) {
    let dom = svg.dom;
    let e = &dom.elements[element];
    for a in &e.attrs {
        let (name, value) = (a.name.as_str(), a.value.as_str());
        if name == SVG_STYLE {
            add_style_to_path(svg, child, Some(value));
        } else if presentation(name).is_some() {
            svg.fill_presentation_attributes(child, name, value);
        } else if name == SVG_CLIP_PATH || name == SVG_MASK {
            svg.add_clip_path_affected_node(child, current_group, value);
        } else if name == SVG_D {
            svg.set_path_data(child, digit_minus_to_comma(value));
        } else if name == "class" {
            svg.add_affected_node_to_style_class(&format!("path.{value}"), child);
            svg.add_affected_node_to_style_class(&format!(".{value}"), child);
        }
    }
}

/// `(\d)-` を `$1,-` に。
fn digit_minus_to_comma(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c.is_ascii_digit() && chars.peek() == Some(&'-') {
            out.push(',');
            out.push('-');
            chars.next();
        }
    }
    out
}

/// `style` の値を属性として入れる。後ろから入れるので、前に書いたものが勝つ。
fn add_style_to_path(svg: &mut Tree, path: NodeId, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    let parts = java::split_char(value, ';');
    for sub_style in parts.iter().rev() {
        let name_value = java::split_char(sub_style, ':');
        if name_value.len() == 2 {
            let attr = java::trim(name_value[0]);
            let val = java::trim(name_value[1]);
            if presentation(attr).is_some() {
                svg.fill_presentation_attributes(path, attr, val);
            }
            // clip-path・mask は属性 clip-path = "url(#name)" と同じに扱う
            if (attr == SVG_CLIP_PATH || attr == SVG_MASK)
                && let Some(parent) = svg.root.and_then(|r| svg.find_parent(r, path))
            {
                svg.add_clip_path_affected_node(path, parent, val);
            }
        }
    }
}

fn parse_float_or_default(value: &str, default: f32) -> f32 {
    if !value.is_empty()
        && let Ok(v) = java::parse_float(value)
    {
        return v;
    }
    default
}

// ---- SvgNode ----

type NodeId = usize;

#[derive(Clone, Debug)]
struct GradientStop {
    color: String,
    offset: String,
    opacity: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GradientUsage {
    Fill,
    Stroke,
}

#[derive(Clone, Debug)]
enum Kind {
    /// `SvgGroupNode`。
    Group {
        children: Vec<NodeId>,
        use_ref: Option<NodeId>,
    },
    /// `SvgClipPathNode`（`SvgGroupNode` の子）。
    ClipPath {
        children: Vec<NodeId>,
        affected: Vec<NodeId>,
    },
    /// `SvgLeafNode`。
    Leaf {
        path_data: Option<String>,
        fill_gradient: Option<NodeId>,
        stroke_gradient: Option<NodeId>,
    },
    /// `SvgGradientNode`。
    Gradient {
        stops: Vec<GradientStop>,
        leaf: Option<NodeId>,
        usage: GradientUsage,
    },
}

impl Kind {
    fn group() -> Self {
        Kind::Group {
            children: Vec::new(),
            use_ref: None,
        }
    }

    fn clip_path() -> Self {
        Kind::ClipPath {
            children: Vec::new(),
            affected: Vec::new(),
        }
    }

    fn leaf() -> Self {
        Kind::Leaf {
            path_data: None,
            fill_gradient: None,
            stroke_gradient: None,
        }
    }

    fn gradient() -> Self {
        Kind::Gradient {
            stops: Vec::new(),
            leaf: None,
            usage: GradientUsage::Fill,
        }
    }

    /// `isGroupNode()`。
    fn is_group(&self) -> bool {
        matches!(self, Kind::Group { .. } | Kind::ClipPath { .. })
    }

    fn children(&self) -> &[NodeId] {
        match self {
            Kind::Group { children, .. } | Kind::ClipPath { children, .. } => children,
            _ => &[],
        }
    }

    fn children_mut(&mut self) -> Option<&mut Vec<NodeId>> {
        match self {
            Kind::Group { children, .. } | Kind::ClipPath { children, .. } => Some(children),
            _ => None,
        }
    }

    fn affected(&self) -> &[NodeId] {
        match self {
            Kind::ClipPath { affected, .. } => affected,
            _ => &[],
        }
    }

    fn java_class(&self) -> &'static str {
        match self {
            Kind::Group { .. } => "com.android.ide.common.vectordrawable.SvgGroupNode",
            Kind::ClipPath { .. } => "com.android.ide.common.vectordrawable.SvgClipPathNode",
            Kind::Leaf { .. } => "com.android.ide.common.vectordrawable.SvgLeafNode",
            Kind::Gradient { .. } => "com.android.ide.common.vectordrawable.SvgGradientNode",
        }
    }
}

#[derive(Clone, Debug)]
struct Node {
    #[allow(dead_code)]
    name: String,
    /// 行の番号を出すための要素。
    element: usize,
    /// VectorDrawable の属性になる SVG の属性。
    vd_attributes: HashMap<String>,
    /// "paint-order: stroke fill" で線を塗りより先に描く。
    stroke_before_fill: bool,
    local_transform: AffineTransform,
    stacked_transform: AffineTransform,
    kind: Kind,
}

const INDENT_UNIT: &str = "  ";
const CONTINUATION_INDENT: &str = "    ";

// ---- SvgTree ----

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum LogLevel {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LogMessage {
    level: LogLevel,
    line: u32,
    message: String,
}

struct Tree<'a> {
    dom: &'a Dom,
    nodes: Vec<Node>,
    w: f32,
    h: f32,
    root_transform: AffineTransform,
    view_box: Option<[f32; 4]>,
    root: Option<NodeId>,
    log_messages: Vec<LogMessage>,
    has_leaf_node: bool,
    has_gradient: bool,
    id_map: StdHashMap<String, NodeId>,
    ignored_ids: HashSet<String>,
    pending_use_set: Vec<NodeId>,
    pending_gradient_ref_set: Vec<NodeId>,
    /// 影響を受けるノード・その親・clip-path の値（`LinkedHashMap`）。
    clip_path_affected_nodes: Vec<(NodeId, NodeId, String)>,
    style_affected_nodes: HashMap<Vec<NodeId>>,
    style_class_attribute_map: StdHashMap<String, String>,
    coordinate_fraction_digits: Option<u32>,
}

impl<'a> Tree<'a> {
    fn new(dom: &'a Dom) -> Self {
        Self {
            dom,
            nodes: Vec::new(),
            w: -1.0,
            h: -1.0,
            root_transform: AffineTransform::default(),
            view_box: None,
            root: None,
            log_messages: Vec::new(),
            has_leaf_node: false,
            has_gradient: false,
            id_map: StdHashMap::new(),
            ignored_ids: HashSet::new(),
            pending_use_set: Vec::new(),
            pending_gradient_ref_set: Vec::new(),
            clip_path_affected_nodes: Vec::new(),
            style_affected_nodes: HashMap::default(),
            style_class_attribute_map: StdHashMap::new(),
            coordinate_fraction_digits: None,
        }
    }

    fn flatten(&mut self) {
        if let Some(root) = self.root {
            self.node_flatten(root, AffineTransform::default());
        }
    }

    fn validate(&mut self) -> J<()> {
        if let Some(root) = self.root {
            self.node_validate(root)?;
        }
        if self.log_messages.is_empty() && !self.has_leaf_node {
            self.log_error("No vector content found", None);
        }
        Ok(())
    }

    fn log_error(&mut self, s: &str, element: Option<usize>) {
        let line = element.map(|e| self.dom.elements[e].line).unwrap_or(0);
        self.log_error_at_line(s, line);
    }

    fn log_error_at_line(&mut self, s: &str, line: u32) {
        self.log_messages.push(LogMessage {
            level: LogLevel::Error,
            line,
            message: s.to_owned(),
        });
    }

    fn log_warning(&mut self, s: &str, element: Option<usize>) {
        let line = element.map(|e| self.dom.elements[e].line).unwrap_or(0);
        self.log_messages.push(LogMessage {
            level: LogLevel::Warning,
            line,
            message: s.to_owned(),
        });
    }

    /// `getErrorMessage()`: 重さ・行・文の順に並べて改行でつなぐ。
    fn error_message(&mut self) -> String {
        self.log_messages.sort_by(|a, b| {
            a.level
                .cmp(&b.level)
                .then(a.line.cmp(&b.line))
                .then_with(|| java::compare(&a.message, &b.message))
        });
        self.log_messages
            .iter()
            .map(|m| {
                let level = match m.level {
                    LogLevel::Error => "ERROR",
                    LogLevel::Warning => "WARNING",
                };
                if m.line == 0 {
                    format!("{level}: {}", m.message)
                } else {
                    format!("{level} @ line {}: {}", m.line, m.message)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn viewport_width(&self) -> f32 {
        self.view_box.map(|v| v[2]).unwrap_or(-1.0)
    }

    fn viewport_height(&self) -> f32 {
        self.view_box.map(|v| v[3]).unwrap_or(-1.0)
    }

    fn parse_dimension(&mut self, element: usize) -> J<()> {
        #[derive(PartialEq)]
        enum SizeType {
            Pixel,
            Percentage,
        }
        let mut width_type = SizeType::Pixel;
        let mut height_type = SizeType::Pixel;
        let dom = self.dom;
        let attrs = &dom.elements[element].attrs;
        for a in attrs {
            let name = java::trim(&a.name);
            let value = java::trim(&a.value);
            let chars: Vec<char> = value.chars().collect();
            let mut sub_string_size = chars.len();
            let mut current_type = SizeType::Pixel;
            let unit: String = chars[chars.len().saturating_sub(2)..].iter().collect();
            if matches!(
                unit.as_str(),
                "em" | "ex" | "px" | "in" | "cm" | "mm" | "pt" | "pc"
            ) {
                sub_string_size -= 2;
            } else if value.ends_with('%') {
                sub_string_size -= 1;
                current_type = SizeType::Percentage;
            }
            let number: String = chars[..sub_string_size].iter().collect();
            if name == "width" {
                self.w = java::parse_float(&number)?;
                width_type = current_type;
            } else if name == "height" {
                self.h = java::parse_float(&number)?;
                height_type = current_type;
            } else if name == "viewBox" {
                let parts = java::split_char(value, ' ');
                let mut view_box = [0f32; 4];
                for (j, v) in view_box.iter_mut().enumerate() {
                    *v = java::parse_float(java::at(&parts, j as isize)?)?;
                }
                self.view_box = Some(view_box);
            }
        }
        // viewBox がなければ幅と高さから
        if self.view_box.is_none() && self.w > 0.0 && self.h > 0.0 {
            self.view_box = Some([0.0, 0.0, self.w, self.h]);
        } else if (self.w < 0.0 || self.h < 0.0)
            && let Some(v) = self.view_box
        {
            self.w = v[2];
            self.h = v[3];
        }
        let no_view_box = || {
            Thrown::new(
                java::NULL_POINTER,
                "Cannot load from float array because \"this.viewBox\" is null",
            )
        };
        if width_type == SizeType::Percentage && self.w > 0.0 {
            self.w = self.view_box.ok_or_else(no_view_box)?[2] * self.w / 100.0;
        }
        if height_type == SizeType::Percentage && self.h > 0.0 {
            self.h = self.view_box.ok_or_else(no_view_box)?[3] * self.h / 100.0;
        }
        Ok(())
    }

    fn parse_x_value(&self, value: &str) -> J<f64> {
        parse_coordinate_or_length(value, self.viewport_width() as f64)
    }

    fn parse_y_value(&self, value: &str) -> J<f64> {
        parse_coordinate_or_length(value, self.viewport_height() as f64)
    }

    fn add_clip_path_affected_node(&mut self, child: NodeId, current_group: NodeId, name: &str) {
        if let Some(entry) = self
            .clip_path_affected_nodes
            .iter_mut()
            .find(|(c, _, _)| *c == child)
        {
            entry.1 = current_group;
            entry.2 = name.to_owned();
        } else {
            self.clip_path_affected_nodes
                .push((child, current_group, name.to_owned()));
        }
    }

    fn add_affected_node_to_style_class(&mut self, class_name: &str, child: NodeId) {
        if let Some(nodes) = self.style_affected_nodes.get_mut(class_name) {
            if !nodes.contains(&child) {
                nodes.push(child);
            }
        } else {
            self.style_affected_nodes.put(class_name, vec![child]);
        }
    }

    /// `findParent`: 根から子だけをたどる。
    fn find_parent(&self, group: NodeId, node: NodeId) -> Option<NodeId> {
        for &n in self.nodes[group].kind.children() {
            if n == node {
                return Some(group);
            }
            if self.nodes[n].kind.is_group()
                && let Some(p) = self.find_parent(n, node)
            {
                return Some(p);
            }
        }
        None
    }

    fn format_coordinate(&mut self, coordinate: f64) -> String {
        let digits = *self.coordinate_fraction_digits.get_or_insert_with(|| {
            java::coordinate_fraction_digits(java::max_f32(
                self.view_box.map(|v| v[3]).unwrap_or(-1.0),
                self.view_box.map(|v| v[2]).unwrap_or(-1.0),
            ))
        });
        java::trim_insignificant_zeros(&java::format_decimal(coordinate, digits))
    }

    fn write_xml(&mut self) -> J<String> {
        let Some(root) = self.root else {
            return Err(Thrown::new(
                java::ILLEGAL_STATE,
                "SvgTree is not fully initialized",
            ));
        };
        let mut out = String::new();
        out.push_str("<vector xmlns:android=\"http://schemas.android.com/apk/res/android\"\n");
        if self.has_gradient {
            out.push_str(CONTINUATION_INDENT);
            out.push_str("xmlns:aapt=\"http://schemas.android.com/aapt\"\n");
        }
        let width = self.format_coordinate(self.w as f64);
        let height = self.format_coordinate(self.h as f64);
        let viewport_width = self.format_coordinate(self.viewport_width() as f64);
        let viewport_height = self.format_coordinate(self.viewport_height() as f64);
        out.push_str(&format!(
            "{CONTINUATION_INDENT}android:width=\"{width}dp\"\n\
             {CONTINUATION_INDENT}android:height=\"{height}dp\"\n\
             {CONTINUATION_INDENT}android:viewportWidth=\"{viewport_width}\"\n\
             {CONTINUATION_INDENT}android:viewportHeight=\"{viewport_height}\">\n"
        ));
        // normalize: viewBox の原点を 0 に
        let view_box = self.view_box.unwrap_or_default();
        let shift =
            AffineTransform::new(1.0, 0.0, 0.0, 1.0, -view_box[0] as f64, -view_box[1] as f64);
        self.root_transform.pre_concatenate(&shift);
        let mut root_transform = self.root_transform;
        self.node_transform_if_needed(root, &mut root_transform)?;
        self.node_write_xml(root, &mut out, INDENT_UNIT)?;
        out.push_str("</vector>\n");
        Ok(out)
    }

    // ---- SvgNode の共通 ----

    /// `new SvgXxxNode(svgTree, element, name)`: 属性から表示の属性と変換を読む。
    fn new_node(&mut self, element: usize, name: String, kind: Kind) -> J<NodeId> {
        let id = self.nodes.len();
        self.nodes.push(Node {
            name,
            element,
            vd_attributes: HashMap::default(),
            stroke_before_fill: false,
            local_transform: AffineTransform::default(),
            stacked_transform: AffineTransform::default(),
            kind,
        });
        let dom = self.dom;
        let attrs = &dom.elements[element].attrs;
        for a in attrs {
            if presentation(&a.name).is_some() {
                self.fill_presentation_attributes_internal(id, &a.name, &a.value);
            }
            if a.name == "transform" {
                self.parse_local_transform(id, &a.value)?;
            }
        }
        Ok(id)
    }

    fn parse_local_transform(&mut self, node: NodeId, value: &str) -> J<()> {
        let value = value.replace(',', " ");
        let matrices = java::split(&value, false, |c| c == '(' || c == ')');
        let mut i = 0;
        while i + 1 < matrices.len() {
            if let Some(t) =
                parse_one_transform(java::trim(matrices[i]), java::trim(matrices[i + 1]))?
            {
                self.nodes[node].local_transform.concatenate(&t);
            }
            i += 2;
        }
        Ok(())
    }

    fn fill_presentation_attributes_internal(&mut self, node: NodeId, name: &str, value: &str) {
        let mut value = value.to_owned();
        match name {
            SVG_PAINT_ORDER => {
                let order = java::split(&value, true, java::is_regex_space);
                let stroke = order.iter().position(|s| *s == SVG_STROKE);
                let fill = order.iter().position(|s| *s == SVG_FILL);
                let stroke = stroke.map(|p| p as isize).unwrap_or(-1);
                let fill = fill.map(|p| p as isize).unwrap_or(-1);
                self.nodes[node].stroke_before_fill = 0 <= stroke && stroke < fill;
                return;
            }
            SVG_FILL_RULE | SVG_CLIP_RULE => {
                if value == "nonzero" {
                    value = "nonZero".into();
                } else if value == "evenodd" {
                    value = "evenOdd".into();
                }
            }
            SVG_STROKE_WIDTH if value == "0" => {
                self.nodes[node].vd_attributes.remove(SVG_STROKE);
            }
            _ => {}
        }
        if value.starts_with("url(") && name != SVG_FILL && name != SVG_STROKE {
            let element = self.nodes[node].element;
            self.log_error(&format!("Unsupported URL value: {value}"), Some(element));
            return;
        }
        if !value.is_empty() {
            self.nodes[node].vd_attributes.put(name, value);
        }
    }

    /// `fillPresentationAttributes`（グループは子にも、子が持っていなければ）。
    fn fill_presentation_attributes(&mut self, node: NodeId, name: &str, value: &str) {
        self.fill_presentation_attributes_internal(node, name, value);
        if self.nodes[node].kind.is_group() {
            for n in self.nodes[node].kind.children().to_vec() {
                if !self.nodes[n].vd_attributes.contains_key(name) {
                    self.fill_presentation_attributes(n, name, value);
                }
            }
        }
    }

    /// 親の属性のうち、ないものを入れる。
    fn fill_empty_attributes(&mut self, node: NodeId, parent: &HashMap<String>) {
        for (name, value) in parent.iter() {
            if !self.nodes[node].vd_attributes.contains_key(name) {
                self.nodes[node].vd_attributes.put(name, value.clone());
            }
        }
    }

    fn copy_from(&mut self, to: NodeId, from: NodeId) {
        let attrs = self.nodes[from].vd_attributes.clone();
        self.fill_empty_attributes(to, &attrs);
        self.nodes[to].local_transform = self.nodes[from].local_transform;
    }

    fn deep_copy(&mut self, node: NodeId) -> J<NodeId> {
        let (element, name) = (self.nodes[node].element, self.nodes[node].name.clone());
        match self.nodes[node].kind.clone() {
            Kind::Leaf { path_data, .. } => {
                let copy = self.new_node(element, name, Kind::leaf())?;
                self.copy_from(copy, node);
                self.set_path_data_opt(copy, path_data);
                Ok(copy)
            }
            Kind::Group { children, .. } => {
                let copy = self.new_node(element, name, Kind::group())?;
                self.copy_from(copy, node);
                for c in children {
                    let cc = self.deep_copy(c)?;
                    self.add_child(copy, cc);
                }
                Ok(copy)
            }
            Kind::ClipPath { children, affected } => {
                let copy = self.new_node(element, name, Kind::clip_path())?;
                self.copy_from(copy, node);
                for c in children {
                    let cc = self.deep_copy(c)?;
                    self.add_child(copy, cc);
                }
                for a in affected {
                    self.add_affected_node(copy, a);
                }
                Ok(copy)
            }
            Kind::Gradient { .. } => {
                let copy = self.new_node(element, name, Kind::gradient())?;
                self.copy_gradient_from(copy, node);
                Ok(copy)
            }
        }
    }

    fn copy_gradient_from(&mut self, to: NodeId, from: NodeId) {
        self.copy_from(to, from);
        let from_stops = match &self.nodes[from].kind {
            Kind::Gradient { stops, .. } => stops.clone(),
            _ => Vec::new(),
        };
        if let Kind::Gradient { stops, .. } = &mut self.nodes[to].kind
            && stops.is_empty()
        {
            *stops = from_stops;
        }
    }

    fn add_child(&mut self, group: NodeId, child: NodeId) {
        if let Some(children) = self.nodes[group].kind.children_mut() {
            children.push(child);
        }
        let attrs = self.nodes[group].vd_attributes.clone();
        self.fill_empty_attributes(child, &attrs);
    }

    fn add_affected_node(&mut self, clip: NodeId, child: NodeId) {
        if let Kind::ClipPath { affected, .. } = &mut self.nodes[clip].kind {
            affected.push(child);
        }
        let attrs = self.nodes[clip].vd_attributes.clone();
        self.fill_empty_attributes(child, &attrs);
    }

    fn replace_child(&mut self, group: NodeId, old: NodeId, new: NodeId) -> J<()> {
        let children = self.nodes[group].kind.children_mut();
        match children.and_then(|c| c.iter().position(|&n| n == old).map(|i| (c, i))) {
            Some((c, i)) => {
                c[i] = new;
                Ok(())
            }
            None => Err(Thrown::new(
                java::ILLEGAL_ARGUMENT,
                "The child being replaced doesn't belong to this group",
            )),
        }
    }

    fn set_path_data(&mut self, leaf: NodeId, data: String) {
        self.set_path_data_opt(leaf, Some(data));
    }

    fn set_path_data_opt(&mut self, leaf: NodeId, data: Option<String>) {
        if let Kind::Leaf { path_data, .. } = &mut self.nodes[leaf].kind {
            *path_data = data;
        }
    }

    /// `href`・`xlink:href` が指す id（"#" を除く）。
    fn href_id(&self, node: NodeId) -> String {
        let e = &self.dom.elements[self.nodes[node].element];
        let mut value = e.get_attribute(SVG_HREF);
        if value.is_empty() {
            value = e.get_attribute(SVG_XLINK_HREF);
        }
        value.chars().skip(1).collect()
    }

    /// `colorSvg2Vd` にノードの行でエラーを出すもの。
    fn node_color_svg2vd(&mut self, node: NodeId, color: &str, fallback: &str) -> Option<String> {
        match color_svg2vd(color) {
            Ok(c) => c,
            Err(()) => {
                let element = self.nodes[node].element;
                self.log_error(
                    &format!("Unsupported color format \"{color}\""),
                    Some(element),
                );
                Some(fallback.to_owned())
            }
        }
    }

    fn node_flatten(&mut self, node: NodeId, transform: AffineTransform) {
        match self.nodes[node].kind.clone() {
            Kind::Group { children, .. } => {
                for c in children {
                    let mut stacked = transform;
                    stacked.concatenate(&self.nodes[node].local_transform);
                    self.nodes[node].stacked_transform = stacked;
                    self.node_flatten(c, stacked);
                }
            }
            Kind::ClipPath { children, affected } => {
                for c in children {
                    let mut stacked = transform;
                    stacked.concatenate(&self.nodes[node].local_transform);
                    self.nodes[node].stacked_transform = stacked;
                    self.node_flatten(c, stacked);
                }
                self.nodes[node].stacked_transform = transform;
                for a in affected {
                    // クリップの変換は影響を受けるノードにはかけない
                    self.node_flatten(a, transform);
                }
                let local = self.nodes[node].local_transform;
                let mut stacked = transform;
                stacked.concatenate(&local);
                let scales = stacked.get_type() & java::TYPE_MASK_SCALE != 0;
                self.nodes[node].stacked_transform = stacked;
                if self.nodes[node]
                    .vd_attributes
                    .contains_key(SVG_STROKE_WIDTH)
                    && scales
                {
                    let element = self.nodes[node].element;
                    self.log_warning("Scaling of the stroke width is ignored", Some(element));
                }
            }
            Kind::Leaf { .. } => {
                let mut stacked = transform;
                stacked.concatenate(&self.nodes[node].local_transform);
                let non_scaling = self.nodes[node]
                    .vd_attributes
                    .get("vector-effect")
                    .map(String::as_str)
                    == Some("non-scaling-stroke");
                if !non_scaling && stacked.get_type() & java::TYPE_MASK_SCALE != 0 {
                    let stroke_width = self.nodes[node]
                        .vd_attributes
                        .get(SVG_STROKE_WIDTH)
                        .cloned();
                    if let Some(stroke_width) = stroke_width
                        && let Ok(mut width) = java::parse_double(&stroke_width)
                    {
                        // VectorDrawable は線の形を変換できないので、線の幅に拡大率をかける
                        let determinant = stacked.get_determinant();
                        if determinant != 0.0 {
                            width *= determinant.abs().sqrt();
                            let w = self.format_coordinate(width);
                            self.nodes[node].vd_attributes.put(SVG_STROKE_WIDTH, w);
                        }
                        if stacked.get_type() & java::TYPE_GENERAL_SCALE != 0 {
                            let element = self.nodes[node].element;
                            self.log_warning(
                                "Scaling of the stroke width is approximate",
                                Some(element),
                            );
                        }
                    }
                }
                self.nodes[node].stacked_transform = stacked;
            }
            Kind::Gradient { .. } => {
                let mut stacked = transform;
                stacked.concatenate(&self.nodes[node].local_transform);
                self.nodes[node].stacked_transform = stacked;
            }
        }
    }

    fn node_validate(&mut self, node: NodeId) -> J<()> {
        match self.nodes[node].kind.clone() {
            Kind::Group { children, .. } => {
                for c in children {
                    self.node_validate(c)?;
                }
            }
            Kind::ClipPath { children, affected } => {
                // super.validate() と、さらに子をもう一度
                for c in &children {
                    self.node_validate(*c)?;
                }
                for c in &children {
                    self.node_validate(*c)?;
                }
                for a in &affected {
                    self.node_validate(*a)?;
                }
                let element = self.nodes[node].element;
                if self.dom.elements[element].tag == SVG_MASK && !self.is_white_fill(node)? {
                    // 白一色でないマスクは、クリップでは表せない半透明になる
                    self.log_error(
                        "Semitransparent mask cannot be represented by a vector drawable",
                        Some(element),
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn is_white_fill(&mut self, node: NodeId) -> J<bool> {
        let Some(fill) = self.nodes[node].vd_attributes.get("fill").cloned() else {
            return Ok(false);
        };
        let Some(fill) = self.node_color_svg2vd(node, &fill, "#000") else {
            return Ok(false);
        };
        Ok(parse_color_value(&fill)? == 0xFFFFFFFFu32 as i32)
    }

    fn node_transform_if_needed(&mut self, node: NodeId, root: &mut AffineTransform) -> J<()> {
        match self.nodes[node].kind.clone() {
            Kind::Group { children, .. } => {
                for c in children {
                    self.node_transform_if_needed(c, root)?;
                }
            }
            Kind::ClipPath { children, affected } => {
                for c in children.into_iter().chain(affected) {
                    self.node_transform_if_needed(c, root)?;
                }
            }
            Kind::Leaf { path_data, .. } => {
                let Some(data) = path_data.filter(|d| !d.is_empty()) else {
                    return Ok(()); // 描くものがない
                };
                let mut nodes = path::parse_path(&data, ParseMode::Svg)?;
                self.nodes[node].stacked_transform.pre_concatenate(root);
                let needs_convert = path::has_rel_move_after_close(&nodes);
                let mut stacked = self.nodes[node].stacked_transform;
                if !stacked.is_identity() || needs_convert {
                    path::transform_nodes(&mut stacked, &mut nodes)?;
                }
                self.nodes[node].stacked_transform = stacked;
                let data = self.node_list_to_string(&nodes)?;
                self.set_path_data(node, data);
            }
            Kind::Gradient { .. } => {}
        }
        Ok(())
    }

    fn node_list_to_string(&mut self, nodes: &[path::PathNode]) -> J<String> {
        let digits = self.coordinate_fraction_digits.unwrap_or_else(|| {
            java::coordinate_fraction_digits(java::max_f32(
                self.viewport_height(),
                self.viewport_width(),
            ))
        });
        self.coordinate_fraction_digits = Some(digits);
        path::node_list_to_string(nodes, |v| {
            java::trim_insignificant_zeros(&java::format_decimal(v, digits))
        })
    }

    fn node_write_xml(&mut self, node: NodeId, out: &mut String, indent: &str) -> J<()> {
        match self.nodes[node].kind.clone() {
            Kind::Group { children, .. } => {
                for c in children {
                    self.node_write_xml(c, out, indent)?;
                }
            }
            Kind::ClipPath { children, affected } => {
                self.clip_path_write_xml(&children, &affected, out, indent)?;
            }
            Kind::Leaf { path_data, .. } => {
                if path_data.as_deref().is_none_or(str::is_empty) {
                    return Ok(()); // 描くパスがない
                }
                if self.nodes[node].stroke_before_fill {
                    // 塗りを線の上に描くため、塗りなしと線なしの 2 つの <path> にする
                    self.write_path_with_suppressed(node, SVG_FILL, out, indent)?;
                    self.write_path_with_suppressed(node, SVG_STROKE, out, indent)?;
                } else {
                    self.write_path_element(node, out, indent)?;
                }
            }
            Kind::Gradient { .. } => self.gradient_write_xml(node, out, indent)?,
        }
        Ok(())
    }

    fn clip_path_write_xml(
        &mut self,
        children: &[NodeId],
        affected: &[NodeId],
        out: &mut String,
        indent: &str,
    ) -> J<()> {
        out.push_str(indent);
        out.push_str("<group>\n");
        let incremented = format!("{indent}{INDENT_UNIT}");
        let mut non_zero: Option<Vec<String>> = None;
        let mut even_odd: Option<Vec<String>> = None;
        for &c in children {
            self.collect_clip_paths(c, &mut non_zero, &mut even_odd);
        }
        for (rule, paths) in [("nonZero", non_zero), ("evenOdd", even_odd)] {
            let Some(paths) = paths else {
                continue;
            };
            out.push_str(&incremented);
            out.push_str("<clip-path\n");
            out.push_str(&incremented);
            out.push_str(INDENT_UNIT);
            out.push_str(INDENT_UNIT);
            out.push_str("android:pathData=\"");
            for (i, p) in paths.iter().enumerate() {
                if i > 0 && !p.starts_with('M') {
                    // 座標系の原点に戻す
                    out.push_str("M 0,0");
                }
                out.push_str(p);
            }
            out.push('"');
            if rule == "evenOdd" {
                out.push('\n');
                out.push_str(&incremented);
                out.push_str(INDENT_UNIT);
                out.push_str(INDENT_UNIT);
                out.push_str("android:fillType=\"evenOdd\"");
            }
            out.push_str("/>\n");
        }
        for &a in affected {
            self.node_write_xml(a, out, &incremented)?;
        }
        out.push_str(indent);
        out.push_str("</group>\n");
        Ok(())
    }

    /// クリップの子の葉のパスを、塗りの規則ごとに集める（`accept(Visitor)`）。
    fn collect_clip_paths(
        &self,
        node: NodeId,
        non_zero: &mut Option<Vec<String>>,
        even_odd: &mut Option<Vec<String>>,
    ) {
        let n = &self.nodes[node];
        match &n.kind {
            Kind::Leaf {
                path_data: Some(data),
                ..
            } if !data.is_empty() => {
                let even =
                    n.vd_attributes.get(SVG_CLIP_RULE).map(String::as_str) == Some("evenOdd");
                let list = if even { even_odd } else { non_zero };
                list.get_or_insert_with(Vec::new).push(data.clone());
            }
            Kind::Group { children, .. } | Kind::ClipPath { children, .. } => {
                for &c in children {
                    self.collect_clip_paths(c, non_zero, even_odd);
                }
            }
            _ => {}
        }
    }

    fn write_path_with_suppressed(
        &mut self,
        node: NodeId,
        attribute: &str,
        out: &mut String,
        indent: &str,
    ) -> J<()> {
        let saved = self.nodes[node]
            .vd_attributes
            .put(attribute, "#00000000".into());
        self.write_path_element(node, out, indent)?;
        match saved {
            None => {
                self.nodes[node].vd_attributes.remove(attribute);
            }
            Some(v) => {
                self.nodes[node].vd_attributes.put(attribute, v);
            }
        }
        Ok(())
    }

    fn leaf_gradients(&self, node: NodeId) -> (Option<NodeId>, Option<NodeId>) {
        match &self.nodes[node].kind {
            Kind::Leaf {
                fill_gradient,
                stroke_gradient,
                ..
            } => (*fill_gradient, *stroke_gradient),
            _ => (None, None),
        }
    }

    fn write_path_element(&mut self, node: NodeId, out: &mut String, indent: &str) -> J<()> {
        let fill_color = self.nodes[node].vd_attributes.get(SVG_FILL).cloned();
        let stroke_color = self.nodes[node].vd_attributes.get(SVG_STROKE).cloned();
        let empty_fill = matches!(fill_color.as_deref(), Some("none") | Some("#00000000"));
        let empty_stroke = matches!(stroke_color.as_deref(), None | Some("none"));
        if empty_fill && empty_stroke {
            return Ok(()); // 描くものがない
        }
        out.push_str(indent);
        out.push_str("<path\n");
        let (fill_gradient, stroke_gradient) = self.leaf_gradients(node);
        if fill_color.is_none() && fill_gradient.is_none() {
            out.push_str(indent);
            out.push_str(CONTINUATION_INDENT);
            out.push_str("android:fillColor=\"#FF000000\"\n");
        }
        if !empty_stroke
            && !self.nodes[node]
                .vd_attributes
                .contains_key(SVG_STROKE_WIDTH)
            && stroke_gradient.is_none()
        {
            out.push_str(indent);
            out.push_str(CONTINUATION_INDENT);
            out.push_str("android:strokeWidth=\"1\"\n");
        }
        let path_data = match &self.nodes[node].kind {
            Kind::Leaf { path_data, .. } => path_data.clone().unwrap_or_default(),
            _ => String::new(),
        };
        out.push_str(indent);
        out.push_str(CONTINUATION_INDENT);
        out.push_str(&format!("android:pathData=\"{path_data}\""));
        self.write_attribute_values(node, out, indent)?;
        let (fill_gradient, stroke_gradient) = self.leaf_gradients(node);
        let has_gradient = fill_gradient.is_some() || stroke_gradient.is_some();
        if !has_gradient {
            out.push('/');
        }
        out.push_str(">\n");
        let inner = format!("{indent}{INDENT_UNIT}");
        if let Some(g) = fill_gradient {
            self.gradient_write_xml(g, out, &inner)?;
        }
        if let Some(g) = stroke_gradient {
            self.gradient_write_xml(g, out, &inner)?;
        }
        if has_gradient {
            out.push_str(indent);
            out.push_str("</path>\n");
        }
        Ok(())
    }

    fn write_attribute_values(&mut self, node: NodeId, out: &mut String, indent: &str) -> J<()> {
        // opacity と fill-opacity・stroke-opacity の重なりを解く
        self.parse_path_opacity(node);
        for (name, value) in self.nodes[node].vd_attributes.to_vec() {
            let Some(attribute) = presentation(&name).filter(|a| !a.is_empty()) else {
                continue;
            };
            let svg_value = java::trim(&value).to_owned();
            let mut vd_value = self.node_color_svg2vd(node, &svg_value, "#000000");
            if vd_value.is_none() {
                if (name == SVG_FILL || name == SVG_STROKE)
                    && let Some(gradient) = self.gradient_node(&svg_value)
                {
                    {
                        let copy = self.deep_copy(gradient)?;
                        let usage = if name == SVG_FILL {
                            GradientUsage::Fill
                        } else {
                            GradientUsage::Stroke
                        };
                        if let Kind::Gradient { leaf, usage: u, .. } = &mut self.nodes[copy].kind {
                            *leaf = Some(node);
                            *u = usage;
                        }
                        if let Kind::Leaf {
                            fill_gradient,
                            stroke_gradient,
                            ..
                        } = &mut self.nodes[node].kind
                        {
                            if usage == GradientUsage::Fill {
                                *fill_gradient = Some(copy);
                            } else {
                                *stroke_gradient = Some(copy);
                            }
                        }
                        continue;
                    }
                }
                vd_value = Some(match svg_value.strip_suffix("px") {
                    Some(v) => java::trim(v).to_owned(),
                    None => svg_value.clone(),
                });
            }
            out.push('\n');
            out.push_str(indent);
            out.push_str(CONTINUATION_INDENT);
            out.push_str(attribute);
            out.push_str("=\"");
            out.push_str(&vd_value.unwrap_or_default());
            out.push('"');
        }
        Ok(())
    }

    fn gradient_node(&self, svg_value: &str) -> Option<NodeId> {
        let id = svg_value.strip_prefix("url(#")?.strip_suffix(')')?;
        let &node = self.id_map.get(id)?;
        matches!(self.nodes[node].kind, Kind::Gradient { .. }).then_some(node)
    }

    fn parse_path_opacity(&mut self, node: NodeId) {
        let opacity = self.opacity_value(node, SVG_OPACITY);
        let fill_opacity = self.opacity_value(node, SVG_FILL_OPACITY);
        let stroke_opacity = self.opacity_value(node, SVG_STROKE_OPACITY);
        self.put_opacity_value(node, SVG_FILL_OPACITY, fill_opacity * opacity);
        self.put_opacity_value(node, SVG_STROKE_OPACITY, stroke_opacity * opacity);
        self.nodes[node].vd_attributes.remove(SVG_OPACITY);
    }

    /// 不透明度（0〜1 に寄せる、なければ 1）。
    fn opacity_value(&self, node: NodeId, name: &str) -> f64 {
        let mut result = 1.0;
        if let Some(opacity) = self.nodes[node].vd_attributes.get(name) {
            let parsed = match opacity.strip_suffix('%') {
                Some(p) => java::parse_double(p).map(|v| v / 100.0),
                None => java::parse_double(opacity),
            };
            if let Ok(v) = parsed {
                result = v;
            }
        }
        java::min(java::max(result, 0.0), 1.0)
    }

    fn put_opacity_value(&mut self, node: NodeId, name: &str, opacity: f64) {
        let value = java::format_float_value(opacity).unwrap_or_else(|_| "NaN".into());
        if value == "1" {
            self.nodes[node].vd_attributes.remove(name);
        } else {
            self.nodes[node].vd_attributes.put(name, value);
        }
    }

    // ---- SvgGradientNode ----

    fn gradient_coordinate(&mut self, node: NodeId, x: &str, default: f64) -> (f64, bool) {
        let Some(value) = self.nodes[node].vd_attributes.get(x) else {
            return (default, false);
        };
        let vd_value = java::trim(value).to_owned();
        if x == "r" && vd_value.starts_with('-') {
            return (default, false);
        }
        let mut val = default;
        let mut percentage = false;
        let parsed = match vd_value.strip_suffix('%') {
            Some(p) => java::parse_double(p).map(|v| (v / 100.0, true)),
            None => java::parse_double(&vd_value).map(|v| (v, false)),
        };
        match parsed {
            Ok((v, p)) => {
                val = v;
                percentage = p;
            }
            Err(_) => {
                let element = self.nodes[node].element;
                self.log_error("Unsupported coordinate value", Some(element));
            }
        }
        (val, percentage)
    }

    fn gradient_write_xml(&mut self, node: NodeId, out: &mut String, indent: &str) -> J<()> {
        let (stops, leaf, usage) = match &self.nodes[node].kind {
            Kind::Gradient { stops, leaf, usage } => (stops.clone(), *leaf, *usage),
            _ => return Ok(()),
        };
        let element = self.nodes[node].element;
        if stops.is_empty() {
            self.log_error("Gradient has no stop info", Some(element));
            return Ok(());
        }
        let Some(leaf) = leaf else {
            return Err(Thrown::new(
                java::NULL_POINTER,
                "Cannot invoke \"com.android.ide.common.vectordrawable.SvgLeafNode.getPathData()\" \
                 because \"this.mSvgLeafNode\" is null",
            ));
        };
        // 既定では、グラデーションの大きさはパスの外接矩形
        let leaf_path = match &self.nodes[leaf].kind {
            Kind::Leaf { path_data, .. } => path_data.clone().unwrap_or_default(),
            _ => String::new(),
        };
        let path_nodes = path::parse_path(&leaf_path, ParseMode::Svg)?;
        let mut path2d = Path2D::default();
        path::create_path(&path_nodes, &mut path2d);
        let (mut start_x, mut start_y, mut width, mut height) = path2d.bounds();

        let gradient_unit = self.nodes[node].vd_attributes.get("gradientUnits").cloned();
        let user_space_on_use = gradient_unit.as_deref() == Some("userSpaceOnUse");
        // userSpaceOnUse なら絵の大きさ
        if user_space_on_use {
            start_x = 0.0;
            start_y = 0.0;
            height = self.h as f64;
            width = self.w as f64;
        }
        if width == 0.0 || height == 0.0 {
            return Ok(()); // 面積がないので見えない
        }
        out.push_str(indent);
        out.push_str(if usage == GradientUsage::Fill {
            "<aapt:attr name=\"android:fillColor\">\n"
        } else {
            "<aapt:attr name=\"android:strokeColor\">\n"
        });
        out.push_str(indent);
        out.push_str(INDENT_UNIT);
        out.push_str("<gradient");

        if let Some(transform) = self.nodes[node]
            .vd_attributes
            .get("gradientTransform")
            .cloned()
        {
            self.parse_local_transform(node, &transform)?;
            if !user_space_on_use {
                let mut tr = AffineTransform::new(width, 0.0, 0.0, height, 0.0, 0.0);
                self.nodes[node].local_transform.pre_concatenate(&tr);
                if let Err(e) = tr.invert() {
                    return Err(Thrown::new(
                        java::ERROR,
                        format!("java.awt.geom.NoninvertibleTransformException: {e}"),
                    ));
                }
                self.nodes[node].local_transform.concatenate(&tr);
            }
        }
        // 図形の変換（stacked）は userSpaceOnUse のときだけかかる。外接矩形はもう変換済み
        if user_space_on_use {
            let stacked = self.nodes[leaf].stacked_transform;
            self.nodes[node].local_transform.pre_concatenate(&stacked);
        }
        let gradient_type = self.nodes[node]
            .vd_attributes
            .get("gradientType")
            .cloned()
            .unwrap_or_else(|| "linear".into());
        let transformed: Vec<f64>;
        if gradient_type == "linear" {
            let mut bounds = [0f64; 4];
            for (index, s) in ["x1", "y1", "x2", "y2"].iter().enumerate() {
                let default = if index == 2 { 1.0 } else { 0.0 };
                let (mut value, percentage) = self.gradient_coordinate(node, s, default);
                if !user_space_on_use || percentage {
                    if index % 2 == 0 {
                        value = value * width + start_x;
                    } else {
                        value = value * height + start_y;
                    }
                }
                bounds[index] = value;
                // 既定の値も書くため、ない座標も表に入れる
                if !self.nodes[node].vd_attributes.contains_key(s) {
                    self.nodes[node].vd_attributes.put(s, String::new());
                }
            }
            let t = self.nodes[node].local_transform;
            let p0 = t.transform_point(bounds[0], bounds[1]);
            let p1 = t.transform_point(bounds[2], bounds[3]);
            transformed = vec![p0.0, p0.1, p1.0, p1.1];
        } else {
            let (mut cx, p) = self.gradient_coordinate(node, "cx", 0.5);
            if !user_space_on_use || p {
                cx = width * cx + start_x;
            }
            let (mut cy, p) = self.gradient_coordinate(node, "cy", 0.5);
            if !user_space_on_use || p {
                cy = height * cy + start_y;
            }
            let (mut r, p) = self.gradient_coordinate(node, "r", 0.5);
            if !user_space_on_use || p {
                r *= java::max(height, width);
            }
            let t = self.nodes[node].local_transform;
            let c = t.transform_point(cx, cy);
            let radius = t.delta_transform_point(r, 0.0);
            transformed = vec![c.0, c.1];
            let cx = self.format_coordinate(c.0);
            let cy = self.format_coordinate(c.1);
            let (dx, dy) = (0.0 - radius.0, 0.0 - radius.1);
            let r = self.format_coordinate((dx * dx + dy * dy).sqrt());
            let attrs = &mut self.nodes[node].vd_attributes;
            attrs.put("cx", cx);
            attrs.put("cy", cy);
            attrs.put("r", r);
        }
        for (svg_attribute, gradient_attr) in GRADIENT_MAP {
            let Some(value) = self.nodes[node].vd_attributes.get(svg_attribute).cloned() else {
                continue;
            };
            if gradient_attr.is_empty() {
                continue;
            }
            let svg_value = java::trim(&value).to_owned();
            let mut vd_value = self.node_color_svg2vd(node, &svg_value, "#000000");
            if vd_value.is_none() {
                let index = ["x1", "y1", "x2", "y2"]
                    .iter()
                    .position(|k| *k == svg_attribute);
                vd_value = Some(if let Some(i) = index {
                    let x = java::at(&transformed, i as isize)?;
                    self.format_coordinate(x)
                } else if svg_attribute == "spreadMethod" {
                    match svg_value.as_str() {
                        "pad" => "clamp".into(),
                        "reflect" => "mirror".into(),
                        "repeat" => "repeat".into(),
                        _ => {
                            self.log_error(
                                &format!("Unsupported spreadMethod {svg_value}"),
                                Some(element),
                            );
                            "clamp".into()
                        }
                    }
                } else if svg_value.ends_with('%') {
                    let (c, _) = self.gradient_coordinate(node, svg_attribute, 0.0);
                    self.format_coordinate(c)
                } else {
                    svg_value.clone()
                });
            }
            out.push('\n');
            out.push_str(indent);
            out.push_str(INDENT_UNIT);
            out.push_str(CONTINUATION_INDENT);
            out.push_str(gradient_attr);
            out.push_str("=\"");
            out.push_str(&vd_value.unwrap_or_default());
            out.push('"');
        }
        out.push_str(">\n");
        let stop_indent = format!("{indent}{INDENT_UNIT}{INDENT_UNIT}");
        self.write_gradient_stops(node, &stops, out, &stop_indent);
        out.push_str(indent);
        out.push_str(INDENT_UNIT);
        out.push_str("</gradient>\n");
        out.push_str(indent);
        out.push_str("</aapt:attr>\n");
        Ok(())
    }

    fn write_gradient_stops(
        &mut self,
        node: NodeId,
        stops: &[GradientStop],
        out: &mut String,
        indent: &str,
    ) {
        let element = self.nodes[node].element;
        for g in stops {
            let mut color = g.color.clone();
            let opacity = match java::parse_float(&g.opacity) {
                Ok(o) => o,
                Err(_) => {
                    self.log_warning("Unsupported opacity value", Some(element));
                    1.0
                }
            };
            match parse_color_value(&color) {
                Ok(c) => color = format!("#{:08X}", apply_alpha(c, opacity) as u32),
                Err(_) => {
                    self.log_warning(&format!("Unsupported color value {color}"), Some(element))
                }
            }
            let offset = java::trim_insignificant_zeros(&g.offset);
            out.push_str(&format!(
                "{indent}<item android:offset=\"{offset}\" android:color=\"{color}\"/>\n"
            ));
            if stops.len() == 1 {
                self.log_warning("Gradient has only one color stop", Some(element));
                out.push_str(&format!(
                    "{indent}<item android:offset=\"1\" android:color=\"{color}\"/>\n"
                ));
            }
        }
    }
}

fn parse_coordinate_or_length(value: &str, percentage_base: f64) -> J<f64> {
    match value.strip_suffix('%') {
        Some(v) => Ok(java::parse_double(v)? / 100.0 * percentage_base),
        None => java::parse_double(value),
    }
}

/// `SvgNode.parseOneTransform`: 変換の 1 つ。知らない種類は単位行列。
fn parse_one_transform(kind: &str, data: &str) -> J<Option<AffineTransform>> {
    let numbers = java::split(data, true, java::is_regex_space)
        .into_iter()
        .map(java::parse_float)
        .collect::<J<Vec<f32>>>()?;
    let n = numbers.len();
    let mut t = AffineTransform::default();
    let kind = kind.to_ascii_lowercase();
    match kind.as_str() {
        "matrix" => {
            if n != 6 {
                return Ok(None);
            }
            let m: Vec<f64> = numbers.iter().map(|&v| v as f64).collect();
            t.set_transform(m[0], m[1], m[2], m[3], m[4], m[5]);
        }
        "translate" => {
            if n != 1 && n != 2 {
                return Ok(None);
            }
            t.translate(
                numbers[0] as f64,
                if n == 2 { numbers[1] as f64 } else { 0.0 },
            );
        }
        "scale" => {
            if n != 1 && n != 2 {
                return Ok(None);
            }
            t.scale(
                numbers[0] as f64,
                numbers[if n == 2 { 1 } else { 0 }] as f64,
            );
        }
        "rotate" => {
            if n != 1 && n != 3 {
                return Ok(None);
            }
            t.rotate_around(
                java::to_radians(numbers[0] as f64),
                if n == 3 { numbers[1] as f64 } else { 0.0 },
                if n == 3 { numbers[2] as f64 } else { 0.0 },
            );
        }
        "skewx" => {
            if n != 1 {
                return Ok(None);
            }
            // SVG は tan(a) を行列に入れる
            t.shear(java::to_radians(numbers[0] as f64).tan(), 0.0);
        }
        "skewy" => {
            if n != 1 {
                return Ok(None);
            }
            t.shear(0.0, java::to_radians(numbers[0] as f64).tan());
        }
        _ => {}
    }
    Ok(Some(t))
}

/// `use` の参照を解く。まだ解けない（参照先が未処理の use）なら false。
fn resolve_use_href(svg: &mut Tree, node: NodeId) -> bool {
    let id = svg.href_id(node);
    let reference = if id.is_empty() {
        None
    } else {
        svg.id_map.get(&id).copied()
    };
    if let Kind::Group { use_ref, .. } = &mut svg.nodes[node].kind {
        *use_ref = reference;
    }
    match reference {
        None => {
            if id.is_empty() || !svg.ignored_ids.contains(&id) {
                let element = svg.nodes[node].element;
                svg.log_error("Referenced id not found", Some(element));
            }
            true
        }
        Some(r) => !svg.pending_use_set.contains(&r),
    }
}

/// `use` の参照先を写して入れ、属性を渡し、x・y だけずらす。
fn handle_use(svg: &mut Tree, node: NodeId) -> J<()> {
    let Kind::Group {
        use_ref: Some(reference),
        ..
    } = svg.nodes[node].kind
    else {
        return Ok(());
    };
    let copied = svg.deep_copy(reference)?;
    svg.add_child(node, copied);
    for (key, value) in svg.nodes[node].vd_attributes.to_vec() {
        svg.fill_presentation_attributes(copied, &key, &value);
    }
    let e = &svg.dom.elements[svg.nodes[node].element];
    let x = parse_float_or_default(e.get_attribute("x"), 0.0);
    let y = parse_float_or_default(e.get_attribute("y"), 0.0);
    let mut t = AffineTransform::new(1.0, 0.0, 0.0, 1.0, x as f64, y as f64);
    svg.node_transform_if_needed(node, &mut t)
}

/// グラデーションの `href` を解く（参照先の属性と色の点を写す）。
fn resolve_gradient_href(svg: &mut Tree, node: NodeId) {
    let id = svg.href_id(node);
    let referenced = if id.is_empty() {
        None
    } else {
        svg.id_map.get(&id).copied()
    };
    let element = svg.nodes[node].element;
    match referenced {
        Some(r) if matches!(svg.nodes[r].kind, Kind::Gradient { .. }) => {
            svg.copy_gradient_from(node, r);
        }
        None => {
            if id.is_empty() || !svg.ignored_ids.contains(&id) {
                svg.log_error("Referenced id not found", Some(element));
            }
        }
        Some(_) => svg.log_error("Referenced element is not a gradient", Some(element)),
    }
}

// ---- SvgColor ----

/// SVG の色を VectorDrawable の "#RRGGBB"・"#AARRGGBB" に。色と読めなければ `Ok(None)`、形が違えば `Err`。
fn color_svg2vd(svg_color_value: &str) -> Result<Option<String>, ()> {
    let color = java::trim(svg_color_value);
    if color.starts_with('#') {
        let chars: Vec<char> = color.chars().collect();
        // RGBA を ARGB に
        if chars.len() == 5 {
            let s: String = chars[4..].iter().chain(&chars[1..4]).collect();
            return Ok(Some(format!("#{s}")));
        } else if chars.len() == 9 {
            let s: String = chars[7..].iter().chain(&chars[1..7]).collect();
            return Ok(Some(format!("#{s}")));
        }
        return Ok(Some(color.to_owned()));
    }
    if color == "none" {
        return Ok(Some("#00000000".into()));
    }
    if let Some(rgb) = color.strip_prefix("rgb(").and_then(|c| c.strip_suffix(')')) {
        let numbers = java::split_char(rgb, ',');
        if numbers.len() != 3 {
            return Err(());
        }
        let mut s = String::from("#");
        for n in numbers {
            s.push_str(&format!("{:02X}", color_component(java::trim(n))?));
        }
        return Ok(Some(s));
    }
    if let Some(rgba) = color
        .strip_prefix("rgba(")
        .and_then(|c| c.strip_suffix(')'))
    {
        let numbers = java::split_char(rgba, ',');
        if numbers.len() != 4 {
            return Err(());
        }
        let mut s = String::from("#");
        for i in 0..4 {
            s.push_str(&format!(
                "{:02X}",
                color_component(java::trim(numbers[(i + 3) % 4]))?
            ));
        }
        return Ok(Some(s));
    }
    Ok(color_keyword(&color.to_lowercase()).map(str::to_owned))
}

fn color_component(component: &str) -> Result<i32, ()> {
    if let Some(p) = component.strip_suffix('%') {
        let value = java::parse_float(p).map_err(|_| ())?;
        return Ok(java::round_float(value * 255.0 / 100.0).clamp(0, 255));
    }
    Ok(java::parse_int(component).map_err(|_| ())?.clamp(0, 255))
}

/// `VdUtil.parseColorValue`: "#AARRGGBB" などを数に。
fn parse_color_value(color: &str) -> J<i32> {
    if !color.starts_with('#') {
        return Err(Thrown::new(
            java::ILLEGAL_ARGUMENT,
            format!("Invalid color value {color}"),
        ));
    }
    let hex = &color[1..];
    const ALPHA: u32 = 0xFF00_0000;
    Ok(match color.chars().count() {
        7 => (java::parse_unsigned_hex(hex)? | ALPHA) as i32,
        9 => java::parse_unsigned_hex(hex)? as i32,
        4 => {
            let v = java::parse_unsigned_hex(hex)?;
            (((v >> 8 & 0xF) * 0x110000) | ((v >> 4 & 0xF) * 0x1100) | ((v & 0xF) * 0x11) | ALPHA)
                as i32
        }
        5 => {
            let v = java::parse_unsigned_hex(hex)?;
            ((v >> 12 & 0xF).wrapping_mul(0x1100_0000)
                | ((v >> 8 & 0xF) * 0x110000)
                | ((v >> 4 & 0xF) * 0x1100)
                | ((v & 0xF) * 0x11)) as i32
        }
        _ => ALPHA as i32,
    })
}

/// `VdPath.applyAlpha`。
fn apply_alpha(color: i32, alpha: f32) -> i32 {
    let alpha_bytes = (color >> 24) & 0xff;
    let color = color & 0x00FF_FFFF;
    // (int) は 0 に向けて切り、範囲の外は端に寄せる
    let a = (alpha_bytes as f32 * alpha) as i32;
    color | a.wrapping_shl(24)
}

/// SVG 1.1 の色の名前。
fn color_keyword(name: &str) -> Option<&'static str> {
    let i = COLOR_KEYWORDS
        .binary_search_by(|(k, _)| k.cmp(&name))
        .ok()?;
    Some(COLOR_KEYWORDS[i].1)
}

const COLOR_KEYWORDS: [(&str, &str); 148] = [
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
    ("cyan", "#00ffff"),
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
    ("steelblue", "#4682b4"),
    ("tan", "#d2b48c"),
    ("teal", "#008080"),
    ("thistle", "#d8bfd8"),
    ("tomato", "#ff6347"),
    ("turquoise", "#40e0d0"),
    ("violet", "#ee82ee"),
    ("wheat", "#f5deb3"),
    ("white", "#ffffff"),
    ("whitesmoke", "#f5f5f5"),
    ("yellow", "#ffff00"),
    ("yellowgreen", "#9acd32"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_keywords_are_sorted() {
        assert!(COLOR_KEYWORDS.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(color_keyword("teal"), Some("#008080"));
    }

    #[test]
    fn colors_like_svg2vector() {
        assert_eq!(color_svg2vd("#f00a"), Ok(Some("#af00".into())));
        assert_eq!(color_svg2vd("rgb(0,128,0)"), Ok(Some("#008000".into())));
        assert_eq!(color_svg2vd("rgba(255,0,0,0.5)"), Err(()));
        assert_eq!(color_svg2vd("Red"), Ok(Some("#ff0000".into())));
        assert_eq!(color_svg2vd("url(#a)"), Ok(None));
        assert_eq!(parse_color_value("#f00").unwrap() as u32, 0xFFFF0000);
    }
}
