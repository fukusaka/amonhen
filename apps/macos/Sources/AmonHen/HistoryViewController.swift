import CAmonhen
import AmonhenKit
import AppKit
import QuartzCore

/// 表の上に重ねてグラフを描くビュー。マウス操作は下の表に通す。
final class GraphView: NSView {
    let metalLayer = CAMetalLayer()

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        metalLayer.pixelFormat = .bgra8Unorm
        metalLayer.isOpaque = false
        metalLayer.framebufferOnly = true
        // 描画先は 3 枚（標準）。2 枚だと 120Hz では表示まで約 3 フレームかかる間ずっと描画先がふさがり、
        // 速くスクロールすると nextDrawable でメインスレッドが止まってフレームを落とす（linux で計測）。
        metalLayer.maximumDrawableCount = 3
        // 表のスクロールと同じフレームで表示して、表とグラフをずらさない。遅れは変わらない（計測済み）。
        metalLayer.presentsWithTransaction = true
        layer = metalLayer
        layerContentsRedrawPolicy = .never
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        metalLayer.contentsScale = window?.backingScaleFactor ?? 2
        updateDrawableSize()
    }

    // フレームは親が直接決めるので、layout() ではなくここで描き先の大きさを合わせる。
    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        updateDrawableSize()
    }

    private func updateDrawableSize() {
        let scale = metalLayer.contentsScale
        metalLayer.drawableSize = CGSize(width: bounds.width * scale, height: bounds.height * scale)
    }
}

/// 選択の色は表にフォーカスがあるかで変わるので、文字を描き直せるように知らせる。
final class FocusTableView: NSTableView {
    var onFocusChange: (() -> Void)?
    /// ドラッグする行の文字。セルを置いていないので、ドラッグの画像はこれから作る。
    var dragLabel: ((IndexSet) -> String?)?

    override func dragImageForRows(
        with dragRows: IndexSet, tableColumns: [NSTableColumn], event: NSEvent, offset: NSPointPointer
    ) -> NSImage {
        guard let label = dragLabel?(dragRows) else {
            return super.dragImageForRows(with: dragRows, tableColumns: tableColumns, event: event, offset: offset)
        }
        let text = NSAttributedString(string: label, attributes: [.font: NSFont.systemFont(ofSize: NSFont.systemFontSize)])
        let size = NSSize(width: min(ceil(text.size().width) + 12, 480), height: rowHeight)
        return NSImage(size: size, flipped: false) { rect in
            NSColor.controlBackgroundColor.withAlphaComponent(0.85).setFill()
            NSBezierPath(roundedRect: rect, xRadius: 4, yRadius: 4).fill()
            text.draw(with: rect.insetBy(dx: 6, dy: (rect.height - text.size().height) / 2),
                      options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
            return true
        }
    }

    override func becomeFirstResponder() -> Bool {
        defer { onFocusChange?() }
        return super.becomeFirstResponder()
    }

    override func resignFirstResponder() -> Bool {
        defer { onFocusChange?() }
        return super.resignFirstResponder()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        NotificationCenter.default.removeObserver(self)
        guard let window else { return }
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification] {
            NotificationCenter.default.addObserver(self, selector: #selector(keyChanged), name: name, object: window)
        }
    }

    @objc private func keyChanged() { onFocusChange?() }
}

final class HistoryViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    private let repository: Repository
    private var history: History?
    private let renderer: GraphRenderer
    private let scrollView = NSScrollView()
    private let tableView = FocusTableView()
    private let graphView = GraphView()
    /// 件名などを描く。表のセルに文字を描かせると、速いスクロールでは CPU で描く分が間に合わない。
    private let textView = GraphView()
    private let text: TextRenderer
    /// 行は 2 行: 1 行目に ref のラベルと件名、2 行目に作者・日時・短い ID。
    private struct RowText {
        var subject: TextRenderer.ShapedLine
        var meta: TextRenderer.ShapedLine
    }
    /// 行ごとに並べた文字。History が変わったら捨てる。
    private var rowText: [Int: RowText] = [:]
    private var ellipsis: TextRenderer.ShapedLine?
    private var metaEllipsis: TextRenderer.ShapedLine?
    private let font = CTFontCreateUIFontForLanguage(.system, NSFont.systemFontSize, nil)!
    private let metaFont = CTFontCreateUIFontForLanguage(.system, NSFont.smallSystemFontSize, nil)!
    private let labelFont = CTFontCreateWithName(
        NSFont.systemFont(ofSize: NSFont.smallSystemFontSize, weight: .medium).fontName as CFString,
        NSFont.smallSystemFontSize, nil)

    /// コミットに付いている ref のラベル（ブランチ・リモートブランチ・タグ）。
    struct RefLabel {
        let name: String
        let kind: RefKind
        /// 今のブランチ。
        let isHead: Bool
    }
    private var refLabels: [Data: [RefLabel]] = [:]
    private var labelText: [String: TextRenderer.ShapedLine] = [:]
    private let graphColumn = NSTableColumn(identifier: .init("graph"))
    private var chunks: [Int: HistoryChunk] = [:]
    /// 裏で読んでいるチャンク。
    private var loadingChunks: Set<Int> = []
    private static let chunkRows = 256

    /// 入力イベントから画面に表示されるまでの時間（秒）。
    var onLatency: ((CFTimeInterval) -> Void)?
    var actions: Actions?

    /// 表示する範囲。
    enum Scope: Equatable {
        case all
        case current
        /// サイドバーで選んだ ref（名前, コミット）。
        case ref(String, Data)
    }

    private(set) var scope: Scope = .all
    private let scopePopup = NSPopUpButton()
    /// HEAD のコミット（Current Branch で使う）。
    var head: Data?
    /// 選んだコミット（20 バイト）。選択がなくなれば nil。
    var onSelectCommit: ((Data?) -> Void)?

    init(repository: Repository, renderer: GraphRenderer, text: TextRenderer) {
        self.repository = repository
        self.renderer = renderer
        self.text = text
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        // 分割の最初の幅は、このビューの幅で決まる。
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 560, height: 700))
        tableView.headerView = NSTableHeaderView()
        tableView.rowHeight = CGFloat(renderer.metrics.rowHeight)
        tableView.intercellSpacing = NSSize(width: 6, height: 0)
        tableView.usesAlternatingRowBackgroundColors = false
        tableView.allowsTypeSelect = false
        tableView.style = .plain
        tableView.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        graphColumn.title = ""
        graphColumn.width = 120
        tableView.addTableColumn(graphColumn)
        let subject = NSTableColumn(identifier: .init("subject"))
        subject.title = "Message"
        subject.width = 240
        tableView.addTableColumn(subject)
        tableView.dataSource = self
        tableView.delegate = self
        tableView.dragLabel = { [weak self] rows in
            guard let row = rows.first, let chunk = self?.chunks[row / Self.chunkRows] else { return nil }
            return chunk.subject(row: row)
        }
        tableView.onFocusChange = { [weak self] in
            // フォーカスが移り終えてから（first responder が入れ替わった後で）描く。
            DispatchQueue.main.async { self?.redrawText() }
        }
        let menu = NSMenu()
        menu.delegate = self
        tableView.menu = menu
        // コミットをドラッグして並べ替える。
        tableView.registerForDraggedTypes([.commitID])
        tableView.setDraggingSourceOperationMask(.move, forLocal: true)

        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = true
        scrollView.autoresizingMask = [.width, .height]
        scrollView.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self, selector: #selector(contentDidScroll), name: NSView.boundsDidChangeNotification,
            object: scrollView.contentView)
        NotificationCenter.default.addObserver(
            self, selector: #selector(contentDidScroll), name: NSTableView.columnDidResizeNotification,
            object: tableView)

        graphView.metalLayer.device = renderer.device
        textView.metalLayer.device = text.device
        scopePopup.target = self
        scopePopup.action = #selector(scopeChanged)
        scopePopup.controlSize = .small
        updateScopePopup()
        container.addSubview(scrollView)
        container.addSubview(graphView)
        container.addSubview(textView)
        container.addSubview(scopePopup)
        view = container
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        // 上に範囲の切り替え、その下に表。
        let barHeight: CGFloat = 26
        scopePopup.frame = NSRect(x: 6, y: view.bounds.height - barHeight + 2, width: 240, height: barHeight - 4)
        scrollView.frame = NSRect(x: 0, y: 0, width: view.bounds.width, height: view.bounds.height - barHeight)
        // 件名の列を表の幅いっぱいにする。
        tableView.sizeLastColumnToFit()
        layoutGraphView()
    }

    private func updateScopePopup() {
        scopePopup.removeAllItems()
        scopePopup.addItems(withTitles: ["All Branches", "Current Branch"])
        if case let .ref(name, _) = scope {
            scopePopup.addItem(withTitle: name)
            scopePopup.selectItem(at: 2)
        } else {
            scopePopup.selectItem(at: scope == .current ? 1 : 0)
        }
    }

    @objc private func scopeChanged() {
        switch scopePopup.indexOfSelectedItem {
        case 0: set(scope: .all)
        case 1: set(scope: .current)
        default: break
        }
    }

    /// 表示する範囲を変えて読み直す。
    func set(scope: Scope, select commit: Data? = nil) {
        self.scope = scope
        updateScopePopup()
        reloadScope(select: commit)
    }

    private func reloadScope(select commit: Data? = nil) {
        let repository = self.repository
        let tips: [Data]? = switch scope {
        case .all: nil
        case .current: head.map { [$0] }
        case let .ref(_, target): [target]
        }
        Task {
            let next: History? = await Task.detached(priority: .userInitiated) {
                if let tips { try? repository.filteredHistory(tips: tips) } else { repository.latestHistory() }
            }.value
            guard let next else { return }
            show(next, resetScroll: true)
            if let commit, let row = next.row(of: commit) { select(row: row) }
        }
    }

    /// 前回の History を永続キャッシュからすぐ表示し、最新にするのは裏で行う。
    func load(completion: @escaping (Error?) -> Void) {
        if let cached = repository.loadCachedHistory() {
            show(cached)
        }
        let repository = self.repository
        Task {
            do {
                let latest = try await Task.detached(priority: .userInitiated) { try repository.loadHistory() }.value
                if scope != .all {
                    reloadScope()
                } else if latest.generation != history?.generation {
                    show(latest)
                }
                completion(nil)
            } catch {
                completion(error)
            }
        }
    }

    /// 表示中の History。
    var currentHistory: History? { history }

    /// 裏で読み直し、変わっていれば表示し直す。
    func refresh() {
        let repository = self.repository
        Task {
            guard let latest = try? await Task.detached(priority: .userInitiated, operation: { try repository.loadHistory() }).value
            else { return }
            if scope != .all {
                reloadScope()
            } else if latest.generation != history?.generation {
                show(latest)
            }
        }
    }

    /// `row` を選び、表の中ほどに見えるようにする。
    func select(row: Int) {
        tableView.selectRowIndexes([row], byExtendingSelection: false)
        let clip = scrollView.contentView
        let rect = tableView.rect(ofRow: row)
        // 上端はヘッダーの分だけ負になる（表の 0 行目がヘッダーのすぐ下に来る位置）。
        let top = -clip.contentInsets.top
        let bottom = max(top, tableView.bounds.height - clip.bounds.height)
        let y = min(max(top, rect.midY - clip.bounds.height / 2), bottom)
        clip.scroll(to: NSPoint(x: clip.bounds.minX, y: y))
        scrollView.reflectScrolledClipView(clip)
    }

    private func show(_ history: History, resetScroll: Bool = false) {
        let first = self.history == nil || resetScroll
        self.history = history
        chunks.removeAll()
        loadingChunks.removeAll()
        rowText.removeAll()
        tableView.reloadData()
        if first { tableView.scrollRowToVisible(0) }
        // 何も選んでいなければ先頭（いちばん新しいコミット）を選び、詳細が空のままにならないようにする。
        if tableView.selectedRow < 0, history.rowCount > 0 {
            tableView.selectRowIndexes([0], byExtendingSelection: false)
        }
        redrawGraph()
    }

    /// グラフの列と、その右の文字の列のうち、ヘッダーの下で見えている範囲に重ねる。
    private func layoutGraphView() {
        let clip = scrollView.contentView
        let clipFrame = view.convert(clip.frame, from: scrollView)
        let column = tableView.rect(ofColumn: tableView.column(withIdentifier: graphColumn.identifier))
        let x = view.convert(column.origin, from: tableView).x
        let height = max(0, clipFrame.height - clip.contentInsets.top)
        graphView.frame = NSRect(x: x, y: clipFrame.minY, width: column.width, height: height)
        let textX = x + column.width
        textView.frame = NSRect(x: textX, y: clipFrame.minY, width: max(0, clipFrame.maxX - textX), height: height)
        redrawGraph()
        redrawText()
    }

    private func redrawText() {
        guard history != nil, textView.window != nil else { return }
        buildText(scale: textView.metalLayer.contentsScale)
        text.draw(layer: textView.metalLayer, background: .zero)
    }

    /// ref の一覧が変わった。各コミットのラベルを作り直して描き直す。
    func show(refs: Refs) {
        var labels: [Data: [RefLabel]] = [:]
        for item in refs.items {
            guard let target = item.target, [.local, .remote, .tag].contains(item.kind) else { continue }
            labels[target, default: []].append(
                RefLabel(name: item.name, kind: item.kind, isHead: item.kind == .local && item.name == refs.headBranch))
        }
        // 今のブランチ、ローカル、リモート、タグの順。
        let order: (RefLabel) -> Int = { $0.isHead ? 0 : [.local: 1, .remote: 2, .tag: 3][$0.kind] ?? 4 }
        refLabels = labels.mapValues { $0.sorted { (order($0), $0.name) < (order($1), $1.name) } }
        redrawText()
    }

    /// 見えている行の文字を積む。まだ読めていない行は空けておき、読めたら描き直す。
    private func buildText(scale: CGFloat) {
        text.instances.removeAll(keepingCapacity: true)
        guard let history else { return }
        let scrollY = tableView.convert(NSPoint(x: 0, y: 0), from: textView).y
        let originX = tableView.convert(NSPoint(x: 0, y: 0), from: textView).x
        let rows = tableView.rows(in: NSRect(x: 0, y: scrollY, width: 1, height: textView.bounds.height))
        guard rows.length > 0 else { return }
        // 表のセルの文字の位置に合わせる（列の左右の余白）。
        let column = tableView.rect(ofColumn: 1).insetBy(dx: 5, dy: 0)
        let ellipsis = self.ellipsis ?? text.shape("…" as CFString, font: font, scale: scale)
        self.ellipsis = ellipsis
        let metaEllipsis = self.metaEllipsis ?? text.shape("…" as CFString, font: metaFont, scale: scale)
        self.metaEllipsis = metaEllipsis
        // 2 行を行の高さの中ほどに置く。
        let subjectHeight = CTFontGetAscent(font) + CTFontGetDescent(font)
        let metaHeight = CTFontGetAscent(metaFont) + CTFontGetDescent(metaFont)
        let gap: CGFloat = 2
        let top = ((tableView.rowHeight - subjectHeight - gap - metaHeight) / 2).rounded()
        let subjectBaseline = top + CTFontGetAscent(font)
        let metaBaseline = (subjectBaseline + CTFontGetDescent(font) + gap + CTFontGetAscent(metaFont)).rounded()
        let focused = view.window?.isKeyWindow == true && view.window?.firstResponder === tableView
        let primary = NSColor.labelColor.rgba, secondary = NSColor.secondaryLabelColor.rgba
        let selected = (focused ? NSColor.alternateSelectedControlTextColor : NSColor.labelColor).rgba
        let selectedSecondary = (focused ? NSColor.alternateSelectedControlTextColor.withAlphaComponent(0.75)
            : NSColor.secondaryLabelColor).rgba
        let selection = tableView.selectedRowIndexes
        if rowText.count > 4096 { rowText.removeAll() }
        for row in rows.location..<min(history.rowCount, rows.location + rows.length) {
            guard let line = shapedRow(row, scale: scale) else { continue }
            let rowTop = tableView.rect(ofRow: row).minY - scrollY
            let isSelected = selection.contains(row)
            var x = column.minX - originX
            let end = column.maxX - originX
            if let id = history.commitID(row: row), let labels = refLabels[id] {
                x = drawLabels(labels, x: x, maxX: end - 40, baseline: rowTop + subjectBaseline, scale: scale,
                               selected: isSelected && focused)
            }
            text.addLine(
                line.subject, origin: CGPoint(x: x, y: rowTop + subjectBaseline), color: isSelected ? selected : primary,
                scale: scale, maxWidth: max(0, end - x), ellipsis: ellipsis)
            text.addLine(
                line.meta, origin: CGPoint(x: column.minX - originX, y: rowTop + metaBaseline),
                color: isSelected ? selectedSecondary : secondary, scale: scale, maxWidth: column.width,
                ellipsis: metaEllipsis)
        }
    }

    /// 件名の前に ref のラベルを並べ、件名を始める位置を返す。入りきらない分は「+N」にまとめる。
    private func drawLabels(
        _ labels: [RefLabel], x start: CGFloat, maxX: CGFloat, baseline: CGFloat, scale: CGFloat, selected: Bool
    ) -> CGFloat {
        let ascent = CTFontGetAscent(labelFont), descent = CTFontGetDescent(labelFont)
        let height = (ascent + descent + 4).rounded()
        let top = (baseline - ascent - 2).rounded()
        var x = start
        for (i, label) in labels.enumerated() {
            let shown = i < 3 ? label.name : "+\(labels.count - i)"
            let line = labelText[shown] ?? text.shape(shown as CFString, font: labelFont, scale: scale)
            labelText[shown] = line
            let width = (line.width + 10).rounded()
            if x + width > maxX, i > 0 { break }
            let (fill, ink) = labelColors(i < 3 ? label : nil, selected: selected)
            text.addRoundedRect(CGRect(x: x, y: top, width: width, height: height), radius: 4, color: fill)
            text.addLine(line, origin: CGPoint(x: x + 5, y: baseline - 1), color: ink, scale: scale)
            x += width + 4
            if i >= 3 { break }
        }
        return x + 2
    }

    private func labelColors(_ label: RefLabel?, selected: Bool) -> (SIMD4<Float>, SIMD4<Float>) {
        guard let label else { return (NSColor.systemGray.withAlphaComponent(0.25).rgba, NSColor.labelColor.rgba) }
        let base: NSColor = switch label.kind {
        case .local: .systemBlue
        case .remote: .systemPurple
        default: .systemOrange
        }
        if label.isHead { return (base.withAlphaComponent(0.85).rgba, NSColor.white.rgba) }
        return (base.withAlphaComponent(selected ? 0.45 : 0.22).rgba, (selected ? NSColor.white : NSColor.labelColor).rgba)
    }

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .short
        return f
    }()

    private func shapedRow(_ row: Int, scale: CGFloat) -> RowText? {
        if let cached = rowText[row] { return cached }
        guard let chunk = chunk(row: row) else { return nil }
        let subject = chunk.withSubject(row: row) { text.shape($0, font: font, scale: scale) }
        let short = history?.commitID(row: row).map { $0.prefix(4).map { String(format: "%02x", $0) }.joined() } ?? ""
        let date = Self.dateFormatter.string(from: chunk.authorTime(row: row))
        let meta = "\(chunk.authorName(row: row))  ·  \(date)  ·  \(short)" as CFString
        let line = RowText(subject: subject, meta: text.shape(meta, font: metaFont, scale: scale))
        rowText[row] = line
        return line
    }

    @objc private func contentDidScroll() {
        motion?.add(y: scrollView.contentView.bounds.minY)
        layoutGraphView()
    }

    /// レーンの位置が動いている間だけ、表示のリフレッシュごとに描き直す。
    private var laneAnimation: CADisplayLink?
    private var lastGraphDraw: CFTimeInterval = 0

    private func updateLaneAnimation() {
        guard renderer.isAnimating else {
            laneAnimation?.isPaused = true
            return
        }
        if laneAnimation == nil {
            let link = graphView.displayLink(target: self, selector: #selector(laneAnimationTick))
            link.add(to: .main, forMode: .common)
            laneAnimation = link
        }
        laneAnimation?.isPaused = false
    }

    @objc private func laneAnimationTick() {
        // スクロールで同じフレームにもう描いていれば描かない。
        if CACurrentMediaTime() - lastGraphDraw > 0.004 { redrawGraph() }
    }

    private func redrawGraph() {
        guard let history else { return }
        defer {
            lastGraphDraw = CACurrentMediaTime()
            updateLaneAnimation()
        }
        // グラフのビューの上端が、表の座標でどこに当たるか。
        let scrollY = Float(tableView.convert(NSPoint(x: 0, y: 0), from: graphView).y)
        let eventTime = pendingPresentStart ?? NSApp.currentEvent?.timestamp
        pendingPresentStart = nil
        let measurement = self.measurement
        var onCompleted: (@Sendable (CFTimeInterval) -> Void)?
        if measurement != nil, let eventTime {
            onCompleted = { [weak self] gpuEnd in
                Task { @MainActor in self?.measurement?.gpuDone.append(gpuEnd - eventTime) }
            }
        }
        renderer.draw(history: history, layer: graphView.metalLayer, scrollY: scrollY, onCompleted: onCompleted) {
            [weak self] presented in
            guard let eventTime, presented > 0 else { return }
            Task { @MainActor in
                measurement?.presented.append(presented)
                measurement?.latency.append(presented - eventTime)
                self?.onLatency?(presented - eventTime)
            }
        }
    }

    // MARK: - 計測

    private var measurement: ScrollMeasurement?
    private var motion: ScrollMotionLog?

    /// トラックパッドでのスクロールを 1 回ずつ記録する（`--log-scroll`）。NSScrollView は続くイベントを
    /// 自分で受け取るのでイベントは見えない。代わりに表の位置の動きを記録し、減速の仕方を確かめる。
    func logScrollGestures() {
        view.window?.title += "（スクロールを記録中）"
        motion = ScrollMotionLog()
    }

    /// 表示のリフレッシュごとに `rowsPerFrame` 行ずつ、`frames` フレーム分スクロールして測る。
    func measureScroll(frames: Int, rowsPerFrame: Double, completion: @escaping (String) -> Void) {
        let m = ScrollMeasurement(frames: frames, step: rowsPerFrame * Double(renderer.metrics.rowHeight), completion: completion)
        measurement = m
        m.link = view.displayLink(target: self, selector: #selector(measureFrame(_:)))
        m.link?.add(to: .main, forMode: .common)
    }

    @objc private func measureFrame(_ link: CADisplayLink) {
        guard let m = measurement else { return }
        if m.link == nil {
            link.invalidate()
            // 最後のフレームが表示されるのを待ってからまとめる。
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
                m.completion(m.summary(refresh: link.duration)
                    + "\nrenderer: drawn \(self.renderer.drawnFrames)  skipped \(self.renderer.skippedFrames)")
                self.measurement = nil
            }
            return
        }
        let clip = scrollView.contentView
        let start = CACurrentMediaTime()
        pendingPresentStart = start
        var origin = clip.bounds.origin
        origin.y += CGFloat(m.step)
        clip.scroll(to: origin)
        scrollView.reflectScrolledClipView(clip)
        tableView.displayIfNeeded()
        let end = CACurrentMediaTime()
        m.work.append(end - start)
        m.frame += 1
        if m.frame >= m.frames { m.link = nil }
    }

    /// 次の描画で、表示までの時間を測る起点。
    private var pendingPresentStart: CFTimeInterval?

    /// `root` の中身にグラフを重ねた画像（`--snapshot` 用）。
    func snapshot(of root: NSView, extra: [(CGImage, NSView)] = []) -> NSBitmapImageRep? {
        guard let rep = root.bitmapImageRepForCachingDisplay(in: root.bounds) else { return nil }
        root.cacheDisplay(in: root.bounds, to: rep)
        guard let context = NSGraphicsContext(bitmapImageRep: rep) else { return rep }
        var overlays = extra
        let scale = view.window?.backingScaleFactor ?? 2
        let scrollY = Float(tableView.convert(NSPoint(x: 0, y: 0), from: graphView).y)
        // History の画面が隠れているときはグラフを描かない。
        if let history, graphView.window != nil,
           let graph = renderer.image(history: history, size: graphView.bounds.size, scale: scale, scrollY: scrollY) {
            overlays.insert((graph, graphView), at: 0)
            buildText(scale: scale)
            if let image = text.image(size: textView.bounds.size, scale: scale, background: .zero) {
                overlays.insert((image, textView), at: 1)
            }
        }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        for (image, view) in overlays {
            var frame = root.convert(view.bounds, from: view)
            if root.isFlipped { frame.origin.y = root.bounds.height - frame.maxY }
            context.cgContext.draw(image, in: frame)
        }
        NSGraphicsContext.restoreGraphicsState()
        return rep
    }

    // MARK: - 表のデータ

    /// 読み終えていれば返す。まだなら裏で読み始め（前後のチャンクも先読みし）、読めたら行を描き直す。
    private func chunk(row: Int) -> HistoryChunk? {
        let index = row / Self.chunkRows
        for i in [index, index + 1, index - 1] { prefetch(chunk: i) }
        return chunks[index]
    }

    private func prefetch(chunk index: Int) {
        guard let history, index >= 0, index * Self.chunkRows < history.rowCount,
              chunks[index] == nil, !loadingChunks.contains(index) else { return }
        loadingChunks.insert(index)
        let rows = index * Self.chunkRows..<min(history.rowCount, (index + 1) * Self.chunkRows)
        Task {
            let chunk = await Task.detached(priority: .userInitiated) { try? history.chunk(rows: rows) }.value
            do {
                // 読んでいる間に History が入れ替わっていたら捨てる。
                guard self.history === history else { return }
                self.loadingChunks.remove(index)
                self.chunks[index] = chunk
                let visible = self.tableView.rows(in: self.tableView.visibleRect)
                let lower = max(rows.lowerBound, visible.location)
                let upper = min(rows.upperBound, visible.location + visible.length)
                if lower < upper { self.redrawText() }
            }
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let row = tableView.clickedRow
        guard let actions, let history, row >= 0, let id = history.commitID(row: row) else { return }
        let hex = id.map { String(format: "%02x", $0) }.joined()
        let short = String(hex.prefix(8))
        let isMerge = history.withNodes { $0[row].flags & UInt16(AmonhenNode_MERGE) != 0 }
        func add(_ title: String, _ action: @escaping () -> Void) {
            menu.addItem(ClosureMenuItem(title: title, action: action))
        }
        add("New Branch at \(short)…") { actions.newBranch(at: hex) }
        add("New Tag at \(short)…") { actions.newTag(at: hex) }
        add("Checkout \(short) (Detached)") { actions.perform(["op": "detach", "oid": hex]) }
        menu.addItem(.separator())
        if !isMerge {
            add("Edit Message…") { actions.reword(hex) }
            add("Squash into Parent…") { actions.squash(hex) }
            add("Fixup into Parent") { actions.rewrite("Fixup \(short)", ["kind": "fixup", "oid": hex]) }
            add("Edit (Stop Here)") { actions.rewrite("Edit \(short)", ["kind": "edit", "oid": hex]) }
            add("Split…") { actions.rewrite("Split \(short)", ["kind": "split", "oid": hex]) }
            add("Drop") {
                guard actions.confirm("\(short) を取り除きますか？", message: "Undo で戻せます。", ok: "取り除く") else { return }
                actions.rewrite("Drop \(short)", ["kind": "drop", "oid": hex])
            }
            menu.addItem(.separator())
        }
        add("Cherry-pick \(short)") { actions.perform(["op": "cherryPick", "oid": hex, "isMerge": isMerge]) }
        add("Revert \(short)") { actions.perform(["op": "revert", "oid": hex, "isMerge": isMerge]) }
        menu.addItem(.separator())
        add("Copy Commit ID") {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(hex, forType: .string)
        }
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        redrawText()
        let row = tableView.selectedRow
        onSelectCommit?(row >= 0 ? history?.commitID(row: row) : nil)
    }

    // MARK: - ドラッグで並べ替え

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> (any NSPasteboardWriting)? {
        guard let id = history?.commitID(row: row) else { return nil }
        let item = NSPasteboardItem()
        item.setString(id.map { String(format: "%02x", $0) }.joined(), forType: .commitID)
        return item
    }

    func tableView(
        _ tableView: NSTableView, validateDrop info: any NSDraggingInfo, proposedRow row: Int,
        proposedDropOperation op: NSTableView.DropOperation
    ) -> NSDragOperation {
        guard info.draggingSource as? NSTableView === tableView else { return [] }
        tableView.setDropRow(row, dropOperation: .above)
        return .move
    }

    func tableView(
        _ tableView: NSTableView, acceptDrop info: any NSDraggingInfo, row: Int, dropOperation: NSTableView.DropOperation
    ) -> Bool {
        guard let actions, let history, let oid = info.draggingPasteboard.string(forType: .commitID) else { return false }
        // 落とした位置の下にあるコミット（なければ上）が、編集できる並びの何番目かで移動先を決める。
        let below = history.commitID(row: min(row, history.rowCount - 1)).map { $0.map { String(format: "%02x", $0) }.joined() }
        Task {
            let chain = await actions.editableCommits()
            guard let from = chain.firstIndex(of: oid), let below, var to = chain.firstIndex(of: below) else {
                await MainActor.run {
                    let alert = NSAlert()
                    alert.messageText = "ここには動かせません"
                    alert.informativeText = "HEAD から最初の merge までのコミットの間でだけ並べ替えられます。"
                    alert.runModal()
                }
                return
            }
            if row >= history.rowCount { to = chain.count - 1 } else if from < to { to -= 1 }
            if from == to { return }
            actions.rewrite("Move \(oid.prefix(8))", ["kind": "move", "oid": oid, "to": to])
        }
        return true
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        history?.rowCount ?? 0
    }

    /// 文字は `textView` に描くので、セルは置かない。
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        nil
    }
}

/// スクロール位置の動きの記録。0.3 秒止まったら 1 回分としてまとめる。
final class ScrollMotionLog {
    private var samples: [(t: CFTimeInterval, y: CGFloat)] = []
    private var idle: DispatchWorkItem?

    func add(y: CGFloat) {
        samples.append((CACurrentMediaTime(), y))
        idle?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.finish() }
        idle = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
    }

    private func finish() {
        defer { samples.removeAll() }
        guard samples.count > 3, let first = samples.first, let last = samples.last else { return }
        // 約 25 ms ごとの速度（pt/s）。
        var speeds: [(t: CFTimeInterval, v: CGFloat)] = []
        var a = first
        for s in samples.dropFirst() where s.t - a.t >= 0.025 {
            speeds.append((s.t - first.t, abs(s.y - a.y) / CGFloat(s.t - a.t)))
            a = s
        }
        let peak = speeds.max { $0.v < $1.v }
        // 最後の 50 ms の速度。滑らかに止まれば小さく、途中で打ち切られれば大きいまま。
        let tail = samples.last { last.t - $0.t >= 0.05 }
        let tailSpeed = tail.map { abs(last.y - $0.y) / CGFloat(last.t - $0.t) } ?? 0
        let profile = speeds.enumerated().filter { $0.offset % 4 == 0 }.map { String(format: "%.0f", $0.element.v) }
        print(String(
            format: "動いた時間 %.0f ms、距離 %.0f pt、最高速度 %.0f pt/s（%.0f ms 時点）、最後の 50 ms の速度 %.0f pt/s",
            (last.t - first.t) * 1000, abs(last.y - first.y), peak?.v ?? 0, (peak?.t ?? 0) * 1000, tailSpeed))
        print("速度の推移（100 ms ごと）: " + profile.joined(separator: " ") + "\n")
        fflush(stdout)
    }
}

/// 自動スクロールでの計測結果。
final class ScrollMeasurement {
    let frames: Int
    let step: Double
    let completion: (String) -> Void
    var link: CADisplayLink?
    var frame = 0
    /// 1 フレーム分のスクロールと再描画にかかったメインスレッドの時間。
    var work: [CFTimeInterval] = []
    var latency: [CFTimeInterval] = []
    /// 入力（フレームの開始）から GPU が描き終えるまで。
    var gpuDone: [CFTimeInterval] = []
    var presented: [CFTimeInterval] = []

    init(frames: Int, step: Double, completion: @escaping (String) -> Void) {
        self.frames = frames
        self.step = step
        self.completion = completion
    }

    func summary(refresh: CFTimeInterval) -> String {
        func ms(_ values: [CFTimeInterval], _ p: Double) -> String {
            guard !values.isEmpty else { return "-" }
            let sorted = values.sorted()
            return String(format: "%.2f", sorted[min(sorted.count - 1, Int(Double(sorted.count) * p))] * 1000)
        }
        let gaps = zip(presented.dropFirst(), presented).map { $0 - $1 }
        let dropped = gaps.filter { $0 > refresh * 1.5 }.count
        return """
        frames: \(frames)  refresh: \(String(format: "%.2f", refresh * 1000)) ms
        main thread work  p50 \(ms(work, 0.5)) ms  p95 \(ms(work, 0.95)) ms  max \(ms(work, 1))
        to GPU done       p50 \(ms(gpuDone, 0.5)) ms  p95 \(ms(gpuDone, 0.95)) ms  max \(ms(gpuDone, 1))
        to presented      p50 \(ms(latency, 0.5)) ms  p95 \(ms(latency, 0.95)) ms  max \(ms(latency, 1))
        presented frames: \(presented.count)  dropped: \(dropped)
        """
    }
}

extension NSPasteboard.PasteboardType {
    static let commitID = NSPasteboard.PasteboardType("io.github.fukusaka.amonhen.commit")
}
