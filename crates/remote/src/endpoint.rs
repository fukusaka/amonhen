//! 手元の共有の daemon の通り道。Unix は Unix ドメインソケット、Windows は名前付きパイプ（git の fsmonitor・
//! VSCode と同じ使い分け）。どちらもバイトの流れで、その上の取り決め（最初に送る場所・フレーム）は同じ。
//!
//! Windows では、パイプの名前に利用者（SID のハッシュ）と通信の形の版を入れ、権限は自分の SID だけにする（何も
//! 指定しないと Everyone に読み取りが付く）。最初の作成は `FILE_FLAG_FIRST_PIPE_INSTANCE`（`interprocess` が付ける）
//! で、ほかの利用者が先に同じ名前で待ち構えることを防ぐ。読み書きは重ねた I/O で、読む側と書く側を分けて同時に
//! 使える（同期式のハンドル 1 つでは、読み取りの待ちが書き込みを止める）。

use std::io::{self, Read, Write};

/// つないだ通り道の、読む側と書く側（別々のスレッドで同時に使える）。
pub struct Duplex {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
}

/// 共有の daemon の通り道の名前（Unix はソケットのパス、Windows は `\\.\pipe\...`）。`amonhen --socket` が出す。
pub fn endpoint() -> String {
    imp::endpoint()
}

/// `endpoint` につなぐ。
pub fn connect(endpoint: &str) -> io::Result<Duplex> {
    imp::connect(endpoint)
}

/// 共有の daemon が待つところ。
pub struct Listener(imp::Listener);

impl Listener {
    /// `endpoint` で待つ。ほかの daemon がもう待っていれば `None`。
    pub fn bind(endpoint: &str) -> io::Result<Option<Self>> {
        Ok(imp::Listener::bind(endpoint)?.map(Self))
    }

    /// 次の相手を待つ。
    pub fn accept(&self) -> io::Result<Duplex> {
        self.0.accept()
    }
}

/// 待つところを片付ける（Unix ではソケットのファイルを消す。Windows では何もしない）。
pub fn remove(endpoint: &str) {
    imp::remove(endpoint)
}

#[cfg(unix)]
mod imp {
    use super::Duplex;
    use std::io;
    use std::os::unix::net::{UnixListener, UnixStream};

    pub fn endpoint() -> String {
        crate::local::socket_path().display().to_string()
    }

    /// Unix ソケットのパスは `sun_path`（macOS は 104 バイト、Linux は 108 バイト。最後の NUL を除く）に収まらないと
    /// 使えない。標準の「path must be shorter than SUN_LEN」では直し方が分からないので、長さと直し方を出す。
    fn fits(endpoint: &str) -> io::Result<()> {
        // SAFETY: ゼロ埋めの sockaddr_un（数と配列だけの構造体）を作って、`sun_path` の大きさを見るだけ。
        let max = unsafe { std::mem::zeroed::<libc::sockaddr_un>() }
            .sun_path
            .len()
            - 1;
        if endpoint.len() <= max {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            amonhen_model::tr!(
                "The shared daemon’s socket path is too long ({length} bytes, at most {max}): {path}. Set AMONHEN_DAEMON_DIR to a shorter folder",
                length = endpoint.len(),
                max = max,
                path = endpoint
            ),
        ))
    }

    pub fn connect(endpoint: &str) -> io::Result<Duplex> {
        fits(endpoint)?;
        let stream = UnixStream::connect(endpoint)?;
        Ok(Duplex {
            reader: Box::new(stream.try_clone()?),
            writer: Box::new(stream),
        })
    }

    pub struct Listener(UnixListener);

    impl Listener {
        pub fn bind(endpoint: &str) -> io::Result<Option<Self>> {
            fits(endpoint)?;
            let path = std::path::Path::new(endpoint);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // もう動いている daemon があれば、それに任せる。
            if UnixStream::connect(path).is_ok() {
                return Ok(None);
            }
            let _ = std::fs::remove_file(path);
            let listener = match UnixListener::bind(path) {
                Ok(l) => l,
                // 同時に起動された別の daemon が先に作った。
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => return Ok(None),
                Err(e) => return Err(e),
            };
            // ソケットは自分だけが読み書きできるようにする。
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            Ok(Some(Self(listener)))
        }

        pub fn accept(&self) -> io::Result<Duplex> {
            let (stream, _) = self.0.accept()?;
            Ok(Duplex {
                reader: Box::new(stream.try_clone()?),
                writer: Box::new(stream),
            })
        }
    }

    pub fn remove(endpoint: &str) {
        let _ = std::fs::remove_file(endpoint);
    }
}

#[cfg(windows)]
mod imp {
    use super::Duplex;
    use interprocess::local_socket::prelude::*;
    use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream};
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use std::io;

    const PIPE_PREFIX: &str = r"\\.\pipe\";

    /// パイプの名前（`\\.\pipe\` を除いた部分）: `amonhen-<SID のハッシュ>-<ビルドの指紋>`。`AMONHEN_DAEMON_DIR` が
    /// あれば、それも混ぜる（試験で手元の daemon と分ける）。
    fn pipe_name() -> String {
        let mut key =
            sid::current().unwrap_or_else(|_| std::env::var("USERNAME").unwrap_or_default());
        if let Some(dir) = std::env::var_os("AMONHEN_DAEMON_DIR") {
            key.push('|');
            key.push_str(&dir.to_string_lossy());
        }
        let hash = sha1_smol::Sha1::from(key.as_bytes()).digest().to_string();
        format!("amonhen-{}-{}", &hash[..16], crate::BUILD_ID)
    }

    pub fn endpoint() -> String {
        format!("{PIPE_PREFIX}{}", pipe_name())
    }

    fn name(endpoint: &str) -> io::Result<interprocess::local_socket::Name<'static>> {
        let short = endpoint
            .strip_prefix(PIPE_PREFIX)
            .unwrap_or(endpoint)
            .to_owned();
        short
            .to_ns_name::<GenericNamespaced>()
            .map(|n| n.into_owned())
    }

    fn duplex(stream: Stream) -> Duplex {
        let (reader, writer) = stream.split();
        Duplex {
            reader: Box::new(reader),
            writer: Box::new(writer),
        }
    }

    pub fn connect(endpoint: &str) -> io::Result<Duplex> {
        Ok(duplex(Stream::connect(name(endpoint)?)?))
    }

    pub struct Listener(interprocess::local_socket::Listener);

    impl Listener {
        pub fn bind(endpoint: &str) -> io::Result<Option<Self>> {
            // もう動いている daemon があれば、それに任せる。
            if Stream::connect(name(endpoint)?).is_ok() {
                return Ok(None);
            }
            // 自分の SID だけに全権を与え、ほかは何も許さない（P: 継承しない）。
            let sddl = format!("D:P(A;;GA;;;{})", sid::current()?);
            let wide = widestring::U16CString::from_str(&sddl).map_err(io::Error::other)?;
            let sd = SecurityDescriptor::deserialize(&wide)?;
            match ListenerOptions::new()
                .name(name(endpoint)?)
                .security_descriptor(sd)
                .create_sync()
            {
                Ok(l) => Ok(Some(Self(l))),
                // 同時に起動された別の daemon が先に作った（最初の作成は FIRST_PIPE_INSTANCE）。
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                    ) =>
                {
                    Ok(None)
                }
                Err(e) => Err(e),
            }
        }

        pub fn accept(&self) -> io::Result<Duplex> {
            Ok(duplex(self.0.accept()?))
        }
    }

    pub fn remove(_endpoint: &str) {}

    /// 今の利用者の SID（`S-1-5-21-...`）。
    mod sid {
        use std::io;
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
        use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows_sys::Win32::Security::{
            GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        pub fn current() -> io::Result<String> {
            // SAFETY: 自分のプロセスのトークンを開き、TokenUser を十分な大きさのバッファに読み、SID を文字列にして
            // 写してから、確保したものを解放する。
            unsafe {
                let mut token: HANDLE = std::ptr::null_mut();
                if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let mut len = 0u32;
                GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
                let mut buf = vec![0u64; (len as usize).div_ceil(8)];
                let ok =
                    GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
                CloseHandle(token);
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                let user = &*(buf.as_ptr() as *const TOKEN_USER);
                let mut wide: *mut u16 = std::ptr::null_mut();
                if ConvertSidToStringSidW(user.User.Sid, &mut wide) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let mut n = 0;
                while *wide.add(n) != 0 {
                    n += 1;
                }
                let sid = String::from_utf16_lossy(std::slice::from_raw_parts(wide, n));
                LocalFree(wide.cast());
                Ok(sid)
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use super::Duplex;
    use std::io;

    pub fn endpoint() -> String {
        String::new()
    }

    pub fn connect(_endpoint: &str) -> io::Result<Duplex> {
        Err(io::Error::other(amonhen_model::tr!(
            "The shared daemon isn’t available in this environment"
        )))
    }

    pub struct Listener;

    impl Listener {
        pub fn bind(_endpoint: &str) -> io::Result<Option<Self>> {
            Err(io::Error::other(amonhen_model::tr!(
                "The shared daemon isn’t available in this environment"
            )))
        }

        pub fn accept(&self) -> io::Result<Duplex> {
            Err(io::Error::other(amonhen_model::tr!(
                "The shared daemon isn’t available in this environment"
            )))
        }
    }

    pub fn remove(_endpoint: &str) {}
}
