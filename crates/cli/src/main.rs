//! `amonhen`: Amon Hen のコアを端末から使う。UI は起動しない（`open` だけはアプリに渡す）。
//!
//!   amonhen [<path>]                    リポジトリを Amon Hen で開く（`amonhen open` と同じ）
//!   amonhen open [<path>|ssh://…]       同上。SSH 先の場所も渡せる
//!   amonhen log [options] [<query>…]    グラフ付きの履歴
//!   amonhen undo [--list|--peek]        Amon Hen の操作ログで、直前の操作を戻す
//!   amonhen merge-file [--all] <file>…  コンフリクトのあるファイルに Magic Merge をかける

mod graph;
mod open;
mod term;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;
use amonhen_model::Oid;

const USAGE: &str = "\
usage: amonhen [<path>]
       amonhen open [<path> | ssh://host/path] [--snapshot]
       amonhen log [-C <path>] [-n <count> | --all] [--fold] [--branch <name>]… [--no-graph]
                   [--color=auto|always|never] [<query>…]
       amonhen undo [-C <path>] [--list | --peek]
       amonhen merge-file [--all] [--check] <file>…

log の <query> は History の検索と同じ書き方（author:名前 path:パス contents:\"文字列\" など）。
merge-file は既定で間違えようのない規則だけをかける（--all ですべての規則）。コンフリクトが残れば終了コード 1
なので、git の mergetool にも使える:
  git config mergetool.amonhen.cmd 'amonhen merge-file \"$MERGED\"'
  git config mergetool.amonhen.trustExitCode true";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => open::run(&[]),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some("open") => open::run(&args[1..]),
        Some("log") => log(&args[1..]),
        Some("undo") => undo(&args[1..]),
        Some("merge-file") => merge_file(&args[1..]),
        // サブコマンドでなければ、開く場所とみなす（`amonhen .`）。
        Some(_) => open::run(&args),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("amonhen: {e}");
            ExitCode::from(2)
        }
    }
}

type Result<T> = amonhen_model::Result<T>;

/// 引数を 1 つずつ読む（`-n 5`・`-n5`・`--color=never` の形）。
struct Args<'a> {
    rest: std::slice::Iter<'a, String>,
}

impl<'a> Args<'a> {
    fn new(args: &'a [String]) -> Self {
        Self { rest: args.iter() }
    }

    fn value(&mut self, name: &str) -> Result<&'a str> {
        self.rest.next().map(String::as_str).ok_or_else(|| format!("{name} に値がありません").into())
    }
}

/// `path` を含むリポジトリの作業ツリーの一番上（`git rev-parse --show-toplevel`）。
fn repository_root(path: &Path) -> Result<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !out.status.success() {
        return Err(format!("{} は Git のリポジトリではありません", path.display()).into());
    }
    Ok(PathBuf::from(String::from_utf8(out.stdout)?.trim()))
}

/// リポジトリを開く。History はアプリと同じ永続キャッシュを使う（2 回目からはすぐ出る）。
fn open_repository(path: &Path) -> Result<Repository> {
    let root = repository_root(path)?;
    let backend = GixBackend::open(&root)?;
    let key = std::fs::canonicalize(&root).unwrap_or(root).display().to_string();
    Ok(match cache_dir() {
        Some(dir) => Repository::with_cache(backend, amonhen_core::cache::path_for(&dir, &key)),
        None => Repository::new(backend),
    })
}

/// アプリと同じキャッシュの置き場所（macOS）。ほかの OS では XDG の場所。
fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Caches/io.github.fukusaka.amonhen")
    } else {
        std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from).join("amonhen")
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// コアの命令を JSON で実行し、結果を返す。失敗なら Err。
fn command(repo: &Repository, json: serde_json::Value) -> Result<serde_json::Value> {
    let response: serde_json::Value = serde_json::from_str(&repo.run_json(&json.to_string()))?;
    if response["ok"] != true {
        return Err(response["error"].as_str().unwrap_or("失敗しました").to_owned().into());
    }
    Ok(response)
}

// MARK: - log

fn log(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut count: Option<usize> = None;
    let mut fold = false;
    let mut branches: Vec<String> = Vec::new();
    let mut show_graph = true;
    let mut color = term::stdout_is_tty();
    let mut query: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "-n" => count = Some(a.value("-n")?.parse()?),
            "--all" => count = Some(usize::MAX),
            "--fold" => fold = true,
            "--branch" | "-b" => branches.push(a.value("--branch")?.to_owned()),
            "--no-graph" => show_graph = false,
            "--color" | "--color=always" => color = true,
            "--color=never" | "--no-color" => color = false,
            "--color=auto" => color = term::stdout_is_tty(),
            s if s.starts_with("-n") && s[2..].parse::<usize>().is_ok() => count = s[2..].parse().ok(),
            s if s.starts_with('-') => return Err(format!("知らないオプション: {s}\n\n{USAGE}").into()),
            _ => query.push(arg.clone()),
        }
    }
    let repo = open_repository(&dir)?;
    // アプリと同じく、前回の History（永続キャッシュ）から始めて、増えたコミットだけを読む。
    repo.load_cached_history();
    let full = repo.load_history()?;
    let refs = repo.load_refs()?;
    // 絞り込むブランチ（名前から ID へ）。
    let tips: Vec<Oid> = branches
        .iter()
        .map(|name| {
            refs.refs
                .entries
                .iter()
                .find(|e| &e.name == name)
                .and_then(|e| e.target)
                .ok_or_else(|| format!("ブランチが見つかりません: {name}"))
        })
        .collect::<std::result::Result<_, _>>()?;
    let tips = (!tips.is_empty()).then_some(tips.as_slice());
    let history = match (fold, tips) {
        (true, tips) => repo.history_folded(tips, &[])?,
        (false, Some(tips)) => repo.history_filtered(tips)?,
        (false, None) => full,
    };
    // 端末に出すときは、既定で 1 画面分。
    let count = count.unwrap_or_else(|| if term::stdout_is_tty() { term::height().saturating_sub(4).max(10) / 2 } else { 100 });
    let rows: Vec<u32> = if query.is_empty() {
        (0..history.graph.len().min(count) as u32).collect()
    } else {
        // 検索の結果は、グラフなしで並べる（行がとびとびなので線をつなげない）。
        show_graph = false;
        let found = repo.search_history(&history, &query.join(" "))?;
        found.into_iter().take(count).collect()
    };
    let style = graph::Style { color, width: term::width() };
    let out = graph::render(&repo, &history, &refs.refs, &rows, show_graph, &style)?;
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}

// MARK: - undo

fn undo(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut mode = "undo";
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--list" | "-l" => mode = "list",
            "--peek" => mode = "peek",
            s => return Err(format!("知らない引数: {s}\n\n{USAGE}").into()),
        }
    }
    let repo = open_repository(&dir)?;
    match mode {
        "list" => {
            let r = command(&repo, serde_json::json!({"op": "undoLog"}))?;
            let entries = r["info"]["entries"].as_array().cloned().unwrap_or_default();
            if entries.is_empty() {
                println!("戻せる操作はありません");
            }
            for (i, e) in entries.iter().enumerate() {
                let time = e["time"].as_f64().map_or_else(String::new, |t| term::local_time((t / 1000.0) as i64, true));
                println!("{:>3}  {time}  {}", i + 1, e["label"].as_str().unwrap_or(""));
            }
        }
        "peek" => {
            let r = command(&repo, serde_json::json!({"op": "undoPeek"}))?;
            match r["label"].as_str() {
                Some(label) => println!("次に戻す操作: {label}"),
                None => println!("戻せる操作はありません"),
            }
        }
        _ => {
            let r = command(&repo, serde_json::json!({"op": "undo"}))?;
            match r["label"].as_str() {
                Some(label) => println!("戻しました: {label}"),
                None => {
                    println!("戻せる操作はありません");
                    return Ok(ExitCode::from(1));
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

// MARK: - merge-file

fn merge_file(args: &[String]) -> Result<ExitCode> {
    let mut rules = amonhen_core::conflict::MagicRules::Safe;
    let mut check = false;
    let mut files = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--all" => rules = amonhen_core::conflict::MagicRules::All,
            "--check" | "--dry-run" => check = true,
            s if s.starts_with('-') => return Err(format!("知らないオプション: {s}\n\n{USAGE}").into()),
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if files.is_empty() {
        return Err(format!("ファイルを渡してください\n\n{USAGE}").into());
    }
    let mut remaining_total = 0;
    for file in &files {
        let data = std::fs::read(file)?;
        let before = amonhen_core::conflict::parse(&data).len();
        let (merged, resolved) = amonhen_core::conflict::magic(&data, rules, None);
        let remaining = amonhen_core::conflict::parse(&merged).len();
        remaining_total += remaining;
        if resolved > 0 && !check {
            std::fs::write(file, &merged)?;
        }
        let verb = if check { "解ける" } else { "解いた" };
        println!("{}: コンフリクト {before} 個のうち {resolved} 個を{verb}、残り {remaining} 個", file.display());
    }
    Ok(if remaining_total == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) })
}
