//! コミットの詳細のうち git に頼む分（署名の検証・notes・変更の行数）。

use amonhen_model::{CommitExtras, FileStat, Oid, Result, SignatureCheck};

use crate::Git;

impl Git {
    pub fn commit_extras(&self, commit: &Oid) -> Result<CommitExtras> {
        let hex = commit.to_string();
        // log.showSignature が有効だと、検証の出力が書式の前に混ざるので切る。
        let log = self.read(&[
            "-c",
            "log.showSignature=false",
            "log",
            "-1",
            "--format=%G?%x00%GS%x00%GK%x00%GF%x00%GT%x00%N",
            &hex,
        ])?;
        let (signature, notes) = parse_signature_and_notes(&String::from_utf8_lossy(&log));
        // 最初の親と比べる（merge でも、変わったファイルの一覧と同じ比べ方にする）。
        let numstat = self.read(&[
            "diff-tree",
            "-r",
            "-z",
            "-M",
            "--numstat",
            "--root",
            "--no-commit-id",
            "--diff-merges=first-parent",
            &hex,
        ])?;
        Ok(CommitExtras {
            signature,
            notes,
            stats: parse_numstat(&numstat),
        })
    }
}

impl Git {
    /// `ids` の署名をまとめて検証し、結果を同じ順に返す（署名がなければ N）。git を 1 回だけ起動する。
    pub fn verify_signatures(&self, ids: &[Oid]) -> Result<Vec<SignatureCheck>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let input: String = ids.iter().map(|id| format!("{id}\n")).collect();
        let out = self.read_with_input(
            &[
                "-c",
                "log.showSignature=false",
                "log",
                "--no-walk=unsorted",
                "--format=%H%x00%G?%x00%GS%x00%GK%x00%GF%x00%GT",
                "--stdin",
            ],
            input.as_bytes(),
        )?;
        let found: std::collections::HashMap<String, SignatureCheck> =
            String::from_utf8_lossy(&out)
                .lines()
                .filter_map(|line| {
                    let (hex, rest) = line.split_once('\0')?;
                    let mut f = rest.split('\0');
                    let mut next = || f.next().unwrap_or_default().to_owned();
                    let status = next().chars().next()?;
                    Some((
                        hex.to_owned(),
                        SignatureCheck {
                            status,
                            signer: next(),
                            key: next(),
                            fingerprint: next(),
                            trust: next(),
                        },
                    ))
                })
                .collect();
        Ok(ids
            .iter()
            .map(|id| {
                found
                    .get(&id.to_string())
                    .cloned()
                    .unwrap_or(SignatureCheck {
                        status: 'E',
                        ..Default::default()
                    })
            })
            .collect())
    }
}

impl Git {
    /// 1 つのコミットの署名の検証の詳細（署名者・鍵・信用度）。
    pub fn signature_details(&self, commit: &Oid) -> Result<Option<SignatureCheck>> {
        let out = self.read(&[
            "-c",
            "log.showSignature=false",
            "log",
            "-1",
            "--format=%G?%x00%GS%x00%GK%x00%GF%x00%GT%x00",
            &commit.to_string(),
        ])?;
        Ok(parse_signature_and_notes(&String::from_utf8_lossy(&out)).0)
    }
}

/// `%G?%x00%GS%x00%GK%x00%GF%x00%GT%x00%N` を読む。署名がなければ（N）`None`。
fn parse_signature_and_notes(out: &str) -> (Option<SignatureCheck>, String) {
    let mut fields = out.splitn(6, '\0');
    let mut next = || fields.next().unwrap_or_default().to_owned();
    let (status, signer, key, fingerprint, trust, notes) =
        (next(), next(), next(), next(), next(), next());
    let status = status.trim().chars().next().unwrap_or('N');
    let signature = (status != 'N').then_some(SignatureCheck {
        status,
        signer,
        key,
        fingerprint,
        trust,
    });
    (signature, notes.trim().to_owned())
}

/// `git diff-tree -z --numstat` を読む。名前の変わったファイルは `追加\t削除\t\0元\0先\0`。
fn parse_numstat(out: &[u8]) -> Vec<FileStat> {
    let mut stats = Vec::new();
    let mut records = out.split(|&b| b == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(record);
        let mut parts = text.splitn(3, '\t');
        let count = |s: Option<&str>| s.and_then(|s| s.parse().ok());
        let (added, removed) = (count(parts.next()), count(parts.next()));
        let mut path = parts.next().unwrap_or_default().to_owned();
        if path.is_empty() {
            // 名前が変わった: 元のパスと新しいパスが続く。新しいほうを使う。
            let _old = records.next();
            path = records
                .next()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .unwrap_or_default();
        }
        stats.push(FileStat {
            path,
            added,
            removed,
        });
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_signature_and_notes() {
        let (sig, notes) = parse_signature_and_notes(
            "G\0Alice <a@example.com>\x003BDF373F5DD89FBB\0B9CF3D7E\0ultimate\0reviewed\n\n",
        );
        let sig = sig.unwrap();
        assert_eq!(
            (sig.status, sig.signer.as_str()),
            ('G', "Alice <a@example.com>")
        );
        assert_eq!(
            (sig.key.as_str(), sig.trust.as_str()),
            ("3BDF373F5DD89FBB", "ultimate")
        );
        assert_eq!(notes, "reviewed");
        assert_eq!(
            parse_signature_and_notes("N\0\0\0\0\0\n"),
            (None, String::new())
        );
    }

    #[test]
    fn reads_numstat_with_renames_and_binaries() {
        let out = b"3\t1\tsrc/a.rs\0-\t-\timg.png\x000\t0\t\0old.txt\0new.txt\0";
        let stats = parse_numstat(out);
        assert_eq!(stats.len(), 3);
        assert_eq!(
            (stats[0].path.as_str(), stats[0].added, stats[0].removed),
            ("src/a.rs", Some(3), Some(1))
        );
        assert_eq!((stats[1].added, stats[1].removed), (None, None));
        assert_eq!(stats[2].path, "new.txt");
    }

    #[test]
    fn reads_extras_from_a_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new(tmp.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "a"],
            &["config", "user.email", "a@example.com"],
            &["config", "commit.gpgsign", "false"],
            &["config", "core.autocrlf", "false"],
        ] {
            git.write(args).unwrap();
        }
        std::fs::write(tmp.path().join("a.txt"), "1\n2\n").unwrap();
        git.stage(&["a.txt"]).unwrap();
        git.commit("first", false).unwrap();
        let head = Oid::from_hex(
            String::from_utf8(git.read(&["rev-parse", "HEAD"]).unwrap())
                .unwrap()
                .trim(),
        )
        .unwrap();
        git.write(&["notes", "add", "-m", "a note"]).unwrap();
        let extras = git.commit_extras(&head).unwrap();
        assert_eq!(extras.signature, None);
        assert_eq!(extras.notes, "a note");
        let checks = git.verify_signatures(&[head, head]).unwrap();
        assert_eq!(
            checks.iter().map(|c| c.status).collect::<Vec<_>>(),
            ['N', 'N']
        );
        assert!(git.verify_signatures(&[]).unwrap().is_empty());
        assert_eq!(
            extras.stats,
            [FileStat {
                path: "a.txt".into(),
                added: Some(2),
                removed: Some(0)
            }]
        );
    }
}
