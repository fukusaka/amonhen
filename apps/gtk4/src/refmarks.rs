//! サイドバーの行の右端の印を押したときの吹き出し（macOS 版の `BranchStatusPopover`・`TagStatusPopover`）。何を
//! 意味するか、どこから分かったか、そのブランチの状態（上流・土台・最後のコミット）やタグのリモートごとの様子と、
//! 次にできること（消す・アーカイブ、タグの push・置き換え）。

use std::rc::Rc;

use amonhen_model::{RefEntry, tr};
use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

use crate::actions::Actions;
use crate::icon::{self, Icon};
use crate::reffacts::TagState;
use crate::sheets;
use crate::sidebar::Known;

/// 見出し（アイコン・題）、説明、「名前: 値」の表、ボタンの吹き出しを `anchor` の下に出す。
struct Popover {
    popover: gtk::Popover,
    content: gtk::Box,
    grid: gtk::Grid,
    rows: i32,
    buttons: gtk::Box,
}

impl Popover {
    fn new(
        anchor: &gtk::Widget,
        kind: icon::Kind,
        css: Option<&str>,
        title: &str,
        text: &str,
    ) -> Self {
        let mark = Icon::default();
        mark.set(kind, false);
        if let Some(css) = css {
            mark.add_css_class(css);
        }
        let heading = gtk::Label::new(Some(title));
        heading.add_css_class("heading");
        heading.set_xalign(0.0);
        heading.set_wrap(true);
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
        if !text.is_empty() {
            let explanation = sheets::note(text);
            explanation.set_max_width_chars(56);
            content.append(&explanation);
        }
        let grid = gtk::Grid::new();
        grid.set_row_spacing(5);
        grid.set_column_spacing(10);
        content.append(&grid);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&content));
        popover.set_parent(anchor);
        popover.set_position(gtk::PositionType::Right);
        // 閉じたら外す（次は作り直す）。
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        Self {
            popover,
            content,
            grid,
            rows: 0,
            buttons,
        }
    }

    /// 表に「名前: 値」の行を足し、値の部品を返す。
    fn row(&mut self, name: &str, value: &str, mono: bool, css: Option<&str>) -> gtk::Label {
        let l = gtk::Label::new(Some(name));
        l.set_xalign(1.0);
        l.set_yalign(0.0);
        l.add_css_class("dim-label");
        l.add_css_class("caption-heading");
        let v = gtk::Label::new(Some(value));
        v.set_xalign(0.0);
        v.set_wrap(true);
        v.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        v.set_max_width_chars(40);
        v.set_hexpand(true);
        if mono {
            v.add_css_class("monospace");
        }
        if let Some(css) = css {
            v.add_css_class(css);
        }
        self.grid.attach(&l, 0, self.rows, 1, 1);
        self.grid.attach(&v, 1, self.rows, 1, 1);
        self.rows += 1;
        v
    }

    /// 次にできることのボタン（最初のものを強調する）。押したら閉じて `f`。
    fn button(&self, label: &str, f: impl Fn() + 'static) {
        let b = gtk::Button::with_label(label);
        if self.buttons.first_child().is_none() {
            b.add_css_class("suggested-action");
        }
        let p = self.popover.downgrade();
        b.connect_clicked(move |_| {
            if let Some(p) = p.upgrade() {
                p.popdown();
            }
            f();
        });
        self.buttons.append(&b);
    }

    fn popup(self) -> gtk::Popover {
        if self.buttons.first_child().is_some() {
            self.content.append(&self.buttons);
        }
        self.popover.popup();
        self.popover
    }
}

/// タグの印（どのリモートにもない・同じ名前の別のタグがあるリモートがある）の詳細。タグはブランチと違って手元に
/// リモートの写しがないので、どこから分かったか（fetch のときに読んだ）と、リモートごとの様子を出す。
pub fn tag(actions: &Rc<Actions>, item: &RefEntry, known: &Known, anchor: &gtk::Widget) {
    let Some(tags) = &known.tags else { return };
    let Some((kind, title, css)) = tags.mark(&item.name) else {
        return;
    };
    let remotes = known.remote_names();
    let different = remotes
        .iter()
        .any(|r| tags.state(&item.name, r) == Some(TagState::Different));
    let text = if different {
        tr!(
            "There, a tag with the same name is a different tag: it points to another commit, or it is another annotated tag on the same commit. Fetch doesn’t overwrite your tag and Push is refused, so the two stay apart until one of them is replaced."
        )
    } else {
        tr!(
            "Tags have no remote copies here the way branches do, so this was read from each remote at the last Fetch. Push doesn’t send tags; push this one to share it."
        )
    };
    let mut p = Popover::new(anchor, kind, css, &title, &text);
    p.row(&tr!("Tag"), &item.name, false, None);
    if let Some(target) = item.target {
        let hex = target.to_string();
        p.row(&tr!("Commit"), &hex[..hex.len().min(12)], true, None);
    }
    for remote in &remotes {
        let (text, css) = match tags.state(&item.name, remote) {
            Some(TagState::Same) => (tr!("Has it"), None),
            Some(TagState::Different) => {
                (tr!("Has a different tag with this name"), Some("warning"))
            }
            Some(TagState::Missing) => (tr!("Doesn’t have it"), Some("dim-label")),
            None => (tr!("Not checked yet (Fetch reads it)"), Some("dim-label")),
        };
        let when = tags
            .checked(remote)
            .map(|ms| format!(" · {}", tr!("read {time}", time = date(ms / 1000))))
            .unwrap_or_default();
        p.row(remote, &format!("{text}{when}"), false, css);
    }
    // 次にできること: ないリモートへ push、別のタグがあるリモートは置き換え（確かめてから）。
    for remote in &remotes {
        if tags.state(&item.name, remote) == Some(TagState::Missing) {
            let (a, name, r) = (actions.clone(), item.name.clone(), remote.clone());
            p.button(&format!("Push to {remote}"), move || {
                a.perform(json!({ "op": "pushTags", "remote": r, "names": [name] }))
            });
        }
    }
    for remote in &remotes {
        if tags.state(&item.name, remote) == Some(TagState::Different) {
            let (a, name, r) = (actions.clone(), item.name.clone(), remote.clone());
            p.button(&tr!("Replace on {remote}…", remote = remote), move || {
                crate::refops::confirm_replace(&a, std::slice::from_ref(&name), &r)
            });
        }
    }
    p.popup();
}

fn date(t: i64) -> String {
    glib::DateTime::from_unix_local(t)
        .and_then(|d| d.format("%Y-%m-%d %H:%M"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// ローカルブランチの印（取り込み済み・gone・↑↓）の詳細。
pub fn branch(
    actions: &Rc<Actions>,
    item: &RefEntry,
    known: &Known,
    is_head: bool,
    anchor: &gtk::Widget,
) -> gtk::Popover {
    let fact = known.groups.as_ref().and_then(|g| g.get(&item.name));
    let merged = fact.is_some_and(|f| f.merged && !f.at_base);
    let base = known
        .groups
        .as_ref()
        .map_or("main", |g| g.base_name())
        .to_owned();
    let upstream = item.detail.clone().unwrap_or_default();
    // 見出し: いちばん大事な状態。
    let (kind, css, title, text) = match (merged, item.gone) {
        (true, true) => (
            icon::Kind::Merge,
            Some("success"),
            tr!("Merged and cleaned up on the remote"),
            tr!(
                "All of this branch’s commits are in {base}, and its upstream {upstream} has been deleted on the remote (typically after the pull request was merged). It is safe to delete.",
                base = base,
                upstream = upstream
            ),
        ),
        (true, false) => (
            icon::Kind::Merge,
            Some("success"),
            tr!("Merged into {base}", base = base),
            tr!(
                "All of this branch’s commits are reachable from {base} (found in the local history, not from GitHub). A branch merged by squash or rebase on GitHub does not count, because its commits were rewritten.",
                base = base
            ),
        ),
        (false, true) => (
            icon::Kind::CloudSlash,
            Some("warning"),
            tr!("Upstream is gone"),
            tr!(
                "This branch tracks {upstream}, which no longer exists on the remote. Its commits are not in {base} in the local history: the pull request was merged by squash or rebase, or it was closed without merging. Check before deleting.",
                upstream = upstream,
                base = base
            ),
        ),
        _ if item.ahead > 0 || item.behind > 0 => {
            let title = if item.ahead > 0 && item.behind > 0 {
                tr!("Diverged from {upstream}", upstream = upstream)
            } else if item.ahead > 0 {
                tr!("Ahead of {upstream}", upstream = upstream)
            } else {
                tr!("Behind {upstream}", upstream = upstream)
            };
            let text = if item.ahead > 0 && item.behind > 0 {
                tr!(
                    "{ahead} commits here are not on the upstream, and {behind} commits there are not here. Pull brings theirs in (merge or rebase), then Push sends yours.",
                    ahead = item.ahead,
                    behind = item.behind
                )
            } else if item.ahead > 0 {
                tr!(
                    "{count} commits here are not on the upstream yet. Push sends them.",
                    count = item.ahead
                )
            } else {
                tr!(
                    "{count} commits on the upstream are not here yet. Pull brings them in.",
                    count = item.behind
                )
            };
            (icon::Kind::Push, None, title, text)
        }
        _ => (icon::Kind::Branch, None, item.name.clone(), String::new()),
    };
    let mut p = Popover::new(anchor, kind, css, &title, &text);
    p.row(&tr!("Branch"), &item.name, false, None);
    if let Some(target) = item.target {
        let hex = target.to_string();
        p.row(&tr!("Tip"), &hex[..hex.len().min(12)], true, None);
    }
    if let Some(time) = fact.and_then(|f| f.time) {
        p.row(&tr!("Last commit"), &date(time), false, None);
    }
    if item.detail.is_some() {
        let state = if item.gone {
            tr!("{upstream} (deleted on the remote)", upstream = upstream)
        } else {
            let mut parts = Vec::new();
            if item.ahead > 0 {
                parts.push(tr!("{count} ahead", count = item.ahead));
            }
            if item.behind > 0 {
                parts.push(tr!("{count} behind", count = item.behind));
            }
            if parts.is_empty() {
                upstream.clone()
            } else {
                format!("{upstream} ({})", parts.join(&tr!(", ")))
            }
        };
        let css = item.gone.then_some("warning");
        p.row(&tr!("Upstream"), &state, false, css);
    }
    if let Some(parent) = fact.and_then(|f| f.parent.as_deref()) {
        p.row(&tr!("Built on"), parent, false, None);
    }
    if merged {
        p.row(&tr!("Merged into"), &base, false, Some("success"));
    }
    // 既定のブランチに取り込んだらコンフリクトするか。取り込み済み・既定のブランチそのものなら調べない。
    let forecast = (known.groups.is_some()
        && !merged
        && !fact.is_some_and(|f| f.at_base)
        && item.name != base
        && !base.ends_with(&format!("/{}", item.name)))
    .then(|| {
        p.row(
            &tr!("Merging into {base}", base = base),
            &tr!("Checking for conflicts…"),
            false,
            Some("dim-label"),
        )
    });
    // 次にできること。消すのは merged のときだけ（git の -d と同じく、取り込んでいないものは消さない）。
    if !is_head {
        if merged {
            let (a, name) = (actions.clone(), item.name.clone());
            p.button(&tr!("Delete Branch"), move || {
                sheets::delete_branches(&a, vec![name.clone()])
            });
        }
        if !fact.is_some_and(|f| f.archived) {
            let (a, name) = (actions.clone(), item.name.clone());
            p.button(&tr!("Archive"), move || {
                a.perform(json!({
                    "op": "configSet", "key": format!("branch.{name}.amonhenArchived"), "value": "true",
                }))
            });
        }
    }
    let popover = p.popup();
    if let (Some(label), Some(repo)) = (forecast, actions.repo()) {
        let name = item.name.clone();
        glib::spawn_future_local(async move {
            let answer = sheets::predict_merge(&repo, &base, &[name]).await;
            label.remove_css_class("dim-label");
            label.set_text(&answer.text(false));
            if answer.has_conflicts() {
                label.add_css_class("warning");
            }
        });
    }
    popover
}
