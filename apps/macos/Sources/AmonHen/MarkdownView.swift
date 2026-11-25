import AmonhenKit
import AppKit
import CAmonhen

/// Markdown の整形表示（History の「すべてのファイル」）。コアが読んだブロックを、NSTextView の文字に自前で組み立てる
/// （WebKit も JavaScript も使わない）。画像はそのコミットの中のものだけを読み、外へは何も読みに行かない。リンクは
/// 押すとブラウザで開く（リポジトリの中のファイルへのリンクは、そのファイルを開く）。HTML の `<details>` は、
/// 見出しを押すと開く・閉じる（組み直す）。`<picture>` は外観に合う画像を出し、外観が変わったら組み直す。数式
/// （```` ```math ````・`$$…$$`・`$…$`）はコアが描いた SVG を、文字の色を当てて絵にする（文中の式は基線にそろえる）。
/// ⌘F で、見えている文字（`<kbd>` の文字も）を探す。帯は差分・ファイルの検索と同じ形。絵と閉じた `<details>` の中は探さない。
final class MarkdownView: NSView, NSTextViewDelegate, NSSearchFieldDelegate {
    private let scroll = NSScrollView()
    /// 文字の欄（試験も中の文字を読む）。
    let text: MarkdownTextView
    /// 文字の置き場（TextKit 1 では文字の置き場が NSLayoutManager を持ち、NSTextView は持たないので、ここで持つ）。
    private let storage = NSTextStorage()
    /// リポジトリの中のファイルへのリンクを押した（Markdown のファイルからの相対パスを解いたもの）。
    var onOpenPath: ((String) -> Void)?
    /// 押したリンクを開く前に、そのファイル（Markdown）の場所。
    private let path: String
    private let blocks: [MarkdownBlock]
    private let image: (String) -> NSImage?
    /// 読んだ画像（組み直すたびに読み直さない。読めなかったものは nil）。
    private var images: [String: NSImage?] = [:]
    /// 開いている `<details>`（文書の中の順の番号）。
    private var openDetails: Set<Int>
    /// 組み立てたときの外観（暗いか）。
    private var builtDark = false

    // 検索（差分の検索と同じ帯・同じ色）
    private let findBar = NSView()
    private let findField = NSSearchField()
    private let findCount = NSTextField(labelWithString: "")
    private(set) var isFindBarVisible = false
    /// 一致（文字の範囲、文書の順）。
    private(set) var matches: [NSRange] = []
    private(set) var currentMatch: Int?
    private static let findBarHeight: CGFloat = 30

    /// `blocks` を組み立てる。`image` はリンクの場所（Markdown のファイルからの相対パス）からそのコミットの画像を読む。
    init(blocks: [MarkdownBlock], path: String, image: @escaping (String) -> NSImage?) {
        self.path = path
        self.blocks = blocks
        self.image = image
        openDetails = MarkdownBuilder.initiallyOpen(blocks)
        // 表（NSTextTable）は TextKit 1 で描く。`<kbd>` の札は自前の NSLayoutManager が描く。
        let layoutManager = KeyCapLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: 600, height: CGFloat.greatestFiniteMagnitude))
        layoutManager.addTextContainer(container)
        text = MarkdownTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 400), textContainer: container)
        super.init(frame: NSRect(x: 0, y: 0, width: 600, height: 400))
        text.owner = self
        text.isEditable = false
        text.isSelectable = true
        text.drawsBackground = false
        text.textContainerInset = NSSize(width: 24, height: 20)
        text.isVerticallyResizable = true
        text.autoresizingMask = .width
        text.textContainer?.widthTracksTextView = true
        text.delegate = self
        // リンクの色は文字に付ける（`<details>` の見出しは押せるが色を変えない）。
        text.linkTextAttributes = [.cursor: NSCursor.pointingHand]
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        addSubview(scroll)
        buildFindBar()
        addSubview(findBar)
        rebuild()
        // 図の配色・構文の色のテーマを変えたら描き直す（図は描くときに色を当てる）。
        NotificationCenter.default.addObserver(self, selector: #selector(themeChanged), name: .syntaxThemeChanged, object: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    @objc private func themeChanged() { text.needsDisplay = true }

    private var dark: Bool { effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua }

    /// 組み立て直す（スクロールの位置は保つ）。
    private func rebuild() {
        builtDark = dark
        var builder = MarkdownBuilder(image: { [weak self] in self?.cachedImage($0) }, dark: builtDark, openDetails: openDetails)
        builder.add(blocks, depth: 0)
        let origin = scroll.contentView.bounds.origin
        text.textStorage?.setAttributedString(builder.out)
        fittedWidth = 0
        fitAttachments()
        scroll.contentView.scroll(to: origin)
        scroll.reflectScrolledClipView(scroll.contentView)
        // `<details>` を開け閉めしたら、見えている文字が変わる。
        if isFindBarVisible { updateMatches(scrollToCurrent: false) }
    }

    private func cachedImage(_ src: String) -> NSImage? {
        if let hit = images[src] { return hit }
        let picture = image(src)
        images[src] = picture
        return picture
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        // `<picture>` の画像は外観で選ぶ。
        if dark != builtDark, MarkdownBuilder.hasPictures(blocks) { rebuild() }
        // `<kbd>` の札の色は描くときの外観で当てる。
        text.needsDisplay = true
    }

    override func layout() {
        super.layout()
        let barHeight = isFindBarVisible ? Self.findBarHeight : 0
        findBar.frame = NSRect(x: 0, y: bounds.height - barHeight, width: bounds.width, height: barHeight)
        scroll.frame = NSRect(x: 0, y: 0, width: bounds.width, height: max(0, bounds.height - barHeight))
        fitAttachments()
    }

    // MARK: - 検索

    private func buildFindBar() {
        findBar.isHidden = true
        findField.placeholderString = L("Search")
        findField.sendsSearchStringImmediately = true
        findField.delegate = self
        findField.target = self
        findField.action = #selector(findFieldChanged)
        findField.controlSize = .small
        findCount.textColor = .secondaryLabelColor
        findCount.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        let previous = NSButton(image: AppIcons.image(symbol: "chevron.up", label: L("Previous"))!,
                                target: self, action: #selector(findPrevious))
        let next = NSButton(image: AppIcons.image(symbol: "chevron.down", label: L("Next"))!,
                            target: self, action: #selector(findNext))
        let done = NSButton(title: L("Done"), target: self, action: #selector(hideFindBar))
        for b in [previous, next, done] {
            b.bezelStyle = .accessoryBarAction
            b.controlSize = .small
        }
        let stack = NSStackView(views: [findField, findCount, previous, next, done])
        stack.spacing = 6
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 8)
        stack.frame = findBar.bounds
        stack.autoresizingMask = [.width, .height]
        findField.widthAnchor.constraint(greaterThanOrEqualToConstant: 180).isActive = true
        findBar.addSubview(stack)
    }

    @objc func showFindBar() {
        let selected = (text.string as NSString).substring(with: text.selectedRange())
        if !selected.isEmpty, !selected.contains("\n") { findField.stringValue = selected }
        if !isFindBarVisible {
            isFindBarVisible = true
            findBar.isHidden = false
            needsLayout = true
            layoutSubtreeIfNeeded()
        }
        window?.makeFirstResponder(findField)
        findField.currentEditor()?.selectAll(nil)
        updateMatches(scrollToCurrent: true)
    }

    @objc func hideFindBar() {
        isFindBarVisible = false
        findBar.isHidden = true
        matches = []
        currentMatch = nil
        highlightMatches()
        needsLayout = true
        window?.makeFirstResponder(text)
    }

    /// 検索欄に文字を入れたまま、検索だけ行う（`--snapshot` の確認用）。
    func find(_ query: String) {
        findField.stringValue = query
        showFindBar()
    }

    @objc private func findFieldChanged() { updateMatches(scrollToCurrent: true) }
    @objc func findNext() { moveMatch(by: 1) }
    @objc func findPrevious() { moveMatch(by: -1) }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.insertNewline(_:)):
            moveMatch(by: NSEvent.modifierFlags.contains(.shift) ? -1 : 1)
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            hideFindBar()
            return true
        default:
            return false
        }
    }

    /// 大文字・小文字を区別せずに、見えている文字から探す（絵は文字を持たないので当たらない）。今の一致は、見えている
    /// 位置より後の最初のもの。
    private func updateMatches(scrollToCurrent: Bool) {
        matches = []
        let query = findField.stringValue
        let s = text.string as NSString
        if !query.isEmpty {
            var search = NSRange(location: 0, length: s.length)
            while true {
                let r = s.range(of: query, options: [.caseInsensitive], range: search)
                guard r.location != NSNotFound else { break }
                matches.append(r)
                let next = r.location + max(r.length, 1)
                guard next < s.length else { break }
                search = NSRange(location: next, length: s.length - next)
            }
        }
        var firstVisible = 0
        if let layout = text.layoutManager, let container = text.textContainer {
            var visible = text.visibleRect
            visible.origin.y -= text.textContainerOrigin.y
            let glyphs = layout.glyphRange(forBoundingRect: visible, in: container)
            firstVisible = layout.characterIndexForGlyph(at: glyphs.location)
        }
        currentMatch = matches.isEmpty ? nil : (matches.firstIndex { $0.location >= firstVisible } ?? 0)
        updateFindCount()
        highlightMatches()
        if scrollToCurrent { revealCurrentMatch() }
    }

    private func moveMatch(by step: Int) {
        if !isFindBarVisible { showFindBar() }
        guard !matches.isEmpty else { return NSSound.beep() }
        currentMatch = ((currentMatch ?? -step) + step + matches.count) % matches.count
        updateFindCount()
        highlightMatches()
        revealCurrentMatch()
    }

    private func updateFindCount() {
        if findField.stringValue.isEmpty {
            findCount.stringValue = ""
        } else if let current = currentMatch {
            findCount.stringValue = "\(current + 1) / \(matches.count)"
        } else {
            findCount.stringValue = L("Not found")
        }
    }

    /// 一致の地を塗る（差分の検索と同じ色。今の一致は濃く）。文字の置き場は変えない（一時の属性）。
    private func highlightMatches() {
        guard let layout = text.layoutManager else { return }
        let whole = NSRange(location: 0, length: (text.string as NSString).length)
        layout.removeTemporaryAttribute(.backgroundColor, forCharacterRange: whole)
        for (i, range) in matches.enumerated() {
            let color = i == currentMatch ? NSColor.systemOrange.withAlphaComponent(0.7) : NSColor.systemYellow.withAlphaComponent(0.35)
            layout.addTemporaryAttribute(.backgroundColor, value: color, forCharacterRange: range)
        }
    }

    private func revealCurrentMatch() {
        guard let current = currentMatch else { return }
        text.scrollRangeToVisible(matches[current])
    }

    /// 絵（画像・図）を列の幅に収める（640 pt まで、高さは比を保つ）。幅が変わったときだけ直す。
    private var fittedWidth: CGFloat = 0
    private func fitAttachments() {
        guard let storage = text.textStorage, let container = text.textContainer else { return }
        let width = container.size.width - container.lineFragmentPadding * 2 - 8
        guard width > 0, abs(width - fittedWidth) > 0.5 else { return }
        fittedWidth = width
        var changed = false
        storage.enumerateAttribute(.attachment, in: NSRange(location: 0, length: storage.length)) { value, range, _ in
            guard let attachment = value as? FittingAttachment else { return }
            let size = attachment.fitting(width)
            if attachment.bounds.size != size {
                attachment.bounds = NSRect(origin: .zero, size: size)
                storage.edited(.editedAttributes, range: range, changeInLength: 0)
                changed = true
            }
        }
        if changed { text.layoutManager?.invalidateLayout(forCharacterRange: NSRange(location: 0, length: storage.length), actualCharacterRange: nil) }
    }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        let target = (link as? URL)?.absoluteString ?? (link as? String) ?? ""
        if let number = target.split(separator: ":").last.flatMap({ Int($0) }), target.hasPrefix(MarkdownBuilder.detailsLink) {
            // `<details>` の見出し: 開く・閉じる。
            if openDetails.remove(number) == nil { openDetails.insert(number) }
            rebuild()
            return true
        }
        if let url = URL(string: target), let scheme = url.scheme, ["http", "https", "mailto"].contains(scheme) {
            NSWorkspace.shared.open(url)
        } else if !target.hasPrefix("#"), let resolved = Self.resolve(target, from: path) {
            onOpenPath?(resolved)
        }
        return true
    }

    /// Markdown のファイル `from` からの相対パス `link` を、リポジトリの根からのパスにする（`../` も解く）。
    static func resolve(_ link: String, from: String) -> String? {
        let target = link.split(separator: "#", maxSplits: 1).first.map(String.init) ?? link
        guard !target.isEmpty, !target.contains("://") else { return nil }
        var parts: [Substring] = target.hasPrefix("/") ? [] : (from as NSString).deletingLastPathComponent.split(separator: "/")
        for part in target.split(separator: "/") {
            if part == ".." { _ = parts.popLast() } else if part != "." { parts.append(part) }
        }
        let path = parts.joined(separator: "/").removingPercentEncoding ?? parts.joined(separator: "/")
        return path.isEmpty ? nil : path
    }
}

/// Markdown の図の絵。形は pele の SVG のまま持ち、色は描くたびに、そのときの外観・構文の色のテーマ・設定の
/// 「図の配色」で当てる（当てた絵は配色ごとに覚える）。テーマを変えても組み直さず、描き直すだけでよい。
final class DiagramPicture {
    private let svg: String
    /// 図の中で指定したテーマ（設定より先）。
    private let own: MermaidThemeConfig?
    private var colored: [String: NSImage] = [:]
    /// 配色・外観・画素の大きさごとに、画素にした絵（描き直しのたびに CoreSVG で SVG を描き直さない）。
    private var bitmaps: [String: NSImage] = [:]

    private init(svg: String, own: MermaidThemeConfig?) {
        self.svg = svg
        self.own = own
    }

    static func image(_ svg: String, own: MermaidThemeConfig?) -> NSImage? {
        let picture = DiagramPicture(svg: svg, own: own)
        // 大きさは配色によらないので、pele の既定の色で一度当てて測る。
        guard let first = MarkdownBlock.colorizeDiagram(svg, scheme: "pele", dark: false, theme: [:]).flatMap(NSImage.init(data:))
        else { return nil }
        let image = NSImage(size: first.size, flipped: false) { rect in
            // 文字の描画（メインスレッド）から呼ばれる。
            MainActor.assumeIsolated { picture.draw(in: rect) }
            return true
        }
        image.cacheMode = .never
        return image
    }

    /// `rect` に描く。今の配色と、描く先の画素の大きさで画素にした絵を覚えておき、次からはそれを描く。
    func draw(in rect: NSRect) {
        guard let (key, vector) = current() else { return }
        let scale = NSGraphicsContext.current.map { abs($0.cgContext.userSpaceToDeviceSpaceTransform.a) } ?? 2
        let width = Int((rect.width * scale).rounded(.up)), height = Int((rect.height * scale).rounded(.up))
        guard width > 0, height > 0 else { return }
        let bitmapKey = "\(key)\n\(width)x\(height)"
        if let bitmap = bitmaps[bitmapKey] {
            bitmap.draw(in: rect)
            return
        }
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4,
            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
        else { return vector.draw(in: rect) }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        vector.draw(in: NSRect(x: 0, y: 0, width: width, height: height))
        NSGraphicsContext.restoreGraphicsState()
        let bitmap = NSImage(size: rect.size)
        bitmap.addRepresentation(rep)
        // 配色・外観を変えたら古い絵は要らない（大きさ違いは残す: 列の幅を変えている途中）
        bitmaps = bitmaps.filter { $0.key.hasPrefix(key + "\n") }
        bitmaps[bitmapKey] = bitmap
        bitmap.draw(in: rect)
    }

    /// 今の外観（描いている最中のもの）での絵と、その配色の鍵。
    private func current() -> (String, NSImage)? {
        let dark = NSAppearance.currentDrawing().bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let scheme = Settings.diagramColors
        let themeName = dark ? Settings.syntaxThemeDark : Settings.syntaxThemeLight
        let imported = scheme.hasPrefix("mermaid-import:")
            ? Settings.importedMermaidThemes[String(scheme.dropFirst("mermaid-import:".count))] : nil
        let key = "\(scheme)\n\(themeName)\n\(dark)\n\(imported?.json ?? "")"
        if let image = colored[key] { return (key, image) }
        var theme: [String: Any] = [:]
        if scheme == "syntax" {
            let syntax = SyntaxTheme.named(themeName) ?? .system
            var kinds: [String: String] = [:]
            for (name, kind) in SyntaxTheme.kinds { kinds[name] = Self.hex(syntax.color(kind)) }
            theme = ["text": Self.hex(.labelColor), "background": Self.hex(.textBackgroundColor), "kinds": kinds]
        }
        let image = MarkdownBlock.colorizeDiagram(svg, scheme: scheme, dark: dark, theme: theme, own: own, imported: imported)
            .flatMap(NSImage.init(data:))
        colored[key] = image
        return image.map { (key, $0) }
    }

    /// 今の外観での色の `#rrggbb`。
    static func hex(_ color: NSColor) -> String {
        guard let c = color.usingColorSpace(.sRGB) else { return "#000000" }
        let v = { (x: CGFloat) in Int((max(0, min(1, x)) * 255).rounded()) }
        return String(format: "#%02x%02x%02x", v(c.redComponent), v(c.greenComponent), v(c.blueComponent))
    }
}

/// Markdown の数式の絵。形はコアが描いた SVG のまま持ち、色は描くたびに、そのときの外観の文字の色で当てる（当てた
/// 絵は色ごとに、画素にした絵は色と画素の大きさごとに覚える）。描く先の画素の大きさで画素にするので、拡大しても
/// ぼやけない。テーマや外観を変えても組み直さず、描き直すだけでよい。
final class FormulaPicture {
    private let svg: String
    private var colored: [String: NSImage] = [:]
    private var bitmaps: [String: NSImage] = [:]

    private init(svg: String) { self.svg = svg }

    /// 大きさ `size`（pt）の絵。
    static func image(_ svg: String, size: NSSize) -> NSImage {
        let picture = FormulaPicture(svg: svg)
        let image = NSImage(size: size, flipped: false) { rect in
            // 文字の描画（メインスレッド）から呼ばれる。
            MainActor.assumeIsolated { picture.draw(in: rect) }
            return true
        }
        image.cacheMode = .never
        return image
    }

    /// 本文の字の大きさ `fontSize` で描く数式の大きさ（pt）と、基線から下の深さ（pt）。
    static func metrics(_ formula: MarkdownBlock.Formula, fontSize: CGFloat) -> (size: NSSize, depth: CGFloat) {
        let em = fontSize * Settings.mathScale
        return (NSSize(width: formula.width * em, height: (formula.height + formula.depth) * em), formula.depth * em)
    }

    func draw(in rect: NSRect) {
        let color = DiagramPicture.hex(.labelColor)
        let vector: NSImage
        if let hit = colored[color] {
            vector = hit
        } else {
            guard let made = MarkdownBlock.colorizeMath(svg, foreground: color).flatMap(NSImage.init(data:)) else { return }
            colored[color] = made
            vector = made
        }
        let scale = NSGraphicsContext.current.map { abs($0.cgContext.userSpaceToDeviceSpaceTransform.a) } ?? 2
        let width = Int((rect.width * scale).rounded(.up)), height = Int((rect.height * scale).rounded(.up))
        guard width > 0, height > 0 else { return }
        let key = "\(color)\n\(width)x\(height)"
        if let bitmap = bitmaps[key] {
            bitmap.draw(in: rect)
            return
        }
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4,
            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
        else { return vector.draw(in: rect) }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        vector.draw(in: NSRect(x: 0, y: 0, width: width, height: height))
        NSGraphicsContext.restoreGraphicsState()
        let bitmap = NSImage(size: rect.size)
        bitmap.addRepresentation(rep)
        // 色を変えたら古い絵は要らない（大きさ違いは残す: 拡大の途中）
        bitmaps = bitmaps.filter { $0.key.hasPrefix(color + "\n") }
        bitmaps[key] = bitmap
        bitmap.draw(in: rect)
    }
}

/// 列の幅に縮む絵。
nonisolated final class FittingAttachment: NSTextAttachment {
    private let natural: NSSize
    /// 幅の割合（HTML の `width="50%"`）と、その基準の列の幅に対する割合（表のマスの中ならマスの分）。
    private let percent: CGFloat?
    private let share: CGFloat?
    private let fixedHeight: CGFloat?

    /// `width`・`height`（HTML の大きさ、pt）があればその大きさで（片方だけなら比を保つ）。`widthPercent` があれば
    /// 幅は列の幅（の `share`）のその割合（`fitting` で決める）。
    init(image: NSImage, width: Int? = nil, height: Int? = nil, widthPercent: Int? = nil, share: CGFloat? = nil) {
        let size = image.size
        percent = widthPercent.map { CGFloat($0) / 100 }
        self.share = share
        fixedHeight = height.map { CGFloat($0) }
        natural = switch (width, height) {
        case let (w?, h?): NSSize(width: w, height: h)
        case let (w?, nil): NSSize(width: CGFloat(w), height: size.width > 0 ? size.height * CGFloat(w) / size.width : size.height)
        case let (nil, h?): NSSize(width: size.height > 0 ? size.width * CGFloat(h) / size.height : size.width, height: CGFloat(h))
        case (nil, nil): size
        }
        super.init(data: nil, ofType: nil)
        self.image = image
        bounds = NSRect(origin: .zero, size: fitting(640))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// 幅 `width`（と 640 pt）に収めた大きさ。幅の割合があれば、幅 `width` の割合（高さは決まっていなければ比を保つ）。
    func fitting(_ width: CGFloat) -> NSSize {
        let limit = min(width, 640)
        var size = natural
        if let percent {
            // 表のマスの中なら、マスの左右の余白と枠（6 pt と 1 pt ずつ）を除いた幅に対して。
            let base = share.map { max(width * $0 - 14, 0) } ?? width
            let w = (base * percent).rounded()
            let picture = image?.size ?? natural
            size = NSSize(width: w, height: fixedHeight ?? (picture.width > 0 ? picture.height * w / picture.width : picture.height))
        }
        guard size.width > limit, size.width > 0 else { return size }
        return NSSize(width: limit, height: size.height * limit / size.width)
    }
}

/// Markdown の文字の欄。⌘F・⌘G・⇧⌘G を MarkdownView の検索へ渡す（差分の欄と同じく、この欄で探していなければ
/// ⌘G は History の検索の一致へ）。
final class MarkdownTextView: NSTextView {
    weak var owner: MarkdownView?

    @objc func showFindBar(_ sender: Any?) { owner?.showFindBar() }
    @objc func findNext(_ sender: Any?) {
        if owner?.isFindBarVisible == true || !moveHistoryMatch(from: window, by: 1) { owner?.findNext() }
    }
    @objc func findPrevious(_ sender: Any?) {
        if owner?.isFindBarVisible == true || !moveHistoryMatch(from: window, by: -1) { owner?.findPrevious() }
    }
}

extension NSAttributedString.Key {
    /// `<kbd>` の文字（値は札ごとの番号。並んだ札を分ける）。
    nonisolated static let amonhenKeyCap = NSAttributedString.Key("AmonhenKeyCap")
}

/// `<kbd>` の札の枠を、`.amonhenKeyCap` の付いた文字の後ろに描く（行に折り返したら行ごとに 1 つ）。色は描くときの
/// 外観で当てる。地は薄く透かすので、選んだ範囲の色は札の上でも見える。表の中も同じ（表の升も同じ入れ物に並ぶ）。
nonisolated final class KeyCapLayoutManager: NSLayoutManager {
    /// 枠と文字の間の左右の空き。
    static let padding: CGFloat = 4
    /// 枠の外の、隣の文字との間。
    static let margin: CGFloat = 2
    /// 札の左右に空ける幅（字送りで空ける）。
    static let room = padding + margin

    /// 行が札で始まるときに文字を送るもの（NSLayoutManager は delegate を弱く持つので、ここで持つ）。
    private let lineStart = KeyCapLineStart()

    override init() {
        super.init()
        delegate = lineStart
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
        guard let storage = textStorage, glyphsToShow.length > 0 else { return }
        let shown = characterRange(forGlyphRange: glyphsToShow, actualGlyphRange: nil)
        storage.enumerateAttribute(.amonhenKeyCap, in: shown) { value, range, _ in
            guard value != nil else { return }
            // 見せる範囲の外まで続く札も、札の全体で描く（はみ出た分は切られる）。
            var whole = range
            _ = storage.attribute(.amonhenKeyCap, at: range.location, longestEffectiveRange: &whole, in: NSRange(location: 0, length: storage.length))
            for rect in capRects(whole) {
                drawCap(NSRect(x: rect.minX + origin.x, y: rect.minY + origin.y, width: rect.width, height: rect.height))
            }
        }
    }

    /// 札（文字の範囲 `characters`）の枠。入れ物の中の位置で、行ごとに 1 つ。行の端の空白は枠に入れない。
    func capRects(_ characters: NSRange) -> [NSRect] {
        guard let storage = textStorage else { return [] }
        let text = storage.string as NSString
        let glyphs = glyphRange(forCharacterRange: characters, actualCharacterRange: nil)
        var rects: [NSRect] = []
        var index = glyphs.location
        while index < NSMaxRange(glyphs) {
            var line = NSRange()
            let fragment = lineFragmentRect(forGlyphAt: index, effectiveRange: &line)
            guard line.length > 0 else { break }
            var piece = NSIntersectionRange(line, glyphs)
            index = NSMaxRange(line)
            // 行の頭・終わりの空白（折り返したところ）は外す。
            func blank(_ glyph: Int) -> Bool {
                let c = text.character(at: characterIndexForGlyph(at: glyph))
                return c == 0x20 || c == 0x09 || c == 0x2028 || c == 0x0A
            }
            while piece.length > 0, blank(piece.location) { piece = NSRange(location: piece.location + 1, length: piece.length - 1) }
            while piece.length > 0, blank(NSMaxRange(piece) - 1) { piece.length -= 1 }
            guard piece.length > 0, let container = textContainer(forGlyphAt: piece.location, effectiveRange: nil) else { continue }
            var box = NSRect.null
            enumerateEnclosingRects(
                forGlyphRange: piece, withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0), in: container
            ) { rect, _ in box = box.union(rect) }
            guard !box.isNull else { continue }
            // 最後の文字の字送り（右の空き）は枠に入れない。
            let last = characterIndexForGlyph(at: NSMaxRange(piece) - 1)
            let kern = (storage.attribute(.kern, at: last, effectiveRange: nil) as? CGFloat) ?? 0
            let right = box.maxX - kern
            // 高さは札の字の大きさで、基線から測る（行の高さ・行間によらない）。
            let font = (storage.attribute(.font, at: characterIndexForGlyph(at: piece.location), effectiveRange: nil) as? NSFont)
                ?? .monospacedSystemFont(ofSize: 11, weight: .regular)
            let baseline = fragment.minY + location(forGlyphAt: piece.location).y
            let top = baseline - font.ascender - 2
            let bottom = baseline - font.descender + 2
            rects.append(NSRect(x: box.minX - Self.padding, y: top, width: right - box.minX + Self.padding * 2, height: bottom - top))
        }
        return rects
    }

    /// 札を 1 つ描く（GitHub の札に似せる: 細い枠、下の縁を少し濃く、角を丸め、地は薄く）。
    private func drawCap(_ rect: NSRect) {
        let frame = rect.insetBy(dx: 0.5, dy: 0.5)
        let path = NSBezierPath(roundedRect: frame, xRadius: 4.5, yRadius: 4.5)
        NSColor.labelColor.withAlphaComponent(0.05).setFill()
        path.fill()
        // 下の縁（内側に 1 px の影）。描く先は上下が逆（flipped）。
        NSGraphicsContext.saveGraphicsState()
        path.addClip()
        NSColor.labelColor.withAlphaComponent(0.22).setFill()
        NSRect(x: frame.minX, y: frame.maxY - 1.5, width: frame.width, height: 1).fill(using: .sourceOver)
        NSGraphicsContext.restoreGraphicsState()
        NSColor.labelColor.withAlphaComponent(0.25).setStroke()
        path.lineWidth = 1
        path.stroke()
    }
}

/// 行が札で始まるとき（段落・表の升の頭、折り返した札の続き）、その行だけ文字を枠の内側の空きの分だけ右へ送る。
/// 枠の左の端が、ほかの行の文字の始まりにそろう（字送りでは行の頭に空きを作れない）。
nonisolated final class KeyCapLineStart: NSObject, NSLayoutManagerDelegate {
    func layoutManager(
        _ layoutManager: NSLayoutManager, shouldSetLineFragmentRect lineFragmentRect: UnsafeMutablePointer<NSRect>,
        lineFragmentUsedRect: UnsafeMutablePointer<NSRect>, baselineOffset: UnsafeMutablePointer<CGFloat>,
        in textContainer: NSTextContainer, forGlyphRange glyphRange: NSRange
    ) -> Bool {
        guard glyphRange.length > 0, let storage = layoutManager.textStorage else { return false }
        let first = layoutManager.characterIndexForGlyph(at: glyphRange.location)
        guard first < storage.length, storage.attribute(.amonhenKeyCap, at: first, effectiveRange: nil) != nil,
            !CharacterSet.whitespacesAndNewlines.contains((storage.string as NSString).character(at: first).scalar)
        else { return false }
        // 行の右の端は動かさない（表の升の枠は行の大きさから決まる）。
        let shift = KeyCapLayoutManager.padding
        lineFragmentRect.pointee.origin.x += shift
        lineFragmentRect.pointee.size.width -= shift
        lineFragmentUsedRect.pointee.origin.x += shift
        lineFragmentUsedRect.pointee.size.width = min(lineFragmentUsedRect.pointee.width, lineFragmentRect.pointee.width)
        return true
    }
}

private extension unichar {
    nonisolated var scalar: Unicode.Scalar { Unicode.Scalar(self) ?? " " }
}

/// ブロックを文字に組み立てる。
private struct MarkdownBuilder {
    let image: (String) -> NSImage?
    /// 暗い外観か（`<picture>` の画像を選ぶ）。
    let dark: Bool
    /// 開いている `<details>` の番号。
    let openDetails: Set<Int>
    let out = NSMutableAttributedString()
    private let bodySize: CGFloat = 14
    /// これまでの `<kbd>` の数（札ごとに `.amonhenKeyCap` の値を変え、並んだ札を 1 つにしない）。
    private let keyCaps = Counter()
    private final class Counter { var count = 0 }
    /// 次の `<details>` の番号（文書の中の順。閉じたものの中のものも数える）。
    private var detailsIndex = 0
    /// `<details>` の見出しのリンク（後ろに番号）。
    static let detailsLink = "amonhen-details:"

    init(image: @escaping (String) -> NSImage?, dark: Bool, openDetails: Set<Int>) {
        self.image = image
        self.dark = dark
        self.openDetails = openDetails
    }

    /// `blocks` を、`<details>` の番号の順（入れ子は親の後）にたどる。
    private static func walk(_ blocks: [MarkdownBlock], _ visit: (MarkdownBlock) -> Void) {
        for block in blocks {
            visit(block)
            switch block {
            case .quote(_, let inner), .footnote(_, let inner), .details(_, _, let inner): walk(inner, visit)
            case .list(_, let items): for item in items { walk(item.blocks, visit) }
            default: break
            }
        }
    }

    /// 最初から開いている `<details>`（`open` の付いたもの）の番号。
    static func initiallyOpen(_ blocks: [MarkdownBlock]) -> Set<Int> {
        var index = 0
        var open: Set<Int> = []
        walk(blocks) { block in
            if case .details(_, let isOpen, _) = block {
                if isOpen { open.insert(index) }
                index += 1
            }
        }
        return open
    }

    /// `blocks` の中の `<details>` の数。
    private static func countDetails(_ blocks: [MarkdownBlock]) -> Int {
        var count = 0
        walk(blocks) { if case .details = $0 { count += 1 } }
        return count
    }

    /// 外観で選ぶ画像（`<picture>`）があるか。
    static func hasPictures(_ blocks: [MarkdownBlock]) -> Bool {
        var found = false
        func check(_ runs: [MarkdownBlock.Run]) {
            if runs.contains(where: { $0.imageDark != nil || $0.imageLight != nil }) { found = true }
        }
        walk(blocks) { block in
            switch block {
            case .heading(_, let runs, _), .paragraph(let runs, _), .details(let runs, _, _): check(runs)
            case .table(_, let head, let rows): (head + rows.flatMap { $0 }).forEach(check)
            case .htmlTable(_, _, let cells): cells.forEach { check($0.runs) }
            default: break
            }
        }
        return found
    }

    /// HTML の `align` の値の、文字のそろえ方。
    private static func alignment(_ align: String?) -> NSTextAlignment {
        switch align {
        case "center": .center
        case "right": .right
        default: .natural
        }
    }

    /// 見出しのない `<details>` の見出し（GitHub と同じ「Details」。訳の鍵は「HTML details: 」を付けたもの）。
    private static var detailsTitle: String {
        let text = L("HTML details: Details")
        return text.hasPrefix("HTML details: ") ? String(text.dropFirst("HTML details: ".count)) : text
    }

    /// `<kbd>` の札。文字はふつうの文字のまま（写す・探す・読み上げで読める）、等幅で本文より少し小さく、
    /// `.amonhenKeyCap` を付ける。枠は KeyCapLayoutManager が文字の後ろに描く。枠の左右の空きは、前の文字と札の
    /// 最後の文字の字送り（kern）で空ける。`before` はこの札の前に並ぶ文字（同じ段落の中なら左の空きを足す）。
    private func keyCap(_ run: MarkdownBlock.Run, size: CGFloat, bold: Bool, style: NSParagraphStyle, before: NSMutableAttributedString)
        -> NSAttributedString
    {
        var font = NSFont.monospacedSystemFont(ofSize: size - 3, weight: .regular)
        var traits: NSFontDescriptor.SymbolicTraits = []
        if bold || run.bold == true { traits.insert(.bold) }
        if run.italic == true { traits.insert(.italic) }
        if !traits.isEmpty {
            font = NSFont(descriptor: font.fontDescriptor.withSymbolicTraits(traits), size: font.pointSize) ?? font
        }
        keyCaps.count += 1
        var attributes: [NSAttributedString.Key: Any] = [
            .font: font, .foregroundColor: NSColor.labelColor, .paragraphStyle: style, .amonhenKeyCap: keyCaps.count,
        ]
        if let link = run.link { attributes[.link] = link }
        let result = NSMutableAttributedString(string: run.text, attributes: attributes)
        guard result.length > 0 else { return result }
        let room = KeyCapLayoutManager.room
        result.addAttribute(.kern, value: room, range: NSRange(location: result.length - 1, length: 1))
        // 前の文字との間（段落・行の頭なら要らない）。
        let previous = before.length - 1
        if previous >= 0, !["\n", "\u{2028}"].contains((before.string as NSString).substring(with: NSRange(location: previous, length: 1))) {
            let kern = (before.attribute(.kern, at: previous, effectiveRange: nil) as? CGFloat) ?? 0
            before.addAttribute(.kern, value: kern + room, range: NSRange(location: previous, length: 1))
        }
        return result
    }

    private func paragraph(indent: CGFloat = 0, spacing: CGFloat = 10, blocks: [NSTextBlock] = []) -> NSMutableParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.paragraphSpacing = spacing
        style.lineSpacing = 2
        style.headIndent = indent
        style.firstLineHeadIndent = indent
        style.textBlocks = blocks
        return style
    }

    /// 飾りの文字。
    /// `share` は表のマスの中なら、列の幅に対するマスの幅の割合（絵の幅の割合の基準）。
    private func runs(_ runs: [MarkdownBlock.Run], size: CGFloat, bold: Bool = false, style: NSParagraphStyle, share: CGFloat? = nil) -> NSAttributedString {
        let result = NSMutableAttributedString()
        for run in runs {
            if run.lineBreak == true {
                result.append(NSAttributedString(string: "\u{2028}", attributes: [.paragraphStyle: style]))
                continue
            }
            if let src = run.image(dark: dark) {
                if let picture = image(src) {
                    // 列より広い絵は縮める（MarkdownView.fitAttachments）。リンクの中の絵（バッジなど）は押せる。
                    let piece = NSMutableAttributedString(attachment: FittingAttachment(
                        image: picture, width: run.width, height: run.height, widthPercent: run.widthPercent, share: share))
                    var attributes: [NSAttributedString.Key: Any] = [.paragraphStyle: style]
                    if let link = run.link { attributes[.link] = link }
                    if !run.text.isEmpty { attributes[.toolTip] = run.text }
                    piece.addAttributes(attributes, range: NSRange(location: 0, length: piece.length))
                    result.append(piece)
                } else {
                    // 読めない画像（外の URL など）は代わりの文。
                    var attributes: [NSAttributedString.Key: Any] = [
                        .font: NSFont.systemFont(ofSize: size), .foregroundColor: NSColor.secondaryLabelColor, .paragraphStyle: style,
                    ]
                    if let link = run.link { attributes[.link] = link }
                    result.append(NSAttributedString(string: "[\(run.text.isEmpty ? src : run.text)]", attributes: attributes))
                }
                continue
            }
            if run.kbd == true {
                result.append(keyCap(run, size: size, bold: bold, style: style, before: result))
                continue
            }
            if let formula = run.math {
                // 文中の数式: 基線を本文にそろえる（深さの分だけ下に出す）。原文は浮かぶ説明に。
                let (box, depth) = FormulaPicture.metrics(formula, fontSize: size)
                let attachment = NSTextAttachment()
                attachment.image = FormulaPicture.image(formula.svg, size: box)
                attachment.bounds = NSRect(x: 0, y: -depth, width: box.width, height: box.height)
                let piece = NSMutableAttributedString(attachment: attachment)
                var attributes: [NSAttributedString.Key: Any] = [.paragraphStyle: style, .toolTip: run.text]
                if let link = run.link { attributes[.link] = link }
                piece.addAttributes(attributes, range: NSRange(location: 0, length: piece.length))
                result.append(piece)
                continue
            }
            // 下付き・上付きは小さく。
            let small = run.sub == true || run.sup == true
            let fontSize = small ? size * 0.75 : size
            var font: NSFont = run.code == true
                ? .monospacedSystemFont(ofSize: fontSize - 1.5, weight: .regular) : .systemFont(ofSize: fontSize)
            var traits: NSFontDescriptor.SymbolicTraits = []
            if bold || run.bold == true { traits.insert(.bold) }
            if run.italic == true { traits.insert(.italic) }
            if !traits.isEmpty {
                font = NSFont(descriptor: font.fontDescriptor.withSymbolicTraits(traits), size: font.pointSize) ?? font
            }
            var attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.labelColor, .paragraphStyle: style]
            if run.code == true { attributes[.backgroundColor] = NSColor.quaternaryLabelColor.withAlphaComponent(0.18) }
            if run.strike == true { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
            if run.underline == true { attributes[.underlineStyle] = NSUnderlineStyle.single.rawValue }
            if run.sub == true { attributes[.baselineOffset] = -size * 0.2 }
            if run.sup == true { attributes[.baselineOffset] = size * 0.35 }
            if let link = run.link {
                attributes[.link] = link
                attributes[.foregroundColor] = NSColor.linkColor
            }
            if let error = run.mathError {
                // 描けなかった文中の数式は原文をコードとして見せ、理由を浮かぶ説明に。
                attributes[.foregroundColor] = NSColor.systemRed
                attributes[.toolTip] = error
            }
            if run.customEmoji != nil {
                // GitHub 独自の絵文字は画像を持たないので、名前を薄い札で見せる。
                attributes[.font] = NSFont.systemFont(ofSize: size - 2)
                attributes[.foregroundColor] = NSColor.secondaryLabelColor
                attributes[.backgroundColor] = NSColor.quaternaryLabelColor.withAlphaComponent(0.25)
                attributes[.toolTip] = L("GitHub’s own emoji, shown by name (its image isn’t included)")
            }
            var text = run.text
            if let note = run.footnote {
                text = "[\(note)]"
                attributes[.baselineOffset] = size * 0.35
                attributes[.font] = NSFont.systemFont(ofSize: size * 0.7)
            }
            result.append(NSAttributedString(string: text, attributes: attributes))
        }
        return result
    }

    private func newline(_ style: NSParagraphStyle) -> NSAttributedString {
        NSAttributedString(string: "\n", attributes: [.paragraphStyle: style, .font: NSFont.systemFont(ofSize: bodySize)])
    }

    /// 引用・囲みの枠（左に縦の線）。
    private func quoteBlock(_ color: NSColor) -> NSTextBlock {
        let block = NSTextBlock()
        // 幅を決めないと、枠の中の幅が 0 になる。
        block.setValue(100, type: .percentageValueType, for: .width)
        block.setWidth(4, type: .absoluteValueType, for: .border, edge: .minX)
        block.setBorderColor(color, for: .minX)
        block.setWidth(12, type: .absoluteValueType, for: .padding, edge: .minX)
        block.setWidth(2, type: .absoluteValueType, for: .padding, edge: .minY)
        block.setWidth(2, type: .absoluteValueType, for: .padding, edge: .maxY)
        return block
    }

    mutating func add(_ blocks: [MarkdownBlock], depth: Int, outer: [NSTextBlock] = [], indent: CGFloat = 0) {
        for block in blocks {
            switch block {
            case .heading(let level, let content, let align):
                let sizes: [CGFloat] = [26, 21, 17, 15, 14, 13]
                let style = paragraph(indent: indent, spacing: level <= 2 ? 12 : 8, blocks: outer)
                style.alignment = Self.alignment(align)
                style.paragraphSpacingBefore = out.length == 0 ? 0 : 10
                out.append(runs(content, size: sizes[min(max(level, 1), 6) - 1], bold: true, style: style))
                out.append(newline(style))
            case .paragraph(let content, let align):
                let style = paragraph(indent: indent, blocks: outer)
                style.alignment = Self.alignment(align)
                out.append(runs(content, size: bodySize, style: style))
                out.append(newline(style))
            case .code(let language, let code):
                let box = NSTextBlock()
                box.setValue(100, type: .percentageValueType, for: .width)
                box.backgroundColor = NSColor.quaternaryLabelColor.withAlphaComponent(0.14)
                box.setWidth(10, type: .absoluteValueType, for: .padding)
                let style = paragraph(indent: indent, spacing: 10, blocks: outer + [box])
                style.lineSpacing = 1
                let start = out.length
                // 行は段落を分けずに改行する（段落の間隔を行ごとに付けない）。長さは変わらないので構文の色の位置はそのまま。
                out.append(NSAttributedString(string: code.replacingOccurrences(of: "\n", with: "\u{2028}"), attributes: [
                    .font: NSFont.monospacedSystemFont(ofSize: 12.5, weight: .regular), .foregroundColor: NSColor.labelColor,
                    .paragraphStyle: style,
                ]))
                // 構文の色（フェンスの言葉を拡張子とみて）。
                let palette = SyntaxColors.palette
                if let spans = SyntaxSpans.highlight(path: "code." + Self.fileExtension(language), text: code) {
                    spans.withSpans { all in
                        for span in all where Int(span.kind) < palette.count && span.end > span.start {
                            let range = NSRange(location: start + Int(span.start), length: Int(span.end - span.start))
                            if NSMaxRange(range) <= out.length { out.addAttribute(.foregroundColor, value: palette[Int(span.kind)], range: range) }
                        }
                    }
                }
                out.append(newline(style))
            case .diagram(let language, let code, let svg, let config, let note, let error):
                guard let picture = svg.flatMap({ DiagramPicture.image($0, own: config) }) else {
                    // 描けない図はコードとして見せ、理由を添える。
                    add([.code(language: language, text: code)], depth: depth, outer: outer, indent: indent)
                    if let error {
                        let style = paragraph(indent: indent, spacing: 10, blocks: outer)
                        // 誤りの位置を ^ で指すので等幅で。行は段落を分けずに改行する。
                        out.append(NSAttributedString(string: error.replacingOccurrences(of: "\n", with: "\u{2028}"), attributes: [
                            .font: NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular), .foregroundColor: NSColor.systemRed,
                            .paragraphStyle: style,
                        ]))
                        out.append(newline(style))
                    }
                    continue
                }
                let style = paragraph(indent: indent, spacing: 12, blocks: outer)
                // 列より広い図は縮める（MarkdownView.fitAttachments）。
                let text = NSMutableAttributedString(attachment: FittingAttachment(image: picture))
                text.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: text.length))
                out.append(text)
                out.append(newline(style))
                // 近いもので描いた図には、そのことを絵の下に添える。
                if let note {
                    let noteStyle = paragraph(indent: indent, spacing: 10, blocks: outer)
                    out.append(NSAttributedString(string: note, attributes: [
                        .font: NSFont.systemFont(ofSize: 11.5), .foregroundColor: NSColor.secondaryLabelColor,
                        .paragraphStyle: noteStyle,
                    ]))
                    out.append(newline(noteStyle))
                }
            case .math(let code, let formula, let error):
                guard let formula else {
                    // 描けない数式（数式を描かない作りも）は TeX をコードとして見せ、理由を添える。
                    add([.code(language: "math", text: code)], depth: depth, outer: outer, indent: indent)
                    if let error {
                        let style = paragraph(indent: indent, spacing: 10, blocks: outer)
                        // 誤りの位置を ^ で指すので等幅で。行は段落を分けずに改行する。
                        out.append(NSAttributedString(string: error.replacingOccurrences(of: "\n", with: "\u{2028}"), attributes: [
                            .font: NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular), .foregroundColor: NSColor.systemRed,
                            .paragraphStyle: style,
                        ]))
                        out.append(newline(style))
                    }
                    continue
                }
                // 数式のブロックは中央に。列より広いものは縮める（MarkdownView.fitAttachments）。
                let style = paragraph(indent: indent, spacing: 12, blocks: outer)
                style.alignment = .center
                style.paragraphSpacingBefore = 4
                let picture = FormulaPicture.image(formula.svg, size: FormulaPicture.metrics(formula, fontSize: bodySize).size)
                let text = NSMutableAttributedString(attachment: FittingAttachment(image: picture))
                text.addAttributes([.paragraphStyle: style, .toolTip: code], range: NSRange(location: 0, length: text.length))
                out.append(text)
                out.append(newline(style))
            case .quote(let alert, let inner):
                let (color, title) = Self.alert(alert)
                let quote = quoteBlock(color)
                if let title {
                    let style = paragraph(spacing: 4, blocks: outer + [quote])
                    out.append(NSAttributedString(string: title, attributes: [
                        .font: NSFont.boldSystemFont(ofSize: bodySize), .foregroundColor: color, .paragraphStyle: style,
                    ]))
                    out.append(newline(style))
                }
                let before = out.length
                add(inner, depth: depth, outer: outer + [quote], indent: 0)
                if alert == nil {
                    out.addAttribute(.foregroundColor, value: NSColor.secondaryLabelColor, range: NSRange(location: before, length: out.length - before))
                }
            case .list(let start, let items):
                let step: CGFloat = 22
                for (i, item) in items.enumerated() {
                    let marker: String
                    if let task = item.task {
                        marker = task ? "☑" : "☐"
                    } else if let start {
                        marker = "\(start + i)."
                    } else {
                        marker = ["•", "◦", "▪︎"][depth % 3]
                    }
                    let before = out.length
                    add(item.blocks, depth: depth + 1, outer: outer, indent: indent + step)
                    // 項目の最初の段落の頭に印を置く（印はぶら下げる）。
                    let style = (out.attribute(.paragraphStyle, at: min(before, max(out.length - 1, 0)), effectiveRange: nil)
                        as? NSParagraphStyle)?.mutableCopy() as? NSMutableParagraphStyle ?? paragraph(indent: indent + step)
                    style.firstLineHeadIndent = indent
                    style.tabStops = [NSTextTab(textAlignment: .left, location: indent + step)]
                    style.paragraphSpacing = 4
                    let firstEnd = (out.string as NSString).range(of: "\n", range: NSRange(location: before, length: out.length - before)).location
                    if firstEnd != NSNotFound {
                        out.addAttribute(.paragraphStyle, value: style, range: NSRange(location: before, length: firstEnd - before + 1))
                    }
                    out.insert(NSAttributedString(string: marker + "\t", attributes: [
                        .font: NSFont.systemFont(ofSize: bodySize), .foregroundColor: NSColor.secondaryLabelColor, .paragraphStyle: style,
                    ]), at: before)
                }
            case .table(let aligns, let head, let rows):
                let table = NSTextTable()
                table.numberOfColumns = max(head.count, rows.map(\.count).max() ?? 0)
                table.setValue(100, type: .percentageValueType, for: .width)
                table.collapsesBorders = true
                table.hidesEmptyCells = false
                for (r, row) in ([head] + rows).enumerated() {
                    for column in 0..<table.numberOfColumns {
                        let cell = NSTextTableBlock(table: table, startingRow: r, rowSpan: 1, startingColumn: column, columnSpan: 1)
                        cell.setWidth(1, type: .absoluteValueType, for: .border)
                        cell.setBorderColor(NSColor.separatorColor)
                        cell.setWidth(6, type: .absoluteValueType, for: .padding)
                        if r == 0 { cell.backgroundColor = NSColor.quaternaryLabelColor.withAlphaComponent(0.12) }
                        let style = paragraph(spacing: 0, blocks: outer + [cell])
                        style.alignment = switch column < aligns.count ? aligns[column] : "none" {
                        case "center": .center
                        case "right": .right
                        default: .left
                        }
                        out.append(runs(
                            column < row.count ? row[column] : [], size: bodySize - 1, bold: r == 0, style: style,
                            share: 1 / CGFloat(table.numberOfColumns)))
                        out.append(newline(style))
                    }
                }
                let after = paragraph(spacing: 10, blocks: outer)
                out.append(NSAttributedString(string: "\n", attributes: [.paragraphStyle: after, .font: NSFont.systemFont(ofSize: 4)]))
            case .rule:
                let line = NSTextBlock()
                line.setValue(100, type: .percentageValueType, for: .width)
                line.setWidth(1, type: .absoluteValueType, for: .border, edge: .maxY)
                line.setBorderColor(NSColor.separatorColor, for: .maxY)
                let style = paragraph(spacing: 12, blocks: outer + [line])
                out.append(NSAttributedString(string: " ", attributes: [.paragraphStyle: style, .font: NSFont.systemFont(ofSize: 4)]))
                out.append(newline(style))
            case .details(let summary, _, let inner):
                let index = detailsIndex
                detailsIndex += 1
                let isOpen = openDetails.contains(index)
                let style = paragraph(indent: indent, spacing: isOpen ? 6 : 10, blocks: outer)
                // 開閉の三角と見出し。押すと開く・閉じる（見出しの中のリンクはそのまま）。
                let line = NSMutableAttributedString(string: (isOpen ? "▼" : "▶") + "\u{2009}\u{2009}", attributes: [
                    .font: NSFont.systemFont(ofSize: bodySize * 0.7), .foregroundColor: NSColor.secondaryLabelColor,
                    .paragraphStyle: style, .baselineOffset: 1.5,
                ])
                line.append(summary.isEmpty
                    ? NSAttributedString(string: Self.detailsTitle, attributes: [
                        .font: NSFont.systemFont(ofSize: bodySize), .foregroundColor: NSColor.labelColor, .paragraphStyle: style,
                    ])
                    : runs(summary, size: bodySize, style: style))
                let whole = NSRange(location: 0, length: line.length)
                line.enumerateAttribute(.link, in: whole) { value, range, _ in
                    if value == nil { line.addAttribute(.link, value: "\(Self.detailsLink)\(index)", range: range) }
                }
                out.append(line)
                out.append(newline(style))
                if isOpen {
                    add(inner, depth: depth, outer: outer, indent: indent)
                } else {
                    detailsIndex += Self.countDetails(inner)
                }
            case .htmlTable(let columns, _, let cells):
                let table = NSTextTable()
                table.numberOfColumns = max(columns, 1)
                table.setValue(100, type: .percentageValueType, for: .width)
                table.collapsesBorders = true
                table.hidesEmptyCells = false
                for cell in cells {
                    let block = NSTextTableBlock(
                        table: table, startingRow: cell.row, rowSpan: max(cell.rowspan, 1), startingColumn: cell.column,
                        columnSpan: max(cell.colspan, 1))
                    block.setWidth(1, type: .absoluteValueType, for: .border)
                    block.setBorderColor(NSColor.separatorColor)
                    block.setWidth(6, type: .absoluteValueType, for: .padding)
                    let header = cell.header == true
                    if header { block.backgroundColor = NSColor.quaternaryLabelColor.withAlphaComponent(0.12) }
                    let style = paragraph(spacing: 0, blocks: outer + [block])
                    // 見出しのマスは、決めなければ中央（ブラウザの既定）。
                    style.alignment = cell.align == nil && header ? .center : Self.alignment(cell.align)
                    out.append(runs(
                        cell.runs, size: bodySize - 1, bold: header, style: style,
                        share: CGFloat(max(cell.colspan, 1)) / CGFloat(table.numberOfColumns)))
                    out.append(newline(style))
                }
                let after = paragraph(spacing: 10, blocks: outer)
                out.append(NSAttributedString(string: "\n", attributes: [.paragraphStyle: after, .font: NSFont.systemFont(ofSize: 4)]))
            case .footnote(let label, let inner):
                let style = paragraph(indent: indent, spacing: 2, blocks: outer)
                out.append(NSAttributedString(string: "[\(label)]", attributes: [
                    .font: NSFont.systemFont(ofSize: bodySize - 2), .foregroundColor: NSColor.secondaryLabelColor, .paragraphStyle: style,
                ]))
                out.append(newline(style))
                add(inner, depth: depth, outer: outer, indent: indent + 16)
            }
        }
    }

    /// GitHub の囲みの色と見出し。
    private static func alert(_ kind: String?) -> (NSColor, String?) {
        // 鍵に前置きを付ける（「Tip」はブランチの先端の訳がすでにある）。訳がなければ前置きを落とす。
        func title(_ key: String) -> String {
            let text = L("GitHub alert: " + key)
            return text.hasPrefix("GitHub alert: ") ? String(text.dropFirst("GitHub alert: ".count)) : text
        }
        return switch kind {
        case "note": (.systemBlue, title("Note"))
        case "tip": (.systemGreen, title("Tip"))
        case "important": (.systemPurple, title("Important"))
        case "warning": (.systemOrange, title("Warning"))
        case "caution": (.systemRed, title("Caution"))
        default: (.separatorColor, nil)
        }
    }

    /// フェンスの言葉から、構文の色付けの拡張子を決める（言葉がそのまま拡張子のものはそのまま）。
    static func fileExtension(_ language: String) -> String {
        let names: [String: String] = [
            "rust": "rs", "python": "py", "javascript": "js", "typescript": "ts", "shell": "sh", "bash": "sh", "zsh": "sh",
            "ruby": "rb", "kotlin": "kt", "markdown": "md", "yaml": "yml", "csharp": "cs", "c#": "cs", "c++": "cpp",
            "objective-c": "m", "golang": "go", "jsx": "jsx", "tsx": "tsx",
        ]
        let lower = language.lowercased()
        return names[lower] ?? lower
    }
}
