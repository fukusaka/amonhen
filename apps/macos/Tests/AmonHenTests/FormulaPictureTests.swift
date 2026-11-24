import AmonhenKit
import AppKit
import Testing

@testable import AmonHen

/// Markdown の数式は、ブロックも文中の式も絵になり、文字の色で描かれる（`\color` の色はそのまま）。
@MainActor @Test func formulasDecodeAndDrawInTheTextColor() throws {
    let blocks = MarkdownBlock.parse("Euler: $e^{i\\pi} + 1 = 0$ and ${\\color{red} r}$.\n\n$$\n\\sum_{i=1}^n i\n$$\n\n$$\\nope{x}$$\n")
    guard case .paragraph(let runs, _) = blocks.first else { Issue.record("\(blocks)"); return }
    let inline = try #require(runs.compactMap(\.math).first)
    #expect(inline.width > 1 && inline.height > 0.5 && inline.depth >= 0)
    #expect(runs.contains { $0.math != nil && $0.text == "$e^{i\\pi} + 1 = 0$" && $0.code == true })
    guard case .math(let tex, let formula, let error) = blocks[1] else { Issue.record("\(blocks)"); return }
    #expect(tex == "\\sum_{i=1}^n i")
    #expect(error == nil)
    let block = try #require(formula)
    #expect(block.height + block.depth > 2)
    guard case .math(_, nil, let broken) = blocks[2] else { Issue.record("\(blocks)"); return }
    #expect(broken?.isEmpty == false)

    // 文字の色を当てると変数は残らず、決めた色は残る。
    let colored = try #require(MarkdownBlock.colorizeMath(runs.compactMap(\.math)[1].svg, foreground: "#123456"))
    let text = String(decoding: colored, as: UTF8.self)
    #expect(!text.contains("var("))
    #expect(text.contains("#ff0000"))

    // 描いた絵に、文字の色（暗い外観なら白に近い色）の画素がある。
    let (size, depth) = FormulaPicture.metrics(block, fontSize: 14)
    #expect(depth > 0)
    let image = FormulaPicture.image(block.svg, size: size)
    for (appearance, light) in [(NSAppearance.Name.aqua, false), (.darkAqua, true)] {
        let rep = try #require(NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(size.width * 2), pixelsHigh: Int(size.height * 2),
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
            bytesPerRow: 0, bitsPerPixel: 0))
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        NSGraphicsContext.current?.cgContext.scaleBy(x: 2, y: 2)
        NSAppearance(named: appearance)?.performAsCurrentDrawingAppearance {
            image.draw(in: NSRect(origin: .zero, size: size))
        }
        NSGraphicsContext.restoreGraphicsState()
        var inked = 0, bright = 0
        for y in 0..<rep.pixelsHigh {
            for x in 0..<rep.pixelsWide {
                guard let c = rep.colorAt(x: x, y: y), c.alphaComponent > 0.9 else { continue }
                inked += 1
                if c.redComponent > 0.8 { bright += 1 }
            }
        }
        #expect(inked > 100, "\(appearance.rawValue)")
        #expect(light ? bright > inked / 2 : bright == 0, "\(appearance.rawValue)")
    }
}
