//! Working Copy とコミットの中身: status・diff・show・stage・unstage・discard・commit・refs。
//! どれも `--json` で、FE が読む形（人が読む形と同じ中身）を出す。

use std::path::PathBuf;
use std::process::ExitCode;

use amonhen_core::diff::{FileDiff, StackedDiff};
use amonhen_core::{Repository, WorkingDiff};
use amonhen_layout::DiffRow;
use amonhen_model::{ChangeKind, FileChange, Oid, RefKind, StatusEntry, StatusKind};
use amonhen_projection::FrontendProfile;
use serde_json::{Value, json};

use crate::{Args, Result, USAGE, command, open_repository, term};

/// 命令ごとに共通の引数（`-C`・`--json`・`--color`）と、残り。
struct Common {
    dir: PathBuf,
    json: bool,
    color: bool,
    rest: Vec<String>,
}

/// `flags` は受け付ける指定、`valued` はそのうち値をとるもの（`-m <message>`・`--rows <rows>`）。
fn common(args: &[String], flags: &[&str], valued: &[&str]) -> Result<(Common, Vec<String>)> {
    let mut c = Common { dir: PathBuf::from("."), json: false, color: term::stdout_is_tty(), rest: Vec::new() };
    let mut seen = Vec::new();
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => c.dir = PathBuf::from(a.value("-C")?),
            "--json" => c.json = true,
            "--color" | "--color=always" => c.color = true,
            "--color=never" | "--no-color" => c.color = false,
            "--color=auto" => c.color = term::stdout_is_tty(),
            s if flags.contains(&s) => {
                seen.push(s.to_owned());
                if valued.contains(&s) {
                    seen.push(a.value(s)?.to_owned());
                }
            }
            "--" => c.rest.push(arg.clone()),
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("知らないオプション: {s}\n\n{USAGE}").into()),
            _ => c.rest.push(arg.clone()),
        }
    }
    Ok((c, seen))
}

fn flag(seen: &[String], name: &str) -> bool {
    seen.iter().any(|s| s == name)
}

fn value<'a>(seen: &'a [String], name: &str) -> Option<&'a str> {
    seen.iter().position(|s| s == name).and_then(|i| seen.get(i + 1)).map(String::as_str)
}

fn print_json(value: &Value) {
    println!("{value}");
}

// MARK: - status

pub fn status(args: &[String]) -> Result<ExitCode> {
    let (c, _) = common(args, &[], &[])?;
    let repo = open_repository(&c.dir)?;
    let status = repo.load_status()?.status.clone();
    if c.json {
        print_json(&json!({
            "branch": status.branch,
            "upstream": status.upstream,
            "ahead": status.ahead,
            "behind": status.behind,
            "entries": status.entries.iter().map(entry_json).collect::<Vec<_>>(),
        }));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    match &status.branch {
        Some(b) => out.push_str(&format!("ブランチ {b}")),
        None => out.push_str("detached HEAD"),
    }
    if let Some(u) = &status.upstream {
        out.push_str(&format!("（{u}"));
        if status.ahead > 0 {
            out.push_str(&format!("・{} 先", status.ahead));
        }
        if status.behind > 0 {
            out.push_str(&format!("・{} 遅れ", status.behind));
        }
        out.push('）');
    }
    out.push('\n');
    if status.entries.is_empty() {
        out.push_str("変更はありません\n");
    }
    for e in &status.entries {
        let xy = format!("{}{}", e.index as char, e.worktree as char);
        let colored = match (e.kind, c.color) {
            (_, false) => xy,
            (StatusKind::Unmerged, true) => format!("\x1b[31m{xy}\x1b[0m"),
            (StatusKind::Untracked, true) => format!("\x1b[90m{xy}\x1b[0m"),
            (_, true) => format!(
                "\x1b[32m{}\x1b[31m{}\x1b[0m",
                e.index as char, e.worktree as char
            ),
        };
        match &e.orig_path {
            Some(from) => out.push_str(&format!("{colored} {from} → {}\n", e.path)),
            None => out.push_str(&format!("{colored} {}\n", e.path)),
        }
    }
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}

fn entry_json(e: &StatusEntry) -> Value {
    json!({
        "kind": match e.kind {
            StatusKind::Changed => "changed",
            StatusKind::Renamed => "renamed",
            StatusKind::Unmerged => "unmerged",
            StatusKind::Untracked => "untracked",
        },
        "path": e.path,
        "origPath": e.orig_path,
        "index": (e.index as char).to_string(),
        "worktree": (e.worktree as char).to_string(),
        "staged": e.is_staged(),
        "unstaged": e.is_unstaged(),
        "submodule": e.submodule,
    })
}

// MARK: - diff・show

/// `amonhen diff`: 作業ツリーの差分。`--staged` なら stage 済みの差分、`--combined` なら HEAD と作業ツリーの差分に
/// 行ごとの stage の印をつける。`<rev>` を 1 つ渡せばそのコミットの、2 つ渡せばその間の差分。
pub fn diff(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--staged", "--cached", "--combined", "--rows", "--stat"], &[])?;
    let repo = open_repository(&c.dir)?;
    let rows = flag(&seen, "--rows");
    let stat = flag(&seen, "--stat");
    // `--` より前で、パスでなくコミットとして読めるものはコミット。
    let (revs, paths): (Vec<String>, Vec<String>) = split_revs(&repo, &c.rest);
    if !revs.is_empty() {
        let ids = revs.iter().map(|r| resolve(&repo, r)).collect::<Result<Vec<_>>>()?;
        let stacked = match ids.as_slice() {
            [one] => repo.stacked_diff(&filter_paths(repo.commit_changes(one)?, &paths))?,
            [from, to] => {
                let mut s = between(&repo, from, to)?;
                s.files.retain(|(p, _)| wanted(p, &paths));
                s
            }
            _ => return Err("コミットは 1 つか 2 つです".into()),
        };
        return print_stacked(&stacked, &c, rows, stat);
    }
    let staged = flag(&seen, "--staged") || flag(&seen, "--cached");
    let combined = flag(&seen, "--combined");
    let status = repo.load_status()?.status.clone();
    let profile = FrontendProfile::MACOS;
    let mut files = Vec::new();
    for e in status.entries.iter().filter(|e| wanted(&e.path, &paths)) {
        if e.kind == StatusKind::Unmerged {
            continue;
        }
        let wanted = if combined { e.is_staged() || e.is_unstaged() } else if staged { e.is_staged() } else { e.is_unstaged() };
        if !wanted {
            continue;
        }
        let d = if combined {
            match repo.working_diff_combined(e, &profile)? {
                Some(d) => d,
                None => repo.working_diff(e, false, &profile)?,
            }
        } else {
            repo.working_diff(e, staged, &profile)?
        };
        files.push(d);
    }
    if c.json {
        print_json(&Value::Array(
            files.iter().map(|d| file_json(&d.entry.path, d.entry.orig_path.as_deref(), &d.file, d.combined.as_deref())).collect(),
        ));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    for d in &files {
        if stat {
            stat_line(&mut out, &d.entry.path, &d.file);
        } else {
            render_file(&mut out, &d.entry.path, d.entry.orig_path.as_deref(), &d.file, rows, d.combined.as_deref(), c.color);
        }
    }
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}

/// `amonhen show [<rev>]`: コミットの詳細と差分。
pub fn show(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--stat", "--rows", "--no-patch"], &[])?;
    let repo = open_repository(&c.dir)?;
    let rev = c.rest.first().map_or("HEAD", String::as_str);
    let id = resolve(&repo, rev)?;
    let info = command(&repo, json!({"op": "commitInfo", "oid": id.to_string()}))?["info"].clone();
    let changes = repo.commit_changes(&id)?;
    let no_patch = flag(&seen, "--no-patch");
    if c.json {
        let diffs = if no_patch { Vec::new() } else { repo.stacked_diff(&changes)?.files };
        print_json(&json!({
            "info": info,
            "changes": changes.iter().map(change_json).collect::<Vec<_>>(),
            "diffs": diffs.iter().map(|(p, d)| file_json(p, None, d, None)).collect::<Vec<_>>(),
        }));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    let paint = |s: &str, code: &str| if c.color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_owned() };
    out.push_str(&paint(&format!("commit {id}"), "33"));
    out.push('\n');
    if let Some(parents) = info["parents"].as_array().filter(|p| p.len() > 1) {
        let short: Vec<&str> = parents.iter().filter_map(|p| p.as_str()).map(|p| &p[..p.len().min(8)]).collect();
        out.push_str(&format!("Merge: {}\n", short.join(" ")));
    }
    out.push_str(&format!(
        "Author: {} <{}>\n",
        info["author"].as_str().unwrap_or(""),
        info["authorEmail"].as_str().unwrap_or("")
    ));
    if let Some(t) = info["authorTime"].as_i64() {
        out.push_str(&format!("Date:   {}\n", term::local_time(t, true)));
    }
    out.push('\n');
    for line in info["message"].as_str().unwrap_or("").trim_end().lines() {
        out.push_str(&format!("    {line}\n"));
    }
    out.push('\n');
    term::write(&out);
    if no_patch {
        return Ok(ExitCode::SUCCESS);
    }
    print_stacked(&repo.stacked_diff(&changes)?, &c, flag(&seen, "--rows"), flag(&seen, "--stat"))
}

fn print_stacked(stacked: &StackedDiff, c: &Common, rows: bool, stat: bool) -> Result<ExitCode> {
    if c.json {
        print_json(&Value::Array(stacked.files.iter().map(|(p, d)| file_json(p, None, d, None)).collect()));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    for (path, d) in &stacked.files {
        if stat {
            stat_line(&mut out, path, d);
        } else {
            render_file(&mut out, path, None, d, rows, None, c.color);
        }
    }
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}

/// 引数のうち、コミットとして読めるものと、パス（`--` の後ろはいつもパス）。
fn split_revs(repo: &Repository, rest: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut revs, mut paths) = (Vec::new(), Vec::new());
    let mut after_dashes = false;
    for arg in rest {
        if arg == "--" {
            after_dashes = true;
        } else if !after_dashes && paths.is_empty() && resolve(repo, arg).is_ok() {
            revs.push(arg.clone());
        } else {
            paths.push(arg.clone());
        }
    }
    (revs, paths)
}

fn wanted(path: &str, paths: &[String]) -> bool {
    paths.is_empty() || paths.iter().any(|p| path == p || path.starts_with(&format!("{p}/")))
}

fn filter_paths(changes: Vec<FileChange>, paths: &[String]) -> Vec<FileChange> {
    changes.into_iter().filter(|c| wanted(&c.path, paths)).collect()
}

/// `from` から `to` までの差分（History で 2 つのコミットを選んだときと同じ）。
fn between(repo: &Repository, from: &Oid, to: &Oid) -> Result<StackedDiff> {
    let history = repo.load_history()?;
    let row = |id: &Oid| history.row_of(id).ok_or_else(|| format!("History にないコミット: {id}"));
    let (_, stacked) = repo.selection_diff(&history, &[row(from)?, row(to)?], amonhen_core::PairMode::Forward)?;
    Ok(stacked)
}

/// `rev`（ブランチ・タグ・短い ID・`HEAD~2` など）をコミットの ID に。
pub fn resolve(repo: &Repository, rev: &str) -> Result<Oid> {
    let r = command(repo, json!({"op": "resolveCommit", "rev": rev}))?;
    r["text"]
        .as_str()
        .and_then(Oid::from_hex)
        .ok_or_else(|| format!("コミットが見つかりません: {rev}").into())
}

fn change_json(c: &FileChange) -> Value {
    json!({
        "kind": kind_name(c.kind),
        "path": c.path,
        "oldPath": c.old_path,
        "oldId": c.old_id.map(|i| i.to_string()),
        "newId": c.new_id.map(|i| i.to_string()),
        "submodule": c.submodule,
    })
}

fn kind_name(k: ChangeKind) -> &'static str {
    match k {
        ChangeKind::Added => "added",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Modified => "modified",
        ChangeKind::Renamed => "renamed",
        ChangeKind::Copied => "copied",
    }
}

/// 差分 1 ファイルの JSON。`rows` の i 番目が差分の i 行目（`stage --rows` などで指す番号）。
fn file_json(path: &str, old_path: Option<&str>, d: &FileDiff, staged: Option<&[bool]>) -> Value {
    let rows: Vec<Value> = d
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let mut row = json!({
                "kind": match l.kind {
                    DiffRow::ADDED => "added",
                    DiffRow::REMOVED => "removed",
                    DiffRow::HUNK => "hunk",
                    _ => "context",
                },
                "old": l.old_no,
                "new": l.new_no,
                "text": String::from_utf8_lossy(d.text(l)),
            });
            if let Some(s) = staged {
                row["staged"] = json!(s[i]);
            }
            row
        })
        .collect();
    json!({
        "path": path,
        "oldPath": old_path,
        "binary": d.binary,
        "added": d.added,
        "removed": d.removed,
        "rows": rows,
    })
}

fn stat_line(out: &mut String, path: &str, d: &FileDiff) {
    if d.binary {
        out.push_str(&format!("{path}  バイナリ\n"));
    } else {
        out.push_str(&format!("{path}  +{} -{}\n", d.added, d.removed));
    }
}

/// git の diff と同じ形で出す。`rows` なら行の番号（`stage --rows` などで指す）を左に、`staged` なら stage 済みの
/// 行に `●` を付ける。
fn render_file(out: &mut String, path: &str, old_path: Option<&str>, d: &FileDiff, rows: bool, staged: Option<&[bool]>, color: bool) {
    let paint = |s: &str, code: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_owned() };
    let header = match old_path {
        Some(from) if from != path => format!("diff {from} → {path}"),
        _ => format!("diff {path}"),
    };
    out.push_str(&paint(&header, "1"));
    out.push('\n');
    if d.binary {
        out.push_str("バイナリのファイル\n");
        return;
    }
    for (i, l) in d.lines.iter().enumerate() {
        if rows {
            out.push_str(&paint(&format!("{i:>5} "), "90"));
        }
        if let Some(s) = staged {
            out.push_str(if s[i] { "● " } else { "  " });
        }
        let text = String::from_utf8_lossy(d.text(l));
        match l.kind {
            DiffRow::HUNK => out.push_str(&paint(&text, "36")),
            DiffRow::ADDED => out.push_str(&paint(&format!("+{text}"), "32")),
            DiffRow::REMOVED => out.push_str(&paint(&format!("-{text}"), "31")),
            _ => {
                out.push(' ');
                out.push_str(&text);
            }
        }
        out.push('\n');
    }
}

// MARK: - stage・unstage・discard

/// `rows`（`3,5-9`）を行の番号に。
fn parse_rows(spec: &str) -> Result<Vec<usize>> {
    let mut rows = Vec::new();
    for part in spec.split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => rows.extend(a.trim().parse::<usize>()?..=b.trim().parse::<usize>()?),
            None => rows.push(part.trim().parse()?),
        }
    }
    Ok(rows)
}

fn entry_for(repo: &Repository, path: &str) -> Result<StatusEntry> {
    repo.load_status()?
        .status
        .entries
        .iter()
        .find(|e| e.path == path)
        .cloned()
        .ok_or_else(|| format!("変更のないファイル: {path}").into())
}

/// `stage`（`unstage` なら false）: パスごと、または `--rows` で差分の行ごと（`--combined` の差分の行番号。
/// `amonhen diff --combined --rows` で見る）。
pub fn stage(args: &[String], on: bool) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--rows", "--all", "-A"], &["--rows"])?;
    let repo = open_repository(&c.dir)?;
    if let Some(spec) = value(&seen, "--rows") {
        let [path] = c.rest.as_slice() else {
            return Err("--rows には、ファイルを 1 つ渡してください".into());
        };
        let d = combined(&repo, path)?;
        repo.set_rows_staged(&d, &parse_rows(spec)?, on)?;
        if !c.json {
            println!("{path} の {spec} 行目を {}", if on { "stage しました" } else { "unstage しました" });
        }
        return done(&c);
    }
    let paths: Vec<String> = if flag(&seen, "--all") || flag(&seen, "-A") {
        let status = repo.load_status()?.status.clone();
        status
            .entries
            .iter()
            .filter(|e| if on { e.is_unstaged() } else { e.is_staged() })
            .map(|e| e.path.clone())
            .collect()
    } else {
        c.rest.clone()
    };
    if paths.is_empty() {
        return Err("ファイルを渡してください（すべてなら --all）".into());
    }
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    if on { repo.stage(&refs)? } else { repo.unstage(&refs)? }
    if !c.json {
        println!("{} ファイルを {}", paths.len(), if on { "stage しました" } else { "unstage しました" });
    }
    done(&c)
}

fn combined(repo: &Repository, path: &str) -> Result<WorkingDiff> {
    let e = entry_for(repo, path)?;
    repo.working_diff_combined(&e, &FrontendProfile::MACOS)?
        .ok_or_else(|| format!("{path} は行ごとに stage できません（バイナリ・サブモジュール・コンフリクト中など）").into())
}

/// `discard`: 作業ツリーの変更を捨てる（Undo できる）。パスごと、または `--rows` で行ごと（stage していない差分の
/// 行番号。`amonhen diff --rows` で見る）。
pub fn discard(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--rows"], &["--rows"])?;
    let repo = open_repository(&c.dir)?;
    if c.rest.is_empty() {
        return Err("ファイルを渡してください".into());
    }
    if let Some(spec) = value(&seen, "--rows") {
        let [path] = c.rest.as_slice() else {
            return Err("--rows には、ファイルを 1 つ渡してください".into());
        };
        let e = entry_for(&repo, path)?;
        let d = repo.working_diff(&e, false, &FrontendProfile::MACOS)?;
        repo.discard_rows(&d, &parse_rows(spec)?)?;
        if !c.json {
            println!("{path} の {spec} 行目の変更を捨てました（amonhen undo で戻せます）");
        }
        return done(&c);
    }
    command(&repo, json!({"op": "discard", "paths": c.rest}))?;
    if !c.json {
        println!("{} ファイルの変更を捨てました（amonhen undo で戻せます）", c.rest.len());
    }
    done(&c)
}

/// `commit -m <message> [--amend]`（Undo できる）。
pub fn commit(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["-m", "--message", "--amend"], &["-m", "--message"])?;
    let repo = open_repository(&c.dir)?;
    let message = value(&seen, "-m").or_else(|| value(&seen, "--message")).ok_or("-m でメッセージを渡してください")?;
    repo.commit(message, flag(&seen, "--amend"))?;
    let head = resolve(&repo, "HEAD")?;
    if c.json {
        print_json(&json!({"ok": true, "commit": head.to_string()}));
    } else {
        println!("コミットしました: {} {}", &head.to_string()[..8], message.lines().next().unwrap_or(""));
    }
    Ok(ExitCode::SUCCESS)
}

fn done(c: &Common) -> Result<ExitCode> {
    if c.json {
        print_json(&json!({"ok": true}));
    }
    Ok(ExitCode::SUCCESS)
}

// MARK: - refs

pub fn refs(args: &[String]) -> Result<ExitCode> {
    let (c, _) = common(args, &[], &[])?;
    let repo = open_repository(&c.dir)?;
    let refs = repo.load_refs()?.refs.clone();
    let kind = |k: RefKind| match k {
        RefKind::Local => "branch",
        RefKind::Remote => "remote",
        RefKind::Tag => "tag",
        RefKind::Stash => "stash",
        RefKind::Submodule => "submodule",
    };
    if c.json {
        print_json(&json!({
            "headBranch": refs.head_branch,
            "head": refs.head.map(|h| h.to_string()),
            "entries": refs.entries.iter().map(|e| json!({
                "kind": kind(e.kind),
                "name": e.name,
                "target": e.target.map(|t| t.to_string()),
                "detail": e.detail,
                "ahead": e.ahead,
                "behind": e.behind,
                "gone": e.gone,
            })).collect::<Vec<_>>(),
        }));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    let mut last = None;
    for e in &refs.entries {
        if last != Some(e.kind) {
            out.push_str(match e.kind {
                RefKind::Local => "ブランチ\n",
                RefKind::Remote => "リモート\n",
                RefKind::Tag => "タグ\n",
                RefKind::Stash => "stash\n",
                RefKind::Submodule => "サブモジュール\n",
            });
            last = Some(e.kind);
        }
        let head = e.kind == RefKind::Local && refs.head_branch.as_deref() == Some(e.name.as_str());
        let target = e.target.map_or_else(|| "        ".to_owned(), |t| t.to_string()[..8].to_owned());
        let mut line = format!("  {} {target} {}", if head { "*" } else { " " }, e.name);
        if let Some(d) = &e.detail {
            line.push_str(&format!("  {d}"));
        }
        if e.gone {
            line.push_str("（上流がない）");
        } else if e.ahead > 0 || e.behind > 0 {
            line.push_str(&format!("（{} 先・{} 遅れ）", e.ahead, e.behind));
        }
        out.push_str(&line);
        out.push('\n');
    }
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}
