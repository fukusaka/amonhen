import AppKit

extension Notification.Name {
    /// 窓の背景の設定（アプリの設定かリポジトリごとの上書き）が変わった。
    static let backdropChanged = Notification.Name("backdropChanged")
    /// 窓に背景を当て直した（object は窓）。差分などが描き直す。
    static let backdropApplied = Notification.Name("backdropApplied")
}

/// 窓の外側の分割。サイドバーがふつうの項目のとき（macOS の既定以外の背景）も、サイドバーの開け閉めを効かせる。
final class MainSplitViewController: NSSplitViewController {
    override func toggleSidebar(_ sender: Any?) {
        guard let item = splitViewItems.first, item.behavior != .sidebar else { return super.toggleSidebar(sender) }
        item.animator().isCollapsed.toggle()
    }
}

/// 窓の背景の型。
enum BackdropStyle: String, CaseIterable, Codable, Hashable {
    /// macOS の既定のまま（サイドバーはガラス、ほかの面はそれぞれの色）。
    case system
    /// サイドバーとツールバーだけ透け、読む面（一覧・差分）は 1 つの色。
    case chrome
    /// 窓全体に地を 1 枚敷き、どの面も自分では塗らない。読む面の濃さは `contentOpacity`。
    case single

    var title: String {
        switch self {
        case .system: L("System")
        case .chrome: L("Sidebar and Toolbar Only")
        case .single: L("One Layer for the Whole Window")
        }
    }
}

/// 背景画像の置き方。
enum BackdropImagePlacement: String, CaseIterable, Codable, Hashable {
    /// 窓いっぱいに広げる（縦横の比は保ち、はみ出た分は切る）。
    case fill
    /// 元の大きさで隅に置く（窓より大きければ窓に収まるまで縮める）。
    case topLeft, topRight, bottomLeft, bottomRight

    var title: String {
        switch self {
        case .fill: L("Fill the Window")
        case .topLeft: L("Top Left")
        case .topRight: L("Top Right")
        case .bottomLeft: L("Bottom Left")
        case .bottomRight: L("Bottom Right")
        }
    }
}

/// 窓の背景の設定。アプリの設定（Settings > Theme）と、リポジトリごとの上書き（ブックマーク）で同じ形。
struct BackdropSettings: Codable, Hashable {
    var style: BackdropStyle = .system
    /// 背景画像のパス（なければ窓の後ろが透ける）。
    var image: String?
    var imagePlacement = BackdropImagePlacement.fill
    /// 1 枚の地の濃さ（0.4〜1）。1 なら背景画像も窓の後ろも見えない。
    var contentOpacity = 0.8
    /// ツールバーに敷く色（`#rrggbb`、なければ地と同じ）。
    var toolbarColor: String?
    var toolbarOpacity = 0.35

    init() {}

    /// 後から足した項目がない記録（古いブックマーク）も読めるように、なければ既定の値。
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = BackdropSettings()
        style = try c.decodeIfPresent(BackdropStyle.self, forKey: .style) ?? d.style
        image = try c.decodeIfPresent(String.self, forKey: .image)
        imagePlacement = try c.decodeIfPresent(BackdropImagePlacement.self, forKey: .imagePlacement) ?? d.imagePlacement
        contentOpacity = try c.decodeIfPresent(Double.self, forKey: .contentOpacity) ?? d.contentOpacity
        toolbarColor = try c.decodeIfPresent(String.self, forKey: .toolbarColor)
        toolbarOpacity = try c.decodeIfPresent(Double.self, forKey: .toolbarOpacity) ?? d.toolbarOpacity
    }

    /// アプリの設定。`-backdropStyle single` のように引数でも渡せる（`--snapshot` で比べるため）。
    static var app: BackdropSettings {
        get {
            let d = UserDefaults.standard
            var s = BackdropSettings()
            s.style = d.string(forKey: "backdropStyle").flatMap(BackdropStyle.init(rawValue:)) ?? .system
            s.image = d.string(forKey: "backdropImage").flatMap { $0.isEmpty ? nil : $0 }
            s.imagePlacement = d.string(forKey: "backdropImagePlacement").flatMap(BackdropImagePlacement.init(rawValue:)) ?? .fill
            if d.object(forKey: "backdropContentOpacity") != nil { s.contentOpacity = d.double(forKey: "backdropContentOpacity") }
            s.toolbarColor = d.string(forKey: "backdropToolbarColor").flatMap { $0.isEmpty ? nil : $0 }
            if d.object(forKey: "backdropToolbarOpacity") != nil { s.toolbarOpacity = d.double(forKey: "backdropToolbarOpacity") }
            return s
        }
        set {
            let d = UserDefaults.standard
            d.set(newValue.style.rawValue, forKey: "backdropStyle")
            d.set(newValue.image, forKey: "backdropImage")
            d.set(newValue.imagePlacement.rawValue, forKey: "backdropImagePlacement")
            d.set(newValue.contentOpacity, forKey: "backdropContentOpacity")
            d.set(newValue.toolbarColor, forKey: "backdropToolbarColor")
            d.set(newValue.toolbarOpacity, forKey: "backdropToolbarOpacity")
            NotificationCenter.default.post(name: .backdropChanged, object: nil)
        }
    }

    /// 読む面に敷く色（`chrome` は不透明、`single` は塗らない、`system` は面ごとの色のまま）。
    var paneColor: NSColor? {
        switch style {
        case .system: nil
        case .chrome: .textBackgroundColor
        case .single: .clear
        }
    }

    /// 差分の地（Metal で塗る色）。
    var diffBackground: SIMD4<Float> {
        style == .single ? .zero : NSColor.textBackgroundColor.rgba
    }

    /// 差分のファイル名の帯。`single` では地と同じ濃さをもう 1 枚重ねる（上に留まった帯が下の行を隠す）。
    var bandColor: SIMD4<Float> {
        switch style {
        case .system: NSColor.windowBackgroundColor.rgba
        case .chrome: (NSColor.textBackgroundColor.blended(withFraction: 0.05, of: .labelColor) ?? .textBackgroundColor).rgba
        case .single: NSColor.textBackgroundColor.withAlphaComponent(contentOpacity).rgba
        }
    }

    /// 差分の中の小さな枠（チェックボックス）の中の色。
    var boxColor: SIMD4<Float> {
        style == .system ? NSColor.windowBackgroundColor.rgba : NSColor.textBackgroundColor.rgba
    }

    /// 窓ごとの設定（差分などが自分の窓から引く）。
    @MainActor private static var byWindow: [ObjectIdentifier: BackdropSettings] = [:]

    @MainActor static func of(_ window: NSWindow?) -> BackdropSettings {
        window.flatMap { byWindow[ObjectIdentifier($0)] } ?? BackdropSettings()
    }

    @MainActor static func set(_ settings: BackdropSettings?, for window: NSWindow) {
        byWindow[ObjectIdentifier(window)] = settings
    }
}

/// 画像を縦横比を保って枠いっぱいに敷く（はみ出た分は切る）。隅に置くときは、枠を画像の大きさにしてある。
private final class FillImageView: NSView {
    var image: NSImage? {
        didSet { layer?.contents = image }
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.contentsGravity = .resizeAspectFill
        layer?.masksToBounds = true
    }

    required init?(coder: NSCoder) { fatalError() }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

/// 色を塗るだけのビュー（マウスは下に通す）。層の色で塗る（外観が変わったら塗り直す）。
private final class TintView: NSView {
    var color: NSColor = .clear {
        didSet { paint() }
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
    }

    required init?(coder: NSCoder) { fatalError() }

    private func paint() {
        effectiveAppearance.performAsCurrentDrawingAppearance { layer?.backgroundColor = color.cgColor }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

/// 窓の中身の土台: 背景の層（窓の後ろ・画像・地・ツールバーの帯）を敷き、その上に中身（分割ビュー）を載せる。
/// `system` 以外では窓をタイトルバーの下まで広げ、ツールバーの下にも同じ層が見えるようにする。
final class BackdropHostController: NSViewController {
    let content: NSViewController
    private let behind = NSVisualEffectView()
    private let image = FillImageView()
    private let ground = TintView()
    private let band = TintView()
    private(set) var settings = BackdropSettings()
    private var loadedImage: (path: String, image: NSImage?)?

    init(content: NSViewController) {
        self.content = content
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let root = LayoutView(frame: NSRect(x: 0, y: 0, width: 1440, height: 820))
        root.onLayout = { [weak self] in self?.layoutContent() }
        behind.blendingMode = .behindWindow
        behind.material = .underWindowBackground
        behind.state = .followsWindowActiveState
        for v in [behind, image, ground, band] as [NSView] {
            v.isHidden = true
            root.addSubview(v)
        }
        addChild(content)
        root.addSubview(content.view)
        view = root
    }

    /// ツールバー（タイトルバー）の高さ。`system` では中身がその下から始まるので 0。
    private var titlebarHeight: CGFloat {
        guard let window = view.window, settings.style != .system else { return 0 }
        return max(0, view.bounds.height - window.contentLayoutRect.maxY)
    }

    private func layoutContent() {
        let bounds = view.bounds
        let top = titlebarHeight
        for v in [behind, ground] as [NSView] { v.frame = bounds }
        image.frame = Self.frame(of: image.image, placement: settings.imagePlacement, in: bounds)
        band.frame = NSRect(x: 0, y: bounds.height - top, width: bounds.width, height: top)
        content.view.frame = NSRect(x: 0, y: 0, width: bounds.width, height: bounds.height - top)
    }

    func apply(_ settings: BackdropSettings) {
        self.settings = settings
        guard let window = view.window else { return }
        let custom = settings.style != .system
        let picture = custom ? settings.image.flatMap(loadImage) : nil
        Backdrop.prepare(window: window, style: settings.style, seeThrough: custom && picture == nil)
        image.image = picture
        image.isHidden = picture == nil
        behind.isHidden = !custom || picture != nil
        behind.material = settings.style == .chrome ? .sidebar : .underWindowBackground
        ground.isHidden = settings.style != .single
        ground.color = NSColor.textBackgroundColor.withAlphaComponent(settings.contentOpacity)
        let tint = custom ? settings.toolbarColor.flatMap(NSColor.init(hex:)) : nil
        band.isHidden = tint == nil
        band.color = tint?.withAlphaComponent(settings.toolbarOpacity) ?? .clear
        view.needsLayout = true
        layoutContent()
    }

    /// 背景画像の枠。隅に置くときは元の大きさ（点）で、窓より大きければ比を保って縮める。
    static func frame(of picture: NSImage?, placement: BackdropImagePlacement, in bounds: NSRect) -> NSRect {
        guard let picture, placement != .fill, picture.size.width > 0, picture.size.height > 0 else { return bounds }
        let scale = min(1, bounds.width / picture.size.width, bounds.height / picture.size.height)
        let size = NSSize(width: (picture.size.width * scale).rounded(), height: (picture.size.height * scale).rounded())
        let x = placement == .topLeft || placement == .bottomLeft ? bounds.minX : bounds.maxX - size.width
        let y = placement == .bottomLeft || placement == .bottomRight ? bounds.minY : bounds.maxY - size.height
        return NSRect(origin: NSPoint(x: x, y: y), size: size)
    }

    private func loadImage(_ path: String) -> NSImage? {
        if let loadedImage, loadedImage.path == path { return loadedImage.image }
        let picture = NSImage(contentsOfFile: (path as NSString).expandingTildeInPath)
        loadedImage = (path, picture)
        return picture
    }
}

/// 窓と面の背景を、型に合わせて切り替える。
@MainActor enum Backdrop {
    /// 窓の形: `system` 以外は中身をタイトルバーの下まで広げる。窓を透けるようにするのは、背景画像がなく窓の後ろを
    /// 見せるときだけ（透ける窓は WindowServer が重ねるので、表示までが 1 フレームの半分ほど遅れる。計測済み）。
    static func prepare(window: NSWindow, style: BackdropStyle, seeThrough: Bool) {
        let custom = style != .system
        if window.styleMask.contains(.fullSizeContentView) != custom {
            // 切り替えると AppKit は中身の大きさを保って窓を縮める・伸ばすので、窓の枠を戻す。
            let frame = window.frame
            if custom {
                window.styleMask.insert(.fullSizeContentView)
            } else {
                window.styleMask.remove(.fullSizeContentView)
            }
            window.setFrame(frame, display: true)
        }
        window.titlebarAppearsTransparent = custom
        window.isOpaque = !seeThrough
        window.backgroundColor = seeThrough ? .clear : .windowBackgroundColor
    }

    /// 面の中の背景（表・一覧・文字の欄・スクロール）を消す・戻す。消す前の値は覚えておき、`system` に戻すときに戻す。
    static func clearBackgrounds(in root: NSView, clear: Bool) {
        func visit(_ view: NSView) {
            let key = ObjectIdentifier(view)
            switch view {
            case let scroll as NSScrollView:
                remember(key, scroll) { [weak scroll, drawsBackground = scroll.drawsBackground] in scroll?.drawsBackground = drawsBackground }
                if clear { scroll.drawsBackground = false }
            case let clip as NSClipView:
                remember(key, clip) { [weak clip, drawsBackground = clip.drawsBackground] in clip?.drawsBackground = drawsBackground }
                if clear { clip.drawsBackground = false }
            case let table as NSTableView:
                remember(key, table) { [weak table, color = table.backgroundColor] in table?.backgroundColor = color }
                if clear { table.backgroundColor = .clear }
            case let text as NSTextView:
                remember(key, text) { [weak text, drawsBackground = text.drawsBackground] in text?.drawsBackground = drawsBackground }
                if clear { text.drawsBackground = false }
            default:
                break
            }
            if !clear, let known = originals.removeValue(forKey: key), known.view === view { known.restore() }
            view.subviews.forEach(visit)
        }
        visit(root)
    }

    private final class Original {
        weak var view: NSView?
        let restore: () -> Void
        init(_ view: NSView, _ restore: @escaping () -> Void) {
            self.view = view
            self.restore = restore
        }
    }

    private static var originals: [ObjectIdentifier: Original] = [:]

    private static func remember(_ key: ObjectIdentifier, _ view: NSView, restore: @escaping () -> Void) {
        // 覚えるのは最初の 1 回だけ（消した後の値で上書きしない）。ビューが消えて番号が使い回されたら覚え直す。
        if let known = originals[key], known.view === view { return }
        originals[key] = Original(view, restore)
    }
}
