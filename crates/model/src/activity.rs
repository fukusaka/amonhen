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
    /// 走っている間の進み具合（git の `--progress` の最後の行など）。
    pub progress: Option<String>,
    /// 取り消せるか（走っている間だけ意味がある）。
    pub cancellable: bool,
    /// 成功したときの標準エラー（hooks の出力など。なければ None）。
    pub output: Option<String>,
    #[cfg_attr(feature = "serde", serde(skip))]
    begun: Option<Instant>,
}

/// 取り消しを頼まれた記録の番号。
static CANCELLED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

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
        progress: None,
        cancellable: false,
        output: None,
        begun: Some(Instant::now()),
    });
    seq
}

fn with_record(seq: u64, f: impl FnOnce(&mut Record)) {
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = log.iter_mut().rev().find(|r| r.seq == seq) {
        f(r);
    }
}

/// 走っている記録の進み具合を書く。
pub fn set_progress(seq: u64, progress: &str) {
    with_record(seq, |r| r.progress = Some(progress.to_owned()));
}

/// 成功したときの出力を残す（長ければ切る）。
pub fn set_output(seq: u64, output: &str) {
    let mut text = output.to_owned();
    if text.len() > MAX_ERROR {
        let mut cut = MAX_ERROR;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push('…');
    }
    with_record(seq, |r| r.output = Some(text));
}

/// 取り消せる記録にする（`cancelled` で確かめながら走るもの）。
pub fn set_cancellable(seq: u64) {
    with_record(seq, |r| r.cancellable = true);
}

/// 取り消しを頼む。取り消せる記録で、まだ走っていれば true。
pub fn cancel(seq: u64) -> bool {
    let running = {
        let log = LOG.lock().unwrap_or_else(|e| e.into_inner());
        log.iter()
            .any(|r| r.seq == seq && r.cancellable && r.ok.is_none())
    };
    if running {
        CANCELLED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seq);
    }
    running
}

/// 取り消しを頼まれたか（頼まれていれば印を消して true）。
pub fn take_cancel(seq: u64) -> bool {
    let mut cancelled = CANCELLED.lock().unwrap_or_else(|e| e.into_inner());
    let found = cancelled.iter().position(|&s| s == seq);
    if let Some(i) = found {
        cancelled.swap_remove(i);
    }
    found.is_some()
}

/// 今走っている記録の番号（`f` の中から、自分の記録に進み具合を書くため）。
pub fn track_with<T, E: std::fmt::Display>(
    kind: &'static str,
    repo: &str,
    label: String,
    f: impl FnOnce(u64) -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    let seq = begin(kind, repo, label);
    let result = f(seq);
    end(seq, result.as_ref().err().map(|e| e.to_string()).as_deref());
    result
}

/// 終えたことを記録する。もう捨てられていれば何もしない。
pub fn end(seq: u64, error: Option<&str>) {
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    // 番号の順に並んでいるので、後ろから探す（終わるのはたいてい最近始めたもの）。
    if let Some(r) = log.iter_mut().rev().find(|r| r.seq == seq) {
        r.duration = r.begun.map(|b| b.elapsed().as_secs_f64() * 1000.0);
        r.progress = None;
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

/// 記録を、ほかのプロセス（SSH 先の daemon）から手元へ渡す形。
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Entry {
    pub seq: u64,
    pub kind: String,
    pub repo: String,
    pub label: String,
    pub started: u64,
    pub duration: Option<f64>,
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub progress: Option<String>,
    pub cancellable: bool,
    pub output: Option<String>,
}

/// 今残っている記録を、渡せる形で（古い順）。
pub fn entries() -> Vec<Entry> {
    records()
        .into_iter()
        .map(|r| Entry {
            seq: r.seq,
            kind: r.kind.to_owned(),
            repo: r.repo,
            label: r.label,
            started: r.started,
            duration: r.duration,
            ok: r.ok,
            error: r.error,
            progress: r.progress,
            cancellable: r.cancellable,
            output: r.output,
        })
        .collect()
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

    #[test]
    fn cancels_only_running_cancellable_records() {
        let seq = begin("git", "/c", "git fetch".into());
        assert!(!cancel(seq), "取り消せる印がなければ断る");
        set_cancellable(seq);
        set_progress(seq, "Receiving objects: 10%");
        assert!(
            records()
                .iter()
                .any(|r| r.seq == seq && r.progress.as_deref() == Some("Receiving objects: 10%"))
        );
        assert!(cancel(seq));
        assert!(take_cancel(seq));
        assert!(!take_cancel(seq));
        end(seq, Some("Cancelled"));
        assert!(!cancel(seq), "終わったものは取り消せない");
    }
}
