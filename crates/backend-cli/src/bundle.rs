//! コミットを 1 つのファイルにまとめる（`git bundle create`）。

use amonhen_model::{Bundle, BundleInfo, BundlePreview, ContentsToFetch, Oid, OidSet, Result, tr};

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

impl Git {
    pub(crate) fn fetch_bundle_bytes(&self, bundle: &[u8], name: &str) -> Result<Vec<String>> {
        let prefix = format!("refs/remotes/{name}/");
        if name.is_empty()
            || name.contains('/')
            || self
                .read(&["check-ref-format", &format!("{prefix}x")])
                .is_err()
        {
            return Err(tr!("“{name}” can’t be used as a name", name = name).into());
        }
        let remotes = self.read(&["remote"])?;
        if String::from_utf8_lossy(&remotes).lines().any(|r| r == name) {
            return Err(tr!(
                "There is already a remote named “{name}”. Choose another name",
                name = name
            )
            .into());
        }
        // SSH 先のエンジンや共有の daemon は 1 つのプロセスで要求を同時に処理するので、呼ぶたびに別のファイルにする。
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = self.git_dir()?.join("amonhen");
        std::fs::create_dir_all(&dir)?;
        let file = dir.join(format!("fetch-bundle-{}-{n}.bundle", std::process::id()));
        std::fs::write(&file, bundle)?;
        let path = file.to_string_lossy().into_owned();
        let spec = format!("+refs/heads/*:{prefix}*");
        let fetched = self.write(&["fetch", "--no-tags", &path, &spec]);
        let _ = std::fs::remove_file(&file);
        fetched?;
        let refs = self.read(&["for-each-ref", "--format=%(refname)", &prefix])?;
        Ok(String::from_utf8_lossy(&refs)
            .lines()
            .map(str::to_owned)
            .collect())
    }

    pub(crate) fn missing_commit_ids(&self, ids: &[String]) -> Result<Vec<String>> {
        Ok(ids
            .iter()
            .filter(|id| {
                self.read(&["rev-parse", "-q", "--verify", &format!("{id}^{{commit}}")])
                    .is_err()
            })
            .cloned()
            .collect())
    }
}

/// `path` が bundle のファイルなら、その見出し（[`amonhen_model::parse_bundle_header`]）。bundle でなければ None。
pub fn bundle_info(path: &std::path::Path) -> Option<BundleInfo> {
    let file = std::fs::File::open(path).ok()?;
    amonhen_model::parse_bundle_header(std::io::BufReader::new(file))
}

/// HEAD のない bundle から clone するとき取り出すブランチ: main、master、なければ最初のブランチ、ブランチがなければ
/// 最初の ref（タグなら detached）。短い名前で返す（`git clone -b` に渡す）。
pub fn bundle_default_branch(info: &BundleInfo) -> Option<String> {
    let short = |r: &str| {
        r.strip_prefix("refs/heads/")
            .or_else(|| r.strip_prefix("refs/tags/"))
            .unwrap_or(r)
            .to_owned()
    };
    ["refs/heads/main", "refs/heads/master"]
        .iter()
        .find(|b| info.refs.iter().any(|r| r == *b))
        .map(|b| short(b))
        .or_else(|| {
            info.refs
                .iter()
                .find(|r| r.starts_with("refs/heads/"))
                .map(|r| short(r))
        })
        .or_else(|| info.refs.first().map(|r| short(r)))
}
