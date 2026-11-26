//! 層状の配置（Gansner ほか 1993 の 4 つの段）。座標はいつも上から下（ランクが y、並びが x）で、ほかの向きは
//! 呼ぶ側が回す。
//!
//! 1. ランク: `rank=same` などの組を 1 つにまとめ、閉路を作る辺を逆にし、ネットワーク単体法で
//!    `Σ weight * 長さ` を小さくする（`minlen`・`constraint=false`）。
//! 2. 並び: 2 つ以上のランクをまたぐ辺に仮のノードを置き、中央値で上下に何度も並べ替え、隣どうしの入れ替えで交差を
//!    減らす。クラスタは各ランクでひとまとまりに保ち、兄弟のクラスタの左右はどのランクでもそろえる（箱が重ならない
//!    ように）。
//! 3. 横の位置: 補助グラフ（辺ごとのノード、クラスタの左右の境）のネットワーク単体法で、辺をまっすぐに保ちながら
//!    ノードの間を空ける。
//! 4. 縦の位置: ランクの高さとクラスタの枠・題の分を空ける。

use crate::graph::RankKind;
use crate::simplex::{self, Edge as NsEdge};

#[derive(Clone, Debug)]
pub struct Node {
    /// 配置の向きの幅（並びの向き）と高さ（ランクの向き）。
    pub w: f64,
    pub h: f64,
    pub cluster: Option<usize>,
    /// 右に空ける分（自分への辺の輪）。
    pub extra_right: f64,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub tail: usize,
    pub head: usize,
    pub minlen: f64,
    pub weight: f64,
    pub constraint: bool,
    /// ラベルの大きさ（配置の向き）。
    pub label: Option<(f64, f64)>,
    /// ポートの並びの向きのずれ。
    pub tail_off: f64,
    pub head_off: f64,
    /// 見えない辺（配置には効くが、同じランクの弧の段や空きは取らない）。
    pub invisible: bool,
}

#[derive(Clone, Debug)]
pub struct Cluster {
    pub parent: Option<usize>,
    pub margin: f64,
    /// 題の大きさ（配置の向き）。
    pub label: (f64, f64),
    /// 題を置く側（配置の座標で）。
    pub side: LabelSide,
}

/// クラスタの題を置く側（配置の座標で）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelSide {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub nodesep: f64,
    pub ranksep: f64,
    pub ordering_out: bool,
    /// 辺のラベルを線の左に置く（ふつうは右）。
    pub label_left: bool,
    /// 単体法の入れ替えの上限（ランク・横の位置）。
    pub rank_iter: usize,
    pub x_iter: usize,
    /// 並べ替えの回数。
    pub sweeps: usize,
    /// クラスタごとにランクを決める（Graphviz の `newrank=false`）。
    pub cluster_rank: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    /// 尾から頭への点（両端はノードの中心とポートのずれ）。ラベルの中心。
    Path {
        points: Vec<(f64, f64)>,
        label: Option<(f64, f64)>,
    },
    /// 同じランクの辺。`lane` は同じ組の何本目か。
    Flat { lane: usize, adjacent: bool },
    /// 自分への辺。`index` はそのノードの何本目か。
    Loop { index: usize },
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub nodes: Vec<(f64, f64)>,
    pub ranks: Vec<i32>,
    pub routes: Vec<Route>,
    /// クラスタの箱（x0, y0, x1, y1）。中にノードがなければ None。
    pub clusters: Vec<Option<[f64; 4]>>,
    pub width: f64,
    pub height: f64,
    /// 並びを決めたあとの交差の数。
    pub crossings: usize,
    /// 辺ごとの通り道（尾から頭へ、仮のノードのランクの行の上下の端での x の区間 (y, 左, 右)）。曲線はこの中を通す
    /// （Gansner ほか 1993 の §5）。仮のノードのない辺は空。
    pub corridors: Vec<Vec<Portal>>,
    /// 仮のノードの上限を超えたので、長い辺の一部を仮のノードなしの直線にした。
    pub straightened: bool,
}

/// 通り道の 1 つの関（y の高さで x が `lo` から `hi` の間）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Portal {
    pub y: f64,
    pub lo: f64,
    pub hi: f64,
}

/// 辺の仮のノードの列（上から下、両端を含む）、逆向きか、ラベルの仮のノード。
type Chain = (Vec<usize>, bool, Option<usize>);

/// 仮のノードの上限。
const MAX_DUMMIES: usize = 12_000;
/// 直角の線の段の間の下限（ランクの間を広げる上限を超えても保つ）。
const MIN_TRACK: f64 = 3.0;
/// 同じランクの辺の輪を調べる辺の数の上限（辺ごとに深さ優先でたどるので、2 乗で増える）。
const FLAT_CYCLE_EDGES: usize = 4096;
/// 並べ替えで、交差が減らない回がこれだけ続けばやめる。
const MINCROSS_STALE: usize = 12;
/// 隣どうしの入れ替えを使う、ランクの間の線の切れ端の数の上限（大きな図の時間のため）。
const TRANSPOSE_SEGS: usize = 1500;

// ---- 1. ランク ----

struct Dsu(Vec<usize>);

impl Dsu {
    fn find(&mut self, mut v: usize) -> usize {
        while self.0[v] != v {
            self.0[v] = self.0[self.0[v]];
            v = self.0[v];
        }
        v
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// ランクの制約の辺（単位の間）。逆にしたときの `minlen` も持つ（単位の中のずれが違うため）。
struct Con {
    tail: usize,
    head: usize,
    /// `x[head] - x[tail] >= fwd`。
    fwd: i64,
    /// 逆にしたとき: `x[tail] - x[head] >= rev`。
    rev: i64,
    weight: f64,
}

/// 強連結成分の番号（Tarjan、明示のスタック）。
fn components(units: usize, out: &[Vec<usize>], cons: &[Con]) -> Vec<usize> {
    const NONE: usize = usize::MAX;
    let mut index = vec![NONE; units];
    let mut low = vec![0usize; units];
    let mut on = vec![false; units];
    let mut comp = vec![NONE; units];
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 0;
    let mut count = 0;
    for root in 0..units {
        if index[root] != NONE {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on[root] = true;
        while let Some(&mut (v, ref mut k)) = call.last_mut() {
            if *k < out[v].len() {
                let w = cons[out[v][*k]].head;
                *k += 1;
                if index[w] == NONE {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on[w] = true;
                    call.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                call.pop();
                if let Some(&(p, _)) = call.last() {
                    low[p] = low[p].min(low[v]);
                }
                if low[v] == index[v] {
                    while let Some(w) = stack.pop() {
                        on[w] = false;
                        comp[w] = count;
                        if w == v {
                            break;
                        }
                    }
                    count += 1;
                }
            }
        }
    }
    comp
}

/// 単位の形: クラスタをまとめたものか、そのランクの幅。
#[derive(Clone, Copy, Debug)]
struct UnitShape {
    rigid: bool,
    span: i64,
}

/// 単位（`units` 個、番号は 0 から）のランクを解く。`cons` は閉路を作るものを逆にし、`fixed` はそのまま足す
/// （`total` は `fixed` のノードを含めた数）。`first` があれば閉路を探す深さ優先をそこから始める。
///
/// `rigid` はクラスタをまとめた単位。それを含む閉路（強連結成分）に入る単位に触れる辺は、向きを強いずに
/// `weight * |長さ - minlen|` を小さくする（クラスタの中のずれで成り立つ閉路は並べ、成り立たなければ重みの軽い辺が
/// 逆を向き、外のノードへの辺も得なら横や上を向く。Graphviz の `newrank=false` の絵と同じで、説明書の「head が
/// tail より上のランクになることがある」にあたる）。ほかの閉路は深さ優先で逆にする（Gansner ほか 1993）。
fn solve_ranks(
    total: usize,
    units: usize,
    cons: &[Con],
    fixed: &[NsEdge],
    first: Option<usize>,
    iter: usize,
    shape: &[UnitShape],
) -> Vec<i64> {
    let rigid: Vec<bool> = shape.iter().map(|s| s.rigid).collect();
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); units];
    for (i, c) in cons.iter().enumerate() {
        out[c.tail].push(i);
    }
    // クラスタを含む閉路（強連結成分）に入る単位に触れる辺（向きを強いない）
    let soft: Vec<bool> = if rigid.iter().any(|&r| r) {
        let comp = components(units, &out, cons);
        let mut size = vec![0usize; units];
        let mut has_rigid = vec![false; units];
        for u in 0..units {
            size[comp[u]] += 1;
            has_rigid[comp[u]] |= rigid.get(u).copied().unwrap_or(false);
        }
        let cyclic = |u: usize| size[comp[u]] > 1 && has_rigid[comp[u]];
        cons.iter()
            .map(|c| cyclic(c.tail) || cyclic(c.head))
            .collect()
    } else {
        vec![false; cons.len()]
    };
    let mut state = vec![0u8; units];
    let mut reverse = vec![false; cons.len()];
    for root in first.into_iter().chain(0..units) {
        if state[root] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        state[root] = 1;
        while let Some(&mut (v, ref mut k)) = stack.last_mut() {
            if *k < out[v].len() {
                let ei = out[v][*k];
                *k += 1;
                if soft[ei] {
                    continue;
                }
                let w = cons[ei].head;
                if state[w] == 1 {
                    reverse[ei] = true;
                } else if state[w] == 0 {
                    state[w] = 1;
                    stack.push((w, 0));
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    let mut edges: Vec<NsEdge> = Vec::with_capacity(cons.len() + fixed.len());
    // 向きを強いない辺は補助のノード z で |x_h - (x_t + d)| にする（z → t は c - d、z → h は c、c = max(d, 0)。
    // z → t の重みは短すぎる（上を向く）分、z → h は長すぎる分）。同じ値の形がいくつもあれば、辺が下を向くほうを選び
    // （短すぎる分をわずかに重く）、それでも同じなら背の高い単位（クラスタのランクの幅）から低い単位への辺を、
    // 同じ高さなら書いた順で先の単位から後の単位への辺を下に向ける（高いクラスタが上。Graphviz の絵と同じ）
    let key = |u: usize| (std::cmp::Reverse(shape.get(u).map_or(0, |s| s.span)), u);
    let mut aux = total;
    for ((c, &r), &s) in cons.iter().zip(&reverse).zip(&soft) {
        if s {
            let d = c.fwd;
            let m = d.max(0);
            let short = if key(c.tail) < key(c.head) {
                1.00101
            } else {
                1.001
            };
            edges.push(NsEdge {
                tail: aux,
                head: c.tail,
                minlen: m - d,
                weight: c.weight * short,
            });
            edges.push(NsEdge {
                tail: aux,
                head: c.head,
                minlen: m,
                weight: c.weight,
            });
            aux += 1;
        } else if r {
            edges.push(NsEdge {
                tail: c.head,
                head: c.tail,
                minlen: c.rev,
                weight: c.weight,
            });
        } else {
            edges.push(NsEdge {
                tail: c.tail,
                head: c.head,
                minlen: c.fwd,
                weight: c.weight,
            });
        }
    }
    edges.extend_from_slice(fixed);
    let real = total;
    let total = aux;
    // 初めの値: 最長路（トポロジカル順）
    let mut x = vec![0i64; total];
    let mut indeg = vec![0usize; total];
    let mut outs: Vec<Vec<usize>> = vec![Vec::new(); total];
    for (i, e) in edges.iter().enumerate() {
        indeg[e.head] += 1;
        outs[e.tail].push(i);
    }
    let mut queue: Vec<usize> = (0..total).filter(|&v| indeg[v] == 0).collect();
    let mut q = 0;
    while q < queue.len() {
        let v = queue[q];
        q += 1;
        for &ei in &outs[v] {
            let e = &edges[ei];
            x[e.head] = x[e.head].max(x[v] + e.minlen);
            indeg[e.head] -= 1;
            if indeg[e.head] == 0 {
                queue.push(e.head);
            }
        }
    }
    simplex::solve(total, &edges, &mut x, iter);
    x.truncate(real);
    x
}

/// ランクを決める。`rigid` なら、クラスタを内から順に、その中の辺だけでランクを決めて 1 つの単位にまとめる
/// （Graphviz の既定の `newrank=false` と同じく、クラスタの中のつながらないノードはクラスタの上のランクに並ぶ）。
#[allow(clippy::too_many_arguments)]
fn rank(
    n: usize,
    edges: &[Edge],
    sets: &[(RankKind, Vec<usize>)],
    cluster_of: &[Option<usize>],
    parent: &[Option<usize>],
    depth: &[usize],
    rigid: bool,
    opt: &Options,
) -> Vec<i32> {
    if n == 0 {
        return Vec::new();
    }
    let clusters = parent.len();
    let mut dsu = Dsu((0..n).collect());
    let mut min_any: Option<usize> = None;
    let mut max_any: Option<usize> = None;
    let mut source = false;
    let mut sink = false;
    for (kind, nodes) in sets {
        let Some(&first) = nodes.first() else {
            continue;
        };
        for &v in nodes {
            dsu.union(first, v);
        }
        match kind {
            RankKind::Same => {}
            RankKind::Min | RankKind::Source => {
                if let Some(m) = min_any {
                    dsu.union(m, first);
                }
                min_any = Some(first);
                source |= *kind == RankKind::Source;
            }
            RankKind::Max | RankKind::Sink => {
                if let Some(m) = max_any {
                    dsu.union(m, first);
                }
                max_any = Some(first);
                sink |= *kind == RankKind::Sink;
            }
        }
    }
    // 単位: 組ごと（ずれ 0）
    let mut unit: Vec<usize> = (0..n).map(|v| dsu.find(v)).collect();
    let mut off = vec![0i64; n];
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); n];
    for v in 0..n {
        members[unit[v]].push(v);
    }
    // クラスタをまとめた単位
    let mut collapsed = vec![false; n];
    let min_u = min_any.map(|v| unit[v]);
    let max_u = max_any.map(|v| unit[v]);
    let min_u = if min_u.is_some() && min_u == max_u {
        None
    } else {
        min_u
    };
    let constraint = |e: &Edge| e.constraint && e.tail < n && e.head < n;
    let minlen = |e: &Edge| e.minlen.round().clamp(0.0, 1000.0) as i64;

    if rigid && clusters > 0 {
        // クラスタの中のノード
        let mut inside: Vec<Vec<usize>> = vec![Vec::new(); clusters];
        for (v, &own) in cluster_of.iter().enumerate().take(n) {
            let mut c = own.filter(|&c| c < clusters);
            let mut hops = 0;
            while let Some(ci) = c {
                inside[ci].push(v);
                c = parent[ci];
                hops += 1;
                if hops > clusters {
                    break;
                }
            }
        }
        let mut order: Vec<usize> = (0..clusters).collect();
        order.sort_by_key(|&c| std::cmp::Reverse(depth[c]));
        let mut mark = vec![usize::MAX; n];
        for c in order {
            if inside[c].len() < 2 {
                continue;
            }
            for &v in &inside[c] {
                mark[v] = c;
            }
            // 外のノードと組になった単位（rank=same が外をまたぐ）や最小・最大の組があれば、まとめない
            let mut local: Vec<usize> = inside[c].iter().map(|&v| unit[v]).collect();
            local.sort_unstable();
            local.dedup();
            if local.len() < 2
                || local.iter().any(|&u| {
                    Some(u) == min_u || Some(u) == max_u || members[u].iter().any(|&v| mark[v] != c)
                })
            {
                continue;
            }
            let index = |u: usize| local.binary_search(&u).ok();
            let mut cons = Vec::new();
            for e in edges.iter().filter(|e| constraint(e)) {
                if mark[e.tail] != c || mark[e.head] != c {
                    continue;
                }
                let (Some(t), Some(h)) = (index(unit[e.tail]), index(unit[e.head])) else {
                    continue;
                };
                if t == h {
                    continue;
                }
                let m = minlen(e);
                cons.push(Con {
                    tail: t,
                    head: h,
                    fwd: m + off[e.tail] - off[e.head],
                    rev: m + off[e.head] - off[e.tail],
                    weight: e.weight.clamp(0.0, 1000.0),
                });
            }
            let units = local.len();
            let root = units;
            let fixed: Vec<NsEdge> = (0..units)
                .map(|u| NsEdge {
                    tail: root,
                    head: u,
                    minlen: 0,
                    weight: 0.0,
                })
                .collect();
            let shape: Vec<UnitShape> = local
                .iter()
                .map(|&u| {
                    let offs = members[u].iter().map(|&v| off[v]);
                    UnitShape {
                        rigid: collapsed[u],
                        span: offs.clone().max().unwrap_or(0) - offs.min().unwrap_or(0),
                    }
                })
                .collect();
            let x = solve_ranks(units + 1, units, &cons, &fixed, None, opt.rank_iter, &shape);
            let low = x[..units].iter().copied().min().unwrap_or(0);
            let into = local[0];
            for (i, &u) in local.iter().enumerate() {
                let shift = x[i] - low;
                let moved = std::mem::take(&mut members[u]);
                for &v in &moved {
                    off[v] += shift;
                    unit[v] = into;
                }
                members[into].extend(moved);
            }
            collapsed[into] = true;
        }
    }

    // 単位の番号を詰める
    let mut dense = vec![usize::MAX; n];
    let mut units = 0;
    for &u in unit.iter().take(n) {
        if dense[u] == usize::MAX {
            dense[u] = units;
            units += 1;
        }
    }
    let min_c = min_u.map(|u| dense[u]);
    let max_c = max_u.map(|u| dense[u]);
    let mut cons = Vec::new();
    for e in edges.iter().filter(|e| constraint(e)) {
        let (mut t, mut h) = (e.tail, e.head);
        let (mut ut, mut uh) = (dense[unit[t]], dense[unit[h]]);
        if ut == uh {
            continue;
        }
        if Some(uh) == min_c || Some(ut) == max_c {
            std::mem::swap(&mut t, &mut h);
            std::mem::swap(&mut ut, &mut uh);
        }
        let m = minlen(e);
        cons.push(Con {
            tail: ut,
            head: uh,
            fwd: m + off[t] - off[h],
            rev: m + off[h] - off[t],
            weight: e.weight.clamp(0.0, 1000.0),
        });
    }
    // 最小・最大の組と、根（すべてをつなぐ）
    let root = units;
    let mut fixed: Vec<NsEdge> = Vec::new();
    for c in 0..units {
        if let Some(m) = min_c
            && c != m
        {
            fixed.push(NsEdge {
                tail: m,
                head: c,
                minlen: i64::from(source),
                weight: 0.0,
            });
        }
        if let Some(m) = max_c
            && c != m
        {
            fixed.push(NsEdge {
                tail: c,
                head: m,
                minlen: i64::from(sink),
                weight: 0.0,
            });
        }
        fixed.push(NsEdge {
            tail: root,
            head: c,
            minlen: 0,
            weight: 0.0,
        });
    }
    // クラスタのノードを上にそろえる弱い引き（まとめなかったクラスタのため）
    let mut seen_pair = std::collections::HashSet::new();
    for v in 0..n {
        if let Some(c) = cluster_of[v].filter(|&c| c < clusters)
            && seen_pair.insert((c, dense[unit[v]]))
        {
            fixed.push(NsEdge {
                tail: units + 1 + c,
                head: dense[unit[v]],
                minlen: -off[v],
                weight: 0.25,
            });
        }
    }
    for c in 0..clusters {
        fixed.push(NsEdge {
            tail: root,
            head: units + 1 + c,
            minlen: 0,
            weight: 0.0,
        });
    }
    let total = units + 1 + clusters;
    let mut rigid_units = vec![false; units];
    for u in 0..n {
        if dense[u] != usize::MAX {
            rigid_units[dense[u]] |= collapsed[u];
        }
    }
    // 単位のランクの幅（クラスタの高さ）
    let mut lo = vec![i64::MAX; units];
    let mut hi = vec![i64::MIN; units];
    for v in 0..n {
        let u = dense[unit[v]];
        if u < units {
            lo[u] = lo[u].min(off[v]);
            hi[u] = hi[u].max(off[v]);
        }
    }
    let shape: Vec<UnitShape> = (0..units)
        .map(|u| UnitShape {
            rigid: rigid_units[u],
            span: if hi[u] >= lo[u] { hi[u] - lo[u] } else { 0 },
        })
        .collect();
    let x = solve_ranks(total, units, &cons, &fixed, min_c, opt.rank_iter, &shape);
    let r: Vec<i64> = (0..n).map(|v| x[dense[unit[v]]] + off[v]).collect();
    let low = r.iter().copied().min().unwrap_or(0);
    r.into_iter()
        .map(|v| (v - low).clamp(0, 100_000) as i32)
        .collect()
}

// ---- 2. 並び ----

/// 並べるノード（本物・仮・クラスタの詰め物）。
#[derive(Clone, Debug)]
struct LNode {
    rank: usize,
    cluster: Option<usize>,
    /// 左右の半分の幅。
    left: f64,
    right: f64,
    h: f64,
    real: bool,
    /// 上下の切れ端。
    up: Vec<usize>,
    down: Vec<usize>,
    /// 元の順（並びのくずれのない比べ）。
    seq: usize,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    upper: usize,
    lower: usize,
    weight: f64,
    upper_off: f64,
    lower_off: f64,
}

struct Ordering<'a> {
    nodes: &'a [LNode],
    segs: &'a [Segment],
    layers: Vec<Vec<usize>>,
    pos: Vec<usize>,
    parent: &'a [Option<usize>],
    /// 兄弟のクラスタの左右の順の手がかり。
    global: Vec<f64>,
    /// 同じランク・同じクラスタの辺: 尾の右に置く頭（Gansner ほか 1993 §3 の、同じランクの辺が決める左右の順）。
    after: Vec<Vec<usize>>,
    /// その辺のあるランク。
    has_flat: Vec<bool>,
}

impl Ordering<'_> {
    /// 切れ端の上か下の端の並びの値: 位置に、ポートのずれをノードの幅で割った分（±0.45 まで）を足す。同じノードに
    /// 着く辺も、ポートの左右の順で並ぶ（Gansner ほか 1993 §3）。
    fn end_key(&self, s: usize, upper: bool) -> f64 {
        let seg = &self.segs[s];
        let (v, off) = if upper {
            (seg.upper, seg.upper_off)
        } else {
            (seg.lower, seg.lower_off)
        };
        let w = self.nodes[v].left + self.nodes[v].right;
        let frac = if off != 0.0 && w > 0.0 {
            (off / w).clamp(-0.45, 0.45)
        } else {
            0.0
        };
        self.pos[v] as f64 + frac
    }

    fn refresh(&mut self) {
        for layer in &self.layers {
            for (i, &v) in layer.iter().enumerate() {
                self.pos[v] = i;
            }
        }
    }

    /// クラスタの左右の手がかり: 中のノードの相対の位置の平均。
    fn global_keys(&mut self) {
        let k = self.global.len();
        let mut sum = vec![0f64; k];
        let mut count = vec![0usize; k];
        for layer in &self.layers {
            let len = layer.len().max(1) as f64;
            for (i, &v) in layer.iter().enumerate() {
                let mut c = self.nodes[v].cluster;
                let mut hops = 0;
                while let Some(ci) = c {
                    sum[ci] += i as f64 / len;
                    count[ci] += 1;
                    c = self.parent[ci];
                    hops += 1;
                    if hops > 1000 {
                        break;
                    }
                }
            }
        }
        for ci in 0..k {
            self.global[ci] = if count[ci] > 0 {
                sum[ci] / count[ci] as f64
            } else {
                0.0
            };
        }
    }

    /// 1 つのランクを値 `val` の順に並べる。クラスタはまとまり、兄弟のクラスタは `global` の順。
    fn sort_layer(&mut self, r: usize, val: &[f64]) {
        let layer = std::mem::take(&mut self.layers[r]);
        // クラスタ → その中の直接の要素（ノードか子のクラスタ）
        #[derive(Clone, Copy)]
        enum Item {
            Node(usize),
            Cluster(usize),
        }
        let k = self.global.len();
        let mut groups: Vec<Vec<Item>> = vec![Vec::new(); k + 1];
        let mut seen = vec![false; k];
        let mut gsum = vec![0f64; k];
        let mut gcount = vec![0usize; k];
        let mut gfirst = vec![usize::MAX; k];
        for &v in &layer {
            let slot = |c: Option<usize>| c.unwrap_or(k);
            groups[slot(self.nodes[v].cluster)].push(Item::Node(v));
            let mut c = self.nodes[v].cluster;
            let mut hops = 0;
            while let Some(ci) = c {
                gsum[ci] += val[v];
                gcount[ci] += 1;
                gfirst[ci] = gfirst[ci].min(self.pos[v]);
                if !seen[ci] {
                    seen[ci] = true;
                    groups[slot(self.parent[ci])].push(Item::Cluster(ci));
                }
                c = self.parent[ci];
                hops += 1;
                if hops > 1000 {
                    break;
                }
            }
        }
        let value = |it: &Item| -> (f64, usize) {
            match *it {
                Item::Node(v) => (val[v], self.pos[v]),
                Item::Cluster(c) => (gsum[c] / gcount[c].max(1) as f64, gfirst[c]),
            }
        };
        let mut out: Vec<usize> = Vec::with_capacity(layer.len());
        // 深さ優先で書き出す（明示のスタック）
        let mut stack: Vec<(usize, usize)> = Vec::new();
        let mut sorted: Vec<Vec<Item>> = vec![Vec::new(); k + 1];
        let prepare = |g: usize, groups: &mut Vec<Vec<Item>>, sorted: &mut Vec<Vec<Item>>| {
            let mut items = std::mem::take(&mut groups[g]);
            items.sort_by(|a, b| {
                let (va, pa) = value(a);
                let (vb, pb) = value(b);
                va.partial_cmp(&vb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(pa.cmp(&pb))
            });
            // クラスタの場所は、兄弟のクラスタのいつもの順で埋め直す
            let mut slots: Vec<usize> = Vec::new();
            let mut clusters: Vec<usize> = Vec::new();
            for (i, it) in items.iter().enumerate() {
                if let Item::Cluster(c) = it {
                    slots.push(i);
                    clusters.push(*c);
                }
            }
            if clusters.len() > 1 {
                clusters.sort_by(|a, b| {
                    self.global[*a]
                        .partial_cmp(&self.global[*b])
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.cmp(b))
                });
                for (s, c) in slots.into_iter().zip(clusters) {
                    items[s] = Item::Cluster(c);
                }
            }
            sorted[g] = items;
        };
        prepare(k, &mut groups, &mut sorted);
        stack.push((k, 0));
        while let Some(&mut (g, ref mut i)) = stack.last_mut() {
            if *i >= sorted[g].len() {
                stack.pop();
                continue;
            }
            let it = sorted[g][*i];
            *i += 1;
            match it {
                Item::Node(v) => out.push(v),
                Item::Cluster(c) => {
                    prepare(c, &mut groups, &mut sorted);
                    stack.push((c, 0));
                }
            }
        }
        self.layers[r] = out;
        for (i, &v) in self.layers[r].iter().enumerate() {
            self.pos[v] = i;
        }
        self.keep_flat_order(r);
    }

    /// 同じランクの辺の尾を頭の左に: 今の順をなるべく保つ位相の順（頭は、尾のすぐ後ろへ下がる）。輪になったら、
    /// 残りでいちばん左のものから出す。
    fn keep_flat_order(&mut self, r: usize) {
        if !self.has_flat[r] {
            return;
        }
        let layer = std::mem::take(&mut self.layers[r]);
        let mut need = std::collections::HashMap::<usize, usize>::new();
        for &v in &layer {
            for &h in &self.after[v] {
                *need.entry(h).or_insert(0) += 1;
            }
        }
        let mut heap = std::collections::BinaryHeap::new();
        for &v in &layer {
            if !need.contains_key(&v) {
                heap.push(std::cmp::Reverse((self.pos[v], v)));
            }
        }
        let mut done = std::collections::HashSet::<usize>::new();
        let mut out = Vec::with_capacity(layer.len());
        while out.len() < layer.len() {
            let v = match heap.pop() {
                Some(std::cmp::Reverse((_, v))) => v,
                None => match layer
                    .iter()
                    .copied()
                    .filter(|v| !done.contains(v))
                    .min_by_key(|&v| self.pos[v])
                {
                    Some(v) => v,
                    None => break,
                },
            };
            if !done.insert(v) {
                continue;
            }
            out.push(v);
            for &h in &self.after[v] {
                if let Some(c) = need.get_mut(&h) {
                    *c -= 1;
                    if *c == 0 && !done.contains(&h) {
                        heap.push(std::cmp::Reverse((self.pos[h], h)));
                    }
                }
            }
        }
        self.layers[r] = out;
        for (i, &v) in self.layers[r].iter().enumerate() {
            self.pos[v] = i;
        }
    }

    fn median(&self, list: &[usize], upper: bool) -> Option<f64> {
        let mut p: Vec<f64> = list.iter().map(|&s| self.end_key(s, upper)).collect();
        if p.is_empty() {
            return None;
        }
        p.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let m = p.len() / 2;
        Some(if p.len() % 2 == 1 {
            p[m]
        } else if p.len() == 2 {
            (p[0] + p[1]) / 2.0
        } else {
            // 重みつきの中央値（Gansner ほか）
            let left = p[m - 1] - p[0];
            let right = p[p.len() - 1] - p[m];
            if left + right == 0.0 {
                (p[m - 1] + p[m]) / 2.0
            } else {
                (p[m - 1] * right + p[m] * left) / (left + right)
            }
        })
    }

    fn sweep(&mut self, down: bool, val: &mut [f64]) {
        let ranks = self.layers.len();
        let order: Vec<usize> = if down {
            (1..ranks).collect()
        } else {
            (0..ranks.saturating_sub(1)).rev().collect()
        };
        for r in order {
            for &v in &self.layers[r] {
                let list = if down {
                    &self.nodes[v].up
                } else {
                    &self.nodes[v].down
                };
                val[v] = self.median(list, down).unwrap_or(self.pos[v] as f64);
            }
            self.sort_layer(r, val);
        }
    }

    fn crossings(&self) -> usize {
        let mut total = 0usize;
        for r in 0..self.layers.len().saturating_sub(1) {
            let mut keys: Vec<(f64, f64)> = Vec::new();
            for &u in &self.layers[r] {
                for &s in &self.nodes[u].down {
                    keys.push((self.end_key(s, true), self.end_key(s, false)));
                }
            }
            // 下の端の値を順位に直す（同じ値は同じ順位）
            let mut lows: Vec<f64> = keys.iter().map(|k| k.1).collect();
            lows.sort_by(f64::total_cmp);
            lows.dedup();
            let mut pairs: Vec<(f64, usize)> = keys
                .iter()
                .map(|&(a, b)| (a, lows.partition_point(|&x| x < b)))
                .collect();
            pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            // 下の位置の逆順の数（BIT）
            let size = lows.len() + 1;
            let mut tree = vec![0usize; size + 1];
            for (seen, &(_, b)) in pairs.iter().enumerate() {
                // b より大きいものの数
                let mut i = b + 1;
                let mut le = 0usize;
                while i > 0 {
                    le += tree[i];
                    i &= i - 1;
                }
                total += seen - le;
                let mut i = b + 1;
                while i <= size {
                    tree[i] += 1;
                    i += i & i.wrapping_neg();
                }
            }
        }
        total
    }

    /// 隣り合う同じクラスタのノードを、交差が減るなら入れ替える。`equal` なら、交差が同じでも入れ替える（同じ数の
    /// 並びの間を動いて、次の並べ替えで減る形を探す。Gansner ほか 1993 の transpose）。
    fn transpose(&mut self, equal: bool) {
        for _ in 0..4 {
            let mut improved = false;
            for r in 0..self.layers.len() {
                let len = self.layers[r].len();
                for i in 0..len.saturating_sub(1) {
                    let (u, v) = (self.layers[r][i], self.layers[r][i + 1]);
                    if self.nodes[u].cluster != self.nodes[v].cluster || self.after[u].contains(&v)
                    {
                        continue;
                    }
                    let before = self.pair_crossings(u, v);
                    let after = self.pair_crossings(v, u);
                    if after < before || (equal && after == before && before > 0) {
                        self.layers[r].swap(i, i + 1);
                        self.pos[u] = i + 1;
                        self.pos[v] = i;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }

    /// u が v の左にあるときの、u と v の辺どうしの交差。
    fn pair_crossings(&self, u: usize, v: usize) -> usize {
        let mut c = 0;
        for (lu, lv, upper) in [
            (&self.nodes[u].up, &self.nodes[v].up, true),
            (&self.nodes[u].down, &self.nodes[v].down, false),
        ] {
            for &a in lu {
                let pa = self.end_key(a, upper);
                for &b in lv {
                    let pb = self.end_key(b, upper);
                    if pa > pb {
                        c += 1;
                    }
                }
            }
        }
        c
    }
}

// ---- 全体 ----

/// 配置する。`sets` はランクの組。
pub fn layout(
    input_nodes: &[Node],
    input_edges: &[Edge],
    clusters: &[Cluster],
    sets: &[(RankKind, Vec<usize>)],
    opt: &Options,
) -> Layout {
    let n = input_nodes.len();
    let k = clusters.len();
    // ラベルがあれば、ラベルのランクを間に置けるよう minlen を倍にする
    let labeled = input_edges
        .iter()
        .any(|e| e.label.is_some() && e.tail != e.head);
    let mut redges: Vec<Edge> = input_edges.to_vec();
    if labeled {
        for e in &mut redges {
            e.minlen *= 2.0;
        }
    }
    let cluster_of: Vec<Option<usize>> = input_nodes.iter().map(|v| v.cluster).collect();
    // クラスタの木
    let mut parent: Vec<Option<usize>> = clusters
        .iter()
        .map(|c| c.parent.filter(|&p| p < k))
        .collect();
    // 輪になった親は切る
    for c in 0..k {
        let mut p = parent[c];
        let mut hops = 0;
        while let Some(pi) = p {
            hops += 1;
            if pi == c || hops > k {
                parent[c] = None;
                break;
            }
            p = parent[pi];
        }
    }
    let mut depth = vec![0usize; k];
    for c in 0..k {
        let mut p = parent[c];
        while let Some(pi) = p {
            depth[c] += 1;
            p = parent[pi];
        }
    }
    let ranks = rank(
        n,
        &redges,
        sets,
        &cluster_of,
        &parent,
        &depth,
        opt.cluster_rank,
        opt,
    );
    let lca = |a: Option<usize>, b: Option<usize>| -> Option<usize> {
        let (mut a, mut b) = (a, b);
        let d = |c: Option<usize>| c.map_or(0, |c| depth[c] + 1);
        while d(a) > d(b) {
            a = a.and_then(|c| parent[c]);
        }
        while d(b) > d(a) {
            b = b.and_then(|c| parent[c]);
        }
        while a != b {
            a = a.and_then(|c| parent[c]);
            b = b.and_then(|c| parent[c]);
        }
        a
    };

    let half_sep = opt.nodesep / 2.0;
    let mut nodes: Vec<LNode> = input_nodes
        .iter()
        .enumerate()
        .map(|(i, nd)| LNode {
            rank: ranks[i] as usize,
            cluster: nd.cluster.filter(|&c| c < k),
            left: nd.w / 2.0,
            right: nd.w / 2.0 + nd.extra_right,
            h: nd.h,
            real: true,
            up: vec![],
            down: vec![],
            seq: i,
        })
        .collect();
    let mut segs: Vec<Segment> = Vec::new();
    let mut routes: Vec<Route> = vec![Route::Loop { index: 0 }; input_edges.len()];
    // 辺ごとの、上から下への仮のノードの列（両端を含む）と逆かどうか
    let mut chains: Vec<Option<Chain>> = vec![None; input_edges.len()];
    // 仮のノードが多すぎれば、長い辺から仮のノードなしの直線にする（時間とメモリのため）
    let mut direct = vec![false; input_edges.len()];
    {
        let mut spans: Vec<(usize, usize)> = input_edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.tail < n && e.head < n)
            .map(|(i, e)| (nodes[e.tail].rank.abs_diff(nodes[e.head].rank), i))
            .filter(|(s, _)| *s > 1)
            .collect();
        let mut dummies: usize = spans.iter().map(|(s, _)| s - 1).sum();
        if dummies > MAX_DUMMIES {
            spans.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            for (s, i) in spans {
                if dummies <= MAX_DUMMIES {
                    break;
                }
                direct[i] = true;
                dummies -= s - 1;
            }
        }
    }
    let mut loops = vec![0usize; n];
    let mut flat_pairs: std::collections::HashMap<(usize, usize), usize> = Default::default();
    let mut flat_edges: Vec<(usize, usize)> = Vec::new();
    for (ei, e) in input_edges.iter().enumerate() {
        if e.tail >= n || e.head >= n {
            continue;
        }
        if e.tail == e.head {
            routes[ei] = Route::Loop {
                index: loops[e.tail],
            };
            loops[e.tail] += 1;
            continue;
        }
        let (rt, rh) = (nodes[e.tail].rank, nodes[e.head].rank);
        if rt == rh {
            let key = (e.tail.min(e.head), e.tail.max(e.head));
            let lane = flat_pairs.entry(key).or_insert(0);
            routes[ei] = Route::Flat {
                lane: *lane,
                adjacent: false,
            };
            // 見えない辺は段を取らない（見える 1 本目が、隣どうしなら真っすぐ通る）
            if !e.invisible {
                *lane += 1;
            }
            flat_edges.push((e.tail, e.head));
            continue;
        }
        if direct[ei] {
            let ends = vec![e.tail, e.head];
            chains[ei] = Some((ends, false, None));
            continue;
        }
        let rev = rt > rh;
        let (a, b) = if rev {
            (e.head, e.tail)
        } else {
            (e.tail, e.head)
        };
        let (a_off, b_off) = if rev {
            (e.head_off, e.tail_off)
        } else {
            (e.tail_off, e.head_off)
        };
        let (ra, rb) = (nodes[a].rank, nodes[b].rank);
        let home = lca(nodes[a].cluster, nodes[b].cluster);
        let label_rank = match e.label {
            Some(_) if rb - ra >= 2 => Some(ra + (rb - ra) / 2),
            _ => None,
        };
        let mut chain = vec![a];
        let mut label_node = None;
        for r in ra + 1..rb {
            let v = nodes.len();
            let (left, right, h) = match (label_rank == Some(r), e.label) {
                (true, Some((lw, lh))) => {
                    label_node = Some(v);
                    if opt.label_left {
                        (lw + 6.0, 2.0, lh)
                    } else {
                        (2.0, lw + 6.0, lh)
                    }
                }
                _ => (half_sep / 2.0, half_sep / 2.0, 0.0),
            };
            nodes.push(LNode {
                rank: r,
                cluster: home,
                left,
                right,
                h,
                real: false,
                up: vec![],
                down: vec![],
                seq: n + ei,
            });
            chain.push(v);
        }
        chain.push(b);
        for w in chain.windows(2) {
            let (u, v) = (w[0], w[1]);
            let omega = match (nodes[u].real, nodes[v].real) {
                (true, true) => 1.0,
                (false, false) => 8.0,
                _ => 2.0,
            };
            let s = segs.len();
            segs.push(Segment {
                upper: u,
                lower: v,
                weight: e.weight.clamp(0.0, 1000.0) * omega,
                upper_off: if u == a { a_off } else { 0.0 },
                lower_off: if v == b { b_off } else { 0.0 },
            });
            nodes[u].down.push(s);
            nodes[v].up.push(s);
        }
        chains[ei] = Some((chain, rev, label_node));
    }

    // クラスタのランクの幅と、間の空いたランクの詰め物
    let mut crange: Vec<(usize, usize)> = vec![(usize::MAX, 0); k];
    for v in nodes.iter().take(n) {
        let mut c = v.cluster;
        while let Some(ci) = c {
            let r = v.rank;
            crange[ci].0 = crange[ci].0.min(r);
            crange[ci].1 = crange[ci].1.max(r);
            c = parent[ci];
        }
    }
    let max_rank = nodes.iter().map(|v| v.rank).max().unwrap_or(0);
    {
        let mut present = vec![std::collections::HashSet::<usize>::new(); k];
        for v in &nodes {
            let mut c = v.cluster;
            while let Some(ci) = c {
                present[ci].insert(v.rank);
                c = parent[ci];
            }
        }
        for c in 0..k {
            let (lo, hi) = crange[c];
            if lo == usize::MAX {
                continue;
            }
            for r in lo..=hi {
                if !present[c].contains(&r) {
                    nodes.push(LNode {
                        rank: r,
                        cluster: Some(c),
                        left: 0.0,
                        right: 0.0,
                        h: 0.0,
                        real: false,
                        up: vec![],
                        down: vec![],
                        seq: usize::MAX / 2 + c,
                    });
                }
            }
        }
    }

    // 初めの並び: ランク 0 は宣言の順、下のランクは上の隣の位置の順
    let total = nodes.len();
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    let mut by_rank: Vec<usize> = (0..total).collect();
    by_rank.sort_by_key(|&v| (nodes[v].rank, nodes[v].seq, v));
    for v in by_rank {
        layers[nodes[v].rank].push(v);
    }
    let mut ord = Ordering {
        nodes: &nodes,
        segs: &segs,
        layers,
        pos: vec![0; total],
        parent: &parent,
        global: vec![0.0; k],
        after: vec![Vec::new(); total],
        has_flat: vec![false; max_rank + 1],
    };
    // 輪になる辺は、先に書いた辺を守って外す（Graphviz の文書どおり、輪は深さ優先で切る）
    for &(t, h) in &flat_edges {
        if nodes[t].cluster != nodes[h].cluster || ord.after[t].contains(&h) {
            continue;
        }
        // h から t へ着けば輪になる（辺が多すぎるときは調べず、並べるときにいちばん左から出して切る）
        let mut stack = if flat_edges.len() <= FLAT_CYCLE_EDGES {
            vec![h]
        } else {
            Vec::new()
        };
        let mut seen = std::collections::HashSet::new();
        let mut cycle = false;
        while let Some(v) = stack.pop() {
            if v == t {
                cycle = true;
                break;
            }
            if seen.insert(v) {
                stack.extend(ord.after[v].iter().copied());
            }
        }
        if !cycle {
            ord.after[t].push(h);
            ord.has_flat[nodes[t].rank] = true;
        }
    }
    ord.refresh();
    ord.global_keys();
    let mut val = vec![0f64; total];
    for r in 0..ord.layers.len() {
        let layer = ord.layers[r].clone();
        for &v in &layer {
            val[v] = if r == 0 {
                ord.pos[v] as f64
            } else {
                ord.median(&nodes[v].up, true)
                    .unwrap_or(1e9 + ord.pos[v] as f64)
            };
        }
        ord.sort_layer(r, &val);
    }
    // 上からの初めの並びと、下からの初めの並び（下のランクの中央値）の 2 回。どちらも、中央値で上下に並べ替えて
    // 入れ替え（1 回おきに交差の同じ入れ替えも）、減らなければいちばん良い並びに戻して続け、MINCROSS_STALE 回
    // 続けて減らなければやめる。2 回のうち交差の少ないほう
    let mut best = ord.layers.clone();
    let mut best_count = ord.crossings();
    let transpose = segs.len() <= TRANSPOSE_SEGS;
    for start in 0..2 {
        if best_count == 0 {
            break;
        }
        if start == 1 {
            ord.layers = best.clone();
            ord.refresh();
            ord.global_keys();
            for r in (0..ord.layers.len()).rev() {
                let layer = ord.layers[r].clone();
                for &v in &layer {
                    val[v] = ord
                        .median(&nodes[v].down, false)
                        .unwrap_or(ord.pos[v] as f64);
                }
                ord.sort_layer(r, &val);
            }
        }
        let mut here = ord.crossings();
        let mut here_layers = ord.layers.clone();
        let mut stale = 0;
        for i in 0..opt.sweeps {
            if here == 0 {
                break;
            }
            ord.global_keys();
            ord.sweep((i + start) % 2 == 0, &mut val);
            if transpose {
                ord.transpose(i % 2 == 1);
            }
            let c = ord.crossings();
            if c < here {
                here = c;
                here_layers = ord.layers.clone();
                stale = 0;
            } else {
                stale += 1;
                if stale >= MINCROSS_STALE {
                    break;
                }
                ord.layers = here_layers.clone();
                ord.refresh();
            }
        }
        if here < best_count {
            best_count = here;
            best = here_layers;
        }
    }
    ord.layers = best;
    ord.refresh();
    // 同じランクの辺は尾を左に（並べ替えの中で守っている。念のため）
    for r in 0..ord.layers.len() {
        ord.keep_flat_order(r);
    }
    // ordering=out: 同じ尾の頭を辺の順に
    if opt.ordering_out {
        for v in 0..n {
            let downs: Vec<usize> = nodes[v].down.clone();
            if downs.len() < 2 {
                continue;
            }
            let heads: Vec<usize> = downs.iter().map(|&s| segs[s].lower).collect();
            let c = nodes[heads[0]].cluster;
            if heads.iter().any(|&h| nodes[h].cluster != c) {
                continue;
            }
            let r = nodes[heads[0]].rank;
            let mut slots: Vec<usize> = heads.iter().map(|&h| ord.pos[h]).collect();
            slots.sort_unstable();
            slots.dedup();
            if slots.len() != heads.len() {
                continue;
            }
            for (slot, &h) in slots.iter().zip(&heads) {
                ord.layers[r][*slot] = h;
            }
            for (i, &u) in ord.layers[r].iter().enumerate() {
                ord.pos[u] = i;
            }
        }
    }
    let crossings = ord.crossings();
    let layers = ord.layers.clone();
    let pos = ord.pos.clone();

    // 隣どうしの同じランクの辺には、その間を空ける
    let mut flat_room: std::collections::HashMap<(usize, usize), f64> = Default::default();
    for (ei, e) in input_edges.iter().enumerate() {
        if let Route::Flat { .. } = routes[ei] {
            let (a, b) = (e.tail, e.head);
            if nodes[a].rank == nodes[b].rank && pos[a].abs_diff(pos[b]) == 1 {
                let (l, r) = if pos[a] < pos[b] { (a, b) } else { (b, a) };
                let room = e.label.map_or(0.0, |(w, _)| w + 8.0);
                let entry = flat_room.entry((l, r)).or_insert(0.0);
                *entry = entry.max(room);
            }
        }
    }
    for (ei, e) in input_edges.iter().enumerate() {
        if let Route::Flat { adjacent, .. } = &mut routes[ei] {
            *adjacent =
                nodes[e.tail].rank == nodes[e.head].rank && pos[e.tail].abs_diff(pos[e.head]) == 1;
        }
    }

    // ---- 3. 横の位置 ----
    let xs = place_x(&nodes, &segs, &layers, &parent, clusters, &flat_room, opt);
    let (xs, cl, cr) = xs;

    // ---- 4. 縦の位置 ----
    let rank_count = layers.len();
    let mut rank_h = vec![0f64; rank_count];
    for v in &nodes {
        rank_h[v.rank] = rank_h[v.rank].max(v.h);
    }
    let base_gap = if labeled {
        opt.ranksep / 2.0
    } else {
        opt.ranksep
    };
    // クラスタの境の分: ランクの上（開くクラスタ）と下（閉じるクラスタ）
    let top_label = |c: usize| {
        if clusters[c].side == LabelSide::Top {
            clusters[c].label.1
        } else {
            0.0
        }
    };
    let bottom_label = |c: usize| {
        if clusters[c].side == LabelSide::Bottom {
            clusters[c].label.1
        } else {
            0.0
        }
    };
    // 同じランクの辺のラベルの分をランクの上に空ける（隣どうしの線のラベルはノードの上半分からはみ出る分、
    // 間を越える弧のラベルは弧の上がりとラベルの高さ）
    let mut flat_up = vec![0f64; rank_count];
    for (ei, e) in input_edges.iter().enumerate() {
        if let (Route::Flat { adjacent, .. }, Some((_, lh))) = (&routes[ei], e.label)
            && e.tail != e.head
            && !e.invisible
        {
            let r = nodes[e.tail].rank;
            let room = if *adjacent {
                (lh + 2.0 - rank_h[r] / 2.0).max(0.0)
            } else {
                lh + 20.0
            };
            flat_up[r] = flat_up[r].max(room);
        }
    }
    // 間を越える同じランクの弧（隣でも 2 本目から）は、ランクのすぐ上を、横の範囲が重なるものと段を分けて通る（描く
    // 側は 1 段 8、最初の段は行の上の端から 18 まで）。重なりの深さとラベルの分が、ランクの間の半分に収まらなければ
    // 上に空ける
    {
        let mut spans: Vec<Vec<(usize, usize, f64)>> = vec![Vec::new(); rank_count];
        for (ei, e) in input_edges.iter().enumerate() {
            if let Route::Flat { adjacent, lane } = routes[ei]
                && e.tail != e.head
                && !e.invisible
                && !(adjacent && lane == 0)
            {
                let r = nodes[e.tail].rank;
                let (a, b) = (pos[e.tail], pos[e.head]);
                spans[r].push((a.min(b), a.max(b), e.label.map_or(0.0, |(_, lh)| lh + 4.0)));
            }
        }
        for (r, list) in spans.iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            let mut ev: Vec<(usize, i32)> = Vec::with_capacity(list.len() * 2);
            for &(lo, hi, _) in list {
                ev.push((lo, 1));
                ev.push((hi, -1));
            }
            ev.sort_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)));
            let (mut now, mut depth) = (0i32, 0i32);
            for (_, d) in ev {
                now += d;
                depth = depth.max(now);
            }
            let label = list.iter().map(|l| l.2).fold(0.0, f64::max);
            let need = 18.0 + 8.0 * (depth - 1).max(0) as f64 + label;
            flat_up[r] = flat_up[r].max(need - base_gap / 2.0);
        }
    }
    // 題が横（LR・RL の絵の上下）のクラスタは、題の長さ（配置の縦）より箱が短ければ、両端のランクの外に場所を
    // 足して箱を伸ばす（中のクラスタから。伸ばすとランクの間が開くので、収まるまで数回やり直す）
    let mut stretch = vec![0f64; k];
    let mut rank_y = vec![0f64; rank_count];
    let mut height = 0.0;
    let mut boxes: Vec<Option<[f64; 4]>> = vec![None; k];
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&c| std::cmp::Reverse(depth[c]));
    for _ in 0..4 {
        let mut open_room = vec![0f64; rank_count];
        let mut close_room = vec![0f64; rank_count];
        for c in 0..k {
            let (lo, hi) = crange[c];
            if lo == usize::MAX {
                continue;
            }
            // c から上のクラスタのうち、同じランクで開く（閉じる）ものの分を足す
            let mut room_open = 0.0;
            let mut room_close = 0.0;
            let mut p = Some(c);
            while let Some(pi) = p {
                if crange[pi].0 == lo {
                    room_open += clusters[pi].margin + top_label(pi) + stretch[pi];
                }
                if crange[pi].1 == hi {
                    room_close += clusters[pi].margin + bottom_label(pi) + stretch[pi];
                }
                p = parent[pi];
            }
            open_room[lo] = open_room[lo].max(room_open);
            close_room[hi] = close_room[hi].max(room_close);
        }
        let mut y = open_room.first().copied().unwrap_or(0.0)
            + flat_up.first().copied().unwrap_or(0.0)
            + rank_h.first().copied().unwrap_or(0.0) / 2.0;
        for r in 0..rank_count {
            if r > 0 {
                y += rank_h[r - 1] / 2.0
                    + base_gap
                    + close_room[r - 1]
                    + open_room[r]
                    + flat_up[r]
                    + rank_h[r] / 2.0;
            }
            rank_y[r] = y;
        }
        height = y
            + rank_h.last().copied().unwrap_or(0.0) / 2.0
            + close_room.last().copied().unwrap_or(0.0);

        // クラスタの箱（内から）
        let mut ys: Vec<(f64, f64)> = vec![(f64::MAX, f64::MIN); k];
        for v in &nodes {
            if let Some(c) = v.cluster {
                let cy = rank_y[v.rank];
                ys[c].0 = ys[c].0.min(cy - v.h / 2.0);
                ys[c].1 = ys[c].1.max(cy + v.h / 2.0);
            }
        }
        let mut short = false;
        for &c in &order {
            if crange[c].0 == usize::MAX {
                continue;
            }
            let (t, b) = ys[c];
            let top = t - clusters[c].margin - top_label(c) - stretch[c];
            let bottom = b + clusters[c].margin + bottom_label(c) + stretch[c];
            boxes[c] = Some([cl[c], top, cr[c], bottom]);
            if let Some(p) = parent[c] {
                ys[p].0 = ys[p].0.min(top);
                ys[p].1 = ys[p].1.max(bottom);
            }
            if matches!(clusters[c].side, LabelSide::Left | LabelSide::Right) {
                let lack = clusters[c].label.1 - (bottom - top);
                if lack > 0.5 {
                    stretch[c] += lack / 2.0;
                    short = true;
                }
            }
        }
        if !short {
            break;
        }
    }

    // 道筋
    let mut corridors: Vec<Vec<Portal>> = vec![Vec::new(); input_edges.len()];
    for (ei, chain) in chains.iter().enumerate() {
        let Some((chain, rev, label_node)) = chain else {
            continue;
        };
        let e = &input_edges[ei];
        // 通り道: 仮のノードのランクの行で、左右の隣の間。ラベルの仮のノードでは、線はラベルの側へ出ない
        let mut gates: Vec<Portal> = Vec::new();
        // 同じ行の左右の隣の間（本物のノードは避ける箱と同じく外形から 4、ラベルの仮のノードはラベルの端から 2、ほかの
        // 仮のノードとは真ん中で分ける。並んだ線が同じ角に寄って束にならない）
        let between = |v: usize| -> (Option<usize>, Option<usize>, f64, f64) {
            let r = nodes[v].rank;
            let i = pos[v];
            let side = |u: usize, right: bool| -> f64 {
                let reach = if right { nodes[u].right } else { nodes[u].left };
                if nodes[u].real {
                    reach + 4.0
                } else if nodes[u].h > 0.0 {
                    reach + 2.0
                } else {
                    (xs[v] - xs[u]).abs() / 2.0
                }
            };
            let left = i.checked_sub(1).map(|k| layers[r][k]);
            let right = layers[r].get(i + 1).copied();
            let lo = left.map_or(f64::NEG_INFINITY, |u| xs[u] + side(u, true));
            let hi = right.map_or(f64::INFINITY, |u| xs[u] - side(u, false));
            (left, right, lo.min(xs[v]), hi.max(xs[v]))
        };
        // 両端のノードの行では、行の端から 4 外（ノードを避ける箱の外）で、同じ行の隣の間を通る（ノードの中心はこの
        // 区間の中なので、そこからの線は隣を横切らない）
        let (a, b) = (chain[0], chain[chain.len() - 1]);
        let end_gate = |u: usize, below: bool| -> Portal {
            let r = nodes[u].rank;
            let half = rank_h[r] / 2.0 + 4.0;
            let (_, _, lo, hi) = between(u);
            Portal {
                y: rank_y[r] + if below { half } else { -half },
                lo,
                hi,
            }
        };
        gates.push(end_gate(a, true));
        for &v in &chain[1..chain.len() - 1] {
            let r = nodes[v].rank;
            let (left, right, mut lo, mut hi) = between(v);
            if Some(v) == *label_node {
                if opt.label_left {
                    lo = xs[v] - 1.0;
                } else {
                    hi = xs[v] + 1.0;
                }
            }

            // 関の高さ: 左右の隣の本物のノード（避ける箱と同じく 4 広げる）とラベルの分。どちらもなければ行の中心だけ
            let mut half: f64 = 0.0;
            for u in [left, right].into_iter().flatten() {
                if nodes[u].real {
                    half = half.max(nodes[u].h / 2.0 + 4.0);
                }
            }
            if Some(v) == *label_node {
                half = half.max(nodes[v].h / 2.0);
            }
            let y = rank_y[r];
            gates.push(Portal {
                y: y - half,
                lo,
                hi,
            });
            if half > 0.0 {
                gates.push(Portal {
                    y: y + half,
                    lo,
                    hi,
                });
            }
        }
        gates.push(end_gate(b, false));
        if *rev {
            gates.reverse();
        }
        corridors[ei] = gates;
        let mut pts: Vec<(f64, f64)> = chain
            .iter()
            .map(|&v| (xs[v], rank_y[nodes[v].rank]))
            .collect();
        let last = pts.len() - 1;
        let (a_off, b_off) = if *rev {
            (e.head_off, e.tail_off)
        } else {
            (e.tail_off, e.head_off)
        };
        pts[0].0 += a_off;
        pts[last].0 += b_off;
        let label = match (label_node, e.label) {
            (Some(v), Some((lw, _))) => {
                let side = if opt.label_left { -1.0 } else { 1.0 };
                Some((xs[*v] + side * (6.0 + lw / 2.0), rank_y[nodes[*v].rank]))
            }
            (None, Some(_)) => {
                let (p, q) = (pts[0], pts[last]);
                Some(((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0))
            }
            _ => None,
        };
        if *rev {
            pts.reverse();
        }
        routes[ei] = Route::Path { points: pts, label };
    }

    let mut width = 0f64;
    for (i, v) in nodes.iter().enumerate() {
        width = width.max(xs[i] + v.right);
    }
    for b in boxes.iter().flatten() {
        width = width.max(b[2]);
        height = height.max(b[3]);
    }
    for r in &routes {
        if let Route::Path {
            label: Some((x, _)),
            ..
        } = r
        {
            width = width.max(*x);
        }
    }
    Layout {
        nodes: (0..n).map(|v| (xs[v], rank_y[nodes[v].rank])).collect(),
        ranks: (0..n).map(|v| nodes[v].rank as i32).collect(),
        routes,
        clusters: boxes,
        width,
        height,
        crossings,
        corridors,
        straightened: direct.iter().any(|&d| d),
    }
}

/// 直角の線（`splines=ortho`）の段の場所をランクの間に空ける。ランクの間を横に動く線（仮のノードの間の横のずれ）の
/// ノードの中心の広がり（横、縦）。`stretch` で伸びるのはこの分だけ。
pub fn center_span(lay: &Layout) -> (f64, f64) {
    let span = |f: fn(&(f64, f64)) -> f64| {
        let lo = lay.nodes.iter().map(f).fold(f64::INFINITY, f64::min);
        let hi = lay.nodes.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
        if hi > lo { hi - lo } else { 0.0 }
    };
    (span(|p| p.0), span(|p| p.1))
}

/// ノードの大きさはそのままに、位置を横に `sx`・縦に `sy` 倍する（`ratio` の伸ばし。どちらも 1 以上）。いちばん
/// 左上のノードの中心を起点に、ノード・道筋・ラベル・クラスタの箱・通り道を伸ばす。
pub fn stretch(lay: &mut Layout, sx: f64, sy: f64) {
    let sx = if sx.is_finite() { sx.max(1.0) } else { 1.0 };
    let sy = if sy.is_finite() { sy.max(1.0) } else { 1.0 };
    if (sx == 1.0 && sy == 1.0) || lay.nodes.is_empty() {
        return;
    }
    let (span_x, span_y) = center_span(lay);
    let ox = lay.nodes.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let oy = lay.nodes.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    // 起点より先は伸ばし、手前（起点の左・上の縁）はそのまま
    let mx = |v: f64| if v > ox { ox + (v - ox) * sx } else { v };
    let my = |v: f64| if v > oy { oy + (v - oy) * sy } else { v };
    let f = |p: &mut (f64, f64)| *p = (mx(p.0), my(p.1));
    lay.nodes.iter_mut().for_each(f);
    for r in &mut lay.routes {
        if let Route::Path { points, label } = r {
            points.iter_mut().for_each(f);
            if let Some(l) = label {
                f(l);
            }
        }
    }
    // クラスタの箱の縁も同じく（中のノードとの間は伸びるだけで、ノードを囲んだまま）
    let mut right = lay.width + span_x * (sx - 1.0);
    let mut bottom = lay.height + span_y * (sy - 1.0);
    for b in lay.clusters.iter_mut().flatten() {
        *b = [mx(b[0]), my(b[1]), mx(b[2]), my(b[3])];
        right = right.max(b[2]);
        bottom = bottom.max(b[3]);
    }
    for gates in &mut lay.corridors {
        for g in gates {
            g.y = my(g.y);
            g.lo = mx(g.lo);
            g.hi = mx(g.hi);
        }
    }
    lay.width = right;
    lay.height = bottom;
}

/// 重なりの最大から段の数を見積もり、`track` の間で並べて両側に `margin`（矢じりの分）を残せるだけ、下のランクを
/// 下げる（ノード・道筋・ラベル・クラスタの箱の y と高さ）。1 つの間で下げるのは `most_add` まで（込んだ図が縦に
/// 伸びすぎないように。足りない分は段の間を詰める）。ただし段の間が `MIN_TRACK` を切る分は、上限を超えても下げる。`heights` はノードの高さ（配置の向き）。
pub fn make_room_for_tracks(
    lay: &mut Layout,
    heights: &[f64],
    track: f64,
    margin: f64,
    most_add: f64,
) {
    let key = |y: f64| (y * 64.0).round() as i64;
    // ランクの行（中心の y → 上下の端）
    let mut rows: Vec<(f64, f64, f64)> = Vec::new();
    for (i, &(_, y)) in lay.nodes.iter().enumerate() {
        let h = heights.get(i).copied().unwrap_or(0.0) / 2.0;
        match rows.iter_mut().find(|r| key(r.0) == key(y)) {
            Some(r) => {
                r.1 = r.1.min(y - h);
                r.2 = r.2.max(y + h);
            }
            None => rows.push((y, y - h, y + h)),
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    if rows.len() < 2 {
        return;
    }
    let row_of = |y: f64| rows.partition_point(|r| r.0 < y - 1e-6);
    // ランクの間ごとの横の線の区間
    let mut spans: Vec<Vec<(f64, f64)>> = vec![Vec::new(); rows.len()];
    for r in &lay.routes {
        if let Route::Path { points, .. } = r {
            for w in points.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.0 - b.0).abs() < 1e-6 || (a.1 - b.1).abs() < 1e-6 {
                    continue;
                }
                let upper = row_of(a.1.min(b.1));
                if upper + 1 < rows.len() && row_of(a.1.max(b.1)) == upper + 1 {
                    spans[upper].push((a.0.min(b.0), a.0.max(b.0)));
                }
            }
        }
    }
    // 下げる分（ランクの間ごと）
    let mut add = vec![0f64; rows.len()];
    for (g, list) in spans.iter_mut().enumerate() {
        if list.len() < 2 || g + 1 >= rows.len() {
            continue;
        }
        // 区間の重なりの最大（端の点を並べて数える）
        let mut ev: Vec<(f64, i32)> = Vec::with_capacity(list.len() * 2);
        for &(x0, x1) in list.iter() {
            ev.push((x0 - 1.0, 1));
            ev.push((x1 + 1.0, -1));
        }
        ev.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
        let (mut now, mut most) = (0i32, 0i32);
        for (_, d) in ev {
            now += d;
            most = most.max(now);
        }
        let gap = rows[g + 1].1 - rows[g].2;
        let need = 2.0 * margin + (most.max(1) - 1) as f64 * track;
        // 上限を超えても、段の間は MIN_TRACK は保つ（それより詰めると違う辺の線が 1 本に見える）
        let least = 2.0 * margin + (most.max(1) - 1) as f64 * MIN_TRACK;
        add[g] = (need - gap).clamp(0.0, most_add).max(least - gap);
    }
    if add.iter().all(|&a| a <= 0.0) {
        return;
    }
    // y を下げる: 行 i とその上の空きのものは add[..i] の和だけ（空きのものは上の行と、クラスタの箱の上の端は下の行と
    // 動く）
    let mut cum = vec![0f64; rows.len() + 1];
    for i in 0..rows.len() {
        cum[i + 1] = cum[i] + add[i];
    }
    let with_upper = |y: f64| -> f64 {
        let i = rows.partition_point(|r| r.0 <= y + 1e-6);
        y + cum[i.saturating_sub(1)]
    };
    let with_lower = |y: f64| -> f64 {
        let i = rows.partition_point(|r| r.0 < y - 1e-6);
        y + cum[i.min(rows.len() - 1)]
    };
    for p in &mut lay.nodes {
        p.1 = with_upper(p.1);
    }
    for r in &mut lay.routes {
        if let Route::Path { points, label } = r {
            for p in points.iter_mut() {
                p.1 = with_upper(p.1);
            }
            if let Some(l) = label {
                l.1 = with_upper(l.1);
            }
        }
    }
    for b in lay.clusters.iter_mut().flatten() {
        b[1] = with_lower(b[1]);
        b[3] = with_upper(b[3]);
    }
    for gates in &mut lay.corridors {
        for g in gates.iter_mut() {
            g.y = with_upper(g.y);
        }
    }
    lay.height += cum[rows.len()];
}

/// 横の位置の釣り合い。単体法の答えは、同じ費用で動ける幅があっても、その端に寄る（2 つの子を持つ親が
/// 片方の真上に来る）。本物のノード 1 つ、まっすぐにつながったノード（長さ 0 の辺の両端）の塊、その塊のうち
/// あるノードから上（下）の部分を、外への辺の長さの和が最も小さく、隣・クラスタの境の制約も守れる幅の
/// 真ん中に動かす（底の外にいれば底へ）。辺が仮のノードを通るなら、その先の本物のノードを的にし、終わりに
/// 仮のノードを両端の間にまっすぐ並べ直す（並べられず、辺が長くなる動きはしない）。上から下、下から上を 1 回ずつ。
fn balance_x(
    x: &mut [i64],
    real: &[bool],
    layers: &[Vec<usize>],
    edges: &[NsEdge],
    segs: &[Segment],
    first_seg: usize,
) {
    let total = real.len();
    let off = |s: &Segment| (s.upper_off.round() as i64, s.lower_off.round() as i64);
    let segs: Vec<&Segment> = segs
        .iter()
        .filter(|s| s.weight > 0.0 && s.upper != s.lower && s.upper < total && s.lower < total)
        .collect();
    // 制約の辺（重みのないもの）
    let cons: Vec<&NsEdge> = edges
        .iter()
        .filter(|e| {
            e.weight == 0.0
                && e.tail < first_seg
                && e.head < first_seg
                && (e.tail < total || e.head < total)
        })
        .collect();
    let down: Vec<usize> = layers.iter().flatten().copied().collect();
    let mut touch: Vec<Vec<usize>> = vec![Vec::new(); total];
    for (i, s) in segs.iter().enumerate() {
        touch[s.upper].push(i);
        touch[s.lower].push(i);
    }
    let mut cons_of: Vec<Vec<usize>> = vec![Vec::new(); total];
    for (i, e) in cons.iter().enumerate() {
        if e.tail < total {
            cons_of[e.tail].push(i);
        }
        if e.head < total {
            cons_of[e.head].push(i);
        }
    }
    // 辺の先: 自分の側の差、相手、相手の側の差
    let far = |m: usize, i: usize| -> (usize, i64, i64) {
        let s = segs[i];
        let (a, b) = off(s);
        if s.upper == m {
            (s.lower, a, b)
        } else {
            (s.upper, b, a)
        }
    };
    // 仮のノードの続き（上下に 1 本ずつ）をたどった先の本物のノードと、その側の差。たどった仮のノードは `via` に
    let follow = |m: usize, i: usize, via: &mut Vec<usize>| -> (usize, i64, i64) {
        let (mut o, mine, mut theirs) = far(m, i);
        let mut came = i;
        via.clear();
        while !real[o] && touch[o].len() == 2 && via.len() < total {
            via.push(o);
            let next = if touch[o][0] == came {
                touch[o][1]
            } else {
                touch[o][0]
            };
            let (n, _, t) = far(o, next);
            came = next;
            o = n;
            theirs = t;
        }
        (o, mine, theirs)
    };
    // 組を動かす制約の幅
    let bounds = |set: &[usize], x: &[i64], mark: &[usize], st: usize| -> (i64, i64) {
        let inside = |u: usize| u < total && mark[u] == st;
        let (mut flo, mut fhi) = (i64::MIN, i64::MAX);
        for &m in set {
            for &i in &cons_of[m] {
                let e = cons[i];
                if e.head == m && !inside(e.tail) {
                    flo = flo.max(x[e.tail] + e.minlen - x[m]);
                }
                if e.tail == m && !inside(e.head) {
                    fhi = fhi.min(x[e.head] - e.minlen - x[m]);
                }
            }
        }
        (flo, fhi)
    };
    let mut mark = vec![0usize; total];
    let mut stamp = 0usize;
    // 本物の m から辺 i の先の仮のノードを、両端の間のまっすぐな線へ寄せる（制約の中で、2 本の辺が長くならない
    // ときだけ）。動かした仮のノードと前の位置を `moved` に足す
    let straighten = |m: usize,
                      i: usize,
                      x: &mut [i64],
                      mark: &mut [usize],
                      stamp: &mut usize,
                      chain: &mut Vec<usize>,
                      moved: &mut Vec<(usize, i64)>| {
        let (r, mine, rt) = follow(m, i, chain);
        if !real[r] || chain.is_empty() {
            return;
        }
        let (from, to) = (x[m] + mine, x[r] + rt);
        let n = chain.len() as i64 + 1;
        for (k, &v) in chain.iter().enumerate() {
            let want = from + (to - from) * (k as i64 + 1) / n;
            *stamp += 1;
            mark[v] = *stamp;
            let (flo, fhi) = bounds(&[v], x, mark, *stamp);
            let d = (want - x[v]).clamp(flo.min(0), fhi.max(0));
            if d == 0 {
                continue;
            }
            let cost = |x: &[i64], at: i64| -> f64 {
                touch[v]
                    .iter()
                    .map(|&j| {
                        let (o, mine, theirs) = far(v, j);
                        segs[j].weight * (at + mine - x[o] - theirs).abs() as f64
                    })
                    .sum()
            };
            if cost(x, x[v] + d) <= cost(x, x[v]) + 1e-9 {
                moved.push((v, x[v]));
                x[v] += d;
            }
        }
    };
    let seg_cost = |x: &[i64], list: &[usize]| -> f64 {
        list.iter()
            .map(|&j| {
                let s = segs[j];
                let (a, b) = off(s);
                s.weight * (x[s.upper] + a - x[s.lower] - b).abs() as f64
            })
            .sum()
    };
    let mut ts: Vec<(i64, f64)> = Vec::new();
    let mut via: Vec<usize> = Vec::new();
    let mut chain: Vec<usize> = Vec::new();
    let mut moved: Vec<(usize, i64)> = Vec::new();
    let mut costed: Vec<usize> = Vec::new();
    let mut seg_tag = vec![0usize; segs.len()];
    let mut shift =
        |set: &[usize], deep: bool, x: &mut [i64], mark: &mut [usize], stamp: &mut usize| -> bool {
            *stamp += 1;
            let st = *stamp;
            for &m in set {
                mark[m] = st;
            }
            ts.clear();
            costed.clear();
            for &m in set {
                for &i in &touch[m] {
                    let (o, mine, theirs) = if deep {
                        follow(m, i, &mut via)
                    } else {
                        via.clear();
                        far(m, i)
                    };
                    if mark[o] == st || via.iter().any(|&v| mark[v] == st) {
                        continue;
                    }
                    ts.push((x[o] + theirs - mine - x[m], segs[i].weight));
                    // 費用を比べる辺: m の辺と、その先の仮のノードの辺
                    for &u in std::iter::once(&m).chain(via.iter()) {
                        for &j in &touch[u] {
                            if seg_tag[j] != st {
                                seg_tag[j] = st;
                                costed.push(j);
                            }
                        }
                    }
                }
            }
            if ts.is_empty() {
                return false;
            }
            ts.sort_by_key(|t| t.0);
            let half: f64 = ts.iter().map(|t| t.1).sum::<f64>() / 2.0;
            let mut acc = 0.0;
            let (mut lo, mut hi) = (ts[0].0, ts[0].0);
            for (k, &(t, w)) in ts.iter().enumerate() {
                acc += w;
                if acc >= half - 1e-9 {
                    lo = t;
                    hi = if (acc - half).abs() <= 1e-9 {
                        ts.get(k + 1).map_or(t, |n| n.0)
                    } else {
                        t
                    };
                    break;
                }
            }
            let (flo, fhi) = bounds(set, x, mark, st);
            if flo > 0 || fhi < 0 {
                return false;
            }
            let d = if hi < flo {
                flo
            } else if lo > fhi {
                fhi
            } else {
                let (l, h) = (lo.max(flo), hi.min(fhi));
                l + (h - l) / 2
            };
            if d == 0 {
                return false;
            }
            let before = seg_cost(x, &costed);
            moved.clear();
            for &m in set {
                moved.push((m, x[m]));
                x[m] += d;
            }
            if deep {
                for &m in set {
                    for &i in &touch[m] {
                        straighten(m, i, x, mark, stamp, &mut chain, &mut moved);
                    }
                }
            }
            // 仮のノードがまっすぐに並べず、辺が長くなるなら戻す
            if seg_cost(x, &costed) > before + 1e-9 {
                for &(v, old) in moved.iter().rev() {
                    x[v] = old;
                }
                return false;
            }
            true
        };
    let mut set: Vec<usize> = Vec::new();
    let mut inset = vec![0usize; total];
    let mut tag = 0usize;
    // 隣を的にして上から下・下から上、仮のノードの先を的にして上から下・下から上
    for pass in 0..4 {
        let deep = pass >= 2;
        let order: Vec<usize> = if pass % 2 == 0 {
            down.clone()
        } else {
            down.iter().rev().copied().collect()
        };
        for v in order {
            // 1 つ
            if deep && !real[v] {
                continue;
            }
            if shift(&[v], deep, x, &mut mark, &mut stamp) {
                continue;
            }
            // v から上、v から下、つながり全体（長さ 0 の辺をたどる）
            for dir in 0..3 {
                set.clear();
                set.push(v);
                tag += 1;
                inset[v] = tag;
                let mut k = 0;
                while k < set.len() {
                    let u = set[k];
                    k += 1;
                    for &i in &touch[u] {
                        let s = segs[i];
                        let (a, b) = off(s);
                        if x[s.upper] + a != x[s.lower] + b {
                            continue;
                        }
                        let o = match dir {
                            0 if s.lower == u => s.upper,
                            1 if s.upper == u => s.lower,
                            2 => {
                                if s.upper == u {
                                    s.lower
                                } else {
                                    s.upper
                                }
                            }
                            _ => continue,
                        };
                        if inset[o] != tag {
                            inset[o] = tag;
                            set.push(o);
                        }
                    }
                }
                if set.len() > 1 && shift(&set, deep, x, &mut mark, &mut stamp) {
                    break;
                }
            }
        }
    }
    // 残りの仮のノードも、両端の間にまっすぐ
    let mut chain: Vec<usize> = Vec::new();
    let mut moved: Vec<(usize, i64)> = Vec::new();
    for &u in &down {
        if !real[u] {
            continue;
        }
        for &i in &touch[u] {
            if segs[i].upper == u {
                straighten(u, i, x, &mut mark, &mut stamp, &mut chain, &mut moved);
            }
        }
    }
}

/// 横の位置（補助グラフのネットワーク単体法）。ノードの x と、クラスタの左右の境を返す。
fn place_x(
    nodes: &[LNode],
    segs: &[Segment],
    layers: &[Vec<usize>],
    parent: &[Option<usize>],
    clusters: &[Cluster],
    flat_room: &std::collections::HashMap<(usize, usize), f64>,
    opt: &Options,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let total = nodes.len();
    let k = clusters.len();
    // 補助のノード: ノード、クラスタの左右、辺、根
    let ln = |c: usize| total + c;
    let rn = |c: usize| total + k + c;
    let seg_node = |s: usize| total + 2 * k + s;
    let root = total + 2 * k + segs.len();
    let count = root + 1;
    let side = |c: usize| -> f64 {
        match clusters[c].side {
            LabelSide::Left => clusters[c].label.0,
            _ => 0.0,
        }
    };
    let right_side = |c: usize| -> f64 {
        match clusters[c].side {
            LabelSide::Right => clusters[c].label.0,
            _ => 0.0,
        }
    };
    let ceil = |v: f64| -> i64 {
        if v.is_finite() {
            v.ceil().clamp(-1e9, 1e9) as i64
        } else {
            0
        }
    };
    let used: Vec<bool> = {
        let mut u = vec![false; k];
        for v in nodes {
            let mut c = v.cluster;
            while let Some(ci) = c {
                u[ci] = true;
                c = parent[ci];
            }
        }
        u
    };
    let sep_of = |u: usize, v: usize| -> f64 {
        if nodes[u].real && nodes[v].real {
            opt.nodesep
        } else {
            opt.nodesep / 2.0
        }
    };
    let build = |with_clusters: bool| -> Vec<NsEdge> {
        let mut e: Vec<NsEdge> = Vec::new();
        let chain = |c: Option<usize>| -> Vec<usize> {
            let mut out = Vec::new();
            let mut p = c;
            while let Some(pi) = p {
                out.push(pi);
                p = parent[pi];
                if out.len() > 1000 {
                    break;
                }
            }
            out.reverse();
            out
        };
        for layer in layers {
            for w in layer.windows(2) {
                let (u, v) = (w[0], w[1]);
                let room = flat_room.get(&(u, v)).copied().unwrap_or(0.0);
                e.push(NsEdge {
                    tail: u,
                    head: v,
                    minlen: ceil(nodes[u].right + nodes[v].left + sep_of(u, v) + room),
                    weight: 0.0,
                });
                if with_clusters {
                    let cu = chain(nodes[u].cluster);
                    let cv = chain(nodes[v].cluster);
                    let common = cu.iter().zip(&cv).take_while(|(a, b)| a == b).count();
                    let a = cu.get(common).copied();
                    let b = cv.get(common).copied();
                    match (a, b) {
                        (Some(a), Some(b)) => e.push(NsEdge {
                            tail: rn(a),
                            head: ln(b),
                            minlen: ceil(opt.nodesep),
                            weight: 0.0,
                        }),
                        (Some(a), None) => e.push(NsEdge {
                            tail: rn(a),
                            head: v,
                            minlen: ceil(nodes[v].left + opt.nodesep / 2.0),
                            weight: 0.0,
                        }),
                        (None, Some(b)) => e.push(NsEdge {
                            tail: u,
                            head: ln(b),
                            minlen: ceil(nodes[u].right + opt.nodesep / 2.0),
                            weight: 0.0,
                        }),
                        (None, None) => {}
                    }
                }
            }
            if let Some(&first) = layer.first() {
                e.push(NsEdge {
                    tail: root,
                    head: first,
                    minlen: 0,
                    weight: 0.0,
                });
            }
        }
        if with_clusters {
            for (v, nd) in nodes.iter().enumerate() {
                if let Some(c) = nd.cluster {
                    e.push(NsEdge {
                        tail: ln(c),
                        head: v,
                        minlen: ceil(nd.left + clusters[c].margin + side(c)),
                        weight: 0.0,
                    });
                    e.push(NsEdge {
                        tail: v,
                        head: rn(c),
                        minlen: ceil(nd.right + clusters[c].margin + right_side(c)),
                        weight: 0.0,
                    });
                }
            }
            for c in 0..k {
                if !used[c] {
                    continue;
                }
                let inner = match clusters[c].side {
                    LabelSide::Top | LabelSide::Bottom => {
                        clusters[c].label.0 + 2.0 * clusters[c].margin
                    }
                    _ => 0.0,
                };
                e.push(NsEdge {
                    tail: ln(c),
                    head: rn(c),
                    minlen: ceil(inner),
                    weight: 1.0,
                });
                e.push(NsEdge {
                    tail: root,
                    head: ln(c),
                    minlen: 0,
                    weight: 0.0,
                });
                if let Some(p) = parent[c] {
                    e.push(NsEdge {
                        tail: ln(p),
                        head: ln(c),
                        minlen: ceil(clusters[p].margin + side(p)),
                        weight: 0.0,
                    });
                    e.push(NsEdge {
                        tail: rn(c),
                        head: rn(p),
                        minlen: ceil(clusters[p].margin + right_side(p)),
                        weight: 0.0,
                    });
                }
            }
        }
        for (s, seg) in segs.iter().enumerate() {
            // |(x_u + a) - (x_v + b)| を小さく: c = max(a, b)
            let a = seg.upper_off.round() as i64;
            let b = seg.lower_off.round() as i64;
            let c = a.max(b);
            e.push(NsEdge {
                tail: seg_node(s),
                head: seg.upper,
                minlen: c - a,
                weight: seg.weight,
            });
            e.push(NsEdge {
                tail: seg_node(s),
                head: seg.lower,
                minlen: c - b,
                weight: seg.weight,
            });
        }
        e
    };
    let longest = |edges: &[NsEdge]| -> Option<Vec<i64>> {
        let mut x = vec![0i64; count];
        let mut indeg = vec![0usize; count];
        let mut outs: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (i, e) in edges.iter().enumerate() {
            indeg[e.head] += 1;
            outs[e.tail].push(i);
        }
        let mut queue: Vec<usize> = (0..count).filter(|&v| indeg[v] == 0).collect();
        let mut q = 0;
        while q < queue.len() {
            let v = queue[q];
            q += 1;
            for &ei in &outs[v] {
                let e = &edges[ei];
                x[e.head] = x[e.head].max(x[v] + e.minlen);
                indeg[e.head] -= 1;
                if indeg[e.head] == 0 {
                    queue.push(e.head);
                }
            }
        }
        (queue.len() == count).then_some(x)
    };
    let mut edges = build(true);
    let mut x = match longest(&edges) {
        Some(x) => x,
        None => {
            // クラスタの左右がランクで食い違うなら、クラスタの境をあきらめる
            edges = build(false);
            longest(&edges).unwrap_or_else(|| vec![0; count])
        }
    };
    // 辺のノードは、両端の左のほうに（初めの値をきつく）
    for (s, seg) in segs.iter().enumerate() {
        let a = seg.upper_off.round() as i64;
        let b = seg.lower_off.round() as i64;
        let c = a.max(b);
        x[seg_node(s)] = (x[seg.upper] - (c - a)).min(x[seg.lower] - (c - b));
    }
    simplex::solve(count, &edges, &mut x, opt.x_iter);
    let real: Vec<bool> = nodes.iter().map(|n| n.real).collect();
    balance_x(&mut x, &real, layers, &edges, segs, seg_node(0));
    let mut min_x = f64::MAX;
    for (v, nd) in nodes.iter().enumerate() {
        min_x = min_x.min(x[v] as f64 - nd.left);
    }
    let mut cl = vec![0f64; k];
    let mut cr = vec![0f64; k];
    for c in 0..k {
        if used[c] && edges.iter().any(|e| e.head == ln(c)) {
            min_x = min_x.min(x[ln(c)] as f64);
        }
    }
    if min_x == f64::MAX {
        min_x = 0.0;
    }
    let xs: Vec<f64> = (0..total).map(|v| x[v] as f64 - min_x).collect();
    let clustered = edges
        .iter()
        .any(|e| (total..total + 2 * k).contains(&e.head));
    for c in 0..k {
        if clustered {
            cl[c] = x[ln(c)] as f64 - min_x;
            cr[c] = x[rn(c)] as f64 - min_x;
        } else {
            // クラスタの境をあきらめたときは、中のノードの外側
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for (v, nd) in nodes.iter().enumerate() {
                let mut p = nd.cluster;
                while let Some(pi) = p {
                    if pi == c {
                        lo = lo.min(xs[v] - nd.left - clusters[c].margin);
                        hi = hi.max(xs[v] + nd.right + clusters[c].margin);
                        break;
                    }
                    p = parent[pi];
                }
            }
            cl[c] = if lo == f64::MAX { 0.0 } else { lo };
            cr[c] = if hi == f64::MIN { 0.0 } else { hi };
        }
    }
    (xs, cl, cr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options {
            nodesep: 18.0,
            ranksep: 36.0,
            ordering_out: false,
            label_left: false,
            rank_iter: 1000,
            x_iter: 1000,
            sweeps: 24,
            cluster_rank: true,
        }
    }

    fn node(c: Option<usize>) -> Node {
        Node {
            w: 54.0,
            h: 36.0,
            cluster: c,
            extra_right: 0.0,
        }
    }

    fn edge(t: usize, h: usize) -> Edge {
        Edge {
            tail: t,
            head: h,
            minlen: 1.0,
            weight: 1.0,
            constraint: true,
            label: None,
            tail_off: 0.0,
            head_off: 0.0,
            invisible: false,
        }
    }

    #[test]
    fn ranks_chains_and_same_sets() {
        let nodes = vec![node(None); 4];
        let edges = vec![edge(0, 1), edge(1, 2), edge(0, 3)];
        let l = layout(
            &nodes,
            &edges,
            &[],
            &[(RankKind::Same, vec![2, 3])],
            &opts(),
        );
        assert_eq!(l.ranks, vec![0, 1, 2, 2]);
        assert!(matches!(l.routes[2], Route::Path { ref points, .. } if points.len() == 3));
        // rank=min・max
        let l = layout(
            &nodes,
            &[edge(0, 1)],
            &[],
            &[(RankKind::Max, vec![2]), (RankKind::Source, vec![3])],
            &opts(),
        );
        assert_eq!(l.ranks[3], 0);
        assert!(l.ranks[0] >= 1);
        assert!(l.ranks[2] >= l.ranks[1]);
    }

    #[test]
    fn breaks_cycles_and_keeps_flat_edges() {
        let nodes = vec![node(None); 3];
        let mut back = edge(2, 0);
        back.constraint = true;
        let mut flat = edge(1, 0);
        flat.constraint = false;
        let l = layout(
            &nodes,
            &[edge(0, 1), edge(1, 2), back, flat],
            &[],
            &[(RankKind::Same, vec![0, 2])],
            &opts(),
        );
        assert_eq!(l.ranks[0], l.ranks[2]);
        assert!(matches!(l.routes[2], Route::Flat { .. }));
        assert!(matches!(l.routes[3], Route::Path { .. }));
    }

    #[test]
    fn keeps_nodes_apart_and_inside_clusters() {
        let nodes = vec![node(Some(0)), node(Some(0)), node(None), node(Some(1))];
        let clusters = vec![
            Cluster {
                parent: None,
                margin: 8.0,
                label: (30.0, 14.0),
                side: LabelSide::Top,
            },
            Cluster {
                parent: None,
                margin: 8.0,
                label: (0.0, 0.0),
                side: LabelSide::Top,
            },
        ];
        let edges = vec![edge(0, 1), edge(2, 1), edge(3, 1), edge(2, 3)];
        let l = layout(&nodes, &edges, &clusters, &[], &opts());
        for r in 0..=2 {
            let mut row: Vec<(f64, usize)> = (0..4)
                .filter(|&v| l.ranks[v] == r)
                .map(|v| (l.nodes[v].0, v))
                .collect();
            row.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            for w in row.windows(2) {
                assert!(w[1].0 - w[0].0 >= 54.0 + 18.0 - 0.5, "{row:?}");
            }
        }
        let b = l.clusters[0].unwrap();
        for v in [0, 1] {
            let (x, y) = l.nodes[v];
            assert!(
                x - 27.0 >= b[0] && x + 27.0 <= b[2] && y - 18.0 >= b[1] && y + 18.0 <= b[3],
                "{b:?} {:?}",
                l.nodes[v]
            );
        }
        // クラスタの外のノードは箱の外
        let (x, y) = l.nodes[2];
        assert!(!(x > b[0] && x < b[2] && y > b[1] && y < b[3]));
        assert_eq!(l.crossings, 0);
    }

    #[test]
    fn ranks_clusters_on_their_own() {
        // クラスタの中でつながらないノードは、外の辺に引かれずにクラスタの上のランクに並ぶ
        let nodes = vec![node(Some(0)), node(Some(0)), node(None), node(None)];
        let clusters = vec![Cluster {
            parent: None,
            margin: 8.0,
            label: (0.0, 0.0),
            side: LabelSide::Top,
        }];
        let edges = vec![edge(0, 2), edge(2, 3), edge(1, 3)];
        let l = layout(&nodes, &edges, &clusters, &[], &opts());
        assert_eq!(l.ranks, [0, 0, 1, 2]);
        let global = Options {
            cluster_rank: false,
            ..opts()
        };
        let l = layout(&nodes, &edges, &clusters, &[], &global);
        assert_eq!(l.ranks, [0, 1, 1, 2]);
    }

    #[test]
    fn ranks_cycles_between_clusters_by_length() {
        let cluster = || Cluster {
            parent: None,
            margin: 8.0,
            label: (0.0, 0.0),
            side: LabelSide::Top,
        };
        let clusters = vec![cluster(), cluster()];
        let weighted = |t: usize, h: usize, w: f64| Edge {
            weight: w,
            ..edge(t, h)
        };
        // A = {0 → 1}, B = {2 → 3}, 0 → 3 and 2 → 1: the cycle fits side by side
        let nodes = vec![node(Some(0)), node(Some(0)), node(Some(1)), node(Some(1))];
        let edges = vec![edge(0, 1), edge(2, 3), edge(0, 3), edge(2, 1)];
        let l = layout(&nodes, &edges, &clusters, &[], &opts());
        assert_eq!(l.ranks, [0, 1, 0, 1]);
        // A = {0 → 1 → 2}, B = {3}: without a cycle the edges between clusters point down
        let nodes = vec![
            node(Some(0)),
            node(Some(0)),
            node(Some(0)),
            node(Some(1)),
            node(Some(1)),
        ];
        let edges = vec![edge(0, 1), edge(1, 2), edge(3, 4), edge(2, 3), edge(0, 4)];
        let l = layout(&nodes, &edges, &clusters, &[], &opts());
        assert_eq!(l.ranks, [0, 1, 2, 3, 4]);
        // a cycle that doesn't fit: the lighter edge turns up
        let nodes = vec![node(Some(0)), node(Some(0)), node(Some(0)), node(Some(1))];
        let chain = [edge(0, 1), edge(1, 2)];
        let up = [
            chain[0].clone(),
            chain[1].clone(),
            weighted(2, 3, 1.0),
            weighted(3, 0, 5.0),
        ];
        let l = layout(&nodes, &up, &clusters, &[], &opts());
        assert_eq!(l.ranks, [1, 2, 3, 0]);
        let down = [
            chain[0].clone(),
            chain[1].clone(),
            weighted(2, 3, 5.0),
            weighted(3, 0, 1.0),
        ];
        let l = layout(&nodes, &down, &clusters, &[], &opts());
        assert_eq!(l.ranks, [0, 1, 2, 3]);
        // equal weights: the cluster written first stays on top
        let tie = [chain[0].clone(), chain[1].clone(), edge(2, 3), edge(3, 0)];
        let l = layout(&nodes, &tie, &clusters, &[], &opts());
        assert_eq!(l.ranks, [0, 1, 2, 3]);
        // A and B in a cycle that fits; x → a1, and b1 → x (heavier) pulls x below b1: x → a1 turns up
        let nodes = vec![
            node(Some(0)),
            node(Some(0)),
            node(Some(1)),
            node(Some(1)),
            node(None),
        ];
        let edges = vec![
            edge(0, 1),
            edge(2, 3),
            edge(0, 3),
            edge(2, 1),
            edge(4, 0),
            weighted(2, 4, 3.0),
        ];
        let l = layout(&nodes, &edges, &clusters, &[], &opts());
        assert_eq!(l.ranks, [0, 1, 0, 1, 1]);
    }

    #[test]
    fn straightens_simple_chains() {
        let nodes = vec![node(None); 3];
        let l = layout(&nodes, &[edge(0, 1), edge(1, 2)], &[], &[], &opts());
        assert_eq!(l.nodes[0].0, l.nodes[1].0);
        assert_eq!(l.nodes[1].0, l.nodes[2].0);
        assert!(l.nodes[1].1 - l.nodes[0].1 >= 36.0 + 36.0);
    }
}
