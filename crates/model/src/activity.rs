//! 内部の動きの記録（Activity の画面に出す）。git の起動とコマンドの実行を、上限つきのリングバッファに残す。
//! 記録は軽い（ロック 1 回とコピー 1 回）ので、いつも取る。読むのは Activity の画面を開いている間だけ。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 残す件数。古いものから捨てる。
const CAPACITY: usize = 1000;
/// エラーの文は、この長さで切る。
const MAX_ERROR: usize = 2000;

/// 1 つの記録。
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Record {
    /// 通し番号（新しいほど大きい）。
    pub seq: u64,
    /// "git"（git の起動）・"command"（コアの命令）。
    pub kind: &'static str,
    /// どのリポジトリか（作業ツリーのパス。わからなければ空）。
    pub repo: String,
    /// git のコマンド行や、命令の名前。
    pub label: String,
    /// 始めた時刻（UNIX 時刻のミリ秒）。
    pub started: u64,
    /// かかった時間（ミリ秒）。走っている間は None。
    pub duration: Option<f64>,
    /// 成功したか。走っている間は None。
    pub ok: Option<bool>,
    pub error: Option<String>,
    #[cfg_attr(feature = "serde", serde(skip))]
    begun: Option<Instant>,
}

static LOG: Mutex<VecDeque<Record>> = Mutex::new(VecDeque::new());
static SEQ: AtomicU64 = AtomicU64::new(0);

/// 始めたことを記録し、終えたときに `end` に渡す番号を返す。
pub fn begin(kind: &'static str, repo: &str, label: String) -> u64 {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if log.len() >= CAPACITY {
        log.pop_front();
    }
    log.push_back(Record {
        seq,
        kind,
        repo: repo.to_owned(),
        label,
        started,
        duration: None,
        ok: None,
        error: None,
        begun: Some(Instant::now()),
    });
    seq
}

/// 終えたことを記録する。もう捨てられていれば何もしない。
pub fn end(seq: u64, error: Option<&str>) {
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    // 番号の順に並んでいるので、後ろから探す（終わるのはたいてい最近始めたもの）。
    if let Some(r) = log.iter_mut().rev().find(|r| r.seq == seq) {
        r.duration = r.begun.map(|b| b.elapsed().as_secs_f64() * 1000.0);
        r.ok = Some(error.is_none());
        r.error = error.map(|e| {
            let mut e = e.to_owned();
            if e.len() > MAX_ERROR {
                let mut cut = MAX_ERROR;
                while !e.is_char_boundary(cut) {
                    cut -= 1;
                }
                e.truncate(cut);
                e.push('…');
            }
            e
        });
    }
}

/// `f` を記録しながら実行する。
pub fn track<T, E: std::fmt::Display>(
    kind: &'static str,
    repo: &str,
    label: String,
    f: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    let seq = begin(kind, repo, label);
    let result = f();
    end(seq, result.as_ref().err().map(|e| e.to_string()).as_deref());
    result
}

/// 今残っている記録（古い順）。
pub fn records() -> Vec<Record> {
    LOG.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_success_failure_and_keeps_order() {
        let a = track("git", "/r", "git status".into(), || Ok::<_, String>(()));
        assert!(a.is_ok());
        let b = track("git", "/r", "git push".into(), || Err::<(), _>("rejected"));
        assert!(b.is_err());
        let mine: Vec<_> = records().into_iter().filter(|r| r.repo == "/r").collect();
        let last = &mine[mine.len() - 2..];
        assert_eq!(last[0].ok, Some(true));
        assert_eq!(last[1].ok, Some(false));
        assert_eq!(last[1].error.as_deref(), Some("rejected"));
        assert!(last[0].seq < last[1].seq);
        assert!(last.iter().all(|r| r.duration.is_some()));
    }
}
