// アプリのアイコンを描いて .icns にする。図柄はコードで持つ（色や形はここを直して作り直す）。
//   swift scripts/make-icon.swift [出力先のフォルダ]
// 出力: AppIcon.icns（正式版）・AppIcon-Snapshot.icns（snapshot 版。右下にオレンジの帯）・AppIcon.png（1024 px の見本）
//
// 図柄: Amon Hen（見張りの座）にちなみ、夜の丘の稜線を git のレーン（分かれて合流する 2 本）で描き、
// 頂上の座に光る点（見る目）を置く。空には淡い北斗七星と、その指極星の先に光る星（北極星の役）。macOS のアイコンの枠（1024 の中の 824 の角の丸い四角）に合わせる。
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
    let sky = CGGradient(colorsSpace: space, colors: [color(0x2A3766), color(0x0B1022)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(sky, start: CGPoint(x: 512, y: 100), end: CGPoint(x: 512, y: 924), options: [])

    // 北斗七星（物語では「ヴァラールの鎌」）。実際の位置（赤経を時、赤緯を度）から並びを作り、柄杓の先の 2 つ
    // （Merak → Dubhe）を延ばした先に頂上の星（北極星の役）が来るように回して置く。淡い点だけで、線は引かない。
    let catalog: [(ra: CGFloat, dec: CGFloat, mag: CGFloat)] = [
        (11.062, 61.75, 1.8), (11.031, 56.38, 2.4), (11.897, 53.69, 2.4), (12.257, 57.03, 3.3),
        (12.900, 55.96, 1.8), (13.399, 54.93, 2.2), (13.792, 49.31, 1.9),
    ]
    // 空を見上げた向き（東が左）の平面に写す（度、y は下向き）。
    let flat = catalog.map { CGPoint(x: -($0.ra - 12) * 15 * cos(55 * CGFloat.pi / 180), y: -$0.dec) }
    let polaris = CGPoint(x: 512, y: 300)
    let dubheAt = CGPoint(x: 330, y: 330)
    let pointer = CGPoint(x: flat[0].x - flat[1].x, y: flat[0].y - flat[1].y)
    let toPolaris = CGPoint(x: polaris.x - dubheAt.x, y: polaris.y - dubheAt.y)
    let turn = atan2(toPolaris.y, toPolaris.x) - atan2(pointer.y, pointer.x)
    let scale: CGFloat = 7.5
    for (i, star) in flat.enumerated() {
        let dx = (star.x - flat[0].x) * scale, dy = (star.y - flat[0].y) * scale
        let p = CGPoint(x: dubheAt.x + dx * cos(turn) - dy * sin(turn), y: dubheAt.y + dx * sin(turn) + dy * cos(turn))
        // 明るい星ほど少し大きく濃く（Megrez は暗い）。
        let r: CGFloat = 6.2 - catalog[i].mag
        ctx.setFillColor(color(0xFFFFFF, 0.5 - catalog[i].mag * 0.08))
        ctx.fillEllipse(in: CGRect(x: p.x - r, y: p.y - r, width: r * 2, height: r * 2))
    }

    // 頂上の後ろの光。
    let glow = CGGradient(
        colorsSpace: space, colors: [color(0xFFC94D, 0.5), color(0xE8A23A, 0.18), color(0xE8A23A, 0)] as CFArray, locations: [0, 0.45, 1])!
    ctx.drawRadialGradient(glow, startCenter: polaris, startRadius: 0,
                           endCenter: polaris, endRadius: 280, options: [])

    // 奥の丘（薄く）。
    let back = CGMutablePath()
    back.move(to: CGPoint(x: 100, y: 640))
    back.addQuadCurve(to: CGPoint(x: 330, y: 560), control: CGPoint(x: 220, y: 570))
    back.addQuadCurve(to: CGPoint(x: 560, y: 650), control: CGPoint(x: 450, y: 560))
    back.addLine(to: CGPoint(x: 560, y: 924))
    back.addLine(to: CGPoint(x: 100, y: 924))
    back.closeSubpath()
    ctx.addPath(back)
    ctx.setFillColor(color(0x26315A, 0.8))
    ctx.fillPath()

    // 手前の丘: 稜線は頂上の座（平らなところ）を挟んで左右に下りる。
    let ridge: [CGPoint] = [
        CGPoint(x: 100, y: 790), CGPoint(x: 290, y: 640), CGPoint(x: 420, y: 520), CGPoint(x: 512, y: 470),
        CGPoint(x: 610, y: 525), CGPoint(x: 760, y: 615), CGPoint(x: 924, y: 700),
    ]
    let hill = CGMutablePath()
    hill.move(to: ridge[0])
    for p in ridge.dropFirst() { hill.addLine(to: p) }
    hill.addLine(to: CGPoint(x: 924, y: 924))
    hill.addLine(to: CGPoint(x: 100, y: 924))
    hill.closeSubpath()
    ctx.saveGState()
    ctx.addPath(hill)
    ctx.clip()
    let ground = CGGradient(colorsSpace: space, colors: [color(0x34437A), color(0x111833)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(ground, start: CGPoint(x: 512, y: 470), end: CGPoint(x: 512, y: 924), options: [])
    ctx.restoreGState()

    // レーン: 稜線に沿う本流（青）と、途中で分かれて下を回り、また合流する枝（緑）。
    let main = ridge.map { CGPoint(x: $0.x, y: $0.y + 4) }
    let branchA = CGPoint(x: 290, y: 644), branchB = CGPoint(x: 760, y: 619)
    let branch = CGMutablePath()
    branch.move(to: branchA)
    branch.addCurve(to: CGPoint(x: 520, y: 690), control1: CGPoint(x: 340, y: 720), control2: CGPoint(x: 430, y: 700))
    branch.addCurve(to: branchB, control1: CGPoint(x: 620, y: 680), control2: CGPoint(x: 720, y: 690))
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    ctx.addPath(branch)
    ctx.setStrokeColor(color(0x5FD38D))
    ctx.setLineWidth(24)
    ctx.strokePath()
    let mainPath = CGMutablePath()
    mainPath.addLines(between: main)
    ctx.addPath(mainPath)
    ctx.setStrokeColor(color(0x6FB6FF))
    ctx.setLineWidth(26)
    ctx.strokePath()

    // コミットの点（地の色の縁取りで線から浮かせる）。
    func node(_ p: CGPoint, _ fill: CGColor, radius: CGFloat = 30) {
        ctx.setFillColor(color(0x141B36))
        ctx.fillEllipse(in: CGRect(x: p.x - radius - 9, y: p.y - radius - 9, width: (radius + 9) * 2, height: (radius + 9) * 2))
        ctx.setFillColor(fill)
        ctx.fillEllipse(in: CGRect(x: p.x - radius, y: p.y - radius, width: radius * 2, height: radius * 2))
    }
    node(branchA, color(0x6FB6FF))
    node(CGPoint(x: 420, y: 524), color(0x6FB6FF))
    node(CGPoint(x: 610, y: 529), color(0x6FB6FF))
    node(branchB, color(0x6FB6FF))
    node(CGPoint(x: 430, y: 697), color(0x5FD38D), radius: 26)
    node(CGPoint(x: 620, y: 681), color(0x5FD38D), radius: 26)

    // 頂上の座: 光る大きな点（見る目）と、その上の四方に光る星。
    node(CGPoint(x: 512, y: 474), color(0xFFE08A), radius: 40)
    ctx.setFillColor(color(0xFFF6D8))
    ctx.fillEllipse(in: CGRect(x: 512 - 16, y: 474 - 16, width: 32, height: 32))
    let star = CGMutablePath()
    let c = polaris, long: CGFloat = 78, short: CGFloat = 16
    for i in 0..<8 {
        // ほんのわずか（3°）傾ける。
        let angle = CGFloat(i) * .pi / 4 - .pi / 2 + 3 * .pi / 180
        let r = i % 2 == 0 ? long : short
        let p = CGPoint(x: c.x + cos(angle) * r, y: c.y + sin(angle) * r)
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
