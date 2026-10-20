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

    /// 画面の倍率が変わった（1 倍と 2 倍の画面のあいだでウィンドウを動かした）。並べ直して描き直してもらう。
    var onScaleChange: (() -> Void)?
    /// 最後に知らせた倍率。metalLayer はこのビューの層なので、AppKit が super の中で contentsScale を先に
    /// 書き換えることがあり、それと比べると変わったことを見落とす（差分の文字が前の倍率のまま崩れていた）。
    private var notifiedScale: CGFloat?

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        let scale = window?.backingScaleFactor ?? 2
        metalLayer.contentsScale = scale
        updateDrawableSize()
        if let notifiedScale, notifiedScale != scale { onScaleChange?() }
        notifiedScale = scale
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

final class HistoryViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate,
    NSSearchFieldDelegate
{
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
    /// 裏で並べておいた行（アトラスに入れる前）。画面に出すときに `rowText` に移す。
    private var preparedRows: [Int: RowPreparation.Row] = [:]
    private var rowFonts: RowPreparation.Fonts { RowPreparation.Fonts(subject: font, meta: metaFont) }
    private var ellipsis: TextRenderer.ShapedLine?
    private var metaEllipsis: TextRenderer.ShapedLine?
    private let font = CTFontCreateUIFontForLanguage(.system, NSFont.systemFontSize, nil)!
    // 数字の幅をそろえ、1 行表示で右に寄せた日時の桁が縦にそろうようにする。
    private let metaFont = NSFont.monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular) as CTFont
    private let labelFont = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize, weight: .medium) as CTFont

    /// コミットに付いている ref のラベル（ブランチ・リモートブランチ・タグ）。
    struct RefLabel {
        let name: String
        let kind: RefKind
        /// 今のブランチ。
        let isHead: Bool
        /// worktree の HEAD の札（名前と変更の数）。
        var worktree = false
        /// ほかの worktree が checkout しているブランチ。
        var elsewhere = false
    }
    private var lastRefs: Refs?
    /// 今のブランチ（書き込む先の worktree のもの。nil なら開いたリポジトリの HEAD）と、ほかの worktree が
    /// checkout しているブランチ。
    private var editingBranch: String??
    private var branchesElsewhere: Set<String> = []

    func setWorktreeBranches(editing: String??, elsewhere: Set<String>) {
        guard editing != editingBranch || elsewhere != branchesElsewhere else { return }
        editingBranch = editing
        branchesElsewhere = elsewhere
        if let lastRefs { show(refs: lastRefs) }
    }
    private var refLabels: [Data: [RefLabel]] = [:]
    /// 各 worktree の HEAD の札（ref の札の前に出す。docs/ux.md の「Worktrees」）。
    private var worktreeLabels: [Data: [RefLabel]] = [:]

    /// worktree ごとの HEAD（コミットの ID）と札の文字。書き込む先は `isHead`。
    func show(worktreeHeads: [(head: Data, name: String, editing: Bool)]) {
        var labels: [Data: [RefLabel]] = [:]
        for w in worktreeHeads {
            labels[w.head, default: []].append(RefLabel(name: w.name, kind: .local, isHead: w.editing, worktree: true))
        }
        guard labels.mapValues({ $0.map(\.name) }) != worktreeLabels.mapValues({ $0.map(\.name) }) else { return }
        worktreeLabels = labels
        redrawText()
    }
    /// 署名の検証の結果（コミット ID → `%G?` の 1 文字）。コミットは変わらないので、History を読み直しても残す。
    private var verifiedSignatures: [Data: SignatureStatus] = [:]
    private var verifyingSignatures: Set<Data> = []
    private var verifyWork: DispatchWorkItem?
    /// 検証する側を変えた回数。変える前に始めた検証の結果は捨てる。
    private var signatureGeneration = 0
    /// GitHub の CI の状態（コミットの ID → 状態）。RepositoryWindow が gh で読んで渡す。
    var ciStatus: [Data: PullRequest.CI] = [:] {
        didSet { if ciStatus != oldValue { redrawText() } }
    }

    /// 検証する側が変わったので、覚えていた結果を捨てて検証し直す（結果を引き継がない）。
    func forgetSignatures() {
        signatureGeneration += 1
        verifyWork?.cancel()
        verifiedSignatures.removeAll()
        verifyingSignatures.removeAll()
        redrawText()
    }
    private var labelText: [String: TextRenderer.ShapedLine] = [:]
    private let graphColumn = NSTableColumn(identifier: .init("graph"))
    private var chunks: [Int: HistoryChunk] = [:]
    /// 裏で読んでいるチャンク。
    private var loadingChunks: Set<Int> = []
    private static let chunkRows = 256
    /// 覚えておくチャンクの数（約 6,000 行）。超えたら、見えている所から遠いものから捨てる。チャンクは
    /// コミットのオブジェクトの中身を指しているので、linux を上から下まで見て全部持つと数 GB になる。
    private static let maxChunks = 24

    /// 入力イベントから画面に表示されるまでの時間（秒）。
    var onLatency: ((CFTimeInterval) -> Void)?
    var actions: Actions?

    /// 表示する範囲。
    enum Scope: Equatable {
        case all
        case current
        /// サイドバーで選んだ ref（種類, 名前, コミット）。ref が動いたら、コミットを付け替える。
        case ref(RefKind, String, Data)
        /// サイドバーで複数選んだ ref。それぞれから辿れるコミットをまとめて出す。
        case refs([Ref])
    }

    /// `Scope.refs` の 1 つ。
    struct Ref: Equatable {
        let kind: RefKind
        let name: String
        var target: Data
    }

    private(set) var scope: Scope = .all
    private let scopePopup = NSPopUpButton()
    /// 折りたたんだ History で開いた merge（コミット ID）。
    private var expandedMerges: Set<Data> = []
    /// 右端の密度の帯（コミットの頻度とブランチの活動量。押すとそこへ飛ぶ）。
    private let densityView = DensityStripView()
    private var densityRequest = 0
    /// 全ブランチをそのまま出すか（折りたたみも絞り込みもしない）。
    private var showsFullHistory: Bool { scope == .all && !Settings.foldMergedBranches }

    // 検索
    private let searchField = NSSearchField()
    private let searchCount = NSTextField(labelWithString: "")
    private let searchSpinner = NSProgressIndicator()
    /// 初めて読み込むまでの間、表の真ん中に出す（daemon 越しでは数十 MB を受け取るので数秒かかる）。
    private let loadingLabel = NSTextField(labelWithString: "")
    private let loadingSpinner = NSProgressIndicator()
    /// 結果を出した問い合わせと、合ったコミットの行（昇順）。
    private var searchQuery: String? {
        didSet { if searchQuery != oldValue { onSearchChanged?(searchQuery) } }
    }
    /// 検索の問い合わせが決まった・消えたとき（詳細の欄で一致を強調する）。
    var onSearchChanged: ((String?) -> Void)?
    private var searchRows: [Int] = []
    private var searchRowSet: Set<Int> = []
    private var searchIndex: Int?
    private var searchTask: Task<Void, Never>?
    /// 検索の前・次の一致へ移るボタンと、一致したコミットだけを出す切り替え。
    private let previousMatchButton = NSButton()
    private let nextMatchButton = NSButton()
    private let matchOnlyButton = NSButton()
    /// 一致したコミットだけを出しているときの、元の History（出していなければ nil）。
    private var baseHistory: History?
    /// HEAD のコミット（Current Branch で使う）。
    var head: Data? {
        // ほかのプログラムで commit・rebase すると HEAD が動く。今のブランチに絞っていれば絞り込み直す。
        didSet { if head != oldValue, scope == .current, history != nil { scheduleReloadScope() } }
    }
    /// 選んだコミット（20 バイト）。選択がなくなれば nil。
    var onSelectCommit: ((Data?) -> Void)?
    /// 複数の行を選んだ（⇧・⌘ クリック）。まとめた差分を見せる。
    var onSelectRows: (([Int], History) -> Void)?

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
        applyRowStyle()
        NotificationCenter.default.addObserver(
            self, selector: #selector(rowStyleChanged), name: .historyRowsChanged, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(highlightAncestorsChanged), name: .highlightAncestorsChanged, object: nil)
        tableView.intercellSpacing = NSSize(width: 6, height: 0)
        tableView.usesAlternatingRowBackgroundColors = false
        tableView.allowsTypeSelect = false
        // ⇧・⌘ クリックで複数のコミットを選び、まとめた差分を見る。
        tableView.allowsMultipleSelection = true
        tableView.style = .plain
        tableView.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        graphColumn.title = ""
        graphColumn.width = 120
        tableView.addTableColumn(graphColumn)
        let subject = NSTableColumn(identifier: .init("subject"))
        subject.title = L("Message")
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
        // 画面の倍率が変わったら、倍率込みで並べた行を捨てて描き直す。
        graphView.onScaleChange = { [weak self] in self?.redrawGraph() }
        textView.onScaleChange = { [weak self] in
            guard let self else { return }
            rowText.removeAll()
            labelText.removeAll()
            redrawText()
        }
        scopePopup.target = self
        scopePopup.action = #selector(scopeChanged)
        scopePopup.controlSize = .small
        updateScopePopup()
        searchField.placeholderString = L("Search (author:, path:, contents: also work)")
        searchField.toolTip = L("Words match the subject, author and ID. Spaces mean AND, “or” means OR, and parentheses group.\nauthor:name  subject:text  commit:ID  path:path  contents:\"diff text\"\nExample: path:fs/ (author:linus or author:greg)")
        searchField.controlSize = .small
        searchField.sendsWholeSearchString = true
        searchField.delegate = self
        searchField.target = self
        searchField.action = #selector(searchSubmitted)
        searchCount.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        searchCount.textColor = .secondaryLabelColor
        searchCount.alignment = .right
        searchSpinner.style = .spinning
        searchSpinner.controlSize = .small
        searchSpinner.isDisplayedWhenStopped = false
        for (button, symbol, tip, action) in [
            (previousMatchButton, "chevron.up", L("Previous Match (⇧↩, ⌘⇧G)"), #selector(previousMatchClicked)),
            (nextMatchButton, "chevron.down", L("Next Match (↩, ⌘G)"), #selector(nextMatchClicked)),
            (matchOnlyButton, "line.3.horizontal.decrease.circle", L("Show Only Matching Commits"), #selector(matchOnlyClicked)),
        ] {
            button.bezelStyle = .accessoryBarAction
            button.isBordered = false
            button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: tip)
            button.toolTip = tip
            button.target = self
            button.action = action
            button.controlSize = .small
            button.isEnabled = false
            container.addSubview(button)
        }
        matchOnlyButton.setButtonType(.pushOnPushOff)
        container.addSubview(searchField)
        container.addSubview(searchCount)
        container.addSubview(searchSpinner)
        container.addSubview(scrollView)
        container.addSubview(graphView)
        container.addSubview(textView)
        container.addSubview(scopePopup)
        densityView.onJump = { [weak self] fraction in
            guard let self, let history else { return }
            let row = min(history.rowCount - 1, max(0, Int(fraction * CGFloat(history.rowCount))))
            let clip = scrollView.contentView
            let y = max(0, CGFloat(row) * tableView.rowHeight - clip.bounds.height / 2)
            clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: min(y, max(0, tableView.frame.height - clip.bounds.height))))
            scrollView.reflectScrolledClipView(clip)
        }
        densityView.toolTip = L("Commit frequency (left) and branch activity (right). Click to jump there.")
        container.addSubview(densityView)
        NotificationCenter.default.addObserver(
            self, selector: #selector(foldChanged), name: .foldMergedBranchesChanged, object: nil)
        loadingLabel.font = .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        loadingLabel.textColor = .secondaryLabelColor
        loadingLabel.alignment = .center
        loadingSpinner.style = .spinning
        loadingSpinner.controlSize = .small
        loadingSpinner.isDisplayedWhenStopped = false
        loadingLabel.isHidden = true
        container.addSubview(loadingLabel)
        container.addSubview(loadingSpinner)
        view = container
    }

    /// ウィンドウのツールバーに検索欄が出ているか。出ていれば、ここの検索欄は隠す（同じ検索の欄を 2 つ並べない）。
    /// 一致の件数・前・次・絞り込みはここに残す。
    var toolbarSearchShown: (() -> Bool)?
    /// ツールバーの検索欄に移る（ここの検索欄を隠しているとき）。
    var focusToolbarSearch: (() -> Void)?

    override func viewDidLayout() {
        super.viewDidLayout()
        // 上に範囲の切り替え、その下に表。
        let barHeight: CGFloat = 26
        let barY = view.bounds.height - barHeight + 2
        scopePopup.frame = NSRect(x: 6, y: barY, width: 180, height: barHeight - 4)
        let countWidth: CGFloat = 96
        // 右から、絞り込み・次・前・件数。
        let buttons: CGFloat = 3 * 22
        let ownField = toolbarSearchShown?() != true
        searchField.isHidden = !ownField
        searchField.frame = NSRect(
            x: 192, y: barY, width: ownField ? max(80, view.bounds.width - 192 - countWidth - buttons - 12) : 0,
            height: barHeight - 4)
        searchSpinner.frame = NSRect(x: (ownField ? searchField.frame.maxX : 186) + 6, y: barY + 3, width: 16, height: 16)
        let countX = view.bounds.width - countWidth - buttons - 6
        searchCount.frame = NSRect(x: countX, y: barY + 2, width: countWidth, height: 16)
        for (i, button) in [previousMatchButton, nextMatchButton, matchOnlyButton].enumerated() {
            button.frame = NSRect(x: countX + countWidth + CGFloat(i) * 22, y: barY, width: 22, height: barHeight - 4)
        }
        // 右端に密度の帯（スクロールバーの左）。
        let stripWidth: CGFloat = 8
        scrollView.frame = NSRect(x: 0, y: 0, width: view.bounds.width - stripWidth, height: view.bounds.height - barHeight)
        let oldHeight = densityView.frame.height
        densityView.frame = NSRect(x: view.bounds.width - stripWidth, y: 0, width: stripWidth, height: scrollView.frame.height)
        if Int(oldHeight) != Int(densityView.frame.height) { updateDensity() }
        // 件名の列を表の幅いっぱいにする。
        tableView.sizeLastColumnToFit()
        layoutGraphView()
        let mid = scrollView.frame.midY
        loadingSpinner.frame = NSRect(x: scrollView.frame.midX - 8, y: mid + 4, width: 16, height: 16)
        loadingLabel.frame = NSRect(x: 0, y: mid - 22, width: scrollView.frame.width, height: 20)
    }

    /// 読み込み中の知らせ（nil で消す）。
    var loadingMessage: String? {
        get { loadingLabel.isHidden ? nil : loadingLabel.stringValue }
        set {
            loadViewIfNeeded()
            loadingLabel.stringValue = newValue ?? ""
            loadingLabel.isHidden = newValue == nil
            if newValue == nil { loadingSpinner.stopAnimation(nil) } else { loadingSpinner.startAnimation(nil) }
        }
    }

    private func updateScopePopup() {
        scopePopup.removeAllItems()
        scopePopup.addItems(withTitles: [L("All Branches"), L("Current Branch")])
        switch scope {
        case let .ref(_, name, _):
            scopePopup.addItem(withTitle: name)
            scopePopup.selectItem(at: 2)
        case let .refs(list):
            scopePopup.addItem(withTitle: L("%ld branches", list.count))
            scopePopup.selectItem(at: 2)
        default:
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

    /// `.ref` の範囲に一緒に出すコミット（選んだブランチの上流。先にある・遅れているコミットを両方見せる）。
    private var companionTips: [Data] = []

    /// 表示する範囲を変えて読み直す。`companions` は `.ref` のときに一緒に辿る起点（ブランチの上流）。
    func set(scope: Scope, select commit: Data? = nil, companions: [Data] = []) {
        self.scope = scope
        companionTips = companions
        updateScopePopup()
        reloadScope(select: commit)
    }

    private var scopeWork: DispatchWorkItem?

    /// ほかのプログラムの変化で絞り込み直す。History の読み直しと ref の読み直し（起点の付け替え）が続けて
    /// 来るので、少し待って 1 回にまとめる（2 回出し直すと、途中の古い起点の一覧が一瞬見える）。
    private func scheduleReloadScope() {
        scopeWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                self?.scopeWork = nil
                self?.reloadScope(resetScroll: false)
            }
        }
        scopeWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1, execute: work)
    }

    private func reloadScope(select commit: Data? = nil, resetScroll: Bool = true) {
        scopeWork?.cancel()
        scopeWork = nil
        let repository = self.repository
        let tips: [Data]? = switch scope {
        case .all: nil
        case .current: head.map { [$0] }
        case let .ref(_, _, target): [target] + companionTips
        case let .refs(list): list.map(\.target)
        }
        let fold = Settings.foldMergedBranches, expanded = Array(expandedMerges)
        Task {
            let next: History? = await Task.detached(priority: .userInitiated) {
                if fold { return try? repository.foldedHistory(tips: tips, expanded: expanded) }
                return if let tips { try? repository.filteredHistory(tips: tips) } else { repository.latestHistory() }
            }.value
            // 起点がもう History にない（ほかのプログラムの rebase で消えた HEAD を、ref を読み直す前に使った）と
            // 空になる。空にはせず今の表示を残し、ref を読み直して起点を付け替えたときに出し直す。
            guard let next, next.rowCount > 0 || repository.latestHistory()?.rowCount == 0 else { return }
            showBase(next, resetScroll: resetScroll)
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
                if !showsFullHistory {
                    reloadScope()
                } else if latest.generation != (baseHistory ?? history)?.generation {
                    showBase(latest)
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
            if !showsFullHistory {
                scheduleReloadScope()
            } else if latest.generation != (baseHistory ?? history)?.generation {
                showBase(latest)
            }
        }
    }

    /// History を最初から作り直して表示し直す（永続キャッシュを捨てる。共有の daemon 越しなら daemon の分も）。
    func rebuild(completion: @escaping (Error?) -> Void) {
        let repository = self.repository
        Task {
            do {
                let latest = try await Task.detached(priority: .userInitiated) { try repository.rebuildHistory() }.value
                if showsFullHistory { showBase(latest) } else { reloadScope() }
                completion(nil)
            } catch {
                completion(error)
            }
        }
    }

    /// 出し直しの番号（見えている行を読んでいる間に次の出し直しが来たら、前のは捨てる）。
    private var presentRequest = 0

    /// 読み込み済みのチャンクとその行の文字。
    private typealias Preloaded = [(index: Int, chunk: HistoryChunk, prepared: [Int: RowPreparation.Row])]

    /// 新しい History を、見えている行の文字を読んでから出す。読むまでは前の表示を残すので、入れ替えの間に
    /// 文字が空になる瞬間が出ない（ほかのプログラムの変化で読み直したときに、一覧がちらつかない）。
    private func present(_ next: History, resetScroll: Bool) {
        presentRequest += 1
        let request = presentRequest
        guard history != nil, next.rowCount > 0 else { return show(next, resetScroll: resetScroll) }
        let visible = tableView.rows(in: tableView.visibleRect)
        let top = resetScroll ? 0 : visible.location
        let first = top / Self.chunkRows
        let last = max(first, (top + max(visible.length, 1) - 1) / Self.chunkRows)
        let indices = (first...last).filter { $0 * Self.chunkRows < next.rowCount }
        let fonts = rowFonts, compact = Settings.compactHistoryRows, chunkRows = Self.chunkRows
        Task {
            let loaded: Preloaded = await Task.detached(priority: .userInitiated) {
                indices.compactMap { index in
                    let rows = index * chunkRows..<min(next.rowCount, (index + 1) * chunkRows)
                    let (chunk, prepared) = Self.loadChunk(rows: rows, of: next, fonts: fonts, compact: compact)
                    return chunk.map { (index, $0, prepared) }
                }
            }.value
            guard request == presentRequest else { return }
            show(next, resetScroll: resetScroll, preloaded: compact == Settings.compactHistoryRows ? loaded : [])
        }
    }

    /// 元になる History を出す。一致したコミットだけを出しているなら、新しい History で探し直して絞り込み直す
    /// （それまでは今の絞り込みを出しておく）。
    private func showBase(_ next: History, resetScroll: Bool = false) {
        guard baseHistory != nil, matchOnlyButton.state == .on, let query = searchQuery else {
            baseHistory = nil
            present(next, resetScroll: resetScroll)
            return
        }
        baseHistory = next
        runSearch(query, in: next)
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

    private func show(_ history: History, resetScroll: Bool = false, preloaded: Preloaded = []) {
        // ウィンドウを組み立てる前にキャッシュから表示することがある。表がまだなければ先に作る。
        loadViewIfNeeded()
        presentRequest += 1
        let first = self.history == nil || resetScroll
        // 前の表示を、新しい表示へ短く溶かして入れ替える（ほかのプログラムの変化で読み直したとき。自分で範囲を
        // 変えたときは待たせない）。グラフと文字は Core Animation のトランザクションの中で出すので、一緒に溶ける。
        if !first, history.generation != self.history?.generation, let layer = view.layer {
            let fade = CATransition()
            fade.type = .fade
            fade.duration = 0.15
            layer.add(fade, forKey: "refresh")
        }
        // 選んでいたコミット（行番号ではなく ID で覚え、新しい History でも同じコミットを選ぶ）。
        let selected: [(id: Data, row: Int)] = self.history.map { old in
            tableView.selectedRowIndexes.compactMap { row in old.commitID(row: row).map { ($0, row) } }
        } ?? []
        // 選び直しの途中（消えたコミットの選択を外し、先頭を選ぶ）は詳細に知らせない。知らせると差分が
        // 「選んでいない」を挟んで読み直され、ちらつく。最後に、選んだコミットが変わったときだけ知らせる。
        quietSelection = true
        defer {
            quietSelection = false
            let now = tableView.selectedRowIndexes.compactMap { history.commitID(row: $0) }
            if now != selected.map(\.id) { notifySelection() }
        }
        self.history = history
        chunks.removeAll()
        loadingChunks.removeAll()
        rowText.removeAll()
        preparedRows.removeAll()
        for item in preloaded {
            chunks[item.index] = item.chunk
            preparedRows.merge(item.prepared) { new, _ in new }
        }
        tableView.reloadData()
        // 行が変わるので、検索の結果は捨てる（問い合わせは残し、Enter で探し直せる）。表を読み直してから描き直す。
        clearSearchResults()
        if first { tableView.scrollRowToVisible(0) }
        // 表は行番号の選択を引き継ぐが、上にコミットが増えると同じ行に別のコミットが来る（知らせも出ないので、
        // 詳細が前のコミットのままになる）。選んでいたコミットを新しい行で選び直す。
        if !selected.isEmpty {
            let rows = IndexSet(selected.compactMap { Self.locate($0.id, near: $0.row, in: history) })
            if rows != tableView.selectedRowIndexes {
                tableView.selectRowIndexes(rows, byExtendingSelection: false)
            }
        }
        // 何も選んでいなければ先頭（いちばん新しいコミット）を選び、詳細が空のままにならないようにする。
        if tableView.selectedRow < 0, history.rowCount > 0 {
            tableView.selectRowIndexes([0], byExtendingSelection: false)
        }
        updateLineage()
        redrawGraph()
        updateDensity()
    }

    /// 密度の帯を、今の History で描き直す（裏で求める。linux で数 ms）。
    private func updateDensity() {
        densityRequest += 1
        let request = densityRequest
        guard let history else { return }
        // 1 区間に 25 行以上入るようにする（少ないと値がばらついて縞になる）。高さ 2 pt につき 1 区間まで。
        let buckets = max(1, min(Int(densityView.bounds.height / 2), history.rowCount / 25))
        Task {
            let density = await Task.detached(priority: .utility) { history.density(buckets: buckets) }.value
            guard request == densityRequest else { return }
            densityView.density = density
            updateDensityViewport()
        }
    }

    /// 帯の上に、今見えている範囲を出す。
    private func updateDensityViewport() {
        let rows = max(1, history?.rowCount ?? 1)
        let visible = tableView.rows(in: tableView.visibleRect)
        densityView.visible = (CGFloat(visible.location) / CGFloat(rows), CGFloat(visible.location + visible.length) / CGFloat(rows))
    }

    @objc private func foldChanged() {
        expandedMerges.removeAll()
        reloadScope()
    }

    /// 折りたたんだ merge を開く・たたむ。
    private func toggleMerge(_ id: Data) {
        if expandedMerges.remove(id) == nil { expandedMerges.insert(id) }
        reloadScope(select: id)
    }

    /// `id` の新しい行。たいていは前の行か、上に増えた分だけ下にずれた所にあるので、近くから探す（History 全体の
    /// 索引は、linux で作るのに 80 ms かかる）。見つからなければ索引で引く。消えていれば nil。
    private static func locate(_ id: Data, near row: Int, in history: History) -> Int? {
        let end = min(history.rowCount, row + 256)
        if row < end, let found = (row..<end).first(where: { history.commitID(row: $0) == id }) { return found }
        return history.row(of: id)
    }

    /// 頼んだ祖先の計算の回数（古い結果を捨てるため）。
    private var lineageRequest = 0

    /// 選んだコミットの祖先を裏で求め、求まったらグラフを描き直す（linux で 10〜25 ms）。
    private func updateLineage() {
        lineageRequest += 1
        let request = lineageRequest
        let row = tableView.selectedRow
        guard Settings.highlightAncestors, let history, row >= 0, row < history.rowCount else {
            if renderer.lineage != nil {
                renderer.lineage = nil
                redrawGraph()
            }
            return
        }
        if let current = renderer.lineage, current.history === history, current.row == row { return }
        Task {
            let lineage = await Task.detached(priority: .userInitiated) { history.lineage(of: row) }.value
            guard request == lineageRequest else { return }
            renderer.lineage = lineage
            redrawGraph()
        }
    }

    @objc private func highlightAncestorsChanged() {
        updateLineage()
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
        FrameProfile.measure("text: build") { buildText(scale: textView.metalLayer.contentsScale) }
        FrameProfile.measure("text: submit") { text.draw(layer: textView.metalLayer, background: .zero) }
    }

    /// 1 行・2 行の表示に合わせて、行の高さ（表とグラフで共通）を決める。
    private func applyRowStyle() {
        renderer.metrics.rowHeight = Settings.compactHistoryRows ? 24 : 38
        tableView.rowHeight = CGFloat(renderer.metrics.rowHeight)
    }

    @objc private func rowStyleChanged() {
        // 見ていたコミットが同じ位置に来るように、選んだ行を見える位置に戻す。
        let selected = tableView.selectedRow
        applyRowStyle()
        rowText.removeAll()
        // 並べ直しが要るので、行の情報は読み直す（裏で並べ直される）。
        preparedRows.removeAll()
        chunks.removeAll()
        loadingChunks.removeAll()
        tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0..<tableView.numberOfRows))
        if selected >= 0 { select(row: selected) }
        layoutGraphView()
    }

    /// ref の一覧が変わった。各コミットのラベルを作り直して描き直す。
    func show(refs: Refs) {
        lastRefs = refs
        // 絞り込んだ ref がほかのプログラムで動いた（rebase・commit・fetch）。前のコミットのままだと、消えた
        // コミットから辿ることになり空になるので、新しいコミットに付け替えて絞り込み直す。
        if case let .ref(kind, name, target) = scope,
           let now = refs.items.first(where: { $0.kind == kind && $0.name == name })?.target, now != target {
            scope = .ref(kind, name, now)
            scheduleReloadScope()
        }
        if case let .refs(list) = scope {
            let moved = list.map { ref in
                var ref = ref
                if let now = refs.items.first(where: { $0.kind == ref.kind && $0.name == ref.name })?.target { ref.target = now }
                return ref
            }
            if moved != list {
                scope = .refs(moved)
                scheduleReloadScope()
            }
        }
        let head = editingBranch ?? refs.headBranch
        var labels: [Data: [RefLabel]] = [:]
        for item in refs.items {
            guard let target = item.target, [.local, .remote, .tag].contains(item.kind) else { continue }
            labels[target, default: []].append(
                RefLabel(name: item.name, kind: item.kind, isHead: item.kind == .local && item.name == head,
                         elsewhere: item.kind == .local && branchesElsewhere.contains(item.name)))
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
        // 2 行なら行の高さの中ほどに 2 行を、1 行なら件名と同じ高さに作者・日時を置く。
        let compact = Settings.compactHistoryRows
        let subjectHeight = CTFontGetAscent(font) + CTFontGetDescent(font)
        let metaHeight = CTFontGetAscent(metaFont) + CTFontGetDescent(metaFont)
        let gap: CGFloat = 2
        let top = ((tableView.rowHeight - subjectHeight - (compact ? 0 : gap + metaHeight)) / 2).rounded()
        let subjectBaseline = top + CTFontGetAscent(font)
        let metaBaseline = compact
            ? subjectBaseline
            : (subjectBaseline + CTFontGetDescent(font) + gap + CTFontGetAscent(metaFont)).rounded()
        let focused = view.window?.isKeyWindow == true && view.window?.firstResponder === tableView
        let primary = NSColor.labelColor.rgba, secondary = NSColor.secondaryLabelColor.rgba
        let selected = (focused ? NSColor.alternateSelectedControlTextColor : NSColor.labelColor).rgba
        let selectedSecondary = (focused ? NSColor.alternateSelectedControlTextColor.withAlphaComponent(0.75)
            : NSColor.secondaryLabelColor).rgba
        let selection = tableView.selectedRowIndexes
        if rowText.count > 4096 { rowText.removeAll() }
        var unverified: [Data] = []
        // History を差し替えた直後（表を読み直す前）は、表が前の History の行を返すことがある（絞り込みで行が
        // 減ったときは、見えている行が全部新しい History の外）。
        let last = min(history.rowCount, rows.location + rows.length)
        guard rows.location < last else { return }
        for row in rows.location..<last {
            guard let line = FrameProfile.measure("text: row text", { shapedRow(row, scale: scale) }) else { continue }
            let rowTop = tableView.rect(ofRow: row).minY - scrollY
            let isSelected = selection.contains(row)
            if !isSelected, searchRowSet.contains(row) {
                text.addRoundedRect(
                    CGRect(x: column.minX - originX - 3, y: rowTop + 1, width: column.width + 6, height: tableView.rowHeight - 2),
                    radius: 4, color: NSColor.systemYellow.withAlphaComponent(0.14).rgba)
            }
            var x = column.minX - originX
            let end = column.maxX - originX
            let id = FrameProfile.measure("text: commit id (FFI)") { history.commitID(row: row) }
            let labels = id.flatMap { id -> [RefLabel]? in
                let all = (worktreeLabels[id] ?? []) + (refLabels[id] ?? [])
                return all.isEmpty ? nil : all
            }
            if let labels {
                x = drawLabels(labels, x: x, maxX: end - 40, baseline: rowTop + subjectBaseline, scale: scale,
                               selected: isSelected && focused)
            }
            // 署名があれば、作者・日時の行に検証の結果の色の丸を置く。結果をまだ知らなければ、裏で検証する。
            let kind = chunks[row / Self.chunkRows]?.signature(row: row) ?? 0
            let signed = kind != 0
            var status = SignatureStatus.pending
            if signed, let id {
                if let known = verifiedSignatures[id] {
                    status = known
                } else if !verifyingSignatures.contains(id) {
                    unverified.append(id)
                }
            }
            // GitHub の CI の状態（知っていれば）。行の右端に置き、署名の印とは離す。
            let ci = id.flatMap { ciStatus[$0] }
            let ciSpace: CGFloat = ci != nil ? 18 : 0
            let right = end - ciSpace
            let dot: CGFloat = signed ? 12 : 0
            // 1 行なら、作者・日時・ID を右に寄せ、件名はその手前まで。
            let metaWidth = compact ? min(line.meta.width, column.width * 0.45) : 0
            let subjectEnd = compact ? right - metaWidth - 12 - dot : right
            text.addLine(
                line.subject, origin: CGPoint(x: x, y: rowTop + subjectBaseline), color: isSelected ? selected : primary,
                scale: scale, maxWidth: max(0, subjectEnd - x), ellipsis: ellipsis)
            text.addLine(
                line.meta, origin: CGPoint(x: compact ? right - metaWidth : column.minX - originX, y: rowTop + metaBaseline),
                color: isSelected ? selectedSecondary : secondary, scale: scale,
                maxWidth: compact ? metaWidth : column.width - dot - ciSpace, ellipsis: metaEllipsis)
            if signed {
                // 署名の状態のアイコン（信用度は形で分かる。塗りつぶしの seal は強い信用）。
                let centerX = compact
                    ? right - metaWidth - 7
                    : column.minX - originX + min(line.meta.width, column.width - dot - ciSpace) + 9
                let centerY = rowTop + metaBaseline - CTFontGetXHeight(metaFont) / 2
                text.addSymbol(
                    status.symbol, pointSize: 9.5, center: CGPoint(x: centerX, y: centerY),
                    color: (isSelected && focused ? selected : status.color.rgba), scale: scale)
            }
            if let ci {
                // 枠のないチェック・バツ・点（署名のバッジと形で見分ける）。
                let (symbol, color, size): (String, NSColor, CGFloat) = switch ci {
                case .passing: ("checkmark", .systemGreen, 10)
                case .failing: ("xmark", .systemRed, 10)
                case .pending: ("circle.fill", .systemOrange, 6)
                case .none: ("circle", .secondaryLabelColor, 6)
                }
                text.addSymbol(
                    symbol, pointSize: size, center: CGPoint(x: end - 9, y: rowTop + tableView.rowHeight / 2),
                    color: (isSelected && focused ? selected : color.rgba), scale: scale)
            }
        }
        if !unverified.isEmpty { verifySoon(unverified) }
    }

    /// スクロールが落ち着いてから、見えている署名を 8 件ずつ裏で検証し、終わった分から色を付ける
    /// （gpg を 1 件ずつ起動するので 1 件 15 ms ほどかかる。描画は覚えた結果を読むだけで待たない）。
    private func verifySoon(_ ids: [Data]) {
        verifyWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self else { return }
            let pending = ids.filter { self.verifiedSignatures[$0] == nil && !self.verifyingSignatures.contains($0) }
            guard !pending.isEmpty else { return }
            verifyingSignatures.formUnion(pending)
            let repository = self.repository
            let generation = signatureGeneration
            Task {
                for start in stride(from: 0, to: pending.count, by: 8) {
                    let batch = Array(pending[start..<min(pending.count, start + 8)])
                    let hexes = batch.map { $0.map { String(format: "%02x", $0) }.joined() }
                    let checks = await Task.detached(priority: .utility) {
                        (try? repository.verifySignatures(hexes)) ?? []
                    }.value
                    guard generation == signatureGeneration else { return }
                    for (id, c) in zip(batch, checks) {
                        verifiedSignatures[id] = SignatureStatus(
                            status: c.status, trust: c.trust, emails: c.emails, committerEmail: c.committerEmail,
                            kind: c.signer.contains("<") ? "gpg" : (c.key.hasPrefix("SHA256:") ? "ssh" : "gpg"))
                    }
                    verifyingSignatures.subtract(batch)
                    redrawText()
                }
            }
        }
        verifyWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15, execute: work)
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
        if label.elsewhere {
            // ほかの worktree が checkout しているブランチ: worktree の札と同じ緑。
            return (NSColor.systemGreen.withAlphaComponent(selected ? 0.45 : 0.22).rgba,
                    (selected ? NSColor.white : NSColor.labelColor).rgba)
        }
        if label.worktree {
            let base = NSColor.systemGreen
            if label.isHead { return (base.withAlphaComponent(0.85).rgba, NSColor.white.rgba) }
            return (base.withAlphaComponent(selected ? 0.45 : 0.25).rgba, (selected ? NSColor.white : NSColor.labelColor).rgba)
        }
        let base: NSColor = switch label.kind {
        case .local: .systemBlue
        case .remote: .systemPurple
        default: .systemOrange
        }
        if label.isHead { return (base.withAlphaComponent(0.85).rgba, NSColor.white.rgba) }
        return (base.withAlphaComponent(selected ? 0.45 : 0.22).rgba, (selected ? NSColor.white : NSColor.labelColor).rgba)
    }

    private func shapedRow(_ row: Int, scale: CGFloat) -> RowText? {
        if let cached = rowText[row] { return cached }
        // 裏で並べておいた行なら、アトラスからグリフを引くだけで済む。
        let prepared = preparedRows.removeValue(forKey: row) ?? chunk(row: row).map { chunk in
            RowPreparation.prepare(row: row, chunk: chunk, history: history, fonts: rowFonts, compact: Settings.compactHistoryRows)
        }
        guard let prepared else { return nil }
        let line = RowText(subject: text.shape(prepared.subject, scale: scale), meta: text.shape(prepared.meta, scale: scale))
        rowText[row] = line
        return line
    }

    @objc private func contentDidScroll() {
        motion?.add(y: scrollView.contentView.bounds.minY)
        layoutGraphView()
        updateDensityViewport()
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
        FrameProfile.measure("graph: total") {
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
    }

    // MARK: - 検索

    /// ⌘F（Edit の Find…）。表やこの画面にフォーカスがあるとき、responder chain でここに届く。
    @objc func showFindBar(_ sender: Any?) {
        if searchField.isHidden, let focusToolbarSearch { return focusToolbarSearch() }
        view.window?.makeFirstResponder(searchField)
        searchField.currentEditor()?.selectAll(nil)
    }

    /// 検索欄に入れて探す（`--search` の確認用）。
    func search(_ query: String) {
        searchField.stringValue = query
        searchSubmitted()
    }

    @objc func findNext(_ sender: Any?) { moveSearch(by: 1) }
    @objc func findPrevious(_ sender: Any?) { moveSearch(by: -1) }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard control === searchField else { return false }
        switch selector {
        case #selector(NSResponder.insertNewline(_:)) where NSEvent.modifierFlags.contains(.shift):
            moveSearch(by: -1)
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            searchField.stringValue = ""
            searchSubmitted()
            view.window?.makeFirstResponder(tableView)
            return true
        default:
            return false
        }
    }

    /// Enter で探す。同じ問い合わせなら次の一致へ進む。
    @objc private func searchSubmitted() {
        let query = searchField.stringValue.trimmingCharacters(in: .whitespaces)
        guard !query.isEmpty, let base = baseHistory ?? history else {
            searchTask?.cancel()
            repository.cancelSearch()
            searchQuery = nil
            // 一致だけを出していたら、元に戻す。
            if let base = baseHistory {
                baseHistory = nil
                show(base)
            }
            clearSearchResults()
            return
        }
        if query == searchQuery {
            moveSearch(by: 1)
            return
        }
        runSearch(query, in: base)
    }

    /// `base` で探す。一致だけを出す切り替えが入っていれば、一致したコミットだけの History にして出す。
    private func runSearch(_ query: String, in base: History) {
        searchTask?.cancel()
        searchSpinner.startAnimation(nil)
        searchCount.stringValue = ""
        let repository = self.repository
        let matchOnly = matchOnlyButton.state == .on
        searchTask = Task {
            let result = await Task.detached(priority: .userInitiated) { Result { try base.search(query) } }.value
            guard !Task.isCancelled, (self.baseHistory ?? self.history) === base else { return }
            let rows: [Int]
            switch result {
            case let .success(found):
                rows = found
            case let .failure(error):
                // 構文の誤りなどは件数の欄に出す（取り消しは新しい検索が始まっているので何もしない）。
                self.searchSpinner.stopAnimation(nil)
                self.searchCount.stringValue = "\(error)"
                self.searchCount.toolTip = "\(error)"
                return
            }
            if matchOnly, !rows.isEmpty {
                let filtered = await Task.detached(priority: .userInitiated) {
                    repository.matchingHistory(of: base, rows: rows)
                }.value
                guard !Task.isCancelled, (self.baseHistory ?? self.history) === base else { return }
                // 選んでいたコミットは、絞り込んだ History でも選んだままにする（`show` が ID で選び直す）。
                self.baseHistory = base
                self.show(filtered)
                self.searchQuery = query
                self.searchRows = Array(0..<filtered.rowCount)
                // 全部の行が一致なので、行に一致の色は付けない。
                self.searchRowSet = []
                self.searchIndex = max(0, self.tableView.selectedRow)
            } else {
                if let original = self.baseHistory {
                    self.baseHistory = nil
                    self.show(original)
                }
                self.searchQuery = query
                self.searchRows = rows
                self.searchRowSet = Set(rows)
                // 今選んでいる行から下で、いちばん近い一致へ。
                let from = max(0, self.tableView.selectedRow)
                self.searchIndex = rows.isEmpty ? nil : (rows.firstIndex { $0 >= from } ?? 0)
            }
            self.searchSpinner.stopAnimation(nil)
            self.showSearchResult()
        }
    }

    @objc private func previousMatchClicked() { moveSearch(by: -1) }
    @objc private func nextMatchClicked() { moveSearch(by: 1) }

    @objc private func matchOnlyClicked() {
        guard let query = searchQuery, let base = baseHistory ?? history else {
            updateMatchButtons()
            return
        }
        runSearch(query, in: base)
    }

    /// 前・次のボタンは一致があるときだけ、絞り込みは探したときだけ押せる。
    private func updateMatchButtons() {
        previousMatchButton.isEnabled = !searchRows.isEmpty
        nextMatchButton.isEnabled = !searchRows.isEmpty
        matchOnlyButton.isEnabled = searchQuery != nil || baseHistory != nil
        // 入っているときは塗りつぶしの印と強調の色で見せる。
        let on = matchOnlyButton.state == .on
        matchOnlyButton.image = NSImage(
            systemSymbolName: on ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle",
            accessibilityDescription: matchOnlyButton.toolTip)
        matchOnlyButton.contentTintColor = on ? .controlAccentColor : nil
    }

    /// History で探しているか（一致がなくても、問い合わせがあれば true）。
    var hasSearch: Bool { searchQuery != nil }

    /// 検索で一致したコミットの次・前へ（⌘G・⌘⇧G、ツールバーの検索欄の ⇧↩）。探していなければ false。
    @discardableResult
    func moveToMatch(by step: Int) -> Bool {
        guard searchQuery != nil else { return false }
        moveSearch(by: step)
        return true
    }

    private func moveSearch(by step: Int) {
        guard searchQuery != nil else {
            showFindBar(nil)
            return
        }
        guard !searchRows.isEmpty else { return NSSound.beep() }
        searchIndex = ((searchIndex ?? -step) + step + searchRows.count) % searchRows.count
        showSearchResult()
    }

    private func showSearchResult() {
        if let i = searchIndex {
            searchCount.stringValue = "\(i + 1) / \(searchRows.count)"
            select(row: searchRows[i])
        } else {
            searchCount.stringValue = searchQuery == nil ? "" : L("No Matches")
        }
        updateMatchButtons()
        redrawText()
    }

    private func clearSearchResults() {
        searchSpinner.stopAnimation(nil)
        searchRows = []
        searchRowSet = []
        searchIndex = nil
        searchQuery = nil
        searchCount.stringValue = ""
        updateMatchButtons()
        redrawText()
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
        FrameProfile.reset()
        FrameProfile.enabled = true
        m.link = view.displayLink(target: self, selector: #selector(measureFrame(_:)))
        m.link?.add(to: .main, forMode: .common)
    }

    @objc private func measureFrame(_ link: CADisplayLink) {
        guard let m = measurement else { return }
        if m.link == nil {
            link.invalidate()
            // 最後のフレームが表示されるのを待ってからまとめる。
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
                FrameProfile.enabled = false
                m.completion(m.summary(refresh: link.duration)
                    + "\nrenderer: drawn \(self.renderer.drawnFrames)  skipped \(self.renderer.skippedFrames)"
                    + "\nbreakdown per frame:\n" + FrameProfile.summary())
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
        FrameProfile.endFrame()
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
        // 見えている行のチャンクは急ぎ、前後は先読みとして、コミットの詳細の読み込みより後回しにする。
        prefetch(chunk: index, priority: .userInitiated)
        for i in [index + 1, index - 1] { prefetch(chunk: i, priority: .utility) }
        return chunks[index]
    }

    /// `rows` の件名などを読み、同じ裏のスレッドで行の文字も並べておく（Core Text の処理をメインスレッドから外す）。
    nonisolated private static func loadChunk(
        rows: Range<Int>, of history: History, fonts: RowPreparation.Fonts, compact: Bool
    ) -> (HistoryChunk?, [Int: RowPreparation.Row]) {
        guard let chunk = try? history.chunk(rows: rows) else { return (nil, [:]) }
        var prepared: [Int: RowPreparation.Row] = [:]
        for row in rows {
            prepared[row] = RowPreparation.prepare(row: row, chunk: chunk, history: history, fonts: fonts, compact: compact)
        }
        return (chunk, prepared)
    }

    private func prefetch(chunk index: Int, priority: TaskPriority) {
        guard let history, index >= 0, index * Self.chunkRows < history.rowCount,
              chunks[index] == nil, !loadingChunks.contains(index) else { return }
        loadingChunks.insert(index)
        let rows = index * Self.chunkRows..<min(history.rowCount, (index + 1) * Self.chunkRows)
        let fonts = rowFonts
        let compact = Settings.compactHistoryRows
        Task {
            let (chunk, prepared) = await Task.detached(priority: priority) {
                Self.loadChunk(rows: rows, of: history, fonts: fonts, compact: compact)
            }.value
            do {
                // 読んでいる間に History や行の表示が変わっていたら捨てる。
                guard self.history === history else { return }
                self.loadingChunks.remove(index)
                self.chunks[index] = chunk
                if compact == Settings.compactHistoryRows {
                    self.preparedRows.merge(prepared) { new, _ in new }
                }
                self.trimChunks()
                let visible = self.tableView.rows(in: self.tableView.visibleRect)
                let lower = max(rows.lowerBound, visible.location)
                let upper = min(rows.upperBound, visible.location + visible.length)
                if lower < upper { self.redrawText() }
            }
        }
    }

    /// 裏のタブになった: 見えている行のチャンクだけを残して、ほかと並べた行・ラベルの文字を捨てる（前に出て
    /// スクロールしたら読み直す）。捨てたチャンクの数を返す。
    @discardableResult
    func shrinkForBackground() -> Int {
        let visible = tableView.rows(in: tableView.visibleRect)
        let first = visible.location / Self.chunkRows
        let last = max(first, (visible.location + max(visible.length, 1) - 1) / Self.chunkRows)
        let before = chunks.count
        for index in Array(chunks.keys) where index < first || index > last {
            chunks.removeValue(forKey: index)
        }
        preparedRows = preparedRows.filter { (first...last).contains($0.key / Self.chunkRows) }
        labelText.removeAll()
        return before - chunks.count
    }

    /// 見えている所から遠いチャンクと、その範囲の並べた行を捨てる（戻ってきたら読み直す）。
    private func trimChunks() {
        guard chunks.count > Self.maxChunks else { return }
        let visible = tableView.rows(in: tableView.visibleRect)
        let center = (visible.location + visible.length / 2) / Self.chunkRows
        let keep = Set(chunks.keys.sorted { abs($0 - center) < abs($1 - center) }.prefix(Self.maxChunks))
        for index in Array(chunks.keys) where !keep.contains(index) {
            chunks.removeValue(forKey: index)
        }
        preparedRows = preparedRows.filter { keep.contains($0.key / Self.chunkRows) }
    }

    /// 複数のコミットを選んで右クリック: 古い順に cherry-pick、新しい順に revert、連続していれば squash、ID のコピー。
    private func buildMultipleMenu(_ menu: NSMenu, rows: [Int], history: History, actions: Actions) {
        // 行は新しい順（0 が一番新しい）。
        let commits: [(hex: String, isMerge: Bool)] = rows.compactMap { row in
            guard let id = history.commitID(row: row) else { return nil }
            let isMerge = history.withNodes { $0[row].flags & UInt16(AmonhenNode_MERGE) != 0 }
            return (id.map { String(format: "%02x", $0) }.joined(), isMerge)
        }
        func add(_ title: String, _ action: @escaping () -> Void) { menu.addItem(ClosureMenuItem(title: title, action: action)) }
        add(L("Cherry-pick %ld Commits (Oldest First)", commits.count)) {
            actions.perform(sequence: commits.reversed().map { ["op": "cherryPick", "oid": $0.hex, "isMerge": $0.isMerge] })
        }
        add(L("Revert %ld Commits (Newest First)", commits.count)) {
            actions.perform(sequence: commits.map { ["op": "revert", "oid": $0.hex, "isMerge": $0.isMerge] })
        }
        // 連続した行（first-parent で続いているかはコアで確かめる）なら、1 つにまとめられる。
        let sorted = rows.sorted()
        if sorted.count >= 2, zip(sorted, sorted.dropFirst()).allSatisfy({ $1 == $0 + 1 }),
           !commits.contains(where: \.isMerge) {
            add("Squash \(commits.count) Commits…") { actions.squash(range: commits.map(\.hex)) }
        }
        menu.addItem(.separator())
        add(L("Copy Commit IDs")) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(commits.map(\.hex).joined(separator: "\n"), forType: .string)
        }
        add(L("Copy Short IDs")) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(commits.map { String($0.hex.prefix(10)) }.joined(separator: " "), forType: .string)
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let row = tableView.clickedRow
        guard let actions, let history, row >= 0, let id = history.commitID(row: row) else { return }
        // 複数選んだ行の上なら、選んだコミットまとめての操作。
        let selected = tableView.selectedRowIndexes
        if selected.count > 1, selected.contains(row) {
            buildMultipleMenu(menu, rows: selected.sorted(), history: history, actions: actions)
            return
        }
        let hex = id.map { String(format: "%02x", $0) }.joined()
        let short = String(hex.prefix(8))
        let isMerge = history.withNodes { $0[row].flags & UInt16(AmonhenNode_MERGE) != 0 }
        func add(_ title: String, _ action: @escaping () -> Void) {
            menu.addItem(ClosureMenuItem(title: title, action: action))
        }
        // 折りたたんだ merge: 取り込んだ側を開く・たたむ。
        let hidden = history.hiddenCount(row: row)
        if hidden > 0 || expandedMerges.contains(id) {
            add(hidden > 0 ? L("Show %ld Merged Commits", hidden) : L("Fold Merged Commits")) { [weak self] in self?.toggleMerge(id) }
            menu.addItem(.separator())
        }
        add("New Branch at \(short)…") { actions.newBranch(at: hex) }
        add("New Tag at \(short)…") { actions.newTag(at: hex) }
        add("Checkout \(short) (Detached)") { actions.perform(["op": "detach", "oid": hex]) }
        menu.addItem(.separator())
        if !isMerge {
            add(L("Edit Message…")) { actions.reword(hex) }
            add("Squash into Parent…") { actions.squash(hex) }
            add("Fixup into Parent") { actions.rewrite("Fixup \(short)", ["kind": "fixup", "oid": hex]) }
            add(L("Edit (Stop Here)")) { actions.rewrite("Edit \(short)", ["kind": "edit", "oid": hex]) }
            add(L("Split…")) { actions.rewrite("Split \(short)", ["kind": "split", "oid": hex]) }
            add("Drop") {
                guard actions.confirm(L("Drop %@?", short), message: L("You can Undo this."), ok: "Drop") else { return }
                actions.rewrite("Drop \(short)", ["kind": "drop", "oid": hex])
            }
            menu.addItem(.separator())
        }
        add("Cherry-pick \(short)") { actions.perform(["op": "cherryPick", "oid": hex, "isMerge": isMerge]) }
        add("Revert \(short)") { actions.perform(["op": "revert", "oid": hex, "isMerge": isMerge]) }
        menu.addItem(.separator())
        add(L("Copy Commit ID")) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(hex, forType: .string)
        }
        // このコミットを指すブランチ・タグの名前（リモートブランチは origin/ を付けたまま）。
        for label in (refLabels[id] ?? []).prefix(5) {
            let name = label.name
            add(label.kind == .tag ? L("Copy Tag Name “%@”", name) : L("Copy Branch Name “%@”", name)) {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(name, forType: .string)
            }
        }
    }

    /// History を入れ替えて選び直している途中（詳細には、終わってから 1 回だけ知らせる）。
    private var quietSelection = false

    func tableViewSelectionDidChange(_ notification: Notification) {
        redrawText()
        updateLineage()
        guard !quietSelection else { return }
        notifySelection()
    }

    /// 選んだコミットを詳細に知らせる。
    private func notifySelection() {
        let rows = tableView.selectedRowIndexes
        if rows.count > 1, let history {
            onSelectRows?(Array(rows), history)
            return
        }
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
                    alert.messageText = L("Can’t Move Here")
                    alert.informativeText = L("Commits can only be reordered between HEAD and the first merge.")
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

/// History の行の文字を並べる（どのスレッドからでも呼べる）。チャンクを裏で読んだときに一緒に行う。
enum RowPreparation {
    nonisolated struct Fonts: @unchecked Sendable {
        let subject: CTFont
        let meta: CTFont
    }

    nonisolated struct Row: @unchecked Sendable {
        let subject: TextRenderer.PreparedLine
        let meta: TextRenderer.PreparedLine
    }

    // DateFormatter は書式を変えなければ複数のスレッドから使える。
    nonisolated(unsafe) private static let shortDate: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .short
        f.timeStyle = .none
        return f
    }()

    nonisolated(unsafe) private static let date: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .short
        return f
    }()

    nonisolated static func prepare(row: Int, chunk: HistoryChunk, history: History?, fonts: Fonts, compact: Bool) -> Row {
        let subject = chunk.withSubject(row: row) { TextRenderer.prepare($0, font: fonts.subject) }
        let time = chunk.authorTime(row: row)
        var meta: String
        if compact {
            // 1 行では件名に幅を譲り、作者と短い日時だけにする（ID は詳細に出る）。
            meta = "\(chunk.authorName(row: row))  \(shortDate.string(from: time))"
        } else {
            let short = history?.commitID(row: row).map { $0.prefix(4).map { String(format: "%02x", $0) }.joined() } ?? ""
            meta = "\(chunk.authorName(row: row))  ·  \(date.string(from: time))  ·  \(short)"
        }
        // 折りたたんだ merge は、隠しているコミットの数を添える。
        let hidden = history?.hiddenCount(row: row) ?? 0
        let shown = hidden > 0 ? meta + "  ·  ⊕ " + L("%ld folded", hidden) : meta
        return Row(subject: subject, meta: TextRenderer.prepare(shown as CFString, font: fonts.meta))
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

/// History の右端の細い帯: 行の区間ごとのコミットの頻度（左半分、青）とブランチの活動量（右半分、緑）を濃さで
/// 描き、今見えている範囲を枠で示す。押す・ドラッグするとそこへ飛ぶ。
final class DensityStripView: NSView {
    var density: [(frequency: Float, activity: Float)] = [] { didSet { needsDisplay = true } }
    /// 見えている範囲（上端・下端、0〜1）。
    /// 見えている範囲。枠は別の層で動かす（スクロールのたびに帯の全体を描き直さない）。
    var visible: (CGFloat, CGFloat) = (0, 0) { didSet { if visible != oldValue { moveViewport() } } }
    var onJump: ((CGFloat) -> Void)?
    private let viewport = CALayer()

    override var isFlipped: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        viewport.borderWidth = 1
        viewport.actions = ["position": NSNull(), "bounds": NSNull(), "frame": NSNull()]
    }

    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        moveViewport()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        moveViewport()
    }

    private func moveViewport() {
        // 層はウィンドウに入ってから作られるので、そのときに枠を載せる。
        if viewport.superlayer == nil, let layer { layer.addSublayer(viewport) }
        let top = visible.0 * bounds.height, bottom = max(top + 3, visible.1 * bounds.height)
        effectiveAppearance.performAsCurrentDrawingAppearance {
            viewport.borderColor = NSColor.labelColor.withAlphaComponent(0.6).cgColor
        }
        // 上が原点のビューでは、層も上が原点になる（そうでなければ下から測る）。
        let flipped = layer?.isGeometryFlipped ?? true
        viewport.frame = NSRect(x: 0, y: flipped ? top : bounds.height - bottom, width: bounds.width, height: bottom - top)
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        bounds.fill()
        let n = density.count
        guard n > 0 else { return }
        let h = bounds.height / CGFloat(n)
        let half = bounds.width / 2
        for (i, d) in density.enumerated() {
            let y = CGFloat(i) * h
            NSColor.systemBlue.withAlphaComponent(CGFloat(d.frequency) * 0.75).setFill()
            NSRect(x: 0, y: y, width: half, height: h + 0.5).fill()
            NSColor.systemGreen.withAlphaComponent(CGFloat(d.activity) * 0.75).setFill()
            NSRect(x: half, y: y, width: half, height: h + 0.5).fill()
        }
    }

    override func mouseDown(with event: NSEvent) { jump(event) }
    override func mouseDragged(with event: NSEvent) { jump(event) }

    private func jump(_ event: NSEvent) {
        let y = convert(event.locationInWindow, from: nil).y
        onJump?(min(1, max(0, y / max(1, bounds.height))))
    }
}
