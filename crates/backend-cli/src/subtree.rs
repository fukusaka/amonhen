//! subtree（`git subtree`）: 一覧と、Add・Pull・Push・Split。`git subtree` は git の contrib で、入っていない git も
//! ある（ソースから作った git など）。
//!
//! 一覧は HEAD の履歴の `git-subtree-dir:` から読む。大きなリポジトリで毎回すべてのコミットを読まないよう、読んだ
//! HEAD と結果を git dir の `amonhen/subtrees` に覚え、次はその後のコミットだけを読む（HEAD が前の HEAD の子孫で
//! なければ読み直す）。

use std::collections::HashMap;

use amonhen_model::{Outcome, Result, Subtree, SubtreeOp, Subtrees, tr};

use crate::Git;

/// 覚えている相手の設定の名前（`amonhen.subtree.<dir>.<key>`。このリポジトリの設定）。
const CONFIG: &str = "amonhen.subtree.";

impl Git {
    /// この機械の git で `git subtree` が使えるか。ないと git は「not a git command」と言う。`-h` の終了コードは
    /// 版でまちまちなので、使い方の文を見る。
    fn subtree_available(&self) -> bool {
        self.command(&["subtree", "-h"]).output().is_ok_and(|o| {
            let text = [o.stdout, o.stderr].concat();
            String::from_utf8_lossy(&text).contains("git subtree add")
        })
    }

    /// HEAD の履歴の `git-subtree-dir:` のフォルダと、その最新の記録が squash か（新しい順）。
    fn subtree_dirs(&self) -> Result<Vec<(String, bool)>> {
        let Ok(head) = self.read(&["rev-parse", "--verify", "-q", "HEAD"]) else {
            return Ok(Vec::new());
        };
        let head = String::from_utf8_lossy(&head).trim().to_owned();
        let cache = self.git_dir()?.join("amonhen").join("subtrees");
        let (seen, mut dirs) = read_cache(&cache);
        if seen.as_deref() == Some(head.as_str()) {
            return Ok(dirs);
        }
        // 前に読んだ HEAD の子孫なら、その後のコミットだけ。
        let range = match &seen {
            Some(old)
                if self
                    .read(&["merge-base", "--is-ancestor", old, &head])
                    .is_ok() =>
            {
                format!("{old}..{head}")
            }
            _ => {
                dirs.clear();
                head.clone()
            }
        };
        let out = self.read(&[
            "log",
            "--grep=^git-subtree-dir:",
            "--format=%x1e%s%x1f%b",
            &range,
            "--",
        ])?;
        let mut found: Vec<(String, bool)> = Vec::new();
        for record in String::from_utf8_lossy(&out).split('\x1e') {
            let Some((subject, body)) = record.split_once('\x1f') else {
                continue;
            };
            let Some(dir) = body
                .lines()
                .find_map(|l| l.strip_prefix("git-subtree-dir:"))
                .map(|d| d.trim().trim_end_matches('/').to_owned())
                .filter(|d| !d.is_empty())
            else {
                continue;
            };
            if !found.iter().any(|(d, _)| *d == dir) {
                found.push((dir, subject.starts_with("Squashed '")));
            }
        }
        // 新しく読んだものが前のものより新しい。
        for (dir, squashed) in dirs {
            if !found.iter().any(|(d, _)| *d == dir) {
                found.push((dir, squashed));
            }
        }
        write_cache(&cache, &head, &found);
        Ok(found)
    }

    /// 覚えている相手（フォルダ → 名前 → 値。名前は git が小文字にする）。
    fn subtree_config(&self) -> HashMap<String, HashMap<String, String>> {
        let mut map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let Ok(out) = self.read(&[
            "config",
            "--local",
            "-z",
            "--get-regexp",
            r"^amonhen\.subtree\.",
        ]) else {
            return map;
        };
        for item in out.split(|&b| b == 0).filter(|i| !i.is_empty()) {
            let item = String::from_utf8_lossy(item);
            let (key, value) = item.split_once('\n').unwrap_or((&item, ""));
            let Some((dir, name)) = key.strip_prefix(CONFIG).and_then(|k| k.rsplit_once('.'))
            else {
                continue;
            };
            map.entry(dir.to_owned())
                .or_default()
                .insert(name.to_owned(), value.to_owned());
        }
        map
    }

    pub(crate) fn subtree_list(&self) -> Result<Subtrees> {
        if !self.subtree_available() {
            return Ok(Subtrees::default());
        }
        let config = self.subtree_config();
        let mut dirs = self.subtree_dirs()?;
        for dir in config.keys() {
            if !dirs.iter().any(|(d, _)| d == dir) {
                dirs.push((dir.clone(), false));
            }
        }
        // 消したフォルダは出さない（HEAD にフォルダとしてあるものだけ）。
        let mut present: Vec<String> = Vec::new();
        if !dirs.is_empty() {
            let mut args = vec!["ls-tree", "-d", "-z", "--name-only", "HEAD", "--"];
            args.extend(dirs.iter().map(|(d, _)| d.as_str()));
            if let Ok(out) = self.read(&args) {
                present = out
                    .split(|&b| b == 0)
                    .filter(|i| !i.is_empty())
                    .map(|i| String::from_utf8_lossy(i).into_owned())
                    .collect();
            }
        }
        let mut subtrees: Vec<Subtree> = dirs
            .into_iter()
            .filter(|(d, _)| present.contains(d))
            .map(|(dir, squashed)| {
                let keys = config.get(&dir);
                let get = |k: &str| keys.and_then(|m| m.get(k)).cloned();
                Subtree {
                    url: get("url"),
                    ref_: get("ref"),
                    push_ref: get("pushref"),
                    squash: get("squash").map(|v| v == "true"),
                    dir,
                    squashed,
                }
            })
            .collect();
        subtrees.sort_by(|a, b| a.dir.cmp(&b.dir));
        Ok(Subtrees {
            available: true,
            subtrees,
        })
    }

    pub(crate) fn run_subtree(&self, op: &SubtreeOp) -> Result<Outcome> {
        let (dir, url, rev) = match op {
            SubtreeOp::Add { dir, url, ref_, .. }
            | SubtreeOp::Pull { dir, url, ref_, .. }
            | SubtreeOp::Push { dir, url, ref_ } => (dir, Some(url), Some(ref_)),
            SubtreeOp::Split { dir, branch } => (dir, None, Some(branch)),
        };
        let dir = dir.trim().trim_end_matches('/');
        // オプションと取り違えないように。
        for value in [Some(dir), url.map(String::as_str), rev.map(String::as_str)]
            .into_iter()
            .flatten()
        {
            if value.trim().is_empty() || value.starts_with('-') {
                return Err(tr!("Not a valid value: {value}", value = value).into());
            }
        }
        if !self.subtree_available() {
            return Err(tr!("This machine’s git has no git subtree").into());
        }
        let prefix = format!("--prefix={dir}");
        let remember = |pairs: &[(&str, &str)]| {
            for (key, value) in pairs {
                let _ = self.write(&["config", "--local", &format!("{CONFIG}{dir}.{key}"), value]);
            }
        };
        match op {
            SubtreeOp::Add {
                url, ref_, squash, ..
            }
            | SubtreeOp::Pull {
                url, ref_, squash, ..
            } => {
                let verb = if matches!(op, SubtreeOp::Add { .. }) {
                    "add"
                } else {
                    "pull"
                };
                let mut args = vec!["subtree", verb, &prefix];
                if *squash {
                    args.push("--squash");
                }
                args.extend(["--", url.trim(), ref_.trim()]);
                let outcome = self.stoppable(&args, &[])?;
                remember(&[
                    ("url", url.trim()),
                    ("ref", ref_.trim()),
                    ("squash", if *squash { "true" } else { "false" }),
                ]);
                Ok(outcome)
            }
            SubtreeOp::Push { url, ref_, .. } => {
                self.write(&["subtree", "push", &prefix, "--", url.trim(), ref_.trim()])?;
                remember(&[("url", url.trim()), ("pushRef", ref_.trim())]);
                Ok(Outcome::Done)
            }
            SubtreeOp::Split { branch, .. } => {
                self.write(&["subtree", "split", &prefix, "-b", branch.trim()])?;
                Ok(Outcome::Done)
            }
        }
    }
}

/// 覚えた HEAD と、フォルダ・squash の列（1 行目が HEAD、その後は `<0|1>\t<dir>`）。
fn read_cache(path: &std::path::Path) -> (Option<String>, Vec<(String, bool)>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, Vec::new());
    };
    let mut lines = text.lines();
    let head = lines.next().map(str::to_owned).filter(|h| !h.is_empty());
    let dirs = lines
        .filter_map(|l| l.split_once('\t'))
        .map(|(s, d)| (d.to_owned(), s == "1"))
        .collect();
    (head, dirs)
}

fn write_cache(path: &std::path::Path, head: &str, dirs: &[(String, bool)]) {
    let mut text = format!("{head}\n");
    for (dir, squashed) in dirs {
        if dir.contains(['\n', '\t']) {
            continue;
        }
        text.push_str(&format!("{}\t{dir}\n", if *squashed { 1 } else { 0 }));
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 覚えるのは速くするためだけなので、書けなくても続ける。
    let _ = std::fs::write(path, text);
}
