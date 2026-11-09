import AppKit

/// History の編集の確認に埋め込む、前と後の絵（docs/design.md の「Previewing graph changes」）。左が今: 書き換わる
/// コミットで、drop するものは中を抜いた点、まとめる・メッセージを変える・止まるものには、そうと添える。右が後:
/// できるコミット（′ 付き。まとめたものは 1 行、メッセージを変えたものは新しい件名）。どちらも 1 本の線で、下に
/// 書き換えないコミット。
final class EditDrawingView: GraphDrawingView {
    let drawing: RewritePreview.EditDrawing
    private let current: String?

    init(_ drawing: RewritePreview.EditDrawing, current: String?) {
        self.drawing = drawing
        self.current = current
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError() }

    private func rows(before: Bool) -> Int {
        let d = drawing
        return before ? d.before.count + (d.beforeMore > 0 ? 1 : 0) : d.after.count + (d.afterMore > 0 ? 1 : 0)
    }

    override var intrinsicContentSize: NSSize {
        let count = max(rows(before: true), rows(before: false)) + (drawing.base == nil ? 0 : 1)
        return NSSize(width: Self.columnWidth * 2 + 24, height: CGFloat(count) * Self.rowHeight + 26)
    }

    override func draw(_ dirtyRect: NSRect) {
        let d = drawing
        let headingAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.secondaryLabelColor,
        ]
        NSString(string: L("Now")).draw(at: NSPoint(x: 4, y: 2), withAttributes: headingAttributes)
        NSString(string: L("After the edit")).draw(at: NSPoint(x: Self.columnWidth + 28, y: 2), withAttributes: headingAttributes)
        let labels = Dictionary(grouping: d.labels, by: \.oid)
        // 下の書き換えないコミットは、左右で同じ高さに。
        let baseY = 22 + CGFloat(max(rows(before: true), rows(before: false))) * Self.rowHeight
        for after in [false, true] {
            let x = after ? Self.columnWidth + 24 : 0
            let steps = after ? d.after : d.before
            let more = after ? d.afterMore : d.beforeMore
            let at = more > 0 ? ((after ? d.afterMoreAt : d.beforeMoreAt) ?? steps.count) : -1
            var y: CGFloat = 22
            var rows: [GraphRow] = []
            var gaps: [Gap] = []
            for (i, step) in steps.enumerated() {
                if i == at { gaps.append(Gap(lane: 0, y: y, count: more)); y += Self.rowHeight }
                let subject = after ? step.subject + "′" : step.subject
                var row = GraphRow(commit: RewritePreview.Drawing.Commit(oid: step.oid, subject: subject), lane: 0, dim: !after, y: y)
                if after {
                    if let n = step.combined, n > 0 { row.note = L("%ld commits in one", n + 1) }
                } else {
                    switch step.action {
                    case "drop":
                        row.hollow = true
                        row.note = L("dropped")
                        row.noteColor = .systemRed
                    case "squash", "fixup": row.note = L("combined with the one below")
                    case "reword": row.note = L("new message")
                    case "edit": row.note = L("stops here")
                    default: break
                    }
                }
                rows.append(row)
                y += Self.rowHeight
            }
            if at == steps.count { gaps.append(Gap(lane: 0, y: y, count: more)); y += Self.rowHeight }
            let top = steps.first?.oid
            drawColumn(x: x, rows: rows, baseY: baseY, base: d.base, gaps: gaps, highlightTop: after ? steps.count : 0,
                       labels: { oid in
                // 今のブランチの札は、左右とも一番上に。ほかの札は左（書き換える前）と、下の書き換えないコミットに。
                var out: [(String, String, Bool)] = []
                if oid == top, let current = self.current { out.append((current, "local", true)) }
                if !after || oid == d.base?.oid {
                    out += (labels[oid] ?? []).filter { $0.name != self.current }.map { ($0.name, $0.kind, false) }
                }
                return out
            })
        }
        NSString(string: "→").draw(
            at: NSPoint(x: Self.columnWidth + 6, y: 22 + CGFloat(max(rows(before: true) - 1, 0)) * Self.rowHeight / 2),
            withAttributes: [.font: NSFont.systemFont(ofSize: 15), .foregroundColor: NSColor.tertiaryLabelColor])
    }
}
