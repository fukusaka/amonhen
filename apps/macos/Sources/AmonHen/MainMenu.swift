import AmonhenKit
import AppKit

/// メニューバー。すべての操作をここから引けるようにし、ショートカットは `docs/ux.md` の規則で割り当てる。
/// ⌘ は macOS 標準と画面の切り替え、⌃⌘ はリモートとの同期、⌥⌘ はローカルの書き込み、⌘⇧ は開く・探す。
/// リポジトリへの操作は target を持たず、レスポンダチェーンで前にあるウィンドウの `RepositoryWindow` に届く。
extension AppDelegate: NSMenuItemValidation, NSMenuDelegate {
    func buildMenu() {
        let main = NSMenu()
        main.addItem(submenu(appMenu()))
        main.addItem(submenu(fileMenu()))
        main.addItem(submenu(editMenu()))
        main.addItem(submenu(viewMenu()))
        main.addItem(submenu(navigateMenu()))
        main.addItem(submenu(repositoryMenu()))
        main.addItem(submenu(commitMenu()))
        let window = windowMenu()
        main.addItem(submenu(window))
        let help = NSMenu(title: L("Help"))
        main.addItem(submenu(help))
        NSApp.mainMenu = main
        NSApp.windowsMenu = window
        NSApp.helpMenu = help
    }

    private func submenu(_ menu: NSMenu) -> NSMenuItem {
        let item = NSMenuItem(title: menu.title, action: nil, keyEquivalent: "")
        item.submenu = menu
        return item
    }

    private func appMenu() -> NSMenu {
        let menu = NSMenu(title: "Amon Hen")
        menu.addItem(withTitle: L("About Amon Hen"), action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(item(L("Settings…"), #selector(showSettings(_:)), ",", target: self))
        menu.addItem(.separator())
        let services = NSMenu(title: L("Services"))
        menu.addItem(submenu(services))
        NSApp.servicesMenu = services
        menu.addItem(.separator())
        menu.addItem(item(L("Hide Amon Hen"), #selector(NSApplication.hide(_:)), "h"))
        menu.addItem(item(L("Hide Others"), #selector(NSApplication.hideOtherApplications(_:)), "h", [.command, .option]))
        menu.addItem(item(L("Show All"), #selector(NSApplication.unhideAllApplications(_:)), ""))
        menu.addItem(.separator())
        menu.addItem(item(L("Quit Amon Hen"), #selector(NSApplication.terminate(_:)), "q"))
        return menu
    }

    private func fileMenu() -> NSMenu {
        let menu = NSMenu(title: L("File"))
        menu.addItem(item(L("New Tab"), #selector(newTab(_:)), "t", target: self))
        menu.addItem(item(L("New Window"), #selector(newWindow(_:)), "n", target: self))
        menu.addItem(item(L("Open…"), #selector(openDocument(_:)), "o", target: self))
        menu.addItem(item("Quick Open…", #selector(quickOpen(_:)), "o", [.command, .shift], target: self))
        menu.addItem(item("Clone…", #selector(cloneRepository(_:)), "c", [.command, .control], target: self))
        let recent = NSMenu(title: L("Open Recent"))
        recent.delegate = self
        menu.addItem(submenu(recent))
        menu.addItem(.separator())
        menu.addItem(item(L("Close"), #selector(NSWindow.performClose(_:)), "w"))
        return menu
    }

    private func editMenu() -> NSMenu {
        // テキスト欄でのコピーや取り消しのため、標準の項目を置く。
        let menu = NSMenu(title: L("Edit"))
        menu.addItem(item(L("Undo"), Selector(("undo:")), "z"))
        menu.addItem(item(L("Redo"), Selector(("redo:")), "z", [.command, .shift]))
        menu.addItem(.separator())
        menu.addItem(item(L("Cut"), #selector(NSText.cut(_:)), "x"))
        menu.addItem(item(L("Copy"), #selector(NSText.copy(_:)), "c"))
        menu.addItem(item(L("Paste"), #selector(NSText.paste(_:)), "v"))
        menu.addItem(item(L("Select All"), #selector(NSText.selectAll(_:)), "a"))
        menu.addItem(.separator())
        menu.addItem(item(L("Find…"), #selector(DiffDocumentView.showFindBar(_:)), "f"))
        menu.addItem(item(L("Find Next"), #selector(DiffDocumentView.findNext(_:)), "g"))
        menu.addItem(item(L("Find Previous"), #selector(DiffDocumentView.findPrevious(_:)), "g", [.command, .shift]))
        menu.addItem(item(L("Find in History…"), #selector(RepositoryWindow.findInHistory(_:)), "f", [.command, .shift]))
        return menu
    }

    private func viewMenu() -> NSMenu {
        let menu = NSMenu(title: L("View"))
        menu.delegate = self
        menu.addItem(item("Working Copy", #selector(RepositoryWindow.showWorkingCopy(_:)), "1"))
        menu.addItem(item("History", #selector(RepositoryWindow.showHistory(_:)), "2"))
        menu.addItem(item("Stashes", #selector(RepositoryWindow.showStashes(_:)), "4"))
        menu.addItem(item("Reflog", #selector(RepositoryWindow.showReflog(_:)), "5"))
        menu.addItem(.separator())
        let refresh = item(L("Refresh"), #selector(RepositoryWindow.refreshRepository(_:)), "r")
        refresh.toolTip = L("Reload History, branches and Working Copy (they normally reload on their own when something changes)")
        menu.addItem(refresh)
        menu.addItem(.separator())
        menu.addItem(item(L("Show Sidebar"), #selector(NSSplitViewController.toggleSidebar(_:)), "s", [.command, .control]))
        // タブが 1 つの窓でもタブバーを出す（タブをつかんで、ほかの窓のタブバーに落とせる）。出していれば、題は
        // メニューを開くときに Hide にする（AppKit は替えない）。
        menu.addItem(item(L("Show Tab Bar"), #selector(NSWindow.toggleTabBar(_:)), "t", [.command, .shift]))
        menu.addItem(.separator())
        menu.addItem(item(L("One Line per Commit"), #selector(toggleCompactRows(_:)), "", target: self))
        let items = NSMenuItem(title: L("History Items"), action: nil, keyEquivalent: "")
        items.toolTip = L("Choose what each commit's row in History shows besides the subject (also from the button at the right of History’s bar and the header’s context menu)")
        items.submenu = HistoryDisplayMenu.shared.itemsMenu()
        menu.addItem(items)
        let order = NSMenuItem(title: L("History Order"), action: nil, keyEquivalent: "")
        order.toolTip = L("How History orders commits (also for the fold and the branch filter)")
        order.submenu = HistoryDisplayMenu.shared.orderMenu()
        menu.addItem(order)
        let ancestors = item(L("Highlight Ancestors"), #selector(toggleHighlightAncestors(_:)), "", target: self)
        ancestors.toolTip = L("Draw only the selected commit’s ancestors and the lines between them in full color, and dim the rest")
        menu.addItem(ancestors)
        let mono = item(L("Monospaced Commit Message"), #selector(toggleMonospacedMessage(_:)), "", target: self)
        mono.toolTip = L("Show commit messages in a monospaced font (keeps bodies wrapped at 72 columns aligned)")
        menu.addItem(mono)
        let avatars = item(L("Show Avatars"), #selector(toggleAvatars(_:)), "", target: self)
        avatars.toolTip = L("Show Gravatar avatars in commit details (sends a hash of the email address to gravatar.com)")
        menu.addItem(avatars)
        let stacked = item(L("All Files in One Diff"), #selector(toggleStackedDiff(_:)), "", target: self)
        stacked.toolTip = L("Stack every file in a commit’s diff (when off, only the selected file)")
        menu.addItem(stacked)
        let fold = item(L("Fold Merged Branches"), #selector(toggleFoldMerged(_:)), "", target: self)
        fold.toolTip = L("In History, hide the commits brought in by a merge and show their count on the merge row (right-click to expand)")
        menu.addItem(fold)
        let tree = item(L("Changed Files as Tree"), #selector(toggleFileTree(_:)), "", target: self)
        tree.toolTip = L("Show a commit’s changed files as a tree to the left of the diff (when off, as a list between the message and the diff)")
        menu.addItem(tree)
        return menu
    }

    private func navigateMenu() -> NSMenu {
        let menu = NSMenu(title: L("Navigate"))
        menu.addItem(item(L("Switch Branch…"), #selector(RepositoryWindow.switchBranch(_:)), "b", [.command, .shift]))
        menu.addItem(item(L("Filter Branches…"), #selector(RepositoryWindow.filterBranches(_:)), "f", [.command, .option]))
        menu.addItem(item(L("Go to HEAD"), #selector(RepositoryWindow.goToHead(_:)), "h", [.command, .shift]))
        let down = String(UnicodeScalar(NSDownArrowFunctionKey)!), up = String(UnicodeScalar(NSUpArrowFunctionKey)!)
        menu.addItem(item(L("Go to Parent"), #selector(RepositoryWindow.goToParent(_:)), down, [.command, .option]))
        menu.addItem(item(L("Go to Child"), #selector(RepositoryWindow.goToChild(_:)), up, [.command, .option]))
        return menu
    }

    private func repositoryMenu() -> NSMenu {
        let menu = NSMenu(title: "Repository")
        menu.addItem(item("Fetch", #selector(RepositoryWindow.fetch(_:)), "f", [.command, .control]))
        menu.addItem(item("Pull", #selector(RepositoryWindow.pull(_:)), "l", [.command, .control]))
        let pullWith = NSMenu(title: L("Pull With"))
        pullWith.addItem(item(L("Follow git’s settings"), #selector(RepositoryWindow.pullGit(_:)), ""))
        pullWith.addItem(.separator())
        pullWith.addItem(item("Merge", #selector(RepositoryWindow.pullMerge(_:)), ""))
        pullWith.addItem(item("Rebase", #selector(RepositoryWindow.pullRebase(_:)), ""))
        pullWith.addItem(item("Fast-Forward Only", #selector(RepositoryWindow.pullFastForward(_:)), ""))
        menu.addItem(submenu(pullWith))
        menu.addItem(item("Push", #selector(RepositoryWindow.push(_:)), "p", [.command, .control]))
        menu.addItem(item("Force Push (with Lease)…", #selector(RepositoryWindow.forcePush(_:)), ""))
        menu.addItem(.separator())
        menu.addItem(item("New Branch…", #selector(RepositoryWindow.newBranch(_:)), "b", [.command, .option]))
        menu.addItem(item("Rename Branch…", #selector(RepositoryWindow.renameBranch(_:)), ""))
        menu.addItem(item(L("Set Upstream…"), #selector(RepositoryWindow.setUpstream(_:)), ""))
        menu.addItem(.separator())
        menu.addItem(item("New Worktree…", #selector(RepositoryWindow.newWorktree(_:)), ""))
        menu.addItem(item(L("Prune Worktrees"), #selector(RepositoryWindow.pruneWorktrees(_:)), ""))
        menu.addItem(item("New Tag…", #selector(RepositoryWindow.newTag(_:)), "t", [.command, .option]))
        menu.addItem(item(L("Add Remote…"), #selector(RepositoryWindow.addRemote(_:)), ""))
        menu.addItem(item("Merge…", #selector(RepositoryWindow.mergeIntoCurrent(_:)), "m", [.command, .option]))
        menu.addItem(item("Rebase…", #selector(RepositoryWindow.rebaseCurrent(_:)), "r", [.command, .option]))
        let compare = item(L("Compare with Previous Version"), #selector(RepositoryWindow.compareWithPreviousVersion(_:)), "")
        compare.toolTip = L("Compare the current branch with its version before it was last rewritten (a rebase, History edit, amend or reset), like git range-diff")
        menu.addItem(compare)
        menu.addItem(.separator())
        menu.addItem(item("Continue", #selector(RepositoryWindow.continueSequence(_:)), ""))
        menu.addItem(item("Skip", #selector(RepositoryWindow.skipSequence(_:)), ""))
        menu.addItem(item("Abort", #selector(RepositoryWindow.abortSequence(_:)), ""))
        menu.addItem(.separator())
        menu.addItem(item("Stash…", #selector(RepositoryWindow.stashChanges(_:)), "s", [.command, .option]))
        menu.addItem(item("Pop Stash", #selector(RepositoryWindow.popStash(_:)), "s", [.command, .option, .shift]))
        menu.addItem(.separator())
        menu.addItem(item("Undo", #selector(RepositoryWindow.undoOperation(_:)), "z", [.command, .option]))
        menu.addItem(.separator())
        menu.addItem(item(L("Repository Settings"), #selector(RepositoryWindow.showRepositorySettings(_:)), ";"))
        let rebuild = item(L("Rebuild History"), #selector(RepositoryWindow.rebuildHistory(_:)), "")
        rebuild.toolTip = L("Discard the History cache and rebuild it from scratch (over the shared daemon, the daemon’s too)")
        menu.addItem(rebuild)
        menu.addItem(.separator())
        menu.addItem(item(L("Create Pull Request…"), #selector(RepositoryWindow.createPullRequest(_:)), ""))
        menu.addItem(item(L("Open on GitHub"), #selector(RepositoryWindow.openOnGitHub(_:)), ""))
        menu.addItem(.separator())
        menu.addItem(item(L("Open in Terminal"), #selector(RepositoryWindow.openInTerminal(_:)), ""))
        menu.addItem(item(L("Reveal in Finder"), #selector(RepositoryWindow.revealInFinder(_:)), ""))
        return menu
    }

    private func commitMenu() -> NSMenu {
        let menu = NSMenu(title: "Commit")
        menu.addItem(item("Commit", #selector(RepositoryWindow.commitChanges(_:)), "\r"))
        menu.addItem(item("Commit and Push", #selector(RepositoryWindow.commitAndPush(_:)), "\r", [.command, .shift]))
        menu.addItem(item("Stage All", #selector(RepositoryWindow.stageAllChanges(_:)), ""))
        return menu
    }

    private func windowMenu() -> NSMenu {
        let menu = NSMenu(title: L("Window"))
        menu.addItem(item(L("Minimize"), #selector(NSWindow.performMiniaturize(_:)), "m"))
        menu.addItem(item(L("Zoom"), #selector(NSWindow.performZoom(_:)), ""))
        menu.addItem(.separator())
        // タブ（複数のリポジトリを 1 つのウィンドウで扱う）。
        menu.addItem(item(L("Show Next Tab"), #selector(NSWindow.selectNextTab(_:)), "]", [.command, .shift]))
        menu.addItem(item(L("Show Previous Tab"), #selector(NSWindow.selectPreviousTab(_:)), "[", [.command, .shift]))
        menu.addItem(item(L("Move Tab to New Window"), #selector(NSWindow.moveTabToNewWindow(_:)), ""))
        menu.addItem(item(L("Merge All Windows"), #selector(NSWindow.mergeAllWindows(_:)), ""))
        // 前の窓のタブを、選んだほかの窓のタブに移す（標準は新しい窓か、全部をまとめるかだけ）。中身は開くときに作る。
        let moveTab = NSMenu(title: L("Move Tab to Window"))
        moveTab.delegate = self
        let moveTabItem = NSMenuItem(title: moveTab.title, action: nil, keyEquivalent: "")
        moveTabItem.submenu = moveTab
        menu.addItem(moveTabItem)
        menu.addItem(.separator())
        menu.addItem(item("Repositories", #selector(showRepositories(_:)), "r", [.command, .shift], target: self))
        menu.addItem(item(L("Settings"), #selector(showSettings(_:)), "", target: self))
        menu.addItem(item("Activity", #selector(showActivity(_:)), "a", [.command, .option], target: self))
        menu.addItem(.separator())
        menu.addItem(item(L("Bring All to Front"), #selector(NSApplication.arrangeInFront(_:)), ""))
        return menu
    }

    private func item(
        _ title: String, _ action: Selector, _ key: String, _ modifiers: NSEvent.ModifierFlags = .command,
        target: AnyObject? = nil
    ) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = target
        return item
    }

    // MARK: - Open Recent

    func menuNeedsUpdate(_ menu: NSMenu) {
        if menu.title == L("Move Tab to Window") { return fillMoveTabMenu(menu) }
        if menu.title == L("View") {
            let shown = (NSApp.keyWindow ?? NSApp.mainWindow)?.tabGroup?.isTabBarVisible == true
            menu.items.first { $0.action == #selector(NSWindow.toggleTabBar(_:)) }?.title =
                shown ? L("Hide Tab Bar") : L("Show Tab Bar")
            return
        }
        guard menu.title == L("Open Recent") else { return }
        menu.removeAllItems()
        for path in RepositoryList.paths.prefix(15) {
            let title = RepositoryLocation.isRemote(path) ? path : (path as NSString).abbreviatingWithTildeInPath
            let item = NSMenuItem(title: title, action: #selector(openRecent(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = path
            menu.addItem(item)
        }
        if menu.items.isEmpty {
            menu.addItem(NSMenuItem(title: L("None"), action: nil, keyEquivalent: ""))
        }
    }

    @objc private func openRecent(_ sender: NSMenuItem) {
        guard let path = sender.representedObject as? String else { return }
        open(path: path)
    }

    // MARK: - Move Tab to Window

    /// 移す先: 前の窓とは別の、リポジトリ（と File History・Blame）の窓ごとに 1 つ。その窓で選んでいるタブの名前と、
    /// ほかのタブの数で出す。File History・Blame からは、開いたリポジトリの窓を先頭に。
    private func fillMoveTabMenu(_ menu: NSMenu) {
        menu.removeAllItems()
        let id = RepositoryWindow.tabbingIdentifier
        if let source = NSApp.keyWindow ?? NSApp.mainWindow, source.tabbingIdentifier == id {
            var groups: [NSWindowTabGroup] = []
            for w in NSApp.windows where w.tabbingIdentifier == id && w.isVisible {
                guard let group = w.tabGroup, group !== source.tabGroup, !groups.contains(where: { $0 === group }) else { continue }
                groups.append(group)
            }
            if let home = (source.windowController as? FileToolWindowController)?.repositoryWindow?.tabGroup,
               let i = groups.firstIndex(where: { $0 === home }) {
                groups.insert(groups.remove(at: i), at: 0)
            }
            for group in groups {
                guard let shown = group.selectedWindow ?? group.windows.first else { continue }
                let name = shown.tab.title.flatMap { $0.isEmpty ? nil : $0 } ?? shown.title
                let others = group.windows.count - 1
                let item = NSMenuItem(
                    title: others > 0 ? L("%@ (+%ld)", name, others) : name, action: #selector(moveTab(toWindow:)), keyEquivalent: "")
                item.target = self
                item.representedObject = shown
                menu.addItem(item)
            }
        }
        if menu.items.isEmpty {
            menu.addItem(NSMenuItem(title: L("None"), action: nil, keyEquivalent: ""))
        }
    }

    @objc private func moveTab(toWindow sender: NSMenuItem) {
        guard let target = sender.representedObject as? NSWindow, let source = NSApp.keyWindow ?? NSApp.mainWindow else { return }
        target.addTabbedWindow(source, ordered: .above)
        source.makeKeyAndOrderFront(nil)
    }

    // MARK: - 設定

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let on: Bool? = switch item.action {
        case #selector(toggleCompactRows(_:)): Settings.compactHistoryRows
        case #selector(toggleHighlightAncestors(_:)): Settings.highlightAncestors
        case #selector(toggleMonospacedMessage(_:)): Settings.monospacedCommitMessage
        case #selector(toggleAvatars(_:)): Settings.showAvatars
        case #selector(toggleStackedDiff(_:)): Settings.stackedCommitDiff
        case #selector(toggleFileTree(_:)): Settings.historyFileTree
        case #selector(toggleFoldMerged(_:)): Settings.foldMergedBranches
        default: nil
        }
        if let on { item.state = on ? .on : .off }
        return true
    }

    @objc func showSettings(_ sender: Any?) { SettingsWindowController.shared.show() }
    @objc func showActivity(_ sender: Any?) { ActivityWindowController.shared.show() }
    @objc func showRepositories(_ sender: Any?) { showHome() }
    @objc private func toggleAvatars(_ item: NSMenuItem) { Settings.showAvatars.toggle() }
    @objc private func toggleMonospacedMessage(_ item: NSMenuItem) { Settings.monospacedCommitMessage.toggle() }
    @objc private func toggleHighlightAncestors(_ item: NSMenuItem) { Settings.highlightAncestors.toggle() }
    @objc private func toggleCompactRows(_ item: NSMenuItem) { Settings.compactHistoryRows.toggle() }
    @objc private func toggleStackedDiff(_ item: NSMenuItem) { Settings.stackedCommitDiff.toggle() }
    @objc private func toggleFileTree(_ item: NSMenuItem) { Settings.historyFileTree.toggle() }
    @objc private func toggleFoldMerged(_ item: NSMenuItem) { Settings.foldMergedBranches.toggle() }
}
