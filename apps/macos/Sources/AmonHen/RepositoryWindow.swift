import AmonhenKit
import AppKit

/// 1 つのリポジトリのウィンドウ。サイドバー・History・Working Copy・ツールバー・監視を持つ。
final class RepositoryWindow: NSObject, NSWindowDelegate {
    let url: URL
    let window: NSWindow
    /// ほかのリポジトリ（サブモジュールなど）を新しいウィンドウで開く。
    var onOpenRepository: ((String) -> Void)?
    var onClose: (() -> Void)?

    private let repository: Repository
    private let history: HistoryViewController
    private let sidebar: SidebarViewController
    private let workingCopy: WorkingCopyViewController
    private let detail: CommitDetailViewController
    private let actions: Actions
    private let toolbar: MainToolbar
    private var watch: RepositoryWatch?

    /// `debug` なら、起動時の引数（`--snapshot` など）の確認用の動きをする。
    init(url: URL, debug: Bool) throws {
        self.url = url
        let path = url.path
        let cacheDirectory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent(Bundle.main.bundleIdentifier ?? "io.github.fukusaka.amonhen")
        repository = try Repository(path: path, cacheDirectory: cacheDirectory)
        guard let renderer = GraphRenderer(), let text = TextRenderer(device: renderer.device) else {
            throw AmonhenError(description: "Metal を使えない")
        }
        history = HistoryViewController(repository: repository, renderer: renderer, text: text)
        sidebar = SidebarViewController()
        detail = CommitDetailViewController(repository: repository, renderer: text)
        workingCopy = WorkingCopyViewController(repository: repository, renderer: text)
        actions = Actions(repository: repository)
        toolbar = MainToolbar(actions: actions)
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1440, height: 820),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init()
        window.title = url.lastPathComponent
        window.representedURL = url
        window.isReleasedWhenClosed = false
        window.delegate = self
        assemble()
        window.setContentSize(NSSize(width: 1100, height: 720))
        window.center()
        window.makeKeyAndOrderFront(nil)
        start(debug: debug)
    }

    private func assemble() {
        let split = NSSplitViewController()
        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebar)
        sidebarItem.minimumThickness = 180
        sidebarItem.maximumThickness = 400
        split.addSplitViewItem(sidebarItem)
        // 左に History、右に選んだコミットの詳細（上から、メッセージとメタ情報、変更ファイル、差分）。
        let content = NSSplitViewController()
        content.splitView.isVertical = true
        let historyItem = NSSplitViewItem(viewController: history)
        historyItem.minimumThickness = 280
        // ウィンドウの大きさを変えたとき、一覧の幅は保ち、差分の側を伸び縮みさせる。
        historyItem.holdingPriority = .init(260)
        content.addSplitViewItem(historyItem)
        let detailItem = NSSplitViewItem(viewController: detail)
        detailItem.minimumThickness = 420
        content.addSplitViewItem(detailItem)
        let pages = PageViewController(pages: [.history: content, .workingCopy: workingCopy])
        split.addSplitViewItem(NSSplitViewItem(viewController: pages))
        window.contentViewController = split

        history.onSelectCommit = { [weak detail] commit in detail?.show(commit: commit) }
        // 詳細の親コミットのリンク。
        detail.onSelectCommit = { [weak history] hex in
            guard let history, let id = Data(hex: hex) else { return }
            if let row = history.currentHistory?.row(of: id) {
                history.select(row: row)
            } else {
                history.set(scope: .all, select: id)
            }
        }
        sidebar.onSelectPage = { [weak pages] page in pages?.show(page) }
        // ブランチ・タグを選ぶと、その履歴に絞り込む。stash は詳細にだけ中身を出す。
        sidebar.onSelect = { [weak history, weak detail] item in
            guard let history, let target = item.target else { return }
            if [.local, .remote, .tag].contains(item.kind) {
                history.set(scope: .ref(item.name, target), select: target)
            } else if item.kind == .stash {
                detail?.show(commit: target, stash: true)
            } else if let row = history.currentHistory?.row(of: target) {
                history.select(row: row)
            }
        }
        let root = repository.workdir ?? url.path
        sidebar.onOpenSubmodule = { [weak self] path in
            self?.onOpenRepository?(URL(fileURLWithPath: root).appendingPathComponent(path).path)
        }

        actions.window = window
        actions.onChange = { [weak self] in
            self?.history.refresh()
            self?.reloadRefs()
            self?.workingCopy.refresh()
        }
        actions.onUndoChange = { [weak toolbar] label in toolbar?.setUndo(label) }
        sidebar.actions = actions
        history.actions = actions
        workingCopy.actions = actions
        workingCopy.repositoryRoot = root
        window.toolbar = toolbar.toolbar
    }

    private func start(debug: Bool) {
        let window = self.window
        history.onLatency = { latency in
            window.subtitle = String(format: "入力→表示 %.1f ms", latency * 1000)
        }
        let repository = self.repository
        let started = Date()
        history.load { error in
            if let error { NSAlert(error: error).runModal() }
            NSLog("History を最新にするまで %.0f ms", Date().timeIntervalSince(started) * 1000)
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
        NSLog("History を %.0f ms で表示（キャッシュ）", Date().timeIntervalSince(started) * 1000)
        actions.refreshUndo()
        sidebar.select(page: debug && CommandLine.arguments.contains("--working-copy") ? .workingCopy : .history)
        workingCopy.refresh()
        reloadRefs()
        do {
            watch = try RepositoryWatch(repository: repository) { [weak self] changes in
                Task { @MainActor in self?.repositoryDidChange(changes) }
            }
        } catch {
            NSLog("リポジトリを監視できない: \(error)")
        }
        if debug { startDebugOptions() }
    }

    /// `--measure-scroll`・`--snapshot` など、確認用の起動オプション。
    private func startDebugOptions() {
        let history = self.history
        if CommandLine.arguments.contains("--measure-scroll") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                // --scroll-rows N で 1 フレームに進む行数を変える（勢いよくスクロールしたときの確認）。
                let rows = AppDelegate.option("--scroll-rows").flatMap(Double.init) ?? 1.5
                history.measureScroll(frames: 600, rowsPerFrame: rows) { summary in
                    print(summary)
                    NSApp.terminate(nil)
                }
            }
        }
        if CommandLine.arguments.contains("--log-scroll") { history.logScrollGestures() }
        if let output = AppDelegate.option("--snapshot") {
            let selectFirst = CommandLine.arguments.contains("--select-first")
            // --select-row N で、N 行目を選んで表の中ほどに出す（履歴の途中の見た目を確かめる）。
            let selectRow = AppDelegate.option("--select-row").flatMap(Int.init) ?? (selectFirst ? 0 : nil)
            let wait = selectRow != nil || CommandLine.arguments.contains("--working-copy")
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                if let selectRow { history.select(row: selectRow) }
            }
            if let query = AppDelegate.option("--find") {
                // 選んだコミットの差分を検索した状態（検索欄と強調）を確かめる。
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { [self] in detail.find(query) }
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + (wait ? 1.5 : 0.5)) { [self] in
                let rep = history.snapshot(of: window.contentView!, extra: detail.overlays() + workingCopy.overlays())
                try? rep?.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: output))
                NSApp.terminate(nil)
            }
        }
    }

    private func repositoryDidChange(_ changes: Changes) {
        if !changes.intersection([.refs, .config]).isEmpty {
            history.refresh()
            reloadRefs()
        }
        if !changes.intersection([.refs, .index, .worktree]).isEmpty {
            workingCopy.refresh()
        }
    }

    private func reloadRefs() {
        let repository = self.repository
        Task {
            do {
                let refs = try await Task.detached(priority: .userInitiated) { try repository.loadRefs() }.value
                sidebar.show(refs)
                history.head = refs.head
            } catch {
                NSLog("ref を読めない: \(error)")
            }
        }
    }

    func windowWillClose(_ notification: Notification) {
        watch = nil
        onClose?()
    }
}
