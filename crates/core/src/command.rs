//! FE から受け取る操作。頻度の低い操作はまとめてここを通し、Undo の記録もここで行う。

use amonhen_model::tr;
use amonhen_model::{Oid, Outcome, PullMode, Result, Root};
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
    Pull {
        #[serde(default)]
        mode: PullMode,
    },
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
    /// 操作ログ（`info.entries`: 新しい順の `label`・`time`（UNIX 時刻のミリ秒））。
    UndoLog,
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
    /// 連続したコミットをまとめるときの、メッセージの初期値（`oids` のメッセージを古い順に並べる）。
    /// `info.oldest`・`info.newest` に、編集できる範囲の中で連続していることを確かめた両端を返す
    /// （`info.message` にも同じメッセージ）。
    SquashRangeMessage {
        oids: Vec<String>,
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
    /// Magic Merge: `text` のコンフリクトのうち、決まった規則で解けるものを解いた文章（`info.text`）と、解いた数
    /// （`info.resolved`）・残った数（`info.remaining`）。ファイルには書かない。
    MagicMerge {
        text: String,
        /// 間違えようのない規則だけにする（ファイルを開いたときの自動の適用）。
        #[serde(default)]
        safe: bool,
        /// この番号のコンフリクトだけを解く（1 つずつ Undo できるように）。
        index: Option<usize>,
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
    /// `tip` から辿れて `base` から辿れないコミット（`git log base..tip`）。`info`: 全部の数 `count` と、新しい順に
    /// `limit` 個（既定 50）までの `commits`（oid・subject・author・time）。Push・Pull の前に、送る・取り込む
    /// コミットを見せる。
    Between {
        base: String,
        tip: String,
        limit: Option<usize>,
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
    /// すべての設定と、それぞれがどこで決まっているか（`info.entries`。後のものほど優先）。
    ConfigList,
    /// このリポジトリ（local）の設定を変える。`value` がなければ消す。
    ConfigSet {
        key: String,
        value: Option<String>,
    },
    /// リモート（`info.remotes`）。
    Remotes,
    /// HEAD と作業ツリーの、ファイルごとの追加・削除の行数（`info.stats`: `path`・`added`・`removed`。
    /// バイナリは null。untracked は含まない）。
    WorkingStats,
    /// `.gitignore` の書きかけの中身（`text`）の各行に合うファイルの例（`info.lines`: 行の番号・パターン・
    /// 例・合う数）。空行・# の行・! の行は飛ばす。多すぎないよう 40 行まで。
    IgnoreMatches {
        text: String,
    },
    /// hooks と Git LFS の様子（`info`: hooksPath・hooks（name・enabled）・lfs（入っているか）・lfsPatterns）。
    HooksAndLfs,
    /// SSH 先の daemon の Activity の記録（`info.records`。手元のリポジトリなら空）。
    RemoteActivity,
    /// 走っている操作を取り消す（`remote` なら SSH 先の daemon の記録の番号）。
    CancelActivity {
        seq: u64,
        #[serde(default)]
        remote: bool,
    },
    ChangeRemote {
        change: amonhen_model::RemoteChange,
    },
    /// `.gitignore`（作業ツリーの直下）と `.git/info/exclude` の中身（`info.gitignore`・`info.exclude`）。
    IgnoreFiles,
    /// `.gitignore` と `.git/info/exclude` を書く。
    SaveIgnoreFiles {
        gitignore: String,
        exclude: String,
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

/// 命令の JSON の `op`（Activity に出す名前）。
fn op_name(json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("op").and_then(|o| o.as_str()).map(str::to_owned))
        .unwrap_or_default()
}

fn oid(hex: &str) -> Result<Oid> {
    Oid::from_hex(hex).ok_or_else(|| tr!("Can’t read the commit ID: {hex}", hex = hex).into())
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
            self.remember_signatures(generation, [(*commit, s.clone())]);
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
        let signature = extras.signature.as_ref().map(signature_json);
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
        let head = self
            .backend()
            .head()?
            .ok_or_else(|| tr!("No commits yet"))?;
        let mut row = history
            .row_of(&head)
            .ok_or_else(|| tr!("HEAD isn’t in the History (reload it)"))?
            as usize;
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
            return Err(tr!("A merge or rebase is in progress. Finish or abort it first").into());
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
        // 手元の共有の daemon につないでいれば、daemon のコアで実行する（操作と Undo の記録を 1 か所にする）。
        // 手元のつながりそのものについての命令（daemon の記録を読む・取り消す）だけは、ここで実行する。
        if !matches!(op_name(json).as_str(), "remoteActivity" | "cancelActivity") {
            match self.delegate(&crate::delegate::Request::Command(json.to_owned())) {
                Ok(Some((crate::delegate::Reply::Command(reply), _))) => return reply,
                Ok(Some(_)) => {
                    let response = Response {
                        error: Some(tr!("Unexpected reply from the daemon")),
                        ..Default::default()
                    };
                    return serde_json::to_string(&response).unwrap_or_default();
                }
                Ok(None) => {}
                Err(e) => {
                    let response = Response {
                        error: Some(e.to_string()),
                        ..Default::default()
                    };
                    return serde_json::to_string(&response).unwrap_or_default();
                }
            }
        }
        let response = match serde_json::from_str::<Command>(json) {
            Ok(command) => match self.run_tracked(command, op_name(json)) {
                Ok(r) => r,
                Err(e) => Response {
                    error: Some(e.to_string()),
                    ..Default::default()
                },
            },
            Err(e) => Response {
                error: Some(tr!("Can’t read the command: {e}", e = e)),
                ..Default::default()
            },
        };
        serde_json::to_string(&response).unwrap_or_default()
    }

    /// 命令を実行し、Activity に記録する（命令の名前と、かかった時間）。
    fn run_tracked(&self, command: Command, name: String) -> Result<Response> {
        let repo = self
            .paths()
            .workdir
            .map(|w| w.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Activity が数秒ごとに daemon の記録を読む問い合わせは、記録しない（一覧がそれで埋まる）。
        if name == "remoteActivity" {
            return self.run(command);
        }
        amonhen_model::activity::track("command", &repo, name, || self.run(command))
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
                let sequence = ops
                    .sequence_in_progress()?
                    .ok_or_else(|| tr!("No operation in progress"))?;
                ops.abort(sequence)?;
                undo.settle_pending(ops)?;
                ok()
            }
            Command::Continue => {
                let sequence = ops
                    .sequence_in_progress()?
                    .ok_or_else(|| tr!("No operation in progress"))?;
                let outcome = ops.continue_sequence(sequence)?;
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::Skip => {
                let sequence = ops
                    .sequence_in_progress()?
                    .ok_or_else(|| tr!("No operation in progress"))?;
                let outcome = ops.skip_sequence(sequence)?;
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::Fetch => {
                ops.fetch()?;
                ok()
            }
            Command::Pull { mode } => {
                done(undo.record(ops, "Pull", &[], false, || ops.pull(mode), not_done)?)
            }
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
                    .ok_or_else(|| tr!("{name} not found", name = name))?;
                let target = entry.target.ok_or_else(|| tr!("The stash has no commit"))?;
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
            Command::UndoLog => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "entries": undo.entries() })),
                ..Default::default()
            }),
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
                    self.backend()
                        .head()?
                        .ok_or_else(|| tr!("No commits yet"))?
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
            Command::SquashRangeMessage { oids } => {
                let ids = oids.iter().map(|o| oid(o)).collect::<Result<Vec<_>>>()?;
                let chain = self.editable_chain()?;
                // chain は新しい順。選んだコミットの位置が隙間なく続いていること。
                let mut positions = ids
                    .iter()
                    .map(|id| {
                        chain.iter().position(|c| c.oid == *id).ok_or_else(|| {
                            tr!("Some commits are outside the editable range (not reachable from HEAD, or before a merge)")
                        })
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                positions.sort_unstable();
                positions.dedup();
                if positions.len() < 2 || positions.windows(2).any(|w| w[1] != w[0] + 1) {
                    return Err(tr!("To squash, select two or more consecutive commits").into());
                }
                let newest = chain[positions[0]].oid;
                let oldest = chain[*positions.last().unwrap()].oid;
                let text = positions
                    .iter()
                    .rev()
                    .map(|&p| {
                        self.commit_message(&chain[p].oid)
                            .map(|m| m.trim_end().to_owned())
                    })
                    .collect::<Result<Vec<_>>>()?
                    .join("\n\n");
                Ok(Response {
                    ok: true,
                    text: Some(text.clone()),
                    info: Some(serde_json::json!({
                        "oldest": oldest.to_string(),
                        "newest": newest.to_string(),
                        "message": text,
                    })),
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
                    .ok_or_else(|| tr!("Can’t squash: the parent isn’t in the editable range"))?;
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
                let data = files
                    .read(Root::Workdir, &path)?
                    .ok_or_else(|| tr!("File not found"))?;
                let resolved = crate::conflict::resolve(&data, index, choice)
                    .ok_or_else(|| tr!("That conflict no longer exists"))?;
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
                let found = self.verify_signatures(&ids)?;
                // 署名者がコミッターと一致するかを FE が確かめられるよう、正しい署名にはコミッターのメールを添える。
                let signed: Vec<Oid> = ids
                    .iter()
                    .zip(&found)
                    .filter(|(_, c)| matches!(c.status, 'G' | 'U'))
                    .map(|(id, _)| *id)
                    .collect();
                let mut committers = std::collections::HashMap::new();
                if !signed.is_empty() {
                    for (id, text) in signed.iter().zip(self.backend().commit_texts(&signed)?) {
                        committers.insert(*id, committer_email(&text.raw));
                    }
                }
                let checks: Vec<serde_json::Value> = ids
                    .iter()
                    .zip(&found)
                    .map(|(id, c)| {
                        let mut json = signature_json(c);
                        json["committerEmail"] =
                            committers.get(id).cloned().unwrap_or_default().into();
                        json
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "checks": checks })),
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
                    .ok_or_else(|| tr!("File not found"))?;
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
                    .ok_or_else(|| tr!("That conflict no longer exists"))?;
                Ok(Response {
                    ok: true,
                    text: Some(String::from_utf8_lossy(&resolved).into_owned()),
                    ..Default::default()
                })
            }
            Command::MagicMerge { text, safe, index } => {
                let rules = if safe {
                    crate::conflict::MagicRules::Safe
                } else {
                    crate::conflict::MagicRules::All
                };
                let (merged, resolved) = crate::conflict::magic(text.as_bytes(), rules, index);
                let remaining = crate::conflict::parse(&merged).len();
                let text = String::from_utf8_lossy(&merged).into_owned();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "text": text, "resolved": resolved, "remaining": remaining
                    })),
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
            Command::Between { base, tip, limit } => {
                let history = self.load_history()?;
                let graph = &history.graph;
                let row = |rev: &str| -> Result<usize> {
                    let id = ops.resolve(&format!("{rev}^{{commit}}"))?;
                    history.row_of(&id).map(|r| r as usize).ok_or_else(|| {
                        tr!("{rev} isn’t in the History (reload it)", rev = rev).into()
                    })
                };
                let (base, tip) = (row(&base)?, row(&tip)?);
                // base から辿れるものに印を付けてから、tip から印のないものだけを辿る。
                let walk = |start: usize, stop: &[bool], seen: &mut Vec<bool>| {
                    let mut stack = vec![start];
                    while let Some(r) = stack.pop() {
                        if seen[r] || stop[r] {
                            continue;
                        }
                        seen[r] = true;
                        stack.extend(graph.parents(r).iter().map(|&p| p as usize));
                    }
                };
                let none = vec![false; graph.len()];
                let mut reachable = vec![false; graph.len()];
                walk(base, &none, &mut reachable);
                let mut only = vec![false; graph.len()];
                walk(tip, &reachable, &mut only);
                // History の行は新しい順。
                let rows: Vec<usize> = (0..graph.len()).filter(|&r| only[r]).collect();
                let count = rows.len();
                let ids: Vec<Oid> = rows
                    .iter()
                    .take(limit.unwrap_or(50))
                    .map(|&r| graph.ids()[r])
                    .collect();
                let texts = self.commit_texts(&ids)?;
                let commits: Vec<_> = ids
                    .iter()
                    .zip(&texts)
                    .map(|(id, t)| {
                        serde_json::json!({
                            "oid": id.to_string(),
                            "subject": String::from_utf8_lossy(t.subject()),
                            "author": String::from_utf8_lossy(t.author_name()),
                            "time": t.author_time,
                        })
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "count": count, "commits": commits })),
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
            Command::ConfigList => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "entries": ops.config_list()? })),
                ..Default::default()
            }),
            Command::ConfigSet { key, value } => {
                ops.config_set(&key, value.as_deref())?;
                ok()
            }
            Command::IgnoreMatches { text } => {
                let lines: Vec<(usize, String)> = text
                    .lines()
                    .enumerate()
                    .map(|(i, l)| (i + 1, l.trim().to_owned()))
                    .filter(|(_, l)| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!'))
                    .take(40)
                    .collect();
                let patterns: Vec<String> = lines.iter().map(|(_, l)| l.clone()).collect();
                let matches = ops.ignore_matches(&patterns, 5)?;
                let lines: Vec<_> = lines
                    .iter()
                    .zip(matches)
                    .map(|((line, pattern), (examples, count))| {
                        serde_json::json!({"line": line, "pattern": pattern, "examples": examples, "count": count})
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "lines": lines })),
                    ..Default::default()
                })
            }
            Command::RemoteActivity => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "records": ops.activity()? })),
                ..Default::default()
            }),
            Command::CancelActivity { seq, remote } => {
                let cancelled = if remote {
                    ops.cancel_activity(seq)?
                } else {
                    amonhen_model::activity::cancel(seq)
                };
                if !cancelled {
                    return Err(tr!(
                        "Can’t cancel (it already finished, or it can’t be cancelled)"
                    )
                    .into());
                }
                Ok(Response {
                    ok: true,
                    ..Default::default()
                })
            }
            Command::HooksAndLfs => {
                let config = ops.config_list()?;
                let value = |key: &str| {
                    config
                        .iter()
                        .rev()
                        .find(|e| e.key.eq_ignore_ascii_case(key))
                        .map(|e| e.value.clone())
                };
                let hooks_path = value("core.hooksPath");
                // hooksPath が作業ツリーの中（相対）なら作業ツリーから、なければ .git/hooks から読む。
                let (root, dir) = match &hooks_path {
                    Some(p) if !p.starts_with('/') && !p.starts_with('~') => {
                        (Root::Workdir, p.trim_end_matches('/').to_owned())
                    }
                    Some(_) => (Root::GitDir, String::new()),
                    None => (Root::GitDir, "hooks".to_owned()),
                };
                let files = self.backend().files();
                let mut hooks = Vec::new();
                if !dir.is_empty() {
                    for name in HOOK_NAMES {
                        if let Some(mode) = files.mode(root, &format!("{dir}/{name}"))? {
                            hooks.push(
                                serde_json::json!({"name": name, "enabled": mode & 0o111 != 0}),
                            );
                        }
                    }
                }
                let attributes = files
                    .read(Root::Workdir, ".gitattributes")?
                    .unwrap_or_default();
                let lfs_patterns: Vec<String> = String::from_utf8_lossy(&attributes)
                    .lines()
                    .filter(|l| l.split_whitespace().any(|a| a == "filter=lfs"))
                    .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "hooksPath": hooks_path,
                        "hooks": hooks,
                        "lfs": value("filter.lfs.process").or(value("filter.lfs.smudge")).is_some(),
                        "lfsPatterns": lfs_patterns,
                    })),
                    ..Default::default()
                })
            }
            Command::WorkingStats => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "stats": ops.working_stats()? })),
                ..Default::default()
            }),
            Command::Remotes => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "remotes": ops.remotes()? })),
                ..Default::default()
            }),
            Command::ChangeRemote { change } => {
                ops.change_remote(&change)?;
                ok()
            }
            Command::IgnoreFiles => {
                let files = self.backend().files();
                let read = |root, path| -> Result<String> {
                    Ok(files
                        .read(root, path)?
                        .map(|b| String::from_utf8_lossy(&b).into_owned())
                        .unwrap_or_default())
                };
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "gitignore": read(Root::Workdir, ".gitignore")?,
                        "exclude": read(Root::GitDir, "info/exclude")?,
                    })),
                    ..Default::default()
                })
            }
            Command::SaveIgnoreFiles { gitignore, exclude } => {
                let files = self.backend().files();
                files.write(Root::Workdir, ".gitignore", gitignore.as_bytes())?;
                files.write(Root::GitDir, "info/exclude", exclude.as_bytes())?;
                ok()
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

/// コミットのオブジェクトの `committer` の行のメールアドレス。
fn committer_email(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.lines()
        .take_while(|l| !l.is_empty())
        .find_map(|l| l.strip_prefix("committer "))
        .and_then(|l| l.split_once('<'))
        .and_then(|(_, rest)| rest.split_once('>'))
        .map_or_else(String::new, |(email, _)| email.to_owned())
}

/// 署名の検証の結果を JSON に（`status` は `%G?` の 1 文字）。
fn signature_json(s: &amonhen_model::SignatureCheck) -> serde_json::Value {
    serde_json::json!({
        "status": s.status.to_string(),
        "signer": s.signer,
        "key": s.key,
        "fingerprint": s.fingerprint,
        "trust": s.trust,
        "emails": s.emails.iter().map(|e| serde_json::json!({
            "email": e.email,
            "validity": e.validity.to_string(),
        })).collect::<Vec<_>>(),
    })
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

/// git が呼ぶ hook の名前（`git help hooks` の順）。
const HOOK_NAMES: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

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
        fn verify(&self, commits: &[(Oid, &[u8])]) -> Result<Vec<amonhen_model::SignatureCheck>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(vec![
                amonhen_model::SignatureCheck {
                    status: 'G',
                    ..Default::default()
                };
                commits.len()
            ])
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

    fn statuses(repo: &Repository, id: Oid) -> Vec<char> {
        repo.verify_signatures(&[id])
            .unwrap()
            .iter()
            .map(|c| c.status)
            .collect()
    }

    #[test]
    fn lists_commits_on_one_side_only() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        for name in ["b", "c"] {
            std::fs::write(dir.join(name), "x\n").unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", &format!("add {name}")]);
        }
        let between = run(&repo, r#"{"op":"between","base":"main","tip":"topic"}"#);
        assert_eq!(between["info"]["count"], 2);
        // 新しい順。
        assert_eq!(between["info"]["commits"][0]["subject"], "add c");
        assert_eq!(between["info"]["commits"][1]["subject"], "add b");
        let limited = run(
            &repo,
            r#"{"op":"between","base":"main","tip":"topic","limit":1}"#,
        );
        assert_eq!(limited["info"]["count"], 2);
        assert_eq!(limited["info"]["commits"].as_array().unwrap().len(), 1);
        let back = run(&repo, r#"{"op":"between","base":"topic","tip":"main"}"#);
        assert_eq!(back["info"]["count"], 0);
    }

    #[test]
    fn does_not_carry_signature_results_over_when_the_checker_changes() {
        let (tmp, repo) = setup();
        let head = Oid::from_hex(&git(tmp.path(), &["rev-parse", "HEAD"])).unwrap();
        // リポジトリ側の git では、署名がないので N。
        assert_eq!(statuses(&repo, head), ['N']);
        // 手元で検証するものに替えると、覚えていた N は使わずに検証し直す。2 回目は覚えている。
        let local = std::sync::Arc::new(AlwaysGood(Default::default()));
        repo.set_signature_verifier(Some(local.clone()));
        assert_eq!(statuses(&repo, head), ['G']);
        assert_eq!(statuses(&repo, head), ['G']);
        assert_eq!(local.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        let extras = run(&repo, &format!(r#"{{"op":"commitExtras","oid":"{head}"}}"#));
        assert_eq!(extras["info"]["signature"]["status"], "G");
        // 戻すと、手元の G は引き継がない。
        let before = repo.signature_generation();
        repo.set_signature_verifier(None);
        assert_eq!(statuses(&repo, head), ['N']);
        // 替える前に始めた検証の結果が後から届いても覚えない。
        repo.remember_signatures(
            before,
            [(
                head,
                amonhen_model::SignatureCheck {
                    status: 'G',
                    ..Default::default()
                },
            )],
        );
        assert_eq!(statuses(&repo, head), ['N']);
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
        assert_eq!(verified["info"]["checks"][0]["status"], "N");
        assert_eq!(
            committer_email(b"tree t\ncommitter A B <a@x.y> 1 +0000\n\nmsg"),
            "a@x.y"
        );
        assert_eq!(verified["info"]["checks"][1]["status"], "N");
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
    fn folds_merged_branches_in_a_real_repository() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "t1.txt", "1");
        commit_file(dir, "t2.txt", "2");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "m.txt", "m");
        git(
            dir,
            &["merge", "-q", "--no-ff", "-m", "merge topic", "topic"],
        );
        let merge = git(dir, &["rev-parse", "HEAD"]);
        repo.load_history().unwrap();
        // HEAD は起点（main）にも含まれる。重複しても動くこと。
        let folded = repo.history_folded(None, &[]).unwrap();
        // topic は merge に取り込み済みなので、ブランチが残っていても折りたたむ。
        assert_eq!(folded.graph.len(), 3);
        let all = repo
            .history_folded(Some(&[Oid::from_hex(&merge).unwrap()]), &[])
            .unwrap();
        assert_eq!(all.graph.len(), 3, "merge・m・first だけ");
        let row = all.row_of(&Oid::from_hex(&merge).unwrap()).unwrap() as usize;
        assert_eq!(all.folded[row], 2);
        assert!(
            all.layout.nodes()[row].flags & amonhen_layout::Node::MERGE != 0,
            "折りたたんでも merge"
        );
        let opened = repo
            .history_folded(
                Some(&[Oid::from_hex(&merge).unwrap()]),
                &[Oid::from_hex(&merge).unwrap()],
            )
            .unwrap();
        assert_eq!(opened.graph.len(), 5);
        assert!(!all.density(4).is_empty());
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
    fn shows_files_matching_ignore_patterns_and_hooks() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("build/out")).unwrap();
        std::fs::write(dir.join("build/out/a.o"), "").unwrap();
        std::fs::write(dir.join("x.log"), "").unwrap();
        std::fs::write(dir.join("y.log"), "").unwrap();
        let r = run(
            &repo,
            &serde_json::json!({"op": "ignoreMatches", "text": "# c\n*.log\n\nbuild/\n!keep.log\nnone.txt"}).to_string(),
        );
        let lines = r["info"]["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 3, "{r}");
        assert_eq!(lines[0]["line"], 2);
        assert_eq!(lines[0]["count"], 2);
        assert_eq!(lines[1]["examples"][0], "build/");
        assert_eq!(lines[2]["count"], 0);

        let hook = dir.join(".git/hooks/pre-commit");
        std::fs::write(&hook, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(
            dir.join(".gitattributes"),
            "*.psd filter=lfs diff=lfs merge=lfs -text\n*.txt text\n",
        )
        .unwrap();
        let r = run(&repo, r#"{"op":"hooksAndLfs"}"#);
        let hooks = r["info"]["hooks"].as_array().unwrap();
        assert!(
            hooks
                .iter()
                .any(|h| h["name"] == "pre-commit" && h["enabled"] == true),
            "{r}"
        );
        // サンプル（.sample）は数えない。
        assert!(
            !hooks
                .iter()
                .any(|h| h["name"].as_str().unwrap().ends_with(".sample"))
        );
        assert_eq!(r["info"]["lfsPatterns"], serde_json::json!(["*.psd"]));
    }

    #[test]
    fn squashes_a_range_of_commits() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let b = commit_file(dir, "b.txt", "b");
        let c = commit_file(dir, "c.txt", "c");
        let d = commit_file(dir, "d.txt", "d");
        commit_file(dir, "e.txt", "e");
        // 選んだ順に関わらず、古い順のメッセージと両端を返す。
        let r = run(
            &repo,
            &serde_json::json!({"op": "squashRangeMessage", "oids": [d, b, c]}).to_string(),
        );
        assert_eq!(r["text"], "b\n\nc\n\nd", "{r}");
        assert_eq!(r["info"]["oldest"], b.as_str());
        assert_eq!(r["info"]["newest"], d.as_str());
        // 飛び飛びは断る。
        let gap = run(
            &repo,
            &serde_json::json!({"op": "squashRangeMessage", "oids": [b, d]}).to_string(),
        );
        assert_eq!(gap["ok"], false, "{gap}");

        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "squashRange", "oid": b, "last": d, "message": "b to d"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["e", "b to d", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["e", "d", "c", "b", "first"]);
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
    fn counts_added_and_removed_lines_against_head() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        // a.txt は 1 2 3 → 1 x 3 y（index に一部、作業ツリーに残り）。
        std::fs::write(dir.join("a.txt"), "1\nx\n3\n").unwrap();
        git(dir, &["add", "a.txt"]);
        std::fs::write(dir.join("a.txt"), "1\nx\n3\ny\n").unwrap();
        std::fs::write(dir.join("new.txt"), "untracked\n").unwrap();
        let r = run(&repo, r#"{"op":"workingStats"}"#);
        let stats = r["info"]["stats"].as_array().unwrap();
        assert_eq!(stats.len(), 1, "{r}");
        assert_eq!(stats[0]["path"], "a.txt");
        assert_eq!(
            (stats[0]["added"].as_u64(), stats[0]["removed"].as_u64()),
            (Some(2), Some(1))
        );
    }

    #[test]
    fn lists_the_undo_log_newest_first() {
        let (_tmp, repo) = setup();
        for name in ["a", "b"] {
            let r = run(
                &repo,
                &format!(
                    r#"{{"op":"createBranch","name":"{name}","start":"HEAD","checkout":true}}"#
                ),
            );
            assert_eq!(r["ok"], true, "{r}");
        }
        let log = run(&repo, r#"{"op":"undoLog"}"#);
        let entries = log["info"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0]["label"].as_str().unwrap().contains('b'));
        assert!(entries[0]["time"].as_u64().unwrap() >= entries[1]["time"].as_u64().unwrap());
    }

    #[test]
    fn edits_local_config_remotes_and_ignore_files() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        assert_eq!(
            run(
                &repo,
                r#"{"op":"configSet","key":"pull.rebase","value":"true"}"#
            )["ok"],
            true
        );
        let list = run(&repo, r#"{"op":"configList"}"#);
        let entries = list["info"]["entries"].as_array().unwrap();
        // 全体の設定（Windows の CI には system の pull.rebase がある）ではなく、このリポジトリの設定を見る。
        let pull = entries
            .iter()
            .find(|e| e["key"] == "pull.rebase" && e["scope"] == "local")
            .unwrap();
        assert_eq!(
            (pull["scope"].as_str(), pull["value"].as_str()),
            (Some("local"), Some("true"))
        );
        assert!(pull["origin"].as_str().unwrap().ends_with("config"));
        // 消す（もともとないものを消しても失敗しない）。
        for _ in 0..2 {
            assert_eq!(
                run(
                    &repo,
                    r#"{"op":"configSet","key":"pull.rebase","value":null}"#
                )["ok"],
                true
            );
        }
        let list = run(&repo, r#"{"op":"configList"}"#);
        let keys = list["info"]["entries"].as_array().unwrap();
        assert!(
            !keys
                .iter()
                .any(|e| e["key"] == "pull.rebase" && e["scope"] == "local")
        );

        let change = |json: &str| {
            run(
                &repo,
                &format!(r#"{{"op":"changeRemote","change":{json}}}"#),
            )
        };
        assert_eq!(
            change(r#"{"kind":"add","name":"up","url":"https://example.com/a.git"}"#)["ok"],
            true
        );
        assert_eq!(
            change(r#"{"kind":"rename","from":"up","to":"origin"}"#)["ok"],
            true
        );
        assert_eq!(
            change(r#"{"kind":"setUrl","name":"origin","url":"git@example.com:a.git"}"#)["ok"],
            true
        );
        let remotes = run(&repo, r#"{"op":"remotes"}"#);
        assert_eq!(remotes["info"]["remotes"][0]["name"], "origin");
        assert_eq!(
            remotes["info"]["remotes"][0]["fetchUrl"],
            "git@example.com:a.git"
        );
        assert_eq!(change(r#"{"kind":"remove","name":"origin"}"#)["ok"], true);
        assert_eq!(
            run(&repo, r#"{"op":"remotes"}"#)["info"]["remotes"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let r = run(
            &repo,
            r#"{"op":"saveIgnoreFiles","gitignore":"target/\n","exclude":"*.local\n"}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "target/\n"
        );
        let files = run(&repo, r#"{"op":"ignoreFiles"}"#);
        assert_eq!(files["info"]["exclude"], "*.local\n");
    }

    #[test]
    fn pulls_by_merge_rebase_or_fast_forward_only() {
        let (upstream, _) = setup();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("clone");
        git(
            tmp.path(),
            &["clone", "-q", upstream.path().to_str().unwrap(), "clone"],
        );
        for (key, value) in [("user.name", "a"), ("user.email", "a@example.com")] {
            git(&dir, &["config", key, value]);
        }
        // 上流と手元の両方で進め、分かれさせる。
        std::fs::write(upstream.path().join("b.txt"), "b\n").unwrap();
        git(upstream.path(), &["add", "."]);
        git(upstream.path(), &["commit", "-q", "-m", "upstream"]);
        std::fs::write(dir.join("c.txt"), "c\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "local"]);
        let repo = Repository::new(GixBackend::open(&dir).unwrap());

        // 分かれていると fast-forward だけでは取り込めない。
        let r = run(&repo, r#"{"op":"pull","mode":"fastForwardOnly"}"#);
        assert_eq!(r["ok"], false, "{r}");
        let r = run(&repo, r#"{"op":"pull","mode":"rebase"}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        // rebase なので merge コミットはなく、手元のコミットが上流の上に載る。
        assert_eq!(git(&dir, &["log", "--format=%s"]), "local\nupstream\nfirst");
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        // mode を省くと merge。
        let r = run(&repo, r#"{"op":"pull"}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(git(&dir, &["rev-list", "--count", "--merges", "HEAD"]), "1");
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

    /// 改行を変換するリポジトリ。`committed` を変換なしで commit し、`config` を設定してから開く
    /// （gix は開いたときの設定を使う）。
    fn setup_converting(
        config: &[(&str, &str)],
        committed: &[u8],
    ) -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), committed).unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        for (key, value) in config {
            git(dir, &["config", key, value]);
        }
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        (tmp, repo)
    }

    fn index_blob(dir: &std::path::Path) -> Vec<u8> {
        Process::new("git")
            .args(["cat-file", "blob", ":a.txt"])
            .current_dir(dir)
            .output()
            .unwrap()
            .stdout
    }

    fn unstaged_diff(repo: &Repository) -> crate::WorkingDiff {
        let status = repo.load_status().unwrap();
        repo.working_diff(&status.status.entries[0], false, &FrontendProfile::MACOS)
            .unwrap()
    }

    #[test]
    fn diffs_and_stages_lines_through_autocrlf() {
        // Windows の既定（core.autocrlf=true）: 作業ツリーは CRLF、index は LF。
        let (tmp, repo) = setup_converting(&[("core.autocrlf", "true")], b"1\n2\n3\n");
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "1\r\nTWO\r\n3\r\n4\r\n").unwrap();
        let diff = unstaged_diff(&repo);
        assert_eq!(
            (diff.file.added, diff.file.removed),
            (2, 1),
            "改行の違いは差分にしない"
        );
        // @@, 1, -2, +TWO, 3, +4: 4 だけを stage する。index に CR を入れない。
        repo.stage_rows(&diff, &[5]).unwrap();
        assert_eq!(index_blob(dir), b"1\n2\n3\n4\n");
    }

    #[test]
    fn discards_lines_keeping_the_other_line_endings() {
        // 改行が混ざったファイル（2 行目だけ LF）。選んだ行のほかは 1 バイトも変えない。
        let (tmp, repo) = setup_converting(&[("core.autocrlf", "true")], b"1\n2\n3\n4\n");
        let dir = tmp.path();
        let before = b"1\r\nTWO\n3\r\nFOUR\r\n";
        std::fs::write(dir.join("a.txt"), before).unwrap();
        let diff = unstaged_diff(&repo);
        // @@, 1, -2, +TWO, 3, -4, +FOUR: 4 → FOUR だけ戻す。戻す行は checkout と同じ CRLF。
        repo.discard_rows(&diff, &[5, 6]).unwrap();
        assert_eq!(
            std::fs::read(dir.join("a.txt")).unwrap(),
            b"1\r\nTWO\n3\r\n4\r\n"
        );
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), before);
    }

    #[test]
    fn discards_lines_in_a_file_whose_endings_differ_from_its_eol_attribute() {
        // `eol=lf` なのに作業ツリーは CRLF。戻す行は LF、ほかの行は CRLF のまま。
        let (tmp, repo) = setup_converting(&[], b"1\n2\n3\n");
        let dir = tmp.path();
        std::fs::write(dir.join(".git/info/attributes"), "*.txt eol=lf\n").unwrap();
        let repo = {
            drop(repo);
            Repository::new(GixBackend::open(dir).unwrap())
        };
        std::fs::write(dir.join("a.txt"), "1\r\n2x\r\n3\r\n").unwrap();
        let diff = unstaged_diff(&repo);
        assert_eq!((diff.file.added, diff.file.removed), (1, 1));
        repo.discard_rows(&diff, &[2, 3]).unwrap();
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"1\r\n2\n3\r\n");
    }

    #[test]
    fn leaves_crlf_committed_files_unconverted() {
        // CRLF のまま commit されたファイルは、core.autocrlf=true でも変換しない（git と同じ）。
        let (tmp, repo) = setup_converting(&[("core.autocrlf", "true")], b"1\r\n2\r\n3\r\n");
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "1\r\nTWO\r\n3\r\n").unwrap();
        let diff = unstaged_diff(&repo);
        assert_eq!((diff.file.added, diff.file.removed), (1, 1));
        // @@, 1, -2, +TWO, 3: TWO を足すことだけを stage する。
        repo.stage_rows(&diff, &[3]).unwrap();
        assert_eq!(index_blob(dir), b"1\r\n2\r\nTWO\r\n3\r\n");
    }
}
