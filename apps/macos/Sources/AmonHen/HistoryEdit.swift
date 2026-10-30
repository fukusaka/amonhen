import AmonhenKit
import AppKit

/// History の編集モード（docs/ux.md の「History's edit mode」）。選んだコミットから HEAD までを並べ、行ごとに
/// 何をするか（そのまま・メッセージを直す・ひとつ古いコミットとまとめる・取り消す・止めて直す）を選び、ドラッグで
/// 並べ替える。Apply を押すまで何も変わらず、まとめて 1 回の rebase（コアの `rewrite` の `plan`）で行う。Undo 1 回で
/// 全部戻る。行は History と同じく新しい順（上が HEAD）。
final class HistoryEditController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    /// 行ですること。git の対話的な rebase の言葉は、ツールチップにだけ出す。
    enum Action: String, CaseIterable {
        case pick, reword, squash, fixup, drop, edit

        var title: String {
            switch self {
            case .pick: L("Keep")
            case .reword: L("Edit Message")
            case .squash: L("Combine with Older")
            case .fixup: L("Combine with Older, Keep Its Message")
            case .drop: "Drop"
            case .edit: L("Stop to Change Files")
            }
        }

        var toolTip: String {
            switch self {
            case .pick: L("Leave the commit as it is (git: pick).")
            case .reword: L("Give the commit a new message (git: reword).")
            case .squash: L("Make this and the older commit below one commit, with a message you can edit (git: squash).")
            case .fixup: L("Make this and the older commit below one commit, keeping the older one’s message (git: fixup).")
            case .drop: L("Remove the commit and its changes (git: drop).")
            case .edit: L("Stop after this commit to change its files and amend it, then Continue (git: edit).")
            }
        }
    }

    /// 1 行（1 コミット）。
    struct Row {
        let oid: String
        let message: String
        let author: String
        var action: Action = .pick
        /// 直したメッセージ（reword のとき）、またはまとめたコミットのメッセージ（squash のとき）。
        var newMessage: String?

        var subject: String {
            String((newMessage ?? message).split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false).first ?? "")
        }
    }

    /// 開くときの最初の変更（右クリックの近道・History のドラッグから）。
    enum Preset {
        case reword(String)
        case combine([String])
        case drop(String)
        /// `oid` を、新しい順の並びで `to` 番目へ。
        case move(String, Int)
    }

    private let actions: Actions
    private var rows: [Row]
    private let originalOrder: [String]
    private let table = NSTableView()
    private let summary = NSTextField(labelWithString: "")
    /// 編集モードを終える（Apply したら true）。
    var onFinish: ((Bool) -> Void)?
    /// 下のステータスバーに出す行（要約・Cancel・Apply）。
    private(set) lazy var barRow: NSView = makeBar()

    init(actions: Actions, rows: [Row]) {
        self.actions = actions
        self.rows = rows
        originalOrder = rows.map(\.oid)
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    /// `start`（16 進）から HEAD までを読み、編集モードを作る。`start` が編集できる並び（HEAD から最初の merge
    /// まで）になければ nil。
    @MainActor
    static func load(actions: Actions, from start: String, includingOlder: Bool = false) async -> HistoryEditController? {
        let chain = await actions.editableCommits()
        guard var end = chain.firstIndex(where: { $0.hasPrefix(start) || start.hasPrefix($0) }) else { return nil }
        if includingOlder {
            guard end + 1 < chain.count else { return nil }
            end += 1
        }
        let oids = Array(chain[...end])
        let repository = actions.repository
        let rows = await Task.detached { () -> [Row] in
            oids.map { oid in
                let message = (try? repository.run(["op": "commitMessage", "oid": oid]).text) ?? nil
                let author = (try? repository.commitInfo(oid: oid))?.author ?? ""
                return Row(oid: oid, message: message ?? "", author: author)
            }
        }.value
        return HistoryEditController(actions: actions, rows: rows)
    }

    override func loadView() {
        let column = NSTableColumn(identifier: .init("row"))
        table.addTableColumn(column)
        table.headerView = nil
        table.style = .inset
        table.rowHeight = 40
        table.dataSource = self
        table.delegate = self
        table.registerForDraggedTypes([.editRow])
        table.draggingDestinationFeedbackStyle = .gap
        table.target = self
        table.doubleAction = #selector(editMessageOfClickedRow)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        let heading = NSTextField(labelWithString: L("Edit Commits"))
        heading.font = .boldSystemFont(ofSize: 15)
        let hint = NSTextField(wrappingLabelWithString: L("Choose what to do with each commit and drag rows to reorder them. Nothing changes until Apply Changes."))
        hint.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        hint.textColor = .secondaryLabelColor
        let top = NSStackView(views: [heading, hint])
        top.orientation = .vertical
        top.alignment = .leading
        top.spacing = 2
        let view = NSView()
        for v in [top, scroll] {
            v.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(v)
        }
        NSLayoutConstraint.activate([
            top.topAnchor.constraint(equalTo: view.topAnchor, constant: 12),
            top.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 16),
            top.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -16),
            scroll.topAnchor.constraint(equalTo: top.bottomAnchor, constant: 8),
            scroll.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        self.view = view
        update()
    }

    /// 最初の変更を入れる（メッセージを直すときは、そのままメッセージの欄を開く）。
    func apply(_ preset: Preset) {
        switch preset {
        case let .reword(oid):
            if let i = rows.firstIndex(where: { $0.oid == oid }) { editMessage(at: i) }
        case let .combine(oids):
            // 選んだもののうちいちばん古いものに、ほかをまとめる。
            let indices = oids.compactMap { oid in rows.firstIndex { $0.oid == oid } }.sorted()
            for i in indices.dropLast() { set(.squash, at: i) }
        case let .drop(oid):
            if let i = rows.firstIndex(where: { $0.oid == oid }) { set(.drop, at: i) }
        case let .move(oid, to):
            if let from = rows.firstIndex(where: { $0.oid == oid }), rows.indices.contains(to) {
                rows.insert(rows.remove(at: from), at: to)
            }
        }
        table.reloadData()
        update()
    }

    // MARK: - 一覧

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let r = rows[row]
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.controlSize = .small
        popup.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        // いちばん古い行は、まとめる相手がない。
        let oldest = row == rows.count - 1
        for action in Action.allCases where !(oldest && [.squash, .fixup].contains(action)) {
            popup.addItem(withTitle: action.title)
            popup.lastItem?.representedObject = action.rawValue
            popup.lastItem?.toolTip = action.toolTip
        }
        popup.selectItem(withTitle: r.action.title)
        popup.tag = row
        popup.target = self
        popup.action = #selector(actionChanged(_:))
        popup.widthAnchor.constraint(equalToConstant: 230).isActive = true
        let subject = NSTextField(labelWithString: r.subject)
        subject.lineBreakMode = .byTruncatingTail
        subject.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let attributes: [NSAttributedString.Key: Any] = switch r.action {
        case .drop: [.strikethroughStyle: NSUnderlineStyle.single.rawValue, .foregroundColor: NSColor.tertiaryLabelColor]
        case .squash, .fixup: [.foregroundColor: NSColor.secondaryLabelColor]
        default: [:]
        }
        subject.attributedStringValue = NSAttributedString(string: r.subject, attributes: attributes)
        if r.newMessage != nil, r.action == .reword || r.action == .squash {
            subject.font = .systemFont(ofSize: NSFont.systemFontSize).withTraits(.italic)
        }
        let detail = NSTextField(labelWithString: "\(r.oid.prefix(Titles.shortIDLength)) · \(r.author)")
        detail.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        detail.textColor = .secondaryLabelColor
        let texts = NSStackView(views: [subject, detail])
        texts.orientation = .vertical
        texts.alignment = .leading
        texts.spacing = 1
        // まとめる行は、まとめる先（下の行）へ向かう印を付ける。
        let mark = NSImageView(image: NSImage(
            systemSymbolName: r.action == .squash || r.action == .fixup ? "arrow.turn.right.down" : "circle.fill",
            accessibilityDescription: nil) ?? NSImage())
        mark.symbolConfiguration = .init(pointSize: r.action == .squash || r.action == .fixup ? 11 : 6, weight: .regular)
        mark.contentTintColor = r.action == .drop ? .tertiaryLabelColor : .controlAccentColor
        mark.widthAnchor.constraint(equalToConstant: 16).isActive = true
        let stack = NSStackView(views: [mark, texts, NSView(), popup])
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 8)
        return stack
    }

    @objc private func actionChanged(_ sender: NSPopUpButton) {
        guard let raw = sender.selectedItem?.representedObject as? String, let action = Action(rawValue: raw),
              rows.indices.contains(sender.tag) else { return }
        if action == .reword {
            editMessage(at: sender.tag)
            return
        }
        set(action, at: sender.tag)
        table.reloadData()
        update()
    }

    private func set(_ action: Action, at i: Int) {
        rows[i].action = action
        if action == .pick || action == .drop || action == .fixup || action == .edit { rows[i].newMessage = nil }
    }

    @objc private func editMessageOfClickedRow() {
        guard table.clickedRow >= 0 else { return }
        editMessage(at: table.clickedRow)
    }

    /// メッセージの欄を開く。まとめる行なら、まとめたコミットのメッセージ（最初は、まとめるものを古い順につないだもの）。
    private func editMessage(at i: Int) {
        let combining = rows[i].action == .squash
        let initial = rows[i].newMessage ?? (combining ? combinedMessage(endingAt: i) : rows[i].message)
        Task { @MainActor in
            guard let edited = await actions.editMessage(combining ? L("Message of the Combined Commit") : L("Edit Message"),
                                                         initial: initial, ok: L("Change")) else {
                table.reloadData()
                return
            }
            if !combining { rows[i].action = .reword }
            rows[i].newMessage = edited
            table.reloadData()
            update()
        }
    }

    /// `i` の行（squash）までのまとまりのメッセージを、古い順につないだもの（git の squash の既定と同じ）。
    private func combinedMessage(endingAt i: Int) -> String {
        // 新しい順の並びで、i から下（古い方）へ、まとめる先の行まで。
        var j = i
        while j + 1 < rows.count, rows[j].action == .squash || rows[j].action == .fixup { j += 1 }
        return rows[i...j].reversed().filter { $0.action != .fixup }.map { $0.newMessage ?? $0.message }
            .joined(separator: "\n\n")
    }

    // MARK: - ドラッグで並べ替え

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> (any NSPasteboardWriting)? {
        let item = NSPasteboardItem()
        item.setString(String(row), forType: .editRow)
        return item
    }

    func tableView(_ tableView: NSTableView, validateDrop info: any NSDraggingInfo, proposedRow row: Int,
                   proposedDropOperation op: NSTableView.DropOperation) -> NSDragOperation {
        tableView.setDropRow(row, dropOperation: .above)
        return .move
    }

    func tableView(_ tableView: NSTableView, acceptDrop info: any NSDraggingInfo, row: Int,
                   dropOperation: NSTableView.DropOperation) -> Bool {
        guard let from = info.draggingPasteboard.string(forType: .editRow).flatMap(Int.init), rows.indices.contains(from) else {
            return false
        }
        let moved = rows.remove(at: from)
        rows.insert(moved, at: from < row ? row - 1 : row)
        // いちばん古い行には、まとめる相手がない。
        if let last = rows.indices.last, [.squash, .fixup].contains(rows[last].action) { set(.pick, at: last) }
        table.reloadData()
        update()
        return true
    }

    // MARK: - 要約と実行

    private func makeBar() -> NSView {
        let icon = NSImageView(image: NSImage(systemSymbolName: "pencil.and.list.clipboard", accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = .controlAccentColor
        summary.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        summary.lineBreakMode = .byTruncatingTail
        summary.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(cancel))
        cancel.keyEquivalent = "\u{1b}"
        let apply = NSButton(title: L("Apply Changes"), target: self, action: #selector(applyClicked))
        apply.keyEquivalent = "\r"
        return StatusBarController.row([icon, summary, NSView(), cancel, apply], height: 34,
                                       tint: NSColor.controlAccentColor.withAlphaComponent(0.08))
    }

    private var hasChanges: Bool {
        rows.map(\.oid) != originalOrder || rows.contains { $0.action != .pick }
    }

    private func update() {
        let count = { (a: Action) in self.rows.filter { $0.action == a }.count }
        var parts: [String] = []
        if rows.map(\.oid) != originalOrder { parts.append(L("reordered")) }
        if count(.reword) > 0 { parts.append(L("%ld messages", count(.reword))) }
        if count(.squash) + count(.fixup) > 0 { parts.append(L("%ld combined", count(.squash) + count(.fixup))) }
        if count(.drop) > 0 { parts.append(L("%ld dropped", count(.drop))) }
        if count(.edit) > 0 { parts.append(L("%ld to change files", count(.edit))) }
        summary.stringValue = parts.isEmpty ? L("No changes yet.")
            : L("Changes: %@.", ListFormatter.localizedString(byJoining: parts)) + " " + Actions.canUndo
    }

    @objc private func cancel() { onFinish?(false) }

    @objc private func applyClicked() {
        guard hasChanges else {
            onFinish?(false)
            return
        }
        // 古い順にし、まとまり（squash を含む）の最後の squash に、まとめたメッセージを付ける。
        var steps: [[String: Any]] = []
        let ordered = Array(rows.reversed())
        for (i, row) in ordered.enumerated() {
            var step: [String: Any] = ["oid": row.oid, "action": row.action.rawValue]
            switch row.action {
            case .reword: step["message"] = row.newMessage ?? row.message
            case .squash:
                let next = ordered.indices.contains(i + 1) ? ordered[i + 1].action : nil
                let lastSquash = !ordered[(i + 1)...].prefix { $0.action == .squash || $0.action == .fixup }
                    .contains { $0.action == .squash }
                if lastSquash || next == nil {
                    step["message"] = row.newMessage ?? combinedMessage(endingAt: rows.count - 1 - i)
                }
            default: break
            }
            steps.append(step)
        }
        actions.rewrite(L("Edit %ld Commits", rows.count), ["kind": "plan", "steps": steps])
        onFinish?(true)
    }
}

extension NSPasteboard.PasteboardType {
    /// 編集モードの行のドラッグ（行の番号）。
    static let editRow = NSPasteboard.PasteboardType("io.github.fukusaka.amonhen.edit-row")
}

private extension NSFont {
    func withTraits(_ traits: NSFontDescriptor.SymbolicTraits) -> NSFont {
        NSFont(descriptor: fontDescriptor.withSymbolicTraits(traits), size: pointSize) ?? self
    }
}
