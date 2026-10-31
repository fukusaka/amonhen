import AmonhenKit
import AppKit

/// Push の確認のシート（ウィンドウに付く。大きさは変えられる）。上に送り先と注意、左に送るコミット（一番上は
/// 「送る変更すべて」。Force Push と分岐では上流から消えるコミットも）、右に選んだものの詳細（右の欄と同じ表示:
/// メッセージ・変更ファイル・差分。読み取り専用）、下にキャンセルと実行（と、分岐なら Pull…）。↩ で実行（Force Push は
/// ↩ では押せない）、Esc でキャンセル。
final class PushSheet: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    /// 押されたボタン。
    enum Choice { case cancel, ok, other }

    /// 左の一覧の 1 行。`commit` が nil なら「送る変更すべて」、`heading` なら選べない見出し。
    private struct Row {
        let title: String
        let detail: String
        let commit: Data?
        let heading: Bool
        let lost: Bool
    }

    private let window: NSWindow
    private let table = NSTableView()
    private let detail: CommitDetailViewController
    private let rows: [Row]
    private let range: (base: Data, tip: Data)
    private let rangeTitle: String
    private let rangeLines: [String]

    /// `commits` は送るもの、`lost` は Force Push で上流から消えるもの（赤）。`warnings` は上に黄色で出す注意。
    /// `other` は Cancel と実行の間に置く 3 つ目のボタン（分岐のときの Pull…）。
    init(
        title: String, message: String, warnings: [String], commits: CommitRange, lost: CommitRange?,
        range: (base: Data, tip: Data), detail: CommitDetailViewController, ok: String, destructive: Bool,
        other: String? = nil
    ) {
        self.detail = detail
        detail.isPreview = true
        self.range = range
        rangeTitle = L("All changes to send (%ld)", commits.count)
        func line(_ c: CommitRange.Commit) -> String { "\(c.oid.prefix(8))  \(c.subject)  — \(c.author)" }
        rangeLines = commits.commits.map(line)
        var rows = [Row(title: rangeTitle, detail: L("From upstream to HEAD, all files"), commit: nil, heading: false, lost: false)]
        func add(_ c: CommitRange.Commit, lost: Bool) {
            let when = Self.relativeFormatter.localizedString(for: Date(timeIntervalSince1970: TimeInterval(c.time)), relativeTo: Date())
            rows.append(Row(title: c.subject, detail: "\(c.oid.prefix(8)) · \(c.author) · \(when)", commit: Self.data(hex: c.oid), heading: false, lost: lost))
        }
        commits.commits.forEach { add($0, lost: false) }
        if let lost, lost.count > 0 {
            rows.append(Row(title: L("Commits upstream will lose (%ld)", lost.count), detail: "", commit: nil, heading: true, lost: true))
            lost.commits.forEach { add($0, lost: true) }
        }
        self.rows = rows

        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1100, height: 720), styleMask: [.titled, .closable, .resizable],
            backing: .buffered, defer: false)
        window.minSize = NSSize(width: 820, height: 480)
        window.title = title
        window.isReleasedWhenClosed = false
        super.init()

        // 上: 題と説明、注意。
        let titleLabel = NSTextField(labelWithString: title)
        titleLabel.font = .boldSystemFont(ofSize: 15)
        let messageLabel = NSTextField(wrappingLabelWithString: message)
        messageLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        messageLabel.textColor = .secondaryLabelColor
        let top = NSStackView(views: [titleLabel] + (message.isEmpty ? [] : [messageLabel]))
        top.orientation = .vertical
        top.alignment = .leading
        top.spacing = 4
        for warning in warnings {
            let label = NSTextField(wrappingLabelWithString: "⚠︎ " + warning)
            label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            label.textColor = .systemOrange
            top.addArrangedSubview(label)
        }

        // 左: コミットの一覧。右: 詳細の欄。
        let column = NSTableColumn(identifier: .init("commit"))
        table.addTableColumn(column)
        table.headerView = nil
        table.style = .inset
        table.rowHeight = 36
        table.dataSource = self
        table.delegate = self
        table.allowsEmptySelection = false
        let left = NSScrollView()
        left.documentView = table
        left.hasVerticalScroller = true
        left.borderType = .noBorder
        let split = NSSplitView()
        split.isVertical = true
        split.dividerStyle = .thin
        split.addArrangedSubview(left)
        split.addArrangedSubview(detail.view)
        split.setHoldingPriority(.defaultLow + 1, forSubviewAt: 0)
        left.widthAnchor.constraint(greaterThanOrEqualToConstant: 240).isActive = true

        // 下: ボタン。
        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(cancelClicked))
        cancel.keyEquivalent = "\u{1b}"
        let okButton = NSButton(title: ok, target: self, action: #selector(okClicked))
        // 消えるものがある実行（Force Push）は ↩ では押せない。分岐のときは ↩ が Pull…。
        okButton.keyEquivalent = destructive ? "" : "\r"
        okButton.hasDestructiveAction = destructive
        let otherButton = other.map { NSButton(title: $0, target: self, action: #selector(otherClicked)) }
        if destructive { otherButton?.keyEquivalent = "\r" }
        let buttons = NSStackView(views: [NSView(), cancel] + (otherButton.map { [$0] } ?? []) + [okButton])
        buttons.orientation = .horizontal
        buttons.distribution = .fill
        buttons.spacing = 12

        // 上の帯は上に、ボタンは下に、分割部分がその間をすべて使う（積み重ねの伸び方に頼らない）。
        let content = NSView()
        for v in [top, split, buttons] {
            v.translatesAutoresizingMaskIntoConstraints = false
            content.addSubview(v)
        }
        NSLayoutConstraint.activate([
            top.topAnchor.constraint(equalTo: content.topAnchor, constant: 16),
            top.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 20),
            top.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -20),
            split.topAnchor.constraint(equalTo: top.bottomAnchor, constant: 12),
            split.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 20),
            split.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -20),
            split.bottomAnchor.constraint(equalTo: buttons.topAnchor, constant: -12),
            buttons.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 20),
            buttons.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -20),
            buttons.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -16),
        ])
        top.setHuggingPriority(.required, for: .vertical)
        buttons.setHuggingPriority(.required, for: .vertical)
        window.contentView = content
        window.initialFirstResponder = table
    }

    /// `parent` にシートで付け、押されたボタンを返す。大きさは親の 85%（シートなので、ほかの窓は止めない）。
    @MainActor
    func present(in parent: NSWindow) async -> Choice {
        table.reloadData()
        table.selectRowIndexes([0], byExtendingSelection: false)
        let size = NSSize(width: max(820, parent.frame.width * 0.85), height: max(480, parent.frame.height * 0.85))
        window.setContentSize(size)
        let response = await parent.beginSheet(window)
        return switch response {
        case .OK: .ok
        case .continue: .other
        default: .cancel
        }
    }

    private func finish(_ response: NSApplication.ModalResponse) {
        window.sheetParent?.endSheet(window, returnCode: response)
    }

    @objc private func okClicked() { finish(.OK) }
    @objc private func otherClicked() { finish(.continue) }
    @objc private func cancelClicked() { finish(.cancel) }

    // MARK: - 一覧

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(_ tableView: NSTableView, isGroupRow row: Int) -> Bool { rows[row].heading }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool { !rows[row].heading }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat { rows[row].heading ? 24 : 36 }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let r = rows[row]
        let id = NSUserInterfaceItemIdentifier(r.heading ? "heading" : "commit")
        let cell = tableView.makeView(withIdentifier: id, owner: nil) as? NSTableCellView ?? {
            let cell = NSTableCellView()
            cell.identifier = id
            let title = NSTextField(labelWithString: "")
            title.lineBreakMode = .byTruncatingTail
            title.translatesAutoresizingMaskIntoConstraints = false
            cell.addSubview(title)
            cell.textField = title
            if r.heading {
                title.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
                NSLayoutConstraint.activate([
                    title.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 4),
                    title.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
                    title.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
                ])
            } else {
                let detail = NSTextField(labelWithString: "")
                detail.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
                detail.textColor = .secondaryLabelColor
                detail.lineBreakMode = .byTruncatingTail
                detail.translatesAutoresizingMaskIntoConstraints = false
                detail.identifier = .init("detail")
                cell.addSubview(detail)
                NSLayoutConstraint.activate([
                    title.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 4),
                    title.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
                    title.topAnchor.constraint(equalTo: cell.topAnchor, constant: 3),
                    detail.leadingAnchor.constraint(equalTo: title.leadingAnchor),
                    detail.trailingAnchor.constraint(equalTo: title.trailingAnchor),
                    detail.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 1),
                ])
            }
            return cell
        }()
        cell.textField?.stringValue = r.title
        cell.textField?.textColor = r.lost && !r.heading ? .systemRed : r.heading ? .systemRed : .labelColor
        cell.textField?.font = r.commit == nil && !r.heading ? .boldSystemFont(ofSize: NSFont.systemFontSize) : r.heading ? .boldSystemFont(ofSize: NSFont.smallSystemFontSize) : .systemFont(ofSize: NSFont.systemFontSize)
        (cell.subviews.first { $0.identifier == .init("detail") } as? NSTextField)?.stringValue = r.detail
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard table.selectedRow >= 0 else { return }
        let r = rows[table.selectedRow]
        if let commit = r.commit {
            detail.show(commit: commit)
        } else {
            detail.show(range: range.base, to: range.tip, title: rangeTitle, lines: rangeLines)
        }
    }

    // MARK: - 部品

    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f
    }()

    /// 16 進のコミット ID（40 桁）を 20 バイトに。
    static func data(hex: String) -> Data? {
        let chars = Array(hex.utf8)
        guard chars.count == 40 else { return nil }
        var out = Data(capacity: 20)
        for i in stride(from: 0, to: 40, by: 2) {
            guard let byte = UInt8(String(decoding: chars[i..<i + 2], as: UTF8.self), radix: 16) else { return nil }
            out.append(byte)
        }
        return out
    }
}
