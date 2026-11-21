//! マージ済みブランチの折りたたみ。たたんだ merge は最初の親だけを辿り、取り込んだ側のコミットは隠す。開いた
//! merge は、取り込んだ側も辿る（その中の merge も、それぞれ開いているかで決める）。隠したコミットの数は、それを
//! 取り込んだ（いちばん新しい）たたんだ merge に数える。

use amonhen_model::{CommitGraph, Oid, Result, Root};
use serde::{Deserialize, Serialize};

use crate::Repository;

/// History でどの merge をたたむか。リポジトリごとに `<common dir>/amonhen/fold.json` に覚え、どの FE で開いても
/// そろえる。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldState {
    /// merge を既定でたたむか（「全部たたむ」・「全部開く」）。
    #[serde(default)]
    pub folded: bool,
    /// 既定と逆にした merge の ID（16 進）。
    #[serde(default)]
    pub except: Vec<String>,
}

const FOLD_FILE: &str = "amonhen/fold.json";

impl FoldState {
    /// `except` を ID にしたもの（読めないものは飛ばす）。
    pub fn except_ids(&self) -> Vec<Oid> {
        self.except
            .iter()
            .filter_map(|h| Oid::from_hex(h))
            .collect()
    }
}

impl Repository {
    /// 覚えている折りたたみ。なければ（壊れていても）何もたたまない。
    pub fn fold_state(&self) -> Result<FoldState> {
        let data = self.backend().files().read(Root::CommonDir, FOLD_FILE)?;
        Ok(data
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default())
    }

    /// 折りたたみを覚える。何もたたまないなら、ファイルを消す。
    pub fn set_fold_state(&self, state: &FoldState) -> Result<()> {
        let files = self.backend().files();
        if *state == FoldState::default() {
            return files.remove(Root::CommonDir, FOLD_FILE);
        }
        let data = serde_json::to_vec(state).map_err(|e| e.to_string())?;
        files.write(Root::CommonDir, FOLD_FILE, &data)
    }
}

pub struct Folded {
    pub graph: CommitGraph,
    /// 新しい行ごとの、その merge が隠しているコミットの数（隠していなければ 0）。
    pub hidden: Vec<u32>,
    /// 元は merge だった新しい行（親を落としても merge として扱う）。
    pub merges: Vec<u32>,
}

/// `roots` と、どのコミットの親でもない行（ブランチの先端）から辿れる行を残す。merge は、`folded`（既定でたたむ
/// か）で、`except` の merge（元の行）はその逆で、たたむと最初の親だけを辿る。
pub fn fold_merged(graph: &CommitGraph, roots: &[u32], folded: bool, except: &[u32]) -> Folded {
    let n = graph.len();
    let mut is_parent = vec![false; n];
    for r in 0..n {
        for &p in graph.parents(r) {
            is_parent[p as usize] = true;
        }
    }
    let mut flipped = vec![false; n];
    for &m in except {
        if let Some(f) = flipped.get_mut(m as usize) {
            *f = true;
        }
    }
    let opened = |r: usize| folded == flipped[r];
    let mut keep = vec![false; n];
    let mut stack: Vec<u32> = roots
        .iter()
        .copied()
        .filter(|&r| (r as usize) < n)
        .chain((0..n as u32).filter(|&r| !is_parent[r as usize]))
        .collect();
    while let Some(r) = stack.pop() {
        let r = r as usize;
        if std::mem::replace(&mut keep[r], true) {
            continue;
        }
        let parents = graph.parents(r);
        if parents.len() > 1 && !opened(r) {
            stack.push(parents[0]);
        } else {
            stack.extend_from_slice(parents);
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
        let f = fold_merged(&g, &[], true, &[]);
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
        let f = fold_merged(&g, &[], true, &[m]);
        assert_eq!(f.graph.len(), g.len());
        assert_eq!(f.hidden[0], 0);
    }

    #[test]
    fn open_by_default_folds_only_the_listed_merge() {
        let g = sample();
        assert_eq!(fold_merged(&g, &[], false, &[]).graph.len(), g.len());
        let f = fold_merged(&g, &[], false, &[0]);
        assert_eq!(f.graph.ids(), &[oid(1), oid(6), oid(2), oid(3)]);
        assert_eq!(f.hidden, vec![2, 0, 0, 0]);
    }

    /// 上から: M(merge: A, N) → A → B、N(merge: C, E) → C → B、E → B。M を開いても N はたたんだまま。
    #[test]
    fn opening_a_merge_keeps_the_merges_inside_folded() {
        let mut b = CommitGraphBuilder::with_capacity(6);
        b.push(oid(1), 6, [oid(2), oid(7)]); // M
        b.push(oid(2), 5, [oid(3)]); // A
        b.push(oid(7), 4, [oid(4), oid(6)]); // N
        b.push(oid(4), 3, [oid(3)]); // C
        b.push(oid(6), 2, [oid(3)]); // E
        b.push(oid(3), 1, []); // B
        let g = b.build();
        let f = fold_merged(&g, &[], true, &[0]);
        assert_eq!(f.graph.ids(), &[oid(1), oid(2), oid(7), oid(4), oid(3)]);
        assert_eq!(f.hidden, vec![0, 0, 1, 0, 0]);
        // 既定で開いていて N だけたたむのと同じ。
        let open = fold_merged(&g, &[], false, &[2]);
        assert_eq!(open.graph.ids(), f.graph.ids());
    }
}
