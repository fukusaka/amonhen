use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{BuildHasherDefault, Hash, Hasher};

use amonhen_layout::Pod;

/// オブジェクト ID（SHA-1）。SHA-256 のリポジトリに対応するときに見直す。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Oid(pub [u8; 20]);

/// オブジェクト ID はそれ自体が暗号学的ハッシュなので、先頭 8 バイトをそのままハッシュ値にする。
/// 既定の SipHash より速く、100 万件を超える表で効く。
impl Hash for Oid {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(u64::from_le_bytes(self.0[..8].try_into().unwrap()));
    }
}

/// `Oid` の `Hash` が渡す値をそのまま使う。
#[derive(Default)]
pub struct OidHasher(u64);

impl Hasher for OidHasher {
    fn write(&mut self, bytes: &[u8]) {
        // `Oid` 以外のキーに使われたときのための予備。
        for &b in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(b);
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

pub type OidMap<V> = HashMap<Oid, V, BuildHasherDefault<OidHasher>>;
pub type OidSet = HashSet<Oid, BuildHasherDefault<OidHasher>>;

unsafe impl Pod for Oid {}

impl Oid {
    /// 40 桁の 16 進数から。
    pub fn from_hex(hex: &str) -> Option<Self> {
        let bytes = hex.as_bytes();
        if bytes.len() != 40 {
            return None;
        }
        let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        let mut out = [0; 20];
        for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
            out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
        }
        Some(Self(out))
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 20]
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Oid({self})")
    }
}
