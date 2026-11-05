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
    /// 操作が最後まで終わった（ステータスバーに一言出し、前の失敗の行を消す）。
    var onSuccess: (([String: Any]) -> Void)?
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
    /// （false を返したら）知らせる。`recover` は画面を触るので、メインスレッドで呼ぶ。
    func perform(_ command: [String: Any], completion: ((CommandResult) -> Void)? = nil,
                 recover: (@MainActor (OperationFailure) async -> Bool)? = nil) {
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
                if result.outcome == nil || result.outcome == .done { onSuccess?(command) }
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

    /// `updateRefs` は、積み重ねたブランチを一緒に動かすかのこの操作だけの指定（nil なら git の設定のとおり）。
    /// `failed` を渡すと、書き換えられなかったときに（失敗を知らせた後で）呼ぶ。`completion` は書き換えたとき
    /// （途中で止まったときも）。
    func rewrite(_ label: String, _ edit: [String: Any], updateRefs: Bool? = nil,
                 completion: ((CommandResult) -> Void)? = nil, failed: (() -> Void)? = nil) {
        var command: [String: Any] = ["op": "rewrite", "label": label, "edit": edit]
        if let updateRefs { command["updateRefs"] = updateRefs }
        var recover: (@MainActor (OperationFailure) async -> Bool)?
        if let failed {
            // 失敗はいつもどおり知らせる（false）。
            recover = { _ in
                failed()
                return false
            }
        }
        perform(command, completion: completion, recover: recover)
    }

    /// 編集できるコミット（新しい順）。
    func editableCommits() async -> [String] {
        await text(["op": "editableCommits"])?.split(separator: "\n").map(String.init) ?? []
    }

    /// HEAD から first-parent で辿ったコミット（新しい順、merge を越えて）と、merge かどうか（History の編集モード）。
    func firstParentCommits() async -> [(oid: String, merge: Bool)] {
        (await text(["op": "editableCommits", "throughMerges": true])?.split(separator: "\n") ?? []).map { line in
            let parts = line.split(separator: " ")
            return (String(parts[0]), parts.count > 1)
        }
    }

    // MARK: - よく使う操作

    /// 題に出す起点: コミットの完全な ID（40・64 桁）は短い ID に（メニューの「New Branch from …」と同じ）。
    nonisolated static func shownStart(_ rev: String) -> String {
        let full = (rev.count == 40 || rev.count == 64) && rev.allSatisfy(\.isHexDigit)
        return full ? String(rev.prefix(Titles.shortIDLength)) : rev
    }

    func newBranch(at start: String) {
        Task { @MainActor in
            guard let name = await ask(L("New Branch from %@", Self.shownStart(start)), message: L("Name of the branch. It is checked out."),
                                       ok: L("Create")) else { return }
            perform(["op": "createBranch", "name": name, "start": start, "checkout": true])
        }
    }

    /// 履歴のない新しいブランチに切り替える（gh-pages など）。既定は git switch --orphan と同じく空から。今のファイルを
    /// 残せば（git checkout --orphan）、stage したまま最初のコミットの材料になる。ブランチは最初のコミットでできる。
    func newOrphanBranch() {
        Task { @MainActor in
            let alert = makeAlert(L("New Orphan Branch"), L("A branch with no history. It is checked out, and is created by its first commit."))
            let name = NSTextField(string: "")
            name.placeholderString = L("Name of the branch")
            name.widthAnchor.constraint(equalToConstant: 320).isActive = true
            let keep = NSButton(checkboxWithTitle: L("Keep the current files, staged for the first commit"), target: nil, action: nil)
            let stack = NSStackView(views: [name, keep])
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 8
            stack.frame = NSRect(x: 0, y: 0, width: 320, height: stack.fittingSize.height)
            alert.accessoryView = stack
            alert.window.initialFirstResponder = name
            alert.addButton(withTitle: L("Create"))
            alert.addButton(withTitle: L("Cancel"))
            guard await present(alert) == .alertFirstButtonReturn else { return }
            let branch = name.stringValue.trimmingCharacters(in: .whitespaces)
            guard !branch.isEmpty else { return NSSound.beep() }
            perform(["op": "createOrphanBranch", "name": branch, "keepFiles": keep.state == .on])
        }
    }

    func renameBranch(_ name: String) {
        Task { @MainActor in
            guard let to = await ask(L("Rename %@", name), message: L("New name of the branch."), initial: name, ok: L("Rename")),
                  to != name else { return }
            perform(["op": "renameBranch", "from": name, "to": to])
        }
    }

    func newTag(at target: String) {
        Task { @MainActor in
            guard let name = await ask(L("New Tag at %@", Self.shownStart(target)), message: L("Name of the tag."), ok: L("Create")) else { return }
            perform(["op": "createTag", "name": name, "target": target])
        }
    }

    /// `remote` から取ってくる（nil ならすべてのリモート）。
    func fetch(remote: String? = nil) {
        var command: [String: Any] = ["op": "fetch"]
        if let remote { command["remote"] = remote }
        perform(command)
    }

    /// 今のブランチ（detached HEAD なら HEAD）を `target` に動かす。動かし方（soft・mixed・hard）は確認のシートで選ぶ。
    func reset(to target: String) {
        Task { @MainActor in
            let short = String(target.prefix(8))
            let name = currentBranch ?? "HEAD"
            let alert = makeAlert(L("Reset %@ to %@?", name, short), L("Commits taken off stay in Reflog.") + " " + Self.canUndo)
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
        ClosureMenuItem(title: Titles.reset(currentBranch ?? "HEAD", to: String(target.prefix(Titles.shortIDLength)))) { [weak self] in self?.reset(to: target) }
    }

    /// ignore されたファイル（ビルドの出力など）を消す（git clean -X）。消すものを先に一覧で見せる。git の記録に
    /// ないので戻せない（赤いボタン）。
    func deleteIgnoredFiles() {
        let repository = self.repository
        Task { @MainActor in
            nonisolated struct Ignored: Decodable, Sendable { let paths: [String] }
            let paths: [String]
            do {
                paths = try await Task.detached(priority: .userInitiated) {
                    try repository.info(["op": "ignoredFiles"], as: Ignored.self)
                }.value.paths
            } catch {
                NSAlert(error: error).runModal()
                return
            }
            let alert = makeAlert(paths.isEmpty ? L("No Ignored Files") : L("Delete %ld Ignored Items?", paths.count),
                                  paths.isEmpty ? L("Nothing in the working tree matches .gitignore.")
                                      : L("These are removed from the working tree (git clean -X); folders go with everything in them.") + " " + Self.cantUndo)
            if paths.isEmpty {
                alert.addButton(withTitle: "OK")
                _ = await present(alert)
                return
            }
            let list = NSTextView(frame: NSRect(x: 0, y: 0, width: 380, height: 160))
            list.string = paths.joined(separator: "\n")
            list.isEditable = false
            list.font = .monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
            let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 380, height: 160))
            scroll.documentView = list
            scroll.hasVerticalScroller = true
            scroll.borderType = .bezelBorder
            alert.accessoryView = scroll
            let delete = alert.addButton(withTitle: L("Delete"))
            delete.hasDestructiveAction = true
            delete.keyEquivalent = ""
            alert.addButton(withTitle: L("Cancel")).keyEquivalent = "\r"
            guard await present(alert) == .alertFirstButtonReturn else { return }
            perform(["op": "deleteIgnored", "paths": paths])
        }
    }

    /// 作業中の変更を stash する。`paths` があれば、そのファイルの変更だけ。何を stash するか（すべて・stage した
    /// 変更だけ）、stage した変更を残すか、untracked のファイルを含めるかを選べる（既定は git と同じく、追跡している
    /// ファイルの変更をすべて）。
    func stash(paths: [String] = []) {
        Task { @MainActor in await stashSheet(paths: paths) }
    }

    @MainActor
    private func stashSheet(paths: [String]) async {
        let alert = NSAlert()
        alert.messageText = paths.isEmpty ? "Stash Changes"
            : paths.count == 1 ? "Stash Changes in \((paths[0] as NSString).lastPathComponent)" : "Stash Changes in \(paths.count) Files"
        alert.informativeText = L("Message (optional)")
        alert.addButton(withTitle: "Stash")
        alert.addButton(withTitle: L("Cancel"))
        let field = NSTextField(string: "")
        field.widthAnchor.constraint(equalToConstant: 320).isActive = true
        // 何を: すべての変更か、stage した変更だけ（git 2.35 から。古い git ならコアがわけを言って断る）。
        let all = NSButton(radioButtonWithTitle: L("All changes"), target: nil, action: nil)
        let staged = NSButton(radioButtonWithTitle: L("Only the staged changes"), target: nil, action: nil)
        all.state = .on
        let keepIndex = NSButton(checkboxWithTitle: L("Keep the staged changes in the working tree (--keep-index)"), target: nil, action: nil)
        let untracked = NSButton(checkboxWithTitle: L("Include untracked files"), target: nil, action: nil)
        // stage した変更だけのときは、ほかの選び方は git が受けない。
        let target = ClosureTarget {
            let onlyStaged = staged.state == .on
            keepIndex.isEnabled = !onlyStaged
            untracked.isEnabled = !onlyStaged
            if onlyStaged { keepIndex.state = .off; untracked.state = .off }
        }
        for radio in [all, staged] {
            radio.target = target
            radio.action = #selector(ClosureTarget.fire)
        }
        let stack = NSStackView(views: [field, all, staged, keepIndex, untracked])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        stack.frame = NSRect(x: 0, y: 0, width: 320, height: stack.fittingSize.height)
        alert.accessoryView = stack
        alert.window.initialFirstResponder = field
        let response = await present(alert)
        withExtendedLifetime(target) {}
        guard response == .alertFirstButtonReturn else { return }
        var command: [String: Any] = [
            "op": "stashPush", "includeUntracked": untracked.state == .on, "keepIndex": keepIndex.state == .on,
            "staged": staged.state == .on, "paths": paths,
        ]
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
            // 読めなければ知らせて止める（遅れているかを 0 件とみなして聞かずに送ると、分かれたブランチの push が
            // 失敗の帯で終わる。Push のシートと同じ）。
            let read = await Task.detached { () -> Result<(WorkingStatus, Int?), any Error> in
                do {
                    let status = try repository.loadStatus()
                    guard let upstream = status.upstream else { return .success((status, nil)) }
                    let behind = try repository.info(["op": "between", "base": "HEAD", "tip": upstream, "limit": 1],
                                                     as: CommitRange.self).count
                    return .success((status, behind))
                } catch {
                    return .failure(error)
                }
            }.value
            let status: WorkingStatus, behind: Int?
            switch read {
            case let .success(value): (status, behind) = value
            case let .failure(error):
                await inform(L("Can’t read the status"), "\(error)")
                return
            }
            guard let branch = status.branch else { return NSSound.beep() }
            guard status.upstream != nil, let behind else { return publishWithConfirmation(branch: branch) }
            if behind > 0 { return pushWithConfirmation() }
            push(branch: branch, force: false, expect: nil)
        }
    }
}

