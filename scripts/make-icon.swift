// アプリのアイコンを描いて .icns にする。図柄はコードで持つ（色や形はここを直して作り直す）。
//   swift scripts/make-icon.swift [出力先のフォルダ]
// 出力: AppIcon.icns（正式版）・AppIcon-Snapshot.icns（snapshot 版。右下にオレンジの帯）・AppIcon.png（1024 px の見本）
//
// 図柄: Amon Hen（シンダール語で「目の丘」）にちなみ、夜の丘の稜線を git のレーンで描く。稜線（本流）と、
// 分かれて下を回り合流する枝が、目の上下のまぶたになり、その間に金色の瞳を置く。空には北斗七星（物語では
// 「ヴァラールの鎌」）と、その指極星の先に北極星。macOS のアイコンの枠（1024 の中の 824 の角の丸い四角）に合わせる。
import AppKit

let outDir = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "apps/macos/Icons")

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(
        srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
        blue: CGFloat(hex & 0xff) / 255, alpha: alpha)
}

/// 1024 の座標（左上が原点）で描く。
func draw(_ ctx: CGContext, snapshot: Bool) {
    ctx.translateBy(x: 0, y: 1024)
    ctx.scaleBy(x: 1, y: -1)
    let body = CGRect(x: 100, y: 100, width: 824, height: 824)
    let shape = CGPath(roundedRect: body, cornerWidth: 185, cornerHeight: 185, transform: nil)

    // 影（アイコンの枠の外に薄く）。
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 20, color: color(0x000000, 0.22))
    ctx.addPath(shape)
    ctx.setFillColor(color(0x0B1022))
    ctx.fillPath()
    ctx.restoreGState()

    ctx.saveGState()
    ctx.addPath(shape)
    ctx.clip()
    let space = CGColorSpace(name: CGColorSpace.sRGB)!

    // 夜空: 上が藍、下が濃紺。
    let sky = CGGradient(colorsSpace: space, colors: [color(0x26325E), color(0x0A0E1E)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(sky, start: CGPoint(x: 512, y: 100), end: CGPoint(x: 512, y: 924), options: [])

    // 頂上の星（北極星の役）と、その後ろの光。
    let polaris = CGPoint(x: 512, y: 270)
    let glow = CGGradient(
        colorsSpace: space, colors: [color(0xFFC94D, 0.45), color(0xE8A23A, 0.15), color(0xE8A23A, 0)] as CFArray,
        locations: [0, 0.45, 1])!
    ctx.drawRadialGradient(glow, startCenter: polaris, startRadius: 0, endCenter: polaris, endRadius: 250, options: [])

    // 北斗七星（物語では「ヴァラールの鎌」）。柄杓の先の 2 つ（Merak→Dubhe）を延ばした先に北極星が来るように置く。
    // 形は Merak を原点、Dubhe への向きを x、柄の側を y にとった座標（指極星の間を 1 とする）。
    let unit: CGFloat = 52
    let tilt: CGFloat = -28 * CGFloat.pi / 180
    let pointer = CGPoint(x: cos(tilt), y: sin(tilt))
    let side = CGPoint(x: pointer.y, y: -pointer.x)
    let reach: CGFloat = unit * 5
    let dubhe = CGPoint(x: polaris.x - pointer.x * reach, y: polaris.y - pointer.y * reach)
    let merak = CGPoint(x: dubhe.x - pointer.x * unit, y: dubhe.y - pointer.y * unit)
    func dipperPoint(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
        let dx: CGFloat = (pointer.x * x + side.x * y) * unit
        let dy: CGFloat = (pointer.y * x + side.y * y) * unit
        return CGPoint(x: merak.x + dx, y: merak.y + dy)
    }
    // Dubhe・Merak・Phecda・Megrez（升）と、Alioth・Mizar・Alkaid（柄）。
    let dipper = [
        dipperPoint(1, 0), dipperPoint(0, 0), dipperPoint(-0.1, 1.3), dipperPoint(0.95, 1.45),
        dipperPoint(1.6, 2.25), dipperPoint(2.15, 2.85), dipperPoint(3.1, 3.2),
    ]
    let lines = CGMutablePath()
    lines.addLines(between: [dipper[0], dipper[1], dipper[2], dipper[3], dipper[0]])
    lines.addLines(between: [dipper[3], dipper[4], dipper[5], dipper[6]])
    ctx.addPath(lines)
    ctx.setStrokeColor(color(0xFFFFFF, 0.16))
    ctx.setLineWidth(3)
    ctx.strokePath()
    for (i, p) in dipper.enumerated() {
        let r: CGFloat = i == 3 ? 5 : 7
        ctx.setFillColor(color(0xFFFFFF, 0.8))
        ctx.fillEllipse(in: CGRect(x: p.x - r, y: p.y - r, width: r * 2, height: r * 2))
    }

    // 丘: 稜線の真ん中が目の上まぶたになる。左右の角から外へは、なだらかに下りる。
    let left = CGPoint(x: 280, y: 640), right = CGPoint(x: 744, y: 640)
    let upper = (CGPoint(x: 380, y: 455), CGPoint(x: 644, y: 455))
    let lower = (CGPoint(x: 380, y: 805), CGPoint(x: 644, y: 805))
    let outerLeft = CGPoint(x: 100, y: 790), outerRight = CGPoint(x: 924, y: 790)
    let hill = CGMutablePath()
    hill.move(to: outerLeft)
    hill.addLine(to: left)
    hill.addCurve(to: right, control1: upper.0, control2: upper.1)
    hill.addLine(to: outerRight)
    hill.addLine(to: CGPoint(x: 924, y: 924))
    hill.addLine(to: CGPoint(x: 100, y: 924))
    hill.closeSubpath()
    ctx.saveGState()
    ctx.addPath(hill)
    ctx.clip()
    let ground = CGGradient(colorsSpace: space, colors: [color(0x3E4F8E), color(0x1A2450)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(ground, start: CGPoint(x: 512, y: 480), end: CGPoint(x: 512, y: 924), options: [])
    ctx.restoreGState()

    // 目の中（上まぶたと下まぶたの間）を少し明るく。
    let eye = CGMutablePath()
    eye.move(to: left)
    eye.addCurve(to: right, control1: upper.0, control2: upper.1)
    eye.addCurve(to: left, control1: lower.1, control2: lower.0)
    eye.closeSubpath()
    ctx.addPath(eye)
    ctx.setFillColor(color(0x5A6CB4, 0.6))
    ctx.fillPath()

    // レーン: 本流（青）が稜線と上まぶた、分かれて下を回って合流する枝（緑）が下まぶた。
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    let branch = CGMutablePath()
    branch.move(to: left)
    branch.addCurve(to: right, control1: lower.0, control2: lower.1)
    ctx.addPath(branch)
    ctx.setStrokeColor(color(0x5FD38D))
    ctx.setLineWidth(24)
    ctx.strokePath()
    let main = CGMutablePath()
    main.move(to: outerLeft)
    main.addLine(to: left)
    main.addCurve(to: right, control1: upper.0, control2: upper.1)
    main.addLine(to: outerRight)
    ctx.addPath(main)
    ctx.setStrokeColor(color(0x6FB6FF))
    ctx.setLineWidth(26)
    ctx.strokePath()

    func bezier(_ t: CGFloat, _ c: (CGPoint, CGPoint)) -> CGPoint {
        let u = 1 - t
        let a = u * u * u, b = 3 * u * u * t, d = 3 * u * t * t, e = t * t * t
        let x: CGFloat = a * left.x + b * c.0.x + d * c.1.x + e * right.x
        let y: CGFloat = a * left.y + b * c.0.y + d * c.1.y + e * right.y
        return CGPoint(x: x, y: y)
    }
    // コミットの点（地の色の縁取りで線から浮かせる）。
    func node(_ p: CGPoint, _ fill: CGColor, radius: CGFloat = 30) {
        ctx.setFillColor(color(0x141B36))
        ctx.fillEllipse(in: CGRect(x: p.x - radius - 9, y: p.y - radius - 9, width: (radius + 9) * 2, height: (radius + 9) * 2))
        ctx.setFillColor(fill)
        ctx.fillEllipse(in: CGRect(x: p.x - radius, y: p.y - radius, width: radius * 2, height: radius * 2))
    }
    node(left, color(0x6FB6FF))
    node(right, color(0x6FB6FF))
    node(bezier(0.22, upper), color(0x6FB6FF), radius: 24)
    node(bezier(0.78, upper), color(0x6FB6FF), radius: 24)
    node(bezier(0.22, lower), color(0x5FD38D), radius: 22)
    node(bezier(0.78, lower), color(0x5FD38D), radius: 22)

    // 瞳: 金色の虹彩と、濃い瞳孔、小さな光。
    let iris = CGPoint(x: 512, y: 632), irisRadius: CGFloat = 88
    ctx.setFillColor(color(0x141B36))
    ctx.fillEllipse(in: CGRect(x: iris.x - irisRadius - 10, y: iris.y - irisRadius - 10,
                               width: (irisRadius + 10) * 2, height: (irisRadius + 10) * 2))
    ctx.saveGState()
    ctx.addEllipse(in: CGRect(x: iris.x - irisRadius, y: iris.y - irisRadius, width: irisRadius * 2, height: irisRadius * 2))
    ctx.clip()
    let gold = CGGradient(colorsSpace: space, colors: [color(0xFFE9A8), color(0xF2B44A)] as CFArray, locations: [0, 1])!
    ctx.drawRadialGradient(gold, startCenter: iris, startRadius: 0, endCenter: iris, endRadius: irisRadius, options: [])
    ctx.restoreGState()
    ctx.setFillColor(color(0x141B36))
    ctx.fillEllipse(in: CGRect(x: iris.x - 36, y: iris.y - 36, width: 72, height: 72))
    ctx.setFillColor(color(0xFFFFFF, 0.9))
    ctx.fillEllipse(in: CGRect(x: iris.x - 34, y: iris.y - 40, width: 22, height: 22))

    // 北極星（四方に光る星）。ほんのわずか（3°）傾ける。
    let star = CGMutablePath()
    let long: CGFloat = 70, short: CGFloat = 14
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

    // snapshot 版: 右下の角にオレンジの帯。
    if snapshot {
        ctx.saveGState()
        ctx.translateBy(x: 760, y: 760)
        ctx.rotate(by: -.pi / 4)
        ctx.setFillColor(color(0xFF8A3D))
        ctx.fill(CGRect(x: -330, y: -46, width: 660, height: 92))
        let text = NSAttributedString(string: "SNAPSHOT", attributes: [
            .font: NSFont.systemFont(ofSize: 50, weight: .heavy), .foregroundColor: NSColor.white,
        ])
        let line = CTLineCreateWithAttributedString(text)
        let width = CTLineGetTypographicBounds(line, nil, nil, nil)
        // 文字は上下を戻して描く（座標を上下反転しているため）。
        ctx.scaleBy(x: 1, y: -1)
        ctx.textPosition = CGPoint(x: -width / 2, y: -17)
        CTLineDraw(line, ctx)
        ctx.restoreGState()
    }
    ctx.restoreGState()
}

func png(size: Int, snapshot: Bool) -> Data {
    let ctx = CGContext(
        data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.scaleBy(x: CGFloat(size) / 1024, y: CGFloat(size) / 1024)
    draw(ctx, snapshot: snapshot)
    let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
    return rep.representation(using: .png, properties: [:])!
}

try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)
for (name, snapshot) in [("AppIcon", false), ("AppIcon-Snapshot", true)] {
    let iconset = FileManager.default.temporaryDirectory.appendingPathComponent("\(name).iconset")
    try? FileManager.default.removeItem(at: iconset)
    try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
    for base in [16, 32, 128, 256, 512] {
        try png(size: base, snapshot: snapshot).write(to: iconset.appendingPathComponent("icon_\(base)x\(base).png"))
        try png(size: base * 2, snapshot: snapshot).write(to: iconset.appendingPathComponent("icon_\(base)x\(base)@2x.png"))
    }
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
    process.arguments = ["-c", "icns", iconset.path, "-o", outDir.appendingPathComponent("\(name).icns").path]
    try process.run()
    process.waitUntilExit()
    guard process.terminationStatus == 0 else { fatalError("iconutil が失敗した") }
    try png(size: 1024, snapshot: snapshot).write(to: outDir.appendingPathComponent("\(name).png"))
}
print("出力: \(outDir.path)")
