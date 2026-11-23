//! コミットの詳細: 件名とメッセージ、作者と日付、変わったファイル、差分。上から下へ 1 本の文書として並べ、
//! スクロールする。

use amonhen_front::Reply;
use amonhen_model::{Oid, tr};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::diff::{DIM, Diff, KeptRows, Scroll, Theme, counts, draw_row_kept, fit_path};

use crate::Clock;
use crate::history::{Marked, ago};
use crate::loader::Request;

struct File {
    path: String,
    old_path: Option<String>,
    /// 「added」「modified」など。
    kind: String,
    added: u64,
    removed: u64,
    binary: bool,
}

/// 差分で無視する空白（アプリの Ignore Whitespace。`commitDiff` の whitespace）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Whitespace {
    #[default]
    Exact,
    AtEol,
    Change,
    All,
}

impl Whitespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Whitespace::Exact => "exact",
            Whitespace::AtEol => "at-eol",
            Whitespace::Change => "change",
            Whitespace::All => "all",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Whitespace::Exact => Whitespace::AtEol,
            Whitespace::AtEol => Whitespace::Change,
            Whitespace::Change => Whitespace::All,
            Whitespace::All => Whitespace::Exact,
        }
    }
}

/// ファイルの一覧の 1 行（一覧なら、ファイルだけ。木なら、フォルダとその下のファイル）。
#[derive(Debug, PartialEq)]
enum Listed {
    /// `files` の何番目か、木での深さ。
    File(usize, u16),
    Folder {
        path: String,
        depth: u16,
        folded: bool,
        /// 下にあるファイルの数（たたんだときに出す）。
        files: usize,
    },
}

/// ファイルの一覧の行を組む。木では、パスの順にフォルダを挟み、たたんだフォルダの下は出さない。
fn list_rows(files: &[File], tree: bool, folded: &HashSet<String>) -> Vec<Listed> {
    if !tree {
        return (0..files.len()).map(|i| Listed::File(i, 0)).collect();
    }
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| files[a].path.cmp(&files[b].path));
    // フォルダごとの、下にあるファイルの数。
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for file in files {
        let mut end = 0;
        while let Some(at) = file.path[end..].find('/') {
            end += at;
            *counts.entry(&file.path[..end]).or_default() += 1;
            end += 1;
        }
    }
    let mut rows = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    for index in order {
        let path = files[index].path.as_str();
        let dirs: Vec<&str> = path.match_indices('/').map(|(at, _)| &path[..at]).collect();
        let same = open.iter().zip(&dirs).take_while(|(a, b)| a == b).count();
        open.truncate(same);
        let mut hidden = open.iter().any(|d| folded.contains(*d));
        for dir in &dirs[same..] {
            if !hidden {
                rows.push(Listed::Folder {
                    path: (*dir).to_owned(),
                    depth: open.len() as u16,
                    folded: folded.contains(*dir),
                    files: counts.get(dir).copied().unwrap_or(0),
                });
            }
            hidden |= folded.contains(*dir);
            open.push(dir);
        }
        if !hidden {
            rows.push(Listed::File(index, dirs.len() as u16));
        }
    }
    rows
}

/// 2 つのコミットを選んだときの、差分の取り方（アプリの ⇄ と「merge-base から」）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pair {
    /// 古い方から新しい方へ（A..B）。
    #[default]
    Forward,
    /// 向きを入れ替える。
    Reverse,
    /// 分かれたところから新しい方へ（A...B）。
    MergeBase,
}

impl Pair {
    fn as_str(self) -> &'static str {
        match self {
            Pair::Forward => "forward",
            Pair::Reverse => "reverse",
            Pair::MergeBase => "mergeBase",
        }
    }
}

/// 出したいもの: 1 つのコミット（か stash）、または History で印をつけたコミット（複数選択）。
#[derive(Clone, Debug, PartialEq)]
pub enum Want {
    Commit {
        id: Oid,
        stash: bool,
    },
    Selection {
        /// 窓口での History の番号。
        history: Value,
        rows: Vec<u32>,
        pair: Pair,
        /// 印をつけたコミット（新しい順）。
        commits: Vec<Marked>,
    },
}

impl Want {
    /// 頼んだものと届いたものを突き合わせる鍵。
    fn key(&self) -> String {
        match self {
            Want::Commit { id, .. } => id.to_string(),
            Want::Selection {
                history,
                rows,
                pair,
                ..
            } => format!("selection {history} {rows:?} {}", pair.as_str()),
        }
    }
}

/// 複数選択の差分の、頭に並べるもの。
struct Selected {
    /// 窓口の答えの `selection`（`kind`: `range`・`pair`・`each`）。
    how: Value,
    commits: Vec<Marked>,
}

struct Commit {
    /// 頼んだものの鍵（`Want::key`）。
    key: String,
    /// 出しているコミット。複数選択では、新しい方の端（Blame・File History をそこで開く）。
    id: Oid,
    info: Value,
    selected: Option<Selected>,
    files: Vec<File>,
    /// ファイルの一覧の行（`list_rows`）。
    listed: Vec<Listed>,
    diff: Diff,
    /// 頭の部分（件名から「Parents」まで）を、幅 `head_width` で組んだもの。
    head: Vec<(String, Style)>,
    head_width: u16,
}

impl Commit {
    fn new(key: String, id: Oid, info: Value, diff: &Reply) -> Self {
        let files = diff.json["files"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| File {
                path: f["path"].as_str().unwrap_or("").to_owned(),
                old_path: f["oldPath"].as_str().map(str::to_owned),
                kind: f["kind"].as_str().unwrap_or("").to_owned(),
                added: f["added"].as_u64().unwrap_or(0),
                removed: f["removed"].as_u64().unwrap_or(0),
                binary: f["binary"] == true,
            })
            .collect();
        Self {
            key,
            id,
            info,
            selected: None,
            listed: Vec::new(),
            files,
            diff: Diff::new(diff, false),
            head: Vec::new(),
            head_width: 0,
        }
    }

    fn diff_rows(&self) -> u32 {
        self.diff.len()
    }

    /// 差分の前に置く行の数: 頭、ファイルの一覧、空行。
    fn before_diff(&self) -> u32 {
        let files = self.listed.len() as u32;
        self.head.len() as u32 + files + u32::from(files > 0)
    }

    fn lines(&self) -> u32 {
        self.before_diff() + self.diff_rows()
    }

    /// 頭の部分を幅 `width` で組む（幅が変わったときだけ）。
    fn build_head(&mut self, width: u16, clock: &Clock) {
        if self.head_width == width && !self.head.is_empty() {
            return;
        }
        self.head_width = width;
        if let Some(selected) = &self.selected {
            self.head = self.selection_head(selected, width, clock);
            return;
        }
        let room = usize::from(width.max(1));
        let plain = Style::new();
        let dim = Style::new().fg(DIM);
        let mut head = Vec::new();
        let message = self.info["message"].as_str().unwrap_or("").trim_end();
        let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
        for line in wrap(subject, room) {
            head.push((line, plain.add_modifier(Modifier::BOLD)));
        }
        let body = body.trim_matches('\n');
        if !body.is_empty() {
            head.push((String::new(), plain));
            for paragraph in body.lines() {
                for line in wrap(paragraph, room) {
                    head.push((line, plain));
                }
            }
        }
        head.push((String::new(), plain));
        let person = |name: &str, email: &str| {
            let name = self.info[name].as_str().unwrap_or("");
            match self.info[email].as_str().filter(|e| !e.is_empty()) {
                Some(email) => format!("{name} <{email}>"),
                None => name.to_owned(),
            }
        };
        let when = |key: &str| {
            self.info[key].as_i64().map(|t| {
                format!(
                    "{} ({})",
                    (clock.local)(t),
                    tr!("{time} ago", time = ago(clock.now, t))
                )
            })
        };
        // 見出しと値。値は、一番長い見出しの 2 つ右でそろえる（セルの幅で）。
        let mut fields = Vec::new();
        let author = person("author", "authorEmail");
        fields.push((tr!("Author"), author.clone()));
        if let Some(date) = when("authorTime") {
            fields.push((tr!("Date"), date));
        }
        // コミッターは、作者と違うときだけ。
        let committer = person("committer", "committerEmail");
        if committer != author {
            fields.push((tr!("Committer"), committer));
            if let Some(date) = when("committerTime") {
                fields.push((tr!("Committed"), date));
            }
        }
        fields.push((tr!("Commit"), self.id.to_string()));
        let parents: Vec<&str> = self.info["parents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|p| &p[..p.len().min(8)])
            .collect();
        if !parents.is_empty() {
            fields.push((tr!("Parents"), parents.join(" ")));
        }
        // このコミットを指すブランチとタグ。
        let refs: Vec<&str> = self.info["refs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !refs.is_empty() {
            fields.push((tr!("Refs"), refs.join(", ")));
        }
        // 署名: 検証の結果（アプリと同じ言葉）と、署名した人。検証できなかったら、種類だけ。
        if let Some(kind) = self.info["signed"].as_str() {
            let check = &self.info["signatureCheck"];
            let value = match check["status"].as_str() {
                Some(status) => {
                    let label = signature_label(status, check, kind);
                    match check["signer"].as_str().filter(|s| !s.is_empty()) {
                        Some(signer) => format!("{label}, {signer}"),
                        None => label,
                    }
                }
                None => tr!("{kind} (not verified)", kind = kind.to_uppercase()),
            };
            fields.push((tr!("Signature"), value));
        }
        let column = fields.iter().map(|(l, _)| l.width()).max().unwrap_or(0) + 2;
        for (label, value) in fields {
            let pad = " ".repeat(column - label.width());
            head.push((format!("{label}{pad}{value}"), dim));
        }
        head.push((String::new(), plain));
        self.head = head;
    }
}

impl Commit {
    /// 複数選択の頭: 数・作者・期間・増減、差分の取り方、選んだコミットの一覧（アプリの上のペインと同じ）。
    fn selection_head(
        &self,
        selected: &Selected,
        width: u16,
        clock: &Clock,
    ) -> Vec<(String, Style)> {
        let room = usize::from(width.max(1));
        let plain = Style::new();
        let dim = Style::new().fg(DIM);
        let commits = &selected.commits;
        let mut authors: Vec<&str> = commits.iter().map(|c| c.author.as_str()).collect();
        authors.sort_unstable();
        authors.dedup();
        let (added, removed) = self
            .files
            .iter()
            .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
        let mut summary = vec![
            tr!("{count} commits", count = commits.len()),
            match authors.len() {
                1 => tr!("1 author"),
                n => tr!("{count} authors", count = n),
            },
        ];
        let newest = commits.iter().map(|c| c.time).max().unwrap_or(0);
        let oldest = commits.iter().map(|c| c.time).min().unwrap_or(0);
        if newest - oldest >= 60 {
            summary.push(tr!("over {span}", span = ago(newest, oldest)));
        }
        summary.push(format!("+{added} −{removed}"));
        let mut head: Vec<(String, Style)> = wrap(&summary.join(" · "), room)
            .into_iter()
            .map(|l| (l, plain.add_modifier(Modifier::BOLD)))
            .collect();
        let short = |key: &str| -> String {
            let id = selected.how[key].as_str().unwrap_or("");
            id[..id.len().min(8)].to_owned()
        };
        let how = match selected.how["kind"].as_str() {
            Some("range") => match selected.how["base"].as_str() {
                Some(_) => tr!(
                    "The changes of the {count} commits together, from {base} to {tip} (as if squashed)",
                    count = selected.how["count"].as_u64().unwrap_or(0),
                    base = short("base"),
                    tip = short("tip")
                ),
                None => tr!(
                    "The changes of the {count} commits together, up to {tip} (as if squashed)",
                    count = selected.how["count"].as_u64().unwrap_or(0),
                    tip = short("tip")
                ),
            },
            Some("pair") => tr!(
                "From {from} to {to}. x swaps the direction; . diffs from where they diverged",
                from = short("from"),
                to = short("to")
            ),
            _ => tr!("Each commit's changes, under the file and the commit"),
        };
        head.extend(wrap(&how, room).into_iter().map(|l| (l, dim)));
        head.push((String::new(), plain));
        for commit in commits {
            let id = commit.id.to_string();
            let line = format!(
                "{} {}  {}, {}",
                &id[..8],
                commit.subject,
                commit.author,
                tr!("{time} ago", time = ago(clock.now, commit.time))
            );
            head.push((crate::history::fit(&line, room).into_owned(), plain));
        }
        head.push((String::new(), plain));
        head
    }
}

/// 署名の検証の結果（git の `%G?`）の、行に付ける 1 文字の印と色（アプリの印の代わり）: 正しい署名は ✓、
/// 期限切れ・失効は !、正しくない署名は ✗、確かめられないものは ?。
pub fn signature_mark(status: &str) -> (char, Color) {
    match status {
        "G" => ('✓', Color::Green),
        "U" => ('✓', Color::Yellow),
        "X" | "Y" | "R" => ('!', Color::Yellow),
        "B" => ('✗', Color::Red),
        _ => ('?', DIM),
    }
}

/// 署名の検証の結果（git の `%G?`）を、アプリと同じ短い言葉に。正しい署名でも、鍵の信用度が足りなければ
/// Untrusted key、鍵のメールアドレスがどれもコミッターと違えば Unverified signer。
fn signature_label(status: &str, check: &Value, kind: &str) -> String {
    match status {
        "G" | "U" => {
            let trust = check["trust"].as_str().unwrap_or("").to_lowercase();
            let trusted = kind == "ssh"
                || matches!(trust.as_str(), "ultimate" | "full" | "fully" | "marginal");
            let committer = check["committerEmail"]
                .as_str()
                .unwrap_or("")
                .to_lowercase();
            let emails: Vec<String> = (check["emails"].as_array().into_iter().flatten())
                .filter_map(|e| e["email"].as_str().map(str::to_lowercase))
                .collect();
            if status == "U" || !trusted {
                tr!("Untrusted key")
            } else if !emails.is_empty() && !committer.is_empty() && !emails.contains(&committer) {
                tr!("Unverified signer")
            } else {
                tr!("Verified")
            }
        }
        "X" | "Y" => tr!("Expired"),
        "R" => tr!("Revoked key"),
        "B" => tr!("Bad signature"),
        "V" => tr!("Can’t verify"),
        _ => tr!("No key"),
    }
}

#[derive(Default)]
pub struct Details {
    /// 出したいもの。
    wanted: Option<Want>,
    /// 頼んで、答えを待っているものの鍵。
    loading: Option<String>,
    commit: Option<Commit>,
    /// 読めなかったものの鍵と、その理由。
    failed: Option<(String, String)>,
    scroll: Scroll,
    /// 前のフレームで組み立てた差分の行。
    kept: KeptRows,
    width: u16,
    /// `[` `]` で移ったファイル（その後にスクロールしたら忘れる）。文書の終わり近くでは、見出しを一番上まで
    /// 動かせないので、どのファイルへ移ったかを覚えておく。
    jumped: Option<usize>,
    /// 詳細の中で探した語と、合った行（画面に出すときに目立たせる）。
    found: Option<(String, u32)>,
    /// ファイルの一覧を木で出す（アプリの List / Tree。コミットを替えても覚えている）。
    tree: bool,
    /// 木でたたんだフォルダ（コミットを替えたら開く）。
    folded: HashSet<String>,
    /// 差分で無視する空白（コミットを替えても覚えている）。
    whitespace: Whitespace,
    /// ファイルごとの前後の行数（`u32::MAX` はファイル全体）。出すものを替えたら忘れる。
    contexts: std::collections::BTreeMap<usize, u32>,
    /// 前後の行数を替えて読み直したら、このファイルの見出しへ動かす。
    keep: Option<usize>,
}

/// `+` で広げる行数（アプリと同じ）。
const WIDEN_BY: u32 = 20;

impl Details {
    /// 頭・ファイルの一覧・差分の `line` 行目の字。
    fn line_text(commit: &Commit, line: u32) -> String {
        let head = commit.head.len() as u32;
        let before = commit.before_diff();
        if line < head {
            commit.head[line as usize].0.clone()
        } else if line < before {
            match commit.listed.get((line - head) as usize) {
                Some(Listed::File(index, _)) => commit.files[*index].path.clone(),
                Some(Listed::Folder { path, .. }) => path.clone(),
                None => String::new(),
            }
        } else {
            commit.diff.row_text((line - before) as usize).to_owned()
        }
    }

    /// 詳細の中で `query` を探す（大文字と小文字は区別しない）。前に合った行の次（`forward` でなければ前）から
    /// 巡って探し、合えばその行が見えるように動かす。合った数と、その何番目かを返す（なければ None）。
    pub fn find(&mut self, query: &str, forward: bool) -> Option<(usize, usize)> {
        let commit = self.commit.as_ref()?;
        let query = query.to_lowercase();
        let lines = commit.lines();
        let hits: Vec<u32> = (0..lines)
            .filter(|&l| Self::line_text(commit, l).to_lowercase().contains(&query))
            .collect();
        if hits.is_empty() {
            self.found = None;
            return None;
        }
        let at = match &self.found {
            Some((q, line)) if *q == query => match forward {
                true => hits.iter().position(|h| h > line).unwrap_or(0),
                false => hits
                    .iter()
                    .rposition(|h| h < line)
                    .unwrap_or(hits.len() - 1),
            },
            // 初めは、見えているところから下で。
            _ => hits.iter().position(|&h| h >= self.scroll.top).unwrap_or(0),
        };
        let line = hits[at];
        self.found = Some((query, line));
        self.jumped = None;
        self.scroll.reveal(line, lines);
        Some((at + 1, hits.len()))
    }

    /// 探した語を忘れる。
    pub fn clear_found(&mut self) {
        self.found = None;
    }

    /// 詳細の中で探している語。
    pub fn finding(&self) -> Option<&str> {
        self.found.as_ref().map(|(q, _)| q.as_str())
    }
    /// 出すものを替える。替わったら true。
    pub fn want(&mut self, want: Want) -> bool {
        if self.wanted.as_ref().is_some_and(|w| w.key() == want.key()) {
            return false;
        }
        self.wanted = Some(want);
        self.found = None;
        self.contexts.clear();
        self.keep = None;
        true
    }

    /// 頼むものの鍵: 出したいものと、無視する空白。
    fn key_of(&self, wanted: &Want) -> String {
        let mut key = wanted.key();
        if self.whitespace != Whitespace::Exact {
            key += &format!(" ws:{}", self.whitespace.as_str());
        }
        if !self.contexts.is_empty() {
            key += &format!(" ctx:{:?}", self.contexts);
        }
        key
    }

    /// `commitDiff` の contexts。
    fn contexts_json(&self) -> Value {
        self.contexts
            .iter()
            .map(|(file, &lines)| {
                let lines = match lines {
                    u32::MAX => Value::from("all"),
                    n => Value::from(n),
                };
                (file.to_string(), lines)
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    }

    /// 今見ているファイルの前後を広げる（アプリの hunk の見出しの印）。ファイル全体を出していれば何もしない。
    pub fn widen(&mut self) -> bool {
        let Some(index) = self.current_index() else {
            return false;
        };
        let lines = self.contexts.get(&index).copied().unwrap_or(3);
        if lines == u32::MAX {
            return false;
        }
        self.contexts.insert(index, lines + WIDEN_BY);
        self.keep = Some(index);
        true
    }

    /// 今見ているファイルの全体を出す・戻す（アプリの Show Whole File）。出すなら true。
    pub fn toggle_whole_file(&mut self) -> Option<bool> {
        let index = self.current_index()?;
        let whole = self.contexts.get(&index) != Some(&u32::MAX);
        if whole {
            self.contexts.insert(index, u32::MAX);
        } else {
            self.contexts.remove(&index);
        }
        self.keep = Some(index);
        Some(whole)
    }

    /// 無視する空白を次のもの（なし → 行末 → 量 → すべて → なし）にして、差分を読み直す。
    pub fn next_whitespace(&mut self) -> Whitespace {
        self.whitespace = self.whitespace.next();
        self.whitespace
    }

    /// 頼むことがあれば、その中身（答えを待っている間は、次を頼まない）。
    pub fn request(&mut self) -> Option<Request> {
        let wanted = self.wanted.as_ref()?;
        let key = self.key_of(wanted);
        let whitespace = self.whitespace.as_str();
        if self.loading.is_some()
            || self.commit.as_ref().is_some_and(|c| c.key == key)
            || self.failed.as_ref().is_some_and(|f| f.0 == key)
        {
            return None;
        }
        self.loading = Some(key.clone());
        Some(match wanted {
            Want::Commit { id, stash } => Request::Commit {
                id: id.to_string(),
                stash: *stash,
                key,
                whitespace,
                contexts: self.contexts_json(),
            },
            Want::Selection {
                history,
                rows,
                pair,
                ..
            } => Request::Selection {
                key,
                history: history.clone(),
                rows: rows.clone(),
                pair: pair.as_str(),
                whitespace,
                contexts: self.contexts_json(),
            },
        })
    }

    /// 詳細と差分が届いた。出したいコミットのものなら入れ替え、構文の色を頼む中身を返す。
    pub fn loaded(
        &mut self,
        id: &str,
        result: std::result::Result<(Value, Reply), String>,
        clock: &Clock,
    ) -> Option<Request> {
        self.loading = None;
        // 待つ間に選択が動いていたら捨てる（`request` が次を頼む）。
        let wanted = self.wanted.as_ref().filter(|w| self.key_of(w) == id)?;
        match result {
            Ok((info, diff)) => {
                let view = diff.json["view"].clone();
                let (oid, selected) = match wanted {
                    Want::Commit { id, .. } => (*id, None),
                    Want::Selection { commits, .. } => {
                        let how = diff.json["selection"].clone();
                        // Blame・File History は、新しい方の端で開く。
                        let tip = ["tip", "to"]
                            .iter()
                            .find_map(|k| how[k].as_str().and_then(Oid::from_hex))
                            .or(commits.first().map(|c| c.id))?;
                        let commits = commits.clone();
                        (tip, Some(Selected { how, commits }))
                    }
                };
                // 行数が決まるよう、頭の部分は届いたときに組む。
                let mut commit = Commit::new(id.to_owned(), oid, info, &diff);
                commit.selected = selected;
                self.folded.clear();
                commit.listed = list_rows(&commit.files, self.tree, &self.folded);
                commit.build_head(self.width, clock);
                self.commit = Some(commit);
                self.failed = None;
                self.scroll.top = 0;
                self.scroll.left = 0;
                self.jumped = None;
                // 前後の行数を替えたファイルの見出しへ。
                if let Some(index) = self.keep.take() {
                    let commit = self.commit.as_ref().expect("just set");
                    if let Some(row) = commit.diff.file_rows.get(index) {
                        let line = commit.before_diff() + row;
                        self.scroll.clamp(commit.lines());
                        self.scroll_to(line);
                        self.jumped = Some(index);
                    }
                }
                (!view.is_null()).then(|| Request::Syntax {
                    id: id.to_owned(),
                    of: serde_json::json!({"view": view}),
                })
            }
            Err(error) => {
                self.failed = Some((id.to_owned(), error));
                None
            }
        }
    }

    pub fn syntax(&mut self, id: &str, reply: Option<&Reply>) {
        let (Some(commit), Some(reply)) = (&mut self.commit, reply) else {
            return;
        };
        if commit.key == id {
            commit.diff.set_syntax(reply);
        }
    }

    /// 出す所の大きさ。幅が変わったら、頭の部分を組み直す。
    pub fn set_size(&mut self, width: u16, height: u16, clock: &Clock) {
        self.width = width;
        self.scroll.height = height;
        if let Some(commit) = &mut self.commit {
            commit.build_head(width, clock);
        }
        self.scroll.clamp(self.lines());
    }

    fn lines(&self) -> u32 {
        self.commit.as_ref().map_or(0, Commit::lines)
    }

    /// 今の位置（一番下に見えている行）と、全体の行数。
    pub fn position(&self) -> (u32, u32) {
        self.scroll.position(self.lines())
    }

    pub fn page(&self) -> i64 {
        self.scroll.page()
    }

    pub fn scroll(&mut self, delta: i64) -> bool {
        self.jumped = None;
        self.scroll.by(delta, self.lines())
    }

    /// 最後に描いてから、縦に何行ずれたか（正なら、中身が上へ）。
    pub fn scrolled(&self) -> i32 {
        self.scroll.scrolled()
    }

    /// 前のフレームで組み立てた行を捨てる。
    pub fn forget_drawn(&mut self) {
        self.kept.forget();
    }

    /// `down` の向きに、もうスクロールできない。
    pub fn at_end(&self, down: bool) -> bool {
        if down {
            self.scroll.position(self.lines()).0 >= self.lines()
        } else {
            self.scroll.top == 0
        }
    }

    pub fn scroll_to(&mut self, top: u32) -> bool {
        self.jumped = None;
        self.scroll.to(top, self.lines())
    }

    /// 差分を横に動かす。
    pub fn pan(&mut self, delta: i32) -> bool {
        self.scroll.pan(delta)
    }

    /// 差分を横に動かしてあるか。
    pub fn panned(&self) -> bool {
        self.scroll.left > 0
    }

    /// 今見ているファイルの番号（`current_file` のもの）。
    fn current_index(&self) -> Option<usize> {
        let commit = self.commit.as_ref()?;
        let before = commit.before_diff();
        let (top, bottom) = (
            self.scroll.top,
            self.scroll.top + u32::from(self.scroll.height),
        );
        let lines = commit.diff.file_rows.iter().map(|row| before + row);
        let index = self.jumped.unwrap_or_else(|| {
            lines
                .clone()
                .position(|line| top <= line && line < bottom)
                .unwrap_or_else(|| lines.filter(|&line| line < top).count().saturating_sub(1))
        });
        (index < commit.files.len()).then_some(index)
    }

    /// 今見ているファイルのパスと、出しているコミット。ファイルは、見えている最初の見出しのもの。見出しが
    /// 見えていなければ、上にある最後の見出しのもの（それもなければ最初のファイル）。
    pub fn current_file(&self) -> Option<(&str, Oid)> {
        let commit = self.commit.as_ref()?;
        let index = self.current_index()?;
        let path = commit.files.get(index)?.path.as_str();
        // 飛び飛びの選択では、見出しが「パス — 短い ID」なので、そのコミットのファイルとして開く。
        if let Some(selected) = &commit.selected
            && let Some((path, short)) = path.rsplit_once(" — ")
            && let Some(of) = selected
                .commits
                .iter()
                .find(|c| c.id.to_string().starts_with(short))
        {
            return Some((path, of.id));
        }
        Some((path, commit.id))
    }

    /// ファイルの一覧を、木と一覧で切り替える。
    pub fn toggle_tree(&mut self) -> bool {
        self.tree = !self.tree;
        self.folded.clear();
        self.relist();
        true
    }

    /// 木のフォルダを、すべてたたむ（たたんだものがあれば、すべて開く）。
    pub fn fold_all(&mut self) -> bool {
        let Some(commit) = &self.commit else {
            return false;
        };
        if !self.tree {
            return false;
        }
        if self.folded.is_empty() {
            // 一番上のフォルダだけをたたむ（下のものは開いたまま、開けば元の形）。
            self.folded = commit
                .listed
                .iter()
                .filter_map(|row| match row {
                    Listed::Folder { path, depth: 0, .. } => Some(path.clone()),
                    _ => None,
                })
                .collect();
            if self.folded.is_empty() {
                return false;
            }
        } else {
            self.folded.clear();
        }
        self.relist();
        true
    }

    fn relist(&mut self) {
        if let Some(commit) = &mut self.commit {
            commit.listed = list_rows(&commit.files, self.tree, &self.folded);
        }
        self.found = None;
        self.jumped = None;
        self.scroll.clamp(self.lines());
    }

    /// 詳細の `row` 行目（見えている所の）を押した: 一覧のフォルダならたたむか開き、ファイルならその差分へ。
    pub fn click(&mut self, row: u16) -> bool {
        let Some(commit) = &self.commit else {
            return false;
        };
        let line = self.scroll.top + u32::from(row);
        let head = commit.head.len() as u32;
        let Some(listed) = line
            .checked_sub(head)
            .and_then(|i| commit.listed.get(i as usize))
        else {
            return false;
        };
        match listed {
            Listed::Folder { path, .. } => {
                let path = path.clone();
                if !self.folded.remove(&path) {
                    self.folded.insert(path);
                }
                self.relist();
                true
            }
            Listed::File(index, _) => {
                let index = *index;
                let Some(start) = commit
                    .diff
                    .file_rows
                    .get(index)
                    .map(|r| commit.before_diff() + r)
                else {
                    return false;
                };
                self.scroll_to(start);
                self.jumped = Some(index);
                true
            }
        }
    }

    /// 次（`forward`）か前のファイルの見出しを一番上に。
    pub fn jump_file(&mut self, forward: bool) -> bool {
        let Some(commit) = &self.commit else {
            return false;
        };
        let before = commit.before_diff();
        // 今のファイル（覚えていればそれ、なければ一番上の行より上の最後の見出し）の、次か前。
        let starts: Vec<u32> = commit.diff.file_rows.iter().map(|r| before + r).collect();
        let current = self.jumped.map(|index| index as i64).unwrap_or_else(|| {
            starts
                .iter()
                .filter(|&&line| line <= self.scroll.top)
                .count() as i64
                - 1
        });
        let target = current + if forward { 1 } else { -1 };
        let Some(&line) = usize::try_from(target).ok().and_then(|t| starts.get(t)) else {
            return false;
        };
        self.scroll_to(line);
        self.jumped = Some(target as usize);
        true
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, clock: &Clock, theme: &Theme) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.scroll.height = area.height;
        let wanted = self.wanted.as_ref().map(|w| self.key_of(w));
        let Some(commit) = &mut self.commit else {
            let (text, style) = match &self.failed {
                Some((_, error)) => (error.clone(), Style::new().fg(Color::Red)),
                None if self.wanted.is_some() => (tr!("Loading…"), Style::new().fg(DIM)),
                None => return,
            };
            buf.set_stringn(area.x, area.y, text, usize::from(area.width), style);
            return;
        };
        commit.build_head(area.width, clock);
        let commit = &*commit;
        self.scroll.clamp(commit.lines());
        self.scroll.drawn();
        // 読めなかったコミットを選んでいる間は、理由を 1 行目に重ねる。
        let failed = self
            .failed
            .as_ref()
            .filter(|f| wanted.as_ref() == Some(&f.0))
            .map(|f| f.1.as_str());
        let head = commit.head.len() as u32;
        let before = commit.before_diff();
        for y in 0..area.height {
            let line = self.scroll.top + u32::from(y);
            if line >= commit.lines() {
                break;
            }
            let at = Rect::new(area.x, area.y + y, area.width, 1);
            if line < head {
                let (text, style) = &commit.head[line as usize];
                buf.set_stringn(at.x, at.y, text, usize::from(at.width), *style);
            } else if line < before {
                match commit.listed.get((line - head) as usize) {
                    Some(Listed::File(index, depth)) => {
                        let file = &commit.files[*index];
                        draw_file(at, buf, file, self.tree.then_some(*depth));
                    }
                    Some(Listed::Folder {
                        path,
                        depth,
                        folded,
                        files,
                    }) => draw_folder(at, buf, path, *depth, *folded, *files),
                    None => {}
                }
            } else {
                draw_row_kept(
                    &mut self.kept,
                    area.height,
                    at,
                    buf,
                    &commit.diff,
                    (line - before) as usize,
                    self.scroll.left,
                    theme,
                );
            }
        }
        // 探して合った行を目立たせる。
        if let Some((_, line)) = &self.found
            && *line >= self.scroll.top
            && *line < self.scroll.top + u32::from(area.height)
        {
            let at = Rect::new(
                area.x,
                area.y + (line - self.scroll.top) as u16,
                area.width,
                1,
            );
            buf.set_style(at, Style::new().bg(Color::Indexed(58)));
        }
        if let Some(error) = failed {
            let at = Rect::new(area.x, area.y, area.width, 1);
            buf.set_style(at, Style::new());
            buf.set_stringn(
                at.x,
                at.y,
                format!("{error:<width$}", width = usize::from(at.width)),
                usize::from(at.width),
                Style::new().fg(Color::Red),
            );
        }
    }
}

/// 木のフォルダの 1 行: 開いているか（▾ ▸）、名前、たたんでいれば下のファイルの数。
fn draw_folder(at: Rect, buf: &mut Buffer, path: &str, depth: u16, folded: bool, files: usize) {
    let name = path.rsplit('/').next().unwrap_or(path);
    let text = if folded {
        format!("{} {name}/ ({files})", '▸')
    } else {
        format!("{} {name}/", '▾')
    };
    let x = at.x + (depth * 2).min(at.width);
    buf.set_stringn(
        x,
        at.y,
        text,
        usize::from(at.right() - x),
        Style::new().fg(Color::Blue),
    );
}

/// ファイルの一覧の 1 行: 種類の 1 文字、パス、増えた行と減った行。木（`depth` が Some）では、深さだけ下げて
/// 名前だけを出す。
fn draw_file(at: Rect, buf: &mut Buffer, file: &File, depth: Option<u16>) {
    let indent = depth.map_or(0, |d| (d * 2).min(at.width));
    let at = Rect::new(at.x + indent, at.y, at.width - indent, 1);
    let (letter, color) = match file.kind.as_str() {
        "added" => ('A', Color::Green),
        "deleted" => ('D', Color::Red),
        "renamed" => ('R', Color::Cyan),
        "copied" => ('C', Color::Cyan),
        _ => ('M', Color::Yellow),
    };
    buf.set_stringn(
        at.x,
        at.y,
        letter.to_string(),
        usize::from(at.width),
        Style::new().fg(color),
    );
    let counts = counts(file.added, file.removed, file.binary);
    let counts_width = counts.iter().map(|(t, _)| t.width() + 1).sum::<usize>() as u16;
    let path = match (&file.old_path, depth) {
        (Some(old), _) if old != &file.path => format!("{old} → {}", file.path),
        (_, Some(_)) => file
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&file.path)
            .to_owned(),
        _ => file.path.clone(),
    };
    let room = at.width.saturating_sub(2 + counts_width);
    let end = buf
        .set_stringn(
            at.x + 2,
            at.y,
            fit_path(&path, usize::from(room)),
            usize::from(room),
            Style::new(),
        )
        .0;
    let mut x = end + 1;
    for (text, style) in counts {
        if x >= at.right() {
            break;
        }
        x = buf
            .set_stringn(x, at.y, text, usize::from(at.right() - x), style)
            .0
            + 1;
    }
}

/// `width` セルに収まるよう、空白で折り返す（空白のない長い語は途中で切る）。
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for word in text.split(' ') {
        let w = word.width();
        if used > 0 && used + 1 + w > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        if used > 0 {
            line.push(' ');
            used += 1;
        }
        if w <= width {
            line.push_str(word);
            used += w;
            continue;
        }
        for ch in word.chars() {
            let cw = ch.width().unwrap_or(0);
            if used + cw > width && used > 0 {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(ch);
            used += cw;
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 折り返しは空白で。空白のない長い語は途中で切る
    #[test]
    fn wrap_breaks_at_spaces() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefgh ij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("日本語の文", 6), ["日本語", "の文"]);
        assert_eq!(wrap("", 10), [""]);
    }
}
