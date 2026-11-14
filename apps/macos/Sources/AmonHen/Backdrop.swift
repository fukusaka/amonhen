import AppKit

extension Notification.Name {
    /// 窓の背景の設定（アプリの設定かリポジトリごとの上書き）が変わった。
    static let backdropChanged = Notification.Name("backdropChanged")
    /// 窓に背景を当て直した（object は窓）。差分などが描き直す。
    static let backdropApplied = Notification.Name("backdropApplied")
}

/// 窓の外側の分割。サイドバーがふつうの項目のとき（窓の後ろを透かすとき）も、サイドバーの開け閉めを効かせる。
final class MainSplitViewController: NSSplitViewController {
    override func toggleSidebar(_ sender: Any?) {
        guard let item = splitViewItems.first, item.behavior != .sidebar else { return super.toggleSidebar(sender) }
        item.animator().isCollapsed.toggle()
    }
}

/// 窓の後ろ（デスクトップや後ろの窓）を透かす場所。
enum BackdropStyle: String, CaseIterable, Codable, Hashable {
    /// どこも透かさない。macOS の既定の窓（サイドバーはガラス、ツールバーは標準）。読む面（一覧・差分・Commit の欄）は 1 つの色。
    case system
    /// サイドバーとツールバーだけ透かし、読む面（一覧・差分）は不透明な 1 つの色。
    case chrome
    /// 窓全体を透かす。どの面も自分では塗らず、窓全体に敷いた 1 枚の地が透け具合を決める。
    case single

    var title: String {
        switch self {
        case .system: L("Nowhere (Standard)")
        case .chrome: L("Sidebar and Toolbar")
        case .single: L("Whole Window")
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
    /// 窓の後ろを透かす場所。
    var style: BackdropStyle = .system
    /// 透かす場所の透過率（0.1〜0.9）。地を `1 - transparency` の濃さで敷く。
    var transparency = 0.4
    /// 透かした先（デスクトップや後ろの窓）のぼかしの強さ（0〜1。0 はぼかさない）。
    var blur = 0.4
    /// 背景画像のパス。透かす場所とは別に、窓全体の中身の上に薄く重ねる。
    var image: String?
    var imagePlacement = BackdropImagePlacement.fill
    /// 背景画像の透過率（0.5〜0.95）。
    var imageTransparency = 0.85
    /// ツールバーに敷く色（`#rrggbb`、なければ地と同じ）。
    var toolbarColor: String?
    var toolbarOpacity = 0.35

    init() {}

    /// 後から足した項目がない記録（古いブックマーク）も読めるように、なければ既定の値。
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = BackdropSettings()
        style = try c.decodeIfPresent(BackdropStyle.self, forKey: .style) ?? d.style
        transparency = try c.decodeIfPresent(Double.self, forKey: .transparency) ?? d.transparency
        blur = try c.decodeIfPresent(Double.self, forKey: .blur) ?? d.blur
        image = try c.decodeIfPresent(String.self, forKey: .image)
        imagePlacement = try c.decodeIfPresent(BackdropImagePlacement.self, forKey: .imagePlacement) ?? d.imagePlacement
        imageTransparency = try c.decodeIfPresent(Double.self, forKey: .imageTransparency) ?? d.imageTransparency
        toolbarColor = try c.decodeIfPresent(String.self, forKey: .toolbarColor)
        toolbarOpacity = try c.decodeIfPresent(Double.self, forKey: .toolbarOpacity) ?? d.toolbarOpacity
    }

    /// アプリの設定。`-backdropStyle single` のように引数でも渡せる（`--snapshot` で比べるため）。
    static var app: BackdropSettings {
        get {
            let d = UserDefaults.standard
            var s = BackdropSettings()
            s.style = d.string(forKey: "backdropStyle").flatMap(BackdropStyle.init(rawValue:)) ?? .system
            if d.object(forKey: "backdropTransparency") != nil { s.transparency = d.double(forKey: "backdropTransparency") }
            if d.object(forKey: "backdropBlur") != nil { s.blur = d.double(forKey: "backdropBlur") }
            s.image = d.string(forKey: "backdropImage").flatMap { $0.isEmpty ? nil : $0 }
            s.imagePlacement = d.string(forKey: "backdropImagePlacement").flatMap(BackdropImagePlacement.init(rawValue:)) ?? .fill
            if d.object(forKey: "backdropImageTransparency") != nil { s.imageTransparency = d.double(forKey: "backdropImageTransparency") }
            s.toolbarColor = d.string(forKey: "backdropToolbarColor").flatMap { $0.isEmpty ? nil : $0 }
            if d.object(forKey: "backdropToolbarOpacity") != nil { s.toolbarOpacity = d.double(forKey: "backdropToolbarOpacity") }
            return s
        }
        set {
            let d = UserDefaults.standard
            d.set(newValue.style.rawValue, forKey: "backdropStyle")
            d.set(newValue.transparency, forKey: "backdropTransparency")
            d.set(newValue.blur, forKey: "backdropBlur")
            d.set(newValue.image, forKey: "backdropImage")
            d.set(newValue.imagePlacement.rawValue, forKey: "backdropImagePlacement")
            d.set(newValue.imageTransparency, forKey: "backdropImageTransparency")
            d.set(newValue.toolbarColor, forKey: "backdropToolbarColor")
            d.set(newValue.toolbarOpacity, forKey: "backdropToolbarOpacity")
            NotificationCenter.default.post(name: .backdropChanged, object: nil)
        }
    }

    /// `location` の窓で使う設定（リポジトリの上書きがあればそれ）。
    static func resolved(for location: String) -> BackdropSettings {
        Bookmarks.shared.bookmark(location)?.backdrop ?? app
    }

    /// 透かす場所に敷く地の色（`1 - transparency` の濃さ）。ぼかしは窓そのものにかけるので、色を足さない。
    var groundColor: NSColor {
        NSColor.textBackgroundColor.withAlphaComponent(1 - transparency)
    }

    /// 窓の後ろのぼかしの半径（点）。強さ 1 で 50（kitty・WezTerm などの端末で使われる値の範囲）。
    var blurRadius: Int {
        style == .system ? 0 : Int((blur * 50).rounded())
    }

    /// 読む面に敷く色（`system`・`chrome` は不透明な 1 つの色、`single` は塗らない）。
    var paneColor: NSColor {
        style == .single ? .clear : .textBackgroundColor
    }

    /// 差分の地（Metal で塗る色）。
    var diffBackground: SIMD4<Float> {
        style == .single ? .zero : NSColor.textBackgroundColor.rgba
    }

    /// 差分のファイル名の帯。`single` では地と同じ濃さをもう 1 枚重ねる（上に留まった帯が下の行を隠す）。
    var bandColor: SIMD4<Float> {
        switch style {
        case .system, .chrome: (NSColor.textBackgroundColor.blended(withFraction: 0.05, of: .labelColor) ?? .textBackgroundColor).rgba
        case .single: groundColor.rgba
        }
    }

    /// 差分の中の小さな枠（チェックボックス）の中の色。
    var boxColor: SIMD4<Float> {
        NSColor.textBackgroundColor.rgba
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

/// 窓の中身の土台: 透かすときは地を敷き、その上に中身（分割ビュー）、さらに上に背景画像とツールバーの色を薄く重ねる
/// （マウスは下に通す）。窓はタイトルバーの下まで広げる。`system` 以外では、ツールバーの下にも地が見える。
final class BackdropHostController: NSViewController {
    let content: NSViewController
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
        ground.isHidden = true
        root.addSubview(ground)
        addChild(content)
        root.addSubview(content.view)
        for v in [image, band] as [NSView] {
            v.isHidden = true
            root.addSubview(v)
        }
        view = root
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        if let window = view.window { Backdrop.setBlur(window: window, radius: settings.blurRadius) }
    }

    /// ツールバーの色を重ねる帯の高さ（ツールバーの高さ）。
    private var toolbarHeight: CGFloat {
        guard let window = view.window else { return 0 }
        return max(0, view.bounds.height - window.contentLayoutRect.maxY)
    }

    private func layoutContent() {
        let bounds = view.bounds
        ground.frame = bounds
        image.frame = Self.frame(of: image.image, placement: settings.imagePlacement, in: bounds)
        let bandHeight = toolbarHeight
        band.frame = NSRect(x: 0, y: bounds.height - bandHeight, width: bounds.width, height: bandHeight)
        // 中身は窓の上端から（中の面が自分でツールバーを避ける）。
        content.view.frame = bounds
    }

    func apply(_ settings: BackdropSettings) {
        self.settings = settings
        guard let window = view.window else { return }
        let custom = settings.style != .system
        Backdrop.prepare(window: window, seeThrough: custom)
        Backdrop.setBlur(window: window, radius: settings.blurRadius)
        ground.isHidden = !custom
        ground.color = settings.groundColor
        let picture = settings.image.flatMap(loadImage)
        image.image = picture
        image.isHidden = picture == nil
        image.alphaValue = 1 - settings.imageTransparency
        let tint = settings.toolbarColor.flatMap(NSColor.init(hex:))
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
    /// 窓の形: どの型も macOS の標準の窓と同じく、中身をタイトルバーの下まで広げ、サイドバーが窓の上端まで伸び、
    /// ツールバーは中身の上に乗る（窓のタイトルは出さない。リポジトリの名前はツールバーにある）。型が変えるのは透け方だけ。
    /// 窓を透けるようにするのは、窓の後ろを透かすときだけ（透ける窓は WindowServer が重ねるので、表示までが 1 フレームの
    /// 半分ほど遅れる。計測済み）。
    static func prepare(window: NSWindow, seeThrough: Bool) {
        if !window.styleMask.contains(.fullSizeContentView) {
            // 切り替えると AppKit は中身の大きさを保って窓を縮めるので、窓の枠を戻す。
            let frame = window.frame
            window.styleMask.insert(.fullSizeContentView)
            window.setFrame(frame, display: true)
        }
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isOpaque = !seeThrough
        window.backgroundColor = seeThrough ? .clear : .windowBackgroundColor
    }

    private typealias SetBlurRadius = @convention(c) (UInt32, Int, Int) -> Int32
    private typealias DefaultConnection = @convention(c) () -> UInt32
    /// 窓の後ろをぼかす WindowServer の関数（非公開。iTerm2・kitty・WezTerm なども使う）。公開の NSVisualEffectView は
    /// ぼかしに素材の色を必ずかぶせ、ぼかしだけを強めたり、色を抜いて透かしたりできない。見つからない版では、ぼかさずに透かす。
    private static let blurFunctions: (DefaultConnection, SetBlurRadius)? = {
        guard let handle = dlopen(nil, RTLD_NOW),
              let connection = dlsym(handle, "CGSDefaultConnectionForThread"),
              let setRadius = dlsym(handle, "CGSSetWindowBackgroundBlurRadius") else { return nil }
        return (unsafeBitCast(connection, to: DefaultConnection.self), unsafeBitCast(setRadius, to: SetBlurRadius.self))
    }()

    /// 窓の後ろのぼかしの半径を決める（0 でぼかさない）。窓が画面に出る前は番号がないので、出たときにもう一度呼ぶ。
    static func setBlur(window: NSWindow, radius: Int) {
        guard window.windowNumber > 0, let (connection, setRadius) = blurFunctions else { return }
        _ = setRadius(connection(), window.windowNumber, radius)
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
