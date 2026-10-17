import AmonhenKit
import AppKit

/// daemon 越しのリポジトリにつないでいる間に出す小さなウィンドウ（SSH の接続と daemon の送り込みに数秒かかる）。
/// つないでいる間もアプリは動き、取り消すとつないだ結果を捨てる。
final class ConnectingWindow: NSObject, NSWindowDelegate {
    let location: String
    let window: NSWindow
    /// 取り消されたか、閉じられた。
    private(set) var isCancelled = false
    var onCancel: (() -> Void)?

    init(location: String) {
        self.location = location
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 110),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        super.init()
        window.title = L("Connecting")
        window.isReleasedWhenClosed = false
        window.delegate = self

        let spinner = NSProgressIndicator()
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.startAnimation(nil)
        let host = RepositoryLocation.host(location) ?? location
        let title = NSTextField(labelWithString: L("Connecting to %@…", host))
        title.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        let detail = NSTextField(labelWithString: RepositoryLocation.path(location))
        detail.textColor = .secondaryLabelColor
        detail.lineBreakMode = .byTruncatingMiddle
        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(cancel))
        cancel.keyEquivalent = "\u{1b}"

        let text = NSStackView(views: [title, detail])
        text.orientation = .vertical
        text.alignment = .leading
        text.spacing = 4
        let top = NSStackView(views: [spinner, text])
        top.alignment = .centerY
        top.spacing = 12
        let stack = NSStackView(views: [top, cancel])
        stack.orientation = .vertical
        stack.alignment = .trailing
        stack.spacing = 12
        stack.edgeInsets = NSEdgeInsets(top: 20, left: 20, bottom: 16, right: 20)
        stack.translatesAutoresizingMaskIntoConstraints = false
        let content = NSView()
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            stack.topAnchor.constraint(equalTo: content.topAnchor),
            stack.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            top.leadingAnchor.constraint(equalTo: stack.leadingAnchor, constant: 20),
            detail.widthAnchor.constraint(lessThanOrEqualToConstant: 340),
        ])
        window.contentView = content
        window.center()
        window.makeKeyAndOrderFront(nil)
    }

    @objc private func cancel() {
        window.close()
    }

    func finish() {
        window.delegate = nil
        window.close()
    }

    func windowWillClose(_ notification: Notification) {
        isCancelled = true
        onCancel?()
    }
}
