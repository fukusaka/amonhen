//! C4 の中身を集める（pele の `src/diagrams/c4/db.ts`）。

use std::collections::HashMap;

use super::types::{C4Attr, C4Element, C4Named, C4Rel, Extra, set_extra};
use crate::js;
use crate::util::chars::is_space_char;

/// 項目を名前で引ける的（形・境界か関係）。
trait Target {
    fn is_rel(&self) -> bool;
    /// 名前つきの引数が決めてよい、文字の項目（`label` を除く）。
    fn field(&mut self, key: &str) -> Option<&mut Option<String>>;
    fn set_label(&mut self, value: String);
    fn label_attr(&mut self) -> &mut Option<C4Named>;
    fn extra(&mut self) -> &mut Option<Extra>;
    fn offset(&mut self, _key: &str) -> Option<&mut Option<f64>> {
        None
    }
}

impl Target for C4Element {
    fn is_rel(&self) -> bool {
        false
    }
    fn field(&mut self, key: &str) -> Option<&mut Option<String>> {
        Some(match key {
            "type" => &mut self.type_,
            "descr" => &mut self.descr,
            "techn" => &mut self.techn,
            "sprite" => &mut self.sprite,
            "tags" => &mut self.tags,
            "link" => &mut self.link,
            "bgColor" => &mut self.bg_color,
            "fontColor" => &mut self.font_color,
            "borderColor" => &mut self.border_color,
            "shadowing" => &mut self.shadowing,
            "shape" => &mut self.shape,
            "legendText" => &mut self.legend_text,
            "legendSprite" => &mut self.legend_sprite,
            _ => return None,
        })
    }
    fn set_label(&mut self, value: String) {
        self.label = value;
    }
    fn label_attr(&mut self) -> &mut Option<C4Named> {
        &mut self.label_attr
    }
    fn extra(&mut self) -> &mut Option<Extra> {
        &mut self.extra
    }
}

impl Target for C4Rel {
    fn is_rel(&self) -> bool {
        true
    }
    fn field(&mut self, key: &str) -> Option<&mut Option<String>> {
        Some(match key {
            "descr" => &mut self.descr,
            "techn" => &mut self.techn,
            "sprite" => &mut self.sprite,
            "tags" => &mut self.tags,
            "link" => &mut self.link,
            "textColor" => &mut self.text_color,
            "lineColor" => &mut self.line_color,
            _ => return None,
        })
    }
    fn set_label(&mut self, value: String) {
        self.label = value;
    }
    fn label_attr(&mut self) -> &mut Option<C4Named> {
        &mut self.label_attr
    }
    fn extra(&mut self) -> &mut Option<Extra> {
        &mut self.extra
    }
    fn offset(&mut self, key: &str) -> Option<&mut Option<f64>> {
        match key {
            "offsetX" => Some(&mut self.offset_x),
            "offsetY" => Some(&mut self.offset_y),
            _ => None,
        }
    }
}

/// 基数を与えない `parseInt`。
pub fn parse_int(s: &str) -> f64 {
    let t = js::trim_start(s);
    let body = t.strip_prefix(['-', '+']).unwrap_or(t);
    if body.starts_with("0x") || body.starts_with("0X") {
        js::parse_int(t, 16)
    } else {
        js::parse_int(t, 10)
    }
}

/// Mermaid は名前つきの引数を、それが名指す項目に（構造の項目でも）当てる。ここでは上の項目にだけ届き、ほかの
/// 名前は `extra` に取っておく。
fn named(target: &mut dyn Target, attr: &C4Named) {
    let (key, value) = (attr.key.as_str(), attr.value.as_str());
    if target.is_rel()
        && let Some(slot) = target.offset(key)
    {
        *slot = Some(parse_int(value));
        return;
    }
    if key == "label" {
        target.set_label(value.to_string());
    } else if let Some(slot) = target.field(key) {
        *slot = Some(value.to_string());
    } else {
        set_extra(target.extra(), key, value);
    }
}

/// 位置の引数はその場所の項目を、名前つきの引数は（どの場所でも）名指す項目を決める。
fn put(target: &mut dyn Target, field: &str, attr: Option<&C4Attr>) {
    match attr {
        None => {}
        Some(C4Attr::Str(s)) => {
            if let Some(slot) = target.field(field) {
                *slot = Some(s.clone());
            }
        }
        Some(C4Attr::Named(n)) => named(target, n),
    }
}

fn text(target: &mut dyn Target, field: &str, attr: Option<&C4Attr>) {
    match attr {
        None => {
            if let Some(slot) = target.field(field)
                && slot.is_none()
            {
                *slot = Some(String::new());
            }
        }
        Some(_) => put(target, field, attr),
    }
}

fn label(target: &mut dyn Target, attr: Option<&C4Attr>) {
    *target.label_attr() = None;
    match attr {
        Some(C4Attr::Named(n)) => {
            target.set_label(String::new());
            *target.label_attr() = Some(n.clone());
            named(target, n);
        }
        Some(C4Attr::Str(s)) => target.set_label(s.clone()),
        None => target.set_label(String::new()),
    }
}

/// 別名で引いた的。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ElementRef {
    Shape(usize),
    Boundary(usize),
}

#[derive(Debug)]
pub struct C4Db {
    /// 頭のキーワード: C4Context、C4Container、C4Component、C4Dynamic、C4Deployment。
    pub c4_type: Option<String>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub shapes: Vec<C4Element>,
    /// 最初の境界は、上の段のすべてを持つ名のないもの。
    pub boundaries: Vec<C4Element>,
    pub rels: Vec<C4Rel>,
    pub shape_in_row: f64,
    pub boundary_in_row: f64,

    shape_index: HashMap<String, usize>,
    // 上の段の境界は表に載せないので、図が `global` と名づけた境界はそれ自身の箱になる。Mermaid ではそれが上の段に
    // 代わり、何も描かれない。
    boundary_index: HashMap<String, usize>,
    rel_index: HashMap<String, HashMap<String, usize>>,
    current: usize,
    stack: Vec<usize>,
}

impl Default for C4Db {
    fn default() -> Self {
        Self::new()
    }
}

fn at(attrs: &[C4Attr], i: usize) -> Option<&C4Attr> {
    attrs.get(i)
}

impl C4Db {
    pub fn new() -> Self {
        C4Db {
            c4_type: None,
            title: None,
            acc_title: None,
            acc_descr: None,
            shapes: Vec::new(),
            boundaries: vec![C4Element {
                alias: "global".to_string(),
                label: "global".to_string(),
                type_: Some("global".to_string()),
                ..Default::default()
            }],
            rels: Vec::new(),
            shape_in_row: 4.0,
            boundary_in_row: 2.0,
            shape_index: HashMap::new(),
            boundary_index: HashMap::new(),
            rel_index: HashMap::new(),
            current: 0,
            stack: Vec::new(),
        }
    }

    pub fn set_c4_type(&mut self, type_: &str) {
        self.c4_type = Some(type_.to_string());
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
    }

    pub fn set_acc_description(&mut self, descr: &str) {
        // `descr.replace(/\n\s+/g, '\n')`
        let mut out = String::new();
        let mut chars = descr.chars().peekable();
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

    fn element(&mut self, boundary: bool, alias: &C4Attr, title: Option<&C4Attr>) -> usize {
        let (list, index) = if boundary {
            (&mut self.boundaries, &mut self.boundary_index)
        } else {
            (&mut self.shapes, &mut self.shape_index)
        };
        let found = match alias {
            C4Attr::Str(s) => index.get(s).copied(),
            C4Attr::Named(_) => None,
        };
        let i = match found {
            Some(i) => i,
            None => {
                let i = list.len();
                list.push(C4Element::default());
                match alias {
                    C4Attr::Str(s) => {
                        list[i].alias = s.clone();
                        index.insert(s.clone(), i);
                    }
                    C4Attr::Named(n) => {
                        list[i].alias_attr = Some(n.clone());
                        named(&mut list[i], n);
                    }
                }
                i
            }
        };
        label(&mut list[i], title);
        i
    }

    /// alias, label, ?descr, ?sprite, ?tags, $link
    pub fn add_person_or_system(&mut self, kind: &str, attrs: &[C4Attr]) {
        let i = self.element(false, &attrs[0], at(attrs, 1));
        text(&mut self.shapes[i], "descr", at(attrs, 2));
        self.finish_shape(i, kind, attrs, 3);
    }

    /// alias, label, ?techn, ?descr, ?sprite, ?tags, $link
    pub fn add_container(&mut self, kind: &str, attrs: &[C4Attr]) {
        let i = self.element(false, &attrs[0], at(attrs, 1));
        text(&mut self.shapes[i], "techn", at(attrs, 2));
        text(&mut self.shapes[i], "descr", at(attrs, 3));
        self.finish_shape(i, kind, attrs, 4);
    }

    pub fn add_component(&mut self, kind: &str, attrs: &[C4Attr]) {
        self.add_container(kind, attrs);
    }

    fn finish_shape(&mut self, i: usize, kind: &str, attrs: &[C4Attr], at_: usize) {
        let shape = &mut self.shapes[i];
        put(shape, "sprite", at(attrs, at_));
        put(shape, "tags", at(attrs, at_ + 1));
        put(shape, "link", at(attrs, at_ + 2));
        shape.kind = kind.to_string();
        shape.parent = Some(self.current);
    }

    fn boundary(&mut self, attrs: &[C4Attr], fallback: &str) -> usize {
        let i = self.element(true, &attrs[0], at(attrs, 1));
        match at(attrs, 2) {
            None => self.boundaries[i].type_ = Some(fallback.to_string()),
            a => put(&mut self.boundaries[i], "type", a),
        }
        i
    }

    fn open(&mut self, i: usize) {
        self.boundaries[i].parent = Some(self.current);
        self.stack.push(self.current);
        self.current = i;
    }

    /// alias, label, ?type, ?tags, $link
    pub fn add_person_or_system_boundary(&mut self, attrs: &[C4Attr]) {
        self.boundary_with(attrs, "system");
    }

    fn boundary_with(&mut self, attrs: &[C4Attr], fallback: &str) {
        let i = self.boundary(attrs, fallback);
        put(&mut self.boundaries[i], "tags", at(attrs, 3));
        put(&mut self.boundaries[i], "link", at(attrs, 4));
        self.open(i);
    }

    pub fn add_container_boundary(&mut self, attrs: &[C4Attr]) {
        self.boundary_with(attrs, "container");
    }

    /// alias, label, ?type, ?descr, ?sprite, ?tags, $link
    pub fn add_deployment_node(&mut self, node_type: &str, attrs: &[C4Attr]) {
        let i = self.boundary(attrs, "node");
        let b = &mut self.boundaries[i];
        text(b, "descr", at(attrs, 3));
        // Mermaid は sprite の場所にあるもの（名前つきの引数も）を捨てる。
        put(b, "sprite", at(attrs, 4));
        put(b, "tags", at(attrs, 5));
        put(b, "link", at(attrs, 6));
        b.node_type = Some(node_type.to_string());
        self.open(i);
    }

    pub fn pop_boundary_parse_stack(&mut self) {
        self.current = self.stack.pop().unwrap_or(0);
    }

    /// from, to, label, ?techn, ?descr, ?sprite, ?tags, $link
    pub fn add_rel(&mut self, type_: &str, attrs: &[C4Attr]) {
        let (Some(from), Some(to), Some(_)) = (at(attrs, 0), at(attrs, 1), at(attrs, 2)) else {
            return;
        };
        let known = match (from, to) {
            (C4Attr::Str(f), C4Attr::Str(t)) => Some((f.clone(), t.clone())),
            _ => None,
        };
        let found = known
            .as_ref()
            .and_then(|(f, t)| self.rel_index.get(f).and_then(|m| m.get(t)).copied());
        let i = match found {
            Some(i) => i,
            None => {
                let i = self.rels.len();
                let mut rel = C4Rel {
                    type_: type_.to_string(),
                    ..Default::default()
                };
                if let Some((f, t)) = known {
                    self.rel_index.entry(f).or_default().insert(t, i);
                }
                match from {
                    C4Attr::Str(s) => rel.from = s.clone(),
                    C4Attr::Named(n) => rel.from_attr = Some(n.clone()),
                }
                match to {
                    C4Attr::Str(s) => rel.to = s.clone(),
                    C4Attr::Named(n) => rel.to_attr = Some(n.clone()),
                }
                self.rels.push(rel);
                i
            }
        };
        let rel = &mut self.rels[i];
        rel.type_ = type_.to_string();
        label(rel, at(attrs, 2));
        text(rel, "techn", at(attrs, 3));
        text(rel, "descr", at(attrs, 4));
        put(rel, "sprite", at(attrs, 5));
        put(rel, "tags", at(attrs, 6));
        put(rel, "link", at(attrs, 7));
    }

    pub fn get_c4_shape(&self, alias: &str) -> Option<ElementRef> {
        self.shape_index
            .get(alias)
            .map(|&i| ElementRef::Shape(i))
            .or_else(|| {
                self.boundary_index
                    .get(alias)
                    .map(|&i| ElementRef::Boundary(i))
            })
    }

    pub fn element_of(&self, r: ElementRef) -> &C4Element {
        match r {
            ElementRef::Shape(i) => &self.shapes[i],
            ElementRef::Boundary(i) => &self.boundaries[i],
        }
    }

    /// elementName, ?bgColor, ?fontColor, ?borderColor, ?shadowing, ?shape, ?sprite, ?techn, ?legendText,
    /// ?legendSprite
    pub fn update_el_style(&mut self, _type: &str, attrs: &[C4Attr]) {
        let r = match at(attrs, 0) {
            Some(C4Attr::Str(s)) => self.get_c4_shape(s),
            _ => None,
        };
        let Some(r) = r else {
            return;
        };
        let el = match r {
            ElementRef::Shape(i) => &mut self.shapes[i],
            ElementRef::Boundary(i) => &mut self.boundaries[i],
        };
        for (k, field) in [
            "bgColor",
            "fontColor",
            "borderColor",
            "shadowing",
            "shape",
            "sprite",
            "techn",
            "legendText",
            "legendSprite",
        ]
        .iter()
        .enumerate()
        {
            put(el, field, at(attrs, k + 1));
        }
    }

    /// from, to, ?textColor, ?lineColor, ?offsetX, ?offsetY
    pub fn update_rel_style(&mut self, _type: &str, attrs: &[C4Attr]) {
        let found = match (at(attrs, 0), at(attrs, 1)) {
            (Some(C4Attr::Str(f)), Some(C4Attr::Str(t))) => {
                self.rel_index.get(f).and_then(|m| m.get(t)).copied()
            }
            _ => None,
        };
        let Some(i) = found else {
            return;
        };
        let rel = &mut self.rels[i];
        put(rel, "textColor", at(attrs, 2));
        put(rel, "lineColor", at(attrs, 3));
        match at(attrs, 4) {
            Some(C4Attr::Str(s)) => rel.offset_x = Some(parse_int(s)),
            Some(C4Attr::Named(n)) => named(rel, n),
            None => {}
        }
        match at(attrs, 5) {
            Some(C4Attr::Str(s)) => rel.offset_y = Some(parse_int(s)),
            Some(C4Attr::Named(n)) => named(rel, n),
            None => {}
        }
    }

    /// ?c4ShapeInRow, ?c4BoundaryInRow。Mermaid と同じく、値がどれを決めるかは名前でなく場所で決まる。
    pub fn update_layout_config(&mut self, _type: &str, attrs: &[C4Attr]) {
        let count = |attr: Option<&C4Attr>| -> f64 {
            match attr {
                None => f64::NAN,
                Some(C4Attr::Str(s)) => parse_int(s),
                Some(C4Attr::Named(n)) => parse_int(&n.value),
            }
        };
        let shapes = count(at(attrs, 0));
        let boundaries = count(at(attrs, 1));
        if shapes >= 1.0 {
            self.shape_in_row = shapes;
        }
        if boundaries >= 1.0 {
            self.boundary_in_row = boundaries;
        }
    }
}
