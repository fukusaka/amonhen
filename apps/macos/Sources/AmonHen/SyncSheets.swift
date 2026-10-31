import AmonhenKit
import AppKit

/// `between` の答え: 片側にだけあるコミット（新しい順）。
nonisolated struct CommitRange: Decodable, Sendable {
    struct Commit: Decodable, Sendable {
        let oid: String
        let subject: String
        let author: String
        let time: Int64
    }

    let count: Int
    let commits: [Commit]

    static let empty = CommitRange(count: 0, commits: [])
}

/// `predictMerge` の答え: マージしたらコンフリクトするファイル。git が古くて調べられなければ nil。
nonisolated struct MergeForecast: Decodable, Sendable {
    /// 取り込むもの 1 つずつの見込み（いくつも取り込むときは、前のものまでを合わせた結果に重ねたもの）。
    struct Step: Decodable, Sendable {
        let rev: String
        let conflicts: [String]
    }

    let conflicts: [String]?
    let steps: [Step]?

    var hasConflicts: Bool { conflicts?.isEmpty == false }

    /// 見せる文。`estimate` なら、rebase の目安だと書き添える（rebase はコミットを 1 つずつ載せ直すので、途中で
    /// 止まることも、最後の結果では重ならないこともある）。
    func text(estimate: Bool) -> String {
        guard let conflicts else { return L("Can’t check for conflicts ahead (needs git 2.38 or newer).") }
        let names = conflicts.prefix(3).joined(separator: ", ") + (conflicts.count > 3 ? ", …" : "")
        let line = conflicts.isEmpty ? L("No conflicts expected.") : L("⚠︎ Conflicts expected in %ld files: %@", conflicts.count, names)
        return estimate ? line + " " + L("Rebase applies the commits one at a time, so this is only an estimate.") : line
    }
}

/// `rewritePreview` の答え: 書き換えで一緒に動くブランチと残るブランチ、push 済みのもの、コンフリクトの見込み。
nonisolated struct RewritePreview: Decodable, Sendable {
    struct Branch: Decodable, Sendable {
        let name: String
        let upstream: String?
        let reason: String?
        let worktree: String?
    }

    struct Conflict: Decodable, Sendable {
        let oid: String
        let files: [String]
    }

    let moving: [Branch]
    let staying: [Branch]
    let current: String?
    let currentUpstream: String?
    /// 調べられなければ nil。
    let conflicts: [Conflict]?

    /// 確認とステータスバーに出す行（ブランチ、push 済み、コンフリクト）。
    func lines() -> [String] {
        let names = { (list: [Branch]) in ListFormatter.localizedString(byJoining: list.map(\.name)) }
        var out: [String] = []
        if !moving.isEmpty { out.append(L("Also moves %@.", names(moving))) }
        let stay = { (reason: String) in staying.filter { $0.reason == reason } }
        if !stay("setting").isEmpty { out.append(L("%@ stays where it is (rebase.updateRefs is off).", names(stay("setting")))) }
        if !stay("git").isEmpty { out.append(L("%@ stays where it is (moving it needs git 2.38).", names(stay("git")))) }
        if !stay("checkedOut").isEmpty { out.append(L("%@ stays where it is (checked out in another worktree).", names(stay("checkedOut")))) }
        let pushed = ([currentUpstream.map { _ in current ?? "" }].compactMap { $0 } + moving.filter { $0.upstream != nil }.map(\.name))
            .filter { !$0.isEmpty }
        if !pushed.isEmpty {
            let list = ListFormatter.localizedString(byJoining: pushed)
            out.append(pushed.count == 1 ? L("%@ is pushed, so pushing afterwards needs Force Push.", list)
                : L("%@ are pushed, so pushing afterwards needs Force Push.", list))
        }
        if let conflicts {
            if let first = conflicts.first {
                let files = first.files.prefix(3).joined(separator: ", ") + (first.files.count > 3 ? ", …" : "")
                out.append(conflicts.count == 1
                    ? L("⚠︎ Conflicts expected at %@: %@", String(first.oid.prefix(Titles.shortIDLength)), files)
                    : L("⚠︎ Conflicts expected at %ld commits, first at %@: %@", conflicts.count,
                        String(first.oid.prefix(Titles.shortIDLength)), files))
            } else {
                out.append(L("No conflicts expected."))
            }
        }
        return out
    }

    var hasConflicts: Bool { conflicts?.isEmpty == false }
}

extension Actions {
    /// 書き換える前の見通しを読む（`edit` は History の編集、`onto` は rebase の先）。読めなければ nil。
    func rewritePreview(edit: [String: Any]? = nil, onto: String? = nil) async -> RewritePreview? {
        var command: [String: Any] = ["op": "rewritePreview"]
        if let edit { command["edit"] = edit }
        if let onto { command["onto"] = onto }
        let repository = self.repository
        guard let json = try? Repository.json(command) else { return nil }
        return await Task.detached { try? repository.info(json: json, as: RewritePreview.self) }.value
    }
}

/// Push・Pull・Pop・Undo の前の確認。ボタンを押しただけで、外に出たり履歴が変わったりしないように、何が起きるか
/// （送る・取り込むコミット、分岐、作業中の変更）をシートで見せてから実行する。↩ で実行できる。
extension Actions {
    // MARK: - Push

    /// Push の前に、送るコミットを見せる（1 つのシート）。上流が先に進んでいれば、同じシートで消えるコミットを見せて
    /// Pull… か Force Push を選ばせる。`force` なら上流から消えるコミットを見せる。Force Push は、シートで見せた上流の
    /// コミットのときだけ置き換える。`branch` を渡せば今のブランチ以外も送れる。送り終えたら `then`（Create Pull
    /// Request の続きなど）。
    func pushWithConfirmation(branch: String? = nil, force: Bool = false, then: (() -> Void)? = nil) {
        Task { @MainActor in
            guard let state = await syncState(branch: branch) else { return }
            // detached HEAD・送るものがないときは、ボタンとメニューを無効にしてある（理由はツールチップ）。
            guard let branch = state.branch else { return NSSound.beep() }
            guard let upstream = state.upstream else { return publishWithConfirmation(branch: branch, then: then) }
            let (outgoing, incoming) = (state.outgoing, state.incoming)
            let diverged = !force && incoming.count > 0
            guard force || diverged || outgoing.count > 0 else { return NSSound.beep() }
            if diverged && outgoing.count == 0 { return NSSound.beep() }
            let title: String
            let message: String
            if diverged {
                title = L("%@ has %ld commits you don’t have", upstream, incoming.count)
                message = L("Pull them first, or force push to replace them. Force Push can’t be undone.")
            } else if force {
                title = L("Force push %@?", branch)
                message = incoming.count > 0
                    ? L("%@ loses %ld commits. This can’t be undone.", upstream, incoming.count)
                    : L("It fails if %@ has moved since you last fetched.", upstream)
            } else {
                title = L("Push %ld commits to %@?", outgoing.count, upstream)
                message = ""
            }
            let ok = force || diverged ? "Force Push" : "Push"
            let choice: PushSheet.Choice
            // 送るコミットとその中身を、右の欄と同じ表示で読んでから押す（Metal がなければ一覧だけのシート）。
            if let range = state.range, let detail = makeDetailView?(), let window {
                let sheet = PushSheet(
                    title: title, message: sheetMessage(message), warnings: state.warnings, commits: outgoing,
                    lost: force || diverged ? incoming : nil, range: range, detail: detail, ok: ok,
                    destructive: force || diverged, other: diverged ? "Pull…" : nil)
                choice = await sheet.present(in: window)
            } else {
                let alert = makeAlert(title, message)
                alert.accessoryView = Self.commitLists([
                    (L("Commits to send"), outgoing, false),
                    (L("Commits %@ will lose", upstream), force || diverged ? incoming : .empty, true),
                ])
                alert.addButton(withTitle: ok).hasDestructiveAction = force || diverged
                alert.addButton(withTitle: L("Cancel"))
                if diverged { alert.addButton(withTitle: "Pull…") }
                choice = switch await present(alert) {
                case .alertFirstButtonReturn: .ok
                case .alertThirdButtonReturn: .other
                default: .cancel
                }
            }
            switch choice {
            case .ok: push(branch: branch, force: force || diverged, expect: state.upstreamTarget, then: then)
            case .other: pullWithConfirmation(mode: .git)
            case .cancel: break
            }
        }
    }

    // MARK: - Pull

    /// Pull の前に、取り込むコミットと取り込み方を見せる（1 つのシート）。取り込み方はシートの中で選ぶ（既定は
    /// `mode`）。上流がなければ、同じシートで取り込み元のリモートのブランチを選び、それを上流にして取り込む。
    func pullWithConfirmation(mode: PullMode) {
        Task { @MainActor in
            guard let state = await syncState() else { return }
            // detached HEAD では、ボタンとメニューを無効にしてある。
            guard let branch = state.status.branch else { return NSSound.beep() }
            let repository = self.repository
            // 上流がなければ、取り込み元を選ぶ（同じ名前のリモートのブランチを先に）。
            var sources: [String] = []
            if state.status.upstream == nil {
                let refs = await Task.detached { try? repository.loadRefs() }.value
                let remotes = refs?.items.filter { $0.kind == .remote && !$0.name.hasSuffix("/HEAD") }.map(\.name) ?? []
                sources = remotes.filter { $0.split(separator: "/", maxSplits: 1).last.map(String.init) == branch }
                    + remotes.filter { $0.split(separator: "/", maxSplits: 1).last.map(String.init) != branch }
                guard !sources.isEmpty else {
                    return await inform(L("No remote branch to pull from"), L("Fetch, or add a remote in Repository Settings."))
                }
            }
            let upstream = state.status.upstream
            let (outgoing, incoming) = (state.outgoing, state.incoming)
            let changed = state.status.items.filter { $0.kind != .untracked }.count
            // git の設定（pull.rebase・pull.ff）が決める取り込み方。「git の設定に従う」の中身として見せる。
            struct Defaults: Decodable { let pull: String }
            let byGit = await Task.detached { try? repository.info(["op": "syncDefaults"], as: Defaults.self) }.value
                .flatMap { PullMode(rawValue: $0.pull) } ?? .merge
            // 選んだ取り込み方が実際に行うこと（.git なら git の設定が決める方）。
            let method = { (m: PullMode) -> PullMode in m == .git ? byGit : m }
            let title = upstream.map {
                incoming.count > 0 ? L("Pull %ld commits from %@?", incoming.count, $0) : L("Pull from %@?", $0)
            } ?? L("Pull into %@?", branch)
            let alert = makeAlert(title, "")
            alert.addButton(withTitle: "Pull")
            alert.addButton(withTitle: L("Cancel"))
            let pullButton = alert.buttons[0]

            let source = NSPopUpButton(frame: .zero, pullsDown: false)
            source.addItems(withTitles: sources)
            let popup = NSPopUpButton(frame: .zero, pullsDown: false)
            for m in PullMode.allCases {
                popup.addItem(withTitle: m == .git ? L("%@ (%@)", m.title, byGit.title) : m.title)
                popup.lastItem?.representedObject = m.rawValue
                popup.lastItem?.toolTip = switch m {
                case .merge: L("Brings the upstream commits in. When both sides have new commits, a merge commit joins them.")
                case .rebase: L("Puts your local commits on top of the upstream commits, keeping the History in one line.")
                case .fastForwardOnly: L("Only moves the branch forward. Stops without changing anything if you have local commits too.")
                case .git: L("Uses this repository’s pull.rebase and pull.ff, as git pull in a terminal does.")
                }
            }
            popup.selectItem(at: PullMode.allCases.firstIndex(of: mode) ?? 0)
            let chosen = { PullMode(rawValue: popup.selectedItem?.representedObject as? String ?? "") ?? mode }
            let note = NSTextField(wrappingLabelWithString: "")
            note.preferredMaxLayoutWidth = 440
            note.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            note.textColor = .secondaryLabelColor
            // 今の状態で、選んだ取り込み方が何をするか（1 行）と、作業中の変更、Undo。
            let describe = { (m: PullMode) -> (String, Bool) in
                var lines: [String] = []
                var blocked = false
                if upstream == nil {
                    lines.append(L("Sets %@ as the upstream, then pulls.", source.titleOfSelectedItem ?? ""))
                } else if incoming.count == 0 {
                    lines.append(L("Nothing new as of the last Fetch (Pull fetches first)."))
                } else if outgoing.count == 0 {
                    lines.append(L("Moves %@ forward (fast-forward).", branch))
                } else {
                    switch method(m) {
                    case .merge: lines.append(L("You have %ld local commits, so this makes a merge commit.", outgoing.count))
                    case .rebase: lines.append(L("Your %ld local commits are replayed on top (their IDs change).", outgoing.count))
                    case .fastForwardOnly:
                        lines.append(L("⚠︎ Fast-Forward Only can’t pull these: you have %ld local commits. Choose Merge or Rebase.", outgoing.count))
                        blocked = true
                    case .git: break
                    }
                }
                if changed > 0 { lines.append(L("%ld files have uncommitted changes. Pull stops if the incoming changes touch them.", changed)) }
                lines.append(L("You can Undo this (⌥⌘Z)."))
                return (lines.joined(separator: "\n"), blocked)
            }
            // 説明の高さは、いちばん長い取り込み方に合わせて決めておく（選び直してもシートの大きさを変えない）。
            let height = PullMode.allCases.map { m -> CGFloat in
                note.stringValue = describe(m).0
                return note.fittingSize.height
            }.max() ?? 0
            note.heightAnchor.constraint(equalToConstant: height).isActive = true
            let update = {
                let (text, blocked) = describe(chosen())
                note.stringValue = text
                pullButton.isEnabled = !blocked
            }
            update()
            // 両方に新しいコミットがあるときだけ、取り込むとコンフリクトするかを裏で調べる（fast-forward なら重ならない）。
            // Fast-Forward Only はマージしないので出さない。rebase は最後の結果での目安。
            let forecastLabel = upstream != nil && incoming.count > 0 && outgoing.count > 0 ? Self.forecastLabel() : nil
            final class Answer { var forecast: MergeForecast?; var done = false }
            let answer = Answer()
            let showForecast = {
                guard let forecastLabel else { return }
                let m = method(chosen())
                if m == .fastForwardOnly {
                    forecastLabel.stringValue = ""
                } else if answer.done {
                    Self.show(answer.forecast, estimate: m == .rebase, in: forecastLabel)
                } else {
                    forecastLabel.stringValue = L("Checking for conflicts…")
                }
            }
            showForecast()
            let target = ClosureTarget {
                update()
                showForecast()
            }
            for p in [popup, source] {
                p.target = target
                p.action = #selector(ClosureTarget.fire)
            }
            if forecastLabel != nil, let upstream {
                Task { @MainActor in
                    answer.forecast = await predictMerge(theirs: upstream)
                    answer.done = true
                    showForecast()
                }
            }
            let grid = NSGridView(views: (sources.isEmpty ? [] : [[NSTextField(labelWithString: L("From:")), source]])
                + [[NSTextField(labelWithString: L("Integrate with:")), popup]])
            grid.rowSpacing = 6
            grid.column(at: 0).xPlacement = .trailing
            let stack = NSStackView(views: [grid, note] + (forecastLabel.map { [$0] } ?? []))
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 8
            if incoming.count > 0 || outgoing.count > 0 {
                stack.addArrangedSubview(Self.commitLists([
                    (L("Commits to pull"), incoming, false), (L("Commits only here"), outgoing, false),
                ]))
            }
            stack.layoutSubtreeIfNeeded()
            stack.frame.size = stack.fittingSize
            alert.accessoryView = stack
            let response = await present(alert)
            withExtendedLifetime(target) {}
            guard response == .alertFirstButtonReturn else { return }
            let pull: [String: Any] = ["op": "pull", "mode": chosen().rawValue]
            guard let picked = source.titleOfSelectedItem, upstream == nil, let slash = picked.firstIndex(of: "/") else {
                return perform(pull)
            }
            perform(sequence: [
                ["op": "configSet", "key": "branch.\(branch).remote", "value": String(picked[..<slash])],
                ["op": "configSet", "key": "branch.\(branch).merge",
                 "value": "refs/heads/" + String(picked[picked.index(after: slash)...])],
                pull,
            ])
        }
    }

    // MARK: - 相手の決まった取り込み（右クリック・ドラッグ）

    /// 相手の決まった Merge・Rebase・Cherry-pick・Revert の前に確かめる。`ours` に `theirs` を取り込んだらコンフリクト
    /// するかを裏で調べて見せる（`estimate` なら rebase のように 1 つずつ載せるので目安）。`commands` を順に実行する。
    func integrateWithConfirmation(_ title: String, ok: String, ours: String = "HEAD", theirs: [String], estimate: Bool = false,
                                   commands: [[String: Any]]) {
        Task { @MainActor in
            let alert = makeAlert(title, Self.canUndo)
            alert.addButton(withTitle: ok)
            alert.addButton(withTitle: L("Cancel"))
            let label = Self.forecastLabel()
            label.frame.size = label.fittingSize
            alert.accessoryView = label
            Task { @MainActor in
                // 今のブランチの rebase なら、積み重ねたブランチが動くか残るかと、コミットごとの見込みを出す。
                if estimate, ours == "HEAD", let rev = theirs.first, let preview = await rewritePreview(onto: rev) {
                    Self.show(preview, in: label)
                    return
                }
                Self.show(await predictMerge(ours: ours, theirs: theirs), estimate: estimate || theirs.count > 1, in: label)
            }
            guard await present(alert) == .alertFirstButtonReturn else { return }
            perform(sequence: commands)
        }
    }

    func mergeWithConfirmation(_ rev: String, into current: String) {
        integrateWithConfirmation(L("Merge %@ into %@?", rev, current), ok: "Merge", theirs: [rev],
                                  commands: [["op": "merge", "rev": rev]])
    }

    func rebaseWithConfirmation(_ current: String, onto rev: String) {
        integrateWithConfirmation(L("Rebase %@ onto %@?", current, rev), ok: "Rebase", theirs: [rev], estimate: true,
                                  commands: [["op": "rebase", "onto": rev]])
    }

    /// サイドバーにドラッグしたブランチを、上流でないリモートのブランチに送る前に確かめる。上流なら Push のシート。
    /// `force` なら、そのリモートのブランチが最後の Fetch の時点のコミットのときだけ置き換える。
    func pushWithConfirmation(branch: String, remote: String, remoteBranch: String, force: Bool, setUpstream: Bool) {
        let repository = self.repository
        Task { @MainActor in
            let refs = await Task.detached { try? repository.loadRefs() }.value
            let target = "\(remote)/\(remoteBranch)"
            let local = refs?.items.first { $0.kind == .local && $0.name == branch }
            if local?.detail == target, local?.gone == false {
                return pushWithConfirmation(branch: branch, force: force)
            }
            let there = refs?.items.first { $0.kind == .remote && $0.name == target }?.target
                .map { $0.map { String(format: "%02x", $0) }.joined() }
            let title = force ? L("Force push %@ to %@?", branch, target) : L("Push %@ to %@?", branch, target)
            let message = force ? L("%@ loses the commits %@ doesn’t have.", target, branch) + " " + Self.cantUndo
                : there == nil ? L("Creates %@.", target) : ""
            guard await confirm(title, message: message, ok: force ? "Force Push" : "Push", destructive: force) else { return }
            var command: [String: Any] = ["op": "push", "remote": remote, "branch": branch, "remoteBranch": remoteBranch,
                                          "force": force, "setUpstream": setUpstream]
            if force, let there { command["expect"] = there }
            perform(command)
        }
    }

    // MARK: - Fetch・Checkout・Cherry-pick・Revert

    /// Fetch の前に、どこから取るかを見せる（↩ で実行）。自動の Fetch は聞かない。
    func fetchWithConfirmation() {
        let repository = self.repository
        Task { @MainActor in
            nonisolated struct Remotes: Decodable { struct Remote: Decodable { let name: String }; let remotes: [Remote] }
            nonisolated struct Entries: Decodable { struct Entry: Decodable { let key: String; let value: String }; let entries: [Entry] }
            let (remotes, entries) = await Task.detached {
                ((try? repository.info(["op": "remotes"], as: Remotes.self))?.remotes.map(\.name) ?? [],
                 (try? repository.info(["op": "configList"], as: Entries.self))?.entries ?? [])
            }.value
            guard !remotes.isEmpty else { return NSSound.beep() }
            let prune = entries.last { $0.key.lowercased() == "fetch.prune" }?.value.lowercased() == "true"
            let alert = makeAlert(L("Fetch from %@?", ListFormatter.localizedString(byJoining: remotes)),
                                  prune ? L("Remote branches deleted there are removed here too (fetch.prune).") : "")
            alert.addButton(withTitle: "Fetch")
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn { fetch() }
        }
    }

    /// 切り替える先。
    enum CheckoutTarget {
        case branch(String)
        /// リモートのブランチを追跡するローカルブランチを作って切り替える。
        case track(String)
        /// コミットを detached HEAD で。
        case detach(String)
    }

    /// Checkout の前に、何が起きるか（作業中の変更はそのまま持って行く、追跡ブランチを作る、detached になる）を見せる。
    func checkoutWithConfirmation(_ target: CheckoutTarget) {
        let repository = self.repository
        Task { @MainActor in
            let changed = await Task.detached {
                (try? repository.loadStatus())?.items.filter { $0.kind != .untracked }.count ?? 0
            }.value
            let (title, command, line): (String, [String: Any], String) = switch target {
            case let .branch(name): (L("Checkout %@?", name), ["op": "checkout", "branch": name], "")
            case let .track(remote):
                (L("Checkout %@?", remote), ["op": "track", "remoteBranch": remote],
                 L("Creates %@ tracking %@.", String(remote.split(separator: "/", maxSplits: 1).last ?? ""), remote))
            case let .detach(oid):
                (L("Checkout %@?", String(oid.prefix(8))), ["op": "detach", "oid": oid], L("HEAD is detached (on no branch)."))
            }
            let lines = [line, changed > 0 ? L("%ld files with uncommitted changes come along.", changed) : ""]
                .filter { !$0.isEmpty }
            let alert = makeAlert(title, lines.joined(separator: " "))
            alert.addButton(withTitle: "Checkout")
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn { perform(command) }
        }
    }

    /// Cherry-pick・Revert の前に確かめる。`commits` は（16 進, merge か）を、実行する順に。
    func pickWithConfirmation(revert: Bool, _ commits: [(hex: String, isMerge: Bool)]) {
        Task { @MainActor in
            let what = commits.count == 1 ? String(commits[0].hex.prefix(8)) : L("%ld commits", commits.count)
            let onto = currentBranch ?? "HEAD"
            let title = revert ? L("Revert %@ on %@?", what, onto) : L("Cherry-pick %@ onto %@?", what, onto)
            let message = (revert ? L("Adds commits that undo them.") : L("Adds copies of them.")) + " " + Self.canUndo
            guard await confirm(title, message: message, ok: revert ? "Revert" : "Cherry-pick") else { return }
            perform(sequence: commits.map { ["op": revert ? "revert" : "cherryPick", "oid": $0.hex, "isMerge": $0.isMerge] })
        }
    }

    // MARK: - Stash・Undo

    /// stash を戻して消す前に、どの stash かを見せる。
    /// stash を作業ツリーに戻す前に、どの stash かを見せる。`pop` なら一覧から消す（コンフリクトしたら残す）。
    func popWithConfirmation(name: String, message: String?, pop: Bool = true) {
        Task { @MainActor in
            let what = pop ? L("Applies it to the working tree and removes it from the list.")
                : L("Applies it to the working tree. It stays in the list.")
            let alert = makeAlert(pop ? L("Pop %@?", name) : L("Apply %@?", name),
                                  (message.map { "“\($0)”\n" } ?? "") + what + " " + Self.canUndo)
            alert.addButton(withTitle: pop ? "Pop" : "Apply")
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn {
                perform(["op": "stashApply", "name": name, "pop": pop])
            }
        }
    }

    /// Undo の前に、何を戻すかを見せる（ツールバーのボタンでは名前がツールチップにしか出ないため）。
    func undoWithConfirmation() {
        let repository = self.repository
        Task { @MainActor in
            let peek = (try? await Task.detached { try repository.undoPeek() }.value) ?? nil
            // 戻すものがなければ、ボタンとメニューを無効にしてある。
            guard let peek else { return NSSound.beep() }
            // 何を戻すかは記録による（ファイルだけを戻す記録は、HEAD とブランチに触らない）。
            let what = switch peek.kind {
            case "files": L("Puts the files back as they were. HEAD and the index stay.")
            case "stash": L("Brings back the dropped stash.")
            case "rename": L("Gives the branch its earlier name back.")
            default: L("Returns HEAD and the branches to where they were.")
            }
            let alert = makeAlert(L("Undo “%@”?", peek.label), what)
            alert.addButton(withTitle: "Undo")
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn { undo() }
        }
    }

    // MARK: - 部品

    /// `theirs` を `ours` にマージしたらコンフリクトするか（裏で `predictMerge`。作業ツリーと index には触らない）。
    /// 調べられなければ nil。
    func predictMerge(ours: String = "HEAD", theirs: String) async -> MergeForecast? {
        await predictMerge(ours: ours, theirs: [theirs])
    }

    /// `theirs` を順に `ours` へ 1 つのコミットでマージしたら（octopus）、それぞれがコンフリクトするか。
    func predictMerge(ours: String = "HEAD", theirs: [String]) async -> MergeForecast? {
        let repository = self.repository
        let theirs: Any = theirs.count == 1 ? theirs[0] : theirs
        guard let json = try? Repository.json(["op": "predictMerge", "ours": ours, "theirs": theirs]) else { return nil }
        return await Task.detached {
            try? repository.info(json: json, as: MergeForecast.self)
        }.value
    }

    /// シートに出す、コンフリクトの見込みの行。最初は「調べています」で、答えが来たら `show` で書き換える。高さは
    /// 3 行分に決めておく（答えが来てもシートの大きさを変えないため）。
    @MainActor
    static func forecastLabel(width: CGFloat = 440) -> NSTextField {
        let label = NSTextField(wrappingLabelWithString: "\n\n")
        label.preferredMaxLayoutWidth = width
        label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        label.textColor = .secondaryLabelColor
        label.maximumNumberOfLines = 3
        label.cell?.truncatesLastVisibleLine = true
        label.heightAnchor.constraint(equalToConstant: label.fittingSize.height).isActive = true
        label.stringValue = L("Checking for conflicts…")
        return label
    }

    /// 見込みの行に、書き換えの見通し（動く・残るブランチ、push 済み、コンフリクト）を書く。
    @MainActor
    static func show(_ preview: RewritePreview, in label: NSTextField) {
        label.stringValue = preview.lines().joined(separator: "\n")
        label.toolTip = label.stringValue
        label.textColor = preview.hasConflicts ? .systemOrange : .secondaryLabelColor
    }

    /// 見込みの行に答えを書く（調べられなかったら空にする）。
    @MainActor
    static func show(_ forecast: MergeForecast?, estimate: Bool, in label: NSTextField) {
        label.stringValue = forecast?.text(estimate: estimate) ?? ""
        label.textColor = forecast?.hasConflicts == true ? .systemOrange : .secondaryLabelColor
    }

    // MARK: - Publish

    /// 上流のないブランチを初めて送る（Publish）。送り先のリモートとそこでの名前を選ばせ、送ったらそれを上流にする。
    /// リモートの初期値は、git の push と同じく branch.<name>.pushRemote・remote.pushDefault の順で、なければ
    /// リモートが 1 つならそれ、いくつもあれば origin。
    func publishWithConfirmation(branch: String, then: (() -> Void)? = nil) {
        let repository = self.repository
        Task { @MainActor in
            nonisolated struct Remotes: Decodable { struct Remote: Decodable { let name: String }; let remotes: [Remote] }
            nonisolated struct Entries: Decodable { struct Entry: Decodable { let key: String; let value: String }; let entries: [Entry] }
            let (remotes, entries, refs) = await Task.detached {
                ((try? repository.info(["op": "remotes"], as: Remotes.self))?.remotes.map(\.name) ?? [],
                 (try? repository.info(["op": "configList"], as: Entries.self))?.entries ?? [],
                 try? repository.loadRefs())
            }.value
            guard !remotes.isEmpty else {
                return await inform(L("Can’t Publish"), L("This repository has no remotes. Add one in Repository > Add Remote…."))
            }
            let value = { (key: String) in entries.last { $0.key.lowercased() == key.lowercased() }?.value }
            let preferred = [value("branch.\(branch).pushRemote"), value("remote.pushDefault"),
                             remotes.count == 1 ? remotes[0] : "origin"].compactMap { $0 }.first { remotes.contains($0) }
            let popup = NSPopUpButton(frame: .zero, pullsDown: false)
            popup.addItems(withTitles: remotes)
            popup.selectItem(withTitle: preferred ?? remotes[0])
            let field = NSTextField(string: branch)
            field.widthAnchor.constraint(equalToConstant: 240).isActive = true
            let grid = NSGridView(views: [[NSTextField(labelWithString: L("Remote")), popup],
                                          [NSTextField(labelWithString: L("Name there")), field]])
            grid.rowSpacing = 8
            grid.column(at: 0).xPlacement = .trailing
            grid.frame.size = grid.fittingSize
            let alert = makeAlert(L("Publish %@?", branch),
                                  L("This creates the branch on the remote and makes it the upstream (where Pull and Push go from now on)."))
            alert.accessoryView = grid
            alert.addButton(withTitle: "Publish")
            alert.addButton(withTitle: L("Cancel"))
            alert.window.initialFirstResponder = popup
            guard await present(alert) == .alertFirstButtonReturn, let remote = popup.titleOfSelectedItem else { return }
            let remoteBranch = field.stringValue.trimmingCharacters(in: .whitespaces)
            guard !remoteBranch.isEmpty else { return }
            let target = "\(remote)/\(remoteBranch)"
            // その名前のブランチがもうリモートにある（最後の Fetch の時点）。作るのではなく、それを上流にして送る。
            if refs?.items.contains(where: { $0.kind == .remote && $0.name == target }) == true {
                let (outgoing, incoming) = await Task.detached {
                    func range(_ base: String, _ tip: String) -> Int {
                        (try? repository.info(["op": "between", "base": base, "tip": tip, "limit": 1], as: CommitRange.self))?.count ?? 0
                    }
                    return (range(target, branch), range(branch, target))
                }.value
                if incoming > 0 {
                    let alert = makeAlert(L("%@ already exists", target),
                                          L("It has %ld commits that %@ doesn’t (as of the last Fetch), so pushing would be refused. Make it the upstream and Pull first.", incoming, branch))
                    alert.addButton(withTitle: L("Set Upstream"))
                    alert.addButton(withTitle: L("Cancel"))
                    if await present(alert) == .alertFirstButtonReturn {
                        perform(sequence: [
                            ["op": "configSet", "key": "branch.\(branch).remote", "value": remote],
                            ["op": "configSet", "key": "branch.\(branch).merge", "value": "refs/heads/\(remoteBranch)"],
                        ])
                    }
                    return
                }
                let alert = makeAlert(L("%@ already exists", target),
                                      outgoing > 0 ? L("This pushes %ld commits to it and makes it the upstream.", outgoing)
                                                   : L("It is the same as %@. This makes it the upstream.", branch))
                alert.addButton(withTitle: outgoing > 0 ? "Push" : L("Set Upstream"))
                alert.addButton(withTitle: L("Cancel"))
                guard await present(alert) == .alertFirstButtonReturn else { return }
            }
            perform(["op": "push", "remote": remote, "branch": branch, "remoteBranch": remoteBranch,
                     "force": false, "setUpstream": true]) { _ in then?() }
        }
    }

    private struct SyncState {
        let status: WorkingStatus
        /// 送るブランチとその上流（今のブランチなら status のもの、ほかのブランチなら ref から）。
        var branch: String?
        var upstream: String?
        /// 上流になくて手元にある（push で送る）。
        let outgoing: CommitRange
        /// 上流にあって手元にない（pull で取り込む）。
        let incoming: CommitRange
        /// 送る範囲の両端（上流のコミットと HEAD）。送るものがなければ nil。
        var range: (base: Data, tip: Data)? = nil
        /// 上流のコミット（16 進、最後の Fetch の時点）。Force Push はリモートがこれのときだけ置き換える。
        var upstreamTarget: String? = nil
        /// Push の前に目を向けてほしいこと（既定のブランチへ直接、整理し忘れのコミット、秘密らしきファイルの追加など）。
        var warnings: [String] = []
    }

    /// シートの説明文（worktree が 2 つ以上なら、どの worktree かを先頭に）。
    private func sheetMessage(_ message: String) -> String {
        worktreeName.map { L("In the worktree “%@”.", $0) + (message.isEmpty ? "" : "\n" + message) } ?? message
    }

    /// Push の前の注意。送るコミットの件名と、送る範囲で足すファイルから。
    nonisolated private static func pushWarnings(upstream: String, outgoing: CommitRange, changes: CommitChanges) -> [String] {
        var out: [String] = []
        let remoteBranch = upstream.split(separator: "/", maxSplits: 1).last.map(String.init) ?? upstream
        if ["main", "master", "trunk", "develop"].contains(remoteBranch) {
            out.append(L("This pushes straight to %@.", upstream))
        }
        let unfinished = outgoing.commits.filter { c in
            let s = c.subject.lowercased()
            return s.hasPrefix("fixup!") || s.hasPrefix("squash!") || s.hasPrefix("amend!") || s.hasPrefix("wip") || s.contains(" wip ") || s.hasSuffix(" wip")
        }
        if !unfinished.isEmpty {
            out.append(L("%ld commits look unfinished (fixup!, squash! or WIP).", unfinished.count))
        }
        let added = changes.items.filter { $0.kind == .added }
        let secretLike = added.map(\.path).filter { path in
            let name = (path as NSString).lastPathComponent.lowercased()
            return name == ".env" || name.hasPrefix(".env.") || name.hasSuffix(".pem") || name.hasSuffix(".key")
                || name.hasSuffix(".p12") || name.hasSuffix(".pfx") || name.hasPrefix("id_rsa") || name.hasPrefix("id_ed25519")
                || name.contains("credential") || name.contains("secret")
        }
        if !secretLike.isEmpty {
            out.append(L("Adds files that may hold secrets: %@", secretLike.prefix(3).joined(separator: ", ")))
        }
        // バイナリの追加（画像や成果物をうっかり入れたとき）。数えるのに blob を読むので、先頭の 100 ファイルまで。
        var binaries = 0
        for (i, item) in changes.items.prefix(100).enumerated() where item.kind == .added {
            if let diff = try? changes.diff(at: i), diff.isBinary { binaries += 1 }
        }
        if binaries > 0 { out.append(L("Adds %ld binary files.", binaries)) }
        return out
    }

    @MainActor
    /// `branch` が nil なら今のブランチ（HEAD）。ほかのブランチは ref の一覧から上流と先端を取る。
    private func syncState(branch: String? = nil) async -> SyncState? {
        let repository = self.repository
        let result = await Task.detached { () -> Result<SyncState, Error> in
            Result {
                let status = try repository.loadStatus()
                let refs = try repository.loadRefs()
                let local = branch.flatMap { name in refs.items.first { $0.kind == .local && $0.name == name } }
                let name = local?.name ?? status.branch
                let upstream = local.map { $0.gone ? nil : $0.detail } ?? status.upstream
                let tipRef = local?.name ?? "HEAD"
                guard let upstream else {
                    return SyncState(status: status, branch: name, upstream: nil, outgoing: .empty, incoming: .empty)
                }
                func range(_ base: String, _ tip: String) throws -> CommitRange {
                    try repository.info(["op": "between", "base": base, "tip": tip, "limit": 500], as: CommitRange.self)
                }
                var state = SyncState(
                    status: status, branch: name, upstream: upstream, outgoing: try range(upstream, tipRef),
                    incoming: try range(tipRef, upstream))
                state.upstreamTarget = refs.items.first { $0.kind == .remote && $0.name == upstream }?.target
                    .map { $0.map { String(format: "%02x", $0) }.joined() }
                // 送るものがあれば、その範囲の両端（シートの「送る変更すべて」の差分）と、注意を求める。
                // 上流が先に進んでいれば、起点は分かれたところ（いちばん古い送るコミットの親）。上流の先から比べると、
                // 上流にしかない変更まで「送る変更」に見えてしまう。
                let forked = state.incoming.count > 0 && state.outgoing.count == state.outgoing.commits.count
                let forkPoint = forked ? state.outgoing.commits.last.flatMap { oldest in
                    (try? repository.run(["op": "resolveCommit", "rev": "\(oldest.oid)^"]))?.text.flatMap { Data(hex: $0) }
                } : nil
                if state.outgoing.count > 0, let tip = local?.target ?? refs.head,
                   let base = forkPoint ?? refs.items.first(where: { $0.kind == .remote && $0.name == upstream })?.target {
                    state.range = (base, tip)
                    let changes = try repository.changes(from: base, to: tip)
                    state.warnings = Self.pushWarnings(upstream: upstream, outgoing: state.outgoing, changes: changes)
                }
                return state
            }
        }.value
        switch result {
        case let .success(state): return state
        case let .failure(error):
            await inform(L("Can’t read the status"), "\(error)")
            return nil
        }
    }

    // MARK: - Delete branch

    /// ブランチを消す前に確かめる。今のブランチにも上流にも取り込まれていないコミットがあれば（git は `-d` を断る）、
    /// それを並べて見せ、Delete Anyway（`-D`）で消す。どちらも Undo で戻せる。
    func deleteBranchWithConfirmation(_ item: RefItem) {
        deleteBranchesWithConfirmation([item.name])
    }

    /// いくつかのブランチをまとめて消す前に確かめる。どこにも取り込まれていないコミットのあるものは、それを並べて
    /// Delete Anyway で消す（1 本だけ断られて途中で止まることがない）。
    func deleteBranchesWithConfirmation(_ names: [String]) {
        let repository = self.repository
        Task { @MainActor in
            // それぞれのブランチにしかないコミット。git と同じく、今のブランチか上流のどちらかに取り込まれていれば消せる。
            let result = await Task.detached { () -> Result<[(String, CommitRange)], Error> in
                Result {
                    let refs = try repository.loadRefs()
                    return try names.map { name in
                        func only(_ base: String) throws -> CommitRange {
                            try repository.info(["op": "between", "base": base, "tip": name, "limit": 30], as: CommitRange.self)
                        }
                        let item = refs.items.first { $0.kind == .local && $0.name == name }
                        let upstream = item.flatMap { $0.gone ? nil : $0.detail }
                        let fromHead = try only("HEAD")
                        guard fromHead.count > 0, let upstream else { return (name, fromHead) }
                        let fromUpstream = try only(upstream)
                        return (name, fromUpstream.count < fromHead.count ? fromUpstream : fromHead)
                    }
                }
            }.value
            let unique: [(String, CommitRange)]
            switch result {
            case let .failure(error): return await inform(L("Couldn’t check the branches"), "\(error)")
            case let .success(ranges): unique = ranges
            }
            let unmerged = unique.filter { $0.1.count > 0 }
            let title = names.count == 1 ? L("Delete %@?", names[0]) : L("Delete %ld branches?", names.count)
            let list = names.count == 1 ? "" : names.joined(separator: ", ") + "\n"
            let message = list + (unmerged.isEmpty ? "" : L("Some commits are on no other branch; they go with the branch.") + " ") + Self.canUndo
            let alert = makeAlert(title, message)
            if !unmerged.isEmpty {
                alert.accessoryView = Self.commitLists(unmerged.map { (L("Commits only on %@", $0.0), $0.1, true) })
            }
            alert.addButton(withTitle: unmerged.isEmpty ? L("Delete") : L("Delete Anyway"))
            alert.addButton(withTitle: L("Cancel"))
            guard await present(alert) == .alertFirstButtonReturn else { return }
            perform(sequence: unique.map { ["op": "deleteBranch", "name": $0.0, "force": $0.1.count > 0] })
        }
    }

    @MainActor
    func makeAlert(_ title: String, _ message: String) -> NSAlert {
        let alert = NSAlert()
        alert.messageText = title
        // worktree が 2 つ以上あれば、どの worktree に対する操作かを書く（docs/ux.md の「Worktrees」）。
        alert.informativeText = worktreeName.map { L("In the worktree “%@”.", $0) + (message.isEmpty ? "" : "\n" + message) } ?? message
        return alert
    }

    @MainActor
    func present(_ alert: NSAlert) async -> NSApplication.ModalResponse {
        if let window { return await alert.beginSheetModal(for: window) }
        return alert.runModal()
    }

    @MainActor
    private func inform(_ title: String, _ message: String) async {
        let alert = makeAlert(title, message)
        alert.addButton(withTitle: "OK")
        _ = await present(alert)
    }

    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f
    }()

    /// コミットの一覧（見出しと、短い ID・件名・作者・いつ）。空の一覧は出さない。`warn` は消えるもの（赤）。
    @MainActor
    static func commitLists(_ lists: [(String, CommitRange, Bool)]) -> NSView {
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 4
        for (title, range, warn) in lists where range.count > 0 {
            let header = NSTextField(labelWithString: "\(title) (\(range.count))")
            header.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
            header.textColor = warn ? .systemRed : .labelColor
            stack.addArrangedSubview(header)
            let text = NSMutableAttributedString()
            let mono = NSFont.monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
            let body = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
            for (i, c) in range.commits.enumerated() {
                if i > 0 { text.append(NSAttributedString(string: "\n")) }
                text.append(NSAttributedString(string: String(c.oid.prefix(8)) + "  ",
                                               attributes: [.font: mono, .foregroundColor: NSColor.secondaryLabelColor]))
                text.append(NSAttributedString(string: c.subject, attributes: [
                    .font: body, .foregroundColor: warn ? NSColor.systemRed : NSColor.labelColor,
                ]))
                let when = relativeFormatter.localizedString(for: Date(timeIntervalSince1970: TimeInterval(c.time)), relativeTo: Date())
                text.append(NSAttributedString(string: "  \(c.author) · \(when)",
                                               attributes: [.font: body, .foregroundColor: NSColor.tertiaryLabelColor]))
            }
            if range.count > range.commits.count {
                text.append(NSAttributedString(string: "\n" + L("and %ld more", range.count - range.commits.count),
                                               attributes: [.font: body, .foregroundColor: NSColor.secondaryLabelColor]))
            }
            let lines = min(range.commits.count + (range.count > range.commits.count ? 1 : 0), 8)
            let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 440, height: CGFloat(lines) * 17 + 8))
            scroll.hasVerticalScroller = true
            scroll.borderType = .bezelBorder
            let view = NSTextView(frame: scroll.bounds)
            view.isEditable = false
            view.drawsBackground = false
            view.textContainerInset = NSSize(width: 2, height: 4)
            view.textContainer?.lineFragmentPadding = 2
            view.textContainer?.widthTracksTextView = false
            view.textContainer?.containerSize = NSSize(width: 2000, height: CGFloat.greatestFiniteMagnitude)
            view.isHorizontallyResizable = true
            view.autoresizingMask = [.width]
            view.textStorage?.setAttributedString(text)
            scroll.documentView = view
            scroll.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                scroll.widthAnchor.constraint(equalToConstant: 440),
                scroll.heightAnchor.constraint(equalToConstant: scroll.frame.height),
            ])
            stack.addArrangedSubview(scroll)
        }
        stack.layoutSubtreeIfNeeded()
        stack.frame.size = stack.fittingSize
        return stack
    }
}
