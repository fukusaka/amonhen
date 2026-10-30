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
    private(set) var isFindBarVisible = false
    private var matches: [(row: Int, range: Range<Int>)] = []
    private var currentMatch: Int?

    private let font = CTFontCreateWithName("Menlo" as CFString, 12, nil)
    private var lineHeight: CGFloat { ceil(CTFontGetAscent(font) + CTFontGetDescent(font) + CTFontGetLeading(font)) + 4 }
    /// 行番号の欄の幅。
    private let numbersWidth: CGFloat = 96
    /// 変更の行の左のチェックボックスの欄（`showsStageChecks` のときだけ）。
    private let checkWidth: CGFloat = 22
    private var gutterWidth: CGFloat { numbersWidth + (showsStageChecks ? checkWidth : 0) }
    /// Working Copy の HEAD と作業ツリーの差分か。変更の行と hunk の見出しの左に、次のコミットに含めるかの
    /// チェックボックスを出す。
    private var showsStageChecks: Bool { (diff as? WorkingFileDiff)?.isCombined == true }
    /// チェックボックスを押した・Space を押した: `rows` 行目の変更を含める（true）・外す。
    var onSetStaged: ((_ rows: [Int], _ staged: Bool) -> Void)?
    /// 全ファイルを並べた差分の、ファイルの見出しの行（`AmonhenDiffRow_FILE`）。i 番目がファイル i。
    private var fileRows: [Int] = []
    /// 見出しを押した: ファイルを畳む・開く。Viewed を押した: 見たことにする・戻す。
    var onToggleFile: ((Int) -> Void)?
    var onToggleViewed: ((Int) -> Void)?
    /// 見たことにしたファイル（見出しに ✓ を出す）。
    var viewedFiles: Set<Int> = [] { didSet { if viewedFiles != oldValue { redraw() } } }
    /// 一番上に見えているファイルが変わった（変更ファイルの一覧の選択を合わせる）。
    var onTopFileChange: ((Int) -> Void)?
    private var topFile: Int?
    private var headerGlyphs: [String: TextRenderer.ShapedLine] = [:]
    private var checkGlyphs: [String: TextRenderer.ShapedLine] = [:]
    /// 上の並べた文字を作ったときの倍率。
    private var cachedScale: CGFloat?

    /// 倍率込みで並べた文字（行、行番号、見出し、チェックの印）を全部捨てる。1 つでも残すと、前の倍率の
    /// グリフで描かれて崩れる。
    private func dropShapedText() {
        shaped.removeAll()
        numberCache.removeAll()
        headerGlyphs.removeAll()
        checkGlyphs.removeAll()
    }

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
        // 画面の倍率が変わったら、倍率込みで並べた文字（行、行番号、見出し、チェックの印）を全部捨てて描き直す。
        // 1 つでも残すと、前の倍率のグリフで描かれて崩れる（行番号で起きていた）。
        textView.onScaleChange = { [weak self] in
            self?.dropShapedText()
            self?.redraw()
        }
        document.owner = self
        buildFindBar()
        container.addSubview(scrollView)
        container.addSubview(textView)
        container.addSubview(findBar)
        view = container
    }

    private func buildFindBar() {
        findBar.isHidden = true
        findField.placeholderString = L("Search Diff")
        findField.sendsSearchStringImmediately = true
        findField.delegate = self
        findField.target = self
        findField.action = #selector(findFieldChanged)
        findField.controlSize = .small
        findCount.textColor = .secondaryLabelColor
        findCount.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        let previous = NSButton(image: NSImage(systemSymbolName: "chevron.up", accessibilityDescription: L("Previous"))!,
                                target: self, action: #selector(findPrevious))
        let next = NSButton(image: NSImage(systemSymbolName: "chevron.down", accessibilityDescription: L("Next"))!,
                            target: self, action: #selector(findNext))
        let done = NSButton(title: L("Done"), target: self, action: #selector(hideFindBar))
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
        headerGlyphs.removeAll()
        fileRows = diff?.withRows { all in all.indices.filter { all[$0].kind == UInt8(AmonhenDiffRow_FILE) } } ?? []
        topFile = nil
        selectedRows.removeAll()
        anchorRow = nil
        textSelection = nil
        onSelectionChange?()
        if isFindBarVisible { updateMatches(scrollToCurrent: false) }
        if keepScroll {
            updateDocumentSize()
            redraw()
            if let hunk = hunkToSelectAfterShow {
                hunkToSelectAfterShow = nil
                selectHunk(min(hunk, max(0, hunkRows.count - 1)))
            }
            return
        }
        hunkToSelectAfterShow = nil
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

    /// `point`（文書の座標）が、ファイルの見出し（上に張り付いたものを含む）なら、そのファイルの番号。
    private func fileHeader(at point: NSPoint) -> Int? {
        guard !fileRows.isEmpty else { return nil }
        let origin = scrollView.contentView.bounds.origin
        if let (file, top) = stickyHeader(origin: origin), point.y - origin.y >= top, point.y - origin.y < top + lineHeight {
            return file
        }
        let row = Int(point.y / lineHeight)
        return fileRows.firstIndex(of: row)
    }

    /// 上に張り付かせる見出し（ファイルの番号と、見えている範囲の上端からの位置）。見出しの行が見えている範囲より
    /// 上にあるファイルのものを出し、次のファイルの見出しが近づいたら押し上げる。
    private func stickyHeader(origin: CGPoint) -> (Int, CGFloat)? {
        let first = Int(origin.y / lineHeight)
        // 最初に見えている行が属するファイル（その見出しが first 以前で最後のもの）。
        guard let file = fileRows.lastIndex(where: { $0 <= first }) else { return nil }
        // 見出しの行がちょうど一番上に見えているなら、張り付かせなくてよい。
        if CGFloat(fileRows[file]) * lineHeight >= origin.y { return nil }
        var top: CGFloat = 0
        if file + 1 < fileRows.count {
            top = min(0, CGFloat(fileRows[file + 1]) * lineHeight - origin.y - lineHeight)
        }
        return (file, top)
    }

    /// ファイル `index` の見出しが一番上に来るようにスクロールする。
    func scrollToFile(_ index: Int) {
        guard index < fileRows.count else { return }
        let clip = scrollView.contentView
        let y = min(CGFloat(fileRows[index]) * lineHeight, max(0, document.frame.height - clip.bounds.height))
        clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: y))
        scrollView.reflectScrolledClipView(clip)
    }

    /// ファイル `index` の見出しが見えている範囲より上にあれば、見出しが一番上に来るまで戻す。
    func revealFileHeader(_ index: Int) {
        guard index < fileRows.count else { return }
        if CGFloat(fileRows[index]) * lineHeight < scrollView.contentView.bounds.origin.y { scrollToFile(index) }
    }

    /// ファイルの見出しの右クリック（blame・ファイルの履歴など）。ファイルの番号を渡し、メニューを返してもらう。
    var fileMenu: ((Int) -> NSMenu?)?

    /// AI で hunk を解説する（ファイルのパスと、hunk の差分の文字を渡す）。持ち主が設定したときだけ右クリックに出す。
    var onExplainHunk: ((String, String) -> Void)?
    /// 1 ファイルの差分のときのパス（全ファイルを並べた差分なら見出しの行から取る）。
    var path: String?

    fileprivate func menu(at point: NSPoint) -> NSMenu? {
        if let file = fileHeader(at: point) { return fileMenu?(file) }
        guard let onExplainHunk, Assistant.isConfigured, let (path, text) = hunkText(containing: Int(point.y / lineHeight))
        else { return nil }
        let menu = NSMenu()
        menu.addItem(ClosureMenuItem(title: L("Explain This Hunk with AI")) { onExplainHunk(path, text) })
        return menu
    }

    /// `row` 行目を含む hunk の差分の文字（見出しから次の hunk・ファイルの手前まで）と、そのファイルのパス。
    private func hunkText(containing row: Int) -> (String, String)? {
        guard let diff else { return nil }
        return diff.withRows { rows -> (String, String)? in
            guard row >= 0, row < rows.count,
                  let start = rows.indices.prefix(row + 1).last(where: { rows[$0].kind == UInt8(AmonhenDiffRow_HUNK) })
            else { return nil }
            // 見出しより後ろで、次の hunk の前（ファイルの見出しが間にあれば、その hunk は別のファイル）。
            if rows.indices[start...row].contains(where: { rows[$0].kind == UInt8(AmonhenDiffRow_FILE) }) { return nil }
            var end = start + 1
            while end < rows.count, rows[end].kind != UInt8(AmonhenDiffRow_HUNK), rows[end].kind != UInt8(AmonhenDiffRow_FILE) {
                end += 1
            }
            let file = rows.indices.prefix(start).last { rows[$0].kind == UInt8(AmonhenDiffRow_FILE) }
            var out = ""
            for i in start..<end {
                let text = diff.text(row: i)
                switch rows[i].kind {
                case UInt8(AmonhenDiffRow_HUNK): out += text + "\n"
                case UInt8(AmonhenDiffRow_ADDED): out += "+" + text + "\n"
                case UInt8(AmonhenDiffRow_REMOVED): out += "-" + text + "\n"
                default: out += " " + text + "\n"
                }
                if out.utf8.count > DiffText.limit { break }
            }
            return (file.map { diff.text(row: $0) } ?? path ?? "", out)
        }
    }

    /// 見出しの Viewed のチェックボックスの左端（見えている範囲の座標）。
    private var viewedBoxX: CGFloat { textView.bounds.width - 76 }

    /// ⌥J・⌥K: 次・前のファイル（全ファイルを並べた差分なら見出しへ、そうでなければ持ち主に頼む）。
    var onMoveFile: ((Int) -> Void)?
    /// 表示し直した後に選ぶ hunk の番号（含める・外すの後に次の hunk へ進むため）。
    var hunkToSelectAfterShow: Int?

    /// hunk の見出しの行。
    private var hunkRows: [Int] {
        diff?.withRows { all in all.indices.filter { all[$0].kind == UInt8(AmonhenDiffRow_HUNK) } } ?? []
    }

    /// 今の hunk の番号（選んだ行か、見えている一番上の行を含む hunk）。
    private var currentHunk: Int? {
        let hunks = hunkRows
        guard !hunks.isEmpty else { return nil }
        let row = selectedRows.min() ?? Int(scrollView.contentView.bounds.origin.y / lineHeight)
        return hunks.lastIndex { $0 <= row }
    }

    /// J・K: 次・前の hunk を選んで、上から 3 分の 1 あたりに出す。
    fileprivate func moveHunk(by step: Int) {
        let hunks = hunkRows
        guard !hunks.isEmpty else { return NSSound.beep() }
        let target: Int
        if selectedRows.isEmpty {
            // 選んでいなければ、見えている一番上から: J はそこ以降の最初、K はそれより前の最後。
            let top = Int(scrollView.contentView.bounds.origin.y / lineHeight)
            target = step > 0 ? (hunks.firstIndex { $0 >= top } ?? hunks.count) : (hunks.lastIndex { $0 < top } ?? -1)
        } else {
            target = (currentHunk ?? 0) + step
        }
        guard hunks.indices.contains(target) else { return NSSound.beep() }
        selectHunk(target)
    }

    /// `index` 番目の hunk を選び（選べる表示なら）、見えるところへスクロールする。
    func selectHunk(_ index: Int) {
        let hunks = hunkRows
        guard hunks.indices.contains(index) else { return }
        let start = hunks[index]
        let end = index + 1 < hunks.count ? hunks[index + 1] : (diff?.rowCount ?? start + 1)
        if isSelectable {
            selectedRows = Set(start..<end)
            anchorRow = start
            onSelectionChange?()
        }
        let clip = scrollView.contentView
        let top = CGFloat(start) * lineHeight
        let y = max(0, min(top - clip.bounds.height / 3, document.frame.height - clip.bounds.height))
        if top < clip.bounds.origin.y || CGFloat(end) * lineHeight > clip.bounds.origin.y + clip.bounds.height {
            clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: y))
            scrollView.reflectScrolledClipView(clip)
        }
        redraw()
    }

    fileprivate func moveFile(by step: Int) {
        if !fileRows.isEmpty {
            let target = (topFile ?? 0) + step
            guard fileRows.indices.contains(target) else { return NSSound.beep() }
            scrollToFile(target)
        } else if let onMoveFile {
            onMoveFile(step)
        } else {
            NSSound.beep()
        }
    }

    /// `row` を含む hunk の、追加・削除の行（`row` が hunk の見出しでも中の行でもよい）。
    private func changeRows(inHunkOf row: Int) -> [Int] {
        guard let diff else { return [] }
        return diff.withRows { all in
            var start = row
            while start > 0, all[start].kind != UInt8(AmonhenDiffRow_HUNK) { start -= 1 }
            var end = start + 1
            while end < all.count, all[end].kind != UInt8(AmonhenDiffRow_HUNK) { end += 1 }
            return (start..<end).filter { Self.isChange(all[$0]) }
        }
    }

    private static func isChange(_ row: AmonhenDiffRow) -> Bool {
        row.kind == UInt8(AmonhenDiffRow_ADDED) || row.kind == UInt8(AmonhenDiffRow_REMOVED)
    }

    /// `rows` の変更を、すべて含めていれば外し、そうでなければすべて含める。切り替えた後は、次の hunk へ進む。
    private func toggleStaged(_ rows: [Int]) {
        guard let diff = diff as? WorkingFileDiff, !rows.isEmpty else { return }
        let hunks = hunkRows
        if let last = rows.max(), let index = hunks.lastIndex(where: { $0 <= last }) {
            hunkToSelectAfterShow = index + 1 < hunks.count ? index + 1 : index
        }
        onSetStaged?(rows, !rows.allSatisfy { diff.isStaged(row: $0) })
    }

    /// Space: 選んだ変更の行（なければ何もしない）を含める・外す。扱ったら true。
    fileprivate func toggleSelectedStaged() -> Bool {
        guard showsStageChecks, !selectedChangeRows.isEmpty else { return false }
        toggleStaged(selectedChangeRows)
        return true
    }

    /// クリックで行を選ぶ。hunk の見出しなら hunk 全体、Shift で範囲、Command で追加・解除。
    /// 左端のチェックボックスなら、その行（hunk の見出しなら hunk の変更すべて）を含める・外す。
    fileprivate func mouseDown(_ event: NSEvent, at point: NSPoint) {
        guard let diff else { return }
        if let file = fileHeader(at: point) {
            let visibleX = point.x - scrollView.contentView.bounds.origin.x
            if visibleX > viewedBoxX - 4 { onToggleViewed?(file) } else { onToggleFile?(file) }
            return
        }
        if showsStageChecks, point.x - scrollView.contentView.bounds.origin.x < checkWidth {
            let row = Int(point.y / lineHeight)
            guard row >= 0, row < diff.rowCount else { return }
            let kind = diff.withRows { $0[row].kind }
            if kind == UInt8(AmonhenDiffRow_HUNK) {
                toggleStaged(changeRows(inHunkOf: row))
            } else if kind == UInt8(AmonhenDiffRow_ADDED) || kind == UInt8(AmonhenDiffRow_REMOVED) {
                toggleStaged([row])
            }
            return
        }
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
            findCount.stringValue = L("Not found")
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
        // 並べた文字は倍率込み。描く倍率が並べたときと違えば、知らせを待たずにここで捨てる。
        if cachedScale != scale { dropShapedText() }
        cachedScale = scale
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
                if r.kind == UInt8(AmonhenDiffRow_FILE) {
                    drawFileHeader(row: row, all: all, top: top, scale: scale)
                    continue
                }
                if showsStageChecks {
                    drawStageCheck(row: row, kind: r.kind, all: all, top: top, scale: scale)
                }
                let numbers = isConflictView
                    ? [(r.new_no, gutterWidth - 6)]
                    : [(r.old_no, gutterWidth - numbersWidth / 2 - 6), (r.new_no, gutterWidth - 6)]
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
            // 見出しの行が上に隠れたファイルは、見出しを上に張り付かせる。
            if let (file, top) = stickyHeader(origin: origin) {
                drawFileHeader(row: fileRows[file], all: all, top: top, scale: scale)
            }
        }
        if !fileRows.isEmpty {
            let file = fileRows.lastIndex { CGFloat($0) * lineHeight <= origin.y + lineHeight / 2 } ?? 0
            if file != topFile {
                topFile = file
                onTopFileChange?(file)
            }
        }
    }
}

extension DiffViewController {
    /// ファイルの見出しの行: 畳む・開くの印、パス、追加・削除の行数、Viewed のチェックボックス。横にはスクロールしない。
    fileprivate func drawFileHeader(
        row: Int, all: UnsafeBufferPointer<AmonhenDiffRow>, top: CGFloat, scale: CGFloat
    ) {
        let r = all[row]
        let width = textView.bounds.width
        renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: NSColor.windowBackgroundColor.rgba)
        renderer.addRect(CGRect(x: 0, y: top + lineHeight - 1, width: width, height: 1), color: NSColor.separatorColor.rgba)
        let small = NSFont.systemFont(ofSize: 11) as CTFont
        let bold = NSFont.boldSystemFont(ofSize: 11) as CTFont
        func glyph(_ text: String, _ font: CTFont) -> TextRenderer.ShapedLine {
            let key = "\(text)|\(CTFontGetSize(font))|\(font === bold ? "b" : "r")"
            if let cached = headerGlyphs[key] { return cached }
            let line = renderer.shape(text as CFString, font: font, scale: scale)
            headerGlyphs[key] = line
            return line
        }
        let baseline = top + (lineHeight + CTFontGetAscent(small) - CTFontGetDescent(small)) / 2
        let collapsed = r.flags & UInt8(AmonhenDiffRow_COLLAPSED) != 0
        let large = NSFont.systemFont(ofSize: 14) as CTFont
        let chevron = glyph(collapsed ? "▸" : "▾", large)
        let chevronBaseline = top + (lineHeight + CTFontGetAscent(large) - CTFontGetDescent(large)) / 2
        renderer.addLine(
            chevron, origin: CGPoint(x: 12 - chevron.width / 2, y: chevronBaseline), color: NSColor.secondaryLabelColor.rgba,
            scale: scale)
        // 右から: Viewed、行数。
        let file = fileRows.firstIndex(of: row) ?? 0
        let viewed = viewedFiles.contains(file)
        let box = CGRect(x: viewedBoxX, y: top + (lineHeight - 12) / 2, width: 12, height: 12)
        let label = glyph("Viewed", small)
        // 行数（バイナリならその旨）は Viewed の左に右揃えで置く。
        let counts: [(TextRenderer.ShapedLine, NSColor)] = r.flags & UInt8(AmonhenDiffRow_BINARY) != 0
            ? [(glyph(L("Binary"), small), .secondaryLabelColor)]
            : [(glyph("+\(r.new_no)", bold), .systemGreen), (glyph("−\(r.old_no)", bold), .systemRed)]
        let countsWidth = counts.map(\.0.width).reduce(0, +) + CGFloat(counts.count - 1) * 6
        let countsX = box.minX - 14 - countsWidth
        // パスは行数の手前までに収める（長ければ、はみ出た分を帯の色で隠す）。
        if let line = shapedLine(row, scale: scale) {
            let textBaseline = top + 2 + CTFontGetAscent(font)
            renderer.addLine(line, origin: CGPoint(x: 24, y: textBaseline), color: NSColor.labelColor.rgba, scale: scale)
            if 24 + line.width > countsX - 8 {
                renderer.addRect(
                    CGRect(x: countsX - 8, y: top, width: width - countsX + 8, height: lineHeight - 1),
                    color: NSColor.windowBackgroundColor.rgba)
            }
        }
        var x = countsX
        for (line, color) in counts {
            renderer.addLine(line, origin: CGPoint(x: x, y: baseline), color: color.rgba, scale: scale)
            x += line.width + 6
        }
        // Viewed は隠す帯より後に描く。
        if viewed {
            renderer.addRect(box, color: NSColor.controlAccentColor.rgba)
            let check = glyph("✓", bold)
            renderer.addLine(check, origin: CGPoint(x: box.midX - check.width / 2, y: baseline), color: NSColor.white.rgba, scale: scale)
        } else {
            renderer.addRect(box, color: NSColor.secondaryLabelColor.rgba)
            renderer.addRect(box.insetBy(dx: 1, dy: 1), color: NSColor.windowBackgroundColor.rgba)
        }
        renderer.addLine(label, origin: CGPoint(x: box.maxX + 5, y: baseline), color: NSColor.secondaryLabelColor.rgba, scale: scale)
    }

    /// 変更の行・hunk の見出しの左のチェックボックス。hunk は中の変更がすべて含まれていれば ✓、一部なら −。
    fileprivate func drawStageCheck(
        row: Int, kind: UInt8, all: UnsafeBufferPointer<AmonhenDiffRow>, top: CGFloat, scale: CGFloat
    ) {
        let state: NSControl.StateValue
        switch kind {
        case UInt8(AmonhenDiffRow_ADDED), UInt8(AmonhenDiffRow_REMOVED):
            state = all[row].flags & UInt8(AmonhenDiffRow_STAGED) != 0 ? .on : .off
        case UInt8(AmonhenDiffRow_HUNK):
            var end = row + 1
            var staged = 0, total = 0
            while end < all.count, all[end].kind != UInt8(AmonhenDiffRow_HUNK) {
                if all[end].kind == UInt8(AmonhenDiffRow_ADDED) || all[end].kind == UInt8(AmonhenDiffRow_REMOVED) {
                    total += 1
                    if all[end].flags & UInt8(AmonhenDiffRow_STAGED) != 0 { staged += 1 }
                }
                end += 1
            }
            state = staged == 0 ? .off : staged == total ? .on : .mixed
        default:
            return
        }
        let size: CGFloat = 12
        let box = CGRect(x: 5, y: top + (lineHeight - size) / 2, width: size, height: size)
        if state == .off {
            renderer.addRect(box, color: NSColor.secondaryLabelColor.rgba)
            renderer.addRect(box.insetBy(dx: 1, dy: 1), color: NSColor.textBackgroundColor.rgba)
            return
        }
        renderer.addRect(box, color: NSColor.controlAccentColor.rgba)
        let mark = state == .on ? "✓" : "−"
        let font = NSFont.boldSystemFont(ofSize: 10) as CTFont
        let glyph = checkGlyphs[mark] ?? renderer.shape(mark as CFString, font: font, scale: scale)
        checkGlyphs[mark] = glyph
        let baseline = box.midY + (CTFontGetAscent(font) - CTFontGetDescent(font)) / 2
        renderer.addLine(
            glyph, origin: CGPoint(x: box.midX - glyph.width / 2, y: baseline), color: NSColor.white.rgba, scale: scale)
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

    override func menu(for event: NSEvent) -> NSMenu? {
        owner?.menu(at: convert(event.locationInWindow, from: nil)) ?? super.menu(for: event)
    }

    override func keyDown(with event: NSEvent) {
        // Space: 選んだ変更の行を次のコミットに含める・外す（Working Copy の差分）。
        if event.charactersIgnoringModifiers == " ", owner?.toggleSelectedStaged() == true { return }
        // J・K: 次・前の hunk。⌥J・⌥K: 次・前のファイル。
        let flags = event.modifierFlags.intersection([.command, .control, .option, .shift])
        switch (event.charactersIgnoringModifiers?.lowercased(), flags) {
        case ("j", []): owner?.moveHunk(by: 1)
        case ("k", []): owner?.moveHunk(by: -1)
        case ("j", [.option]), ("∆", [.option]): owner?.moveFile(by: 1)
        case ("k", [.option]), ("˚", [.option]): owner?.moveFile(by: -1)
        default: super.keyDown(with: event)
        }
    }

    override func mouseDragged(with event: NSEvent) {
        autoscroll(with: event)
        owner?.mouseDragged(event, at: convert(event.locationInWindow, from: nil))
    }

    @objc func copy(_ sender: Any?) { owner?.copySelection() }
    override func selectAll(_ sender: Any?) { owner?.selectAllText() }
    @objc func showFindBar(_ sender: Any?) { owner?.showFindBar() }
    // ⌘G・⌘⇧G: この差分の中を探しているときはここで。そうでなければ History の検索の一致へ。
    @objc func findNext(_ sender: Any?) {
        if owner?.isFindBarVisible == true || !moveHistoryMatch(from: window, by: 1) { owner?.findNext() }
    }
    @objc func findPrevious(_ sender: Any?) {
        if owner?.isFindBarVisible == true || !moveHistoryMatch(from: window, by: -1) { owner?.findPrevious() }
    }
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
    /// 全ファイルを縦に並べて見せているときの、すべての差分と、ファイルごとに畳んでいるか。
    private var stacked: StackedDiff?
    private var collapsed: [Bool] = []
    /// 差分のスクロールに合わせて一覧の選択を変えている間（選択の変化でスクロールし返さない）。
    private var syncingSelection = false
    /// 見たことにしたファイル（コミット ID → パス）。次の起動でも覚えておく（古いものから捨てて 500 コミットまで）。
    private static var viewed: [String: Set<String>] = {
        let stored = UserDefaults.standard.dictionary(forKey: "viewedFiles") as? [String: [String]] ?? [:]
        return stored.mapValues(Set.init)
    }() {
        didSet {
            var order = UserDefaults.standard.stringArray(forKey: "viewedOrder") ?? []
            for key in viewed.keys where !order.contains(key) { order.append(key) }
            order.removeAll { viewed[$0] == nil || viewed[$0]!.isEmpty }
            if order.count > 500 {
                for key in order.prefix(order.count - 500) { viewed[key] = nil }
                order.removeFirst(order.count - 500)
            }
            UserDefaults.standard.set(order, forKey: "viewedOrder")
            UserDefaults.standard.set(viewed.filter { !$0.value.isEmpty }.mapValues(Array.init), forKey: "viewedFiles")
        }
    }
    /// 全ファイルを並べるのは、変更ファイルがこの数までのコミット（多ければ 1 ファイルずつ）。
    nonisolated private static let stackLimit = 200
    var onDiffShown: (() -> Void)?
    /// 親コミットのリンクを押したとき（40 桁の ID）。
    var onSelectCommit: ((String) -> Void)?
    /// 作者の名前を押したとき（History を検索する問い合わせ）と、ref の名前を押したとき。
    var onSearch: ((String) -> Void)?
    var onSelectRef: ((String) -> Void)?
    /// 署名をどこで検証しているか（吹き出しに出す。RepositoryWindow が決める）。
    var signaturePlace = SignaturePlace(local: true, detail: L("The repository is on this Mac"))
    private var signaturePopover: NSPopover?
    /// メッセージを書き直して保存したとき（コミットの ID と新しいメッセージ）。
    var onReword: ((String, String) -> Void)?
    /// 書き直している間だけ出す編集欄。
    private var rewordEditor: NSView?
    private var rewordText: NSTextView?
    private var headerContainer: NSView?
    private var headerScroll: NSScrollView?
    /// 標準・詳細のとき、上に作者とコミッターを並べる帯。
    private let peopleStrip = PeopleStrip(frame: .zero)

    /// 表示中のコミットの親と子（キーボードで移るため）。子は詳細を読み終えるまで空。shallow clone の境界の
    /// コミットの親は取得していないので、移れない（空）。
    var currentParents: [String] { info?.parentsNotFetched == true ? [] : info?.parents ?? [] }
    var currentChildren: [String] { extras?.children ?? [] }
    /// メッセージ・作者・ID・親コミット。
    private let header = FindableTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 170))
    private let repository: Repository
    private let files = NSTableView()
    private let diffView: DiffViewController
    /// 差分の中を探す（⌘F の検索の帯を開く。メニューバーを使わずに届くように）。
    private let findButton = NSButton()

    @objc private func findInDiff() { diffView.showFindBar() }
    private var changes: CommitChanges?
    private var request = 0
    private var headerRequest = 0
    /// 表示しているコミットの情報と、裏で読んだ詳細（見せ方を変えたときに描き直す）。
    private var info: CommitInfo?
    private var extras: CommitExtras? {
        didSet {
            // ファイルの一覧の行数（+/−）は詳細と一緒に読むので、届いたら一覧を描き直す。
            fileStats = Dictionary((extras?.stats ?? []).map { ($0.path, $0) }, uniquingKeysWith: { a, _ in a })
            // 選択を保つため、行を入れ替えずに描き直す。
            files.reloadData(forRowIndexes: IndexSet(integersIn: 0..<files.numberOfRows), columnIndexes: [0])
        }
    }
    private var fileStats: [String: CommitExtras.FileStat] = [:]
    /// 変更ファイルの一覧の行。上に並べるときはファイルだけ、左に木で出すときはフォルダの行も入る。
    private enum FileRow {
        case directory(String, depth: Int)
        case file(Int, depth: Int)
    }
    private var fileRows: [FileRow] = []
    /// ファイルの番号 → 一覧の行。
    private var rowOfFile: [Int] = []
    private var filesController: NSViewController?
    private var diffItem: NSSplitViewItem?
    private var sideSplit: NSSplitViewController?
    /// 署名を検証する側を変えた回数。変える前に始めた詳細の読み込みの結果は使わない。
    private var signatureGeneration = 0
    /// 詳細の欄だけを右の列いっぱいに広げる（変更ファイルと差分を隠す）。
    private let expandButton = NSButton()
    /// AI: メッセージの翻訳と、コミットの解説（AI の CLI を選んでいるときだけ出す）。
    private let translateButton = NSButton()
    private let explainButton = NSButton()
    /// History の検索の問い合わせ。件名と本文の中の一致を強調する。
    var searchQuery: String? {
        didSet { if searchQuery != oldValue { renderHeader() } }
    }
    private let styleControl = NSSegmentedControl(
        labels: CommitHeaderStyle.allCases.map(\.title), trackingMode: .selectOne, target: nil, action: nil)
    /// Push の確認に埋め込むとき: メタ情報は「標準」に固定して切り替えを出さず、広げる・翻訳・AI のボタンも出さない。
    /// `view` を作る前に設定する。
    var isPreview = false

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
        // 上から、メッセージとメタ情報、変更ファイル、差分。境目の位置は覚えておく。
        splitView.isVertical = false
        splitView.autosaveName = "CommitDetail"
        let column = NSTableColumn(identifier: .init("path"))
        column.resizingMask = .autoresizingMask
        files.addTableColumn(column)
        // パスの列を表の幅いっぱいにする（狭いと途中が省略される）。
        files.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        files.headerView = nil
        // 別の窓（Push の確認）では、sourceList の透ける背景が窓の後ろを映すので、ふつうの一覧にする。
        files.style = isPreview ? .inset : .sourceList
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
        // ⌘F で、詳細の欄の中（メッセージ）を探す（欄にフォーカスがあるとき）。
        header.usesFindBar = true
        header.isIncrementalSearchingEnabled = true
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
        self.headerContainer = headerContainer
        self.headerScroll = headerScroll
        peopleStrip.isHidden = true
        peopleStrip.onSelectPerson = { [weak self] name in
            self?.onSearch?("author:\"\(name.replacingOccurrences(of: "\"", with: ""))\"")
        }
        headerContainer.addSubview(peopleStrip)
        styleControl.controlSize = .mini
        styleControl.font = .systemFont(ofSize: NSFont.systemFontSize(for: .mini))
        styleControl.selectedSegment = isPreview ? CommitHeaderStyle.standard.rawValue : CommitHeaderStyle.saved.rawValue
        styleControl.isHidden = isPreview
        styleControl.target = self
        styleControl.action = #selector(styleChanged)
        styleControl.sizeToFit()
        styleControl.frame.origin = NSPoint(
            x: headerContainer.bounds.width - styleControl.frame.width - 16,
            y: headerContainer.bounds.height - styleControl.frame.height - 4)
        styleControl.autoresizingMask = [.minXMargin, .minYMargin]
        headerContainer.addSubview(styleControl)
        // 見せ方の切り替えの左に、詳細だけを広げるボタン。
        expandButton.bezelStyle = .accessoryBarAction
        expandButton.isBordered = false
        expandButton.image = NSImage(systemSymbolName: "arrow.up.left.and.arrow.down.right", accessibilityDescription: L("Expand Details"))
        expandButton.toolTip = L("Expand the commit details to fill the right column (click again to restore).")
        expandButton.target = self
        expandButton.action = #selector(toggleExpanded)
        expandButton.controlSize = .small
        expandButton.sizeToFit()
        expandButton.frame.origin = NSPoint(
            x: styleControl.frame.minX - expandButton.frame.width - 4,
            y: styleControl.frame.midY - expandButton.frame.height / 2)
        expandButton.autoresizingMask = [.minXMargin, .minYMargin]
        expandButton.isHidden = isPreview
        headerContainer.addSubview(expandButton)
        var right = expandButton.frame.minX
        for (button, symbol, tip, action) in [
            (findButton, "magnifyingglass", L("Find in the diff (⌘F)"), #selector(findInDiff)),
            (explainButton, "sparkles", L("AI: Explain this commit (or the combined diff of the selected commits)."), #selector(explainCommit)),
            (translateButton, "character.bubble", L("AI: Translate the message."), #selector(translateMessage)),
        ] {
            button.bezelStyle = .accessoryBarAction
            button.isBordered = false
            button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: tip)
            button.toolTip = tip
            button.target = self
            button.action = action
            button.controlSize = .small
            button.sizeToFit()
            button.frame.origin = NSPoint(x: right - button.frame.width - 2, y: expandButton.frame.minY)
            button.autoresizingMask = [.minXMargin, .minYMargin]
            right = button.frame.minX
            headerContainer.addSubview(button)
        }
        updateAssistantButtons()
        NotificationCenter.default.addObserver(
            self, selector: #selector(updateAssistantButtons), name: .assistantChanged, object: nil)
        if CommitHeaderStyle.saved == .compact, !isPreview {
            // 簡潔で始めるなら、最初から低くしておく。
            headerContainer.frame.size.height = 48
        }
        // ウィンドウの高さを変えたとき、メッセージと変更ファイルの高さは保ち、差分の側を伸び縮みさせる。
        let headerController = NSViewController()
        headerController.view = headerContainer
        let headerItem = NSSplitViewItem(viewController: headerController)
        headerItem.minimumThickness = 40
        headerItem.holdingPriority = .init(260)
        addSplitViewItem(headerItem)
        let filesController = NSViewController()
        filesController.view = scroll
        self.filesController = filesController
        diffView.onToggleFile = { [weak self] file in self?.toggleCollapsed(file) }
        diffView.fileMenu = { [weak self] file in self?.menuForFile(file) }
        diffView.onToggleViewed = { [weak self] file in self?.toggleViewed(file) }
        diffView.onExplainHunk = { [weak self] path, hunk in self?.explainHunk(path: path, hunk: hunk) }
        diffView.onTopFileChange = { [weak self] file in
            guard let self, stacked != nil, file < rowOfFile.count else { return }
            let row = rowOfFile[file]
            guard files.selectedRow != row else { return }
            syncingSelection = true
            files.selectRowIndexes([row], byExtendingSelection: false)
            files.scrollRowToVisible(row)
            syncingSelection = false
        }
        arrangeFiles()
        NotificationCenter.default.addObserver(
            self, selector: #selector(fileTreeChanged), name: .historyFileTreeChanged, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(messageStyleChanged), name: .commitMessageStyleChanged, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(stackedChanged), name: .stackedCommitDiffChanged, object: nil)
        // アバターを読み終えたら、表示中のコミットの人なら描き直す。
        Avatars.shared.onLoad.append { [weak self] email in
            guard let self, let info,
                  [info.authorEmail, info.committerEmail].contains(where: { $0.lowercased() == email }) else { return }
            peopleStrip.updateAvatars()
            renderHeader()
        }
    }

    /// 表示中のコミット（全ファイルを並べるかを変えたときに読み直す）。
    private var shownCommit: (id: Data, stash: Bool)?

    @objc private func stackedChanged() {
        guard let shownCommit else { return }
        show(commit: shownCommit.id, stash: shownCommit.stash)
    }

    @objc private func messageStyleChanged() {
        peopleStrip.updateAvatars()
        renderHeader()
    }

    /// ヘッダーの右クリック: ID・件名・メッセージ・git show のコピーを先頭に足す。
    func textView(_ view: NSTextView, menu: NSMenu, for event: NSEvent, at charIndex: Int) -> NSMenu? {
        guard view === header, let info else { return menu }
        let message = info.message.trimmingCharacters(in: .whitespacesAndNewlines)
        let subject = message.components(separatedBy: "\n").first ?? message
        let items: [(String, String)] = [
            (Titles.copyCommitID, info.oid),
            (Titles.copyShortID, String(info.oid.prefix(Titles.shortIDLength))),
            (L("Copy Subject"), subject),
            (L("Copy Message"), message),
            (L("Copy git show Command"), "git show \(info.oid)"),
        ]
        for (i, (title, value)) in items.enumerated() {
            menu.insertItem(ClosureMenuItem(title: title) {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(value, forType: .string)
            }, at: i)
        }
        menu.insertItem(.separator(), at: items.count)
        if extras?.headDistance != nil, info.parents.count <= 1 {
            menu.insertItem(ClosureMenuItem(title: L("Edit Message…")) { [weak self] in self?.beginReword() }, at: items.count + 1)
            menu.insertItem(.separator(), at: items.count + 2)
        }
        return menu
    }

    // MARK: - 署名の詳細

    /// 署名の行の、押した文字のところに詳細の吹き出しを出す。
    private func showSignaturePopover(at charIndex: Int) {
        guard let info else { return }
        let style = CommitHeaderStyle(rawValue: styleControl.selectedSegment) ?? .standard
        guard let status = CommitHeaderRenderer(info: info, extras: extras, style: style).signatureStatus else { return }
        showSignaturePopover(SignaturePopover(
            status: status, signature: extras?.signature, kind: info.signed ?? "gpg",
            committerEmail: info.recordedCommitterEmail, place: signaturePlace), at: charIndex)
    }

    /// タグの署名の印の吹き出し。`pointing` はこのコミットを指すタグの、`merged` は merge したタグの何番目か。
    private func showTagSignaturePopover(at charIndex: Int, pointing: Int? = nil, merged: Int? = nil) {
        guard let info, let extras else { return }
        let tag: (name: String, signed: String?, email: String, message: String, signature: CommitExtras.Signature?)
        if let pointing, extras.tagObjects.indices.contains(pointing) {
            let t = extras.tagObjects[pointing]
            tag = (t.name, t.signed, t.taggerEmail ?? "", t.message ?? "", t.signature)
        } else if let merged, info.mergedTags.indices.contains(merged), extras.mergedTagSignatures.indices.contains(merged) {
            let t = info.mergedTags[merged]
            tag = (t.name, t.signed, t.taggerEmail, t.message, extras.mergedTagSignatures[merged])
        } else {
            return
        }
        guard let status = CommitHeaderRenderer.tagSignatureStatus(
            signed: tag.signed, signature: tag.signature, loaded: true, taggerEmail: tag.email) else { return }
        showSignaturePopover(SignaturePopover(
            status: status, signature: tag.signature, kind: tag.signed ?? "gpg", committerEmail: tag.email,
            place: signaturePlace, tag: .init(name: tag.name, message: tag.message)), at: charIndex)
    }

    /// 署名の吹き出しを、押した文字のところに出す。
    private func showSignaturePopover(_ content: SignaturePopover, at charIndex: Int) {
        guard let layout = header.layoutManager, let container = header.textContainer else { return }
        let glyphs = layout.glyphRange(forCharacterRange: NSRange(location: charIndex, length: 1), actualCharacterRange: nil)
        var rect = layout.boundingRect(forGlyphRange: glyphs, in: container)
        rect.origin.x += header.textContainerOrigin.x
        rect.origin.y += header.textContainerOrigin.y
        let popover = NSPopover()
        popover.behavior = .transient
        popover.contentViewController = content
        popover.show(relativeTo: rect, of: header, preferredEdge: .maxY)
        signaturePopover = popover
    }

    // MARK: - その場で書き直す

    /// メッセージの欄を編集欄に替える。保存（⌘↩）で書き直し、キャンセル（esc）で戻す。
    func beginReword() {
        guard rewordEditor == nil, let info, let container = headerContainer else { return }
        // 低い（簡潔の）ときは、書き直す間だけ広げる。
        if container.frame.height < 160 { setHeaderHeight(220) }
        let editor = NSView(frame: container.bounds)
        editor.autoresizingMask = [.width, .height]
        editor.wantsLayer = true
        editor.layer?.backgroundColor = NSColor.textBackgroundColor.cgColor
        let barHeight: CGFloat = 30
        let scroll = NSScrollView(frame: NSRect(
            x: 0, y: barHeight, width: editor.bounds.width, height: editor.bounds.height - barHeight))
        scroll.autoresizingMask = [.width, .height]
        scroll.hasVerticalScroller = true
        let text = NSTextView(frame: scroll.bounds)
        text.isRichText = false
        text.allowsUndo = true
        text.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        text.textContainerInset = NSSize(width: 6, height: 6)
        for flag in [\NSTextView.isAutomaticQuoteSubstitutionEnabled, \.isAutomaticDashSubstitutionEnabled,
                     \.isAutomaticTextReplacementEnabled, \.isAutomaticSpellingCorrectionEnabled] {
            text[keyPath: flag] = false
        }
        text.autoresizingMask = [.width]
        text.string = info.message.trimmingCharacters(in: .whitespacesAndNewlines)
        scroll.documentView = text
        let save = NSButton(title: L("Save"), target: self, action: #selector(saveReword))
        save.keyEquivalent = "\r"
        save.keyEquivalentModifierMask = .command
        save.bezelColor = .controlAccentColor
        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(cancelReword))
        cancel.keyEquivalent = "\u{1b}"
        let hint = NSTextField(labelWithString: L("⌘↩ to save, esc to cancel. You can Undo it."))
        hint.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        hint.textColor = .secondaryLabelColor
        // 下の帯: 左に説明、右端に「キャンセル」「保存」。
        var right = editor.bounds.width - 8
        for b in [save, cancel] {
            b.controlSize = .small
            b.sizeToFit()
            right -= b.frame.width
            b.frame.origin = NSPoint(x: right, y: (barHeight - b.frame.height) / 2)
            b.autoresizingMask = [.minXMargin]
            editor.addSubview(b)
            right -= 6
        }
        hint.sizeToFit()
        hint.frame = NSRect(x: 10, y: (barHeight - hint.frame.height) / 2, width: max(0, right - 16), height: hint.frame.height)
        hint.autoresizingMask = [.width]
        hint.lineBreakMode = .byTruncatingTail
        editor.addSubview(hint)
        editor.addSubview(scroll)
        container.addSubview(editor)
        rewordEditor = editor
        rewordText = text
        view.window?.makeFirstResponder(text)
    }

    @objc private func saveReword() {
        guard let info, let text = rewordText else { return }
        let message = text.string.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !message.isEmpty else { return NSSound.beep() }
        guard message != info.message.trimmingCharacters(in: .whitespacesAndNewlines) else { return endReword() }
        let oid = info.oid
        guard let remotes = extras?.remoteBranches, !remotes.isEmpty, let window = view.window else {
            endReword()
            onReword?(oid, message)
            return
        }
        let alert = NSAlert()
        alert.messageText = L("Edit a pushed commit?")
        alert.informativeText = L("It is in %@, so pushing afterwards needs Force Push.", remotes.joined(separator: ", "))
        alert.addButton(withTitle: L("Edit Message"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { [weak self] response in
            guard response == .alertFirstButtonReturn else { return }
            self?.endReword()
            self?.onReword?(oid, message)
        }
    }

    @objc private func cancelReword() { endReword() }

    private func endReword() {
        rewordEditor?.removeFromSuperview()
        rewordEditor = nil
        rewordText = nil
    }

    func overlays() -> [(CGImage, NSView)] {
        view.window == nil ? [] : diffView.overlay().map { [$0] } ?? []
    }

    /// `commit` の変更を裏で読んで表示する。`stash` なら stash の中身（untracked のファイルを含む）。
    /// 差分を検索する（`--find` の確認用）。
    func find(_ query: String) { diffView.find(query) }

    func show(commit: Data?, stash: Bool = false) {
        // ほかのコミットを選んだら、書き直しはやめる。
        endReword()
        request += 1
        let request = self.request
        guard let commit else {
            changes = nil
            stacked = nil
            shownCommit = nil
            reloadFiles()
            diffView.show(nil, message: L("Select a commit to see its changes."))
            info = nil
            extras = nil
            header.string = ""
            return
        }
        shownCommit = (commit, stash)
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
        let stack = Settings.stackedCommitDiff
        let changesTask = Task.detached(priority: .userInitiated) {
            () -> (CommitChanges?, FileDiff?, StackedDiff?, CFTimeInterval, CFTimeInterval) in
            let t = CACurrentMediaTime()
            let changes = try? stash ? repository.stashChanges(of: commit) : repository.changes(of: commit)
            // 全ファイルを並べるなら全ファイルの差分を、そうでなければ最初のファイルの差分を読む。
            if stack, let changes, (1...Self.stackLimit).contains(changes.items.count),
               let stacked = try? changes.stacked() {
                return (changes, nil, stacked, t, CACurrentMediaTime())
            }
            let first = changes.flatMap { $0.items.isEmpty ? nil : try? $0.diff(at: 0) }
            return (changes, first, nil, t, CACurrentMediaTime())
        }
        // 署名の検証や含むタグなど時間のかかる分は、ヘッダーを出した後に足す（stash では読まない）。
        let signatureGeneration = self.signatureGeneration
        let extrasTask = stash ? nil : Task.detached(priority: .utility) { try? repository.commitExtras(oid: hex) }
        Task {
            guard let info = try? await infoTask.value, headerRequest == self.headerRequest else { return }
            FrameProfile.add("select: commit info (JSON)", CACurrentMediaTime() - selectedAt)
            self.info = info
            extras = nil
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
            let (changes, firstDiff, stacked, callStart, callEnd) = await changesTask.value
            guard request == self.request else { return }
            let resumed = CACurrentMediaTime()
            FrameProfile.add("select: changed files: start delay", callStart - selectedAt)
            FrameProfile.add("select: changed files: in call", callEnd - callStart)
            FrameProfile.add("select: changed files: main resume delay", resumed - callEnd)
            self.changes = changes
            self.stacked = stacked
            reloadFiles()
            if changes?.items.isEmpty ?? true {
                diffView.show(nil, message: L("No changes"))
                onDiffShown?()
            } else if let stacked {
                showStacked(stacked, key: hex)
                FrameProfile.add("select: → diff shown", CACurrentMediaTime() - selectedAt)
                PerformanceLog.record("History: select → diff", ms: (CACurrentMediaTime() - selectedAt) * 1000)
                onDiffShown?()
            } else {
                // 最初のファイルの差分は読んであるので、選んだときに読み直さない。
                preloadedDiff = firstDiff
                if let row = rowOfFile.first { files.selectRowIndexes([row], byExtendingSelection: false) }
            }
        }
    }

    // MARK: - 複数のコミット

    /// History で複数のコミットを選んだ: まとめた差分（`History.selectionDiff`）を全ファイル並べて見せる。
    /// 上の欄には、選んだコミットの一覧と、差分の取り方を出す。
    /// 表示中の複数選択（2 つのときの取り方を変えて読み直すため）。
    private var shownSelection: (rows: [Int], history: History, pair: PairMode)?
    private static let pairScheme = "amonhen-pair:"

    func show(selection rows: [Int], in history: History, pair: PairMode = .forward) {
        endReword()
        shownSelection = (rows, history, pair)
        request += 1
        headerRequest += 1
        let request = self.request
        shownCommit = nil
        commitHex = nil
        info = nil
        extras = nil
        changes = nil
        stacked = nil
        reloadFiles()
        renderSelectionHeader(rows: rows, history: history, kind: nil)
        diffView.show(nil, message: L("Loading the diff…"))
        let started = CACurrentMediaTime()
        Task {
            let result = await Task.detached(priority: .userInitiated) { try? history.selectionDiff(rows: rows, pair: pair) }.value
            guard request == self.request else { return }
            guard let (kind, stacked) = result else {
                diffView.show(nil, message: L("Couldn’t load the diff"))
                return
            }
            NSLog("選んだ %d コミットの差分を %.0f ms で読んだ", rows.count, (CACurrentMediaTime() - started) * 1000)
            self.stacked = stacked
            reloadFiles()
            renderSelectionHeader(rows: rows, history: history, kind: kind, pair: pair)
            if stacked.count == 0 {
                diffView.show(nil, message: L("No changes"))
                return
            }
            let ids = rows.sorted().compactMap { history.commitID(row: $0) }
            showStacked(stacked, key: ids.map { $0.map { String(format: "%02x", $0) }.joined() }.joined(separator: ",") + "|\(pair.rawValue)")
        }
    }

    /// Push のシートの「送る変更すべて」: `base`..`tip`（上流から HEAD まで）の差分を、全ファイル縦に並べて出す。
    /// 見出しは `title` と、送るコミットの 1 行ずつ（`lines`）。
    func show(range base: Data, to tip: Data, title: String, lines: [String]) {
        endReword()
        shownSelection = nil
        request += 1
        headerRequest += 1
        let request = self.request
        shownCommit = nil
        commitHex = nil
        info = nil
        extras = nil
        changes = nil
        stacked = nil
        reloadFiles()
        renderRangeHeader(title: title, lines: lines, note: L("Loading the diff…"))
        diffView.show(nil, message: L("Loading the diff…"))
        let repository = self.repository
        Task {
            let result = await Task.detached(priority: .userInitiated) {
                try? repository.changes(from: base, to: tip).stacked()
            }.value
            guard request == self.request else { return }
            guard let stacked = result else {
                diffView.show(nil, message: L("Couldn’t load the diff"))
                return
            }
            self.stacked = stacked
            reloadFiles()
            renderRangeHeader(title: title, lines: lines, note: L("%ld files changed", stacked.count))
            if stacked.count == 0 {
                diffView.show(nil, message: L("No changes"))
                return
            }
            let hex = { (d: Data) in d.map { String(format: "%02x", $0) }.joined() }
            showStacked(stacked, key: hex(base) + ".." + hex(tip))
        }
    }

    private func renderRangeHeader(title: String, lines: [String], note: String) {
        let text = NSMutableAttributedString()
        let bold: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 13), .foregroundColor: NSColor.labelColor]
        let small: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor]
        let body: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.labelColor]
        text.append(NSAttributedString(string: title + "\n", attributes: bold))
        text.append(NSAttributedString(string: note + "\n\n", attributes: small))
        for line in lines { text.append(NSAttributedString(string: line + "\n", attributes: body)) }
        layoutPeopleStrip(visible: false)
        header.textStorage?.setAttributedString(text)
        scheduleHeaderFit()
    }

    /// 選んだコミットの数・差分の取り方・各コミットの 1 行。
    private func renderSelectionHeader(rows: [Int], history: History, kind: SelectionKind?, pair: PairMode = .forward) {
        let sorted = rows.sorted()
        let text = NSMutableAttributedString()
        let title: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 13), .foregroundColor: NSColor.labelColor]
        let note: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor]
        let mono: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular), .foregroundColor: NSColor.secondaryLabelColor,
        ]
        let body: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.labelColor]
        text.append(NSAttributedString(string: L("%ld commits selected", rows.count) + "\n", attributes: title))
        let explanation = switch kind {
        case .range?: L("They are consecutive, so this is the diff from the parent of the oldest to the newest (the same as squashing them into one).")
        case .pair?: switch pair {
            case .forward: L("Two non-consecutive commits, so this is the diff from the older to the newer (A..B).")
            case .reverse: L("The diff of the two non-consecutive commits from the newer to the older (reversed).")
            case .mergeBase: L("The diff of the two non-consecutive commits from their common ancestor to the newer (A...B: only the changes on the newer side).")
            }
        case .each?: L("They are not contiguous, so each commit’s diff is listed file by file (headed by the path and short ID).")
        case nil: L("Loading the diff…")
        }
        text.append(NSAttributedString(string: explanation + "\n", attributes: note))
        if kind == .pair {
            // 取り方の切り替え（押すと読み直す）。
            let options: [(PairMode, String)] = [(.forward, "A..B"), (.reverse, L("Reverse")), (.mergeBase, L("From the merge-base (A...B)"))]
            for (mode, title) in options where mode != pair {
                var link = note
                link[.link] = Self.pairScheme + "\(mode.rawValue)"
                link[.foregroundColor] = NSColor.linkColor
                text.append(NSAttributedString(string: title, attributes: link))
                text.append(NSAttributedString(string: "　", attributes: note))
            }
            text.append(NSAttributedString(string: "\n", attributes: note))
        }
        text.append(NSAttributedString(string: "\n", attributes: note))
        for row in sorted {
            guard let id = history.commitID(row: row) else { continue }
            let short = id.prefix(5).map { String(format: "%02x", $0) }.joined().prefix(8)
            let chunk = try? history.chunk(rows: row..<row + 1)
            text.append(NSAttributedString(string: "\(short)  ", attributes: mono))
            text.append(NSAttributedString(string: chunk?.subject(row: row) ?? "", attributes: body))
            text.append(NSAttributedString(string: "  — \(chunk?.authorName(row: row) ?? "")\n", attributes: note))
        }
        layoutPeopleStrip(visible: false)
        header.textStorage?.setAttributedString(text)
        scheduleHeaderFit()
    }

    // MARK: - 全ファイルを縦に並べた差分

    /// 最初から畳むファイル: lockfile などの生成物（見ても仕方がない大きな差分）。
    private static func isGenerated(_ path: String) -> Bool {
        let name = (path as NSString).lastPathComponent
        let lockfiles: Set<String> = [
            "Cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "Gemfile.lock", "poetry.lock",
            "composer.lock", "go.sum", "Package.resolved", "Podfile.lock", "bun.lockb", "uv.lock",
        ]
        return lockfiles.contains(name) || name.hasSuffix(".min.js") || name.hasSuffix(".min.css")
    }

    /// 1 ファイルの変更がこの行数を超えたら、最初は畳む。
    private static let collapseAbove = 1000

    /// Viewed を覚える鍵（コミット ID。複数のコミットを選んでいるときは、その ID を並べたもの）。
    private var viewedKey: String?

    private func showStacked(_ stacked: StackedDiff, key: String) {
        viewedKey = key
        let viewed = Self.viewed[key] ?? []
        collapsed = stacked.names.map { Self.isGenerated($0) || viewed.contains($0) }
        var view = stacked.view(collapsed: collapsed)
        // 行数は差分を取って初めてわかるので、大きなファイルがあれば畳んで作り直す。
        let large: [Int] = view.withRows { all in
            var file = -1
            var out: [Int] = []
            for r in all where r.kind == UInt8(AmonhenDiffRow_FILE) {
                file += 1
                if Int(r.old_no + r.new_no) > Self.collapseAbove { out.append(file) }
            }
            return out
        }
        if large.contains(where: { !collapsed[$0] }) {
            for i in large { collapsed[i] = true }
            view = stacked.view(collapsed: collapsed)
        }
        diffView.viewedFiles = Set(stacked.names.indices.filter { viewed.contains(stacked.names[$0]) })
        FrameProfile.measure("select: diff draw") { diffView.show(view) }
        syncingSelection = true
        if let row = rowOfFile.first { files.selectRowIndexes([row], byExtendingSelection: false) }
        syncingSelection = false
    }

    /// 畳むファイルを変えたので、表示を作り直す（スクロールの位置は保つ）。畳んだファイルの見出しが上に隠れて
    /// いたら、見出しが見えるところまで戻す。
    private func reshowStacked(keeping file: Int) {
        guard let stacked else { return }
        diffView.show(stacked.view(collapsed: collapsed), keepScroll: true)
        diffView.revealFileHeader(file)
    }

    /// ファイルの見出しの右クリック。とびとびに選んだコミットの見出し（「パス — 短い ID」）からはパスを取り出す。
    private func menuForFile(_ file: Int) -> NSMenu? {
        guard let stacked, file < stacked.names.count else { return nil }
        let name = stacked.names[file]
        let path = name.components(separatedBy: " — ").first ?? name
        let menu = NSMenu()
        let collapsedNow = file < collapsed.count && collapsed[file]
        menu.addItem(ClosureMenuItem(title: collapsedNow ? L("Expand") : L("Collapse")) { [weak self] in self?.toggleCollapsed(file) })
        menu.addItem(ClosureMenuItem(title: L("Toggle Viewed")) { [weak self] in self?.toggleViewed(file) })
        menu.addItem(.separator())
        if let commit = commitHex {
            menu.addItem(ClosureMenuItem(title: Titles.blame(at: String(commit.prefix(Titles.shortIDLength)))) { [weak self] in self?.onBlame?(path, commit) })
        }
        menu.addItem(ClosureMenuItem(title: Titles.fileHistory) { [weak self] in self?.onFileHistory?(path) })
        menu.addItem(.separator())
        menu.addItem(ClosureMenuItem(title: L("Copy Path")) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(path, forType: .string)
        })
        if let workdir = repository.workdir {
            let full = (workdir as NSString).appendingPathComponent(path)
            if FileManager.default.fileExists(atPath: full) {
                menu.addItem(ClosureMenuItem(title: L("Show in Finder")) {
                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: full)])
                })
                menu.addItem(ClosureMenuItem(title: L("Open with Default App")) { NSWorkspace.shared.open(URL(fileURLWithPath: full)) })
            }
        }
        return menu
    }

    private func toggleCollapsed(_ file: Int) {
        guard file < collapsed.count else { return }
        collapsed[file].toggle()
        reshowStacked(keeping: file)
    }

    /// 見たことにしたら畳み、戻したら開く（GitHub と同じ）。
    private func toggleViewed(_ file: Int) {
        guard let stacked, let commit = viewedKey, file < stacked.names.count else { return }
        let path = stacked.names[file]
        var set = Self.viewed[commit] ?? []
        let nowViewed = set.insert(path).inserted
        if !nowViewed { set.remove(path) }
        Self.viewed[commit] = set
        collapsed[file] = nowViewed
        diffView.viewedFiles = Set(stacked.names.indices.filter { set.contains(stacked.names[$0]) })
        reshowStacked(keeping: file)
    }

    private func renderHeader() {
        guard let info else { return }
        let style = CommitHeaderStyle(rawValue: styleControl.selectedSegment) ?? .standard
        var renderer = CommitHeaderRenderer(info: info, extras: extras, style: style)
        renderer.monospacedBody = Settings.monospacedCommitMessage
        renderer.highlight = CommitHeaderRenderer.highlightTerms(of: searchQuery)
        let text = NSMutableAttributedString(attributedString: renderer.render())
        let showPeople = style != .compact
        // 帯の高さは人によるので、人を入れてから並べる。
        if showPeople { peopleStrip.show(info, date: CommitHeaderRenderer.date) }
        layoutPeopleStrip(visible: showPeople)
        if !showPeople {
            // 右上の切り替えに文字が重ならないよう、最初の行の右端を空ける。
            let first = NSMutableParagraphStyle()
            first.tailIndent = -(styleControl.frame.width + 12)
            let firstLine = (text.string as NSString).lineRange(for: NSRange(location: 0, length: 0))
            text.addAttribute(.paragraphStyle, value: first, range: firstLine)
        }
        header.textStorage?.setAttributedString(text)
        scheduleHeaderFit()
    }

    private var headerFitPending = false

    /// 欄の高さ合わせは右の列全体を並べ直すので（linux で 5 ms ほど）、選んでから表示するまでの間ではなく、
    /// 表示を終えた後にまとめて 1 回だけ行う（キーを押し続けて次々に選ぶ間は、最後のものだけ）。
    private func scheduleHeaderFit() {
        guard !headerFitPending else { return }
        headerFitPending = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
            guard let self else { return }
            headerFitPending = false
            fitHeaderToContent()
        }
    }

    /// ref が変わったので、表示中のコミットの情報（ref の一覧など）を読み直す。
    func reloadInfo() {
        guard let hex = info?.oid else { return }
        let request = headerRequest, repository = self.repository
        Task {
            guard let info = try? await Task.detached(priority: .utility, operation: { try repository.commitInfo(oid: hex) }).value,
                  request == headerRequest, info.oid == self.info?.oid,
                  // ref が動いても、このコミットに付く ref が同じなら描き直さない（描き直すと右の列を並べ直し、ちらつく）。
                  info != self.info else { return }
            self.info = info
            renderHeader()
        }
    }

    /// 検証する側が変わったので、表示中のコミットの署名を検証し直す（前の結果は使わない）。コミットの情報を
    /// まだ読んでいる途中なら、読み終えたときに番号の違いに気づいて読み直す。
    func reloadSignature() {
        signatureGeneration += 1
        guard let info else { return }
        if info.signed != nil || info.mergedTags.contains(where: { $0.signed != nil })
            || extras?.tagObjects.contains(where: { $0.signed != nil }) == true {
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

    /// 作者とコミッターの帯を上に置き（右上の切り替えの手前まで）、文字の欄をその下にする。
    private func layoutPeopleStrip(visible: Bool) {
        guard let container = headerContainer, let scroll = headerScroll else { return }
        peopleStrip.isHidden = !visible
        let height = visible ? peopleStrip.preferredHeight : 0
        let bounds = container.bounds
        peopleStrip.frame = NSRect(
            x: 0, y: bounds.height - height, width: max(0, styleControl.frame.minX - 8), height: height)
        peopleStrip.autoresizingMask = [.width, .minYMargin]
        scroll.frame = NSRect(x: 0, y: 0, width: bounds.width, height: bounds.height - height)
    }

    // MARK: - AI

    @objc private func updateAssistantButtons() {
        translateButton.isHidden = !Assistant.isConfigured || isPreview
        explainButton.isHidden = !Assistant.isConfigured || isPreview
    }

    /// メッセージ（複数のコミットを選んでいれば、上の欄の一覧）。
    private var shownMessage: String { info?.message ?? header.string }

    @objc private func translateMessage() {
        let message = shownMessage.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !message.isEmpty, Assistant.confirm(repository: repository.workdir ?? "remote", summary: L("The message (%ld characters)", message.count))
        else { return }
        let workdir = repository.workdir
        AssistantPanel.show(title: L("Translation"), over: view.window) { useCache, onText in
            try await Assistant.run(.translate, variables: ["message": message], workdir: workdir, useCache: useCache, onText: onText)
        }
    }

    /// 差分の右クリック > AI でこの hunk を解説。
    private func explainHunk(path: String, hunk: String) {
        guard Assistant.confirm(repository: repository.workdir ?? "remote", summary: L("One hunk of %@", path)) else { return }
        let message = shownMessage, workdir = repository.workdir
        AssistantPanel.show(title: L("Hunk Explanation"), over: view.window) { useCache, onText in
            try await Assistant.run(
                .explainHunk, variables: ["path": path, "hunk": hunk, "message": message], workdir: workdir,
                useCache: useCache, onText: onText)
        }
    }

    @objc private func explainCommit() {
        let message = shownMessage
        let stacked = self.stacked, changes = self.changes
        let files = stacked?.names ?? changes?.items.map(\.path) ?? []
        guard !files.isEmpty || !message.isEmpty,
              Assistant.confirm(repository: repository.workdir ?? "remote", summary: L("The message and the diff of %ld files", files.count))
        else { return }
        let workdir = repository.workdir
        AssistantPanel.show(title: L("Explanation"), over: view.window) { useCache, onText in
            // 差分の文字は裏で作る（大きければ切る）。
            let diff = await Task.detached(priority: .userInitiated) { () -> String in
                if let stacked { return DiffText.make(stacked.view(collapsed: [])) }
                guard let changes else { return "" }
                var out = ""
                for i in changes.items.indices where out.utf8.count < DiffText.limit {
                    if let d = try? changes.diff(at: i) { out += DiffText.make(d, path: changes.items[i].path) }
                }
                return out
            }.value
            return try await Assistant.run(
                .explainCommit, variables: ["message": message, "diff": diff, "files": files.joined(separator: "\n")],
                workdir: workdir, useCache: useCache, onText: onText)
        }
    }

    /// 詳細の欄を広げているか（変更ファイルと差分を畳んでいる）。
    private var isExpanded: Bool { splitViewItems.count > 1 && splitViewItems[1].isCollapsed }

    @objc private func toggleExpanded() {
        guard splitViewItems.count > 1 else { return }
        let expand = !isExpanded
        for item in splitViewItems.dropFirst() { item.isCollapsed = expand }
        expandButton.image = NSImage(
            systemSymbolName: expand ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right",
            accessibilityDescription: expand ? L("Restore Details") : L("Expand Details"))
    }

    @objc private func styleChanged() {
        let style = CommitHeaderStyle(rawValue: styleControl.selectedSegment) ?? .standard
        CommitHeaderStyle.saved = style
        // 広げたままだと高さを合わせられないので、先に戻す。
        if isExpanded { toggleExpanded() }
        renderHeader()
    }

    /// 詳細の欄の高さをメッセージに合わせる（短ければ縮め、長ければ右の列の半分まで伸ばす。続きは欄の中で
    /// スクロールするか、⤢ で広げる）。コミットを選ぶたび・見せ方を変えるたびに合わせる。
    private func fitHeaderToContent() {
        guard splitViewItems.count > 1, !isExpanded, rewordEditor == nil,
              let layout = header.layoutManager, let container = header.textContainer else { return }
        layout.ensureLayout(for: container)
        let strip = peopleStrip.isHidden ? 0 : peopleStrip.preferredHeight
        let content = ceil(strip + layout.usedRect(for: container).height + header.textContainerInset.height * 2 + 4)
        let cap = max(splitViewItems[0].minimumThickness, floor(splitView.bounds.height * 0.5))
        let target = max(splitViewItems[0].minimumThickness, min(cap, content))
        guard abs(splitViewItems[0].viewController.view.frame.height - target) > 1 else { return }
        setHeaderHeight(target)
    }

    /// 詳細の欄の高さを変える。仕切りは一度に隣の仕切りの今の位置までしか動かない（変更したファイルの欄が下に
    /// 並ぶときは、その高さは保たれ、差分の欄が縮む）ので、1 回では伸ばせるのがファイルの欄の高さまでになる。
    /// 届くか動かなくなるまで繰り返す。
    private func setHeaderHeight(_ height: CGFloat) {
        var last = splitViewItems[0].viewController.view.frame.height
        for _ in 0..<8 {
            splitView.setPosition(height, ofDividerAt: 0)
            let now = splitViewItems[0].viewController.view.frame.height
            if abs(now - height) <= 1 || abs(now - last) < 1 { break }
            last = now
        }
    }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        guard let s = (link as? String) ?? (link as? URL)?.absoluteString else { return false }
        if s.hasPrefix(Self.pairScheme), let selection = shownSelection,
           let raw = UInt8(s.dropFirst(Self.pairScheme.count)), let mode = PairMode(rawValue: raw) {
            show(selection: selection.rows, in: selection.history, pair: mode)
            return true
        }
        if s.hasPrefix(CommitHeaderRenderer.authorScheme) {
            let name = String(s.dropFirst(CommitHeaderRenderer.authorScheme.count)).replacingOccurrences(of: "\"", with: "")
            onSearch?("author:\"\(name)\"")
            return true
        }
        if s.hasPrefix(CommitHeaderRenderer.refScheme) {
            onSelectRef?(String(s.dropFirst(CommitHeaderRenderer.refScheme.count)))
            return true
        }
        if s == CommitHeaderRenderer.signatureLink {
            showSignaturePopover(at: charIndex)
            return true
        }
        if s.hasPrefix(CommitHeaderRenderer.tagSignatureLink),
           let i = Int(s.dropFirst(CommitHeaderRenderer.tagSignatureLink.count)) {
            showTagSignaturePopover(at: charIndex, pointing: i)
            return true
        }
        if s.hasPrefix(CommitHeaderRenderer.mergedTagSignatureLink),
           let i = Int(s.dropFirst(CommitHeaderRenderer.mergedTagSignatureLink.count)) {
            showTagSignaturePopover(at: charIndex, merged: i)
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

    func numberOfRows(in tableView: NSTableView) -> Int { fileRows.count }

    /// 一覧の行のファイルの番号（フォルダの行なら nil）。
    private func fileIndex(row: Int) -> Int? {
        guard row >= 0, row < fileRows.count, case let .file(index, _) = fileRows[row] else { return nil }
        return index
    }

    /// ファイルの一覧を作り直す。木で出すときは、フォルダごとにまとめ（子が 1 つだけのフォルダはつなげる）、
    /// 各階層でフォルダを先に、名前の順に並べる。
    private func reloadFiles() {
        // 複数のコミットを選んでいるときは、全ファイルの差分の見出しを並べる。
        let names = changes?.items.map(\.path) ?? stacked?.names ?? []
        if Settings.historyFileTree {
            final class Folder {
                var folders: [String: Folder] = [:]
                var files: [(String, Int)] = []
            }
            let root = Folder()
            for (index, name) in names.enumerated() {
                var parts = name.split(separator: "/").map(String.init)
                let leaf = parts.popLast() ?? name
                var folder = root
                for part in parts {
                    let next = folder.folders[part] ?? Folder()
                    folder.folders[part] = next
                    folder = next
                }
                folder.files.append((leaf, index))
            }
            var rows: [FileRow] = []
            func add(_ folder: Folder, depth: Int) {
                for name in folder.folders.keys.sorted(by: { $0.localizedStandardCompare($1) == .orderedAscending }) {
                    var label = name
                    var child = folder.folders[name]!
                    while child.files.isEmpty, child.folders.count == 1, let (next, grand) = child.folders.first {
                        label += "/" + next
                        child = grand
                    }
                    rows.append(.directory(label, depth: depth))
                    add(child, depth: depth + 1)
                }
                for (_, index) in folder.files.sorted(by: { $0.0.localizedStandardCompare($1.0) == .orderedAscending }) {
                    rows.append(.file(index, depth: depth))
                }
            }
            add(root, depth: 0)
            fileRows = rows
        } else {
            fileRows = names.indices.map { .file($0, depth: 0) }
        }
        rowOfFile = Array(repeating: 0, count: names.count)
        for (row, item) in fileRows.enumerated() {
            if case let .file(index, _) = item { rowOfFile[index] = row }
        }
        files.reloadData()
    }

    /// 変更ファイルの一覧を、メッセージと差分の間（上に一覧）か、差分の左（木）に置く。
    private func arrangeFiles() {
        guard let filesController else { return }
        // 今の置き方を外す（ビューコントローラは 1 か所にしか入れられない）。
        if let diffItem { removeSplitViewItem(diffItem) }
        if let sideSplit, let item = splitViewItems.first(where: { $0.viewController === sideSplit }) {
            for item in sideSplit.splitViewItems { sideSplit.removeSplitViewItem(item) }
            removeSplitViewItem(item)
        }
        if let item = splitViewItems.first(where: { $0.viewController === filesController }) { removeSplitViewItem(item) }
        sideSplit = nil
        let filesItem = NSSplitViewItem(viewController: filesController)
        filesItem.canCollapse = true
        let diffItem = NSSplitViewItem(viewController: diffView)
        diffItem.minimumThickness = 200
        diffItem.canCollapse = true
        self.diffItem = nil
        if Settings.historyFileTree {
            let side = NSSplitViewController()
            side.splitView.isVertical = true
            side.splitView.dividerStyle = .thin
            side.splitView.autosaveName = "CommitDetailFileTree"
            filesItem.minimumThickness = 140
            filesItem.holdingPriority = .init(260)
            // 最初の幅（境目を動かしたら、その位置を覚える）。
            filesController.view.frame.size.width = 260
            side.addSplitViewItem(filesItem)
            side.addSplitViewItem(diffItem)
            let item = NSSplitViewItem(viewController: side)
            item.minimumThickness = 200
            item.canCollapse = true
            addSplitViewItem(item)
            sideSplit = side
        } else {
            filesController.view.frame.size.height = 120
            filesItem.minimumThickness = 60
            filesItem.holdingPriority = .init(260)
            addSplitViewItem(filesItem)
            addSplitViewItem(diffItem)
            self.diffItem = diffItem
        }
    }

    @objc private func fileTreeChanged() {
        arrangeFiles()
        let selected = fileIndex(row: files.selectedRow)
        syncingSelection = true
        reloadFiles()
        if let selected { files.selectRowIndexes([rowOfFile[selected]], byExtendingSelection: false) }
        syncingSelection = false
    }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool { fileIndex(row: row) != nil }

    private func showFileDiff(_ diff: FileDiff?) {
        if let diff, diff.isBinary {
            diffView.show(nil, message: L("Binary file"))
        } else {
            diffView.show(diff)
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard let index = fileIndex(row: files.clickedRow), let item = changes?.items[index] else { return }
        let path = item.path
        if let commit = commitHex, item.kind != .deleted {
            menu.addItem(ClosureMenuItem(title: Titles.blame(at: String(commit.prefix(Titles.shortIDLength)))) { [weak self] in self?.onBlame?(path, commit) })
        }
        menu.addItem(ClosureMenuItem(title: Titles.fileHistory) { [weak self] in self?.onFileHistory?(path) })
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("file"), owner: nil) as? FileCell ?? FileCell()
        guard row < fileRows.count else { return nil }
        let tree = Settings.historyFileTree
        let index: Int
        switch fileRows[row] {
        case let .directory(name, depth):
            cell.indent = CGFloat(depth) * 14
            cell.path.attributedStringValue = FileCell.folderTitle(name)
            cell.path.toolTip = name
            cell.show(stat: nil)
            return cell
        case let .file(i, depth):
            cell.indent = CGFloat(depth) * 14
            index = i
        }
        // 木では、フォルダの下にファイルの名前だけを出す。
        let display = { (path: String) in tree ? String(path.split(separator: "/").last ?? Substring(path)) : path }
        guard let item = changes?.items[index] else {
            guard let name = stacked?.names[index] else { return nil }
            cell.path.stringValue = display(name)
            cell.path.toolTip = name
            cell.show(stat: nil)
            return cell
        }
        let field = cell.path
        let mark = switch item.kind {
        case .added: "A"
        case .deleted: "D"
        case .modified: "M"
        case .renamed: "R"
        case .copied: "C"
        }
        field.stringValue = "\(mark)  \(display(item.path))"
        field.toolTip = item.oldPath.map { "\($0) → \(item.path)" } ?? item.path
        cell.show(stat: fileStats[item.path])
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard let index = fileIndex(row: files.selectedRow) else { return }
        diffView.path = changes?.items[index].path
        // 全ファイルを並べているなら、そのファイルの見出しまでスクロールする。
        if stacked != nil {
            if !syncingSelection { diffView.scrollToFile(index) }
            return
        }
        guard let changes else { return }
        request += 1
        let request = self.request
        if index == 0, let diff = preloadedDiff {
            preloadedDiff = nil
            FrameProfile.measure("select: diff draw") { showFileDiff(diff) }
            FrameProfile.add("select: → diff shown", CACurrentMediaTime() - selectedAt)
                PerformanceLog.record("History: select → diff", ms: (CACurrentMediaTime() - selectedAt) * 1000)
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
                PerformanceLog.record("History: select → diff", ms: (CACurrentMediaTime() - selectedAt) * 1000)
            onDiffShown?()
        }
    }
}

/// 変更ファイルの一覧の行: 左に種類とパス、右に行数（+ 緑・− 赤、バイナリは「バイナリ」）。
final class FileCell: NSTableCellView {
    let path = NSTextField(labelWithString: "")
    private let stat = NSTextField(labelWithString: "")
    private var leading: NSLayoutConstraint!
    /// 木で出すときの字下げ。
    var indent: CGFloat = 0 {
        didSet { leading.constant = 2 + indent }
    }

    /// フォルダの行の文字（フォルダの印と名前、薄い色）。
    static func folderTitle(_ name: String) -> NSAttributedString {
        let text = NSMutableAttributedString()
        if let image = NSImage(systemSymbolName: "folder", accessibilityDescription: nil) {
            let attachment = NSTextAttachment()
            attachment.image = image
            attachment.bounds = NSRect(x: 0, y: -2, width: 14, height: 12)
            text.append(NSAttributedString(attachment: attachment))
            text.append(NSAttributedString(string: " "))
        }
        text.append(NSAttributedString(string: name))
        // 長いときは頭を省く（近い側のフォルダの名前を残す）。
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingHead
        let all = NSRange(location: 0, length: text.length)
        text.addAttribute(.foregroundColor, value: NSColor.secondaryLabelColor, range: all)
        text.addAttribute(.paragraphStyle, value: paragraph, range: all)
        return text
    }

    init() {
        super.init(frame: .zero)
        identifier = .init("file")
        path.lineBreakMode = .byTruncatingMiddle
        path.maximumNumberOfLines = 1
        stat.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        stat.alignment = .right
        for v in [path, stat] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        path.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        stat.setContentCompressionResistancePriority(.required, for: .horizontal)
        stat.setContentHuggingPriority(.required, for: .horizontal)
        leading = path.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2)
        NSLayoutConstraint.activate([
            leading,
            path.centerYAnchor.constraint(equalTo: centerYAnchor),
            stat.leadingAnchor.constraint(greaterThanOrEqualTo: path.trailingAnchor, constant: 8),
            stat.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            stat.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    func show(stat value: CommitExtras.FileStat?) {
        guard let value else {
            stat.stringValue = ""
            return
        }
        guard let added = value.added, let removed = value.removed else {
            stat.attributedStringValue = NSAttributedString(string: L("Binary"), attributes: [.foregroundColor: NSColor.secondaryLabelColor])
            return
        }
        let text = NSMutableAttributedString()
        if added > 0 { text.append(NSAttributedString(string: "+\(added)", attributes: [.foregroundColor: NSColor.systemGreen])) }
        if removed > 0 {
            if added > 0 { text.append(NSAttributedString(string: " ")) }
            text.append(NSAttributedString(string: "−\(removed)", attributes: [.foregroundColor: NSColor.systemRed]))
        }
        stat.attributedStringValue = text
    }
}

/// ⌘F で文字の欄の中を探す NSTextView（Edit > Find… は、フォーカスのある欄の `showFindBar:` を呼ぶ）。
final class FindableTextView: NSTextView {
    @objc func showFindBar(_ sender: Any?) { performTextFinderAction(findAction(.showFindInterface)) }
    // ⌘G・⌘⇧G: この欄の中を探しているときはここで。そうでなければ History の検索の一致へ。
    @objc func findNext(_ sender: Any?) {
        if enclosingScrollView?.isFindBarVisible == true || !moveHistoryMatch(from: window, by: 1) {
            performTextFinderAction(findAction(.nextMatch))
        }
    }
    @objc func findPrevious(_ sender: Any?) {
        if enclosingScrollView?.isFindBarVisible == true || !moveHistoryMatch(from: window, by: -1) {
            performTextFinderAction(findAction(.previousMatch))
        }
    }

    private func findAction(_ action: NSTextFinder.Action) -> NSMenuItem {
        let item = NSMenuItem()
        item.tag = action.rawValue
        return item
    }
}

/// History の検索の一致を次・前へ移せるもの（リポジトリのウィンドウ）。
protocol HistoryMatchNavigating: AnyObject {
    /// 移したら true（History で探していなければ false）。
    func moveHistoryMatch(by step: Int) -> Bool
}

/// `window` が History で探していれば、その一致を次・前へ移す。移したら true。
@MainActor
func moveHistoryMatch(from window: NSWindow?, by step: Int) -> Bool {
    (window?.delegate as? HistoryMatchNavigating)?.moveHistoryMatch(by: step) ?? false
}
