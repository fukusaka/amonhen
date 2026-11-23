//! sankey の中身（pele の `src/diagrams/sankey/db.ts`）。

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct SankeyNode {
    pub id: String,
    pub index: usize,
}

#[derive(Clone, Debug)]
pub struct SankeyLink {
    /// 節の番号。
    pub source: usize,
    pub target: usize,
    pub value: f64,
}

#[derive(Clone, Debug, Default)]
pub struct SankeyDb {
    pub nodes: Vec<SankeyNode>,
    pub links: Vec<SankeyLink>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    lookup: HashMap<String, usize>,
}

impl SankeyDb {
    pub fn find_or_create_node(&mut self, id: &str) -> usize {
        if let Some(&i) = self.lookup.get(id) {
            return i;
        }
        let index = self.nodes.len();
        self.lookup.insert(id.to_string(), index);
        self.nodes.push(SankeyNode {
            id: id.to_string(),
            index,
        });
        index
    }

    pub fn add_link(&mut self, source: usize, target: usize, value: f64) {
        self.links.push(SankeyLink {
            source,
            target,
            value,
        });
    }
}
