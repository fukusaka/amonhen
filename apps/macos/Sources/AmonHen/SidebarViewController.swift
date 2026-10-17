import AmonhenKit
import AppKit

/// サイドバー: Branches / Pull Requests / Remotes / Tags / Stashes / Submodules。
final class SidebarViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate, NSMenuDelegate {
    /// 項目を選んだとき（指すコミットがあれば History でその行を選ぶ）。
    var onSelect: ((RefItem) -> Void)?
    var actions: Actions?
    /// チェックアウト中のブランチ。
    private var headBranch: String?
    /// サブモジュールを新しいウィンドウで開く（作業ツリーからのパス）。
    var onOpenSubmodule: ((String) -> Void)?
    /// Working Copy か History を選んだとき。
    var onSelectPage: ((Page) -> Void)?
    /// プルリクエストの右クリック: チェックアウト（gh pr checkout）。
    var onCheckoutPullRequest: ((PullRequest) -> Void)?

    enum Page {
        case workingCopy, history
    }

    private final class Node {
        var title: String
        let item: RefItem?
        let page: Page?
        /// Remotes の下のリモートそのもの（ブランチを落とすと push する）。
        var remote: String?
        var children: [Node]
        let isHead: Bool
        /// 右に薄く出す数（Working Copy の変更の数）。
        var badge: String?
        /// Pull Requests の下のプルリクエスト。
        var pullRequest: PullRequest?

        init(title: String, item: RefItem? = nil, page: Page? = nil, children: [Node] = [], isHead: Bool = false) {
            self.title = title
            self.item = item
            self.page = page
            self.children = children
            self.isHead = isHead
        }
    }

    private let workingCopyNode = Node(title: "Working Copy", page: .workingCopy)
    private let historyNode = Node(title: "History", page: .history)
    /// コンフリクトがあるときだけ出す。選ぶと Working Copy で最初のコンフリクトのファイルを開く。
    private let conflictsNode = Node(title: L("Conflicts"), page: .workingCopy)
    private lazy var workspace = Node(title: "Workspace", children: [workingCopyNode, historyNode])
    /// Conflicts を選んだ（最初のコンフリクトのファイルを開く）。
    var onSelectConflicts: (() -> Void)?

    /// コンフリクトのファイルの数（0 なら Conflicts を出さない）。
    func setConflictCount(_ count: Int) {
        let shown = workspace.children.contains { $0 === conflictsNode }
        conflictsNode.badge = count > 0 ? "\(count)" : nil
        if (count > 0) != shown {
            workspace.children = count > 0 ? [workingCopyNode, historyNode, conflictsNode] : [workingCopyNode, historyNode]
            outline.reloadItem(workspace, reloadChildren: true)
        } else if shown {
            outline.reloadItem(conflictsNode)
        }
    }

    private let outline = NSOutlineView()
    private var groups: [Node] = []
    private var generation: UInt64?

    override func loadView() {
        let column = NSTableColumn(identifier: .init("name"))
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.headerView = nil
        outline.style = .sourceList
        outline.rowSizeStyle = .default
        outline.floatsGroupRows = false
        outline.dataSource = self
        outline.delegate = self
        // 見出しは、押せば開く・閉じる（既定ではカーソルを載せたときの「表示」でしか開けず、閉じた見出しが空に見える）。
        outline.target = self
        outline.action = #selector(outlineClicked)
        outline.allowsTypeSelect = true
        let menu = NSMenu()
        menu.delegate = self
        outline.menu = menu
        outline.registerForDraggedTypes([.branchName])
        outline.setDraggingSourceOperationMask([.move, .copy], forLocal: true)

        let scroll = NSScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        view = scroll
    }

    private var refs: Refs?
    private var pullRequests: [PullRequest] = []

    func show(_ refs: Refs) {
        guard refs.generation != generation else { return }
        let first = generation == nil
        generation = refs.generation
        self.refs = refs
        rebuild(first: first)
    }

    /// 開いているプルリクエスト（GitHub のリポジトリで gh があるときだけ）。
    func show(pullRequests: [PullRequest]) {
        guard pullRequests != self.pullRequests else { return }
        self.pullRequests = pullRequests
        rebuild(first: false)
    }

    private func rebuild(first: Bool) {
        guard let refs else { return }
        headBranch = refs.headBranch
        let expanded = Set(groups.filter { outline.isItemExpanded($0) }.map(\.title))
        let selectedTitle = (outline.item(atRow: outline.selectedRow) as? Node).map { ($0.title, $0.page) }

        func items(_ kind: RefKind) -> [RefItem] { refs.items.filter { $0.kind == kind } }
        var groups: [Node] = []
        // feature/x のような名前はフォルダにまとめる。
        let branchRoot = Node(title: "Branches")
        for item in items(.local) {
            var parent = branchRoot
            let parts = item.name.split(separator: "/").map(String.init)
            for folder in parts.dropLast() {
                if let existing = parent.children.first(where: { $0.item == nil && $0.title == folder }) {
                    parent = existing
                } else {
                    let node = Node(title: folder)
                    parent.children.append(node)
                    parent = node
                }
            }
            parent.children.append(Node(title: parts.last ?? item.name, item: item, isHead: item.name == refs.headBranch))
        }
        if !branchRoot.children.isEmpty { groups.append(branchRoot) }

        if !pullRequests.isEmpty {
            groups.append(Node(title: "Pull Requests", children: pullRequests.map { pr in
                let node = Node(title: "#\(pr.number) \(pr.title)")
                node.pullRequest = pr
                node.badge = [pr.isDraft ? "draft" : nil, pr.ciMark.isEmpty ? nil : pr.ciMark].compactMap { $0 }.joined(separator: " ")
                return node
            }))
        }

        // リモートごとにまとめる（origin/main → origin の下の main）。
        var remotes: [String: [Node]] = [:]
        for item in items(.remote) {
            let parts = item.name.split(separator: "/", maxSplits: 1).map(String.init)
            remotes[parts[0], default: []].append(Node(title: parts.count > 1 ? parts[1] : item.name, item: item))
        }
        if !remotes.isEmpty {
            groups.append(Node(title: "Remotes", children: remotes.keys.sorted().map {
                let node = Node(title: $0, children: remotes[$0]!)
                node.remote = $0
                return node
            }))
        }
        for (kind, title) in [(RefKind.tag, "Tags"), (.stash, "Stashes"), (.submodule, "Submodules")] {
            let nodes = items(kind).map { Node(title: $0.name, item: $0) }
            if !nodes.isEmpty { groups.append(Node(title: title, children: nodes)) }
        }

        self.groups = [workspace] + groups
        outline.reloadData()
        outline.expandItem(workspace)
        for group in groups {
            let open = expanded.contains(group.title) || (first && group.title == "Branches")
                || (group.title == "Pull Requests" && pullRequestsFirstShown)
            if open { outline.expandItem(group) }
        }
        if !pullRequests.isEmpty { pullRequestsFirstShown = false }
        // 作り直す前に選んでいた Working Copy・History を選び直す。
        if let (_, page) = selectedTitle, let page { select(page: page) }
    }

    /// プルリクエストの見出しを初めて出すときは開いておく。
    private var pullRequestsFirstShown = true

    // MARK: - NSOutlineViewDataSource

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        (item as? Node)?.children.count ?? groups.count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        (item as? Node)?.children[index] ?? groups[index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        !((item as? Node)?.children.isEmpty ?? true)
    }

    // MARK: - NSOutlineViewDelegate

    func outlineView(_ outlineView: NSOutlineView, isGroupItem item: Any) -> Bool {
        groups.contains { $0 === item as? Node }
    }

    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool {
        (item as? Node).map { $0.item != nil || $0.page != nil || $0.pullRequest != nil } ?? false
    }

    /// 見出しの下の項目の数（リモートはブランチの数）。
    private static func leafCount(_ node: Node) -> Int {
        node.children.isEmpty ? 1 : node.children.reduce(0) { $0 + leafCount($1) }
    }

    @objc private func outlineClicked() {
        let row = outline.clickedRow
        guard row >= 0, let node = outline.item(atRow: row) as? Node, groups.contains(where: { $0 === node }),
              !node.children.isEmpty else { return }
        if outline.isItemExpanded(node) { outline.animator().collapseItem(node) } else { outline.animator().expandItem(node) }
    }

    /// Working Copy の変更の数を表示する。
    func setWorkingCopyCount(_ count: Int) {
        workingCopyNode.badge = count > 0 ? "\(count)" : nil
        outline.reloadItem(workingCopyNode)
    }

    func select(page: Page) {
        let node = page == .workingCopy ? workingCopyNode : historyNode
        if groups.isEmpty {
            groups = [workspace]
            outline.reloadData()
            outline.expandItem(workspace)
        }
        outline.selectRowIndexes([outline.row(forItem: node)], byExtendingSelection: false)
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? Node else { return nil }
        let isGroup = groups.contains { $0 === node }
        if isGroup {
            let cell = outlineView.makeView(withIdentifier: .init("group"), owner: nil) as? NSTableCellView
                ?? SidebarCell(identifier: .init("group"), hasIcon: false)
            // 閉じていても中身があると分かるよう、数を添える（Workspace は除く）。
            let count = node === workspace ? 0 : Self.leafCount(node)
            if count > 0, let field = cell.textField {
                let text = NSMutableAttributedString(string: node.title)
                text.append(NSAttributedString(string: "  \(count)", attributes: [
                    .foregroundColor: NSColor.tertiaryLabelColor,
                    .font: field.font ?? NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
                ]))
                field.attributedStringValue = text
            } else {
                cell.textField?.stringValue = node.title
            }
            return cell
        }
        let cell = outlineView.makeView(withIdentifier: .init("item"), owner: nil) as? SidebarCell
            ?? SidebarCell(identifier: .init("item"), hasIcon: true)
        cell.textField?.stringValue = node.title
        cell.textField?.font = node.isHead ? .boldSystemFont(ofSize: NSFont.systemFontSize) : .systemFont(ofSize: NSFont.systemFontSize)
        cell.imageView?.image = NSImage(systemSymbolName: Self.symbol(for: node), accessibilityDescription: nil)
        cell.imageView?.contentTintColor = node.isHead ? .controlAccentColor : .secondaryLabelColor
        var badge = node.badge ?? ""
        if let item = node.item, item.kind == .local {
            if item.gone {
                badge = "gone"
            } else {
                badge = [item.ahead > 0 ? "↑\(item.ahead)" : nil, item.behind > 0 ? "↓\(item.behind)" : nil]
                    .compactMap { $0 }.joined(separator: " ")
            }
        }
        cell.badge.stringValue = badge
        cell.badge.textColor = .secondaryLabelColor
        cell.toolTip = node.item?.detail
        if let pr = node.pullRequest {
            let ci = switch pr.ci {
            case .passing: L("CI: passed")
            case .failing: L("CI: failed")
            case .pending: L("CI: running")
            case .none: L("No CI")
            }
            let review = switch pr.reviewDecision {
            case "APPROVED": L("Approved")
            case "CHANGES_REQUESTED": L("Changes requested")
            case "REVIEW_REQUIRED": L("Review required")
            default: ""
            }
            cell.toolTip = ["#\(pr.number) \(pr.title)", "\(pr.author.login) · \(pr.headRefName) → \(pr.baseRefName)",
                            [ci, review].filter { !$0.isEmpty }.joined(separator: " · ")].joined(separator: "\n")
            cell.badge.textColor = pr.ci == .failing ? .systemRed : pr.ci == .passing ? .systemGreen : .secondaryLabelColor
        }
        return cell
    }

    private static func symbol(for node: Node) -> String {
        if node.title == L("Conflicts"), node.page != nil { return "exclamationmark.triangle" }
        if let page = node.page { return page == .workingCopy ? "square.and.pencil" : "clock" }
        if node.pullRequest != nil { return "arrow.triangle.pull" }
        if node.remote != nil { return "network" }
        switch node.item?.kind {
        case .local?, .remote?: return "arrow.triangle.branch"
        case .tag?: return "tag"
        case .stash?: return "tray"
        case .submodule?: return "shippingbox"
        case nil: return "folder"
        }
    }

    // MARK: - ドラッグ＆ドロップ: merge・rebase・push

    func outlineView(_ outlineView: NSOutlineView, pasteboardWriterForItem item: Any) -> (any NSPasteboardWriting)? {
        guard let node = item as? Node, let ref = node.item, [.local, .remote, .tag].contains(ref.kind) else { return nil }
        let p = NSPasteboardItem()
        // 種類も渡す（ローカルブランチの名前にも / は入りうる）。
        p.setString("\(ref.kind == .local ? "local" : "other"):\(ref.name)", forType: .branchName)
        return p
    }

    func outlineView(
        _ outlineView: NSOutlineView, validateDrop info: any NSDraggingInfo, proposedItem item: Any?,
        proposedChildIndex index: Int
    ) -> NSDragOperation {
        guard let (isLocal, dragged) = draggedBranch(info), let node = item as? Node else { return [] }
        // ブランチの上に落とす（間に落とすのではない）。
        outlineView.setDropItem(node, dropChildIndex: NSOutlineViewDropOnItemIndex)
        if let ref = node.item, ref.kind == .local, ref.name != dragged { return .move }
        // push できるのはローカルブランチだけ。
        if isLocal, node.remote != nil || node.item?.kind == .remote { return .copy }
        return []
    }

    func outlineView(
        _ outlineView: NSOutlineView, acceptDrop info: any NSDraggingInfo, item: Any?, childIndex index: Int
    ) -> Bool {
        guard let actions, let (isLocal, dragged) = draggedBranch(info), let node = item as? Node else { return false }
        let menu = NSMenu()
        func add(_ title: String, _ action: @escaping () -> Void) {
            menu.addItem(ClosureMenuItem(title: title, action: action))
        }
        if let ref = node.item, ref.kind == .local {
            let target = ref.name
            // 落とした先が今のブランチでなければ、先にチェックアウトする。
            let checkout: [[String: Any]] = target == headBranch ? [] : [["op": "checkout", "branch": target]]
            // checkout していなくても、fast-forward で済むならブランチだけ進める。
            add("Merge \(dragged) into \(target)") {
                actions.perform(["op": "mergeInto", "target": target, "rev": dragged])
            }
            add("Rebase \(target) onto \(dragged)") {
                actions.perform(sequence: checkout + [["op": "rebase", "onto": dragged]])
            }
        } else if isLocal, let remote = node.remote ?? node.item.map({ String($0.name.split(separator: "/")[0]) }) {
            let remoteBranch = node.item.map { String($0.name.dropFirst(remote.count + 1)) } ?? dragged
            add("Push \(dragged) to \(remote)/\(remoteBranch)") {
                actions.perform(["op": "push", "remote": remote, "branch": dragged, "remoteBranch": remoteBranch,
                                 "force": false, "setUpstream": node.item == nil])
            }
            add("Force Push \(dragged) to \(remote)/\(remoteBranch)") {
                actions.perform(["op": "push", "remote": remote, "branch": dragged, "remoteBranch": remoteBranch,
                                 "force": true, "setUpstream": false])
            }
        }
        guard !menu.items.isEmpty else { return false }
        menu.addItem(.separator())
        menu.addItem(withTitle: L("Cancel"), action: nil, keyEquivalent: "")
        menu.popUp(positioning: nil, at: outlineView.convert(info.draggingLocation, from: nil), in: outlineView)
        return true
    }

    /// ドラッグしているブランチ（ローカルか, 名前）。
    private func draggedBranch(_ info: any NSDraggingInfo) -> (Bool, String)? {
        guard let value = info.draggingPasteboard.string(forType: .branchName),
              let colon = value.firstIndex(of: ":") else { return nil }
        return (value[..<colon] == "local", String(value[value.index(after: colon)...]))
    }

    // MARK: - 右クリックメニュー

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        if let pr = (outline.item(atRow: outline.clickedRow) as? Node)?.pullRequest {
            menu.addItem(ClosureMenuItem(title: "Checkout #\(pr.number)") { [weak self] in self?.onCheckoutPullRequest?(pr) })
            menu.addItem(ClosureMenuItem(title: L("Open in Browser")) {
                if let url = URL(string: pr.url) { NSWorkspace.shared.open(url) }
            })
            menu.addItem(ClosureMenuItem(title: L("Copy URL")) {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(pr.url, forType: .string)
            })
            return
        }
        guard let actions, let node = outline.item(atRow: outline.clickedRow) as? Node, let item = node.item else { return }
        let name = item.name
        func add(_ title: String, _ action: @escaping () -> Void) {
            menu.addItem(ClosureMenuItem(title: title, action: action))
        }
        let current = headBranch ?? "HEAD"
        switch item.kind {
        case .local:
            if name != headBranch {
                add("Checkout \(name)") { actions.perform(["op": "checkout", "branch": name]) }
                add("Merge \(name) into \(current)") { actions.perform(["op": "merge", "rev": name]) }
                add("Rebase \(current) onto \(name)") { actions.perform(["op": "rebase", "onto": name]) }
                menu.addItem(.separator())
            }
            add("New Branch from \(name)…") { actions.newBranch(at: name) }
            add("New Tag at \(name)…") { actions.newTag(at: name) }
            add(L("Rename %@…", name)) {
                guard let to = actions.ask(L("Rename Branch"), initial: name, ok: L("Change")) else { return }
                actions.perform(["op": "renameBranch", "from": name, "to": to])
            }
            if name != headBranch {
                add(L("Delete %@", name)) { actions.perform(["op": "deleteBranch", "name": name, "force": false]) }
            }
        case .remote:
            add("Checkout \(name) as Local Branch") { actions.perform(["op": "track", "remoteBranch": name]) }
            add("Merge \(name) into \(current)") { actions.perform(["op": "merge", "rev": name]) }
            add("Rebase \(current) onto \(name)") { actions.perform(["op": "rebase", "onto": name]) }
            menu.addItem(.separator())
            add("New Branch from \(name)…") { actions.newBranch(at: name) }
        case .tag:
            add("Merge \(name) into \(current)") { actions.perform(["op": "merge", "rev": name]) }
            add("New Branch from \(name)…") { actions.newBranch(at: name) }
        case .stash:
            add("Apply \(name)") { actions.perform(["op": "stashApply", "name": name, "pop": false]) }
            add("Pop \(name)") { actions.perform(["op": "stashApply", "name": name, "pop": true]) }
            add("Drop \(name)") { actions.perform(["op": "stashDrop", "name": name]) }
        case .submodule:
            add("Update \(name)") { actions.perform(["op": "updateSubmodules", "paths": [name]]) }
            add(L("Open %@ in New Window", name)) { [weak self] in self?.onOpenSubmodule?(name) }
        }
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard let node = outline.item(atRow: outline.selectedRow) as? Node else { return }
        if node === conflictsNode {
            onSelectPage?(.workingCopy)
            onSelectConflicts?()
        } else if let page = node.page {
            onSelectPage?(page)
        } else if let item = node.item {
            onSelectPage?(.history)
            onSelect?(item)
        } else if let pr = node.pullRequest,
                  let branch = refs?.items.first(where: { $0.kind == .remote && $0.name.hasSuffix("/" + pr.headRefName) })
                    ?? refs?.items.first(where: { $0.kind == .local && $0.name == pr.headRefName }) {
            // プルリクエストのブランチが手元にあれば、その履歴に絞り込む。
            onSelectPage?(.history)
            onSelect?(branch)
        }
    }
}

/// 選んだら閉包を呼ぶメニュー項目を作る。
/// サイドバーの行: アイコン、名前、右に薄い数（ahead・behind や変更の数）。
final class SidebarCell: NSTableCellView {
    let badge = NSTextField(labelWithString: "")

    init(identifier: NSUserInterfaceItemIdentifier, hasIcon: Bool) {
        super.init(frame: .zero)
        self.identifier = identifier
        let field = NSTextField(labelWithString: "")
        field.lineBreakMode = .byTruncatingTail
        field.translatesAutoresizingMaskIntoConstraints = false
        badge.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        badge.textColor = .secondaryLabelColor
        badge.translatesAutoresizingMaskIntoConstraints = false
        badge.setContentCompressionResistancePriority(.required, for: .horizontal)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        addSubview(field)
        addSubview(badge)
        textField = field
        var leading = leadingAnchor
        if hasIcon {
            let icon = NSImageView()
            icon.translatesAutoresizingMaskIntoConstraints = false
            icon.symbolConfiguration = .init(pointSize: 12, weight: .regular)
            addSubview(icon)
            imageView = icon
            NSLayoutConstraint.activate([
                icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
                icon.centerYAnchor.constraint(equalTo: centerYAnchor),
                icon.widthAnchor.constraint(equalToConstant: 18),
            ])
            leading = icon.trailingAnchor
        }
        NSLayoutConstraint.activate([
            field.leadingAnchor.constraint(equalTo: leading, constant: hasIcon ? 4 : 2),
            field.centerYAnchor.constraint(equalTo: centerYAnchor),
            badge.leadingAnchor.constraint(greaterThanOrEqualTo: field.trailingAnchor, constant: 6),
            badge.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            badge.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }
}

func ClosureMenuItem(title: String, action handler: @escaping () -> Void) -> NSMenuItem {
    let target = MenuAction(handler)
    let item = NSMenuItem(title: title, action: #selector(MenuAction.fire), keyEquivalent: "")
    item.target = target
    // target は弱参照なので、項目に持たせて生かしておく。
    item.representedObject = target
    return item
}

final class MenuAction: NSObject {
    private let handler: () -> Void

    init(_ handler: @escaping () -> Void) {
        self.handler = handler
    }

    @objc func fire() {
        handler()
    }
}

extension NSPasteboard.PasteboardType {
    static let branchName = NSPasteboard.PasteboardType("io.github.fukusaka.amonhen.branch")
}
