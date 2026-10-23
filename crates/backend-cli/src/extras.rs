//! コミットの詳細のうち git に頼む分（署名の検証・notes・変更の行数）。

use amonhen_model::{CommitExtras, FileStat, Oid, Result, SignatureCheck, SignerEmail};

use crate::Git;

impl Git {
    pub fn commit_extras(&self, commit: &Oid) -> Result<CommitExtras> {
        let hex = commit.to_string();
        let gpg = self.gpg_without_key_fetch();
        // log.showSignature が有効だと、検証の出力が書式の前に混ざるので切る。
        let log = self.read(&[
            "-c",
            "log.showSignature=false",
            "-c",
            &gpg,
            "log",
            "-1",
            "--format=%G?%x00%GS%x00%GK%x00%GF%x00%GT%x00%GP%x00%N",
            &hex,
        ])?;
        let (signature, primary, notes) = parse_signature_and_notes(&String::from_utf8_lossy(&log));
        let signature = signature.map(|s| self.with_emails(vec![(s, primary)]).remove(0));
        // 最初の親と比べる（merge でも、変わったファイルの一覧と同じ比べ方にする）。`--diff-merges` は git 2.31
        // から。それより前は `-m --first-parent` が同じ意味になる（新しい git では両方の親と比べてしまう）。
        let mut args = vec![
            "diff-tree",
            "-r",
            "-z",
            "-M",
            "--numstat",
            "--root",
            "--no-commit-id",
        ];
        if crate::git_at_least(2, 31) {
            args.push("--diff-merges=first-parent");
        } else {
            args.extend(["-m", "--first-parent"]);
        }
        args.push(&hex);
        let numstat = self.read(&args)?;
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
        let gpg = self.gpg_without_key_fetch();
        let out = self.read_with_input(
            &[
                "-c",
                "log.showSignature=false",
                "-c",
                &gpg,
                "log",
                "--no-walk=unsorted",
                "--format=%H%x00%G?%x00%GS%x00%GK%x00%GF%x00%GT%x00%GP",
                "--stdin",
            ],
            input.as_bytes(),
        )?;
        let found: std::collections::HashMap<String, (SignatureCheck, String)> =
            String::from_utf8_lossy(&out)
                .lines()
                .filter_map(|line| {
                    let (hex, rest) = line.split_once('\0')?;
                    let mut f = rest.split('\0');
                    let mut next = || known(f.next().unwrap_or_default());
                    let status = next().chars().next()?;
                    let check = SignatureCheck {
                        status,
                        signer: next(),
                        key: next(),
                        fingerprint: next(),
                        trust: next(),
                        emails: Vec::new(),
                    };
                    Some((hex.to_owned(), (check, next())))
                })
                .collect();
        let checks = ids
            .iter()
            .map(|id| {
                found.get(&id.to_string()).cloned().unwrap_or_else(|| {
                    let check = SignatureCheck {
                        status: 'E',
                        ..Default::default()
                    };
                    (check, String::new())
                })
            })
            .collect();
        Ok(self.with_emails(checks))
    }
}

impl Git {
    /// 検証の結果に、署名した鍵のメールアドレスを足す（`primary` は主鍵の指紋 `%GP`）。GPG は gpg で鍵の
    /// ユーザー ID をすべて読み（鍵が何個あっても gpg は 1 回）、SSH は git が allowed signers から見つけた
    /// プリンシパル（`%GS`）を使う。読めなければ空のまま。
    fn with_emails(&self, checks: Vec<(SignatureCheck, String)>) -> Vec<SignatureCheck> {
        let signed = |c: &SignatureCheck| matches!(c.status, 'G' | 'U' | 'X' | 'Y' | 'R');
        let primaries: Vec<&str> = checks
            .iter()
            .filter(|(c, p)| signed(c) && !p.is_empty() && !c.key.starts_with("SHA256:"))
            .map(|(_, p)| p.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let uids = if primaries.is_empty() {
            Default::default()
        } else {
            self.key_uids(&primaries)
        };
        checks
            .into_iter()
            .map(|(mut c, primary)| {
                if c.key.starts_with("SHA256:") {
                    if signed(&c) {
                        c.emails = c
                            .signer
                            .split(',')
                            .map(str::trim)
                            .filter(|p| !p.is_empty())
                            .map(|p| SignerEmail {
                                email: p.to_owned(),
                                validity: 'f',
                            })
                            .collect();
                    }
                } else if let Some(emails) = uids.get(&primary) {
                    c.emails = emails.clone();
                }
                c
            })
            .collect()
    }

    /// 設定されている gpg（`gpg.program`、なければ `gpg`）。
    fn gpg_program(&self) -> String {
        self.read(&["config", "--get", "gpg.program"])
            .ok()
            .map(|p| String::from_utf8_lossy(&p).trim().to_owned())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "gpg".to_owned())
    }

    /// 検証のときに git に渡す `gpg.program=…`。gpg の `auto-key-retrieve` が有効だと、知らない鍵の署名を
    /// 検証するたびに鍵サーバーや WKD から鍵を取って鍵束に加えてしまう（裏で何百も検証するので困る）。
    /// git は gpg に引数を足す設定を持たないので、`--no-auto-key-retrieve` を付けて gpg を呼ぶ小さな
    /// スクリプトを置き、それを指す。作れなければ設定どおりの gpg を使う。
    fn gpg_without_key_fetch(&self) -> String {
        let program = self.gpg_program();
        match no_fetch_wrapper(&program) {
            Some(path) => format!("gpg.program={}", path.display()),
            None => format!("gpg.program={program}"),
        }
    }

    /// `primaries`（主鍵の指紋）の鍵のユーザー ID のメールアドレスと有効性。gpg を 1 回だけ起動する。
    fn key_uids(&self, primaries: &[&str]) -> std::collections::HashMap<String, Vec<SignerEmail>> {
        let program = self.gpg_program();
        let out = amonhen_model::process::command(&program)
            .args([
                "--batch",
                "--no-tty",
                "--list-keys",
                "--with-colons",
                "--fixed-list-mode",
            ])
            .args(primaries)
            .env("LC_ALL", "C")
            .stderr(std::process::Stdio::null())
            .output();
        match out {
            Ok(out) => parse_key_uids(&String::from_utf8_lossy(&out.stdout)),
            Err(_) => Default::default(),
        }
    }

    /// 1 つのコミットの署名の検証の詳細（署名者・鍵・信用度）。
    pub fn signature_details(&self, commit: &Oid) -> Result<Option<SignatureCheck>> {
        let gpg = self.gpg_without_key_fetch();
        let out = self.read(&[
            "-c",
            "log.showSignature=false",
            "-c",
            &gpg,
            "log",
            "-1",
            "--format=%G?%x00%GS%x00%GK%x00%GF%x00%GT%x00%GP%x00",
            &commit.to_string(),
        ])?;
        let (signature, primary, _) = parse_signature_and_notes(&String::from_utf8_lossy(&out));
        Ok(signature.map(|s| self.with_emails(vec![(s, primary)]).remove(0)))
    }
}

/// `program` を `--no-auto-key-retrieve` 付きで呼ぶスクリプト（gpg ごとに 1 つ、一時フォルダに置く）。
#[cfg(unix)]
fn no_fetch_wrapper(program: &str) -> Option<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    use std::os::unix::fs::PermissionsExt;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    program.hash(&mut hasher);
    let name = format!("amonhen-gpg-{:016x}", hasher.finish());
    let path = std::env::temp_dir().join(name);
    let script = format!(
        "#!/bin/sh\n# Amon Hen: 署名を検証するときに、gpg に鍵を取りに行かせない。\nexec '{}' --no-auto-key-retrieve \"$@\"\n",
        program.replace('\'', "'\\''")
    );
    if std::fs::read_to_string(&path).ok().as_deref() != Some(script.as_str()) {
        // 別の名前で書いてから置き換える（同時に検証していても、書きかけを実行しない）。
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, &script).ok()?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).ok()?;
        std::fs::rename(&tmp, &path).ok()?;
    }
    Some(path)
}

#[cfg(not(unix))]
fn no_fetch_wrapper(_program: &str) -> Option<std::path::PathBuf> {
    None
}

/// `--format` の欄。git が知らない書式（`%GT` は 2.26 から）はそのまま出てくるので、空とみなす。
fn known(field: &str) -> String {
    let unknown = field.len() == 3 && field.starts_with("%G");
    if unknown {
        String::new()
    } else {
        field.to_owned()
    }
}

/// `%G?%x00%GS%x00%GK%x00%GF%x00%GT%x00%GP%x00%N` を読む（署名と主鍵の指紋と notes）。署名がなければ（N）`None`。
fn parse_signature_and_notes(out: &str) -> (Option<SignatureCheck>, String, String) {
    let mut fields = out.splitn(7, '\0');
    let mut next = || known(fields.next().unwrap_or_default());
    let (status, signer, key, fingerprint, trust, primary, notes) =
        (next(), next(), next(), next(), next(), next(), next());
    let status = status.trim().chars().next().unwrap_or('N');
    let signature = (status != 'N').then_some(SignatureCheck {
        status,
        signer,
        key,
        fingerprint,
        trust,
        emails: Vec::new(),
    });
    (signature, primary, notes.trim().to_owned())
}

/// `gpg --list-keys --with-colons --fixed-list-mode` を読み、主鍵の指紋ごとにユーザー ID のメールアドレスと
/// 有効性を返す。ユーザー ID の `:` は `\x3a` と書かれる。
fn parse_key_uids(out: &str) -> std::collections::HashMap<String, Vec<SignerEmail>> {
    let mut keys: std::collections::HashMap<String, Vec<SignerEmail>> = Default::default();
    let mut current: Option<String> = None;
    let mut want_fpr = false;
    for line in out.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        match fields.first().copied() {
            Some("pub") => {
                current = None;
                want_fpr = true;
            }
            Some("sub") | Some("ssb") => want_fpr = false,
            Some("fpr") if want_fpr => {
                let fpr = fields.get(9).copied().unwrap_or_default().to_owned();
                keys.entry(fpr.clone()).or_default();
                current = Some(fpr);
                want_fpr = false;
            }
            Some("uid") => {
                let (Some(fpr), Some(uid)) = (&current, fields.get(9)) else {
                    continue;
                };
                let uid = uid.replace("\\x3a", ":");
                let email = match (uid.rfind('<'), uid.rfind('>')) {
                    (Some(a), Some(b)) if a < b => uid[a + 1..b].to_owned(),
                    _ if uid.contains('@') => uid.trim().to_owned(),
                    _ => continue,
                };
                let validity = fields.get(1).and_then(|v| v.chars().next()).unwrap_or('-');
                keys.get_mut(fpr)
                    .unwrap()
                    .push(SignerEmail { email, validity });
            }
            _ => {}
        }
    }
    keys
}

/// `git diff-tree -z --numstat` を読む。名前の変わったファイルは `追加\t削除\t\0元\0先\0`。
pub(crate) fn parse_numstat(out: &[u8]) -> Vec<FileStat> {
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
#[allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する
mod tests {
    use super::*;

    #[test]
    fn reads_signature_and_notes() {
        let (sig, primary, notes) = parse_signature_and_notes(
            "G\0Alice <a@example.com>\x003BDF373F5DD89FBB\0B9CF3D7E\0ultimate\0ABCD\0reviewed\n\n",
        );
        assert_eq!(primary, "ABCD");
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
            parse_signature_and_notes("N\0\0\0\0\0\0\n"),
            (None, String::new(), String::new())
        );
        // git 2.26 より前は `%GT` を知らず、そのまま出す。
        let (sig, primary, _) = parse_signature_and_notes("G\0Alice\0KEY\0FPR\0%GT\0ABCD\0\n");
        assert_eq!(
            (sig.unwrap().trust.as_str(), primary.as_str()),
            ("", "ABCD")
        );
    }

    #[cfg(unix)]
    #[test]
    fn calls_gpg_without_fetching_keys() {
        let tmp = tempfile::tempdir().unwrap();
        // 渡された引数を書き出すだけの偽の gpg。
        let fake = tmp.path().join("fake gpg");
        let log = tmp.path().join("args");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\necho \"$@\" > '{}'\n", log.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let wrapper = no_fetch_wrapper(fake.to_str().unwrap()).unwrap();
        // 書いたばかりのファイルは、並んで走るほかの試験が子プロセスを作った瞬間にその子へ書き込みの記述子が渡り、
        // Linux では実行すると「Text file busy」で断られることがある。そのときは少し待って実行し直す。
        let status = (0..20)
            .find_map(|_| {
                match std::process::Command::new(&wrapper)
                    .args(["--status-fd=1", "--verify", "sig", "-"])
                    .status()
                {
                    Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        None
                    }
                    other => Some(other.unwrap()),
                }
            })
            .expect("still busy after retries");
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().trim(),
            "--no-auto-key-retrieve --status-fd=1 --verify sig -"
        );
    }

    #[test]
    fn reads_every_user_id_of_a_key() {
        let out = "tru::1:1:0:3:1:5\npub:-:4096:1:7B3C391EFEA93624:1:::-:::scESC::::::23::0:\nfpr:::::::::4880B8C9BD0E5106FC070F4F7B3C391EFEA93624:\nuid:-::::1::A::Christian Brauner <christian@brauner.io>::::::::::0:\nuid:f::::1::B::Christian Brauner <brauner@kernel.org>::::::::::0:\nuid:r::::1::C::Old <old@x.org>::::::::::0:\nsub:-:4096:1:91C61BC06578DCA2:1::::::s::::::23:\nfpr:::::::::408734571EA70C78B332692891C61BC06578DCA2:\n";
        let keys = parse_key_uids(out);
        let emails = &keys["4880B8C9BD0E5106FC070F4F7B3C391EFEA93624"];
        assert_eq!(emails.len(), 3, "副鍵の指紋は主鍵にしない");
        assert_eq!(
            (emails[1].email.as_str(), emails[1].validity),
            ("brauner@kernel.org", 'f')
        );
        assert_eq!(emails[2].validity, 'r');
        assert!(!keys.contains_key("408734571EA70C78B332692891C61BC06578DCA2"));
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
            &["init", "-q"][..],
            &["symbolic-ref", "HEAD", "refs/heads/main"],
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
