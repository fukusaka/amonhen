import AmonhenKit
import AppKit

/// File > Clone…: URL（gh があれば自分の GitHub のリポジトリからも選べる）と置き場所を決めて clone し、タブで開く。
enum CloneSheet {
    private static let folderKey = "cloneFolder"

    /// 置き場所の初期値: 前に clone した場所、なければスキャンするフォルダの最初、なければホーム。
    private static var defaultFolder: String {
        UserDefaults.standard.string(forKey: folderKey)
            ?? Bookmarks.shared.data.scanRoots.first?.path
            ?? NSHomeDirectory()
    }

    /// URL の最後の部分（`.git` を除く）。
    static func name(fromURL url: String) -> String {
        var last = url.trimmingCharacters(in: .whitespaces)
        while last.hasSuffix("/") { last.removeLast() }
        last = String(last.split(whereSeparator: { $0 == "/" || $0 == ":" }).last ?? "")
        if last.hasSuffix(".git") { last.removeLast(4) }
        return last
    }

    static func show(onCloned: @escaping (String) -> Void) {
        let url = NSTextField(string: "")
        url.placeholderString = L("https://github.com/owner/repo.git or git@host:owner/repo.git")
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
            [NSTextField(labelWithString: "URL:"), url],
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
                    }
                    mine.menu?.addItem(item)
                }
                _ = picker
            }
        }
        let grid = NSGridView(views: rows)
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 1).width = 380
        grid.rowSpacing = 8
        grid.frame = NSRect(x: 0, y: 0, width: 500, height: CGFloat(rows.count) * 32)
        alert.messageText = "Clone Repository"
        alert.addButton(withTitle: "Clone")
        alert.addButton(withTitle: L("Cancel"))
        alert.accessoryView = grid
        alert.window.initialFirstResponder = url
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        _ = chooser
        let source = url.stringValue.trimmingCharacters(in: .whitespaces)
        let folderName = name.stringValue.trimmingCharacters(in: .whitespaces).isEmpty
            ? Self.name(fromURL: source) : name.stringValue.trimmingCharacters(in: .whitespaces)
        let parent = (folder.stringValue.trimmingCharacters(in: .whitespaces) as NSString).expandingTildeInPath
        guard !source.isEmpty, !folderName.isEmpty, !parent.isEmpty else { return NSSound.beep() }
        let dest = (parent as NSString).appendingPathComponent(folderName)
        if FileManager.default.fileExists(atPath: dest) {
            let exists = NSAlert()
            exists.messageText = L("%@ already exists", (dest as NSString).abbreviatingWithTildeInPath)
            exists.informativeText = L("Choose a different name or location.")
            exists.runModal()
            return
        }
        UserDefaults.standard.set(parent, forKey: folderKey)
        run(url: source, dest: dest, onCloned: onCloned)
    }

    /// 置き場所を選ぶシート（フォルダを作れる）。
    static func chooseLocation(initial: String, in window: NSWindow?, done: @escaping (String?) -> Void) {
        FilePicker.presentLocal(title: L("Clone Location"), message: L("Choose the folder to clone into."),
                                kind: .folder, initial: initial, canCreateFolders: true, in: window) { done($0?.first) }
    }

    /// clone している間は、進み具合の小さなウィンドウを出す。
    private static func run(url: String, dest: String, onCloned: @escaping (String) -> Void) {
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 420, height: 90), styleMask: [.titled], backing: .buffered, defer: false)
        panel.title = "Clone"
        let spinner = NSProgressIndicator()
        spinner.style = .spinning
        spinner.startAnimation(nil)
        let label = NSTextField(wrappingLabelWithString: L("Cloning %@…", url))
        let stack = NSStackView(views: [spinner, label])
        stack.edgeInsets = NSEdgeInsets(top: 16, left: 16, bottom: 16, right: 16)
        panel.contentView = stack
        panel.center()
        panel.makeKeyAndOrderFront(nil)
        RunningOperations.begin()
        Task {
            defer { RunningOperations.end() }
            do {
                try await Task.detached(priority: .userInitiated) { try Repository.clone(url: url, to: dest) }.value
                panel.close()
                onCloned(dest)
            } catch {
                panel.close()
                let alert = NSAlert(error: error)
                alert.messageText = L("Couldn’t clone")
                alert.informativeText = "\(error)"
                alert.runModal()
            }
        }
    }
}

/// 閉包を呼ぶだけのボタンの相手（target は弱い参照なので、使う間は持っておく）。
final class ClosureTarget: NSObject {
    private let action: () -> Void
    init(_ action: @escaping () -> Void) { self.action = action }
    @objc func fire() { action() }
}
