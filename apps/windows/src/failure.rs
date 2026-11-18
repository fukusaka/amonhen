//! 失敗した操作の見せ方（GTK4 版の `statusbar::Failure`。コアの `errorKind`・`errorPath` から、何が失敗したか・
//! なぜか・次に何ができるかを出す）。

use amonhen_model::tr;
use serde_json::Value;

/// 失敗した操作。
#[derive(Clone)]
pub struct Failure {
    /// git やコアの文（全文。詳細に出す）。
    pub message: String,
    /// 失敗の種類（`crates/core/src/failure.rs`）。
    pub kind: Option<String>,
    /// 失敗に関わるパス（ほかの worktree の場所、ロックファイル）。
    pub path: Option<String>,
    /// 失敗した命令（`op` で何の操作か分かる。「もう一度」で同じものを実行する）。
    pub command: Option<Value>,
}

/// 次の一手（この FE にあるもの）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Remedy {
    OpenTerminal,
    Retry,
    Pull,
    WorkingCopy,
}

impl Remedy {
    pub fn title(&self) -> String {
        match self {
            Remedy::OpenTerminal => tr!("Open in Terminal"),
            Remedy::Retry => tr!("Try Again"),
            Remedy::Pull => "Pull…".to_owned(),
            Remedy::WorkingCopy => tr!("Open Working Copy"),
        }
    }
}

impl Failure {
    /// コアの答え（`error`・`errorKind`・`errorPath`）から。
    pub fn from_reply(reply: &Value, command: Option<Value>) -> Self {
        Self {
            message: reply["error"].as_str().unwrap_or_default().to_owned(),
            kind: reply["errorKind"].as_str().map(str::to_owned),
            path: reply["errorPath"].as_str().map(str::to_owned),
            command,
        }
    }

    fn op(&self) -> Option<&str> {
        self.command.as_ref()?["op"].as_str()
    }

    /// 命令から決める見出し（「詳細」の題。macOS 版の `OperationFailure.title`）。
    pub fn op_title(&self) -> String {
        match self.op() {
            Some("checkout" | "track" | "detach") => tr!("Couldn’t Checkout"),
            Some("pull") => tr!("Couldn’t Pull"),
            Some("push" | "pushTags") => tr!("Couldn’t Push"),
            Some("fetch" | "deepen" | "unshallow" | "fetchAllContents") => tr!("Couldn’t Fetch"),
            Some("sparseCheckout" | "disableSparseCheckout") => tr!("Couldn’t Change the Folders"),
            Some("maintenance") => tr!("Couldn’t Change git maintenance"),
            Some("merge" | "mergeInto") => tr!("Couldn’t Merge"),
            Some("rebase") => tr!("Couldn’t Rebase"),
            Some("cherryPick") => tr!("Couldn’t Cherry-pick"),
            Some("revert") => tr!("Couldn’t Revert"),
            Some("formatPatch") => tr!("Couldn’t Save the Patches"),
            Some("am" | "amThreeWay" | "applyPatch") => tr!("Couldn’t Apply the Patch"),
            Some("commit") => tr!("Couldn’t Commit"),
            Some("continue") => tr!("Couldn’t Continue"),
            Some("deleteBranch") => tr!("Couldn’t Delete the Branch"),
            Some("stashPush") => tr!("Couldn’t Stash"),
            Some("stashApply") => tr!("Couldn’t Apply the Stash"),
            Some("discard" | "deleteUntracked") => tr!("Couldn’t Discard"),
            Some("undo") => tr!("Couldn’t Undo"),
            _ => tr!("Couldn’t Finish"),
        }
    }

    /// 要点（1 行）。種類が分かれば言い直し、分からなければ git の fatal:・error: の行（なければ最初の行）。
    pub fn summary(&self) -> String {
        let place = self
            .path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match self.kind.as_deref() {
            Some("worktreeInUse") => match self.command.as_ref().and_then(|c| c["branch"].as_str())
            {
                Some(branch) => tr!(
                    "{branch} is checked out in the worktree {place}. A branch can be checked out in only one worktree.",
                    branch = branch,
                    place = place
                ),
                None => tr!(
                    "The branch is checked out in the worktree {place}. A branch can be checked out in only one worktree.",
                    place = place
                ),
            },
            Some("auth") => {
                tr!("The remote didn’t accept the sign-in (password, token or SSH key).")
            }
            Some("hostKey") => {
                tr!("The SSH host key of the remote isn’t known yet, or it has changed.")
            }
            Some("network") => {
                tr!("Can’t reach the remote. Check the network and the remote’s address.")
            }
            Some("rejected") => {
                tr!("The remote has commits you don’t have yet. Pull them first, then Push.")
            }
            Some("diverged") => tr!(
                "Both your branch and its upstream have new commits, so it can’t just move forward."
            ),
            Some("localChanges") => {
                tr!("Uncommitted changes would be overwritten. Commit or stash them first.")
            }
            Some("locked") => tr!(
                "Another git command is running in this repository, or one stopped and left a lock file."
            ),
            Some("noUpstream") => tr!("This branch has no upstream to pull from or push to."),
            _ => self.git_line(),
        }
    }

    /// 次にどうすればよいか（「詳細」に出す。分からなければ None）。
    pub fn advice(&self) -> Option<String> {
        match self.kind.as_deref() {
            Some("auth") => Some(tr!(
                "The remote refused the password, token or SSH key. If a dialog asked for a password or passphrase, check what you typed (GitHub needs a token, not the account password). To stop being asked, store the credentials with a credential helper (for example `gh auth setup-git` or `git config --global credential.helper libsecret`), or add the SSH key to the agent (`ssh-add`). Settings > Git > Programs shows which git the app uses, and Settings > Git > Environment Variables whether the SSH agent is there."
            )),
            Some("hostKey") => Some(tr!(
                "Connect once in a terminal (for example `ssh -T git@github.com`) and confirm the host key, then try again."
            )),
            Some("locked") => self.path.as_ref().map(|p| {
                tr!(
                    "If no other git command is running, delete the lock file and try again:\n{path}",
                    path = p
                )
            }),
            Some("diverged") => Some(tr!(
                "Pull with Merge or Rebase instead of Fast-Forward Only."
            )),
            _ => None,
        }
    }

    /// git の文のうち、帯に出す 1 行。
    fn git_line(&self) -> String {
        let lines: Vec<&str> = self
            .message
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("hint:"))
            .collect();
        lines
            .iter()
            .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
            .or(lines.first())
            .map_or(String::new(), |l| l.to_string())
    }

    /// 次にできること。端末で開くのは手元のリポジトリ（`local`）だけ（daemon・SSH 先では開けない）。
    pub fn remedies(&self, local: bool) -> Vec<Remedy> {
        let retry = if self.command.is_some() {
            vec![Remedy::Retry]
        } else {
            vec![]
        };
        match self.kind.as_deref() {
            // ほかの worktree を開く・上流を決めるは、この FE ではまだ一手でできない。
            Some("worktreeInUse") => vec![],
            Some("auth" | "hostKey") if local => [vec![Remedy::OpenTerminal], retry].concat(),
            Some("auth" | "hostKey") => retry,
            Some("network" | "locked") => retry,
            Some("rejected" | "diverged") => vec![Remedy::Pull],
            Some("localChanges") => vec![Remedy::WorkingCopy],
            Some("noUpstream") => vec![],
            _ => vec![],
        }
    }

    /// 取り消した（失敗ではない）。
    pub fn cancelled(&self) -> bool {
        amonhen_model::i18n::is_cancelled(&self.message)
    }

    /// 詳細の文（要点・次にどうするか・git の文。GTK4 版の Details と同じ）。
    pub fn details(&self) -> String {
        [
            self.kind.is_some().then(|| self.summary()),
            self.advice(),
            Some(self.message.clone()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n")
    }
}
