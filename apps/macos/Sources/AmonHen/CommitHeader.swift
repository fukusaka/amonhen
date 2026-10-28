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
        case .compact: L("Brief")
        case .standard: L("Standard")
        case .detailed: L("Full")
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
    /// 件名と本文の中で強調する語（History の検索の語）。
    var highlight: [String] = []


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
    private func renderMessage(notes: Bool) {
        let messageStart = text.length
        add(subject, subjectAttrs)
        // 本文は長くても畳まずに全文を出す（欄の中でスクロールする。⤢ で欄を広げられる）。
        if !bodyWithoutTrailers.isEmpty {
            add("\n\n", body)
            linkedBody(bodyWithoutTrailers)
        }
        if !info.trailers.isEmpty {
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

    static func date(_ time: Int64) -> String {
        dateFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(time)))
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

    /// 署名の状態。署名がなければ nil。検証の結果が届くまでは `.pending`。
    var signatureStatus: SignatureStatus? {
        guard info.signed != nil || extras?.signature != nil else { return nil }
        guard let extras else { return .pending }
        guard let s = extras.signature else { return info.signed == nil ? nil : .noKey }
        return SignatureStatus(
            status: s.status, trust: s.trust, emails: s.emails, committerEmail: info.recordedCommitterEmail, kind: info.signed)
    }

    /// 署名の行を押すと、詳細の吹き出しを出す。
    static let signatureLink = "amonhen-signature:"

    /// どのリモートブランチにも含まれていなければ「未 push」（リモートがないリポジトリでは出さない）。
    private var isUnpushed: Bool {
        guard let extras, extras.originUrl != nil else { return false }
        return extras.remoteBranches.isEmpty
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
                    L("Cherry-picked from")
                } else if line.contains("reverts commit") {
                    L("Reverts")
                } else if line.hasPrefix("fixes:") {
                    L("Fixes")
                } else {
                    L("Mentions")
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
        link[.toolTip] = L("Show only commits by %@", name)
        add(name, link)
    }

    /// ref の名前を並べる（押すと、その ref の履歴に絞り込む）。
    private func refLinks(_ names: [String], _ attrs: [NSAttributedString.Key: Any]) {
        for (i, name) in names.enumerated() {
            if i > 0 { add(", ", attrs) }
            var link = attrs
            link[.link] = Self.refScheme + name
            link[.toolTip] = L("Show only the history of %@", name)
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

    /// 簡潔: 小さなアバター・作者・作者の日時・ref・親の 1 行と、件名。
    private func renderCompact() {
        avatar(info.authorEmail, name: info.author, size: 16)
        add(" ", dim)
        authorLink(info.author, [.font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.labelColor])
        add("  \(date(info.authorTime))", dim)
        if !info.refs.isEmpty {
            add("  ", dim)
            refLinks(info.refs, refAttrs)
        }
        if !info.parents.isEmpty {
            add(info.parents.count > 1 ? "  Parents " : "  Parent ", dim)
            commitLinks(info.parents)
        }
        add("\n", dim)
        let start = text.length
        add(subject, subjectAttrs)
        highlightTerms(from: start)
    }

    /// 標準: 詳細と同じ形で、項目を絞ったもの（作者とコミッターは上の帯）。
    private func renderStandard() {
        let table = rows()
        table.row("Commit") { add(info.oid, mono) }
        if !info.parents.isEmpty { table.row(info.parents.count > 1 ? "Parents" : "Parent") { fullCommitLinks(info.parents) } }
        signatureRow(table)
        if !info.refs.isEmpty { table.row("Refs") { refLinks(info.refs, refAttrs) } }
        tagRows(table)
        mergedTagRows(table)
        if let extras, extras.mergedCount > 0 {
            table.row("Merged") { add(L("%ld commits", extras.mergedCount), value) }
        }
        add("\n", dim)
        renderMessage(notes: true)
    }

    /// 詳細: 項目ごとの表（作者とコミッターは、上の帯に左右に並べる）。
    private func renderDetailed() {
        let table = rows()
        table.row("Commit") { add(info.oid, mono) }
        if !info.parents.isEmpty { table.row(info.parents.count > 1 ? "Parents" : "Parent") { fullCommitLinks(info.parents) } }
        if let children = extras?.children, !children.isEmpty {
            table.row(children.count > 1 ? "Children" : "Child") { fullCommitLinks(children) }
        }
        if let rev = extras?.headRev, !rev.isEmpty { table.row("Rev") { add(rev, mono) } }
        signatureRow(table)
        if !info.refs.isEmpty { table.row("Refs") { refLinks(info.refs, refAttrs) } }
        tagRows(table)
        if let extras {
            if !extras.branches.isEmpty { table.row("Branches") { refLinks(extras.branches, value) } }
            if extras.tagCount > 0 {
                let shown = extras.tags.prefix(12).joined(separator: ", ")
                let more = extras.tagCount > 12 ? " " + L("and %ld more tags", extras.tagCount - 12) : ""
                table.row("Tags") { add(shown + more, value) }
            }
            if let previous = extras.previousTag { table.row("Previous Tag") { add(previous, value) } }
            if extras.originUrl != nil {
                table.row("Pushed") {
                    if isUnpushed {
                        add(L("Not pushed (not on any remote branch)"), unpushedAttrs)
                    } else {
                        refLinks(extras.remoteBranches, value)
                    }
                }
            }
        }
        mergedTagRows(table)
        if let extras, extras.mergedCount > 0 {
            table.row("Merged") {
                add(L("%ld commits", extras.mergedCount), value)
                for commit in extras.merged.prefix(20) {
                    add("\u{2028}", value)
                    summaryLine(commit)
                }
                if extras.mergedCount > 20 { add("\u{2028}" + L("and %ld more", extras.mergedCount - 20), dim) }
            }
        }
        for (kind, commit) in relatedCommits {
            table.row("Related") {
                add("\(kind)  ", dim)
                summaryLine(commit)
            }
        }
        add("\n", dim)
        renderMessage(notes: true)
    }

    // MARK: - 表の部品

    /// 表の値の文字。
    private var value: [NSAttributedString.Key: Any] {
        [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.labelColor]
    }

    /// 見出しを右揃えにした 2 列の表。値が折り返したら値の列に揃える。
    private struct Rows {
        let renderer: CommitHeaderRenderer
        let paragraph: NSParagraphStyle

        func row(_ label: String, _ content: () -> Void) {
            let text = renderer.text
            let start = text.length
            text.append(NSAttributedString(string: "\t\(label)\t", attributes: [
                .font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.secondaryLabelColor,
            ]))
            content()
            text.append(NSAttributedString(string: "\n"))
            text.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: start, length: text.length - start))
        }
    }

    private func rows() -> Rows {
        let labelEnd: CGFloat = 84, valueStart: CGFloat = 94
        let paragraph = NSMutableParagraphStyle()
        paragraph.tabStops = [
            NSTextTab(textAlignment: .right, location: labelEnd),
            NSTextTab(textAlignment: .left, location: valueStart),
        ]
        paragraph.headIndent = valueStart
        paragraph.paragraphSpacing = 2
        return Rows(renderer: self, paragraph: paragraph)
    }

    /// アバターの画像（丸）。読み終えていなければ頭文字の丸。
    private func avatar(_ email: String, name: String, size: CGFloat) {
        guard Settings.showAvatars else { return }
        let attachment = NSTextAttachment()
        attachment.image = Avatars.shared.circle(for: email, name: name, size: size)
        // 文字の高さの中ほどに置く。
        attachment.bounds = NSRect(x: 0, y: size > 20 ? -size + 12 : -4, width: size, height: size)
        text.append(NSAttributedString(attachment: attachment))
    }

    /// 40 桁の ID を 1 行ずつ（選んでコピーでき、押すとそのコミットへ移る）。
    private func fullCommitLinks(_ ids: [String]) {
        for (i, id) in ids.enumerated() {
            if i > 0 { add("\u{2028}", mono) }
            commitLink(id, length: 40)
        }
    }

    /// 署名を 1 行に: アイコン・短い言葉・信用度の目盛り・方式と署名者。押すと詳細の吹き出し。
    private func signatureRow(_ table: Rows) {
        guard let status = signatureStatus else { return }
        table.row("Signature") {
            signatureMark(status, link: Self.signatureLink, kind: info.signed, signer: extras?.signature?.signer ?? "", tag: false)
        }
    }

    /// 署名の印（アイコン・短い言葉・信用度の目盛り・方式と署名者）。アイコンと言葉は `link` で、押すと詳細の吹き出し。
    private func signatureMark(_ status: SignatureStatus, link: String, kind: String?, signer: String, tag: Bool) {
        let start = text.length
        if let image = Self.symbolImage(status.symbol, color: status.color, size: 13) {
            let attachment = NSTextAttachment()
            attachment.image = image
            attachment.bounds = NSRect(x: 0, y: -2.5, width: image.size.width, height: image.size.height)
            text.append(NSAttributedString(attachment: attachment))
        }
        add(" \(status.label)", [.font: NSFont.systemFont(ofSize: 11, weight: .semibold), .foregroundColor: status.color])
        text.addAttributes(
            [.link: link, .toolTip: status.toolTip(tag: tag), .cursor: NSCursor.pointingHand],
            range: NSRange(location: start, length: text.length - start))
        if let bars = status.trustBars {
            add(" ", value)
            let attachment = NSTextAttachment()
            let meter = Self.trustMeter(bars, color: status.color)
            attachment.image = meter
            attachment.bounds = NSRect(x: 0, y: -1, width: meter.size.width, height: meter.size.height)
            text.append(NSAttributedString(attachment: attachment))
        }
        let kind = (kind ?? "gpg").uppercased()
        add("  \(kind)" + (signer.isEmpty ? "" : " · \(signer)"), dim)
    }

    /// タグの署名の状態。署名がなければ nil。詳細の時間のかかる分（検証の結果）が届くまでは `.pending`。
    static func tagSignatureStatus(signed: String?, signature: CommitExtras.Signature?, loaded: Bool, taggerEmail: String) -> SignatureStatus? {
        guard let signed else { return nil }
        guard loaded else { return .pending }
        guard let s = signature else { return .noKey }
        return SignatureStatus(status: s.status, trust: s.trust, emails: s.emails, committerEmail: taggerEmail, kind: signed)
    }

    /// タグの署名の印を押すと、詳細の吹き出し（`<n>` はこのコミットを指すタグ、merge したタグの何番目か）。
    static let tagSignatureLink = "amonhen-tag-signature:"
    static let mergedTagSignatureLink = "amonhen-merged-tag-signature:"

    /// このコミットを指すタグを 1 つずつ: 名前・タガーと日時・件名と署名の印。軽量タグは名前だけ。
    /// 詳細の時間のかかる分（`tagObjects`）が届いてから出す（それまでは Refs の行に名前がある）。
    private func tagRows(_ table: Rows) {
        guard let extras else { return }
        for (i, tag) in extras.tagObjects.enumerated() {
            table.row("Tag") {
                add(tag.name, [.font: NSFont.systemFont(ofSize: 11, weight: .semibold), .foregroundColor: NSColor.labelColor])
                guard tag.id != nil else {
                    add(L(" (lightweight tag)"), dim)
                    return
                }
                let who = [tag.tagger, tag.time.map(date)].compactMap { $0 }.filter { !$0.isEmpty }
                if !who.isEmpty { add(" (\(who.joined(separator: " · ")))", dim) }
                if let subject = tag.subject, !subject.isEmpty { add(" — \(subject)", dim) }
                if let status = Self.tagSignatureStatus(signed: tag.signed, signature: tag.signature, loaded: true, taggerEmail: tag.taggerEmail ?? "") {
                    add("  ", value)
                    signatureMark(status, link: Self.tagSignatureLink + "\(i)", kind: tag.signed, signer: tag.signature?.signer ?? "", tag: true)
                }
            }
        }
    }

    /// 色を付けた SF Symbols の画像（文字の中に置く）。形に色を塗るだけにして、塗りつぶしの seal の中の
    /// チェックや × は抜いたまま残す（色を指定して描かせると、中の印まで同じ色になって塊に見える）。
    static func symbolImage(_ name: String, color: NSColor, size: CGFloat) -> NSImage? {
        let config = NSImage.SymbolConfiguration(pointSize: size, weight: .medium)
        guard let symbol = NSImage(systemSymbolName: name, accessibilityDescription: nil)?.withSymbolConfiguration(config)
        else { return nil }
        return NSImage(size: symbol.size, flipped: false) { rect in
            symbol.draw(in: rect)
            color.set()
            rect.fill(using: .sourceAtop)
            return true
        }
    }

    /// 信用度の 3 段の目盛り（`bars` 段を色で塗る）。
    static func trustMeter(_ bars: Int, color: NSColor) -> NSImage {
        let width: CGFloat = 4, height: CGFloat = 10, gap: CGFloat = 2
        return NSImage(size: NSSize(width: width * 3 + gap * 2, height: height), flipped: false) { _ in
            for i in 0..<3 {
                let rect = NSRect(x: CGFloat(i) * (width + gap), y: 0, width: width, height: height)
                (i < bars ? color : NSColor.tertiaryLabelColor.withAlphaComponent(0.4)).setFill()
                NSBezierPath(roundedRect: rect, xRadius: 1.5, yRadius: 1.5).fill()
            }
            return true
        }
    }

    private func mergedTagRows(_ table: Rows) {
        for (i, tag) in info.mergedTags.enumerated() {
            table.row("Merged Tag") {
                add("\(tag.name)", value)
                if !tag.tagger.isEmpty { add(" (\(tag.tagger))", dim) }
                if !tag.subject.isEmpty { add(" — \(tag.subject)", dim) }
                let signature = extras?.mergedTagSignatures.indices.contains(i) == true ? extras?.mergedTagSignatures[i] : nil
                if let status = Self.tagSignatureStatus(signed: tag.signed, signature: signature, loaded: extras != nil, taggerEmail: tag.taggerEmail) {
                    add("  ", value)
                    signatureMark(status, link: Self.mergedTagSignatureLink + "\(i)", kind: tag.signed, signer: signature?.signer ?? "", tag: true)
                }
            }
        }
    }
}
