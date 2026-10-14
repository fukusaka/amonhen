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
        // 表示待ちのフレームを減らし、入力から表示までの遅れを 1 フレーム縮める。
        metalLayer.maximumDrawableCount = 2
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

final class HistoryViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    private let repository: Repository
    private var history: History?
    private let renderer: GraphRenderer
    private let scrollView = NSScrollView()
    private let tableView = NSTableView()
    private let graphView = GraphView()
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

    init(repository: Repository, renderer: GraphRenderer) {
        self.repository = repository
        self.renderer = renderer
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 900, height: 420))
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
        for (id, title, width) in [("subject", "Message", 520.0), ("author", "Author", 140.0), ("date", "Date", 140.0)] {
            let column = NSTableColumn(identifier: .init(id))
            column.title = title
            column.width = width
            tableView.addTableColumn(column)
        }
        tableView.dataSource = self
        tableView.delegate = self
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
        scopePopup.target = self
        scopePopup.action = #selector(scopeChanged)
        scopePopup.controlSize = .small
        updateScopePopup()
        container.addSubview(scrollView)
        container.addSubview(graphView)
        container.addSubview(scopePopup)
        view = container
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        // 上に範囲の切り替え、その下に表。
        let barHeight: CGFloat = 26
        scopePopup.frame = NSRect(x: 6, y: view.bounds.height - barHeight + 2, width: 240, height: barHeight - 4)
        scrollView.frame = NSRect(x: 0, y: 0, width: view.bounds.width, height: view.bounds.height - barHeight)
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
        tableView.reloadData()
        if first { tableView.scrollRowToVisible(0) }
        redrawGraph()
    }

    /// グラフの列のうち、ヘッダーの下で見えている範囲に重ねる。
    private func layoutGraphView() {
        let clip = scrollView.contentView
        let clipFrame = view.convert(clip.frame, from: scrollView)
        let column = tableView.rect(ofColumn: tableView.column(withIdentifier: graphColumn.identifier))
        let x = view.convert(column.origin, from: tableView).x
        let top = clip.contentInsets.top
        graphView.frame = NSRect(x: x, y: clipFrame.minY, width: column.width, height: max(0, clipFrame.height - top))
        redrawGraph()
    }

    @objc private func contentDidScroll() {
        layoutGraphView()
    }

    private func redrawGraph() {
        guard let history else { return }
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
                if lower < upper {
                    let overlap = lower..<upper
                    let columns = IndexSet(integersIn: 1..<self.tableView.numberOfColumns)
                    self.tableView.reloadData(forRowIndexes: IndexSet(integersIn: overlap), columnIndexes: columns)
                }
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

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .short
        return f
    }()

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let id = tableColumn?.identifier, id != graphColumn.identifier else { return nil }
        let chunk = chunk(row: row)
        let field = tableView.makeView(withIdentifier: id, owner: nil) as? NSTextField ?? {
            let f = NSTextField(labelWithString: "")
            f.identifier = id
            f.lineBreakMode = .byTruncatingTail
            f.cell?.truncatesLastVisibleLine = true
            if id.rawValue != "subject" { f.textColor = .secondaryLabelColor }
            return f
        }()
        // 表のセルは文字列を持ち続けるので、コアのメモリを指したままの CFString は渡さずコピーする。
        // 見えている行の分だけ。差分表示と同じく Metal で文字を描くようになったら置き換える。
        switch (id.rawValue, chunk) {
        case (_, nil): field.stringValue = ""
        case ("subject", let chunk?): field.stringValue = chunk.subject(row: row)
        case ("author", let chunk?): field.stringValue = chunk.authorName(row: row)
        case (_, let chunk?): field.stringValue = Self.dateFormatter.string(from: chunk.authorTime(row: row))
        }
        return field
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
