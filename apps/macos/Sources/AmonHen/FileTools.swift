import AmonhenKit
import AppKit
import CAmonhen

private let fileToolDate: DateFormatter = {
    let f = DateFormatter()
    f.dateStyle = .medium
    f.timeStyle = .short
    return f
}()

/// File History・Blame の中身。リポジトリのタブの隣のタブ（`FileToolWindowController`）に置く。
class FileToolViewController: NSViewController {
    let path: String
    /// 何を出しているか（窓の題）。
    var heading = "" { didSet { onHeadingChange?(heading) } }
    var onHeadingChange: ((String) -> Void)?
    /// タブの短い題（種類とファイル名）。
    var tabTitle: String { (path as NSString).lastPathComponent }

    init(path: String) {
        self.path = path
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    /// 中身（サブクラスが作る）。
    func makeContent() -> NSView { NSView() }

    override func loadView() {
        let content = makeContent()
        content.frame = NSRect(x: 0, y: 0, width: 1200, height: 780)
        view = content
    }
}

/// File History・Blame のタブ。リポジトリのタブ（`tabWith`）の隣に開く（タブを引き出せば別の窓）。リポジトリのタブと
/// 同じツールバーを付けるので、タブを切り替えても上の並びと中身の大きさが変わらない。ツールバーとメニューの操作は、
/// リポジトリのタブに切り替えてからリポジトリ（`RepositoryWindow`）に渡す。
/// 次の起動で開き直すタブの記録には入らない（記録するのはリポジトリのタブだけ）。閉じるまで `FileToolWindows` が持つ。
final class FileToolWindowController: NSWindowController {
    private var toolbar: MainToolbar?
    private var forwarder: RepositoryForwarder?
    /// 何のタブか（同じものを開き直したら、前のタブを閉じて入れ替える）。
    private var key = ""

    static func show(
        _ tool: FileToolViewController, key: String, tabWith: NSWindow?, toolbar: MainToolbar?, repository: RepositoryWindow?
    ) {
        let previous = FileToolWindows.controllers.first { ($0 as? FileToolWindowController)?.key == key }
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1200, height: 780),
            styleMask: tabWith?.styleMask ?? [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        tool.onHeadingChange = { [weak window] in window?.title = $0 }
        window.title = tool.heading
        window.tab.title = tool.tabTitle
        window.contentViewController = tool
        window.setContentSize(NSSize(width: 1200, height: 780))
        let controller = FileToolWindowController(window: window)
        controller.key = key
        controller.toolbar = toolbar
        window.toolbar = toolbar?.toolbar
        if let repository {
            controller.forwarder = RepositoryForwarder(repository: repository)
        }
        FileToolWindows.keep(controller)
        window.tabbingIdentifier = RepositoryWindow.tabbingIdentifier
        if let previous, let old = previous.window, old.tabGroup != nil {
            // 前のタブの場所に入れ替える（読み直した中身にする）。
            old.addTabbedWindow(window, ordered: .above)
            window.makeKeyAndOrderFront(nil)
            previous.close()
        } else if let tabWith {
            tabWith.addTabbedWindow(window, ordered: .above)
            window.makeKeyAndOrderFront(nil)
        } else {
            window.center()
            controller.showWindow(nil)
        }
    }

    /// 開いたリポジトリの窓（Window > Move Tab to Window で先頭に出す）。
    var repositoryWindow: NSWindow? { forwarder?.repositoryWindow }

    override func supplementalTarget(forAction action: Selector, sender: Any?) -> Any? {
        if let forwarder, forwarder.responds(to: action) { return forwarder }
        return super.supplementalTarget(forAction: action, sender: sender)
    }
}

/// File History・Blame のタブで受けたメニューとツールバーの操作を、リポジトリに渡す。シートや一覧はリポジトリの窓に出る
/// ので、先にリポジトリのタブに切り替える（使えるかの問い合わせは、切り替えずに渡す）。
private final class RepositoryForwarder: NSObject {
    private weak var repository: RepositoryWindow?

    init(repository: RepositoryWindow) {
        self.repository = repository
    }

    /// リポジトリの窓（閉じていれば nil）。
    var repositoryWindow: NSWindow? { repository?.window }

    private static let validations: Set<Selector> = [
        #selector(NSToolbarItemValidation.validateToolbarItem(_:)),
        #selector(NSMenuItemValidation.validateMenuItem(_:)),
        NSSelectorFromString("validateUserInterfaceItem:"),
    ]

    // レスポンダチェーンから（メインスレッドで）呼ばれる。
    nonisolated override func responds(to selector: Selector!) -> Bool {
        MainActor.assumeIsolated { repository?.responds(to: selector) ?? false }
    }

    nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
        let target: RepositoryWindow? = MainActor.assumeIsolated {
            guard let repository else { return nil }
            if !Self.validations.contains(selector) { repository.window.makeKeyAndOrderFront(nil) }
            return repository
        }
        return target
    }
}

/// blame: ファイルの各行を最後に変えたコミット。同じコミットの行をまとめ、まとまりの先頭にコミットを出す。
/// 行をダブルクリックすると、そのコミットを History で選ぶ。読み終える前から（読んでいます…と）出す。
final class BlameViewController: FileToolViewController, NSTableViewDataSource, NSTableViewDelegate {
    private let rev: String?
    private let repository: Repository
    private var blame: Blame?
    /// 行ごとの、まとまりの番号（背景を交互に変える）と、まとまりの中で何行目か。
    private var blocks: [(index: Int, offset: Int)] = []
    /// 構文の色の範囲（行をつないだ文章の先頭からの UTF-16、始まりの順）と、各行の先頭の位置。読めたら描き直す。
    private var syntax: [AmonhenSyntaxSpan] = []
    private var lineStarts: [Int] = []
    private let palette = SyntaxColors.palette
    private let table = NSTableView()
    private let message = NSTextField(labelWithString: L("Loading…"))
    private let onSelectCommit: (Data) -> Void

    init(path: String, rev: String?, repository: Repository, onSelectCommit: @escaping (Data) -> Void) {
        self.rev = rev
        self.repository = repository
        self.onSelectCommit = onSelectCommit
        super.init(path: path)
        heading = "Blame — \(path)" + (rev.map { " @ \($0.prefix(8))" } ?? "")
        load()
    }

    override var tabTitle: String { "Blame — " + super.tabTitle }

    required init?(coder: NSCoder) { fatalError() }

    private func load() {
        var command: [String: Any] = ["op": "blame", "path": path]
        if let rev { command["rev"] = rev }
        let repository = self.repository
        Task { [weak self] in
            do {
                let json = try Repository.json(command)
                let blame = try await Task.detached(priority: .userInitiated) {
                    try repository.info(json: json, as: Blame.self)
                }.value
                self?.show(blame)
            } catch {
                NSLog("ファイルの情報を読めない: \(error)")
                self?.message.stringValue = error.localizedDescription
            }
        }
    }

    private func show(_ blame: Blame) {
        var index = -1, offset = 0, previous = -1
        blocks = blame.lines.map { line in
            if line.c != previous {
                index += 1
                offset = 0
                previous = line.c
            } else {
                offset += 1
            }
            return (index, offset)
        }
        self.blame = blame
        message.isHidden = true
        table.reloadData()
        colorSyntax(blame)
    }

    /// 行をつないだ文章を裏で解析し、構文の色を付ける（差分と同じ色）。
    private func colorSyntax(_ blame: Blame) {
        var starts: [Int] = []
        var position = 0
        for line in blame.lines {
            starts.append(position)
            position += (line.t as NSString).length + 1
        }
        let text = blame.lines.map(\.t).joined(separator: "\n")
        let path = self.path
        Task { [weak self] in
            let spans = await Task.detached(priority: .utility) {
                SyntaxSpans.highlight(path: path, text: text)?.withSpans { Array($0) } ?? []
            }.value
            guard let self, !spans.isEmpty else { return }
            lineStarts = starts
            syntax = spans
            let code = table.column(withIdentifier: .init("code"))
            if code >= 0 { table.reloadData(forRowIndexes: IndexSet(0..<table.numberOfRows), columnIndexes: [code]) }
        }
    }

    /// `row` 行目の文字に、構文の色を付けたもの。
    private func colored(_ text: String, row: Int, font: NSFont?) -> NSAttributedString {
        let result = NSMutableAttributedString(string: text, attributes: [
            .font: font ?? .monospacedSystemFont(ofSize: 12, weight: .regular), .foregroundColor: NSColor.labelColor,
        ])
        let start = lineStarts[row], end = start + result.length
        // 範囲は重ならず始まりの順なので、終わりもその順。この行にかかる最初の範囲を二分探索で探す。
        var low = 0, high = syntax.count
        while low < high {
            let mid = (low + high) / 2
            if Int(syntax[mid].end) <= start { low = mid + 1 } else { high = mid }
        }
        while low < syntax.count, Int(syntax[low].start) < end {
            let span = syntax[low]
            let from = max(Int(span.start), start) - start, to = min(Int(span.end), end) - start
            if to > from {
                result.addAttribute(.foregroundColor, value: palette[Int(span.kind) & 15], range: NSRange(location: from, length: to - from))
            }
            low += 1
        }
        return result
    }

    override func makeContent() -> NSView {
        for (id, title, width) in [("info", "Commit", 240.0), ("number", "", 48.0), ("code", "", 700.0)] {
            let column = NSTableColumn(identifier: .init(id))
            column.title = title
            column.width = width
            table.addTableColumn(column)
        }
        table.headerView = nil
        table.rowHeight = 17
        table.intercellSpacing = NSSize(width: 8, height: 0)
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.gridStyleMask = []
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(openCommit)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        message.textColor = .secondaryLabelColor
        message.translatesAutoresizingMaskIntoConstraints = false
        scroll.addSubview(message)
        NSLayoutConstraint.activate([
            message.centerXAnchor.constraint(equalTo: scroll.centerXAnchor),
            message.centerYAnchor.constraint(equalTo: scroll.centerYAnchor),
        ])
        return scroll
    }

    @objc private func openCommit() {
        let row = table.clickedRow
        guard row >= 0, let blame else { return }
        let commit = blame.commits[blame.lines[row].c]
        guard commit.isCommitted, let id = Data(hex: commit.oid) else { return }
        onSelectCommit(id)
    }

    func numberOfRows(in tableView: NSTableView) -> Int { blame?.lines.count ?? 0 }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        let view = BlameRowView()
        view.shaded = blocks[row].index % 2 == 1
        return view
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let id = tableColumn?.identifier.rawValue else { return nil }
        let field = tableView.makeView(withIdentifier: .init(id), owner: nil) as? NSTextField ?? {
            let f = NSTextField(labelWithString: "")
            f.identifier = .init(id)
            f.lineBreakMode = .byTruncatingTail
            f.font = id == "code" || id == "number"
                ? .monospacedSystemFont(ofSize: 12, weight: .regular) : .systemFont(ofSize: NSFont.smallSystemFontSize)
            if id == "number" {
                f.alignment = .right
                f.textColor = .tertiaryLabelColor
            }
            return f
        }()
        guard let blame else { return nil }
        let line = blame.lines[row]
        let commit = blame.commits[line.c]
        switch id {
        case "info":
            // まとまりの 1 行目に ID・作者・日時、2 行目に件名を出す。
            let offset = blocks[row].offset
            if !commit.isCommitted {
                field.stringValue = offset == 0 ? L("Not committed yet") : ""
            } else if offset == 0 {
                let date = fileToolDate.string(from: Date(timeIntervalSince1970: TimeInterval(commit.time)))
                field.stringValue = "\(commit.oid.prefix(8))  \(commit.author)  \(date)"
            } else if offset == 1 {
                field.stringValue = commit.summary
            } else {
                field.stringValue = ""
            }
            field.textColor = offset == 0 ? .secondaryLabelColor : .tertiaryLabelColor
            field.toolTip = commit.isCommitted ? "\(commit.summary)\n\(commit.oid)\n" + L("Double-click to show in History.") : nil
        case "number":
            field.stringValue = "\(row + 1)"
        default:
            if syntax.isEmpty || row >= lineStarts.count {
                field.stringValue = line.t
            } else {
                field.attributedStringValue = colored(line.t, row: row, font: field.font)
            }
        }
        return field
    }
}

/// まとまりごとに背景を交互に変える行。
final class BlameRowView: NSTableRowView {
    var shaded = false

    override func drawBackground(in dirtyRect: NSRect) {
        (shaded ? NSColor.labelColor.withAlphaComponent(0.05) : NSColor.clear).setFill()
        bounds.fill()
    }
}

/// ファイルの履歴: 左にファイルを変えたコミット、右にそのコミットでのファイルの差分。名前の変更も追う。
/// すぐに出し、新しい方の `firstPage` 個を先に出してから残りを足す。名前が変わっていれば、名前ごとにその時点の
/// パスの見出しを挟む（上に張り付くので、今どの名前の頃を見ているかが分かる）。
final class FileHistoryViewController: FileToolViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    /// 一覧の行。
    enum Row: Equatable {
        /// その下のコミットの時点のパス。`index` はその下の最初のコミットの番号。
        case path(String, index: Int)
        case entry(Int)
        /// 残りを読んでいる。
        case loading
    }

    /// 先に読む数（`git log -n` は Bloom フィルタがなくても早く返る）。
    static let firstPage = 100

    private let repository: Repository
    private var entries: [FileHistory.Entry] = []
    private var rows: [Row] = [.loading]
    private var complete = false
    private let table = Table()
    private let diffView: DiffViewController
    private let onSelectCommit: (Data) -> Void
    private let onBlame: (String, String) -> Void
    /// 差分を出しているコミット（読み直して選び直しても、同じなら読み直さない）。
    private var shown: String?
    private var request = 0
    /// 読み直した回数（前の読み込みの結果を捨てる）。
    private var generation = 0
    /// マージも出すか（Settings の File History。一覧の右クリックで切り替える）。
    private var merges = Settings.fileHistoryMerges
    /// 読み直す前に選んでいたコミット（読み直した一覧にあれば選び直す）。
    private var keptSelection: String?

    /// ⏎ で History へ、⌘C で ID をコピーする（表の中で）。
    private final class Table: NSTableView {
        var onReturn: (() -> Void)?
        var onCopy: (() -> Void)?
        override func keyDown(with event: NSEvent) {
            if event.keyCode == 36 || event.keyCode == 76 { onReturn?() } else { super.keyDown(with: event) }
        }
        @objc func copy(_ sender: Any?) { onCopy?() }
    }

    init(
        path: String, repository: Repository, renderer: TextRenderer,
        onSelectCommit: @escaping (Data) -> Void, onBlame: @escaping (_ path: String, _ commit: String) -> Void
    ) {
        self.repository = repository
        self.diffView = DiffViewController(renderer: renderer)
        self.onSelectCommit = onSelectCommit
        self.onBlame = onBlame
        super.init(path: path)
        addChild(diffView)
        updateTitle()
        load()
        NotificationCenter.default.addObserver(
            self, selector: #selector(mergesSettingChanged), name: .fileHistoryMergesChanged, object: nil)
    }

    override var tabTitle: String { "File History — " + super.tabTitle }

    @objc private func mergesSettingChanged() {
        guard merges != Settings.fileHistoryMerges else { return }
        merges = Settings.fileHistoryMerges
        reload()
    }

    /// マージを出す・出さないを変えたので、初めから読み直す（選んでいたコミットは、まだあれば選んだまま）。
    private func reload() {
        keptSelection = selectedEntry?.oid ?? keptSelection
        entries = []
        complete = false
        rows = [.loading]
        if isViewLoaded { table.reloadData() }
        updateTitle()
        load()
    }

    required init?(coder: NSCoder) { fatalError() }

    override func makeContent() -> NSView {
        let column = NSTableColumn(identifier: .init("commit"))
        column.resizingMask = .autoresizingMask
        column.width = 340
        table.addTableColumn(column)
        table.headerView = nil
        table.style = .inset
        table.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(openClickedCommit)
        table.onReturn = { [weak self] in self?.selectedEntry.map { self?.showInHistory($0) } }
        table.onCopy = { [weak self] in self?.selectedEntry.map { self?.copy($0.oid) } }
        let menu = NSMenu()
        menu.delegate = self
        table.menu = menu
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 360, height: 780))
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        let split = NSSplitView(frame: NSRect(x: 0, y: 0, width: 1200, height: 780))
        split.isVertical = true
        split.dividerStyle = .thin
        split.addArrangedSubview(scroll)
        diffView.view.frame = NSRect(x: 0, y: 0, width: 840, height: 780)
        split.addArrangedSubview(diffView.view)
        split.setHoldingPriority(.init(260), forSubviewAt: 0)
        diffView.show(nil, message: L("Loading…"))
        return split
    }

    private func load() {
        generation += 1
        let generation = self.generation
        Task { [weak self] in
            guard let self else { return }
            do {
                let first = try await read(limit: Self.firstPage)
                guard generation == self.generation else { return }
                show(first.entries, complete: !(first.more ?? false))
                guard !complete else { return }
                let all = try await read(limit: nil)
                guard generation == self.generation else { return }
                show(all.entries, complete: true)
            } catch {
                guard generation == self.generation else { return }
                NSLog("ファイルの履歴を読めない: \(error)")
                complete = true
                show(entries, complete: true)
                diffView.show(nil, message: error.localizedDescription)
            }
        }
    }

    private func read(limit: Int?) async throws -> FileHistory {
        var command: [String: Any] = ["op": "fileHistory", "path": path, "merges": merges]
        if let limit { command["limit"] = limit }
        let json = try Repository.json(command)
        let repository = self.repository
        return try await Task.detached(priority: .userInitiated) {
            try repository.info(json: json, as: FileHistory.self)
        }.value
    }

    /// 一覧を入れ替える。選んでいたコミットは選んだまま（見出しが増えて行がずれても）。
    private func show(_ entries: [FileHistory.Entry], complete: Bool) {
        let selected = selectedEntry?.oid ?? keptSelection
        if complete { keptSelection = nil }
        self.entries = entries
        self.complete = complete
        rows = Self.rows(for: entries.map(\.path), complete: complete)
        updateTitle()
        guard isViewLoaded else { return }
        table.reloadData()
        let row = { (oid: String?) in
            self.rows.firstIndex(where: { if case .entry(let i) = $0 { entries[i].oid == oid } else { false } })
        }
        // 選んでいたコミットが読み直した一覧になければ、読み終えたところで先頭を選ぶ。
        if let row = row(selected ?? entries.first?.oid) ?? (complete ? row(entries.first?.oid) : nil) {
            table.selectRowIndexes([row], byExtendingSelection: false)
        } else if complete, entries.isEmpty, shown == nil {
            diffView.show(nil, message: L("No commits changed this file."))
        }
    }

    /// 各コミットの時点のパスから、一覧の行を作る。名前が 1 つだけなら見出しは入れない（窓の題にある）。
    static func rows(for paths: [String], complete: Bool) -> [Row] {
        var rows: [Row] = []
        let renamed = Set(paths).count > 1
        for (i, path) in paths.enumerated() {
            if renamed, i == 0 || paths[i - 1] != path { rows.append(.path(path, index: i)) }
            rows.append(.entry(i))
        }
        if !complete { rows.append(.loading) }
        return rows
    }

    private func updateTitle() {
        heading =
            if complete { L("File History — %@ (%ld commits)", path, entries.count) }
            else if entries.isEmpty { L("File History — %@ (loading…)", path) }
            else { L("File History — %@ (%ld+ commits)", path, entries.count) }
    }

    private var selectedEntry: FileHistory.Entry? { entry(at: table.selectedRow) }

    private func entry(at row: Int) -> FileHistory.Entry? {
        guard rows.indices.contains(row), case .entry(let i) = rows[row] else { return nil }
        return entries[i]
    }

    @objc private func openClickedCommit() {
        entry(at: table.clickedRow).map(showInHistory)
    }

    private func showInHistory(_ entry: FileHistory.Entry) {
        guard let id = Data(hex: entry.oid) else { return }
        onSelectCommit(id)
    }

    private func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    // MARK: - NSMenuDelegate

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        func add(_ title: String, key: String = "", _ handler: @escaping () -> Void) {
            let item = ClosureMenuItem(title: title, action: handler)
            item.keyEquivalent = key
            item.keyEquivalentModifierMask = key == "c" ? .command : []
            menu.addItem(item)
        }
        if let entry = entry(at: table.clickedRow) {
            let short = String(entry.oid.prefix(8))
            add(L("Show in History"), key: "\r") { [weak self] in self?.showInHistory(entry) }
            add(Titles.blame(at: short)) { [weak self] in self?.onBlame(entry.path, entry.oid) }
            menu.addItem(.separator())
            add(Titles.copyCommitID, key: "c") { [weak self] in self?.copy(entry.oid) }
            add(Titles.copyShortID) { [weak self] in self?.copy(short) }
            add(Titles.copyPath) { [weak self] in self?.copy(entry.path) }
            menu.addItem(.separator())
        }
        // 既定（Settings）も変え、開いているほかの File History も読み直す。
        add(L("Show Merge Commits")) { Settings.fileHistoryMerges.toggle() }
        menu.items.last?.state = merges ? .on : .off
    }

    // MARK: - NSTableView

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(_ tableView: NSTableView, isGroupRow row: Int) -> Bool {
        if case .path = rows[row] { true } else { false }
    }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        if case .entry = rows[row] { true } else { false }
    }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        switch rows[row] {
        case .path: 24
        case .entry: 38
        case .loading: 30
        }
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        switch rows[row] {
        case .path(let path, let index):
            // ファイル名を先に（狭くても切れない）、ディレクトリを薄く後ろに。
            let name = (path as NSString).lastPathComponent
            let directory = (path as NSString).deletingLastPathComponent
            let font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize, weight: .semibold)
            let text = NSMutableAttributedString(string: name, attributes: [.font: font, .foregroundColor: NSColor.labelColor])
            if !directory.isEmpty {
                text.append(NSAttributedString(string: "  " + directory, attributes: [
                    .font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.secondaryLabelColor,
                ]))
            }
            let label = NSTextField(labelWithAttributedString: text)
            label.lineBreakMode = .byTruncatingTail
            label.toolTip = path
            // 1 つ上（新しい方）のコミットで、今の名前（の 1 つ前）に変わった。
            if index > 0 {
                let newer = entries[index - 1]
                label.toolTip = path + "\n" + L("Renamed or copied to %@ in %@", newer.path, String(newer.oid.prefix(8)))
            }
            return label
        case .entry(let i):
            let cell = tableView.makeView(withIdentifier: .init("entry"), owner: nil) as? TwoLineCell ?? TwoLineCell()
            let entry = entries[i]
            cell.top.stringValue = entry.subject
            let date = fileToolDate.string(from: Date(timeIntervalSince1970: TimeInterval(entry.time)))
            cell.bottom.stringValue = "\(entry.author)  ·  \(date)  ·  \(entry.oid.prefix(8))"
            cell.toolTip = entry.path
            return cell
        case .loading:
            let label = NSTextField(labelWithString: L("Loading…"))
            label.textColor = .secondaryLabelColor
            label.alignment = .center
            return label
        }
    }

    /// 選んだコミットの変更から、そのときのパスのファイルを探して差分を出す。
    func tableViewSelectionDidChange(_ notification: Notification) {
        guard let entry = selectedEntry, entry.oid != shown, let id = Data(hex: entry.oid) else { return }
        shown = entry.oid
        let path = entry.path
        request += 1
        let request = self.request
        let repository = self.repository
        Task {
            let diff = await Task.detached(priority: .userInitiated) { () -> FileDiff? in
                guard let changes = try? repository.changes(of: id),
                      let index = changes.items.firstIndex(where: { $0.path == path }) else { return nil }
                return try? changes.diff(at: index)
            }.value
            guard request == self.request else { return }
            if let diff, !diff.isBinary {
                diffView.show(diff)
            } else {
                diffView.show(nil, message: diff == nil ? L("Can’t read the diff") : L("Binary file"))
            }
        }
    }
}

/// 2 行のセル（上に件名、下に作者・日時）。
final class TwoLineCell: NSTableCellView {
    let top = NSTextField(labelWithString: "")
    let bottom = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        identifier = .init("entry")
        top.lineBreakMode = .byTruncatingTail
        bottom.lineBreakMode = .byTruncatingTail
        bottom.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        bottom.textColor = .secondaryLabelColor
        for v in [top, bottom] {
            v.translatesAutoresizingMaskIntoConstraints = false
            v.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            top.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            top.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -4),
            top.topAnchor.constraint(equalTo: topAnchor, constant: 3),
            bottom.leadingAnchor.constraint(equalTo: top.leadingAnchor),
            bottom.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -4),
            bottom.topAnchor.constraint(equalTo: top.bottomAnchor, constant: 1),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }
}

/// 開いている blame・ファイルの履歴のウィンドウを、閉じるまで保持する。
enum FileToolWindows {
    private static var open: [NSWindowController] = []
    static var controllers: [NSWindowController] { open }

    static func keep(_ controller: NSWindowController) {
        open.append(controller)
        NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification, object: controller.window, queue: .main
        ) { [weak controller] _ in
            open.removeAll { $0 === controller || $0.window == nil }
        }
    }
}
