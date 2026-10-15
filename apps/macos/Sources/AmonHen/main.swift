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

/// Working Copy と History の詳細で共通の、左の列（ファイル）の最初の幅。2 つの画面の並びをそろえる。
let fileColumnWidth: CGFloat = 280

extension NSSplitViewController {
    /// 最初に幅が決まったときに 1 回だけ、左の列を `fileColumnWidth` にする（分割は最初、中のビューの
    /// 幅の比で配分されるので、画面ごとにずれる）。
    func placeFileColumnOnce(_ placed: inout Bool) {
        guard !placed, splitView.bounds.width > fileColumnWidth * 2 else { return }
        placed = true
        splitView.setPosition(fileColumnWidth, ofDividerAt: 0)
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
    /// 開いているリポジトリのウィンドウ。
    private var windows: [RepositoryWindow] = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        buildMenu()
        guard let path = repositoryPath() else {
            NSApp.terminate(nil)
            return
        }
        open(path: path, debug: true)
        NSApp.activate()
    }

    /// リポジトリを新しいウィンドウで開く。すでに開いていれば前に出す。
    func open(path: String, debug: Bool = false) {
        let url = URL(fileURLWithPath: path).standardizedFileURL
        if let existing = windows.first(where: { $0.url == url }) {
            existing.window.makeKeyAndOrderFront(nil)
            return
        }
        do {
            let window = try RepositoryWindow(url: url, debug: debug)
            window.onOpenRepository = { [weak self] path in self?.open(path: path) }
            window.onClose = { [weak self, weak window] in self?.windows.removeAll { $0 === window } }
            windows.append(window)
        } catch {
            NSAlert(error: error).runModal()
            if windows.isEmpty { NSApp.terminate(nil) }
        }
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
        commitGraph.toolTip = "commit-graph がないリポジトリを開いたとき、裏で git commit-graph write --reachable を実行する（.git に書き込む）"
        appMenu.addItem(commitGraph)
        appMenu.addItem(.separator())
        appMenu.addItem(NSMenuItem(title: "Amon Hen を終了", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))

        let fileItem = NSMenuItem()
        main.addItem(fileItem)
        let fileMenu = NSMenu(title: "File")
        fileItem.submenu = fileMenu
        let open = NSMenuItem(title: "Open…", action: #selector(openDocument(_:)), keyEquivalent: "o")
        open.target = self
        fileMenu.addItem(open)
        fileMenu.addItem(NSMenuItem(title: "Close Window", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w"))

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

    @objc private func toggleWriteCommitGraph(_ item: NSMenuItem) {
        Settings.writeCommitGraph.toggle()
        item.state = Settings.writeCommitGraph ? .on : .off
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    static func option(_ name: String) -> String? {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
        return args[i + 1]
    }

    /// 引数で渡されたリポジトリ。なければ選ばせる。
    private func repositoryPath() -> String? {
        var args = Array(CommandLine.arguments.dropFirst())
        for option in ["--snapshot", "--scroll-rows", "--find", "--select-row"] {
            if let i = args.firstIndex(of: option) { args.removeSubrange(i...min(i + 1, args.count - 1)) }
        }
        args.removeAll { ["--measure-scroll", "--select-first", "--working-copy", "--log-scroll"].contains($0) }
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
