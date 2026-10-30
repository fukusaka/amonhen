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

/// Push・Pull・Pop・Undo の前の確認。ボタンを押しただけで、外に出たり履歴が変わったりしないように、何が起きるか
/// （送る・取り込むコミット、分岐、作業中の変更）をシートで見せてから実行する。↩ で実行できる。
extension Actions {
    // MARK: - Push

    /// Push の前に、送るコミットを見せる。上流が先に進んでいれば Pull を勧め、`force` なら上流から消えるコミットを見せる。
    /// `branch` を渡せば今のブランチ以外も送れる（Create Pull Request の前など）。
    func pushWithConfirmation(branch: String? = nil, force: Bool = false) {
        Task { @MainActor in
            guard let state = await syncState(branch: branch) else { return }
            guard let branch = state.branch else {
                return await inform(L("Can’t Push"), L("No branch is checked out (detached HEAD)."))
            }
            guard let upstream = state.upstream else { return publishWithConfirmation(branch: branch) }
            let (outgoing, incoming) = (state.outgoing, state.incoming)
            if force {
                let title = L("Force push %@?", branch)
                let message = L("This replaces %@ with your %@ (--force-with-lease: it fails if someone pushed in the meantime).", upstream, branch)
                if let range = state.range, let detail = makeDetailView?(), let window {
                    let sheet = PushSheet(
                        title: title, message: sheetMessage(message), warnings: state.warnings, commits: outgoing,
                        lost: incoming, range: range, detail: detail, ok: "Force Push", destructive: true)
                    if await sheet.present(in: window) { push(branch: branch, force: true) }
                    return
                }
                let alert = makeAlert(title, message)
                alert.accessoryView = Self.commitLists([
                    (L("Commits to send"), outgoing, false), (L("Commits %@ will lose", upstream), incoming, true),
                ])
                let button = alert.addButton(withTitle: "Force Push")
                button.hasDestructiveAction = true
                alert.addButton(withTitle: L("Cancel"))
                if await present(alert) == .alertFirstButtonReturn { push(branch: branch, force: true) }
                return
            }
            if incoming.count > 0 {
                let alert = makeAlert(
                    outgoing.count > 0 ? L("%@ and %@ have diverged", branch, upstream) : L("Nothing to push"),
                    L("%@ has %ld commits you don’t have (as of the last Fetch). Pull them first, then push.", upstream, incoming.count))
                alert.accessoryView = Self.commitLists([
                    (L("Commits to send"), outgoing, false), (L("Commits only on %@", upstream), incoming, false),
                ])
                alert.addButton(withTitle: "Pull…")
                alert.addButton(withTitle: L("Cancel"))
                if outgoing.count > 0 { alert.addButton(withTitle: "Force Push…") }
                switch await present(alert) {
                case .alertFirstButtonReturn: pullWithConfirmation(mode: .git)
                case .alertThirdButtonReturn: pushWithConfirmation(branch: branch, force: true)
                default: break
                }
                return
            }
            if outgoing.count == 0 {
                return await inform(L("Nothing to push"), L("%@ is the same as %@ (as of the last Fetch).", branch, upstream))
            }
            let title = L("Push %ld commits to %@?", outgoing.count, upstream)
            // 送るコミットとその中身を、右の欄と同じ表示で読んでから押す（Metal がなければ一覧だけのシート）。
            if let range = state.range, let detail = makeDetailView?(), let window {
                let sheet = PushSheet(
                    title: title, message: sheetMessage(""), warnings: state.warnings, commits: outgoing, lost: nil,
                    range: range, detail: detail, ok: "Push", destructive: false)
                if await sheet.present(in: window) { push(branch: branch, force: false) }
                return
            }
            let alert = makeAlert(title, "")
            alert.accessoryView = Self.commitLists([(L("Commits to send"), outgoing, false)])
            alert.addButton(withTitle: "Push")
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn { push(branch: branch, force: false) }
        }
    }

    // MARK: - Pull

    /// Pull の前に、取り込むコミットと取り込み方を見せる。取り込み方はシートの中で変えられる（既定は `mode`）。
    /// ツールバーと ⌃⌘L は「git の設定に従う」で開く（Repository > Pull With は選んだ方法で）。
    func pullWithConfirmation(mode: PullMode) {
        Task { @MainActor in
            guard let state = await syncState() else { return }
            guard let branch = state.status.branch else {
                return await inform(L("Can’t Pull"), L("No branch is checked out (detached HEAD)."))
            }
            guard let upstream = state.status.upstream else {
                let alert = makeAlert(L("%@ has no upstream", branch), L("Choose the remote branch to pull from first."))
                alert.addButton(withTitle: L("Set Upstream…"))
                alert.addButton(withTitle: L("Cancel"))
                if await present(alert) == .alertFirstButtonReturn {
                    NSApp.sendAction(#selector(RepositoryWindow.setUpstream(_:)), to: nil, from: nil)
                }
                return
            }
            let (outgoing, incoming) = (state.outgoing, state.incoming)
            let changed = state.status.items.filter { $0.kind != .untracked }.count
            // git の設定（pull.rebase・pull.ff）が決める取り込み方。「git の設定に従う」の中身として見せる。
            struct Defaults: Decodable { let pull: String }
            let repository = self.repository
            let byGit = await Task.detached { try? repository.info(["op": "syncDefaults"], as: Defaults.self) }.value
                .flatMap { PullMode(rawValue: $0.pull) } ?? .merge
            // 選んだ取り込み方が実際に行うこと（.git なら git の設定が決める方）。
            let method = { (m: PullMode) -> PullMode in m == .git ? byGit : m }
            let alert = makeAlert(
                incoming.count > 0 ? L("Pull %ld commits from %@?", incoming.count, upstream) : L("Pull from %@?", upstream),
                "")
            let popup = NSPopUpButton(frame: .zero, pullsDown: false)
            for m in PullMode.allCases {
                popup.addItem(withTitle: m == .git ? L("%@ (%@)", m.title, byGit.title) : m.title)
                popup.lastItem?.representedObject = m.rawValue
            }
            popup.selectItem(at: PullMode.allCases.firstIndex(of: mode) ?? 0)
            let note = NSTextField(wrappingLabelWithString: "")
            note.preferredMaxLayoutWidth = 440
            note.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            note.textColor = .secondaryLabelColor
            let describe = { (chosen: PullMode) -> String in
                // 取り込み方そのものの説明（選び直すと必ず変わる）と、今の状態でそれが何をするか。
                let m = method(chosen)
                var lines: [String] = switch m {
                case .merge: [L("Merge brings the upstream commits into %@. When both sides have new commits, a merge commit joins them.", branch)]
                case .rebase: [L("Rebase puts your local commits on top of the upstream commits, keeping the History in one line.")]
                case .fastForwardOnly: [L("Fast-Forward Only just moves %@ forward. If you have local commits too, it stops without changing anything.", branch)]
                case .git: []
                }
                if chosen == .git {
                    lines.insert(L("This repository’s git settings (pull.rebase, pull.ff) choose %@, as git pull in a terminal would.", m.title), at: 0)
                }
                if incoming.count == 0 {
                    lines.append(L("Nothing new as of the last Fetch (Pull fetches first)."))
                } else if outgoing.count == 0 {
                    lines.append(L("This just moves %@ forward (fast-forward).", branch))
                } else {
                    switch m {
                    case .merge: lines.append(L("You have %ld local commits, so this makes a merge commit.", outgoing.count))
                    case .rebase: lines.append(L("Your %ld local commits are replayed on top (their IDs change).", outgoing.count))
                    case .fastForwardOnly: lines.append(L("⚠︎ You have %ld local commits too, so Fast-Forward Only can’t pull these.", outgoing.count))
                    case .git: break
                    }
                }
                if changed > 0 { lines.append(L("%ld files have uncommitted changes. Pull stops if the incoming changes touch them.", changed)) }
                lines.append(L("You can Undo it afterwards (⌥⌘Z)."))
                return lines.joined(separator: "\n")
            }
            // 説明の高さは、いちばん長い取り込み方に合わせて決めておく（選び直してもシートの大きさを変えず、
            // 文が切れない）。
            let height = PullMode.allCases.map { m -> CGFloat in
                note.stringValue = describe(m)
                return note.fittingSize.height
            }.max() ?? 0
            note.heightAnchor.constraint(equalToConstant: height).isActive = true
            let update = {
                note.stringValue = describe(PullMode(rawValue: popup.selectedItem?.representedObject as? String ?? "") ?? .merge)
            }
            update()
            // 両方に新しいコミットがあるときだけ、取り込むとコンフリクトするかを裏で調べる（fast-forward なら重ならない）。
            // Fast-Forward Only はマージしないので出さない。rebase は最後の結果での目安。
            let forecastLabel = incoming.count > 0 && outgoing.count > 0 ? Self.forecastLabel() : nil
            final class Answer { var forecast: MergeForecast?; var done = false }
            let answer = Answer()
            let showForecast = {
                guard let forecastLabel else { return }
                let m = method(PullMode(rawValue: popup.selectedItem?.representedObject as? String ?? "") ?? .merge)
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
            popup.target = target
            popup.action = #selector(ClosureTarget.fire)
            if forecastLabel != nil {
                Task { @MainActor in
                    answer.forecast = await predictMerge(theirs: upstream)
                    answer.done = true
                    showForecast()
                }
            }
            let modeRow = NSStackView(views: [NSTextField(labelWithString: L("Integrate with:")), popup])
            modeRow.spacing = 6
            let stack = NSStackView(views: [modeRow, note] + (forecastLabel.map { [$0] } ?? []))
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
            alert.addButton(withTitle: "Pull")
            alert.addButton(withTitle: L("Cancel"))
            let response = await present(alert)
            withExtendedLifetime(target) {}
            guard response == .alertFirstButtonReturn else { return }
            let chosen = PullMode(rawValue: popup.selectedItem?.representedObject as? String ?? "") ?? mode
            perform(["op": "pull", "mode": chosen.rawValue])
        }
    }

    // MARK: - Stash・Undo

    /// stash を戻して消す前に、どの stash かを見せる。
    func popWithConfirmation(name: String, message: String?) {
        Task { @MainActor in
            let alert = makeAlert(
                L("Pop %@?", name),
                (message.map { "“\($0)”\n\n" } ?? "") + L("This applies it to the working tree and removes it from the stash list (it is kept if it conflicts)."))
            alert.addButton(withTitle: "Pop")
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn {
                perform(["op": "stashApply", "name": name, "pop": true])
            }
        }
    }

    /// Undo の前に、何を戻すかを見せる（ツールバーのボタンでは名前がツールチップにしか出ないため）。
    func undoWithConfirmation() {
        let repository = self.repository
        Task { @MainActor in
            let peek = (try? await Task.detached { try repository.undoPeek() }.value) ?? nil
            guard let peek else {
                return await inform(L("Nothing to Undo"), "")
            }
            // 何を戻すかは記録による（ファイルだけを戻す記録は、HEAD とブランチに触らない）。
            let what = switch peek.kind {
            case "files": L("This puts the files back as they were before; HEAD and the index stay as they are. The operation log in Activity can undo it too.")
            case "stash": L("This brings back the stash that was dropped. The operation log in Activity can undo it too.")
            case "rename": L("This gives the branch its earlier name back. The operation log in Activity can undo it too.")
            default: L("This returns HEAD and the branches to where they were before. The operation log in Activity can undo it too.")
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
    func publishWithConfirmation(branch: String) {
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
                     "force": false, "setUpstream": true])
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
                // 送るものがあれば、その範囲の両端（シートの「送る変更すべて」の差分）と、注意を求める。
                if state.outgoing.count > 0, let tip = local?.target ?? refs.head,
                   let base = refs.items.first(where: { $0.kind == .remote && $0.name == upstream })?.target {
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
        let repository = self.repository
        let name = item.name
        let upstream = item.gone ? nil : item.detail
        Task { @MainActor in
            // このブランチにしかないコミット。git と同じく、今のブランチか上流のどちらかに取り込まれていれば消せる。
            let result = await Task.detached { () -> Result<CommitRange, Error> in
                Result {
                    func only(_ base: String) throws -> CommitRange {
                        try repository.info(["op": "between", "base": base, "tip": name, "limit": 30], as: CommitRange.self)
                    }
                    let fromHead = try only("HEAD")
                    guard fromHead.count > 0, let upstream else { return fromHead }
                    let fromUpstream = try only(upstream)
                    return fromUpstream.count < fromHead.count ? fromUpstream : fromHead
                }
            }.value
            let unique: CommitRange
            switch result {
            case let .failure(error): return await inform(L("Can’t check %@", name), "\(error)")
            case let .success(range): unique = range
            }
            let alert: NSAlert
            if unique.count == 0 {
                alert = makeAlert(L("Delete %@?", name), L("You can Undo it afterwards (⌥⌘Z)."))
                alert.addButton(withTitle: L("Delete")).hasDestructiveAction = true
            } else {
                alert = makeAlert(
                    L("Delete %@?", name),
                    L("These commits are on no other branch (not in the current branch, nor in its upstream), so git refuses a plain delete. Delete Anyway removes them with the branch.")
                        + " " + L("You can Undo it afterwards (⌥⌘Z)."))
                alert.accessoryView = Self.commitLists([(L("Commits only on %@", name), unique, true)])
                alert.addButton(withTitle: L("Delete Anyway")).hasDestructiveAction = true
            }
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn {
                perform(["op": "deleteBranch", "name": name, "force": unique.count > 0])
            }
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
