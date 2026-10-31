//! リポジトリの形（partial clone・sparse checkout・maintenance の登録）と、それを変える操作。

use std::path::Path;

use amonhen_model::{Oid, PartialClone, RepositoryForm, Result, SparseCheckout, tr};

use crate::Git;

/// git の設定の真偽（`true`・`yes`・`on`・`1`）。
fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "on" | "1"
    )
}

/// `git config -z` の答え（`key\nvalue\0` の並び）を (キー, 値) にする。値のないキー（`[core] bare` のような
/// 書き方）は空の値にする。
fn parse_config_z(out: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(out)
        .split('\0')
        .filter(|e| !e.is_empty())
        .map(|e| match e.split_once('\n') {
            Some((k, v)) => (k.to_owned(), v.to_owned()),
            None => (e.to_owned(), String::new()),
        })
        .collect()
}

/// cone mode の `info/sparse-checkout` から、取り出しているフォルダを読む。git は取り出すフォルダ `/a/b/` に
/// 加えて、その上のフォルダを `/a/` と `!/a/*/`（直下のファイルだけ）で書くので、`!/x/*/` の続くものは上の
/// フォルダとして除く。`*`・`?`・`[`・`\` は `\` を前に付けて書かれている。
pub(crate) fn parse_cone(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let parents: std::collections::HashSet<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix('!')?.strip_suffix("*/"))
        .collect();
    let mut folders: Vec<String> = lines
        .iter()
        .filter(|l| !l.starts_with('!') && l.ends_with('/') && **l != "/*" && l.len() > 2)
        .filter(|l| !parents.contains(**l))
        .map(|l| unescape(&l[1..l.len() - 1]))
        .collect();
    folders.sort();
    folders.dedup();
    folders
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 同じ場所を指すパスか（git が書いた形同士なので、区切りと末尾の `/` をそろえて比べる。だめなら実体で）。
fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_owned();
    if norm(a) == norm(b) {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl Git {
    /// `args` の `git config` を読む。何も合わなければ（終了コード 1）空。
    fn config_matching(&self, args: &[&str]) -> Result<Vec<(String, String)>> {
        match self.read(args) {
            Ok(out) => Ok(parse_config_z(&out)),
            Err(e) if amonhen_model::process::missing_git().is_some() => Err(e),
            Err(_) => Ok(Vec::new()),
        }
    }

    pub(crate) fn repository_form(&self) -> Result<RepositoryForm> {
        let config = self.config_matching(&[
            "config",
            "-z",
            "--get-regexp",
            r"^(extensions\.partialclone|core\.partialclonefilter|core\.sparsecheckout|core\.sparsecheckoutcone|remote\..*\.partialclonefilter)$",
        ])?;
        let get = |key: &str| {
            config
                .iter()
                .rev()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.as_str())
        };
        // remote の名前は大文字小文字をそのまま残している（`remote.<name>.<key>` の key だけ小文字）。
        let remote_with = |key: &str| {
            config.iter().rev().find_map(|(k, v)| {
                let name = k
                    .strip_prefix("remote.")?
                    .strip_suffix(key)?
                    .strip_suffix('.')?;
                Some((name.to_owned(), v.clone()))
            })
        };
        let filter_of = |remote: &str| {
            get(&format!("remote.{remote}.partialclonefilter"))
                .or_else(|| get("core.partialclonefilter"))
                .filter(|f| !f.is_empty())
                .map(str::to_owned)
        };
        let partial = if let Some(remote) = get("extensions.partialclone").filter(|r| !r.is_empty())
        {
            Some(PartialClone {
                remote: remote.to_owned(),
                filter: filter_of(remote),
            })
        } else if let Some((remote, filter)) =
            remote_with("partialclonefilter").filter(|(_, f)| !f.is_empty())
        {
            Some(PartialClone {
                remote,
                filter: Some(filter),
            })
        } else {
            None
        };
        let sparse = if get("core.sparsecheckout").is_some_and(truthy) {
            let cone = get("core.sparsecheckoutcone").is_some_and(truthy);
            let file = self.read(&["rev-parse", "--git-path", "info/sparse-checkout"])?;
            let file = String::from_utf8_lossy(&file).trim().to_owned();
            let text =
                std::fs::read_to_string(self.workdir().join(Path::new(&file))).unwrap_or_default();
            let folders = if cone {
                parse_cone(&text)
            } else {
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_owned)
                    .collect()
            };
            Some(SparseCheckout { cone, folders })
        } else {
            None
        };
        // `git maintenance register` は、作業ツリー（なければ git dir）の実体のパスをユーザー全体の設定に足す。
        let registered =
            self.config_matching(&["config", "--global", "-z", "--get-all", "maintenance.repo"])?;
        let maintenance = !registered.is_empty() && {
            let here = self
                .read(&["rev-parse", "--show-toplevel"])
                .ok()
                .map(|o| String::from_utf8_lossy(&o).trim().to_owned())
                .filter(|p| !p.is_empty())
                .or_else(|| {
                    self.read(&["rev-parse", "--absolute-git-dir"])
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o).trim().to_owned())
                });
            // `--get-all` の答えは値だけ（キーがない）なので、`parse_config_z` ではキーの側に入る。
            here.is_some_and(|here| registered.iter().any(|(path, _)| same_path(path, &here)))
        };
        Ok(RepositoryForm {
            partial,
            sparse,
            maintenance,
        })
    }

    /// partial clone の手元にないオブジェクト `ids` を、`remote` から 1 回の fetch でまとめて取る（git の差分が足りない
    /// blob を取るときと同じ形。`cat-file` や `log -G` は 1 つずつ取りに行く）。FETCH_HEAD は書き換えない（git 2.29
    /// から。それより古い git は書き換え、ID を引数に並べるので 500 個ずつに分ける）。
    pub fn fetch_missing(&self, remote: &str, ids: &[String]) -> Result<()> {
        // 手元のコミットを「持っている」と伝えると、サーバーはそこから辿れる blob も持っているとみなして送らない
        // （GitHub）。git 自身が足りないものを取るときと同じく、何も伝えない。
        let mut args = vec![
            "-c",
            "fetch.negotiationAlgorithm=noop",
            "fetch",
            "--quiet",
            "--no-tags",
            "--recurse-submodules=no",
        ];
        if crate::git_at_least(2, 29) {
            args.extend([
                "--no-write-fetch-head",
                "--filter=blob:none",
                "--stdin",
                remote,
            ]);
            let input: String = ids.iter().map(|id| format!("{id}\n")).collect();
            return self.write_with_input(&args, input.as_bytes()).map(drop);
        }
        args.extend(["--filter=blob:none", remote]);
        for chunk in ids.chunks(500) {
            let mut args = args.clone();
            args.extend(chunk.iter().map(String::as_str));
            self.write(&args)?;
        }
        Ok(())
    }

    /// partial clone なら、`tips` から辿れる範囲（`path` があればその下）の手元にない中身をまとめて取る。内容で探す
    /// （`log -G`）前に呼ぶ。git は足りない blob を 1 つずつ取りに行くので、大きなリポジトリでは終わらない。
    pub(crate) fn fetch_missing_reachable(&self, tips: &[Oid], path: Option<&str>) -> Result<()> {
        let Some(partial) = self.repository_form()?.partial else {
            return Ok(());
        };
        // `--missing=print` は足りないものを取りに行かず、`?<ID>` の行で知らせる。
        let mut args = vec!["rev-list", "--objects", "--missing=print", "--stdin", "--"];
        args.extend(path);
        let revs: String = tips.iter().map(|t| format!("{t}\n")).collect();
        let out = self.read_with_input(&args, revs.as_bytes())?;
        let missing: Vec<String> = String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|line| line.strip_prefix('?'))
            .map(|id| id.trim().to_owned())
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        self.fetch_missing(&partial.remote, &missing)
    }

    pub(crate) fn fetch_all_contents(&self, remote: &str) -> Result<()> {
        let remote_key = format!("remote.{remote}.partialclonefilter");
        let read = |key: &str| {
            self.read(&["config", "--get", key])
                .ok()
                .map(|o| String::from_utf8_lossy(&o).trim().to_owned())
        };
        let old = [
            (remote_key.as_str(), read(&remote_key)),
            ("core.partialclonefilter", read("core.partialclonefilter")),
        ];
        // filter が残っていると、--refetch はその filter で取り直す。
        for (key, value) in &old {
            if value.is_some() {
                self.write(&["config", "--unset-all", key])?;
            }
        }
        if let Err(e) = self.write(&["fetch", "--refetch", remote]) {
            // 取れなかった（取り消しも）なら、partial clone のまま戻す。
            for (key, value) in &old {
                if let Some(value) = value {
                    let _ = self.write(&["config", key, value]);
                }
            }
            return Err(e);
        }
        // git 2.23 までの形（`extensions.partialClone`）は、2.24 からの形（`remote.<name>.promisor`）に移す。
        // 残せば、filter がなくても partial clone と読まれる。足りないものがまだあれば、git はこの remote から取る。
        if read("extensions.partialclone").as_deref() == Some(remote) {
            self.write(&["config", &format!("remote.{remote}.promisor"), "true"])?;
            self.write(&["config", "--unset-all", "extensions.partialclone"])?;
        }
        Ok(())
    }

    pub(crate) fn sparse_checkout(&self, folders: Option<&[String]>) -> Result<()> {
        let Some(folders) = folders else {
            return self.write(&["sparse-checkout", "disable"]).map(drop);
        };
        if folders.iter().any(|f| f.contains('\n')) {
            return Err(tr!("A folder name can’t contain a line break").into());
        }
        let mut input = folders.join("\n");
        input.push('\n');
        if crate::git_at_least(2, 35) {
            return self
                .write_with_input(
                    &["sparse-checkout", "set", "--cone", "--stdin"],
                    input.as_bytes(),
                )
                .map(drop);
        }
        // git 2.34 までは、set は cone mode を始められない。先に init --cone で始める。
        let form = self.repository_form()?;
        if !form.sparse.is_some_and(|s| s.cone) {
            self.write(&["sparse-checkout", "init", "--cone"])?;
        }
        self.write_with_input(&["sparse-checkout", "set", "--stdin"], input.as_bytes())
            .map(drop)
    }

    /// `git maintenance start` は、登録（ユーザー全体の `maintenance.repo`）に加えて、このリポジトリの
    /// `maintenance.auto=false`・`maintenance.strategy=incremental` を書き、OS の予定（macOS は launchd、Linux は
    /// systemd か cron、Windows はタスク スケジューラ）を作る。予定は登録したすべてのリポジトリを回るので、
    /// やめるときは登録を外す（`unregister`）。`unregister` は `maintenance.auto=false` を残し、そのままでは
    /// commit などの後の自動の手入れ（`git maintenance run --auto`）も止まったままになるので、外して git の既定に
    /// 戻す（`strategy` は予定がなければ使われないので残す）。
    pub(crate) fn maintenance(&self, on: bool) -> Result<()> {
        if on {
            return self.write(&["maintenance", "start"]).map(drop);
        }
        self.write(&["maintenance", "unregister"])?;
        let auto = self.read(&["config", "--local", "--get", "maintenance.auto"]);
        if auto.is_ok_and(|v| String::from_utf8_lossy(&v).trim() == "false") {
            self.write(&["config", "--local", "--unset", "maintenance.auto"])?;
        }
        Ok(())
    }

    pub(crate) fn tree_folders(&self, path: &str) -> Result<Vec<String>> {
        let out = self.read(&["ls-tree", "-z", &format!("HEAD:{path}")])?;
        let mut names: Vec<String> = String::from_utf8_lossy(&out)
            .split('\0')
            .filter_map(|line| {
                let (info, name) = line.split_once('\t')?;
                (info.split(' ').nth(1) == Some("tree")).then(|| name.to_owned())
            })
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_folders_of_a_cone() {
        let text = "/*\n!/*/\n/crates/\n!/crates/*/\n/crates/core/\n/docs/\n/a\\*b/\n";
        assert_eq!(parse_cone(text), ["a*b", "crates/core", "docs"]);
        assert!(parse_cone("/*\n!/*/\n").is_empty());
    }

    #[test]
    fn reads_config_with_nul() {
        assert_eq!(
            parse_config_z(b"core.sparsecheckout\ntrue\0remote.Up.promisor\ntrue\0"),
            [
                ("core.sparsecheckout".to_owned(), "true".to_owned()),
                ("remote.Up.promisor".to_owned(), "true".to_owned())
            ]
        );
        assert_eq!(
            parse_config_z(b"/a/b\0"),
            [("/a/b".to_owned(), String::new())]
        );
    }
}
