import AmonhenKit
import AppKit

/// 差分の帯の、行を比べるときに無視する空白を選ぶボタン（History の右の列と Working Copy）。帯を狭くしないように
/// アイコンだけで、押すと 4 つの選び方を出す（今のものに印）。無視している間はアイコンをアクセントの色にする。
/// 選んだものは覚えない（開くたびに「空白を区別」から）。
final class WhitespacePopUp: NSPopUpButton {
    /// 選び直したとき。
    var onChange: ((DiffWhitespace) -> Void)?

    var whitespace: DiffWhitespace = WhitespacePopUp.initial {
        didSet { update() }
    }

    init() {
        super.init(frame: .zero, pullsDown: true)
        bezelStyle = .accessoryBarAction
        controlSize = .small
        isBordered = false
        let first = NSMenuItem()
        first.title = ""
        first.image = AppIcons.image(symbol: "space", label: L("Whitespace"))
            ?? AppIcons.image(symbol: "textformat", label: L("Whitespace"))
        menu?.addItem(first)
        for mode in DiffWhitespace.allCases {
            let item = NSMenuItem(title: Self.title(mode), action: #selector(choose(_:)), keyEquivalent: "")
            item.target = self
            item.tag = Int(mode.rawValue)
            item.toolTip = Self.toolTip(mode)
            menu?.addItem(item)
        }
        update()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    @objc private func choose(_ item: NSMenuItem) {
        guard let mode = DiffWhitespace(rawValue: UInt8(clamping: item.tag)), mode != whitespace else { return }
        whitespace = mode
        onChange?(mode)
    }

    /// 印・アイコンの色・ツールチップ（今の選び方を言う）。
    private func update() {
        for item in menu?.items.dropFirst() ?? [] {
            item.state = item.tag == Int(whitespace.rawValue) ? .on : .off
        }
        contentTintColor = whitespace == .exact ? nil : .controlAccentColor
        toolTip = "\(Self.title(whitespace))\n\(L("Whitespace to ignore when comparing lines (as git diff does)"))"
    }

    /// 最初に選んでおくもの。ふだんは「空白を区別」で、確かめる用（`--snapshot`）に `-AMHWhitespace 0〜3` で変えられる。
    static var initial: DiffWhitespace {
        DiffWhitespace(rawValue: UInt8(clamping: UserDefaults.standard.integer(forKey: "AMHWhitespace"))) ?? .exact
    }

    static func title(_ mode: DiffWhitespace) -> String {
        switch mode {
        case .exact: L("Compare Whitespace")
        case .atEol: L("Ignore Whitespace at Line Ends")
        case .change: L("Ignore Changes in Whitespace")
        case .all: L("Ignore All Whitespace")
        }
    }

    static func toolTip(_ mode: DiffWhitespace) -> String {
        switch mode {
        case .exact: L("Lines that differ only in whitespace are changes")
        case .atEol: L("Ignore whitespace at the end of lines (git diff --ignore-space-at-eol)")
        case .change: L("Ignore changes in the amount of whitespace (git diff -b)")
        case .all: L("Ignore all whitespace when comparing lines (git diff -w)")
        }
    }
}

/// 差分の帯の「長い行を折り返す」ボタン（History の右の列と Working Copy）。アイコンだけで、押すたびに切り替える。
/// 設定はアプリ全体で 1 つ（表示 > 長い行を折り返す、⌥Z と同じ）。オンの間はアイコンをアクセントの色にする。
final class WrapLinesButton: NSButton {
    init() {
        super.init(frame: .zero)
        bezelStyle = .accessoryBarAction
        controlSize = .small
        isBordered = false
        image = AppIcons.image(symbol: "arrow.turn.down.left", label: L("Wrap Long Lines"))
        imagePosition = .imageOnly
        target = self
        action = #selector(toggle)
        update()
        NotificationCenter.default.addObserver(self, selector: #selector(update), name: .diffWrapLinesChanged, object: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    @objc private func toggle() { Settings.wrapDiffLines.toggle() }

    @objc private func update() {
        let on = Settings.wrapDiffLines
        contentTintColor = on ? .controlAccentColor : nil
        toolTip = (on ? L("Wrapping long lines (⌥Z)") : L("Wrap Long Lines (⌥Z)")) + "\n"
            + L("Wrap lines wider than the diff instead of scrolling sideways (in every diff)")
    }
}

/// 帯の右端の「表示の設定」（スライダーのアイコンだけのボタン）。押すたびに `fill` で項目を作り直す（今の設定の印を付ける）。
final class ViewOptionsPopUp: NSPopUpButton, NSMenuDelegate {
    private let fill: (NSMenu) -> Void

    init(fill: @escaping (NSMenu) -> Void) {
        self.fill = fill
        super.init(frame: .zero, pullsDown: true)
        bezelStyle = .accessoryBarAction
        controlSize = .small
        isBordered = false
        toolTip = L("View Options")
        menu?.delegate = self
        menu?.addItem(Self.iconItem())
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        menu.addItem(Self.iconItem())
        fill(menu)
    }

    private static func iconItem() -> NSMenuItem {
        let first = NSMenuItem()
        first.image = AppIcons.image(symbol: "slider.horizontal.3", label: L("View Options"))
        first.title = ""
        return first
    }
}
