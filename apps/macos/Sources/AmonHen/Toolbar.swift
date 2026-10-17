import AppKit

/// ツールバー: 同期（Fetch・Pull・Push）、統合（Merge・Rebase）、退避（Stash・Pop）、状態（Refresh・Undo）。
/// ボタンの action は target を持たず、レスポンダチェーンで `RepositoryWindow` に届く（メニューと同じ操作）。
/// 使えるかどうかも `RepositoryWindow.validateToolbarItem` で決める。
final class MainToolbar: NSObject, NSToolbarDelegate {
    /// 並びを変えたら名前を変える（保存した並びに新しい項目が出てこないため）。
    let toolbar = NSToolbar(identifier: "main.v4")
    private let repositoryItem = NSToolbarItem(itemIdentifier: .repository)
    private let repositoryButton = NSButton()
    private let branchItem = NSMenuToolbarItem(itemIdentifier: .branch)
    private let searchItem = NSSearchToolbarItem(itemIdentifier: .search)
    /// 検索欄で ⏎ を押した（History を探す）。
    var onSearch: ((String) -> Void)?
    /// 検索欄で ⇧↩: 前の一致へ（-1）。
    var onSearchStep: ((Int) -> Void)?
    private let undoItem = NSToolbarItem(itemIdentifier: .undo)
    private let pushItem = NSMenuToolbarItem(itemIdentifier: .push)
    private let pullItem = NSMenuToolbarItem(itemIdentifier: .pull)
    private let activityItem = NSToolbarItem(itemIdentifier: .activity)
    private let spinner = NSProgressIndicator()

    override init() {
        super.init()
        toolbar.delegate = self
        toolbar.displayMode = .iconAndLabel
        toolbar.allowsUserCustomization = true
        toolbar.autosavesConfiguration = true
        setUndo(nil)
        // Activity: 操作を実行している間は回る。押すと Activity の画面。
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        let button = NSButton(
            image: NSImage(systemSymbolName: "waveform.path.ecg", accessibilityDescription: "Activity")!,
            target: nil, action: #selector(AppDelegate.showActivity(_:)))
        button.bezelStyle = .texturedRounded
        button.isBordered = false
        let stack = NSStackView(views: [spinner, button])
        stack.spacing = 2
        activityItem.view = stack
        activityItem.label = "Activity"
        activityItem.paletteLabel = "Activity"
        activityItem.toolTip = "実行した git とコマンド、操作ログ、画面の速さ（⌥⌘A）"
        NotificationCenter.default.addObserver(
            self, selector: #selector(runningChanged), name: .runningOperationsChanged, object: nil)
        // 今のリポジトリ。押すとリポジトリの一覧を出し、選んだものにこのタブを切り替える。
        repositoryButton.bezelStyle = .texturedRounded
        repositoryButton.image = NSImage(systemSymbolName: "chevron.down", accessibilityDescription: "リポジトリを切り替える")?
            .withSymbolConfiguration(.init(pointSize: 9, weight: .semibold))
        repositoryButton.imagePosition = .imageTrailing
        repositoryButton.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        repositoryButton.action = #selector(RepositoryWindow.showRepositoryList(_:))
        repositoryButton.setContentHuggingPriority(.required, for: .horizontal)
        repositoryItem.view = repositoryButton
        repositoryItem.label = "Repository"
        repositoryItem.paletteLabel = "Repository"
        repositoryItem.toolTip = "リポジトリを切り替える"
        // 今のブランチ。押すとブランチの切り替え（⌘⇧B）、▾ でブランチの操作。
        branchItem.image = NSImage(systemSymbolName: "arrow.triangle.branch", accessibilityDescription: "ブランチ")
        branchItem.label = "Branch"
        branchItem.paletteLabel = "Branch"
        branchItem.action = #selector(RepositoryWindow.switchBranch(_:))
        branchItem.toolTip = "ブランチを切り替える（⌘⇧B）"
        branchItem.menu = Self.menu([
            ("Switch Branch…", #selector(RepositoryWindow.switchBranch(_:))),
            ("New Branch…", #selector(RepositoryWindow.newBranch(_:))),
            ("Rename Branch…", #selector(RepositoryWindow.renameBranch(_:))),
            ("Set Upstream…", #selector(RepositoryWindow.setUpstream(_:))),
        ])
        // History の検索。▾ で、作者・パス・内容で探す書き方を入れる。
        let field = searchItem.searchField
        field.placeholderString = "History を検索"
        field.sendsWholeSearchString = true
        field.target = self
        field.action = #selector(searchSubmitted)
        let scopes = NSMenu(title: "検索")
        for (title, prefix) in [("作者で探す（author:）", "author:"), ("パスで探す（path:）", "path:"), ("内容で探す（contents:）", "contents:")] {
            let item = NSMenuItem(title: title, action: #selector(insertSearchPrefix(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = prefix
            scopes.addItem(item)
        }
        field.searchMenuTemplate = scopes
        searchItem.paletteLabel = "Search"
    }

    /// 今のリポジトリの名前を出す。
    func setRepository(_ name: String) {
        repositoryButton.title = name
        repositoryItem.toolTip = "\(name) — 押すとリポジトリを切り替える"
    }

    /// リポジトリを開いていないタブ（リポジトリのページ）: 名前の代わりに案内を出し、押せなくする。
    func showNoRepository() {
        repositoryButton.title = "リポジトリを選んでください"
        repositoryButton.isEnabled = false
        repositoryItem.toolTip = nil
    }

    /// リポジトリの一覧を出す位置（ツールバーにあれば、リポジトリ名のボタン）。
    var repositoryAnchor: NSView? { repositoryButton.window == nil ? nil : repositoryButton }

    /// 今のブランチを出す（detached なら短い ID）。
    func setBranch(_ name: String?) {
        branchItem.label = name ?? "detached"
        branchItem.toolTip = "\(name ?? "detached HEAD") — 押すとブランチを切り替える（⌘⇧B）"
    }

    /// History の検索欄の中身と揃える。
    func setSearch(_ query: String?) {
        let field = searchItem.searchField
        if field.currentEditor() == nil { field.stringValue = query ?? "" }
    }

    /// ⌘⇧F: 検索欄に移る。
    func focusSearch() {
        searchItem.beginSearchInteraction()
    }

    @objc private func searchSubmitted() {
        // ⇧↩ は前の一致へ（↩ は、同じ問い合わせなら次の一致へ）。ツールバーの検索欄はキーを横取りできないので、
        // 押されたときの修飾キーで見分ける。
        if let event = NSApp.currentEvent, event.type == .keyDown, event.modifierFlags.contains(.shift) {
            onSearchStep?(-1)
            return
        }
        onSearch?(searchItem.searchField.stringValue.trimmingCharacters(in: .whitespaces))
    }

    @objc private func insertSearchPrefix(_ sender: NSMenuItem) {
        guard let prefix = sender.representedObject as? String else { return }
        let field = searchItem.searchField
        let current = field.stringValue.trimmingCharacters(in: .whitespaces)
        field.stringValue = current.isEmpty ? prefix : "\(current) \(prefix)"
        field.window?.makeFirstResponder(field)
        field.currentEditor()?.moveToEndOfDocument(nil)
    }

    @objc private func runningChanged() {
        if RunningOperations.count > 0 {
            spinner.startAnimation(nil)
            activityItem.toolTip = "実行中の操作 \(RunningOperations.count) 個（押すと Activity）"
        } else {
            spinner.stopAnimation(nil)
            activityItem.toolTip = "実行した git とコマンド、操作ログ、画面の速さ（⌥⌘A）"
        }
    }

    func setUndo(_ label: String?) {
        // ラベルの長さでツールバーの幅が変わらないよう、操作の名前はツールチップに出す。
        undoItem.toolTip = label.map { "Undo: \($0)（⌥⌘Z）" } ?? "Undo できる操作はありません"
    }

    /// 今のブランチが上流より先（push できる）・後ろ（pull できる）にあるコミットの数を、ボタンのバッジに出す。
    func setTracking(ahead: Int, behind: Int) {
        if #available(macOS 26.0, *) {
            pushItem.badge = ahead > 0 ? .count(ahead) : nil
            pullItem.badge = behind > 0 ? .count(behind) : nil
        } else {
            // バッジのない macOS では、ラベルに数を添える。
            pushItem.label = ahead > 0 ? "Push ↑\(ahead)" : "Push"
            pullItem.label = behind > 0 ? "Pull ↓\(behind)" : "Pull"
        }
        pushItem.toolTip = ahead > 0 ? "上流より \(ahead) コミット先にあります（⌃⌘P）" : "Push（⌃⌘P）"
        pullItem.toolTip = behind > 0 ? "上流より \(behind) コミット後ろにあります（⌃⌘L）" : "Pull（⌃⌘L）"
    }

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.repositories, .repository, .branch, .fetch, .pull, .push, .space, .merge, .rebase, .space, .stash, .pop, .flexibleSpace,
         .activity, .refresh, .undo, .search]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.repositories, .repository, .branch, .search, .fetch, .pull, .push, .merge, .rebase, .stash, .pop, .newBranch, .activity, .refresh, .undo, .terminal, .finder,
         .space, .flexibleSpace]
    }

    func toolbar(
        _ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool
    ) -> NSToolbarItem? {
        if id == .activity { return activityItem }
        if id == .branch { return branchItem }
        if id == .repository { return repositoryItem }
        if id == .search { return searchItem }
        let item: NSToolbarItem, label: String, symbol: String, action: Selector, tip: String
        switch id {
        case .repositories:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Repositories", "square.grid.2x2",
                                                  #selector(AppDelegate.showRepositories(_:)), "リポジトリのページに切り替える（⌘⇧R）")
        case .fetch:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Fetch", "arrow.down.circle",
                                                  #selector(RepositoryWindow.fetch(_:)), "すべてのリモートを fetch（⌃⌘F）")
        case .pull:
            pullItem.menu = Self.menu([
                ("Pull with Merge", #selector(RepositoryWindow.pullMerge(_:))),
                ("Pull with Rebase", #selector(RepositoryWindow.pullRebase(_:))),
                ("Pull Fast-Forward Only", #selector(RepositoryWindow.pullFastForward(_:))),
            ])
            (item, label, symbol, action, tip) = (pullItem, "Pull", "arrow.down.to.line.circle",
                                                  #selector(RepositoryWindow.pull(_:)), "Pull（⌃⌘L）")
        case .push:
            pushItem.menu = Self.menu([
                ("Push", #selector(RepositoryWindow.push(_:))),
                ("Force Push (with Lease)…", #selector(RepositoryWindow.forcePush(_:))),
            ])
            (item, label, symbol, action, tip) = (pushItem, "Push", "arrow.up.circle",
                                                  #selector(RepositoryWindow.push(_:)), "Push（⌃⌘P）")
        case .merge:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Merge", "arrow.triangle.merge",
                                                  #selector(RepositoryWindow.mergeIntoCurrent(_:)),
                                                  "ブランチ・タグ・選んだコミットを今のブランチへ merge（⌥⌘M）")
        case .rebase:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Rebase", "arrow.triangle.pull",
                                                  #selector(RepositoryWindow.rebaseCurrent(_:)),
                                                  "今のブランチを、ブランチ・タグ・選んだコミットの上へ rebase（⌥⌘R）")
        case .stash:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Stash", "tray.and.arrow.down",
                                                  #selector(RepositoryWindow.stashChanges(_:)), "作業中の変更を退避（⌥⌘S）")
        case .pop:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Pop", "tray.and.arrow.up",
                                                  #selector(RepositoryWindow.popStash(_:)), "最新の stash を戻して消す（⌥⌘⇧S）")
        case .newBranch:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Branch", "plus.square.on.square",
                                                  #selector(RepositoryWindow.newBranch(_:)), "HEAD から新しいブランチ（⌥⌘B）")
        case .refresh:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Refresh", "arrow.clockwise",
                                                  #selector(RepositoryWindow.refreshRepository(_:)),
                                                  "History・ブランチ・Working Copy を読み直す（⌘R）")
        case .undo:
            (item, label, symbol, action, tip) = (undoItem, "Undo", "arrow.uturn.backward.circle",
                                                  #selector(RepositoryWindow.undoOperation(_:)), undoItem.toolTip ?? "")
        case .terminal:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Terminal", "terminal",
                                                  #selector(RepositoryWindow.openInTerminal(_:)), "ターミナルで開く")
        case .finder:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Finder", "folder",
                                                  #selector(RepositoryWindow.revealInFinder(_:)), "Finder で表示")
        default:
            return nil
        }
        if item.label.isEmpty { item.label = label }
        item.paletteLabel = label
        item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        item.action = action
        if item.toolTip == nil { item.toolTip = tip }
        return item
    }

    private static func menu(_ items: [(String, Selector)]) -> NSMenu {
        let menu = NSMenu()
        for (title, action) in items { menu.addItem(withTitle: title, action: action, keyEquivalent: "") }
        return menu
    }
}

extension NSToolbarItem.Identifier {
    static let fetch = Self("fetch")
    static let pull = Self("pull")
    static let push = Self("push")
    static let merge = Self("merge")
    static let rebase = Self("rebase")
    static let stash = Self("stash")
    static let pop = Self("pop")
    static let newBranch = Self("newBranch")
    static let refresh = Self("refresh")
    static let undo = Self("undo")
    static let terminal = Self("terminal")
    static let finder = Self("finder")
    static let activity = Self("activity")
    static let branch = Self("branch")
    static let repository = Self("repository")
    static let repositories = Self("repositories")
    static let search = Self("search")
}
