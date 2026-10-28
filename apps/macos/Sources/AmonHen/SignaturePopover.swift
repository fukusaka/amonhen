import AmonhenKit
import AppKit

/// 署名の詳細の吹き出し（GitHub の Verified の吹き出しに倣う）。状態・説明・署名者・コミッター（タグはタガー）との
/// 一致・方式・鍵 ID・指紋・信用度・検証した場所。値は選んでコピーできる。
final class SignaturePopover: NSViewController {
    /// タグの署名なら、タグの名前とメッセージ（見出しとメッセージの行に出す）。
    struct Tag {
        var name: String
        var message: String
    }

    private let status: SignatureStatus
    private let signature: CommitExtras.Signature?
    private let kind: String
    /// 署名した鍵と比べるメールアドレス（コミッター、タグならタガー）。
    private let committerEmail: String
    private let place: SignaturePlace
    private let tag: Tag?

    init(status: SignatureStatus, signature: CommitExtras.Signature?, kind: String, committerEmail: String, place: SignaturePlace, tag: Tag? = nil) {
        self.status = status
        self.signature = signature
        self.kind = kind
        self.committerEmail = committerEmail
        self.place = place
        self.tag = tag
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let icon = NSImageView()
        icon.image = CommitHeaderRenderer.symbolImage(status.symbol, color: status.color, size: 18)
        let title = NSTextField(labelWithString: status.label)
        title.font = .systemFont(ofSize: 14, weight: .semibold)
        let badge = NSTextField(labelWithString: " " + L("Checked on %@", place.name) + " ")
        badge.font = .systemFont(ofSize: 10, weight: .medium)
        badge.textColor = status.color
        badge.wantsLayer = true
        badge.layer?.backgroundColor = status.color.withAlphaComponent(0.15).cgColor
        badge.layer?.cornerRadius = 4
        let spacer = NSView()
        spacer.setContentHuggingPriority(.init(1), for: .horizontal)
        var titles: [NSView] = [icon, title]
        if let tag {
            let name = NSTextField(labelWithString: L("Tag %@", tag.name))
            name.font = .systemFont(ofSize: 12)
            name.textColor = .secondaryLabelColor
            name.lineBreakMode = .byTruncatingMiddle
            titles.append(name)
        }
        let header = NSStackView(views: titles + [spacer, badge])
        header.spacing = 6

        let explanation = NSTextField(wrappingLabelWithString: status.explanation(tag: tag != nil))
        explanation.font = .systemFont(ofSize: 12)
        explanation.textColor = .secondaryLabelColor
        explanation.preferredMaxLayoutWidth = 380

        var rows: [(String, NSView)] = []
        func value(_ s: String, mono: Bool = false, color: NSColor = .labelColor) -> NSTextField {
            let f = NSTextField(wrappingLabelWithString: s)
            f.isSelectable = true
            f.font = mono ? .monospacedSystemFont(ofSize: 11, weight: .regular) : .systemFont(ofSize: 12)
            f.textColor = color
            f.preferredMaxLayoutWidth = 290
            f.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)
            return f
        }
        if let s = signature {
            if !s.signer.isEmpty { rows.append((L("Signer"), value(s.signer))) }
            // 署名した鍵のすべてのメールアドレスとコミッター（タグはタガー）を、状態に関係なく比べて出す。
            let label = tag == nil ? L("Committer email") : L("Tagger email")
            switch CommitterMatch(emails: s.emails, committerEmail: committerEmail) {
            case let .matched(email):
                rows.append((label, value(L("Listed on the key (%@)", email), color: .systemGreen)))
            case let .unconfirmed(email):
                rows.append((label, value(L("Listed on the key (%@), but that user ID isn’t confirmed", email), color: .systemYellow)))
            case .mismatched:
                rows.append((label, value(L("Not listed on the key (%@)", committerEmail), color: .systemOrange)))
            case .unknown:
                if !committerEmail.isEmpty {
                    rows.append((label, value(L("Can’t check: the key’s email addresses can’t be read"), color: .secondaryLabelColor)))
                }
            }
            let others = s.emails.filter { $0.email.lowercased() != committerEmail.lowercased() }
            if kind != "ssh", !others.isEmpty {
                let lines = others.map { e in
                    e.validity == "r" ? L("%@ (revoked)", e.email) : e.validity == "e" ? L("%@ (expired)", e.email) : e.email
                }
                rows.append((L("Other IDs on the key"), value(lines.joined(separator: "\n"))))
            }
        }
        rows.append((L("Format"), value(kind == "ssh" ? "SSH" : kind == "x509" ? L("X.509 (S/MIME)") : L("GPG (OpenPGP)"))))
        if let s = signature {
            if !s.key.isEmpty { rows.append((kind == "ssh" ? L("Public Key") : L("Key ID"), value(s.key, mono: true))) }
            if !s.fingerprint.isEmpty, s.fingerprint != s.key {
                rows.append((L("Fingerprint"), value(Self.grouped(s.fingerprint), mono: true)))
            }
            if kind != "ssh", !s.trust.isEmpty { rows.append((L("Trust"), value(Self.trustText(s.trust)))) }
        }
        let keys = kind == "ssh" ? "allowed signers" : "~/.gnupg"
        rows.append((L("Checked on"), value(L("%@ on %@ (%@)", keys, place.name, place.detail))))
        if let tag, !tag.message.isEmpty { rows.append((L("Message"), value(tag.message))) }

        let grid = NSGridView(views: rows.map { label, view in
            let l = NSTextField(labelWithString: label)
            l.font = .systemFont(ofSize: 11, weight: .medium)
            l.textColor = .secondaryLabelColor
            return [l, view]
        })
        grid.column(at: 0).xPlacement = .trailing
        // 表を中身の幅にする（縦のスタックが横いっぱいに引き伸ばすと、値が短いときに余りが列の間に回り、表が
        // 中ほどに寄って見える）。値の欄の縮みにくさ（750）より弱くして、折り返す値を縮めない（縮めると、290 で
        // 求めた高さより行が増え、下が切れる。折り返すラベルはもとは 250 で縮みやすい）。
        grid.setContentHuggingPriority(.init(749), for: .horizontal)
        grid.rowSpacing = 5
        grid.columnSpacing = 10
        for i in 0..<grid.numberOfRows { grid.row(at: i).rowAlignment = .firstBaseline }

        let stack = NSStackView(views: [header, explanation, grid])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.edgeInsets = NSEdgeInsets(top: 14, left: 16, bottom: 14, right: 16)
        header.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -32).isActive = true
        stack.widthAnchor.constraint(equalToConstant: 420).isActive = true
        view = stack
    }

    /// 指紋を 4 文字ずつに区切る（読み上げて照らし合わせやすくする）。
    static func grouped(_ fingerprint: String) -> String {
        stride(from: 0, to: fingerprint.count, by: 4).map { i -> String in
            let start = fingerprint.index(fingerprint.startIndex, offsetBy: i)
            let end = fingerprint.index(start, offsetBy: 4, limitedBy: fingerprint.endIndex) ?? fingerprint.endIndex
            return String(fingerprint[start..<end])
        }.joined(separator: " ")
    }

    static func trustText(_ trust: String) -> String {
        switch trust.lowercased() {
        case "ultimate": L("ultimate (your own key)")
        case "full", "fully": L("full (fully trusted)")
        case "marginal": L("marginal (partly trusted)")
        case "never": L("never (not trusted)")
        default: L("%@ (trust not set)", trust)
        }
    }
}

/// 署名を検証した場所と、そこになった理由（吹き出しに出す）。
struct SignaturePlace {
    var local: Bool
    var host: String = ""
    var detail: String

    var name: String { local ? L("this Mac") : L("%@ (SSH host)", host) }
}
