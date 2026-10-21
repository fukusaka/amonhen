//! ブランチの分け方（サイドバーで、関連するもの・自分の作業があるもの・古いものを分ける）。History のグラフから
//! 求めるので、git を動かさない。
//!
//! 「固有のコミット」は、既定のブランチ（`origin/HEAD` の先など）から辿れない、そのブランチだけのコミット。
//! 既定のブランチに取り込み済みなら固有のコミットはない（マージ済み）。

use amonhen_model::CommitGraph;

/// 1 本のブランチを調べた結果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchFacts {
    /// 既定のブランチに取り込み済み（先端が既定のブランチから辿れる）。
    pub merged: bool,
    /// 固有のコミットの行（新しい順、`limit` まで）。
    pub unique: Vec<u32>,
}

/// `bases`（既定のブランチの先端の行。手元のものと上流のもの）から辿れる行の印。
pub fn reachable(graph: &CommitGraph, bases: &[u32]) -> Vec<bool> {
    let mut seen = vec![false; graph.len()];
    let mut stack: Vec<u32> = bases.to_vec();
    while let Some(row) = stack.pop() {
        let seen_row = &mut seen[row as usize];
        if *seen_row {
            continue;
        }
        *seen_row = true;
        stack.extend(
            graph
                .parents(row as usize)
                .iter()
                .filter(|&&p| !seen[p as usize]),
        );
    }
    seen
}

/// `tip` の行から、`in_base` に当たるまで辿ったコミット（`limit` まで）。
pub fn facts(graph: &CommitGraph, in_base: &[bool], tip: u32, limit: usize) -> BranchFacts {
    if in_base[tip as usize] {
        return BranchFacts {
            merged: true,
            unique: Vec::new(),
        };
    }
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![tip];
    let mut unique = Vec::new();
    while let Some(row) = stack.pop() {
        if in_base[row as usize] || !seen.insert(row) {
            continue;
        }
        unique.push(row);
        if unique.len() >= limit {
            break;
        }
        stack.extend(graph.parents(row as usize));
    }
    // 行は新しい順（トポロジカル順）なので、行番号で並べれば新しい順になる。
    unique.sort_unstable();
    BranchFacts {
        merged: false,
        unique,
    }
}

/// 土台を決めるのに使う、ブランチごとの手がかり。
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    /// 先端の行。History にない（コミット以外を指す）なら `None`。
    pub tip: Option<u32>,
    /// 幹（上流が同じ名前のリモートブランチ。`main` ↔ `origin/main`）。幹は、ほかのブランチの上に積んだものと
    /// 見なさない。
    pub trunk: bool,
    /// 上流がある（長く使うブランチの印）。
    pub has_upstream: bool,
    /// 上流に当たる手元のブランチ（`wip` の上流が `origin/current` で、`current` の上流も `origin/current` なら
    /// `current`）。
    pub upstream_local: Option<usize>,
    /// 既定のブランチに取り込み済み（土台にも、積んだものにもならない）。
    pub merged: bool,
}

/// 土台を探すときに、先端から辿るコミットの数の上限。これより前で分かれたブランチは、土台とは見なさない。
pub const PARENT_WALK: usize = 5000;

/// ブランチごとの土台のブランチ（`candidates` の添字）。`in_base` は既定のブランチから辿れる行の印
/// （[`reachable`]）。次の順で決める。
///
/// 1. 上流に当たる手元のブランチ（`wip` の上流が `origin/current` なら `current`）。上流は人が決めたものなので、
///    履歴より優先する。
/// 2. なければ、先端から親を辿って最初に当たる、ほかのブランチの先端（そのブランチの上に積んである）。同じ先端が
///    複数あれば、幹、上流のあるものの順。
/// 3. 先端より先に幹の固有のコミット（既定のブランチにないもの）に当たれば、その幹（最後に分かれた幹。幹が
///    分かれたあとに進んでいても見つかる）。同じコミットを複数の幹が持っていれば、固有のコミットが少ない幹
///    （積み重ねの下のほう）。
///
/// 幹と取り込み済みのものは土台を持たない。先端が同じブランチどうしは土台にしない。幹でないブランチは、先端が
/// 履歴にあるときだけ土台になる（同じ点から分かれた兄弟どうしを、互いの土台にしない）。
///
/// 既定のブランチの履歴は、どの幹の履歴にも入っている（push したブランチはどれも幹）ので、そこは幹の印と
/// 見なさない。見なすと、既定のブランチから分かれただけのブランチが、あとから分かれた無関係な幹の下に入る。
/// 辿るのも既定のブランチの履歴に入ったところで止める（その先に未マージの先端も幹の固有のコミットもない）。
/// 幹の固有のコミットに当たったら、その先は辿らない（その先で見つかる先端は、その幹の土台であって、この
/// ブランチの土台ではない）。
pub fn parents(
    graph: &CommitGraph,
    in_base: &[bool],
    candidates: &[Candidate],
) -> Vec<Option<usize>> {
    let n = candidates.len();
    let in_base = |row: u32| in_base.get(row as usize).copied().unwrap_or(false);
    // 幹ごとの、固有のコミット（既定のブランチの履歴に入る前まで）の行のビットと、その数。
    let words = graph.len().div_ceil(64);
    let trunk_sets: Vec<Option<(Vec<u64>, usize)>> = candidates
        .iter()
        .map(|c| {
            let tip = c.tip.filter(|_| c.trunk && !c.merged)?;
            let mut bits = vec![0u64; words];
            let mut count = 0;
            let mut stack = vec![tip];
            while let Some(row) = stack.pop() {
                let (w, b) = (row as usize / 64, row % 64);
                if in_base(row) || bits[w] & (1 << b) != 0 {
                    continue;
                }
                bits[w] |= 1 << b;
                count += 1;
                stack.extend(graph.parents(row as usize));
            }
            Some((bits, count))
        })
        .collect();
    let in_trunk = |a: usize, row: u32| {
        trunk_sets[a]
            .as_ref()
            .is_some_and(|(bits, _)| bits[row as usize / 64] & (1 << (row % 64)) != 0)
    };
    let trunk_size = |a: usize| trunk_sets[a].as_ref().map_or(0, |(_, count)| *count);
    // 先端の行 → その先端のブランチ。
    let mut at_tip: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
    for (a, c) in candidates.iter().enumerate() {
        if let Some(tip) = c.tip
            && !c.merged
        {
            at_tip.entry(tip).or_default().push(a);
        }
    }
    (0..n)
        .map(|b| {
            let c = &candidates[b];
            let tip = c.tip?;
            if c.trunk || c.merged {
                return None;
            }
            if let Some(a) = c
                .upstream_local
                .filter(|&a| a != b && candidates[a].tip.is_some() && candidates[a].tip != c.tip)
            {
                return Some(a);
            }
            let prefer = |a: usize| {
                let k = &candidates[a];
                (
                    c.upstream_local != Some(a),
                    !k.trunk,
                    !k.has_upstream,
                    trunk_size(a),
                    a,
                )
            };
            let trunks: Vec<usize> = (0..n)
                .filter(|&a| a != b && trunk_sets[a].is_some() && candidates[a].tip != c.tip)
                .filter(|&a| !in_trunk(a, tip))
                .collect();
            // 先端から、新しい順（行の順）に辿る。
            let mut heap = std::collections::BinaryHeap::new();
            let mut seen = std::collections::HashSet::new();
            heap.push(std::cmp::Reverse(tip));
            let mut walked = 0;
            while let Some(std::cmp::Reverse(row)) = heap.pop() {
                if in_base(row) || !seen.insert(row) {
                    continue;
                }
                walked += 1;
                if walked > PARENT_WALK {
                    break;
                }
                if row != tip {
                    if let Some(found) = at_tip.get(&row).and_then(|list| {
                        list.iter()
                            .copied()
                            .filter(|&a| a != b)
                            .min_by_key(|&a| prefer(a))
                    }) {
                        return Some(found);
                    }
                    if let Some(found) = trunks
                        .iter()
                        .copied()
                        .filter(|&a| in_trunk(a, row))
                        .min_by_key(|&a| prefer(a))
                    {
                        return Some(found);
                    }
                }
                heap.extend(
                    graph
                        .parents(row as usize)
                        .iter()
                        .map(|&p| std::cmp::Reverse(p)),
                );
            }
            None
        })
        .collect()
}

/// コミットのオブジェクトの `author` の行のメールアドレス（小文字）。
pub fn author_email(raw: &[u8]) -> Option<String> {
    let header_end = raw
        .windows(2)
        .position(|w| w == b"\n\n")
        .unwrap_or(raw.len());
    let line = raw[..header_end]
        .split(|&b| b == b'\n')
        .find(|l| l.starts_with(b"author "))?;
    let start = line.iter().position(|&b| b == b'<')? + 1;
    let end = start + line[start..].iter().position(|&b| b == b'>')?;
    Some(String::from_utf8_lossy(&line[start..end]).to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_model::{CommitGraphBuilder, Oid};

    fn id(n: u8) -> Oid {
        Oid([n; 20])
    }

    /// 新しい順: 4（topic の先、親 2）、3（main の先、親 1）、2（親 1）、1。
    fn graph() -> CommitGraph {
        let mut b = CommitGraphBuilder::with_capacity(4);
        b.push(id(4), 40, [id(2)]);
        b.push(id(3), 30, [id(1)]);
        b.push(id(2), 20, [id(1)]);
        b.push(id(1), 10, []);
        b.build()
    }

    #[test]
    fn finds_unique_commits_and_merged_branches() {
        let g = graph();
        let in_base = reachable(&g, &[1]);
        assert_eq!(in_base, vec![false, true, false, true]);
        assert_eq!(
            facts(&g, &in_base, 0, 100),
            BranchFacts {
                merged: false,
                unique: vec![0, 2]
            }
        );
        assert_eq!(
            facts(&g, &in_base, 3, 100),
            BranchFacts {
                merged: true,
                unique: vec![]
            }
        );
        assert_eq!(facts(&g, &in_base, 0, 1).unique, vec![0]);
    }

    #[test]
    fn finds_the_branch_underneath() {
        // 新しい順: 0（c の先、親 1）、1（b の先、親 2）、2（a の先、親 3）、3（main）。d は b と同じ先端。
        let mut builder = CommitGraphBuilder::with_capacity(4);
        builder.push(id(4), 40, [id(3)]);
        builder.push(id(3), 30, [id(2)]);
        builder.push(id(2), 20, [id(1)]);
        builder.push(id(1), 10, []);
        let g = builder.build();
        let cand = |tip: u32, trunk: bool| Candidate {
            tip: Some(tip),
            trunk,
            has_upstream: trunk,
            ..Default::default()
        };
        // a は main の上、b は a の上、c は b の上（同じ先端の d ではなく、添字の小さい b）、d は a の上。
        let cands = [
            cand(2, false),
            cand(1, false),
            cand(0, false),
            cand(1, false),
            cand(3, true),
        ];
        assert_eq!(
            parents(&g, &[false; 4], &cands),
            vec![Some(4), Some(0), Some(1), Some(0), None]
        );
    }

    #[test]
    fn finds_the_branch_it_forked_from_even_after_it_moved() {
        // 0（trunk の先、親 2）、1（wip の先、親 2）、2（分かれた点、親 3）、3。trunk は wip を切ったあとに進んだ。
        let mut builder = CommitGraphBuilder::with_capacity(4);
        builder.push(id(4), 40, [id(2)]);
        builder.push(id(3), 30, [id(2)]);
        builder.push(id(2), 20, [id(1)]);
        builder.push(id(1), 10, []);
        let g = builder.build();
        let trunk = Candidate {
            tip: Some(0),
            trunk: true,
            has_upstream: true,
            ..Default::default()
        };
        let wip = Candidate {
            tip: Some(1),
            upstream_local: Some(0),
            ..Default::default()
        };
        // 同じ点から分かれたもう 1 本（上流なし）があっても、上流に当たる trunk を選ぶ。
        let sibling = Candidate {
            tip: Some(1),
            ..Default::default()
        };
        assert_eq!(
            parents(&g, &[false; 4], &[trunk.clone(), wip.clone()]),
            vec![None, Some(0)]
        );
        let mut builder = CommitGraphBuilder::with_capacity(5);
        builder.push(id(5), 50, [id(2)]);
        builder.push(id(4), 40, [id(2)]);
        builder.push(id(3), 30, [id(2)]);
        builder.push(id(2), 20, [id(1)]);
        builder.push(id(1), 10, []);
        let g = builder.build();
        let sibling = Candidate {
            tip: Some(0),
            ..sibling
        };
        let wip = Candidate {
            tip: Some(1),
            upstream_local: Some(1),
            ..wip
        };
        let trunk = Candidate {
            tip: Some(2),
            ..trunk
        };
        assert_eq!(
            parents(&g, &[false; 5], &[sibling, trunk, wip]),
            vec![Some(1), None, Some(1)]
        );
    }

    fn trunk(tip: u32) -> Candidate {
        Candidate {
            tip: Some(tip),
            trunk: true,
            has_upstream: true,
            ..Default::default()
        }
    }

    fn topic(tip: u32) -> Candidate {
        Candidate {
            tip: Some(tip),
            ..Default::default()
        }
    }

    #[test]
    fn does_not_stack_on_a_trunk_that_only_shares_the_default_branch() {
        // 0（pr の先、親 1）、1（main の先、親 2）、2（old の先、親 3）、3。old は main の古い所から分かれた
        // だけで、あとから main を push した pr とは無関係。pr の履歴は main の履歴ごと持つが、それを幹の印と
        // 見なさない。
        let mut builder = CommitGraphBuilder::with_capacity(4);
        builder.push(id(4), 40, [id(2)]);
        builder.push(id(3), 30, [id(1)]);
        builder.push(id(2), 20, [id(0)]);
        builder.push(id(1), 10, []);
        let g = builder.build();
        let in_base = reachable(&g, &[1]);
        let main = Candidate {
            merged: true,
            ..trunk(1)
        };
        assert_eq!(
            parents(&g, &in_base, &[trunk(0), main, topic(2)]),
            vec![None, None, None]
        );
        // pr の固有のコミットから分かれたものは、pr の上。
        let mut builder = CommitGraphBuilder::with_capacity(5);
        builder.push(id(5), 50, [id(4)]);
        builder.push(id(4), 40, [id(2)]);
        builder.push(id(3), 30, [id(1)]);
        builder.push(id(2), 20, [id(0)]);
        builder.push(id(1), 10, []);
        let g = builder.build();
        let in_base = reachable(&g, &[2]);
        let main = Candidate {
            merged: true,
            ..trunk(2)
        };
        assert_eq!(
            parents(&g, &in_base, &[topic(0), trunk(1), main]),
            vec![Some(1), None, None]
        );
    }

    #[test]
    fn stops_at_the_trunk_it_forked_from() {
        // 0（x の先、親 2）、1（pr の先、親 2）、2（pr の古いコミット、親 3）、3（stale の先、親 4）、4（main）。
        // pr は stale の上に作ったあと進み、x は pr の途中から分かれた。x の土台は pr であって、その向こうの
        // stale ではない。
        let mut builder = CommitGraphBuilder::with_capacity(5);
        builder.push(id(5), 50, [id(3)]);
        builder.push(id(4), 40, [id(3)]);
        builder.push(id(3), 30, [id(2)]);
        builder.push(id(2), 20, [id(1)]);
        builder.push(id(1), 10, []);
        let g = builder.build();
        let in_base = reachable(&g, &[4]);
        let main = Candidate {
            merged: true,
            ..trunk(4)
        };
        assert_eq!(
            parents(&g, &in_base, &[topic(0), trunk(1), topic(3), main]),
            vec![Some(1), None, None, None]
        );
    }

    #[test]
    fn prefers_the_lower_trunk_when_both_hold_the_fork_point() {
        // 0（upper の先、親 1）、1（lower の先、親 3）、2（x の先、親 3）、3（lower の古いコミット、親 4）、
        // 4（main）。upper は lower の上に積んであり、x は lower の途中から分かれた。どちらの幹も 3 を持つが、
        // 土台は固有のコミットが少ない lower（添字は upper のほうが小さくても）。
        let mut builder = CommitGraphBuilder::with_capacity(5);
        builder.push(id(5), 50, [id(4)]);
        builder.push(id(4), 40, [id(2)]);
        builder.push(id(3), 30, [id(2)]);
        builder.push(id(2), 20, [id(1)]);
        builder.push(id(1), 10, []);
        let g = builder.build();
        let in_base = reachable(&g, &[4]);
        let main = Candidate {
            merged: true,
            ..trunk(4)
        };
        assert_eq!(
            parents(&g, &in_base, &[trunk(0), trunk(1), topic(2), main]),
            vec![None, None, Some(1), None]
        );
    }

    #[test]
    fn reads_the_author_email() {
        let raw = b"tree x\nauthor A B <A@Example.com> 1 +0900\ncommitter C <c@x> 1 +0900\n\nmsg author <no>";
        assert_eq!(author_email(raw).as_deref(), Some("a@example.com"));
    }
}
