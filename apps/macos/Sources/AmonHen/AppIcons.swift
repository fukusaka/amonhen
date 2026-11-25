import AmonhenKit
import AppKit

/// どの FE でも同じ形のアイコン（`crates/icons`、`docs/icons.md`）を、AppKit の画像にする。テンプレート画像なので、
/// 色はツールバーや行が付ける（SF Symbols と同じ）。
enum AppIcons {
    /// SF Symbols の名前から、同じ意味の共通のアイコンへ（置き換えたものだけ）。
    private static let bySymbol: [String: String] = [
        "arrow.triangle.branch": "branch",
    ]
    private static var cache: [String: NSImage] = [:]

    /// 共通のアイコン `name` を `size` ポイントの正方形の画像にする。ないアイコンなら nil。
    static func image(_ name: String, size: CGFloat = 16, label: String? = nil) -> NSImage? {
        let key = "\(name)@\(size)"
        if let image = cache[key] {
            return label.map { image.copy(with: $0) } ?? image
        }
        guard let strokes = SharedIcons.strokes(name) else { return nil }
        let image = NSImage(size: NSSize(width: size, height: size), flipped: true) { rect in
            guard let cg = NSGraphicsContext.current?.cgContext else { return false }
            let scale = rect.width / SharedIcons.grid
            cg.scaleBy(x: scale, y: scale)
            cg.setStrokeColor(NSColor.black.cgColor)
            cg.setLineWidth(SharedIcons.strokeWidth)
            cg.setLineCap(.round)
            cg.setLineJoin(.round)
            for stroke in strokes {
                cg.setLineDash(phase: 0, lengths: stroke.dash)
                cg.addPath(stroke.path)
                cg.strokePath()
            }
            return true
        }
        image.isTemplate = true
        cache[key] = image
        return label.map { image.copy(with: $0) } ?? image
    }

    /// SF Symbols の `symbol` を描く画像。共通のアイコンに置き換えたものはそちらを使う。
    static func image(symbol: String, size: CGFloat = 16, label: String? = nil) -> NSImage? {
        if let name = bySymbol[symbol], let image = image(name, size: size, label: label) { return image }
        return NSImage(systemSymbolName: symbol, accessibilityDescription: label)
    }
}

private extension NSImage {
    /// 読み上げの説明を付けた写し（キャッシュの画像そのものは変えない）。
    func copy(with label: String) -> NSImage {
        let image = copy() as! NSImage
        image.accessibilityDescription = label
        return image
    }
}
