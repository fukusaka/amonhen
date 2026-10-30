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
        for (title, commits) in Self.moreChoices {
            menu.addItem(ClosureMenuItem(title: title) { [weak self] in self?.fetch(["op": "deepen", "commits": commits]) })
        }
        menu.addItem(ClosureMenuItem(title: L("Other…")) { [weak self] in
            Self.askDeepen(in: self?.window) { commits in self?.fetch(["op": "deepen", "commits": commits]) }
        })
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

    /// 「さらに取る」の数（項目の名前と、コミットの数）。Repository Settings の「取得とフォルダ」も同じものを出す。
    static var moreChoices: [(title: String, commits: Int)] {
        [(L("100 Commits"), 100), (L("1,000 Commits"), 1_000), (L("10,000 Commits"), 10_000)]
    }

    /// 「さらに取る」の「その他…」。何コミット取り足すかを尋ねる。
    static func askDeepen(in window: NSWindow?, done: @escaping (Int) -> Void) {
        askCount(in: window, title: L("Fetch More History"),
                 message: L("How many older commits to fetch (git fetch --deepen)."), button: L("Fetch"),
                 initial: 100, done: done)
    }

    /// コミットの数（1 以上）を尋ねる。決めたら `done` に渡す。
    static func askCount(in window: NSWindow?, title: String, message: String, button: String, initial: Int,
                         done: @escaping (Int) -> Void) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: button)
        alert.addButton(withTitle: L("Cancel"))
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 120, height: 22))
        let format = NumberFormatter()
        format.numberStyle = .decimal
        format.minimum = 1
        format.maximumFractionDigits = 0
        format.allowsFloats = false
        field.formatter = format
        field.integerValue = initial
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        let run: (NSApplication.ModalResponse) -> Void = { response in
            guard response == .alertFirstButtonReturn, field.integerValue >= 1 else { return }
            done(field.integerValue)
        }
        if let window { alert.beginSheetModal(for: window, completionHandler: run) } else { run(alert.runModal()) }
    }

    /// 日付を尋ね、その日付より新しいコミットをすべて取る。
    private func fetchSinceDate() {
        Self.askDate(in: window, boundary: boundaryDate?()) { [weak self] since in self?.fetch(["op": "deepen", "since": since]) }
    }

    /// 取り足す日付を尋ねる（初めは `boundary`（境界のコミットの日時。分からなければ今）の 1 年前）。決めたら、git が
    /// 読む形（`YYYY-MM-DD`）で `done` に渡す。
    static func askDate(in window: NSWindow?, boundary: Date?, done: @escaping (String) -> Void) {
        let alert = NSAlert()
        alert.messageText = L("Fetch History Up to a Date")
        alert.informativeText = L("Fetches the commits made after this date (git fetch --shallow-since).")
        alert.addButton(withTitle: L("Fetch"))
        alert.addButton(withTitle: L("Cancel"))
        let picker = NSDatePicker()
        picker.datePickerStyle = .textFieldAndStepper
        picker.datePickerElements = .yearMonthDay
        picker.dateValue = Calendar.current.date(byAdding: .year, value: -1, to: boundary ?? Date()) ?? Date()
        picker.maxDate = Date()
        picker.sizeToFit()
        alert.accessoryView = picker
        alert.window.initialFirstResponder = picker
        let run: (NSApplication.ModalResponse) -> Void = { response in
            guard response == .alertFirstButtonReturn else { return }
            let f = DateFormatter()
            f.calendar = Calendar(identifier: .gregorian)
            f.locale = Locale(identifier: "en_US_POSIX")
            f.dateFormat = "yyyy-MM-dd"
            done(f.string(from: picker.dateValue))
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
