//! ある時点のファイルを zip・tar・tar.gz にまとめる（`git archive`）。

use amonhen_model::{Archive, ContentsToFetch, Oid, OidSet, Result, tr};

use crate::Git;

/// 書き出せる形（`git archive --format`。git 2.23 からどれもある。tar.gz は git 2.38 より前は外の gzip を使う）。
const FORMATS: &[&str] = &["zip", "tar", "tar.gz"];

impl Git {
    pub(crate) fn archive_bytes(&self, options: &Archive) -> Result<Vec<u8>> {
        if !FORMATS.contains(&options.format.as_str()) {
            return Err(tr!("Unknown archive format: {format}", format = options.format).into());
        }
        let mut narrowed = false;
        // partial clone で手元にない中身は、git archive が取りに行く。先に 1 回の fetch でまとめて取る。
        if let Some(partial) = self.repository_form()?.partial {
            let missing = self.missing_in_tree(&options.rev, &options.paths, usize::MAX)?;
            if !missing.ids.is_empty() {
                let ids: Vec<String> = missing.ids.iter().map(Oid::to_string).collect();
                self.fetch_missing(&partial.remote, &ids)?;
            }
            narrowed = narrows_fetch(&options.paths);
        }
        let attr_source = format!("--attr-source={}", options.rev);
        let format = format!("--format={}", options.format);
        let prefix = format!("--prefix={}", options.prefix);
        let mut args = Vec::new();
        if narrowed {
            // git archive は、.gitattributes を読むためにコミットの tree をまるごと index に読み込み、そのとき手元に
            // ない中身をすべて取る（git 2.33 からはまとめて、それより前は 1 つずつ）。選んだフォルダより多く取らない
            // よう index を作らせず（`--worktree-attributes`）、.gitattributes は同じコミットの tree から読ませる
            // （`--attr-source`）。書き出す中身は git archive の既定と同じ。
            args.push(attr_source.as_str());
        }
        args.extend(["archive", format.as_str()]);
        if narrowed {
            args.push("--worktree-attributes");
        }
        if !options.prefix.is_empty() {
            args.push(&prefix);
        }
        args.push(&options.rev);
        if !options.paths.is_empty() {
            args.push("--");
            args.extend(options.paths.iter().map(String::as_str));
        }
        self.read(&args)
    }

    pub(crate) fn archive_missing(
        &self,
        rev: &str,
        paths: &[String],
        limit: usize,
    ) -> Result<ContentsToFetch> {
        if self.repository_form()?.partial.is_none() {
            return Ok(ContentsToFetch::default());
        }
        self.missing_in_tree(rev, paths, limit)
    }

    /// `rev` の tree の blob のうち、git archive が読むもので手元にないもの。取りには行かない
    /// （`rev-list --missing=print` は足りないものを `?` を付けて出すだけ。`--no-walk` でそのコミットの tree だけ）。
    /// `paths` で絞れるのは [`narrows_fetch`] のときだけで、その下と、上のフォルダの `.gitattributes`。それ以外は
    /// git archive が tree をまるごと読むので、すべてを数える。
    fn missing_in_tree(
        &self,
        rev: &str,
        paths: &[String],
        limit: usize,
    ) -> Result<ContentsToFetch> {
        let commit = format!("{rev}^{{commit}}");
        let listed = self.read(&[
            "rev-list",
            "--objects",
            "--missing=print",
            "--no-walk",
            &commit,
        ])?;
        let missing: OidSet = String::from_utf8_lossy(&listed)
            .lines()
            .filter_map(|line| line.strip_prefix('?'))
            .filter_map(|id| Oid::from_hex(id.trim()))
            .collect();
        let mut found = ContentsToFetch::default();
        if missing.is_empty() {
            return Ok(found);
        }
        // フォルダで絞るのは ls-tree で（rev-list のパスはコミットを選ぶのに使われ、tree の中身を絞らない）。
        let mut args = vec!["ls-tree", "-r", "-z", "--full-tree", &commit];
        let attributes = attributes_above(paths);
        if narrows_fetch(paths) {
            args.push("--");
            args.extend(paths.iter().map(String::as_str));
            args.extend(attributes.iter().map(String::as_str));
        }
        let mut seen = OidSet::default();
        for entry in self.read(&args)?.split(|&b| b == 0) {
            // `<mode> <type> <id>\t<path>`
            let Some(head) = entry.split(|&b| b == b'\t').next() else {
                continue;
            };
            let mut fields = head.split(|&b| b == b' ');
            let (Some(_), Some(b"blob"), Some(id)) = (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Some(id) = Oid::from_hex(&String::from_utf8_lossy(id)) else {
                continue;
            };
            if missing.contains(&id) && seen.insert(id) {
                if found.ids.len() == limit {
                    found.more = true;
                    break;
                }
                found.ids.push(id);
            }
        }
        Ok(found)
    }
}

/// git archive に、`paths` の下の中身（と、上のフォルダの `.gitattributes`）だけを読ませられるか。index を作らずに
/// コミットの tree から .gitattributes を読む `--attr-source` が git 2.41 から。
fn narrows_fetch(paths: &[String]) -> bool {
    !paths.is_empty() && crate::git_at_least(2, 41)
}

/// `paths` の上にあるフォルダ（いちばん上を含む）の `.gitattributes`。git archive は、書き出すファイルの属性を
/// それらからも読む。
fn attributes_above(paths: &[String]) -> Vec<String> {
    let mut found = std::collections::BTreeSet::from([".gitattributes".to_owned()]);
    for path in paths {
        let path = path.trim_end_matches('/');
        for (i, _) in path.match_indices('/') {
            found.insert(format!("{}/.gitattributes", &path[..i]));
        }
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::attributes_above;

    #[test]
    fn lists_the_attributes_of_the_folders_above() {
        let paths = [
            "docs".to_owned(),
            "src/app/".to_owned(),
            "src/lib/a.rs".to_owned(),
        ];
        assert_eq!(
            attributes_above(&paths),
            [
                ".gitattributes",
                "src/.gitattributes",
                "src/lib/.gitattributes"
            ]
        );
    }
}
