import AmonhenKit
import AppKit

/// コンフリクトのあるファイル 1 つを解決するエディタ。左に Current、真ん中に編集できる Result（今の作業ツリーの
/// 内容から始める）、右に Incoming（左右は、すべてのコンフリクトをその側にした内容）。共通の祖先（diff3）が
/// あれば、Base を左端に出せる。
///
/// 列の間の帯は、左右の欄のコンフリクトと Result のコンフリクトをつなぎ、≫・≪ を押すとその側で解決する。
/// ✨ Magic Merge は、決まった規則で解けるコンフリクトをまとめて解く。3 列のスクロールは、コンフリクトの位置で
/// 揃える。解決の計算はコア（`resolveText`・`conflictRegions`・`magicMerge`）に任せる。
///
/// Working Copy の右に埋め込むほか、別ウィンドウ（`MergeEditorWindowController`）でも開ける。
final class MergeEditorViewController: NSViewController, NSTextViewDelegate {
    let path: String
    private let sides: MergeSides
    private let repository: Repository
    private let actions: Actions
    /// 保存して解決済みにした（埋め込んだ側はエディタを外し、ウィンドウは閉じる）。
    var onResolved: (() -> Void)?
    /// 別ウィンドウで開く（埋め込んだときだけ出す）。
    var onPopOut: (() -> Void)?

    private let current = MergeEditorViewController.textView(editable: false)
    private let incoming = MergeEditorViewController.textView(editable: false)
    private let base = MergeEditorViewController.textView(editable: false)
    private let result = MergeEditorViewController.textView(editable: true)
    private lazy var leftBand = ConflictBand(side: .ours)
    private lazy var rightBand = ConflictBand(side: .theirs)
    private var panes: [NSView] = []
    private var basePane: NSView?
    private let position = NSTextField(labelWithString: "")
    private let previousButton = NSButton()
    private let nextButton = NSButton()
    private var acceptButtons: [NSButton] = []
    private let magicButton = NSButton(title: "✨ Magic Merge", target: nil, action: nil)
    private let baseToggle = NSButton(checkboxWithTitle: "Base", target: nil, action: nil)
    /// Magic Merge の結果など、直前の操作の知らせ。
    private let note = NSTextField(labelWithString: "")
    /// AI: 選んだコンフリクトの解消の提案と解説。
    private let aiResolveButton = NSButton(title: L("Resolve with AI"), target: nil, action: nil)
    private let aiExplainButton = NSButton(title: L("Explain with AI"), target: nil, action: nil)

    /// 最初の Result に入っていたコンフリクト（両側の中身）。添字が左右の欄の `regions` の添字に当たる。
    private var originals: [(current: String, incoming: String)] = []
    /// 今の Result に残っているコンフリクトと、それぞれが `originals` の何番目か（わからなければ nil）。
    private var conflicts: [ConflictRegions.Conflict] = []
    private var originalOf: [Int?] = []
    /// 選んでいるコンフリクト（`conflicts` の添字）。
    private var selected = 0
    private(set) var savedText: String
    private var reparse: DispatchWorkItem?
    /// ほかの欄をスクロールで揃えている間（揃えた結果の通知で揃え返さない）。
    private var syncing = false

    init(path: String, sides: MergeSides, repository: Repository, actions: Actions) {
        self.path = path
        self.sides = sides
        self.repository = repository
        self.actions = actions
        savedText = sides.result
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    /// Result を保存していない変更があるか。
    var hasUnsavedChanges: Bool { result.string != savedText }

    /// `path` の中身を読んでエディタを作る（裏で読む）。
    static func load(path: String, repository: Repository, actions: Actions) async throws -> MergeEditorViewController {
        let sides = try await Task.detached(priority: .userInitiated) {
            try repository.info(["op": "mergeSides", "path": path], as: MergeSides.self)
        }.value
        return MergeEditorViewController(path: path, sides: sides, repository: repository, actions: actions)
    }

    // MARK: - 組み立て

    private static func textView(editable: Bool) -> NSTextView {
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        text.isEditable = editable
        text.isRichText = false
        text.allowsUndo = editable
        text.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        text.textContainerInset = NSSize(width: 4, height: 4)
        for flag in [\NSTextView.isAutomaticQuoteSubstitutionEnabled, \.isAutomaticDashSubstitutionEnabled,
                     \.isAutomaticTextReplacementEnabled, \.isAutomaticSpellingCorrectionEnabled, \.isContinuousSpellCheckingEnabled] {
            text[keyPath: flag] = false
        }
        // コードなので折り返さず、横にスクロールする（行の高さが揃い、3 列を行で揃えられる）。
        text.isHorizontallyResizable = true
        text.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        text.textContainer?.widthTracksTextView = false
        text.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        return text
    }

    private static let labelHeight: CGFloat = 20

    /// 見出しと、スクロールする文字の欄。
    private func pane(_ title: String, _ text: NSTextView) -> NSView {
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 400))
        let label = NSTextField(labelWithString: title)
        label.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
        label.textColor = .secondaryLabelColor
        label.lineBreakMode = .byTruncatingTail
        label.frame = NSRect(x: 6, y: 400 - Self.labelHeight + 2, width: 388, height: Self.labelHeight - 4)
        label.autoresizingMask = [.width, .minYMargin]
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 400, height: 400 - Self.labelHeight))
        scroll.autoresizingMask = [.width, .height]
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        text.frame = NSRect(origin: .zero, size: scroll.contentSize)
        text.minSize = NSSize(width: 0, height: scroll.contentSize.height)
        text.isVerticallyResizable = true
        text.autoresizingMask = [.width]
        scroll.documentView = text
        scroll.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self, selector: #selector(scrolled(_:)), name: NSView.boundsDidChangeNotification, object: scroll.contentView)
        container.addSubview(label)
        container.addSubview(scroll)
        return container
    }

    override func loadView() {
        let hasBase = sides.base != nil
        let currentPane = pane(L("Current (your branch)"), current)
        let resultPane = pane(L("Result (editable)"), result)
        let incomingPane = pane(L("Incoming (being merged in)"), incoming)
        if hasBase { basePane = pane(L("Base (common ancestor)"), base) }
        panes = [currentPane, resultPane, incomingPane]
        for band in [leftBand, rightBand] {
            band.owner = self
        }

        previousButton.image = NSImage(systemSymbolName: "chevron.up", accessibilityDescription: L("Previous Conflict"))
        previousButton.target = self
        previousButton.action = #selector(selectPrevious)
        previousButton.toolTip = L("Previous Conflict (⌥⌘↑)")
        nextButton.image = NSImage(systemSymbolName: "chevron.down", accessibilityDescription: L("Next Conflict"))
        nextButton.target = self
        nextButton.action = #selector(selectNext)
        nextButton.toolTip = L("Next Conflict (⌥⌘↓)")
        magicButton.target = self
        magicButton.action = #selector(magicMerge)
        magicButton.toolTip = L(
            "Resolve all conflicts that fixed rules can solve: both sides are the same, only one side changed (diff3), only whitespace differs, or different words on the same line changed (diff3). You can Undo it.")
        var accepts: [(String, String)] = [(L("Accept Current"), "ours"), (L("Accept Incoming"), "theirs"), (L("Accept Both"), "both")]
        if hasBase { accepts.append((L("Accept Base"), "base")) }
        acceptButtons = accepts.map { title, choice in
            let b = NSButton(title: title, target: self, action: #selector(accept(_:)))
            b.identifier = .init(choice)
            return b
        }
        baseToggle.target = self
        baseToggle.action = #selector(toggleBase)
        baseToggle.isHidden = !hasBase
        baseToggle.toolTip = L("Show the common ancestor on the left.")
        let popOut = NSButton(
            image: NSImage(systemSymbolName: "macwindow.on.rectangle", accessibilityDescription: L("Open in a Separate Window"))!,
            target: self, action: #selector(popOutClicked))
        popOut.toolTip = L("Open in a Separate Window")
        popOut.isHidden = onPopOut == nil
        let save = NSButton(title: L("Save"), target: self, action: #selector(saveOnly))
        let resolve = NSButton(title: L("Save & Mark Resolved"), target: self, action: #selector(saveAndResolve))
        resolve.keyEquivalent = "\r"
        resolve.keyEquivalentModifierMask = .command
        resolve.toolTip = L("Save and mark as resolved (⌘↩).")
        for b in [previousButton, nextButton, magicButton] + acceptButtons + [popOut, save, resolve] {
            b.bezelStyle = .push
            b.controlSize = .small
        }
        position.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        let bar = NSStackView(
            views: [position, previousButton, nextButton, magicButton, aiResolveButton, aiExplainButton, note] + acceptButtons
                + [baseToggle, NSView(), popOut, save, resolve])
        aiResolveButton.target = self
        aiResolveButton.action = #selector(aiResolve)
        aiResolveButton.toolTip = L("AI: Suggest a resolution for the selected conflict (Result doesn’t change until you accept it).")
        aiExplainButton.target = self
        aiExplainButton.action = #selector(aiExplain)
        aiExplainButton.toolTip = L("AI: Explain what each side of the selected conflict is trying to do.")
        for b in [aiResolveButton, aiExplainButton] {
            b.bezelStyle = .push
            b.controlSize = .small
            b.isHidden = !Assistant.isConfigured
        }
        note.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        note.textColor = .secondaryLabelColor
        // 文が長くてもボタンを押し出さないよう、メモの方を切り詰める。
        note.lineBreakMode = .byTruncatingTail
        note.setContentCompressionResistancePriority(.defaultLow - 1, for: .horizontal)
        bar.spacing = 6
        bar.edgeInsets = NSEdgeInsets(top: 0, left: 10, bottom: 0, right: 10)

        let root = MergeLayoutView(bar: bar)
        root.onLayout = { [weak self] in self?.layoutColumns() }
        root.frame = NSRect(x: 0, y: 0, width: 1200, height: 700)
        for v in [basePane].compactMap({ $0 }) + panes + [leftBand, rightBand] { root.addSubview(v) }
        basePane?.isHidden = true
        view = root
        layoutColumns()

        result.delegate = self
        for text in [current, incoming, base] { text.delegate = self }
        result.string = sides.result
        result.undoManager?.removeAllActions()
        current.string = sides.current?.text ?? ""
        incoming.string = sides.incoming?.text ?? ""
        base.string = sides.base?.text ?? ""
        updateConflicts(initial: true)
        for text in [current, incoming, base, result] { colorSyntax(text) }
    }

    private var autoApplied = false

    override func viewDidAppear() {
        super.viewDidAppear()
        // 開いたときに、間違えようのないコンフリクト（両側が同じ、片側だけが変えた）を解いておく（設定で切れる）。
        // Undo はウィンドウに入ってからでないと効かないので、最初に出たときにする。
        guard !autoApplied else { return }
        autoApplied = true
        guard Settings.autoMagicMerge else { return }
        let solved = applyMagic(safe: true)
        if solved > 0 {
            colorSyntax(result)
            note.stringValue = L("Resolved %ld when opened (⌘Z to undo).", solved)
        }
    }

    /// 欄を左から Base（出すとき）・Current・帯・Result・帯・Incoming に並べる。帯は幅を固定し、残りを等分する。
    private func layoutColumns() {
        // 組み立てている途中（`view` を入れる前）に呼ばれたら何もしない（`view` を読むと組み立て直しになる）。
        guard let root = viewIfLoaded as? MergeLayoutView else { return }
        let area = root.contentArea
        let bandWidth: CGFloat = 40
        let showBase = basePane.map { !$0.isHidden } ?? false
        let columns: [NSView] = (showBase ? [basePane!] : []) + panes
        let gaps = CGFloat(columns.count - 1)
        let width = max(0, (area.width - bandWidth * 2 - (showBase ? 1 : 0)) / CGFloat(columns.count))
        var x = area.minX
        for (i, column) in columns.enumerated() {
            column.frame = NSRect(x: x, y: area.minY, width: width, height: area.height)
            x += width
            if column === panes[0] {
                leftBand.frame = NSRect(x: x, y: area.minY, width: bandWidth, height: area.height - Self.labelHeight)
                x += bandWidth
            } else if column === panes[1] {
                rightBand.frame = NSRect(x: x, y: area.minY, width: bandWidth, height: area.height - Self.labelHeight)
                x += bandWidth
            } else if i < Int(gaps) {
                x += 1
            }
        }
        leftBand.needsDisplay = true
        rightBand.needsDisplay = true
    }

    @objc private func toggleBase() {
        basePane?.isHidden = baseToggle.state != .on
        layoutColumns()
    }

    @objc private func popOutClicked() { onPopOut?() }

    // MARK: - 行の位置

    /// 1 行の高さ（等幅で折り返さないので、どの欄も同じ）。
    private lazy var lineHeight: CGFloat = {
        result.layoutManager?.defaultLineHeight(for: result.font ?? .monospacedSystemFont(ofSize: 12, weight: .regular)) ?? 15
    }()

    /// `text` の `line` 行目の上端の、帯の座標（欄の文字の領域の上端からの距離。見えている範囲の外なら外の値）。
    fileprivate func y(ofLine line: Int, in text: NSTextView) -> CGFloat {
        let top = text.enclosingScrollView?.contentView.bounds.origin.y ?? 0
        return CGFloat(line) * lineHeight + text.textContainerInset.height - top
    }

    /// 帯に描くもの: 左右の欄のコンフリクトの行範囲と、Result のコンフリクトの行範囲（選んでいるか）。
    fileprivate func bandRegions(_ side: ConflictBand.Side) -> [(side: Range<Int>, result: Range<Int>, selected: Bool, index: Int)] {
        let regions = (side == .ours ? sides.current : sides.incoming)?.regions ?? []
        return conflicts.indices.compactMap { i in
            guard let j = originalOf[i], j < regions.count, regions[j].count == 2 else { return nil }
            let lines = conflicts[i].lines
            return (regions[j][0]..<regions[j][1], lines[0]..<lines[1], i == selected, i)
        }
    }

    fileprivate func sideText(_ side: ConflictBand.Side) -> NSTextView { side == .ours ? current : incoming }
    fileprivate var resultText: NSTextView { result }

    /// Result の行 `line` に当たる、`side` の欄の行（コンフリクトの位置で区切って、間は比例で割り当てる）。
    private func map(_ line: CGFloat, from: [Int], to: [Int]) -> CGFloat {
        // 区切り: (0, 0) と、各コンフリクトの始まり・終わり。
        guard from.count == to.count, !from.isEmpty else { return line }
        if line <= CGFloat(from[0]) { return line - CGFloat(from[0]) + CGFloat(to[0]) }
        for k in 1..<from.count where line <= CGFloat(from[k]) {
            let (f0, f1, t0, t1) = (CGFloat(from[k - 1]), CGFloat(from[k]), CGFloat(to[k - 1]), CGFloat(to[k]))
            return f1 == f0 ? t1 : t0 + (line - f0) * (t1 - t0) / (f1 - f0)
        }
        return line - CGFloat(from.last!) + CGFloat(to.last!)
    }

    /// Result と `side` の欄の、対応する行の区切り（Result の行, 欄の行）。
    private func anchors(_ side: MergeSides.Side?) -> (result: [Int], side: [Int]) {
        var r = [0], s = [0]
        guard let regions = side?.regions else { return (r, s) }
        for i in conflicts.indices {
            guard let j = originalOf[i], j < regions.count, regions[j].count == 2 else { continue }
            let lines = conflicts[i].lines
            // 前の区切りより後ろのものだけ（順が入れ替わった対応は使わない）。
            guard lines[0] >= r.last!, regions[j][0] >= s.last! else { continue }
            r += [lines[0], lines[1]]
            s += [regions[j][0], regions[j][1]]
        }
        return (r, s)
    }

    /// どれかの欄をスクロールしたら、ほかの欄の一番上の行を揃える。
    @objc private func scrolled(_ notification: Notification) {
        leftBand.needsDisplay = true
        rightBand.needsDisplay = true
        guard !syncing, let clip = notification.object as? NSClipView,
              let source = [result, current, incoming, base].first(where: { $0.enclosingScrollView?.contentView === clip })
        else { return }
        syncing = true
        defer { syncing = false }
        let inset = result.textContainerInset.height
        let topLine = (clip.bounds.origin.y - inset) / lineHeight
        // 一度 Result の行に直してから、ほかの欄の行にする。
        let pairs: [(NSTextView, MergeSides.Side?)] = [(current, sides.current), (incoming, sides.incoming), (base, sides.base)]
        let resultLine: CGFloat
        if source === result {
            resultLine = topLine
        } else if let side = pairs.first(where: { $0.0 === source })?.1 {
            let a = anchors(side)
            resultLine = map(topLine, from: a.side, to: a.result)
        } else {
            return
        }
        for (text, side) in pairs where text !== source {
            let a = anchors(side)
            scroll(text, toY: map(resultLine, from: a.result, to: a.side) * lineHeight + inset)
        }
        if source !== result { scroll(result, toY: resultLine * lineHeight + inset) }
    }

    private func scroll(_ text: NSTextView, toY y: CGFloat) {
        guard let scrollView = text.enclosingScrollView else { return }
        let clip = scrollView.contentView
        let maxY = max(0, text.frame.height - clip.bounds.height)
        clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: min(max(0, y), maxY)))
        scrollView.reflectScrolledClipView(clip)
    }

    // MARK: - コンフリクト

    /// Result を読み直して、残っているコンフリクトと左右の欄との対応を決め、強調し直す。
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
        position.stringValue = conflicts.isEmpty ? L("No conflicts") : L("Conflict %ld of %ld", selected + 1, conflicts.count)
        for b in [previousButton, nextButton, magicButton] + acceptButtons { b.isEnabled = !conflicts.isEmpty }
        // 共通の祖先は、対応のわかるコンフリクトでだけ選べる。
        if let baseIndex = acceptButtons.firstIndex(where: { $0.identifier?.rawValue == "base" }) {
            acceptButtons[baseIndex].isEnabled = !conflicts.isEmpty && originalOf[selected] != nil
        }
        // 解決済み（Result に残っていない）のコンフリクトは、左右の欄で薄くする。
        let live = Set(originalOf.compactMap { $0 })
        let chosen = conflicts.isEmpty ? nil : originalOf[selected]
        highlight(current, regions: sides.current?.regions ?? [], live: live, selected: chosen, color: .systemBlue)
        highlight(incoming, regions: sides.incoming?.regions ?? [], live: live, selected: chosen, color: .systemGreen)
        highlight(base, regions: sides.base?.regions ?? [], live: live, selected: chosen, color: .systemGray)
        highlight(result, regions: conflicts.map(\.lines), live: nil, selected: conflicts.isEmpty ? nil : selected, color: .systemRed)
        leftBand.needsDisplay = true
        rightBand.needsDisplay = true
    }

    private func highlight(_ text: NSTextView, regions: [[Int]], live: Set<Int>?, selected: Int?, color: NSColor) {
        guard let layout = text.layoutManager else { return }
        let whole = NSRange(location: 0, length: (text.string as NSString).length)
        layout.removeTemporaryAttribute(.backgroundColor, forCharacterRange: whole)
        let starts = Self.lineStarts(text.string)
        for (i, r) in regions.enumerated() where r.count == 2 {
            let alpha: CGFloat = i == selected ? 0.35 : live.map { $0.contains(i) ? 0.16 : 0.05 } ?? 0.16
            layout.addTemporaryAttribute(
                .backgroundColor, value: color.withAlphaComponent(alpha),
                forCharacterRange: Self.characterRange(lines: r[0]..<r[1], starts: starts))
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

    /// 選んだコンフリクトが Result の上から 3 分の 1 あたりに来るようにスクロールする（ほかの欄は揃う）。
    private func reveal() {
        guard !conflicts.isEmpty, let clip = result.enclosingScrollView?.contentView else { return }
        let line = conflicts[selected].lines[0]
        scroll(result, toY: CGFloat(line) * lineHeight + result.textContainerInset.height - clip.bounds.height / 3)
    }

    @objc func selectPrevious() { move(by: -1) }
    @objc func selectNext() { move(by: 1) }

    private func move(by step: Int) {
        guard !conflicts.isEmpty else { return }
        selected = (selected + step + conflicts.count) % conflicts.count
        refresh()
        reveal()
    }

    @objc private func accept(_ sender: NSButton) {
        guard let choice = sender.identifier?.rawValue else { return }
        resolve(selected, choice)
    }

    /// 帯の ≫・≪ を押した。
    fileprivate func acceptFromBand(_ index: Int, side: ConflictBand.Side) {
        resolve(index, side == .ours ? "ours" : "theirs")
    }

    /// `index` 番目のコンフリクトを `choice` で解決する。Result の変わった部分だけを置き換えて、Undo できるようにする。
    private func resolve(_ index: Int, _ choice: String) {
        guard index < conflicts.count else { return }
        let old = result.string
        do {
            guard let text = try repository.run(["op": "resolveText", "text": old, "index": index, "choice": choice]).text
            else { return }
            selected = index
            replace(old: old, with: text)
        } catch {
            NSAlert(error: error).runModal()
            return
        }
        updateConflicts()
        reveal()
    }

    /// ✨ Magic Merge: 決まった規則で解けるコンフリクトをまとめて解く（Undo でまとめて戻せる）。
    @objc private func magicMerge() {
        let solved = applyMagic(safe: false)
        note.stringValue = solved == 0 ? L("Nothing to resolve") : L("Resolved %ld (⌘Z undoes them one at a time).", solved)
        if solved > 0 { reveal() }
    }

    /// Magic Merge をコンフリクト 1 つずつかける（それぞれ別の Undo にする）。後ろから解くので、前の番号は変わらない。
    /// `safe` なら、間違えようのない規則（両側が同じ、片側だけが変えた）だけ。解いた数を返す。
    @discardableResult
    private func applyMagic(safe: Bool) -> Int {
        struct Info: Decodable { let text: String; let resolved: Int }
        guard let undo = result.undoManager else { return 0 }
        let wasGrouping = undo.groupsByEvent
        undo.groupsByEvent = false
        defer { undo.groupsByEvent = wasGrouping }
        var solved = 0
        for index in conflicts.indices.reversed() {
            let old = result.string
            guard let info = try? repository.info(
                ["op": "magicMerge", "text": old, "safe": safe, "index": index], as: Info.self),
                info.resolved > 0 else { continue }
            undo.beginUndoGrouping()
            replace(old: old, with: info.text)
            undo.setActionName(safe ? L("Magic Merge (on open)") : "Magic Merge")
            undo.endUndoGrouping()
            solved += 1
        }
        if solved > 0 { updateConflicts() }
        return solved
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

    // MARK: - AI

    /// 選んだコンフリクトの両側と共通の祖先の中身。
    private func selectedConflictTexts() -> (ours: String, theirs: String, base: String)? {
        guard !conflicts.isEmpty else { return nil }
        let c = conflicts[selected]
        var base = ""
        if let j = originalOf[selected], let side = sides.base, j < side.regions.count, side.regions[j].count == 2 {
            let lines = side.text.components(separatedBy: "\n")
            let r = side.regions[j]
            base = lines[min(r[0], lines.count)..<min(r[1], lines.count)].joined(separator: "\n")
        }
        return (c.current, c.incoming, base)
    }

    private var assistantKey: String { repository.workdir ?? "remote" }

    @objc private func aiExplain() {
        guard let texts = selectedConflictTexts(), Assistant.confirm(repository: assistantKey, summary: L("One conflict in %@", path))
        else { return NSSound.beep() }
        let path = self.path, workdir = repository.workdir
        AssistantPanel.show(title: L("Conflict Explanation"), over: view.window) { useCache, onText in
            try await Assistant.run(
                .explainConflict, variables: ["path": path, "ours": texts.ours, "theirs": texts.theirs, "base": texts.base],
                workdir: workdir, useCache: useCache, onText: onText)
        }
    }

    /// AI の解消の提案（`resolveConflict` の JSON）。
    private struct Proposal: Decodable { let text: String; let reason: String? }

    /// 返事から JSON を取り出して読む（コードの囲みや前後の文は除く）。
    private static func parseProposal(_ raw: String) -> Proposal? {
        guard let start = raw.firstIndex(of: "{"), let end = raw.lastIndex(of: "}") else { return nil }
        return try? JSONDecoder().decode(Proposal.self, from: Data(raw[start...end].utf8))
    }

    @objc private func aiResolve() {
        guard let texts = selectedConflictTexts(), Assistant.confirm(repository: assistantKey, summary: L("One conflict in %@", path))
        else { return NSSound.beep() }
        let index = selected
        let original = conflicts[index]
        let path = self.path, workdir = repository.workdir
        /// 返事を読めたら、ここに入れる（「Result に入れる」で使う）。
        final class Box { var proposal: Proposal? }
        let box = Box()
        AssistantPanel.show(title: L("Conflict Resolution"), over: view.window, useTitle: L("Put in Result"), onUse: { [weak self] _ in
            guard let self, let proposal = box.proposal else { return }
            applyProposal(proposal.text, to: original)
        }) { useCache, onText in
            let raw = try await Assistant.run(
                .resolveConflict, variables: ["path": path, "ours": texts.ours, "theirs": texts.theirs, "base": texts.base],
                workdir: workdir, useCache: useCache, onText: onText)
            guard let proposal = Self.parseProposal(raw) else {
                throw Assistant.Failure(description: L("Couldn’t read the reply (not JSON in the expected shape):") + "\n\n\(raw)")
            }
            let markers = ["<<<<<<<", "=======", ">>>>>>>", "|||||||"]
            if proposal.text.components(separatedBy: "\n").contains(where: { line in markers.contains { line.hasPrefix($0) } }) {
                throw Assistant.Failure(description: L("The suggestion still has conflict markers. Try again.") + "\n\n\(proposal.text)")
            }
            box.proposal = proposal
            return L("Reason: %@", proposal.reason ?? L("(none)")) + "\n\n" + L("── Suggestion ──") + "\n\(proposal.text)"
        }
    }

    /// 提案で、そのコンフリクト（マーカーを含む行）を置き換える。Undo で戻せる。コンフリクトが動いていれば
    /// 中身で探し直す。
    private func applyProposal(_ text: String, to original: ConflictRegions.Conflict) {
        guard let c = conflicts.first(where: { $0.current == original.current && $0.incoming == original.incoming })
            ?? conflicts.first(where: { $0.lines == original.lines })
        else { return NSSound.beep() }
        let old = result.string
        var lines = old.components(separatedBy: "\n")
        let start = min(c.lines[0], lines.count), end = min(c.lines[1], lines.count)
        var replacement = text.components(separatedBy: "\n")
        if replacement.last == "" { replacement.removeLast() }
        lines.replaceSubrange(start..<end, with: replacement)
        replace(old: old, with: lines.joined(separator: "\n"))
        updateConflicts()
        note.stringValue = L("Put in the AI’s suggestion (⌘Z to undo).")
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

    /// 欄でクリックした位置のコンフリクトを選ぶ。
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
            L("%ld conflicts remain", conflicts.count), message: L("Mark as resolved with the markers left in?"), ok: L("Mark Resolved")) {
            return
        }
        save(stage: true)
    }

    private func save(stage: Bool) {
        let text = result.string
        actions.perform(["op": "saveMerge", "path": path, "text": text, "stage": stage]) { [weak self] _ in
            guard let self else { return }
            savedText = text
            if stage { onResolved?() }
        }
    }
}

/// 上に操作の帯、残りに欄を並べる。
private final class MergeLayoutView: NSView {
    private let bar: NSView
    private let barHeight: CGFloat = 36
    var onLayout: (() -> Void)?

    init(bar: NSView) {
        self.bar = bar
        super.init(frame: .zero)
        addSubview(bar)
        // 幅を制約で決める（frame だと、中身が入りきらないときにスタックがはみ出す）。
        bar.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: topAnchor),
            bar.leadingAnchor.constraint(equalTo: leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: trailingAnchor),
            bar.heightAnchor.constraint(equalToConstant: barHeight),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    /// 欄を並べる領域。
    var contentArea: NSRect { NSRect(x: 0, y: 0, width: bounds.width, height: max(0, bounds.height - barHeight)) }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        onLayout?()
    }
}

/// 左右の欄と Result の間の帯。コンフリクトごとに、欄の行範囲と Result の行範囲を曲線でつなぎ、欄の側に
/// その側で解決する矢印（≫・≪）を出す。
private final class ConflictBand: NSView {
    enum Side { case ours, theirs }
    let side: Side
    weak var owner: MergeEditorViewController?
    /// 矢印の押せる範囲と、そのコンフリクト。
    private var arrows: [(NSRect, Int)] = []

    init(side: Side) {
        self.side = side
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        bounds.fill()
        arrows.removeAll()
        guard let owner else { return }
        let sideText = owner.sideText(side), result = owner.resultText
        let color: NSColor = side == .ours ? .systemBlue : .systemGreen
        for region in owner.bandRegions(side) {
            let s0 = owner.y(ofLine: region.side.lowerBound, in: sideText)
            let s1 = max(s0 + 2, owner.y(ofLine: region.side.upperBound, in: sideText))
            let r0 = owner.y(ofLine: region.result.lowerBound, in: result)
            let r1 = owner.y(ofLine: region.result.upperBound, in: result)
            // 左の帯は左が欄・右が Result、右の帯はその逆。
            let (l0, l1, rr0, rr1) = side == .ours ? (s0, s1, r0, r1) : (r0, r1, s0, s1)
            guard max(l1, rr1) >= 0, min(l0, rr0) <= bounds.height else { continue }
            let w = bounds.width
            let path = NSBezierPath()
            path.move(to: NSPoint(x: 0, y: l0))
            path.curve(to: NSPoint(x: w, y: rr0), controlPoint1: NSPoint(x: w / 2, y: l0), controlPoint2: NSPoint(x: w / 2, y: rr0))
            path.line(to: NSPoint(x: w, y: rr1))
            path.curve(to: NSPoint(x: 0, y: l1), controlPoint1: NSPoint(x: w / 2, y: rr1), controlPoint2: NSPoint(x: w / 2, y: l1))
            path.close()
            color.withAlphaComponent(region.selected ? 0.35 : 0.16).setFill()
            path.fill()
            color.withAlphaComponent(0.5).setStroke()
            path.lineWidth = 1
            path.stroke()
            // 欄の側の上端に、その側で解決する矢印。
            let arrow = side == .ours ? "≫" : "≪"
            let attrs: [NSAttributedString.Key: Any] = [
                .font: NSFont.boldSystemFont(ofSize: 13), .foregroundColor: color,
            ]
            let size = (arrow as NSString).size(withAttributes: attrs)
            let x = side == .ours ? 2 : w - size.width - 2
            let rect = NSRect(x: x, y: s0, width: size.width, height: size.height)
            (arrow as NSString).draw(at: rect.origin, withAttributes: attrs)
            arrows.append((rect.insetBy(dx: -4, dy: -2), region.index))
        }
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard let hit = arrows.first(where: { $0.0.contains(point) }) else { return }
        owner?.acceptFromBand(hit.1, side: side)
    }

    override func resetCursorRects() {
        for (rect, _) in arrows { addCursorRect(rect, cursor: .pointingHand) }
    }
}

/// マージエディタを別ウィンドウで開く。パスごとに 1 つ。
final class MergeEditorWindowController: NSWindowController, NSWindowDelegate {
    private static var open: [String: MergeEditorWindowController] = [:]
    private let editor: MergeEditorViewController
    private let actions: Actions

    static func show(path: String, repository: Repository, actions: Actions) {
        if let existing = open[path] {
            existing.window?.makeKeyAndOrderFront(nil)
            return
        }
        Task {
            do {
                show(try await MergeEditorViewController.load(path: path, repository: repository, actions: actions), actions: actions)
            } catch {
                NSAlert(error: error).runModal()
            }
        }
    }

    /// 埋め込んでいたエディタを、編集中の内容のまま別ウィンドウに移す。
    static func show(_ editor: MergeEditorViewController, actions: Actions) {
        let controller = MergeEditorWindowController(editor: editor, actions: actions)
        open[editor.path] = controller
        controller.showWindow(nil)
    }

    /// 開いているエディタの画像（`--snapshot` と `--merge-editor` の確認用）。
    static func snapshot() -> NSBitmapImageRep? {
        guard let view = open.values.first?.window?.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return nil }
        view.cacheDisplay(in: view.bounds, to: rep)
        return rep
    }

    private init(editor: MergeEditorViewController, actions: Actions) {
        self.editor = editor
        self.actions = actions
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1300, height: 820),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "Merge — \(editor.path)"
        super.init(window: window)
        editor.onPopOut = nil
        editor.onResolved = { [weak self] in self?.close() }
        window.contentViewController = editor
        window.setContentSize(NSSize(width: 1300, height: 820))
        window.center()
        window.delegate = self
    }

    required init?(coder: NSCoder) { fatalError() }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        !editor.hasUnsavedChanges || actions.confirm(
            L("You have unsaved changes"), message: L("Closing discards the changes to Result."), ok: L("Close Without Saving"))
    }

    func windowWillClose(_ notification: Notification) {
        Self.open[editor.path] = nil
    }
}
