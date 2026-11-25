import CAmonhen
import CoreGraphics

/// どの FE でも同じ形で描くアイコン（`crates/icons`）の 1 本の線。座標は 16×16 の格子で、y は下向き。
public struct SharedIconStroke: @unchecked Sendable {
    public let path: CGPath
    /// 破線の長さ（空なら実線。長さ 0 の線と丸い端で点になる）。
    public let dash: [CGFloat]
}

/// どの FE でも同じ形で描くアイコン。形の決まりと経緯は `docs/icons.md`。
public enum SharedIcons {
    /// 形を決めた格子の一辺。
    public static let grid: CGFloat = 16
    /// 線の太さ（格子の単位）。端と角は丸。
    public static let strokeWidth = CGFloat(amonhen_icon_stroke_width())

    /// `name`（`crates/icons/svg` のファイル名）の線。ないアイコンなら nil。
    public static func strokes(_ name: String) -> [SharedIconStroke]? {
        let count = amonhen_icon_shape_count(name)
        guard count > 0 else { return nil }
        return (0..<Int(count)).compactMap { index in
            var len: UInt = 0, dashLen: UInt = 0
            var dash: UnsafePointer<Float>?
            guard let raw = amonhen_icon_shape(name, UInt(index), &len, &dash, &dashLen) else { return nil }
            let s = UnsafeBufferPointer(start: raw, count: Int(len))
            let path = CGMutablePath()
            var i = 0
            func p(_ k: Int) -> CGPoint { CGPoint(x: CGFloat(s[i + k]), y: CGFloat(s[i + k + 1])) }
            while i < s.count {
                switch s[i] {
                case 0: path.move(to: p(1)); i += 3
                case 1: path.addLine(to: p(1)); i += 3
                case 2: path.addCurve(to: p(5), control1: p(1), control2: p(3)); i += 7
                case 3: path.closeSubpath(); i += 1
                default: return nil
                }
            }
            let pattern = dash.map { Array(UnsafeBufferPointer(start: $0, count: Int(dashLen))).map { CGFloat($0) } } ?? []
            return SharedIconStroke(path: path, dash: pattern)
        }
    }

    /// `name` を一辺 `size`（pt）の正方形に太さ `stroke`（pt）で描く線。縦・横の線は画素（`scale` は 1pt あたりの
    /// 画素の数）に合わせてある。座標は pt で、y は下向き。ないアイコンなら nil。
    public static func strokes(_ name: String, size: CGFloat, scale: CGFloat, stroke: CGFloat) -> [SharedIconStroke]? {
        var len: UInt = 0
        guard let raw = amonhen_icon_strokes(name, Float(size), Float(scale), Float(stroke), &len) else { return nil }
        defer { amonhen_icon_strokes_free(raw, len) }
        let s = UnsafeBufferPointer(start: raw, count: Int(len))
        var at = 1
        var result: [SharedIconStroke] = []
        for _ in 0..<Int(s[0]) {
            let (n, d) = (Int(s[at]), Int(s[at + 1]))
            at += 2
            result.append(SharedIconStroke(path: path(Array(s[at..<(at + n)])), dash: s[(at + n)..<(at + n + d)].map { CGFloat($0) }))
            at += n + d
        }
        return result
    }

    /// 移動・直線・ベジェ・閉じるの列を CGPath に。
    static func path(_ s: [Float]) -> CGPath {
        let path = CGMutablePath()
        var i = 0
        func p(_ k: Int) -> CGPoint { CGPoint(x: CGFloat(s[i + k]), y: CGFloat(s[i + k + 1])) }
        while i < s.count {
            switch s[i] {
            case 0: path.move(to: p(1)); i += 3
            case 1: path.addLine(to: p(1)); i += 3
            case 2: path.addCurve(to: p(5), control1: p(1), control2: p(3)); i += 7
            case 3: path.closeSubpath(); i += 1
            default: return path
            }
        }
        return path
    }
}
