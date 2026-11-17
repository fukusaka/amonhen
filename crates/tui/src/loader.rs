//! 窓口を呼ぶワーカー。`Front::call` は答えを待つので、メインスレッドからは呼ばない。
//!
//! ワーカーは 2 つ。History の行と、コミットの詳細。大きなコミットの差分を待つ間も、行は読める。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};

use amonhen_front::{Front, Reply};
use amonhen_model::tr;
use serde_json::{Value, json};

use crate::Result;
use crate::app::Event;

pub enum Request {
    /// リポジトリの名前・ref・History を読む。
    Open,
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
    /// コミットの詳細（作者・メッセージなど）と差分。`id` は 16 進の ID。`stash` なら、stash としての差分
    /// （作ったときの HEAD から、作業中の変更まで）。
    Commit { id: String, stash: bool },
    /// 差分の構文の色。`of` は `{"view": …}`（`commitDiff` の答えのもの）か `{"diff": …}`（`workingDiff` の
    /// もの）。`id` は、答えを見分ける名前。
    Syntax { id: String, of: Value },
    /// 作業ツリーの状態（変わったファイルの一覧）。
    Status,
    /// HEAD の reflog。
    Reflog,
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
    /// コアの命令（fetch・push・checkout など）を動かす。`name` は、答えを見分ける名前。
    Run { name: &'static str, command: Value },
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
        start: u32,
        reply: Reply,
    },
    /// 検索に合った行（上から順）。だめなら理由。
    Search {
        query: String,
        result: std::result::Result<Vec<u32>, String>,
    },
    /// `row` 行目のコミットの祖先（行ごとに 1 ビット）。取れなければ `None`。
    Lineage {
        row: u32,
        bits: Option<Vec<u64>>,
    },
    /// コミットの行（History になければ `None`）。
    RowOf(Option<u32>),
    Commit {
        id: String,
        /// `commitInfo` の `info` と、`commitDiff` の答え。だめなら理由。
        result: std::result::Result<(Value, Reply), String>,
    },
    /// 構文の色。取れなければ `None`（色なしで出すだけ）。
    Syntax {
        id: String,
        reply: Option<Reply>,
    },
    /// `blame` か `fileHistory` の答え。だめなら理由。
    File {
        path: String,
        blame: bool,
        result: std::result::Result<Value, String>,
    },
    /// `reflog` の答え。だめなら理由。
    Reflog(std::result::Result<Value, String>),
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
            Request::Open
            | Request::Rows { .. }
            | Request::RowOf { .. }
            | Request::Lineage { .. } => &self.rows,
            Request::Search { .. } => &self.search,
            Request::Run { .. } => &self.operations,
            Request::Commit { .. } | Request::Syntax { .. } => &self.details,
            Request::Status
            | Request::Reflog
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
                let event = match load(&*front, request) {
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

fn load(front: &dyn Front, request: Request) -> Result<Loaded> {
    Ok(match request {
        Request::Open => Loaded::Opened {
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
            history: front.call(json!({"op": "history"}))?.json,
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
        },
        Request::Search {
            history,
            query,
            cancelled,
        } => {
            let result = front
                .call_until(
                    json!({"op": "historySearch", "history": history, "query": query}),
                    &|| cancelled.load(Ordering::Relaxed),
                )
                .map(|r| r.u32s("rows"))
                .map_err(|e| e.to_string());
            Loaded::Search { query, result }
        }
        Request::Lineage { history, row } => Loaded::Lineage {
            row,
            bits: front
                .call(json!({"op": "historyLineage", "history": history, "row": row}))
                .ok()
                .map(|r| r.items::<u64>("rows")),
        },
        Request::RowOf { history, id } => Loaded::RowOf(
            front
                .call(json!({"op": "historyRowOf", "history": history, "id": id}))
                .ok()
                .and_then(|r| r.json["row"].as_u64())
                .map(|row| row as u32),
        ),
        Request::Commit { id, stash } => {
            let result = commit(front, &id, stash).map_err(|e| e.to_string());
            Loaded::Commit { id, result }
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
        Request::Reflog => {
            Loaded::Reflog(run(front, json!({"op": "reflog"})).map_err(|e| e.to_string()))
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
        } => {
            let diff =
                |mode: &str| front.call(json!({"op": "workingDiff", "path": path, "mode": mode}));
            // コンフリクトのあるファイルは、マーカーつきの中身（出せなければ、ふつうの差分）。
            // 行ごとの stage の印つきが出せないファイル（バイナリなど）は、片方だけを出す。
            let result = match conflict {
                true => front.call(json!({"op": "conflict", "path": path})),
                false => Err("".into()),
            }
            .or_else(|_| diff("combined"))
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
        Request::Run { name, command } => Loaded::Ran {
            name,
            result: run(front, command).map_err(|e| e.to_string()),
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

fn commit(front: &dyn Front, id: &str, stash: bool) -> Result<(Value, Reply)> {
    let mut info = run(front, json!({"op": "commitInfo", "oid": id}))?;
    let of = if stash { "stash" } else { "id" };
    let diff = front.call(json!({"op": "commitDiff", of: id}))?;
    Ok((info["info"].take(), diff))
}
