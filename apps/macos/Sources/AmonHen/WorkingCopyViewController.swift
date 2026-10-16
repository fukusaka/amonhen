import AmonhenKit
import AppKit

/// Working Copy: 左の上にコミットの入力、その下に変更のあるファイルの 1 つの一覧（チェックボックスで次のコミットに
/// 含めるか）。右に選んだファイルの差分（行を選んで stage・unstage）。
/// チェックボックスは index の状態そのもので、FE だけの選択状態は持たない（ターミナルで `git add` しても揃う）。
final class WorkingCopyViewController: NSSplitViewController, NSOutlineViewDataSource, NSOutlineViewDelegate,
    NSTextViewDelegate, NSMenuDelegate, NSSearchFieldDelegate
{
    /// ファイルの右クリック: 作業ツリーの内容の blame（パス）とファイルの履歴（パス）。
    var onBlame: ((String) -> Void)?
    var onFileHistory: ((String) -> Void)?
    private let repository: Repository
    private let diffView: DiffViewController
    private let files = KeyOutlineView()
    /// コミットメッセージの 1 行目と、2 行目以降。
    private let summary = NSTextField()
    private let summaryCount = NSTextField(labelWithString: "")
    private let message = PlaceholderTextView()
    /// ファイルの一覧をディレクトリの木で見せるか。
    private let layoutControl = NSSegmentedControl()
    private var showsTree: Bool {
        get { UserDefaults.standard.bool(forKey: "workingCopyTree") }
        set { UserDefaults.standard.set(newValue, forKey: "workingCopyTree") }
    }
    /// 一覧の見出し: すべてを含める・外すチェックボックスと、含める数。
    private let allCheckbox = NSButton(checkboxWithTitle: "Changes", target: nil, action: nil)
    private let countLabel = NSTextField(labelWithString: "")
    /// パスで一覧を絞り込む。
    private let filterField = NSSearchField()
    private let emptyLabel = NSTextField(labelWithString: "変更はありません")
    private var diffBar: TitleAndButtonsBar?
    private let amend = NSButton(checkboxWithTitle: "Amend", target: nil, action: nil)
    private let commitButton = NSButton(title: "Commit", target: nil, action: nil)
    /// 一部だけ stage したファイルで、作業ツリーの側と stage 済みの側のどちらの差分を見せるか。
    private let sideControl = NSSegmentedControl(labels: ["Unstaged", "Staged"], trackingMode: .selectOne, target: nil, action: nil)
    private let linesButton = NSButton(title: "Stage Lines", target: nil, action: nil)
    private let discardButton = NSButton(title: "Discard Lines", target: nil, action: nil)
    var actions: Actions?
    private let titleLabel = NSTextField(labelWithString: "")
    /// merge・rebase などの途中のときに出す帯。
    private let sequenceBar = NSStackView()
    private let sequenceLabel = NSTextField(labelWithString: "")

    /// 一覧の 1 ファイル。
    private final class Entry {
        let item: StatusItem
        init(item: StatusItem) { self.item = item }
        var isConflict: Bool { item.kind == .unmerged }
        /// 次のコミットに含めるか（index の状態）。
        var check: NSControl.StateValue {
            if isConflict || !item.isStaged { return .off }
            return item.isUnstaged ? .mixed : .on
        }
    }

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
        var dirs: [Dir] = []
        var files: [Entry] = []
        init(name: String) { self.name = name }
        var children: [Any] { dirs as [Any] + files as [Any] }
        /// 配下のすべてのファイル。
        var allFiles: [Entry] { files + dirs.flatMap(\.allFiles) }
    }
    private var tree: Dir?
    private var current: WorkingFileDiff?
    /// 選んでいたファイルと、見せていた側（読み直した後に選び直す）。
    private var selectedKey: (path: String, staged: Bool)?
    private var request = 0
    private var isConflict = false
    /// 右の欄（帯と、差分かマージエディタ）。
    private var rightView: BarAndContentView?
    private var rightController: NSViewController?
    /// コンフリクトのファイルごとのマージエディタ（ほかのファイルに移っても、編集中の内容を保つ）。
    private var mergeEditors: [String: MergeEditorViewController] = [:]
    private var embeddedEditor: MergeEditorViewController?

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
        message.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        message.isAutomaticQuoteSubstitutionEnabled = false
        message.isAutomaticDashSubstitutionEnabled = false
        message.delegate = self
        message.textContainerInset = NSSize(width: 4, height: 4)
        message.placeholder = "Description（任意）"
        summary.placeholderString = "Summary"
        summaryCount.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        summaryCount.textColor = .tertiaryLabelColor
        summaryCount.alignment = .right
        summary.font = .systemFont(ofSize: 13)
        summary.target = self
        summary.action = #selector(summaryChanged)
        NotificationCenter.default.addObserver(
            self, selector: #selector(summaryChanged), name: NSControl.textDidChangeNotification, object: summary)
        let messageScroll = NSScrollView()
        messageScroll.documentView = message
        messageScroll.hasVerticalScroller = true
        messageScroll.borderType = .bezelBorder
        message.autoresizingMask = [.width]
        message.isVerticallyResizable = true
        message.textContainer?.widthTracksTextView = true

        commitButton.target = self
        commitButton.action = #selector(commit)
        commitButton.keyEquivalent = "\r"
        commitButton.keyEquivalentModifierMask = .command
        commitButton.toolTip = "チェックしたファイルをコミット（⌘↩）"
        amend.target = self
        amend.action = #selector(toggleAmend)
        let commitRow = NSStackView(views: [amend, NSView(), commitButton])
        let summaryRow = NSStackView(views: [summary, summaryCount])
        summaryCount.setContentHuggingPriority(.required, for: .horizontal)
        summaryCount.widthAnchor.constraint(equalToConstant: 28).isActive = true
        let separator = NSBox()
        separator.boxType = .separator

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
        allCheckbox.toolTip = "一覧のファイルをすべて次のコミットに含める・外す"
        countLabel.textColor = .secondaryLabelColor
        countLabel.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        layoutControl.segmentCount = 2
        layoutControl.setImage(NSImage(systemSymbolName: "list.bullet", accessibilityDescription: "リスト"), forSegment: 0)
        layoutControl.setImage(NSImage(systemSymbolName: "list.bullet.indent", accessibilityDescription: "ツリー"), forSegment: 1)
        layoutControl.setToolTip("ファイルをパスの一覧で表示", forSegment: 0)
        layoutControl.setToolTip("ファイルをディレクトリの木で表示", forSegment: 1)
        layoutControl.controlSize = .small
        layoutControl.selectedSegment = showsTree ? 1 : 0
        layoutControl.target = self
        layoutControl.action = #selector(layoutChanged)
        let filesHeader = NSStackView(views: [allCheckbox, countLabel, NSView(), layoutControl])
        filterField.placeholderString = "パスで絞り込む"
        filterField.controlSize = .small
        filterField.delegate = self

        let column = NSTableColumn(identifier: .init("path"))
        files.addTableColumn(column)
        files.outlineTableColumn = column
        files.headerView = nil
        files.style = .sourceList
        files.allowsMultipleSelection = true
        files.dataSource = self
        files.delegate = self
        // Space か Enter で、選んだファイルを次のコミットに含める・外す。
        files.onToggle = { [weak self] in self?.toggleSelected() }
        let fileMenu = NSMenu()
        fileMenu.delegate = self
        files.menu = fileMenu
        let filesScroll = NSScrollView()
        filesScroll.documentView = files
        filesScroll.hasVerticalScroller = true
        emptyLabel.textColor = .secondaryLabelColor
        emptyLabel.alignment = .center
        emptyLabel.translatesAutoresizingMaskIntoConstraints = false
        filesScroll.addSubview(emptyLabel)
        emptyLabel.centerXAnchor.constraint(equalTo: filesScroll.centerXAnchor).isActive = true
        emptyLabel.centerYAnchor.constraint(equalTo: filesScroll.centerYAnchor).isActive = true

        let views = [sequenceBar, summaryRow, messageScroll, commitRow, separator, filesHeader, filterField, filesScroll]
        let left = NSStackView(views: views)
        left.orientation = .vertical
        left.alignment = .leading
        left.spacing = 6
        left.edgeInsets = NSEdgeInsets(top: 8, left: 8, bottom: 8, right: 8)
        for v in views {
            v.widthAnchor.constraint(equalTo: left.widthAnchor, constant: -16).isActive = true
        }
        left.setCustomSpacing(10, after: commitRow)
        left.setCustomSpacing(10, after: separator)
        messageScroll.heightAnchor.constraint(equalToConstant: 72).isActive = true
        left.frame = NSRect(x: 0, y: 0, width: 280, height: 500)
        let leftController = NSViewController()
        leftController.view = left
        let leftItem = NSSplitViewItem(viewController: leftController)
        leftItem.minimumThickness = 220
        leftItem.holdingPriority = .init(260)
        addSplitViewItem(leftItem)

        // 右: 操作ボタンと差分。
        sideControl.target = self
        sideControl.action = #selector(sideChanged)
        sideControl.controlSize = .small
        sideControl.setToolTip("作業ツリーにあって、まだ stage していない変更", forSegment: 0)
        sideControl.setToolTip("stage 済み（次のコミットに含める）の変更", forSegment: 1)
        linesButton.target = self
        linesButton.action = #selector(stageLines)
        titleLabel.lineBreakMode = .byTruncatingMiddle
        discardButton.target = self
        discardButton.action = #selector(discard)
        // コンフリクトのファイルは、下にマージエディタを出す。帯にはファイル全体の操作を置く。
        let conflictActions: [(String, Selector)] = [
            ("Take Current File", #selector(takeOursFile)), ("Take Incoming File", #selector(takeTheirsFile)),
            ("Mark Resolved", #selector(markResolved)),
        ]
        conflictButtons.setViews(conflictActions.map { NSButton(title: $0.0, target: self, action: $0.1) }, in: .leading)
        conflictButtons.isHidden = true
        let actionButtons = NSStackView(views: [sideControl, conflictButtons, discardButton, linesButton])
        let bar = TitleAndButtonsBar(title: titleLabel, buttons: actionButtons)
        diffBar = bar
        diffView.isSelectable = true
        diffView.onSelectionChange = { [weak self] in self?.updateButtons() }
        diffView.onSetStaged = { [weak self] rows, staged in self?.setStaged(rows: rows, staged: staged) }
        let right = BarAndContentView(bar: bar, content: diffView.view)
        right.frame = NSRect(x: 0, y: 0, width: 700, height: 500)
        // NSSplitViewController に addChild すると分割の項目が増えるので、右側の親に付ける。
        let rightController = NSViewController()
        rightController.view = right
        rightController.addChild(diffView)
        self.rightView = right
        self.rightController = rightController
        let rightItem = NSSplitViewItem(viewController: rightController)
        rightItem.minimumThickness = 300
        addSplitViewItem(rightItem)
        updateButtons()
    }

    func overlays() -> [(CGImage, NSView)] {
        view.window == nil ? [] : diffView.overlay().map { [$0] } ?? []
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
        Task {
            async let sequence = Task.detached { try? repository.run(["op": "sequence"]).text }.value
            async let status = Task.detached(priority: .userInitiated) { try? repository.loadStatus() }.value
            let (s, w) = await (sequence, status)
            showSequence(s ?? nil)
            if let w, w.generation != self.status?.generation { show(w) }
            isRefreshing = false
            if needsRefresh {
                needsRefresh = false
                refresh()
            }
        }
    }

    private func show(_ status: WorkingStatus) {
        self.status = status
        // コンフリクトを先に、あとはパスの順。
        // 解決済みになったファイルのエディタは捨てる。
        let conflicted = Set(status.items.filter { $0.kind == .unmerged }.map(\.path))
        mergeEditors = mergeEditors.filter { conflicted.contains($0.key) }
        if let editor = embeddedEditor, !conflicted.contains(editor.path) { unembed() }
        allEntries = status.items.map(Entry.init).sorted {
            $0.isConflict != $1.isConflict ? $0.isConflict : $0.item.path < $1.item.path
        }
        notifySequence()
        rebuildList()
    }

    /// 絞り込みと見せ方（一覧・木）に合わせて並べ直し、選んでいたファイルを選び直す。
    private func rebuildList() {
        let query = filterField.stringValue.trimmingCharacters(in: .whitespaces).lowercased()
        entries = query.isEmpty ? allEntries : allEntries.filter { $0.item.path.lowercased().contains(query) }
        tree = showsTree ? Self.makeTree(entries) : nil
        files.reloadData()
        files.expandItem(nil, expandChildren: true)
        emptyLabel.stringValue = allEntries.isEmpty ? "変更はありません" : "一致するファイルはありません"
        emptyLabel.isHidden = !entries.isEmpty
        updateHeader()

        // 同じファイルを選び直す。なければ最初のもの。
        let target = selectedKey.flatMap { key in entries.first { $0.item.path == key.path } } ?? entries.first
        if let target {
            let row = files.row(forItem: target)
            files.selectRowIndexes([row], byExtendingSelection: false)
            let keep = target.item.path == selectedKey?.path
            showDiff(of: target, staged: side(for: target.item), keepScroll: keep)
        } else {
            current = nil
            selectedKey = nil
            diffView.show(nil, message: allEntries.isEmpty ? "変更はありません" : "")
            titleLabel.stringValue = ""
            updateButtons()
        }
    }

    /// `item` の差分で見せる側。stage 済みの変更だけなら stage 済み、作業ツリーにも変更があれば作業ツリー。
    /// 両方にあるファイルを選び直すときは、前に見せていた側のまま。
    private func side(for item: StatusItem) -> Bool {
        if !item.isUnstaged { return item.isStaged }
        if !item.isStaged { return false }
        return selectedKey?.path == item.path ? selectedKey?.staged ?? false : false
    }

    /// 一覧の見出し: すべてのチェックの状態と、含める数 / 全体の数。
    private func updateHeader() {
        let candidates = entries.filter { !$0.isConflict }
        let states = Set(candidates.map(\.check))
        allCheckbox.state = states == [.on] ? .on : states.isEmpty || states == [.off] ? .off : .mixed
        allCheckbox.isEnabled = !candidates.isEmpty
        let included = allEntries.filter { $0.item.isStaged && !$0.isConflict }.count
        countLabel.stringValue = allEntries.isEmpty ? "" : "\(included) / \(allEntries.count)"
        countLabel.toolTip = "次のコミットに含めるファイル / 変更のあるファイル"
    }

    /// ファイルをディレクトリの木にする。
    private static func makeTree(_ entries: [Entry]) -> Dir {
        let root = Dir(name: "")
        for entry in entries {
            var dir = root
            let parts = entry.item.path.split(separator: "/").map(String.init)
            for part in parts.dropLast() {
                if let next = dir.dirs.first(where: { $0.name == part }) {
                    dir = next
                } else {
                    let next = Dir(name: part)
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

    @objc private func layoutChanged() {
        showsTree = layoutControl.selectedSegment == 1
        rebuildList()
    }

    func controlTextDidChange(_ obj: Notification) {
        guard (obj.object as? NSSearchField) === filterField else { return }
        rebuildList()
    }

    private func showDiff(of entry: Entry, staged: Bool, keepScroll: Bool = false) {
        guard let status else { return }
        selectedKey = (entry.item.path, staged)
        titleLabel.stringValue = entry.item.path
        request += 1
        let request = self.request
        isConflict = entry.isConflict
        diffView.isConflictView = entry.isConflict
        // 一部だけ含めたファイルで、1 本の差分にできないときだけ、見る側を切り替える。
        sideControl.isHidden = true
        sideControl.selectedSegment = staged ? 1 : 0
        if entry.isConflict {
            let path = entry.item.path
            current = nil
            updateButtons()
            if let editor = mergeEditors[path] {
                embed(editor)
                return
            }
            Task {
                do {
                    let editor = try await MergeEditorViewController.load(path: path, repository: repository, actions: actions!)
                    guard request == self.request else { return }
                    mergeEditors[path] = editor
                    embed(editor)
                } catch {
                    guard request == self.request else { return }
                    unembed()
                    diffView.show(nil, message: "読めません: \(error)")
                }
            }
            return
        }
        unembed()
        Task {
            // HEAD と作業ツリーの 1 本の差分（行ごとのチェック）。表せないときは、選んだ側の差分。
            let (diff, combined) = await Task.detached(priority: .userInitiated) { () -> (WorkingFileDiff?, Bool) in
                if let combined = try? status.combinedDiff(of: entry.item) { return (combined, true) }
                return (try? status.diff(of: entry.item, staged: staged), false)
            }.value
            guard request == self.request else { return }
            sideControl.isHidden = combined || !(entry.item.isStaged && entry.item.isUnstaged)
            selectedKey = (entry.item.path, combined ? false : staged)
            current = diff
            if let diff, diff.isBinary {
                diffView.show(nil, message: "バイナリファイル")
            } else {
                diffView.show(diff, keepScroll: keepScroll)
            }
            updateButtons()
        }
    }

    @objc private func sideChanged() {
        guard let entry = files.item(atRow: files.selectedRow) as? Entry else { return }
        showDiff(of: entry, staged: sideControl.selectedSegment == 1)
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
            MergeEditorWindowController.show(editor, actions: actions)
            diffView.show(nil, message: "マージエディタを別ウィンドウで開いています")
        }
        editor.onResolved = { [weak self, weak editor] in
            guard let self, let editor else { return }
            if embeddedEditor === editor { unembed() }
            mergeEditors[editor.path] = nil
        }
        rightController.addChild(editor)
        rightView.content = editor.view
        embeddedEditor = editor
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
        let selectedStaged = current?.isCombined == true && !selected.isEmpty
            && selected.allSatisfy { current?.isStaged(row: $0) == true }
        // 選んだファイルがなければ、ファイルの操作は出さない。
        linesButton.isHidden = isConflict || selectedKey == nil
        linesButton.title = staged || selectedStaged ? "Unstage Lines" : "Stage Lines"
        linesButton.isEnabled = current != nil && !selected.isEmpty
        linesButton.toolTip = current?.isCombined == true ? "選んだ行を次のコミットに含める・外す（Space）" : nil
        // 選んだ行があれば行を、なければファイルを破棄する。stage 済みの変更は破棄しない。
        discardButton.isHidden = staged || isConflict || selectedKey == nil
        discardButton.title = selected.isEmpty ? "Discard File" : "Discard Lines"
        discardButton.isEnabled = selectedKey != nil
            && !(current?.isCombined == true && selected.contains { current?.isStaged(row: $0) == true })
        discardButton.toolTip = selected.isEmpty
            ? "作業ツリーの変更を捨て、次のコミットに含める内容（index）に戻す"
            : "選んだ行の変更を作業ツリーから捨てる（含めている行は先に外す）"
        let count = stagedCount
        commitButton.isEnabled = !(count == 0 && amend.state == .off) && !composedMessage.isEmpty
        // ボタンの数や名前が変わると幅が変わるので、並べ直す。
        diffBar?.needsLayout = true
        commitButton.title = amend.state == .on ? "Amend Commit"
            : count == 0 ? "Commit" : count == 1 ? "Commit 1 File" : "Commit \(count) Files"
        // 1 行目の長さ。72 文字を超えたら目立たせる（git の慣習）。
        let length = summary.stringValue.count
        summaryCount.stringValue = length == 0 ? "" : "\(length)"
        summaryCount.textColor = length > 72 ? .systemOrange : length > 50 ? .systemYellow : .tertiaryLabelColor
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
        guard let current else { return }
        let rows = diffView.selectedChangeRows
        guard !rows.isEmpty else { return }
        if current.isCombined {
            setStaged(rows: rows, staged: !rows.allSatisfy { current.isStaged(row: $0) })
        } else {
            run { try current.stage(rows: rows) }
        }
    }

    /// 1 本の差分の `rows` 行目の変更を、次のコミットに含める・外す。
    private func setStaged(rows: [Int], staged: Bool) {
        guard let current, current.isCombined else { return }
        run { try current.setStaged(rows: rows, staged: staged) }
    }

    @objc private func discard() {
        guard let key = selectedKey, !key.staged, let actions else { return }
        let rows = diffView.selectedChangeRows
        if let current, !rows.isEmpty {
            actions.perform { try current.discard(rows: rows) }
        } else if current?.item.kind == .untracked {
            guard actions.confirm(
                "\(key.path) を削除しますか？", message: "untracked のファイルなので、Undo では戻せません。", ok: "削除") else { return }
            // ファイルの操作はコアに任せる（SSH 越しのリポジトリでも同じに動く）。
            actions.perform(["op": "deleteUntracked", "paths": [key.path]])
        } else {
            actions.perform(["op": "discard", "paths": [key.path]])
        }
    }

    /// merge・rebase などの途中なら、その名前（Repository > Continue・Skip・Abort を使えるか）。
    private(set) var sequence: String?

    /// Commit > Commit（⌘↩）・Stage All を使えるか。
    var canCommit: Bool { commitButton.isEnabled }
    var canStageAll: Bool { allEntries.contains { !$0.isConflict && $0.item.isUnstaged } }

    /// メニューから: ボタンと同じ。
    func commitFromMenu() { if canCommit { commit() } }
    func stageAllFromMenu() { stageAll() }

    /// merge・rebase などの途中かどうかと、コンフリクトのファイルの数が変わった（ウィンドウの帯に出す）。
    var onSequenceChange: ((_ sequence: String?, _ conflicts: Int) -> Void)?

    private func showSequence(_ sequence: String?) {
        self.sequence = sequence
        // 途中であることと Continue・Skip・Abort は、ウィンドウの帯に出す（どの画面でも見えるように）。
        sequenceBar.isHidden = true
        notifySequence()
    }

    /// 変更のあるファイルの数が変わった（タブの名前に出す）。
    var onChangeCount: ((Int) -> Void)?

    private func notifySequence() {
        onSequenceChange?(sequence, allEntries.filter(\.isConflict).count)
        onChangeCount?(allEntries.count)
    }

    @objc private func sequenceAction(_ sender: NSButton) {
        guard let op = sender.identifier?.rawValue else { return }
        actions?.perform(["op": op])
    }

    @objc private func takeOursFile() {
        guard let key = selectedKey else { return }
        actions?.perform(["op": "takeSide", "path": key.path, "ours": true])
    }

    @objc private func takeTheirsFile() {
        guard let key = selectedKey else { return }
        actions?.perform(["op": "takeSide", "path": key.path, "ours": false])
    }

    @objc private func markResolved() {
        guard let key = selectedKey else { return }
        struct Count: Decodable { let count: Int }
        let remaining = (try? repository.info(["op": "conflictCount", "path": key.path], as: Count.self))?.count ?? 0
        if remaining > 0,
           actions?.confirm("\(key.path) にコンフリクトが \(remaining) 個残っています", message: "このまま解決済みにしますか？", ok: "解決済みにする") != true {
            return
        }
        let repository = self.repository
        let path = key.path
        actions?.perform { try repository.stage(paths: [path]) }
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

    /// Summary と Description をつないだメッセージ。
    private var composedMessage: String {
        let head = summary.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        let body = message.string.trimmingCharacters(in: .whitespacesAndNewlines)
        return body.isEmpty ? head : head.isEmpty ? body : "\(head)\n\n\(body)"
    }

    @objc private func summaryChanged() {
        updateButtons()
    }

    /// 作業ツリーの変更（untracked を含む）をすべて次のコミットに含める。
    @objc private func stageAll() {
        let paths = allEntries.filter { !$0.isConflict && $0.item.isUnstaged }.map(\.item.path)
        guard !paths.isEmpty else { return }
        let repository = self.repository
        actions?.perform { try repository.stage(paths: paths) }
    }

    @objc private func commit() {
        let text = composedMessage
        let isAmend = amend.state == .on
        let repository = self.repository
        Task {
            do {
                try await Task.detached(priority: .userInitiated) { try repository.commit(message: text, amend: isAmend) }.value
                message.string = ""
                summary.stringValue = ""
                amend.state = .off
            } catch {
                _ = await NSAlert(error: error).beginSheetModal(for: view.window!)
            }
            refresh()
            updateButtons()
        }
    }

    func textDidChange(_ notification: Notification) {
        updateButtons()
    }

    // MARK: - NSOutlineView

    private func children(of item: Any?) -> [Any] {
        switch item {
        case nil: tree?.children ?? entries
        case let dir as Dir: dir.children
        default: []
        }
    }

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        children(of: item).count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        children(of: item)[index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        item is Dir
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        let cell = outlineView.makeView(withIdentifier: WorkingFileCell.identifier, owner: nil) as? WorkingFileCell ?? WorkingFileCell()
        switch item {
        case let dir as Dir:
            let states = Set(dir.allFiles.filter { !$0.isConflict }.map(\.check))
            let state: NSControl.StateValue = states == [.on] ? .on : states.isEmpty || states == [.off] ? .off : .mixed
            cell.show(check: states.isEmpty ? nil : state, mark: nil, name: dir.name + "/", dimmed: true, tip: nil)
        case let entry as Entry:
            // 木ではディレクトリが上に出るので、ファイル名だけにする。
            let name = showsTree ? (entry.item.path.split(separator: "/").last.map(String.init) ?? entry.item.path) : entry.item.path
            let tip = entry.item.origPath.map { "\($0) → \(entry.item.path)" } ?? entry.item.path
            cell.show(check: entry.isConflict ? nil : entry.check, mark: Self.mark(of: entry), name: name, dimmed: false, tip: tip)
        default:
            break
        }
        cell.onToggle = { [weak self] in
            guard let self else { return }
            toggle(files(under: item))
        }
        return cell
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
        // untracked のファイルにはまだ履歴がない。
        guard entry.item.kind != .untracked else { return }
        if entry.item.worktreeState != "D" {
            menu.addItem(ClosureMenuItem(title: "Blame") { [weak self] in self?.onBlame?(path) })
        }
        menu.addItem(ClosureMenuItem(title: "File History") { [weak self] in self?.onFileHistory?(path) })
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard let entry = files.item(atRow: files.selectedRow) as? Entry else { return }
        if selectedKey?.path == entry.item.path, current != nil || isConflict { return }
        showDiff(of: entry, staged: side(for: entry.item))
    }
}

/// Working Copy の一覧の 1 行: 次のコミットに含めるかのチェックボックス、状態の印、パス。
final class WorkingFileCell: NSTableCellView {
    static let identifier = NSUserInterfaceItemIdentifier("file")
    var onToggle: (() -> Void)?
    private let checkbox = NSButton(checkboxWithTitle: "", target: nil, action: nil)
    private let mark = NSTextField(labelWithString: "")
    private let name = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        identifier = Self.identifier
        checkbox.allowsMixedState = true
        checkbox.target = self
        checkbox.action = #selector(toggled)
        mark.font = .monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .semibold)
        mark.alignment = .center
        name.lineBreakMode = .byTruncatingMiddle
        let stack = NSStackView(views: [checkbox, mark, name])
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        mark.widthAnchor.constraint(equalToConstant: 14).isActive = true
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    /// `check` が nil ならチェックボックスを出さない（コンフリクトのファイル）。`mark` が nil なら印を出さない。
    func show(check: NSControl.StateValue?, mark: (String, NSColor)?, name: String, dimmed: Bool, tip: String?) {
        checkbox.isHidden = check == nil
        checkbox.state = check ?? .off
        checkbox.toolTip = switch check {
        case .on?: "次のコミットに含める（クリックで外す）"
        case .mixed?: "一部の行だけ含める（クリックですべて含める）"
        default: "次のコミットに含めない（クリックで含める）"
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

/// 空のときに薄い案内を出すテキスト欄。
final class PlaceholderTextView: NSTextView {
    var placeholder = "" { didSet { needsDisplay = true } }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
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

/// Space・Enter で選んだファイルを次のコミットに含める・外す一覧。
final class KeyOutlineView: NSOutlineView {
    var onToggle: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        switch event.charactersIgnoringModifiers {
        case " ", "\r":
            onToggle?()
        default:
            super.keyDown(with: event)
        }
    }
}
