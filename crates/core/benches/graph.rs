use std::hint::black_box;

use amonhen_core::cache::{self, CachedHistory};
use amonhen_core::graph::{Layouter, layout, relayout, synthetic};
use criterion::{Criterion, criterion_group, criterion_main};

fn bench(c: &mut Criterion) {
    for commits in [100_000, 1_000_000] {
        let (g, head) = synthetic::graph(commits, 42);
        c.bench_function(&format!("layout {commits}"), |b| {
            b.iter(|| layout(black_box(&g), head))
        });
    }

    let (g, head) = synthetic::graph(1_000_000, 42);
    c.bench_function("first screen (60 rows) of 1M", |b| {
        b.iter(|| {
            let mut l = Layouter::new(black_box(&g), head);
            l.advance(60);
            l.row()
        })
    });

    let (old_g, old_head) = synthetic::graph(1_000_000, 42);
    let old = layout(&old_g, old_head);
    let (g, head) = synthetic::extended(1_000_000, 10, 42);
    c.bench_function("relayout 1M after 10 new commits", |b| {
        b.iter(|| relayout(black_box(&old), &old_g, &g, head))
    });

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.bin");
    let history = CachedHistory { tips: vec![old_g.ids()[0]], head: None, graph: old_g.clone(), layout: old.clone() };
    c.bench_function("cache write 1M", |b| b.iter(|| cache::write(&path, &history).unwrap()));
    c.bench_function("cache read 1M", |b| b.iter(|| cache::read(black_box(&path)).unwrap()));

    let l = layout(&old_g, old_head);
    c.bench_function("edges_in 60 rows", |b| {
        b.iter(|| l.edges_in(black_box(500_000..500_060)).count())
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
