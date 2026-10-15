import AmonhenKit
import AppKit

/// コンフリクトのあるファイル 1 つを解決するウィンドウ。上の左に Incoming、右に Current（どちらも、すべての
/// コンフリクトをその側にした内容）、下に編集できる Result（今の作業ツリーの内容から始める）。
///
/// コンフリクトごとに採用する側を選ぶと Result のそのコンフリクトが置き換わり、Result は直接編集もできる。
/// 解決の計算はコア（`resolveText`・`conflictRegions`）に任せる。
final class MergeEditorWindowController: NSWindowController, NSWindowDelegate, NSTextViewDelegate {
    /// 開いているエディタ（パスごとに 1 つ）。
    private static var open: [String: MergeEditorWindowController] = [:]

    static func show(path: String, repository: Repository, actions: Actions) {
        if let existing = open[path] {
            existing.window?.makeKeyAndOrderFront(nil)
            return
        }
        Task {
            do {
                let sides = try await Task.detached(priority: .userInitiated) {
                    try repository.info(["op": "mergeSides", "path": path], as: MergeSides.self)
                }.value
                let controller = MergeEditorWindowController(path: path, sides: sides, repository: repository, actions: actions)
                open[path] = controller
                controller.showWindow(nil)
            } catch {
                NSAlert(error: error).runModal()
            }
        }
    }

    /// 開いているエディタの画像（`--snapshot` と `--merge-editor` の確認用）。
    static func snapshot() -> NSBitmapImageRep? {
        guard let view = open.values.first?.window?.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return nil }
        view.cacheDisplay(in: view.bounds, to: rep)
        return rep
    }

    private let path: String
    private let sides: MergeSides
    private let repository: Repository
    private let actions: Actions

    private let incoming = MergeEditorWindowController.textView(editable: false)
    private let current = MergeEditorWindowController.textView(editable: false)
    private let base = MergeEditorWindowController.textView(editable: false)
    private let result = MergeEditorWindowController.textView(editable: true)
    private let position = NSTextField(labelWithString: "")
    private let previousButton = NSButton()
    private let nextButton = NSButton()
    private var acceptButtons: [NSButton] = []
    private var baseButton: NSButton?

    /// 最初の Result に入っていたコンフリクト（両側の中身）。添字が上の欄の `regions` の添字に当たる。
    private var originals: [(current: String, incoming: String)] = []
    /// 今の Result に残っているコンフリクトと、それぞれが `originals` の何番目か（わからなければ nil）。
    private var conflicts: [ConflictRegions.Conflict] = []
    private var originalOf: [Int?] = []
    /// 選んでいるコンフリクト（`conflicts` の添字）。
    private var selected = 0
    private var savedText: String
    private var reparse: DispatchWorkItem?

    private init(path: String, sides: MergeSides, repository: Repository, actions: Actions) {
        self.path = path
        self.sides = sides
        self.repository = repository
        self.actions = actions
        savedText = sides.result
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1200, height: 820),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "Merge — \(path)"
        window.center()
        super.init(window: window)
        window.delegate = self
        build()
        result.string = sides.result
        result.undoManager?.removeAllActions()
        incoming.string = sides.incoming?.text ?? ""
        current.string = sides.current?.text ?? ""
        base.string = sides.base?.text ?? ""
        updateConflicts(initial: true)
        for text in [incoming, current, base, result] { colorSyntax(text) }
    }

    required init?(coder: NSCoder) { fatalError() }

    // MARK: - 組み立て

    private static func textView(editable: Bool) -> NSTextView {
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 300))
        text.isEditable = editable
        text.isRichText = false
        text.allowsUndo = editable
        text.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        text.textContainerInset = NSSize(width: 4, height: 4)
        for flag in [\NSTextView.isAutomaticQuoteSubstitutionEnabled, \.isAutomaticDashSubstitutionEnabled,
                     \.isAutomaticTextReplacementEnabled, \.isAutomaticSpellingCorrectionEnabled, \.isContinuousSpellCheckingEnabled] {
            text[keyPath: flag] = false
        }
        // コードなので折り返さず、横にスクロールする。
        text.isHorizontallyResizable = true
        text.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        text.textContainer?.widthTracksTextView = false
        text.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        return text
    }

    /// 見出しと、スクロールする文字の欄。フレームと自動の伸縮で並べる（分割の中で大きさが決まるように）。
    private func pane(_ title: String, _ text: NSTextView) -> NSView {
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 600, height: 400))
        let labelHeight: CGFloat = 20
        let label = NSTextField(labelWithString: title)
        label.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
        label.textColor = .secondaryLabelColor
        label.frame = NSRect(x: 6, y: 400 - labelHeight + 2, width: 588, height: labelHeight - 4)
        label.autoresizingMask = [.width, .minYMargin]
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 600, height: 400 - labelHeight))
        scroll.autoresizingMask = [.width, .height]
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        text.frame = NSRect(origin: .zero, size: scroll.contentSize)
        text.minSize = NSSize(width: 0, height: scroll.contentSize.height)
        text.isVerticallyResizable = true
        text.autoresizingMask = [.width]
        scroll.documentView = text
        container.addSubview(label)
        container.addSubview(scroll)
        return container
    }

    private func build() {
        let hasBase = sides.base != nil
        var top = [pane("Incoming（取り込む側）", incoming)]
        if hasBase { top.append(pane("Base（共通の祖先）", base)) }
        top.append(pane("Current（今のブランチ）", current))
        let topSplit = NSSplitView()
        topSplit.isVertical = true
        topSplit.dividerStyle = .thin
        for v in top { topSplit.addArrangedSubview(v) }
        let outer = NSSplitView()
        outer.isVertical = false
        outer.dividerStyle = .thin
        outer.addArrangedSubview(topSplit)
        outer.addArrangedSubview(pane("Result（結果、編集できる）", result))

        previousButton.image = NSImage(systemSymbolName: "chevron.up", accessibilityDescription: "前のコンフリクト")
        previousButton.target = self
        previousButton.action = #selector(selectPrevious)
        nextButton.image = NSImage(systemSymbolName: "chevron.down", accessibilityDescription: "次のコンフリクト")
        nextButton.target = self
        nextButton.action = #selector(selectNext)
        var accepts: [(String, String)] = [("Accept Incoming", "theirs"), ("Accept Current", "ours"), ("Accept Both", "both")]
        if hasBase { accepts.append(("Accept Base", "base")) }
        acceptButtons = accepts.map { title, choice in
            let b = NSButton(title: title, target: self, action: #selector(accept(_:)))
            b.identifier = .init(choice)
            return b
        }
        let save = NSButton(title: "Save", target: self, action: #selector(saveOnly))
        let resolve = NSButton(title: "Save & Mark Resolved", target: self, action: #selector(saveAndResolve))
        resolve.keyEquivalent = "\r"
        for b in [previousButton, nextButton] + acceptButtons + [save, resolve] {
            b.bezelStyle = .push
            b.controlSize = .small
        }
        position.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        let bar = NSStackView(views: [position, previousButton, nextButton] + acceptButtons + [NSView(), save, resolve])
        bar.spacing = 6
        bar.edgeInsets = NSEdgeInsets(top: 0, left: 10, bottom: 0, right: 10)

        // 上に操作の帯、残りに欄。
        let size = window?.contentView?.bounds.size ?? NSSize(width: 1200, height: 820)
        let barHeight: CGFloat = 36
        let root = NSView(frame: NSRect(origin: .zero, size: size))
        bar.frame = NSRect(x: 0, y: size.height - barHeight, width: size.width, height: barHeight)
        bar.autoresizingMask = [.width, .minYMargin]
        outer.frame = NSRect(x: 0, y: 0, width: size.width, height: size.height - barHeight)
        outer.autoresizingMask = [.width, .height]
        topSplit.frame = NSRect(x: 0, y: 0, width: size.width, height: (size.height - barHeight) * 0.45)
        root.addSubview(bar)
        root.addSubview(outer)
        window?.contentView = root

        result.delegate = self
        incoming.delegate = self
        current.delegate = self
        base.delegate = self
        // 上下を半分ずつ、上の欄は等分から始める。
        DispatchQueue.main.async {
            outer.setPosition(outer.bounds.height * 0.45, ofDividerAt: 0)
            let width = topSplit.bounds.width / CGFloat(top.count)
            for i in 0..<(top.count - 1) { topSplit.setPosition(width * CGFloat(i + 1), ofDividerAt: i) }
        }
    }

    // MARK: - コンフリクト

    /// Result を読み直して、残っているコンフリクトと上の欄との対応を決め、強調し直す。
    private func updateConflicts(initial: Bool = false) {
        let text = result.string
        let found = (try? repository.info(["op": "conflictRegions", "text": text], as: ConflictRegions.self))?.conflicts ?? []
        if initial { originals = found.map { ($0.current, $0.incoming) } }
        conflicts = found
        // 両側の中身が同じ最初のコンフリクトに対応させる（手で直したものは対応がなくなる）。
        var used = Set<Int>()
        originalOf = found.map { c in
            let j = originals.indices.first { !used.contains($0) && originals[$0] == (c.current, c.incoming) }
            if let j { used.insert(j) }
            return j
        }
        selected = min(selected, max(0, conflicts.count - 1))
        refresh()
    }

    private func refresh() {
        position.stringValue = conflicts.isEmpty ? "コンフリクトなし" : "Conflict \(selected + 1) / \(conflicts.count)"
        for b in [previousButton, nextButton] + acceptButtons { b.isEnabled = !conflicts.isEmpty }
        // 共通の祖先は、対応のわかるコンフリクトでだけ選べる。
        if let baseIndex = acceptButtons.firstIndex(where: { $0.identifier?.rawValue == "base" }) {
            acceptButtons[baseIndex].isEnabled = !conflicts.isEmpty && originalOf[selected] != nil
        }
        let chosen = conflicts.isEmpty ? nil : originalOf[selected]
        highlight(incoming, regions: sides.incoming?.regions ?? [], selected: chosen, color: .systemGreen)
        highlight(current, regions: sides.current?.regions ?? [], selected: chosen, color: .systemBlue)
        highlight(base, regions: sides.base?.regions ?? [], selected: chosen, color: .systemGray)
        highlight(result, regions: conflicts.map(\.lines), selected: conflicts.isEmpty ? nil : selected, color: .systemOrange)
    }

    private func highlight(_ text: NSTextView, regions: [[Int]], selected: Int?, color: NSColor) {
        guard let layout = text.layoutManager else { return }
        let whole = NSRange(location: 0, length: (text.string as NSString).length)
        layout.removeTemporaryAttribute(.backgroundColor, forCharacterRange: whole)
        let starts = Self.lineStarts(text.string)
        for (i, r) in regions.enumerated() where r.count == 2 {
            let range = Self.characterRange(lines: r[0]..<r[1], starts: starts)
            layout.addTemporaryAttribute(
                .backgroundColor, value: color.withAlphaComponent(i == selected ? 0.35 : 0.14), forCharacterRange: range)
        }
    }

    /// 欄ごとの、色付けを頼んだ回数（古い結果を捨てるため）。
    private var syntaxGeneration: [ObjectIdentifier: Int] = [:]

    /// 構文の色を、文字の一時的な属性として付ける（中身や取り消しの履歴は変えない）。解析は裏で行う。
    private func colorSyntax(_ text: NSTextView) {
        let key = ObjectIdentifier(text)
        let generation = (syntaxGeneration[key] ?? 0) + 1
        syntaxGeneration[key] = generation
        let string = text.string, path = self.path
        Task {
            let spans = await Task.detached(priority: .utility) { SyntaxSpans.highlight(path: path, text: string) }.value
            // 解析している間に書き換わっていたら、次の色付けに任せる。
            guard let spans, syntaxGeneration[key] == generation, let layout = text.layoutManager else { return }
            let length = (text.string as NSString).length
            layout.removeTemporaryAttribute(.foregroundColor, forCharacterRange: NSRange(location: 0, length: length))
            let palette = SyntaxColors.palette
            spans.withSpans { spans in
                for span in spans where Int(span.end) <= length {
                    layout.addTemporaryAttribute(
                        .foregroundColor, value: palette[Int(span.kind) & 15],
                        forCharacterRange: NSRange(location: Int(span.start), length: Int(span.end - span.start)))
                }
            }
        }
    }

    /// 各行の先頭の位置（UTF-16）。最後に文章の長さを足す。
    private static func lineStarts(_ string: String) -> [Int] {
        var starts = [0]
        let s = string as NSString
        var i = 0
        while i < s.length {
            if s.character(at: i) == 0x0A { starts.append(i + 1) }
            i += 1
        }
        if starts.last != s.length { starts.append(s.length) }
        return starts
    }

    private static func characterRange(lines: Range<Int>, starts: [Int]) -> NSRange {
        let start = starts[min(lines.lowerBound, starts.count - 1)]
        let end = starts[min(lines.upperBound, starts.count - 1)]
        return NSRange(location: start, length: max(0, end - start))
    }

    /// 選んだコンフリクトが見えるように、各欄をスクロールする。
    private func reveal() {
        guard !conflicts.isEmpty else { return }
        scroll(result, toLine: conflicts[selected].lines[0])
        guard let j = originalOf[selected] else { return }
        for (text, side) in [(incoming, sides.incoming), (current, sides.current), (base, sides.base)] {
            if let r = side?.regions[j] { scroll(text, toLine: r[0]) }
        }
    }

    private func scroll(_ text: NSTextView, toLine line: Int) {
        guard let layout = text.layoutManager, let container = text.textContainer,
              let clip = text.enclosingScrollView?.contentView else { return }
        let range = Self.characterRange(lines: line..<(line + 1), starts: Self.lineStarts(text.string))
        let glyphs = layout.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
        let rect = layout.boundingRect(forGlyphRange: glyphs, in: container)
        // 上から 3 分の 1 あたりに出す。
        let y = max(0, rect.minY - clip.bounds.height / 3)
        clip.scroll(to: NSPoint(x: 0, y: y))
        text.enclosingScrollView?.reflectScrolledClipView(clip)
    }

    @objc private func selectPrevious() { move(by: -1) }
    @objc private func selectNext() { move(by: 1) }

    private func move(by step: Int) {
        guard !conflicts.isEmpty else { return }
        selected = (selected + step + conflicts.count) % conflicts.count
        refresh()
        reveal()
    }

    /// 選んだコンフリクトを、ボタンの側で解決する。Result の変わった部分だけを置き換えて、Undo できるようにする。
    @objc private func accept(_ sender: NSButton) {
        guard !conflicts.isEmpty, let choice = sender.identifier?.rawValue else { return }
        let old = result.string
        let resolved: String
        do {
            guard let text = try repository.run(["op": "resolveText", "text": old, "index": selected, "choice": choice]).text
            else { return }
            resolved = text
        } catch {
            NSAlert(error: error).runModal()
            return
        }
        replace(old: old, with: resolved)
        updateConflicts()
        reveal()
    }

    private func replace(old: String, with new: String) {
        let a = Array((old as NSString as String).utf16), b = Array(new.utf16)
        var prefix = 0
        while prefix < a.count, prefix < b.count, a[prefix] == b[prefix] { prefix += 1 }
        var suffix = 0
        while suffix < a.count - prefix, suffix < b.count - prefix, a[a.count - 1 - suffix] == b[b.count - 1 - suffix] {
            suffix += 1
        }
        let range = NSRange(location: prefix, length: a.count - prefix - suffix)
        let replacement = String(utf16CodeUnits: Array(b[prefix..<(b.count - suffix)]), count: b.count - prefix - suffix)
        guard result.shouldChangeText(in: range, replacementString: replacement) else { return }
        result.textStorage?.replaceCharacters(in: range, with: replacement)
        result.didChangeText()
    }

    // MARK: - 編集と選択

    func textDidChange(_ notification: Notification) {
        guard notification.object as? NSTextView === result else { return }
        // 打つたびに解析しないよう、少し待ってから読み直す。
        reparse?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self else { return }
            updateConflicts()
            colorSyntax(result)
        }
        reparse = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25, execute: work)
    }

    /// 上の欄やResult でクリックした位置のコンフリクトを選ぶ。
    func textViewDidChangeSelection(_ notification: Notification) {
        guard let text = notification.object as? NSTextView, !conflicts.isEmpty else { return }
        let location = text.selectedRange().location
        let starts = Self.lineStarts(text.string)
        let line = (starts.lastIndex { $0 <= location } ?? 0)
        var index: Int?
        if text === result {
            index = conflicts.firstIndex { $0.lines[0] <= line && line < $0.lines[1] }
        } else {
            let side = text === incoming ? sides.incoming : text === current ? sides.current : sides.base
            if let j = side?.regions.firstIndex(where: { $0[0] <= line && line < max($0[1], $0[0] + 1) }) {
                index = originalOf.firstIndex { $0 == j }
            }
        }
        if let index, index != selected {
            selected = index
            refresh()
        }
    }

    // MARK: - 保存

    @objc private func saveOnly() { save(stage: false) }

    @objc private func saveAndResolve() {
        if !conflicts.isEmpty, !actions.confirm(
            "コンフリクトが \(conflicts.count) 個残っています", message: "マーカーを残したまま解決済みにしますか？", ok: "解決済みにする") {
            return
        }
        save(stage: true)
    }

    private func save(stage: Bool) {
        let text = result.string
        actions.perform(["op": "saveMerge", "path": path, "text": text, "stage": stage]) { [weak self] _ in
            guard let self else { return }
            self.savedText = text
            if stage { self.close() }
        }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        result.string == savedText || actions.confirm(
            "保存していない変更があります", message: "閉じると Result の変更は失われます。", ok: "保存せずに閉じる")
    }

    func windowWillClose(_ notification: Notification) {
        Self.open[path] = nil
    }
}
