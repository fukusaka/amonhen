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

/// 差分の中の文字の位置（行と、行の中の UTF-16 の位置）。
struct TextPosition: Comparable {
    var row: Int
    var index: Int

    static func < (a: Self, b: Self) -> Bool { (a.row, a.index) < (b.row, b.index) }
}

/// 差分の表示。スクロールは NSScrollView に任せ、見えている行だけを Metal で描く。
final class DiffViewController: NSViewController, NSSearchFieldDelegate {
    private let renderer: TextRenderer
    private let scrollView = NSScrollView()
    private let document = DiffDocumentView()
    private let textView = GraphView()
    private var diff: FileDiff?
    /// 行ごとの並べたグリフ。表示し直すまで使い回す。
    private var shaped: [Int: TextRenderer.ShapedLine] = [:]
    private var numberCache: [UInt32: TextRenderer.ShapedLine] = [:]
    private var message: String?
    private var highlights: [Int: [Range<Int>]] = [:]
    /// 行ごとの構文の色付け（UTF-16 の範囲と種類）。差分を表示した後に裏で求める。
    private var syntax: [Int: [(range: Range<Int32>, kind: UInt8)]] = [:]

    /// 行を選べるか（Working Copy で行単位の stage に使う）。
    var isSelectable = false
    /// コンフリクトの表示か（`old_no` はコンフリクトの番号 + 1 で、クリックするとコンフリクト全体を選ぶ）。
    var isConflictView = false
    /// 選んだ行。
    private(set) var selectedRows: Set<Int> = []
    private var anchorRow: Int?
    var onSelectionChange: (() -> Void)?
    /// 文字の選択（行を選ぶ表示では使わない）。
    private var textSelection: (anchor: TextPosition, head: TextPosition)?
    /// 単語・行単位で選び始めたときの最初の範囲（ドラッグでその単位のまま広げる）。
    private var selectionUnit: (start: TextPosition, end: TextPosition)?

    // 検索
    private let findBar = NSView()
    private let findField = NSSearchField()
    private let findCount = NSTextField(labelWithString: "")
    private var isFindBarVisible = false
    private var matches: [(row: Int, range: Range<Int>)] = []
    private var currentMatch: Int?

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
        document.owner = self
        buildFindBar()
        container.addSubview(scrollView)
        container.addSubview(textView)
        container.addSubview(findBar)
        view = container
    }

    private func buildFindBar() {
        findBar.isHidden = true
        findField.placeholderString = "差分を検索"
        findField.sendsSearchStringImmediately = true
        findField.delegate = self
        findField.target = self
        findField.action = #selector(findFieldChanged)
        findField.controlSize = .small
        findCount.textColor = .secondaryLabelColor
        findCount.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        let previous = NSButton(image: NSImage(systemSymbolName: "chevron.up", accessibilityDescription: "前へ")!,
                                target: self, action: #selector(findPrevious))
        let next = NSButton(image: NSImage(systemSymbolName: "chevron.down", accessibilityDescription: "次へ")!,
                            target: self, action: #selector(findNext))
        let done = NSButton(title: "完了", target: self, action: #selector(hideFindBar))
        for b in [previous, next, done] {
            b.bezelStyle = .accessoryBarAction
            b.controlSize = .small
        }
        let stack = NSStackView(views: [findField, findCount, previous, next, done])
        stack.spacing = 6
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 8)
        stack.frame = findBar.bounds
        stack.autoresizingMask = [.width, .height]
        findField.widthAnchor.constraint(greaterThanOrEqualToConstant: 180).isActive = true
        findBar.addSubview(stack)
    }

    private static let findBarHeight: CGFloat = 30

    private func layoutSubviews() {
        let barHeight = isFindBarVisible ? Self.findBarHeight : 0
        findBar.frame = NSRect(x: 0, y: view.bounds.height - barHeight, width: view.bounds.width, height: barHeight)
        scrollView.frame = NSRect(x: 0, y: 0, width: view.bounds.width, height: view.bounds.height - barHeight)
        textView.frame = view.convert(scrollView.contentView.frame, from: scrollView)
        updateDocumentSize()
        redraw()
    }

    func show(_ diff: FileDiff?, message: String? = nil, keepScroll: Bool = false) {
        self.diff = diff
        // 行ごとの強調する範囲。
        highlights = [:]
        diff?.withHighlights { all in
            for h in all { highlights[Int(h.row), default: []].append(Int(h.start)..<Int(h.end)) }
        }
        self.message = message
        syntax = [:]
        if let diff { loadSyntax(of: diff) }
        shaped.removeAll()
        selectedRows.removeAll()
        anchorRow = nil
        textSelection = nil
        onSelectionChange?()
        if isFindBarVisible { updateMatches(scrollToCurrent: false) }
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

    /// ファイル全体の解析は時間がかかることがあるので、色なしで表示した後に裏で求めて描き直す。
    private func loadSyntax(of diff: FileDiff) {
        Task {
            // 色付けは後追いなので、ほかの読み込み（選んだコミットの詳細など）を先にする。
            let spans = await Task.detached(priority: .utility) { diff.syntax() }.value
            guard let spans, self.diff === diff else { return }
            var byRow: [Int: [(range: Range<Int32>, kind: UInt8)]] = [:]
            spans.withSpans { all in
                for s in all where s.start < s.end {
                    byRow[Int(s.row), default: []].append((Int32(s.start)..<Int32(s.end), s.kind))
                }
            }
            self.syntax = byRow
            self.redraw()
        }
    }

    /// 構文の種類ごとの色（`AmonhenSyntaxSpan` の定数の順）。明るい・暗い外観で変わる。
    private static func syntaxPalette() -> [SIMD4<Float>] {
        SyntaxColors.palette.map(\.rgba)
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
    fileprivate func mouseDown(_ event: NSEvent, at point: NSPoint) {
        guard let diff else { return }
        guard isSelectable else {
            beginTextSelection(event, at: point)
            return
        }
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

    // MARK: - 文字の選択とコピー

    /// 行の並べたグリフ。まだなければ並べてキャッシュする。
    private func shapedLine(_ row: Int, scale: CGFloat? = nil) -> TextRenderer.ShapedLine? {
        if let line = shaped[row] { return line }
        guard let diff, row >= 0, row < diff.rowCount else { return nil }
        let scale = scale ?? view.window?.backingScaleFactor ?? 2
        let line = diff.withText(row: row) { renderer.shape($0, font: font, scale: scale, ranges: highlights[row] ?? []) }
        shaped[row] = line
        return line
    }

    private func text(row: Int) -> String {
        diff?.withText(row: row) { $0 as String } ?? ""
    }

    private func position(at point: NSPoint) -> TextPosition? {
        guard let diff, diff.rowCount > 0 else { return nil }
        if point.y < 0 { return TextPosition(row: 0, index: 0) }
        let row = Int(point.y / lineHeight)
        if row >= diff.rowCount { return TextPosition(row: diff.rowCount - 1, index: text(row: diff.rowCount - 1).utf16.count) }
        return TextPosition(row: row, index: shapedLine(row)?.index(at: point.x - gutterWidth - 8) ?? 0)
    }

    /// クリックの回数で、文字・単語・行の単位で選ぶ。
    private func unitRange(at p: TextPosition, clicks: Int) -> (start: TextPosition, end: TextPosition) {
        let string = text(row: p.row)
        switch clicks {
        case 2:
            let r = NSAttributedString(string: string).doubleClick(at: min(p.index, max(0, string.utf16.count - 1)))
            return (TextPosition(row: p.row, index: r.location), TextPosition(row: p.row, index: r.location + r.length))
        case 3...:
            return (TextPosition(row: p.row, index: 0), TextPosition(row: p.row, index: string.utf16.count))
        default:
            return (p, p)
        }
    }

    private func beginTextSelection(_ event: NSEvent, at point: NSPoint) {
        guard let p = position(at: point) else { return }
        if event.modifierFlags.contains(.shift), let current = textSelection {
            textSelection = (current.anchor, p)
        } else {
            let unit = unitRange(at: p, clicks: event.clickCount)
            selectionUnit = event.clickCount > 1 ? unit : nil
            textSelection = (unit.start, unit.end)
        }
        redraw()
    }

    fileprivate func mouseDragged(_ event: NSEvent, at point: NSPoint) {
        guard !isSelectable, let p = position(at: point), let current = textSelection else { return }
        if let unit = selectionUnit {
            let extended = unitRange(at: p, clicks: event.clickCount)
            textSelection = p < unit.start ? (unit.end, extended.start) : (unit.start, max(unit.end, extended.end))
        } else {
            textSelection = (current.anchor, p)
        }
        redraw()
    }

    /// 選んだ文字。行を選ぶ表示では、選んだ行の文字。
    var selectedText: String? {
        if isSelectable {
            guard !selectedRows.isEmpty else { return nil }
            return selectedRows.sorted().map { text(row: $0) }.joined(separator: "\n")
        }
        guard let (start, end) = orderedSelection, start != end else { return nil }
        return (start.row...end.row).map { row in
            let units = Array(text(row: row).utf16)
            let from = row == start.row ? min(start.index, units.count) : 0
            let to = row == end.row ? min(end.index, units.count) : units.count
            return String(utf16CodeUnits: Array(units[from..<max(from, to)]), count: max(0, to - from))
        }.joined(separator: "\n")
    }

    private var orderedSelection: (TextPosition, TextPosition)? {
        textSelection.map { $0.anchor < $0.head ? ($0.anchor, $0.head) : ($0.head, $0.anchor) }
    }

    fileprivate func copySelection() {
        guard let text = selectedText else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    fileprivate func selectAllText() {
        guard let diff, diff.rowCount > 0 else { return }
        if isSelectable {
            selectedRows = Set(0..<diff.rowCount)
            onSelectionChange?()
        } else {
            let last = diff.rowCount - 1
            textSelection = (TextPosition(row: 0, index: 0), TextPosition(row: last, index: text(row: last).utf16.count))
        }
        redraw()
    }

    fileprivate func clearTextSelection() {
        textSelection = nil
        redraw()
    }

    fileprivate var hasSelection: Bool { selectedText != nil }

    // MARK: - 検索

    @objc func showFindBar() {
        if let selected = selectedText, !selected.contains("\n") { findField.stringValue = selected }
        if !isFindBarVisible {
            isFindBarVisible = true
            findBar.isHidden = false
            layoutSubviews()
        }
        view.window?.makeFirstResponder(findField)
        findField.currentEditor()?.selectAll(nil)
        updateMatches(scrollToCurrent: true)
    }

    @objc func hideFindBar() {
        isFindBarVisible = false
        findBar.isHidden = true
        matches = []
        currentMatch = nil
        layoutSubviews()
        view.window?.makeFirstResponder(document)
    }

    /// 検索欄に文字を入れたまま、検索だけ行う（`--snapshot` の確認用）。
    func find(_ query: String) {
        findField.stringValue = query
        showFindBar()
    }

    @objc private func findFieldChanged() {
        updateMatches(scrollToCurrent: true)
    }

    @objc func findNext() { moveMatch(by: 1) }
    @objc func findPrevious() { moveMatch(by: -1) }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.insertNewline(_:)):
            moveMatch(by: NSEvent.modifierFlags.contains(.shift) ? -1 : 1)
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            hideFindBar()
            return true
        default:
            return false
        }
    }

    /// 大文字・小文字を区別せずに、すべての行から探す。今の一致は、見えている位置より後の最初のもの。
    private func updateMatches(scrollToCurrent: Bool) {
        matches = []
        let query = findField.stringValue
        if let diff, !query.isEmpty {
            for row in 0..<diff.rowCount {
                diff.withText(row: row) { cf in
                    let s = cf as NSString
                    var search = NSRange(location: 0, length: s.length)
                    while true {
                        let r = s.range(of: query, options: [.caseInsensitive], range: search)
                        guard r.location != NSNotFound else { break }
                        matches.append((row, r.location..<r.location + r.length))
                        let next = r.location + max(r.length, 1)
                        guard next < s.length else { break }
                        search = NSRange(location: next, length: s.length - next)
                    }
                }
            }
        }
        let firstVisible = Int(scrollView.contentView.bounds.minY / lineHeight)
        currentMatch = matches.isEmpty ? nil : (matches.firstIndex { $0.row >= firstVisible } ?? 0)
        updateFindCount()
        if scrollToCurrent { revealCurrentMatch() }
        redraw()
    }

    private func moveMatch(by step: Int) {
        if !isFindBarVisible { showFindBar() }
        guard !matches.isEmpty else { return NSSound.beep() }
        currentMatch = ((currentMatch ?? -step) + step + matches.count) % matches.count
        updateFindCount()
        revealCurrentMatch()
        redraw()
    }

    private func updateFindCount() {
        if findField.stringValue.isEmpty {
            findCount.stringValue = ""
        } else if let current = currentMatch {
            findCount.stringValue = "\(current + 1) / \(matches.count)"
        } else {
            findCount.stringValue = "見つかりません"
        }
    }

    /// 今の一致を選び（⌘C でコピーできるように）、見える位置までスクロールする。
    private func revealCurrentMatch() {
        guard let current = currentMatch, let line = shapedLine(matches[current].row) else { return }
        let m = matches[current]
        if !isSelectable {
            textSelection = (TextPosition(row: m.row, index: m.range.lowerBound), TextPosition(row: m.row, index: m.range.upperBound))
        }
        let x0 = gutterWidth + 8 + line.offset(of: m.range.lowerBound)
        let x1 = gutterWidth + 8 + line.offset(of: m.range.upperBound)
        let clip = scrollView.contentView
        var origin = clip.bounds.origin
        let top = CGFloat(m.row) * lineHeight
        if top < origin.y || top + lineHeight > origin.y + clip.bounds.height {
            origin.y = max(0, min(top - clip.bounds.height / 3, document.frame.height - clip.bounds.height))
        }
        if x0 < origin.x + gutterWidth || x1 > origin.x + clip.bounds.width {
            origin.x = max(0, x0 - gutterWidth - 40)
        }
        if origin != clip.bounds.origin {
            clip.scroll(to: origin)
            scrollView.reflectScrolledClipView(clip)
        }
    }

    /// 今の表示を画像にする（`--snapshot` 用）。
    func overlay() -> (CGImage, NSView)? {
        buildInstances()
        let scale = view.window?.backingScaleFactor ?? 2
        return renderer.image(size: textView.bounds.size, scale: scale, background: NSColor.textBackgroundColor.rgba)
            .map { ($0, textView) }
    }

    fileprivate func redrawFocus() { redraw() }

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
        let strongAdded = NSColor.systemGreen.withAlphaComponent(0.38).rgba
        let strongRemoved = NSColor.systemRed.withAlphaComponent(0.38).rgba
        let ours = NSColor.systemBlue.withAlphaComponent(0.18).rgba
        let theirs = NSColor.systemGreen.withAlphaComponent(0.18).rgba
        let base = NSColor.systemGray.withAlphaComponent(0.18).rgba
        let marker = NSColor.systemOrange.withAlphaComponent(0.14).rgba
        let text = NSColor.labelColor.rgba
        let secondary = NSColor.secondaryLabelColor.rgba
        let width = textView.bounds.width
        let selection = NSColor.selectedContentBackgroundColor.withAlphaComponent(0.35).rgba
        let focused = view.window?.isKeyWindow == true && view.window?.firstResponder === document
        let selectedText = (focused ? NSColor.selectedTextBackgroundColor : NSColor.unemphasizedSelectedTextBackgroundColor).rgba
        let matchColor = NSColor.systemYellow.withAlphaComponent(0.35).rgba
        let currentMatchColor = NSColor.systemOrange.withAlphaComponent(0.7).rgba
        let palette = syntax.isEmpty ? [] : Self.syntaxPalette()
        let textRange = orderedSelection
        // 見えている行の一致（番号つき）。
        var matchesByRow: [Int: [(Int, Range<Int>)]] = [:]
        if !matches.isEmpty {
            // 一致は行の順に並んでいるので、見えている最初の行を二分探索する。
            var lo = 0, hi = matches.count
            while lo < hi {
                let mid = (lo + hi) / 2
                if matches[mid].row < rows.lowerBound { lo = mid + 1 } else { hi = mid }
            }
            var i = lo
            while i < matches.count, matches[i].row < rows.upperBound {
                matchesByRow[matches[i].row, default: []].append((i, matches[i].range))
                i += 1
            }
        }
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
                guard let line = shapedLine(row, scale: scale) else { continue }
                // 変わった単語を、行の色より濃くする。
                let strong = r.kind == UInt8(AmonhenDiffRow_ADDED) ? strongAdded : strongRemoved
                for (x0, x1) in line.spans {
                    renderer.addRect(
                        CGRect(x: gutterWidth + 8 - origin.x + x0, y: top + 1, width: max(1, x1 - x0), height: lineHeight - 2),
                        color: strong)
                }
                let textX = gutterWidth + 8 - origin.x
                // 文字の選択。次の行に続くときは、改行の分として少し右まで塗る。
                if let (start, end) = textRange, start.row <= row, row <= end.row {
                    let x0 = row == start.row ? line.offset(of: start.index) : 0
                    let x1 = row == end.row ? line.offset(of: end.index) : line.width + 6
                    if x1 > x0 {
                        renderer.addRect(CGRect(x: textX + x0, y: top, width: x1 - x0, height: lineHeight), color: selectedText)
                    }
                }
                for (i, m) in matchesByRow[row] ?? [] {
                    let x0 = line.offset(of: m.lowerBound), x1 = line.offset(of: m.upperBound)
                    renderer.addRect(
                        CGRect(x: textX + x0, y: top + 1, width: max(2, x1 - x0), height: lineHeight - 2),
                        color: i == currentMatch ? currentMatchColor : matchColor)
                }
                let isHunk = r.kind == UInt8(AmonhenDiffRow_HUNK) || r.kind == UInt8(AmonhenDiffRow_MARKER)
                let spans = (syntax[row] ?? []).map { (range: $0.range, color: palette[Int($0.kind) & 15]) }
                renderer.addLine(
                    line, origin: CGPoint(x: gutterWidth + 8 - origin.x, y: baseline), color: isHunk ? secondary : text,
                    scale: scale, spans: spans)
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

/// 差分のスクロール用の中身。描くのは上に重ねた Metal のビューで、ここはクリック・コピー・検索を受ける。
final class DiffDocumentView: NSView, NSMenuItemValidation {
    weak var owner: DiffViewController?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        defer { DispatchQueue.main.async { self.owner?.redrawFocus() } }
        return super.becomeFirstResponder()
    }

    override func resignFirstResponder() -> Bool {
        defer { DispatchQueue.main.async { self.owner?.redrawFocus() } }
        return super.resignFirstResponder()
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        owner?.mouseDown(event, at: convert(event.locationInWindow, from: nil))
    }

    override func mouseDragged(with event: NSEvent) {
        autoscroll(with: event)
        owner?.mouseDragged(event, at: convert(event.locationInWindow, from: nil))
    }

    @objc func copy(_ sender: Any?) { owner?.copySelection() }
    override func selectAll(_ sender: Any?) { owner?.selectAllText() }
    @objc func showFindBar(_ sender: Any?) { owner?.showFindBar() }
    @objc func findNext(_ sender: Any?) { owner?.findNext() }
    @objc func findPrevious(_ sender: Any?) { owner?.findPrevious() }
    override func cancelOperation(_ sender: Any?) { owner?.clearTextSelection() }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        item.action == #selector(copy(_:)) ? owner?.hasSelection == true : true
    }
}

/// 選んだコミットの詳細: 変わったファイルの一覧と、選んだファイルの差分。
final class CommitDetailViewController: NSSplitViewController, NSTableViewDataSource, NSTableViewDelegate,
    NSTextViewDelegate, NSMenuDelegate
{
    /// ファイルの右クリック: blame（パス、コミット）とファイルの履歴（パス）。
    var onBlame: ((String, String) -> Void)?
    var onFileHistory: ((String) -> Void)?
    /// 表示しているコミット（40 桁）。stash のときは nil（blame の起点にしない）。
    private var commitHex: String?
    /// コミットを選んでから最初のファイルの差分を出すまでを測る（`--measure-select`）。
    private var selectedAt: CFTimeInterval = 0
    /// コミットの変更と一緒に読んだ、最初のファイルの差分。
    private var preloadedDiff: FileDiff?
    var onDiffShown: (() -> Void)?
    /// 親コミットのリンクを押したとき（40 桁の ID）。
    var onSelectCommit: ((String) -> Void)?
    /// 作者の名前を押したとき（History を検索する問い合わせ）と、ref の名前を押したとき。
    var onSearch: ((String) -> Void)?
    var onSelectRef: ((String) -> Void)?

    /// 表示中のコミットの親と子（キーボードで移るため）。子は詳細を読み終えるまで空。
    var currentParents: [String] { info?.parents ?? [] }
    var currentChildren: [String] { extras?.children ?? [] }
    /// メッセージ・作者・ID・親コミット。
    private let header = NSTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 170))
    private let repository: Repository
    private let files = NSTableView()
    private let diffView: DiffViewController
    private var changes: CommitChanges?
    private var request = 0
    private var headerRequest = 0
    /// 表示しているコミットの情報と、裏で読んだ詳細（見せ方を変えたときに描き直す）。
    private var info: CommitInfo?
    private var extras: CommitExtras?
    /// 署名を検証する側を変えた回数。変える前に始めた詳細の読み込みの結果は使わない。
    private var signatureGeneration = 0
    /// 長い本文を開いているか（コミットを替えたら閉じる）。
    private var bodyExpanded = false
    /// History の検索の問い合わせ。件名と本文の中の一致を強調する。
    var searchQuery: String? {
        didSet { if searchQuery != oldValue { renderHeader() } }
    }
    private let styleControl = NSSegmentedControl(
        labels: CommitHeaderStyle.allCases.map(\.title), trackingMode: .selectOne, target: nil, action: nil)

    init(repository: Repository, renderer: TextRenderer) {
        self.repository = repository
        self.diffView = DiffViewController(renderer: renderer)
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLayout() {
        super.viewDidLayout()
        files.sizeLastColumnToFit()
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        // 上から、メッセージとメタ情報、変更ファイル、差分。
        splitView.isVertical = false
        let column = NSTableColumn(identifier: .init("path"))
        column.resizingMask = .autoresizingMask
        files.addTableColumn(column)
        // パスの列を表の幅いっぱいにする（狭いと途中が省略される）。
        files.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        files.headerView = nil
        files.style = .sourceList
        files.dataSource = self
        files.delegate = self
        let fileMenu = NSMenu()
        fileMenu.delegate = self
        files.menu = fileMenu
        // 分割の最初の高さは、中のビューの高さで決まる（メッセージ 170、変更ファイル 120、残りを差分）。
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 600, height: 120))
        scroll.documentView = files
        scroll.hasVerticalScroller = true
        header.isEditable = false
        header.drawsBackground = true
        header.backgroundColor = .textBackgroundColor
        header.textContainerInset = NSSize(width: 6, height: 6)
        header.delegate = self
        // リンクの色は文字ごとに決める（作者・ref は元の色のまま押せる）。
        header.linkTextAttributes = [.cursor: NSCursor.pointingHand]
        header.autoresizingMask = [.width]
        header.isVerticallyResizable = true
        header.minSize = NSSize(width: 0, height: 0)
        header.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        header.textContainer?.widthTracksTextView = true
        let headerScroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 600, height: 170))
        // 文字の領域を欄の幅に合わせてから入れる（幅が違うと、自動の伸縮でその差を保ったままになる）。
        header.frame = NSRect(origin: .zero, size: headerScroll.contentSize)
        headerScroll.documentView = header
        headerScroll.hasVerticalScroller = true
        headerScroll.drawsBackground = false
        headerScroll.autoresizingMask = [.width, .height]
        // 右上に見せ方の切り替え（簡潔・標準・詳細）を重ねる。文字は右端を空けて折り返す。
        let headerContainer = NSView(frame: headerScroll.frame)
        headerContainer.addSubview(headerScroll)
        styleControl.controlSize = .mini
        styleControl.font = .systemFont(ofSize: NSFont.systemFontSize(for: .mini))
        styleControl.selectedSegment = CommitHeaderStyle.saved.rawValue
        styleControl.target = self
        styleControl.action = #selector(styleChanged)
        styleControl.sizeToFit()
        styleControl.frame.origin = NSPoint(
            x: headerContainer.bounds.width - styleControl.frame.width - 16,
            y: headerContainer.bounds.height - styleControl.frame.height - 4)
        styleControl.autoresizingMask = [.minXMargin, .minYMargin]
        headerContainer.addSubview(styleControl)
        if CommitHeaderStyle.saved == .compact {
            // 簡潔で始めるなら、最初から低くしておく。
            headerContainer.frame.size.height = 48
        }
        // ウィンドウの高さを変えたとき、メッセージと変更ファイルの高さは保ち、差分の側を伸び縮みさせる。
        for (content, minimum) in [(headerContainer, 40.0), (scroll, 60.0)] {
            let controller = NSViewController()
            controller.view = content
            let item = NSSplitViewItem(viewController: controller)
            item.minimumThickness = minimum
            item.holdingPriority = .init(260)
            addSplitViewItem(item)
        }
        let diffItem = NSSplitViewItem(viewController: diffView)
        diffItem.minimumThickness = 200
        addSplitViewItem(diffItem)
        NotificationCenter.default.addObserver(
            self, selector: #selector(messageStyleChanged), name: .commitMessageStyleChanged, object: nil)
    }

    @objc private func messageStyleChanged() { renderHeader() }

    /// ヘッダーの右クリック: ID・件名・メッセージ・git show のコピーを先頭に足す。
    func textView(_ view: NSTextView, menu: NSMenu, for event: NSEvent, at charIndex: Int) -> NSMenu? {
        guard view === header, let info else { return menu }
        let message = info.message.trimmingCharacters(in: .whitespacesAndNewlines)
        let subject = message.components(separatedBy: "\n").first ?? message
        let items: [(String, String)] = [
            ("ID をコピー", info.oid),
            ("短い ID をコピー", String(info.oid.prefix(10))),
            ("件名をコピー", subject),
            ("メッセージをコピー", message),
            ("git show コマンドをコピー", "git show \(info.oid)"),
        ]
        for (i, (title, value)) in items.enumerated() {
            menu.insertItem(ClosureMenuItem(title: title) {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(value, forType: .string)
            }, at: i)
        }
        menu.insertItem(.separator(), at: items.count)
        return menu
    }

    func overlays() -> [(CGImage, NSView)] {
        view.window == nil ? [] : diffView.overlay().map { [$0] } ?? []
    }

    /// `commit` の変更を裏で読んで表示する。`stash` なら stash の中身（untracked のファイルを含む）。
    /// 差分を検索する（`--find` の確認用）。
    func find(_ query: String) { diffView.find(query) }

    func show(commit: Data?, stash: Bool = false) {
        request += 1
        let request = self.request
        guard let commit else {
            changes = nil
            files.reloadData()
            diffView.show(nil, message: "コミットを選ぶと、変更が表示されます")
            info = nil
            extras = nil
            header.string = ""
            return
        }
        let repository = self.repository
        let hex = commit.map { String(format: "%02x", $0) }.joined()
        commitHex = stash ? nil : hex
        selectedAt = CACurrentMediaTime()
        // ファイルの差分の読み込みとは別の番号で、古い結果を捨てる。
        headerRequest += 1
        let headerRequest = self.headerRequest
        let selectedAt = self.selectedAt
        // 裏の読み込みはここですぐ始める（メインスレッドが表のスクロールや描画をしている間に進める）。
        let infoTask = Task.detached(priority: .userInitiated) { try repository.commitInfo(oid: hex) }
        // 変わったファイルと、最初のファイルの差分を続けて読む（メインスレッドとの往復を 1 回で済ませる）。
        let changesTask = Task.detached(priority: .userInitiated) {
            () -> (CommitChanges?, FileDiff?, CFTimeInterval, CFTimeInterval) in
            let t = CACurrentMediaTime()
            let changes = try? stash ? repository.stashChanges(of: commit) : repository.changes(of: commit)
            let first = changes.flatMap { $0.items.isEmpty ? nil : try? $0.diff(at: 0) }
            return (changes, first, t, CACurrentMediaTime())
        }
        // 署名の検証や含むタグなど時間のかかる分は、ヘッダーを出した後に足す（stash では読まない）。
        let signatureGeneration = self.signatureGeneration
        let extrasTask = stash ? nil : Task.detached(priority: .utility) { try? repository.commitExtras(oid: hex) }
        Task {
            guard let info = try? await infoTask.value, headerRequest == self.headerRequest else { return }
            FrameProfile.add("select: commit info (JSON)", CACurrentMediaTime() - selectedAt)
            self.info = info
            extras = nil
            bodyExpanded = false
            FrameProfile.measure("select: header layout") { renderHeader() }
            FrameProfile.add("select: → header shown", CACurrentMediaTime() - selectedAt)
            guard let extras = await extrasTask?.value, headerRequest == self.headerRequest else { return }
            // 読んでいる間に検証する側が変わっていたら、その結果は使わずに読み直す。
            guard signatureGeneration == self.signatureGeneration else {
                loadExtras(oid: hex)
                return
            }
            self.extras = extras
            renderHeader()
        }
        Task {
            let (changes, firstDiff, callStart, callEnd) = await changesTask.value
            guard request == self.request else { return }
            let resumed = CACurrentMediaTime()
            FrameProfile.add("select: changed files: start delay", callStart - selectedAt)
            FrameProfile.add("select: changed files: in call", callEnd - callStart)
            FrameProfile.add("select: changed files: main resume delay", resumed - callEnd)
            self.changes = changes
            files.reloadData()
            if changes?.items.isEmpty ?? true {
                diffView.show(nil, message: "変更なし")
                onDiffShown?()
            } else {
                // 最初のファイルの差分は読んであるので、選んだときに読み直さない。
                preloadedDiff = firstDiff
                files.selectRowIndexes([0], byExtendingSelection: false)
            }
        }
    }

    private func renderHeader() {
        guard let info else { return }
        let style = CommitHeaderStyle(rawValue: styleControl.selectedSegment) ?? .standard
        var renderer = CommitHeaderRenderer(info: info, extras: extras, style: style)
        renderer.monospacedBody = Settings.monospacedCommitMessage
        renderer.expanded = bodyExpanded
        renderer.highlight = CommitHeaderRenderer.highlightTerms(of: searchQuery)
        let text = NSMutableAttributedString(attributedString: renderer.render())
        // 右上の切り替えに文字が重ならないよう、最初の行の右端を空ける。
        let first = NSMutableParagraphStyle()
        first.tailIndent = -(styleControl.frame.width + 12)
        let firstLine = (text.string as NSString).lineRange(for: NSRange(location: 0, length: 0))
        text.addAttribute(.paragraphStyle, value: first, range: firstLine)
        header.textStorage?.setAttributedString(text)
    }

    /// 検証する側が変わったので、表示中のコミットの署名を検証し直す（前の結果は使わない）。コミットの情報を
    /// まだ読んでいる途中なら、読み終えたときに番号の違いに気づいて読み直す。
    func reloadSignature() {
        signatureGeneration += 1
        guard let info else { return }
        if info.signed != nil {
            extras = nil
            renderHeader()
        }
        loadExtras(oid: info.oid)
    }

    /// 詳細の時間のかかる分を読み直す。読んでいる間にコミットか検証する側が変わったら、その結果は捨てる。
    private func loadExtras(oid hex: String) {
        let request = headerRequest, generation = signatureGeneration
        let repository = self.repository
        Task {
            let extras = await Task.detached(priority: .utility) { try? repository.commitExtras(oid: hex) }.value
            guard let extras, request == headerRequest, generation == signatureGeneration else { return }
            self.extras = extras
            renderHeader()
        }
    }

    @objc private func styleChanged() {
        let previous = CommitHeaderStyle.saved
        let style = CommitHeaderStyle(rawValue: styleControl.selectedSegment) ?? .standard
        CommitHeaderStyle.saved = style
        renderHeader()
        fitHeader(from: previous, to: style)
    }

    /// 簡潔にしたら欄を文字の高さまで縮め、ほかに戻したら前の高さに戻す。
    private var expandedHeaderHeight: CGFloat = 170

    private func fitHeader(from previous: CommitHeaderStyle, to style: CommitHeaderStyle) {
        guard splitViewItems.count > 1 else { return }
        let current = splitViewItems[0].viewController.view.frame.height
        if previous != .compact { expandedHeaderHeight = current }
        let target: CGFloat
        if style == .compact {
            guard let layout = header.layoutManager, let container = header.textContainer else { return }
            layout.ensureLayout(for: container)
            target = ceil(layout.usedRect(for: container).height + header.textContainerInset.height * 2 + 4)
        } else {
            target = expandedHeaderHeight
        }
        splitView.setPosition(max(splitViewItems[0].minimumThickness, target), ofDividerAt: 0)
    }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        guard let s = (link as? String) ?? (link as? URL)?.absoluteString else { return false }
        if s.hasPrefix(CommitHeaderRenderer.authorScheme) {
            let name = String(s.dropFirst(CommitHeaderRenderer.authorScheme.count)).replacingOccurrences(of: "\"", with: "")
            onSearch?("author:\"\(name)\"")
            return true
        }
        if s.hasPrefix(CommitHeaderRenderer.refScheme) {
            onSelectRef?(String(s.dropFirst(CommitHeaderRenderer.refScheme.count)))
            return true
        }
        if s == CommitHeaderRenderer.expandLink {
            bodyExpanded.toggle()
            renderHeader()
            return true
        }
        if s.hasPrefix(CommitHeaderRenderer.commitScheme) {
            onSelectCommit?(String(s.dropFirst(CommitHeaderRenderer.commitScheme.count)))
            return true
        }
        if s.hasPrefix(CommitHeaderRenderer.revScheme) {
            // メッセージの中の短い ID。コミットを引けたら選ぶ（引けなければ何もしない）。
            let rev = String(s.dropFirst(CommitHeaderRenderer.revScheme.count))
            let repository = self.repository
            Task {
                guard let hex = try? await Task.detached(priority: .userInitiated) { try repository.resolveCommit(rev) }.value
                else { NSSound.beep(); return }
                onSelectCommit?(hex)
            }
            return true
        }
        // URL は既定どおりブラウザで開く。
        return false
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        changes?.items.count ?? 0
    }

    private func showFileDiff(_ diff: FileDiff?) {
        if let diff, diff.isBinary {
            diffView.show(nil, message: "バイナリファイル")
        } else {
            diffView.show(diff)
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let row = files.clickedRow
        guard row >= 0, let item = changes?.items[row] else { return }
        let path = item.path
        if let commit = commitHex, item.kind != .deleted {
            menu.addItem(ClosureMenuItem(title: "Blame at \(commit.prefix(8))") { [weak self] in self?.onBlame?(path, commit) })
        }
        menu.addItem(ClosureMenuItem(title: "File History") { [weak self] in self?.onFileHistory?(path) })
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
        if index == 0, let diff = preloadedDiff {
            preloadedDiff = nil
            FrameProfile.measure("select: diff draw") { showFileDiff(diff) }
            FrameProfile.add("select: → diff shown", CACurrentMediaTime() - selectedAt)
            onDiffShown?()
            return
        }
        preloadedDiff = nil
        let selectedAt = self.selectedAt
        Task {
            let started = CACurrentMediaTime()
            let diff = try? await Task.detached(priority: .userInitiated) { try changes.diff(at: index) }.value
            guard request == self.request else { return }
            FrameProfile.add("select: file diff (FFI)", CACurrentMediaTime() - started)
            FrameProfile.measure("select: diff draw") { showFileDiff(diff) }
            FrameProfile.add("select: → diff shown", CACurrentMediaTime() - selectedAt)
            onDiffShown?()
        }
    }
}
