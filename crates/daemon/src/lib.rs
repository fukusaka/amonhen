//! daemon（Amon Hen のエンジンの、FE とつなぐ側）。実行ファイルは CLI と 1 つで、`amonhen --stdio`・`--serve`・
//! `--socket`（または `amonhend` という名前での起動）でこちらになる。
//!
//! `--stdio <repo>`: リポジトリの git の操作を、標準入出力の要求に応じて行う daemon。
//! SSH 先では `ssh host amonhend --stdio <repo>` で起動し、手元の Amon Hen がつなぐ。
//! `amonhend --serve [<socket>]`: 手元に常駐し、アプリ・CLI・VSCode 拡張がソケットでつなぐ共有の daemon（`serve`）。
//! `amonhend --socket`: そのソケットの場所を出す。

mod serve;

use amonhen_model::tr;
use std::sync::Arc;

use amonhen_backend_gix::GixBackend;
use amonhen_model::GitBackend;
use amonhen_remote::{Notification, Server};

/// daemon としての起動か（`--stdio`・`--serve`・`--socket` で始まる引数、または `amonhend` という名前）。
pub fn is_daemon(argv0: &str, args: &[String]) -> bool {
    let name = std::path::Path::new(argv0)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    name.starts_with("amonhend")
        || matches!(
            args.first().map(String::as_str),
            Some("--stdio" | "--serve" | "--socket")
        )
}

/// daemon として動き、終わったら終了コードを返す（`args` は実行ファイルの名前を除いた引数）。
pub fn run(args: &[String]) -> i32 {
    // 版の印を実行ファイルに残す（SSH 先へ送る側が、中身でこの版かを確かめる）。
    std::hint::black_box(amonhen_remote::PROTOCOL_MARK);
    // 既定の言語は起動した側が `AMONHEN_LANG` で渡す（つないだ相手ごとの言語は挨拶で受け取る）。
    amonhen_model::i18n::set_default(amonhen_model::i18n::Lang::from_env());
    // macOS のアプリ（AppKit）は SIGTERM などを無視・保留する設定にしていて、子のプロセスはそれを引き継ぐ。
    // 手元で起動されたときにも、ふつうに止められるよう既定に戻す（スレッドを作る前に）。
    #[cfg(unix)]
    // SAFETY: 既定の動作に戻し、保留を解くだけで、ハンドラは登録しない。
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(signal, libc::SIG_DFL);
            libc::sigaddset(&mut set, signal);
        }
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
    }
    let repo = match args {
        [flag, repo] if flag == "--stdio" => repo.clone(),
        [flag, rest @ ..] if flag == "--serve" && rest.len() <= 1 => {
            if let Err(e) = serve::run(rest.first().cloned()) {
                eprintln!("amonhen: {e}");
                return 1;
            }
            return 0;
        }
        // 共有の daemon のソケットの場所（VSCode 拡張などが、通信の形の版を知らずにつなげるように）。
        [flag] if flag == "--socket" => {
            println!("{}", amonhen_remote::endpoint::endpoint());
            return 0;
        }
        [flag] if flag == "--version" => {
            println!("{}", version());
            return 0;
        }
        _ => {
            eprintln!(
                "{}",
                tr!(
                    "Usage: amonhen --stdio <repository path> | amonhen --serve [<socket>] | amonhen --socket"
                )
            );
            return 2;
        }
    };
    match stdio(&repo) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("amonhen: {e}");
            1
        }
    }
}

/// 配る版（`0.1.1-snapshot.5+abc1234` など）。配るエンジンを作るとき（`scripts/build-engine.sh`）に
/// `AMONHEN_VERSION` で埋め込む。なければ（開発中のビルド）Cargo.toml の版。`amonhen install` はこれで新旧を決める。
pub const VERSION: &str = match option_env!("AMONHEN_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// `amonhen 0.1.0 (protocol 16)`。FE と SSH 先は、ここの protocol で使えるかを決める。
pub fn version() -> String {
    format!(
        "amonhen {VERSION} (protocol {})",
        amonhen_remote::PROTOCOL_VERSION
    )
}

fn stdio(repo: &str) -> amonhen_model::Result<()> {
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
            eprintln!("{}", tr!("amonhen: Can’t watch the repository: {e}", e = e));
            None
        }
    };
    server.run(std::io::stdin().lock())?;
    Ok(())
}
