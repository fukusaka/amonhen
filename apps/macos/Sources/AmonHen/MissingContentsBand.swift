import AmonhenKit
import AppKit

/// partial clone で中身が手元にないときの帯:「N 個のファイルの中身がまだ手元にありません［取ってくる］」。読む画面は
/// 勝手に取りに行かず、押したときだけ `fetch` で取り、取れたら `done` で出し直す。次のコミットではまた聞く（取った
/// 中身は手元に残るので、同じものでは二度と出ない）。
final class MissingContentsBand: NSView {
    private let label: NSTextField
    private let button: NSButton
    private let fetch: @Sendable () throws -> Void
    private let done: @MainActor () -> Void

    /// `files` は中身が手元にないファイルの数。`fetch` は裏のスレッドで呼ぶ。
    convenience init(files: Int, fetch: @escaping @Sendable () throws -> Void, done: @escaping @MainActor () -> Void) {
        self.init(message: L("The contents of %ld files aren’t here yet (partial clone).", files), fetch: fetch, done: done)
    }

    /// 帯の文を `message` にしたもの（1 つのファイルを開いたとき）。
    init(message: String, fetch: @escaping @Sendable () throws -> Void, done: @escaping @MainActor () -> Void) {
        self.fetch = fetch
        self.done = done
        label = NSTextField(labelWithString: message)
        button = NSButton(title: L("Fetch Contents"), target: nil, action: nil)
        super.init(frame: .zero)
        let icon = NSImageView(image: AppIcons.image(symbol: "icloud.and.arrow.down") ?? NSImage())
        icon.contentTintColor = .secondaryLabelColor
        label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        label.lineBreakMode = .byTruncatingTail
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        button.controlSize = .small
        button.bezelStyle = .rounded
        button.toolTip = L("Fetch just these contents from the remote, in one go, and show them.")
        button.target = self
        button.action = #selector(fetchPressed)
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        let stack = NSStackView(views: [icon, label, spacer, button])
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 10, bottom: 0, right: 10)
        stack.translatesAutoresizingMaskIntoConstraints = false
        let line = NSBox()
        line.boxType = .separator
        line.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        addSubview(line)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
            line.leadingAnchor.constraint(equalTo: leadingAnchor),
            line.trailingAnchor.constraint(equalTo: trailingAnchor),
            line.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel(label.stringValue)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var wantsUpdateLayer: Bool { true }

    override func updateLayer() {
        layer?.backgroundColor = NSColor.systemYellow.withAlphaComponent(0.12).cgColor
    }

    @objc private func fetchPressed() {
        button.isEnabled = false
        let shown = label.stringValue
        label.stringValue = L("Fetching the contents…")
        let fetch = self.fetch
        RunningOperations.begin()
        Task {
            let result = await Task.detached(priority: .userInitiated) { Result { try fetch() } }.value
            RunningOperations.end()
            switch result {
            case .success:
                done()
            case .failure(let error):
                button.isEnabled = true
                label.stringValue = L("Couldn’t fetch the contents: %@", error.localizedDescription)
                label.toolTip = shown
            }
        }
    }
}
