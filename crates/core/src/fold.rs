//! マージ済みブランチの折りたたみ。各起点から最初の親だけを辿った道筋（幹）を残し、merge で取り込まれた
//! 側のコミットは隠す。隠したコミットの数は、それを取り込んだ（いちばん新しい）merge に数える。開いた
//! merge は、取り込んだ側をすべて見せる。

use amonhen_model::CommitGraph;

pub struct Folded {
    pub graph: CommitGraph,
    /// 新しい行ごとの、その merge が隠しているコミットの数（隠していなければ 0）。
    pub hidden: Vec<u32>,
    /// 元は merge だった新しい行（親を落としても merge として扱う）。
    pub merges: Vec<u32>,
}

/// `roots` と、どのコミットの親でもない行（ブランチの先端）から、最初の親だけを辿った行を残す。`expanded` の
/// merge（元の行）は、取り込んだ側もすべて残す。
pub fn fold_merged(graph: &CommitGraph, roots: &[u32], expanded: &[u32]) -> Folded {
    let n = graph.len();
    let mut is_parent = vec![false; n];
    for r in 0..n {
        for &p in graph.parents(r) {
            is_parent[p as usize] = true;
        }
    }
    let mut keep = vec![false; n];
    let starts = roots
        .iter()
        .copied()
        .filter(|&r| (r as usize) < n)
        .chain((0..n as u32).filter(|&r| !is_parent[r as usize]));
    for start in starts {
        let mut r = start as usize;
        while !std::mem::replace(&mut keep[r], true) {
            match graph.parents(r).first() {
                Some(&p) => r = p as usize,
                None => break,
            }
        }
    }
    // 開いた merge: 取り込んだ側を、残っている行に着くまですべて残す。
    let mut open = vec![false; n];
    for &m in expanded {
        if (m as usize) < n && keep[m as usize] {
            open[m as usize] = true;
        }
    }
    for m in 0..n {
        if !open[m] || !keep[m] {
            continue;
        }
        let mut stack: Vec<u32> = graph.parents(m).iter().skip(1).copied().collect();
        while let Some(r) = stack.pop() {
            if !std::mem::replace(&mut keep[r as usize], true) {
                stack.extend_from_slice(graph.parents(r as usize));
            }
        }
    }
    // 隠した数: 上（新しい）から merge ごとに、まだ数えていない隠れた祖先を数える。
    let mut claimed = keep.clone();
    let mut hidden_by_row = vec![0u32; n];
    for m in 0..n {
        if !keep[m] || graph.parents(m).len() < 2 {
            continue;
        }
        let mut stack: Vec<u32> = graph.parents(m).iter().skip(1).copied().collect();
        let mut count = 0;
        while let Some(r) = stack.pop() {
            if !std::mem::replace(&mut claimed[r as usize], true) {
                count += 1;
                stack.extend_from_slice(graph.parents(r as usize));
            }
        }
        hidden_by_row[m] = count;
    }
    let kept: Vec<usize> = (0..n).filter(|&r| keep[r]).collect();
    let hidden = kept.iter().map(|&r| hidden_by_row[r]).collect();
    let merges = kept
        .iter()
        .enumerate()
        .filter(|&(_, &r)| graph.parents(r).len() > 1)
        .map(|(new, _)| new as u32)
        .collect();
    Folded {
        graph: graph.retain(&keep),
        hidden,
        merges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_model::{CommitGraphBuilder, Oid};

    fn oid(i: u8) -> Oid {
        Oid([i; 20])
    }

    /// 上から: M(merge: A, C) → A → B、C → D → B（C・D は取り込まれたブランチ）、E は B から出た別の先端。
    fn sample() -> CommitGraph {
        let mut b = CommitGraphBuilder::with_capacity(6);
        b.push(oid(1), 6, [oid(2), oid(4)]); // M
        b.push(oid(6), 5, [oid(3)]); // E
        b.push(oid(2), 4, [oid(3)]); // A
        b.push(oid(4), 3, [oid(5)]); // C
        b.push(oid(5), 2, [oid(3)]); // D
        b.push(oid(3), 1, []); // B
        b.build()
    }

    #[test]
    fn hides_merged_side_and_counts_it_on_the_merge() {
        let g = sample();
        let f = fold_merged(&g, &[], &[]);
        assert_eq!(f.graph.ids(), &[oid(1), oid(6), oid(2), oid(3)]);
        assert_eq!(f.hidden, vec![2, 0, 0, 0]);
        // merge は最初の親だけを持つ。
        assert_eq!(f.graph.parents(0), &[2]);
        assert_eq!(f.merges, vec![0]);
    }

    #[test]
    fn expanded_merge_shows_its_side() {
        let g = sample();
        let m = g.ids().iter().position(|&i| i == oid(1)).unwrap() as u32;
        let f = fold_merged(&g, &[], &[m]);
        assert_eq!(f.graph.len(), g.len());
        assert_eq!(f.hidden[0], 0);
    }
}
