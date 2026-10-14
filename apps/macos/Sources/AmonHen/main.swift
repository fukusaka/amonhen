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

    init(actions: Actions) {
        self.actions = actions
        super.init()
        toolbar.delegate = self
        toolbar.displayMode = .iconAndLabel
        setUndo(nil)
    }

    func setUndo(_ label: String?) {
        undoItem.isEnabled = label != nil
        undoItem.toolTip = label.map { "Undo \($0)" } ?? "Undo できる操作はありません"
        undoItem.label = label.map { "Undo \($0)" } ?? "Undo"
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
        case "push": (item, symbol, action) = (NSToolbarItem(itemIdentifier: id), "arrow.up.circle", #selector(push))
        case "pull": (item, symbol, action) = (NSToolbarItem(itemIdentifier: id), "arrow.down.to.line.circle", #selector(pull))
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
    private var window: NSWindow?
    private var history: HistoryViewController?
    private var sidebar: SidebarViewController?
    private var repository: Repository?
    private var watch: RepositoryWatch?
    private var workingCopy: WorkingCopyViewController?
    private var actions: Actions?
    private var toolbar: MainToolbar?

    func applicationDidFinishLaunching(_ notification: Notification) {
        buildMenu()
        guard let path = repositoryPath() else {
            NSApp.terminate(nil)
            return
        }
        do {
            let cacheDirectory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                .appendingPathComponent(Bundle.main.bundleIdentifier ?? "io.github.fukusaka.amonhen")
            let repository = try Repository(path: path, cacheDirectory: cacheDirectory)
            guard let renderer = GraphRenderer() else { throw AmonhenError(description: "Metal を使えない") }
            let controller = HistoryViewController(repository: repository, renderer: renderer)
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 1100, height: 720),
                styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
            let name = URL(fileURLWithPath: path).lastPathComponent
            window.title = name
            let sidebar = SidebarViewController()
            let split = NSSplitViewController()
            let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebar)
            sidebarItem.minimumThickness = 180
            sidebarItem.maximumThickness = 400
            split.addSplitViewItem(sidebarItem)
            // 上に History、下に選んだコミットの詳細。
            guard let text = TextRenderer(device: renderer.device) else { throw AmonhenError(description: "Metal を使えない") }
            let detail = CommitDetailViewController(repository: repository, renderer: text)
            let content = NSSplitViewController()
            content.splitView.isVertical = false
            content.addSplitViewItem(NSSplitViewItem(viewController: controller))
            let detailItem = NSSplitViewItem(viewController: detail)
            detailItem.minimumThickness = 160
            content.addSplitViewItem(detailItem)
            controller.onSelectCommit = { [weak detail] commit in detail?.show(commit: commit) }
            // 詳細の親コミットのリンク。
            detail.onSelectCommit = { [weak controller] hex in
                guard let controller, let id = Data(hex: hex) else { return }
                if let row = controller.currentHistory?.row(of: id) {
                    controller.select(row: row)
                } else {
                    controller.set(scope: .all, select: id)
                }
            }
            let workingCopy = WorkingCopyViewController(repository: repository, renderer: text)
            let pages = PageViewController(pages: [.history: content, .workingCopy: workingCopy])
            sidebar.onSelectPage = { [weak pages] page in pages?.show(page) }
            split.addSplitViewItem(NSSplitViewItem(viewController: pages))
            // ブランチ・タグを選ぶと、その履歴に絞り込む。stash などはそのコミットを選ぶだけ。
            sidebar.onSelect = { [weak controller] item in
                guard let controller, let target = item.target else { return }
                if [.local, .remote, .tag].contains(item.kind) {
                    controller.set(scope: .ref(item.name, target), select: target)
                } else if let row = controller.currentHistory?.row(of: target) {
                    controller.select(row: row)
                }
            }
            window.contentViewController = split
            window.setContentSize(NSSize(width: 1100, height: 720))
            window.center()
            window.makeKeyAndOrderFront(nil)
            controller.onLatency = { latency in
                window.subtitle = String(format: "入力→表示 %.1f ms", latency * 1000)
            }
            let start = Date()
            controller.load { error in
                if let error { NSAlert(error: error).runModal() }
                NSLog("History を最新にするまで %.0f ms", Date().timeIntervalSince(start) * 1000)
                if error == nil, Settings.writeCommitGraph {
                    Task.detached(priority: .utility) {
                        do {
                            if try repository.writeCommitGraphIfMissing() { NSLog("commit-graph を書き出した") }
                        } catch {
                            NSLog("commit-graph を書き出せない: \(error)")
                        }
                    }
                }
            }
            NSLog("History を %.0f ms で表示（キャッシュ）", Date().timeIntervalSince(start) * 1000)
            self.window = window
            self.history = controller
            self.sidebar = sidebar
            self.repository = repository
            self.workingCopy = workingCopy
            let actions = Actions(repository: repository)
            actions.window = window
            actions.onChange = { [weak self] in
                self?.history?.refresh()
                self?.reloadRefs()
                self?.workingCopy?.refresh()
            }
            sidebar.actions = actions
            controller.actions = actions
            workingCopy.actions = actions
            workingCopy.repositoryRoot = repository.workdir ?? path
            let toolbar = MainToolbar(actions: actions)
            window.toolbar = toolbar.toolbar
            actions.onUndoChange = { [weak toolbar] label in toolbar?.setUndo(label) }
            actions.refreshUndo()
            self.actions = actions
            self.toolbar = toolbar
            sidebar.select(page: CommandLine.arguments.contains("--working-copy") ? .workingCopy : .history)
            workingCopy.refresh()
            reloadRefs()
            do {
                watch = try RepositoryWatch(repository: repository) { [weak self] changes in
                    Task { @MainActor in self?.repositoryDidChange(changes) }
                }
            } catch {
                NSLog("リポジトリを監視できない: \(error)")
            }
            if CommandLine.arguments.contains("--measure-scroll") {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    controller.measureScroll(frames: 600, rowsPerFrame: 1.5) { summary in
                        print(summary)
                        NSApp.terminate(nil)
                    }
                }
            }
            if let output = Self.option("--snapshot") {
                let selectFirst = CommandLine.arguments.contains("--select-first")
                    || CommandLine.arguments.contains("--working-copy")
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    if CommandLine.arguments.contains("--select-first") { controller.select(row: 0) }
                }
                DispatchQueue.main.asyncAfter(deadline: .now() + (selectFirst ? 1.5 : 0.5)) {
                    let rep = controller.snapshot(of: window.contentView!, extra: detail.overlays() + workingCopy.overlays())
                    try? rep?.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: output))
                    NSApp.terminate(nil)
                }
            }
        } catch {
            let alert = NSAlert(error: error)
            alert.runModal()
            NSApp.terminate(nil)
        }
        NSApp.activate()
    }

    private func repositoryDidChange(_ changes: Changes) {
        if !changes.intersection([.refs, .config]).isEmpty {
            history?.refresh()
            reloadRefs()
        }
        if !changes.intersection([.refs, .index, .worktree]).isEmpty {
            workingCopy?.refresh()
        }
    }

    private func reloadRefs() {
        guard let repository else { return }
        Task {
            do {
                let refs = try await Task.detached(priority: .userInitiated) { try repository.loadRefs() }.value
                sidebar?.show(refs)
                history?.head = refs.head
            } catch {
                NSLog("ref を読めない: \(error)")
            }
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
        commitGraph.toolTip = "commit-graph がないリポジトリを開いたとき、裏で git commit-graph write --reachable を実行する（.git に書き込む）"
        appMenu.addItem(commitGraph)
        appMenu.addItem(.separator())
        appMenu.addItem(NSMenuItem(title: "Amon Hen を終了", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
        NSApp.mainMenu = main
    }

    @objc private func toggleWriteCommitGraph(_ item: NSMenuItem) {
        Settings.writeCommitGraph.toggle()
        item.state = Settings.writeCommitGraph ? .on : .off
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    private static func option(_ name: String) -> String? {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
        return args[i + 1]
    }

    /// 引数で渡されたリポジトリ。なければ選ばせる。
    private func repositoryPath() -> String? {
        var args = Array(CommandLine.arguments.dropFirst())
        if let i = args.firstIndex(of: "--snapshot") { args.removeSubrange(i...min(i + 1, args.count - 1)) }
        args.removeAll { ["--measure-scroll", "--select-first", "--working-copy"].contains($0) }
        if let path = args.first { return path }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.message = "リポジトリを選んでください"
        return panel.runModal() == .OK ? panel.url?.path : nil
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
