//! bundle のファイル（`git bundle create` が書くもの）の見出しを読む。

/// bundle のファイルの見出し（git を動かさずに読む。どの版の git でも同じ形）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BundleInfo {
    /// 入っている ref（`refs/heads/main` など。HEAD は除く）。
    pub refs: Vec<String>,
    /// HEAD が入っている（clone したとき、それを取り出す）。
    pub head: bool,
    /// 前提のコミットの数（受け取る側が持っているはずのもの。あれば clone できない）。
    pub prerequisites: usize,
    /// 前提のコミットの ID（16 進）。
    pub prerequisite_ids: Vec<String>,
}

/// `reader` が bundle のファイルなら、その見出し（git を動かさずに読む）。bundle でなければ None。
///
/// 形（`git bundle` の v2・v3）: `# v2 git bundle`（v3 なら続けて `@capability` の行）、前提のコミットの
/// `-<id> <comment>` の行、ref の `<id> <name>` の行、空の行、その後に pack。
pub fn parse_bundle_header(reader: impl std::io::BufRead) -> Option<BundleInfo> {
    let mut lines = reader.split(b'\n');
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
        if let Some(rest) = line.strip_prefix('-') {
            info.prerequisites += 1;
            if let Some(id) = rest.split(' ').next() {
                info.prerequisite_ids.push(id.to_owned());
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_header_of_v2_and_v3_bundles() {
        let v2 = b"# v2 git bundle\n-aaaa c1\nbbbb HEAD\nbbbb refs/heads/main\n\nPACK";
        let info = parse_bundle_header(&v2[..]).unwrap();
        assert_eq!(info.refs, ["refs/heads/main"]);
        assert!(info.head);
        assert_eq!(
            (info.prerequisites, info.prerequisite_ids.clone()),
            (1, vec!["aaaa".to_owned()])
        );
        let v3 = b"# v3 git bundle\n@object-format=sha1\ncccc refs/tags/v1\n\n";
        assert_eq!(parse_bundle_header(&v3[..]).unwrap().refs, ["refs/tags/v1"]);
        assert!(parse_bundle_header(&b"not a bundle\n"[..]).is_none());
        // 見出しが終わらないもの。
        assert!(parse_bundle_header(&b"# v2 git bundle\nbbbb refs/heads/main\n"[..]).is_none());
    }
}
