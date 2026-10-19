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

/// Push・Pull・Pop・Undo の前の確認。ボタンを押しただけで、外に出たり履歴が変わったりしないように、何が起きるか
/// （送る・取り込むコミット、分岐、作業中の変更）をシートで見せてから実行する。↩ で実行できる。
extension Actions {
    // MARK: - Push

    /// Push の前に、送るコミットを見せる。上流が先に進んでいれば Pull を勧め、`force` なら上流から消えるコミットを見せる。
    func pushWithConfirmation(force: Bool = false) {
        Task { @MainActor in
            guard let state = await syncState() else { return }
            guard let branch = state.status.branch else {
                return await inform(L("Can’t Push"), L("No branch is checked out (detached HEAD)."))
            }
            guard let upstream = state.status.upstream else {
                let alert = makeAlert(L("Push %@ to origin?", branch),
                                      L("origin has no %@ yet. This creates origin/%@ and makes it the upstream.", branch, branch))
                alert.addButton(withTitle: "Push")
                alert.addButton(withTitle: L("Cancel"))
                if await present(alert) == .alertFirstButtonReturn { push(force: false) }
                return
            }
            let (outgoing, incoming) = (state.outgoing, state.incoming)
            if force {
                let alert = makeAlert(
                    L("Force push %@?", branch),
                    L("This replaces %@ with your %@ (--force-with-lease: it fails if someone pushed in the meantime).", upstream, branch))
                alert.accessoryView = Self.commitLists([
                    (L("Commits to send"), outgoing, false), (L("Commits %@ will lose", upstream), incoming, true),
                ])
                let button = alert.addButton(withTitle: "Force Push")
                button.hasDestructiveAction = true
                alert.addButton(withTitle: L("Cancel"))
                if await present(alert) == .alertFirstButtonReturn { push(force: true) }
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
                case .alertFirstButtonReturn: pullWithConfirmation(mode: Settings.pullMode)
                case .alertThirdButtonReturn: pushWithConfirmation(force: true)
                default: break
                }
                return
            }
            if outgoing.count == 0 {
                return await inform(L("Nothing to push"), L("%@ is the same as %@ (as of the last Fetch).", branch, upstream))
            }
            let alert = makeAlert(L("Push %ld commits to %@?", outgoing.count, upstream), "")
            alert.accessoryView = Self.commitLists([(L("Commits to send"), outgoing, false)])
            alert.addButton(withTitle: "Push")
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn { push(force: false) }
        }
    }

    // MARK: - Pull

    /// Pull の前に、取り込むコミットと取り込み方を見せる。取り込み方はシートの中で変えられる（既定は `mode`）。
    /// 選んで Pull した取り込み方は、次の Pull の既定になる（`Settings.pullMode`）。
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
            let alert = makeAlert(
                incoming.count > 0 ? L("Pull %ld commits from %@?", incoming.count, upstream) : L("Pull from %@?", upstream),
                "")
            let popup = NSPopUpButton(frame: .zero, pullsDown: false)
            for m in PullMode.allCases {
                popup.addItem(withTitle: m.title)
                popup.lastItem?.representedObject = m.rawValue
            }
            popup.selectItem(at: PullMode.allCases.firstIndex(of: mode) ?? 0)
            let note = NSTextField(wrappingLabelWithString: "")
            note.preferredMaxLayoutWidth = 440
            note.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            note.textColor = .secondaryLabelColor
            let describe = { (m: PullMode) -> String in
                // 取り込み方そのものの説明（選び直すと必ず変わる）と、今の状態でそれが何をするか。
                var lines: [String] = switch m {
                case .merge: [L("Merge brings the upstream commits into %@. When both sides have new commits, a merge commit joins them.", branch)]
                case .rebase: [L("Rebase puts your local commits on top of the upstream commits, keeping the History in one line.")]
                case .fastForwardOnly: [L("Fast-Forward Only just moves %@ forward. If you have local commits too, it stops without changing anything.", branch)]
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
            let target = ClosureTarget(update)
            popup.target = target
            popup.action = #selector(ClosureTarget.fire)
            let modeRow = NSStackView(views: [NSTextField(labelWithString: L("Integrate with:")), popup])
            modeRow.spacing = 6
            let stack = NSStackView(views: [modeRow, note])
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
            Settings.pullMode = chosen
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
            let label = (try? await Task.detached { try repository.run(["op": "undoPeek"]).label }.value) ?? nil
            guard let label else {
                return await inform(L("Nothing to Undo"), "")
            }
            let alert = makeAlert(L("Undo “%@”?", label), L("This returns HEAD and the branches to where they were before. The operation log in Activity can undo it too."))
            alert.addButton(withTitle: "Undo")
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn { undo() }
        }
    }

    // MARK: - 部品

    private struct SyncState {
        let status: WorkingStatus
        /// 上流になくて手元にある（push で送る）。
        let outgoing: CommitRange
        /// 上流にあって手元にない（pull で取り込む）。
        let incoming: CommitRange
    }

    @MainActor
    private func syncState() async -> SyncState? {
        let repository = self.repository
        let result = await Task.detached { () -> Result<SyncState, Error> in
            Result {
                let status = try repository.loadStatus()
                guard let upstream = status.upstream else {
                    return SyncState(status: status, outgoing: .empty, incoming: .empty)
                }
                func range(_ base: String, _ tip: String) throws -> CommitRange {
                    try repository.info(["op": "between", "base": base, "tip": tip, "limit": 30], as: CommitRange.self)
                }
                return SyncState(status: status, outgoing: try range(upstream, "HEAD"), incoming: try range("HEAD", upstream))
            }
        }.value
        switch result {
        case let .success(state): return state
        case let .failure(error):
            await inform(L("Can’t read the status"), "\(error)")
            return nil
        }
    }

    @MainActor
    private func makeAlert(_ title: String, _ message: String) -> NSAlert {
        let alert = NSAlert()
        alert.messageText = title
        // worktree が 2 つ以上あれば、どの worktree に対する操作かを書く（docs/ux.md の「Worktrees」）。
        alert.informativeText = worktreeName.map { L("In the worktree “%@”.", $0) + (message.isEmpty ? "" : "\n" + message) } ?? message
        return alert
    }

    @MainActor
    private func present(_ alert: NSAlert) async -> NSApplication.ModalResponse {
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
