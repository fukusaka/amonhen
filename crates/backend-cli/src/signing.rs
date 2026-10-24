//! 署名の鍵を集める（リポジトリの設定の「作者と署名」の「鍵」の一覧）。OpenPGP は gpg、SSH は `~/.ssh/*.pub` と
//! ssh-agent、X.509 は gpgsm。プログラムがなければ空（欄には「その他…」で入れられる）。

use std::path::{Path, PathBuf};

use amonhen_model::{Result, SigningKey};

use crate::Git;

impl Git {
    pub(crate) fn signing_keys_for(&self, format: &str) -> Result<Vec<SigningKey>> {
        match format {
            "openpgp" => Ok(parse_gpg_keys(&self.list_keys(
                &self.program("gpg.openpgp.program", "gpg"),
                "--list-secret-keys",
            ))),
            "ssh" => Ok(ssh_keys()),
            "x509" => Ok(parse_gpg_keys(&self.list_keys(
                &self.program("gpg.x509.program", "gpgsm"),
                "--list-secret-keys",
            ))),
            other => {
                Err(amonhen_model::tr!("Unknown signature format: {format}", format = other).into())
            }
        }
    }

    /// `key`（`gpg.openpgp.program` など）で設定されたプログラム。なければ `gpg.program`、それもなければ `fallback`。
    fn program(&self, key: &str, fallback: &str) -> String {
        for k in [key, "gpg.program"] {
            if let Some(p) = self
                .read(&["config", "--get", k])
                .ok()
                .map(|p| String::from_utf8_lossy(&p).trim().to_owned())
                .filter(|p| !p.is_empty())
            {
                return p;
            }
        }
        fallback.to_owned()
    }

    /// gpg か gpgsm の `--with-colons` の一覧。動かせなければ空。
    fn list_keys(&self, program: &str, what: &str) -> String {
        amonhen_model::process::command(program)
            .args([
                "--batch",
                "--no-tty",
                "--with-colons",
                "--fixed-list-mode",
                what,
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    }
}

/// gpg（`sec`・`fpr`・`uid`）と gpgsm（`crs`・`fpr`・`uid`）の `--with-colons` の出力から、鍵の指紋と
/// ユーザー ID を読む。失効・期限切れ（有効性 `r`・`e`）は飛ばす。
pub(crate) fn parse_gpg_keys(text: &str) -> Vec<SigningKey> {
    let mut keys: Vec<SigningKey> = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        match f.first().copied() {
            Some("sec" | "crs" | "crt") => {
                skipping = matches!(f.get(1).copied(), Some("r" | "e"));
                if !skipping {
                    keys.push(SigningKey {
                        id: f.get(4).unwrap_or(&"").to_string(),
                        label: String::new(),
                        emails: Vec::new(),
                    });
                }
            }
            Some("ssb" | "sub") => skipping = true,
            Some("fpr") if !skipping => {
                if let (Some(key), Some(fpr)) = (keys.last_mut(), f.get(9)) {
                    // 主鍵の指紋（`sec` の直後）。副鍵の指紋は `ssb` のあとなので来ない。
                    if key.label.is_empty() && key.emails.is_empty() {
                        key.id = (*fpr).to_owned();
                    }
                }
            }
            Some("uid") if !skipping => {
                if let (Some(key), Some(uid)) = (keys.last_mut(), f.get(9)) {
                    if matches!(f.get(1).copied(), Some("r" | "e")) {
                        continue;
                    }
                    if key.label.is_empty() {
                        key.label = (*uid).to_owned();
                    }
                    if let Some(email) = email_in(uid) {
                        key.emails.push(email);
                    }
                }
            }
            _ => {}
        }
    }
    keys.retain(|k| !k.id.is_empty());
    keys
}

/// `Name <me@example.com>` のメールアドレス。
fn email_in(uid: &str) -> Option<String> {
    let start = uid.rfind('<')?;
    let end = uid[start..].find('>')? + start;
    let email = uid[start + 1..end].trim();
    (!email.is_empty() && email.contains('@')).then(|| email.to_owned())
}

/// `~/.ssh/*.pub` の公開鍵と、ssh-agent が持つ鍵（`ssh-add -L`。ファイルにもあるものは除く）。
fn ssh_keys() -> Vec<SigningKey> {
    let mut keys = Vec::new();
    let mut seen = Vec::new();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let dir = PathBuf::from(home).join(".ssh");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "pub"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for path in files {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(key) = ssh_key_from_file(&path, &text) {
                if let Some(blob) = text.split_whitespace().nth(1) {
                    seen.push(blob.to_owned());
                }
                keys.push(key);
            }
        }
    }
    if let Ok(out) = amonhen_model::process::command("ssh-add")
        .arg("-L")
        .output()
        && out.status.success()
    {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let mut parts = line.split_whitespace();
            let (Some(kind), Some(blob)) = (parts.next(), parts.next()) else {
                continue;
            };
            if seen.iter().any(|s| s == blob) {
                continue;
            }
            let comment = parts.collect::<Vec<_>>().join(" ");
            keys.push(SigningKey {
                id: format!("key::{kind} {blob}"),
                label: if comment.is_empty() {
                    kind.to_owned()
                } else {
                    comment.clone()
                } + " (ssh-agent)",
                emails: comment
                    .contains('@')
                    .then(|| comment.clone())
                    .into_iter()
                    .collect(),
            });
        }
    }
    keys
}

/// 公開鍵のファイル 1 つ。`id` はそのパス（git の `user.signingkey` にそのまま書ける）。
pub(crate) fn ssh_key_from_file(path: &Path, text: &str) -> Option<SigningKey> {
    let mut parts = text.split_whitespace();
    let (_kind, _blob) = (parts.next()?, parts.next()?);
    let comment = parts.collect::<Vec<_>>().join(" ");
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(SigningKey {
        id: path.to_string_lossy().into_owned(),
        label: if comment.is_empty() {
            name
        } else {
            format!("{comment} ({name})")
        },
        emails: comment
            .contains('@')
            .then(|| comment.clone())
            .into_iter()
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_gpg_secret_keys_with_their_uids() {
        let text = "\
sec:u:255:22:1111222233334444:1700000000:::u:::scESC:::+:::23::0:
fpr:::::::::AAAA1111222233334444555566667777888899990000:
grp:::::::::0123456789ABCDEF0123456789ABCDEF01234567:
uid:u::::1700000000::HASH::Alice Example <alice@example.com>::::::::::0:
uid:u::::1700000000::HASH::Alice (work) <alice@work.example>::::::::::0:
ssb:u:255:18:5555666677778888:1700000000::::::e:::+:::23:
fpr:::::::::BBBB1111222233334444555566667777888899990000:
sec:e:255:22:9999000011112222:1600000000:1650000000::u:::scESC:::+:::23::0:
fpr:::::::::CCCC1111222233334444555566667777888899990000:
uid:e::::1600000000::HASH::Old <old@example.com>::::::::::0:
";
        let keys = parse_gpg_keys(text);
        assert_eq!(keys.len(), 1, "{keys:?}");
        assert_eq!(keys[0].id, "AAAA1111222233334444555566667777888899990000");
        assert_eq!(keys[0].label, "Alice Example <alice@example.com>");
        assert_eq!(keys[0].emails, ["alice@example.com", "alice@work.example"]);
    }

    #[test]
    fn reads_an_ssh_public_key_file() {
        let key = ssh_key_from_file(
            Path::new("/home/a/.ssh/id_ed25519.pub"),
            "ssh-ed25519 AAAAC3Nza me@example.com\n",
        )
        .unwrap();
        assert_eq!(key.id, "/home/a/.ssh/id_ed25519.pub");
        assert_eq!(key.label, "me@example.com (id_ed25519.pub)");
        assert_eq!(key.emails, ["me@example.com"]);
        let bare =
            ssh_key_from_file(Path::new("/home/a/.ssh/id_rsa.pub"), "ssh-rsa AAAAB3\n").unwrap();
        assert_eq!(bare.label, "id_rsa.pub");
        assert!(bare.emails.is_empty());
        assert!(ssh_key_from_file(Path::new("/x.pub"), "garbage").is_none());
    }
}
