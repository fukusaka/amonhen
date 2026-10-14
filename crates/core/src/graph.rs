//! History のグラフのレイアウト。
//!
//! 行は `CommitGraph` と同じトポロジカル順（子が親より先）。上から 1 行ずつレーンを割り当て、
//! 点（`Node`）と線（`Edge`）を作る。線は行ごとに分けず、縦の直線は区間 1 つ、
//! レーンをまたぐ所だけ 1 行分の曲線にする。
//!
//! - HEAD から first-parent で辿れる幹線はレーン 0 に固定する。
//! - 色はレーン番号ではなく、枝の出自のコミットから決める。前の行の状態に依存しないので、
//!   再計算しても色が変わらない。
//! - `CHECKPOINT_INTERVAL` 行ごとにレーンの状態を保存し、コミットが増えたときは
//!   先頭から計算し直して、状態が前回と一致した所で打ち切る（`relayout`）。

use std::collections::HashMap;
use std::ops::Range;

use amonhen_layout::{Edge, Node};
use amonhen_model::{CommitGraph, Oid};

pub const CHECKPOINT_INTERVAL: u32 = 4096;
const BUCKET_ROWS: u32 = 1024;
const MAINLINE_COLOR: u16 = 0;

/// まだ親に届いていない線。`target` の行に来たら `Edge` にする。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Track {
    target: u32,
    child_row: u32,
    child_lane: u16,
    color: u16,
    first_parent: bool,
}

impl Track {
    fn shifted(self, delta: i64) -> Self {
        Self {
            target: (self.target as i64 + delta) as u32,
            child_row: (self.child_row as i64 + delta) as u32,
            ..self
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct State {
    /// レーンごとの線。`None` は空きレーン。
    lanes: Vec<Option<Track>>,
    /// ほかの線が待っている親に合流する線。縦の部分と終わりの曲線はレーンの持ち主が描くので、
    /// 始まりの曲線だけを出す。
    riders: Vec<(u16, Track)>,
}

impl State {
    fn shifted(&self, delta: i64) -> Self {
        Self {
            lanes: self
                .lanes
                .iter()
                .map(|t| t.map(|t| t.shifted(delta)))
                .collect(),
            riders: self
                .riders
                .iter()
                .map(|&(l, t)| (l, t.shifted(delta)))
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
struct Checkpoint {
    /// 再計算で前回の結果を継ぎ足すと、`CHECKPOINT_INTERVAL` の倍数からずれる。
    row: u32,
    state: State,
    /// この行より前に確定した線の数。
    edges_len: usize,
}

#[derive(Clone, Debug)]
pub struct GraphLayout {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    lane_count: u16,
    mainline: Vec<bool>,
    checkpoints: Vec<Checkpoint>,
    /// `BUCKET_ROWS` 行ごとに、その範囲に重なる線の添字。
    buckets: Vec<Vec<u32>>,
}

impl GraphLayout {
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn lane_count(&self) -> u16 {
        self.lane_count
    }

    /// `rows` に重なる線の添字。
    pub fn edges_in(&self, rows: Range<u32>) -> impl Iterator<Item = u32> + '_ {
        let first = rows.start / BUCKET_ROWS;
        let last = rows.end.saturating_sub(1) / BUCKET_ROWS;
        (first..=last)
            .filter_map(|b| self.buckets.get(b as usize).map(|bucket| (b, bucket)))
            .flat_map(move |(b, bucket)| {
                // 複数のバケットに重なる線は、範囲内で最初のバケットでだけ数える。
                let bucket_start = b * BUCKET_ROWS;
                bucket.iter().copied().filter(move |&i| {
                    let e = &self.edges[i as usize];
                    let first_seen = b == first || e.row_start >= bucket_start;
                    first_seen && e.row_start < rows.end && e.row_end >= rows.start
                })
            })
    }

    fn build_buckets(&mut self) {
        let count = (self.nodes.len() as u32).div_ceil(BUCKET_ROWS) as usize;
        let mut buckets = vec![Vec::new(); count];
        for (i, e) in self.edges.iter().enumerate() {
            for b in e.row_start / BUCKET_ROWS..=e.row_end / BUCKET_ROWS {
                buckets[b as usize].push(i as u32);
            }
        }
        self.buckets = buckets;
    }
}

/// HEAD から first-parent で辿れる行。
fn mainline(graph: &CommitGraph, head_row: Option<u32>) -> Vec<bool> {
    let mut flags = vec![false; graph.len()];
    let mut row = head_row;
    while let Some(r) = row {
        flags[r as usize] = true;
        row = graph.parents(r as usize).first().copied();
    }
    flags
}

fn color_of(id: &Oid) -> u16 {
    // 0 は幹線に取っておく。
    (u16::from_le_bytes([id.0[0], id.0[1]]) % (u16::MAX - 1)) + 1
}

/// レイアウトを少しずつ進める。最初の 1 画面分だけ先に計算して表示するのに使う。
pub struct Layouter<'g> {
    graph: &'g CommitGraph,
    mainline: Vec<bool>,
    has_mainline: bool,
    row: u32,
    state: State,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    lane_count: u16,
    checkpoints: Vec<Checkpoint>,
}

impl<'g> Layouter<'g> {
    pub fn new(graph: &'g CommitGraph, head_row: Option<u32>) -> Self {
        let mainline = mainline(graph, head_row);
        Self {
            graph,
            mainline,
            has_mainline: head_row.is_some(),
            row: 0,
            state: State::default(),
            nodes: Vec::with_capacity(graph.len()),
            edges: Vec::with_capacity(graph.len() + graph.len() / 4),
            lane_count: 0,
            checkpoints: Vec::new(),
        }
    }

    /// 次に計算する行。
    pub fn row(&self) -> u32 {
        self.row
    }

    pub fn is_done(&self) -> bool {
        self.row as usize >= self.graph.len()
    }

    /// 最大 `rows` 行進める。
    pub fn advance(&mut self, rows: u32) {
        let end = (self.row as usize)
            .saturating_add(rows as usize)
            .min(self.graph.len());
        while (self.row as usize) < end {
            self.step();
        }
    }

    pub fn finish(mut self) -> GraphLayout {
        self.advance(u32::MAX);
        let mut layout = GraphLayout {
            nodes: self.nodes,
            edges: self.edges,
            lane_count: self.lane_count,
            mainline: self.mainline,
            checkpoints: self.checkpoints,
            buckets: Vec::new(),
        };
        layout.build_buckets();
        layout
    }

    fn alloc_lane(&mut self) -> u16 {
        let start = usize::from(self.has_mainline);
        let lanes = &mut self.state.lanes;
        let lane = match lanes.iter().skip(start).position(Option::is_none) {
            Some(i) => i + start,
            None => {
                lanes.resize(lanes.len().max(start) + 1, None);
                lanes.len() - 1
            }
        };
        lane as u16
    }

    fn push_edge(&mut self, rows: (u32, u32), lanes: (u16, u16), color: u16, first_parent: bool) {
        self.edges.push(Edge {
            row_start: rows.0,
            row_end: rows.1,
            lane_start: lanes.0,
            lane_end: lanes.1,
            color,
            kind: if first_parent { Edge::FIRST_PARENT } else { 0 },
        });
    }

    /// レーン `lane` を通ってきた線を、`row` の `node_lane` にある点で終える。
    fn finish_track(&mut self, t: Track, lane: u16, row: u32, node_lane: u16) {
        let (cr, cl, c, fp) = (t.child_row, t.child_lane, t.color, t.first_parent);
        if cr + 1 == row {
            self.push_edge((cr, row), (cl, node_lane), c, fp);
            return;
        }
        let vstart = if cl != lane {
            self.push_edge((cr, cr + 1), (cl, lane), c, fp);
            cr + 1
        } else {
            cr
        };
        let vend = if node_lane != lane { row - 1 } else { row };
        if vend > vstart {
            self.push_edge((vstart, vend), (lane, lane), c, fp);
        }
        if node_lane != lane {
            self.push_edge((row - 1, row), (lane, node_lane), c, fp);
        }
    }

    fn finish_rider(&mut self, t: Track, lane: u16, row: u32, node_lane: u16) {
        let (cr, cl) = (t.child_row, t.child_lane);
        if cr + 1 == row {
            self.push_edge((cr, row), (cl, node_lane), t.color, t.first_parent);
        } else {
            self.push_edge((cr, cr + 1), (cl, lane), t.color, t.first_parent);
        }
    }

    fn step(&mut self) {
        let r = self.row;
        if r.is_multiple_of(CHECKPOINT_INTERVAL) {
            self.checkpoints.push(Checkpoint {
                row: r,
                state: self.state.clone(),
                edges_len: self.edges.len(),
            });
        }
        let graph = self.graph;
        let id = &graph.ids()[r as usize];
        let is_main = self.mainline[r as usize];

        // この行の点で終わる線のレーン。
        let arrivals: Vec<u16> = (0..self.state.lanes.len())
            .filter(|&l| matches!(self.state.lanes[l], Some(t) if t.target == r))
            .map(|l| l as u16)
            .collect();
        let first_parent_arrival = arrivals
            .iter()
            .copied()
            .find(|&l| self.state.lanes[l as usize].is_some_and(|t| t.first_parent));
        let node_lane = if is_main {
            0
        } else if let Some(l) = first_parent_arrival.or(arrivals.first().copied()) {
            l
        } else {
            self.alloc_lane()
        };
        if self.state.lanes.len() <= node_lane as usize {
            self.state.lanes.resize(node_lane as usize + 1, None);
        }
        let color = match self.state.lanes[node_lane as usize] {
            Some(t) if t.target == r => t.color,
            _ if is_main => MAINLINE_COLOR,
            _ => color_of(id),
        };

        for lane in arrivals {
            let t = self.state.lanes[lane as usize].take().unwrap();
            self.finish_track(t, lane, r, node_lane);
        }
        let mut riders = std::mem::take(&mut self.state.riders);
        riders.retain(|&(lane, t)| {
            if t.target == r {
                self.finish_rider(t, lane, r, node_lane);
                false
            } else {
                true
            }
        });
        self.state.riders = riders;

        let parents = graph.parents(r as usize);
        self.nodes.push(Node {
            row: r,
            lane: node_lane,
            color,
            flags: if parents.len() > 1 { Node::MERGE } else { 0 },
            _pad: 0,
        });

        for (i, &p) in parents.iter().enumerate() {
            let first_parent = i == 0;
            let p_main = self.mainline[p as usize];
            // 同じ親を待っている線があれば合流する。幹線のコミットにはレーン 0 でだけ合流する。
            let shared = self
                .state
                .lanes
                .iter()
                .position(|t| t.is_some_and(|t| t.target == p))
                .filter(|&l| !p_main || l == 0);
            if let Some(lane) = shared {
                let lane_color = self.state.lanes[lane].unwrap().color;
                let track = Track {
                    target: p,
                    child_row: r,
                    child_lane: node_lane,
                    color: if first_parent { color } else { lane_color },
                    first_parent,
                };
                self.state.riders.push((lane as u16, track));
                continue;
            }
            let (lane, track_color) = if first_parent {
                (node_lane, color)
            } else {
                (self.alloc_lane(), color_of(&graph.ids()[p as usize]))
            };
            self.state.lanes[lane as usize] = Some(Track {
                target: p,
                child_row: r,
                child_lane: node_lane,
                color: track_color,
                first_parent,
            });
        }

        self.lane_count = self.lane_count.max(self.state.lanes.len() as u16);
        // 末尾の空きレーンを残すと、再計算で状態が一致しなくなる。
        while self.state.lanes.last().is_some_and(Option::is_none) {
            self.state.lanes.pop();
        }
        self.row += 1;
    }
}

pub fn layout(graph: &CommitGraph, head_row: Option<u32>) -> GraphLayout {
    Layouter::new(graph, head_row).finish()
}

/// コミットが増えたり減ったりした後のレイアウト。先頭から計算し直し、チェックポイントで
/// 前回と状態が一致したら、残りは前回の結果を行をずらして使う。
pub fn relayout(
    old: &GraphLayout,
    old_graph: &CommitGraph,
    graph: &CommitGraph,
    head_row: Option<u32>,
) -> GraphLayout {
    relayout_inner(old, old_graph, graph, head_row).0
}

/// 打ち切った行（前回の結果を使い始めた行）も返す。
fn relayout_inner(
    old: &GraphLayout,
    old_graph: &CommitGraph,
    graph: &CommitGraph,
    head_row: Option<u32>,
) -> (GraphLayout, Option<usize>) {
    let old_rows: HashMap<Oid, usize> = old
        .checkpoints
        .iter()
        .enumerate()
        .filter(|(_, ck)| ck.row > 0)
        .map(|(k, ck)| (old_graph.ids()[ck.row as usize], k))
        .collect();

    let mut l = Layouter::new(graph, head_row);
    while !l.is_done() {
        l.advance(1);
        let r = l.row() as usize;
        if l.is_done() {
            break;
        }
        let Some(&k) = old_rows.get(&graph.ids()[r]) else {
            continue;
        };
        let ck = &old.checkpoints[k];
        let ro = ck.row as usize;
        let delta = r as i64 - ro as i64;
        let converged = graph.ids()[r..] == old_graph.ids()[ro..]
            && l.mainline[r..] == old.mainline[ro..]
            && ck.state.shifted(delta) == l.state;
        if converged {
            return (splice(l, old, k, delta), Some(r));
        }
    }
    (l.finish(), None)
}

fn splice(l: Layouter<'_>, old: &GraphLayout, k: usize, delta: i64) -> GraphLayout {
    let shift = |row: u32| (row as i64 + delta) as u32;
    let ck = &old.checkpoints[k];
    let ro = ck.row as usize;
    let new_edges_len = l.edges.len();

    let mut nodes = l.nodes;
    nodes.extend(old.nodes[ro..].iter().map(|n| Node {
        row: shift(n.row),
        ..*n
    }));
    let mut edges = l.edges;
    edges.extend(old.edges[ck.edges_len..].iter().map(|e| Edge {
        row_start: shift(e.row_start),
        row_end: shift(e.row_end),
        ..*e
    }));
    let mut checkpoints = l.checkpoints;
    checkpoints.extend(old.checkpoints[k..].iter().map(|c| Checkpoint {
        row: shift(c.row),
        state: c.state.shifted(delta),
        edges_len: c.edges_len - ck.edges_len + new_edges_len,
    }));

    let mut layout = GraphLayout {
        nodes,
        edges,
        lane_count: l.lane_count.max(old.lane_count),
        mainline: l.mainline,
        checkpoints,
        buckets: Vec::new(),
    };
    layout.build_buckets();
    layout
}

#[doc(hidden)]
pub mod synthetic {
    //! ベンチとテスト用の、それらしい履歴。幹線に feature ブランチが分岐・合流する。

    use amonhen_model::{CommitGraph, CommitGraphBuilder, Oid};

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn oid(n: u64) -> Oid {
        let mut bytes = [0; 20];
        bytes[..8].copy_from_slice(&n.wrapping_mul(0x9E37_79B9_7F4A_7C15).to_le_bytes());
        bytes[8..16].copy_from_slice(&n.to_le_bytes());
        Oid(bytes)
    }

    /// 古い順に `commits` 個作り、新しい順の `CommitGraph` と HEAD（幹線の先頭）の行を返す。
    pub fn graph(commits: usize, seed: u64) -> (CommitGraph, Option<u32>) {
        let mut rng = Rng(seed | 1);
        let mut parents: Vec<Vec<usize>> = vec![Vec::new()];
        let mut main = 0;
        let mut branches: Vec<usize> = Vec::new();
        while parents.len() < commits {
            let new = parents.len();
            match rng.below(10) {
                0 if branches.len() < 30 => {
                    parents.push(vec![main]);
                    branches.push(new);
                }
                1 if !branches.is_empty() => {
                    let b = branches.swap_remove(rng.below(branches.len() as u64) as usize);
                    parents.push(vec![main, b]);
                    main = new;
                }
                2..=5 if !branches.is_empty() => {
                    let i = rng.below(branches.len() as u64) as usize;
                    parents.push(vec![branches[i]]);
                    branches[i] = new;
                }
                _ => {
                    parents.push(vec![main]);
                    main = new;
                }
            }
        }
        let mut b = CommitGraphBuilder::with_capacity(commits);
        for i in (0..parents.len()).rev() {
            b.push(
                oid(i as u64),
                i as i64,
                parents[i].iter().map(|&p| oid(p as u64)),
            );
        }
        let head_row = (parents.len() - 1 - main) as u32;
        (b.build(), Some(head_row))
    }

    /// `graph(commits, seed)` の履歴に、新しいコミットを `extra` 個足したもの。
    pub fn extended(commits: usize, extra: usize, seed: u64) -> (CommitGraph, Option<u32>) {
        graph(commits + extra, seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_model::CommitGraphBuilder;

    fn oid(n: u8) -> Oid {
        Oid([n; 20])
    }

    fn graph(commits: &[(u8, &[u8])]) -> CommitGraph {
        let mut b = CommitGraphBuilder::default();
        for (id, parents) in commits {
            b.push(oid(*id), 0, parents.iter().map(|&p| oid(p)));
        }
        b.build()
    }

    fn check_invariants(graph: &CommitGraph, layout: &GraphLayout) {
        assert_eq!(layout.nodes().len(), graph.len());
        for (row, node) in layout.nodes().iter().enumerate() {
            assert_eq!(node.row as usize, row);
            assert!(node.lane < layout.lane_count());
            if layout.mainline[row] {
                assert_eq!(node.lane, 0);
            }
        }
        for e in layout.edges() {
            assert!(e.row_start < e.row_end);
            assert!(e.lane_start < layout.lane_count() && e.lane_end < layout.lane_count());
            // レーンをまたぐのは 1 行分の曲線だけ。
            if e.lane_start != e.lane_end {
                assert_eq!(e.row_end - e.row_start, 1);
            }
        }
        // 親子のどの組にも、子の点から出る線と、親の点に入る線がある。
        for child in 0..graph.len() {
            let n = layout.nodes()[child];
            for &p in graph.parents(child) {
                let pn = layout.nodes()[p as usize];
                assert!(
                    layout
                        .edges()
                        .iter()
                        .any(|e| e.row_start == n.row && e.lane_start == n.lane)
                );
                assert!(
                    layout
                        .edges()
                        .iter()
                        .any(|e| e.row_end == pn.row && e.lane_end == pn.lane)
                );
            }
        }
    }

    #[test]
    fn linear_history_is_one_straight_line_on_lane_0() {
        let g = graph(&[(3, &[2]), (2, &[1]), (1, &[0]), (0, &[])]);
        let l = layout(&g, Some(0));
        check_invariants(&g, &l);
        assert_eq!(l.lane_count(), 1);
        // 隣り合う行を結ぶ線が 3 本（1 行ずつ確定する）。
        assert_eq!(l.edges().len(), 3);
        assert!(l.nodes().iter().all(|n| n.color == MAINLINE_COLOR));
    }

    #[test]
    fn a_long_line_is_one_edge() {
        // 4 は 3 と 1 の merge。1 への線は 2 行をまたいで 1 本の縦線になる。
        let g = graph(&[(4, &[3, 1]), (3, &[2]), (2, &[0]), (1, &[0]), (0, &[])]);
        let l = layout(&g, Some(0));
        check_invariants(&g, &l);
        let side = l.nodes()[3];
        assert_eq!(side.lane, 1);
        assert!(
            l.edges().iter().any(|e| e.lane_start == 1
                && e.lane_end == 1
                && e.row_start == 1
                && e.row_end == 3)
        );
    }

    #[test]
    fn mainline_stays_on_lane_0_even_if_another_branch_is_newer() {
        // 行 0 は topic（HEAD ではない）、HEAD は行 1 の main。
        let g = graph(&[(5, &[2]), (4, &[3]), (3, &[2]), (2, &[])]);
        let l = layout(&g, Some(1));
        check_invariants(&g, &l);
        assert_eq!(l.nodes()[1].lane, 0);
        assert_eq!(l.nodes()[0].lane, 1);
    }

    #[test]
    fn synthetic_history_keeps_invariants() {
        let (g, head) = synthetic::graph(3000, 7);
        let l = layout(&g, head);
        check_invariants(&g, &l);
        assert!(l.lane_count() > 1);
    }

    #[test]
    fn edges_in_returns_each_overlapping_edge_once() {
        let (g, head) = synthetic::graph(5000, 3);
        let l = layout(&g, head);
        for range in [0..50, 1000..1030, 1020..2100, 4990..5000] {
            let mut got: Vec<u32> = l.edges_in(range.clone()).collect();
            got.sort_unstable();
            let mut want: Vec<u32> = (0..l.edges().len() as u32)
                .filter(|&i| {
                    let e = l.edges()[i as usize];
                    e.row_start < range.end && e.row_end >= range.start
                })
                .collect();
            want.sort_unstable();
            assert_eq!(got, want, "{range:?}");
        }
    }

    #[test]
    fn streaming_gives_the_same_result() {
        let (g, head) = synthetic::graph(10_000, 11);
        let mut l = Layouter::new(&g, head);
        l.advance(40);
        assert_eq!(l.row(), 40);
        let streamed = l.finish();
        let full = layout(&g, head);
        assert_eq!(streamed.nodes(), full.nodes());
        assert_eq!(streamed.edges(), full.edges());
    }

    #[test]
    fn relayout_after_new_commits_matches_a_full_layout() {
        let (old_g, old_head) = synthetic::graph(20_000, 5);
        let old = layout(&old_g, old_head);
        for extra in [1, 37, 500] {
            let (g, head) = synthetic::extended(20_000, extra, 5);
            let (re, spliced_at) = relayout_inner(&old, &old_g, &g, head);
            let first_checkpoint = CHECKPOINT_INTERVAL as usize + extra;
            assert_eq!(spliced_at, Some(first_checkpoint), "extra = {extra}");
            let full = layout(&g, head);
            assert_eq!(re.nodes(), full.nodes(), "extra = {extra}");
            assert_eq!(re.edges(), full.edges(), "extra = {extra}");
        }
    }
}
