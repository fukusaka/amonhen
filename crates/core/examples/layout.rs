//! 実際のリポジトリで History の読み込みとレイアウトにかかる時間を測る。
//! `cargo run --release -p amonhen-core --example layout -- <repo>`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::graph::layout;
use amonhen_model::GitBackend;

fn main() -> amonhen_model::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let backend = GixBackend::open(&path)?;

    let t = Instant::now();
    let tips = backend.tips()?;
    let head = backend.head()?;
    let raw = backend.read_commits(&tips, &|_| false)?;
    let read = t.elapsed();
    let t = Instant::now();
    let graph = amonhen_model::topo::sort(raw);
    let sort = t.elapsed();
    let walk = read + sort;
    println!("read: {read:?}  sort: {sort:?}");

    let head_row = head
        .and_then(|h| graph.ids().iter().position(|id| *id == h))
        .map(|r| r as u32);
    let t = Instant::now();
    let l = layout(&graph, head_row);
    let lay = t.elapsed();

    println!(
        "commits: {}  walk: {walk:?}  layout: {lay:?}  lanes: {}  edges: {}",
        graph.len(),
        l.lane_count(),
        l.edges().len()
    );
    print_widths(&l);
    Ok(())
}

/// 行ごとのグラフの幅（その行に点か線があるいちばん右のレーン + 1）の分布。
fn print_widths(l: &amonhen_core::graph::GraphLayout) {
    let rows = l.nodes().len();
    if rows == 0 {
        return;
    }
    // 線の区間の始まりと終わりで、レーンごとの本数を増減する。
    let mut starts: Vec<Vec<u16>> = vec![Vec::new(); rows + 1];
    let mut ends: Vec<Vec<u16>> = vec![Vec::new(); rows + 1];
    for e in l.edges() {
        let lane = e.lane_start.max(e.lane_end);
        starts[e.row_start as usize].push(lane);
        ends[e.row_end as usize + 1].push(lane);
    }
    let mut count = vec![0u32; l.lane_count() as usize + 1];
    let mut max = 0usize;
    let mut widths = Vec::with_capacity(rows);
    let mut occupied = Vec::with_capacity(rows);
    let mut open = 0i64;
    for (row, node) in l.nodes().iter().enumerate() {
        for &lane in &ends[row] {
            count[lane as usize] -= 1;
            open -= 1;
        }
        for &lane in &starts[row] {
            count[lane as usize] += 1;
            open += 1;
            max = max.max(lane as usize);
        }
        while max > 0 && count[max] == 0 {
            max -= 1;
        }
        let edge_width = if count[max] > 0 { max + 1 } else { 0 };
        widths.push(edge_width.max(node.lane as usize + 1));
        occupied.push(open as usize);
    }
    occupied.sort_unstable();
    let occ = |p: f64| occupied[((rows - 1) as f64 * p) as usize];
    println!(
        "lines crossing a row  p50: {}  p90: {}  p99: {}  max: {}",
        occ(0.5),
        occ(0.9),
        occ(0.99),
        occupied[rows - 1]
    );
    let mut sorted = widths.clone();
    sorted.sort_unstable();
    let at = |p: f64| sorted[((rows - 1) as f64 * p) as usize];
    println!(
        "row width  p50: {}  p90: {}  p99: {}  max: {}",
        at(0.5),
        at(0.9),
        at(0.99),
        sorted[rows - 1]
    );
    for limit in [8, 16, 32, 64] {
        let over = widths.iter().filter(|&&w| w > limit).count();
        println!(
            "  wider than {limit}: {:.2}% of rows",
            over as f64 * 100.0 / rows as f64
        );
    }
}
