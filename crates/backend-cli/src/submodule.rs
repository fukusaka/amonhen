//! submodule の様子と、それを変える操作（`git submodule`）。

use std::collections::HashMap;

use amonhen_model::{Result, SubmoduleChange, SubmoduleState, SubmoduleUpdate, tr};

use crate::Git;

/// `git submodule set-url` が使える git の版。それより前は `.gitmodules` を書いて `sync` する。
const SET_URL: (u32, u32) = (2, 25);

impl Git {
    pub(crate) fn run_submodule_update(
        &self,
        paths: &[&str],
        options: &SubmoduleUpdate,
    ) -> Result<()> {
        let mut args = vec!["submodule", "update", "--init"];
        if options.recursive {
            args.push("--recursive");
        }
        if options.remote {
            args.push("--remote");
        }
        args.push("--");
        args.extend(paths);
        self.write(&args).map(drop)
    }

    /// `.gitmodules` の `submodule.<name>.<key>` を、名前ごとに（`path`・`url`・`branch`）。
    fn gitmodules(&self) -> Result<Vec<(String, HashMap<String, String>)>> {
        if !self.workdir.join(".gitmodules").is_file() {
            return Ok(Vec::new());
        }
        let out = self.read(&["config", "-f", ".gitmodules", "-z", "--list"])?;
        let mut names: Vec<(String, HashMap<String, String>)> = Vec::new();
        for item in out.split(|&b| b == 0).filter(|i| !i.is_empty()) {
            let item = String::from_utf8_lossy(item);
            let (key, value) = item.split_once('\n').unwrap_or((&item, ""));
            // `submodule.<name>.<key>`（名前は `.` や `/` を含んでもよい。最後の `.` で分ける）。
            let Some(rest) = key.strip_prefix("submodule.") else {
                continue;
            };
            let Some((name, key)) = rest.rsplit_once('.') else {
                continue;
            };
            let at = match names.iter().position(|(n, _)| n == name) {
                Some(at) => at,
                None => {
                    names.push((name.to_owned(), HashMap::new()));
                    names.len() - 1
                }
            };
            names[at]
                .1
                .insert(key.to_ascii_lowercase(), value.to_owned());
        }
        Ok(names)
    }

    pub(crate) fn submodule_states(&self) -> Result<Vec<SubmoduleState>> {
        let modules = self.gitmodules()?;
        let mut states: Vec<SubmoduleState> = modules
            .iter()
            .filter_map(|(name, keys)| {
                Some(SubmoduleState {
                    path: keys.get("path")?.clone(),
                    name: name.clone(),
                    url: keys.get("url").cloned(),
                    branch: keys.get("branch").cloned(),
                    ..Default::default()
                })
            })
            .collect();
        if states.is_empty() {
            return Ok(states);
        }
        // `git submodule status` は使わない: index のロックを取ることがあり、裏で読むと同時に動く `submodule add`
        // などの `git add` とぶつかる。取り出してあるか（status の `-` でない）は、場所に `.git` があるかで見る。
        for s in &mut states {
            let git = self.workdir.join(&s.path).join(".git");
            s.initialized = git.exists();
            s.own_git_dir = git.is_dir();
        }
        // ロックを取らない status の行から: `S<C><M><U>`（C は記録と違うコミット、M は追跡しているファイルの変更、
        // U は untracked のファイル）と、コンフリクトの `u` の行。
        let mut args = vec![
            "status",
            "--porcelain=v2",
            "-z",
            "--ignore-submodules=none",
            "--",
        ];
        args.extend(states.iter().map(|s| s.path.as_str()));
        let out = self.read(&args)?;
        for entry in out.split(|&b| b == 0) {
            let entry = String::from_utf8_lossy(entry);
            if let Some(rest) = entry.strip_prefix("u ") {
                let path = rest.splitn(10, ' ').nth(9);
                if let Some(s) = states.iter_mut().find(|s| Some(s.path.as_str()) == path) {
                    s.conflict = true;
                }
                continue;
            }
            let fields: Vec<&str> = entry.splitn(9, ' ').collect();
            if fields.first() != Some(&"1") || fields.len() < 9 {
                continue;
            }
            let sub = fields[2].as_bytes();
            if sub.first() != Some(&b'S') || sub.len() != 4 {
                continue;
            }
            let path = fields[8];
            if let Some(s) = states.iter_mut().find(|s| s.path == path) {
                s.moved = sub[1] == b'C';
                s.changed = sub[2] == b'M' || sub[3] == b'U';
            }
        }
        states.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(states)
    }

    pub(crate) fn run_submodule_change(&self, change: &SubmoduleChange) -> Result<()> {
        match change {
            SubmoduleChange::Add { url, path, branch } => {
                let mut args = vec!["submodule", "add"];
                if let Some(branch) = branch.as_deref().filter(|b| !b.is_empty()) {
                    args.extend(["-b", branch]);
                }
                args.extend(["--", url, path]);
                self.write(&args).map(drop)
            }
            SubmoduleChange::Remove {
                path,
                delete_git_dir,
            } => {
                let name = self.submodule_name(path)?;
                // `git rm` は `.gitmodules` に stage していない変更があると断る。deinit の前に確かめ、途中で止めない。
                if !self
                    .read(&["diff", "--name-only", "--", ".gitmodules"])?
                    .is_empty()
                {
                    return Err(tr!("Stage or discard the changes to .gitmodules first").into());
                }
                self.write(&["submodule", "deinit", "-f", "--", path])?;
                self.write(&["rm", "-f", "--", path])?;
                if *delete_git_dir {
                    let dir =
                        self.read(&["rev-parse", "--git-path", &format!("modules/{name}")])?;
                    let dir = self.workdir.join(String::from_utf8_lossy(&dir).trim());
                    if dir.is_dir() {
                        std::fs::remove_dir_all(&dir)?;
                    }
                }
                Ok(())
            }
            SubmoduleChange::SetUrl { path, url } => {
                if crate::git_at_least(SET_URL.0, SET_URL.1) {
                    return self
                        .write(&["submodule", "set-url", "--", path, url])
                        .map(drop);
                }
                let name = self.submodule_name(path)?;
                self.write(&[
                    "config",
                    "-f",
                    ".gitmodules",
                    &format!("submodule.{name}.url"),
                    url,
                ])?;
                self.write(&["submodule", "sync", "--", path]).map(drop)
            }
            SubmoduleChange::SetBranch { path, branch } => match branch
                .as_deref()
                .filter(|b| !b.is_empty())
            {
                Some(branch) => self.write(&["submodule", "set-branch", "-b", branch, "--", path]),
                None => self.write(&["submodule", "set-branch", "--default", "--", path]),
            }
            .map(drop),
            SubmoduleChange::Absorb { paths } => {
                let mut args = vec!["submodule", "absorbgitdirs", "--"];
                args.extend(paths.iter().map(String::as_str));
                self.write(&args).map(drop)
            }
            SubmoduleChange::Foreach { command, recursive } => {
                if command.trim().is_empty() {
                    return Err(tr!("Give the command to run").into());
                }
                let mut args = vec!["submodule", "foreach"];
                if *recursive {
                    args.push("--recursive");
                }
                args.push(command);
                self.write(&args).map(drop)
            }
            SubmoduleChange::Sync { recursive } => {
                let mut args = vec!["submodule", "sync"];
                if *recursive {
                    args.push("--recursive");
                }
                self.write(&args).map(drop)
            }
        }
    }

    /// `path` の submodule の `.gitmodules` での名前。
    fn submodule_name(&self, path: &str) -> Result<String> {
        self.gitmodules()?
            .into_iter()
            .find(|(_, keys)| keys.get("path").map(String::as_str) == Some(path))
            .map(|(name, _)| name)
            .ok_or_else(|| tr!("{path} isn’t a submodule", path = path).into())
    }
}
