import AmonhenKit
import AppKit

/// Working Copy: Staged / Unstaged のファイル、選んだファイルの差分（行を選んで stage・unstage）、
/// コミットメッセージ。
final class WorkingCopyViewController: NSSplitViewController, NSOutlineViewDataSource, NSOutlineViewDelegate,
    NSTextViewDelegate
{
    private let repository: Repository
    private let diffView: DiffViewController
    private let files = NSOutlineView()
    private let message = NSTextView()
    private let amend = NSButton(checkboxWithTitle: "Amend", target: nil, action: nil)
    private let commitButton = NSButton(title: "Commit", target: nil, action: nil)
    private let fileButton = NSButton(title: "Stage File", target: nil, action: nil)
    private let linesButton = NSButton(title: "Stage Lines", target: nil, action: nil)
    private let titleLabel = NSTextField(labelWithString: "")

    private final class Group {
        let staged: Bool
        var items: [StatusItem] = []
        init(staged: Bool) { self.staged = staged }
        var title: String { staged ? "Staged" : "Unstaged" }
    }

    /// 一覧の項目。同じファイルが両方の組に出ることがある。
    private final class Entry {
        let item: StatusItem
        let staged: Bool
        init(item: StatusItem, staged: Bool) {
            self.item = item
            self.staged = staged
        }
    }

    private var status: WorkingStatus?
    private let stagedGroup = Group(staged: true)
    private let unstagedGroup = Group(staged: false)
    private var entries: [ObjectIdentifier: [Entry]] = [:]
    private var current: WorkingFileDiff?
    /// 選んでいたファイル（読み直した後に選び直す）。
    private var selectedKey: (path: String, staged: Bool)?
    private var request = 0

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
        let filesScroll = NSScrollView()
        filesScroll.documentView = files
        filesScroll.hasVerticalScroller = true

        message.isRichText = false
        message.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        message.isAutomaticQuoteSubstitutionEnabled = false
        message.isAutomaticDashSubstitutionEnabled = false
        message.delegate = self
        message.textContainerInset = NSSize(width: 4, height: 4)
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
        let commitRow = NSStackView(views: [amend, NSView(), commitButton])

        let left = NSStackView(views: [filesScroll, messageScroll, commitRow])
        left.orientation = .vertical
        left.alignment = .leading
        left.spacing = 6
        left.edgeInsets = NSEdgeInsets(top: 0, left: 0, bottom: 8, right: 8)
        for v in [filesScroll, messageScroll, commitRow] {
            v.widthAnchor.constraint(equalTo: left.widthAnchor, constant: -8).isActive = true
        }
        messageScroll.heightAnchor.constraint(equalToConstant: 96).isActive = true
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
        let bar = NSStackView(views: [titleLabel, NSView(), linesButton, fileButton])
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
            guard let status = try? await Task.detached(priority: .userInitiated, operation: { try repository.loadStatus() }).value
            else { return }
            if status.generation != self.status?.generation { show(status) }
        }
    }

    private func show(_ status: WorkingStatus) {
        self.status = status
        stagedGroup.items = status.items.filter(\.isStaged)
        unstagedGroup.items = status.items.filter(\.isUnstaged)
        entries.removeAll()
        files.reloadData()
        files.expandItem(nil, expandChildren: true)

        // 同じファイル・同じ組を選び直す。なければ同じファイルのもう一方の組、それもなければ最初のもの。
        let groups = [stagedGroup, unstagedGroup]
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
        let made = group.items.map { Entry(item: $0, staged: group.staged) }
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
        let staged = selectedKey?.staged ?? false
        fileButton.title = staged ? "Unstage File" : "Stage File"
        fileButton.isEnabled = selectedKey != nil
        linesButton.title = staged ? "Unstage Lines" : "Stage Lines"
        linesButton.isEnabled = current != nil && !diffView.selectedChangeRows.isEmpty
        commitButton.isEnabled = !(stagedGroup.items.isEmpty && amend.state == .off)
            && !message.string.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
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

    @objc private func toggleAmend() {
        updateButtons()
    }

    @objc private func commit() {
        let text = message.string
        let isAmend = amend.state == .on
        let repository = self.repository
        Task {
            do {
                try await Task.detached(priority: .userInitiated) { try repository.commit(message: text, amend: isAmend) }.value
                message.string = ""
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

    private var groups: [Group] { [stagedGroup, unstagedGroup].filter { !$0.items.isEmpty } }

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
