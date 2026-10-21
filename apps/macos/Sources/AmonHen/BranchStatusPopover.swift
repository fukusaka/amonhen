import AmonhenKit
import AppKit

/// サイドバーのブランチの行の右端のアイコン（取り込み済み・gone）を押したときの吹き出し（署名の吹き出しと同じ作り）。
/// 何を意味するか、どこから分かったか、そのブランチの状態（上流・土台・最後のコミット）と、次にできること
/// （消す・アーカイブ）。
final class BranchStatusPopover: NSViewController {
    struct Facts {
        let item: RefItem
        let branch: BranchGroups.Branch?
        /// 既定のブランチ（`main`・`origin/main`）。
        let base: String?
        /// 今のブランチか（消せない）。
        let isHead: Bool
        /// ほかの worktree が checkout している（⌂・⑂ の印）。
        var otherWorktree: WorktreeInfo?
        /// 今のブランチで、worktree が 2 つ以上あり、Working Copy がこのブランチの worktree を編集している（✎ の印）。
        var editing = false
        /// このブランチから開いているプルリクエスト。
        var pullRequest: PullRequest?
        /// プルリクエストを作れる（GitHub のリポジトリで gh が使え、既定のブランチでない）。
        var canCreatePullRequest = false
    }

    private let facts: Facts
    private let onDelete: () -> Void
    private let onArchive: () -> Void
    private let onOpenWorktree: (String) -> Void
    private let onCreatePullRequest: () -> Void

    init(facts: Facts, onDelete: @escaping () -> Void, onArchive: @escaping () -> Void,
         onOpenWorktree: @escaping (String) -> Void, onCreatePullRequest: @escaping () -> Void) {
        self.facts = facts
        self.onDelete = onDelete
        self.onArchive = onArchive
        self.onOpenWorktree = onOpenWorktree
        self.onCreatePullRequest = onCreatePullRequest
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let item = facts.item
        let merged = facts.branch?.merged == true && facts.branch?.atBase != true
        let base = facts.base ?? "main"
        // 見出し: いちばん大事な状態。
        let (symbol, color, title, explanation): (String, NSColor, String, String) = switch (merged, item.gone) {
        case (true, true):
            ("arrow.triangle.merge", .systemGreen, L("Merged and cleaned up on the remote"),
             L("All of this branch’s commits are in %@, and its upstream %@ has been deleted on the remote (typically after the pull request was merged). It is safe to delete.", base, item.detail ?? ""))
        case (true, false):
            ("arrow.triangle.merge", .systemGreen, L("Merged into %@", base),
             L("All of this branch’s commits are reachable from %@ (found in the local history, not from GitHub). A branch merged by squash or rebase on GitHub does not count, because its commits were rewritten.", base))
        case (false, true):
            ("icloud.slash", .systemOrange, L("Upstream is gone"),
             L("This branch tracks %@, which no longer exists on the remote. Its commits are not in %@ in the local history: the pull request was merged by squash or rebase, or it was closed without merging. Check before deleting.", item.detail ?? "", base))
        case _ where facts.otherWorktree != nil:
            (facts.otherWorktree?.main == true ? "house" : "arrow.triangle.branch",
             facts.otherWorktree?.main == true ? NSColor.secondaryLabelColor : NSColor.systemGreen,
             L("Checked out in the worktree “%@”", facts.otherWorktree?.name ?? ""),
             L("Git lets a branch be checked out in only one worktree, so it can’t be checked out here. Open that worktree to work on it (its folder: %@).", facts.otherWorktree?.path ?? ""))
        case _ where item.ahead > 0 || item.behind > 0:
            ("arrow.up.arrow.down", .secondaryLabelColor,
             item.ahead > 0 && item.behind > 0 ? L("Diverged from %@", item.detail ?? "")
                : item.ahead > 0 ? L("Ahead of %@", item.detail ?? "") : L("Behind %@", item.detail ?? ""),
             item.ahead > 0 && item.behind > 0
                ? L("%1$ld commits here are not on the upstream, and %2$ld commits there are not here. Pull brings theirs in (merge or rebase), then Push sends yours.", item.ahead, item.behind)
                : item.ahead > 0 ? L("%ld commits here are not on the upstream yet. Push sends them.", item.ahead)
                : L("%ld commits on the upstream are not here yet. Pull brings them in.", item.behind))
        case _ where facts.editing:
            ("pencil.circle", .controlAccentColor, L("Current branch, being edited"),
             L("Working Copy shows and commits to this branch’s worktree. Other worktrees are listed under Working Copy."))
        default:
            ("arrow.triangle.branch", .secondaryLabelColor, item.name, "")
        }
        let icon = NSImageView()
        icon.image = CommitHeaderRenderer.symbolImage(symbol, color: color, size: 18)
        let titleField = NSTextField(labelWithString: title)
        titleField.font = .systemFont(ofSize: 14, weight: .semibold)
        let header = NSStackView(views: [icon, titleField])
        header.spacing = 6

        let explanationField = NSTextField(wrappingLabelWithString: explanation)
        explanationField.font = .systemFont(ofSize: 12)
        explanationField.textColor = .secondaryLabelColor
        explanationField.preferredMaxLayoutWidth = 380

        func value(_ s: String, mono: Bool = false, color: NSColor = .labelColor) -> NSTextField {
            let f = NSTextField(wrappingLabelWithString: s)
            f.isSelectable = true
            f.font = mono ? .monospacedSystemFont(ofSize: 11, weight: .regular) : .systemFont(ofSize: 12)
            f.textColor = color
            f.preferredMaxLayoutWidth = 290
            return f
        }
        var rows: [(String, NSView)] = [(L("Branch"), value(item.name))]
        if let target = item.target {
            let hex = target.map { String(format: "%02x", $0) }.joined()
            rows.append((L("Tip"), value(String(hex.prefix(12)), mono: true)))
        }
        if let time = facts.branch?.time {
            rows.append((L("Last commit"), value(CommitHeaderRenderer.date(time))))
        }
        if let upstream = item.detail {
            let state = item.gone ? L("%@ (deleted on the remote)", upstream)
                : [item.ahead > 0 ? L("%ld ahead", item.ahead) : nil, item.behind > 0 ? L("%ld behind", item.behind) : nil]
                    .compactMap { $0 }.joined(separator: L(", "))
            rows.append((L("Upstream"), value(state.isEmpty ? upstream : item.gone ? state : "\(upstream) (\(state))",
                                              color: item.gone ? .systemOrange : .labelColor)))
        }
        if let parent = facts.branch?.parent {
            rows.append((L("Built on"), value(parent)))
        }
        if merged {
            rows.append((L("Merged into"), value(base, color: .systemGreen)))
        }
        if let pr = facts.pullRequest {
            rows.append((L("Pull Request"), value("#\(pr.number) \(pr.title)" + (pr.isDraft ? " " + L("(draft)") : ""))))
        }

        let grid = NSGridView(views: rows.map { label, view in
            let l = NSTextField(labelWithString: label)
            l.font = .systemFont(ofSize: 11, weight: .medium)
            l.textColor = .secondaryLabelColor
            return [l, view]
        })
        grid.column(at: 0).xPlacement = .trailing
        grid.rowSpacing = 5
        grid.columnSpacing = 10
        for i in 0..<grid.numberOfRows { grid.row(at: i).rowAlignment = .firstBaseline }

        // 次にできること。消すのは merged のときだけ（git の -d と同じく、取り込んでいないものは消さない）。
        let buttons = NSStackView()
        buttons.spacing = 8
        if facts.otherWorktree != nil {
            let open = NSButton(title: L("Open Worktree"), target: self, action: #selector(openWorktree))
            open.bezelColor = .controlAccentColor
            buttons.addArrangedSubview(open)
        }
        if !facts.isHead, facts.otherWorktree == nil {
            if merged {
                let delete = NSButton(title: L("Delete Branch"), target: self, action: #selector(deleteBranch))
                delete.bezelColor = .controlAccentColor
                buttons.addArrangedSubview(delete)
            }
            if facts.branch?.archived != true {
                buttons.addArrangedSubview(NSButton(title: L("Archive"), target: self, action: #selector(archive)))
            }
        }
        // プルリクエスト: 開いていればそれを開く。なければ、push 済み（上流があり先行コミットがない）で未マージのときに作る
        // （push 起因の CI を見てから作る流れ。まだ push していなければ、作る前に push の確認の窓が出る）。
        if let pr = facts.pullRequest {
            let open = NSButton(title: L("Open Pull Request #%ld", pr.number), target: self, action: #selector(openPullRequest))
            open.bezelColor = .controlAccentColor
            buttons.addArrangedSubview(open)
        } else if facts.canCreatePullRequest, !merged, item.detail != nil, !item.gone, item.ahead == 0 {
            let create = NSButton(title: L("Create Pull Request…"), target: self, action: #selector(createPullRequest))
            if buttons.arrangedSubviews.isEmpty { create.bezelColor = .controlAccentColor }
            buttons.addArrangedSubview(create)
        }
        for b in buttons.arrangedSubviews { (b as? NSButton)?.controlSize = .small }

        let stack = NSStackView(views: [header] + (explanation.isEmpty ? [] : [explanationField]) + [grid]
            + (buttons.arrangedSubviews.isEmpty ? [] : [buttons]))
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.edgeInsets = NSEdgeInsets(top: 14, left: 16, bottom: 14, right: 16)
        stack.widthAnchor.constraint(equalToConstant: 420).isActive = true
        view = stack
    }

    @objc private func deleteBranch() {
        dismiss(nil)
        onDelete()
    }

    @objc private func archive() {
        dismiss(nil)
        onArchive()
    }

    @objc private func createPullRequest() {
        dismiss(nil)
        onCreatePullRequest()
    }

    @objc private func openPullRequest() {
        dismiss(nil)
        if let url = facts.pullRequest.flatMap({ URL(string: $0.url) }) { NSWorkspace.shared.open(url) }
    }

    @objc private func openWorktree() {
        dismiss(nil)
        if let w = facts.otherWorktree { onOpenWorktree(w.path) }
    }
}
