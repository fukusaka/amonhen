import AmonhenKit
import AppKit

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

    static let imageExtensions: Set<String> = ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "heic", "ico", "svg"]
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
            images: modes.map { NSImage(systemSymbolName: $0.0, accessibilityDescription: $0.1)! },
            trackingMode: .selectOne, target: nil, action: nil)
        for (i, mode) in modes.enumerated() { control.setToolTip(mode.1, forSegment: i) }
        return control
    }()
    private let slider = NSSlider(value: 0.5, minValue: 0, maxValue: 1, target: nil, action: nil)
    private let caption = NSTextField(labelWithString: "")
    private var baseCaption = ""
    /// SVG のとき「テキストで見る」を押した。
    var onShowText: (() -> Void)?

    init(before: Data?, after: Data?, svg: Bool, mode: Mode, position: CGFloat) {
        let old = before.flatMap(NSImage.init(data:)), new = after.flatMap(NSImage.init(data:))
        canvas = ImageCompareCanvas(before: old, after: new)
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
        baseCaption = L("Before") + ": " + describe(before, old) + "    →    " + L("After") + ": " + describe(after, new)
        var top: [NSView] = [modeControl]
        if svg {
            let text = NSButton(
                image: NSImage(systemSymbolName: "doc.plaintext", accessibilityDescription: L("Show as Text"))!,
                target: self, action: #selector(showText))
            text.toolTip = L("Show as Text")
            text.bezelStyle = .accessoryBarAction
            text.controlSize = .small
            top.append(text)
        }
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        let bar = NSStackView(views: top + [spacer, slider])
        bar.spacing = 10
        slider.widthAnchor.constraint(equalToConstant: 160).isActive = true
        let line = NSBox()
        line.boxType = .separator
        for v in [bar, canvas, caption, line] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
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
            caption.bottomAnchor.constraint(equalTo: line.topAnchor, constant: -6),
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

    private func apply() {
        let mode = Mode(rawValue: modeControl.selectedSegment) ?? .twoUp
        canvas.mode = mode
        canvas.position = CGFloat(slider.doubleValue)
        slider.isHidden = mode == .twoUp || mode == .difference
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
    private lazy var difference: (image: CGImage?, changed: Int, total: Int) = makeDifference()

    init(before: NSImage?, after: NSImage?) {
        self.before = before
        self.after = after
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError() }
    override var isFlipped: Bool { true }

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

    /// `size` を `box` に収める（小さな絵は整数倍まで拡げ、ぼかさない）。
    private func fit(_ size: NSSize, in box: NSRect) -> NSRect {
        guard size.width > 0, size.height > 0 else { return .zero }
        var scale = min(box.width / size.width, box.height / size.height)
        if scale > 1 { scale = max(1, floor(min(scale, 8))) }
        let w = size.width * scale, h = size.height * scale
        return NSRect(x: box.midX - w / 2, y: box.midY - h / 2, width: w, height: h).integral
    }

    private func checker(_ rect: NSRect) {
        NSColor.white.setFill()
        rect.fill()
        NSColor(white: 0.86, alpha: 1).setFill()
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
        let common = NSSize(
            width: max(pixelSize(before).width, pixelSize(after).width),
            height: max(pixelSize(before).height, pixelSize(after).height))
        switch mode {
        case .twoUp:
            let half = (area.width - 16) / 2
            for (i, (image, title, color)) in [(before, L("Before"), NSColor.systemRed), (after, L("After"), NSColor.systemGreen)].enumerated() {
                let box = NSRect(x: area.minX + CGFloat(i) * (half + 16), y: area.minY + 16, width: half, height: area.height - 16)
                label(title, at: NSPoint(x: box.minX, y: area.minY))
                if let image {
                    let rect = fit(pixelSize(image), in: box.insetBy(dx: 4, dy: 4))
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
                label(L("Before"), at: NSPoint(x: rect.minX - 56, y: rect.minY))
                label(L("After"), at: NSPoint(x: rect.maxX + 8, y: rect.minY))
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
                label(L("Before"), at: NSPoint(x: rect.minX - 56, y: rect.minY))
                label(L("After"), at: NSPoint(x: rect.maxX + 8, y: rect.minY))
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

    override func mouseDown(with event: NSEvent) { track(event) }
    override func mouseDragged(with event: NSEvent) { track(event) }
    private func track(_ event: NSEvent) {
        guard mode == .swipe || mode == .onion else { return }
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
    private let imageView = NSImageView()
    private let pathBar = NSStackView()
    private let pathLabel = NSTextField(labelWithString: "")
    private let changeButton = NSButton()
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
        imageView.imageScaling = .scaleProportionallyUpOrDown
        let back = NSButton(
            image: NSImage(systemSymbolName: "chevron.left", accessibilityDescription: L("Back"))!, target: self,
            action: #selector(backToTree))
        back.bezelStyle = .accessoryBarAction
        back.controlSize = .small
        back.toolTip = L("Back to the files")
        changeButton.title = L("Show This Change")
        changeButton.bezelStyle = .accessoryBarAction
        changeButton.controlSize = .small
        changeButton.target = self
        changeButton.action = #selector(showChange)
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        pathLabel.lineBreakMode = .byTruncatingHead
        pathLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        for v in [back, pathLabel, spacer, changeButton] { pathBar.addArrangedSubview(v) }
        pathBar.edgeInsets = NSEdgeInsets(top: 0, left: 6, bottom: 0, right: 10)
        pathBar.spacing = 6
        for v in [treeScroll, content.view, imageView, pathBar] as [NSView] { addSubview(v) }
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
        imageView.frame = rest.insetBy(dx: 16, dy: 16)
        outline.sizeLastColumnToFit()
    }

    @objc private func backToTree() {
        openPath = nil
        treeScroll.isHidden = false
        content.view.isHidden = true
        imageView.isHidden = true
        pathBar.isHidden = true
        needsLayout = true
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

    /// `path` の中身を出す（`line` があればその行へ）。変えたファイルは前後をファイル全体にした差分、変えていない
    /// ファイルはその時点の中身。
    func open(path: String, line: Int?) {
        openPath = path
        let index = changedFiles[path]
        let deleted = index.map { changes?.items[$0].kind == .deleted } ?? false
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
        if ImageDiffPanel.isImage(path), !ImageDiffPanel.isSVG(path) {
            content.view.isHidden = true
            imageView.isHidden = false
            let changes = self.changes, repository = self.repository
            let oid = node(at: path)?.oid
            imageView.image = nil
            Task {
                let data = await Task.detached(priority: .userInitiated) { () -> Data? in
                    if let index { return changes?.blob(at: index, after: !deleted) }
                    return oid.flatMap { repository.blob(oid: $0) }
                }.value
                guard request == self.request else { return }
                imageView.image = data.flatMap(NSImage.init(data:))
            }
            return
        }
        imageView.isHidden = true
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
            if diff.isBinary { return content.show(nil, message: L("Binary file")) }
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
        let icon = NSImageView(image: NSImage(systemSymbolName: node.isDirectory ? "folder" : "doc", accessibilityDescription: nil)!)
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
