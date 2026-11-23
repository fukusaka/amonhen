//! ishikawa のデータベース（pele の `src/diagrams/ishikawa/db.ts`）。

use super::types::IshikawaNode;

#[derive(Default)]
pub struct IshikawaDb {
    pub title: Option<String>,
    /// ノード。あれば 0 が根。
    pub nodes: Vec<IshikawaNode>,
    /// 最後のノードの祖先の並びと、それぞれの段。
    stack: Vec<(i64, usize)>,
    base_level: Option<i64>,
}

impl IshikawaDb {
    pub fn root(&self) -> Option<usize> {
        (!self.nodes.is_empty()).then_some(0)
    }

    /// 最初の行は結果。後の行はどれも原因で、最初の原因からの字下げで入れ子になる。結果そのものは原因より深く
    /// 字下げしてもよい。
    pub fn add_node(&mut self, raw_level: i64, text: String) {
        let index = self.nodes.len();
        self.nodes.push(IshikawaNode {
            text,
            children: Vec::new(),
        });
        if index == 0 {
            self.stack = vec![(0, 0)];
            return;
        }
        let base = *self.base_level.get_or_insert(raw_level);
        let level = (raw_level - base + 1).max(1);
        while self.stack.len() > 1 && self.stack.last().unwrap().0 >= level {
            self.stack.pop();
        }
        let parent = self.stack.last().unwrap().1;
        self.nodes[parent].children.push(index);
        self.stack.push((level, index));
    }
}
