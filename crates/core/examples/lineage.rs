//! 祖先の強調の計算時間を測る（History の先頭・中ほど・末尾の近くの行で）。
//! `cargo run --release -p amonhen-core --example lineage -- <repo>`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;
use amonhen_core::lineage::{NOT_LIT, lineage};

fn main() -> amonhen_model::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let repo = Repository::new(GixBackend::open(&path)?);
    let history = repo.load_history()?;
    let rows = history.graph.len() as u32;
    println!("{rows} コミット、線 {} 本", history.layout.edges().len());
    for row in [0, 1000, rows / 2, rows - 1000] {
        let t = Instant::now();
        let l = lineage(&history.graph, &history.layout, row);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let ancestors: u32 = l.rows.iter().map(|w| w.count_ones()).sum();
        let lit = l.edge_from.iter().filter(|&&f| f != NOT_LIT).count();
        println!("行 {row}: {ms:.1} ms（祖先 {ancestors}、光る線 {lit}）");
    }
    Ok(())
}
