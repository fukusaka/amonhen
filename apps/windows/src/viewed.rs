//! 見たことにしたファイル（Viewed）。コミットごとに覚えておく（古いものから捨てて 500 コミットまで。GTK4 版の
//! `commitfiles.rs` と同じ設定の鍵と形）。見たことにしたファイルは畳んで出す。

use std::collections::HashSet;

use serde_json::{Value, json};

const KEY: &str = "viewedFiles";
/// 覚えておくコミットの数。
const KEEP: usize = 500;

fn store() -> Value {
    crate::prefs::string(KEY)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "order": [], "files": {} }))
}

/// コミット `key` で見たことにしたパス。
pub fn paths(key: &str) -> HashSet<String> {
    store()["files"][key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p.as_str().map(str::to_owned))
        .collect()
}

/// コミット `key` で見たことにしたパスを覚える（古いコミットのものから捨てる）。
pub fn save(key: &str, paths: &HashSet<String>) {
    let store = store();
    let mut order: Vec<String> = store["order"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|k| k.as_str().map(str::to_owned))
        .filter(|k| k != key)
        .collect();
    let mut files = store["files"].as_object().cloned().unwrap_or_default();
    if paths.is_empty() {
        files.remove(key);
    } else {
        let mut list: Vec<&String> = paths.iter().collect();
        list.sort();
        files.insert(key.to_owned(), json!(list));
        order.push(key.to_owned());
    }
    while order.len() > KEEP {
        let old = order.remove(0);
        files.remove(&old);
    }
    let store = json!({ "order": order, "files": files });
    if let Err(e) = crate::prefs::set_string(KEY, &store.to_string()) {
        eprintln!("Viewed を覚えられない: {e}");
    }
}

/// コミット `key` のパス `path` を、見たことにする・戻す。見たことにしたら true。
pub fn toggle(key: &str, path: &str) -> bool {
    let mut set = paths(key);
    let viewed = if set.remove(path) {
        false
    } else {
        set.insert(path.to_owned());
        true
    };
    save(key, &set);
    viewed
}
