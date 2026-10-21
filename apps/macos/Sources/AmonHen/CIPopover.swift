import AppKit

/// CI の吹き出しを出す側。同じ印をもう一度押したら閉じるだけにする（押した瞬間に transient の吹き出しが閉じ、
/// すぐ出し直すと、閉じられないように見える。サイドバーのブランチの吹き出しと同じ）。
final class CIPopoverPresenter: NSObject, NSPopoverDelegate {
    private var popover: NSPopover?
    private var shownKey: String?
    private var closed: (key: String, at: CFTimeInterval)?

    /// `key`（コミット ID）の印を押した。出ていれば閉じ、閉じた直後なら何もせず、そうでなければ出す。
    func toggle(key: String, status: PullRequest.CI, sha: String, repo: GitHubRepository, relativeTo rect: NSRect, of view: NSView) {
        if let popover, popover.isShown, shownKey == key {
            popover.performClose(nil)
            return
        }
        if let closed, closed.key == key, CACurrentMediaTime() - closed.at < 0.3 {
            self.closed = nil
            return
        }
        let popover = NSPopover()
        popover.behavior = .transient
        popover.delegate = self
        popover.contentViewController = CIPopover(status: status, sha: sha, repo: repo)
        popover.show(relativeTo: rect, of: view, preferredEdge: .maxY)
        self.popover = popover
        shownKey = key
    }

    func popoverDidClose(_ notification: Notification) {
        guard notification.object as? NSPopover === popover else { return }
        if let shownKey { closed = (shownKey, CACurrentMediaTime()) }
        shownKey = nil
        popover = nil
    }
}

/// CI の印（History の行の右端）を押したときの吹き出し。GitHub のコミットのチェック（Actions のジョブと外部の
/// status）を 1 つずつ、結果・名前・かかった時間で並べる。名前を押すとそのログを、下のボタンでコミットのチェックの
/// ページをブラウザで開く。チェックは開いてから読む（印は一覧を描くときの状態だけで、詳細は持っていない）。
final class CIPopover: NSViewController {
    private let status: PullRequest.CI
    private let sha: String
    private let repo: GitHubRepository
    private let list = NSStackView()
    private let note = NSTextField(wrappingLabelWithString: "")

    init(status: PullRequest.CI, sha: String, repo: GitHubRepository) {
        self.status = status
        self.sha = sha
        self.repo = repo
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    /// 状態ごとの記号・色・文。
    static func look(_ ci: PullRequest.CI) -> (symbol: String, color: NSColor, label: String) {
        switch ci {
        case .passing: ("checkmark.circle.fill", .systemGreen, L("CI: passed"))
        case .failing: ("xmark.circle.fill", .systemRed, L("CI: failed"))
        case .pending: ("circle.fill", .systemOrange, L("CI: running"))
        case .none: ("circle", .secondaryLabelColor, L("No CI"))
        }
    }

    override func loadView() {
        let look = Self.look(status)
        let icon = NSImageView()
        icon.image = CommitHeaderRenderer.symbolImage(look.symbol, color: look.color, size: 18)
        let title = NSTextField(labelWithString: look.label)
        title.font = .systemFont(ofSize: 14, weight: .semibold)
        let badge = NSTextField(labelWithString: " " + String(sha.prefix(8)) + " ")
        badge.font = .monospacedSystemFont(ofSize: 10, weight: .medium)
        badge.textColor = .secondaryLabelColor
        badge.wantsLayer = true
        badge.layer?.backgroundColor = NSColor.secondaryLabelColor.withAlphaComponent(0.15).cgColor
        badge.layer?.cornerRadius = 4
        let spacer = NSView()
        spacer.setContentHuggingPriority(.init(1), for: .horizontal)
        let header = NSStackView(views: [icon, title, spacer, badge])
        header.spacing = 6

        note.font = .systemFont(ofSize: 12)
        note.textColor = .secondaryLabelColor
        note.preferredMaxLayoutWidth = 380
        note.stringValue = L("Loading the checks…")

        list.orientation = .vertical
        list.alignment = .leading
        list.spacing = 4

        let open = NSButton(title: L("Open on GitHub"), target: self, action: #selector(openOnGitHub))
        open.controlSize = .small
        let footer = NSStackView(views: [NSView(), open])
        footer.spacing = 8

        let stack = NSStackView(views: [header, note, list, footer])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.edgeInsets = NSEdgeInsets(top: 14, left: 16, bottom: 14, right: 16)
        stack.widthAnchor.constraint(equalToConstant: 420).isActive = true
        footer.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -32).isActive = true
        view = stack

        let (repo, sha) = (self.repo, self.sha)
        Task { [weak self] in
            do {
                let checks = try await GitHub.commitChecks(repo, sha: sha)
                self?.show(checks)
            } catch {
                self?.note.stringValue = L("Couldn’t read the checks: %@", "\(error)")
            }
        }
    }

    /// 読めたチェックを並べる（失敗を上に、走っているもの、成功の順）。
    private func show(_ all: [CommitCheck]) {
        // 同じ名前の run は 1 つに（成功があれば、やり直す前の取り消しは出さない）。
        let checks = CommitCheck.collapsed(all)
        if checks.isEmpty {
            note.stringValue = L("No checks on this commit.")
            return
        }
        note.stringValue = L("%ld checks", checks.count)
        let order: (CommitCheck) -> Int = { c in
            switch c.ci {
            case .failing: 0
            case .pending: 1
            case .none: c.isCancelled ? 2 : 4
            case .passing: 3
            }
        }
        for check in checks.sorted(by: { (order($0), $0.name) < (order($1), $1.name) }) {
            // 取り消しで終わったものは、失敗ではなく止まった印（灰色）。
            let look = check.isCancelled
                ? (symbol: "slash.circle", color: NSColor.secondaryLabelColor, label: "")
                : Self.look(check.ci)
            let icon = NSImageView()
            icon.image = CommitHeaderRenderer.symbolImage(look.symbol, color: look.color, size: 12)
            // 行の幅は決まっている。名前が長いときは名前を省略し、記号と結果は縮めない（同じ優先度だと記号が幅 0 になる）。
            icon.setContentCompressionResistancePriority(.required, for: .horizontal)
            icon.setContentHuggingPriority(.required, for: .horizontal)
            let name: NSView
            if let url = check.url.flatMap(URL.init(string:)) {
                let button = NSButton(title: check.name, target: self, action: #selector(openCheck(_:)))
                button.isBordered = false
                button.contentTintColor = .linkColor
                button.font = .systemFont(ofSize: 12)
                button.lineBreakMode = .byTruncatingTail
                button.toolTip = check.name + "\n" + url.absoluteString
                button.identifier = NSUserInterfaceItemIdentifier(url.absoluteString)
                name = button
            } else {
                let label = NSTextField(labelWithString: check.name)
                label.font = .systemFont(ofSize: 12)
                label.lineBreakMode = .byTruncatingTail
                label.toolTip = check.name
                name = label
            }
            name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            let result = NSTextField(labelWithString: check.resultLabel + (check.seconds.map { "  " + Self.duration($0) } ?? ""))
            result.font = .systemFont(ofSize: 11)
            result.textColor = .secondaryLabelColor
            result.setContentCompressionResistancePriority(.required, for: .horizontal)
            let spacer = NSView()
            spacer.setContentHuggingPriority(.init(1), for: .horizontal)
            let row = NSStackView(views: [icon, name, spacer, result])
            row.spacing = 6
            row.widthAnchor.constraint(equalToConstant: 388).isActive = true
            list.addArrangedSubview(row)
        }
        view.layoutSubtreeIfNeeded()
        preferredContentSize = view.fittingSize
    }

    private static func duration(_ seconds: Int) -> String {
        seconds < 60 ? "\(seconds)s" : seconds < 3600 ? "\(seconds / 60)m \(seconds % 60)s" : "\(seconds / 3600)h \(seconds % 3600 / 60)m"
    }

    @objc private func openCheck(_ sender: NSButton) {
        guard let raw = sender.identifier?.rawValue, let url = URL(string: raw) else { return }
        NSWorkspace.shared.open(url)
    }

    @objc private func openOnGitHub() {
        NSWorkspace.shared.open(repo.webURL.appendingPathComponent("commit/\(sha)/checks"))
    }
}
