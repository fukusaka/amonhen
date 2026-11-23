import AmonhenKit
import AppKit
import CAmonhen

/// Markdown の整形表示（History の「すべてのファイル」）。コアが読んだブロックを、NSTextView の文字に自前で組み立てる
/// （WebKit も JavaScript も使わない）。画像はそのコミットの中のものだけを読み、外へは何も読みに行かない。リンクは
/// 押すとブラウザで開く（リポジトリの中のファイルへのリンクは、そのファイルを開く）。
final class MarkdownView: NSView, NSTextViewDelegate {
    private let scroll = NSScrollView()
    private let text: NSTextView
    /// リポジトリの中のファイルへのリンクを押した（Markdown のファイルからの相対パスを解いたもの）。
    var onOpenPath: ((String) -> Void)?
    /// 押したリンクを開く前に、そのファイル（Markdown）の場所。
    private let path: String

    /// `blocks` を組み立てる。`image` はリンクの場所（Markdown のファイルからの相対パス）からそのコミットの画像を読む。
    init(blocks: [MarkdownBlock], path: String, image: @escaping (String) -> NSImage?) {
        self.path = path
        // 表（NSTextTable）は TextKit 1 で描く。
        text = NSTextView(usingTextLayoutManager: false)
        super.init(frame: NSRect(x: 0, y: 0, width: 600, height: 400))
        text.isEditable = false
        text.isSelectable = true
        text.drawsBackground = false
        text.textContainerInset = NSSize(width: 24, height: 20)
        text.isVerticallyResizable = true
        text.autoresizingMask = .width
        text.textContainer?.widthTracksTextView = true
        text.delegate = self
        text.linkTextAttributes = [.foregroundColor: NSColor.linkColor, .cursor: NSCursor.pointingHand]
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        addSubview(scroll)
        var builder = MarkdownBuilder(image: image)
        builder.add(blocks, depth: 0)
        text.textStorage?.setAttributedString(builder.out)
        // 図の配色・構文の色のテーマを変えたら描き直す（図は描くときに色を当てる）。
        NotificationCenter.default.addObserver(self, selector: #selector(themeChanged), name: .syntaxThemeChanged, object: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    @objc private func themeChanged() { text.needsDisplay = true }

    override func layout() {
        super.layout()
        scroll.frame = bounds
        fitAttachments()
    }

    /// 絵（画像・図）を列の幅に収める（640 pt まで、高さは比を保つ）。幅が変わったときだけ直す。
    private var fittedWidth: CGFloat = 0
    private func fitAttachments() {
        guard let storage = text.textStorage, let container = text.textContainer else { return }
        let width = container.size.width - container.lineFragmentPadding * 2 - 8
        guard width > 0, abs(width - fittedWidth) > 0.5 else { return }
        fittedWidth = width
        var changed = false
        storage.enumerateAttribute(.attachment, in: NSRange(location: 0, length: storage.length)) { value, range, _ in
            guard let attachment = value as? FittingAttachment else { return }
            let size = attachment.fitting(width)
            if attachment.bounds.size != size {
                attachment.bounds = NSRect(origin: .zero, size: size)
                storage.edited(.editedAttributes, range: range, changeInLength: 0)
                changed = true
            }
        }
        if changed { text.layoutManager?.invalidateLayout(forCharacterRange: NSRange(location: 0, length: storage.length), actualCharacterRange: nil) }
    }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        let target = (link as? URL)?.absoluteString ?? (link as? String) ?? ""
        if let url = URL(string: target), let scheme = url.scheme, ["http", "https", "mailto"].contains(scheme) {
            NSWorkspace.shared.open(url)
        } else if !target.hasPrefix("#"), let resolved = Self.resolve(target, from: path) {
            onOpenPath?(resolved)
        }
        return true
    }

    /// Markdown のファイル `from` からの相対パス `link` を、リポジトリの根からのパスにする（`../` も解く）。
    static func resolve(_ link: String, from: String) -> String? {
        let target = link.split(separator: "#", maxSplits: 1).first.map(String.init) ?? link
        guard !target.isEmpty, !target.contains("://") else { return nil }
        var parts: [Substring] = target.hasPrefix("/") ? [] : (from as NSString).deletingLastPathComponent.split(separator: "/")
        for part in target.split(separator: "/") {
            if part == ".." { _ = parts.popLast() } else if part != "." { parts.append(part) }
        }
        let path = parts.joined(separator: "/").removingPercentEncoding ?? parts.joined(separator: "/")
        return path.isEmpty ? nil : path
    }
}

/// Markdown の図の絵。形は pele の SVG のまま持ち、色は描くたびに、そのときの外観・構文の色のテーマ・設定の
/// 「図の配色」で当てる（当てた絵は配色ごとに覚える）。テーマを変えても組み直さず、描き直すだけでよい。
final class DiagramPicture {
    private let svg: String
    /// 図の中で指定したテーマ（設定より先）。
    private let own: MermaidThemeConfig?
    private var colored: [String: NSImage] = [:]
    /// 配色・外観・画素の大きさごとに、画素にした絵（描き直しのたびに CoreSVG で SVG を描き直さない）。
    private var bitmaps: [String: NSImage] = [:]

    private init(svg: String, own: MermaidThemeConfig?) {
        self.svg = svg
        self.own = own
    }

    static func image(_ svg: String, own: MermaidThemeConfig?) -> NSImage? {
        let picture = DiagramPicture(svg: svg, own: own)
        // 大きさは配色によらないので、pele の既定の色で一度当てて測る。
        guard let first = MarkdownBlock.colorizeDiagram(svg, scheme: "pele", dark: false, theme: [:]).flatMap(NSImage.init(data:))
        else { return nil }
        let image = NSImage(size: first.size, flipped: false) { rect in
            // 文字の描画（メインスレッド）から呼ばれる。
            MainActor.assumeIsolated { picture.draw(in: rect) }
            return true
        }
        image.cacheMode = .never
        return image
    }

    /// `rect` に描く。今の配色と、描く先の画素の大きさで画素にした絵を覚えておき、次からはそれを描く。
    func draw(in rect: NSRect) {
        guard let (key, vector) = current() else { return }
        let scale = NSGraphicsContext.current.map { abs($0.cgContext.userSpaceToDeviceSpaceTransform.a) } ?? 2
        let width = Int((rect.width * scale).rounded(.up)), height = Int((rect.height * scale).rounded(.up))
        guard width > 0, height > 0 else { return }
        let bitmapKey = "\(key)\n\(width)x\(height)"
        if let bitmap = bitmaps[bitmapKey] {
            bitmap.draw(in: rect)
            return
        }
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4,
            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
        else { return vector.draw(in: rect) }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        vector.draw(in: NSRect(x: 0, y: 0, width: width, height: height))
        NSGraphicsContext.restoreGraphicsState()
        let bitmap = NSImage(size: rect.size)
        bitmap.addRepresentation(rep)
        // 配色・外観を変えたら古い絵は要らない（大きさ違いは残す: 列の幅を変えている途中）
        bitmaps = bitmaps.filter { $0.key.hasPrefix(key + "\n") }
        bitmaps[bitmapKey] = bitmap
        bitmap.draw(in: rect)
    }

    /// 今の外観（描いている最中のもの）での絵と、その配色の鍵。
    private func current() -> (String, NSImage)? {
        let dark = NSAppearance.currentDrawing().bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let scheme = Settings.diagramColors
        let themeName = dark ? Settings.syntaxThemeDark : Settings.syntaxThemeLight
        let imported = scheme.hasPrefix("mermaid-import:")
            ? Settings.importedMermaidThemes[String(scheme.dropFirst("mermaid-import:".count))] : nil
        let key = "\(scheme)\n\(themeName)\n\(dark)\n\(imported?.json ?? "")"
        if let image = colored[key] { return (key, image) }
        var theme: [String: Any] = [:]
        if scheme == "syntax" {
            let syntax = SyntaxTheme.named(themeName) ?? .system
            var kinds: [String: String] = [:]
            for (name, kind) in SyntaxTheme.kinds { kinds[name] = Self.hex(syntax.color(kind)) }
            theme = ["text": Self.hex(.labelColor), "background": Self.hex(.textBackgroundColor), "kinds": kinds]
        }
        let image = MarkdownBlock.colorizeDiagram(svg, scheme: scheme, dark: dark, theme: theme, own: own, imported: imported)
            .flatMap(NSImage.init(data:))
        colored[key] = image
        return image.map { (key, $0) }
    }

    /// 今の外観での色の `#rrggbb`。
    private static func hex(_ color: NSColor) -> String {
        guard let c = color.usingColorSpace(.sRGB) else { return "#000000" }
        let v = { (x: CGFloat) in Int((max(0, min(1, x)) * 255).rounded()) }
        return String(format: "#%02x%02x%02x", v(c.redComponent), v(c.greenComponent), v(c.blueComponent))
    }
}

/// 列の幅に縮む絵。
nonisolated final class FittingAttachment: NSTextAttachment {
    private let natural: NSSize

    init(image: NSImage) {
        natural = image.size
        super.init(data: nil, ofType: nil)
        self.image = image
        bounds = NSRect(origin: .zero, size: fitting(640))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// 幅 `width`（と 640 pt）に収めた大きさ。
    func fitting(_ width: CGFloat) -> NSSize {
        let limit = min(width, 640)
        guard natural.width > limit, natural.width > 0 else { return natural }
        return NSSize(width: limit, height: natural.height * limit / natural.width)
    }
}

/// ブロックを文字に組み立てる。
private struct MarkdownBuilder {
    let image: (String) -> NSImage?
    let out = NSMutableAttributedString()
    private let bodySize: CGFloat = 14

    init(image: @escaping (String) -> NSImage?) { self.image = image }

    private func paragraph(indent: CGFloat = 0, spacing: CGFloat = 10, blocks: [NSTextBlock] = []) -> NSMutableParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.paragraphSpacing = spacing
        style.lineSpacing = 2
        style.headIndent = indent
        style.firstLineHeadIndent = indent
        style.textBlocks = blocks
        return style
    }

    /// 飾りの文字。
    private func runs(_ runs: [MarkdownBlock.Run], size: CGFloat, bold: Bool = false, style: NSParagraphStyle) -> NSAttributedString {
        let result = NSMutableAttributedString()
        for run in runs {
            if run.lineBreak == true {
                result.append(NSAttributedString(string: "\u{2028}", attributes: [.paragraphStyle: style]))
                continue
            }
            if let src = run.image {
                if let picture = image(src) {
                    // 列より広い絵は縮める（MarkdownView.fitAttachments）。
                    result.append(NSAttributedString(attachment: FittingAttachment(image: picture)))
                } else {
                    // 読めない画像（外の URL など）は代わりの文。
                    result.append(NSAttributedString(string: "[\(run.text.isEmpty ? src : run.text)]", attributes: [
                        .font: NSFont.systemFont(ofSize: size), .foregroundColor: NSColor.secondaryLabelColor, .paragraphStyle: style,
                    ]))
                }
                continue
            }
            var font: NSFont = run.code == true || run.html == true
                ? .monospacedSystemFont(ofSize: size - 1.5, weight: .regular) : .systemFont(ofSize: size)
            var traits: NSFontDescriptor.SymbolicTraits = []
            if bold || run.bold == true { traits.insert(.bold) }
            if run.italic == true { traits.insert(.italic) }
            if !traits.isEmpty {
                font = NSFont(descriptor: font.fontDescriptor.withSymbolicTraits(traits), size: font.pointSize) ?? font
            }
            var attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.labelColor, .paragraphStyle: style]
            if run.code == true { attributes[.backgroundColor] = NSColor.quaternaryLabelColor.withAlphaComponent(0.18) }
            if run.html == true { attributes[.foregroundColor] = NSColor.secondaryLabelColor }
            if run.strike == true { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
            if let link = run.link { attributes[.link] = link }
            if run.customEmoji != nil {
                // GitHub 独自の絵文字は画像を持たないので、名前を薄い札で見せる。
                attributes[.font] = NSFont.systemFont(ofSize: size - 2)
                attributes[.foregroundColor] = NSColor.secondaryLabelColor
                attributes[.backgroundColor] = NSColor.quaternaryLabelColor.withAlphaComponent(0.25)
                attributes[.toolTip] = L("GitHub’s own emoji, shown by name (its image isn’t included)")
            }
            var text = run.text
            if let note = run.footnote {
                text = "[\(note)]"
                attributes[.baselineOffset] = size * 0.35
                attributes[.font] = NSFont.systemFont(ofSize: size * 0.7)
            }
            result.append(NSAttributedString(string: text, attributes: attributes))
        }
        return result
    }

    private func newline(_ style: NSParagraphStyle) -> NSAttributedString {
        NSAttributedString(string: "\n", attributes: [.paragraphStyle: style, .font: NSFont.systemFont(ofSize: bodySize)])
    }

    /// 引用・囲みの枠（左に縦の線）。
    private func quoteBlock(_ color: NSColor) -> NSTextBlock {
        let block = NSTextBlock()
        // 幅を決めないと、枠の中の幅が 0 になる。
        block.setValue(100, type: .percentageValueType, for: .width)
        block.setWidth(4, type: .absoluteValueType, for: .border, edge: .minX)
        block.setBorderColor(color, for: .minX)
        block.setWidth(12, type: .absoluteValueType, for: .padding, edge: .minX)
        block.setWidth(2, type: .absoluteValueType, for: .padding, edge: .minY)
        block.setWidth(2, type: .absoluteValueType, for: .padding, edge: .maxY)
        return block
    }

    mutating func add(_ blocks: [MarkdownBlock], depth: Int, outer: [NSTextBlock] = [], indent: CGFloat = 0) {
        for block in blocks {
            switch block {
            case .heading(let level, let content):
                let sizes: [CGFloat] = [26, 21, 17, 15, 14, 13]
                let style = paragraph(indent: indent, spacing: level <= 2 ? 12 : 8, blocks: outer)
                style.paragraphSpacingBefore = out.length == 0 ? 0 : 10
                out.append(runs(content, size: sizes[min(max(level, 1), 6) - 1], bold: true, style: style))
                out.append(newline(style))
            case .paragraph(let content):
                let style = paragraph(indent: indent, blocks: outer)
                out.append(runs(content, size: bodySize, style: style))
                out.append(newline(style))
            case .code(let language, let code):
                let box = NSTextBlock()
                box.setValue(100, type: .percentageValueType, for: .width)
                box.backgroundColor = NSColor.quaternaryLabelColor.withAlphaComponent(0.14)
                box.setWidth(10, type: .absoluteValueType, for: .padding)
                let style = paragraph(indent: indent, spacing: 10, blocks: outer + [box])
                style.lineSpacing = 1
                let start = out.length
                // 行は段落を分けずに改行する（段落の間隔を行ごとに付けない）。長さは変わらないので構文の色の位置はそのまま。
                out.append(NSAttributedString(string: code.replacingOccurrences(of: "\n", with: "\u{2028}"), attributes: [
                    .font: NSFont.monospacedSystemFont(ofSize: 12.5, weight: .regular), .foregroundColor: NSColor.labelColor,
                    .paragraphStyle: style,
                ]))
                // 構文の色（フェンスの言葉を拡張子とみて）。
                let palette = SyntaxColors.palette
                if let spans = SyntaxSpans.highlight(path: "code." + Self.fileExtension(language), text: code) {
                    spans.withSpans { all in
                        for span in all where Int(span.kind) < palette.count && span.end > span.start {
                            let range = NSRange(location: start + Int(span.start), length: Int(span.end - span.start))
                            if NSMaxRange(range) <= out.length { out.addAttribute(.foregroundColor, value: palette[Int(span.kind)], range: range) }
                        }
                    }
                }
                out.append(newline(style))
            case .diagram(let language, let code, let svg, let config, let error):
                guard let picture = svg.flatMap({ DiagramPicture.image($0, own: config) }) else {
                    // 描けない図はコードとして見せ、理由を添える。
                    add([.code(language: language, text: code)], depth: depth, outer: outer, indent: indent)
                    if let error {
                        let style = paragraph(indent: indent, spacing: 10, blocks: outer)
                        // 誤りの位置を ^ で指すので等幅で。行は段落を分けずに改行する。
                        out.append(NSAttributedString(string: error.replacingOccurrences(of: "\n", with: "\u{2028}"), attributes: [
                            .font: NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular), .foregroundColor: NSColor.systemRed,
                            .paragraphStyle: style,
                        ]))
                        out.append(newline(style))
                    }
                    continue
                }
                let style = paragraph(indent: indent, spacing: 12, blocks: outer)
                // 列より広い図は縮める（MarkdownView.fitAttachments）。
                let text = NSMutableAttributedString(attachment: FittingAttachment(image: picture))
                text.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: text.length))
                out.append(text)
                out.append(newline(style))
            case .quote(let alert, let inner):
                let (color, title) = Self.alert(alert)
                let quote = quoteBlock(color)
                if let title {
                    let style = paragraph(spacing: 4, blocks: outer + [quote])
                    out.append(NSAttributedString(string: title, attributes: [
                        .font: NSFont.boldSystemFont(ofSize: bodySize), .foregroundColor: color, .paragraphStyle: style,
                    ]))
                    out.append(newline(style))
                }
                let before = out.length
                add(inner, depth: depth, outer: outer + [quote], indent: 0)
                if alert == nil {
                    out.addAttribute(.foregroundColor, value: NSColor.secondaryLabelColor, range: NSRange(location: before, length: out.length - before))
                }
            case .list(let start, let items):
                let step: CGFloat = 22
                for (i, item) in items.enumerated() {
                    let marker: String
                    if let task = item.task {
                        marker = task ? "☑" : "☐"
                    } else if let start {
                        marker = "\(start + i)."
                    } else {
                        marker = ["•", "◦", "▪︎"][depth % 3]
                    }
                    let before = out.length
                    add(item.blocks, depth: depth + 1, outer: outer, indent: indent + step)
                    // 項目の最初の段落の頭に印を置く（印はぶら下げる）。
                    let style = (out.attribute(.paragraphStyle, at: min(before, max(out.length - 1, 0)), effectiveRange: nil)
                        as? NSParagraphStyle)?.mutableCopy() as? NSMutableParagraphStyle ?? paragraph(indent: indent + step)
                    style.firstLineHeadIndent = indent
                    style.tabStops = [NSTextTab(textAlignment: .left, location: indent + step)]
                    style.paragraphSpacing = 4
                    let firstEnd = (out.string as NSString).range(of: "\n", range: NSRange(location: before, length: out.length - before)).location
                    if firstEnd != NSNotFound {
                        out.addAttribute(.paragraphStyle, value: style, range: NSRange(location: before, length: firstEnd - before + 1))
                    }
                    out.insert(NSAttributedString(string: marker + "\t", attributes: [
                        .font: NSFont.systemFont(ofSize: bodySize), .foregroundColor: NSColor.secondaryLabelColor, .paragraphStyle: style,
                    ]), at: before)
                }
            case .table(let aligns, let head, let rows):
                let table = NSTextTable()
                table.numberOfColumns = max(head.count, rows.map(\.count).max() ?? 0)
                table.setValue(100, type: .percentageValueType, for: .width)
                table.collapsesBorders = true
                table.hidesEmptyCells = false
                for (r, row) in ([head] + rows).enumerated() {
                    for column in 0..<table.numberOfColumns {
                        let cell = NSTextTableBlock(table: table, startingRow: r, rowSpan: 1, startingColumn: column, columnSpan: 1)
                        cell.setWidth(1, type: .absoluteValueType, for: .border)
                        cell.setBorderColor(NSColor.separatorColor)
                        cell.setWidth(6, type: .absoluteValueType, for: .padding)
                        if r == 0 { cell.backgroundColor = NSColor.quaternaryLabelColor.withAlphaComponent(0.12) }
                        let style = paragraph(spacing: 0, blocks: outer + [cell])
                        style.alignment = switch column < aligns.count ? aligns[column] : "none" {
                        case "center": .center
                        case "right": .right
                        default: .left
                        }
                        out.append(runs(column < row.count ? row[column] : [], size: bodySize - 1, bold: r == 0, style: style))
                        out.append(newline(style))
                    }
                }
                let after = paragraph(spacing: 10, blocks: outer)
                out.append(NSAttributedString(string: "\n", attributes: [.paragraphStyle: after, .font: NSFont.systemFont(ofSize: 4)]))
            case .rule:
                let line = NSTextBlock()
                line.setValue(100, type: .percentageValueType, for: .width)
                line.setWidth(1, type: .absoluteValueType, for: .border, edge: .maxY)
                line.setBorderColor(NSColor.separatorColor, for: .maxY)
                let style = paragraph(spacing: 12, blocks: outer + [line])
                out.append(NSAttributedString(string: " ", attributes: [.paragraphStyle: style, .font: NSFont.systemFont(ofSize: 4)]))
                out.append(newline(style))
            case .html(let html):
                let style = paragraph(indent: indent, blocks: outer)
                out.append(NSAttributedString(string: html, attributes: [
                    .font: NSFont.monospacedSystemFont(ofSize: 12, weight: .regular), .foregroundColor: NSColor.secondaryLabelColor,
                    .paragraphStyle: style,
                ]))
                out.append(newline(style))
            case .footnote(let label, let inner):
                let style = paragraph(indent: indent, spacing: 2, blocks: outer)
                out.append(NSAttributedString(string: "[\(label)]", attributes: [
                    .font: NSFont.systemFont(ofSize: bodySize - 2), .foregroundColor: NSColor.secondaryLabelColor, .paragraphStyle: style,
                ]))
                out.append(newline(style))
                add(inner, depth: depth, outer: outer, indent: indent + 16)
            }
        }
    }

    /// GitHub の囲みの色と見出し。
    private static func alert(_ kind: String?) -> (NSColor, String?) {
        // 鍵に前置きを付ける（「Tip」はブランチの先端の訳がすでにある）。訳がなければ前置きを落とす。
        func title(_ key: String) -> String {
            let text = L("GitHub alert: " + key)
            return text.hasPrefix("GitHub alert: ") ? String(text.dropFirst("GitHub alert: ".count)) : text
        }
        return switch kind {
        case "note": (.systemBlue, title("Note"))
        case "tip": (.systemGreen, title("Tip"))
        case "important": (.systemPurple, title("Important"))
        case "warning": (.systemOrange, title("Warning"))
        case "caution": (.systemRed, title("Caution"))
        default: (.separatorColor, nil)
        }
    }

    /// フェンスの言葉から、構文の色付けの拡張子を決める（言葉がそのまま拡張子のものはそのまま）。
    static func fileExtension(_ language: String) -> String {
        let names: [String: String] = [
            "rust": "rs", "python": "py", "javascript": "js", "typescript": "ts", "shell": "sh", "bash": "sh", "zsh": "sh",
            "ruby": "rb", "kotlin": "kt", "markdown": "md", "yaml": "yml", "csharp": "cs", "c#": "cs", "c++": "cpp",
            "objective-c": "m", "golang": "go", "jsx": "jsx", "tsx": "tsx",
        ]
        let lower = language.lowercased()
        return names[lower] ?? lower
    }
}
