import AmonhenKit
import AppKit
import CAmonhen

extension NSColor {
    /// シェーダーに渡す sRGB の色。
    var rgba: SIMD4<Float> {
        let c = usingColorSpace(.sRGB) ?? self
        return [Float(c.redComponent), Float(c.greenComponent), Float(c.blueComponent), Float(c.alphaComponent)]
    }
}

/// 差分の表示。スクロールは NSScrollView に任せ、見えている行だけを Metal で描く。
final class DiffViewController: NSViewController {
    private let renderer: TextRenderer
    private let scrollView = NSScrollView()
    private let document = DiffDocumentView()
    private let textView = GraphView()
    private var diff: FileDiff?
    /// 行ごとの並べたグリフ。表示し直すまで使い回す。
    private var shaped: [Int: TextRenderer.ShapedLine] = [:]
    private var numberCache: [UInt32: TextRenderer.ShapedLine] = [:]
    private var message: String?

    /// 行を選べるか（Working Copy で行単位の stage に使う）。
    var isSelectable = false
    /// コンフリクトの表示か（`old_no` はコンフリクトの番号 + 1 で、クリックするとコンフリクト全体を選ぶ）。
    var isConflictView = false
    /// 選んだ行。
    private(set) var selectedRows: Set<Int> = []
    private var anchorRow: Int?
    var onSelectionChange: (() -> Void)?

    private let font = CTFontCreateWithName("Menlo" as CFString, 12, nil)
    private var lineHeight: CGFloat { ceil(CTFontGetAscent(font) + CTFontGetDescent(font) + CTFontGetLeading(font)) + 4 }
    private let gutterWidth: CGFloat = 96

    init(renderer: TextRenderer) {
        self.renderer = renderer
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        // 分割の最初の大きさは、中のビューの大きさで決まる。
        let container = LayoutView(frame: NSRect(x: 0, y: 0, width: 700, height: 300))
        container.onLayout = { [weak self] in self?.layoutSubviews() }
        scrollView.documentView = document
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.drawsBackground = false
        scrollView.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self, selector: #selector(redraw), name: NSView.boundsDidChangeNotification, object: scrollView.contentView)
        textView.metalLayer.device = renderer.device
        document.onMouseDown = { [weak self] event, point in self?.mouseDown(event, at: point) }
        container.addSubview(scrollView)
        container.addSubview(textView)
        view = container
    }

    private func layoutSubviews() {
        scrollView.frame = view.bounds
        textView.frame = view.convert(scrollView.contentView.frame, from: scrollView)
        updateDocumentSize()
        redraw()
    }

    func show(_ diff: FileDiff?, message: String? = nil, keepScroll: Bool = false) {
        self.diff = diff
        self.message = message
        shaped.removeAll()
        selectedRows.removeAll()
        anchorRow = nil
        onSelectionChange?()
        if keepScroll {
            updateDocumentSize()
            redraw()
            return
        }
        updateDocumentSize()
        scrollView.contentView.scroll(to: .zero)
        scrollView.reflectScrolledClipView(scrollView.contentView)
        redraw()
    }

    private func updateDocumentSize() {
        let rows = CGFloat(diff?.rowCount ?? 0)
        let clip = scrollView.contentView.bounds.size
        document.frame.size = NSSize(width: max(clip.width, 3000), height: max(clip.height, rows * lineHeight))
    }

    /// 選んだ行のうち、追加・削除の行。
    var selectedChangeRows: [Int] {
        guard let diff else { return [] }
        return diff.withRows { rows in
            selectedRows.sorted().filter {
                rows[$0].kind == UInt8(AmonhenDiffRow_ADDED) || rows[$0].kind == UInt8(AmonhenDiffRow_REMOVED)
            }
        }
    }

    /// 選んだコンフリクト（0 始まり）。
    var selectedConflict: Int? {
        guard isConflictView, let diff, let row = selectedRows.min() else { return nil }
        let id = diff.withRows { Int($0[row].old_no) }
        return id > 0 ? id - 1 : nil
    }

    /// クリックで行を選ぶ。hunk の見出しなら hunk 全体、Shift で範囲、Command で追加・解除。
    private func mouseDown(_ event: NSEvent, at point: NSPoint) {
        guard isSelectable, let diff else { return }
        let row = Int(point.y / lineHeight)
        guard row >= 0, row < diff.rowCount else { return }
        if isConflictView {
            // コンフリクトの行なら、そのコンフリクト全体を選ぶ。
            selectedRows = diff.withRows { all in
                let id = all[row].old_no
                guard id > 0 else { return [] }
                return Set(all.indices.filter { all[$0].old_no == id })
            }
            onSelectionChange?()
            redraw()
            return
        }
        let rows: Set<Int> = diff.withRows { all in
            guard all[row].kind == UInt8(AmonhenDiffRow_HUNK) else { return [row] }
            var end = row + 1
            while end < all.count, all[end].kind != UInt8(AmonhenDiffRow_HUNK) { end += 1 }
            return Set(row..<end)
        }
        if event.modifierFlags.contains(.shift), let anchor = anchorRow {
            selectedRows = Set(min(anchor, row)...max(anchor, row))
        } else if event.modifierFlags.contains(.command) {
            selectedRows = selectedRows.isSuperset(of: rows) ? selectedRows.subtracting(rows) : selectedRows.union(rows)
            anchorRow = row
        } else {
            selectedRows = rows
            anchorRow = row
        }
        onSelectionChange?()
        redraw()
    }

    /// 今の表示を画像にする（`--snapshot` 用）。
    func overlay() -> (CGImage, NSView)? {
        buildInstances()
        let scale = view.window?.backingScaleFactor ?? 2
        return renderer.image(size: textView.bounds.size, scale: scale, background: NSColor.textBackgroundColor.rgba)
            .map { ($0, textView) }
    }

    @objc private func redraw() {
        buildInstances()
        renderer.draw(layer: textView.metalLayer, background: NSColor.textBackgroundColor.rgba)
    }

    private func buildInstances() {
        let scale = view.window?.backingScaleFactor ?? 2
        let origin = scrollView.contentView.bounds.origin
        let height = textView.bounds.height
        renderer.instances.removeAll(keepingCapacity: true)

        if let message {
            let line = renderer.shape(message as CFString, font: font, scale: scale)
            renderer.addLine(line, origin: CGPoint(x: 12, y: 24), color: NSColor.secondaryLabelColor.rgba, scale: scale)
        } else if let diff {
            let first = max(0, Int(origin.y / lineHeight))
            let last = min(diff.rowCount, Int((origin.y + height) / lineHeight) + 1)
            if first < last {
                draw(diff, rows: first..<last, origin: origin, scale: scale)
            }
        }
    }

    private func draw(_ diff: FileDiff, rows: Range<Int>, origin: CGPoint, scale: CGFloat) {
        let ascent = CTFontGetAscent(font)
        let added = NSColor.systemGreen.withAlphaComponent(0.16).rgba
        let removed = NSColor.systemRed.withAlphaComponent(0.16).rgba
        let hunk = NSColor.systemBlue.withAlphaComponent(0.10).rgba
        let ours = NSColor.systemBlue.withAlphaComponent(0.18).rgba
        let theirs = NSColor.systemGreen.withAlphaComponent(0.18).rgba
        let base = NSColor.systemGray.withAlphaComponent(0.18).rgba
        let marker = NSColor.systemOrange.withAlphaComponent(0.14).rgba
        let text = NSColor.labelColor.rgba
        let secondary = NSColor.secondaryLabelColor.rgba
        let width = textView.bounds.width
        let selection = NSColor.selectedContentBackgroundColor.withAlphaComponent(0.35).rgba
        diff.withRows { all in
            for row in rows {
                let r = all[row]
                let top = CGFloat(row) * lineHeight - origin.y
                if selectedRows.contains(row) {
                    renderer.addRect(CGRect(x: 0, y: top, width: gutterWidth, height: lineHeight), color: selection)
                }
                switch r.kind {
                case UInt8(AmonhenDiffRow_ADDED): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: added)
                case UInt8(AmonhenDiffRow_REMOVED): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: removed)
                case UInt8(AmonhenDiffRow_HUNK): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: hunk)
                case UInt8(AmonhenDiffRow_OURS): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: ours)
                case UInt8(AmonhenDiffRow_THEIRS): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: theirs)
                case UInt8(AmonhenDiffRow_BASE): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: base)
                case UInt8(AmonhenDiffRow_MARKER): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: marker)
                default: break
                }
                let baseline = top + 2 + ascent
                // コンフリクトの表示では old_no はコンフリクトの番号なので、行番号は new_no だけ出す。
                let numbers = isConflictView ? [(r.new_no, gutterWidth - 6)] : [(r.old_no, gutterWidth / 2 - 6), (r.new_no, gutterWidth - 6)]
                for (number, right) in numbers where number > 0 {
                    let line = numberCache[number] ?? renderer.shape("\(number)" as CFString, font: font, scale: scale)
                    numberCache[number] = line
                    renderer.addLine(line, origin: CGPoint(x: right - line.width, y: baseline), color: secondary, scale: scale)
                }
                let line = shaped[row] ?? diff.withText(row: row) { renderer.shape($0, font: font, scale: scale) }
                shaped[row] = line
                let isHunk = r.kind == UInt8(AmonhenDiffRow_HUNK) || r.kind == UInt8(AmonhenDiffRow_MARKER)
                renderer.addLine(
                    line, origin: CGPoint(x: gutterWidth + 8 - origin.x, y: baseline), color: isHunk ? secondary : text,
                    scale: scale)
            }
        }
    }
}

/// 大きさが変わるたびに並べ直す（親がフレームを直接決めると layout() が呼ばれないことがあるため）。
final class LayoutView: NSView {
    var onLayout: (() -> Void)?

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        onLayout?()
    }

    override func layout() {
        super.layout()
        onLayout?()
    }
}

final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}

/// 差分のスクロール用の中身。描くのは上に重ねた Metal のビューで、ここはクリックを受けるだけ。
final class DiffDocumentView: NSView {
    var onMouseDown: ((NSEvent, NSPoint) -> Void)?

    override var isFlipped: Bool { true }

    override func mouseDown(with event: NSEvent) {
        onMouseDown?(event, convert(event.locationInWindow, from: nil))
    }
}

/// 選んだコミットの詳細: 変わったファイルの一覧と、選んだファイルの差分。
final class CommitDetailViewController: NSSplitViewController, NSTableViewDataSource, NSTableViewDelegate {
    private let repository: Repository
    private let files = NSTableView()
    private let diffView: DiffViewController
    private var changes: CommitChanges?
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
        let column = NSTableColumn(identifier: .init("path"))
        files.addTableColumn(column)
        files.headerView = nil
        files.style = .sourceList
        files.dataSource = self
        files.delegate = self
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 240, height: 300))
        scroll.documentView = files
        scroll.hasVerticalScroller = true
        let list = NSViewController()
        list.view = scroll
        let listItem = NSSplitViewItem(viewController: list)
        listItem.minimumThickness = 160
        // ウィンドウの大きさを変えたとき、ファイル一覧の幅は保ち、差分の側を伸び縮みさせる。
        listItem.holdingPriority = .init(260)
        addSplitViewItem(listItem)
        let diffItem = NSSplitViewItem(viewController: diffView)
        diffItem.minimumThickness = 300
        addSplitViewItem(diffItem)
    }

    func overlays() -> [(CGImage, NSView)] {
        view.window == nil ? [] : diffView.overlay().map { [$0] } ?? []
    }

    /// `commit` の変更を裏で読んで表示する。
    func show(commit: Data?) {
        request += 1
        let request = self.request
        guard let commit else {
            changes = nil
            files.reloadData()
            diffView.show(nil)
            return
        }
        let repository = self.repository
        Task {
            let changes = try? await Task.detached(priority: .userInitiated) { try repository.changes(of: commit) }.value
            guard request == self.request else { return }
            self.changes = changes
            files.reloadData()
            if changes?.items.isEmpty ?? true {
                diffView.show(nil, message: "変更なし")
            } else {
                files.selectRowIndexes([0], byExtendingSelection: false)
            }
        }
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        changes?.items.count ?? 0
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let item = changes?.items[row] else { return nil }
        let field = tableView.makeView(withIdentifier: .init("file"), owner: nil) as? NSTextField ?? {
            let f = NSTextField(labelWithString: "")
            f.identifier = .init("file")
            f.lineBreakMode = .byTruncatingMiddle
            return f
        }()
        let mark = switch item.kind {
        case .added: "A"
        case .deleted: "D"
        case .modified: "M"
        case .renamed: "R"
        case .copied: "C"
        }
        field.stringValue = "\(mark)  \(item.path)"
        field.toolTip = item.oldPath.map { "\($0) → \(item.path)" } ?? item.path
        return field
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard let changes, files.selectedRow >= 0 else { return }
        let index = files.selectedRow
        request += 1
        let request = self.request
        Task {
            let diff = try? await Task.detached(priority: .userInitiated) { try changes.diff(at: index) }.value
            guard request == self.request else { return }
            if let diff, diff.isBinary {
                diffView.show(nil, message: "バイナリファイル")
            } else {
                diffView.show(diff)
            }
        }
    }
}
