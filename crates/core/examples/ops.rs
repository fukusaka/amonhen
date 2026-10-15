//! 実際のリポジトリで、画面の操作に対応する処理の時間を測る。
//! `cargo run --release -p amonhen-core --example ops -- <repo>`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;
use amonhen_projection::FrontendProfile;

fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let v = f();
    println!("{label:<36} {:>9.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    v
}

fn main() -> amonhen_model::Result<()> {
    let path = std::env::args().nth(1).expect("リポジトリを指定する");
    let repo = Repository::new(GixBackend::open(&path)?);
    let history = time("load_history（キャッシュなし）", || {
        repo.load_history()
    })?;
    time("load_history（変化なし）", || repo.load_history())?;
    time("load_refs", || repo.load_refs())?;
    time("load_refs（2 回目）", || repo.load_refs())?;
    time("load_status", || repo.load_status())?;
    let ids = history.graph.ids();
    for row in [0usize, 1, 2] {
        let id = ids[row];
        let parents = history.graph.parents(row).len();
        let changes = time(
            &format!("commit_changes 行 {row}（親 {parents}）"),
            || repo.commit_changes(&id),
        )?;
        time(&format!("commit_info 行 {row}"), || repo.commit_info(&id))?;
        if let Some(first) = changes.first() {
            time(
                &format!("file_diff 行 {row}（{} ファイル中の先頭）", changes.len()),
                || repo.file_diff(first, &FrontendProfile::MACOS),
            )?;
        }
    }
    time("history_chunk 256 行", || {
        repo.history_chunk(&history, 0, 256, &FrontendProfile::MACOS)
    })?;
    Ok(())
}
