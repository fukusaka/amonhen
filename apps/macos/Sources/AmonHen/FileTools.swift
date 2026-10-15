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
                field.stringValue = offset == 0 ? "まだコミットしていない" : ""
            } else if offset == 0 {
                let date = fileToolDate.string(from: Date(timeIntervalSince1970: TimeInterval(commit.time)))
                field.stringValue = "\(commit.oid.prefix(8))  \(commit.author)  \(date)"
            } else if offset == 1 {
                field.stringValue = commit.summary
            } else {
                field.stringValue = ""
            }
            field.textColor = offset == 0 ? .secondaryLabelColor : .tertiaryLabelColor
            field.toolTip = commit.isCommitted ? "\(commit.summary)\n\(commit.oid)\nダブルクリックで History に表示" : nil
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
final class FileHistoryWindowController: NSWindowController, NSTableViewDataSource, NSTableViewDelegate {
    private let history: FileHistory
    private let repository: Repository
    private let table = NSTableView()
    private let diffView: DiffViewController
    private let onSelectCommit: (Data) -> Void
    private var request = 0

    static func show(path: String, repository: Repository, renderer: TextRenderer, onSelectCommit: @escaping (Data) -> Void) {
        Task {
            do {
                let json = try Repository.json(["op": "fileHistory", "path": path])
                let history = try await Task.detached(priority: .userInitiated) {
                    try repository.info(json: json, as: FileHistory.self)
                }.value
                let controller = FileHistoryWindowController(
                    path: path, history: history, repository: repository, renderer: renderer, onSelectCommit: onSelectCommit)
                controller.showWindow(nil)
                FileToolWindows.keep(controller)
            } catch {
                NSLog("ファイルの情報を読めない: \(error)")
                NSAlert(error: error).runModal()
            }
        }
    }

    private init(
        path: String, history: FileHistory, repository: Repository, renderer: TextRenderer,
        onSelectCommit: @escaping (Data) -> Void
    ) {
        self.history = history
        self.repository = repository
        self.diffView = DiffViewController(renderer: renderer)
        self.onSelectCommit = onSelectCommit
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1200, height: 780),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "History — \(path)（\(history.entries.count) コミット）"
        window.center()
        super.init(window: window)
        build()
        if !history.entries.isEmpty {
            table.selectRowIndexes([0], byExtendingSelection: false)
        } else {
            diffView.show(nil, message: "このファイルを変えたコミットはありません")
        }
    }

    required init?(coder: NSCoder) { fatalError() }

    private func build() {
        let column = NSTableColumn(identifier: .init("commit"))
        column.resizingMask = .autoresizingMask
        column.width = 340
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 38
        table.style = .inset
        table.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(openCommit)
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

    @objc private func openCommit() {
        let row = table.clickedRow
        guard row >= 0, let id = Data(hex: history.entries[row].oid) else { return }
        onSelectCommit(id)
    }

    func numberOfRows(in tableView: NSTableView) -> Int { history.entries.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("entry"), owner: nil) as? TwoLineCell ?? TwoLineCell()
        let entry = history.entries[row]
        cell.top.stringValue = entry.subject
        let date = fileToolDate.string(from: Date(timeIntervalSince1970: TimeInterval(entry.time)))
        cell.bottom.stringValue = "\(entry.author)  ·  \(date)  ·  \(entry.oid.prefix(8))"
        cell.toolTip = entry.path
        return cell
    }

    /// 選んだコミットの変更から、そのときのパスのファイルを探して差分を出す。
    func tableViewSelectionDidChange(_ notification: Notification) {
        let row = table.selectedRow
        guard row >= 0, let id = Data(hex: history.entries[row].oid) else { return }
        let path = history.entries[row].path
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
                diffView.show(nil, message: diff == nil ? "差分を読めません" : "バイナリファイル")
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
