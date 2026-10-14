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
    Ok(())
}
