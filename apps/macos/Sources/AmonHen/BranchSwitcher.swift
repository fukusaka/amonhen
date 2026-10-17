import AmonhenKit
import AppKit

/// ブランチの切り替え（ツールバーのブランチ、⌘⇧B）: ブランチをあいまい検索して切り替える。最近アプリで切り替えた
/// ものを上に出す。リモートのブランチは、追跡するローカルブランチを作って切り替える。一致するものがなければ、
/// 打った名前で新しいブランチを作れる。
final class BranchSwitcherPanel: NSObject, NSSearchFieldDelegate, NSTableViewDataSource, NSTableViewDelegate, NSWindowDelegate {
    static let shared = BranchSwitcherPanel()

    enum Choice {
        case local(String)
        case remote(String)
        case create(String)
    }

    private let panel: NSPanel
    private let field = NSSearchField()
    private let table = NSTableView()
    private let hint = NSTextField(labelWithString: "")
    private var refs: Refs?
    private var recent: [String] = []
    private var shown: [Choice] = []
    private var onChoose: ((Choice) -> Void)?

    private override init() {
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 480, height: 400),
            styleMask: [.titled, .closable, .fullSizeContentView], backing: .buffered, defer: false)
        super.init()
        panel.titleVisibility = .hidden
        panel.titlebarAppearsTransparent = true
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = true
        panel.delegate = self
        build()
    }

    private func build() {
        guard let content = panel.contentView else { return }
        field.placeholderString = L("Find a branch or type a new name")
        field.font = .systemFont(ofSize: 16)
        field.focusRingType = .none
        field.delegate = self
        table.addTableColumn(NSTableColumn(identifier: .init("branch")))
        table.headerView = nil
        table.rowHeight = 26
        table.style = .plain
        table.backgroundColor = .clear
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(chooseSelected)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        hint.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        hint.textColor = .secondaryLabelColor
        hint.stringValue = L("⏎ Switch   esc Close   (a remote branch gets a tracking branch)")
        for v in [field, scroll, hint] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            content.addSubview(v)
        }
        NSLayoutConstraint.activate([
            field.topAnchor.constraint(equalTo: content.topAnchor, constant: 28),
            field.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 14),
            field.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -14),
            scroll.topAnchor.constraint(equalTo: field.bottomAnchor, constant: 8),
            scroll.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 6),
            scroll.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -6),
            hint.topAnchor.constraint(equalTo: scroll.bottomAnchor, constant: 6),
            hint.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 14),
            hint.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -8),
        ])
    }

    /// `refs` のブランチから選んでもらう。`recent` は最近切り替えたブランチ（新しい順）。
    func show(refs: Refs, recent: [String], over window: NSWindow?, onChoose: @escaping (Choice) -> Void) {
        self.refs = refs
        self.recent = recent
        self.onChoose = onChoose
        field.stringValue = ""
        update()
        if let window {
            let f = window.frame
            panel.setFrameOrigin(NSPoint(x: f.midX - panel.frame.width / 2, y: f.maxY - panel.frame.height - 90))
        }
        panel.makeKeyAndOrderFront(nil)
        panel.makeFirstResponder(field)
    }

    private func update() {
        guard let refs else { return }
        let query = field.stringValue.trimmingCharacters(in: .whitespaces)
        let lower = query.lowercased()
        let head = refs.headBranch
        let locals = refs.items.filter { $0.kind == .local && $0.name != head }.map(\.name)
        let localSet = Set(locals + [head].compactMap { $0 })
        // ローカルにもう同じ名前があるリモートのブランチ（origin/main など）は出さない。
        let remotes = refs.items.filter { item in
            guard item.kind == .remote, !item.name.hasSuffix("/HEAD") else { return false }
            let short = item.name.split(separator: "/", maxSplits: 1).last.map(String.init) ?? item.name
            return !localSet.contains(short)
        }.map(\.name)
        // 最近切り替えたものを先に、残りは名前の順。
        let rank = Dictionary(recent.enumerated().map { ($1, $0) }, uniquingKeysWith: { a, _ in a })
        let orderedLocals = locals.sorted { (rank[$0] ?? .max, $0) < (rank[$1] ?? .max, $1) }
        var candidates: [(Choice, String, Int)] = orderedLocals.enumerated().map { (.local($1), $1, 2000 - $0) }
            + remotes.enumerated().map { (.remote($1), $1, 1000 - $0) }
        if !lower.isEmpty {
            candidates = candidates.compactMap { c in
                QuickOpenPanel.score(lower, in: c.1.lowercased()).map { (c.0, c.1, $0 + (c.2 > 1000 ? 500 : 0)) }
            }
            .sorted { $0.2 > $1.2 }
        }
        shown = candidates.map(\.0)
        // ちょうど同じ名前のローカルブランチがなく、ブランチの名前として使えれば、作る行を足す。
        if !query.isEmpty, !localSet.contains(query), !query.contains(" "), !query.hasPrefix("-") {
            shown.append(.create(query))
        }
        table.reloadData()
        if !shown.isEmpty { table.selectRowIndexes([0], byExtendingSelection: false) }
    }

    func controlTextDidChange(_ obj: Notification) { update() }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.moveDown(_:)): move(by: 1)
        case #selector(NSResponder.moveUp(_:)): move(by: -1)
        case #selector(NSResponder.insertNewline(_:)): chooseSelected()
        case #selector(NSResponder.cancelOperation(_:)): panel.orderOut(nil)
        default: return false
        }
        return true
    }

    private func move(by step: Int) {
        guard !shown.isEmpty else { return }
        let row = max(0, min(shown.count - 1, table.selectedRow + step))
        table.selectRowIndexes([row], byExtendingSelection: false)
        table.scrollRowToVisible(row)
    }

    @objc private func chooseSelected() {
        let row = table.clickedRow >= 0 ? table.clickedRow : table.selectedRow
        guard row >= 0, row < shown.count else { return }
        panel.orderOut(nil)
        onChoose?(shown[row])
    }

    func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("branch"), owner: nil) as? NSTableCellView ?? {
            let c = NSTableCellView()
            c.identifier = .init("branch")
            let image = NSImageView()
            let text = NSTextField(labelWithString: "")
            text.lineBreakMode = .byTruncatingMiddle
            for v in [image, text] as [NSView] {
                v.translatesAutoresizingMaskIntoConstraints = false
                c.addSubview(v)
            }
            NSLayoutConstraint.activate([
                image.leadingAnchor.constraint(equalTo: c.leadingAnchor, constant: 6),
                image.centerYAnchor.constraint(equalTo: c.centerYAnchor),
                image.widthAnchor.constraint(equalToConstant: 16),
                text.leadingAnchor.constraint(equalTo: image.trailingAnchor, constant: 6),
                text.trailingAnchor.constraint(equalTo: c.trailingAnchor, constant: -6),
                text.centerYAnchor.constraint(equalTo: c.centerYAnchor),
            ])
            c.imageView = image
            c.textField = text
            return c
        }()
        let (symbol, title, color): (String, String, NSColor) = switch shown[row] {
        case let .local(name): ("arrow.triangle.branch", name + (recent.contains(name) ? L("  · recent") : ""), .labelColor)
        case let .remote(name): ("network", name + L("  · creates a tracking branch"), .secondaryLabelColor)
        case let .create(name): ("plus.circle", L("Create branch %@ and switch to it", name), .controlAccentColor)
        }
        cell.imageView?.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
        cell.imageView?.contentTintColor = color
        cell.textField?.stringValue = title
        cell.textField?.textColor = color
        return cell
    }

    func windowDidResignKey(_ notification: Notification) {
        panel.orderOut(nil)
    }
}
