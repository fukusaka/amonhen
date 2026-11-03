//! 署名の見せ方（macOS 版の `SignatureStatus` と `SignaturePopover`）。git の検証結果（`%G?`）に、信用度と「署名者が
//! コミッターと一致するか」を加えて状態を決め、詳細の Signature の行を押すと吹き出しで、状態・説明・署名者・
//! コミッターとの一致・方式・鍵 ID・指紋・信用度・検証した場所を出す（GitHub の Verified の吹き出しに倣う）。

use amonhen_model::tr;
use gtk::prelude::*;
use serde_json::Value;

/// 署名の状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// 正しい署名で、鍵を信用している。中身は信用度の段階（3 は ultimate・full、SSH の allowed signers。2 は
    /// marginal）。
    Verified(u8),
    /// 署名は正しいが、鍵の信用度が決まっていない（U、または信用度 undefined・never）。
    Untrusted,
    /// 署名は正しいが、署名した人のメールアドレスがコミッターと違う。
    SignerMismatch,
    /// 期限切れの署名（X）か、期限切れの鍵（Y）。
    Expired,
    /// 失効した鍵（R）。
    Revoked,
    /// 不正な署名（B）。
    Bad,
    /// 鍵がないなどで検証できない（E）。
    NoKey,
    /// 使っている git がその種類の署名を確かめられない（V）。
    Unsupported,
    /// 署名はあるが、まだ検証していない。
    Pending,
}

/// 署名した鍵のメールアドレスと、コミッターのメールアドレスの関係。
#[derive(Clone, Debug, PartialEq, Eq)]
enum CommitterMatch {
    /// 有効なユーザー ID（u・f・m、SSH のプリンシパル）と一致する。
    Matched(String),
    /// 有効性が分からないユーザー ID とだけ一致する（誰でも他人のアドレスを書けるので、言い切らない）。
    Unconfirmed(String),
    /// 鍵のメールアドレスを読めたが、どれとも一致しない。
    Mismatched,
    /// 鍵のメールアドレスが読めないか、コミッターのメールアドレスが分からない。
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

/// 信用度の段階（3 は ultimate・full、2 は marginal、1 は undefined、0 は never）。SSH の署名は allowed signers に
/// 載っていれば正しい（G）になるので、信用しているものとして 3 にする。
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

    /// 色の CSS のクラス。
    fn css(self) -> Option<&'static str> {
        match self {
            Self::Verified(_) => Some("success"),
            Self::Untrusted | Self::SignerMismatch | Self::Expired => Some("warning"),
            Self::Revoked | Self::Bad => Some("error"),
            Self::NoKey | Self::Unsupported | Self::Pending => Some("dim-label"),
        }
    }

    /// 状態の印（History の行と吹き出しの見出し。macOS 版の `symbol` と同じく状態ごとに形を変え、信用度も形で
    /// 分かる: 塗った seal は強い信用、縁だけは弱い信用。サイドバーのタグの印と同じ記号）。
    pub(crate) fn icon(self) -> crate::icon::Kind {
        use crate::icon::Kind;
        match self {
            Self::Verified(trust) if trust >= 3 => Kind::SealFill,
            Self::Verified(_) | Self::Pending => Kind::Seal,
            Self::Untrusted => Kind::Shield,
            Self::SignerMismatch => Kind::PersonQuestion,
            Self::Expired => Kind::ClockAlert,
            Self::Revoked => Kind::ShieldAlertFill,
            Self::Bad => Kind::SealCrossFill,
            Self::NoKey => Kind::Question,
            Self::Unsupported => Kind::ExclamationCircle,
        }
    }

    /// 信用度の目盛りの段数（0〜3。macOS 版の `trustBars`）。信用度に意味がない状態は None。
    pub fn trust_bars(self) -> Option<u8> {
        match self {
            Self::Verified(trust) => Some(trust),
            Self::Untrusted => Some(1),
            _ => None,
        }
    }

    /// 吹き出しの説明。`tag` ならタグの署名の言い方（コミッターでなくタガー。macOS 版と同じ）。
    pub fn explanation_for(self, tag: bool) -> String {
        match self {
            Self::Verified(trust) if trust >= 3 && tag => {
                tr!("This tag has a valid signature from a trusted key.")
            }
            Self::Verified(trust) if trust >= 3 => {
                tr!("This commit has a valid signature from a trusted key.")
            }
            Self::Verified(_) if tag => {
                tr!("This tag has a valid signature. The key’s trust is marginal (partly trusted).")
            }
            Self::Verified(_) => tr!(
                "This commit has a valid signature. The key’s trust is marginal (partly trusted)."
            ),
            Self::Untrusted => tr!(
                "The signature is valid, but the key’s trust isn’t set. Check who owns the key and set its trust to make it Verified."
            ),
            Self::SignerMismatch if tag => tr!(
                "The signature is valid, but the tagger’s email address isn’t listed on the signing key. Someone other than the tagger may have signed it."
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
            Self::Bad if tag => tr!(
                "The signature is invalid. The tag was changed after it was signed, or the signature is corrupt."
            ),
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

/// 署名を検証した場所（吹き出しに出す）。
#[derive(Clone, Debug)]
pub struct Place {
    /// SSH 先のホスト（手元で検証していれば None）。
    pub host: Option<String>,
    /// そこになった理由。
    pub detail: String,
}

impl Default for Place {
    fn default() -> Self {
        Self {
            host: None,
            detail: tr!("the repository is on this computer"),
        }
    }
}

impl Place {
    /// `location`（開いたリポジトリの場所）の署名を検証する場所（macOS 版と同じく、設定が効くのは SSH 先だけ）。
    pub fn of(location: &str) -> Self {
        match crate::sshhosts::host_of(location) {
            Some(host) => Self {
                host: (!crate::sshhosts::checks_signatures_locally(location)).then_some(host),
                detail: tr!("chosen in Settings > SSH Hosts"),
            },
            None => Self::default(),
        }
    }

    pub(crate) fn name(&self) -> String {
        match &self.host {
            None => tr!("this computer"),
            Some(host) => tr!("{host} (SSH host)", host = host),
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

/// タグの署名なら、タグの名前とメッセージ（吹き出しの見出しとメッセージの行に出す。macOS 版の
/// `SignaturePopover.Tag`）。
#[derive(Clone, Debug, Default)]
pub struct Tag {
    pub name: String,
    pub message: String,
}

/// 詳細の署名の印（macOS 版の `signatureMark`）: 状態の短い言葉（色付き。押すと吹き出し）・信用度の目盛り・方式と
/// 署名者。`check` は検証の結果（なければ None）、`email` は署名した鍵と比べるメール（コミッター、タグならタガー）。
pub fn row(
    status: Status,
    check: Option<Value>,
    kind: String,
    email: String,
    place: Place,
    tag: Option<Tag>,
) -> gtk::Widget {
    let label = gtk::Label::new(Some(status.label()));
    label.set_xalign(0.0);
    label.add_css_class("caption");
    label.add_css_class("heading");
    if let Some(css) = status.css() {
        label.add_css_class(css);
    }
    label.set_cursor_from_name(Some("pointer"));
    label.set_tooltip_text(Some(&tr!(
        "{label} — {explanation} (Click for details.)",
        label = status.label(),
        explanation = status.explanation_for(tag.is_some())
    )));
    let signer = check
        .as_ref()
        .and_then(|c| c["signer"].as_str())
        .unwrap_or_default()
        .to_owned();
    let mark = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    mark.append(&label);
    if let Some(bars) = status.trust_bars() {
        mark.append(&trust_meter(bars, status.css()));
    }
    let shown = kind.to_uppercase();
    let detail = gtk::Label::new(Some(&if signer.is_empty() {
        format!(" {shown}")
    } else {
        format!(" {shown} · {signer}")
    }));
    detail.set_xalign(0.0);
    detail.set_wrap(true);
    detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    detail.set_hexpand(true);
    detail.add_css_class("caption");
    detail.add_css_class("dim-label");
    mark.append(&detail);
    let open: std::rc::Rc<dyn Fn(&gtk::Widget)> = std::rc::Rc::new(move |w| {
        show(
            w,
            status,
            check.as_ref(),
            &kind,
            &email,
            &place,
            tag.as_ref(),
            None,
        );
    });
    let click = gtk::GestureClick::new();
    let f = open.clone();
    click.connect_released(move |gesture, _, _, _| {
        if let Some(w) = gesture.widget() {
            f(&w);
        }
    });
    label.add_controller(click);
    // 確認用（`--signature-popover`）: 検証の結果が届いた行で、一度だけ出す。
    if status != Status::Pending && POPOVER_ONCE.with(|p| p.replace(false)) {
        let w = label.clone();
        gtk::glib::idle_add_local_once(move || open(w.upcast_ref()));
    }
    mark.upcast()
}

/// 吹き出しの札の見た目（状態の色の文字に、同じ色の薄い地。macOS 版と同じ）。
fn install_css() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".signature-badge { border-radius: 4px; padding: 1px 5px; font-weight: 500; \
         background-color: alpha(currentColor, 0.15); }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 信用度の 3 段の目盛り（`bars` 段を状態の色で塗る。macOS 版の `trustMeter`）。
fn trust_meter(bars: u8, css: Option<&'static str>) -> gtk::DrawingArea {
    let (width, height, gap) = (4.0, 10.0, 2.0);
    let area = gtk::DrawingArea::new();
    area.set_content_width((width * 3.0 + gap * 2.0) as i32);
    area.set_content_height(height as i32);
    area.set_valign(gtk::Align::Center);
    if let Some(css) = css {
        area.add_css_class(css);
    }
    area.set_draw_func(move |area, cr, _, _| {
        let color = area.color();
        for i in 0..3 {
            let alpha = if i < bars as i32 {
                color.alpha()
            } else {
                color.alpha() * 0.25
            };
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                alpha as f64,
            );
            let x = i as f64 * (width + gap);
            let r = 1.5;
            cr.new_sub_path();
            cr.arc(x + width - r, r, r, -std::f64::consts::FRAC_PI_2, 0.0);
            cr.arc(
                x + width - r,
                height - r,
                r,
                0.0,
                std::f64::consts::FRAC_PI_2,
            );
            cr.arc(
                x + r,
                height - r,
                r,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI,
            );
            cr.arc(
                x + r,
                r,
                r,
                std::f64::consts::PI,
                3.0 * std::f64::consts::FRAC_PI_2,
            );
            cr.close_path();
            let _ = cr.fill();
        }
    });
    area
}

thread_local! {
    /// 確認用（`--signature-popover`）: 最初に Signature の行を作ったら、吹き出しを出す。
    static POPOVER_ONCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 確認用（`--signature-popover`）: 検証の結果が届いた Signature の行を最初に作ったら、吹き出しを出す。
pub fn popover_once() {
    POPOVER_ONCE.with(|p| p.set(true));
}

/// 署名の詳細の吹き出し。値は選んでコピーできる。`tag` ならタグの署名（タガーと比べ、名前とメッセージを出す）。
/// `position` は `parent` のどちら側に出すか（None なら GTK の既定）。出した吹き出しを返す。
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    parent: &gtk::Widget,
    status: Status,
    check: Option<&Value>,
    kind: &str,
    committer: &str,
    place: &Place,
    tag: Option<&Tag>,
    position: Option<gtk::PositionType>,
) -> gtk::Popover {
    install_css();
    // 見出し: 状態の色のアイコンと言葉、右に検証した場所の札（状態の色の文字と薄い地。macOS 版と同じ）。
    let icon = crate::icon::Icon::default();
    icon.set(status.icon(), false);
    let title = gtk::Label::new(Some(status.label()));
    title.add_css_class("heading");
    if let Some(css) = status.css() {
        icon.add_css_class(css);
        title.add_css_class(css);
    }
    let badge = gtk::Label::new(Some(&tr!("Checked on {place}", place = place.name())));
    badge.add_css_class("caption");
    badge.add_css_class("signature-badge");
    if let Some(css) = status.css() {
        badge.add_css_class(css);
    }
    badge.set_valign(gtk::Align::Center);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.append(&icon);
    header.append(&title);
    if let Some(tag) = tag {
        let name = gtk::Label::new(Some(&tr!("Tag {name}", name = tag.name)));
        name.add_css_class("dim-label");
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        header.append(&name);
    }
    header.append(&spacer);
    header.append(&badge);
    let explanation = gtk::Label::new(Some(&status.explanation_for(tag.is_some())));
    explanation.set_wrap(true);
    explanation.set_max_width_chars(52);
    explanation.set_xalign(0.0);
    explanation.add_css_class("dim-label");

    let mut rows: Vec<(String, String, Option<&'static str>, bool)> = Vec::new();
    let text = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    if let Some(s) = check {
        let signer = text(s, "signer");
        if !signer.is_empty() {
            rows.push((tr!("Signer"), signer, None, false));
        }
        // 署名した鍵のすべてのメールアドレスとコミッター（タグはタガー）を、状態に関係なく比べて出す。
        let list = emails(s);
        let who = if tag.is_some() {
            tr!("Tagger email")
        } else {
            tr!("Committer email")
        };
        match CommitterMatch::new(&list, committer) {
            CommitterMatch::Matched(email) => rows.push((
                who,
                tr!("Listed on the key ({email})", email = email),
                Some("success"),
                false,
            )),
            CommitterMatch::Unconfirmed(email) => rows.push((
                who,
                tr!(
                    "Listed on the key ({email}), but that user ID isn’t confirmed",
                    email = email
                ),
                Some("warning"),
                false,
            )),
            CommitterMatch::Mismatched => rows.push((
                who,
                tr!("Not listed on the key ({email})", email = committer),
                Some("warning"),
                false,
            )),
            CommitterMatch::Unknown if !committer.is_empty() => rows.push((
                who,
                tr!("Can’t check: the key’s email addresses can’t be read"),
                Some("dim-label"),
                false,
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
            rows.push((tr!("Other IDs on the key"), others.join("\n"), None, false));
        }
    }
    let format = match kind {
        "ssh" => "SSH".to_owned(),
        "x509" => tr!("X.509 (S/MIME)"),
        _ => tr!("GPG (OpenPGP)"),
    };
    rows.push((tr!("Format"), format, None, false));
    if let Some(s) = check {
        let (key, fingerprint, trust) = (text(s, "key"), text(s, "fingerprint"), text(s, "trust"));
        if !key.is_empty() {
            let title = if kind == "ssh" {
                tr!("Public Key")
            } else {
                tr!("Key ID")
            };
            rows.push((title, key.clone(), None, true));
        }
        if !fingerprint.is_empty() && fingerprint != key {
            rows.push((tr!("Fingerprint"), grouped(&fingerprint), None, true));
        }
        if kind != "ssh" && !trust.is_empty() {
            rows.push((tr!("Trust"), trust_text(&trust), None, false));
        }
    }
    let keys = if kind == "ssh" {
        "allowed signers"
    } else {
        "~/.gnupg"
    };
    rows.push((
        tr!("Checked on"),
        tr!(
            "{keys} on {place} ({detail})",
            keys = keys,
            place = place.name(),
            detail = place.detail
        ),
        None,
        false,
    ));
    if let Some(tag) = tag.filter(|t| !t.message.trim().is_empty()) {
        rows.push((tr!("Message"), tag.message.trim().to_owned(), None, false));
    }

    let grid = gtk::Grid::builder()
        .column_spacing(10)
        .row_spacing(5)
        .build();
    for (i, (name, value, css, mono)) in rows.into_iter().enumerate() {
        let head = gtk::Label::new(Some(&name));
        head.set_xalign(1.0);
        head.set_yalign(0.0);
        head.add_css_class("caption");
        head.add_css_class("dim-label");
        let cell = gtk::Label::new(Some(&value));
        cell.set_xalign(0.0);
        cell.set_wrap(true);
        cell.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        cell.set_max_width_chars(40);
        cell.set_selectable(true);
        // 選べるラベルはフォーカスを受けると全体を選ぶので、開いたときに最初の値が選ばれた見た目になる。
        // フォーカスは取らない（マウスで選んでコピーはできる）。
        cell.set_focusable(false);
        cell.add_css_class("caption");
        if mono {
            cell.add_css_class("monospace");
        }
        if let Some(css) = css {
            cell.add_css_class(css);
        }
        grid.attach(&head, 0, i as i32, 1, 1);
        grid.attach(&cell, 1, i as i32, 1, 1);
    }
    let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
    column.set_margin_top(12);
    column.set_margin_bottom(12);
    column.set_margin_start(14);
    column.set_margin_end(14);
    column.set_size_request(380, -1);
    column.append(&header);
    column.append(&explanation);
    column.append(&grid);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&column));
    popover.set_parent(parent);
    if let Some(position) = position {
        popover.set_position(position);
    }
    popover.connect_closed(|p| {
        let p = p.clone();
        gtk::glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
    popover
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_follows_the_macos_rules() {
        let check = |status: &str, trust: &str, emails: Value| json!({ "status": status, "trust": trust, "emails": emails });
        let me = json!([{ "email": "a@x", "validity": "u" }]);
        assert_eq!(
            Status::new(
                Some(&check("G", "ultimate", me.clone())),
                "a@x",
                Some("gpg")
            ),
            Status::Verified(3)
        );
        assert_eq!(
            Status::new(
                Some(&check("G", "marginal", me.clone())),
                "a@x",
                Some("gpg")
            ),
            Status::Verified(2)
        );
        assert_eq!(
            Status::new(
                Some(&check("G", "undefined", me.clone())),
                "a@x",
                Some("gpg")
            ),
            Status::Untrusted
        );
        assert_eq!(
            Status::new(Some(&check("G", "full", me)), "b@x", Some("gpg")),
            Status::SignerMismatch
        );
        // SSH の署名は allowed signers に載っていれば信用している。
        assert_eq!(
            Status::new(Some(&check("G", "", json!([]))), "a@x", Some("ssh")),
            Status::Verified(3)
        );
        assert_eq!(Status::new(None, "a@x", None), Status::Pending);
        assert_eq!(grouped("ABCDEF0123"), "ABCD EF01 23");
    }
}
