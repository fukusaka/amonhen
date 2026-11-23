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
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        scroll.frame = bounds
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
                    let attachment = NSTextAttachment()
                    attachment.image = picture
                    // 列より広い絵は縮める（高さは比を保つ）。
                    let maxWidth: CGFloat = 640
                    let size = picture.size.width > maxWidth
                        ? NSSize(width: maxWidth, height: picture.size.height * maxWidth / picture.size.width) : picture.size
                    attachment.bounds = NSRect(origin: .zero, size: size)
                    result.append(NSAttributedString(attachment: attachment))
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
        switch kind {
        case "note": (.systemBlue, L("Note"))
        case "tip": (.systemGreen, L("Tip"))
        case "important": (.systemPurple, L("Important"))
        case "warning": (.systemOrange, L("Warning"))
        case "caution": (.systemRed, L("Caution"))
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
