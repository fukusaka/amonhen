import AmonhenKit
import AppKit

/// Stashes: 左に stash の一覧（新しい順）と Apply・Pop・Drop、右に選んだ stash の中身（History の詳細と同じ欄）。
/// stash の一覧はすべての worktree で共有だが、戻す先は書き込む先の worktree。
final class StashesViewController: NSSplitViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    /// 書き込む先の worktree の操作（worktree を変えると差し替わる）。
    var actions: Actions?
    /// 書き込む先の worktree の名前（worktree が 2 つ以上あるときだけ）。戻す先として帯に出す。
    var worktreeName: String? {
        didSet { updateTarget() }
    }
    /// 書き込む先のブランチ。ほかのブランチで作った stash は、ブランチの名前を薄くして見分ける。
    var currentBranch: String? {
        didSet { if currentBranch != oldValue { table.reloadData() } }
    }

    let detail: CommitDetailViewController
    private let repository: Repository
    private let table = NSTableView()
    private let applyButton = NSButton()
    private let popButton = NSButton()
    private let dropButton = NSButton()
    private let branchButton = NSButton()
    private let target = NSTextField(labelWithString: "")
    private var stashes: [Stash] = []
    /// stash のコミットの情報（土台・untracked を含むか・日時）。ID で引く。stash の数は少ないので全部読む。
    private var infos: [Data: CommitInfo] = [:]
    /// 土台と、書き込む先の worktree の HEAD の離れ具合（stash の名前で引く）。
    private var bases: [String: StashBase] = [:]
    private var basesRequest = 0

    /// 一覧の 1 行。
    private struct Stash {
        let item: RefItem
        let id: Data
        /// 作ったときのブランチ（メッセージの `WIP on main:`・`On main:` から。detached HEAD なら `(no branch)`）。
        let branch: String?
        /// 見出し: `-m` のメッセージ。なければ WIP と、作ったときの HEAD の件名。
        let title: String

        init?(_ item: RefItem) {
            guard let id = item.target else { return nil }
            self.item = item
            self.id = id
            let message = item.detail ?? ""
            let wip = message.hasPrefix("WIP on ")
            if let colon = message.range(of: ": "), wip || message.hasPrefix("On ") {
                branch = String(message[message.index(message.startIndex, offsetBy: wip ? 7 : 3)..<colon.lowerBound])
                let rest = message[colon.upperBound...]
                // WIP の残りは「短い ID 件名」なので、ID は外す（土台は 2 行目に出す）。
                title = wip ? "WIP · " + (rest.split(separator: " ", maxSplits: 1).dropFirst().first.map(String.init) ?? "") : String(rest)
            } else {
                branch = nil
                title = message
            }
        }
    }

    init(repository: Repository, renderer: TextRenderer) {
        self.repository = repository
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

    private func makeList() -> NSView {
        for (button, title, action, tip) in [
            (applyButton, "Apply", #selector(applyStash), L("Apply the stash to the working tree and keep it in the list")),
            (popButton, "Pop", #selector(popStash), L("Apply the stash to the working tree and remove it from the list")),
            (dropButton, L("Drop…"), #selector(dropStash), L("Remove the stash without applying it (you can Undo it)")),
            (branchButton, L("Branch…"), #selector(branchFromStash),
             L("Create a branch at the commit the stash was made on, switch to it and apply the stash there (git stash branch)")),
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
        let bar = NSStackView(views: [applyButton, popButton, dropButton, branchButton, target])
        bar.spacing = 6
        bar.edgeInsets = NSEdgeInsets(top: 6, left: 8, bottom: 6, right: 8)
        bar.setCustomSpacing(12, after: branchButton)

        let column = NSTableColumn(identifier: .init("stash"))
        table.addTableColumn(column)
        table.headerView = nil
        table.style = .inset
        table.rowHeight = 40
        table.usesAutomaticRowHeights = false
        table.dataSource = self
        table.delegate = self
        table.target = self
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
        return stack
    }

    /// ref の一覧の stash（新しい順）を出す。選んでいた stash は ID で選び直す（番号は積むたびにずれる）。
    func show(_ items: [RefItem]) {
        // 画面を開く前に ref が届いても選べるよう、一覧を先に作っておく。
        loadViewIfNeeded()
        let next = items.filter { $0.kind == .stash }.compactMap(Stash.init)
        guard next.map(\.item) != stashes.map(\.item) else { return }
        let selected = stashes.indices.contains(table.selectedRow) ? stashes[table.selectedRow].id : nil
        stashes = next
        table.reloadData()
        let row = next.firstIndex { $0.id == selected } ?? (next.isEmpty ? nil : 0)
        if let row {
            table.selectRowIndexes([row], byExtendingSelection: false)
        }
        // 選び直した行が前と同じ stash なら、選択の変化の知らせは来ないので、ここで出す。
        if row == nil || next[row!].id != selected { showSelected() }
        loadInfos()
        updateButtons()
    }

    /// 土台と HEAD の離れ具合を読み直す（ref が変わった・書き込む先の worktree を変えたとき）。
    func reloadBases() {
        guard let repository = actions?.repository, !stashes.isEmpty else { return }
        basesRequest += 1
        let request = basesRequest
        Task {
            let loaded = (try? await Task.detached(priority: .utility) { try repository.stashBases() }.value) ?? []
            guard request == basesRequest else { return }
            let next = Dictionary(loaded.map { ($0.name, $0) }, uniquingKeysWith: { a, _ in a })
            guard next != bases else { return }
            bases = next
            table.reloadData()
        }
    }

    /// まだ読んでいない stash のコミットの情報を裏で読む。
    private func loadInfos() {
        let missing = stashes.map(\.id).filter { infos[$0] == nil }
        guard !missing.isEmpty else { return }
        let repository = self.repository
        Task {
            let loaded = await Task.detached(priority: .utility) {
                missing.compactMap { id in
                    (try? repository.commitInfo(oid: id.map { String(format: "%02x", $0) }.joined())).map { (id, $0) }
                }
            }.value
            for (id, info) in loaded { infos[id] = info }
            table.reloadData()
        }
    }

    private var selected: Stash? {
        stashes.indices.contains(table.selectedRow) ? stashes[table.selectedRow] : nil
    }

    private func showSelected() {
        detail.show(commit: selected?.id, stash: true)
        updateButtons()
    }

    private func updateButtons() {
        for button in [applyButton, popButton, dropButton, branchButton] { button.isEnabled = selected != nil }
    }

    private func updateTarget() {
        target.stringValue = worktreeName.map { L("Applies to ✎ %@", $0) } ?? ""
    }

    // MARK: - 操作

    @objc private func applyStash() { selected.map { apply($0, pop: false) } }
    @objc private func popStash() { selected.map { apply($0, pop: true) } }
    @objc private func dropStash() { selected.map(drop) }
    @objc private func branchFromStash() { selected.map(branch(from:)) }

    /// `git stash branch`: 土台から新しいブランチを作って切り替え、そこに戻す。HEAD が進んでいても重ならない。
    private func branch(from stash: Stash) {
        guard let actions else { return }
        let name = stash.item.name
        Task { @MainActor in
            guard let branch = await actions.ask(
                L("New Branch from %@", name),
                message: L("Created where the stash was made, checked out, and the stash applied there (then dropped)."),
                ok: L("Create")) else { return }
            actions.perform(["op": "stashBranch", "name": name, "branch": branch])
        }
    }

    private func apply(_ stash: Stash, pop: Bool) {
        actions?.perform(["op": "stashApply", "name": stash.item.name, "pop": pop])
    }

    private func drop(_ stash: Stash) {
        guard let actions else { return }
        let name = stash.item.name
        Task { @MainActor in
            guard await actions.confirm(L("Drop %@?", name), message: (stash.item.detail.map { "“\($0)”\n" } ?? "") + Actions.canUndo,
                                        ok: "Drop") else { return }
            actions.perform(["op": "stashDrop", "name": name])
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let row = table.clickedRow
        guard stashes.indices.contains(row) else { return }
        let stash = stashes[row]
        let name = stash.item.name
        func add(_ title: String, _ handler: @escaping () -> Void) {
            menu.addItem(ClosureMenuItem(title: title, action: handler))
        }
        add("Apply \(name)") { [weak self] in self?.apply(stash, pop: false) }
        add("Pop \(name)") { [weak self] in self?.apply(stash, pop: true) }
        add(L("Drop %@…", name)) { [weak self] in self?.drop(stash) }
        add(L("New Branch from %@…", name)) { [weak self] in self?.branch(from: stash) }
        menu.addItem(.separator())
        add(L("Copy Name")) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(name, forType: .string)
        }
    }

    // MARK: - NSTableView

    func numberOfRows(in tableView: NSTableView) -> Int { stashes.count }

    func tableViewSelectionDidChange(_ notification: Notification) { showSelected() }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("stash"), owner: nil) as? StashCell ?? StashCell()
        let stash = stashes[row]
        let title = NSMutableAttributedString(string: stash.item.name + "  ", attributes: [
            .foregroundColor: NSColor.secondaryLabelColor,
            .font: NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular),
        ])
        title.append(NSAttributedString(string: stash.title, attributes: [.foregroundColor: NSColor.labelColor]))
        cell.title.attributedStringValue = title
        cell.title.toolTip = stash.item.detail

        // 2 行目: 作ったブランチ・土台と HEAD の関係・いつ・untracked を含むか・土台の短い ID（狭ければ後ろから切れる）。
        let small = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        let sub = NSMutableAttributedString()
        func part(_ text: String, _ color: NSColor = .secondaryLabelColor) {
            if sub.length > 0 { sub.append(NSAttributedString(string: " · ", attributes: [.font: small, .foregroundColor: NSColor.tertiaryLabelColor])) }
            sub.append(NSAttributedString(string: text, attributes: [.font: small, .foregroundColor: color]))
        }
        if let branch = stash.branch {
            part(branch, branch == currentBranch ? .secondaryLabelColor : .tertiaryLabelColor)
        }
        // 土台が今の HEAD の履歴にない（ほかの系統で作った）と、戻すときに重なりやすい。
        if let b = bases[stash.item.name] {
            if b.behind > 0 {
                part(L("not in HEAD’s history"), .systemOrange)
            } else {
                part(b.ahead == 0 ? L("at HEAD") : L("%ld behind HEAD", b.ahead))
            }
        }
        if let info = infos[stash.id] {
            part(Self.relative.localizedString(for: Date(timeIntervalSince1970: TimeInterval(info.committerTime)), relativeTo: Date()))
            // 3 つ目の親が untracked のファイル（`git stash -u`）。
            if info.parents.count > 2 { part(L("with untracked files")) }
            if let base = info.parents.first { part(String(base.prefix(7))) }
        }
        cell.subtitle.attributedStringValue = sub
        return cell
    }

    private static let relative: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f
    }()
}

/// stash の一覧の行（見出しと、ブランチ・土台・いつの 2 行）。
private final class StashCell: NSTableCellView {
    let title = NSTextField(labelWithString: "")
    let subtitle = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        identifier = .init("stash")
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
