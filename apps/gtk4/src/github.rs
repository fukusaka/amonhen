//! GitHub（macOS 版の `GitHub.swift`）: リモートの URL から GitHub のリポジトリを見分け、Repository > Open on GitHub
//! と Create Pull Request…（`gh` で作る）。CI の印・アバター・プルリクエストの一覧はまだない。

use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::actions::Actions;
use crate::sheets::{Response, Sheet, row};

/// GitHub のリポジトリ（ホスト、持ち主、名前）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubRepository {
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl GitHubRepository {
    /// `https://github.com/o/r.git`・`git@github.com:o/r.git`・`ssh://git@github.com/o/r` を読む。ホストの名前に
    /// "github" を含むものだけ（GitHub Enterprise を含む）。
    pub fn parse(url: &str) -> Option<Self> {
        let url = url.trim();
        let (host, rest) = if let Some((_, after)) = url.split_once("://") {
            let after = match after.split_once('@') {
                Some((user, rest)) if !user.contains('/') => rest,
                _ => after,
            };
            after.split_once('/')?
        } else {
            // scp 形式: user@host:owner/repo
            let (host, rest) = url.split_once(':')?;
            (host.rsplit('@').next().unwrap_or(host), rest)
        };
        let host = host.split(':').next().unwrap_or(host);
        if !host.to_lowercase().contains("github") {
            return None;
        }
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            return None;
        }
        Some(Self {
            host: host.to_owned(),
            owner: parts[parts.len() - 2].to_owned(),
            name: parts[parts.len() - 1].to_owned(),
        })
    }

    /// `gh` の `-R` に渡す形。
    pub fn slug(&self) -> String {
        if self.host == "github.com" {
            format!("{}/{}", self.owner, self.name)
        } else {
            format!("{}/{}/{}", self.host, self.owner, self.name)
        }
    }

    pub fn web_url(&self) -> String {
        format!("https://{}/{}/{}", self.host, self.owner, self.name)
    }
}

/// `gh` が使えるか。
pub fn available() -> bool {
    glib::find_program_in_path("gh").is_some()
}

/// リポジトリの GitHub（origin、なければ最初のリモート）。
pub async fn of(repo: &Arc<Repository>) -> Option<GitHubRepository> {
    let reply = crate::sheets::run(repo, json!({ "op": "remotes" })).await;
    let remotes = reply["info"]["remotes"].as_array()?.clone();
    let remote = remotes
        .iter()
        .find(|r| r["name"].as_str() == Some("origin"))
        .or(remotes.first())?;
    GitHubRepository::parse(remote["fetchUrl"].as_str()?)
}

fn open_uri(window: Option<&gtk::Window>, uri: &str) {
    gtk::UriLauncher::new(uri).launch(window, gio::Cancellable::NONE, |_| {});
}

/// Repository > Open on GitHub: 今のブランチのページを開く。
pub fn open_on_github(actions: &Rc<Actions>, branch: Option<String>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let window = crate::sheets::parent(&actions);
        let Some(repo) = actions.repo() else { return };
        let Some(github) = of(&repo).await else {
            if let Some(w) = &window {
                w.error_bell();
            }
            return;
        };
        let mut url = github.web_url();
        if let Some(b) = branch {
            url.push_str(&format!("/tree/{b}"));
        }
        open_uri(window.as_ref(), &url);
    });
}

/// `gh` を `dir` で動かし、標準出力を返す。失敗したら標準エラー。
fn gh(args: &[&str], dir: Option<&std::path::Path>) -> Result<String, String> {
    let mut command = amonhen_model::process::command("gh");
    command.args(args);
    if let Some(d) = dir {
        command.current_dir(d);
    }
    let out = command.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// Repository > Create Pull Request…: `branch` からプルリクエストを作る。まだ push していない（上流がない・先行
/// コミットがある）なら、まず Push のシートを出し、送り終えたらそのまま作る。
pub fn create_pull_request(actions: &Rc<Actions>, branch: String, pushed: bool) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let window = crate::sheets::parent(&actions);
        let Some(repo) = actions.repo() else { return };
        // gh で作るので、gh がなければ作れない。
        if !available() {
            if let Some(w) = &window {
                w.error_bell();
            }
            return;
        }
        let (Some(github), Some(dir)) = (of(&repo).await, repo.paths().workdir) else {
            if let Some(w) = &window {
                w.error_bell();
            }
            return;
        };
        let r = repo.clone();
        let refs = gio::spawn_blocking(move || r.load_refs().ok())
            .await
            .ok()
            .flatten();
        let item = refs.as_ref().and_then(|r| {
            r.refs
                .entries
                .iter()
                .find(|e| e.kind == amonhen_model::RefKind::Local && e.name == branch)
                .cloned()
        });
        if !pushed
            && item
                .as_ref()
                .is_none_or(|i| i.detail.is_none() || i.gone || i.ahead > 0)
        {
            let (a, b) = (actions.clone(), branch.clone());
            crate::sheets::push(
                &actions,
                Some(branch.clone()),
                false,
                Some(Box::new(move || create_pull_request(&a, b, true))),
            );
            return;
        }
        // タイトルと本文は、ブランチの先頭のコミットのメッセージから。
        let tip = item
            .and_then(|i| i.target)
            .map_or_else(|| "HEAD".to_owned(), |t| t.to_string());
        let message = crate::sheets::run(&repo, json!({ "op": "commitMessage", "oid": tip })).await
            ["text"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let (title, body) = message.split_once('\n').unwrap_or((&message, ""));
        let (title, body) = (
            if title.is_empty() {
                branch.clone()
            } else {
                title.to_owned()
            },
            body.trim().to_owned(),
        );
        let slug = github.slug();
        let s = slug.clone();
        let base = gio::spawn_blocking(move || {
            gh(
                &[
                    "repo",
                    "view",
                    &s,
                    "--json",
                    "defaultBranchRef",
                    "-q",
                    ".defaultBranchRef.name",
                ],
                None,
            )
            .ok()
        })
        .await
        .ok()
        .flatten()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "main".to_owned());

        let sheet = Sheet::new(
            window.as_ref(),
            "Create Pull Request",
            &tr!("From {branch}", branch = branch),
        );
        let title_field = gtk::Entry::builder().text(&title).hexpand(true).build();
        let base_field = gtk::Entry::builder().text(&base).hexpand(true).build();
        let body_view = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(4)
            .right_margin(4)
            .build();
        body_view.buffer().set_text(&body);
        let body_scrolled = gtk::ScrolledWindow::builder()
            .child(&body_view)
            .min_content_height(160)
            .min_content_width(460)
            .has_frame(true)
            .build();
        let draft = gtk::CheckButton::with_label(&tr!("Create as Draft"));
        sheet.add(&row(&tr!("Title:"), &title_field));
        sheet.add(&row("base:", &base_field));
        sheet.add(&row(&tr!("Body:"), &body_scrolled));
        sheet.add(&row("", &draft));
        sheet.buttons(&tr!("Create"), false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let title = title_field.text().trim().to_owned();
        let base = base_field.text().trim().to_owned();
        if title.is_empty() || base.is_empty() {
            return;
        }
        let b = body_view.buffer();
        let body = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let draft = draft.is_active();
        let head = branch.clone();
        let result = gio::spawn_blocking(move || {
            let mut args = vec![
                "pr", "create", "-R", &slug, "--head", &head, "--title", &title, "--body", &body,
                "--base", &base,
            ];
            if draft {
                args.push("--draft");
            }
            gh(&args, Some(&dir))
        })
        .await
        .unwrap_or_else(|_| Err(String::new()));
        match result {
            Ok(url) => {
                let sheet = Sheet::new(window.as_ref(), &tr!("Pull Request Created"), &url);
                sheet.buttons(&tr!("Open in Browser"), false, None);
                if sheet.run().await == Response::Ok {
                    open_uri(window.as_ref(), &url);
                }
            }
            Err(e) => {
                crate::sheets::inform(
                    window.as_ref(),
                    &tr!("Couldn’t create the pull request"),
                    &e,
                )
                .await
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_remotes() {
        let expected = Some(GitHubRepository {
            host: "github.com".into(),
            owner: "o".into(),
            name: "r".into(),
        });
        assert_eq!(
            GitHubRepository::parse("https://github.com/o/r.git"),
            expected
        );
        assert_eq!(GitHubRepository::parse("git@github.com:o/r.git"), expected);
        assert_eq!(
            GitHubRepository::parse("ssh://git@github.com/o/r"),
            expected
        );
        assert_eq!(GitHubRepository::parse("https://gitlab.com/o/r.git"), None);
        assert_eq!(
            GitHubRepository::parse("https://github.example.com/o/r").map(|g| g.slug()),
            Some("github.example.com/o/r".into())
        );
    }
}
