//! Linux・BSD の fsmonitor: git の fsmonitor の hook（version 2）に Watchman で答える。git の
//! `fsmonitor--daemon` は macOS と Windows にしかないので、ほかの Unix では、PATH に `watchman` があれば、
//! エンジン自身（`amonhen fsmonitor-watchman <version> <token>`）を `core.fsmonitor` の hook にして status に渡す。
//! リポジトリの設定には書かない。
//!
//! 取り決めは git の `fsmonitor-watchman.sample` と同じ: git は作業ツリーの一番上で hook を起動し、前に返した
//! token を渡す。hook は新しい token（Watchman の clock）と、それから変わったパスを NUL で区切って出す。`/` は
//! 「すべて変わったかもしれない」（git がすべて調べ直す）。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use serde_json::{Value, json};

/// hook の取り決めの版（git の `core.fsmonitorHookVersion`）。
pub(crate) const HOOK_VERSION: &str = "2";

/// PATH にある `watchman`（一度だけ探す）。
pub fn watchman() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| find_on_path(amonhen_model::process::var_os("PATH")?, "watchman"))
        .as_deref()
}

/// `path`（PATH の値）で最初に見つかる実行できる `name`。
fn find_on_path(path: std::ffi::OsString, name: &str) -> Option<PathBuf> {
    let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&path)
        .map(|dir| dir.join(&name))
        .find(|p| is_executable(p))
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    meta.is_file()
}

/// `core.fsmonitor` に入れる値: エンジンを hook として起動する命令（git はシェルで起動し、版と token を後ろに足す）。
pub(crate) fn hook_command(engine: &Path) -> String {
    format!(
        "{} fsmonitor-watchman",
        shell_quote(&engine.to_string_lossy())
    )
}

/// シェルの 1 語にする（`'...'`。中の `'` は `'\''`）。
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `amonhen fsmonitor-watchman <version> <token>`: git の hook として答えを標準出力に出す。作業ツリーは今のフォルダ
/// （git がそこで起動する）。版が違えば 1 を返す（git は hook を使わず、すべて調べる）。
pub fn run_hook(args: &[String]) -> i32 {
    let (Some(version), Some(token)) = (args.first(), args.get(1)) else {
        return 1;
    };
    if version != HOOK_VERSION {
        return 1;
    }
    let root = std::env::current_dir().unwrap_or_default();
    let found = find_on_path(std::env::var_os("PATH").unwrap_or_default(), "watchman");
    let out = respond(token, &root, found.as_deref());
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&out).and_then(|_| stdout.flush()).is_err() {
        return 1;
    }
    0
}

/// hook の答え（`<clock>\0<path>\0<path>...`）。Watchman が使えないときは、前の token のまま `/` を返す
/// （git はすべて調べる。次は前の token からの変化を聞くので、取りこぼさない）。
pub(crate) fn respond(token: &str, root: &Path, watchman: Option<&Path>) -> Vec<u8> {
    watchman
        .and_then(|w| changes(w, token, root))
        .unwrap_or_else(|| encode(token, None))
}

/// Watchman に聞いた、`token` からの変化。見張っていなければ見張りを加えて（`watch-project`）聞き直す。
fn changes(watchman: &Path, token: &str, root: &Path) -> Option<Vec<u8>> {
    let root = root.to_str()?;
    let mut reply = call(watchman, &query(root, None, token))?;
    if reply.get("error").is_some() {
        // 見張っていない（か、上のフォルダを見張っている）: プロジェクトの見張りを加えて、その中で聞く。
        let watch = call(watchman, &json!(["watch-project", root]))?;
        if watch.get("error").is_some() {
            return None;
        }
        let top = watch.get("watch")?.as_str()?;
        let relative = watch.get("relative_path").and_then(Value::as_str);
        reply = call(watchman, &query(top, relative, token))?;
        if reply.get("error").is_some() {
            return None;
        }
    }
    let clock = reply.get("clock")?.as_str()?;
    // 初めて（か Watchman が起動し直した）なら、全部のファイルが返る。一つずつ渡すより、すべて調べ直してもらう。
    if reply.get("is_fresh_instance").and_then(Value::as_bool) == Some(true) {
        return Some(encode(clock, None));
    }
    let files: Vec<&str> = reply
        .get("files")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    Some(encode(clock, Some(&files)))
}

/// Watchman の `query`: `token`（前に返した clock。`c` で始まる）からの変化を、`.git` の中を除いて名前だけ。
/// clock でなければ（git が初めに作る時刻）、すべてを聞く。
fn query(root: &str, relative: Option<&str>, token: &str) -> Value {
    let mut spec = json!({
        "fields": ["name"],
        "expression": ["not", ["dirname", ".git"]],
    });
    if token.starts_with('c') {
        spec["since"] = json!(token);
    }
    if let Some(relative) = relative {
        spec["relative_root"] = json!(relative);
    }
    json!(["query", root, spec])
}

/// `watchman -j --no-pretty` に `request` を送り、答えを読む。起動できない・答えが JSON でなければ None。
fn call(watchman: &Path, request: &Value) -> Option<Value> {
    let mut child = amonhen_model::process::command(watchman)
        .args(["-j", "--no-pretty"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    {
        let mut stdin = child.stdin.take()?;
        stdin.write_all(request.to_string().as_bytes()).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// hook の答えの形: token、NUL、NUL で区切ったパス。`files` が None なら `/`（すべて調べる）。
fn encode(token: &str, files: Option<&[&str]>) -> Vec<u8> {
    let mut out = token.as_bytes().to_vec();
    out.push(0);
    match files {
        None => out.push(b'/'),
        Some(files) => out.extend_from_slice(files.join("\0").as_bytes()),
    }
    out
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// 決まった答えを出す偽の watchman（受け取った命令は `requests` に足していく）。`answers` は命令の名前ごとの答え。
    fn fake(dir: &Path, answers: &[(&str, &str)]) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let log = dir.join("requests");
        let mut script = format!(
            "#!/bin/sh\n[ \"$1 $2\" = '-j --no-pretty' ] || exit 2\nreq=$(cat)\nprintf '%s\\n' \"$req\" >> '{}'\ncase \"$req\" in\n",
            log.display()
        );
        for (name, answer) in answers {
            script.push_str(&format!("  '[\"{name}\"'*) printf '%s' '{answer}' ;;\n"));
        }
        script.push_str("  *) exit 1 ;;\nesac\n");
        let path = dir.join("watchman");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn requests(dir: &Path) -> Vec<Value> {
        std::fs::read_to_string(dir.join("requests"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn reports_the_files_changed_since_the_clock() {
        let dir = tempfile::tempdir().unwrap();
        let w = fake(
            dir.path(),
            &[(
                "query",
                r#"{"version":"2024","clock":"c:1:2","is_fresh_instance":false,"files":["a.txt","sub/b.txt"]}"#,
            )],
        );
        let out = respond("c:1:1", Path::new("/work"), Some(&w));
        assert_eq!(out, b"c:1:2\0a.txt\0sub/b.txt");
        let sent = requests(dir.path());
        assert_eq!(
            sent,
            vec![json!(["query", "/work", {
                "since": "c:1:1",
                "fields": ["name"],
                "expression": ["not", ["dirname", ".git"]],
            }])]
        );
    }

    #[test]
    fn asks_for_everything_without_a_clock_and_says_all_on_a_fresh_instance() {
        let dir = tempfile::tempdir().unwrap();
        let w = fake(
            dir.path(),
            &[(
                "query",
                r#"{"clock":"c:1:5","is_fresh_instance":true,"files":["a.txt"]}"#,
            )],
        );
        // git が初めに渡す token は時刻（clock ではない）。
        let out = respond("1730000000000000000", Path::new("/work"), Some(&w));
        assert_eq!(out, b"c:1:5\0/");
        assert!(requests(dir.path())[0][2].get("since").is_none());
    }

    #[test]
    fn adds_the_watch_when_the_work_tree_is_not_watched() {
        let dir = tempfile::tempdir().unwrap();
        // 1 回目の query は見張っていないと答え、watch-project の後は上のフォルダの中で聞く。
        let w = fake(
            dir.path(),
            &[
                (
                    "query\",\"/work",
                    r#"{"error":"unable to resolve root /work: directory /work is not watched"}"#,
                ),
                (
                    "watch-project",
                    r#"{"watch":"/top","relative_path":"work"}"#,
                ),
                (
                    "query\",\"/top",
                    r#"{"clock":"c:2:1","is_fresh_instance":true,"files":[]}"#,
                ),
            ],
        );
        let out = respond("c:1:1", Path::new("/work"), Some(&w));
        assert_eq!(out, b"c:2:1\0/");
        let sent = requests(dir.path());
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[1], json!(["watch-project", "/work"]));
        assert_eq!(sent[2][1], "/top");
        assert_eq!(sent[2][2]["relative_root"], "work");
    }

    #[test]
    fn falls_back_to_a_full_scan_when_watchman_fails() {
        let dir = tempfile::tempdir().unwrap();
        // 何を聞いても失敗する。
        let w = fake(dir.path(), &[]);
        assert_eq!(respond("c:1:1", Path::new("/work"), Some(&w)), b"c:1:1\0/");
        // 答えが JSON でない。
        let w = fake(dir.path(), &[("query", "not json")]);
        assert_eq!(respond("c:1:1", Path::new("/work"), Some(&w)), b"c:1:1\0/");
        // 見張りを加えられない。
        let w = fake(
            dir.path(),
            &[
                ("query", r#"{"error":"not watched"}"#),
                ("watch-project", r#"{"error":"inotify limit"}"#),
            ],
        );
        assert_eq!(respond("c:1:1", Path::new("/work"), Some(&w)), b"c:1:1\0/");
        // watchman がない。
        assert_eq!(respond("c:1:1", Path::new("/work"), None), b"c:1:1\0/");
    }

    #[test]
    fn finds_watchman_on_the_path_and_quotes_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        let w = fake(dir.path(), &[]);
        let path = std::env::join_paths([Path::new("/nonexistent"), dir.path()]).unwrap();
        assert_eq!(find_on_path(path, "watchman"), Some(w));
        assert_eq!(find_on_path("/nonexistent".into(), "watchman"), None);
        assert_eq!(
            hook_command(Path::new("/opt/it's here/amonhen")),
            r"'/opt/it'\''s here/amonhen' fsmonitor-watchman"
        );
    }
}
