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
}

/// `branchGroups` の答え。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchGroups {
    /// 既定のブランチ（`origin/main`・`main` など）。
    pub base: Option<String>,
    pub branches: HashMap<String, Branch>,
}

impl BranchGroups {
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
}

/// サイドバーの木。`refs` がまだなければ Workspace だけ。
/// `range_diff` なら、Workspace に Range Diff の行を出す（開いている間だけ）。
/// `worktrees` は worktree の行（キーと見出し。2 つ以上あるときだけ Workspace の下に Worktrees を出す）。
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
    } = marks;
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
    let mut workspace = vec![
        TreeNode::new("page:working-copy", working),
        TreeNode::new("page:history", history),
    ];
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
    if worktrees.len() > 1 {
        out.push(
            group(
                "worktrees",
                "Worktrees",
                worktrees
                    .iter()
                    .map(|(key, text)| TreeNode::new(key.as_str(), text.as_str())),
            )
            .expanded(true),
        );
    }

    let locals: Vec<&RefEntry> = of(RefKind::Local).collect();
    let branches = branch_rows(&locals, head, groups);
    if !branches.is_empty() {
        out.push(group("branches", "Branches", branches).expanded(true));
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
        let title = match tag_mark(tag_remotes, &e.name) {
            Some(mark) => format!("{}  {mark}", parts[parts.len() - 1]),
            None => parts[parts.len() - 1].to_owned(),
        };
        tags.insert(&parts, TreeNode::new(ref_key(e), title));
    }
    let tags = tags.into_nodes("tags");
    if !tags.is_empty() {
        out.push(group("tags", "Tags", tags));
    }

    let submodules: Vec<TreeNode> = of(RefKind::Submodule)
        .map(|e| {
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
