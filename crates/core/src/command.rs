//! FE から受け取る操作。頻度の低い操作はまとめてここを通し、Undo の記録もここで行う。

use amonhen_model::{Oid, Outcome, Result, Root};
use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::rewrite::{self, ChainCommit, HistoryEdit};

/// 編集できる範囲を調べるときに辿るコミットの数。
const CHAIN_LIMIT: usize = 500;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    Checkout {
        branch: String,
    },
    /// リモートブランチ（`origin/main`）を追跡するローカルブランチを作って切り替える。
    Track {
        remote_branch: String,
    },
    /// コミットを detached HEAD でチェックアウトする。
    Detach {
        oid: String,
    },
    CreateBranch {
        name: String,
        start: String,
        checkout: bool,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    RenameBranch {
        from: String,
        to: String,
    },
    CreateTag {
        name: String,
        target: String,
        message: Option<String>,
    },
    /// 今のブランチに `rev` を merge する。
    Merge {
        rev: String,
    },
    /// ブランチ `target` に `rev` を merge する。`target` を checkout していなくても、fast-forward で
    /// 済むならブランチだけ進める。そうでなければ checkout してから merge する。
    MergeInto {
        target: String,
        rev: String,
    },
    /// 今のブランチを `onto` の上に rebase する。
    Rebase {
        onto: String,
    },
    CherryPick {
        oid: String,
        is_merge: bool,
    },
    Revert {
        oid: String,
        is_merge: bool,
    },
    Abort,
    Continue,
    Skip,
    Fetch,
    /// 今のブランチに上流を取り込む（Undo できる）。
    Pull,
    Push {
        remote: String,
        branch: String,
        remote_branch: String,
        force: bool,
        set_upstream: bool,
    },
    /// 作業ツリーのファイルを index の内容に戻す（Undo できる）。
    Discard {
        paths: Vec<String>,
    },
    StashPush {
        message: Option<String>,
        include_untracked: bool,
    },
    StashApply {
        name: String,
        pop: bool,
    },
    /// stash を消す（Undo できる）。
    StashDrop {
        name: String,
    },
    UpdateSubmodules {
        paths: Vec<String>,
    },
    Undo,
    /// 次に Undo される操作の名前。
    UndoPeek,
    /// コミットの編集（reword・squash・fixup・drop・edit・split・move）。
    Rewrite {
        label: String,
        edit: HistoryEdit,
    },
    /// コミットのメッセージ全文（`text`）。
    CommitMessage {
        oid: String,
    },
    /// 途中の操作（`text` に "rebase"・"merge"・"cherry-pick"・"revert"、なければなし）。
    Sequence,
    /// 編集できるコミット（HEAD から first-parent で最初の merge の手前まで、新しい順）。`text` に改行区切り。
    EditableCommits,
    /// `oid` を親にまとめるときの、メッセージの初期値（親のメッセージ ＋ `oid` のメッセージ）。
    SquashMessage {
        oid: String,
    },
    /// `path` の `index` 番目のコンフリクトを解決する（Undo できる）。
    ResolveConflict {
        path: String,
        index: usize,
        choice: crate::conflict::Choice,
    },
    /// コミットの詳細（`info`: message・author・authorEmail・authorTime・authorTz・committer・committerEmail・
    /// committerTime・committerTz・parents・refs・signed・mergetag・trailers）。すぐ返せる分だけ。
    CommitInfo {
        oid: String,
    },
    /// コミットの詳細のうち時間のかかる分（`info`: signature・notes・stats・children・branches・tags・tagCount・
    /// firstTag・previousTag）。裏で読む。
    CommitExtras {
        oid: String,
    },
    /// 署名の検証の結果（`info`: `statuses` に `oids` と同じ順の `%G?` の 1 文字）。一度検証したものは覚えている。
    VerifySignatures {
        oids: Vec<String>,
    },
    /// 名前や短い ID が指すコミット（`text`: 40 桁の ID）。メッセージの中の ID のリンクに使う。
    ResolveCommit {
        rev: String,
    },
    /// `path` を片側にして解決済みにする。
    TakeSide {
        path: String,
        ours: bool,
    },
    /// マージエディタの中身（`info`）。`current`・`incoming`・`base`（diff3 のときだけ）は、すべての
    /// コンフリクトをその側にした `text` と、各コンフリクトの行範囲 `regions`（`[開始, 終わり)`、0 始まり）。
    /// `result` は今の作業ツリーの内容（マーカーつき）。
    MergeSides {
        path: String,
    },
    /// `text` の `index` 番目のコンフリクトを解決した文章（`text`）。ファイルには書かない。
    ResolveText {
        text: String,
        index: usize,
        choice: crate::conflict::Choice,
    },
    /// `text` に残っているコンフリクト（`info.conflicts`）。それぞれ、マーカーを含む行範囲 `lines` と、
    /// 両側の中身 `current`・`incoming`（マージエディタの上の欄のどのコンフリクトかを突き合わせる）。
    ConflictRegions {
        text: String,
    },
    /// `path` の各行を最後に変えたコミット（`info`: `commits` と、各行の `c`（commits の添字）・`t`（内容））。
    /// `rev` がなければ作業ツリーの内容。
    Blame {
        path: String,
        rev: Option<String>,
    },
    /// HEAD から `path` を変えたコミット（`info.entries`: oid・subject・author・time・その時点の path）。
    FileHistory {
        path: String,
    },
    /// untracked のファイルを消す（git の記録にないので Undo では戻せない）。
    DeleteUntracked {
        paths: Vec<String>,
    },
    /// `path` に残っているコンフリクトの数（`info.count`）。
    ConflictCount {
        path: String,
    },
    /// マージエディタの結果を `path` に書く（Undo できる）。`stage` なら解決済みにする（index に入れる）。
    SaveMerge {
        path: String,
        text: String,
        stage: bool,
    },
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub ok: bool,
    /// "done"・"conflict"・"stopped"。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<&'static str>,
    /// Undo した・次に Undo される操作の名前。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn outcome_name(o: Outcome) -> &'static str {
    match o {
        Outcome::Done => "done",
        Outcome::Conflict => "conflict",
        Outcome::Stopped => "stopped",
    }
}

fn oid(hex: &str) -> Result<Oid> {
    Oid::from_hex(hex).ok_or_else(|| format!("コミット ID を読めない: {hex}").into())
}

fn heads(name: &str) -> String {
    format!("refs/heads/{name}")
}

impl Repository {
    /// コミットのメッセージ全文。
    pub fn commit_message(&self, commit: &Oid) -> Result<String> {
        let text = self
            .backend()
            .commit_texts(std::slice::from_ref(commit))?
            .remove(0);
        // ヘッダーの後の最初の空行から後ろがメッセージ。
        let raw = &text.raw[..];
        let start = raw
            .windows(2)
            .position(|w| w == b"\n\n")
            .map_or(raw.len(), |i| i + 2);
        Ok(String::from_utf8_lossy(&raw[start..]).into_owned())
    }

    /// コミットの詳細。ヘッダーの行（parent・author・committer）とメッセージ、そのコミットを指す ref。
    pub fn commit_info(&self, commit: &Oid) -> Result<serde_json::Value> {
        let text = self
            .backend()
            .commit_texts(std::slice::from_ref(commit))?
            .remove(0);
        let raw = String::from_utf8_lossy(&text.raw).into_owned();
        let (header, message) = raw.split_once("\n\n").unwrap_or((&raw, ""));
        // "Name <email> 1700000000 +0900" を名前・メール・時刻・タイムゾーンに分ける。
        let signature = |line: &str| -> (String, String, i64, String) {
            let (name, rest) = line.split_once(" <").unwrap_or((line, ""));
            let (email, rest) = rest.split_once("> ").unwrap_or((rest, ""));
            let mut parts = rest.split(' ');
            let time = parts.next().and_then(|t| t.parse().ok()).unwrap_or(0);
            let tz = parts.next().unwrap_or_default().to_owned();
            (name.to_owned(), email.to_owned(), time, tz)
        };
        let mut parents = Vec::new();
        let (mut author, mut committer) = (None, None);
        // 署名の種類（gpgsig の最初の行で分かる）と、署名付きのタグを merge したか。
        let (mut signed, mut mergetag) = (None, false);
        for line in header.lines() {
            if let Some(p) = line.strip_prefix("parent ") {
                parents.push(p.to_owned());
            } else if let Some(a) = line.strip_prefix("author ") {
                author = Some(signature(a));
            } else if let Some(c) = line.strip_prefix("committer ") {
                committer = Some(signature(c));
            } else if let Some(sig) = line
                .strip_prefix("gpgsig ")
                .or_else(|| line.strip_prefix("gpgsig-sha256 "))
            {
                signed = Some(if sig.contains("SSH SIGNATURE") {
                    "ssh"
                } else if sig.contains("SIGNED MESSAGE") {
                    "x509"
                } else {
                    "gpg"
                });
            } else if line.starts_with("mergetag ") {
                mergetag = true;
            }
        }
        let merged_tags: Vec<serde_json::Value> = merged_tags(header)
            .into_iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "tagger": t.tagger,
                    "subject": t.subject,
                    "signed": t.signed,
                })
            })
            .collect();
        let (author, email, author_time, author_tz) = author.unwrap_or_default();
        let (committer, committer_email, committer_time, committer_tz) =
            committer.unwrap_or_default();
        let trailers: Vec<serde_json::Value> = trailers(message)
            .into_iter()
            .map(|(key, value)| serde_json::json!({ "key": key, "value": value }))
            .collect();
        // ref はディスクから読み直さず、最後に読んだものを使う（linux では読み直すと 15〜30 ms かかる）。
        let refs: Vec<String> = self
            .refs()?
            .refs
            .entries
            .iter()
            .filter(|e| {
                e.target.as_ref() == Some(commit) && e.kind != amonhen_model::RefKind::Stash
            })
            .map(|e| e.name.clone())
            .collect();
        Ok(serde_json::json!({
            "oid": commit.to_string(),
            "message": message,
            "author": author,
            "authorEmail": email,
            "authorTime": author_time,
            "authorTz": author_tz,
            "committer": committer,
            "committerEmail": committer_email,
            "committerTime": committer_time,
            "committerTz": committer_tz,
            "parents": parents,
            "refs": refs,
            "signed": signed,
            "mergetag": mergetag,
            "mergedTags": merged_tags,
            "trailers": trailers,
        }))
    }

    /// コミットの詳細のうち時間のかかる分。署名の検証・notes・行数は git に頼み、子コミット・含むブランチと
    /// タグ・前後のタグは History のグラフから求める（History をまだ読んでいなければ空）。
    pub fn commit_extras(&self, commit: &Oid) -> Result<serde_json::Value> {
        const MAX_MERGED: usize = 50;
        let generation = self.signature_generation();
        let mut extras = self.backend().commit_extras(commit)?;
        // 手元で検証するなら、署名の結果だけは手元の鍵で求め直す（notes と行数はリポジトリ側のまま）。
        let generation = match self.local_signature_details(commit) {
            Some(result) => {
                let (g, signature) = result?;
                extras.signature = signature;
                g
            }
            None => generation,
        };
        if let Some(s) = &extras.signature {
            self.remember_signatures(generation, [(*commit, s.status)]);
        }
        let mut children = Vec::new();
        let (mut branches, mut tags) = (Vec::new(), Vec::new());
        let (mut local_branches, mut remote_branches) = (Vec::new(), Vec::new());
        let (mut first_tag, mut previous_tag) = (None, None);
        let (mut merged_rows, mut head_distance) = (Vec::new(), None);
        let mut head_rev: Option<String> = None;
        let mut summaries: Vec<Oid> = Vec::new();
        if let Some(history) = self.history()
            && let Some(row) = history.row_of(commit)
        {
            let graph = &history.graph;
            let (contains, child_rows) = crate::lineage::descendants(graph, row);
            children = child_rows
                .iter()
                .map(|&r| graph.ids()[r as usize].to_string())
                .collect();
            let refs = self.refs()?;
            let row_of = |e: &amonhen_model::RefEntry| e.target.and_then(|t| history.row_of(&t));
            let in_set = |r: u32| contains[r as usize / 64] & (1 << (r % 64)) != 0;
            // 含むタグは、古い（行が下の）ものから。いちばん古いものが「最初に含んだリリース」。
            let mut tag_rows = Vec::new();
            for e in &refs.refs.entries {
                let Some(r) = row_of(e) else { continue };
                match e.kind {
                    amonhen_model::RefKind::Local if in_set(r) => {
                        branches.push(e.name.clone());
                        local_branches.push(e.name.clone());
                    }
                    amonhen_model::RefKind::Remote if in_set(r) => {
                        branches.push(e.name.clone());
                        remote_branches.push(e.name.clone());
                    }
                    amonhen_model::RefKind::Tag if in_set(r) => tag_rows.push((r, e.name.clone())),
                    _ => {}
                }
            }
            tag_rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            first_tag = tag_rows.first().map(|(_, n)| n.clone());
            tags = tag_rows.into_iter().map(|(_, n)| n).collect();
            // このコミットより前（祖先）で、いちばん近いタグ。
            let mut tagged: std::collections::HashMap<u32, &str> = std::collections::HashMap::new();
            for e in &refs.refs.entries {
                if e.kind == amonhen_model::RefKind::Tag
                    && let Some(r) = row_of(e)
                {
                    tagged.entry(r).or_insert(&e.name);
                }
            }
            previous_tag = crate::lineage::nearest_ancestor(graph, row, |r| {
                r != row && tagged.contains_key(&r)
            })
            .map(|r| tagged[&r].to_owned());
            // HEAD が含むなら、HEAD から何コミット前か（HEAD の祖先のうち、このコミットを含むものの数 - 1）。
            if let Some(head) = refs.refs.head.and_then(|h| history.row_of(&h))
                && in_set(head)
            {
                let ancestors = crate::lineage::ancestors(graph, head);
                let both: u32 = ancestors
                    .iter()
                    .zip(&contains)
                    .map(|(a, c)| (a & c).count_ones())
                    .sum();
                head_distance = Some(both.saturating_sub(1));
                head_rev =
                    crate::lineage::rev_path(graph, head, row).map(|path| format!("HEAD{path}"));
            }
            merged_rows = crate::lineage::merged_commits(graph, row);
            summaries = merged_rows
                .iter()
                .take(MAX_MERGED)
                .map(|&r| graph.ids()[r as usize])
                .collect();
        }
        // メッセージが参照するコミット（cherry-pick・revert の元、Fixes: など）。件名を添える。
        let references = self.message_references(commit)?;
        summaries.extend(references.iter().map(|(_, id)| *id));
        let texts = if summaries.is_empty() {
            Vec::new()
        } else {
            self.backend().commit_summaries(&summaries)?
        };
        let summary = |i: usize| {
            let t = &texts[i];
            (
                String::from_utf8_lossy(t.subject()).into_owned(),
                String::from_utf8_lossy(t.author_name()).into_owned(),
            )
        };
        let shown_merged = summaries.len() - references.len();
        let merged: Vec<serde_json::Value> = (0..shown_merged)
            .map(|i| {
                let (subject, author) = summary(i);
                serde_json::json!({ "oid": summaries[i].to_string(), "subject": subject, "author": author })
            })
            .collect();
        let references: serde_json::Map<String, serde_json::Value> = references
            .iter()
            .enumerate()
            .map(|(i, (token, id))| {
                let (subject, _) = summary(shown_merged + i);
                (
                    token.clone(),
                    serde_json::json!({ "oid": id.to_string(), "subject": subject }),
                )
            })
            .collect();
        let signature = extras.signature.map(|s| {
            serde_json::json!({
                "status": s.status.to_string(),
                "signer": s.signer,
                "key": s.key,
                "fingerprint": s.fingerprint,
                "trust": s.trust,
            })
        });
        let stats: Vec<serde_json::Value> = extras
            .stats
            .iter()
            .map(|s| serde_json::json!({ "path": s.path, "added": s.added, "removed": s.removed }))
            .collect();
        // タグは多いことがある（linux の古いコミットは 900 以上）ので、数と古い順の先頭だけを返す。
        const MAX_TAGS: usize = 50;
        Ok(serde_json::json!({
            "signature": signature,
            "notes": extras.notes,
            "stats": stats,
            "children": children,
            "branches": branches,
            "tagCount": tags.len(),
            "tags": tags.into_iter().take(MAX_TAGS).collect::<Vec<_>>(),
            "firstTag": first_tag,
            "previousTag": previous_tag,
            "originUrl": self.origin_url(),
            "localBranches": local_branches,
            "remoteBranches": remote_branches,
            "headDistance": head_distance,
            "headRev": head_rev,
            "mergedCount": merged_rows.len(),
            "merged": merged,
            "references": references,
        }))
    }

    /// メッセージに出てくるコミット ID（7〜40 桁の 16 進）のうち、引けたもの（書かれていた文字列と ID）。
    /// 多すぎるメッセージは先頭の 10 個まで。
    fn message_references(&self, commit: &Oid) -> Result<Vec<(String, Oid)>> {
        let message = self.commit_message(commit)?;
        let mut tokens: Vec<&str> = Vec::new();
        for word in message.split(|c: char| !c.is_ascii_alphanumeric()) {
            let hex = word.len() >= 7
                && word.len() <= 40
                && word.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                // 数字だけの語（日付・番号）は ID とみなさない。
                && word.bytes().any(|b| b.is_ascii_alphabetic());
            if hex && !tokens.contains(&word) && word != commit.to_string() {
                tokens.push(word);
            }
            if tokens.len() == 10 {
                break;
            }
        }
        let history = self.history();
        let ops = self.backend().operations()?;
        let mut out = Vec::new();
        for token in tokens {
            let full = Oid::from_hex(token)
                .filter(|id| history.as_ref().is_some_and(|h| h.row_of(id).is_some()));
            let id = match full {
                Some(id) => Some(id),
                None => ops.resolve(&format!("{token}^{{commit}}")).ok(),
            };
            if let Some(id) = id {
                out.push((token.to_owned(), id));
            }
        }
        Ok(out)
    }

    /// `remote.origin.url`（メッセージの `#123` をリンクにするため）。読めなければ `None`。
    fn origin_url(&self) -> Option<String> {
        let config = self.backend().files().read(Root::GitDir, "config").ok()??;
        origin_url(&String::from_utf8_lossy(&config))
    }

    /// HEAD から first-parent で辿り、最初の merge の手前まで（新しい順）。
    fn editable_chain(&self) -> Result<Vec<ChainCommit>> {
        let history = match self.history() {
            Some(h) => h,
            None => self.load_history()?,
        };
        let head = self.backend().head()?.ok_or("まだコミットがありません")?;
        let mut row = history
            .row_of(&head)
            .ok_or("HEAD が History にない（読み直してください）")? as usize;
        let graph = &history.graph;
        let mut chain = Vec::new();
        while chain.len() < CHAIN_LIMIT {
            let parents = graph.parents(row);
            if parents.len() > 1 {
                break;
            }
            let parent = parents.first().map(|&p| graph.ids()[p as usize]);
            chain.push(ChainCommit {
                oid: graph.ids()[row],
                parent,
            });
            match parents.first() {
                Some(&p) => row = p as usize,
                None => break,
            }
        }
        Ok(chain)
    }

    /// コミットを編集する。非対話の rebase で行い、Undo できるように記録する。
    pub fn rewrite(&self, label: &str, edit: &HistoryEdit) -> Result<Outcome> {
        let ops = self.backend().operations()?;
        if ops.sequence_in_progress()?.is_some() {
            return Err("merge や rebase の途中です。先に終わらせるか中止してください".into());
        }
        // 書き換える前に、最新の History で範囲を決める。
        self.load_history()?;
        let chain = self.editable_chain()?;
        // メッセージと todo は `.git/amonhen` に書く。git に渡すのはリポジトリ側の絶対パス。
        let dir = self.backend().paths().git_dir.join("amonhen");
        let plan = rewrite::plan_rebase(&chain, edit, &dir)?;
        let files = self.backend().files();
        for (name, message) in &plan.messages {
            files.write(Root::GitDir, &format!("amonhen/{name}"), message.as_bytes())?;
        }
        files.write(Root::GitDir, "amonhen/rebase-todo", plan.todo.as_bytes())?;
        let todo = dir.join("rebase-todo");
        self.undo_log()?.record_stoppable(
            ops,
            label,
            || ops.rebase_todo(plan.base.as_ref(), &todo),
            |o| *o == Outcome::Conflict,
            |o| *o == Outcome::Stopped,
        )
    }

    /// JSON の命令を実行し、JSON の結果を返す。
    pub fn run_json(&self, json: &str) -> String {
        let response = match serde_json::from_str::<Command>(json) {
            Ok(command) => match self.run(command) {
                Ok(r) => r,
                Err(e) => Response {
                    error: Some(e.to_string()),
                    ..Default::default()
                },
            },
            Err(e) => Response {
                error: Some(format!("命令を読めない: {e}")),
                ..Default::default()
            },
        };
        serde_json::to_string(&response).unwrap_or_default()
    }

    pub fn run(&self, command: Command) -> Result<Response> {
        let ops = self.backend().operations()?;
        let undo = self.undo_log()?;
        let not_done = |o: &Outcome| *o != Outcome::Done;
        let done = |outcome: Outcome| {
            Ok(Response {
                ok: true,
                outcome: Some(outcome_name(outcome)),
                ..Default::default()
            })
        };
        let ok = || done(Outcome::Done);
        match command {
            Command::Checkout { branch } => {
                ops.switch_branch(&branch)?;
                ok()
            }
            Command::Track { remote_branch } => {
                ops.track_branch(&remote_branch)?;
                ok()
            }
            Command::Detach { oid: o } => {
                ops.switch_to(&amonhen_model::HeadState {
                    branch: None,
                    oid: Some(oid(&o)?),
                })?;
                ok()
            }
            Command::CreateBranch {
                name,
                start,
                checkout,
            } => {
                undo.record(
                    ops,
                    &format!("Create branch {name}"),
                    &[heads(&name)],
                    false,
                    || ops.create_branch(&name, &start, checkout),
                    |_| false,
                )?;
                ok()
            }
            Command::DeleteBranch { name, force } => {
                undo.record(
                    ops,
                    &format!("Delete branch {name}"),
                    &[heads(&name)],
                    false,
                    || ops.delete_branch(&name, force),
                    |_| false,
                )?;
                ok()
            }
            Command::RenameBranch { from, to } => {
                undo.record(
                    ops,
                    &format!("Rename {from} to {to}"),
                    &[heads(&from), heads(&to)],
                    false,
                    || ops.rename_branch(&from, &to),
                    |_| false,
                )?;
                ok()
            }
            Command::CreateTag {
                name,
                target,
                message,
            } => {
                undo.record(
                    ops,
                    &format!("Create tag {name}"),
                    &[format!("refs/tags/{name}")],
                    false,
                    || ops.create_tag(&name, &target, message.as_deref()),
                    |_| false,
                )?;
                ok()
            }
            // コンフリクトや途中で止まったときは記録しない（戻すのは Abort の役目）。
            Command::Merge { rev } => done(undo.record(
                ops,
                &format!("Merge {rev}"),
                &[],
                false,
                || ops.merge(&rev),
                not_done,
            )?),
            Command::MergeInto { target, rev } => {
                let current = ops.head_state()?.branch;
                let full = heads(&target);
                if current.as_deref() != Some(full.as_str()) && ops.is_ancestor(&full, &rev)? {
                    let from = ops.resolve(&full)?;
                    let to = ops.resolve(&rev)?;
                    undo.record(
                        ops,
                        &format!("Fast-forward {target} to {rev}"),
                        std::slice::from_ref(&full),
                        false,
                        || ops.update_ref(&full, Some(&to), Some(&from)),
                        |_| false,
                    )?;
                    return ok();
                }
                if current.as_deref() != Some(full.as_str()) {
                    ops.switch_branch(&target)?;
                }
                done(undo.record(
                    ops,
                    &format!("Merge {rev} into {target}"),
                    &[],
                    false,
                    || ops.merge(&rev),
                    not_done,
                )?)
            }
            Command::Rebase { onto } => done(undo.record(
                ops,
                &format!("Rebase onto {onto}"),
                &[],
                false,
                || ops.rebase(&onto),
                not_done,
            )?),
            Command::CherryPick { oid: o, is_merge } => {
                let id = oid(&o)?;
                done(undo.record(
                    ops,
                    &format!("Cherry-pick {}", &o[..o.len().min(8)]),
                    &[],
                    false,
                    || ops.cherry_pick(&id, is_merge),
                    not_done,
                )?)
            }
            Command::Revert { oid: o, is_merge } => {
                let id = oid(&o)?;
                done(undo.record(
                    ops,
                    &format!("Revert {}", &o[..o.len().min(8)]),
                    &[],
                    false,
                    || ops.revert(&id, is_merge),
                    not_done,
                )?)
            }
            Command::Abort => {
                let sequence = ops.sequence_in_progress()?.ok_or("途中の操作がない")?;
                ops.abort(sequence)?;
                undo.settle_pending(ops)?;
                ok()
            }
            Command::Continue => {
                let sequence = ops.sequence_in_progress()?.ok_or("途中の操作がない")?;
                let outcome = ops.continue_sequence(sequence)?;
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::Skip => {
                let sequence = ops.sequence_in_progress()?.ok_or("途中の操作がない")?;
                let outcome = ops.skip_sequence(sequence)?;
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::Fetch => {
                ops.fetch()?;
                ok()
            }
            Command::Pull => done(undo.record(ops, "Pull", &[], false, || ops.pull(), not_done)?),
            Command::Push {
                remote,
                branch,
                remote_branch,
                force,
                set_upstream,
            } => {
                ops.push(&remote, &branch, &remote_branch, force, set_upstream)?;
                ok()
            }
            Command::Discard { paths } => {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                let label = match paths.as_slice() {
                    [one] => format!("Discard {one}"),
                    many => format!("Discard {} files", many.len()),
                };
                undo.record_files(ops, &label, &paths, || ops.restore_worktree(&paths))?;
                ok()
            }
            Command::StashPush {
                message,
                include_untracked,
            } => {
                ops.stash_push(message.as_deref(), include_untracked)?;
                ok()
            }
            Command::StashApply { name, pop } => done(ops.stash_apply(&name, pop)?),
            Command::StashDrop { name } => {
                let refs = self.load_refs()?;
                let entry = refs
                    .refs
                    .entries
                    .iter()
                    .find(|e| e.kind == amonhen_model::RefKind::Stash && e.name == name)
                    .ok_or_else(|| format!("{name} がない"))?;
                let target = entry.target.ok_or("stash のコミットがない")?;
                let message = entry.detail.clone().unwrap_or_else(|| name.clone());
                undo.record_stash_drop(ops, &name, &target, &message)?;
                ok()
            }
            Command::UpdateSubmodules { paths } => {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                ops.update_submodules(&paths)?;
                ok()
            }
            Command::Undo => {
                let label = undo.undo(ops)?;
                Ok(Response {
                    ok: true,
                    label,
                    ..Default::default()
                })
            }
            Command::UndoPeek => {
                // 外で rebase を終わらせた場合も、ここで確定させる。
                undo.settle_pending(ops)?;
                Ok(Response {
                    ok: true,
                    label: undo.peek(),
                    ..Default::default()
                })
            }
            Command::Rewrite { label, edit } => done(self.rewrite(&label, &edit)?),
            Command::CommitMessage { oid: o } => {
                // "HEAD" なら今のコミット（Amend の初期値）。
                let id = if o == "HEAD" {
                    self.backend().head()?.ok_or("まだコミットがありません")?
                } else {
                    oid(&o)?
                };
                Ok(Response {
                    ok: true,
                    text: Some(self.commit_message(&id)?),
                    ..Default::default()
                })
            }
            Command::EditableCommits => {
                let chain = self.editable_chain()?;
                let text = chain
                    .iter()
                    .map(|c| c.oid.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(Response {
                    ok: true,
                    text: Some(text),
                    ..Default::default()
                })
            }
            Command::SquashMessage { oid: o } => {
                let id = oid(&o)?;
                let chain = self.editable_chain()?;
                let parent = chain
                    .iter()
                    .find(|c| c.oid == id)
                    .and_then(|c| c.parent)
                    .ok_or("親が編集できる範囲にないので、まとめられません")?;
                let text = format!(
                    "{}\n\n{}",
                    self.commit_message(&parent)?.trim_end(),
                    self.commit_message(&id)?.trim_end()
                );
                Ok(Response {
                    ok: true,
                    text: Some(text),
                    ..Default::default()
                })
            }
            Command::ResolveConflict {
                path,
                index,
                choice,
            } => {
                let files = self.backend().files();
                let data = files.read(Root::Workdir, &path)?.ok_or("ファイルがない")?;
                let resolved = crate::conflict::resolve(&data, index, choice)
                    .ok_or("そのコンフリクトはもうない")?;
                undo.record_files(
                    ops,
                    &format!("Resolve conflict in {path}"),
                    &[&path],
                    || files.write(Root::Workdir, &path, &resolved),
                )?;
                ok()
            }
            Command::CommitInfo { oid: o } => Ok(Response {
                ok: true,
                info: Some(self.commit_info(&oid(&o)?)?),
                ..Default::default()
            }),
            Command::VerifySignatures { oids } => {
                let ids = oids.iter().map(|o| oid(o)).collect::<Result<Vec<_>>>()?;
                let statuses: Vec<String> = self
                    .verify_signatures(&ids)?
                    .iter()
                    .map(char::to_string)
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "statuses": statuses })),
                    ..Default::default()
                })
            }
            Command::ResolveCommit { rev } => Ok(Response {
                ok: true,
                text: Some(ops.resolve(&format!("{rev}^{{commit}}"))?.to_string()),
                ..Default::default()
            }),
            Command::CommitExtras { oid: o } => Ok(Response {
                ok: true,
                info: Some(self.commit_extras(&oid(&o)?)?),
                ..Default::default()
            }),
            Command::TakeSide { path, ours } => {
                ops.take_side(&path, ours)?;
                ok()
            }
            Command::MergeSides { path } => {
                let data = self
                    .backend()
                    .files()
                    .read(Root::Workdir, &path)?
                    .ok_or("ファイルがない")?;
                let side = |choice| {
                    crate::conflict::side(&data, choice).map(|s| {
                        serde_json::json!({
                            "text": String::from_utf8_lossy(&s.text),
                            "regions": s.regions.iter().map(|r| [r.start, r.end]).collect::<Vec<_>>(),
                        })
                    })
                };
                use crate::conflict::Choice;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "current": side(Choice::Ours),
                        "incoming": side(Choice::Theirs),
                        "base": side(Choice::Base),
                        "result": String::from_utf8_lossy(&data),
                    })),
                    ..Default::default()
                })
            }
            Command::ResolveText {
                text,
                index,
                choice,
            } => {
                let resolved = crate::conflict::resolve(text.as_bytes(), index, choice)
                    .ok_or("そのコンフリクトはもうない")?;
                Ok(Response {
                    ok: true,
                    text: Some(String::from_utf8_lossy(&resolved).into_owned()),
                    ..Default::default()
                })
            }
            Command::ConflictRegions { text } => {
                let data = text.as_bytes();
                let ls = crate::conflict::line_ranges(data);
                let join = |r: &std::ops::Range<usize>| -> String {
                    r.clone()
                        .map(|i| String::from_utf8_lossy(&data[ls[i].clone()]))
                        .collect()
                };
                let conflicts: Vec<_> = crate::conflict::parse(data)
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "lines": [c.lines.start, c.lines.end],
                            "current": join(&c.ours),
                            "incoming": join(&c.theirs),
                        })
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "conflicts": conflicts })),
                    ..Default::default()
                })
            }
            Command::Blame { path, rev } => {
                let rev = rev.map(|r| oid(&r)).transpose()?;
                let blame = crate::blame::parse_blame(&ops.blame(&path, rev.as_ref())?);
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::to_value(blame)?),
                    ..Default::default()
                })
            }
            Command::FileHistory { path } => {
                let entries = crate::blame::parse_file_log(&ops.file_log(&path)?);
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "entries": entries })),
                    ..Default::default()
                })
            }
            Command::DeleteUntracked { paths } => {
                let files = self.backend().files();
                for path in &paths {
                    files.remove(Root::Workdir, path)?;
                }
                ok()
            }
            Command::ConflictCount { path } => {
                let data = self
                    .backend()
                    .files()
                    .read(Root::Workdir, &path)?
                    .unwrap_or_default();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "count": crate::conflict::parse(&data).len() })),
                    ..Default::default()
                })
            }
            Command::SaveMerge { path, text, stage } => {
                let files = self.backend().files();
                undo.record_files(ops, &format!("Merge {path}"), &[&path], || {
                    files.write(Root::Workdir, &path, text.as_bytes())
                })?;
                if stage {
                    self.stage(&[&path])?;
                }
                ok()
            }
            Command::Sequence => {
                // rebase なら何番目かも添える（"rebase (2/5)"）。
                let text = match ops.sequence_in_progress()? {
                    Some(amonhen_model::Sequence::Rebase) => Some(match ops.rebase_progress()? {
                        Some((current, total)) => format!("rebase ({current}/{total})"),
                        None => "rebase".to_owned(),
                    }),
                    other => other.map(|s| s.command().to_owned()),
                };
                Ok(Response {
                    ok: true,
                    text,
                    ..Default::default()
                })
            }
        }
    }
}

/// merge したタグ（ヘッダーの `mergetag`）。
struct MergedTag {
    name: String,
    tagger: String,
    subject: String,
    signed: bool,
}

/// ヘッダーの `mergetag` を読む。値は複数行で、続きの行は空白 1 つで始まる（タグのオブジェクトそのもの）。
fn merged_tags(header: &str) -> Vec<MergedTag> {
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut in_tag = false;
    for line in header.lines() {
        if let Some(first) = line.strip_prefix("mergetag ") {
            blocks.push(vec![first]);
            in_tag = true;
        } else if let Some(rest) = line.strip_prefix(' ')
            && in_tag
        {
            blocks.last_mut().unwrap().push(rest);
        } else {
            in_tag = false;
        }
    }
    blocks
        .into_iter()
        .map(|lines| {
            let field = |key: &str| {
                lines
                    .iter()
                    .find_map(|l| l.strip_prefix(key))
                    .unwrap_or_default()
                    .to_owned()
            };
            let tagger = field("tagger ");
            let tagger = tagger.split(" <").next().unwrap_or_default().to_owned();
            // ヘッダーの後の空行の次がタグのメッセージ。
            let subject = lines
                .iter()
                .skip_while(|l| !l.is_empty())
                .find(|l| !l.is_empty())
                .map_or_else(String::new, |l| (*l).to_owned());
            MergedTag {
                name: field("tag "),
                tagger,
                subject,
                signed: lines.iter().any(|l| l.starts_with("-----BEGIN ")),
            }
        })
        .collect()
}

/// git の config の `[remote "origin"]` の `url`。
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == r#"[remote"origin"]"#;
        } else if in_origin
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

/// メッセージの最後の段落にあるトレーラー（`Signed-off-by: 名前 <メール>` など）。段落のすべての行が
/// `キー: 値`（か、その続きの字下げした行）のときだけトレーラーとみなす。
fn trailers(message: &str) -> Vec<(String, String)> {
    // 件名だけの段落はトレーラーにしない（`Fix: ...` のような件名がある）。
    let message = message.trim_end();
    let Some((_, last)) = message.rsplit_once("\n\n") else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for line in last.lines() {
        if line.starts_with([' ', '\t']) {
            match out.last_mut() {
                Some((_, value)) => {
                    value.push(' ');
                    value.push_str(line.trim());
                }
                None => return Vec::new(),
            }
        } else if let Some((key, value)) = trailer(line) {
            out.push((key.to_owned(), value.to_owned()));
        } else {
            return Vec::new();
        }
    }
    out
}

fn trailer(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(": ")?;
    let valid = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then_some((key, value.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_backend_gix::GixBackend;
    use amonhen_projection::FrontendProfile;
    use std::process::Command as Process;

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = Process::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        // テスト用のリポジトリは、CI の環境（利用者名がない・Windows の autocrlf）に左右されないようにする。
        if args.first() == Some(&"init") {
            for (key, value) in [
                ("user.name", "a"),
                ("user.email", "a@example.com"),
                ("core.autocrlf", "false"),
            ] {
                git(dir, &["config", key, value]);
            }
        }
        out
    }

    fn setup() -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.name", "a"]);
        git(dir, &["config", "user.email", "a@example.com"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        (tmp, repo)
    }

    fn run(repo: &Repository, json: &str) -> serde_json::Value {
        serde_json::from_str(&repo.run_json(json)).unwrap()
    }

    /// 何を渡されても G を返し、呼ばれた回数を数える検証。
    struct AlwaysGood(std::sync::atomic::AtomicUsize);

    impl amonhen_model::SignatureVerifier for AlwaysGood {
        fn verify(&self, commits: &[(Oid, &[u8])]) -> Result<Vec<char>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(vec!['G'; commits.len()])
        }
        fn details(&self, _: &Oid, _: &[u8]) -> Result<Option<amonhen_model::SignatureCheck>> {
            Ok(Some(amonhen_model::SignatureCheck {
                status: 'G',
                ..Default::default()
            }))
        }
    }

    #[test]
    fn reports_merged_commits_head_distance_and_references() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let first = git(dir, &["rev-parse", "HEAD"]);
        git(dir, &["switch", "-q", "-c", "topic"]);
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "topic work"]);
        git(dir, &["switch", "-q", "main"]);
        git(
            dir,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                "Merge branch 'topic'",
                "topic",
            ],
        );
        let merge = git(dir, &["rev-parse", "HEAD"]);
        let short = &first[..10];
        git(
            dir,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("Follow up\n\nFixes: {short} (\"first\")"),
            ],
        );
        let follow = git(dir, &["rev-parse", "HEAD"]);
        repo.load_history().unwrap();
        repo.load_refs().unwrap();
        let extras = |oid: &str| {
            run(&repo, &format!(r#"{{"op":"commitExtras","oid":"{oid}"}}"#))["info"].clone()
        };

        let m = extras(&merge);
        assert_eq!(m["mergedCount"], 1);
        assert_eq!(m["merged"][0]["subject"], "topic work");
        assert_eq!(m["headDistance"], 1, "HEAD の 1 つ前");
        assert_eq!(m["headRev"], "HEAD~1");
        assert_eq!(m["localBranches"], serde_json::json!(["main"]));
        assert_eq!(m["remoteBranches"], serde_json::json!([]));
        let f = extras(&follow);
        assert_eq!(f["headDistance"], 0);
        assert_eq!(f["references"][short]["oid"], first.as_str());
        assert_eq!(f["references"][short]["subject"], "first");
        assert_eq!(extras(&first)["mergedCount"], 0);
    }

    #[test]
    fn does_not_carry_signature_results_over_when_the_checker_changes() {
        let (tmp, repo) = setup();
        let head = Oid::from_hex(&git(tmp.path(), &["rev-parse", "HEAD"])).unwrap();
        // リポジトリ側の git では、署名がないので N。
        assert_eq!(repo.verify_signatures(&[head]).unwrap(), ['N']);
        // 手元で検証するものに替えると、覚えていた N は使わずに検証し直す。2 回目は覚えている。
        let local = std::sync::Arc::new(AlwaysGood(Default::default()));
        repo.set_signature_verifier(Some(local.clone()));
        assert_eq!(repo.verify_signatures(&[head]).unwrap(), ['G']);
        assert_eq!(repo.verify_signatures(&[head]).unwrap(), ['G']);
        assert_eq!(local.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        let extras = run(&repo, &format!(r#"{{"op":"commitExtras","oid":"{head}"}}"#));
        assert_eq!(extras["info"]["signature"]["status"], "G");
        // 戻すと、手元の G は引き継がない。
        let before = repo.signature_generation();
        repo.set_signature_verifier(None);
        assert_eq!(repo.verify_signatures(&[head]).unwrap(), ['N']);
        // 替える前に始めた検証の結果が後から届いても覚えない。
        repo.remember_signatures(before, [(head, 'G')]);
        assert_eq!(repo.verify_signatures(&[head]).unwrap(), ['N']);
    }

    #[test]
    fn reads_merged_tags_from_the_header() {
        let header = "tree t\nparent a\nparent b\nauthor x <x@y> 1 +0000\nmergetag object b\n type commit\n tag v6.5-fixes\n tagger Greg <g@k> 2 +0200\n \n USB fixes for 6.5\n \n -----BEGIN PGP SIGNATURE-----\n abc\n -----END PGP SIGNATURE-----\ncommitter x <x@y> 3 +0000";
        let tags = merged_tags(header);
        assert_eq!(tags.len(), 1);
        assert_eq!(
            (tags[0].name.as_str(), tags[0].tagger.as_str()),
            ("v6.5-fixes", "Greg")
        );
        assert_eq!(tags[0].subject, "USB fixes for 6.5");
        assert!(tags[0].signed);
        assert!(merged_tags("tree t\nauthor x <x@y> 1 +0000").is_empty());
    }

    #[test]
    fn reads_the_origin_url_from_the_config() {
        let config = "[core]\n\turl = no\n[remote \"upstream\"]\n\turl = u\n[remote \"origin\"]\n\turl = git@github.com:a/b.git\n";
        assert_eq!(
            origin_url(config).as_deref(),
            Some("git@github.com:a/b.git")
        );
        assert_eq!(origin_url("[core]\n"), None);
    }

    #[test]
    fn reads_trailers_only_from_the_last_paragraph() {
        assert_eq!(
            trailers("Fix it\n\nBody.\n\nSigned-off-by: A <a@x>\nCo-authored-by: B\n  <b@x>\n"),
            [
                ("Signed-off-by".to_owned(), "A <a@x>".to_owned()),
                ("Co-authored-by".to_owned(), "B <b@x>".to_owned()),
            ]
        );
        assert!(trailers("Fix: subject only").is_empty());
        assert!(trailers("Fix\n\nNot a trailer line\nKey: value").is_empty());
    }

    #[test]
    fn reports_details_from_git_and_the_graph() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["tag", "v1"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n4\n").unwrap();
        git(
            dir,
            &[
                "commit",
                "-q",
                "-am",
                "second\n\nSigned-off-by: a <a@example.com>",
            ],
        );
        git(dir, &["tag", "v2"]);
        git(dir, &["branch", "topic"]);
        let first = git(dir, &["rev-parse", "HEAD~1"]);
        let second = git(dir, &["rev-parse", "HEAD"]);
        repo.load_history().unwrap();
        repo.load_refs().unwrap();

        let info =
            run(&repo, &format!(r#"{{"op":"commitInfo","oid":"{second}"}}"#))["info"].clone();
        assert_eq!(info["trailers"][0]["key"], "Signed-off-by");
        assert_eq!(info["committerEmail"], "a@example.com");
        assert!(info["signed"].is_null());

        let extras = run(
            &repo,
            &format!(r#"{{"op":"commitExtras","oid":"{first}"}}"#),
        )["info"]
            .clone();
        assert_eq!(extras["children"][0], second.as_str());
        assert_eq!(extras["branches"], serde_json::json!(["main", "topic"]));
        assert_eq!(
            extras["tags"],
            serde_json::json!(["v1", "v2"]),
            "古いタグから"
        );
        assert_eq!(extras["firstTag"], "v1");
        assert!(extras["previousTag"].is_null());
        assert!(extras["signature"].is_null());
        let extras = run(
            &repo,
            &format!(r#"{{"op":"commitExtras","oid":"{second}"}}"#),
        )["info"]
            .clone();
        assert_eq!(extras["previousTag"], "v1");
        let resolved = run(
            &repo,
            &format!(r#"{{"op":"resolveCommit","rev":"{}"}}"#, &first[..8]),
        );
        assert_eq!(resolved["text"], first.as_str());
        let verified = run(
            &repo,
            &format!(r#"{{"op":"verifySignatures","oids":["{first}","{second}"]}}"#),
        );
        assert_eq!(verified["info"]["statuses"], serde_json::json!(["N", "N"]));
        assert_eq!(extras["stats"][0]["added"], 1);
    }

    #[test]
    fn runs_commands_and_undoes_them() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let r = run(
            &repo,
            r#"{"op":"createBranch","name":"topic","start":"main","checkout":false}"#,
        );
        assert_eq!(r["ok"], true);
        assert_eq!(
            run(&repo, r#"{"op":"undoPeek"}"#)["label"],
            "Create branch topic"
        );
        assert_eq!(
            run(&repo, r#"{"op":"undo"}"#)["label"],
            "Create branch topic"
        );
        assert!(git(dir, &["branch", "--list", "topic"]).is_empty());

        let bad = run(&repo, r#"{"op":"merge","rev":"no-such-branch"}"#);
        assert_eq!(bad["ok"], false);
        assert!(!bad["error"].as_str().unwrap().is_empty());
        assert_eq!(run(&repo, r#"{"op":"nonsense"}"#)["ok"], false);
    }

    fn log(dir: &std::path::Path) -> Vec<String> {
        git(dir, &["log", "--format=%s"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn commit_file(dir: &std::path::Path, name: &str, subject: &str) -> String {
        std::fs::write(dir.join(name), subject).unwrap();
        git(dir, &["add", name]);
        git(dir, &["commit", "-q", "-m", subject]);
        git(dir, &["rev-parse", "HEAD"])
    }

    fn rewrite(repo: &Repository, edit: serde_json::Value) -> serde_json::Value {
        let json = serde_json::json!({"op": "rewrite", "label": "edit", "edit": edit});
        run(repo, &json.to_string())
    }

    #[test]
    fn rewrites_history_and_undoes_it() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let b = commit_file(dir, "b.txt", "b");
        let c = commit_file(dir, "c.txt", "c");
        commit_file(dir, "d.txt", "d");
        assert_eq!(log(dir), ["d", "c", "b", "first"]);

        assert_eq!(
            run(&repo, &format!(r#"{{"op":"commitMessage","oid":"{c}"}}"#))["text"],
            "c\n"
        );
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": c, "message": "C!"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["d", "C!", "b", "first"]);
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(log(dir), ["d", "c", "b", "first"]);

        rewrite(&repo, serde_json::json!({"kind": "fixup", "oid": c}));
        assert_eq!(log(dir), ["d", "b", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        rewrite(&repo, serde_json::json!({"kind": "drop", "oid": b}));
        assert_eq!(log(dir), ["d", "c", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        rewrite(
            &repo,
            serde_json::json!({"kind": "move", "oid": b, "to": 0}),
        );
        assert_eq!(log(dir), ["b", "d", "c", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["d", "c", "b", "first"]);
    }

    #[test]
    fn splits_a_commit_and_records_undo_after_continuing() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("x.txt"), "x").unwrap();
        std::fs::write(dir.join("y.txt"), "y").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "x and y"]);
        let xy = git(dir, &["rev-parse", "HEAD"]);
        commit_file(dir, "z.txt", "z");

        let r = rewrite(&repo, serde_json::json!({"kind": "split", "oid": xy}));
        assert_eq!(r["outcome"], "stopped", "{r}");
        // todo は pick・exec・break・pick の 4 行で、break まで進んだところで止まる。
        assert_eq!(run(&repo, r#"{"op":"sequence"}"#)["text"], "rebase (3/4)");
        // 止まっている間は記録しない（保留）。
        assert!(run(&repo, r#"{"op":"undoPeek"}"#)["label"].is_null());
        git(dir, &["add", "x.txt"]);
        git(dir, &["commit", "-q", "-m", "x"]);
        git(dir, &["add", "y.txt"]);
        git(dir, &["commit", "-q", "-m", "y"]);
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(log(dir), ["z", "y", "x", "first"]);
        assert_eq!(run(&repo, r#"{"op":"undoPeek"}"#)["label"], "edit");
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["z", "x and y", "first"]);
    }

    #[test]
    fn resolves_merge_conflicts() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "a.txt", "1\nTOPIC\n3\n");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "a.txt", "1\nMAIN\n3\n");
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );

        let v = repo
            .conflict_view("a.txt", &FrontendProfile::MACOS)
            .unwrap();
        assert_eq!(v.removed, 1, "コンフリクトの数");
        let r = run(
            &repo,
            r#"{"op":"resolveConflict","path":"a.txt","index":0,"choice":"theirs"}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nTOPIC\n3\n"
        );
        // 解決は Undo できる。
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .contains("<<<<<<<")
        );

        assert_eq!(
            run(&repo, r#"{"op":"takeSide","path":"a.txt","ours":true}"#)["ok"],
            true
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nMAIN\n3\n"
        );
        let status = repo.load_status().unwrap();
        assert!(
            status
                .status
                .entries
                .iter()
                .all(|e| e.kind != amonhen_model::StatusKind::Unmerged)
        );
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(
            run(&repo, r#"{"op":"sequence"}"#)["text"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn merge_editor_reads_both_sides_resolves_text_and_saves() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "a.txt", "1\nTOPIC\n3\n");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "a.txt", "1\nMAIN\n3\n");
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );

        let sides = run(&repo, r#"{"op":"mergeSides","path":"a.txt"}"#);
        let info = &sides["info"];
        assert_eq!(info["current"]["text"], "1\nMAIN\n3\n", "{sides}");
        assert_eq!(info["incoming"]["text"], "1\nTOPIC\n3\n");
        assert_eq!(info["current"]["regions"], serde_json::json!([[1, 2]]));
        assert_eq!(
            info["base"],
            serde_json::Value::Null,
            "diff3 でなければ共通の祖先はない"
        );
        let result = info["result"].as_str().unwrap().to_owned();

        let command = |op: serde_json::Value| run(&repo, &op.to_string());
        let regions = command(serde_json::json!({"op": "conflictRegions", "text": result}));
        let conflict = &regions["info"]["conflicts"][0];
        assert_eq!(conflict["lines"], serde_json::json!([1, 6]));
        assert_eq!(
            (conflict["current"].as_str(), conflict["incoming"].as_str()),
            (Some("MAIN\n"), Some("TOPIC\n"))
        );
        let both = command(
            serde_json::json!({"op": "resolveText", "text": result, "index": 0, "choice": "both"}),
        );
        assert_eq!(both["text"], "1\nMAIN\nTOPIC\n3\n");
        // ファイルにはまだ書かない。
        assert!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .contains("<<<<<<<")
        );

        let saved = command(
            serde_json::json!({"op": "saveMerge", "path": "a.txt", "text": "1\nMERGED\n3\n", "stage": true}),
        );
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nMERGED\n3\n"
        );
        let status = repo.load_status().unwrap();
        assert!(
            status
                .status
                .entries
                .iter()
                .all(|e| e.kind != amonhen_model::StatusKind::Unmerged)
        );
        assert_eq!(run(&repo, r#"{"op":"undoPeek"}"#)["label"], "Merge a.txt");
    }

    #[test]
    fn blames_lines_and_follows_a_renamed_file() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        commit_file(dir, "old.txt", "one\ntwo\n");
        git(dir, &["mv", "old.txt", "new.txt"]);
        git(dir, &["commit", "-q", "-m", "rename"]);
        commit_file(dir, "new.txt", "one\nTWO\n");
        std::fs::write(dir.join("new.txt"), "one\nTWO\nthree\n").unwrap();

        let blame = run(&repo, r#"{"op":"blame","path":"new.txt"}"#);
        let lines = blame["info"]["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 3, "{blame}");
        let commits = blame["info"]["commits"].as_array().unwrap();
        let oid_of = |i: usize| {
            commits[lines[i]["c"].as_u64().unwrap() as usize]["oid"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(lines[1]["t"], "TWO");
        assert_ne!(oid_of(0), oid_of(1), "1 行目と 2 行目は別のコミット");
        assert!(
            oid_of(2).bytes().all(|b| b == b'0'),
            "まだコミットしていない行"
        );

        let history = run(&repo, r#"{"op":"fileHistory","path":"new.txt"}"#);
        let entries = history["info"]["entries"].as_array().unwrap();
        let paths: Vec<&str> = entries
            .iter()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths.first(), Some(&"new.txt"), "{history}");
        assert_eq!(paths.last(), Some(&"old.txt"), "名前を変える前も追う");
    }

    #[test]
    fn fast_forwards_a_branch_without_checking_it_out() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["branch", "old"]);
        commit_file(dir, "b.txt", "b");
        let head = git(dir, &["rev-parse", "HEAD"]);
        let r = run(&repo, r#"{"op":"mergeInto","target":"old","rev":"main"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(git(dir, &["rev-parse", "old"]), head);
        assert_eq!(
            git(dir, &["symbolic-ref", "--short", "HEAD"]),
            "main",
            "checkout しない"
        );
        run(&repo, r#"{"op":"undo"}"#);
        assert_ne!(git(dir, &["rev-parse", "old"]), head);
    }

    #[test]
    fn commits_and_discards_lines_undoably() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        let status = repo.load_status().unwrap();
        let diff = repo
            .working_diff(&status.status.entries[0], false, &FrontendProfile::MACOS)
            .unwrap();
        // @@, -1, +one, 2, -3, +three: 1 → one だけ戻す。
        repo.discard_rows(&diff, &[1, 2]).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\n2\nthree\n"
        );
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "one\n2\nthree\n"
        );

        repo.stage(&["a.txt"]).unwrap();
        repo.commit("second", false).unwrap();
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["label"], "Commit");
        assert_eq!(git(dir, &["log", "--format=%s"]), "first");
        assert_eq!(git(dir, &["diff", "--cached", "--name-only"]), "a.txt");
    }
}
