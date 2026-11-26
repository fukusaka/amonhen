//! サイドバーの木: Workspace（Working Copy・History・あるときだけ Stashes）/ Branches / Remotes / Tags /
//! あるときだけ Submodules。GTK4 版の `sidebar.rs`（macOS 版の `SidebarViewController`）と同じ並べ方。
//!
//! Branches はコアの `branchGroups` で並べる: お気に入り、今のブランチ（積んであればその根元）と既定の
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
    /// 行の中身（鍵 → アイコン・名前・印）。`row`。
    static ROWS: std::cell::RefCell<HashMap<String, Row>> = std::cell::RefCell::new(HashMap::new());
    /// 木を作った回数と時（`generation`・`built_at`）。
    static BUILT: std::cell::Cell<(u64, Option<std::time::Instant>)> =
        const { std::cell::Cell::new((0, None)) };
    /// 取り込み済みのブランチ（名前 → 印のツールチップ）。`branch_rows` が作る間だけ。
    static MERGED: std::cell::RefCell<HashMap<String, String>> = std::cell::RefCell::new(HashMap::new());
}

/// 行の後ろの、押すと吹き出しを出す印（macOS 版・GTK4 版の行の右端の印）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// ブランチの merged・gone: ブランチの様子。
    Branch,
    /// タグの署名。
    Signature,
    /// タグのリモートの様子（同じ名前の別のタグがある・どのリモートにもない）。
    TagRemotes,
    /// プルリクエストの CI（通った・落ちた・走っている）: チェックの一覧。
    Checks,
}

/// 行の右端の印 1 つ（macOS 版の行の右端のアイコン）。`icon` は共通のアイコンか文字、`color` は `#rrggbb`
/// （`None` は薄い灰色）、`tip` はその印のツールチップ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowMark {
    pub mark: Option<Mark>,
    pub icon: Icon,
    pub color: Option<String>,
    pub tip: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Icon {
    /// 共通のアイコン（`crates/icons` の名前）。
    Shared(&'static str),
    Text(String),
}

/// サイドバーの行 1 つの中身: 前のアイコン（共通のアイコンの名前。見出しにはない）、名前、右端の印。
/// `accent` はアイコンを強調の色に（チェックアウト中のブランチ、開いている worktree。macOS 版と同じ）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub icon: Option<&'static str>,
    pub accent: bool,
    pub text: String,
    pub marks: Vec<RowMark>,
}

impl Mark {
    fn key(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Signature => "signature",
            Self::TagRemotes => "tagremotes",
            Self::Checks => "checks",
        }
    }

    /// 行の中身の Tag（`mark:…`）から。
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag.strip_prefix("mark:")? {
            "branch" => Some(Self::Branch),
            "signature" => Some(Self::Signature),
            "tagremotes" => Some(Self::TagRemotes),
            "checks" => Some(Self::Checks),
            _ => None,
        }
    }
}

/// 灰色（macOS 版の secondaryLabelColor。明るい外観でも暗い外観でも読める）。
const SECONDARY: &str = "#8a8a8a";
/// 行の前のアイコンの色（文字の色）と、強調の色（テーマに従う）。
const PRIMARY: &str = "{ThemeResource TextFillColorPrimaryBrush}";
const ACCENT: &str = "{ThemeResource AccentTextFillColorPrimaryBrush}";
/// 行の前のアイコンと、右端の印の大きさ（Segoe Fluent Icons の 16px に合わせる）。
const ICON_SIZE: f32 = 16.0;
const MARK_SIZE: f32 = 14.0;

/// XAML の属性の値に入れる文字（`&`・`<`・`>`・`"`）を逃がす。
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 行の中身（XAML）: 前のアイコン、名前（細ければ … で切る）と、右端に寄せた印。押した印は Tag（`mark:…`）で
/// 分かる。
pub fn row_xaml(row: &Row) -> String {
    let mut icons = String::new();
    for m in &row.marks {
        let color = m.color.as_deref().unwrap_or(SECONDARY);
        let inner = match &m.icon {
            Icon::Shared(name) => crate::icons::xaml(name, MARK_SIZE, color),
            Icon::Text(t) => format!(
                r#"<TextBlock Text="{}" FontSize="12" Foreground="{color}" VerticalAlignment="Center"/>"#,
                escape(t)
            ),
        };
        let tag = m
            .mark
            .map(|k| format!(r#" Tag="mark:{}""#, k.key()))
            .unwrap_or_default();
        let tip = if m.tip.is_empty() {
            String::new()
        } else {
            format!(r#" ToolTipService.ToolTip="{}""#, escape(&m.tip))
        };
        icons.push_str(&format!(
            r#"<Border Background="Transparent" Padding="2,0,2,0" VerticalAlignment="Center"{tag}{tip}>{inner}</Border>"#
        ));
    }
    // 見出し（アイコンのない行）は前を空けない。
    let lead = match row.icon {
        Some(name) => format!(
            r#"<Border Width="{ICON_SIZE}" Height="{ICON_SIZE}" Margin="0,0,8,0" VerticalAlignment="Center">{}</Border>"#,
            crate::icons::xaml(name, ICON_SIZE, if row.accent { ACCENT } else { PRIMARY })
        ),
        None => String::new(),
    };
    format!(
        r#"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" HorizontalAlignment="Stretch"><Grid.ColumnDefinitions><ColumnDefinition Width="Auto"/><ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>{lead}<TextBlock Grid.Column="1" Text="{}" TextTrimming="CharacterEllipsis" VerticalAlignment="Center"/><StackPanel Grid.Column="2" Orientation="Horizontal" Spacing="2" Margin="8,0,2,0" VerticalAlignment="Center">{icons}</StackPanel></Grid>"#,
        escape(&row.text)
    )
}

/// 行の中身（鍵 → アイコン・名前・印）。木を作るたびに作り直す。行の部品（TreeViewItem）が並んだ後、その中身を
/// `row_xaml` で作ったものに差し替える（TreeView の項目は文字だけを持つ）。
pub fn row(key: &str) -> Option<Row> {
    ROWS.with(|m| m.borrow().get(key).cloned())
}

/// 木を作った回数（作るたびに行の中身が変わりうる）。
pub fn generation() -> u64 {
    BUILT.with(|b| b.get().0)
}

/// 最後に木を作った時。
pub fn built_at() -> Option<std::time::Instant> {
    BUILT.with(|b| b.get().1)
}

/// アイコンと印のある行（`text` は差し替える前の行の文字にもなる）。
fn row_node(
    key: impl Into<String>,
    icon: Option<&'static str>,
    accent: bool,
    text: impl Into<String>,
    marks: Vec<RowMark>,
) -> TreeNode {
    let key = key.into();
    let text = text.into();
    ROWS.with(|m| {
        m.borrow_mut().insert(
            key.clone(),
            Row {
                icon,
                accent,
                text: text.clone(),
                marks,
            },
        )
    });
    TreeNode::new(key, text)
}

/// アイコンだけの行。
fn icon_node(key: impl Into<String>, icon: &'static str, text: impl Into<String>) -> TreeNode {
    row_node(key, Some(icon), false, text, Vec::new())
}

/// 署名の状態の色（`#rrggbb`。History の行の印と同じ。色のないものは灰色）。
fn status_color(status: crate::signature::Status) -> Option<String> {
    status
        .color()
        .map(|c| format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b))
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
/// `worktrees` は worktree の行（キー・アイコン・開いているものか・見出し。2 つ以上あるときだけ Working Copy の
/// 下に出す）。
/// `form` はリポジトリの形の印（History の行の後ろ・Working Copy の行の後ろ。GTK4 版の `history_marks` と
/// sparse checkout の印）。
pub fn nodes(
    refs: Option<&Refs>,
    groups: Option<&BranchGroups>,
    range_diff: bool,
    worktrees: &[(String, &'static str, bool, String)],
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
    ROWS.with(|m| m.borrow_mut().clear());
    BUILT.with(|b| b.set((b.get().0 + 1, Some(std::time::Instant::now()))));
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
    // リポジトリの形の印（History の右端に shallow・partial、Working Copy の右端に sparse。macOS 版と同じ）。
    let form_mark = |icon: &'static str, tip: String| RowMark {
        mark: None,
        icon: Icon::Shared(icon),
        color: None,
        tip,
    };
    let mut history_marks = Vec::new();
    if form.shallow {
        history_marks.push(form_mark(
            "shallow",
            tr!("Shallow clone: older history isn’t fetched"),
        ));
    }
    if let Some(filter) = &form.partial {
        history_marks.push(form_mark(
            "partial",
            tr!(
                "Partial clone: file contents are fetched when needed ({filter})",
                filter = filter
            ),
        ));
    }
    let working_marks = form
        .sparse
        .iter()
        .map(|s| form_mark("sparse", s.clone()))
        .collect();
    // worktree が 2 つ以上あれば、Working Copy の下に並べる（GTK4 版・macOS 版と同じ。開いているもの・メイン・
    // ほかのアイコンと変更の数つき）。
    let mut working = row_node(
        "page:working-copy",
        Some("working-copy"),
        false,
        "Working Copy",
        working_marks,
    );
    if worktrees.len() > 1 {
        working = working
            .children(worktrees.iter().map(|(key, icon, editing, text)| {
                row_node(
                    key.as_str(),
                    Some(icon),
                    *editing,
                    text.as_str(),
                    Vec::new(),
                )
            }))
            .expanded(true);
    }
    let mut workspace = vec![
        working,
        row_node(
            "page:history",
            Some("history"),
            false,
            "History",
            history_marks,
        ),
    ];
    // コンフリクトのファイルがあるときだけ、History の下に数を添えて（GTK4 版と同じ）。
    if conflicts > 0 {
        workspace.push(icon_node(
            "page:conflicts",
            "conflict",
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
        workspace.push(icon_node(
            "page:stashes",
            "stashes",
            format!("Stashes  {stashes}"),
        ));
    }
    if range_diff {
        workspace.push(icon_node("page:range-diff", "range-diff", "Range Diff"));
    }
    // いつも Workspace の最後に出す（GTK4 版と同じ）。
    workspace.push(icon_node("page:reflog", "reflog", "Reflog"));
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
            // 右端の印: draft と CI（通った 緑・落ちた 赤・走っている 橙。押すとチェックの一覧）。件名は細ければ
            // … で切る。
            let mut marks = Vec::new();
            if pr.is_draft {
                marks.push(RowMark {
                    mark: None,
                    icon: Icon::Text("draft".to_owned()),
                    color: None,
                    tip: String::new(),
                });
            }
            let ci = pr.ci();
            let ci_mark = match ci {
                crate::github::Ci::Passing => Some(("ci-passed", "#2b9a45")),
                crate::github::Ci::Failing => Some(("ci-failed", "#d03333")),
                crate::github::Ci::Pending => Some(("ci-running", "#d96b10")),
                crate::github::Ci::None => None,
            };
            if let Some((name, color)) = ci_mark {
                marks.push(RowMark {
                    mark: Some(Mark::Checks),
                    icon: Icon::Shared(name),
                    color: Some(color.to_owned()),
                    tip: ci.label().to_owned(),
                });
            }
            row_node(
                key,
                Some("pull-request"),
                false,
                format!("#{} {}", pr.number, pr.title),
                marks,
            )
        });
        out.push(group("pull-requests", "Pull Requests", rows).expanded(true));
    }

    // リモートごとにまとめる（origin/feature/x → origin の下の feature/x。その下はフォルダにしない）。
    let mut by_remote: Vec<(&str, Vec<TreeNode>)> = Vec::new();
    for e in of(RefKind::Remote) {
        let (remote, branch) = e.name.split_once('/').unwrap_or((&e.name, &e.name));
        let node = icon_node(ref_key(e), "branch", branch);
        match by_remote.iter_mut().find(|(r, _)| *r == remote) {
            Some((_, nodes)) => nodes.push(node),
            None => by_remote.push((remote, vec![node])),
        }
    }
    by_remote.sort_by(|a, b| a.0.cmp(b.0));
    if !by_remote.is_empty() {
        let remotes = by_remote.into_iter().map(|(remote, nodes)| {
            icon_node(format!("folder:remotes/{remote}"), "remote", remote)
                .children(nodes)
                .expanded(EXPAND.with(|e| e.get()))
        });
        out.push(group("remotes", "Remotes", remotes).expanded(true));
    }

    // タグも `v1/x` のような名前はフォルダにまとめる。
    let mut tags = Tree::default();
    for e in of(RefKind::Tag) {
        let parts: Vec<&str> = e.name.split('/').collect();
        // 右端の印: 署名（状態の形と色）と、リモートの様子（別のタグがある・どのリモートにもない）。macOS 版と同じ。
        let mut marks: Vec<RowMark> = tag_signatures
            .get(&e.name)
            .map(|(status, _)| RowMark {
                mark: Some(Mark::Signature),
                icon: Icon::Shared(status.icon()),
                color: status_color(*status),
                tip: status.label().to_owned(),
            })
            .into_iter()
            .collect();
        match tag_mark(tag_remotes, &e.name) {
            Some("⚠") => marks.push(RowMark {
                mark: Some(Mark::TagRemotes),
                icon: Icon::Shared("remote-conflict"),
                color: Some("#e66100".to_owned()),
                tip: tag_tooltip(tag_remotes, &e.name).join("\n"),
            }),
            Some(_) => marks.push(RowMark {
                mark: Some(Mark::TagRemotes),
                icon: Icon::Shared("upstream-gone"),
                color: None,
                tip: tr!("Not on any remote (as of the last fetch)"),
            }),
            None => {}
        }
        set_tooltip(&ref_key(e), tag_tooltip(tag_remotes, &e.name));
        let name = parts[parts.len() - 1];
        tags.insert(
            &parts,
            row_node(ref_key(e), Some("tag"), false, name, marks),
        );
    }
    let tags = tags.into_nodes("tags");
    if !tags.is_empty() {
        out.push(group("tags", "Tags", tags));
    }

    let submodules: Vec<TreeNode> = of(RefKind::Submodule)
        .map(|e| {
            set_tooltip(&ref_key(e), submodule_tooltip(submodule_states, &e.name));
            row_node(
                ref_key(e),
                Some("submodule"),
                false,
                e.name.as_str(),
                submodule_marks(submodule_states, &e.name),
            )
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
            // squash して取り込んだことは、印にせずツールチップだけに出す（macOS 版・GTK4 版と同じ）。
            Some(icon_node(format!("subtree:{dir}"), "subtree", dir))
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

/// submodule の行の右端の印（GTK4 版の `finish_submodule`）: コンフリクト、取り出していない、記録と違うコミット、
/// 中に変更。
fn submodule_marks(states: &serde_json::Value, path: &str) -> Vec<RowMark> {
    let Some(s) = submodule_state(states, path) else {
        return Vec::new();
    };
    let on = |k: &str| s[k].as_bool() == Some(true);
    let mark = |icon: &'static str, color: Option<&str>, tip: String| RowMark {
        mark: None,
        icon: Icon::Shared(icon),
        color: color.map(str::to_owned),
        tip,
    };
    if on("conflict") {
        return vec![mark("conflict", Some("#d03333"), tr!("In conflict"))];
    }
    if !on("initialized") {
        return vec![mark(
            "not-checked-out",
            None,
            tr!("Not checked out (Update checks it out)"),
        )];
    }
    [
        on("moved").then(|| {
            mark(
                "range-diff",
                Some("#d96b10"),
                tr!("At another commit than the one recorded (Update goes back to it)"),
            )
        }),
        on("changed").then(|| mark("modified", None, tr!("Has uncommitted changes inside"))),
    ]
    .into_iter()
    .flatten()
    .collect()
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

/// 上流との差の札（`↑2 ↓3`。差がなければ None）。
fn tracking_text(e: &RefEntry) -> Option<String> {
    let mut parts = Vec::new();
    if e.ahead > 0 {
        parts.push(format!("↑{}", e.ahead));
    }
    if e.behind > 0 {
        parts.push(format!("↓{}", e.behind));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// ローカルブランチの行（前のアイコン: チェックアウト中は強調の色の今のブランチ、ほかは `icon`（お気に入り・
/// 積んだブランチ）かブランチ。名前と、右端の印: 取り込み済み、上流がなくなった、上流との差。macOS 版と同じく
/// 灰色で、意味はツールチップ。押すとブランチの様子の吹き出し）。`title` は見せる名前（フォルダの中なら最後の
/// 部分）。
fn branch_node(
    e: &RefEntry,
    title: &str,
    head: Option<&str>,
    icon: Option<&'static str>,
) -> TreeNode {
    let mut marks = Vec::new();
    // 取り込み済み（既定のブランチ、squash・rebase でマージしたプルリクエスト）。
    if let Some(tip) = MERGED.with(|m| m.borrow().get(&e.name).cloned()) {
        set_tooltip(&ref_key(e), vec![tip.clone()]);
        marks.push(RowMark {
            mark: Some(Mark::Branch),
            icon: Icon::Shared("merge"),
            color: None,
            tip,
        });
    }
    if e.gone {
        marks.push(RowMark {
            mark: Some(Mark::Branch),
            icon: Icon::Shared("upstream-gone"),
            color: None,
            tip: tr!(
                "Its upstream {upstream} no longer exists on the remote",
                upstream = e.detail.clone().unwrap_or_default()
            ),
        });
    }
    // 上流との差は右端の札に（macOS 版のバッジと同じく、押すとブランチの様子の吹き出し）。
    if let Some(counts) = tracking_text(e) {
        marks.push(RowMark {
            mark: Some(Mark::Branch),
            icon: Icon::Text(counts),
            color: None,
            tip: String::new(),
        });
    }
    let current = head == Some(e.name.as_str());
    let icon = if current {
        "current-branch"
    } else {
        icon.unwrap_or("branch")
    };
    row_node(ref_key(e), Some(icon), current, title, marks)
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
                    icon_node(format!("folder:{path}"), "folder", name)
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
            tree.insert(&parts, branch_node(e, parts[parts.len() - 1], head, None));
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
            attach(
                branch_node(child, &child.name, head, Some("stacked")),
                &child.name,
                stacked,
                head,
                depth + 1,
            )
        }))
    }
    let tree = |e: &RefEntry, icon: Option<&'static str>| {
        let node = branch_node(e, &e.name, head, icon);
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
    let mut rows: Vec<TreeNode> = stars.iter().map(|e| tree(e, Some("favorite"))).collect();
    top.sort_by_key(|e| Some(e.name.as_str()) != stack_root.as_deref());
    rows.extend(top.iter().map(|e| tree(e, None)));
    // 動いているブランチは新しい順。
    newest(&mut active);
    let mut active_tree = Tree::default();
    for e in &active {
        let parts: Vec<&str> = e.name.split('/').collect();
        let node = branch_node(e, parts[parts.len() - 1], head, None);
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
                icon_node(
                    format!("folder:branches/#{key}"),
                    "folder",
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
    fn rows_become_xaml() {
        let row = Row {
            icon: Some("branch"),
            accent: true,
            text: "feat/<x> & \"y\"".into(),
            marks: vec![
                RowMark {
                    mark: Some(Mark::Branch),
                    icon: Icon::Shared("merge"),
                    color: None,
                    tip: "Merged into main".into(),
                },
                RowMark {
                    mark: None,
                    icon: Icon::Text("draft".into()),
                    color: Some("#2b9a45".into()),
                    tip: String::new(),
                },
                RowMark {
                    mark: Some(Mark::TagRemotes),
                    icon: Icon::Shared("upstream-gone"),
                    color: None,
                    tip: "a \"b\" <c> & d".into(),
                },
            ],
        };
        let xaml = row_xaml(&row);
        assert!(xaml.contains(r#"Text="feat/&lt;x&gt; &amp; &quot;y&quot;""#));
        assert!(xaml.contains(r#"Tag="mark:branch""#));
        assert!(xaml.contains("<Path "));
        assert!(xaml.contains(ACCENT));
        assert!(xaml.contains(r#"ToolTipService.ToolTip="a &quot;b&quot; &lt;c&gt; &amp; d""#));
        assert_eq!(Mark::from_tag("mark:checks"), Some(Mark::Checks));
        assert_eq!(Mark::from_tag("checks"), None);
        // 見出しには前のアイコンを出さない。
        let plain = Row {
            icon: None,
            accent: false,
            text: "Branches".into(),
            marks: Vec::new(),
        };
        assert!(!row_xaml(&plain).contains("<Path "));
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
        let icon = |key: &str| row(key).and_then(|r| r.icon);
        assert!(rows[0].contains("star"));
        assert_eq!(icon("ref:0:star"), Some("favorite"));
        assert!(rows[1].contains("main"));
        assert_eq!(icon("ref:0:main"), Some("current-branch"));
        assert!(row("ref:0:main").unwrap().accent);
        // feature/b は feature/a の上に積んである。
        assert!(rows[2].contains("folder:branches/feature") && rows[2].contains("feature/b"));
        assert_eq!(icon("ref:0:feature/b"), Some("stacked"));
        assert_eq!(icon("folder:branches/feature"), Some("folder"));
        assert!(rows[3].contains("Inactive  1") && rows[3].contains("old"));
        assert!(rows[4].contains("Archived  1") && rows[4].contains("gone-by"));
    }
}
