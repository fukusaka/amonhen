import AmonhenKit
import AppKit

/// Quick Open（⌘⇧O、⌘T、⌘N）: ブックマーク・最近開いたもの・スキャンで見つけたものを、あいまい検索して開く。
/// ⏎ で前のウィンドウのタブに、⌘⏎ で新しいウィンドウに開く。esc で閉じる。
final class QuickOpenPanel: NSObject, NSSearchFieldDelegate, NSTableViewDataSource, NSTableViewDelegate, NSWindowDelegate {
    static let shared = QuickOpenPanel()

    /// 開く（パス、新しいウィンドウにするか）。AppDelegate が設定する。
    var onOpen: ((String, Bool) -> Void)?

    private let panel: NSPanel
    private let field = NSSearchField()
    private let table = NSTableView()
    private let hint = NSTextField(labelWithString: "")
    private var candidates: [String] = []
    private var shown: [String] = []
    /// ⏎ を新しいウィンドウにするか（⌘N から開いたとき）。
    private var preferNewWindow = false

    private override init() {
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 560, height: 380),
            styleMask: [.titled, .closable, .fullSizeContentView], backing: .buffered, defer: false)
        super.init()
        panel.titleVisibility = .hidden
        panel.titlebarAppearsTransparent = true
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = true
        panel.isMovableByWindowBackground = true
        panel.delegate = self
        build()
    }

    private func build() {
        guard let content = panel.contentView else { return }
        field.placeholderString = L("Open a repository (fuzzy search by name or path)")
        field.font = .systemFont(ofSize: 17)
        field.focusRingType = .none
        field.delegate = self
        let column = NSTableColumn(identifier: .init("repo"))
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 40
        table.style = .plain
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(openSelected)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        table.backgroundColor = .clear
        hint.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        hint.textColor = .secondaryLabelColor
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

    /// 出す。`newWindow` なら ⏎ で新しいウィンドウに開く（⌘N）。
    func show(newWindow: Bool = false) {
        preferNewWindow = newWindow
        candidates = Bookmarks.shared.allLocations
        // スキャンの結果がまだなければ探し始める（見つかったら候補に足す）。
        if Bookmarks.shared.scanned.isEmpty, !Bookmarks.shared.data.scanRoots.isEmpty { Bookmarks.shared.rescan() }
        field.stringValue = ""
        hint.stringValue = newWindow
            ? L("⏎ Open in New Window   ⌘⏎ Open in Tab   esc Close")
            : L("⏎ Open in Tab   ⌘⏎ Open in New Window   esc Close")
        update()
        if let screen = (NSApp.keyWindow ?? NSApp.mainWindow)?.screen ?? NSScreen.main {
            let frame = screen.visibleFrame
            panel.setFrameOrigin(NSPoint(x: frame.midX - panel.frame.width / 2, y: frame.maxY - panel.frame.height - frame.height * 0.15))
        }
        panel.makeKeyAndOrderFront(nil)
        panel.makeFirstResponder(field)
    }

    /// ブックマークやスキャンの結果が変わった。
    func refreshIfVisible() {
        guard panel.isVisible else { return }
        candidates = Bookmarks.shared.allLocations
        update()
    }

    /// 問い合わせに合うものを、合い方の良い順に並べる。
    private func update() {
        let query = field.stringValue.trimmingCharacters(in: .whitespaces).lowercased()
        if query.isEmpty {
            shown = candidates
        } else {
            shown = candidates
                .compactMap { path -> (String, Int)? in
                    let name = Bookmarks.shared.displayName(path).lowercased()
                    // 名前で合えば高く、パスでしか合わなければ低くする。
                    if let s = Self.score(query, in: name) { return (path, s + 1000) }
                    return Self.score(query, in: path.lowercased()).map { (path, $0) }
                }
                .sorted { $0.1 > $1.1 }
                .map(\.0)
        }
        table.reloadData()
        if !shown.isEmpty { table.selectRowIndexes([0], byExtendingSelection: false) }
    }

    /// `query` の文字を `text` の中に順に（飛び飛びでよい）見つけられれば、点数（続いているほど・先頭や区切りの
    /// 直後ほど高い）。見つからなければ nil。
    static func score(_ query: String, in text: String) -> Int? {
        let q = Array(query), t = Array(text)
        var score = 0, ti = 0, previous = -2
        for c in q {
            while ti < t.count, t[ti] != c { ti += 1 }
            guard ti < t.count else { return nil }
            if ti == previous + 1 { score += 5 }
            if ti == 0 || "/-_. ".contains(t[ti - 1]) { score += 8 }
            previous = ti
            ti += 1
        }
        // 短いものほど少し高く。
        return score * 100 - t.count
    }

    func controlTextDidChange(_ obj: Notification) { update() }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.moveDown(_:)): move(by: 1)
        case #selector(NSResponder.moveUp(_:)): move(by: -1)
        case #selector(NSResponder.insertNewline(_:)): openSelected()
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

    @objc private func openSelected() {
        let row = table.clickedRow >= 0 ? table.clickedRow : table.selectedRow
        guard row >= 0, row < shown.count else { return }
        let command = NSApp.currentEvent?.modifierFlags.contains(.command) == true
        panel.orderOut(nil)
        onOpen?(shown[row], command != preferNewWindow)
    }

    func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("repo"), owner: nil) as? RepositoryCell ?? RepositoryCell()
        cell.show(shown[row])
        return cell
    }

    func windowDidResignKey(_ notification: Notification) {
        panel.orderOut(nil)
    }
}
