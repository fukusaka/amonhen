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

/// bundle のファイルの見出し（git を動かさずに読む。どの版の git でも同じ形）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BundleInfo {
    /// 入っている ref（`refs/heads/main` など。HEAD は除く）。
    pub refs: Vec<String>,
    /// HEAD が入っている（clone したとき、それを取り出す）。
    pub head: bool,
    /// 前提のコミットの数（受け取る側が持っているはずのもの。あれば clone できない）。
    pub prerequisites: usize,
}

/// `path` が bundle のファイルなら、その見出し。bundle でなければ None。
///
/// 形（`git bundle` の v2・v3）: `# v2 git bundle`（v3 なら続けて `@capability` の行）、前提のコミットの
/// `-<id> <comment>` の行、ref の `<id> <name>` の行、空の行、その後に pack。
pub fn bundle_info(path: &std::path::Path) -> Option<BundleInfo> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    let mut lines = std::io::BufReader::new(file).split(b'\n');
    let first = lines.next()?.ok()?;
    if first != b"# v2 git bundle" && first != b"# v3 git bundle" {
        return None;
    }
    let mut info = BundleInfo::default();
    for line in lines {
        let line = line.ok()?;
        let line = String::from_utf8_lossy(&line);
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            return Some(info);
        }
        if line.starts_with('@') {
            continue;
        }
        if line.starts_with('-') {
            info.prerequisites += 1;
        } else if let Some((_, name)) = line.split_once(' ') {
            if name == "HEAD" {
                info.head = true;
            } else {
                info.refs.push(name.to_owned());
            }
        }
    }
    // 見出しが終わらないうちにファイルが終わった。
    None
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
