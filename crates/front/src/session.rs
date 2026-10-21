//! FE 1 つとのやりとり。History のスナップショットと Working Copy の差分は番号で持ち、FE はその番号で続きを頼む
//! （FE がスクロールする間に History が新しくなっても、見ている行は変わらない）。

use amonhen_model::tr;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use amonhen_core::diff::DiffView;
use amonhen_core::{HistorySnapshot, PairMode, Repository, Selection, WorkingDiff};
use amonhen_model::i18n::{self, Lang};
use amonhen_model::{ChangeKind, FileChange, Oid, RefKind, StatusEntry, StatusKind};
use amonhen_projection::FrontendProfile;
use serde_json::{Value, json};

use crate::{Records, Reply, Result};

/// 覚えておく History と差分の数（古いものから捨てる）。
const KEEP: usize = 16;

const PROFILE: FrontendProfile = FrontendProfile::WEB;

pub struct Session {
    repo: Arc<Repository>,
    next: AtomicU64,
    histories: Mutex<Kept<Arc<HistorySnapshot>>>,
    diffs: Mutex<Kept<Arc<WorkingDiff>>>,
    /// 相手が `hello` で伝えた言語（`Lang` の値）。答えのエラーの文に使う。伝えなければプロセスの既定。
    language: AtomicU8,
}

/// 番号つきで、新しいものを `KEEP` 個まで。
struct Kept<T> {
    items: HashMap<u64, T>,
    order: Vec<u64>,
}

impl<T: Clone> Kept<T> {
    fn new() -> Self {
        Self {
            items: HashMap::new(),
            order: Vec::new(),
        }
    }

    fn insert(&mut self, id: u64, item: T) {
        self.items.insert(id, item);
        self.order.push(id);
        while self.order.len() > KEEP {
            let old = self.order.remove(0);
            self.items.remove(&old);
        }
    }

    fn get(&self, id: u64) -> Option<T> {
        self.items.get(&id).cloned()
    }

    fn remove(&mut self, id: u64) {
        self.items.remove(&id);
        self.order.retain(|i| *i != id);
    }
}

fn arg<'a>(request: &'a Value, name: &str) -> Result<&'a Value> {
    let v = &request[name];
    if v.is_null() {
        Err(tr!("{name} is missing", name = name).into())
    } else {
        Ok(v)
    }
}

fn str_arg<'a>(request: &'a Value, name: &str) -> Result<&'a str> {
    arg(request, name)?
        .as_str()
        .ok_or_else(|| tr!("{name} must be a string", name = name).into())
}

fn u64_arg(request: &Value, name: &str) -> Result<u64> {
    arg(request, name)?
        .as_u64()
        .ok_or_else(|| tr!("{name} must be a number", name = name).into())
}

fn rows_arg(request: &Value, name: &str) -> Result<Vec<u32>> {
    arg(request, name)?
        .as_array()
        .ok_or_else(|| tr!("{name} must be an array of numbers", name = name))?
        .iter()
        .map(|v| {
            v.as_u64()
                .map(|n| n as u32)
                .ok_or_else(|| tr!("{name} must be an array of numbers", name = name).into())
        })
        .collect()
}

fn oid(text: &str) -> Result<Oid> {
    Oid::from_hex(text).ok_or_else(|| tr!("Not a commit ID: {text}", text = text).into())
}

fn oids(request: &Value, name: &str) -> Result<Option<Vec<Oid>>> {
    match &request[name] {
        Value::Null => Ok(None),
        Value::Array(items) => items
            .iter()
            .map(|v| oid(v.as_str().unwrap_or_default()))
            .collect::<Result<Vec<_>>>()
            .map(Some),
        _ => Err(tr!("{name} must be an array of IDs", name = name).into()),
    }
}

impl Session {
    pub fn new(repo: Arc<Repository>) -> Self {
        Self {
            repo,
            next: AtomicU64::new(1),
            histories: Mutex::new(Kept::new()),
            diffs: Mutex::new(Kept::new()),
            language: AtomicU8::new(i18n::current() as u8),
        }
    }

    pub fn repository(&self) -> &Arc<Repository> {
        &self.repo
    }

    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    fn history(&self, request: &Value) -> Result<Arc<HistorySnapshot>> {
        let id = u64_arg(request, "history")?;
        self.histories
            .lock()
            .unwrap()
            .get(id)
            .ok_or_else(|| tr!("History {id} no longer exists (reload it)", id = id).into())
    }

    fn keep_history(&self, h: Arc<HistorySnapshot>) -> Value {
        let id = self.id();
        let head = h.head.and_then(|id| h.row_of(&id));
        let reply = json!({
            "history": id,
            "generation": h.generation,
            "rows": h.graph.len(),
            "head": h.head.map(|o| o.to_string()),
            "headRow": head,
            "folded": !h.folded.is_empty(),
        });
        self.histories.lock().unwrap().insert(id, h);
        reply
    }

    /// 命令（`{"op": "...", ...}`）を実行する。`cancelled` が true になれば、時間のかかる処理をやめる。
    pub fn handle(&self, request: &Value, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Reply> {
        if request["op"] == "hello"
            && let Some(language) = request["language"].as_str()
        {
            self.language
                .store(Lang::parse(language) as u8, Ordering::Relaxed);
        }
        let lang = if self.language.load(Ordering::Relaxed) == Lang::Ja as u8 {
            Lang::Ja
        } else {
            Lang::En
        };
        i18n::with(lang, || self.handle_in_language(request, cancelled))
    }

    fn handle_in_language(
        &self,
        request: &Value,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Reply> {
        let op = request["op"].as_str().ok_or_else(|| tr!("op is missing"))?;
        let repo = &*self.repo;
        Ok(match op {
            "hello" => {
                let paths = repo.paths();
                Reply::json(json!({
                    "version": crate::VERSION,
                    "workdir": paths.workdir.map(|p| p.display().to_string()),
                    "gitDir": paths.git_dir.display().to_string(),
                }))
            }

            // MARK: History
            "history" => {
                let tips = oids(request, "tips")?;
                let h = if request["rebuild"] == true {
                    repo.rebuild_history()?
                } else if request["fold"] == true {
                    let expanded = oids(request, "expanded")?.unwrap_or_default();
                    repo.history_folded(tips.as_deref(), &expanded)?
                } else if let Some(tips) = &tips {
                    repo.history_filtered(tips)?
                } else {
                    // 前回の History（永続キャッシュ）があれば、そこから増えた分だけ読む。
                    repo.load_cached_history();
                    repo.load_history()?
                };
                Reply::json(self.keep_history(h))
            }
            "historyMatching" => {
                let h = self.history(request)?;
                let m = repo.history_matching(&h, &rows_arg(request, "rows")?);
                Reply::json(self.keep_history(m))
            }
            "historyRelease" => {
                self.histories
                    .lock()
                    .unwrap()
                    .remove(u64_arg(request, "history")?);
                Reply::json(json!({}))
            }
            "historyRows" => self.history_rows(request)?,
            "historyGraph" => {
                let h = self.history(request)?;
                let mut r = Records::new();
                r.put_slice("nodes", h.layout.nodes())
                    .put_slice("edges", h.layout.edges())
                    .put_slice("ids", h.graph.ids())
                    .put_slice("folded", &h.folded);
                r.finish(json!({"rows": h.graph.len(), "edges": h.layout.edges().len()}))
            }
            "historySearch" => {
                let h = self.history(request)?;
                let rows = repo.search_history_until(&h, str_arg(request, "query")?, cancelled)?;
                let mut r = Records::new();
                r.put_slice("rows", &rows);
                r.finish(json!({"count": rows.len()}))
            }
            "historyDensity" => {
                let h = self.history(request)?;
                let d = h.density(u64_arg(request, "buckets")? as usize);
                let mut r = Records::new();
                r.put_slice("density", &d);
                r.finish(json!({"buckets": d.len()}))
            }
            "historyLineage" => {
                let h = self.history(request)?;
                let l = amonhen_core::lineage::lineage(
                    &h.graph,
                    &h.layout,
                    u64_arg(request, "row")? as u32,
                );
                let mut r = Records::new();
                r.put_slice("rows", &l.rows)
                    .put_slice("edgeFrom", &l.edge_from);
                r.finish(json!({}))
            }
            "historyLanes" => {
                let h = self.history(request)?;
                let m = &request["metrics"];
                let f = |k: &str| m[k].as_f64().unwrap_or(0.0) as f32;
                let metrics = amonhen_core::lanes::LaneMetrics {
                    full: f("full"),
                    thin: f("thin"),
                    min_full: f("minFull"),
                    min_thin: f("minThin"),
                    available: f("available"),
                };
                let (start, end) = (
                    u64_arg(request, "start")? as u32,
                    u64_arg(request, "end")? as u32,
                );
                let (positions, width) =
                    amonhen_core::lanes::lane_positions(&h.layout, start..end, &metrics);
                let mut r = Records::new();
                r.put_slice("positions", &positions);
                r.finish(json!({"width": width}))
            }
            "historyRowOf" => {
                let h = self.history(request)?;
                Reply::json(json!({"row": h.row_of(&oid(str_arg(request, "id")?)?)}))
            }
            "historyOrder" => {
                let name = str_arg(request, "order")?;
                let order = amonhen_model::HistoryOrder::from_name(name)
                    .ok_or_else(|| tr!("Unknown History order: {name}", name = name))?;
                repo.set_history_order(order)?;
                Reply::json(json!({"order": order.name()}))
            }

            // MARK: 参照・状態
            "refs" => Reply::json(refs_json(&repo.load_refs()?.refs)),
            "status" => {
                let s = repo.load_status()?;
                Reply::json(json!({
                    "generation": s.generation,
                    "branch": s.status.branch,
                    "upstream": s.status.upstream,
                    "ahead": s.status.ahead,
                    "behind": s.status.behind,
                    "entries": s.status.entries.iter().map(entry_json).collect::<Vec<_>>(),
                }))
            }
            "connection" => Reply::json(match repo.connection_state() {
                Some(c) => {
                    json!({"sent": c.sent, "received": c.received, "closed": c.closed, "engine": c.engine})
                }
                None => json!(null),
            }),

            // MARK: 差分
            "commitDiff" => self.commit_diff(request)?,
            "workingDiff" => self.working_diff(request)?,
            "conflict" => {
                let path = str_arg(request, "path")?;
                let view = repo.conflict_view(path, &PROFILE)?;
                diff_records(&view).finish(json!({"path": path, "rows": view.rows.len()}))
            }

            // MARK: Working Copy の操作
            "stage" | "unstage" => {
                let paths = paths_arg(request)?;
                let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
                if op == "stage" {
                    repo.stage(&refs)?
                } else {
                    repo.unstage(&refs)?
                }
                Reply::json(json!({}))
            }
            "stageRows" => {
                let diff = self.diff(request)?;
                let rows: Vec<usize> = rows_arg(request, "rows")?
                    .into_iter()
                    .map(|r| r as usize)
                    .collect();
                match &diff.combined {
                    Some(_) => repo.set_rows_staged(&diff, &rows, request["stage"] != false)?,
                    None => repo.stage_rows(&diff, &rows)?,
                }
                Reply::json(json!({}))
            }
            "discardRows" => {
                let diff = self.diff(request)?;
                let rows: Vec<usize> = rows_arg(request, "rows")?
                    .into_iter()
                    .map(|r| r as usize)
                    .collect();
                repo.discard_rows(&diff, &rows)?;
                Reply::json(json!({}))
            }
            "commit" => {
                repo.commit(str_arg(request, "message")?, request["amend"] == true)?;
                Reply::json(json!({}))
            }

            // コアの命令（`run_json`）をそのまま。
            "run" => {
                let reply: Value =
                    serde_json::from_str(&repo.run_json(&arg(request, "command")?.to_string()))?;
                Reply::json(reply)
            }
            other => return Err(tr!("Unknown command: {other}", other = other).into()),
        })
    }

    /// History の行: `start` から `count` 行（その範囲に重なる線も）か、`rows` で指した行（検索の結果など。線は
    /// なし）。点・ID・日時・件名と作者（UTF-8 の置き場の位置）・署名・折りたたんだ数・親の行。
    fn history_rows(&self, request: &Value) -> Result<Reply> {
        let h = self.history(request)?;
        let total = h.graph.len() as u32;
        let (rows, edge_ids): (Vec<u32>, Vec<u32>) = match &request["rows"] {
            Value::Null => {
                let start = (u64_arg(request, "start")? as u32).min(total);
                let end = start
                    .saturating_add(u64_arg(request, "count")? as u32)
                    .min(total);
                (
                    (start..end).collect(),
                    h.layout.edges_in(start..end).collect(),
                )
            }
            _ => {
                let rows = rows_arg(request, "rows")?;
                if let Some(bad) = rows.iter().find(|&&r| r >= total) {
                    return Err(tr!("Row not in History: {bad}", bad = bad).into());
                }
                (rows, Vec::new())
            }
        };
        let contiguous = rows.windows(2).all(|w| w[1] == w[0] + 1);
        // 件名と作者。続いた行なら FE 向けの形（`history_chunk`）、とびとびなら 1 つずつ。
        let mut text = Vec::new();
        let mut spans = Vec::with_capacity(rows.len() * 4);
        let mut push = |part: &[u8]| {
            spans.push(text.len() as u32);
            spans.push(part.len() as u32);
            text.extend_from_slice(part);
        };
        let (times, signatures) = if contiguous && !rows.is_empty() {
            let chunk = self
                .repo
                .history_chunk(&h, rows[0], rows.len() as u32, &PROFILE)?;
            for (s, a) in chunk.subjects.iter().zip(&chunk.author_names) {
                push(chunk.strings.bytes(*s));
                push(chunk.strings.bytes(*a));
            }
            (chunk.author_times, chunk.signatures)
        } else {
            let ids: Vec<Oid> = rows.iter().map(|&r| h.graph.ids()[r as usize]).collect();
            let texts = self.repo.commit_texts(&ids)?;
            for t in &texts {
                push(String::from_utf8_lossy(t.subject()).as_bytes());
                push(String::from_utf8_lossy(t.author_name()).as_bytes());
            }
            (
                texts.iter().map(|t| t.author_time).collect(),
                texts.iter().map(|t| t.signature).collect(),
            )
        };
        let nodes: Vec<amonhen_layout::Node> =
            rows.iter().map(|&r| h.layout.nodes()[r as usize]).collect();
        let ids: Vec<Oid> = rows.iter().map(|&r| h.graph.ids()[r as usize]).collect();
        let folded: Vec<u32> = if h.folded.is_empty() {
            Vec::new()
        } else {
            rows.iter().map(|&r| h.folded[r as usize]).collect()
        };
        // 親の行: 行ごとの始まり（行の数 + 1 個）と、続けた親の行。
        let mut parent_starts = Vec::with_capacity(rows.len() + 1);
        let mut parents = Vec::new();
        for &r in &rows {
            parent_starts.push(parents.len() as u32);
            parents.extend_from_slice(h.graph.parents(r as usize));
        }
        parent_starts.push(parents.len() as u32);
        let parent_ids: Vec<Oid> = parents.iter().map(|&p| h.graph.ids()[p as usize]).collect();
        let all = h.layout.edges();
        let edges: Vec<amonhen_layout::Edge> = edge_ids.iter().map(|&i| all[i as usize]).collect();
        let mut r = Records::new();
        r.put_slice("rowNumbers", &rows)
            .put_slice("nodes", &nodes)
            .put_slice("ids", &ids)
            .put_slice("times", &times)
            .put_slice("spans", &spans)
            .put("text", &text)
            .put("signatures", &signatures)
            .put_slice("folded", &folded)
            .put_slice("parentStarts", &parent_starts)
            .put_slice("parents", &parents)
            .put_slice("parentIds", &parent_ids)
            .put_slice("edges", &edges)
            .put_slice("edgeIds", &edge_ids);
        Ok(r.finish(json!({"count": rows.len(), "edges": edges.len()})))
    }

    fn commit_diff(&self, request: &Value) -> Result<Reply> {
        let repo = &*self.repo;
        let (changes, stacked, selection) = if let Ok(id) = str_arg(request, "id") {
            let changes = repo.commit_changes(&oid(id)?)?;
            let stacked = repo.stacked_diff(&changes)?;
            (Some(changes), stacked, Value::Null)
        } else if let Ok(id) = str_arg(request, "stash") {
            let changes = repo.stash_changes(&oid(id)?)?;
            let stacked = repo.stacked_diff(&changes)?;
            (Some(changes), stacked, Value::Null)
        } else {
            let h = self.history(request)?;
            let pair = match request["pair"].as_str() {
                Some("reverse") => PairMode::Reverse,
                Some("mergeBase") => PairMode::MergeBase,
                _ => PairMode::Forward,
            };
            let (selection, stacked) =
                repo.selection_diff(&h, &rows_arg(request, "rows")?, pair)?;
            (None, stacked, selection_json(&selection))
        };
        let collapsed = vec![false; stacked.files.len()];
        let view = stacked.view(&collapsed, &PROFILE);
        let files: Vec<Value> = stacked
            .files
            .iter()
            .enumerate()
            .map(|(i, (path, d))| {
                let change = changes.as_ref().and_then(|c| c.get(i));
                json!({
                    "path": path,
                    "oldPath": change.and_then(|c| c.old_path.clone()),
                    "kind": change.map(|c| kind_name(c.kind)),
                    "added": d.added,
                    "removed": d.removed,
                    "binary": d.binary,
                })
            })
            .collect();
        let changes: Vec<Value> = changes.iter().flatten().map(change_json).collect();
        Ok(diff_records(&view).finish(json!({
            "files": files,
            "changes": changes,
            "selection": selection,
            "rows": view.rows.len(),
        })))
    }

    fn working_diff(&self, request: &Value) -> Result<Reply> {
        let repo = &*self.repo;
        let path = str_arg(request, "path")?;
        let status = repo.load_status()?;
        let entry = status
            .status
            .entries
            .iter()
            .find(|e| e.path == path)
            .ok_or_else(|| tr!("File has no changes: {path}", path = path))?;
        let mode = request["mode"].as_str().unwrap_or("unstaged");
        let diff = match mode {
            "staged" => repo.working_diff(entry, true, &PROFILE)?,
            "unstaged" => repo.working_diff(entry, false, &PROFILE)?,
            "combined" => repo
                .working_diff_combined(entry, &PROFILE)?
                .ok_or_else(|| tr!("This diff can’t be shown with staged marks (read the staged and unstaged diffs separately)"))?,
            other => return Err(tr!("Unknown mode: {other}", other = other).into()),
        };
        let id = self.id();
        let reply = diff_records(&diff.view).finish(json!({
            "diff": id,
            "path": path,
            "mode": mode,
            "binary": diff.view.binary,
            "added": diff.view.added,
            "removed": diff.view.removed,
            "rows": diff.view.rows.len(),
            "lineEndings": diff.line_endings.as_ref().map(|le| json!({
                "index": le.index.name(),
                "worktree": le.worktree.name(),
                "checkout": le.checkout.name(),
                "rule": le.rule,
                "notable": le.notable(),
                "warning": le.warning(),
            })),
        }));
        self.diffs.lock().unwrap().insert(id, Arc::new(diff));
        Ok(reply)
    }

    fn diff(&self, request: &Value) -> Result<Arc<WorkingDiff>> {
        let id = u64_arg(request, "diff")?;
        self.diffs
            .lock()
            .unwrap()
            .get(id)
            .ok_or_else(|| tr!("Diff {id} no longer exists (reload it)", id = id).into())
    }
}

fn paths_arg(request: &Value) -> Result<Vec<String>> {
    arg(request, "paths")?
        .as_array()
        .ok_or_else(|| tr!("paths must be an array of strings"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| tr!("paths must be an array of strings").into())
        })
        .collect()
}

/// 差分の行: `rows` は 1 行 16 バイト（kind u8・flags u8・2 バイト空き・old u32・new u32・text の位置 u32）、
/// `textLengths` は行ごとの長さ（u32）、`text` は UTF-8、`highlights` は（行・始め・終わり）の u32 × 3（バイト位置）。
fn diff_records(view: &DiffView) -> Records {
    let mut text = Vec::new();
    let mut rows = Vec::with_capacity(view.rows.len() * 4);
    let mut lengths = Vec::with_capacity(view.rows.len());
    for row in &view.rows {
        let bytes = view.strings.bytes(row.text);
        rows.push(u32::from(row.kind) | (u32::from(row.flags) << 8));
        rows.push(row.old_no);
        rows.push(row.new_no);
        rows.push(text.len() as u32);
        lengths.push(bytes.len() as u32);
        text.extend_from_slice(bytes);
    }
    let highlights: Vec<u32> = view
        .highlights
        .iter()
        .flat_map(|h| [h.row, h.start, h.end])
        .collect();
    let mut r = Records::new();
    r.put_slice("rows", &rows)
        .put_slice("textLengths", &lengths)
        .put("text", &text)
        .put_slice("highlights", &highlights);
    r
}

fn selection_json(s: &Selection) -> Value {
    match s {
        Selection::Range { base, tip, count } => {
            json!({"kind": "range", "base": base.map(|o| o.to_string()), "tip": tip.to_string(), "count": count})
        }
        Selection::Pair { from, to } => {
            json!({"kind": "pair", "from": from.to_string(), "to": to.to_string()})
        }
        Selection::Each(ids) => {
            json!({"kind": "each", "ids": ids.iter().map(Oid::to_string).collect::<Vec<_>>()})
        }
    }
}

pub(crate) fn kind_name(k: ChangeKind) -> &'static str {
    match k {
        ChangeKind::Added => "added",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Modified => "modified",
        ChangeKind::Renamed => "renamed",
        ChangeKind::Copied => "copied",
    }
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

fn refs_json(refs: &amonhen_model::Refs) -> Value {
    let kind = |k: RefKind| match k {
        RefKind::Local => "branch",
        RefKind::Remote => "remote",
        RefKind::Tag => "tag",
        RefKind::Stash => "stash",
        RefKind::Submodule => "submodule",
    };
    json!({
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
    })
}
