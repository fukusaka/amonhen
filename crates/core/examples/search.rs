//! History の検索（件名・作者名・ID）にかかる時間。1 回目は件名・作者名の索引を作るので遅く、2 回目からは索引を
//! 探すだけになる。`cargo run --release -p amonhen-core --example search -- <repo> [<query>…]`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;

fn main() -> amonhen_model::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| ".".into());
    let mut queries: Vec<String> = args.collect();
    if queries.is_empty() {
        queries = ["fix", "xfs", "fix xfs", r#"subject:"use after free""#, "author:torvalds", "3857c2fe"]
            .map(String::from)
            .to_vec();
    }
    let repo = Repository::new(GixBackend::open(&path)?);
    let history = repo.load_history()?;
    println!("commits: {}", history.graph.len());
    for (i, query) in queries.iter().enumerate() {
        let t = Instant::now();
        let found = repo.search_history(&history, query)?;
        let first = t.elapsed().as_secs_f64() * 1000.0;
        let mut again = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            repo.search_history(&history, query)?;
            again.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        again.sort_by(f64::total_cmp);
        let t = Instant::now();
        let matching = repo.history_matching(&history, &found);
        let filter = t.elapsed().as_secs_f64() * 1000.0;
        let note = if i == 0 { "（索引を作る）" } else { "" };
        println!("{:<28} 一致だけの History（{} 行）を作る {filter:.1} ms", "", matching.graph.len());
        println!("{query:<28} {:>8} 件  1 回目 {first:>8.1} ms{note}  2 回目から {:>6.1} ms", found.len(), again[1]);
    }
    Ok(())
}
