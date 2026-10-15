import AppKit

/// 開いたことのあるリポジトリの一覧（新しく開いた順）。UserDefaults に置く。
enum RepositoryList {
    private static let key = "repositories"

    static var paths: [String] {
        get { UserDefaults.standard.stringArray(forKey: key) ?? [] }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }

    /// 開いているリポジトリ（次の起動で開き直す）。
    static var open: [String] {
        get { UserDefaults.standard.stringArray(forKey: "openRepositories") ?? [] }
        set { UserDefaults.standard.set(newValue, forKey: "openRepositories") }
    }

    /// 開いたリポジトリを先頭に置く。
    static func touch(_ path: String) {
        paths = [path] + paths.filter { $0 != path }
    }

    static func remove(_ path: String) {
        paths = paths.filter { $0 != path }
    }

    /// `path` が Git のリポジトリ（作業ツリーの直下に `.git` がある）か。
    static func isRepository(_ path: String) -> Bool {
        FileManager.default.fileExists(atPath: (path as NSString).appendingPathComponent(".git"))
    }

    /// 今のブランチ。`.git/HEAD` を読むだけで、git は起動しない（一覧を開くたびに数十件を読むため）。
    static func branch(of path: String) -> String? {
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

/// リポジトリの管理: 一覧から開く・加える・外す。
final class RepositoryManagerWindowController: NSWindowController, NSTableViewDataSource, NSTableViewDelegate,
    NSSearchFieldDelegate
{
    static let shared = RepositoryManagerWindowController()

    /// リポジトリを開く（AppDelegate が設定する）。
    var onOpen: ((String) -> Void)?

    private let table = NSTableView()
    private let filter = NSSearchField()
    private var shown: [String] = []

    private init() {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 560, height: 460),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "Repositories"
        window.setFrameAutosaveName("Repositories")
        super.init(window: window)
        build()
        reload()
    }

    required init?(coder: NSCoder) { fatalError() }

    func show() {
        reload()
        if window?.isVisible != true { window?.center() }
        showWindow(nil)
        window?.makeFirstResponder(table)
    }

    private func build() {
        guard let content = window?.contentView else { return }
        let column = NSTableColumn(identifier: .init("repository"))
        column.resizingMask = .autoresizingMask
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 44
        table.style = .inset
        table.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(openSelected)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true

        filter.placeholderString = "名前で絞り込む"
        filter.delegate = self
        filter.target = self
        filter.action = #selector(filterChanged)
        let add = NSButton(title: "Add…", target: self, action: #selector(addRepository))
        let remove = NSButton(title: "Remove", target: self, action: #selector(removeSelected))
        let open = NSButton(title: "Open", target: self, action: #selector(openSelected))
        open.keyEquivalent = "\r"
        let buttons = NSStackView(views: [add, remove, NSView(), open])

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

    private func reload() {
        let query = filter.stringValue.trimmingCharacters(in: .whitespaces).lowercased()
        shown = RepositoryList.paths.filter {
            query.isEmpty || ($0 as NSString).lastPathComponent.lowercased().contains(query)
        }
        table.reloadData()
        if !shown.isEmpty, table.selectedRow < 0 { table.selectRowIndexes([0], byExtendingSelection: false) }
    }

    @objc private func filterChanged() { reload() }

    func controlTextDidChange(_ notification: Notification) { reload() }

    @objc private func addRepository() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.message = "リポジトリのフォルダを選んでください"
        guard panel.runModal() == .OK else { return }
        var rejected: [String] = []
        for url in panel.urls.reversed() {
            if RepositoryList.isRepository(url.path) {
                RepositoryList.touch(url.path)
            } else {
                rejected.append(url.lastPathComponent)
            }
        }
        reload()
        if !rejected.isEmpty {
            let alert = NSAlert()
            alert.messageText = "Git のリポジトリではありません"
            alert.informativeText = rejected.joined(separator: "、")
            alert.runModal()
        }
    }

    @objc private func removeSelected() {
        guard table.selectedRow >= 0 else { return }
        // 一覧から外すだけで、フォルダには触れない。
        RepositoryList.remove(shown[table.selectedRow])
        reload()
    }

    @objc private func openSelected() {
        let row = table.clickedRow >= 0 ? table.clickedRow : table.selectedRow
        guard row >= 0, row < shown.count else { return }
        let path = shown[row]
        guard RepositoryList.isRepository(path) else {
            let alert = NSAlert()
            alert.messageText = "\((path as NSString).lastPathComponent) が見つかりません"
            alert.informativeText = "\(path)\n一覧から外しますか？"
            alert.addButton(withTitle: "外す")
            alert.addButton(withTitle: "キャンセル")
            if alert.runModal() == .alertFirstButtonReturn {
                RepositoryList.remove(path)
                reload()
            }
            return
        }
        onOpen?(path)
    }

    func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("repo"), owner: nil) as? RepositoryCell ?? RepositoryCell()
        let path = shown[row]
        let exists = RepositoryList.isRepository(path)
        cell.name.stringValue = (path as NSString).lastPathComponent
        cell.name.textColor = exists ? .labelColor : .tertiaryLabelColor
        cell.path.stringValue = exists ? (path as NSString).abbreviatingWithTildeInPath : "見つかりません — \(path)"
        cell.branch.stringValue = exists ? RepositoryList.branch(of: path) ?? "" : ""
        return cell
    }
}

/// 一覧の行: 名前、その下にパス、右に今のブランチ。
final class RepositoryCell: NSTableCellView {
    let name = NSTextField(labelWithString: "")
    let path = NSTextField(labelWithString: "")
    let branch = NSTextField(labelWithString: "")

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
        for v in [name, path, branch] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        path.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        branch.setContentCompressionResistancePriority(.required, for: .horizontal)
        NSLayoutConstraint.activate([
            name.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            name.topAnchor.constraint(equalTo: topAnchor, constant: 5),
            path.leadingAnchor.constraint(equalTo: name.leadingAnchor),
            path.topAnchor.constraint(equalTo: name.bottomAnchor, constant: 1),
            path.trailingAnchor.constraint(lessThanOrEqualTo: branch.leadingAnchor, constant: -8),
            branch.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            branch.centerYAnchor.constraint(equalTo: centerYAnchor),
            name.trailingAnchor.constraint(lessThanOrEqualTo: branch.leadingAnchor, constant: -8),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }
}
