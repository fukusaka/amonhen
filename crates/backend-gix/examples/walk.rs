//! History の読み込みにかかる時間を測る。`cargo run --release -p amonhen-backend-gix --example walk -- <repo>`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_model::GitBackend;

fn main() -> amonhen_model::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".into());

    let t = Instant::now();
    let backend = GixBackend::open(&path)?;
    let tips = backend.tips()?;
    let tips_time = t.elapsed();

    let t = Instant::now();
    let graph = backend.commit_graph(&tips)?;
    let walk_time = t.elapsed();

    println!("tips: {} ({tips_time:?})", tips.len());
    println!("commits: {} ({walk_time:?})", graph.len());
    Ok(())
}
