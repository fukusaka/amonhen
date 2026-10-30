//! リポジトリの形（shallow clone・partial clone・sparse checkout・maintenance の登録）を読み、partial clone の
//! 中身と sparse checkout のフォルダを変える（`Command::RepositoryForm` など）。

use amonhen_model::{Operations, Result, tr};

use crate::Repository;

/// partial clone の中身をすべて取る `git fetch --refetch` が使える git の版。
const REFETCH: (u32, u32) = (2, 36);
/// `git sparse-checkout`（cone mode）が使える git の版。
const SPARSE_CHECKOUT: (u32, u32) = (2, 25);

/// git が `needed` より古ければ、そう言うエラー（版が読めなければ使えるとみなす）。
fn require(ops: &dyn Operations, needed: (u32, u32)) -> Result<()> {
    match ops.git_version()? {
        Some((a, b, c)) if (a, b) < needed => Err(tr!(
            "Needs git {needed} or later. This repository’s git is {version}.",
            needed = format!("{}.{}", needed.0, needed.1),
            version = format!("{a}.{b}.{c}"),
        )
        .into()),
        _ => Ok(()),
    }
}

impl Repository {
    /// `Command::RepositoryForm` の答え。
    pub(crate) fn repository_form(&self, ops: &dyn Operations) -> Result<serde_json::Value> {
        let form = ops.repository_form()?;
        let shallow = match self.history() {
            Some(history) => !history.shallow.is_empty(),
            None => !self.backend().shallow()?.is_empty(),
        };
        let version = ops.git_version()?;
        let at_least = |needed: (u32, u32)| version.is_none_or(|(a, b, _)| (a, b) >= needed);
        Ok(serde_json::json!({
            "shallow": shallow,
            "partial": form.partial.map(|p| serde_json::json!({ "remote": p.remote, "filter": p.filter })),
            "sparse": form.sparse.map(|s| serde_json::json!({ "cone": s.cone, "folders": s.folders })),
            "maintenance": form.maintenance,
            "workingTree": self.paths().workdir.is_some(),
            "gitVersion": version.map(|(a, b, c)| format!("{a}.{b}.{c}")),
            "canFetchAllContents": at_least(REFETCH),
            "canSparseCheckout": at_least(SPARSE_CHECKOUT),
        }))
    }

    /// `Command::FetchAllContents`。
    pub(crate) fn fetch_all_contents(&self, ops: &dyn Operations) -> Result<()> {
        let Some(partial) = ops.repository_form()?.partial else {
            return Err(
                tr!("This repository has all of its contents (not a partial clone)").into(),
            );
        };
        require(ops, REFETCH)?;
        ops.fetch_all_contents(&partial.remote)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // テストの git はそのまま起動する
    use super::*;
    use amonhen_backend_gix::GixBackend;
    use std::path::Path;
    use std::process::Command as Process;

    /// `global` なら、テストのホームのユーザー全体の設定を使う（maintenance の登録を書く）。
    fn git_with(dir: &Path, args: &[&str], global: bool) -> String {
        amonhen_model::testing::isolate_from_user_settings();
        let mut cmd = Process::new("git");
        cmd.args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1");
        if !global {
            cmd.env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            );
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        if args.first() == Some(&"init") {
            for (key, value) in [
                ("user.name", "a"),
                ("user.email", "a@example.com"),
                ("core.autocrlf", "false"),
                ("commit.gpgsign", "false"),
            ] {
                git(dir, &["config", key, value]);
            }
        }
        out
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        git_with(dir, args, false)
    }

    fn run(repo: &Repository, json: &str) -> serde_json::Value {
        serde_json::from_str(&repo.run_json(json)).unwrap()
    }

    fn version() -> Option<(u32, u32, u32)> {
        amonhen_backend_cli::git_version()
    }

    /// `src` の file:// の URL（Windows は file:///C:/…）。
    fn file_url(src: &Path) -> String {
        let path = src.to_str().unwrap().replace('\\', "/");
        if path.starts_with('/') {
            format!("file://{path}")
        } else {
            format!("file:///{path}")
        }
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    /// partial clone の filter を読み、取らずにおいた中身をすべて取る。git 2.36 より古ければ、版を言って断る。
    #[test]
    fn fetches_all_contents_of_a_partial_clone() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        git(&src, &["init", "-q"]);
        // 取り出したときに中身が要らないよう、古いコミットにだけあるファイルを作る。
        write(&src, "old.txt", "old");
        git(&src, &["add", "."]);
        git(&src, &["commit", "-q", "-m", "old"]);
        git(&src, &["rm", "-q", "old.txt"]);
        write(&src, "new.txt", "new");
        git(&src, &["add", "."]);
        git(&src, &["commit", "-q", "-m", "new"]);
        git(&src, &["config", "uploadpack.allowFilter", "true"]);
        git(&src, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
        // git 2.24 からの形（remote.<name>.promisor）と、2.23 までの形（extensions.partialClone）。
        for old_form in [false, true] {
            let name = if old_form { "old" } else { "partial" };
            git(
                tmp.path(),
                &["clone", "-q", "--filter=blob:none", &file_url(&src), name],
            );
            let dir = tmp.path().join(name);
            let config = |key: &str| {
                let out = Process::new("git")
                    .args(["config", "--get", key])
                    .current_dir(&dir)
                    .output()
                    .unwrap();
                String::from_utf8_lossy(&out.stdout).trim().to_owned()
            };
            if old_form && config("extensions.partialclone").is_empty() {
                git(&dir, &["config", "--unset", "remote.origin.promisor"]);
                git(
                    &dir,
                    &["config", "--unset", "remote.origin.partialclonefilter"],
                );
                git(&dir, &["config", "extensions.partialclone", "origin"]);
                git(&dir, &["config", "core.partialclonefilter", "blob:none"]);
            }
            let missing = || {
                git(&dir, &["rev-list", "--objects", "--missing=print", "--all"])
                    .lines()
                    .filter(|l| l.starts_with('?'))
                    .count()
            };
            assert_eq!(missing(), 1);
            let repo = Repository::new(GixBackend::open(&dir).unwrap());
            let form = run(&repo, r#"{"op":"repositoryForm"}"#);
            assert_eq!(form["info"]["partial"]["remote"], "origin", "{form}");
            assert_eq!(form["info"]["partial"]["filter"], "blob:none", "{form}");

            if version().is_some_and(|(a, b, _)| (a, b) < (2, 36)) {
                assert_eq!(form["info"]["canFetchAllContents"], false);
                let r = run(&repo, r#"{"op":"fetchAllContents"}"#);
                assert!(r["error"].as_str().unwrap().contains("2.36"), "{r}");
                assert!(!run(&repo, r#"{"op":"repositoryForm"}"#)["info"]["partial"].is_null());
                continue;
            }
            assert_eq!(form["info"]["canFetchAllContents"], true);
            let r = run(&repo, r#"{"op":"fetchAllContents"}"#);
            assert_eq!(r["ok"], true, "{r}");
            assert_eq!(missing(), 0, "{name}");
            assert!(run(&repo, r#"{"op":"repositoryForm"}"#)["info"]["partial"].is_null());
            assert_eq!(config("remote.origin.promisor"), "true");
            assert_eq!(run(&repo, r#"{"op":"fetchAllContents"}"#)["ok"], false);
        }
    }

    /// `git maintenance register` と同じく、ユーザー全体の `maintenance.repo` に作業ツリーの実体のパスを足すと、
    /// 登録してあると読む。
    #[test]
    fn knows_a_repository_registered_for_maintenance() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q"]);
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        assert_eq!(
            run(&repo, r#"{"op":"repositoryForm"}"#)["info"]["maintenance"],
            false
        );
        let top = git(dir, &["rev-parse", "--show-toplevel"]);
        git_with(
            dir,
            &["config", "--global", "--add", "maintenance.repo", &top],
            true,
        );
        let form = run(&repo, r#"{"op":"repositoryForm"}"#);
        git_with(
            dir,
            &[
                "config",
                "--global",
                "--unset-all",
                "maintenance.repo",
                &regex_escape(&top),
            ],
            true,
        );
        assert_eq!(form["info"]["maintenance"], true, "{form}");
    }

    fn regex_escape(s: &str) -> String {
        s.chars()
            .flat_map(|c| {
                let special = ".^$*+?()[]{}|\\".contains(c);
                special.then_some('\\').into_iter().chain([c])
            })
            .collect()
    }
}
