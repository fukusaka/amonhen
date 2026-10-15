import AmonhenKit
import AppKit

/// 利用者の設定。
enum Settings {
    /// commit-graph がないリポジトリで `git commit-graph write --reachable` を実行するか。
    /// リポジトリの `.git` に書き込むので、既定では行わない。
    static var writeCommitGraph: Bool {
        get { UserDefaults.standard.bool(forKey: "writeCommitGraph") }
        set { UserDefaults.standard.set(newValue, forKey: "writeCommitGraph") }
    }

    /// 手元のリポジトリの status で git の fsmonitor を使うか（既定は使う）。リポジトリごとに
    /// `git fsmonitor--daemon` が常駐し、status が速くなる。
    static var useFSMonitor: Bool {
        get { UserDefaults.standard.object(forKey: "useFSMonitor") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "useFSMonitor")
            Repository.setFSMonitor(newValue)
        }
    }

    /// SSH 先のリポジトリのコミットの署名を、手元の鍵で検証するか（既定は手元）。false なら SSH 先の git と鍵で
    /// 検証する。変えたら `signatureCheckChanged` を知らせる。
    static var checkRemoteSignaturesLocally: Bool {
        get { UserDefaults.standard.object(forKey: "checkRemoteSignaturesLocally") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "checkRemoteSignaturesLocally")
            NotificationCenter.default.post(name: .signatureCheckChanged, object: nil)
        }
    }

    /// コミットの詳細に作者・コミッターの Gravatar のアバターを出すか（既定は出す。メールアドレスの MD5 を
    /// gravatar.com に送る）。変えたら `commitMessageStyleChanged` を知らせる。
    static var showAvatars: Bool {
        get { UserDefaults.standard.object(forKey: "showAvatars") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "showAvatars")
            NotificationCenter.default.post(name: .commitMessageStyleChanged, object: nil)
        }
    }

    /// コミットの本文を等幅で出すか（既定は等幅）。変えたら `commitMessageStyleChanged` を知らせる。
    static var monospacedCommitMessage: Bool {
        get { UserDefaults.standard.object(forKey: "monospacedCommitMessage") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "monospacedCommitMessage")
            NotificationCenter.default.post(name: .commitMessageStyleChanged, object: nil)
        }
    }

    /// 選んだコミットの祖先を強調するか（既定はする）。変えたら `highlightAncestorsChanged` を知らせる。
    static var highlightAncestors: Bool {
        get { UserDefaults.standard.object(forKey: "highlightAncestors") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "highlightAncestors")
            NotificationCenter.default.post(name: .highlightAncestorsChanged, object: nil)
        }
    }

    /// History の行を 1 行にするか（既定は件名と作者・日時の 2 行）。変えたら `historyRowsChanged` を知らせる。
    static var compactHistoryRows: Bool {
        get { UserDefaults.standard.bool(forKey: "compactHistoryRows") }
        set {
            UserDefaults.standard.set(newValue, forKey: "compactHistoryRows")
            NotificationCenter.default.post(name: .historyRowsChanged, object: nil)
        }
    }
}

extension Notification.Name {
    static let historyRowsChanged = Notification.Name("historyRowsChanged")
    static let highlightAncestorsChanged = Notification.Name("highlightAncestorsChanged")
    static let signatureCheckChanged = Notification.Name("signatureCheckChanged")
    static let commitMessageStyleChanged = Notification.Name("commitMessageStyleChanged")
}

/// daemon 越しに開くときに起動する `amonhend`。アプリの中（Contents/MacOS）にあればそれを、開発中は
/// リポジトリの target/release のものを使う。`AMONHEN_DAEMON` で差し替えられる。
enum Daemon {
    static var path: String {
        if let override = ProcessInfo.processInfo.environment["AMONHEN_DAEMON"] { return override }
        if let bundled = Bundle.main.url(forAuxiliaryExecutable: "amonhend") { return bundled.path }
        return developmentFile("target/release/amonhend") ?? "amonhend"
    }

    /// SSH 先に送り込む `amonhend-<os>-<arch>` の置き場所（アプリの Resources/daemons、開発中は dist/linux）。
    static var bundledDirectory: String {
        if let resources = Bundle.main.resourceURL?.appendingPathComponent("daemons"),
           FileManager.default.fileExists(atPath: resources.path) {
            return resources.path
        }
        return developmentFile("dist/linux") ?? ""
    }

    /// 開発中: 実行ファイルの場所から上へ辿り、リポジトリの中の `relative` を探す。
    private static func developmentFile(_ relative: String) -> String? {
        var dir = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath().deletingLastPathComponent()
        while dir.path != "/" {
            let candidate = dir.appendingPathComponent(relative)
            if FileManager.default.fileExists(atPath: candidate.path) { return candidate.path }
            dir.deleteLastPathComponent()
        }
        return nil
    }
}

/// 子の画面を 1 つだけ表示し、切り替える。
final class PageViewController: NSViewController {
    private let pages: [SidebarViewController.Page: NSViewController]
    private var current: NSViewController?

    init(pages: [SidebarViewController.Page: NSViewController]) {
        self.pages = pages
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: 900, height: 700))
    }

    func show(_ page: SidebarViewController.Page) {
        guard let next = pages[page], next !== current else { return }
        current?.view.removeFromSuperview()
        current?.removeFromParent()
        addChild(next)
        next.view.frame = view.bounds
        next.view.autoresizingMask = [.width, .height]
        view.addSubview(next.view)
        current = next
    }
}

/// ツールバー: Fetch・Push・Undo。
final class MainToolbar: NSObject, NSToolbarDelegate {
    let toolbar = NSToolbar(identifier: "main")
    private let actions: Actions
    private let undoItem = NSToolbarItem(itemIdentifier: .init("undo"))
    private let pushItem = NSToolbarItem(itemIdentifier: .init("push"))
    private let pullItem = NSToolbarItem(itemIdentifier: .init("pull"))

    init(actions: Actions) {
        self.actions = actions
        super.init()
        toolbar.delegate = self
        toolbar.displayMode = .iconAndLabel
        setUndo(nil)
    }

    func setUndo(_ label: String?) {
        undoItem.isEnabled = label != nil
        // ラベルの長さでツールバーの幅が変わらないよう、操作の名前はツールチップに出す。
        undoItem.toolTip = label.map { "Undo: \($0)" } ?? "Undo できる操作はありません"
        undoItem.label = "Undo"
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
        pushItem.toolTip = ahead > 0 ? "上流より \(ahead) コミット先にあります" : "Push"
        pullItem.toolTip = behind > 0 ? "上流より \(behind) コミット後ろにあります" : "Pull"
    }

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.init("fetch"), .init("pull"), .init("push"), .init("stash"), .flexibleSpace, .init("undo")]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    func toolbar(
        _ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool
    ) -> NSToolbarItem? {
        let item: NSToolbarItem, symbol: String, action: Selector
        switch id.rawValue {
        case "fetch": (item, symbol, action) = (NSToolbarItem(itemIdentifier: id), "arrow.down.circle", #selector(fetch))
        case "push": (item, symbol, action) = (pushItem, "arrow.up.circle", #selector(push))
        case "pull": (item, symbol, action) = (pullItem, "arrow.down.to.line.circle", #selector(pull))
        case "stash": (item, symbol, action) = (NSToolbarItem(itemIdentifier: id), "tray.and.arrow.down", #selector(stash))
        case "undo": (item, symbol, action) = (undoItem, "arrow.uturn.backward.circle", #selector(undo))
        default: return nil
        }
        if item.label.isEmpty { item.label = id.rawValue.capitalized }
        item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: item.label)
        item.target = self
        item.action = action
        return item
    }

    @objc private func fetch() { actions.fetch() }
    @objc private func push() { actions.push() }
    @objc private func pull() { actions.pull() }
    @objc private func stash() { actions.stash() }
    @objc private func undo() { actions.undo() }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    /// 開いているリポジトリのウィンドウ。
    private var windows: [RepositoryWindow] = [] {
        didSet {
            // 終了するときにウィンドウが閉じても、開いていたものの記録は消さない（次の起動で開き直す）。
            if !isTerminating, !Self.isCheckRun { RepositoryList.open = windows.map(\.location) }
        }
    }
    private var isTerminating = false
    /// 確認用の起動（画面を撮る・計測する）。開いていたリポジトリの記録を書き換えない。
    static let isCheckRun = ["--snapshot", "--measure-scroll", "--log-scroll", "--measure-select"].contains { CommandLine.arguments.contains($0) }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        isTerminating = true
        return .terminateNow
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        buildMenu()
        Repository.setFSMonitor(Settings.useFSMonitor)
        RepositoryManagerWindowController.shared.onOpen = { [weak self] path in self?.open(path: path) }
        // 引数でリポジトリを渡されたらそれを、なければ前回開いていたものを開く。どれもなければ一覧を出す。
        if let path = repositoryPath() {
            open(path: path, debug: true)
        } else {
            for path in RepositoryList.open where RepositoryList.isRepository(path) {
                open(path: path)
            }
            if windows.isEmpty, connecting.isEmpty { RepositoryManagerWindowController.shared.show() }
        }
        NSApp.activate()
    }

    /// リポジトリを新しいウィンドウで開く。すでに開いていれば前に出す。
    /// `path` は手元のパスか、daemon 越しの場所（`daemon:/path`・`ssh://host/path`）。
    func open(path: String, debug: Bool = false) {
        let location = RepositoryLocation.isRemote(path) ? path : URL(fileURLWithPath: path).standardizedFileURL.path
        if let existing = windows.first(where: { $0.location == location }) {
            existing.window.makeKeyAndOrderFront(nil)
            return
        }
        if RepositoryLocation.isRemote(location) {
            connect(location: location, debug: debug)
            return
        }
        do {
            add(try RepositoryWindow(location: location, debug: debug))
        } catch {
            NSAlert(error: error).runModal()
            if windows.isEmpty, connecting.isEmpty { RepositoryManagerWindowController.shared.show() }
        }
    }

    /// つないでいる daemon 越しのリポジトリ（同じ場所を二重につながない）。
    private var connecting: [ConnectingWindow] = []

    /// daemon 越しのリポジトリに裏でつなぎ、つながったらウィンドウを開く。その間は接続中のウィンドウを出す。
    private func connect(location: String, debug: Bool) {
        if let existing = connecting.first(where: { $0.location == location }) {
            existing.window.makeKeyAndOrderFront(nil)
            return
        }
        let progress = ConnectingWindow(location: location)
        progress.onCancel = { [weak self, weak progress] in
            self?.connecting.removeAll { $0 === progress }
            if self?.windows.isEmpty == true, self?.connecting.isEmpty == true {
                RepositoryManagerWindowController.shared.show()
            }
        }
        connecting.append(progress)
        RepositoryManagerWindowController.shared.close()
        let daemon = Daemon.path, daemons = Daemon.bundledDirectory
        Task {
            let result = await Task.detached(priority: .userInitiated) {
                Result { try RepositoryWindow.openRepository(location: location, daemon: daemon, daemons: daemons) }
            }.value
            // 取り消されていたら、つないだ結果を捨てる（Repository を手放すと daemon も終わる）。
            guard !progress.isCancelled else { return }
            progress.finish()
            connecting.removeAll { $0 === progress }
            do {
                add(try RepositoryWindow(location: location, repository: try result.get(), debug: debug))
            } catch {
                let alert = NSAlert(error: error)
                alert.messageText = "\(RepositoryLocation.host(location) ?? location) に接続できませんでした"
                alert.informativeText = String(describing: error)
                alert.runModal()
                if windows.isEmpty, connecting.isEmpty { RepositoryManagerWindowController.shared.show() }
            }
        }
    }

    private func add(_ window: RepositoryWindow) {
        window.onOpenRepository = { [weak self] path in self?.open(path: path) }
        window.onReconnect = { [weak self, weak window] in
            guard let self, let window else { return }
            // 先につなぎ始めてから閉じる（閉じたときにリポジトリの一覧を出さないため）。
            connect(location: window.location, debug: false)
            window.window.close()
        }
        window.onClose = { [weak self, weak window] in
            self?.windows.removeAll { $0 === window }
            // 最後のリポジトリを閉じたら、終わらずにリポジトリの一覧を出す。
            if self?.windows.isEmpty == true, self?.connecting.isEmpty == true {
                RepositoryManagerWindowController.shared.show()
            }
        }
        windows.append(window)
        RepositoryList.touch(window.location)
        RepositoryManagerWindowController.shared.close()
    }

    @objc private func openDocument(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.message = "リポジトリを選んでください"
        if panel.runModal() == .OK, let url = panel.url {
            open(path: url.path)
        }
    }

    private func buildMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        main.addItem(appItem)
        let appMenu = NSMenu()
        appItem.submenu = appMenu
        let commitGraph = NSMenuItem(
            title: "commit-graph を自動で書き出す", action: #selector(toggleWriteCommitGraph(_:)), keyEquivalent: "")
        commitGraph.target = self
        commitGraph.state = Settings.writeCommitGraph ? .on : .off
        commitGraph.toolTip = "commit-graph がないリポジトリを開いたとき、裏で git commit-graph write --reachable --changed-paths を実行する（.git に書き込む。履歴の読み込みと path: での検索が速くなる）"
        appMenu.addItem(commitGraph)
        let fsmonitor = NSMenuItem(
            title: "fsmonitor で status を速くする", action: #selector(toggleFSMonitor(_:)), keyEquivalent: "")
        fsmonitor.target = self
        fsmonitor.state = Settings.useFSMonitor ? .on : .off
        fsmonitor.toolTip = "手元のリポジトリの status で git の fsmonitor を使う（リポジトリごとに git fsmonitor--daemon が常駐し、.git にソケットを作る）"
        appMenu.addItem(fsmonitor)
        let signatures = NSMenuItem(
            title: "SSH 先の署名を手元の鍵で検証する", action: #selector(toggleRemoteSignatureCheck(_:)), keyEquivalent: "")
        signatures.target = self
        signatures.state = Settings.checkRemoteSignaturesLocally ? .on : .off
        signatures.toolTip = "SSH 先のリポジトリのコミットの署名を、手元の gpg・allowed_signers・信用度で検証する（外すと SSH 先の git と鍵で検証する）"
        appMenu.addItem(signatures)
        appMenu.addItem(.separator())
        appMenu.addItem(NSMenuItem(title: "Amon Hen を終了", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))

        let fileItem = NSMenuItem()
        main.addItem(fileItem)
        let fileMenu = NSMenu(title: "File")
        fileItem.submenu = fileMenu
        let open = NSMenuItem(title: "Open…", action: #selector(openDocument(_:)), keyEquivalent: "o")
        open.target = self
        fileMenu.addItem(open)
        let manager = NSMenuItem(title: "Repositories…", action: #selector(showRepositories(_:)), keyEquivalent: "O")
        manager.target = self
        fileMenu.addItem(manager)
        fileMenu.addItem(NSMenuItem(title: "Close Window", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w"))

        let viewItem = NSMenuItem()
        main.addItem(viewItem)
        let viewMenu = NSMenu(title: "View")
        viewItem.submenu = viewMenu
        let compact = NSMenuItem(title: "One Line per Commit", action: #selector(toggleCompactRows(_:)), keyEquivalent: "")
        compact.target = self
        compact.state = Settings.compactHistoryRows ? .on : .off
        let refresh = NSMenuItem(
            title: "Refresh", action: #selector(RepositoryWindow.refreshRepository(_:)), keyEquivalent: "r")
        refresh.toolTip = "History・ブランチ・Working Copy を読み直す（ふだんは変化を検知して自動で読み直す）"
        viewMenu.addItem(refresh)
        let parent = NSMenuItem(title: "Go to Parent", action: #selector(RepositoryWindow.goToParent(_:)), keyEquivalent: String(UnicodeScalar(NSDownArrowFunctionKey)!))
        parent.keyEquivalentModifierMask = [.command, .option]
        viewMenu.addItem(parent)
        let child = NSMenuItem(title: "Go to Child", action: #selector(RepositoryWindow.goToChild(_:)), keyEquivalent: String(UnicodeScalar(NSUpArrowFunctionKey)!))
        child.keyEquivalentModifierMask = [.command, .option]
        viewMenu.addItem(child)
        viewMenu.addItem(.separator())
        viewMenu.addItem(compact)
        let ancestors = NSMenuItem(
            title: "Highlight Ancestors", action: #selector(toggleHighlightAncestors(_:)), keyEquivalent: "")
        ancestors.target = self
        ancestors.state = Settings.highlightAncestors ? .on : .off
        ancestors.toolTip = "選んだコミットの祖先と、それをつなぐ線だけを濃く描き、ほかを薄くする"
        viewMenu.addItem(ancestors)
        let mono = NSMenuItem(
            title: "Monospaced Commit Message", action: #selector(toggleMonospacedMessage(_:)), keyEquivalent: "")
        mono.target = self
        mono.state = Settings.monospacedCommitMessage ? .on : .off
        mono.toolTip = "コミットの本文を等幅の字体で出す（72 桁で改行した本文が崩れない）"
        viewMenu.addItem(mono)
        let avatars = NSMenuItem(title: "Show Avatars", action: #selector(toggleAvatars(_:)), keyEquivalent: "")
        avatars.target = self
        avatars.state = Settings.showAvatars ? .on : .off
        avatars.toolTip = "コミットの詳細に Gravatar のアバターを出す（メールアドレスのハッシュを gravatar.com に送る）"
        viewMenu.addItem(avatars)

        // テキスト欄でのコピーや取り消しのため、標準の Edit メニューを置く。
        let editItem = NSMenuItem()
        main.addItem(editItem)
        let editMenu = NSMenu(title: "Edit")
        editItem.submenu = editMenu
        editMenu.addItem(NSMenuItem(title: "Undo", action: Selector(("undo:")), keyEquivalent: "z"))
        editMenu.addItem(NSMenuItem(title: "Redo", action: Selector(("redo:")), keyEquivalent: "Z"))
        editMenu.addItem(.separator())
        editMenu.addItem(NSMenuItem(title: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x"))
        editMenu.addItem(NSMenuItem(title: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c"))
        editMenu.addItem(NSMenuItem(title: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v"))
        editMenu.addItem(NSMenuItem(title: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a"))
        editMenu.addItem(.separator())
        editMenu.addItem(NSMenuItem(title: "Find…", action: #selector(DiffDocumentView.showFindBar(_:)), keyEquivalent: "f"))
        editMenu.addItem(NSMenuItem(title: "Find Next", action: #selector(DiffDocumentView.findNext(_:)), keyEquivalent: "g"))
        editMenu.addItem(NSMenuItem(title: "Find Previous", action: #selector(DiffDocumentView.findPrevious(_:)), keyEquivalent: "G"))
        NSApp.mainMenu = main
    }

    @objc private func toggleAvatars(_ item: NSMenuItem) {
        Settings.showAvatars.toggle()
        item.state = Settings.showAvatars ? .on : .off
    }

    @objc private func toggleMonospacedMessage(_ item: NSMenuItem) {
        Settings.monospacedCommitMessage.toggle()
        item.state = Settings.monospacedCommitMessage ? .on : .off
    }

    @objc private func toggleHighlightAncestors(_ item: NSMenuItem) {
        Settings.highlightAncestors.toggle()
        item.state = Settings.highlightAncestors ? .on : .off
    }

    @objc private func toggleCompactRows(_ item: NSMenuItem) {
        Settings.compactHistoryRows.toggle()
        item.state = Settings.compactHistoryRows ? .on : .off
    }

    @objc private func showRepositories(_ sender: Any?) {
        RepositoryManagerWindowController.shared.show()
    }

    @objc private func toggleRemoteSignatureCheck(_ item: NSMenuItem) {
        Settings.checkRemoteSignaturesLocally.toggle()
        item.state = Settings.checkRemoteSignaturesLocally ? .on : .off
    }

    @objc private func toggleFSMonitor(_ item: NSMenuItem) {
        Settings.useFSMonitor.toggle()
        item.state = Settings.useFSMonitor ? .on : .off
    }

    @objc private func toggleWriteCommitGraph(_ item: NSMenuItem) {
        Settings.writeCommitGraph.toggle()
        item.state = Settings.writeCommitGraph ? .on : .off
    }

    /// リポジトリをすべて閉じても終わらない（リポジトリの一覧から開き直せる）。終えるのは ⌘Q。
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    static func option(_ name: String) -> String? {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
        return args[i + 1]
    }

    /// 引数で渡されたリポジトリ。
    private func repositoryPath() -> String? {
        var args = Array(CommandLine.arguments.dropFirst())
        for option in ["--snapshot", "--scroll-rows", "--find", "--select-row", "--merge-editor", "--search", "--blame", "--file-history", "--measure-select"] {
            if let i = args.firstIndex(of: option) { args.removeSubrange(i...min(i + 1, args.count - 1)) }
        }
        args.removeAll { ["--measure-scroll", "--select-first", "--working-copy", "--log-scroll", "--show-latency"].contains($0) }
        return args.first
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()

extension Data {
    /// 16 進数の文字列から。
    init?(hex: String) {
        var bytes = [UInt8]()
        var index = hex.startIndex
        while index < hex.endIndex {
            let next = hex.index(index, offsetBy: 2, limitedBy: hex.endIndex) ?? hex.endIndex
            guard let byte = UInt8(hex[index..<next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        self.init(bytes)
    }
}
