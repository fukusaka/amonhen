import AmonhenKit
import AppKit
import Testing

@testable import AmonHen

/// `<kbd>` の文字はふつうの文字のまま（絵にしない）で、札の印が付き、写すと文字が出る。札の枠は行ごとに 1 つ。
@MainActor @Test func keyCapsStayText() throws {
    let blocks = MarkdownBlock.parse(
        "Press <kbd>Ctrl</kbd><kbd>C</kbd> or [<kbd>Esc</kbd>](https://example.com) and **<kbd>B</kbd>**.\n\n"
            + "| key | what |\n|---|---|\n| <kbd>Tab</kbd> | next |\n")
    let view = MarkdownView(blocks: blocks, path: "README.md") { _ in nil }
    let storage = try #require(view.text.textStorage)
    let string = storage.string
    #expect(string.contains("Press Ctrl"))
    #expect(string.contains("Esc"))
    #expect(string.contains("Tab"))

    var attachments = 0
    storage.enumerateAttribute(.attachment, in: NSRange(location: 0, length: storage.length)) { value, _, _ in
        if value != nil { attachments += 1 }
    }
    #expect(attachments == 0)

    // 札ごとに印（並んだ Ctrl と C は別の札）。
    var caps: [String] = []
    storage.enumerateAttribute(.amonhenKeyCap, in: NSRange(location: 0, length: storage.length)) { value, range, _ in
        if value != nil { caps.append((string as NSString).substring(with: range)) }
    }
    #expect(caps == ["Ctrl", "C", "Esc", "B", "Tab"])
    let esc = (string as NSString).range(of: "Esc")
    #expect(storage.attribute(.link, at: esc.location, effectiveRange: nil) != nil)
    let bold = (string as NSString).range(of: "B.")
    let boldFont = try #require(storage.attribute(.font, at: bold.location, effectiveRange: nil) as? NSFont)
    #expect(boldFont.fontDescriptor.symbolicTraits.contains(.bold))
    #expect(boldFont.fontDescriptor.symbolicTraits.contains(.monoSpace))

    // 写す（文字と書式付き。型は NSTextView が書けるものを、古い名前のまま渡す）。
    view.text.setSelectedRange(NSRange(location: 0, length: storage.length))
    let board = NSPasteboard.withUniqueName()
    defer { board.releaseGlobally() }
    #expect(view.text.writeSelection(to: board, types: view.text.writablePasteboardTypes))
    #expect(board.string(forType: .string)?.contains("Press CtrlC or Esc and B.") == true)
    let rtf = try #require(board.data(forType: .rtf))
    let rich = try NSAttributedString(data: rtf, options: [.documentType: NSAttributedString.DocumentType.rtf], documentAttributes: nil)
    #expect(rich.string.contains("CtrlC"))
    #expect(rich.string.contains("Tab"))

    // 札の枠: 1 行の札は 1 つ、表の中も描ける。
    let layout = try #require(view.text.layoutManager as? KeyCapLayoutManager)
    view.text.frame.size.width = 600
    layout.ensureLayout(for: try #require(view.text.textContainer))
    let ctrl = layout.capRects((string as NSString).range(of: "Ctrl"))
    #expect(ctrl.count == 1)
    let c = layout.capRects(NSRange(location: (string as NSString).range(of: "Ctrl").location + 4, length: 1))
    #expect(c.count == 1)
    // 並んだ札は重ならない。
    if let a = ctrl.first, let b = c.first { #expect(a.maxX < b.minX) }
    #expect(layout.capRects((string as NSString).range(of: "Tab")).count == 1)
}

/// 折り返した札は、行ごとに枠を描く。
@MainActor @Test func wrappedKeyCapDrawsPerLine() throws {
    let keys = Array(repeating: "Shift", count: 30).joined(separator: " + ")
    let blocks = MarkdownBlock.parse("Hold <kbd>\(keys)</kbd> now.\n")
    let view = MarkdownView(blocks: blocks, path: "README.md") { _ in nil }
    view.frame = NSRect(x: 0, y: 0, width: 300, height: 400)
    view.layoutSubtreeIfNeeded()
    let layout = try #require(view.text.layoutManager as? KeyCapLayoutManager)
    let string = view.text.string as NSString
    layout.ensureLayout(for: try #require(view.text.textContainer))
    let rects = layout.capRects(string.range(of: keys))
    #expect(rects.count > 2)
    // 行ごとに縦にずれる。
    #expect(Set(rects.map(\.minY)).count == rects.count)
}

/// 行の頭の札（段落の頭・折り返した続き・表の升の頭）は、枠の左の端がほかの行の文字の始まりにそろい、文字は枠に付かない。
@MainActor @Test func keyCapsAtLineStartAlignWithText() throws {
    let keys = Array(repeating: "Shift", count: 30).joined(separator: " + ")
    let blocks = MarkdownBlock.parse(
        "Plain text.\n\n<kbd>Esc</kbd> first.\n\nHold <kbd>\(keys)</kbd> now.\n\n"
            + "| key |\n|---|\n| plain |\n| <kbd>Tab</kbd> |\n")
    let view = MarkdownView(blocks: blocks, path: "README.md") { _ in nil }
    view.frame = NSRect(x: 0, y: 0, width: 300, height: 400)
    view.layoutSubtreeIfNeeded()
    let layout = try #require(view.text.layoutManager as? KeyCapLayoutManager)
    layout.ensureLayout(for: try #require(view.text.textContainer))
    let string = view.text.string as NSString
    /// 文字 `index` の左の端（入れ物の中の位置）。
    func start(_ index: Int) -> CGFloat {
        let glyph = layout.glyphIndexForCharacter(at: index)
        return layout.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil).minX + layout.location(forGlyphAt: glyph).x
    }
    let line = start(string.range(of: "Plain").location)

    // 段落の頭。
    let esc = string.range(of: "Esc")
    let escBox = try #require(layout.capRects(esc).first)
    #expect(abs(escBox.minX - line) < 0.5)
    #expect(start(esc.location) - escBox.minX >= KeyCapLayoutManager.padding - 0.5)

    // 折り返した続きの行（最初の行は文の途中なので、今まで通り左へはみ出す）。
    let wrapped = layout.capRects(string.range(of: keys))
    #expect(wrapped.count > 2)
    #expect(wrapped[0].minX > line + 10)
    for box in wrapped.dropFirst() { #expect(abs(box.minX - line) < 0.5) }

    // 表の升の頭: 同じ列のほかの升の文字の始まりにそろう。
    let tab = string.range(of: "Tab")
    let cell = start(string.range(of: "plain").location)
    let tabBox = try #require(layout.capRects(tab).first)
    #expect(abs(tabBox.minX - cell) < 0.5)
    #expect(start(tab.location) - tabBox.minX >= KeyCapLayoutManager.padding - 0.5)
}
