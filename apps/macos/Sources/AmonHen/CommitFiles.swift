import AmonhenKit
import AppKit
import ImageIO
import UniformTypeIdentifiers

// History の右の列: 画像の比較（変更の見出しの下に置く欄）と、その時点のファイルの木と中身（すべてのファイル）。

/// 画像の大きさとバイト数（「64 × 48 px · 1.2 KB」）。
private func describe(_ data: Data?, _ image: NSImage?) -> String {
    guard let data else { return L("None") }
    let bytes = ByteCountFormatter.string(fromByteCount: Int64(data.count), countStyle: .file)
    guard let rep = image?.representations.first else { return bytes }
    let w = rep.pixelsWide > 0 ? rep.pixelsWide : Int(rep.size.width)
    let h = rep.pixelsHigh > 0 ? rep.pixelsHigh : Int(rep.size.height)
    return "\(w) × \(h) px · \(bytes)"
}

/// 見出しの下に置く、画像の前と後の比較（並べる・スワイプ・オニオンスキン・違うピクセル）。
final class ImageDiffPanel: NSView {
    enum Mode: Int { case twoUp, swipe, onion, difference }

    /// 画像として見せる拡張子: ImageIO が読める形式（avif・jxl・psd・カメラの RAW なども）と SVG。PDF などの文書は
    /// Quick Look に任せる。
    static let imageExtensions: Set<String> = {
        var set: Set<String> = ["svg"]
        for id in (CGImageSourceCopyTypeIdentifiers() as? [String]) ?? [] {
            guard let type = UTType(id), !type.conforms(to: .pdf) else { continue }
            set.formUnion(type.tags[.filenameExtension] ?? [])
        }
        set.subtract(["pdf", "ai", "eps"])
        return set
    }()
    static func isImage(_ path: String) -> Bool { imageExtensions.contains((path as NSString).pathExtension.lowercased()) }
    static func isSVG(_ path: String) -> Bool { (path as NSString).pathExtension.lowercased() == "svg" }
    /// 比べ方を変えた（次に開く画像も同じ比べ方にする）。
    var onModeChange: ((Mode) -> Void)?
    private let canvas: ImageCompareCanvas
    private let modeControl: NSSegmentedControl = {
        let modes: [(String, String)] = [
            ("rectangle.split.2x1", L("2-Up")), ("rectangle.righthalf.inset.filled", L("Swipe")),
            ("circle.lefthalf.filled", L("Onion Skin")), ("plus.forwardslash.minus", L("Difference")),
        ]
        let control = NSSegmentedControl(
            images: modes.map { AppIcons.image(symbol: $0.0, label: $0.1)! },
            trackingMode: .selectOne, target: nil, action: nil)
        for (i, mode) in modes.enumerated() { control.setToolTip(mode.1, forSegment: i) }
        return control
    }()
    private let slider = NSSlider(value: 0.5, minValue: 0, maxValue: 1, target: nil, action: nil)
    /// 拡大縮小: 縮める・倍率（押すと 100% と合わせるを切り替える）・広げる・合わせる。
    private let zoomControl: NSSegmentedControl = {
        let items: [(String, String)] = [
            ("minus.magnifyingglass", L("Zoom Out")), ("plus.magnifyingglass", L("Zoom In")),
            ("arrow.up.left.and.down.right.magnifyingglass", L("Zoom to Fit")),
        ]
        let control = NSSegmentedControl(
            images: items.map { AppIcons.image(symbol: $0.0, label: $0.1)! },
            trackingMode: .momentary, target: nil, action: nil)
        for (i, item) in items.enumerated() { control.setToolTip(item.1, forSegment: i) }
        return control
    }()
    private let zoomLabel = NSButton(title: "", target: nil, action: nil)
    /// 市松模様の明暗の切り替え（透けた画素があるときだけ出す）。
    private let checkerButton = NSButton(
        image: AppIcons.image(symbol: "checkerboard.rectangle", label: L("Darken the Checkerboard"))!,
        target: nil, action: nil)
    private let caption = NSTextField(labelWithString: "")
    private var baseCaption = ""
    /// SVG のとき「テキストで見る」を押した。
    var onShowText: (() -> Void)?

    /// `single` なら比べずに 1 枚だけ（すべてのファイルで開いた画像。`after` を出す）。
    convenience init(before: Data?, after: Data?, svg: Bool, mode: Mode, position: CGFloat, single: Bool = false) {
        let old = before.flatMap(NSImage.init(data:)), new = after.flatMap(NSImage.init(data:))
        let text = single ? describe(after, new)
            : L("Before") + ": " + describe(before, old) + "    →    " + L("After") + ": " + describe(after, new)
        self.init(
            before: old, after: new, titles: (L("Before"), L("After")), caption: text, svg: svg, mode: mode,
            position: position, single: single)
    }

    /// 描いた画像で作る（`titles` は前と後の見出し）。
    init(
        before old: NSImage?, after new: NSImage?, titles: (String, String), caption text: String, svg: Bool, mode: Mode,
        position: CGFloat, single: Bool, notes: NSAttributedString? = nil, accessory: NSView? = nil,
        footnote: [String] = []
    ) {
        canvas = ImageCompareCanvas(before: old, after: new)
        canvas.single = single
        canvas.titles = titles
        super.init(frame: NSRect(x: 0, y: 0, width: 600, height: 300))
        wantsLayer = true
        layer?.backgroundColor = NSColor.textBackgroundColor.cgColor
        modeControl.target = self
        modeControl.action = #selector(modeChanged)
        modeControl.controlSize = .small
        modeControl.selectedSegment = mode.rawValue
        slider.target = self
        slider.action = #selector(sliderChanged)
        slider.controlSize = .small
        slider.doubleValue = Double(position)
        caption.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        caption.textColor = .secondaryLabelColor
        baseCaption = text
        zoomControl.controlSize = .small
        zoomControl.target = self
        zoomControl.action = #selector(zoomChanged)
        zoomLabel.bezelStyle = .accessoryBarAction
        zoomLabel.controlSize = .small
        zoomLabel.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        zoomLabel.toolTip = L("Actual Size (100%) or Zoom to Fit")
        zoomLabel.target = self
        zoomLabel.action = #selector(toggleActualSize)
        canvas.onZoom = { [weak self] in self?.updateZoomLabel() }
        // 確かめる用（`--snapshot`）: 最初の倍率（`-AMHImageZoom 4` で 400%）。
        let zoom = UserDefaults.standard.double(forKey: "AMHImageZoom")
        if zoom > 0 { canvas.setZoom(CGFloat(zoom)) }
        // 段の左に置く部品（SVG のモバイルの、部品の切り替え）。
        var top: [NSView] = (accessory.map { [$0] } ?? []) + (single ? [] : [modeControl])
        if svg {
            let text = NSButton(
                image: AppIcons.image(symbol: "doc.plaintext", label: L("Show as Text"))!,
                target: self, action: #selector(showText))
            text.toolTip = L("Show as Text")
            text.bezelStyle = .accessoryBarAction
            text.controlSize = .small
            top.append(text)
        }
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        // 市松模様の明暗: 既定は絵の色から（確かめる用に `-AMHCheckerboard dark|light`）。開いている間はボタンが勝つ
        let tone = canvas.checkerDefault()
        switch UserDefaults.standard.string(forKey: "AMHCheckerboard") {
        case "dark": canvas.darkChecker = true
        case "light": canvas.darkChecker = false
        default: canvas.darkChecker = tone.dark
        }
        checkerButton.setButtonType(.pushOnPushOff)
        checkerButton.bezelStyle = .accessoryBarAction
        checkerButton.controlSize = .small
        checkerButton.target = self
        checkerButton.action = #selector(checkerChanged)
        checkerButton.isHidden = !tone.transparent
        updateCheckerButton()
        let bar = NSStackView(views: top + [spacer, slider, checkerButton, zoomLabel, zoomControl])
        bar.spacing = 10
        slider.widthAnchor.constraint(equalToConstant: 160).isActive = true
        let line = NSBox()
        line.boxType = .separator
        // 絵の下の説明（SVG のモバイル向けの確認で、崩れる機能の一覧）。
        let notesView = NSScrollView()
        if let notes {
            let textView = NSTextView()
            textView.isEditable = false
            textView.drawsBackground = false
            textView.textContainerInset = NSSize(width: 0, height: 4)
            textView.textStorage?.setAttributedString(notes)
            textView.isVerticallyResizable = true
            textView.autoresizingMask = .width
            notesView.documentView = textView
            notesView.hasVerticalScroller = true
            notesView.drawsBackground = false
        }
        // 絵の下に添える短い説明（VectorDrawable の引けなかった色など。Markdown の図の注記と同じ見た目）。
        let footnoteLabel = NSTextField(wrappingLabelWithString: footnote.joined(separator: "\n"))
        footnoteLabel.font = .systemFont(ofSize: 11.5)
        footnoteLabel.textColor = .secondaryLabelColor
        footnoteLabel.isSelectable = true
        let extra: [NSView] = (notes == nil ? [] : [notesView]) + (footnote.isEmpty ? [] : [footnoteLabel])
        for v in [bar, canvas, caption, line] + extra as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        let below: NSView = notes == nil ? line : notesView
        // 説明があれば、見出し（大きさ）の下に置く。
        let last: NSView = footnote.isEmpty ? caption : footnoteLabel
        if !footnote.isEmpty {
            NSLayoutConstraint.activate([
                footnoteLabel.topAnchor.constraint(equalTo: caption.bottomAnchor, constant: 4),
                footnoteLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
                footnoteLabel.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -12),
            ])
        }
        if notes != nil {
            NSLayoutConstraint.activate([
                notesView.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
                notesView.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
                notesView.heightAnchor.constraint(equalTo: heightAnchor, multiplier: 0.35),
                notesView.bottomAnchor.constraint(equalTo: line.topAnchor, constant: -6),
            ])
        }
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            bar.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            bar.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            canvas.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 8),
            canvas.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            canvas.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            caption.topAnchor.constraint(equalTo: canvas.bottomAnchor, constant: 6),
            caption.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            last.bottomAnchor.constraint(equalTo: below.topAnchor, constant: -6),
            line.leadingAnchor.constraint(equalTo: leadingAnchor),
            line.trailingAnchor.constraint(equalTo: trailingAnchor),
            line.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        canvas.onPosition = { [weak self] value in self?.slider.doubleValue = Double(value) }
        apply()
    }

    required init?(coder: NSCoder) { fatalError() }
    override var isFlipped: Bool { true }

    @objc private func modeChanged() {
        apply()
        onModeChange?(Mode(rawValue: modeControl.selectedSegment) ?? .twoUp)
    }
    @objc private func sliderChanged() {
        canvas.position = CGFloat(slider.doubleValue)
    }
    @objc private func showText() { onShowText?() }

    @objc private func checkerChanged() {
        canvas.darkChecker = checkerButton.state == .on
        updateCheckerButton()
    }

    private func updateCheckerButton() {
        checkerButton.state = canvas.darkChecker ? .on : .off
        let title = canvas.darkChecker ? L("Lighten the Checkerboard") : L("Darken the Checkerboard")
        checkerButton.toolTip = title
        checkerButton.setAccessibilityLabel(title)
    }

    @objc private func zoomChanged() {
        switch zoomControl.selectedSegment {
        case 0: canvas.zoom(by: 1 / 1.5)
        case 1: canvas.zoom(by: 1.5)
        default: canvas.zoomToFit()
        }
    }

    @objc private func toggleActualSize() {
        if canvas.zoom == 1 { canvas.zoomToFit() } else { canvas.setZoom(1) }
    }

    private func updateZoomLabel() {
        zoomLabel.title = canvas.zoom == nil ? L("Fit") : "\(Int((canvas.scale * 100).rounded()))%"
    }

    override func layout() {
        super.layout()
        updateZoomLabel()
    }

    private func apply() {
        let mode = Mode(rawValue: modeControl.selectedSegment) ?? .twoUp
        canvas.mode = mode
        canvas.position = CGFloat(slider.doubleValue)
        slider.isHidden = canvas.single || mode == .twoUp || mode == .difference
        if mode == .difference {
            caption.stringValue = canvas.differenceSummary
        } else {
            caption.stringValue = baseCaption
        }
    }
}

final class ImageCompareCanvas: NSView {
    private let before: NSImage?
    private let after: NSImage?
    var mode: ImageDiffPanel.Mode = .twoUp { didSet { needsDisplay = true } }
    var position: CGFloat = 0.5 { didSet { needsDisplay = true } }
    var onPosition: ((CGFloat) -> Void)?
    /// 1 枚だけ見せる（比べない）。
    var single = false
    /// 市松模様を暗くする（既定は絵の色から。`checkerDefault`）。
    var darkChecker = false { didSet { needsDisplay = true } }
    /// 前と後の見出し（SVG をブラウザと iOS で比べるときは、その名前）。
    var titles = (L("Before"), L("After"))
    /// 倍率（画像の 1 ピクセルを何ポイントで描くか）。nil なら枠に合わせる。
    private(set) var zoom: CGFloat?
    /// 拡大したときの、枠の中心からのずらし（ポイント）。
    private var pan = CGPoint.zero
    /// 倍率が変わった（帯の倍率の表示）。
    var onZoom: (() -> Void)?
    private var dragStart: (point: NSPoint, pan: CGPoint)?
    private lazy var difference: (image: CGImage?, changed: Int, total: Int) = makeDifference()

    init(before: NSImage?, after: NSImage?) {
        self.before = before
        self.after = after
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError() }
    override var isFlipped: Bool { true }

    /// 市松模様の既定（絵の色から。比べるときは両方を合わせて 1 つ）と、透けた画素があるか。2 枚の絵をそれぞれ
    /// 256 × 256 に伸ばして並べた画素で数える。
    func checkerDefault() -> (dark: Bool, transparent: Bool) {
        let images = [before, after].compactMap { $0 }
        guard !images.isEmpty else { return (false, false) }
        let cell = 256
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: cell * images.count, pixelsHigh: cell, bitsPerSample: 8,
            samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0,
            bitsPerPixel: 0)
        else { return (false, false) }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        for (i, image) in images.enumerated() {
            image.draw(in: NSRect(x: i * cell, y: 0, width: cell, height: cell))
        }
        NSGraphicsContext.restoreGraphicsState()
        return checkerTone(pixels: rep.bitmapData, width: rep.pixelsWide, height: rep.pixelsHigh, stride: rep.bytesPerRow)
    }

    var sizeSummary: String {
        func size(_ image: NSImage?) -> String {
            guard let rep = image?.representations.first else { return L("None") }
            return "\(rep.pixelsWide > 0 ? rep.pixelsWide : Int(rep.size.width)) × \(rep.pixelsHigh > 0 ? rep.pixelsHigh : Int(rep.size.height)) px"
        }
        return L("Before") + ": " + size(before) + "    →    " + L("After") + ": " + size(after)
    }

    var differenceSummary: String {
        let d = difference
        guard d.total > 0 else { return L("Nothing to compare") }
        return String(format: L("%ld of %ld pixels differ (%.1f%%)"), d.changed, d.total, Double(d.changed) * 100 / Double(d.total))
    }

    private func pixelSize(_ image: NSImage?) -> NSSize {
        guard let image, let rep = image.representations.first else { return .zero }
        return rep.pixelsWide > 0 ? NSSize(width: rep.pixelsWide, height: rep.pixelsHigh) : image.size
    }

    /// 枠に合わせるときの倍率（小さな絵は整数倍まで拡げ、ぼかさない）。
    private func fitScale(_ size: NSSize, in box: NSRect) -> CGFloat {
        guard size.width > 0, size.height > 0 else { return 1 }
        var scale = min(box.width / size.width, box.height / size.height)
        if scale > 1 { scale = max(1, floor(min(scale, 8))) }
        return scale
    }

    /// `size` を `box` に置く: 倍率を決めていなければ収め、決めていればその倍率で中心からずらして。
    private func fit(_ size: NSSize, in box: NSRect) -> NSRect {
        guard size.width > 0, size.height > 0 else { return .zero }
        let scale = zoom ?? fitScale(size, in: box)
        let w = size.width * scale, h = size.height * scale
        let (dx, dy) = zoom == nil ? (0, 0) : (pan.x, pan.y)
        return NSRect(x: box.midX - w / 2 + dx, y: box.midY - h / 2 + dy, width: w, height: h).integral
    }

    private var commonSize: NSSize {
        NSSize(
            width: max(pixelSize(before).width, pixelSize(after).width),
            height: max(pixelSize(before).height, pixelSize(after).height))
    }

    /// 今の倍率（合わせているならその倍率）。
    var scale: CGFloat {
        if let zoom { return zoom }
        let box = single || mode != .twoUp ? bounds.insetBy(dx: 4, dy: 4)
            : NSRect(x: 0, y: 16, width: (bounds.width - 16) / 2, height: bounds.height - 16).insetBy(dx: 4, dy: 4)
        return fitScale(commonSize, in: box)
    }

    func setZoom(_ value: CGFloat?) {
        zoom = value.map { min(max($0, 0.05), 64) }
        if zoom == nil { pan = .zero }
        needsDisplay = true
        onZoom?()
    }

    func zoom(by factor: CGFloat) { setZoom(scale * factor) }
    func zoomToFit() { setZoom(nil) }

    override func magnify(with event: NSEvent) { zoom(by: 1 + event.magnification) }

    override func scrollWheel(with event: NSEvent) {
        if event.modifierFlags.contains(.command) {
            zoom(by: event.scrollingDeltaY > 0 ? 1.1 : 1 / 1.1)
        } else if zoom != nil {
            // 拡大している間は、絵を動かす（合わせているときは差分をスクロールする）。
            pan.x += event.scrollingDeltaX
            pan.y += event.scrollingDeltaY
            needsDisplay = true
        } else {
            super.scrollWheel(with: event)
        }
    }

    private func checker(_ rect: NSRect) {
        let dark = darkChecker
        NSColor(white: dark ? 0.16 : 1, alpha: 1).setFill()
        rect.fill()
        NSColor(white: dark ? 0.24 : 0.86, alpha: 1).setFill()
        let s: CGFloat = 8
        var y = rect.minY
        var row = 0
        while y < rect.maxY {
            var x = rect.minX + (row % 2 == 0 ? 0 : s)
            while x < rect.maxX {
                NSRect(x: x, y: y, width: min(s, rect.maxX - x), height: min(s, rect.maxY - y)).fill()
                x += 2 * s
            }
            y += s
            row += 1
        }
    }

    private func draw(_ image: NSImage?, in rect: NSRect, alpha: CGFloat = 1) {
        guard let image else { return }
        // ベクター（SVG）は描く大きさで描き直す（ビットマップに焼いたものを伸ばすとぼやける）。
        if let rep = image.representations.first, !(rep is NSBitmapImageRep) {
            NSGraphicsContext.current?.imageInterpolation = .high
            rep.draw(in: rect, from: .zero, operation: .sourceOver, fraction: alpha, respectFlipped: true, hints: nil)
            return
        }
        NSGraphicsContext.current?.imageInterpolation = rect.width >= pixelSize(image).width * 2 ? .none : .high
        image.draw(in: rect, from: .zero, operation: .sourceOver, fraction: alpha, respectFlipped: true, hints: nil)
    }

    private func label(_ text: String, at point: NSPoint) {
        (text as NSString).draw(at: point, withAttributes: [
            .font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.secondaryLabelColor,
        ])
    }

    override func draw(_ dirtyRect: NSRect) {
        let area = bounds
        let common = commonSize
        NSBezierPath(rect: area).addClip()
        if single {
            let rect = fit(pixelSize(after), in: area.insetBy(dx: 4, dy: 4))
            checker(rect)
            draw(after, in: rect)
            return
        }
        switch mode {
        case .twoUp:
            let half = (area.width - 16) / 2
            for (i, (image, title, color)) in [(before, titles.0, NSColor.systemRed), (after, titles.1, NSColor.systemGreen)].enumerated() {
                let box = NSRect(x: area.minX + CGFloat(i) * (half + 16), y: area.minY + 16, width: half, height: area.height - 16)
                label(title, at: NSPoint(x: box.minX, y: area.minY))
                if let image {
                    let rect = fit(pixelSize(image), in: box.insetBy(dx: 4, dy: 4))
                    NSGraphicsContext.saveGraphicsState()
                    NSBezierPath(rect: box).addClip()
                    defer { NSGraphicsContext.restoreGraphicsState() }
                    checker(rect)
                    draw(image, in: rect)
                    color.withAlphaComponent(0.8).setStroke()
                    let frame = NSBezierPath(rect: rect.insetBy(dx: -1.5, dy: -1.5))
                    frame.lineWidth = 2
                    frame.stroke()
                } else {
                    NSColor.tertiaryLabelColor.setStroke()
                    let dash = NSBezierPath(rect: box.insetBy(dx: 30, dy: 20))
                    dash.setLineDash([4, 4], count: 2, phase: 0)
                    dash.stroke()
                    label(L("None"), at: NSPoint(x: box.midX - 14, y: box.midY - 7))
                }
            }
        case .swipe, .onion:
            let rect = fit(common, in: area.insetBy(dx: 4, dy: 4))
            checker(rect)
            if mode == .onion {
                draw(before, in: fitted(before, rect))
                draw(after, in: fitted(after, rect), alpha: position)
                label(titles.0, at: NSPoint(x: rect.minX - 56, y: rect.minY))
                label(titles.1, at: NSPoint(x: rect.maxX + 8, y: rect.minY))
            } else {
                draw(after, in: fitted(after, rect))
                let split = rect.minX + rect.width * position
                NSGraphicsContext.saveGraphicsState()
                NSRect(x: rect.minX, y: rect.minY, width: split - rect.minX, height: rect.height).clip()
                checker(rect)
                draw(before, in: fitted(before, rect))
                NSGraphicsContext.restoreGraphicsState()
                NSColor.controlAccentColor.setFill()
                NSRect(x: split - 1, y: rect.minY - 4, width: 2, height: rect.height + 8).fill()
                NSBezierPath(ovalIn: NSRect(x: split - 7, y: rect.midY - 7, width: 14, height: 14)).fill()
                label(titles.0, at: NSPoint(x: rect.minX - 56, y: rect.minY))
                label(titles.1, at: NSPoint(x: rect.maxX + 8, y: rect.minY))
            }
        case .difference:
            let rect = fit(common, in: area.insetBy(dx: 4, dy: 4))
            NSColor.black.setFill()
            rect.fill()
            if let image = difference.image, let context = NSGraphicsContext.current?.cgContext {
                context.saveGState()
                context.interpolationQuality = .none
                context.translateBy(x: rect.minX, y: rect.maxY)
                context.scaleBy(x: 1, y: -1)
                context.draw(image, in: CGRect(origin: .zero, size: rect.size))
                context.restoreGState()
            }
        }
    }

    /// 共通の枠の中の、その画像の位置（左上にそろえる）。
    private func fitted(_ image: NSImage?, _ rect: NSRect) -> NSRect {
        let common = NSSize(
            width: max(pixelSize(before).width, pixelSize(after).width),
            height: max(pixelSize(before).height, pixelSize(after).height))
        let size = pixelSize(image)
        guard common.width > 0 else { return rect }
        let scale = rect.width / common.width
        return NSRect(x: rect.minX, y: rect.minY, width: size.width * scale, height: size.height * scale)
    }

    private func pixels(_ image: NSImage?, _ size: NSSize) -> [UInt8] {
        let w = Int(size.width), h = Int(size.height)
        var buffer = [UInt8](repeating: 0, count: w * h * 4)
        guard let image, let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return buffer }
        let own = pixelSize(image)
        buffer.withUnsafeMutableBytes { raw in
            guard let context = CGContext(
                data: raw.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return }
            context.draw(cg, in: CGRect(x: 0, y: CGFloat(h) - own.height, width: own.width, height: own.height))
        }
        return buffer
    }

    /// 違うピクセルを赤紫で、同じところは後の画像を薄く。
    private func makeDifference() -> (CGImage?, Int, Int) {
        let size = NSSize(
            width: max(pixelSize(before).width, pixelSize(after).width),
            height: max(pixelSize(before).height, pixelSize(after).height))
        let w = Int(size.width), h = Int(size.height)
        guard w > 0, h > 0 else { return (nil, 0, 0) }
        let a = pixels(before, size), b = pixels(after, size)
        var out = [UInt8](repeating: 0, count: w * h * 4)
        var changed = 0
        for i in stride(from: 0, to: out.count, by: 4) {
            let differs = a[i] != b[i] || a[i + 1] != b[i + 1] || a[i + 2] != b[i + 2] || a[i + 3] != b[i + 3]
            if differs {
                changed += 1
                out[i] = 255; out[i + 1] = 0; out[i + 2] = 200; out[i + 3] = 255
            } else {
                let gray = UInt8((Int(b[i]) + Int(b[i + 1]) + Int(b[i + 2])) / 3 / 3)
                out[i] = gray; out[i + 1] = gray; out[i + 2] = gray; out[i + 3] = 255
            }
        }
        let provider = CGDataProvider(data: Data(out) as CFData)
        let image = provider.flatMap {
            CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                    space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                    provider: $0, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        }
        return (image, changed, w * h)
    }

    override func mouseDown(with event: NSEvent) {
        // 拡大している間は、並べる・違うピクセル・1 枚ではドラッグで動かす（スワイプ・オニオンスキンは境を動かすので
        // ⌥ を押しながら）。
        if zoom != nil, single || mode == .twoUp || mode == .difference || event.modifierFlags.contains(.option) {
            dragStart = (convert(event.locationInWindow, from: nil), pan)
            return
        }
        dragStart = nil
        track(event)
    }
    override func mouseDragged(with event: NSEvent) {
        if let start = dragStart {
            let p = convert(event.locationInWindow, from: nil)
            pan = CGPoint(x: start.pan.x + p.x - start.point.x, y: start.pan.y + p.y - start.point.y)
            needsDisplay = true
            return
        }
        track(event)
    }
    override func mouseUp(with event: NSEvent) { dragStart = nil }
    private func track(_ event: NSEvent) {
        guard !single, mode == .swipe || mode == .onion else { return }
        let common = NSSize(
            width: max(pixelSize(before).width, pixelSize(after).width),
            height: max(pixelSize(before).height, pixelSize(after).height))
        let rect = fit(common, in: bounds.insetBy(dx: 4, dy: 4))
        guard rect.width > 0 else { return }
        let p = convert(event.locationInWindow, from: nil)
        position = max(0, min(1, (p.x - rect.minX) / rect.width))
        onPosition?(position)
    }
}

/// その時点のリポジトリ全体の木と、開いたファイルの中身（右の列の幅いっぱい）。
/// その時点のリポジトリ全体の木（ツリーかリスト）と、開いたファイルの中身（右の列の幅いっぱい）。フォルダの中は
/// 開いたときに読む。中身は差分と同じ描き方で、このコミットで足した行を緑、変えた行を青、消したところを印で出す。
final class CommitFilesBrowser: NSView, NSOutlineViewDataSource, NSOutlineViewDelegate {
    final class Node {
        let name: String
        let path: String
        let isDirectory: Bool
        let oid: String
        /// 読んだ子（フォルダで、まだ読んでいなければ nil）。
        var children: [Node]?
        var kind: ChangeKind?
        var containsChanges = false
        init(name: String, path: String, isDirectory: Bool, oid: String) {
            self.name = name
            self.path = path
            self.isDirectory = isDirectory
            self.oid = oid
        }
    }

    private let repository: Repository
    private let commit: String
    private let commitID: Data
    private let changes: CommitChanges?
    private let root: Node
    private let outline = NSOutlineView()
    private let treeScroll = NSScrollView()
    private let content: DiffViewController
    /// 開いた画像（拡大縮小できる 1 枚の表示）。
    private var imagePanel: ImageDiffPanel?
    /// SVG の見せ方: 絵（CoreSVG で描く）・モバイル（部品ごとの崩れた絵と崩れる機能の一覧）・テキスト。
    private enum SVGView: Int { case picture, compare, text }
    /// 確かめる用（`--snapshot`）に `-AMHSVGView picture|compare|text` で最初の見せ方を変えられる。
    private var svgView: SVGView = switch UserDefaults.standard.string(forKey: "AMHSVGView") {
    case "compare": .compare
    case "text": .text
    default: .picture
    }
    private let svgSwitch = NSSegmentedControl(
        labels: [L("Picture"), L("Mobile"), L("Text")], trackingMode: .selectOne, target: nil, action: nil)
    /// モバイルで見る部品（確かめる用に `-AMHMobileRenderer android|ios|flutter|reactNative`）。
    private var mobileRenderer: MobileRenderer = MobileRenderer.allCases.first {
        $0.key == UserDefaults.standard.string(forKey: "AMHMobileRenderer")
    } ?? .android
    /// Markdown の見せ方（整形・テキスト）。
    private var markdownText = UserDefaults.standard.string(forKey: "AMHMarkdownView") == "text"
    private let markdownSwitch = NSSegmentedControl(
        labels: [L("Preview"), L("Text")], trackingMode: .selectOne, target: nil, action: nil)
    private var markdownView: MarkdownView?
    /// 整形した Markdown が出たら探す文字（`--find` の確認用。読むのを待つ）。
    private var pendingFind: String?
    /// Android の VectorDrawable の見せ方（絵・テキスト）。確かめる用に `-AMHDrawableView text`。
    private var drawableText = UserDefaults.standard.string(forKey: "AMHDrawableView") == "text"
    private let drawableSwitch = NSSegmentedControl(
        labels: [L("Picture"), L("Text")], trackingMode: .selectOne, target: nil, action: nil)
    /// Markdown の #123・@user・コミットの ID をリンクにする先（`https://github.com/owner/repo`）。
    var gitHub: String?
    private let rendererSwitch = NSSegmentedControl(
        labels: ["Android", "iOS", "Flutter", "React Native"], trackingMode: .selectOne, target: nil, action: nil)
    private let pathBar = NSStackView()
    private let pathLabel = NSTextField(labelWithString: "")
    private let changeButton = NSButton()
    private let quickLookButton = NSButton()
    /// バイナリのファイルの案内と、Quick Look で開くボタン（アプリで描けないもの）。
    private let binaryBox = NSStackView()
    private var openPath: String?
    private var request = 0
    /// 変更ファイル（パス → 番号）と、変更を含むフォルダ。
    private var changedFiles: [String: Int] = [:]
    private var changedFolders: Set<String> = []
    /// このコミットで消したファイル（木にはないので、親のフォルダに足して赤で出す）。
    private var deletedFiles: [String: [String]] = [:]
    /// 「この変更を見る」: 変更の表示に戻り、そのファイルを開く（ファイルの番号）。
    var onShowChange: ((Int) -> Void)?
    /// フォルダを開いた・畳んだ（帯のフォルダのボタンの向き）。
    var onFoldersChange: (() -> Void)?
    /// リストで見せる（変更ファイルを含め、読んだフォルダのファイルをパスで並べる）。
    var flat = false {
        didSet {
            guard flat != oldValue else { return }
            if flat { loadAll() }
            outline.reloadData()
            if !flat { expandChanged(root) }
        }
    }

    init(repository: Repository, commit: String, commitID: Data, changes: CommitChanges?, renderer: TextRenderer) {
        self.repository = repository
        self.commit = commit
        self.commitID = commitID
        self.changes = changes
        root = Node(name: "", path: "", isDirectory: true, oid: "")
        content = DiffViewController(renderer: renderer)
        super.init(frame: NSRect(x: 0, y: 0, width: 600, height: 400))
        // gantt の時間帯（設定・OS）と数式の設定を変えたら、整形した Markdown を読み直す（日付の位置・式の大きさが変わる）。
        for name in [Notification.Name.ganttTimeZoneChanged, .NSSystemTimeZoneDidChange, .mathSettingsChanged] {
            NotificationCenter.default.addObserver(self, selector: #selector(timeZoneChanged), name: name, object: nil)
        }
        for (i, item) in (changes?.items ?? []).enumerated() {
            changedFiles[item.path] = i
            var folder = (item.path as NSString).deletingLastPathComponent
            while !folder.isEmpty {
                changedFolders.insert(folder)
                folder = (folder as NSString).deletingLastPathComponent
            }
            changedFolders.insert("")
            if item.kind == .deleted {
                deletedFiles[(item.path as NSString).deletingLastPathComponent, default: []].append(item.path)
            }
        }
        let column = NSTableColumn(identifier: .init("name"))
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.headerView = nil
        outline.style = .sourceList
        outline.dataSource = self
        outline.delegate = self
        outline.target = self
        outline.action = #selector(openSelected)
        treeScroll.documentView = outline
        treeScroll.hasVerticalScroller = true
        treeScroll.drawsBackground = false
        content.fileMode = true
        let back = NSButton(
            image: AppIcons.image(symbol: "amonhen.back", label: L("Back"))!, target: self,
            action: #selector(backToTree))
        back.bezelStyle = .accessoryBarAction
        back.controlSize = .small
        back.toolTip = L("Back to the files")
        changeButton.title = L("Show This Change")
        changeButton.bezelStyle = .accessoryBarAction
        changeButton.controlSize = .small
        changeButton.target = self
        changeButton.action = #selector(showChange)
        quickLookButton.image = AppIcons.image(symbol: "eye", label: L("Quick Look"))
        quickLookButton.bezelStyle = .accessoryBarAction
        quickLookButton.controlSize = .small
        quickLookButton.toolTip = L("Open this file at the commit in Quick Look")
        quickLookButton.target = self
        quickLookButton.action = #selector(openInQuickLook)
        let binaryLabel = NSTextField(labelWithString: L("Binary file"))
        binaryLabel.textColor = .secondaryLabelColor
        let binaryButton = NSButton(title: L("Open in Quick Look"), target: self, action: #selector(openInQuickLook))
        binaryButton.bezelStyle = .push
        binaryBox.orientation = .vertical
        binaryBox.spacing = 10
        binaryBox.alignment = .centerX
        for v in [binaryLabel, binaryButton] { binaryBox.addArrangedSubview(v) }
        binaryBox.isHidden = true
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        pathLabel.lineBreakMode = .byTruncatingHead
        pathLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        svgSwitch.controlSize = .small
        svgSwitch.selectedSegment = 0
        svgSwitch.setToolTip(L("Draw the SVG"), forSegment: 0)
        svgSwitch.setToolTip(
            L("Check for mobile: list what breaks on Android, iOS, Flutter and React Native"), forSegment: 1)
        svgSwitch.setToolTip(L("Show the SVG source"), forSegment: 2)
        svgSwitch.target = self
        svgSwitch.action = #selector(svgViewChanged)
        svgSwitch.isHidden = true
        rendererSwitch.controlSize = .small
        for (i, renderer) in MobileRenderer.allCases.enumerated() {
            rendererSwitch.setToolTip(String(format: L("How the SVG looks on %@"), renderer.name), forSegment: i)
        }
        rendererSwitch.target = self
        rendererSwitch.action = #selector(rendererChanged)
        markdownSwitch.controlSize = .small
        markdownSwitch.setToolTip(L("Show the Markdown formatted"), forSegment: 0)
        markdownSwitch.setToolTip(L("Show the Markdown source"), forSegment: 1)
        markdownSwitch.target = self
        markdownSwitch.action = #selector(markdownViewChanged)
        markdownSwitch.isHidden = true
        drawableSwitch.controlSize = .small
        drawableSwitch.setToolTip(L("Draw the VectorDrawable"), forSegment: 0)
        drawableSwitch.setToolTip(L("Show the XML source"), forSegment: 1)
        drawableSwitch.target = self
        drawableSwitch.action = #selector(drawableViewChanged)
        drawableSwitch.isHidden = true
        for v in [back, pathLabel, spacer, svgSwitch, markdownSwitch, drawableSwitch, quickLookButton, changeButton] {
            pathBar.addArrangedSubview(v)
        }
        pathBar.edgeInsets = NSEdgeInsets(top: 0, left: 6, bottom: 0, right: 10)
        pathBar.spacing = 6
        for v in [treeScroll, content.view, binaryBox, pathBar] as [NSView] { addSubview(v) }
        load(root)
        outline.reloadData()
        expandChanged(root)
        backToTree()
    }

    required init?(coder: NSCoder) { fatalError() }

    /// 差分と同じく、Metal で描いた中身の絵（`--snapshot` 用）。
    func overlay() -> (CGImage, NSView)? {
        content.view.isHidden || openPath == nil ? nil : content.overlay()
    }

    /// フォルダの中を読む（まだなら）。このコミットで消したファイルも足す。
    private func load(_ node: Node) {
        guard node.isDirectory, node.children == nil else { return }
        let entries = (try? repository.treeEntries(commit: commit, path: node.path)) ?? []
        var children = entries.map { entry -> Node in
            let path = node.path.isEmpty ? entry.name : node.path + "/" + entry.name
            let child = Node(name: entry.name, path: path, isDirectory: entry.isFolder, oid: entry.oid)
            if let index = changedFiles[path] { child.kind = changes?.items[index].kind }
            child.containsChanges = child.kind != nil || (entry.isFolder && changedFolders.contains(path))
            return child
        }
        for path in deletedFiles[node.path] ?? [] {
            let child = Node(name: (path as NSString).lastPathComponent, path: path, isDirectory: false, oid: "")
            child.kind = .deleted
            child.containsChanges = true
            children.append(child)
        }
        // 消したフォルダ（このコミットで中のファイルをすべて消した）も、変更を含むフォルダとして足す。
        let names = Set(children.map(\.name))
        for folder in changedFolders where !folder.isEmpty && (folder as NSString).deletingLastPathComponent == node.path {
            let name = (folder as NSString).lastPathComponent
            if !names.contains(name) {
                let child = Node(name: name, path: folder, isDirectory: true, oid: "")
                child.children = nil
                child.containsChanges = true
                children.append(child)
            }
        }
        children.sort { ($0.isDirectory ? 0 : 1, $0.name.lowercased()) < ($1.isDirectory ? 0 : 1, $1.name.lowercased()) }
        node.children = children
    }

    /// リストのために、すべてのフォルダを読む。
    private func loadAll() {
        var stack = [root]
        while let node = stack.popLast() {
            load(node)
            stack.append(contentsOf: (node.children ?? []).filter(\.isDirectory))
        }
    }

    private var flatCache: [Node]?
    private var flatFiles: [Node] {
        if let flatCache { return flatCache }
        var out: [Node] = []
        var stack = [root]
        while let node = stack.popLast() {
            for child in node.children ?? [] { if child.isDirectory { stack.append(child) } else { out.append(child) } }
        }
        let sorted = out.sorted { $0.path.lowercased() < $1.path.lowercased() }
        flatCache = sorted
        return sorted
    }

    /// 開いているフォルダがあるか（フォルダのボタンの向き）。
    var anyFolderExpanded: Bool {
        !flat && (root.children ?? []).contains { $0.isDirectory && outline.isItemExpanded($0) }
    }

    /// すべてのフォルダを開く・畳む（開くときは、中を読む）。
    func setAllFolders(expanded: Bool) {
        if expanded {
            loadAll()
            outline.reloadData()
            outline.expandItem(nil, expandChildren: true)
        } else {
            outline.collapseItem(nil, collapseChildren: true)
        }
    }

    private func expandChanged(_ node: Node) {
        for child in node.children ?? [] where child.isDirectory && child.containsChanges {
            load(child)
            outline.reloadItem(child, reloadChildren: true)
            outline.expandItem(child)
            expandChanged(child)
        }
    }

    override func layout() {
        super.layout()
        let barHeight: CGFloat = openPath == nil ? 0 : 28
        pathBar.frame = NSRect(x: 0, y: bounds.height - barHeight, width: bounds.width, height: barHeight)
        let rest = NSRect(x: 0, y: 0, width: bounds.width, height: bounds.height - barHeight)
        treeScroll.frame = rest
        content.view.frame = rest
        imagePanel?.frame = rest
        markdownView?.frame = rest
        let box = binaryBox.fittingSize
        binaryBox.frame = NSRect(x: rest.midX - box.width / 2, y: rest.midY - box.height / 2, width: box.width, height: box.height)
        outline.sizeLastColumnToFit()
    }

    @objc private func backToTree() {
        openPath = nil
        treeScroll.isHidden = false
        content.view.isHidden = true
        removeImage()
        binaryBox.isHidden = true
        pathBar.isHidden = true
        needsLayout = true
    }

    /// モバイル向けの描画部品ごとの、崩れる機能の一覧。
    /// 上に、見ている部品の取り込みが読めなかった理由・警告（`preview`）を置く。
    private static func mobileNotes(_ issues: [SVGIssue], preview: MobilePreview) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let body: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.labelColor]
        let head: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 12), .foregroundColor: NSColor.labelColor]
        let mono: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular), .foregroundColor: NSColor.secondaryLabelColor,
        ]
        if let error = preview.error {
            out.append(NSAttributedString(
                string: error + "\n", attributes: [.font: NSFont.boldSystemFont(ofSize: 12), .foregroundColor: NSColor.systemRed]))
        }
        if !preview.messages.isEmpty {
            out.append(NSAttributedString(string: String(format: L("What %@ reports"), preview.title) + "\n", attributes: head))
            for message in preview.messages {
                out.append(NSAttributedString(string: "    " + message + "\n", attributes: mono))
            }
        }
        if out.length > 0 { out.append(NSAttributedString(string: "\n", attributes: body)) }
        guard !issues.isEmpty else {
            out.append(NSAttributedString(
                string: L("Nothing known to break on Android VectorDrawable, iOS (CoreSVG), flutter_svg or react-native-svg."),
                attributes: body))
            return out
        }
        let targets: [(String, String)] = [
            ("android", "Android VectorDrawable"), ("ios", "iOS (CoreSVG)"), ("flutter", "flutter_svg"),
            ("reactNative", "react-native-svg"),
        ]
        for (key, name) in targets {
            let own = issues.filter { $0.targets.contains(key) }
            out.append(NSAttributedString(string: "\(name)\n", attributes: head))
            if own.isEmpty {
                out.append(NSAttributedString(string: "    " + L("Nothing known to break") + "\n", attributes: body))
            }
            for issue in own {
                out.append(NSAttributedString(string: "    " + issue.feature, attributes: mono))
                if issue.count > 1 { out.append(NSAttributedString(string: " ×\(issue.count)", attributes: mono)) }
                out.append(NSAttributedString(string: "  " + issue.message + "\n", attributes: body))
            }
        }
        return out
    }

    @objc private func timeZoneChanged() {
        if markdownView != nil, let openPath { open(path: openPath, line: nil) }
    }

    @objc private func markdownViewChanged() {
        markdownText = markdownSwitch.selectedSegment == 1
        if let openPath { open(path: openPath, line: nil) }
    }

    /// Markdown の中の画像（Markdown のファイル `from` からの相対パス）を、このコミットの中から読む。外の URL は読まない。
    private func image(_ link: String, from: String) -> NSImage? {
        guard !link.contains("://"), let path = MarkdownView.resolve(link, from: from) else { return nil }
        return fileReader()(path).flatMap(NSImage.init(data:))
    }

    /// このコミットのファイルの中身を、リポジトリの根からのパスで読む（変えたファイルは変更後）。どのスレッドからでも。
    private func fileReader() -> @Sendable (String) -> Data? {
        let changed = changedFiles, source = changes, store = repository, commit = self.commit
        return { path in
            if let index = changed[path] { return source?.blob(at: index, after: true) }
            let folder = (path as NSString).deletingLastPathComponent, name = (path as NSString).lastPathComponent
            guard let entry = (try? store.treeEntries(commit: commit, path: folder))?.first(where: { $0.name == name }) else {
                return nil
            }
            return store.blob(oid: entry.oid)
        }
    }

    @objc private func rendererChanged() {
        mobileRenderer = MobileRenderer(rawValue: UInt8(clamping: rendererSwitch.selectedSegment)) ?? .android
        if let openPath { open(path: openPath, line: nil) }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        // VectorDrawable の絵は引けない色を外観の文字の色で描くので、外観が変わったら描き直す
        if imagePanel != nil, !drawableSwitch.isHidden, !drawableText, let openPath { open(path: openPath, line: nil) }
    }

    @objc private func drawableViewChanged() {
        drawableText = drawableSwitch.selectedSegment == 1
        if let openPath { open(path: openPath, line: nil) }
    }

    @objc private func svgViewChanged() {
        svgView = SVGView(rawValue: svgSwitch.selectedSegment) ?? .picture
        if let openPath { open(path: openPath, line: nil) }
    }

    private func removeImage() {
        imagePanel?.removeFromSuperview()
        imagePanel = nil
        markdownView?.removeFromSuperview()
        markdownView = nil
    }

    /// 開いているファイルのその時点の中身（変えたファイルは変更後、消したファイルは変更前）。裏で読む。
    private func contents(of path: String) async -> Data? {
        let index = changedFiles[path]
        let deleted = index.map { self.changes?.items[$0].kind == .deleted } ?? false
        let source = changes, store = repository, oid = node(at: path)?.oid
        return await Task.detached(priority: .userInitiated) { () -> Data? in
            if let index { return source?.blob(at: index, after: !deleted) }
            return oid.flatMap { store.blob(oid: $0) }
        }.value
    }

    /// 開いているファイルを Quick Look の窓で見る。
    @objc private func openInQuickLook() {
        guard let path = openPath else { return }
        Task {
            guard let data = await contents(of: path), path == openPath else { return NSSound.beep() }
            QuickLook.shared.show(data, name: (path as NSString).lastPathComponent)
        }
    }

    @objc private func openSelected() {
        guard let node = outline.item(atRow: outline.selectedRow) as? Node else { return }
        if node.isDirectory {
            if outline.isItemExpanded(node) { outline.collapseItem(node) } else { outline.expandItem(node) }
        } else {
            open(path: node.path, line: nil)
        }
    }

    @objc private func showChange() {
        if let openPath, let file = changedFiles[openPath] { onShowChange?(file) }
    }

    /// 開いているファイルを探す（`--find` の確認用）。整形した Markdown がまだ読めていなければ、出たときに探す。
    func find(_ query: String) {
        if let markdownView {
            markdownView.find(query)
        } else if let openPath, MarkdownBlock.isMarkdown(openPath), !markdownText {
            pendingFind = query
        } else {
            content.find(query)
        }
    }

    /// `path` の中身を出す（`line` があればその行へ）。変えたファイルは前後をファイル全体にした差分、変えていない
    /// ファイルはその時点の中身。
    /// partial clone か（ファイルを開く前に、中身が手元にあるかを確かめる）。
    var isPartialClone = false

    func open(path: String, line: Int?) {
        openPath = path
        let index = changedFiles[path]
        let deleted = index.map { self.changes?.items[$0].kind == .deleted } ?? false
        let title = NSMutableAttributedString()
        let folder = (path as NSString).deletingLastPathComponent
        if !folder.isEmpty {
            title.append(NSAttributedString(string: folder.replacingOccurrences(of: "/", with: " / ") + " / ", attributes: [
                .foregroundColor: NSColor.secondaryLabelColor, .font: NSFont.systemFont(ofSize: 12),
            ]))
        }
        title.append(NSAttributedString(string: (path as NSString).lastPathComponent, attributes: [
            .font: NSFont.boldSystemFont(ofSize: 12),
        ]))
        title.append(NSAttributedString(string: "  @ " + String(commit.prefix(8)), attributes: [
            .foregroundColor: NSColor.secondaryLabelColor, .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular),
        ]))
        if deleted {
            title.append(NSAttributedString(string: "  " + L("(deleted in this commit)"), attributes: [
                .foregroundColor: NSColor.systemRed, .font: NSFont.systemFont(ofSize: 11),
            ]))
        }
        pathLabel.attributedStringValue = title
        changeButton.isHidden = index == nil
        treeScroll.isHidden = true
        pathBar.isHidden = false
        needsLayout = true
        request += 1
        let request = self.request
        binaryBox.isHidden = true
        removeImage()
        content.noticeBar = nil
        // partial clone で中身が手元になければ、取りに行かずに「取ってくる」の帯で聞く（押したときだけ取る）。
        if isPartialClone {
            let store = repository, commit = self.commit
            content.view.isHidden = false
            content.show(nil, message: L("Loading the diff…"))
            Task {
                let missing = await Task.detached(priority: .userInitiated) {
                    (try? store.fileContentsMissing(commit: commit, path: path)) ?? []
                }.value
                guard request == self.request else { return }
                if missing.isEmpty { return openContents(path: path, line: line, request: request) }
                content.show(nil, message: L("This file’s contents aren’t here yet."))
                content.noticeBar = MissingContentsBand(
                    message: L("This file’s contents aren’t here yet (partial clone)."),
                    fetch: { try store.fetchContents(hex: missing) }
                ) { [weak self] in
                    guard let self, request == self.request else { return }
                    content.noticeBar = nil
                    openContents(path: path, line: line, request: request)
                }
            }
            return
        }
        openContents(path: path, line: line, request: request)
    }

    /// `open(path:line:)` の、中身を読んで出す分（partial clone なら中身が手元にあると確かめた後）。
    private func openContents(path: String, line: Int?, request: Int) {
        let index = changedFiles[path]
        let svg = ImageDiffPanel.isSVG(path)
        let markdown = MarkdownBlock.isMarkdown(path)
        drawableSwitch.isHidden = true
        markdownSwitch.isHidden = !markdown
        markdownSwitch.selectedSegment = markdownText ? 1 : 0
        if markdown, !markdownText {
            content.view.isHidden = true
            Task {
                guard let data = await contents(of: path), request == self.request else { return }
                let text = String(decoding: data, as: UTF8.self)
                let gitHub = self.gitHub
                let zone = Settings.ganttZone
                let read = self.fileReader()
                let blocks = await Task.detached {
                    MarkdownBlock.parse(text, gitHub: gitHub, timeZone: zone, path: path, image: read)
                }.value
                guard request == self.request else { return }
                let view = MarkdownView(blocks: blocks, path: path) { [weak self] link in self?.image(link, from: path) }
                view.onOpenPath = { [weak self] target in self?.open(path: target, line: nil) }
                markdownView = view
                addSubview(view, positioned: .below, relativeTo: pathBar)
                needsLayout = true
                if let query = pendingFind {
                    pendingFind = nil
                    layoutSubtreeIfNeeded()
                    view.find(query)
                }
            }
            return
        }
        svgSwitch.isHidden = !svg
        svgSwitch.selectedSegment = svgView.rawValue
        // 部品の切り替えは、モバイルの比べる欄の段に置く（ファイルの帯を詰まらせない）。
        rendererSwitch.removeFromSuperview()
        rendererSwitch.selectedSegment = Int(mobileRenderer.rawValue)
        if svg, svgView != .text {
            content.view.isHidden = true
            let view = svgView, renderer = mobileRenderer
            Task {
                guard let data = await contents(of: path), request == self.request else { return }
                // CoreSVG（iOS のアセットカタログ・SF Symbols と同じ描き方）で描く。
                let image = NSImage(data: data)
                let bytes = ByteCountFormatter.string(fromByteCount: Int64(data.count), countStyle: .file)
                let size = image.map { "\(Int($0.size.width)) × \(Int($0.size.height)) pt · \(bytes)" } ?? bytes
                let panel: ImageDiffPanel
                if view == .compare {
                    // 元の SVG と、その部品での見え方を並べる。Android と flutter_svg は本物の取り込みを移したもの
                    // （1 ms 前後なので待たせない）、iOS と react-native-svg は近似。
                    let (issues, preview) = await Task.detached {
                        (SVGIssue.lint(data), renderer.preview(data))
                    }.value
                    guard request == self.request else { return }
                    panel = ImageDiffPanel(
                        before: image, after: preview.svg.flatMap(NSImage.init(data:)),
                        titles: (L("Original"), preview.title),
                        caption: size + "    " + preview.caption,
                        svg: false, mode: .twoUp, position: 0.5, single: false,
                        notes: Self.mobileNotes(issues, preview: preview), accessory: rendererSwitch)
                } else {
                    panel = ImageDiffPanel(
                        before: nil, after: image, titles: ("", ""), caption: size, svg: false, mode: .twoUp,
                        position: 0.5, single: true)
                }
                imagePanel = panel
                addSubview(panel, positioned: .below, relativeTo: pathBar)
                needsLayout = true
            }
            return
        }
        // .xml は中身の頭を見て、Android の VectorDrawable なら絵にする（ほかの XML はテキスト）。
        if (path as NSString).pathExtension.lowercased() == "xml" {
            content.view.isHidden = true
            let store = repository, commitID = self.commitID
            Task {
                let data = await contents(of: path)
                guard request == self.request else { return }
                guard let data, Repository.isVectorDrawable(path: path, data: data) else {
                    return showFile(path: path, index: index, line: line, request: request)
                }
                drawableSwitch.isHidden = false
                drawableSwitch.selectedSegment = drawableText ? 1 : 0
                if drawableText { return showFile(path: path, index: index, line: line, request: request) }
                // 引けない色（Android のテーマ属性など）は、今の外観の文字の色で描く。色の資源も、暗い外観なら
                // values-night などを先に引く（外観が変わったら描き直す）
                var fallback = ""
                effectiveAppearance.performAsCurrentDrawingAppearance { fallback = DiagramPicture.hex(.labelColor) }
                let dark = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
                let picture = await Task.detached(priority: .userInitiated) { [fallback] in
                    try? store.vectorDrawable(
                        commit: commitID, path: path, data: data, fallback: fallback, dark: dark)
                }.value
                guard request == self.request else { return }
                // 描けないもの（参照した drawable がないなど）はテキストで見せる。
                guard let picture, let image = NSImage(data: picture.svg) else {
                    drawableSwitch.isHidden = true
                    return showFile(path: path, index: index, line: line, request: request)
                }
                let bytes = ByteCountFormatter.string(fromByteCount: Int64(data.count), countStyle: .file)
                let size = "\(Int(image.size.width)) × \(Int(image.size.height)) dp · \(bytes)"
                let panel = ImageDiffPanel(
                    before: nil, after: image, titles: ("", ""), caption: size, svg: false, mode: .twoUp,
                    position: 0.5, single: true, footnote: picture.notes)
                imagePanel = panel
                addSubview(panel, positioned: .below, relativeTo: pathBar)
                needsLayout = true
            }
            return
        }
        if ImageDiffPanel.isImage(path), !svg {
            content.view.isHidden = true
            Task {
                let data = await contents(of: path)
                guard request == self.request else { return }
                guard let data, NSImage(data: data) != nil else {
                    // 読めない形式（拡張子は画像でも）はバイナリと同じく Quick Look に。
                    binaryBox.isHidden = false
                    needsLayout = true
                    return
                }
                let panel = ImageDiffPanel(before: nil, after: data, svg: false, mode: .twoUp, position: 0.5, single: true)
                imagePanel = panel
                addSubview(panel, positioned: .below, relativeTo: pathBar)
                needsLayout = true
            }
            return
        }
        showFile(path: path, index: index, line: line, request: request)
    }

    /// `path` の中身をテキストで出す（変えたファイルは前後をファイル全体にした差分、変えていないファイルはその時点の
    /// 中身。`line` があればその行へ）。
    private func showFile(path: String, index: Int?, line: Int?, request: Int) {
        content.view.isHidden = false
        content.show(nil, message: L("Loading the diff…"))
        let changes = self.changes, repository = self.repository, commitID = self.commitID
        Task {
            let diff = await Task.detached(priority: .userInitiated) { () -> FileDiff? in
                if let index { return try? changes?.diff(at: index, context: .max) }
                return try? repository.treeFile(commit: commitID, path: path)
            }.value
            guard request == self.request else { return }
            guard let diff else { return content.show(nil, message: L("Couldn’t load the diff")) }
            if diff.isBinary {
                content.show(nil)
                content.view.isHidden = true
                binaryBox.isHidden = false
                needsLayout = true
                return
            }
            content.show(diff)
            if let line { content.scrollToLine(line) }
        }
    }

    /// 読んだ木の中の `path` のもの。
    private func node(at path: String) -> Node? {
        var node = root
        for part in path.split(separator: "/") {
            guard let next = node.children?.first(where: { $0.name == part }) else { return nil }
            node = next
        }
        return node
    }

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        if flat { return item == nil ? flatFiles.count : 0 }
        let node = (item as? Node) ?? root
        load(node)
        return node.children?.count ?? 0
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        if flat { return flatFiles[index] }
        return ((item as? Node) ?? root).children![index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool { !flat && (item as? Node)?.isDirectory == true }

    func outlineViewItemDidExpand(_ notification: Notification) { onFoldersChange?() }
    func outlineViewItemDidCollapse(_ notification: Notification) { onFoldersChange?() }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? Node else { return nil }
        let cell = NSTableCellView()
        let icon = NSImageView(image: AppIcons.image(symbol: node.isDirectory ? "folder" : "doc")!)
        icon.contentTintColor = node.isDirectory ? .secondaryLabelColor : .tertiaryLabelColor
        var nameAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 13),
            .foregroundColor: node.containsChanges ? NSColor.labelColor : NSColor.secondaryLabelColor,
        ]
        if node.kind == .deleted {
            nameAttributes[.foregroundColor] = NSColor.systemRed
            nameAttributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        }
        let text = NSMutableAttributedString()
        let folder = (node.path as NSString).deletingLastPathComponent
        if flat, !folder.isEmpty {
            text.append(NSAttributedString(string: folder + "/", attributes: [
                .font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.tertiaryLabelColor,
            ]))
        }
        text.append(NSAttributedString(string: node.name, attributes: nameAttributes))
        if let kind = node.kind {
            let (mark, color) = FileCell.mark(kind)
            text.append(NSAttributedString(string: "  " + mark, attributes: [
                .foregroundColor: color, .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .semibold),
            ]))
        } else if node.isDirectory, node.containsChanges {
            text.append(NSAttributedString(string: "  ●", attributes: [
                .foregroundColor: NSColor.systemOrange, .font: NSFont.systemFont(ofSize: 7),
            ]))
        }
        let label = NSTextField(labelWithAttributedString: text)
        label.lineBreakMode = .byTruncatingTail
        let stack = NSStackView(views: [icon, label])
        stack.spacing = 5
        stack.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: cell.trailingAnchor),
            stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }
}
