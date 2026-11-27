import AmonhenKit
import AppKit

/// Working Copy の「すべてのファイル」: 作業ツリーのすべてのファイル（ツリー・リスト・ファイル名）。
/// HEAD の木に status を重ねる（足したファイル・untracked を足し、変えたファイルに印、消したファイルは赤の取り消し線）。
/// 無視しているファイルは出さない。フォルダの中は開いたときに読む。
final class WorkingTreeFiles: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate {
    final class Node {
        let name: String
        let path: String
        let isDirectory: Bool
        /// 読んだ子（フォルダで、まだ読んでいなければ nil）。
        var children: [Node]?
        /// 変更の印（変えていなければ nil）。
        var mark: (String, NSColor)?
        var deleted = false
        var containsChanges = false
        init(name: String, path: String, isDirectory: Bool) {
            self.name = name
            self.path = path
            self.isDirectory = isDirectory
        }
    }

    /// 一覧の見せ方（Working Copy の変更の一覧と同じ 3 つ）。
    enum Layout { case list, tree, flat }

    let outline = KeyOutlineView()
    let scroll = NSScrollView()
    private let repository: Repository
    /// HEAD（40 桁）。まだコミットがなければ nil。
    private var head: String?
    private var root = Node(name: "", path: "", isDirectory: true)
    /// 変えたファイル（パス → 印・消したか）と、変更を含むフォルダ。
    private var changed: [String: (mark: (String, NSColor), deleted: Bool)] = [:]
    private var changedFolders: Set<String> = []
    /// 開いたファイルを差分に出す。
    var onOpen: ((String) -> Void)?
    var layout: Layout = .tree {
        didSet {
            guard layout != oldValue else { return }
            reloadKeepingState()
        }
    }

    init(repository: Repository) {
        self.repository = repository
        super.init()
        let column = NSTableColumn(identifier: .init("name"))
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.headerView = nil
        outline.style = .sourceList
        outline.dataSource = self
        outline.delegate = self
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
    }

    /// 選んでいるファイル。
    var selectedPath: String? {
        (outline.item(atRow: outline.selectedRow) as? Node).flatMap { $0.isDirectory ? nil : $0.path }
    }

    /// HEAD と status から作り直す（開いていたフォルダと選んでいたファイルは保つ）。
    func reload(head: String?, items: [StatusItem], mark: (StatusItem) -> (String, NSColor)) {
        self.head = head
        changed = [:]
        changedFolders = [""]
        for item in items {
            for path in [item.path] + (item.origPath.map { [$0] } ?? []) {
                var folder = (path as NSString).deletingLastPathComponent
                while !folder.isEmpty {
                    changedFolders.insert(folder)
                    folder = (folder as NSString).deletingLastPathComponent
                }
            }
            let deleted = item.worktreeState == "D" || (item.indexState == "D" && item.worktreeState == ".")
            changed[item.path] = (mark(item), deleted)
            // 名前を変えたファイルの元の名前は、作業ツリーにはもうない（消したファイルと同じく赤の取り消し線）。
            if let orig = item.origPath, changed[orig] == nil { changed[orig] = (("D", .systemRed), true) }
            var folder = (item.path as NSString).deletingLastPathComponent
            while !folder.isEmpty {
                changedFolders.insert(folder)
                folder = (folder as NSString).deletingLastPathComponent
            }
        }
        reloadKeepingState()
    }

    private func reloadKeepingState() {
        let expanded = expandedPaths()
        let selected = selectedPath
        root = Node(name: "", path: "", isDirectory: true)
        flatCache = nil
        if layout != .tree { loadAll() }
        outline.reloadData()
        if layout == .tree {
            load(root)
            if expanded.isEmpty { expandChanged(root) } else { expand(expanded, in: root) }
        }
        if let selected {
            let row = (0..<outline.numberOfRows).first { (outline.item(atRow: $0) as? Node)?.path == selected }
            if let row { outline.selectRowIndexes([row], byExtendingSelection: false) }
        }
    }

    private func expandedPaths() -> Set<String> {
        var out: Set<String> = []
        for row in 0..<outline.numberOfRows {
            if let node = outline.item(atRow: row) as? Node, node.isDirectory, outline.isItemExpanded(node) { out.insert(node.path) }
        }
        return out
    }

    private func expand(_ paths: Set<String>, in node: Node) {
        for child in node.children ?? [] where child.isDirectory && paths.contains(child.path) {
            load(child)
            outline.expandItem(child)
            expand(paths, in: child)
        }
    }

    /// フォルダの中を読む（まだなら）。HEAD の木に、HEAD にない変更のファイル・フォルダを足す。
    private func load(_ node: Node) {
        guard node.isDirectory, node.children == nil else { return }
        var children: [Node] = []
        if let head {
            for entry in (try? repository.treeEntries(commit: head, path: node.path)) ?? [] {
                let path = node.path.isEmpty ? entry.name : node.path + "/" + entry.name
                children.append(Node(name: entry.name, path: path, isDirectory: entry.isFolder))
            }
        }
        var names = Set(children.map(\.name))
        for path in changed.keys where (path as NSString).deletingLastPathComponent == node.path {
            let name = (path as NSString).lastPathComponent
            if names.insert(name).inserted { children.append(Node(name: name, path: path, isDirectory: false)) }
        }
        for folder in changedFolders where !folder.isEmpty && (folder as NSString).deletingLastPathComponent == node.path {
            let name = (folder as NSString).lastPathComponent
            if names.insert(name).inserted { children.append(Node(name: name, path: folder, isDirectory: true)) }
        }
        for child in children {
            if let change = changed[child.path], !child.isDirectory {
                child.mark = change.mark
                child.deleted = change.deleted
            }
            child.containsChanges = child.isDirectory ? changedFolders.contains(child.path) : changed[child.path] != nil
        }
        children.sort { ($0.isDirectory ? 0 : 1, $0.name.lowercased()) < ($1.isDirectory ? 0 : 1, $1.name.lowercased()) }
        node.children = children
    }

    /// リストのために、すべてのフォルダを読む。
    private func loadAll() {
        var stack = [root]
        while let node = stack.popLast() {
            load(node)
            stack.append(contentsOf: (node.children ?? []).filter(\.isDirectory))
        }
    }

    private var flatCache: [Node]?
    private var flatFiles: [Node] {
        if let flatCache { return flatCache }
        var out: [Node] = []
        var stack = [root]
        while let node = stack.popLast() {
            for child in node.children ?? [] { if child.isDirectory { stack.append(child) } else { out.append(child) } }
        }
        let sorted = layout == .flat
            ? out.sorted { ($0.name.lowercased(), $0.path.lowercased()) < ($1.name.lowercased(), $1.path.lowercased()) }
            : out.sorted { $0.path.lowercased() < $1.path.lowercased() }
        flatCache = sorted
        return sorted
    }

    private func expandChanged(_ node: Node) {
        for child in node.children ?? [] where child.isDirectory && child.containsChanges {
            load(child)
            outline.reloadItem(child, reloadChildren: true)
            outline.expandItem(child)
            expandChanged(child)
        }
    }

    /// `path` のファイルを選ぶ（ツリーなら親のフォルダを開く）。
    func select(path: String) {
        if layout == .tree {
            var node = root
            for part in path.split(separator: "/").dropLast() {
                load(node)
                guard let next = node.children?.first(where: { $0.name == part && $0.isDirectory }) else { return }
                load(next)
                outline.expandItem(next)
                node = next
            }
        }
        let row = (0..<outline.numberOfRows).first { (outline.item(atRow: $0) as? Node)?.path == path }
        guard let row else { return }
        outline.selectRowIndexes([row], byExtendingSelection: false)
        outline.scrollRowToVisible(row)
    }

    /// 開いているフォルダがあるか。
    var anyFolderExpanded: Bool {
        layout == .tree && (root.children ?? []).contains { $0.isDirectory && outline.isItemExpanded($0) }
    }

    /// すべてのフォルダを開く・畳む（開くときは、中を読む）。
    func setAllFolders(expanded: Bool) {
        if expanded {
            loadAll()
            outline.reloadData()
            outline.expandItem(nil, expandChildren: true)
        } else {
            outline.collapseItem(nil, collapseChildren: true)
        }
    }

    // MARK: - NSOutlineView

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        if layout != .tree { return item == nil ? flatFiles.count : 0 }
        let node = (item as? Node) ?? root
        load(node)
        return node.children?.count ?? 0
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        if layout != .tree { return flatFiles[index] }
        return ((item as? Node) ?? root).children![index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        layout == .tree && (item as? Node)?.isDirectory == true
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        if let path = selectedPath { onOpen?(path) }
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? Node else { return nil }
        let cell = NSTableCellView()
        let icon = NSImageView(image: AppIcons.image(symbol: node.isDirectory ? "folder" : "doc")!)
        icon.contentTintColor = node.isDirectory ? .secondaryLabelColor : .tertiaryLabelColor
        var nameAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 13),
            .foregroundColor: node.containsChanges ? NSColor.labelColor : NSColor.secondaryLabelColor,
        ]
        if node.deleted {
            nameAttributes[.foregroundColor] = NSColor.systemRed
            nameAttributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        }
        let text = NSMutableAttributedString()
        let folder = (node.path as NSString).deletingLastPathComponent
        let dim: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.tertiaryLabelColor]
        if layout == .list, !folder.isEmpty { text.append(NSAttributedString(string: folder + "/", attributes: dim)) }
        text.append(NSAttributedString(string: node.name, attributes: nameAttributes))
        if layout == .flat, !folder.isEmpty { text.append(NSAttributedString(string: "  " + folder, attributes: dim)) }
        if let (mark, color) = node.mark {
            text.append(NSAttributedString(string: "  " + mark, attributes: [
                .foregroundColor: color, .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .semibold),
            ]))
        } else if node.isDirectory, node.containsChanges {
            text.append(NSAttributedString(string: "  ●", attributes: [
                .foregroundColor: NSColor.systemOrange, .font: NSFont.systemFont(ofSize: 7),
            ]))
        }
        let label = NSTextField(labelWithAttributedString: text)
        label.lineBreakMode = layout == .list ? .byTruncatingHead : .byTruncatingTail
        label.usesSingleLineMode = true
        label.maximumNumberOfLines = 1
        label.cell?.truncatesLastVisibleLine = true
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        label.toolTip = node.path
        let stack = NSStackView(views: [icon, label])
        stack.spacing = 5
        stack.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: cell.trailingAnchor),
            stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }
}
