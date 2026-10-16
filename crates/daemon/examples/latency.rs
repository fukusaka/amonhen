//! 手元の共有の daemon（`amonhend --serve`）との往復の遅れを、同じプロセスの場合と比べる。
//! `cargo run --release -p amonhen-daemon --example latency -- <repo>`（先に `cargo build --release -p amonhen-daemon`）

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_model::GitBackend;
use amonhen_remote::RemoteBackend;

fn stats(name: &str, mut samples: Vec<f64>) {
    samples.sort_by(f64::total_cmp);
    let q = |p: f64| samples[((samples.len() as f64 - 1.0) * p) as usize];
    println!("{name:<44} p50 {:>8.1} µs  p95 {:>8.1} µs  max {:>8.1} µs", q(0.5), q(0.95), q(1.0));
}

fn bench(name: &str, backend: &dyn GitBackend, ids: &[amonhen_model::Oid]) {
    let mut small = Vec::new();
    for _ in 0..2000 {
        let t = Instant::now();
        backend.head().unwrap();
        small.push(t.elapsed().as_secs_f64() * 1e6);
    }
    stats(&format!("{name}: head（小さな要求）"), small);
    let mut chunk = Vec::new();
    for i in 0..200 {
        let start = (i * 256) % ids.len().saturating_sub(256).max(1);
        let t = Instant::now();
        backend.commit_summaries(&ids[start..start + 256.min(ids.len())]).unwrap();
        chunk.push(t.elapsed().as_secs_f64() * 1e6);
    }
    stats(&format!("{name}: 256 行の件名・作者（チャンク）"), chunk);
}

/// コミットの全ファイルの差分（History でコミットを選んだとき）と、Working Copy の status。
fn bench_core(name: &str, repo: &amonhen_core::Repository) {
    let history = repo.load_history().unwrap();
    let ids: Vec<_> = history.graph.ids().iter().take(300).copied().collect();
    let mut diffs = Vec::new();
    for id in ids.iter().step_by(3) {
        let t = Instant::now();
        let changes = repo.commit_changes(id).unwrap();
        repo.stacked_diff(&changes).unwrap();
        diffs.push(t.elapsed().as_secs_f64() * 1e6);
    }
    stats(&format!("{name}: コミットの全ファイルの差分"), diffs);
    let mut status = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        repo.load_status().unwrap();
        status.push(t.elapsed().as_secs_f64() * 1e6);
    }
    stats(&format!("{name}: Working Copy の status"), status);
}

fn main() -> amonhen_model::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let local = GixBackend::open(&path)?;
    let ids = local.commit_graph(&local.tips()?)?.ids().iter().take(100_000).copied().collect::<Vec<_>>();
    bench("同じプロセス", &local, &ids);
    let daemon = std::env::current_exe()?.parent().unwrap().join("../amonhend").display().to_string();
    let conn = amonhen_remote::local::connect(&std::fs::canonicalize(&path)?.display().to_string(), &daemon)?;
    let remote = RemoteBackend::connect(conn)?;
    bench("daemon 越し", &remote, &ids);
    bench_core("同じプロセス", &amonhen_core::Repository::new(GixBackend::open(&path)?));
    let conn = amonhen_remote::local::connect(&std::fs::canonicalize(&path)?.display().to_string(), &daemon)?;
    bench_core("daemon 越し", &amonhen_core::Repository::new(RemoteBackend::connect(conn)?));
    Ok(())
}
