import AppKit

/// ツールバー: 同期（Fetch・Pull・Push）、統合（Merge・Rebase）、退避（Stash・Pop）、状態（Refresh・Undo）。
/// ボタンの action は target を持たず、レスポンダチェーンで `RepositoryWindow` に届く（メニューと同じ操作）。
/// 使えるかどうかも `RepositoryWindow.validateToolbarItem` で決める。
final class MainToolbar: NSObject, NSToolbarDelegate {
    /// 並びを変えたら名前を変える（保存した並びに新しい項目が出てこないため）。
    let toolbar = NSToolbar(identifier: "main.v4")
    private let repositoryItem = NSToolbarItem(itemIdentifier: .repository)
    private let repositoryButton = NSButton()
    private let branchItem = NSToolbarItem(itemIdentifier: .branch)
    /// 今のブランチの名前を出すボタン（ラベルにすると、名前の長さでツールバーの並びがずれるため、幅を決める）。
    private let branchButton = NSButton()
    private let searchItem = NSSearchToolbarItem(itemIdentifier: .search)
    /// 検索欄で ⏎ を押した（History を探す）。
    var onSearch: ((String) -> Void)?
    /// 検索欄で ⇧↩: 前の一致へ（-1）。
    var onSearchStep: ((Int) -> Void)?
    private let undoItem = NSToolbarItem(itemIdentifier: .undo)
    /// Pull・Push は ▾ で選ばせず、押すと確認のシート（取り込み方もそこで選ぶ）。
    private let pushItem = NSToolbarItem(itemIdentifier: .push)
    private let pullItem = NSToolbarItem(itemIdentifier: .pull)
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
        activityItem.toolTip = L("Git and commands run, the operation log, and UI speed (⌥⌘A)")
        NotificationCenter.default.addObserver(
            self, selector: #selector(runningChanged), name: .runningOperationsChanged, object: nil)
        // 今のリポジトリ。押すとリポジトリの一覧を出し、選んだものにこのタブを切り替える。
        repositoryButton.bezelStyle = .texturedRounded
        repositoryButton.image = NSImage(systemSymbolName: "chevron.down", accessibilityDescription: L("Switch Repository"))?
            .withSymbolConfiguration(.init(pointSize: 9, weight: .semibold))
        repositoryButton.imagePosition = .imageTrailing
        repositoryButton.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        repositoryButton.action = #selector(RepositoryWindow.showRepositoryList(_:))
        // 幅は決めておく（タブを切り替えても、名前の長さで後ろのボタンや検索欄の位置が変わらないように）。長い名前は
        // 途中を省き、全体はツールチップに出す。
        repositoryButton.lineBreakMode = .byTruncatingMiddle
        repositoryButton.translatesAutoresizingMaskIntoConstraints = false
        repositoryButton.widthAnchor.constraint(equalToConstant: 200).isActive = true
        repositoryItem.view = repositoryButton
        repositoryItem.label = "Repository"
        repositoryItem.paletteLabel = "Repository"
        repositoryItem.toolTip = L("Switch Repository")
        // 今のブランチ。押すとブランチの切り替え（⌘⇧B）。名前の変更・上流は Repository メニュー。
        branchButton.bezelStyle = .texturedRounded
        branchButton.image = NSImage(systemSymbolName: "arrow.triangle.branch", accessibilityDescription: "Branch")
        branchButton.imagePosition = .imageLeading
        branchButton.lineBreakMode = .byTruncatingMiddle
        branchButton.action = #selector(RepositoryWindow.switchBranch(_:))
        branchButton.translatesAutoresizingMaskIntoConstraints = false
        branchButton.widthAnchor.constraint(equalToConstant: 150).isActive = true
        branchItem.view = branchButton
        branchItem.label = "Branch"
        branchItem.paletteLabel = "Branch"
        branchItem.toolTip = L("Switch Branch (⌘⇧B)")
        // History の検索。▾ で、作者・パス・内容で探す書き方を入れる。
        let field = searchItem.searchField
        field.placeholderString = L("Search History")
        field.sendsWholeSearchString = true
        field.target = self
        field.action = #selector(searchSubmitted)
        let scopes = NSMenu(title: L("Search"))
        for (title, prefix) in [(L("Search by Author (author:)"), "author:"), (L("Search by Path (path:)"), "path:"), (L("Search by Contents (contents:)"), "contents:")] {
            let item = NSMenuItem(title: title, action: #selector(insertSearchPrefix(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = prefix
            scopes.addItem(item)
        }
        field.searchMenuTemplate = scopes
        searchItem.label = L("Search")
        searchItem.paletteLabel = L("Search")
    }

    /// 今のリポジトリの名前を出す。
    /// `worktree` は書き込む先の worktree（開いたものでなければ）。名前に ✎ で添える。
    func setRepository(_ name: String, worktree: String? = nil) {
        repositoryButton.title = worktree.map { "\(name) ✎ \($0)" } ?? name
        repositoryItem.toolTip = worktree.map { L("%@ — editing the worktree %@. Click to switch repository", name, $0) }
            ?? L("%@ — click to switch repository", name)
    }

    /// リポジトリを開いていないタブ（リポジトリのページ）: 名前の代わりに案内を出し、押せなくする。
    func showNoRepository() {
        showUnopened(L("Choose a Repository"))
    }

    /// まだ開けていないリポジトリのタブ（つないでいる途中・つなげなかった）: `name` を出し、押せなくする。
    func showUnopened(_ name: String) {
        repositoryButton.title = name
        repositoryButton.isEnabled = false
        repositoryItem.toolTip = nil
        branchButton.title = "—"
        branchButton.isEnabled = false
    }

    /// リポジトリの一覧を出す位置（ツールバーにあれば、リポジトリ名のボタン）。
    var repositoryAnchor: NSView? { repositoryButton.window == nil ? nil : repositoryButton }

    /// 今のブランチを出す（detached なら短い ID）。
    func setBranch(_ name: String?) {
        branchButton.title = name ?? "detached"
        branchItem.toolTip = L("%@ — click to switch branch (⌘⇧B)", name ?? "detached HEAD")
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
            activityItem.toolTip = L("%ld operations running (click for Activity)", RunningOperations.count)
        } else {
            spinner.stopAnimation(nil)
            activityItem.toolTip = L("Git and commands run, the operation log, and UI speed (⌥⌘A)")
        }
    }

    func setUndo(_ label: String?) {
        // ラベルの長さでツールバーの幅が変わらないよう、操作の名前はツールチップに出す。
        undoItem.toolTip = label.map { L("Undo: %@ (⌥⌘Z)", $0) } ?? L("Nothing to Undo")
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
        pushItem.toolTip = ahead > 0 ? L("%ld commits ahead of upstream (⌃⌘P)", ahead) : L("Push (⌃⌘P)")
        pullItem.toolTip = behind > 0 ? L("%ld commits behind upstream (⌃⌘L)", behind) : L("Pull (⌃⌘L)")
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
                                                  #selector(AppDelegate.showRepositories(_:)), L("Show the Repositories page (⌘⇧R)"))
        case .fetch:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Fetch", "arrow.down.circle",
                                                  #selector(RepositoryWindow.fetch(_:)), L("Fetch all remotes (⌃⌘F)"))
        case .pull:
            (item, label, symbol, action, tip) = (pullItem, "Pull", "arrow.down.to.line.circle",
                                                  #selector(RepositoryWindow.pull(_:)), L("Pull — review the incoming commits and how to integrate them first (⌃⌘L)"))
        case .push:
            (item, label, symbol, action, tip) = (pushItem, "Push", "arrow.up.circle",
                                                  #selector(RepositoryWindow.push(_:)), L("Push — review the commits to send first (⌃⌘P)"))
        case .merge:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Merge", "arrow.triangle.merge",
                                                  #selector(RepositoryWindow.mergeIntoCurrent(_:)),
                                                  L("Merge a branch, tag or the selected commit into the current branch (⌥⌘M)"))
        case .rebase:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Rebase", "arrow.triangle.pull",
                                                  #selector(RepositoryWindow.rebaseCurrent(_:)),
                                                  L("Rebase the current branch onto a branch, tag or the selected commit (⌥⌘R)"))
        case .stash:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Stash", "tray.and.arrow.down",
                                                  #selector(RepositoryWindow.stashChanges(_:)), L("Stash your uncommitted changes (⌥⌘S)"))
        case .pop:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Pop", "tray.and.arrow.up",
                                                  #selector(RepositoryWindow.popStash(_:)), L("Apply the latest stash and remove it (⌥⌘⇧S)"))
        case .newBranch:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Branch", "plus.square.on.square",
                                                  #selector(RepositoryWindow.newBranch(_:)), L("New branch from HEAD (⌥⌘B)"))
        case .refresh:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), L("Refresh"), "arrow.clockwise",
                                                  #selector(RepositoryWindow.refreshRepository(_:)),
                                                  L("Reload History, branches and Working Copy (⌘R)"))
        case .undo:
            (item, label, symbol, action, tip) = (undoItem, "Undo", "arrow.uturn.backward.circle",
                                                  #selector(RepositoryWindow.undoOperation(_:)), undoItem.toolTip ?? "")
        case .terminal:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), L("Terminal"), "terminal",
                                                  #selector(RepositoryWindow.openInTerminal(_:)), L("Open in Terminal"))
        case .finder:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Finder", "folder",
                                                  #selector(RepositoryWindow.revealInFinder(_:)), L("Reveal in Finder"))
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
