import AmonhenKit
import AppKit

/// 1 つのリポジトリのウィンドウ。サイドバー・History・Working Copy・ツールバー・監視を持つ。
final class RepositoryWindow: NSObject, NSWindowDelegate, NSMenuItemValidation, NSToolbarItemValidation,
    HistoryMatchNavigating
{
    /// 手元のパスか、daemon 越しの場所（`daemon:/path`・`ssh://host/path`）。
    let location: String
    /// 差分の文字を描く（ファイルの履歴のウィンドウでも使う）。
    private let textRenderer: TextRenderer
    private var pages: PageViewController?
    let window: NSWindow
    /// ほかのリポジトリ（サブモジュールなど）を新しいウィンドウで開く。
    var onOpenRepository: ((String) -> Void)?
    /// ツールバーのリポジトリ名から開く一覧で選んだ: このタブを切り替える（`here`）・新しいタブ・新しいウィンドウ。
    var onSwitchRepository: ((String, RepositoryBrowser.OpenMode) -> Void)?
    /// 一覧の「すべてのリポジトリ…」: リポジトリのページを出す。
    var onShowRepositories: (() -> Void)?
    private var repositoryPopover: NSPopover?
    var onClose: (() -> Void)?
    /// 切れた daemon 越しのリポジトリをつなぎ直す（AppDelegate がこのウィンドウを閉じて開き直す）。
    var onReconnect: (() -> Void)?
    /// daemon 越しのとき、受け取った量と切断を見張る。
    private var connectionTimer: Timer?

    private let repository: Repository
    private let history: HistoryViewController
    private let sidebar: SidebarViewController
    /// 書き込む先の worktree（git の「今いる worktree」）。HEAD・index・作業ツリーに触れる操作はここに向ける。
    /// History・ref・詳細は、開いたリポジトリ（`repository`）のものを全部の worktree で共通に使う。
    private var editing: WorktreeSlot
    /// 開いたリポジトリの組（最初の書き込む先）。
    private let opened: WorktreeSlot
    /// 一度書き込む先にした worktree の組（パスで引く）。
    private var slots: [String: WorktreeSlot] = [:]
    /// リポジトリの worktree（`git worktree list`）。
    private var worktrees: [WorktreeInfo] = []
    private var workingCopy: WorkingCopyViewController { editing.workingCopy }
    private let detail: CommitDetailViewController
    /// Stashes の画面（stash の一覧と、選んだ stash の中身）。
    private let stashes: StashesViewController
    /// Reflog の画面（書き込む先の worktree の HEAD の reflog と、選んだ行のコミットの中身）。
    private let reflog: ReflogViewController
    /// リポジトリの設定の画面（サイドバーの Workspace の Settings）。
    private let settings: RepositorySettingsPage
    private var actions: Actions { editing.actions }
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
        guard RepositoryLocation.isRemote(location) else {
            return try Repository(path: location, cacheDirectory: cacheDirectory)
        }
        // SSH 先ごとの選択（エンジンと git）も渡す。
        let repository = try Repository(location: location, daemon: daemon, daemons: daemons, cacheDirectory: cacheDirectory,
                                        hostOptions: Settings.hostOptionsJSON(for: location))
        // 署名をどこで検証するかは、何かを読む前に渡す（後で渡すと、開いてすぐ読んだ詳細だけ SSH 先の鍵で検証される）。
        repository.setLocalSignatureCheck(Settings.checksSignaturesLocally(at: location))
        return repository
    }

    /// `debug` なら、起動時の引数（`--snapshot` など）の確認用の動きをする。
    convenience init(location: String, debug: Bool, tabWith: NSWindow?) throws {
        let repository = try Self.openRepository(location: location, daemon: Daemon.path, daemons: Daemon.bundledDirectory)
        try self.init(location: location, repository: repository, debug: debug, tabWith: tabWith)
    }

    /// タブのまとまりの名前（同じ名前のウィンドウだけがタブでまとまる）。
    static let tabbingIdentifier = "io.github.fukusaka.amonhen.repository"


    /// 開いた `repository` のウィンドウを作る。
    /// `tabWith` があればそのウィンドウのタブに入れ、なければ新しいウィンドウにする。`front` でなければ、タブに入れる
    /// だけで前に出さない（裏で接続が終わったタブ）。
    init(location: String, repository: Repository, debug: Bool, tabWith: NSWindow?, front: Bool = true) throws {
        self.location = location
        self.repository = repository
        guard let renderer = GraphRenderer(), let text = TextRenderer(device: renderer.device) else {
            throw AmonhenError(description: L("Metal isn’t available."))
        }
        textRenderer = text
        history = HistoryViewController(repository: repository, renderer: renderer, text: text)
        sidebar = SidebarViewController()
        detail = CommitDetailViewController(repository: repository, renderer: text)
        stashes = StashesViewController(repository: repository, renderer: text)
        reflog = ReflogViewController(repository: repository, renderer: text)
        settings = RepositorySettingsPage(repository: repository, location: location)
        let slot = WorktreeSlot(path: repository.workdir ?? RepositoryLocation.path(location), repository: repository, renderer: text)
        opened = slot
        editing = slot
        toolbar = MainToolbar()
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1440, height: 820),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init()
        let path = RepositoryLocation.path(location)
        // 表示名（リポジトリの設定で変えられる）。daemon 越しなら接続先を添える（手元のリポジトリと見分ける）。
        window.title = Bookmarks.shared.displayName(location)
        if RepositoryLocation.host(location) == nil {
            window.representedURL = URL(fileURLWithPath: path)
        }
        window.isReleasedWhenClosed = false
        window.delegate = self
        assemble()
        scheduleAutoFetch()
        // リポジトリごとに、前回の位置と大きさで開く（初めてなら真ん中に）。
        let frameName = "Repository \(location)"
        window.setContentSize(NSSize(width: 1440, height: 820))
        if !window.setFrameUsingName(frameName) { window.center() }
        window.setFrameAutosaveName(frameName)
        window.tabbingIdentifier = Self.tabbingIdentifier
        if let tabWith {
            tabWith.addTabbedWindow(window, ordered: .above)
            if front { window.makeKeyAndOrderFront(nil) }
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
        let pages = PageViewController(pages: [.history: content, .workingCopy: workingCopy, .stashes: stashes, .reflog: reflog, .settings: settings])
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
        // 行の右端の CI の印を押したら、そのコミットのチェックの一覧を吹き出しで。
        history.onClickCI = { [weak self] id, ci, rect, view in
            guard let self, let gitHub else { return }
            let sha = id.map { String(format: "%02x", $0) }.joined()
            ciPopovers.toggle(key: sha, status: ci, sha: sha, repo: gitHub, relativeTo: rect, of: view)
        }
        history.onSearchChanged = { [weak self] query in
            self?.detail.searchQuery = query
            self?.toolbar.setSearch(query)
        }
        toolbar.onSearch = { [weak self] query in
            guard let self else { return }
            sidebar.select(page: .history)
            history.search(query)
        }
        toolbar.onSearchStep = { [weak self] step in
            guard let self else { return }
            sidebar.select(page: .history)
            history.moveToMatch(by: step)
        }
        detail.onSearch = { [weak history] query in history?.search(query) }
        history.toolbarSearchShown = { [weak self] in self?.toolbarHasSearch ?? false }
        history.focusToolbarSearch = { [weak self] in self?.toolbar.focusSearch() }
        // ツールバーの検索欄を足し引き・ツールバーを隠したら、History の検索欄を出し直す。
        for name in [NSToolbar.willAddItemNotification, NSToolbar.didRemoveItemNotification] {
            NotificationCenter.default.addObserver(forName: name, object: toolbar.toolbar, queue: .main) { [weak self] _ in
                DispatchQueue.main.async {
                    self?.history.view.needsLayout = true
                    self?.updateSubtitle()
                }
            }
        }
        detail.onSelectRef = { [weak self] name in self?.selectRef(named: name) }
        updateSignaturePlace()
        detail.onReword = { [weak self] oid, message in
            self?.actions.rewrite("Reword \(oid.prefix(8))", ["kind": "reword", "oid": oid, "message": message])
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
        // stash の詳細の親（土台）のリンクは、History でそのコミットを選ぶ。
        stashes.detail.onSelectCommit = { [weak self] hex in
            guard let self, let id = Data(hex: hex) else { return }
            sidebar.select(page: .history)
            if let row = history.currentHistory?.row(of: id) {
                history.select(row: row)
            } else {
                history.set(scope: .all, select: id)
            }
        }
        stashes.detail.onBlame = { [weak self] path, commit in self?.openBlame(path: path, rev: commit) }
        stashes.detail.onFileHistory = { [weak self] path in self?.openFileHistory(path: path) }
        // reflog の行のコミット・詳細の親のリンクは、History でそのコミットを選ぶ。
        let showInHistory = { [weak self] (id: Data) in
            guard let self else { return }
            sidebar.select(page: .history)
            if let row = history.currentHistory?.row(of: id) {
                history.select(row: row)
            } else {
                history.set(scope: .all, select: id)
            }
        }
        reflog.onShowInHistory = showInHistory
        reflog.detail.onSelectCommit = { hex in Data(hex: hex).map(showInHistory) }
        reflog.detail.onBlame = { [weak self] path, commit in self?.openBlame(path: path, rev: commit) }
        reflog.detail.onFileHistory = { [weak self] path in self?.openFileHistory(path: path) }
        detail.onBlame = { [weak self] path, commit in self?.openBlame(path: path, rev: commit) }
        detail.onFileHistory = { [weak self] path in self?.openFileHistory(path: path) }
        sidebar.onSelectConflicts = { [weak self] in self?.workingCopy.selectFirstConflict() }
        sidebar.onCheckoutPullRequest = { [weak self] pr in self?.checkout(pr) }
        sidebar.onCreatePullRequest = { [weak self] branch in self?.createPullRequest(from: branch) }
        sidebar.onClickPullRequestCI = { [weak self] pr, view in
            guard let self, let gitHub, let sha = pr.headRefOid else { return }
            ciPopovers.toggle(key: "pr\(pr.number)", status: pr.ci, sha: sha, repo: gitHub, relativeTo: view.bounds, of: view)
        }
        sidebar.onSelectWorktree = { [weak self] path in self?.edit(worktree: path) }
        sidebar.onWorktreeAction = { [weak self] path, action in self?.worktreeAction(path, action) }
        sidebar.onRemoteAction = { [weak self] name, action in self?.remoteAction(name, action) }
        // ブランチ・タグを選ぶと、その履歴に絞り込む。
        sidebar.onSelect = { [weak self, weak history, weak detail] item in
            guard let history, let target = item.target else { return }
            if [.local, .remote, .tag].contains(item.kind) {
                // 上流と分かれていれば上流も一緒に辿り、先にある・遅れているコミットを両方 History に出す。
                let upstream = item.kind == .local && (item.ahead > 0 || item.behind > 0)
                    ? self?.latestRefs?.items.first { $0.kind == .remote && $0.name == item.detail }?.target : nil
                history.set(scope: .ref(item.kind, item.name, target), select: target, companions: upstream.map { [$0] } ?? [])
            } else if let row = history.currentHistory?.row(of: target) {
                history.select(row: row)
            }
        }
        // ブランチ・タグを複数選ぶと、それぞれから辿れる履歴をまとめて出す。
        sidebar.onSelectMany = { [weak history] items in
            history?.set(scope: .refs(items.compactMap { item in
                item.target.map { HistoryViewController.Ref(kind: item.kind, name: item.name, target: $0) }
            }))
        }
        let root = repository.workdir ?? RepositoryLocation.path(location)
        let remote = RepositoryLocation.isRemote(location)
        sidebar.onOpenSubmodule = { [weak self] path in
            // サブモジュールは、同じ接続先の上の相対パスとして開く。
            let full = (root as NSString).appendingPathComponent(path)
            self?.onOpenRepository?(remote ? Self.replacingPath(of: self?.location ?? "", with: full) : full)
        }

        NotificationCenter.default.addObserver(
            self, selector: #selector(gitHubSettingsChanged), name: .gitHubSettingsChanged, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(autoFetchSettingChanged), name: .autoFetchChanged, object: nil)
        wire(editing)
        sidebar.actions = actions
        sidebar.repository = repository
        history.actions = actions
        stashes.actions = actions
        reflog.actions = actions
        // git の設定・リモートを変えたら、読み直す。
        settings.model.onRepositoryChange = { [weak self] in self?.refreshRepository(nil) }
        window.toolbar = toolbar.toolbar
        toolbar.setRepository(window.title)
    }

    /// worktree の組の、Working Copy と操作の知らせをこのウィンドウにつなぐ。書き込む先でないものの知らせは、
    /// 帯やタブの名前には出さない。
    private func wire(_ slot: WorktreeSlot) {
        slot.workingCopy.onSequenceChange = { [weak self, weak slot] sequence, conflicts in
            guard let self, slot === editing else { return }
            showSequenceBanner(sequence, conflicts: conflicts)
            sidebar.setConflictCount(conflicts)
        }
        slot.workingCopy.onChangeCount = { [weak self, weak slot] count in
            guard let self, let slot else { return }
            worktreeChanges[WorktreeInfo.canonical(slot.path)] = count
            sidebar.setWorktreeChanges(worktreeChanges)
            showWorktreeHeads()
            guard slot === editing else { return }
            changeCount = count
            updateTabTitle()
        }
        slot.workingCopy.onBlame = { [weak self, weak slot] path in self?.openBlame(path: path, rev: nil, in: slot?.repository) }
        slot.workingCopy.issueCandidates = { [weak self] in await self?.issueCandidates() ?? [] }
        slot.workingCopy.onFileHistory = { [weak self, weak slot] path in self?.openFileHistory(path: path, in: slot?.repository) }
        slot.actions.window = window
        slot.actions.onFailure = { [weak self] failure in self?.showFailure(failure) }
        // Push のシートは、右の欄と同じ詳細の表示を読み取り専用で埋め込む（同じ描画器で）。
        slot.actions.makeDetailView = { [weak self, weak slot] in
            guard let self, let slot else { return nil }
            return CommitDetailViewController(repository: slot.repository, renderer: textRenderer)
        }
        slot.actions.onChange = { [weak self, weak slot] in
            self?.history.refresh()
            self?.reloadRefs()
            slot?.workingCopy.refresh()
            self?.reloadWorktrees()
        }
        slot.actions.onFetch = { [weak self] in self?.updateCommitGraph() }
        slot.actions.onUndoChange = { [weak self, weak slot] label in
            guard let self, slot === editing else { return }
            undoLabel = label
            toolbar.setUndo(label)
        }
    }

    // MARK: - worktree

    /// worktree ごとの変更の数（サイドバーに出す）。
    private var worktreeChanges: [String: Int] = [:]

    /// worktree の一覧を読み直す（増えた・消えた・ブランチが変わった）。変更の数は手元のものだけ裏で数える。
    private func reloadWorktrees() {
        let repository = self.repository
        Task {
            guard let list = try? await Task.detached(priority: .utility, operation: {
                try repository.info(["op": "worktrees"], as: WorktreeList.self)
            }).value.worktrees else { return }
            if list != worktrees {
                worktrees = list
                // 書き込む先が消えたら、開いたリポジトリに戻す。
                if editing !== opened, !list.contains(where: { $0.isSame(editing.path) }) {
                    activate(opened)
                }
                showWorktrees()
            }
        }
    }

    private func showWorktrees() {
        sidebar.show(worktrees: worktrees, editing: editing.path)
        actionsWorktreeName()
        updateEditingBranch()
        reloadOtherWorktrees()
    }

    /// 確認のシートに worktree の名前を出す（worktree が 2 つ以上あるときだけ）。
    private func actionsWorktreeName() {
        let name = worktrees.count > 1 ? worktrees.first { $0.isSame(editing.path) }?.name : nil
        editing.actions.worktreeName = name
        stashes.worktreeName = name
        reflog.worktreeName = name
    }

    /// `path` の worktree を書き込む先にする。まだ開いていなければ開く（SSH 先なら同じ接続先の上のパス）。
    func edit(worktree path: String) {
        if WorktreeInfo.canonical(path) == WorktreeInfo.canonical(editing.path) { return }
        Task {
            guard let slot = await slot(for: path) else { return }
            activate(slot)
        }
    }

    /// `path` の worktree の組（なければ開く。開いただけでは書き込む先にしない）。
    private func slot(for path: String) async -> WorktreeSlot? {
        let key = WorktreeInfo.canonical(path)
        if key == WorktreeInfo.canonical(opened.path) { return opened }
        if let slot = slots[key] { return slot }
        // 同じ worktree を開いている途中なら、それを待つ（重ねて開かない）。
        if let opening = openingSlots[key] { return await opening.value }
        let task = Task { await openSlot(path) }
        openingSlots[key] = task
        let slot = await task.value
        openingSlots[key] = nil
        return slot
    }

    /// 開いている途中の worktree（パスで引く）。
    private var openingSlots: [String: Task<WorktreeSlot?, Never>] = [:]

    private func openSlot(_ path: String) async -> WorktreeSlot? {
        let started = CACurrentMediaTime()
        defer { PerformanceLog.record("Worktrees: open one", ms: (CACurrentMediaTime() - started) * 1000) }
        let location = RepositoryLocation.isRemote(self.location) ? Self.replacingPath(of: self.location, with: path) : path
        let daemon = Daemon.path, daemons = Daemon.bundledDirectory
        do {
            let repository = try await Task.detached(priority: .userInitiated) {
                try Self.openRepository(location: location, daemon: daemon, daemons: daemons)
            }.value
            // 開いている間にほかから開いていたら、そちらを使う。
            if let slot = slots[WorktreeInfo.canonical(path)] { return slot }
            let slot = WorktreeSlot(path: path, repository: repository, renderer: textRenderer)
            wire(slot)
            slots[WorktreeInfo.canonical(path)] = slot
            slot.watch = try? RepositoryWatch(repository: repository) { [weak self, weak slot] changes in
                Task { @MainActor in
                    guard let self, let slot, !changes.intersection([.refs, .index, .worktree]).isEmpty else { return }
                    // ref は全部の worktree で共通なので、どの worktree の知らせも 1 回の読み直しにまとめる。
                    if changes.contains(.refs) { self.refsChanged() }
                    // 変わった worktree だけを読み直す（全部は読み直さない）。
                    if slot === self.editing { slot.workingCopy.refresh() } else { self.reloadOtherWorktrees(after: 1) }
                }
            }
            return slot
        } catch {
            showFailure("\(error)", title: L("Can’t open the worktree"))
            return nil
        }
    }

    private var refsWork: DispatchWorkItem?

    /// ref（と worktree の HEAD）が変わった。worktree ごとの見張りが一斉に知らせるので、少し待って 1 回だけ
    /// History・ref・worktree の一覧を読み直す。
    private func refsChanged() {
        refsWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.refsWork = nil
                self.history.refresh()
                self.reloadRefs()
                self.reloadWorktrees()
            }
        }
        refsWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
    }

    /// ほかの worktree の status の指紋（変わっていなければ画面を変えない）。
    private var worktreeStamps: [String: UInt64] = [:]
    private var countsWork: DispatchWorkItem?
    private var countsRunning = false
    private var countsAgain = false
    /// ほかの worktree は開かないので変化の知らせが来ない。前にある間は 10 秒ごとに数を頼む（エンジンが変化を
    /// 見張っていて、変わったものだけを数え直すので軽い）。
    private var countsTimer: Timer?
    /// 最後に全部を数え直した時刻。エンジンが作業ツリーを見張るのは変更のある worktree だけなので、変更のない
    /// worktree が書き換えられたのは、全部を数え直すまで分からない（5 分ごと）。
    private var lastFreshCount = Date.distantPast
    private var countsFresh = false

    /// ほかの worktree の変更の数を読み直す（サイドバーの Working Copy の下と、History の札に出す）。開いた
    /// リポジトリに 1 回で頼み、worktree ごとには開かない（SSH 先でも 1 本のつながりで済む）。続けて呼ばれたら、
    /// `delay` 待ってまとめる。
    private func reloadOtherWorktrees(after delay: Double = 0.3, fresh: Bool = false) {
        countsFresh = countsFresh || fresh
        countsWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated { self?.readWorktreeCounts() }
        }
        countsWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
        if countsTimer == nil {
            countsTimer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, NSApp.isActive, self.window.isVisible, !self.isBackgroundTab else { return }
                    self.reloadOtherWorktrees(after: 0)
                }
            }
        }
    }

    private func readWorktreeCounts() {
        guard !countsRunning else {
            countsAgain = true
            return
        }
        let paths = worktrees.filter { !$0.isSame(editing.path) && $0.prunable == nil && !$0.bare }.map(\.path)
        guard worktrees.count > 1, !paths.isEmpty else { return showWorktreeCounts() }
        countsRunning = true
        let fresh = countsFresh || Date().timeIntervalSince(lastFreshCount) > 300
        countsFresh = false
        if fresh { lastFreshCount = Date() }
        let repository = opened.repository
        let started = CACurrentMediaTime()
        Task {
            let result = try? await Task.detached(priority: .utility) {
                try repository.info(["op": "worktreeChanges", "paths": paths, "fresh": fresh], as: WorktreeChangesList.self)
            }.value
            PerformanceLog.record(fresh ? "Worktrees: count all the others' changes" : "Worktrees: count the others' changes",
                                  ms: (CACurrentMediaTime() - started) * 1000)
            countsRunning = false
            var changed = false
            for w in result?.worktrees ?? [] {
                let key = WorktreeInfo.canonical(w.path)
                guard worktreeStamps[key] != w.stamp || worktreeChanges[key] == nil else { continue }
                worktreeStamps[key] = w.stamp
                worktreeChanges[key] = w.count ?? 0
                changed = true
            }
            if changed { showWorktreeCounts() }
            if countsAgain {
                countsAgain = false
                reloadOtherWorktrees()
            }
        }
    }

    private func showWorktreeCounts() {
        sidebar.setWorktreeChanges(worktreeChanges)
        showWorktreeHeads()
    }

    /// History に、各 worktree の HEAD の札（✎ は書き込む先、数は変更のあるファイル）。
    private func showWorktreeHeads() {
        guard worktrees.count > 1 else { return history.show(worktreeHeads: []) }
        history.show(worktreeHeads: worktrees.compactMap { w in
            guard let hex = w.head, let head = Data(hex: hex) else { return nil }
            let editingThis = w.isSame(editing.path)
            let changes = worktreeChanges[WorktreeInfo.canonical(w.path)] ?? 0
            // ✎ 書き込む先、⌂ メインの作業ツリー、⑂ 追加した worktree（サイドバーと同じ印）。
            let mark = editingThis ? "✎ " : w.main ? "⌂ " : "⑂ "
            let name = mark + w.name + (changes > 0 ? " · \(changes)" : "")
            return (head, name, editingThis)
        })
    }

    /// 書き込む先にした worktree の順（新しいものが後ろ）。開いたままにするのは、開いたリポジトリと最近の
    /// `keptSlots` 個まで（SSH 先では 1 つごとにつながりとエンジンを使う。戻ったら開き直す）。
    private var slotOrder: [String] = []
    private static let keptSlots = 3

    private func activate(_ slot: WorktreeSlot) {
        editing = slot
        if slot !== opened {
            let key = WorktreeInfo.canonical(slot.path)
            slotOrder.removeAll { $0 == key }
            slotOrder.append(key)
            while slotOrder.count > Self.keptSlots {
                slots[slotOrder.removeFirst()] = nil
            }
        }
        pages?.replace(.workingCopy, with: slot.workingCopy)
        sidebar.actions = slot.actions
        history.actions = slot.actions
        stashes.actions = slot.actions
        stashes.reloadBases()
        reflog.actions = slot.actions
        slot.actions.refreshUndo()
        slot.workingCopy.refresh()
        changeCount = worktreeChanges[WorktreeInfo.canonical(slot.path)] ?? 0
        showWorktrees()
        updateTabTitle()
    }

    /// ツールバーに、書き込む先の worktree とそのブランチを出す。History とサイドバーの「今のブランチ」も
    /// 書き込む先のものにし、ほかの worktree が checkout しているブランチに印を付ける。
    private func updateEditingBranch() {
        let info = worktrees.first { $0.isSame(editing.path) }
        let multiple = worktrees.count > 1
        history.setWorktreeBranches(
            editing: multiple ? .some(info?.branch) : nil,
            elsewhere: multiple ? Set(worktrees.filter { !$0.isSame(editing.path) && !$0.main }.compactMap(\.branch)) : [])
        if multiple, let hex = info?.head, let head = Data(hex: hex) { history.head = head }
        let other = editing !== opened || (info.map { !$0.main } ?? false)
        toolbar.setRepository(window.title, worktree: other ? info?.name ?? (editing.path as NSString).lastPathComponent : nil)
        // 書き込む先のブランチ（開いたリポジトリに戻ったときも、そのブランチに戻す）。
        let branch = editing === opened ? latestRefs?.headBranch : info?.branch
        toolbar.setBranch(branch)
        stashes.currentBranch = branch
        editing.actions.currentBranch = branch
        let current = latestRefs?.items.first { $0.kind == .local && $0.name == branch }
        toolbar.setTracking(ahead: current?.ahead ?? 0, behind: current?.behind ?? 0, published: current.map(Self.isPublished) ?? true)
        workingCopy.publishesBranch = !(current.map(Self.isPublished) ?? true)
        if editing === opened, let head = latestRefs?.head { history.head = head }
    }

    /// サイドバーの worktree の右クリック。
    /// サイドバーのリモートの操作。追加と編集はリポジトリの設定と同じシート、削除は確かめてから。
    private func remoteAction(_ name: String?, _ action: SidebarViewController.RemoteAction) {
        let window = self.window
        switch action {
        case .add:
            RemoteSheet.present(in: window, repository: repository, remote: nil) { [weak self] in self?.actions.perform(sequence: $0) }
        case .edit:
            guard let name else { return }
            let repository = self.repository
            Task {
                nonisolated struct Remotes: Decodable { let remotes: [RemoteEntry] }
                let remotes = (try? await Task.detached { try repository.info(["op": "remotes"], as: Remotes.self) }.value)?.remotes ?? []
                guard let remote = remotes.first(where: { $0.name == name }) else { return }
                RemoteSheet.present(in: window, repository: repository, remote: remote) { [weak self] in self?.actions.perform(sequence: $0) }
            }
        case .remove:
            guard let name, RemoteSheet.confirmRemove(name) else { return }
            actions.perform(["op": "changeRemote", "change": ["kind": "remove", "name": name]])
        }
    }

    /// Repository > Add Remote…（サイドバーと設定のページと同じシート）。
    @objc func addRemote(_ sender: Any?) {
        remoteAction(nil, .add)
    }

    private func worktreeAction(_ path: String, _ action: SidebarViewController.WorktreeAction) {
        guard let info = worktrees.first(where: { $0.isSame(path) }) else { return }
        switch action {
        case .reveal:
            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
        case .terminal:
            openTerminal(at: path)
        case .remove:
            removeWorktree(info)
        case .prune:
            opened.actions.perform(["op": "worktreePrune"])
        }
    }

    /// Repository > New Worktree…: ブランチを作り、それを checkout した worktree を作って、書き込む先にする。
    @objc func newWorktree(_ sender: Any?) {
        let start = startPoint
        let base = opened.path
        let alert = NSAlert()
        alert.messageText = L("New Worktree")
        alert.informativeText = L("Creates a branch from %@ and checks it out in a new folder.", start)
        let branch = NSTextField(string: "")
        branch.placeholderString = L("Branch name")
        let folder = NSTextField(string: "")
        folder.placeholderString = L("Folder")
        for field in [branch, folder] { field.frame.size = NSSize(width: 360, height: 24) }
        let stack = NSStackView(views: [branch, folder])
        stack.orientation = .vertical
        stack.spacing = 8
        stack.frame = NSRect(x: 0, y: 0, width: 360, height: 56)
        alert.accessoryView = stack
        alert.window.initialFirstResponder = branch
        // フォルダの既定は、リポジトリの隣の「<名前>-<ブランチ>」（git の worktree でよくある置き方）。
        let parent = (base as NSString).deletingLastPathComponent, repoName = (base as NSString).lastPathComponent
        let target = ClosureTarget {
            let name = branch.stringValue.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "/", with: "-")
            folder.placeholderString = name.isEmpty ? L("Folder")
                : (parent as NSString).appendingPathComponent("\(repoName)-\(name)")
        }
        NotificationCenter.default.addObserver(forName: NSControl.textDidChangeNotification, object: branch, queue: .main) { _ in
            MainActor.assumeIsolated { target.fire() }
        }
        alert.addButton(withTitle: L("Create"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { [weak self] response in
            withExtendedLifetime(target) {}
            guard let self, response == .alertFirstButtonReturn else { return }
            let name = branch.stringValue.trimmingCharacters(in: .whitespaces)
            guard !name.isEmpty else { return NSSound.beep() }
            var path = folder.stringValue.trimmingCharacters(in: .whitespaces)
            if path.isEmpty { path = folder.placeholderString ?? "" }
            path = (path as NSString).expandingTildeInPath
            opened.actions.perform(["op": "worktreeAdd", "path": path, "newBranch": name, "start": start]) { [weak self] _ in
                self?.reloadWorktrees()
                self?.edit(worktree: path)
            }
        }
    }

    /// Repository > Prune Worktrees: フォルダがなくなった worktree の記録を片付ける。
    @objc func pruneWorktrees(_ sender: Any?) {
        opened.actions.perform(["op": "worktreePrune"])
    }

    /// worktree を消す（変更が残っていれば、それも捨てると確かめてから --force）。開いたリポジトリは消せない。
    private func removeWorktree(_ info: WorktreeInfo) {
        guard !info.main, !info.isSame(opened.path) else { return NSSound.beep() }
        let changes = worktreeChanges[WorktreeInfo.canonical(info.path)] ?? 0
        let alert = NSAlert()
        alert.messageText = L("Remove the worktree “%@”?", info.name)
        alert.informativeText = changes > 0
            ? L("%@ has uncommitted changes in %ld files. They are lost with the folder. The branch stays.", info.path, changes)
            : L("Removes the folder %@. The branch stays.", info.path)
        let button = alert.addButton(withTitle: L("Remove Worktree"))
        button.hasDestructiveAction = true
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .alertFirstButtonReturn else { return }
            if info.isSame(editing.path) { activate(opened) }
            if let slot = slots.removeValue(forKey: WorktreeInfo.canonical(info.path)) { slot.watch = nil }
            opened.actions.perform(["op": "worktreeRemove", "path": info.path, "force": changes > 0 || info.locked != nil])
        }
    }

    /// ツールバーのリポジトリ名: リポジトリの一覧を出し、選んだものにこのタブを切り替える（⌘ を押していれば新しい
    /// ウィンドウ）。リポジトリのページと同じ一覧で、名前と状態だけを出す。
    @objc func showRepositoryList(_ sender: Any?) {
        if let popover = repositoryPopover, popover.isShown {
            popover.close()
            return
        }
        let browser = RepositoryBrowser(style: .compact)
        browser.onOpen = { [weak self] path, mode in
            self?.repositoryPopover?.close()
            self?.onSwitchRepository?(path, mode)
        }
        browser.onShowAll = { [weak self] in
            self?.repositoryPopover?.close()
            self?.onShowRepositories?()
        }
        let popover = NSPopover()
        popover.behavior = .transient
        popover.contentViewController = browser
        popover.contentSize = NSSize(width: 440, height: 520)
        let anchor = toolbar.repositoryAnchor ?? window.contentView!
        popover.show(relativeTo: anchor.bounds, of: anchor, preferredEdge: .minY)
        browser.prepare()
        repositoryPopover = popover
    }

    private func start(debug: Bool) {
        let window = self.window
        // --show-latency で、入力から表示までの時間をタイトルの下に出す（ふだんは今のブランチ）。
        if CommandLine.arguments.contains("--show-latency") {
            history.onLatency = { latency in
                window.subtitle = String(format: "入力→表示 %.1f ms", latency * 1000)
                PerformanceLog.record("History: input → display", ms: latency * 1000)
            }
        } else {
            history.onLatency = { latency in PerformanceLog.record("History: input → display", ms: latency * 1000) }
        }
        let repository = self.repository
        let started = Date()
        history.load { [weak self] error in
            self?.history.loadingMessage = nil
            if let error, self?.isDisconnected != true { NSAlert(error: error).runModal() }
            NSLog("History を最新にするまで %.0f ms", Date().timeIntervalSince(started) * 1000)
            if error == nil { self?.updateCommitGraph() }
        }
        NSLog("History を %.0f ms で表示（キャッシュ）", Date().timeIntervalSince(started) * 1000)
        actions.refreshUndo()
        sidebar.select(page: debug && CommandLine.arguments.contains("--working-copy") ? .workingCopy : .history)
        workingCopy.refresh()
        reloadRefs()
        reloadWorktrees()
        loadGitHub(force: true)
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
                history.loadingMessage = L("Loading History… %@ received", size)
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
        repository.setLocalSignatureCheck(Settings.sshHost(of: location).map(Settings.checkSignaturesLocally(on:)) ?? Settings.checkRemoteSignaturesLocally)
        updateSignaturePlace()
        history.forgetSignatures()
        sidebar.forgetSignatures()
        detail.reloadSignature()
    }

    /// 署名をどこで検証しているか（詳細の吹き出しに出す）。設定が効くのは SSH 先のリポジトリだけなので、
    /// 理由も添える。
    private func updateSignaturePlace() {
        let place = if let host = RepositoryLocation.host(location) {
            Settings.checkSignaturesLocally(on: Settings.sshHost(of: location) ?? host)
                ? SignaturePlace(local: true, detail: L("chosen in Settings > SSH Hosts"))
                : SignaturePlace(local: false, host: host, detail: L("chosen in Settings > SSH Hosts"))
        } else {
            SignaturePlace(local: true, detail: L("the repository is on this Mac"))
        }
        detail.signaturePlace = place
        sidebar.signaturePlace = place
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
            changeCount > 0 ? L("%ld changed files", changeCount) : nil,
            tracking.ahead > 0 ? L("%ld commits ahead of upstream", tracking.ahead) : nil,
            tracking.behind > 0 ? L("%ld commits behind upstream", tracking.behind) : nil,
        ].compactMap { $0 }.joined(separator: L(", "))
    }

    // MARK: - merge・rebase の途中の帯

    private var sequenceBanner: NSTitlebarAccessoryViewController?
    private let sequenceLabel = NSTextField(labelWithString: "")
    private let sequenceDetail = NSTextField(labelWithString: "")
    private let skipButton = NSButton(title: "Skip", target: nil, action: nil)
    private let continueButton = NSButton(title: "Continue", target: nil, action: nil)

    /// merge・rebase・cherry-pick・revert の途中なら、ツールバーの下に色の付いた帯を出す。1 行目に何をどこへして
    /// いる途中か（何番目か）、2 行目にどこで・なぜ止まっていて次に何をすればよいか。コンフリクトが残っていれば数と
    /// Working Copy へのリンク、なければ Continue を強調する。
    private func showSequenceBanner(_ sequence: SequenceInfo?, conflicts: Int) {
        guard let sequence else {
            if let banner = sequenceBanner,
               let index = window.titlebarAccessoryViewControllers.firstIndex(of: banner) {
                window.removeTitlebarAccessoryViewController(at: index)
            }
            sequenceBanner = nil
            return
        }
        sequenceLabel.stringValue = Self.sequenceTitle(sequence)
        sequenceDetail.stringValue = Self.sequenceDetail(sequence, conflicts: conflicts)
        sequenceDetail.toolTip = sequenceDetail.stringValue
        skipButton.isHidden = !["rebase", "cherry-pick"].contains(sequence.kind)
        continueButton.keyEquivalent = conflicts == 0 ? "\r" : ""
        guard sequenceBanner == nil else { return }
        let icon = NSImageView(image: NSImage(systemSymbolName: "arrow.triangle.merge", accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = .systemOrange
        icon.symbolConfiguration = .init(pointSize: 18, weight: .regular)
        sequenceLabel.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        sequenceLabel.lineBreakMode = .byTruncatingTail
        sequenceDetail.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        sequenceDetail.lineBreakMode = .byTruncatingTail
        for label in [sequenceLabel, sequenceDetail] {
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }
        let texts = NSStackView(views: [sequenceLabel, sequenceDetail])
        texts.orientation = .vertical
        texts.alignment = .leading
        texts.spacing = 1
        let show = NSButton(title: L("Open Working Copy"), target: self, action: #selector(showWorkingCopy(_:)))
        continueButton.target = self
        continueButton.action = #selector(continueSequence(_:))
        skipButton.target = self
        skipButton.action = #selector(skipSequence(_:))
        let abort = NSButton(title: "Abort", target: self, action: #selector(abortSequence(_:)))
        let stack = NSStackView(views: [icon, texts, NSView(), show, continueButton, skipButton, abort])
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 6, left: 12, bottom: 6, right: 12)
        stack.frame = NSRect(x: 0, y: 0, width: 600, height: 46)
        // 細い帯だと気づきにくいので、色を敷く（作業の途中であることを、どの画面でも見落とさないように）。
        stack.wantsLayer = true
        stack.layer?.backgroundColor = NSColor.systemOrange.withAlphaComponent(0.14).cgColor
        let bar = NSTitlebarAccessoryViewController()
        bar.view = stack
        bar.layoutAttribute = .bottom
        window.addTitlebarAccessoryViewController(bar)
        sequenceBanner = bar
    }

    /// 帯の 1 行目: 何をどこへしている途中か。
    private static func sequenceTitle(_ s: SequenceInfo) -> String {
        let title = switch (s.kind, s.branch, s.onto ?? s.from) {
        case let ("rebase", branch?, onto?): L("Rebasing %@ onto %@", branch, onto)
        case let ("merge", branch?, from?): L("Merging %@ into %@", from, branch)
        case let ("cherry-pick", branch?, _): L("Cherry-picking into %@", branch)
        case let ("revert", branch?, _): L("Reverting in %@", branch)
        case ("rebase", _, _): L("Rebase in progress")
        case ("merge", _, _): L("Merge in progress")
        case ("cherry-pick", _, _): L("Cherry-pick in progress")
        default: L("Revert in progress")
        }
        guard let current = s.current, let total = s.total else { return title }
        return title + " · " + L("step %ld of %ld", current, total)
    }

    /// 帯の 2 行目: どこで・なぜ止まっていて、次に何をすればよいか。
    private static func sequenceDetail(_ s: SequenceInfo, conflicts: Int) -> String {
        let subject = s.step?.subject.map { "“\($0)”" }
        if s.step?.action == "exec", conflicts == 0 {
            return L("The command %@ failed. Fix the problem, then Continue.", subject ?? "")
        }
        if conflicts > 0 {
            return subject.map { L("%ld files have conflicts at %@. Resolve them in Working Copy, then Continue.", conflicts, $0) }
                ?? L("%ld files have conflicts. Resolve them in Working Copy, then Continue.", conflicts)
        }
        if s.editing == true {
            return L("Stopped to edit %@. Change files or amend the commit, then Continue.", subject ?? "")
        }
        return L("Conflicts resolved. Continue to finish, or Abort to go back to where you started.")
    }

    // MARK: - 失敗の帯

    private var failureBanner: NSTitlebarAccessoryViewController?
    private var failure: OperationFailure?

    /// 操作の失敗を、ツールバーの下の帯で知らせる（作業を止めない）。閉じるか、次の失敗で置き換えるまで出す。
    func showFailure(_ message: String, title: String) {
        showFailure(OperationFailure(message: message), title: title)
    }

    /// 帯には、何が失敗したか（見出し）、なぜか（要点）、次にできること（ボタン）を出す。git の全文は「詳細」と
    /// ツールチップ。
    func showFailure(_ failure: OperationFailure, title: String? = nil) {
        hideFailure()
        self.failure = failure
        let cancelled = CoreLanguage.isCancelled(failure.message)
        let icon = NSImageView(image: NSImage(
            systemSymbolName: cancelled ? "stop.circle" : "xmark.octagon.fill", accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = cancelled ? .secondaryLabelColor : .systemRed
        let label = NSTextField(labelWithString: cancelled ? L("Operation Cancelled") : title ?? failure.title)
        label.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
        let detail = NSTextField(labelWithString: cancelled ? "" : failure.summary)
        detail.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        detail.lineBreakMode = .byTruncatingTail
        detail.toolTip = failure.message
        detail.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        // 次の一手（いちばん目立たせるのは最初のもの）。
        let remedies: [NSButton] = cancelled ? [] : failure.remedies.enumerated().map { i, remedy in
            let button = NSButton(title: remedy.title, target: self, action: #selector(runRemedy(_:)))
            button.tag = i
            if i == 0 { button.bezelColor = .controlAccentColor }
            return button
        }
        let more = NSButton(title: L("Details"), target: self, action: #selector(showFailureDetail))
        // AppDelegate の Window > Activity（target が nil なので、レスポンダチェーンで届く）。
        let activity = NSButton(title: "Activity", target: nil, action: #selector(AppDelegate.showActivity(_:)))
        let close = NSButton(title: L("Close"), target: self, action: #selector(hideFailure))
        more.isHidden = cancelled
        for b in remedies + [more, activity, close] { b.controlSize = .small }
        let stack = NSStackView(views: [icon, label, detail, NSView()] + remedies + [more, activity, close])
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 4, right: 12)
        stack.frame = NSRect(x: 0, y: 0, width: 600, height: 30)
        let bar = NSTitlebarAccessoryViewController()
        bar.view = stack
        bar.layoutAttribute = .bottom
        window.addTitlebarAccessoryViewController(bar)
        failureBanner = bar
        // 取り消したことは、しばらくしたら消す。
        if cancelled {
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self, weak bar] in
                if let bar, self?.failureBanner === bar { self?.hideFailure() }
            }
        }
    }

    @objc private func hideFailure() {
        guard let banner = failureBanner, let index = window.titlebarAccessoryViewControllers.firstIndex(of: banner) else { return }
        window.removeTitlebarAccessoryViewController(at: index)
        failureBanner = nil
    }

    @objc private func runRemedy(_ sender: NSButton) {
        guard let failure, failure.remedies.indices.contains(sender.tag) else { return }
        hideFailure()
        switch failure.remedies[sender.tag] {
        case let .openWorktree(path): edit(worktree: path)
        case .openTerminal: openTerminal(at: editing.path)
        case .retry: if let command = failure.command { actions.perform(command) }
        case .pull: pull(nil)
        case .workingCopy: showWorkingCopy(nil)
        case .setUpstream: setUpstream(nil)
        }
    }

    @objc private func showFailureDetail() {
        guard let failure else { return }
        let alert = NSAlert()
        alert.messageText = failure.title
        // 要点と、どうすればよいか、git の全文。
        alert.informativeText = [failure.kind == nil ? nil : failure.summary, failure.advice, failure.message]
            .compactMap { $0 }.joined(separator: "\n\n")
        alert.beginSheetModal(for: window)
    }

    /// ツールバーの下に、切れたことと「つなぎ直す」を出す。
    private func showDisconnected(reason: String) {
        let icon = NSImageView(image: NSImage(systemSymbolName: "bolt.horizontal.circle", accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = .systemOrange
        let host = RepositoryLocation.host(location) ?? location
        let label = NSTextField(labelWithString: L("Lost connection to %@", host))
        label.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
        let detail = NSTextField(labelWithString: reason)
        detail.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        detail.textColor = .secondaryLabelColor
        detail.lineBreakMode = .byTruncatingTail
        detail.toolTip = reason
        detail.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let button = NSButton(title: L("Reconnect"), target: self, action: #selector(reconnect))
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
        // --stashes で、Stashes の画面を開く（stash は ref を読んだ後に出るので、少し待つ）。
        if CommandLine.arguments.contains("--stashes") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [self] in sidebar.select(page: .stashes) }
        }
        // --reflog で、Reflog の画面を開く。
        if CommandLine.arguments.contains("--reflog") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [self] in sidebar.select(page: .reflog) }
        }
        // --settings で、リポジトリの設定の画面を開く（--settings-page NAME で、そのページ。見た目の確認用）。
        if CommandLine.arguments.contains("--settings") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [self] in sidebar.select(page: .settings) }
        }
        // --working-copy で、Working Copy を開く（見た目の確認用）。
        if CommandLine.arguments.contains("--working-copy") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [self] in sidebar.select(page: .workingCopy) }
        }
        // --remote-sheet [NAME] で、リモートの追加（名前を付ければ編集）のシートを開く（見た目の確認用）。
        if CommandLine.arguments.contains("--remote-sheet") {
            let name = AppDelegate.option("--remote-sheet").flatMap { $0.hasPrefix("-") ? nil : $0 }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [self] in remoteAction(name, name == nil ? .add : .edit) }
        }
        // --app-settings で、アプリの Settings ウィンドウ（--app-settings-tab NAME でそのタブ。名前は英語の見出し）を開く
        // （見た目の確認用。--snapshot なら、そのウィンドウを撮る）。
        let appSettings = CommandLine.arguments.contains("--app-settings")
        if appSettings {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                SettingsWindowController.shared.show(tab: AppDelegate.option("--app-settings-tab").map { L($0) })
            }
        }
        // --blame PATH・--file-history PATH で、そのファイルの blame・履歴を開く（見た目の確認用）。
        if let path = AppDelegate.option("--blame") { openBlame(path: path, rev: nil) }
        if let path = AppDelegate.option("--file-history") { openFileHistory(path: path) }
        // --measure-select N で、History の行を N 個順に選び、詳細が出るまでの各段階の時間を測る。
        if let count = AppDelegate.option("--measure-select").flatMap(Int.init) {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [self] in measureSelection(count: count) }
        }
        // --filter-sidebar WORDS で、サイドバーを絞り込んだ状態にする（見出しが開くので、タグの行も見える）。
        if let words = AppDelegate.option("--filter-sidebar") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [self] in sidebar.filter(words) }
        }
        // --select-first・--select-row N で、その行を選んで表の中ほどに出す（履歴の途中の見た目を確かめる）。--snapshot
        // がなくても効く（本物のウィンドウを画面キャプチャで撮るとき）。
        let selectFirst = CommandLine.arguments.contains("--select-first")
        let selectRow = AppDelegate.option("--select-row").flatMap(Int.init) ?? (selectFirst ? 0 : nil)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            if let selectRow { history.select(row: selectRow) }
        }
        if let output = AppDelegate.option("--snapshot") {
            let wait = selectRow != nil || CommandLine.arguments.contains("--working-copy") || CommandLine.arguments.contains("--stashes")
                || CommandLine.arguments.contains("--reflog") || CommandLine.arguments.contains("--settings")
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
            // --snapshot-after SECONDS で待ちを決める（SSH 先につながるのを待つなど）。
            let delay = AppDelegate.option("--snapshot-after").flatMap(Double.init)
                ?? (query != nil ? 3.5 : wait || mergePath != nil || appSettings ? 1.5 : 0.5)
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [self] in
                let rep = appSettings ? SettingsWindowController.shared.snapshot()
                    : mergePath != nil
                    ? MergeEditorWindowController.snapshot()
                    : history.snapshot(of: window.contentView!, extra: detail.overlays() + workingCopy.overlays() + stashes.detail.overlays() + reflog.detail.overlays())
                try? rep?.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: output))
                NSApp.terminate(nil)
            }
        }
    }

    /// 詳細の欄で押した ref の履歴に絞り込み、ref が指すコミットを選ぶ（サイドバーで選んだときと同じ）。
    private func selectRef(named name: String) {
        guard let item = latestRefs?.items.first(where: { $0.name == name && [.local, .remote, .tag].contains($0.kind) }),
              let target = item.target else { return NSSound.beep() }
        history.set(scope: .ref(item.kind, item.name, target), select: target)
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
        loadGitHub(force: true)
    }

    /// 裏のタブで溜めた変化（前に出たときに読み直す）。
    private var pendingChanges = Changes(rawValue: 0)

    /// タブのまとまりの中で、前に出ていないタブか。
    private var isBackgroundTab: Bool {
        guard let group = window.tabGroup, group.windows.count > 1 else { return false }
        return group.selectedWindow !== window
    }

    func windowDidResignKey(_ notification: Notification) {
        // タブを切り替えたら、裏になった History のメモリを絞る（タブの選択が変わってから確かめる）。
        DispatchQueue.main.async { [weak self] in
            guard let self, isBackgroundTab else { return }
            let dropped = history.shrinkForBackground()
            if dropped > 0 { EventLog.add(repo: window.title, L("Background tab: dropped %ld History chunks", dropped)) }
        }
    }

    func windowDidBecomeKey(_ notification: Notification) {
        // ほかの worktree の変化は知らせが来ないので、戻ってきたら数え直す。
        if worktrees.count > 1 { reloadOtherWorktrees(after: 0.5) }
        guard pendingChanges.rawValue != 0 else { return }
        let changes = pendingChanges
        pendingChanges = Changes(rawValue: 0)
        repositoryDidChange(changes)
    }

    private func repositoryDidChange(_ changes: Changes) {
        let names = [(Changes.refs, "ref"), (.index, "index"), (.worktree, L("working tree")), (.config, L("config"))]
            .filter { changes.contains($0.0) }.map(\.1)
        EventLog.add(
            repo: window.title,
            L("Changed: %@", names.joined(separator: L(", "))) + (isBackgroundTab && !changes.isDisjoint(with: [.index, .worktree]) ? L(" (background tab: the working tree waits until the tab comes to the front)") : ""))
        // 裏のタブでは、作業ツリーの変化（ビルドなどで頻繁に来る）の読み直しを前に出るまで待つ。ref の変化は
        // タブの名前（ahead・behind）に出すので、すぐ読む。
        if isBackgroundTab, !changes.intersection([.index, .worktree]).isEmpty {
            pendingChanges.formUnion(changes.intersection([.index, .worktree]))
            let rest = changes.subtracting([.index, .worktree])
            guard rest.rawValue != 0 else { return }
            return repositoryDidChange(rest)
        }
        if !changes.intersection([.refs, .config]).isEmpty {
            refsChanged()
            // push・fetch の後はプルリクエストも変わりうる（読みすぎないよう、間を空ける）。
            loadGitHub(force: false)
        }
        if !changes.intersection([.refs, .index, .worktree]).isEmpty {
            // 開いたリポジトリの変化。書き込む先がほかの worktree なら、一覧の見出しの分だけ読み直す。
            if editing === opened { workingCopy.refresh() } else { reloadOtherWorktrees(after: 1) }
        }
    }

    private func reloadRefs() {
        let repository = self.repository
        Task {
            do {
                let refs = try await Task.detached(priority: .userInitiated) { try repository.loadRefs() }.value
                latestRefs = refs
                sidebar.show(refs)
                stashes.show(refs.items)
                stashes.reloadBases()
                reflog.reload()
                // 最後の stash を戻した・消したら、空の Stashes の画面から History に移る。
                if pages?.page == .stashes, !refs.items.contains(where: { $0.kind == .stash }) {
                    sidebar.select(page: .history)
                }
                reloadBranchGroups()
                reloadTagRemotes()
                detail.reloadInfo()
                updateSubtitle()
                history.head = refs.head
                history.show(refs: refs)
                let current = refs.items.first { $0.kind == .local && $0.name == refs.headBranch }
                toolbar.setTracking(ahead: current?.ahead ?? 0, behind: current?.behind ?? 0,
                                    published: current.map(Self.isPublished) ?? true)
                workingCopy.publishesBranch = !(current.map(Self.isPublished) ?? true)
                toolbar.setBranch(refs.headBranch)
                if let branch = refs.headBranch { rememberBranch(branch) }
                tracking = (current?.ahead ?? 0, current?.behind ?? 0)
                updateEditingBranch()
                updateTabTitle()
            } catch {
                NSLog("ref を読めない: \(error)")
            }
        }
    }

    /// ローカルブランチの分け方を裏で求め、サイドバーに出す（History のグラフから求める）。
    /// タグがどのリモートにあるか（fetch のときにコアが読んで覚えたもの）を読み直す。
    private func reloadTagRemotes() {
        let repository = self.repository
        Task {
            guard let remotes = await Task.detached(priority: .utility, operation: {
                try? repository.info(["op": "tagRemotes"], as: TagRemotes.self)
            }).value else { return }
            sidebar.show(tagRemotes: remotes)
        }
    }

    private func reloadBranchGroups() {
        let repository = self.repository
        Task {
            let started = CACurrentMediaTime()
            guard let groups = await Task.detached(priority: .utility, operation: {
                try? repository.info(["op": "branchGroups"], as: BranchGroups.self)
            }).value else { return }
            PerformanceLog.record("Sidebar: branch groups", ms: (CACurrentMediaTime() - started) * 1000)
            sidebar.show(branchGroups: groups)
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

    /// 作業ツリーの内容（`rev` なし）とファイルの履歴の起点（HEAD）は worktree ごとに違うので、`repository`（なければ
    /// 書き込む先の worktree）で読む。開いたリポジトリで読むと、ほかの worktree の作業ツリー・ブランチを見てしまう。
    private func openBlame(path: String, rev: String?, in repository: Repository? = nil) {
        let repository = repository ?? editing.repository
        let tool = BlameViewController(path: path, rev: rev, repository: repository) { [weak self] id in
            self?.reveal(commit: id)
        }
        showFileTool(tool, key: "blame \(path) \(rev ?? "") \(ObjectIdentifier(repository).hashValue)")
    }

    private func openFileHistory(path: String, in repository: Repository? = nil) {
        let repository = repository ?? editing.repository
        let tool = FileHistoryViewController(
            path: path, repository: repository, renderer: textRenderer,
            onSelectCommit: { [weak self] id in self?.reveal(commit: id) },
            onBlame: { [weak self] path, commit in self?.openBlame(path: path, rev: commit, in: repository) })
        showFileTool(tool, key: "fileHistory \(path) \(ObjectIdentifier(repository).hashValue)")
    }

    /// File History・Blame を、このリポジトリのタブの隣のタブで開く。同じものがもう開いていれば、そのタブを読み直した
    /// ものに入れ替える。
    private func showFileTool(_ tool: FileToolViewController, key: String) {
        FileToolWindowController.show(tool, key: key, tabWith: window, toolbar: mirrorToolbar(), repository: self)
    }

    /// File History・Blame のタブに付ける、このタブと同じツールバー。検索はこのタブに戻って History で探す。
    private func mirrorToolbar() -> MainToolbar {
        let mirror = toolbar.mirror()
        mirror.onSearch = { [weak self] query in
            self?.window.makeKeyAndOrderFront(nil)
            self?.toolbar.onSearch?(query)
        }
        mirror.onSearchStep = { [weak self] step in
            self?.window.makeKeyAndOrderFront(nil)
            self?.toolbar.onSearchStep?(step)
        }
        return mirror
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

    @objc func rebuildHistory(_ sender: Any?) {
        history.rebuild { [weak self] error in
            if let error { self?.showFailure(error.localizedDescription, title: L("Couldn’t Rebuild History")) }
        }
    }

    @objc func fetch(_ sender: Any?) { actions.fetch() }
    @objc func pull(_ sender: Any?) { pull(mode: .git) }
    @objc func pullGit(_ sender: Any?) { pull(mode: .git) }
    @objc func pullMerge(_ sender: Any?) { pull(mode: .merge) }
    @objc func pullRebase(_ sender: Any?) { pull(mode: .rebase) }
    @objc func pullFastForward(_ sender: Any?) { pull(mode: .fastForwardOnly) }

    /// Pull・Push・Pop は、すぐには実行しないで何が起きるかをシートで見せる（`SyncSheets.swift`）。
    private func pull(mode: PullMode) { actions.pullWithConfirmation(mode: mode) }

    @objc func push(_ sender: Any?) { actions.pushWithConfirmation() }
    @objc func forcePush(_ sender: Any?) { actions.pushWithConfirmation(force: true) }

    @objc func stashChanges(_ sender: Any?) { actions.stash() }

    /// 最新の stash（`stash@{0}`）を戻して消す。
    @objc func popStash(_ sender: Any?) {
        guard let stash = latestRefs?.items.first(where: { $0.kind == .stash }) else { return NSSound.beep() }
        actions.popWithConfirmation(name: stash.name, message: stash.detail)
    }

    private var latestStash: String? {
        latestRefs?.items.first { $0.kind == .stash }?.name
    }

    @objc func mergeIntoCurrent(_ sender: Any?) {
        let current = latestRefs?.headBranch ?? "HEAD"
        Task { @MainActor in
            guard let rev = await chooseRevision(
                title: L("Merge into %@", current), message: L("The branch, tag or commit to merge in"), ok: "Merge",
                forecast: .merge) else { return }
            actions.perform(["op": "merge", "rev": rev])
        }
    }

    @objc func rebaseCurrent(_ sender: Any?) {
        let current = latestRefs?.headBranch ?? "HEAD"
        Task { @MainActor in
            guard let rev = await chooseRevision(
                title: L("Rebase %@", current), message: L("The branch, tag or commit to rebase onto"), ok: "Rebase",
                forecast: .rebase) else { return }
            actions.perform(["op": "rebase", "onto": rev])
        }
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
    @objc func undoOperation(_ sender: Any?) { actions.undoWithConfirmation() }

    @objc func showWorkingCopy(_ sender: Any?) { sidebar.select(page: .workingCopy) }
    @objc func showHistory(_ sender: Any?) { sidebar.select(page: .history) }
    @objc func showStashes(_ sender: Any?) { sidebar.select(page: .stashes) }
    @objc func showReflog(_ sender: Any?) { sidebar.select(page: .reflog) }

    /// Navigate > Filter Branches…: サイドバーの絞り込みの欄にカーソルを移す（サイドバーが畳んであれば開く）。
    @objc func filterBranches(_ sender: Any?) {
        if let split = sidebar.parent as? NSSplitViewController, let item = split.splitViewItem(for: sidebar), item.isCollapsed {
            item.animator().isCollapsed = false
        }
        sidebar.focusFilter()
    }

    /// タイトルの下のブランチ名。ツールバーにブランチのボタンが出ていれば、同じ名前を 2 か所に出さない。
    private func updateSubtitle() {
        guard !CommandLine.arguments.contains("--show-latency") else { return }
        let shown = window.toolbar?.isVisible == true && window.toolbar?.items.contains { $0.itemIdentifier == .branch } == true
        window.subtitle = shown ? "" : latestRefs?.headBranch ?? "detached HEAD"
    }

    /// ツールバーに検索欄が出ている。
    var toolbarHasSearch: Bool {
        window.toolbar?.isVisible == true && window.toolbar?.items.contains(where: { $0.itemIdentifier == .search }) == true
    }

    @objc func findInHistory(_ sender: Any?) {
        sidebar.select(page: .history)
        // ツールバーに検索欄があればそこへ、なければ History の検索欄へ。
        if toolbarHasSearch {
            toolbar.focusSearch()
        } else {
            history.showFindBar(sender)
        }
    }

    // MARK: - ブランチ

    /// 最近切り替えたブランチ（新しい順、10 個まで）。ブランチの切り替えで上に出す。
    private var recentBranchesKey: String { "recentBranches." + location }

    private func rememberBranch(_ branch: String) {
        var list = UserDefaults.standard.stringArray(forKey: recentBranchesKey) ?? []
        guard list.first != branch else { return }
        list.removeAll { $0 == branch }
        list.insert(branch, at: 0)
        UserDefaults.standard.set(Array(list.prefix(10)), forKey: recentBranchesKey)
    }

    /// Navigate > Switch Branch…（⌘⇧B）・ツールバーのブランチ。
    @objc func switchBranch(_ sender: Any?) {
        guard let refs = latestRefs else { return NSSound.beep() }
        let recent = UserDefaults.standard.stringArray(forKey: recentBranchesKey) ?? []
        let inUse = Dictionary(
            worktrees.filter { $0.branch != nil && !$0.isSame(editing.path) }.map { ($0.branch!, $0.name) },
            uniquingKeysWith: { a, _ in a })
        BranchSwitcherPanel.shared.show(refs: refs, recent: recent, inUse: inUse, over: window) { [weak self] choice in
            guard let self else { return }
            switch choice {
            case let .local(name):
                // ほかの worktree が checkout しているブランチは、git と同じく押し通さず、その worktree に移る。
                if let other = worktrees.first(where: { $0.branch == name && !$0.isSame(editing.path) }) {
                    edit(worktree: other.path)
                } else {
                    actions.perform(["op": "checkout", "branch": name])
                }
            case let .remote(name): actions.perform(["op": "track", "remoteBranch": name])
            case let .create(name): actions.perform(["op": "createBranch", "name": name, "start": "HEAD", "checkout": true])
            }
        }
    }

    /// Repository > Rename Branch…: 今のブランチの名前を変える。
    @objc func renameBranch(_ sender: Any?) {
        guard let branch = latestRefs?.headBranch,
              let to = actions.ask("Rename Branch", message: L("New name for %@", branch), initial: branch, ok: L("Rename")),
              to != branch else { return }
        actions.perform(["op": "renameBranch", "from": branch, "to": to])
    }

    /// Repository > Set Upstream…: 今のブランチの上流（pull・push の相手）を、リモートのブランチから選ぶ。
    @objc func setUpstream(_ sender: Any?) {
        guard let branch = latestRefs?.headBranch else { return NSSound.beep() }
        let remotes = (latestRefs?.items ?? []).filter { $0.kind == .remote && !$0.name.hasSuffix("/HEAD") }.map(\.name)
        guard !remotes.isEmpty else { return NSSound.beep() }
        let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 320, height: 26), pullsDown: false)
        popup.addItems(withTitles: remotes)
        if let same = remotes.first(where: { $0.hasSuffix("/" + branch) }) { popup.selectItem(withTitle: same) }
        let alert = NSAlert()
        alert.messageText = L("Upstream of %@", branch)
        alert.informativeText = L("The remote branch that Pull brings in from and Push sends to.")
        alert.addButton(withTitle: L("Set"))
        alert.addButton(withTitle: L("Cancel"))
        alert.accessoryView = popup
        guard alert.runModal() == .alertFirstButtonReturn, let upstream = popup.titleOfSelectedItem,
              let slash = upstream.firstIndex(of: "/") else { return }
        let remote = String(upstream[..<slash]), remoteBranch = String(upstream[upstream.index(after: slash)...])
        actions.perform(sequence: [
            ["op": "configSet", "key": "branch.\(branch).remote", "value": remote],
            ["op": "configSet", "key": "branch.\(branch).merge", "value": "refs/heads/\(remoteBranch)"],
        ])
    }

    @objc func goToHead(_ sender: Any?) {
        guard let head = latestRefs?.head else { return NSSound.beep() }
        sidebar.select(page: .history)
        goTo(id: head)
    }

    @objc func commitChanges(_ sender: Any?) { workingCopy.commitFromMenu() }
    @objc func commitAndPush(_ sender: Any?) { workingCopy.commitAndPushFromMenu() }
    @objc func stageAllChanges(_ sender: Any?) { workingCopy.stageAllFromMenu() }

    // MARK: - GitHub

    /// origin（なければ最初のリモート）が GitHub なら、そのリポジトリ。
    private var gitHub: GitHubRepository?
    private var gitHubLoadedAt: Date?

    /// GitHub のリポジトリを判定し、開いているプルリクエストを読んでサイドバーに出す。gh がなければ何もしない。
    /// `force` でなければ、前に読んでから 30 秒は読まない。
    private func loadGitHub(force: Bool) {
        guard GitHub.isAvailable else { return }
        if !force, let at = gitHubLoadedAt, Date().timeIntervalSince(at) < 30 { return }
        gitHubLoadedAt = Date()
        let repository = self.repository
        Task {
            struct Remotes: Decodable {
                struct Remote: Decodable { let name: String; let fetchUrl: String }
                let remotes: [Remote]
            }
            let remotes = try? await Task.detached { try repository.info(["op": "remotes"], as: Remotes.self) }.value
            let remote = remotes?.remotes.first { $0.name == "origin" } ?? remotes?.remotes.first
            gitHub = remote.flatMap { GitHubRepository(remoteURL: $0.fetchUrl) }
            sidebar.canCreatePullRequest = gitHub != nil && GitHub.isAvailable
            guard let gitHub else {
                sidebar.show(pullRequests: [])
                return
            }
            do {
                sidebar.show(pullRequests: try await GitHub.pullRequests(gitHub))
            } catch {
                EventLog.add(repo: window.title, L("Can’t read pull requests: %@", "\(error)"))
            }
            await loadCommitStatuses(gitHub)
        }
    }

    /// 今のブランチと既定のブランチの、直近のコミットの CI の状態（History の行の印）と作者のアバター。
    /// 読むものと量は Settings > Git > GitHub で決める。
    private func loadCommitStatuses(_ gitHub: GitHubRepository) async {
        let ci = Settings.gitHubCIStatus
        // アバターの URL は、1 回だけにするなら、読めた後は頼まない。
        let authors = Settings.gitHubAvatars && !(Settings.gitHubAvatarsOnce && avatarsLoaded)
        if !ci { history.ciStatus = [:] }
        guard ci || authors else { return }
        if Settings.gitHubCIDefaultBranch, defaultBranch == nil { defaultBranch = await GitHub.defaultBranch(gitHub) }
        let branches = Array(Set([latestRefs?.headBranch, Settings.gitHubCIDefaultBranch ? defaultBranch : nil].compactMap { $0 })).sorted()
        do {
            let (statuses, avatars) = try await GitHub.commitStatuses(
                gitHub, branches: branches, count: Settings.gitHubCICommits, ci: ci, authors: authors)
            if ci {
                history.ciStatus = Dictionary(uniqueKeysWithValues: statuses.compactMap { key, value in Data(hex: key).map { ($0, value) } })
            }
            if authors {
                Avatars.shared.useGitHub(avatars)
                avatarsLoaded = true
            }
        } catch {
            EventLog.add(repo: window.title, L("Can’t read the CI status: %@", "\(error)"))
        }
    }

    /// このウィンドウで GitHub のアバターの URL を読んだか。
    private var avatarsLoaded = false
    /// CI の吹き出し（同じ印をもう一度押したら閉じる）。
    private let ciPopovers = CIPopoverPresenter()

    @objc private func autoFetchSettingChanged() { scheduleAutoFetch() }

    @objc private func gitHubSettingsChanged() {
        if !Settings.gitHubAvatars {
            Avatars.shared.forgetGitHub()
            avatarsLoaded = false
        }
        loadGitHub(force: true)
    }

    private var defaultBranch: String?
    private var cachedIssues: (at: Date, items: [(number: Int, title: String)])?

    /// コミットのメッセージの #123 の候補: 開いている PR と Issue（番号の大きい順）。5 分は覚えておく。
    private func issueCandidates() async -> [(number: Int, title: String)] {
        guard let gitHub else { return [] }
        if let cached = cachedIssues, Date().timeIntervalSince(cached.at) < 300 { return cached.items }
        let issues = (try? await GitHub.issues(gitHub)) ?? []
        let pulls = (try? await GitHub.pullRequests(gitHub))?.map { (number: $0.number, title: "PR: " + $0.title) } ?? []
        let items = (issues + pulls).sorted { $0.number > $1.number }
        cachedIssues = (Date(), items)
        return items
    }

    /// プルリクエストのブランチをチェックアウトする（gh pr checkout）。手元のリポジトリだけ。
    private func checkout(_ pr: PullRequest) {
        guard let gitHub, isLocal, let dir = repository.workdir else { return NSSound.beep() }
        RunningOperations.begin()
        Task {
            defer { RunningOperations.end() }
            do {
                try await GitHub.checkout(pr.number, in: dir, repo: gitHub)
            } catch {
                _ = await NSAlert(error: error).beginSheetModal(for: window)
            }
            refreshRepository(nil)
        }
    }

    /// Repository > Open on GitHub: 今のブランチのページを開く。
    @objc func openOnGitHub(_ sender: Any?) {
        guard let gitHub else { return NSSound.beep() }
        var url = gitHub.webURL
        if let branch = latestRefs?.headBranch { url = url.appendingPathComponent("tree").appendingPathComponent(branch) }
        NSWorkspace.shared.open(url)
    }

    /// Repository > Create Pull Request…: 今のブランチからプルリクエストを作る。
    @objc func createPullRequest(_ sender: Any?) {
        guard let branch = latestRefs?.headBranch else { return NSSound.beep() }
        createPullRequest(from: branch)
    }

    /// `branch` からプルリクエストを作る（メニュー、サイドバーの右クリック、ブランチの印の吹き出しから）。まだ push して
    /// いない（上流がない・先行コミットがある）なら、まず push の確認の窓を出してそこで終わる。CI を見てから、
    /// 吹き出しなどからもう一度作ってもらう。
    func createPullRequest(from branch: String) {
        guard let gitHub, let dir = repository.workdir else { return NSSound.beep() }
        let repository = self.repository
        Task {
            let refs = (try? await Task.detached { try repository.loadRefs() }.value) ?? latestRefs
            let item = refs?.items.first { $0.kind == .local && $0.name == branch }
            if item?.detail == nil || item?.gone == true || (item?.ahead ?? 0) > 0 {
                actions.pushWithConfirmation(branch: branch)
                return
            }
            // タイトルと本文は、ブランチの先頭のコミットのメッセージから（命令はコミット ID を取る）。
            let tip = item?.target.map { $0.map { String(format: "%02x", $0) }.joined() } ?? "HEAD"
            let message = (try? await Task.detached { try repository.run(["op": "commitMessage", "oid": tip]).text }.value) ?? nil
            let parts = (message ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                .split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
            let title = parts.first.map(String.init) ?? branch
            let body = parts.count > 1 ? parts[1].trimmingCharacters(in: .whitespacesAndNewlines) : ""
            let base = await GitHub.defaultBranch(gitHub) ?? "main"
            CreatePullRequestSheet.ask(in: window, branch: branch, title: title, body: body, base: base) { [weak self] answer in
                guard let self, let answer else { return }
                RunningOperations.begin()
                Task {
                    defer { RunningOperations.end() }
                    do {
                        let url = try await GitHub.createPullRequest(
                            in: dir, repo: gitHub, head: branch, title: answer.title, body: answer.body, base: answer.base,
                            draft: answer.draft)
                        let done = NSAlert()
                        done.messageText = L("Pull Request Created")
                        done.informativeText = url
                        done.addButton(withTitle: L("Open in Browser"))
                        done.addButton(withTitle: L("Close"))
                        if await done.beginSheetModal(for: self.window) == .alertFirstButtonReturn, let link = URL(string: url) {
                            NSWorkspace.shared.open(link)
                        }
                    } catch {
                        _ = await NSAlert(error: error).beginSheetModal(for: self.window)
                    }
                    self.loadGitHub(force: true)
                }
            }
        }
    }

    /// Activity の画面で、このリポジトリの操作ログを読み・戻すため。
    var activityRepository: ActivityRepository {
        ActivityRepository(name: window.title, repository: repository, actions: actions, isRemote: !isLocal, location: location)
    }

    /// Repository > Repository Settings（⌘;）。サイドバーの Workspace の Settings を開く。
    @objc func showRepositorySettings(_ sender: Any?) { sidebar.select(page: .settings) }

    // MARK: - commit-graph

    /// commit-graph を書いている間に、もう一度呼ばれた。
    private var commitGraphUpdate: (running: Bool, again: Bool) = (false, false)

    /// 設定（Settings > Performance）で選ばれていれば、commit-graph を裏で最新にする。なければ書き出し、
    /// あれば増えたコミットだけを書き足す。開いたときと fetch・pull の後に呼ぶ。書き足しは増えた数に比例し
    /// （linux で 1 万 7 千コミットなら 2 秒）、増えていなければ何も書かない。書いている間に呼ばれたら、
    /// 終わってからもう一度書き足す。
    func updateCommitGraph() {
        guard Settings.writeCommitGraph else { return }
        guard !commitGraphUpdate.running else {
            commitGraphUpdate.again = true
            return
        }
        commitGraphUpdate = (true, false)
        let repository = self.repository
        Task {
            do {
                let written = try await Task.detached(priority: .utility) { try repository.updateCommitGraph() }.value
                if written { NSLog("commit-graph を書き出した") }
            } catch {
                NSLog("commit-graph を書き出せない: \(error)")
            }
            let again = commitGraphUpdate.again
            commitGraphUpdate = (false, false)
            if again { updateCommitGraph() }
        }
    }

    // MARK: - 自動の fetch

    private var autoFetchTimer: Timer?

    /// リポジトリの設定で決めた間隔で、開いている間 fetch する（SSH 先も、daemon が fetch する）。
    func scheduleAutoFetch() {
        autoFetchTimer?.invalidate()
        autoFetchTimer = nil
        let minutes = Settings.autoFetchMinutes
        guard minutes > 0 else { return }
        autoFetchTimer = Timer.scheduledTimer(withTimeInterval: TimeInterval(minutes * 60), repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.autoFetch() }
        }
    }

    private func autoFetch() {
        let repository = self.repository
        let name = window.title
        Task {
            do {
                try await Task.detached(priority: .utility) { try repository.run(json: #"{"op":"fetch"}"#) }.value
                EventLog.add(repo: name, L("Automatic fetch"))
                // ref が変わらなくても、リモートのタグは読み直している。
                reloadTagRemotes()
                updateCommitGraph()
            } catch {
                EventLog.add(repo: name, L("Automatic fetch failed: %@", "\(error)"))
                showFailure(OperationFailure(error, command: ["op": "fetch"]), title: L("Automatic Fetch Failed"))
            }
        }
    }

    /// 書き込む先の worktree の場所で開く。
    @objc func openInTerminal(_ sender: Any?) {
        openTerminal(at: editing.path)
    }

    private func openTerminal(at path: String) {
        guard isLocal, let terminal = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.Terminal")
        else { return NSSound.beep() }
        NSWorkspace.shared.open(
            [URL(fileURLWithPath: path)], withApplicationAt: terminal, configuration: NSWorkspace.OpenConfiguration())
    }

    @objc func revealInFinder(_ sender: Any?) {
        guard isLocal else { return NSSound.beep() }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: editing.path)])
    }

    /// 選んだものを今のブランチに取り込んだら、コンフリクトするか（rebase は目安）。
    private enum Forecast { case merge, rebase }

    /// Merge・Rebase の相手を選んでもらう。候補は History で選んでいるコミット（あれば先頭）、
    /// 今のブランチ以外のローカルブランチ、リモートブランチ、タグ。キャンセルなら nil。`forecast` なら、選ぶたびに
    /// コンフリクトするかを裏で調べて見せる。シートで出す（`runModal` の間は、裏で調べた答えを画面に戻せない）。
    private func chooseRevision(title: String, message: String, ok: String, forecast: Forecast? = nil) async -> String? {
        var choices: [(title: String, rev: String)] = []
        if let selectedCommit, selectedCommit != latestRefs?.head {
            let hex = selectedCommit.map { String(format: "%02x", $0) }.joined()
            choices.append((L("Selected Commit %@", String(hex.prefix(8))), hex))
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
        alert.addButton(withTitle: L("Cancel"))
        alert.accessoryView = popup
        // 選び直すたびに調べる（答えは覚えておき、遅れて来た前の答えは捨てる）。
        var target: ClosureTarget?
        if let forecast {
            let label = Actions.forecastLabel(width: popup.frame.width)
            final class Answers { var known: [String: MergeForecast?] = [:]; var shown = "" }
            let answers = Answers()
            let actions = self.actions
            let update = {
                guard let item = popup.selectedItem else { return }
                let rev = choices[item.tag].rev
                answers.shown = rev
                if let known = answers.known[rev] {
                    return Actions.show(known, estimate: forecast == .rebase, in: label)
                }
                label.stringValue = L("Checking for conflicts…")
                label.textColor = .secondaryLabelColor
                Task { @MainActor in
                    let answer = await actions.predictMerge(theirs: rev)
                    answers.known[rev] = answer
                    if answers.shown == rev { Actions.show(answer, estimate: forecast == .rebase, in: label) }
                }
            }
            update()
            target = ClosureTarget(update)
            popup.target = target
            popup.action = #selector(ClosureTarget.fire)
            let stack = NSStackView(views: [popup, label])
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 8
            stack.layoutSubtreeIfNeeded()
            stack.frame.size = stack.fittingSize
            alert.accessoryView = stack
        }
        alert.window.initialFirstResponder = popup
        let response = await alert.beginSheetModal(for: window)
        withExtendedLifetime(target) {}
        guard response == .alertFirstButtonReturn, let item = popup.selectedItem else { return nil }
        return choices[item.tag].rev
    }

    /// メニューの項目とツールバーのボタンを使えるか。
    // MARK: - History の検索の一致

    /// ⌘G・⌘⇧G がどの欄にも拾われなかったとき（ツールバーの検索欄や、探していない欄にいるとき）。
    @objc func findNext(_ sender: Any?) {
        if !moveHistoryMatch(by: 1) { NSSound.beep() }
    }

    @objc func findPrevious(_ sender: Any?) {
        if !moveHistoryMatch(by: -1) { NSSound.beep() }
    }

    func moveHistoryMatch(by step: Int) -> Bool {
        guard history.hasSearch else { return false }
        sidebar.select(page: .history)
        return history.moveToMatch(by: step)
    }

    private func validate(_ action: Selector?) -> Bool {
        let sequence = workingCopy.sequence
        switch action {
        case #selector(findNext(_:)), #selector(findPrevious(_:)): return history.hasSearch
        case #selector(continueSequence(_:)), #selector(abortSequence(_:)): return sequence != nil
        case #selector(skipSequence(_:)): return sequence.map { $0.hasPrefix("rebase") || $0 == "cherry-pick" } ?? false
        case #selector(push(_:)), #selector(forcePush(_:)): return latestRefs?.headBranch != nil
        case #selector(pull(_:)), #selector(pullMerge(_:)), #selector(pullRebase(_:)), #selector(pullFastForward(_:)):
            return latestRefs?.headBranch != nil && sequence == nil
        case #selector(mergeIntoCurrent(_:)), #selector(rebaseCurrent(_:)): return latestRefs != nil && sequence == nil
        case #selector(popStash(_:)), #selector(showStashes(_:)): return latestStash != nil
        case #selector(undoOperation(_:)): return undoLabel != nil
        case #selector(goToHead(_:)): return latestRefs?.head != nil
        case #selector(switchBranch(_:)): return latestRefs != nil
        case #selector(renameBranch(_:)), #selector(setUpstream(_:)): return latestRefs?.headBranch != nil
        case #selector(commitChanges(_:)), #selector(commitAndPush(_:)): return showsWorkingCopy && workingCopy.canCommit
        case #selector(stageAllChanges(_:)): return showsWorkingCopy && workingCopy.canStageAll
        case #selector(openInTerminal(_:)), #selector(revealInFinder(_:)): return isLocal
        case #selector(openOnGitHub(_:)): return gitHub != nil
        case #selector(createPullRequest(_:)): return gitHub != nil && GitHub.isAvailable && latestRefs?.headBranch != nil
        default: return true
        }
    }

    func windowWillClose(_ notification: Notification) {
        autoFetchTimer?.invalidate()
        countsTimer?.invalidate()
        connectionTimer?.invalidate()
        watch = nil
        onClose?()
    }
}

extension RepositoryWindow {
    /// 上流があって、そのリモートブランチもある（Push で送れる）。なければ Push は Publish（送り先を選ぶ）になる。
    nonisolated static func isPublished(_ branch: RefItem) -> Bool { branch.detail != nil && !branch.gone }

    /// 今のブランチを送るのが Publish か（ブランチがなければ Push のまま）。
    private var publishesCurrent: Bool {
        let branch = editing.actions.currentBranch
        guard let item = latestRefs?.items.first(where: { $0.kind == .local && $0.name == branch }) else { return false }
        return !Self.isPublished(item)
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(undoOperation(_:)):
            item.title = undoLabel.map { "Undo \($0)" } ?? "Undo"
        case #selector(push(_:)):
            item.title = publishesCurrent ? "Publish…" : "Push"
        case #selector(commitAndPush(_:)):
            item.title = publishesCurrent ? "Commit and Publish…" : "Commit and Push"
        case #selector(showWorkingCopy(_:)):
            item.state = pages?.page == .workingCopy ? .on : .off
        case #selector(showHistory(_:)):
            item.state = pages?.page == .history ? .on : .off
        case #selector(showStashes(_:)):
            item.state = pages?.page == .stashes ? .on : .off
        case #selector(showReflog(_:)):
            item.state = pages?.page == .reflog ? .on : .off
        case #selector(showRepositorySettings(_:)):
            item.state = pages?.page == .settings ? .on : .off
        default:
            break
        }
        return validate(item.action)
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        validate(item.action)
    }
}
