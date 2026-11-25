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

    /// 線の太さ（大きさ 1pt あたり）。SF Symbols の regular をツールバーの大きさ（18pt）で描いた線（約 1.4pt）に合わせる。
    static let strokePerPoint: CGFloat = 1.4 / 18

    /// 共通のアイコン `name` を `size` ポイントの正方形の画像にする。描く画面の倍率で、縦・横の線を画素に合わせる。
    /// ないアイコンなら nil。
    static func image(_ name: String, size: CGFloat = 16, label: String? = nil) -> NSImage? {
        let key = "\(name)@\(size)"
        if let image = cache[key] {
            return label.map { image.copy(with: $0) } ?? image
        }
        guard SharedIcons.strokes(name, size: size, scale: 1, stroke: 1) != nil else { return nil }
        let image = NSImage(size: NSSize(width: size, height: size), flipped: true) { rect in
            guard let cg = NSGraphicsContext.current?.cgContext else { return false }
            // 描く先の 1pt あたりの画素（Retina なら 2）。
            let scale = max(1, abs(cg.userSpaceToDeviceSpaceTransform.a) * size / rect.width)
            let width = strokePerPoint * size
            guard let strokes = SharedIcons.strokes(name, size: size, scale: scale, stroke: width) else { return false }
            cg.scaleBy(x: rect.width / size, y: rect.height / size)
            cg.setStrokeColor(NSColor.black.cgColor)
            cg.setLineWidth(width)
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
