//! 作者名を出す所（History の行・作者の検索・コミットの詳細）を、実際のリポジトリで計る。mailmap で名前を置き換える
//! 前後を比べるため（linux は `.mailmap` が約 1000 行）。`AMONHEN_BENCH_REPO` にリポジトリの場所を入れたときだけ動く。
//!
//! `AMONHEN_BENCH_REPO=/path/to/linux cargo bench -p amonhen-core --bench authors`

use std::hint::black_box;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;
use amonhen_projection::FrontendProfile;
use criterion::{Criterion, criterion_group, criterion_main};

fn bench(c: &mut Criterion) {
    let Ok(path) = std::env::var("AMONHEN_BENCH_REPO") else {
        eprintln!("AMONHEN_BENCH_REPO がないので計らない");
        return;
    };
    // History は永続キャッシュに置き、2 回目からは git を読まない。
    let cache = std::env::temp_dir().join("amonhen-bench-authors.cache");
    let repo = Repository::with_cache(GixBackend::open(&path).unwrap(), cache);
    let history = repo.load_history().unwrap();
    let rows = history.graph.len() as u32;

    c.bench_function("history chunk (60 rows)", |b| {
        let mut start = 0;
        b.iter(|| {
            // 毎回別の行（同じ行ばかりだとオブジェクトのキャッシュに当たる）。
            start = (start + 7919) % (rows - 60);
            repo.history_chunk(black_box(&history), start, 60, &FrontendProfile::MACOS)
                .unwrap()
        })
    });

    // 1 回目に索引を作る（criterion では計らず、1 回だけの時間を出す）。
    let started = std::time::Instant::now();
    repo.search_history(&history, "author:torvalds").unwrap();
    eprintln!("first search (builds the index): {:?}", started.elapsed());
    for query in ["author:torvalds", "author:\"tejun heo\"", "fix memory leak"] {
        c.bench_function(&format!("search {query}"), |b| {
            b.iter(|| repo.search_history(black_box(&history), query).unwrap())
        });
    }

    let head = history.graph.ids()[0].to_string();
    let command = format!(r#"{{"op":"commitInfo","oid":"{head}"}}"#);
    c.bench_function("commit info", |b| {
        b.iter(|| repo.run_json(black_box(&command)))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench
}
criterion_main!(benches);
