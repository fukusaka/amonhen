//! 祖先の強調の計算時間を測る（History の先頭・中ほど・末尾の近くの行で）。描く行の分だけ求める
//! `LineageWindow` は、先頭の行を選んで 5 万行ずつ伸ばしたときの 1 回ごとの時間も出す。
//! `cargo run --release -p amonhen-core --example lineage -- <repo>`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;
use amonhen_core::lineage::{LineageWindow, NOT_LIT, lineage};

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
    let step = 50_000;
    let mut times = Vec::new();
    let t = Instant::now();
    let mut w = LineageWindow::new(&history.graph, 0);
    while w.end() < rows {
        let s = Instant::now();
        w.extend(&history.graph, &history.layout, w.end() + step);
        times.push(s.elapsed().as_secs_f64() * 1000.0);
    }
    let total = t.elapsed().as_secs_f64() * 1000.0;
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    println!(
        "LineageWindow を {step} 行ずつ {} 回: 合計 {total:.1} ms、1 回 中央 {:.2} ms・最大 {:.2} ms・最初 {:.2} ms",
        times.len(),
        sorted[sorted.len() / 2],
        sorted[sorted.len() - 1],
        times[0]
    );
    Ok(())
}
