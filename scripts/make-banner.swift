// README の頭のバナーを描く。図柄はアイコン（scripts/make-icon.swift）を横に広げたもの: 夜の丘の稜線を git のレーンで
// 描き、頂上の座に光る点、その上に光る星。左にアイコンと名前。
//   swift scripts/make-banner.swift [出力先のファイル]
// 出力: docs/images/banner.png（1600×400 の 2 倍の解像度）
import AppKit

let out = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "docs/images/banner.png")
let icon = NSImage(contentsOfFile: "apps/macos/Icons/AppIcon.png")!

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(
        srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
        blue: CGFloat(hex & 0xff) / 255, alpha: alpha)
}

let width: CGFloat = 1600, height: CGFloat = 400, scale: CGFloat = 2

/// 1600×400 の座標（左上が原点）で描く。
func draw(_ ctx: CGContext) {
    ctx.translateBy(x: 0, y: height)
    ctx.scaleBy(x: 1, y: -1)
    let body = CGRect(x: 0, y: 0, width: width, height: height)
    ctx.addPath(CGPath(roundedRect: body, cornerWidth: 28, cornerHeight: 28, transform: nil))
    ctx.clip()
    let space = CGColorSpace(name: CGColorSpace.sRGB)!

    // 夜空: 上が藍、下が濃紺。
    let sky = CGGradient(colorsSpace: space, colors: [color(0x2A3766), color(0x0B1022)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(sky, start: CGPoint(x: 0, y: 0), end: CGPoint(x: 0, y: height), options: [])

    // 星: 空に淡く散らす（種を決めた乱数）。名前と頂上の星のまわりは避ける。
    var seed: UInt64 = 0x5EED_BA22
    func random() -> CGFloat {
        seed = seed &* 6364136223846793005 &+ 1442695040888963407
        return CGFloat(seed >> 33) / CGFloat(1 << 31)
    }
    let polaris = CGPoint(x: 1180, y: 92)
    var placed = 0
    while placed < 70 {
        let at = CGPoint(x: random() * width, y: random() * 260)
        if hypot(at.x - polaris.x, at.y - polaris.y) < 70 { continue }
        if at.x > 70, at.x < 860, at.y > 90, at.y < 300 { continue }
        let mag = 2.4 + random() * 1.8
        let r: CGFloat = (6.2 - mag) * 0.55
        ctx.setFillColor(color(0xFFFFFF, 0.5 - mag * 0.08))
        ctx.fillEllipse(in: CGRect(x: at.x - r, y: at.y - r, width: r * 2, height: r * 2))
        placed += 1
    }

    // 頂上の後ろの光。
    let glow = CGGradient(
        colorsSpace: space, colors: [color(0xFFC94D, 0.5), color(0xE8A23A, 0.18), color(0xE8A23A, 0)] as CFArray,
        locations: [0, 0.45, 1])!
    ctx.drawRadialGradient(glow, startCenter: polaris, startRadius: 0, endCenter: polaris, endRadius: 190, options: [])

    // 奥の丘（薄く）。
    let back = CGMutablePath()
    back.move(to: CGPoint(x: 0, y: 330))
    back.addQuadCurve(to: CGPoint(x: 520, y: 300), control: CGPoint(x: 260, y: 270))
    back.addQuadCurve(to: CGPoint(x: 960, y: 250), control: CGPoint(x: 760, y: 320))
    back.addQuadCurve(to: CGPoint(x: 1600, y: 230), control: CGPoint(x: 1360, y: 150))
    back.addLine(to: CGPoint(x: 1600, y: height))
    back.addLine(to: CGPoint(x: 0, y: height))
    back.closeSubpath()
    ctx.addPath(back)
    ctx.setFillColor(color(0x26315A, 0.8))
    ctx.fillPath()

    // 手前の丘: 稜線は右寄りの頂上の座を挟んで左右に下りる。
    let ridge: [CGPoint] = [
        CGPoint(x: 0, y: 372), CGPoint(x: 420, y: 350), CGPoint(x: 820, y: 300), CGPoint(x: 1060, y: 220),
        CGPoint(x: 1180, y: 186), CGPoint(x: 1300, y: 224), CGPoint(x: 1460, y: 268), CGPoint(x: 1600, y: 300),
    ]
    let hill = CGMutablePath()
    hill.move(to: ridge[0])
    for p in ridge.dropFirst() { hill.addLine(to: p) }
    hill.addLine(to: CGPoint(x: width, y: height))
    hill.addLine(to: CGPoint(x: 0, y: height))
    hill.closeSubpath()
    ctx.saveGState()
    ctx.addPath(hill)
    ctx.clip()
    let ground = CGGradient(colorsSpace: space, colors: [color(0x34437A), color(0x111833)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(ground, start: CGPoint(x: 0, y: 186), end: CGPoint(x: 0, y: height), options: [])
    ctx.restoreGState()

    // レーン: 稜線に沿う本流（青）と、途中で分かれて下を回り、また合流する枝（緑）。
    let main = ridge.map { CGPoint(x: $0.x, y: $0.y + 2) }
    let branchA = CGPoint(x: 820, y: 302), branchB = CGPoint(x: 1460, y: 270)
    let branch = CGMutablePath()
    branch.move(to: branchA)
    branch.addCurve(to: CGPoint(x: 1150, y: 318), control1: CGPoint(x: 880, y: 350), control2: CGPoint(x: 1020, y: 330))
    branch.addCurve(to: branchB, control1: CGPoint(x: 1280, y: 306), control2: CGPoint(x: 1400, y: 320))
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    ctx.addPath(branch)
    ctx.setStrokeColor(color(0x5FD38D))
    ctx.setLineWidth(9)
    ctx.strokePath()
    let mainPath = CGMutablePath()
    mainPath.addLines(between: main)
    ctx.addPath(mainPath)
    ctx.setStrokeColor(color(0x6FB6FF))
    ctx.setLineWidth(10)
    ctx.strokePath()

    // コミットの点（地の色の縁取りで線から浮かせる）。
    func node(_ p: CGPoint, _ fill: CGColor, radius: CGFloat = 11) {
        ctx.setFillColor(color(0x141B36))
        ctx.fillEllipse(in: CGRect(x: p.x - radius - 4, y: p.y - radius - 4, width: (radius + 4) * 2, height: (radius + 4) * 2))
        ctx.setFillColor(fill)
        ctx.fillEllipse(in: CGRect(x: p.x - radius, y: p.y - radius, width: radius * 2, height: radius * 2))
    }
    node(CGPoint(x: 420, y: 352), color(0x6FB6FF))
    node(branchA, color(0x6FB6FF))
    node(CGPoint(x: 1060, y: 222), color(0x6FB6FF))
    node(CGPoint(x: 1300, y: 226), color(0x6FB6FF))
    node(branchB, color(0x6FB6FF))
    node(CGPoint(x: 1010, y: 328), color(0x5FD38D), radius: 10)
    node(CGPoint(x: 1290, y: 308), color(0x5FD38D), radius: 10)

    // 頂上の座: 光る大きな点（見る目）と、その上の四方に光る星。
    node(CGPoint(x: 1180, y: 188), color(0xFFE08A), radius: 15)
    ctx.setFillColor(color(0xFFF6D8))
    ctx.fillEllipse(in: CGRect(x: 1180 - 6, y: 188 - 6, width: 12, height: 12))
    let star = CGMutablePath()
    let long: CGFloat = 40, short: CGFloat = 8
    for i in 0..<8 {
        let angle = CGFloat(i) * .pi / 4 - .pi / 2 + 3 * .pi / 180
        let r = i % 2 == 0 ? long : short
        let p = CGPoint(x: polaris.x + cos(angle) * r, y: polaris.y + sin(angle) * r)
        if i == 0 { star.move(to: p) } else { star.addLine(to: p) }
    }
    star.closeSubpath()
    ctx.addPath(star)
    ctx.setFillColor(color(0xFFE9A8))
    ctx.fillPath()

    // 左: アイコンと名前・一言。文字は上下を戻して描く（座標を上下反転しているため）。
    let iconRect = CGRect(x: 72, y: 96, width: 200, height: 200)
    ctx.saveGState()
    ctx.translateBy(x: iconRect.minX, y: iconRect.maxY)
    ctx.scaleBy(x: 1, y: -1)
    let cg = icon.cgImage(forProposedRect: nil, context: nil, hints: nil)!
    ctx.draw(cg, in: CGRect(origin: .zero, size: iconRect.size))
    ctx.restoreGState()
    func text(_ s: String, _ size: CGFloat, _ weight: NSFont.Weight, _ fill: NSColor, at: CGPoint) {
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: s, attributes: [
            .font: NSFont.systemFont(ofSize: size, weight: weight), .foregroundColor: fill,
        ]))
        ctx.saveGState()
        ctx.translateBy(x: at.x, y: at.y)
        ctx.scaleBy(x: 1, y: -1)
        ctx.textPosition = .zero
        CTLineDraw(line, ctx)
        ctx.restoreGState()
    }
    text("Amon Hen", 84, .bold, .white, at: CGPoint(x: 296, y: 204))
    text("A quick, responsive Git GUI.", 30, .medium, NSColor(white: 1, alpha: 0.72), at: CGPoint(x: 300, y: 254))
}

let pixels = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(width * scale), pixelsHigh: Int(height * scale), bitsPerSample: 8,
    samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
let ctx = NSGraphicsContext(bitmapImageRep: pixels)!.cgContext
ctx.scaleBy(x: scale, y: scale)
draw(ctx)
try! FileManager.default.createDirectory(at: out.deletingLastPathComponent(), withIntermediateDirectories: true)
try! pixels.representation(using: .png, properties: [:])!.write(to: out)
print(out.path)
