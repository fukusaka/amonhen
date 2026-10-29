import AmonhenKit
import AppKit

/// Range Diff: 同じ一連のコミットの 2 つの版（rebase の前と後など）を比べる（`git range-diff`）。上に比べる範囲、
/// 左に組（同じ・変わった・消えた・足された）の一覧、右に選んだ組の差分の差分（変わった組）か、そのコミットの中身。
final class RangeDiffViewController: NSSplitViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate,
    NSTextFieldDelegate
{
    /// コミットを History で選ぶ。
    var onShowInHistory: ((Data) -> Void)?
    /// 閉じる（サイドバーから Range Diff の行を消す）。
    var onClose: (() -> Void)?

    let detail: CommitDetailViewController
    private let repository: Repository
    private let diffView: DiffViewController
    private let rangesField = NSTextField()
    private let compareButton = NSButton()
    private let summary = NSTextField(labelWithString: "")
    private let counts = NSTextField(labelWithString: "")
    private let changesOnly = NSButton()
    private let table = NSTableView()
    private let pairHeader = NSTextField(labelWithString: "")
    /// 右の欄（変わった組は差分の差分、ほかはコミットの中身）。
    private let right = NSView()
    private let pairPane = NSView()
    private var pairs: [RangePair] = []
    /// 一覧に出している組（`changesOnly` なら同じものを除く）。
    private var shown: [RangePair] = []
    private var request = 0

    init(repository: Repository, renderer: TextRenderer) {
        self.repository = repository
        detail = CommitDetailViewController(repository: repository, renderer: renderer)
        diffView = DiffViewController(renderer: renderer)
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        splitView.isVertical = true
        let list = NSViewController()
        list.view = makeList()
        let listItem = NSSplitViewItem(viewController: list)
        listItem.minimumThickness = 320
        listItem.holdingPriority = .init(260)
        addSplitViewItem(listItem)
        // 差分と詳細は右の欄の子にする（分割の画面の子にすると、分割の欄が増える）。
        let rightController = NSViewController()
        rightController.view = makeRight()
        rightController.addChild(diffView)
        rightController.addChild(detail)
        let rightItem = NSSplitViewItem(viewController: rightController)
        rightItem.minimumThickness = 420
        addSplitViewItem(rightItem)
    }

    private func makeList() -> NSView {
        rangesField.placeholderString = L("old...new  or  base old new")
        rangesField.font = .monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        rangesField.controlSize = .small
        rangesField.delegate = self
        rangesField.toolTip = L("The ranges to compare, written as for git range-diff")
        rangesField.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        compareButton.title = L("Compare")
        compareButton.bezelStyle = .push
        compareButton.controlSize = .small
        compareButton.target = self
        compareButton.action = #selector(compare)
        let close = NSButton(image: NSImage(systemSymbolName: "xmark", accessibilityDescription: L("Close"))!,
                             target: self, action: #selector(closeView))
        close.isBordered = false
        close.toolTip = L("Close Range Diff")
        let top = NSStackView(views: [rangesField, compareButton, close])
        top.spacing = 6

        for field in [summary, counts] {
            field.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            field.textColor = .secondaryLabelColor
            field.lineBreakMode = .byTruncatingTail
            field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }
        changesOnly.setButtonType(.switch)
        changesOnly.title = L("Changes Only")
        changesOnly.controlSize = .small
        changesOnly.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        changesOnly.target = self
        changesOnly.action = #selector(filter)
        changesOnly.toolTip = L("Hide the commits that are the same in both versions")
        let second = NSStackView(views: [counts, NSView(), changesOnly])
        second.spacing = 6

        let bar = NSStackView(views: [top, summary, second])
        bar.orientation = .vertical
        bar.alignment = .leading
        bar.spacing = 4
        bar.edgeInsets = NSEdgeInsets(top: 6, left: 8, bottom: 6, right: 8)
        for view in [top, summary, second] {
            view.translatesAutoresizingMaskIntoConstraints = false
            view.widthAnchor.constraint(equalTo: bar.widthAnchor, constant: -16).isActive = true
        }

        let column = NSTableColumn(identifier: .init("pair"))
        table.addTableColumn(column)
        table.headerView = nil
        table.style = .inset
        table.rowHeight = 40
        table.usesAutomaticRowHeights = false
        table.dataSource = self
        table.delegate = self
        let menu = NSMenu()
        menu.delegate = self
        table.menu = menu
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false

        let stack = NSStackView(views: [bar, scroll])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 0
        for view in [bar, scroll] {
            view.translatesAutoresizingMaskIntoConstraints = false
            view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
        return stack
    }

    private func makeRight() -> NSView {
        pairHeader.font = .systemFont(ofSize: NSFont.systemFontSize)
        pairHeader.lineBreakMode = .byTruncatingTail
        pairHeader.maximumNumberOfLines = 2
        pairHeader.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let header = NSStackView(views: [pairHeader])
        header.edgeInsets = NSEdgeInsets(top: 8, left: 10, bottom: 8, right: 10)
        let stack = NSStackView(views: [header, diffView.view])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 0
        for view in [header, diffView.view] {
            view.translatesAutoresizingMaskIntoConstraints = false
            view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
        fill(pairPane, with: stack)
        fill(right, with: pairPane)
        fill(right, with: detail.view)
        showPair(nil)
        return right
    }

    private func fill(_ container: NSView, with view: NSView) {
        view.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(view)
        NSLayoutConstraint.activate([
            view.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            view.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            view.topAnchor.constraint(equalTo: container.topAnchor),
            view.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])
    }

    // MARK: - 比べる

    /// `branch`（nil なら今のブランチ）を、最後に書き換える前の版と比べる。
    func open(previousOf branch: String?) {
        loadViewIfNeeded()
        request += 1
        let request = self.request
        let repository = self.repository
        summary.stringValue = L("Looking for the previous version…")
        Task {
            let result = await Task.detached(priority: .userInitiated) {
                Result { try repository.previousVersion(branch: branch) }
            }.value
            guard request == self.request else { return }
            switch result {
            case .success(let previous?):
                let when = Self.relative.localizedString(
                    for: Date(timeIntervalSince1970: TimeInterval(previous.time)), relativeTo: Date())
                open(ranges: previous.ranges,
                     summary: L("%1$@ → %2$@ · %3$@ · %4$@", previous.selector, previous.branch, previous.message, when))
            case .success(nil):
                show([], ranges: [], summary: L("%@ hasn’t been rewritten (nothing in its reflog)", branch ?? "HEAD"))
            case .failure(let error):
                show([], ranges: [], summary: error.localizedDescription)
            }
        }
    }

    /// `ranges`（git range-diff と同じ形）で比べる。
    func open(ranges: [String], summary text: String? = nil) {
        loadViewIfNeeded()
        request += 1
        let request = self.request
        let repository = self.repository
        rangesField.stringValue = ranges.map(Self.short).joined(separator: " ")
        summary.stringValue = text ?? ""
        counts.stringValue = L("Comparing…")
        Task {
            let result = await Task.detached(priority: .userInitiated) { Result { try repository.rangeDiff(ranges) } }.value
            guard request == self.request else { return }
            switch result {
            case .success(let pairs): show(pairs, ranges: ranges, summary: text)
            case .failure(let error): show([], ranges: ranges, summary: error.localizedDescription)
            }
        }
    }

    /// 40 桁の ID は短くして入力欄に出す（そのまま比べ直せる長さに）。
    private static func short(_ range: String) -> String {
        range.replacing(/[0-9a-f]{40}/) { String($0.output.prefix(12)) }
    }

    @objc private func compare() {
        let ranges = rangesField.stringValue.split(whereSeparator: \.isWhitespace).map(String.init)
        guard !ranges.isEmpty else { return }
        open(ranges: ranges)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.insertNewline(_:)) else { return false }
        compare()
        return true
    }

    @objc private func closeView() { onClose?() }

    private func show(_ next: [RangePair], ranges: [String], summary text: String?) {
        pairs = next
        summary.stringValue = text ?? ""
        summary.toolTip = ranges.isEmpty ? nil : "git range-diff " + ranges.joined(separator: " ")
        var count: [RangePair.Status: Int] = [:]
        for p in next { count[p.status, default: 0] += 1 }
        counts.stringValue = next.isEmpty && !ranges.isEmpty
            ? L("No commits to compare")
            : [(RangePair.Status.same, L("%ld same")), (.changed, L("%ld changed")), (.removed, L("%ld removed")),
               (.added, L("%ld added"))]
                .compactMap { status, format in count[status].map { String(format: format, $0) } }
                .joined(separator: " · ")
        filter()
    }

    @objc private func filter() {
        let selected = shown.indices.contains(table.selectedRow) ? shown[table.selectedRow] : nil
        shown = changesOnly.state == .on ? pairs.filter { $0.status != .same } : pairs
        table.reloadData()
        let row = selected.flatMap { s in shown.firstIndex(of: s) }
            ?? shown.firstIndex { $0.status != .same } ?? (shown.isEmpty ? nil : 0)
        if let row {
            table.selectRowIndexes([row], byExtendingSelection: false)
            table.scrollRowToVisible(row)
        }
        showPair(row.map { shown[$0] })
    }

    // MARK: - 右の欄

    private func showPair(_ pair: RangePair?) {
        guard let pair else {
            pairPane.isHidden = false
            detail.view.isHidden = true
            pairHeader.stringValue = ""
            diffView.show(nil, message: L("Choose two versions to compare"))
            return
        }
        // 変わった組は差分の差分。ほかはコミットそのもの（消えたものは古い版、ほかは新しい版）。
        guard pair.status == .changed else {
            pairPane.isHidden = true
            detail.view.isHidden = false
            detail.show(commit: (pair.new ?? pair.old).flatMap { Data(hex: $0.oid) })
            return
        }
        pairPane.isHidden = false
        detail.view.isHidden = true
        let header = NSMutableAttributedString(string: pair.subject + "\n", attributes: [
            .font: NSFont.boldSystemFont(ofSize: NSFont.systemFontSize),
        ])
        let small = NSFont.monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        header.append(NSAttributedString(
            string: L("%1$@ → %2$@ · the colored lines changed between the two versions",
                      String(pair.old?.oid.prefix(7) ?? ""), String(pair.new?.oid.prefix(7) ?? "")),
            attributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor]))
        pairHeader.attributedStringValue = header
        diffView.show(repository.rangeDiffView(pair))
    }

    /// スナップショット用に、Metal で描いた差分の画像。
    func overlays() -> [(CGImage, NSView)] {
        guard view.window != nil else { return [] }
        return pairPane.isHidden ? detail.overlays() : diffView.overlay().map { [$0] } ?? []
    }

    // MARK: - 右クリック

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let row = table.clickedRow
        guard shown.indices.contains(row) else { return }
        let pair = shown[row]
        for (side, title) in [(pair.old, L("Show Old Version in History")), (pair.new, L("Show New Version in History"))] {
            guard let side, let id = Data(hex: side.oid) else { continue }
            menu.addItem(ClosureMenuItem(title: title) { [weak self] in self?.onShowInHistory?(id) })
        }
        menu.addItem(.separator())
        for (side, title) in [(pair.old, L("Copy Old Commit ID")), (pair.new, L("Copy New Commit ID"))] {
            guard let oid = side?.oid else { continue }
            menu.addItem(ClosureMenuItem(title: title) {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(oid, forType: .string)
            })
        }
    }

    // MARK: - NSTableView

    func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

    func tableViewSelectionDidChange(_ notification: Notification) {
        showPair(shown.indices.contains(table.selectedRow) ? shown[table.selectedRow] : nil)
    }

    /// 組の印（git range-diff と同じ記号）と色。
    private static func mark(_ status: RangePair.Status) -> (String, NSColor, String) {
        switch status {
        case .same: ("=", .tertiaryLabelColor, L("same"))
        case .changed: ("!", .systemOrange, L("changed"))
        case .removed: ("<", .systemRed, L("removed"))
        case .added: (">", .systemGreen, L("added"))
        }
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("pair"), owner: nil) as? RangePairCell ?? RangePairCell()
        let pair = shown[row]
        let (symbol, color, word) = Self.mark(pair.status)
        let digits = NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        // 1 行目: 印、古い番号 → 新しい番号、件名。
        let title = NSMutableAttributedString(string: symbol + " ", attributes: [
            .font: NSFont.monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .bold), .foregroundColor: color,
        ])
        let numbers = "\(pair.old.map { String($0.index) } ?? "–") → \(pair.new.map { String($0.index) } ?? "–")  "
        title.append(NSAttributedString(string: numbers, attributes: [.font: digits, .foregroundColor: NSColor.secondaryLabelColor]))
        title.append(NSAttributedString(string: pair.subject, attributes: [
            .foregroundColor: pair.status == .same ? NSColor.secondaryLabelColor : NSColor.labelColor,
            .strikethroughStyle: pair.status == .removed ? NSUnderlineStyle.single.rawValue : 0,
        ]))
        cell.title.attributedStringValue = title
        cell.title.toolTip = pair.subject
        // 2 行目: どうなったか、短い ID。
        let small = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        let ids = [pair.old, pair.new].map { $0.map { String($0.oid.prefix(7)) } ?? "–" }.joined(separator: " → ")
        let sub = NSMutableAttributedString(string: word, attributes: [.font: small, .foregroundColor: color])
        sub.append(NSAttributedString(string: " · " + ids, attributes: [
            .font: NSFont.monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular),
            .foregroundColor: NSColor.secondaryLabelColor,
        ]))
        cell.subtitle.attributedStringValue = sub
        return cell
    }

    private static let relative: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f
    }()
}

/// 組の行（印・番号・件名と、どうなったか・ID の 2 行）。
private final class RangePairCell: NSTableCellView {
    let title = NSTextField(labelWithString: "")
    let subtitle = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        identifier = .init("pair")
        for field in [title, subtitle] {
            field.lineBreakMode = .byTruncatingTail
            field.maximumNumberOfLines = 1
            field.translatesAutoresizingMaskIntoConstraints = false
            field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            addSubview(field)
        }
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            title.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor, constant: 14),
            subtitle.trailingAnchor.constraint(equalTo: title.trailingAnchor),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 1),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }
}
