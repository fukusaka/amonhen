import AppKit

/// shallow clone の History の最後の行の下に置く帯。ここより古い履歴を取得していないことを言い、取り足す
/// ボタン（さらに取る・すべて取る）を並べる。取っている間はボタンを止めて回る印を出す。
final class ShallowHistoryBand: NSView {
    static let height: CGFloat = 44

    /// 取り足す命令（`deepen`・`unshallow`）を実行する。終わったら（失敗しても）`done` を呼んでもらう。
    var onFetch: (([String: Any], _ done: @escaping () -> Void) -> Void)?
    /// 「この日付まで…」の初期値（境界のコミットの日時。分からなければ nil）。
    var boundaryDate: (() -> Date?)?

    private let label = NSTextField(labelWithString: L("Older history isn’t fetched (shallow clone)"))
    private let moreButton = NSPopUpButton(frame: .zero, pullsDown: true)
    private let allButton = NSButton(title: L("Fetch All"), target: nil, action: nil)
    private let spinner = NSProgressIndicator()
    private let stack = NSStackView()

    override init(frame: NSRect) {
        super.init(frame: frame)
        label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        label.textColor = .secondaryLabelColor
        label.lineBreakMode = .byTruncatingTail
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        moreButton.controlSize = .small
        moreButton.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        moreButton.bezelStyle = .push
        let menu = moreButton.menu!
        // 引き下げ式なので、最初の項目がボタンの文字。
        menu.addItem(withTitle: L("Fetch More"), action: nil, keyEquivalent: "")
        for (title, commits) in [(L("100 Commits"), 100), (L("1,000 Commits"), 1_000), (L("10,000 Commits"), 10_000)] {
            menu.addItem(ClosureMenuItem(title: title) { [weak self] in self?.fetch(["op": "deepen", "commits": commits]) })
        }
        menu.addItem(.separator())
        menu.addItem(ClosureMenuItem(title: L("Up to a Date…")) { [weak self] in self?.fetchSinceDate() })
        moreButton.toolTip = L("Fetch older commits from the remote (git fetch --deepen)")
        allButton.controlSize = .small
        allButton.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        allButton.bezelStyle = .push
        allButton.target = self
        allButton.action = #selector(fetchAll)
        allButton.toolTip = L("Fetch the whole history from the remote (git fetch --unshallow)")
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 14, bottom: 0, right: 14)
        for view in [label, moreButton, allButton, spinner] { stack.addArrangedSubview(view) }
        stack.setCustomSpacing(14, after: label)
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor, constant: 2),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    // 表の空いた所と同じく、押しても行の選択を変えない（下の表に通さない）。
    override func mouseDown(with event: NSEvent) {}

    override func draw(_ dirtyRect: NSRect) {
        // 最後の行との境目に、途切れたことを表す点線。
        NSColor.separatorColor.setStroke()
        let line = NSBezierPath()
        line.move(to: NSPoint(x: 0, y: 0.5))
        line.line(to: NSPoint(x: bounds.width, y: 0.5))
        line.lineWidth = 1
        line.setLineDash([3, 3], count: 2, phase: 0)
        line.stroke()
    }

    @objc private func fetchAll() {
        fetch(["op": "unshallow"])
    }

    /// 日付を尋ね、その日付より新しいコミットをすべて取る。
    private func fetchSinceDate() {
        let alert = NSAlert()
        alert.messageText = L("Fetch History Up to a Date")
        alert.informativeText = L("Fetches the commits made after this date (git fetch --shallow-since).")
        alert.addButton(withTitle: L("Fetch"))
        alert.addButton(withTitle: L("Cancel"))
        let picker = NSDatePicker()
        picker.datePickerStyle = .textFieldAndStepper
        picker.datePickerElements = .yearMonthDay
        picker.dateValue = Calendar.current.date(byAdding: .year, value: -1, to: boundaryDate?() ?? Date()) ?? Date()
        picker.maxDate = Date()
        picker.sizeToFit()
        alert.accessoryView = picker
        alert.window.initialFirstResponder = picker
        let run: (NSApplication.ModalResponse) -> Void = { [weak self] response in
            guard response == .alertFirstButtonReturn else { return }
            let f = DateFormatter()
            f.calendar = Calendar(identifier: .gregorian)
            f.locale = Locale(identifier: "en_US_POSIX")
            f.dateFormat = "yyyy-MM-dd"
            self?.fetch(["op": "deepen", "since": f.string(from: picker.dateValue)])
        }
        if let window { alert.beginSheetModal(for: window, completionHandler: run) } else { run(alert.runModal()) }
    }

    private func fetch(_ command: [String: Any]) {
        guard let onFetch else { return }
        setRunning(true)
        onFetch(command) { [weak self] in self?.setRunning(false) }
    }

    private func setRunning(_ running: Bool) {
        moreButton.isEnabled = !running
        allButton.isEnabled = !running
        if running { spinner.startAnimation(nil) } else { spinner.stopAnimation(nil) }
    }
}
