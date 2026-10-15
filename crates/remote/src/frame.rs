//! ストリームの上のフレーム。`長さ（u32）・種類（u8）・番号（u64）・中身` を並べる。長さは種類から後ろ。

use std::io::{self, Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Request = 1,
    /// 成功の応答。中身は要求ごとの型を postcard で書いたもの。
    Ok = 2,
    /// 失敗の応答。中身はエラーの文（UTF-8）。
    Err = 3,
    /// daemon からの知らせ（リポジトリの変化など）。番号は使わない。
    Notify = 4,
    /// 番号の要求を取り消す（パス・内容の検索など）。
    Cancel = 5,
}

impl Kind {
    fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::Request,
            2 => Self::Ok,
            3 => Self::Err,
            4 => Self::Notify,
            5 => Self::Cancel,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub struct Frame {
    pub kind: Kind,
    pub id: u64,
    pub payload: Vec<u8>,
}

/// 1 つのフレームの上限（壊れた入力で巨大な領域を取らないため）。
const MAX_FRAME: u32 = 1 << 30;

pub fn write(out: &mut impl Write, kind: Kind, id: u64, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(1 + 8 + payload.len())
        .map_err(|_| io::Error::other("フレームが大きすぎる"))?;
    let mut head = [0u8; 13];
    head[..4].copy_from_slice(&len.to_le_bytes());
    head[4] = kind as u8;
    head[5..].copy_from_slice(&id.to_le_bytes());
    out.write_all(&head)?;
    out.write_all(payload)?;
    out.flush()
}

/// 次のフレーム。相手が閉じたら `None`。
pub fn read(input: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut len = [0u8; 4];
    match input.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len);
    if !(9..=MAX_FRAME).contains(&len) {
        return Err(io::Error::other(format!("フレームの長さがおかしい: {len}")));
    }
    let mut head = [0u8; 9];
    input.read_exact(&mut head)?;
    let kind = Kind::from_u8(head[0]).ok_or_else(|| io::Error::other("知らないフレームの種類"))?;
    let id = u64::from_le_bytes(head[1..].try_into().unwrap());
    let mut payload = vec![0u8; len as usize - 9];
    input.read_exact(&mut payload)?;
    Ok(Some(Frame { kind, id, payload }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ends_cleanly() {
        let mut buf = Vec::new();
        write(&mut buf, Kind::Request, 7, b"abc").unwrap();
        write(&mut buf, Kind::Notify, 0, b"").unwrap();
        let mut input = &buf[..];
        let a = read(&mut input).unwrap().unwrap();
        assert_eq!(
            (a.kind, a.id, a.payload.as_slice()),
            (Kind::Request, 7, &b"abc"[..])
        );
        let b = read(&mut input).unwrap().unwrap();
        assert_eq!((b.kind, b.payload.len()), (Kind::Notify, 0));
        assert!(read(&mut input).unwrap().is_none());
        // 壊れた長さは読まない。
        let mut bad = &[0xff, 0xff, 0xff, 0xff][..];
        assert!(read(&mut bad).is_err());
    }
}
