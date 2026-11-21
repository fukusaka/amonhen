//! 最近開いたリポジトリ（`recent.json`）。アプリ・GTK4 のアプリ・TUI が同じものを使い、どれで開いたものも、ほかの
//! 一覧・Quick Open に出る。置き場所は設定と同じフォルダ（[`crate::settings::dir`]、`~/.config/amonhen/recent.json`）。
//!
//! 形は `{"repositories": [{"path": "/w/repo", "openedAt": 1732000000.5}, …]}` で、新しく開いた順。`openedAt` は
//! 最後に開いた日時（UNIX 時間の秒）で、この記録を始める前に開いたものにはない。書くときは隣の `recent.lock` を
//! 取ってから読み直す（同時に動くほかの Amon Hen の変更を消さない。macOS 版も同じ鍵を取る）。知らないキーは残す。

use std::path::PathBuf;

use crate::Result;

/// 最近開いたリポジトリ。
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    /// 場所（この機械のパスか、`ssh://`・`daemon:` で始まる場所）。
    pub path: String,
    /// 最後に開いた日時（UNIX 時間の秒）。
    pub opened_at: Option<f64>,
}

/// 最近開いたもののファイル。決められなければ None。
pub fn path() -> Option<PathBuf> {
    crate::settings::dir().map(|d| d.join("recent.json"))
}

/// 最近開いたもの（新しい順）。ファイルがなければ空。読めない（JSON でない）ときはその理由。
pub fn read() -> Result<Vec<Recent>, String> {
    let Some(path) = path().filter(|p| p.exists()) else {
        return Ok(Vec::new());
    };
    Ok(entries(&crate::settings::read(&path)?)
        .iter()
        .filter_map(parse)
        .collect())
}

/// 開いたことを覚える: `location` を先頭に置き、開いた日時を今にする。
pub fn touch(location: &str) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    edit(|list| {
        let mut entry = list
            .iter()
            .position(|e| parse(e).is_some_and(|r| r.path == location))
            .map(|i| list.remove(i))
            .and_then(|e| match e {
                serde_json::Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default();
        entry.insert("path".into(), location.into());
        entry.insert("openedAt".into(), now.into());
        list.insert(0, entry.into());
    })
}

/// 一覧から外す（Repositories の「一覧から外す」）。
pub fn forget(locations: &[&str]) -> Result<()> {
    edit(|list| list.retain(|e| parse(e).is_none_or(|r| !locations.contains(&r.path.as_str()))))
}

/// まだファイルがなければ、FE の前の記録 `entries`（新しい順）を写す。写したら `true`（ファイルがあれば何もしない）。
pub fn import(entries: &[Recent]) -> Result<bool> {
    let mut wrote = false;
    edit_if(
        |exists| !exists && !entries.is_empty(),
        |list| {
            wrote = true;
            list.extend(entries.iter().map(|r| {
                let mut entry = serde_json::Map::new();
                entry.insert("path".into(), r.path.clone().into());
                if let Some(t) = r.opened_at {
                    entry.insert("openedAt".into(), t.into());
                }
                serde_json::Value::Object(entry)
            }));
        },
    )?;
    Ok(wrote)
}

fn entries(object: &serde_json::Map<String, serde_json::Value>) -> Vec<serde_json::Value> {
    object
        .get("repositories")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn parse(entry: &serde_json::Value) -> Option<Recent> {
    Some(Recent {
        path: entry
            .get("path")?
            .as_str()
            .filter(|p| !p.is_empty())?
            .to_owned(),
        opened_at: entry.get("openedAt").and_then(serde_json::Value::as_f64),
    })
}

fn edit(change: impl FnOnce(&mut Vec<serde_json::Value>)) -> Result<()> {
    edit_if(|_| true, change)
}

/// `recent.lock` を取ってから読み直し、`proceed(ファイルがあるか)` なら `change` を当てて書く。
fn edit_if(
    proceed: impl FnOnce(bool) -> bool,
    change: impl FnOnce(&mut Vec<serde_json::Value>),
) -> Result<()> {
    let path = path()
        .ok_or_else(|| crate::tr!("Can’t tell where Amon Hen’s settings go (HOME isn’t set)"))?;
    let dir = path.parent().expect("recent.json is in a folder");
    std::fs::create_dir_all(dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("recent.lock"))?;
    // 閉じれば外れる。
    lock.lock()?;
    let exists = path.exists();
    if !proceed(exists) {
        return Ok(());
    }
    let mut object = if exists {
        crate::settings::read(&path).map_err(|e| -> crate::Error { e.into() })?
    } else {
        serde_json::Map::new()
    };
    let mut list = entries(&object);
    let before = list.clone();
    change(&mut list);
    if exists && list == before {
        return Ok(());
    }
    object.insert("repositories".into(), list.into());
    let mut text = serde_json::to_string_pretty(&object)?;
    text.push('\n');
    // 途中で読まれても半端な中身を見せないよう、隣に書いてから置き換える。
    let temp = dir.join(format!("recent.json.tmp.{}", std::process::id()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Vec<String> {
        read().unwrap().into_iter().map(|r| r.path).collect()
    }

    #[test]
    fn touches_forgets_and_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let vars = crate::settings::config_vars(dir.path().to_str().unwrap());
        let env = crate::process::ClientEnv::from_vars(vars);
        crate::process::with_client_env(&env, || {
            let file = dir.path().join("amonhen").join("recent.json");
            assert_eq!(path(), Some(file.clone()));
            assert_eq!(read(), Ok(Vec::new()), "no file is no recents");

            touch("/w/a").unwrap();
            touch("ssh://host/src/b").unwrap();
            assert_eq!(paths(), ["ssh://host/src/b", "/w/a"]);
            assert!(read().unwrap()[0].opened_at.unwrap() > 1.0e9);
            // 開き直すと先頭に来る（二度は並ばない）。
            touch("/w/a").unwrap();
            assert_eq!(paths(), ["/w/a", "ssh://host/src/b"]);

            // ほかの FE が書いた知らないキーは残す。
            std::fs::write(
                &file,
                r#"{"other": 1, "repositories": [
                    {"path": "/w/a", "color": "blue"},
                    {"openedAt": 5},
                    {"path": "/w/c", "openedAt": 7}
                ]}"#,
            )
            .unwrap();
            assert_eq!(
                paths(),
                ["/w/a", "/w/c"],
                "an entry without a path is skipped"
            );
            assert_eq!(read().unwrap()[0].opened_at, None);
            touch("/w/c").unwrap();
            forget(&["/w/a"]).unwrap();
            let written: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(written["other"], 1, "other keys stay");
            assert_eq!(paths(), ["/w/c"]);

            std::fs::write(&file, "[").unwrap();
            assert!(read().unwrap_err().contains("recent.json"));
            assert!(touch("/w/x").is_err(), "a broken file isn't overwritten");
        });
    }

    #[test]
    fn imports_only_when_there_is_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let vars = crate::settings::config_vars(dir.path().to_str().unwrap());
        let env = crate::process::ClientEnv::from_vars(vars);
        crate::process::with_client_env(&env, || {
            let old = |path: &str, opened_at| Recent {
                path: path.to_owned(),
                opened_at,
            };
            assert!(!import(&[]).unwrap(), "nothing to import writes nothing");
            assert!(!path().unwrap().exists());
            let entries = [old("/w/new", Some(20.0)), old("/w/old", None)];
            assert!(import(&entries).unwrap());
            assert_eq!(read().unwrap(), entries);
            // もうあれば写さない（ほかの FE が先に写した・書いた）。
            assert!(!import(&[old("/w/other", None)]).unwrap());
            assert_eq!(paths(), ["/w/new", "/w/old"]);
        });
    }
}
