import AppKit

/// ウィンドウの下のステータスバー（docs/ux.md の「Status bar」）。「今どうなっているか」を 1 か所に集める:
/// merge・rebase の途中（Continue・Skip・Abort）、失敗（原因と次の一手）、接続が切れた、終わった操作の一言。
/// 行は上から重要な順に積み、何もなければ高さ 0 で隠れる。GTK4 の FE では `GtkActionBar` が同じ役。
final class StatusBarController: NSViewController {
    /// 行の種類（上から並ぶ順）。同じ種類は 1 つだけ（出し直せば置き換える）。
    enum Row: Int, CaseIterable { case editing, sequence, failure, connection, message }

    private let content: NSViewController
    private let stack = NSStackView()
    private var rows: [Row: NSView] = [:]
    private var messageTimer: Timer?

    init(content: NSViewController) {
        self.content = content
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let view = NSView()
        addChild(content)
        stack.orientation = .vertical
        stack.alignment = .width
        stack.spacing = 0
        view.addSubview(content.view)
        view.addSubview(stack)
        content.view.autoresizingMask = []
        stack.autoresizingMask = []
        self.view = view
    }

    /// 下に行の高さだけステータスバー、残りを中身に（制約の取り合いで空きが出ないよう、自分で割り振る）。
    override func viewDidLayout() {
        super.viewDidLayout()
        let bounds = view.bounds
        stack.frame.size.width = bounds.width
        stack.layoutSubtreeIfNeeded()
        let height = stack.arrangedSubviews.isEmpty ? 0 : stack.fittingSize.height
        stack.frame = NSRect(x: 0, y: 0, width: bounds.width, height: height)
        content.view.frame = NSRect(x: 0, y: height, width: bounds.width, height: max(0, bounds.height - height))
    }

    /// `row` の行を出す（同じ種類の前の行は置き換える）。
    func show(_ row: Row, _ view: NSView) {
        hide(row)
        rows[row] = view
        let separator = NSBox()
        separator.boxType = .separator
        let wrapper = NSStackView(views: [separator, view])
        wrapper.orientation = .vertical
        wrapper.alignment = .width
        wrapper.spacing = 0
        wrapper.identifier = NSUserInterfaceItemIdentifier("status.\(row.rawValue)")
        // 決まった順に差し込む（上から sequence・failure・connection・message）。
        let index = stack.arrangedSubviews.firstIndex {
            Int($0.identifier?.rawValue.dropFirst("status.".count) ?? "") ?? 0 > row.rawValue
        } ?? stack.arrangedSubviews.count
        stack.insertArrangedSubview(wrapper, at: index)
        view.needsLayout = true
    }

    func hide(_ row: Row) {
        rows[row] = nil
        stack.arrangedSubviews.first { $0.identifier?.rawValue == "status.\(row.rawValue)" }?.removeFromSuperview()
        view.needsLayout = true
    }

    func isShown(_ row: Row) -> Bool { rows[row] != nil }

    /// 終わった操作の一言を、しばらく出して消す。
    func flash(_ text: String, symbol: String = "checkmark.circle") {
        let icon = NSImageView(image: NSImage(systemSymbolName: symbol, accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = .systemGreen
        let label = NSTextField(labelWithString: text)
        label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        label.textColor = .secondaryLabelColor
        label.lineBreakMode = .byTruncatingTail
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        show(.message, Self.row([icon, label, NSView()], height: 24))
        messageTimer?.invalidate()
        messageTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.hide(.message) }
        }
    }

    /// 行の並び（左から右）を、高さと余白をそろえて作る。`tint` は敷く色。
    static func row(_ views: [NSView], height: CGFloat, tint: NSColor? = nil) -> NSView {
        let stack = NSStackView(views: views)
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 4, right: 12)
        stack.heightAnchor.constraint(greaterThanOrEqualToConstant: height).isActive = true
        if let tint {
            stack.wantsLayer = true
            stack.layer?.backgroundColor = tint.cgColor
        }
        return stack
    }
}
