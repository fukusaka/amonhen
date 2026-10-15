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
    private let body: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.labelColor]
    private let dim: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor]
    private let mono: [NSAttributedString.Key: Any] = [
        .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular), .foregroundColor: NSColor.secondaryLabelColor,
    ]
    private let subjectAttrs: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 13), .foregroundColor: NSColor.labelColor]
    private let refAttrs: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 11), .foregroundColor: NSColor.systemBlue]

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

    /// 本文。URL・コミット ID・`#123` をリンクにする。
    private func linkedBody(_ string: String) {
        let start = text.length
        add(string, body)
        let range = NSRange(location: start, length: (string as NSString).length)
        let ns = text.string as NSString
        if let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue) {
            for match in detector.matches(in: text.string, range: range) {
                if let url = match.url { text.addAttribute(.link, value: url, range: match.range) }
            }
        }
        // 7〜40 桁の 16 進（前後が英数字でないもの）は、コミット ID とみなす。
        if let regex = try? NSRegularExpression(pattern: "(?<![0-9A-Za-z/])[0-9a-f]{7,40}(?![0-9A-Za-z])") {
            for match in regex.matches(in: text.string, range: range)
            where text.attribute(.link, at: match.range.location, effectiveRange: nil) == nil {
                text.addAttribute(.link, value: Self.revScheme + ns.substring(with: match.range), range: match.range)
            }
        }
        if let web = Self.webURL(extras?.originUrl),
           let regex = try? NSRegularExpression(pattern: "(?<![0-9A-Za-z&])#([0-9]+)\\b") {
            for match in regex.matches(in: text.string, range: range)
            where text.attribute(.link, at: match.range.location, effectiveRange: nil) == nil {
                let number = ns.substring(with: match.range(at: 1))
                let path = web.host?.contains("gitlab") == true ? "-/issues" : "issues"
                text.addAttribute(.link, value: web.appendingPathComponent("\(path)/\(number)"), range: match.range)
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

    private func renderCompact() {
        add(subject + "\n", subjectAttrs)
        if let sig = signatureSummary {
            var attrs = dim
            attrs[.foregroundColor] = sig.color
            attrs[.toolTip] = sig.text
            add(sig.mark + " ", attrs)
        }
        add("\(info.author) · \(relative(info.authorTime)) · ", dim)
        commitLink(info.oid, length: 10)
        if !info.refs.isEmpty {
            add(" · ", dim)
            add(info.refs.joined(separator: ", "), refAttrs)
        }
        if let stats = statsSummary { add(" · " + stats, dim) }
    }

    private func renderStandard() {
        // 件名、メタ情報、本文の順。欄が低くてもメタ情報までは見えるようにする。
        add(subject + "\n", subjectAttrs)
        if !info.refs.isEmpty {
            add(info.refs.joined(separator: ", ") + "\n", refAttrs)
        }
        add("\(info.author) <\(info.authorEmail)>  \(date(info.authorTime))（\(relative(info.authorTime))）", dim)
        if info.committer != info.author {
            add("  committed by \(info.committer)", dim)
        }
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
        let facts = [tagSummary, statsSummary].compactMap { $0 }
        if !facts.isEmpty { add("\n" + facts.joined(separator: " · "), dim) }
        if !bodyText.isEmpty {
            add("\n\n", body)
            linkedBody(bodyText)
        }
        if let notes = extras?.notes, !notes.isEmpty {
            add("\n\nNotes:\n", [.font: NSFont.boldSystemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor])
            linkedBody(notes)
        }
    }

    private func renderDetailed() {
        add(subject + "\n\n", subjectAttrs)
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

        row("Author", plain("\(info.author) <\(info.authorEmail)>"))
        row("Author Date", plain("\(date(info.authorTime)) (\(info.authorTz))  \(relative(info.authorTime))"))
        row("Committer", plain("\(info.committer) <\(info.committerEmail)>"))
        row("Commit Date", plain("\(date(info.committerTime)) (\(info.committerTz))  \(relative(info.committerTime))"))
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
        if info.mergetag { row("Merged Tag", plain("署名付きのタグを merge したコミット")) }
        if !info.refs.isEmpty { row("Refs") { add(info.refs.joined(separator: ", "), refAttrs) } }
        if let extras {
            if !extras.branches.isEmpty { row("Branches", plain(extras.branches.joined(separator: ", "))) }
            if extras.tagCount > 0 {
                let shown = extras.tags.prefix(12).joined(separator: ", ")
                let more = extras.tagCount > 12 ? " ほか \(extras.tagCount - 12) 個" : ""
                row("Tags", plain("\(shown)\(more)"))
            }
            if let previous = extras.previousTag { row("Previous Tag", plain(previous)) }
            if let stats = statsSummary { row("Changes", plain(stats)) }
        } else {
            row("", plain("…"))
        }
        for trailer in info.trailers {
            row(trailer.key, plain(trailer.value))
        }
        if let notes = extras?.notes, !notes.isEmpty { row("Notes", plain(notes)) }
        if !bodyText.isEmpty {
            add("\n", body)
            linkedBody(bodyText)
        }
    }
}
