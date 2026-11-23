//! agentflow の読みの置き場（pele の `src/diagrams/agentflow/db.ts`）。

use std::collections::{HashMap, HashSet};

use super::graph::AgentGraph;
use super::shapes::{ShapeSink, is_valid_shape, resolve_shape_alias};
use super::types::{
    AgentClassDef, AgentEdge, AgentNode, AgentSubgraph, Diagnostic, DocItem, FlowText, LabelType,
    LinkInfo, Loc, Metadata,
};
use crate::errors::{ErrorCode, PeleError, Result, locate};
use crate::js;
use crate::util::chars::is_space_char;
use crate::util::url::safe_url;
use crate::util::yaml::{Map, Value, parse_yaml};

#[derive(Clone, Debug, Default)]
pub struct AgentflowDbOptions {
    pub max_edges: Option<usize>,
    pub inherit_dir: bool,
    pub direction: Option<String>,
    /// 読み手の見る文字列の前の front matter の行の数。知らせる行に足す。
    pub line_offset: usize,
    /// 読み手の見る文字列。知らせの位置はここで求める（UTF-16）。
    pub source: Option<Vec<u16>>,
}

const TOOL_SHAPES: [&str; 5] = [
    "subroutine",
    "subprocess",
    "subproc",
    "framed-rectangle",
    "tool",
];

pub fn is_tool_definition(vertex: &AgentNode) -> bool {
    vertex
        .type_
        .as_deref()
        .is_some_and(|t| TOOL_SHAPES.contains(&t))
}

fn label_type(type_: Option<&Value>) -> LabelType {
    match type_.and_then(Value::as_str) {
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

/// `__proto__`・`constructor`・`prototype` の名前をどの段からも落とす。メタデータを自分のオブジェクトに混ぜる
/// 使い手が、プロトタイプを汚されないように。
fn strip_prototype_keys(value: &Value) -> Value {
    match value {
        Value::Arr(a) => Value::Arr(a.iter().map(strip_prototype_keys).collect()),
        Value::Obj(m) => {
            let mut out = Map::new();
            for (k, v) in m.iter() {
                if k == "__proto__" || k == "constructor" || k == "prototype" {
                    continue;
                }
                out.set(k, strip_prototype_keys(v));
            }
            Value::Obj(out)
        }
        v => v.clone(),
    }
}

/// メタデータの塊の中身を要素のメタデータに足す。Mermaid は読んだ YAML を何であれ広げるので、並びは位置を、
/// 文字列は文字を名前として足す。
fn merge(into: Option<Metadata>, doc: &Value) -> Metadata {
    let mut out = into.unwrap_or_default();
    match doc {
        Value::Obj(m) => {
            for (k, v) in m.iter() {
                out.set(k, v.clone());
            }
        }
        Value::Arr(a) => {
            for (i, v) in a.iter().enumerate() {
                out.set(&i.to_string(), v.clone());
            }
        }
        Value::Str(s) => {
            for (i, u) in s.encode_utf16().enumerate() {
                out.set(&i.to_string(), Value::Str(js::from16(&[u])));
            }
        }
        _ => {}
    }
    out
}

/// `/:\s*[>|][\d+-]*\s*$/`
fn block_scalar(code: &str) -> bool {
    let cs: Vec<char> = code.chars().collect();
    (0..cs.len()).any(|i| {
        if cs[i] != ':' {
            return false;
        }
        let mut k = i + 1;
        while k < cs.len() && is_space_char(cs[k]) {
            k += 1;
        }
        if k >= cs.len() || (cs[k] != '>' && cs[k] != '|') {
            return false;
        }
        k += 1;
        while k < cs.len() && (cs[k].is_ascii_digit() || cs[k] == '+' || cs[k] == '-') {
            k += 1;
        }
        cs[k..].iter().all(|&c| is_space_char(c))
    })
}

/// `code.replace(/,([\t ]*)$/, '$1')`
fn strip_trailing_comma(code: &str) -> String {
    let body = code.trim_end_matches([' ', '\t']);
    match body.strip_suffix(',') {
        Some(head) => format!("{head}{}", &code[body.len()..]),
        None => code.to_string(),
    }
}

fn indent_of(line: &str) -> usize {
    line.chars().count() - js::trim_start(line).chars().count()
}

/// 複数行のメタデータの本体で、行を終えるカンマを除く（ブロックの YAML は許さない）。引用符・フローの集まり・
/// ブロックスカラーの中のカンマは中身なので残す。
fn strip_line_trailing_commas(body: &str) -> String {
    let mut in_single = false;
    let mut in_double = false;
    let mut flow_depth: i64 = 0;
    let mut block_scalar_indent: Option<usize> = None;
    let mut out: Vec<String> = Vec::new();
    for line in body.split('\n') {
        if let Some(bi) = block_scalar_indent {
            let indent = indent_of(line);
            if js::trim(line).is_empty() || indent > bi {
                out.push(line.to_string());
                continue;
            }
            block_scalar_indent = None;
        }
        let cs: Vec<char> = line.chars().collect();
        let mut comment_start: Option<usize> = None;
        let mut i = 0;
        while i < cs.len() {
            let ch = cs[i];
            if in_double {
                if ch == '\\' {
                    i += 1;
                } else if ch == '"' {
                    in_double = false;
                }
            } else if in_single {
                if ch == '\'' {
                    in_single = false;
                }
            } else if ch == '"' {
                in_double = true;
            } else if ch == '\'' {
                in_single = true;
            } else if ch == '#' && (i == 0 || cs[i - 1] == ' ' || cs[i - 1] == '\t') {
                comment_start = Some(i);
                break;
            } else if ch == '[' || ch == '{' {
                flow_depth += 1;
            } else if ch == ']' || ch == '}' {
                flow_depth = (flow_depth - 1).max(0);
            }
            i += 1;
        }
        if in_single || in_double || flow_depth > 0 {
            out.push(line.to_string());
            continue;
        }
        let (code, comment): (String, String) = match comment_start {
            Some(c) => (cs[..c].iter().collect(), cs[c..].iter().collect()),
            None => (line.to_string(), String::new()),
        };
        if block_scalar(&code) {
            block_scalar_indent = Some(indent_of(line));
            out.push(line.to_string());
            continue;
        }
        out.push(strip_trailing_comma(&code) + &comment);
    }
    out.join("\n")
}

fn edge_semantic(type_: &str, stroke: &str) -> Option<&'static str> {
    match (type_, stroke) {
        ("arrow_point", "normal") => Some("sequence"),
        ("arrow_cross", "normal") => Some("failure"),
        ("arrow_open", "dotted") => Some("reference"),
        _ => None,
    }
}

fn destruct_end_link(raw: &str) -> (String, String, f64) {
    let s = js::trim(raw);
    let last = s.chars().next_back();
    // `str.slice(0, -1)`
    let mut line: &str = match last {
        Some(c) => &s[..s.len() - c.len_utf8()],
        None => s,
    };
    let mut type_ = "arrow_open";
    match last {
        Some('x') => type_ = "arrow_cross",
        Some('>') => type_ = "arrow_point",
        Some('-' | '.') => line = s,
        _ => {}
    }
    let mut stroke = "normal";
    let mut length = js::len16(line) as f64 - 1.0;
    let dots = line.matches('.').count();
    if dots > 0 {
        stroke = "dotted";
        length = dots as f64;
    }
    (type_.into(), stroke.into(), length)
}

/// `JSON.stringify`。
fn json(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => {
            if n.is_finite() {
                js::fmt(*n)
            } else {
                "null".into()
            }
        }
        Value::Str(s) => {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\u{8}' => out.push_str("\\b"),
                    '\u{c}' => out.push_str("\\f"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        Value::Arr(a) => format!("[{}]", a.iter().map(json).collect::<Vec<_>>().join(",")),
        Value::Obj(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}:{}", json(&Value::Str(k.clone())), json(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

#[derive(Default)]
pub struct AgentflowDb {
    pub direction: Option<String>,
    /// ノード（入れた順）。つなぎ手もここにある（TS では同じオブジェクト）。
    pub nodes: Vec<AgentNode>,
    node_index: HashMap<String, usize>,
    pub edges: Vec<AgentEdge>,
    pub subgraphs: Vec<AgentSubgraph>,
    /// つなぎ手の名前。
    pub connectors: HashSet<String>,
    pub classes: HashMap<String, AgentClassDef>,
    pub tooltips: HashMap<String, String>,
    pub default_edge_style: Option<Vec<String>>,
    pub default_interpolate: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub line_offset: usize,
    /// 描くグラフ（読んだときに一度だけ作る）。
    pub graph: Option<AgentGraph>,

    subgraph_lookup: HashMap<String, usize>,
    global_nodes: HashSet<String>,
    members: HashSet<String>,
    edge_by_id: HashMap<String, usize>,
    pair_count: HashMap<String, usize>,
    sub_count: usize,
    reported: HashMap<String, Vec<Diagnostic>>,
    options: AgentflowDbOptions,
}

impl AgentflowDb {
    pub fn new(options: AgentflowDbOptions) -> Self {
        AgentflowDb {
            line_offset: options.line_offset,
            options,
            ..Default::default()
        }
    }

    pub fn node(&self, id: &str) -> Option<&AgentNode> {
        self.node_index.get(id).map(|&i| &self.nodes[i])
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut AgentNode> {
        self.node_index.get(id).map(|&i| &mut self.nodes[i])
    }

    fn metadata(&self, id: &str, source: &str, loc: Option<Loc>) -> Result<Value> {
        let multiline = source.contains('\n');
        let yaml = if multiline {
            format!("{source}\n")
        } else {
            format!("{{\n{source}\n}}")
        };
        let parsed = match parse_yaml(&yaml) {
            Ok(v) => v,
            Err(error) => {
                // ブロックの YAML は 1 行の書き方のカンマを許さないので、カンマを除いてもう一度だけ読む。
                let stripped = if multiline {
                    strip_line_trailing_commas(source)
                } else {
                    source.to_string()
                };
                let retried = if stripped != source {
                    parse_yaml(&format!("{stripped}\n")).ok()
                } else {
                    None
                };
                match retried {
                    Some(v) => v,
                    None => {
                        let (line, column) = self.error_at(loc);
                        let at = if loc.is_some() {
                            format!(" ({line}:{column})")
                        } else {
                            String::new()
                        };
                        let mut e = PeleError::typed(
                            format!("Metadata of \"{id}\" is not valid YAML. {error}{at}"),
                            ErrorCode::Syntax,
                            "agentflow",
                        );
                        e.line = line;
                        e.column = column;
                        return Err(e);
                    }
                }
            }
        };
        Ok(match parsed {
            Value::Null => Value::Obj(Map::new()),
            v => strip_prototype_keys(&v),
        })
    }

    fn error_at(&self, loc: Option<Loc>) -> (usize, usize) {
        let Some(loc) = loc else {
            return (0, 0);
        };
        let Some(source) = &self.options.source else {
            return (loc.first_line + self.line_offset, loc.first_column + 1);
        };
        let mut start = 0;
        for _ in 1..loc.first_line {
            start = source
                .get(start..)
                .and_then(|s| s.iter().position(|&c| c == 10))
                .map_or(0, |k| start + k + 1);
        }
        let l = locate(source, start + loc.first_column);
        (l.line, l.column)
    }

    fn shape_error(&self, message: String, loc: Option<Loc>) -> PeleError {
        let (line, column) = self.error_at(loc);
        let mut e = PeleError::typed(message, ErrorCode::Semantic, "agentflow");
        e.line = line;
        e.column = column;
        e
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
        metadata_loc: Option<Loc>,
    ) -> Result<()> {
        if id.is_empty() || js::trim(id).is_empty() {
            return Ok(());
        }

        let mut doc: Option<Value> = None;
        let mut authored_shape: Option<String> = None;
        if let Some(metadata) = metadata {
            let mut d = self.metadata(id, metadata, metadata_loc)?;
            if let Value::Obj(m) = &mut d
                && let Some(Value::Str(shape)) = m.get("shape").cloned()
            {
                let resolved = resolve_shape_alias(Some(&shape)).unwrap_or_default();
                authored_shape = Some(shape);
                m.set("shape", Value::Str(resolved));
            }
            doc = Some(d);
        }
        // `doc` の JS の真偽。
        let doc_truthy = doc.as_ref().is_some_and(Value::truthy);

        if let Some(&si) = self.subgraph_lookup.get(id)
            && doc_truthy
        {
            let sg = &mut self.subgraphs[si];
            sg.metadata = Some(merge(sg.metadata.take(), doc.as_ref().unwrap()));
            return Ok(());
        }

        if self.connectors.contains(id) && doc_truthy {
            let node = self.node_mut(id).unwrap();
            node.metadata = Some(merge(node.metadata.take(), doc.as_ref().unwrap()));
            return Ok(());
        }

        // Mermaid はこの名前を取っておき、ノードを作らない。
        if id == "connectors" {
            return Ok(());
        }

        let fields = doc.as_ref().and_then(Value::as_obj);
        if let Some(&ei) = self.edge_by_id.get(id) {
            let edge = &mut self.edges[ei];
            if let Some(f) = fields {
                if let Some(v) = f.get("animate") {
                    edge.animate = Some(v.clone());
                }
                if let Some(v) = f.get("animation") {
                    edge.animation = Some(v.clone());
                }
                if let Some(v) = f.get("curve") {
                    edge.interpolate = Some(v.clone());
                }
            }
            if doc_truthy {
                edge.metadata = Some(merge(edge.metadata.take(), doc.as_ref().unwrap()));
            }
            return Ok(());
        }

        if !self.node_index.contains_key(id) {
            self.node_index.insert(id.to_string(), self.nodes.len());
            self.nodes.push(AgentNode {
                id: id.to_string(),
                text: None,
                label_type: LabelType::Text,
                type_: None,
                styles: Vec::new(),
                classes: Vec::new(),
                dir: None,
                props: None,
                link: None,
                link_target: None,
                metadata: None,
                is_connector: false,
            });
        }
        let vertex = self.node_mut(id).unwrap();

        if let Some(t) = text_obj {
            vertex.label_type = t.type_;
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

        if !doc_truthy {
            return Ok(());
        }
        let doc = doc.unwrap();
        vertex.metadata = Some(merge(vertex.metadata.take(), &doc));
        let Value::Obj(fields) = doc else {
            return Ok(());
        };

        if let Some(shape) = fields.get("shape").filter(|v| v.truthy()) {
            let authored = match &authored_shape {
                Some(a) => a.clone(),
                None => {
                    let message = format!("No such shape: {}.", json(shape));
                    return Err(self.shape_error(message, metadata_loc));
                }
            };
            if authored != authored.to_lowercase() || authored.contains('_') {
                let message =
                    format!("No such shape: {authored}. Shape names should be lowercase.");
                return Err(self.shape_error(message, metadata_loc));
            }
            let shape = shape.as_str().unwrap_or_default().to_string();
            if !is_valid_shape(&shape) {
                let message = format!("No such shape: {shape}.");
                return Err(self.shape_error(message, metadata_loc));
            }
            self.node_mut(id).unwrap().type_ = Some(shape);
        }
        if let Some(label) = fields.get("label").filter(|v| v.truthy()) {
            let vertex = self.node_mut(id).unwrap();
            vertex.text = Some(label.to_js_string());
            vertex.label_type = label_type(fields.get("labelType"));
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
        let mut edge = AgentEdge {
            start: start.to_string(),
            end: end.to_string(),
            type_: None,
            text: String::new(),
            label_type: LabelType::Text,
            stroke: None,
            edge_semantic: None,
            length: None,
            style: None,
            classes: Vec::new(),
            id: None,
            is_user_defined_id: false,
            interpolate: self.default_interpolate.clone().map(Value::Str),
            animate: None,
            animation: None,
            metadata: None,
        };
        if let Some(link_text) = type_.and_then(|t| t.text.as_ref()) {
            edge.text = unquote(js::trim(&link_text.text));
            edge.label_type = link_text.type_;
        }
        if let Some(t) = type_ {
            edge.type_ = Some(t.type_.clone());
            edge.stroke = Some(t.stroke.clone());
            edge.length = t.length.map(|l| if l > 10.0 { 10.0 } else { l });
            if t.edge_semantic.is_some() {
                edge.edge_semantic = t.edge_semantic;
            }
        }
        let pair = format!("{start}\0{end}");
        let existing = self.pair_count.get(&pair).copied().unwrap_or(0);
        match id.filter(|i| !i.is_empty() && !self.edge_by_id.contains_key(*i)) {
            Some(i) => {
                edge.id = Some(i.to_string());
                edge.is_user_defined_id = true;
            }
            None => {
                edge.id = Some(format!(
                    "L_{start}_{end}_{}",
                    if existing == 0 { 0 } else { existing + 1 }
                ))
            }
        }

        if let Some(max) = self.options.max_edges
            && self.edges.len() >= max
        {
            return Err(PeleError::typed(
                format!("Edge limit exceeded. The limit is {max}."),
                ErrorCode::Limit,
                "agentflow",
            ));
        }
        let eid = edge.id.clone().unwrap();
        self.edges.push(edge);
        self.pair_count.insert(pair, existing + 1);
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

    /// Mermaid は数の位置だけ範囲を確かめ、自分の読み手の渡す文字列では落ちる。
    fn edge_at(&self, pos: &str) -> Result<usize> {
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
            "agentflow",
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
                self.edges[i].interpolate = Some(Value::Str(interpolate.to_string()));
            }
        }
        Ok(())
    }

    /// TS ではどの位置も同じ配列を共有し、辺に入れるときに `fill:none` を足す。最後に共有の配列がなる形を
    /// どこにも入れる（足した後に default に入れた配列にも `fill:none` が入る）。
    pub fn update_link(&mut self, positions: &[String], style: Vec<String>) -> Result<()> {
        let mut style = style;
        let mut targets: Vec<Option<usize>> = Vec::new();
        for pos in positions {
            if pos == "default" {
                targets.push(None);
            } else {
                targets.push(Some(self.edge_at(pos)?));
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
            let class_node = self
                .classes
                .entry(id.to_string())
                .or_insert_with(|| AgentClassDef {
                    id: id.to_string(),
                    styles: Vec::new(),
                    text_styles: Vec::new(),
                });
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
        self.acc_title = Some(text.trim_start_matches(is_space_char).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        // `text.replace(/\n\s+/g, '\n')`
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' {
                while chars.peek().is_some_and(|c| is_space_char(*c)) {
                    chars.next();
                }
            }
        }
        self.acc_descr = Some(out);
    }

    pub fn add_sub_graph(
        &mut self,
        id_text: Option<&FlowText>,
        list: &[DocItem],
        title_text: Option<&FlowText>,
    ) -> String {
        let id: Option<String> = id_text.map(|t| js::trim(&t.text).to_string());
        let title = title_text.map(|t| t.text.clone()).unwrap_or_default();

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
        self.sub_count += 1;

        // flow は自分も、どこからでも見えるノードも、ほかの flow がすでに持つノードも含まない。
        let nodes: Vec<String> = node_list
            .into_iter()
            .filter(|n| *n != id && !self.global_nodes.contains(n) && !self.members.contains(n))
            .collect();
        for n in &nodes {
            self.members.insert(n.clone());
        }

        if let Some(&ei) = self.subgraph_lookup.get(&id) {
            // 同じ名前を繰り返すと、その中身を最初の flow に足す。題と向きは後のものが勝つ。
            let existing = &mut self.subgraphs[ei];
            existing.nodes.extend(nodes);
            if !js::trim(&title).is_empty() {
                existing.title = js::trim(&title).to_string();
            }
            if let Some(d) = dir.filter(|d| !d.is_empty()) {
                existing.dir = Some(d);
            }
            return id;
        }
        let subgraph = AgentSubgraph {
            id: id.clone(),
            nodes,
            title: js::trim(&title).to_string(),
            classes: Vec::new(),
            dir,
            label_type: match title_text {
                Some(t) => t.type_,
                None => LabelType::Markdown,
            },
            metadata: None,
        };
        self.subgraph_lookup
            .insert(id.clone(), self.subgraphs.len());
        self.subgraphs.push(subgraph);
        id
    }

    /// `global` の塊の中で名指したノードは、それを挙げる flow の外に置く。
    pub fn add_global(&mut self, list: &[DocItem]) {
        let mut release = false;
        let mut add = |item: &str, this: &mut Self| {
            let id = js::trim(item);
            if id.is_empty() {
                return;
            }
            this.global_nodes.insert(id.to_string());
            if this.members.remove(id) {
                release = true;
            }
        };
        for entry in list {
            match entry {
                DocItem::Str(s) => add(s, self),
                DocItem::List(items) => {
                    for item in items {
                        add(item, self);
                    }
                }
                DocItem::Dir(_) => {}
            }
        }
        if !release {
            return;
        }
        let global = &self.global_nodes;
        for sg in &mut self.subgraphs {
            sg.nodes.retain(|n| !global.contains(n));
        }
    }

    pub fn add_connector(
        &mut self,
        text_obj: Option<&FlowText>,
        title_obj: Option<&FlowText>,
    ) -> String {
        let id = text_obj
            .map(|t| js::trim(&t.text).to_string())
            .unwrap_or_default();
        if id.is_empty() {
            return String::new();
        }
        let title = title_obj
            .map(|t| js::trim(&t.text).to_string())
            .unwrap_or_default();
        if !self.connectors.contains(&id) {
            let created = AgentNode {
                id: id.clone(),
                label_type: LabelType::Text,
                styles: Vec::new(),
                classes: Vec::new(),
                is_connector: true,
                text: Some(if title.is_empty() { id.clone() } else { title }),
                type_: None,
                dir: None,
                props: None,
                link: None,
                link_target: None,
                metadata: None,
            };
            self.connectors.insert(id.clone());
            match self.node_index.get(&id) {
                Some(&i) => self.nodes[i] = created,
                None => {
                    self.node_index.insert(id.clone(), self.nodes.len());
                    self.nodes.push(created);
                }
            }
        } else if !title.is_empty() {
            self.node_mut(&id).unwrap().text = Some(title);
        }
        id
    }

    pub fn destruct_link(&self, end: &str, start: Option<&str>) -> LinkInfo {
        let (type_, stroke, length) = destruct_end_link(end);
        if let Some(start) = start.filter(|s| !s.is_empty()) {
            // 割った矢印の前半は、線が点線かどうかだけを言う。
            let s = if start.contains('.') {
                "dotted"
            } else {
                "normal"
            };
            if s != stroke {
                return LinkInfo {
                    type_: "INVALID".into(),
                    stroke: "INVALID".into(),
                    length: None,
                    edge_semantic: None,
                    text: None,
                    id: None,
                };
            }
        }
        LinkInfo {
            edge_semantic: edge_semantic(&type_, &stroke),
            type_,
            stroke,
            length: Some(length),
            text: None,
            id: None,
        }
    }

    pub fn subgraph(&self, id: &str) -> Option<&AgentSubgraph> {
        self.subgraph_lookup.get(id).map(|&i| &self.subgraphs[i])
    }

    pub fn vertex_kind(vertex: &AgentNode) -> &'static str {
        if is_tool_definition(vertex) {
            return "tool";
        }
        match resolve_shape_alias(vertex.type_.as_deref()).as_deref() {
            Some("hexagon" | "hex") => "action",
            Some("lean-right" | "lean_right") => "input",
            Some("lin-doc" | "lined-document") => "refdoc",
            Some("diamond") => "decision",
            _ => "task",
        }
    }

    fn emit(
        &mut self,
        id: &'static str,
        severity: &'static str,
        message: String,
        node_id: Option<&str>,
    ) {
        let diagnostic = Diagnostic {
            id,
            severity,
            message,
            node_id: node_id.filter(|n| !n.is_empty()).map(str::to_string),
            edge_id: None,
        };
        // グラフは何度も作りうるが、知らせはそれぞれ一度だけ。
        let key = node_id.unwrap_or("").to_string();
        let same = self.reported.entry(key).or_default();
        if same.contains(&diagnostic) {
            return;
        }
        same.push(diagnostic.clone());
        self.diagnostics.push(diagnostic);
    }

    pub fn emit_warning(&mut self, id: &'static str, message: String, node_id: Option<&str>) {
        self.emit(id, "warning", message, node_id);
    }

    pub fn emit_error(&mut self, id: &'static str, message: String, node_id: Option<&str>) {
        self.emit(id, "error", message, node_id);
    }
}

impl ShapeSink for AgentflowDb {
    fn emit_error(&mut self, id: &'static str, message: String, node_id: &str) {
        AgentflowDb::emit_error(self, id, message, Some(node_id));
    }

    fn emit_warning(&mut self, id: &'static str, message: String, node_id: &str) {
        AgentflowDb::emit_warning(self, id, message, Some(node_id));
    }
}
