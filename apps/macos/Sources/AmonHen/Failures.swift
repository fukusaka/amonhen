import AmonhenKit
import Foundation

/// 失敗した操作。帯に、何が失敗したか・なぜか・次に何ができるかを出すための材料。
struct OperationFailure {
    /// git やコアの文（全文。「詳細」とツールチップに出す）。
    let message: String
    /// 失敗の種類（`crates/core/src/failure.rs`）。見分けられなければ nil。
    var kind: String?
    /// 失敗に関わるパス（ほかの worktree の場所、ロックファイル）。
    var path: String?
    /// 失敗した命令（`op` で何の操作か分かる。「もう一度」で同じものを実行する）。
    var command: [String: Any]?

    init(message: String, kind: String? = nil, path: String? = nil, command: [String: Any]? = nil) {
        self.message = message
        self.kind = kind
        self.path = path
        self.command = command
    }

    init(_ error: Error, command: [String: Any]? = nil) {
        let amonhen = error as? AmonhenError
        self.init(message: "\(error)", kind: amonhen?.kind, path: amonhen?.path, command: command)
    }

    var op: String? { command?["op"] as? String }

    /// 帯の見出し。何の操作が失敗したか。
    var title: String {
        switch op {
        case "checkout", "track", "detach": L("Couldn’t Checkout")
        case "pull": L("Couldn’t Pull")
        case "push", "pushTags": L("Couldn’t Push")
        case "fetch", "deepen", "unshallow", "fetchAllContents": L("Couldn’t Fetch")
        case "sparseCheckout", "disableSparseCheckout": L("Couldn’t Change the Folders")
        case "merge", "mergeInto": L("Couldn’t Merge")
        case "rebase": L("Couldn’t Rebase")
        case "cherryPick": L("Couldn’t Cherry-pick")
        case "revert": L("Couldn’t Revert")
        case "formatPatch": L("Couldn’t Save the Patches")
        case "am", "amThreeWay", "applyPatch": L("Couldn’t Apply the Patch")
        case "commit": L("Couldn’t Commit")
        case "continue": L("Couldn’t Continue")
        case "deleteBranch": L("Couldn’t Delete the Branch")
        case "stashPush": L("Couldn’t Stash")
        case "stashApply": L("Couldn’t Apply the Stash")
        case "discard", "deleteUntracked": L("Couldn’t Discard")
        case "undo": L("Couldn’t Undo")
        default: L("Couldn’t Finish")
        }
    }

    /// 要点（1 行）。種類が分かれば言い直し、分からなければ git の fatal:・error: の行（なければ最初の行）。
    var summary: String {
        switch kind {
        case "worktreeInUse":
            let place = path.map { ($0 as NSString).lastPathComponent } ?? ""
            return if let branch = command?["branch"] as? String {
                L("%@ is checked out in the worktree %@. A branch can be checked out in only one worktree.", branch, place)
            } else {
                L("The branch is checked out in the worktree %@. A branch can be checked out in only one worktree.", place)
            }
        case "auth": return L("The remote didn’t accept the sign-in (password, token or SSH key).")
        case "hostKey": return L("The SSH host key of the remote isn’t known yet, or it has changed.")
        case "network": return L("Can’t reach the remote. Check the network and the remote’s address.")
        case "rejected": return L("The remote has commits you don’t have yet. Pull them first, then Push.")
        case "diverged": return L("Both your branch and its upstream have new commits, so it can’t just move forward.")
        case "localChanges": return L("Uncommitted changes would be overwritten. Commit or stash them first.")
        case "locked": return L("Another git command is running in this repository, or one stopped and left a lock file.")
        case "noUpstream": return L("This branch has no upstream to pull from or push to.")
        default: return gitLine
        }
    }

    /// 次にどうすればよいか（「詳細」に出す。分からなければ nil）。
    var advice: String? {
        switch kind {
        case "auth":
            L("""
            The remote refused the password, token or SSH key. If a dialog asked for a password or passphrase, check \
            what you typed (GitHub needs a token, not the account password). To stop being asked, store the \
            credentials with a credential helper (for example `gh auth setup-git` or \
            `git config --global credential.helper osxkeychain`), or add the SSH key to the agent \
            (`ssh-add --apple-use-keychain`). Settings > Git > Programs shows which git the app uses, and Settings > \
            Git > Environment Variables whether the SSH agent is there.
            """)
        case "hostKey":
            L("Connect once in Terminal (for example `ssh -T git@github.com`) and confirm the host key, then try again.")
        case "locked":
            path.map {
                L("If no other git command is running, delete the lock file and try again:\n%@", $0)
            }
        case "diverged":
            L("Pull with Merge or Rebase instead of Fast-Forward Only.")
        default: nil
        }
    }

    /// git の文のうち、帯に出す 1 行。
    var gitLine: String {
        let lines = message.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty && !$0.hasPrefix("hint:") }
        return lines.first { $0.hasPrefix("fatal:") || $0.hasPrefix("error:") } ?? lines.first ?? ""
    }

    /// 帯に出す次の一手。
    enum Remedy {
        case openWorktree(String)
        case openTerminal
        case retry
        case pull
        case workingCopy
        case setUpstream

        var title: String {
            switch self {
            case .openWorktree: L("Open Worktree")
            case .openTerminal: L("Open in Terminal")
            case .retry: L("Try Again")
            case .pull: L("Pull…")
            case .workingCopy: L("Open Working Copy")
            case .setUpstream: L("Set Upstream…")
            }
        }
    }

    var remedies: [Remedy] {
        let retry: [Remedy] = command == nil ? [] : [.retry]
        switch kind {
        case "worktreeInUse": return path.map { [.openWorktree($0)] } ?? []
        case "auth", "hostKey": return [.openTerminal] + retry
        case "network", "locked": return retry
        case "rejected", "diverged": return [.pull]
        case "localChanges": return [.workingCopy]
        case "noUpstream": return [.setUpstream]
        default: return []
        }
    }
}
