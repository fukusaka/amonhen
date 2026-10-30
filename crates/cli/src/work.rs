//! Working Copy とコミットの中身: status・diff・show・stage・unstage・discard・commit・refs。どれもコアへの窓口
//! （`amonhen-front`）だけを使う。`--json` で、FE が読む形（人が読む形と同じ中身）を出す。

use std::path::PathBuf;
use std::process::ExitCode;

use amonhen_front::{Front, Reply};
use amonhen_layout::DiffRow;
use serde_json::{Value, json};

use amonhen_model::tr;

use crate::{Args, Result, call, command, open_repository, term, usage};

/// 命令ごとに共通の引数（`-C`・`--json`・`--color`）と、残り。
struct Common {
    dir: PathBuf,
    json: bool,
    color: bool,
    rest: Vec<String>,
}

/// `flags` は受け付ける指定、`valued` はそのうち値をとるもの（`-m <message>`・`--rows <rows>`）。
fn common(args: &[String], flags: &[&str], valued: &[&str]) -> Result<(Common, Vec<String>)> {
    let mut c = Common {
        dir: PathBuf::from("."),
        json: false,
        color: term::stdout_is_tty(),
        rest: Vec::new(),
    };
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
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            _ => c.rest.push(arg.clone()),
        }
    }
    Ok((c, seen))
}

fn flag(seen: &[String], name: &str) -> bool {
    seen.iter().any(|s| s == name)
}

fn value<'a>(seen: &'a [String], name: &str) -> Option<&'a str> {
    seen.iter()
        .position(|s| s == name)
        .and_then(|i| seen.get(i + 1))
        .map(String::as_str)
}

fn print_json(value: &Value) {
    println!("{value}");
}

fn paint(s: &str, code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_owned()
    }
}

// MARK: - status

pub fn status(args: &[String]) -> Result<ExitCode> {
    let (c, _) = common(args, &[], &[])?;
    let front = open_repository(&c.dir)?;
    let status = call(&*front, json!({"op": "status"}))?.json;
    if c.json {
        print_json(&status);
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    match status["branch"].as_str() {
        Some(b) => out.push_str(&tr!("On branch {branch}", branch = b)),
        None => out.push_str("detached HEAD"),
    }
    if let Some(u) = status["upstream"].as_str() {
        let mut items = vec![u.to_owned()];
        let (ahead, behind) = (
            status["ahead"].as_u64().unwrap_or(0),
            status["behind"].as_u64().unwrap_or(0),
        );
        if ahead > 0 {
            items.push(tr!("{count} ahead", count = ahead));
        }
        if behind > 0 {
            items.push(tr!("{count} behind", count = behind));
        }
        out.push_str(&tr!(" ({items})", items = items.join(&tr!(", "))));
    }
    out.push('\n');
    // リポジトリの形（shallow・partial clone・sparse checkout・maintenance の登録）。git status は言わないが、
    // ないファイルや履歴のわけになる。読めなければ出さない。
    if let Ok(form) = command(&*front, json!({"op": "repositoryForm"})) {
        let form = &form["info"];
        let mut items = Vec::new();
        if form["shallow"] == true {
            items.push(tr!("shallow clone"));
        }
        if let Some(partial) = form["partial"].as_object() {
            items.push(match partial["filter"].as_str() {
                Some(filter) => tr!("partial clone ({filter})", filter = filter),
                None => tr!("partial clone"),
            });
        }
        if let Some(sparse) = form["sparse"].as_object() {
            items.push(match sparse["folders"].as_array() {
                Some(folders) if sparse["cone"] == true && folders.len() == 1 => {
                    tr!("sparse checkout (1 folder)")
                }
                Some(folders) if sparse["cone"] == true => {
                    tr!("sparse checkout ({count} folders)", count = folders.len())
                }
                _ => tr!("sparse checkout"),
            });
        }
        if form["maintenance"] == true {
            items.push(tr!("git maintenance"));
        }
        if !items.is_empty() {
            out.push_str(&paint(&items.join(" · "), "90", c.color));
            out.push('\n');
        }
    }
    let entries = status["entries"].as_array().cloned().unwrap_or_default();
    if entries.is_empty() {
        out.push_str(&tr!("No changes"));
        out.push('\n');
    }
    for e in &entries {
        let (x, y) = (
            e["index"].as_str().unwrap_or(" "),
            e["worktree"].as_str().unwrap_or(" "),
        );
        let xy = match e["kind"].as_str() {
            Some("unmerged") => paint(&format!("{x}{y}"), "31", c.color),
            Some("untracked") => paint(&format!("{x}{y}"), "90", c.color),
            _ => format!("{}{}", paint(x, "32", c.color), paint(y, "31", c.color)),
        };
        let path = e["path"].as_str().unwrap_or("");
        match e["origPath"].as_str() {
            Some(from) => out.push_str(&format!("{xy} {from} → {path}")),
            None => out.push_str(&format!("{xy} {path}")),
        }
        // マーカーの残っていないコンフリクト（rerere が解決を当てた、ほかのエディタで直した）。
        if e["markers"] == 0 {
            let note = if e["rerere"] == true {
                tr!("rerere applied the earlier resolution")
            } else {
                tr!("no conflict markers")
            };
            out.push_str(&paint(&format!("  ({note})"), "90", c.color));
        }
        out.push('\n');
    }
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}

// MARK: - diff・show

/// 窓口の差分（`workingDiff`・`commitDiff`・`conflict` の答え）の行: 種類・印・前後の行番号・文字列。
struct DiffRows {
    rows: Vec<(u8, u8, u32, u32, String)>,
}

impl DiffRows {
    fn from(reply: &Reply) -> Self {
        let raw = reply.u32s("rows");
        let lengths = reply.u32s("textLengths");
        let text = reply.section("text");
        let rows = raw
            .as_chunks::<4>()
            .0
            .iter()
            .zip(lengths)
            .map(|(r, len)| {
                let off = r[3] as usize;
                let s = String::from_utf8_lossy(&text[off..off + len as usize]).into_owned();
                (
                    (r[0] & 0xff) as u8,
                    ((r[0] >> 8) & 0xff) as u8,
                    r[1],
                    r[2],
                    s,
                )
            })
            .collect();
        Self { rows }
    }

    fn kind(kind: u8) -> &'static str {
        match kind {
            DiffRow::ADDED => "added",
            DiffRow::REMOVED => "removed",
            DiffRow::HUNK => "hunk",
            DiffRow::FILE => "file",
            DiffRow::OURS => "ours",
            DiffRow::THEIRS => "theirs",
            DiffRow::BASE => "base",
            DiffRow::MARKER => "marker",
            _ => "context",
        }
    }

    fn json(&self, staged: bool) -> Vec<Value> {
        self.rows
            .iter()
            .map(|(kind, flags, old, new, text)| {
                let mut row =
                    json!({"kind": Self::kind(*kind), "old": old, "new": new, "text": text});
                if staged {
                    row["staged"] = json!(flags & DiffRow::STAGED != 0);
                }
                row
            })
            .collect()
    }

    /// git の diff と同じ形で出す。`numbers` なら行の番号（`stage --rows` などで指す）を左に、`staged` なら
    /// stage 済みの行に `●` を付ける。ファイルの見出しの行（`FILE`）は `diff <path>` にする。
    fn render(
        &self,
        out: &mut String,
        header: Option<&str>,
        numbers: bool,
        staged: bool,
        color: bool,
    ) {
        if let Some(h) = header {
            out.push_str(&paint(&format!("diff {h}"), "1", color));
            out.push('\n');
        }
        for (i, (kind, flags, _, _, text)) in self.rows.iter().enumerate() {
            if *kind == DiffRow::FILE {
                out.push_str(&paint(&format!("diff {text}"), "1", color));
                out.push('\n');
                continue;
            }
            if numbers {
                out.push_str(&paint(&format!("{i:>5} "), "90", color));
            }
            if staged {
                out.push_str(if flags & DiffRow::STAGED != 0 {
                    "● "
                } else {
                    "  "
                });
            }
            match *kind {
                DiffRow::HUNK => out.push_str(&paint(text, "36", color)),
                DiffRow::ADDED => out.push_str(&paint(&format!("+{text}"), "32", color)),
                DiffRow::REMOVED => out.push_str(&paint(&format!("-{text}"), "31", color)),
                _ => {
                    out.push(' ');
                    out.push_str(text);
                }
            }
            out.push('\n');
        }
    }
}

/// `amonhen diff`: 作業ツリーの差分。`--staged` なら stage 済みの差分、`--combined` なら HEAD と作業ツリーの差分に
/// 行ごとの stage の印をつける。`<rev>` を 1 つ渡せばそのコミットの、2 つ渡せば History で 2 つ選んだときの差分。
pub fn diff(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(
        args,
        &["--staged", "--cached", "--combined", "--rows", "--stat"],
        &[],
    )?;
    let front = open_repository(&c.dir)?;
    let numbers = flag(&seen, "--rows");
    let stat = flag(&seen, "--stat");
    let (revs, paths) = split_revs(&*front, &c.rest);
    if !revs.is_empty() {
        let ids = revs
            .iter()
            .map(|r| resolve(&*front, r))
            .collect::<Result<Vec<_>>>()?;
        let reply = match ids.as_slice() {
            [one] => call(&*front, json!({"op": "commitDiff", "id": one}))?,
            [from, to] => {
                let history = call(&*front, json!({"op": "history"}))?.json["history"].clone();
                let row = |id: &str| -> Result<Value> {
                    let r = call(
                        &*front,
                        json!({"op": "historyRowOf", "history": history, "id": id}),
                    )?
                    .json["row"]
                        .clone();
                    if r.is_null() {
                        Err(tr!("Commit not in History: {id}", id = id).into())
                    } else {
                        Ok(r)
                    }
                };
                call(
                    &*front,
                    json!({"op": "commitDiff", "history": history, "rows": [row(from)?, row(to)?]}),
                )?
            }
            _ => return Err(tr!("Give one or two commits").into()),
        };
        return print_commit_diff(&reply, &paths, &c, numbers, stat);
    }
    let staged = flag(&seen, "--staged") || flag(&seen, "--cached");
    let combined = flag(&seen, "--combined");
    let status = call(&*front, json!({"op": "status"}))?.json;
    let mut files = Vec::new();
    for e in status["entries"].as_array().into_iter().flatten() {
        let path = e["path"].as_str().unwrap_or("");
        if e["kind"] == "unmerged" || !wanted(path, &paths) {
            continue;
        }
        let (is_staged, is_unstaged) = (e["staged"] == true, e["unstaged"] == true);
        let shown = if combined {
            is_staged || is_unstaged
        } else if staged {
            is_staged
        } else {
            is_unstaged
        };
        if !shown {
            continue;
        }
        let mode = if combined {
            "combined"
        } else if staged {
            "staged"
        } else {
            "unstaged"
        };
        let reply = match call(
            &*front,
            json!({"op": "workingDiff", "path": path, "mode": mode}),
        ) {
            Ok(r) => r,
            // 行ごとの印をつけられない差分（バイナリなど）は、stage していない差分を出す。
            Err(_) if combined => call(
                &*front,
                json!({"op": "workingDiff", "path": path, "mode": "unstaged"}),
            )?,
            Err(e) => return Err(e),
        };
        files.push(reply);
    }
    if c.json {
        print_json(&Value::Array(
            files
                .iter()
                .map(|r| {
                    let staged = r.json["mode"] == "combined";
                    json!({
                        "path": r.json["path"],
                        "binary": r.json["binary"],
                        "added": r.json["added"],
                        "removed": r.json["removed"],
                        "rows": DiffRows::from(r).json(staged),
                        "lineEndings": r.json["lineEndings"],
                    })
                })
                .collect(),
        ));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    for r in &files {
        let path = r.json["path"].as_str().unwrap_or("");
        if stat {
            stat_line(&mut out, path, &r.json);
        } else if r.json["binary"] == true {
            out.push_str(&format!(
                "{}\n{}\n",
                paint(&format!("diff {path}"), "1", c.color),
                tr!("Binary file")
            ));
        } else {
            out.push_str(&paint(&format!("diff {path}"), "1", c.color));
            out.push('\n');
            if let Some(note) = line_endings_note(&r.json["lineEndings"]) {
                let warning = r.json["lineEndings"]["warning"] == true;
                out.push_str(&paint(&note, if warning { "33" } else { "90" }, c.color));
                out.push('\n');
            }
            DiffRows::from(r).render(
                &mut out,
                None,
                numbers,
                r.json["mode"] == "combined",
                c.color,
            );
        }
    }
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}

/// `amonhen show [<rev>]`: コミットの詳細と差分。
pub fn show(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--stat", "--rows", "--no-patch"], &[])?;
    let front = open_repository(&c.dir)?;
    let rev = c.rest.first().map_or("HEAD", String::as_str);
    let id = resolve(&*front, rev)?;
    let info = command(&*front, json!({"op": "commitInfo", "oid": id}))?["info"].clone();
    // 署名とタグ（コミットの署名、指すタグ、merge したタグ）。検証に gpg を使うので、どれかがあるときだけ読む。
    let has = |key: &str| info[key].as_array().is_some_and(|a| !a.is_empty());
    let extras = if !info["signed"].is_null() || has("refs") || has("mergedTags") {
        command(&*front, json!({"op": "commitExtras", "oid": id}))?["info"].clone()
    } else {
        Value::Null
    };
    let no_patch = flag(&seen, "--no-patch");
    let diff = call(&*front, json!({"op": "commitDiff", "id": id}))?;
    if c.json {
        let mut out = json!({
            "info": info,
            "signature": extras["signature"],
            "tags": extras.get("tagObjects").cloned().unwrap_or_else(|| json!([])),
            "mergedTagSignatures": extras.get("mergedTagSignatures").cloned().unwrap_or_else(|| json!([])),
            "changes": diff.json["changes"],
            "files": diff.json["files"],
        });
        if !no_patch {
            out["rows"] = json!(DiffRows::from(&diff).json(false));
        }
        print_json(&out);
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    out.push_str(&paint(&format!("commit {id}"), "33", c.color));
    out.push('\n');
    if let Some(parents) = info["parents"].as_array().filter(|p| p.len() > 1) {
        let short: Vec<&str> = parents
            .iter()
            .filter_map(|p| p.as_str())
            .map(|p| &p[..p.len().min(8)])
            .collect();
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
    if let Some(kind) = info["signed"].as_str() {
        let line = match extras.get("signature").filter(|s| !s.is_null()) {
            Some(signature) => signature_text(signature, kind),
            None => format!("No key ({})", kind.to_uppercase()),
        };
        out.push_str(&format!("Signature: {line}\n"));
    }
    for tag in extras["tagObjects"].as_array().into_iter().flatten() {
        let name = tag["name"].as_str().unwrap_or("");
        let mut line = format!("Tag: {name}");
        if tag["id"].is_null() {
            line.push_str(&tr!(" (lightweight)"));
        } else {
            line.push_str(&tag_text(
                tag["subject"].as_str(),
                tag["signed"].as_str(),
                &tag["signature"],
            ));
        }
        out.push_str(&format!("{line}\n"));
    }
    let merged_signatures = extras["mergedTagSignatures"].as_array();
    for (i, tag) in info["mergedTags"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let signature = merged_signatures
            .and_then(|s| s.get(i))
            .unwrap_or(&Value::Null);
        out.push_str(&format!(
            "Merged tag: {} ({}){}\n",
            tag["name"].as_str().unwrap_or(""),
            tag["tagger"].as_str().unwrap_or(""),
            tag_text(tag["subject"].as_str(), tag["signed"].as_str(), signature)
        ));
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
    print_commit_diff(&diff, &[], &c, flag(&seen, "--rows"), flag(&seen, "--stat"))
}

fn print_commit_diff(
    reply: &Reply,
    paths: &[String],
    c: &Common,
    numbers: bool,
    stat: bool,
) -> Result<ExitCode> {
    let files: Vec<&Value> = reply.json["files"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    // 見出しの行（`FILE`）ごとにファイルを分ける。
    let mut parts: Vec<DiffRows> = Vec::new();
    for row in DiffRows::from(reply).rows {
        if row.0 == DiffRow::FILE || parts.is_empty() {
            parts.push(DiffRows { rows: Vec::new() });
        }
        parts.last_mut().unwrap().rows.push(row);
    }
    let chosen: Vec<(&Value, DiffRows)> = files
        .into_iter()
        .zip(parts)
        .filter(|(f, _)| wanted(f["path"].as_str().unwrap_or(""), paths))
        .collect();
    if c.json {
        print_json(&Value::Array(
            chosen
                .iter()
                .map(|(f, d)| {
                    json!({
                        "path": f["path"],
                        "oldPath": f["oldPath"],
                        "binary": f["binary"],
                        "added": f["added"],
                        "removed": f["removed"],
                        "rows": d.json(false),
                    })
                })
                .collect(),
        ));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    for (f, d) in &chosen {
        let path = f["path"].as_str().unwrap_or("");
        if stat {
            stat_line(&mut out, path, f);
        } else {
            d.render(&mut out, None, numbers, false, c.color);
        }
    }
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}

/// 引数のうち、コミットとして読めるものと、パス（`--` の後ろはいつもパス）。
fn split_revs(front: &dyn Front, rest: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut revs, mut paths) = (Vec::new(), Vec::new());
    let mut after_dashes = false;
    for arg in rest {
        if arg == "--" {
            after_dashes = true;
        } else if !after_dashes && paths.is_empty() && resolve(front, arg).is_ok() {
            revs.push(arg.clone());
        } else {
            paths.push(arg.clone());
        }
    }
    (revs, paths)
}

fn wanted(path: &str, paths: &[String]) -> bool {
    paths.is_empty()
        || paths
            .iter()
            .any(|p| path == p || path.starts_with(&format!("{p}/")))
}

/// `rev`（ブランチ・タグ・短い ID・`HEAD~2` など）をコミットの ID に。
pub fn resolve(front: &dyn Front, rev: &str) -> Result<String> {
    let r = command(front, json!({"op": "resolveCommit", "rev": rev}))?;
    r["text"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| tr!("Commit not found: {rev}", rev = rev).into())
}

/// 改行を変換している・改行が混ざっているファイルの説明（`workingDiff` の `lineEndings`。普段のファイルでは None）。
fn line_endings_note(le: &Value) -> Option<String> {
    if le["notable"] != true {
        return None;
    }
    let name = |key: &str| match le[key].as_str() {
        Some("lf") => "LF".to_owned(),
        Some("crlf") => "CRLF".to_owned(),
        Some("mixed") => tr!("mixed"),
        _ => tr!("none"),
    };
    let mut note = tr!(
        "line endings: working tree {worktree}, git {index}, checkout {checkout}",
        worktree = name("worktree"),
        index = name("index"),
        checkout = name("checkout")
    );
    if let Some(rule) = le["rule"].as_str().filter(|r| !r.is_empty()) {
        note.push_str(&format!(" ({rule})"));
    }
    Some(note)
}

fn stat_line(out: &mut String, path: &str, file: &Value) {
    if file["binary"] == true {
        out.push_str(&format!("{path}  {}\n", tr!("binary")));
    } else {
        out.push_str(&format!(
            "{path}  +{} -{}\n",
            file["added"], file["removed"]
        ));
    }
}

// MARK: - stage・unstage・discard

/// `rows`（`3,5-9`）を行の番号に。
fn parse_rows(spec: &str) -> Result<Vec<u32>> {
    let mut rows = Vec::new();
    for part in spec.split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => rows.extend(a.trim().parse::<u32>()?..=b.trim().parse::<u32>()?),
            None => rows.push(part.trim().parse()?),
        }
    }
    Ok(rows)
}

/// `stage`（`unstage` なら false）: パスごと、または `--rows` で差分の行ごと（`--combined` の差分の行番号。
/// `amonhen diff --combined --rows` で見る）。
pub fn stage(args: &[String], on: bool) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--rows", "--all", "-A"], &["--rows"])?;
    let front = open_repository(&c.dir)?;
    if let Some(spec) = value(&seen, "--rows") {
        let [path] = c.rest.as_slice() else {
            return Err(tr!("--rows takes exactly one file").into());
        };
        let d = call(
            &*front,
            json!({"op": "workingDiff", "path": path, "mode": "combined"}),
        )
        .map_err(|e| tr!("Can’t stage {path} by line: {e}", path = path, e = e))?;
        call(
            &*front,
            json!({"op": "stageRows", "diff": d.json["diff"], "rows": parse_rows(spec)?, "stage": on}),
        )?;
        if !c.json {
            let done = if on {
                tr!("Staged lines {rows} of {path}", rows = spec, path = path)
            } else {
                tr!("Unstaged lines {rows} of {path}", rows = spec, path = path)
            };
            println!("{done}");
        }
        return done(&c);
    }
    let paths: Vec<String> = if flag(&seen, "--all") || flag(&seen, "-A") {
        let status = call(&*front, json!({"op": "status"}))?.json;
        status["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| {
                if on {
                    e["unstaged"] == true
                } else {
                    e["staged"] == true
                }
            })
            .filter_map(|e| e["path"].as_str().map(str::to_owned))
            .collect()
    } else {
        c.rest.clone()
    };
    if paths.is_empty() {
        return Err(tr!("Give the files (--all for all of them)").into());
    }
    call(
        &*front,
        json!({"op": if on { "stage" } else { "unstage" }, "paths": paths}),
    )?;
    if !c.json {
        let done = match (on, paths.len()) {
            (true, 1) => tr!("Staged 1 file"),
            (true, n) => tr!("Staged {count} files", count = n),
            (false, 1) => tr!("Unstaged 1 file"),
            (false, n) => tr!("Unstaged {count} files", count = n),
        };
        println!("{done}");
    }
    done(&c)
}

/// `discard`: 作業ツリーの変更を捨てる（Undo できる）。パスごと、または `--rows` で行ごと（stage していない差分の
/// 行番号。`amonhen diff --rows` で見る）。
pub fn discard(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--rows"], &["--rows"])?;
    let front = open_repository(&c.dir)?;
    if c.rest.is_empty() {
        return Err(tr!("Give the files").into());
    }
    if let Some(spec) = value(&seen, "--rows") {
        let [path] = c.rest.as_slice() else {
            return Err(tr!("--rows takes exactly one file").into());
        };
        let d = call(
            &*front,
            json!({"op": "workingDiff", "path": path, "mode": "unstaged"}),
        )?;
        call(
            &*front,
            json!({"op": "discardRows", "diff": d.json["diff"], "rows": parse_rows(spec)?}),
        )?;
        if !c.json {
            println!(
                "{}",
                tr!(
                    "Discarded the changes on lines {rows} of {path} (amonhen undo brings them back)",
                    rows = spec,
                    path = path
                )
            );
        }
        return done(&c);
    }
    command(&*front, json!({"op": "discard", "paths": c.rest}))?;
    if !c.json {
        let done = match c.rest.len() {
            1 => tr!("Discarded the changes in 1 file (amonhen undo brings them back)"),
            n => tr!(
                "Discarded the changes in {count} files (amonhen undo brings them back)",
                count = n
            ),
        };
        println!("{done}");
    }
    done(&c)
}

/// `commit -m <message> [--amend]`（Undo できる）。
pub fn commit(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["-m", "--message", "--amend"], &["-m", "--message"])?;
    let front = open_repository(&c.dir)?;
    let message = value(&seen, "-m")
        .or_else(|| value(&seen, "--message"))
        .ok_or_else(|| tr!("Give the message with -m"))?;
    call(
        &*front,
        json!({"op": "commit", "message": message, "amend": flag(&seen, "--amend")}),
    )?;
    let head = resolve(&*front, "HEAD")?;
    if c.json {
        print_json(&json!({"ok": true, "commit": head}));
    } else {
        println!(
            "{}",
            tr!(
                "Committed: {id} {subject}",
                id = &head[..8],
                subject = message.lines().next().unwrap_or("")
            )
        );
    }
    Ok(ExitCode::SUCCESS)
}

// MARK: - rewrite-preview

/// 書き換える前の見通し: 今のブランチを `<onto>` へ rebase したら（`--edit` なら History の編集をしたら）、一緒に
/// 動くブランチと残るブランチ、push 済みのもの、コンフリクトしそうなコミット。何にも触らない。終了コードは
/// コンフリクトの見込みがなければ 0、あれば 1、git が古くてわからなければ 2。
pub fn rewrite_preview(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(
        args,
        &["--edit", "--update-refs", "--no-update-refs"],
        &["--edit"],
    )?;
    let mut request = json!({"op": "rewritePreview"});
    match (value(&seen, "--edit"), c.rest.as_slice()) {
        (Some(edit), []) => {
            request["edit"] =
                serde_json::from_str(edit).map_err(|e| tr!("Can’t read the edit: {e}", e = e))?;
        }
        (None, [onto]) => request["onto"] = json!(onto),
        _ => {
            return Err(format!(
                "{}

{}",
                tr!("Give the commit to rebase onto, or --edit"),
                usage()
            )
            .into());
        }
    }
    if flag(&seen, "--update-refs") {
        request["updateRefs"] = json!(true);
    } else if flag(&seen, "--no-update-refs") {
        request["updateRefs"] = json!(false);
    }
    let front = open_repository(&c.dir)?;
    let response = command(&*front, request)?;
    let info = &response["info"];
    let conflicts = &info["conflicts"];
    if c.json {
        print_json(info);
    } else {
        let names = |list: &serde_json::Value| -> Vec<String> {
            list.as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["name"].as_str().map(str::to_owned))
                .collect()
        };
        let mut out = String::new();
        let moving = names(&info["moving"]);
        if !moving.is_empty() {
            out.push_str(&tr!("Moves with it: {names}", names = moving.join(", ")));
            out.push('\n');
        }
        for branch in info["staying"].as_array().into_iter().flatten() {
            let name = branch["name"].as_str().unwrap_or("");
            let why = match branch["reason"].as_str() {
                Some("checkedOut") => tr!("checked out in another worktree"),
                Some("git") => tr!("moving it needs git 2.38"),
                Some("off") => tr!("not this time (--no-update-refs)"),
                _ => tr!("rebase.updateRefs is off"),
            };
            out.push_str(&tr!("Stays: {name} ({why})", name = name, why = why));
            out.push('\n');
        }
        let mut pushed: Vec<String> = info["currentUpstream"]
            .as_str()
            .and(info["current"].as_str())
            .map(str::to_owned)
            .into_iter()
            .collect();
        pushed.extend(
            info["moving"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| !b["upstream"].is_null())
                .filter_map(|b| b["name"].as_str().map(str::to_owned)),
        );
        if !pushed.is_empty() {
            out.push_str(&tr!(
                "Pushed already (pushing afterwards needs a force push): {names}",
                names = pushed.join(", ")
            ));
            out.push('\n');
        }
        match conflicts.as_array() {
            None => out.push_str(&tr!("Conflicts: can’t tell without git 2.40 or newer")),
            Some(list) if list.is_empty() => out.push_str(&tr!("No conflicts expected")),
            Some(list) => {
                out.push_str(&tr!(
                    "Conflicts expected at {count} commits:",
                    count = list.len()
                ));
                for step in list {
                    let oid = step["oid"].as_str().unwrap_or("");
                    let files: Vec<&str> = step["files"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|f| f.as_str())
                        .collect();
                    out.push_str(&format!(
                        "\n  {}  {}",
                        &oid[..oid.len().min(8)],
                        files.join(", ")
                    ));
                }
            }
        }
        out.push('\n');
        term::write(&out);
    }
    Ok(match conflicts.as_array() {
        None => ExitCode::from(2),
        Some(list) if list.is_empty() => ExitCode::SUCCESS,
        Some(_) => ExitCode::from(1),
    })
}

// MARK: - merge-check

/// `rev` を HEAD（`--into` ならそのブランチ）にマージしたらコンフリクトするファイル。いくつも渡せば、1 つの
/// コミットにまとめる merge（octopus）を、その順に重ねて調べる。終了コードは、なければ 0、あれば 1、git が古くて
/// わからなければ 2。
pub fn merge_check(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--into"], &["--into"])?;
    if c.rest.is_empty() {
        return Err(format!("{}\n\n{}", tr!("Give the commit to merge"), usage()).into());
    }
    let revs = &c.rest;
    let rev = revs.join(", ");
    let ours = value(&seen, "--into").unwrap_or("HEAD");
    let front = open_repository(&c.dir)?;
    let theirs = if let [one] = revs.as_slice() {
        json!(one)
    } else {
        json!(revs)
    };
    let response = command(
        &*front,
        json!({"op": "predictMerge", "ours": ours, "theirs": theirs}),
    )?;
    let conflicts = &response["info"]["conflicts"];
    if c.json {
        print_json(&response["info"]);
    } else if conflicts.is_null() {
        println!("{}", tr!("Can’t tell without git 2.38 or newer"));
    } else if let Some(paths) = conflicts.as_array().filter(|p| !p.is_empty()) {
        let mut out = tr!(
            "Merging {rev} into {ours} would conflict in {count} files:",
            rev = rev,
            ours = ours,
            count = paths.len()
        );
        out.push('\n');
        let steps = response["info"]["steps"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if revs.len() == 1 {
            for path in paths {
                out.push_str(&format!("  {}\n", path.as_str().unwrap_or("")));
            }
        } else {
            // どれを取り込むところでぶつかるか。最後のもの以外でぶつかると、git は 1 つのコミットにまとめられない。
            for step in &steps {
                let Some(files) = step["conflicts"].as_array().filter(|f| !f.is_empty()) else {
                    continue;
                };
                out.push_str(&format!("  {}:\n", step["rev"].as_str().unwrap_or("")));
                for path in files {
                    out.push_str(&format!("    {}\n", path.as_str().unwrap_or("")));
                }
            }
            let blocked = steps.split_last().is_some_and(|(_, init)| {
                init.iter()
                    .any(|s| s["conflicts"].as_array().is_some_and(|f| !f.is_empty()))
            });
            if blocked {
                out.push_str(&tr!(
                    "git stops before the last one when a conflict comes earlier: put the one that conflicts last, or merge them one at a time."
                ));
                out.push('\n');
            }
        }
        term::write(&out);
    } else {
        println!(
            "{}",
            tr!(
                "{rev} merges into {ours} without conflicts",
                rev = rev,
                ours = ours
            )
        );
    }
    Ok(match conflicts.as_array() {
        None => ExitCode::from(2),
        Some(paths) if paths.is_empty() => ExitCode::SUCCESS,
        Some(_) => ExitCode::from(1),
    })
}

// MARK: - range-diff

/// 同じ一連のコミットの 2 つの版を比べる（`git range-diff` と同じ範囲の書き方）。`--previous` ならブランチ
/// （既定は今のブランチ）の、最後に書き換える前の版と今の版を比べる。
pub fn range_diff(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(
        args,
        &["--previous", "--creation-factor"],
        &["--creation-factor"],
    )?;
    let factor = match value(&seen, "--creation-factor") {
        Some(v) => Some(
            v.parse::<u32>()
                .map_err(|_| tr!("--creation-factor takes a percentage: {value}", value = v))?,
        ),
        None => None,
    };
    let front = open_repository(&c.dir)?;
    let mut previous = Value::Null;
    let ranges: Vec<String> = if flag(&seen, "--previous") {
        let branch = match c.rest.as_slice() {
            [] => None,
            [branch] => Some(branch.clone()),
            _ => return Err(format!("{}\n\n{}", tr!("Give one branch"), usage()).into()),
        };
        previous = command(&*front, json!({"op": "previousVersion", "branch": branch}))?["info"]
            ["previous"]
            .clone();
        if previous.is_null() {
            if c.json {
                print_json(&json!({"previous": null, "pairs": []}));
            } else {
                println!(
                    "{}",
                    tr!("The branch hasn’t been rewritten (nothing in its reflog)")
                );
            }
            return Ok(ExitCode::SUCCESS);
        }
        previous["ranges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_str().map(str::to_owned))
            .collect()
    } else {
        if !(1..=3).contains(&c.rest.len()) {
            return Err(format!("{}\n\n{}", tr!("Give the two ranges to compare"), usage()).into());
        }
        c.rest.clone()
    };
    let info = command(
        &*front,
        json!({"op": "rangeDiff", "ranges": ranges, "creationFactor": factor}),
    )?["info"]
        .clone();
    if c.json {
        print_json(&json!({"previous": previous, "pairs": info["pairs"]}));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    if !previous.is_null() {
        let text = |key: &str| previous[key].as_str().unwrap_or("").to_owned();
        out.push_str(&paint(
            &tr!(
                "{old} → {branch}: {message}",
                old = text("selector"),
                branch = text("branch"),
                message = text("message")
            ),
            "1",
            c.color,
        ));
        out.push_str(&format!("\n  git range-diff {}\n\n", ranges.join(" ")));
    }
    out.push_str(&range_diff_text(&info["pairs"], c.color));
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}

/// `git range-diff` に近い形の文字列（ID は短く。色は git の dual-color に近く、外側の記号で行の色を決め、
/// 内側の記号は薄く色を付ける）。
fn range_diff_text(pairs: &Value, color: bool) -> String {
    let pairs = pairs.as_array().map(Vec::as_slice).unwrap_or_default();
    let width = pairs
        .iter()
        .flat_map(|p| [&p["old"]["index"], &p["new"]["index"]])
        .filter_map(Value::as_u64)
        .max()
        .unwrap_or(1)
        .to_string()
        .len();
    let side = |c: &Value| match (c["index"].as_u64(), c["oid"].as_str()) {
        (Some(i), Some(oid)) => format!("{i:>width$}:  {}", &oid[..oid.len().min(7)]),
        _ => format!("{:>width$}:  -------", "-"),
    };
    let mut out = String::new();
    for p in pairs {
        let (mark, code) = match p["status"].as_str().unwrap_or("") {
            "same" => ("=", "2"),
            "changed" => ("!", "33"),
            "removed" => ("<", "31"),
            _ => (">", "32"),
        };
        let header = format!(
            "{} {mark} {} {}",
            side(&p["old"]),
            side(&p["new"]),
            p["subject"].as_str().unwrap_or("")
        );
        out.push_str(&paint(&header, code, color));
        out.push('\n');
        for line in p["diff"].as_array().into_iter().flatten() {
            let line = line.as_str().unwrap_or("");
            let code = match line.as_bytes().first() {
                Some(b'-') => "31",
                Some(b'+') => "32",
                Some(b'@') => "36",
                _ => match line.as_bytes().get(1) {
                    Some(b'-') => "2;31",
                    Some(b'+') => "2;32",
                    _ => "",
                },
            };
            out.push_str("    ");
            out.push_str(&if code.is_empty() {
                line.to_owned()
            } else {
                paint(line, code, color)
            });
            out.push('\n');
        }
    }
    out
}

fn done(c: &Common) -> Result<ExitCode> {
    if c.json {
        print_json(&json!({"ok": true}));
    }
    Ok(ExitCode::SUCCESS)
}

// MARK: - refs

/// 署名の行: 短い言葉と、方式と署名者（`Verified (GPG · Alice <a@example.com>)`）。
fn signature_text(signature: &Value, kind: &str) -> String {
    let signer = signature["signer"].as_str().unwrap_or("");
    let mut text = format!("{} ({}", signature_label(signature), kind.to_uppercase());
    if !signer.is_empty() {
        text.push_str(&format!(" · {signer}"));
    }
    text.push(')');
    text
}

/// タグの行の続き: 件名と、署名があればその結果。
fn tag_text(subject: Option<&str>, kind: Option<&str>, signature: &Value) -> String {
    let mut text = String::new();
    if let Some(subject) = subject.filter(|s| !s.is_empty()) {
        text.push_str(&format!(" — {subject}"));
    }
    if let Some(kind) = kind {
        let result = if signature.is_null() {
            format!("No key ({})", kind.to_uppercase())
        } else {
            signature_text(signature, kind)
        };
        text.push_str(&format!("  {result}"));
    }
    text
}

/// 署名の検証の結果の短い言葉（アプリと同じく GitHub に合わせた英語）。署名者とタガーの一致は見ない。
fn signature_label(signature: &Value) -> &'static str {
    match signature["status"].as_str().unwrap_or("") {
        "G" if signature["trust"] != "undefined" && signature["trust"] != "never" => "Verified",
        "G" | "U" => "Untrusted key",
        "X" | "Y" => "Expired",
        "R" => "Revoked key",
        "B" => "Bad signature",
        "V" => "Can’t verify",
        _ => "No key",
    }
}

pub fn refs(args: &[String]) -> Result<ExitCode> {
    let (c, seen) = common(args, &["--verify"], &[])?;
    let front = open_repository(&c.dir)?;
    let mut refs = call(&*front, json!({"op": "refs"}))?.json;
    // --verify: タグの署名を確かめ、タグの行に添える（タグごとに git と gpg を起動するので、多いと数秒かかる）。
    if flag(&seen, "--verify") {
        let names: Vec<String> = refs["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["kind"] == "tag")
            .filter_map(|e| e["name"].as_str().map(str::to_owned))
            .collect();
        let tags =
            command(&*front, json!({"op": "tagSignatures", "names": names}))?["info"]["tags"]
                .clone();
        let by_name: std::collections::HashMap<&str, &Value> = tags
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t["name"].as_str()?, t)))
            .collect();
        for e in refs["entries"].as_array_mut().into_iter().flatten() {
            if e["kind"] != "tag" {
                continue;
            }
            let tag = e["name"].as_str().and_then(|n| by_name.get(n));
            e["signed"] = tag.map_or(Value::Null, |t| t["signed"].clone());
            e["signature"] = tag.map_or(Value::Null, |t| t["signature"].clone());
        }
    }
    if c.json {
        print_json(&refs);
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    let mut last = String::new();
    let head = refs["headBranch"].as_str();
    for e in refs["entries"].as_array().into_iter().flatten() {
        let kind = e["kind"].as_str().unwrap_or("");
        if kind != last {
            out.push_str(&match kind {
                "branch" => tr!("Branches"),
                "remote" => tr!("Remotes"),
                "tag" => tr!("Tags"),
                "stash" => "stash".to_owned(),
                _ => tr!("Submodules"),
            });
            out.push('\n');
            last = kind.to_owned();
        }
        let name = e["name"].as_str().unwrap_or("");
        let is_head = kind == "branch" && head == Some(name);
        let target = e["target"]
            .as_str()
            .map_or_else(|| "        ".to_owned(), |t| t[..8].to_owned());
        let mut line = format!("  {} {target} {name}", if is_head { "*" } else { " " });
        if let Some(d) = e["detail"].as_str() {
            line.push_str(&format!("  {d}"));
        }
        let (ahead, behind) = (
            e["ahead"].as_u64().unwrap_or(0),
            e["behind"].as_u64().unwrap_or(0),
        );
        if let Some(signature) = e.get("signature").filter(|s| !s.is_null()) {
            let kind = e["signed"].as_str().unwrap_or("gpg");
            line.push_str(&format!("  {}", signature_text(signature, kind)));
        }
        if e["gone"] == true {
            line.push_str(&tr!(" (upstream gone)"));
        } else if ahead > 0 || behind > 0 {
            line.push_str(&tr!(
                " ({ahead} ahead, {behind} behind)",
                ahead = ahead,
                behind = behind
            ));
        }
        out.push_str(&line);
        out.push('\n');
    }
    term::write(&out);
    Ok(ExitCode::SUCCESS)
}
