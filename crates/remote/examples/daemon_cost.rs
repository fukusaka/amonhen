//! daemon 越しの読み込みの時間と転送量を測る（SSH の手前の見積もり）。
//! `cargo run --release -p amonhen-remote --example daemon_cost -- <amonhend> <repo>`

use std::time::Instant;

use amonhen_core::Repository;
use amonhen_projection::FrontendProfile;
use amonhen_remote::{Connection, RemoteBackend};

fn main() -> amonhen_model::Result<()> {
    let mut args = std::env::args().skip(1);
    let daemon = args.next().expect("amonhend のパス");
    let repo_path = args.next().expect("リポジトリのパス");
    let conn = Connection::spawn(&daemon, &["--stdio".to_owned(), repo_path])?;
    let traffic = conn.clone();
    let repo = Repository::new(RemoteBackend::connect(conn)?);
    let mut last = (0, 0);
    let mut step = |name: &str,
                    f: &mut dyn FnMut() -> amonhen_model::Result<()>|
     -> amonhen_model::Result<()> {
        let t = Instant::now();
        f()?;
        let (s, r) = traffic.traffic();
        println!(
            "{name:<34} {:>8.1} ms  送信 {:>9} B  受信 {:>11} B",
            t.elapsed().as_secs_f64() * 1000.0,
            s - last.0,
            r - last.1
        );
        last = (s, r);
        Ok(())
    };
    let mut history = None;
    step("load_history（初回）", &mut || {
        history = Some(repo.load_history()?);
        Ok(())
    })?;
    let history = history.unwrap();
    step("load_history（変化なし）", &mut || {
        repo.load_history().map(drop)
    })?;
    step("load_refs", &mut || repo.load_refs().map(drop))?;
    step("load_status", &mut || repo.load_status().map(drop))?;
    step("history_chunk 256 行", &mut || {
        repo.history_chunk(&history, 0, 256, &FrontendProfile::MACOS)
            .map(drop)
    })?;
    let id = history.graph.ids()[5];
    step("commit_changes ＋ 最初の差分", &mut || {
        let changes = repo.commit_changes(&id)?;
        if let Some(c) = changes.first() {
            repo.file_diff(c, &FrontendProfile::MACOS)?;
        }
        Ok(())
    })?;
    step("search \"fix\"（件名の走査）", &mut || {
        repo.search_history(&history, "fix").map(drop)
    })?;
    Ok(())
}
