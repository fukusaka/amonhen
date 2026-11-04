import AppKit

/// subtree（コアの `subtrees`）。
nonisolated struct SubtreeInfo: Decodable, Hashable, Sendable {
    let dir: String
    /// 最後に記録した取り込みが squash だった。
    let squashed: Bool
    /// 前に使った相手（このリポジトリの設定に覚えたもの）。
    let url: String?
    let ref: String?
    let pushRef: String?
    let squash: Bool?
}

/// subtree の一覧と、`git subtree` が使えるか。
nonisolated struct SubtreeList: Decodable, Sendable {
    let available: Bool
    let subtrees: [SubtreeInfo]
}

/// subtree の Add・Pull・Push・Split のシート（NSAlert）。答えをコアの `subtree` の命令にして `perform` に渡す。
/// `actions`（選んだときの worktree のもの）の worktree の名前を書く（docs/ux.md の「Worktrees」）。
@MainActor
enum SubtreeSheet {
    private static func field(_ value: String, placeholder: String) -> NSTextField {
        let field = NSTextField(string: value)
        field.placeholderString = placeholder
        field.frame.size = NSSize(width: 360, height: 24)
        field.widthAnchor.constraint(equalToConstant: 360).isActive = true
        return field
    }

    private static func stack(_ views: [NSView]) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.frame = NSRect(x: 0, y: 0, width: 360, height: stack.fittingSize.height)
        return stack
    }

    private static func squashBox(_ on: Bool) -> NSButton {
        let box = NSButton(checkboxWithTitle: L("Squash its history into one commit (--squash)"), target: nil, action: nil)
        box.state = on ? .on : .off
        return box
    }

    private static func trimmed(_ field: NSTextField) -> String {
        field.stringValue.trimmingCharacters(in: .whitespaces)
    }

    /// Add Subtree…: 別のリポジトリの ref を、このリポジトリのフォルダに取り込む。squash の既定は git と同じくオフ。
    static func add(in window: NSWindow, actions: Actions, perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = L("Add Subtree")
        alert.informativeText = actions.sheetMessage(L("Brings another repository’s branch into a folder of this one as a commit (git subtree add). Pull and Push later use the same repository and branch."))
        let url = field("", placeholder: L("Repository URL"))
        let ref = field("", placeholder: L("Branch or tag"))
        let dir = field("", placeholder: L("Folder"))
        let squash = squashBox(false)
        alert.accessoryView = stack([url, ref, dir, squash])
        alert.window.initialFirstResponder = url
        let target = ClosureTarget {
            let name = SubmoduleSheet.defaultPath(for: url.stringValue)
            dir.placeholderString = name.isEmpty ? L("Folder") : name
        }
        NotificationCenter.default.addObserver(forName: NSControl.textDidChangeNotification, object: url, queue: .main) { _ in
            MainActor.assumeIsolated { target.fire() }
        }
        alert.addButton(withTitle: L("Add"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            withExtendedLifetime(target) {}
            guard response == .alertFirstButtonReturn else { return }
            let u = trimmed(url), r = trimmed(ref)
            var d = trimmed(dir)
            if d.isEmpty { d = SubmoduleSheet.defaultPath(for: u) }
            guard !u.isEmpty, !r.isEmpty, !d.isEmpty else { return NSSound.beep() }
            perform(["op": "subtree", "action": ["kind": "add", "dir": d, "url": u, "ref": r, "squash": squash.state == .on]])
        }
    }

    /// Pull…（`push` なら Push…）: 前に使った相手を入れておく。Pull の squash は前の選び方。
    static func pullOrPush(_ push: Bool, in window: NSWindow, actions: Actions, subtree: SubtreeInfo, perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = push ? "Push \(subtree.dir)" : "Pull \(subtree.dir)"
        alert.informativeText = actions.sheetMessage(push
            ? L("Sends the commits that changed this folder to the repository’s branch, as that repository’s own history (git subtree push).")
            : L("Merges the repository’s branch into this folder (git subtree pull). Conflicts stop it like a merge."))
        let url = field(subtree.url ?? "", placeholder: L("Repository URL"))
        let ref = field((push ? subtree.pushRef ?? subtree.ref : subtree.ref) ?? "", placeholder: L("Branch or tag"))
        let squash = squashBox(subtree.squash ?? false)
        alert.accessoryView = stack(push ? [url, ref] : [url, ref, squash])
        alert.window.initialFirstResponder = url.stringValue.isEmpty ? url : ref
        alert.addButton(withTitle: push ? "Push" : "Pull")
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            let u = trimmed(url), r = trimmed(ref)
            guard !u.isEmpty, !r.isEmpty else { return NSSound.beep() }
            var action: [String: Any] = ["kind": push ? "push" : "pull", "dir": subtree.dir, "url": u, "ref": r]
            if !push { action["squash"] = squash.state == .on }
            perform(["op": "subtree", "action": action])
        }
    }

    /// Split…: このフォルダの履歴だけのブランチを作る。
    static func split(in window: NSWindow, actions: Actions, subtree: SubtreeInfo, perform: @escaping ([String: Any]) -> Void) {
        let alert = NSAlert()
        alert.messageText = "Split \(subtree.dir)"
        alert.informativeText = actions.sheetMessage(L("Creates a branch with only the history of this folder, its files at the top (git subtree split)."))
        let name = (subtree.dir as NSString).lastPathComponent
        let branch = field("", placeholder: "\(name)-split")
        alert.accessoryView = branch
        alert.window.initialFirstResponder = branch
        alert.addButton(withTitle: L("Create"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            var b = trimmed(branch)
            if b.isEmpty { b = branch.placeholderString ?? "" }
            perform(["op": "subtree", "action": ["kind": "split", "dir": subtree.dir, "branch": b]])
        }
    }
}
