import AmonhenKit
import AppKit

/// コミットの署名の見せ方。git の検証結果（`%G?`）に、信用度と「署名者がコミッターと一致するか」を加えて決める。
/// ヘッダー・吹き出し・History の一覧で同じアイコンと言葉を使う。
enum SignatureStatus: Equatable {
    /// 正しい署名で、鍵を信用している。`trust` は 3（ultimate・full、SSH の allowed signers）か 2（marginal）。
    case verified(trust: Int)
    /// 署名は正しいが、鍵の信用度が決まっていない（U、または信用度 undefined・never）。
    case untrusted
    /// 署名は正しいが、署名した人のメールアドレスがコミッターと違う。
    case signerMismatch
    /// 期限切れの署名（X）か、期限切れの鍵（Y）。
    case expired
    /// 失効した鍵（R）。
    case revoked
    /// 不正な署名（B）。
    case bad
    /// 鍵がないなどで検証できない（E）。
    case noKey
    /// 署名はあるが、まだ検証していない。
    case pending

    /// 検証の結果から決める。`trust` は `%GT`、`emails` は署名した鍵に登録されたメールアドレス。
    /// 署名者がコミッターと違うとするのは、鍵のメールアドレスを読めて、どれも一致しないときだけ。
    init(status: String?, trust: String = "", emails: [SignerEmail] = [], committerEmail: String = "", kind: String? = nil) {
        guard let status else {
            self = .pending
            return
        }
        switch status {
        case "G", "U":
            let level = Self.trustLevel(trust, ssh: kind == "ssh")
            if status == "U" || level < 2 {
                self = .untrusted
            } else if CommitterMatch(emails: emails, committerEmail: committerEmail) == .mismatched {
                self = .signerMismatch
            } else {
                self = .verified(trust: level)
            }
        case "X", "Y": self = .expired
        case "R": self = .revoked
        case "B": self = .bad
        default: self = .noKey
        }
    }

    /// 信用度の段階（3 は ultimate・full、2 は marginal、1 は undefined、0 は never）。SSH の署名は allowed signers に
    /// 載っていれば正しい（G）になるので、信用しているものとして 3 にする。
    static func trustLevel(_ trust: String, ssh: Bool) -> Int {
        if ssh { return 3 }
        switch trust.lowercased() {
        case "ultimate", "full", "fully": return 3
        case "marginal": return 2
        case "never": return 0
        default: return 1
        }
    }

    /// SF Symbols の名前。信用度はアイコンの形でも分かる（塗りつぶしの seal は強い信用、縁だけは弱い信用）。
    var symbol: String {
        switch self {
        case let .verified(trust): trust >= 3 ? "checkmark.seal.fill" : "checkmark.seal"
        case .untrusted: "checkmark.shield"
        case .signerMismatch: "person.crop.circle.badge.questionmark"
        case .expired: "clock.badge.exclamationmark"
        case .revoked: "exclamationmark.shield.fill"
        case .bad: "xmark.seal.fill"
        // 縦長の鍵は、ほかのアイコンと大きさが揃わないので丸に「?」にする。
        case .noKey: "questionmark.circle"
        case .pending: "seal"
        }
    }

    var color: NSColor {
        switch self {
        case .verified: .systemGreen
        case .untrusted, .signerMismatch: .systemYellow
        case .expired: .systemOrange
        case .revoked, .bad: .systemRed
        case .noKey: .secondaryLabelColor
        case .pending: .tertiaryLabelColor
        }
    }

    /// 短い言葉（GitHub に合わせて英語）。
    var label: String {
        switch self {
        case .verified: "Verified"
        case .untrusted: "Untrusted key"
        case .signerMismatch: "Unverified signer"
        case .expired: "Expired"
        case .revoked: "Revoked key"
        case .bad: "Bad signature"
        case .noKey: "No key"
        case .pending: "Checking…"
        }
    }

    /// 吹き出しの説明（日本語）。
    var explanation: String {
        switch self {
        case let .verified(trust):
            trust >= 3 ? L("This commit has a valid signature from a trusted key.")
                : L("This commit has a valid signature. The key’s trust is marginal (partly trusted).")
        case .untrusted: L("The signature is valid, but the key’s trust isn’t set. Check who owns the key and set its trust to make it Verified.")
        case .signerMismatch: L("The signature is valid, but the committer’s email address isn’t listed on the signing key. Someone other than the committer may have signed it.")
        case .expired: L("The signature or the key has expired. It may have been valid when it was signed.")
        case .revoked: L("The signing key has been revoked. This signature can’t be trusted.")
        case .bad: L("The signature is invalid. The commit was changed after it was signed, or the signature is corrupt.")
        case .noKey: L("There’s no public key to verify the signature (it isn’t in the GPG keyring or the SSH allowed signers).")
        case .pending: L("Verifying the signature.")
        }
    }

    /// かざしたときの短い説明。
    var toolTip: String { L("%@ — %@ (Click for details.)", label, explanation) }

    /// 信用度の目盛りの段数（0〜3）。信用度に意味がない状態は nil。
    var trustBars: Int? {
        switch self {
        case let .verified(trust): trust
        case .untrusted: 1
        default: nil
        }
    }
}

/// 署名した鍵のメールアドレスと、コミッターのメールアドレスの関係。
enum CommitterMatch: Equatable {
    /// 有効なユーザー ID（u・f・m、SSH のプリンシパル）と一致する。
    case matched(String)
    /// 有効性が分からないユーザー ID とだけ一致する（誰でも他人のアドレスを書けるので、言い切らない）。
    case unconfirmed(String)
    /// 鍵のメールアドレスを読めたが、どれとも一致しない（失効・期限切れのユーザー ID との一致は数えない）。
    case mismatched
    /// 鍵のメールアドレスが読めない（鍵がないなど）か、コミッターのメールアドレスが分からない。
    case unknown

    init(emails: [SignerEmail], committerEmail: String) {
        let email = committerEmail.lowercased()
        guard !emails.isEmpty, !email.isEmpty else {
            self = .unknown
            return
        }
        let same = emails.filter { $0.email.lowercased() == email }
        if same.contains(where: { ["u", "f", "m"].contains($0.validity) }) {
            self = .matched(committerEmail)
        } else if same.contains(where: { !["r", "e", "n"].contains($0.validity) }) {
            self = .unconfirmed(committerEmail)
        } else {
            self = .mismatched
        }
    }
}
