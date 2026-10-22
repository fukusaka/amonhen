import AmonhenKit
import AppKit

/// つなぐ必要のあるリポジトリ（daemon 越し・SSH 先）のタブ。開いてすぐ、リポジトリが入る場所に置いておき、つないで
/// いる間はそのことを、つなげなかったらエラーと「もう一度」を出す。つながったら AppDelegate がリポジトリのタブに
/// 替える（タブとウィンドウの並びが、つながった順に左右されない。docs/ux.md の「1. Multiple repositories」）。
/// 起動時に開けなかった手元のリポジトリのタブにも使う。
final class ConnectingTab: NSObject, NSWindowDelegate {
    let location: String
    let window: NSWindow
    /// リポジトリのタブと同じツールバー（タブを切り替えてもタブの並びの位置が動かないように）。
    private let toolbar = MainToolbar()
    /// 閉じられた（取り消された）。つないだ結果は捨てる。
    private(set) var isClosed = false
    /// 「もう一度」。
    var onRetry: (() -> Void)?
    /// 閉じられた（AppDelegate が手放す）。リポジトリのタブに替わったとき（`finish`）は呼ばない。
    var onClose: (() -> Void)?

    init(location: String, tabWith: NSWindow?) {
        self.location = location
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1440, height: 820),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init()
        window.title = Bookmarks.shared.displayName(location)
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.toolbar = toolbar.toolbar
        toolbar.showUnopened(window.title)
        // 位置と大きさはリポジトリのウィンドウのものを借りる（記録はしない。同じ名前で記録できるのは 1 つだけで、
        // 替わるリポジトリのウィンドウが記録する）。
        window.setContentSize(NSSize(width: 1440, height: 820))
        if !window.setFrameUsingName("Repository \(location)") { window.center() }
        window.tabbingIdentifier = RepositoryWindow.tabbingIdentifier
        if let tabWith {
            tabWith.addTabbedWindow(window, ordered: .above)
            window.makeKeyAndOrderFront(nil)
        } else {
            window.tabbingMode = .disallowed
            window.makeKeyAndOrderFront(nil)
            window.tabbingMode = .preferred
        }
    }

    private var host: String { RepositoryLocation.host(location) ?? location }

    /// つないでいる間（SSH の接続と daemon の送り込みに数秒かかる）。
    func showConnecting() {
        let spinner = NSProgressIndicator()
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.startAnimation(nil)
        let title = NSTextField(labelWithString: L("Connecting to %@…", host))
        title.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        let heading = NSStackView(views: [spinner, title])
        heading.spacing = 8
        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(close))
        cancel.keyEquivalent = "\u{1b}"
        show([heading, pathLabel(), cancel])
    }

    /// つなげなかった（開けなかった）。タブは残し、閉じるまで次の起動でも開き直す。
    func showError(_ error: Error) {
        let icon = NSImageView(image: NSImage(systemSymbolName: "exclamationmark.triangle", accessibilityDescription: nil) ?? NSImage())
        icon.symbolConfiguration = .init(pointSize: 28, weight: .regular)
        icon.contentTintColor = .secondaryLabelColor
        let title = NSTextField(labelWithString: RepositoryLocation.isRemote(location)
            ? L("Couldn’t connect to %@", host) : L("Couldn’t open %@", window.title))
        title.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        let detail = NSTextField(wrappingLabelWithString: String(describing: error))
        detail.textColor = .secondaryLabelColor
        detail.isSelectable = true
        detail.alignment = .center
        detail.preferredMaxLayoutWidth = 520
        let close = NSButton(title: L("Close Tab"), target: self, action: #selector(close))
        let retry = NSButton(title: L("Try Again"), target: self, action: #selector(retry))
        retry.keyEquivalent = "\r"
        let buttons = NSStackView(views: [close, retry])
        show([icon, title, pathLabel(), detail, buttons])
    }

    private func pathLabel() -> NSTextField {
        let path = NSTextField(labelWithString: RepositoryLocation.path(location))
        path.textColor = .secondaryLabelColor
        path.lineBreakMode = .byTruncatingMiddle
        return path
    }

    /// `views` を縦に並べて真ん中に置く。
    private func show(_ views: [NSView]) {
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .centerX
        stack.spacing = 10
        stack.translatesAutoresizingMaskIntoConstraints = false
        let content = NSView()
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: content.centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: content.centerYAnchor),
            stack.widthAnchor.constraint(lessThanOrEqualToConstant: 560),
            stack.leadingAnchor.constraint(greaterThanOrEqualTo: content.leadingAnchor, constant: 20),
        ])
        window.contentView = content
    }

    @objc private func close() {
        window.close()
    }

    @objc private func retry() {
        onRetry?()
    }

    /// リポジトリのタブに替わった: 閉じるが、取り消しにはしない。
    func finish() {
        window.delegate = nil
        window.close()
    }

    func windowWillClose(_ notification: Notification) {
        isClosed = true
        onClose?()
    }
}
