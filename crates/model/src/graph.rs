use amonhen_layout::Slab;

use crate::Oid;

/// コミットの親子関係。列ごとの配列（SoA）で持ち、行の並びは子が親より先に来るトポロジカル順。
///
/// 親は行番号で指す。読み込んだ範囲の外にある親（shallow clone の境界など）は持たない。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitGraph {
    ids: Slab<Oid>,
    commit_times: Slab<i64>,
    /// `parents[parent_offsets[i]..parent_offsets[i + 1]]` が行 `i` の親。
    parent_offsets: Slab<u32>,
    parents: Slab<u32>,
}

impl CommitGraph {
    /// 永続キャッシュなどから組み立てる。配列の大きさと親の行番号が正しくなければ `None`。
    pub fn from_parts(
        ids: Slab<Oid>,
        commit_times: Slab<i64>,
        parent_offsets: Slab<u32>,
        parents: Slab<u32>,
    ) -> Option<Self> {
        let n = ids.len();
        let valid = commit_times.len() == n
            && parent_offsets.len() == n + 1
            && parent_offsets.first() == Some(&0)
            && parent_offsets.windows(2).all(|w| w[0] <= w[1])
            && *parent_offsets.last()? as usize == parents.len()
            && parents.iter().all(|&p| (p as usize) < n);
        valid.then_some(Self {
            ids,
            commit_times,
            parent_offsets,
            parents,
        })
    }

    pub fn parent_offsets(&self) -> &Slab<u32> {
        &self.parent_offsets
    }

    pub fn parent_rows(&self) -> &Slab<u32> {
        &self.parents
    }

    pub fn ids_slab(&self) -> &Slab<Oid> {
        &self.ids
    }

    pub fn commit_times_slab(&self) -> &Slab<i64> {
        &self.commit_times
    }

    /// `roots` の行から辿れる行だけを、順番を変えずに残した History。
    pub fn reachable_from(&self, roots: &[u32]) -> CommitGraph {
        let mut keep = vec![false; self.len()];
        let mut stack: Vec<u32> = roots
            .iter()
            .copied()
            .filter(|&r| (r as usize) < self.len())
            .collect();
        while let Some(r) = stack.pop() {
            if !std::mem::replace(&mut keep[r as usize], true) {
                stack.extend_from_slice(self.parents(r as usize));
            }
        }
        self.retain(&keep)
    }

    /// `keep` の立っている行だけのグラフ（並びは保つ）。残らない親への線は落とす。
    pub fn retain(&self, keep: &[bool]) -> CommitGraph {
        let mut new_row = vec![u32::MAX; self.len()];
        let mut next = 0;
        for (r, &k) in keep.iter().enumerate() {
            if k {
                new_row[r] = next;
                next += 1;
            }
        }
        let mut ids = Vec::with_capacity(next as usize);
        let mut times = Vec::with_capacity(next as usize);
        let mut offsets = vec![0u32];
        let mut parents = Vec::new();
        for r in (0..self.len()).filter(|&r| keep[r]) {
            ids.push(self.ids[r]);
            times.push(self.commit_times[r]);
            parents.extend(
                self.parents(r)
                    .iter()
                    .map(|&p| new_row[p as usize])
                    .filter(|&p| p != u32::MAX),
            );
            offsets.push(parents.len() as u32);
        }
        CommitGraph {
            ids: ids.into(),
            commit_times: times.into(),
            parent_offsets: offsets.into(),
            parents: parents.into(),
        }
    }

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
        self.parent_counts
            .push((self.parent_ids.len() - before) as u32);
    }

    pub fn build(self) -> CommitGraph {
        let row_of: crate::OidMap<u32> = self
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
            ids: self.ids.into(),
            commit_times: self.commit_times.into(),
            parent_offsets: parent_offsets.into(),
            parents: parents.into(),
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
    fn keeps_only_commits_reachable_from_the_roots() {
        // 3 は 2 と 1 の merge、4 は 0 の子（別のブランチ）。
        let mut b = CommitGraphBuilder::default();
        b.push(oid(4), 40, [oid(0)]);
        b.push(oid(3), 30, [oid(2), oid(1)]);
        b.push(oid(2), 20, [oid(0)]);
        b.push(oid(1), 10, [oid(0)]);
        b.push(oid(0), 0, []);
        let g = b.build();
        let sub = g.reachable_from(&[1]);
        assert_eq!(sub.ids(), &[oid(3), oid(2), oid(1), oid(0)]);
        assert_eq!(sub.parents(0), &[1, 2]);
        assert_eq!(sub.parents(1), &[3]);
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

        let rebuilt = CommitGraph::from_parts(
            g.ids_slab().clone(),
            g.commit_times_slab().clone(),
            g.parent_offsets().clone(),
            g.parent_rows().clone(),
        );
        assert_eq!(rebuilt.as_ref(), Some(&g));
        let broken = CommitGraph::from_parts(
            g.ids_slab().clone(),
            g.commit_times_slab().clone(),
            g.parent_offsets().clone(),
            vec![9].into(),
        );
        assert!(broken.is_none());
    }
}
