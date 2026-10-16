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
        let help = NSMenu(title: "Help")
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
        menu.addItem(withTitle: "Amon Hen について", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(item("Settings…", #selector(showSettings(_:)), ",", target: self))
        menu.addItem(.separator())
        let services = NSMenu(title: "Services")
        menu.addItem(submenu(services))
        NSApp.servicesMenu = services
        menu.addItem(.separator())
        menu.addItem(item("Amon Hen を隠す", #selector(NSApplication.hide(_:)), "h"))
        menu.addItem(item("ほかを隠す", #selector(NSApplication.hideOtherApplications(_:)), "h", [.command, .option]))
        menu.addItem(item("すべてを表示", #selector(NSApplication.unhideAllApplications(_:)), ""))
        menu.addItem(.separator())
        menu.addItem(item("Amon Hen を終了", #selector(NSApplication.terminate(_:)), "q"))
        return menu
    }

    private func fileMenu() -> NSMenu {
        let menu = NSMenu(title: "File")
        menu.addItem(item("New Tab", #selector(newTab(_:)), "t", target: self))
        menu.addItem(item("New Window", #selector(newWindow(_:)), "n", target: self))
        menu.addItem(item("Open…", #selector(openDocument(_:)), "o", target: self))
        menu.addItem(item("Quick Open…", #selector(quickOpen(_:)), "o", [.command, .shift], target: self))
        let recent = NSMenu(title: "Open Recent")
        recent.delegate = self
        menu.addItem(submenu(recent))
        menu.addItem(.separator())
        menu.addItem(item("Close", #selector(NSWindow.performClose(_:)), "w"))
        return menu
    }

    private func editMenu() -> NSMenu {
        // テキスト欄でのコピーや取り消しのため、標準の項目を置く。
        let menu = NSMenu(title: "Edit")
        menu.addItem(item("Undo", Selector(("undo:")), "z"))
        menu.addItem(item("Redo", Selector(("redo:")), "z", [.command, .shift]))
        menu.addItem(.separator())
        menu.addItem(item("Cut", #selector(NSText.cut(_:)), "x"))
        menu.addItem(item("Copy", #selector(NSText.copy(_:)), "c"))
        menu.addItem(item("Paste", #selector(NSText.paste(_:)), "v"))
        menu.addItem(item("Select All", #selector(NSText.selectAll(_:)), "a"))
        menu.addItem(.separator())
        menu.addItem(item("Find…", #selector(DiffDocumentView.showFindBar(_:)), "f"))
        menu.addItem(item("Find Next", #selector(DiffDocumentView.findNext(_:)), "g"))
        menu.addItem(item("Find Previous", #selector(DiffDocumentView.findPrevious(_:)), "g", [.command, .shift]))
        menu.addItem(item("Find in History…", #selector(RepositoryWindow.findInHistory(_:)), "f", [.command, .shift]))
        return menu
    }

    private func viewMenu() -> NSMenu {
        let menu = NSMenu(title: "View")
        menu.addItem(item("Working Copy", #selector(RepositoryWindow.showWorkingCopy(_:)), "1"))
        menu.addItem(item("History", #selector(RepositoryWindow.showHistory(_:)), "2"))
        menu.addItem(.separator())
        let refresh = item("Refresh", #selector(RepositoryWindow.refreshRepository(_:)), "r")
        refresh.toolTip = "History・ブランチ・Working Copy を読み直す（ふだんは変化を検知して自動で読み直す）"
        menu.addItem(refresh)
        menu.addItem(.separator())
        menu.addItem(item("Show Sidebar", #selector(NSSplitViewController.toggleSidebar(_:)), "s", [.command, .control]))
        menu.addItem(.separator())
        menu.addItem(item("One Line per Commit", #selector(toggleCompactRows(_:)), "", target: self))
        let ancestors = item("Highlight Ancestors", #selector(toggleHighlightAncestors(_:)), "", target: self)
        ancestors.toolTip = "選んだコミットの祖先と、それをつなぐ線だけを濃く描き、ほかを薄くする"
        menu.addItem(ancestors)
        let mono = item("Monospaced Commit Message", #selector(toggleMonospacedMessage(_:)), "", target: self)
        mono.toolTip = "コミットの本文を等幅の字体で出す（72 桁で改行した本文が崩れない）"
        menu.addItem(mono)
        let avatars = item("Show Avatars", #selector(toggleAvatars(_:)), "", target: self)
        avatars.toolTip = "コミットの詳細に Gravatar のアバターを出す（メールアドレスのハッシュを gravatar.com に送る）"
        menu.addItem(avatars)
        let stacked = item("All Files in One Diff", #selector(toggleStackedDiff(_:)), "", target: self)
        stacked.toolTip = "コミットの差分で、全ファイルを縦に並べる（オフなら選んだファイルだけ）"
        menu.addItem(stacked)
        return menu
    }

    private func navigateMenu() -> NSMenu {
        let menu = NSMenu(title: "Navigate")
        menu.addItem(item("Go to HEAD", #selector(RepositoryWindow.goToHead(_:)), "h", [.command, .shift]))
        let down = String(UnicodeScalar(NSDownArrowFunctionKey)!), up = String(UnicodeScalar(NSUpArrowFunctionKey)!)
        menu.addItem(item("Go to Parent", #selector(RepositoryWindow.goToParent(_:)), down, [.command, .option]))
        menu.addItem(item("Go to Child", #selector(RepositoryWindow.goToChild(_:)), up, [.command, .option]))
        return menu
    }

    private func repositoryMenu() -> NSMenu {
        let menu = NSMenu(title: "Repository")
        menu.addItem(item("Fetch", #selector(RepositoryWindow.fetch(_:)), "f", [.command, .control]))
        menu.addItem(item("Pull", #selector(RepositoryWindow.pull(_:)), "l", [.command, .control]))
        let pullWith = NSMenu(title: "Pull With")
        pullWith.addItem(item("Merge", #selector(RepositoryWindow.pullMerge(_:)), ""))
        pullWith.addItem(item("Rebase", #selector(RepositoryWindow.pullRebase(_:)), ""))
        pullWith.addItem(item("Fast-Forward Only", #selector(RepositoryWindow.pullFastForward(_:)), ""))
        menu.addItem(submenu(pullWith))
        menu.addItem(item("Push", #selector(RepositoryWindow.push(_:)), "p", [.command, .control]))
        menu.addItem(item("Force Push (with Lease)…", #selector(RepositoryWindow.forcePush(_:)), ""))
        menu.addItem(.separator())
        menu.addItem(item("New Branch…", #selector(RepositoryWindow.newBranch(_:)), "b", [.command, .option]))
        menu.addItem(item("New Tag…", #selector(RepositoryWindow.newTag(_:)), "t", [.command, .option]))
        menu.addItem(item("Merge…", #selector(RepositoryWindow.mergeIntoCurrent(_:)), "m", [.command, .option]))
        menu.addItem(item("Rebase…", #selector(RepositoryWindow.rebaseCurrent(_:)), "r", [.command, .option]))
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
        menu.addItem(item("Open in Terminal", #selector(RepositoryWindow.openInTerminal(_:)), ""))
        menu.addItem(item("Reveal in Finder", #selector(RepositoryWindow.revealInFinder(_:)), ""))
        return menu
    }

    private func commitMenu() -> NSMenu {
        let menu = NSMenu(title: "Commit")
        menu.addItem(item("Commit", #selector(RepositoryWindow.commitChanges(_:)), "\r"))
        menu.addItem(item("Stage All", #selector(RepositoryWindow.stageAllChanges(_:)), ""))
        return menu
    }

    private func windowMenu() -> NSMenu {
        let menu = NSMenu(title: "Window")
        menu.addItem(item("最小化", #selector(NSWindow.performMiniaturize(_:)), "m"))
        menu.addItem(item("拡大／縮小", #selector(NSWindow.performZoom(_:)), ""))
        menu.addItem(.separator())
        // タブ（複数のリポジトリを 1 つのウィンドウで扱う）。
        menu.addItem(item("Show Next Tab", #selector(NSWindow.selectNextTab(_:)), "]", [.command, .shift]))
        menu.addItem(item("Show Previous Tab", #selector(NSWindow.selectPreviousTab(_:)), "[", [.command, .shift]))
        menu.addItem(item("Move Tab to New Window", #selector(NSWindow.moveTabToNewWindow(_:)), ""))
        menu.addItem(item("Merge All Windows", #selector(NSWindow.mergeAllWindows(_:)), ""))
        menu.addItem(.separator())
        menu.addItem(item("Repositories", #selector(showRepositories(_:)), "r", [.command, .shift], target: self))
        menu.addItem(item("Settings", #selector(showSettings(_:)), "", target: self))
        menu.addItem(.separator())
        menu.addItem(item("すべてを手前に移動", #selector(NSApplication.arrangeInFront(_:)), ""))
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
        guard menu.title == "Open Recent" else { return }
        menu.removeAllItems()
        for path in RepositoryList.paths.prefix(15) {
            let title = RepositoryLocation.isRemote(path) ? path : (path as NSString).abbreviatingWithTildeInPath
            let item = NSMenuItem(title: title, action: #selector(openRecent(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = path
            menu.addItem(item)
        }
        if menu.items.isEmpty {
            menu.addItem(NSMenuItem(title: "なし", action: nil, keyEquivalent: ""))
        }
    }

    @objc private func openRecent(_ sender: NSMenuItem) {
        guard let path = sender.representedObject as? String else { return }
        open(path: path)
    }

    // MARK: - 設定

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let on: Bool? = switch item.action {
        case #selector(toggleCompactRows(_:)): Settings.compactHistoryRows
        case #selector(toggleHighlightAncestors(_:)): Settings.highlightAncestors
        case #selector(toggleMonospacedMessage(_:)): Settings.monospacedCommitMessage
        case #selector(toggleAvatars(_:)): Settings.showAvatars
        case #selector(toggleStackedDiff(_:)): Settings.stackedCommitDiff
        default: nil
        }
        if let on { item.state = on ? .on : .off }
        return true
    }

    @objc func showSettings(_ sender: Any?) { SettingsWindowController.shared.show() }
    @objc func showRepositories(_ sender: Any?) { RepositoryManagerWindowController.shared.show() }
    @objc private func toggleAvatars(_ item: NSMenuItem) { Settings.showAvatars.toggle() }
    @objc private func toggleMonospacedMessage(_ item: NSMenuItem) { Settings.monospacedCommitMessage.toggle() }
    @objc private func toggleHighlightAncestors(_ item: NSMenuItem) { Settings.highlightAncestors.toggle() }
    @objc private func toggleCompactRows(_ item: NSMenuItem) { Settings.compactHistoryRows.toggle() }
    @objc private func toggleStackedDiff(_ item: NSMenuItem) { Settings.stackedCommitDiff.toggle() }
}
