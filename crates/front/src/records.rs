//! 答えに付けるレコード。区画ごとに 8 バイト境界に揃えて並べ、名前ごとの（始め、長さ）を JSON に書く。数値は
//! リトルエンディアン。点・線などは `amonhen-layout` の `repr(C)` の並びそのまま。

use serde_json::{Map, Value, json};

#[derive(Default)]
pub struct Records {
    data: Vec<u8>,
    sections: Map<String, Value>,
}

impl Records {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, name: &str, bytes: &[u8]) -> &mut Self {
        while !self.data.len().is_multiple_of(8) {
            self.data.push(0);
        }
        self.sections.insert(name.to_owned(), json!([self.data.len(), bytes.len()]));
        self.data.extend_from_slice(bytes);
        self
    }

    /// `repr(C)` の値の並びをそのまま。
    pub fn put_slice<T: Copy>(&mut self, name: &str, items: &[T]) -> &mut Self {
        // SAFETY: 呼ぶのは `repr(C)` で詰め物のない型（数値・`Node`・`Edge`・`Oid`）だけ。
        let bytes = unsafe { std::slice::from_raw_parts(items.as_ptr().cast::<u8>(), std::mem::size_of_val(items)) };
        self.put(name, bytes)
    }

    /// JSON（`data` に区画の位置を足す）とレコード。
    pub fn finish(self, mut json: Value) -> crate::Reply {
        json["data"] = Value::Object(self.sections);
        crate::Reply { json, data: self.data }
    }
}
