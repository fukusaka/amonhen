//! `amonhen`: Amon Hen のコアを端末から使う。UI は起動しない（`open` だけはアプリに渡す）。コアでできることは、
//! ここからもすべて使える（VSCode 拡張やほかの FE が通る道を、先に通して確かめる）。
//!
//!   amonhen [<path>]                    リポジトリを Amon Hen で開く（`amonhen open` と同じ）
//!   amonhen log [options] [<query>…]    グラフ付きの履歴
//!   amonhen status / diff / show        Working Copy とコミットの中身
//!   amonhen stage / unstage / discard / commit   Working Copy の操作（行ごとにも）
//!   amonhen refs                        ブランチ・タグ・リモート・stash
//!   amonhen run [<json>…]               コアの命令をそのまま（なければ標準入力の 1 行 1 命令）
//!   amonhen watch                       変化の知らせを受け続ける
//!   amonhen undo [--list|--peek]        Amon Hen の操作ログで、直前の操作を戻す
//!   amonhen merge-file [--all] <file>…  コンフリクトのあるファイルに Magic Merge をかける
//!   amonhen clone <url> [<dir>]
//!   amonhen daemon [status|start|stop|rebuild]  手元の共有の daemon

mod graph;
mod open;
mod term;
mod work;
mod session;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;
use amonhen_model::Oid;

pub(crate) const USAGE: &str = "\
usage: amonhen [<path>]
       amonhen open [<path> | ssh://host/path] [--daemon] [--snapshot]
       amonhen log [-n <count> | --all] [--fold] [--branch <name>]… [--no-graph] [--rebuild] [--json] [<query>…]
       amonhen status [--json]
       amonhen diff [--staged | --combined] [--rows] [--stat] [--json] [<rev> [<rev>]] [[--] <path>…]
       amonhen show [<rev>] [--stat] [--no-patch] [--rows] [--json]
       amonhen stage [--rows <rows>] <path>… | --all
       amonhen unstage [--rows <rows>] <path>… | --all
       amonhen discard [--rows <rows>] <path>…
       amonhen commit -m <message> [--amend]
       amonhen refs [--json]
       amonhen run [<json>…]
       amonhen watch [--json]
       amonhen undo [--list | --peek]
       amonhen merge-file [--all] [--check] <file>…
       amonhen clone <url> [<dir>]
       amonhen daemon [status [--json] | start | stop | rebuild [<path> | ssh://host/path]]

リポジトリを扱う命令は -C <path>・-C ssh://host/path（共有の daemon 越し）・-C daemon:<path> で場所を選ぶ。
人が読む形のほかに --json で FE が読む形を出す（--color=auto|always|never も）。
log の <query> は History の検索と同じ書き方（author:名前 path:パス contents:\"文字列\" など）。
diff は既定で stage していない変更、--staged で stage 済み、--combined で HEAD からの変更に stage 済みの行の印
（●）。<rev> を 1 つ渡すとそのコミットの差分、2 つ渡すと History で 2 つ選んだときと同じ差分（first-parent で
つながっていれば、古いほうの親から新しいほうまで）。
--rows は差分の行番号を出す。stage・unstage の --rows <rows>（3,5-9 の形）は diff --combined の行番号、discard
の --rows は diff（stage していない差分）の行番号で、その行の変更だけを扱う。discard と commit は undo で戻せる。
run の命令は {\"op\":\"checkout\",\"branch\":\"main\"} の形（コアの command.rs）。答えも 1 行の JSON。
log --rebuild と daemon rebuild は、History のキャッシュを捨てて最初から作り直す（共有の daemon の分も）。
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
        Some("daemon") => daemon(&args[1..]),
        Some("run") => session::run(&args[1..]),
        Some("watch") => session::watch(&args[1..]),
        Some("clone") => session::clone(&args[1..]),
        Some("status") => work::status(&args[1..]),
        Some("diff") => work::diff(&args[1..]),
        Some("show") => work::show(&args[1..]),
        Some("stage" | "add") => work::stage(&args[1..], true),
        Some("unstage") => work::stage(&args[1..], false),
        Some("discard") => work::discard(&args[1..]),
        Some("commit") => work::commit(&args[1..]),
        Some("refs" | "branch") => work::refs(&args[1..]),
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

pub(crate) type Result<T> = amonhen_model::Result<T>;

/// 引数を 1 つずつ読む（`-n 5`・`-n5`・`--color=never` の形）。
pub(crate) struct Args<'a> {
    pub(crate) rest: std::slice::Iter<'a, String>,
}

impl<'a> Args<'a> {
    pub(crate) fn new(args: &'a [String]) -> Self {
        Self { rest: args.iter() }
    }

    pub(crate) fn value(&mut self, name: &str) -> Result<&'a str> {
        self.rest.next().map(String::as_str).ok_or_else(|| format!("{name} に値がありません").into())
    }
}

/// `path` を含むリポジトリの作業ツリーの一番上（`git rev-parse --show-toplevel`）。
pub(crate) fn repository_root(path: &Path) -> Result<PathBuf> {
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

/// リポジトリを開く。History はアプリと同じ永続キャッシュを使う（2 回目からはすぐ出る）。`daemon:/path` なら、
/// 手元の共有の daemon（アプリと同じもの。いなければ起動する）越しに開く。`ssh://host/path` はいつも共有の daemon
/// 越しに開く（daemon が SSH のつながりと History・検索の索引を持ち続けるので、2 回目からは速い）。
pub(crate) fn open_repository(path: &Path) -> Result<Repository> {
    let given = path.to_str().unwrap_or_default();
    let through_daemon = if given.starts_with("ssh://") { Some(format!("daemon:{given}")) } else { None };
    if let Some(location) = through_daemon.as_deref().or(Some(given).filter(|p| p.starts_with("daemon:"))) {
        let repo = amonhen_remote::location::local_path(location)?;
        let conn = amonhen_remote::local::connect(repo, &daemon_program()?)?;
        // History は daemon が用意したファイルを直接読むので、手元のキャッシュは持たない。
        return Ok(Repository::new(amonhen_remote::RemoteBackend::connect(conn)?));
    }
    let root = repository_root(path)?;
    let backend = GixBackend::open(&root)?;
    let key = std::fs::canonicalize(&root).unwrap_or(root).display().to_string();
    Ok(match cache_dir() {
        Some(dir) => Repository::with_cache(backend, amonhen_core::cache::path_for(&dir, &key)),
        None => Repository::new(backend),
    })
}

// MARK: - daemon

fn daemon(args: &[String]) -> Result<ExitCode> {
    match args.first().map(String::as_str).unwrap_or("status") {
        "status" => {
            let Some(json) = amonhen_remote::local::control("status")? else {
                println!("共有の daemon は動いていません（{}）", amonhen_remote::local::socket_path().display());
                return Ok(ExitCode::from(1));
            };
            if args.iter().any(|a| a == "--json") {
                println!("{json}");
                return Ok(ExitCode::SUCCESS);
            }
            let s: serde_json::Value = serde_json::from_str(&json)?;
            let secs = |ms: &serde_json::Value| ms.as_u64().unwrap_or(0) / 1000;
            println!("共有の daemon: 動いています");
            println!("  pid          {}", s["pid"]);
            println!("  版           {}（通信の形 {}）", s["version"].as_str().unwrap_or(""), s["protocol"]);
            println!("  ソケット     {}", s["socket"].as_str().unwrap_or(""));
            println!("  動いている   {}", duration(secs(&s["uptimeMs"])));
            println!("  つながり     {}", s["connections"]);
            let repos = s["repos"].as_array().cloned().unwrap_or_default();
            if repos.is_empty() {
                println!("  リポジトリ   なし");
            }
            for r in repos {
                let index = match (r["indexRows"].as_u64(), r["indexBytes"].as_u64()) {
                    (Some(rows), Some(bytes)) => format!("検索の索引 {rows} 行・{:.1} MB", bytes as f64 / 1e6),
                    _ => "検索の索引なし".to_owned(),
                };
                let clients = r["clients"].as_u64().unwrap_or(0);
                let state = if clients > 0 {
                    format!("{clients} つながり")
                } else {
                    format!("だれもつないでいない（{}前から）", duration(secs(&r["idleMs"])))
                };
                println!("  {}", r["path"].as_str().unwrap_or(""));
                if r["ssh"] == true {
                    let mb = |v: &serde_json::Value| v.as_u64().unwrap_or(0) as f64 / 1e6;
                    match r["closed"].as_str() {
                        Some(reason) => println!("      SSH のつながり: 切れた（{reason}）"),
                        None => println!(
                            "      SSH のつながり: 送った {:.1} MB・受け取った {:.1} MB",
                            mb(&r["sent"]),
                            mb(&r["received"])
                        ),
                    }
                }
                println!(
                    "      {state}・History {}・監視 {}・{index}",
                    if r["history"] == true { "あり" } else { "なし" },
                    if r["watching"] == true { "あり" } else { "なし" },
                );
            }
            println!("  だれもつながらなくなって {}たつと終わります", duration(secs(&s["idleExitMs"])));
            Ok(ExitCode::SUCCESS)
        }
        "start" => {
            amonhen_remote::local::start(&daemon_program()?)?;
            println!("共有の daemon を起動しました（{}）", amonhen_remote::local::socket_path().display());
            Ok(ExitCode::SUCCESS)
        }
        "stop" => match amonhen_remote::local::control("stop")? {
            Some(_) => {
                println!("共有の daemon を止めました");
                Ok(ExitCode::SUCCESS)
            }
            None => {
                println!("共有の daemon は動いていません");
                Ok(ExitCode::from(1))
            }
        },
        "rebuild" => {
            // daemon が持つ History を作り直す（daemon で開いていなければ開いてから）。
            let given = args.get(1).map_or(".", String::as_str);
            let location = if given.starts_with("ssh://") || given.starts_with("daemon:") {
                given.to_owned()
            } else {
                let root = repository_root(Path::new(given))?;
                format!("daemon:{}", std::fs::canonicalize(&root).unwrap_or(root).display())
            };
            let repo = open_repository(Path::new(&location))?;
            let started = std::time::Instant::now();
            let history = repo.rebuild_history()?;
            println!(
                "History を作り直しました（{} コミット・{:.2} 秒）",
                history.graph.len(),
                started.elapsed().as_secs_f64()
            );
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("知らない指示: {other}\n\n{USAGE}").into()),
    }
}

/// 秒を「1 時間 2 分」のように。
fn duration(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s} 秒"),
        s if s < 3600 && s % 60 == 0 => format!("{} 分", s / 60),
        s if s < 3600 => format!("{} 分 {} 秒", s / 60, s % 60),
        s => format!("{} 時間 {} 分", s / 3600, s % 3600 / 60),
    }
}

/// `amonhend` の場所: `AMONHEN_DAEMON`、なければ自分の隣（開発中の target/）か、`.app` の中
/// （`Contents/Helpers/amonhen` から見た `Contents/MacOS/amonhend`）。
fn daemon_program() -> Result<String> {
    if let Ok(p) = std::env::var("AMONHEN_DAEMON") {
        return Ok(p);
    }
    let exe = std::env::current_exe()?.canonicalize()?;
    let dir = exe.parent().ok_or("自分の場所が分からない")?;
    [dir.join("amonhend"), dir.join("../MacOS/amonhend")]
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
        .ok_or_else(|| "amonhend が見つかりません（AMONHEN_DAEMON で場所を指定できます）".into())
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
pub(crate) fn command(repo: &Repository, json: serde_json::Value) -> Result<serde_json::Value> {
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
    let mut rebuild = false;
    let mut as_json = false;
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "-n" => count = Some(a.value("-n")?.parse()?),
            "--all" => count = Some(usize::MAX),
            "--fold" => fold = true,
            "--rebuild" => rebuild = true,
            "--json" => as_json = true,
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
    let full = if rebuild {
        repo.rebuild_history()?
    } else {
        repo.load_cached_history();
        repo.load_history()?
    };
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
    if as_json {
        println!("{}", graph::json_rows(&repo, &history, &refs.refs, &rows)?);
        return Ok(ExitCode::SUCCESS);
    }
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
