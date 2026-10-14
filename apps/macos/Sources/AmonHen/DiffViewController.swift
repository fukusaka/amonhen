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
    private let document = FlippedView()
    private let textView = GraphView()
    private var diff: FileDiff?
    /// 行ごとの並べたグリフ。表示し直すまで使い回す。
    private var shaped: [Int: TextRenderer.ShapedLine] = [:]
    private var numbers: [UInt32: TextRenderer.ShapedLine] = [:]
    private var message: String?

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
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 700, height: 300))
        scrollView.documentView = document
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.drawsBackground = false
        scrollView.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self, selector: #selector(redraw), name: NSView.boundsDidChangeNotification, object: scrollView.contentView)
        textView.metalLayer.device = renderer.device
        container.addSubview(scrollView)
        container.addSubview(textView)
        view = container
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        scrollView.frame = view.bounds
        textView.frame = view.convert(scrollView.contentView.frame, from: scrollView)
        updateDocumentSize()
        redraw()
    }

    func show(_ diff: FileDiff?, message: String? = nil) {
        self.diff = diff
        self.message = message
        shaped.removeAll()
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
        let text = NSColor.labelColor.rgba
        let secondary = NSColor.secondaryLabelColor.rgba
        let width = textView.bounds.width
        diff.withRows { all in
            for row in rows {
                let r = all[row]
                let top = CGFloat(row) * lineHeight - origin.y
                switch r.kind {
                case UInt8(AmonhenDiffRow_ADDED): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: added)
                case UInt8(AmonhenDiffRow_REMOVED): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: removed)
                case UInt8(AmonhenDiffRow_HUNK): renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: hunk)
                default: break
                }
                let baseline = top + 2 + ascent
                for (number, right) in [(r.old_no, gutterWidth / 2 - 6), (r.new_no, gutterWidth - 6)] where number > 0 {
                    let line = numbers[number] ?? renderer.shape("\(number)" as CFString, font: font, scale: scale)
                    numbers[number] = line
                    renderer.addLine(line, origin: CGPoint(x: right - line.width, y: baseline), color: secondary, scale: scale)
                }
                let line = shaped[row] ?? diff.withText(row: row) { renderer.shape($0, font: font, scale: scale) }
                shaped[row] = line
                let isHunk = r.kind == UInt8(AmonhenDiffRow_HUNK)
                renderer.addLine(
                    line, origin: CGPoint(x: gutterWidth + 8 - origin.x, y: baseline), color: isHunk ? secondary : text,
                    scale: scale)
            }
        }
    }
}

final class FlippedView: NSView {
    override var isFlipped: Bool { true }
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
        diffView.overlay().map { [$0] } ?? []
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
