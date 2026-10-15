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
