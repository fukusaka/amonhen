import AmonhenKit
import AppKit

/// 開いたことのあるリポジトリの一覧（新しく開いた順）。UserDefaults に置く。
enum RepositoryList {
    private static let key = "repositories"

    static var paths: [String] {
        get { UserDefaults.standard.stringArray(forKey: key) ?? [] }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }

    /// 開いているリポジトリ（次の起動で開き直す）。ウィンドウごとに、タブの順。前の版の記録（ウィンドウごとに
    /// 1 つ）しかなければ、それぞれを 1 つのウィンドウにする。
    static var openWindows: [[String]] {
        get {
            if let groups = UserDefaults.standard.array(forKey: "openWindows") as? [[String]] { return groups }
            return (UserDefaults.standard.stringArray(forKey: "openRepositories") ?? []).map { [$0] }
        }
        set { UserDefaults.standard.set(newValue, forKey: "openWindows") }
    }

    /// 最後に開いた日時（リポジトリの一覧で見渡すため）。この記録を始める前に開いたものは nil。
    static func openedAt(_ path: String) -> Date? {
        (UserDefaults.standard.dictionary(forKey: openedKey)?[path] as? Double).map { Date(timeIntervalSince1970: $0) }
    }

    private static let openedKey = "repositoryOpenedAt"

    /// 開いたリポジトリを先頭に置き、開いた日時を覚える。
    static func touch(_ path: String) {
        paths = [path] + paths.filter { $0 != path }
        var opened = UserDefaults.standard.dictionary(forKey: openedKey) ?? [:]
        opened[path] = Date().timeIntervalSince1970
        UserDefaults.standard.set(opened, forKey: openedKey)
    }

    static func remove(_ path: String) {
        paths = paths.filter { $0 != path }
        var opened = UserDefaults.standard.dictionary(forKey: openedKey) ?? [:]
        opened[path] = nil
        UserDefaults.standard.set(opened, forKey: openedKey)
    }

    /// `path` が Git のリポジトリ（作業ツリーの直下に `.git` がある）か。daemon 越しの場所は、つなぐまで
    /// わからないので、あるものとする。
    static func isRepository(_ path: String) -> Bool {
        if RepositoryLocation.isRemote(path) { return true }
        return FileManager.default.fileExists(atPath: (path as NSString).appendingPathComponent(".git"))
    }

    /// `path` が追加した worktree なら、その元のリポジトリの作業ツリー（`.git` が
    /// `gitdir: <元>/.git/worktrees/<名前>` のファイル）。手元のものだけ。
    static func mainRepository(ofWorktree path: String) -> String? {
        if RepositoryLocation.isRemote(path) { return nil }
        if let cached = worktreeMains[path] { return cached }
        var main: String?
        let dotGit = (path as NSString).appendingPathComponent(".git")
        var isDirectory: ObjCBool = false
        if FileManager.default.fileExists(atPath: dotGit, isDirectory: &isDirectory), !isDirectory.boolValue,
           let text = try? String(contentsOfFile: dotGit, encoding: .utf8), text.hasPrefix("gitdir:") {
            let target = text.dropFirst("gitdir:".count).trimmingCharacters(in: .whitespacesAndNewlines)
            let gitDir = target.hasPrefix("/") ? target : (path as NSString).appendingPathComponent(target)
            if let range = gitDir.range(of: "/.git/worktrees/") {
                main = String(gitDir[..<range.lowerBound])
            }
        }
        worktreeMains[path] = .some(main)
        return main
    }

    nonisolated(unsafe) private static var worktreeMains: [String: String?] = [:]

    /// 今のブランチ。`.git/HEAD` を読むだけで、git は起動しない（一覧を開くたびに数十件を読むため）。
    static func branch(of path: String) -> String? {
        if RepositoryLocation.isRemote(path) { return nil }
        var gitDir = (path as NSString).appendingPathComponent(".git")
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: gitDir, isDirectory: &isDirectory) else { return nil }
        // worktree やサブモジュールでは `.git` が `gitdir: <path>` のファイルになる。
        if !isDirectory.boolValue,
           let text = try? String(contentsOfFile: gitDir, encoding: .utf8), text.hasPrefix("gitdir:") {
            let target = text.dropFirst("gitdir:".count).trimmingCharacters(in: .whitespacesAndNewlines)
            gitDir = target.hasPrefix("/") ? target : (path as NSString).appendingPathComponent(target)
        }
        guard let head = try? String(contentsOfFile: (gitDir as NSString).appendingPathComponent("HEAD"), encoding: .utf8)
        else { return nil }
        let line = head.trimmingCharacters(in: .whitespacesAndNewlines)
        if line.hasPrefix("ref: refs/heads/") { return String(line.dropFirst("ref: refs/heads/".count)) }
        return "detached " + String(line.prefix(8))
    }
}

/// リポジトリの一覧。各リポジトリは 1 回だけ出す。
///
/// 2 つの見た目で使う。
/// - `page`: 新しいタブのページ。左に分類（すべて・お気に入り・場所（このMac・SSH のホストごと）・手で作ったグループ・
///   スキャンするフォルダ）、右にその中の
///   リポジトリを列（名前・ブランチ・状態・最後のコミット・開いた日時）で並べ、列で並べ替える（既定は最後に開いた順）。
///   行を左のグループ・お気に入りにドラッグすると入れられる。
/// - `compact`: ツールバーのリポジトリ名から出す一覧。お気に入り、そのあと最後に開いた順に、名前と状態だけを出す。
///   選ぶとそのタブのリポジトリを切り替える。
///
/// 1 画面に並べるものは 5〜6 個に抑える（固定の分類は 2 つ、列は 5 つ、下のボタンは 2 つ）。それ以外の操作は右クリック。
final class RepositoryBrowser: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSOutlineViewDataSource,
    NSOutlineViewDelegate, NSSearchFieldDelegate, NSMenuDelegate
{
    enum Style { case page, compact }

    /// 開き方: `here` はこのタブ（ページならページの代わりに、ツールバーの一覧なら今のリポジトリと入れ替える）。
    enum OpenMode { case here, newTab, newWindow }

    let style: Style
    /// リポジトリを開く。AppDelegate・RepositoryWindow が設定する。
    var onOpen: ((String, OpenMode) -> Void)?
    /// ツールバーの一覧の「すべてのリポジトリ…」: ページを出す。
    var onShowAll: (() -> Void)?

    /// 表示している一覧（ブックマークや行の状態が変わったら、全部に知らせる）。
    private static let live = NSHashTable<RepositoryBrowser>.weakObjects()

    /// ブックマーク・スキャンの結果が変わった。
    static func bookmarksChanged() {
        for browser in live.allObjects { browser.bookmarksChanged() }
    }

    /// 行の状態（ブランチ・変更の数など）が変わった。
    static func rowChanged(_ path: String) {
        for browser in live.allObjects { browser.rowChanged(path) }
    }

    // MARK: - 分類

    /// 左の分類。
    private enum Category: Equatable {
        /// `local` はこの Mac のリポジトリ（手元の共有の daemon 越しも含む）、`host` は SSH のホストごと。`folder` は
        /// この Mac のフォルダの下、`hostFolder` は SSH のホストのフォルダの下（場所の階層）、`scanFolder` はスキャンする
        /// フォルダの中のフォルダの下（スキャンの階層）。場所とスキャンで同じフォルダが出ても、別の分類にする（同じに
        /// すると、スキャンの中で選んだものが場所の中の節に選び直されてしまう）。
        case all, favorites, local, host(String), group(String), scan(String), folder(String), hostFolder(String, String)
        case scanFolder(String, String)

        /// 覚えておくための文字列。
        var key: String {
            switch self {
            case .all: "all"
            case .favorites: "favorites"
            case .local: "local"
            case let .host(name): "host:" + name
            case let .group(name): "group:" + name
            case let .scan(path): "scan:" + path
            case let .folder(path): "folder:" + path
            case let .hostFolder(host, path): "hostfolder:" + host + "\n" + path
            case let .scanFolder(root, path): "scanfolder:" + root + "\n" + path
            }
        }

        init?(key: String) {
            if key == "all" { self = .all } else if key == "favorites" { self = .favorites }
            else if key == "local" { self = .local }
            else if key.hasPrefix("host:") { self = .host(String(key.dropFirst(5))) }
            else if key.hasPrefix("group:") { self = .group(String(key.dropFirst(6))) }
            else if key.hasPrefix("scan:") { self = .scan(String(key.dropFirst(5))) }
            else if key.hasPrefix("folder:") { self = .folder(String(key.dropFirst(7))) }
            else if key.hasPrefix("hostfolder:"), let split = key.dropFirst(11).firstIndex(of: "\n") {
                let rest = key.dropFirst(11)
                self = .hostFolder(String(rest[..<split]), String(rest[rest.index(after: split)...]))
            }
            else if key.hasPrefix("scanfolder:"), let split = key.dropFirst(11).firstIndex(of: "\n") {
                let rest = key.dropFirst(11)
                self = .scanFolder(String(rest[..<split]), String(rest[rest.index(after: split)...]))
            }
            else { return nil }
        }
    }

    /// SSH 先のリポジトリなら、そのホスト（`ssh://host/…`・`daemon:ssh://host/…`）。この Mac のものは nil。
    static func sshHost(_ location: String) -> String? {
        let rest = location.hasPrefix("daemon:") ? String(location.dropFirst("daemon:".count)) : location
        guard rest.hasPrefix("ssh://") else { return nil }
        return String(rest.dropFirst("ssh://".count).prefix { $0 != "/" })
    }

    /// 手元のリポジトリのパス（`daemon:/path` なら `/path`）。
    static func localPath(_ location: String) -> String {
        location.hasPrefix("daemon:") ? String(location.dropFirst("daemon:".count)) : location
    }

    /// 左の木の節: 見出し（場所・グループ・スキャン）か分類。場所とスキャンの下は、リポジトリのあるフォルダの階層。
    private final class SideNode {
        let title: String
        let symbol: String
        /// nil なら見出し。
        let category: Category?
        var children: [SideNode] = []

        /// 見出しの、開いた・閉じたを覚えるための名前（訳した `title` は言語で変わるので使わない）。
        let header: String?

        init(_ title: String, symbol: String = "", _ category: Category?, header: String? = nil, children: [SideNode] = []) {
            self.title = title
            self.header = header
            self.symbol = symbol
            self.category = category
            self.children = children
        }

        /// 開いた・閉じたを覚えるための名前。
        var key: String { category?.key ?? "header:" + (header ?? title) }
    }

    private static let categoryKey = "repositoryPageCategory"
    private var category: Category = .all {
        didSet { UserDefaults.standard.set(category.key, forKey: Self.categoryKey) }
    }

    private var sideRoots: [SideNode] = []
    /// 開いている節（`SideNode.key`）。最初は見出しと「このMac」を開く。
    private static let expandedKey = "repositoryPageExpanded"
    private var expanded: Set<String> = Set(
        UserDefaults.standard.stringArray(forKey: RepositoryBrowser.expandedKey) ?? ["header:場所", "header:グループ", "header:スキャン", "local"])
    /// 木を作り直している間（開閉の知らせで、覚えている状態を書き換えない）。
    private var rebuildingSidebar = false
    /// 右に出しているリポジトリ（絞り込み・並べ替えの後）。
    private var shown: [String] = []

    /// ⏎ で開く（表の中で）。
    private final class Table: NSTableView {
        var onReturn: (() -> Void)?
        override func keyDown(with event: NSEvent) {
            if event.keyCode == 36 || event.keyCode == 76 { onReturn?() } else { super.keyDown(with: event) }
        }
    }

    private let sidebar = NSOutlineView()
    private let table = Table()
    private let tableScroll = NSScrollView()
    private let filter = NSSearchField()
    private let scanning = NSProgressIndicator()
    /// 左の下の ＋（ページだけ）。
    private var addButton: NSPopUpButton?

    init(style: Style) {
        self.style = style
        super.init(nibName: nil, bundle: nil)
        if style == .page, let saved = UserDefaults.standard.string(forKey: Self.categoryKey).flatMap(Category.init(key:)) {
            category = saved
        }
        Self.live.add(self)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: style == .page ? 1100 : 420, height: style == .page ? 700 : 480))
        build()
        reload()
    }

    /// 出す前に: 行の状態を読み直し、スキャンするフォルダを探し直して、絞り込みの欄に入る。
    func prepare() {
        _ = view
        Bookmarks.shared.forgetPeeks()
        Bookmarks.shared.rescan()
        filter.stringValue = ""
        reload()
        view.window?.makeFirstResponder(filter)
    }

    /// 絞り込みの欄に入る（ページを前に出したとき）。
    func focusFilter() {
        view.window?.makeFirstResponder(filter)
    }

    private func bookmarksChanged() {
        guard isViewLoaded, view.window != nil else { return }
        scanning.isHidden = !Bookmarks.shared.isScanning
        if Bookmarks.shared.isScanning { scanning.startAnimation(nil) } else { scanning.stopAnimation(nil) }
        reload()
    }

    /// 行の状態が変わった。その行だけ描き直す（並びは変えない。読めるたびに行が動くと選びにくい）。
    private func rowChanged(_ path: String) {
        guard isViewLoaded, view.window != nil, let row = shown.firstIndex(of: path) else { return }
        table.reloadData(forRowIndexes: [row], columnIndexes: IndexSet(integersIn: 0..<table.numberOfColumns))
    }

    // MARK: - 組み立て

    private func build() {
        let page = style == .page
        // 右: リポジトリの表。
        let name = NSTableColumn(identifier: .init("name"))
        name.title = L("Name")
        name.minWidth = 200
        name.resizingMask = .autoresizingMask
        name.sortDescriptorPrototype = NSSortDescriptor(key: "name", ascending: true)
        table.addTableColumn(name)
        if page {
            for (id, title, width, ascending) in [
                ("branch", L("Branch"), 150.0, true), ("status", L("Status"), 100.0, false),
                ("commit", L("Last Commit"), 300.0, false), ("opened", L("Opened"), 100.0, false),
            ] {
                let column = NSTableColumn(identifier: .init(id))
                column.title = title
                column.width = width
                column.minWidth = 60
                column.resizingMask = .userResizingMask
                column.sortDescriptorPrototype = NSSortDescriptor(key: id, ascending: ascending)
                column.headerToolTip = [
                    "branch": L("Current branch"),
                    "status": L("Changes n: files with changes (untracked files aren’t counted)\n↑ commits ahead of the upstream · ↓ commits behind it\n✓ no changes and same as the upstream"),
                    "commit": L("Date and subject of the last commit"),
                    "opened": L("When you last opened it in this app (— if never)"),
                ][id]
                table.addTableColumn(column)
            }
            table.autosaveName = "RepositoryPage.v2"
            table.autosaveTableColumns = true
            table.style = .inset
            table.usesAlternatingRowBackgroundColors = true
            let sortKey = UserDefaults.standard.string(forKey: "repositoryPageSort") ?? "opened"
            let ascending = UserDefaults.standard.object(forKey: "repositoryPageSortAscending") as? Bool ?? false
            table.sortDescriptors = [NSSortDescriptor(key: sortKey, ascending: ascending)]
        } else {
            table.headerView = nil
            table.style = .sourceList
        }
        table.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        table.rowHeight = 40
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(openSelected)
        table.onReturn = { [weak self] in self?.openSelected() }
        table.allowsMultipleSelection = true
        table.setDraggingSourceOperationMask(.move, forLocal: true)
        let menu = NSMenu()
        menu.delegate = self
        table.menu = menu
        tableScroll.documentView = table
        tableScroll.hasVerticalScroller = true
        tableScroll.drawsBackground = false

        filter.placeholderString = page ? L("Filter by name or path") : L("Switch repository")
        filter.delegate = self
        filter.target = self
        filter.action = #selector(filterChanged)
        scanning.style = .spinning
        scanning.controlSize = .small
        scanning.isHidden = true

        // 下のボタン（2 つ）。
        let bottom: NSStackView
        if page {
            let add = NSPopUpButton(frame: .zero, pullsDown: true)
            add.addItem(withTitle: L("＋ Add"))
            for (title, action) in [
                (L("Local Repository…"), #selector(addRepository)), ("Clone…", #selector(clone)),
                (L("SSH Connection…"), #selector(addRemote as () -> Void)), ("-", nil),
                (L("Group…"), #selector(newGroup)), (L("Folder to Scan…"), #selector(addScanFolder)),
            ] as [(String, Selector?)] {
                if title == "-" {
                    add.menu?.addItem(.separator())
                    continue
                }
                let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
                item.target = self
                add.menu?.addItem(item)
            }
            addButton = add
            add.bezelStyle = .accessoryBarAction
            add.isBordered = false
            (add.cell as? NSPopUpButtonCell)?.arrowPosition = .noArrow
            add.item(at: 0)?.title = L("Add")
            add.imagePosition = .imageLeading
            add.item(at: 0)?.image = NSImage(systemSymbolName: "plus", accessibilityDescription: L("Add"))
            add.toolTip = L("Add repositories, SSH connections, groups and folders to scan")
            // 右は絞り込みと一覧だけ（開くのはダブルクリックと ⏎、加えるのは左の ＋ と右クリック）。
            bottom = NSStackView(views: [])
        } else {
            let all = NSButton(title: L("All Repositories…"), target: self, action: #selector(showAll))
            all.bezelStyle = .inline
            all.toolTip = L("Open the repository page (⌘⇧R)")
            bottom = NSStackView(views: [scanning, NSView(), all])
        }

        let right = NSView()
        let margin: CGFloat = page ? 14 : 10
        for v in [filter, tableScroll, bottom] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            right.addSubview(v)
        }
        NSLayoutConstraint.activate([
            filter.topAnchor.constraint(equalTo: right.topAnchor, constant: margin),
            filter.leadingAnchor.constraint(equalTo: right.leadingAnchor, constant: margin),
            filter.trailingAnchor.constraint(equalTo: right.trailingAnchor, constant: -margin),
            tableScroll.topAnchor.constraint(equalTo: filter.bottomAnchor, constant: 8),
            tableScroll.leadingAnchor.constraint(equalTo: right.leadingAnchor),
            tableScroll.trailingAnchor.constraint(equalTo: right.trailingAnchor),
            bottom.topAnchor.constraint(equalTo: tableScroll.bottomAnchor, constant: page ? 0 : 8),
            bottom.leadingAnchor.constraint(equalTo: right.leadingAnchor, constant: margin),
            bottom.trailingAnchor.constraint(equalTo: right.trailingAnchor, constant: -margin),
            bottom.bottomAnchor.constraint(equalTo: right.bottomAnchor, constant: page ? 0 : -margin),
        ])

        guard page else {
            right.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(right)
            NSLayoutConstraint.activate([
                right.topAnchor.constraint(equalTo: view.topAnchor), right.bottomAnchor.constraint(equalTo: view.bottomAnchor),
                right.leadingAnchor.constraint(equalTo: view.leadingAnchor), right.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            ])
            return
        }

        // 左: 分類。
        let column = NSTableColumn(identifier: .init("category"))
        column.resizingMask = .autoresizingMask
        sidebar.addTableColumn(column)
        sidebar.outlineTableColumn = column
        sidebar.headerView = nil
        sidebar.style = .sourceList
        sidebar.rowHeight = 26
        sidebar.floatsGroupRows = false
        sidebar.dataSource = self
        sidebar.delegate = self
        sidebar.registerForDraggedTypes([Self.dragType])
        let sideMenu = NSMenu()
        sideMenu.delegate = self
        sidebar.menu = sideMenu
        let sideScroll = NSScrollView()
        sideScroll.documentView = sidebar
        sideScroll.hasVerticalScroller = true
        sideScroll.drawsBackground = false
        let left = NSVisualEffectView()
        left.material = .sidebar
        left.blendingMode = .behindWindow
        let leftBar = NSStackView(views: [addButton ?? NSView(), scanning, NSView()])
        leftBar.spacing = 6
        for v in [sideScroll, leftBar] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            left.addSubview(v)
        }
        NSLayoutConstraint.activate([
            sideScroll.topAnchor.constraint(equalTo: left.topAnchor, constant: 8),
            sideScroll.leadingAnchor.constraint(equalTo: left.leadingAnchor),
            sideScroll.trailingAnchor.constraint(equalTo: left.trailingAnchor),
            leftBar.topAnchor.constraint(equalTo: sideScroll.bottomAnchor, constant: 4),
            leftBar.leadingAnchor.constraint(equalTo: left.leadingAnchor, constant: 10),
            leftBar.trailingAnchor.constraint(equalTo: left.trailingAnchor, constant: -10),
            leftBar.bottomAnchor.constraint(equalTo: left.bottomAnchor, constant: -8),
        ])

        let split = NSSplitView()
        split.isVertical = true
        split.dividerStyle = .thin
        split.autosaveName = "RepositoryPageSplit.v2"
        split.addArrangedSubview(left)
        split.addArrangedSubview(right)
        split.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(split)
        NSLayoutConstraint.activate([
            split.topAnchor.constraint(equalTo: view.topAnchor), split.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            split.leadingAnchor.constraint(equalTo: view.leadingAnchor), split.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            left.widthAnchor.constraint(greaterThanOrEqualToConstant: 200),
        ])
        left.widthAnchor.constraint(equalToConstant: 250).priority = .defaultLow
        split.setHoldingPriority(.defaultHigh, forSubviewAt: 0)
        split.setPosition(250, ofDividerAt: 0)
    }

    // MARK: - 左の木

    /// 左の木: すべて・お気に入り、場所（このMac・SSH のホストごと。その下はフォルダの階層）、グループ、スキャン（その下は
    /// フォルダの階層）。
    private func makeSidebar() -> [SideNode] {
        let bookmarks = Bookmarks.shared
        let all = bookmarks.allLocations
        var roots = [SideNode(L("All"), symbol: "tray.full", .all), SideNode(L("Favorites"), symbol: "star", .favorites)]

        // 場所: この Mac と SSH のホストごと。その下に、スキャンするフォルダ（🔍。そこだけを探す）と、それ以外の
        // リポジトリ（開いた・手で入れたもの）のあるフォルダの木。
        func scanNodes(_ roots: [ScanRoot], title: (String) -> String) -> [SideNode] {
            roots.map { root in
                let base = Self.pathPart(root.path)
                return SideNode(title(base), symbol: "magnifyingglass", .scan(root.path),
                                children: folderTree((bookmarks.scanned[root.path] ?? []).map(Self.pathPart), base: base,
                                                     category: { .scanFolder(root.path, $0) }, title: { $0 }))
            }
        }
        /// どのスキャンするフォルダの下にもないもの（木に 2 回出さない）。
        func outside(_ paths: [String], of roots: [ScanRoot]) -> [String] {
            let bases = roots.map { Self.pathPart($0.path) }
            return paths.filter { path in !bases.contains { path == $0 || path.hasPrefix($0 + "/") } }
        }
        let localRoots = bookmarks.data.scanRoots.filter { Self.sshHost($0.path) == nil }
        let local = all.filter { Self.sshHost($0) == nil }.map(Self.localPath)
        let tilde = { (path: String) in (path as NSString).abbreviatingWithTildeInPath }
        var places = [SideNode(L("This Mac"), symbol: "laptopcomputer", .local,
                               children: scanNodes(localRoots, title: tilde)
                                   + folderTree(outside(local, of: localRoots), base: "", category: { .folder($0) }, title: tilde))]
        let hosts = Set(all.compactMap(Self.sshHost) + bookmarks.data.scanRoots.compactMap { Self.sshHost($0.path) })
        for host in hosts.sorted() {
            let hostRoots = bookmarks.data.scanRoots.filter { Self.sshHost($0.path) == host }
            RemoteHome.learn(host)
            let remote = all.filter { Self.sshHost($0) == host }.map(Self.pathPart)
            places.append(SideNode(host, symbol: "network", .host(host),
                                   children: scanNodes(hostRoots, title: Self.remoteTitle)
                                       + folderTree(outside(remote, of: hostRoots), base: "", category: { .hostFolder(host, $0) },
                                                    title: Self.remoteTitle)))
        }
        roots.append(SideNode(L("Places"), nil, header: "場所", children: places))
        // グループは作ってから出す（空の見出しを並べない。作るのは左下の ＋ と、リポジトリの右クリック）。
        if !bookmarks.data.groups.isEmpty {
            roots.append(SideNode(L("Groups"), nil, header: "グループ", children: bookmarks.data.groups.map { SideNode($0, symbol: "folder", .group($0)) }))
        }
        return roots
    }

    /// `location` が、同じ場所（`host`。手元なら nil）のスキャンするフォルダの下にあるか。
    private static func isScanned(_ location: String, host: String?) -> Bool {
        let path = pathPart(location)
        return Bookmarks.shared.data.scanRoots.contains { root in
            guard sshHost(root.path) == host else { return false }
            let base = pathPart(root.path)
            return path == base || path.hasPrefix(base + "/")
        }
    }

    /// 場所の中のパス（SSH 先ならそのホストの上のパス、手元の共有の daemon 越しなら手元のパス）。
    /// SSH 先では、ホームの下の絶対パスを `~` に揃える（`~/prj` と `/home/me/prj` を同じ木に並べる）。
    static func pathPart(_ location: String) -> String {
        guard let host = sshHost(location) else { return localPath(location) }
        let path = RepositoryLocation.path(location)
        if let home = RemoteHome.known(host), path == home || path.hasPrefix(home + "/") {
            return "/~" + path.dropFirst(home.count)
        }
        return path
    }

    /// SSH 先のパスの見せ方（`/~/prj` は `~/prj`）。
    private static func remoteTitle(_ path: String) -> String {
        path.hasPrefix("/~") ? String(path.dropFirst()) : path
    }

    /// リポジトリのあるフォルダの階層。`repositories` はリポジトリのパス（`base` の下）。リポジトリの入っている
    /// フォルダとその上のフォルダを節にし、枝分かれもリポジトリもない途中のフォルダは 1 つにまとめる（`a/b`）。
    /// 一番上の節の名前は `title`（フルパスから）、その下はひとつ上からの相対。
    private func folderTree(
        _ repositories: [String], base: String, category: (String) -> Category, title: (String) -> String
    ) -> [SideNode] {
        final class Trie {
            var children: [String: Trie] = [:]
            var repositories = 0
        }
        let root = Trie()
        for path in repositories {
            let parent = (path as NSString).deletingLastPathComponent
            guard parent == base || parent.hasPrefix(base + "/") || base.isEmpty else { continue }
            var node = root
            for part in parent.dropFirst(base.count).split(separator: "/") {
                let next = node.children[String(part)] ?? Trie()
                node.children[String(part)] = next
                node = next
            }
            node.repositories += 1
        }
        func make(_ node: Trie, prefix: String, top: Bool) -> [SideNode] {
            node.children.keys.sorted { $0.localizedStandardCompare($1) == .orderedAscending }.map { name in
                var child = node.children[name]!
                var path = prefix + "/" + name
                var label = name
                while child.repositories == 0, child.children.count == 1, let (next, grand) = child.children.first {
                    path += "/" + next
                    label += "/" + next
                    child = grand
                }
                return SideNode(top ? title(path) : label, symbol: "folder", category(path),
                                children: make(child, prefix: path, top: false))
            }
        }
        // 一番上（`base` の直下）は、枝分かれがなければそのまま下の節に降りる。
        var start = root
        var prefix = base
        while start.repositories == 0, start.children.count == 1, let (next, child) = start.children.first {
            prefix += "/" + next
            start = child
        }
        if base.isEmpty, prefix.isEmpty { return make(start, prefix: "", top: true) }
        if prefix == base { return make(start, prefix: base, top: base.isEmpty) }
        // まとめた途中のフォルダを 1 つの節にする（リポジトリが散っていれば、その下を枝に）。
        let label = base.isEmpty ? title(prefix) : String(prefix.dropFirst(base.count + 1))
        return [SideNode(label, symbol: "folder", category(prefix), children: make(start, prefix: prefix, top: false))]
    }

    /// `category` の節までの道（見つからなければ nil）。
    private func pathTo(_ category: Category, in nodes: [SideNode]) -> [SideNode]? {
        for node in nodes {
            if node.category == category { return [node] }
            if let rest = pathTo(category, in: node.children) { return [node] + rest }
        }
        return nil
    }

    // MARK: - 中身

    /// 分類の中のリポジトリ（絞り込み・並べ替えの前）。
    /// `category` のリポジトリ。追加した worktree は、元のリポジトリも一覧にあれば出さない（元のリポジトリのタブの
    /// Worktrees から使う。docs/ux.md の「Worktrees」）。
    private func paths(in category: Category) -> [String] {
        let all = Set(Bookmarks.shared.allLocations.map(Self.localPath))
        return allPaths(in: category).filter { path in
            guard let main = RepositoryList.mainRepository(ofWorktree: Self.localPath(path)) else { return true }
            return !all.contains(main)
        }
    }

    private func allPaths(in category: Category) -> [String] {
        let bookmarks = Bookmarks.shared
        switch category {
        case .all: return bookmarks.allLocations
        case .favorites: return bookmarks.data.items.filter(\.favorite).map(\.path)
        case .local: return bookmarks.allLocations.filter { Self.sshHost($0) == nil }
        case let .host(name): return bookmarks.allLocations.filter { Self.sshHost($0) == name }
        case let .group(name): return bookmarks.data.items.filter { $0.group == name }.map(\.path)
        case let .scan(path): return bookmarks.scanned[path] ?? []
        // 場所の下のフォルダは、スキャンするフォルダの外のものだけ（左の木と同じ。スキャンした分は 🔍 の下に出す）。
        case let .folder(prefix):
            return bookmarks.allLocations.filter {
                Self.sshHost($0) == nil && Self.localPath($0).hasPrefix(prefix + "/") && !Self.isScanned($0, host: nil)
            }
        case let .hostFolder(host, prefix):
            return bookmarks.allLocations.filter {
                Self.sshHost($0) == host && Self.pathPart($0).hasPrefix(prefix + "/") && !Self.isScanned($0, host: host)
            }
        case let .scanFolder(root, prefix):
            return (bookmarks.scanned[root] ?? []).filter { Self.pathPart($0).hasPrefix(prefix + "/") }
        }
    }

    /// 左と右を作り直す。選んでいた行とスクロールの位置は保つ。
    private func reload() {
        let bookmarks = Bookmarks.shared
        if style == .page {
            sideRoots = makeSidebar()
            var all: [Category] = []
            func collect(_ nodes: [SideNode]) { for n in nodes { if let c = n.category { all.append(c) }; collect(n.children) } }
            collect(sideRoots)
            // 消えたグループ・フォルダを選んでいたら、すべてに戻す。
            if !all.contains(category) { category = .all }
            rebuildingSidebar = true
            sidebar.reloadData()
            func expand(_ nodes: [SideNode]) {
                for n in nodes where !n.children.isEmpty && expanded.contains(n.key) {
                    sidebar.expandItem(n)
                    expand(n.children)
                }
            }
            expand(sideRoots)
            // 選んでいる分類が閉じた節の中なら、そこまで開く。
            if let path = pathTo(category, in: sideRoots) {
                for ancestor in path.dropLast() { sidebar.expandItem(ancestor) }
                let row = sidebar.row(forItem: path.last)
                if row >= 0 { sidebar.selectRowIndexes([row], byExtendingSelection: false) }
            }
            rebuildingSidebar = false
        }

        let selected = Set(table.selectedRowIndexes.compactMap { $0 < shown.count ? shown[$0] : nil })
        let scrolled = tableScroll.contentView.bounds.origin
        let query = filter.stringValue.trimmingCharacters(in: .whitespaces).lowercased()
        var list = paths(in: style == .page ? category : .all).filter {
            query.isEmpty || bookmarks.displayName($0).lowercased().contains(query) || $0.lowercased().contains(query)
        }
        list = sorted(list)
        shown = list
        table.reloadData()
        let rows = IndexSet(shown.indices.filter { selected.contains(shown[$0]) })
        if !rows.isEmpty, query.isEmpty {
            table.selectRowIndexes(rows, byExtendingSelection: false)
        } else if !shown.isEmpty {
            table.selectRowIndexes([0], byExtendingSelection: false)
        }
        if query.isEmpty {
            tableScroll.contentView.scroll(to: scrolled)
            tableScroll.reflectScrolledClipView(tableScroll.contentView)
        } else {
            table.scrollRowToVisible(0)
        }
    }

    /// 並べ替え。ツールバーの一覧は、お気に入り、そのあと最後に開いた順。
    private func sorted(_ list: [String]) -> [String] {
        let bookmarks = Bookmarks.shared
        let opened = { (path: String) in RepositoryList.openedAt(path) ?? .distantPast }
        guard style == .page, let sort = table.sortDescriptors.first, let key = sort.key else {
            return list.sorted { a, b in
                let fa = bookmarks.bookmark(a)?.favorite == true, fb = bookmarks.bookmark(b)?.favorite == true
                if fa != fb { return fa }
                return opened(a) > opened(b)
            }
        }
        let name = { (path: String) in bookmarks.displayName(path).lowercased() }
        let ordered: (String, String) -> Bool
        switch key {
        case "branch":
            let branch = { (path: String) in bookmarks.peek(path)?.branch ?? RepositoryList.branch(of: path) ?? "" }
            ordered = { branch($0) < branch($1) }
        case "status":
            let weight = { (path: String) -> Int in
                guard let peek = bookmarks.peek(path) else { return -1 }
                return peek.changed * 10_000 + peek.ahead + peek.behind
            }
            ordered = { weight($0) < weight($1) }
        case "commit":
            let date = { (path: String) in bookmarks.peek(path)?.lastCommitDate ?? .distantPast }
            ordered = { date($0) < date($1) }
        case "opened":
            ordered = { opened($0) < opened($1) }
        default:
            ordered = { name($0) < name($1) }
        }
        // 同じ値なら名前の順（並びが毎回変わらないように）。
        return list.sorted { a, b in
            if ordered(a, b) { return sort.ascending }
            if ordered(b, a) { return !sort.ascending }
            return name(a) < name(b)
        }
    }

    func tableView(_ tableView: NSTableView, sortDescriptorsDidChange oldDescriptors: [NSSortDescriptor]) {
        guard tableView === table, let sort = table.sortDescriptors.first else { return }
        UserDefaults.standard.set(sort.key, forKey: "repositoryPageSort")
        UserDefaults.standard.set(sort.ascending, forKey: "repositoryPageSortAscending")
        reload()
    }

    @objc private func filterChanged() { reload() }

    func controlTextDidChange(_ notification: Notification) { reload() }

    /// 絞り込みの欄から、↑↓ で行を選び、⏎ で開き、Esc で消す・閉じる。
    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.moveDown(_:)): moveSelection(1)
        case #selector(NSResponder.moveUp(_:)): moveSelection(-1)
        case #selector(NSResponder.insertNewline(_:)): openSelected()
        case #selector(NSResponder.cancelOperation(_:)):
            if !filter.stringValue.isEmpty {
                filter.stringValue = ""
                reload()
            } else if style == .compact {
                view.window?.performClose(nil)
            }
        default: return false
        }
        return true
    }

    private func moveSelection(_ step: Int) {
        guard !shown.isEmpty else { return }
        let row = min(max(0, table.selectedRow + step), shown.count - 1)
        table.selectRowIndexes([row], byExtendingSelection: false)
        table.scrollRowToVisible(row)
    }

    @objc private func showAll() { onShowAll?() }

    // MARK: - 加える

    @objc private func addRepository() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.message = L("Choose the repository folders to add to the list.")
        guard panel.runModal() == .OK else { return }
        var rejected: [String] = []
        for url in panel.urls {
            if RepositoryList.isRepository(url.path) {
                Bookmarks.shared.add(url.standardizedFileURL.path, group: selectedGroup)
            } else {
                rejected.append(url.lastPathComponent)
            }
        }
        if !rejected.isEmpty {
            let alert = NSAlert()
            alert.messageText = L("Not a Git repository")
            alert.informativeText = rejected.joined(separator: L(", "))
            alert.runModal()
        }
    }

    @objc private func clone() {
        NSApp.sendAction(#selector(AppDelegate.cloneRepository(_:)), to: nil, from: self)
    }

    /// 左で手で作ったグループを選んでいれば、その名前（加えるものをそこに入れる）。
    private var selectedGroup: String? {
        if case let .group(name) = category { return name }
        return nil
    }

    @objc private func addScanFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.message = L("Choose a folder to search for repositories (up to 3 levels deep).")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Bookmarks.shared.addScanRoot(url.standardizedFileURL.path)
    }

    /// SSH のホストでスキャンするフォルダを加える（そのホストの上で、エンジンが探す）。
    private func addRemoteScanFolder(host: String) {
        let alert = NSAlert()
        alert.messageText = L("Add a Folder to Scan on %@", host)
        alert.informativeText = L("Searches up to 3 levels below this folder on %@.", host)
        let path = NSTextField(string: "~/prj")
        path.frame = NSRect(x: 0, y: 0, width: 300, height: 24)
        alert.accessoryView = path
        alert.addButton(withTitle: L("Add"))
        alert.addButton(withTitle: L("Cancel"))
        alert.window.initialFirstResponder = path
        guard alert.runModal() == .alertFirstButtonReturn,
              let location = RepositoryLocation.ssh(host: host, path: path.stringValue) else { return }
        Bookmarks.shared.addScanRoot(location)
    }

    @objc private func newGroup() {
        guard let name = ask(L("New Group"), message: L("Group name"), initial: "") else { return }
        Bookmarks.shared.addGroup(name)
        category = .group(name)
        reload()
    }

    private func ask(_ title: String, message: String, initial: String) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: L("Cancel"))
        let field = NSTextField(string: initial)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let value = field.stringValue.trimmingCharacters(in: .whitespaces)
        return value.isEmpty ? nil : value
    }

    /// 前回入れた接続先（続けて同じホストのリポジトリを加えやすくする）。
    private static let lastHostKey = "lastRemoteHost"

    @objc private func addRemote() {
        if case let .host(name) = category { addRemote(host: name) } else { addRemote(host: nil) }
    }

    /// SSH 先のリポジトリを加えて開く。`host` を渡すと、接続先を入れた状態で聞く（ホストの行の右クリック）。
    private func addRemote(host initialHost: String?) {
        guard let window = view.window else { return }
        let host = NSTextField(string: initialHost ?? UserDefaults.standard.string(forKey: Self.lastHostKey) ?? "")
        host.placeholderString = L("user@host:port (or a name in ~/.ssh/config)")
        let path = NSTextField(string: "")
        path.placeholderString = L("~/src/repo or /srv/git/repo")
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Host:")), host],
            [NSTextField(labelWithString: L("Path:")), path],
        ])
        grid.column(at: 0).xPlacement = .trailing
        grid.rowSpacing = 8
        grid.column(at: 1).width = 300
        grid.frame = NSRect(x: 0, y: 0, width: 370, height: 56)

        let alert = NSAlert()
        alert.messageText = initialHost.map { L("Add a Repository on %@", $0) } ?? L("Add SSH Connection")
        alert.informativeText = L("Connects over SSH. Use a host you can log in to without a password (with a key or ssh-agent).")
        alert.accessoryView = grid
        alert.addButton(withTitle: L("Add and Open"))
        alert.addButton(withTitle: L("Cancel"))
        alert.window.initialFirstResponder = host.stringValue.isEmpty ? host : path
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .alertFirstButtonReturn else { return }
            guard let location = RepositoryLocation.ssh(host: host.stringValue, path: path.stringValue) else {
                let error = NSAlert()
                error.messageText = L("Enter a host and a path")
                error.informativeText = L("The host can’t contain spaces or “/”.")
                error.beginSheetModal(for: window)
                return
            }
            UserDefaults.standard.set(host.stringValue.trimmingCharacters(in: .whitespaces), forKey: Self.lastHostKey)
            Bookmarks.shared.add(location, group: selectedGroup)
            RepositoryList.touch(location)
            onOpen?(location, .here)
        }
    }

    // MARK: - 外す

    /// 今の分類から外す: すべてなら一覧から（ブックマークと開いた記録を消す）、お気に入りなら印を外す、グループなら
    /// グループから出す。スキャンで見つけたものは外せない（フォルダを外す）。
    private func remove(_ paths: [String]) {
        for path in paths {
            switch category {
            case .all:
                RepositoryList.remove(path)
                Bookmarks.shared.remove(path)
            case .favorites:
                if Bookmarks.shared.bookmark(path)?.favorite == true { Bookmarks.shared.toggleFavorite(path) }
            case .group:
                Bookmarks.shared.move(path, to: nil)
            case .local, .host, .folder, .hostFolder:
                RepositoryList.remove(path)
                Bookmarks.shared.remove(path)
            case .scan, .scanFolder:
                NSSound.beep()
            }
        }
        reload()
    }

    private var removeTitle: String? {
        switch category {
        case .all, .local, .host, .folder, .hostFolder: L("Remove from List")
        case .favorites: L("Remove from Favorites")
        case .group: L("Remove from Group")
        case .scan, .scanFolder: nil
        }
    }

    // MARK: - ドラッグ（行を左のお気に入り・グループへ）

    private static let dragType = NSPasteboard.PasteboardType("io.github.fukusaka.amonhen.repository")

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
        guard tableView === table, style == .page, row < shown.count else { return nil }
        let item = NSPasteboardItem()
        item.setString(shown[row], forType: Self.dragType)
        return item
    }

    func outlineView(
        _ outlineView: NSOutlineView, validateDrop info: NSDraggingInfo, proposedItem item: Any?, proposedChildIndex index: Int
    ) -> NSDragOperation {
        guard info.draggingPasteboard.string(forType: Self.dragType) != nil, let target = (item as? SideNode)?.category else { return [] }
        switch target {
        case .favorites, .group:
            outlineView.setDropItem(item, dropChildIndex: NSOutlineViewDropOnItemIndex)
            return .move
        default: return []
        }
    }

    func outlineView(_ outlineView: NSOutlineView, acceptDrop info: NSDraggingInfo, item: Any?, childIndex index: Int) -> Bool {
        guard let target = (item as? SideNode)?.category else { return false }
        let paths = (info.draggingPasteboard.pasteboardItems ?? []).compactMap { $0.string(forType: Self.dragType) }
        for path in paths {
            switch target {
            case .favorites:
                if Bookmarks.shared.bookmark(path)?.favorite != true { Bookmarks.shared.toggleFavorite(path) }
            case let .group(name):
                Bookmarks.shared.move(path, to: name)
            default:
                return false
            }
        }
        return !paths.isEmpty
    }

    // MARK: - 開く

    @objc private func openSelected() {
        let row = table.clickedRow >= 0 && NSApp.currentEvent?.type != .keyDown ? table.clickedRow : table.selectedRow
        guard row >= 0, row < shown.count else { return }
        open(shown[row], NSApp.currentEvent?.modifierFlags.contains(.command) == true ? .newWindow : .here)
    }

    private func open(_ path: String, _ mode: OpenMode) {
        guard RepositoryList.isRepository(path) else {
            let alert = NSAlert()
            alert.messageText = L("%@ not found", (path as NSString).lastPathComponent)
            alert.informativeText = L("%@\nRemove it from the list?", path)
            alert.addButton(withTitle: L("Remove"))
            alert.addButton(withTitle: L("Cancel"))
            if alert.runModal() == .alertFirstButtonReturn {
                RepositoryList.remove(path)
                Bookmarks.shared.remove(path)
                reload()
            }
            return
        }
        onOpen?(path, mode)
    }

    // MARK: - 右クリック

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        if menu === sidebar.menu {
            sidebarMenu(menu)
        } else {
            rowMenu(menu)
        }
    }

    private func sidebarMenu(_ menu: NSMenu) {
        guard let node = sidebar.item(atRow: sidebar.clickedRow) as? SideNode else { return }
        func add(_ title: String, _ action: @escaping () -> Void) { menu.addItem(ClosureMenuItem(title: title, action: action)) }
        guard let target = node.category else {
            // 見出し: そこに加えるもの。
            if node.header == "場所" {
                add(L("Add Local Repository…")) { [weak self] in self?.addRepository() }
                add(L("Add SSH Connection…")) { [weak self] in self?.addRemote(host: nil) }
            } else if node.header == "グループ" {
                add(L("Create Group…")) { [weak self] in self?.newGroup() }
            }
            return
        }
        let paths = paths(in: target)
        if !paths.isEmpty {
            add(L("Fetch All (%ld)", paths.count)) { Bookmarks.shared.fetch(paths) }
            menu.addItem(.separator())
        }
        switch target {
        case let .host(name):
            add(L("Add Repository on This Host…")) { [weak self] in self?.addRemote(host: name) }
            add(L("Add Folder to Scan on This Host…")) { [weak self] in self?.addRemoteScanFolder(host: name) }
        case .local:
            add(L("Add Local Repository…")) { [weak self] in self?.addRepository() }
            add(L("Add Folder to Scan…")) { [weak self] in self?.addScanFolder() }
        case let .group(name):
            add(L("Rename…")) { [weak self] in
                guard let new = self?.ask(L("Rename Group"), message: L("Group name"), initial: name) else { return }
                let members = Bookmarks.shared.data.items.filter { $0.group == name }.map(\.path)
                Bookmarks.shared.addGroup(new)
                for path in members { Bookmarks.shared.move(path, to: new) }
                Bookmarks.shared.removeGroup(name)
            }
            add(L("Delete Group")) { Bookmarks.shared.removeGroup(name) }
        case let .scan(path):
            if let error = Bookmarks.shared.scanErrors[path] {
                let item = NSMenuItem(title: L("Couldn’t search: %@", error), action: nil, keyEquivalent: "")
                item.isEnabled = false
                menu.addItem(item)
                menu.addItem(.separator())
            }
            let depth = Bookmarks.shared.data.scanRoots.first { $0.path == path }?.depth ?? 3
            let depths = NSMenu(title: L("Search Depth"))
            for d in 1...6 {
                let item = ClosureMenuItem(title: "\(d)") { Bookmarks.shared.setDepth(d, of: path) }
                item.state = d == depth ? .on : .off
                depths.addItem(item)
            }
            let depthItem = NSMenuItem(title: L("Search Depth"), action: nil, keyEquivalent: "")
            depthItem.submenu = depths
            menu.addItem(depthItem)
            add(L("Search Again")) { Bookmarks.shared.rescan() }
            if Self.sshHost(path) == nil {
                add(L("Show in Finder")) { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
            }
            add(L("Stop Scanning")) { Bookmarks.shared.removeScanRoot(path) }
        default:
            break
        }
    }

    private func rowMenu(_ menu: NSMenu) {
        let clicked = table.clickedRow
        guard clicked >= 0, clicked < shown.count else { return }
        let path = shown[clicked]
        let targets = table.selectedRowIndexes.contains(clicked)
            ? table.selectedRowIndexes.compactMap { $0 < shown.count ? shown[$0] : nil } : [path]
        let bookmark = Bookmarks.shared.bookmark(path)
        func add(_ title: String, _ action: @escaping () -> Void) { menu.addItem(ClosureMenuItem(title: title, action: action)) }
        add(style == .page ? L("Open") : L("Switch to This Repository")) { [weak self] in self?.open(path, .here) }
        add(L("Open in New Tab")) { [weak self] in self?.open(path, .newTab) }
        add(L("Open in New Window")) { [weak self] in self?.open(path, .newWindow) }
        if !path.hasPrefix("daemon:"), !Settings.openThroughDaemon {
            let item = ClosureMenuItem(title: L("Open Through the Shared daemon")) { [weak self] in self?.onOpen?("daemon:" + path, .here) }
            item.toolTip = L("Share History, the search index and the operation log with the CLI and others")
            menu.addItem(item)
        }
        menu.addItem(.separator())
        add(targets.count > 1 ? L("Fetch (%ld)", targets.count) : "Fetch") { Bookmarks.shared.fetch(targets) }
        menu.addItem(.separator())
        add(bookmark?.favorite == true ? L("Remove from Favorites") : L("Add to Favorites")) { Bookmarks.shared.toggleFavorite(path) }
        let groups = NSMenu(title: L("Group"))
        let none = ClosureMenuItem(title: L("None")) { for p in targets { Bookmarks.shared.move(p, to: nil) } }
        none.state = bookmark?.group == nil ? .on : .off
        groups.addItem(none)
        for group in Bookmarks.shared.data.groups {
            let item = ClosureMenuItem(title: group) { for p in targets { Bookmarks.shared.move(p, to: group) } }
            item.state = bookmark?.group == group ? .on : .off
            groups.addItem(item)
        }
        groups.addItem(.separator())
        groups.addItem(ClosureMenuItem(title: L("New Group…")) { [weak self] in
            guard let name = self?.ask(L("New Group"), message: L("Group name"), initial: "") else { return }
            Bookmarks.shared.addGroup(name)
            for p in targets { Bookmarks.shared.move(p, to: name) }
        })
        let groupItem = NSMenuItem(title: L("Group"), action: nil, keyEquivalent: "")
        groupItem.submenu = groups
        menu.addItem(groupItem)
        let colors = NSMenu(title: L("Color Label"))
        let noColor = ClosureMenuItem(title: L("None")) { for p in targets { Bookmarks.shared.setColor(p, nil) } }
        noColor.state = bookmark?.color == nil ? .on : .off
        colors.addItem(noColor)
        for entry in Bookmarks.colors {
            let item = ClosureMenuItem(title: entry.title) { for p in targets { Bookmarks.shared.setColor(p, entry.name) } }
            item.image = RepositoryCell.dot(entry.color)
            item.state = bookmark?.color == entry.name ? .on : .off
            colors.addItem(item)
        }
        let colorItem = NSMenuItem(title: L("Color Label"), action: nil, keyEquivalent: "")
        colorItem.submenu = colors
        menu.addItem(colorItem)
        add(L("Rename…")) { [weak self] in
            let current = Bookmarks.shared.displayName(path)
            let name = self?.ask(L("Rename"), message: L("Name shown in the list (leave empty for the folder name)"), initial: current)
            Bookmarks.shared.rename(path, to: name)
        }
        if !RepositoryLocation.isRemote(path) {
            menu.addItem(.separator())
            add(L("Show in Finder")) { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
            add(L("Open in Terminal")) {
                guard let terminal = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.Terminal") else { return }
                NSWorkspace.shared.open(
                    [URL(fileURLWithPath: path)], withApplicationAt: terminal, configuration: NSWorkspace.OpenConfiguration())
            }
        }
        if style == .page, let title = removeTitle {
            menu.addItem(.separator())
            add(title) { [weak self] in self?.remove(targets) }
        }
    }

    // MARK: - NSTableView

    func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat { 40 }

    // MARK: - 左の木（NSOutlineView）

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        (item as? SideNode)?.children.count ?? sideRoots.count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        (item as? SideNode)?.children[index] ?? sideRoots[index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        !((item as? SideNode)?.children.isEmpty ?? true)
    }

    func outlineView(_ outlineView: NSOutlineView, isGroupItem item: Any) -> Bool {
        (item as? SideNode)?.category == nil
    }

    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool {
        (item as? SideNode)?.category != nil
    }

    func outlineView(_ outlineView: NSOutlineView, heightOfRowByItem item: Any) -> CGFloat { 26 }

    func outlineViewItemDidExpand(_ notification: Notification) { rememberExpansion(notification, true) }
    func outlineViewItemDidCollapse(_ notification: Notification) { rememberExpansion(notification, false) }

    private func rememberExpansion(_ notification: Notification, _ open: Bool) {
        guard !rebuildingSidebar, let node = notification.userInfo?["NSObject"] as? SideNode else { return }
        if open { expanded.insert(node.key) } else { expanded.remove(node.key) }
        UserDefaults.standard.set(Array(expanded), forKey: Self.expandedKey)
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard !rebuildingSidebar, let chosen = (sidebar.item(atRow: sidebar.selectedRow) as? SideNode)?.category, chosen != category
        else { return }
        category = chosen
        reload()
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? SideNode else { return nil }
        guard let category = node.category else {
            let field = outlineView.makeView(withIdentifier: .init("header"), owner: nil) as? NSTextField ?? {
                let f = NSTextField(labelWithString: "")
                f.identifier = .init("header")
                f.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
                f.textColor = .secondaryLabelColor
                return f
            }()
            field.stringValue = node.title
            return field
        }
        let cell = outlineView.makeView(withIdentifier: .init("category"), owner: nil) as? CategoryCell ?? CategoryCell()
        var count = "\(paths(in: category).count)"
        if case let .scan(root) = category {
            if Bookmarks.shared.isScanning { count = "…" } else if Bookmarks.shared.scanErrors[root] != nil { count = "⚠︎" }
        }
        cell.show(title: node.title, symbol: node.symbol, count: count)
        // 狭いと名前の途中が省かれるので、フォルダはフルパスをツールチップに出す。
        let tilde = { (path: String) in (path as NSString).abbreviatingWithTildeInPath }
        switch category {
        case let .folder(path): cell.toolTip = tilde(path)
        case let .scan(root): cell.toolTip = L("Searches %@", (Self.sshHost(root).map { $0 + ":" } ?? "") + tilde(Self.pathPart(root)))
        case let .scanFolder(root, path): cell.toolTip = "\(Self.sshHost(root).map { $0 + ":" } ?? "")\(tilde(path))"
        case let .hostFolder(host, path): cell.toolTip = "\(host):\(Self.remoteTitle(path))"
        default: cell.toolTip = nil
        }
        return cell
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row < shown.count else { return nil }
        let path = shown[row]
        let column = tableColumn?.identifier.rawValue ?? "name"
        if column == "name" {
            let id = NSUserInterfaceItemIdentifier(style == .page ? "repo-page" : "repo")
            let cell = tableView.makeView(withIdentifier: id, owner: nil) as? RepositoryCell ?? RepositoryCell(details: style == .compact)
            cell.identifier = id
            cell.show(path)
            return cell
        }
        let cell = tableView.makeView(withIdentifier: .init(column), owner: nil) as? DetailCell ?? DetailCell(column)
        cell.show(column: column, location: path)
        return cell
    }

    /// 左の分類の行: 印、名前、右に件数。
    private final class CategoryCell: NSTableCellView {
        let icon = NSImageView()
        let title = NSTextField(labelWithString: "")
        let count = NSTextField(labelWithString: "")

        init() {
            super.init(frame: .zero)
            identifier = .init("category")
            title.lineBreakMode = .byTruncatingMiddle
            count.textColor = .secondaryLabelColor
            count.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
            for v in [icon, title, count] {
                v.translatesAutoresizingMaskIntoConstraints = false
                addSubview(v)
            }
            title.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            NSLayoutConstraint.activate([
                icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
                icon.centerYAnchor.constraint(equalTo: centerYAnchor),
                icon.widthAnchor.constraint(equalToConstant: 16),
                title.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 6),
                title.centerYAnchor.constraint(equalTo: centerYAnchor),
                title.trailingAnchor.constraint(lessThanOrEqualTo: count.leadingAnchor, constant: -6),
                count.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
                count.centerYAnchor.constraint(equalTo: centerYAnchor),
            ])
        }

        required init?(coder: NSCoder) { fatalError() }

        func show(title text: String, symbol: String, count number: String) {
            icon.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
            title.stringValue = text
            count.stringValue = number
        }
    }

    /// ページの列（ブランチ・状態・最後のコミット・開いた日時）。
    private final class DetailCell: NSTableCellView {
        let label = NSTextField(labelWithString: "")

        init(_ column: String) {
            super.init(frame: .zero)
            identifier = .init(column)
            label.font = column == "status"
                ? .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .medium)
                : .systemFont(ofSize: NSFont.smallSystemFontSize)
            label.lineBreakMode = .byTruncatingTail
            label.translatesAutoresizingMaskIntoConstraints = false
            addSubview(label)
            textField = label
            NSLayoutConstraint.activate([
                label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
                label.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -4),
                label.centerYAnchor.constraint(equalTo: centerYAnchor),
            ])
        }

        required init?(coder: NSCoder) { fatalError() }

        private static let relative: RelativeDateTimeFormatter = {
            let f = RelativeDateTimeFormatter()
            f.unitsStyle = .short
            return f
        }()

        func show(column: String, location: String) {
            let remote = RepositoryLocation.isRemote(location)
            let exists = RepositoryList.isRepository(location)
            let peek = exists && !remote ? Bookmarks.shared.peek(location) : nil
            label.textColor = .secondaryLabelColor
            toolTip = nil
            switch column {
            case "branch":
                label.stringValue = remote ? "—"
                    : exists ? peek.map { $0.branch ?? "detached" } ?? RepositoryList.branch(of: location) ?? "" : ""
                label.textColor = .labelColor
            case "status":
                if remote {
                    // SSH 先・daemon 越しは、開くまで状態を読まない。
                    label.stringValue = RepositoryBrowser.sshHost(location) != nil ? "SSH" : "daemon"
                    toolTip = L("The status isn’t read until you open it")
                    break
                }
                var parts: [String] = []
                if let peek {
                    if peek.changed > 0 { parts.append(L("Changes %ld", peek.changed)) }
                    if peek.ahead > 0 { parts.append("↑\(peek.ahead)") }
                    if peek.behind > 0 { parts.append("↓\(peek.behind)") }
                    if parts.isEmpty { parts.append("✓") }
                }
                let fetchError = Bookmarks.shared.fetchErrors[location]
                if Bookmarks.shared.fetching.contains(location) { parts.insert(L("Fetching…"), at: 0) }
                if fetchError != nil { parts.insert(L("Fetch failed"), at: 0) }
                if !exists { parts = [L("Not found")] }
                label.stringValue = parts.joined(separator: "  ")
                label.textColor = fetchError != nil || !exists ? .systemRed : peek?.changed ?? 0 > 0 ? .systemOrange : .secondaryLabelColor
                toolTip = fetchError.map { L("Fetch failed: %@", $0) } ?? peek.map {
                    [
                        $0.changed > 0 ? L("%ld files with changes (untracked files aren’t counted)", $0.changed) : L("No changes"),
                        $0.hasUpstream ? L("%ld ahead of the upstream, %ld behind", $0.ahead, $0.behind) : L("No upstream"),
                    ].joined(separator: "\n")
                }
            case "commit":
                if let peek, let date = peek.lastCommitDate {
                    label.stringValue = Self.relative.localizedString(for: date, relativeTo: Date()) + " — " + (peek.lastCommitSubject ?? "")
                    toolTip = (peek.lastCommitSubject ?? "") + "\n" + date.formatted(date: .abbreviated, time: .shortened)
                } else {
                    label.stringValue = remote ? "—" : ""
                }
            case "opened":
                label.stringValue = RepositoryList.openedAt(location).map { Self.relative.localizedString(for: $0, relativeTo: Date()) } ?? "—"
            default:
                label.stringValue = ""
            }
        }
    }
}

/// 一覧の行: 名前、その下にパス、右に今のブランチと状態（変更の数・上流との差）。
final class RepositoryCell: NSTableCellView {
    let name = NSTextField(labelWithString: "")
    let path = NSTextField(labelWithString: "")
    let branch = NSTextField(labelWithString: "")
    let state = NSTextField(labelWithString: "")

    /// `details` が false なら、ブランチと状態を出さない（ページでは列に分けて出す）。
    init(details: Bool = true) {
        super.init(frame: .zero)
        identifier = .init("repo")
        branch.isHidden = !details
        state.isHidden = !details
        name.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        path.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        path.textColor = .secondaryLabelColor
        path.lineBreakMode = .byTruncatingMiddle
        branch.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        branch.textColor = .secondaryLabelColor
        branch.alignment = .right
        state.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .medium)
        state.alignment = .right
        for v in [name, path, branch, state] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        path.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        branch.setContentCompressionResistancePriority(.required, for: .horizontal)
        state.setContentCompressionResistancePriority(.required, for: .horizontal)
        NSLayoutConstraint.activate([
            name.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            name.topAnchor.constraint(equalTo: topAnchor, constant: 3),
            path.leadingAnchor.constraint(equalTo: name.leadingAnchor),
            path.topAnchor.constraint(equalTo: name.bottomAnchor, constant: 1),
            path.trailingAnchor.constraint(lessThanOrEqualTo: state.leadingAnchor, constant: -8),
            branch.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            branch.topAnchor.constraint(equalTo: topAnchor, constant: 4),
            state.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            state.topAnchor.constraint(equalTo: branch.bottomAnchor, constant: 1),
            name.trailingAnchor.constraint(lessThanOrEqualTo: branch.leadingAnchor, constant: -8),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    /// `location` の名前・パス・ブランチ・状態を出す。状態はまだ読んでいなければ空（読めたら一覧ごと描き直す）。
    func show(_ location: String) {
        let exists = RepositoryList.isRepository(location)
        let bookmark = Bookmarks.shared.bookmark(location)
        let title = NSMutableAttributedString()
        if let color = Bookmarks.color(named: bookmark?.color), let dot = Self.dot(color) {
            let attachment = NSTextAttachment()
            attachment.image = dot
            attachment.bounds = NSRect(x: 0, y: 0, width: 10, height: 10)
            title.append(NSAttributedString(attachment: attachment))
            title.append(NSAttributedString(string: " "))
        }
        title.append(NSAttributedString(string: (bookmark?.favorite == true ? "★ " : "") + Bookmarks.shared.displayName(location)))
        title.addAttributes(
            [.font: NSFont.boldSystemFont(ofSize: NSFont.systemFontSize),
             .foregroundColor: exists ? NSColor.labelColor : NSColor.tertiaryLabelColor],
            range: NSRange(location: 0, length: title.length))
        name.attributedStringValue = title
        path.stringValue = RepositoryLocation.isRemote(location) ? location
            : exists ? (location as NSString).abbreviatingWithTildeInPath : L("Not found — %@", location)
        let peek = exists ? Bookmarks.shared.peek(location) : nil
        branch.stringValue = exists ? peek.map { $0.branch ?? "detached" } ?? RepositoryList.branch(of: location) ?? "" : ""
        var parts: [String] = []
        if let peek {
            if peek.changed > 0 { parts.append(L("Changes %ld", peek.changed)) }
            if peek.ahead > 0 { parts.append("↑\(peek.ahead)") }
            if peek.behind > 0 { parts.append("↓\(peek.behind)") }
        }
        let fetching = Bookmarks.shared.fetching.contains(location)
        let fetchError = Bookmarks.shared.fetchErrors[location]
        if fetching { parts.insert(L("Fetching…"), at: 0) }
        if fetchError != nil { parts.insert(L("Fetch failed"), at: 0) }
        state.stringValue = parts.joined(separator: "  ")
        state.textColor = fetchError != nil ? .systemRed : peek?.changed ?? 0 > 0 ? .systemOrange : .secondaryLabelColor
        toolTip = peek.map {
            [
                $0.changed > 0 ? L("%ld files with changes (untracked files aren’t counted)", $0.changed) : L("No changes"),
                $0.hasUpstream ? L("%ld ahead of the upstream, %ld behind", $0.ahead, $0.behind) : L("No upstream"),
            ].joined(separator: "\n")
        }
        if let fetchError { toolTip = L("Fetch failed: %@", fetchError) + (toolTip.map { "\n" + $0 } ?? "") }
    }

    /// 色ラベルの丸。
    static func dot(_ color: NSColor) -> NSImage? {
        NSImage(size: NSSize(width: 10, height: 10), flipped: false) { rect in
            color.setFill()
            NSBezierPath(ovalIn: rect.insetBy(dx: 0.5, dy: 0.5)).fill()
            return true
        }
    }
}

/// SSH のホストのホームディレクトリ。`ssh <host> 'printf %s "$HOME"'` で一度だけ問い合わせ、覚えておく
/// （パスワードを聞かれるホストでは問い合わせない: BatchMode）。分かったらリポジトリの一覧を作り直す。
enum RemoteHome {
    private static let key = "sshHome"
    nonisolated(unsafe) private static var asked: Set<String> = []

    static func known(_ host: String) -> String? {
        (UserDefaults.standard.dictionary(forKey: key) as? [String: String])?[host]
    }

    static func learn(_ host: String) {
        guard known(host) == nil, !asked.contains(host) else { return }
        asked.insert(host)
        Task.detached(priority: .utility) {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
            process.arguments = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, "printf %s \"$HOME\""]
            let out = Pipe()
            process.standardOutput = out
            process.standardError = FileHandle.nullDevice
            process.standardInput = FileHandle.nullDevice
            guard (try? process.run()) != nil else { return }
            let data = out.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            let home = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            guard process.terminationStatus == 0, home.hasPrefix("/"), home.count > 1 else { return }
            await MainActor.run {
                var all = UserDefaults.standard.dictionary(forKey: key) as? [String: String] ?? [:]
                all[host] = home
                UserDefaults.standard.set(all, forKey: key)
                RepositoryBrowser.bookmarksChanged()
            }
        }
    }
}
