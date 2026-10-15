//! コミットを選んだときの「変わったファイル」の時間を測る（履歴に散らした行で）。
//! `cargo run --release -p amonhen-core --example changes -- <repo> [count]`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;

fn main() -> amonhen_model::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| ".".into());
    let count: usize = args.next().and_then(|c| c.parse().ok()).unwrap_or(60);
    let repo = Repository::new(GixBackend::open(&path)?);
    let history = repo.load_history()?;
    let rows = history.graph.len();
    let mut times = Vec::new();
    let mut merges = Vec::new();
    for i in 0..count {
        let row = (i * 7919) % rows;
        let id = history.graph.ids()[row];
        let t = Instant::now();
        let changes = repo.commit_changes(&id)?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let parents = history.graph.parents(row).len();
        times.push(ms);
        if parents > 1 {
            merges.push((ms, changes.len()));
        }
    }
    times.sort_by(f64::total_cmp);
    let at = |p: f64| times[((times.len() - 1) as f64 * p) as usize];
    println!(
        "commit_changes x{count}: p50 {:.1} ms  p95 {:.1} ms  max {:.1} ms",
        at(0.5),
        at(0.95),
        at(1.0)
    );
    let merge_ms: Vec<String> = merges
        .iter()
        .map(|(ms, n)| format!("{ms:.0}ms/{n}"))
        .collect();
    println!("merges: {} ({})", merges.len(), merge_ms.join(", "));
    Ok(())
}
