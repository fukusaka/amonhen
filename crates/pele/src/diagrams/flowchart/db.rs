//! flowchart の読みの置き場（pele の `src/diagrams/flowchart/db.ts`、Mermaid の FlowDB に合わせたもの）。

use std::collections::{HashMap, HashSet};

use super::shapes::is_valid_shape;
use super::types::{
    DocItem, FlowClassDef, FlowEdge, FlowNode, FlowSubgraph, FlowText, LabelType, LinkInfo,
};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;
use crate::util::url::safe_url;
use crate::util::yaml::{Map, Value, parse_yaml};

#[derive(Clone, Debug, Default)]
pub struct FlowDbOptions {
    pub max_edges: Option<usize>,
    pub inherit_dir: bool,
    pub direction: Option<String>,
}

fn label_type(type_: Option<&str>) -> LabelType {
    match type_ {
        Some("text") => LabelType::Text,
        Some("string") => LabelType::String,
        _ => LabelType::Markdown,
    }
}

fn unquote(text: &str) -> String {
    if text.starts_with('"') && text.ends_with('"') {
        // `text.substring(1, text.length - 1)`。1 文字の `"` では引数が入れ替わり、`"` のまま。
        if text.len() == 1 {
            return text.to_string();
        }
        return text[1..text.len() - 1].to_string();
    }
    text.to_string()
}

struct LinkEnds {
    type_: String,
    stroke: String,
    length: Option<f64>,
}

fn destruct_start_link(raw: &str) -> LinkEnds {
    let mut s = js::trim(raw);
    let mut type_ = "arrow_open";
    match s.chars().next() {
        Some('<') => {
            type_ = "arrow_point";
            s = &s[1..];
        }
        Some('x') => {
            type_ = "arrow_cross";
            s = &s[1..];
        }
        Some('o') => {
            type_ = "arrow_circle";
            s = &s[1..];
        }
        _ => {}
    }
    let mut stroke = "normal";
    if s.contains('=') {
        stroke = "thick";
    }
    if s.contains('.') {
        stroke = "dotted";
    }
    LinkEnds {
        type_: type_.into(),
        stroke: stroke.into(),
        length: None,
    }
}

fn destruct_end_link(raw: &str) -> LinkEnds {
    let s = js::trim(raw);
    let last = s.chars().next_back();
    // `str.slice(0, -1)`
    let mut line: &str = match last {
        Some(c) => &s[..s.len() - c.len_utf8()],
        None => s,
    };
    let mut type_ = "arrow_open".to_string();
    match last {
        Some('x') => {
            type_ = "arrow_cross".into();
            if s.starts_with('x') {
                type_ = format!("double_{type_}");
                line = &line[1..];
            }
        }
        Some('>') => {
            type_ = "arrow_point".into();
            if s.starts_with('<') {
                type_ = format!("double_{type_}");
                line = &line[1..];
            }
        }
        Some('o') => {
            type_ = "arrow_circle".into();
            if s.starts_with('o') {
                type_ = format!("double_{type_}");
                line = &line[1..];
            }
        }
        _ => {}
    }
    let mut stroke = "normal";
    let mut length = js::len16(line) as f64 - 1.0;
    if line.starts_with('=') {
        stroke = "thick";
    }
    if line.starts_with('~') {
        stroke = "invisible";
    }
    let dots = line.matches('.').count();
    if dots > 0 {
        stroke = "dotted";
        length = dots as f64;
    }
    LinkEnds {
        type_,
        stroke: stroke.into(),
        length: Some(length),
    }
}

#[derive(Default)]
pub struct FlowDb {
    pub direction: Option<String>,
    /// ノード（入れた順）。
    pub nodes: Vec<FlowNode>,
    node_index: HashMap<String, usize>,
    pub edges: Vec<FlowEdge>,
    pub subgraphs: Vec<FlowSubgraph>,
    /// クラスの定義（入れた順）。
    pub classes: Vec<FlowClassDef>,
    class_index: HashMap<String, usize>,
    pub tooltips: HashMap<String, String>,
    pub default_edge_style: Option<Vec<String>>,
    pub default_interpolate: Option<String>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,

    subgraph_lookup: HashMap<String, usize>,
    members: HashSet<String>,
    edge_by_id: HashMap<String, usize>,
    pair_count: HashMap<String, usize>,
    sub_count: usize,
    options: FlowDbOptions,
}

/// `{...a, ...b}`
fn spread(a: Option<&Map>, b: &Map) -> Map {
    let mut out = a.cloned().unwrap_or_default();
    for (k, v) in b.iter() {
        out.set(k, v.clone());
    }
    out
}

/// `doc.x as string` を、JS の `||` が見るとおりに（偽の値は None）。
fn truthy_string(v: &Value) -> Option<String> {
    v.truthy().then(|| v.to_js_string())
}

impl FlowDb {
    pub fn new(options: FlowDbOptions) -> Self {
        FlowDb {
            options,
            ..Default::default()
        }
    }

    pub fn node(&self, id: &str) -> Option<&FlowNode> {
        self.node_index.get(id).map(|&i| &self.nodes[i])
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut FlowNode> {
        self.node_index.get(id).map(|&i| &mut self.nodes[i])
    }

    pub fn class(&self, id: &str) -> Option<&FlowClassDef> {
        self.class_index.get(id).map(|&i| &self.classes[i])
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_vertex(
        &mut self,
        id: &str,
        text_obj: Option<&FlowText>,
        type_: Option<&str>,
        style: Option<&[String]>,
        classes: Option<&[String]>,
        dir: Option<&str>,
        props: Option<(String, String)>,
        metadata: Option<&str>,
    ) -> Result<()> {
        if id.is_empty() || js::trim(id).is_empty() {
            return Ok(());
        }

        let mut doc: Option<Map> = None;
        if let Some(metadata) = metadata {
            let yaml = if metadata.contains('\n') {
                format!("{metadata}\n")
            } else {
                format!("{{\n{metadata}\n}}")
            };
            let parsed = parse_yaml(&yaml).map_err(|m| {
                PeleError::typed(
                    format!("Metadata of \"{id}\" is not valid YAML. {m}"),
                    ErrorCode::Syntax,
                    "flowchart",
                )
            })?;
            if let Value::Obj(m) = parsed {
                doc = Some(m);
            }
        }

        if let (Some(&si), Some(d)) = (self.subgraph_lookup.get(id), doc.as_ref()) {
            let sg = &mut self.subgraphs[si];
            sg.metadata = Some(spread(sg.metadata.as_ref(), d));
            return Ok(());
        }

        if let Some(&ei) = self.edge_by_id.get(id) {
            if let Some(d) = &doc {
                let edge = &mut self.edges[ei];
                if let Some(v) = d.get("animate") {
                    edge.animate = Some(v.clone());
                }
                if let Some(v) = d.get("animation") {
                    edge.animation = Some(v.clone());
                }
                if let Some(v) = d.get("curve") {
                    edge.interpolate = truthy_string(v);
                }
            }
            return Ok(());
        }

        if !self.node_index.contains_key(id) {
            self.node_index.insert(id.to_string(), self.nodes.len());
            self.nodes.push(FlowNode {
                id: id.to_string(),
                label_type: Some(LabelType::Text),
                ..Default::default()
            });
        }
        let vertex = self.node_mut(id).unwrap();

        if let Some(t) = text_obj {
            vertex.label_type = Some(t.type_);
            vertex.text = Some(unquote(js::trim(&t.text)));
        } else if vertex.text.is_none() {
            vertex.text = Some(id.to_string());
        }
        if let Some(ty) = type_ {
            vertex.type_ = Some(ty.to_string());
        }
        if let Some(st) = style {
            vertex.styles.extend(st.iter().cloned());
        }
        if let Some(cl) = classes {
            vertex.classes.extend(cl.iter().cloned());
        }
        if let Some(d) = dir {
            vertex.dir = Some(d.to_string());
        }
        match (&mut vertex.props, props) {
            (None, p) => vertex.props = Some(p.into_iter().collect()),
            (Some(list), Some((k, v))) => match list.iter_mut().find(|(kk, _)| *kk == k) {
                Some(slot) => slot.1 = v,
                None => list.push((k, v)),
            },
            _ => {}
        }

        let Some(doc) = doc else { return Ok(()) };

        if let Some(shape) = doc.get("shape").filter(|v| v.truthy()) {
            let shape = shape.to_js_string();
            if shape != shape.to_lowercase() || shape.contains('_') {
                return Err(PeleError::typed(
                    format!("No such shape: {shape}. Shape names should be lowercase."),
                    ErrorCode::Semantic,
                    "flowchart",
                ));
            }
            if !is_valid_shape(&shape) {
                return Err(PeleError::typed(
                    format!("No such shape: {shape}."),
                    ErrorCode::Semantic,
                    "flowchart",
                ));
            }
            vertex.type_ = Some(shape);
        }
        let label = doc
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let label_blank = label.as_deref().is_none_or(|l| js::trim(l).is_empty());
        if let Some(l) = doc.get("label").filter(|v| v.truthy()) {
            vertex.text = Some(l.to_js_string());
            vertex.label_type = Some(label_type(doc.get("labelType").and_then(|v| v.as_str())));
        }
        if let Some(icon) = doc.get("icon").filter(|v| v.truthy()) {
            vertex.icon = Some(icon.to_js_string());
            if label_blank && vertex.text.as_deref() == Some(id) {
                vertex.text = Some(String::new());
            }
        }
        if let Some(v) = doc.get("form").filter(|v| v.truthy()) {
            vertex.form = Some(v.to_js_string());
        }
        if let Some(v) = doc.get("pos").filter(|v| v.truthy()) {
            vertex.pos = Some(v.to_js_string());
        }
        if let Some(img) = doc.get("img").filter(|v| v.truthy()) {
            vertex.img = Some(img.to_js_string());
            if label_blank && vertex.text.as_deref() == Some(id) {
                vertex.text = Some(String::new());
            }
        }
        if let Some(v) = doc.get("constraint").filter(|v| v.truthy()) {
            vertex.constraint = Some(v.to_js_string());
        }
        if let Some(v) = doc.get("w").filter(|v| v.truthy()) {
            vertex.asset_width = Some(v.to_number());
        }
        if let Some(v) = doc.get("h").filter(|v| v.truthy()) {
            vertex.asset_height = Some(v.to_number());
        }
        Ok(())
    }

    pub fn add_single_link(
        &mut self,
        start: &str,
        end: &str,
        type_: Option<&LinkInfo>,
        id: Option<&str>,
    ) -> Result<()> {
        let mut edge = FlowEdge {
            start: start.to_string(),
            end: end.to_string(),
            type_: None,
            text: String::new(),
            label_type: LabelType::Text,
            stroke: None,
            length: None,
            style: None,
            classes: Vec::new(),
            id: None,
            is_user_defined_id: false,
            interpolate: self.default_interpolate.clone(),
            animate: None,
            animation: None,
        };
        if let Some(link_text) = type_.and_then(|t| t.text.as_ref()) {
            edge.text = unquote(js::trim(&link_text.text));
            edge.label_type = link_text.type_;
        }
        if let Some(t) = type_ {
            edge.type_ = Some(t.type_.clone());
            edge.stroke = Some(t.stroke.clone());
            edge.length = t.length.map(|l| if l > 10.0 { 10.0 } else { l });
        }
        let pair = format!("{start}\0{end}");
        let count = self.pair_count.get(&pair).copied().unwrap_or(0);
        match id.filter(|i| !i.is_empty() && !self.edge_by_id.contains_key(*i)) {
            Some(i) => {
                edge.id = Some(i.to_string());
                edge.is_user_defined_id = true;
            }
            None => {
                edge.id = Some(format!(
                    "L_{start}_{end}_{}",
                    if count == 0 { 0 } else { count + 1 }
                ))
            }
        }

        if let Some(max) = self.options.max_edges
            && self.edges.len() >= max
        {
            return Err(PeleError::typed(
                format!("Edge limit exceeded. The limit is {max}."),
                ErrorCode::Limit,
                "flowchart",
            ));
        }
        let eid = edge.id.clone().unwrap();
        self.edges.push(edge);
        self.pair_count.insert(pair, count + 1);
        self.edge_by_id.entry(eid).or_insert(self.edges.len() - 1);
        Ok(())
    }

    pub fn add_link(
        &mut self,
        starts: &[String],
        ends: &[String],
        link: Option<&LinkInfo>,
    ) -> Result<()> {
        let id = link
            .and_then(|l| l.id.as_ref())
            .map(|i| i.replacen('@', "", 1));
        let last_start = starts.last();
        let first_end = ends.first();
        for start in starts {
            for end in ends {
                let use_id = if Some(start) == last_start && Some(end) == first_end {
                    id.as_deref()
                } else {
                    None
                };
                self.add_single_link(start, end, link, use_id)?;
            }
        }
        Ok(())
    }

    fn edge_at(&mut self, pos: &str) -> Result<usize> {
        let n = js::number(pos);
        if n >= 0.0 && n.fract() == 0.0 && (n as usize) < self.edges.len() {
            return Ok(n as usize);
        }
        Err(PeleError::typed(
            format!(
                "The index {pos} for linkStyle is out of bounds. Valid indices for linkStyle are between 0 and {}. (Help: Ensure that the index is within the range of existing edges.)",
                self.edges.len() as i64 - 1
            ),
            ErrorCode::Semantic,
            "flowchart",
        ))
    }

    pub fn update_link_interpolate(
        &mut self,
        positions: &[String],
        interpolate: &str,
    ) -> Result<()> {
        for pos in positions {
            if pos == "default" {
                self.default_interpolate = Some(interpolate.to_string());
            } else {
                let i = self.edge_at(pos)?;
                self.edges[i].interpolate = Some(interpolate.to_string());
            }
        }
        Ok(())
    }

    /// TS ではどの位置も同じ配列を共有し、辺に入れるときに `fill:none` を足す。最後に共有の配列がなる形を、
    /// 割り当てた順に作る（足した後に default に入れた配列にも `fill:none` が入る）。
    pub fn update_link(&mut self, positions: &[String], style: Vec<String>) -> Result<()> {
        let mut style = style;
        // 辺に入れる前に、エラーになる位置を先に確かめる（TS も順に投げる。投げた後の状態は使わない）。
        let mut targets: Vec<Option<usize>> = Vec::new();
        for pos in positions {
            if pos == "default" {
                targets.push(None);
            } else {
                {
                    let i = self.edge_at(pos)?;
                    targets.push(Some(i))
                }
            }
        }
        if targets.iter().any(Option::is_some)
            && !style.is_empty()
            && !style.iter().any(|s| s.starts_with("fill"))
        {
            style.push("fill:none".into());
        }
        for t in targets {
            match t {
                None => self.default_edge_style = Some(style.clone()),
                Some(i) => self.edges[i].style = Some(style.clone()),
            }
        }
        Ok(())
    }

    pub fn add_class(&mut self, ids: &str, styles: &[String]) {
        let joined = styles
            .join(",")
            .replace("\\,", "§§§")
            .replace(',', ";")
            .replace("§§§", ",");
        let style: Vec<&str> = joined.split(';').collect();
        for id in ids.split(',') {
            if !self.class_index.contains_key(id) {
                self.class_index.insert(id.to_string(), self.classes.len());
                self.classes.push(FlowClassDef {
                    id: id.to_string(),
                    styles: Vec::new(),
                    text_styles: Vec::new(),
                });
            }
            let ci = self.class_index[id];
            let class_node = &mut self.classes[ci];
            for s in &style {
                if s.contains("color") {
                    class_node.text_styles.push(s.replacen("fill", "bgFill", 1));
                }
                class_node.styles.push(s.to_string());
            }
        }
    }

    pub fn set_direction(&mut self, dir: &str) {
        let mut d = js::trim(dir).to_string();
        if d.contains('<') {
            d = "RL".into();
        }
        if d.contains('^') {
            d = "BT".into();
        }
        if d.contains('>') {
            d = "LR".into();
        }
        if d.contains('v') {
            d = "TB".into();
        }
        if d == "TD" {
            d = "TB".into();
        }
        self.direction = Some(d);
    }

    pub fn set_class(&mut self, ids: &str, class_name: &str) {
        for id in ids.split(',') {
            if let Some(n) = self.node_mut(id) {
                n.classes.push(class_name.to_string());
            }
            if let Some(&ei) = self.edge_by_id.get(id) {
                self.edges[ei].classes.push(class_name.to_string());
            }
            if let Some(&si) = self.subgraph_lookup.get(id) {
                self.subgraphs[si].classes.push(class_name.to_string());
            }
        }
    }

    pub fn set_tooltip(&mut self, ids: &str, tooltip: &str) {
        for id in ids.split(',') {
            self.tooltips.insert(id.to_string(), tooltip.to_string());
        }
    }

    pub fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
        let url = js::trim(link).to_string();
        for id in ids.split(',') {
            if let Some(v) = self.node_mut(id) {
                v.link = if url.is_empty() {
                    None
                } else {
                    Some(safe_url(&url))
                };
                v.link_target = target.map(str::to_string);
            }
        }
        self.set_class(ids, "clickable");
    }

    /// 呼び出しは決して行わない。Mermaid の strict の設定と同じく、ノードに押せる印をつけるだけ。
    pub fn set_click_event(&mut self, ids: &str) {
        self.set_class(ids, "clickable");
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(
            text.trim_start_matches(crate::util::chars::is_space_char)
                .to_string(),
        );
    }

    pub fn set_acc_description(&mut self, text: &str) {
        // `text.replace(/\n\s+/g, '\n')`
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' {
                while chars
                    .peek()
                    .is_some_and(|c| crate::util::chars::is_space_char(*c))
                {
                    chars.next();
                }
            }
        }
        // `\n\s+` は \s+ が 1 つ以上要る。改行の後に空白がなければそのまま（上で何も飛ばさない）。
        self.acc_descr = Some(out);
    }

    /// `same` は、題を id と別に書かなかった（TS で `idText === titleText`）。
    pub fn add_sub_graph(
        &mut self,
        id_text: Option<&FlowText>,
        list: &[DocItem],
        title_text: Option<&FlowText>,
        same: bool,
    ) -> String {
        let mut id: Option<String> = id_text.map(|t| js::trim(&t.text).to_string());
        let mut title = title_text.map(|t| t.text.clone()).unwrap_or_default();
        if id_text.is_some() && same && title.chars().any(crate::util::chars::is_space_char) {
            id = None;
        }

        let mut seen = HashSet::new();
        let mut node_list: Vec<String> = Vec::new();
        let mut dir: Option<String> = None;
        for entry in list {
            match entry {
                DocItem::Str(s) => {
                    if !js::trim(s).is_empty() && seen.insert(s.clone()) {
                        node_list.push(s.clone());
                    }
                }
                DocItem::List(items) => {
                    for item in items {
                        if !js::trim(item).is_empty() && seen.insert(item.clone()) {
                            node_list.push(item.clone());
                        }
                    }
                }
                DocItem::Dir(v) => dir = Some(v.clone()),
            }
        }
        if dir.is_none() && self.options.inherit_dir {
            dir = self
                .direction
                .clone()
                .or_else(|| self.options.direction.clone());
        }

        let id = id.unwrap_or_else(|| format!("subGraph{}", self.sub_count));
        if title.is_empty() {
            title = String::new();
        }
        self.sub_count += 1;

        // ノードは、それを並べた最初の subgraph のもの。subgraph は自分を含まない。
        let nodes: Vec<String> = node_list
            .into_iter()
            .filter(|n| !self.members.contains(n) && *n != id)
            .collect();
        for n in &nodes {
            self.members.insert(n.clone());
        }
        let subgraph = FlowSubgraph {
            id: id.clone(),
            nodes,
            title: js::trim(&title).to_string(),
            classes: Vec::new(),
            dir,
            label_type: label_type(title_text.map(|t| t.type_.as_str())),
            metadata: None,
        };
        match self.subgraph_lookup.get(&id) {
            Some(&ei) => {
                let extra = subgraph.nodes.clone();
                self.subgraphs[ei].nodes.extend(extra);
            }
            None => {
                self.subgraph_lookup
                    .insert(id.clone(), self.subgraphs.len());
                self.subgraphs.push(subgraph);
            }
        }
        id
    }

    pub fn destruct_link(&self, end: &str, start: Option<&str>) -> LinkInfo {
        let info = destruct_end_link(end);
        let Some(start) = start.filter(|s| !s.is_empty()) else {
            return LinkInfo {
                type_: info.type_,
                stroke: info.stroke,
                length: info.length,
                text: None,
                id: None,
            };
        };
        let mut start_info = destruct_start_link(start);
        let invalid = || LinkInfo {
            type_: "INVALID".into(),
            stroke: "INVALID".into(),
            length: None,
            text: None,
            id: None,
        };
        if start_info.stroke != info.stroke {
            return invalid();
        }
        if start_info.type_ == "arrow_open" {
            start_info.type_ = info.type_;
        } else {
            if start_info.type_ != info.type_ {
                return invalid();
            }
            start_info.type_ = format!("double_{}", start_info.type_);
        }
        if start_info.type_ == "double_arrow" {
            start_info.type_ = "double_arrow_point".into();
        }
        LinkInfo {
            type_: start_info.type_,
            stroke: start_info.stroke,
            length: info.length,
            text: None,
            id: None,
        }
    }

    pub fn subgraph(&self, id: &str) -> Option<&FlowSubgraph> {
        self.subgraph_lookup.get(id).map(|&i| &self.subgraphs[i])
    }
}
