import AmonhenKit
import AppKit
import os

/// daemon 越しに開くときに起動するエンジン（`amonhen`。CLI と daemon を兼ねる）。アプリの中（Contents/Helpers）の
/// エンジンを起動時に置き場所（`~/.local/share/amonhen`）へ置き（`amonhen install`）、daemon はそのエンジンが選ぶもの
/// （`amonhen engine`。置き場所の current が同じ通信の形ならそれ）で起動する。どの FE が先に起動しても同じ daemon に
/// なる。開発中はこのビルドのチェックアウトの target/release のものを使い、置き場所には置かない。`AMONHEN_DAEMON` で
/// 差し替えられる。
enum Daemon {
    static var path: String {
        if let override = ProcessInfo.processInfo.environment["AMONHEN_DAEMON"] { return override }
        if let resolved = resolved.withLock({ $0 }) { return resolved }
        if let helper { return helper }
        return Engines.development ?? "amonhen"
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

    /// SSH 先に送り込むエンジン（`amonhen-<os>-<arch>`）の置き場所（アプリの Resources/engines、開発中はこのビルドの
    /// チェックアウトの dist/engine）。
    static var bundledDirectory: String { Engines.bundledDirectory }

    /// 開発中のリポジトリの中のファイル（`.app` でないとき）。
    static func developmentFile(_ relative: String) -> String? {
        var dir = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath().deletingLastPathComponent()
        while dir.path != "/" {
            let candidate = dir.appendingPathComponent(relative)
            if FileManager.default.fileExists(atPath: candidate.path) { return candidate.path }
            dir.deleteLastPathComponent()
        }
        return nil
    }
}

/// git と ssh が資格情報（HTTPS のユーザー名とパスワード、SSH の鍵のパスフレーズ）を尋ねる先。アプリは端末なしで
/// git を動かすので、尋ねられると失敗していた。ダイアログで尋ねる小さなスクリプト（`Contents/Resources/amonhen-askpass`、
/// 開発中は `apps/macos/Helpers`）を `AMONHEN_ASKPASS` で渡し、コアが git を動かすときに `GIT_ASKPASS`・`SSH_ASKPASS` に
/// する（共有の daemon にも引き継がれる）。
enum Askpass {
    static func install() {
        guard ProcessInfo.processInfo.environment["AMONHEN_ASKPASS"] == nil else { return }
        let helper = Bundle.main.resourceURL?.appendingPathComponent("amonhen-askpass").path ?? ""
        let path = FileManager.default.isExecutableFile(atPath: helper) ? helper : Daemon.developmentFile("apps/macos/Helpers/amonhen-askpass")
        if let path { setenv("AMONHEN_ASKPASS", path, 1) }
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

    /// まだ開けていないリポジトリのタブ（つないでいる途中・つなげなかった）。リポジトリのタブと同じく記録する。
    private var connecting: [ConnectingTab] = [] {
        didSet { saveOpenWindows() }
    }

    /// 起動時に前回のものを開き直している途中（終わってからまとめて記録する）。
    private var isRestoring = false

    /// 開いているリポジトリを、ウィンドウごとにタブの順で記録する（次の起動で同じ並びに開き直す）。
    /// 終了するときにウィンドウが閉じても、記録は消さない。
    private func saveOpenWindows() {
        guard !isTerminating, !isRestoring, !Self.isCheckRun else { return }
        let opened = windows.map { ($0.window, $0.location) } + connecting.map { ($0.window, $0.location) }
        var groups: [[String]] = []
        var seen = Set<ObjectIdentifier>()
        for (window, location) in opened where !seen.contains(ObjectIdentifier(window)) {
            let tabs = window.tabbedWindows ?? [window]
            for tab in tabs { seen.insert(ObjectIdentifier(tab)) }
            let locations = tabs.compactMap { tab in opened.first { $0.0 === tab }?.1 }
            groups.append(locations.isEmpty ? [location] : locations)
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
        } else if CommandLine.arguments.contains("--ssh-sheet") {
            // 見た目の確認用: 前回のリポジトリを開かず、Repositories の画面だけを出す。
            showHome()
        } else {
            // 1 つ前に開いたタブの隣に入れていく（つなぐのを待たずにタブを置くので、並びが崩れない）。
            isRestoring = true
            for group in RepositoryList.openWindows {
                var previous: NSWindow?
                for path in group where RepositoryList.isRepository(path) {
                    previous = open(path: path, inNewWindow: previous == nil, after: previous) ?? previous
                }
            }
            isRestoring = false
            saveOpenWindows()
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
                    ?? self.connecting.first(where: { ObjectIdentifier($0.window) == id })?.window
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
    /// 開いてから、それを閉じる（そのタブがそのリポジトリに替わる）。`after` を渡すと、そのタブの隣に開く。
    /// 開いた（つなぎ始めた）タブを返す。
    @discardableResult
    func open(path: String, debug: Bool = false, inNewWindow: Bool = false, replacing: NSWindow? = nil,
              after: NSWindow? = nil) -> NSWindow? {
        // 追加した worktree は、元のリポジトリのタブを開き、その worktree を書き込む先にする（別のリポジトリに
        // しない。docs/ux.md の「Worktrees」）。
        if !RepositoryLocation.isRemote(path), let main = RepositoryList.mainRepository(ofWorktree: path) {
            let opened = open(path: main, debug: debug, inNewWindow: inNewWindow, replacing: replacing, after: after)
            // `/private/tmp` と `/tmp` のような違いがあるので、リンクを解いて比べる。
            let key = WorktreeInfo.canonical(main)
            if let window = windows.first(where: { WorktreeInfo.canonical(RepositoryLocation.path($0.location)) == key }) {
                window.edit(worktree: path)
            }
            return opened
        }
        var location = RepositoryLocation.isRemote(path) ? path : URL(fileURLWithPath: path).standardizedFileURL.path
        // 共有の daemon 越しに開く設定なら、`daemon:` の場所にする（SSH 先も `daemon:ssh://…`）。
        if Settings.openThroughDaemon, !location.hasPrefix("daemon:") { location = "daemon:" + location }
        if let existing = windows.first(where: { $0.location == location })?.window
            ?? connecting.first(where: { $0.location == location })?.window {
            existing.makeKeyAndOrderFront(nil)
            return existing
        }
        let tabWith = inNewWindow ? nil : replacing ?? after ?? tabTarget
        if RepositoryLocation.isRemote(location) {
            return connect(location: location, debug: debug, tabWith: tabWith, replacing: replacing)
        }
        do {
            let window = try RepositoryWindow(location: location, debug: debug, tabWith: tabWith)
            add(window, replacing: replacing)
            return window.window
        } catch {
            // 起動時に開けなかったものは、エラーを出したタブを残す（閉じるまで次の起動でも開き直す）。
            if isRestoring {
                return connect(location: location, debug: debug, tabWith: tabWith, failed: error)
            }
            NSAlert(error: error).runModal()
            showHomeIfNothingOpen()
            return nil
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

    /// 最後に前に出ていたリポジトリのウィンドウ（アプリが後ろにいるときに、外から開いたものを入れる先）。
    private weak var lastActiveWindow: NSWindow?

    /// 新しいタブを入れるウィンドウ: 前にあるリポジトリのウィンドウ。アプリが後ろにいれば（`amonhen` コマンドや
    /// Finder から開いたとき）、最後に使っていたウィンドウ。どれもなければ新しいウィンドウ。
    private var tabTarget: NSWindow? {
        let candidates = [NSApp.keyWindow, NSApp.mainWindow, lastActiveWindow].compactMap { $0 }
        return candidates.first { w in
            windows.contains { $0.window === w } || connecting.contains { $0.window === w } || homes.contains { $0.window === w }
        } ?? windows.last?.window ?? connecting.last?.window ?? homes.last?.window
    }

    /// daemon 越し・SSH 先のリポジトリ: すぐにそのタブを `tabWith` の隣（なければ新しいウィンドウ）に置き、裏で
    /// つなぎ、つながったらリポジトリのタブに替える。`failed` を渡すと、つながずにそのエラーを出す。
    private func connect(location: String, debug: Bool, tabWith: NSWindow?, replacing: NSWindow? = nil,
                         failed: Error? = nil) -> NSWindow {
        let tab = ConnectingTab(location: location, tabWith: tabWith)
        tab.onRetry = { [weak self, weak tab] in
            if let self, let tab { start(tab, debug: debug) }
        }
        tab.onClose = { [weak self, weak tab] in
            self?.connecting.removeAll { $0 === tab }
            self?.showHomeIfNothingOpen()
        }
        connecting.append(tab)
        replacing?.close()
        if let failed { tab.showError(failed) } else { start(tab, debug: debug) }
        return tab.window
    }

    private func start(_ tab: ConnectingTab, debug: Bool) {
        tab.showConnecting()
        let location = tab.location, daemon = Daemon.path, daemons = Daemon.bundledDirectory
        Task {
            let result = await Task.detached(priority: .userInitiated) {
                Result { try RepositoryWindow.openRepository(location: location, daemon: daemon, daemons: daemons) }
            }.value
            // 閉じられていたら、つないだ結果を捨てる（Repository を手放すと daemon も終わる）。
            guard !tab.isClosed else { return }
            do {
                try replace(tab, with: result.get(), debug: debug)
            } catch {
                tab.showError(error)
            }
        }
    }

    /// つながったタブを、リポジトリのタブに替える。前に出すのは、そのタブが前にあったときだけ。
    private func replace(_ tab: ConnectingTab, with repository: Repository, debug: Bool) throws {
        let group = tab.window.tabGroup
        let selected = group?.selectedWindow === tab.window
        let key = tab.window.isKeyWindow || (!NSApp.isActive && lastActiveWindow === tab.window)
        let window = try RepositoryWindow(
            location: tab.location, repository: repository, debug: debug, tabWith: tab.window, front: key)
        if selected, !key { group?.selectedWindow = window.window }
        if lastActiveWindow === tab.window { lastActiveWindow = window.window }
        connecting.removeAll { $0 === tab }
        add(window)
        tab.finish()
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
            // 同じ場所につなぎ直すタブに置き換える（閉じたときにリポジトリのページを出さないよう、先に置く）。
            connect(location: window.location, debug: false, tabWith: window.window, replacing: window.window)
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

    /// Dock のアイコンをクリックしたとき: 見えているウィンドウがなければ、しまってあるウィンドウを戻し（AppKit に
    /// 任せる）、それもなければリポジトリのページを出す。
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag, !sender.windows.contains(where: \.isMiniaturized) { showHomeIfNothingOpen() }
        return true
    }

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
        args.removeAll { ["--measure-scroll", "--select-first", "--working-copy", "--stashes", "--reflog", "--log-scroll", "--show-latency", "--ssh-sheet"].contains($0) }
        return args.first
    }
}

// ターミナルの環境（PATH・SSH_AUTH_SOCK・GIT_* など）を取り込み、git が資格情報を尋ねる先（askpass）を決める
// （git・daemon を動かす前に）。
Settings.applyGitExecutable()
ShellEnvironment.import()
if let agent = ShellEnvironment.launchAgent { Repository.setSSHAgent(agent) }
Askpass.install()
// --print-environment: 取り込んだあとの環境（git に渡るもの）を出して終わる（Finder からの起動を確かめる用）。
if CommandLine.arguments.contains("--print-environment") {
    let env = ProcessInfo.processInfo.environment
    print("imported: \(ShellEnvironment.imported.joined(separator: " "))")
    for key in ["PATH", "SSH_AUTH_SOCK", "AMONHEN_ASKPASS"] + env.keys.filter({ $0.hasPrefix("GIT_") }).sorted() {
        print("\(key)=\(env[key] ?? "")")
    }
    exit(0)
}
// コアが作る文（エラーなど）も、画面と同じ言語にする（リポジトリを開く前に）。
CoreLanguage.set(L10n.language ?? "en")
HangWatch.startIfRequested()
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
