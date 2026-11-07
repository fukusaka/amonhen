import AppKit

/// submodule の様子（コアの `submodules`）。
nonisolated struct SubmoduleState: Decodable, Hashable, Sendable {
    let path: String
    let name: String
    let url: String?
    /// 追うブランチ（`update --remote` で進める先）。
    let branch: String?
    /// 取り出してある。
    let initialized: Bool
    /// 中のリポジトリのデータを自分のフォルダに持っている（`.git/modules` に移せる）。
    var ownGitDir: Bool? = nil
    /// 記録したコミットと違うコミットにいる。
    let moved: Bool
    /// 中に、コミットしていない変更か untracked のファイルがある。
    let changed: Bool
    let conflict: Bool
}

/// submodule を足す・取り出す・外す・URL や追うブランチを変えるときの小さなシート（NSAlert）。どれも答えを
/// コアの命令にして `perform` に渡す。submodule は worktree ごとのもの（作業ツリーと index）なので、説明には
/// `actions`（選んだときの worktree のもの）の worktree の名前を書く（docs/ux.md の「Worktrees」）。
@MainActor
enum SubmoduleSheet {
    /// 1 行の入力欄。
    private static func field(_ value: String, placeholder: String) -> NSTextField {
        let field = NSTextField(string: value)
        field.placeholderString = placeholder
        field.frame.size = NSSize(width: 360, height: 24)
        // 左寄せの縦の並びに入れても、幅が文字の分に縮まないように。
        field.widthAnchor.constraint(equalToConstant: 360).isActive = true
        return field
    }

    private static func stack(_ views: [NSView], height: CGFloat) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.frame = NSRect(x: 0, y: 0, width: 360, height: height)
        return stack
    }

    /// URL の最後の部分から `.git` を除いたもの（git が submodule の場所の既定にするもの）。
    static func defaultPath(for url: String) -> String {
        var last = url.trimmingCharacters(in: .whitespaces)
        while last.hasSuffix("/") { last.removeLast() }
        last = String(last.split(whereSeparator: { $0 == "/" || $0 == ":" }).last ?? "")
        return last.hasSuffix(".git") ? String(last.dropLast(4)) : last
    }

    /// Add Submodule…: URL・場所・追うブランチ（なくてもよい）。
    static func add(in window: NSWindow, actions: Actions, perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = L("Add Submodule")
        alert.informativeText = actions.sheetMessage(L("Clones the repository into a folder of this one and records it in .gitmodules (git submodule add). Commit to keep it."))
        let url = field("", placeholder: L("Repository URL"))
        let path = field("", placeholder: L("Folder"))
        let branch = field("", placeholder: L("Branch to follow (optional)"))
        alert.accessoryView = stack([url, path, branch], height: 88)
        alert.window.initialFirstResponder = url
        let target = ClosureTarget {
            let name = defaultPath(for: url.stringValue)
            path.placeholderString = name.isEmpty ? L("Folder") : name
        }
        NotificationCenter.default.addObserver(forName: NSControl.textDidChangeNotification, object: url, queue: .main) { _ in
            MainActor.assumeIsolated { target.fire() }
        }
        alert.addButton(withTitle: L("Add"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            withExtendedLifetime(target) {}
            guard response == .alertFirstButtonReturn else { return }
            let u = url.stringValue.trimmingCharacters(in: .whitespaces)
            var p = path.stringValue.trimmingCharacters(in: .whitespaces)
            if p.isEmpty { p = defaultPath(for: u) }
            guard !u.isEmpty, !p.isEmpty else { return NSSound.beep() }
            let b = branch.stringValue.trimmingCharacters(in: .whitespaces)
            perform(["op": "changeSubmodule",
                     "change": ["kind": "add", "url": u, "path": p, "branch": b.isEmpty ? NSNull() : b] as [String: Any]])
        }
    }

    /// Update…: git の既定（記録したコミット、中の submodule には入らない）から変えて取り出す。`paths` が空なら
    /// すべて。
    static func update(in window: NSWindow, actions: Actions, paths: [String], perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = paths.count == 1 ? L("Update %@", paths[0]) : L("Update All Submodules")
        alert.informativeText = actions.sheetMessage(L("Checks out the commit recorded in this repository, unless you choose otherwise."))
        let recursive = NSButton(checkboxWithTitle: L("Also update the submodules inside it (--recursive)"), target: nil, action: nil)
        let remote = NSButton(checkboxWithTitle: L("Use the latest commit of the branch it follows (--remote)"), target: nil, action: nil)
        alert.accessoryView = stack([recursive, remote], height: 48)
        alert.addButton(withTitle: "Update")
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            perform(["op": "updateSubmodules", "paths": paths,
                     "recursive": recursive.state == .on, "remote": remote.state == .on])
        }
    }

    /// Edit URL…: `.gitmodules` の URL を変え、このリポジトリの設定にも写す。
    static func setURL(in window: NSWindow, actions: Actions, submodule: SubmoduleState, perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = L("Edit URL of %@", submodule.path)
        alert.informativeText = actions.sheetMessage(L("Changes the URL in .gitmodules and copies it to this repository’s settings (git submodule set-url). Commit .gitmodules to share it."))
        let url = field(submodule.url ?? "", placeholder: L("Repository URL"))
        alert.accessoryView = url
        alert.window.initialFirstResponder = url
        alert.addButton(withTitle: L("Save"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            let u = url.stringValue.trimmingCharacters(in: .whitespaces)
            guard !u.isEmpty else { return NSSound.beep() }
            perform(["op": "changeSubmodule", "change": ["kind": "setUrl", "path": submodule.path, "url": u]])
        }
    }

    /// Set Branch…: Update で「追うブランチの最新」を選んだときに進める先。空なら既定（リモートの HEAD）。
    static func setBranch(in window: NSWindow, actions: Actions, submodule: SubmoduleState, perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = L("Branch to Follow for %@", submodule.path)
        alert.informativeText = actions.sheetMessage(L("Updating with “the latest commit of the branch it follows” moves to this branch’s latest commit. Leave it empty for the remote’s default branch."))
        let branch = field(submodule.branch ?? "", placeholder: L("Default branch"))
        alert.accessoryView = branch
        alert.window.initialFirstResponder = branch
        alert.addButton(withTitle: L("Save"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            let b = branch.stringValue.trimmingCharacters(in: .whitespaces)
            perform(["op": "changeSubmodule",
                     "change": ["kind": "setBranch", "path": submodule.path, "branch": b.isEmpty ? NSNull() : b] as [String: Any]])
        }
    }

    /// Run in Each Submodule…: それぞれの submodule の中で、シェルで命令を動かす（git submodule foreach）。出力は
    /// Activity に残る。
    static func foreach(in window: NSWindow, actions: Actions, perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = L("Run in Each Submodule")
        alert.informativeText = actions.sheetMessage(L("Runs the command with the shell in each submodule’s folder (git submodule foreach). Its output is in Activity."))
        let command = field("", placeholder: L("Command"))
        let recursive = NSButton(checkboxWithTitle: L("Also in the submodules inside them (--recursive)"), target: nil, action: nil)
        alert.accessoryView = stack([command, recursive], height: 56)
        alert.window.initialFirstResponder = command
        alert.addButton(withTitle: L("Run"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            let c = command.stringValue.trimmingCharacters(in: .whitespaces)
            guard !c.isEmpty else { return NSSound.beep() }
            perform(["op": "changeSubmodule", "change": ["kind": "foreach", "command": c, "recursive": recursive.state == .on]])
        }
    }

    /// Remove…: 作業ツリーと `.gitmodules` から外す。git の既定どおり `.git/modules/<name>` は残し、消すかは選ぶ。
    static func remove(in window: NSWindow, actions: Actions, submodule: SubmoduleState, perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = L("Remove %@?", submodule.path)
        alert.informativeText = actions.sheetMessage(L("Removes the folder and its entry in .gitmodules, and stages that. Commit to keep it."))
        let deleteGitDir = NSButton(checkboxWithTitle: L("Also delete its repository data (.git/modules/%@)", submodule.name), target: nil, action: nil)
        let note = NSTextField(wrappingLabelWithString: L("Kept, it brings back the submodule’s commits and stashes when you add it again, including ones not pushed."))
        note.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        note.textColor = .secondaryLabelColor
        note.preferredMaxLayoutWidth = 360
        alert.accessoryView = stack([deleteGitDir, note], height: 64)
        alert.addButton(withTitle: L("Remove"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            perform(["op": "changeSubmodule",
                     "change": ["kind": "remove", "path": submodule.path, "deleteGitDir": deleteGitDir.state == .on] as [String: Any]])
        }
    }
}
