//! 窓口を呼ぶワーカー。`Front::call` は答えを待つので、メインスレッドからは呼ばない。

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use amonhen_front::{Front, Reply};
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
}

pub struct Loader {
    tx: Sender<Request>,
}

impl Loader {
    /// 頼まれた順に窓口を呼び、答えを `events` に返すスレッドを始める。
    pub fn start(front: Arc<dyn Front>, events: Sender<Event>) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let _ = std::thread::Builder::new()
            .name("amonhen-tui-loader".into())
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
        Self { tx }
    }

    pub fn send(&self, request: Request) {
        let _ = self.tx.send(request);
    }
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
    })
}
