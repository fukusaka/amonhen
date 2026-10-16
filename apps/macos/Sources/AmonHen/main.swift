import AmonhenKit
import AppKit

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
        RepositoryManagerWindowController.shared.onOpen = { [weak self] path, newWindow in
            self?.open(path: path, inNewWindow: newWindow)
        }
        QuickOpenPanel.shared.onOpen = { [weak self] path, newWindow in
            self?.open(path: path, inNewWindow: newWindow)
        }
        ActivityWindowController.shared.model.listRepositories = { [weak self] in
            self?.windows.map(\.activityRepository) ?? []
        }
        // ブックマーク・スキャンの結果・行の状態が変わったら、一覧と Quick Open を描き直す。
        Bookmarks.shared.onChange = {
            RepositoryManagerWindowController.shared.bookmarksChanged()
            QuickOpenPanel.shared.refreshIfVisible()
        }
        Bookmarks.shared.watchScanRoots()
        // 引数でリポジトリを渡されたらそれを、なければ前回開いていたものを同じウィンドウ・タブの並びで開く。
        // どれもなければ一覧を出す。
        if let path = repositoryPath() {
            open(path: path, debug: true)
        } else {
            for group in RepositoryList.openWindows {
                for (i, path) in group.enumerated() where RepositoryList.isRepository(path) {
                    open(path: path, inNewWindow: i == 0)
                }
            }
            if windows.isEmpty, connecting.isEmpty { RepositoryManagerWindowController.shared.show() }
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
                if let id, let window = self.windows.first(where: { ObjectIdentifier($0.window) == id })?.window {
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

    func open(path: String, debug: Bool = false, inNewWindow: Bool = false) {
        let location = RepositoryLocation.isRemote(path) ? path : URL(fileURLWithPath: path).standardizedFileURL.path
        if let existing = windows.first(where: { $0.location == location }) {
            existing.window.makeKeyAndOrderFront(nil)
            return
        }
        if RepositoryLocation.isRemote(location) {
            connect(location: location, debug: debug, inNewWindow: inNewWindow)
            return
        }
        do {
            add(try RepositoryWindow(location: location, debug: debug, tabWith: inNewWindow ? nil : tabTarget))
        } catch {
            NSAlert(error: error).runModal()
            if windows.isEmpty, connecting.isEmpty { RepositoryManagerWindowController.shared.show() }
        }
    }

    /// つないでいる daemon 越しのリポジトリ（同じ場所を二重につながない）。
    private var connecting: [ConnectingWindow] = []

    /// 最後に前に出ていたリポジトリのウィンドウ（アプリが後ろにいるときに、外から開いたものを入れる先）。
    private weak var lastActiveWindow: NSWindow?

    /// 新しいタブを入れるウィンドウ: 前にあるリポジトリのウィンドウ。アプリが後ろにいれば（`amonhen` コマンドや
    /// Finder から開いたとき）、最後に使っていたウィンドウ。どれもなければ新しいウィンドウ。
    private var tabTarget: NSWindow? {
        let candidates = [NSApp.keyWindow, NSApp.mainWindow, lastActiveWindow].compactMap { $0 }
        return candidates.first { w in windows.contains { $0.window === w } } ?? windows.last?.window
    }

    /// daemon 越しのリポジトリに裏でつなぎ、つながったらウィンドウを開く。その間は接続中のウィンドウを出す。
    private func connect(location: String, debug: Bool, inNewWindow: Bool = false) {
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
                add(try RepositoryWindow(
                    location: location, repository: try result.get(), debug: debug, tabWith: inNewWindow ? nil : tabTarget))
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
            connect(location: window.location, debug: false, inNewWindow: window.window.tabbedWindows == nil)
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
        if Settings.autoBookmark { Bookmarks.shared.register(window.location) }
        RepositoryManagerWindowController.shared.close()
    }

    /// File > New Tab（⌘T）・タブバーの「+」: Quick Open で選んだリポジトリを、前のウィンドウのタブで開く。
    @objc func newTab(_ sender: Any?) {
        QuickOpenPanel.shared.show()
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

    /// File > New Window（⌘N）: Quick Open で選んだリポジトリを、新しいウィンドウで開く。
    @objc func newWindow(_ sender: Any?) {
        QuickOpenPanel.shared.show(newWindow: true)
    }

    @objc func openDocument(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.message = "リポジトリを選んでください"
        if panel.runModal() == .OK, let url = panel.url {
            open(path: url.path)
        }
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
