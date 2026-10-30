import AmonhenKit
import AppKit

/// merge で取り込むもの 1 つ。種類は merge のメッセージの見本に使う。
nonisolated struct MergeSource: Equatable, Sendable {
    enum Kind { case branch, remoteBranch, tag, commit }
    let rev: String
    let kind: Kind
}

/// いくつものブランチ・タグ・コミットを今のブランチに merge する（`git merge a b c`。1 つのコミットにまとめる
/// octopus）。git は最後のもの以外でコンフリクトすると断るので、前もって調べ、そうなりそうなら、ぶつかるものを
/// 最後に回すか、1 つずつ merge することを勧める。
extension Actions {
    /// 確認のシートを出してから merge する。`branch` は今のブランチ（detached HEAD なら nil）。
    func mergeSeveralWithConfirmation(_ sources: [MergeSource], into branch: String?) {
        guard sources.count > 1 else {
            if let source = sources.first { perform(["op": "merge", "rev": source.rev]) }
            return
        }
        Task { @MainActor in
            let target = branch ?? "HEAD"
            let alert = makeAlert(
                L("Merge %ld into %@?", sources.count, target),
                L("This makes one merge commit with all of them as parents, as git merge does with several branches. Drag to change the order. Undo puts it back."))
            alert.addButton(withTitle: "Merge")
            alert.addButton(withTitle: L("Cancel"))
            alert.addButton(withTitle: L("Merge One by One"))

            let list = MergeOrderList(sources: sources)
            let label = Actions.forecastLabel(width: 420)
            let message = NSTextField(wrappingLabelWithString: "")
            message.preferredMaxLayoutWidth = 420
            message.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            message.textColor = .secondaryLabelColor
            message.maximumNumberOfLines = 2
            message.cell?.truncatesLastVisibleLine = true
            // 大きさを決めるため、いちばん長い名前で作っておく（答えが来るまでは隠す）。
            let longest = sources.map(\.rev).max { $0.count < $1.count } ?? ""
            let putLast = NSButton(title: L("Put %@ Last", longest), target: nil, action: nil)
            putLast.controlSize = .small
            putLast.isHidden = true

            // 順を変えるたびに調べる（答えは順ごとに覚えておき、遅れて来た前の答えは捨てる）。
            final class Answers { var known: [[String]: MergeForecast?] = [:]; var shown: [String] = []; var last: Int? }
            let answers = Answers()
            let show = { (forecast: MergeForecast?) in
                let steps = forecast?.steps?.map(\.conflicts)
                list.steps = steps
                let conflicting = (steps ?? []).indices.filter { !(steps?[$0].isEmpty ?? true) }
                let early = conflicting.filter { $0 < sources.count - 1 }
                answers.last = nil
                if early.isEmpty {
                    Actions.show(forecast, estimate: false, in: label)
                } else {
                    let names = early.map { list.sources[$0].rev }
                    label.stringValue = conflicting.count == 1
                        ? L("⚠︎ %@ conflicts, and git stops when one before the last conflicts. Put it last to resolve the conflicts in this merge, or merge them one by one.", names[0])
                        : L("⚠︎ %@ conflict, and git stops when one before the last conflicts. Merge them one by one instead.", ListFormatter.localizedString(byJoining: names))
                    label.textColor = .systemOrange
                    if conflicting.count == 1 { answers.last = early[0] }
                }
                if let last = answers.last {
                    putLast.title = L("Put %@ Last", list.sources[last].rev)
                }
                putLast.isHidden = answers.last == nil
            }
            let update = {
                let revs = list.sources.map(\.rev)
                message.stringValue = L("Message: %@", Actions.mergeMessage(list.sources, into: branch))
                answers.shown = revs
                if let known = answers.known[revs] { return show(known) }
                list.steps = nil
                putLast.isHidden = true
                label.stringValue = L("Checking for conflicts…")
                label.textColor = .secondaryLabelColor
                Task { @MainActor in
                    let answer = await self.predictMerge(theirs: revs)
                    answers.known[revs] = answer
                    if answers.shown == revs { show(answer) }
                }
            }
            list.onReorder = update
            let putLastTarget = ClosureTarget {
                guard let last = answers.last else { return }
                list.moveToEnd(last)
                update()
            }
            putLast.target = putLastTarget
            putLast.action = #selector(ClosureTarget.fire)

            let stack = NSStackView()
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 8
            // 「最後に回す」は答えが来てから出すので、場所を取っておく（シートの大きさを変えないため）。隠す前に決める
            // （隠してから切り替えても、外した場所は戻らない）。
            stack.detachesHiddenViews = false
            for view in [list.view, label, putLast, message] { stack.addArrangedSubview(view) }
            update()
            list.view.widthAnchor.constraint(equalToConstant: 420).isActive = true
            stack.layoutSubtreeIfNeeded()
            stack.frame.size = stack.fittingSize
            alert.accessoryView = stack
            let response = await present(alert)
            withExtendedLifetime(putLastTarget) {}
            let revs = list.sources.map(\.rev)
            switch response {
            case .alertFirstButtonReturn: merge(revs, oneByOne: false)
            case .alertThirdButtonReturn: merge(revs, oneByOne: true)
            default: break
            }
        }
    }

    /// `revs` を merge する。1 つのコミットにまとめるのを git が断ったら（何も変わっていない）、1 つずつ merge
    /// するかを尋ねる。
    private func merge(_ revs: [String], oneByOne: Bool) {
        var command: [String: Any] = ["op": "merge", "rev": revs]
        if oneByOne { command["oneByOne"] = true }
        perform(command, recover: oneByOne ? nil : { [weak self] failure in
            guard let self, failure.kind == "octopus" else { return false }
            let alert = makeAlert(
                L("Can’t Merge Them in One Commit"),
                L("git stopped because one of them conflicts before the last, and nothing was changed. Merge them one by one instead? Each makes its own merge commit, and you resolve conflicts as they come. Undo puts them all back."))
            alert.addButton(withTitle: L("Merge One by One"))
            alert.addButton(withTitle: L("Cancel"))
            if await present(alert) == .alertFirstButtonReturn { merge(revs, oneByOne: true) }
            return true
        })
    }

    /// git が付ける merge のメッセージ（`git fmt-merge-msg` と同じ形）。ブランチ、リモートブランチ、タグの順に
    /// 種類ごとにまとめ、コミットは `;` の後に。`main`・`master` 以外へなら ` into <branch>` が付く。
    nonisolated static func mergeMessage(_ sources: [MergeSource], into branch: String?) -> String {
        func joined(_ names: [String]) -> String {
            let quoted = names.map { "'\($0)'" }
            guard let last = quoted.last, quoted.count > 1 else { return quoted.first ?? "" }
            return quoted.dropLast().joined(separator: ", ") + " and " + last
        }
        let kinds: [(MergeSource.Kind, String, String)] = [
            (.branch, "branch", "branches"), (.remoteBranch, "remote-tracking branch", "remote-tracking branches"),
            (.tag, "tag", "tags"),
        ]
        var groups: [String] = []
        for (kind, one, many) in kinds {
            let names = sources.filter { $0.kind == kind }.map(\.rev)
            if !names.isEmpty { groups.append((names.count == 1 ? one : many) + " " + joined(names)) }
        }
        var text = "Merge " + groups.joined(separator: ", ")
        let commits = sources.filter { $0.kind == .commit }.map(\.rev)
        if !commits.isEmpty {
            text += (groups.isEmpty ? "" : "; ") + (commits.count == 1 ? "commit " : "commits ") + joined(commits)
        }
        if branch != "main", branch != "master" { text += " into " + (branch ?? "HEAD") }
        return text
    }
}

/// 取り込む順の一覧。ドラッグで並べ替え、それぞれの右にコンフリクトの見込みを出す。
final class MergeOrderList: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    private(set) var sources: [MergeSource]
    /// 順に、コンフリクトする見込みのファイル（調べていなければ nil）。
    var steps: [[String]]? { didSet { table.reloadData() } }
    /// 並べ替えた。
    var onReorder: (() -> Void)?
    let view = NSScrollView()
    private let table = NSTableView()
    private static let rowType = NSPasteboard.PasteboardType("io.github.fukusaka.amonhen.merge-row")

    init(sources: [MergeSource]) {
        self.sources = sources
        super.init()
        for (id, width) in [("name", 250.0), ("forecast", 150.0)] {
            let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(id))
            column.width = width
            table.addTableColumn(column)
        }
        // 見込みの列を一覧の幅に合わせる（はみ出して横にずれ、名前の頭が切れないように）。
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.headerView = nil
        table.rowHeight = 20
        table.style = .plain
        table.usesAlternatingRowBackgroundColors = true
        table.dataSource = self
        table.delegate = self
        table.registerForDraggedTypes([Self.rowType])
        table.setDraggingSourceOperationMask(.move, forLocal: true)
        view.documentView = table
        // 6 行を超えるときだけスクロールする（収まるのに縦のスクロールバーが出ると、その分だけ幅が減る）。
        view.hasVerticalScroller = sources.count > 6
        view.autohidesScrollers = true
        view.borderType = .bezelBorder
        let rows = CGFloat(min(sources.count, 6))
        view.heightAnchor.constraint(equalToConstant: rows * (table.rowHeight + table.intercellSpacing.height) + 4).isActive = true
    }

    func moveToEnd(_ index: Int) {
        sources.append(sources.remove(at: index))
        steps = nil
    }

    func numberOfRows(in tableView: NSTableView) -> Int { sources.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let field = NSTextField(labelWithString: "")
        field.lineBreakMode = .byTruncatingTail
        field.translatesAutoresizingMaskIntoConstraints = false
        let cell = NSTableCellView()
        cell.addSubview(field)
        cell.textField = field
        NSLayoutConstraint.activate([
            field.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 6),
            field.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -6),
            field.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        if tableColumn?.identifier.rawValue == "name" {
            field.stringValue = "\(row + 1). \(sources[row].rev)"
            return cell
        }
        guard let steps, steps.indices.contains(row) else { return cell }
        let files = steps[row]
        if files.isEmpty {
            field.stringValue = "✓"
            field.textColor = .secondaryLabelColor
        } else {
            field.stringValue = "⚠︎ " + files.joined(separator: ", ")
            field.textColor = .systemOrange
            field.toolTip = files.joined(separator: "\n")
        }
        return cell
    }

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> (any NSPasteboardWriting)? {
        let item = NSPasteboardItem()
        item.setString(String(row), forType: Self.rowType)
        return item
    }

    func tableView(_ tableView: NSTableView, validateDrop info: any NSDraggingInfo, proposedRow row: Int,
                   proposedDropOperation dropOperation: NSTableView.DropOperation) -> NSDragOperation {
        if dropOperation == .on { tableView.setDropRow(row, dropOperation: .above) }
        return .move
    }

    func tableView(_ tableView: NSTableView, acceptDrop info: any NSDraggingInfo, row: Int,
                   dropOperation: NSTableView.DropOperation) -> Bool {
        guard let text = info.draggingPasteboard.pasteboardItems?.first?.string(forType: Self.rowType),
              let from = Int(text), sources.indices.contains(from) else { return false }
        let source = sources.remove(at: from)
        sources.insert(source, at: from < row ? row - 1 : row)
        steps = nil
        onReorder?()
        return true
    }
}
