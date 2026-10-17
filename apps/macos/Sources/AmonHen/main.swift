import AmonhenKit
import AppKit
import os

/// daemon 越しに開くときに起動するエンジン（`amonhen`。CLI と daemon を兼ねる）。アプリの中（Contents/Helpers）の
/// エンジンを起動時に置き場所（`~/.local/share/amonhen`）へ置き（`amonhen install`）、daemon はそのエンジンが選ぶもの
/// （`amonhen engine`。置き場所の current が同じ通信の形ならそれ）で起動する。どの FE が先に起動しても同じ daemon に
/// なる。開発中はリポジトリの target/release のものを使い、置き場所には置かない。`AMONHEN_DAEMON` で差し替えられる。
enum Daemon {
    static var path: String {
        if let override = ProcessInfo.processInfo.environment["AMONHEN_DAEMON"] { return override }
        if let resolved = resolved.withLock({ $0 }) { return resolved }
        if let helper { return helper }
        return developmentFile("target/release/amonhen") ?? "amonhen"
    }

    /// アプリの中のエンジン（.app でなければ nil）。
    private static var helper: String? {
        let helper = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/amonhen").path
        return FileManager.default.isExecutableFile(atPath: helper) ? helper : nil
    }

    /// `amonhen engine` が選んだもの。
    private static let resolved = OSAllocatedUnfairLock<String?>(initialState: nil)

    /// アプリの中のエンジンを置き場所に置き、daemon に使うエンジンを決める（起動時に裏で）。
    static func prepare() {
        guard ProcessInfo.processInfo.environment["AMONHEN_DAEMON"] == nil, let helper else { return }
        DispatchQueue.global(qos: .utility).async {
            _ = run(helper, ["install", "--quiet"])
            if let engine = run(helper, ["engine"])?.trimmingCharacters(in: .whitespacesAndNewlines), !engine.isEmpty {
                resolved.withLock { $0 = engine }
            }
        }
    }

    private static func run(_ program: String, _ arguments: [String]) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: program)
        process.arguments = arguments
        let out = Pipe()
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return nil }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return process.terminationStatus == 0 ? String(decoding: data, as: UTF8.self) : nil
    }

    /// SSH 先に送り込むエンジン（`amonhen-<os>-<arch>`）の置き場所（アプリの Resources/engines、開発中は dist/engine）。
    static var bundledDirectory: String {
        if let resources = Bundle.main.resourceURL?.appendingPathComponent("engines"),
           FileManager.default.fileExists(atPath: resources.path) {
            return resources.path
        }
        return developmentFile("dist/engine") ?? ""
    }

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
    private var pages: [SidebarViewController.Page: NSViewController]
    private var current: NSViewController?
    /// 表示している画面。
    private(set) var page: SidebarViewController.Page?

    init(pages: [SidebarViewController.Page: NSViewController]) {
        self.pages = pages
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: 900, height: 700))
    }

    /// `page` の画面を差し替える（書き込む先の worktree を変えたときの Working Copy）。表示中なら出し直す。
    func replace(_ page: SidebarViewController.Page, with controller: NSViewController) {
        let showing = self.page == page
        pages[page] = controller
        if showing {
            self.page = nil
            show(page)
        }
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
        self.page = page
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    /// 開いているリポジトリのウィンドウ（タブを含む）。
    private var windows: [RepositoryWindow] = [] {
        didSet { saveOpenWindows() }
    }

    /// 開いているリポジトリを、ウィンドウごとにタブの順で記録する（次の起動で同じ並びに開き直す）。
    /// 終了するときにウィンドウが閉じても、記録は消さない。
    private func saveOpenWindows() {
        guard !isTerminating, !Self.isCheckRun else { return }
        var groups: [[String]] = []
        var seen = Set<ObjectIdentifier>()
        for w in windows where !seen.contains(ObjectIdentifier(w.window)) {
            let tabs = (w.window.tabbedWindows ?? [w.window]).compactMap { tab in windows.first { $0.window === tab } }
            for tab in tabs { seen.insert(ObjectIdentifier(tab.window)) }
            groups.append((tabs.isEmpty ? [w] : tabs).map(\.location))
        }
        RepositoryList.openWindows = groups
    }

    private var isTerminating = false
    /// 確認用の起動（画面を撮る・計測する）。開いていたリポジトリの記録を書き換えない。
    static let isCheckRun = ["--snapshot", "--measure-scroll", "--log-scroll", "--measure-select"].contains { CommandLine.arguments.contains($0) }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // タブの並べ替えやウィンドウへの切り出しは知らせがないので、終わる前に記録し直す。
        saveOpenWindows()
        isTerminating = true
        return .terminateNow
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        buildMenu()
        // swift build のまま起動したとき（.app でないのでアイコンがない）は、ソースの横のアイコンを Dock に出す。
        if Bundle.main.object(forInfoDictionaryKey: "CFBundleIconFile") == nil {
            let icon = URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent().appendingPathComponent("../../Icons/AppIcon.icns").standardizedFileURL
            if let image = NSImage(contentsOf: icon) { NSApp.applicationIconImage = image }
        }
        Repository.setFSMonitor(Settings.useFSMonitor)
        Daemon.prepare()
        QuickOpenPanel.shared.onOpen = { [weak self] path, newWindow in
            self?.open(path: path, inNewWindow: newWindow)
        }
        ActivityWindowController.shared.model.listRepositories = { [weak self] in
            self?.windows.map(\.activityRepository) ?? []
        }
        // ブックマーク・スキャンの結果・行の状態が変わったら、一覧（ページ・ツールバー）と Quick Open を描き直す。
        Bookmarks.shared.onChange = {
            RepositoryBrowser.bookmarksChanged()
            QuickOpenPanel.shared.refreshIfVisible()
        }
        Bookmarks.shared.onRowChange = { path in
            RepositoryBrowser.rowChanged(path)
            QuickOpenPanel.shared.refreshIfVisible()
        }
        Bookmarks.shared.watchScanRoots()
        // 引数でリポジトリを渡されたらそれを、なければ前回開いていたものを同じウィンドウ・タブの並びで開く。
        // どれもなければリポジトリのページを出す。
        if let path = repositoryPath() {
            open(path: path, debug: true)
        } else {
            for group in RepositoryList.openWindows {
                for (i, path) in group.enumerated() where RepositoryList.isRepository(path) {
                    open(path: path, inNewWindow: i == 0)
                }
            }
            showHomeIfNothingOpen()
        }
        // 起動の途中で受け取った「開く」（`amonhen` コマンド・Finder から）を、ここで開く。
        isLaunched = true
        for location in pendingOpen { open(path: location) }
        pendingOpen.removeAll()
        // ウィンドウを前に出すたびに記録し直す（タブの並べ替え・切り出しを拾う）。
        NotificationCenter.default.addObserver(
            forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main
        ) { [weak self] note in
            // 通知はスレッドをまたいで渡せないので、どのウィンドウかは ID で持ち込む。
            let id = (note.object as AnyObject?).map(ObjectIdentifier.init)
            MainActor.assumeIsolated {
                guard let self else { return }
                if let id, let window = self.windows.first(where: { ObjectIdentifier($0.window) == id })?.window
                    ?? self.homes.first(where: { ObjectIdentifier($0.window) == id })?.window {
                    self.lastActiveWindow = window
                }
                self.saveOpenWindows()
            }
        }
        NSApp.activate()
    }

    /// リポジトリを開く。ふつうは前にあるリポジトリのウィンドウのタブに入れ、`inNewWindow` なら新しいウィンドウに
    /// する。すでに開いていれば前に出す。`path` は手元のパスか、daemon 越しの場所（`daemon:/path`・`ssh://host/path`）。
    private var isLaunched = false
    private var pendingOpen: [String] = []

    /// `amonhen [path]`・`amonhen open ssh://…` や Finder から渡されたリポジトリ。フォルダはそのまま、SSH 先などは
    /// `amonhen://open?location=…` の URL で受け取る。
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            let location: String?
            if url.isFileURL {
                location = url.path
            } else if url.scheme == "amonhen", url.host == "open",
                      let item = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first(where: { $0.name == "location" }) {
                location = item.value
            } else {
                location = nil
            }
            guard let location else { continue }
            if isLaunched {
                open(path: location)
                NSApp.activate()
            } else {
                pendingOpen.append(location)
            }
        }
    }

    /// `replacing` を渡すと、そのタブ（リポジトリのページ、ツールバーの一覧から切り替える前のリポジトリ）の隣に
    /// 開いてから、それを閉じる（そのタブがそのリポジトリに替わる）。
    func open(path: String, debug: Bool = false, inNewWindow: Bool = false, replacing: NSWindow? = nil) {
        // 追加した worktree は、元のリポジトリのタブを開き、その worktree を書き込む先にする（別のリポジトリに
        // しない。docs/ux.md の「Worktrees」）。
        if !RepositoryLocation.isRemote(path), let main = RepositoryList.mainRepository(ofWorktree: path) {
            open(path: main, debug: debug, inNewWindow: inNewWindow, replacing: replacing)
            // `/private/tmp` と `/tmp` のような違いがあるので、リンクを解いて比べる。
            let key = WorktreeInfo.canonical(main)
            if let window = windows.first(where: { WorktreeInfo.canonical(RepositoryLocation.path($0.location)) == key }) {
                window.edit(worktree: path)
            }
            return
        }
        var location = RepositoryLocation.isRemote(path) ? path : URL(fileURLWithPath: path).standardizedFileURL.path
        // 共有の daemon 越しに開く設定なら、`daemon:` の場所にする（SSH 先も `daemon:ssh://…`）。
        if Settings.openThroughDaemon, !location.hasPrefix("daemon:") { location = "daemon:" + location }
        if let existing = windows.first(where: { $0.location == location }) {
            existing.window.makeKeyAndOrderFront(nil)
            return
        }
        if RepositoryLocation.isRemote(location) {
            connect(location: location, debug: debug, inNewWindow: inNewWindow, replacing: replacing)
            return
        }
        do {
            add(try RepositoryWindow(location: location, debug: debug, tabWith: inNewWindow ? nil : replacing ?? tabTarget),
                replacing: replacing)
        } catch {
            NSAlert(error: error).runModal()
            showHomeIfNothingOpen()
        }
    }

    // MARK: - リポジトリのページ

    /// リポジトリのページ（リポジトリを開いていないタブ）。
    private var homes: [RepositoryHomeWindow] = []

    /// リポジトリのページを出す。前のウィンドウのタブの並びにもうあれば、それを前に出す。`newWindow` なら
    /// 新しいウィンドウに出す。
    func showHome(newWindow: Bool = false) {
        if !newWindow, let target = tabTarget {
            let group = target.tabbedWindows ?? [target]
            if let home = homes.first(where: { h in group.contains { $0 === h.window } }) {
                home.show()
                return
            }
        }
        let home = RepositoryHomeWindow(tabWith: newWindow ? nil : tabTarget)
        home.browser.onOpen = { [weak self, weak home] path, mode in
            self?.open(path: path, inNewWindow: mode == .newWindow, replacing: mode == .here ? home?.window : nil)
        }
        home.onClose = { [weak self, weak home] in
            self?.homes.removeAll { $0 === home }
        }
        homes.append(home)
    }

    /// 何も開いていなければ（リポジトリもページも、つないでいる途中のものもない）、ページを出す。
    private func showHomeIfNothingOpen() {
        if windows.isEmpty, homes.isEmpty, connecting.isEmpty { showHome(newWindow: true) }
    }

    /// つないでいる daemon 越しのリポジトリ（同じ場所を二重につながない）。
    private var connecting: [ConnectingWindow] = []

    /// 最後に前に出ていたリポジトリのウィンドウ（アプリが後ろにいるときに、外から開いたものを入れる先）。
    private weak var lastActiveWindow: NSWindow?

    /// 新しいタブを入れるウィンドウ: 前にあるリポジトリのウィンドウ。アプリが後ろにいれば（`amonhen` コマンドや
    /// Finder から開いたとき）、最後に使っていたウィンドウ。どれもなければ新しいウィンドウ。
    private var tabTarget: NSWindow? {
        let candidates = [NSApp.keyWindow, NSApp.mainWindow, lastActiveWindow].compactMap { $0 }
        return candidates.first { w in windows.contains { $0.window === w } || homes.contains { $0.window === w } }
            ?? windows.last?.window ?? homes.last?.window
    }

    /// daemon 越しのリポジトリに裏でつなぎ、つながったらウィンドウを開く。その間は接続中のウィンドウを出す。
    private func connect(location: String, debug: Bool, inNewWindow: Bool = false, replacing: NSWindow? = nil) {
        if let existing = connecting.first(where: { $0.location == location }) {
            existing.window.makeKeyAndOrderFront(nil)
            return
        }
        let progress = ConnectingWindow(location: location)
        progress.onCancel = { [weak self, weak progress] in
            self?.connecting.removeAll { $0 === progress }
            self?.showHomeIfNothingOpen()
        }
        connecting.append(progress)
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
                add(try RepositoryWindow(
                    location: location, repository: try result.get(), debug: debug,
                    tabWith: inNewWindow ? nil : replacing ?? tabTarget), replacing: replacing)
            } catch {
                let alert = NSAlert(error: error)
                alert.messageText = L("Couldn’t connect to %@", RepositoryLocation.host(location) ?? location)
                alert.informativeText = String(describing: error)
                alert.runModal()
                showHomeIfNothingOpen()
            }
        }
    }

    private func add(_ window: RepositoryWindow, replacing: NSWindow? = nil) {
        window.onOpenRepository = { [weak self] path in self?.open(path: path) }
        // ツールバーのリポジトリ名から開く一覧: 選んだものにこのタブを切り替える（⌘ なら新しいウィンドウ）。
        window.onSwitchRepository = { [weak self, weak window] path, mode in
            self?.open(path: path, inNewWindow: mode == .newWindow, replacing: mode == .here ? window?.window : nil)
        }
        window.onShowRepositories = { [weak self] in self?.showHome() }
        window.onReconnect = { [weak self, weak window] in
            guard let self, let window else { return }
            // 先につなぎ始めてから閉じる（閉じたときにリポジトリの一覧を出さないため）。
            connect(location: window.location, debug: false, inNewWindow: window.window.tabbedWindows == nil)
            window.window.close()
        }
        window.onClose = { [weak self, weak window] in
            self?.windows.removeAll { $0 === window }
            // 最後のリポジトリを閉じたら、終わらずにリポジトリのページを出す。
            self?.showHomeIfNothingOpen()
        }
        windows.append(window)
        RepositoryList.touch(window.location)
        if Settings.autoBookmark { Bookmarks.shared.register(window.location) }
        // 開いたタブで、元のタブ（ページ・切り替える前のリポジトリ）を置き換える。
        replacing?.close()
    }

    /// File > New Tab（⌘T）・タブバーの「+」: 前のウィンドウに、リポジトリのページのタブを出す。
    @objc func newTab(_ sender: Any?) {
        showHome()
    }

    /// File > Clone…（⌃⌘C）: clone して、前のウィンドウのタブで開く（ブックマークにも入れる）。
    @objc func cloneRepository(_ sender: Any?) {
        CloneSheet.show { [weak self] path in
            Bookmarks.shared.add(path)
            self?.open(path: path)
        }
    }

    /// File > Quick Open（⌘⇧O）。
    @objc func quickOpen(_ sender: Any?) {
        QuickOpenPanel.shared.show()
    }

    /// タブバーの「+」（AppKit がこの名前で探す）。
    @objc func newWindowForTab(_ sender: Any?) { newTab(sender) }

    /// File > New Window（⌘N）: 新しいウィンドウに、リポジトリのページを出す。
    @objc func newWindow(_ sender: Any?) {
        showHome(newWindow: true)
    }

    /// File > Open…（⌘O）: リポジトリのページを出す（一覧から選ぶ。一覧にないフォルダは、ページの Add から加える）。
    @objc func openDocument(_ sender: Any?) {
        showHome()
    }

    /// リポジトリをすべて閉じても終わらない（リポジトリのページから開き直せる）。終えるのは ⌘Q。
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

// コアが作る文（エラーなど）も、画面と同じ言語にする（リポジトリを開く前に）。
CoreLanguage.set(L10n.language ?? "en")
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
