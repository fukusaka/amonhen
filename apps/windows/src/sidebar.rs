//! サイドバーの木: Workspace（Working Copy・History・あるときだけ Stashes）/ Branches / Remotes / Tags /
//! あるときだけ Submodules。GTK4 版の `sidebar.rs`（macOS 版の `SidebarViewController`）と同じ並べ方。
//!
//! Branches はコアの `branchGroups` で並べる: お気に入り（★）、今のブランチ（積んであればその根元）と既定の
//! ブランチ、動いているブランチ（新しい順。`feature/x` はフォルダにまとめ、フォルダは中でいちばん新しいブランチの
//! 位置に出る）、Inactive・Archived のフォルダ。ほかのブランチの上に積んだブランチは、土台の行の下に入れ子で出す。
//! `branchGroups` が届くまでは名前順のフォルダ。
//!
//! 項目のキー: `ref:<種類>:<名前>`（ref）、`page:<名前>`（Workspace の画面）、`group:`・`folder:`（見出し）。

use std::collections::{HashMap, HashSet};

use amonhen_model::{RefEntry, RefKind, Refs, tr};
use serde_json::Value;
use windows_reactor::TreeNode;

thread_local! {
    /// 絞り込んでいる間（フォルダを開いて、合うものを見せる）。`nodes` が作る間だけ。
    static EXPAND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// 行のツールチップ（項目のキーごと。印の意味など）。`nodes` が作り直す。
    static TIPS: std::cell::RefCell<HashMap<String, String>> = std::cell::RefCell::new(HashMap::new());
    /// 取り込み済みのブランチ（名前 → 印のツールチップ）。`branch_rows` が作る間だけ。
    static MERGED: std::cell::RefCell<HashMap<String, String>> = std::cell::RefCell::new(HashMap::new());
}

/// 行の後ろの、押すと吹き出しを出す印（macOS 版・GTK4 版の行の右端の印）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// ブランチの merged・gone: ブランチの様子。
    Branch,
    /// タグの 🔏・🔓: 署名。
    Signature,
    /// タグの ⚠・∅: リモートの様子。
    TagRemotes,
    /// プルリクエストの CI の ✓・✗・●: チェックの一覧。
    Checks,
}

/// `key` の行の文字 `text` にある印と、その位置（UTF-16 の単位で、始まりと長さ）。印は名前の後ろの 2 つの空白の
/// 後にある（名前の中の同じ字は見ない）。
pub fn marks_in(key: &str, text: &str) -> Vec<(Mark, usize, usize)> {
    let candidates: &[(&str, Mark)] = match parse_ref_key(key) {
        Some((kind, _)) if kind == RefKind::Local as u8 => {
            &[("merged", Mark::Branch), ("gone", Mark::Branch)]
        }
        Some((kind, _)) if kind == RefKind::Tag as u8 => &[
            ("🔏", Mark::Signature),
            ("🔓", Mark::Signature),
            ("⚠", Mark::TagRemotes),
            ("∅", Mark::TagRemotes),
        ],
        _ if key.starts_with("pr:") => &[
            ("✓", Mark::Checks),
            ("✗", Mark::Checks),
            ("●", Mark::Checks),
        ],
        _ => return Vec::new(),
    };
    let Some(tail) = text.find("  ") else {
        return Vec::new();
    };
    let units = |s: &str| s.encode_utf16().count();
    let mut found = Vec::new();
    // 名前の後ろを空白で切った語のうち、印のもの（ref の名前に空白は入らない）。
    let mut at = tail;
    for word in text[tail..].split(' ') {
        if let Some((_, mark)) = candidates.iter().find(|(w, _)| *w == word) {
            found.push((*mark, units(&text[..at]), units(word)));
        }
        at += word.len() + 1;
    }
    found
}

/// 項目のツールチップ（GTK4 版の行のツールチップ: 印の意味と、submodule・subtree・タグの様子）。
pub fn tooltip(key: &str) -> Option<String> {
    TIPS.with(|t| t.borrow().get(key).cloned())
}

fn set_tooltip(key: &str, lines: Vec<String>) {
    if !lines.is_empty() {
        TIPS.with(|t| t.borrow_mut().insert(key.to_owned(), lines.join("\n")));
    }
}

/// Inactive とみなす日数（最後のコミットからこれだけ経ったブランチ）。
const STALE_DAYS: i64 = 90;

/// リポジトリの形（`repositoryForm` の答えのうち、印に使うもの。GTK4 版の `repoform::Form`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepositoryForm {
    pub shallow: bool,
    /// partial clone なら取らずにおいたもの（`blob:none` など）。
    pub partial: Option<String>,
    /// sparse checkout なら 1 文の説明（Working Copy の一覧の上に出す）。
    pub sparse: Option<String>,
}

impl RepositoryForm {
    pub fn from_info(info: &Value) -> Self {
        let sparse = info["sparse"].as_object().map(|s| {
            let cone = s.get("cone").and_then(Value::as_bool).unwrap_or(false);
            let folders = s
                .get("folders")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if !cone {
                amonhen_model::tr!(
                    "Sparse checkout: only the files matching its patterns are checked out."
                )
            } else {
                match folders {
                    0 => amonhen_model::tr!(
                        "Sparse checkout: only the files at the top are checked out."
                    ),
                    1 => amonhen_model::tr!("Sparse checkout: only 1 folder is checked out."),
                    n => amonhen_model::tr!(
                        "Sparse checkout: only {count} folders are checked out.",
                        count = n
                    ),
                }
            }
        });
        Self {
            shallow: info["shallow"].as_bool().unwrap_or(false),
            partial: info["partial"].as_object().map(|p| {
                p.get("filter")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_owned()
            }),
            sparse,
        }
    }
}

/// ブランチ 1 本の分け方（`branchGroups` の 1 件）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branch {
    /// 先端のコミットの日時（UNIX 時間）。
    pub time: Option<i64>,
    /// その上に積んであるブランチ（土台）。
    pub parent: Option<String>,
    pub archived: bool,
    /// お気に入りの順位（小さいほど上）。
    pub pinned: Option<i64>,
    /// 既定のブランチに取り込み済み。
    pub merged: bool,
    /// 既定のブランチの先端と同じ（作ったばかりで、まだコミットしていない）。
    pub at_base: bool,
    /// squash・rebase でマージしたプルリクエストの番号（GitHub から読んだもの）。
    pub pull_request: Option<u64>,
}

/// `branchGroups` の答え。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchGroups {
    /// 既定のブランチ（`origin/main`・`main` など）。
    pub base: Option<String>,
    pub branches: HashMap<String, Branch>,
}

impl BranchGroups {
    /// 既定のブランチの、手元のブランチの名前（`origin/main` なら `main`）。
    pub fn local_base(&self) -> Option<&str> {
        let base = self.base.as_deref()?;
        Some(base.split_once('/').map_or(base, |(_, b)| b))
    }

    /// `run_json` の答え（`{"info": {...}}`）から。
    pub fn from_reply(reply: &Value) -> Option<Self> {
        let info = &reply["info"];
        let branches = info["branches"]
            .as_array()?
            .iter()
            .filter_map(|b| {
                Some((
                    b["name"].as_str()?.to_owned(),
                    Branch {
                        time: b["time"].as_i64(),
                        parent: b["parent"].as_str().map(str::to_owned),
                        archived: b["archived"].as_bool().unwrap_or(false),
                        pinned: b["pinned"].as_i64(),
                        merged: b["merged"].as_bool().unwrap_or(false),
                        at_base: b["atBase"].as_bool().unwrap_or(false),
                        pull_request: b["pullRequest"].as_u64(),
                    },
                ))
            })
            .collect();
        Some(Self {
            base: info["base"].as_str().map(str::to_owned),
            branches,
        })
    }
}

pub fn ref_key(entry: &RefEntry) -> String {
    format!("ref:{}:{}", entry.kind as u8, entry.name)
}

/// `ref:` のキーの種類と名前。
pub fn parse_ref_key(key: &str) -> Option<(u8, &str)> {
    let (kind, name) = key.strip_prefix("ref:")?.split_once(':')?;
    Some((kind.parse().ok()?, name))
}

/// 行に付ける印のもと: リポジトリの形、コンフリクトの数、タグのリモートでの様子（`tagRemotes` の info）。
pub struct Marks<'a> {
    pub form: &'a RepositoryForm,
    pub conflicts: usize,
    pub tag_remotes: &'a serde_json::Value,
    /// submodule の様子（コアの `submodules` の info の `submodules`）。
    pub submodules: &'a serde_json::Value,
    /// subtree（コアの `subtrees` の info の `subtrees`）。
    pub subtrees: &'a serde_json::Value,
    /// 署名のあるタグの状態と署名者（コアの `tagSignatures`）。
    pub tag_signatures: &'a std::collections::HashMap<String, (crate::signature::Status, String)>,
    /// 開いているプルリクエスト（GitHub から読んだもの。Branches の後に見出しを出す）。
    pub pull_requests: &'a [crate::github::PullRequest],
}

/// サイドバーの木。`refs` がまだなければ Workspace だけ。
/// `range_diff` なら、Workspace に Range Diff の行を出す（開いている間だけ）。
/// `worktrees` は worktree の行（キーと見出し。2 つ以上あるときだけ Working Copy の下に出す）。
/// `form` はリポジトリの形の印（History の行の後ろ・Working Copy の行の後ろ。GTK4 版の `history_marks` と
/// sparse checkout の印）。
pub fn nodes(
    refs: Option<&Refs>,
    groups: Option<&BranchGroups>,
    range_diff: bool,
    worktrees: &[(String, String)],
    marks: Marks,
    filter: &str,
) -> Vec<TreeNode> {
    let Marks {
        form,
        conflicts,
        tag_remotes,
        submodules: submodule_states,
        subtrees,
        tag_signatures,
        pull_requests,
    } = marks;
    TIPS.with(|t| t.borrow_mut().clear());
    let mut history_tips = Vec::new();
    if form.shallow {
        history_tips.push(tr!("Shallow clone: older history isn’t fetched"));
    }
    if let Some(filter) = &form.partial {
        history_tips.push(tr!(
            "Partial clone: file contents are fetched when needed ({filter})",
            filter = filter
        ));
    }
    set_tooltip("page:history", history_tips);
    set_tooltip("page:working-copy", form.sparse.iter().cloned().collect());
    let mut history = String::from("History");
    if form.shallow {
        history.push_str("  ✂");
    }
    if form.partial.is_some() {
        history.push_str("  ☁");
    }
    let working = if form.sparse.is_some() {
        "Working Copy  ⊟"
    } else {
        "Working Copy"
    };
    // worktree が 2 つ以上あれば、Working Copy の下に並べる（GTK4 版・macOS 版と同じ。✎ ⌂ ⑂ と変更の数つき）。
    let mut working = TreeNode::new("page:working-copy", working);
    if worktrees.len() > 1 {
        working = working
            .children(
                worktrees
                    .iter()
                    .map(|(key, text)| TreeNode::new(key.as_str(), text.as_str())),
            )
            .expanded(true);
    }
    let mut workspace = vec![working, TreeNode::new("page:history", history)];
    // コンフリクトのファイルがあるときだけ、History の下に数を添えて（GTK4 版と同じ）。
    if conflicts > 0 {
        workspace.push(TreeNode::new(
            "page:conflicts",
            format!("Conflicts  {conflicts}"),
        ));
    }
    let Some(refs) = refs else {
        return vec![group("workspace", "Workspace", workspace).expanded(true)];
    };
    // 絞り込み: ブランチ・リモートのブランチ・タグを、名前にすべての語を含むものだけに（大文字小文字を区別しない。
    // GTK4 版の `set_filter`）。
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    EXPAND.with(|e| e.set(!words.is_empty()));
    let filtered;
    let refs = if words.is_empty() {
        refs
    } else {
        filtered = Refs {
            entries: refs
                .entries
                .iter()
                .filter(|e| {
                    !matches!(e.kind, RefKind::Local | RefKind::Remote | RefKind::Tag) || {
                        let name = e.name.to_lowercase();
                        words.iter().all(|w| name.contains(w.as_str()))
                    }
                })
                .cloned()
                .collect(),
            ..refs.clone()
        };
        &filtered
    };
    let of = |kind: RefKind| refs.entries.iter().filter(move |e| e.kind == kind);
    let head = refs.head_branch.as_deref();
    // stash は見出しにしない（Workspace の Stashes の行に数だけ）。
    let stashes = of(RefKind::Stash).count();
    if stashes > 0 {
        workspace.push(TreeNode::new("page:stashes", format!("Stashes  {stashes}")));
    }
    if range_diff {
        workspace.push(TreeNode::new("page:range-diff", "Range Diff"));
    }
    // いつも Workspace の最後に出す（GTK4 版と同じ）。
    workspace.push(TreeNode::new("page:reflog", "Reflog"));
    let mut out = vec![group("workspace", "Workspace", workspace).expanded(true)];

    let locals: Vec<&RefEntry> = of(RefKind::Local).collect();
    let branches = branch_rows(&locals, head, groups);
    if !branches.is_empty() {
        out.push(group("branches", "Branches", branches).expanded(true));
    }
    // Pull Requests は Branches の後に（開いているものがあるときだけ。絞り込み中は出さない。GTK4 版と同じ）。
    if words.is_empty() && !pull_requests.is_empty() {
        let rows = pull_requests.iter().map(|pr| {
            let key = format!("pr:{}", pr.number);
            set_tooltip(&key, vec![pr.tooltip()]);
            let marks: Vec<&str> = [pr.is_draft.then_some("draft"), Some(pr.ci().mark())]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect();
            let mut title = format!("#{} {}", pr.number, pr.title);
            if !marks.is_empty() {
                title.push_str("  ");
                title.push_str(&marks.join(" "));
            }
            TreeNode::new(key, title)
        });
        out.push(group("pull-requests", "Pull Requests", rows).expanded(true));
    }

    // リモートごとにまとめる（origin/feature/x → origin の下の feature/x。その下はフォルダにしない）。
    let mut by_remote: Vec<(&str, Vec<TreeNode>)> = Vec::new();
    for e in of(RefKind::Remote) {
        let (remote, branch) = e.name.split_once('/').unwrap_or((&e.name, &e.name));
        let node = TreeNode::new(ref_key(e), branch);
        match by_remote.iter_mut().find(|(r, _)| *r == remote) {
            Some((_, nodes)) => nodes.push(node),
            None => by_remote.push((remote, vec![node])),
        }
    }
    by_remote.sort_by(|a, b| a.0.cmp(b.0));
    if !by_remote.is_empty() {
        let remotes = by_remote.into_iter().map(|(remote, nodes)| {
            TreeNode::new(format!("folder:remotes/{remote}"), remote)
                .children(nodes)
                .expanded(EXPAND.with(|e| e.get()))
        });
        out.push(group("remotes", "Remotes", remotes).expanded(true));
    }

    // タグも `v1/x` のような名前はフォルダにまとめる。
    let mut tags = Tree::default();
    for e in of(RefKind::Tag) {
        let parts: Vec<&str> = e.name.split('/').collect();
        // 署名の印（検証できた署名は 🔏、それ以外の署名は 🔓。GTK4 版・macOS 版は状態の色の印）と、リモートの印。
        let signature = tag_signatures.get(&e.name).map(|(status, _)| {
            if matches!(status, crate::signature::Status::Verified(_)) {
                "🔏"
            } else {
                "🔓"
            }
        });
        let shown: Vec<&str> = signature
            .into_iter()
            .chain(tag_mark(tag_remotes, &e.name))
            .collect();
        set_tooltip(&ref_key(e), tag_tooltip(tag_remotes, &e.name));
        let title = if shown.is_empty() {
            parts[parts.len() - 1].to_owned()
        } else {
            format!("{}  {}", parts[parts.len() - 1], shown.join(" "))
        };
        tags.insert(&parts, TreeNode::new(ref_key(e), title));
    }
    let tags = tags.into_nodes("tags");
    if !tags.is_empty() {
        out.push(group("tags", "Tags", tags));
    }

    let submodules: Vec<TreeNode> = of(RefKind::Submodule)
        .map(|e| {
            set_tooltip(&ref_key(e), submodule_tooltip(submodule_states, &e.name));
            let title = match submodule_mark(submodule_states, &e.name) {
                Some(mark) => format!("{}  {mark}", e.name),
                None => e.name.clone(),
            };
            TreeNode::new(ref_key(e), title)
        })
        .collect();
    if !submodules.is_empty() {
        out.push(group("submodules", "Submodules", submodules));
    }

    // subtree（取り込んだフォルダ。GTK4 版と同じく、Pull の squash の記録があれば印）。
    let subtrees: Vec<TreeNode> = subtrees
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let dir = s["dir"].as_str()?;
            let about = [
                s["url"].as_str().map(|u| match s["ref"].as_str() {
                    Some(r) => format!("{u} {r}"),
                    None => u.to_owned(),
                }),
                (s["squashed"].as_bool() == Some(true)).then(|| tr!("Squashed when last pulled")),
            ];
            set_tooltip(
                &format!("subtree:{dir}"),
                about.into_iter().flatten().collect(),
            );
            let title = if s["squashed"].as_bool() == Some(true) {
                format!("{dir}  ⊟")
            } else {
                dir.to_owned()
            };
            Some(TreeNode::new(format!("subtree:{dir}"), title))
        })
        .collect();
    if !subtrees.is_empty() {
        out.push(group("subtrees", "Subtrees", subtrees));
    }
    out
}

/// submodule の様子（`submodules` の答えの 1 つ）。
pub fn submodule_state<'a>(
    states: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    states
        .as_array()?
        .iter()
        .find(|s| s["path"].as_str() == Some(path))
}

/// submodule の行の後ろの印（GTK4 版の `finish_submodule`）: コンフリクト ⚠、取り出していない ○、記録と違う
/// コミット ≠、中に変更 ✎。
fn submodule_mark(states: &serde_json::Value, path: &str) -> Option<String> {
    let s = submodule_state(states, path)?;
    let on = |k: &str| s[k].as_bool() == Some(true);
    if on("conflict") {
        return Some("⚠".to_owned());
    }
    if !on("initialized") {
        return Some("○".to_owned());
    }
    let marks: Vec<&str> = [(on("moved"), "≠"), (on("changed"), "✎")]
        .into_iter()
        .filter_map(|(yes, m)| yes.then_some(m))
        .collect();
    (!marks.is_empty()).then(|| marks.join(" "))
}

/// submodule の行のツールチップ（GTK4 版の `finish_submodule`）: 印の意味、URL、追うブランチ。
fn submodule_tooltip(states: &serde_json::Value, path: &str) -> Vec<String> {
    let Some(s) = submodule_state(states, path) else {
        return Vec::new();
    };
    let on = |k: &str| s[k].as_bool() == Some(true);
    let mut lines = Vec::new();
    if on("conflict") {
        lines.push(tr!("In conflict"));
    } else if !on("initialized") {
        lines.push(tr!("Not checked out (Update checks it out)"));
    } else {
        if on("moved") {
            lines.push(tr!(
                "At another commit than the one recorded (Update goes back to it)"
            ));
        }
        if on("changed") {
            lines.push(tr!("Has uncommitted changes inside"));
        }
    }
    lines.extend(s["url"].as_str().map(str::to_owned));
    lines.extend(
        s["branch"]
            .as_str()
            .map(|b| tr!("Follows {branch}", branch = b)),
    );
    lines
}

/// タグの行のツールチップ（GTK4 版の `finish_tag`）: 印の意味と、リモートごとの様子。
pub fn tag_tooltip(info: &serde_json::Value, tag: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let checked: Vec<&str> = info["remotes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["checked"].is_i64())
        .filter_map(|r| r["name"].as_str())
        .collect();
    match tag_mark(info, tag) {
        Some("⚠") => {
            let different: Vec<&str> = checked
                .iter()
                .copied()
                .filter(|r| info["tags"][tag][*r].as_str() == Some("different"))
                .collect();
            lines.push(tr!(
                "A different tag with this name is on {remotes}",
                remotes = different.join(", ")
            ));
        }
        Some(_) => lines.push(tr!("Not on any remote (as of the last fetch)")),
        None => {}
    }
    lines.extend(
        info["remotes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["name"].as_str())
            .map(|r| tag_state_text(info, tag, r)),
    );
    lines
}

/// タグの行の後ろの印（GTK4 版の `TagRemotes::mark`）: 読んだリモートのどれかに同じ名前の別のタグがあれば ⚠、
/// 読んだどのリモートにもなければ ∅。`info` はコアの `tagRemotes` の info。
pub fn tag_mark(info: &serde_json::Value, tag: &str) -> Option<&'static str> {
    let checked: Vec<&str> = info["remotes"]
        .as_array()?
        .iter()
        .filter(|r| r["checked"].is_i64())
        .filter_map(|r| r["name"].as_str())
        .collect();
    if checked.is_empty() {
        return None;
    }
    let state = |remote: &str| info["tags"][tag][remote].as_str();
    if checked.iter().any(|r| state(r) == Some("different")) {
        return Some("⚠");
    }
    checked
        .iter()
        .all(|r| state(r).is_none_or(|s| s == "missing"))
        .then_some("∅")
}

/// タグの、リモート 1 つでの様子の一文（GTK4 版の `tag_state_text`。タグのメニューの先頭に並べる）。
pub fn tag_state_text(info: &serde_json::Value, tag: &str, remote: &str) -> String {
    let checked = info["remotes"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|r| r["name"].as_str() == Some(remote) && r["checked"].is_i64());
    if !checked {
        return tr!(
            "{remote}: not checked yet (Fetch reads it)",
            remote = remote
        );
    }
    match info["tags"][tag][remote].as_str() {
        Some("same") => tr!("{remote}: has it", remote = remote),
        Some("different") => tr!(
            "{remote}: has a different tag with this name",
            remote = remote
        ),
        _ => tr!("{remote}: doesn’t have it", remote = remote),
    }
}

fn group(key: &str, title: &str, children: impl IntoIterator<Item = TreeNode>) -> TreeNode {
    TreeNode::new(format!("group:{key}"), title).children(children)
}

/// ローカルブランチの行の文字。`title` は見せる名前（フォルダの中なら最後の部分）。チェックアウト中なら ✓、
/// お気に入りなら ★、積んだブランチなら ↳、上流との差（↑ 先にある、↓ 取り込んでいない）、上流がなくなって
/// いれば gone。
fn branch_text(e: &RefEntry, title: &str, head: Option<&str>, mark: Option<&str>) -> String {
    let mut text = String::new();
    if head == Some(e.name.as_str()) {
        text.push_str("✓ ");
    }
    if let Some(mark) = mark {
        text.push_str(mark);
        text.push(' ');
    }
    text.push_str(title);
    if e.ahead > 0 {
        text.push_str(&format!("  ↑{}", e.ahead));
    }
    if e.behind > 0 {
        text.push_str(&format!("  ↓{}", e.behind));
    }
    if e.gone {
        text.push_str("  gone");
    }
    // 取り込み済み（既定のブランチ、squash・rebase でマージしたプルリクエスト）。意味はツールチップに。
    if let Some(tip) = MERGED.with(|m| m.borrow().get(&e.name).cloned()) {
        text.push_str("  merged");
        set_tooltip(&ref_key(e), vec![tip]);
    }
    text
}

/// 名前を `/` で切ったフォルダの木。フォルダは、中で最初に入れた項目の位置に出る。
#[derive(Default)]
struct Tree {
    items: Vec<TreeItem>,
}

enum TreeItem {
    Folder(String, Tree),
    Leaf(TreeNode),
}

impl Tree {
    fn insert(&mut self, parts: &[&str], node: TreeNode) {
        if parts.len() <= 1 {
            self.items.push(TreeItem::Leaf(node));
            return;
        }
        let found = self
            .items
            .iter()
            .position(|i| matches!(i, TreeItem::Folder(n, _) if n == parts[0]));
        let index = found.unwrap_or_else(|| {
            self.items
                .push(TreeItem::Folder(parts[0].to_owned(), Tree::default()));
            self.items.len() - 1
        });
        if let TreeItem::Folder(_, tree) = &mut self.items[index] {
            tree.insert(&parts[1..], node);
        }
    }

    fn into_nodes(self, path: &str) -> Vec<TreeNode> {
        self.items
            .into_iter()
            .map(|item| match item {
                TreeItem::Leaf(node) => node,
                TreeItem::Folder(name, tree) => {
                    let path = format!("{path}/{name}");
                    TreeNode::new(format!("folder:{path}"), name)
                        .children(tree.into_nodes(&path))
                        .expanded(EXPAND.with(|e| e.get()))
                }
            })
            .collect()
    }
}

/// Branches の行（GTK4 版の `branch_rows` と同じ分け方）。
fn branch_rows(
    locals: &[&RefEntry],
    head: Option<&str>,
    groups: Option<&BranchGroups>,
) -> Vec<TreeNode> {
    let folders = |list: &[&RefEntry]| {
        let mut tree = Tree::default();
        for e in list {
            let parts: Vec<&str> = e.name.split('/').collect();
            let text = branch_text(e, parts[parts.len() - 1], head, None);
            tree.insert(&parts, TreeNode::new(ref_key(e), text));
        }
        tree.into_nodes("branches")
    };
    MERGED.with(|m| m.borrow_mut().clear());
    let Some(groups) = groups else {
        return folders(locals);
    };
    let fact = |name: &str| groups.branches.get(name);
    let by_name: HashMap<&str, &RefEntry> = locals.iter().map(|e| (e.name.as_str(), *e)).collect();
    // 既定のブランチは `origin/main` のようにリモートのこともある（手元のブランチの名前にする）。
    let base_name = groups.base.as_deref().map(|name| {
        if by_name.contains_key(name) {
            name
        } else {
            name.split_once('/').map_or(name, |(_, b)| b)
        }
    });
    // 取り込み済みの印（今のブランチと既定のブランチ、作ったばかりのものは除く。GTK4 版と同じ）。
    let base_label = groups.base.clone().unwrap_or_default();
    MERGED.with(|m| {
        let mut m = m.borrow_mut();
        for (name, b) in &groups.branches {
            if Some(name.as_str()) == head || Some(name.as_str()) == base_name {
                continue;
            }
            if let Some(number) = b.pull_request {
                m.insert(
                    name.clone(),
                    tr!("Merged by pull request #{number}", number = number),
                );
            } else if b.merged && !b.at_base {
                m.insert(name.clone(), tr!("Merged into {base}", base = base_label));
            }
        }
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let time = |name: &str| fact(name).and_then(|f| f.time).unwrap_or(0);
    let newest = |list: &mut Vec<&RefEntry>| list.sort_by_key(|e| std::cmp::Reverse(time(&e.name)));
    let archived = |name: &str| fact(name).is_some_and(|f| f.archived);
    let pinned = |name: &str| fact(name).and_then(|f| f.pinned).is_some();
    // 今のブランチがほかのブランチの上に積んであれば、その積み重ねの根元ごと一番上に出す。
    let mut stack_root = head.map(str::to_owned);
    let mut seen = HashSet::new();
    while let Some(name) = stack_root.clone() {
        let Some(parent) = fact(&name).and_then(|f| f.parent.clone()) else {
            break;
        };
        if seen.contains(&parent) || !by_name.contains_key(parent.as_str()) || archived(&parent) {
            break;
        }
        seen.insert(name);
        stack_root = Some(parent);
    }
    let is_top = |name: &str| {
        (Some(name) == stack_root.as_deref() && !pinned(name)) || Some(name) == base_name
    };
    // 土台のブランチ → その上に積んだブランチ（土台がアーカイブ済みなら、積んだほうは自分の場所に出す。
    // お気に入りは入れ子にしない）。
    let mut stacked: HashMap<&str, Vec<&RefEntry>> = HashMap::new();
    let mut nested = HashSet::new();
    for e in locals {
        if is_top(&e.name) || archived(&e.name) || pinned(&e.name) {
            continue;
        }
        let Some(parent) = fact(&e.name).and_then(|f| f.parent.as_deref()) else {
            continue;
        };
        if !by_name.contains_key(parent) || archived(parent) {
            continue;
        }
        stacked.entry(parent).or_default().push(e);
        nested.insert(e.name.as_str());
    }
    for list in stacked.values_mut() {
        newest(list);
    }
    // 積んだブランチを、土台の行の下に（さらに積んだものはその下に）付ける。積んだブランチは土台とは別の名前
    // なので、名前を全部出す。
    fn attach(
        node: TreeNode,
        name: &str,
        stacked: &HashMap<&str, Vec<&RefEntry>>,
        head: Option<&str>,
        depth: usize,
    ) -> TreeNode {
        let Some(children) = stacked.get(name).filter(|_| depth < 32) else {
            return node;
        };
        node.expanded(true).children(children.iter().map(|child| {
            let text = branch_text(child, &child.name, head, Some("↳"));
            attach(
                TreeNode::new(ref_key(child), text),
                &child.name,
                stacked,
                head,
                depth + 1,
            )
        }))
    }
    let tree = |e: &RefEntry, mark: Option<&str>| {
        let node = TreeNode::new(ref_key(e), branch_text(e, &e.name, head, mark));
        attach(node, &e.name, &stacked, head, 0)
    };

    let (mut stars, mut top, mut active, mut inactive, mut archive) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for e in locals.iter().copied() {
        if nested.contains(e.name.as_str()) {
            continue;
        }
        if pinned(&e.name) {
            stars.push(e);
        } else if is_top(&e.name) {
            top.push(e);
        } else if archived(&e.name) {
            archive.push(e);
        } else if fact(&e.name)
            .and_then(|f| f.time)
            .is_some_and(|t| now - t > STALE_DAYS * 86_400)
        {
            inactive.push(e);
        } else {
            active.push(e);
        }
    }
    // お気に入りは決めた順（順位が同じなら名前順）。
    stars.sort_by_key(|e| {
        (
            fact(&e.name).and_then(|f| f.pinned).unwrap_or(0),
            e.name.clone(),
        )
    });
    let mut rows: Vec<TreeNode> = stars.iter().map(|e| tree(e, Some("★"))).collect();
    top.sort_by_key(|e| Some(e.name.as_str()) != stack_root.as_deref());
    rows.extend(top.iter().map(|e| tree(e, None)));
    // 動いているブランチは新しい順。
    newest(&mut active);
    let mut active_tree = Tree::default();
    for e in &active {
        let parts: Vec<&str> = e.name.split('/').collect();
        let node = TreeNode::new(
            ref_key(e),
            branch_text(e, parts[parts.len() - 1], head, None),
        );
        active_tree.insert(&parts, attach(node, &e.name, &stacked, head, 0));
    }
    rows.extend(active_tree.into_nodes("branches"));
    newest(&mut inactive);
    archive.sort_by(|a, b| a.name.cmp(&b.name));
    for (key, title, list) in [
        ("inactive", "Inactive", &inactive),
        ("archived", "Archived", &archive),
    ] {
        if !list.is_empty() {
            rows.push(
                TreeNode::new(
                    format!("folder:branches/#{key}"),
                    format!("{title}  {}", list.len()),
                )
                .children(list.iter().map(|e| tree(e, None))),
            );
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    #[test]
    fn marks_are_found_after_the_name() {
        let branch = format!("ref:{}:topic", RefKind::Local as u8);
        assert_eq!(
            marks_in(&branch, "✓ topic  ↑1  merged"),
            [(Mark::Branch, 13, 6)]
        );
        // 名前の中の同じ語は印ではない。
        assert_eq!(marks_in(&branch, "merged"), []);
        let tag = format!("ref:{}:v1", RefKind::Tag as u8);
        // 🔏 は UTF-16 で 2 つ。
        assert_eq!(
            marks_in(&tag, "v1  🔏 ⚠"),
            [(Mark::Signature, 4, 2), (Mark::TagRemotes, 7, 1)]
        );
        assert_eq!(marks_in("pr:3", "#3 Fix  draft ✗"), [(Mark::Checks, 14, 1)]);
        assert_eq!(marks_in("page:history", "History  merged"), []);
    }

    use super::*;

    fn local(name: &str) -> RefEntry {
        RefEntry {
            kind: RefKind::Local,
            name: name.into(),
            target: None,
            detail: None,
            ahead: 0,
            behind: 0,
            gone: false,
        }
    }

    fn texts(nodes: &[TreeNode]) -> Vec<String> {
        nodes.iter().map(|n| format!("{n:?}")).collect()
    }

    #[test]
    fn groups_branches_like_the_other_apps() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let reply = serde_json::json!({ "info": {
            "base": "origin/main",
            "branches": [
                { "name": "main", "time": now },
                { "name": "feature/a", "time": now - 10 },
                { "name": "feature/b", "time": now - 5, "parent": "feature/a" },
                { "name": "old", "time": now - 200 * 86_400 },
                { "name": "gone-by", "time": now, "archived": true },
                { "name": "star", "time": now, "pinned": 1 },
            ]
        }});
        let groups = BranchGroups::from_reply(&reply).unwrap();
        let entries: Vec<RefEntry> = ["feature/a", "feature/b", "gone-by", "main", "old", "star"]
            .into_iter()
            .map(local)
            .collect();
        let locals: Vec<&RefEntry> = entries.iter().collect();
        let rows = branch_rows(&locals, Some("main"), Some(&groups));
        let rows = texts(&rows);
        assert_eq!(rows.len(), 5, "{rows:#?}");
        assert!(rows[0].contains("★ star"));
        assert!(rows[1].contains("✓ main"));
        // feature/b は feature/a の上に積んである。
        assert!(rows[2].contains("folder:branches/feature") && rows[2].contains("↳ feature/b"));
        assert!(rows[3].contains("Inactive  1") && rows[3].contains("old"));
        assert!(rows[4].contains("Archived  1") && rows[4].contains("gone-by"));
    }
}
