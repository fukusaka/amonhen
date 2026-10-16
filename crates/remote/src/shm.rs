//! 手元の共有の daemon とのあいだで、大きなバイト列（差分のファイルの中身）を共有メモリで渡す（POSIX の
//! `shm_open`）。daemon は 1 つの領域にまとめて書き、ソケットでは領域の名前と各部分の位置だけを送る。受け取った
//! 側は領域を読むだけで自分のメモリに見せ（コピーしない）、開いたらすぐ名前を消す。受け取った側が開かずに
//! 終わったときのため、daemon も少し後に名前を消す。

use std::ffi::CString;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use amonhen_layout::{Blob, ByteSource};

/// 受け取った側が開かなかったとき、daemon が名前を消すまでの時間。
const UNLINK_AFTER: Duration = Duration::from_secs(30);

/// 読むだけの共有メモリの領域。手放すと領域を外す。
pub struct Region {
    ptr: *const u8,
    len: usize,
}

// 領域は読むだけで、手放すまで外さない。
unsafe impl Send for Region {}
unsafe impl Sync for Region {}

impl AsRef<[u8]> for Region {
    fn as_ref(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: `ptr` から `len` バイトは、手放すまで外さない読むだけの領域。
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        if self.len > 0 {
            // SAFETY: `open` で `len` バイトを写した領域。
            unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
        }
    }
}

/// 領域の名前（macOS は 31 文字まで）。
fn next_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "/amh{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// `parts` を 1 つの領域に並べて書き、領域の名前と各部分の（始め、終わり）を返す。
pub fn publish(parts: &[Blob]) -> io::Result<(String, Vec<(u64, u64)>)> {
    let total: usize = parts.iter().map(|p| p.len()).sum();
    let name = next_name();
    let cname = CString::new(name.clone()).map_err(io::Error::other)?;
    // SAFETY: NUL 終端の名前と、フラグ・権限を渡すだけ。
    let fd = unsafe {
        libc::shm_open(
            cname.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
            0o600 as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let unlink = || {
        // SAFETY: 名前を消すだけ。
        unsafe { libc::shm_unlink(cname.as_ptr()) };
    };
    let size = total.max(1);
    // SAFETY: 開いた fd の大きさを決め、書ける領域として写す。
    let result = unsafe {
        if libc::ftruncate(fd, size as libc::off_t) != 0 {
            Err(io::Error::last_os_error())
        } else {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            if ptr == libc::MAP_FAILED {
                Err(io::Error::last_os_error())
            } else {
                let mut offsets = Vec::with_capacity(parts.len());
                let mut at = 0usize;
                for part in parts {
                    std::ptr::copy_nonoverlapping(
                        part.as_ptr(),
                        (ptr as *mut u8).add(at),
                        part.len(),
                    );
                    offsets.push((at as u64, (at + part.len()) as u64));
                    at += part.len();
                }
                libc::munmap(ptr, size);
                Ok(offsets)
            }
        }
    };
    // SAFETY: 開いた fd を閉じるだけ（領域は名前で残る）。
    unsafe { libc::close(fd) };
    match result {
        Ok(offsets) => {
            // 受け取った側が開かなかったときのために、少し後に名前を消す（開いていれば、もう消えている）。
            let late = name.clone();
            std::thread::spawn(move || {
                std::thread::sleep(UNLINK_AFTER);
                if let Ok(c) = CString::new(late) {
                    // SAFETY: 名前を消すだけ。
                    unsafe { libc::shm_unlink(c.as_ptr()) };
                }
            });
            Ok((name, offsets))
        }
        Err(e) => {
            unlink();
            Err(e)
        }
    }
}

/// `name` の領域を読むだけで写し、名前を消す。
pub fn open(name: &str) -> io::Result<Arc<Region>> {
    let cname = CString::new(name).map_err(io::Error::other)?;
    // SAFETY: NUL 終端の名前で、読むだけで開く。
    let fd = unsafe { libc::shm_open(cname.as_ptr(), libc::O_RDONLY, 0 as libc::c_uint) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: 開いた fd の大きさを調べ、読むだけの領域として写し、fd を閉じて名前を消す。
    unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        let result = if libc::fstat(fd, &mut stat) != 0 {
            Err(io::Error::last_os_error())
        } else {
            let len = stat.st_size as usize;
            let ptr = if len == 0 {
                std::ptr::null_mut()
            } else {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            if ptr == libc::MAP_FAILED {
                Err(io::Error::last_os_error())
            } else {
                Ok(Arc::new(Region {
                    ptr: ptr as *const u8,
                    len,
                }))
            }
        };
        libc::close(fd);
        libc::shm_unlink(cname.as_ptr());
        result
    }
}

/// 領域の中の各部分を、コピーせずに指すバイト列にする。
pub fn parts(region: Arc<Region>, offsets: &[(u64, u64)]) -> io::Result<Vec<Blob>> {
    let len = (*region).as_ref().len() as u64;
    let source: ByteSource = region;
    offsets
        .iter()
        .map(|&(start, end)| {
            if start > end || end > len {
                return Err(io::Error::other("共有メモリの範囲が領域の外"));
            }
            Ok(Blob::part(source.clone(), start as usize..end as usize))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_parts_over_through_shared_memory() {
        let parts = vec![
            Blob::from(b"hello".to_vec()),
            Blob::default(),
            Blob::from(b"world!".to_vec()),
        ];
        let (name, offsets) = publish(&parts).unwrap();
        let region = open(&name).unwrap();
        let got = super::parts(region, &offsets).unwrap();
        assert_eq!(got, parts);
        // 開いたら名前は消える。
        assert!(open(&name).is_err());
    }
}
