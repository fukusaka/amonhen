import AmonhenKit
import AppKit

/// コミットの詳細の上の欄（メッセージとメタ情報）の見せ方。
enum CommitHeaderStyle: Int, CaseIterable {
    /// 件名と 1 行のメタ情報。差分を広く見たいとき。
    case compact
    /// 件名・ref・作者・ID・署名・含むタグ・行数と本文。
    case standard
    /// 項目ごとの表と本文。
    case detailed

    var title: String {
        switch self {
        case .compact: "簡潔"
        case .standard: "標準"
        case .detailed: "詳細"
        }
    }

    static var saved: CommitHeaderStyle {
        get { CommitHeaderStyle(rawValue: UserDefaults.standard.integer(forKey: "commitHeaderStyle")) ?? .standard }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "commitHeaderStyle") }
    }
}

/// コミットの情報を、見せ方に合わせた文字列にする。`extras` は裏で読み終えてから渡す（それまでは nil）。
struct CommitHeaderRenderer {
    let info: CommitInfo
    let extras: CommitExtras?
    let style: CommitHeaderStyle

    /// リンクの URL の形。`amonhen-commit:<40 桁>` は History で選び、`amonhen-rev:<短い ID>` は引いてから選ぶ。
    static let commitScheme = "amonhen-commit:"
    static let revScheme = "amonhen-rev:"
    /// 作者で History を絞り込む・ref の履歴に絞り込むリンク。
    static let authorScheme = "amonhen-author:"
    static let refScheme = "amonhen-ref:"

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .medium
        return f
    }()

    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .full
        return f
    }()

    private let text = NSMutableAttributedString()
    private var body: [NSAttributedString.Key: Any] {
        [.font: monospacedBody ? NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular) : NSFont.systemFont(ofSize: 12),
         .foregroundColor: NSColor.labelColor]
    }
    private let dim: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor]
    private let mono: [NSAttributedString.Key: Any] = [
        .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular), .foregroundColor: NSColor.secondaryLabelColor,
    ]
    private let subjectAttrs: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 13), .foregroundColor: NSColor.labelColor]
    private let refAttrs: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 11), .foregroundColor: NSColor.systemBlue]

    /// 本文を等幅で出すか（git の慣習の 72 桁の改行が崩れない）。
    var monospacedBody = true
    /// 長い本文を折りたたまずに出すか。
    var expanded = false
    /// 件名と本文の中で強調する語（History の検索の語）。
    var highlight: [String] = []

    /// 本文がこの行数を超えたら、`collapsedLines` 行で折りたたむ。
    static let collapseAbove = 16
    static let collapsedLines = 12
    /// 「続きを表示」「折りたたむ」のリンク。
    static let expandLink = "amonhen-expand:"

    init(info: CommitInfo, extras: CommitExtras?, style: CommitHeaderStyle) {
        self.info = info
        self.extras = extras
        self.style = style
    }

    func render() -> NSAttributedString {
        switch style {
        case .compact: renderCompact()
        case .standard: renderStandard()
        case .detailed: renderDetailed()
        }
        return text
    }

    /// 件名・本文（トレーラーは除く）・トレーラー・notes。どの見せ方でも最後に置く。
    private func renderMessage(trailers: Bool, notes: Bool) {
        let messageStart = text.length
        add(subject, subjectAttrs)
        let lines = bodyWithoutTrailers.components(separatedBy: "\n")
        if !bodyWithoutTrailers.isEmpty {
            add("\n\n", body)
            let collapse = lines.count > Self.collapseAbove && !expanded
            linkedBody(collapse ? lines.prefix(Self.collapsedLines).joined(separator: "\n") : bodyWithoutTrailers)
            if lines.count > Self.collapseAbove {
                var link = dim
                link[.link] = Self.expandLink
                add("\n", body)
                add(collapse ? "続きを表示（あと \(lines.count - Self.collapsedLines) 行）" : "折りたたむ", link)
            }
        }
        if trailers, !info.trailers.isEmpty {
            add("\n", body)
            for trailer in info.trailers {
                add("\n\(trailer.key): ", [.font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.secondaryLabelColor])
                let start = text.length
                add(trailer.value, dim)
                linkURLs(in: NSRange(location: start, length: text.length - start))
            }
        }
        if notes, let notes = extras?.notes, !notes.isEmpty {
            add("\n\nNotes:\n", [.font: NSFont.boldSystemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor])
            linkedBody(notes)
        }
        highlightTerms(from: messageStart)
    }

    /// 本文から、最後の段落のトレーラー（`Signed-off-by:` など）を除いたもの。トレーラーは別に並べる。
    private var bodyWithoutTrailers: String {
        let body = bodyText
        guard !info.trailers.isEmpty else { return body }
        guard let split = body.range(of: "\n\n", options: .backwards) else {
            // 本文がトレーラーだけ。
            return ""
        }
        return String(body[..<split.lowerBound]).trimmingCharacters(in: .newlines)
    }

    /// `from` から後ろで、検索の語に背景の色を付ける（大文字・小文字は区別しない）。
    private func highlightTerms(from start: Int) {
        let ns = text.string as NSString
        let range = NSRange(location: start, length: ns.length - start)
        for term in highlight where !term.isEmpty {
            var search = range
            while true {
                let found = ns.range(of: term, options: .caseInsensitive, range: search)
                guard found.location != NSNotFound else { break }
                text.addAttribute(.backgroundColor, value: NSColor.systemYellow.withAlphaComponent(0.35), range: found)
                search = NSRange(location: found.upperBound, length: range.upperBound - found.upperBound)
            }
        }
    }

    /// History の検索の問い合わせから、件名と本文で強調する語を取り出す（`author:`・`path:` などは除く）。
    static func highlightTerms(of query: String?) -> [String] {
        guard let query else { return [] }
        var terms: [String] = []
        var word = "", quoted = false
        func flush() {
            defer { word = "" }
            let lower = word.lowercased()
            guard !word.isEmpty, lower != "or", lower != "and" else { return }
            if let colon = word.firstIndex(of: ":") {
                let key = word[..<colon].lowercased(), value = String(word[word.index(after: colon)...])
                if ["subject", "message"].contains(key), !value.isEmpty { terms.append(value) }
                if !["author", "commit", "id", "path", "file", "contents", "content", "subject", "message"].contains(key) {
                    terms.append(word)
                }
            } else {
                terms.append(word)
            }
        }
        for c in query {
            if c == "\"" {
                quoted.toggle()
            } else if !quoted, c.isWhitespace || "()|".contains(c) {
                flush()
            } else {
                word.append(c)
            }
        }
        flush()
        return terms
    }

    // MARK: - 部品

    private func add(_ s: String, _ attrs: [NSAttributedString.Key: Any]) {
        text.append(NSAttributedString(string: s, attributes: attrs))
    }

    private var message: String { info.message.trimmingCharacters(in: .whitespacesAndNewlines) }

    private var subject: String {
        message.firstIndex(of: "\n").map { String(message[..<$0]) } ?? message
    }

    private var bodyText: String {
        guard let newline = message.firstIndex(of: "\n") else { return "" }
        return String(message[newline...]).trimmingCharacters(in: .newlines)
    }

    private func date(_ time: Int64) -> String {
        Self.dateFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(time)))
    }

    private func relative(_ time: Int64) -> String {
        Self.relativeFormatter.localizedString(for: Date(timeIntervalSince1970: TimeInterval(time)), relativeTo: Date())
    }

    private func commitLink(_ hex: String, length: Int = 12) {
        var link = mono
        link[.link] = Self.commitScheme + hex
        link[.foregroundColor] = NSColor.linkColor
        add(String(hex.prefix(length)), link)
    }

    private func commitLinks(_ ids: [String]) {
        for (i, id) in ids.enumerated() {
            if i > 0 { add(", ", mono) }
            commitLink(id)
        }
    }

    /// 署名の状態の短い言い方と色。
    private var signatureSummary: (mark: String, text: String, color: NSColor)? {
        let kind = (info.signed ?? "gpg").uppercased()
        if let s = extras?.signature {
            switch s.status {
            case "G": return ("✓", "正しい \(kind) 署名", .systemGreen)
            case "U": return ("✓", "正しい \(kind) 署名（鍵の信用度は不明）", .systemGreen)
            case "X": return ("!", "期限切れの \(kind) 署名", .systemOrange)
            case "Y": return ("!", "期限切れの鍵による \(kind) 署名", .systemOrange)
            case "R": return ("!", "失効した鍵による \(kind) 署名", .systemOrange)
            case "B": return ("✗", "不正な \(kind) 署名", .systemRed)
            default: return ("?", "\(kind) 署名（検証できない。鍵がないなど）", .secondaryLabelColor)
            }
        }
        guard info.signed != nil else { return nil }
        return ("…", "\(kind) 署名（検証中）", .secondaryLabelColor)
    }

    private var statsSummary: String? {
        guard let stats = extras?.stats, !stats.isEmpty else { return nil }
        let added = stats.compactMap(\.added).reduce(0, +)
        let removed = stats.compactMap(\.removed).reduce(0, +)
        return "\(stats.count) ファイル  +\(added) −\(removed)"
    }

    private var tagSummary: String? {
        guard let extras else { return nil }
        var parts: [String] = []
        if let first = extras.firstTag { parts.append("\(first) で初めて入った") }
        if let previous = extras.previousTag { parts.append("直前のタグ \(previous)") }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// HEAD との位置。詳細を読み終えるまでは nil。
    private var positionSummary: String? {
        guard let extras else { return nil }
        switch extras.headDistance {
        case 0: return "HEAD"
        case let n?: return "HEAD の \(n) コミット前"
        case nil: return "HEAD に含まれない"
        }
    }

    /// どのリモートブランチにも含まれていなければ「未 push」（リモートがないリポジトリでは出さない）。
    private var isUnpushed: Bool {
        guard let extras, extras.originUrl != nil else { return false }
        return extras.remoteBranches.isEmpty
    }

    /// 作者の日時からコミットの日時まで 1 時間より空いていれば、書き直した（rebase・amend・cherry-pick）とみなす。
    private var rewriteSummary: String? {
        let gap = info.committerTime - info.authorTime
        guard gap > 3600 else { return nil }
        let f = DateComponentsFormatter()
        f.unitsStyle = .full
        f.maximumUnitCount = 1
        f.allowedUnits = [.year, .month, .day, .hour]
        guard let span = f.string(from: TimeInterval(gap)) else { return nil }
        return "作成の \(span)後にコミット（rebase・amend・cherry-pick など）"
    }

    /// `Co-authored-by` のトレーラーの名前。
    private var coAuthors: [String] {
        info.trailers
            .filter { $0.key.lowercased() == "co-authored-by" }
            .map { $0.value.components(separatedBy: " <").first ?? $0.value }
    }

    /// メッセージに書かれたコミットを、書かれた順に、書かれ方（cherry-pick・revert・Fixes）と一緒に。
    private var relatedCommits: [(kind: String, commit: CommitExtras.CommitSummary)] {
        guard let references = extras?.references, !references.isEmpty else { return [] }
        let message = info.message
        return references
            .compactMap { token, commit -> (Int, String, CommitExtras.CommitSummary)? in
                guard let range = message.range(of: token) else { return nil }
                let line = message[message.lineRange(for: range)].lowercased()
                let kind = if line.contains("cherry picked from") {
                    "cherry-pick 元"
                } else if line.contains("reverts commit") {
                    "revert 元"
                } else if line.hasPrefix("fixes:") {
                    "修正対象"
                } else {
                    "参照"
                }
                return (message.distance(from: message.startIndex, to: range.lowerBound), kind, commit)
            }
            .sorted { $0.0 < $1.0 }
            .map { ($0.1, $0.2) }
    }

    /// 作者の名前（押すと、その人のコミットで絞り込む）。
    private func authorLink(_ name: String, _ attrs: [NSAttributedString.Key: Any]) {
        var link = attrs
        link[.link] = Self.authorScheme + name
        link[.toolTip] = "\(name) のコミットで絞り込む"
        add(name, link)
    }

    /// ref の名前を並べる（押すと、その ref の履歴に絞り込む）。
    private func refLinks(_ names: [String], _ attrs: [NSAttributedString.Key: Any]) {
        for (i, name) in names.enumerated() {
            if i > 0 { add(", ", attrs) }
            var link = attrs
            link[.link] = Self.refScheme + name
            link[.toolTip] = "\(name) の履歴に絞り込む"
            add(name, link)
        }
    }

    private func summaryLine(_ commit: CommitExtras.CommitSummary) {
        commitLink(commit.oid, length: 10)
        add("  \(commit.subject)", [.font: NSFont.systemFont(ofSize: 11.5), .foregroundColor: NSColor.labelColor])
        if let author = commit.author { add(" — \(author)", dim) }
    }

    private var unpushedAttrs: [NSAttributedString.Key: Any] {
        [.font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.systemOrange]
    }

    /// 本文。URL・コミット ID・`#123` をリンクにする。
    private func linkURLs(in range: NSRange) {
        if let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue) {
            for match in detector.matches(in: text.string, range: range) {
                if let url = match.url {
                    text.addAttributes([.link: url, .foregroundColor: NSColor.linkColor], range: match.range)
                }
            }
        }
    }

    private func linkedBody(_ string: String) {
        let start = text.length
        add(string, body)
        let range = NSRange(location: start, length: (string as NSString).length)
        let ns = text.string as NSString
        linkURLs(in: range)
        // 7〜40 桁の 16 進（前後が英数字でないもの）は、コミット ID とみなす。
        if let regex = try? NSRegularExpression(pattern: "(?<![0-9A-Za-z/])[0-9a-f]{7,40}(?![0-9A-Za-z])") {
            for match in regex.matches(in: text.string, range: range)
            where text.attribute(.link, at: match.range.location, effectiveRange: nil) == nil {
                text.addAttributes(
                    [.link: Self.revScheme + ns.substring(with: match.range), .foregroundColor: NSColor.linkColor], range: match.range)
            }
        }
        if let web = Self.webURL(extras?.originUrl),
           let regex = try? NSRegularExpression(pattern: "(?<![0-9A-Za-z&])#([0-9]+)\\b") {
            for match in regex.matches(in: text.string, range: range)
            where text.attribute(.link, at: match.range.location, effectiveRange: nil) == nil {
                let number = ns.substring(with: match.range(at: 1))
                let path = web.host?.contains("gitlab") == true ? "-/issues" : "issues"
                text.addAttributes(
                    [.link: web.appendingPathComponent("\(path)/\(number)"), .foregroundColor: NSColor.linkColor], range: match.range)
            }
        }
    }

    /// `git@github.com:a/b.git`・`ssh://git@host/a/b`・`https://host/a/b.git` を、ブラウザで開ける URL にする。
    static func webURL(_ remote: String?) -> URL? {
        guard var s = remote?.trimmingCharacters(in: .whitespaces), !s.isEmpty else { return nil }
        if s.hasSuffix(".git") { s.removeLast(4) }
        if s.hasPrefix("http://") || s.hasPrefix("https://") {
            return URL(string: s)
        }
        if s.hasPrefix("ssh://") {
            s.removeFirst("ssh://".count)
            guard let slash = s.firstIndex(of: "/") else { return nil }
            var host = String(s[..<slash])
            if let at = host.lastIndex(of: "@") { host = String(host[host.index(after: at)...]) }
            if let colon = host.firstIndex(of: ":") { host = String(host[..<colon]) }
            return URL(string: "https://\(host)\(s[slash...])")
        }
        // scp の形（user@host:path）。
        guard let colon = s.firstIndex(of: ":"), !s.contains("://") else { return nil }
        var host = String(s[..<colon])
        if let at = host.lastIndex(of: "@") { host = String(host[host.index(after: at)...]) }
        let path = s[s.index(after: colon)...].trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        return URL(string: "https://\(host)/\(path)")
    }

    // MARK: - 見せ方

    // どの見せ方も、メタ情報、件名、本文の残りの順に並べる。

    private func renderCompact() {
        if let sig = signatureSummary {
            var attrs = dim
            attrs[.foregroundColor] = sig.color
            attrs[.toolTip] = sig.text
            add(sig.mark + " ", attrs)
        }
        authorLink(info.author, dim)
        add(" · \(relative(info.authorTime)) · ", dim)
        commitLink(info.oid, length: 10)
        if !info.refs.isEmpty {
            add(" · ", dim)
            refLinks(info.refs, refAttrs)
        }
        if let stats = statsSummary { add(" · " + stats, dim) }
        if isUnpushed { add(" · 未 push", unpushedAttrs) }
        add("\n", dim)
        let start = text.length
        add(subject, subjectAttrs)
        highlightTerms(from: start)
    }

    private func renderStandard() {
        if !info.refs.isEmpty {
            refLinks(info.refs, refAttrs)
            add("\n", refAttrs)
        }
        authorLink(info.author, dim)
        add(" <\(info.authorEmail)>  \(date(info.authorTime))（\(relative(info.authorTime))）", dim)
        if info.committer != info.author {
            add("  committed by \(info.committer)", dim)
        }
        if !coAuthors.isEmpty { add("\n共同作者: " + coAuthors.joined(separator: ", "), dim) }
        if let sig = signatureSummary {
            var attrs = dim
            attrs[.foregroundColor] = sig.color
            add("\n\(sig.mark) \(sig.text)", attrs)
            if let signer = extras?.signature?.signer, !signer.isEmpty { add(" — \(signer)", dim) }
        }
        add("\n\(info.oid)", mono)
        if !info.parents.isEmpty {
            add(info.parents.count > 1 ? "  Parents " : "  Parent ", mono)
            commitLinks(info.parents)
        }
        if let children = extras?.children, !children.isEmpty {
            add(children.count > 1 ? "  Children " : "  Child ", mono)
            commitLinks(children)
        }
        let facts = [positionSummary, tagSummary, statsSummary].compactMap { $0 }
        if !facts.isEmpty { add("\n" + facts.joined(separator: " · "), dim) }
        if isUnpushed { add(facts.isEmpty ? "\n未 push" : " · 未 push", unpushedAttrs) }
        if let rewrite = rewriteSummary { add("\n" + rewrite, dim) }
        for tag in info.mergedTags {
            add("\n\(tag.signed ? "署名付きの" : "")タグ \(tag.name)（\(tag.tagger)）を merge", dim)
            if !tag.subject.isEmpty { add(" — \(tag.subject)", dim) }
        }
        if let extras, extras.mergedCount > 0 {
            add("\n取り込んだコミット \(extras.mergedCount) 件", dim)
            for commit in extras.merged.prefix(5) {
                add("\n  ", dim)
                summaryLine(commit)
            }
            if extras.mergedCount > 5 { add("\n  ほか \(extras.mergedCount - 5) 件", dim) }
        }
        for (kind, commit) in relatedCommits {
            add("\n\(kind): ", dim)
            summaryLine(commit)
        }
        add("\n\n", dim)
        renderMessage(trailers: true, notes: true)
    }

    private func renderDetailed() {
        // 見出しの列を揃え、値が折り返したら値の列に揃える。
        let paragraph = NSMutableParagraphStyle()
        let labelWidth: CGFloat = 92
        paragraph.tabStops = [NSTextTab(textAlignment: .left, location: labelWidth)]
        paragraph.headIndent = labelWidth
        paragraph.paragraphSpacing = 2
        let labelAttrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.secondaryLabelColor,
            .paragraphStyle: paragraph,
        ]
        func row(_ label: String, _ value: () -> Void) {
            let start = text.length
            add(label + "\t", labelAttrs)
            value()
            add("\n", [:])
            // 値の中の改行の後も、値の列に揃える。
            text.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: start, length: text.length - start))
        }
        // 値の中の改行は行の区切り（U+2028）にする。段落が分かれると、値の列に揃わない。
        func plain(_ s: String) -> () -> Void {
            { add(s.replacingOccurrences(of: "\n", with: "\u{2028}"), [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.labelColor]) }
        }

        row("Author") {
            authorLink(info.author, [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.labelColor])
            plain(" <\(info.authorEmail)>")()
        }
        row("Author Date", plain("\(date(info.authorTime)) (\(info.authorTz))  \(relative(info.authorTime))"))
        row("Committer", plain("\(info.committer) <\(info.committerEmail)>"))
        row("Commit Date", plain("\(date(info.committerTime)) (\(info.committerTz))  \(relative(info.committerTime))"))
        if !coAuthors.isEmpty { row("Co-authors", plain(coAuthors.joined(separator: ", "))) }
        if let rewrite = rewriteSummary { row("Rewritten", plain(rewrite)) }
        row("Commit") { add(info.oid, mono) }
        if !info.parents.isEmpty { row(info.parents.count > 1 ? "Parents" : "Parent") { commitLinks(info.parents) } }
        if let children = extras?.children, !children.isEmpty {
            row(children.count > 1 ? "Children" : "Child") { commitLinks(children) }
        }
        if let sig = signatureSummary {
            row("Signature") {
                var attrs = dim
                attrs[.foregroundColor] = sig.color
                attrs[.font] = NSFont.systemFont(ofSize: 11)
                add("\(sig.mark) \(sig.text)", attrs)
                if let s = extras?.signature {
                    var details = [s.signer]
                    if !s.key.isEmpty { details.append("鍵 \(s.key)") }
                    if !s.trust.isEmpty { details.append("信用 \(s.trust)") }
                    add("\u{2028}" + details.filter { !$0.isEmpty }.joined(separator: " · "), dim)
                }
            }
        }
        for tag in info.mergedTags {
            let subject = tag.subject.isEmpty ? "" : "\n\(tag.subject)"
            row("Merged Tag", plain("\(tag.name)（\(tag.tagger)、\(tag.signed ? "署名付き" : "署名なし")）\(subject)"))
        }
        if !info.refs.isEmpty { row("Refs") { refLinks(info.refs, refAttrs) } }
        if let extras {
            if !extras.branches.isEmpty {
                row("Branches") { refLinks(extras.branches, [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.labelColor]) }
            }
            if extras.tagCount > 0 {
                let shown = extras.tags.prefix(12).joined(separator: ", ")
                let more = extras.tagCount > 12 ? " ほか \(extras.tagCount - 12) 個" : ""
                row("Tags", plain("\(shown)\(more)"))
            }
            if let previous = extras.previousTag { row("Previous Tag", plain(previous)) }
            if let position = positionSummary { row("Position", plain(position)) }
            if extras.originUrl != nil {
                row("Pushed") {
                    if isUnpushed {
                        add("未 push（どのリモートブランチにも含まれない）", unpushedAttrs)
                    } else {
                        refLinks(extras.remoteBranches, [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.labelColor])
                    }
                }
            }
            if extras.mergedCount > 0 {
                row("Merged") {
                    plain("\(extras.mergedCount) コミット")()
                    for commit in extras.merged.prefix(20) {
                        add("\u{2028}", dim)
                        summaryLine(commit)
                    }
                    if extras.mergedCount > 20 { add("\u{2028}ほか \(extras.mergedCount - 20) 件", dim) }
                }
            }
            for (kind, commit) in relatedCommits {
                row("Related") {
                    add("\(kind)  ", dim)
                    summaryLine(commit)
                }
            }
            if let stats = statsSummary { row("Changes", plain(stats)) }
        } else {
            row("", plain("…"))
        }
        for trailer in info.trailers {
            row(trailer.key, plain(trailer.value))
        }
        if let notes = extras?.notes, !notes.isEmpty { row("Notes", plain(notes)) }
        add("\n", dim)
        renderMessage(trailers: false, notes: false)
    }
}
