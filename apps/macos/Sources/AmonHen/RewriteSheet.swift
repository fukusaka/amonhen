import AppKit

/// rebase の確認に埋め込む、前と後の絵（docs/design.md の「Rewriting operations and stacked branches」）。左が今、
/// 右が rebase の後。History と同じく新しいものが上で、線・点・ブランチの札・件名で描く。載せ直すコミットは左で
/// 薄く、右で強調（A → A′）。一緒に動くブランチの札は右で新しいコミットへ移り、残るブランチと `origin/…` は左の
/// 元のコミットに残る。
final class RewriteDrawingView: NSView {
    var preview: RewritePreview? {
        didSet {
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }

    private static let rowHeight: CGFloat = 22
    private static let laneWidth: CGFloat = 14
    private static let columnWidth: CGFloat = 330

    override var isFlipped: Bool { true }

    override var intrinsicContentSize: NSSize {
        guard let d = preview?.drawing else { return NSSize(width: Self.columnWidth * 2 + 24, height: 0) }
        let rows = max(d.picks.count + d.onto.count + (d.ontoMore > 0 ? 1 : 0) + 1, d.picks.count + d.onto.count + 1)
        return NSSize(width: Self.columnWidth * 2 + 24, height: CGFloat(rows) * Self.rowHeight + 26)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let preview, let d = preview.drawing else { return }
        let headingAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.secondaryLabelColor,
        ]
        NSString(string: L("Now")).draw(at: NSPoint(x: 4, y: 2), withAttributes: headingAttributes)
        NSString(string: L("After the rebase")).draw(at: NSPoint(x: Self.columnWidth + 28, y: 2), withAttributes: headingAttributes)
        let top: CGFloat = 22
        let moving = Set(preview.moving.map(\.name))
        let labels = Dictionary(grouping: d.labels, by: \.oid)

        // 左: 今。上に載せ直すコミット（右の線）、その下に載せ先で進んだコミット（左の線）、いちばん下で合流。
        let picks = Array(d.picks.reversed())
        var y = top
        var rows: [(commit: RewritePreview.Drawing.Commit, lane: Int, dim: Bool, y: CGFloat)] = []
        for c in picks {
            rows.append((c, 1, true, y))
            y += Self.rowHeight
        }
        for c in d.onto {
            rows.append((c, 0, false, y))
            y += Self.rowHeight
        }
        let moreY = d.ontoMore > 0 ? y : nil
        if d.ontoMore > 0 { y += Self.rowHeight }
        let baseY = y
        drawColumn(x: 0, rows: rows, baseY: baseY, base: d.base, moreY: moreY, more: d.ontoMore, labels: { oid in
            // 左には今のブランチの札をすべて（動くものも、まだ元のところにある）。
            (labels[oid] ?? []).map { ($0.name, $0.kind, $0.name == preview.current) }
        })

        // 右: rebase の後。載せ直したコミット（A′）が載せ先の上に 1 本で続く。
        var after: [(commit: RewritePreview.Drawing.Commit, lane: Int, dim: Bool, y: CGFloat)] = []
        y = top
        for c in picks {
            after.append((RewritePreview.Drawing.Commit(oid: c.oid, subject: c.subject + "′"), 0, false, y))
            y += Self.rowHeight
        }
        for c in d.onto {
            after.append((c, 0, true, y))
            y += Self.rowHeight
        }
        let afterMoreY = d.ontoMore > 0 ? y : nil
        if d.ontoMore > 0 { y += Self.rowHeight }
        drawColumn(x: Self.columnWidth + 24, rows: after, baseY: y, base: d.base, moreY: afterMoreY, more: d.ontoMore,
                   highlightTop: picks.count, labels: { oid in
            var out: [(String, String, Bool)] = []
            // 今のブランチは先頭の A′ へ、一緒に動くブランチは、元の先頭に対応する A′ へ。
            if oid == picks.first?.oid, let current = preview.current { out.append((current, "local", true)) }
            for label in labels[oid] ?? [] where label.kind == "local" && moving.contains(label.name) {
                out.append((label.name, "moved", false))
            }
            // 載せ先のコミットに付いている札は、そのまま。
            if d.onto.contains(where: { $0.oid == oid }) || d.base?.oid == oid {
                out += (labels[oid] ?? []).filter { $0.name != preview.current && !moving.contains($0.name) }
                    .map { ($0.name, $0.kind, false) }
            }
            return out
        })
        // 真ん中の矢印。
        let arrow = NSString(string: "→")
        arrow.draw(at: NSPoint(x: Self.columnWidth + 6, y: top + CGFloat(max(picks.count - 1, 0)) * Self.rowHeight / 2),
                   withAttributes: [.font: NSFont.systemFont(ofSize: 15), .foregroundColor: NSColor.tertiaryLabelColor])
    }

    /// 1 列を描く。`rows` の lane 1 は右の線（いちばん下で lane 0 へ合流）。`highlightTop` は強調する上からの行数。
    private func drawColumn(
        x: CGFloat, rows: [(commit: RewritePreview.Drawing.Commit, lane: Int, dim: Bool, y: CGFloat)], baseY: CGFloat,
        base: RewritePreview.Drawing.Commit?, moreY: CGFloat?, more: Int, highlightTop: Int = 0,
        labels: (String) -> [(String, String, Bool)]
    ) {
        let laneX = { (lane: Int) in x + 10 + CGFloat(lane) * Self.laneWidth }
        let center = { (y: CGFloat) in y + Self.rowHeight / 2 }
        // 線。
        let path = NSBezierPath()
        path.lineWidth = 2
        let lane1 = rows.filter { $0.lane == 1 }
        if let first = lane1.first, let last = lane1.last {
            path.move(to: NSPoint(x: laneX(1), y: center(first.y)))
            path.line(to: NSPoint(x: laneX(1), y: center(last.y)))
            path.curve(to: NSPoint(x: laneX(0), y: center(baseY)), controlPoint1: NSPoint(x: laneX(1), y: center(baseY) - 6),
                       controlPoint2: NSPoint(x: laneX(0), y: center(last.y) + 8))
        }
        let lane0 = rows.filter { $0.lane == 0 }
        if let first = lane0.first {
            path.move(to: NSPoint(x: laneX(0), y: center(first.y)))
            path.line(to: NSPoint(x: laneX(0), y: center(baseY)))
        }
        NSColor.secondaryLabelColor.withAlphaComponent(0.5).setStroke()
        path.stroke()
        // 点と件名、札。
        for (i, row) in rows.enumerated() {
            let highlighted = i < highlightTop
            drawRow(at: NSPoint(x: laneX(row.lane), y: row.y), textX: laneX(1) + 14, width: x + Self.columnWidth,
                    subject: row.commit.subject, dim: row.dim, highlighted: highlighted, labels: labels(row.commit.oid))
        }
        if let moreY {
            NSString(string: L("and %ld more", more)).draw(
                at: NSPoint(x: laneX(1) + 14, y: moreY + 3),
                withAttributes: [.font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.tertiaryLabelColor])
        }
        if let base {
            drawRow(at: NSPoint(x: laneX(0), y: baseY), textX: laneX(1) + 14, width: x + Self.columnWidth, subject: base.subject,
                    dim: true, highlighted: false, labels: labels(base.oid))
        }
    }

    private func drawRow(at dot: NSPoint, textX: CGFloat, width: CGFloat, subject: String, dim: Bool, highlighted: Bool,
                         labels: [(String, String, Bool)]) {
        let r: CGFloat = highlighted ? 5 : 4
        let circle = NSBezierPath(ovalIn: NSRect(x: dot.x - r, y: dot.y + Self.rowHeight / 2 - r, width: r * 2, height: r * 2))
        (highlighted ? NSColor.controlAccentColor : dim ? NSColor.tertiaryLabelColor : NSColor.secondaryLabelColor).setFill()
        circle.fill()
        var x = textX
        for (name, kind, current) in labels {
            x = drawLabel(name, kind: kind, current: current, at: NSPoint(x: x, y: dot.y + 3)) + 4
        }
        let attributes: [NSAttributedString.Key: Any] = [
            .font: highlighted ? NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize) : NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
            .foregroundColor: dim ? NSColor.tertiaryLabelColor : NSColor.labelColor,
        ]
        let text = NSString(string: subject)
        text.draw(with: NSRect(x: x, y: dot.y + 4, width: max(0, width - x), height: Self.rowHeight),
                  options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine], attributes: attributes)
    }

    /// History と同じ色の札（ローカルは青、リモートは紫、一緒に動いたものは緑）。右端を返す。
    private func drawLabel(_ name: String, kind: String, current: Bool, at point: NSPoint) -> CGFloat {
        let color: NSColor = switch kind {
        case "remote": .systemPurple
        case "moved": .systemGreen
        default: .systemBlue
        }
        let font = current ? NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize - 1) : NSFont.systemFont(ofSize: NSFont.smallSystemFontSize - 1)
        let text = NSAttributedString(string: name, attributes: [.font: font, .foregroundColor: current ? NSColor.white : color])
        let size = text.size()
        let rect = NSRect(x: point.x, y: point.y, width: size.width + 10, height: size.height + 2)
        let pill = NSBezierPath(roundedRect: rect, xRadius: 4, yRadius: 4)
        (current ? color : color.withAlphaComponent(0.16)).setFill()
        pill.fill()
        text.draw(at: NSPoint(x: rect.minX + 5, y: rect.minY + 1))
        return rect.maxX
    }
}

/// rebase の確認に埋め込む、前と後の絵・見通しの文・「積み重ねたブランチも一緒に動かす」（この操作だけ。最初は git の
/// `rebase.updateRefs` のとおり、git が古ければ選べない）。切り替えると絵と文も変わる。
final class RewritePreviewPanel: NSStackView {
    private let actions: Actions
    private let drawing = RewriteDrawingView()
    private let lines = NSTextField(wrappingLabelWithString: "")
    let moveStacked = NSButton(checkboxWithTitle: L("Also move the stacked branches"), target: nil, action: nil)
    private var onto: String?
    private var configured = false
    private var touched = false
    private var task: Task<Void, Never>?
    /// 見通しが届いた（シートの大きさを合わせる）。
    var onUpdate: (() -> Void)?

    init(actions: Actions) {
        self.actions = actions
        super.init(frame: .zero)
        orientation = .vertical
        alignment = .leading
        spacing = 8
        lines.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        lines.textColor = .secondaryLabelColor
        lines.preferredMaxLayoutWidth = 684
        lines.stringValue = L("Checking for conflicts…")
        moveStacked.target = self
        moveStacked.action = #selector(toggled)
        moveStacked.isHidden = true
        moveStacked.toolTip = L("For this rebase only (git’s --update-refs). Starts from rebase.updateRefs.")
        for v in [drawing, lines, moveStacked] { addArrangedSubview(v) }
    }

    required init?(coder: NSCoder) { fatalError() }

    /// この rebase で、積み重ねたブランチを一緒に動かすかの指定（git の設定と同じなら nil）。
    var updateRefs: Bool? {
        guard touched else { return nil }
        let on = moveStacked.state == .on
        return on == configured ? nil : on
    }

    /// `rev` への rebase の見通しを読む。
    func load(onto rev: String) {
        onto = rev
        refresh()
    }

    @objc private func toggled() {
        touched = true
        refresh()
    }

    private func refresh() {
        guard let onto else { return }
        task?.cancel()
        let choice = touched ? moveStacked.state == .on : nil
        task = Task { @MainActor [weak self] in
            guard let self, let preview = await actions.rewritePreview(onto: onto, updateRefs: choice), !Task.isCancelled else { return }
            if let state = preview.updateRefs, !touched {
                configured = state.configured
                moveStacked.state = state.configured ? .on : .off
                moveStacked.isEnabled = state.available
                if !state.available { moveStacked.toolTip = L("Moving stacked branches needs git 2.38 or newer.") }
            }
            // 積み重ねたブランチがなければ、選ぶものもない。
            moveStacked.isHidden = preview.moving.isEmpty && !preview.staying.contains { $0.reason == "setting" || $0.reason == "off" }
            drawing.preview = preview
            drawing.isHidden = preview.drawing == nil
            lines.stringValue = preview.lines().joined(separator: "\n")
            lines.textColor = preview.hasConflicts ? .systemOrange : .secondaryLabelColor
            layoutSubtreeIfNeeded()
            frame.size = fittingSize
            onUpdate?()
        }
    }
}
