use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use memmap2::Mmap;

/// バイト列をそのまま読んでよい型（どのビット列も正しい値で、参照やパディングの罠がない）。
///
/// # Safety
/// `#[repr(C)]` か基本型で、すべてのビット列が有効な値であること。
pub unsafe trait Pod: Copy + 'static {}

unsafe impl Pod for u8 {}
unsafe impl Pod for u16 {}
unsafe impl Pod for u32 {}
unsafe impl Pod for u64 {}
unsafe impl Pod for i64 {}
unsafe impl Pod for [u8; 20] {}

/// 不変の配列。計算した結果は `Vec`、永続キャッシュから読んだものは mmap した領域をそのまま指す。
pub struct Slab<T: Pod>(Inner<T>);

enum Inner<T> {
    Owned(Vec<T>),
    Mapped {
        map: Arc<Mmap>,
        offset: usize,
        len: usize,
    },
}

impl<T: Pod> Slab<T> {
    /// `map` の `offset` バイト目から `len` 個。範囲外か、`T` の境界に揃っていなければ `None`。
    pub fn mapped(map: Arc<Mmap>, offset: usize, len: usize) -> Option<Self> {
        let bytes = len.checked_mul(size_of::<T>())?;
        let end = offset.checked_add(bytes)?;
        let aligned = (map.as_ptr() as usize + offset).is_multiple_of(align_of::<T>());
        (end <= map.len() && aligned).then_some(Self(Inner::Mapped { map, offset, len }))
    }

    pub fn as_bytes(&self) -> &[u8] {
        let s: &[T] = self;
        unsafe { std::slice::from_raw_parts(s.as_ptr().cast(), std::mem::size_of_val(s)) }
    }
}

impl<T: Pod> Deref for Slab<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        match &self.0 {
            Inner::Owned(v) => v,
            // `mapped` で範囲と境界を確かめ、`T: Pod` なのでどのビット列も有効。
            Inner::Mapped { map, offset, len } => unsafe {
                std::slice::from_raw_parts(map.as_ptr().add(*offset).cast(), *len)
            },
        }
    }
}

impl<T: Pod> From<Vec<T>> for Slab<T> {
    fn from(v: Vec<T>) -> Self {
        Self(Inner::Owned(v))
    }
}

impl<T: Pod> Default for Slab<T> {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl<T: Pod> Clone for Slab<T> {
    fn clone(&self) -> Self {
        match &self.0 {
            Inner::Owned(v) => Self(Inner::Owned(v.clone())),
            Inner::Mapped { map, offset, len } => Self(Inner::Mapped {
                map: map.clone(),
                offset: *offset,
                len: *len,
            }),
        }
    }
}

impl<T: Pod + PartialEq> PartialEq for Slab<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Pod + Eq> Eq for Slab<T> {}

impl<T: Pod + fmt::Debug> fmt::Debug for Slab<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn mapped_slabs_read_the_file_in_place_and_check_bounds() {
        let mut file = tempfile::tempfile().unwrap();
        let values: Vec<u32> = (0..10).collect();
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        file.write_all(&bytes).unwrap();
        let map = Arc::new(unsafe { Mmap::map(&file) }.unwrap());

        let slab = Slab::<u32>::mapped(map.clone(), 8, 4).unwrap();
        assert_eq!(&*slab, &[2, 3, 4, 5]);
        assert_eq!(slab.as_ptr().cast::<u8>(), unsafe { map.as_ptr().add(8) });
        assert_eq!(slab, Slab::from(vec![2, 3, 4, 5]));

        assert!(Slab::<u32>::mapped(map.clone(), 8, 9).is_none(), "範囲外");
        assert!(
            Slab::<u32>::mapped(map, 2, 1).is_none(),
            "境界に揃っていない"
        );
    }
}
