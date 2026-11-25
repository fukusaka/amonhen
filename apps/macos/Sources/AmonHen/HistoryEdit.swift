import AmonhenKit
import AppKit

/// History の編集モード（docs/ux.md の「History's edit mode」）。選んだコミットから HEAD までの範囲で、行ごとに
/// 何をするか（pick・reword・squash・fixup・drop・edit）を決め、並べ替える。行は History がそのまま描き（範囲の
/// 行に操作の印、外は薄く）、ここは範囲と決めたこと、下の帯（要約・Cancel・Apply）を持つ。Apply を押すまで何も
/// 変わらず、まとめて 1 回の rebase（コアの `rewrite` の `plan`）で行う。Undo 1 回で全部戻る。並びは History と
/// 同じく新しい順（上が HEAD）。
final class HistoryEditController: NSObject {
    /// 行ですること。札は git の対話的な rebase の言葉、何をするかはツールチップと選ぶ一覧の説明に。
    enum Action: String, CaseIterable {
        case pick, reword, squash, fixup, drop, edit

        var title: String { rawValue }

        /// 選ぶ一覧の項目（言葉と説明）。
        var menuTitle: String { "\(rawValue) — \(summary)" }

        /// 選ぶ一覧で、言葉に添える説明。
        var summary: String {
            switch self {
            case .pick: L("Keep")
            case .reword: L("Edit Message")
            case .squash: L("Combine with Older")
            case .fixup: L("Combine with Older, Keep Its Message")
            case .drop: L("Remove")
            case .edit: L("Stop to Change Files")
            }
        }

        var toolTip: String {
            switch self {
            case .pick: L("Leave the commit as it is (git: pick).")
            case .reword: L("Give the commit a new message (git: reword).")
            case .squash: L("Make this and the older commit below one commit, with a message you can edit (git: squash).")
            case .fixup: L("Make this and the older commit below one commit, keeping the older one’s message (git: fixup).")
            case .drop: L("Remove the commit and its changes (git: drop).")
            case .edit: L("Stop after this commit to change its files and amend it, then Continue (git: edit).")
            }
        }
    }

    /// 1 行（1 コミット）。
    struct Row {
        let oid: String
        let message: String
        let author: String
        /// merge のコミット（作り直すだけで、することは選べない）。
        var merge = false
        var action: Action = .pick
        /// 直したメッセージ（reword のとき）、またはまとめたコミットのメッセージ（squash のとき）。
        var newMessage: String?

        var subject: String {
            String((newMessage ?? message).split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false).first ?? "")
        }
    }

    /// 開くときの最初の変更（右クリックの近道・History のドラッグから）。
    enum Preset {
        case reword(String)
        case combine([String])
        case drop(String)
        /// `oid` を、新しい順の並びで `to` 番目へ。
        case move(String, Int)
        /// `oid` を `onto` のすぐ上へ動かし、`onto` にまとめる（History で重ねて落とした）。
        case squash(String, onto: String)
    }

    private let actions: Actions
    private(set) var rows: [Row]
    private(set) var originalOrder: [String]
    /// HEAD から first-parent で辿った並び（範囲を広げる・狭める先）。
    private(set) var line: [(oid: String, merge: Bool)] = []
    /// 行・すること・見込みが変わった（History が描き直す）。
    var onChange: (() -> Void)?
    private let summary = NSTextField(labelWithString: "")
    /// 一緒に動くブランチ・残るブランチ・コンフリクトの見込み（`rewritePreview`）。
    private let preview = NSTextField(wrappingLabelWithString: "")
    /// 読みかけの見通し（変えるたびに読み直し、古い答えは捨てる）。
    private var previewTask: Task<Void, Never>?
    /// コンフリクトしそうなコミット（行に印を付ける）。
    private var conflicting: [String: [String]] = [:]
    /// 積み重ねたブランチも一緒に動かすか（この編集だけ。最初は git の rebase.updateRefs のとおり）。
    private let moveStacked = NSButton(checkboxWithTitle: L("Also move the stacked branches"), target: nil, action: nil)
    private var configuredUpdateRefs: Bool?
    private var touchedUpdateRefs = false

    /// この編集の指定（git の設定と同じなら nil）。
    private var updateRefs: Bool? {
        guard touchedUpdateRefs, let configuredUpdateRefs else { return nil }
        let on = moveStacked.state == .on
        return on == configuredUpdateRefs ? nil : on
    }

    @objc private func moveStackedToggled() {
        touchedUpdateRefs = true
        updatePreview()
    }
    /// 編集モードを終える（Apply したら true）。
    var onFinish: ((Bool) -> Void)?
    /// 下のステータスバーに出す行（要約・Cancel・Apply）。
    private(set) lazy var barRow: NSView = makeBar()

    init(actions: Actions, rows: [Row]) {
        self.actions = actions
        self.rows = rows
        originalOrder = rows.map(\.oid)
        super.init()
    }

    /// `start`（16 進）から HEAD までを読み、編集モードを作る。`start` が編集できる並び（HEAD から最初の merge
    /// まで）になければ nil。
    @MainActor
    static func load(actions: Actions, from start: String, includingOlder: Bool = false) async -> HistoryEditController? {
        // merge を越えて first-parent で辿る（merge は git の --rebase-merges で作り直す）。
        let line = await actions.firstParentCommits()
        let chain = line.map(\.oid)
        let merges = Set(line.filter(\.merge).map(\.oid))
        guard var end = chain.firstIndex(where: { $0.hasPrefix(start) || start.hasPrefix($0) }) else { return nil }
        if includingOlder {
            guard end + 1 < chain.count else { return nil }
            end += 1
        }
        let rows = await readRows(Array(chain[...end]), merges: merges, repository: actions.repository)
        let editor = HistoryEditController(actions: actions, rows: rows)
        editor.line = line
        return editor
    }

    private static func readRows(_ oids: [String], merges: Set<String>, repository: Repository) async -> [Row] {
        await Task.detached { () -> [Row] in
            oids.map { oid in
                let message = (try? repository.run(["op": "commitMessage", "oid": oid]).text) ?? nil
                let author = (try? repository.commitInfo(oid: oid))?.author ?? ""
                return Row(oid: oid, message: message ?? "", author: author, merge: merges.contains(oid))
            }
        }.value
    }

    // MARK: - History から

    /// 範囲を、HEAD から first-parent で `count` 個にする。広げるときは古い側に足し、狭めるときは外れる行が
    /// どれもそのまま（pick で、元の位置）のときだけ。できなければ false。
    @discardableResult
    func resize(to count: Int) async -> Bool {
        let count = min(max(1, count), line.count)
        if count == rows.count { return true }
        if count < rows.count {
            let leaving = line[count..<rows.count].map(\.oid)
            guard Array(rows.suffix(leaving.count)).map(\.oid) == leaving,
                  rows.suffix(leaving.count).allSatisfy({ $0.action == .pick }) else { return false }
            rows.removeLast(leaving.count)
            originalOrder.removeLast(leaving.count)
        } else {
            let adding = line[rows.count..<count]
            let more = await Self.readRows(adding.map(\.oid), merges: Set(adding.filter(\.merge).map(\.oid)),
                                           repository: actions.repository)
            rows += more
            originalOrder += more.map(\.oid)
        }
        // いちばん古い行には、まとめる相手がない。
        if let last = rows.indices.last, [.squash, .fixup].contains(rows[last].action) { set(.pick, at: last) }
        update()
        return true
    }

    /// 範囲を狭めて `count` 個にできるか（外れる行がそのまま）。
    func canShrink(to count: Int) -> Bool {
        guard count >= 1, count < rows.count else { return count >= rows.count }
        let leaving = line[count..<rows.count].map(\.oid)
        return Array(rows.suffix(leaving.count)).map(\.oid) == leaving
            && rows.suffix(leaving.count).allSatisfy { $0.action == .pick }
    }

    /// 行 `oid` で選べること。
    func choices(for oid: String) -> [Action] {
        rows.firstIndex { $0.oid == oid }.map(choices(at:)) ?? []
    }

    /// 行 `oid` のすることを変える（reword はメッセージの欄を開く）。
    func choose(_ action: Action, for oid: String) {
        guard let i = rows.firstIndex(where: { $0.oid == oid }), choices(at: i).contains(action) else { return }
        if action == .reword {
            editMessage(at: i)
            return
        }
        set(action, at: i)
        update()
    }

    /// 行 `oid` のメッセージを直す（ダブルクリック）。
    func editMessage(of oid: String) {
        if let i = rows.firstIndex(where: { $0.oid == oid }) { editMessage(at: i) }
    }

    /// 下の帯の要約を書く（始めに呼ぶ）。
    func refreshSummary() { update() }

    /// 並べ替えられるか（merge をまたいで並べ替えると、何をどこへ動かすかが一意でない）。
    var reorderable: Bool { !rows.contains(where: \.merge) }

    /// 行 `oid` を、新しい順の並びで `to` 番目へ。
    func move(_ oid: String, to: Int) {
        guard reorderable, let from = rows.firstIndex(where: { $0.oid == oid }), rows.indices.contains(to), from != to else { return }
        rows.insert(rows.remove(at: from), at: to)
        if let last = rows.indices.last, [.squash, .fixup].contains(rows[last].action) { set(.pick, at: last) }
        update()
    }

    /// 行 `oid` を `target` に重ねて落とした: `target` のすぐ上へ動かし、squash にする。
    func squash(_ oid: String, onto target: String) {
        if place(oid, squashingInto: target) { update() }
    }

    /// 行 `oid` を `target` のすぐ上（新しい側）へ動かし、squash にする。動かすのに並べ替えが要るのに並べ替えられ
    /// ない（merge を含む）、merge の行、同じ行なら false。
    private func place(_ oid: String, squashingInto target: String) -> Bool {
        guard oid != target, let from = rows.firstIndex(where: { $0.oid == oid }), !rows[from].merge,
              let at = rows.firstIndex(where: { $0.oid == target }), !rows[at].merge,
              from + 1 == at || reorderable else { return false }
        let row = rows.remove(at: from)
        let to = from < at ? at - 1 : at
        rows.insert(row, at: to)
        rows[to].newMessage = nil
        set(.squash, at: to)
        // 動かした跡で、いちばん古い行が squash のままにならないように（move と同じ）。
        if let last = rows.indices.last, [.squash, .fixup].contains(rows[last].action) { set(.pick, at: last) }
        return true
    }

    /// 確かめる用（--edit-commits）: `prefix` で始まる行のすることを、メッセージの欄を開かずに決める。
    func set(_ action: Action, prefix: String, message: String? = nil) {
        guard let i = rows.firstIndex(where: { $0.oid.hasPrefix(prefix) }) else { return }
        set(action, at: i)
        if let message { rows[i].newMessage = message }
        update()
    }

    /// 確かめる用: `prefix` で始まる行を `to` 番目へ。
    func move(prefix: String, to: Int) {
        if let row = rows.first(where: { $0.oid.hasPrefix(prefix) }) { move(row.oid, to: to) }
    }

    /// コンフリクトしそうな行（印とツールチップ）。
    var expectedConflicts: [String: [String]] { conflicting }

    /// 読んだ行（HEAD から first-parent で辿ったもの）に、`oids` がすべてある。
    func contains(_ oids: [String]) -> Bool {
        oids.allSatisfy { oid in rows.contains { $0.oid == oid } }
    }

    /// `oids` が並びで隣り合っている（一致したコミットだけの History では、間のコミットが隠れている）。
    func areAdjacent(_ oids: [String]) -> Bool {
        let at = oids.compactMap { oid in rows.firstIndex { $0.oid == oid } }.sorted()
        return zip(at, at.dropFirst()).allSatisfy { $1 == $0 + 1 }
    }

    /// 最初の変更を入れる（メッセージを直すときは、そのままメッセージの欄を開く）。
    func apply(_ preset: Preset) {
        switch preset {
        case let .reword(oid):
            if let i = rows.firstIndex(where: { $0.oid == oid }) { editMessage(at: i) }
        case let .combine(oids):
            // 選んだもののうちいちばん古いものに、ほかをまとめる。1 つだけなら、1 つ古いコミット（そのために読んで
            // ある）にまとめる。
            let indices = oids.compactMap { oid in rows.firstIndex { $0.oid == oid } }.sorted()
            for i in indices.count == 1 ? indices : Array(indices.dropLast()) where choices(at: i).contains(.squash) {
                set(.squash, at: i)
            }
        case let .drop(oid):
            if let i = rows.firstIndex(where: { $0.oid == oid }) { set(.drop, at: i) }
        case let .move(oid, to):
            if let from = rows.firstIndex(where: { $0.oid == oid }), rows.indices.contains(to) {
                rows.insert(rows.remove(at: from), at: to)
            }
        case let .squash(oid, target):
            _ = place(oid, squashingInto: target)
        }
        update()
    }

    /// 行で選べる操作。いちばん古い行と merge のすぐ上の行は、まとめる相手がない。merge の行は作り直すだけ。
    private func choices(at row: Int) -> [Action] {
        if rows[row].merge { return [.pick] }
        let noOlder = row == rows.count - 1 || rows[row + 1].merge
        return Action.allCases.filter { !(noOlder && [.squash, .fixup].contains($0)) }
    }

    private func set(_ action: Action, at i: Int) {
        rows[i].action = action
        if action == .pick || action == .drop || action == .fixup || action == .edit { rows[i].newMessage = nil }
    }

    /// メッセージの欄を開く。まとめる行なら、まとめたコミットのメッセージ（最初は、まとめるものを古い順につないだもの）。
    private func editMessage(at i: Int) {
        // merge の行は作り直すだけで、メッセージは変えられない（することの選択と同じ）。
        guard !rows[i].merge else { return }
        let combining = rows[i].action == .squash
        let initial = rows[i].newMessage ?? (combining ? combinedMessage(endingAt: i) : rows[i].message)
        Task { @MainActor in
            guard let edited = await actions.editMessage(combining ? L("Message of the Combined Commit") : L("Edit Message"),
                                                         initial: initial, ok: L("Change")) else { return }
            if !combining { rows[i].action = .reword }
            rows[i].newMessage = edited
            update()
        }
    }

    /// `i` の行（squash）までのまとまりのメッセージを、古い順につないだもの（git の squash の既定と同じ）。
    private func combinedMessage(endingAt i: Int) -> String {
        // 新しい順の並びで、i から下（古い方）へ、まとめる先の行まで。
        var j = i
        while j + 1 < rows.count, rows[j].action == .squash || rows[j].action == .fixup { j += 1 }
        return rows[i...j].reversed().filter { $0.action != .fixup }.map { $0.newMessage ?? $0.message }
            .joined(separator: "\n\n")
    }

    // MARK: - 要約と実行

    private func makeBar() -> NSView {
        let icon = NSImageView(image: AppIcons.image(symbol: "pencil.and.list.clipboard") ?? NSImage())
        icon.contentTintColor = .controlAccentColor
        summary.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        summary.lineBreakMode = .byTruncatingTail
        summary.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        preview.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        preview.textColor = .secondaryLabelColor
        preview.maximumNumberOfLines = 3
        preview.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let texts = NSStackView(views: [summary, preview])
        texts.orientation = .vertical
        texts.alignment = .leading
        texts.spacing = 2
        texts.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        moveStacked.target = self
        moveStacked.action = #selector(moveStackedToggled)
        moveStacked.controlSize = .small
        moveStacked.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        moveStacked.isHidden = true
        moveStacked.toolTip = L("For this edit only (git’s update-ref). Starts from rebase.updateRefs.")
        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(cancel))
        cancel.keyEquivalent = "\u{1b}"
        let apply = NSButton(title: L("Apply Changes"), target: self, action: #selector(applyClicked))
        apply.keyEquivalent = "\r"
        barButtons = [cancel, apply]
        return StatusBarController.row([icon, texts, NSView(), moveStacked, cancel, apply], height: 34,
                                       tint: NSColor.controlAccentColor.withAlphaComponent(0.08))
    }

    private var hasChanges: Bool {
        rows.map(\.oid) != originalOrder || rows.contains { $0.action != .pick }
    }

    private func update() {
        let count = { (a: Action) in self.rows.filter { $0.action == a }.count }
        var parts: [String] = []
        if rows.map(\.oid) != originalOrder { parts.append(L("reordered")) }
        if count(.reword) > 0 { parts.append(L("%ld messages", count(.reword))) }
        if count(.squash) + count(.fixup) > 0 { parts.append(L("%ld combined", count(.squash) + count(.fixup))) }
        if count(.drop) > 0 { parts.append(L("%ld dropped", count(.drop))) }
        if count(.edit) > 0 { parts.append(L("%ld to change files", count(.edit))) }
        summary.stringValue = parts.isEmpty ? L("No changes yet.")
            : L("Changes: %@.", ListFormatter.localizedString(byJoining: parts)) + sentenceGap + Actions.canUndo
        updatePreview()
        onChange?()
    }

    /// 変えるたびに、一緒に動くブランチとコンフリクトの見込みを読み直す（少し待ってから。続けて変えたら前のものは
    /// 捨てる）。
    private func updatePreview() {
        previewTask?.cancel()
        guard hasChanges else {
            // 変更をすべて戻したら、前の見込み（コンフリクトの印、積み重ねたブランチの選択）も消す。
            preview.stringValue = ""
            preview.isHidden = true
            moveStacked.isHidden = true
            if !conflicting.isEmpty {
                conflicting = [:]
                    }
            return
        }
        let edit: [String: Any] = ["kind": "plan", "steps": planSteps()]
        let actions = self.actions
        let choice: Bool? = touchedUpdateRefs ? moveStacked.state == .on : nil
        previewTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            let answer = await actions.rewritePreview(edit: edit, updateRefs: choice)
            guard !Task.isCancelled, let self else { return }
            if let state = answer?.updateRefs, !touchedUpdateRefs {
                configuredUpdateRefs = state.configured
                moveStacked.state = state.configured ? .on : .off
                moveStacked.isEnabled = state.available
                // 選べない理由を添える（Rebase のシートと同じ）。
                moveStacked.toolTip = state.available ? L("For this edit only (git’s update-ref). Starts from rebase.updateRefs.")
                    : L("Moving stacked branches needs git 2.38 or newer.")
            }
            // 積み重ねたブランチがあるときだけ選ばせる。
            moveStacked.isHidden = answer.map { $0.moving.isEmpty && !$0.staying.contains { $0.reason == "setting" || $0.reason == "off" } } ?? true
            let lines = answer?.lines() ?? []
            preview.stringValue = lines.joined(separator: " ")
            preview.textColor = answer?.hasConflicts == true ? .systemOrange : .secondaryLabelColor
            preview.isHidden = lines.isEmpty
            let marks = Dictionary((answer?.conflicts ?? []).map { ($0.oid, $0.files) }, uniquingKeysWith: { a, _ in a })
            if marks.keys.sorted() != conflicting.keys.sorted() {
                conflicting = marks
                        onChange?()
            }
            NotificationCenter.default.post(name: .statusBarContentChanged, object: nil)
        }
    }

    @objc private func cancel() { onFinish?(false) }

    /// コアに渡す並び（古い順）。まとまり（squash を含む）の最後の squash に、まとめたメッセージを付ける。
    private func planSteps() -> [[String: Any]] {
        var steps: [[String: Any]] = []
        let ordered = Array(rows.reversed())
        for (i, row) in ordered.enumerated() {
            var step: [String: Any] = ["oid": row.oid, "action": row.action.rawValue]
            switch row.action {
            case .reword: step["message"] = row.newMessage ?? row.message
            case .squash:
                let next = ordered.indices.contains(i + 1) ? ordered[i + 1].action : nil
                let lastSquash = !ordered[(i + 1)...].prefix { $0.action == .squash || $0.action == .fixup }
                    .contains { $0.action == .squash }
                if lastSquash || next == nil {
                    step["message"] = row.newMessage ?? combinedMessage(endingAt: rows.count - 1 - i)
                }
            default: break
            }
            steps.append(step)
        }
        return steps
    }

    /// 帯の Cancel と Apply Changes（書き換えている間は押せない）。
    private var barButtons: [NSButton] = []

    /// 書き換えが終わってから編集モードを閉じる。書き換えられなければ（失敗を知らせて）編集モードと選んだものを
    /// 残す（閉じてしまうと、組んだ手順がすべて消える）。途中で止まったら（コンフリクト・edit）閉じる。
    @objc private func applyClicked() {
        guard hasChanges else {
            onFinish?(false)
            return
        }
        guard barButtons.allSatisfy(\.isEnabled) else { return }
        let steps = planSteps()
        barButtons.forEach { $0.isEnabled = false }
        let updateRefs = updateRefs
        Task { @MainActor [weak self] in
            guard let self else { return }
            // 前と後の絵を見せて確かめる（merge を越える編集では描けないので、そのまま）。
            let answer = await actions.rewritePreview(edit: ["kind": "plan", "steps": steps], updateRefs: updateRefs)
            if let drawing = answer?.editDrawing {
                let alert = actions.makeAlert(L("Apply These Changes?"), (answer?.lines() ?? []).joined(separator: " "))
                alert.addButton(withTitle: L("Apply Changes"))
                alert.addButton(withTitle: L("Cancel"))
                alert.accessoryView = EditDrawingView(drawing, current: answer?.current)
                alert.accessoryView?.frame.size = alert.accessoryView?.intrinsicContentSize ?? .zero
                guard await actions.present(alert) == .alertFirstButtonReturn else {
                    barButtons.forEach { $0.isEnabled = true }
                    return
                }
            }
            actions.rewrite(
                L("Edit %ld Commits", rows.count), ["kind": "plan", "steps": steps], updateRefs: updateRefs,
                completion: { [weak self] _ in self?.onFinish?(true) },
                failed: { [weak self] in self?.barButtons.forEach { $0.isEnabled = true } })
        }
    }
}
