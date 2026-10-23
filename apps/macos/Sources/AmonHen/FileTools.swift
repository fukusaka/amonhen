import AmonhenKit
import AppKit

private let fileToolDate: DateFormatter = {
    let f = DateFormatter()
    f.dateStyle = .medium
    f.timeStyle = .short
    return f
}()

/// blame: ファイルの各行を最後に変えたコミット。同じコミットの行をまとめ、まとまりの先頭にコミットを出す。
/// 行をダブルクリックすると、そのコミットを History で選ぶ。
final class BlameWindowController: NSWindowController, NSTableViewDataSource, NSTableViewDelegate {
    private let blame: Blame
    /// 行ごとの、まとまりの番号（背景を交互に変える）と、まとまりの中で何行目か。
    private var blocks: [(index: Int, offset: Int)] = []
    private let table = NSTableView()
    private let onSelectCommit: (Data) -> Void

    static func show(path: String, rev: String?, repository: Repository, onSelectCommit: @escaping (Data) -> Void) {
        Task {
            do {
                var command: [String: Any] = ["op": "blame", "path": path]
                if let rev { command["rev"] = rev }
                let json = try Repository.json(command)
                let blame = try await Task.detached(priority: .userInitiated) {
                    try repository.info(json: json, as: Blame.self)
                }.value
                let controller = BlameWindowController(
                    blame: blame, title: "Blame — \(path)" + (rev.map { " @ \($0.prefix(8))" } ?? ""),
                    onSelectCommit: onSelectCommit)
                controller.showWindow(nil)
                FileToolWindows.keep(controller)
            } catch {
                NSLog("ファイルの情報を読めない: \(error)")
                NSAlert(error: error).runModal()
            }
        }
    }

    private init(blame: Blame, title: String, onSelectCommit: @escaping (Data) -> Void) {
        self.blame = blame
        self.onSelectCommit = onSelectCommit
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1100, height: 760),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = title
        window.center()
        super.init(window: window)
        var index = -1, offset = 0, previous = -1
        for line in blame.lines {
            if line.c != previous {
                index += 1
                offset = 0
                previous = line.c
            } else {
                offset += 1
            }
            blocks.append((index, offset))
        }
        build()
    }

    required init?(coder: NSCoder) { fatalError() }

    private func build() {
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
        window?.contentView = scroll
    }

    @objc private func openCommit() {
        let row = table.clickedRow
        guard row >= 0 else { return }
        let commit = blame.commits[blame.lines[row].c]
        guard commit.isCommitted, let id = Data(hex: commit.oid) else { return }
        onSelectCommit(id)
    }

    func numberOfRows(in tableView: NSTableView) -> Int { blame.lines.count }

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
            field.stringValue = line.t
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
/// 窓はすぐに開き、新しい方の `firstPage` 個を先に出してから残りを足す。名前が変わっていれば、名前ごとにその時点の
/// パスの見出しを挟む（上に張り付くので、今どの名前の頃を見ているかが分かる）。
final class FileHistoryWindowController: NSWindowController, NSTableViewDataSource, NSTableViewDelegate {
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

    private let path: String
    private let repository: Repository
    private var entries: [FileHistory.Entry] = []
    private var rows: [Row] = [.loading]
    private var complete = false
    private let table = NSTableView()
    private let diffView: DiffViewController
    private let onSelectCommit: (Data) -> Void
    /// 差分を出しているコミット（読み直して選び直しても、同じなら読み直さない）。
    private var shown: String?
    private var request = 0

    static func show(path: String, repository: Repository, renderer: TextRenderer, onSelectCommit: @escaping (Data) -> Void) {
        let controller = FileHistoryWindowController(
            path: path, repository: repository, renderer: renderer, onSelectCommit: onSelectCommit)
        controller.showWindow(nil)
        FileToolWindows.keep(controller)
        controller.load()
    }

    private init(path: String, repository: Repository, renderer: TextRenderer, onSelectCommit: @escaping (Data) -> Void) {
        self.path = path
        self.repository = repository
        self.diffView = DiffViewController(renderer: renderer)
        self.onSelectCommit = onSelectCommit
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1200, height: 780),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.center()
        super.init(window: window)
        build()
        updateTitle()
        diffView.show(nil, message: L("Loading…"))
    }

    required init?(coder: NSCoder) { fatalError() }

    private func build() {
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
        window?.contentView = split
    }

    private func load() {
        Task { [weak self] in
            guard let self else { return }
            do {
                let first = try await read(limit: Self.firstPage)
                show(first.entries, complete: !(first.more ?? false))
                guard !complete, window?.isVisible == true else { return }
                show(try await read(limit: nil).entries, complete: true)
            } catch {
                NSLog("ファイルの履歴を読めない: \(error)")
                complete = true
                show(entries, complete: true)
                diffView.show(nil, message: error.localizedDescription)
            }
        }
    }

    private func read(limit: Int?) async throws -> FileHistory {
        var command: [String: Any] = ["op": "fileHistory", "path": path]
        if let limit { command["limit"] = limit }
        let json = try Repository.json(command)
        let repository = self.repository
        return try await Task.detached(priority: .userInitiated) {
            try repository.info(json: json, as: FileHistory.self)
        }.value
    }

    /// 一覧を入れ替える。選んでいたコミットは選んだまま（見出しが増えて行がずれても）。
    private func show(_ entries: [FileHistory.Entry], complete: Bool) {
        let selected = selectedEntry?.oid
        self.entries = entries
        self.complete = complete
        rows = Self.rows(for: entries.map(\.path), complete: complete)
        table.reloadData()
        updateTitle()
        let oid = selected ?? entries.first?.oid
        if let row = rows.firstIndex(where: { if case .entry(let i) = $0 { entries[i].oid == oid } else { false } }) {
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
        window?.title =
            if complete { L("History — %@ (%ld commits)", path, entries.count) }
            else if entries.isEmpty { L("History — %@ (loading…)", path) }
            else { L("History — %@ (%ld+ commits)", path, entries.count) }
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

    static func keep(_ controller: NSWindowController) {
        open.append(controller)
        NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification, object: controller.window, queue: .main
        ) { [weak controller] _ in
            open.removeAll { $0 === controller || $0.window == nil }
        }
    }
}
