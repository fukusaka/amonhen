use amonhen_model::tr;
use amonhen_model::{Oid, Result, Status, StatusEntry, StatusKind};

/// `git status --porcelain=v2 -z --branch --untracked-files=all` の出力を読む。
pub fn parse_status(output: &[u8]) -> Result<Status> {
    let mut status = Status::default();
    let mut records = output.split(|&b| b == 0).filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        let text = std::str::from_utf8(record)?;
        let fields = |n: usize| -> Result<(Vec<&str>, &str)> {
            let mut parts = text.splitn(n + 1, ' ');
            let tokens: Vec<&str> = parts.by_ref().take(n).collect();
            let rest = parts.next().ok_or_else(|| {
                tr!(
                    "status line is too short: {text}",
                    text = format!("{text:?}")
                )
            })?;
            Ok((tokens, rest))
        };
        let oid = |hex: &str| Oid::from_hex(hex).filter(|o| !o.is_zero());
        let mode = |octal: &str| u32::from_str_radix(octal, 8).unwrap_or(0);
        match record[0] {
            b'#' => {
                let (tokens, value) = fields(2)?;
                match tokens[1] {
                    "branch.head" if value != "(detached)" => {
                        status.branch = Some(value.to_owned())
                    }
                    "branch.upstream" => status.upstream = Some(value.to_owned()),
                    "branch.ab" => {
                        let mut ab = value.split(' ');
                        let mut num = |prefix| {
                            ab.next()
                                .and_then(|v: &str| v.strip_prefix(prefix))
                                .and_then(|v| v.parse().ok())
                        };
                        status.ahead = num('+').unwrap_or(0);
                        status.behind = num('-').unwrap_or(0);
                    }
                    _ => {}
                }
            }
            b'1' | b'2' => {
                // 1 XY sub mH mI mW hH hI path / 2 XY sub mH mI mW hH hI Xscore path\0origPath
                let renamed = record[0] == b'2';
                let (tokens, path) = fields(if renamed { 9 } else { 8 })?;
                let xy = tokens[1].as_bytes();
                let orig_path = if renamed {
                    let orig = records
                        .next()
                        .ok_or_else(|| tr!("Missing the original path of a rename"))?;
                    Some(String::from_utf8_lossy(orig).into_owned())
                } else {
                    None
                };
                status.entries.push(StatusEntry {
                    kind: if renamed {
                        StatusKind::Renamed
                    } else {
                        StatusKind::Changed
                    },
                    path: path.to_owned(),
                    orig_path,
                    index: xy[0],
                    worktree: xy[1],
                    head_id: oid(tokens[6]),
                    index_id: oid(tokens[7]),
                    index_mode: mode(tokens[4]),
                    submodule: tokens[2] != "N...",
                });
            }
            b'u' => {
                // u XY sub m1 m2 m3 mW h1 h2 h3 path
                let (tokens, path) = fields(10)?;
                let xy = tokens[1].as_bytes();
                status.entries.push(StatusEntry {
                    kind: StatusKind::Unmerged,
                    path: path.to_owned(),
                    orig_path: None,
                    index: xy[0],
                    worktree: xy[1],
                    head_id: oid(tokens[7]),
                    index_id: None,
                    index_mode: 0,
                    submodule: tokens[2] != "N...",
                });
            }
            b'?' => status.entries.push(StatusEntry {
                kind: StatusKind::Untracked,
                path: text[2..].to_owned(),
                orig_path: None,
                index: b'?',
                worktree: b'?',
                head_id: None,
                index_id: None,
                index_mode: 0,
                submodule: false,
            }),
            // 無視したファイル（`!`）は出さない。
            _ => {}
        }
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2() {
        let h = "a".repeat(40);
        let i = "b".repeat(40);
        let z = "0".repeat(40);
        let out = format!(
            "# branch.oid {h}\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
             1 MM N... 100644 100644 100644 {h} {i} src/a b.rs\0\
             1 A. N... 000000 100644 100644 {z} {i} new.txt\0\
             2 R. N... 100644 100644 100644 {h} {h} R100 to.txt\0from.txt\0\
             u UU N... 100644 100644 100644 100644 {h} {i} {h} conflict.txt\0\
             ? untracked.txt\0! ignored.txt\0"
        );
        let s = parse_status(out.as_bytes()).unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert_eq!(s.entries.len(), 5);

        let a = &s.entries[0];
        assert_eq!(
            (a.path.as_str(), a.index, a.worktree),
            ("src/a b.rs", b'M', b'M')
        );
        assert_eq!(a.head_id, Oid::from_hex(&h));
        assert_eq!(a.index_mode, 0o100644);
        assert!(a.is_staged() && a.is_unstaged());

        let new = &s.entries[1];
        assert_eq!(new.head_id, None);
        assert!(new.is_staged() && !new.is_unstaged());

        let r = &s.entries[2];
        assert_eq!(
            (r.kind, r.orig_path.as_deref()),
            (StatusKind::Renamed, Some("from.txt"))
        );
        assert_eq!(s.entries[3].kind, StatusKind::Unmerged);
        let u = &s.entries[4];
        assert_eq!(
            (u.kind, u.path.as_str()),
            (StatusKind::Untracked, "untracked.txt")
        );
        assert!(!u.is_staged() && u.is_unstaged());
    }
}
