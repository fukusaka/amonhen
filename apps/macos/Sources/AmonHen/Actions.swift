import AmonhenKit
import AppKit

/// 操作の実行。裏で git を動かし、失敗やコンフリクトを知らせ、終わったら画面を読み直す。
final class Actions {
    let repository: Repository
    weak var window: NSWindow?
    /// 操作の後に、History・サイドバー・Working Copy を読み直す。
    var onChange: (() -> Void)?
    /// 次に Undo される操作の名前が変わった。
    var onUndoChange: ((String?) -> Void)?
    /// 操作が失敗した（ウィンドウの帯で知らせる）。なければ警告のシートを出す。
    var onFailure: ((String) -> Void)?

    init(repository: Repository) {
        self.repository = repository
    }

    /// `command` を実行する（`crates/core/src/command.rs` の `Command`）。
    func perform(_ command: [String: Any], completion: ((CommandResult) -> Void)? = nil) {
        let repository = self.repository
        RunningOperations.begin()
        Task {
            defer { RunningOperations.end() }
            do {
                let json = try Repository.json(command)
                let result = try await Task.detached(priority: .userInitiated) { try repository.run(json: json) }.value
                switch result.outcome {
                case .conflict:
                    await alert(L("There are conflicts"), L("Resolve them in Working Copy, then Continue or Abort."))
                case .stopped:
                    await alert(L("Stopped partway"), L("Check Working Copy, then Continue."))
                default:
                    break
                }
                completion?(result)
            } catch {
                await fail(error)
            }
            onChange?()
            refreshUndo()
        }
    }

    /// 命令を順に実行する。失敗したり、途中で止まったりしたら、そこでやめる。
    func perform(sequence commands: [[String: Any]]) {
        guard let first = commands.first else { return }
        perform(first) { [weak self] result in
            if result.outcome == nil || result.outcome == .done {
                self?.perform(sequence: Array(commands.dropFirst()))
            }
        }
    }

    /// 裏で実行し、終わったら読み直す（JSON の命令以外の操作）。
    func perform(_ work: @escaping @Sendable () throws -> Void) {
        RunningOperations.begin()
        Task {
            defer { RunningOperations.end() }
            do {
                try await Task.detached(priority: .userInitiated) { try work() }.value
            } catch {
                await fail(error)
            }
            onChange?()
            refreshUndo()
        }
    }

    func refreshUndo() {
        let repository = self.repository
        Task {
            let label = try? await Task.detached { try repository.run(["op": "undoPeek"]).label }.value
            onUndoChange?(label ?? nil)
        }
    }

    func undo() {
        perform(["op": "undo"])
    }

    @MainActor
    private func fail(_ error: Error) async {
        if let onFailure {
            onFailure("\(error)")
        } else {
            await alert(L("The operation failed"), "\(error)")
        }
    }

    @MainActor
    private func alert(_ title: String, _ message: String) async {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        if let window {
            _ = await alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }

    /// 名前を入れてもらう。キャンセルなら nil。
    func ask(_ title: String, message: String = "", initial: String = "", ok: String = "OK") -> String? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: L("Cancel"))
        let field = NSTextField(string: initial)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let value = field.stringValue.trimmingCharacters(in: .whitespaces)
        return value.isEmpty ? nil : value
    }

    /// 取り消せない操作の前に確かめる。
    func confirm(_ title: String, message: String, ok: String) -> Bool {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: L("Cancel"))
        return alert.runModal() == .alertFirstButtonReturn
    }

    /// 複数行のメッセージを編集してもらう。キャンセルなら nil。
    func editMessage(_ title: String, initial: String, ok: String) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: L("Cancel"))
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 460, height: 180))
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        let text = NSTextView(frame: scroll.bounds)
        text.isRichText = false
        text.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        text.isAutomaticQuoteSubstitutionEnabled = false
        text.autoresizingMask = [.width]
        text.string = initial
        scroll.documentView = text
        alert.accessoryView = scroll
        alert.window.initialFirstResponder = text
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let value = text.string.trimmingCharacters(in: .whitespacesAndNewlines)
        return value.isEmpty ? nil : value
    }

    /// 命令を実行して `text` を受け取る（メッセージの初期値など）。
    private func text(_ command: [String: Any]) async -> String? {
        let repository = self.repository
        do {
            let json = try Repository.json(command)
            return try await Task.detached { try repository.run(json: json).text }.value
        } catch {
            await alert(L("Can’t Read"), "\(error)")
            return nil
        }
    }

    func rewrite(_ label: String, _ edit: [String: Any]) {
        perform(["op": "rewrite", "label": label, "edit": edit])
    }

    func reword(_ oid: String) {
        Task {
            guard let message = await text(["op": "commitMessage", "oid": oid]),
                  let edited = editMessage(L("Edit Message"), initial: message, ok: L("Change")) else { return }
            rewrite("Reword \(oid.prefix(8))", ["kind": "reword", "oid": oid, "message": edited])
        }
    }

    func squash(_ oid: String) {
        Task {
            guard let message = await text(["op": "squashMessage", "oid": oid]),
                  let edited = editMessage("Squash into Parent", initial: message, ok: "Squash") else { return }
            rewrite("Squash \(oid.prefix(8))", ["kind": "squash", "oid": oid, "message": edited])
        }
    }

    /// 選んだ連続したコミットを 1 つにまとめる。メッセージは古い順に並べたものを初期値に編集してもらう。
    func squash(range oids: [String]) {
        let repository = self.repository
        Task {
            let result: Result<SquashRangeInfo, Error>
            do {
                let json = try Repository.json(["op": "squashRangeMessage", "oids": oids])
                result = await Task.detached { Result { try repository.info(json: json, as: SquashRangeInfo.self) } }.value
            } catch {
                result = .failure(error)
            }
            switch result {
            case let .failure(error):
                await alert(L("Can’t Squash"), "\(error)")
            case let .success(info):
                let title = "Squash \(oids.count) Commits"
                guard let edited = editMessage(title, initial: info.message, ok: "Squash") else { return }
                rewrite(title, ["kind": "squashRange", "oid": info.oldest, "last": info.newest, "message": edited])
            }
        }
    }

    /// 編集できるコミット（新しい順）。
    func editableCommits() async -> [String] {
        await text(["op": "editableCommits"])?.split(separator: "\n").map(String.init) ?? []
    }

    // MARK: - よく使う操作

    func newBranch(at start: String) {
        guard let name = ask("New Branch", message: L("Name of the branch to create from %@", start), ok: L("Create and Switch")) else { return }
        perform(["op": "createBranch", "name": name, "start": start, "checkout": true])
    }

    func newTag(at target: String) {
        guard let name = ask("New Tag", message: L("Name of the tag for %@", target), ok: L("Create")) else { return }
        perform(["op": "createTag", "name": name, "target": target])
    }

    func fetch() {
        perform(["op": "fetch"])
    }

    /// 作業中の変更を stash する。untracked のファイルを含めるかを選べる。
    func stash() {
        let alert = NSAlert()
        alert.messageText = "Stash Changes"
        alert.informativeText = L("Message (optional)")
        alert.addButton(withTitle: "Stash")
        alert.addButton(withTitle: L("Cancel"))
        let field = NSTextField(frame: NSRect(x: 0, y: 30, width: 300, height: 24))
        let untracked = NSButton(checkboxWithTitle: L("Include untracked files"), target: nil, action: nil)
        untracked.frame = NSRect(x: 0, y: 0, width: 300, height: 22)
        let box = NSView(frame: NSRect(x: 0, y: 0, width: 300, height: 56))
        box.addSubview(field)
        box.addSubview(untracked)
        alert.accessoryView = box
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        var command: [String: Any] = ["op": "stashPush", "includeUntracked": untracked.state == .on]
        let message = field.stringValue.trimmingCharacters(in: .whitespaces)
        if !message.isEmpty { command["message"] = message }
        perform(command)
    }

    /// 今のブランチを上流に push する。上流がなければ origin に同じ名前で作る。
    func push(force: Bool = false) {
        let repository = self.repository
        Task {
            guard let status = try? await Task.detached(operation: { try repository.loadStatus() }).value,
                  let branch = status.branch else {
                await alert(L("Can’t Push"), L("No branch is checked out."))
                return
            }
            if let upstream = status.upstream, let slash = upstream.firstIndex(of: "/") {
                let remote = String(upstream[..<slash])
                let remoteBranch = String(upstream[upstream.index(after: slash)...])
                perform(["op": "push", "remote": remote, "branch": branch, "remoteBranch": remoteBranch,
                         "force": force, "setUpstream": false])
            } else {
                perform(["op": "push", "remote": "origin", "branch": branch, "remoteBranch": branch,
                         "force": force, "setUpstream": true])
            }
        }
    }
}

/// `squashRangeMessage` の結果: まとめる範囲の両端と、メッセージの初期値。
nonisolated struct SquashRangeInfo: Decodable, Sendable {
    let oldest: String
    let newest: String
    let message: String
}
