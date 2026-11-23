//! kanban の形（pele の `src/diagrams/kanban/db.ts`）。

use crate::diagrams::common::outline::{Decoration, OutlineDb};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::util::yaml::{Value, parse_yaml};

#[derive(Clone, Debug, Default)]
pub struct KanbanNode {
    pub id: String,
    pub level: usize,
    pub label: String,
    pub icon: Option<String>,
    pub assigned: Option<String>,
    pub ticket: Option<String>,
    pub priority: Option<String>,
    pub css_classes: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct KanbanSection {
    pub node: KanbanNode,
    pub items: Vec<KanbanNode>,
}

#[derive(Default)]
pub struct KanbanDb {
    pub title: Option<String>,
    pub sections: Vec<KanbanSection>,
    /// 最後のノード: 列の番号と、カードならその番号。
    last: Option<(usize, Option<usize>)>,
    section_level: usize,
    /// 最初の列より字下げの浅いノードのラベル。
    misplaced: Option<String>,
}

fn semantic(message: String) -> PeleError {
    PeleError::typed(message, ErrorCode::Semantic, "kanban")
}

impl OutlineDb for KanbanDb {
    fn add_node(
        &mut self,
        level: usize,
        id: &str,
        descr: &str,
        _type: i32,
        shape_data: Option<&str>,
    ) -> Result<()> {
        let mut node = KanbanNode {
            id: id.to_string(),
            level,
            label: descr.to_string(),
            ..Default::default()
        };

        if let Some(shape_data) = shape_data {
            let yaml = if shape_data.contains('\n') {
                format!("{shape_data}\n")
            } else {
                format!("{{\n{shape_data}\n}}")
            };
            let doc = parse_yaml(&yaml).map_err(|message| {
                PeleError::typed(
                    format!("Metadata of \"{}\" is not valid YAML. {message}", node.id),
                    ErrorCode::Syntax,
                    "kanban",
                )
            })?;
            // Mermaid は空か対応づけでないメタデータに TypeError を投げる。ここでは無視する。
            if let Value::Obj(doc) = &doc {
                let text = |key: &str| doc.get(key).filter(|v| v.truthy()).map(Value::to_js_string);
                let shape = text("shape").unwrap_or_default();
                if shape != shape.to_lowercase() || shape.contains('_') {
                    return Err(semantic(format!(
                        "No such shape: {shape}. Shape names should be lowercase."
                    )));
                }
                if let Some(label) = text("label") {
                    node.label = label;
                }
                if let Some(icon) = text("icon") {
                    node.icon = Some(icon);
                }
                if let Some(assigned) = text("assigned") {
                    node.assigned = Some(assigned);
                }
                if let Some(ticket) = text("ticket") {
                    node.ticket = Some(ticket);
                }
                if let Some(priority) = text("priority") {
                    node.priority = Some(priority);
                }
            }
        }

        // 最初の列より字下げの浅いノードはカードになり、Mermaid はその後のものを拒む。
        if let Some(label) = &self.misplaced {
            return Err(semantic(format!(
                "Items without section detected, found section (\"{label}\")"
            )));
        }
        if self.sections.is_empty() {
            self.section_level = level;
        }
        if level == self.section_level {
            self.sections.push(KanbanSection {
                node,
                items: Vec::new(),
            });
            self.last = Some((self.sections.len() - 1, None));
        } else {
            let misplaced = level < self.section_level;
            let label = node.label.clone();
            let k = self.sections.len() - 1;
            let section = &mut self.sections[k];
            section.items.push(node);
            self.last = Some((k, Some(section.items.len() - 1)));
            if misplaced {
                self.misplaced = Some(label);
            }
        }
        Ok(())
    }

    /// Mermaid は、どのノードより前の飾りに TypeError を投げる。ここでは無視する。
    fn decorate_node(&mut self, decoration: Decoration) {
        let Some((s, item)) = self.last else {
            return;
        };
        let section = &mut self.sections[s];
        let node = match item {
            Some(k) => &mut section.items[k],
            None => &mut section.node,
        };
        match decoration {
            Decoration::Icon(icon) if !icon.is_empty() => node.icon = Some(icon),
            Decoration::Class(class) if !class.is_empty() => node.css_classes = Some(class),
            _ => {}
        }
    }
}
