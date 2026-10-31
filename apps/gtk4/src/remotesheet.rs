//! リモートの追加・編集のシート（macOS 版の `RemoteSheet.swift`。docs/ux.md の 4.5 のリモート）: 名前、fetch の
//! URL、push の URL（空なら fetch と同じ）、URL の先が何か（GitHub・GitLab・パス・SSH 先）、保存する前の接続の確認
//! （コアの `checkRemote`。SSH 先のリポジトリなら SSH 先から）。編集なら、ここから消すこともできる。
//!
//! サイドバー（Remotes の見出しとリモートの右クリック）と、リポジトリの設定の Remotes のページが同じシートを使う。
//! 消す前の確認（[`confirm_remove`]）も、どこから消しても同じ文言と同じボタン。

use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::sheets::{self, Response, Sheet, Style};

/// リモート（コアの `remotes` の答えの 1 つ）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

/// このリポジトリのリモート。
pub async fn remotes(repo: &Arc<Repository>) -> Vec<Remote> {
    sheets::run(repo, json!({ "op": "remotes" })).await["info"]["remotes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Remote {
                name: r["name"].as_str()?.to_owned(),
                fetch_url: r["fetchUrl"].as_str().unwrap_or_default().to_owned(),
                push_url: r["pushUrl"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

/// 新しいリモートを足すシート。
pub fn add(actions: &Rc<Actions>) {
    present(actions, None);
}

/// `name` のリモートを編集するシート。
pub fn edit(actions: &Rc<Actions>, name: &str) {
    let (actions, name) = (actions.clone(), name.to_owned());
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        if let Some(remote) = remotes(&repo).await.into_iter().find(|r| r.name == name) {
            present(&actions, Some(remote));
        }
    });
}

/// リモートを消す前に確かめる（取り消せないが、リモートのリポジトリには触らないので赤くしない。macOS 版と同じ）。
pub fn confirm_remove(actions: &Rc<Actions>, name: &str) {
    sheets::confirm_then(
        actions,
        tr!("Remove {name}?", name = name),
        tr!(
            "This removes the remote’s settings and its remote-tracking branches ({name}/…). The remote repository isn’t touched.",
            name = name
        ),
        tr!("Delete"),
        false,
        vec![json!({ "op": "changeRemote", "change": { "kind": "remove", "name": name } })],
    );
}

/// URL の先が何か: GitHub・GitLab などのサービス、パス、SSH 先、それ以外はホスト名。
pub fn kind(url: &str) -> String {
    let lower = url.to_lowercase();
    for (host, name) in [
        ("github.com", "GitHub"),
        ("gitlab", "GitLab"),
        ("bitbucket", "Bitbucket"),
        ("codeberg", "Codeberg"),
    ] {
        if lower.contains(host) {
            return name.to_owned();
        }
    }
    if ["/", "./", "../", "~", "file://"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return tr!("A path");
    }
    // `scheme://[user@]host[:port]/path` のホスト。
    let host_of = |rest: &str| -> String {
        let authority = rest.split('/').next().unwrap_or_default();
        let host = authority.rsplit('@').next().unwrap_or_default();
        host.split(':').next().unwrap_or_default().to_owned()
    };
    if let Some(rest) = lower
        .strip_prefix("ssh://")
        .or_else(|| lower.strip_prefix("git+ssh://"))
    {
        return tr!("SSH host {host}", host = host_of(rest));
    }
    if let Some((_, rest)) = url.split_once("://") {
        let host = host_of(rest);
        if !host.is_empty() {
            return host;
        }
    } else if let Some((left, _)) = url.split_once(':') {
        // scp の形（user@host:path）。
        let host = left.rsplit('@').next().unwrap_or(left);
        return tr!("SSH host {host}", host = host);
    }
    if url.is_empty() {
        String::new()
    } else {
        tr!("Other")
    }
}

/// 書く命令（`changeRemote` の列）。`remote` が None なら新しいリモート。
fn changes(remote: Option<&Remote>, name: &str, fetch: &str, push: &str) -> Vec<Value> {
    let mut out = Vec::new();
    match remote {
        Some(r) => {
            if r.name != name {
                out.push(json!({ "kind": "rename", "from": r.name, "to": name }));
            }
            if r.fetch_url != fetch {
                out.push(json!({ "kind": "setUrl", "name": name, "url": fetch }));
            }
            let had_push = if r.push_url != r.fetch_url {
                r.push_url.as_str()
            } else {
                ""
            };
            if had_push != push {
                let url = if push.is_empty() {
                    Value::Null
                } else {
                    json!(push)
                };
                out.push(json!({ "kind": "setPushUrl", "name": name, "url": url }));
            }
        }
        None => {
            out.push(json!({ "kind": "add", "name": name, "url": fetch }));
            if !push.is_empty() {
                out.push(json!({ "kind": "setPushUrl", "name": name, "url": push }));
            }
        }
    }
    out.into_iter()
        .map(|change| json!({ "op": "changeRemote", "change": change }))
        .collect()
}

/// リモートの追加（`remote` が None）・編集のシートを出す。保存したら `changeRemote` を順に実行する。
pub fn present(actions: &Rc<Actions>, remote: Option<Remote>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let title = if remote.is_some() {
            tr!("Edit Remote")
        } else {
            tr!("Add Remote")
        };
        let sheet = Sheet::new(sheets::parent(&actions).as_ref(), &title, "");
        let entry = |text: &str| {
            let e = gtk::Entry::new();
            e.set_text(text);
            e.set_hexpand(true);
            e
        };
        let name = entry(remote.as_ref().map_or("origin", |r| r.name.as_str()));
        let fetch = entry(remote.as_ref().map_or("", |r| r.fetch_url.as_str()));
        let push = entry(
            remote
                .as_ref()
                .filter(|r| r.push_url != r.fetch_url)
                .map_or("", |r| r.push_url.as_str()),
        );
        push.set_placeholder_text(Some(&tr!("Same as the fetch URL")));
        let points = gtk::Label::new(Some(&kind(&fetch.text())));
        points.set_xalign(0.0);
        points.add_css_class("dim-label");
        let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
        form.append(&sheets::row(&tr!("Name"), &name));
        form.append(&sheets::row(&tr!("Fetch URL"), &fetch));
        form.append(&sheets::row(&tr!("Push URL"), &push));
        form.append(&sheets::row(&tr!("Points to"), &points));
        sheet.add(&form);

        // 保存する前の接続の確かめ（裏で `git ls-remote`。結果はシートに出す）。
        let check = gtk::Button::with_label(&tr!("Check Connection"));
        let spinner = gtk::Spinner::new();
        let result = gtk::Label::new(None);
        result.set_xalign(0.0);
        result.set_wrap(true);
        result.set_max_width_chars(44);
        result.set_hexpand(true);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.append(&check);
        line.append(&spinner);
        line.append(&result);
        sheet.add(&line);

        if remote.is_some() {
            let remove = sheet.button(&tr!("Remove…"), Response::Other, Some(Style::Destructive));
            remove.set_hexpand(true);
            remove.set_halign(gtk::Align::Start);
        }
        sheet.button(&tr!("Cancel"), Response::Cancel, None);
        let save = sheet.button(&tr!("Save"), Response::Ok, Some(Style::Suggested));
        let checking = Rc::new(std::cell::Cell::new(false));
        let update = {
            let (name, fetch, save, check, checking) = (
                name.clone(),
                fetch.clone(),
                save.clone(),
                check.clone(),
                checking.clone(),
            );
            move || {
                let filled = !name.text().trim().is_empty() && !fetch.text().trim().is_empty();
                save.set_sensitive(filled && !checking.get());
                check.set_sensitive(!fetch.text().trim().is_empty() && !checking.get());
            }
        };
        update();
        let u = update.clone();
        name.connect_changed(move |_| u());
        let (u, p) = (update.clone(), points.clone());
        fetch.connect_changed(move |e| {
            p.set_text(&kind(e.text().trim()));
            u();
        });
        let (r, f, s, res, c, u) = (
            repo.clone(),
            fetch.clone(),
            spinner.clone(),
            result.clone(),
            checking.clone(),
            update.clone(),
        );
        check.connect_clicked(move |_| {
            let url = f.text().trim().to_owned();
            if url.is_empty() {
                return;
            }
            c.set(true);
            u();
            s.start();
            res.set_text(&tr!("Connecting…"));
            for css in ["success", "error"] {
                res.remove_css_class(css);
            }
            let (r, s, res, c, u) = (r.clone(), s.clone(), res.clone(), c.clone(), u.clone());
            glib::spawn_future_local(async move {
                let reply = sheets::run(&r, json!({ "op": "checkRemote", "url": url })).await;
                if reply["ok"].as_bool() == Some(true) {
                    let info = &reply["info"];
                    let mut text = tr!(
                        "Reachable: {branches} branches, {tags} tags",
                        branches = info["branches"].as_u64().unwrap_or(0),
                        tags = info["tags"].as_u64().unwrap_or(0)
                    );
                    if let Some(head) = info["head"].as_str() {
                        text.push_str(&tr!(", HEAD at {head}", head = head));
                    }
                    res.set_text(&text);
                    res.add_css_class("success");
                } else {
                    res.set_text(reply["error"].as_str().unwrap_or_default());
                    res.add_css_class("error");
                }
                s.stop();
                c.set(false);
                u();
            });
        });

        match sheet.run().await {
            Response::Ok => {
                let commands = changes(
                    remote.as_ref(),
                    name.text().trim(),
                    fetch.text().trim(),
                    push.text().trim(),
                );
                if !commands.is_empty() {
                    actions.perform_sequence(commands);
                }
            }
            Response::Other => {
                if let Some(remote) = &remote {
                    confirm_remove(&actions, &remote.name);
                }
            }
            Response::Cancel => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_what_a_url_points_to() {
        assert_eq!(kind("git@github.com:me/repo.git"), "GitHub");
        assert_eq!(kind("https://gitlab.example.com/me/repo"), "GitLab");
        assert_eq!(kind("https://git.example.com/me/repo"), "git.example.com");
        assert_eq!(kind(""), "");
    }

    #[test]
    fn writes_only_what_changed() {
        let origin = Remote {
            name: "origin".into(),
            fetch_url: "a".into(),
            push_url: "a".into(),
        };
        assert!(changes(Some(&origin), "origin", "a", "").is_empty());
        let c = changes(Some(&origin), "upstream", "a", "b");
        assert_eq!(c[0]["change"]["kind"], "rename");
        assert_eq!(c[1]["change"]["kind"], "setPushUrl");
        assert_eq!(c[1]["change"]["name"], "upstream");
        let c = changes(None, "backup", "x", "");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["change"]["kind"], "add");
    }
}
