//! FE 向けの窓口。コアのすべての働きを、言語を問わず使える形（命令と答えは JSON、大きなデータはレコード）で出す。
//!
//! - 手元の共有の daemon（`amonhend --serve`）は、つないで最初に `PREAMBLE` で始まる場所を送った相手と、この形で
//!   やりとりする。コアは daemon で動き、FE（VSCode 拡張・CLI）はコアを持たない。SSH 先も daemon が開く。
//! - 同じ処理を同じプロセスでも使える（`InProcess`）。CLI はどちらでも同じ命令を通す。
//!
//! 取り決めは `docs/front.md`。

mod client;
mod records;
mod session;

pub use client::{Front, InProcess, Remote, open};
pub use records::Records;
pub use session::Session;

use amonhen_model::tr;
use serde_json::Value;

/// 取り決めの版。変えたら上げる（`hello` の答えで確かめる）。
pub const VERSION: u32 = 4;

/// 共有の daemon につないで最初に送る場所が、この文字で始まれば FE 向けの窓口。
pub const PREAMBLE: char = '\u{1}';

pub type Result<T> = amonhen_model::Result<T>;

/// 答え: JSON と、あればレコード（`json["data"]` に区画の名前ごとの位置と長さ）。
#[derive(Debug, Default)]
pub struct Reply {
    pub json: Value,
    pub data: Vec<u8>,
}

impl Reply {
    pub fn json(json: Value) -> Self {
        Self {
            json,
            data: Vec::new(),
        }
    }

    /// フレームの中身: JSON の長さ（u32、LE）、JSON、8 バイト境界まで詰め物、レコード。
    pub fn encode(&self) -> Vec<u8> {
        let json = self.json.to_string();
        let mut out = Vec::with_capacity(8 + json.len() + self.data.len());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(json.as_bytes());
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        out.extend_from_slice(&self.data);
        out
    }

    pub fn decode(payload: &[u8]) -> Result<Self> {
        let len = payload
            .get(..4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
            .ok_or_else(|| tr!("The reply is too short"))?;
        let json = payload
            .get(4..4 + len)
            .ok_or_else(|| tr!("The reply’s JSON is cut off"))?;
        let start = (4 + len).next_multiple_of(8).min(payload.len());
        Ok(Self {
            json: serde_json::from_slice(json)?,
            data: payload[start..].to_vec(),
        })
    }

    /// `name` の区画のバイト列。
    pub fn section(&self, name: &str) -> &[u8] {
        let at = &self.json["data"][name];
        match (at[0].as_u64(), at[1].as_u64()) {
            (Some(off), Some(len)) => self
                .data
                .get(off as usize..(off + len) as usize)
                .unwrap_or(&[]),
            _ => &[],
        }
    }

    /// `name` の区画を、`T`（`repr(C)` の数値・`Node`・`Edge`・`Oid` など）の列として。
    pub fn items<T: Copy>(&self, name: &str) -> Vec<T> {
        let bytes = self.section(name);
        let size = std::mem::size_of::<T>();
        (0..bytes.len() / size)
            // SAFETY: `T` は詰め物のない `repr(C)` の型で、区画はその並びを書いたもの。揃っていない位置から読む。
            .map(|i| unsafe { std::ptr::read_unaligned(bytes.as_ptr().add(i * size).cast::<T>()) })
            .collect()
    }

    /// `name` の区画を `u32` の列として。
    pub fn u32s(&self, name: &str) -> Vec<u32> {
        self.section(name)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect()
    }
}
