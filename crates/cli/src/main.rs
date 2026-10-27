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
//!   amonhen worktree [list|add|remove|prune]  worktree（`git worktree` と同じ並び）
//!   amonhen merge-file [--all] <file>…  コンフリクトのあるファイルに Magic Merge をかける
//!   amonhen merge-check <rev>           マージしたらコンフリクトするファイル（作業ツリーには触らない）
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
use amonhen_model::tr;

/// 使い方（`--help` と、引数を間違えたとき）。
pub(crate) fn usage() -> String {
    tr!("\
usage: amonhen [<path>]
       amonhen open [<path> | ssh://host/path] [--daemon] [--snapshot]
       amonhen log [-n <count> | --all] [--fold] [--topo-order] [--branch <name>]… [--no-graph] [--rebuild] [--json] [<query>…]
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
       amonhen worktree [list [--changes] [--json] | add <path> [<start>] [-b <branch>] | remove [--force] <path> | prune]
       amonhen merge-file [--all] [--check] <file>…
       amonhen merge-check [--into <branch>] [--json] <rev>
       amonhen clone <url> [<dir>]
       amonhen daemon [status [--json] | start | stop | rebuild [<path> | ssh://host/path]]
       amonhen install [--quiet] [--no-link] [--force]   (puts the engine in ~/.local/share/amonhen and links ~/.local/bin/amonhen)
       amonhen engine   (the engine to start as the shared daemon)
       amonhen find-repositories <dir> [--depth <n>] [--exclude <name>]…   (repositories under a folder; used to scan SSH hosts)
       amonhen --version
       amonhen --serve [<socket>] | --stdio <repo> | --socket   (runs as a daemon; used by FEs and SSH hosts)

Commands that work on a repository choose it with -C <path>, -C ssh://host/path (through the shared daemon) or
-C daemon:<path>. Besides the human-readable form, --json prints the form FEs read (also --color=auto|always|never).
log's <query> uses the History search syntax (author:name path:path contents:\"text\" and so on).
diff shows unstaged changes by default, staged changes with --staged, and with --combined the changes since HEAD with
staged lines marked (●). With one <rev> it shows that commit's diff; with two, the same diff as selecting two commits in
History (if they connect along first-parent, from the older one's parent to the newer one).
--rows prints diff line numbers. The --rows <rows> of stage and unstage (like 3,5-9) are diff --combined line numbers,
and discard's --rows are diff (unstaged) line numbers; only the changes on those lines are touched. discard and commit
can be undone with undo.
run takes commands like {\"op\":\"checkout\",\"branch\":\"main\"} (the core's command.rs). Each answer is one line of JSON.
log --rebuild and daemon rebuild throw away the History cache and build it again (the shared daemon's too).
merge-file applies only the rules that can't go wrong by default (--all for every rule). If conflicts remain the exit
code is 1, so it also works as git's mergetool:
  git config mergetool.amonhen.cmd 'amonhen merge-file \"$MERGED\"'
  git config mergetool.amonhen.trustExitCode true
merge-check lists the files that merging <rev> into HEAD (or --into <branch>) would conflict in, without touching the
worktree. The exit code is 1 if there are conflicts, and 2 if it can't tell (git older than 2.38).")
}

fn main() -> ExitCode {
    amonhen_model::i18n::set_default(amonhen_model::i18n::Lang::from_env());
    #[cfg(target_os = "macos")]
    amonhen_model::process::set_agent_finder(session_agent);
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
            println!("{}", usage());
            Ok(ExitCode::SUCCESS)
        }
        Some("-V" | "--version" | "version") => {
            println!("{}", amonhen_daemon::version());
            Ok(ExitCode::SUCCESS)
        }
        Some("open") => open::run(&args[1..]),
        Some("log") => log(&args[1..]),
        Some("undo") => undo(&args[1..]),
        Some("worktree") => worktree(&args[1..]),
        Some("merge-file") => merge_file(&args[1..]),
        Some("merge-check") => work::merge_check(&args[1..]),
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

/// このログインの SSH の agent（launchd が用意するもの）。環境の `SSH_AUTH_SOCK` が死んでいたときに使う（共有の
/// daemon は、起動した FE の環境を終わるまで持ち続ける）。
#[cfg(target_os = "macos")]
fn session_agent() -> Option<std::ffi::OsString> {
    let out = amonhen_model::process::command("launchctl")
        .args(["getenv", "SSH_AUTH_SOCK"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    (out.status.success() && !path.is_empty()).then(|| path.into())
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
            .ok_or_else(|| tr!("{name} needs a value", name = name).into())
    }
}

/// `path` を含むリポジトリの作業ツリーの一番上（`git rev-parse --show-toplevel`）。
pub(crate) fn repository_root(path: &Path) -> Result<PathBuf> {
    if let Some(reason) = amonhen_model::process::missing_git() {
        return Err(reason.into());
    }
    let out = amonhen_model::process::command("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !out.status.success() {
        return Err(tr!("{path} isn’t a Git repository", path = path.display()).into());
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
                    "{}",
                    tr!(
                        "The shared daemon isn’t running ({endpoint})",
                        endpoint = amonhen_remote::endpoint::endpoint()
                    )
                );
                return Ok(ExitCode::from(1));
            };
            if args.iter().any(|a| a == "--json") {
                println!("{json}");
                return Ok(ExitCode::SUCCESS);
            }
            let s: serde_json::Value = serde_json::from_str(&json)?;
            let secs = |ms: &serde_json::Value| ms.as_u64().unwrap_or(0) / 1000;
            println!("{}", tr!("Shared daemon: running"));
            println!("  pid          {}", s["pid"]);
            println!(
                "{}",
                tr!(
                    "  version      {version} (build {build})",
                    version = s["version"].as_str().unwrap_or(""),
                    build = s["build"].as_str().unwrap_or("")
                )
            );
            println!(
                "{}",
                tr!(
                    "  socket       {socket}",
                    socket = s["socket"].as_str().unwrap_or("")
                )
            );
            println!(
                "{}",
                tr!(
                    "  uptime       {duration}",
                    duration = duration(secs(&s["uptimeMs"]))
                )
            );
            println!(
                "{}",
                tr!("  connections  {count}", count = s["connections"])
            );
            let repos = s["repos"].as_array().cloned().unwrap_or_default();
            if repos.is_empty() {
                println!("{}", tr!("  repositories none"));
            }
            for r in repos {
                let index = match (r["indexRows"].as_u64(), r["indexBytes"].as_u64()) {
                    (Some(rows), Some(bytes)) => {
                        tr!(
                            "search index {rows} rows, {size} MB",
                            rows = rows,
                            size = format!("{:.1}", bytes as f64 / 1e6)
                        )
                    }
                    _ => tr!("no search index"),
                };
                let clients = r["clients"].as_u64().unwrap_or(0);
                let state = match clients {
                    0 => tr!(
                        "no connections (idle for {duration})",
                        duration = duration(secs(&r["idleMs"]))
                    ),
                    1 => tr!("1 connection"),
                    n => tr!("{count} connections", count = n),
                };
                println!("  {}", r["path"].as_str().unwrap_or(""));
                if r["ssh"] == true {
                    let mb = |v: &serde_json::Value| v.as_u64().unwrap_or(0) as f64 / 1e6;
                    match r["closed"].as_str() {
                        Some(reason) => println!(
                            "{}",
                            tr!("      SSH connection: closed ({reason})", reason = reason)
                        ),
                        None => println!(
                            "{}",
                            tr!(
                                "      SSH connection: sent {sent} MB, received {received} MB",
                                sent = format!("{:.1}", mb(&r["sent"])),
                                received = format!("{:.1}", mb(&r["received"]))
                            )
                        ),
                    }
                    if let Some(engine) = r["engine"].as_str() {
                        println!(
                            "{}",
                            tr!("      SSH host engine: {engine}", engine = engine)
                        );
                    }
                }
                let yes_no =
                    |v: &serde_json::Value| if *v == true { tr!("yes") } else { tr!("no") };
                println!(
                    "{}",
                    tr!(
                        "      {state}, History {history}, watching {watching}, {index}",
                        state = state,
                        history = yes_no(&r["history"]),
                        watching = yes_no(&r["watching"]),
                        index = index
                    )
                );
            }
            println!(
                "{}",
                tr!(
                    "  Exits after {duration} with no connections",
                    duration = duration(secs(&s["idleExitMs"]))
                )
            );
            Ok(ExitCode::SUCCESS)
        }
        "start" => {
            amonhen_remote::local::start(&daemon_program()?)?;
            println!(
                "{}",
                tr!(
                    "Started the shared daemon ({endpoint})",
                    endpoint = amonhen_remote::endpoint::endpoint()
                )
            );
            Ok(ExitCode::SUCCESS)
        }
        "stop" => match amonhen_remote::local::control("stop")? {
            Some(_) => {
                println!("{}", tr!("Stopped the shared daemon"));
                Ok(ExitCode::SUCCESS)
            }
            None => {
                println!("{}", tr!("The shared daemon isn’t running"));
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
                "{}",
                tr!(
                    "Rebuilt History ({count} commits, {seconds} s)",
                    count = history["rows"],
                    seconds = format!("{:.2}", started.elapsed().as_secs_f64())
                )
            );
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!(
            "{}\n\n{}",
            tr!("Unknown subcommand: {name}", name = other),
            usage()
        )
        .into()),
    }
}

/// 秒を「1 時間 2 分」のように。
fn duration(secs: u64) -> String {
    match secs {
        s if s < 60 => tr!("{s} s", s = s),
        s if s < 3600 && s % 60 == 0 => tr!("{m} min", m = s / 60),
        s if s < 3600 => tr!("{m} min {s} s", m = s / 60, s = s % 60),
        s => tr!("{h} h {m} min", h = s / 3600, m = s % 3600 / 60),
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
            .map_or_else(|| tr!("Failed"), str::to_owned)
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
    let mut topo_order = false;
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "-n" => count = Some(a.value("-n")?.parse()?),
            "--all" => count = Some(usize::MAX),
            "--fold" => fold = true,
            "--topo-order" => topo_order = true,
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
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
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
                .ok_or_else(|| tr!("Branch not found: {name}", name = name))
        })
        .collect::<std::result::Result<_, _>>()?;
    if topo_order {
        call(
            &*front,
            serde_json::json!({"op": "historyOrder", "order": "topo"}),
        )?;
    }
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
            s => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown argument: {argument}", argument = s),
                    usage()
                )
                .into());
            }
        }
    }
    let front = open_repository(&dir)?;
    match mode {
        "list" => {
            let r = command(&*front, serde_json::json!({"op": "undoLog"}))?;
            let entries = r["info"]["entries"].as_array().cloned().unwrap_or_default();
            if entries.is_empty() {
                println!("{}", tr!("Nothing to undo"));
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
                Some(label) => println!("{}", tr!("Next to undo: {label}", label = label)),
                None => println!("{}", tr!("Nothing to undo")),
            }
        }
        _ => {
            let r = command(&*front, serde_json::json!({"op": "undo"}))?;
            match r["label"].as_str() {
                Some(label) => println!("{}", tr!("Undid: {label}", label = label)),
                None => {
                    println!("{}", tr!("Nothing to undo"));
                    return Ok(ExitCode::from(1));
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

// MARK: - worktree

/// `amonhen worktree`: `git worktree` と同じ並び（list・add・remove・prune）。コアの命令を通す（SSH 先も同じ）。
fn worktree(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut json = false;
    let mut force = false;
    let mut changes = false;
    let mut branch: Option<String> = None;
    let mut words: Vec<String> = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--json" => json = true,
            "--force" | "-f" => force = true,
            "--changes" => changes = true,
            "-b" => branch = Some(a.value("-b")?.to_owned()),
            s if s.starts_with('-') => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            s => words.push(s.to_owned()),
        }
    }
    let front = open_repository(&dir)?;
    let absolute = |path: &str| -> String {
        std::path::absolute(path).map_or_else(|_| path.to_owned(), |p| p.display().to_string())
    };
    match words.first().map(String::as_str).unwrap_or("list") {
        "list" => {
            let r = command(&*front, serde_json::json!({"op": "worktrees"}))?;
            let mut list = r["info"]["worktrees"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            // --changes なら、それぞれの変更のあるファイルの数（開いたリポジトリから 1 回で読む）を `changes` に。
            if changes {
                let paths: Vec<&str> = list.iter().filter_map(|w| w["path"].as_str()).collect();
                let r = command(
                    &*front,
                    serde_json::json!({"op": "worktreeChanges", "paths": paths, "fresh": true}),
                )?;
                for c in r["info"]["worktrees"].as_array().into_iter().flatten() {
                    if let Some(w) = list.iter_mut().find(|w| w["path"] == c["path"]) {
                        w["changes"] = c["count"].clone();
                    }
                }
            }
            if json {
                println!("{}", serde_json::Value::Array(list));
                return Ok(ExitCode::SUCCESS);
            }
            for w in &list {
                let head = w["head"].as_str().map_or("", |h| &h[..h.len().min(8)]);
                let what = w["branch"]
                    .as_str()
                    .map_or_else(|| tr!("(detached)"), |b| format!("[{b}]"));
                let mut marks = Vec::new();
                if w["current"] == true {
                    marks.push(tr!("current"));
                }
                if w["locked"].is_string() {
                    marks.push(tr!("locked"));
                }
                if w["prunable"].is_string() {
                    marks.push(tr!("prunable"));
                }
                if let Some(n) = w["changes"].as_u64().filter(|&n| n > 0) {
                    marks.push(tr!("{count} changed", count = n));
                }
                let marks = if marks.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", marks.join(", "))
                };
                println!(
                    "{}  {head}  {what}{marks}",
                    w["path"].as_str().unwrap_or("")
                );
            }
        }
        "add" => {
            let Some(path) = words.get(1) else {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Give the folder for the new worktree"),
                    usage()
                )
                .into());
            };
            command(
                &*front,
                serde_json::json!({"op": "worktreeAdd", "path": absolute(path), "start": words.get(2), "newBranch": branch}),
            )?;
            println!(
                "{}",
                tr!("Created the worktree {path}", path = absolute(path))
            );
        }
        "remove" => {
            let Some(path) = words.get(1) else {
                return Err(
                    format!("{}\n\n{}", tr!("Give the worktree to remove"), usage()).into(),
                );
            };
            command(
                &*front,
                serde_json::json!({"op": "worktreeRemove", "path": absolute(path), "force": force}),
            )?;
            println!(
                "{}",
                tr!("Removed the worktree {path}", path = absolute(path))
            );
        }
        "prune" => {
            command(&*front, serde_json::json!({"op": "worktreePrune"}))?;
        }
        other => {
            return Err(format!(
                "{}\n\n{}",
                tr!("Unknown argument: {argument}", argument = other),
                usage()
            )
            .into());
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
                    .ok_or_else(|| tr!("--depth takes a number"))?;
                i += 1;
            }
            "--exclude" => {
                excludes.push(
                    args.get(i + 1)
                        .ok_or_else(|| tr!("--exclude takes a name"))?
                        .clone(),
                );
                i += 1;
            }
            other if dir.is_none() && !other.starts_with("--") => dir = Some(other),
            other => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown argument: {argument}", argument = other),
                    usage()
                )
                .into());
            }
        }
        i += 1;
    }
    let dir = dir.ok_or_else(|| tr!("Give the folder to search"))?;
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
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if files.is_empty() {
        return Err(format!("{}\n\n{}", tr!("Give the files"), usage()).into());
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
        let line = if check {
            tr!(
                "{file}: {resolved} of {before} conflicts can be resolved, {remaining} left",
                file = file.display(),
                before = before,
                resolved = resolved,
                remaining = remaining
            )
        } else {
            tr!(
                "{file}: resolved {resolved} of {before} conflicts, {remaining} left",
                file = file.display(),
                before = before,
                resolved = resolved,
                remaining = remaining
            )
        };
        println!("{line}");
    }
    Ok(if remaining_total == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
