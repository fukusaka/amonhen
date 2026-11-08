import AmonhenKit
import AppKit

/// ref を動かしたときの見込み（エンジンの `movePreview`）。Force Push・Push・Reset の確認に描く。
nonisolated struct MovePreview: Decodable, Sendable {
    struct Drawing: Decodable, Sendable {
        /// 行き先だけから辿れるコミット（新しい順、先頭のいくつか）と、ほかにいくつあるか。
        let gained: [RewritePreview.Drawing.Commit]
        let gainedMore: Int
        /// 今のところだけから辿れる、外れるコミット（新しい順、先頭のいくつか）と、ほかにいくつあるか。
        let lost: [RewritePreview.Drawing.Commit]
        let lostMore: Int
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

    init(moving: String, kind: String, afterTitle: String, lostNote: String, lostColor: NSColor?) {
        self.moving = moving
        movingKind = kind
        self.afterTitle = afterTitle
        self.lostNote = lostNote
        self.lostColor = lostColor
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
            var more: [(y: CGFloat, count: Int, lane: Int)] = []
            for c in d.gained {
                rows.append(GraphRow(commit: c, lane: 0, dim: false, y: y))
                y += Self.rowHeight
            }
            if d.gainedMore > 0 {
                more.append((y, d.gainedMore, 0))
                y += Self.rowHeight
            }
            for c in d.lost {
                var row = GraphRow(commit: c, lane: lostLane, dim: after, y: y)
                if after {
                    row.hollow = true
                    row.note = lostNote
                    row.noteColor = lostColor
                }
                rows.append(row)
                y += Self.rowHeight
            }
            if d.lostMore > 0 {
                more.append((y, d.lostMore, lostLane))
                y += Self.rowHeight
            }
            drawColumn(x: x, rows: rows, baseY: y, base: d.base, moreY: nil, more: 0,
                       highlightTop: after ? d.gained.count : 0, labels: { oid in
                var out = (labels[oid] ?? []).filter { $0.name != self.moving }.map { ($0.name, $0.kind, false) }
                // 動かす ref は、左では今のところ、右では行き先に、塗った札で。
                let here = after ? oid == destination : labels[oid]?.contains { $0.name == self.moving } == true
                if here { out.insert((self.moving, self.movingKind, true), at: 0) }
                return out
            })
            let laneX = { (lane: Int) in x + 10 + CGFloat(lane) * Self.laneWidth }
            for m in more {
                NSString(string: L("and %ld more", m.count)).draw(
                    at: NSPoint(x: laneX(1) + 14, y: m.y + 3),
                    withAttributes: [.font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
                                     .foregroundColor: NSColor.tertiaryLabelColor])
            }
        }
        // 真ん中の矢印。
        NSString(string: "→").draw(
            at: NSPoint(x: Self.columnWidth + 6, y: 22 + CGFloat(max(rowCount(d) - 1, 0)) * Self.rowHeight / 2),
            withAttributes: [.font: NSFont.systemFont(ofSize: 15), .foregroundColor: NSColor.tertiaryLabelColor])
    }
}
