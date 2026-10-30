//! リポジトリの形（shallow clone・partial clone・sparse checkout・maintenance の登録）を読み、partial clone の
//! 中身と sparse checkout のフォルダを変える（`Command::RepositoryForm` など）。

use amonhen_model::{Operations, Result};

use crate::Repository;

/// partial clone の中身をすべて取る `git fetch --refetch` が使える git の版。
const REFETCH: (u32, u32) = (2, 36);
/// `git sparse-checkout`（cone mode）が使える git の版。
const SPARSE_CHECKOUT: (u32, u32) = (2, 25);

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
