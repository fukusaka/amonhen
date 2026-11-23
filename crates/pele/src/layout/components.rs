//! つながっていない部分を行に並べ直す（pele の `src/layout/components.ts`）。

use super::compound::{CEdge, CNode, CompoundResult};
use crate::js;

struct Part {
    nodes: Vec<usize>,
    edges: Vec<usize>,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Part {
    fn include(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.left = js::min(self.left, x - w / 2.0);
        self.top = js::min(self.top, y - h / 2.0);
        self.right = js::max(self.right, x + w / 2.0);
        self.bottom = js::max(self.bottom, y + h / 2.0);
    }
}

fn join(parent: &mut [usize], a: usize, b: usize) {
    let ra = root(parent, a);
    let rb = root(parent, b);
    parent[ra] = rb;
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// つながっていない部分を、部分の中の配置を変えずに `width` の行に並べる。並べ直しても狭くならなければ `None`。
pub fn wrap_components(
    nodes: &mut [CNode],
    edges: &mut [CEdge],
    width: f64,
    gap: f64,
) -> Option<CompoundResult> {
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    for (i, c) in nodes.iter().enumerate() {
        if c.parent >= 0 {
            join(&mut parent, i, c.parent as usize);
        }
    }
    for e in edges.iter() {
        join(&mut parent, e.src, e.dst);
    }

    // Map の入れた順。
    let mut parts: Vec<Part> = Vec::new();
    let mut part_of: Vec<Option<usize>> = vec![None; nodes.len()];
    for (i, c) in nodes.iter().enumerate() {
        let id = root(&mut parent, i);
        let k = *part_of[id].get_or_insert_with(|| {
            parts.push(Part {
                nodes: Vec::new(),
                edges: Vec::new(),
                left: f64::INFINITY,
                top: f64::INFINITY,
                right: f64::NEG_INFINITY,
                bottom: f64::NEG_INFINITY,
            });
            parts.len() - 1
        });
        let part = &mut parts[k];
        part.nodes.push(i);
        part.include(c.x, c.y, c.w, c.h);
    }
    if parts.len() < 2 {
        return None;
    }
    for (ei, e) in edges.iter().enumerate() {
        let k = part_of[root(&mut parent, e.src)].expect("part");
        let part = &mut parts[k];
        part.edges.push(ei);
        let mut i = 0;
        while i < e.route.len() {
            part.include(e.route[i], e.route[i + 1], 0.0, 0.0);
            i += 3;
        }
        if e.label_w > 0.0 {
            part.include(e.label_x, e.label_y, e.label_w, e.label_h);
        }
    }

    let (mut x, mut y, mut row_height, mut right) = (0.0, 0.0, 0.0, 0.0);
    let (mut from, mut to) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut shifts: Vec<(f64, f64)> = Vec::new();
    for part in &parts {
        let w = part.right - part.left;
        let h = part.bottom - part.top;
        if x > 0.0 && x + w > width {
            y += row_height + gap;
            x = 0.0;
            row_height = 0.0;
        }
        shifts.push((x - part.left, y - part.top));
        from = js::min(from, part.left);
        to = js::max(to, part.right);
        right = js::max(right, x + w);
        row_height = js::max(row_height, h);
        x += w + gap;
    }
    if right >= to - from {
        return None;
    }
    for (part, &(ox, oy)) in parts.iter().zip(&shifts) {
        for &i in &part.nodes {
            nodes[i].x += ox;
            nodes[i].y += oy;
        }
        for &ei in &part.edges {
            let e = &mut edges[ei];
            let mut k = 0;
            while k < e.route.len() {
                e.route[k] += ox;
                e.route[k + 1] += oy;
                k += 3;
            }
            e.label_x += ox;
            e.label_y += oy;
        }
    }
    Some(CompoundResult {
        width: right,
        height: y + row_height,
    })
}
