//! 実際のリポジトリで、マージ済みブランチの折りたたみと、スクロールバーの密度にかかる時間を測る。
//! `cargo run --release -p amonhen-core --example fold -- <repo>`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;

fn main() -> amonhen_model::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let repo = Repository::new(GixBackend::open(&path)?);
    let full = repo.load_history()?;
    println!("commits: {}", full.graph.len());

    let t = Instant::now();
    let folded = repo.history_folded(None, &[])?;
    println!(
        "fold (all branches): {:?}  rows: {}  hiding merges: {}",
        t.elapsed(),
        folded.graph.len(),
        folded.folded.iter().filter(|&&h| h > 0).count()
    );

    for buckets in [400, 1000] {
        let t = Instant::now();
        let d = full.density(buckets);
        println!(
            "density ({buckets}): {:?}  buckets: {}",
            t.elapsed(),
            d.len()
        );
    }
    Ok(())
}
