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
    /// 差分（`view` は `commitDiff` の答えのもの）の構文の色。
    Syntax { id: String, view: Value },
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
}

pub struct Loader {
    rows: Sender<Request>,
    details: Sender<Request>,
}

impl Loader {
    /// 頼まれた順に窓口を呼び、答えを `events` に返すスレッドを始める。
    pub fn start(front: Arc<dyn Front>, events: Sender<Event>) -> Self {
        Self {
            rows: worker("amonhen-tui-rows", front.clone(), events.clone()),
            details: worker("amonhen-tui-details", front, events),
        }
    }

    pub fn send(&self, request: Request) {
        let to = match request {
            Request::Open | Request::Rows { .. } => &self.rows,
            Request::Commit { .. } | Request::Syntax { .. } => &self.details,
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
        Request::Syntax { id, view } => Loaded::Syntax {
            id,
            reply: front.call(json!({"op": "diffSyntax", "view": view})).ok(),
        },
    })
}

fn commit(front: &dyn Front, id: &str) -> Result<(Value, Reply)> {
    let mut info = front
        .call(json!({"op": "run", "command": {"op": "commitInfo", "oid": id}}))?
        .json;
    if info["ok"] != true {
        return Err(info["error"]
            .as_str()
            .map_or_else(|| tr!("Failed"), str::to_owned)
            .into());
    }
    let diff = front.call(json!({"op": "commitDiff", "id": id}))?;
    Ok((info["info"].take(), diff))
}
