import AmonhenKit
import AppKit
import Testing

@testable import AmonHen

/// Markdown の図は、2 回目からは画素にした絵を描く（描き直しのたびに CoreSVG で SVG を描かない）。
@MainActor @Test func diagramsDrawFromTheirBitmapAfterTheFirstTime() throws {
    let svg = try #require(MarkdownBlock.sampleDiagram)
    let image = try #require(DiagramPicture.image(svg, own: nil))
    let rep = try #require(NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(image.size.width * 2), pixelsHigh: Int(image.size.height * 2),
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0))
    func time(_ body: () -> Void) -> Double {
        let start = DispatchTime.now().uptimeNanoseconds
        body()
        return Double(DispatchTime.now().uptimeNanoseconds - start) / 1e6
    }
    NSGraphicsContext.saveGraphicsState()
    defer { NSGraphicsContext.restoreGraphicsState() }
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.cgContext.scaleBy(x: 2, y: 2)
    let rect = NSRect(origin: .zero, size: image.size)
    let first = time { image.draw(in: rect) }
    let again = (0..<20).map { _ in time { image.draw(in: rect) } }.sorted()[10]
    print("diagram draw: first \(first) ms, then \(again) ms")
    #expect(again < first)
}
