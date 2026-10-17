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
                return await inform("Push できません", "ブランチをチェックアウトしていません（detached HEAD）。")
            }
            guard let upstream = state.status.upstream else {
                let alert = makeAlert("\(branch) を origin に push しますか？",
                                      "origin にはまだ \(branch) がありません。origin/\(branch) を作り、上流にします。")
                alert.addButton(withTitle: "Push")
                alert.addButton(withTitle: "キャンセル")
                if await present(alert) == .alertFirstButtonReturn { push(force: false) }
                return
            }
            let (outgoing, incoming) = (state.outgoing, state.incoming)
            if force {
                let alert = makeAlert(
                    "\(branch) を force push しますか？",
                    "\(upstream) を手元の \(branch) で置き換えます（--force-with-lease。ほかの人が後から push していたら失敗します）。")
                alert.accessoryView = Self.commitLists([
                    ("送るコミット", outgoing, false), ("\(upstream) から消えるコミット", incoming, true),
                ])
                let button = alert.addButton(withTitle: "Force Push")
                button.hasDestructiveAction = true
                alert.addButton(withTitle: "キャンセル")
                if await present(alert) == .alertFirstButtonReturn { push(force: true) }
                return
            }
            if incoming.count > 0 {
                let alert = makeAlert(
                    outgoing.count > 0 ? "\(branch) は \(upstream) と分かれています" : "送るコミットはありません",
                    "\(upstream) に、手元にないコミットが \(incoming.count) 個あります（最後の Fetch の時点）。"
                        + "先に Pull で取り込んでから push してください。")
                alert.accessoryView = Self.commitLists([
                    ("送るコミット", outgoing, false), ("\(upstream) にだけあるコミット", incoming, false),
                ])
                alert.addButton(withTitle: "Pull…")
                alert.addButton(withTitle: "キャンセル")
                if outgoing.count > 0 { alert.addButton(withTitle: "Force Push…") }
                switch await present(alert) {
                case .alertFirstButtonReturn: pullWithConfirmation(mode: Settings.pullMode)
                case .alertThirdButtonReturn: pushWithConfirmation(force: true)
                default: break
                }
                return
            }
            if outgoing.count == 0 {
                return await inform("送るコミットはありません", "\(branch) は \(upstream) と同じです（最後の Fetch の時点）。")
            }
            let alert = makeAlert("\(outgoing.count) コミットを \(upstream) に push しますか？", "")
            alert.accessoryView = Self.commitLists([("送るコミット", outgoing, false)])
            alert.addButton(withTitle: "Push")
            alert.addButton(withTitle: "キャンセル")
            if await present(alert) == .alertFirstButtonReturn { push(force: false) }
        }
    }

    // MARK: - Pull

    /// Pull の前に、取り込むコミットと取り込み方を見せる。取り込み方はシートの中で変えられる（既定は `mode`）。
    func pullWithConfirmation(mode: PullMode) {
        Task { @MainActor in
            guard let state = await syncState() else { return }
            guard let branch = state.status.branch else {
                return await inform("Pull できません", "ブランチをチェックアウトしていません（detached HEAD）。")
            }
            guard let upstream = state.status.upstream else {
                let alert = makeAlert("\(branch) に上流がありません", "Pull で取り込むリモートのブランチを先に決めてください。")
                alert.addButton(withTitle: "Set Upstream…")
                alert.addButton(withTitle: "キャンセル")
                if await present(alert) == .alertFirstButtonReturn {
                    NSApp.sendAction(#selector(RepositoryWindow.setUpstream(_:)), to: nil, from: nil)
                }
                return
            }
            let (outgoing, incoming) = (state.outgoing, state.incoming)
            let changed = state.status.items.filter { $0.kind != .untracked }.count
            let alert = makeAlert(
                incoming.count > 0 ? "\(upstream) から \(incoming.count) コミット取り込みますか？" : "\(upstream) から Pull しますか？",
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
            let update = {
                let m = PullMode(rawValue: popup.selectedItem?.representedObject as? String ?? "") ?? .merge
                var lines: [String] = []
                if incoming.count == 0 {
                    lines.append("最後の Fetch の時点では、取り込むコミットはありません（Pull は先に Fetch します）。")
                } else if outgoing.count == 0 {
                    lines.append("\(branch) を進めるだけです（fast-forward）。")
                } else {
                    switch m {
                    case .merge: lines.append("手元に \(outgoing.count) コミットあるので、マージコミットを 1 つ作ります。")
                    case .rebase: lines.append("手元の \(outgoing.count) コミットを、取り込んだコミットの上に載せ直します（ID が変わります）。")
                    case .fastForwardOnly: lines.append("⚠︎ 手元にも \(outgoing.count) コミットあるので、Fast-Forward Only では取り込めません。")
                    }
                }
                if changed > 0 { lines.append("作業中の変更が \(changed) ファイルあります。取り込む変更と重なると止まります。") }
                lines.append("あとから Undo（⌥⌘Z）で戻せます。")
                note.stringValue = lines.joined(separator: "\n")
            }
            update()
            let target = ClosureTarget(update)
            popup.target = target
            popup.action = #selector(ClosureTarget.fire)
            let modeRow = NSStackView(views: [NSTextField(labelWithString: "取り込み方:"), popup])
            modeRow.spacing = 6
            let stack = NSStackView(views: [modeRow, note])
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 8
            if incoming.count > 0 || outgoing.count > 0 {
                stack.addArrangedSubview(Self.commitLists([
                    ("取り込むコミット", incoming, false), ("手元にだけあるコミット", outgoing, false),
                ]))
            }
            stack.layoutSubtreeIfNeeded()
            stack.frame.size = stack.fittingSize
            alert.accessoryView = stack
            alert.addButton(withTitle: "Pull")
            alert.addButton(withTitle: "キャンセル")
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
                "\(name) を戻して消しますか？",
                (message.map { "「\($0)」\n\n" } ?? "") + "作業ツリーに戻し、stash の一覧から消します（重なってコンフリクトしたときは消しません）。")
            alert.addButton(withTitle: "Pop")
            alert.addButton(withTitle: "キャンセル")
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
                return await inform("Undo できる操作はありません", "")
            }
            let alert = makeAlert("「\(label)」を戻しますか？", "操作の前の HEAD とブランチに戻します。Activity の操作ログからも戻せます。")
            alert.addButton(withTitle: "Undo")
            alert.addButton(withTitle: "キャンセル")
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
            await inform("状態を読めません", "\(error)")
            return nil
        }
    }

    @MainActor
    private func makeAlert(_ title: String, _ message: String) -> NSAlert {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
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
            let header = NSTextField(labelWithString: "\(title)（\(range.count)）")
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
                text.append(NSAttributedString(string: "\nほか \(range.count - range.commits.count) コミット",
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
