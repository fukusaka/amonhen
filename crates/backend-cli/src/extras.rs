//! コミットの詳細のうち git に頼む分（署名の検証・notes・変更の行数）。

use std::collections::HashSet;

use amonhen_model::{CommitExtras, CommitText, FileStat, Oid, Result, SignatureCheck, SignerEmail};

use crate::Git;

impl Git {
    pub fn commit_extras(&self, commit: &Oid) -> Result<CommitExtras> {
        let hex = commit.to_string();
        let gpg = self.gpg_without_key_fetch();
        // この git が確かめられない署名は、git に検証を頼まずに notes だけ読む。
        let unsupported = !self.unverifiable(&[*commit])?.is_empty();
        // log.showSignature が有効だと、検証の出力が書式の前に混ざるので切る。
        let log = self.read(&[
            "-c",
            "log.showSignature=false",
            "-c",
            &gpg,
            "log",
            "-1",
            if unsupported {
                "--format=%x00%x00%x00%x00%x00%x00%N"
            } else {
                "--format=%G?%x00%GS%x00%GK%x00%GF%x00%GT%x00%GP%x00%N"
            },
            &hex,
        ])?;
        let (signature, primary, notes) = parse_signature_and_notes(&String::from_utf8_lossy(&log));
        let signature = if unsupported {
            Some(unsupported_check())
        } else {
            signature.map(|s| self.with_emails(vec![(s, primary)]).remove(0))
        };
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
        let unsupported = self.unverifiable(ids)?;
        let input: String = ids
            .iter()
            .filter(|id| !unsupported.contains(id))
            .map(|id| format!("{id}\n"))
            .collect();
        if input.is_empty() {
            return Ok(vec![unsupported_check(); ids.len()]);
        }
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
                if unsupported.contains(id) {
                    return (unsupported_check(), String::new());
                }
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
    /// プリンシパル（`%GS`）を使う。読めなければ空のまま。信用度が読めなかった（git 2.26 より前）ら、GPG は
    /// 鍵の有効性から足す。
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
                } else if let Some(key) = uids.get(&primary) {
                    c.emails = key.emails.clone();
                    // 信用度（`%GT`）は git 2.26 から。それより前の git では読めないので、鍵の有効性から決める
                    // （gpg が検証で返す信用度は、署名した鍵の有効性そのもの）。
                    if c.trust.is_empty() {
                        c.trust = trust_from_validity(key.validity).to_owned();
                    }
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
    fn key_uids(&self, primaries: &[&str]) -> std::collections::HashMap<String, KeyInfo> {
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
        if !self.unverifiable(&[*commit])?.is_empty() {
            return Ok(Some(unsupported_check()));
        }
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

impl Git {
    /// `ids` のうち、この git では確かめられない署名のコミット。SSH の署名は git 2.34 からで、それより前の git に
    /// `%G?` で頼むと、署名を読めずに git が止まる（`BUG: gpg-interface.c: bad signature`。まとめて頼んだほかの
    /// コミットの結果も返らない）。新しい git では何も読まない。
    fn unverifiable(&self, ids: &[Oid]) -> Result<HashSet<Oid>> {
        if ids.is_empty() || crate::git_at_least(2, 34) {
            return Ok(HashSet::new());
        }
        let input: String = ids.iter().map(|id| format!("{id}\n")).collect();
        let out = self.read_with_input(&["cat-file", "--batch"], input.as_bytes())?;
        Ok(ids
            .iter()
            .zip(signature_kinds(&out))
            .filter(|(_, kind)| *kind == CommitText::SIGNED_SSH)
            .map(|(id, _)| *id)
            .collect())
    }
}

/// タグの署名の検証。
impl Git {
    /// `tags`（タグのオブジェクトの ID と中身）の署名を検証し、結果を同じ順に返す（署名がなければ N、この git
    /// で確かめられない種類なら V）。git には `%G?` に当たるものがタグにないので、`git verify-tag --raw` の出力を
    /// git 自身と同じ読み方で読む。merge したタグはマージコミットの mergetag にしか残っていないことがあるので、
    /// 中身を一時的なオブジェクトの置き場に書き、そこを足して確かめる（リポジトリのオブジェクトは増やさず、
    /// リポジトリの設定で確かめる）。タグごとに git（と gpg）を 1 回起動するので、いくつか並べて動かす。
    pub fn verify_tags(&self, tags: &[(Oid, &[u8])]) -> Result<Vec<SignatureCheck>> {
        let mut checks: Vec<Option<(SignatureCheck, String)>> = tags
            .iter()
            .map(|(_, raw)| match CommitText::tag_signature_kind(raw) {
                0 => Some((
                    SignatureCheck {
                        status: 'N',
                        ..Default::default()
                    },
                    String::new(),
                )),
                CommitText::SIGNED_SSH if !crate::git_at_least(2, 34) => {
                    Some((unsupported_check(), String::new()))
                }
                _ => None,
            })
            .collect();
        let pending: Vec<usize> = (0..tags.len()).filter(|&i| checks[i].is_none()).collect();
        if !pending.is_empty() {
            let objects = TemporaryObjects::new()?;
            let ids = objects.write(self, pending.iter().map(|&i| (&tags[i].0, tags[i].1)))?;
            let gpg = self.gpg_without_key_fetch();
            let next = std::sync::atomic::AtomicUsize::new(0);
            let found = std::sync::Mutex::new(Vec::new());
            std::thread::scope(|scope| {
                for _ in 0..pending.len().min(8) {
                    scope.spawn(|| {
                        loop {
                            let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(&i) = pending.get(n) else { break };
                            let check = if ids.contains(&tags[i].0) {
                                let mut cmd = self.command(&[
                                    "-c",
                                    &gpg,
                                    "verify-tag",
                                    "--raw",
                                    &tags[i].0.to_string(),
                                ]);
                                cmd.env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &objects.dir)
                                    .env("GIT_OPTIONAL_LOCKS", "0");
                                crate::run_lenient(cmd).map(|(_, _, err)| {
                                    parse_verify_tag(&String::from_utf8_lossy(&err))
                                })
                            } else {
                                // 中身が ID と合わない（壊れている）。
                                Ok(parse_verify_tag(""))
                            };
                            found.lock().unwrap().push((i, check));
                        }
                    });
                }
            });
            for (i, check) in found.into_inner().unwrap() {
                checks[i] = Some(check?);
            }
        }
        Ok(self.with_emails(checks.into_iter().map(Option::unwrap).collect()))
    }
}

/// 検証のあいだだけ使う、オブジェクトの置き場（`GIT_OBJECT_DIRECTORY` に使えるフォルダ）。消すときに中身ごと消す。
struct TemporaryObjects {
    dir: std::path::PathBuf,
}

impl TemporaryObjects {
    fn new() -> Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("amonhen-tags-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// タグのオブジェクトを書き（git は 1 回だけ起動する）、中身が ID と合ったものの ID を返す。
    fn write<'a>(
        &self,
        git: &Git,
        tags: impl Iterator<Item = (&'a Oid, &'a [u8])>,
    ) -> Result<HashSet<Oid>> {
        let mut paths = String::new();
        let mut ids = Vec::new();
        for (i, (id, raw)) in tags.enumerate() {
            let path = self.dir.join(format!("tag-{i}"));
            std::fs::write(&path, raw)?;
            paths.push_str(&format!("{}\n", path.display()));
            ids.push(*id);
        }
        let mut cmd = git.command(&[
            "hash-object",
            "-t",
            "tag",
            "-w",
            "--literally",
            "--stdin-paths",
        ]);
        cmd.env("GIT_OBJECT_DIRECTORY", &self.dir);
        let written = crate::run(cmd, Some(paths.as_bytes()))?;
        let written = String::from_utf8_lossy(&written);
        Ok(ids
            .into_iter()
            .zip(written.lines())
            .filter(|(id, line)| line.trim() == id.to_string())
            .map(|(id, _)| id)
            .collect())
    }
}

impl Drop for TemporaryObjects {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `git verify-tag --raw` の標準エラーを読み、検証の結果と主鍵の指紋を返す（git 自身の `parse_gpg_output`・
/// `parse_ssh_output` と同じ読み方。どちらの出力も翻訳されない）。GPG と X.509（gpgsm）は `[GNUPG:]` の状態行、
/// SSH は ssh-keygen の文。どれもなければ（鍵の設定がないなど）E。
fn parse_verify_tag(out: &str) -> (SignatureCheck, String) {
    let mut check = SignatureCheck {
        status: 'E',
        ..Default::default()
    };
    let mut primary = String::new();
    for line in out.lines() {
        if let Some(status) = line.strip_prefix("[GNUPG:] ") {
            let (keyword, rest) = status.split_once(' ').unwrap_or((status, ""));
            let result = match keyword {
                "GOODSIG" => Some('G'),
                "BADSIG" => Some('B'),
                "EXPSIG" => Some('X'),
                "EXPKEYSIG" => Some('Y'),
                "REVKEYSIG" => Some('R'),
                "ERRSIG" => Some('E'),
                _ => None,
            };
            if let Some(result) = result {
                check.status = result;
                let (key, signer) = rest.split_once(' ').unwrap_or((rest, ""));
                check.key = key.to_owned();
                if result == 'E' {
                    // ERRSIG <鍵 ID> <公開鍵の方式> <ハッシュ> <種類> <日時> <理由> <指紋>
                    check.fingerprint = rest.split(' ').nth(6).unwrap_or_default().to_owned();
                } else {
                    check.signer = signer.to_owned();
                }
            } else if keyword == "VALIDSIG" {
                // VALIDSIG <指紋> … の 10 番目が主鍵の指紋。
                let fields: Vec<&str> = rest.split(' ').collect();
                check.fingerprint = fields.first().copied().unwrap_or_default().to_owned();
                primary = fields.get(9).copied().unwrap_or_default().to_owned();
            } else if let Some(trust) = keyword.strip_prefix("TRUST_") {
                check.trust = trust.to_lowercase();
                if matches!(trust, "UNDEFINED" | "NEVER") && check.status == 'G' {
                    check.status = 'U';
                }
            }
        } else if let Some(rest) = line.strip_prefix("Good \"git\" signature ") {
            // `for <プリンシパル> with <方式> key <指紋>`。allowed signers にない鍵は `with <方式> key <指紋>`。
            let (principal, key) = match rest.strip_prefix("for ") {
                Some(rest) => rest.rsplit_once(" with ").unwrap_or((rest, "")),
                None => ("", rest.strip_prefix("with ").unwrap_or(rest)),
            };
            let fingerprint = key.rsplit(' ').next().unwrap_or_default().to_owned();
            check.signer = principal.to_owned();
            check.key = fingerprint.clone();
            check.fingerprint = fingerprint;
            (check.status, check.trust) = if principal.is_empty() {
                ('U', "undefined".to_owned())
            } else {
                ('G', "fully".to_owned())
            };
        } else if (line.starts_with("Could not verify signature")
            || line.starts_with("Signature verification failed"))
            && check.status == 'E'
        {
            check.status = 'B';
            check.trust = "never".to_owned();
        } else if line.starts_with("error: no signature found") && check.status == 'E' {
            check.status = 'N';
        }
    }
    (check, primary)
}

/// この git では確かめられない署名の結果。
fn unsupported_check() -> SignatureCheck {
    SignatureCheck {
        status: SignatureCheck::UNSUPPORTED,
        ..Default::default()
    }
}

/// `git cat-file --batch` の出力を読み、オブジェクトごとに署名の種類（`CommitText::SIGNED_*`、なければ 0）を
/// 同じ順に返す。署名はヘッダーの `gpgsig`（SHA-256 のリポジトリでは `gpgsig-sha256`）。
fn signature_kinds(out: &[u8]) -> Vec<u8> {
    let mut kinds = Vec::new();
    let mut rest = out;
    while let Some(end) = rest.iter().position(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(&rest[..end]);
        rest = &rest[end + 1..];
        // `<id> <type> <size>`。ないオブジェクトは `<id> missing`。
        let Some(size) = line
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse::<usize>().ok())
        else {
            kinds.push(0);
            continue;
        };
        let body = &rest[..size.min(rest.len())];
        rest = rest.get(size + 1..).unwrap_or_default();
        let header_end = body
            .windows(2)
            .position(|w| w == b"\n\n")
            .unwrap_or(body.len());
        let kind = body[..header_end]
            .split(|&b| b == b'\n')
            .find_map(|l| {
                l.strip_prefix(b"gpgsig ")
                    .or_else(|| l.strip_prefix(b"gpgsig-sha256 "))
            })
            .map_or(0, CommitText::signature_kind);
        kinds.push(kind);
    }
    kinds
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

/// 鍵の有効性（gpg の `--with-colons` の 1 文字）を、`%GT` と同じ言葉の信用度にする。
fn trust_from_validity(validity: char) -> &'static str {
    match validity {
        'u' => "ultimate",
        'f' => "fully",
        'm' => "marginal",
        'n' => "never",
        _ => "undefined",
    }
}

/// 署名した鍵（主鍵）について gpg から読んだこと。
#[derive(Debug, Default)]
struct KeyInfo {
    /// 鍵の有効性（`pub` の行の 2 番目の欄。u・f・m・n・`-`・q など）。
    validity: char,
    /// ユーザー ID のメールアドレスと、その有効性。
    emails: Vec<SignerEmail>,
}

/// `gpg --list-keys --with-colons --fixed-list-mode` を読み、主鍵の指紋ごとに鍵の有効性と、ユーザー ID の
/// メールアドレスと有効性を返す。ユーザー ID の `:` は `\x3a` と書かれる。
fn parse_key_uids(out: &str) -> std::collections::HashMap<String, KeyInfo> {
    let mut keys: std::collections::HashMap<String, KeyInfo> = Default::default();
    let mut validity = '-';
    let mut current: Option<String> = None;
    let mut want_fpr = false;
    for line in out.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        match fields.first().copied() {
            Some("pub") => {
                current = None;
                want_fpr = true;
                validity = fields.get(1).and_then(|v| v.chars().next()).unwrap_or('-');
            }
            Some("sub") | Some("ssb") => want_fpr = false,
            Some("fpr") if want_fpr => {
                let fpr = fields.get(9).copied().unwrap_or_default().to_owned();
                keys.entry(fpr.clone()).or_default().validity = validity;
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
                    .emails
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
        let key = &keys["4880B8C9BD0E5106FC070F4F7B3C391EFEA93624"];
        assert_eq!(trust_from_validity(key.validity), "undefined");
        let emails = &key.emails;
        assert_eq!(emails.len(), 3, "副鍵の指紋は主鍵にしない");
        assert_eq!(
            (emails[1].email.as_str(), emails[1].validity),
            ("brauner@kernel.org", 'f')
        );
        assert_eq!(emails[2].validity, 'r');
        assert!(!keys.contains_key("408734571EA70C78B332692891C61BC06578DCA2"));
        let full = out.replace("pub:-:4096", "pub:f:4096");
        let keys = parse_key_uids(&full);
        assert_eq!(
            trust_from_validity(keys["4880B8C9BD0E5106FC070F4F7B3C391EFEA93624"].validity),
            "fully"
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

    /// SSH で署名したコミットは、git 2.34 より前の git では確かめずに V にする（頼むと git が止まり、まとめて
    /// 頼んだほかのコミットの結果も notes も返らない）。新しい git では git の結果のまま。
    #[test]
    fn leaves_ssh_signatures_to_git_that_can_check_them() {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new(tmp.path());
        // notes を足すのに作者が要る（CI には利用者名がない）。
        for args in [
            &["init", "-q"][..],
            &["config", "user.name", "a"],
            &["config", "user.email", "a@example.com"],
        ] {
            git.write(args).unwrap();
        }
        let tree = String::from_utf8(git.write_with_input(&["mktree"], b"").unwrap()).unwrap();
        let commit = |message: &str, signature: &str| {
            let raw = format!(
                "tree {}\nauthor a <a@example.com> 1 +0000\ncommitter a <a@example.com> 1 +0000\n{signature}\n{message}\n",
                tree.trim()
            );
            let id = git
                .write_with_input(
                    &["hash-object", "-t", "commit", "-w", "--stdin"],
                    raw.as_bytes(),
                )
                .unwrap();
            Oid::from_hex(String::from_utf8(id).unwrap().trim()).unwrap()
        };
        let plain = commit("plain", "");
        let ssh = commit(
            "ssh",
            "gpgsig -----BEGIN SSH SIGNATURE-----\n U1NIU0lH\n -----END SSH SIGNATURE-----\n",
        );
        git.write(&["notes", "add", "-m", "a note", &ssh.to_string()])
            .unwrap();
        let old = !crate::git_at_least(2, 34);

        let checks = git.verify_signatures(&[plain, ssh, plain]).unwrap();
        assert_eq!(checks[0].status, 'N');
        assert_eq!(checks[2].status, 'N');
        assert_eq!(checks[1].status == SignatureCheck::UNSUPPORTED, old);
        let extras = git.commit_extras(&ssh).unwrap();
        assert_eq!(extras.notes, "a note");
        let status = extras.signature.map(|s| s.status);
        assert_eq!(status == Some(SignatureCheck::UNSUPPORTED), old);
        let details = git.signature_details(&ssh).unwrap().map(|s| s.status);
        assert_eq!(details == Some(SignatureCheck::UNSUPPORTED), old);
    }

    #[test]
    fn reads_signature_kinds_from_cat_file() {
        let out = b"a commit 59\ntree t\ngpgsig -----BEGIN SSH SIGNATURE-----\n x\n\nmsg gpgsig\n\nb missing\nc commit 21\ntree t\n\ngpgsig SSH x\n\n";
        assert_eq!(signature_kinds(out), [CommitText::SIGNED_SSH, 0, 0]);
    }

    /// SSH で署名したタグ（試験用の鍵で作ったもの）と、その公開鍵。
    const SSH_TAG: &str = "object 26e76af3ea3bc42b40e826a4619f1bb69b865cdd\ntype commit\ntag v1\ntagger T <t@example.com> 1793119458 +0900\n\nsigned\n-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgxE0ba12VuxZC4s+CtBauTHQy59\nIJJBx9mxsyUe/mOd4AAAADZ2l0AAAAAAAAAAZzaGE1MTIAAABTAAAAC3NzaC1lZDI1NTE5\nAAAAQGI12MW85RV+KV1lKCNoNN7FNdt83KxIGGtcHfsox2SUE6OrmG/W8r58Ko9f/WRm44\nXYp+tH3VSltZ1bIBVR6wc=\n-----END SSH SIGNATURE-----\n";
    const SSH_TAG_ID: &str = "9b82bccbdc45d4f48b592537fe4573be9270992a";
    const SSH_TAG_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMRNG2tdlbsWQuLPgrQWrkx0MufSCSQcfZsbMlHv5jne test";

    /// リポジトリにないタグ（merge したタグ）も、一時的な置き場に書いて検証する。SSH の署名は git 2.34 から
    /// なので、古い git では V。署名のないタグは git を起動せずに N。
    #[test]
    fn verifies_tags_that_are_not_in_the_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new(tmp.path().join("repo"));
        std::fs::create_dir(tmp.path().join("repo")).unwrap();
        git.write(&["init", "-q"]).unwrap();
        let allowed = tmp.path().join("allowed_signers");
        std::fs::write(&allowed, format!("t@example.com {SSH_TAG_KEY}\n")).unwrap();
        git.write(&[
            "config",
            "gpg.ssh.allowedSignersFile",
            allowed.to_str().unwrap(),
        ])
        .unwrap();
        let signed = Oid::from_hex(SSH_TAG_ID).unwrap();
        let changed = SSH_TAG.replace("\nsigned\n", "\nchanged\n");
        let changed_id = Oid::from_hex(
            String::from_utf8(
                git.write_with_input(&["hash-object", "-t", "tag", "--stdin"], changed.as_bytes())
                    .unwrap(),
            )
            .unwrap()
            .trim(),
        )
        .unwrap();
        let unsigned = "object 26e76af3ea3bc42b40e826a4619f1bb69b865cdd\ntype commit\ntag v0\ntagger T <t@example.com> 1 +0000\n\nplain\n";
        let checks = git
            .verify_tags(&[
                (signed, SSH_TAG.as_bytes()),
                (changed_id, changed.as_bytes()),
                (Oid([0; 20]), unsigned.as_bytes()),
                // 中身が ID と合わない。
                (Oid([1; 20]), SSH_TAG.as_bytes()),
            ])
            .unwrap();
        let statuses: Vec<char> = checks.iter().map(|c| c.status).collect();
        if crate::git_at_least(2, 34) {
            assert_eq!(statuses, ['G', 'B', 'N', 'E']);
            assert_eq!(checks[0].signer, "t@example.com");
            assert!(checks[0].key.starts_with("SHA256:"));
            assert_eq!(checks[0].emails[0].email, "t@example.com");
        } else {
            assert_eq!(statuses, ['V', 'V', 'N', 'V']);
        }
        // リポジトリにオブジェクトを増やさない。
        assert!(git.read(&["cat-file", "-e", SSH_TAG_ID]).is_err());
    }

    #[test]
    fn reads_verify_tag_output() {
        let gpg = "[GNUPG:] NEWSIG torvalds@example.org\n[GNUPG:] KEY_CONSIDERED ABAF 0\n[GNUPG:] GOODSIG 79BE3E4300411886 Linus Torvalds <torvalds@example.org>\n[GNUPG:] VALIDSIG ABAF11C65A2970B130ABE3C479BE3E4300411886 2026-10-25 1 0 4 0 1 10 00 ABAF11C65A2970B130ABE3C479BE3E4300411886\n[GNUPG:] TRUST_FULLY 0 pgp\n";
        let (check, primary) = parse_verify_tag(gpg);
        assert_eq!(
            (check.status, check.key.as_str(), check.trust.as_str()),
            ('G', "79BE3E4300411886", "fully")
        );
        assert_eq!(check.signer, "Linus Torvalds <torvalds@example.org>");
        assert_eq!(
            check.fingerprint,
            "ABAF11C65A2970B130ABE3C479BE3E4300411886"
        );
        assert_eq!(primary, "ABAF11C65A2970B130ABE3C479BE3E4300411886");
        let untrusted = gpg.replace("TRUST_FULLY", "TRUST_UNDEFINED");
        assert_eq!(parse_verify_tag(&untrusted).0.status, 'U');
        let bad = "[GNUPG:] BADSIG 1B9A0874E06A27C8 Alice <alice@example.com>\n[GNUPG:] FAILURE gpg-exit 33554433\n";
        assert_eq!(parse_verify_tag(bad).0.status, 'B');
        let no_key = "[GNUPG:] ERRSIG 79BE3E4300411886 1 10 00 1791146725 9 ABAF11C65A2970B130ABE3C479BE3E4300411886\n[GNUPG:] NO_PUBKEY 79BE3E4300411886\n";
        let (check, _) = parse_verify_tag(no_key);
        assert_eq!(
            (check.status, check.key.as_str()),
            ('E', "79BE3E4300411886")
        );
        assert_eq!(
            check.fingerprint,
            "ABAF11C65A2970B130ABE3C479BE3E4300411886"
        );
        let expired_key = "[GNUPG:] EXPKEYSIG 1B9A0874E06A27C8 Alice <a@example.com>\n";
        assert_eq!(parse_verify_tag(expired_key).0.status, 'Y');

        let ssh = "Good \"git\" signature for alice@example.com,a@example.org with ED25519 key SHA256:6eaz\n";
        let (check, _) = parse_verify_tag(ssh);
        assert_eq!(
            (check.status, check.signer.as_str(), check.key.as_str()),
            ('G', "alice@example.com,a@example.org", "SHA256:6eaz")
        );
        let unknown =
            "Good \"git\" signature with ED25519 key SHA256:Fp41\nNo principal matched.\n";
        let (check, _) = parse_verify_tag(unknown);
        assert_eq!(
            (check.status, check.signer.as_str(), check.key.as_str()),
            ('U', "", "SHA256:Fp41")
        );
        let changed =
            "Could not verify signature.\nSignature verification failed: incorrect signature\n";
        assert_eq!(parse_verify_tag(changed).0.status, 'B');
        let unconfigured = "error: gpg.ssh.allowedSignersFile needs to be configured and exist for ssh signature verification\n";
        assert_eq!(parse_verify_tag(unconfigured).0.status, 'E');
        assert_eq!(
            parse_verify_tag("error: no signature found\n").0.status,
            'N'
        );
    }
}
