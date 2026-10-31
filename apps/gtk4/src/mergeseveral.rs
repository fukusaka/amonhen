//! いくつものブランチ・タグ・コミットを今のブランチに merge する（macOS 版の `MergeSeveralSheet.swift`。
//! `git merge a b c`、1 つのコミットにまとめる octopus）。git は最後のもの以外でコンフリクトすると断るので、
//! 前もって調べ、そうなりそうなら、ぶつかるものを最後に回すか、1 つずつ merge することを勧める。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use amonhen_core::HistorySnapshot;
use amonhen_model::{RefKind, tr};
use gtk::prelude::*;
use gtk::{gdk, glib};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::sheets::{self, Response, Sheet};

/// 順に、コンフリクトする見込みのファイル（調べられなければ None）。
type Steps = Option<Vec<Vec<String>>>;

/// merge で取り込むもの 1 つ。種類は merge のメッセージの見本に使う。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub rev: String,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Branch,
    RemoteBranch,
    Tag,
    Commit,
}

/// History で選んだ行（昇順、上から）のうち、今のコミット以外を取り込むものにする。ブランチ・タグが指して
/// いればその名前で（merge のメッセージが git と同じになる）。
pub fn sources(actions: &Actions, history: &HistorySnapshot, rows: &[u32]) -> Vec<Source> {
    let refs = actions.repo().and_then(|r| r.refs().ok());
    rows.iter()
        .filter_map(|&row| {
            let id = *history.graph.ids().get(row as usize)?;
            if Some(id) == history.head {
                return None;
            }
            let commit = Source {
                rev: id.to_string(),
                kind: Kind::Commit,
            };
            let Some(refs) = &refs else {
                return Some(commit);
            };
            // History の行の名前と同じ順（ローカル、リモート、タグ、名前順）の最初のもの。今のブランチは除く。
            let label = refs
                .refs
                .entries
                .iter()
                .filter(|e| e.target == Some(id))
                .filter(|e| matches!(e.kind, RefKind::Local | RefKind::Remote | RefKind::Tag))
                .filter(|e| {
                    !(e.kind == RefKind::Local
                        && Some(e.name.as_str()) == refs.refs.head_branch.as_deref())
                })
                .min_by_key(|e| (e.kind, e.name.clone()));
            Some(match label {
                Some(e) => Source {
                    rev: e.name.clone(),
                    kind: match e.kind {
                        RefKind::Local => Kind::Branch,
                        RefKind::Remote => Kind::RemoteBranch,
                        _ => Kind::Tag,
                    },
                },
                None => commit,
            })
        })
        .collect()
}

/// git が付ける merge のメッセージ（`git fmt-merge-msg` と同じ形）。ブランチ、リモートブランチ、タグの順に
/// 種類ごとにまとめ、コミットは `;` の後に。`main`・`master` 以外へなら ` into <branch>` が付く。
pub fn merge_message(sources: &[Source], branch: Option<&str>) -> String {
    fn joined(names: &[&str]) -> String {
        let quoted: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
        match quoted.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => quoted.first().cloned().unwrap_or_default(),
        }
    }
    let names = |kind: Kind| -> Vec<&str> {
        sources
            .iter()
            .filter(|s| s.kind == kind)
            .map(|s| s.rev.as_str())
            .collect()
    };
    let mut groups = Vec::new();
    for (kind, one, many) in [
        (Kind::Branch, "branch", "branches"),
        (
            Kind::RemoteBranch,
            "remote-tracking branch",
            "remote-tracking branches",
        ),
        (Kind::Tag, "tag", "tags"),
    ] {
        let list = names(kind);
        if !list.is_empty() {
            let word = if list.len() == 1 { one } else { many };
            groups.push(format!("{word} {}", joined(&list)));
        }
    }
    let mut text = format!("Merge {}", groups.join(", "));
    let commits = names(Kind::Commit);
    if !commits.is_empty() {
        let word = if commits.len() == 1 {
            "commit"
        } else {
            "commits"
        };
        let sep = if groups.is_empty() { "" } else { "; " };
        text.push_str(&format!("{sep}{word} {}", joined(&commits)));
    }
    if !matches!(branch, Some("main" | "master")) {
        text.push_str(&format!(" into {}", branch.unwrap_or("HEAD")));
    }
    text
}

/// 確認のシートを出してから merge する。`branch` は今のブランチ（detached HEAD なら None）。
pub fn confirm(actions: &Rc<Actions>, sources: Vec<Source>, branch: Option<String>) {
    if sources.len() < 2 {
        if let Some(s) = sources.first() {
            actions.perform(json!({ "op": "merge", "rev": s.rev }));
        }
        return;
    }
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let target = branch.clone().unwrap_or_else(|| "HEAD".to_owned());
        let sheet = Sheet::new(
            sheets::parent(&actions).as_ref(),
            &tr!(
                "Merge {count} into {branch}?",
                count = sources.len(),
                branch = target
            ),
            &tr!(
                "This makes one merge commit with all of them as parents, as git merge does with several branches. Drag to change the order. Undo puts it back."
            ),
        );
        let list = OrderList::new(sources.clone());
        sheet.add(&list.scrolled);
        let label = sheets::forecast_label();
        sheet.add(&label);
        let put_last = gtk::Button::with_label("");
        put_last.set_halign(gtk::Align::Start);
        // 答えが来るまでは見えないようにして、場所は取っておく（シートの大きさを変えないため）。
        hide_keeping_space(&put_last, true);
        sheet.add(&put_last);
        let message = sheets::note("");
        message.set_lines(2);
        message.set_ellipsize(gtk::pango::EllipsizeMode::End);
        sheet.add(&message);
        sheet.buttons("Merge", false, Some(&tr!("Merge One by One")));

        // 順を変えるたびに調べる（答えは順ごとに覚えておき、遅れて来た前の答えは捨てる）。
        let known: Rc<RefCell<HashMap<Vec<String>, Steps>>> = Rc::default();
        let shown: Rc<RefCell<Vec<String>>> = Rc::default();
        // 「最後に回す」で動かす行。
        let last: Rc<RefCell<Option<usize>>> = Rc::default();
        let show = {
            let (list, label, put_last, last) =
                (list.clone(), label.clone(), put_last.clone(), last.clone());
            Rc::new(move |steps: Option<Vec<Vec<String>>>| {
                list.set_steps(steps.clone());
                let sources = list.sources();
                let conflicting: Vec<usize> = steps
                    .iter()
                    .flatten()
                    .enumerate()
                    .filter(|(_, c)| !c.is_empty())
                    .map(|(i, _)| i)
                    .collect();
                let early: Vec<usize> = conflicting
                    .iter()
                    .copied()
                    .filter(|&i| i + 1 < sources.len())
                    .collect();
                last.replace(None);
                if early.is_empty() {
                    let mut all: Option<Vec<String>> = steps.map(|s| s.concat());
                    if let Some(all) = &mut all {
                        all.sort();
                        all.dedup();
                    }
                    sheets::show_forecast(&label, &sheets::MergeForecast { conflicts: all }, false);
                } else {
                    let names: Vec<&str> = early.iter().map(|&i| sources[i].rev.as_str()).collect();
                    label.set_text(&if conflicting.len() == 1 {
                        tr!(
                            "⚠︎ {name} conflicts, and git stops when one before the last conflicts. Put it last to resolve the conflicts in this merge, or merge them one by one.",
                            name = names[0]
                        )
                    } else {
                        tr!(
                            "⚠︎ {names} conflict, and git stops when one before the last conflicts. Merge them one by one instead.",
                            names = names.join(", ")
                        )
                    });
                    label.add_css_class("warning");
                    if conflicting.len() == 1 {
                        last.replace(Some(early[0]));
                    }
                }
                match *last.borrow() {
                    Some(i) => {
                        put_last.set_label(&tr!("Put {name} Last", name = sources[i].rev));
                        hide_keeping_space(&put_last, false);
                    }
                    None => hide_keeping_space(&put_last, true),
                }
            })
        };
        let update = {
            let (list, message, label, put_last, branch) = (
                list.clone(),
                message.clone(),
                label.clone(),
                put_last.clone(),
                branch.clone(),
            );
            Rc::new(move || {
                let sources = list.sources();
                let revs: Vec<String> = sources.iter().map(|s| s.rev.clone()).collect();
                message.set_text(&tr!(
                    "Message: {message}",
                    message = merge_message(&sources, branch.as_deref())
                ));
                shown.replace(revs.clone());
                if let Some(steps) = known.borrow().get(&revs).cloned() {
                    return show(steps);
                }
                list.set_steps(None);
                hide_keeping_space(&put_last, true);
                label.set_text(&tr!("Checking for conflicts…"));
                label.remove_css_class("warning");
                let (repo, known, shown, show) =
                    (repo.clone(), known.clone(), shown.clone(), show.clone());
                glib::spawn_future_local(async move {
                    let steps = predict(&repo, &revs).await;
                    known.borrow_mut().insert(revs.clone(), steps.clone());
                    if *shown.borrow() == revs {
                        show(steps);
                    }
                });
            })
        };
        let u = update.clone();
        list.connect_reorder(move || u());
        let (l, u) = (list.clone(), update.clone());
        put_last.connect_clicked(move |_| {
            if let Some(i) = *last.borrow() {
                l.move_to_end(i);
            }
            u();
        });
        update();
        let response = sheet.run().await;
        let revs: Vec<String> = list.sources().into_iter().map(|s| s.rev).collect();
        match response {
            Response::Ok => merge(&actions, revs, false),
            Response::Other => merge(&actions, revs, true),
            Response::Cancel => {}
        }
    });
}

/// `predictMerge` の、順ごとのコンフリクトするファイル（調べられなければ None）。
async fn predict(
    repo: &std::sync::Arc<amonhen_core::Repository>,
    revs: &[String],
) -> Option<Vec<Vec<String>>> {
    let reply = sheets::run(
        repo,
        json!({ "op": "predictMerge", "ours": "HEAD", "theirs": revs }),
    )
    .await;
    reply["info"]["steps"].as_array().map(|steps| {
        steps
            .iter()
            .map(|s| {
                s["conflicts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c.as_str().map(str::to_owned))
                    .collect()
            })
            .collect()
    })
}

/// `revs` を merge する。1 つのコミットにまとめるのを git が断ったら（何も変わっていない）、1 つずつ merge
/// するかを尋ねる。
fn merge(actions: &Rc<Actions>, revs: Vec<String>, one_by_one: bool) {
    let mut command = json!({ "op": "merge", "rev": revs });
    if one_by_one {
        command["oneByOne"] = json!(true);
    }
    let a = actions.clone();
    actions.perform_with(vec![command], move |reply: &Value| {
        if one_by_one || reply["errorKind"].as_str() != Some("octopus") {
            return;
        }
        let a = a.clone();
        glib::spawn_future_local(async move {
            let sheet = Sheet::new(
                sheets::parent(&a).as_ref(),
                &tr!("Can’t Merge Them in One Commit"),
                &tr!(
                    "git stopped because one of them conflicts before the last, and nothing was changed. Merge them one by one instead? Each makes its own merge commit, and you resolve conflicts as they come. Undo puts them all back."
                ),
            );
            sheet.buttons(&tr!("Merge One by One"), false, None);
            if sheet.run().await == Response::Ok {
                merge(&a, revs, true);
            }
        });
    });
}

/// 見えなくする（`hide` なら）。場所は取っておく。
fn hide_keeping_space(widget: &impl IsA<gtk::Widget>, hide: bool) {
    widget.set_opacity(if hide { 0.0 } else { 1.0 });
    widget.set_can_target(!hide);
    widget.set_sensitive(!hide);
}

/// 取り込む順の一覧。ドラッグで並べ替え、それぞれの右にコンフリクトの見込みを出す。
struct OrderList {
    scrolled: gtk::ScrolledWindow,
    list: gtk::ListBox,
    sources: RefCell<Vec<Source>>,
    /// 順に、コンフリクトする見込みのファイル（調べていなければ None）。
    steps: RefCell<Option<Vec<Vec<String>>>>,
    on_reorder: RefCell<Option<Rc<dyn Fn()>>>,
}

/// 一覧の 1 行の高さ（6 行を超えるときだけスクロールする）。
const ROW_HEIGHT: i32 = 34;

impl OrderList {
    fn new(sources: Vec<Source>) -> Rc<Self> {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_width(440)
            .propagate_natural_height(true)
            .max_content_height(6 * ROW_HEIGHT)
            .build();
        scrolled.add_css_class("frame");
        let this = Rc::new(Self {
            scrolled,
            list,
            sources: RefCell::new(sources),
            steps: RefCell::default(),
            on_reorder: RefCell::default(),
        });
        this.refresh();
        this
    }

    fn sources(&self) -> Vec<Source> {
        self.sources.borrow().clone()
    }

    fn connect_reorder(&self, f: impl Fn() + 'static) {
        self.on_reorder.replace(Some(Rc::new(f)));
    }

    fn set_steps(self: &Rc<Self>, steps: Option<Vec<Vec<String>>>) {
        self.steps.replace(steps);
        self.refresh();
    }

    fn move_to_end(self: &Rc<Self>, index: usize) {
        {
            let mut sources = self.sources.borrow_mut();
            let s = sources.remove(index);
            sources.push(s);
        }
        self.set_steps(None);
    }

    /// `from` 行目を `to` 行目に動かして、知らせる。
    fn reorder(self: &Rc<Self>, from: usize, to: usize) {
        {
            let mut sources = self.sources.borrow_mut();
            if from == to || from >= sources.len() || to >= sources.len() {
                return;
            }
            let s = sources.remove(from);
            sources.insert(to, s);
        }
        self.set_steps(None);
        let f = self.on_reorder.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }

    /// 行を作り直す（数は少ない）。
    fn refresh(self: &Rc<Self>) {
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        let steps = self.steps.borrow().clone();
        for (i, source) in self.sources.borrow().iter().enumerate() {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            line.set_margin_start(8);
            line.set_margin_end(8);
            line.set_height_request(ROW_HEIGHT - 6);
            let name = gtk::Label::new(Some(&format!("{}. {}", i + 1, source.rev)));
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            line.append(&name);
            let forecast = gtk::Label::new(None);
            forecast.set_xalign(1.0);
            forecast.set_max_width_chars(24);
            forecast.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if let Some(files) = steps.as_ref().and_then(|s| s.get(i)) {
                if files.is_empty() {
                    forecast.set_text("✓");
                    forecast.add_css_class("dim-label");
                } else {
                    forecast.set_text(&format!("⚠︎ {}", files.join(", ")));
                    forecast.add_css_class("warning");
                    forecast.set_tooltip_text(Some(&files.join("\n")));
                }
            }
            line.append(&forecast);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&line));
            row.set_activatable(false);
            // ドラッグで並べ替える（持ち上げた行の番号を運ぶ）。
            let drag = gtk::DragSource::new();
            drag.set_actions(gdk::DragAction::MOVE);
            drag.connect_prepare(move |_, _, _| {
                Some(gdk::ContentProvider::for_value(&(i as u32).to_value()))
            });
            row.add_controller(drag);
            let drop = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
            let weak = Rc::downgrade(self);
            drop.connect_drop(move |_, value, _, _| {
                let (Some(this), Ok(from)) = (weak.upgrade(), value.get::<u32>()) else {
                    return false;
                };
                // 落とした後で作り直す（落とした先の行を、処理の途中で外さない）。
                glib::idle_add_local_once(move || this.reorder(from as usize, i));
                true
            });
            row.add_controller(drop);
            self.list.append(&row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(rev: &str, kind: Kind) -> Source {
        Source {
            rev: rev.into(),
            kind,
        }
    }

    #[test]
    fn writes_the_message_as_git_does() {
        let sources = [
            s("a", Kind::Branch),
            s("b", Kind::Branch),
            s("c", Kind::Branch),
        ];
        assert_eq!(
            merge_message(&sources, Some("main")),
            "Merge branches 'a', 'b' and 'c'"
        );
        let sources = [
            s("topic", Kind::Branch),
            s("origin/x", Kind::RemoteBranch),
            s("v1", Kind::Tag),
            s("1234", Kind::Commit),
        ];
        assert_eq!(
            merge_message(&sources, Some("dev")),
            "Merge branch 'topic', remote-tracking branch 'origin/x', tag 'v1'; commit '1234' into dev"
        );
        assert_eq!(
            merge_message(&[s("1", Kind::Commit), s("2", Kind::Commit)], None),
            "Merge commits '1' and '2' into HEAD"
        );
    }
}
