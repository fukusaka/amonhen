//! History のグラフを GSK のパスで描く。
//!
//! 点・線・曲線は、色・太さ・不透明度が同じものを 1 つのパスにまとめ、まとめた数だけ stroke / fill する
//! （GSK は GPU でパスを描くので、命令の数を少なく保つ）。形は macOS 版（`GraphRenderer.swift`）と同じで、
//! 縦線は区間 1 つ、レーンをまたぐ所は縦向きの接線を持つ 3 次ベジェ曲線。

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::Instant;

use amonhen_core::HistorySnapshot;
use amonhen_core::lanes::{LaneMetrics, lane_positions};
use amonhen_core::lineage::{Lineage, NOT_LIT};
use amonhen_layout::Edge;
use gtk::prelude::*;
use gtk::{gdk, graphene, gsk};

pub struct Metrics {
    /// History の行の高さ（件名と、作者・日時の 2 行）。
    pub row_height: f32,
    pub lane_width: f32,
    pub left_inset: f32,
    pub node_radius: f32,
    pub line_width: f32,
    pub first_parent_width: f32,
    /// 線だけのレーンの幅（魚眼で詰めたとき）。入りきらなければ 0 まで詰めて、薄い帯にする。
    pub thin_lane_width: f32,
    pub min_lane_width: f32,
    pub min_thin_lane_width: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            row_height: 38.0,
            lane_width: 14.0,
            left_inset: 10.0,
            node_radius: 4.0,
            line_width: 1.6,
            first_parent_width: 2.4,
            thin_lane_width: 3.0,
            min_lane_width: 7.0,
            min_thin_lane_width: 0.0,
        }
    }
}

/// 色は `color % PALETTE.len()` で選ぶ。0 は幹線。
const PALETTE: [[f32; 3]; 8] = [
    [0.30, 0.55, 0.95],
    [0.93, 0.45, 0.30],
    [0.35, 0.75, 0.45],
    [0.75, 0.45, 0.85],
    [0.95, 0.70, 0.25],
    [0.30, 0.75, 0.80],
    [0.90, 0.40, 0.60],
    [0.60, 0.60, 0.65],
];

fn color(index: u16, alpha: f32) -> gdk::RGBA {
    let [r, g, b] = PALETTE[index as usize % PALETTE.len()];
    gdk::RGBA::new(r, g, b, alpha)
}

/// グラフの右の余白（列の幅に足す）。
const RIGHT_INSET: f32 = 4.0;

/// レーンの横位置と列の幅。見えている範囲が変わったら、コアの決めた位置と、見えているレーンの分の幅へ約 0.15 秒で
/// 近づける（macOS 版の `GraphRenderer` と同じ）。
#[derive(Default)]
pub struct Lanes {
    current: Vec<f32>,
    target: Vec<f32>,
    current_width: f32,
    target_width: f32,
    key: Option<(u64, Range<u32>, u32)>,
    last_advance: Option<Instant>,
    pub animating: bool,
}

impl Lanes {
    /// `rows` を、幅 `width` まで使える列に描くときの位置と列の幅に近づける。`animate` でなければすぐ合わせる。
    pub fn update(
        &mut self,
        history: &HistorySnapshot,
        rows: Range<u32>,
        width: f32,
        m: &Metrics,
        animate: bool,
    ) {
        // 少し先の行まで含めて決め、スクロールで点が入ってくる直前に広げ始める。
        let margin = 4;
        let count = history.layout.nodes().len() as u32;
        let window = rows.start.saturating_sub(margin)..(rows.end + margin).min(count);
        let key = (history.generation, window.clone(), width.to_bits());
        let history_changed = self.key.as_ref().is_none_or(|k| k.0 != key.0);
        if self.key.as_ref() != Some(&key) {
            let slack = (m.left_inset - m.lane_width / 2.0) + RIGHT_INSET;
            let metrics = LaneMetrics {
                full: m.lane_width,
                thin: m.thin_lane_width,
                min_full: m.min_lane_width,
                min_thin: m.min_thin_lane_width,
                available: width - slack,
            };
            let (target, total) = lane_positions(&history.layout, window, &metrics);
            self.target = target;
            // 列は見えているレーンの分だけ取る（幹線の分は必ず取る）。
            self.target_width = (slack + total.max(m.lane_width)).min(width);
            self.key = Some(key);
            // 別の History に替わったときは、動かさずにその位置から始める。
            if history_changed || self.current.len() != self.target.len() {
                self.current = self.target.clone();
                self.current_width = self.target_width;
            }
        }
        let now = Instant::now();
        let dt = self
            .last_advance
            .map_or(0.0, |t| (now - t).as_secs_f32())
            .min(0.05);
        self.last_advance = Some(now);
        if !animate {
            self.current.clone_from(&self.target);
            self.current_width = self.target_width;
            self.animating = false;
            return;
        }
        // 約 0.15 秒で近づく指数的な動き。
        let k = (dt * 18.0).min(1.0);
        let mut moving = false;
        // 列は、広がるときはレーンと同じ速さで（点が列からはみ出ないように）、狭まるときはゆっくり（スクロール中に
        // レーンの数が行き来しても、文字が左右に揺れないように。macOS 版と同じ）。
        let d = self.target_width - self.current_width;
        if d.abs() < 0.05 {
            self.current_width = self.target_width;
        } else {
            self.current_width += d * if d > 0.0 { k } else { (dt * 4.0).min(1.0) };
            moving = true;
        }
        for (c, t) in self.current.iter_mut().zip(&self.target) {
            let d = t - *c;
            if d.abs() < 0.05 {
                *c = *t;
            } else {
                *c += d * k;
                moving = true;
            }
        }
        self.animating = moving;
    }

    /// 今の列の幅（文字の位置がにじまないように整数にする）。
    pub fn width(&self) -> f32 {
        self.current_width.round()
    }

    pub fn x(&self, lane: u16, m: &Metrics) -> f32 {
        let i = lane as usize;
        let x = self
            .current
            .get(i)
            .copied()
            .unwrap_or(i as f32 * m.lane_width + m.lane_width / 2.0);
        m.left_inset - m.lane_width / 2.0 + x
    }

    /// レーンの今の幅（隣のレーンとの間隔）。
    fn spacing(&self, lane: u16, m: &Metrics) -> f32 {
        let i = lane as usize;
        if i >= self.current.len() {
            return m.lane_width;
        }
        let left = if i > 0 {
            self.current[i] - self.current[i - 1]
        } else {
            m.lane_width
        };
        let right = self
            .current
            .get(i + 1)
            .map_or(m.lane_width, |x| x - self.current[i]);
        left.min(right)
    }
}

/// 同じ見た目でまとめるときの鍵（色、太さと不透明度は 1/100 単位）。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Style {
    color: u16,
    width: u16,
    alpha: u16,
}

impl Style {
    fn new(color: u16, width: f32, alpha: f32) -> Self {
        Self {
            color: color % PALETTE.len() as u16,
            width: (width * 100.0).round() as u16,
            alpha: (alpha * 100.0).round() as u16,
        }
    }
}

/// 祖先を強調するときの、ほかの点と線の不透明度（macOS 版と同じ）。
const DIMMED_ALPHA: f32 = 0.18;

/// `rows` 行の点と線を `snapshot` に描く。y は `scroll_y` だけずらす。`lineage` があれば、祖先の点と
/// それをつなぐ線のほかは薄く描く。`boundary`（shallow clone の境界のコミットの行）の点からは、下へ短い点線を
/// 描く（親はあるが取得していない。macOS 版の `appendUnfetchedStub`）。行ごとのグラフの右端（その行の点と、その行を
/// 通る線のいちばん右。macOS 版の `rightExtent`）を返す。文字はそのすぐ右から始める。
#[allow(clippy::too_many_arguments)]
pub fn draw(
    snapshot: &gtk::Snapshot,
    history: &HistorySnapshot,
    lanes: &Lanes,
    rows: Range<u32>,
    scroll_y: f32,
    m: &Metrics,
    lineage: Option<&Lineage>,
    boundary: &HashSet<u32>,
) -> Vec<f32> {
    let row_y = |row: u32| (row as f32 + 0.5) * m.row_height - scroll_y;
    // 幹線の点の分は必ず取る。
    let mut extents = vec![lanes.x(0, m) + m.node_radius; rows.len()];
    let mut strokes: HashMap<Style, gsk::PathBuilder> = HashMap::new();
    let edges = history.layout.edges();
    for i in history.layout.edges_in(rows.clone()) {
        let e = &edges[i as usize];
        let (ax, ay) = (lanes.x(e.lane_start, m), row_y(e.row_start));
        let (bx, by) = (lanes.x(e.lane_end, m), row_y(e.row_end));
        // 線は 2 つのレーンの間に収まる（曲線もその間）。通る行の右端を広げる。
        let (lower, upper) = (
            e.row_start.max(rows.start),
            e.row_end.min(rows.end.saturating_sub(1)),
        );
        if lower <= upper && !rows.is_empty() {
            let right = ax.max(bx) + m.first_parent_width / 2.0;
            for r in lower..=upper {
                let x = &mut extents[(r - rows.start) as usize];
                *x = x.max(right);
            }
        }
        let mut width = if e.kind & Edge::FIRST_PARENT != 0 {
            m.first_parent_width
        } else {
            m.line_width
        };
        let mut alpha = 1.0;
        // 詰めたレーンの線は、間隔に合わせて細く薄くし、点を隠さない帯にする。
        let spacing = lanes
            .spacing(e.lane_start, m)
            .max(lanes.spacing(e.lane_end, m));
        if spacing < m.lane_width / 2.0 {
            width = width.min((spacing * 0.8).max(0.6));
            alpha = 0.45;
        }
        // 祖先を強調するときは、光らない線（や縦線の光らない上の部分）を薄くする。
        let from = lineage.map_or(e.row_start, |l| l.edge_from[i as usize]);
        if from == NOT_LIT {
            alpha *= DIMMED_ALPHA;
        } else if from > e.row_start && e.lane_start == e.lane_end {
            let my = row_y(from);
            let dim = strokes
                .entry(Style::new(e.color, width, alpha * DIMMED_ALPHA))
                .or_default();
            dim.move_to(ax, ay);
            dim.line_to(ax, my);
            let lit = strokes
                .entry(Style::new(e.color, width, alpha))
                .or_default();
            lit.move_to(ax, my);
            lit.line_to(bx, by);
            continue;
        }
        let path = strokes
            .entry(Style::new(e.color, width, alpha))
            .or_default();
        path.move_to(ax, ay);
        if e.lane_start == e.lane_end {
            path.line_to(bx, by);
        } else {
            let my = (ay + by) / 2.0;
            path.cubic_to(ax, my, bx, my, bx, by);
        }
    }
    for (style, path) in strokes {
        let stroke = gsk::Stroke::new(style.width as f32 / 100.0);
        stroke.set_line_cap(gsk::LineCap::Round);
        snapshot.append_stroke(
            &path.to_path(),
            &stroke,
            &color(style.color, style.alpha as f32 / 100.0),
        );
    }

    let mut fills: HashMap<(u16, bool), gsk::PathBuilder> = HashMap::new();
    let mut stubs: HashMap<(u16, bool), gsk::PathBuilder> = HashMap::new();
    let nodes = history.layout.nodes();
    for n in &nodes[rows.start as usize..rows.end as usize] {
        if n.flags & amonhen_layout::Node::PENDING != 0 {
            continue;
        }
        let p = graphene::Point::new(lanes.x(n.lane, m), row_y(n.row));
        if let Some(x) = n
            .row
            .checked_sub(rows.start)
            .and_then(|i| extents.get_mut(i as usize))
        {
            *x = x.max(p.x() + m.node_radius);
        }
        let dimmed = lineage.is_some_and(|l| !l.contains(n.row));
        fills
            .entry((n.color % PALETTE.len() as u16, dimmed))
            .or_default()
            .add_circle(&p, m.node_radius);
        if boundary.contains(&n.row) {
            let dots = stubs
                .entry((n.color % PALETTE.len() as u16, dimmed))
                .or_default();
            for y in unfetched_stub(m) {
                dots.add_circle(&graphene::Point::new(p.x(), p.y() + y), 1.1);
            }
        }
    }
    for ((c, dimmed), path) in fills {
        let alpha = if dimmed { DIMMED_ALPHA } else { 1.0 };
        snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, &color(c, alpha));
    }
    for ((c, dimmed), path) in stubs {
        let alpha = if dimmed { DIMMED_ALPHA } else { 1.0 } * 0.8;
        snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, &color(c, alpha));
    }
    extents
}

/// shallow clone の境界のコミットの点から下へ描く点線の、点の位置（点の中心からの下向きの距離）。行の下半分に
/// 収める（macOS 版と同じく、点の縁から 3.5 離して 4 ずつ）。
fn unfetched_stub(m: &Metrics) -> Vec<f32> {
    let mut ys = Vec::new();
    let mut y = m.node_radius + 3.5;
    while y <= m.row_height / 2.0 + 1.0 {
        ys.push(y);
        y += 4.0;
    }
    ys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unfetched_stub_stays_in_the_lower_half_of_the_row() {
        let m = Metrics::default();
        let ys = unfetched_stub(&m);
        assert!(!ys.is_empty());
        assert!(ys[0] > m.node_radius);
        assert!(ys.iter().all(|&y| y <= m.row_height / 2.0 + 1.0));
    }
}
