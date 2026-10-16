//! コアにそのまま話す命令: run（`run_json` の命令）・watch（変化の知らせを受け続ける）・clone。
//! `run` を引数なしで動かすと、標準入力の 1 行 1 命令の JSON に、1 行ずつ答える（拡張などが CLI を窓口にする形）。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use amonhen_watch::Changes;
use serde_json::{Value, json};

use crate::{Args, Result, USAGE, open_repository, term};

/// `amonhen run [-C <path>] [<json>…]`: コアの命令（`{"op":"checkout","branch":"main"}` など）を実行し、答えの
/// JSON を出す。命令を渡さなければ、標準入力から 1 行ずつ読んで 1 行ずつ答える（つないだまま何度でも）。
pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut commands = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("知らないオプション: {s}\n\n{USAGE}").into()),
            _ => commands.push(arg.clone()),
        }
    }
    let repo = open_repository(&dir)?;
    if !commands.is_empty() {
        let mut all_ok = true;
        for c in &commands {
            let reply = repo.run_json(c);
            all_ok &= ok(&reply);
            println!("{reply}");
        }
        return Ok(if all_ok { ExitCode::SUCCESS } else { ExitCode::from(1) });
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        writeln!(out, "{}", repo.run_json(&line))?;
        out.flush()?;
    }
    Ok(ExitCode::SUCCESS)
}

fn ok(reply: &str) -> bool {
    serde_json::from_str::<Value>(reply).is_ok_and(|v| v["ok"] == true)
}

/// `amonhen watch [-C <path>] [--json]`: リポジトリの変化を受けるたびに、何が変わったかと今の様子（HEAD・コミットの
/// 数・変更のあるファイルの数）を 1 行ずつ出す。Ctrl-C か、daemon とのつながりが切れるまで続ける。
pub fn watch(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut as_json = false;
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => as_json = true,
            s => return Err(format!("知らない引数: {s}\n\n{USAGE}").into()),
        }
    }
    let repo = open_repository(&dir)?;
    let (tx, rx) = mpsc::channel::<u32>();
    // daemon 越しなら daemon の知らせ、手元なら自分で監視する（アプリと同じ）。
    let remote_tx = tx.clone();
    let _watcher = if repo.subscribe_changes(Box::new(move |bits| {
        let _ = remote_tx.send(bits);
    })) {
        None
    } else {
        Some(amonhen_watch::Watcher::new(repo.paths(), move |changes| {
            let _ = tx.send(changes.0);
        })?)
    };
    let report = |bits: u32| -> Result<()> {
        let changes = Changes(bits);
        let names: Vec<&str> = [
            (Changes::REFS, "refs"),
            (Changes::INDEX, "index"),
            (Changes::WORKTREE, "worktree"),
            (Changes::CONFIG, "config"),
        ]
        .into_iter()
        .filter(|(c, _)| changes.contains(*c))
        .map(|(_, n)| n)
        .collect();
        let history = repo.load_history()?;
        let status = repo.load_status()?;
        let head = history.head.map(|h| h.to_string());
        let changed = status.status.entries.len();
        if as_json {
            println!(
                "{}",
                json!({"changes": names, "head": head, "commits": history.graph.len(), "changedFiles": changed})
            );
        } else {
            let head = head.as_deref().map_or("なし", |h| &h[..8]);
            println!(
                "{}  {}  HEAD {head}・コミット {}・変更のあるファイル {changed}",
                term::local_time(now(), true),
                if names.is_empty() { "-".to_owned() } else { names.join(",") },
                history.graph.len(),
            );
        }
        std::io::stdout().flush()?;
        Ok(())
    };
    // 最初に今の様子を出す。
    report(0)?;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(mut bits) => {
                // 続けて来た知らせはまとめる。
                while let Ok(more) = rx.recv_timeout(Duration::from_millis(50)) {
                    bits |= more;
                }
                report(bits)?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some(reason) = repo.connection_state().and_then(|s| s.closed) {
            return Err(format!("daemon とのつながりが切れました: {reason}").into());
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `amonhen clone <url> [<dir>]`
pub fn clone(args: &[String]) -> Result<ExitCode> {
    let (url, dest) = match args {
        [url] => {
            let name = url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or("repo");
            (url, PathBuf::from(name.strip_suffix(".git").unwrap_or(name)))
        }
        [url, dir] => (url, PathBuf::from(dir)),
        _ => return Err(format!("clone には URL と（あれば）置き場所を渡してください\n\n{USAGE}").into()),
    };
    let dest = if dest.is_absolute() { dest } else { std::env::current_dir()?.join(dest) };
    amonhen_backend_cli::clone(url, Path::new(&dest))?;
    println!("clone しました: {}", dest.display());
    Ok(ExitCode::SUCCESS)
}
