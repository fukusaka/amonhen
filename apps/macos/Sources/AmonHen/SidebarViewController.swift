import AmonhenKit
import AppKit

/// サイドバー: Branches / Remotes / Tags / Stashes / Submodules。
final class SidebarViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate {
    /// 項目を選んだとき（指すコミットがあれば History でその行を選ぶ）。
    var onSelect: ((RefItem) -> Void)?

    private final class Node {
        let title: String
        let item: RefItem?
        var children: [Node]
        let isHead: Bool

        init(title: String, item: RefItem? = nil, children: [Node] = [], isHead: Bool = false) {
            self.title = title
            self.item = item
            self.children = children
            self.isHead = isHead
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
        outline.allowsTypeSelect = true

        let scroll = NSScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        view = scroll
    }

    func show(_ refs: Refs) {
        guard refs.generation != generation else { return }
        generation = refs.generation
        let expanded = Set(groups.filter { outline.isItemExpanded($0) }.map(\.title))
        let first = groups.isEmpty

        func items(_ kind: RefKind) -> [RefItem] { refs.items.filter { $0.kind == kind } }
        var groups: [Node] = []
        let branches = items(.local).map {
            Node(title: $0.name, item: $0, isHead: $0.name == refs.headBranch)
        }
        if !branches.isEmpty { groups.append(Node(title: "Branches", children: branches)) }

        // リモートごとにまとめる（origin/main → origin の下の main）。
        var remotes: [String: [Node]] = [:]
        for item in items(.remote) {
            let parts = item.name.split(separator: "/", maxSplits: 1).map(String.init)
            remotes[parts[0], default: []].append(Node(title: parts.count > 1 ? parts[1] : item.name, item: item))
        }
        if !remotes.isEmpty {
            groups.append(Node(title: "Remotes", children: remotes.keys.sorted().map {
                Node(title: $0, children: remotes[$0]!)
            }))
        }
        for (kind, title) in [(RefKind.tag, "Tags"), (.stash, "Stashes"), (.submodule, "Submodules")] {
            let nodes = items(kind).map { Node(title: $0.name, item: $0) }
            if !nodes.isEmpty { groups.append(Node(title: title, children: nodes)) }
        }

        self.groups = groups
        outline.reloadData()
        for group in groups where first ? group.title == "Branches" : expanded.contains(group.title) {
            outline.expandItem(group)
        }
    }

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
        (item as? Node)?.item != nil
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? Node else { return nil }
        let isGroup = groups.contains { $0 === node }
        let id = NSUserInterfaceItemIdentifier(isGroup ? "group" : "item")
        let cell = outlineView.makeView(withIdentifier: id, owner: nil) as? NSTableCellView ?? {
            let cell = NSTableCellView()
            cell.identifier = id
            let field = NSTextField(labelWithString: "")
            field.lineBreakMode = .byTruncatingTail
            field.translatesAutoresizingMaskIntoConstraints = false
            cell.addSubview(field)
            cell.textField = field
            NSLayoutConstraint.activate([
                field.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
                field.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -2),
                field.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
            ])
            return cell
        }()
        cell.textField?.stringValue = node.isHead ? "● \(node.title)" : node.title
        cell.textField?.font = node.isHead ? .boldSystemFont(ofSize: NSFont.systemFontSize) : .systemFont(ofSize: NSFont.systemFontSize)
        cell.toolTip = node.item?.detail
        return cell
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard let node = outline.item(atRow: outline.selectedRow) as? Node, let item = node.item else { return }
        onSelect?(item)
    }
}
