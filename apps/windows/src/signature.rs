//! 署名の見せ方（GTK4 版の `signature.rs`、macOS 版の `SignatureStatus` と `SignaturePopover`）。git の検証結果
//! （`%G?`）に、信用度と「署名者がコミッターと一致するか」を加えて状態を決め、詳細の Signature の行を押すと
//! 吹き出しで、状態・説明・署名者・コミッターとの一致・方式・鍵 ID・指紋・信用度を出す。

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// 署名の状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// 正しい署名で、鍵を信用している（信用度の段階。3 は ultimate・full・SSH、2 は marginal）。
    Verified(u8),
    /// 署名は正しいが、鍵の信用度が決まっていない。
    Untrusted,
    /// 署名は正しいが、署名した人のメールアドレスがコミッターと違う。
    SignerMismatch,
    Expired,
    Revoked,
    Bad,
    NoKey,
    /// 使っている git がその種類の署名を確かめられない。
    Unsupported,
    /// 署名はあるが、まだ検証していない。
    Pending,
}

/// 署名した鍵のメールアドレスと、コミッターのメールアドレスの関係。
#[derive(Clone, Debug, PartialEq, Eq)]
enum CommitterMatch {
    Matched(String),
    Unconfirmed(String),
    Mismatched,
    Unknown,
}

/// 検証の結果の鍵のメールアドレス（アドレス、有効性の 1 文字）。
fn emails(check: &Value) -> Vec<(String, String)> {
    check["emails"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            (
                e["email"].as_str().unwrap_or_default().to_owned(),
                e["validity"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

impl CommitterMatch {
    fn new(emails: &[(String, String)], committer: &str) -> Self {
        let email = committer.to_lowercase();
        if emails.is_empty() || email.is_empty() {
            return Self::Unknown;
        }
        let same: Vec<&(String, String)> = emails
            .iter()
            .filter(|(e, _)| e.to_lowercase() == email)
            .collect();
        if same
            .iter()
            .any(|(_, v)| matches!(v.as_str(), "u" | "f" | "m"))
        {
            Self::Matched(committer.to_owned())
        } else if same
            .iter()
            .any(|(_, v)| !matches!(v.as_str(), "r" | "e" | "n"))
        {
            Self::Unconfirmed(committer.to_owned())
        } else {
            Self::Mismatched
        }
    }
}

/// 信用度の段階（3 は ultimate・full、2 は marginal、1 は undefined、0 は never。SSH は 3）。
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

impl Status {
    /// 検証の結果（`check`。まだなら None）から決める。`kind` は署名の種類（gpg・ssh・x509）。
    pub fn new(check: Option<&Value>, committer: &str, kind: Option<&str>) -> Self {
        let Some(check) = check else {
            return Self::Pending;
        };
        let status = check["status"].as_str().unwrap_or_default();
        match status {
            "G" | "U" => {
                let level = trust_level(
                    check["trust"].as_str().unwrap_or_default(),
                    kind == Some("ssh"),
                );
                if status == "U" || level < 2 {
                    Self::Untrusted
                } else if CommitterMatch::new(&emails(check), committer)
                    == CommitterMatch::Mismatched
                {
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
        }
    }

    /// 短い言葉（GitHub に合わせて英語）。
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
            Self::Pending => "Checking…",
        }
    }

    /// 状態の共通のアイコン（GTK4 版の `Status::icon`）。
    pub fn icon(self) -> &'static str {
        match self {
            Self::Verified(_) | Self::Pending => "signed",
            Self::Untrusted => "trust-unknown",
            Self::SignerMismatch => "signer-mismatch",
            Self::Expired => "expired-key",
            Self::Revoked => "revoked-key",
            Self::Bad => "bad-signature",
            Self::NoKey => "missing-key",
            Self::Unsupported => "cannot-verify",
        }
    }

    /// 状態の色（History の行の印と同じ。色のないものは None）。
    pub fn color(self) -> Option<Color> {
        match self {
            Self::Verified(_) => Some(Color::rgb(0x2b, 0x9a, 0x45)),
            Self::Untrusted | Self::SignerMismatch => Some(Color::rgb(0xb8, 0x8a, 0x00)),
            Self::Expired => Some(Color::rgb(0xd9, 0x6b, 0x10)),
            Self::Revoked | Self::Bad => Some(Color::rgb(0xd0, 0x33, 0x33)),
            Self::NoKey | Self::Unsupported | Self::Pending => None,
        }
    }

    /// 吹き出しの説明（コミットの署名）。
    pub fn explanation(self) -> String {
        match self {
            Self::Verified(trust) if trust >= 3 => {
                tr!("This commit has a valid signature from a trusted key.")
            }
            Self::Verified(_) => tr!(
                "This commit has a valid signature. The key’s trust is marginal (partly trusted)."
            ),
            Self::Untrusted => tr!(
                "The signature is valid, but the key’s trust isn’t set. Check who owns the key and set its trust to make it Verified."
            ),
            Self::SignerMismatch => tr!(
                "The signature is valid, but the committer’s email address isn’t listed on the signing key. Someone other than the committer may have signed it."
            ),
            Self::Expired => tr!(
                "The signature or the key has expired. It may have been valid when it was signed."
            ),
            Self::Revoked => {
                tr!("The signing key has been revoked. This signature can’t be trusted.")
            }
            Self::Bad => tr!(
                "The signature is invalid. The commit was changed after it was signed, or the signature is corrupt."
            ),
            Self::NoKey => tr!(
                "There’s no public key to verify the signature (it isn’t in the GPG keyring or the SSH allowed signers)."
            ),
            Self::Unsupported => {
                tr!("This git can’t verify SSH signatures. It needs git 2.34 or later.")
            }
            Self::Pending => tr!("Verifying the signature."),
        }
    }
}

/// 指紋を 4 文字ずつに区切る（読み上げて照らし合わせやすくする）。
fn grouped(fingerprint: &str) -> String {
    let chars: Vec<char> = fingerprint.chars().collect();
    chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

fn trust_text(trust: &str) -> String {
    match trust.to_lowercase().as_str() {
        "ultimate" => tr!("ultimate (your own key)"),
        "full" | "fully" => tr!("full (fully trusted)"),
        "marginal" => tr!("marginal (partly trusted)"),
        "never" => tr!("never (not trusted)"),
        _ => tr!("{trust} (trust not set)", trust = trust),
    }
}

/// コアの `tagSignatures` の 1 件の状態（GTK4 版の `tagsign::Status::of`）と署名者。署名のないタグ・軽量タグは
/// None。
pub fn of_tag(tag: &Value) -> Option<(Status, String)> {
    let kind = tag["signed"].as_str()?;
    let check = &tag["signature"];
    let status = if check.is_null() {
        Status::NoKey
    } else {
        Status::new(
            Some(check),
            tag["taggerEmail"].as_str().unwrap_or_default(),
            Some(kind),
        )
    };
    Some((
        status,
        check["signer"].as_str().unwrap_or_default().to_owned(),
    ))
}

/// 詳細の署名（状態、検証の結果、種類）。署名がなければ None。検証の結果は時間のかかる分（`extras`）で届く。
pub fn of<'a>(
    info: &'a Value,
    extras: Option<&'a Value>,
) -> Option<(Status, Option<&'a Value>, String)> {
    let check = extras.map(|e| &e["signature"]).filter(|s| !s.is_null());
    let kind = info["signed"].as_str();
    if kind.is_none() && check.is_none() {
        return None;
    }
    let committer = info["committerEmail"].as_str().unwrap_or_default();
    let status = match extras {
        None => Status::Pending,
        // 検証したが結果がなければ、鍵がない。
        Some(_) if check.is_none() => Status::NoKey,
        Some(_) => Status::new(check, committer, kind),
    };
    Some((status, check, kind.unwrap_or("gpg").to_owned()))
}

/// 署名の詳細の吹き出しの中身（GTK4 版の `show`）。値は選んでコピーできる。
pub fn popover(status: Status, check: Option<&Value>, kind: &str, committer: &str) -> View {
    let mut title = TextBlock::new().text(status.label()).font_size(14.0);
    if let Some(color) = status.color() {
        title = title.foreground(Brush::Solid(color));
    }
    let header = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(10.0)
        .children((
            title,
            TextBlock::new()
                .text(tr!("Checked on {place}", place = tr!("this computer")))
                .font_size(11.0)
                .opacity(0.6)
                .vertical_alignment(VerticalAlignment::Center),
        ));
    let explanation = TextBlock::new()
        .text(status.explanation())
        .font_size(12.0)
        .opacity(0.7)
        .text_wrapping(TextWrapping::Wrap)
        .max_width(380.0);

    // (見出し, 値, 色)。
    let mut rows: Vec<(String, String, Option<Color>)> = Vec::new();
    let text = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    if let Some(s) = check {
        let signer = text(s, "signer");
        if !signer.is_empty() {
            rows.push((tr!("Signer"), signer, None));
        }
        // 署名した鍵のすべてのメールアドレスとコミッターを、状態に関係なく比べて出す。
        let list = emails(s);
        let who = tr!("Committer email");
        match CommitterMatch::new(&list, committer) {
            CommitterMatch::Matched(email) => rows.push((
                who,
                tr!("Listed on the key ({email})", email = email),
                Status::Verified(3).color(),
            )),
            CommitterMatch::Unconfirmed(email) => rows.push((
                who,
                tr!(
                    "Listed on the key ({email}), but that user ID isn’t confirmed",
                    email = email
                ),
                Status::Untrusted.color(),
            )),
            CommitterMatch::Mismatched => rows.push((
                who,
                tr!("Not listed on the key ({email})", email = committer),
                Status::Expired.color(),
            )),
            CommitterMatch::Unknown if !committer.is_empty() => rows.push((
                who,
                tr!("Can’t check: the key’s email addresses can’t be read"),
                None,
            )),
            CommitterMatch::Unknown => {}
        }
        let others: Vec<String> = list
            .iter()
            .filter(|(e, _)| e.to_lowercase() != committer.to_lowercase())
            .map(|(e, v)| match v.as_str() {
                "r" => tr!("{email} (revoked)", email = e),
                "e" => tr!("{email} (expired)", email = e),
                _ => e.clone(),
            })
            .collect();
        if kind != "ssh" && !others.is_empty() {
            rows.push((tr!("Other IDs on the key"), others.join("\n"), None));
        }
    }
    let format = match kind {
        "ssh" => "SSH".to_owned(),
        "x509" => tr!("X.509 (S/MIME)"),
        _ => tr!("GPG (OpenPGP)"),
    };
    rows.push((tr!("Format"), format, None));
    if let Some(s) = check {
        let (key, fingerprint, trust) = (text(s, "key"), text(s, "fingerprint"), text(s, "trust"));
        if !key.is_empty() {
            let title = if kind == "ssh" {
                tr!("Public Key")
            } else {
                tr!("Key ID")
            };
            rows.push((title, key.clone(), None));
        }
        if !fingerprint.is_empty() && fingerprint != key {
            rows.push((tr!("Fingerprint"), grouped(&fingerprint), None));
        }
        if kind != "ssh" && !trust.is_empty() {
            rows.push((tr!("Trust"), trust_text(&trust), None));
        }
    }
    let count = rows.len();
    let cells = rows
        .into_iter()
        .enumerate()
        .flat_map(|(i, (name, value, color))| {
            let mut cell = TextBlock::new()
                .text(value)
                .font_size(12.0)
                .is_text_selection_enabled(true)
                .text_wrapping(TextWrapping::Wrap)
                .max_width(300.0)
                .grid_row(i as i32)
                .grid_column(1);
            if let Some(color) = color {
                cell = cell.foreground(Brush::Solid(color));
            }
            [
                (
                    format!("h{i}"),
                    View::from(
                        TextBlock::new()
                            .text(name)
                            .font_size(12.0)
                            .opacity(0.6)
                            .horizontal_alignment(HorizontalAlignment::Right)
                            .grid_row(i as i32)
                            .grid_column(0),
                    ),
                ),
                (format!("v{i}"), View::from(cell)),
            ]
        });
    let grid = Grid::new()
        .columns([GridLength::Auto, GridLength::Star(1.0)])
        .rows(vec![GridLength::Auto; count])
        .column_spacing(10.0)
        .row_spacing(5.0)
        .children((View::keyed_fragment(cells),));
    StackPanel::new()
        .spacing(10.0)
        .max_width(420.0)
        .children((header, explanation, grid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_signer_without_the_committers_email_is_not_verified() {
        let check = json!({
            "status": "G", "trust": "ultimate", "signer": "A <a@x>",
            "emails": [{"email": "a@x", "validity": "u"}],
        });
        assert_eq!(
            Status::new(Some(&check), "a@x", Some("gpg")),
            Status::Verified(3)
        );
        assert_eq!(
            Status::new(Some(&check), "b@x", Some("gpg")),
            Status::SignerMismatch
        );
        assert_eq!(Status::new(None, "a@x", Some("gpg")), Status::Pending);
    }
}
