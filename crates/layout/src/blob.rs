//! 不変のバイト列（ファイルの中身やコミットの本文）。ふつうはメモリ上の配列だが、手元の共有の daemon から受け取った
//! ものは共有メモリの領域の一部を指す（コピーしない）。どちらも同じように `&[u8]` として読める。

use std::fmt;
use std::ops::{Deref, Range};
use std::sync::Arc;

/// バイト列を持つもの（配列・共有メモリの領域など）。
pub type ByteSource = Arc<dyn AsRef<[u8]> + Send + Sync>;

#[derive(Clone)]
pub struct Blob {
    source: ByteSource,
    range: Range<usize>,
}

impl Blob {
    /// `source` の `range` の部分（共有メモリの 1 つの領域に並べた、ファイルごとの中身など）。
    pub fn part(source: ByteSource, range: Range<usize>) -> Self {
        assert!(range.start <= range.end && range.end <= (*source).as_ref().len());
        Self { source, range }
    }
}

impl Deref for Blob {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &(*self.source).as_ref()[self.range.clone()]
    }
}

impl AsRef<[u8]> for Blob {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl From<Arc<Vec<u8>>> for Blob {
    fn from(v: Arc<Vec<u8>>) -> Self {
        let len = v.len();
        Self {
            source: v,
            range: 0..len,
        }
    }
}

impl From<Vec<u8>> for Blob {
    fn from(v: Vec<u8>) -> Self {
        Arc::new(v).into()
    }
}

impl Default for Blob {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl PartialEq for Blob {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl fmt::Debug for Blob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Blob({} bytes)", self.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_part_of_a_shared_source() {
        let source: ByteSource = Arc::new(b"helloworld".to_vec());
        let world = Blob::part(source.clone(), 5..10);
        assert_eq!(&*world, b"world");
        assert_eq!(
            Blob::from(b"abc".to_vec()),
            Blob::from(Arc::new(b"abc".to_vec()))
        );
        assert!(Blob::default().is_empty());
    }
}
