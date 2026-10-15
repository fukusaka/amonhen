import AmonhenKit
import AppKit

/// Working Copy: Staged / Unstaged のファイル、選んだファイルの差分（行を選んで stage・unstage）、
/// コミットメッセージ。
final class WorkingCopyViewController: NSSplitViewController, NSOutlineViewDataSource, NSOutlineViewDelegate,
    NSTextViewDelegate
{
    private let repository: Repository
    private let diffView: DiffViewController
    private let files = KeyOutlineView()
    /// コミットメッセージの 1 行目と、2 行目以降。
    private let summary = NSTextField()
    private let message = NSTextView()
    private let stageAllButton = NSButton(title: "Stage All", target: nil, action: nil)
    private let amend = NSButton(checkboxWithTitle: "Amend", target: nil, action: nil)
    private let commitButton = NSButton(title: "Commit", target: nil, action: nil)
    private let fileButton = NSButton(title: "Stage File", target: nil, action: nil)
    private let linesButton = NSButton(title: "Stage Lines", target: nil, action: nil)
    private let discardButton = NSButton(title: "Discard Lines", target: nil, action: nil)
    var actions: Actions?
    private let titleLabel = NSTextField(labelWithString: "")
    /// merge・rebase などの途中のときに出す帯。
    private let sequenceBar = NSStackView()
    private let sequenceLabel = NSTextField(labelWithString: "")

    private final class Group {
        let staged: Bool
        let conflicts: Bool
        var items: [StatusItem] = []
        init(staged: Bool, conflicts: Bool = false) {
            self.staged = staged
            self.conflicts = conflicts
        }
        var title: String { conflicts ? "Conflicts" : staged ? "Staged" : "Unstaged" }
    }

    /// 一覧の項目。同じファイルが両方の組に出ることがある。
    private final class Entry {
        let item: StatusItem
        let staged: Bool
        let conflict: Bool
        init(item: StatusItem, staged: Bool, conflict: Bool) {
            self.item = item
            self.staged = staged
            self.conflict = conflict
        }
    }

    private var status: WorkingStatus?
    private let stagedGroup = Group(staged: true)
    private let unstagedGroup = Group(staged: false)
    private let conflictGroup = Group(staged: false, conflicts: true)
    /// コンフリクトの解決のボタン。
    private let conflictButtons = NSStackView()
    private var entries: [ObjectIdentifier: [Entry]] = [:]
    private var current: WorkingFileDiff?
    /// 選んでいたファイル（読み直した後に選び直す）。
    private var selectedKey: (path: String, staged: Bool)?
    private var request = 0
    private var isConflict = false

    init(repository: Repository, renderer: TextRenderer) {
        self.repository = repository
        self.diffView = DiffViewController(renderer: renderer)
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        splitView.isVertical = true

        // 左: ファイルの一覧とコミット欄。
        let column = NSTableColumn(identifier: .init("path"))
        files.addTableColumn(column)
        files.outlineTableColumn = column
        files.headerView = nil
        files.style = .sourceList
        files.floatsGroupRows = false
        files.dataSource = self
        files.delegate = self
        // Space か Enter で、選んだファイルを stage / unstage する。
        files.onToggle = { [weak self] in self?.toggleFile() }
        let filesScroll = NSScrollView()
        filesScroll.documentView = files
        filesScroll.hasVerticalScroller = true

        message.isRichText = false
        message.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        message.isAutomaticQuoteSubstitutionEnabled = false
        message.isAutomaticDashSubstitutionEnabled = false
        message.delegate = self
        message.textContainerInset = NSSize(width: 4, height: 4)
        summary.placeholderString = "Summary"
        summary.font = .systemFont(ofSize: 13)
        summary.target = self
        summary.action = #selector(summaryChanged)
        NotificationCenter.default.addObserver(
            self, selector: #selector(summaryChanged), name: NSControl.textDidChangeNotification, object: summary)
        let messageScroll = NSScrollView()
        messageScroll.documentView = message
        messageScroll.hasVerticalScroller = true
        messageScroll.borderType = .bezelBorder
        message.autoresizingMask = [.width]
        message.isVerticallyResizable = true
        message.textContainer?.widthTracksTextView = true

        commitButton.target = self
        commitButton.action = #selector(commit)
        commitButton.keyEquivalent = "\r"
        commitButton.keyEquivalentModifierMask = .command
        amend.target = self
        amend.action = #selector(toggleAmend)
        stageAllButton.target = self
        stageAllButton.action = #selector(stageAll)
        let commitRow = NSStackView(views: [amend, stageAllButton, NSView(), commitButton])

        sequenceLabel.textColor = .systemOrange
        sequenceLabel.lineBreakMode = .byTruncatingTail
        let buttons = [("Continue", "continue"), ("Skip", "skip"), ("Abort", "abort")].map { title, op in
            let b = NSButton(title: title, target: self, action: #selector(sequenceAction(_:)))
            b.identifier = .init(op)
            return b
        }
        sequenceBar.setViews([sequenceLabel] + buttons, in: .leading)
        sequenceBar.isHidden = true
        let left = NSStackView(views: [sequenceBar, filesScroll, summary, messageScroll, commitRow])
        left.orientation = .vertical
        left.alignment = .leading
        left.spacing = 6
        left.edgeInsets = NSEdgeInsets(top: 0, left: 0, bottom: 8, right: 8)
        for v in [sequenceBar, filesScroll, summary, messageScroll, commitRow] {
            v.widthAnchor.constraint(equalTo: left.widthAnchor, constant: -8).isActive = true
        }
        messageScroll.heightAnchor.constraint(equalToConstant: 72).isActive = true
        left.frame = NSRect(x: 0, y: 0, width: 280, height: 500)
        let leftController = NSViewController()
        leftController.view = left
        let leftItem = NSSplitViewItem(viewController: leftController)
        leftItem.minimumThickness = 220
        leftItem.holdingPriority = .init(260)
        addSplitViewItem(leftItem)

        // 右: 操作ボタンと差分。
        fileButton.target = self
        fileButton.action = #selector(toggleFile)
        linesButton.target = self
        linesButton.action = #selector(stageLines)
        titleLabel.lineBreakMode = .byTruncatingMiddle
        discardButton.target = self
        discardButton.action = #selector(discard)
        let conflictActions: [(String, Selector)] = [
            ("Merge Editor…", #selector(openMergeEditor)),
            ("Use Current", #selector(useOurs)), ("Use Incoming", #selector(useTheirs)), ("Use Both", #selector(useBoth)),
            ("Take Current File", #selector(takeOursFile)), ("Take Incoming File", #selector(takeTheirsFile)),
            ("Mark Resolved", #selector(markResolved)),
        ]
        conflictButtons.setViews(conflictActions.map { NSButton(title: $0.0, target: self, action: $0.1) }, in: .leading)
        conflictButtons.isHidden = true
        let bar = NSStackView(views: [titleLabel, NSView(), conflictButtons, discardButton, linesButton, fileButton])
        bar.edgeInsets = NSEdgeInsets(top: 6, left: 8, bottom: 6, right: 8)
        diffView.isSelectable = true
        diffView.onSelectionChange = { [weak self] in self?.updateButtons() }
        let right = BarAndContentView(bar: bar, content: diffView.view)
        right.frame = NSRect(x: 0, y: 0, width: 700, height: 500)
        // NSSplitViewController に addChild すると分割の項目が増えるので、右側の親に付ける。
        let rightController = NSViewController()
        rightController.view = right
        rightController.addChild(diffView)
        let rightItem = NSSplitViewItem(viewController: rightController)
        rightItem.minimumThickness = 300
        addSplitViewItem(rightItem)
        updateButtons()
    }

    func overlays() -> [(CGImage, NSView)] {
        view.window == nil ? [] : diffView.overlay().map { [$0] } ?? []
    }

    // MARK: - 読み直し

    /// status を裏で読み直す。
    func refresh() {
        let repository = self.repository
        Task {
            let sequence = try? await Task.detached { try repository.run(["op": "sequence"]).text }.value
            showSequence(sequence ?? nil)
        }
        Task {
            guard let status = try? await Task.detached(priority: .userInitiated, operation: { try repository.loadStatus() }).value
            else { return }
            if status.generation != self.status?.generation { show(status) }
        }
    }

    private func show(_ status: WorkingStatus) {
        self.status = status
        conflictGroup.items = status.items.filter { $0.kind == .unmerged }
        stagedGroup.items = status.items.filter { $0.kind != .unmerged && $0.isStaged }
        unstagedGroup.items = status.items.filter { $0.kind != .unmerged && $0.isUnstaged }
        entries.removeAll()
        files.reloadData()
        files.expandItem(nil, expandChildren: true)

        // 同じファイル・同じ組を選び直す。なければ同じファイルのもう一方の組、それもなければ最初のもの。
        let groups = [conflictGroup, stagedGroup, unstagedGroup]
        let target = selectedKey.flatMap { key in
            groups.first { $0.staged == key.staged }.flatMap { g in entry(in: g, path: key.path) }
                ?? groups.lazy.compactMap { self.entry(in: $0, path: key.path) }.first
        } ?? groups.lazy.compactMap { self.entries(of: $0).first }.first
        if let target {
            let row = files.row(forItem: target)
            files.selectRowIndexes([row], byExtendingSelection: false)
            showDiff(of: target, keepScroll: target.item.path == selectedKey?.path && target.staged == selectedKey?.staged)
        } else {
            current = nil
            selectedKey = nil
            diffView.show(nil, message: "変更なし")
            titleLabel.stringValue = ""
            updateButtons()
        }
    }

    private func entries(of group: Group) -> [Entry] {
        if let cached = entries[ObjectIdentifier(group)] { return cached }
        let made = group.items.map { Entry(item: $0, staged: group.staged, conflict: group.conflicts) }
        entries[ObjectIdentifier(group)] = made
        return made
    }

    private func entry(in group: Group, path: String) -> Entry? {
        entries(of: group).first { $0.item.path == path }
    }

    private func showDiff(of entry: Entry, keepScroll: Bool = false) {
        guard let status else { return }
        selectedKey = (entry.item.path, entry.staged)
        titleLabel.stringValue = entry.item.path
        request += 1
        let request = self.request
        isConflict = entry.conflict
        diffView.isConflictView = entry.conflict
        if entry.conflict {
            let repository = self.repository
            let path = entry.item.path
            Task {
                let view = try? await Task.detached(priority: .userInitiated) { try repository.conflictView(path: path) }.value
                guard request == self.request else { return }
                current = nil
                diffView.show(view, keepScroll: keepScroll)
                updateButtons()
            }
            return
        }
        Task {
            let diff = try? await Task.detached(priority: .userInitiated) {
                try status.diff(of: entry.item, staged: entry.staged)
            }.value
            guard request == self.request else { return }
            current = diff
            if let diff, diff.isBinary {
                diffView.show(nil, message: "バイナリファイル")
            } else {
                diffView.show(diff, keepScroll: keepScroll)
            }
            updateButtons()
        }
    }

    private func updateButtons() {
        conflictButtons.isHidden = !isConflict
        for v in [discardButton, linesButton, fileButton] { v.isHidden = isConflict }
        if isConflict {
            let hasConflict = diffView.selectedConflict != nil
            // 先頭はマージエディタ（いつでも開ける）、続く 3 つが選んだコンフリクトの解決。
            for case let b as NSButton in conflictButtons.views.dropFirst().prefix(3) { b.isEnabled = hasConflict }
        }
        let staged = selectedKey?.staged ?? false
        fileButton.title = staged ? "Unstage File" : "Stage File"
        fileButton.isEnabled = selectedKey != nil
        linesButton.title = staged ? "Unstage Lines" : "Stage Lines"
        linesButton.isEnabled = current != nil && !diffView.selectedChangeRows.isEmpty
        // 選んだ行があれば行を、なければファイルを破棄する。stage 済みの変更は破棄しない。
        discardButton.isHidden = staged || isConflict
        discardButton.title = diffView.selectedChangeRows.isEmpty ? "Discard File" : "Discard Lines"
        discardButton.isEnabled = selectedKey != nil
        commitButton.isEnabled = !(stagedGroup.items.isEmpty && amend.state == .off)
            && !composedMessage.isEmpty
        stageAllButton.isEnabled = !unstagedGroup.items.isEmpty
    }

    // MARK: - 操作

    /// 裏で git を実行し、終わったら status を読み直す。失敗したら知らせる。
    private func run(_ work: @escaping @Sendable () throws -> Void) {
        Task {
            do {
                try await Task.detached(priority: .userInitiated) { try work() }.value
            } catch {
                _ = await NSAlert(error: error).beginSheetModal(for: view.window!)
            }
            refresh()
        }
    }

    @objc private func toggleFile() {
        guard let key = selectedKey else { return }
        let repository = self.repository
        let path = key.path
        if key.staged {
            run { try repository.unstage(paths: [path]) }
        } else {
            run { try repository.stage(paths: [path]) }
        }
    }

    @objc private func stageLines() {
        guard let current else { return }
        let rows = diffView.selectedChangeRows
        guard !rows.isEmpty else { return }
        run { try current.stage(rows: rows) }
    }

    @objc private func discard() {
        guard let key = selectedKey, !key.staged, let actions else { return }
        let rows = diffView.selectedChangeRows
        if let current, !rows.isEmpty {
            actions.perform { try current.discard(rows: rows) }
        } else if current?.item.kind == .untracked {
            guard actions.confirm(
                "\(key.path) を削除しますか？", message: "untracked のファイルなので、Undo では戻せません。", ok: "削除") else { return }
            let url = URL(fileURLWithPath: repositoryRoot).appendingPathComponent(key.path)
            actions.perform { try FileManager.default.removeItem(at: url) }
        } else {
            actions.perform(["op": "discard", "paths": [key.path]])
        }
    }

    /// 作業ツリーのルート（untracked のファイルを消すときに使う）。
    var repositoryRoot = ""

    private func showSequence(_ sequence: String?) {
        sequenceBar.isHidden = sequence == nil
        if let sequence {
            sequenceLabel.stringValue = "\(sequence) の途中です"
        }
    }

    @objc private func sequenceAction(_ sender: NSButton) {
        guard let op = sender.identifier?.rawValue else { return }
        actions?.perform(["op": op])
    }

    private func resolve(_ choice: String) {
        guard let key = selectedKey, let index = diffView.selectedConflict else { return }
        actions?.perform(["op": "resolveConflict", "path": key.path, "index": index, "choice": choice])
    }

    @objc private func openMergeEditor() {
        guard let key = selectedKey, let actions else { return }
        MergeEditorWindowController.show(path: key.path, repository: repository, actions: actions)
    }

    @objc private func useOurs() { resolve("ours") }
    @objc private func useTheirs() { resolve("theirs") }
    @objc private func useBoth() { resolve("both") }

    @objc private func takeOursFile() {
        guard let key = selectedKey else { return }
        actions?.perform(["op": "takeSide", "path": key.path, "ours": true])
    }

    @objc private func takeTheirsFile() {
        guard let key = selectedKey else { return }
        actions?.perform(["op": "takeSide", "path": key.path, "ours": false])
    }

    @objc private func markResolved() {
        guard let key = selectedKey else { return }
        let file = URL(fileURLWithPath: repositoryRoot).appendingPathComponent(key.path)
        if let text = try? String(contentsOf: file, encoding: .utf8), text.contains("<<<<<<<"),
           actions?.confirm("\(key.path) にコンフリクトのマーカーが残っています", message: "このまま解決済みにしますか？", ok: "解決済みにする") != true {
            return
        }
        let repository = self.repository
        let path = key.path
        actions?.perform { try repository.stage(paths: [path]) }
    }

    @objc private func toggleAmend() {
        // 直前のコミットのメッセージを入れて直せるようにする。
        if amend.state == .on, message.string.isEmpty, summary.stringValue.isEmpty {
            let repository = self.repository
            Task {
                let text = try? await Task.detached { try repository.run(["op": "commitMessage", "oid": "HEAD"]).text }.value
                if amend.state == .on, message.string.isEmpty, summary.stringValue.isEmpty, let text = text ?? nil {
                    // 1 行目を Summary、空行の後を Description に分ける。
                    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
                    let parts = trimmed.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
                    summary.stringValue = String(parts.first ?? "")
                    message.string = parts.count > 1 ? parts[1].trimmingCharacters(in: .whitespacesAndNewlines) : ""
                }
                updateButtons()
            }
        }
        updateButtons()
    }

    /// Summary と Description をつないだメッセージ。
    private var composedMessage: String {
        let head = summary.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        let body = message.string.trimmingCharacters(in: .whitespacesAndNewlines)
        return body.isEmpty ? head : head.isEmpty ? body : "\(head)\n\n\(body)"
    }

    @objc private func summaryChanged() {
        updateButtons()
    }

    /// stage していない変更（untracked を含む）をすべて stage する。
    @objc private func stageAll() {
        let paths = unstagedGroup.items.map(\.path)
        guard !paths.isEmpty else { return }
        let repository = self.repository
        actions?.perform { try repository.stage(paths: paths) }
    }

    @objc private func commit() {
        let text = composedMessage
        let isAmend = amend.state == .on
        let repository = self.repository
        Task {
            do {
                try await Task.detached(priority: .userInitiated) { try repository.commit(message: text, amend: isAmend) }.value
                message.string = ""
                summary.stringValue = ""
                amend.state = .off
            } catch {
                _ = await NSAlert(error: error).beginSheetModal(for: view.window!)
            }
            refresh()
            updateButtons()
        }
    }

    func textDidChange(_ notification: Notification) {
        updateButtons()
    }

    // MARK: - NSOutlineView

    private var groups: [Group] { [conflictGroup, stagedGroup, unstagedGroup].filter { !$0.items.isEmpty } }

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        switch item {
        case nil: groups.count
        case let group as Group: entries(of: group).count
        default: 0
        }
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        switch item {
        case let group as Group: entries(of: group)[index]
        default: groups[index]
        }
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        item is Group
    }

    func outlineView(_ outlineView: NSOutlineView, isGroupItem item: Any) -> Bool {
        item is Group
    }

    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool {
        item is Entry
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        let field = outlineView.makeView(withIdentifier: .init("cell"), owner: nil) as? NSTextField ?? {
            let f = NSTextField(labelWithString: "")
            f.identifier = .init("cell")
            f.lineBreakMode = .byTruncatingMiddle
            return f
        }()
        switch item {
        case let group as Group:
            field.stringValue = "\(group.title) (\(group.items.count))"
        case let entry as Entry:
            let state = entry.staged ? entry.item.indexState : entry.item.worktreeState
            let mark = state == "?" ? "U" : String(state)
            field.stringValue = "\(mark)  \(entry.item.path)"
            field.toolTip = entry.item.origPath.map { "\($0) → \(entry.item.path)" } ?? entry.item.path
        default:
            break
        }
        return field
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard let entry = files.item(atRow: files.selectedRow) as? Entry else { return }
        if selectedKey?.path == entry.item.path, selectedKey?.staged == entry.staged, current != nil { return }
        showDiff(of: entry)
    }
}

/// 上に高さ固定の帯、残りに中身を置く。
final class BarAndContentView: NSView {
    private let bar: NSView
    private let content: NSView
    private let barHeight: CGFloat = 34

    init(bar: NSView, content: NSView) {
        self.bar = bar
        self.content = content
        super.init(frame: .zero)
        addSubview(bar)
        addSubview(content)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        arrange()
    }

    override func layout() {
        super.layout()
        arrange()
    }

    private func arrange() {
        bar.frame = NSRect(x: 0, y: 0, width: bounds.width, height: barHeight)
        content.frame = NSRect(x: 0, y: barHeight, width: bounds.width, height: max(0, bounds.height - barHeight))
    }
}

/// Space・Enter でファイルを stage / unstage する一覧。
final class KeyOutlineView: NSOutlineView {
    var onToggle: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        switch event.charactersIgnoringModifiers {
        case " ", "\r":
            onToggle?()
        default:
            super.keyDown(with: event)
        }
    }
}
