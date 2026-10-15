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
            let spans = await Task.detached(priority: .userInitiated) { diff.syntax() }.value
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
        var colors = [SIMD4<Float>](repeating: NSColor.labelColor.rgba, count: 16)
        let pairs: [(Int32, NSColor)] = [
            (AmonhenSyntaxSpan_KEYWORD, .systemPink),
            (AmonhenSyntaxSpan_STRING, .systemRed),
            (AmonhenSyntaxSpan_COMMENT, .systemGray),
            (AmonhenSyntaxSpan_NUMBER, .systemPurple),
            (AmonhenSyntaxSpan_FUNCTION, .systemBlue),
            (AmonhenSyntaxSpan_TYPE, .systemTeal),
            (AmonhenSyntaxSpan_CONSTANT, .systemPurple),
            (AmonhenSyntaxSpan_PROPERTY, .systemCyan),
            (AmonhenSyntaxSpan_ATTRIBUTE, .systemOrange),
            (AmonhenSyntaxSpan_TAG, .systemPink),
            (AmonhenSyntaxSpan_OPERATOR, .labelColor),
            (AmonhenSyntaxSpan_VARIABLE, .systemPink),
            (AmonhenSyntaxSpan_ESCAPE, .systemOrange),
            (AmonhenSyntaxSpan_MODULE, .systemBrown),
        ]
        for (kind, color) in pairs { colors[Int(kind)] = color.rgba }
        return colors
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
    NSTextViewDelegate
{
    /// 親コミットのリンクを押したとき（40 桁の ID）。
    var onSelectCommit: ((String) -> Void)?
    /// メッセージ・作者・ID・親コミット。
    private let header = NSTextView(frame: NSRect(x: 0, y: 0, width: 240, height: 150))
    private let repository: Repository
    private let files = NSTableView()
    private let diffView: DiffViewController
    private var changes: CommitChanges?
    private var request = 0
    private var headerRequest = 0

    init(repository: Repository, renderer: TextRenderer) {
        self.repository = repository
        self.diffView = DiffViewController(renderer: renderer)
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    private var fileColumnPlaced = false

    override func viewDidLayout() {
        super.viewDidLayout()
        placeFileColumnOnce(&fileColumnPlaced)
    }

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
        header.isEditable = false
        header.drawsBackground = true
        header.backgroundColor = .textBackgroundColor
        header.textContainerInset = NSSize(width: 6, height: 6)
        header.delegate = self
        header.autoresizingMask = [.width]
        header.isVerticallyResizable = true
        header.minSize = NSSize(width: 0, height: 0)
        header.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        header.textContainer?.widthTracksTextView = true
        let headerScroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 240, height: 150))
        headerScroll.documentView = header
        headerScroll.hasVerticalScroller = true
        headerScroll.drawsBackground = false
        // Working Copy の左の列と同じ幅から始める。
        let leftColumn = NSSplitView(frame: NSRect(x: 0, y: 0, width: 280, height: 300))
        leftColumn.isVertical = false
        leftColumn.dividerStyle = .thin
        // Working Copy と同じく、上にファイル、下にメッセージ（Working Copy ではコミットの入力）。
        leftColumn.addArrangedSubview(scroll)
        leftColumn.addArrangedSubview(headerScroll)
        leftColumn.setHoldingPriority(.init(260), forSubviewAt: 1)
        let list = NSViewController()
        list.view = leftColumn
        let listItem = NSSplitViewItem(viewController: list)
        listItem.minimumThickness = 220
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

    /// `commit` の変更を裏で読んで表示する。`stash` なら stash の中身（untracked のファイルを含む）。
    /// 差分を検索する（`--find` の確認用）。
    func find(_ query: String) { diffView.find(query) }

    func show(commit: Data?, stash: Bool = false) {
        request += 1
        let request = self.request
        guard let commit else {
            changes = nil
            files.reloadData()
            diffView.show(nil)
            header.string = ""
            return
        }
        let repository = self.repository
        let hex = commit.map { String(format: "%02x", $0) }.joined()
        // ファイルの差分の読み込みとは別の番号で、古い結果を捨てる。
        headerRequest += 1
        let headerRequest = self.headerRequest
        Task {
            guard let info = try? await Task.detached(priority: .userInitiated, operation: { try repository.commitInfo(oid: hex) }).value,
                  headerRequest == self.headerRequest else { return }
            showHeader(info)
        }
        Task {
            let changes = try? await Task.detached(priority: .userInitiated) {
                try stash ? repository.stashChanges(of: commit) : repository.changes(of: commit)
            }.value
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

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .medium
        return f
    }()

    private func showHeader(_ info: CommitInfo) {
        let text = NSMutableAttributedString()
        func add(_ s: String, _ attrs: [NSAttributedString.Key: Any]) {
            text.append(NSAttributedString(string: s, attributes: attrs))
        }
        let body: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.labelColor]
        let dim: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor]
        let mono: [NSAttributedString.Key: Any] = [.font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular), .foregroundColor: NSColor.secondaryLabelColor]
        let message = info.message.trimmingCharacters(in: .whitespacesAndNewlines)
        let (subject, rest) = message.firstIndex(of: "\n").map { (String(message[..<$0]), String(message[$0...])) } ?? (message, "")
        add(subject, [.font: NSFont.boldSystemFont(ofSize: 13), .foregroundColor: NSColor.labelColor])
        add(rest + "\n\n", body)
        if !info.refs.isEmpty {
            add(info.refs.joined(separator: ", ") + "\n", [.font: NSFont.boldSystemFont(ofSize: 11), .foregroundColor: NSColor.systemBlue])
        }
        let date = Self.dateFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(info.authorTime)))
        add("\(info.author) <\(info.authorEmail)>  \(date)\n", dim)
        if info.committer != info.author {
            add("Committed by \(info.committer)\n", dim)
        }
        add("Commit  \(info.oid)\n", mono)
        for parent in info.parents {
            add("Parent  ", mono)
            var link = mono
            link[.link] = "amonhen-commit:\(parent)"
            add(String(parent.prefix(12)) + "\n", link)
        }
        header.textStorage?.setAttributedString(text)
    }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        guard let s = (link as? String) ?? (link as? URL)?.absoluteString, s.hasPrefix("amonhen-commit:") else { return false }
        onSelectCommit?(String(s.dropFirst("amonhen-commit:".count)))
        return true
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
