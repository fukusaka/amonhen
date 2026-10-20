import AmonhenKit
import AppKit

/// サイドバー: Branches / Pull Requests / Remotes / Tags / Stashes / Submodules。
final class SidebarViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate, NSMenuDelegate,
    NSPopoverDelegate
{
    /// 項目を選んだとき（指すコミットがあれば History でその行を選ぶ）。
    var onSelect: ((RefItem) -> Void)?
    /// ブランチ・タグを複数選んだとき（それぞれから辿れる履歴をまとめて出す）。
    var onSelectMany: (([RefItem]) -> Void)?
    var actions: Actions?
    /// チェックアウト中のブランチ。
    private var headBranch: String?
    /// サブモジュールを新しいウィンドウで開く（作業ツリーからのパス）。
    var onOpenSubmodule: ((String) -> Void)?
    /// Working Copy か History を選んだとき。
    var onSelectPage: ((Page) -> Void)?
    /// プルリクエストの右クリック: チェックアウト（gh pr checkout）。
    var onCheckoutPullRequest: ((PullRequest) -> Void)?
    /// プルリクエストの行の右端の CI の印を押した（詳細の吹き出しを、その印の下に）。
    var onClickPullRequestCI: ((PullRequest, NSView) -> Void)?

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
        /// Working Copy の下の worktree。
        var worktree: WorktreeInfo?
        /// 変更のない worktree をまとめる行。
        var isCleanWorktrees = false
        /// Branches の下の分け方のフォルダ（`inactive`・`archived`）。開いているかを覚えるのに使う。
        var branchGroup: String?
        /// ほかのブランチの上に積んだブランチなら、その土台のブランチ（行は土台の下に入れ子で、↳ の印）。
        var stackedOn: String?
        /// お気に入り（Branches の一番上に、決めた順で出す）。
        var isPinned = false

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

    private let outline = SidebarOutlineView()
    private var groups: [Node] = []
    private var generation: UInt64?
    /// ブランチ名の絞り込みの欄（サイドバーの上）。
    private let filterField = NSSearchField()
    /// 絞り込みの語（空白で区切る。小文字。名前がすべてを含む行だけ出す）。空なら絞り込まない。
    private var filterWords: [String] = []

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
        // ブランチ・タグは複数選べる（まとめて履歴を見る、まとめて消す・アーカイブする）。
        outline.allowsMultipleSelection = true
        let menu = NSMenu()
        menu.delegate = self
        outline.menu = menu
        outline.registerForDraggedTypes([.branchName])
        outline.setDraggingSourceOperationMask([.move, .copy], forLocal: true)

        let scroll = NSScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        // 絞り込みの欄は表の上に。打つたびに絞り込む（Esc で消す）。
        filterField.placeholderString = L("Filter branches")
        filterField.controlSize = .small
        filterField.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        filterField.sendsSearchStringImmediately = true
        filterField.sendsWholeSearchString = false
        filterField.target = self
        filterField.action = #selector(filterChanged)
        filterField.translatesAutoresizingMaskIntoConstraints = false
        let container = NSView()
        container.addSubview(filterField)
        container.addSubview(scroll)
        NSLayoutConstraint.activate([
            filterField.topAnchor.constraint(equalTo: container.safeAreaLayoutGuide.topAnchor, constant: 6),
            filterField.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 10),
            filterField.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -10),
            scroll.topAnchor.constraint(equalTo: filterField.bottomAnchor, constant: 4),
            scroll.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])
        view = container
    }

    @objc private func filterChanged() {
        let words = filterField.stringValue.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard words != filterWords else { return }
        filterWords = words
        rebuild(first: false)
    }

    /// Navigate > Filter Branches…: 絞り込みの欄にカーソルを移す。
    func focusFilter() {
        view.window?.makeFirstResponder(filterField)
    }

    private func matchesFilter(_ name: String) -> Bool {
        let lower = name.lowercased()
        return filterWords.allSatisfy { lower.contains($0) }
    }

    private var refs: Refs?
    /// ローカルブランチの分け方（まだ求めていなければ nil。そのときは名前のフォルダだけ）。
    private var branchGroups: BranchGroups?
    /// 開いている分け方のフォルダ（Merged・Inactive・Archived。初めはすべて畳む）。
    private var openBranchGroups: Set<String> = []
    /// お気に入りのブランチ（上から順）。ドラッグで並べ替える。
    private var pinnedOrder: [String] = []

    /// 分け方のフォルダの説明（ツールチップ）。
    private static func branchGroupHelp(_ key: String) -> String? {
        switch key {
        case "inactive": L("Branches with no new commit for 90 days. Folded.")
        case "archived": L("Branches you archived from their menu. Folded. Archiving is kept in git’s config, so it follows a rename.")
        default: nil
        }
    }

    /// ブランチの名前 → 分け方（行を描くときに引く）。
    private var branchFacts: [String: BranchGroups.Branch] = [:]

    /// ローカルブランチの分け方が変わった。
    func show(branchGroups: BranchGroups) {
        guard branchGroups != self.branchGroups else { return }
        self.branchGroups = branchGroups
        branchFacts = Dictionary(branchGroups.branches.map { ($0.name, $0) }, uniquingKeysWith: { a, _ in a })
        rebuild(first: false)
    }
    private var pullRequests: [PullRequest] = []
    private var worktrees: [WorktreeInfo] = []
    /// 書き込む先の worktree のパス。
    private var editingWorktree: String?
    private var editingInfo: WorktreeInfo?
    /// ほかの worktree が checkout しているブランチ。
    private var branchesElsewhere: [String: WorktreeInfo] = [:]
    private var worktreeChanges: [String: Int] = [:]
    /// worktree を選んだ（書き込む先にする）。
    var onSelectWorktree: ((String) -> Void)?
    enum WorktreeAction { case reveal, terminal, remove, prune }
    var onWorktreeAction: ((String, WorktreeAction) -> Void)?

    /// リポジトリの worktree（2 つ以上あるときだけ Worktrees に出す）。
    func show(worktrees: [WorktreeInfo], editing: String) {
        let changed = worktrees != self.worktrees || editing != editingWorktree
        self.worktrees = worktrees
        editingWorktree = editing
        // 行を描くたびに探さないよう、ブランチ → それを checkout しているほかの worktree を先に作っておく。
        let editingKey = WorktreeInfo.canonical(editing)
        editingInfo = worktrees.count > 1 ? worktrees.first { WorktreeInfo.canonical($0.path) == editingKey } : nil
        branchesElsewhere = worktrees.count > 1 ? Dictionary(
            worktrees.filter { WorktreeInfo.canonical($0.path) != editingKey }.compactMap { w in w.branch.map { ($0, w) } },
            uniquingKeysWith: { a, _ in a }) : [:]
        if changed { rebuild(first: false) }
    }

    /// `branch` を checkout している、書き込む先以外の worktree。
    private func otherWorktree(using branch: String) -> WorktreeInfo? {
        branchesElsewhere[branch]
    }

    /// worktree ごとの変更の数。
    func setWorktreeChanges(_ changes: [String: Int]) {
        // パスの書き方の違い（/tmp と /private/tmp など）をなくして引けるよう、リンクを解いた形で持つ。
        let changes = Dictionary(changes.map { (WorktreeInfo.canonical($0.key), $0.value) }, uniquingKeysWith: { a, _ in a })
        guard changes != worktreeChanges else { return }
        worktreeChanges = changes
        // 変更のある・ないが入れ替わったら並べ直す。数だけなら行を描き直す。
        if !arrangeWorktrees() {
            for row in 0..<outline.numberOfRows {
                if let node = outline.item(atRow: row) as? Node, node.worktree != nil { outline.reloadItem(node) }
            }
        }
    }

    /// 変更のない worktree をまとめる行（「変更なし N」。初めは畳んでおく）。
    private let cleanWorktreesNode: Node = {
        let node = Node(title: L("No changes"))
        node.isCleanWorktrees = true
        return node
    }()

    /// Working Copy の下に並べる worktree: 書き込む先、変更のあるもの、フォルダが消えた・ロックしたもの（メインの
    /// 作業ツリーを先に、残りは名前の順。w2 は w10 より前）と、「変更なし」にまとめるもの（2 つ以上あるときだけ
    /// まとめる）。worktree が 1 つなら出さない。
    private func worktreeLayout() -> (shown: [WorktreeInfo], clean: [WorktreeInfo]) {
        guard worktrees.count > 1 else { return ([], []) }
        let ordered = worktrees.sorted {
            $0.main != $1.main ? $0.main : $0.name.localizedStandardCompare($1.name) == .orderedAscending
        }
        var shown: [WorktreeInfo] = [], clean: [WorktreeInfo] = []
        for w in ordered {
            let changes = worktreeChanges[WorktreeInfo.canonical(w.path)] ?? 0
            if w == editingInfo || changes > 0 || w.prunable != nil || w.locked != nil { shown.append(w) } else { clean.append(w) }
        }
        return clean.count < 2 ? (ordered, []) : (shown, clean)
    }

    private func setWorktreeNodes(_ layout: (shown: [WorktreeInfo], clean: [WorktreeInfo])) {
        func node(_ w: WorktreeInfo) -> Node {
            let node = Node(title: w.name, isHead: w == editingInfo)
            node.worktree = w
            return node
        }
        cleanWorktreesNode.children = layout.clean.map(node)
        workingCopyNode.children = layout.shown.map(node) + (layout.clean.isEmpty ? [] : [cleanWorktreesNode])
    }

    /// 変更の数が変わったときに並べ直す（変更のある・ないが入れ替わったときだけ）。並べ直したら `true`。
    private func arrangeWorktrees() -> Bool {
        let layout = worktreeLayout()
        let current = workingCopyNode.children.compactMap { $0.worktree?.path }
        guard layout.shown.map(\.path) != current || layout.clean.map(\.path) != cleanWorktreesNode.children.compactMap({ $0.worktree?.path })
        else { return false }
        let cleanOpen = outline.isItemExpanded(cleanWorktreesNode)
        let selected = (outline.item(atRow: outline.selectedRow) as? Node)?.worktree?.path
        setWorktreeNodes(layout)
        outline.reloadItem(workingCopyNode, reloadChildren: true)
        outline.expandItem(workingCopyNode)
        if cleanOpen { outline.expandItem(cleanWorktreesNode) }
        if let selected, let row = (0..<outline.numberOfRows).first(where: {
            (outline.item(atRow: $0) as? Node)?.worktree?.path == selected
        }) {
            outline.selectRowIndexes([row], byExtendingSelection: false)
        }
        return true
    }

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
        // 今のブランチは、書き込む先の worktree のもの（worktree が 1 つなら開いたリポジトリの HEAD）。
        headBranch = editingInfo.map(\.branch) ?? refs.headBranch
        let head = headBranch
        let expanded = Set(groups.filter { outline.isItemExpanded($0) }.map(\.title))
        let selectedTitle = (outline.item(atRow: outline.selectedRow) as? Node).map { ($0.title, $0.page) }

        func items(_ kind: RefKind) -> [RefItem] { refs.items.filter { $0.kind == kind } }
        // 絞り込み中は、合うブランチ・タグを名前のまま平らに（フォルダや積み重ねにはしない）、見出しは開いて出す。
        if !filterWords.isEmpty {
            var groups: [Node] = []
            for (kind, title) in [(RefKind.local, "Branches"), (.remote, "Remotes"), (.tag, "Tags")] {
                let matched = items(kind).filter { matchesFilter($0.name) }.sorted { $0.name < $1.name }
                guard !matched.isEmpty else { continue }
                groups.append(Node(title: title, children: matched.map {
                    Node(title: $0.name, item: $0, isHead: $0.kind == .local && $0.name == head)
                }))
            }
            setWorktreeNodes(worktreeLayout())
            self.groups = [workspace] + groups
            outline.reloadData()
            outline.expandItem(workspace)
            outline.expandItem(workingCopyNode)
            groups.forEach { outline.expandItem($0) }
            if let (_, page) = selectedTitle, let page { select(page: page) }
            return
        }
        var groups: [Node] = []
        // feature/x・backup/x のような名前はフォルダにまとめる（ブランチとタグ）。
        func folders(_ title: String, _ list: [RefItem]) -> Node {
            let root = Node(title: title)
            for item in list {
                var parent = root
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
                parent.children.append(Node(title: parts.last ?? item.name, item: item,
                                            isHead: item.kind == .local && item.name == head))
            }
            return root
        }
        let branchRoot = branchesNode(items(.local), head: head, folders: folders)
        if !branchRoot.children.isEmpty { groups.append(branchRoot) }

        // worktree は Working Copy の下に並べる（1 つだけなら出さない）。
        let cleanOpen = outline.isItemExpanded(cleanWorktreesNode)
        setWorktreeNodes(worktreeLayout())

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
        let tagRoot = folders("Tags", items(.tag))
        if !tagRoot.children.isEmpty { groups.append(tagRoot) }
        for (kind, title) in [(RefKind.stash, "Stashes"), (.submodule, "Submodules")] {
            let nodes = items(kind).map { Node(title: $0.name, item: $0) }
            if !nodes.isEmpty { groups.append(Node(title: title, children: nodes)) }
        }

        self.groups = [workspace] + groups
        outline.reloadData()
        outline.expandItem(workspace)
        outline.expandItem(workingCopyNode)
        if cleanOpen { outline.expandItem(cleanWorktreesNode) }
        for group in groups {
            let open = expanded.contains(group.title) || (first && group.title == "Branches")
                || (group.title == "Pull Requests" && pullRequestsFirstShown)
            if open { outline.expandItem(group) }
        }
        if !pullRequests.isEmpty { pullRequestsFirstShown = false }
        expandBranchGroups()
        // 作り直す前に選んでいた Working Copy・History を選び直す。
        if let (_, page) = selectedTitle, let page { select(page: page) }
    }

    /// Inactive とみなす日数（最後のコミットからこれだけ経ったブランチ）。
    private static let staleDays: Int64 = 90

    /// Branches の中身。分け方が分かっていれば、上から順に:
    /// - お気に入り（★。決めた順。ドラッグで並べ替える。積んだブランチでも入れ子にせず、ここに平らに出す）
    /// - 今のブランチ（その積み重ねの根元ごと）と既定のブランチ
    /// - 動いているブランチ（新しい順。`/` の名前はフォルダ。既定のブランチに取り込み済みなら、gone と同じく
    ///   行の右端に merged の印）
    /// - Inactive（最後のコミットから 90 日以上。畳んでおく）
    /// - Archived（手でアーカイブしたもの。畳んでおく）
    /// ほかのブランチの上に積んだブランチは、その土台のブランチの行の下に入れ子で出す（↳ の印）。見出しは
    /// Branches・Remotes と同じく英語のまま。ほかの worktree が checkout しているブランチは、Merged・Inactive に入れない。
    private func branchesNode(_ locals: [RefItem], head: String?, folders: (String, [RefItem]) -> Node) -> Node {
        guard let groups = branchGroups else { return folders("Branches", locals) }
        let facts = Dictionary(groups.branches.map { ($0.name, $0) }, uniquingKeysWith: { a, _ in a })
        let baseName = groups.base.map { name in
            locals.contains { $0.name == name } ? name : String(name.split(separator: "/", maxSplits: 1).last ?? "")
        }
        let now = Int64(Date().timeIntervalSince1970)
        func newest(_ a: RefItem, _ b: RefItem) -> Bool { (facts[a.name]?.time ?? 0) > (facts[b.name]?.time ?? 0) }
        func isArchived(_ item: RefItem) -> Bool { facts[item.name]?.archived == true }
        func isPinned(_ item: RefItem) -> Bool { facts[item.name]?.pinned != nil }
        // 今のブランチがほかのブランチの上に積んであれば、その積み重ねの根元ごと一番上に出す（積み重ねを切らない）。
        var stackRoot = head
        var seen: Set<String> = []
        while let name = stackRoot, let parent = facts[name]?.parent, !seen.contains(parent),
              locals.contains(where: { $0.name == parent && !isArchived($0) }) {
            seen.insert(name)
            stackRoot = parent
        }
        // 根元がお気に入りなら、そのお気に入りの下に入れ子で出す（お気に入りはもう一番上にある）。
        func isTop(_ item: RefItem) -> Bool { (item.name == stackRoot && !isPinned(item)) || item.name == baseName }
        // 土台のブランチ → その上に積んだブランチ（土台がアーカイブ済みなら、積んだほうは自分の場所に出す。
        // お気に入りは入れ子にしない）。
        let byName = Dictionary(locals.map { ($0.name, $0) }, uniquingKeysWith: { a, _ in a })
        var stacked: [String: [RefItem]] = [:]
        var nested: Set<String> = []
        for item in locals where !isTop(item) && !isArchived(item) && !isPinned(item) {
            guard let parent = facts[item.name]?.parent, let under = byName[parent], !isArchived(under) else { continue }
            stacked[parent, default: []].append(item)
            nested.insert(item.name)
        }
        // 積んだブランチを、土台の行の下に（さらに積んだものはその下に）付ける。名前のフォルダの中の行は
        // 名前の最後の部分だけだが、積んだブランチの行は土台とは別の名前なので、名前を全部出す。
        func attach(_ node: Node) {
            if let item = node.item {
                node.children = (stacked[item.name] ?? []).sorted(by: newest).map { tree($0, on: item.name) }
            } else {
                node.children.forEach(attach)
            }
        }
        func tree(_ item: RefItem, on parent: String? = nil) -> Node {
            let node = Node(title: item.name, item: item, isHead: item.name == head)
            node.stackedOn = parent
            attach(node)
            return node
        }
        var pinned: [RefItem] = [], top: [RefItem] = [], active: [RefItem] = []
        var inactive: [RefItem] = [], archived: [RefItem] = []
        for item in locals where !nested.contains(item.name) {
            let f = facts[item.name]
            if isPinned(item) {
                pinned.append(item)
            } else if isTop(item) {
                top.append(item)
            } else if isArchived(item) {
                archived.append(item)
            } else if let f, otherWorktree(using: item.name) == nil,
                      f.time.map({ now - $0 > Self.staleDays * 86_400 }) ?? false {
                inactive.append(item)
            } else {
                active.append(item)
            }
        }
        func group(_ key: String, _ title: String, _ list: [RefItem]) -> Node? {
            guard !list.isEmpty else { return nil }
            let node = Node(title: title, children: list.map { tree($0) })
            node.branchGroup = key
            node.badge = "\(list.count)"
            return node
        }
        // お気に入りは決めた順（順位が同じなら名前順）。積んだブランチでも平らに出し、土台は行の右に書く。
        pinned.sort { (facts[$0.name]?.pinned ?? 0, $0.name) < (facts[$1.name]?.pinned ?? 0, $1.name) }
        pinnedOrder = pinned.map(\.name)
        let stars: [Node] = pinned.map { item in
            let node = Node(title: item.name, item: item, isHead: item.name == head)
            node.isPinned = true
            node.stackedOn = facts[item.name]?.parent.flatMap { byName[$0] }.map(\.name)
            // お気に入りの上に積んだブランチは、お気に入りの下に入れ子で出す。
            attach(node)
            return node
        }
        // 動いているブランチは新しい順。`/` のフォルダは、中でいちばん新しいブランチの位置に出る。
        let named = folders("", active.sorted(by: newest)).children
        named.forEach(attach)
        let root = Node(title: "Branches")
        var children: [Node] = stars + top.sorted { a, _ in a.name == stackRoot }.map { tree($0) } + named
        if let node = group("inactive", "Inactive", inactive.sorted(by: newest)) { children.append(node) }
        if let node = group("archived", "Archived", archived.sorted { $0.name < $1.name }) { children.append(node) }
        root.children = children
        return root
    }

    private var statusPopover: NSPopover?
    /// 吹き出しを出していたブランチと、閉じた時刻。同じアイコンをもう一度押したら、閉じるだけにする（押した
    /// 瞬間に transient の吹き出しが閉じ、すぐ出し直すと、閉じられないように見える）。
    private var statusClosed: (name: String, at: CFTimeInterval)?

    /// 行の右端の印（取り込み済み・gone・ほかの worktree・↑↓・✎）の詳細の吹き出し（印の下に出す）。
    private func showBranchStatus(
        _ item: RefItem, isHead: Bool, otherWorktree: WorktreeInfo? = nil, editing: Bool = false, from view: NSView
    ) {
        // 出ている間にもう一度押した（transient の吹き出しが閉じるのは、この後）: 閉じるだけ。
        if let shown = statusPopover, shown.isShown, statusShown == item.name {
            shown.close()
            return
        }
        // 押した瞬間に閉じた直後なら、出し直さない。
        if let closed = statusClosed, closed.name == item.name, CACurrentMediaTime() - closed.at < 0.3 {
            statusClosed = nil
            return
        }
        var facts = BranchStatusPopover.Facts(item: item, branch: branchFacts[item.name], base: branchGroups?.base, isHead: isHead)
        facts.otherWorktree = otherWorktree
        facts.editing = editing
        let popover = NSPopover()
        popover.behavior = .transient
        popover.delegate = self
        popover.contentViewController = BranchStatusPopover(
            facts: facts,
            onDelete: { [weak self] in self?.actions?.perform(["op": "deleteBranch", "name": item.name, "force": false]) },
            onArchive: { [weak self] in
                self?.actions?.perform(["op": "configSet", "key": "branch.\(item.name).amonhenArchived", "value": "true"])
            },
            onOpenWorktree: { [weak self] path in self?.onSelectWorktree?(path) })
        popover.show(relativeTo: view.bounds, of: view, preferredEdge: .maxY)
        statusPopover = popover
        statusShown = item.name
    }

    private var statusShown: String?

    func popoverDidClose(_ notification: Notification) {
        guard notification.object as? NSPopover === statusPopover else { return }
        if let name = statusShown { statusClosed = (name, CACurrentMediaTime()) }
        statusShown = nil
        statusPopover = nil
    }

    /// お気に入りの順を書く（`branch.<name>.amonhenPinned` に 1 から順に）。
    private func savePinnedOrder(_ names: [String]) {
        guard let actions else { return }
        actions.perform(sequence: names.enumerated().map { i, name in
            ["op": "configSet", "key": "branch.\(name).amonhenPinned", "value": "\(i + 1)"]
        })
    }

    /// 畳んだ、積んだブランチのあるブランチ（初めはすべて開いておく）。
    private var foldedStacks: Set<String> = []

    /// Branches の下の分け方のフォルダを、前と同じように開く。
    private func expandBranchGroups() {
        // 積んだブランチのある行は、畳んだもののほかは開く（分け方のフォルダの中も）。
        func expandStacks(_ node: Node) {
            for child in node.children {
                if let item = child.item, !child.children.isEmpty, !foldedStacks.contains(item.name) {
                    outline.expandItem(child)
                }
                expandStacks(child)
            }
        }
        for group in groups where group.title == "Branches" {
            for node in group.children where node.branchGroup.map(openBranchGroups.contains) == true {
                outline.expandItem(node)
            }
            expandStacks(group)
        }
    }

    func outlineViewItemDidExpand(_ notification: Notification) {
        guard let node = notification.userInfo?["NSObject"] as? Node else { return }
        if let key = node.branchGroup { openBranchGroups.insert(key) }
        if let item = node.item, item.kind == .local { foldedStacks.remove(item.name) }
    }

    func outlineViewItemDidCollapse(_ notification: Notification) {
        guard let node = notification.userInfo?["NSObject"] as? Node else { return }
        if let key = node.branchGroup { openBranchGroups.remove(key) }
        if let item = node.item, item.kind == .local { foldedStacks.insert(item.name) }
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
        (item as? Node).map { $0.item != nil || $0.page != nil || $0.pullRequest != nil || $0.worktree != nil } ?? false
    }

    /// 見出しの下の項目の数（リモートはブランチの数）。
    private static func leafCount(_ node: Node) -> Int {
        node.children.isEmpty ? 1 : node.children.reduce(0) { $0 + leafCount($1) }
    }

    @objc private func outlineClicked() {
        let row = outline.clickedRow
        // 見出しと、選べない行（ブランチ・タグのフォルダ、リモート、「変更なし」）は、押せば開く・閉じる。
        guard row >= 0, let node = outline.item(atRow: row) as? Node, !node.children.isEmpty,
              groups.contains(where: { $0 === node }) || !outlineView(outline, shouldSelectItem: node) else { return }
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
        cell.textField?.textColor = node === cleanWorktreesNode ? .secondaryLabelColor : .labelColor
        cell.imageView?.image = NSImage(systemSymbolName: Self.symbol(for: node), accessibilityDescription: nil)
        cell.imageView?.contentTintColor = node.isHead ? .controlAccentColor : .secondaryLabelColor
        var badge = node.badge ?? ""
        /// 右端のアイコン（SF Symbols の名前, ツールチップ）。
        var marks: [(String, String)] = []
        if let item = node.item, item.kind == .local, let other = otherWorktree(using: item.name) {
            // ほかの worktree が checkout している（git はここでは checkout させない）。名前は長いことが多いので、
            // 行には印だけを出し、worktree の名前はツールチップに（ブランチ名を隠さない）。メインの作業ツリーは ⌂、
            // 追加した worktree は ⑂（緑）。
            cell.toolTip = L("Checked out in the worktree “%@” (%@)", other.name, other.path)
            cell.badge.stringValue = other.main ? "⌂" : "⑂"
            cell.badge.textColor = other.main ? .secondaryLabelColor : .systemGreen
            if !other.main { cell.imageView?.contentTintColor = .systemGreen }
            cell.onBadgeClick = { [weak self, weak cell] in
                guard let self, let cell else { return }
                showBranchStatus(item, isHead: node.isHead, otherWorktree: other, from: cell.badge)
            }
            return cell
        } else if let item = node.item, item.kind == .local {
            if !item.gone {
                badge = [item.ahead > 0 ? "↑\(item.ahead)" : nil, item.behind > 0 ? "↓\(item.behind)" : nil]
                    .compactMap { $0 }.joined(separator: " ")
            }
            // 取り込み済み（既定のブランチの先端と同じ、作ったばかりのものは除く）と gone は、文字でなくアイコンで
            // （行の右端。意味はツールチップ）。
            let merged = branchFacts[item.name].map { $0.merged && !$0.atBase } ?? false
                && !node.isHead && item.name != branchGroups?.base
            marks = [merged ? ("arrow.triangle.merge", L("Merged into %@", branchGroups?.base ?? "main")) : nil,
                     item.gone ? ("icloud.slash", L("Its upstream %@ no longer exists on the remote", item.detail ?? "")) : nil]
                .compactMap { $0 }
            // worktree が 2 つ以上なら、今のブランチ（書き込む先のもの）に ✎ を添える。
            if node.isHead, editingInfo != nil {
                badge = badge.isEmpty ? "✎" : "✎ " + badge
            }
            // お気に入りに出した積んだブランチは入れ子にならないので、↳ を右に添える（土台はツールチップ）。
            // サイドバーは狭いので、土台の名前は行に出さない（名前が隠れる）。
            if node.isPinned, node.stackedOn != nil {
                badge = badge.isEmpty ? "↳" : "↳ " + badge
            }
        }
        if node === cleanWorktreesNode { badge = "\(node.children.count)" }
        if let w = node.worktree {
            badge = w.prunable != nil ? L("missing") : w.locked != nil ? L("locked")
                : (worktreeChanges[WorktreeInfo.canonical(w.path)]).flatMap { $0 > 0 ? "\($0)" : nil } ?? ""
        }
        cell.badge.textColor = .secondaryLabelColor
        if marks.isEmpty {
            cell.badge.stringValue = badge
        } else {
            // アイコンは文字に添付して並べる（アイコンの後に、数などの文字）。
            let text = NSMutableAttributedString()
            for (symbol, _) in marks {
                let attachment = NSTextAttachment()
                attachment.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)?
                    .withSymbolConfiguration(.init(pointSize: NSFont.smallSystemFontSize, weight: .regular))
                let icon = NSMutableAttributedString(attachment: attachment)
                icon.addAttribute(.foregroundColor, value: NSColor.secondaryLabelColor, range: NSRange(location: 0, length: icon.length))
                text.append(icon)
                text.append(NSAttributedString(string: " "))
            }
            text.append(NSAttributedString(string: badge, attributes: [
                .font: cell.badge.font ?? .systemFont(ofSize: NSFont.smallSystemFontSize),
                .foregroundColor: NSColor.secondaryLabelColor,
            ]))
            cell.badge.attributedStringValue = text
        }
        cell.toolTip = node === cleanWorktreesNode ? L("Worktrees with no changes (click to show)")
            : node.branchGroup.flatMap(Self.branchGroupHelp)
            ?? node.stackedOn.map { L("Built on %@ (its commits are on top of that branch)", $0) }
            ?? node.item?.detail
        // 右端のアイコンの意味は、ツールチップの先頭に（上流の名前はその後ろに残す）。右端に何か出ているローカル
        // ブランチは、押すと詳細の吹き出し。
        cell.onBadgeClick = nil
        if !marks.isEmpty {
            cell.toolTip = (marks.map(\.1) + [cell.toolTip].compactMap { $0 }).joined(separator: "\n")
        }
        if let item = node.item, item.kind == .local, !marks.isEmpty || !badge.isEmpty {
            let editing = node.isHead && editingInfo != nil
            cell.onBadgeClick = { [weak self, weak cell] in
                guard let self, let cell else { return }
                showBranchStatus(item, isHead: node.isHead, editing: editing, from: cell.badge)
            }
        }
        if let w = node.worktree {
            cell.toolTip = [w.path, w.branch.map { L("Branch: %@", $0) } ?? L("detached HEAD"),
                            node.isHead ? L("Editing this worktree") : L("Click to edit this worktree"),
                            w.locked.map { $0.isEmpty ? L("Locked") : L("Locked: %@", $0) },
                            w.prunable.map { L("Its folder is gone (Prune removes it): %@", $0) }]
                .compactMap { $0 }.joined(separator: "\n")
        }
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
            // CI の印は押せる（History の行の印と同じ吹き出し）。
            if pr.ci != .none, pr.headRefOid != nil {
                cell.onBadgeClick = { [weak self, weak cell] in
                    guard let self, let cell else { return }
                    onClickPullRequestCI?(pr, cell.badge)
                }
            }
        }
        return cell
    }

    private static func symbol(for node: Node) -> String {
        if node.title == L("Conflicts"), node.page != nil { return "exclamationmark.triangle" }
        if let page = node.page { return page == .workingCopy ? "square.and.pencil" : "clock" }
        if node.pullRequest != nil { return "arrow.triangle.pull" }
        if node.isCleanWorktrees { return "checkmark.circle" }
        if let w = node.worktree { return node.isHead ? "pencil.circle" : w.main ? "house" : "arrow.triangle.branch" }
        if node.remote != nil { return "network" }
        if node.isPinned { return "star.fill" }
        if node.stackedOn != nil { return "arrow.turn.down.right" }
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
        // お気に入りのブランチは、Branches の直下のお気に入りの間に落として並べ替える。
        if isLocal, pinnedOrder.contains(dragged), node.title == "Branches", node.item == nil, node.page == nil,
           index != NSOutlineViewDropOnItemIndex, index <= pinnedOrder.count {
            return .move
        }
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
        if isLocal, index != NSOutlineViewDropOnItemIndex, let from = pinnedOrder.firstIndex(of: dragged) {
            var order = pinnedOrder
            order.remove(at: from)
            order.insert(dragged, at: min(index > from ? index - 1 : index, order.count))
            if order != pinnedOrder { savePinnedOrder(order) }
            return true
        }
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
            if let other = otherWorktree(using: target) {
                // ほかの worktree が checkout しているブランチは、ここでは checkout できない。その worktree で行う。
                add(L("Open the Worktree of %@ to Rebase It", target)) { [weak self] in self?.onSelectWorktree?(other.path) }
            } else {
                add("Rebase \(target) onto \(dragged)") {
                    actions.perform(sequence: checkout + [["op": "rebase", "onto": dragged]])
                }
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
        if let w = (outline.item(atRow: outline.clickedRow) as? Node)?.worktree {
            let path = w.path
            if w.prunable == nil {
                menu.addItem(ClosureMenuItem(title: L("Edit This Worktree")) { [weak self] in self?.onSelectWorktree?(path) })
                menu.addItem(ClosureMenuItem(title: L("Show in Finder")) { [weak self] in self?.onWorktreeAction?(path, .reveal) })
                menu.addItem(ClosureMenuItem(title: L("Open in Terminal")) { [weak self] in self?.onWorktreeAction?(path, .terminal) })
            }
            if !w.main {
                menu.addItem(.separator())
                if w.prunable != nil {
                    menu.addItem(ClosureMenuItem(title: L("Prune Worktrees")) { [weak self] in self?.onWorktreeAction?(path, .prune) })
                } else {
                    menu.addItem(ClosureMenuItem(title: L("Remove Worktree…")) { [weak self] in self?.onWorktreeAction?(path, .remove) })
                }
            }
            return
        }
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
        // 複数選んだ行の上で開いたら、選んだものをまとめて扱う（名前をコピー。ローカルブランチだけなら、まとめて
        // お気に入り・アーカイブ・削除も）。
        let selected = outline.selectedRowIndexes.compactMap { (outline.item(atRow: $0) as? Node)?.item }
        if selected.count > 1, outline.selectedRowIndexes.contains(outline.clickedRow) {
            let names = selected.map(\.name)
            add(L("Copy %ld Names", names.count)) {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(names.joined(separator: "\n"), forType: .string)
            }
            let locals = selected.filter { $0.kind == .local }.map(\.name)
            if locals.count == selected.count {
                menu.addItem(.separator())
                // お気に入りは、全部入っていれば外す、そうでなければ（まだのものを）入れる。
                let allPinned = locals.allSatisfy { pinnedOrder.contains($0) }
                add(allPinned ? L("Remove %ld from Favorites", locals.count) : L("Add %ld to Favorites", locals.count)) { [weak self] in
                    guard let self else { return }
                    if allPinned {
                        savePinnedOrder(pinnedOrder.filter { !locals.contains($0) })
                        actions.perform(sequence: locals.map {
                            ["op": "configSet", "key": "branch.\($0).amonhenPinned", "value": NSNull()]
                        })
                    } else {
                        savePinnedOrder(pinnedOrder + locals.filter { !pinnedOrder.contains($0) })
                    }
                }
                let allArchived = locals.allSatisfy { branchFacts[$0]?.archived == true }
                add(allArchived ? L("Unarchive %ld Branches", locals.count) : L("Archive %ld Branches", locals.count)) {
                    actions.perform(sequence: locals.map {
                        ["op": "configSet", "key": "branch.\($0).amonhenArchived", "value": allArchived ? NSNull() : "true"]
                    })
                }
                // 今のブランチと、ほかの worktree が checkout しているブランチは消せない（git が断る）。
                if locals.allSatisfy({ $0 != headBranch && otherWorktree(using: $0) == nil }) {
                    add(L("Delete %ld Branches…", locals.count)) { [weak self] in self?.confirmDelete(locals) }
                }
            }
            return
        }
        let current = headBranch ?? "HEAD"
        // 名前をコピー（ターミナルや PR に貼るため。リモートブランチは origin/ を付けたまま）。
        let copyTitle = switch item.kind {
        case .local, .remote: L("Copy Branch Name")
        case .tag: L("Copy Tag Name")
        default: L("Copy Name")
        }
        add(copyTitle) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(name, forType: .string)
        }
        menu.addItem(.separator())
        switch item.kind {
        case .local:
            if name != headBranch {
                if let other = otherWorktree(using: name) {
                    // ほかの worktree が checkout しているブランチは checkout できない（git が断る）。その worktree を開く。
                    add(L("Open Worktree %@", other.name)) { [weak self] in self?.onSelectWorktree?(other.path) }
                } else {
                    add("Checkout \(name)") { actions.perform(["op": "checkout", "branch": name]) }
                }
                add("Merge \(name) into \(current)") { actions.perform(["op": "merge", "rev": name]) }
                add("Rebase \(current) onto \(name)") { actions.perform(["op": "rebase", "onto": name]) }
                menu.addItem(.separator())
            }
            // お気に入りとアーカイブは git の設定に書く（ブランチを消せば一緒に消え、名前を変えればついていく）。
            let pinned = pinnedOrder.contains(name)
            add(pinned ? L("Remove from Favorites") : L("Add to Favorites")) { [weak self] in
                guard let self else { return }
                savePinnedOrder(pinned ? pinnedOrder.filter { $0 != name } : pinnedOrder + [name])
                if pinned { actions.perform(["op": "configSet", "key": "branch.\(name).amonhenPinned", "value": NSNull()]) }
            }
            let archived = branchGroups?.branches.first { $0.name == name }?.archived ?? false
            add(archived ? L("Unarchive") : L("Archive")) {
                actions.perform(["op": "configSet", "key": "branch.\(name).amonhenArchived",
                                 "value": archived ? NSNull() : "true"])
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

    /// 選んだブランチをまとめて消す前に、名前を並べて確かめる。消すのは 1 つずつ（git が断ったらそこで止まり、
    /// 帯に出る）。
    private func confirmDelete(_ names: [String]) {
        guard let actions, let window = view.window else { return }
        let alert = NSAlert()
        alert.messageText = L("Delete %ld branches?", names.count)
        alert.informativeText = names.joined(separator: "\n") + "\n\n"
            + L("A branch with commits that are merged nowhere is kept (git refuses).") + " "
            + L("You can Undo it afterwards (⌥⌘Z).")
        let button = alert.addButton(withTitle: L("Delete"))
        button.hasDestructiveAction = true
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            actions.perform(sequence: names.map { ["op": "deleteBranch", "name": $0, "force": false] })
        }
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        // ブランチ・タグを複数選んだら、それぞれから辿れる履歴をまとめて出す。
        if outline.selectedRowIndexes.count > 1 {
            let selected = outline.selectedRowIndexes.compactMap { (outline.item(atRow: $0) as? Node)?.item }
                .filter { [.local, .remote, .tag].contains($0.kind) && $0.target != nil }
            if selected.count > 1 {
                onSelectPage?(.history)
                onSelectMany?(selected)
                return
            }
        }
        guard let node = outline.item(atRow: outline.selectedRow) as? Node else { return }
        if node === conflictsNode {
            onSelectPage?(.workingCopy)
            onSelectConflicts?()
        } else if let page = node.page {
            onSelectPage?(page)
        } else if let item = node.item {
            onSelectPage?(.history)
            onSelect?(item)
        } else if let w = node.worktree {
            // 選んだ worktree を書き込む先にして、その変更を見せる。
            guard w.prunable == nil else { return }
            onSelectWorktree?(w.path)
            onSelectPage?(.workingCopy)
        } else if let pr = node.pullRequest,
                  let branch = refs?.items.first(where: { $0.kind == .remote && $0.name.hasSuffix("/" + pr.headRefName) })
                    ?? refs?.items.first(where: { $0.kind == .local && $0.name == pr.headRefName }) {
            // プルリクエストのブランチが手元にあれば、その履歴に絞り込む。
            onSelectPage?(.history)
            onSelect?(branch)
        }
    }
}

/// 右端の文字（数・アイコン）。クリックはセルに渡す（セルが、押されたのが右端かを見る）。
private final class BadgeLabel: NSTextField {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

/// サイドバーの行: アイコン、名前、右に薄い数（ahead・behind や変更の数）。
final class SidebarCell: NSTableCellView {
    let badge: NSTextField = BadgeLabel(labelWithString: "")
    /// 右端のアイコンを押した（取り込み済み・gone の詳細の吹き出し）。なければ押しても何も起きない。
    var onBadgeClick: (() -> Void)?

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

    /// `point`（セルの座標）が右端のアイコンの上か。小さなアイコンなので、少し広めに当てる。
    func hitsBadge(_ point: NSPoint) -> Bool {
        onBadgeClick != nil && badge.frame.insetBy(dx: -6, dy: -6).contains(point)
    }
}

/// サイドバーの表。行の右端のアイコン（取り込み済み・gone）の上で押したら、行を選ばずに吹き出しを出す。
/// セルの mouseDown では拾えない（表が先に受けて、行の選択とドラッグにする）ので、表で見る。
final class SidebarOutlineView: NSOutlineView {
    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let row = self.row(at: point)
        if row >= 0, let cell = view(atColumn: 0, row: row, makeIfNecessary: false) as? SidebarCell,
           cell.hitsBadge(cell.convert(event.locationInWindow, from: nil)) {
            cell.onBadgeClick?()
            return
        }
        super.mouseDown(with: event)
    }
}

/// 選んだら閉包を呼ぶメニュー項目を作る。
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
