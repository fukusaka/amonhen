//! class の読みの置き場（pele の `src/diagrams/class/db.ts`）。

use std::collections::{HashMap, HashSet};

use super::members::{ClassMember, MemberType};
use super::types::{
    ClassInterface, ClassNode, ClassNote, ClassRelation, NamespaceNode, StyleClass,
};
use crate::js;
use crate::text::entities::decode_entities;
use crate::util::url::sanitize_url;

pub const LOLLIPOP: u8 = 4;

/// Mermaid は数字で始まる id をこの接頭辞のもとで探すので、どのクラスにも当たらない。
const DOM_ID_PREFIX: &str = "classId-";

/// `(className, type)`
fn split_name(id: &str) -> (&str, &str) {
    match id.find('~') {
        Some(at) if at > 0 => {
            let rest = &id[at + 1..];
            let end = rest.find('~').unwrap_or(rest.len());
            (&id[..at], &rest[..end])
        }
        _ => (id, ""),
    }
}

pub fn class_name(id: &str) -> &str {
    split_name(id).0
}

fn lookup_id(id: &str) -> String {
    if id.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        class_name(&format!("{DOM_ID_PREFIX}{id}")).to_string()
    } else {
        class_name(id).to_string()
    }
}

#[derive(Default)]
pub struct ClassDb {
    pub direction: String,
    /// クラス（入れた順）。
    pub classes: Vec<ClassNode>,
    class_index: HashMap<String, usize>,
    pub relations: Vec<ClassRelation>,
    /// ノート（入れた順。id は `note<index>`）。
    pub notes: Vec<ClassNote>,
    /// 名前空間（入れた順）。
    pub namespaces: Vec<NamespaceNode>,
    namespace_index: HashMap<String, usize>,
    pub interfaces: Vec<ClassInterface>,
    pub style_classes: Vec<StyleClass>,
    style_index: HashMap<String, usize>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,

    namespace_stack: Vec<String>,
    /// CSS のクラスの名前から、それを持つクラス（入れた順）。
    with_css_class: HashMap<String, (Vec<usize>, HashSet<usize>)>,
}

impl ClassDb {
    pub fn new() -> Self {
        ClassDb {
            direction: "TB".into(),
            ..Default::default()
        }
    }

    pub fn class(&self, id: &str) -> Option<&ClassNode> {
        self.class_index.get(id).map(|&i| &self.classes[i])
    }

    pub fn class_at(&self, id: &str) -> Option<usize> {
        self.class_index.get(id).copied()
    }

    pub fn namespace(&self, id: &str) -> Option<&NamespaceNode> {
        self.namespace_index.get(id).map(|&i| &self.namespaces[i])
    }

    pub fn style_class(&self, id: &str) -> Option<&StyleClass> {
        self.style_index.get(id).map(|&i| &self.style_classes[i])
    }

    pub fn add_class(&mut self, id: &str) {
        let (name, type_) = split_name(id);
        if self.class_index.contains_key(name) {
            return;
        }
        let k = self.classes.len();
        self.classes.push(ClassNode {
            id: name.to_string(),
            type_: type_.to_string(),
            label: name.to_string(),
            css_classes: "default".into(),
            methods: Vec::new(),
            members: Vec::new(),
            annotations: Vec::new(),
            styles: Vec::new(),
            parent: None,
            link: None,
            link_target: None,
            have_callback: false,
            tooltip: None,
        });
        self.class_index.insert(name.to_string(), k);
        self.tag(k, "default");
    }

    fn tag(&mut self, node: usize, css_class: &str) {
        let (list, seen) = self
            .with_css_class
            .entry(css_class.to_string())
            .or_default();
        if seen.insert(node) {
            list.push(node);
        }
    }

    /// 総称の型のついた名前のクラス。なければ作る。Mermaid は、まだ宣言していないクラスへの注釈のように、ここで作る
    /// 所で TypeError を投げる。
    fn ensure(&mut self, id: &str) -> usize {
        self.add_class(id);
        self.class_index[class_name(id)]
    }

    pub fn set_class_label(&mut self, id: &str, label: &str) {
        let k = self.ensure(id);
        self.classes[k].label = label.to_string();
    }

    pub fn add_relation(&mut self, mut relation: ClassRelation) {
        let (type1, type2) = (relation.relation.type1, relation.relation.type2);
        if type1 == Some(LOLLIPOP) && type2.is_none() {
            self.add_class(&relation.id2);
            relation.id1 = self.add_interface(&relation.id1, &relation.id2);
        } else if type2 == Some(LOLLIPOP) && type1.is_none() {
            self.add_class(&relation.id1);
            relation.id2 = self.add_interface(&relation.id2, &relation.id1);
        } else {
            self.add_class(&relation.id1);
            self.add_class(&relation.id2);
        }
        relation.id1 = class_name(&relation.id1).to_string();
        relation.id2 = class_name(&relation.id2).to_string();
        relation.relation_title1 = js::trim(&relation.relation_title1).to_string();
        relation.relation_title2 = js::trim(&relation.relation_title2).to_string();
        self.relations.push(relation);
    }

    fn add_interface(&mut self, label: &str, class_id: &str) -> String {
        let id = format!("interface{}", self.interfaces.len());
        self.interfaces.push(ClassInterface {
            id: id.clone(),
            label: label.to_string(),
            class_id: class_id.to_string(),
        });
        id
    }

    pub fn add_annotation(&mut self, class: &str, annotation: &str) {
        let k = self.ensure(class);
        self.classes[k].annotations.push(annotation.to_string());
    }

    pub fn add_member(&mut self, class: &str, member: &str) {
        let k = self.ensure(class);
        let node = &mut self.classes[k];
        let text = js::trim(member);
        if text.starts_with("<<") && text.ends_with(">>") {
            node.annotations.push(text[2..text.len() - 2].to_string());
        } else if text.find(')').is_some_and(|at| at > 0) {
            let parsed = ClassMember::new(text, MemberType::Method);
            if parsed.member_type == MemberType::Method {
                node.methods.push(parsed);
            } else {
                node.members.push(parsed);
            }
        } else if !text.is_empty() {
            node.members
                .push(ClassMember::new(text, MemberType::Attribute));
        }
    }

    pub fn add_members(&mut self, class: &str, mut members: Vec<String>) {
        members.reverse();
        for member in &members {
            self.add_member(class, member);
        }
    }

    pub fn add_note(&mut self, text: &str, class: Option<&str>) -> String {
        let index = self.notes.len();
        let id = format!("note{index}");
        self.notes.push(ClassNote {
            id: id.clone(),
            class: class.map(str::to_string),
            text: text.to_string(),
            index,
            parent: None,
        });
        id
    }

    pub fn cleanup_label(&self, label: &str) -> String {
        js::trim(label.strip_prefix(':').unwrap_or(label)).to_string()
    }

    pub fn set_css_class(&mut self, ids: &str, class: &str) {
        for id in ids.split(',') {
            let Some(&k) = self.class_index.get(&lookup_id(id)) else {
                continue;
            };
            self.classes[k].css_classes.push(' ');
            self.classes[k].css_classes.push_str(class);
            for word in class.split(' ') {
                self.tag(k, word);
            }
        }
    }

    pub fn define_class(&mut self, ids: &[String], style: &[String]) {
        for id in ids {
            let k = match self.style_index.get(id) {
                Some(&k) => k,
                None => {
                    self.style_index
                        .insert(id.clone(), self.style_classes.len());
                    self.style_classes.push(StyleClass {
                        id: id.clone(),
                        styles: Vec::new(),
                        text_styles: Vec::new(),
                    });
                    self.style_classes.len() - 1
                }
            };
            for s in style {
                if s.contains("color") {
                    self.style_classes[k]
                        .text_styles
                        .push(s.replacen("fill", "bgFill", 1));
                }
                self.style_classes[k].styles.push(s.clone());
            }
            // Mermaid は `cssClasses.includes(id)` で確かめ、長い名前の一部にも当たる（`def` という classDef は
            // `default` を通してすべてのクラスに効く）。ここでは名前全体を比べる。
            let Some((tagged, _)) = self.with_css_class.get(id) else {
                continue;
            };
            let parts: Vec<&str> = style.iter().flat_map(|s| s.split(',')).collect();
            for &node in tagged {
                for part in &parts {
                    self.classes[node].styles.push(part.to_string());
                }
            }
        }
    }

    pub fn set_tooltip(&mut self, ids: &str, tooltip: Option<&str>) {
        let Some(tooltip) = tooltip else {
            return;
        };
        for id in ids.split(',') {
            if let Some(&k) = self.class_index.get(class_name(id)) {
                self.classes[k].tooltip = Some(tooltip.to_string());
            }
        }
    }

    pub fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
        // 符号は先に文字に戻し、ブラウザがたどるものを確かめる。
        let url = js::trim(&decode_entities(link)).to_string();
        for id in ids.split(',') {
            if let Some(&k) = self.class_index.get(&lookup_id(id)) {
                let node = &mut self.classes[k];
                node.link = (!url.is_empty()).then(|| sanitize_url(&url));
                node.link_target = Some(target.unwrap_or("_blank").to_string());
            }
        }
        self.set_css_class(ids, "clickable");
    }

    /// 呼び出しは決して行わない。Mermaid の strict と同じく、クラスを押せる印にするだけ。
    pub fn set_click_event(&mut self, ids: &str) {
        for id in ids.split(',') {
            if let Some(&k) = self.class_index.get(class_name(id)) {
                self.classes[k].have_callback = true;
            }
        }
        self.set_css_class(ids, "clickable");
    }

    pub fn set_direction(&mut self, dir: &str) {
        self.direction = dir.to_string();
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(collapse_indent(text));
    }

    pub fn add_namespace(&mut self, id: &str, label: Option<&str>) -> String {
        let prefix = self
            .namespace_stack
            .last()
            .cloned()
            .filter(|p| !p.is_empty());
        let qualified = match &prefix {
            Some(p) => format!("{p}.{id}"),
            None => id.to_string(),
        };
        self.namespace_stack.push(qualified.clone());
        let label = label.filter(|l| !l.is_empty());

        if let Some(&k) = self.namespace_index.get(&qualified) {
            let existing = &mut self.namespaces[k];
            existing.explicit = true;
            if let Some(l) = label {
                existing.label = l.to_string();
            }
            return qualified;
        }

        // 囲む名前空間とその祖先はもうあるので、`id` の部分だけをたどる。
        let mut parent_id = prefix.clone();
        let mut from = prefix.as_ref().map_or(0, |p| p.len() + 1);
        loop {
            let dot = qualified[from..].find('.').map(|k| k + from);
            let leaf = dot.is_none();
            let current = match dot {
                Some(d) => qualified[..d].to_string(),
                None => qualified.clone(),
            };
            match self.namespace_index.get(&current) {
                None => {
                    let part = &qualified[from..dot.unwrap_or(qualified.len())];
                    self.namespace_index
                        .insert(current.clone(), self.namespaces.len());
                    self.namespaces.push(NamespaceNode {
                        id: current.clone(),
                        label: match label {
                            Some(l) if leaf => l.to_string(),
                            _ => part.to_string(),
                        },
                        parent: parent_id.clone(),
                        explicit: leaf,
                    });
                }
                Some(&k) => {
                    if leaf {
                        self.namespaces[k].explicit = true;
                    }
                }
            }
            if let Some(p) = &parent_id
                && self.namespace_index.contains_key(p)
            {
                let k = self.namespace_index[&current];
                if self.namespaces[k].parent.is_none() {
                    self.namespaces[k].parent = parent_id.clone();
                }
            }
            if leaf {
                return qualified;
            }
            parent_id = Some(current);
            from = dot.unwrap() + 1;
        }
    }

    pub fn pop_namespace(&mut self) {
        self.namespace_stack.pop();
    }

    pub fn add_classes_to_namespace(&mut self, id: &str, classes: &[String], notes: &[String]) {
        if !self.namespace_index.contains_key(id) {
            return;
        }
        for name in classes {
            if let Some(&k) = self.class_index.get(class_name(name)) {
                self.classes[k].parent = Some(id.to_string());
            }
        }
        for name in notes {
            if let Some(note) = self.notes.iter_mut().find(|n| &n.id == name) {
                note.parent = Some(id.to_string());
            }
        }
    }

    pub fn set_css_style(&mut self, id: &str, styles: &[String]) {
        let Some(&k) = self.class_index.get(id) else {
            return;
        };
        for s in styles {
            for part in s.split(',') {
                self.classes[k].styles.push(part.to_string());
            }
        }
    }
}

/// `text.replace(/\n\s+/g, '\n')`
pub fn collapse_indent(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|&c| js::is_js_space(c)) {
                chars.next();
            }
        }
    }
    out
}
