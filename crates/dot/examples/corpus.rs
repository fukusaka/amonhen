//! DOT の図の集まりを描いて確かめる: 描けないもの、注記のつくものとその理由、時間（中央値・最大）、メモリの山。
//!
//! ```text
//! cargo run --release --offline -p amonhen-dot --example corpus -- [--layout <engine>] <dir> [<svg-out-dir>]
//! ```
//!
//! `<dir>` の `*.dot`・`*.gv` を名前の順に描く。`<svg-out-dir>` があれば、描いた SVG をそこに書く。`--layout` は
//! `layout` の属性のない図の配置（` ```neato ` のフェンスと同じ）。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// 確保の量と山を数える。
struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let mut engine = amonhen_dot::engines::Engine::Dot;
    if args.peek().is_some_and(|a| a == "--layout") {
        args.next();
        let name = args.next().unwrap_or_default();
        engine = amonhen_dot::engines::Engine::from_name(&name).unwrap_or_else(|| {
            eprintln!("unknown layout {name}");
            std::process::exit(2);
        });
    }
    let Some(dir) = args.next() else {
        eprintln!("usage: corpus [--layout <engine>] <dir> [<svg-out-dir>]");
        std::process::exit(2);
    };
    let out = args.next();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "dot" || x == "gv"))
        .collect();
    files.sort();
    let mut times: Vec<(f64, String)> = Vec::new();
    let mut errors = 0;
    let mut noted = 0;
    let mut peak_max = 0usize;
    let mut peak_name = String::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let base = NOW.load(Ordering::Relaxed);
        PEAK.store(base, Ordering::Relaxed);
        let started = Instant::now();
        let result = std::panic::catch_unwind(|| amonhen_dot::render_with(&text, engine));
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let peak = PEAK.load(Ordering::Relaxed).saturating_sub(base);
        if peak > peak_max {
            peak_max = peak;
            peak_name = name.clone();
        }
        times.push((ms, name.clone()));
        match result {
            Ok(Ok(d)) => {
                if !d.notes.is_empty() {
                    noted += 1;
                    let reasons: Vec<&str> = d.notes.iter().map(|n| n.reason()).collect();
                    println!("note   {name}: {}", reasons.join("; "));
                }
                if let Some(out) = &out {
                    let target = std::path::Path::new(out).join(format!("{name}.svg"));
                    let _ = std::fs::write(target, &d.svg);
                }
            }
            Ok(Err(e)) => {
                errors += 1;
                println!("error  {name}: {e}");
            }
            Err(_) => {
                errors += 1;
                println!("panic  {name}");
            }
        }
    }
    times.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let n = times.len();
    if n == 0 {
        println!("no files");
        return;
    }
    let median = times[n / 2].0;
    let (max, max_name) = times[n - 1].clone();
    println!(
        "{n} graphs, {errors} not drawn, {noted} with notes; time median {median:.2} ms, max {max:.2} ms ({max_name}); peak memory {:.1} MB ({peak_name})",
        peak_max as f64 / 1_048_576.0
    );
    for (ms, name) in times.iter().rev().take(5) {
        println!("  slow {ms:8.2} ms  {name}");
    }
}
