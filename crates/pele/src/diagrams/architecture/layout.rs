//! architecture の格子の配置（pele の `src/diagrams/architecture/layout.ts`）。
//!
//! Mermaid は辺の向きを力学のソルバーに相対の配置の制約として渡す。pele はそれをそのまま当てる: サービスと
//! 分岐点はそれぞれ 1 つのます目を取り、辺 `a:R -- L:b` は b を a の右のます目に置き、グループはます目の
//! 詰まった長方形なので、グループが自分のものでないノードを囲むことはない。入れ物（図、次に各グループ）は
//! それぞれ別に、内側から並べ、並べ終えたグループは 1 つの塊として親に置く。

use std::collections::HashMap;

use super::db::{ArchEdge, ArchGroup, ArchLayoutHint, ArchNode};

pub struct Grid {
    /// 渡したノードの順の、各ノードのます目。
    pub col: Vec<i64>,
    pub row: Vec<i64>,
    /// 各グループのます目の長方形と親。中身のないグループは 1 つのます目を取る。
    pub group_col: Vec<i64>,
    pub group_row: Vec<i64>,
    pub group_cols: Vec<i64>,
    pub group_rows: Vec<i64>,
    pub group_parent: Vec<i64>,
    pub group_empty: Vec<bool>,
    pub node_parent: Vec<i64>,
}

const DX: [i64; 4] = [-1, 1, 0, 0];
const DY: [i64; 4] = [0, 0, -1, 1];

pub fn side_index(side: char) -> usize {
    match side {
        'L' => 0,
        'R' => 1,
        'T' => 2,
        _ => 3,
    }
}

/// これ以下のます目の塊はます目の表で探し、大きいものは 1 つずつ比べる。
const SMALL: i64 = 64;
const OFFSET: i64 = 1 << 25;
const SPAN: i64 = 1 << 26;

struct Placer {
    n: usize,
    parent: Vec<usize>,
    link_a: Vec<usize>,
    link_b: Vec<usize>,
    side_a: Vec<usize>,
    side_b: Vec<usize>,
    item_a: Vec<usize>,
    item_b: Vec<usize>,
    edge_links: Vec<Option<Vec<usize>>>,
    x: Vec<i64>,
    y: Vec<i64>,
    w: Vec<i64>,
    h: Vec<i64>,
    placed: Vec<bool>,
    order: Vec<usize>,
    cells: HashMap<i64, usize>,
    large: Vec<usize>,
    reach: HashMap<[i64; 6], i64>,
    first: usize,
}

impl Placer {
    /// `node` から `item` までの、入れ物の中の位置の和。
    fn anchor(&self, node: usize, item: usize) -> (i64, i64) {
        let (mut ax, mut ay) = (0, 0);
        let mut cur = node;
        while cur != item {
            ax += self.x[cur];
            ay += self.y[cur];
            cur = self.n + self.parent[cur];
        }
        (ax, ay)
    }

    fn put(&mut self, item: usize, px: i64, py: i64) {
        self.x[item] = px;
        self.y[item] = py;
        self.placed[item] = true;
        self.order.push(item);
        if self.w[item] * self.h[item] > SMALL {
            self.large.push(item);
            return;
        }
        for cy in py..py + self.h[item] {
            for cx in px..px + self.w[item] {
                self.cells.insert((cx + OFFSET) * SPAN + cy + OFFSET, item);
            }
        }
    }

    fn overlaps(&self, item: usize, rx: i64, ry: i64, rw: i64, rh: i64) -> bool {
        self.x[item] < rx + rw
            && rx < self.x[item] + self.w[item]
            && self.y[item] < ry + rh
            && ry < self.y[item] + self.h[item]
    }

    fn collide(&self, rx: i64, ry: i64, rw: i64, rh: i64) -> Option<usize> {
        if rw * rh > SMALL {
            return self.order[self.first..]
                .iter()
                .copied()
                .find(|&i| self.overlaps(i, rx, ry, rw, rh));
        }
        for cy in ry..ry + rh {
            for cx in rx..rx + rw {
                if let Some(&hit) = self.cells.get(&((cx + OFFSET) * SPAN + cy + OFFSET)) {
                    return Some(hit);
                }
            }
        }
        self.large
            .iter()
            .copied()
            .find(|&i| self.overlaps(i, rx, ry, rw, rh))
    }

    /// 塊が (sx, sy) に沿ってどれだけ滑れば空くか。各歩は当たったものを飛び越え、答えは覚えておく
    /// （同じ所から始まる後の塊は、それより手前では止まれない）。
    fn slide(&mut self, rx: i64, ry: i64, rw: i64, rh: i64, sx: i64, sy: i64) -> i64 {
        let key = [rx, ry, rw, rh, sx, sy];
        let mut k = self.reach.get(&key).copied().unwrap_or(0);
        while let Some(hit) = self.collide(rx + k * sx, ry + k * sy, rw, rh) {
            k = if sx > 0 {
                self.x[hit] + self.w[hit] - rx
            } else if sx < 0 {
                rx + rw - self.x[hit]
            } else if sy > 0 {
                self.y[hit] + self.h[hit] - ry
            } else {
                ry + rh - self.y[hit]
            };
        }
        self.reach.insert(key, k);
        k
    }

    fn follow(&mut self, from: usize, k: usize) {
        let forward = self.item_a[k] == from;
        let to = if forward {
            self.item_b[k]
        } else {
            self.item_a[k]
        };
        if self.placed[to] {
            return;
        }
        let sp = if forward {
            self.side_a[k]
        } else {
            self.side_b[k]
        };
        let sq = if forward {
            self.side_b[k]
        } else {
            self.side_a[k]
        };
        let (pax, pay) = self.anchor(
            if forward {
                self.link_a[k]
            } else {
                self.link_b[k]
            },
            from,
        );
        let (ax, ay) = self.anchor(
            if forward {
                self.link_b[k]
            } else {
                self.link_a[k]
            },
            to,
        );
        let mut dx = DX[sp];
        let mut dy = DY[sp];
        let mut shift_x = 0;
        let mut shift_y = 0;
        let facing = DX[sp] + DX[sq] == 0 && DY[sp] + DY[sq] == 0;
        if sp == sq {
            // 同じ側: 辺は回り込むので、ノードは出る向きを横切って積む。
            dx = if DX[sp] == 0 { 1 } else { 0 };
            dy = 1 - dx;
        } else if !facing {
            // 曲がり: 出る側へ 1 歩、入る側から 1 歩戻る。
            shift_x = -DX[sq];
            shift_y = -DY[sq];
        }
        let mut bx;
        let mut by;
        if dx != 0 {
            bx = if dx > 0 {
                self.x[from] + self.w[from]
            } else {
                self.x[from] - self.w[to]
            };
            by = self.y[from] + pay + shift_y - ay;
        } else {
            by = if dy > 0 {
                self.y[from] + self.h[from]
            } else {
                self.y[from] - self.h[to]
            };
            bx = self.x[from] + pax + shift_x - ax;
        }
        if facing {
            // 置いたいくつかのノードが同じ側の組でこれに届くときは、最初に訪れたものでなく真ん中のものにそろえる。
            if let Some(list) = self.edge_links[to].clone()
                && list.len() > 1
            {
                let mut wanted: Vec<i64> = Vec::new();
                for j in list {
                    let to_is_b = self.item_b[j] == to;
                    let other = if to_is_b {
                        self.item_a[j]
                    } else {
                        self.item_b[j]
                    };
                    let s_other = if to_is_b {
                        self.side_a[j]
                    } else {
                        self.side_b[j]
                    };
                    let s_to = if to_is_b {
                        self.side_b[j]
                    } else {
                        self.side_a[j]
                    };
                    if !self.placed[other] || s_other != sp || s_to != sq {
                        continue;
                    }
                    let (oax, oay) = self.anchor(
                        if to_is_b {
                            self.link_a[j]
                        } else {
                            self.link_b[j]
                        },
                        other,
                    );
                    let at = if dx != 0 {
                        self.y[other] + oay
                    } else {
                        self.x[other] + oax
                    };
                    let (tax, tay) = self.anchor(
                        if to_is_b {
                            self.link_b[j]
                        } else {
                            self.link_a[j]
                        },
                        to,
                    );
                    wanted.push(at - if dx != 0 { tay } else { tax });
                }
                if wanted.len() > 1 {
                    wanted.sort();
                    let mid = wanted[(wanted.len() - 1) >> 1];
                    if dx != 0 {
                        by = mid;
                    } else {
                        bx = mid;
                    }
                }
            }
            // 取られていれば、近い方の横へずらす。辺がまっすぐ出られるように。
            let px = if dx == 0 { 1 } else { 0 };
            let py = 1 - px;
            let (tw, th) = (self.w[to], self.h[to]);
            let forth = self.slide(bx, by, tw, th, px, py);
            let back = if forth == 0 {
                0
            } else {
                self.slide(bx, by, tw, th, -px, -py)
            };
            let near = if forth <= back { forth } else { -back };
            bx += px * near;
            by += py * near;
        } else {
            let (tw, th) = (self.w[to], self.h[to]);
            let far = self.slide(bx, by, tw, th, dx, dy);
            bx += dx * far;
            by += dy * far;
        }
        self.put(to, bx, by);
    }
}

pub fn place_on_grid(
    nodes: &[ArchNode],
    groups: &[ArchGroup],
    edges: &[ArchEdge],
    hints: &[ArchLayoutHint],
) -> Grid {
    let n = nodes.len();
    let g_count = groups.len();
    let root = g_count;
    let mut node_index: HashMap<&str, usize> = HashMap::new();
    let mut group_index: HashMap<&str, usize> = HashMap::new();
    // 項目はノード（0..N-1）、次にグループ（N..N+G-1）。入れ物はグループ（0..G-1）、次に図。
    let mut parent = vec![root; n + g_count];
    let mut depth = vec![0i64; g_count + 1];
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); g_count + 1];
    for (g, group) in groups.iter().enumerate() {
        group_index.insert(&group.id, g);
        let p = group
            .in_
            .as_deref()
            .and_then(|i| group_index.get(i).copied());
        match p {
            Some(p) if p != g => {
                parent[n + g] = p;
                depth[g] = depth[p] + 1;
            }
            _ => depth[g] = 1,
        }
    }
    for (i, node) in nodes.iter().enumerate() {
        node_index.insert(&node.id, i);
        if let Some(p) = node
            .in_
            .as_deref()
            .and_then(|i| group_index.get(i).copied())
        {
            parent[i] = p;
        }
        kids[parent[i]].push(i);
    }
    for g in 0..g_count {
        kids[parent[n + g]].push(n + g);
    }

    let mut pl = Placer {
        n,
        parent,
        link_a: Vec::new(),
        link_b: Vec::new(),
        side_a: Vec::new(),
        side_b: Vec::new(),
        item_a: Vec::new(),
        item_b: Vec::new(),
        edge_links: vec![None; n + g_count],
        x: vec![0; n + g_count],
        y: vec![0; n + g_count],
        w: vec![1; n + g_count],
        h: vec![1; n + g_count],
        placed: vec![false; n + g_count],
        order: Vec::new(),
        cells: HashMap::new(),
        large: Vec::new(),
        reach: HashMap::new(),
        first: 0,
    };
    let mut hint_links: Vec<Option<Vec<usize>>> = vec![None; n + g_count];

    // つながりは、あるノードを別のノードの横に置くよう求める。両端を持ついちばん内側の入れ物のもので、
    // そこで両端がいるその入れ物の 2 つの子を結ぶ。
    let add_link = |pl: &mut Placer,
                    lists: &mut Vec<Option<Vec<usize>>>,
                    a: usize,
                    b: usize,
                    sa: usize,
                    sb: usize| {
        if a == b {
            return;
        }
        let (mut ia, mut ib) = (a, b);
        let (mut ca, mut cb) = (pl.parent[a], pl.parent[b]);
        while ca != cb {
            if depth[ca] >= depth[cb] {
                ia = n + ca;
                ca = pl.parent[ia];
            } else {
                ib = n + cb;
                cb = pl.parent[ib];
            }
        }
        let k = pl.link_a.len();
        pl.link_a.push(a);
        pl.link_b.push(b);
        pl.side_a.push(sa);
        pl.side_b.push(sb);
        pl.item_a.push(ia);
        pl.item_b.push(ib);
        lists[ia].get_or_insert_with(Vec::new).push(k);
        lists[ib].get_or_insert_with(Vec::new).push(k);
    };
    for hint in hints {
        let row = hint.direction == "row";
        for i in 1..hint.members.len() {
            let a = node_index.get(hint.members[i - 1].as_str());
            let b = node_index.get(hint.members[i].as_str());
            if let (Some(&a), Some(&b)) = (a, b) {
                add_link(
                    &mut pl,
                    &mut hint_links,
                    a,
                    b,
                    if row { 1 } else { 3 },
                    if row { 0 } else { 2 },
                );
            }
        }
    }
    // 同じ側どうしの辺は端の置き場所をいちばん言わないので、最後に。
    let mut edge_links = std::mem::take(&mut pl.edge_links);
    for like_sides in [false, true] {
        for edge in edges {
            if (edge.lhs_dir == edge.rhs_dir) != like_sides {
                continue;
            }
            let a = node_index.get(edge.lhs_id.as_str());
            let b = node_index.get(edge.rhs_id.as_str());
            if let (Some(&a), Some(&b)) = (a, b) {
                add_link(
                    &mut pl,
                    &mut edge_links,
                    a,
                    b,
                    side_index(edge.lhs_dir),
                    side_index(edge.rhs_dir),
                );
            }
        }
    }
    pl.edge_links = edge_links;

    let layout = |pl: &mut Placer, c: usize| {
        let items = &kids[c];
        if items.is_empty() {
            return;
        }
        pl.order.clear();
        let mut starts: Vec<usize> = Vec::new();
        for &start in items {
            if pl.placed[start] {
                continue;
            }
            pl.first = pl.order.len();
            starts.push(pl.first);
            pl.cells.clear();
            pl.large.clear();
            pl.reach.clear();
            pl.put(start, 0, 0);
            // 決めた並べ方は、ノードを置いたらすぐ、どの辺よりも先にたどる。
            let mut hint_at = pl.first;
            let mut edge_at = pl.first;
            let mut link_at = 0;
            loop {
                if hint_at < pl.order.len() {
                    let from = pl.order[hint_at];
                    hint_at += 1;
                    if let Some(list) = &hint_links[from] {
                        for &k in list {
                            pl.follow(from, k);
                        }
                    }
                    continue;
                }
                if edge_at >= pl.order.len() {
                    break;
                }
                let from = pl.order[edge_at];
                let next = pl.edge_links[from]
                    .as_ref()
                    .and_then(|list| list.get(link_at).copied());
                match next {
                    None => {
                        edge_at += 1;
                        link_at = 0;
                    }
                    Some(k) => {
                        link_at += 1;
                        pl.follow(from, k);
                    }
                }
            }
        }
        starts.push(pl.order.len());

        // つながらない部分は横に並べ、全体がおおよそ正方形になるよう段に折り返す。
        let count = starts.len() - 1;
        let mut min_x = vec![0i64; count];
        let mut min_y = vec![0i64; count];
        let mut part_w = vec![0i64; count];
        let mut part_h = vec![0i64; count];
        let mut area = 0i64;
        let mut widest = 0i64;
        for p in 0..count {
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for i in starts[p]..starts[p + 1] {
                let item = pl.order[i];
                x0 = x0.min(pl.x[item]);
                y0 = y0.min(pl.y[item]);
                x1 = x1.max(pl.x[item] + pl.w[item]);
                y1 = y1.max(pl.y[item] + pl.h[item]);
            }
            min_x[p] = x0;
            min_y[p] = y0;
            part_w[p] = x1 - x0;
            part_h[p] = y1 - y0;
            area += part_w[p] * part_h[p];
            widest = widest.max(part_w[p]);
        }
        let limit = widest.max(((area * 2) as f64).sqrt().ceil() as i64);
        let (mut cx, mut cy, mut shelf, mut width) = (0i64, 0i64, 0i64, 0i64);
        for p in 0..count {
            if cx > 0 && cx + part_w[p] > limit {
                cy += shelf;
                cx = 0;
                shelf = 0;
            }
            for i in starts[p]..starts[p + 1] {
                let item = pl.order[i];
                pl.x[item] += cx - min_x[p];
                pl.y[item] += cy - min_y[p];
            }
            cx += part_w[p];
            shelf = shelf.max(part_h[p]);
            width = width.max(cx);
        }
        if c < g_count {
            pl.w[n + c] = width;
            pl.h[n + c] = cy + shelf;
        }
    };
    for c in (0..g_count).rev() {
        layout(&mut pl, c);
    }
    layout(&mut pl, root);

    let mut group_col = vec![0i64; g_count];
    let mut group_row = vec![0i64; g_count];
    let mut group_parent = vec![0i64; g_count];
    let mut group_empty = vec![false; g_count];
    for g in 0..g_count {
        let p = pl.parent[n + g];
        group_parent[g] = if p == root { -1 } else { p as i64 };
        group_col[g] = pl.x[n + g] + if p == root { 0 } else { group_col[p] };
        group_row[g] = pl.y[n + g] + if p == root { 0 } else { group_row[p] };
        group_empty[g] = kids[g].is_empty();
    }
    let mut col = vec![0i64; n];
    let mut row = vec![0i64; n];
    let mut node_parent = vec![0i64; n];
    for i in 0..n {
        let p = pl.parent[i];
        node_parent[i] = if p == root { -1 } else { p as i64 };
        col[i] = pl.x[i] + if p == root { 0 } else { group_col[p] };
        row[i] = pl.y[i] + if p == root { 0 } else { group_row[p] };
    }
    Grid {
        col,
        row,
        group_col,
        group_row,
        group_cols: pl.w[n..].to_vec(),
        group_rows: pl.h[n..].to_vec(),
        group_parent,
        group_empty,
        node_parent,
    }
}
