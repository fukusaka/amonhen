use std::collections::BinaryHeap;

use crate::{CommitGraph, Oid, OidMap};

/// 走査で読んだコミット 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawCommit {
    pub id: Oid,
    pub commit_time: i64,
    pub parents: Vec<Oid>,
}

/// 順不同のコミットを、子が親より先に来る順に並べる。並べられるもののうちでは新しいものを先にする
/// （`git log --date-order` と同じ）。`commits` にない親は持たない。
pub fn sort(commits: Vec<RawCommit>) -> CommitGraph {
    let index = index_of(&commits);
    let parents = Parents::resolve(&commits, |id| index.get(id).copied());
    let order = order(&commits, &parents);
    let mut row = vec![0u32; commits.len()];
    for (r, &i) in order.iter().enumerate() {
        row[i as usize] = r as u32;
    }
    let mut ids = Vec::with_capacity(order.len());
    let mut times = Vec::with_capacity(order.len());
    let mut offsets = Vec::with_capacity(order.len() + 1);
    let mut rows = Vec::with_capacity(parents.indices.len());
    offsets.push(0u32);
    for &i in &order {
        let c = &commits[i as usize];
        ids.push(c.id);
        times.push(c.commit_time);
        rows.extend(
            parents
                .of(i)
                .iter()
                .filter(|&&j| j != Parents::NONE)
                .map(|&j| row[j as usize]),
        );
        offsets.push(rows.len() as u32);
    }
    CommitGraph::from_parts(ids.into(), times.into(), offsets.into(), rows.into())
        .expect("並べた History は正しい")
}

fn index_of(commits: &[RawCommit]) -> OidMap<u32> {
    let mut index = OidMap::with_capacity_and_hasher(commits.len(), Default::default());
    index.extend(commits.iter().enumerate().map(|(i, c)| (c.id, i as u32)));
    index
}

/// 親を `commits` の添字にしたもの。ハッシュ表を引くのはここで 1 回だけにする
/// （100 万件を超えると表がキャッシュに収まらず、1 回ごとの読み込みが重い）。
struct Parents {
    offsets: Vec<u32>,
    /// `commits` にない親は `NONE`。
    indices: Vec<u32>,
}

impl Parents {
    const NONE: u32 = u32::MAX;

    fn resolve(commits: &[RawCommit], find: impl Fn(&Oid) -> Option<u32>) -> Self {
        let mut offsets = Vec::with_capacity(commits.len() + 1);
        let mut indices = Vec::with_capacity(commits.len() + commits.len() / 8);
        offsets.push(0);
        for c in commits {
            indices.extend(c.parents.iter().map(|p| find(p).unwrap_or(Self::NONE)));
            offsets.push(indices.len() as u32);
        }
        Self { offsets, indices }
    }

    fn of(&self, i: u32) -> &[u32] {
        &self.indices[self.offsets[i as usize] as usize..self.offsets[i as usize + 1] as usize]
    }
}

/// `commits` の添字を、子が親より先・新しい順に並べる。
fn order(commits: &[RawCommit], parents: &Parents) -> Vec<u32> {
    // 子の数。0 になったら並べられる。
    let mut pending_children = vec![0u32; commits.len()];
    for &p in &parents.indices {
        if p != Parents::NONE {
            pending_children[p as usize] += 1;
        }
    }
    let mut ready: BinaryHeap<(i64, u32)> = pending_children
        .iter()
        .enumerate()
        .filter(|&(_, &n)| n == 0)
        .map(|(i, _)| (commits[i].commit_time, i as u32))
        .collect();

    let mut order = Vec::with_capacity(commits.len());
    while let Some((_, i)) = ready.pop() {
        for &p in parents.of(i) {
            if p != Parents::NONE {
                let n = &mut pending_children[p as usize];
                *n -= 1;
                if *n == 0 {
                    ready.push((commits[p as usize].commit_time, p));
                }
            }
        }
        order.push(i);
    }
    order
}

/// 前回の History `old` に、そこから先を辿って読んだ `new` を足す。新しいコミットを先頭に並べ、
/// `old` の行はその後ろに順番を変えずに置く。`tips` から辿れなくなった `old` の行は除く。
///
/// `old_rows` は `old` のコミット ID から行への索引。
pub fn extend(
    new: Vec<RawCommit>,
    old: &CommitGraph,
    old_rows: &OidMap<u32>,
    tips: &[Oid],
) -> CommitGraph {
    let index = index_of(&new);
    let order = order(&new, &Parents::resolve(&new, |id| index.get(id).copied()));
    // Ok は new の添字、Err は old の行。
    let locate = |id: &Oid| -> Option<Result<u32, u32>> {
        index
            .get(id)
            .map(|&i| Ok(i))
            .or_else(|| old_rows.get(id).map(|&r| Err(r)))
    };

    // tips から辿れる行。
    let mut new_reachable = vec![false; new.len()];
    let mut old_reachable = vec![false; old.len()];
    let mut stack: Vec<Result<u32, u32>> = tips.iter().filter_map(locate).collect();
    while let Some(at) = stack.pop() {
        match at {
            Ok(i) if !new_reachable[i as usize] => {
                new_reachable[i as usize] = true;
                stack.extend(new[i as usize].parents.iter().filter_map(locate));
            }
            Err(r) if !old_reachable[r as usize] => {
                old_reachable[r as usize] = true;
                stack.extend(old.parents(r as usize).iter().map(|&p| Err(p)));
            }
            _ => {}
        }
    }

    // 新しい行番号。
    let kept_new: Vec<u32> = order
        .into_iter()
        .filter(|&i| new_reachable[i as usize])
        .collect();
    let mut new_row = vec![u32::MAX; new.len()];
    for (row, &i) in kept_new.iter().enumerate() {
        new_row[i as usize] = row as u32;
    }
    let mut old_row = vec![u32::MAX; old.len()];
    let mut next = kept_new.len() as u32;
    for (r, &reachable) in old_reachable.iter().enumerate() {
        if reachable {
            old_row[r] = next;
            next += 1;
        }
    }

    let rows = next as usize;
    let mut ids = Vec::with_capacity(rows);
    let mut times = Vec::with_capacity(rows);
    let mut offsets = Vec::with_capacity(rows + 1);
    let mut parents = Vec::with_capacity(rows + rows / 8);
    offsets.push(0u32);
    for &i in &kept_new {
        let c = &new[i as usize];
        ids.push(c.id);
        times.push(c.commit_time);
        parents.extend(c.parents.iter().filter_map(locate).map(|at| match at {
            Ok(j) => new_row[j as usize],
            Err(r) => old_row[r as usize],
        }));
        offsets.push(parents.len() as u32);
    }
    for r in (0..old.len()).filter(|&r| old_reachable[r]) {
        ids.push(old.ids()[r]);
        times.push(old.commit_times()[r]);
        parents.extend(old.parents(r).iter().map(|&p| old_row[p as usize]));
        offsets.push(parents.len() as u32);
    }
    CommitGraph::from_parts(ids.into(), times.into(), offsets.into(), parents.into())
        .expect("組み立てた History は正しい")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: u8, time: i64, parents: &[u8]) -> RawCommit {
        RawCommit {
            id: Oid([id; 20]),
            commit_time: time,
            parents: parents.iter().map(|&p| Oid([p; 20])).collect(),
        }
    }

    #[test]
    fn children_come_before_parents_and_newer_first() {
        // 4 は 3 と 2 の merge。2 の時刻は 3 より新しいが親子の順が優先。9 は範囲外の親。
        let g = sort(vec![
            c(1, 10, &[9]),
            c(3, 30, &[1]),
            c(2, 40, &[1]),
            c(4, 50, &[3, 2]),
        ]);
        let order: Vec<u8> = g.ids().iter().map(|id| id.0[0]).collect();
        assert_eq!(order, [4, 2, 3, 1]);
        assert_eq!(g.parents(0), &[2, 1]);
        assert_eq!(g.parents(3), &[] as &[u32]);
    }

    fn ids(g: &CommitGraph) -> Vec<u8> {
        g.ids().iter().map(|id| id.0[0]).collect()
    }

    #[test]
    fn extend_puts_new_commits_first_and_drops_unreachable_ones() {
        // 前回: 3 → 2 → 1、5（消えるブランチ）→ 1。
        let old = sort(vec![
            c(1, 10, &[]),
            c(2, 20, &[1]),
            c(3, 30, &[2]),
            c(5, 25, &[1]),
        ]);
        let rows: OidMap<u32> = old
            .ids()
            .iter()
            .enumerate()
            .map(|(r, id)| (*id, r as u32))
            .collect();
        // 今回: 7 は 6 と 2 の merge、6 は 3 の子。起点は 7 だけ（5 のブランチは消えた）。
        let g = extend(
            vec![c(6, 60, &[3]), c(7, 70, &[6, 2])],
            &old,
            &rows,
            &[Oid([7; 20])],
        );
        assert_eq!(ids(&g), [7, 6, 3, 2, 1]);
        assert_eq!(g.parents(0), &[1, 3]);
        assert_eq!(g.parents(1), &[2]);
        assert_eq!(g.parents(2), &[3]);
    }

    #[test]
    fn parents_always_come_after_children_even_with_skewed_clocks() {
        // 子の時刻が親より古い（時計がずれていた）。
        let g = sort(vec![c(1, 100, &[]), c(2, 5, &[1])]);
        let order: Vec<u8> = g.ids().iter().map(|id| id.0[0]).collect();
        assert_eq!(order, [2, 1]);
    }
}
