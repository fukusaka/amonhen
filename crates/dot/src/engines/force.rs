//! fdp と sfdp: ばねと斥力の配置（Fruchterman, Reingold, "Graph Drawing by Force-directed Placement", 1991）。
//! 引力は d²/L（L は辺の望む長さ）、斥力は L²/d。温度を下げながら動きを抑える。ノードの大きさは、望む長さと斥力の
//! 長さに半径を足して入れる。
//!
//! - fdp はクラスタを内から 1 つの箱として配置し（中を配置して箱にし、外ではその箱を 1 つのノードとして扱う）、
//!   箱を描く。
//! - sfdp は多段（Walshaw, "A Multilevel Algorithm for Force-Directed Graph Drawing", 2000）: 隣どうしを組にして
//!   粗いグラフを重ね、いちばん粗いものから配置して細かいものへ戻しながら整える。斥力は Barnes–Hut の四分木
//!   （Barnes, Hut 1986。Hu, "Efficient and High Quality Force-Directed Graph Drawing", 2005 と同じ使い方）。

use std::collections::BTreeSet;

use super::common::{self, INCH, Overlap, P, Rng};
use super::{Group, Item, Link};
use crate::Note;
use crate::graph::Attrs;

/// 力の計算で見る対の数の上限（図ぜんぶ）。
const WORK: usize = 20_000_000;
/// 斥力の届く距離（望む長さの倍数）。
const REACH: f64 = 2.5;
/// すべての対で斥力を数える大きさの上限（超えたら格子か四分木）。
const ALL_PAIRS: usize = 300;

/// 力で動かす系。
struct System {
    pos: Vec<P>,
    /// 半径（大きさの半分の平均）。
    r: Vec<f64>,
    /// 重さ（sfdp の粗いノードの数）。
    mass: Vec<f64>,
    fixed: Vec<bool>,
    /// 尾・頭・望む長さ・重み。
    edges: Vec<(usize, usize, f64, f64)>,
}

impl System {
    /// 斥力（i への力）を `out` に足す。`k` は斥力の基の長さ。
    fn repulse_all(&self, k: f64, out: &mut [P]) {
        let n = self.pos.len();
        for i in 0..n {
            for j in i + 1..n {
                let (dx, dy) = (self.pos[i].0 - self.pos[j].0, self.pos[i].1 - self.pos[j].1);
                let mut d2 = dx * dx + dy * dy;
                let (ux, uy) = if d2 < 1e-6 {
                    d2 = 1e-6;
                    if (i + j).is_multiple_of(2) {
                        (1e-3, 0.0)
                    } else {
                        (0.0, 1e-3)
                    }
                } else {
                    (dx, dy)
                };
                let l = k + self.r[i] + self.r[j];
                // 遠い対は数えない（Fruchterman と Reingold の格子の版と同じく、望む長さの数倍まで）
                if d2 > REACH * REACH * l * l {
                    continue;
                }
                // L²/d の大きさで、(dx, dy)/d の向き → L² (dx, dy)/d²
                let f = l * l / d2;
                let (fx, fy) = (ux * f, uy * f);
                out[i].0 += fx * self.mass[j];
                out[i].1 += fy * self.mass[j];
                out[j].0 -= fx * self.mass[i];
                out[j].1 -= fy * self.mass[i];
            }
        }
    }

    /// 格子で近いものだけの斥力（Fruchterman と Reingold の格子の版。遠い対は数えない）。
    fn repulse_grid(&self, k: f64, out: &mut [P]) {
        let n = self.pos.len();
        let rmax = self.r.iter().copied().fold(0.0, f64::max);
        let cell = 2.0 * (k + 2.0 * rmax);
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &self.pos {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        let cols = (((x1 - x0) / cell) as usize + 1).clamp(1, 512);
        let rows = (((y1 - y0) / cell) as usize + 1).clamp(1, 512);
        let cell_x = ((x1 - x0) / cols as f64).max(1e-9);
        let cell_y = ((y1 - y0) / rows as f64).max(1e-9);
        let at = |p: P| -> (usize, usize) {
            (
                (((p.0 - x0) / cell_x) as usize).min(cols - 1),
                (((p.1 - y0) / cell_y) as usize).min(rows - 1),
            )
        };
        let mut grid: Vec<Vec<u32>> = vec![Vec::new(); cols * rows];
        for (i, p) in self.pos.iter().enumerate() {
            let (cx, cy) = at(*p);
            grid[cy * cols + cx].push(i as u32);
        }
        let reach = cell * cell;
        for (i, o) in out.iter_mut().enumerate().take(n) {
            let (cx, cy) = at(self.pos[i]);
            for gy in cy.saturating_sub(1)..=(cy + 1).min(rows - 1) {
                for gx in cx.saturating_sub(1)..=(cx + 1).min(cols - 1) {
                    for &j in &grid[gy * cols + gx] {
                        let j = j as usize;
                        if j == i {
                            continue;
                        }
                        let (dx, dy) =
                            (self.pos[i].0 - self.pos[j].0, self.pos[i].1 - self.pos[j].1);
                        let mut d2 = dx * dx + dy * dy;
                        if d2 > reach {
                            continue;
                        }
                        let (ux, uy) = if d2 < 1e-6 {
                            d2 = 1e-6;
                            if i < j { (1e-3, 0.0) } else { (-1e-3, 0.0) }
                        } else {
                            (dx, dy)
                        };
                        let l = k + self.r[i] + self.r[j];
                        let f = l * l / d2 * self.mass[j];
                        o.0 += ux * f;
                        o.1 += uy * f;
                    }
                }
            }
        }
    }

    /// 四分木で遠いまとまりを 1 つの重さとして数える斥力（Barnes–Hut）。
    fn repulse_tree(&self, k: f64, out: &mut [P]) {
        let tree = Quad::build(&self.pos, &self.mass);
        let r_avg = self.r.iter().sum::<f64>() / self.r.len().max(1) as f64;
        let l = k + 2.0 * r_avg;
        let l2 = l * l;
        let mut stack: Vec<u32> = Vec::new();
        for (i, o) in out.iter_mut().enumerate().take(self.pos.len()) {
            let p = self.pos[i];
            stack.clear();
            stack.push(0);
            while let Some(q) = stack.pop() {
                let node = &tree.nodes[q as usize];
                if node.mass <= 0.0 {
                    continue;
                }
                let (cx, cy) = (node.mx / node.mass, node.my / node.mass);
                let (dx, dy) = (p.0 - cx, p.1 - cy);
                let d2 = dx * dx + dy * dy;
                let leaf = node.child == 0;
                if leaf && node.body == i as u32 && node.count == 1 {
                    continue;
                }
                let size = 2.0 * node.half;
                if leaf || size * size < THETA2 * d2 {
                    let (ux, uy, d2) = if d2 < 1e-6 {
                        (1e-3 * ((i % 2) as f64 * 2.0 - 1.0), 0.0, 1e-6)
                    } else {
                        (dx, dy, d2)
                    };
                    // 自分を含む葉（重なった点）は自分の分を除く
                    let mass = if leaf && node.contains(i as u32) {
                        node.mass - self.mass[i]
                    } else {
                        node.mass
                    };
                    let f = l2 / d2 * mass;
                    o.0 += ux * f;
                    o.1 += uy * f;
                } else {
                    for c in 0..4 {
                        stack.push(node.child + c);
                    }
                }
            }
        }
    }

    /// 引力。
    fn attract(&self, out: &mut [P]) {
        for &(a, b, ideal, w) in &self.edges {
            let (dx, dy) = (self.pos[b].0 - self.pos[a].0, self.pos[b].1 - self.pos[a].1);
            let d = (dx * dx + dy * dy).sqrt();
            if d < 1e-9 {
                continue;
            }
            // d²/L の大きさ → (dx, dy) d / L
            let f = d / ideal.max(1.0) * w;
            out[a].0 += dx * f;
            out[a].1 += dy * f;
            out[b].0 -= dx * f;
            out[b].1 -= dy * f;
        }
    }

    /// 温度を下げながら `iters` 回動かす。`t0` は初めの温度（1 回に動ける長さ）。
    fn run(&mut self, k: f64, iters: usize, t0: f64, mode: Repulsion, budget: &mut usize) {
        let n = self.pos.len();
        if n < 2 {
            return;
        }
        let per = match mode {
            Repulsion::All => n * n / 2 + self.edges.len(),
            Repulsion::Grid => n * 100 + self.edges.len(),
            Repulsion::Tree => n * 150 + self.edges.len(),
        };
        let want = iters;
        let iters = iters.min(*budget / per.max(1)).max(1);
        if iters < want {
            crate::cut_short();
        }
        let mut force = vec![(0.0, 0.0); n];
        for it in 0..iters {
            *budget = budget.saturating_sub(per);
            force.iter_mut().for_each(|f| *f = (0.0, 0.0));
            match mode {
                Repulsion::All => self.repulse_all(k, &mut force),
                Repulsion::Grid => self.repulse_grid(k, &mut force),
                Repulsion::Tree => self.repulse_tree(k, &mut force),
            }
            self.attract(&mut force);
            let t = t0 * (1.0 - it as f64 / iters as f64) + t0 * 0.01;
            for (i, &(fx, fy)) in force.iter().enumerate().take(n) {
                if self.fixed[i] {
                    continue;
                }
                let l = (fx * fx + fy * fy).sqrt();
                if l < 1e-12 {
                    continue;
                }
                let s = l.min(t) / l;
                self.pos[i].0 += fx * s;
                self.pos[i].1 += fy * s;
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Repulsion {
    All,
    Grid,
    Tree,
}

/// Barnes–Hut の開きの角（θ²）。
const THETA2: f64 = 0.8 * 0.8;

#[derive(Clone)]
struct QNode {
    cx: f64,
    cy: f64,
    half: f64,
    mass: f64,
    mx: f64,
    my: f64,
    /// 子の最初の番号（4 つ並ぶ）。0 は葉。
    child: u32,
    /// 葉の点（`count` が 1 のとき）。
    body: u32,
    count: u32,
    /// 深さの上限で重なった点（葉に積む）。
    extra: Vec<u32>,
}

impl QNode {
    fn contains(&self, i: u32) -> bool {
        self.body == i || self.extra.contains(&i)
    }
}

struct Quad {
    nodes: Vec<QNode>,
}

impl Quad {
    fn build(pos: &[P], mass: &[f64]) -> Quad {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in pos {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        let half = ((x1 - x0).max(y1 - y0) / 2.0).max(1.0) * 1.001;
        let root = QNode {
            cx: (x0 + x1) / 2.0,
            cy: (y0 + y1) / 2.0,
            half,
            mass: 0.0,
            mx: 0.0,
            my: 0.0,
            child: 0,
            body: u32::MAX,
            count: 0,
            extra: Vec::new(),
        };
        let mut q = Quad {
            nodes: Vec::with_capacity(pos.len() * 2 + 1),
        };
        q.nodes.push(root);
        for i in 0..pos.len() {
            q.insert(i as u32, pos, mass);
        }
        q
    }

    fn insert(&mut self, i: u32, pos: &[P], mass: &[f64]) {
        let p = pos[i as usize];
        let m = mass[i as usize];
        let mut at = 0usize;
        let mut depth = 0;
        loop {
            let node = &mut self.nodes[at];
            node.mass += m;
            node.mx += p.0 * m;
            node.my += p.1 * m;
            if node.child == 0 {
                if node.count == 0 {
                    node.body = i;
                    node.count = 1;
                    return;
                }
                if depth >= 24 {
                    node.extra.push(i);
                    node.count += 1;
                    return;
                }
                // 分ける: 前の点を子に移す
                let old = node.body;
                let (cx, cy, h) = (node.cx, node.cy, node.half / 2.0);
                let first = self.nodes.len() as u32;
                for c in 0..4 {
                    self.nodes.push(QNode {
                        cx: cx + if c & 1 == 1 { h } else { -h },
                        cy: cy + if c & 2 == 2 { h } else { -h },
                        half: h,
                        mass: 0.0,
                        mx: 0.0,
                        my: 0.0,
                        child: 0,
                        body: u32::MAX,
                        count: 0,
                        extra: Vec::new(),
                    });
                }
                let node = &mut self.nodes[at];
                node.child = first;
                node.body = u32::MAX;
                node.count = 0;
                let op = pos[old as usize];
                let om = mass[old as usize];
                let oc = Self::quadrant(&self.nodes[at], op);
                let child = &mut self.nodes[(first + oc) as usize];
                child.mass += om;
                child.mx += op.0 * om;
                child.my += op.1 * om;
                child.body = old;
                child.count = 1;
            }
            let node = &self.nodes[at];
            at = (node.child + Self::quadrant(node, p)) as usize;
            depth += 1;
        }
    }

    fn quadrant(node: &QNode, p: P) -> u32 {
        u32::from(p.0 >= node.cx) | (u32::from(p.1 >= node.cy) << 1)
    }
}

/// 大きさの半径（外接の箱の対角の半分）。
fn radius(w: f64, h: f64) -> f64 {
    (w * w + h * h).sqrt() / 2.0
}

/// 斥力の数え方（大きさで選ぶ）。
fn repulsion_for(n: usize, tree: bool) -> Repulsion {
    if n <= ALL_PAIRS {
        Repulsion::All
    } else if tree {
        Repulsion::Tree
    } else {
        Repulsion::Grid
    }
}

/// `K`（インチ）を点で。
fn spring(g: &Attrs, default: f64) -> f64 {
    g.num("K")
        .filter(|v| *v > 0.0)
        .unwrap_or(default)
        .clamp(0.01, 20.0)
        * INCH
}

// ---- fdp ----

/// 配置の単位（ノードか、中を配置したクラスタの箱）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    Node(usize),
    Cluster(usize),
}

struct Fdp<'a> {
    items: &'a [Item],
    links: &'a [Link],
    groups: &'a [Group],
    /// ノードの、根から自分のクラスタまでの列。
    chains: Vec<Vec<usize>>,
    k: f64,
    maxiter: usize,
    rng: Rng,
    budget: usize,
    /// 結果（相対の位置は呼び出しの中で足す）。
    pos: Vec<P>,
    boxes: Vec<Option<[f64; 4]>>,
}

pub fn fdp(
    items: &[Item],
    links: &[Link],
    groups: &[Group],
    g: &Attrs,
    notes: &mut BTreeSet<Note>,
) -> (Vec<P>, Vec<Option<[f64; 4]>>) {
    let n = items.len();
    let chains: Vec<Vec<usize>> = items
        .iter()
        .map(|it| {
            let mut chain = Vec::new();
            let mut c = it.cluster.filter(|&c| c < groups.len());
            while let Some(x) = c {
                chain.push(x);
                c = groups[x].parent;
                if chain.len() > groups.len() {
                    break;
                }
            }
            chain.reverse();
            chain
        })
        .collect();
    let mut f = Fdp {
        items,
        links,
        groups,
        chains,
        k: spring(g, 0.3),
        maxiter: g
            .num("maxiter")
            .map_or(600, |v| v.clamp(0.0, 10_000.0) as usize),
        rng: Rng::new(common::seed(g)),
        budget: WORK,
        pos: vec![(0.0, 0.0); n],
        boxes: vec![None; groups.len()],
    };
    let mode = common::overlap(g, Overlap::Remove, notes);
    let sep = g.get("sep").map(str::to_owned);
    f.solve(None, mode, sep.as_deref(), g);
    f.orient();
    (f.pos, f.boxes)
}

impl Fdp<'_> {
    fn unit_at(&self, v: usize, level: Option<usize>) -> Option<Unit> {
        let chain = &self.chains[v];
        let start = match level {
            None => 0,
            Some(c) => chain.iter().position(|&x| x == c)? + 1,
        };
        Some(match chain.get(start) {
            Some(&c) => Unit::Cluster(c),
            None => Unit::Node(v),
        })
    }

    /// `level` の中を配置する。中の位置は箱の中心が原点の相対。大きさ（箱）を返す。
    fn solve(&mut self, level: Option<usize>, mode: Overlap, sep: Option<&str>, g: &Attrs) -> P {
        // 単位: 中のクラスタ（中にノードがあるもの）と、直に入るノード
        let mut units: Vec<Unit> = Vec::new();
        let mut size: Vec<P> = Vec::new();
        for c in 0..self.groups.len() {
            if self.groups[c].parent == level
                && self.chains.iter().any(|ch| ch.contains(&c))
                && Some(c) != level
            {
                let s = self.solve(Some(c), mode, sep, g);
                units.push(Unit::Cluster(c));
                size.push(s);
            }
        }
        for (v, it) in self.items.iter().enumerate() {
            if it.cluster.filter(|&c| c < self.groups.len()) == level {
                units.push(Unit::Node(v));
                size.push((it.w, it.h));
            }
        }
        let m = units.len();
        let index = |u: Unit| units.iter().position(|&x| x == u);
        let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); m];
        let mut edges: Vec<(usize, usize, f64, f64)> = Vec::new();
        let r: Vec<f64> = size.iter().map(|s| radius(s.0, s.1)).collect();
        for l in self.links {
            let (Some(a), Some(b)) = (self.unit_at(l.tail, level), self.unit_at(l.head, level))
            else {
                continue;
            };
            let (Some(a), Some(b)) = (index(a), index(b)) else {
                continue;
            };
            if a == b {
                continue;
            }
            let ideal = l.len.unwrap_or(self.k) + r[a] + r[b];
            edges.push((a, b, ideal, l.weight.clamp(0.0, 100.0).max(0.01)));
            adj[a].push((b, 1.0));
            adj[b].push((a, 1.0));
        }
        // 位置の決まったノード。クラスタの中では、同じクラスタに直に入るノードどうしの位置の差を保つ（クラスタは
        // 1 つの単位として外の配置で動く。Graphviz の fdp と同じ）
        let mut fixed = vec![false; m];
        let mut pos: Vec<P> = vec![(0.0, 0.0); m];
        let mut placed = vec![false; m];
        for (k, u) in units.iter().enumerate() {
            if let Unit::Node(v) = *u
                && let Some(p) = self.items[v].pos
            {
                pos[k] = p;
                placed[k] = true;
                fixed[k] = self.items[v].pinned;
            }
        }
        let comps = common::components(&adj);
        let avg = if m == 0 {
            self.k
        } else {
            self.k + 2.0 * r.iter().sum::<f64>() / m as f64
        };
        let mut movable = Vec::with_capacity(comps.len());
        for comp in &comps {
            movable.push(!comp.iter().any(|&u| placed[u]));
            let side = (comp.len() as f64).sqrt() * avg;
            for &u in comp {
                if !placed[u] {
                    pos[u] = (self.rng.unit() * side, self.rng.unit() * side);
                }
            }
            if comp.len() < 2 {
                continue;
            }
            let mut local = vec![usize::MAX; m];
            for (k, &u) in comp.iter().enumerate() {
                local[u] = k;
            }
            let mut sys = System {
                pos: comp.iter().map(|&u| pos[u]).collect(),
                r: comp.iter().map(|&u| r[u]).collect(),
                mass: vec![1.0; comp.len()],
                fixed: comp.iter().map(|&u| fixed[u]).collect(),
                edges: edges
                    .iter()
                    .filter(|e| local[e.0] != usize::MAX)
                    .map(|e| (local[e.0], local[e.1], e.2, e.3))
                    .collect(),
            };
            let rep = repulsion_for(comp.len(), false);
            sys.run(
                self.k,
                self.maxiter,
                side / 4.0 + avg,
                rep,
                &mut self.budget,
            );
            for (k, &u) in comp.iter().enumerate() {
                pos[u] = sys.pos[k];
            }
        }
        // 重なり（クラスタの箱どうしは必ず離す）
        let half: Vec<P> = size.iter().map(|s| (s.0 / 2.0, s.1 / 2.0)).collect();
        let mut sattrs = Attrs::default();
        if let Some(s) = sep {
            sattrs.set("sep", crate::parse::Value::plain(s));
        }
        let hs = common::sep(&sattrs, &half);
        let keep_boxes =
            mode == Overlap::Keep && units.iter().any(|u| matches!(u, Unit::Cluster(_)));
        if keep_boxes {
            common::remove_overlaps(&mut pos, &hs, &fixed);
        } else {
            common::apply_overlap(mode, &mut pos, &hs, &fixed);
        }
        if comps.len() > 1 {
            common::pack(&comps, &movable, &mut pos, &half, common::pack_gap(g, 16.0));
        }
        // 箱
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for k in 0..m {
            b[0] = b[0].min(pos[k].0 - half[k].0);
            b[1] = b[1].min(pos[k].1 - half[k].1);
            b[2] = b[2].max(pos[k].0 + half[k].0);
            b[3] = b[3].max(pos[k].1 + half[k].1);
        }
        if m == 0 {
            b = [0.0; 4];
        }
        let (mut bw, mut bh) = (b[2] - b[0], b[3] - b[1]);
        let mut cx = (b[0] + b[2]) / 2.0;
        let mut cy = (b[1] + b[3]) / 2.0;
        if let Some(c) = level {
            let gr = &self.groups[c];
            let mm = gr.margin;
            let (lw, lh) = gr.label;
            bw = bw.max(lw) + 2.0 * mm;
            bh += 2.0 * mm + lh;
            // 題の分だけ中身を下に
            cy -= lh / 2.0;
            let _ = &mut cx;
        }
        // 中身を相対の位置に
        for k in 0..m {
            let d = (pos[k].0 - cx, pos[k].1 - cy);
            match units[k] {
                Unit::Node(v) => self.pos[v] = d,
                Unit::Cluster(c) => self.shift(c, d),
            }
        }
        if let Some(c) = level {
            self.boxes[c] = Some([-bw / 2.0, -bh / 2.0, bw / 2.0, bh / 2.0]);
        } else if m > 0 {
            // 根は相対のまま（呼ぶ側は bbox を数え直す）
        }
        (bw, bh)
    }

    /// 中は外を知らずに配置するので、外から順に、クラスタの中身を左右・上下に裏返したもののうち、外へ出る辺の
    /// 長さの和がいちばん短いものにする（中のクラスタは裏返さずに動かす。題の場所を保つ）。
    fn orient(&mut self) {
        let mut order: Vec<usize> = (0..self.groups.len())
            .filter(|&c| self.boxes[c].is_some())
            .collect();
        order.sort_by_key(|&c| common::depth(self.groups, c));
        for c in order {
            let Some(b) = self.boxes[c] else { continue };
            // 動かさないノードの入るクラスタは裏返さない（決めた位置の並びが鏡になる）
            if self
                .items
                .iter()
                .enumerate()
                .any(|(v, it)| it.pinned && it.pos.is_some() && self.chains[v].contains(&c))
            {
                continue;
            }
            let lh = self.groups[c].label.1;
            let center = ((b[0] + b[2]) / 2.0, (b[1] + lh + b[3]) / 2.0);
            // 直の単位とその中心
            let mut units: Vec<(Unit, P)> = Vec::new();
            for x in 0..self.groups.len() {
                if self.groups[x].parent == Some(c)
                    && let Some(xb) = self.boxes[x]
                {
                    units.push((
                        Unit::Cluster(x),
                        ((xb[0] + xb[2]) / 2.0, (xb[1] + xb[3]) / 2.0),
                    ));
                }
            }
            for (v, it) in self.items.iter().enumerate() {
                if it.cluster == Some(c) {
                    units.push((Unit::Node(v), self.pos[v]));
                }
            }
            let unit_of = |f: &Self, v: usize| -> Option<usize> {
                let u = f.unit_at(v, Some(c))?;
                units.iter().position(|x| x.0 == u)
            };
            let crossing: Vec<(usize, usize)> = self
                .links
                .iter()
                .filter_map(|l| {
                    let (a, b) = (unit_of(self, l.tail), unit_of(self, l.head));
                    match (a, b) {
                        (Some(u), None) => Some((u, l.head)),
                        (None, Some(u)) => Some((u, l.tail)),
                        _ => None,
                    }
                })
                .collect();
            if crossing.is_empty() {
                continue;
            }
            let delta = |u: usize, sx: f64, sy: f64| -> P {
                let p = units[u].1;
                (
                    center.0 + sx * (p.0 - center.0) - p.0,
                    center.1 + sy * (p.1 - center.1) - p.1,
                )
            };
            let mut best = (f64::MAX, 1.0, 1.0);
            for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                let cost: f64 = crossing
                    .iter()
                    .map(|&(u, out)| {
                        let d = delta(u, sx, sy);
                        let p = units[u].1;
                        let q = self.pos[out];
                        let (dx, dy) = (p.0 + d.0 - q.0, p.1 + d.1 - q.1);
                        (dx * dx + dy * dy).sqrt()
                    })
                    .sum();
                if cost < best.0 - 1e-9 {
                    best = (cost, sx, sy);
                }
            }
            if best.1 == 1.0 && best.2 == 1.0 {
                continue;
            }
            for (u, unit) in units.iter().enumerate() {
                let d = delta(u, best.1, best.2);
                match unit.0 {
                    Unit::Node(v) => {
                        self.pos[v].0 += d.0;
                        self.pos[v].1 += d.1;
                    }
                    Unit::Cluster(x) => self.shift(x, d),
                }
            }
        }
    }

    /// クラスタ `c` の中を `d` だけ動かす。
    fn shift(&mut self, c: usize, d: P) {
        for v in 0..self.items.len() {
            if self.chains[v].contains(&c) {
                self.pos[v].0 += d.0;
                self.pos[v].1 += d.1;
            }
        }
        for x in 0..self.groups.len() {
            if (x == c || self.is_inside(x, c))
                && let Some(b) = &mut self.boxes[x]
            {
                b[0] += d.0;
                b[1] += d.1;
                b[2] += d.0;
                b[3] += d.1;
            }
        }
    }

    fn is_inside(&self, mut x: usize, c: usize) -> bool {
        let mut steps = 0;
        while let Some(p) = self.groups[x].parent {
            if p == c {
                return true;
            }
            x = p;
            steps += 1;
            if steps > self.groups.len() {
                break;
            }
        }
        false
    }
}

// ---- sfdp ----

/// 多段の 1 段。
struct Level {
    n: usize,
    edges: Vec<(usize, usize, f64)>,
    mass: Vec<f64>,
    r: Vec<f64>,
    /// 1 つ粗い段での番号。
    up: Vec<usize>,
}

/// 隣どうしを組にして粗くする（次数の小さいものから、重さの小さい隣と）。
fn coarsen(l: &Level) -> Option<Level> {
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); l.n];
    for &(a, b, w) in &l.edges {
        adj[a].push((b, w));
        adj[b].push((a, w));
    }
    let mut order: Vec<usize> = (0..l.n).collect();
    order.sort_by_key(|&v| (adj[v].len(), v));
    let mut up = vec![usize::MAX; l.n];
    let mut next = 0;
    for &v in &order {
        if up[v] != usize::MAX {
            continue;
        }
        let partner = adj[v]
            .iter()
            .filter(|&&(u, _)| u != v && up[u] == usize::MAX)
            .min_by(|a, b| l.mass[a.0].total_cmp(&l.mass[b.0]).then(a.0.cmp(&b.0)))
            .map(|e| e.0);
        up[v] = next;
        if let Some(u) = partner {
            up[u] = next;
        }
        next += 1;
    }
    if next as f64 > l.n as f64 * 0.8 || next < 2 {
        return None;
    }
    let mut mass = vec![0.0; next];
    let mut r2 = vec![0.0; next];
    for v in 0..l.n {
        mass[up[v]] += l.mass[v];
        r2[up[v]] += l.r[v] * l.r[v];
    }
    let mut edges: Vec<(usize, usize, f64)> = l
        .edges
        .iter()
        .filter(|e| up[e.0] != up[e.1])
        .map(|&(a, b, w)| {
            let (a, b) = (up[a].min(up[b]), up[a].max(up[b]));
            (a, b, w)
        })
        .collect();
    edges.sort_by_key(|x| (x.0, x.1));
    let mut merged: Vec<(usize, usize, f64)> = Vec::with_capacity(edges.len());
    for e in edges {
        match merged.last_mut() {
            Some(last) if last.0 == e.0 && last.1 == e.1 => last.2 += e.2,
            _ => merged.push(e),
        }
    }
    Some(Level {
        n: next,
        edges: merged,
        mass,
        r: r2.into_iter().map(f64::sqrt).collect(),
        up,
    })
}

/// 近くの stress で整えるときの、1 つのノードが見る相手の数の上限・繰り返しの数・手間の上限（対の更新の数）。
const LOCAL_PEERS: usize = 32;
const LOCAL_ITERS: usize = 30;
const LOCAL_WORK: usize = 3_000_000;

/// 力の配置の仕上げ: 2 歩までの近くの対だけで stress（Gansner・Koren・North の stress majorization、重み d⁻²）を
/// 下げる。望む長さは歩数 × 今の辺の長さの中央値なので、全体の大きさと形は変えずに、近くの形（三角などの小さな
/// 輪）を整える。1 つのノードが見る相手は `LOCAL_PEERS` まで、対の更新は図ぜんぶで `LOCAL_WORK` まで。
fn local_stress(x: &mut [P], edges: &[(usize, usize, f64)]) {
    let n = x.len();
    if n < 3 || edges.is_empty() {
        return;
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut lens: Vec<f64> = Vec::with_capacity(edges.len());
    for &(a, b, _) in edges {
        if a == b || a >= n || b >= n {
            continue;
        }
        adj[a].push(b);
        adj[b].push(a);
        lens.push((x[a].0 - x[b].0).hypot(x[a].1 - x[b].1));
    }
    if lens.is_empty() {
        return;
    }
    lens.sort_by(f64::total_cmp);
    let unit = lens[lens.len() / 2];
    if !(unit.is_finite() && unit > 1e-6) {
        return;
    }
    // 相手: 隣（1 歩）と、隣の隣（2 歩）
    let mut seen = vec![usize::MAX; n];
    let peers: Vec<Vec<(usize, f64)>> = (0..n)
        .map(|v| {
            seen[v] = v;
            let mut out: Vec<(usize, f64)> = Vec::new();
            for &u in &adj[v] {
                if out.len() >= LOCAL_PEERS {
                    break;
                }
                if seen[u] != v {
                    seen[u] = v;
                    out.push((u, unit));
                }
            }
            let first = out.len();
            for k in 0..first {
                let u = out[k].0;
                for &w in &adj[u] {
                    if out.len() >= LOCAL_PEERS {
                        break;
                    }
                    if seen[w] != v {
                        seen[w] = v;
                        out.push((w, 2.0 * unit));
                    }
                }
            }
            out
        })
        .collect();
    let pairs: usize = peers.iter().map(Vec::len).sum::<usize>().max(1);
    let iters = LOCAL_ITERS.min(LOCAL_WORK / pairs);
    if iters < LOCAL_ITERS {
        crate::cut_short();
    }
    for _ in 0..iters {
        for v in 0..n {
            let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
            for &(u, d) in &peers[v] {
                let w = 1.0 / (d * d);
                let (dx, dy) = (x[v].0 - x[u].0, x[v].1 - x[u].1);
                let l = dx.hypot(dy);
                let (ox, oy) = if l > 1e-9 {
                    (d * dx / l, d * dy / l)
                } else {
                    (0.0, 0.0)
                };
                sx += w * (x[u].0 + ox);
                sy += w * (x[u].1 + oy);
                sw += w;
            }
            if sw > 0.0 {
                x[v] = (sx / sw, sy / sw);
            }
        }
    }
}

pub fn sfdp(items: &[Item], links: &[Link], g: &Attrs, notes: &mut BTreeSet<Note>) -> Vec<P> {
    let n = items.len();
    if n == 0 {
        return vec![];
    }
    if items.iter().any(|it| it.pos.is_some()) {
        notes.insert(Note::LayoutOptions);
    }
    let k = spring(g, 0.3);
    let half = common::halves(items);
    let adj = common::adjacency(n, links, k);
    let comps = common::components(&adj);
    let mut rng = Rng::new(common::seed(g));
    let mut budget = WORK;
    let mut pos = vec![(0.0, 0.0); n];
    let mut local = vec![usize::MAX; n];
    for comp in &comps {
        if comp.len() < 2 {
            continue;
        }
        for (i, &v) in comp.iter().enumerate() {
            local[v] = i;
        }
        let mut edges: Vec<(usize, usize, f64)> = Vec::new();
        for l in links {
            if l.tail == l.head || local[l.tail] == usize::MAX || local[l.head] == usize::MAX {
                continue;
            }
            edges.push((
                local[l.tail],
                local[l.head],
                l.weight.clamp(0.0, 100.0).max(0.01),
            ));
        }
        let base = Level {
            n: comp.len(),
            edges,
            mass: vec![1.0; comp.len()],
            r: comp
                .iter()
                .map(|&v| radius(items[v].w, items[v].h))
                .collect(),
            up: vec![],
        };
        let mut levels = vec![base];
        while levels.len() < 20 && levels[levels.len() - 1].n > 30 {
            match coarsen(&levels[levels.len() - 1]) {
                Some(c) => levels.push(c),
                None => break,
            }
        }
        let n0 = comp.len() as f64;
        let r_avg = levels[0].r.iter().sum::<f64>() / n0;
        // いちばん粗い段から
        let top = levels.len() - 1;
        let spacing = |lv: &Level| (k + 2.0 * r_avg) * (n0 / lv.n as f64).sqrt();
        let side = (levels[top].n as f64).sqrt() * spacing(&levels[top]);
        // いちばん粗い段の初めは stress の配置（乱数の初めだと三角が 1 本の線に畳まれることがある）
        let mut x: Vec<P> = {
            let lv = &levels[top];
            let dummy: Vec<Item> = (0..lv.n)
                .map(|_| Item {
                    w: 0.0,
                    h: 0.0,
                    cluster: None,
                    pos: None,
                    pinned: false,
                    area: 1.0,
                    root: false,
                    sortv: None,
                })
                .collect();
            let ls: Vec<Link> = lv
                .edges
                .iter()
                .map(|&(a, b, _)| Link {
                    tail: a,
                    head: b,
                    len: None,
                    weight: 1.0,
                })
                .collect();
            let s = spacing(lv) / INCH;
            super::stress::layout(&dummy, &ls, &Attrs::default(), false, &mut BTreeSet::new())
                .into_iter()
                .map(|p| (p.0 * s, p.1 * s))
                .collect()
        };
        let _ = side;
        for li in (0..=top).rev() {
            let lv = &levels[li];
            if li < top {
                // 細かい段へ: 粗いノードの位置の近くに
                let up = &levels[li + 1].up;
                let jitter = spacing(lv) * 0.1;
                x = (0..lv.n)
                    .map(|v| {
                        let p = x[up[v]];
                        (
                            p.0 + (rng.unit() - 0.5) * jitter,
                            p.1 + (rng.unit() - 0.5) * jitter,
                        )
                    })
                    .collect();
            }
            let sp = spacing(lv);
            let kk = (sp - 2.0 * r_avg).max(1.0);
            let mut sys = System {
                pos: x,
                r: vec![r_avg; lv.n],
                mass: lv.mass.iter().map(|m| m.sqrt()).collect(),
                fixed: vec![false; lv.n],
                edges: lv.edges.iter().map(|&(a, b, w)| (a, b, sp, w)).collect(),
            };
            let iters = if li == top { 300 } else { 60 };
            let t0 = sp;
            sys.run(kk, iters, t0, repulsion_for(lv.n, true), &mut budget);
            x = sys.pos;
        }
        // いちばん細かい段の形を、近くの stress で整える（小さな輪が平たく畳まれたままにならないように）
        local_stress(&mut x, &levels[0].edges);
        for (i, &v) in comp.iter().enumerate() {
            pos[v] = x[i];
        }
        for &v in comp {
            local[v] = usize::MAX;
        }
    }
    // 辺の長さの中央値を、半径を足した望む長さの 1.3 倍に（多くの斥力で縮んだ分を戻す）
    let r_all: f64 = items.iter().map(|it| radius(it.w, it.h)).sum::<f64>() / n as f64;
    let mut lens: Vec<f64> = links
        .iter()
        .filter(|l| l.tail != l.head)
        .map(|l| {
            let (a, b) = (pos[l.tail], pos[l.head]);
            ((a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1)).sqrt()
        })
        .filter(|d| *d > 1e-6)
        .collect();
    if !lens.is_empty() {
        lens.sort_by(f64::total_cmp);
        let s = 1.3 * (k + 2.0 * r_all) / lens[lens.len() / 2];
        if s.is_finite() && s > 0.0 {
            for p in &mut pos {
                p.0 *= s;
                p.1 *= s;
            }
        }
    }
    if comps.len() > 1 {
        let movable = vec![true; comps.len()];
        common::pack(&comps, &movable, &mut pos, &half, common::pack_gap(g, 16.0));
    }
    let mode = common::overlap(g, Overlap::Remove, notes);
    let hs = common::sep(g, &half);
    common::apply_overlap(mode, &mut pos, &hs, &vec![false; n]);
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(cluster: Option<usize>) -> Item {
        Item {
            w: 54.0,
            h: 36.0,
            cluster,
            pos: None,
            pinned: false,
            area: 1.0,
            root: false,
            sortv: None,
        }
    }

    fn link(t: usize, h: usize) -> Link {
        Link {
            tail: t,
            head: h,
            len: None,
            weight: 1.0,
        }
    }

    fn overlaps(p: &[P], w: f64, h: f64) -> bool {
        (0..p.len()).any(|i| {
            (i + 1..p.len())
                .any(|j| (p[i].0 - p[j].0).abs() < w - 0.5 && (p[i].1 - p[j].1).abs() < h - 0.5)
        })
    }

    #[test]
    fn fdp_keeps_clusters_apart() {
        let groups = vec![
            Group {
                parent: None,
                margin: 8.0,
                label: (20.0, 18.0),
                sortv: None,
            };
            2
        ];
        let items = vec![
            item(Some(0)),
            item(Some(0)),
            item(Some(1)),
            item(Some(1)),
            item(None),
        ];
        let links = vec![link(0, 1), link(2, 3), link(1, 2), link(4, 0)];
        let (p, boxes) = fdp(
            &items,
            &links,
            &groups,
            &Attrs::default(),
            &mut BTreeSet::new(),
        );
        let (a, b) = (boxes[0].unwrap(), boxes[1].unwrap());
        assert!(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1]);
        for (i, c) in [(0, a), (1, a), (2, b), (3, b)] {
            assert!(p[i].0 > c[0] && p[i].0 < c[2] && p[i].1 > c[1] && p[i].1 < c[3]);
        }
        assert!(!overlaps(&p, 54.0, 36.0));
    }

    #[test]
    fn fdp_pins_nodes_inside_clusters() {
        let groups = vec![Group {
            parent: None,
            margin: 8.0,
            label: (20.0, 18.0),
            sortv: None,
        }];
        let mut items = vec![item(Some(0)), item(Some(0)), item(Some(0)), item(None)];
        items[0].pos = Some((0.0, 0.0));
        items[0].pinned = true;
        items[1].pos = Some((144.0, -72.0));
        items[1].pinned = true;
        let links = vec![link(0, 1), link(1, 2), link(2, 3), link(3, 0)];
        let (p, boxes) = fdp(
            &items,
            &links,
            &groups,
            &Attrs::default(),
            &mut BTreeSet::new(),
        );
        // 2 つの動かさないノードの位置の差はそのまま、どちらもクラスタの箱の中
        let d = (p[1].0 - p[0].0, p[1].1 - p[0].1);
        assert!(
            (d.0 - 144.0).abs() < 1e-6 && (d.1 + 72.0).abs() < 1e-6,
            "{d:?}"
        );
        let b = boxes[0].unwrap();
        for q in &p[..3] {
            assert!(q.0 > b[0] && q.0 < b[2] && q.1 > b[1] && q.1 < b[3]);
        }
    }

    #[test]
    fn sfdp_keeps_small_cycles_open() {
        // a -- {b c d e}; b -- {f g}; c -- {h i}; d -- j; e -- {k l m}; m -- a（a・e・m の三角）
        let items = vec![item(None); 13];
        let pairs = [
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4),
            (1, 5),
            (1, 6),
            (2, 7),
            (2, 8),
            (3, 9),
            (4, 10),
            (4, 11),
            (4, 12),
            (12, 0),
        ];
        let links: Vec<Link> = pairs.iter().map(|&(a, b)| link(a, b)).collect();
        let p = sfdp(&items, &links, &Attrs::default(), &mut BTreeSet::new());
        let (a, e, m) = (p[0], p[4], p[12]);
        let area = ((e.0 - a.0) * (m.1 - a.1) - (m.0 - a.0) * (e.1 - a.1)).abs() / 2.0;
        let side = [(a, e), (e, m), (m, a)]
            .iter()
            .map(|(x, y)| (x.0 - y.0).hypot(x.1 - y.1))
            .fold(0.0, f64::max);
        // 正三角形は 0.43。平たいと 0 に近い
        assert!(area > 0.2 * side * side, "{area} {side}");
    }

    #[test]
    fn sfdp_spreads_a_grid() {
        // 20×20 の格子
        let items = vec![item(None); 400];
        let mut links = Vec::new();
        for y in 0..20 {
            for x in 0..20 {
                let v = y * 20 + x;
                if x + 1 < 20 {
                    links.push(link(v, v + 1));
                }
                if y + 1 < 20 {
                    links.push(link(v, v + 20));
                }
            }
        }
        let p = sfdp(&items, &links, &Attrs::default(), &mut BTreeSet::new());
        assert!(!overlaps(&p, 54.0, 36.0));
        // 角どうしは隣より遠い
        let d = |a: usize, b: usize| (p[a].0 - p[b].0).hypot(p[a].1 - p[b].1);
        assert!(d(0, 399) > 5.0 * d(0, 1), "{} {}", d(0, 399), d(0, 1));
    }
}
