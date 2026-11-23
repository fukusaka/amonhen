//! 窓口を呼ぶワーカー。`Front::call` は答えを待つので、メインスレッドからは呼ばない。
//!
//! ワーカーは 2 つ。History の行と、コミットの詳細。大きなコミットの差分を待つ間も、行は読める。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use amonhen_front::{Front, Reply};
use amonhen_model::tr;
use serde_json::{Value, json};

use crate::Result;
use crate::app::Event;

pub enum Request {
    /// リポジトリの名前・ref・History を読む。`fold` ならマージ済みのブランチを折りたたむ（`expanded` のマージは
    /// 取り込んだ側も出す）。
    /// `rebuild` なら、History のキャッシュを捨てて作り直してから（Rebuild History）。
    Open {
        fold: bool,
        expanded: Vec<String>,
        rebuild: bool,
        /// History を、これらのコミット（選んだ ref の指す先、16 進）から辿れるものだけにする。空ならすべて。
        tips: Vec<String>,
    },
    /// History の行（`start` から `count` 行）。
    Rows {
        history: Value,
        start: u32,
        count: u32,
    },
    /// History を検索する（書き方は `crates/core/src/query.rs`）。`cancelled` が true になったら取り消す。
    Search {
        history: Value,
        query: String,
        cancelled: Arc<AtomicBool>,
    },
    /// `row` 行目のコミットの祖先。
    Lineage { history: Value, row: u32 },
    /// コミット（16 進の ID）が History の何行目か。
    RowOf { history: Value, id: String },
    /// 印をつけていたコミット（16 進の ID）が、読み直した History の何行目か。
    MarksOf { history: Value, ids: Vec<String> },
    /// コミットの詳細（作者・メッセージなど）と差分。`id` は 16 進の ID。`stash` なら、stash としての差分
    /// （作ったときの HEAD から、作業中の変更まで）。
    /// `key` は答えを見分ける鍵、`whitespace` は無視する空白（`commitDiff` の whitespace）。
    Commit {
        id: String,
        stash: bool,
        key: String,
        whitespace: &'static str,
        /// ファイルごとの前後の行数（`commitDiff` の contexts）。
        contexts: Value,
    },
    /// History で印をつけたコミット（行）の差分（`commitDiff` の rows・pair）。答えは `Loaded::Commit` で、`id` は
    /// `key`（詳細のものは null）。
    Selection {
        key: String,
        history: Value,
        rows: Vec<u32>,
        pair: &'static str,
        whitespace: &'static str,
        contexts: Value,
    },
    /// 差分の構文の色。`of` は `{"view": …}`（`commitDiff` の答えのもの）か `{"diff": …}`（`workingDiff` の
    /// もの）。`id` は、答えを見分ける名前。
    Syntax { id: String, of: Value },
    /// 作業ツリーの状態（変わったファイルの一覧）。
    Status,
    /// `name` の ref の reflog（`None` は HEAD）。
    Reflog { name: Option<String> },
    /// ファイルの Blame（`blame`。`rev` があれば、そのコミットの時点の）か、そのファイルを変えたコミット。
    File {
        path: String,
        blame: bool,
        rev: Option<String>,
    },
    /// 作業ツリーのファイルの差分。`serial` は、どの回の `status` に対するものか。`unstaged` は、stage して
    /// いない変更があるか（行ごとの印つきの差分が出せないときに、どちらを出すか）。
    WorkingDiff {
        path: String,
        serial: u64,
        unstaged: bool,
        /// コンフリクトのあるファイル（マーカーつきの中身を出す）。
        conflict: bool,
        /// 無視する空白（`workingDiff` の whitespace）。無視するなら、行ごとの印つきの差分は出せない。
        whitespace: &'static str,
    },
    /// ファイルを stage する（`stage` が false なら unstage）。
    Stage { paths: Vec<String>, stage: bool },
    /// 差分（`workingDiff` の答えの番号）の行を stage する（`stage` が false なら unstage）。
    StageRows {
        diff: Value,
        rows: Vec<u32>,
        stage: bool,
    },
    /// 差分の行の変更を、作業ツリーから取り除く。
    DiscardRows { diff: Value, rows: Vec<u32> },
    /// ファイルの変更を捨てる。
    Discard { paths: Vec<String> },
    /// コアの命令（fetch・push・checkout など）を動かす。`name` は、答えを見分ける名前。`command` が配列なら、
    /// その命令を順に（失敗したら、そこでやめる）。
    Run { name: &'static str, command: Value },
    /// 窓口の命令（`run` を通さないもの。History の並び順など）を、操作として動かす。答えは `Ran` で返す。
    Call { name: &'static str, request: Value },
    /// History の編集モードの行: `from`（16 進）から HEAD までの、編集できるコミットとそのメッセージ（新しい順。
    /// merge を越えて first-parent で辿る）。答えは `editRows` の `rows`（oid・merge・message）。
    EditRows { from: String },
    /// 書きかけのメッセージ（`commitDraft`）。なければ `commit.template` の中身（`#` の行を除く）。リポジトリ専用の
    /// テンプレートはコアから読み（SSH 先でも）、ほかのファイルは `local`（この機械のリポジトリ）のときだけ読む。
    DraftOrTemplate { local: bool },
    /// 作業ツリーの直下の .gitignore の終わりに `pattern` を足す（もうあれば足さない。アプリの Add to .gitignore）。
    AddToGitignore { pattern: String },
    /// 今のブランチの最近のコミット（first-parent で 30 まで）のメッセージ（`recentMessages`）。
    RecentMessages,
    /// コミットを作る（`amend` なら、直前のコミットを作り直す）。
    MakeCommit { message: String, amend: bool },
    /// コミット（16 進の ID）のメッセージ（amend で欄に入れる）。
    Message { id: String },
}

pub enum Loaded {
    Opened {
        workdir: Option<String>,
        refs: Value,
        history: Value,
    },
    Rows {
        /// どの History のものか（読み直した後に届いた、前の History の行は捨てる）。
        history: Value,
        start: u32,
        reply: Reply,
    },
    /// 検索に合った行（上から順）。だめなら理由。
    Search {
        query: String,
        result: std::result::Result<Vec<u32>, String>,
    },
    /// `row` 行目のコミットの祖先（行ごとに 1 ビット）と、線ごとの光らせ始める行（History のすべての線の並び。
    /// 光らせない線は `u32::MAX`）。取れなければ `None`。
    Lineage {
        row: u32,
        bits: Option<(Vec<u64>, Vec<u32>)>,
    },
    /// コミットの行（History になければ `None`）。
    RowOf(Option<u32>),
    /// 印をつけていたコミットの行（`MarksOf` の順。なければ `None`）。
    MarksOf(Vec<Option<u32>>),
    Commit {
        id: String,
        /// `commitInfo` の `info` と、`commitDiff` の答え。だめなら理由。
        result: std::result::Result<(Value, Reply), String>,
    },
    /// 構文の色。取れなければ `None`（色なしで出すだけ）。
    Syntax { id: String, reply: Option<Reply> },
    /// `blame` か `fileHistory` の答え。だめなら理由。
    File {
        path: String,
        blame: bool,
        result: std::result::Result<Value, String>,
    },
    /// `reflog` の答えと、どの ref のものか（`None` は HEAD）。だめなら理由。
    Reflog(Option<String>, std::result::Result<Value, String>),
    /// `status` の答え。だめなら理由。
    Status(std::result::Result<Value, String>),
    WorkingDiff {
        path: String,
        serial: u64,
        result: std::result::Result<Reply, String>,
    },
    /// コアの命令が済んだ（その答え）。だめなら理由。
    Ran {
        name: &'static str,
        result: std::result::Result<Value, String>,
    },
    /// コミットを作った（hooks が書いたもの）。だめなら理由。
    Committed(std::result::Result<String, String>),
    /// コミットのメッセージ（読めなければ `None`）。
    Message(Option<String>),
    /// stage・unstage・discard が済んだ。だめなら理由。
    Staged(std::result::Result<(), String>),
}

pub struct Loader {
    rows: Sender<Request>,
    details: Sender<Request>,
    work: Sender<Request>,
    search: Sender<Request>,
    /// fetch・push など、時間のかかる操作。
    operations: Sender<Request>,
}

impl Loader {
    /// 頼まれた順に窓口を呼び、答えを `events` に返すスレッドを始める。
    pub fn start(front: Arc<dyn Front>, events: Sender<Event>) -> Self {
        Self {
            rows: worker("amonhen-tui-rows", front.clone(), events.clone()),
            details: worker("amonhen-tui-details", front.clone(), events.clone()),
            work: worker("amonhen-tui-work", front.clone(), events.clone()),
            search: worker("amonhen-tui-search", front.clone(), events.clone()),
            operations: worker("amonhen-tui-operations", front, events),
        }
    }

    pub fn send(&self, request: Request) {
        let to = match request {
            Request::Open { .. }
            | Request::Rows { .. }
            | Request::RowOf { .. }
            | Request::MarksOf { .. }
            | Request::Lineage { .. } => &self.rows,
            Request::Search { .. } => &self.search,
            Request::Run { .. }
            | Request::Call { .. }
            | Request::EditRows { .. }
            | Request::RecentMessages
            | Request::AddToGitignore { .. }
            | Request::DraftOrTemplate { .. } => &self.operations,
            Request::Commit { .. } | Request::Selection { .. } | Request::Syntax { .. } => {
                &self.details
            }
            Request::Status
            | Request::Reflog { .. }
            | Request::File { .. }
            | Request::WorkingDiff { .. }
            | Request::Stage { .. }
            | Request::StageRows { .. }
            | Request::DiscardRows { .. }
            | Request::Discard { .. }
            | Request::MakeCommit { .. }
            | Request::Message { .. } => &self.work,
        };
        let _ = to.send(request);
    }
}

fn worker(name: &str, front: Arc<dyn Front>, events: Sender<Event>) -> Sender<Request> {
    let (tx, rx) = mpsc::channel::<Request>();
    let _ = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            for request in rx {
                let event = match load(&*front, request, &events) {
                    Ok(loaded) => Event::Loaded(loaded),
                    Err(e) => Event::Failed(e.to_string()),
                };
                if events.send(event).is_err() {
                    break;
                }
            }
        });
    tx
}

/// 動いている検索に、分かった分を聞く間隔。
const PARTIAL_EVERY: Duration = Duration::from_millis(200);

/// `history` の `query` の検索を動かし、終わるまでの間、分かった分（`historySearchPartial`）を `events` に知らせる。
fn search(
    front: &dyn Front,
    history: &Value,
    query: &str,
    cancelled: &AtomicBool,
    events: &Sender<Event>,
) -> Result<Reply> {
    let finished = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let events = events.clone();
        let finished = &finished;
        let asking = scope.spawn(move || {
            // 前に知らせた分（合った行の数と、調べ終えた割合）。変わらなければ知らせない。
            let mut told = (0, 0.0);
            loop {
                std::thread::park_timeout(PARTIAL_EVERY);
                if finished.load(Ordering::Relaxed) || cancelled.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(reply) = front.call(
                    json!({"op": "historySearchPartial", "history": history, "query": query}),
                ) else {
                    return;
                };
                let rows = reply.u32s("rows");
                let done = reply.json["done"].as_f64().unwrap_or(0.0) as f32;
                if (rows.len(), done) == told || finished.load(Ordering::Relaxed) {
                    continue;
                }
                told = (rows.len(), done);
                let partial = Event::Partial {
                    query: query.to_owned(),
                    rows,
                    done,
                };
                if events.send(partial).is_err() {
                    return;
                }
            }
        });
        let result = front.call_until(
            json!({"op": "historySearch", "history": history, "query": query}),
            &|| cancelled.load(Ordering::Relaxed),
        );
        finished.store(true, Ordering::Relaxed);
        asking.thread().unpark();
        result
    })
}

fn load(front: &dyn Front, request: Request, events: &Sender<Event>) -> Result<Loaded> {
    Ok(match request {
        Request::Open {
            fold,
            expanded,
            rebuild,
            tips,
        } => Loaded::Opened {
            workdir: front.call(json!({"op": "hello"}))?.json["workdir"]
                .as_str()
                .map(str::to_owned),
            refs: {
                // worktree の一覧も一緒に（サイドバーに並べる）。
                let mut refs = front.call(json!({"op": "refs"}))?.json;
                refs["remotes"] = run(front, json!({"op": "remotes"}))
                    .map(|mut r| r["info"]["remotes"].take())
                    .unwrap_or(Value::Null);
                refs["worktrees"] = run(front, json!({"op": "worktrees"}))
                    .map(|mut w| w["info"]["worktrees"].take())
                    .unwrap_or(Value::Null);
                refs
            },
            history: {
                if rebuild {
                    front.call(json!({"op": "history", "rebuild": true}))?;
                }
                let mut request = match fold {
                    true => json!({"op": "history", "fold": true, "expanded": expanded}),
                    false => json!({"op": "history"}),
                };
                if !tips.is_empty() {
                    request["tips"] = json!(tips);
                }
                front.call(request)?.json
            },
        },
        Request::Rows {
            history,
            start,
            count,
        } => Loaded::Rows {
            start,
            reply: front.call(json!({
                "op": "historyRows", "history": history, "start": start, "count": count,
            }))?,
            history,
        },
        Request::Search {
            history,
            query,
            cancelled,
        } => {
            let result = search(front, &history, &query, &cancelled, events)
                .map(|r| r.u32s("rows"))
                .map_err(|e| e.to_string());
            Loaded::Search { query, result }
        }
        Request::Lineage { history, row } => Loaded::Lineage {
            row,
            bits: front
                .call(json!({"op": "historyLineage", "history": history, "row": row}))
                .ok()
                .map(|r| (r.items::<u64>("rows"), r.u32s("edgeFrom"))),
        },
        Request::RowOf { history, id } => Loaded::RowOf(
            front
                .call(json!({"op": "historyRowOf", "history": history, "id": id}))
                .ok()
                .and_then(|r| r.json["row"].as_u64())
                .map(|row| row as u32),
        ),
        Request::MarksOf { history, ids } => Loaded::MarksOf(
            ids.iter()
                .map(|id| {
                    front
                        .call(json!({"op": "historyRowOf", "history": history, "id": id}))
                        .ok()
                        .and_then(|r| r.json["row"].as_u64())
                        .map(|row| row as u32)
                })
                .collect(),
        ),
        Request::Selection {
            key,
            history,
            rows,
            pair,
            whitespace,
            contexts,
        } => Loaded::Commit {
            id: key,
            result: front
                .call(json!({"op": "commitDiff", "history": history, "rows": rows, "pair": pair, "whitespace": whitespace, "contexts": contexts}))
                .map(|diff| (Value::Null, diff))
                .map_err(|e| e.to_string()),
        },
        Request::Commit {
            id,
            stash,
            key,
            whitespace,
            contexts,
        } => {
            let result =
                commit(front, &id, stash, whitespace, contexts).map_err(|e| e.to_string());
            Loaded::Commit { id: key, result }
        }
        Request::Syntax { id, mut of } => {
            of["op"] = json!("diffSyntax");
            Loaded::Syntax {
                id,
                reply: front.call(of).ok(),
            }
        }
        Request::File { path, blame, rev } => {
            let command = match blame {
                true => json!({"op": "blame", "path": path, "rev": rev}),
                false => json!({"op": "fileHistory", "path": path, "limit": 500}),
            };
            let result = run(front, command).map_err(|e| e.to_string());
            Loaded::File {
                path,
                blame,
                result,
            }
        }
        Request::Reflog { name } => {
            let reply = run(front, json!({"op": "reflog", "name": name}));
            Loaded::Reflog(name, reply.map_err(|e| e.to_string()))
        }
        Request::Status => Loaded::Status(
            front
                .call(json!({"op": "status"}))
                .map(|r| {
                    // 途中の操作（merge・rebase など）も一緒に聞く。
                    let mut status = r.json;
                    status["sequence"] = run(front, json!({"op": "sequence"}))
                        .map(|mut s| s["info"].take())
                        .unwrap_or(Value::Null);
                    status
                })
                .map_err(|e| e.to_string()),
        ),
        Request::WorkingDiff {
            path,
            serial,
            unstaged,
            conflict,
            whitespace,
        } => {
            let diff = |mode: &str| {
                front.call(
                    json!({"op": "workingDiff", "path": path, "mode": mode, "whitespace": whitespace}),
                )
            };
            let exact = whitespace == "exact";
            // コンフリクトのあるファイルは、マーカーつきの中身（出せなければ、ふつうの差分）。
            // 行ごとの stage の印つきが出せないファイル（バイナリなど）と、空白を無視するときは、片方だけを出す。
            let result = match conflict && exact {
                true => front.call(json!({"op": "conflict", "path": path})),
                false => Err("".into()),
            }
            .or_else(|e| match exact {
                true => diff("combined"),
                false => Err(e),
            })
            .or_else(|_| diff(if unstaged { "unstaged" } else { "staged" }))
            .map_err(|e| e.to_string());
            Loaded::WorkingDiff {
                path,
                serial,
                result,
            }
        }
        Request::Stage { paths, stage } => {
            done(front.call(json!({"op": if stage { "stage" } else { "unstage" }, "paths": paths})))
        }
        Request::StageRows { diff, rows, stage } => {
            done(front.call(json!({"op": "stageRows", "diff": diff, "rows": rows, "stage": stage})))
        }
        Request::DiscardRows { diff, rows } => {
            done(front.call(json!({"op": "discardRows", "diff": diff, "rows": rows})))
        }
        Request::Discard { paths } => done(run(front, json!({"op": "discard", "paths": paths}))),
        Request::Call { name, request } => Loaded::Ran {
            name,
            result: front
                .call(request)
                .map(|r| r.json)
                .map_err(|e| e.to_string()),
        },
        Request::Run { name, command } => Loaded::Ran {
            name,
            result: match command {
                // いくつかの命令を順に（どれかが失敗したら、そこでやめる）。答えは最後のもの。
                Value::Array(commands) => commands
                    .into_iter()
                    .try_fold(Value::Null, |_, c| run(front, c))
                    .map_err(|e| e.to_string()),
                command => run(front, command).map_err(|e| e.to_string()),
            },
        },
        Request::DraftOrTemplate { local } => Loaded::Ran {
            name: "draftOrTemplate",
            result: draft_or_template(front, local).map_err(|e| e.to_string()),
        },
        Request::AddToGitignore { pattern } => Loaded::Ran {
            name: "addToGitignore",
            result: add_to_gitignore(front, &pattern).map_err(|e| e.to_string()),
        },
        Request::RecentMessages => Loaded::Ran {
            name: "recentMessages",
            result: recent_messages(front).map_err(|e| e.to_string()),
        },
        Request::EditRows { from } => Loaded::Ran {
            name: "editRows",
            result: edit_rows(front, &from).map_err(|e| e.to_string()),
        },
        Request::MakeCommit { message, amend } => Loaded::Committed(
            front
                .call(json!({"op": "commit", "message": message, "amend": amend}))
                .map(|r| r.json["output"].as_str().unwrap_or("").to_owned())
                .map_err(|e| e.to_string()),
        ),
        Request::Message { id } => Loaded::Message(
            run(front, json!({"op": "commitInfo", "oid": id}))
                .ok()
                .and_then(|r| r["info"]["message"].as_str().map(str::to_owned)),
        ),
    })
}

fn done<T>(result: Result<T>) -> Loaded {
    Loaded::Staged(result.map(|_| ()).map_err(|e| e.to_string()))
}

/// コアの命令（`run`）。答えの `ok` が true でなければ、その理由で失敗にする。
fn run(front: &dyn Front, command: Value) -> Result<Value> {
    let reply = front.call(json!({"op": "run", "command": command}))?.json;
    if reply["ok"] != true {
        return Err(reply["error"]
            .as_str()
            .map_or_else(|| tr!("Failed"), str::to_owned)
            .into());
    }
    Ok(reply)
}

/// `Request::DraftOrTemplate` の中身: `{"draft": …}` か `{"template": …}`（どちらもなければ空）。
fn draft_or_template(front: &dyn Front, local: bool) -> Result<Value> {
    let draft = run(front, json!({"op": "commitDraft"}))?;
    if let Some(text) = draft["info"]["text"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
    {
        return Ok(json!({"draft": text}));
    }
    let own = run(front, json!({"op": "commitTemplate"}))?;
    let entries = run(front, json!({"op": "configList"}))?;
    let chosen = (entries["info"]["entries"].as_array().into_iter().flatten())
        .rev()
        .find(|e| (e["key"].as_str()).is_some_and(|k| k.eq_ignore_ascii_case("commit.template")))
        .and_then(|e| e["value"].as_str())
        .filter(|v| !v.is_empty());
    let text = match chosen {
        None => None,
        Some(path) if Some(path) == own["info"]["path"].as_str() => {
            own["info"]["text"].as_str().map(str::to_owned)
        }
        Some(path) if local => {
            let path = match path.strip_prefix("~/") {
                Some(rest) => std::env::var_os("HOME")
                    .map(|home| std::path::PathBuf::from(home).join(rest))
                    .unwrap_or_else(|| path.into()),
                None => path.into(),
            };
            std::fs::read_to_string(path).ok()
        }
        Some(_) => None,
    };
    let text = text.map(|t| {
        (t.lines())
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_matches(['\n', '\r'])
            .to_owned()
    });
    Ok(match text.filter(|t| !t.trim().is_empty()) {
        Some(template) => json!({"template": template}),
        None => json!({}),
    })
}

/// `Request::AddToGitignore` の中身。読み書きはコアがするので、SSH 先でも同じ。`added` は足したか。
fn add_to_gitignore(front: &dyn Front, pattern: &str) -> Result<Value> {
    let files = run(front, json!({"op": "ignoreFiles"}))?;
    let files = &files["info"];
    let mut gitignore = files["gitignore"].as_str().unwrap_or("").to_owned();
    if gitignore.lines().any(|l| l.trim() == pattern) {
        return Ok(json!({"added": false}));
    }
    if !gitignore.is_empty() && !gitignore.ends_with('\n') {
        gitignore.push('\n');
    }
    gitignore.push_str(pattern);
    gitignore.push('\n');
    run(
        front,
        json!({"op": "saveIgnoreFiles", "gitignore": gitignore, "exclude": files["exclude"]}),
    )?;
    Ok(json!({"added": true}))
}

/// `Request::RecentMessages` の中身: HEAD から first-parent をたどった 30 のコミットのメッセージと作者（新しい順。
/// `authors` は `[名前, メール]`）、Signed-off-by に書く自分（`user.name`・`user.email`。なければ null）。
fn recent_messages(front: &dyn Front) -> Result<Value> {
    let reply = run(
        front,
        json!({"op": "editableCommits", "throughMerges": true}),
    )?;
    let infos = (reply["text"].as_str().unwrap_or("").lines())
        .filter_map(|l| l.split(' ').next())
        .take(30)
        .map(|oid| Ok(run(front, json!({"op": "commitInfo", "oid": oid}))?["info"].take()))
        .collect::<Result<Vec<_>>>()?;
    let messages: Vec<&Value> = infos.iter().map(|i| &i["message"]).collect();
    let authors: Vec<Value> = infos
        .iter()
        .map(|i| json!([i["author"], i["authorEmail"]]))
        .collect();
    // 読めなくても、ほかは使える（Signed-off-by だけ出さない）。
    let entries = run(front, json!({"op": "configList"}))
        .map(|r| r["info"]["entries"].clone())
        .unwrap_or_default();
    let value = |key: &str| {
        (entries.as_array().into_iter().flatten())
            .rev()
            .find(|e| {
                e["key"]
                    .as_str()
                    .is_some_and(|k| k.eq_ignore_ascii_case(key))
            })
            .and_then(|e| e["value"].as_str())
            .filter(|v| !v.is_empty())
    };
    let me = match (value("user.name"), value("user.email")) {
        (Some(name), Some(email)) => json!([name, email]),
        _ => Value::Null,
    };
    Ok(json!({"messages": messages, "authors": authors, "me": me}))
}

/// `Request::EditRows` の中身。
fn edit_rows(front: &dyn Front, from: &str) -> Result<Value> {
    let reply = run(
        front,
        json!({"op": "editableCommits", "throughMerges": true}),
    )?;
    let line: Vec<(&str, bool)> = reply["text"]
        .as_str()
        .unwrap_or("")
        .lines()
        .filter_map(|l| {
            let mut parts = l.split(' ');
            Some((parts.next()?, parts.next().is_some()))
        })
        .collect();
    let Some(end) = line.iter().position(|(oid, _)| *oid == from) else {
        return Err(tr!("This commit isn’t on the current branch’s first-parent line, so it can’t be edited here").into());
    };
    let rows = line[..=end]
        .iter()
        .map(|(oid, merge)| {
            let info = run(front, json!({"op": "commitInfo", "oid": oid}))?;
            let message = info["info"]["message"].as_str().unwrap_or("").to_owned();
            Ok(json!({"oid": oid, "merge": merge, "message": message}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"rows": rows}))
}

fn commit(
    front: &dyn Front,
    id: &str,
    stash: bool,
    whitespace: &str,
    contexts: Value,
) -> Result<(Value, Reply)> {
    let mut info = run(front, json!({"op": "commitInfo", "oid": id}))?;
    // 署名があれば、検証もする（gpg・ssh を動かす。だめなら検証の結果なしで出す）。
    if info["info"]["signed"].is_string()
        && let Ok(mut checked) = run(front, json!({"op": "verifySignatures", "oids": [id]}))
    {
        info["info"]["signatureCheck"] = checked["info"]["checks"][0].take();
    }
    let of = if stash { "stash" } else { "id" };
    let diff = front.call(
        json!({"op": "commitDiff", of: id, "whitespace": whitespace, "contexts": contexts}),
    )?;
    Ok((info["info"].take(), diff))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use amonhen_front::Records;

    use super::*;

    /// 検索が、分かった分を 2 回聞かれるまで終わらない窓口。
    struct Slow {
        asked: AtomicU32,
    }

    fn rows(rows: &[u32], json: Value) -> Reply {
        let mut r = Records::new();
        r.put_slice("rows", rows);
        r.finish(json)
    }

    impl Front for Slow {
        fn call_until(
            &self,
            request: Value,
            _cancelled: &(dyn Fn() -> bool + Sync),
        ) -> Result<Reply> {
            match request["op"].as_str() {
                Some("historySearch") => {
                    while self.asked.load(Ordering::Relaxed) < 2 {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(rows(&[3, 5, 8], json!({"count": 3})))
                }
                Some("historySearchPartial") => {
                    Ok(match self.asked.fetch_add(1, Ordering::Relaxed) {
                        0 => rows(&[3], json!({"count": 1, "done": 0.25})),
                        _ => rows(&[3, 5], json!({"count": 2, "done": 0.5})),
                    })
                }
                op => Err(format!("unexpected {op:?}").into()),
            }
        }

        fn subscribe(&self, _: Box<dyn Fn(u32) + Send + Sync>) -> Result<()> {
            Ok(())
        }
    }

    /// 検索が動いている間、分かった分が届く。最後の答えは、その後。
    #[test]
    fn a_running_search_reports_what_it_found_so_far() {
        let front = Slow {
            asked: AtomicU32::new(0),
        };
        let (tx, rx) = mpsc::channel();
        let cancelled = AtomicBool::new(false);
        let reply = search(&front, &json!(1), "contents:x", &cancelled, &tx).unwrap();
        assert_eq!(reply.u32s("rows"), [3, 5, 8]);
        let partial: Vec<(String, Vec<u32>, f32)> = rx
            .try_iter()
            .map(|e| match e {
                Event::Partial { query, rows, done } => (query, rows, done),
                _ => panic!("only partial results are expected"),
            })
            .collect();
        assert_eq!(partial[0], ("contents:x".to_owned(), vec![3], 0.25));
        // 2 回目は、検索が終わるのと同時になることがある（終わっていれば知らせない）。
        assert!(partial.len() <= 2);
        if let Some(second) = partial.get(1) {
            assert_eq!(second.1, [3, 5]);
        }
    }

    /// 取り消した検索には、もう聞かない。
    #[test]
    fn a_cancelled_search_is_not_asked_again() {
        let front = Slow {
            asked: AtomicU32::new(2),
        };
        let (tx, rx) = mpsc::channel();
        let cancelled = AtomicBool::new(true);
        search(&front, &json!(1), "x", &cancelled, &tx).unwrap();
        assert_eq!(rx.try_iter().count(), 0);
        assert_eq!(front.asked.load(Ordering::Relaxed), 2);
    }
}
