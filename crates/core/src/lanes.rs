//! 見えている範囲に合わせたレーンの横位置（魚眼）。
//!
//! linux のような履歴では、1 行を 300 本以上の線が横切る（日付順でもトポロジー順でも変わらない）。
//! すべてのレーンを同じ幅で並べると、ほとんどのコミットの点が列の外に出てしまう。そこで、見えている
//! 範囲に点があるレーンだけを広く取り、ほかの線（素通りするものや、点のレーンへ曲がってくるもの）は
//! 細い束に詰め、何もないレーンは幅を取らない。範囲が変わったときの動きは FE がアニメーションでつなぐ。

use std::ops::Range;

use crate::graph::GraphLayout;

/// レーンの幅の決め方（単位は FE の座標、macOS ならポイント）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneMetrics {
    /// 点のあるレーンの幅。
    pub full: f32,
    /// 線だけのレーンの幅。
    pub thin: f32,
    /// 収まらないときに、ここまで詰める。
    pub min_full: f32,
    pub min_thin: f32,
    /// 使える幅。
    pub available: f32,
}

const EMPTY: u8 = 0;
const PASS: u8 = 1;
const ACTIVE: u8 = 2;

/// `rows` を表示するときの、レーンごとの中心の横位置（左端を 0 とする）。何もないレーンは幅を取らず、
/// 次のレーンと同じ位置になる（アニメーションで点が湧き出るように見せるため、位置は必ず持たせる）。
pub fn lane_positions(layout: &GraphLayout, rows: Range<u32>, m: &LaneMetrics) -> Vec<f32> {
    let mut x = 0.0;
    lane_widths(layout, rows, m)
        .into_iter()
        .map(|width| {
            let center = x + width / 2.0;
            x += width;
            center
        })
        .collect()
}

/// レーンごとの幅。
fn lane_widths(layout: &GraphLayout, rows: Range<u32>, m: &LaneMetrics) -> Vec<f32> {
    let lanes = layout.lane_count() as usize;
    let mut activity = vec![EMPTY; lanes];
    if lanes == 0 {
        return Vec::new();
    }
    // 幹線は常に広く取る（スクロールしても左端が動かないように）。
    activity[0] = ACTIVE;
    let rows = rows.start..rows.end.min(layout.nodes().len() as u32);
    for n in &layout.nodes()[rows.start as usize..rows.end as usize] {
        activity[n.lane as usize] = ACTIVE;
    }
    for i in layout.edges_in(rows.clone()) {
        let e = &layout.edges()[i as usize];
        for lane in [e.lane_start, e.lane_end] {
            let a = &mut activity[lane as usize];
            *a = (*a).max(PASS);
        }
    }

    let active = activity.iter().filter(|&&a| a == ACTIVE).count() as f32;
    let pass = activity.iter().filter(|&&a| a == PASS).count() as f32;
    let (mut full, mut thin) = (m.full, m.thin);
    // 収まらなければ、まず線だけのレーンを、それでも足りなければ点のあるレーンを詰める。
    if active * full + pass * thin > m.available && pass > 0.0 {
        thin = ((m.available - active * full) / pass).clamp(m.min_thin, m.thin);
    }
    if active * full + pass * thin > m.available {
        full = ((m.available - pass * thin) / active).clamp(m.min_full, m.full);
    }
    activity
        .into_iter()
        .map(|a| match a {
            ACTIVE => full,
            PASS => thin,
            _ => 0.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{layout, synthetic};

    const M: LaneMetrics = LaneMetrics {
        full: 14.0,
        thin: 3.0,
        min_full: 6.0,
        min_thin: 0.0,
        available: 10_000.0,
    };

    #[test]
    fn lanes_with_nodes_are_wide_lanes_with_only_lines_are_thin_and_others_take_no_space() {
        let (graph, head) = synthetic::graph(3000, 7);
        let l = layout(&graph, head);
        let rows = 250..280;
        let widths = lane_widths(&l, rows.clone(), &M);
        assert_eq!(widths.len(), l.lane_count() as usize);
        let node_lanes: Vec<usize> = l.nodes()[250..280]
            .iter()
            .map(|n| n.lane as usize)
            .collect();
        let line_lanes: Vec<usize> = l
            .edges_in(rows)
            .flat_map(|i| {
                let e = l.edges()[i as usize];
                [e.lane_start as usize, e.lane_end as usize]
            })
            .collect();
        for (lane, &w) in widths.iter().enumerate() {
            let expected = if lane == 0 || node_lanes.contains(&lane) {
                14.0
            } else if line_lanes.contains(&lane) {
                3.0
            } else {
                0.0
            };
            assert_eq!(w, expected, "レーン {lane}");
        }
        assert!(
            widths.contains(&3.0) && widths.contains(&0.0),
            "この範囲には線だけのレーンも空のレーンもある"
        );
        // 中心は、左のレーンの幅の合計 + 自分の幅の半分。
        let x = lane_positions(&l, 250..280, &M);
        assert_eq!(x[1], widths[0] + widths[1] / 2.0);
    }

    #[test]
    fn squeezes_lines_first_then_nodes_to_fit() {
        let (graph, head) = synthetic::graph(3000, 7);
        let l = layout(&graph, head);
        let all = lane_widths(&l, 250..280, &M);
        let nodes = all.iter().filter(|&&w| w == 14.0).count() as f32;
        // 点のレーンは収まるが線のレーンは収まらない幅: 線のレーンだけ細くなる。
        let narrow = LaneMetrics {
            available: nodes * 14.0 + 1.0,
            ..M
        };
        let w = lane_widths(&l, 250..280, &narrow);
        assert!((w.iter().sum::<f32>() - narrow.available).abs() < 0.01);
        assert_eq!(w.iter().filter(|&&w| w == 14.0).count() as f32, nodes);
        // 点のレーンも収まらない幅: 点のレーンも詰めるが、最小の幅より狭くはしない。
        let tiny = LaneMetrics {
            available: 10.0,
            ..M
        };
        let w = lane_widths(&l, 250..280, &tiny);
        assert!(w.iter().all(|&w| w == 0.0 || w >= 6.0));
    }
}
