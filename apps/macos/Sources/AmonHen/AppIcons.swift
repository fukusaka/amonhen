import AmonhenKit
import AppKit
import SwiftUI

/// どの FE でも同じ形のアイコン（`crates/icons`、`docs/icons.md`）を、AppKit の画像にする。テンプレート画像なので、
/// 色はツールバーや行が付ける（SF Symbols と同じ）。
enum AppIcons {
    /// SF Symbols の名前（と、SF Symbols にない形の `amonhen.` の名前）から、同じ意味の共通のアイコンへ。ここにない
    /// 記号（標準の部品の ▾・×・＋ など）は SF Symbols のまま。Pull request（arrow.triangle.pull）も GitHub の印のまま。
    static let bySymbol: [String: String] = [
        "arrow.triangle.branch": "branch",
        "square.and.pencil": "working-copy",
        "clock": "history",
        "clock.arrow.circlepath": "reflog",
        "network": "remote",
        "tag": "tag",
        "tray.2": "stashes",
        "tray": "stashes",
        "shippingbox": "submodule",
        "square.stack.3d.down.right": "subtree",
        "folder": "folder",
        "folder.fill": "folder",
        "line.3.horizontal.decrease": "filter",
        "line.3.horizontal.decrease.circle": "filter",
        "line.3.horizontal.decrease.circle.fill": "filter",
        "list.bullet": "list",
        "list.bullet.indent": "tree",
        "doc": "files",
        "doc.text": "files",
        "doc.plaintext": "files",
        "arrow.left.arrow.right": "range-diff",
        "arrow.up.arrow.down": "ahead-behind",
        "person.crop.circle": "account",
        "paintbrush": "appearance",
        "swatchpalette": "theme",
        "speedometer": "performance",
        "apple.terminal": "terminal",
        "terminal": "terminal",
        "questionmark.square.dashed": "unknown-file",
        "slider.horizontal.3": "display-options",
        "location": "go-to-head",
        "star": "favorite",
        "star.fill": "favorite",
        "arrow.turn.down.right": "stacked",
        "icloud.slash": "upstream-gone",
        "exclamationmark.icloud": "remote-conflict",
        "house": "main-worktree",
        "checkmark.circle": "ci-passed",
        "amonhen.clean": "clean",
        "amonhen.current-branch": "current-branch",
        "gearshape": "settings",
        "exclamationmark.triangle": "conflict",
        "exclamationmark.triangle.fill": "conflict",
        "plus.circle": "new-branch-row",
        "pencil.circle": "edited-worktree",
        "checkmark.seal": "signed",
        "checkmark.seal.fill": "signed",
        "xmark.seal": "bad-signature",
        "xmark.seal.fill": "bad-signature",
        "questionmark.circle": "missing-key",
        "checkmark.shield": "trust-unknown",
        "exclamationmark.shield.fill": "revoked-key",
        "person.crop.circle.badge.questionmark": "signer-mismatch",
        "clock.badge.exclamationmark": "expired-key",
        "exclamationmark.circle": "cannot-verify",
        "sparkles": "ai",
        "character.bubble": "translate",
        "scissors": "shallow",
        "icloud.and.arrow.down": "partial",
        "folder.badge.minus": "sparse",
        "circle.dashed": "not-checked-out",
        "pencil": "modified",
        "pencil.and.list.clipboard": "edit-mode",
        "xmark.octagon.fill": "failed",
        "stop.circle": "cancelled",
        "bolt.horizontal.circle": "disconnected",
        "checkmark.circle.fill": "ci-passed",
        "xmark.circle.fill": "ci-failed",
        "circle.fill": "ci-running",
        "circle": "ci-none",
        "slash.circle": "ci-cancelled",
        "arrow.up.left.and.arrow.down.right": "expand",
        "arrow.down.right.and.arrow.up.left": "restore",
        "rectangle.expand.vertical": "expand-all",
        "rectangle.compress.vertical": "collapse-all",
        "amonhen.back": "back",
        "rectangle.split.2x1": "side-by-side",
        "rectangle.righthalf.inset.filled": "swipe",
        "circle.lefthalf.filled": "onion-skin",
        "plus.forwardslash.minus": "changed-pixels",
        "space": "whitespace",
        "eye": "preview",
        "minus.magnifyingglass": "zoom-out",
        "plus.magnifyingglass": "zoom-in",
        "arrow.up.left.and.down.right.magnifyingglass": "zoom-fit",
        "checkerboard.rectangle": "checkerboard",
        "arrow.turn.down.left": "wrap-lines",
        "magnifyingglass": "search",
        "waveform.path.ecg": "activity",
        "arrow.clockwise": "refresh",
        "arrow.triangle.merge": "merge",
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

extension AppIcons {
    /// 大きさを決めた SF Symbols の代わり（`pointSize` の文字と並ぶ大きさ）。共通のアイコンがなければ SF Symbols を
    /// `config` で描く。
    static func image(symbol: String, pointSize: CGFloat, config: NSImage.SymbolConfiguration, label: String? = nil) -> NSImage? {
        if let name = bySymbol[symbol], let image = image(name, size: (pointSize * 1.2).rounded(), label: label) { return image }
        return NSImage(systemSymbolName: symbol, accessibilityDescription: label)?.withSymbolConfiguration(config)
    }
}

extension AppIcons {
    /// SwiftUI の `Image(systemName:)` の代わり。共通のアイコンがあればそれを（色は前景の色）。
    static func swiftUI(_ symbol: String) -> Image {
        if bySymbol[symbol] != nil, let image = image(symbol: symbol) {
            return Image(nsImage: image).renderingMode(.template)
        }
        return Image(systemName: symbol)
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
