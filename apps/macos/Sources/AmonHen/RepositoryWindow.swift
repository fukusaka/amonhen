import AmonhenKit
import AppKit

/// 1 つのリポジトリのウィンドウ。サイドバー・History・Working Copy・ツールバー・監視を持つ。
final class RepositoryWindow: NSObject, NSWindowDelegate {
    /// 手元のパスか、daemon 越しの場所（`daemon:/path`・`ssh://host/path`）。
    let location: String
    /// 差分の文字を描く（ファイルの履歴のウィンドウでも使う）。
    private let textRenderer: TextRenderer
    private var pages: PageViewController?
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
    init(location: String, debug: Bool) throws {
        self.location = location
        let cacheDirectory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent(Bundle.main.bundleIdentifier ?? "io.github.fukusaka.amonhen")
        repository = if RepositoryLocation.isRemote(location) {
            try Repository(location: location, daemon: Daemon.path, daemons: Daemon.bundledDirectory, cacheDirectory: cacheDirectory)
        } else {
            try Repository(path: location, cacheDirectory: cacheDirectory)
        }
        guard let renderer = GraphRenderer(), let text = TextRenderer(device: renderer.device) else {
            throw AmonhenError(description: "Metal を使えない")
        }
        textRenderer = text
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
        let path = RepositoryLocation.path(location)
        window.title = (path as NSString).lastPathComponent
        if let host = RepositoryLocation.host(location) {
            // daemon 越しなら、タイトルに接続先を添える（手元のリポジトリと見分ける）。
            window.title += " — \(host)"
        } else {
            window.representedURL = URL(fileURLWithPath: path)
        }
        window.isReleasedWhenClosed = false
        window.delegate = self
        assemble()
        // リポジトリごとに、前回の位置と大きさで開く（初めてなら真ん中に）。
        let frameName = "Repository \(location)"
        window.setContentSize(NSSize(width: 1440, height: 820))
        if !window.setFrameUsingName(frameName) { window.center() }
        window.setFrameAutosaveName(frameName)
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
        self.pages = pages
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
        detail.onBlame = { [weak self] path, commit in self?.openBlame(path: path, rev: commit) }
        detail.onFileHistory = { [weak self] path in self?.openFileHistory(path: path) }
        workingCopy.onBlame = { [weak self] path in self?.openBlame(path: path, rev: nil) }
        workingCopy.onFileHistory = { [weak self] path in self?.openFileHistory(path: path) }
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
        let root = repository.workdir ?? RepositoryLocation.path(location)
        let remote = RepositoryLocation.isRemote(location)
        sidebar.onOpenSubmodule = { [weak self] path in
            // サブモジュールは、同じ接続先の上の相対パスとして開く。
            let full = (root as NSString).appendingPathComponent(path)
            self?.onOpenRepository?(remote ? Self.replacingPath(of: self?.location ?? "", with: full) : full)
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
        window.toolbar = toolbar.toolbar
    }

    private func start(debug: Bool) {
        let window = self.window
        // --show-latency で、入力から表示までの時間をタイトルの下に出す（ふだんは今のブランチ）。
        if CommandLine.arguments.contains("--show-latency") {
            history.onLatency = { latency in
                window.subtitle = String(format: "入力→表示 %.1f ms", latency * 1000)
            }
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
        // --blame PATH・--file-history PATH で、そのファイルの blame・履歴を開く（見た目の確認用）。
        if let path = AppDelegate.option("--blame") { openBlame(path: path, rev: nil) }
        if let path = AppDelegate.option("--file-history") { openFileHistory(path: path) }
        // --measure-select N で、History の行を N 個順に選び、詳細が出るまでの各段階の時間を測る。
        if let count = AppDelegate.option("--measure-select").flatMap(Int.init) {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [self] in measureSelection(count: count) }
        }
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
            // --search QUERY で、History を検索した状態を撮る（すべてのコミットを読むので長めに待つ）。
            let query = AppDelegate.option("--search")
            if let query {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { history.search(query) }
            }
            // --merge-editor PATH で、そのファイルのマージエディタを開いて撮る。
            let mergePath = AppDelegate.option("--merge-editor")
            if let mergePath {
                MergeEditorWindowController.show(path: mergePath, repository: repository, actions: actions)
            }
            let delay = query != nil ? 3.5 : wait || mergePath != nil ? 1.5 : 0.5
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [self] in
                let rep = mergePath != nil
                    ? MergeEditorWindowController.snapshot()
                    : history.snapshot(of: window.contentView!, extra: detail.overlays() + workingCopy.overlays())
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
                if !CommandLine.arguments.contains("--show-latency") {
                    window.subtitle = refs.headBranch ?? "detached HEAD"
                }
                history.head = refs.head
                history.show(refs: refs)
                let current = refs.items.first { $0.kind == .local && $0.name == refs.headBranch }
                toolbar.setTracking(ahead: current?.ahead ?? 0, behind: current?.behind ?? 0)
            } catch {
                NSLog("ref を読めない: \(error)")
            }
        }
    }

    private func measureSelection(count: Int) {
        let total = history.currentHistory?.rowCount ?? 0
        guard total > 0 else { return }
        FrameProfile.reset()
        FrameProfile.enabled = true
        var done = 0
        // 前の差分が出てから次を選ぶ。行は History 全体に散らす（キャッシュに乗らないように）。
        func next() {
            guard done < count else {
                detail.onDiffShown = nil
                FrameProfile.enabled = false
                print("selections: \(count)\n" + FrameProfile.summary())
                NSApp.terminate(nil)
                return
            }
            // 起動時に選んである先頭の行は避ける（選び直しても読み込みが起きない）。
            let row = 1 + (done * 7919) % max(1, total - 1)
            done += 1
            history.select(row: row)
        }
        detail.onDiffShown = {
            FrameProfile.endFrame()
            DispatchQueue.main.async { next() }
        }
        next()
    }

    /// daemon 越しの場所の、パスの部分を差し替える（サブモジュールを同じ接続先で開く）。
    private static func replacingPath(of location: String, with path: String) -> String {
        let old = RepositoryLocation.path(location)
        guard location.hasSuffix(old) else { return location }
        return String(location.dropLast(old.count)) + path
    }

    // MARK: - blame・ファイルの履歴

    private func openBlame(path: String, rev: String?) {
        BlameWindowController.show(path: path, rev: rev, repository: repository) { [weak self] id in self?.reveal(commit: id) }
    }

    private func openFileHistory(path: String) {
        FileHistoryWindowController.show(path: path, repository: repository, renderer: textRenderer) { [weak self] id in
            self?.reveal(commit: id)
        }
    }

    /// History に切り替えて `commit` を選ぶ。今の範囲になければ全ブランチにする。
    private func reveal(commit: Data) {
        sidebar.select(page: .history)
        pages?.show(.history)
        if let row = history.currentHistory?.row(of: commit) {
            history.select(row: row)
        } else {
            history.set(scope: .all, select: commit)
        }
        window.makeKeyAndOrderFront(nil)
    }

    func windowWillClose(_ notification: Notification) {
        watch = nil
        onClose?()
    }
}
