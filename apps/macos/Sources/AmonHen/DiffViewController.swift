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
    let renderer: TextRenderer
    private let scrollView = NSScrollView()
    private let document = DiffDocumentView()

    /// 差分の上に固定して置く帯（History の右の列の、変更／すべてのファイルの切り替えなど）。
    var topBar: NSView? {
        didSet {
            oldValue?.removeFromSuperview()
            if let topBar { view.addSubview(topBar) }
            layoutSubviews()
        }
    }
    private static let topBarHeight: CGFloat = 30

    private let textView = GraphView()
    private var diff: FileDiff? {
        didSet { hunkCursor = nil }
    }
    /// J・K で最後に出した hunk の番号（行を選べない差分で、次の J・K をそこから進めるため）。
    private var hunkCursor: Int?
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
    /// いちばん長い行の幅（横にスクロールできる範囲）。読んだときに文字数から見積もり、描いた行がそれより長ければ広げる。
    private var textWidth: CGFloat = 0
    private var gutterWidth: CGFloat { numbersWidth + (showsStageChecks ? checkWidth : 0) }
    /// Working Copy の HEAD と作業ツリーの差分か。変更の行と hunk の見出しの左に、次のコミットに含めるかの
    /// チェックボックスを出す。
    private var showsStageChecks: Bool { (diff as? WorkingFileDiff)?.isCombined == true }
    /// チェックボックスを押した・Space を押した: `rows` 行目の変更を含める（true）・外す。
    var onSetStaged: ((_ rows: [Int], _ staged: Bool) -> Void)?
    /// 全ファイルを並べた差分の、ファイルの見出しの行（`AmonhenDiffRow_FILE`）。i 番目がファイル i。
    private var fileRows: [Int] = []
    /// 見出しを押した: ファイルを畳む・開く。
    var onToggleFile: ((Int) -> Void)?
    /// 一番上に見えているファイルが変わった（変更ファイルの一覧の選択を合わせる）。
    var onTopFileChange: ((Int) -> Void)?
    private var topFile: Int?
    private var headerGlyphs: [String: TextRenderer.ShapedLine] = [:]
    private var checkGlyphs: [String: TextRenderer.ShapedLine] = [:]
    /// 上の並べた文字を作ったときの倍率。
    private var cachedScale: CGFloat?

    // History の右の列: 変更ファイルの一覧を兼ねる見出し、ツリー、画像の欄、その時点のファイル。
    /// ファイルごとの種類の印（A・M・D・R）と、行数の代わりに出す文字（画像の大きさなど）。
    var fileMarks: [Int: (String, NSColor)] = [:]
    var fileNotes: [Int: String] = [:]
    /// 見出しを開いた形（▾）で描くファイル（中身は差分の行ではなく、下に置いた欄）。
    var openOverride: Set<Int> = []
    /// hunk の見出しの左に、前後を広げる印を出す（押すと `onExpandHunk`）。
    var expandsHunks = false
    /// 前後を広げる印を押した（ファイルの番号）。
    var onExpandHunk: ((Int) -> Void)?
    /// ファイルごとの行数（まだ差分を取っていないファイル。見出しの行は 0 なので、詳細の行数で出す）。
    var fileCounts: [Int: (added: Int, removed: Int)] = [:] {
        didSet { redraw() }
    }
    /// マウスが乗っているファイルの見出し。
    var hoveredFile: Int? {
        didSet { if hoveredFile != oldValue { redraw() } }
    }
    /// 見出しの下に置く欄（画像の比較など。ファイルの番号 → ビュー）と、その分だけ空ける高さ。
    private(set) var filePanels: [Int: NSView] = [:]
    private var extraSorted: [(row: Int, height: CGFloat)] = []
    let panelClip = PanelClipView()
    /// 差分の代わりに出すビュー（その時点のファイル）。
    var replacement: NSView? {
        didSet {
            oldValue?.removeFromSuperview()
            if let replacement { view.addSubview(replacement) }
            for v in [scrollView, textView, panelClip] as [NSView] { v.isHidden = replacement != nil }
            layoutSubviews()
        }
    }

    func setPanels(_ panels: [Int: NSView]) {
        for v in filePanels.values { v.removeFromSuperview() }
        filePanels = panels
        for v in panels.values { panelClip.addSubview(v, positioned: .below, relativeTo: nil) }
        rebuildExtra()
    }

    /// 木で見せるとき: ファイルの前に出すフォルダの行（名前と深さ）、ファイルの深さと名前。
    struct Folder {
        let name: String
        let depth: Int
        let path: String
        let collapsed: Bool
    }
    private(set) var folderGroups: [Int: [Folder]] = [:]
    /// 畳んだフォルダの中のファイル（見出しの行も出さない）。
    private(set) var hiddenFiles: Set<Int> = []
    private var hiddenRows: Set<Int> = []
    /// フォルダの行を押した（フォルダのパス）。
    var onToggleFolder: ((String) -> Void)?
    var fileDepths: [Int: Int] = [:]
    var fileTitles: [Int: String] = [:]

    func setFolders(_ groups: [Int: [Folder]], hidden: Set<Int> = []) {
        folderGroups = groups
        hiddenFiles = hidden
        hiddenRows = Set(hidden.compactMap { $0 < fileRows.count ? fileRows[$0] : nil })
        rebuildExtra()
    }

    private func rebuildExtra() {
        var entries: [(row: Int, order: Int, height: CGFloat)] = []
        for (file, v) in filePanels where file < fileRows.count { entries.append((fileRows[file], 0, v.frame.height)) }
        for (file, folders) in folderGroups where file < fileRows.count {
            entries.append((fileRows[file] - 1, 1, CGFloat(folders.count) * lineHeight))
        }
        // 畳んだフォルダの中の見出しの行（続いた行ごと）は、その後ろの行を上に詰める。
        var run: [Int] = []
        func flush() {
            if let last = run.last { entries.append((last, 2, -CGFloat(run.count) * lineHeight)) }
            run = []
        }
        for row in hiddenRows.sorted() {
            if let last = run.last, row != last + 1 { flush() }
            run.append(row)
        }
        flush()
        for mark in deletionMarks { entries.append((mark.after, 3, lineHeight)) }
        extraSorted = entries.sorted { ($0.row, $0.order) < ($1.row, $1.order) }.map { ($0.row, $0.height) }
        updateDocumentSize()
        redraw()
    }

    /// その時点のファイルとして見せる（History の「すべてのファイル」）。hunk の見出しと消した行は出さず、消した
    /// ところに「ここで N 行を削除」の行を置く。足した行は緑、変えた行（消した行に続けて足した行）は青。行番号は
    /// 後の側だけ。ファイルごと消したなら、消した行をそのまま出す。
    var fileMode = false
    /// 消したところの印（その前の最後の行と、消した行の数・中身）。
    private var deletionMarks: [(after: Int, count: Int, text: String)] = []
    private var modifiedRows: Set<Int> = []

    /// `fileMode` の、出さない行・印・変えた行を決める。
    private func computeFileMode() {
        deletionMarks = []
        modifiedRows = []
        guard fileMode, let diff else { return }
        var hidden = Set<Int>()
        diff.withRows { all in
            let deletedFile = !all.contains { $0.kind == UInt8(AmonhenDiffRow_ADDED) || $0.kind == UInt8(AmonhenDiffRow_CONTEXT) }
            var row = 0
            while row < all.count {
                let kind = all[row].kind
                if kind == UInt8(AmonhenDiffRow_HUNK) {
                    hidden.insert(row)
                    row += 1
                } else if kind == UInt8(AmonhenDiffRow_REMOVED), !deletedFile {
                    let start = row
                    while row < all.count, all[row].kind == UInt8(AmonhenDiffRow_REMOVED) { row += 1 }
                    for r in start..<row { hidden.insert(r) }
                    if row < all.count, all[row].kind == UInt8(AmonhenDiffRow_ADDED) {
                        while row < all.count, all[row].kind == UInt8(AmonhenDiffRow_ADDED) {
                            modifiedRows.insert(row)
                            row += 1
                        }
                    } else {
                        let text = (start..<row).map { self.text(row: $0) }.joined(separator: "\n")
                        deletionMarks.append((row - 1, row - start, text))
                    }
                } else {
                    row += 1
                }
            }
        }
        hiddenRows = hidden
        rebuildExtra()
    }

    /// 消したところの印の行。
    private func drawDeletionMarks(origin: CGPoint, scale: CGFloat) {
        let font = NSFont.systemFont(ofSize: 11, weight: .medium) as CTFont
        let red = NSColor.systemRed
        for mark in deletionMarks {
            let top = rowY(mark.after + 1) - lineHeight - origin.y
            guard top + lineHeight > 0, top < textView.bounds.height else { continue }
            let key = "deleted|\(mark.count)"
            let glyph = headerGlyphs[key] ?? renderer.shape(L("%ld lines deleted here", mark.count) as CFString, font: font, scale: scale)
            headerGlyphs[key] = glyph
            renderer.addRect(CGRect(x: gutterWidth + 4, y: top + 2, width: glyph.width + 16, height: lineHeight - 4),
                             color: red.withAlphaComponent(0.16).rgba)
            let baseline = top + (lineHeight + CTFontGetAscent(font) - CTFontGetDescent(font)) / 2
            renderer.addLine(glyph, origin: CGPoint(x: gutterWidth + 12, y: baseline), color: red.rgba, scale: scale)
        }
    }

    /// `line` 行目（後の側の行番号。ファイルごと消したなら前の側）を、見えている範囲の上の方に出して選ぶ。
    func scrollToLine(_ line: Int) {
        guard let diff else { return }
        let row = diff.withRows { all in
            all.indices.first { Int(all[$0].new_no) == line && all[$0].kind != UInt8(AmonhenDiffRow_HUNK) }
                ?? all.indices.first { Int(all[$0].old_no) == line && all[$0].kind == UInt8(AmonhenDiffRow_REMOVED) }
        }
        guard let row else { return }
        let clip = scrollView.contentView
        let y = max(0, min(rowY(row) - clip.bounds.height / 3, document.frame.height - clip.bounds.height))
        clip.scroll(to: NSPoint(x: 0, y: y))
        scrollView.reflectScrolledClipView(clip)
        selectedRows = [row]
        redraw()
    }

    /// `point` が消したところの印なら、消した行の中身（ツールチップに出す）。
    func deletedText(at point: NSPoint) -> String? {
        for mark in deletionMarks {
            let top = rowY(mark.after + 1) - lineHeight
            if point.y >= top, point.y < top + lineHeight { return mark.text }
        }
        return nil
    }

    /// `point`（文書の座標）にあるフォルダの行のパス。
    fileprivate func folder(at point: NSPoint) -> String? {
        for (file, folders) in folderGroups where file < fileRows.count {
            let top = rowY(fileRows[file]) - CGFloat(folders.count) * lineHeight
            let i = Int(floor((point.y - top) / lineHeight))
            if i >= 0, i < folders.count { return folders[i].path }
        }
        return nil
    }

    /// 木で見せるときのフォルダの行。
    private func drawFolders(origin: CGPoint, scale: CGFloat) {
        let height = textView.bounds.height
        let small = NSFont.systemFont(ofSize: 12, weight: .semibold) as CTFont
        for (file, folders) in folderGroups where file < fileRows.count {
            let bottom = rowY(fileRows[file]) - origin.y
            let top = bottom - CGFloat(folders.count) * lineHeight
            guard bottom > 0, top < height else { continue }
            for (i, folder) in folders.enumerated() {
                let y = top + CGFloat(i) * lineHeight
                renderer.addRect(CGRect(x: 0, y: y, width: textView.bounds.width, height: lineHeight), color: backdrop.bandColor)
                renderer.addRect(CGRect(x: 0, y: y + lineHeight - 1, width: textView.bounds.width, height: 1), color: NSColor.separatorColor.rgba)
                let key = "folder|\(folder.name)|\(folder.collapsed)"
                let glyph = headerGlyphs[key] ?? renderer.shape("\(folder.collapsed ? "▸" : "▾")  \(folder.name)/" as CFString, font: small, scale: scale)
                headerGlyphs[key] = glyph
                let baseline = y + (lineHeight + CTFontGetAscent(small) - CTFontGetDescent(small)) / 2
                renderer.addLine(glyph, origin: CGPoint(x: 6 + CGFloat(folder.depth) * 14, y: baseline),
                                 color: NSColor.secondaryLabelColor.rgba, scale: scale)
            }
        }
    }

    /// 行の上端（文書の座標）。見出しの下に置いた欄の分だけ下がる。
    func rowY(_ row: Int) -> CGFloat {
        var y = CGFloat(row) * lineHeight
        for e in extraSorted where e.row < row { y += e.height }
        return y
    }

    /// 文書の座標 `y` にある行（欄の中なら、その見出しの行）。
    func rowAt(_ y: CGFloat) -> Int {
        var offset: CGFloat = 0
        for e in extraSorted {
            let start = CGFloat(e.row + 1) * lineHeight + offset
            if e.height < 0 {
                if y < start + e.height { break }
            } else {
                if y < start { break }
                if y < start + e.height { return e.row }
            }
            offset += e.height
        }
        return Int((y - offset) / lineHeight)
    }

    private func positionPanels() {
        guard isViewLoaded else { return }
        panelClip.frame = textView.frame
        let origin = scrollView.contentView.bounds.origin
        let width = textView.bounds.width
        for (file, v) in filePanels where file < fileRows.count {
            v.frame = NSRect(x: 0, y: rowY(fileRows[file]) + lineHeight - origin.y, width: width, height: v.frame.height)
        }
    }

    fileprivate func hover(at point: NSPoint?) {
        if fileMode {
            document.toolTip = point.flatMap { deletedText(at: $0) }
            return
        }
        guard !fileRows.isEmpty else { return }
        hoveredFile = point.flatMap { fileHeader(at: $0) }
    }

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
        // 背景・構文の色を変えたら、見えている差分だけ描き直す。
        NotificationCenter.default.addObserver(self, selector: #selector(redrawIfShown), name: .backdropApplied, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(redrawIfShown), name: .syntaxThemeChanged, object: nil)
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
        panelClip.wheelTarget = scrollView
        container.addSubview(panelClip)
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
        guard isViewLoaded else { return }
        let width = view.bounds.width
        let barHeight = isFindBarVisible ? Self.findBarHeight : 0
        var top = view.bounds.height - barHeight
        findBar.frame = NSRect(x: 0, y: top, width: width, height: barHeight)
        if let topBar {
            top -= Self.topBarHeight
            topBar.frame = NSRect(x: 0, y: top, width: width, height: Self.topBarHeight)
        }
        scrollView.frame = NSRect(x: 0, y: 0, width: width, height: max(0, top))
        replacement?.frame = scrollView.frame
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
        if !filePanels.isEmpty || !folderGroups.isEmpty || !hiddenRows.isEmpty {
            folderGroups = [:]
            hiddenFiles = []
            hiddenRows = []
            setPanels([:])
        }
        if fileMode { computeFileMode() }
        textWidth = diff.map(estimatedTextWidth) ?? 0
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
        let clip = scrollView.contentView.bounds.size
        let width = textWidth > 0 ? gutterWidth + 8 + textWidth + 40 : 0
        document.frame.size = NSSize(width: max(clip.width, width), height: max(clip.height, rowY(diff?.rowCount ?? 0)))
    }

    /// いちばん長い行の幅の見積もり（等幅の文字数から。タブや全角の文字の分は、描いたときに広げる）。
    private func estimatedTextWidth(of diff: FileDiff) -> CGFloat {
        let columns = diff.withRows { all in
            (0..<all.count).reduce(0) { longest, row in
                // 長さ（ASCII のバイト数か UTF-16 の符号単位の数）は行に書いてある。文字列は作らない。
                all[row].kind == UInt8(AmonhenDiffRow_FILE) ? longest : max(longest, Int(all[row].text.len_and_kind & 0x7fff_ffff))
            }
        }
        let digit = ("0" as NSString).size(withAttributes: [.font: font as NSFont]).width
        return CGFloat(columns) * digit
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
        let row = rowAt(point.y)
        return fileRows.firstIndex(of: row)
    }

    /// 上に張り付かせる見出し（ファイルの番号と、見えている範囲の上端からの位置）。見出しの行が見えている範囲より
    /// 上にあるファイルのものを出し、次のファイルの見出しが近づいたら押し上げる。
    private func stickyHeader(origin: CGPoint) -> (Int, CGFloat)? {
        let first = rowAt(origin.y)
        // 最初に見えている行が属するファイル（その見出しが first 以前で最後のもの）。
        guard let file = fileRows.lastIndex(where: { $0 <= first && !hiddenRows.contains($0) }) else { return nil }
        // 見出しの行がちょうど一番上に見えているなら、張り付かせなくてよい。
        if rowY(fileRows[file]) >= origin.y { return nil }
        var top: CGFloat = 0
        if file + 1 < fileRows.count {
            top = min(0, rowY(fileRows[file + 1]) - origin.y - lineHeight)
        }
        return (file, top)
    }

    /// ファイル `index` の見出しが一番上に来るようにスクロールする。
    func scrollToFile(_ index: Int) {
        guard index < fileRows.count else { return }
        let clip = scrollView.contentView
        let y = min(rowY(fileRows[index]), max(0, document.frame.height - clip.bounds.height))
        clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: y))
        scrollView.reflectScrolledClipView(clip)
    }

    /// ファイル `index` の見出しが見えている範囲より上にあれば、見出しが一番上に来るまで戻す。
    func revealFileHeader(_ index: Int) {
        guard index < fileRows.count else { return }
        if rowY(fileRows[index]) < scrollView.contentView.bounds.origin.y { scrollToFile(index) }
    }

    /// ファイルの見出しの右クリック（blame・ファイルの履歴など）。ファイルの番号を渡し、メニューを返してもらう。
    var fileMenu: ((Int) -> NSMenu?)?

    /// AI で hunk を解説する（ファイルのパスと、hunk の差分の文字を渡す）。持ち主が設定したときだけ右クリックに出す。
    var onExplainHunk: ((String, String) -> Void)?
    /// 1 ファイルの差分のときのパス（全ファイルを並べた差分なら見出しの行から取る）。
    var path: String?

    /// 試作: 行の右クリックの「この時点のファイルを開く」（ファイルの番号と、後の側の行番号）。
    var onOpenAtCommit: ((Int, Int?) -> Void)?

    fileprivate func menu(at point: NSPoint) -> NSMenu? {
        if let file = fileHeader(at: point) { return fileMenu?(file) }
        if let onOpenAtCommit, let diff, !fileRows.isEmpty {
            let row = rowAt(point.y)
            if row >= 0, row < diff.rowCount, let file = fileRows.lastIndex(where: { $0 <= row }) {
                let line = diff.withRows { Int($0[row].new_no) }
                let menu = NSMenu()
                menu.addItem(ClosureMenuItem(title: L("Open File at This Commit")) { onOpenAtCommit(file, line > 0 ? line : nil) })
                return menu
            }
        }
        guard let onExplainHunk, Assistant.isConfigured, let (path, text) = hunkText(containing: rowAt(point.y))
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
            // つながらないコミットを並べた差分では、見出しは「パス — 短い ID」。パスだけを渡す。
            let header = file.map { diff.text(row: $0) }
            return (header.map { String($0.components(separatedBy: " — ")[0]) } ?? path ?? "", out)
        }
    }

    /// 見出しの Viewed のチェックボックスの左端（見えている範囲の座標）。

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
        let row = selectedRows.min() ?? rowAt(scrollView.contentView.bounds.origin.y)
        return hunks.lastIndex { $0 <= row }
    }

    /// J・K: 次・前の hunk を選んで、上から 3 分の 1 あたりに出す。
    fileprivate func moveHunk(by step: Int) {
        let hunks = hunkRows
        guard !hunks.isEmpty else { return NSSound.beep() }
        let target: Int
        if selectedRows.isEmpty, let hunkCursor {
            // 選べない差分では、前に J・K で出した hunk から進む（出した hunk は上から 3 分の 1 にあるので、
            // 見えている一番上から探すと同じ hunk にまた止まる）。
            target = hunkCursor + step
        } else if selectedRows.isEmpty {
            // 選んでいなければ、見えている一番上から: J はそこ以降の最初、K はそれより前の最後。
            let top = rowAt(scrollView.contentView.bounds.origin.y)
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
        hunkCursor = index
        if isSelectable {
            selectedRows = Set(start..<end)
            anchorRow = start
            onSelectionChange?()
        }
        let clip = scrollView.contentView
        let top = rowY(start)
        let y = max(0, min(top - clip.bounds.height / 3, document.frame.height - clip.bounds.height))
        if top < clip.bounds.origin.y || rowY(end) > clip.bounds.origin.y + clip.bounds.height {
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
        if let folder = folder(at: point) {
            onToggleFolder?(folder)
            return
        }
        if let file = fileHeader(at: point) {
            let visibleX = point.x - scrollView.contentView.bounds.origin.x
            if visibleX > textView.bounds.width - 30, let menu = fileMenu?(file) {
                menu.popUp(positioning: nil, at: NSPoint(x: point.x, y: point.y + lineHeight / 2), in: document)
            } else {
                onToggleFile?(file)
            }
            return
        }
        // hunk の見出しの左の印: そのファイルの前後の行を広げる。
        if expandsHunks, let onExpandHunk, point.x - scrollView.contentView.bounds.origin.x < numbersWidth {
            let row = rowAt(point.y)
            if row >= 0, row < diff.rowCount, diff.withRows({ $0[row].kind }) == UInt8(AmonhenDiffRow_HUNK),
               let file = fileRows.lastIndex(where: { $0 <= row }) {
                onExpandHunk(file)
                return
            }
        }
        if showsStageChecks, point.x - scrollView.contentView.bounds.origin.x < checkWidth {
            let row = rowAt(point.y)
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
        let row = rowAt(point.y)
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
        let row = rowAt(point.y)
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
        let firstVisible = rowAt(scrollView.contentView.bounds.minY)
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
        let top = rowY(m.row)
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
        positionPanels()
        let scale = view.window?.backingScaleFactor ?? 2
        return renderer.image(size: textView.bounds.size, scale: scale, background: backdrop.diffBackground)
            .map { ($0, textView) }
    }

    fileprivate func redrawFocus() { redraw() }

    @objc private func redrawIfShown() {
        if isViewLoaded, view.window != nil { redraw() }
    }

    @objc private func redraw() {
        buildInstances()
        positionPanels()
        renderer.draw(layer: textView.metalLayer, background: backdrop.diffBackground)
    }

    /// この窓の背景の設定（差分の地・帯の色）。
    fileprivate var backdrop: BackdropSettings { BackdropSettings.of(view.window) }

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
            let first = max(0, rowAt(origin.y))
            let last = min(diff.rowCount, rowAt(origin.y + height) + 1)
            if first < last {
                draw(diff, rows: first..<last, origin: origin, scale: scale)
            }
        }
    }

    private func draw(_ diff: FileDiff, rows: Range<Int>, origin: CGPoint, scale: CGFloat) {
        let ascent = CTFontGetAscent(font)
        // 差分の色は配色テーマから（テーマが持たなければ、設定で標準にしていれば、標準の色）。
        let colors = SyntaxColors.diff
        func color(_ kind: DiffColor) -> SIMD4<Float> { (colors[kind] ?? kind.standard).rgba }
        let added = color(.added), removed = color(.removed), hunk = color(.hunk)
        let strongAdded = color(.addedWord), strongRemoved = color(.removedWord)
        let ours = color(.current), theirs = color(.incoming), base = color(.base), marker = color(.marker)
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
        func collectMatches(_ rows: Range<Int>) {
            guard !matches.isEmpty else { return }
            // 一致は行の順に並んでいるので、最初の行を二分探索する。
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
        collectMatches(rows)
        diff.withRows { all in
            for row in rows {
                if hiddenRows.contains(row) { continue }
                let r = all[row]
                let top = rowY(row) - origin.y
                if selectedRows.contains(row) {
                    renderer.addRect(CGRect(x: 0, y: top, width: gutterWidth, height: lineHeight), color: selection)
                }
                switch r.kind {
                case UInt8(AmonhenDiffRow_ADDED):
                    renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight),
                                     color: modifiedRows.contains(row) ? NSColor.systemBlue.withAlphaComponent(0.2).rgba : added)
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
                    drawCompactHeader(row: row, all: all, top: top, scale: scale)
                    continue
                }
                if expandsHunks, r.kind == UInt8(AmonhenDiffRow_HUNK) {
                    drawExpandMark(top: top, scale: scale)
                }
                if showsStageChecks {
                    drawStageCheck(row: row, kind: r.kind, all: all, top: top, scale: scale)
                }
                let numbers = isConflictView || fileMode
                    ? [(fileMode && r.new_no == 0 ? r.old_no : r.new_no, gutterWidth - 6)]
                    : [(r.old_no, gutterWidth - numbersWidth / 2 - 6), (r.new_no, gutterWidth - 6)]
                for (number, right) in numbers where number > 0 {
                    let line = numberCache[number] ?? renderer.shape("\(number)" as CFString, font: font, scale: scale)
                    numberCache[number] = line
                    renderer.addLine(line, origin: CGPoint(x: right - line.width, y: baseline), color: secondary, scale: scale)
                }
                guard let line = shapedLine(row, scale: scale) else { continue }
                if line.width > textWidth {
                    // 見積もりより長い行（タブ、全角の文字）。描き終えてから広げる。
                    textWidth = line.width
                    DispatchQueue.main.async { [weak self] in self?.updateDocumentSize() }
                }
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
            if !folderGroups.isEmpty { drawFolders(origin: origin, scale: scale) }
            if !deletionMarks.isEmpty { drawDeletionMarks(origin: origin, scale: scale) }
            // 見出しの行が上に隠れたファイルは、見出しを上に張り付かせる。
            if let (file, top) = stickyHeader(origin: origin) {
                let row = fileRows[file]
                if matchesByRow[row] == nil { collectMatches(row..<row + 1) }
                drawCompactHeader(row: row, all: all, top: top, scale: scale)
            }
        }
        if !fileRows.isEmpty {
            let file = fileRows.lastIndex { rowY($0) <= origin.y + lineHeight / 2 } ?? 0
            if file != topFile {
                topFile = file
                onTopFileChange?(file)
            }
        }
    }
}

extension DiffViewController {
    /// ファイルの見出しの行（変更ファイルの一覧の行を兼ねる）: ▸、種類の印、パス（ツリーなら名前）、右に行数と、
    /// いつも出す ⋯（右クリックと同じメニュー）。横にはスクロールしない。
    fileprivate func drawCompactHeader(row: Int, all: UnsafeBufferPointer<AmonhenDiffRow>, top: CGFloat, scale: CGFloat) {
        let r = all[row]
        let width = textView.bounds.width
        let file = fileRows.firstIndex(of: row) ?? 0
        let hovered = hoveredFile == file
        let band = hovered ? NSColor.selectedContentBackgroundColor.withAlphaComponent(0.12).rgba : backdrop.bandColor
        renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: backdrop.bandColor)
        if hovered { renderer.addRect(CGRect(x: 0, y: top, width: width, height: lineHeight), color: band) }
        renderer.addRect(CGRect(x: 0, y: top + lineHeight - 1, width: width, height: 1), color: NSColor.separatorColor.rgba)
        let small = NSFont.systemFont(ofSize: 11) as CTFont
        let bold = NSFont.boldSystemFont(ofSize: 11) as CTFont
        let mono = NSFont.monospacedSystemFont(ofSize: 11, weight: .semibold) as CTFont
        let large = NSFont.systemFont(ofSize: 14) as CTFont
        func glyph(_ text: String, _ font: CTFont) -> TextRenderer.ShapedLine {
            let key = "\(text)|\(CTFontGetSize(font))|\(font === bold ? "b" : font === mono ? "m" : "r")"
            if let cached = headerGlyphs[key] { return cached }
            let line = renderer.shape(text as CFString, font: font, scale: scale)
            headerGlyphs[key] = line
            return line
        }
        let baseline = top + (lineHeight + CTFontGetAscent(small) - CTFontGetDescent(small)) / 2
        let open = r.flags & UInt8(AmonhenDiffRow_COLLAPSED) == 0 || openOverride.contains(file)
        let chevron = glyph(open ? "▾" : "▸", large)
        let chevronBaseline = top + (lineHeight + CTFontGetAscent(large) - CTFontGetDescent(large)) / 2
        // 右から順に置く場所を決め、パスを描いてから、はみ出しを隠して右の側を描く。
        var right = width - 12
        var later: [(TextRenderer.ShapedLine, CGFloat, SIMD4<Float>)] = []
        // よく使う操作は、いつも出す ⋯ のメニュー（右クリックと同じ）。
        let more = glyph("⋯", large)
        later.append((more, right - more.width, (hovered ? NSColor.labelColor : NSColor.secondaryLabelColor).rgba))
        right -= more.width + 14
        let pending = r.flags & UInt8(AmonhenDiffRow_PENDING) != 0
        let (added, removed) = pending ? fileCounts[file] ?? (0, 0) : (Int(r.new_no), Int(r.old_no))
        let counts: [(TextRenderer.ShapedLine, NSColor)] = r.flags & UInt8(AmonhenDiffRow_BINARY) != 0 || fileNotes[file] != nil
            ? [(glyph(fileNotes[file] ?? L("Binary"), small), .secondaryLabelColor)]
            : added == 0 && removed == 0 ? []
            : [(glyph("+\(added)", bold), .systemGreen), (glyph("−\(removed)", bold), .systemRed)]
        let countsWidth = counts.map(\.0.width).reduce(0, +) + CGFloat(max(0, counts.count - 1)) * 6
        var x = right - countsWidth
        for (line, color) in counts {
            later.append((line, x, color.rgba))
            x += line.width + 6
        }
        right -= countsWidth + 14
        let indent = CGFloat(fileDepths[file] ?? 0) * 14
        renderer.addLine(chevron, origin: CGPoint(x: indent + 12 - chevron.width / 2, y: chevronBaseline), color: NSColor.secondaryLabelColor.rgba, scale: scale)
        var pathX: CGFloat = 26 + indent
        if let (mark, color) = fileMarks[file] {
            let m = glyph(mark, mono)
            renderer.addLine(m, origin: CGPoint(x: pathX, y: baseline), color: color.rgba, scale: scale)
            pathX += 16
        }
        if let line = fileTitles[file].map({ glyph($0, font) }) ?? shapedLine(row, scale: scale) {
            renderer.addLine(line, origin: CGPoint(x: pathX, y: top + 2 + CTFontGetAscent(font)), color: NSColor.labelColor.rgba, scale: scale)
            if pathX + line.width > right - 8 {
                renderer.addRect(CGRect(x: right - 8, y: top, width: width - right + 8, height: lineHeight - 1), color: backdrop.bandColor)
                if hovered { renderer.addRect(CGRect(x: right - 8, y: top, width: width - right + 8, height: lineHeight - 1), color: band) }
            }
        }
        for (line, x, color) in later {
            renderer.addLine(line, origin: CGPoint(x: x, y: baseline), color: color, scale: scale)
        }
    }

    /// hunk の見出しの左（行番号の欄）の、前後の行を広げる印。
    fileprivate func drawExpandMark(top: CGFloat, scale: CGFloat) {
        let font = NSFont.systemFont(ofSize: 12, weight: .semibold) as CTFont
        let key = "expand|\(scale)"
        let glyph = headerGlyphs[key] ?? renderer.shape("⤒  ⤓" as CFString, font: font, scale: scale)
        headerGlyphs[key] = glyph
        renderer.addRect(CGRect(x: 0, y: top, width: numbersWidth, height: lineHeight),
                         color: NSColor.controlAccentColor.withAlphaComponent(0.18).rgba)
        let baseline = top + (lineHeight + CTFontGetAscent(font) - CTFontGetDescent(font)) / 2
        renderer.addLine(glyph, origin: CGPoint(x: (numbersWidth - glyph.width) / 2, y: baseline),
                         color: NSColor.controlAccentColor.rgba, scale: scale)
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
            renderer.addRect(box.insetBy(dx: 1, dy: 1), color: backdrop.boxColor)
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

/// 試作: 差分の上に重ねる欄の入れ物。中のビューのないところは、クリックとホイールを下の差分に通す。
final class PanelClipView: NSView {
    weak var wheelTarget: NSView?
    override var isFlipped: Bool { true }
    override init(frame: NSRect) {
        super.init(frame: frame)
        clipsToBounds = true
    }
    required init?(coder: NSCoder) { fatalError() }
    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        return hit === self ? nil : hit
    }
    override func scrollWheel(with event: NSEvent) { wheelTarget?.scrollWheel(with: event) }
}

/// 差分のスクロール用の中身。描くのは上に重ねた Metal のビューで、ここはクリック・コピー・検索を受ける。
/// ホイールを先に `onWheel` に渡す（true を返せば、差分はスクロールしない）。
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

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas { removeTrackingArea(area) }
        addTrackingArea(NSTrackingArea(
            rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
    }

    override func mouseMoved(with event: NSEvent) { owner?.hover(at: convert(event.locationInWindow, from: nil)) }
    override func mouseExited(with event: NSEvent) { owner?.hover(at: nil) }

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
final class CommitDetailViewController: NSSplitViewController, NSTextViewDelegate {
    /// ファイルの右クリック: blame（パス、コミット）とファイルの履歴（パス）。
    var onBlame: ((String, String) -> Void)?
    var onFileHistory: ((String) -> Void)?
    /// ファイルの右クリック: 作業ツリーのファイルを、このコミットの版（`before` ならその前の版）に戻す。`paths` は
    /// 戻すパス（名前を変えたファイルを前の版に戻すときは、元の名前と今の名前）、`rev` はその版。
    var onRestore: ((_ path: String, _ paths: [String], _ rev: String, _ before: Bool) -> Void)?
    /// 表示しているコミット（40 桁）。stash のときは nil（blame の起点にしない）。
    private var commitHex: String?
    /// コミットを選んでから最初のファイルの差分を出すまでを測る（`--measure-select`）。
    private var selectedAt: CFTimeInterval = 0
    /// 全ファイルを縦に並べた差分と、ファイルごとに畳んでいるか（コアに渡す形）。
    private var stacked: StackedDiff?
    private var collapsed: [Bool] = []
    var onDiffShown: (() -> Void)?
    /// 親コミットのリンクを押したとき（40 桁の ID）。
    var onSelectCommit: ((String) -> Void)?
    /// 作者の名前を押したとき（History を検索する問い合わせ）と、ref の名前を押したとき。
    var onSearch: ((String) -> Void)?
    var onSelectRef: ((String) -> Void)?
    /// 署名をどこで検証しているか（吹き出しに出す。RepositoryWindow が決める）。
    var signaturePlace = SignaturePlace(local: true, detail: L("the repository is on this Mac"))
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

    /// sparse checkout なら、その様子（取り出していないフォルダのファイルは、ディスクにないので開けない）。
    var sparse: RepositoryForm.Sparse?

    /// 表示中のコミットの親と子（キーボードで移るため）。子は詳細を読み終えるまで空。shallow clone の境界の
    /// コミットの親は取得していないので、移れない（空）。
    var currentParents: [String] { info?.parentsNotFetched == true ? [] : info?.parents ?? [] }
    var currentChildren: [String] { extras?.children ?? [] }
    /// メッセージ・作者・ID・親コミット。
    private let header = FindableTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 170))
    private let repository: Repository
    private let diffView: DiffViewController
    private var changes: CommitChanges?
    private var request = 0
    private var headerRequest = 0
    /// 表示しているコミットの情報と、裏で読んだ詳細（見せ方を変えたときに描き直す）。
    private var info: CommitInfo?
    private var extras: CommitExtras? {
        didSet {
            // ファイルの多いコミットで、まだ差分を取っていないファイルの行数は、詳細と一緒に読んだもので出す。
            fileStats = Dictionary((extras?.stats ?? []).map { ($0.path, $0) }, uniquingKeysWith: { a, _ in a })
            updatePendingCounts()
        }
    }
    private var fileStats: [String: CommitExtras.FileStat] = [:]
    /// 署名を検証する側を変えた回数。変える前に始めた詳細の読み込みの結果は使わない。
    private var signatureGeneration = 0

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

    override func viewDidLoad() {
        super.viewDidLoad()
        // 上に詳細（メッセージとメタ情報）、下に差分。境目の位置は覚えておく。変更ファイルの一覧は差分の
        // ファイルの見出しそのもの（最初はすべて畳む）。
        splitView.isVertical = false
        splitView.autosaveName = "CommitDetail"
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
        let headerContainer = NSView(frame: headerScroll.frame)
        headerContainer.addSubview(headerScroll)
        self.headerContainer = headerContainer
        self.headerScroll = headerScroll
        peopleStrip.isHidden = true
        peopleStrip.onSelectPerson = { [weak self] name in
            self?.onSearch?("author:\"\(name.replacingOccurrences(of: "\"", with: ""))\"")
        }
        headerContainer.addSubview(peopleStrip)
        styleControl.selectedSegment = isPreview ? CommitHeaderStyle.standard.rawValue : CommitHeaderStyle.saved.rawValue
        if CommitHeaderStyle.saved == .compact, !isPreview {
            // 簡潔で始めるなら、最初から低くしておく。
            headerContainer.frame.size.height = 48
        }
        // ウィンドウの高さを変えたとき、詳細の高さは保ち、差分の側を伸び縮みさせる。
        let headerController = NSViewController()
        headerController.view = headerContainer
        let headerItem = NSSplitViewItem(viewController: headerController)
        headerItem.minimumThickness = 40
        headerItem.holdingPriority = .init(260)
        addSplitViewItem(headerItem)
        diffView.loadViewIfNeeded()
        diffView.expandsHunks = true
        diffView.topBar = makeBar()
        let diffItem = NSSplitViewItem(viewController: diffView)
        diffItem.minimumThickness = 200
        diffItem.canCollapse = true
        addSplitViewItem(diffItem)
        diffView.onToggleFile = { [weak self] file in self?.toggleOpen(file) }
        diffView.fileMenu = { [weak self] file in self?.menuForFile(file) }
        diffView.onExplainHunk = { [weak self] path, hunk in self?.explainHunk(path: path, hunk: hunk) }
        diffView.onExpandHunk = { [weak self] file in self?.widenContext(file) }
        diffView.onOpenAtCommit = { [weak self] file, line in self?.openAtCommit(file: file, line: line) }
        diffView.onToggleFolder = { [weak self] folder in
            guard let self else { return }
            if collapsedFolders.contains(folder) { collapsedFolders.remove(folder) } else { collapsedFolders.insert(folder) }
            reshowStacked(keepScroll: true)
        }
        NotificationCenter.default.addObserver(
            self, selector: #selector(fileTreeChanged), name: .historyFileTreeChanged, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(messageStyleChanged), name: .commitMessageStyleChanged, object: nil)
        // アバターを読み終えたら、表示中のコミットの人なら描き直す。
        Avatars.shared.onLoad.append { [weak self] email in
            guard let self, let info,
                  [info.authorEmail, info.committerEmail].contains(where: { $0.lowercased() == email }) else { return }
            peopleStrip.updateAvatars()
            renderHeader()
        }
    }

    /// 開いている（差分を出す）ファイル、SVG をテキストで見るファイル、ツリーで畳んだフォルダ。最初はすべて畳む。
    fileprivate var openFiles: Set<Int> = []
    /// ファイル全体を出しているファイルと、前後の行を広げたファイルの行数。
    fileprivate var wholeFiles: Set<Int> = []
    fileprivate var contexts: [Int: Int] = [:]
    /// まだ差分を取っていないファイル（ファイルの多いコミット。開いたときに取る）。
    fileprivate var pendingFiles: Set<Int> = []
    /// 画像の比べ方（コミットを変えても保つ）。
    fileprivate var imageMode: ImageDiffPanel.Mode = .twoUp
    /// 帯の表示の設定のメニュー。
    fileprivate var viewOptions: NSMenu?
    fileprivate var svgAsText: Set<Int> = []
    fileprivate var filesBrowser: CommitFilesBrowser?
    fileprivate let viewSwitch = NSSegmentedControl(labels: [L("Changes"), L("All Files")], trackingMode: .selectOne, target: nil, action: nil)
    fileprivate let barTotals = NSTextField(labelWithString: "")
    fileprivate let expandAllButton = NSButton()
    fileprivate var collapsedFolders: Set<String> = []
    fileprivate let foldersButton = NSButton()
    fileprivate var whitespaceBox: NSButton?
    fileprivate let treeSwitch: NSSegmentedControl = {
        let control = NSSegmentedControl(
            images: [NSImage(systemSymbolName: "list.bullet", accessibilityDescription: L("List"))!,
                     NSImage(systemSymbolName: "list.bullet.indent", accessibilityDescription: L("Tree"))!],
            trackingMode: .selectOne, target: nil, action: nil)
        control.setToolTip(L("Show the changed files as a list"), forSegment: 0)
        control.setToolTip(L("Show the changed files as a tree"), forSegment: 1)
        control.controlSize = .small
        control.selectedSegment = Settings.historyFileTree ? 1 : 0
        return control
    }()

    /// 表示中のコミット。
    private var shownCommit: (id: Data, stash: Bool)?

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
        // 書き直すのは History の詳細だけ（`onReword` があるもの。Reflog・Range Diff では保存先がない）。
        if onReword != nil, extras?.headDistance != nil, info.parents.count <= 1 {
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
        guard view.window != nil else { return [] }
        // すべてのファイルの中身も、差分と同じく Metal で描いている。
        if let filesBrowser, diffView.replacement === filesBrowser {
            return filesBrowser.overlay().map { [$0] } ?? []
        }
        var out = diffView.overlay().map { [$0] } ?? []
        // 差分の上に重ねた欄（画像の比較）も、差分の絵の後に重ねる。
        let clip = diffView.panelClip
        if diffView.replacement == nil, !clip.subviews.isEmpty, let rep = clip.bitmapImageRepForCachingDisplay(in: clip.bounds) {
            clip.cacheDisplay(in: clip.bounds, to: rep)
            if let image = rep.cgImage { out.append((image, clip)) }
        }
        return out
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
        // 変わったファイルと全ファイルの差分を続けて読む（メインスレッドとの往復を 1 回で済ませる）。ファイルの
        // 多いコミットでは、コアは見出しだけ並べ、開いたファイルの差分をそのときに取る。
        let changesTask = Task.detached(priority: .userInitiated) {
            () -> (CommitChanges?, StackedDiff?, CFTimeInterval, CFTimeInterval) in
            let t = CACurrentMediaTime()
            let changes = try? stash ? repository.stashChanges(of: commit) : repository.changes(of: commit)
            let stacked = changes.flatMap { $0.items.isEmpty ? nil : try? $0.stacked() }
            return (changes, stacked, t, CACurrentMediaTime())
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
            let (changes, stacked, callStart, callEnd) = await changesTask.value
            guard request == self.request else { return }
            let resumed = CACurrentMediaTime()
            FrameProfile.add("select: changed files: start delay", callStart - selectedAt)
            FrameProfile.add("select: changed files: in call", callEnd - callStart)
            FrameProfile.add("select: changed files: main resume delay", resumed - callEnd)
            self.changes = changes
            self.stacked = stacked
            if changes == nil || (stacked == nil && changes?.items.isEmpty == false) {
                // 読めなかった（変更がないのとは違う）。
                diffView.show(nil, message: L("Couldn’t load the diff"))
                onDiffShown?()
            } else if changes?.items.isEmpty ?? true {
                diffView.show(nil, message: L("No changes"))
                onDiffShown?()
            } else if let stacked {
                showStacked(stacked, key: hex)
                FrameProfile.add("select: → diff shown", CACurrentMediaTime() - selectedAt)
                PerformanceLog.record("History: select → diff", ms: (CACurrentMediaTime() - selectedAt) * 1000)
                onDiffShown?()
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
        renderSelectionHeader(rows: rows, history: history, kind: nil)
        diffView.show(nil, message: L("Loading the diff…"))
        let started = CACurrentMediaTime()
        Task {
            let result = await Task.detached(priority: .userInitiated) { try? history.selectionDiff(rows: rows, pair: pair) }.value
            guard request == self.request else { return }
            guard let (kind, stacked) = result else {
                renderSelectionHeader(rows: rows, history: history, kind: nil, failed: true)
                diffView.show(nil, message: L("Couldn’t load the diff"))
                return
            }
            NSLog("選んだ %d コミットの差分を %.0f ms で読んだ", rows.count, (CACurrentMediaTime() - started) * 1000)
            self.stacked = stacked
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
        renderRangeHeader(title: title, lines: lines, note: L("Loading the diff…"))
        diffView.show(nil, message: L("Loading the diff…"))
        let repository = self.repository
        Task {
            let result = await Task.detached(priority: .userInitiated) {
                try? repository.changes(from: base, to: tip).stacked()
            }.value
            guard request == self.request else { return }
            guard let stacked = result else {
                renderRangeHeader(title: title, lines: lines, note: L("Couldn’t load the diff"))
                diffView.show(nil, message: L("Couldn’t load the diff"))
                return
            }
            self.stacked = stacked
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
    /// `kind` が nil なら読んでいる途中（`failed` なら読めなかった）。
    private func renderSelectionHeader(rows: [Int], history: History, kind: SelectionKind?, pair: PairMode = .forward,
                                       failed: Bool = false) {
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
        case nil: failed ? L("Couldn’t load the diff") : L("Loading the diff…")
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

    /// 1 ファイルの変更がこの行数を超えたら、最初は畳む。
    private static let collapseAbove = 1000

    private func showStacked(_ stacked: StackedDiff, key: String) {
        showInline(stacked)
    }

    /// ファイルの見出しの右クリック。とびとびに選んだコミットの見出し（「パス — 短い ID」）からはパスを取り出す。
    private func menuForFile(_ file: Int) -> NSMenu? {
        guard let stacked, file < stacked.names.count else { return nil }
        let name = stacked.names[file]
        let path = name.components(separatedBy: " — ").first ?? name
        let menu = NSMenu()
        menu.addItem(ClosureMenuItem(title: openFiles.contains(file) ? L("Collapse") : L("Expand")) { [weak self] in self?.toggleOpen(file) })
        let whole = ClosureMenuItem(title: L("Show Whole File")) { [weak self] in self?.toggleWholeFile(file) }
        whole.state = wholeFiles.contains(file) ? .on : .off
        menu.addItem(whole)
        if commitHex != nil, changes.map({ file < $0.items.count && $0.items[file].kind != .deleted }) ?? false {
            menu.addItem(ClosureMenuItem(title: L("Open File at This Commit")) { [weak self] in self?.openAtCommit(file: file, line: nil) })
        }
        menu.addItem(.separator())
        // 消したファイルは、そのコミットにはない（ファイルの一覧の右クリックと同じく Blame を出さない）。
        let deleted = changes.map { file < $0.items.count && $0.items[file].kind == .deleted } ?? false
        if let commit = commitHex, !deleted {
            menu.addItem(ClosureMenuItem(title: Titles.blame(at: String(commit.prefix(Titles.shortIDLength)))) { [weak self] in self?.onBlame?(path, commit) })
        }
        menu.addItem(ClosureMenuItem(title: Titles.fileHistory) { [weak self] in self?.onFileHistory?(path) })
        if let item = changes.flatMap({ file < $0.items.count ? $0.items[file] : nil }) {
            addRestoreItems(to: menu, item: item)
        }
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
                let editor = ClosureMenuItem(title: L("Open in Editor")) { ExternalEditor.open([URL(fileURLWithPath: full)]) }
                editor.toolTip = ExternalEditor.toolTip
                menu.addItem(editor)
                menu.addItem(ClosureMenuItem(title: L("Open with Default App")) { NSWorkspace.shared.open(URL(fileURLWithPath: full)) })
            } else if let sparse, !sparse.contains(path) {
                // sparse checkout で取り出していないフォルダのファイル。項目は出すが、押せないわけを添える。
                for title in [L("Show in Finder"), L("Open in Editor"), L("Open with Default App")] {
                    let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
                    item.toolTip = L("This folder isn’t checked out")
                    menu.addItem(item)
                }
            }
        }
        return menu
    }

    private func renderHeader() {
        guard let info else { return }
        let style = CommitHeaderStyle(rawValue: styleControl.selectedSegment) ?? .standard
        var renderer = CommitHeaderRenderer(info: info, extras: extras, style: style)
        renderer.monospacedBody = Settings.monospacedCommitMessage
        renderer.highlight = CommitHeaderRenderer.highlightTerms(of: searchQuery)
        renderer.linksRefs = onSelectRef != nil
        renderer.linksAuthor = onSearch != nil
        peopleStrip.selectable = onSearch != nil
        let text = NSMutableAttributedString(attributedString: renderer.render())
        let showPeople = style != .compact
        // 帯の高さは人によるので、人を入れてから並べる。
        if showPeople { peopleStrip.show(info, date: CommitHeaderRenderer.date) }
        layoutPeopleStrip(visible: showPeople)
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

    /// 作者とコミッターの帯を上に置き、文字の欄をその下にする。
    private func layoutPeopleStrip(visible: Bool) {
        guard let container = headerContainer, let scroll = headerScroll else { return }
        peopleStrip.isHidden = !visible
        let height = visible ? peopleStrip.preferredHeight : 0
        let bounds = container.bounds
        peopleStrip.frame = NSRect(
            x: 0, y: bounds.height - height, width: bounds.width, height: height)
        peopleStrip.autoresizingMask = [.width, .minYMargin]
        scroll.frame = NSRect(x: 0, y: 0, width: bounds.width, height: bounds.height - height)
    }

    // MARK: - AI


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

    /// 作業ツリーのファイルを、このコミットの版か、その前の版に戻す（`git restore --source`）。このコミットで消した
    /// ファイルは、このコミットの版にはない。前の版に戻すと、このコミットで足したファイルは消え、名前を変えたファイルは
    /// 元の名前に戻る。
    private func addRestoreItems(to menu: NSMenu, item: FileChangeItem) {
        guard let commit = commitHex, onRestore != nil, !item.isSubmodule else { return }
        menu.addItem(.separator())
        if item.kind != .deleted {
            menu.addItem(ClosureMenuItem(title: L("Restore This Version…")) { [weak self] in
                self?.onRestore?(item.path, [item.path], commit, false)
            })
        }
        let paths = [item.oldPath, item.path].compactMap { $0 }
        menu.addItem(ClosureMenuItem(title: L("Restore Version Before This Commit…")) { [weak self] in
            self?.onRestore?(item.path, paths, commit + "^", true)
        })
    }

}

// MARK: - 変更ファイルの行・帯・その時点のファイル

extension CommitDetailViewController {
    /// `file` 番目のファイルのパス（とびとびに選んだコミットの見出し「パス — 短い ID」からはパスを取り出す）。
    fileprivate func path(of file: Int) -> String {
        if let items = changes?.items, file < items.count { return items[file].path }
        let name = stacked?.names[file] ?? ""
        return name.components(separatedBy: " — ").first ?? name
    }

    /// 1 ファイルの変更がこの行数を超えたら、「差分をすべて開く」でも開かない（1 つずつなら開ける）。
    private static let expandAllLimit = 1000

    /// 右の列の上の帯: 変更／すべてのファイル、合計、リスト／ツリー、フォルダ・差分をすべて開く、空白、検索、
    /// 表示の設定。
    fileprivate func makeBar() -> NSView {
        viewSwitch.selectedSegment = 0
        viewSwitch.controlSize = .small
        viewSwitch.target = self
        viewSwitch.action = #selector(viewSwitchChanged)
        barTotals.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        barTotals.lineBreakMode = .byTruncatingTail
        barTotals.usesSingleLineMode = true
        barTotals.maximumNumberOfLines = 1
        barTotals.setContentHuggingPriority(.required, for: .horizontal)
        for button in [foldersButton, expandAllButton] {
            button.bezelStyle = .accessoryBarAction
            button.controlSize = .small
            button.imagePosition = .imageOnly
            button.target = self
        }
        foldersButton.action = #selector(toggleAllFolders)
        expandAllButton.action = #selector(toggleAllDiffs)
        let whitespace = NSButton(checkboxWithTitle: L("Ignore Whitespace"), target: nil, action: nil)
        whitespace.controlSize = .small
        whitespace.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        whitespace.isEnabled = false
        whitespace.toolTip = L("Not available yet")
        whitespaceBox = whitespace
        let find = NSButton(
            image: NSImage(systemSymbolName: "magnifyingglass", accessibilityDescription: L("Find"))!,
            target: self, action: #selector(findInDiff))
        find.bezelStyle = .accessoryBarAction
        find.controlSize = .small
        find.toolTip = L("Find in the diff (⌘F)")
        let display = NSPopUpButton(frame: .zero, pullsDown: true)
        display.bezelStyle = .accessoryBarAction
        display.controlSize = .small
        display.isBordered = false
        display.toolTip = L("View Options")
        display.menu?.delegate = self
        let first = NSMenuItem()
        first.image = NSImage(systemSymbolName: "slider.horizontal.3", accessibilityDescription: L("View Options"))
        first.title = ""
        display.menu?.addItem(first)
        viewOptions = display.menu
        treeSwitch.target = self
        treeSwitch.action = #selector(treeSwitchChanged)
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        let stack = NSStackView(views: [viewSwitch, barTotals, spacer, treeSwitch, foldersButton, expandAllButton, whitespace, find, display])
        // 狭いときは右の側から切れる（差分の列の幅を押し広げない）。
        for v in stack.arrangedSubviews { v.setContentCompressionResistancePriority(.defaultLow, for: .horizontal) }
        stack.setClippingResistancePriority(.defaultLow, for: .horizontal)
        stack.spacing = 10
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 8)
        let bar = NSView()
        stack.translatesAutoresizingMaskIntoConstraints = false
        let line = NSBox()
        line.boxType = .separator
        line.translatesAutoresizingMaskIntoConstraints = false
        bar.addSubview(stack)
        bar.addSubview(line)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: bar.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: bar.trailingAnchor),
            stack.centerYAnchor.constraint(equalTo: bar.centerYAnchor),
            line.leadingAnchor.constraint(equalTo: bar.leadingAnchor),
            line.trailingAnchor.constraint(equalTo: bar.trailingAnchor),
            line.bottomAnchor.constraint(equalTo: bar.bottomAnchor),
        ])
        return bar
    }

    /// 表示の設定のメニュー（開くたびに作る）: 詳細の見せ方、詳細を広げる、AI。
    fileprivate func fillViewOptions(_ menu: NSMenu) {
        while menu.items.count > 1 { menu.removeItem(at: 1) }
        let section = NSMenuItem(title: L("Details"), action: nil, keyEquivalent: "")
        section.isEnabled = false
        menu.addItem(section)
        for style in CommitHeaderStyle.allCases {
            let item = ClosureMenuItem(title: style.title) { [weak self] in
                self?.styleControl.selectedSegment = style.rawValue
                self?.styleChanged()
            }
            item.state = styleControl.selectedSegment == style.rawValue ? .on : .off
            item.indentationLevel = 1
            menu.addItem(item)
        }
        let expand = ClosureMenuItem(title: L("Expand Details")) { [weak self] in self?.toggleExpanded() }
        expand.state = isExpanded ? .on : .off
        expand.toolTip = L("Expand the commit details to fill the right column (click again to restore).")
        menu.addItem(expand)
        guard Assistant.isConfigured, !isPreview else { return }
        menu.addItem(.separator())
        menu.addItem(ClosureMenuItem(title: L("AI: Explain This Commit")) { [weak self] in self?.explainCommit() })
        menu.addItem(ClosureMenuItem(title: L("AI: Translate the Message")) { [weak self] in self?.translateMessage() })
    }

    @objc fileprivate func findInDiff() { diffView.showFindBar() }

    @objc fileprivate func treeSwitchChanged() {
        Settings.historyFileTree = treeSwitch.selectedSegment == 1
    }

    @objc func fileTreeChanged() {
        treeSwitch.selectedSegment = Settings.historyFileTree ? 1 : 0
        if viewSwitch.selectedSegment == 1 { updateBar() } else { reshowStacked(keepScroll: true) }
    }

    /// ツリーで見せるときの、フォルダの行・ファイルの深さと名前・畳んだフォルダの中のファイル（並びはパスの順のまま）。
    fileprivate func treeLayout(count: Int)
        -> (groups: [Int: [DiffViewController.Folder]], depths: [Int: Int], titles: [Int: String], hidden: Set<Int>)?
    {
        guard treeSwitch.selectedSegment == 1 else { return nil }
        var groups: [Int: [DiffViewController.Folder]] = [:]
        var depths: [Int: Int] = [:]
        var titles: [Int: String] = [:]
        var hidden: Set<Int> = []
        var previous: [String] = []
        for file in 0..<count {
            let path = path(of: file)
            var parts = path.split(separator: "/").map(String.init)
            let name = parts.removeLast()
            var common = 0
            while common < min(previous.count, parts.count), previous[common] == parts[common] { common += 1 }
            // 畳んだフォルダ（いちばん外側）。その中は見出しを出さない。
            let outer = (0..<parts.count).first { collapsedFolders.contains(parts[0...$0].joined(separator: "/")) }
            var folders: [DiffViewController.Folder] = []
            for level in common..<parts.count where outer == nil || level <= outer! {
                let folderPath = parts[0...level].joined(separator: "/")
                folders.append(.init(name: parts[level], depth: level, path: folderPath, collapsed: collapsedFolders.contains(folderPath)))
            }
            if outer != nil { hidden.insert(file) }
            if !folders.isEmpty { groups[file] = folders }
            depths[file] = parts.count
            // 名前を変えたファイルは、同じフォルダの中なら `old → new`。
            let label = changes.flatMap { file < $0.items.count ? $0.items[file].label : nil } ?? path
            let folder = parts.joined(separator: "/")
            titles[file] = folder.isEmpty ? label : label.hasPrefix(folder + "/") ? String(label.dropFirst(folder.count + 1)) : name
            previous = parts
        }
        return (groups, depths, titles, hidden)
    }

    /// リストの見出し: 名前を変えたファイルは git の --stat と同じ `src/{old → new}`。ほかはコアのパスのまま。
    fileprivate func renameTitles(count: Int) -> [Int: String] {
        guard let items = changes?.items else { return [:] }
        var titles: [Int: String] = [:]
        for file in 0..<min(count, items.count) where items[file].label != items[file].path {
            titles[file] = items[file].label
        }
        return titles
    }

    /// コミットを選んだ: すべて畳んで、変更ファイルの一覧として見せる。
    fileprivate func showInline(_ stacked: StackedDiff) {
        openFiles = []
        svgAsText = []
        wholeFiles = []
        contexts = [:]
        collapsedFolders = []
        // まだ差分を取っていないファイル（ファイルの多いコミット）。
        pendingFiles = stacked.view(collapsed: [Bool](repeating: true, count: stacked.count)).withRows { rows in
            Set(rows.indices.filter { rows[$0].flags & UInt8(AmonhenDiffRow_PENDING) != 0 })
        }
        updateBarTotals()
        var marks: [Int: (String, NSColor)] = [:]
        var notes: [Int: String] = [:]
        for file in 0..<stacked.count {
            if let items = changes?.items, file < items.count { marks[file] = FileCell.mark(items[file].kind) }
            let p = path(of: file)
            if ImageDiffPanel.isImage(p), !ImageDiffPanel.isSVG(p) { notes[file] = L("Image") }
        }
        diffView.fileMarks = marks
        diffView.fileNotes = notes
        updatePendingCounts()
        filesBrowser = nil
        setFilesMode(false)
        reshowStacked(keepScroll: false)
        applySnapshotOptions()
    }

    /// 帯の合計（「10 ファイル +9 −8」）。まだ差分を取っていないファイルの行数は、詳細の行数で数える。
    fileprivate func updateBarTotals() {
        guard let stacked else { return barTotals.stringValue = "" }
        var (added, removed) = stacked.totals
        for file in pendingFiles {
            if let stat = fileStats[path(of: file)] {
                added += stat.added ?? 0
                removed += stat.removed ?? 0
            }
        }
        let text = NSMutableAttributedString(
            string: L("%ld files", stacked.count) + "  ", attributes: [.foregroundColor: NSColor.secondaryLabelColor])
        text.append(NSAttributedString(string: "+\(added)", attributes: [.foregroundColor: NSColor.systemGreen]))
        text.append(NSAttributedString(string: " −\(removed)", attributes: [.foregroundColor: NSColor.systemRed]))
        text.addAttribute(.font, value: NSFont.monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular),
                          range: NSRange(location: 0, length: text.length))
        barTotals.attributedStringValue = text
    }

    /// まだ差分を取っていないファイルの見出しの行数（詳細と一緒に読んだもの）。
    fileprivate func updatePendingCounts() {
        var counts: [Int: (added: Int, removed: Int)] = [:]
        for file in pendingFiles {
            if let stat = fileStats[path(of: file)] { counts[file] = (stat.added ?? 0, stat.removed ?? 0) }
        }
        diffView.fileCounts = counts
        updateBarTotals()
    }

    /// 開いている・畳んでいるファイル、ツリー、画像の欄から、差分の表示を作り直す。
    fileprivate func reshowStacked(keepScroll: Bool) {
        guard let stacked else { return }
        let images = Set((0..<stacked.count).filter { ImageDiffPanel.isImage(path(of: $0)) })
        let tree = treeLayout(count: stacked.count)
        let hidden = tree?.hidden ?? []
        let panelFiles = openFiles.intersection(images).subtracting(svgAsText).subtracting(hidden)
        collapsed = (0..<stacked.count).map { !openFiles.contains($0) || panelFiles.contains($0) || hidden.contains($0) }
        diffView.openOverride = panelFiles
        diffView.show(stacked.view(collapsed: collapsed), keepScroll: keepScroll)
        diffView.fileDepths = tree?.depths ?? [:]
        diffView.fileTitles = tree?.titles ?? renameTitles(count: stacked.count)
        diffView.setFolders(tree?.groups ?? [:], hidden: hidden)
        var panels: [Int: NSView] = [:]
        for file in panelFiles { if let panel = makeImagePanel(file) { panels[file] = panel } }
        diffView.setPanels(panels)
        updateBar()
    }

    /// 見出しを押した: そのファイルの差分を開く・畳む。まだ差分を取っていなければ、ここで取る。
    fileprivate func toggleOpen(_ file: Int) {
        if openFiles.contains(file) {
            openFiles.remove(file)
        } else {
            load([file])
            openFiles.insert(file)
        }
        reshowStacked(keepScroll: true)
        diffView.revealFileHeader(file)
    }

    /// まだ差分を取っていないファイルの差分を取る。
    private func load(_ files: [Int]) {
        guard let stacked else { return }
        for file in files where pendingFiles.contains(file) {
            do {
                try stacked.setContext(file, 3, in: repository)
                pendingFiles.remove(file)
            } catch {
                NSLog("差分を読めない（%@）: %@", path(of: file), "\(error)")
            }
        }
        updatePendingCounts()
    }

    /// hunk の見出しの印: そのファイルの前後の行を 20 行ずつ広げる。
    fileprivate func widenContext(_ file: Int) {
        guard let stacked, !wholeFiles.contains(file) else { return }
        let context = (contexts[file] ?? 3) + 20
        guard (try? stacked.setContext(file, UInt32(context), in: repository)) != nil else { return NSSound.beep() }
        contexts[file] = context
        reshowStacked(keepScroll: true)
    }

    /// 見出しの ⋯ > ファイル全体を表示: そのファイルの差分の前後の行を、ファイル全体にする・戻す。
    fileprivate func toggleWholeFile(_ file: Int) {
        guard let stacked else { return }
        load([file])
        let whole = !wholeFiles.contains(file)
        let context = whole ? UInt32.max : UInt32(contexts[file] ?? 3)
        guard (try? stacked.setContext(file, context, in: repository)) != nil else { return NSSound.beep() }
        if whole { wholeFiles.insert(file) } else { wholeFiles.remove(file) }
        openFiles.insert(file)
        reshowStacked(keepScroll: true)
        diffView.revealFileHeader(file)
    }

    fileprivate func makeImagePanel(_ file: Int) -> NSView? {
        guard let changes, file < changes.items.count else { return nil }
        let item = changes.items[file]
        let panel = ImageDiffPanel(
            before: item.kind == .added ? nil : changes.blob(at: file, after: false),
            after: item.kind == .deleted ? nil : changes.blob(at: file, after: true),
            svg: ImageDiffPanel.isSVG(item.path), mode: imageMode, position: 0.5)
        panel.frame.size.height = 300
        panel.onModeChange = { [weak self] mode in self?.imageMode = mode }
        panel.onShowText = { [weak self] in
            guard let self else { return }
            load([file])
            svgAsText.insert(file)
            reshowStacked(keepScroll: true)
        }
        return panel
    }

    /// 帯の部品を、今の見せ方で効くものだけにする（すべてのファイルでは差分にしか効かないものを隠す、フォルダの
    /// 操作はツリーのとき）。
    fileprivate func updateBar() {
        // その時点の木は 1 つのコミットにだけある（stash・複数のコミット・範囲では押せない）。
        viewSwitch.setEnabled(commitHex != nil && shownCommit != nil, forSegment: 1)
        let files = viewSwitch.selectedSegment == 1
        let tree = treeSwitch.selectedSegment == 1
        expandAllButton.isHidden = files
        whitespaceBox?.isHidden = files
        foldersButton.isHidden = !tree
        expandAllButton.toolTip = allDiffsOpen ? L("Collapse All Diffs") : L("Expand All Diffs")
        expandAllButton.image = NSImage(
            systemSymbolName: allDiffsOpen ? "rectangle.compress.vertical" : "rectangle.expand.vertical",
            accessibilityDescription: expandAllButton.toolTip)
        let expanded = files ? filesBrowser?.anyFolderExpanded ?? false : collapsedFolders.isEmpty
        foldersButton.toolTip = expanded ? L("Collapse All Folders") : L("Expand All Folders")
        foldersButton.image = NSImage(
            systemSymbolName: expanded ? "folder.fill" : "folder", accessibilityDescription: foldersButton.toolTip)
        if files { filesBrowser?.flat = !tree }
    }

    /// すべてのフォルダを開く（どれか畳んでいれば）・畳む。差分を開いているかは変えない。
    @objc fileprivate func toggleAllFolders() {
        if viewSwitch.selectedSegment == 1, let browser = filesBrowser {
            browser.setAllFolders(expanded: !browser.anyFolderExpanded)
            updateBar()
            return
        }
        guard let stacked else { return }
        if collapsedFolders.isEmpty {
            var all = Set<String>()
            for file in 0..<stacked.count {
                let parts = path(of: file).split(separator: "/").dropLast()
                for level in parts.indices { all.insert(parts[...level].joined(separator: "/")) }
            }
            collapsedFolders = all
        } else {
            collapsedFolders = []
        }
        reshowStacked(keepScroll: true)
    }

    /// 「差分をすべて開く」で開くファイル（生成物と、変更の多いファイルは除く）。
    private var expandableFiles: Set<Int> {
        guard let stacked else { return [] }
        let rows = stacked.view(collapsed: [Bool](repeating: true, count: stacked.count))
        return rows.withRows { all in
            Set((0..<stacked.count).filter { file in
                !stacked.isGenerated(file)
                    && Int(all[file].old_no + all[file].new_no) <= Self.expandAllLimit
                    && (fileStats[path(of: file)].map { ($0.added ?? 0) + ($0.removed ?? 0) <= Self.expandAllLimit } ?? true)
            })
        }
    }

    /// 生成物などのほかは、すべて開いているか。
    fileprivate var allDiffsOpen: Bool {
        let targets = expandableFiles
        return !targets.isEmpty && targets.isSubset(of: openFiles)
    }

    @objc fileprivate func toggleAllDiffs() {
        if allDiffsOpen {
            openFiles = []
        } else {
            let targets = expandableFiles
            load(Array(targets))
            openFiles.formUnion(targets)
        }
        reshowStacked(keepScroll: false)
    }

    @objc fileprivate func viewSwitchChanged() { setFilesMode(viewSwitch.selectedSegment == 1) }

    /// 右の列を「すべてのファイル」（その時点の木と中身）にする・変更に戻す。
    fileprivate func setFilesMode(_ on: Bool) {
        viewSwitch.selectedSegment = on ? 1 : 0
        if on {
            if filesBrowser == nil, let commit = commitHex, let id = shownCommit?.id {
                let browser = CommitFilesBrowser(
                    repository: repository, commit: commit, commitID: id, changes: changes, renderer: diffView.renderer)
                browser.onShowChange = { [weak self] file in
                    guard let self else { return }
                    setFilesMode(false)
                    load([file])
                    openFiles.insert(file)
                    reshowStacked(keepScroll: true)
                    diffView.scrollToFile(file)
                }
                browser.onFoldersChange = { [weak self] in self?.updateBar() }
                filesBrowser = browser
            }
            diffView.replacement = filesBrowser
        } else {
            diffView.replacement = nil
        }
        updateBar()
    }

    /// その時点のファイルを開く（見出しの ⋯・行の右クリック）。
    fileprivate func openAtCommit(file: Int, line: Int?) {
        setFilesMode(true)
        filesBrowser?.open(path: path(of: file), line: line)
    }

    /// 確かめる用（`--snapshot`）: 開くファイル・ツリーで畳むフォルダ・すべてのファイル・画像の見せ方。
    private func applySnapshotOptions() {
        let defaults = UserDefaults.standard
        if let open = defaults.string(forKey: "AMHOpenFiles") {
            let files = open.split(separator: ",").compactMap { Int($0) }
            load(files)
            openFiles = Set(files)
        }
        if let folders = defaults.string(forKey: "AMHCollapsedFolders") {
            collapsedFolders = Set(folders.split(separator: ",").map(String.init))
        }
        if defaults.object(forKey: "AMHOpenFiles") != nil || defaults.object(forKey: "AMHCollapsedFolders") != nil {
            reshowStacked(keepScroll: false)
        }
        if defaults.bool(forKey: "AMHExpandAll") { toggleAllDiffs() }
        if let target = defaults.string(forKey: "AMHFilesMode") {
            setFilesMode(true)
            if target != "tree" {
                let parts = target.split(separator: ":")
                filesBrowser?.open(path: String(parts[0]), line: parts.count > 1 ? Int(parts[1]) : nil)
            }
        }
    }
}

extension CommitDetailViewController: NSMenuDelegate {
    func menuNeedsUpdate(_ menu: NSMenu) {
        if menu === viewOptions { fillViewOptions(menu) }
    }
}

final class FileCell: NSTableCellView {
    let path = NSTextField(labelWithString: "")
    private let stat = NSTextField(labelWithString: "")
    private var leading: NSLayoutConstraint!
    /// 木で出すときの字下げ。
    var indent: CGFloat = 0 {
        didSet { leading.constant = 2 + indent }
    }

    /// 変更の種類の印と色（追加は緑、削除は赤、変更は橙、名前の変更・コピーは青）。
    static func mark(_ kind: ChangeKind) -> (String, NSColor) {
        switch kind {
        case .added: ("A", .systemGreen)
        case .deleted: ("D", .systemRed)
        case .modified: ("M", .systemOrange)
        case .renamed: ("R", .systemBlue)
        case .copied: ("C", .systemBlue)
        }
    }

    /// ファイルの行の文字: 色の付いた種類の印、薄いフォルダ、ふつうの色のファイル名。名前の変更の `{a → b}` の
    /// 中は分けない（その後ろの `/` までをフォルダとする）。
    static func pathTitle(_ path: String, kind: ChangeKind?) -> NSAttributedString {
        let text = NSMutableAttributedString()
        let font = NSFont.systemFont(ofSize: NSFont.systemFontSize)
        if let kind {
            let (mark, color) = mark(kind)
            text.append(NSAttributedString(string: mark + "  ", attributes: [
                .foregroundColor: color,
                .font: NSFont.monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .semibold),
            ]))
        }
        var depth = 0
        var split = path.startIndex
        for i in path.indices {
            switch path[i] {
            case "{": depth += 1
            case "}": depth -= 1
            case "/" where depth == 0: split = path.index(after: i)
            default: break
            }
        }
        text.append(NSAttributedString(string: String(path[..<split]), attributes: [.foregroundColor: NSColor.secondaryLabelColor, .font: font]))
        text.append(NSAttributedString(string: String(path[split...]), attributes: [.foregroundColor: NSColor.labelColor, .font: font]))
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingMiddle
        text.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: 0, length: text.length))
        return text
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


