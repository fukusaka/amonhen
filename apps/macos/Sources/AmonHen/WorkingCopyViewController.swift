import AmonhenKit
import AppKit

/// Working Copy: 左に変更のあるファイルの 1 つの一覧（チェックボックスで次のコミットに含めるか。上下いっぱい）。
/// 右の上にコミットの入力（Summary だけに畳める）、その下に選んだファイルの差分（行を選んで stage・unstage）。
/// チェックボックスは index の状態そのもので、FE だけの選択状態は持たない（ターミナルで `git add` しても揃う）。
final class WorkingCopyViewController: NSSplitViewController, NSOutlineViewDataSource, NSOutlineViewDelegate,
    NSTextViewDelegate, NSMenuDelegate, NSSearchFieldDelegate, NSMenuItemValidation
{
    /// ファイルの右クリック: 作業ツリーの内容の blame（パス）とファイルの履歴（パス）。
    var onBlame: ((String) -> Void)?
    var onFileHistory: ((String) -> Void)?
    /// 「一部のフォルダだけ取り出し中」を押した（Repository Settings の取得とフォルダを開く）。
    var onShowFolders: (() -> Void)?
    private let repository: Repository
    private let diffView: DiffViewController
    private let files = KeyOutlineView()
    private let filesScroll = NSScrollView()
    /// コミットメッセージの 1 行目と、2 行目以降。
    private let summary = NSTextField()
    private let summaryCount = NSTextField(labelWithString: "")
    private let message = PlaceholderTextView()
    /// ファイルの一覧をディレクトリの木で見せるか。
    private let layoutControl = NSSegmentedControl()
    /// 一覧の見せ方: パスの一覧・ディレクトリの木・ファイル名（親のディレクトリは薄く）。
    private enum Layout: Int { case list, tree, flat }
    /// この Working Copy の見せ方（タブ・worktree ごと。ほかのタブの一覧は、そのボタンと合ったまま変わらない）。
    /// 変えたら覚えて、次に開く Working Copy の最初の見せ方にする。
    private var layout: Layout = WorkingCopyViewController.savedLayout {
        didSet { UserDefaults.standard.set(layout.rawValue, forKey: "workingCopyLayout") }
    }
    private static var savedLayout: Layout {
        if let raw = UserDefaults.standard.object(forKey: "workingCopyLayout") as? Int { return Layout(rawValue: raw) ?? .list }
        return UserDefaults.standard.bool(forKey: "workingCopyTree") ? .tree : .list
    }
    private var showsTree: Bool { layout == .tree }
    /// 一覧を git status のように Staged・Unstaged の節に分ける（Mixed なら 1 つの一覧）。覚える。
    private var showsSections = UserDefaults.standard.bool(forKey: "workingCopySections") {
        didSet { UserDefaults.standard.set(showsSections, forKey: "workingCopySections") }
    }
    private let sectionsButton = NSButton()
    /// ファイルごとの追加・削除の行数（HEAD と作業ツリー。untracked はない）。
    private var stats: [String: (added: Int?, removed: Int?)] = [:]
    /// 節分けの一覧の、節ごとの行数（Staged は HEAD と index、Unstaged は index と作業ツリー）。
    private var splitStats: (staged: [String: (added: Int?, removed: Int?)], unstaged: [String: (added: Int?, removed: Int?)])?
    /// 一覧の見出し: すべてを含める・外すチェックボックスと、含める数。
    private let allCheckbox = NSButton(checkboxWithTitle: "", target: nil, action: nil)
    /// 一覧に出すもの: 変更のあるファイルか、作業ツリーのすべてのファイル（History と同じ切り替え）。
    private let contentSwitch = NSSegmentedControl(
        labels: [L("Changes"), L("All Files")], trackingMode: .selectOne, target: nil, action: nil)
    private lazy var treeFiles = WorkingTreeFiles(repository: repository)
    private var showsAllFiles: Bool { contentSwitch.selectedSegment == 1 }
    /// すべてのファイルで開いている、変えていないファイル（変えたファイルは `selectedKey`）。
    private var treeFilePath: String?
    /// 一覧の見出しの 2 段目（すべてを含める・外すと、含める数）。すべてのファイルでは隠す。
    private let includeRow = NSStackView()
    private let countLabel = NSTextField(labelWithString: "")
    /// sparse checkout のとき、一覧の上に薄く出す「一部のフォルダだけ取り出し中」（押すと取得とフォルダのページ）。
    private let sparseNote = NSButton(title: "", target: nil, action: nil)
    /// パスで一覧を絞り込む。
    private let filterField = NSSearchField()
    /// 一覧が空のときの説明。最初の status を読むまでは空（「変更なし」と言わない）。
    private let emptyLabel = NSTextField(labelWithString: "")
    private var diffBar: TitleAndButtonsBar?
    /// 行を比べるときに無視する空白。無視している間は、1 本の差分（行ごとのチェック）にせず、行ごとの stage・
    /// 破棄もできない。覚えない。
    private let whitespacePopUp = WhitespacePopUp()
    /// 差分の表示の設定（長い行を折り返す）。
    private let wrapButton = WrapLinesButton()
    /// ファイル全体を表示（差分の前後を、変わっていない行も含めてファイル全体にする）。
    private let wholeFileButton = NSButton()
    /// 差分の中を探す（⌘F）。
    private let findButton = NSButton()
    /// 帯の右端の表示の設定（コード > 長い行を折り返す・ファイル全体を表示、コミットメッセージ > 畳む）。
    private lazy var viewOptions = ViewOptionsPopUp { [weak self] menu in self?.fillViewOptions(menu) }
    /// ファイルごとの、変更のない所（隙間）の開き方と、隙間の数・まだ隠している行のある隙間の数。覚えない。
    private var openGaps: [String: [Int: GapOpening]] = [:]
    private var gapCounts: [String: (all: Int, folded: Int)] = [:]
    /// 画像のファイルの比べ方（History と同じ 4 つ）。
    private var imageMode: ImageDiffPanel.Mode = .twoUp
    /// 差分の代わりに出している画像の比較。
    private var imagePanel: ImageDiffPanel?
    /// SVG をテキストの差分で見ているファイル。
    private var svgAsText: Set<String> = []
    private let amend = NSButton(checkboxWithTitle: "Amend", target: nil, action: nil)
    private let commitButton = YieldingButton(title: "Commit", target: nil, action: nil)
    private let suggestButton = NSButton(title: "", target: nil, action: nil)
    /// 過去のメッセージの一覧・型や Co-authored-by の挿入・Commit の続き（Commit and Push など）。
    private let historyButton = NSPopUpButton(frame: .zero, pullsDown: true)
    private let insertButton = NSPopUpButton(frame: .zero, pullsDown: true)
    private let commitMenuButton = NSPopUpButton(frame: .zero, pullsDown: true)
    /// 最近のコミット（メッセージ・作者）。⌥↑・⌥↓ と一覧で使う。
    private var recentCommits: [(message: String, author: String, email: String)] = []
    /// ⌥↑・⌥↓ で今出している過去のメッセージ（-1 は自分で書いているもの）。
    private var recentCursor = -1
    /// ⌥↑ を押す前に書いていたもの（⌥↓ で戻ってきたら戻す）。
    private var draftBeforeRecall: (String, String)?
    /// 一部だけ stage したファイルで、作業ツリーの側と stage 済みの側のどちらの差分を見せるか。
    private let sideControl = NSSegmentedControl(labels: ["Unstaged", "Staged", "All"], trackingMode: .selectOne, target: nil, action: nil)
    /// 差分で見る側（git の index → working tree・HEAD → index・HEAD → working tree）。Working Copy で 1 つ。
    private enum DiffSide: Int { case unstaged, staged, all }
    /// 確かめる用（`--snapshot`）に `-AMHWorkingCopySide 1` で最初の側を決められる（0 Unstaged・1 Staged・2 All）。
    private var diffSide = DiffSide(rawValue: UserDefaults.standard.integer(forKey: "AMHWorkingCopySide")) ?? .unstaged
    private let linesButton = NSButton(title: "Stage Lines", target: nil, action: nil)
    private let discardButton = NSButton(title: "Discard Lines", target: nil, action: nil)
    var actions: Actions?
    private let titleLabel = NSTextField(labelWithString: "")
    /// 改行を変換している・改行が混ざっているファイルの印（差分には出ないので）。
    private let lineEndingsLabel = NSTextField(labelWithString: "")
    /// merge・rebase などの途中のときに出す帯。
    private let sequenceBar = NSStackView()
    /// 右の列の上下の仕切り（コミットの入力と、差分）。NSSplitViewController 自身を delegate にすると
    /// 自分の分割項目と照合して崩れるので、専用の delegate を持つ。
    private let messageSplit = NSSplitView()
    private let messageSplitDelegate = MessageSplitDelegate()
    /// コミットの入力の行: Summary（畳んだときは Commit も）と、その下のボタンの行。
    private let summaryRow = NSStackView()
    private let commitRow = NSStackView()
    private let messageScroll = NSScrollView()
    /// コミットの入力を畳む・開く（畳むと Summary と Commit の 1 行）。
    private let collapseButton = NSButton()
    /// コミットの入力を畳んでいる（覚える）。
    private var commitCollapsed = UserDefaults.standard.bool(forKey: "workingCopyCommitCollapsed") {
        didSet { UserDefaults.standard.set(commitCollapsed, forKey: "workingCopyCommitCollapsed") }
    }
    /// 畳む前の仕切りの位置（開いたら戻す）。
    private var expandedMessageHeight: CGFloat?
    private let sequenceLabel = NSTextField(labelWithString: "")

    /// 一覧の 1 ファイル。
    private final class Entry {
        let item: StatusItem
        /// 節分けの一覧で、どの節の行か（Mixed とコンフリクトは nil）。一部だけ stage したファイルは両方の節に出る。
        let section: DiffSide?
        init(item: StatusItem, section: DiffSide? = nil) {
            self.item = item
            self.section = section
        }
        var isConflict: Bool { item.kind == .unmerged }
        /// 次のコミットに含めるか（index の状態）。節では、Staged の行は含める、Unstaged の行は含めない。
        var check: NSControl.StateValue {
            if isConflict { return .off }
            switch section {
            case .staged: return .on
            case .unstaged: return .off
            default:
                if !item.isStaged { return .off }
                return item.isUnstaged ? .mixed : .on
            }
        }
    }

    /// 節分けの一覧の節（git status の Unmerged paths・Changes to be committed・Changes not staged）。
    private final class Section {
        /// nil なら Unmerged。
        let side: DiffSide?
        let entries: [Entry]
        let tree: Dir?
        init(side: DiffSide?, entries: [Entry], tree: Dir?) {
            self.side = side
            self.entries = entries
            self.tree = tree
        }
        var title: String {
            switch side {
            case .staged: "Staged"
            case .unstaged: "Unstaged"
            default: "Unmerged"
            }
        }
    }
    private var sections: [Section]?

    private var status: WorkingStatus?
    /// 一覧に出すファイル（コンフリクトが先、あとはパスの順。絞り込み済み）。
    private var entries: [Entry] = []
    /// 絞り込む前のすべてのファイル。
    private var allEntries: [Entry] = []
    /// コンフリクトの解決のボタン。
    private let conflictButtons = NSStackView()

    /// 木で見せるときのディレクトリ。子が 1 つのディレクトリだけなら名前をつないで 1 段にする。
    private final class Dir {
        var name: String
        /// リポジトリの中のパス（畳んだディレクトリを読み直しても畳んだままにするため）。
        var path: String
        var dirs: [Dir] = []
        var files: [Entry] = []
        init(name: String, path: String) {
            self.name = name
            self.path = path
        }
        /// 配下のすべてのファイル。
        var allFiles: [Entry] { files + dirs.flatMap(\.allFiles) }
    }
    private var tree: Dir?
    /// 木で畳んだディレクトリ（パス）。読み直しても畳んだままにする。
    private var collapsedDirs: Set<String> = []
    /// 読み直した木を開いている途中（開いた・畳んだの知らせを、使う人の操作として覚えない）。
    private var isExpandingTree = false
    private var current: WorkingFileDiff?
    /// 一覧で 2 つ以上選んだとき、右に並べている差分（選んだファイルのパスと、つないだ差分）。
    private var multi: (paths: [String], stacked: WorkingStackedDiff?)?
    /// 並べた差分で畳んだファイル（パス）。
    private var multiCollapsed: Set<String> = []
    /// 選んでいたファイルと、見せていた側（読み直した後に選び直す）。
    private var selectedKey: (path: String, staged: Bool)?
    /// 読み直した後に選び直している途中（選択の変化で差分を替えない）。
    private var isRestoringSelection = false
    private var request = 0
    private var isConflict = false
    /// 右の欄（帯と、差分かマージエディタ）。
    private var rightView: BarAndContentView?
    private var rightController: NSViewController?
    /// コンフリクトのファイルごとのマージエディタ（ほかのファイルに移っても、編集中の内容を保つ）。
    private var mergeEditors: [String: MergeEditorViewController] = [:]
    private var embeddedEditor: MergeEditorViewController?
    /// 開いたときの自動の解決を済ませたファイル（コンフリクトの間は、読み直しても 1 回だけ）。
    private var autoAppliedPaths: Set<String> = []

    init(repository: Repository, renderer: TextRenderer) {
        self.repository = repository
        self.diffView = DiffViewController(renderer: renderer)
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        splitView.isVertical = true

        // 左の上: コミットの入力。
        message.isRichText = false
        // ⌘Z で打ったものを戻せるように（コードで作った NSTextView は既定で戻せない）。
        message.allowsUndo = true
        message.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        message.isAutomaticQuoteSubstitutionEnabled = false
        message.isAutomaticDashSubstitutionEnabled = false
        message.delegate = self
        message.textContainerInset = NSSize(width: 4, height: 4)
        message.placeholder = L("Description (optional)")
        summary.placeholderString = "Summary"
        summaryCount.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        summaryCount.textColor = .tertiaryLabelColor
        summaryCount.alignment = .right
        summary.font = .systemFont(ofSize: 13)
        summary.target = self
        summary.action = #selector(summaryChanged)
        NotificationCenter.default.addObserver(
            self, selector: #selector(summaryChanged), name: NSControl.textDidChangeNotification, object: summary)
        messageScroll.documentView = message
        messageScroll.hasVerticalScroller = true
        messageScroll.borderType = .bezelBorder
        message.autoresizingMask = [.width]
        message.isVerticallyResizable = true
        message.textContainer?.widthTracksTextView = true

        commitButton.target = self
        commitButton.action = #selector(commitClicked)
        commitButton.keyEquivalent = "\r"
        commitButton.keyEquivalentModifierMask = .command
        // 埋め込んだマージエディタを操作している間の ⌘↩ は、エディタの Save & Mark Resolved。
        commitButton.yieldsKeyEquivalent = { [weak self] in self?.embeddedEditor?.hasFocus == true }
        commitButton.toolTip = L("Commit the checked files (⌘↩)")
        amend.target = self
        amend.action = #selector(toggleAmend)
        suggestButton.image = AppIcons.image(symbol: "sparkles", label: L("Suggest a message with AI"))
        suggestButton.toolTip = L("AI: suggest a commit message from the staged changes")
        suggestButton.target = self
        suggestButton.action = #selector(suggestMessage)
        suggestButton.isHidden = !Assistant.isConfigured
        NotificationCenter.default.addObserver(
            self, selector: #selector(assistantChanged), name: .assistantChanged, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(mergeWindowClosed(_:)), name: .mergeWindowClosed, object: nil)
        for (button, symbol, tip) in [
            (historyButton, "clock.arrow.circlepath", L("Insert the message of a recent commit (⌥↑ and ⌥↓ in the Summary field also work)")),
            (insertButton, "plus.circle", L("Insert a Conventional Commits type, Co-authored-by or Signed-off-by")),
        ] {
            button.addItem(withTitle: "")
            button.item(at: 0)?.image = AppIcons.image(symbol: symbol, label: tip)
            button.isBordered = false
            button.controlSize = .small
            button.toolTip = tip
            (button.cell as? NSPopUpButtonCell)?.arrowPosition = .noArrow
        }
        commitMenuButton.addItem(withTitle: "")
        commitMenuButton.controlSize = .small
        commitMenuButton.isBordered = false
        commitMenuButton.toolTip = L("Commit and Push (⌘⇧↩), Amend")
        for (title, action) in [("Commit and Push", #selector(commitAndPush)), (L("Amend Last Commit"), #selector(startAmend))] {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = self
            commitMenuButton.menu?.addItem(item)
        }
        buildInsertMenu()
        summary.delegate = self
        message.guideColumn = 72
        restoreDraftOrTemplate()
        loadRecentCommits()
        // 件名の文字数は Commit の横に置く（件名の欄は Description と同じ幅にする）。畳んだときは Summary の行に移す。
        commitRow.spacing = 3
        summaryRow.spacing = 6
        summaryCount.setContentHuggingPriority(.required, for: .horizontal)
        summaryCount.setContentCompressionResistancePriority(.required, for: .horizontal)
        // 狭くても Amend の文字は削らない（足りなければ余白が縮む）。
        amend.setContentCompressionResistancePriority(.required, for: .horizontal)
        summaryCount.toolTip = L("Length of the summary (yellow over 50, orange over 72)")
        summary.setContentHuggingPriority(.init(1), for: .horizontal)
        summary.setContentCompressionResistancePriority(.init(1), for: .horizontal)
        collapseButton.bezelStyle = .accessoryBarAction
        collapseButton.isBordered = false
        collapseButton.controlSize = .small
        collapseButton.imagePosition = .imageOnly
        collapseButton.target = self
        collapseButton.action = #selector(toggleCommitCollapsed)
        collapseButton.setContentHuggingPriority(.required, for: .horizontal)

        sequenceLabel.textColor = .systemOrange
        sequenceLabel.lineBreakMode = .byTruncatingTail
        let buttons = [("Continue", "continue"), ("Skip", "skip"), ("Abort", "abort")].map { title, op in
            let b = NSButton(title: title, target: self, action: #selector(sequenceAction(_:)))
            b.identifier = .init(op)
            return b
        }
        sequenceBar.setViews([sequenceLabel] + buttons, in: .leading)
        sequenceBar.isHidden = true

        // 左の下: 変更のあるファイルの一覧。
        allCheckbox.allowsMixedState = true
        allCheckbox.target = self
        allCheckbox.action = #selector(toggleAll)
        allCheckbox.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        allCheckbox.toolTip = L("Include or exclude all listed files in the next commit")
        countLabel.textColor = .secondaryLabelColor
        countLabel.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        layoutControl.segmentCount = 3
        layoutControl.setImage(AppIcons.image(symbol: "list.bullet", label: L("List")), forSegment: 0)
        layoutControl.setImage(AppIcons.image(symbol: "list.bullet.indent", label: L("Tree")), forSegment: 1)
        layoutControl.setImage(AppIcons.image(symbol: "doc.text", label: L("File Name")), forSegment: 2)
        layoutControl.setToolTip(L("Show files as a list of paths"), forSegment: 0)
        layoutControl.setToolTip(L("Show files as a directory tree"), forSegment: 1)
        layoutControl.setToolTip(L("Show the file name first and the parent directory dimmed"), forSegment: 2)
        layoutControl.controlSize = .small
        layoutControl.selectedSegment = layout.rawValue
        layoutControl.target = self
        layoutControl.action = #selector(layoutChanged)
        contentSwitch.selectedSegment = 0
        contentSwitch.controlSize = .small
        contentSwitch.target = self
        contentSwitch.action = #selector(contentSwitchChanged)
        contentSwitch.setToolTip(L("Changed files, with checkboxes for the next commit"), forSegment: 0)
        contentSwitch.setToolTip(L("Every file in the working tree (ignored files are left out)"), forSegment: 1)
        sectionsButton.setButtonType(.pushOnPushOff)
        sectionsButton.bezelStyle = .accessoryBarAction
        sectionsButton.isBordered = false
        sectionsButton.image = AppIcons.image(symbol: "rectangle.split.1x2", label: L("Staged and Unstaged"))
        sectionsButton.target = self
        sectionsButton.action = #selector(sectionsChanged)
        sectionsButton.state = showsSections ? .on : .off
        sectionsButton.contentTintColor = showsSections ? .controlAccentColor : nil
        sectionsButton.toolTip = L("Split the list into Staged and Unstaged, like git status (a partly staged file is in both)")
        let filesHeader = NSStackView(views: [contentSwitch, NSView(), sectionsButton, layoutControl])
        filesHeader.spacing = 6
        includeRow.setViews([allCheckbox, countLabel], in: .leading)
        includeRow.spacing = 2
        treeFiles.onOpen = { [weak self] path in self?.openTreeFile(path) }
        treeFiles.scroll.isHidden = true
        sparseNote.isBordered = false
        sparseNote.controlSize = .small
        sparseNote.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        sparseNote.contentTintColor = .secondaryLabelColor
        let small = NSFont.smallSystemFontSize
        sparseNote.image = AppIcons.image(symbol: "folder.badge.minus", pointSize: small, config: .init(pointSize: small, weight: .regular))
        sparseNote.imagePosition = .imageLeading
        sparseNote.alignment = .left
        sparseNote.target = self
        sparseNote.action = #selector(showFolders)
        sparseNote.isHidden = true
        filterField.placeholderString = L("Filter by path")
        filterField.controlSize = .small
        filterField.delegate = self

        let column = NSTableColumn(identifier: .init("path"))
        files.addTableColumn(column)
        files.outlineTableColumn = column
        files.headerView = nil
        files.style = .sourceList
        // 節の見出しは張り付かせない（ほかの行と同じに流れる）。
        files.floatsGroupRows = false
        files.allowsMultipleSelection = true
        files.dataSource = self
        files.delegate = self
        // Space か Enter で、選んだファイルを次のコミットに含める・外す。
        files.onToggle = { [weak self] in self?.toggleSelected() }
        // ⌫ か Delete で、選んだファイルの変更を捨てる（確かめてから。docs/ux.md の Working Copy）。
        files.onDiscard = { [weak self] in
            guard let self else { return }
            guard let entry = files.item(atRow: files.selectedRow) as? Entry else { return NSSound.beep() }
            discardFile(entry)
        }
        let fileMenu = NSMenu()
        fileMenu.delegate = self
        files.menu = fileMenu
        filesScroll.documentView = files
        filesScroll.hasVerticalScroller = true
        emptyLabel.textColor = .secondaryLabelColor
        emptyLabel.alignment = .center
        emptyLabel.translatesAutoresizingMaskIntoConstraints = false
        filesScroll.addSubview(emptyLabel)
        emptyLabel.centerXAnchor.constraint(equalTo: filesScroll.centerXAnchor).isActive = true
        emptyLabel.centerYAnchor.constraint(equalTo: filesScroll.centerYAnchor).isActive = true

        // 左の列はファイルの一覧だけ（上下いっぱい）。右の列は上下 2 段: 上にコミットの入力、下に差分。間の仕切りを
        // ドラッグするとメッセージの欄の高さが変わり、位置は覚えておく。窓の高さが変わったときは、差分の側が伸び縮みする。
        let stackOf = { (views: [NSView], top: CGFloat, bottom: CGFloat) -> NSStackView in
            let stack = NSStackView(views: views)
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 6
            stack.edgeInsets = NSEdgeInsets(top: top, left: 8, bottom: bottom, right: 8)
            for v in views {
                v.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -16).isActive = true
            }
            return stack
        }
        let left = stackOf([filesHeader, includeRow, sparseNote, filterField, filesScroll, treeFiles.scroll], 8, 8)
        filesScroll.setContentHuggingPriority(.init(1), for: .vertical)
        treeFiles.scroll.setContentHuggingPriority(.init(1), for: .vertical)
        left.frame = NSRect(x: 0, y: 0, width: 280, height: 500)
        let leftController = NSViewController()
        leftController.view = left
        let leftItem = NSSplitViewItem(viewController: leftController)
        leftItem.minimumThickness = 220
        leftItem.holdingPriority = .init(260)
        addSplitViewItem(leftItem)
        let commitPanel = stackOf([sequenceBar, summaryRow, messageScroll, commitRow], 8, 8)
        messageScroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 48).isActive = true
        messageScroll.setContentHuggingPriority(.init(1), for: .vertical)
        applyCommitCollapsed()

        // 右: 操作ボタンと差分。
        sideControl.target = self
        sideControl.action = #selector(sideChanged)
        sideControl.controlSize = .small
        sideControl.setToolTip(L("Changes in the working tree that are not staged yet"), forSegment: 0)
        sideControl.setToolTip(L("Staged changes (included in the next commit)"), forSegment: 1)
        sideControl.setToolTip(
            L("Every change from HEAD to the working tree (the bar on the left of each line shows whether it’s staged)"),
            forSegment: 2)
        linesButton.target = self
        linesButton.action = #selector(stageLines)
        titleLabel.lineBreakMode = .byTruncatingMiddle
        discardButton.target = self
        discardButton.action = #selector(discard)
        // 行を捨てるのは Undo で戻せるので、赤くしない（赤は元に戻せないものだけ）。
        // コンフリクトのファイルは、下にマージエディタを出す。帯にはファイル全体の操作を置く。
        let conflictActions: [(String, Selector)] = [
            (L("Take Current File"), #selector(takeOursFile)), (L("Take Incoming File"), #selector(takeTheirsFile)),
            (L("Mark Resolved"), #selector(markResolved)),
        ]
        conflictButtons.setViews(conflictActions.map { NSButton(title: $0.0, target: self, action: $0.1) }, in: .leading)
        conflictButtons.isHidden = true
        lineEndingsLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        lineEndingsLabel.isHidden = true
        whitespacePopUp.onChange = { [weak self] _ in self?.whitespaceChanged() }
        wholeFileButton.target = self
        wholeFileButton.action = #selector(toggleWholeFile)
        findButton.image = AppIcons.image(symbol: "magnifyingglass", label: L("Find"))
        findButton.toolTip = L("Find in the diff (⌘F)")
        findButton.target = self
        findButton.action = #selector(findInDiff)
        for button in [wholeFileButton, findButton] {
            button.bezelStyle = .accessoryBarAction
            button.controlSize = .small
            button.isBordered = false
            button.imagePosition = .imageOnly
        }
        updateWholeFileButton()
        // 見せ方（ファイル全体・空白・折り返し・検索・表示の設定）を先に、ファイルの操作を右端に。
        let viewButtons = NSStackView(views: [lineEndingsLabel, wholeFileButton, whitespacePopUp, wrapButton, findButton, viewOptions])
        viewButtons.spacing = 8
        let actionButtons = NSStackView(views: [
            viewButtons, sideControl, conflictButtons, discardButton, linesButton,
        ])
        actionButtons.spacing = 10
        let bar = TitleAndButtonsBar(title: titleLabel, buttons: actionButtons)
        diffBar = bar
        diffView.isSelectable = true
        // 隙間の行のボタン: 変更のない所を開く・畳む。
        diffView.foldsGaps = true
        diffView.onSetGap = { [weak self] gap, opening in self?.setGap(gap, opening) }
        diffView.onSelectionChange = { [weak self] in self?.updateButtons() }
        diffView.onExplainHunk = { [weak self] path, hunk in
            guard let self, Assistant.confirm(repository: repository.workdir ?? "remote", summary: L("One hunk of %@", path)) else { return }
            let workdir = repository.workdir
            AssistantPanel.show(title: L("Hunk Explanation"), over: view.window) { useCache, onText in
                try await Assistant.run(
                    .explainHunk, variables: ["path": path, "hunk": hunk, "message": ""], workdir: workdir,
                    useCache: useCache, onText: onText)
            }
        }
        diffView.onSetStaged = { [weak self] rows, staged in self?.setStaged(rows: rows, staged: staged) }
        diffView.onStageRows = { [weak self] rows in self?.stageRows(rows) }
        diffView.onDiscardRows = { [weak self] rows in self?.discardRows(rows) }
        NotificationCenter.default.addObserver(
            self, selector: #selector(stageStyleChanged), name: .lineStageStyleChanged, object: nil)
        // ⌥J・⌥K: 一覧の次・前のファイルへ。
        diffView.onMoveFile = { [weak self] step in
            guard let self else { return }
            let rows = (0..<files.numberOfRows).filter { files.item(atRow: $0) is Entry }
            let current = rows.firstIndex(of: files.selectedRow) ?? -1
            let next = current + step
            guard rows.indices.contains(next) else { return NSSound.beep() }
            files.selectRowIndexes([rows[next]], byExtendingSelection: false)
            files.scrollRowToVisible(rows[next])
        }
        let right = BarAndContentView(bar: bar, content: diffView.view)
        right.frame = NSRect(x: 0, y: 0, width: 700, height: 400)
        messageSplit.isVertical = false
        messageSplit.dividerStyle = .thin
        messageSplit.addArrangedSubview(commitPanel)
        messageSplit.addArrangedSubview(right)
        messageSplitDelegate.minimum = { [weak self] in self?.messageMinimum ?? 170 }
        messageSplitDelegate.maximum = { [weak self] in
            guard let self else { return 0 }
            return commitCollapsed ? messageMinimum : messageSplit.bounds.height - 200
        }
        messageSplit.delegate = messageSplitDelegate
        messageSplit.frame = NSRect(x: 0, y: 0, width: 700, height: 600)
        messageSplit.autosaveName = "WorkingCopyCommitMessage"
        // NSSplitViewController に addChild すると分割の項目が増えるので、右側の親に付ける。
        let rightController = NSViewController()
        rightController.view = messageSplit
        rightController.addChild(diffView)
        self.rightView = right
        self.rightController = rightController
        let rightItem = NSSplitViewItem(viewController: rightController)
        rightItem.minimumThickness = 300
        addSplitViewItem(rightItem)
        updateButtons()
        // 確かめる用（`--snapshot`）に `-AMHWorkingCopyView all` で、すべてのファイルで開く。
        if UserDefaults.standard.string(forKey: "AMHWorkingCopyView") == "all" {
            contentSwitch.selectedSegment = 1
            contentSwitchChanged()
        }
        // 開く前に裏で読んだ status でコンフリクトのファイルを選んでいたら、そのとき読んだマージエディタをここで出す。
        if let key = selectedKey, let editor = mergeEditors[key.path] { embed(editor) }
    }

    func overlays() -> [(CGImage, NSView)] {
        // マージエディタを埋め込んでいる間は、差分の欄が外れている。
        diffView.view.window == nil ? [] : diffView.overlay().map { [$0] } ?? []
    }

    // MARK: - 読み直し

    /// status を裏で読み直す。
    /// 読み直している途中か。途中に頼まれたら、終わってからもう 1 回だけ読み直す
    /// （ビルドなどで変化の知らせが続いても、status を重ねて走らせない）。
    private var isRefreshing = false
    private var needsRefresh = false

    func refresh() {
        if isRefreshing {
            needsRefresh = true
            return
        }
        isRefreshing = true
        let repository = self.repository
        let started = CACurrentMediaTime()
        Task {
            defer { PerformanceLog.record("Working Copy: reload status", ms: (CACurrentMediaTime() - started) * 1000) }
            async let sequence = Task.detached { try? repository.sequence() }.value
            async let status = Task.detached(priority: .userInitiated) { try? repository.loadStatus() }.value
            let (s, w) = await (sequence, status)
            showSequence(s ?? nil)
            if let w, w.generation != self.status?.generation {
                show(w)
                loadStats()
            } else if w != nil {
                // status は同じでも、変更のあるファイルをさらに書き換えたかもしれない。行数と出している差分は
                // 読み直す（コアは中身が同じなら前回の status を返すので、一覧は作り直さない）。
                loadStats()
                reloadShownDiff()
                // 見張りが先に読んでいたら、ここで選ぶ（残すと、後で別の変更があったときに選びが飛ぶ）。
                if wantsFirstConflict {
                    wantsFirstConflict = false
                    selectFirstConflict()
                }
            }
            loadConflictStates()
            isRefreshing = false
            if needsRefresh {
                needsRefresh = false
                refresh()
            }
        }
    }

    /// 出している差分を読み直し、中身が変わっていれば位置を保って出し直す（コンフリクトはマージエディタのまま）。
    private func reloadShownDiff() {
        if let multi, !showsAllFiles {
            let chosen = multi.paths.compactMap { path in entries.first { $0.item.path == path } }
            if chosen.count >= 2 { return showMulti(chosen, keepScroll: true) }
        }
        if showsAllFiles, let path = treeFilePath ?? selectedKey?.path {
            // 変えていないファイルはそのまま（作業ツリーで書き換えれば status が変わり、作り直す）。
            if treeFilePath == nil { openTreeFile(path, keepScroll: true) }
            return
        }
        guard let key = selectedKey, let entry = entries.first(where: { $0.item.path == key.path }),
              !entry.isConflict else { return }
        showDiff(of: entry, staged: side(for: entry.item), keepScroll: true, onlyIfChanged: true)
    }

    /// 行数を裏で読み、一覧を描き直す（数がなくても一覧は先に出す）。
    private func loadStats() {
        let repository = self.repository
        Task {
            struct Stats: Decodable {
                struct Stat: Decodable { let path: String; let added: Int?; let removed: Int? }
                let stats: [Stat]
            }
            let read = try? await Task.detached(priority: .utility) {
                try repository.info(["op": "workingStats"], as: Stats.self)
            }.value
            guard let read else { return }
            stats = Dictionary(read.stats.map { ($0.path, ($0.added, $0.removed)) }, uniquingKeysWith: { a, _ in a })
            files.reloadData(forRowIndexes: IndexSet(integersIn: 0..<files.numberOfRows), columnIndexes: [0])
            if showsSections { loadSplitStats() }
        }
    }

    /// 節ごとの行数を裏で読み、一覧を描き直す（節分けのときだけ）。
    private func loadSplitStats() {
        let repository = self.repository
        Task {
            struct Split: Decodable {
                struct Stat: Decodable { let path: String; let added: Int?; let removed: Int? }
                let staged: [Stat]
                let unstaged: [Stat]
            }
            let read = try? await Task.detached(priority: .utility) {
                try repository.info(["op": "workingStatsSplit"], as: Split.self)
            }.value
            guard let read else { return }
            let map = { (list: [Split.Stat]) in
                Dictionary(list.map { ($0.path, ($0.added, $0.removed)) }, uniquingKeysWith: { a, _ in a })
            }
            splitStats = (map(read.staged), map(read.unstaged))
            files.reloadData(forRowIndexes: IndexSet(integersIn: 0..<files.numberOfRows), columnIndexes: [0])
        }
    }

    /// 1 ファイルの変更を捨てる（行のホバーのボタン）。作業ツリーを index の内容に戻すので、含めている分は残る。
    /// untracked のファイルを消す前に確かめる（Undo では戻せない）。
    private func confirmDeleteUntracked(_ path: String, actions: Actions) {
        Task { @MainActor in
            guard await actions.confirm(L("Delete %@?", path), message: L("It’s untracked.") + sentenceGap + Actions.cantUndo,
                                        ok: L("Delete"), destructive: true) else { return }
            // ファイルの操作はコアに任せる（SSH 越しのリポジトリでも同じに動く）。
            actions.perform(["op": "deleteUntracked", "paths": [path]])
        }
    }

    /// ファイルの変更を捨てる前に確かめる（Undo で戻せる）。
    private func confirmDiscard(_ path: String, actions: Actions) {
        Task { @MainActor in
            guard await actions.confirm(L("Discard changes to %@?", path), message: Actions.canUndo, ok: "Discard") else { return }
            actions.perform(["op": "discard", "paths": [path]])
        }
    }

    /// untracked のファイルは消す（Undo では戻せないので確かめる）。
    private func discardFile(_ entry: Entry) {
        guard let actions, !entry.isConflict else { return }
        let path = entry.item.path
        if entry.item.kind == .untracked {
            confirmDeleteUntracked(path, actions: actions)
        } else if entry.item.isUnstaged {
            confirmDiscard(path, actions: actions)
        } else {
            NSSound.beep()
        }
    }

    private func show(_ status: WorkingStatus) {
        self.status = status
        // コンフリクトを先に、あとはパスの順。
        // 解決済みになったファイルのエディタは捨てる。
        let conflicted = Set(status.items.filter { $0.kind == .unmerged }.map(\.path))
        mergeEditors = mergeEditors.filter { conflicted.contains($0.key) }
        autoAppliedPaths.formIntersection(conflicted)
        if let editor = embeddedEditor, !conflicted.contains(editor.path) { unembed() }
        allEntries = status.items.map { Entry(item: $0) }.sorted {
            $0.isConflict != $1.isConflict ? $0.isConflict : $0.item.path < $1.item.path
        }
        notifySequence()
        rebuildList()
        if wantsFirstConflict {
            wantsFirstConflict = false
            selectFirstConflict()
        }
    }

    /// 操作がコンフリクトで止まった。読み直したら最初のコンフリクトのファイルを選ぶ（右にマージエディタが出る）。
    private var wantsFirstConflict = false

    func openFirstConflict() {
        wantsFirstConflict = true
    }

    /// 内容のコンフリクトのファイルに残っているマーカーの数と、rerere が解決を当てたか（パスごと）。
    private var conflictStates: [String: ConflictState] = [:]

    private struct ConflictState: Decodable, Equatable {
        let path: String
        let markers: Int
        let rerere: Bool
    }

    /// マーカーの残っていないコンフリクトのファイル（rerere が解決を当てた、ほかのエディタで直した）。
    private var conflictsWithoutMarkers: [String] {
        allEntries.filter { $0.isConflict && conflictStates[$0.item.path]?.markers == 0 }.map(\.item.path)
    }

    /// コンフリクトのファイルの中身を読み直す（index が変わらなくても、ほかのエディタでマーカーを消せば変わる）。
    private func loadConflictStates() {
        guard allEntries.contains(where: \.isConflict) else {
            conflictStates = [:]
            return
        }
        let repository = self.repository
        Task {
            struct Files: Decodable { let files: [ConflictState] }
            let read = try? await Task.detached(priority: .userInitiated) {
                try repository.info(["op": "conflictFiles"], as: Files.self)
            }.value
            guard let read else { return }
            let states = Dictionary(read.files.map { ($0.path, $0) }, uniquingKeysWith: { a, _ in a })
            guard states != conflictStates else { return }
            let old = conflictStates
            conflictStates = states
            files.reloadData(forRowIndexes: IndexSet(integersIn: 0..<files.numberOfRows), columnIndexes: [0])
            notifySequence()
            // ほかで書き換えたファイル（マーカーの数が変わった）のマージエディタは、手を付けていなければ読み直す。
            // 前の中身のまま Save すると、ほかでした解決をマーカー付きの中身で上書きしてしまう。
            for (path, state) in states where old[path].map({ $0 != state }) ?? false {
                guard let editor = mergeEditors[path], !editor.hasUnsavedChanges else { continue }
                mergeEditors[path] = nil
                if embeddedEditor === editor, let entry = entries.first(where: { $0.item.path == path }) {
                    unembed()
                    showDiff(of: entry, staged: side(for: entry.item))
                }
            }
        }
    }

    /// 絞り込みと見せ方（一覧・木）に合わせて並べ直し、選んでいたファイルを選び直す。
    private func rebuildList() {
        // 選んでいたファイル（いくつでも）。読み直しても、一覧に残っているものは選んだままにする。
        let selectedPaths = Set(files.selectedRowIndexes.compactMap { (files.item(atRow: $0) as? Entry)?.item.path })
        let query = filterField.stringValue.trimmingCharacters(in: .whitespaces).lowercased()
        let filtered = query.isEmpty ? allEntries : allEntries.filter { $0.item.path.lowercased().contains(query) }
        if showsSections {
            // git status と同じ節: Unmerged（あれば）、Staged、Unstaged。
            let conflicts = filtered.filter(\.isConflict)
            let staged = filtered.filter { !$0.isConflict && $0.item.isStaged }.map { Entry(item: $0.item, section: .staged) }
            let unstaged = filtered.filter { !$0.isConflict && $0.item.isUnstaged }.map { Entry(item: $0.item, section: .unstaged) }
            sections = [(nil, conflicts), (DiffSide.staged, staged), (DiffSide.unstaged, unstaged)]
                .filter { $0.0 != nil || !$0.1.isEmpty }
                .map { Section(side: $0.0, entries: $0.1, tree: showsTree ? Self.makeTree($0.1) : nil) }
            entries = conflicts + staged + unstaged
            tree = nil
        } else {
            sections = nil
            entries = filtered
            tree = showsTree ? Self.makeTree(entries) : nil
        }
        files.reloadData()
        if let sections {
            for section in sections {
                files.expandItem(section)
                expandTree(below: section.tree)
            }
        }
        expandTree(below: tree)
        emptyLabel.stringValue = allEntries.isEmpty ? L("No changes") : L("No matching files")
        emptyLabel.isHidden = !entries.isEmpty
        updateHeader()
        // すべてのファイルを見ている間は、変更の一覧で選んでいるファイルの差分に替えない。
        guard !showsAllFiles else { return reloadTreeFiles() }

        // 同じファイルを選び直す（いくつか選んでいれば、残っているものをすべて）。差分に出していたファイルが
        // なくなれば、選んでいたほかのファイル、それもなければ最初のもの。
        let kept = entries.filter { selectedPaths.contains($0.item.path) }
        // 確かめる用（`--snapshot`）に `-AMHWorkingCopySelect <パス>` で最初に選ぶファイルを決められる。
        if selectedKey == nil, let path = UserDefaults.standard.string(forKey: "AMHWorkingCopySelect"),
           let entry = entries.first(where: { $0.item.path == path }) {
            selectedKey = (path, side(for: entry.item))
        }
        // 節分けでは、今見ている側の節の行を選ぶ（なければもう一方の節）。
        let target = selectedKey.flatMap { key in
            entries.first { $0.item.path == key.path && ($0.section == nil || $0.section == diffSide) }
                ?? entries.first { $0.item.path == key.path }
        } ?? kept.first ?? entries.first
        if let target {
            adoptSection(of: target)
            let row = files.row(forItem: target)
            let others = IndexSet(kept.filter { $0 !== target }.map { files.row(forItem: $0) }.filter { $0 >= 0 })
            // 差分に出すファイルを最後に足し、選択の起点（selectedRow）にする。途中の選択の変化では差分を替えない。
            // 畳んだディレクトリの中なら行はない。差分はそのまま出す。
            isRestoringSelection = true
            files.selectRowIndexes(others, byExtendingSelection: false)
            files.selectRowIndexes(row >= 0 ? [row] : [], byExtendingSelection: !others.isEmpty)
            isRestoringSelection = false
            let keep = target.item.path == selectedKey?.path
            // いくつか選んでいれば、並べた差分のまま。
            let chosen = files.selectedRowIndexes.compactMap { files.item(atRow: $0) as? Entry }.filter { !$0.isConflict }
            if multi != nil, chosen.count >= 2 {
                showMulti(chosen, keepScroll: true)
            } else {
                showDiff(of: target, staged: side(for: target.item), keepScroll: keep)
            }
        } else {
            current = nil
            selectedKey = nil
            // 選んでいたコンフリクトがなくなった（merge --abort など）: その解決のボタンも消す。
            isConflict = false
            diffView.show(nil, message: allEntries.isEmpty ? L("No changes") : "")
            titleLabel.stringValue = ""
            showLineEndings(nil)
            updateButtons()
        }
        // 確かめる用（`--snapshot`）に `-AMHWorkingCopySelectMany a,b` でいくつか選んだ形にできる。
        if !appliedSelectMany, let many = UserDefaults.standard.string(forKey: "AMHWorkingCopySelectMany") {
            appliedSelectMany = true
            let paths = Set(many.split(separator: ",").map(String.init))
            let rows = IndexSet((0..<files.numberOfRows).filter { (files.item(atRow: $0) as? Entry).map { paths.contains($0.item.path) } ?? false })
            isRestoringSelection = true
            files.selectRowIndexes(rows, byExtendingSelection: false)
            isRestoringSelection = false
            let chosen = rows.compactMap { files.item(atRow: $0) as? Entry }
            if chosen.count >= 2 { showMulti(chosen) }
        }
    }
    private var appliedSelectMany = false

    /// 節分けの一覧の行を選んだ: 差分の側をその節にそろえる。替えたら true。
    @discardableResult
    private func adoptSection(of entry: Entry) -> Bool {
        guard let section = entry.section, section != diffSide else { return false }
        diffSide = section
        sideControl.selectedSegment = section.rawValue
        return true
    }

    /// 一覧の Mixed・節分けを切り替える。
    @objc private func sectionsChanged() {
        showsSections = sectionsButton.state == .on
        sectionsButton.contentTintColor = showsSections ? .controlAccentColor : nil
        rebuildList()
        if showsSections { loadSplitStats() }
    }

    /// `item` の差分で見せる側が Staged か（見る側は Working Copy で 1 つ）。
    private func side(for item: StatusItem) -> Bool {
        diffSide == .staged
    }

    /// 隙間の開き方を覚えるキー（見る側ごとに隙間が違う）。
    private func gapKey(_ path: String) -> String {
        "\(effectiveSide.rawValue)\u{0}\(path)"
    }

    /// 今の差分の側（すべてのファイルでは、ファイル全体を All で見せる）。
    private var effectiveSide: DiffSide { showsAllFiles ? .all : diffSide }

    /// sparse checkout なら、一覧の上にそう出す（取り出していないフォルダのファイルは変更に出ない）。
    func setSparse(_ sparse: RepositoryForm.Sparse?) {
        sparseNote.isHidden = sparse == nil
        guard let sparse else { return }
        let title = sparse.cone ? L("Only some folders are checked out (%ld)", sparse.folders.count)
            : L("Only some files are checked out")
        sparseNote.attributedTitle = NSAttributedString(string: title, attributes: [
            .font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
            .foregroundColor: NSColor.secondaryLabelColor,
        ])
        sparseNote.toolTip = RepositoryForm.sparseHelp(sparse) + "\n" + L("Click to change them in Settings > Fetching and Folders.")
    }

    @objc private func showFolders() { onShowFolders?() }

    /// 一覧の見出し: すべてのチェックの状態と、含める数 / 全体の数。
    private func updateHeader() {
        let candidates = entries.filter { !$0.isConflict }
        let states = Set(candidates.map(\.check))
        allCheckbox.state = states == [.on] ? .on : states.isEmpty || states == [.off] ? .off : .mixed
        allCheckbox.isEnabled = !candidates.isEmpty
        let included = allEntries.filter { $0.item.isStaged && !$0.isConflict }.count
        countLabel.stringValue = allEntries.isEmpty ? "" : "\(included) / \(allEntries.count)"
        countLabel.toolTip = L("Files in the next commit / changed files")
    }

    /// ファイルをディレクトリの木にする。
    private static func makeTree(_ entries: [Entry]) -> Dir {
        let root = Dir(name: "", path: "")
        for entry in entries {
            var dir = root
            let parts = entry.item.path.split(separator: "/").map(String.init)
            for part in parts.dropLast() {
                if let next = dir.dirs.first(where: { $0.name == part }) {
                    dir = next
                } else {
                    let next = Dir(name: part, path: dir.path.isEmpty ? part : dir.path + "/" + part)
                    dir.dirs.append(next)
                    dir = next
                }
            }
            dir.files.append(entry)
        }
        func compact(_ dir: Dir) {
            while dir.files.isEmpty, dir.dirs.count == 1, !dir.name.isEmpty {
                let only = dir.dirs[0]
                dir.name += "/" + only.name
                dir.path = only.path
                dir.dirs = only.dirs
                dir.files = only.files
            }
            dir.dirs.sort { $0.name < $1.name }
            dir.dirs.forEach(compact)
        }
        root.dirs.forEach(compact)
        root.dirs.sort { $0.name < $1.name }
        return root
    }

    /// `dir` の下のディレクトリを、使う人が畳んだもの（とその中）を除いて開く。
    private func expandTree(below dir: Dir?) {
        guard let dir else { return }
        isExpandingTree = true
        defer { isExpandingTree = false }
        func expand(_ dir: Dir) {
            for child in dir.dirs where !collapsedDirs.contains(child.path) {
                files.expandItem(child)
                expand(child)
            }
        }
        expand(dir)
    }

    @objc private func layoutChanged() {
        layout = Layout(rawValue: layoutControl.selectedSegment) ?? .list
        treeFiles.layout = switch layout {
        case .list: .list
        case .tree: .tree
        case .flat: .flat
        }
        rebuildList()
    }

    /// 変更 ／ すべてのファイル。
    @objc private func contentSwitchChanged() {
        let all = showsAllFiles
        filesScroll.isHidden = all
        treeFiles.scroll.isHidden = !all
        includeRow.isHidden = all
        filterField.isHidden = all
        treeFilePath = nil
        if all {
            treeFiles.layout = switch layout {
            case .list: .list
            case .tree: .tree
            case .flat: .flat
            }
            reloadTreeFiles()
            // 差分に出していたファイルを、すべてのファイルの木でも選ぶ（ファイル全体の差分にする）。
            if let path = selectedKey?.path { openTreeFile(path) }
        } else {
            rebuildList()
        }
        view.window?.makeFirstResponder(all ? treeFiles.outline : files)
    }

    /// すべてのファイルの木を、HEAD と今の status で作り直す。
    private func reloadTreeFiles() {
        guard showsAllFiles, let status else { return }
        let repository = self.repository
        Task {
            let head = await Task.detached(priority: .userInitiated) { try? repository.resolveCommit("HEAD") }.value
            guard showsAllFiles, self.status === status else { return }
            treeFiles.reload(head: head, items: status.items) { Self.mark(of: Entry(item: $0)) }
            treeHead = head
            if treeFilePath == nil, selectedKey == nil,
               let path = UserDefaults.standard.string(forKey: "AMHWorkingCopySelect") {
                treeFiles.select(path: path)
            }
        }
    }

    /// すべてのファイルを作った HEAD（40 桁）。
    private var treeHead: String?

    /// すべてのファイルで `path` を開く: 変えたファイルはファイル全体の差分（行の stage もできる）、変えていない
    /// ファイルは今の中身。
    private func openTreeFile(_ path: String, keepScroll: Bool = false) {
        if let entry = allEntries.first(where: { $0.item.path == path }) {
            treeFilePath = nil
            showDiff(of: entry, staged: side(for: entry.item), keepScroll: keepScroll)
            return
        }
        treeFilePath = path
        selectedKey = nil
        current = nil
        sideControl.isHidden = true
        isConflict = false
        unembed()
        removeImage()
        titleLabel.stringValue = path
        diffView.path = path
        showLineEndings(nil)
        request += 1
        let request = self.request
        let repository = self.repository
        updateButtons()
        guard let head = treeHead, let commit = Data(hex: head) else {
            return diffView.show(nil, message: L("Couldn’t load the diff"))
        }
        if ImageDiffPanel.isImage(path) {
            Task {
                let data = await Task.detached(priority: .userInitiated) { () -> Data? in
                    let folder = (path as NSString).deletingLastPathComponent, name = (path as NSString).lastPathComponent
                    guard let entry = (try? repository.treeEntries(commit: head, path: folder))?.first(where: { $0.name == name })
                    else { return nil }
                    return repository.blob(oid: entry.oid)
                }.value
                guard request == self.request else { return }
                guard let data, NSImage(data: data) != nil else { return diffView.show(nil, message: L("Binary file")) }
                let panel = ImageDiffPanel(before: nil, after: data, svg: false, mode: .twoUp, position: 0.5, single: true)
                imagePanel = panel
                imagePanelPath = path
                rightView?.content = panel
                Backdrop.clearBackgrounds(in: panel, clear: true)
                diffView.show(nil)
                updateButtons()
            }
            return
        }
        Task {
            let diff = await Task.detached(priority: .userInitiated) { try? repository.treeFile(commit: commit, path: path) }.value
            guard request == self.request else { return }
            guard let diff else { return diffView.show(nil, message: L("Couldn’t load the diff")) }
            if diff.isBinary { return diffView.show(nil, message: L("Binary file")) }
            diffView.fileMode = true
            diffView.show(diff, keepScroll: keepScroll)
            updateButtons()
        }
    }

    func controlTextDidChange(_ obj: Notification) {
        if (obj.object as? NSTextField) === summary, let editor = summary.currentEditor() as? NSTextView {
            offerIssues(in: editor)
            return
        }
        guard (obj.object as? NSSearchField) === filterField else { return }
        rebuildList()
    }

    // MARK: - #123 の補完

    /// GitHub の開いている Issue・PR（番号・タイトル）。RepositoryWindow が gh で読んで渡す。
    var issueCandidates: (() async -> [(number: Int, title: String)])?

    /// `#` を打った直後なら、開いている Issue・PR の一覧を出し、選んだ番号を入れる。
    private func offerIssues(in textView: NSTextView) {
        let range = textView.selectedRange()
        // 変換中（IME で打っている途中）は、確定するまで待つ。全角の ＃ でもよい。
        guard let issueCandidates, !textView.hasMarkedText(), range.length == 0, range.location > 0,
              ["#", "＃"].contains((textView.string as NSString).substring(with: NSRange(location: range.location - 1, length: 1)))
        else { return }
        Task {
            let items = await issueCandidates()
            // 待っている間に打ち進めていたら出さない。
            guard !items.isEmpty, textView.selectedRange() == range, let window = textView.window else { return }
            let menu = NSMenu()
            for item in items.prefix(30) {
                menu.addItem(ClosureMenuItem(title: "#\(item.number)  \(item.title)") {
                    // 全角の ＃ なら半角に直して、番号を入れる。
                    let at = textView.selectedRange()
                    textView.insertText("#\(item.number)", replacementRange: NSRange(location: at.location - 1, length: 1))
                })
            }
            // 文字の位置の下に出す。
            let rect = textView.firstRect(forCharacterRange: range, actualRange: nil)
            let point = window.convertPoint(fromScreen: NSPoint(x: rect.minX, y: rect.minY))
            menu.popUp(positioning: nil, at: textView.convert(point, from: nil), in: textView)
        }
    }

    // MARK: - fixup! と squash!

    /// Commit ▾ の Fixup・Squash の一覧を、最近のコミットで作り直す。
    /// Fixup・Squash の項目の前の区切りの印。
    private static let fixupSeparatorTag = 0x6678

    private func buildFixupMenus() {
        guard let menu = commitMenuButton.menu else { return }
        for title in [L("Fixup Commit For"), L("Squash Commit For")] {
            if let old = menu.item(withTitle: title) { menu.removeItem(old) }
        }
        // 前に足した区切りも外す（作り直すたびに区切りが増えないように）。
        while let old = menu.item(withTag: Self.fixupSeparatorTag) { menu.removeItem(old) }
        var subjects: [String] = []
        for commit in recentCommits {
            let subject = commit.message.split(separator: "\n", maxSplits: 1).first.map(String.init) ?? ""
            // fixup! を重ねない（rebase --autosquash は元のコミットの件名で探す）。
            if subject.isEmpty || subject.hasPrefix("fixup! ") || subject.hasPrefix("squash! ") || subjects.contains(subject) { continue }
            subjects.append(subject)
            if subjects.count == 15 { break }
        }
        guard !subjects.isEmpty else { return }
        let separator = NSMenuItem.separator()
        separator.tag = Self.fixupSeparatorTag
        menu.addItem(separator)
        for (title, prefix, keepBody) in [(L("Fixup Commit For"), "fixup! ", false), (L("Squash Commit For"), "squash! ", true)] {
            let sub = NSMenu(title: title)
            for subject in subjects {
                sub.addItem(ClosureMenuItem(title: subject) { [weak self] in
                    guard let self else { return }
                    // fixup! は件名だけ（元のメッセージを残す）、squash! は本文に書き足せる。
                    summary.stringValue = prefix + subject
                    if !keepBody { message.string = "" }
                    amend.state = .off
                    updateButtons()
                    view.window?.makeFirstResponder(keepBody ? message : summary)
                })
            }
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.submenu = sub
            item.toolTip = prefix == "fixup! "
                ? L("Commit as a fixup! of the chosen commit (combine later with rebase -i --autosquash)")
                : L("Commit as a squash! of the chosen commit (its message is added when combined)")
            menu.addItem(item)
        }
    }

    /// `onlyIfChanged` なら、読んだ差分が出しているものと同じ中身のとき出し直さない（選んだ行を外さない）。
    private func showDiff(of entry: Entry, staged: Bool, keepScroll: Bool = false, onlyIfChanged: Bool = false) {
        guard let status else { return }
        if multi != nil {
            multi = nil
            diffView.onToggleFile = nil
            diffView.fileMenu = nil
            diffView.fileMarks = [:]
        }
        diffView.stackedStageChecks = false
        selectedKey = (entry.item.path, staged)
        treeFilePath = nil
        diffView.fileMode = false
        titleLabel.stringValue = entry.item.path
        diffView.path = entry.item.path
        request += 1
        let request = self.request
        isConflict = entry.isConflict
        diffView.isConflictView = entry.isConflict
        // 見る側（Unstaged・Staged・All）はファイルを替えても同じ。
        let side = effectiveSide
        sideControl.isHidden = entry.isConflict || showsAllFiles
        sideControl.selectedSegment = diffSide.rawValue
        selectedKey = (entry.item.path, side == .staged)
        if entry.isConflict {
            let path = entry.item.path
            current = nil
            showLineEndings(nil)
            updateButtons()
            // 別ウィンドウで開いていれば、もう 1 つ作らずにそう案内する（Result が 2 つになって食い違わないように）。
            if MergeEditorWindowController.isOpen(path: path, repository: repository) {
                unembed()
                diffView.show(nil, message: L("The merge editor is open in a separate window"))
                return
            }
            if let editor = mergeEditors[path] {
                embed(editor)
                return
            }
            Task {
                do {
                    let editor = try await MergeEditorViewController.load(path: path, repository: repository, actions: actions!)
                    guard request == self.request else { return }
                    editor.autoApplied = !autoAppliedPaths.insert(path).inserted
                    mergeEditors[path] = editor
                    embed(editor)
                } catch {
                    guard request == self.request else { return }
                    unembed()
                    diffView.show(nil, message: L("Can’t read: %@", "\(error)"))
                }
            }
            return
        }
        unembed()
        let whitespace = whitespacePopUp.whitespace
        // すべてのファイルでは、変えたファイルもファイル全体で見せる（畳まない）。
        let context: UInt32 = showsAllFiles ? .max : 3
        // 確かめる用（`--snapshot`）に `-AMHWorkingCopyOpenGaps 1:20:0,2:0:max` で隙間の開き方（番号・上・下）を
        // 決められる。
        if openGaps[gapKey(entry.item.path)] == nil, let gaps = UserDefaults.standard.string(forKey: "AMHWorkingCopyOpenGaps") {
            var parsed: [Int: GapOpening] = [:]
            for item in gaps.split(separator: ",") {
                let parts = item.split(separator: ":").map { $0 == "max" ? UInt32.max : UInt32($0) ?? 0 }
                guard parts.count == 3 else { continue }
                parsed[Int(parts[0])] = GapOpening(top: parts[1], bottom: parts[2])
            }
            openGaps[gapKey(entry.item.path)] = parsed
        }
        let open: [Int: GapOpening]? = showsAllFiles ? nil : openGaps[gapKey(entry.item.path)] ?? [:]
        diffView.foldsGaps = !showsAllFiles
        diffView.gapOpenings = openGaps[gapKey(entry.item.path)] ?? [:]
        updateWholeFileButton()
        // その側に変更がなければ、読まずにそう出す。
        if side != .all, !(side == .staged ? entry.item.isStaged : entry.item.isUnstaged) {
            current = nil
            removeImage()
            showLineEndings(nil)
            diffView.show(nil, message: side == .staged ? L("No staged changes in this file") : L("No unstaged changes in this file"))
            updateButtons()
            return
        }
        // Unstaged・Staged では、変更のかたまりのボタンで Stage・Unstage する（空白を無視していれば出さない）。
        diffView.splitStageTitle = side == .all || whitespace != .exact ? nil : side == .staged ? "Unstage" : "Stage"
        Task {
            // All は HEAD と作業ツリーの 1 本の差分（行ごとに staged か）。表せないとき・空白を無視するときは、そう出す。
            let (diff, combined, failure) = await Task.detached(priority: .userInitiated) { () -> (WorkingFileDiff?, Bool, String?) in
                if side == .all {
                    guard whitespace == .exact else {
                        return (nil, false, L("All can’t be shown while whitespace is ignored. See Unstaged and Staged."))
                    }
                    do {
                        if let combined = try status.combinedDiff(of: entry.item, context: context, openGaps: open) {
                            return (combined, true, nil)
                        }
                        return (nil, false, L("This file’s staged and unstaged changes can’t be shown in one diff. See Unstaged and Staged."))
                    } catch {
                        return (nil, false, L("Can’t read the diff: %@", "\(error)"))
                    }
                }
                do {
                    return (try status.diff(of: entry.item, staged: side == .staged, context: context, openGaps: open, whitespace: whitespace),
                            false, nil)
                } catch {
                    return (nil, false, L("Can’t read the diff: %@", "\(error)"))
                }
            }.value
            guard request == self.request else { return }
            if onlyIfChanged, let diff, let shown = current, shown.item.path == diff.item.path,
               shown.isCombined == diff.isCombined, shown.staged == diff.staged, shown.hasSameRows(as: diff) {
                return
            }
            _ = combined
            current = diff
            showLineEndings(diff?.lineEndings)
            let path = entry.item.path
            if let diff, open != nil {
                gapCounts[gapKey(path)] = (diff.hunkCount, diff.foldedGapCount)
                updateWholeFileButton()
            }
            let svg = ImageDiffPanel.isSVG(path)
            if failure == nil, let diff, ImageDiffPanel.isImage(path) || svg, diff.isBinary || (svg && !svgAsText.contains(path)) {
                // 画像は History と同じく、前後を並べる・スワイプ・重ねる・違いの 4 つで比べる。
                showImage(diff, path: path, keepMode: keepScroll)
                diffView.show(nil)
            } else {
                removeImage()
                if let failure {
                    // 空の差分にせず、読めなかった・出せない理由を出す。
                    diffView.show(nil, message: failure)
                } else if let diff, diff.isBinary {
                    diffView.show(nil, message: L("Binary file"))
                } else {
                    diffView.show(diff, keepScroll: keepScroll)
                }
            }
            updateButtons()
        }
    }

    /// 選んだいくつかのファイルの差分を、見出しでつないで並べる（上から `Settings.diffOpenFiles` 個まで）。見る側
    /// （Unstaged・Staged・All）は 1 ファイルのときと同じ。その側に変更のないファイルは並べない。
    private func showMulti(_ chosen: [Entry], keepScroll: Bool = false) {
        guard let status else { return }
        let side = diffSide
        let whitespace = whitespacePopUp.whitespace
        let candidates = chosen.filter { entry in
            side == .all || (side == .staged ? entry.item.isStaged : entry.item.isUnstaged)
        }
        let shown = Array(candidates.prefix(Settings.diffOpenFiles))
        let paths = chosen.map(\.item.path)
        if multi?.paths != paths { multiCollapsed = [] }
        multi = (paths, multi?.paths == paths ? multi?.stacked : nil)
        selectedKey = nil
        current = nil
        treeFilePath = nil
        isConflict = false
        unembed()
        removeImage()
        showLineEndings(nil)
        sideControl.isHidden = false
        sideControl.selectedSegment = side.rawValue
        var title = L("%ld files", chosen.count)
        if candidates.count > shown.count { title += "  " + L("(showing the first %ld)", shown.count) }
        titleLabel.stringValue = title
        request += 1
        let request = self.request
        let openings = shown.map { openGaps[gapKey($0.item.path)] ?? [:] }
        diffView.foldsGaps = true
        diffView.splitStageTitle = side == .all || whitespace != .exact ? nil : side == .staged ? "Unstage" : "Stage"
        diffView.stackedStageChecks = side == .all
        guard !shown.isEmpty else {
            diffView.show(nil, message: side == .staged ? L("No staged changes in these files") : L("No unstaged changes in these files"))
            return updateButtons()
        }
        Task {
            let diffs = await Task.detached(priority: .userInitiated) { () -> [WorkingFileDiff] in
                zip(shown, openings).compactMap { entry, open in
                    if side == .all {
                        guard whitespace == .exact else { return nil }
                        return try? status.combinedDiff(of: entry.item, context: 3, openGaps: open)
                    }
                    return try? status.diff(of: entry.item, staged: side == .staged, context: 3, openGaps: open, whitespace: whitespace)
                }
            }.value
            guard request == self.request, multi?.paths == paths else { return }
            showStacked(diffs, keepScroll: keepScroll)
        }
    }

    /// 並べる差分を、畳んだファイルを除いてつなぎ、出す。
    private func showStacked(_ diffs: [WorkingFileDiff], keepScroll: Bool) {
        let collapsed = diffs.map { multiCollapsed.contains($0.item.path) }
        let stacked = WorkingStackedDiff(files: diffs, collapsed: collapsed)
        multi?.stacked = stacked
        // 隙間の番号は、並べた差分の頭から数える（畳んだファイルには隙間の行がない）。ファイルごとに直す。
        var offsets: [(file: Int, first: Int, count: Int)] = []
        var next = 0
        for (i, d) in diffs.enumerated() where !collapsed[i] && !d.isBinary {
            offsets.append((i, next, d.hunkCount))
            next += d.hunkCount
        }
        var openings: [Int: GapOpening] = [:]
        for o in offsets {
            for (gap, opening) in openGaps[gapKey(diffs[o.file].item.path)] ?? [:] where gap < o.count {
                openings[o.first + gap] = opening
            }
        }
        diffView.gapOpenings = openings
        diffView.onSetGap = { [weak self] gap, opening in
            guard let self, let o = offsets.last(where: { $0.first <= gap }) else { return }
            openGaps[gapKey(diffs[o.file].item.path), default: [:]][gap - o.first] = opening
            reloadShownDiff()
        }
        var marks: [Int: (String, NSColor)] = [:]
        for (i, d) in diffs.enumerated() { marks[i] = Self.mark(of: Entry(item: d.item)) }
        diffView.fileMarks = marks
        diffView.onToggleFile = { [weak self] file in
            guard let self, file < diffs.count else { return }
            let path = diffs[file].item.path
            if multiCollapsed.remove(path) == nil { multiCollapsed.insert(path) }
            showStacked(diffs, keepScroll: true)
        }
        diffView.fileMenu = { [weak self] file in
            guard let self, file < diffs.count,
                  let entry = entries.first(where: { $0.item.path == diffs[file].item.path }) else { return nil }
            let menu = NSMenu()
            menu.addItem(ClosureMenuItem(title: entry.item.isStaged && !entry.item.isUnstaged ? "Unstage" : "Stage") { [weak self] in
                guard let self else { return }
                let repository = self.repository
                let path = entry.item.path
                if entry.item.isStaged && !entry.item.isUnstaged {
                    run { try repository.unstage(paths: [path]) }
                } else {
                    run { try repository.stage(paths: [path]) }
                }
            })
            if entry.item.isUnstaged {
                menu.addItem(ClosureMenuItem(title: "Discard…") { [weak self] in self?.discardFile(entry) })
            }
            return menu
        }
        diffView.show(stacked, keepScroll: keepScroll)
        updateButtons()
    }

    /// 改行を変換している（作業ツリーと git で違う）・改行が混ざっているファイルの印。普段のファイルでは出さない。
    private func showLineEndings(_ lineEndings: LineEndings?) {
        guard let le = lineEndings, le.notable else {
            lineEndingsLabel.isHidden = true
            return
        }
        let name: (LineEnding) -> String = {
            switch $0 {
            case .lf: "LF"
            case .crlf: "CRLF"
            case .mixed: L("Mixed")
            case .none: L("None")
            }
        }
        // 作業ツリーの形 → git に入る形。
        let text = le.worktree != le.index && le.index != .none
            ? "\(name(le.worktree)) → \(name(le.index))" : name(le.worktree)
        lineEndingsLabel.stringValue = le.warning ? "⚠︎ \(text)" : text
        lineEndingsLabel.textColor = le.warning ? .systemOrange : .secondaryLabelColor
        var tip = [
            L("Line endings: working tree %1$@, git %2$@, checkout %3$@", name(le.worktree), name(le.index), name(le.checkout))
        ]
        if !le.rule.isEmpty { tip.append(L("Conversion: %@", le.rule)) }
        if le.warning { tip.append(L("The next commit or checkout may change the line endings of the whole file")) }
        lineEndingsLabel.toolTip = tip.joined(separator: "\n")
        lineEndingsLabel.isHidden = false
    }

    /// 画像の比較を、差分の代わりに出す。`keepMode` なら（読み直し）スワイプの位置などを保つため、同じファイルなら
    /// 作り直さない。
    private func showImage(_ diff: WorkingFileDiff, path: String, keepMode: Bool) {
        if keepMode, imagePanel != nil, imagePanelPath == path { return }
        let item = diff.item
        let added = item.kind == .untracked || (diff.isCombined ? item.indexState == "A" : (diff.staged ? item.indexState == "A" : false))
        let deleted = item.worktreeState == "D" || (diff.staged && item.indexState == "D")
        let panel = ImageDiffPanel(
            before: added ? nil : diff.blob(after: false), after: deleted ? nil : diff.blob(after: true),
            svg: ImageDiffPanel.isSVG(path), mode: imageMode, position: 0.5)
        panel.onModeChange = { [weak self] mode in self?.imageMode = mode }
        panel.onShowText = { [weak self] in
            guard let self, let key = selectedKey, let entry = entries.first(where: { $0.item.path == key.path }) else { return }
            svgAsText.insert(path)
            showDiff(of: entry, staged: key.staged)
        }
        imagePanel = panel
        imagePanelPath = path
        rightView?.content = panel
        Backdrop.clearBackgrounds(in: panel, clear: true)
    }

    private var imagePanelPath: String?

    /// 画像の比較をやめ、差分に戻す。
    private func removeImage() {
        guard imagePanel != nil else { return }
        imagePanel = nil
        imagePanelPath = nil
        if embeddedEditor == nil { rightView?.content = diffView.view }
    }

    /// 隙間の行のボタン: 出している差分の、その変更のない所の開き方を変える（nil なら畳む）。
    private func setGap(_ gap: Int, _ opening: GapOpening?) {
        guard !showsAllFiles, let key = selectedKey, !isConflict,
              let entry = entries.first(where: { $0.item.path == key.path }) else { return }
        openGaps[gapKey(key.path), default: [:]][gap] = opening
        showDiff(of: entry, staged: key.staged, keepScroll: true)
    }

    /// 帯のボタンと表示の設定: 出している差分の変更のない所をすべて開く（ファイル全体）・すべて閉じる。
    @objc private func toggleWholeFile() {
        guard !showsAllFiles, let key = selectedKey, !isConflict, let entry = entries.first(where: { $0.item.path == key.path }) else {
            return NSSound.beep()
        }
        let count = gapCounts[gapKey(key.path)]?.all ?? 0
        openGaps[gapKey(key.path)] = showsWholeFile ? [:] : Dictionary(uniqueKeysWithValues: (0..<count).map {
            // 隙間の行は上に残す（末尾の隙間でも同じ。そこから畳める）。
            ($0, GapOpening(top: 0, bottom: .max))
        })
        showDiff(of: entry, staged: key.staged, keepScroll: true)
    }

    private var showsWholeFile: Bool {
        showsAllFiles || selectedKey.map { key in
            let counts = gapCounts[gapKey(key.path)] ?? (0, 0)
            return counts.all > 0 && counts.folded == 0
        } ?? false
    }

    private func updateWholeFileButton() {
        let on = showsWholeFile
        wholeFileButton.image = AppIcons.image(symbol: "doc.plaintext", label: L("Show Whole File"))
        wholeFileButton.contentTintColor = on ? .controlAccentColor : nil
        wholeFileButton.toolTip = (on ? L("Showing the whole file") : L("Show Whole File")) + "\n"
            + L("Open every unchanged part of the file (each part also opens from its own row)")
    }

    @objc private func findInDiff() {
        guard imagePanel == nil, embeddedEditor == nil, selectedKey != nil || treeFilePath != nil else { return NSSound.beep() }
        diffView.showFindBar()
    }

    /// 表示の設定のメニュー（開くたびに作る）: コード（ファイル全体・長い行を折り返す）、コミットメッセージ（畳む）。
    private func fillViewOptions(_ menu: NSMenu) {
        menu.addItem(.sectionHeader(title: L("Code")))
        let whole = ClosureMenuItem(title: L("Show Whole File")) { [weak self] in self?.toggleWholeFile() }
        whole.state = showsWholeFile ? .on : .off
        whole.isEnabled = selectedKey != nil && !isConflict && !showsAllFiles
        whole.indentationLevel = 1
        menu.addItem(whole)
        let wrap = DiffViewController.wrapLinesMenuItem()
        wrap.indentationLevel = 1
        menu.addItem(wrap)
        menu.addItem(.sectionHeader(title: L("Commit Message")))
        let collapse = ClosureMenuItem(title: L("Summary Only")) { [weak self] in self?.toggleCommitCollapsed() }
        collapse.state = commitCollapsed ? .on : .off
        collapse.indentationLevel = 1
        collapse.toolTip = L("Collapse the commit message to the summary line to give the diff more room")
        menu.addItem(collapse)
    }

    /// 無視する空白を選び直した: 出している差分を読み直す。
    private func whitespaceChanged() {
        guard let key = selectedKey, !isConflict,
              let entry = entries.first(where: { $0.item.path == key.path }) else { return updateButtons() }
        showDiff(of: entry, staged: key.staged, keepScroll: true)
    }

    @objc private func sideChanged() {
        diffSide = DiffSide(rawValue: sideControl.selectedSegment) ?? .unstaged
        guard let key = selectedKey else { return }
        // 節分けでは、一覧の選択もその側の節の行に移す（その節になければ、そのまま）。
        if showsSections, let entry = entries.first(where: { $0.item.path == key.path && $0.section == diffSide }) {
            let row = files.row(forItem: entry)
            if row >= 0 {
                isRestoringSelection = true
                files.selectRowIndexes([row], byExtendingSelection: false)
                isRestoringSelection = false
            }
        }
        guard let entry = entries.first(where: { $0.item.path == key.path }) else { return }
        showDiff(of: entry, staged: diffSide == .staged)
    }

    /// 埋め込んだ・覚えているマージエディタに、保存していない Result の変更がある（閉じる前に確かめる）。
    var hasUnsavedMergeEdits: Bool {
        mergeEditors.values.contains { $0.hasUnsavedChanges } || embeddedEditor?.hasUnsavedChanges == true
    }

    /// 別ウィンドウのマージエディタを解決せずに閉じた: そのファイルを出していれば（「別のウィンドウで開いている」の
    /// 案内のまま）、ここに埋め込み直す（同じ行を押し直しても何も起きないので）。
    @objc private func mergeWindowClosed(_ notification: Notification) {
        guard isConflict, embeddedEditor == nil, let key = selectedKey,
              notification.userInfo?["key"] as? String == MergeEditorWindowController.key(path: key.path, repository: repository),
              let entry = entries.first(where: { $0.item.path == key.path }) else { return }
        showDiff(of: entry, staged: key.staged)
    }

    /// 右の欄の差分の代わりに、マージエディタを出す。
    private func embed(_ editor: MergeEditorViewController) {
        guard embeddedEditor !== editor, let rightView, let rightController else { return }
        unembed()
        editor.onPopOut = { [weak self, weak editor] in
            guard let self, let editor, let actions else { return }
            // 編集中の内容のまま、別ウィンドウに移す。
            unembed()
            mergeEditors[editor.path] = nil
            MergeEditorWindowController.show(editor, actions: actions, from: view.window)
            diffView.show(nil, message: L("The merge editor is open in a separate window"))
        }
        editor.onResolved = { [weak self, weak editor] in
            guard let self, let editor else { return }
            if embeddedEditor === editor { unembed() }
            mergeEditors[editor.path] = nil
        }
        editor.onReload = { [weak self, weak editor] in
            guard let self, let editor else { return }
            if embeddedEditor === editor { unembed() }
            mergeEditors[editor.path] = nil
            if let key = selectedKey, key.path == editor.path,
               let entry = entries.first(where: { $0.item.path == key.path }) {
                showDiff(of: entry, staged: key.staged)
            }
        }
        rightController.addChild(editor)
        rightView.content = editor.view
        embeddedEditor = editor
        Backdrop.clearBackgrounds(in: editor.view, clear: true)
    }

    private func unembed() {
        guard let editor = embeddedEditor, let rightView else { return }
        rightView.content = diffView.view
        editor.removeFromParent()
        embeddedEditor = nil
    }

    private func updateButtons() {
        conflictButtons.isHidden = !isConflict
        let staged = selectedKey?.staged ?? false
        let selected = diffView.selectedChangeRows
        // 1 本の差分では、選んだ行がすべて含まれていれば外す、そうでなければ含める。
        let stacked = multi?.stacked
        let selectedStaged = !selected.isEmpty && (stacked != nil
            ? effectiveSide == .all && selected.allSatisfy { stacked?.isStaged(row: $0) == true }
            : current?.isCombined == true && selected.allSatisfy { current?.isStaged(row: $0) == true })
        let shown = selectedKey != nil || multi != nil
        // 選んだファイルがなければ、ファイルの操作は出さない。
        // 「選んでから」の形では、選んだ行の操作は hunk の見出しの行と Space・右クリックで（帯のボタンは出さない）。
        let selectStyle = Settings.lineStageStyle == .select
        linesButton.isHidden = isConflict || !shown || selectStyle
        linesButton.title = staged || selectedStaged ? "Unstage Lines" : "Stage Lines"
        // 空白を無視した差分の行は中身をそのまま表していないので、行ごとには stage・破棄しない。
        let exact = whitespacePopUp.whitespace == .exact
        whitespacePopUp.isHidden = isConflict || !shown || imagePanel != nil
        wrapButton.isHidden = whitespacePopUp.isHidden
        wholeFileButton.isHidden = whitespacePopUp.isHidden || showsAllFiles
        findButton.isHidden = isConflict || imagePanel != nil || (selectedKey == nil && treeFilePath == nil)
        viewOptions.isHidden = isConflict
        updateWholeFileButton()
        linesButton.isEnabled = (current != nil || stacked != nil) && !selected.isEmpty && exact
        linesButton.toolTip = !exact ? L("Can’t stage or discard lines while whitespace is ignored")
            : current?.isCombined == true ? L("Include or exclude the selected lines in the next commit (Space)") : nil
        // 選んだ行があれば行を、なければファイルを破棄する。stage 済みの変更は破棄しない。
        discardButton.isHidden = staged || isConflict || selectedKey == nil
        let linesDiscard = !selected.isEmpty && !selectStyle
        discardButton.title = linesDiscard ? "Discard Lines" : "Discard File"
        // 作業ツリーに変更がない（すべて stage 済みの）ファイルには、捨てるものがない。
        discardButton.isEnabled = selectedKey != nil && current?.item.isUnstaged == true
            && !(linesDiscard && current?.isCombined == true && selected.contains { current?.isStaged(row: $0) == true })
            && (!linesDiscard || exact)
        discardButton.toolTip = linesDiscard && !exact ? L("Can’t stage or discard lines while whitespace is ignored")
            : !linesDiscard
            ? L("Discard the working tree changes and return to the content of the next commit (the index)")
            : L("Discard the selected lines’ changes from the working tree (included lines are excluded first)")
        let count = stagedCount
        // コミットしている間は押せない（2 回目の ⌘↩ が、もう 1 つ git commit を始めないように）。
        commitButton.isEnabled = !committing && !(count == 0 && amend.state == .off) && !composedMessage.isEmpty
        // 押せないときは、ツールチップでわけを言う（docs/ux.md の決まり）。
        commitButton.toolTip = commitDisabledReason(staged: count, amend: amend.state == .on)
            ?? L("Commit the checked files (⌘↩)")
        // ボタンの数や名前が変わると幅が変わるので、並べ直す。
        diffBar?.needsLayout = true
        commitButton.title = amend.state == .on ? "Amend Commit"
            : count == 0 ? "Commit" : count == 1 ? "Commit 1 File" : "Commit \(count) Files"
        // 1 行目の長さ。72 文字を超えたら目立たせる（git の慣習）。
        let length = summary.stringValue.count
        summaryCount.stringValue = length == 0 ? "" : "\(length)"
        summaryCount.textColor = length > 72 ? .systemOrange : length > 50 ? .systemYellow : .tertiaryLabelColor
    }

    /// Commit を押せないわけ（押せるときは nil）。
    private func commitDisabledReason(staged: Int, amend: Bool) -> String? {
        if committing { return L("Committing…") }
        var reasons: [String] = []
        if staged == 0 && !amend { reasons.append(L("No files are checked")) }
        if composedMessage.isEmpty { reasons.append(L("Write a commit message")) }
        return reasons.isEmpty ? nil : reasons.joined(separator: "\n")
    }

    /// 次のコミットに含めるファイルの数。
    private var stagedCount: Int {
        allEntries.filter { $0.item.isStaged && !$0.isConflict }.count
    }

    // MARK: - 操作

    /// 裏で git を実行し、終わったら status を読み直す。失敗したら知らせる。
    private func run(_ work: @escaping @Sendable () throws -> Void) {
        Task {
            do {
                try await Task.detached(priority: .userInitiated) { try work() }.value
            } catch {
                _ = await NSAlert(error: error).beginSheetModal(for: view.window!)
            }
            // 押したチェックは AppKit が先に進めている。何も変わらなかったとき（失敗・断られた）は status が同じで
            // 一覧を作り直さないので、今の状態から付け直す。
            files.reloadData(forRowIndexes: IndexSet(integersIn: 0..<files.numberOfRows), columnIndexes: [0])
            updateHeader()
            refresh()
        }
    }

    /// `entries` を次のコミットに含める・外す。すべて含めていれば外し、そうでなければすべて含める
    /// （一部だけ含めていたファイルも、作業ツリーの変更ごと含める）。コンフリクトのファイルは触らない。
    private func toggle(_ entries: [Entry]) {
        let targets = entries.filter { !$0.isConflict }
        guard !targets.isEmpty else { return NSSound.beep() }
        let repository = self.repository
        let paths = targets.map(\.item.path)
        if targets.allSatisfy({ $0.check == .on }) {
            run { try repository.unstage(paths: paths) }
        } else {
            run { try repository.stage(paths: paths) }
        }
    }

    /// 一覧の行（ファイルかディレクトリ）の配下のファイル。
    private func files(under item: Any) -> [Entry] {
        switch item {
        case let entry as Entry: [entry]
        case let dir as Dir: dir.allFiles
        case let section as Section: section.entries
        default: []
        }
    }

    /// Space・Enter: 選んだ行のファイルを含める・外す。
    private func toggleSelected() {
        let selected = files.selectedRowIndexes.compactMap { files.item(atRow: $0) }.flatMap { files(under: $0) }
        // 木でディレクトリとその中のファイルを一緒に選んでいても、1 回ずつにする。
        var seen = Set<String>()
        toggle(selected.filter { seen.insert($0.item.path).inserted })
    }

    @objc private func toggleAll() {
        toggle(entries)
    }

    @objc private func stageLines() {
        if let stacked = multi?.stacked {
            let rows = diffView.selectedChangeRows
            guard !rows.isEmpty else { return }
            if effectiveSide == .all {
                setStaged(rows: rows, staged: !rows.allSatisfy { stacked.isStaged(row: $0) })
            } else {
                stageRows(rows)
            }
            return
        }
        guard let current else { return }
        let rows = diffView.selectedChangeRows
        guard !rows.isEmpty else { return }
        guard whitespacePopUp.whitespace == .exact else { return NSSound.beep() }
        if current.isCombined {
            setStaged(rows: rows, staged: !rows.allSatisfy { current.isStaged(row: $0) })
        } else {
            run { try current.stage(rows: rows) }
        }
    }

    /// Unstaged・Staged の差分の `rows` 行目の変更を stage する（Staged なら unstage する）。
    private func stageRows(_ rows: [Int]) {
        if let stacked = multi?.stacked {
            guard whitespacePopUp.whitespace == .exact else { return NSSound.beep() }
            let parts = stacked.split(rows)
            return run { for (file, rows) in parts { try stacked.files[file].stage(rows: rows) } }
        }
        guard let current, !current.isCombined, !rows.isEmpty else { return }
        guard whitespacePopUp.whitespace == .exact else { return NSSound.beep() }
        run { try current.stage(rows: rows) }
    }

    /// 1 本の差分の `rows` 行目の変更を、次のコミットに含める・外す。
    private func setStaged(rows: [Int], staged: Bool) {
        if let stacked = multi?.stacked {
            let parts = stacked.split(rows)
            return run { for (file, rows) in parts { try stacked.files[file].setStaged(rows: rows, staged: staged) } }
        }
        guard let current, current.isCombined else { return }
        run { try current.setStaged(rows: rows, staged: staged) }
    }

    // MARK: - コミットの入力と差分の間の仕切り

    /// 上の段の最小の高さ: Summary、48 pt のメッセージ、ボタンの行（操作の途中ならその帯も）。畳んでいれば Summary の
    /// 1 行だけ。
    private var messageMinimum: CGFloat {
        (commitCollapsed ? 38 : 146) + (sequenceBar.isHidden ? 0 : 34)
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        // 畳んでいれば 1 行の高さに。覚えた位置がなければ、メッセージの欄を少し広めに。
        if commitCollapsed {
            messageSplit.setPosition(messageMinimum, ofDividerAt: 0)
        } else if messageSplit.arrangedSubviews.first.map({ $0.frame.height < messageMinimum }) ?? false {
            messageSplit.setPosition(messageMinimum + 40, ofDividerAt: 0)
        }
    }

    /// コミットの入力を畳む・開く（ボタンと表示の設定）。
    @objc private func toggleCommitCollapsed() {
        if !commitCollapsed { expandedMessageHeight = messageSplit.arrangedSubviews.first?.frame.height }
        commitCollapsed.toggle()
        applyCommitCollapsed()
        messageSplit.setPosition(commitCollapsed ? messageMinimum : max(messageMinimum, expandedMessageHeight ?? messageMinimum + 40), ofDividerAt: 0)
        if commitCollapsed, view.window?.firstResponder === message { view.window?.makeFirstResponder(summary) }
    }

    /// 畳んでいれば Summary の行に Commit を並べ、Description とボタンの行を隠す。
    private func applyCommitCollapsed() {
        collapseButton.image = AppIcons.image(
            symbol: commitCollapsed ? "chevron.right" : "chevron.down",
            label: commitCollapsed ? L("Show Description") : L("Hide Description"))
        collapseButton.toolTip = commitCollapsed
            ? L("Show the description and the commit options") : L("Collapse to the summary line")
        if commitCollapsed {
            summaryRow.setViews([collapseButton, summary, summaryCount, commitButton, commitMenuButton], in: .leading)
            commitRow.setViews([], in: .leading)
        } else {
            summaryRow.setViews([collapseButton, summary], in: .leading)
            commitRow.setViews(
                [amend, historyButton, insertButton, NSView(), summaryCount, suggestButton, commitButton, commitMenuButton],
                in: .leading)
        }
        messageScroll.isHidden = commitCollapsed
        commitRow.isHidden = commitCollapsed
        messageSplit.adjustSubviews()
    }


    @objc private func discard() {
        guard let key = selectedKey, !key.staged, let actions else { return }
        guard current?.item.isUnstaged == true else { return NSSound.beep() }
        let rows = Settings.lineStageStyle == .select ? [] : diffView.selectedChangeRows
        if !rows.isEmpty {
            discardRows(rows)
        } else if current?.item.kind == .untracked {
            confirmDeleteUntracked(key.path, actions: actions)
        } else {
            // ファイル全体を一度に捨てるときは確かめる（行の単位は選んで押すので、そのまま）。
            confirmDiscard(key.path, actions: actions)
        }
    }

    @objc private func stageStyleChanged() { updateButtons() }

    /// 差分の `rows` 行目の変更を作業ツリーから捨てる（hunk の見出しの Discard・帯の Discard Lines・右クリック）。
    /// いくつかのファイルを並べた差分なら、ファイルごとに。
    private func discardRows(_ rows: [Int]) {
        guard let actions, !rows.isEmpty else { return }
        guard whitespacePopUp.whitespace == .exact else { return NSSound.beep() }
        if let stacked = multi?.stacked {
            let parts = stacked.split(rows)
            actions.perform { for (file, rows) in parts { try stacked.files[file].discard(rows: rows) } }
        } else if let current {
            actions.perform { try current.discard(rows: rows) }
        }
    }

    /// merge・rebase などの途中なら、その名前（Repository > Continue・Skip・Abort を使えるか）。
    /// 途中の操作の種類（`rebase`・`merge`・`cherry-pick`・`revert`）。
    var sequence: String? { sequenceInfo?.kind }
    private(set) var sequenceInfo: SequenceInfo?

    /// Commit > Commit（⌘↩）・Stage All を使えるか。
    var canCommit: Bool { commitButton.isEnabled }
    var canStageAll: Bool { allEntries.contains { !$0.isConflict && $0.item.isUnstaged } }

    /// メニューから: ボタンと同じ。
    func commitFromMenu() { if canCommit { commit(thenPush: false) } }
    func stageAllFromMenu() { stageAll() }

    /// merge・rebase などの途中かどうかと、コンフリクトのファイルの数が変わった（ウィンドウの帯に出す）。
    var onSequenceChange: ((_ sequence: SequenceInfo?, _ conflicts: Int, _ withoutMarkers: Int) -> Void)?

    private func showSequence(_ sequence: SequenceInfo?) {
        sequenceInfo = sequence
        // 途中であることと Continue・Skip・Abort は、ウィンドウの帯に出す（どの画面でも見えるように）。
        sequenceBar.isHidden = true
        notifySequence()
    }

    /// 最初のコンフリクトのファイルを選ぶ（右にマージエディタが出る）。
    func selectFirstConflict() {
        guard let entry = entries.first(where: \.isConflict) else { return }
        let row = files.row(forItem: entry)
        guard row >= 0 else { return }
        files.selectRowIndexes([row], byExtendingSelection: false)
        files.scrollRowToVisible(row)
    }

    /// 変更のあるファイルの数が変わった（タブの名前に出す）。
    var onChangeCount: ((Int) -> Void)?

    private func notifySequence() {
        onSequenceChange?(sequenceInfo, allEntries.filter(\.isConflict).count, conflictsWithoutMarkers.count)
        onChangeCount?(allEntries.count)
    }

    @objc private func sequenceAction(_ sender: NSButton) {
        guard let op = sender.identifier?.rawValue else { return }
        actions?.perform(["op": op])
    }

    @objc private func takeOursFile() { takeSide(ours: true) }

    @objc private func takeTheirsFile() { takeSide(ours: false) }

    /// 下のマージエディタか別ウィンドウのマージエディタに、このファイルの保存していない Result がある。
    private func unsavedEditor(for path: String) -> MergeEditorViewController? {
        let editor = embeddedEditor.flatMap { $0.path == path ? $0 : nil }
            ?? MergeEditorWindowController.editor(path: path, repository: repository)
        return editor?.hasUnsavedChanges == true ? editor : nil
    }

    /// 片側のファイルを取る。別ウィンドウで開いていれば閉じる（Result は捨てると確かめてある・変えていない）。
    private func performTakeSide(_ path: String, ours: Bool) {
        MergeEditorWindowController.discard(path: path, repository: repository)
        actions?.perform(["op": "takeSide", "path": path, "ours": ours])
    }

    private func takeSide(ours: Bool) {
        guard let key = selectedKey, let actions else { return }
        // 保存していない Result は、片側のファイルで上書きされて失われる（git checkout --ours・--theirs と同じく
        // 作業ツリーのファイルを置き換える）。閉じるときと同じく、失うことを確かめる。
        if unsavedEditor(for: key.path) != nil {
            let title = ours ? L("Take Current File") : L("Take Incoming File")
            Task { @MainActor in
                guard await actions.confirm(
                    ours ? L("Take the current file without saving Result?") : L("Take the incoming file without saving Result?"),
                    message: L("Your changes to Result are lost."), ok: title, destructive: true) else { return }
                performTakeSide(key.path, ours: ours)
            }
            return
        }
        // マーカーが残っていなければ、解決した内容（rerere・ほかのエディタ）を片側で上書きしてしまう。
        if conflictStates[key.path]?.markers == 0 {
            let title = ours ? L("Take Current File") : L("Take Incoming File")
            Task { @MainActor in
                guard await actions.confirm(
                    L("Replace the resolved content of %@?", key.path),
                    message: L("No conflict markers are left, so it may already be resolved. It becomes one side’s whole file."),
                    ok: title) else { return }
                performTakeSide(key.path, ours: ours)
            }
            return
        }
        performTakeSide(key.path, ours: ours)
    }

    /// マーカーの残っていないコンフリクトのファイルを、まとめて解決済みにする（ウィンドウの帯から）。
    func markResolvedWithoutMarkers() {
        // 保存していない Result は、保存してから解決済みにする（Mark Resolved と同じ。黙って捨てない）。
        let paths = conflictsWithoutMarkers.filter { path in
            guard let editor = unsavedEditor(for: path) else { return true }
            editor.saveAndResolve()
            return false
        }
        guard !paths.isEmpty else { return }
        let repository = self.repository
        actions?.perform { try repository.stage(paths: paths) }
    }

    @objc private func markResolved() {
        guard let key = selectedKey, let actions else { return }
        // 保存していない Result があれば、エディタの Mark Resolved と同じく保存してから解決済みにする（git が解決済みに
        // するのは作業ツリーのファイルなので、Result の変更を書いてから。残りのコンフリクトは Result で数える）。
        if let editor = unsavedEditor(for: key.path) { return editor.saveAndResolve() }
        let repository = self.repository
        let path = key.path
        Task { @MainActor in
            // 残りのマーカーは裏で数える（SSH 先・daemon 越しでは往復がかかる）。
            let remaining = await Task.detached(priority: .userInitiated) {
                struct Count: Decodable { let count: Int }
                return (try? repository.info(["op": "conflictCount", "path": path], as: Count.self))?.count ?? 0
            }.value
            guard remaining > 0 else { return actions.perform { try repository.stage(paths: [path]) } }
            guard await actions.confirm(L("Mark %@ as resolved with %ld conflicts left?", path, remaining),
                                        message: L("The conflict markers stay in the file."), ok: L("Mark Resolved")) else { return }
            actions.perform { try repository.stage(paths: [path]) }
        }
    }

    @objc private func toggleAmend() {
        // 直前のコミットのメッセージを入れて直せるようにする。
        if amend.state == .on, message.string.isEmpty, summary.stringValue.isEmpty {
            let repository = self.repository
            Task {
                let text = try? await Task.detached { try repository.run(["op": "commitMessage", "oid": "HEAD"]).text }.value
                if amend.state == .on, message.string.isEmpty, summary.stringValue.isEmpty, let text = text ?? nil {
                    // 1 行目を Summary、空行の後を Description に分ける。
                    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
                    let parts = trimmed.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
                    summary.stringValue = String(parts.first ?? "")
                    message.string = parts.count > 1 ? parts[1].trimmingCharacters(in: .whitespacesAndNewlines) : ""
                }
                updateButtons()
            }
        }
        updateButtons()
    }

    @objc private func assistantChanged() {
        suggestButton.isHidden = !Assistant.isConfigured
    }

    /// AI: stage 済みの変更からメッセージを提案し、「メッセージに入れる」で Summary と Description に入れる。
    @objc private func suggestMessage() {
        guard let status else { return }
        let staged = status.items.filter { $0.isStaged && $0.kind != .unmerged }
        guard !staged.isEmpty else {
            NSSound.beep()
            return
        }
        guard Assistant.confirm(repository: repository.workdir ?? "remote", summary: L("The diff of %ld staged files", staged.count))
        else { return }
        let repository = self.repository, workdir = repository.workdir
        // 書き方の手本: 最近のコミットのメッセージ（まだ読めていなければ直前のもの）。
        let examples = recentCommits.prefix(10).map(\.message).joined(separator: "\n---\n")
        AssistantPanel.show(title: L("Suggested Commit Message"), over: view.window, useTitle: L("Use Message"), onUse: { [weak self] text in
            guard let self else { return }
            let parts = text.trimmingCharacters(in: .whitespacesAndNewlines)
                .split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
            summary.stringValue = String(parts.first ?? "")
            message.string = parts.count > 1 ? parts[1].trimmingCharacters(in: .whitespacesAndNewlines) : ""
            updateButtons()
        }) { useCache, onText in
            let (diff, recent) = await Task.detached(priority: .userInitiated) { () -> (String, String) in
                var out = ""
                for item in staged where out.utf8.count < DiffText.limit {
                    if let d = try? status.diff(of: item, staged: true) { out += DiffText.make(d, path: item.path) }
                }
                if !examples.isEmpty { return (out, examples) }
                let head = (try? repository.run(["op": "commitMessage", "oid": "HEAD"]).text) ?? nil
                return (out, head ?? "")
            }.value
            return try await Assistant.run(
                .suggestMessage,
                variables: ["diff": diff, "files": staged.map(\.path).joined(separator: "\n"), "recent_messages": recent],
                workdir: workdir, useCache: useCache, onText: onText)
        }
    }

    /// Summary と Description をつないだメッセージ。
    private var composedMessage: String {
        let head = summary.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        let body = message.string.trimmingCharacters(in: .whitespacesAndNewlines)
        return body.isEmpty ? head : head.isEmpty ? body : "\(head)\n\n\(body)"
    }

    @objc private func summaryChanged() {
        updateButtons()
        saveDraft()
    }

    /// 作業ツリーの変更（untracked を含む）をすべて次のコミットに含める。
    @objc private func stageAll() {
        let paths = allEntries.filter { !$0.isConflict && $0.item.isUnstaged }.map(\.item.path)
        guard !paths.isEmpty else { return }
        let repository = self.repository
        actions?.perform { try repository.stage(paths: paths) }
    }

    @objc private func commitClicked() { commit(thenPush: false) }

    /// 今のブランチに上流がない。Commit ▾ の Commit and Push を Commit and Publish（送り先を選ぶ）と見せる。
    var publishesBranch = false {
        didSet {
            guard publishesBranch != oldValue else { return }
            let title = publishesBranch ? "Commit and Publish…" : "Commit and Push"
            commitMenuButton.menu?.items.first { $0.action == #selector(commitAndPush) }?.title = title
            commitMenuButton.toolTip = publishesBranch ? L("Commit and Publish (⌘⇧↩), Amend") : L("Commit and Push (⌘⇧↩), Amend")
        }
    }

    /// Commit ▾ > Commit and Push（⌘⇧↩）: コミットできたら push する（上流がなければ Publish のシートを出す）。
    @objc private func commitAndPush() {
        guard canCommit else { return NSSound.beep() }
        commit(thenPush: true)
    }

    /// Commit ▾ の Commit and Push は、Commit を押せるときだけ（何も含めていない、メッセージが空なら切る）。
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(commitAndPush) { return canCommit }
        return super.validateUserInterfaceItem(item)
    }

    func commitAndPushFromMenu() { if canCommit { commitAndPush() } }

    /// コミットしている途中。
    private var committing = false

    /// コミットの hooks の出力と失敗を出す窓: リポジトリの窓（終えたときに Working Copy が窓にないことがある: hooks の
    /// 間に History などへ移った・ほかの worktree を選んだ。そのとき `view.window` は nil で、失敗が黙って消える）。
    private var sheetWindow: NSWindow? { actions?.window ?? view.window }

    private func commit(thenPush: Bool) {
        guard !committing else { return NSSound.beep() }
        let text = composedMessage
        let isAmend = amend.state == .on
        let repository = self.repository
        let started = UInt64(Date().timeIntervalSince1970 * 1000)
        committing = true
        updateButtons()
        Task {
            defer {
                committing = false
                updateButtons()
            }
            do {
                try await Task.detached(priority: .userInitiated) { try repository.commit(message: text, amend: isAmend) }.value
                // hooks（pre-commit・commit-msg など）が何か書いていれば見せる。
                if let output = ActivityRecord.all().last(where: {
                    $0.kind == "git" && $0.label.hasPrefix("git commit") && $0.started >= started && $0.ok == true
                })?.output, !output.isEmpty, let window = sheetWindow {
                    HookOutputSheet.show(title: L("Committed (hooks output)"), text: output, in: window)
                }
                message.string = ""
                summary.stringValue = ""
                amend.state = .off
                clearDraft()
                loadRecentCommits()
                if thenPush { actions?.pushWithoutAsking() }
            } catch {
                // hooks で止まったときは出力が長いので、読めるシートで出す。
                if let window = sheetWindow {
                    HookOutputSheet.show(title: L("Couldn’t Commit"), text: "\(error)", in: window)
                }
            }
            refresh()
            updateButtons()
        }
    }

    func textDidChange(_ notification: Notification) {
        updateButtons()
        if notification.object as? NSTextView === message { offerIssues(in: message) }
        if notification.object as? NSTextView === message { saveDraft() }
    }

    // MARK: - コミットの入力の補助

    /// 下書きを覚える鍵（リポジトリごと）。
    private var draftKey: String { "commitDraft." + (repository.workdir ?? "remote") }

    /// 書きかけのメッセージを覚える（コミットするまで、アプリを終えても残る）。
    private func saveDraft() {
        guard recentCursor < 0 else { return }
        let draft = [summary.stringValue, message.string]
        if draft.allSatisfy({ $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) {
            UserDefaults.standard.removeObject(forKey: draftKey)
        } else {
            UserDefaults.standard.set(draft, forKey: draftKey)
        }
    }

    private func clearDraft() {
        UserDefaults.standard.removeObject(forKey: draftKey)
        recentCursor = -1
        draftBeforeRecall = nil
    }

    /// 下書きがあればそれを、なければテンプレートを入れる: リポジトリが Settings > Git の共有の一覧から選んだもの
    /// （`amonhen.commitTemplate`）、なければ `commit.template` のファイル（このリポジトリ専用のものはコアから読むので
    /// SSH 先でも入る。ほかのファイルは手元のリポジトリだけ）。ファイルのほうがリポジトリに近い段で決まっていれば
    /// ファイル（設定のページと同じ決め方。`MessageTemplate`）。どれも # の行は除く。
    private func restoreDraftOrTemplate() {
        if let draft = UserDefaults.standard.stringArray(forKey: draftKey), draft.count == 2 {
            summary.stringValue = draft[0]
            message.string = draft[1]
            updateButtons()
            return
        }
        let repository = self.repository
        let workdir = repository.workdir
        Task {
            struct Entries: Decodable {
                struct Entry: Decodable { let scope: String; let key: String; let value: String }
                let entries: [Entry]
            }
            let entries = try? await Task.detached { try repository.info(["op": "configList"], as: Entries.self) }.value
            let entry = { (key: String) in entries?.entries.last { $0.key.lowercased() == key } }
            let value = { (key: String) in entry(key)?.value }
            let text: String
            if let shared = entry("amonhen.committemplate"), let t = Settings.commitTemplates.first(where: { $0.name == shared.value }),
               !MessageTemplate.sharedYields(shared: shared.scope, file: entry("commit.template").flatMap { $0.value.isEmpty ? nil : $0.scope }) {
                text = t.text
            } else {
                guard let path = value("commit.template") else { return }
                if path.hasSuffix("/amonhen/commit-template") {
                    struct Template: Decodable { let text: String? }
                    guard let own = try? await Task.detached { try repository.info(["op": "commitTemplate"], as: Template.self) }.value,
                          let read = own.text else { return }
                    text = read
                } else {
                    guard let workdir else { return }
                    let expanded = (path as NSString).expandingTildeInPath
                    let file = expanded.hasPrefix("/") ? expanded : (workdir as NSString).appendingPathComponent(expanded)
                    guard let read = try? String(contentsOfFile: file, encoding: .utf8) else { return }
                    text = read
                }
            }
            guard summary.stringValue.isEmpty, message.string.isEmpty else { return }
            let lines = text.components(separatedBy: "\n").filter { !$0.hasPrefix("#") }
            let body = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            let parts = body.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
            summary.stringValue = String(parts.first ?? "")
            message.string = parts.count > 1 ? parts[1].trimmingCharacters(in: .newlines) : ""
            updateButtons()
        }
    }

    /// 最近のコミット 30 個のメッセージと作者を裏で読み、一覧と Co-authored-by の候補にする。
    private func loadRecentCommits() {
        let repository = self.repository
        Task {
            let commits = await Task.detached(priority: .utility) { () -> [(String, String, String)] in
                guard let history = repository.latestHistory() else { return [] }
                return (0..<min(30, history.rowCount)).compactMap { row in
                    guard let id = history.commitID(row: row),
                          let info = try? repository.commitInfo(oid: id.map { String(format: "%02x", $0) }.joined())
                    else { return nil }
                    return (info.message.trimmingCharacters(in: .whitespacesAndNewlines), info.author, info.authorEmail)
                }
            }.value
            recentCommits = commits.map { (message: $0.0, author: $0.1, email: $0.2) }
            buildHistoryMenu()
            buildInsertMenu()
            buildFixupMenus()
        }
    }

    private func buildHistoryMenu() {
        let menu = historyButton.menu!
        while menu.items.count > 1 { menu.removeItem(at: 1) }
        var seen = Set<String>()
        for commit in recentCommits where seen.insert(commit.message).inserted {
            let subject = commit.message.components(separatedBy: "\n").first ?? ""
            let item = ClosureMenuItem(title: String(subject.prefix(80))) { [weak self] in self?.useMessage(commit.message) }
            item.toolTip = commit.message
            menu.addItem(item)
        }
        if menu.items.count == 1 { menu.addItem(NSMenuItem(title: L("Not loaded yet"), action: nil, keyEquivalent: "")) }
    }

    private func useMessage(_ text: String) {
        let parts = text.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
        summary.stringValue = String(parts.first ?? "")
        message.string = parts.count > 1 ? parts[1].trimmingCharacters(in: .whitespacesAndNewlines) : ""
        updateButtons()
        saveDraft()
    }

    /// Conventional Commits の型と、トレーラー（Co-authored-by・Signed-off-by）。
    private func buildInsertMenu() {
        let menu = insertButton.menu!
        while menu.items.count > 1 { menu.removeItem(at: 1) }
        let types = NSMenu(title: L("Type"))
        for type in ["feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert"] {
            types.addItem(ClosureMenuItem(title: "\(type):") { [weak self] in self?.setType(type) })
        }
        let typesItem = NSMenuItem(title: L("Conventional Commits Type"), action: nil, keyEquivalent: "")
        typesItem.submenu = types
        menu.addItem(typesItem)
        let authors = NSMenu(title: "Co-authored-by")
        var seen = Set<String>()
        for c in recentCommits where !c.email.isEmpty && seen.insert(c.email.lowercased()).inserted {
            authors.addItem(ClosureMenuItem(title: "\(c.author) <\(c.email)>") { [weak self] in
                self?.addTrailer("Co-authored-by: \(c.author) <\(c.email)>")
            })
        }
        if authors.items.isEmpty { authors.addItem(NSMenuItem(title: L("Recent authors aren’t loaded yet"), action: nil, keyEquivalent: "")) }
        let authorsItem = NSMenuItem(title: "Co-authored-by", action: nil, keyEquivalent: "")
        authorsItem.submenu = authors
        menu.addItem(authorsItem)
        menu.addItem(ClosureMenuItem(title: L("Signed-off-by (Me)")) { [weak self] in self?.addSignOff() })
    }

    /// 件名の先頭を `type: ` にする（もう型があれば置き換える）。
    private func setType(_ type: String) {
        let s = summary.stringValue
        let stripped = s.replacingOccurrences(of: #"^[a-z]+(\([^)]*\))?!?:\s*"#, with: "", options: .regularExpression)
        summary.stringValue = "\(type): " + stripped
        summaryChanged()
        view.window?.makeFirstResponder(summary)
    }

    /// 本文の末尾にトレーラーを足す（もうあれば足さない）。
    private func addTrailer(_ line: String) {
        guard !message.string.contains(line) else { return }
        var body = message.string.trimmingCharacters(in: .whitespacesAndNewlines)
        // トレーラーは本文と空行で分ける（前の行がトレーラーなら続ける）。
        let last = body.components(separatedBy: "\n").last ?? ""
        let lastIsTrailer = last.range(of: #"^[A-Za-z-]+: "#, options: .regularExpression) != nil
        body += body.isEmpty ? line : lastIsTrailer ? "\n" + line : "\n\n" + line
        // 打ったのと同じく ⌘Z で戻せるように、テキスト欄の編集として入れ替える（`string` に入れると Undo を捨てる）。
        let all = NSRange(location: 0, length: (message.string as NSString).length)
        if message.shouldChangeText(in: all, replacementString: body) {
            message.textStorage?.replaceCharacters(
                in: all, with: NSAttributedString(string: body, attributes: message.typingAttributes))
            message.didChangeText()
        }
        updateButtons()
        saveDraft()
    }

    private func addSignOff() {
        let repository = self.repository
        Task {
            struct Entries: Decodable {
                struct Entry: Decodable { let key: String; let value: String }
                let entries: [Entry]
            }
            let entries = (try? await Task.detached { try repository.info(["op": "configList"], as: Entries.self) }.value)?.entries ?? []
            let value = { (key: String) in entries.last { $0.key.lowercased() == key }?.value }
            guard let name = value("user.name"), let email = value("user.email") else { return NSSound.beep() }
            addTrailer("Signed-off-by: \(name) <\(email)>")
        }
    }

    /// Commit ▾ > Amend Last Commit: Amend を入れて、直前のメッセージを入れる。
    @objc private func startAmend() {
        amend.state = .on
        toggleAmend()
    }

    /// 件名の欄の ⌥↑・⌥↓ で、最近のメッセージを順に呼び出す。
    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard control === summary, NSApp.currentEvent?.modifierFlags.contains(.option) == true else { return false }
        // ⌥↑ には moveBackward: と moveToBeginningOfParagraph: の 2 つが割り当てられ、続けて届く。1 回だけ進むよう、
        // 後のほうにだけ反応する（⌥↓ も同じ）。
        let older = [#selector(NSResponder.moveToBeginningOfParagraph(_:)), #selector(NSResponder.moveUp(_:))].contains(selector)
        let newer = [#selector(NSResponder.moveToEndOfParagraph(_:)), #selector(NSResponder.moveDown(_:))].contains(selector)
        if [#selector(NSResponder.moveBackward(_:)), #selector(NSResponder.moveForward(_:))].contains(selector) { return true }
        guard older || newer, !recentCommits.isEmpty else { return false }
        if recentCursor < 0 { draftBeforeRecall = (summary.stringValue, message.string) }
        recentCursor = max(-1, min(recentCommits.count - 1, recentCursor + (older ? 1 : -1)))
        if recentCursor < 0, let draft = draftBeforeRecall {
            summary.stringValue = draft.0
            message.string = draft.1
            updateButtons()
        } else if recentCursor >= 0 {
            let text = recentCommits[recentCursor].message
            let parts = text.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
            summary.stringValue = String(parts.first ?? "")
            message.string = parts.count > 1 ? parts[1].trimmingCharacters(in: .whitespacesAndNewlines) : ""
            updateButtons()
        }
        return true
    }

    // MARK: - NSOutlineView

    // 一覧の子は、配列を作らずに元の配列から直接答える（行ごとに全体の配列を作ると n² になり、変更の多い
    // リポジトリで数秒止まった）。

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        switch item {
        case nil where sections != nil: sections!.count
        case let section as Section: section.tree.map { $0.dirs.count + $0.files.count } ?? section.entries.count
        case nil: tree.map { $0.dirs.count + $0.files.count } ?? entries.count
        case let dir as Dir: dir.dirs.count + dir.files.count
        default: 0
        }
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        if item == nil, let sections { return sections[index] }
        if let section = item as? Section {
            guard let dir = section.tree else { return section.entries[index] }
            return index < dir.dirs.count ? dir.dirs[index] : dir.files[index - dir.dirs.count]
        }
        let dir = item as? Dir ?? tree
        guard let dir else { return entries[index] }
        return index < dir.dirs.count ? dir.dirs[index] : dir.files[index - dir.dirs.count]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        item is Dir || item is Section
    }

    func outlineView(_ outlineView: NSOutlineView, isGroupItem item: Any) -> Bool {
        item is Section
    }

    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool {
        !(item is Section)
    }

    /// 節の見出し: 名前と数、Stage All・Unstage All。
    private func sectionHeader(_ section: Section) -> NSView {
        let cell = NSTableCellView()
        let title = NSTextField(labelWithString: section.title)
        title.font = .systemFont(ofSize: NSFont.smallSystemFontSize, weight: .semibold)
        title.textColor = .secondaryLabelColor
        let count = NSTextField(labelWithString: "\(section.entries.count)")
        count.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        count.textColor = .tertiaryLabelColor
        var views: [NSView] = [title, count, NSView()]
        if let side = section.side, !section.entries.isEmpty {
            let entries = section.entries
            let button = SectionActionButton(title: side == .staged ? "Unstage All" : "Stage All") { [weak self] in self?.toggle(entries) }
            button.isBordered = false
            button.controlSize = .small
            button.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            button.contentTintColor = .controlAccentColor
            views.append(button)
        }
        let stack = NSStackView(views: views)
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
            stack.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        if let section = item as? Section { return sectionHeader(section) }
        let cell = outlineView.makeView(withIdentifier: WorkingFileCell.identifier, owner: nil) as? WorkingFileCell ?? WorkingFileCell()
        switch item {
        case let dir as Dir:
            let states = Set(dir.allFiles.filter { !$0.isConflict }.map(\.check))
            let state: NSControl.StateValue = states == [.on] ? .on : states.isEmpty || states == [.off] ? .off : .mixed
            cell.show(check: states.isEmpty ? nil : state, mark: nil, name: dir.name + "/", dimmed: true, tip: nil)
            cell.onDiscard = nil
        case let entry as Entry:
            // 木ではディレクトリが上に出るので、ファイル名だけにする。Flat ではファイル名の後に親を薄く。
            let path = entry.item.path
            let fileName = (path as NSString).lastPathComponent
            let parent = (path as NSString).deletingLastPathComponent
            let name = layout == .list ? path : fileName
            let tip = entry.item.origPath.map { "\($0) → \(path)" } ?? path
            // マーカーの残っていないコンフリクトは、解決できていそうなことを印と短い注記で示す。
            let state = entry.isConflict ? conflictStates[path].flatMap { $0.markers == 0 ? $0 : nil } : nil
            cell.show(
                check: entry.isConflict ? nil : entry.check, mark: state != nil ? ("✓", .systemOrange) : Self.mark(of: entry),
                name: name, dimmed: false, tip: state.map { "\(tip)\n" + Self.withoutMarkersMessage(rerere: $0.rerere) } ?? tip,
                detail: state.map { $0.rerere ? "rerere" : L("no markers") } ?? (layout == .flat && !parent.isEmpty ? parent : nil),
                stat: entry.section == .staged ? splitStats?.staged[path]
                    : entry.section == .unstaged ? splitStats?.unstaged[path] ?? stats[path] : stats[path])
            cell.onDiscard = entry.isConflict || !entry.item.isUnstaged || entry.section == .staged ? nil
                : { [weak self] in self?.discardFile(entry) }
        default:
            break
        }
        cell.onToggle = { [weak self] in
            guard let self else { return }
            toggle(files(under: item))
        }
        return cell
    }

    /// マーカーの残っていないコンフリクトの説明（一覧のヒントとマージエディタの案内）。
    static func withoutMarkersMessage(rerere: Bool) -> String {
        rerere
            ? L("git rerere applied the earlier resolution. Check the result, then Mark Resolved.")
            : L("No conflict markers are left. Check the result, then Mark Resolved.")
    }

    /// 状態の印と色。作業ツリーの側を優先する（index にだけ変更があれば index の側）。
    private static func mark(of entry: Entry) -> (String, NSColor) {
        if entry.isConflict { return ("⚠", .systemRed) }
        let item = entry.item
        let state = item.worktreeState != "." ? item.worktreeState : item.indexState
        switch state {
        case "?": return ("U", .systemGreen)
        case "A": return ("A", .systemGreen)
        case "D": return ("D", .systemRed)
        case "R": return ("R", .systemBlue)
        default: return (String(state), .systemOrange)
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard let entry = files.item(atRow: files.clickedRow) as? Entry else { return }
        let path = entry.item.path
        func add(_ title: String, _ action: @escaping () -> Void) { menu.addItem(ClosureMenuItem(title: title, action: action)) }
        // 含める・外す・捨てる（チェックとホバーのボタンと同じ操作を、右クリックからも）。
        if !entry.isConflict {
            add(entry.check == .on ? "Unstage" : "Stage") { [weak self] in self?.toggle([entry]) }
            if entry.item.kind == .untracked {
                add(L("Delete %@…", (path as NSString).lastPathComponent)) { [weak self] in self?.discardFile(entry) }
            } else if entry.item.isUnstaged {
                add("Discard…") { [weak self] in self?.discardFile(entry) }
            }
            // 右クリックした行を含めて選んでいれば、選んだファイルの変更を。
            let chosen = files.selectedRowIndexes.contains(files.clickedRow)
                ? files.selectedRowIndexes.compactMap { files.item(atRow: $0) as? Entry } : [entry]
            let selected = chosen.map(\.item.path)
            let hasUntracked = chosen.contains { $0.item.kind == .untracked }
            add("Stash…") { [weak self] in self?.actions?.stash(paths: selected, untrackedChosen: hasUntracked) }
            menu.addItem(.separator())
        }
        // untracked のファイルにはまだ履歴がない。代わりに .gitignore に足せる（そのパスか、拡張子のパターン）。
        if entry.item.kind == .untracked {
            add(L("Add “%@” to .gitignore", path)) { [weak self] in self?.addToGitignore(path) }
            let ext = (path as NSString).pathExtension
            if !ext.isEmpty {
                add(L("Add “%@” to .gitignore", "*." + ext)) { [weak self] in self?.addToGitignore("*." + ext) }
            }
        } else {
            if entry.item.worktreeState != "D", entry.item.indexState != "D" {
                add(Titles.blame(at: nil)) { [weak self] in self?.onBlame?(path) }
            }
            add(Titles.fileHistory) { [weak self] in self?.onFileHistory?(path) }
        }
        menu.addItem(.separator())
        add(Titles.copyPath) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(path, forType: .string)
        }
        // 手元のリポジトリなら、Finder とほかのアプリで開ける（SSH 先のファイルは開けない）。
        if let workdir = repository.workdir, entry.item.worktreeState != "D", entry.item.indexState != "D" {
            let url = URL(fileURLWithPath: workdir).appendingPathComponent(path)
            add(Titles.showInFinder) { NSWorkspace.shared.activateFileViewerSelecting([url]) }
            add(L("Open in Editor")) { ExternalEditor.open([url]) }
            menu.items.last?.toolTip = ExternalEditor.toolTip
            add(L("Open with Default App")) { NSWorkspace.shared.open(url) }
        }
    }

    /// `pattern` を作業ツリーの直下の .gitignore の最後に足す（コアで読み書きするので、SSH 先でも同じ）。
    private func addToGitignore(_ pattern: String) {
        let repository = self.repository
        actions?.perform {
            nonisolated struct Ignore: Decodable { let gitignore: String; let exclude: String }
            let files = try repository.info(["op": "ignoreFiles"], as: Ignore.self)
            var gitignore = files.gitignore
            if gitignore.split(separator: "\n").contains(where: { $0.trimmingCharacters(in: .whitespaces) == pattern }) { return }
            if !gitignore.isEmpty, !gitignore.hasSuffix("\n") { gitignore += "\n" }
            gitignore += pattern + "\n"
            try repository.run(json: Repository.json(["op": "saveIgnoreFiles", "gitignore": gitignore, "exclude": files.exclude]))
        }
    }

    func outlineViewItemDidCollapse(_ notification: Notification) {
        guard !isExpandingTree, let dir = notification.userInfo?["NSObject"] as? Dir else { return }
        collapsedDirs.insert(dir.path)
    }

    func outlineViewItemDidExpand(_ notification: Notification) {
        guard !isExpandingTree, let dir = notification.userInfo?["NSObject"] as? Dir else { return }
        collapsedDirs.remove(dir.path)
        // 中のディレクトリも、畳んでいなければ開く（開いたことがなければ畳んだままになる）。
        expandTree(below: dir)
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard !isRestoringSelection else { return }
        // 2 つ以上選んだら、選んだファイルの差分を並べる（すべてのファイルを見ている間は 1 つ）。
        let chosen = files.selectedRowIndexes.compactMap { files.item(atRow: $0) as? Entry }.filter { !$0.isConflict }
        if !showsAllFiles, chosen.count >= 2 { return showMulti(chosen) }
        guard let entry = files.item(atRow: files.selectedRow) as? Entry else { return }
        let sideChanged = adoptSection(of: entry)
        if !sideChanged, selectedKey?.path == entry.item.path, current != nil || isConflict { return }
        showDiff(of: entry, staged: side(for: entry.item))
    }
}

/// Working Copy の一覧の 1 行: 次のコミットに含めるかのチェックボックス、状態の印、パス。
final class WorkingFileCell: NSTableCellView {
    static let identifier = NSUserInterfaceItemIdentifier("file")
    var onToggle: (() -> Void)?
    /// ホバーで出す「変更を捨てる」（nil なら出さない）。
    var onDiscard: (() -> Void)?
    private let checkbox = NSButton(checkboxWithTitle: "", target: nil, action: nil)
    private let mark = NSTextField(labelWithString: "")
    private let name = NSTextField(labelWithString: "")
    /// Flat で、ファイル名の後に薄く出す親のディレクトリ。
    private let detail = NSTextField(labelWithString: "")
    /// 追加・削除の行数。
    private let stat = NSTextField(labelWithString: "")
    private let discardButton = NSButton()

    init() {
        super.init(frame: .zero)
        identifier = Self.identifier
        checkbox.allowsMixedState = true
        checkbox.target = self
        checkbox.action = #selector(toggled)
        mark.font = .monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .semibold)
        mark.alignment = .center
        name.lineBreakMode = .byTruncatingMiddle
        detail.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        detail.textColor = .tertiaryLabelColor
        detail.lineBreakMode = .byTruncatingHead
        stat.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        stat.alignment = .right
        discardButton.image = AppIcons.image(symbol: "arrow.uturn.backward", label: L("Discard Changes"))
        discardButton.isBordered = false
        discardButton.bezelStyle = .accessoryBarAction
        discardButton.toolTip = L("Discard the working tree changes (return to the content of the index)")
        discardButton.target = self
        discardButton.action = #selector(discardClicked)
        discardButton.isHidden = true
        let stack = NSStackView(views: [checkbox, mark, name, detail, NSView(), stat, discardButton])
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        mark.widthAnchor.constraint(equalToConstant: 14).isActive = true
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        detail.setContentCompressionResistancePriority(.init(200), for: .horizontal)
        stat.setContentCompressionResistancePriority(.required, for: .horizontal)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    // ホバーしている間だけ「変更を捨てる」を出す（行数の代わりに）。
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) { setHover(true) }
    override func mouseExited(with event: NSEvent) { setHover(false) }

    private func setHover(_ on: Bool) {
        discardButton.isHidden = !on || onDiscard == nil
    }

    @objc private func discardClicked() { onDiscard?() }

    required init?(coder: NSCoder) { fatalError() }

    /// `check` が nil ならチェックボックスを出さない（コンフリクトのファイル）。`mark` が nil なら印を出さない。
    func show(
        check: NSControl.StateValue?, mark: (String, NSColor)?, name: String, dimmed: Bool, tip: String?,
        detail: String? = nil, stat: (added: Int?, removed: Int?)? = nil
    ) {
        self.detail.stringValue = detail ?? ""
        self.detail.isHidden = detail == nil
        let text = NSMutableAttributedString()
        if let stat {
            let font = NSFont.monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
            if stat.added == nil, stat.removed == nil {
                text.append(NSAttributedString(string: "bin", attributes: [.font: font, .foregroundColor: NSColor.secondaryLabelColor]))
            } else {
                if let a = stat.added, a > 0 {
                    text.append(NSAttributedString(string: "+\(a)", attributes: [.font: font, .foregroundColor: NSColor.systemGreen]))
                }
                if let r = stat.removed, r > 0 {
                    text.append(NSAttributedString(string: (text.length > 0 ? " " : "") + "−\(r)",
                                                   attributes: [.font: font, .foregroundColor: NSColor.systemRed]))
                }
            }
        }
        self.stat.attributedStringValue = text
        discardButton.isHidden = true
        checkbox.isHidden = check == nil
        checkbox.state = check ?? .off
        checkbox.toolTip = switch check {
        case .on?: L("Included in the next commit (click to exclude)")
        case .mixed?: L("Only some lines are included (click to include all)")
        default: L("Not included in the next commit (click to include)")
        }
        self.mark.isHidden = mark == nil
        self.mark.stringValue = mark?.0 ?? ""
        self.mark.textColor = mark?.1 ?? .labelColor
        self.name.stringValue = name
        self.name.textColor = dimmed ? .secondaryLabelColor : .labelColor
        toolTip = tip
    }

    @objc private func toggled() {
        // 押した時点で AppKit が状態を進めるが、正しい状態は index を読み直して決める。
        onToggle?()
    }
}

/// 左に見出し、右にボタン。ボタンは元の幅のまま右に寄せ、見出しは残りの幅に収める（長いパスは途中を省く）。
final class TitleAndButtonsBar: NSView {
    private let title: NSTextField
    private let buttons: NSStackView

    init(title: NSTextField, buttons: NSStackView) {
        self.title = title
        self.buttons = buttons
        super.init(frame: .zero)
        addSubview(title)
        addSubview(buttons)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        arrange()
    }

    override func layout() {
        super.layout()
        arrange()
    }

    private func arrange() {
        let inset: CGFloat = 8
        let size = buttons.fittingSize
        buttons.frame = NSRect(
            x: max(inset, bounds.width - inset - size.width), y: (bounds.height - size.height) / 2,
            width: size.width, height: size.height)
        let titleHeight = title.intrinsicContentSize.height
        title.frame = NSRect(
            x: inset, y: (bounds.height - titleHeight) / 2, width: max(0, buttons.frame.minX - inset * 2),
            height: titleHeight)
    }
}

/// 空のときに薄い案内を出すテキスト欄。`guideColumn` があれば、その桁に目安の縦線を引く（72 桁で改行する慣習）。
final class PlaceholderTextView: NSTextView {
    var placeholder = "" { didSet { needsDisplay = true } }

    /// この欄だけの Undo（窓の UndoManager を使うと、下で捨てるときに Summary・埋め込んだマージエディタの Result・
    /// Repository Settings の取り消しまで消える）。
    private let ownUndoManager = UndoManager()
    override var undoManager: UndoManager? { allowsUndo ? ownUndoManager : super.undoManager }

    /// コードで中身を入れ替えたら、前に打ったものの Undo は捨てる（入れ替えた後の文に当てると範囲がずれる。
    /// マージエディタの Result と同じ）。
    override var string: String {
        get { super.string }
        set {
            super.string = newValue
            if allowsUndo { ownUndoManager.removeAllActions() }
        }
    }
    var guideColumn: Int? { didSet { needsDisplay = true } }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        if let column = guideColumn, let font {
            let width = ("0" as NSString).size(withAttributes: [.font: font]).width
            let x = textContainerInset.width + (textContainer?.lineFragmentPadding ?? 0) + width * CGFloat(column)
            NSColor.separatorColor.setFill()
            NSRect(x: x, y: bounds.minY, width: 1, height: bounds.height).fill()
        }
        guard string.isEmpty, !placeholder.isEmpty else { return }
        let attrs: [NSAttributedString.Key: Any] = [
            .font: font ?? NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.placeholderTextColor,
        ]
        let origin = NSPoint(x: textContainerInset.width + (textContainer?.lineFragmentPadding ?? 0), y: textContainerInset.height)
        placeholder.draw(at: origin, withAttributes: attrs)
    }

    override func didChangeText() {
        super.didChangeText()
        needsDisplay = true
    }
}

/// 上に高さ固定の帯、残りに中身を置く。
final class BarAndContentView: NSView {
    private let bar: NSView
    /// 中身（差し替えられる）。
    var content: NSView {
        didSet {
            guard content !== oldValue else { return }
            oldValue.removeFromSuperview()
            addSubview(content)
            arrange()
        }
    }
    private let barHeight: CGFloat = 34

    init(bar: NSView, content: NSView) {
        self.bar = bar
        self.content = content
        super.init(frame: .zero)
        addSubview(bar)
        addSubview(content)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        arrange()
    }

    override func layout() {
        super.layout()
        arrange()
    }

    private func arrange() {
        bar.frame = NSRect(x: 0, y: 0, width: bounds.width, height: barHeight)
        content.frame = NSRect(x: 0, y: barHeight, width: bounds.width, height: max(0, bounds.height - barHeight))
    }
}

/// Space・Enter で選んだファイルを次のコミットに含める・外し、⌫・Delete で変更を捨てる一覧。
final class KeyOutlineView: NSOutlineView {
    var onToggle: (() -> Void)?
    var onDiscard: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        switch event.charactersIgnoringModifiers {
        case " ", "\r":
            onToggle?()
        // ⌫（delete）と ⌦（forward delete、NSDeleteFunctionKey）。
        case "\u{7f}", "\u{f728}":
            onDiscard?()
        default:
            super.keyDown(with: event)
        }
    }
}

/// hooks の出力（git が標準エラーに書いたもの）を等幅の字体で読めるシート。
enum HookOutputSheet {
    static func show(title: String, text: String, in window: NSWindow) {
        let alert = NSAlert()
        alert.messageText = title
        alert.addButton(withTitle: "OK")
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 560, height: 260))
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        let view = NSTextView(frame: scroll.bounds)
        view.isEditable = false
        view.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        view.string = text
        view.autoresizingMask = [.width]
        scroll.documentView = view
        alert.accessoryView = scroll
        alert.beginSheetModal(for: window)
    }
}

/// コミットの入力と差分の間の仕切りの決まり: 上は最低限の高さを保ち、下にも差分の分を残し、窓の高さが
/// 変わったら下だけが伸び縮みする。
final class MessageSplitDelegate: NSObject, NSSplitViewDelegate {
    var minimum: () -> CGFloat = { 170 }
    /// 上の段の最大の高さ（nil なら下に 140 残す）。
    var maximum: (() -> CGFloat)?

    func splitView(_ splitView: NSSplitView, constrainMinCoordinate proposedMinimumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
        max(proposedMinimumPosition, minimum())
    }

    func splitView(_ splitView: NSSplitView, constrainMaxCoordinate proposedMaximumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
        min(proposedMaximumPosition, maximum?() ?? splitView.bounds.height - 140)
    }

    func splitView(_ splitView: NSSplitView, shouldAdjustSizeOfSubview view: NSView) -> Bool {
        view !== splitView.arrangedSubviews.first
    }
}

/// 近道（keyEquivalent）を、条件が合う間だけほかに譲るボタン。
private final class YieldingButton: NSButton {
    var yieldsKeyEquivalent: (() -> Bool)?

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if yieldsKeyEquivalent?() == true { return false }
        return super.performKeyEquivalent(with: event)
    }
}

/// 節の見出しの、押したら `action` を呼ぶボタン（Stage All・Unstage All）。
private final class SectionActionButton: NSButton {
    private var handler: (() -> Void)?

    convenience init(title: String, action: @escaping () -> Void) {
        self.init(frame: .zero)
        self.title = title
        handler = action
        target = self
        self.action = #selector(run)
    }

    @objc private func run() { handler?() }
}
