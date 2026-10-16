import AmonhenKit
import AppKit

/// 1 つのリポジトリのウィンドウ。サイドバー・History・Working Copy・ツールバー・監視を持つ。
final class RepositoryWindow: NSObject, NSWindowDelegate, NSMenuItemValidation, NSToolbarItemValidation {
    /// 手元のパスか、daemon 越しの場所（`daemon:/path`・`ssh://host/path`）。
    let location: String
    /// 差分の文字を描く（ファイルの履歴のウィンドウでも使う）。
    private let textRenderer: TextRenderer
    private var pages: PageViewController?
    let window: NSWindow
    /// ほかのリポジトリ（サブモジュールなど）を新しいウィンドウで開く。
    var onOpenRepository: ((String) -> Void)?
    var onClose: (() -> Void)?
    /// 切れた daemon 越しのリポジトリをつなぎ直す（AppDelegate がこのウィンドウを閉じて開き直す）。
    var onReconnect: (() -> Void)?
    /// daemon 越しのとき、受け取った量と切断を見張る。
    private var connectionTimer: Timer?

    private let repository: Repository
    private let history: HistoryViewController
    private let sidebar: SidebarViewController
    private let workingCopy: WorkingCopyViewController
    private let detail: CommitDetailViewController
    private let actions: Actions
    private let toolbar: MainToolbar
    private var watch: RepositoryWatch?
    /// 最後に読んだ ref（詳細の欄で ref の名前を押したときに引く）。
    private var latestRefs: Refs?
    /// History で選んでいるコミット（Merge・Rebase の相手の候補）。
    private var selectedCommit: Data?
    /// 次に Undo される操作の名前。
    private var undoLabel: String?

    /// リポジトリを開く。daemon 越しなら SSH でつなぎ、daemon を送り込んで起動するので数秒かかる（裏で呼ぶこと）。
    /// `daemon`・`daemons` は `Daemon.path`・`Daemon.bundledDirectory`（メインスレッドで読んで渡す）。
    nonisolated static func openRepository(location: String, daemon: String, daemons: String) throws -> Repository {
        let cacheDirectory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent(Bundle.main.bundleIdentifier ?? "io.github.fukusaka.amonhen")
        return if RepositoryLocation.isRemote(location) {
            try Repository(location: location, daemon: daemon, daemons: daemons, cacheDirectory: cacheDirectory)
        } else {
            try Repository(path: location, cacheDirectory: cacheDirectory)
        }
    }

    /// `debug` なら、起動時の引数（`--snapshot` など）の確認用の動きをする。
    convenience init(location: String, debug: Bool, tabWith: NSWindow?) throws {
        let repository = try Self.openRepository(location: location, daemon: Daemon.path, daemons: Daemon.bundledDirectory)
        try self.init(location: location, repository: repository, debug: debug, tabWith: tabWith)
    }

    /// タブのまとまりの名前（同じ名前のウィンドウだけがタブでまとまる）。
    static let tabbingIdentifier = "io.github.fukusaka.amonhen.repository"


    /// 開いた `repository` のウィンドウを作る。
    /// `tabWith` があればそのウィンドウのタブに入れ、なければ新しいウィンドウにする。
    init(location: String, repository: Repository, debug: Bool, tabWith: NSWindow?) throws {
        self.location = location
        self.repository = repository
        guard let renderer = GraphRenderer(), let text = TextRenderer(device: renderer.device) else {
            throw AmonhenError(description: "Metal を使えない")
        }
        textRenderer = text
        history = HistoryViewController(repository: repository, renderer: renderer, text: text)
        sidebar = SidebarViewController()
        detail = CommitDetailViewController(repository: repository, renderer: text)
        workingCopy = WorkingCopyViewController(repository: repository, renderer: text)
        actions = Actions(repository: repository)
        toolbar = MainToolbar()
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
        window.tabbingIdentifier = Self.tabbingIdentifier
        if let tabWith {
            tabWith.addTabbedWindow(window, ordered: .above)
            window.makeKeyAndOrderFront(nil)
        } else {
            // 新しいウィンドウにする（タブにしない）。出した後は、ほかのウィンドウをタブとして受け入れる。
            window.tabbingMode = .disallowed
            window.makeKeyAndOrderFront(nil)
            window.tabbingMode = .preferred
        }
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

        history.onSelectCommit = { [weak self] commit in
            self?.selectedCommit = commit
            self?.detail.show(commit: commit)
        }
        history.onSelectRows = { [weak self] rows, history in
            self?.selectedCommit = nil
            self?.detail.show(selection: rows, in: history)
        }
        history.onSearchChanged = { [weak detail] query in detail?.searchQuery = query }
        detail.onSearch = { [weak history] query in history?.search(query) }
        detail.onSelectRef = { [weak self] name in self?.selectRef(named: name) }
        updateSignaturePlace()
        detail.onReword = { [weak actions] oid, message in
            actions?.rewrite("Reword \(oid.prefix(8))", ["kind": "reword", "oid": oid, "message": message])
        }
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
        workingCopy.onSequenceChange = { [weak self] sequence, conflicts in
            self?.showSequenceBanner(sequence, conflicts: conflicts)
        }
        workingCopy.onChangeCount = { [weak self] count in
            self?.changeCount = count
            self?.updateTabTitle()
        }
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
        actions.onUndoChange = { [weak self] label in
            self?.undoLabel = label
            self?.toolbar.setUndo(label)
        }
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
                PerformanceLog.record("History: 入力 → 表示", ms: latency * 1000)
            }
        } else {
            history.onLatency = { latency in PerformanceLog.record("History: 入力 → 表示", ms: latency * 1000) }
        }
        let repository = self.repository
        let started = Date()
        history.load { [weak self] error in
            self?.history.loadingMessage = nil
            if let error, self?.isDisconnected != true { NSAlert(error: error).runModal() }
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
        watchConnection()
        if debug { startDebugOptions() }
    }

    private var isDisconnected: Bool {
        if case .closed = repository.connectionState { true } else { false }
    }

    /// daemon 越しなら、初めての読み込みの進み具合と、接続が切れたことを出す。
    private func watchConnection() {
        guard RepositoryLocation.isRemote(location) else { return }
        repository.setLocalSignatureCheck(Settings.checkRemoteSignaturesLocally)
        NotificationCenter.default.addObserver(
            self, selector: #selector(signatureCheckChanged), name: .signatureCheckChanged, object: nil)
        updateConnection()
        connectionTimer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.updateConnection() }
        }
    }

    private func updateConnection() {
        switch repository.connectionState {
        case .local:
            connectionTimer?.invalidate()
        case let .connected(received):
            // キャッシュがなく、まだ何も出せないときだけ（キャッシュがあれば差分だけで済み、すぐ終わる）。
            if history.currentHistory == nil {
                let size = ByteCountFormatter.string(fromByteCount: Int64(received), countStyle: .file)
                history.loadingMessage = "History を読み込んでいます… \(size) 受信"
            }
        case let .closed(reason):
            connectionTimer?.invalidate()
            connectionTimer = nil
            history.loadingMessage = nil
            showDisconnected(reason: reason)
        }
    }

    /// 署名を検証する側が変わった。コアと画面が覚えていた結果を捨て、見えているものを検証し直す。
    @objc private func signatureCheckChanged() {
        repository.setLocalSignatureCheck(Settings.checkRemoteSignaturesLocally)
        updateSignaturePlace()
        history.forgetSignatures()
        detail.reloadSignature()
    }

    /// 署名をどこで検証しているか（詳細の吹き出しに出す）。設定が効くのは SSH 先のリポジトリだけなので、
    /// 理由も添える。
    private func updateSignaturePlace() {
        guard let host = RepositoryLocation.host(location) else {
            detail.signaturePlace = SignaturePlace(local: true, detail: "リポジトリがこの Mac にある")
            return
        }
        detail.signaturePlace = Settings.checkRemoteSignaturesLocally
            ? SignaturePlace(local: true, detail: "設定で選択")
            : SignaturePlace(local: false, host: host, detail: "設定で選択")
    }

    // MARK: - タブの名前

    private var changeCount = 0
    private var tracking = (ahead: 0, behind: 0)

    /// タブの名前に、未コミットの変更（●）と上流との差（↑↓）を添える。ウィンドウのタイトルは変えない。
    private func updateTabTitle() {
        var title = window.title
        if changeCount > 0 { title += " ●" }
        if tracking.ahead > 0 { title += " ↑\(tracking.ahead)" }
        if tracking.behind > 0 { title += " ↓\(tracking.behind)" }
        window.tab.title = title
        window.tab.toolTip = [
            changeCount > 0 ? "変更のあるファイル \(changeCount) 個" : nil,
            tracking.ahead > 0 ? "上流より \(tracking.ahead) コミット先" : nil,
            tracking.behind > 0 ? "上流より \(tracking.behind) コミット後ろ" : nil,
        ].compactMap { $0 }.joined(separator: "、")
    }

    // MARK: - merge・rebase の途中の帯

    private var sequenceBanner: NSTitlebarAccessoryViewController?
    private let sequenceLabel = NSTextField(labelWithString: "")
    private let skipButton = NSButton(title: "Skip", target: nil, action: nil)
    private let continueButton = NSButton(title: "Continue", target: nil, action: nil)

    /// merge・rebase・cherry-pick・revert の途中なら、ツールバーの下に帯を出す。コンフリクトが残っていれば数と
    /// Working Copy へのリンク、なければ Continue を強調する。
    private func showSequenceBanner(_ sequence: String?, conflicts: Int) {
        guard let sequence else {
            if let banner = sequenceBanner,
               let index = window.titlebarAccessoryViewControllers.firstIndex(of: banner) {
                window.removeTitlebarAccessoryViewController(at: index)
            }
            sequenceBanner = nil
            return
        }
        let name = sequence.prefix(1).uppercased() + sequence.dropFirst()
        sequenceLabel.stringValue = conflicts > 0
            ? "\(name) の途中です · コンフリクトのファイルが \(conflicts) 個"
            : "\(name) の途中です · コンフリクトは解決済み"
        skipButton.isHidden = !(sequence.hasPrefix("rebase") || sequence == "cherry-pick")
        continueButton.keyEquivalent = conflicts == 0 ? "\r" : ""
        guard sequenceBanner == nil else { return }
        let icon = NSImageView(image: NSImage(systemSymbolName: "arrow.triangle.merge", accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = .systemOrange
        sequenceLabel.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
        let show = NSButton(title: "Working Copy を開く", target: self, action: #selector(showWorkingCopy(_:)))
        continueButton.target = self
        continueButton.action = #selector(continueSequence(_:))
        skipButton.target = self
        skipButton.action = #selector(skipSequence(_:))
        let abort = NSButton(title: "Abort", target: self, action: #selector(abortSequence(_:)))
        for b in [show, continueButton, skipButton, abort] { b.controlSize = .small }
        let stack = NSStackView(views: [icon, sequenceLabel, NSView(), show, continueButton, skipButton, abort])
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 4, right: 12)
        stack.frame = NSRect(x: 0, y: 0, width: 600, height: 30)
        let bar = NSTitlebarAccessoryViewController()
        bar.view = stack
        bar.layoutAttribute = .bottom
        window.addTitlebarAccessoryViewController(bar)
        sequenceBanner = bar
    }

    /// ツールバーの下に、切れたことと「つなぎ直す」を出す。
    private func showDisconnected(reason: String) {
        let icon = NSImageView(image: NSImage(systemSymbolName: "bolt.horizontal.circle", accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = .systemOrange
        let host = RepositoryLocation.host(location) ?? location
        let label = NSTextField(labelWithString: "\(host) との接続が切れました")
        label.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
        let detail = NSTextField(labelWithString: reason)
        detail.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        detail.textColor = .secondaryLabelColor
        detail.lineBreakMode = .byTruncatingTail
        detail.toolTip = reason
        detail.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let button = NSButton(title: "つなぎ直す", target: self, action: #selector(reconnect))
        button.controlSize = .small
        let stack = NSStackView(views: [icon, label, detail, NSView(), button])
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 4, right: 12)
        stack.frame = NSRect(x: 0, y: 0, width: 600, height: 30)
        let bar = NSTitlebarAccessoryViewController()
        bar.view = stack
        bar.layoutAttribute = .bottom
        window.addTitlebarAccessoryViewController(bar)
    }

    @objc private func reconnect() {
        onReconnect?()
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

    /// 詳細の欄で押した ref の履歴に絞り込み、ref が指すコミットを選ぶ（サイドバーで選んだときと同じ）。
    private func selectRef(named name: String) {
        guard let item = latestRefs?.items.first(where: { $0.name == name && [.local, .remote, .tag].contains($0.kind) }),
              let target = item.target else { return NSSound.beep() }
        history.set(scope: .ref(item.name, target), select: target)
    }

    /// View > Go to Parent（⌥⌘↓）・Go to Child（⌥⌘↑）。最初の親・最初の子へ移る。
    @objc func goToParent(_ sender: Any?) { goTo(detail.currentParents.first) }
    @objc func goToChild(_ sender: Any?) { goTo(detail.currentChildren.first) }

    private func goTo(_ hex: String?) {
        guard let hex, let id = Data(hex: hex) else { return NSSound.beep() }
        goTo(id: id)
    }

    private func goTo(id: Data) {
        if let row = history.currentHistory?.row(of: id) {
            history.select(row: row)
        } else {
            history.set(scope: .all, select: id)
        }
    }

    /// View > Refresh（⌘R）。変化の知らせを待たずに、History・ブランチ・Working Copy を読み直す。
    /// ウィンドウの delegate はレスポンダチェーンに入るので、メニューから届く。
    @objc func refreshRepository(_ sender: Any?) {
        repositoryDidChange(Changes(rawValue: ~0))
        actions.refreshUndo()
    }

    /// 裏のタブで溜めた変化（前に出たときに読み直す）。
    private var pendingChanges = Changes(rawValue: 0)

    /// タブのまとまりの中で、前に出ていないタブか。
    private var isBackgroundTab: Bool {
        guard let group = window.tabGroup, group.windows.count > 1 else { return false }
        return group.selectedWindow !== window
    }

    func windowDidBecomeKey(_ notification: Notification) {
        guard pendingChanges.rawValue != 0 else { return }
        let changes = pendingChanges
        pendingChanges = Changes(rawValue: 0)
        repositoryDidChange(changes)
    }

    private func repositoryDidChange(_ changes: Changes) {
        let names = [(Changes.refs, "ref"), (.index, "index"), (.worktree, "作業ツリー"), (.config, "設定")]
            .filter { changes.contains($0.0) }.map(\.1)
        EventLog.add(
            repo: window.title,
            "変化: " + names.joined(separator: "・") + (isBackgroundTab && !changes.isDisjoint(with: [.index, .worktree]) ? "（裏のタブ: 作業ツリーは前に出るまで待つ）" : ""))
        // 裏のタブでは、作業ツリーの変化（ビルドなどで頻繁に来る）の読み直しを前に出るまで待つ。ref の変化は
        // タブの名前（ahead・behind）に出すので、すぐ読む。
        if isBackgroundTab, !changes.intersection([.index, .worktree]).isEmpty {
            pendingChanges.formUnion(changes.intersection([.index, .worktree]))
            let rest = changes.subtracting([.index, .worktree])
            guard rest.rawValue != 0 else { return }
            return repositoryDidChange(rest)
        }
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
                latestRefs = refs
                sidebar.show(refs)
                detail.reloadInfo()
                if !CommandLine.arguments.contains("--show-latency") {
                    window.subtitle = refs.headBranch ?? "detached HEAD"
                }
                history.head = refs.head
                history.show(refs: refs)
                let current = refs.items.first { $0.kind == .local && $0.name == refs.headBranch }
                toolbar.setTracking(ahead: current?.ahead ?? 0, behind: current?.behind ?? 0)
                tracking = (current?.ahead ?? 0, current?.behind ?? 0)
                updateTabTitle()
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

    // MARK: - メニューとツールバーの操作（レスポンダチェーンで届く）

    private var isLocal: Bool { !RepositoryLocation.isRemote(location) }
    private var showsWorkingCopy: Bool { pages?.page == .workingCopy }

    @objc func fetch(_ sender: Any?) { actions.fetch() }
    @objc func pull(_ sender: Any?) { pull(mode: Settings.pullMode) }
    @objc func pullMerge(_ sender: Any?) { pull(mode: .merge) }
    @objc func pullRebase(_ sender: Any?) { pull(mode: .rebase) }
    @objc func pullFastForward(_ sender: Any?) { pull(mode: .fastForwardOnly) }

    private func pull(mode: PullMode) {
        actions.perform(["op": "pull", "mode": mode.rawValue])
    }

    @objc func push(_ sender: Any?) { actions.push() }

    @objc func forcePush(_ sender: Any?) {
        let branch = latestRefs?.headBranch ?? "HEAD"
        guard actions.confirm(
            "\(branch) を force push しますか？",
            message: "上流のブランチを手元の内容で置き換えます（--force-with-lease。ほかの人が先に push していたら失敗します）。",
            ok: "Force Push") else { return }
        actions.push(force: true)
    }

    @objc func stashChanges(_ sender: Any?) { actions.stash() }

    /// 最新の stash（`stash@{0}`）を戻して消す。
    @objc func popStash(_ sender: Any?) {
        guard let name = latestStash else { return NSSound.beep() }
        actions.perform(["op": "stashApply", "name": name, "pop": true])
    }

    private var latestStash: String? {
        latestRefs?.items.first { $0.kind == .stash }?.name
    }

    @objc func mergeIntoCurrent(_ sender: Any?) {
        let current = latestRefs?.headBranch ?? "HEAD"
        guard let rev = chooseRevision(
            title: "\(current) に merge する", message: "取り込むブランチ・タグ・コミット", ok: "Merge") else { return }
        actions.perform(["op": "merge", "rev": rev])
    }

    @objc func rebaseCurrent(_ sender: Any?) {
        let current = latestRefs?.headBranch ?? "HEAD"
        guard let rev = chooseRevision(
            title: "\(current) を rebase する", message: "上に載せる先のブランチ・タグ・コミット", ok: "Rebase") else { return }
        actions.perform(["op": "rebase", "onto": rev])
    }

    /// 新しいブランチ・タグの起点。History で選んでいるコミットがあればそれ、なければ HEAD。
    private var startPoint: String {
        if pages?.page == .history, let selectedCommit {
            return String(selectedCommit.map { String(format: "%02x", $0) }.joined().prefix(12))
        }
        return latestRefs?.headBranch ?? "HEAD"
    }

    @objc func newBranch(_ sender: Any?) { actions.newBranch(at: startPoint) }
    @objc func newTag(_ sender: Any?) { actions.newTag(at: startPoint) }

    @objc func continueSequence(_ sender: Any?) { actions.perform(["op": "continue"]) }
    @objc func skipSequence(_ sender: Any?) { actions.perform(["op": "skip"]) }
    @objc func abortSequence(_ sender: Any?) { actions.perform(["op": "abort"]) }

    /// Repository > Undo（⌥⌘Z）・ツールバーの Undo。git の操作を操作ログで戻す（テキストの ⌘Z とは別）。
    @objc func undoOperation(_ sender: Any?) { actions.undo() }

    @objc func showWorkingCopy(_ sender: Any?) { sidebar.select(page: .workingCopy) }
    @objc func showHistory(_ sender: Any?) { sidebar.select(page: .history) }

    @objc func findInHistory(_ sender: Any?) {
        sidebar.select(page: .history)
        history.showFindBar(sender)
    }

    @objc func goToHead(_ sender: Any?) {
        guard let head = latestRefs?.head else { return NSSound.beep() }
        sidebar.select(page: .history)
        goTo(id: head)
    }

    @objc func commitChanges(_ sender: Any?) { workingCopy.commitFromMenu() }
    @objc func stageAllChanges(_ sender: Any?) { workingCopy.stageAllFromMenu() }

    /// Activity の画面で、このリポジトリの操作ログを読み・戻すため。
    var activityRepository: ActivityRepository {
        ActivityRepository(name: window.title, repository: repository, actions: actions)
    }

    /// Repository > Repository Settings…（⌘;）。
    @objc func showRepositorySettings(_ sender: Any?) {
        RepositorySettingsSheet.show(for: repository, title: window.title, in: window) { [weak self] in
            self?.refreshRepository(nil)
        }
    }

    @objc func openInTerminal(_ sender: Any?) {
        guard isLocal, let terminal = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.Terminal")
        else { return NSSound.beep() }
        let url = URL(fileURLWithPath: RepositoryLocation.path(location))
        NSWorkspace.shared.open([url], withApplicationAt: terminal, configuration: NSWorkspace.OpenConfiguration())
    }

    @objc func revealInFinder(_ sender: Any?) {
        guard isLocal else { return NSSound.beep() }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: RepositoryLocation.path(location))])
    }

    /// Merge・Rebase の相手を選んでもらう。候補は History で選んでいるコミット（あれば先頭）、
    /// 今のブランチ以外のローカルブランチ、リモートブランチ、タグ。キャンセルなら nil。
    private func chooseRevision(title: String, message: String, ok: String) -> String? {
        var choices: [(title: String, rev: String)] = []
        if let selectedCommit, selectedCommit != latestRefs?.head {
            let hex = selectedCommit.map { String(format: "%02x", $0) }.joined()
            choices.append(("選んでいるコミット \(hex.prefix(8))", hex))
        }
        let items = latestRefs?.items ?? []
        for kind in [RefKind.local, .remote, .tag] {
            for item in items where item.kind == kind && item.name != latestRefs?.headBranch {
                choices.append((item.name, item.name))
            }
        }
        guard !choices.isEmpty else {
            NSSound.beep()
            return nil
        }
        let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 320, height: 26), pullsDown: false)
        for (i, choice) in choices.enumerated() {
            popup.addItem(withTitle: choice.title)
            popup.lastItem?.tag = i
        }
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: "キャンセル")
        alert.accessoryView = popup
        alert.window.initialFirstResponder = popup
        guard alert.runModal() == .alertFirstButtonReturn, let item = popup.selectedItem else { return nil }
        return choices[item.tag].rev
    }

    /// メニューの項目とツールバーのボタンを使えるか。
    private func validate(_ action: Selector?) -> Bool {
        let sequence = workingCopy.sequence
        switch action {
        case #selector(continueSequence(_:)), #selector(abortSequence(_:)): return sequence != nil
        case #selector(skipSequence(_:)): return sequence.map { $0.hasPrefix("rebase") || $0 == "cherry-pick" } ?? false
        case #selector(push(_:)), #selector(forcePush(_:)): return latestRefs?.headBranch != nil
        case #selector(pull(_:)), #selector(pullMerge(_:)), #selector(pullRebase(_:)), #selector(pullFastForward(_:)):
            return latestRefs?.headBranch != nil && sequence == nil
        case #selector(mergeIntoCurrent(_:)), #selector(rebaseCurrent(_:)): return latestRefs != nil && sequence == nil
        case #selector(popStash(_:)): return latestStash != nil
        case #selector(undoOperation(_:)): return undoLabel != nil
        case #selector(goToHead(_:)): return latestRefs?.head != nil
        case #selector(commitChanges(_:)): return showsWorkingCopy && workingCopy.canCommit
        case #selector(stageAllChanges(_:)): return showsWorkingCopy && workingCopy.canStageAll
        case #selector(openInTerminal(_:)), #selector(revealInFinder(_:)): return isLocal
        default: return true
        }
    }

    func windowWillClose(_ notification: Notification) {
        connectionTimer?.invalidate()
        watch = nil
        onClose?()
    }
}

extension RepositoryWindow {
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(undoOperation(_:)):
            item.title = undoLabel.map { "Undo \($0)" } ?? "Undo"
        case #selector(showWorkingCopy(_:)):
            item.state = pages?.page == .workingCopy ? .on : .off
        case #selector(showHistory(_:)):
            item.state = pages?.page == .history ? .on : .off
        default:
            break
        }
        return validate(item.action)
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        validate(item.action)
    }
}
