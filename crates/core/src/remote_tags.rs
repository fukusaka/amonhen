//! リモートごとのタグ。ブランチと違って手元にリモートのタグの写しはなく（fetch したタグは手元の `refs/tags` に
//! 混ざる）、どのリモートにどのタグがあるかは手元の ref からは分からない。そこで fetch のついでに
//! `git ls-remote --tags` で読み、共通の git dir の `amonhen/remote-tags.json` に覚える。タグを push・削除した
//! ときは、その結果で覚えを直す（ネットワークを待たずに出し直せる）。

use std::collections::BTreeMap;

use amonhen_model::{Oid, Operations, RepoFiles, Result, Root};
use serde::{Deserialize, Serialize};

const FILE: &str = "amonhen/remote-tags.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Known {
    /// 読んだ時刻（UNIX 時刻のミリ秒）。
    checked: u64,
    /// タグの名前 → 指すコミット（注釈付きのタグはたどった先）。
    tags: BTreeMap<String, String>,
}

/// リモートの名前 → 覚えているタグ。
type Cache = BTreeMap<String, Known>;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn load(files: &dyn RepoFiles) -> Cache {
    files
        .read(Root::CommonDir, FILE)
        .ok()
        .flatten()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

fn save(files: &dyn RepoFiles, cache: &Cache) -> Result<()> {
    files.write(Root::CommonDir, FILE, &serde_json::to_vec_pretty(cache)?)
}

/// すべてのリモートのタグを読み直す。読めなかったリモート（つながらないなど）は前の覚えのまま、もうない
/// リモートの覚えは消す。
pub fn refresh(ops: &dyn Operations, files: &dyn RepoFiles) -> Result<()> {
    let remotes = ops.remotes()?;
    let mut cache = load(files);
    cache.retain(|name, _| remotes.iter().any(|r| &r.name == name));
    for remote in &remotes {
        if let Ok(tags) = ops.remote_tags(&remote.name) {
            let tags = tags.into_iter().map(|(n, o)| (n, o.to_string())).collect();
            cache.insert(
                remote.name.clone(),
                Known {
                    checked: now_ms(),
                    tags,
                },
            );
        }
    }
    save(files, &cache)
}

/// タグを `remote` に push した（`delete` なら消した）結果で覚えを直す。`tags` は名前と、手元のタグが指す
/// コミット。まだ読んだことのないリモートは、読んだことにしない（ほかのタグは分からないまま）。
pub fn record_push(
    files: &dyn RepoFiles,
    remote: &str,
    tags: &[(&str, Option<Oid>)],
    delete: bool,
) -> Result<()> {
    let mut cache = load(files);
    let Some(known) = cache.get_mut(remote) else {
        return Ok(());
    };
    for (name, target) in tags {
        if delete {
            known.tags.remove(*name);
        } else if let Some(target) = target {
            known.tags.insert(name.to_string(), target.to_string());
        }
    }
    save(files, &cache)
}

/// 手元のタグ（名前と指すコミット）ごとの、リモートでの様子。`remotes` はすべてのリモートと読んだ時刻
/// （読んだことがなければ null）、`tags` はタグ → 読んだリモート → `same`（同じコミット）・`different`
/// （同じ名前で違うコミット）・`missing`（ない）。
pub fn status(
    ops: &dyn Operations,
    files: &dyn RepoFiles,
    local: &[(&str, Option<Oid>)],
) -> Result<serde_json::Value> {
    let remotes = ops.remotes()?;
    let cache = load(files);
    let known: Vec<(&str, &Known)> = remotes
        .iter()
        .filter_map(|r| cache.get(&r.name).map(|k| (r.name.as_str(), k)))
        .collect();
    let mut tags = serde_json::Map::new();
    for (name, target) in local {
        let mut states = serde_json::Map::new();
        for (remote, k) in &known {
            let state = match k.tags.get(*name) {
                None => "missing",
                Some(oid) if target.is_some_and(|t| t.to_string() == *oid) => "same",
                Some(_) => "different",
            };
            states.insert(remote.to_string(), state.into());
        }
        tags.insert(name.to_string(), states.into());
    }
    let remotes: Vec<serde_json::Value> = remotes
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "checked": cache.get(&r.name).map(|k| k.checked),
            })
        })
        .collect();
    Ok(serde_json::json!({ "remotes": remotes, "tags": tags }))
}
