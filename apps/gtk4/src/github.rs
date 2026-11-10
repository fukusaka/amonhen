//! GitHub（macOS 版の `GitHub.swift`）: リモートの URL から GitHub のリポジトリを見分け、Repository > Open on GitHub
//! と Create Pull Request…（`gh` で作る）、開いているプルリクエスト（サイドバーの Pull Requests）、History の行の
//! CI の状態とコミットの作者のアバターの URL（GraphQL で 1 回に読む）、CI の吹き出しのチェック。手元の `gh`
//! （GitHub CLI）を起動して読み書きし、ログインとトークンは gh に任せる（アプリはトークンに触れない）。gh がなければ
//! 連携の機能を出さない。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use amonhen_core::Repository;
use amonhen_model::{Oid, tr};
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::sheets::{Response, Sheet, Style, row};

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
}

/// PATH のほかに gh を探す場所（デスクトップのランチャーから起動すると、端末の PATH がないことがある。macOS 版と
/// 同じ）。gh を動かすときも PATH の前に足す。
const GH_DIRS: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

/// gh の場所（よくある場所、なければ PATH から。最初に一度だけ探す）。
fn gh_path() -> Option<&'static std::path::Path> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        GH_DIRS
            .iter()
            .map(|d| std::path::Path::new(d).join("gh"))
            .find(|p| p.is_file())
            .or_else(|| glib::find_program_in_path("gh"))
    })
    .as_deref()
}

/// `gh` が使えるか。
pub fn available() -> bool {
    gh_path().is_some()
}

/// gh を動かす `Command`（見つけた場所の gh。尋ねてこないように、色を付けないように。gh が呼ぶもの（git など）も
/// よくある場所から探せるよう PATH の前に足す）。gh がなければ None。
pub fn gh_command() -> Option<std::process::Command> {
    let mut command = amonhen_model::process::command(gh_path()?);
    let mut dirs: Vec<std::path::PathBuf> = GH_DIRS.iter().map(Into::into).collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        command.env("PATH", joined);
    }
    command.env("GH_PROMPT_DISABLED", "1").env("NO_COLOR", "1");
    Some(command)
}

/// リポジトリの GitHub（origin、なければ最初のリモート）。
pub async fn of(repo: &Arc<Repository>) -> Option<GitHubRepository> {
    of_remote(repo).await.map(|(_, github)| github)
}

/// [`of`] と、そのリモートの名前（fork のとき、プルリクエストのブランチをここへ送る）。
async fn of_remote(repo: &Arc<Repository>) -> Option<(String, GitHubRepository)> {
    let reply = crate::sheets::run(repo, json!({ "op": "remotes" })).await;
    let remotes = reply["info"]["remotes"].as_array()?.clone();
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

    /// `pr list` の、fork のときの引数（その fork の持ち主が出したものに絞り、head の持ち主も読む）。元のリポジトリの
    /// 新しいものから数えると、ほかの人のプルリクエストで埋まるので、作者で絞ってから数える。組織の fork は作者で
    /// 絞れないので、多めに読んで [`Self::mine`] で絞る（それより古いものは見つからない）。
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

thread_local! {
    /// 読めた fork の元（slug → 元と、持ち主が組織か。fork でなければ None）。読めなかったものは覚えない（次にまた読む）。
    static PARENTS: RefCell<HashMap<String, Option<(GitHubRepository, bool)>>> = RefCell::default();
}

/// `origin` のリポジトリからプルリクエストを出す先（fork なら元のリポジトリ。macOS 版と同じ）。
pub async fn pull_request_target(origin: &GitHubRepository) -> PullRequestTarget {
    let slug = origin.slug();
    let known = PARENTS.with(|p| p.borrow().get(&slug).cloned());
    let parent = match known {
        Some(parent) => parent,
        None => {
            let args = ["repo", "view", &slug, "--json", "parent,isInOrganization"]
                .map(str::to_owned)
                .to_vec();
            let read = gh_async(args, None)
                .await
                .ok()
                .and_then(|out| serde_json::from_str::<Value>(&out).ok())
                .map(|view| {
                    let organization = view["isInOrganization"].as_bool() == Some(true);
                    parent_from(&view, &origin.host).map(|parent| (parent, organization))
                });
            if let Some(parent) = &read {
                PARENTS.with(|p| p.borrow_mut().insert(slug, parent.clone()));
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

pub fn open_uri(window: Option<&gtk::Window>, uri: &str) {
    gtk::UriLauncher::new(uri).launch(window, gio::Cancellable::NONE, |_| {});
}

/// ブランチのページの URL の後ろ。`#` や `?` を URL の区切りにしない（`/` はそのまま。macOS 版と同じ）。
fn tree_path(branch: &str) -> String {
    format!(
        "/tree/{}",
        glib::Uri::escape_string(branch, Some("/"), false)
    )
}

/// Repository > Open on GitHub: 今のブランチのページを開く。
pub fn open_on_github(actions: &Rc<Actions>, branch: Option<String>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let window = crate::sheets::parent(&actions);
        let Some(repo) = actions.repo() else { return };
        let Some(github) = of(&repo).await else {
            if let Some(w) = &window {
                w.error_bell();
            }
            return;
        };
        let mut url = github.web_url();
        if let Some(b) = branch {
            url.push_str(&tree_path(&b));
        }
        open_uri(window.as_ref(), &url);
    });
}

/// `gh` を `dir` で動かし、標準出力を返す。失敗したら標準エラー（空なら「gh … が失敗しました」）。裏の糸で呼ぶ。
fn gh(args: &[&str], dir: Option<&std::path::Path>) -> Result<String, String> {
    let Some(mut command) = gh_command() else {
        return Err(tr!(
            "gh (GitHub CLI) wasn’t found. Install it with brew install gh, then sign in with gh auth login."
        ));
    };
    command.args(args);
    if let Some(d) = dir {
        command.current_dir(d);
    }
    // かかった時間を Activity の Performance に（macOS 版と同じ「GitHub: gh <サブコマンド>」）。記録は画面の糸で。
    let started = std::time::Instant::now();
    let out = command.output();
    let label = format!("GitHub: gh {}", args.first().copied().unwrap_or_default());
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    glib::MainContext::default().invoke(move || crate::activity::record(&label, ms));
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

/// [`gh`] を裏の糸で動かし始める（呼んだときに始まる）。
fn gh_spawn(
    args: Vec<String>,
    dir: Option<std::path::PathBuf>,
) -> gio::JoinHandle<Result<String, String>> {
    gio::spawn_blocking(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        gh(&refs, dir.as_deref())
    })
}

/// [`gh`] を裏の糸で動かす。
async fn gh_async(args: Vec<String>, dir: Option<std::path::PathBuf>) -> Result<String, String> {
    gh_spawn(args, dir)
        .await
        .unwrap_or_else(|_| Err(String::new()))
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

    pub fn ci_mark(&self) -> &'static str {
        match self.ci() {
            Ci::Passing => "✓",
            Ci::Failing => "✗",
            Ci::Pending => "●",
            Ci::None => "",
        }
    }

    /// サイドバーの行のツールチップ（番号と題、作者とブランチ、CI とレビュー）。
    pub fn tooltip(&self) -> String {
        let review = match self.review_decision.as_deref() {
            Some("APPROVED") => tr!("Approved"),
            Some("CHANGES_REQUESTED") => tr!("Changes requested"),
            Some("REVIEW_REQUIRED") => tr!("Review required"),
            _ => String::new(),
        };
        let state: Vec<String> = [self.ci().label(), review]
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
pub async fn pull_requests(target: &PullRequestTarget) -> Result<Vec<PullRequest>, String> {
    let args = target.list_args(
        "open",
        "50",
        "number,title,url,headRefName,baseRefName,isDraft,author,reviewDecision,statusCheckRollup,headRefOid",
    );
    let out = gh_async(args, None).await?;
    let list: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(target
        .mine(list.as_array().cloned().unwrap_or_default())
        .iter()
        .filter_map(PullRequest::from)
        .collect())
}

/// マージされたプルリクエスト（新しい順、最大 200 件。fork なら、その fork から出したもの）を、コアの
/// `branchCleanup` の `pullRequests` の形で。squash・rebase で取り込んだブランチを見分ける（macOS 版と同じ）。
pub async fn merged_pull_requests(target: &PullRequestTarget) -> Result<Vec<Value>, String> {
    let args = target.list_args("merged", "200", "number,headRefName,headRefOid");
    let out = gh_async(args, None).await?;
    let list: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(target
        .mine(list.as_array().cloned().unwrap_or_default())
        .iter()
        .map(|p| serde_json::json!({ "number": p["number"], "branch": p["headRefName"], "oid": p["headRefOid"] }))
        .collect())
}

/// 既定のブランチ（プルリクエストの base の初期値）。
pub async fn default_branch(repo: &GitHubRepository) -> Option<String> {
    let args = [
        "repo",
        "view",
        &repo.slug(),
        "--json",
        "defaultBranchRef",
        "-q",
        ".defaultBranchRef.name",
    ]
    .map(str::to_owned)
    .to_vec();
    gh_async(args, None).await.ok().filter(|b| !b.is_empty())
}

/// ブランチの直近のコミット（それぞれ `count` 個まで）の CI の状態（`ci` のとき）と、作者の GitHub のアバターの
/// URL（`authors` のとき。メールアドレスの小文字 → URL）。GraphQL で 1 回に読む。GitHub にないブランチは飛ばす。
pub async fn commit_statuses(
    repo: &GitHubRepository,
    branches: &[String],
    count: u32,
    ci: bool,
    authors: bool,
) -> Result<(HashMap<Oid, Ci>, HashMap<String, String>), String> {
    if branches.is_empty() || !(ci || authors) {
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
    // ブランチごとに別名を付けて並べる（b0・b1 …）。
    let fields = (0..branches.len())
        .map(|i| {
            format!(
                "b{i}: ref(qualifiedName: $r{i}) {{ target {{ ... on Commit {{ history(first: {count}) {{ nodes {{\n  {wanted} }} }} }} }} }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let variables = (0..branches.len())
        .map(|i| format!("$r{i}: String!"))
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
    if repo.host != "github.com" {
        args.extend(["--hostname".into(), repo.host.clone()]);
    }
    for (i, branch) in branches.iter().enumerate() {
        args.extend(["-f".into(), format!("r{i}=refs/heads/{branch}")]);
    }
    let out = gh_async(args, None).await?;
    let response: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    let mut statuses = HashMap::new();
    let mut avatars = HashMap::new();
    for r in response["data"]["repository"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
    {
        for node in r["target"]["history"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
        {
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
    Ok((statuses, avatars))
}

/// `sha` のコミットのチェック（Actions の check run と、外部の status）。印を押したときに読む。
pub async fn commit_checks(repo: &GitHubRepository, sha: &str) -> Result<Vec<CommitCheck>, String> {
    let host: Vec<String> = if repo.host == "github.com" {
        Vec::new()
    } else {
        vec!["--hostname".into(), repo.host.clone()]
    };
    let base = format!("repos/{}/{}/commits/{sha}", repo.owner, repo.name);
    let runs_args = [
        vec!["api".into(), format!("{base}/check-runs?per_page=100")],
        host.clone(),
    ]
    .concat();
    let status_args = [vec!["api".into(), format!("{base}/status")], host].concat();
    // 2 つを並べて読む。
    let runs = gh_spawn(runs_args, None);
    let status = gh_spawn(status_args, None);
    let failed = |_| Err(String::new());
    let runs = runs.await.unwrap_or_else(failed)?;
    let status = status.await.unwrap_or_else(failed)?;
    let runs: Value = serde_json::from_str(&runs).map_err(|e| e.to_string())?;
    let status: Value = serde_json::from_str(&status).map_err(|e| e.to_string())?;
    let time = |v: &Value| {
        v.as_str()
            .and_then(|s| glib::DateTime::from_iso8601(s, None).ok())
            .map(|d| d.to_unix())
    };
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

thread_local! {
    /// 窓ごとの読み手（プルリクエストを作ったら、その窓の分を読み直す）。
    static LOADERS: RefCell<Vec<std::rc::Weak<Loader>>> = RefCell::default();
}

/// 窓ごとに GitHub から読むもの（macOS 版の `RepositoryWindow.loadGitHub`）: 開いているプルリクエスト（サイドバー）、
/// 今のブランチと既定のブランチの直近のコミットの CI の状態（History の行の印）と作者のアバターの URL。読むものと
/// 量は Settings > Accounts で決める。gh がなければ何もしない。
pub struct Loader {
    actions: Rc<Actions>,
    view: glib::WeakRef<crate::history::HistoryView>,
    sidebar: std::rc::Weak<crate::sidebar::Sidebar>,
    github: RefCell<Option<GitHubRepository>>,
    /// プルリクエストを出す先（`github` が fork なら元のリポジトリ）。
    target: RefCell<Option<PullRequestTarget>>,
    loaded_at: Cell<Option<Instant>>,
    default_branch: RefCell<Option<String>>,
    /// この窓で GitHub のアバターの URL を読んだか。
    avatars_loaded: Cell<bool>,
}

/// GitHub の連携の設定（変わったら読み直す）。
const SETTING_KEYS: [&str; 5] = [
    crate::settings::GITHUB_CI_STATUS,
    crate::settings::GITHUB_AVATARS,
    crate::settings::GITHUB_CI_DEFAULT_BRANCH,
    crate::settings::GITHUB_CI_COMMITS,
    crate::settings::GITHUB_AVATARS_ONCE,
];

impl Loader {
    pub fn new(
        actions: &Rc<Actions>,
        view: &crate::history::HistoryView,
        sidebar: &Rc<crate::sidebar::Sidebar>,
    ) -> Rc<Self> {
        let loader = Rc::new(Self {
            actions: actions.clone(),
            view: view.downgrade(),
            sidebar: Rc::downgrade(sidebar),
            github: RefCell::default(),
            target: RefCell::default(),
            loaded_at: Cell::new(None),
            default_branch: RefCell::default(),
            avatars_loaded: Cell::new(false),
        });
        LOADERS.with(|l| {
            let mut l = l.borrow_mut();
            l.retain(|w| w.strong_count() > 0);
            l.push(Rc::downgrade(&loader));
        });
        let weak = Rc::downgrade(&loader);
        crate::settings::connect_changed_while(view, move |key| {
            let Some(loader) = weak.upgrade() else { return };
            if !SETTING_KEYS.contains(&key) {
                return;
            }
            if !crate::settings::bool(crate::settings::GITHUB_AVATARS) {
                crate::avatars::forget_github();
                loader.avatars_loaded.set(false);
            }
            loader.load(true);
        });
        // History の行の CI の印を押したら、そのコミットのチェックの吹き出し。
        let weak = Rc::downgrade(&loader);
        view.connect_ci(move |id, ci, rect| {
            let Some(loader) = weak.upgrade() else { return };
            if let (Some(view), Some(github)) = (loader.view.upgrade(), loader.github()) {
                let sha = id.to_string();
                crate::cipopover::show(view.upcast_ref(), Some(rect), ci, &sha, &sha, github);
            }
        });
        loader
    }

    /// origin（なければ最初のリモート）が GitHub なら、そのリポジトリ（まだ読んでいなければ None）。
    pub fn github(&self) -> Option<GitHubRepository> {
        self.github.borrow().clone()
    }

    pub fn target(&self) -> Option<PullRequestTarget> {
        self.target.borrow().clone()
    }

    /// GitHub のリポジトリを判定し、開いているプルリクエストを読んでサイドバーに出し、CI の状態とアバターを読む。
    /// `force` でなければ、前に読んでから 30 秒は読まない。
    pub fn load(self: &Rc<Self>, force: bool) {
        if !available() {
            return;
        }
        if !force
            && self
                .loaded_at
                .get()
                .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
        {
            return;
        }
        let Some(repo) = self.actions.repo() else {
            return;
        };
        self.loaded_at.set(Some(Instant::now()));
        let this = self.clone();
        glib::spawn_future_local(async move {
            let github = of(&repo).await;
            this.github.replace(github.clone());
            // Open on GitHub・Create Pull Request… は、fork かどうかを gh に聞く前から使える（macOS 版と同じ）。
            this.actions.update_facts(|f| {
                f.github = github.is_some();
                f.gh = available();
            });
            if let Some(s) = this.sidebar.upgrade() {
                let fork = this
                    .target
                    .borrow()
                    .as_ref()
                    .is_some_and(PullRequestTarget::is_fork);
                s.set_can_create_pull_request(github.is_some(), fork);
            }
            let target = match &github {
                Some(github) => Some(pull_request_target(github).await),
                None => None,
            };
            this.target.replace(target.clone());
            let fork = target.as_ref().is_some_and(PullRequestTarget::is_fork);
            // Open on GitHub・Create Pull Request… は GitHub のリポジトリだけ（macOS 版と同じ）。
            this.actions.update_facts(|f| {
                f.github = github.is_some();
                f.github_fork = fork;
                f.gh = available();
            });
            let sidebar = this.sidebar.upgrade();
            if let Some(s) = &sidebar {
                s.set_can_create_pull_request(github.is_some(), fork);
            }
            let (Some(github), Some(target)) = (github, target) else {
                if let Some(s) = &sidebar {
                    s.show_pull_requests(Vec::new());
                }
                return;
            };
            let name = repo_name(&repo);
            match pull_requests(&target).await {
                Ok(list) => {
                    if let Some(s) = this.sidebar.upgrade() {
                        s.show_pull_requests(list);
                    }
                }
                Err(e) => crate::activity::event(
                    &name,
                    tr!("Can’t read pull requests: {error}", error = e),
                ),
            }
            // マージされたプルリクエストも同じときに読む（squash・rebase で取り込んだブランチに印を出す）。
            if let Ok(merged) = merged_pull_requests(&target).await
                && let Some(s) = this.sidebar.upgrade()
            {
                s.set_merged_pull_requests(merged);
            }
            this.load_statuses(&github, &name).await;
        });
    }

    /// 今のブランチと既定のブランチの、直近のコミットの CI の状態（History の行の印）と作者のアバター。
    async fn load_statuses(&self, github: &GitHubRepository, name: &str) {
        use crate::settings::{
            self, GITHUB_AVATARS, GITHUB_AVATARS_ONCE, GITHUB_CI_DEFAULT_BRANCH,
        };
        let ci = settings::bool(settings::GITHUB_CI_STATUS);
        // アバターの URL は、1 回だけにするなら、読めた後は頼まない。
        let authors = settings::bool(GITHUB_AVATARS)
            && !(settings::bool(GITHUB_AVATARS_ONCE) && self.avatars_loaded.get());
        if !ci && let Some(view) = self.view.upgrade() {
            view.set_ci(HashMap::new());
        }
        if !ci && !authors {
            return;
        }
        let with_default = settings::bool(GITHUB_CI_DEFAULT_BRANCH);
        if with_default && self.default_branch.borrow().is_none() {
            let branch = default_branch(github).await;
            self.default_branch.replace(branch);
        }
        let mut branches: Vec<String> = [
            self.actions.head(),
            with_default
                .then(|| self.default_branch.borrow().clone())
                .flatten(),
        ]
        .into_iter()
        .flatten()
        .collect();
        branches.sort();
        branches.dedup();
        let count = settings::github_ci_commits();
        match commit_statuses(github, &branches, count, ci, authors).await {
            Ok((statuses, avatars)) => {
                if ci && let Some(view) = self.view.upgrade() {
                    view.set_ci(statuses);
                }
                if authors {
                    crate::avatars::use_github(avatars);
                    self.avatars_loaded.set(true);
                }
            }
            Err(e) => {
                crate::activity::event(name, tr!("Can’t read the CI status: {error}", error = e))
            }
        }
    }
}

/// `actions` の窓の読み手。
fn loader_of(actions: &Rc<Actions>) -> Option<Rc<Loader>> {
    LOADERS.with(|l| {
        l.borrow()
            .iter()
            .filter_map(std::rc::Weak::upgrade)
            .find(|l| Rc::ptr_eq(&l.actions, actions))
    })
}

/// View > Refresh: `actions` の窓のプルリクエスト・CI の状態を、間を空けずに読み直す（macOS 版と同じ）。
pub fn reload(actions: &Rc<Actions>) {
    if let Some(loader) = loader_of(actions) {
        loader.load(true);
    }
}

/// #123 の候補（番号とタイトル）と、読んだ時刻。
type IssueCache = (std::time::Instant, Vec<(u64, String)>);

thread_local! {
    /// コミットのメッセージの #123 の候補（リポジトリの slug ごと）。
    static ISSUES: RefCell<std::collections::HashMap<String, IssueCache>> = RefCell::default();
}

/// コミットのメッセージの #123 の候補: 開いている Issue（30 件）と PR（「PR: 」を付けて）を、番号の大きい順に。
/// 5 分は覚えておく（macOS 版と同じ）。GitHub のリポジトリでなければ空。
pub async fn issue_candidates(actions: &Rc<Actions>) -> Vec<(u64, String)> {
    // fork なら、Issue とプルリクエストは元のリポジトリのもの。
    let Some(target) = target_of(actions).filter(|_| available()) else {
        return Vec::new();
    };
    let slug = target.repo.slug();
    let cached = ISSUES.with(|i| {
        i.borrow()
            .get(&slug)
            .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(300))
            .map(|(_, items)| items.clone())
    });
    if let Some(items) = cached {
        return items;
    }
    let args = [
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
    ]
    .map(str::to_owned)
    .to_vec();
    let issues: Vec<(u64, String)> = gh_async(args, None)
        .await
        .ok()
        .and_then(|out| serde_json::from_str::<Value>(&out).ok())
        .and_then(|v| v.as_array().cloned())
        .into_iter()
        .flatten()
        .filter_map(|i| Some((i["number"].as_u64()?, i["title"].as_str()?.to_owned())))
        .collect();
    let pulls = pull_requests(&target).await.unwrap_or_default();
    let mut items: Vec<(u64, String)> = issues
        .into_iter()
        .chain(
            pulls
                .into_iter()
                .map(|p| (p.number, format!("PR: {}", p.title))),
        )
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.0));
    ISSUES.with(|i| {
        i.borrow_mut()
            .insert(slug, (std::time::Instant::now(), items.clone()))
    });
    items
}

/// `actions` の窓のプルリクエストを出す先（読んでいれば。fork なら元のリポジトリ）。
pub fn target_of(actions: &Rc<Actions>) -> Option<PullRequestTarget> {
    loader_of(actions).and_then(|l| l.target())
}

fn repo_name(repo: &Repository) -> String {
    repo.paths()
        .workdir
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// サイドバーの Pull Requests の行の右クリック（macOS 版と同じ項目）。
pub fn pull_request_menu(actions: &Rc<Actions>, pr: &PullRequest) -> Vec<crate::menu::Item> {
    let (a, p) = (actions.clone(), pr.clone());
    let url = pr.url.clone();
    let copied = pr.url.clone();
    let w = actions.window();
    vec![
        crate::menu::action(format!("Checkout #{}…", pr.number), move || {
            checkout_pull_request(&a, &p)
        }),
        crate::menu::action(tr!("Open in Browser"), move || {
            open_uri(w.as_ref().map(|w| w.upcast_ref()), &url)
        }),
        crate::menu::action(tr!("Copy URL"), move || {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&copied);
            }
        }),
    ]
}

/// プルリクエストのブランチをチェックアウトする（gh pr checkout。なければ作る）。手元のリポジトリだけ。
pub fn checkout_pull_request(actions: &Rc<Actions>, pr: &PullRequest) {
    let actions = actions.clone();
    let pr = pr.clone();
    glib::spawn_future_local(async move {
        let window = crate::sheets::parent(&actions);
        let github = target_of(&actions).map(|t| t.repo);
        let dir = actions
            .repo()
            .filter(|r| r.connection_state().is_none())
            .and_then(|r| r.paths().workdir);
        let (Some(github), Some(dir)) = (github, dir) else {
            if let Some(w) = &window {
                w.error_bell();
            }
            return;
        };
        // worktree の行は書かない（macOS 版の確認と同じ）。
        let sheet = Sheet::unnamed(
            window.as_ref(),
            &tr!("Checkout #{number}?", number = pr.number),
            &tr!(
                "“{title}” is checked out as a local branch.",
                title = pr.title
            ),
        );
        sheet.buttons("Checkout", false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let (number, slug) = (pr.number.to_string(), github.slug());
        // 失敗したら gh の文をそのまま知らせ、その後で読み直す（macOS 版と同じ）。
        crate::activity::begin();
        let result = gio::spawn_blocking(move || {
            gh(&["pr", "checkout", &number, "-R", &slug], Some(&dir)).map(|_| ())
        })
        .await
        .unwrap_or_else(|_| Err(String::new()));
        crate::activity::end();
        if let Err(e) = result {
            crate::sheets::alert(window.as_ref(), &e, "").await;
        }
        // View > Refresh と同じく、プルリクエストと CI も間を空けずに読み直す（macOS 版の `refreshRepository`）。
        actions.reload();
        reload(&actions);
    });
}

/// Repository > Create Pull Request…: `branch` からプルリクエストを作る。まだ push していない（上流がない・先行
/// コミットがある）なら、まず Push のシートを出し、送り終えたらそのまま作る。origin が fork なら元のリポジトリへ
/// 出す。そのときは既定のブランチからも出せ、送る先は上流でなく fork の同じ名前のブランチ（手元と同じコミットで
/// なければ、まずそこへ送る。macOS 版と同じ）。
pub fn create_pull_request(actions: &Rc<Actions>, branch: String, pushed: bool) {
    // 選んだときの worktree で読み、送るならその worktree の Push のシートで（読んでいる間に書き込む先を替えても）。
    if let Some(target) = crate::sheets::Target::current(actions) {
        create_pull_request_in(actions, target, branch, pushed);
    }
}

fn create_pull_request_in(
    actions: &Rc<Actions>,
    target: crate::sheets::Target,
    branch: String,
    pushed: bool,
) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let window = crate::sheets::parent(&actions);
        let repo = target.repo.clone();
        // gh で作るので、gh がなければ作れない。
        if !available() {
            if let Some(w) = &window {
                w.error_bell();
            }
            return;
        }
        let (Some((remote, github)), Some(dir)) = (of_remote(&repo).await, repo.paths().workdir)
        else {
            if let Some(w) = &window {
                w.error_bell();
            }
            return;
        };
        // SSH 先のパスは手元にないので、そこでは gh を起動しない（送り先は -R と --head で決まる）。
        let dir = repo.connection_state().is_none().then_some(dir);
        let to = pull_request_target(&github).await;
        if !to.is_fork() && actions.default_branch().as_deref() == Some(branch.as_str()) {
            if let Some(w) = &window {
                w.error_bell();
            }
            return;
        }
        let r = repo.clone();
        let refs = gio::spawn_blocking(move || r.load_refs().ok())
            .await
            .ok()
            .flatten();
        let item = refs.as_ref().and_then(|r| {
            r.refs
                .entries
                .iter()
                .find(|e| e.kind == amonhen_model::RefKind::Local && e.name == branch)
                .cloned()
        });
        let again = {
            let (a, b, t) = (actions.clone(), branch.clone(), target.clone());
            Box::new(move || create_pull_request_in(&a, t, b, true))
        };
        if to.is_fork() {
            let there = format!("{remote}/{branch}");
            let pushed_there = refs.as_ref().is_some_and(|r| {
                r.refs.entries.iter().any(|e| {
                    e.kind == amonhen_model::RefKind::Remote
                        && e.name == there
                        && e.target == item.as_ref().and_then(|i| i.target)
                })
            });
            if !pushed && item.is_some() && !pushed_there {
                let command = json!({
                    "op": "push", "remote": remote, "branch": branch, "remoteBranch": branch,
                    "force": false, "setUpstream": item.as_ref().is_some_and(|i| i.detail.is_none()),
                });
                crate::sheets::push_to_in(
                    &actions,
                    target,
                    &branch,
                    &there,
                    command,
                    false,
                    Some(again),
                );
                return;
            }
        } else if !pushed
            && item
                .as_ref()
                .is_none_or(|i| i.detail.is_none() || i.gone || i.ahead > 0)
        {
            crate::sheets::push_in(&actions, target, Some(branch.clone()), false, Some(again));
            return;
        }
        // タイトルと本文は、ブランチの先頭のコミットのメッセージから。
        let tip = item
            .and_then(|i| i.target)
            .map_or_else(|| "HEAD".to_owned(), |t| t.to_string());
        let message = crate::sheets::run(&repo, json!({ "op": "commitMessage", "oid": tip })).await
            ["text"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        // メッセージが空なら、タイトルも空のまま（macOS 版と同じ。空のタイトルでは作らない）。
        let (title, body) = message.split_once('\n').unwrap_or((&message, ""));
        let (title, body) = (title.to_owned(), body.trim().to_owned());
        let slug = to.repo.slug();
        let base = default_branch(&to.repo)
            .await
            .unwrap_or_else(|| "main".to_owned());

        // ブランチを名前で渡すので、worktree の行は書かない（macOS 版と同じ）。
        let sheet = Sheet::unnamed(
            window.as_ref(),
            &tr!("Create Pull Request"),
            &tr!("From {branch}", branch = branch),
        );
        // NSAlert の欄と同じく、Return で Create を押す。
        let title_field = gtk::Entry::builder()
            .text(&title)
            .placeholder_text(tr!("Title"))
            .activates_default(true)
            .hexpand(true)
            .build();
        let base_field = gtk::Entry::builder()
            .text(&base)
            .placeholder_text("main")
            .activates_default(true)
            .hexpand(true)
            .build();
        let body_view = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(4)
            .right_margin(4)
            .build();
        body_view.buffer().set_text(&body);
        let body_scrolled = gtk::ScrolledWindow::builder()
            .child(&body_view)
            .min_content_height(160)
            .min_content_width(460)
            .has_frame(true)
            .build();
        let draft = gtk::CheckButton::with_label(&tr!("Create as Draft"));
        sheet.add(&row(&tr!("Title:"), &title_field));
        sheet.add(&row("base:", &base_field));
        sheet.add(&row(&tr!("Body:"), &body_scrolled));
        sheet.add(&row("", &draft));
        let create = sheet.buttons(&tr!("Create"), false, None);
        // 題と base が空の間は Create を押せない（押してもシートと打った文を黙って捨てないように。macOS 版と同じ）。
        let update = {
            let (create, t, b) = (create.clone(), title_field.clone(), base_field.clone());
            move || create.set_sensitive(!t.text().trim().is_empty() && !b.text().trim().is_empty())
        };
        update();
        for field in [&title_field, &base_field] {
            let update = update.clone();
            field.connect_changed(move |_| update());
        }
        if sheet.run().await != Response::Ok {
            return;
        }
        let title = title_field.text().trim().to_owned();
        let base = base_field.text().trim().to_owned();
        if title.is_empty() || base.is_empty() {
            return;
        }
        let b = body_view.buffer();
        let body = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let draft = draft.is_active();
        let head = to.head(&branch);
        // 作っている間は、ツールバーの Activity の印を回す（macOS 版と同じ）。
        crate::activity::begin();
        let result = gio::spawn_blocking(move || {
            let mut args = vec![
                "pr", "create", "-R", &slug, "--head", &head, "--title", &title, "--body", &body,
                "--base", &base,
            ];
            if draft {
                args.push("--draft");
            }
            gh(&args, dir.as_deref())
        })
        .await
        .unwrap_or_else(|_| Err(String::new()));
        crate::activity::end();
        match result {
            Ok(url) => {
                // 済んだことの知らせなので、閉じるボタンは Close（macOS 版と同じ）。
                let sheet = Sheet::unnamed(window.as_ref(), &tr!("Pull Request Created"), &url);
                sheet.button(&tr!("Close"), Response::Cancel, None);
                sheet.button(
                    &tr!("Open in Browser"),
                    Response::Ok,
                    Some(Style::Suggested),
                );
                if sheet.run().await == Response::Ok {
                    open_uri(window.as_ref(), &url);
                }
            }
            // gh の文を見出しにして知らせる（macOS 版の NSAlert(error:)）。
            Err(e) => crate::sheets::alert(window.as_ref(), &e, "").await,
        }
        // できてもできなくても、知らせを閉じてから GitHub の様子を読み直す（macOS 版と同じ）。
        if let Some(loader) = loader_of(&actions) {
            loader.load(true);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_page_escapes_what_would_end_the_path() {
        assert_eq!(tree_path("fix#12"), "/tree/fix%2312");
        assert_eq!(tree_path("feature/a?b"), "/tree/feature/a%3Fb");
    }

    #[test]
    fn a_fork_sends_pull_requests_to_its_parent() {
        let view = json!({ "parent": { "name": "tool", "owner": { "login": "upstream" } } });
        let parent = parent_from(&view, "github.com").unwrap();
        assert_eq!(parent.slug(), "upstream/tool");
        assert_eq!(parent_from(&json!({ "parent": null }), "github.com"), None);

        let fork = PullRequestTarget {
            repo: parent,
            fork_owner: Some("me".to_owned()),
            fork_in_organization: false,
        };
        assert_eq!(fork.head("main"), "me:main");
        let args = fork.list_args("open", "50", "number");
        assert_eq!(
            args[args.len() - 4..],
            ["--author", "me", "--json", "number,headRepositoryOwner"].map(str::to_owned)
        );
        let list = vec![
            json!({ "number": 1, "headRepositoryOwner": { "login": "Me" } }),
            json!({ "number": 2, "headRepositoryOwner": { "login": "someone" } }),
        ];
        let mine: Vec<_> = fork
            .mine(list.clone())
            .iter()
            .map(|p| p["number"].clone())
            .collect();
        assert_eq!(mine, [json!(1)]);

        // 組織の fork は作者で絞れないので、多めに読んで head の持ち主で絞る。
        let organization = PullRequestTarget {
            fork_in_organization: true,
            ..fork.clone()
        };
        let args = organization.list_args("open", "50", "number");
        assert!(!args.contains(&"--author".to_owned()), "{args:?}");
        assert!(
            args.contains(&ORGANIZATION_FORK_LIMIT.to_owned()),
            "{args:?}"
        );
        assert!(
            args.iter().any(|a| a == "number,headRepositoryOwner"),
            "{args:?}"
        );

        let origin = PullRequestTarget {
            repo: GitHubRepository::parse("git@github.com:me/tool.git").unwrap(),
            fork_owner: None,
            fork_in_organization: false,
        };
        assert_eq!(origin.head("topic"), "topic");
        assert_eq!(origin.mine(list).len(), 2);
        assert!(
            !origin
                .list_args("open", "50", "number")
                .contains(&"--author".to_owned())
        );
    }

    #[test]
    fn parses_github_remotes() {
        let expected = Some(GitHubRepository {
            host: "github.com".into(),
            owner: "o".into(),
            name: "r".into(),
        });
        assert_eq!(
            GitHubRepository::parse("https://github.com/o/r.git"),
            expected
        );
        assert_eq!(GitHubRepository::parse("git@github.com:o/r.git"), expected);
        assert_eq!(
            GitHubRepository::parse("ssh://git@github.com/o/r"),
            expected
        );
        assert_eq!(GitHubRepository::parse("https://gitlab.com/o/r.git"), None);
        assert_eq!(
            GitHubRepository::parse("https://github.example.com/o/r").map(|g| g.slug()),
            Some("github.example.com/o/r".into())
        );
    }
}
