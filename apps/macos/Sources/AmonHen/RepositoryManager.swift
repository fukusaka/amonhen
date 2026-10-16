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

    /// 開いたリポジトリを先頭に置く。
    static func touch(_ path: String) {
        paths = [path] + paths.filter { $0 != path }
    }

    static func remove(_ path: String) {
        paths = paths.filter { $0 != path }
    }

    /// `path` が Git のリポジトリ（作業ツリーの直下に `.git` がある）か。daemon 越しの場所は、つなぐまで
    /// わからないので、あるものとする。
    static func isRepository(_ path: String) -> Bool {
        if RepositoryLocation.isRemote(path) { return true }
        return FileManager.default.fileExists(atPath: (path as NSString).appendingPathComponent(".git"))
    }

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

/// リポジトリの一覧: お気に入り・最近開いたもの・ブックマーク（手で作ったグループ）・スキャンするフォルダ。
/// 開く・加える・外す・グループに分ける・色ラベル・ドラッグで並べ替える・まとめて Fetch。行には今のブランチ・
/// 変更の有無・上流との差を出す（見えている行だけ裏で読む）。
final class RepositoryManagerWindowController: NSWindowController, NSOutlineViewDataSource, NSOutlineViewDelegate,
    NSSearchFieldDelegate, NSMenuDelegate
{
    static let shared = RepositoryManagerWindowController()

    /// リポジトリを開く（パス、新しいウィンドウにするか）。AppDelegate が設定する。
    var onOpen: ((String, Bool) -> Void)?

    /// 一覧の見出し。
    private final class Section {
        enum Kind: Equatable { case favorites, recent, bookmarks, group(String), scan(String) }
        let kind: Kind
        var paths: [String] = []
        init(_ kind: Kind) { self.kind = kind }

        var title: String {
            switch kind {
            case .favorites: "Favorites"
            case .recent: "Recent"
            case .bookmarks: "Bookmarks"
            case let .group(name): name
            case let .scan(path): (path as NSString).abbreviatingWithTildeInPath
            }
        }
    }

    /// 行（同じリポジトリが複数の見出しに出るので、見出しと組にする）。
    private final class Row {
        let path: String
        unowned let section: Section
        init(_ path: String, in section: Section) {
            self.path = path
            self.section = section
        }
    }

    private let outline = NSOutlineView()
    private let filter = NSSearchField()
    private let scanning = NSProgressIndicator()
    private var sections: [Section] = []
    private var rows: [ObjectIdentifier: [Row]] = [:]

    private init() {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 640, height: 560),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "Repositories"
        window.setFrameAutosaveName("Repositories")
        super.init(window: window)
        build()
        reload()
    }

    required init?(coder: NSCoder) { fatalError() }

    func show() {
        Bookmarks.shared.forgetPeeks()
        Bookmarks.shared.rescan()
        reload()
        if window?.isVisible != true { window?.center() }
        showWindow(nil)
        window?.makeFirstResponder(outline)
    }

    /// ブックマーク・スキャンの結果・行の状態が変わった。
    func bookmarksChanged() {
        guard window?.isVisible == true else { return }
        scanning.isHidden = !Bookmarks.shared.isScanning
        if Bookmarks.shared.isScanning { scanning.startAnimation(nil) } else { scanning.stopAnimation(nil) }
        reload()
    }

    private func build() {
        guard let content = window?.contentView else { return }
        let column = NSTableColumn(identifier: .init("repository"))
        column.resizingMask = .autoresizingMask
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.headerView = nil
        outline.rowHeight = 40
        outline.style = .sourceList
        outline.floatsGroupRows = false
        outline.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        outline.dataSource = self
        outline.delegate = self
        outline.target = self
        outline.doubleAction = #selector(openSelected)
        outline.autosaveName = "RepositorySections"
        outline.autosaveExpandedItems = true
        outline.allowsMultipleSelection = true
        outline.registerForDraggedTypes([Self.dragType])
        outline.setDraggingSourceOperationMask(.move, forLocal: true)
        let menu = NSMenu()
        menu.delegate = self
        outline.menu = menu
        let scroll = NSScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true

        filter.placeholderString = "名前・パスで絞り込む"
        filter.delegate = self
        filter.target = self
        filter.action = #selector(filterChanged)
        scanning.style = .spinning
        scanning.controlSize = .small
        scanning.isHidden = true
        let add = NSPopUpButton(frame: .zero, pullsDown: true)
        add.addItem(withTitle: "Add")
        for (title, action) in [
            ("Add Repository…", #selector(addRepository)), ("Add Remote Repository…", #selector(addRemote)),
            ("Add Folder to Scan…", #selector(addScanFolder)), ("New Group…", #selector(newGroup)),
        ] {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = self
            add.menu?.addItem(item)
        }
        let rescan = NSButton(title: "Rescan", target: self, action: #selector(rescanFolders))
        rescan.toolTip = "スキャンするフォルダを探し直す"
        let fetch = NSButton(title: "Fetch", target: self, action: #selector(fetchSelected))
        fetch.toolTip = "選んだリポジトリ（見出しを選べばその中のすべて）をまとめて fetch する"
        let remove = NSButton(title: "Remove", target: self, action: #selector(removeSelected))
        remove.toolTip = "一覧から外す（フォルダには触れない）"
        let open = NSButton(title: "Open", target: self, action: #selector(openSelected))
        open.keyEquivalent = "\r"
        open.toolTip = "⏎ タブで開く　⌘⏎ 新しいウィンドウで開く"
        let buttons = NSStackView(views: [add, rescan, fetch, remove, scanning, NSView(), open])

        for v in [filter, scroll, buttons] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            content.addSubview(v)
        }
        NSLayoutConstraint.activate([
            filter.topAnchor.constraint(equalTo: content.topAnchor, constant: 12),
            filter.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            filter.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: filter.bottomAnchor, constant: 8),
            scroll.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            buttons.topAnchor.constraint(equalTo: scroll.bottomAnchor, constant: 8),
            buttons.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            buttons.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            buttons.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -12),
        ])
    }

    /// 見出しと行を作り直す。選んでいた行と、開いていた見出しは保つ。
    private func reload() {
        let selectedPath = (outline.item(atRow: outline.selectedRow) as? Row)?.path
        let query = filter.stringValue.trimmingCharacters(in: .whitespaces).lowercased()
        let bookmarks = Bookmarks.shared
        let matches = { (path: String) in
            query.isEmpty || bookmarks.displayName(path).lowercased().contains(query) || path.lowercased().contains(query)
        }
        var made: [Section] = []
        func add(_ kind: Section.Kind, _ paths: [String], always: Bool = false) {
            let section = Section(kind)
            section.paths = paths.filter(matches)
            if !section.paths.isEmpty || (always && query.isEmpty) { made.append(section) }
        }
        let items = bookmarks.data.items
        add(.favorites, items.filter(\.favorite).map(\.path))
        add(.recent, Array(RepositoryList.paths.prefix(15)), always: true)
        add(.bookmarks, items.filter { $0.group == nil || !bookmarks.data.groups.contains($0.group!) }.map(\.path))
        for group in bookmarks.data.groups {
            add(.group(group), items.filter { $0.group == group }.map(\.path), always: true)
        }
        for root in bookmarks.data.scanRoots {
            add(.scan(root.path), bookmarks.scanned[root.path] ?? [], always: true)
        }
        sections = made
        rows = Dictionary(uniqueKeysWithValues: made.map { s in (ObjectIdentifier(s), s.paths.map { Row($0, in: s) }) })
        outline.reloadData()
        for section in sections where !outline.isItemExpanded(section) {
            outline.expandItem(section)
        }
        if let selectedPath, let row = sections.lazy.compactMap({ s in self.rows[ObjectIdentifier(s)]?.first { $0.path == selectedPath } }).first {
            outline.selectRowIndexes([outline.row(forItem: row)], byExtendingSelection: false)
        } else if outline.selectedRow < 0, let first = sections.lazy.compactMap({ self.rows[ObjectIdentifier($0)]?.first }).first {
            outline.selectRowIndexes([outline.row(forItem: first)], byExtendingSelection: false)
        }
    }

    @objc private func filterChanged() { reload() }

    func controlTextDidChange(_ notification: Notification) { reload() }

    // MARK: - 加える・外す

    @objc private func addRepository() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.message = "ブックマークに加えるリポジトリのフォルダを選んでください"
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
            alert.messageText = "Git のリポジトリではありません"
            alert.informativeText = rejected.joined(separator: "、")
            alert.runModal()
        }
    }

    /// 選んでいる見出しが手で作ったグループなら、その名前（加えるものをそこに入れる）。
    private var selectedGroup: String? {
        let item = outline.item(atRow: outline.selectedRow)
        let section = (item as? Section) ?? (item as? Row)?.section
        if case let .group(name)? = section?.kind { return name }
        return nil
    }

    @objc private func addScanFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.message = "リポジトリを探すフォルダを選んでください（3 段下まで探します）"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Bookmarks.shared.addScanRoot(url.standardizedFileURL.path)
    }

    @objc private func newGroup() {
        guard let name = ask("New Group", message: "グループの名前", initial: "") else { return }
        Bookmarks.shared.addGroup(name)
    }

    @objc private func rescanFolders() {
        Bookmarks.shared.forgetPeeks()
        Bookmarks.shared.rescan()
    }

    private func ask(_ title: String, message: String, initial: String) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "キャンセル")
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
        let host = NSTextField(string: UserDefaults.standard.string(forKey: Self.lastHostKey) ?? "")
        host.placeholderString = "user@host:port（~/.ssh/config の名前でもよい）"
        let path = NSTextField(string: "")
        path.placeholderString = "~/src/repo や /srv/git/repo"
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: "接続先:"), host],
            [NSTextField(labelWithString: "パス:"), path],
        ])
        grid.column(at: 0).xPlacement = .trailing
        grid.rowSpacing = 8
        grid.column(at: 1).width = 300
        grid.frame = NSRect(x: 0, y: 0, width: 370, height: 56)

        let alert = NSAlert()
        alert.messageText = "リモートのリポジトリを加える"
        alert.informativeText = "SSH で接続します。鍵か ssh-agent でパスワードなしに入れるホストにしてください。"
        alert.accessoryView = grid
        alert.addButton(withTitle: "加えて開く")
        alert.addButton(withTitle: "キャンセル")
        alert.window.initialFirstResponder = host.stringValue.isEmpty ? host : path
        alert.beginSheetModal(for: window!) { [weak self] response in
            guard let self, response == .alertFirstButtonReturn else { return }
            guard let location = RepositoryLocation.ssh(host: host.stringValue, path: path.stringValue) else {
                let error = NSAlert()
                error.messageText = "接続先とパスを入れてください"
                error.informativeText = "接続先には空白や / を含められません。"
                error.beginSheetModal(for: self.window!)
                return
            }
            UserDefaults.standard.set(host.stringValue.trimmingCharacters(in: .whitespaces), forKey: Self.lastHostKey)
            Bookmarks.shared.add(location, group: selectedGroup)
            RepositoryList.touch(location)
            onOpen?(location, false)
        }
    }

    /// 選んだ行を、その見出しから外す（最近開いたものからは履歴を、ブックマークからは登録を消す）。
    /// スキャンで見つけたものは外せない（フォルダを外す）。
    @objc private func removeSelected() {
        let item = outline.item(atRow: outline.selectedRow)
        if let row = item as? Row {
            remove(row)
        } else if let section = item as? Section {
            switch section.kind {
            case let .group(name): Bookmarks.shared.removeGroup(name)
            case let .scan(path): Bookmarks.shared.removeScanRoot(path)
            default: NSSound.beep()
            }
        }
    }

    private func remove(_ row: Row) {
        switch row.section.kind {
        case .recent:
            RepositoryList.remove(row.path)
            reload()
        case .favorites: Bookmarks.shared.toggleFavorite(row.path)
        case .bookmarks, .group: Bookmarks.shared.remove(row.path)
        case .scan: NSSound.beep()
        }
    }

    // MARK: - まとめて Fetch

    /// 選んだ行のリポジトリ（見出しなら、その中のすべて）。
    private var selectedPaths: [String] {
        var seen = Set<String>()
        return outline.selectedRowIndexes.flatMap { index -> [String] in
            switch outline.item(atRow: index) {
            case let row as Row: [row.path]
            case let section as Section: section.paths
            default: []
            }
        }.filter { seen.insert($0).inserted }
    }

    @objc private func fetchSelected() {
        let paths = selectedPaths
        guard !paths.isEmpty else { return NSSound.beep() }
        Bookmarks.shared.fetch(paths)
    }

    // MARK: - ドラッグでの並べ替え

    private static let dragType = NSPasteboard.PasteboardType("io.github.fukusaka.amonhen.repository")

    func outlineView(_ outlineView: NSOutlineView, pasteboardWriterForItem item: Any) -> NSPasteboardWriting? {
        // ブックマーク・グループ・お気に入りの行だけを動かせる（最近開いたもの・スキャンの結果は並びが決まっている）。
        guard let row = item as? Row else { return nil }
        switch row.section.kind {
        case .bookmarks, .group, .favorites: break
        default: return nil
        }
        let pb = NSPasteboardItem()
        pb.setString(row.path, forType: Self.dragType)
        return pb
    }

    func outlineView(
        _ outlineView: NSOutlineView, validateDrop info: NSDraggingInfo, proposedItem item: Any?, proposedChildIndex index: Int
    ) -> NSDragOperation {
        guard info.draggingPasteboard.string(forType: Self.dragType) != nil else { return [] }
        // 行の上に落としたら、その行の前に入れる。
        if let row = item as? Row {
            guard let section = row.section as Section?, let i = rows[ObjectIdentifier(section)]?.firstIndex(where: { $0 === row })
            else { return [] }
            outlineView.setDropItem(section, dropChildIndex: i)
            return dropTarget(section) ? .move : []
        }
        guard let section = item as? Section, dropTarget(section) else { return [] }
        return .move
    }

    /// 落とせる見出し（ブックマーク・手で作ったグループ・お気に入り）。
    private func dropTarget(_ section: Section) -> Bool {
        switch section.kind {
        case .bookmarks, .group, .favorites: true
        default: false
        }
    }

    func outlineView(_ outlineView: NSOutlineView, acceptDrop info: NSDraggingInfo, item: Any?, childIndex index: Int) -> Bool {
        guard let path = info.draggingPasteboard.string(forType: Self.dragType), let section = item as? Section else { return false }
        let list = rows[ObjectIdentifier(section)] ?? []
        // 動かす行そのものは数えずに、落とした位置の次の行を探す。
        let target = list.dropFirst(max(0, index == -1 ? list.count : index)).first { $0.path != path }?.path
        switch section.kind {
        case .bookmarks: Bookmarks.shared.reorder(path, before: target, group: nil)
        case let .group(name): Bookmarks.shared.reorder(path, before: target, group: name)
        case .favorites:
            // お気に入りの中の並びも、ブックマークの並びで決まる。グループは変えない。
            if Bookmarks.shared.bookmark(path)?.favorite != true { Bookmarks.shared.toggleFavorite(path) }
            Bookmarks.shared.reorder(path, before: target, group: Bookmarks.shared.bookmark(path)?.group)
        default: return false
        }
        return true
    }

    // MARK: - 開く

    @objc private func openSelected() {
        let row = outline.clickedRow >= 0 ? outline.clickedRow : outline.selectedRow
        guard let item = outline.item(atRow: row) as? Row else { return }
        open(item.path, newWindow: NSApp.currentEvent?.modifierFlags.contains(.command) == true)
    }

    private func open(_ path: String, newWindow: Bool) {
        guard RepositoryList.isRepository(path) else {
            let alert = NSAlert()
            alert.messageText = "\((path as NSString).lastPathComponent) が見つかりません"
            alert.informativeText = "\(path)\n最近開いたものとブックマークから外しますか？"
            alert.addButton(withTitle: "外す")
            alert.addButton(withTitle: "キャンセル")
            if alert.runModal() == .alertFirstButtonReturn {
                RepositoryList.remove(path)
                Bookmarks.shared.remove(path)
                reload()
            }
            return
        }
        onOpen?(path, newWindow)
    }

    // MARK: - 右クリック

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let item = outline.item(atRow: outline.clickedRow)
        func add(_ title: String, _ action: @escaping () -> Void) { menu.addItem(ClosureMenuItem(title: title, action: action)) }
        if let section = item as? Section {
            if !section.paths.isEmpty {
                let paths = section.paths
                add("Fetch All（\(paths.count)）") { Bookmarks.shared.fetch(paths) }
                menu.addItem(.separator())
            }
            switch section.kind {
            case let .group(name):
                add("Rename Group…") { [weak self] in
                    guard let new = self?.ask("Rename Group", message: "グループの名前", initial: name) else { return }
                    let members = Bookmarks.shared.data.items.filter { $0.group == name }.map(\.path)
                    Bookmarks.shared.addGroup(new)
                    for path in members { Bookmarks.shared.move(path, to: new) }
                    Bookmarks.shared.removeGroup(name)
                }
                add("Remove Group") { Bookmarks.shared.removeGroup(name) }
            case let .scan(path):
                let depth = Bookmarks.shared.data.scanRoots.first { $0.path == path }?.depth ?? 3
                let depths = NSMenu(title: "Depth")
                for d in 1...6 {
                    let item = ClosureMenuItem(title: "\(d)") { Bookmarks.shared.setDepth(d, of: path) }
                    item.state = d == depth ? .on : .off
                    depths.addItem(item)
                }
                let depthItem = NSMenuItem(title: "Depth", action: nil, keyEquivalent: "")
                depthItem.submenu = depths
                menu.addItem(depthItem)
                add("Rescan") { Bookmarks.shared.rescan() }
                add("Reveal in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                }
                add("Remove Folder") { Bookmarks.shared.removeScanRoot(path) }
            default:
                break
            }
            return
        }
        guard let row = item as? Row else { return }
        let path = row.path
        let bookmark = Bookmarks.shared.bookmark(path)
        add("Open in Tab") { [weak self] in self?.open(path, newWindow: false) }
        add("Open in New Window") { [weak self] in self?.open(path, newWindow: true) }
        menu.addItem(.separator())
        let targets = outline.selectedRowIndexes.contains(outline.clickedRow) ? selectedPaths : [path]
        add(targets.count > 1 ? "Fetch \(targets.count) Repositories" : "Fetch") { Bookmarks.shared.fetch(targets) }
        menu.addItem(.separator())
        add(bookmark?.favorite == true ? "Remove from Favorites" : "Add to Favorites") { Bookmarks.shared.toggleFavorite(path) }
        let colors = NSMenu(title: "Color Label")
        let noColor = ClosureMenuItem(title: "なし") { for p in targets { Bookmarks.shared.setColor(p, nil) } }
        noColor.state = bookmark?.color == nil ? .on : .off
        colors.addItem(noColor)
        for entry in Bookmarks.colors {
            let item = ClosureMenuItem(title: entry.title) { for p in targets { Bookmarks.shared.setColor(p, entry.name) } }
            item.image = RepositoryCell.dot(entry.color)
            item.state = bookmark?.color == entry.name ? .on : .off
            colors.addItem(item)
        }
        let colorItem = NSMenuItem(title: "Color Label", action: nil, keyEquivalent: "")
        colorItem.submenu = colors
        menu.addItem(colorItem)
        if bookmark == nil {
            add("Add to Bookmarks") { Bookmarks.shared.add(path) }
        }
        add("Rename…") { [weak self] in
            let current = Bookmarks.shared.displayName(path)
            let name = self?.ask("Rename", message: "一覧に出す名前（空にするとフォルダの名前）", initial: current)
            Bookmarks.shared.rename(path, to: name)
        }
        let groups = NSMenu(title: "Move to Group")
        let none = ClosureMenuItem(title: "なし") { Bookmarks.shared.move(path, to: nil) }
        none.state = bookmark != nil && bookmark?.group == nil ? .on : .off
        groups.addItem(none)
        for group in Bookmarks.shared.data.groups {
            let item = ClosureMenuItem(title: group) { Bookmarks.shared.move(path, to: group) }
            item.state = bookmark?.group == group ? .on : .off
            groups.addItem(item)
        }
        let groupItem = NSMenuItem(title: "Move to Group", action: nil, keyEquivalent: "")
        groupItem.submenu = groups
        menu.addItem(groupItem)
        if !RepositoryLocation.isRemote(path) {
            menu.addItem(.separator())
            add("Reveal in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
            add("Open in Terminal") {
                guard let terminal = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.Terminal") else { return }
                NSWorkspace.shared.open(
                    [URL(fileURLWithPath: path)], withApplicationAt: terminal, configuration: NSWorkspace.OpenConfiguration())
            }
        }
        if case .scan = row.section.kind {} else {
            menu.addItem(.separator())
            add("Remove from \(row.section.title)") { [weak self] in self?.remove(row) }
        }
    }

    // MARK: - NSOutlineView

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        switch item {
        case nil: sections.count
        case let section as Section: rows[ObjectIdentifier(section)]?.count ?? 0
        default: 0
        }
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        switch item {
        case let section as Section: rows[ObjectIdentifier(section)]![index]
        default: sections[index]
        }
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool { item is Section }
    func outlineView(_ outlineView: NSOutlineView, isGroupItem item: Any) -> Bool { item is Section }

    func outlineView(_ outlineView: NSOutlineView, heightOfRowByItem item: Any) -> CGFloat {
        item is Section ? 24 : 40
    }

    /// 見出しを開いたままにするための、保存できる名前。
    func outlineView(_ outlineView: NSOutlineView, persistentObjectForItem item: Any?) -> Any? {
        (item as? Section)?.title
    }

    func outlineView(_ outlineView: NSOutlineView, itemForPersistentObject object: Any) -> Any? {
        sections.first { $0.title == object as? String }
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        if let section = item as? Section {
            let field = outlineView.makeView(withIdentifier: .init("section"), owner: nil) as? NSTextField ?? {
                let f = NSTextField(labelWithString: "")
                f.identifier = .init("section")
                return f
            }()
            var title = section.title
            if case .scan = section.kind { title = "📁 " + title + (Bookmarks.shared.isScanning ? "（探しています…）" : "") }
            field.stringValue = title
            return field
        }
        guard let row = item as? Row else { return nil }
        let cell = outlineView.makeView(withIdentifier: .init("repo"), owner: nil) as? RepositoryCell ?? RepositoryCell()
        cell.show(row.path)
        return cell
    }
}

/// 一覧の行: 名前、その下にパス、右に今のブランチと状態（変更の数・上流との差）。
final class RepositoryCell: NSTableCellView {
    let name = NSTextField(labelWithString: "")
    let path = NSTextField(labelWithString: "")
    let branch = NSTextField(labelWithString: "")
    let state = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        identifier = .init("repo")
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
            : exists ? (location as NSString).abbreviatingWithTildeInPath : "見つかりません — \(location)"
        let peek = exists ? Bookmarks.shared.peek(location) : nil
        branch.stringValue = exists ? peek.map { $0.branch ?? "detached" } ?? RepositoryList.branch(of: location) ?? "" : ""
        var parts: [String] = []
        if let peek {
            if peek.changed > 0 { parts.append("● \(peek.changed)") }
            if peek.ahead > 0 { parts.append("↑\(peek.ahead)") }
            if peek.behind > 0 { parts.append("↓\(peek.behind)") }
        }
        let fetching = Bookmarks.shared.fetching.contains(location)
        let fetchError = Bookmarks.shared.fetchErrors[location]
        if fetching { parts.insert("Fetch 中…", at: 0) }
        if fetchError != nil { parts.insert("Fetch に失敗", at: 0) }
        state.stringValue = parts.joined(separator: "  ")
        state.textColor = fetchError != nil ? .systemRed : peek?.changed ?? 0 > 0 ? .systemOrange : .secondaryLabelColor
        toolTip = peek.map {
            [
                $0.changed > 0 ? "変更のあるファイル \($0.changed) 個（untracked は数えない）" : "変更なし",
                $0.hasUpstream ? "上流より \($0.ahead) 先、\($0.behind) 後ろ" : "上流なし",
            ].joined(separator: "\n")
        }
        if let fetchError { toolTip = "Fetch に失敗: \(fetchError)" + (toolTip.map { "\n" + $0 } ?? "") }
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
