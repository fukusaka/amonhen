//! Working Copy の status の時間を測る（1 回目は index に書き残し、続く回はキャッシュを読むだけ）。
//! `cargo run --release -p amonhen-core --example status -- <repo> [count] [--fsmonitor]`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;

fn main() -> amonhen_model::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().map_or(".", String::as_str);
    let count: usize = args.get(1).and_then(|c| c.parse().ok()).unwrap_or(5);
    amonhen_backend_cli::set_fsmonitor(args.iter().any(|a| a == "--fsmonitor"));
    let repo = Repository::new(GixBackend::open(path)?);
    for i in 0..count {
        let t = Instant::now();
        let status = repo.load_status()?;
        println!(
            "{i}: {:.0} ms（{} 件）",
            t.elapsed().as_secs_f64() * 1000.0,
            status.status.entries.len()
        );
    }
    Ok(())
}
