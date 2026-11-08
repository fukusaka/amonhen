import AmonhenKit
import AppKit

/// ref を動かしたときの見込み（エンジンの `movePreview`）。Force Push・Push・Reset の確認に描く。
nonisolated struct MovePreview: Decodable, Sendable {
    struct Drawing: Decodable, Sendable {
        /// 行き先だけから辿れるコミット（新しい順、先頭のいくつか）と、ほかにいくつあるか。
        let gained: [RewritePreview.Drawing.Commit]
        let gainedMore: Int
        /// 省いたところ（`gained` の何番目の前か）。
        let gainedMoreAt: Int?
        /// 今のところだけから辿れる、外れるコミット（新しい順、先頭のいくつか）と、ほかにいくつあるか。
        let lost: [RewritePreview.Drawing.Commit]
        let lostMore: Int
        let lostMoreAt: Int?
        /// 分かれたところ。
        let base: RewritePreview.Drawing.Commit?
        let labels: [RewritePreview.Drawing.Label]
    }

    let gainedCount: Int
    let lostCount: Int
    /// 行き先から今のところまでに変わったファイルの数。
    let files: Int
    /// 動かすブランチの上流と、それが外れるコミットをいくつ持っているか。
    let upstream: String?
    let upstreamHas: Int
    /// 同じところなら nil。
    let drawing: Drawing?
}

extension Actions {
    /// ref を `from` から `to` へ動かしたときの見込みを読む（`branch` を渡せば、その上流が外れるコミットを持つかも）。
    /// 読めなければ nil。
    /// ブランチ `branch` を消したときの見込み（`against` と分かれたところへ動かしたものとして）。読めなければ nil。
    func deletePreview(branch: String, against: String) async -> MovePreview? {
        let command: [String: Any] = ["op": "deletePreview", "branch": branch, "against": against]
        let repository = self.repository
        guard let json = try? Repository.json(command) else { return nil }
        return await Task.detached { try? repository.info(json: json, as: MovePreview.self) }.value
    }

    func movePreview(from: String, to: String, branch: String? = nil) async -> MovePreview? {
        var command: [String: Any] = ["op": "movePreview", "from": from, "to": to]
        if let branch { command["branch"] = branch }
        let repository = self.repository
        guard let json = try? Repository.json(command) else { return nil }
        return await Task.detached { try? repository.info(json: json, as: MovePreview.self) }.value
    }
}

/// ref を動かす確認（Force Push・Push・Reset）に埋め込む、前と後の絵（docs/design.md の「Previewing graph
/// changes」）。左が今、右が動かした後。行き先だけにあるコミットは左の線、外れるコミットは右の線で、分かれた
/// ところで合流する（片方しかなければ 1 本）。動かす ref の札は、右では行き先へ移る。外れるコミットは右で中を抜いた
/// 点にして、`lostNote` を添える。
final class MoveDrawingView: GraphDrawingView {
    var preview: MovePreview? {
        didSet {
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }

    /// 動かす ref（`main`、`origin/main`）と、その札の種類（`local`・`remote`）。
    private let moving: String
    private let movingKind: String
    private let afterTitle: String
    /// 外れるコミットに添える文と色（Force Push は赤、Reset は灰）。
    private let lostNote: String
    private let lostColor: NSColor?
    /// ref が消える（ブランチの削除）。右には札を描かない。
    private let removes: Bool

    init(moving: String, kind: String, afterTitle: String, lostNote: String, lostColor: NSColor?, removes: Bool = false) {
        self.moving = moving
        movingKind = kind
        self.afterTitle = afterTitle
        self.lostNote = lostNote
        self.lostColor = lostColor
        self.removes = removes
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError() }

    private func rowCount(_ d: MovePreview.Drawing) -> Int {
        d.gained.count + (d.gainedMore > 0 ? 1 : 0) + d.lost.count + (d.lostMore > 0 ? 1 : 0) + (d.base == nil ? 0 : 1)
    }

    override var intrinsicContentSize: NSSize {
        guard let d = preview?.drawing else { return NSSize(width: Self.columnWidth * 2 + 24, height: 0) }
        return NSSize(width: Self.columnWidth * 2 + 24, height: CGFloat(rowCount(d)) * Self.rowHeight + 26)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let d = preview?.drawing else { return }
        let headingAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.secondaryLabelColor,
        ]
        NSString(string: L("Now")).draw(at: NSPoint(x: 4, y: 2), withAttributes: headingAttributes)
        NSString(string: afterTitle).draw(at: NSPoint(x: Self.columnWidth + 28, y: 2), withAttributes: headingAttributes)
        let labels = Dictionary(grouping: d.labels, by: \.oid)
        // 両方にあれば 2 本（行き先の側が左）、片方だけなら 1 本。
        let lostLane = d.gained.isEmpty ? 0 : 1
        // 動かす ref の行き先（行き先だけのコミットがなければ、分かれたところ）。
        let destination = d.gained.first?.oid ?? d.base?.oid

        for after in [false, true] {
            let x = after ? Self.columnWidth + 24 : 0
            var y: CGFloat = 22
            var rows: [GraphRow] = []
            var gaps: [Gap] = []
            // 長ければ真ん中を省く（そこだけ点線にして「ほか n コミット」）。
            let gainedAt = d.gainedMore > 0 ? (d.gainedMoreAt ?? d.gained.count) : -1
            for (i, c) in d.gained.enumerated() {
                if i == gainedAt { gaps.append(Gap(lane: 0, y: y, count: d.gainedMore)); y += Self.rowHeight }
                var row = GraphRow(commit: c, lane: 0, dim: false, y: y)
                // どこからも辿れないコミットは、今は History にない（Reflog にだけある）。
                if !after, c.hidden == true {
                    row = GraphRow(commit: c, lane: 0, dim: true, y: y)
                    row.hollow = true
                    row.note = L("only in Reflog")
                }
                rows.append(row)
                y += Self.rowHeight
            }
            if gainedAt == d.gained.count { gaps.append(Gap(lane: 0, y: y, count: d.gainedMore)); y += Self.rowHeight }
            let lostAt = d.lostMore > 0 ? (d.lostMoreAt ?? d.lost.count) : -1
            for (i, c) in d.lost.enumerated() {
                if i == lostAt { gaps.append(Gap(lane: lostLane, y: y, count: d.lostMore)); y += Self.rowHeight }
                var row = GraphRow(commit: c, lane: lostLane, dim: after, y: y)
                if after {
                    row.hollow = true
                    row.note = lostNote
                    row.noteColor = lostColor
                }
                rows.append(row)
                y += Self.rowHeight
            }
            if lostAt == d.lost.count { gaps.append(Gap(lane: lostLane, y: y, count: d.lostMore)); y += Self.rowHeight }
            // 両方の側があれば、左の分かれたところに、いつ分かれたか。
            let split = !after && !d.gained.isEmpty && !d.lost.isEmpty ? Self.splitNote(d.base) : nil
            drawColumn(x: x, rows: rows, baseY: y, base: d.base, gaps: gaps,
                       highlightTop: after ? d.gained.count : 0, baseNote: split, labels: { oid in
                var out = (labels[oid] ?? []).filter { $0.name != self.moving }.map { ($0.name, $0.kind, false) }
                // 動かす ref は、左では今のところ、右では行き先に、塗った札で。
                let here = after ? oid == destination && !self.removes : labels[oid]?.contains { $0.name == self.moving } == true
                if here { out.insert((self.moving, self.movingKind, true), at: 0) }
                return out
            })
        }
        // 真ん中の矢印。
        NSString(string: "→").draw(
            at: NSPoint(x: Self.columnWidth + 6, y: 22 + CGFloat(max(rowCount(d) - 1, 0)) * Self.rowHeight / 2),
            withAttributes: [.font: NSFont.systemFont(ofSize: 15), .foregroundColor: NSColor.tertiaryLabelColor])
    }
}
