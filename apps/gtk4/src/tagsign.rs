//! タグの署名の見せ方（macOS 版の `SignatureStatus` のタグの分と、サイドバーのタグの行の署名の印・吹き出し）。
//! git の検証結果（`%G?`）に、信用度と「署名者がタガーと一致するか」を加えて決める。

use amonhen_model::tr;
use serde_json::Value;

use crate::icon;
use crate::sidebar::Mark;

/// タグの署名の状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// 正しい署名で、鍵を信用している。3 は ultimate・full（SSH の allowed signers）、2 は marginal。
    Verified(u8),
    /// 署名は正しいが、鍵の信用度が決まっていない。
    Untrusted,
    /// 署名は正しいが、署名した人のメールアドレスがタガーと違う。
    SignerMismatch,
    Expired,
    Revoked,
    Bad,
    /// 鍵がないなどで検証できない。
    NoKey,
    /// この git がその種類の署名を確かめられない（SSH の署名は git 2.34 から）。
    Unsupported,
}

impl Status {
    /// コアの `tagSignatures` の 1 件から決める。署名のないタグ・軽量タグは None。
    pub fn of(tag: &Value) -> Option<Self> {
        let kind = tag["signed"].as_str()?;
        let s = &tag["signature"];
        if s.is_null() {
            return Some(Self::NoKey);
        }
        Some(match s["status"].as_str().unwrap_or_default() {
            "G" | "U" => {
                let level = trust_level(s["trust"].as_str().unwrap_or_default(), kind == "ssh");
                if s["status"] == "U" || level < 2 {
                    Self::Untrusted
                } else if mismatched(
                    &s["emails"],
                    tag["taggerEmail"].as_str().unwrap_or_default(),
                ) {
                    Self::SignerMismatch
                } else {
                    Self::Verified(level)
                }
            }
            "X" | "Y" => Self::Expired,
            "R" => Self::Revoked,
            "B" => Self::Bad,
            "V" => Self::Unsupported,
            _ => Self::NoKey,
        })
    }

    /// 短い言葉（GitHub に合わせて英語のまま）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Verified(_) => "Verified",
            Self::Untrusted => "Untrusted key",
            Self::SignerMismatch => "Unverified signer",
            Self::Expired => "Expired",
            Self::Revoked => "Revoked key",
            Self::Bad => "Bad signature",
            Self::NoKey => "No key",
            Self::Unsupported => "Can’t verify",
        }
    }

    /// 吹き出しの説明（タグの言い方）。
    pub fn explanation(self) -> String {
        match self {
            Self::Verified(trust) if trust >= 3 => {
                tr!("This tag has a valid signature from a trusted key.")
            }
            Self::Verified(_) => {
                tr!("This tag has a valid signature. The key’s trust is marginal (partly trusted).")
            }
            Self::Untrusted => tr!(
                "The signature is valid, but the key’s trust isn’t set. Check who owns the key and set its trust to make it Verified."
            ),
            Self::SignerMismatch => tr!(
                "The signature is valid, but the tagger’s email address isn’t listed on the signing key. Someone other than the tagger may have signed it."
            ),
            Self::Expired => tr!(
                "The signature or the key has expired. It may have been valid when it was signed."
            ),
            Self::Revoked => {
                tr!("The signing key has been revoked. This signature can’t be trusted.")
            }
            Self::Bad => tr!(
                "The signature is invalid. The tag was changed after it was signed, or the signature is corrupt."
            ),
            Self::NoKey => tr!(
                "There’s no public key to verify the signature (it isn’t in the GPG keyring or the SSH allowed signers)."
            ),
            Self::Unsupported => {
                tr!("This git can’t verify SSH signatures. It needs git 2.34 or later.")
            }
        }
    }

    /// 印のアイコンと色（CSS のクラス）。
    fn icon(self) -> (icon::Kind, Option<&'static str>) {
        match self {
            // 信用度は形でも分かる（塗った seal は強い信用、縁だけは弱い信用。macOS 版と同じ記号）。
            Self::Verified(t) if t >= 3 => (icon::Kind::SealFill, Some("success")),
            Self::Verified(_) => (icon::Kind::Seal, Some("success")),
            Self::Untrusted => (icon::Kind::Shield, Some("caution")),
            Self::SignerMismatch => (icon::Kind::PersonQuestion, Some("caution")),
            Self::Expired => (icon::Kind::ClockAlert, Some("warning")),
            Self::Revoked => (icon::Kind::ShieldAlertFill, Some("error")),
            Self::Bad => (icon::Kind::SealCrossFill, Some("error")),
            Self::NoKey => (icon::Kind::Question, None),
            Self::Unsupported => (icon::Kind::ExclamationCircle, None),
        }
    }

    /// サイドバーのタグの行の印（ツールチップは「言葉 — 説明」）。
    pub fn mark(self) -> Mark {
        let (icon, css) = self.icon();
        Mark {
            icon,
            tooltip: format!("{} — {}", self.label(), self.explanation()),
            css,
        }
    }
}

/// 信用度の段階（3 は ultimate・full、2 は marginal、1 は undefined、0 は never）。SSH の署名は allowed signers に
/// 載っていれば正しい（G）になるので、信用しているものとして 3。
fn trust_level(trust: &str, ssh: bool) -> u8 {
    if ssh {
        return 3;
    }
    match trust.to_lowercase().as_str() {
        "ultimate" | "full" | "fully" => 3,
        "marginal" => 2,
        "never" => 0,
        _ => 1,
    }
}

/// 鍵のメールアドレスを読めて、どれもタガーと一致しないか（失効・期限切れのユーザー ID との一致は数えない）。
fn mismatched(emails: &Value, tagger: &str) -> bool {
    let emails = emails.as_array().map(Vec::as_slice).unwrap_or_default();
    let tagger = tagger.to_lowercase();
    if emails.is_empty() || tagger.is_empty() {
        return false;
    }
    !emails.iter().any(|e| {
        e["email"].as_str().unwrap_or_default().to_lowercase() == tagger
            && !matches!(e["validity"].as_str(), Some("r" | "e" | "n"))
    })
}

/// タグの署名の吹き出しを `anchor` の右に出す。中身はコミットの署名の吹き出しと同じ（状態・説明・署名者・タガーとの
/// 一致・鍵のほかの ID・方式・鍵・指紋・信用度・検証した場所・メッセージ。macOS 版の `SignaturePopover`）。
pub fn popover(
    anchor: &gtk::Widget,
    name: &str,
    status: Status,
    tag: &Value,
    place: &crate::signature::Place,
) {
    let check = Some(&tag["signature"]).filter(|s| !s.is_null());
    crate::signature::show(
        anchor,
        status.into(),
        check,
        tag["signed"].as_str().unwrap_or("gpg"),
        tag["taggerEmail"].as_str().unwrap_or_default(),
        place,
        Some(&crate::signature::Tag {
            name: name.to_owned(),
            message: tag["message"].as_str().unwrap_or_default().to_owned(),
        }),
        Some(gtk::PositionType::Right),
    );
}

impl From<Status> for crate::signature::Status {
    fn from(status: Status) -> Self {
        use crate::signature::Status as S;
        match status {
            Status::Verified(trust) => S::Verified(trust),
            Status::Untrusted => S::Untrusted,
            Status::SignerMismatch => S::SignerMismatch,
            Status::Expired => S::Expired,
            Status::Revoked => S::Revoked,
            Status::Bad => S::Bad,
            Status::NoKey => S::NoKey,
            Status::Unsupported => S::Unsupported,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decides_like_the_macos_app() {
        let tag = |status: &str, trust: &str, emails: Value| {
            json!({"signed": "gpg", "taggerEmail": "t@example.com",
                   "signature": {"status": status, "trust": trust, "emails": emails}})
        };
        assert_eq!(Status::of(&json!({"signed": null})), None);
        assert_eq!(
            Status::of(&json!({"signed": "gpg", "signature": null})),
            Some(Status::NoKey)
        );
        assert_eq!(
            Status::of(&tag("G", "ultimate", json!([]))),
            Some(Status::Verified(3))
        );
        assert_eq!(
            Status::of(&tag("G", "marginal", json!([]))),
            Some(Status::Verified(2))
        );
        assert_eq!(
            Status::of(&tag("G", "undefined", json!([]))),
            Some(Status::Untrusted)
        );
        assert_eq!(
            Status::of(&tag(
                "G",
                "full",
                json!([{"email": "other@example.com", "validity": "u"}])
            )),
            Some(Status::SignerMismatch)
        );
        assert_eq!(
            Status::of(&tag(
                "G",
                "full",
                json!([{"email": "T@example.com", "validity": "u"}])
            )),
            Some(Status::Verified(3))
        );
        assert_eq!(Status::of(&tag("B", "", json!([]))), Some(Status::Bad));
        assert_eq!(Status::of(&tag("Y", "", json!([]))), Some(Status::Expired));
    }
}
