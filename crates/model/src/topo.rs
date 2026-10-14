use std::collections::{BinaryHeap, HashMap};

use crate::{CommitGraph, CommitGraphBuilder, Oid};

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
    let index: HashMap<Oid, u32> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id, i as u32))
        .collect();
    // 子の数。0 になったら並べられる。
    let mut pending_children = vec![0u32; commits.len()];
    for c in &commits {
        for p in &c.parents {
            if let Some(&i) = index.get(p) {
                pending_children[i as usize] += 1;
            }
        }
    }
    let mut ready: BinaryHeap<(i64, u32)> = pending_children
        .iter()
        .enumerate()
        .filter(|&(_, &n)| n == 0)
        .map(|(i, _)| (commits[i].commit_time, i as u32))
        .collect();

    let mut builder = CommitGraphBuilder::with_capacity(commits.len());
    while let Some((_, i)) = ready.pop() {
        let c = &commits[i as usize];
        for p in &c.parents {
            if let Some(&pi) = index.get(p) {
                let n = &mut pending_children[pi as usize];
                *n -= 1;
                if *n == 0 {
                    ready.push((commits[pi as usize].commit_time, pi));
                }
            }
        }
        builder.push(c.id, c.commit_time, c.parents.iter().copied());
    }
    builder.build()
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

    #[test]
    fn parents_always_come_after_children_even_with_skewed_clocks() {
        // 子の時刻が親より古い（時計がずれていた）。
        let g = sort(vec![c(1, 100, &[]), c(2, 5, &[1])]);
        let order: Vec<u8> = g.ids().iter().map(|id| id.0[0]).collect();
        assert_eq!(order, [2, 1]);
    }
}
