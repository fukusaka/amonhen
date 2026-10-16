//! `amonhend` を子プロセスとして起動してつなぐ（SSH の代わりに手元で起動するだけで、通信は同じ）。
//! 偽の ssh や daemon の停止に sh を使うので、Unix だけで動かす。
#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use amonhen_core::Repository;
use amonhen_remote::{Connection, Notification, RemoteBackend};

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "a")
        .env("GIT_AUTHOR_EMAIL", "a@example.com")
        .env("GIT_COMMITTER_NAME", "a")
        .env("GIT_COMMITTER_EMAIL", "a@example.com")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn talks_to_the_daemon_process_and_hears_about_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "first"]);

    let conn = Connection::spawn(
        env!("CARGO_BIN_EXE_amonhend"),
        &["--stdio".to_owned(), dir.display().to_string()],
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    conn.on_notification(move |n| {
        let _ = tx.send(n);
    });
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());
    assert_eq!(repo.load_history().unwrap().graph.len(), 1);

    // daemon の側でリポジトリが変わると知らせが来る。
    git(dir, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let Notification::Changed(bits) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("変化の知らせが来ない");
    assert_ne!(bits, 0);
    assert_eq!(repo.load_history().unwrap().graph.len(), 2);
}

#[test]
fn reports_progress_and_the_reason_when_the_daemon_goes_away() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "first"]);

    // 2 秒たったら daemon を止める（ネットワークが切れた代わり）。
    let conn = Connection::spawn(
        "sh",
        &[
            "-c".to_owned(),
            r#"(sleep 2; echo 'gone' >&2; kill $$) & exec "$0" --stdio "$1""#.to_owned(),
            env!("CARGO_BIN_EXE_amonhend").to_owned(),
            dir.display().to_string(),
        ],
    )
    .unwrap();
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());
    assert_eq!(repo.load_history().unwrap().graph.len(), 1);
    let state = repo.connection_state().unwrap();
    assert!(state.received > 0 && state.closed.is_none());

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let reason = loop {
        if let Some(r) = repo.connection_state().unwrap().closed {
            break r;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "切れたことに気づかない"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(reason.contains("gone"), "{reason}");
    assert!(repo.load_status().is_err());
}

#[test]
fn refuses_a_missing_repository() {
    let conn = Connection::spawn(
        env!("CARGO_BIN_EXE_amonhend"),
        &["--stdio".to_owned(), "/nonexistent/repo".to_owned()],
    )
    .unwrap();
    assert!(RemoteBackend::connect(conn).is_err());
}

/// SSH の代わりに、送られたコマンドを手元で（偽の HOME で）実行するスクリプト。
fn fake_ssh(dir: &Path, home: &Path) -> std::path::PathBuf {
    let script = dir.join("fake-ssh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
while [ $# -gt 0 ]; do
    case "$1" in
        -T) shift ;;
        -o|-p) shift 2 ;;
        *) break ;;
    esac
done
shift
HOME='{}' exec sh -c "$*"
"#,
            home.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[cfg(unix)]
#[test]
fn sends_the_daemon_once_and_reuses_it() {
    use amonhen_remote::ssh;
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    git(&repo_dir, &["init", "-q", "-b", "main"]);
    git(&repo_dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let ssh_program = fake_ssh(tmp.path(), &home);
    // 送り込む daemon の置き場所。手元の OS 向けに、テストで作った amonhend を置く。
    let bundled = tmp.path().join("daemons");
    std::fs::create_dir(&bundled).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_amonhend"),
        bundled.join(format!("amonhen-{}", ssh::local_platform())),
    )
    .unwrap();
    let daemons = ssh::Daemons {
        dir: bundled,
        local: None,
    };
    let target = ssh::Target::parse(&format!("ssh://dev{}", repo_dir.display())).unwrap();

    let conn = ssh::connect(ssh_program.to_str().unwrap(), &target, &daemons).unwrap();
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());
    assert_eq!(repo.load_history().unwrap().graph.len(), 1);
    let cache = home.join(".cache/amonhen");
    let sent: Vec<_> = std::fs::read_dir(&cache)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(sent.len(), 1, "daemon を 1 つ送った: {sent:?}");
    let first = std::fs::metadata(&sent[0]).unwrap().modified().unwrap();
    drop(repo);

    // 前の版の daemon と、書きかけの一時ファイル（新しいもの・1 日より前のもの）を置いておく。
    let old = cache.join("amonhend-0000000000000000");
    let fresh_tmp = cache.join("amonhend-1111111111111111.tmp.42");
    let stale_tmp = cache.join("amonhend-2222222222222222.tmp.43");
    for f in [&old, &fresh_tmp, &stale_tmp] {
        std::fs::write(f, b"").unwrap();
    }
    let ok = Command::new("touch")
        .args(["-t", "202001010000"])
        .arg(&stale_tmp)
        .status()
        .unwrap()
        .success();
    assert!(ok);

    // 2 回目は送らずに、置いてある daemon を使う。前の版と古い一時ファイルは消える。
    let conn = ssh::connect(ssh_program.to_str().unwrap(), &target, &daemons).unwrap();
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());
    assert_eq!(repo.load_history().unwrap().graph.len(), 1);
    assert_eq!(
        std::fs::metadata(&sent[0]).unwrap().modified().unwrap(),
        first
    );
    assert!(!old.exists() && !stale_tmp.exists());
    assert!(fresh_tmp.exists(), "書いている途中かもしれないので残す");
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 2);
}

/// 共有の daemon（`--serve`）: 2 つの相手が同じリポジトリを開くと、どちらにも変化の知らせが来る。開けない
/// パスは理由を返す。
#[test]
fn shares_one_daemon_between_clients() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("repo");
    std::fs::create_dir(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
    // ソケットのパスは短くする（macOS は 104 バイトまで）。
    let socket = tmp.path().join("d.sock");
    // History のキャッシュは試験の中に書かせる（手元のアプリのキャッシュに混ぜない）。
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_amonhend"))
        .arg("--serve")
        .arg(&socket)
        .env("HOME", &home)
        .spawn()
        .unwrap();
    let connect = |path: &str| {
        let started = std::time::Instant::now();
        let stream = loop {
            match std::os::unix::net::UnixStream::connect(&socket) {
                Ok(s) => break s,
                Err(_) if started.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => panic!("つなげない: {e}"),
            }
        };
        let mut writer = stream.try_clone().unwrap();
        amonhen_remote::local::write_preamble(&mut writer, path).unwrap();
        let conn = Connection::new(stream, std::io::BufWriter::new(writer));
        conn.mark_local();
        conn
    };
    let path = dir.display().to_string();
    let (a, b) = (connect(&path), connect(&path));
    let (tx, rx) = mpsc::channel();
    for (name, conn) in [("a", &a), ("b", &b)] {
        let tx = tx.clone();
        conn.on_notification(move |_| {
            let _ = tx.send(name);
        });
    }
    let ra = Repository::new(RemoteBackend::connect(a).unwrap());
    let rb = Repository::new(RemoteBackend::connect(b).unwrap());
    assert_eq!(ra.load_history().unwrap().graph.len(), 1);
    assert_eq!(rb.load_history().unwrap().graph.len(), 1);
    // History は daemon が 1 つだけ作ってファイルに書き、どちらもそれを読む。
    let caches: Vec<_> = walk(&home).into_iter().filter(|p| p.to_string_lossy().contains("history-")).collect();
    assert_eq!(caches.len(), 1, "{caches:?}");

    git(&dir, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let mut heard = std::collections::HashSet::new();
    while heard.len() < 2 {
        heard.insert(rx.recv_timeout(Duration::from_secs(10)).expect("両方に知らせが来ない"));
    }
    assert_eq!(rb.load_history().unwrap().graph.len(), 2);

    // History の作り直しは daemon のコアで行い、同じ 1 つのキャッシュを書き直す。
    let before = ra.load_history().unwrap();
    let rebuilt = ra.rebuild_history().unwrap();
    assert_eq!(rebuilt.graph, before.graph);
    assert!(rebuilt.generation > before.generation);
    let caches: Vec<_> = walk(&home).into_iter().filter(|p| p.to_string_lossy().contains("history-")).collect();
    assert_eq!(caches.len(), 1, "{caches:?}");
    assert_eq!(rb.load_history().unwrap().graph.len(), 2);

    // 命令は daemon のコアで実行される（ブランチができる）。
    let reply = ra.run_json(r#"{"op":"createBranch","name":"topic","start":"main","checkout":false}"#);
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    let out = Command::new("git").args(["branch", "--list", "topic"]).current_dir(&dir).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("topic"));

    // コミットは daemon のコアで行い、Undo の記録も daemon の側に書く（daemon のコアの Undo で戻せる）。
    std::fs::write(dir.join("c.txt"), "c\n").unwrap();
    rb.stage(&["c.txt"]).unwrap();
    rb.commit("from b", false).unwrap();
    let log = Command::new("git").args(["log", "-1", "--format=%s"]).current_dir(&dir).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "from b");
    let peek = ra.run_json(r#"{"op":"undoPeek"}"#);
    assert!(peek.contains("Commit"), "a からも daemon の記録が見える: {peek}");

    // 変更を捨てる操作も daemon のコアで行い、daemon の Undo で戻せる。
    std::fs::write(dir.join("c.txt"), "changed\n").unwrap();
    let status = rb.load_status().unwrap();
    let entry = status.status.entries.iter().find(|e| e.path == "c.txt").unwrap();
    let diff = rb.working_diff(entry, false, &amonhen_projection::FrontendProfile::MACOS).unwrap();
    let removed_and_added: Vec<usize> = (0..diff.file.lines.len())
        .filter(|&r| diff.file.lines[r].kind == amonhen_layout::DiffRow::ADDED || diff.file.lines[r].kind == amonhen_layout::DiffRow::REMOVED)
        .collect();
    rb.discard_rows(&diff, &removed_and_added).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("c.txt")).unwrap(), "c\n");
    let undo = ra.run_json(r#"{"op":"undo"}"#);
    assert!(undo.contains("Discard lines in c.txt"), "{undo}");
    assert_eq!(std::fs::read_to_string(dir.join("c.txt")).unwrap(), "changed\n");
    git(&dir, &["checkout", "--", "c.txt"]);

    // daemon 越しの差分は、手元で計算したものと同じになる。
    std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
    std::fs::write(dir.join("b.txt"), "x\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "files"]);
    let head = ra.load_history().unwrap().graph.ids()[0];
    let changes = ra.commit_changes(&head).unwrap();
    let remote = ra.stacked_diff(&changes).unwrap();
    let local = Repository::new(amonhen_backend_gix::GixBackend::open(&dir).unwrap()).stacked_diff(&changes).unwrap();
    assert_eq!(remote.files.len(), 2);
    for ((rp, rd), (lp, ld)) in remote.files.iter().zip(&local.files) {
        assert_eq!(rp, lp);
        assert_eq!(rd.lines, ld.lines);
        assert_eq!(rd.new, ld.new);
    }

    // 開けないパスは、最初の要求に理由が返る。
    let bad = connect("/nonexistent-amonhen");
    let error = RemoteBackend::connect(bad).err().expect("開けないはず").to_string();
    assert!(error.contains("開けない"), "{error}");

    // 2 つ目の daemon は、動いているものに任せてすぐ終わる。
    let second = Command::new(env!("CARGO_BIN_EXE_amonhend")).arg("--serve").arg(&socket).status().unwrap();
    assert!(second.success());
    let _ = daemon.kill();
    let _ = daemon.wait();
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// 共有の daemon が SSH 先を開く: 2 つの相手が同じ SSH 先を開いても、SSH 先の daemon は 1 つだけ送り込み、
/// History は手元の daemon が作ってファイルで渡す。命令も手元の daemon のコアから SSH 先に届く。
#[test]
fn shared_daemon_opens_an_ssh_repository_for_everyone() {
    use amonhen_remote::ssh;
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    git(&repo_dir, &["init", "-q", "-b", "main"]);
    git(&repo_dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
    // SSH 先のホーム（送り込んだ daemon が置かれる）と、手元の daemon のホーム（History のキャッシュ）。
    let remote_home = tmp.path().join("remote-home");
    let local_home = tmp.path().join("local-home");
    std::fs::create_dir(&remote_home).unwrap();
    std::fs::create_dir(&local_home).unwrap();
    let ssh_program = fake_ssh(tmp.path(), &remote_home);
    let bundled = tmp.path().join("daemons");
    std::fs::create_dir(&bundled).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_amonhend"), bundled.join(format!("amonhen-{}", ssh::local_platform()))).unwrap();
    let socket = tmp.path().join("d.sock");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_amonhend"))
        .arg("--serve")
        .arg(&socket)
        .env("HOME", &local_home)
        .env("AMONHEN_SSH", &ssh_program)
        .env("AMONHEN_ENGINES", &bundled)
        .spawn()
        .unwrap();
    let connect = |location: &str| {
        let started = std::time::Instant::now();
        let stream = loop {
            match std::os::unix::net::UnixStream::connect(&socket) {
                Ok(s) => break s,
                Err(_) if started.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => panic!("つなげない: {e}"),
            }
        };
        let mut writer = stream.try_clone().unwrap();
        amonhen_remote::local::write_preamble(&mut writer, location).unwrap();
        let conn = Connection::new(stream, std::io::BufWriter::new(writer));
        conn.mark_local();
        conn
    };
    let location = format!("ssh://dev{}", repo_dir.display());
    let a = Repository::new(RemoteBackend::connect(connect(&location)).unwrap());
    let b = Repository::new(RemoteBackend::connect(connect(&location)).unwrap());
    assert_eq!(a.load_history().unwrap().graph.len(), 1);
    assert_eq!(b.load_history().unwrap().graph.len(), 1);
    // SSH 先には daemon を 1 つだけ送った（2 つ目の相手は同じつながりを使う）。
    let sent = std::fs::read_dir(remote_home.join(".cache/amonhen")).unwrap().count();
    assert_eq!(sent, 1);

    // 命令は手元の daemon のコアから SSH 先の git に届き、History にも出る。
    let reply = a.run_json(r#"{"op":"createBranch","name":"topic","start":"main","checkout":false}"#);
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    git(&repo_dir, &["commit", "-q", "--allow-empty", "-m", "second"]);
    assert_eq!(b.load_history().unwrap().graph.len(), 2);

    // daemon の様子に、SSH 先のつながりが出る。
    let mut status = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    amonhen_remote::local::write_preamble(&mut status, "\0status").unwrap();
    let frame = amonhen_remote::frame::read(&mut status).unwrap().unwrap();
    let text = String::from_utf8_lossy(&frame.payload).into_owned();
    assert!(text.contains(r#""ssh":true"#) && text.contains("ssh://dev"), "{text}");
    let _ = daemon.kill();
    let _ = daemon.wait();
}
