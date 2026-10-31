//! タグの署名の見せ方（macOS 版の `SignatureStatus` のタグの分と、サイドバーのタグの行の署名の印・吹き出し）。
//! git の検証結果（`%G?`）に、信用度と「署名者がタガーと一致するか」を加えて決める。

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;
use serde_json::Value;

use crate::icon::{self, Icon};
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
            Self::Verified(_) => (icon::Kind::Seal, Some("success")),
            Self::Untrusted => (icon::Kind::Seal, Some("caution")),
            Self::SignerMismatch => (icon::Kind::Question, Some("caution")),
            Self::Expired => (icon::Kind::History, Some("warning")),
            Self::Revoked | Self::Bad => (icon::Kind::SealCross, Some("error")),
            Self::NoKey | Self::Unsupported => (icon::Kind::Question, None),
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

/// タグの署名の吹き出し（状態、説明、署名者・鍵・タグのメッセージ）を `anchor` の下に出す。
pub fn popover(anchor: &gtk::Widget, name: &str, status: Status, tag: &Value) {
    let (kind, css) = status.icon();
    let mark = Icon::default();
    mark.set(kind, false);
    if let Some(css) = css {
        mark.add_css_class(css);
    }
    let heading = gtk::Label::new(Some(status.label()));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.append(&mark);
    header.append(&heading);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(14);
    content.set_margin_end(14);
    content.set_width_request(400);
    content.append(&header);
    let explanation = crate::sheets::note(&status.explanation());
    explanation.set_max_width_chars(56);
    content.append(&explanation);

    let grid = gtk::Grid::new();
    grid.set_row_spacing(5);
    grid.set_column_spacing(10);
    let mut rows = 0;
    let mut row = |title: &str, value: &str, mono: bool| {
        if value.is_empty() {
            return;
        }
        let l = gtk::Label::new(Some(title));
        l.set_xalign(1.0);
        l.set_yalign(0.0);
        l.add_css_class("dim-label");
        l.add_css_class("caption-heading");
        let v = gtk::Label::new(Some(value));
        v.set_xalign(0.0);
        v.set_wrap(true);
        v.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        v.set_max_width_chars(44);
        v.set_hexpand(true);
        if mono {
            v.add_css_class("monospace");
        }
        grid.attach(&l, 0, rows, 1, 1);
        grid.attach(&v, 1, rows, 1, 1);
        rows += 1;
    };
    let s = &tag["signature"];
    let kind = tag["signed"].as_str().unwrap_or("gpg").to_uppercase();
    row(&tr!("Tag"), name, false);
    row(&tr!("Signature"), &kind, false);
    row(
        &tr!("Signer"),
        s["signer"].as_str().unwrap_or_default(),
        false,
    );
    row(&tr!("Key"), s["key"].as_str().unwrap_or_default(), true);
    row(
        &tr!("Fingerprint"),
        s["fingerprint"].as_str().unwrap_or_default(),
        true,
    );
    row(
        &tr!("Trust"),
        s["trust"].as_str().unwrap_or_default(),
        false,
    );
    content.append(&grid);
    let message = tag["message"].as_str().unwrap_or_default().trim();
    if !message.is_empty() {
        let m = gtk::Label::new(Some(message));
        m.set_xalign(0.0);
        m.set_wrap(true);
        m.set_max_width_chars(56);
        m.set_lines(8);
        m.set_ellipsize(gtk::pango::EllipsizeMode::End);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&m);
    }

    let popover = gtk::Popover::new();
    popover.set_child(Some(&content));
    popover.set_parent(anchor);
    popover.set_position(gtk::PositionType::Right);
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
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
