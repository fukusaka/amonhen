//! ネットワーク単体法（Gansner, Koutsofios, North, Vo: "A Technique for Drawing Directed Graphs", IEEE TSE 1993 の
//! 4 章）。`x[head] - x[tail] >= minlen` を満たしたまま、`Σ weight * (x[head] - x[tail])` を小さくする。ランクと
//! 横の位置の両方に使う。
//!
//! 初めの張る木は、木を丸ごとずらして最も余裕の少ない辺を締める手を、2 つのヒープ（木から出る辺・木へ入る辺）と
//! 木全体のずらしの量で O(E log E) にした。木を入れ替えるたびに低・高の番号と切断の値を全部数え直す（O(V + E)）。
//! 回数には上限を置き、超えたらそこまでの（制約は満たした）答えを使う。

use std::cmp::Reverse;
use std::collections::BinaryHeap;

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub tail: usize,
    pub head: usize,
    pub minlen: i64,
    pub weight: f64,
}

struct Tree {
    /// ノードの親への木の辺（根は usize::MAX）。
    parent_edge: Vec<usize>,
    low: Vec<usize>,
    lim: Vec<usize>,
    cut: Vec<f64>,
    in_tree: Vec<bool>,
    /// 木の辺（ノードごと）。
    adj: Vec<Vec<usize>>,
    /// 後順（子が先）。
    post: Vec<usize>,
}

fn other(e: &Edge, v: usize) -> usize {
    if e.tail == v { e.head } else { e.tail }
}

/// 張る木を作り、`x` をその木の辺がきつくなるように動かす。グラフはつながっていること（そうでなければ
/// 届かないノードを根からの別の木に残すが、そのノードは動かさない）。
fn feasible_tree(n: usize, edges: &[Edge], inc: &[Vec<usize>], x: &mut [i64]) -> Vec<bool> {
    let mut in_tree_node = vec![false; n];
    let mut in_tree_edge = vec![false; edges.len()];
    let mut stored = vec![0i64; n];
    let mut members: Vec<usize> = Vec::new();
    let mut shift: i64 = 0;
    // 木から出る辺: slack = key - shift。木へ入る辺: slack = key + shift。
    let mut out_heap: BinaryHeap<Reverse<(i64, usize)>> = BinaryHeap::new();
    let mut in_heap: BinaryHeap<Reverse<(i64, usize)>> = BinaryHeap::new();
    let add = |v: usize,
               in_tree_node: &mut Vec<bool>,
               stored: &mut Vec<i64>,
               shift: i64,
               x: &[i64],
               out_heap: &mut BinaryHeap<Reverse<(i64, usize)>>,
               in_heap: &mut BinaryHeap<Reverse<(i64, usize)>>| {
        in_tree_node[v] = true;
        stored[v] = x[v] - shift;
        for &ei in &inc[v] {
            let e = &edges[ei];
            let w = other(e, v);
            if in_tree_node[w] {
                continue;
            }
            if e.tail == v {
                out_heap.push(Reverse((x[e.head] - stored[v] - e.minlen, ei)));
            } else {
                in_heap.push(Reverse((stored[v] - x[e.tail] - e.minlen, ei)));
            }
        }
    };
    for start in 0..n {
        if in_tree_node[start] {
            continue;
        }
        members.clear();
        members.push(start);
        add(
            start,
            &mut in_tree_node,
            &mut stored,
            shift,
            x,
            &mut out_heap,
            &mut in_heap,
        );
        loop {
            while let Some(Reverse((_, ei))) = out_heap.peek() {
                if in_tree_node[edges[*ei].head] {
                    out_heap.pop();
                } else {
                    break;
                }
            }
            while let Some(Reverse((_, ei))) = in_heap.peek() {
                if in_tree_node[edges[*ei].tail] {
                    in_heap.pop();
                } else {
                    break;
                }
            }
            let out_slack = out_heap.peek().map(|Reverse((k, e))| (k - shift, *e));
            let in_slack = in_heap.peek().map(|Reverse((k, e))| (k + shift, *e));
            let (slack, ei, outward) = match (out_slack, in_slack) {
                (None, None) => break,
                (Some((s, e)), None) => (s, e, true),
                (None, Some((s, e))) => (s, e, false),
                (Some((so, eo)), Some((si, e2))) => {
                    if so <= si {
                        (so, eo, true)
                    } else {
                        (si, e2, false)
                    }
                }
            };
            let slack = slack.max(0);
            let newcomer = if outward {
                shift += slack;
                out_heap.pop();
                edges[ei].head
            } else {
                shift -= slack;
                in_heap.pop();
                edges[ei].tail
            };
            in_tree_edge[ei] = true;
            members.push(newcomer);
            add(
                newcomer,
                &mut in_tree_node,
                &mut stored,
                shift,
                x,
                &mut out_heap,
                &mut in_heap,
            );
        }
        // この木のノードの位置を確定する
        for &v in &members {
            x[v] = stored[v] + shift;
        }
        shift = 0;
        out_heap.clear();
        in_heap.clear();
    }
    in_tree_edge
}

impl Tree {
    /// 根から深さ優先で低・高の番号と親の辺をつける（再帰なし）。
    fn number(&mut self, n: usize, edges: &[Edge]) {
        self.parent_edge.fill(usize::MAX);
        let mut visited = vec![false; n];
        let mut next = 1usize;
        self.post.clear();
        for root in 0..n {
            if visited[root] {
                continue;
            }
            visited[root] = true;
            self.low[root] = next;
            let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
            while let Some(&mut (v, ref mut k)) = stack.last_mut() {
                if *k < self.adj[v].len() {
                    let ei = self.adj[v][*k];
                    *k += 1;
                    let w = other(&edges[ei], v);
                    if !visited[w] {
                        visited[w] = true;
                        self.parent_edge[w] = ei;
                        self.low[w] = next;
                        stack.push((w, 0));
                    }
                } else {
                    self.lim[v] = next;
                    next += 1;
                    self.post.push(v);
                    stack.pop();
                    if let Some(&(p, _)) = stack.last() {
                        self.low[p] = self.low[p].min(self.low[v]);
                    }
                }
            }
        }
    }

    /// 後順に切断の値を数える。
    fn cut_values(&mut self, n: usize, edges: &[Edge], inc: &[Vec<usize>]) {
        let _ = n;
        for k in 0..self.post.len() {
            let c = self.post[k];
            let pe = self.parent_edge[c];
            if pe == usize::MAX {
                continue;
            }
            let e = &edges[pe];
            let child_is_tail = e.tail == c;
            let mut cv = e.weight;
            for &fi in &inc[c] {
                if fi == pe {
                    continue;
                }
                let f = &edges[fi];
                let is_out = f.tail == c;
                let points_to_head = is_out == child_is_tail;
                cv += if points_to_head { f.weight } else { -f.weight };
                if self.in_tree[fi] {
                    let o = other(f, c);
                    if self.parent_edge[o] == fi {
                        let oc = self.cut[fi];
                        cv += if points_to_head { -oc } else { oc };
                    }
                }
            }
            self.cut[pe] = cv;
        }
    }

    fn descendant(&self, v: usize, root: usize) -> bool {
        self.low[root] <= self.lim[v] && self.lim[v] <= self.lim[root]
    }
}

/// 1 回の解きで使ってよい手間（入れ替えの回数 × (ノード + 辺)）。およそ数十ミリ秒。
pub const WORK: usize = 3_000_000;

/// 値の変わらない入れ替えをこれだけ続けたら止める。
const STALL: usize = 64;

/// 解く。`x` は制約を満たした初めの値。`max_iter` は木の入れ替えの上限（`WORK` でも抑える）。
pub fn solve(n: usize, edges: &[Edge], x: &mut [i64], max_iter: usize) {
    if n == 0 {
        return;
    }
    let max_iter = max_iter.min(WORK / (n + edges.len()).max(1));
    let mut inc: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, e) in edges.iter().enumerate() {
        if e.tail == e.head {
            continue;
        }
        inc[e.tail].push(i);
        inc[e.head].push(i);
    }
    let in_tree = feasible_tree(n, edges, &inc, x);
    let mut tree = Tree {
        parent_edge: vec![usize::MAX; n],
        low: vec![0; n],
        lim: vec![0; n],
        cut: vec![0.0; edges.len()],
        in_tree,
        adj: vec![Vec::new(); n],
        post: Vec::with_capacity(n),
    };
    for (i, e) in edges.iter().enumerate() {
        if tree.in_tree[i] {
            tree.adj[e.tail].push(i);
            tree.adj[e.head].push(i);
        }
    }
    tree.number(n, edges);
    tree.cut_values(n, edges, &inc);
    let mut search_from = 0usize;
    // 値の変わらない入れ替えが続けば止める（それ以上はほとんど良くならない）
    let mut flat = 0usize;
    // 入れ替えの上限まで使い切ったら（まだ良くなるかもしれない）、打ち切ったと記す
    let mut finished = false;
    for _ in 0..max_iter {
        // 切断の値が負の木の辺
        let m = edges.len();
        let mut leave = None;
        for k in 0..m {
            let i = (search_from + k) % m;
            if tree.in_tree[i] && tree.cut[i] < -1e-9 {
                leave = Some(i);
                search_from = i + 1;
                break;
            }
        }
        let Some(leave) = leave else {
            finished = true;
            break;
        };
        let le = edges[leave];
        // 木を切ったときの尾の側（根を含まないほう）を決める
        let (tail_side, flip) = if tree.lim[le.tail] > tree.lim[le.head] {
            (le.head, true)
        } else {
            (le.tail, false)
        };
        let mut best: Option<(i64, usize)> = None;
        for (i, e) in edges.iter().enumerate() {
            if tree.in_tree[i] || e.tail == e.head {
                continue;
            }
            if flip == tree.descendant(e.tail, tail_side)
                && flip != tree.descendant(e.head, tail_side)
            {
                let slack = x[e.head] - x[e.tail] - e.minlen;
                if best.is_none_or(|(s, _)| slack < s) {
                    best = Some((slack, i));
                }
            }
        }
        let Some((slack, enter)) = best else {
            finished = true;
            break;
        };
        if slack == 0 {
            flat += 1;
            if flat > STALL {
                finished = true;
                break;
            }
        } else {
            flat = 0;
        }
        // 入れ替え
        tree.in_tree[leave] = false;
        tree.in_tree[enter] = true;
        tree.adj[le.tail].retain(|&e| e != leave);
        tree.adj[le.head].retain(|&e| e != leave);
        let ee = edges[enter];
        tree.adj[ee.tail].push(enter);
        tree.adj[ee.head].push(enter);
        tree.number(n, edges);
        tree.cut_values(n, edges, &inc);
        // 木の辺がきついように位置を伝える（根から前順）
        for k in (0..tree.post.len()).rev() {
            let v = tree.post[k];
            let pe = tree.parent_edge[v];
            if pe == usize::MAX {
                continue;
            }
            let e = &edges[pe];
            if e.tail == v {
                x[v] = x[e.head] - e.minlen;
            } else {
                x[v] = x[e.tail] + e.minlen;
            }
        }
    }
    if !finished && (0..edges.len()).any(|i| tree.in_tree[i] && tree.cut[i] < -1e-9) {
        crate::cut_short();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(tail: usize, head: usize, minlen: i64, weight: f64) -> Edge {
        Edge {
            tail,
            head,
            minlen,
            weight,
        }
    }

    fn feasible(edges: &[Edge], x: &[i64]) -> bool {
        edges.iter().all(|e| x[e.head] - x[e.tail] >= e.minlen)
    }

    fn cost(edges: &[Edge], x: &[i64]) -> f64 {
        edges
            .iter()
            .map(|e| e.weight * (x[e.head] - x[e.tail]) as f64)
            .sum()
    }

    #[test]
    fn ranks_a_small_graph_tightly() {
        // a -> b -> c, a -> c: the long edge pulls nothing apart
        let edges = [
            e(0, 1, 1, 1.0),
            e(1, 2, 1, 1.0),
            e(0, 2, 1, 1.0),
            e(3, 2, 1, 1.0),
        ];
        let mut x = vec![0, 1, 2, 0];
        solve(4, &edges, &mut x, 100);
        assert!(feasible(&edges, &x));
        assert_eq!(x[2] - x[3], 1, "{x:?}");
        assert_eq!(cost(&edges, &x), 5.0);
    }

    #[test]
    fn improves_a_poor_start() {
        // 初めは d を遠くに置く
        let edges = [
            e(0, 1, 1, 1.0),
            e(1, 2, 1, 1.0),
            e(3, 2, 1, 5.0),
            e(0, 3, 1, 1.0),
        ];
        let mut x = vec![0, 1, 2, -10];
        solve(4, &edges, &mut x, 100);
        assert!(feasible(&edges, &x));
        assert_eq!(x[2] - x[3], 1, "{x:?}");
    }

    #[test]
    fn handles_disconnected_graphs_and_zero_weights() {
        let edges = [e(0, 1, 2, 0.0), e(2, 3, 1, 1.0)];
        let mut x = vec![0, 5, 0, 9];
        solve(4, &edges, &mut x, 100);
        assert!(feasible(&edges, &x));
        assert_eq!(x[3] - x[2], 1);
    }

    #[test]
    fn stays_feasible_with_a_small_budget() {
        let mut edges = Vec::new();
        for i in 0..50 {
            edges.push(e(i, i + 1, 1, 1.0));
            if i + 7 <= 50 {
                edges.push(e(i, i + 7, 2, 3.0));
            }
        }
        let mut x: Vec<i64> = (0..51).map(|i| i * 3).collect();
        solve(51, &edges, &mut x, 3);
        assert!(feasible(&edges, &x));
        let mut y: Vec<i64> = (0..51).map(|i| i * 3).collect();
        solve(51, &edges, &mut y, 10_000);
        assert!(feasible(&edges, &y));
        assert!(cost(&edges, &y) <= cost(&edges, &x));
    }
}
