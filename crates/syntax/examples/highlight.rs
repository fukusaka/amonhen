//! ファイルの色付けにかかる時間を測る: `cargo run --release -p amonhen-syntax --example highlight -- <file>...`

use std::time::Instant;

fn main() {
    for path in std::env::args().skip(1) {
        let source = std::fs::read(&path).expect("読めない");
        // 1 回目は文法のクエリの読み込みを含む。
        for round in ["1 回目", "2 回目"] {
            let started = Instant::now();
            let tokens = amonhen_syntax::highlight(&path, &source);
            println!(
                "{path} ({} 行, {} KB) {round}: {:.1} ms, {} トークン",
                source.iter().filter(|&&b| b == b'\n').count(),
                source.len() / 1024,
                started.elapsed().as_secs_f64() * 1000.0,
                tokens.len()
            );
        }
    }
}
