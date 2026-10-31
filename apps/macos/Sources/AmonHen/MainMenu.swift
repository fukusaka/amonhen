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

    /// メニューバーの中身を 1 項目 1 行で出す（`--print-menus`）。
    func printMenus() {
        func walk(_ menu: NSMenu, _ path: String) {
            for item in menu.items {
                if item.isSeparatorItem {
                    print("\(path) | ---")
                    continue
                }
                if let sub = item.submenu {
                    walk(sub, path.isEmpty ? item.title : "\(path) > \(item.title)")
                    continue
                }
                let mods = item.keyEquivalent.isEmpty ? "" : item.keyEquivalentModifierMask.intersection([.command, .control, .option, .shift])
                    .rawValue.description
                let key = item.keyEquivalent.unicodeScalars.map { String(format: "%04X", $0.value) }.joined()
                print("\(path) | \(item.title) | \(key) \(mods) | \(item.identifier?.rawValue ?? "") | \(item.toolTip ?? "") | "
                    + "\(item.action.map(NSStringFromSelector) ?? "") | \(item.target.map { String(describing: type(of: $0)) } ?? "")")
            }
        }
        if let main = NSApp.mainMenu { walk(main, "") }
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
        add([Commands.settings], to: menu)
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

    /// 表の操作を並べる（nil は区切り）。
    private func add(_ commands: [Command?], to menu: NSMenu) {
        for command in commands {
            menu.addItem(command.map { $0.menuItem(appTarget: self) } ?? .separator())
        }
    }

    private func fileMenu() -> NSMenu {
        let menu = NSMenu(title: L("File"))
        menu.addItem(item(L("New Tab"), #selector(newTab(_:)), "t", target: self))
        menu.addItem(item(L("New Window"), #selector(newWindow(_:)), "n", target: self))
        menu.addItem(item(L("Open…"), #selector(openDocument(_:)), "o", target: self))
        add([Commands.quickOpen], to: menu)
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
        add([Commands.findInHistory], to: menu)
        return menu
    }

    private func viewMenu() -> NSMenu {
        let menu = NSMenu(title: L("View"))
        menu.delegate = self
        add([Commands.showWorkingCopy, Commands.showHistory, Commands.showStashes, Commands.showReflog, nil,
             Commands.refresh, Commands.rebuildHistory, nil, Commands.showSidebar], to: menu)
        // タブが 1 つの窓でもタブバーを出す（タブをつかんで、ほかの窓のタブバーに落とせる）。出していれば、題は
        // メニューを開くときに Hide にする（AppKit は替えない）。
        menu.addItem(item(L("Show Tab Bar"), #selector(NSWindow.toggleTabBar(_:)), "t", [.command, .shift]))
        menu.addItem(.separator())
        // History の表示（History の上のバーの表示ボタンと同じもの）。
        for display in HistoryDisplayMenu.shared.displayMenu().items {
            display.menu?.removeItem(display)
            menu.addItem(display)
        }
        return menu
    }

    private func navigateMenu() -> NSMenu {
        let menu = NSMenu(title: L("Navigate"))
        add([Commands.switchBranch, Commands.filterBranches, Commands.goToHead, Commands.goToParent, Commands.goToChild], to: menu)
        return menu
    }

    private func repositoryMenu() -> NSMenu {
        let menu = NSMenu(title: "Repository")
        add([
            Commands.fetch, Commands.pull, Commands.push, Commands.forcePush, nil,
            Commands.newBranch, Commands.renameBranch, Commands.setUpstream, Commands.newTag, nil,
            Commands.newWorktree, Commands.pruneWorktrees, Commands.addRemote, nil,
            Commands.merge, Commands.rebase, Commands.compareWithPrevious, Commands.applyPatch, nil,
            Commands.continueSequence, Commands.skipSequence, Commands.abortSequence, nil,
            Commands.stash, Commands.popStash, nil,
            Commands.undo, nil,
            Commands.repositorySettings, nil,
            Commands.createPullRequest, Commands.openOnGitHub, nil,
            Commands.openInTerminal, Commands.revealInFinder,
        ], to: menu)
        return menu
    }

    private func commitMenu() -> NSMenu {
        let menu = NSMenu(title: "Commit")
        add([Commands.commit, Commands.commitAndPush, Commands.stageAll, nil, Commands.savePatch], to: menu)
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
        add([Commands.repositories, Commands.activity], to: menu)
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

    func validateMenuItem(_ item: NSMenuItem) -> Bool { true }

    @objc func showSettings(_ sender: Any?) { SettingsWindowController.shared.show() }
    @objc func showActivity(_ sender: Any?) { ActivityWindowController.shared.show() }
    @objc func showRepositories(_ sender: Any?) { showHome() }
}
