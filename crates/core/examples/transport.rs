//! VSCode 拡張（Node・webview）に History を渡す形を比べる。画面 1 枚分（256 行: 点・線・ID・件名・作者・日時）と
//! 全行（点・線・ID）を、JSON とレコード（`repr(C)` の並びと、UTF-8 の文字列を 1 つにまとめた置き場）で作り、
//! 時間と大きさを出す。書き出したファイルを Node で読んで測る（点は 12 バイト、線は 24 バイト、ID は 20 バイト）。
//! `cargo run --release -p amonhen-core --example transport -- <repo> <out-dir>`

use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::{HistorySnapshot, Repository};
use amonhen_layout::Edge;

const WINDOW: u32 = 256;

fn main() -> amonhen_model::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| ".".into());
    let out = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&out)?;
    let repo = Repository::new(GixBackend::open(&path)?);
    let t = Instant::now();
    let history = repo.load_history()?;
    println!("commits: {}（History {:.0} ms）", history.graph.len(), t.elapsed().as_secs_f64() * 1000.0);

    // 画面 1 枚分: 真ん中あたり。
    let start = history.graph.len() as u32 / 2;
    let end = (start + WINDOW).min(history.graph.len() as u32);
    let ids = &history.graph.ids()[start as usize..end as usize];
    let texts = repo.commit_texts(ids)?;
    let window = Window::new(&history, start, end, &texts);
    report("window", &out, || window.json(), || window.records())?;

    // 全行: 点・線・ID（文字列は画面に出す分だけ取るので含めない）。
    let all = All(&history);
    report("all", &out, || all.json(), || all.records())?;
    Ok(())
}

fn report(name: &str, out: &std::path::Path, json: impl Fn() -> Vec<u8>, records: impl Fn() -> Vec<u8>) -> amonhen_model::Result<()> {
    for (kind, make) in [("json", &json as &dyn Fn() -> Vec<u8>), ("bin", &records)] {
        let mut times = Vec::new();
        let mut bytes = Vec::new();
        let runs = if name == "all" { 5 } else { 200 };
        for _ in 0..runs {
            let t = Instant::now();
            bytes = make();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{name:<7} {kind:<5} 作る p50 {:>8.3} ms  大きさ {:>10} バイト",
            times[times.len() / 2],
            bytes.len()
        );
        std::fs::write(out.join(format!("{name}.{kind}")), &bytes)?;
    }
    Ok(())
}

struct Window<'a> {
    history: &'a HistorySnapshot,
    start: u32,
    end: u32,
    texts: &'a [amonhen_model::CommitText],
}

impl<'a> Window<'a> {
    fn new(history: &'a HistorySnapshot, start: u32, end: u32, texts: &'a [amonhen_model::CommitText]) -> Self {
        Self { history, start, end, texts }
    }

    fn edges(&self) -> Vec<Edge> {
        let all = self.history.layout.edges();
        self.history.layout.edges_in(self.start..self.end).map(|i| all[i as usize]).collect()
    }

    fn json(&self) -> Vec<u8> {
        let nodes = self.history.layout.nodes();
        let ids = self.history.graph.ids();
        let rows: Vec<serde_json::Value> = (self.start..self.end)
            .zip(self.texts)
            .map(|(r, t)| {
                let n = &nodes[r as usize];
                serde_json::json!({
                    "row": r,
                    "id": ids[r as usize].to_string(),
                    "lane": n.lane,
                    "color": n.color,
                    "flags": n.flags,
                    "subject": String::from_utf8_lossy(t.subject()),
                    "author": String::from_utf8_lossy(t.author_name()),
                    "time": t.author_time,
                })
            })
            .collect();
        let edges: Vec<serde_json::Value> = self
            .edges()
            .iter()
            .map(|e| serde_json::json!([e.row_start, e.row_end, e.lane_start, e.lane_end, e.color, e.kind]))
            .collect();
        serde_json::to_vec(&serde_json::json!({"rows": rows, "edges": edges})).unwrap()
    }

    /// 先頭: 行数・線の数（u32）。続けて 8 バイト境界で、点（Node）・線（Edge）・ID（20 バイト）・日時（i64）・
    /// 文字列の位置（件名・作者の始めと長さ、u32 × 4）・文字列（UTF-8）。
    fn records(&self) -> Vec<u8> {
        let nodes = &self.history.layout.nodes()[self.start as usize..self.end as usize];
        let edges = self.edges();
        let ids = &self.history.graph.ids()[self.start as usize..self.end as usize];
        let mut out = Vec::new();
        out.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
        out.extend_from_slice(&(edges.len() as u32).to_le_bytes());
        put(&mut out, bytes_of(nodes));
        put(&mut out, bytes_of(&edges));
        put(&mut out, bytes_of(ids));
        let times: Vec<i64> = self.texts.iter().map(|t| t.author_time).collect();
        put(&mut out, bytes_of(&times));
        let mut text = Vec::new();
        let mut spans = Vec::with_capacity(self.texts.len() * 4);
        for t in self.texts {
            for part in [t.subject(), t.author_name()] {
                spans.push(text.len() as u32);
                spans.push(part.len() as u32);
                text.extend_from_slice(part);
            }
        }
        put(&mut out, bytes_of(&spans));
        put(&mut out, &text);
        out
    }
}

struct All<'a>(&'a HistorySnapshot);

impl All<'_> {
    fn json(&self) -> Vec<u8> {
        let h = self.0;
        let nodes: Vec<serde_json::Value> = h
            .layout
            .nodes()
            .iter()
            .zip(h.graph.ids().iter())
            .map(|(n, id)| serde_json::json!([id.to_string(), n.lane, n.color, n.flags]))
            .collect();
        let edges: Vec<serde_json::Value> = h
            .layout
            .edges()
            .iter()
            .map(|e| serde_json::json!([e.row_start, e.row_end, e.lane_start, e.lane_end, e.color, e.kind]))
            .collect();
        serde_json::to_vec(&serde_json::json!({"nodes": nodes, "edges": edges})).unwrap()
    }

    fn records(&self) -> Vec<u8> {
        let h = self.0;
        let mut out = Vec::new();
        out.extend_from_slice(&(h.layout.nodes().len() as u32).to_le_bytes());
        out.extend_from_slice(&(h.layout.edges().len() as u32).to_le_bytes());
        put(&mut out, bytes_of(h.layout.nodes()));
        put(&mut out, bytes_of(h.layout.edges()));
        put(&mut out, bytes_of(h.graph.ids()));
        out
    }
}

/// 8 バイト境界に揃えてから足す。
fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
    out.extend_from_slice(bytes);
}

fn bytes_of<T>(items: &[T]) -> &[u8] {
    // SAFETY: `Node`・`Edge`・`Oid`・数値は `repr(C)` で詰め物のない並び。
    unsafe { std::slice::from_raw_parts(items.as_ptr().cast(), std::mem::size_of_val(items)) }
}
