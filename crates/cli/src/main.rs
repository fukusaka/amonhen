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
//!   amonhen install / engine           エンジンの置き場所（`install.rs`）
//!   amonhen daemon [status|start|stop|rebuild]  手元の共有の daemon

mod graph;
mod install;
mod open;
mod session;
mod term;
mod work;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use amonhen_front::Front;

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
       amonhen install [--quiet] [--no-link] [--force]   （エンジンを ~/.local/share/amonhen に置き、~/.local/bin/amonhen を張る）
       amonhen engine   （共有の daemon として起動するエンジンの場所）
       amonhen find-repositories <dir> [--depth <n>] [--exclude <name>]…   （フォルダの下のリポジトリ。SSH 先のスキャンに使う）
       amonhen --version
       amonhen --serve [<socket>] | --stdio <repo> | --socket   （daemon として動く。FE と SSH 先が使う）

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
    let argv0 = std::env::args().next().unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // エンジンは 1 つの実行ファイルで、daemon としての起動（`--serve`・`--stdio`・`--socket`、または `amonhend` と
    // いう名前）なら daemon になる。
    if amonhen_daemon::is_daemon(&argv0, &args) {
        return ExitCode::from(amonhen_daemon::run(&args) as u8);
    }
    let result = match args.first().map(String::as_str) {
        None => open::run(&[]),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some("-V" | "--version" | "version") => {
            println!("{}", amonhen_daemon::version());
            Ok(ExitCode::SUCCESS)
        }
        Some("open") => open::run(&args[1..]),
        Some("log") => log(&args[1..]),
        Some("undo") => undo(&args[1..]),
        Some("merge-file") => merge_file(&args[1..]),
        Some("find-repositories") => find_repositories(&args[1..]),
        Some("daemon") => daemon(&args[1..]),
        Some("install") => install::install(&args[1..]),
        Some("engine") => install::engine(&args[1..]),
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
        self.rest
            .next()
            .map(String::as_str)
            .ok_or_else(|| format!("{name} に値がありません").into())
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

/// リポジトリを開く（コアへの窓口）。手元のパスなら、このプロセスのコアで（History はアプリと同じ永続キャッシュを
/// 使うので、2 回目からはすぐ出る）。`daemon:<path>` と `ssh://host/path` は、手元の共有の daemon（アプリと同じもの。
/// いなければ起動する）のコアで動かし、ここでは答えを出すだけにする（daemon が SSH のつながりと History・検索の
/// 索引を持ち続けるので、2 回目からは速い）。どちらも同じ命令を通る（VSCode 拡張と同じ道）。
pub(crate) fn open_repository(path: &Path) -> Result<Box<dyn Front>> {
    let given = path.to_str().unwrap_or_default();
    if given.starts_with("ssh://") || given.starts_with("daemon:") {
        return amonhen_front::open(given, &daemon_program()?, None);
    }
    let root = repository_root(path)?;
    amonhen_front::open(&root.display().to_string(), "", cache_dir().as_deref())
}

/// 窓口に命令を送る。
pub(crate) fn call(front: &dyn Front, request: serde_json::Value) -> Result<amonhen_front::Reply> {
    front.call(request)
}

// MARK: - daemon

fn daemon(args: &[String]) -> Result<ExitCode> {
    match args.first().map(String::as_str).unwrap_or("status") {
        "status" => {
            let Some(json) = amonhen_remote::local::control("status")? else {
                println!(
                    "共有の daemon は動いていません（{}）",
                    amonhen_remote::endpoint::endpoint()
                );
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
            println!(
                "  版           {}（通信の形 {}）",
                s["version"].as_str().unwrap_or(""),
                s["protocol"]
            );
            println!("  ソケット     {}", s["socket"].as_str().unwrap_or(""));
            println!("  動いている   {}", duration(secs(&s["uptimeMs"])));
            println!("  つながり     {}", s["connections"]);
            let repos = s["repos"].as_array().cloned().unwrap_or_default();
            if repos.is_empty() {
                println!("  リポジトリ   なし");
            }
            for r in repos {
                let index = match (r["indexRows"].as_u64(), r["indexBytes"].as_u64()) {
                    (Some(rows), Some(bytes)) => {
                        format!("検索の索引 {rows} 行・{:.1} MB", bytes as f64 / 1e6)
                    }
                    _ => "検索の索引なし".to_owned(),
                };
                let clients = r["clients"].as_u64().unwrap_or(0);
                let state = if clients > 0 {
                    format!("{clients} つながり")
                } else {
                    format!(
                        "だれもつないでいない（{}前から）",
                        duration(secs(&r["idleMs"]))
                    )
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
                    if let Some(engine) = r["engine"].as_str() {
                        println!("      SSH 先のエンジン: {engine}");
                    }
                }
                println!(
                    "      {state}・History {}・監視 {}・{index}",
                    if r["history"] == true {
                        "あり"
                    } else {
                        "なし"
                    },
                    if r["watching"] == true {
                        "あり"
                    } else {
                        "なし"
                    },
                );
            }
            println!(
                "  だれもつながらなくなって {}たつと終わります",
                duration(secs(&s["idleExitMs"]))
            );
            Ok(ExitCode::SUCCESS)
        }
        "start" => {
            amonhen_remote::local::start(&daemon_program()?)?;
            println!(
                "共有の daemon を起動しました（{}）",
                amonhen_remote::endpoint::endpoint()
            );
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
                format!(
                    "daemon:{}",
                    std::fs::canonicalize(&root).unwrap_or(root).display()
                )
            };
            let front = open_repository(Path::new(&location))?;
            let started = std::time::Instant::now();
            let history = call(
                &*front,
                serde_json::json!({"op": "history", "rebuild": true}),
            )?
            .json;
            println!(
                "History を作り直しました（{} コミット・{:.2} 秒）",
                history["rows"],
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

/// 共有の daemon として起動するエンジン: `AMONHEN_DAEMON`、なければエンジンの置き場所の `current`（通信の形の版が
/// 同じなら）か自分（`install::engine_for_daemon`）。
fn daemon_program() -> Result<String> {
    if let Ok(p) = std::env::var("AMONHEN_DAEMON") {
        return Ok(p);
    }
    Ok(install::engine_for_daemon()?.display().to_string())
}

/// アプリと同じキャッシュの置き場所（macOS）。ほかの OS では XDG の場所。
fn cache_dir() -> Option<PathBuf> {
    amonhen_remote::local::cache_dir()
}

/// コアの命令（`run_json`）を実行し、結果を返す。失敗なら Err。
pub(crate) fn command(front: &dyn Front, json: serde_json::Value) -> Result<serde_json::Value> {
    let response = front
        .call(serde_json::json!({"op": "run", "command": json}))?
        .json;
    if response["ok"] != true {
        return Err(response["error"]
            .as_str()
            .unwrap_or("失敗しました")
            .to_owned()
            .into());
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
            s if s.starts_with("-n") && s[2..].parse::<usize>().is_ok() => {
                count = s[2..].parse().ok()
            }
            s if s.starts_with('-') => {
                return Err(format!("知らないオプション: {s}\n\n{USAGE}").into());
            }
            _ => query.push(arg.clone()),
        }
    }
    let front = open_repository(&dir)?;
    let refs = call(&*front, serde_json::json!({"op": "refs"}))?.json;
    // 絞り込むブランチ（名前から ID へ）。
    let tips: Vec<String> = branches
        .iter()
        .map(|name| {
            refs["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|e| e["name"] == name.as_str() && e["kind"] != "tag")
                .and_then(|e| e["target"].as_str().map(str::to_owned))
                .ok_or_else(|| format!("ブランチが見つかりません: {name}"))
        })
        .collect::<std::result::Result<_, _>>()?;
    let mut request = serde_json::json!({"op": "history", "rebuild": rebuild, "fold": fold});
    if !tips.is_empty() {
        request["tips"] = serde_json::json!(tips);
    }
    let history = call(&*front, request)?.json;
    let (id, total) = (
        history["history"].clone(),
        history["rows"].as_u64().unwrap_or(0),
    );
    // 端末に出すときは、既定で 1 画面分。
    let count = count.unwrap_or_else(|| {
        if term::stdout_is_tty() {
            term::height().saturating_sub(4).max(10) / 2
        } else {
            100
        }
    });
    let rows = if query.is_empty() {
        call(
            &*front,
            serde_json::json!({"op": "historyRows", "history": id, "start": 0, "count": count.min(total as usize)}),
        )?
    } else {
        // 検索の結果は、グラフなしで並べる（行がとびとびなので線をつなげない）。
        show_graph = false;
        let found = call(
            &*front,
            serde_json::json!({"op": "historySearch", "history": id, "query": query.join(" ")}),
        )?;
        let found: Vec<u32> = found.u32s("rows").into_iter().take(count).collect();
        call(
            &*front,
            serde_json::json!({"op": "historyRows", "history": id, "rows": found}),
        )?
    };
    let rows = graph::Rows::from(&rows);
    if as_json {
        println!("{}", graph::json_rows(&rows, &refs));
        return Ok(ExitCode::SUCCESS);
    }
    let style = graph::Style {
        color,
        width: term::width(),
    };
    term::write(&graph::render(&rows, &refs, show_graph, &style));
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
    let front = open_repository(&dir)?;
    match mode {
        "list" => {
            let r = command(&*front, serde_json::json!({"op": "undoLog"}))?;
            let entries = r["info"]["entries"].as_array().cloned().unwrap_or_default();
            if entries.is_empty() {
                println!("戻せる操作はありません");
            }
            for (i, e) in entries.iter().enumerate() {
                let time = e["time"]
                    .as_f64()
                    .map_or_else(String::new, |t| term::local_time((t / 1000.0) as i64, true));
                println!(
                    "{:>3}  {time}  {}",
                    i + 1,
                    e["label"].as_str().unwrap_or("")
                );
            }
        }
        "peek" => {
            let r = command(&*front, serde_json::json!({"op": "undoPeek"}))?;
            match r["label"].as_str() {
                Some(label) => println!("次に戻す操作: {label}"),
                None => println!("戻せる操作はありません"),
            }
        }
        _ => {
            let r = command(&*front, serde_json::json!({"op": "undo"}))?;
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

/// `amonhen find-repositories <dir> [--depth <n>] [--exclude <name>]…`: `dir` の下のリポジトリを、`dir` からの相対
/// パスで 1 行ずつ出す（アプリのリポジトリの一覧の「スキャンするフォルダ」と同じ規則。SSH 先のスキャンで使う）。
/// `--exclude` を 1 つでも渡すと、既定の除外（node_modules など）の代わりにそれを使う。
fn find_repositories(args: &[String]) -> Result<ExitCode> {
    let mut dir: Option<&str> = None;
    let mut depth = 3;
    let mut excludes: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--depth" => {
                depth = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--depth には数を渡してください")?;
                i += 1;
            }
            "--exclude" => {
                excludes.push(
                    args.get(i + 1)
                        .ok_or("--exclude には名前を渡してください")?
                        .clone(),
                );
                i += 1;
            }
            other if dir.is_none() && !other.starts_with("--") => dir = Some(other),
            other => return Err(format!("知らない引数: {other}\n\n{USAGE}").into()),
        }
        i += 1;
    }
    let dir = dir.ok_or("探すフォルダを渡してください")?;
    if excludes.is_empty() {
        excludes = amonhen_model::scan::DEFAULT_EXCLUDES
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
    }
    for path in amonhen_model::scan::find_repositories(Path::new(dir), depth, &excludes) {
        println!("{}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn merge_file(args: &[String]) -> Result<ExitCode> {
    let mut rules = amonhen_core::conflict::MagicRules::Safe;
    let mut check = false;
    let mut files = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--all" => rules = amonhen_core::conflict::MagicRules::All,
            "--check" | "--dry-run" => check = true,
            s if s.starts_with('-') => {
                return Err(format!("知らないオプション: {s}\n\n{USAGE}").into());
            }
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
        println!(
            "{}: コンフリクト {before} 個のうち {resolved} 個を{verb}、残り {remaining} 個",
            file.display()
        );
    }
    Ok(if remaining_total == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
