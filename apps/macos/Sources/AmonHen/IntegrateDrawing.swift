import AmonhenKit
import AppKit

/// merge したときの見込み（エンジンの `mergePreview`）。Merge・Pull の確認に描く。
nonisolated struct MergePreview: Decodable, Sendable {
    struct Drawing: Decodable, Sendable {
        /// 分かれてからのコミット（新しい順、長ければ真ん中を省く）。`ours` は今のブランチ、`theirs` は取り込む側。
        let ours: [RewritePreview.Drawing.Commit]
        let oursMore: Int
        let oursMoreAt: Int?
        let theirs: [RewritePreview.Drawing.Commit]
        let theirsMore: Int
        let theirsMoreAt: Int?
        /// 分かれたところ（日時つき）。
        let base: RewritePreview.Drawing.Commit?
        let labels: [RewritePreview.Drawing.Label]
        /// git が書く merge コミットの件名。
        let subject: String
    }

    /// fast-forward で済む（`move` に ref を動かす見込み）。
    let fastForward: Bool?
    /// 取り込むものがない。
    let upToDate: Bool?
    let move: MovePreview?
    let drawing: Drawing?
}

/// cherry-pick・revert したときの見込み（エンジンの `pickPreview`）。
nonisolated struct PickPreview: Decodable, Sendable {
    struct Drawing: Decodable, Sendable {
        /// 今の先端とその親。
        let head: [RewritePreview.Drawing.Commit]
        /// 作るコミット（新しい順、長ければ真ん中を省く）。
        let picks: [RewritePreview.Drawing.Commit]
        let picksMore: Int
        let picksMoreAt: Int?
        let labels: [RewritePreview.Drawing.Label]
    }

    let drawing: Drawing?
}

extension Actions {
    /// `theirs` を今のブランチ（`ours`）に merge したときの見込みを読む。`ffSetting` は fast-forward を決める git の
    /// 設定（Pull は `pull.ff`）。読めなければ nil。
    func mergePreview(theirs: String, ours: String = "HEAD", ffSetting: String = "merge.ff") async -> MergePreview? {
        let command: [String: Any] = ["op": "mergePreview", "theirs": theirs, "ours": ours, "ffSetting": ffSetting]
        let repository = self.repository
        guard let json = try? Repository.json(command) else { return nil }
        return await Task.detached { try? repository.info(json: json, as: MergePreview.self) }.value
    }

    /// コミット `oids`（古い順）を cherry-pick・revert したときの見込みを読む。読めなければ nil。
    func pickPreview(oids: [String], revert: Bool) async -> PickPreview? {
        let command: [String: Any] = ["op": "pickPreview", "oids": oids, "revert": revert]
        let repository = self.repository
        guard let json = try? Repository.json(command) else { return nil }
        return await Task.detached { try? repository.info(json: json, as: PickPreview.self) }.value
    }

    /// merge の見込みの絵（fast-forward なら ref が動く絵）。描くものがなければ nil。`theirs` は取り込むものの名前。
    func mergeDrawing(_ preview: MergePreview?, theirs: String, afterTitle: String = L("After the merge")) -> NSView? {
        guard let preview else { return nil }
        let current = currentBranch ?? "HEAD"
        if preview.fastForward == true, let move = preview.move, move.drawing != nil {
            let view = MoveDrawingView(moving: current, kind: "local", afterTitle: afterTitle, lostNote: "", lostColor: nil)
            view.preview = move
            return view
        }
        guard preview.drawing != nil else { return nil }
        let view = MergeDrawingView(current: current, theirs: theirs, afterTitle: afterTitle)
        view.preview = preview
        return view
    }

    /// cherry-pick・revert の見込みの絵。描くものがなければ nil。
    func pickDrawing(_ preview: PickPreview?, revert: Bool) -> NSView? {
        guard let preview, preview.drawing != nil else { return nil }
        let view = PickDrawingView(current: currentBranch ?? "HEAD", revert: revert)
        view.preview = preview
        return view
    }
}

/// merge の確認に埋め込む、前と後の絵（docs/design.md の「Previewing graph changes」）。左が今: 今のブランチ（左の
/// 線）と取り込む側（右の線）が、分かれたところで合流する。右が merge の後: 一番上に、これから作る merge コミット
/// （点線の丸）が両方の先端を親に持ち、今のブランチの札がそこへ移る。
final class MergeDrawingView: GraphDrawingView {
    var preview: MergePreview? {
        didSet {
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }

    private let current: String
    private let theirs: String
    private let afterTitle: String

    init(current: String, theirs: String, afterTitle: String) {
        self.current = current
        self.theirs = theirs
        self.afterTitle = afterTitle
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError() }

    private func rowCount(_ d: MergePreview.Drawing) -> Int {
        d.ours.count + (d.oursMore > 0 ? 1 : 0) + d.theirs.count + (d.theirsMore > 0 ? 1 : 0) + (d.base == nil ? 0 : 1)
    }

    override var intrinsicContentSize: NSSize {
        guard let d = preview?.drawing else { return NSSize(width: Self.columnWidth * 2 + 24, height: 0) }
        // 右には merge コミットの 1 行が増える。
        return NSSize(width: Self.columnWidth * 2 + 24, height: CGFloat(rowCount(d) + 1) * Self.rowHeight + 26)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let d = preview?.drawing else { return }
        let headingAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.secondaryLabelColor,
        ]
        NSString(string: L("Now")).draw(at: NSPoint(x: 4, y: 2), withAttributes: headingAttributes)
        NSString(string: afterTitle).draw(at: NSPoint(x: Self.columnWidth + 28, y: 2), withAttributes: headingAttributes)
        let labels = Dictionary(grouping: d.labels, by: \.oid)
        let oursTip = d.ours.first?.oid ?? d.base?.oid
        for after in [false, true] {
            let x = after ? Self.columnWidth + 24 : 0
            var y: CGFloat = 22
            var rows: [GraphRow] = []
            var gaps: [Gap] = []
            // 右の一番上に、これから作る merge コミット。
            let mergeY = y
            if after {
                var row = GraphRow(commit: RewritePreview.Drawing.Commit(oid: "", subject: d.subject), lane: 0, dim: false, y: y)
                row.isNew = true
                rows.append(row)
                y += Self.rowHeight
            }
            let oursAt = d.oursMore > 0 ? (d.oursMoreAt ?? d.ours.count) : -1
            for (i, c) in d.ours.enumerated() {
                if i == oursAt { gaps.append(Gap(lane: 0, y: y, count: d.oursMore)); y += Self.rowHeight }
                rows.append(GraphRow(commit: c, lane: 0, dim: false, y: y))
                y += Self.rowHeight
            }
            if oursAt == d.ours.count { gaps.append(Gap(lane: 0, y: y, count: d.oursMore)); y += Self.rowHeight }
            let theirsAt = d.theirsMore > 0 ? (d.theirsMoreAt ?? d.theirs.count) : -1
            for (i, c) in d.theirs.enumerated() {
                if i == theirsAt { gaps.append(Gap(lane: 1, y: y, count: d.theirsMore)); y += Self.rowHeight }
                rows.append(GraphRow(commit: c, lane: 1, dim: false, y: y))
                y += Self.rowHeight
            }
            if theirsAt == d.theirs.count { gaps.append(Gap(lane: 1, y: y, count: d.theirsMore)); y += Self.rowHeight }
            // 今のブランチだけが進んでいなければ（取り込む側だけ）、左の線は分かれたところから。
            drawColumn(x: x, rows: rows, baseY: y, base: d.base, gaps: gaps, highlightTop: 0,
                       baseNote: after ? nil : Self.splitNote(d.base), lane1JoinsAt: after ? mergeY : nil, labels: { oid in
                var out = (labels[oid] ?? []).filter { $0.name != self.current }.map { ($0.name, $0.kind, false) }
                // 今のブランチの札は、左では今の先端、右では merge コミットに、塗った札で。
                let here = after ? oid.isEmpty : oid == oursTip
                if here { out.insert((self.current, "local", true), at: 0) }
                return out
            })
        }
        NSString(string: "→").draw(
            at: NSPoint(x: Self.columnWidth + 6, y: 22 + CGFloat(max(rowCount(d) - 1, 0)) * Self.rowHeight / 2),
            withAttributes: [.font: NSFont.systemFont(ofSize: 15), .foregroundColor: NSColor.tertiaryLabelColor])
    }
}

/// cherry-pick・revert の確認に埋め込む、前と後の絵。左が今の先端、右が後: その上に、これから作るコミット（点線の
/// 丸。cherry-pick は件名に ′、revert は「Revert "…"」）が並び、今のブランチの札が一番上へ移る。
final class PickDrawingView: GraphDrawingView {
    var preview: PickPreview? {
        didSet {
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }

    private let current: String
    private let revert: Bool

    init(current: String, revert: Bool) {
        self.current = current
        self.revert = revert
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError() }

    private func newRows(_ d: PickPreview.Drawing) -> Int { d.picks.count + (d.picksMore > 0 ? 1 : 0) }

    override var intrinsicContentSize: NSSize {
        guard let d = preview?.drawing else { return NSSize(width: Self.columnWidth * 2 + 24, height: 0) }
        return NSSize(width: Self.columnWidth * 2 + 24, height: CGFloat(newRows(d) + d.head.count) * Self.rowHeight + 26)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let d = preview?.drawing else { return }
        let headingAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.secondaryLabelColor,
        ]
        NSString(string: L("Now")).draw(at: NSPoint(x: 4, y: 2), withAttributes: headingAttributes)
        NSString(string: revert ? L("After the revert") : L("After the cherry-pick"))
            .draw(at: NSPoint(x: Self.columnWidth + 28, y: 2), withAttributes: headingAttributes)
        let labels = Dictionary(grouping: d.labels, by: \.oid)
        let tip = d.head.first?.oid
        let firstNew = d.picks.first.map { "new:" + $0.oid }
        for after in [false, true] {
            let x = after ? Self.columnWidth + 24 : 0
            // 左は今の先端を、右の作るコミットと同じ高さに揃えず、上から描く。
            var y: CGFloat = 22
            var rows: [GraphRow] = []
            var gaps: [Gap] = []
            if after {
                let at = d.picksMore > 0 ? (d.picksMoreAt ?? d.picks.count) : -1
                for (i, c) in d.picks.enumerated() {
                    if i == at { gaps.append(Gap(lane: 0, y: y, count: d.picksMore)); y += Self.rowHeight }
                    let subject = revert ? c.subject : c.subject + "′"
                    var row = GraphRow(commit: RewritePreview.Drawing.Commit(oid: "new:" + c.oid, subject: subject), lane: 0, dim: false, y: y)
                    row.isNew = true
                    rows.append(row)
                    y += Self.rowHeight
                }
                if at == d.picks.count { gaps.append(Gap(lane: 0, y: y, count: d.picksMore)); y += Self.rowHeight }
            }
            for (i, c) in d.head.enumerated() {
                rows.append(GraphRow(commit: c, lane: 0, dim: after || i > 0, y: y))
                y += Self.rowHeight
            }
            drawColumn(x: x, rows: rows, baseY: y - Self.rowHeight, base: nil, gaps: gaps, labels: { oid in
                var out = (labels[oid] ?? []).filter { $0.name != self.current }.map { ($0.name, $0.kind, false) }
                let here = after ? oid == firstNew : oid == tip
                if here { out.insert((self.current, "local", true), at: 0) }
                return out
            })
        }
        NSString(string: "→").draw(
            at: NSPoint(x: Self.columnWidth + 6, y: 22 + CGFloat(max(newRows(d) - 1, 0)) * Self.rowHeight / 2),
            withAttributes: [.font: NSFont.systemFont(ofSize: 15), .foregroundColor: NSColor.tertiaryLabelColor])
    }
}
