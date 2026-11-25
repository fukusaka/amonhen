// docs/icons.md の図を作る（macOS で）。
//   swift figures.swift intent       <data.tsv> <svg dir> <out.png>
//   swift figures.swift alternatives <data.tsv> <svg dir> <out.png>
// data.tsv の 1 列目が行の種類（title・note・legend・key・head・icon・group・alt）。
import AppKit

let args = CommandLine.arguments
let mode = args[1], rows = try! String(contentsOfFile: args[2], encoding: .utf8)
    .split(separator: "\n").map { $0.split(separator: "\t", omittingEmptySubsequences: false).map(String.init) }
let dir = args[3], out = args[4]

let ink = NSColor(white: 0.13, alpha: 1), sub = NSColor(white: 0.42, alpha: 1)
let para = NSMutableParagraphStyle(); para.lineSpacing = 2
func attr(_ s: String, _ f: NSFont, _ c: NSColor = ink) -> NSAttributedString {
    NSAttributedString(string: s, attributes: [.font: f, .foregroundColor: c, .paragraphStyle: para])
}
func height(_ a: NSAttributedString, _ w: CGFloat) -> CGFloat {
    ceil(a.boundingRect(with: NSSize(width: w, height: 1e4), options: [.usesLineFragmentOrigin]).height)
}
func text(_ a: NSAttributedString, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat) {
    a.draw(with: NSRect(x: x, y: y, width: w, height: 1e4), options: [.usesLineFragmentOrigin])
}
func icon(_ name: String, _ x: CGFloat, _ y: CGFloat, _ size: CGFloat, _ c: NSColor = ink) {
    guard let img = NSImage(contentsOf: URL(fileURLWithPath: "\(dir)/\(name).svg")) else { fatalError("no \(name).svg") }
    let r = NSImage(size: NSSize(width: size, height: size), flipped: false) { rr in
        img.draw(in: rr); c.set(); rr.fill(using: .sourceAtop); return true
    }
    r.draw(in: NSRect(x: x, y: y, width: size, height: size), from: .zero, operation: .sourceOver, fraction: 1,
           respectFlipped: true, hints: nil)
}
/// 左上を原点にした 2 倍の画像に描く。
func render(_ W: CGFloat, _ H: CGFloat, _ body: () -> Void) {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(W * 2), pixelsHigh: Int(H * 2), bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: W, height: H)
    NSGraphicsContext.saveGraphicsState()
    let cg = NSGraphicsContext(bitmapImageRep: rep)!.cgContext
    cg.translateBy(x: 0, y: H); cg.scaleBy(x: 1, y: -1)
    NSGraphicsContext.current = NSGraphicsContext(cgContext: cg, flipped: true)
    NSColor(white: 0.98, alpha: 1).setFill(); NSRect(x: 0, y: 0, width: W, height: H).fill()
    body()
    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
}
func value(_ kind: String) -> [String] { rows.first { $0[0] == kind } ?? [kind, ""] }
let body = NSFont.systemFont(ofSize: 13), bold = NSFont.boldSystemFont(ofSize: 14), pad: CGFloat = 28

func rule(_ y: CGFloat, _ W: CGFloat) { NSColor(white: 0.82, alpha: 1).setFill(); NSRect(x: pad, y: y, width: W - 2 * pad, height: 1).fill() }

if mode == "intent" {
    let W: CGFloat = 900, textX: CGFloat = 250, textW = W - textX - pad, legendH: CGFloat = 115
    let items = rows.filter { $0[0] == "head" || $0[0] == "icon" }
    let hs = items.map { $0[0] == "head" ? 64 : max(72, height(attr($0[3], body), textW) + 40) }
    render(W, 70 + legendH + hs.reduce(0, +) + pad) {
        text(attr(value("title")[1], NSFont.boldSystemFont(ofSize: 20)), pad, 22, W)
        var y: CGFloat = 70
        NSColor(white: 0.93, alpha: 1).setFill()
        NSBezierPath(roundedRect: NSRect(x: pad, y: y, width: W - 2 * pad, height: legendH - 20), xRadius: 8, yRadius: 8).fill()
        text(attr(value("legend")[1], bold), pad + 16, y + 12, W)
        let lw: CGFloat = 3.75
        func stroke(_ p: NSBezierPath, dot: Bool = false) {
            ink.setStroke(); p.lineWidth = lw; p.lineCapStyle = .round; p.lineJoinStyle = .round
            if dot { p.setLineDash([0, lw * 2.2], count: 2, phase: 0) }
            p.stroke()
        }
        for (i, key) in rows.filter({ $0[0] == "key" }).enumerated() {
            let x = pad + 24 + CGFloat(i) * 210, ky = y + 46
            let p = NSBezierPath()
            switch key[1] {
            case "circle": stroke(NSBezierPath(ovalIn: NSRect(x: x + 12, y: ky + 12, width: 16, height: 16)))
            case "line": p.move(to: NSPoint(x: x + 2, y: ky + 20)); p.line(to: NSPoint(x: x + 38, y: ky + 20)); stroke(p)
            case "arrow":
                p.move(to: NSPoint(x: x + 20, y: ky + 36)); p.line(to: NSPoint(x: x + 20, y: ky + 4))
                p.move(to: NSPoint(x: x + 10, y: ky + 14)); p.line(to: NSPoint(x: x + 20, y: ky + 4)); p.line(to: NSPoint(x: x + 30, y: ky + 14))
                stroke(p)
            default: p.move(to: NSPoint(x: x + 2, y: ky + 20)); p.line(to: NSPoint(x: x + 38, y: ky + 20)); stroke(p, dot: true)
            }
            text(attr(key[2], body), x + 50, ky + 12, 160)
        }
        y += legendH
        for (r, h) in zip(items, hs) {
            if r[0] == "head" {
                rule(y + 18, W)
                text(attr(r[1], NSFont.boldSystemFont(ofSize: 16)), pad, y + 28, 300)
                text(attr(r[2], body, sub), pad + 190, y + 31, W - 2 * pad - 190)
            } else {
                icon(r[1], pad + 4, y + 12, 48); icon(r[1], pad + 68, y + 28, 20); icon(r[1], pad + 100, y + 30, 16)
                text(attr(r[2], bold), pad + 128, y + 12, 110)
                text(attr(r[3], body), textX, y + 12, textW)
            }
            y += h
        }
    }
} else {
    let W: CGFloat = 960, cols = 5, gap: CGFloat = 12
    let cw = (W - 2 * pad - gap * CGFloat(cols - 1)) / CGFloat(cols), small = NSFont.systemFont(ofSize: 12)
    var groups: [([String], [[String]])] = []
    for r in rows {
        if r[0] == "group" { groups.append((r, [])) } else if r[0] == "alt" { groups[groups.count - 1].1.append(r) }
    }
    let gh = groups.map { g in 44 + 110 + (g.1.map { height(attr($0[4], small), cw - 20) }.max() ?? 0) + 40 }
    render(W, 90 + gh.reduce(0, +)) {
        text(attr(value("title")[1], NSFont.boldSystemFont(ofSize: 20)), pad, 22, W)
        text(attr(value("note")[1], small, sub), pad, 54, W)
        var y: CGFloat = 90
        for ((g, alts), h) in zip(groups, gh) {
            rule(y + 8, W)
            text(attr(g[2], NSFont.boldSystemFont(ofSize: 16)), pad, y + 16, 400)
            for (i, a) in alts.enumerated() {
                let x = pad + CGFloat(i) * (cw + gap), top = y + 44, chosen = a[3] == "chosen"
                (chosen ? NSColor(red: 0.90, green: 0.95, blue: 1.0, alpha: 1) : NSColor.white).setFill()
                NSBezierPath(roundedRect: NSRect(x: x, y: top, width: cw, height: h - 52), xRadius: 8, yRadius: 8).fill()
                (chosen ? NSColor(red: 0.2, green: 0.5, blue: 0.9, alpha: 1) : NSColor(white: 0.86, alpha: 1)).setStroke()
                let border = NSBezierPath(roundedRect: NSRect(x: x + 0.5, y: top + 0.5, width: cw - 1, height: h - 53), xRadius: 8, yRadius: 8)
                border.lineWidth = chosen ? 2 : 1; border.stroke()
                icon(a[1], x + 12, top + 12, 48); icon(a[1], x + 72, top + 40, 20); icon(a[1], x + 100, top + 42, 16)
                NSColor(white: 0.16, alpha: 1).setFill()
                NSBezierPath(roundedRect: NSRect(x: x + 122, y: top + 36, width: 28, height: 28), xRadius: 4, yRadius: 4).fill()
                icon(a[1], x + 128, top + 42, 16, NSColor(white: 0.9, alpha: 1))
                text(attr(a[2], NSFont.boldSystemFont(ofSize: 13)), x + 10, top + 72, cw - 20)
                text(attr(a[4], small), x + 10, top + 94, cw - 20)
            }
            y += h
        }
    }
}
