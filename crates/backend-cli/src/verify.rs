//! daemon 越しのリポジトリのコミットの署名を、手元の鍵で検証する。受け取ったコミットのオブジェクトを手元の
//! 作業用のリポジトリに書き、手元の git に `%G?` で検証させる（手元のリポジトリと同じ規則・設定になる。
//! gpg.program・gpg.ssh.allowedSignersFile・gpg.minTrustLevel など）。木やほかのオブジェクトは要らない。

use std::path::PathBuf;
use std::sync::Mutex;

use amonhen_model::{Oid, Result, SignatureCheck, SignatureVerifier};

use crate::Git;

pub struct LocalVerifier {
    /// 作業用の bare リポジトリ（中身はコミットのオブジェクトだけ。同じ ID は 1 回だけ書く）。
    dir: PathBuf,
    ready: Mutex<bool>,
}

impl LocalVerifier {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            ready: Mutex::new(false),
        }
    }

    fn git(&self) -> Result<Git> {
        let mut ready = self.ready.lock().unwrap();
        if !*ready {
            if !self.dir.join("HEAD").exists() {
                std::fs::create_dir_all(&self.dir)?;
                Git::new(&self.dir).write(&["init", "--bare", "-q", "."])?;
            }
            *ready = true;
        }
        Ok(Git::new(&self.dir))
    }

    /// コミットのオブジェクトをまとめて書く（git は 1 回だけ起動する）。書いた ID が渡された ID と違えば
    /// （中身が壊れている）断る。
    fn store(&self, git: &Git, commits: &[(Oid, &[u8])]) -> Result<()> {
        // 同時に検証することがある（ウィンドウごと）ので、呼ぶたびに別のフォルダにする。
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let scratch = self
            .dir
            .join(format!("incoming-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&scratch)?;
        let result = (|| -> Result<()> {
            let mut paths = String::new();
            for (i, (_, raw)) in commits.iter().enumerate() {
                let path = scratch.join(i.to_string());
                std::fs::write(&path, raw)?;
                paths.push_str(&format!("{}\n", path.display()));
            }
            let written = git.write_with_input(
                &[
                    "hash-object",
                    "-t",
                    "commit",
                    "-w",
                    "--literally",
                    "--stdin-paths",
                ],
                paths.as_bytes(),
            )?;
            let written = String::from_utf8_lossy(&written);
            let mut lines = written.lines();
            for (id, _) in commits {
                if lines.next().map(str::trim) != Some(id.to_string().as_str()) {
                    return Err(format!("コミット {id} の中身が ID と合わない").into());
                }
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&scratch);
        result
    }
}

impl SignatureVerifier for LocalVerifier {
    fn verify(&self, commits: &[(Oid, &[u8])]) -> Result<Vec<SignatureCheck>> {
        let git = self.git()?;
        self.store(&git, commits)?;
        let ids: Vec<Oid> = commits.iter().map(|(id, _)| *id).collect();
        git.verify_signatures(&ids)
    }

    fn details(&self, id: &Oid, raw: &[u8]) -> Result<Option<SignatureCheck>> {
        let git = self.git()?;
        self.store(&git, &[(*id, raw)])?;
        git.signature_details(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_a_commit_copied_from_another_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let source = Git::new(tmp.path().join("source"));
        std::fs::create_dir(tmp.path().join("source")).unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "a"],
            &["config", "user.email", "a@example.com"],
            &["config", "commit.gpgsign", "false"],
        ] {
            source.write(args).unwrap();
        }
        source
            .write(&["commit", "-q", "--allow-empty", "-m", "first"])
            .unwrap();
        let hex = String::from_utf8(source.read(&["rev-parse", "HEAD"]).unwrap()).unwrap();
        let id = Oid::from_hex(hex.trim()).unwrap();
        let raw = source.read(&["cat-file", "commit", "HEAD"]).unwrap();

        let verifier = LocalVerifier::new(tmp.path().join("verify.git"));
        assert_eq!(verifier.verify(&[(id, &raw)]).unwrap()[0].status, 'N');
        assert_eq!(verifier.details(&id, &raw).unwrap(), None);
        // 中身が ID と合わなければ断る。
        assert!(verifier.verify(&[(id, b"tree 0\n\nbroken\n")]).is_err());
    }
}
