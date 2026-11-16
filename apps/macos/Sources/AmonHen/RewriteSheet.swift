import AppKit

/// 確認のシートに埋め込む、コミットグラフの前と後の絵の共通の部品（rebase の `RewriteDrawingView` と、ref を動かす
/// `MoveDrawingView`）。History と同じく新しいものが上で、線・点・ブランチの札・件名で描く。
class GraphDrawingView: NSView, NSViewToolTipOwner {
    /// 描く 1 行。lane 0 は左の線、lane 1 は右の線。`hollow` は中を抜いた点、`note` は件名の後ろに添える文。
    struct GraphRow {
        let commit: RewritePreview.Drawing.Commit
        let lane: Int
        let dim: Bool
        let y: CGFloat
        var hollow = false
        var note: String?
        var noteColor: NSColor?
        /// これから作るコミット（点線の丸）。
        var isNew = false
    }

    static let rowHeight: CGFloat = 22
    static let laneWidth: CGFloat = 14
    static let columnWidth: CGFloat = 330

    override var isFlipped: Bool { true }

    /// 絵の高さは中身のとおり（積み重ねの中で余った高さを受け取って、下に空きを作らない）。
    override init(frame: NSRect) {
        super.init(frame: frame)
        setContentHuggingPriority(.required, for: .vertical)
    }

    required init?(coder: NSCoder) { fatalError() }

    /// 省いたコミットの行（`lane` の線の、`y` の行。そこだけ線を点線にして「ほか n コミット」と書く）。
    struct Gap {
        let lane: Int
        let y: CGFloat
        let count: Int
        /// 「ほか n コミット」の代わりに書く文（octopus の「ほか n 本」）。
        var text: String?
    }

    /// 札の折り返しで伸びた高さを、列ごとに覚える（`labelExtraHeight` が測るときだけ使う）。
    private var measuring = false
    private var extras: [CGFloat] = []

    /// 札を折り返して伸びる高さ（いちばん伸びる列の分）。描くのと同じ道筋を、描かずに通って測る。
    func labelExtraHeight() -> CGFloat {
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: 1, pixelsHigh: 1, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
            isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
        else { return 0 }
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        measuring = true
        defer { measuring = false }
        extras = []
        drawContent()
        return extras.max() ?? 0
    }

    /// 絵の高さの上限。これより高い絵は、絵の中でスクロールする（シートが画面からはみ出さないように）。
    static let maxHeight: CGFloat = 360
    /// 絵の全体の高さと、スクロールした量。
    private var fullHeight: CGFloat = 0
    private var scrollOffset: CGFloat = 0
    /// 「+n」の札のツールチップ（省いた札の名前）。
    private var tips: [NSView.ToolTipTag: String] = [:]

    func view(_ view: NSView, stringForToolTip tag: NSView.ToolTipTag, point: NSPoint, userData data: UnsafeMutableRawPointer?) -> String {
        tips[tag] ?? ""
    }

    /// 絵の全体の高さ `height` を覚え、見せる高さ（上限まで）を返す。
    func capped(_ height: CGFloat) -> CGFloat {
        fullHeight = height
        return min(height, Self.maxHeight)
    }

    /// 絵の中身を描く（上が 0。スクロールした分は `draw` がずらす）。
    func drawContent() {}

    override func draw(_ dirtyRect: NSRect) {
        removeAllToolTips()
        tips = [:]
        let overflow = max(0, fullHeight - bounds.height)
        scrollOffset = min(max(0, scrollOffset), overflow)
        NSGraphicsContext.saveGraphicsState()
        // 上限を超えた分は描かない（macOS 14 からは、枠の外にも描けてしまう）。
        NSBezierPath(rect: bounds).addClip()
        let shift = NSAffineTransform()
        shift.translateX(by: 0, yBy: -scrollOffset)
        shift.concat()
        drawContent()
        NSGraphicsContext.restoreGraphicsState()
        guard overflow > 0 else { return }
        // スクロールできることが分かるよう、右端に細いつまみ。
        let knob = max(24, bounds.height * bounds.height / fullHeight)
        let y = (bounds.height - knob) * scrollOffset / overflow
        NSColor.tertiaryLabelColor.setFill()
        NSBezierPath(roundedRect: NSRect(x: bounds.maxX - 5, y: y, width: 4, height: knob), xRadius: 2, yRadius: 2).fill()
    }

    override func scrollWheel(with event: NSEvent) {
        let overflow = fullHeight - bounds.height
        guard overflow > 0 else { return super.scrollWheel(with: event) }
        let delta = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * Self.rowHeight
        scrollOffset = min(max(0, scrollOffset - delta), overflow)
        needsDisplay = true
    }

    /// 「+n」の札に、省いた札の名前をツールチップで出す（`rect` は絵の中身の座標）。
    private func addTip(_ text: String, at rect: NSRect) {
        guard !measuring else { return }
        let tag = addToolTip(rect.offsetBy(dx: 0, dy: -scrollOffset), owner: self, userData: nil)
        tips[tag] = text
    }

    /// 1 行の並べ方。札と件名を文のように左から並べ、入らなければ次の段へ折り返す（札は省かず、1 つの札だけで
    /// 段に入らないときだけ真ん中を省く）。件名も残りが狭ければ次の段へ。
    struct RowLayout {
        struct Pill {
            let name: String
            let kind: String
            let current: Bool
            let x: CGFloat
            let line: Int
            let maxWidth: CGFloat
        }

        var pills: [Pill] = []
        /// 段に入りきらずに「+n」の札にまとめた札の名前。
        var hidden: [String] = []
        var subjectX: CGFloat = 0
        var subjectLine = 0
        var lines: Int { (pills.map(\.line) + [subjectLine]).max()! + 1 }
    }

    /// 折り返した段の間隔。
    static let lineStep: CGFloat = 19
    /// 件名に少なくとも残す幅（これより狭ければ件名は次の段へ）。
    static let subjectMinWidth: CGFloat = 120
    /// 札を並べる段の上限。入らない札は、最後の段の「+n」の札にまとめる。
    static let maxLabelLines = 3

    func layoutRow(textX: CGFloat, right: CGFloat, labels: [(String, String, Bool)], subjectWidth: CGFloat) -> RowLayout {
        var layout = RowLayout()
        var x = textX
        var line = 0
        let room = right - textX
        for (name, kind, current) in labels {
            let width = min(labelWidth(name, current: current), room)
            if x > textX, x + width > right {
                line += 1
                x = textX
            }
            layout.pills.append(.init(name: name, kind: kind, current: current, x: x, line: line, maxWidth: room))
            x += width + 4
        }
        // 段の上限を超えたら、超えた札と、「+n」の札が入るまで最後の段の札を、まとめる。
        if line >= Self.maxLabelLines {
            let last = Self.maxLabelLines - 1
            var hidden = layout.pills.filter { $0.line > last }
            layout.pills.removeAll { $0.line > last }
            func end() -> CGFloat {
                layout.pills.last.map { $0.x + min(labelWidth($0.name, current: $0.current), room) + 4 } ?? textX
            }
            while let tail = layout.pills.last, tail.line == last,
                  end() + labelWidth("+\(hidden.count + 1)", current: false) > right {
                hidden.insert(tail, at: 0)
                layout.pills.removeLast()
            }
            let more = layout.pills.last?.line == last ? end() : textX
            layout.pills.append(.init(name: "+\(hidden.count)", kind: "more", current: false, x: more, line: last, maxWidth: room))
            layout.hidden = hidden.map(\.name)
            line = last
            x = more + labelWidth("+\(hidden.count)", current: false) + 4
        }
        if x > textX, right - x < min(subjectWidth, Self.subjectMinWidth) {
            line += 1
            x = textX
        }
        layout.subjectX = x
        layout.subjectLine = line
        return layout
    }

    /// 1 列を描く。`rows` の lane 1 から先は右の線（それぞれ、いちばん下で lane 0 へ合流。octopus merge では
    /// いくつも）。`highlightTop` は強調する上からの行数。`gaps` は省いたところ、`baseNote` は分かれたところに
    /// 添える文（いつ分かれたか）。`lane1JoinsAt` は merge コミットの行で、そこから右の線それぞれの先頭へ下りる。
    /// 札が折り返した行は下へ伸び、その下の行・省いたところ・分かれたところは、その分だけ下がる。
    func drawColumn(
        x: CGFloat, rows: [GraphRow], baseY: CGFloat,
        base: RewritePreview.Drawing.Commit?, gaps: [Gap] = [], highlightTop: Int = 0, baseNote: String? = nil,
        lane1JoinsAt: CGFloat? = nil, labels: (String) -> [(String, String, Bool)]
    ) {
        let laneX = { (lane: Int) in x + 10 + CGFloat(lane) * Self.laneWidth }
        let lanes = max(1, rows.map(\.lane).max() ?? 1)
        let textX = laneX(lanes) + 14
        // 右端は、スクロールのつまみの分を空ける。
        let right = x + Self.columnWidth - 8
        // 先に各行を並べ、折り返した段の分だけ、下にあるものを下げる。
        func subjectWidth(_ subject: String, bold: Bool) -> CGFloat {
            let font = bold ? NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize) : NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
            return NSString(string: subject).size(withAttributes: [.font: font]).width
        }
        let layouts = rows.enumerated().map { i, row in
            layoutRow(textX: textX, right: right, labels: labels(row.commit.oid),
                      subjectWidth: subjectWidth(row.commit.subject, bold: i < highlightTop || row.isNew))
        }
        let baseLayout = base.map { layoutRow(textX: textX, right: right, labels: labels($0.oid), subjectWidth: subjectWidth($0.subject, bold: false)) }
        let grows = zip(rows, layouts).map { ($0.y, CGFloat($1.lines - 1) * Self.lineStep) }.filter { $0.1 > 0 }
        let shift = { (y: CGFloat) in y + grows.filter { $0.0 < y }.reduce(0) { $0 + $1.1 } }
        let baseGrows = CGFloat((baseLayout?.lines ?? 1) - 1) * Self.lineStep
        if measuring {
            extras.append(grows.reduce(0) { $0 + $1.1 } + baseGrows)
            return
        }
        let rows = rows.map { row in
            var moved = GraphRow(commit: row.commit, lane: row.lane, dim: row.dim, y: shift(row.y))
            moved.hollow = row.hollow
            moved.note = row.note
            moved.noteColor = row.noteColor
            moved.isNew = row.isNew
            return moved
        }
        let gaps = gaps.map { Gap(lane: $0.lane, y: shift($0.y), count: $0.count, text: $0.text) }
        let baseY = shift(baseY)
        let lane1JoinsAt = lane1JoinsAt.map(shift)

        let center = { (y: CGFloat) in y + Self.rowHeight / 2 }
        let solid = NSBezierPath()
        solid.lineWidth = 2
        let dashed = NSBezierPath()
        dashed.lineWidth = 2
        dashed.setLineDash([3, 3], count: 2, phase: 0)
        // まっすぐな線を、省いたところ（前の点から次の点まで）だけ点線にして引く。
        func vertical(_ lane: Int, from top: CGFloat, to bottom: CGFloat) {
            var y = top
            for gap in gaps.filter({ $0.lane == lane }).sorted(by: { $0.y < $1.y }) {
                let (a, b) = (center(gap.y) - Self.rowHeight, center(gap.y) + Self.rowHeight)
                guard a >= top, b <= bottom else { continue }
                solid.move(to: NSPoint(x: laneX(lane), y: y))
                solid.line(to: NSPoint(x: laneX(lane), y: a))
                dashed.move(to: NSPoint(x: laneX(lane), y: a))
                dashed.line(to: NSPoint(x: laneX(lane), y: b))
                y = b
            }
            solid.move(to: NSPoint(x: laneX(lane), y: y))
            solid.line(to: NSPoint(x: laneX(lane), y: bottom))
        }
        for lane in 1...lanes {
            let lane1 = rows.filter { $0.lane == lane }
            guard let first = lane1.first, let last = lane1.last else { continue }
            let lastY = max(last.y, gaps.filter { $0.lane == lane }.map(\.y).max() ?? last.y)
            vertical(lane, from: center(first.y), to: center(lastY))
            solid.move(to: NSPoint(x: laneX(lane), y: center(lastY)))
            solid.curve(to: NSPoint(x: laneX(0), y: center(baseY)), controlPoint1: NSPoint(x: laneX(lane), y: center(baseY) - 6),
                        controlPoint2: NSPoint(x: laneX(0), y: center(lastY) + 8))
            // merge コミットから、その線へ曲がり、取り込む側の先端まで下りる。
            if let joinY = lane1JoinsAt {
                let corner = NSPoint(x: laneX(lane), y: center(joinY))
                solid.move(to: NSPoint(x: laneX(0), y: center(joinY)))
                solid.curve(to: NSPoint(x: laneX(lane), y: center(joinY) + Self.rowHeight * 0.8), controlPoint1: corner, controlPoint2: corner)
                solid.line(to: NSPoint(x: laneX(lane), y: center(first.y)))
            }
        }
        if let first = rows.first(where: { $0.lane == 0 }) {
            vertical(0, from: center(first.y), to: center(baseY))
        }
        NSColor.secondaryLabelColor.withAlphaComponent(0.5).setStroke()
        solid.stroke()
        dashed.stroke()
        // 点と件名、札。
        for (i, row) in rows.enumerated() {
            let highlighted = i < highlightTop
            drawRow(at: NSPoint(x: laneX(row.lane), y: row.y), layout: layouts[i], right: right,
                    subject: row.commit.subject, dim: row.dim, highlighted: highlighted,
                    hollow: row.hollow, note: row.note, noteColor: row.noteColor, isNew: row.isNew)
        }
        for gap in gaps {
            NSString(string: gap.text ?? L("and %ld more", gap.count)).draw(
                at: NSPoint(x: textX, y: gap.y + 3),
                withAttributes: [.font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.tertiaryLabelColor])
        }
        if let base, let baseLayout {
            drawRow(at: NSPoint(x: laneX(0), y: baseY), layout: baseLayout, right: right, subject: base.subject,
                    dim: true, highlighted: false, note: baseNote)
        }
    }

    /// 分かれたところに添える文（「3 か月前に分かれた」）。日時がなければ nil。
    static func splitNote(_ base: RewritePreview.Drawing.Commit?) -> String? {
        guard let time = base?.time, time > 0 else { return nil }
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .short
        return L("split off %@", formatter.localizedString(for: Date(timeIntervalSince1970: TimeInterval(time)), relativeTo: Date()))
    }

    /// 1 行を描く（並べ方は `layout`）。`hollow` なら中を抜いた点（外れる・消えるコミット）、`note` は件名の後ろに
    /// `noteColor` で添える。
    func drawRow(at dot: NSPoint, layout: RowLayout, right: CGFloat, subject: String, dim: Bool, highlighted: Bool,
                 hollow: Bool = false, note: String? = nil, noteColor: NSColor? = nil, isNew: Bool = false) {
        let r: CGFloat = highlighted || isNew ? 5 : 4
        let circle = NSBezierPath(ovalIn: NSRect(x: dot.x - r, y: dot.y + Self.rowHeight / 2 - r, width: r * 2, height: r * 2))
        let dotColor = highlighted ? NSColor.controlAccentColor : dim ? NSColor.tertiaryLabelColor : NSColor.secondaryLabelColor
        if isNew {
            // これから作るコミットは、点線の丸（まだない）。
            NSColor.controlAccentColor.setStroke()
            circle.lineWidth = 1.5
            circle.setLineDash([2, 2], count: 2, phase: 0)
            circle.stroke()
        } else if hollow {
            (noteColor ?? dotColor).setStroke()
            circle.lineWidth = 1.5
            circle.stroke()
        } else {
            dotColor.setFill()
            circle.fill()
        }
        for pill in layout.pills {
            let point = NSPoint(x: pill.x, y: dot.y + 3 + CGFloat(pill.line) * Self.lineStep)
            let end = drawLabel(pill.name, kind: pill.kind, current: pill.current, at: point, maxWidth: pill.maxWidth)
            if pill.kind == "more" {
                addTip(layout.hidden.joined(separator: "\n"), at: NSRect(x: point.x, y: point.y, width: end - point.x, height: Self.lineStep))
            }
        }
        let attributes: [NSAttributedString.Key: Any] = [
            .font: highlighted || isNew ? NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize) : NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
            .foregroundColor: dim ? NSColor.tertiaryLabelColor : NSColor.labelColor,
        ]
        let text = NSMutableAttributedString(string: subject, attributes: attributes)
        if let note {
            var noteAttributes = attributes
            noteAttributes[.foregroundColor] = noteColor ?? NSColor.tertiaryLabelColor
            noteAttributes[.font] = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
            text.append(NSAttributedString(string: " — " + note, attributes: noteAttributes))
        }
        let y = dot.y + 4 + CGFloat(layout.subjectLine) * Self.lineStep
        text.draw(with: NSRect(x: layout.subjectX, y: y, width: right - layout.subjectX, height: Self.rowHeight),
                  options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }

    private static func labelFont(current: Bool) -> NSFont {
        current ? NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize - 1) : NSFont.systemFont(ofSize: NSFont.smallSystemFontSize - 1)
    }

    /// 札の幅（省かずに描いたとき）。
    func labelWidth(_ name: String, current: Bool) -> CGFloat {
        NSString(string: name).size(withAttributes: [.font: Self.labelFont(current: current)]).width + 10
    }

    /// History と同じ色の札（ローカルは青、リモートは紫、一緒に動いたものは緑）。右端を返す。`maxWidth` に入らなければ
    /// 真ん中を省く。
    func drawLabel(_ name: String, kind: String, current: Bool, at point: NSPoint, maxWidth: CGFloat = .greatestFiniteMagnitude) -> CGFloat {
        let color: NSColor = switch kind {
        case "remote": .systemPurple
        case "moved": .systemGreen
        case "more": .secondaryLabelColor
        default: .systemBlue
        }
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingMiddle
        let text = NSAttributedString(string: name, attributes: [.font: Self.labelFont(current: current),
                                                                 .foregroundColor: current ? NSColor.white : color,
                                                                 .paragraphStyle: style])
        let size = text.size()
        let textWidth = min(size.width, max(0, maxWidth - 10))
        let rect = NSRect(x: point.x, y: point.y, width: textWidth + 10, height: size.height + 2)
        let pill = NSBezierPath(roundedRect: rect, xRadius: 4, yRadius: 4)
        (current ? color : color.withAlphaComponent(0.16)).setFill()
        pill.fill()
        text.draw(with: NSRect(x: rect.minX + 5, y: rect.minY + 1, width: textWidth, height: size.height),
                  options: [.usesLineFragmentOrigin])
        return rect.maxX
    }
}

/// rebase の確認に埋め込む、前と後の絵（docs/design.md の「Rewriting operations and stacked branches」）。左が今、
/// 右が rebase の後。History と同じく新しいものが上で、線・点・ブランチの札・件名で描く。載せ直すコミットは左で
/// 薄く、右で強調（A → A′）。一緒に動くブランチの札は右で新しいコミットへ移り、残るブランチと `origin/…` は左の
/// 元のコミットに残る。
final class RewriteDrawingView: GraphDrawingView {
    var preview: RewritePreview? {
        didSet {
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }

    override var intrinsicContentSize: NSSize {
        guard let d = preview?.drawing else { return NSSize(width: Self.columnWidth * 2 + 24, height: 0) }
        let rows = d.picks.count + ((d.picksMore ?? 0) > 0 ? 1 : 0) + d.onto.count + (d.ontoMore > 0 ? 1 : 0) + 1
        return NSSize(width: Self.columnWidth * 2 + 24, height: capped(CGFloat(rows) * Self.rowHeight + 26 + labelExtraHeight()))
    }

    override func drawContent() {
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
        var rows: [GraphRow] = []
        var gaps: [Gap] = []
        // 載せ直すコミットも載せ先で進んだコミットも、長ければ真ん中を省く（そこだけ点線）。
        let picksMore = d.picksMore ?? 0
        let picksAt = picksMore > 0 ? (d.picksMoreAt ?? picks.count) : -1
        for (i, c) in picks.enumerated() {
            if i == picksAt { gaps.append(Gap(lane: 1, y: y, count: picksMore)); y += Self.rowHeight }
            rows.append(GraphRow(commit: c, lane: 1, dim: true, y: y))
            y += Self.rowHeight
        }
        if picksAt == picks.count { gaps.append(Gap(lane: 1, y: y, count: picksMore)); y += Self.rowHeight }
        let moreAt = d.ontoMore > 0 ? (d.ontoMoreAt ?? d.onto.count) : -1
        for (i, c) in d.onto.enumerated() {
            if i == moreAt { gaps.append(Gap(lane: 0, y: y, count: d.ontoMore)); y += Self.rowHeight }
            rows.append(GraphRow(commit: c, lane: 0, dim: false, y: y))
            y += Self.rowHeight
        }
        if moreAt == d.onto.count { gaps.append(Gap(lane: 0, y: y, count: d.ontoMore)); y += Self.rowHeight }
        let baseY = y
        drawColumn(x: 0, rows: rows, baseY: baseY, base: d.base, gaps: gaps, baseNote: Self.splitNote(d.base), labels: { oid in
            // 左には今のブランチの札をすべて（動くものも、まだ元のところにある）。
            (labels[oid] ?? []).map { ($0.name, $0.kind, $0.name == preview.current) }
        })

        // 右: rebase の後。載せ直したコミット（A′）が載せ先の上に 1 本で続く。
        var after: [GraphRow] = []
        var afterGaps: [Gap] = []
        y = top
        for (i, c) in picks.enumerated() {
            if i == picksAt { afterGaps.append(Gap(lane: 0, y: y, count: picksMore)); y += Self.rowHeight }
            after.append(GraphRow(commit: RewritePreview.Drawing.Commit(oid: c.oid, subject: c.subject + "′"), lane: 0, dim: false, y: y))
            y += Self.rowHeight
        }
        if picksAt == picks.count { afterGaps.append(Gap(lane: 0, y: y, count: picksMore)); y += Self.rowHeight }
        for (i, c) in d.onto.enumerated() {
            if i == moreAt { afterGaps.append(Gap(lane: 0, y: y, count: d.ontoMore)); y += Self.rowHeight }
            after.append(GraphRow(commit: c, lane: 0, dim: true, y: y))
            y += Self.rowHeight
        }
        if moreAt == d.onto.count { afterGaps.append(Gap(lane: 0, y: y, count: d.ontoMore)); y += Self.rowHeight }
        drawColumn(x: Self.columnWidth + 24, rows: after, baseY: y, base: d.base, gaps: afterGaps,
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
            guard let self else { return }
            let answer = await actions.rewritePreview(onto: onto, updateRefs: choice)
            guard !Task.isCancelled else { return }
            guard let preview = answer else {
                // 見通しを出せなければ、「確かめている」のままにせず消す（実行はできる）。
                lines.stringValue = ""
                drawing.isHidden = true
                layoutSubtreeIfNeeded()
                frame.size = fittingSize
                onUpdate?()
                return
            }
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
