import AmonhenKit
import AppKit

/// 署名の詳細の吹き出し（GitHub の Verified の吹き出しに倣う）。状態・説明・署名者・コミッターとの一致・方式・
/// 鍵 ID・指紋・信用度・検証した場所。値は選んでコピーできる。
final class SignaturePopover: NSViewController {
    private let status: SignatureStatus
    private let signature: CommitExtras.Signature?
    private let kind: String
    private let committerEmail: String
    private let place: SignaturePlace

    init(status: SignatureStatus, signature: CommitExtras.Signature?, kind: String, committerEmail: String, place: SignaturePlace) {
        self.status = status
        self.signature = signature
        self.kind = kind
        self.committerEmail = committerEmail
        self.place = place
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let icon = NSImageView()
        icon.image = CommitHeaderRenderer.symbolImage(status.symbol, color: status.color, size: 18)
        let title = NSTextField(labelWithString: status.label)
        title.font = .systemFont(ofSize: 14, weight: .semibold)
        let badge = NSTextField(labelWithString: " \(place.name)で検証 ")
        badge.font = .systemFont(ofSize: 10, weight: .medium)
        badge.textColor = status.color
        badge.wantsLayer = true
        badge.layer?.backgroundColor = status.color.withAlphaComponent(0.15).cgColor
        badge.layer?.cornerRadius = 4
        let spacer = NSView()
        spacer.setContentHuggingPriority(.init(1), for: .horizontal)
        let header = NSStackView(views: [icon, title, spacer, badge])
        header.spacing = 6

        let explanation = NSTextField(wrappingLabelWithString: status.explanation)
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
            return f
        }
        if let s = signature {
            if !s.signer.isEmpty { rows.append(("署名者", value(s.signer))) }
            // 署名者のメールアドレスとコミッターを、状態に関係なく比べて出す。
            if let matches = Self.signerMatches(signer: s.signer, committerEmail: committerEmail) {
                rows.append(matches
                    ? ("コミッター", value("一致（\(committerEmail)）", color: .systemGreen))
                    : ("コミッター", value("一致しない（\(committerEmail)）", color: .systemYellow)))
            }
        }
        rows.append(("方式", value(kind == "ssh" ? "SSH" : kind == "x509" ? "X.509（S/MIME）" : "GPG（OpenPGP）")))
        if let s = signature {
            if !s.key.isEmpty { rows.append((kind == "ssh" ? "鍵" : "鍵 ID", value(s.key, mono: true))) }
            if !s.fingerprint.isEmpty, s.fingerprint != s.key {
                rows.append(("指紋", value(Self.grouped(s.fingerprint), mono: true)))
            }
            if kind != "ssh", !s.trust.isEmpty { rows.append(("信用", value(Self.trustText(s.trust)))) }
        }
        let keys = kind == "ssh" ? "allowed signers" : "~/.gnupg"
        rows.append(("検証した場所", value("\(place.name)の \(keys)（\(place.detail)）")))

        let grid = NSGridView(views: rows.map { label, view in
            let l = NSTextField(labelWithString: label)
            l.font = .systemFont(ofSize: 11, weight: .medium)
            l.textColor = .secondaryLabelColor
            return [l, view]
        })
        grid.column(at: 0).xPlacement = .trailing
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

    /// 署名者のメールアドレスがコミッターと同じか。どちらかが分からなければ nil。
    static func signerMatches(signer: String, committerEmail: String) -> Bool? {
        let email = committerEmail.lowercased()
        guard !signer.isEmpty, !email.isEmpty else { return nil }
        let signed = signer.range(of: "<").map { String(signer[$0.upperBound...].prefix { $0 != ">" }) } ?? signer
        return signed.lowercased() == email
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
        case "ultimate": "ultimate（自分の鍵）"
        case "full", "fully": "full（完全に信用）"
        case "marginal": "marginal（部分的に信用）"
        case "never": "never（信用しない）"
        default: "\(trust)（信用度が決まっていない）"
        }
    }
}

/// 署名を検証した場所と、そこになった理由（吹き出しに出す）。
struct SignaturePlace {
    var local: Bool
    var host: String = ""
    var detail: String

    var name: String { local ? "この Mac" : "\(host)（SSH 先）" }
}
