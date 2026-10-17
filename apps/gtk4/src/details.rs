//! コミットの詳細（件名・本文・作者・日時・ID・親・ref）。コアの `commit_info` の答えを並べる。

use gtk::glib;
use gtk::prelude::*;
use serde_json::Value;

pub struct Details {
    pub root: gtk::ScrolledWindow,
    column: gtk::Box,
    subject: gtk::Label,
    meta: gtk::Label,
    body: gtk::Label,
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
        let subject = label(&["title-4"]);
        let meta = label(&["dim-label", "caption"]);
        let body = label(&[]);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.set_margin_top(10);
        column.set_margin_bottom(10);
        column.set_margin_start(12);
        column.set_margin_end(12);
        column.append(&subject);
        column.append(&meta);
        column.append(&body);
        let root = gtk::ScrolledWindow::builder()
            .child(&column)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let details = Self {
            root,
            column,
            subject,
            meta,
            body,
        };
        details.clear();
        details
    }

    pub fn clear(&self) {
        self.subject.set_text("");
        self.meta.set_text("");
        self.body.set_text("");
        self.body.set_visible(false);
    }

    pub fn show(&self, info: &Value) {
        let message = info["message"].as_str().unwrap_or_default();
        let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
        self.subject.set_text(subject.trim_end());
        let body = body.trim();
        self.body.set_text(body);
        self.body.set_visible(!body.is_empty());

        let mut lines = Vec::new();
        let who =
            |name: &str, email: &str, time: i64| format!("{name} <{email}>  ·  {}", date(time));
        let author = who(
            info["author"].as_str().unwrap_or_default(),
            info["authorEmail"].as_str().unwrap_or_default(),
            info["authorTime"].as_i64().unwrap_or_default(),
        );
        lines.push(format!("作者  {author}"));
        if info["committer"] != info["author"] || info["committerTime"] != info["authorTime"] {
            let committer = who(
                info["committer"].as_str().unwrap_or_default(),
                info["committerEmail"].as_str().unwrap_or_default(),
                info["committerTime"].as_i64().unwrap_or_default(),
            );
            lines.push(format!("コミット  {committer}"));
        }
        lines.push(format!("ID  {}", info["oid"].as_str().unwrap_or_default()));
        let parents: Vec<&str> = info["parents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(|p| &p[..p.len().min(8)]))
            .collect();
        if !parents.is_empty() {
            lines.push(format!("親  {}", parents.join("  ")));
        }
        let refs: Vec<&str> = info["refs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !refs.is_empty() {
            lines.push(format!("ref  {}", refs.join("  ")));
        }
        self.meta.set_text(&lines.join("\n"));
    }

    /// 幅 `width` で、中身をすべて見せるのに要る高さ。
    pub fn natural_height(&self, width: i32) -> i32 {
        let (_, natural, _, _) = self.column.measure(gtk::Orientation::Vertical, width);
        natural
    }
}

fn date(time: i64) -> String {
    glib::DateTime::from_unix_local(time)
        .and_then(|d| d.format("%Y-%m-%d %H:%M:%S"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}
