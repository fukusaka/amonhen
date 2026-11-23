import AmonhenKit
import AppKit

/// 差分の帯の、行を比べるときに無視する空白を選ぶポップアップ（History の右の列と Working Copy）。
/// 選んだものは覚えない（開くたびに「空白を区別」から）。
final class WhitespacePopUp: NSPopUpButton {
    /// 選び直したとき。
    var onChange: ((DiffWhitespace) -> Void)?

    var whitespace: DiffWhitespace {
        get { DiffWhitespace(rawValue: UInt8(max(0, indexOfSelectedItem))) ?? .exact }
        set { selectItem(at: Int(newValue.rawValue)) }
    }

    init() {
        super.init(frame: .zero, pullsDown: false)
        bezelStyle = .accessoryBarAction
        controlSize = .small
        font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        for mode in DiffWhitespace.allCases {
            let item = NSMenuItem(title: Self.title(mode), action: nil, keyEquivalent: "")
            item.toolTip = Self.toolTip(mode)
            menu?.addItem(item)
        }
        whitespace = Self.initial
        toolTip = L("Whitespace to ignore when comparing lines (as git diff does)")
        target = self
        action = #selector(changed)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    @objc private func changed() { onChange?(whitespace) }

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
