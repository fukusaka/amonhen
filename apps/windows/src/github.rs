//! GitHub（GTK4 版の `github.rs`、macOS 版の `GitHub.swift`）: リモートの URL から GitHub のリポジトリを見分け、
//! 開いているプルリクエスト（サイドバーの Pull Requests）、マージされたプルリクエスト（squash・rebase で取り込んだ
//! ブランチの印と Clean Up Branches）、History の行の CI の状態とコミットの作者のアバターの URL（GraphQL で 1 回に
//! 読む）、CI の吹き出しのチェック、プルリクエストの作成。手元の `gh`（GitHub CLI）を起動して読み書きし、ログインと
//! トークンは gh に任せる（アプリはトークンに触れない）。gh がなければ連携の機能を出さない。
//!
//! ここの読み書きはどれも待つ（裏のスレッドで呼ぶ）。

use std::collections::HashMap;
use std::sync::Mutex;

use amonhen_core::Repository;
use amonhen_model::{Oid, tr};
use serde_json::{Value, json};

/// GitHub のリポジトリ（ホスト、持ち主、名前）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubRepository {
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl GitHubRepository {
    /// `https://github.com/o/r.git`・`git@github.com:o/r.git`・`ssh://git@github.com/o/r` を読む。ホストの名前に
    /// "github" を含むものだけ（GitHub Enterprise を含む）。
    pub fn parse(url: &str) -> Option<Self> {
        let url = url.trim();
        let (host, rest) = if let Some((_, after)) = url.split_once("://") {
            let after = match after.split_once('@') {
                Some((user, rest)) if !user.contains('/') => rest,
                _ => after,
            };
            after.split_once('/')?
        } else {
            // scp 形式: user@host:owner/repo
            let (host, rest) = url.split_once(':')?;
            (host.rsplit('@').next().unwrap_or(host), rest)
        };
        let host = host.split(':').next().unwrap_or(host);
        if !host.to_lowercase().contains("github") {
            return None;
        }
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            return None;
        }
        Some(Self {
            host: host.to_owned(),
            owner: parts[parts.len() - 2].to_owned(),
            name: parts[parts.len() - 1].to_owned(),
        })
    }

    /// `gh` の `-R` に渡す形。
    pub fn slug(&self) -> String {
        if self.host == "github.com" {
            format!("{}/{}", self.owner, self.name)
        } else {
            format!("{}/{}/{}", self.host, self.owner, self.name)
        }
    }

    pub fn web_url(&self) -> String {
        format!("https://{}/{}/{}", self.host, self.owner, self.name)
    }

    /// `gh api` に渡すホスト（github.com なら要らない）。
    fn hostname_args(&self) -> Vec<String> {
        if self.host == "github.com" {
            Vec::new()
        } else {
            vec!["--hostname".into(), self.host.clone()]
        }
    }
}

/// PATH のほかに gh を探す場所（インストーラーの既定の場所。アプリを起動した後に入れると PATH にまだ入っていない）。
fn gh_dirs() -> Vec<std::path::PathBuf> {
    ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .flat_map(|base| {
            let base = std::path::PathBuf::from(base);
            [
                base.join("GitHub CLI"),
                base.join("Programs").join("GitHub CLI"),
            ]
        })
        .collect()
}

/// gh の場所（PATH、なければよくある場所から。最初に一度だけ探す）。
fn gh_path() -> Option<&'static std::path::Path> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let from_path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default();
        from_path
            .into_iter()
            .chain(gh_dirs())
            .map(|d| d.join("gh.exe"))
            .find(|p| p.is_file())
    })
    .as_deref()
}

/// `gh` が使えるか。
pub fn available() -> bool {
    gh_path().is_some()
}

/// gh を動かす `Command`（見つけた場所の gh。尋ねてこないように、色を付けないように）。gh がなければ None。
fn gh_command() -> Option<std::process::Command> {
    let mut command = amonhen_model::process::command(gh_path()?);
    command.env("GH_PROMPT_DISABLED", "1").env("NO_COLOR", "1");
    Some(command)
}

/// Activity の Performance に出す名前（macOS 版と同じ「GitHub: gh <サブコマンド>」。記録は `&'static str`）。
fn metric(subcommand: &str) -> &'static str {
    match subcommand {
        "api" => "GitHub: gh api",
        "pr" => "GitHub: gh pr",
        "repo" => "GitHub: gh repo",
        "issue" => "GitHub: gh issue",
        _ => "GitHub: gh",
    }
}

/// `gh` を `dir` で動かし、標準出力を返す。失敗したら標準エラー（空なら「gh … が失敗しました」）。待つ。
pub fn gh(args: &[&str], dir: Option<&std::path::Path>) -> Result<String, String> {
    let Some(mut command) = gh_command() else {
        return Err(tr!(
            "gh (GitHub CLI) wasn’t found. Install it with brew install gh, then sign in with gh auth login."
        ));
    };
    command.args(args);
    if let Some(d) = dir {
        command.current_dir(d);
    }
    let started = std::time::Instant::now();
    let out = command.output();
    crate::activity::record(
        metric(args.first().copied().unwrap_or_default()),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    let out = out.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        let message = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if message.is_empty() {
            tr!("gh {args} failed.", args = args.join(" "))
        } else {
            message
        })
    }
}

fn gh_owned(args: &[String], dir: Option<&std::path::Path>) -> Result<String, String> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    gh(&refs, dir)
}

/// リポジトリの GitHub（origin、なければ最初のリモート）と、そのリモートの名前（fork のとき、プルリクエストの
/// ブランチをここへ送る）。
pub fn of_remote(repo: &Repository) -> Option<(String, GitHubRepository)> {
    let reply: Value = serde_json::from_str(&repo.run_json(r#"{"op":"remotes"}"#)).ok()?;
    let remotes = reply["info"]["remotes"].as_array()?;
    let remote = remotes
        .iter()
        .find(|r| r["name"].as_str() == Some("origin"))
        .or(remotes.first())?;
    let github = GitHubRepository::parse(remote["fetchUrl"].as_str()?)?;
    Some((remote["name"].as_str()?.to_owned(), github))
}

/// 組織の fork のとき、元のリポジトリから読むプルリクエストの数（macOS 版と同じ）。
const ORGANIZATION_FORK_LIMIT: &str = "300";

/// プルリクエストを出す先。origin が GitHub の fork なら、その元のリポジトリ（`fork_owner` は fork の持ち主。
/// head は `持ち主:ブランチ` の形で渡し、一覧はその fork から出したものだけにする）。fork でなければ origin。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestTarget {
    pub repo: GitHubRepository,
    pub fork_owner: Option<String>,
    /// fork の持ち主が組織（プルリクエストの作者は人なので、作者では絞れない）。
    pub fork_in_organization: bool,
}

impl PullRequestTarget {
    /// fork から元のリポジトリへ出す。既定のブランチ（fork の main など）からも出せる。
    pub fn is_fork(&self) -> bool {
        self.fork_owner.is_some()
    }

    /// `gh pr create --head` に渡す形。
    pub fn head(&self, branch: &str) -> String {
        match &self.fork_owner {
            Some(owner) => format!("{owner}:{branch}"),
            None => branch.to_owned(),
        }
    }

    /// `pr list` の答えのうち、この先に出したもの（fork なら、その fork から出したものだけ）。
    fn mine(&self, list: Vec<Value>) -> Vec<Value> {
        let Some(owner) = &self.fork_owner else {
            return list;
        };
        list.into_iter()
            .filter(|p| {
                p["headRepositoryOwner"]["login"]
                    .as_str()
                    .is_some_and(|o| o.eq_ignore_ascii_case(owner))
            })
            .collect()
    }

    /// `pr list` の、fork のときの引数（その fork の持ち主が出したものに絞り、head の持ち主も読む）。組織の fork は
    /// 作者で絞れないので、多めに読んで [`Self::mine`] で絞る（GTK4 版と同じ）。
    fn list_args(&self, state: &str, limit: &str, fields: &str) -> Vec<String> {
        let mut fields = fields.to_owned();
        let limit = if self.fork_in_organization {
            ORGANIZATION_FORK_LIMIT
        } else {
            limit
        };
        let mut args = [
            "pr",
            "list",
            "-R",
            &self.repo.slug(),
            "--state",
            state,
            "--limit",
            limit,
        ]
        .map(str::to_owned)
        .to_vec();
        if let Some(owner) = &self.fork_owner {
            fields.push_str(",headRepositoryOwner");
            if !self.fork_in_organization {
                args.extend(["--author".to_owned(), owner.clone()]);
            }
        }
        args.extend(["--json".to_owned(), fields]);
        args
    }
}

/// `gh repo view --json parent` の答えから、元のリポジトリ（同じホスト。fork でなければ None）。
fn parent_from(view: &Value, host: &str) -> Option<GitHubRepository> {
    let parent = &view["parent"];
    Some(GitHubRepository {
        host: host.to_owned(),
        owner: parent["owner"]["login"].as_str()?.to_owned(),
        name: parent["name"].as_str()?.to_owned(),
    })
}

/// 読めた fork の元（slug → 元と、持ち主が組織か。fork でなければ None）。読めなかったものは覚えない。
type Parents = HashMap<String, Option<(GitHubRepository, bool)>>;
static PARENTS: Mutex<Option<Parents>> = Mutex::new(None);

/// `origin` のリポジトリからプルリクエストを出す先（fork なら元のリポジトリ。macOS 版と同じ）。
pub fn pull_request_target(origin: &GitHubRepository) -> PullRequestTarget {
    let slug = origin.slug();
    let known = PARENTS
        .lock()
        .ok()
        .and_then(|p| p.as_ref().and_then(|p| p.get(&slug).cloned()));
    let parent = match known {
        Some(parent) => parent,
        None => {
            let read = gh(
                &["repo", "view", &slug, "--json", "parent,isInOrganization"],
                None,
            )
            .ok()
            .and_then(|out| serde_json::from_str::<Value>(&out).ok())
            .map(|view| {
                let organization = view["isInOrganization"].as_bool() == Some(true);
                parent_from(&view, &origin.host).map(|parent| (parent, organization))
            });
            if let Some(parent) = &read
                && let Ok(mut p) = PARENTS.lock()
            {
                p.get_or_insert_with(HashMap::new)
                    .insert(slug, parent.clone());
            }
            read.flatten()
        }
    };
    match parent {
        Some((repo, fork_in_organization)) => PullRequestTarget {
            repo,
            fork_owner: Some(origin.owner.clone()),
            fork_in_organization,
        },
        None => PullRequestTarget {
            repo: origin.clone(),
            fork_owner: None,
            fork_in_organization: false,
        },
    }
}

/// ブランチのページの URL の後ろ。`#` や `?` を URL の区切りにしない（`/` はそのまま。macOS 版と同じ）。
pub fn tree_path(branch: &str) -> String {
    let mut out = String::from("/tree/");
    for b in branch.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// CI の状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ci {
    Passing,
    Failing,
    Pending,
    None,
}

impl Ci {
    /// 吹き出しやツールチップの文（macOS 版の `CIPopover.look`）。
    pub fn label(self) -> String {
        match self {
            Self::Passing => tr!("CI: passed"),
            Self::Failing => tr!("CI: failed"),
            Self::Pending => tr!("CI: running"),
            Self::None => tr!("No CI"),
        }
    }

    pub fn mark(self) -> &'static str {
        match self {
            Self::Passing => "✓",
            Self::Failing => "✗",
            Self::Pending => "●",
            Self::None => "",
        }
    }
}

/// プルリクエストの `statusCheckRollup` の 1 つ。CheckRun なら status・conclusion、StatusContext なら state。
#[derive(Clone, Debug, PartialEq)]
struct RollupCheck {
    status: Option<String>,
    conclusion: Option<String>,
    state: Option<String>,
}

/// プルリクエスト（`gh pr list --json`）。
#[derive(Clone, Debug, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub is_draft: bool,
    pub author: String,
    pub review_decision: Option<String>,
    checks: Option<Vec<RollupCheck>>,
    /// head のコミット ID（CI の吹き出しで、そのコミットのチェックを読む）。
    pub head_ref_oid: Option<String>,
}

impl PullRequest {
    fn from(v: &Value) -> Option<Self> {
        let s = |key: &str| v[key].as_str().map(str::to_owned);
        let checks = v["statusCheckRollup"].as_array().map(|list| {
            list.iter()
                .map(|c| {
                    let f = |key: &str| c[key].as_str().map(str::to_owned);
                    RollupCheck {
                        status: f("status"),
                        conclusion: f("conclusion"),
                        state: f("state"),
                    }
                })
                .collect()
        });
        Some(Self {
            number: v["number"].as_u64()?,
            title: s("title")?,
            url: s("url").unwrap_or_default(),
            head_ref_name: s("headRefName").unwrap_or_default(),
            base_ref_name: s("baseRefName").unwrap_or_default(),
            is_draft: v["isDraft"].as_bool().unwrap_or(false),
            author: v["author"]["login"].as_str().unwrap_or_default().to_owned(),
            review_decision: s("reviewDecision").filter(|r| !r.is_empty()),
            checks,
            head_ref_oid: s("headRefOid"),
        })
    }

    /// CI の状態: 1 つでも失敗なら失敗、走っているものがあれば途中、すべて成功なら成功。
    pub fn ci(&self) -> Ci {
        let Some(checks) = self.checks.as_ref().filter(|c| !c.is_empty()) else {
            return Ci::None;
        };
        let results: Vec<String> = checks
            .iter()
            .map(|c| {
                c.conclusion
                    .as_deref()
                    .or(c.state.as_deref())
                    .unwrap_or_default()
                    .to_uppercase()
            })
            .collect();
        if results.iter().any(|r| {
            matches!(
                r.as_str(),
                "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
            )
        }) {
            return Ci::Failing;
        }
        if checks.iter().any(|c| {
            c.status.as_deref().unwrap_or_default().to_uppercase() != "COMPLETED"
                && c.state.is_none()
        }) || results
            .iter()
            .any(|r| matches!(r.as_str(), "PENDING" | "EXPECTED" | ""))
        {
            return Ci::Pending;
        }
        Ci::Passing
    }

    /// レビューの様子の言葉（なければ空）。
    pub fn review_label(&self) -> String {
        match self.review_decision.as_deref() {
            Some("APPROVED") => tr!("Approved"),
            Some("CHANGES_REQUESTED") => tr!("Changes requested"),
            Some("REVIEW_REQUIRED") => tr!("Review required"),
            _ => String::new(),
        }
    }

    /// サイドバーの行のツールチップ（番号と題、作者とブランチ、CI とレビュー）。
    pub fn tooltip(&self) -> String {
        let state: Vec<String> = [self.ci().label(), self.review_label()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        format!(
            "#{} {}\n{} · {} → {}\n{}",
            self.number,
            self.title,
            self.author,
            self.head_ref_name,
            self.base_ref_name,
            state.join(" · ")
        )
    }
}

/// 1 つのコミットのチェック 1 つ（GitHub Actions のジョブ、または外部の status）。
#[derive(Clone, Debug, PartialEq)]
pub struct CommitCheck {
    pub name: String,
    /// CheckRun なら conclusion（走っている途中は status）、status context なら state。小文字。
    pub result: String,
    pub url: Option<String>,
    /// かかった時間（秒。終わっていなければ None）。
    pub seconds: Option<i64>,
}

impl CommitCheck {
    /// 吹き出しの行の見た目に使う状態。取り消しは失敗ではない（やり直す前の run や、上書きされた run）。
    pub fn ci(&self) -> Ci {
        match self.result.as_str() {
            "success" => Ci::Passing,
            "failure" | "error" | "timed_out" | "action_required" | "startup_failure" | "stale" => {
                Ci::Failing
            }
            "queued" | "in_progress" | "pending" | "waiting" | "requested" | "expected" => {
                Ci::Pending
            }
            _ => Ci::None,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.result == "cancelled"
    }

    /// 同じ名前のチェック（やり直しや上書きで run が増えたもの）を 1 つにまとめる。成功があれば成功 1 つ、なければ
    /// 失敗、走っている途中、取り消し、その他の順で代表を選ぶ。順は読んだ順（新しい run が先）。
    pub fn collapsed(checks: &[CommitCheck]) -> Vec<CommitCheck> {
        let rank = |c: &CommitCheck| match c.ci() {
            Ci::Passing => 0,
            Ci::Failing => 1,
            Ci::Pending => 2,
            Ci::None if c.is_cancelled() => 3,
            Ci::None => 4,
        };
        let mut names: Vec<&str> = Vec::new();
        let mut best: HashMap<&str, &CommitCheck> = HashMap::new();
        for check in checks {
            match best.get(check.name.as_str()) {
                None => {
                    names.push(&check.name);
                    best.insert(&check.name, check);
                }
                // 同じ順位なら先のもの（新しい run）を残す。
                Some(b) if rank(check) < rank(b) => {
                    best.insert(&check.name, check);
                }
                Some(_) => {}
            }
        }
        names.iter().map(|n| best[n].clone()).collect()
    }

    pub fn result_label(&self) -> String {
        match self.result.as_str() {
            "success" => tr!("passed"),
            "failure" | "error" | "startup_failure" => tr!("failed"),
            "cancelled" => tr!("cancelled"),
            "timed_out" => tr!("timed out"),
            "skipped" => tr!("skipped"),
            "neutral" => tr!("neutral"),
            "action_required" => tr!("action required"),
            "stale" => tr!("stale"),
            "queued" | "waiting" | "requested" | "expected" => tr!("queued"),
            "in_progress" | "pending" => tr!("running"),
            other => other.to_owned(),
        }
    }
}

/// 開いているプルリクエスト（新しい順、最大 50 件。fork なら、その fork から出したもの）。
pub fn pull_requests(target: &PullRequestTarget) -> Result<Vec<PullRequest>, String> {
    let args = target.list_args(
        "open",
        "50",
        "number,title,url,headRefName,baseRefName,isDraft,author,reviewDecision,statusCheckRollup,headRefOid",
    );
    let out = gh_owned(&args, None)?;
    let list: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(target
        .mine(list.as_array().cloned().unwrap_or_default())
        .iter()
        .filter_map(PullRequest::from)
        .collect())
}

/// マージされたプルリクエスト（新しい順、最大 200 件。fork なら、その fork から出したもの）を、コアの
/// `branchGroups`・`branchCleanup` の `pullRequests` の形で（macOS 版と同じ）。
pub fn merged_pull_requests(target: &PullRequestTarget) -> Result<Vec<Value>, String> {
    let args = target.list_args("merged", "200", "number,headRefName,headRefOid");
    let out = gh_owned(&args, None)?;
    let list: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(target
        .mine(list.as_array().cloned().unwrap_or_default())
        .iter()
        .map(|p| json!({ "number": p["number"], "branch": p["headRefName"], "oid": p["headRefOid"] }))
        .collect())
}

/// 既定のブランチ（プルリクエストの base の初期値）。
pub fn default_branch(repo: &GitHubRepository) -> Option<String> {
    gh(
        &[
            "repo",
            "view",
            &repo.slug(),
            "--json",
            "defaultBranchRef",
            "-q",
            ".defaultBranchRef.name",
        ],
        None,
    )
    .ok()
    .filter(|b| !b.is_empty())
}

/// ブランチ `branches`（GitHub の先）とコミット `tips`（16 進。手元の先）から数えた直近のコミット（それぞれ `count`
/// 個まで）の CI の状態（`ci` のとき）と、作者の GitHub のアバターの URL（`authors` のとき。メールアドレスの小文字
/// → URL）。GraphQL で 1 回に読む。GitHub にないブランチ・コミット（push していない）は飛ばす（GTK4 版と同じ）。
pub fn commit_statuses(
    repo: &GitHubRepository,
    branches: &[String],
    tips: &[String],
    count: u32,
    ci: bool,
    authors: bool,
) -> Result<Statuses, String> {
    if (branches.is_empty() && tips.is_empty()) || !(ci || authors) {
        return Ok(Default::default());
    }
    // 要るものだけを頼む（量を減らす）。
    let wanted = [
        Some("oid"),
        authors.then_some("author { email avatarUrl(size: 96) }"),
        ci.then_some("statusCheckRollup { state }"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let history = format!("... on Commit {{ history(first: {count}) {{ nodes {{ {wanted} }} }} }}");
    // ブランチ・コミットごとに別名を付けて並べる（b0・b1 …、c0・c1 …）。
    let fields = (0..branches.len())
        .map(|i| format!("b{i}: ref(qualifiedName: $r{i}) {{ target {{ {history} }} }}"))
        .chain((0..tips.len()).map(|i| format!("c{i}: object(oid: $o{i}) {{ {history} }}")))
        .collect::<Vec<_>>()
        .join("\n");
    let variables = (0..branches.len())
        .map(|i| format!("$r{i}: String!"))
        .chain((0..tips.len()).map(|i| format!("$o{i}: GitObjectID!")))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "query($owner: String!, $name: String!, {variables}) {{ repository(owner: $owner, name: $name) {{ {fields} }} }}"
    );
    let mut args: Vec<String> = vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={query}"),
        "-f".into(),
        format!("owner={}", repo.owner),
        "-f".into(),
        format!("name={}", repo.name),
    ];
    args.extend(repo.hostname_args());
    for (i, branch) in branches.iter().enumerate() {
        args.extend(["-f".into(), format!("r{i}=refs/heads/{branch}")]);
    }
    for (i, tip) in tips.iter().enumerate() {
        args.extend(["-f".into(), format!("o{i}={tip}")]);
    }
    let out = gh_owned(&args, None)?;
    let response: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(statuses_from(&response))
}

/// コミットごとの CI の状態と、作者のアバターの URL（メールアドレスの小文字 → URL）。
type Statuses = (HashMap<Oid, Ci>, HashMap<String, String>);

/// [`commit_statuses`] の答えから、コミットごとの CI の状態と作者のアバターの URL。
fn statuses_from(response: &Value) -> Statuses {
    let mut statuses = HashMap::new();
    let mut avatars = HashMap::new();
    for r in response["data"]["repository"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
    {
        // ブランチ（b…）は `target` の下に、コミット（c…）はそのまま `history` がある。
        let history = if r["target"].is_null() {
            &r["history"]
        } else {
            &r["target"]["history"]
        };
        for node in history["nodes"].as_array().into_iter().flatten() {
            let Some(oid) = node["oid"].as_str().and_then(Oid::from_hex) else {
                continue;
            };
            if let Some(state) = node["statusCheckRollup"]["state"].as_str() {
                statuses.insert(
                    oid,
                    match state {
                        "SUCCESS" => Ci::Passing,
                        "FAILURE" | "ERROR" => Ci::Failing,
                        _ => Ci::Pending,
                    },
                );
            }
            let email = node["author"]["email"].as_str().unwrap_or_default();
            if let Some(url) = node["author"]["avatarUrl"].as_str()
                && !email.is_empty()
            {
                avatars.insert(email.to_lowercase(), url.to_owned());
            }
        }
    }
    (statuses, avatars)
}

/// ISO 8601 の UTC の時刻（`2026-11-21T03:04:05Z`）を UNIX 時間に。
fn unix_time(text: &str) -> Option<i64> {
    let (date, time) = text.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|p| p.parse::<f64>().ok());
    let (h, min, s) = (t.next()??, t.next()??, t.next()??);
    // 1970-01-01 からの日数（グレゴリオ暦。Howard Hinnant の days_from_civil）。
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + (h * 3600.0 + min * 60.0 + s) as i64)
}

/// `sha` のコミットのチェック（Actions の check run と、外部の status）。印を押したときに読む。
pub fn commit_checks(repo: &GitHubRepository, sha: &str) -> Result<Vec<CommitCheck>, String> {
    let base = format!("repos/{}/{}/commits/{sha}", repo.owner, repo.name);
    let runs_args = [
        vec!["api".into(), format!("{base}/check-runs?per_page=100")],
        repo.hostname_args(),
    ]
    .concat();
    let status_args = [
        vec!["api".into(), format!("{base}/status")],
        repo.hostname_args(),
    ]
    .concat();
    // 2 つを並べて読む。
    let (runs, status) = std::thread::scope(|s| {
        let runs = s.spawn(|| gh_owned(&runs_args, None));
        let status = gh_owned(&status_args, None);
        (runs.join().unwrap_or_else(|_| Err(String::new())), status)
    });
    let runs: Value = serde_json::from_str(&runs?).map_err(|e| e.to_string())?;
    let status: Value = serde_json::from_str(&status?).map_err(|e| e.to_string())?;
    let time = |v: &Value| v.as_str().and_then(unix_time);
    let mut out = Vec::new();
    for run in runs["check_runs"].as_array().into_iter().flatten() {
        let result = run["conclusion"]
            .as_str()
            .or(run["status"].as_str())
            .unwrap_or_default()
            .to_lowercase();
        let seconds = time(&run["started_at"])
            .zip(time(&run["completed_at"]))
            .map(|(s, e)| e - s);
        out.push(CommitCheck {
            name: run["name"].as_str().unwrap_or_default().to_owned(),
            result,
            url: run["html_url"].as_str().map(str::to_owned),
            seconds,
        });
    }
    for context in status["statuses"].as_array().into_iter().flatten() {
        out.push(CommitCheck {
            name: context["context"].as_str().unwrap_or_default().to_owned(),
            result: context["state"].as_str().unwrap_or_default().to_lowercase(),
            url: context["target_url"].as_str().map(str::to_owned),
            seconds: None,
        });
    }
    Ok(out)
}

/// 窓が GitHub から読んだもの（GTK4 版の `Loader` が持つもの）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    /// origin（なければ最初のリモート）が GitHub なら、そのリポジトリとリモートの名前。
    pub github: Option<(String, GitHubRepository)>,
    /// プルリクエストを出す先（fork なら元のリポジトリ）。
    pub target: Option<PullRequestTarget>,
    /// 開いているプルリクエスト。
    pub pull_requests: Vec<PullRequest>,
    /// マージされたプルリクエスト（`branchGroups` の `pullRequests` の形。読めなければ None）。
    pub merged: Option<Vec<Value>>,
    /// History の行の CI の状態（読んでいなければ None。そのときは前のものを残す）。
    pub ci: Option<HashMap<Oid, Ci>>,
    /// コミットの作者の GitHub のアバターの URL（メールアドレスの小文字 → URL）。
    pub avatars: HashMap<String, String>,
    /// 既定のブランチ（CI を読むブランチ）。
    pub default_branch: Option<String>,
    /// 読めなかったこと（Activity の Events に出す）。
    pub errors: Vec<String>,
}

/// 読み方（Settings > Accounts の選び）。
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub ci: bool,
    pub authors: bool,
    pub with_default: bool,
    pub count: u32,
}

/// 窓ごとに GitHub から読むもの（macOS 版の `RepositoryWindow.loadGitHub`、GTK4 版の `Loader::load`）: 開いている
/// プルリクエスト、マージされたプルリクエスト、今のブランチと既定のブランチの直近のコミットの CI の状態と作者の
/// アバターの URL。`head` は今のブランチ、`default_branch` は前に読んだ既定のブランチ。待つ。
pub fn load(
    repo: &Repository,
    head: Option<String>,
    default_branch: Option<String>,
    options: Options,
) -> Loaded {
    let mut out = Loaded::default();
    let Some((remote, github)) = of_remote(repo) else {
        return out;
    };
    let target = pull_request_target(&github);
    out.github = Some((remote.clone(), github.clone()));
    out.target = Some(target.clone());
    match pull_requests(&target) {
        Ok(list) => out.pull_requests = list,
        Err(e) => out
            .errors
            .push(tr!("Can’t read pull requests: {error}", error = e)),
    }
    // マージされたプルリクエストも同じときに読む（squash・rebase で取り込んだブランチに印を出す）。
    out.merged = merged_pull_requests(&target).ok();
    if !options.ci && !options.authors {
        out.ci = options.ci.then(HashMap::new);
        return out;
    }
    out.default_branch = default_branch;
    if options.with_default && out.default_branch.is_none() {
        out.default_branch = self::default_branch(&github);
    }
    let mut branches: Vec<String> = [
        head,
        options
            .with_default
            .then(|| out.default_branch.clone())
            .flatten(),
    ]
    .into_iter()
    .flatten()
    .collect();
    branches.sort();
    branches.dedup();
    let tips = ci_tips(repo, &branches, Some(&remote));
    match commit_statuses(
        &github,
        &branches,
        &tips,
        options.count,
        options.ci,
        options.authors,
    ) {
        Ok((statuses, avatars)) => {
            out.ci = Some(if options.ci { statuses } else { HashMap::new() });
            out.avatars = avatars;
        }
        Err(e) => out
            .errors
            .push(tr!("Can’t read the CI status: {error}", error = e)),
    }
    out
}

/// CI の状態を読み始めるコミット（16 進）: ブランチの手元の先と、その上流（なければ GitHub のリモート `remote` の同じ
/// 名前）の先。手元が遅れていても History に出ている行に、手元が進んでいても push した分に印が付く（macOS 版と同じ）。
fn ci_tips(repo: &Repository, branches: &[String], remote: Option<&str>) -> Vec<String> {
    use amonhen_model::RefKind;
    let Ok(refs) = repo.load_refs() else {
        return Vec::new();
    };
    let entries = &refs.refs.entries;
    let target = |kind: RefKind, name: &str| {
        entries
            .iter()
            .find(|i| i.kind == kind && i.name == name)
            .and_then(|i| i.target)
    };
    let mut tips = Vec::new();
    for branch in branches {
        let local = entries
            .iter()
            .find(|i| i.kind == RefKind::Local && &i.name == branch);
        let upstream = match local {
            Some(l) if l.gone => None,
            Some(l) => l.detail.clone(),
            None => None,
        }
        .or_else(|| remote.map(|r| format!("{r}/{branch}")));
        for tip in [
            local.and_then(|l| l.target),
            upstream.and_then(|u| target(RefKind::Remote, &u)),
        ]
        .into_iter()
        .flatten()
        {
            let tip = tip.to_string();
            if !tips.contains(&tip) {
                tips.push(tip);
            }
        }
    }
    tips
}

/// 自分の GitHub のリポジトリ（`nameWithOwner` と URL。名前の順、最大 200 件。File > Clone… で選ぶ。GTK4 版の
/// `repos::clone`）。読めなければ空。待つ。
pub fn my_repositories() -> Vec<(String, String)> {
    let mut repos: Vec<(String, String)> = gh(
        &[
            "repo",
            "list",
            "--json",
            "nameWithOwner,url",
            "--limit",
            "200",
        ],
        None,
    )
    .ok()
    .and_then(|out| serde_json::from_str::<Value>(&out).ok())
    .and_then(|v| v.as_array().cloned())
    .into_iter()
    .flatten()
    .filter_map(|r| {
        Some((
            r["nameWithOwner"].as_str()?.to_owned(),
            r["url"].as_str()?.to_owned(),
        ))
    })
    .collect();
    repos.sort_by_key(|r| r.0.to_lowercase());
    repos
}

/// #123 の候補（番号とタイトル）と、読んだ時刻。
type IssueCache = (std::time::Instant, Vec<(u64, String)>);
static ISSUES: Mutex<Option<HashMap<String, IssueCache>>> = Mutex::new(None);

/// コミットのメッセージの #123 の候補: 開いている Issue（30 件）と PR（「PR: 」を付けて）を、番号の大きい順に。
/// 5 分は覚えておく（macOS 版と同じ）。待つ。
pub fn issue_candidates(target: &PullRequestTarget) -> Vec<(u64, String)> {
    let slug = target.repo.slug();
    let cached = ISSUES.lock().ok().and_then(|i| {
        i.as_ref()?
            .get(&slug)
            .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(300))
            .map(|(_, items)| items.clone())
    });
    if let Some(items) = cached {
        return items;
    }
    let issues: Vec<(u64, String)> = gh(
        &[
            "issue",
            "list",
            "-R",
            &slug,
            "--state",
            "open",
            "--limit",
            "30",
            "--json",
            "number,title",
        ],
        None,
    )
    .ok()
    .and_then(|out| serde_json::from_str::<Value>(&out).ok())
    .and_then(|v| v.as_array().cloned())
    .into_iter()
    .flatten()
    .filter_map(|i| Some((i["number"].as_u64()?, i["title"].as_str()?.to_owned())))
    .collect();
    let pulls = pull_requests(target).unwrap_or_default();
    let mut items: Vec<(u64, String)> = issues
        .into_iter()
        .chain(
            pulls
                .into_iter()
                .map(|p| (p.number, format!("PR: {}", p.title))),
        )
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.0));
    if let Ok(mut i) = ISSUES.lock() {
        i.get_or_insert_with(HashMap::new)
            .insert(slug, (std::time::Instant::now(), items.clone()));
    }
    items
}

/// かかった時間の短い形（GTK4 版の `cipopover::duration`）。
pub fn duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_durations() {
        assert_eq!(super::duration(42), "42s");
        assert_eq!(super::duration(125), "2m 5s");
        assert_eq!(super::duration(3720), "1h 2m");
    }

    use super::*;

    #[test]
    fn reads_github_urls_of_every_form() {
        let r = |url| GitHubRepository::parse(url).map(|g| g.slug());
        assert_eq!(r("https://github.com/o/r.git").as_deref(), Some("o/r"));
        assert_eq!(r("git@github.com:o/r.git").as_deref(), Some("o/r"));
        assert_eq!(r("ssh://git@github.com/o/r").as_deref(), Some("o/r"));
        assert_eq!(
            r("https://github.example.com/o/r").as_deref(),
            Some("github.example.com/o/r")
        );
        assert_eq!(r("https://gitlab.com/o/r.git"), None);
        assert_eq!(r("C:\\src\\repo"), None);
    }

    #[test]
    fn a_branch_page_escapes_what_would_end_the_path() {
        assert_eq!(tree_path("fix#12"), "/tree/fix%2312");
        assert_eq!(tree_path("feature/a?b"), "/tree/feature/a%3Fb");
    }

    #[test]
    fn iso_times_become_unix_times() {
        assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_time("2026-11-21T00:00:10Z"), Some(1_795_219_210));
    }

    #[test]
    fn rollups_fail_before_they_run() {
        let pr = |checks: Value| {
            PullRequest::from(&json!({ "number": 1, "title": "t", "statusCheckRollup": checks }))
                .expect("PR")
                .ci()
        };
        assert_eq!(pr(json!([])), Ci::None);
        assert_eq!(
            pr(json!([{ "status": "COMPLETED", "conclusion": "SUCCESS" }])),
            Ci::Passing
        );
        assert_eq!(
            pr(
                json!([{ "status": "COMPLETED", "conclusion": "SUCCESS" }, { "status": "IN_PROGRESS" }])
            ),
            Ci::Pending
        );
        assert_eq!(
            pr(json!([{ "status": "IN_PROGRESS" }, { "state": "FAILURE" }])),
            Ci::Failing
        );
    }

    #[test]
    fn reruns_of_a_check_collapse_to_the_best() {
        let c = |name: &str, result: &str| CommitCheck {
            name: name.into(),
            result: result.into(),
            url: None,
            seconds: None,
        };
        let collapsed = CommitCheck::collapsed(&[
            c("build", "failure"),
            c("test", "cancelled"),
            c("build", "success"),
            c("test", "in_progress"),
        ]);
        assert_eq!(
            collapsed,
            vec![c("build", "success"), c("test", "in_progress")]
        );
    }

    #[test]
    fn a_fork_lists_only_its_own_pull_requests() {
        let target = PullRequestTarget {
            repo: GitHubRepository::parse("https://github.com/up/r").expect("repo"),
            fork_owner: Some("me".into()),
            fork_in_organization: false,
        };
        let args = target.list_args("open", "50", "number");
        assert!(args.windows(2).any(|w| w == ["--author", "me"]));
        assert!(args.contains(&"number,headRepositoryOwner".to_owned()));
        let mine = target.mine(vec![
            json!({ "number": 1, "headRepositoryOwner": { "login": "ME" } }),
            json!({ "number": 2, "headRepositoryOwner": { "login": "other" } }),
        ]);
        assert_eq!(mine.len(), 1);
        assert_eq!(target.head("topic"), "me:topic");
    }

    #[test]
    fn statuses_come_from_branches_and_commits() {
        let oid = "0123456789abcdef0123456789abcdef01234567";
        let response = json!({ "data": { "repository": {
            "b0": { "target": { "history": { "nodes": [
                { "oid": oid, "statusCheckRollup": { "state": "SUCCESS" },
                  "author": { "email": "A@Example.com", "avatarUrl": "https://a" } }
            ] } } },
            "c0": { "history": { "nodes": [
                { "oid": "1123456789abcdef0123456789abcdef01234567", "statusCheckRollup": { "state": "PENDING" } }
            ] } },
        } } });
        let (statuses, avatars) = statuses_from(&response);
        assert_eq!(
            statuses.get(&Oid::from_hex(oid).expect("oid")),
            Some(&Ci::Passing)
        );
        assert_eq!(statuses.len(), 2);
        assert_eq!(
            avatars.get("a@example.com").map(String::as_str),
            Some("https://a")
        );
    }
}
