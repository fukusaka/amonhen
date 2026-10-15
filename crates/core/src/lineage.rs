//! 選んだコミットの祖先の強調。祖先の点と、祖先をつなぐ線を求め、FE はそれ以外を薄く描く。
//!
//! 線は、レーンの持ち主の子から親へ引かれている。同じ親を待つほかの子（合流する線）は始まりの曲線だけを
//! 持ち、そこから先は持ち主の縦線を共有する。だから合流した子だけが祖先なら、縦線は合流したところから
//! 下だけを光らせる。線ごとに「この行から下を光らせる」行を返すのはそのため。

use std::collections::HashMap;

use amonhen_layout::Edge;
use amonhen_model::CommitGraph;

use crate::graph::GraphLayout;

/// 線を光らせないことを表す `edge_from` の値。
pub const NOT_LIT: u32 = u32::MAX;

pub struct Lineage {
    /// 行ごとのビット。立っていれば、選んだコミット（を含む）の祖先。
    pub rows: Vec<u64>,
    /// 線ごとの、光らせ始める行（`NOT_LIT` なら光らせない）。曲線は全体を光らせるか、まったく光らせない。
    pub edge_from: Vec<u32>,
}

impl Lineage {
    pub fn contains(&self, row: u32) -> bool {
        self.rows
            .get(row as usize / 64)
            .is_some_and(|w| w & (1 << (row % 64)) != 0)
    }
}

/// `row` のコミットとその祖先。行はトポロジー順（親は子より下）なので、上から 1 回なめるだけで済む。
pub fn lineage(graph: &CommitGraph, layout: &GraphLayout, row: u32) -> Lineage {
    let n = graph.len();
    let mut rows = vec![0u64; n.div_ceil(64)];
    if (row as usize) < n {
        rows[row as usize / 64] |= 1 << (row % 64);
        for r in row as usize..n {
            if rows[r / 64] & (1 << (r % 64)) == 0 {
                continue;
            }
            for &p in graph.parents(r) {
                rows[p as usize / 64] |= 1 << (p % 64);
            }
        }
    }
    let lit = |r: u32| rows[r as usize / 64] & (1 << (r % 64)) != 0;

    let edges = layout.edges();
    // 祖先の子が合流した線（親とレーン）と、そのうちいちばん上の合流の行。
    let mut joins: HashMap<(u32, u16), u32> = HashMap::new();
    for e in edges {
        if e.kind & Edge::RIDER != 0 && lit(e.child) {
            let j = joins.entry((e.parent, e.lane_end)).or_insert(e.row_end);
            *j = (*j).min(e.row_end);
        }
    }
    let edge_from = edges
        .iter()
        .map(|e| {
            if lit(e.child) {
                return e.row_start;
            }
            if e.kind & Edge::RIDER != 0 {
                return NOT_LIT;
            }
            // 持ち主の線のレーン（親に届く部分は始まりのレーン、そうでなければ終わりのレーン）。
            let lane = if e.row_end == e.parent {
                e.lane_start
            } else {
                e.lane_end
            };
            match joins.get(&(e.parent, lane)) {
                // 縦線は合流したところから。曲線は、合流がそれより上のときだけ。
                Some(&j) if e.lane_start == e.lane_end && j < e.row_end => j.max(e.row_start),
                Some(&j) if e.lane_start != e.lane_end && j <= e.row_start => e.row_start,
                _ => NOT_LIT,
            }
        })
        .collect();
    Lineage { rows, edge_from }
}

/// `from` から `to` への、`HEAD~2^2~1` の形の道のり（`~n` は 1 つ目の親を n 回、`^k` は k 番目の親）。
/// git の name-rev と同じく、1 つ目の親を辿る道を優先し、merge の 2 つ目以降の親へ移るのは重く数える。
/// `to` が `from` の祖先でなければ `None`。`from` と `to` が同じなら空文字列。
pub fn rev_path(graph: &CommitGraph, from: u32, to: u32) -> Option<String> {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};
    /// 2 つ目以降の親へ移る重み（name-rev の MERGE_TRAVERSAL_WEIGHT と同じ）。
    const MERGE_WEIGHT: u64 = 65535;
    if from == to {
        return Some(String::new());
    }
    // 親は子より下の行にあるので、`to` より下には行かない。
    let mut best: HashMap<u32, (u64, u32, u32)> = HashMap::new(); // 行 → (重さ, 前の行, 何番目の親か)
    let mut queue = BinaryHeap::new();
    best.insert(from, (0, from, 0));
    queue.push(Reverse((0u64, from)));
    while let Some(Reverse((cost, row))) = queue.pop() {
        if row == to {
            break;
        }
        if best.get(&row).is_some_and(|b| b.0 < cost) {
            continue;
        }
        for (i, &p) in graph.parents(row as usize).iter().enumerate() {
            if p > to {
                continue;
            }
            let next = cost + if i == 0 { 1 } else { MERGE_WEIGHT };
            if best.get(&p).is_none_or(|b| next < b.0) {
                best.insert(p, (next, row, i as u32 + 1));
                queue.push(Reverse((next, p)));
            }
        }
    }
    best.get(&to)?;
    // 道を逆に辿り、`~n` と `^k` を組み立てる。
    let mut steps = Vec::new();
    let mut row = to;
    while row != from {
        let (_, prev, nth) = best[&row];
        steps.push(nth);
        row = prev;
    }
    steps.reverse();
    let mut out = String::new();
    let mut run = 0;
    for nth in steps {
        if nth == 1 {
            run += 1;
            continue;
        }
        if run > 0 {
            out.push_str(&format!("~{run}"));
            run = 0;
        }
        out.push_str(&format!("^{nth}"));
    }
    if run > 0 {
        out.push_str(&format!("~{run}"));
    }
    Some(out)
}

/// `row` のコミットとその祖先（行ごとのビット）。
pub fn ancestors(graph: &CommitGraph, row: u32) -> Vec<u64> {
    let n = graph.len();
    let mut rows = vec![0u64; n.div_ceil(64)];
    if (row as usize) < n {
        rows[row as usize / 64] |= 1 << (row % 64);
        for r in row as usize..n {
            if rows[r / 64] & (1 << (r % 64)) != 0 {
                for &p in graph.parents(r) {
                    rows[p as usize / 64] |= 1 << (p % 64);
                }
            }
        }
    }
    rows
}

/// merge の `row` が取り込んだコミット（2 つ目以降の親から辿れて、1 つ目の親から辿れないもの）を、行の順に。
/// merge でなければ空。
pub fn merged_commits(graph: &CommitGraph, row: u32) -> Vec<u32> {
    let parents = graph.parents(row as usize);
    if parents.len() < 2 {
        return Vec::new();
    }
    let mainline = ancestors(graph, parents[0]);
    let on_mainline = |r: u32| mainline[r as usize / 64] & (1 << (r % 64)) != 0;
    let mut merged = vec![0u64; mainline.len()];
    let mut start = graph.len();
    for &p in &parents[1..] {
        if !on_mainline(p) {
            merged[p as usize / 64] |= 1 << (p % 64);
            start = start.min(p as usize);
        }
    }
    let mut out = Vec::new();
    for r in start..graph.len() {
        if merged[r / 64] & (1 << (r % 64)) == 0 {
            continue;
        }
        out.push(r as u32);
        for &p in graph.parents(r) {
            if !on_mainline(p) {
                merged[p as usize / 64] |= 1 << (p % 64);
            }
        }
    }
    out
}

/// `row` のコミットを含む（`row` から辿れる側の）コミットと、直接の子。子は親より上の行にあるので、
/// `row` から上へ 1 回なめるだけで済む。返すビット集合は `row` 自身を含む。
pub fn descendants(graph: &CommitGraph, row: u32) -> (Vec<u64>, Vec<u32>) {
    let n = graph.len();
    let mut rows = vec![0u64; n.div_ceil(64)];
    let mut children = Vec::new();
    if (row as usize) >= n {
        return (rows, children);
    }
    rows[row as usize / 64] |= 1 << (row % 64);
    for r in (0..row as usize).rev() {
        let parents = graph.parents(r);
        if parents.contains(&row) {
            children.push(r as u32);
        }
        if parents
            .iter()
            .any(|&p| rows[p as usize / 64] & (1 << (p % 64)) != 0)
        {
            rows[r / 64] |= 1 << (r % 64);
        }
    }
    children.reverse();
    (rows, children)
}

/// `row` から辿れる祖先のうち、`marked` が真を返すいちばん上の行（`row` 自身を含む）。
pub fn nearest_ancestor(
    graph: &CommitGraph,
    row: u32,
    marked: impl Fn(u32) -> bool,
) -> Option<u32> {
    let n = graph.len();
    let mut rows = vec![0u64; n.div_ceil(64)];
    if (row as usize) >= n {
        return None;
    }
    rows[row as usize / 64] |= 1 << (row % 64);
    for r in row as usize..n {
        if rows[r / 64] & (1 << (r % 64)) == 0 {
            continue;
        }
        if marked(r as u32) {
            return Some(r as u32);
        }
        for &p in graph.parents(r) {
            rows[p as usize / 64] |= 1 << (p % 64);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::layout;
    use amonhen_model::{CommitGraphBuilder, Oid};

    fn oid(n: u8) -> Oid {
        let mut b = [0u8; 20];
        b[0] = n;
        Oid(b)
    }

    /// `(コミット, 親)` を新しい順に。
    fn graph(commits: &[(u8, &[u8])]) -> CommitGraph {
        let mut b = CommitGraphBuilder::default();
        for (c, parents) in commits {
            b.push(oid(*c), 0, parents.iter().map(|&p| oid(p)));
        }
        b.build()
    }

    #[test]
    fn marks_ancestors_and_the_edges_between_them() {
        // 4 は 3 と 1 の merge。3 → 2 → 0、1 → 0。
        let g = graph(&[(4, &[3, 1]), (3, &[2]), (2, &[0]), (1, &[0]), (0, &[])]);
        let l = layout(&g, Some(0));
        // 行は 4, 3, 2, 1, 0 の順。コミット 3 は 1 行目。
        let side = lineage(&g, &l, 1);
        let rows: Vec<u32> = (0..5).filter(|&r| side.contains(r)).collect();
        assert_eq!(rows, [1, 2, 4], "3、2、0 が祖先");
        for (e, &from) in l.edges().iter().zip(&side.edge_from) {
            let on_path = side.contains(e.child) && side.contains(e.parent);
            assert_eq!(from != NOT_LIT, on_path, "{e:?}");
        }
        // merge を選べば、すべてが祖先。
        let all = lineage(&g, &l, 0);
        assert!(all.edge_from.iter().all(|&f| f != NOT_LIT));
    }

    #[test]
    fn lit_edges_lead_to_ancestors_on_a_synthetic_history() {
        let (g, head) = crate::graph::synthetic::graph(3000, 7);
        let l = layout(&g, head);
        for row in [0, 17, 400, 1500] {
            let picked = lineage(&g, &l, row);
            for (e, &from) in l.edges().iter().zip(&picked.edge_from) {
                if picked.contains(e.child) {
                    assert_eq!(from, e.row_start, "祖先から出る線は全体が光る");
                }
                if from != NOT_LIT {
                    assert!(picked.contains(e.parent), "光る線は祖先に届く");
                    assert!(e.row_start <= from && from < e.row_end.max(e.row_start + 1));
                }
            }
            // 祖先の点に入る線が、少なくとも 1 本は光る（選んだ点より下）。
            for r in row + 1..g.len() as u32 {
                if picked.contains(r) {
                    let reached = l
                        .edges()
                        .iter()
                        .zip(&picked.edge_from)
                        .any(|(e, &f)| e.row_end == r && e.parent == r && f != NOT_LIT);
                    assert!(reached, "行 {r} に光る線が届かない（選んだ行 {row}）");
                }
            }
        }
    }

    #[test]
    fn finds_descendants_children_and_the_nearest_marked_ancestor() {
        // 行は 4, 3, 2, 1, 0 の順。4 は 3 と 1 の merge。
        let g = graph(&[(4, &[3, 1]), (3, &[2]), (2, &[0]), (1, &[0]), (0, &[])]);
        let (rows, children) = descendants(&g, 4);
        assert_eq!(children, [2, 3], "0 の子は 2 と 1（2 行目と 3 行目）");
        let set: Vec<u32> = (0..5).filter(|&r| rows[0] & (1 << r) != 0).collect();
        assert_eq!(set, [0, 1, 2, 3, 4], "すべてが 0 を含む");
        let (rows, children) = descendants(&g, 3);
        assert_eq!(
            (rows[0], children),
            (0b1001, vec![0]),
            "1 を含むのは 1 と 4"
        );
        assert_eq!(nearest_ancestor(&g, 1, |r| r == 4 || r == 2), Some(2));
        assert_eq!(nearest_ancestor(&g, 3, |r| r == 2), None);
    }

    #[test]
    fn lists_the_commits_a_merge_brought_in() {
        // 行は 5, 4, 3, 2, 1, 0 の順。5 は 4 と 2 の merge、2 → 1 → 0、4 → 3 → 0。
        let g = graph(&[
            (5, &[4, 2]),
            (4, &[3]),
            (3, &[0]),
            (2, &[1]),
            (1, &[0]),
            (0, &[]),
        ]);
        assert_eq!(
            merged_commits(&g, 0),
            [3, 4],
            "2 と 1（3 行目と 4 行目）。0 は 1 つ目の親から辿れる"
        );
        assert!(merged_commits(&g, 1).is_empty(), "merge でない");
        let a = ancestors(&g, 1);
        assert_eq!(a[0], 0b100110, "4、3、0（1・2・5 行目）");
    }

    #[test]
    fn names_a_commit_relative_to_head_like_name_rev() {
        // 行は 5, 4, 3, 2, 1, 0 の順。5 は 4 と 2 の merge、2 → 1 → 0、4 → 3 → 0。
        let g = graph(&[
            (5, &[4, 2]),
            (4, &[3]),
            (3, &[0]),
            (2, &[1]),
            (1, &[0]),
            (0, &[]),
        ]);
        assert_eq!(rev_path(&g, 0, 0).as_deref(), Some(""));
        assert_eq!(rev_path(&g, 0, 2).as_deref(), Some("~2"), "コミット 3");
        assert_eq!(rev_path(&g, 0, 3).as_deref(), Some("^2"), "コミット 2");
        assert_eq!(rev_path(&g, 0, 4).as_deref(), Some("^2~1"), "コミット 1");
        assert_eq!(
            rev_path(&g, 0, 5).as_deref(),
            Some("~3"),
            "コミット 0 は 1 つ目の親を辿る道を選ぶ"
        );
        assert_eq!(rev_path(&g, 1, 3), None, "4 から 2 へは辿れない");
    }

    #[test]
    fn a_rider_lights_the_shared_line_from_where_it_joins() {
        // 5 と 3 が同じ親 0 を待つ。5 が持ち主で、3 の線は 5 の縦線に合流する。
        let g = graph(&[
            (6, &[5, 4]),
            (5, &[0]),
            (4, &[3]),
            (3, &[0]),
            (2, &[1]),
            (1, &[0]),
            (0, &[]),
        ]);
        let l = layout(&g, Some(0));
        let row_of = |c: u8| g.ids().iter().position(|&id| id == oid(c)).unwrap() as u32;
        let picked = lineage(&g, &l, row_of(3));
        assert!(picked.contains(row_of(0)) && !picked.contains(row_of(5)));
        // 0 に届く線のうち、3 から来るものは光り、5 から来るもの（の 3 より上）は光らない。
        for (e, &from) in l.edges().iter().zip(&picked.edge_from) {
            if e.parent != row_of(0) {
                continue;
            }
            if from != NOT_LIT {
                assert!(
                    from > row_of(3) || e.child == row_of(3),
                    "{e:?} は 3 より上で光っている"
                );
            }
        }
        let reaches_parent = l
            .edges()
            .iter()
            .zip(&picked.edge_from)
            .any(|(e, &from)| e.row_end == row_of(0) && from != NOT_LIT);
        assert!(reaches_parent, "親 0 まで光る線がつながる");
    }
}
