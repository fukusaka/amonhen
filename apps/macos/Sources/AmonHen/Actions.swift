import AmonhenKit
import AppKit

/// 操作の実行。裏で git を動かし、失敗やコンフリクトを知らせ、終わったら画面を読み直す。
final class Actions {
    let repository: Repository
    weak var window: NSWindow?
    /// 操作の後に、History・サイドバー・Working Copy を読み直す。
    var onChange: (() -> Void)?
    /// fetch・pull が終わった（commit-graph に増えたコミットを書き足す）。
    var onFetch: (() -> Void)?
    /// 次に Undo される操作の名前が変わった。
    var onUndoChange: ((String?) -> Void)?
    /// 操作がコンフリクトか途中で止まった（Working Copy を開いて最初のコンフリクトを選ぶ）。なければ警告のシートを出す。
    var onStop: (() -> Void)?
    /// 操作が失敗した（ウィンドウの帯で知らせる）。なければ警告のシートを出す。
    var onFailure: ((OperationFailure) -> Void)?
    /// 書き込む先の worktree の名前（worktree が 2 つ以上あるときだけ）。確認のシートに出す。
    var worktreeName: String?
    /// 書き込む先の worktree のブランチ（detached HEAD なら nil）。reset の確認に出す。
    var currentBranch: String?
    /// Push のシートに埋め込む詳細の欄（メッセージ・変更ファイル・差分）を作る。Metal の描画器を持つウィンドウが渡す。
    var makeDetailView: (() -> CommitDetailViewController?)?

    init(repository: Repository) {
        self.repository = repository
    }

    /// `command` を実行する（`crates/core/src/command.rs` の `Command`）。失敗したら、`recover` が引き受けなければ
    /// （false を返したら）知らせる。
    func perform(_ command: [String: Any], completion: ((CommandResult) -> Void)? = nil,
                 recover: ((OperationFailure) async -> Bool)? = nil) {
        let repository = self.repository
        RunningOperations.begin()
        Task {
            defer { RunningOperations.end() }
            do {
                let json = try Repository.json(command)
                let result = try await Task.detached(priority: .userInitiated) { try repository.run(json: json) }.value
                switch result.outcome {
                case .conflict where onStop != nil && !result.remaining.isEmpty:
                    // 1 つずつの merge が途中で止まった。まだ merge していないものは、ここでしか言えない。
                    onStop?()
                    await alert(L("Not merged yet: %@", ListFormatter.localizedString(byJoining: result.remaining)),
                                L("Merge them again after resolving these conflicts."))
                case .conflict where onStop != nil, .stopped where onStop != nil:
                    // どこで止まったか・次に何をするかはウィンドウの帯に出る。Working Copy でコンフリクトを開く。
                    onStop?()
                case .conflict where (command["op"] as? String) == "am":
                    // 当たらずに止まったのか、3-way でコンフリクトを残したのかは、帯に出す。
                    await alert(L("Stopped at a Patch"), L("The bar under the toolbar says why and what you can do next."))
                case .conflict where !result.remaining.isEmpty:
                    // 1 つずつの merge が途中で止まった。残りは、解決してから改めて merge する。
                    await alert(L("There are conflicts"), L("Resolve them in Working Copy, then Continue or Abort. Not merged yet: %@.",
                                                            ListFormatter.localizedString(byJoining: result.remaining)))
                case .conflict:
                    await alert(L("There are conflicts"), L("Resolve them in Working Copy, then Continue or Abort."))
                case .stopped:
                    await alert(L("Stopped partway"), L("Check Working Copy, then Continue."))
                default:
                    break
                }
                completion?(result)
                if let op = command["op"] as? String, op == "fetch" || op == "pull" { onFetch?() }
            } catch {
                let failure = OperationFailure(error, command: command)
                if await recover?(failure) != true { await fail(failure) }
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
                await fail(OperationFailure(error))
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
    func fail(_ failure: OperationFailure) async {
        if let onFailure {
            onFailure(failure)
        } else {
            await alert(failure.title, [failure.summary, failure.advice, failure.kind == nil ? nil : failure.message]
                .compactMap { $0 }.joined(separator: "\n\n"))
        }
    }

    @MainActor
    func alert(_ title: String, _ message: String) async {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        if let window {
            _ = await alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }

    /// 確認の文の終わりに置く定型（docs/ux.md の「Confirmations and wording」）。この 2 つ以外の言い方はしない。
    static let canUndo = L("You can Undo this (⌥⌘Z).")
    static let cantUndo = L("This can’t be undone.")

    /// 名前を入れてもらう（ウィンドウのシートで）。キャンセルなら nil。
    @MainActor
    func ask(_ title: String, message: String = "", initial: String = "", ok: String) async -> String? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: L("Cancel"))
        let field = NSTextField(string: initial)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard await present(alert) == .alertFirstButtonReturn else { return nil }
        let value = field.stringValue.trimmingCharacters(in: .whitespaces)
        return value.isEmpty ? nil : value
    }

    /// 実行の前に確かめる（ウィンドウのシートで）。`destructive` は元に戻せないもの（ボタンを赤く、↩ では押せない）。
    @MainActor
    func confirm(_ title: String, message: String, ok: String, destructive: Bool = false, in host: NSWindow? = nil) async -> Bool {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        let button = alert.addButton(withTitle: ok)
        button.hasDestructiveAction = destructive
        alert.addButton(withTitle: L("Cancel"))
        if destructive {
            button.keyEquivalent = ""
            alert.buttons[1].keyEquivalent = "\r"
        }
        if let host { return await alert.beginSheetModal(for: host) == .alertFirstButtonReturn }
        return await present(alert) == .alertFirstButtonReturn
    }

    /// 複数行のメッセージを編集してもらう（ウィンドウのシートで）。キャンセルなら nil。
    @MainActor
    func editMessage(_ title: String, initial: String, ok: String) async -> String? {
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
        guard await present(alert) == .alertFirstButtonReturn else { return nil }
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
                  let edited = await editMessage(L("Edit Message"), initial: message, ok: L("Change")) else { return }
            rewrite("Reword \(oid.prefix(8))", ["kind": "reword", "oid": oid, "message": edited])
        }
    }

    func squash(_ oid: String) {
        Task {
            guard let message = await text(["op": "squashMessage", "oid": oid]),
                  let edited = await editMessage("Squash into Parent", initial: message, ok: "Squash") else { return }
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
                guard let edited = await editMessage(title, initial: info.message, ok: "Squash") else { return }
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
        Task { @MainActor in
            guard let name = await ask(L("New Branch from %@", start), message: L("Name of the branch. It is checked out."),
                                       ok: L("Create")) else { return }
            perform(["op": "createBranch", "name": name, "start": start, "checkout": true])
        }
    }

    func renameBranch(_ name: String) {
        Task { @MainActor in
            guard let to = await ask(L("Rename %@", name), message: L("New name of the branch."), initial: name, ok: "Rename"),
                  to != name else { return }
            perform(["op": "renameBranch", "from": name, "to": to])
        }
    }

    func newTag(at target: String) {
        Task { @MainActor in
            guard let name = await ask(L("New Tag at %@", target), message: L("Name of the tag."), ok: L("Create")) else { return }
            perform(["op": "createTag", "name": name, "target": target])
        }
    }

    func fetch() {
        perform(["op": "fetch"])
    }

    /// 今のブランチ（detached HEAD なら HEAD）を `target` に動かす。動かし方（soft・mixed・hard）は確認のシートで選ぶ。
    func reset(to target: String) {
        Task { @MainActor in
            let short = String(target.prefix(8))
            let name = currentBranch ?? "HEAD"
            let alert = NSAlert()
            alert.messageText = L("Reset %@ to %@?", name, short)
            alert.informativeText = L("Commits taken off stay in Reflog.") + " " + Self.canUndo
            alert.addButton(withTitle: "Reset")
            alert.addButton(withTitle: L("Cancel"))
            let modes: [(String, String)] = [
                ("soft", L("Soft: keep the changes staged")),
                ("mixed", L("Mixed: keep the changes in the working tree")),
                ("hard", L("Hard: match the commit (uncommitted changes are stashed first)")),
            ]
            let target_ = ClosureTarget {}
            let radios = modes.map { NSButton(radioButtonWithTitle: $0.1, target: target_, action: #selector(ClosureTarget.fire)) }
            radios[1].state = .on
            let stack = NSStackView(views: radios)
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 4
            stack.frame.size = stack.fittingSize
            alert.accessoryView = stack
            let response = await present(alert)
            withExtendedLifetime(target_) {}
            guard response == .alertFirstButtonReturn,
                  let mode = zip(radios, modes).first(where: { $0.0.state == .on })?.1.0 else { return }
            perform(["op": "reset", "oid": target, "mode": mode])
        }
    }

    /// 右クリックの「Reset … to …」。History と Reflog で使う。
    func resetMenuItem(to target: String) -> NSMenuItem {
        ClosureMenuItem(title: "Reset \(currentBranch ?? "HEAD") to \(target.prefix(8))…") { [weak self] in self?.reset(to: target) }
    }

    /// 作業中の変更を stash する。untracked のファイルを含めるかを選べる。
    func stash() {
        Task { @MainActor in await stashSheet() }
    }

    @MainActor
    private func stashSheet() async {
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
        guard await present(alert) == .alertFirstButtonReturn else { return }
        var command: [String: Any] = ["op": "stashPush", "includeUntracked": untracked.state == .on]
        let message = field.stringValue.trimmingCharacters(in: .whitespaces)
        if !message.isEmpty { command["message"] = message }
        perform(command)
    }

    /// ブランチ（nil なら今のブランチ）を上流に push する（確認は済んでいる）。`expect` は Force Push で置き換えてよい
    /// リモートのコミット。送り終えたら `then`。
    func push(branch: String, force: Bool, expect: String?, then: (() -> Void)? = nil) {
        let repository = self.repository
        Task {
            let upstream: String? = await Task.detached {
                if let item = (try? repository.loadRefs())?.items.first(where: { $0.kind == .local && $0.name == branch }) {
                    return item.gone ? nil : item.detail
                }
                return nil
            }.value
            guard let upstream, let slash = upstream.firstIndex(of: "/") else {
                return publishWithConfirmation(branch: branch, then: then)
            }
            var command: [String: Any] = [
                "op": "push", "remote": String(upstream[..<slash]), "branch": branch,
                "remoteBranch": String(upstream[upstream.index(after: slash)...]), "force": force, "setUpstream": false,
            ]
            if force, let expect { command["expect"] = expect }
            perform(command) { _ in then?() }
        }
    }

    /// Commit and Push の push。聞かずに送るが、上流がない・上流が先に進んでいるときは、Push のシートを出す（失敗の帯で
    /// 知らせるより、そこで選べるほうがよい）。
    func pushWithoutAsking() {
        let repository = self.repository
        Task {
            let status = try? await Task.detached { try repository.loadStatus() }.value
            guard let branch = status?.branch else { return NSSound.beep() }
            guard let upstream = status?.upstream else { return publishWithConfirmation(branch: branch) }
            let behind = await Task.detached {
                (try? repository.info(["op": "between", "base": "HEAD", "tip": upstream, "limit": 1], as: CommitRange.self))?.count ?? 0
            }.value
            if behind > 0 { return pushWithConfirmation() }
            push(branch: branch, force: false, expect: nil)
        }
    }
}

/// `squashRangeMessage` の結果: まとめる範囲の両端と、メッセージの初期値。
nonisolated struct SquashRangeInfo: Decodable, Sendable {
    let oldest: String
    let newest: String
    let message: String
}
