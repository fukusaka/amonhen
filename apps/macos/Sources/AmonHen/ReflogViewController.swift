import AmonhenKit
import AppKit

/// Reflog: 左に書き込む先の worktree の HEAD の reflog（新しい順）、右に選んだ行のコミットの中身（History の詳細と
/// 同じ欄）。reset・rebase・amend で外れたコミットを見つけて、ブランチを作って取り戻すのに使う。
final class ReflogViewController: NSSplitViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    /// 書き込む先の worktree の操作（worktree を変えると差し替わる。reflog もその worktree の HEAD のもの）。
    var actions: Actions? {
        didSet { if actions !== oldValue { reload() } }
    }
    /// 書き込む先の worktree の名前（worktree が 2 つ以上あるときだけ）。どの HEAD の reflog かを帯に出す。
    var worktreeName: String? {
        didSet { updateTarget() }
    }
    /// 辿れるコミットを History で選ぶ。
    var onShowInHistory: ((Data) -> Void)?
    /// Range Diff で比べる（git range-diff と同じ形の範囲）。
    var onRangeDiff: (([String]) -> Void)?

    let detail: CommitDetailViewController
    private let table = NSTableView()
    private let branchButton = NSButton()
    private let checkoutButton = NSButton()
    private let target = NSTextField(labelWithString: "")
    private var entries: [ReflogEntry] = []
    private var request = 0
    /// 見えていない間に変わった（次に出すときに読み直す）。
    private var stale = true

    init(repository: Repository, renderer: TextRenderer) {
        detail = CommitDetailViewController(repository: repository, renderer: renderer)
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        splitView.isVertical = true
        let list = NSViewController()
        list.view = makeList()
        let listItem = NSSplitViewItem(viewController: list)
        listItem.minimumThickness = 280
        listItem.holdingPriority = .init(260)
        addSplitViewItem(listItem)
        let detailItem = NSSplitViewItem(viewController: detail)
        detailItem.minimumThickness = 420
        addSplitViewItem(detailItem)
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        if stale { reload() }
    }

    private func makeList() -> NSView {
        for (button, title, action, tip) in [
            (branchButton, "New Branch…", #selector(newBranch), L("Create a branch at this commit and switch to it")),
            (checkoutButton, "Checkout", #selector(checkout), L("Check out this commit as a detached HEAD")),
        ] {
            button.title = title
            button.bezelStyle = .push
            button.controlSize = .small
            button.target = self
            button.action = action
            button.toolTip = tip
        }
        target.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        target.textColor = .secondaryLabelColor
        target.lineBreakMode = .byTruncatingMiddle
        target.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let bar = NSStackView(views: [branchButton, checkoutButton, target])
        bar.spacing = 6
        bar.edgeInsets = NSEdgeInsets(top: 6, left: 8, bottom: 6, right: 8)
        bar.setCustomSpacing(12, after: checkoutButton)

        let column = NSTableColumn(identifier: .init("reflog"))
        table.addTableColumn(column)
        table.headerView = nil
        table.style = .inset
        table.rowHeight = 40
        table.usesAutomaticRowHeights = false
        table.dataSource = self
        table.delegate = self
        let menu = NSMenu()
        menu.delegate = self
        table.menu = menu
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false

        let stack = NSStackView(views: [bar, scroll])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 0
        for view in [bar, scroll] {
            view.translatesAutoresizingMaskIntoConstraints = false
            view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
        updateButtons()
        updateTarget()
        return stack
    }

    /// reflog を読み直す（HEAD が動いた・書き込む先の worktree を変えたとき）。見えていなければ、次に出すときに読む。
    func reload() {
        guard isViewLoaded, view.window != nil else {
            stale = true
            return
        }
        guard let repository = actions?.repository else { return }
        stale = false
        request += 1
        let request = self.request
        Task {
            let loaded = (try? await Task.detached(priority: .userInitiated) { try repository.reflog() }.value) ?? []
            guard request == self.request else { return }
            show(loaded)
        }
    }

    /// 行を入れ替える。選んでいた行は、同じ動き（コミットと日時とメッセージ）を選び直す（番号は動くたびにずれる）。
    private func show(_ next: [ReflogEntry]) {
        guard next != entries else { return }
        let same = { (a: ReflogEntry, b: ReflogEntry) in a.new == b.new && a.time == b.time && a.message == b.message }
        let previous = selected
        entries = next
        table.reloadData()
        let row = previous.flatMap { p in next.firstIndex { same($0, p) } } ?? (next.isEmpty ? nil : 0)
        if let row {
            table.selectRowIndexes([row], byExtendingSelection: false)
        }
        // 選び直した行が前と同じなら、選択の変化の知らせは来ないので、ここで出す。
        if row == nil || previous.map({ !same(next[row!], $0) }) ?? true { showSelected() }
        updateButtons()
    }

    private var selected: ReflogEntry? {
        entries.indices.contains(table.selectedRow) ? entries[table.selectedRow] : nil
    }

    private func showSelected() {
        detail.show(commit: selected.flatMap { Data(hex: $0.new) })
        updateButtons()
    }

    private func updateButtons() {
        for button in [branchButton, checkoutButton] { button.isEnabled = selected != nil }
    }

    private func updateTarget() {
        target.stringValue = worktreeName.map { L("HEAD of ✎ %@", $0) } ?? ""
    }

    // MARK: - 操作

    @objc private func newBranch() { selected.map { actions?.newBranch(at: $0.new) } }
    @objc private func checkout() { selected.map { actions?.checkoutWithConfirmation(.detach($0.new)) } }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let row = table.clickedRow
        guard let actions, entries.indices.contains(row) else { return }
        let entry = entries[row]
        let hex = entry.new
        let short = String(hex.prefix(8))
        func add(_ title: String, _ handler: @escaping () -> Void) {
            menu.addItem(ClosureMenuItem(title: title, action: handler))
        }
        add(Titles.newBranch(from: short)) { actions.newBranch(at: hex) }
        add(Titles.newTag(at: short)) { actions.newTag(at: hex) }
        add(Titles.checkoutDetached(short)) { actions.checkoutWithConfirmation(.detach(hex)) }
        menu.addItem(actions.resetMenuItem(to: hex))
        // この時点の HEAD と今の HEAD を、同じ一連のコミットの 2 つの版として比べる。
        if row > 0 {
            menu.addItem(.separator())
            add(L("Compare with Current HEAD (Range Diff)")) { [weak self] in self?.onRangeDiff?(["\(hex)...HEAD"]) }
        }
        if entry.reachable, let id = Data(hex: hex) {
            menu.addItem(.separator())
            add(L("Show in History")) { [weak self] in self?.onShowInHistory?(id) }
        }
        menu.addItem(.separator())
        add(Titles.copyCommitID) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(hex, forType: .string)
        }
        add(L("Copy Name")) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(entry.selector, forType: .string)
        }
    }

    // MARK: - NSTableView

    func numberOfRows(in tableView: NSTableView) -> Int { entries.count }

    func tableViewSelectionDidChange(_ notification: Notification) { showSelected() }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("reflog"), owner: nil) as? ReflogCell ?? ReflogCell()
        let entry = entries[row]
        // 1 行目: 番号と、何をしたか（git reflog と同じ）。
        let title = NSMutableAttributedString(string: entry.selector + "  ", attributes: [
            .foregroundColor: NSColor.secondaryLabelColor,
            .font: NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular),
        ])
        title.append(NSAttributedString(string: entry.message.isEmpty ? "—" : entry.message, attributes: [.foregroundColor: NSColor.labelColor]))
        cell.title.attributedStringValue = title
        cell.title.toolTip = entry.message

        // 2 行目: どのブランチにも残っていないか・いつ・短い ID・件名（狭ければ後ろから切れる）。
        let small = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        let sub = NSMutableAttributedString()
        func part(_ text: String, _ color: NSColor = .secondaryLabelColor) {
            if sub.length > 0 { sub.append(NSAttributedString(string: " · ", attributes: [.font: small, .foregroundColor: NSColor.tertiaryLabelColor])) }
            sub.append(NSAttributedString(string: text, attributes: [.font: small, .foregroundColor: color]))
        }
        // ブランチから外れたコミットは、ここからしか辿れない（reflog が切れると gc で消える）。
        if !entry.reachable { part(L("not on any branch"), .systemOrange) }
        part(Self.relative.localizedString(for: Date(timeIntervalSince1970: TimeInterval(entry.time)), relativeTo: Date()))
        part(String(entry.new.prefix(7)))
        if !entry.subject.isEmpty { part(entry.subject) }
        cell.subtitle.attributedStringValue = sub
        cell.subtitle.toolTip = entry.subject
        return cell
    }

    private static let relative: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f
    }()
}

/// reflog の行（何をしたかと、いつ・ID・件名の 2 行）。
private final class ReflogCell: NSTableCellView {
    let title = NSTextField(labelWithString: "")
    let subtitle = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        identifier = .init("reflog")
        for field in [title, subtitle] {
            field.lineBreakMode = .byTruncatingTail
            field.maximumNumberOfLines = 1
            field.translatesAutoresizingMaskIntoConstraints = false
            field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            addSubview(field)
        }
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            title.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            subtitle.trailingAnchor.constraint(equalTo: title.trailingAnchor),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 1),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }
}
