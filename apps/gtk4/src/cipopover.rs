//! CI の印（History の行の右端、サイドバーのプルリクエストの行）を押したときの吹き出し（macOS 版の `CIPopover`）。
//! GitHub のコミットのチェック（Actions のジョブと外部の status）を 1 つずつ、結果・名前・かかった時間で並べる。
//! 名前を押すとそのログを、下のボタンでコミットのチェックのページをブラウザで開く。チェックは開いてから読む
//! （印は一覧を描くときの状態だけで、詳細は持っていない）。

use gtk::glib;
use gtk::prelude::*;

use amonhen_model::tr;

use crate::github::{self, Ci, CommitCheck, GitHubRepository};

/// 色の付いた状態の記号（macOS 版の `look` の SF Symbols）。見出しは 18 px、チェックの行は 12 px。
/// 取り消しで終わったものは、失敗ではなく止まった印（灰色）。
fn ci_icon(ci: Ci, cancelled: bool, size: i32) -> gtk::Widget {
    use crate::icon::{Icon, Kind};
    let (kind, css) = match ci {
        _ if cancelled => (Kind::SlashCircle, Some("dim-label")),
        Ci::Passing => (Kind::CheckCircleFill, Some("success")),
        Ci::Failing => (Kind::XCircleFill, Some("error")),
        Ci::Pending => (Kind::CircleFill, Some("warning")),
        Ci::None => (Kind::Circle, Some("dim-label")),
    };
    let icon = Icon::default();
    icon.set(kind, false);
    icon.set_pixel_size(size);
    if let Some(css) = css {
        icon.add_css_class(css);
    }
    icon.upcast()
}

/// 吹き出しの札とボタンの見た目（1 度だけ入れる）。
fn install_css() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "label.ci-badge { background-color: alpha(currentColor, 0.15); border-radius: 4px; \
         padding: 0 4px; opacity: 0.75; }\n\
         button.ci-open { min-height: 0; padding: 2px 8px; font-size: 0.85em; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

thread_local! {
    /// 出している吹き出しと、その印（`show` の `key`）。
    static SHOWN: std::cell::RefCell<Option<(String, glib::WeakRef<gtk::Popover>)>> =
        const { std::cell::RefCell::new(None) };
    /// 最後に閉じた吹き出しの印と、閉じた時刻。
    static CLOSED: std::cell::RefCell<Option<(String, std::time::Instant)>> =
        const { std::cell::RefCell::new(None) };
}

/// `anchor`（の `rect`）を指して、`sha` のコミットのチェックの吹き出しを出す。同じ印（`key`）をもう一度押したら
/// 閉じるだけ（macOS 版の `CIPopoverPresenter` と同じ。押した瞬間に吹き出しが閉じ、すぐ出し直すと閉じられない
/// ように見えるので、閉じてから 0.3 秒の間の同じ印は何もしない）。`key` は History の行ならコミット、プルリクエスト
/// の行なら `pr<番号>`（macOS 版と同じ。同じコミットでも別の印なら続けて開ける）。出したら、その吹き出し。
pub fn show(
    anchor: &gtk::Widget,
    rect: Option<gtk::gdk::Rectangle>,
    status: Ci,
    sha: &str,
    key: &str,
    repo: GitHubRepository,
) -> Option<gtk::Popover> {
    let open = SHOWN.with(|s| {
        s.borrow()
            .as_ref()
            .filter(|(shown, _)| shown == key)
            .and_then(|(_, p)| p.upgrade())
            .filter(|p| p.is_visible())
    });
    if let Some(open) = open {
        open.popdown();
        return None;
    }
    let just_closed = CLOSED.with(|c| {
        c.borrow().as_ref().is_some_and(|(closed, at)| {
            closed == key && at.elapsed() < std::time::Duration::from_millis(300)
        })
    });
    if just_closed {
        CLOSED.with(|c| c.replace(None));
        return None;
    }
    install_css();
    let title = gtk::Label::new(Some(&status.label()));
    title.add_css_class("heading");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    // 短い ID は、薄い色を敷いた角丸の札（macOS 版と同じ）。
    let badge = gtk::Label::new(Some(&sha[..sha.len().min(8)]));
    badge.add_css_class("monospace");
    badge.add_css_class("caption");
    badge.add_css_class("ci-badge");
    badge.set_valign(gtk::Align::Center);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.append(&ci_icon(status, false, 18));
    header.append(&title);
    header.append(&badge);

    let note = gtk::Label::new(Some(&tr!("Loading the checks…")));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    note.add_css_class("caption");

    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);

    // 小さなボタン（macOS 版の `.small`）。
    let open = gtk::Button::with_label(&tr!("Open on GitHub"));
    open.add_css_class("ci-open");
    open.set_halign(gtk::Align::End);
    let checks_url = format!("{}/commit/{sha}/checks", repo.web_url());
    open.connect_clicked(move |b| {
        let window = b.root().and_downcast::<gtk::Window>();
        github::open_uri(window.as_ref(), &checks_url);
    });

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_width_request(420);
    content.append(&header);
    content.append(&note);
    content.append(&list);
    content.append(&open);

    let popover = gtk::Popover::new();
    popover.set_child(Some(&content));
    popover.set_parent(anchor);
    // 印の下に出す（History の行もサイドバーの行も。macOS 版の .maxY と同じ）。
    popover.set_position(gtk::PositionType::Bottom);
    if let Some(rect) = rect {
        popover.set_pointing_to(Some(&rect));
    }
    // 閉じたら外す（次は作り直す）。閉じたコミットと時刻を覚える。
    let closed_key = key.to_owned();
    popover.connect_closed(move |p| {
        CLOSED.with(|c| c.replace(Some((closed_key.clone(), std::time::Instant::now()))));
        SHOWN.with(|s| {
            let mut s = s.borrow_mut();
            if s.as_ref()
                .is_some_and(|(_, w)| w.upgrade().as_ref() == Some(p))
            {
                *s = None;
            }
        });
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
    SHOWN.with(|s| s.replace(Some((key.to_owned(), popover.downgrade()))));

    let sha = sha.to_owned();
    let (note, list) = (note.downgrade(), list.downgrade());
    glib::spawn_future_local(async move {
        let result = github::commit_checks(&repo, &sha).await;
        let (Some(note), Some(list)) = (note.upgrade(), list.upgrade()) else {
            return;
        };
        match result {
            Ok(all) => fill(&note, &list, &all),
            Err(e) => note.set_text(&tr!("Couldn’t read the checks: {error}", error = e)),
        }
    });
    Some(popover)
}

/// 読めたチェックを並べる（失敗を上に、走っているもの、取り消し、成功の順）。
fn fill(note: &gtk::Label, list: &gtk::Box, all: &[CommitCheck]) {
    // 同じ名前の run は 1 つに（成功があれば、やり直す前の取り消しは出さない）。
    let mut checks = CommitCheck::collapsed(all);
    if checks.is_empty() {
        note.set_text(&tr!("No checks on this commit."));
        return;
    }
    note.set_text(&tr!("{count} checks", count = checks.len()));
    let order = |c: &CommitCheck| match c.ci() {
        Ci::Failing => 0,
        Ci::Pending => 1,
        Ci::None if c.is_cancelled() => 2,
        Ci::None => 4,
        Ci::Passing => 3,
    };
    checks.sort_by(|a, b| (order(a), &a.name).cmp(&(order(b), &b.name)));
    for check in checks {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&ci_icon(check.ci(), check.is_cancelled(), 12));
        // 名前が長いときは名前を省略し、印と結果は縮めない。
        // URL があれば、押すとそのログをブラウザで開くリンク。
        let name = gtk::Label::new(None);
        match &check.url {
            Some(url) => {
                name.set_markup(&format!(
                    "<a href=\"{}\">{}</a>",
                    glib::markup_escape_text(url),
                    glib::markup_escape_text(&check.name)
                ));
                name.set_tooltip_text(Some(&format!("{}\n{url}", check.name)));
            }
            None => {
                name.set_text(&check.name);
                name.set_tooltip_text(Some(&check.name));
            }
        }
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_xalign(0.0);
        name.set_hexpand(true);
        row.append(&name);
        let mut text = check.result_label();
        if let Some(s) = check.seconds {
            text.push_str("  ");
            text.push_str(&duration(s));
        }
        let result = gtk::Label::new(Some(&text));
        result.add_css_class("dim-label");
        result.add_css_class("caption");
        row.append(&result);
        list.append(&row);
    }
}

fn duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(name: &str, result: &str) -> CommitCheck {
        CommitCheck {
            name: name.into(),
            result: result.into(),
            url: None,
            seconds: None,
        }
    }

    #[test]
    fn collapses_reruns() {
        let all = [
            check("build", "cancelled"),
            check("build", "success"),
            check("test", "failure"),
            check("test", "in_progress"),
        ];
        let out = CommitCheck::collapsed(&all);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].result, "success");
        assert_eq!(out[1].result, "failure");
    }

    #[test]
    fn formats_durations() {
        assert_eq!(duration(42), "42s");
        assert_eq!(duration(125), "2m 5s");
        assert_eq!(duration(3720), "1h 2m");
    }
}
