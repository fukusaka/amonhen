use crate::Oid;

/// コミットの親子関係。列ごとの配列（SoA）で持ち、行の並びは子が親より先に来るトポロジカル順。
///
/// 親は行番号で指す。読み込んだ範囲の外にある親（shallow clone の境界など）は持たない。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitGraph {
    ids: Vec<Oid>,
    commit_times: Vec<i64>,
    /// `parents[parent_offsets[i]..parent_offsets[i + 1]]` が行 `i` の親。
    parent_offsets: Vec<u32>,
    parents: Vec<u32>,
}

impl CommitGraph {
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn ids(&self) -> &[Oid] {
        &self.ids
    }

    pub fn commit_times(&self) -> &[i64] {
        &self.commit_times
    }

    pub fn parents(&self, row: usize) -> &[u32] {
        let start = self.parent_offsets[row] as usize;
        let end = self.parent_offsets[row + 1] as usize;
        &self.parents[start..end]
    }
}

/// トポロジカル順に並んだコミットから `CommitGraph` を組み立てる。
#[derive(Default)]
pub struct CommitGraphBuilder {
    ids: Vec<Oid>,
    commit_times: Vec<i64>,
    parent_ids: Vec<Oid>,
    parent_counts: Vec<u32>,
}

impl CommitGraphBuilder {
    pub fn with_capacity(commits: usize) -> Self {
        Self {
            ids: Vec::with_capacity(commits),
            commit_times: Vec::with_capacity(commits),
            parent_ids: Vec::with_capacity(commits + commits / 8),
            parent_counts: Vec::with_capacity(commits),
        }
    }

    /// 子が親より先に来る順で呼ぶこと。
    pub fn push(&mut self, id: Oid, commit_time: i64, parents: impl IntoIterator<Item = Oid>) {
        self.ids.push(id);
        self.commit_times.push(commit_time);
        let before = self.parent_ids.len();
        self.parent_ids.extend(parents);
        self.parent_counts.push((self.parent_ids.len() - before) as u32);
    }

    pub fn build(self) -> CommitGraph {
        let row_of: std::collections::HashMap<Oid, u32> = self
            .ids
            .iter()
            .enumerate()
            .map(|(row, id)| (*id, row as u32))
            .collect();

        let mut parent_offsets = Vec::with_capacity(self.ids.len() + 1);
        let mut parents = Vec::with_capacity(self.parent_ids.len());
        parent_offsets.push(0);
        let mut next = self.parent_ids.iter();
        for count in self.parent_counts {
            for parent in next.by_ref().take(count as usize) {
                if let Some(&row) = row_of.get(parent) {
                    parents.push(row);
                }
            }
            parent_offsets.push(parents.len() as u32);
        }

        CommitGraph {
            ids: self.ids,
            commit_times: self.commit_times,
            parent_offsets,
            parents,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(n: u8) -> Oid {
        Oid([n; 20])
    }

    #[test]
    fn parents_point_to_rows_and_unknown_parents_are_dropped() {
        // 3 は 2 と 1 の merge、2 と 1 は 0 の子、0 の親 9 は読み込んだ範囲の外。
        let mut b = CommitGraphBuilder::default();
        b.push(oid(3), 30, [oid(2), oid(1)]);
        b.push(oid(2), 20, [oid(0)]);
        b.push(oid(1), 10, [oid(0)]);
        b.push(oid(0), 0, [oid(9)]);
        let g = b.build();

        assert_eq!(g.len(), 4);
        assert_eq!(g.parents(0), &[1, 2]);
        assert_eq!(g.parents(1), &[3]);
        assert_eq!(g.parents(2), &[3]);
        assert_eq!(g.parents(3), &[] as &[u32]);
        assert_eq!(g.commit_times(), &[30, 20, 10, 0]);
    }
}
