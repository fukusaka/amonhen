import AppKit

/// 操作の表。メニューバー・ツールバーの ≡・右クリックの名前は、ここから作る（同じ操作を同じ名前で出す。
/// docs/ux.md の「Menus」）。`id` は GTK4 の FE の action 名（`win.<id>`）と同じにする。
///
/// 名前の決まり: git の操作と画面の名前（Fetch・Push・Working Copy など）は英語のまま、それ以外は `L()` で訳す。
/// シートを開くものだけ「…」で終える。
struct Command {
    /// どこが受けるか。`window` はレスポンダチェーンで前の `RepositoryWindow`、`app` は `AppDelegate`。
    enum Target { case window, app }

    let id: String
    let title: String
    let action: Selector
    var key = ""
    var modifiers: NSEvent.ModifierFlags = .command
    var target = Target.window
    var toolTip: String?

    /// メニューの項目。`app` の操作は `appTarget` に送る。
    @MainActor
    func menuItem(appTarget: AnyObject? = nil) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = target == .app ? appTarget : nil
        item.toolTip = toolTip
        item.identifier = NSUserInterfaceItemIdentifier(id)
        return item
    }
}

@MainActor
enum Commands {
    // MARK: - リモート

    static let fetch = Command(id: "fetch", title: "Fetch…", action: #selector(RepositoryWindow.fetch(_:)), key: "f",
                               modifiers: [.command, .control])
    static let pull = Command(id: "pull", title: "Pull…", action: #selector(RepositoryWindow.pull(_:)), key: "l",
                              modifiers: [.command, .control])
    static let push = Command(id: "push", title: "Push…", action: #selector(RepositoryWindow.push(_:)), key: "p",
                              modifiers: [.command, .control])
    static let forcePush = Command(id: "force-push", title: "Force Push…", action: #selector(RepositoryWindow.forcePush(_:)))
    static let setUpstream = Command(id: "set-upstream", title: L("Set Upstream…"), action: #selector(RepositoryWindow.setUpstream(_:)))
    static let addRemote = Command(id: "add-remote", title: L("Add Remote…"), action: #selector(RepositoryWindow.addRemote(_:)))
    static let createPullRequest = Command(id: "create-pull-request", title: L("Create Pull Request…"),
                                           action: #selector(RepositoryWindow.createPullRequest(_:)))
    static let openOnGitHub = Command(id: "open-on-github", title: L("Open on GitHub"), action: #selector(RepositoryWindow.openOnGitHub(_:)))

    // MARK: - ブランチ・タグ・worktree

    static let switchBranch = Command(id: "switch-branch", title: L("Switch Branch…"), action: #selector(RepositoryWindow.switchBranch(_:)),
                                      key: "b", modifiers: [.command, .shift])
    static let newBranch = Command(id: "new-branch", title: "New Branch…", action: #selector(RepositoryWindow.newBranch(_:)), key: "b",
                                   modifiers: [.command, .option])
    static let renameBranch = Command(id: "rename-branch", title: L("Rename Branch…"), action: #selector(RepositoryWindow.renameBranch(_:)))
    static let newTag = Command(id: "new-tag", title: "New Tag…", action: #selector(RepositoryWindow.newTag(_:)), key: "t",
                                modifiers: [.command, .option])
    static let newWorktree = Command(id: "new-worktree", title: "New Worktree…", action: #selector(RepositoryWindow.newWorktree(_:)))
    static let pruneWorktrees = Command(id: "prune-worktrees", title: L("Prune Worktrees"), action: #selector(RepositoryWindow.pruneWorktrees(_:)))

    // MARK: - 取り込み・書き換え

    static let merge = Command(id: "merge", title: "Merge…", action: #selector(RepositoryWindow.mergeIntoCurrent(_:)), key: "m",
                               modifiers: [.command, .option])
    static let rebase = Command(id: "rebase", title: "Rebase…", action: #selector(RepositoryWindow.rebaseCurrent(_:)), key: "r",
                                modifiers: [.command, .option])
    static let compareWithPrevious = Command(
        id: "compare-with-previous", title: L("Compare with Previous Version"),
        action: #selector(RepositoryWindow.compareWithPreviousVersion(_:)),
        toolTip: L("Compare the current branch with its version before it was last rewritten (a rebase, History edit, amend or reset), like git range-diff"))
    static let applyPatch = Command(id: "apply-patch", title: L("Apply Patch…"), action: #selector(RepositoryWindow.applyPatch(_:)))
    static let continueSequence = Command(id: "continue", title: "Continue", action: #selector(RepositoryWindow.continueSequence(_:)))
    static let skipSequence = Command(id: "skip", title: "Skip", action: #selector(RepositoryWindow.skipSequence(_:)))
    static let abortSequence = Command(id: "abort", title: "Abort", action: #selector(RepositoryWindow.abortSequence(_:)))
    static let stash = Command(id: "stash", title: "Stash…", action: #selector(RepositoryWindow.stashChanges(_:)), key: "s",
                               modifiers: [.command, .option])
    static let popStash = Command(id: "pop-stash", title: "Pop…", action: #selector(RepositoryWindow.popStash(_:)), key: "s",
                                  modifiers: [.command, .option, .shift])
    static let undo = Command(id: "undo", title: "Undo…", action: #selector(RepositoryWindow.undoOperation(_:)), key: "z",
                              modifiers: [.command, .option])

    // MARK: - コミット

    static let commit = Command(id: "commit", title: "Commit", action: #selector(RepositoryWindow.commitChanges(_:)), key: "\r")
    static let commitAndPush = Command(id: "commit-and-push", title: "Commit and Push", action: #selector(RepositoryWindow.commitAndPush(_:)),
                                       key: "\r", modifiers: [.command, .shift])
    static let stageAll = Command(id: "stage-all", title: "Stage All", action: #selector(RepositoryWindow.stageAllChanges(_:)))
    static let savePatch = Command(id: "save-patch", title: L("Save as Patch…"), action: #selector(RepositoryWindow.savePatch(_:)))

    // MARK: - 画面・移動

    static let showWorkingCopy = Command(id: "show-working-copy", title: "Working Copy", action: #selector(RepositoryWindow.showWorkingCopy(_:)), key: "1")
    static let showHistory = Command(id: "show-history", title: "History", action: #selector(RepositoryWindow.showHistory(_:)), key: "2")
    static let showStashes = Command(id: "show-stashes", title: "Stashes", action: #selector(RepositoryWindow.showStashes(_:)), key: "4")
    static let showReflog = Command(id: "show-reflog", title: "Reflog", action: #selector(RepositoryWindow.showReflog(_:)), key: "5")
    static let refresh = Command(
        id: "refresh", title: L("Refresh"), action: #selector(RepositoryWindow.refreshRepository(_:)), key: "r",
        toolTip: L("Reload History, branches and Working Copy (they normally reload on their own when something changes)"))
    static let rebuildHistory = Command(
        id: "rebuild-history", title: L("Rebuild History"), action: #selector(RepositoryWindow.rebuildHistory(_:)),
        toolTip: L("Discard the History cache and rebuild it from scratch (over the shared daemon, the daemon’s too)"))
    static let showSidebar = Command(id: "show-sidebar", title: L("Show Sidebar"), action: #selector(NSSplitViewController.toggleSidebar(_:)),
                                     key: "s", modifiers: [.command, .control])
    static let filterBranches = Command(id: "filter-branches", title: L("Filter Branches…"), action: #selector(RepositoryWindow.filterBranches(_:)),
                                        key: "f", modifiers: [.command, .option])
    static let goToHead = Command(id: "go-to-head", title: L("Go to HEAD"), action: #selector(RepositoryWindow.goToHead(_:)), key: "h",
                                  modifiers: [.command, .shift])
    static let goToParent = Command(id: "go-to-parent", title: L("Go to Parent"), action: #selector(RepositoryWindow.goToParent(_:)),
                                    key: String(UnicodeScalar(NSDownArrowFunctionKey)!), modifiers: [.command, .option])
    static let goToChild = Command(id: "go-to-child", title: L("Go to Child"), action: #selector(RepositoryWindow.goToChild(_:)),
                                   key: String(UnicodeScalar(NSUpArrowFunctionKey)!), modifiers: [.command, .option])
    static let findInHistory = Command(id: "find-in-history", title: L("Find in History…"), action: #selector(RepositoryWindow.findInHistory(_:)),
                                       key: "f", modifiers: [.command, .shift])
    static let repositorySettings = Command(id: "repository-settings", title: L("Repository Settings"),
                                            action: #selector(RepositoryWindow.showRepositorySettings(_:)), key: ";")
    static let openInTerminal = Command(id: "open-in-terminal", title: L("Open in Terminal"), action: #selector(RepositoryWindow.openInTerminal(_:)))
    static let revealInFinder = Command(id: "reveal-in-files", title: L("Show in Finder"), action: #selector(RepositoryWindow.revealInFinder(_:)))

    // MARK: - アプリ

    static let quickOpen = Command(id: "quick-open", title: "Quick Open…", action: #selector(AppDelegate.quickOpen(_:)), key: "o",
                                   modifiers: [.command, .shift], target: .app)
    static let repositories = Command(id: "repositories", title: "Repositories", action: #selector(AppDelegate.showRepositories(_:)),
                                      key: "r", modifiers: [.command, .shift], target: .app)
    static let activity = Command(id: "activity", title: "Activity", action: #selector(AppDelegate.showActivity(_:)), key: "a",
                                  modifiers: [.command, .option], target: .app)
    static let settings = Command(id: "settings", title: L("Settings…"), action: #selector(AppDelegate.showSettings(_:)), key: ",",
                                  target: .app)
}

/// 右クリックとドラッグのメニューの、対象の入った名前。同じ操作はどこでも同じ言い方にする（History と Reflog の
/// 「New Branch at」と、サイドバーの「New Branch from」のような揺れをなくす）。
@MainActor
enum Titles {
    static func checkout(_ name: String) -> String { L("Checkout %@…", name) }
    static func checkoutAsLocal(_ remote: String) -> String { L("Checkout %@ as Local Branch…", remote) }
    static func checkoutDetached(_ short: String) -> String { L("Checkout %@ (Detached)…", short) }
    static func merge(_ rev: String, into target: String) -> String { L("Merge %@ into %@…", rev, target) }
    static func rebase(_ target: String, onto rev: String) -> String { L("Rebase %@ onto %@…", target, rev) }
    static func newBranch(from start: String) -> String { L("New Branch from %@…", start) }
    static func newTag(at target: String) -> String { L("New Tag at %@…", target) }
    static func reset(_ branch: String, to short: String) -> String { L("Reset %@ to %@…", branch, short) }
    static func cherryPick(_ short: String) -> String { L("Cherry-pick %@…", short) }
    static func revert(_ short: String) -> String { L("Revert %@…", short) }
    static func push(_ branch: String, to target: String) -> String { L("Push %@ to %@…", branch, target) }
    static func forcePush(_ branch: String, to target: String) -> String { L("Force Push %@ to %@…", branch, target) }
    static func apply(_ stash: String) -> String { L("Apply %@…", stash) }
    static func pop(_ stash: String) -> String { L("Pop %@…", stash) }
    static func blame(at short: String?) -> String { short.map { L("Blame at %@", $0) } ?? "Blame" }
    static let fileHistory = "File History"
    static let copyCommitID = L("Copy Commit ID")
    static let copyCommitIDs = L("Copy Commit IDs")
    static let copyShortID = L("Copy Short ID")
    static let copyShortIDs = L("Copy Short IDs")
    static let copyBranchName = L("Copy Branch Name")
    static let copyTagName = L("Copy Tag Name")
    static let copyPath = L("Copy Path")
    static let showInFinder = L("Show in Finder")

    /// 短いコミット ID の桁（どこでも同じ）。
    static let shortIDLength = 8
}
