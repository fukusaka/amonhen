import AppKit

/// SSH の接続先の候補: `~/.ssh/config` の `Host`（`*`・`?` のない名前）と `~/.ssh/known_hosts` のホスト（ハッシュに
/// したものは読めないので出ない。ポートがあれば `host:port`）。
enum SSHHosts {
    static func candidates() -> [String] {
        let ssh = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".ssh")
        var names: [String] = []
        if let config = try? String(contentsOf: ssh.appendingPathComponent("config"), encoding: .utf8) {
            for line in config.split(whereSeparator: \.isNewline) {
                let words = line.split(whereSeparator: { $0 == " " || $0 == "\t" || $0 == "=" })
                guard words.count > 1, words[0].lowercased() == "host" else { continue }
                names += words.dropFirst().map(String.init).filter { !$0.contains(where: { "*?!".contains($0) }) }
            }
        }
        if let known = try? String(contentsOf: ssh.appendingPathComponent("known_hosts"), encoding: .utf8) {
            for line in known.split(whereSeparator: \.isNewline) {
                var fields = line.split(separator: " ", omittingEmptySubsequences: true)
                if fields.first?.hasPrefix("@") == true { fields.removeFirst() }  // @cert-authority・@revoked
                guard let hosts = fields.first, !hosts.hasPrefix("#"), !hosts.hasPrefix("|") else { continue }
                for host in hosts.split(separator: ",") {
                    // [host]:port
                    if host.hasPrefix("["), let close = host.firstIndex(of: "]") {
                        let name = host[host.index(after: host.startIndex)..<close]
                        let port = host[close...].dropFirst().drop { $0 == ":" }
                        names.append(port.isEmpty || port == "22" ? String(name) : "\(name):\(port)")
                    } else if !host.contains(where: { "*?!".contains($0) }) {
                        names.append(String(host))
                    }
                }
            }
        }
        // 名前を先に、IP アドレスは後に。
        func isAddress(_ s: String) -> Bool { s.first?.isNumber == true && s.allSatisfy { $0.isNumber || $0 == "." || $0 == ":" } || s.contains("::") }
        var seen = Set<String>()
        return names.filter { seen.insert($0).inserted }
            .sorted { (isAddress($0) ? 1 : 0, $0.lowercased()) < (isAddress($1) ? 1 : 0, $1.lowercased()) }
    }
}

/// SSH 先のフォルダを選ぶ: ホストを選び、フォルダの中身を `ssh` で読んで辿る（ダブルクリックで入る、「..」で上へ）。
/// git のリポジトリには印を付ける。行を選んでいればその行、選んでいなければ今のフォルダを返す。
/// パスワードを聞かれるホストでは読めない（BatchMode）ので、そのときはパスを直接入れてもらう。
@MainActor
final class SSHFolderChooser: NSObject, NSTableViewDataSource, NSTableViewDelegate, NSComboBoxDelegate {
    nonisolated struct Entry: Sendable {
        let name: String
        let isRepository: Bool
    }

    private let host = NSComboBox()
    private let path = NSTextField()
    private let table = NSTableView()
    private let status = NSTextField(labelWithString: "")
    private let spinner = NSProgressIndicator()
    private var entries: [Entry] = []
    /// 今読んでいるフォルダ（ホームからの `~/…` か、`/` からのパス。読めたら SSH 先が答えた本当のパス）。
    private var folder = ""
    /// 今の読み込み（古い読み込みの答えは捨てる）。
    private var loading = 0
    private var loadedHost = ""

    /// 選んだ (ホスト, パス)。取り消したら nil。`fixedHost` を渡すとホストは変えられない。
    static func run(title: String, message: String, button: String, fixedHost: String?, hosts: [String],
                    initialHost: String?, initialPath: String) -> (host: String, path: String)? {
        let chooser = SSHFolderChooser()
        return chooser.runModal(title: title, message: message, button: button, fixedHost: fixedHost, hosts: hosts,
                                initialHost: initialHost, initialPath: initialPath)
    }

    private func runModal(title: String, message: String, button: String, fixedHost: String?, hosts: [String],
                          initialHost: String?, initialPath: String) -> (host: String, path: String)? {
        host.addItems(withObjectValues: hosts)
        host.numberOfVisibleItems = 12
        host.completes = true
        host.placeholderString = L("user@host:port (or a name in ~/.ssh/config)")
        host.stringValue = fixedHost ?? initialHost ?? ""
        host.isEnabled = fixedHost == nil
        host.target = self
        host.action = #selector(hostChosen)
        host.delegate = self
        path.stringValue = initialPath
        path.placeholderString = L("Folder")
        path.target = self
        path.action = #selector(pathEntered)
        let reload = NSButton(image: NSImage(systemSymbolName: "arrow.clockwise", accessibilityDescription: L("Reload"))!,
                              target: self, action: #selector(pathEntered))
        reload.bezelStyle = .accessoryBarAction
        reload.toolTip = L("Read the folder on the host")

        let column = NSTableColumn(identifier: .init("name"))
        column.resizingMask = .autoresizingMask
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 22
        table.style = .plain
        table.usesAlternatingRowBackgroundColors = true
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(enter)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder

        status.textColor = .secondaryLabelColor
        status.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        status.lineBreakMode = .byTruncatingTail
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false

        let pathRow = NSStackView(views: [path, reload])
        let statusRow = NSStackView(views: [spinner, status])
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Host:")), host],
            [NSTextField(labelWithString: L("Folder:")), pathRow],
            [NSGridCell.emptyContentView, scroll],
            [NSGridCell.emptyContentView, statusRow],
        ])
        grid.column(at: 0).xPlacement = .trailing
        grid.rowSpacing = 8
        grid.column(at: 1).width = 380
        grid.row(at: 2).height = 240
        grid.frame = NSRect(x: 0, y: 0, width: 460, height: 340)
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.heightAnchor.constraint(equalToConstant: 240).isActive = true

        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.accessoryView = grid
        alert.addButton(withTitle: button)
        alert.addButton(withTitle: L("Cancel"))
        alert.window.initialFirstResponder = host.stringValue.isEmpty ? host : path
        if !host.stringValue.isEmpty { load(path.stringValue) }
        let response = alert.runModal()
        loading += 1
        guard response == .alertFirstButtonReturn else { return nil }
        let name = host.stringValue.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return nil }
        let row = table.selectedRow
        if row >= 0, row < entries.count, entries[row].name != "..", name == loadedHost {
            return (name, Self.join(folder, entries[row].name))
        }
        return (name, path.stringValue)
    }

    @objc private func hostChosen() { load(path.stringValue) }

    func comboBoxSelectionDidChange(_ notification: Notification) {
        // 選んだ項目は、この知らせの後で欄に入る。
        Self.onMain { [weak self] in
            guard let self else { return }
            load(path.stringValue)
        }
    }

    @objc private func pathEntered() { load(path.stringValue) }

    @objc private func enter() {
        let row = table.clickedRow >= 0 ? table.clickedRow : table.selectedRow
        guard row >= 0, row < entries.count else { return }
        let entry = entries[row]
        load(entry.name == ".." ? Self.parent(folder) : Self.join(folder, entry.name))
    }

    /// `host` の `target` の中のフォルダを読む。
    private func load(_ target: String) {
        let name = host.stringValue.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty, !name.contains(where: { $0.isWhitespace }) else { return }
        let target = target.trimmingCharacters(in: .whitespaces).isEmpty ? "~" : target.trimmingCharacters(in: .whitespaces)
        guard !target.contains("'") else {
            status.stringValue = L("The folder name can’t contain “'”.")
            return
        }
        loading += 1
        let token = loading
        spinner.startAnimation(nil)
        status.stringValue = L("Reading %@ on %@…", target, name)
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Self.list(host: name, folder: target)
            Self.onMain { [weak self] in
                guard let self, token == loading else { return }
                show(result, host: name)
            }
        }
    }

    /// メインスレッドで `body` を動かす。モーダルのダイアログの最中でも届くよう、メインキューではなく run loop の
    /// common モードに入れる（メインキューの仕事の中から開いたダイアログの間は、メインキューは止まっている）。
    nonisolated private static func onMain(_ body: @escaping @MainActor () -> Void) {
        RunLoop.main.perform(inModes: [.common]) { MainActor.assumeIsolated { body() } }
        CFRunLoopWakeUp(CFRunLoopGetMain())
    }

    private func show(_ result: Result<(String, [Entry]), ListFailure>, host name: String) {
        spinner.stopAnimation(nil)
        switch result {
        case let .success((resolved, list)):
            loadedHost = name
            folder = Self.home(resolved, of: name)
            path.stringValue = folder
            entries = (folder == "/" ? [] : [Entry(name: "..", isRepository: false)]) + list
            table.reloadData()
            table.deselectAll(nil)
            let repositories = list.filter(\.isRepository).count
            status.stringValue = repositories > 0
                ? L("Folders: %d, repositories: %d", list.count, repositories)
                : L("Folders: %d", list.count)
        case let .failure(failure):
            loadedHost = ""
            entries = []
            table.reloadData()
            status.stringValue = failure.text
        }
    }

    /// 読めたパスを、ホームの下なら `~/…` にする（`RemoteHome` が知っていれば。知らなければ覚える）。
    private static func home(_ resolved: String, of host: String) -> String {
        guard let home = RemoteHome.known(host) else {
            RemoteHome.learn(host)
            return resolved
        }
        if resolved == home { return "~" }
        if resolved.hasPrefix(home + "/") { return "~" + resolved.dropFirst(home.count) }
        return resolved
    }

    nonisolated struct ListFailure: Error {
        let text: String
    }

    /// SSH 先で、`folder` に入って本当のパスと中のフォルダ（`.git` のあるものには印）を出す。シェルが sh でなくても
    /// 動くよう `sh -c` に渡し、パスは引数で渡す（`'` を含むパスは呼ぶ前に断る）。
    nonisolated private static func list(host: String, folder: String) -> Result<(String, [Entry]), ListFailure> {
        let script = #"p=$1; case $p in "~") p=$HOME;; "~/"*) p=$HOME/${p#"~/"};; esac; cd -- "$p" || exit 3; pwd; for d in */; do [ -d "$d" ] || continue; if [ -e "$d.git" ]; then echo "G${d%/}"; else echo "D${d%/}"; fi; done"#
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
        // 辿るたびにつなぎ直さないよう、つないだままにしておく（1 分）。
        process.arguments = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                             "-o", "ControlMaster=auto", "-o", "ControlPath=/tmp/amonhen-ssh-%C", "-o", "ControlPersist=60",
                             host, "exec sh -c '\(script)' sh '\(folder)'"]
        let out = Pipe(), err = Pipe()
        process.standardOutput = out
        process.standardError = err
        process.standardInput = FileHandle.nullDevice
        do { try process.run() } catch { return .failure(ListFailure(text: "\(error)")) }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        let errors = err.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        switch process.terminationStatus {
        case 0: break
        case 3: return .failure(ListFailure(text: L("No folder %@", folder)))
        case 255:
            let text = String(decoding: errors, as: UTF8.self).split(whereSeparator: \.isNewline).last.map(String.init) ?? ""
            return .failure(ListFailure(text: L("Can’t connect without a password: %@", text)))
        default:
            return .failure(ListFailure(text: String(decoding: errors, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
        }
        var lines = String(decoding: data, as: UTF8.self).split(whereSeparator: \.isNewline).map(String.init)
        guard !lines.isEmpty else { return .failure(ListFailure(text: L("No answer from %@", host))) }
        let resolved = lines.removeFirst()
        let entries = lines.compactMap { line -> Entry? in
            guard let kind = line.first, line.count > 1 else { return nil }
            return Entry(name: String(line.dropFirst()), isRepository: kind == "G")
        }.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        return .success((resolved, entries))
    }

    private static func join(_ folder: String, _ name: String) -> String {
        folder.hasSuffix("/") ? folder + name : folder + "/" + name
    }

    private static func parent(_ folder: String) -> String {
        if folder == "~" { return "~/.." }
        let parent = (folder as NSString).deletingLastPathComponent
        return parent.isEmpty ? "/" : parent
    }

    // MARK: - 一覧

    func numberOfRows(in tableView: NSTableView) -> Int { entries.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: .init("entry"), owner: nil) as? NSTableCellView ?? {
            let cell = NSTableCellView()
            cell.identifier = .init("entry")
            let image = NSImageView()
            let text = NSTextField(labelWithString: "")
            text.lineBreakMode = .byTruncatingMiddle
            for v in [image, text] as [NSView] {
                v.translatesAutoresizingMaskIntoConstraints = false
                cell.addSubview(v)
            }
            cell.imageView = image
            cell.textField = text
            NSLayoutConstraint.activate([
                image.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 4),
                image.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
                image.widthAnchor.constraint(equalToConstant: 16),
                text.leadingAnchor.constraint(equalTo: image.trailingAnchor, constant: 6),
                text.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
                text.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
            ])
            return cell
        }()
        let entry = entries[row]
        cell.textField?.stringValue = entry.name
        cell.imageView?.image = NSImage(systemSymbolName: entry.name == ".." ? "arrow.turn.left.up"
                                        : entry.isRepository ? "arrow.triangle.branch" : "folder",
                                        accessibilityDescription: nil)
        cell.imageView?.contentTintColor = entry.isRepository ? .systemGreen : .secondaryLabelColor
        cell.toolTip = entry.isRepository ? L("Git repository") : nil
        return cell
    }
}
