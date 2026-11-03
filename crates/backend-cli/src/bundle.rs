//! コミットを 1 つのファイルにまとめる（`git bundle create`）。

use amonhen_model::{Bundle, BundlePreview, ContentsToFetch, Oid, OidSet, Result, tr};

use crate::Git;

impl Git {
    /// `git rev-list` に渡す、bundle に入れるコミットの指定。
    fn bundle_revs(options: &Bundle) -> Vec<String> {
        let mut revs: Vec<String> = if options.refs.is_empty() {
            ["HEAD", "--branches", "--tags"].map(String::from).to_vec()
        } else {
            options.refs.clone()
        };
        revs.extend(options.bases.iter().map(|b| format!("^{b}")));
        revs
    }

    pub(crate) fn bundle_bytes(&self, options: &Bundle) -> Result<Vec<u8>> {
        let preview = self.preview_bundle(options, usize::MAX)?;
        if preview.commits == 0 {
            return Err(tr!("There are no commits to put in the bundle").into());
        }
        if preview.beyond_shallow {
            return Err(tr!(
                "This shallow clone doesn’t have all the history the bundle needs. Fetch more history, or choose a newer base"
            )
            .into());
        }
        // partial clone で手元にない中身は、git が 1 つずつ取りに行く。先に 1 回の fetch でまとめて取る。
        if !preview.contents.ids.is_empty()
            && let Some(partial) = self.repository_form()?.partial
        {
            let ids: Vec<String> = preview.contents.ids.iter().map(Oid::to_string).collect();
            self.fetch_missing(&partial.remote, &ids)?;
        }
        let revs = Self::bundle_revs(options);
        let mut args = vec!["bundle", "create", "-"];
        args.extend(revs.iter().map(String::as_str));
        self.read(&args)
    }

    pub(crate) fn preview_bundle(&self, options: &Bundle, limit: usize) -> Result<BundlePreview> {
        let revs = Self::bundle_revs(options);
        let mut args = vec!["rev-list"];
        args.extend(revs.iter().map(String::as_str));
        let listed = self.read(&args)?;
        let commits: Vec<Oid> = String::from_utf8_lossy(&listed)
            .lines()
            .filter_map(|l| Oid::from_hex(l.trim()))
            .collect();
        let mut preview = BundlePreview {
            commits: commits.len(),
            ..Default::default()
        };
        // shallow clone の境のコミットが入るなら、その親は手元にない（git は作れてしまうが、clone できない）。
        // `--git-path` は作業ツリーからの相対パス（worktree でも、shallow は共通の git dir にある）。
        let shallow = self.read(&["rev-parse", "--git-path", "shallow"])?;
        let shallow = self.workdir.join(String::from_utf8_lossy(&shallow).trim());
        if let Ok(text) = std::fs::read_to_string(shallow) {
            let boundary: OidSet = text
                .lines()
                .filter_map(|l| Oid::from_hex(l.trim()))
                .collect();
            for c in commits.iter().filter(|c| boundary.contains(c)) {
                // 根まで取り足すと、git は根のコミットも境に残す。親のないコミットは欠けていない。shallow の境の親は
                // git から見えなくなるので、コミットの中身の parent の行で見る。
                let body = self.read(&["cat-file", "commit", &c.to_string()])?;
                let header = String::from_utf8_lossy(&body);
                let header = header.split("\n\n").next().unwrap_or_default();
                if header.lines().any(|l| l.starts_with("parent ")) {
                    preview.beyond_shallow = true;
                    break;
                }
            }
        }
        if self.repository_form()?.partial.is_some() && !commits.is_empty() {
            preview.contents = self.missing_objects(&revs, limit)?;
        }
        Ok(preview)
    }

    /// `revs` から辿れるオブジェクトのうち、手元にないもの（取りには行かない）。
    fn missing_objects(&self, revs: &[String], limit: usize) -> Result<ContentsToFetch> {
        let mut args = vec!["rev-list", "--objects", "--missing=print"];
        args.extend(revs.iter().map(String::as_str));
        let listed = self.read(&args)?;
        let mut found = ContentsToFetch::default();
        for id in String::from_utf8_lossy(&listed)
            .lines()
            .filter_map(|l| l.strip_prefix('?'))
            .filter_map(|id| Oid::from_hex(id.trim()))
        {
            if found.ids.len() == limit {
                found.more = true;
                break;
            }
            found.ids.push(id);
        }
        Ok(found)
    }
}
