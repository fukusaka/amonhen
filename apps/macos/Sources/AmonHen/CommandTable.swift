// 生成したファイル。手で直さない: 元は crates/commands（`AMONHEN_UPDATE=1 cargo test -p amonhen-commands` で作り直す）。
import AppKit

/// 操作の表（crates/commands の `COMMANDS`）。呼び先は `Commands` が結びつける。
enum CommandTable {
    struct Entry {
        let id: String
        /// 英語の名前（`localized` なら訳の鍵）。
        let title: String
        let localized: Bool
        let key: String
        let modifiers: NSEvent.ModifierFlags
        /// 説明（訳の鍵）。
        let toolTip: String?
        /// アプリ（`AppDelegate`）が受けるか。
        let app: Bool
    }

    static let fetch = Entry(
        id: "fetch", title: "Fetch…", localized: false, key: "f", modifiers: [.command, .control],
        toolTip: nil, app: false)
    static let pull = Entry(
        id: "pull", title: "Pull…", localized: false, key: "l", modifiers: [.command, .control],
        toolTip: nil, app: false)
    static let push = Entry(
        id: "push", title: "Push…", localized: false, key: "p", modifiers: [.command, .control],
        toolTip: nil, app: false)
    static let forcePush = Entry(
        id: "force-push", title: "Force Push…", localized: false, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let setUpstream = Entry(
        id: "set-upstream", title: "Set Upstream…", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let addRemote = Entry(
        id: "add-remote", title: "Add Remote…", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let createPullRequest = Entry(
        id: "create-pull-request", title: "Create Pull Request…", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let openOnGithub = Entry(
        id: "open-on-github", title: "Open on GitHub", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let switchBranch = Entry(
        id: "switch-branch", title: "Switch Branch…", localized: true, key: "b", modifiers: [.command, .shift],
        toolTip: nil, app: false)
    static let newBranch = Entry(
        id: "new-branch", title: "New Branch…", localized: false, key: "b", modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let renameBranch = Entry(
        id: "rename-branch", title: "Rename Branch…", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let newTag = Entry(
        id: "new-tag", title: "New Tag…", localized: false, key: "t", modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let newWorktree = Entry(
        id: "new-worktree", title: "New Worktree…", localized: false, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let pruneWorktrees = Entry(
        id: "prune-worktrees", title: "Prune Worktrees", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let merge = Entry(
        id: "merge", title: "Merge…", localized: false, key: "m", modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let rebase = Entry(
        id: "rebase", title: "Rebase…", localized: false, key: "r", modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let compareWithPrevious = Entry(
        id: "compare-with-previous", title: "Compare with Previous Version", localized: true, key: "", modifiers: [.command],
        toolTip: "Compare the current branch with its version before it was last rewritten (a rebase, History edit, amend or reset), like git range-diff", app: false)
    static let applyPatch = Entry(
        id: "apply-patch", title: "Apply Patch…", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let `continue` = Entry(
        id: "continue", title: "Continue", localized: false, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let skip = Entry(
        id: "skip", title: "Skip", localized: false, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let abort = Entry(
        id: "abort", title: "Abort", localized: false, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let stash = Entry(
        id: "stash", title: "Stash…", localized: false, key: "s", modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let popStash = Entry(
        id: "pop-stash", title: "Pop…", localized: false, key: "s", modifiers: [.command, .option, .shift],
        toolTip: nil, app: false)
    static let undo = Entry(
        id: "undo", title: "Undo…", localized: false, key: "z", modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let commit = Entry(
        id: "commit", title: "Commit", localized: false, key: "\r", modifiers: [.command],
        toolTip: nil, app: false)
    static let commitAndPush = Entry(
        id: "commit-and-push", title: "Commit and Push", localized: false, key: "\r", modifiers: [.command, .shift],
        toolTip: nil, app: false)
    static let stageAll = Entry(
        id: "stage-all", title: "Stage All", localized: false, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let savePatch = Entry(
        id: "save-patch", title: "Save as Patch…", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let showWorkingCopy = Entry(
        id: "show-working-copy", title: "Working Copy", localized: false, key: "1", modifiers: [.command],
        toolTip: nil, app: false)
    static let showHistory = Entry(
        id: "show-history", title: "History", localized: false, key: "2", modifiers: [.command],
        toolTip: nil, app: false)
    static let showStashes = Entry(
        id: "show-stashes", title: "Stashes", localized: false, key: "4", modifiers: [.command],
        toolTip: nil, app: false)
    static let showReflog = Entry(
        id: "show-reflog", title: "Reflog", localized: false, key: "5", modifiers: [.command],
        toolTip: nil, app: false)
    static let refresh = Entry(
        id: "refresh", title: "Refresh", localized: true, key: "r", modifiers: [.command],
        toolTip: "Reload History, branches and Working Copy (they normally reload on their own when something changes)", app: false)
    static let rebuildHistory = Entry(
        id: "rebuild-history", title: "Rebuild History", localized: true, key: "", modifiers: [.command],
        toolTip: "Discard the History cache and rebuild it from scratch (over the shared daemon, the daemon’s too)", app: false)
    static let showSidebar = Entry(
        id: "show-sidebar", title: "Show Sidebar", localized: true, key: "s", modifiers: [.command, .control],
        toolTip: nil, app: false)
    static let filterBranches = Entry(
        id: "filter-branches", title: "Filter Branches…", localized: true, key: "f", modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let goToHead = Entry(
        id: "go-to-head", title: "Go to HEAD", localized: true, key: "h", modifiers: [.command, .shift],
        toolTip: nil, app: false)
    static let goToParent = Entry(
        id: "go-to-parent", title: "Go to Parent", localized: true, key: String(UnicodeScalar(NSDownArrowFunctionKey)!), modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let goToChild = Entry(
        id: "go-to-child", title: "Go to Child", localized: true, key: String(UnicodeScalar(NSUpArrowFunctionKey)!), modifiers: [.command, .option],
        toolTip: nil, app: false)
    static let findInHistory = Entry(
        id: "find-in-history", title: "Find in History…", localized: true, key: "f", modifiers: [.command, .shift],
        toolTip: nil, app: false)
    static let repositorySettings = Entry(
        id: "repository-settings", title: "Repository Settings", localized: true, key: ";", modifiers: [.command],
        toolTip: nil, app: false)
    static let openInTerminal = Entry(
        id: "open-in-terminal", title: "Open in Terminal", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let revealInFiles = Entry(
        id: "reveal-in-files", title: "Show in Finder", localized: true, key: "", modifiers: [.command],
        toolTip: nil, app: false)
    static let quickOpen = Entry(
        id: "quick-open", title: "Quick Open…", localized: false, key: "o", modifiers: [.command, .shift],
        toolTip: nil, app: true)
    static let repositories = Entry(
        id: "repositories", title: "Repositories", localized: false, key: "r", modifiers: [.command, .shift],
        toolTip: nil, app: true)
    static let activity = Entry(
        id: "activity", title: "Activity", localized: false, key: "a", modifiers: [.command, .option],
        toolTip: nil, app: true)
    static let settings = Entry(
        id: "settings", title: "Settings…", localized: true, key: ",", modifiers: [.command],
        toolTip: nil, app: true)

    /// メニューの区切り（上から）。
    static let viewMenu: [[Entry]] = [
        [showWorkingCopy, showHistory, showStashes, showReflog],
        [refresh, rebuildHistory],
        [showSidebar],
    ]
    static let navigateMenu: [[Entry]] = [
        [switchBranch, filterBranches, goToHead, goToParent, goToChild],
    ]
    static let repositoryMenu: [[Entry]] = [
        [fetch, pull, push, forcePush],
        [newBranch, renameBranch, setUpstream, newTag],
        [newWorktree, pruneWorktrees, addRemote],
        [merge, rebase, compareWithPrevious, applyPatch],
        [`continue`, skip, abort],
        [stash, popStash],
        [undo],
        [repositorySettings],
        [createPullRequest, openOnGithub],
        [openInTerminal, revealInFiles],
    ]
    static let commitMenu: [[Entry]] = [
        [commit, commitAndPush, stageAll],
        [savePatch],
    ]
}
