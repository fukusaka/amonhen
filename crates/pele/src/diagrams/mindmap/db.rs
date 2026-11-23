//! mindmap の形（pele の `src/diagrams/mindmap/db.ts`）。

use crate::diagrams::common::outline::{Decoration, OutlineDb};
use crate::errors::{ErrorCode, PeleError, Result};

#[derive(Clone, Debug)]
pub struct MindmapNode {
    pub id: usize,
    pub node_id: String,
    pub level: i64,
    pub descr: String,
    pub type_: i32,
    /// 子の `id`。
    pub children: Vec<usize>,
    pub is_root: bool,
    /// ノードの属する最初の枝の番号。Mermaid と同じく 11 で回る。
    pub section: Option<usize>,
    pub class: Option<String>,
    pub icon: Option<String>,
}

const SECTIONS: usize = 11;

#[derive(Default)]
pub struct MindmapDb {
    pub title: Option<String>,
    /// 書いた順（深さ優先）。最初のノードが根。
    pub nodes: Vec<MindmapNode>,
    base_level: i64,
    /// まだ子を取れるノードの並び。字下げは真に増える。
    open: Vec<usize>,
}

impl OutlineDb for MindmapDb {
    fn add_node(
        &mut self,
        level: usize,
        id: &str,
        descr: &str,
        type_: i32,
        _shape_data: Option<&str>,
    ) -> Result<()> {
        let is_root = self.nodes.is_empty();
        let mut level = level as i64;
        if is_root {
            self.base_level = level;
        }
        level -= self.base_level;

        // Mermaid の親は、木のどこにあっても、字下げの浅い最後のノード。
        let mut depth = self.open.len();
        while depth > 0 && self.nodes[self.open[depth - 1]].level >= level {
            depth -= 1;
        }
        let parent = depth.checked_sub(1).map(|d| self.open[d]);
        if parent.is_none() && !is_root {
            return Err(PeleError::typed(
                format!("There can be only one root. No parent could be found for (\"{descr}\")"),
                ErrorCode::Semantic,
                "mindmap",
            ));
        }
        let index = self.nodes.len();
        let mut node = MindmapNode {
            id: index,
            node_id: id.to_string(),
            level,
            descr: descr.to_string(),
            type_,
            children: Vec::new(),
            is_root,
            section: None,
            class: None,
            icon: None,
        };
        if let Some(p) = parent {
            let parent = &mut self.nodes[p];
            node.section = if parent.is_root {
                Some(parent.children.len() % SECTIONS)
            } else {
                parent.section
            };
            parent.children.push(index);
        }
        self.open.truncate(depth);
        self.open.push(index);
        self.nodes.push(node);
        Ok(())
    }

    /// Mermaid は、どのノードより前の飾りに TypeError を投げる。ここでは無視する。
    fn decorate_node(&mut self, decoration: Decoration) {
        let Some(node) = self.nodes.last_mut() else {
            return;
        };
        match decoration {
            Decoration::Icon(icon) if !icon.is_empty() => node.icon = Some(icon),
            Decoration::Class(class) if !class.is_empty() => node.class = Some(class),
            _ => {}
        }
    }
}
