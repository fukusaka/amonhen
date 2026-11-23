//! er の読みの置き場（pele の `src/diagrams/er/db.ts`）。

use std::collections::{HashMap, HashSet};

use super::types::{
    DocItem, ErAttribute, ErClassDef, ErEntity, ErRelationship, ErSubgraph, RelSpec,
};
use crate::js;

#[derive(Default)]
pub struct ErDb {
    pub direction: String,
    /// 実体（名前、入れた順）。
    pub entities: Vec<(String, ErEntity)>,
    entity_index: HashMap<String, usize>,
    pub relationships: Vec<ErRelationship>,
    pub classes: HashMap<String, ErClassDef>,
    pub subgraphs: Vec<ErSubgraph>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    subgraph_lookup: HashMap<String, usize>,
    claimed: HashSet<String>,
}

impl ErDb {
    pub fn new() -> Self {
        ErDb {
            direction: "TB".into(),
            ..Default::default()
        }
    }

    pub fn entity(&self, name: &str) -> Option<&ErEntity> {
        self.entity_index.get(name).map(|&k| &self.entities[k].1)
    }

    pub fn add_entity(&mut self, name: &str, alias: &str) -> &mut ErEntity {
        let k = match self.entity_index.get(name) {
            Some(&k) => {
                let entity = &mut self.entities[k].1;
                if entity.alias.is_empty() && !alias.is_empty() {
                    entity.alias = alias.to_string();
                }
                k
            }
            None => {
                let k = self.entities.len();
                self.entities.push((
                    name.to_string(),
                    ErEntity {
                        id: format!("entity-{name}-{k}"),
                        label: name.to_string(),
                        alias: alias.to_string(),
                        attributes: Vec::new(),
                        css_classes: "default".into(),
                        css_styles: Vec::new(),
                    },
                ));
                self.entity_index.insert(name.to_string(), k);
                k
            }
        };
        &mut self.entities[k].1
    }

    /// 文法は並びを後ろから作るので、最後に書いた属性が先に来る。
    pub fn add_attributes(&mut self, name: &str, attributes: Vec<ErAttribute>) {
        let entity = self.add_entity(name, "");
        for attribute in attributes.into_iter().rev() {
            entity.attributes.push(attribute);
        }
    }

    pub fn add_relationship(&mut self, a: &str, role: &str, b: &str, rel_spec: RelSpec) {
        let entity_a = if self.subgraph_lookup.contains_key(a) {
            a.to_string()
        } else {
            self.add_entity(a, "").id.clone()
        };
        let entity_b = if self.subgraph_lookup.contains_key(b) {
            b.to_string()
        } else {
            self.add_entity(b, "").id.clone()
        };
        self.relationships.push(ErRelationship {
            entity_a,
            role_a: role.to_string(),
            entity_b,
            rel_spec,
        });
    }

    pub fn set_direction(&mut self, dir: &str) {
        self.direction = dir.to_string();
    }

    pub fn add_css_styles(&mut self, ids: &[String], styles: &[String]) {
        for id in ids {
            if let Some(&k) = self.entity_index.get(id) {
                self.entities[k].1.css_styles.extend(styles.iter().cloned());
            }
            if let Some(&k) = self.subgraph_lookup.get(id) {
                self.subgraphs[k].css_styles.extend(styles.iter().cloned());
            }
        }
    }

    pub fn add_class(&mut self, ids: &[String], styles: &[String]) {
        for id in ids {
            let def = self
                .classes
                .entry(id.clone())
                .or_insert_with(|| ErClassDef {
                    id: id.clone(),
                    styles: Vec::new(),
                    text_styles: Vec::new(),
                });
            for style in styles {
                if style.contains("color") {
                    def.text_styles.push(style.replacen("fill", "bgFill", 1));
                }
                def.styles.push(style.clone());
            }
        }
    }

    pub fn set_class(&mut self, ids: &[String], class_names: &[String]) {
        for id in ids {
            if let Some(&k) = self.entity_index.get(id) {
                for name in class_names {
                    let entity = &mut self.entities[k].1;
                    entity.css_classes.push(' ');
                    entity.css_classes.push_str(name);
                }
            }
            if let Some(&k) = self.subgraph_lookup.get(id) {
                self.subgraphs[k]
                    .classes
                    .extend(class_names.iter().cloned());
            }
        }
    }

    pub fn add_sub_graph(&mut self, id_text: &str, list: &[DocItem], title: &str) -> String {
        let id = js::trim(id_text).to_string();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut nodes: Vec<String> = Vec::new();
        let mut dir: Option<String> = None;
        for item in list {
            let item = match item {
                DocItem::Dir(v) => {
                    dir = Some(v.clone());
                    continue;
                }
                DocItem::Text(s) => s,
            };
            let trimmed = js::trim(item);
            if trimmed.is_empty() || !seen.insert(trimmed) {
                continue;
            }
            // 実体は、それを挙げる最初のまとまりのもの。
            if !self.claimed.contains(item) {
                nodes.push(item.clone());
            }
        }
        for node in &nodes {
            self.claimed.insert(node.clone());
        }
        self.subgraph_lookup
            .insert(id.clone(), self.subgraphs.len());
        self.subgraphs.push(ErSubgraph {
            id: id.clone(),
            nodes,
            title: js::trim(title).to_string(),
            classes: Vec::new(),
            css_styles: Vec::new(),
            dir,
        });
        id
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        // `text.replace(/\n\s+/g, '\n')`
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' {
                while chars.peek().is_some_and(|&c| js::is_js_space(c)) {
                    chars.next();
                }
            }
        }
        self.acc_descr = Some(out);
    }
}
