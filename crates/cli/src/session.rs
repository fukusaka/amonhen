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
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!("知らないオプション: {s}\n\n{USAGE}").into());
            }
            _ => commands.push(arg.clone()),
        }
    }
    let front = open_repository(&dir)?;
    // コアの命令をそのまま渡し、答えの JSON を 1 行で返す。
    let run = |line: &str| -> String {
        let command = match serde_json::from_str::<Value>(line) {
            Ok(c) => c,
            Err(e) => {
                return json!({"ok": false, "error": format!("命令を読めない: {e}")}).to_string();
            }
        };
        match front.call(json!({"op": "run", "command": command})) {
            Ok(reply) => reply.json.to_string(),
            Err(e) => json!({"ok": false, "error": e.to_string()}).to_string(),
        }
    };
    if !commands.is_empty() {
        let mut all_ok = true;
        for c in &commands {
            let reply = run(c);
            all_ok &= ok(&reply);
            println!("{reply}");
        }
        return Ok(if all_ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        writeln!(out, "{}", run(&line))?;
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
    let front = open_repository(&dir)?;
    let (tx, rx) = mpsc::channel::<u32>();
    // 手元ならこのプロセスで監視し、daemon 越しなら daemon の知らせを受ける（アプリと同じ）。
    front.subscribe(Box::new(move |bits| {
        let _ = tx.send(bits);
    }))?;
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
        let history = front.call(json!({"op": "history"}))?.json;
        let status = front.call(json!({"op": "status"}))?.json;
        let _ = front.call(json!({"op": "historyRelease", "history": history["history"]}));
        let head = history["head"].as_str().map(str::to_owned);
        let commits = history["rows"].as_u64().unwrap_or(0);
        let changed = status["entries"].as_array().map_or(0, Vec::len);
        if as_json {
            println!(
                "{}",
                json!({"changes": names, "head": head, "commits": commits, "changedFiles": changed})
            );
        } else {
            let head = head.as_deref().map_or("なし", |h| &h[..8]);
            println!(
                "{}  {}  HEAD {head}・コミット {commits}・変更のあるファイル {changed}",
                term::local_time(now(), true),
                if names.is_empty() {
                    "-".to_owned()
                } else {
                    names.join(",")
                },
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
        if let Some(reason) = front.closed() {
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
            let name = url
                .trim_end_matches('/')
                .rsplit(['/', ':'])
                .next()
                .unwrap_or("repo");
            (
                url,
                PathBuf::from(name.strip_suffix(".git").unwrap_or(name)),
            )
        }
        [url, dir] => (url, PathBuf::from(dir)),
        _ => {
            return Err(
                format!("clone には URL と（あれば）置き場所を渡してください\n\n{USAGE}").into(),
            );
        }
    };
    let dest = if dest.is_absolute() {
        dest
    } else {
        std::env::current_dir()?.join(dest)
    };
    amonhen_backend_cli::clone(url, Path::new(&dest))?;
    println!("clone しました: {}", dest.display());
    Ok(ExitCode::SUCCESS)
}
