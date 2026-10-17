//! コミットの詳細。macOS 版の「詳細」の見せ方と同じ並び: 上に作者とコミッター、英語の見出しの表（Commit・
//! Parent・Child・Rev・Signature・Refs・Branches・Tags・Previous Tag・Pushed・Merged）、件名・本文・notes。
//! すぐ読める分（`commit_info`）を先に出し、時間のかかる分（`commit_extras`: 署名の検証・子・含むブランチと
//! タグ・notes など）が届いたら表を作り直す。

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use serde_json::Value;

pub struct Details {
    pub root: gtk::ScrolledWindow,
    column: gtk::Box,
    people: gtk::Label,
    table: gtk::Grid,
    subject: gtk::Label,
    body: gtk::Label,
    notes: gtk::Label,
    info: RefCell<Option<Value>>,
}

impl Details {
    pub fn new() -> Self {
        let label = |css: &[&str]| {
            let l = gtk::Label::new(None);
            l.set_xalign(0.0);
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            l.set_selectable(true);
            for c in css {
                l.add_css_class(c);
            }
            l
        };
        let people = label(&["caption"]);
        let table = gtk::Grid::builder()
            .column_spacing(10)
            .row_spacing(2)
            .build();
        let subject = label(&["title-4"]);
        let body = label(&[]);
        let notes = label(&["dim-label"]);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        column.set_margin_top(10);
        column.set_margin_bottom(10);
        column.set_margin_start(12);
        column.set_margin_end(12);
        column.append(&people);
        column.append(&table);
        column.append(&subject);
        column.append(&body);
        column.append(&notes);
        let root = gtk::ScrolledWindow::builder()
            .child(&column)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let details = Self {
            root,
            column,
            people,
            table,
            subject,
            body,
            notes,
            info: RefCell::default(),
        };
        details.clear();
        details
    }

    pub fn clear(&self) {
        self.info.replace(None);
        self.people.set_text("");
        self.clear_table();
        self.subject.set_text("");
        for l in [&self.body, &self.notes] {
            l.set_text("");
            l.set_visible(false);
        }
    }

    fn clear_table(&self) {
        while let Some(child) = self.table.first_child() {
            self.table.remove(&child);
        }
    }

    /// すぐ読める分を出す（時間のかかる分は、届いたら `show_extras`）。
    pub fn show(&self, info: &Value) {
        self.info.replace(Some(info.clone()));
        let message = info["message"].as_str().unwrap_or_default();
        let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
        self.subject.set_text(subject.trim_end());
        let body = body.trim();
        self.body.set_text(body);
        self.body.set_visible(!body.is_empty());

        // 上の帯: 作者と（違えば）コミッター。
        let who = |name: &str, email: &str, time: i64| format!("{name} <{email}>  {}", date(time));
        let mut people = format!(
            "Author  {}",
            who(
                str_of(info, "author"),
                str_of(info, "authorEmail"),
                info["authorTime"].as_i64().unwrap_or_default(),
            )
        );
        if info["committer"] != info["author"] || info["committerTime"] != info["authorTime"] {
            people.push_str(&format!(
                "\nCommitter  {}",
                who(
                    str_of(info, "committer"),
                    str_of(info, "committerEmail"),
                    info["committerTime"].as_i64().unwrap_or_default(),
                )
            ));
        }
        self.people.set_text(&people);
        self.build_table(None);
    }

    /// 時間のかかる分（`commit_extras` の答え）を足す。`info` と同じコミットのものであること。
    pub fn show_extras(&self, extras: &Value) {
        if self.info.borrow().is_none() {
            return;
        }
        self.build_table(Some(extras));
        let notes = str_of(extras, "notes").trim();
        self.notes.set_text(notes);
        self.notes.set_visible(!notes.is_empty());
    }

    fn build_table(&self, extras: Option<&Value>) {
        self.clear_table();
        let info = self.info.borrow();
        let Some(info) = info.as_ref() else { return };
        let mut rows: Vec<(String, String)> = vec![("Commit".into(), str_of(info, "oid").into())];
        let parents = strings(&info["parents"]);
        if !parents.is_empty() {
            let title = if parents.len() > 1 {
                "Parents"
            } else {
                "Parent"
            };
            rows.push((title.into(), parents.join("  ")));
        }
        if let Some(extras) = extras {
            let children = strings(&extras["children"]);
            if !children.is_empty() {
                let title = if children.len() > 1 {
                    "Children"
                } else {
                    "Child"
                };
                rows.push((title.into(), children.join("  ")));
            }
            if let Some(rev) = extras["headRev"].as_str().filter(|r| !r.is_empty()) {
                rows.push(("Rev".into(), rev.into()));
            }
        }
        if let Some(signature) = signature_text(info, extras) {
            rows.push(("Signature".into(), signature));
        }
        let refs = strings(&info["refs"]);
        if !refs.is_empty() {
            rows.push(("Refs".into(), refs.join(", ")));
        }
        if let Some(extras) = extras {
            let branches = strings(&extras["branches"]);
            if !branches.is_empty() {
                rows.push(("Branches".into(), branches.join(", ")));
            }
            let count = extras["tagCount"].as_u64().unwrap_or(0) as usize;
            if count > 0 {
                let tags = strings(&extras["tags"]);
                let shown = tags.iter().take(12).cloned().collect::<Vec<_>>().join(", ");
                let more = if count > 12 {
                    format!(" ほか {} 個", count - 12)
                } else {
                    String::new()
                };
                rows.push(("Tags".into(), shown + &more));
            }
            if let Some(previous) = extras["previousTag"].as_str() {
                rows.push(("Previous Tag".into(), previous.into()));
            }
            if !extras["originUrl"].is_null() {
                let remote = strings(&extras["remoteBranches"]);
                let pushed = if remote.is_empty() {
                    "未 push（どのリモートブランチにも含まれない）".to_owned()
                } else {
                    remote.join(", ")
                };
                rows.push(("Pushed".into(), pushed));
            }
            let merged = extras["mergedCount"].as_u64().unwrap_or(0);
            if merged > 0 {
                let mut text = format!("{merged} コミット");
                for commit in extras["merged"].as_array().into_iter().flatten().take(20) {
                    let oid = str_of(commit, "oid");
                    text.push_str(&format!(
                        "\n{}  {}",
                        &oid[..8.min(oid.len())],
                        str_of(commit, "subject")
                    ));
                }
                if merged > 20 {
                    text.push_str(&format!("\nほか {} 件", merged - 20));
                }
                rows.push(("Merged".into(), text));
            }
        }
        for (i, (title, value)) in rows.into_iter().enumerate() {
            let head = gtk::Label::new(Some(&title));
            head.set_xalign(1.0);
            head.set_yalign(0.0);
            head.add_css_class("dim-label");
            head.add_css_class("caption");
            let cell = gtk::Label::new(Some(&value));
            cell.set_xalign(0.0);
            cell.set_wrap(true);
            cell.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            cell.set_selectable(true);
            cell.set_hexpand(true);
            cell.add_css_class("caption");
            if title == "Commit" || title.starts_with("Parent") || title.starts_with("Child") {
                cell.add_css_class("monospace");
            }
            self.table.attach(&head, 0, i as i32, 1, 1);
            self.table.attach(&cell, 1, i as i32, 1, 1);
        }
    }

    /// 幅 `width` で、中身をすべて見せるのに要る高さ。
    pub fn natural_height(&self, width: i32) -> i32 {
        let (_, natural, _, _) = self.column.measure(gtk::Orientation::Vertical, width);
        natural
    }
}

/// 署名の行（macOS 版と同じ短い言葉と、種類・署名者）。署名がなければ None。
fn signature_text(info: &Value, extras: Option<&Value>) -> Option<String> {
    let kind = info["signed"].as_str();
    let check = extras.map(|e| &e["signature"]).filter(|s| !s.is_null());
    if kind.is_none() && check.is_none() {
        return None;
    }
    let label = match check {
        None if extras.is_none() => "Checking…",
        None => "No key",
        Some(s) => match str_of(s, "status") {
            "G" if matches!(str_of(s, "trust"), "ultimate" | "full" | "") => "Verified",
            "G" | "U" => "Untrusted key",
            "X" | "Y" => "Expired",
            "R" => "Revoked key",
            "B" => "Bad signature",
            _ => "No key",
        },
    };
    let signer = check.map(|s| str_of(s, "signer")).unwrap_or_default();
    let kind = kind.unwrap_or("gpg").to_uppercase();
    Some(if signer.is_empty() {
        format!("{label}  {kind}")
    } else {
        format!("{label}  {kind} · {signer}")
    })
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect()
}

fn date(time: i64) -> String {
    glib::DateTime::from_unix_local(time)
        .and_then(|d| d.format("%Y-%m-%d %H:%M:%S"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}
