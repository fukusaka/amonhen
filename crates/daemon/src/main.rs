//! `amonhend --stdio <repo>`: リポジトリの git の操作を、標準入出力の要求に応じて行う daemon。
//! SSH 先では `ssh host amonhend --stdio <repo>` で起動し、手元の Amon Hen がつなぐ。

use std::sync::Arc;

use amonhen_backend_gix::GixBackend;
use amonhen_model::GitBackend;
use amonhen_remote::{Notification, Server};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let repo = match args.as_slice() {
        [flag, repo] if flag == "--stdio" => repo.clone(),
        [flag] if flag == "--version" => {
            println!(
                "amonhend {} (protocol {})",
                env!("CARGO_PKG_VERSION"),
                amonhen_remote::PROTOCOL_VERSION
            );
            return;
        }
        _ => {
            eprintln!("使い方: amonhend --stdio <リポジトリのパス>");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&repo) {
        eprintln!("amonhend: {e}");
        std::process::exit(1);
    }
}

fn run(repo: &str) -> amonhen_model::Result<()> {
    let backend: Arc<dyn GitBackend> = Arc::new(GixBackend::open(repo)?);
    // 標準出力は応答だけに使う（ログは標準エラーへ）。
    let server = Server::new(backend.clone(), std::io::stdout());
    let notify = server.notifier();
    // リポジトリの変化を手元に知らせる（監視できなくても、要求には答える）。
    let _watcher = match amonhen_watch::Watcher::new(backend.paths(), move |changes| {
        notify(Notification::Changed(changes.0));
    }) {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("amonhend: リポジトリを監視できない: {e}");
            None
        }
    };
    server.run(std::io::stdin().lock())?;
    Ok(())
}
