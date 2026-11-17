//! 窓口を呼ぶワーカー。`Front::call` は答えを待つので、メインスレッドからは呼ばない。
//!
//! ワーカーは 2 つ。History の行と、コミットの詳細。大きなコミットの差分を待つ間も、行は読める。

use std::sync::Arc;
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
    /// コミットの詳細（作者・メッセージなど）と差分。`id` は 16 進の ID。
    Commit { id: String },
    /// 差分の構文の色。`of` は `{"view": …}`（`commitDiff` の答えのもの）か `{"diff": …}`（`workingDiff` の
    /// もの）。`id` は、答えを見分ける名前。
    Syntax { id: String, of: Value },
    /// 作業ツリーの状態（変わったファイルの一覧）。
    Status,
    /// 作業ツリーのファイルの差分。`serial` は、どの回の `status` に対するものか。`unstaged` は、stage して
    /// いない変更があるか（行ごとの印つきの差分が出せないときに、どちらを出すか）。
    WorkingDiff {
        path: String,
        serial: u64,
        unstaged: bool,
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
    /// `status` の答え。だめなら理由。
    Status(std::result::Result<Value, String>),
    WorkingDiff {
        path: String,
        serial: u64,
        result: std::result::Result<Reply, String>,
    },
    /// stage・unstage・discard が済んだ。だめなら理由。
    Staged(std::result::Result<(), String>),
}

pub struct Loader {
    rows: Sender<Request>,
    details: Sender<Request>,
    work: Sender<Request>,
}

impl Loader {
    /// 頼まれた順に窓口を呼び、答えを `events` に返すスレッドを始める。
    pub fn start(front: Arc<dyn Front>, events: Sender<Event>) -> Self {
        Self {
            rows: worker("amonhen-tui-rows", front.clone(), events.clone()),
            details: worker("amonhen-tui-details", front.clone(), events.clone()),
            work: worker("amonhen-tui-work", front, events),
        }
    }

    pub fn send(&self, request: Request) {
        let to = match request {
            Request::Open | Request::Rows { .. } => &self.rows,
            Request::Commit { .. } | Request::Syntax { .. } => &self.details,
            Request::Status
            | Request::WorkingDiff { .. }
            | Request::Stage { .. }
            | Request::StageRows { .. }
            | Request::DiscardRows { .. }
            | Request::Discard { .. } => &self.work,
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
            refs: front.call(json!({"op": "refs"}))?.json,
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
        Request::Commit { id } => {
            let result = commit(front, &id).map_err(|e| e.to_string());
            Loaded::Commit { id, result }
        }
        Request::Syntax { id, mut of } => {
            of["op"] = json!("diffSyntax");
            Loaded::Syntax {
                id,
                reply: front.call(of).ok(),
            }
        }
        Request::Status => Loaded::Status(
            front
                .call(json!({"op": "status"}))
                .map(|r| r.json)
                .map_err(|e| e.to_string()),
        ),
        Request::WorkingDiff {
            path,
            serial,
            unstaged,
        } => {
            let diff =
                |mode: &str| front.call(json!({"op": "workingDiff", "path": path, "mode": mode}));
            // 行ごとの stage の印つきが出せないファイル（バイナリ・コンフリクトなど）は、片方だけを出す。
            let result = diff("combined")
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

fn commit(front: &dyn Front, id: &str) -> Result<(Value, Reply)> {
    let mut info = run(front, json!({"op": "commitInfo", "oid": id}))?;
    let diff = front.call(json!({"op": "commitDiff", "id": id}))?;
    Ok((info["info"].take(), diff))
}
