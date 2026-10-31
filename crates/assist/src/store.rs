//! 利用者のプロンプト（`<dir>/<用途>.md`）、リポジトリのプロンプト（`.amonhen/prompts/<用途>.md`）、返事の
//! キャッシュ。置き場所は FE が決めて渡す（macOS 版は `~/Library/Application Support/Amon Hen/prompts`）。

use std::path::{Path, PathBuf};

use crate::Purpose;

/// 利用者のプロンプトの置き場所。書き換えたものだけファイルにする。
pub struct Prompts {
    pub dir: PathBuf,
}

impl Prompts {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn file(&self, purpose: Purpose) -> PathBuf {
        self.dir.join(format!("{}.md", purpose.id()))
    }

    /// 利用者のプロンプト（書き換えていなければ組み込みのもの）。
    pub fn user_prompt(&self, purpose: Purpose) -> String {
        std::fs::read_to_string(self.file(purpose))
            .unwrap_or_else(|_| purpose.default_prompt().to_owned())
    }

    pub fn is_customized(&self, purpose: Purpose) -> bool {
        self.file(purpose).exists()
    }

    /// 書き換えるためにファイルを作り（なければ組み込みのものを書く）、その場所を返す。
    pub fn editable_file(&self, purpose: Purpose) -> std::io::Result<PathBuf> {
        let file = self.file(purpose);
        if !file.exists() {
            std::fs::create_dir_all(&self.dir)?;
            std::fs::write(&file, purpose.default_prompt())?;
        }
        Ok(file)
    }

    /// 書き換えたものを保存する（組み込みのものと同じなら、ファイルを消して既定に戻す）。
    pub fn save(&self, purpose: Purpose, text: &str) -> std::io::Result<()> {
        if text == purpose.default_prompt() {
            self.reset(purpose);
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.file(purpose), text)
    }

    pub fn reset(&self, purpose: Purpose) {
        let _ = std::fs::remove_file(self.file(purpose));
    }
}

/// リポジトリの `.amonhen/prompts/<用途>.md`（あれば利用者のものより優先する）。パスと中身。
pub fn repository_prompt(workdir: &Path, purpose: Purpose) -> Option<(PathBuf, String)> {
    let path = workdir
        .join(".amonhen/prompts")
        .join(format!("{}.md", purpose.id()));
    std::fs::read_to_string(&path).ok().map(|text| (path, text))
}

/// 返事のキャッシュの鍵: CLI・モデル・用途・送るプロンプト（コミットの中身・言語・プロンプトの書き換えを含む）の
/// ハッシュ（macOS 版と同じ作り）。
pub fn cache_key(cli: &str, model: &str, purpose: Purpose, prompt: &str) -> String {
    sha256_hex([cli, model, purpose.id(), prompt].join("\0").as_bytes())
}

/// 返事のキャッシュ（`<dir>/<鍵>.txt`）。
pub struct Cache {
    pub dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.join(format!("{key}.txt"))).ok()
    }

    pub fn put(&self, key: &str, text: &str) {
        let _ = std::fs::create_dir_all(&self.dir);
        let path = self.dir.join(format!("{key}.txt"));
        let temp = self.dir.join(format!("{key}.tmp"));
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
    }
}

/// SHA-256 の 16 進（リポジトリのプロンプトの許可と、キャッシュの鍵に使う）。
pub fn sha256_hex(data: &[u8]) -> String {
    sha256(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_values() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 2 ブロックにまたがる長さ。
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn user_prompts_fall_back_to_the_default_and_repository_ones_win() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = Prompts::new(dir.path().join("prompts"));
        let p = Purpose::Translate;
        assert!(!prompts.is_customized(p));
        assert_eq!(prompts.user_prompt(p), p.default_prompt());
        let file = prompts.editable_file(p).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), p.default_prompt());
        prompts.save(p, "mine {{message}}").unwrap();
        assert_eq!(prompts.user_prompt(p), "mine {{message}}");
        // 既定と同じ中身で保存したら、書き換えていないことにする。
        prompts.save(p, p.default_prompt()).unwrap();
        assert!(!prompts.is_customized(p));

        let repo = dir.path().join("repo");
        assert!(repository_prompt(&repo, p).is_none());
        std::fs::create_dir_all(repo.join(".amonhen/prompts")).unwrap();
        std::fs::write(repo.join(".amonhen/prompts/translate.md"), "team").unwrap();
        let (path, text) = repository_prompt(&repo, p).unwrap();
        assert!(path.ends_with(".amonhen/prompts/translate.md"));
        assert_eq!(text, "team");
    }

    #[test]
    fn the_cache_key_changes_with_every_part() {
        let base = cache_key("claude", "m", Purpose::Translate, "p");
        assert_eq!(base.len(), 64);
        assert_eq!(base, cache_key("claude", "m", Purpose::Translate, "p"));
        for other in [
            cache_key("codex", "m", Purpose::Translate, "p"),
            cache_key("claude", "n", Purpose::Translate, "p"),
            cache_key("claude", "m", Purpose::ExplainCommit, "p"),
            cache_key("claude", "m", Purpose::Translate, "q"),
        ] {
            assert_ne!(base, other);
        }
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().join("assist"));
        assert!(cache.get(&base).is_none());
        cache.put(&base, "訳");
        assert_eq!(cache.get(&base).as_deref(), Some("訳"));
    }
}
