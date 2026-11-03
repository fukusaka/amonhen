import AmonhenKit
import AppKit
import UniformTypeIdentifiers

/// File > Clone…: URL（gh があれば自分の GitHub のリポジトリからも選べる）と置き場所を決めて clone し、タブで開く。
/// 取得する範囲（すべて・ファイルの中身は必要なときに・最近の履歴だけ）と、一部のフォルダだけを取り出すかも選べる
/// （docs/ux.md の「Clone」）。既定は git と同じく、すべてを取得してすべてを取り出す。
enum CloneSheet {
    private static let folderKey = "cloneFolder"
    /// `git clone --sparse` が使える git の版。
    private static let sparseVersion = (2, 27)

    /// 取得する範囲。
    enum Download: Int, CaseIterable {
        /// すべて（ふつうの `git clone`）。
        case everything
        /// ファイルの中身は必要なときに取る（`--filter=blob:none`）。
        case contentsWhenNeeded
        /// 最近の履歴だけ（`--depth`）。
        case recentHistory

        var title: String {
            switch self {
            case .everything: L("Everything")
            case .contentsWhenNeeded: L("File contents when needed")
            case .recentHistory: L("Recent history only")
            }
        }

        /// 選んだものの 1 文の説明。
        var help: String {
            switch self {
            case .everything: L("The whole history and the contents of every file, as git does by default.")
            case .contentsWhenNeeded: L("Good for large repositories: file contents are fetched when a diff or file needs them.")
            case .recentHistory: L("Older history can be fetched later from the end of History.")
            }
        }
    }

    /// 置き場所の初期値: 前に clone した場所、なければスキャンするフォルダの最初（手元のもの。SSH 先のフォルダには
    /// clone できない）、なければホーム。
    private static var defaultFolder: String {
        UserDefaults.standard.string(forKey: folderKey)
            ?? Bookmarks.shared.data.scanRoots.first { RepositoryBrowser.sshHost($0.path) == nil }
                .map { RepositoryBrowser.localPath($0.path) }
            ?? NSHomeDirectory()
    }

    /// URL の最後の部分（`.git` を除く）。
    static func name(fromURL url: String) -> String {
        var last = url.trimmingCharacters(in: .whitespaces)
        while last.hasSuffix("/") { last.removeLast() }
        last = String(last.split(whereSeparator: { $0 == "/" || $0 == ":" }).last ?? "")
        if last.hasSuffix(".git") { last.removeLast(4) }
        if last.hasSuffix(".bundle") { last.removeLast(7) }
        return last
    }

    /// `window` のシートで聞く（なければアプリ全体で）。
    static func show(in window: NSWindow?, onCloned: @escaping (String) -> Void) {
        let url = NSTextField(string: "")
        url.placeholderString = L("https://github.com/owner/repo.git, git@host:owner/repo.git or a bundle file")
        // bundle のファイルを選ぶ（URL の欄にパスを入れる）。
        let bundleButton = NSButton(title: L("Choose Bundle…"), target: nil, action: nil)
        let folder = NSTextField(string: (defaultFolder as NSString).abbreviatingWithTildeInPath)
        let name = NSTextField(string: "")
        name.placeholderString = L("Taken from the URL")
        let choose = NSButton(title: L("Choose…"), target: nil, action: nil)
        let alert = NSAlert()
        let chooser = ClosureTarget { [weak alert] in
            chooseLocation(initial: folder.stringValue, in: alert?.window) { picked in
                if let picked { folder.stringValue = (picked as NSString).abbreviatingWithTildeInPath }
            }
        }
        choose.target = chooser
        choose.action = #selector(ClosureTarget.fire)
        var rows: [[NSView]] = [
            [NSTextField(labelWithString: "URL:"), NSStackView(views: [url, bundleButton])],
            [NSTextField(labelWithString: L("Location:")), NSStackView(views: [folder, choose])],
            [NSTextField(labelWithString: L("Folder Name:")), name],
        ]
        // gh があれば、自分のリポジトリから選べる（読み終えたら一覧に入れる）。
        let mine = NSPopUpButton(frame: .zero, pullsDown: true)
        var picker: ClosureTarget?
        if GitHub.isAvailable {
            mine.addItem(withTitle: L("Choose from My GitHub Repositories"))
            mine.menu?.addItem(withTitle: L("Loading…"), action: nil, keyEquivalent: "")
            rows.insert([NSGridCell.emptyContentView, mine], at: 1)
            Task {
                struct Repo: Decodable { let nameWithOwner: String; let url: String }
                let data = try? await GitHub.run(["repo", "list", "--json", "nameWithOwner,url", "--limit", "200"])
                let repos = data.flatMap { try? JSONDecoder().decode([Repo].self, from: $0) } ?? []
                while (mine.menu?.items.count ?? 0) > 1 { mine.menu?.removeItem(at: 1) }
                if repos.isEmpty { mine.menu?.addItem(withTitle: L("None found"), action: nil, keyEquivalent: "") }
                let target = ClosureTarget {}
                picker = target
                for repo in repos.sorted(by: { $0.nameWithOwner.lowercased() < $1.nameWithOwner.lowercased() }) {
                    let item = ClosureMenuItem(title: repo.nameWithOwner) {
                        url.stringValue = repo.url + ".git"
                        name.stringValue = ""
                        // コードで入れても知らせが来ないので、打ち込んだときと同じく知らせる（bundle の欄などを戻す）。
                        NotificationCenter.default.post(name: NSControl.textDidChangeNotification, object: url)
                    }
                    mine.menu?.addItem(item)
                }
                _ = picker
            }
        }
        // 取得する範囲と、最近の履歴だけなら何コミットか（既定は 100）。下に選んだものの説明。
        let download = NSPopUpButton(frame: .zero, pullsDown: false)
        download.addItems(withTitles: Download.allCases.map(\.title))
        // 1（いちばん新しいコミットだけ）・決まった数・「その他…」（好きな数を尋ねる。決めた数は項目にして選ぶ）。
        let depth = NSPopUpButton(frame: .zero, pullsDown: false)
        depth.addItem(withTitle: L("1 Commit"))
        depth.lastItem?.tag = 1
        for choice in ShallowHistoryBand.moreChoices {
            depth.addItem(withTitle: choice.title)
            depth.lastItem?.tag = choice.commits
        }
        depth.menu?.addItem(.separator())
        depth.addItem(withTitle: L("Other…"))
        depth.lastItem?.tag = -1
        depth.selectItem(withTag: 100)
        var depthChosen = 100
        let depthTarget = ClosureTarget { [weak depth] in
            guard let depth else { return }
            guard depth.selectedTag() == -1 else {
                depthChosen = depth.selectedTag()
                return
            }
            depth.selectItem(withTag: depthChosen)
            ShallowHistoryBand.askCount(in: nil, title: L("Recent History"),
                                        message: L("How many recent commits to fetch (git clone --depth)."),
                                        button: L("Choose"), initial: depthChosen) { commits in
                if depth.indexOfItem(withTag: commits) < 0 {
                    let title = commits == 1 ? L("1 Commit")
                        : L("%@ Commits", NumberFormatter.localizedString(from: NSNumber(value: commits), number: .decimal))
                    depth.menu?.insertItem(withTitle: title, action: nil, keyEquivalent: "",
                                           at: depth.indexOfItem(withTag: -1) - 1)
                    depth.item(at: depth.indexOfItem(withTag: -1) - 2)?.tag = commits
                }
                depth.selectItem(withTag: commits)
                depthChosen = commits
            }
        }
        depth.target = depthTarget
        depth.action = #selector(ClosureTarget.fire)
        let downloadHelp = helpLabel()
        // 一部のフォルダだけ取り出す（git 2.27 から）。下に説明か、使えなければ ⚠ と理由。
        let sparse = NSButton(checkboxWithTitle: L("Check out only some folders"), target: nil, action: nil)
        let sparseHelp = helpLabel()
        var sparseUnavailable: String?
        // submodule も取り出す（git の既定は取り出さない。あとからサイドバーの Submodules で取り出せる）。
        let submodules = NSButton(checkboxWithTitle: L("Check out submodules too"), target: nil, action: nil)
        // bundle のファイルなら: 取得する範囲は選べない（git は中身をすべて写す）。HEAD がなければ取り出すブランチを選ぶ。
        let branch = NSPopUpButton(frame: .zero, pullsDown: false)
        let branchLabel = NSTextField(labelWithString: L("Branch:"))
        var bundle: BundleFileInfo?
        let update = {
            let source = (url.stringValue.trimmingCharacters(in: .whitespaces) as NSString).expandingTildeInPath
            let read = FileManager.default.fileExists(atPath: source) ? BundleFileInfo.read(path: source) : nil
            if read?.refs != bundle?.refs || (read == nil) != (bundle == nil) {
                bundle = read
                branch.removeAllItems()
                for ref in read?.refs ?? [] {
                    let short = ref.hasPrefix("refs/heads/") ? String(ref.dropFirst(11))
                        : ref.hasPrefix("refs/tags/") ? String(ref.dropFirst(10)) : ref
                    branch.addItem(withTitle: short)
                }
                if let chosen = read?.branch { branch.selectItem(withTitle: chosen) }
            }
            let showBranch = bundle.map { !$0.head && !$0.refs.isEmpty && $0.prerequisites == 0 } ?? false
            branch.isHidden = !showBranch
            branchLabel.isHidden = !showBranch
            download.isEnabled = bundle == nil
            let choice = bundle == nil ? (Download(rawValue: download.indexOfSelectedItem) ?? .everything) : .everything
            depth.isHidden = choice != .recentHistory
            downloadHelp.textColor = .secondaryLabelColor
            if let bundle, bundle.prerequisites > 0 {
                downloadHelp.stringValue = "⚠ " + L("This bundle has only the commits after ones it doesn’t include, so it can’t be cloned.")
                downloadHelp.textColor = .systemOrange
            } else if bundle != nil {
                downloadHelp.stringValue = L("From a bundle file, everything in it is copied.")
            } else {
                downloadHelp.stringValue = choice.help
            }
            sparse.isEnabled = sparseUnavailable == nil
            if let sparseUnavailable {
                sparse.state = .off
                sparseHelp.stringValue = "⚠ " + sparseUnavailable
                sparseHelp.textColor = .systemOrange
            } else {
                sparseHelp.stringValue = L("Only the files at the top are checked out; right after the clone, choose a folder to check out.")
                sparseHelp.textColor = .secondaryLabelColor
            }
        }
        let changed = ClosureTarget { update() }
        download.target = changed
        download.action = #selector(ClosureTarget.fire)
        // 打ち込んだり貼り付けたりしたパスが bundle なら、すぐ切り替える（知らせはメインスレッドに来る）。
        nonisolated(unsafe) let refresh = update
        let typed = NotificationCenter.default.addObserver(forName: NSControl.textDidChangeNotification, object: url,
                                                          queue: .main) { _ in MainActor.assumeIsolated { refresh() } }
        let bundleChooser = ClosureTarget { [weak alert] in
            let panel = NSOpenPanel()
            panel.allowedContentTypes = [UTType(filenameExtension: "bundle") ?? .data]
            panel.canChooseDirectories = false
            panel.message = L("Choose a bundle file (git bundle) to clone from.")
            let picked = { (response: NSApplication.ModalResponse) in
                guard response == .OK, let path = panel.url?.path else { return }
                url.stringValue = (path as NSString).abbreviatingWithTildeInPath
                update()
            }
            if let window = alert?.window { panel.beginSheetModal(for: window, completionHandler: picked) } else { picked(panel.runModal()) }
        }
        bundleButton.target = bundleChooser
        bundleButton.action = #selector(ClosureTarget.fire)
        // 見た目の確認用の起動（`--clone-sheet history,sparse` など）では、その選び方で出す。
        let preset = AppDelegate.option("--clone-sheet") ?? ""
        if preset.contains("contents") { download.selectItem(at: Download.contentsWhenNeeded.rawValue) }
        if preset.contains("history") { download.selectItem(at: Download.recentHistory.rawValue) }
        if preset.contains("sparse") { sparse.state = .on }
        if preset.contains("submodules") { submodules.state = .on }
        update()
        // この Mac の git の版を裏で読み、古ければ止める（読めなければ使えるとみなす。clone のときにコアが確かめる）。
        Task {
            let version = await Task.detached(priority: .userInitiated) { LocalGit.version() }.value
            let numbers = version?.split(separator: ".").prefix(2).compactMap { Int($0) } ?? []
            if numbers.count == 2, (numbers[0], numbers[1]) < sparseVersion, let version {
                sparseUnavailable = L("Needs git 2.27 or later. This Mac’s git is %@.", version)
                update()
            }
        }
        rows += [
            [branchLabel, branch],
            [NSTextField(labelWithString: L("Download:")), NSStackView(views: [download, depth])],
            [NSGridCell.emptyContentView, downloadHelp],
            [NSGridCell.emptyContentView, sparse],
            [NSGridCell.emptyContentView, sparseHelp],
            [NSGridCell.emptyContentView, submodules],
        ]
        let grid = NSGridView(views: rows)
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 1).width = 380
        grid.rowSpacing = 8
        grid.frame = NSRect(x: 0, y: 0, width: 500, height: grid.fittingSize.height)
        alert.messageText = "Clone Repository"
        alert.addButton(withTitle: "Clone")
        alert.addButton(withTitle: L("Cancel"))
        alert.accessoryView = grid
        alert.window.initialFirstResponder = url
        // 窓のシートで聞く（ほかの窓は止めない）。閉じたら、選んだもので clone を始める。
        func present() {
            if let window { alert.beginSheetModal(for: window, completionHandler: ended) } else { ended(alert.runModal()) }
        }
        func ended(_ response: NSApplication.ModalResponse) {
            // シートに戻るときは、URL の欄の見張りとボタンの相手を残す（bundle のパスを打てば切り替わるように）。
            if response == .alertFirstButtonReturn, startOrAskAgain() { return }
            // 閉じ終えた（取り消した・clone を始めた）ら片づける。
            _ = (chooser, changed, depthTarget, bundleChooser)
            NotificationCenter.default.removeObserver(typed)
        }
        present()

        /// clone を始める。置き場所がもうあれば、そう言ってからシートに戻り（入れたものは残す）、true を返す。
        func startOrAskAgain() -> Bool {
            update()
            var source = url.stringValue.trimmingCharacters(in: .whitespaces)
            if bundle != nil { source = (source as NSString).expandingTildeInPath }
            let folderName = name.stringValue.trimmingCharacters(in: .whitespaces).isEmpty
                ? Self.name(fromURL: source) : name.stringValue.trimmingCharacters(in: .whitespaces)
            let parent = (folder.stringValue.trimmingCharacters(in: .whitespaces) as NSString).expandingTildeInPath
            guard !source.isEmpty, !folderName.isEmpty, !parent.isEmpty else {
                NSSound.beep()
                return false
            }
            let dest = (parent as NSString).appendingPathComponent(folderName)
            if FileManager.default.fileExists(atPath: dest) {
                let exists = NSAlert()
                exists.messageText = L("%@ already exists", (dest as NSString).abbreviatingWithTildeInPath)
                exists.informativeText = L("Choose a different name or location.")
                // 閉じ終えたシートの後に出し、閉じたら Clone のシートに戻る。
                DispatchQueue.main.async {
                    if let window {
                        exists.beginSheetModal(for: window) { _ in DispatchQueue.main.async { present() } }
                    } else {
                        exists.runModal()
                        present()
                    }
                }
                return true
            }
            UserDefaults.standard.set(parent, forKey: folderKey)
            let choice = bundle == nil ? (Download(rawValue: download.indexOfSelectedItem) ?? .everything) : .everything
            run(url: source, dest: dest, filter: choice == .contentsWhenNeeded ? "blob:none" : nil,
                depth: choice == .recentHistory ? depth.selectedTag() : nil,
                sparse: sparse.isEnabled && sparse.state == .on,
                branch: branch.isHidden ? nil : branch.titleOfSelectedItem, submodules: submodules.state == .on,
                onCloned: onCloned)
            return false
        }
    }

    /// 選んだものの説明（小さく、薄く。2 行まで折り返す）。
    private static func helpLabel() -> NSTextField {
        let label = NSTextField(wrappingLabelWithString: "")
        label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        label.textColor = .secondaryLabelColor
        label.preferredMaxLayoutWidth = 380
        // 選び直して文の長さが変わっても、シートの高さが変わらないよう 2 行分を取っておく。
        label.heightAnchor.constraint(equalToConstant: ceil(label.font!.boundingRectForFont.height) * 2 + 2).isActive = true
        return label
    }

    /// 置き場所を選ぶシート（フォルダを作れる）。
    static func chooseLocation(initial: String, in window: NSWindow?, done: @escaping (String?) -> Void) {
        FilePicker.presentLocal(title: L("Clone Location"), message: L("Choose the folder to clone into."),
                                kind: .folder, initial: initial, canCreateFolders: true, in: window) { done($0?.first) }
    }

    /// clone している間は、進み具合の小さなウィンドウを出す。`sparse` なら、clone の後で取り出すフォルダを選んで
    /// もらってから開く（取り消したら根のファイルだけのまま。フォルダは Settings の「取得とフォルダ」で足せる）。
    private static func run(url: String, dest: String, filter: String?, depth: Int?, sparse: Bool, branch: String?,
                            submodules: Bool, onCloned: @escaping (String) -> Void) {
        let panel = progressPanel(L("Cloning %@…", url))
        RunningOperations.begin()
        Task {
            defer { RunningOperations.end() }
            do {
                try await Task.detached(priority: .userInitiated) {
                    try Repository.clone(url: url, to: dest, filter: filter, depth: depth, sparse: sparse, branch: branch,
                                         recurseSubmodules: submodules)
                }.value
                panel.close()
                if sparse {
                    chooseFolder(in: dest) { onCloned(dest) }
                } else {
                    onCloned(dest)
                }
            } catch {
                panel.close()
                let alert = NSAlert(error: error)
                alert.messageText = L("Couldn’t clone")
                alert.informativeText = "\(error)"
                alert.runModal()
            }
        }
    }

    /// 根のファイルだけを取り出した clone で、取り出すフォルダを HEAD の木から選び（取得とフォルダのページと同じ
    /// シート）、取り出す。終わったら（取り消しても、失敗しても）`done`。
    private static func chooseFolder(in dest: String, done: @escaping () -> Void) {
        guard let repository = try? Repository(path: dest) else { return done() }
        FilePicker.presentTree(
            title: L("Check Out Only Some Folders"),
            message: L("Only the files at the top are checked out. Choose a folder to check out too; add more later in Settings > Fetching and Folders."),
            repository: repository
        ) { folder in
            guard let folder else { return done() }
            let panel = progressPanel(L("Checking out %@…", folder))
            RunningOperations.begin()
            Task {
                defer { RunningOperations.end() }
                let failure = await Task.detached(priority: .userInitiated) { () -> String? in
                    do {
                        try repository.run(["op": "sparseCheckout", "folders": [folder]])
                        return nil
                    } catch {
                        return "\(error)"
                    }
                }.value
                panel.close()
                if let failure {
                    let alert = NSAlert()
                    alert.messageText = L("Couldn’t check out %@", folder)
                    alert.informativeText = failure
                    alert.runModal()
                }
                done()
            }
        }
    }

    /// 進み具合の小さなウィンドウ。
    private static func progressPanel(_ text: String) -> NSPanel {
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 420, height: 90), styleMask: [.titled], backing: .buffered, defer: false)
        panel.title = "Clone"
        let spinner = NSProgressIndicator()
        spinner.style = .spinning
        spinner.startAnimation(nil)
        let label = NSTextField(wrappingLabelWithString: text)
        let stack = NSStackView(views: [spinner, label])
        stack.edgeInsets = NSEdgeInsets(top: 16, left: 16, bottom: 16, right: 16)
        panel.contentView = stack
        panel.center()
        panel.makeKeyAndOrderFront(nil)
        return panel
    }
}

/// 閉包を呼ぶだけのボタンの相手（target は弱い参照なので、使う間は持っておく）。
final class ClosureTarget: NSObject {
    private let action: () -> Void
    init(_ action: @escaping () -> Void) { self.action = action }
    @objc func fire() { action() }
}
