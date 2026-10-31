//! CI の印（History の行の右端、サイドバーのプルリクエストの行）を押したときの吹き出し（macOS 版の `CIPopover`）。
//! GitHub のコミットのチェック（Actions のジョブと外部の status）を 1 つずつ、結果・名前・かかった時間で並べる。
//! 名前を押すとそのログを、下のボタンでコミットのチェックのページをブラウザで開く。チェックは開いてから読む
//! （印は一覧を描くときの状態だけで、詳細は持っていない）。

use gtk::glib;
use gtk::prelude::*;

use amonhen_model::tr;

use crate::github::{self, Ci, CommitCheck, GitHubRepository};

/// 状態の印（文字と色）の部品。
fn mark(ci: Ci, cancelled: bool, large: bool) -> gtk::Label {
    // 取り消しで終わったものは、失敗ではなく止まった印（灰色）。
    let (symbol, css) = if cancelled {
        ("⊘", "dim-label")
    } else {
        ci.symbol()
    };
    let l = gtk::Label::new(Some(symbol));
    l.add_css_class(css);
    if large {
        l.add_css_class("title-3");
    }
    l
}

/// `anchor`（の `rect`）を指して、`sha` のコミットのチェックの吹き出しを出す。
pub fn show(
    anchor: &gtk::Widget,
    rect: Option<gtk::gdk::Rectangle>,
    status: Ci,
    sha: &str,
    repo: GitHubRepository,
) -> gtk::Popover {
    let title = gtk::Label::new(Some(&status.label()));
    title.add_css_class("heading");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let badge = gtk::Label::new(Some(&sha[..sha.len().min(8)]));
    badge.add_css_class("monospace");
    badge.add_css_class("caption");
    badge.add_css_class("dim-label");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.append(&mark(status, false, true));
    header.append(&title);
    header.append(&badge);

    let note = gtk::Label::new(Some(&tr!("Loading the checks…")));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    note.add_css_class("caption");

    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);

    let open = gtk::Button::with_label(&tr!("Open on GitHub"));
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
    // History の行の印（位置あり）は下に、サイドバーの行の印は右に（左に切れないよう）。
    match rect {
        Some(rect) => {
            popover.set_position(gtk::PositionType::Bottom);
            popover.set_pointing_to(Some(&rect));
        }
        None => popover.set_position(gtk::PositionType::Right),
    }
    // 閉じたら外す（次は作り直す）。
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();

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
    popover
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
        row.append(&mark(check.ci(), check.is_cancelled(), false));
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
