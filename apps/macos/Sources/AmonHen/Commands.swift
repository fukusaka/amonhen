import AppKit

/// 操作の表の、macOS の側。表そのもの（id・名前・ショートカット・説明・メニューの区切り）は crates/commands にあり、
/// そこから作った `CommandTable.swift` を使う（docs/ux.md の「Menus」）。ここでは呼び先（Selector）だけを結びつける。
/// メニューバー・ツールバーの ≡・右クリックの名前は、ここから作る（同じ操作を同じ名前で出す）。
///
/// 名前の決まり: git の操作と画面の名前（Fetch・Push・Working Copy など）は英語のまま、それ以外は `L()` で訳す。
/// シートを開くものだけ「…」で終える。
struct Command {
    /// どこが受けるか。`window` はレスポンダチェーンで前の `RepositoryWindow`、`app` は `AppDelegate`。
    enum Target { case window, app }

    let id: String
    let title: String
    let action: Selector
    let key: String
    let modifiers: NSEvent.ModifierFlags
    let target: Target
    let toolTip: String?

    /// 表（`CommandTable`）の操作を `action` に結びつける。名前と説明は、表の決まりどおりに訳す。
    init(_ entry: CommandTable.Entry, action: Selector) {
        id = entry.id
        title = entry.localized ? L(entry.title) : entry.title
        self.action = action
        key = entry.key
        modifiers = entry.modifiers
        target = entry.app ? .app : .window
        toolTip = entry.toolTip.map { L($0) }
    }

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

/// 操作の表（`CommandTable`。元は crates/commands）の呼び先。
@MainActor
enum Commands {
    static let fetch = Command(CommandTable.fetch, action: #selector(RepositoryWindow.fetch(_:)))
    static let pull = Command(CommandTable.pull, action: #selector(RepositoryWindow.pull(_:)))
    static let push = Command(CommandTable.push, action: #selector(RepositoryWindow.push(_:)))
    static let forcePush = Command(CommandTable.forcePush, action: #selector(RepositoryWindow.forcePush(_:)))
    static let setUpstream = Command(CommandTable.setUpstream, action: #selector(RepositoryWindow.setUpstream(_:)))
    static let addRemote = Command(CommandTable.addRemote, action: #selector(RepositoryWindow.addRemote(_:)))
    static let createPullRequest = Command(CommandTable.createPullRequest, action: #selector(RepositoryWindow.createPullRequest(_:)))
    static let openOnGitHub = Command(CommandTable.openOnGithub, action: #selector(RepositoryWindow.openOnGitHub(_:)))
    static let switchBranch = Command(CommandTable.switchBranch, action: #selector(RepositoryWindow.switchBranch(_:)))
    static let newBranch = Command(CommandTable.newBranch, action: #selector(RepositoryWindow.newBranch(_:)))
    static let renameBranch = Command(CommandTable.renameBranch, action: #selector(RepositoryWindow.renameBranch(_:)))
    static let newTag = Command(CommandTable.newTag, action: #selector(RepositoryWindow.newTag(_:)))
    static let newWorktree = Command(CommandTable.newWorktree, action: #selector(RepositoryWindow.newWorktree(_:)))
    static let pruneWorktrees = Command(CommandTable.pruneWorktrees, action: #selector(RepositoryWindow.pruneWorktrees(_:)))
    static let merge = Command(CommandTable.merge, action: #selector(RepositoryWindow.mergeIntoCurrent(_:)))
    static let rebase = Command(CommandTable.rebase, action: #selector(RepositoryWindow.rebaseCurrent(_:)))
    static let compareWithPrevious = Command(CommandTable.compareWithPrevious, action: #selector(RepositoryWindow.compareWithPreviousVersion(_:)))
    static let applyPatch = Command(CommandTable.applyPatch, action: #selector(RepositoryWindow.applyPatch(_:)))
    static let continueSequence = Command(CommandTable.`continue`, action: #selector(RepositoryWindow.continueSequence(_:)))
    static let skipSequence = Command(CommandTable.skip, action: #selector(RepositoryWindow.skipSequence(_:)))
    static let abortSequence = Command(CommandTable.abort, action: #selector(RepositoryWindow.abortSequence(_:)))
    static let stash = Command(CommandTable.stash, action: #selector(RepositoryWindow.stashChanges(_:)))
    static let popStash = Command(CommandTable.popStash, action: #selector(RepositoryWindow.popStash(_:)))
    static let undo = Command(CommandTable.undo, action: #selector(RepositoryWindow.undoOperation(_:)))
    static let commit = Command(CommandTable.commit, action: #selector(RepositoryWindow.commitChanges(_:)))
    static let commitAndPush = Command(CommandTable.commitAndPush, action: #selector(RepositoryWindow.commitAndPush(_:)))
    static let stageAll = Command(CommandTable.stageAll, action: #selector(RepositoryWindow.stageAllChanges(_:)))
    static let savePatch = Command(CommandTable.savePatch, action: #selector(RepositoryWindow.savePatch(_:)))
    static let showWorkingCopy = Command(CommandTable.showWorkingCopy, action: #selector(RepositoryWindow.showWorkingCopy(_:)))
    static let showHistory = Command(CommandTable.showHistory, action: #selector(RepositoryWindow.showHistory(_:)))
    static let showStashes = Command(CommandTable.showStashes, action: #selector(RepositoryWindow.showStashes(_:)))
    static let showReflog = Command(CommandTable.showReflog, action: #selector(RepositoryWindow.showReflog(_:)))
    static let refresh = Command(CommandTable.refresh, action: #selector(RepositoryWindow.refreshRepository(_:)))
    static let rebuildHistory = Command(CommandTable.rebuildHistory, action: #selector(RepositoryWindow.rebuildHistory(_:)))
    static let showSidebar = Command(CommandTable.showSidebar, action: #selector(NSSplitViewController.toggleSidebar(_:)))
    static let filterBranches = Command(CommandTable.filterBranches, action: #selector(RepositoryWindow.filterBranches(_:)))
    static let goToHead = Command(CommandTable.goToHead, action: #selector(RepositoryWindow.goToHead(_:)))
    static let goToParent = Command(CommandTable.goToParent, action: #selector(RepositoryWindow.goToParent(_:)))
    static let goToChild = Command(CommandTable.goToChild, action: #selector(RepositoryWindow.goToChild(_:)))
    static let findInHistory = Command(CommandTable.findInHistory, action: #selector(RepositoryWindow.findInHistory(_:)))
    static let repositorySettings = Command(CommandTable.repositorySettings, action: #selector(RepositoryWindow.showRepositorySettings(_:)))
    static let openInTerminal = Command(CommandTable.openInTerminal, action: #selector(RepositoryWindow.openInTerminal(_:)))
    static let revealInFinder = Command(CommandTable.revealInFiles, action: #selector(RepositoryWindow.revealInFinder(_:)))
    static let quickOpen = Command(CommandTable.quickOpen, action: #selector(AppDelegate.quickOpen(_:)))
    static let repositories = Command(CommandTable.repositories, action: #selector(AppDelegate.showRepositories(_:)))
    static let activity = Command(CommandTable.activity, action: #selector(AppDelegate.showActivity(_:)))
    static let settings = Command(CommandTable.settings, action: #selector(AppDelegate.showSettings(_:)))

    static let all: [Command] = [
        fetch,
        pull,
        push,
        forcePush,
        setUpstream,
        addRemote,
        createPullRequest,
        openOnGitHub,
        switchBranch,
        newBranch,
        renameBranch,
        newTag,
        newWorktree,
        pruneWorktrees,
        merge,
        rebase,
        compareWithPrevious,
        applyPatch,
        continueSequence,
        skipSequence,
        abortSequence,
        stash,
        popStash,
        undo,
        commit,
        commitAndPush,
        stageAll,
        savePatch,
        showWorkingCopy,
        showHistory,
        showStashes,
        showReflog,
        refresh,
        rebuildHistory,
        showSidebar,
        filterBranches,
        goToHead,
        goToParent,
        goToChild,
        findInHistory,
        repositorySettings,
        openInTerminal,
        revealInFinder,
        quickOpen,
        repositories,
        activity,
        settings,
    ]

    /// 表のメニューの区切りを、この FE の操作にする。
    static func menu(_ sections: [[CommandTable.Entry]]) -> [Command?] {
        var out: [Command?] = []
        for (i, section) in sections.enumerated() {
            if i > 0 { out.append(nil) }
            out += section.compactMap { entry in all.first { $0.id == entry.id } }
        }
        return out
    }
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
    nonisolated static let shortIDLength = 8
}
