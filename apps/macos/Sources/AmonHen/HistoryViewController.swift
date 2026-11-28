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

    /// Core Animation の反映を待たずに、描いたらすぐ出す（GPUI と同じく、描く経路はこの 1 本だけにする）。
    func presentImmediately() {
        metalLayer.presentsWithTransaction = false
        // 画面の切り替えを待つ列に並べず、WindowServer が次に重ねるときに最新の絵を使わせる。並べると、毎フレーム
        // 描くスクロールで列がたまり、表示まで 4 フレームかかった（linux で計測。切ると p50 10.7 ms）。
        metalLayer.displaySyncEnabled = false
    }

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
    /// 行の印（CI など）の上で押した。true を返せば行を選ばない（吹き出しを出す）。点は表の座標。
    var onClickMark: ((Int, NSPoint) -> Bool)?

    /// 最後に押した行（いくつか選んだ行の 1 つをドラッグしたとき、つかんだ行だけを動かすため）。
    private(set) var mouseDownRow = -1

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let row = row(at: point)
        mouseDownRow = row
        if row >= 0, onClickMark?(row, point) == true { return }
        super.mouseDown(with: event)
    }
    /// 押したキー。true を返せば表に渡さない（History の上で編集するときの p・r・s・f・d・e）。
    var onKey: ((NSEvent) -> Bool)?

    override func keyDown(with event: NSEvent) {
        if onKey?(event) == true { return }
        super.keyDown(with: event)
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

    /// 最後の行の下に置くビュー（shallow clone の帯）。表をその高さの分だけ伸ばし、スクロールで一緒に動かす。
    var footer: NSView? {
        didSet {
            guard footer !== oldValue else { return }
            oldValue?.removeFromSuperview()
            if let footer { addSubview(footer) }
            tile()
        }
    }

    private var rowsHeight: CGFloat { CGFloat(numberOfRows) * (rowHeight + intercellSpacing.height) }

    // 表は行の分の高さに自分で合わせるので、そのたびに帯の分を足す（足した高さから求め直さないよう、行から求める）。
    override func setFrameSize(_ newSize: NSSize) {
        var size = newSize
        if let footer { size.height = max(size.height, rowsHeight + footer.frame.height) }
        super.setFrameSize(size)
        placeFooter()
    }

    override func tile() {
        super.tile()
        placeFooter()
    }

    private func placeFooter() {
        guard let footer else { return }
        footer.frame = NSRect(x: 0, y: rowsHeight, width: bounds.width, height: footer.frame.height)
    }
}

/// History の表を入れるスクロールビュー。表のアクセシビリティは子に行と列のほかを置けない（置くと子を返せなくなる）
/// ので、表の下の帯はここの子として出す。
private final class HistoryScrollView: NSScrollView {
    override func accessibilityChildren() -> [Any]? {
        let children = super.accessibilityChildren()
        guard let footer = (documentView as? FocusTableView)?.footer else { return children }
        return (children ?? []) + [footer]
    }
}

final class HistoryViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate,
    NSSearchFieldDelegate
{
    private let repository: Repository
    private var history: History?
    private let renderer: GraphRenderer
    private let scrollView: NSScrollView = HistoryScrollView()
    private let tableView = FocusTableView()
    /// 窓を開いたときにキー入力を受けるところ（GTK4 版と同じく一覧）。
    var initialKeyView: NSView { tableView }
    private let graphView = GraphView()
    /// 件名などを描く。表のセルに文字を描かせると、速いスクロールでは CPU で描く分が間に合わない。
    private let textView = GraphView()
    private let text: TextRenderer
    /// グラフと文字を描いて出す、ただ 1 本の描画のスレッド。
    private let renderThread: HistoryRenderThread
    /// 行は 2 行: 1 行目に ref のラベルと件名、2 行目に作者・日時・短い ID。
    /// その行のグラフの右端から文字までの余白。
    private static let graphGap: CGFloat = 10

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
    /// 行の右端の CI の印を押した（コミット、その印の矩形、矩形の座標の元のビュー）。詳細の吹き出しを出す。
    var onClickCI: ((Data, PullRequest.CI, NSRect, NSView) -> Void)?

    /// 検証する側が変わったので、覚えていた結果を捨てて検証し直す（結果を引き継がない）。
    func forgetSignatures() {
        signatureGeneration += 1
        verifyWork?.cancel()
        verifiedSignatures.removeAll()
        verifyingSignatures.removeAll()
        redrawText()
    }
    private var labelText: [String: TextRenderer.ShapedLine] = [:]
    /// 見えている行の、名前を全部は出していない札（中央を切った札・「+N」）の横の範囲（表の座標）。押すと名前の
    /// 一覧を出す。
    private var clippedLabels: [Int: [ClosedRange<CGFloat>]] = [:]
    private var refsPopover: NSPopover?
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
    /// merge を既定でたたむか（「すべてたたむ」・「すべて表示」）と、既定と逆にした merge（コミット ID）。
    /// リポジトリの `.git/amonhen/fold.json` に覚える（コアが読み書きし、どの FE でもそろう）。
    private var foldsByDefault = false
    private var foldExceptions: Set<Data> = []
    /// たたんだ merge の「+N」の札の横の範囲（表の座標。押すと開く）。
    private var foldBadges: [Int: ClosedRange<CGFloat>] = [:]
    /// partial clone か（中身の検索は、取っていない中身をすべて取るので、先に確かめる）。
    var isPartialClone = false
    /// shallow clone の History の最後の行の下に置く帯（取得していない古い履歴を取り足す）。
    private lazy var shallowBand: ShallowHistoryBand = {
        let band = ShallowHistoryBand(frame: NSRect(x: 0, y: 0, width: 400, height: ShallowHistoryBand.height))
        band.onFetch = { [weak self] command, done in
            guard let actions = self?.actions else { return done() }
            actions.perform(command, completion: { _ in done() }, recover: { _ in
                done()
                return false
            })
        }
        band.boundaryDate = { [weak self] in self?.oldestBoundaryDate() }
        return band
    }()
    /// 右端の密度の帯（コミットの頻度とブランチの活動量。押すとそこへ飛ぶ）。
    private let densityView = DensityStripView()
    private var densityRequest = 0
    /// 全ブランチをそのまま出すか（折りたたみも絞り込みもしない）。
    private var showsFullHistory: Bool { scope == .all && !foldsMerges }
    /// たたむ merge があるか。
    private var foldsAny: Bool { foldsByDefault || !foldExceptions.isEmpty }
    /// 取り込んだ枝をたたむか（編集の間は全部の行を出す）。
    private var foldsMerges: Bool { foldsAny && editor == nil }

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
    /// Repository Settings の取得とフォルダのページを開く（中身の検索に要る中身が多すぎるとき）。
    var onShowFetchingSettings: (() -> Void)?
    private var searchRows: [Int] = []
    private var searchRowSet: Set<Int> = []
    private var searchIndex: Int?
    private var searchTask: Task<Void, Never>?
    /// 走っている検索（partial clone の中身の確認を含む）があるか。`searchGeneration` で、後から始めた検索と見分ける。
    private var searchRunning = false
    private var searchGeneration = 0
    /// 検索の前・次の一致へ移るボタンと、一致したコミットだけを出す切り替え。
    private let previousMatchButton = NSButton()
    private let nextMatchButton = NSButton()
    private let matchOnlyButton = NSButton()
    /// 並び順・項目・1 行表示のメニューを出すボタン（バーの右端）。
    private let displayButton = NSButton()
    /// HEAD のコミットへ（Go to HEAD、⌘⇧H。メニューバーを使わずに届くように）。
    private let headButton = NSButton()
    /// 一致したコミットだけを出しているときの、元の History（出していなければ nil）。
    private var baseHistory: History?
    /// HEAD のコミット（Current Branch で使う）。
    var head: Data? {
        // ほかのプログラムで commit・rebase すると HEAD が動く。今のブランチに絞っていれば絞り込み直す。
        didSet {
            renderer.head = head
            if head != oldValue { redrawGraph(); refreshFirstParentLine() }
            if head != oldValue, scope == .current, history != nil { scheduleReloadScope() }
        }
    }
    /// 選んだコミット（20 バイト）。選択がなくなれば nil。
    /// History の編集モードに入る（そのコミットから HEAD まで、最初の変更つき）。
    var onEditCommits: ((String, HistoryEditController.Preset?) -> Void)?
    var onSelectCommit: ((Data?) -> Void)?
    /// 複数の行を選んだ（⇧・⌘ クリック）。まとめた差分を見せる。
    var onSelectRows: (([Int], History) -> Void)?
    /// Range Diff で比べる（git range-diff と同じ形の範囲）。
    var onRangeDiff: (([String]) -> Void)?

    init(repository: Repository, renderer: GraphRenderer, text: TextRenderer) {
        self.repository = repository
        self.renderer = renderer
        self.text = text
        // GraphRenderer と TextRenderer は同じ既定の GPU を使う。
        renderThread = HistoryRenderThread(device: renderer.device)!
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
            self, selector: #selector(historyOrderChanged), name: .historyOrderChanged, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(highlightAncestorsChanged), name: .highlightAncestorsChanged, object: nil)
        tableView.intercellSpacing = NSSize(width: 6, height: 0)
        tableView.usesAlternatingRowBackgroundColors = false
        tableView.allowsTypeSelect = false
        // ⇧・⌘ クリックで複数のコミットを選び、まとめた差分を見る。
        tableView.allowsMultipleSelection = true
        tableView.style = .plain
        tableView.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        // ヘッダーの右クリックで、並び順と行に出す項目を選ぶ（バーの右端のボタンと同じ）。
        tableView.headerView?.menu = HistoryDisplayMenu.shared.displayMenu()
        graphColumn.title = ""
        // 幅は見えているレーンの分だけ取る（`layoutGraphView` が描くたびに合わせる）。
        graphColumn.width = 120
        graphColumn.resizingMask = []
        tableView.addTableColumn(graphColumn)
        let subject = NSTableColumn(identifier: .init("subject"))
        subject.title = L("Message")
        subject.width = 240
        tableView.addTableColumn(subject)
        // 行の右端の CI の印（描くのと同じ位置: 2 列目の右端から 18 pt）を押したら、行を選ばずに詳細の吹き出し。
        tableView.onClickMark = { [weak self] row, point in
            // History の上で編集しているときの、札・境目・ダブルクリック。
            if let self, editor != nil, handleEditClick(row: row, point: point) { return true }
            // たたんだ merge の「+N」を押したら、行を選ばずに開く。
            if let self, foldBadges[row]?.contains(point.x) == true, let id = history?.commitID(row: row) {
                toggleMerge(id)
                return true
            }
            // 名前を全部は出していない札（中央を切った札・「+N」）を押したら、行を選ばずに名前の一覧の吹き出し。
            if let self, let range = clippedLabels[row]?.first(where: { $0.contains(point.x) }) {
                showRefs(row: row, range: range)
                return true
            }
            guard let self, onClickCI != nil, Settings.historyItems.contains(.ci), let history,
                  let id = history.commitID(row: row), let ci = ciStatus[id]
            else { return false }
            let column = tableView.rect(ofColumn: 1).insetBy(dx: 5, dy: 0)
            guard point.x >= column.maxX - 18, point.x <= column.maxX + 2 else { return false }
            let rowRect = tableView.rect(ofRow: row)
            let mark = NSRect(x: column.maxX - 16, y: rowRect.midY - 7, width: 14, height: 14)
            onClickCI?(id, ci, mark, tableView)
            return true
        }
        tableView.dataSource = self
        tableView.delegate = self
        tableView.onKey = { [weak self] event in
            guard let self else { return false }
            return handleEditKey(event) || handleFoldKey(event)
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
        graphView.presentImmediately()
        textView.presentImmediately()
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
        searchField.toolTip = L("Words match the subject, author and ID. Spaces mean AND, “or” means OR, and parentheses group.\nauthor:name  committer:name  subject:text  message:text  commit:ID  path:path  contents:\"diff text\"\nExample: path:fs/ (author:linus or author:greg)")
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
            (displayButton, "slider.horizontal.3", L("History Display: order, items, one line per commit"), #selector(displayClicked)),
            (headButton, "location", L("Go to HEAD (⌘⇧H)"), #selector(RepositoryWindow.goToHead(_:))),
        ] {
            button.bezelStyle = .accessoryBarAction
            button.isBordered = false
            button.image = AppIcons.image(symbol: symbol, label: tip)
            button.toolTip = tip
            button.target = self
            button.action = action
            button.controlSize = .small
            button.isEnabled = false
            container.addSubview(button)
        }
        matchOnlyButton.setButtonType(.pushOnPushOff)
        displayButton.isEnabled = true
        // Go to HEAD はウィンドウ（レスポンダチェーン）が受ける。
        headButton.target = nil
        headButton.isEnabled = true
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
        // 右から、表示のメニュー・HEAD へ・絞り込み・次・前・件数。
        let buttons: CGFloat = 5 * 22
        let ownField = toolbarSearchShown?() != true
        searchField.isHidden = !ownField
        searchField.frame = NSRect(
            x: 192, y: barY, width: ownField ? max(80, view.bounds.width - 192 - countWidth - buttons - 12) : 0,
            height: barHeight - 4)
        searchSpinner.frame = NSRect(x: (ownField ? searchField.frame.maxX : 186) + 6, y: barY + 3, width: 16, height: 16)
        let countX = view.bounds.width - countWidth - buttons - 6
        searchCount.frame = NSRect(x: countX, y: barY + 2, width: countWidth, height: 16)
        for (i, button) in [previousMatchButton, nextMatchButton, matchOnlyButton, headButton, displayButton].enumerated() {
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
        let fold = foldsMerges, folded = foldsByDefault, except = Array(foldExceptions)
        Task {
            let next: History? = await Task.detached(priority: .userInitiated) {
                if fold { return try? repository.foldedHistory(tips: tips, folded: folded, except: except) }
                return if let tips { try? repository.filteredHistory(tips: tips) } else { repository.latestHistory() }
            }.value
            // 起点がもう History にない（ほかのプログラムの rebase で消えた HEAD を、ref を読み直す前に使った）と
            // 空になる。空にはせず今の表示を残し、ref を読み直して起点を付け替えたときに出し直す。
            guard let next, next.rowCount > 0 || repository.latestHistory()?.rowCount == 0 else { return }
            showBase(next, resetScroll: resetScroll)
            if baseHistory != nil {
                // 探し直している間は今の絞り込みが出ている。選ぶのは、新しい絞り込みを出してから（行番号は別の History のもの）。
                selectAfterSearch = commit
            } else if let commit, let row = next.row(of: commit) {
                select(row: row)
            }
        }
    }

    /// 前回の History を永続キャッシュからすぐ表示し、最新にするのは裏で行う。
    func load(completion: @escaping (Error?) -> Void) {
        if let cached = repository.loadCachedHistory() {
            show(cached)
        }
        let repository = self.repository, order = Settings.historyOrder
        Task {
            do {
                let (latest, fold) = try await Task.detached(priority: .userInitiated) {
                    // 並び順は読む前にコアへ（既定の日付順なら何もしない）。
                    if order != .date { try repository.setHistoryOrder(order) }
                    return (try repository.loadHistory(), repository.foldState())
                }.value
                foldsByDefault = fold.folded
                foldExceptions = Set(fold.except)
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

    /// 行の文字を読み直す（mailmap が変わって作者名が変わるときなど。History はそのまま）。
    func reloadRows() {
        guard let history else { return }
        present(history, resetScroll: false)
    }

    /// 並び順が変わった。コアに並べ直させて（git は読み直さない）、表示し直す。選んでいたコミットは `show` が探し直す。
    @objc private func historyOrderChanged() {
        let repository = self.repository, order = Settings.historyOrder
        Task {
            let latest: History? = await Task.detached(priority: .userInitiated) {
                guard (try? repository.setHistoryOrder(order)) != nil else { return nil }
                return try? repository.loadHistory()
            }.value
            guard let latest else { return }
            if showsFullHistory { showBase(latest) } else { reloadScope(resetScroll: false) }
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
        let fonts = rowFonts, compact = Settings.oneLineHistoryRows, items = Settings.historyItems
        let chunkRows = Self.chunkRows
        Task {
            let loaded: Preloaded = await Task.detached(priority: .userInitiated) {
                indices.compactMap { index in
                    let rows = index * chunkRows..<min(next.rowCount, (index + 1) * chunkRows)
                    let (chunk, prepared) = Self.loadChunk(rows: rows, of: next, fonts: fonts, compact: compact, items: items)
                    return chunk.map { (index, $0, prepared) }
                }
            }.value
            guard request == presentRequest else { return }
            let same = compact == Settings.oneLineHistoryRows && items == Settings.historyItems
            show(next, resetScroll: resetScroll, preloaded: same ? loaded : [])
        }
    }

    /// 探し直して出した History で選ぶコミット（範囲を替えたとき）。
    private var selectAfterSearch: Data?

    private func selectWanted(in history: History) {
        guard let id = selectAfterSearch else { return }
        selectAfterSearch = nil
        if let row = history.row(of: id) { select(row: row) }
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
        scrollToCenter(row: row)
    }

    /// `row` を真ん中あたりに出す（選択は変えない）。
    private func scrollToCenter(row: Int) {
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
        // キャッシュから開いた直後の History に、残りのレイアウトが届いただけ（行は同じで、グラフが足されるだけ）なら
        // 溶かさない。前に出ていない窓ではアニメーションが進まず、グラフの欠けた前の見た目のまま残る。
        let layoutCompleted = self.history.map { old in
            old.laidOutRows < old.rowCount && old.rowCount == history.rowCount
                && old.commitID(row: 0) == history.commitID(row: 0)
        } ?? false
        if !first, !layoutCompleted, history.generation != self.history?.generation, let layer = view.layer {
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
        updateShallowBand()
        if editor != nil { rebuildEditMap() }
        // History が替わるとレーンの数も替わる。列の幅を合わせてから描く。
        layoutGraphView()
        updateDensity()
        if let afterShow {
            self.afterShow = nil
            afterShow()
        }
    }

    /// 次に History を出したらすること（編集を終えて元の表示に戻すとき）。
    private var afterShow: (() -> Void)?

    /// shallow clone の History なら、最後の行の下に帯を出す（一致したコミットだけを出しているときは出さない。
    /// その終わりは History の終わりではない）。
    private func updateShallowBand() {
        tableView.footer = history?.isShallow == true && baseHistory == nil ? shallowBand : nil
    }

    /// 境界のコミットのうち、いちばん古いものの作者の日時（読み込んであるチャンクにあれば）。
    private func oldestBoundaryDate() -> Date? {
        guard let history else { return nil }
        // 境界の行は表の終わりのほうにあり、まだ読んでいないことが多い。読んでいなければその行だけ読む。
        return history.boundaryRows().compactMap { row in
            (chunks[row / Self.chunkRows] ?? (try? history.chunk(rows: row..<row + 1)))?.authorTime(row: row)
        }.min()
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

    /// merge がたたまれているか（覚えている状態で。隠すコミットがなくても）。
    private func isFolded(_ id: Data) -> Bool { foldsByDefault != foldExceptions.contains(id) }

    /// merge を開く・たたむ。覚えて、History を作り直す。
    private func toggleMerge(_ id: Data) {
        if foldExceptions.remove(id) == nil { foldExceptions.insert(id) }
        saveFoldState()
        reloadScope(select: id, resetScroll: false)
    }

    /// すべての merge をたたむ・開く（個々に開いた・たたんだものも忘れる）。
    func setAllMerges(folded: Bool) {
        guard editor == nil else { return NSSound.beep() }
        foldsByDefault = folded
        foldExceptions.removeAll()
        saveFoldState()
        let selected = tableView.selectedRow >= 0 ? history?.commitID(row: tableView.selectedRow) : nil
        reloadScope(select: selected, resetScroll: false)
    }

    private func saveFoldState() {
        let repository = self.repository, folded = foldsByDefault, except = Array(foldExceptions)
        Task.detached(priority: .utility) { try? repository.setFoldState(folded: folded, except: except) }
    }

    /// 選んだ merge を ← でたたみ、→ で開く（ツリーの開閉と同じ）。修飾キーがあれば何もしない。
    private func handleFoldKey(_ event: NSEvent) -> Bool {
        guard editor == nil, event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty,
              event.keyCode == 123 || event.keyCode == 124,
              tableView.selectedRowIndexes.count == 1, let history else { return false }
        let row = tableView.selectedRow
        guard row >= 0, row < history.rowCount,
              history.withNodes({ $0[row].flags & UInt16(AmonhenNode_MERGE) != 0 }),
              let id = history.commitID(row: row) else { return false }
        let open = event.keyCode == 124
        if open, history.hiddenCount(row: row) == 0 { return false }
        if !open, isFolded(id) { return false }
        toggleMerge(id)
        return true
    }

    /// `id` の新しい行。たいていは前の行か、上に増えた分だけ下にずれた所にあるので、近くから探す（History 全体の
    /// 索引は、linux で作るのに 80 ms かかる）。見つからなければ索引で引く。消えていれば nil。
    private static func locate(_ id: Data, near row: Int, in history: History) -> Int? {
        let end = min(history.rowCount, row + 256)
        if row < end, let found = (row..<end).first(where: { history.commitID(row: $0) == id }) { return found }
        return history.row(of: id)
    }

    /// 選んだコミットの祖先を強調する。
    private func updateLineage() {
        let row = tableView.selectedRow
        guard Settings.highlightAncestors, let history, row >= 0, row < history.rowCount else {
            if renderer.lineage != nil {
                renderer.lineage = nil
                redrawGraph()
            }
            return
        }
        if let current = renderer.lineage, current.history === history, current.row == row { return }
        // 描く行の分だけ、描くときに求める（gitk と同じ）。選んだ入力と同じフレームに出る。
        renderer.lineage = history.lineage(of: row)
        redrawGraph()
    }

    /// バーの右端のボタン: 並び順・項目・1 行表示のメニューをボタンの下に出す。
    @objc private func displayClicked() {
        let menu = HistoryDisplayMenu.shared.displayMenu()
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: displayButton.bounds.minY - 4), in: displayButton)
    }

    @objc private func highlightAncestorsChanged() {
        updateLineage()
    }

    /// グラフの列と、その右の文字の列のうち、ヘッダーの下で見えている範囲に重ねる。グラフの列は見えている
    /// レーンの分だけ取り（文字との隙間を空けない）、幅が変わればその場で列を変えて置き直す。
    private func layoutGraphView() {
        let clip = scrollView.contentView
        let clipFrame = view.convert(clip.frame, from: scrollView)
        let height = max(0, clipFrame.height - clip.contentInsets.top)
        // 2 つのビュー（Metal の層）は列の幅に合わせて伸び縮みさせない。幅が変わるたびに描画先を作り直すと、
        // 列の幅のアニメーションの間、1 フレームごとに数 ms 止まる。グラフは上限の幅、文字は列の左端から右端まで
        // 取り（文字のビューが上。どちらもクリックは通す）、文字の位置は表の列の矩形から決める。
        let column = tableView.rect(ofColumn: tableView.column(withIdentifier: graphColumn.identifier))
        let x = view.convert(column.origin, from: tableView).x
        graphView.frame = NSRect(
            x: x, y: clipFrame.minY, width: CGFloat(renderer.metrics.maxWidth), height: height)
        textView.frame = NSRect(x: x, y: clipFrame.minY, width: max(0, clipFrame.maxX - x), height: height)
        if let history {
            let scrollY = Float(tableView.convert(NSPoint(x: 0, y: 0), from: graphView).y)
            let width = CGFloat(renderer.prepareLanes(history: history, scrollY: scrollY, height: Float(height)))
            if width != graphColumn.width {
                // 列を変えると columnDidResize が来るが、ここで置き直すので受けない。
                resizingGraphColumn = true
                graphColumn.width = width
                tableView.sizeLastColumnToFit()
                resizingGraphColumn = false
            }
        }
        redrawGraph()
        redrawText()
    }

    private var resizingGraphColumn = false

    private func redrawText() {
        guard history != nil, textView.window != nil else { return }
        FrameProfile.measure("text: build") { buildText(scale: textView.metalLayer.contentsScale) }
        FrameProfile.measure("text: submit") { renderThread.submit(text.job(layer: textView.metalLayer, background: .zero)) }
    }

    /// `row` のコミットに付いている ref の名前をすべて並べた吹き出しを、押した札の下に出す。
    private func showRefs(row: Int, range: ClosedRange<CGFloat>) {
        guard let id = history?.commitID(row: row) else { return }
        let labels = (worktreeLabels[id] ?? []) + (refLabels[id] ?? [])
        guard !labels.isEmpty else { return }
        // 署名の吹き出しと同じ組み方: 見出しと、1 行に 1 つの名前（札と同じ色の印つき、選んでコピーできる）。
        // 名前は切らない。吹き出しは一番長い名前の幅に合わせ、480 pt を超える名前は折り返す。
        let font = NSFont.systemFont(ofSize: 12), headFont = NSFont.systemFont(ofSize: 12, weight: .semibold)
        let widest = labels.map { label in
            NSString(string: label.name).size(withAttributes: [.font: label.isHead ? headFont : font]).width
        }.max() ?? 0
        let nameWidth = min(480, ceil(widest) + 2)
        let title = NSTextField(labelWithString: L("Branches and Tags"))
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        var rows: [NSView] = [title]
        for label in labels {
            // 色の印は名前の 1 行目の高さの中ほどに置く（名前が折り返しても 1 行目に並ぶ）。
            let lineHeight = ceil(NSLayoutManager().defaultLineHeight(for: label.isHead ? headFont : font))
            let mark = NSView(frame: NSRect(x: 0, y: 0, width: 8, height: lineHeight))
            let dot = CALayer()
            dot.frame = CGRect(x: 0, y: ((lineHeight - 8) / 2).rounded(), width: 8, height: 8)
            dot.cornerRadius = 2
            dot.backgroundColor = Self.labelBase(label).cgColor
            mark.wantsLayer = true
            mark.layer?.addSublayer(dot)
            mark.translatesAutoresizingMaskIntoConstraints = false
            mark.widthAnchor.constraint(equalToConstant: 8).isActive = true
            mark.heightAnchor.constraint(equalToConstant: lineHeight).isActive = true
            let name = NSTextField(wrappingLabelWithString: label.name)
            name.isSelectable = true
            name.font = label.isHead ? headFont : font
            // ブランチ名には空白がないので、どこででも折り返す。
            name.lineBreakMode = .byCharWrapping
            name.preferredMaxLayoutWidth = nameWidth
            name.widthAnchor.constraint(equalToConstant: nameWidth).isActive = true
            let line = NSStackView(views: [mark, name])
            line.spacing = 6
            line.alignment = .top
            rows.append(line)
        }
        let stack = NSStackView(views: rows)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 5
        stack.setCustomSpacing(8, after: title)
        stack.edgeInsets = NSEdgeInsets(top: 12, left: 16, bottom: 12, right: 16)
        stack.widthAnchor.constraint(equalToConstant: max(nameWidth + 8 + 6, title.intrinsicContentSize.width) + 32)
            .isActive = true
        let content = NSViewController()
        content.view = stack
        // 吹き出しの大きさを中身に合わせる。
        stack.layoutSubtreeIfNeeded()
        content.preferredContentSize = stack.fittingSize
        let rowRect = tableView.rect(ofRow: row)
        let anchor = NSRect(x: range.lowerBound, y: rowRect.minY + 2, width: range.upperBound - range.lowerBound, height: 20)
        refsPopover?.close()
        let popover = NSPopover()
        popover.behavior = .transient
        popover.contentViewController = content
        popover.show(relativeTo: anchor, of: tableView, preferredEdge: .maxY)
        refsPopover = popover
    }

    /// 1 行・2 行の表示に合わせて、行の高さ（表とグラフで共通）を決める。
    private func applyRowStyle() {
        renderer.metrics.rowHeight = Settings.oneLineHistoryRows ? 24 : 38
        tableView.rowHeight = CGFloat(renderer.metrics.rowHeight)
    }

    @objc private func rowStyleChanged() {
        // 見ていたコミットが同じ位置に来るように、選んだ行を見える位置に戻す（いくつか選んでいれば、選んだまま）。
        let selected = tableView.selectedRow
        applyRowStyle()
        rowText.removeAll()
        // 並べ直しが要るので、行の情報は読み直す（裏で並べ直される）。
        preparedRows.removeAll()
        chunks.removeAll()
        loadingChunks.removeAll()
        tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0..<tableView.numberOfRows))
        if selected >= 0 { scrollToCenter(row: selected) }
        layoutGraphView()
    }

    /// ref の一覧が変わった。各コミットのラベルを作り直して描き直す。
    func show(refs: Refs) {
        lastRefs = refs
        refreshFirstParentLine()
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
        let compact = Settings.oneLineHistoryRows
        let items = Settings.historyItems
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
        // ドラッグの絵は選んでいない行の色で（表の選択の背景は絵に入らない）。
        let selection = drawingDragImage ? IndexSet() : tableView.selectedRowIndexes
        if rowText.count > 4096 { rowText.removeAll() }
        clippedLabels.removeAll(keepingCapacity: true)
        var unverified: [Data] = []
        // History を差し替えた直後（表を読み直す前）は、表が前の History の行を返すことがある（絞り込みで行が
        // 減ったときは、見えている行が全部新しい History の外）。
        let last = min(history.rowCount, rows.location + rows.length)
        guard rows.location < last else { return }
        editChips.removeAll(keepingCapacity: true)
        foldBadges.removeAll(keepingCapacity: true)
        for row in rows.location..<last {
            // ドラッグしている行は空けておく（つかんだ行は絵になって動いている）。どこにも落とせないときは、通る線も薄く。
            if row == draggingRow {
                if editor == nil, dropTargets?.isEmpty == true {
                    text.addRect(CGRect(x: 0, y: tableView.rect(ofRow: row).minY - scrollY, width: textView.bounds.width,
                                        height: tableView.rowHeight),
                                 color: NSColor.windowBackgroundColor.withAlphaComponent(0.62).rgba)
                }
                continue
            }
            // History の上で編集しているときは、範囲の行に並べ替えた後のコミットを描く。
            let editIndex = editSlots[row]
            let editRow = editIndex.flatMap { i in editor.map { $0.rows[i] } }
            let source = editSource[row] ?? row
            let outside = editor != nil && editIndex == nil
            guard let line = FrameProfile.measure("text: row text", { shapedRow(source, scale: scale) }) else { continue }
            let rowTop = tableView.rect(ofRow: row).minY - scrollY
            let isSelected = selection.contains(row)
            // 文字はその行のグラフの右端のすぐ右から（行ごとに左端が違う。グラフと文字のビューは左端が同じ）。
            // グラフを描いていない行（History を隠しているとき）は列の位置。
            let start = renderer.rightExtent(row: row).map { CGFloat($0) + Self.graphGap } ?? (column.minX - originX)
            // 右端は列の右端。スクロールバーが場所を取っていれば、見えている所まで（「…」が隠れないように）。
            let end = min(column.maxX - originX, textView.bounds.width - 5)
            if !isSelected, searchRowSet.contains(row) {
                text.addRoundedRect(
                    CGRect(x: start - 3, y: rowTop + 1, width: end - start + 6, height: tableView.rowHeight - 2),
                    radius: 4, color: NSColor.systemYellow.withAlphaComponent(0.14).rgba)
            }
            var x = start
            let id = FrameProfile.measure("text: commit id (FFI)") { history.commitID(row: source) }
            let labels = id.flatMap { id -> [RefLabel]? in
                let all = (worktreeLabels[id] ?? []) + (refLabels[id] ?? [])
                return all.isEmpty ? nil : all
            }
            // 署名があれば、作者・日時の行に検証の結果の色の丸を置く。結果をまだ知らなければ、裏で検証する。
            let kind = chunks[source / Self.chunkRows]?.signature(row: source) ?? 0
            let signed = kind != 0 && items.contains(.signature)
            var status = SignatureStatus.pending
            if signed, let id {
                if let known = verifiedSignatures[id] {
                    status = known
                } else if !verifyingSignatures.contains(id) {
                    unverified.append(id)
                }
            }
            // GitHub の CI の状態（知っていれば）。行の右端に置き、署名の印とは離す。
            let ci = items.contains(.ci) ? id.flatMap { ciStatus[$0] } : nil
            let ciSpace: CGFloat = ci != nil ? 18 : 0
            let right = end - ciSpace
            let dot: CGFloat = signed ? 12 : 0
            // 1 行なら、作者・日時・ID を右に寄せ、件名はその手前まで。
            let metaWidth = compact ? min(line.meta.width, column.width * 0.45) : 0
            var subjectEnd = compact ? right - metaWidth - 12 - dot : right
            // 操作の印（git の言葉）は、件名の場所の右端に。
            if let editRow, !editRow.merge {
                let conflict = editor?.expectedConflicts[editRow.oid] != nil
                let w = editChipWidth(editRow.action, scale: scale)
                drawEditChip(editRow.action, x: subjectEnd - w, baseline: rowTop + subjectBaseline, scale: scale, conflict: conflict)
                editChips[row] = (subjectEnd - w + originX)...(subjectEnd + originX)
                subjectEnd -= w + 8
            }
            // たたんだ merge は、点の右に隠したコミットの数の札（押すと開く）。
            let hidden = editor == nil ? history.hiddenCount(row: source) : 0
            if hidden > 0 {
                let width = drawFoldBadge(hidden, x: x, baseline: rowTop + subjectBaseline, scale: scale, selected: isSelected && focused)
                foldBadges[row] = (x + originX)...(x + width + originX)
                x += width + 6
            }
            // 札は件名の場所の 6 割まで（残りは件名に残す）。入りきらない札は「+N」にまとめる。
            var clipped: [ClosedRange<CGFloat>] = []
            if let labels, items.contains(.refs) {
                (x, clipped) = drawLabels(
                    labels, x: x, maxX: start + (subjectEnd - start) * 0.6, baseline: rowTop + subjectBaseline,
                    scale: scale, selected: isSelected && focused)
            }
            // 名前を全部は出していない札は、押すと名前の一覧を出す（表の座標で覚える）。
            if !clipped.isEmpty {
                clippedLabels[row] = clipped.map { ($0.lowerBound + originX)...($0.upperBound + originX) }
            }
            // 直したメッセージは、その件名を描く。まとめる行は薄く、取り消す行は線で消す。
            let subjectLine = editRow.flatMap { r in r.newMessage.map { _ in editSubject(r.subject, scale: scale) } } ?? line.subject
            let combining = editRow.map { $0.action == .squash || $0.action == .fixup } ?? false
            let dropping = editRow?.action == .drop
            text.addLine(
                subjectLine, origin: CGPoint(x: x, y: rowTop + subjectBaseline),
                color: isSelected ? selected : (combining || dropping) ? secondary : primary,
                scale: scale, maxWidth: max(0, subjectEnd - x), ellipsis: ellipsis)
            if dropping {
                let y = rowTop + subjectBaseline - CTFontGetXHeight(font) / 2
                text.addRect(CGRect(x: x, y: y, width: min(subjectLine.width, max(0, subjectEnd - x)), height: 1),
                             color: isSelected ? selected : secondary)
            }
            let metaMax = end - start - dot - ciSpace
            text.addLine(
                line.meta, origin: CGPoint(x: compact ? right - metaWidth : start, y: rowTop + metaBaseline),
                color: isSelected ? selectedSecondary : secondary, scale: scale,
                maxWidth: compact ? metaWidth : metaMax, ellipsis: metaEllipsis)
            if signed {
                // 署名の状態のアイコン（信用度は形で分かる。塗りつぶしの seal は強い信用）。
                let centerX = compact
                    ? right - metaWidth - 7
                    : start + min(line.meta.width, metaMax) + 9
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
            // 編集の範囲の外の行と、ドラッグで落とせない行（編集できる並びの外）は、グラフごと薄くする（上に背景の色を重ねる）。
            let undroppable = editor == nil && draggingRow != nil && dropTargets.map { id.map($0.contains) != true } == true
            if outside || undroppable {
                text.addRect(CGRect(x: 0, y: rowTop, width: textView.bounds.width, height: tableView.rowHeight),
                             color: NSColor.windowBackgroundColor.withAlphaComponent(0.62).rgba)
            }
        }
        // 範囲のいちばん下の境目（下へ引くと広げ、上へ引くと狭める）。
        if let boundary = editBoundaryRow, boundary >= rows.location, boundary < last {
            let y = tableView.rect(ofRow: boundary).maxY - scrollY
            let accent = NSColor.controlAccentColor.rgba
            text.addRect(CGRect(x: 0, y: y - 1, width: textView.bounds.width, height: 2), color: accent)
            text.addRoundedRect(CGRect(x: 6, y: y - 4, width: 30, height: 8), radius: 4, color: accent)
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

    /// 件名の前に ref のラベルを並べ、件名を始める位置と、名前を全部は出していない札（中央を切った・「+N」に
    /// まとめた）の横の範囲を返す。
    private func drawLabels(
        _ labels: [RefLabel], x start: CGFloat, maxX: CGFloat, baseline: CGFloat, scale: CGFloat, selected: Bool
    ) -> (CGFloat, [ClosedRange<CGFloat>]) {
        let ascent = CTFontGetAscent(labelFont), descent = CTFontGetDescent(labelFont)
        let height = (ascent + descent + 4).rounded()
        let top = (baseline - ascent - 2).rounded()
        func shaped(_ string: String) -> TextRenderer.ShapedLine {
            if let line = labelText[string] { return line }
            let line = text.shape(string as CFString, font: labelFont, scale: scale)
            labelText[string] = line
            return line
        }
        let slots = LabelFit.place(
            widths: labels.prefix(LabelFit.names).map { (shaped($0.name).width + 10).rounded() }, count: labels.count,
            start: start, maxX: maxX, rest: { (shaped("+\($0)").width + 10).rounded() })
        var x = start
        var clipped: [ClosedRange<CGFloat>] = []
        for slot in slots {
            let label: RefLabel?, line: TextRenderer.ShapedLine
            switch slot.content {
            case .name(let i): (label, line) = (labels[i], shaped(labels[i].name))
            case .rest(let n): (label, line) = (nil, shaped("+\(n)"))
            }
            let (fill, ink) = labelColors(label, selected: selected)
            text.addRoundedRect(CGRect(x: slot.x, y: top, width: slot.width, height: height), radius: 4, color: fill)
            let origin = CGPoint(x: slot.x + 5, y: baseline - 1)
            if slot.width < (line.width + 10).rounded() {
                // 入りきらない名前は中央を切る（頭と、版番号などの末尾を残す）。
                clipped.append(slot.x...(slot.x + slot.width))
                text.addLineMiddle(
                    line, origin: origin, color: ink, scale: scale, maxWidth: slot.width - 10, ellipsis: shaped("…"))
            } else {
                if label == nil { clipped.append(slot.x...(slot.x + slot.width)) }
                text.addLine(line, origin: origin, color: ink, scale: scale)
            }
            x = slot.x + slot.width + 4
        }
        return (x + 2, clipped)
    }

    /// たたんだ merge の札（枠だけの丸い札に「+N」）。幅を返す。
    private func drawFoldBadge(_ count: Int, x: CGFloat, baseline: CGFloat, scale: CGFloat, selected: Bool) -> CGFloat {
        let ascent = CTFontGetAscent(labelFont), descent = CTFontGetDescent(labelFont)
        let height = (ascent + descent + 4).rounded()
        let top = (baseline - ascent - 2).rounded()
        let string = "+\(count)"
        let line = labelText[string] ?? text.shape(string as CFString, font: labelFont, scale: scale)
        labelText[string] = line
        let width = max(height, (line.width + 12).rounded())
        let ink = (selected ? NSColor.white : NSColor.secondaryLabelColor).rgba
        let rect = CGRect(x: x, y: top, width: width, height: height)
        // 枠: 外を塗ってから、内を背景の色で塗る（下を通る線を隠す）。
        text.addRoundedRect(rect, radius: height / 2, color: ink)
        text.addRoundedRect(rect.insetBy(dx: 1, dy: 1), radius: height / 2 - 1,
                            color: (selected ? NSColor.selectedContentBackgroundColor : NSColor.controlBackgroundColor).rgba)
        text.addLine(line, origin: CGPoint(x: x + (width - line.width) / 2, y: baseline - 1), color: ink, scale: scale)
        return width
    }

    /// 札の種類の色（worktree とほかの worktree のブランチは緑、ローカルは青、リモートは紫、タグは橙）。
    private static func labelBase(_ label: RefLabel) -> NSColor {
        if label.worktree || label.elsewhere { return .systemGreen }
        return switch label.kind {
        case .local: .systemBlue
        case .remote: .systemPurple
        default: .systemOrange
        }
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
        let base = Self.labelBase(label)
        if label.isHead { return (base.withAlphaComponent(0.85).rgba, NSColor.white.rgba) }
        return (base.withAlphaComponent(selected ? 0.45 : 0.22).rgba, (selected ? NSColor.white : NSColor.labelColor).rgba)
    }

    private func shapedRow(_ row: Int, scale: CGFloat) -> RowText? {
        if let cached = rowText[row] { return cached }
        // 裏で並べておいた行なら、アトラスからグリフを引くだけで済む。
        let prepared = preparedRows.removeValue(forKey: row) ?? chunk(row: row).map { chunk in
            RowPreparation.prepare(
                row: row, chunk: chunk, history: history, fonts: rowFonts, compact: Settings.oneLineHistoryRows,
                items: Settings.historyItems)
        }
        guard let prepared else { return nil }
        let line = RowText(subject: text.shape(prepared.subject, scale: scale), meta: text.shape(prepared.meta, scale: scale))
        rowText[row] = line
        return line
    }

    @objc private func contentDidScroll() {
        guard !resizingGraphColumn else { return }
        motion?.add(y: scrollView.contentView.bounds.minY)
        layoutGraphView()
        updateDensityViewport()
    }

    /// レーンの位置が動いている間だけ、表示のリフレッシュごとに描き直す。
    private var laneAnimation: CADisplayLink?
    private var lastGraphDraw: CFTimeInterval = 0

    private func updateLaneAnimation() {
        guard renderer.isAnimating || renderer.lineagePending else {
            laneAnimation?.isPaused = true
            return
        }
        if laneAnimation == nil {
            let link = graphView.displayLink(target: self, selector: #selector(laneAnimationTick(_:)))
            link.add(to: .main, forMode: .common)
            laneAnimation = link
        }
        laneAnimation?.isPaused = false
    }

    private var drawnAtLastTick = 0

    @objc private func laneAnimationTick(_ link: CADisplayLink) {
        // 前の tick からスクロールで描いていれば描かない（そちらが位置を進めている。同じフレームに 2 回描くと、
        // 先のほうは表示されずに捨てられる）。列の幅も動くので、置き直しから。
        let drawn = renderer.drawnFrames
        defer { drawnAtLastTick = renderer.drawnFrames }
        if drawn == drawnAtLastTick { layoutGraphView() }
    }

    private func redrawGraph() {
        guard let history else { return }
        defer {
            lastGraphDraw = CACurrentMediaTime()
            updateLaneAnimation()
        }
        // グラフのビューの上端が、表の座標でどこに当たるか。
        let scrollY = Float(tableView.convert(NSPoint(x: 0, y: 0), from: graphView).y)
        let eventTime = pendingPresentStart ?? InputEvent.timestamp
        pendingPresentStart = nil
        let measurement = self.measurement
        var onCompleted: (@Sendable (CFTimeInterval) -> Void)?
        if measurement != nil, let eventTime {
            onCompleted = { [weak self] gpuEnd in
                Task { @MainActor in self?.measurement?.gpuDone.append(gpuEnd - eventTime) }
            }
        }
        FrameProfile.measure("graph: total") {
        renderThread.submit(renderer.job(
            history: history, layer: graphView.metalLayer, scrollY: scrollY, onCompleted: onCompleted) {
            [weak self] presented in
            guard let eventTime, presented > 0 else { return }
            Task { @MainActor in
                measurement?.presented.append(presented)
                measurement?.latency.append(presented - eventTime)
                self?.onLatency?(presented - eventTime)
            }
        })
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

    /// ⇧↩（History とツールバーの検索欄）: 新しく打った問い合わせなら、まずそれで探す（前の問い合わせの結果の中を
    /// 動かない）。同じなら `step` だけ一致を動く。
    func search(_ query: String, step: Int) {
        if query != searchQuery { search(query) } else { moveSearch(by: step) }
    }

    @objc func findNext(_ sender: Any?) { moveSearch(by: 1) }
    @objc func findPrevious(_ sender: Any?) { moveSearch(by: -1) }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard control === searchField else { return false }
        switch selector {
        case #selector(NSResponder.insertNewline(_:)) where NSEvent.modifierFlags.contains(.shift):
            search(searchField.stringValue.trimmingCharacters(in: .whitespaces), step: -1)
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
            searchRunning = false
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
        guard isPartialClone, Self.searchesContents(query), let window = view.window else {
            return runSearch(query, in: base)
        }
        // partial clone では、中身の検索（git log -G）の前に、要る中身のうち手元にないものを数えて断る。
        searchTask?.cancel()
        searchSpinner.startAnimation(nil)
        searchCount.stringValue = ""
        let generation = startSearch()
        searchTask = Task {
            defer { self.endSearch(generation) }
            let result = await Task.detached(priority: .userInitiated) { Result { try base.contentsToFetch(query) } }.value
            guard !Task.isCancelled, (self.baseHistory ?? self.history) === base else { return }
            self.searchSpinner.stopAnimation(nil)
            switch result {
            case .success(nil):
                self.runSearch(query, in: base)
            case let .success(need?):
                self.confirmContentsSearch(query, in: base, need: need, window: window)
            case let .failure(error):
                self.searchCount.stringValue = "\(error)"
                self.searchCount.toolTip = "\(error)"
            }
        }
    }

    /// partial clone で中身を探す前に、取る中身の数を出して断る。上限を超えるなら探さず、絞るか中身をすべて取るよう頼む。
    private func confirmContentsSearch(_ query: String, in base: History, need: (count: Int, more: Bool), window: NSWindow) {
        let count = NumberFormatter.localizedString(from: NSNumber(value: need.count), number: .decimal)
        let alert = NSAlert()
        if need.more {
            alert.messageText = L("Too Many Contents to Fetch")
            alert.informativeText = L("This search needs more than %@ file contents not fetched yet. Narrow it with path:, after: or before:, or fetch all contents in Fetching and Folders.", count)
            alert.addButton(withTitle: L("Open Fetching and Folders"))
            alert.addButton(withTitle: L("Cancel"))
            alert.beginSheetModal(for: window) { [weak self] response in
                if response == .alertFirstButtonReturn { self?.onShowFetchingSettings?() }
            }
            return
        }
        alert.messageText = L("Search the Contents?")
        alert.informativeText = L("This search fetches %@ file contents not fetched yet.", count)
        alert.addButton(withTitle: L("Search"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .alertFirstButtonReturn, (baseHistory ?? history) === base else { return }
            runSearch(query, in: base)
        }
    }

    /// 問い合わせに `contents:`（`content:`）があるか（引用の中の語も数えるが、尋ねすぎるだけ）。
    static func searchesContents(_ query: String) -> Bool {
        query.range(of: #"(^|[\s(])contents?:\S"#, options: [.regularExpression, .caseInsensitive]) != nil
    }

    /// `base` で探す。一致だけを出す切り替えが入っていれば、一致したコミットだけの History にして出す。
    private func runSearch(_ query: String, in base: History) {
        searchTask?.cancel()
        searchSpinner.startAnimation(nil)
        searchCount.stringValue = ""
        let repository = self.repository
        let generation = startSearch()
        searchTask = Task {
            defer { self.endSearch(generation) }
            // 内容の検索は大きな履歴では数分かかるので、分かった分から色を付け、件数と進み具合を出す。
            let partial = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(for: .milliseconds(300))
                    guard let self, !Task.isCancelled else { return }
                    // 絞り込みを出したまま元の History で探しているときは、行の番号が合わないので色は付けない。
                    showPartialSearch(repository.searchPartial(), paint: self.history === base)
                }
            }
            let result = await Task.detached(priority: .userInitiated) { Result { try base.search(query) } }.value
            partial.cancel()
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
                self.selectAfterSearch = nil
                return
            }
            // 一致だけを出すかは、終わったときの切り替えで決める（探している間に押されても、この検索を活かす）。
            if self.matchOnlyButton.state == .on, !rows.isEmpty {
                let filtered = await Task.detached(priority: .userInitiated) {
                    repository.matchingHistory(of: base, rows: rows)
                }.value
                guard !Task.isCancelled, (self.baseHistory ?? self.history) === base else { return }
                // 選んでいたコミットは、絞り込んだ History でも選んだままにする（`show` が ID で選び直す）。
                self.baseHistory = base
                self.show(filtered)
                self.selectWanted(in: filtered)
                self.searchQuery = query
                self.searchRows = Array(0..<filtered.rowCount)
                // 全部の行が一致なので、行に一致の色は付けない。
                self.searchRowSet = []
                self.searchIndex = max(0, self.tableView.selectedRow)
            } else {
                if let original = self.baseHistory {
                    self.baseHistory = nil
                    self.show(original)
                    self.selectWanted(in: original)
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

    /// 検索を始めた印を付け、その番号を返す（`endSearch` に渡す）。
    private func startSearch() -> Int {
        searchGeneration += 1
        searchRunning = true
        return searchGeneration
    }

    /// `generation` の検索が終わった（後から別の検索が始まっていれば、そちらが走っている）。
    private func endSearch(_ generation: Int) {
        if generation == searchGeneration { searchRunning = false }
    }

    @objc private func matchOnlyClicked() {
        // 探している途中なら、その検索が終わったときに切り替えを見る（前の問い合わせで探し直して捨てない）。
        if searchRunning { return updateMatchButtons() }
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
        matchOnlyButton.image = AppIcons.image(symbol: on ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle", label: matchOnlyButton.toolTip)
        matchOnlyButton.contentTintColor = on ? .controlAccentColor : nil
    }

    /// History で探しているか（一致がなくても、問い合わせがあれば true）。
    var hasSearch: Bool { searchQuery != nil }

    /// 検索欄に書いてある問い合わせ。History を読み直して結果を捨てても残る（Enter で探し直せる）。
    var searchText: String { searchField.stringValue.trimmingCharacters(in: .whitespaces) }

    /// 検索で一致したコミットの次・前へ（⌘G・⌘⇧G）。探していなければ false。
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

    /// 走っている検索の、そこまでに分かった行に色を付け、件数と探し終えた割合を出す（選んでいる行は動かさない）。
    private func showPartialSearch(_ partial: (rows: [Int], done: Float), paint: Bool) {
        guard partial.done > 0 || !partial.rows.isEmpty else { return }
        if paint { searchRowSet = Set(partial.rows) }
        searchCount.stringValue = L("%d found · %d%%", partial.rows.count, Int(partial.done * 100))
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
        drawGlassContents(in: root, context: context)
        for (image, view) in overlays {
            var frame = root.convert(view.bounds, from: view)
            if root.isFlipped { frame.origin.y = root.bounds.height - frame.maxY }
            context.cgContext.draw(image, in: frame)
        }
        // 窓の背景は contentView の外にあり、透けているところ（History の上の帯など）は透明のまま残る。下に敷く。
        root.effectiveAppearance.performAsCurrentDrawingAppearance {
            NSColor.windowBackgroundColor.setFill()
            root.bounds.fill(using: .destinationOver)
        }
        NSGraphicsContext.restoreGraphicsState()
        return rep
    }

    /// macOS 26 の Liquid Glass（サイドバーなど）は中身をレイヤーの下に置くので、`cacheDisplay` では白く抜ける。
    /// ガラスの形を窓の背景の色で塗り、中身を別に描いて重ねる。
    private func drawGlassContents(in root: NSView, context: NSGraphicsContext) {
        guard #available(macOS 26.0, *) else { return }
        func visit(_ view: NSView) {
            guard !view.isHidden else { return }
            guard let glass = view as? NSGlassEffectView else { return view.subviews.forEach(visit) }
            guard let content = glass.contentView,
                  let rep = content.bitmapImageRepForCachingDisplay(in: content.bounds) else { return }
            content.cacheDisplay(in: content.bounds, to: rep)
            func frame(of view: NSView) -> NSRect {
                var frame = root.convert(view.bounds, from: view)
                if root.isFlipped { frame.origin.y = root.bounds.height - frame.maxY }
                return frame
            }
            let shape = frame(of: glass)
            root.effectiveAppearance.performAsCurrentDrawingAppearance {
                NSColor.windowBackgroundColor.setFill()
                NSBezierPath(roundedRect: shape, xRadius: glass.cornerRadius, yRadius: glass.cornerRadius).fill()
            }
            if let image = rep.cgImage { context.cgContext.draw(image, in: frame(of: content)) }
        }
        visit(root)
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
        rows: Range<Int>, of history: History, fonts: RowPreparation.Fonts, compact: Bool, items: HistoryItems
    ) -> (HistoryChunk?, [Int: RowPreparation.Row]) {
        guard let chunk = try? history.chunk(rows: rows) else { return (nil, [:]) }
        var prepared: [Int: RowPreparation.Row] = [:]
        for row in rows {
            prepared[row] = RowPreparation.prepare(
                row: row, chunk: chunk, history: history, fonts: fonts, compact: compact, items: items)
        }
        return (chunk, prepared)
    }

    private func prefetch(chunk index: Int, priority: TaskPriority) {
        guard let history, index >= 0, index * Self.chunkRows < history.rowCount,
              chunks[index] == nil, !loadingChunks.contains(index) else { return }
        loadingChunks.insert(index)
        let rows = index * Self.chunkRows..<min(history.rowCount, (index + 1) * Self.chunkRows)
        let fonts = rowFonts
        let compact = Settings.oneLineHistoryRows, items = Settings.historyItems
        Task {
            let (chunk, prepared) = await Task.detached(priority: priority) {
                Self.loadChunk(rows: rows, of: history, fonts: fonts, compact: compact, items: items)
            }.value
            do {
                // 読んでいる間に History や行の表示が変わっていたら捨てる。
                guard self.history === history else { return }
                self.loadingChunks.remove(index)
                self.chunks[index] = chunk
                if compact == Settings.oneLineHistoryRows, items == Settings.historyItems {
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

    /// 複数のコミットを選んで右クリック: ID のコピー、続いていれば squash、古い順に cherry-pick、新しい順に revert、
    /// まとめて merge、パッチ、2 つなら Range Diff（並びは 1 つのときと同じ）。
    private func buildMultipleMenu(_ menu: NSMenu, rows: [Int], history: History, actions: Actions) {
        // 行は新しい順（0 が一番新しい）。
        let commits: [(hex: String, isMerge: Bool)] = rows.compactMap { row in
            guard let id = history.commitID(row: row) else { return nil }
            let isMerge = history.withNodes { $0[row].flags & UInt16(AmonhenNode_MERGE) != 0 }
            return (id.map { String(format: "%02x", $0) }.joined(), isMerge)
        }
        func add(_ title: String, _ action: @escaping () -> Void) { menu.addItem(ClosureMenuItem(title: title, action: action)) }
        add(Titles.copyCommitIDs) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(commits.map(\.hex).joined(separator: "\n"), forType: .string)
        }
        add(Titles.copyShortIDs) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(commits.map { String($0.hex.prefix(Titles.shortIDLength)) }.joined(separator: " "), forType: .string)
        }
        menu.addItem(.separator())
        // 連続した行で、first-parent の並びで続いた merge でないコミットなら、1 つにまとめられる（編集の項目なので、
        // できなければ出さない）。
        let sorted = rows.sorted()
        if sorted.count >= 2, zip(sorted, sorted.dropFirst()).allSatisfy({ $1 == $0 + 1 }),
           !commits.contains(where: \.isMerge), combinable(commits.map(\.hex)) {
            add(L("Combine %ld Commits…", commits.count)) { [weak self] in
                self?.onEditCommits?(commits.last!.hex, .combine(commits.map(\.hex)))
            }
            menu.addItem(.separator())
        }
        add(L("Cherry-pick %ld Commits…", commits.count)) { actions.pickWithConfirmation(revert: false, commits.reversed()) }
        add(L("Revert %ld Commits…", commits.count)) { actions.pickWithConfirmation(revert: true, commits) }
        // 今のコミット以外を、上の行から順に 1 つのコミットで merge する（octopus）。ブランチ・タグがあればその名前で
        // （merge のメッセージが git と同じになる）。
        let current = editingBranch ?? lastRefs?.headBranch
        let sources: [MergeSource] = rows.compactMap { row in
            guard let id = history.commitID(row: row), id != head else { return nil }
            let commit = MergeSource(rev: id.map { String(format: "%02x", $0) }.joined(), kind: .commit)
            guard let label = (refLabels[id] ?? []).first(where: { !$0.isHead }) else { return commit }
            return switch label.kind {
            case .local: MergeSource(rev: label.name, kind: .branch)
            case .remote: MergeSource(rev: label.name, kind: .remoteBranch)
            case .tag: MergeSource(rev: label.name, kind: .tag)
            default: commit
            }
        }
        if sources.count > 1 {
            add(L("Merge %ld into %@…", sources.count, current ?? "HEAD")) {
                actions.mergeSeveralWithConfirmation(sources, into: current)
            }
        } else {
            // merge できる数は 1 以下なので、数は出さない。
            menu.addItem(Self.disabledItem(L("Merge into %@…", current ?? "HEAD"),
                                           reason: L("Select two or more commits other than the current one.")))
        }
        menu.addItem(.separator())
        // マージはパッチにできない（git format-patch は飛ばす）。
        if commits.contains(where: \.isMerge) {
            menu.addItem(Self.disabledItem(L("Save %ld Commits as Patches…", commits.count),
                                           reason: L("A merge can’t be saved as a patch.")))
        } else {
            add(L("Save %ld Commits as Patches…", commits.count)) { actions.savePatches(commits.reversed().map(\.hex)) }
        }
        // 2 つなら、それぞれを先端とする 2 つの版として比べられる（rebase の前と後など）。
        if commits.count == 2 {
            add(L("Compare as Two Versions (Range Diff)")) { [weak self] in
                self?.onRangeDiff?(["\(commits[1].hex)...\(commits[0].hex)"])
            }
        } else {
            menu.addItem(Self.disabledItem(L("Compare as Two Versions (Range Diff)"), reason: L("Select exactly two commits.")))
        }
    }

    /// 選んだコミットを古い順に（Commit › Save as Patch…）。マージを含む・選んでいなければ nil。
    /// アーカイブの既定の名前: コミットを指すタグ（なければブランチ）の名前、どれもなければ短い ID。
    func archiveLabel(_ id: Data) -> String {
        let labels = (refLabels[id] ?? []).filter { !$0.worktree }
        let label = labels.first { $0.kind == .tag } ?? labels.first { $0.kind == .local }
        return label?.name ?? String(id.map { String(format: "%02x", $0) }.joined().prefix(Titles.shortIDLength))
    }

    /// Commit > Export Archive…: 選んでいるコミット（1 つだけのとき）。なければ nil（HEAD を書き出す）。
    var selectedCommitForArchive: (hex: String, label: String)? {
        guard let history, tableView.selectedRowIndexes.count == 1, let row = tableView.selectedRowIndexes.first,
              let id = history.commitID(row: row) else { return nil }
        return (id.map { String(format: "%02x", $0) }.joined(), archiveLabel(id))
    }

    var selectedCommitsForPatch: [String]? {
        guard let history else { return nil }
        let rows = tableView.selectedRowIndexes.sorted()
        guard !rows.isEmpty else { return nil }
        var oids: [String] = []
        for row in rows.reversed() {
            guard let id = history.commitID(row: row),
                  history.withNodes({ $0[row].flags & UInt16(AmonhenNode_MERGE) == 0 }) else { return nil }
            oids.append(id.map { String(format: "%02x", $0) }.joined())
        }
        return oids
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let row = tableView.clickedRow
        guard let actions, let history, row >= 0, let id = history.commitID(row: editSource[row] ?? row) else { return }
        if editor != nil { return buildEditMenu(menu, row: row, id: id) }
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
        // 並びはサイドバーと同じくコピーから: コピー・作る・移る・書き換える・取り込む・書き出す・たたむ。
        add(Titles.copyCommitID) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(hex, forType: .string)
        }
        // このコミットを指すブランチ・タグの名前（リモートブランチは origin/ を付けたまま）。
        for label in (refLabels[id] ?? []).prefix(5) {
            let name = label.name
            let shown = Titles.menuName(name)
            add(label.kind == .tag ? L("Copy Tag Name “%@”", shown) : L("Copy Branch Name “%@”", shown)) {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(name, forType: .string)
            }
        }
        menu.addItem(.separator())
        add(Titles.newBranch(from: short)) { actions.newBranch(at: hex) }
        add(Titles.newTag(at: short)) { actions.newTag(at: hex) }
        menu.addItem(.separator())
        add(Titles.checkoutDetached(short)) { actions.checkoutWithConfirmation(.detach(hex)) }
        menu.addItem(actions.resetMenuItem(to: hex))
        menu.addItem(.separator())
        // コミットの編集は、History の編集モードで（変更を積んで Apply）。ここにあるのは、最初の変更を入れて
        // 編集モードを開く近道。編集できないコミット（HEAD の first-parent の並びの外・merge）には出さない。
        let edits = editableEdits(hex, isMerge: isMerge)
        if edits.any {
            add(L("Edit Message…")) { [weak self] in self?.onEditCommits?(hex, .reword(hex)) }
            if edits.squash {
                add(L("Squash into Previous…")) { [weak self] in self?.onEditCommits?(hex, .combine([hex])) }
            }
            add("Drop…") { [weak self] in self?.onEditCommits?(hex, .drop(hex)) }
            add(L("Interactive Rebase from Here…")) { [weak self] in self?.onEditCommits?(hex, nil) }
            // 分ける（止めてから Working Copy で分けてコミットする）は、編集モードの外の操作。
            if edits.split {
                add("Split…") {
                    Task { @MainActor in
                        // worktree の今のブランチを書き換えるので、Cherry-pick・Revert と同じく、どの worktree かを書く。
                        guard await actions.confirm(L("Split %@?", short),
                                                    message: actions.sheetMessage(
                                                        L("Its changes go back to the working tree; commit them in parts, then Continue.") + sentenceGap
                                                            + Actions.canUndo), ok: "Split") else { return }
                        actions.rewrite("Split \(short)", ["kind": "split", "oid": hex])
                    }
                }
            }
            menu.addItem(.separator())
        }
        add(Titles.cherryPick(short)) { actions.pickWithConfirmation(revert: false, [(hex, isMerge)]) }
        add(Titles.revert(short)) { actions.pickWithConfirmation(revert: true, [(hex, isMerge)]) }
        menu.addItem(.separator())
        if isMerge {
            menu.addItem(Self.disabledItem(L("Save as Patch…"), reason: L("A merge can’t be saved as a patch.")))
        } else {
            add(L("Save as Patch…")) { actions.savePatches([hex]) }
        }
        add(L("Export Archive…")) { [weak self] in
            actions.exportArchive(rev: hex, label: self?.archiveLabel(id) ?? short)
        }
        // merge: 取り込んだ側をたたむ・開く。
        if isMerge, editor == nil {
            menu.addItem(.separator())
            let hidden = history.hiddenCount(row: row)
            if hidden > 0 {
                add(L("Show %ld Merged Commits", hidden)) { [weak self] in self?.toggleMerge(id) }
            } else if isFolded(id) {
                // たたんでいるが、取り込んだコミットはどれもほかの枝から見えている。
                menu.addItem(Self.disabledItem(
                    L("Fold Merged Commits"), reason: L("Every commit this merge brought in is shown on another branch.")))
            } else {
                add(L("Fold Merged Commits")) { [weak self] in self?.toggleMerge(id) }
            }
        }
    }

    /// 押せない項目（理由はツールチップに）。
    private static func disabledItem(_ title: String, reason: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.toolTip = reason
        return item
    }

    /// HEAD から first-parent で辿った並び（新しい順、merge かどうか）。右クリックで、編集の項目を出すかを決める。
    /// 読む前は nil（そのときは項目を出し、押してから確かめる）。
    private var firstParentLine: [(oid: String, merge: Bool)]?
    private var firstParentRequest = 0

    /// HEAD・ref が動いたら、first-parent の並びを読み直す。
    private func refreshFirstParentLine() {
        guard let actions else { return }
        firstParentRequest += 1
        let request = firstParentRequest
        Task { @MainActor in
            // 読めなければ（ほかのツールが書いている途中など）知らせずに nil に戻す（項目を出し、押してから確かめる）。
            let line = await actions.firstParentCommitsQuietly()
            guard request == firstParentRequest else { return }
            firstParentLine = line
        }
    }

    /// コミット `hex` に出す編集の項目。編集モードは merge を越えて first-parent の並びを書き換えられる。Squash は
    /// 1 つ前（古い方）が並びにあって merge でないとき、Split は最初の merge より新しく、最初のコミットでないとき。
    private func editableEdits(_ hex: String, isMerge: Bool) -> (any: Bool, squash: Bool, split: Bool) {
        guard !isMerge else { return (false, false, false) }
        guard let line = firstParentLine else { return (true, true, true) }
        guard let index = line.firstIndex(where: { $0.oid == hex }) else { return (false, false, false) }
        let hasOlder = index + 1 < line.count
        let squash = hasOlder && !line[index + 1].merge
        let split = hasOlder && !line[..<index].contains(where: \.merge)
        return (true, squash, split)
    }

    /// 選んだ行（新しい順）が、first-parent の並びで続いた、merge でないコミットか（Combine で 1 つにまとめられる）。
    private func combinable(_ hexes: [String]) -> Bool {
        guard let line = firstParentLine else { return true }
        guard let first = line.firstIndex(where: { $0.oid == hexes.first }), first + hexes.count <= line.count else { return false }
        return zip(line[first...], hexes).allSatisfy { $0.oid == $1 && !$0.merge }
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
            onSelectRows?(rows.map { editSource[$0] ?? $0 }, history)
            return
        }
        let row = tableView.selectedRow
        onSelectCommit?(row >= 0 ? history?.commitID(row: editSource[row] ?? row) : nil)
    }

    // MARK: - ドラッグで並べ替え

    /// ドラッグの間だけ覚えておく、編集できるコミット（HEAD から最初の merge まで、新しい順の 16 進）。落とせる所を
    /// その場で決める（落としてから「ここには動かせない」と言わない）。
    private var dragChain: [String]?
    /// ドラッグしている行（表の行）。その行は空けて描く（点も文字も描かない）。
    private var draggingRow: Int?
    /// ドラッグの間に落とせる行のコミット（編集できる並び。つかんだコミットが並びの外なら空）。読み終えるまでは nil で、
    /// それまでは薄くしない。
    private var dropTargets: Set<Data>?
    /// ドラッグの絵を描いている（選んだ色にしない）。
    private var drawingDragImage = false

    /// `row` の行を、画面と同じ絵（グラフの点と線・札・件名・作者）にする。置き場所は表の座標。
    private func rowImage(_ row: Int) -> (image: NSImage, frame: NSRect)? {
        guard let history, row < tableView.numberOfRows else { return nil }
        let scale = view.window?.backingScaleFactor ?? 2
        let graphScrollY = tableView.convert(NSPoint(x: 0, y: 0), from: graphView).y
        let textScrollY = tableView.convert(NSPoint(x: 0, y: 0), from: textView).y
        drawingDragImage = true
        buildText(scale: scale)
        drawingDragImage = false
        guard let graph = renderer.image(history: history, size: graphView.bounds.size, scale: scale, scrollY: Float(graphScrollY)),
              let text = text.image(size: textView.bounds.size, scale: scale, background: .zero) else { return nil }
        // 次の描画で、選んだ色の文字に戻す。
        DispatchQueue.main.async { [weak self] in self?.redrawText() }
        let rowRect = tableView.rect(ofRow: row)
        let left = tableView.convert(NSPoint(x: 0, y: 0), from: textView).x
        let column = tableView.rect(ofColumn: 1).insetBy(dx: 5, dy: 0)
        let frame = NSRect(x: left, y: rowRect.minY, width: max(1, min(column.maxX, tableView.visibleRect.maxX) - left),
                           height: rowRect.height)
        func crop(_ image: CGImage, top: CGFloat) -> CGImage? {
            image.cropping(to: CGRect(x: 0, y: top * scale, width: frame.width * scale, height: frame.height * scale)
                .integral.intersection(CGRect(x: 0, y: 0, width: image.width, height: image.height)))
        }
        let graphRow = crop(graph, top: rowRect.minY - graphScrollY)
        let textRow = crop(text, top: rowRect.minY - textScrollY)
        let appearance = view.effectiveAppearance
        let image = NSImage(size: frame.size, flipped: false) { rect in
            appearance.performAsCurrentDrawingAppearance {
                let shape = NSBezierPath(roundedRect: rect.insetBy(dx: 0.5, dy: 0.5), xRadius: 5, yRadius: 5)
                NSColor.controlBackgroundColor.withAlphaComponent(0.92).setFill()
                shape.fill()
                NSColor.separatorColor.setStroke()
                shape.stroke()
            }
            guard let context = NSGraphicsContext.current?.cgContext else { return true }
            for image in [graphRow, textRow].compactMap({ $0 }) {
                context.draw(image, in: CGRect(x: 0, y: rect.height - CGFloat(image.height) / scale,
                                               width: CGFloat(image.width) / scale, height: CGFloat(image.height) / scale))
            }
            return true
        }
        return (image, frame)
    }

    func tableView(_ tableView: NSTableView, draggingSession session: NSDraggingSession, willBeginAt screenPoint: NSPoint, forRowIndexes rowIndexes: IndexSet) {
        dragChain = nil
        dropTargets = nil
        let grabbed = self.tableView.mouseDownRow
        let row = rowIndexes.contains(grabbed) ? grabbed : rowIndexes.first
        // 運ぶ絵は、その行と同じ絵（セルを置いていないので作る）。行の上にそのまま重ねて持ち上げる。空ける前に描く。
        draggingRow = nil
        if let row, let (image, frame) = rowImage(row) {
            session.enumerateDraggingItems(
                options: [], for: tableView, classes: [NSPasteboardItem.self], searchOptions: [:]
            ) { item, _, _ in
                item.setDraggingFrame(frame, contents: image)
            }
        }
        draggingRow = row
        renderer.hiddenRow = row
        redrawGraph()
        redrawText()
        guard let actions, editor == nil else { return }
        Task { @MainActor in
            let chain = await actions.editableCommits()
            guard draggingRow != nil else { return }
            dragChain = chain
            let dragged = draggingRow.flatMap { r in history?.commitID(row: editSource[r] ?? r) }
            let ids = chain.compactMap(Data.init(hex:))
            dropTargets = dragged.map { ids.contains($0) } == true ? Set(ids) : []
            redrawText()
        }
    }

    func tableView(_ tableView: NSTableView, draggingSession session: NSDraggingSession, movedTo screenPoint: NSPoint) {
        // 窓の外では、落とすと drop にする（戻る絵を出さず、消える形のカーソル）。
        let outside = draggedOutside(screenPoint) && canDropDragged(session)
        session.animatesToStartingPositionsOnCancelOrFail = !outside
        if outside { NSCursor.disappearingItem.set() }
    }

    func tableView(_ tableView: NSTableView, draggingSession session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) {
        let oid = session.draggingPasteboard.string(forType: .commitID)
        // 窓の外へ落とした（Esc で取り消したのではない）なら、そのコミットを drop にする（Apply まで変えない）。
        let dropping = operation.isEmpty && draggedOutside(screenPoint) && canDropDragged(session)
            && NSApp.currentEvent?.type == .leftMouseUp
        let chain = dragChain
        dragChain = nil
        dropTargets = nil
        draggingRow = nil
        renderer.hiddenRow = nil
        redrawGraph()
        redrawText()
        guard dropping, let oid else { return }
        NSCursor.arrow.set()
        NSAnimationEffect.disappearingItemDefault.show(centeredAt: screenPoint, size: .zero)
        if let editor {
            editor.choose(.drop, for: oid)
            return
        }
        guard let actions else { return }
        Task { @MainActor in
            let chain = if let chain { chain } else { await actions.editableCommits() }
            guard chain.contains(oid) else { return NSSound.beep() }
            self.onEditCommits?(oid, .drop(oid))
        }
    }

    /// `dragged` を表の行 `row` に重ねて落としたときに、まとめる先のコミット（16 進）。まとめられなければ nil
    /// （同じコミット、merge、編集の範囲・編集できる並びの外。並びを読み終えるまでは、落としてから確かめる）。
    private func squashTarget(_ dragged: String, row: Int) -> String? {
        if let editor {
            guard let slot = editSlots[row], editor.rows.indices.contains(slot) else { return nil }
            let target = editor.rows[slot]
            return target.oid != dragged && !target.merge ? target.oid : nil
        }
        guard let history, row < history.rowCount,
              let target = history.commitID(row: row).map({ $0.map { String(format: "%02x", $0) }.joined() }),
              target != dragged else { return nil }
        if let chain = dragChain, !(chain.contains(dragged) && chain.contains(target)) { return nil }
        return target
    }

    /// 窓の外にいる。
    private func draggedOutside(_ screenPoint: NSPoint) -> Bool {
        view.window.map { !$0.frame.contains(screenPoint) } ?? false
    }

    /// 運んでいるコミットを drop にできる（編集の間は範囲の merge でない行、外では編集できる並びの行。並びを読み
    /// 終えるまでは、できるものとして扱う）。
    private func canDropDragged(_ session: NSDraggingSession) -> Bool {
        guard let oid = session.draggingPasteboard.string(forType: .commitID) else { return false }
        if let editor { return editor.choices(for: oid).contains(.drop) }
        return dragChain.map { $0.contains(oid) } ?? true
    }

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> (any NSPasteboardWriting)? {
        // いくつか選んだ行の 1 つをつかんだら、つかんだ行だけを動かす（落とす側は 1 つのコミットしか読まない）。
        let grabbed = self.tableView.mouseDownRow
        if grabbed >= 0, row != grabbed, tableView.selectedRowIndexes.contains(grabbed) { return nil }
        // 編集の間は、範囲の行だけを並べ替えられる（merge をまたがないとき）。
        if let editor, editSlots[row] == nil || !editor.reorderable { return nil }
        guard let id = history?.commitID(row: editSource[row] ?? row) else { return nil }
        let item = NSPasteboardItem()
        item.setString(id.map { String(format: "%02x", $0) }.joined(), forType: .commitID)
        return item
    }

    func tableView(
        _ tableView: NSTableView, validateDrop info: any NSDraggingInfo, proposedRow row: Int,
        proposedDropOperation op: NSTableView.DropOperation
    ) -> NSDragOperation {
        guard info.draggingSource as? NSTableView === tableView, let history else { return [] }
        // 行の上に重ねたら、そのコミットにまとめる（squash）。まとめられない行なら、行の間へ並べ替える。
        if op == .on, let dragged = info.draggingPasteboard.string(forType: .commitID), squashTarget(dragged, row: row) != nil {
            return .move
        }
        if editor != nil {
            // 範囲の中か、範囲のすぐ下へ。
            guard let last = editSlots.keys.max(), editSlots[row] != nil || row == last + 1 else { return [] }
            tableView.setDropRow(row, dropOperation: .above)
            return .move
        }
        // 編集できる並びの外（最初の merge より下、HEAD でない枝）には落とせない。並びを読み終えるまでは許す。
        if let chain = dragChain {
            let hex = { (r: Int) in history.commitID(row: r).map { $0.map { String(format: "%02x", $0) }.joined() } }
            let dragged = info.draggingPasteboard.string(forType: .commitID)
            let below = hex(min(row, history.rowCount - 1))
            let above = row > 0 ? hex(row - 1) : nil
            guard let dragged, chain.contains(dragged),
                  below.map(chain.contains) == true || above.map(chain.contains) == true else { return [] }
        }
        tableView.setDropRow(row, dropOperation: .above)
        return .move
    }

    func tableView(
        _ tableView: NSTableView, acceptDrop info: any NSDraggingInfo, row: Int, dropOperation: NSTableView.DropOperation
    ) -> Bool {
        guard let actions, let history, let oid = info.draggingPasteboard.string(forType: .commitID) else { return false }
        if dropOperation == .on {
            guard let target = squashTarget(oid, row: row) else { return false }
            if let editor {
                editor.squash(oid, onto: target)
                return true
            }
            // 範囲は、2 つの古い方から HEAD まで。
            Task {
                let chain = await actions.editableCommits()
                guard let from = chain.firstIndex(of: oid), let to = chain.firstIndex(of: target) else { return NSSound.beep() }
                await MainActor.run { self.onEditCommits?(chain[max(from, to)], .squash(oid, onto: target)) }
            }
            return true
        }
        if let editor {
            guard let from = editor.rows.firstIndex(where: { $0.oid == oid }) else { return false }
            let target = editSlots[row] ?? editor.rows.count
            editor.move(oid, to: from < target ? target - 1 : target)
            return true
        }
        // 落とした位置の下にあるコミット（なければ上）が、編集できる並びの何番目かで移動先を決める。
        let below = history.commitID(row: min(row, history.rowCount - 1)).map { $0.map { String(format: "%02x", $0) }.joined() }
        Task {
            let chain = await actions.editableCommits()
            guard let from = chain.firstIndex(of: oid), let below, var to = chain.firstIndex(of: below) else { return NSSound.beep() }
            if row >= history.rowCount { to = chain.count - 1 } else if from < to { to -= 1 }
            if from == to { return }
            // 並べ替えも編集モードで（並びを見て Apply）。範囲は、動かす 2 か所の古い方から HEAD まで。
            await MainActor.run { self.onEditCommits?(chain[max(from, to)], .move(oid, to)) }
        }
        return true
    }

    // MARK: - History の上で編集する

    /// 編集している範囲（nil なら編集していない）。
    private var editor: HistoryEditController?
    private var scopeBeforeEdit: Scope?
    private var matchOnlyBeforeEdit = false
    /// 表の行 → 範囲の何番目（新しい順）。
    private var editSlots: [Int: Int] = [:]
    /// 表の行 → そこに描くコミットの History の行（並べ替えた分）。
    private var editSource: [Int: Int] = [:]
    /// first-parent の並びの各コミットの History の行（ない所は -1）。境目を引く先。
    private var lineRows: [Int] = []
    /// 見えている行の、編集の札の横の範囲（表の座標）。押すとすることを選ぶ。
    private var editChips: [Int: ClosedRange<CGFloat>] = [:]
    /// 境目を引いている間の、範囲の数。
    private var boundaryPreview: Int?
    private var editSubjects: [String: TextRenderer.ShapedLine] = [:]

    /// 範囲のいちばん下の行（境目を描く）。
    private var editBoundaryRow: Int? {
        guard let editor else { return nil }
        let count = boundaryPreview ?? editor.rows.count
        guard count > 0, count <= lineRows.count, lineRows[count - 1] >= 0 else { return editSlots.keys.max() }
        return lineRows[count - 1]
    }

    /// 編集を始める。今のブランチの全部の行（たたまない・絞り込まない）にして、終えたら元に戻す。
    func beginEditing(_ editor: HistoryEditController) {
        self.editor = editor
        editor.onChange = { [weak self] in self?.rebuildEditMap() }
        scopeBeforeEdit = scope
        matchOnlyBeforeEdit = matchOnlyButton.state == .on
        let reload = scope != .current || foldsAny || matchOnlyBeforeEdit
        matchOnlyButton.state = .off
        updateMatchButtons()
        scopePopup.isEnabled = false
        if reload {
            scope = .current
            updateScopePopup()
            reloadScope(resetScroll: false)
        }
        rebuildEditMap()
    }

    /// 編集を終える（Apply・Cancel）。元の表示に戻す。
    func endEditing() {
        editor = nil
        editSlots = [:]
        editSource = [:]
        lineRows = []
        editSubjects = [:]
        scopePopup.isEnabled = true
        if let saved = scopeBeforeEdit {
            let restoreMatch = matchOnlyBeforeEdit
            scope = saved
            updateScopePopup()
            if restoreMatch {
                afterShow = { [weak self] in
                    self?.matchOnlyButton.state = .on
                    self?.matchOnlyClicked()
                }
            }
            reloadScope(resetScroll: false)
        }
        scopeBeforeEdit = nil
        redrawText()
        // 並べ替えて描いていた行から、元の行に戻る（詳細を選んでいる行のコミットに合わせる）。
        notifySelection()
    }

    /// 範囲の行と描くコミットを求め直す（並べ替え・範囲・History が変わったとき）。
    private func rebuildEditMap() {
        editSlots = [:]
        editSource = [:]
        guard let editor, let history else { return redrawText() }
        let rowOf = { (hex: String) -> Int? in Data(hex: hex).flatMap { Self.locate($0, near: 0, in: history) } }
        let slots = editor.rows.compactMap { rowOf($0.oid) }.sorted()
        for (i, slot) in slots.enumerated() where i < editor.rows.count {
            editSlots[slot] = i
            editSource[slot] = rowOf(editor.rows[i].oid)
        }
        lineRows = editor.line.map { rowOf($0.oid) ?? -1 }
        redrawText()
        notifySelection()
    }

    private func editChipColors(_ action: HistoryEditController.Action, conflict: Bool) -> (SIMD4<Float>, SIMD4<Float>) {
        if conflict { return (NSColor.systemOrange.withAlphaComponent(0.85).rgba, NSColor.white.rgba) }
        return switch action {
        case .pick: (NSColor.labelColor.withAlphaComponent(0.08).rgba, NSColor.secondaryLabelColor.rgba)
        case .drop: (NSColor.systemRed.withAlphaComponent(0.8).rgba, NSColor.white.rgba)
        default: (NSColor.labelColor.withAlphaComponent(0.7).rgba, NSColor.textBackgroundColor.rgba)
        }
    }

    private func shapedChip(_ action: HistoryEditController.Action, scale: CGFloat) -> TextRenderer.ShapedLine {
        let key = "edit:" + action.rawValue
        if let line = labelText[key] { return line }
        let line = text.shape(action.rawValue as CFString, font: labelFont, scale: scale)
        labelText[key] = line
        return line
    }

    private func editChipWidth(_ action: HistoryEditController.Action, scale: CGFloat) -> CGFloat {
        (shapedChip(action, scale: scale).width + 12).rounded()
    }

    /// 編集の札を描き、幅を返す。
    @discardableResult
    private func drawEditChip(_ action: HistoryEditController.Action, x: CGFloat, baseline: CGFloat, scale: CGFloat,
                              conflict: Bool) -> CGFloat {
        let ascent = CTFontGetAscent(labelFont), descent = CTFontGetDescent(labelFont)
        let line = shapedChip(action, scale: scale)
        let width = editChipWidth(action, scale: scale)
        let (fill, ink) = editChipColors(action, conflict: conflict)
        text.addRoundedRect(CGRect(x: x, y: (baseline - ascent - 2).rounded(), width: width, height: (ascent + descent + 4).rounded()),
                            radius: 8, color: fill)
        text.addLine(line, origin: CGPoint(x: x + 6, y: baseline - 1), color: ink, scale: scale)
        return width
    }

    private func editSubject(_ subject: String, scale: CGFloat) -> TextRenderer.ShapedLine {
        if let line = editSubjects[subject] { return line }
        let line = text.shape(subject as CFString, font: font, scale: scale)
        editSubjects[subject] = line
        return line
    }

    /// 編集の間に押した: 札ならすることの一覧、境目ならそれを引く、範囲の行のダブルクリックはメッセージを直す。
    private func handleEditClick(row: Int, point: NSPoint) -> Bool {
        guard let editor else { return false }
        if let boundary = editBoundaryRow, abs(point.y - tableView.rect(ofRow: boundary).maxY) <= 4 {
            dragBoundary()
            return true
        }
        guard let index = editSlots[row] else { return false }
        let oid = editor.rows[index].oid
        if let chip = editChips[row], chip.contains(point.x) {
            let menu = NSMenu()
            addEditChoices(to: menu, oid: oid)
            menu.popUp(positioning: nil, at: NSPoint(x: chip.lowerBound, y: tableView.rect(ofRow: row).maxY), in: tableView)
            return true
        }
        if NSApp.currentEvent?.clickCount == 2, !editor.rows[index].merge {
            editor.editMessage(of: oid)
            return true
        }
        return false
    }

    /// 境目を、押したまま引いた先の first-parent のコミットまで動かす（離したら範囲を変える）。
    private func dragBoundary() {
        guard let editor, let window = view.window else { return }
        window.trackEvents(matching: [.leftMouseDragged, .leftMouseUp], timeout: .infinity, mode: .eventTracking) { event, stop in
            guard let event else {
                stop.pointee = true
                return
            }
            let y = tableView.convert(event.locationInWindow, from: nil).y
            // 引いた所にいちばん近い、並びのコミットの行の下端。
            var best: (count: Int, distance: CGFloat)?
            for (k, row) in lineRows.enumerated() where row >= 0 {
                let d = abs(tableView.rect(ofRow: row).maxY - y)
                if best == nil || d < best!.distance { best = (k + 1, d) }
            }
            if let count = best?.count, count >= editor.rows.count || editor.canShrink(to: count) {
                boundaryPreview = count
                redrawText()
            }
            if event.type == .leftMouseUp { stop.pointee = true }
        }
        guard let count = boundaryPreview else { return }
        Task { @MainActor in
            await editor.resize(to: count)
            boundaryPreview = nil
            rebuildEditMap()
        }
    }

    /// することの一覧（git の言葉と説明）。今のものに印。
    private func addEditChoices(to menu: NSMenu, oid: String) {
        guard let editor, let row = editor.rows.first(where: { $0.oid == oid }) else { return }
        for action in editor.choices(for: oid) {
            let item = ClosureMenuItem(title: action.menuTitle) { editor.choose(action, for: oid) }
            item.state = row.action == action ? .on : .off
            item.toolTip = action.toolTip
            menu.addItem(item)
        }
    }

    /// 編集の間の右クリック: 範囲の行はすることの一覧、ほかはコピーだけ。
    private func buildEditMenu(_ menu: NSMenu, row: Int, id: Data) {
        let hex = id.map { String(format: "%02x", $0) }.joined()
        if let index = editSlots[row], let editor, !editor.rows[index].merge {
            addEditChoices(to: menu, oid: hex)
            menu.addItem(.separator())
        }
        menu.addItem(ClosureMenuItem(title: Titles.copyCommitID) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(hex, forType: .string)
        })
    }

    /// 選んだ範囲の行で p・r・s・f・d・e を押すと、することを替える。
    private func handleEditKey(_ event: NSEvent) -> Bool {
        guard let editor, event.modifierFlags.intersection([.command, .control, .option]).isEmpty,
              let index = editSlots[tableView.selectedRow], let key = event.charactersIgnoringModifiers?.lowercased()
        else { return false }
        let actions: [String: HistoryEditController.Action] = [
            "p": .pick, "r": .reword, "s": .squash, "f": .fixup, "d": .drop, "e": .edit,
        ]
        guard let action = actions[key] else { return false }
        editor.choose(action, for: editor.rows[index].oid)
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

/// History の行の ref の札の置き方。名前は `names` 個までで、残りと入りきらない分は「+N」にまとめる。
enum LabelFit {
    static let names = 3
    /// 切り詰めた札に残す幅（これより狭くはしない）。
    static let minimum: CGFloat = 40

    enum Content: Equatable {
        /// `labels` の中の位置。
        case name(Int)
        /// まとめた札の数（「+N」）。
        case rest(Int)
    }

    struct Slot: Equatable {
        var content: Content
        var x: CGFloat
        /// 札の幅。名前の幅より狭ければ、名前を「…」で切る。
        var width: CGFloat
    }

    /// `widths` は先頭から `names` 個までの札の幅、`count` は札の総数、`rest` は「+N」の札の幅。
    /// 札は `maxX` を越えない。1 つ目が入らなければ切り詰め、2 つ目からは入らなければ「+N」にまとめる
    /// （「+N」の分の幅は先に取っておくので、まとめた札は必ず出る）。
    static func place(
        widths: [CGFloat], count: Int, start: CGFloat, maxX: CGFloat, gap: CGFloat = 4, rest: (Int) -> CGFloat
    ) -> [Slot] {
        var slots: [Slot] = []
        var x = start
        for (i, width) in widths.prefix(names).enumerated() {
            let after = count - i - 1
            let room = maxX - x - (after > 0 ? gap + rest(after) : 0)
            if width > room {
                if i > 0 {
                    slots.append(Slot(content: .rest(count - i), x: x, width: rest(count - i)))
                    return slots
                }
                let clipped = min(width, max(minimum, room))
                slots.append(Slot(content: .name(0), x: x, width: clipped))
                if after > 0 { slots.append(Slot(content: .rest(after), x: x + clipped + gap, width: rest(after))) }
                return slots
            }
            slots.append(Slot(content: .name(i), x: x, width: width))
            x += width + gap
        }
        if count > widths.prefix(names).count {
            let n = count - widths.prefix(names).count
            slots.append(Slot(content: .rest(n), x: x, width: rest(n)))
        }
        return slots
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

    nonisolated static func prepare(
        row: Int, chunk: HistoryChunk, history: History?, fonts: Fonts, compact: Bool, items: HistoryItems
    ) -> Row {
        let subject = chunk.withSubject(row: row) { TextRenderer.prepare($0, font: fonts.subject) }
        // 作者・日時・ID のうち出すもの。1 行では件名に幅を譲り、短い日時にして詰めて並べる。
        var parts: [String] = []
        if items.contains(.author) { parts.append(chunk.authorName(row: row)) }
        if items.contains(.date) { parts.append((compact ? shortDate : date).string(from: chunk.authorTime(row: row))) }
        if items.contains(.id), let id = history?.commitID(row: row) {
            parts.append(id.prefix(4).map { String(format: "%02x", $0) }.joined())
        }
        let shown = parts.joined(separator: compact ? "  " : "  ·  ")
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
