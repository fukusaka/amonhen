//! `amonhend` を子プロセスとして起動してつなぐ（SSH の代わりに手元で起動するだけで、通信は同じ）。
//! 偽の ssh や daemon の停止に sh を使うので、Unix だけで動かす。
#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する
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
    // daemon が動かす git は利用者の global の設定を読む。そこでコミットに署名すると、並べて走らせたときに gpg が
    // 失敗することがある。
    if args.first() == Some(&"init") {
        for key in ["commit.gpgsign", "tag.gpgsign"] {
            git(dir, &["config", key, "false"]);
        }
    }
}

#[test]
fn talks_to_the_daemon_process_and_hears_about_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
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

/// パスなしの `--stdio` は、その機械の設定（`git config --global`）・git の版・プログラムを扱う。
#[test]
fn serves_the_machine_settings_without_a_repository() {
    use amonhen_core::command::Command;

    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let global = tmp.path().join("gitconfig");
    // 設定のリポジトリはデータの置き場所（XDG_DATA_HOME）に作られ、global の設定はこのテストのファイルに書く。
    let conn = Connection::spawn(
        "sh",
        &[
            "-c".to_owned(),
            r#"XDG_DATA_HOME="$1" GIT_CONFIG_GLOBAL="$2" GIT_CONFIG_NOSYSTEM=1 exec "$0" --stdio"#
                .to_owned(),
            env!("CARGO_BIN_EXE_amonhend").to_owned(),
            data.display().to_string(),
            global.display().to_string(),
        ],
    )
    .unwrap();
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());

    let set = repo
        .run(Command::ConfigSet {
            key: "user.name".to_owned(),
            value: Some("Ada".to_owned()),
            scope: Some("global".to_owned()),
        })
        .unwrap();
    assert!(set.ok, "{set:?}");
    assert!(data.join("amonhen/settings-repo/.git").is_dir());
    assert!(
        std::fs::read_to_string(&global)
            .unwrap()
            .contains("name = Ada")
    );

    let list = repo.run(Command::ConfigList).unwrap();
    let entries = list.info.unwrap()["entries"].to_string();
    assert!(
        entries.contains("\"user.name\"") && entries.contains("\"Ada\""),
        "{entries}"
    );

    let version = repo.run(Command::GitVersion).unwrap();
    assert!(version.info.unwrap()["version"].is_string());

    let programs = repo
        .run(Command::Programs {
            name: "git".to_owned(),
        })
        .unwrap();
    assert!(
        !programs.info.unwrap()["programs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reports_progress_and_the_reason_when_the_daemon_goes_away() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
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
# 手元の daemon が ssh に渡した環境（つないだ FE のもの）を残す。
echo "${{GIT_AMONHEN_TEST:-}}" >> '{1}/ssh-env.log'
# SSH 先の環境は手元から引き継がない（手元に入れた amonhen を SSH 先のものと見なさない）。
exec env -i HOME='{0}' PATH='{0}/bin:/usr/bin:/bin' sh -c "$*"
"#,
            home.display(),
            dir.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// SSH 先のエンジンの置き場所（`~/.local/share/amonhen/engine`）にあるもの。
fn engines(home: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(home.join(".local/share/amonhen/engine"))
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn touch_old(path: &Path) {
    let ok = Command::new("touch")
        .args(["-t", "202001010000"])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[cfg(unix)]
#[test]
fn provisions_the_engine_on_ssh_hosts() {
    use amonhen_remote::ssh;
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    git(&repo_dir, &["init", "-q"]);
    git(&repo_dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&repo_dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let ssh_program = fake_ssh(tmp.path(), &home);
    let ssh_program = ssh_program.to_str().unwrap();
    // 送り込むエンジンの置き場所。手元の OS 向けに、テストで作ったものを置く。
    let bundled = tmp.path().join("engines");
    std::fs::create_dir(&bundled).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_amonhend"),
        bundled.join(format!("amonhen-{}", ssh::local_platform())),
    )
    .unwrap();
    let daemons = ssh::Daemons {
        dir: bundled.clone(),
        local: None,
        remote_engine: None,
        push: true,
    };
    let target = ssh::Target::parse(&format!("ssh://dev{}", repo_dir.display())).unwrap();
    let open = |daemons: &ssh::Daemons| -> amonhen_model::Result<String> {
        let conn = ssh::connect(ssh_program, &target, daemons)?;
        let engine = conn.remote_engine().unwrap_or_default();
        let repo = Repository::new(RemoteBackend::connect(conn)?);
        assert_eq!(repo.load_history()?.graph.len(), 1);
        Ok(engine)
    };

    // 1 回目: 送る。置いたら中身を確かめる。
    let engine = open(&daemons).unwrap();
    assert!(engine.contains("sent now"), "{engine}");
    let sent = engines(&home);
    assert_eq!(sent.len(), 1, "{sent:?}");
    let binary = home
        .join(".local/share/amonhen/engine")
        .join(&sent[0])
        .join("amonhen");
    let first = std::fs::metadata(&binary).unwrap().modified().unwrap();
    // 送ったエンジンのビルドの指紋も置く。
    assert_eq!(
        std::fs::read_to_string(binary.with_file_name("build")).unwrap(),
        amonhen_remote::BUILD_ID
    );

    // ほかのマシンが最近送った版・30 日使われていない版・前の置き場所の古い daemon・書きかけを置いておく。
    let root = home.join(".local/share/amonhen/engine");
    for name in ["0000000000000000", "1111111111111111"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("amonhen"), b"").unwrap();
        std::fs::write(root.join(name).join("used"), b"").unwrap();
    }
    touch_old(&root.join("1111111111111111/used"));
    let stale_tmp = root.join(&sent[0]).join("amonhen.tmp.43");
    std::fs::write(&stale_tmp, b"").unwrap();
    touch_old(&stale_tmp);
    let legacy = home.join(".cache/amonhen/amonhend-2222222222222222");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, b"").unwrap();
    touch_old(&legacy);

    // 2 回目: 送らずに、置いてあるものを使う。最近使われたほかの版は残し、使われていないものだけを片付ける。
    let engine = open(&daemons).unwrap();
    assert!(engine.contains("sent earlier"), "{engine}");
    assert_eq!(
        std::fs::metadata(&binary).unwrap().modified().unwrap(),
        first
    );
    assert_eq!(
        engines(&home),
        ["0000000000000000".to_owned(), sent[0].clone()]
    );
    assert!(!stale_tmp.exists() && !legacy.exists());

    // 手元に送れるエンジンがなくても（別のビルドのものだけ）、SSH 先にこのビルドのものがあれば、それを使う。
    let elsewhere = tmp.path().join("other-engines");
    std::fs::create_dir(&elsewhere).unwrap();
    std::fs::write(
        elsewhere.join(format!("amonhen-{}", ssh::local_platform())),
        b"amonhen-build=0000000000000000;",
    )
    .unwrap();
    let other_build = ssh::Daemons {
        dir: elsewhere,
        local: None,
        remote_engine: None,
        push: true,
    };
    let engine = open(&other_build).unwrap();
    assert!(
        engine.contains("sent earlier") && engine.contains(&sent[0]),
        "{engine}"
    );
    // SSH 先にもなければ、別のビルドだと断る（送らない）。
    std::fs::remove_file(binary.with_file_name("build")).unwrap();
    let err = open(&other_build).unwrap_err().to_string();
    assert!(err.contains("different build"), "{err}");
    // 前の版が置いた（指紋の記録がない）このビルドのものは、中身のハッシュで見つけて、指紋を記録し直す。
    open(&daemons).unwrap();
    assert_eq!(
        std::fs::read_to_string(binary.with_file_name("build")).unwrap(),
        amonhen_remote::BUILD_ID
    );

    // SSH 先の PATH に同じビルドの amonhen があれば、それを使う。
    let bin = home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_amonhend"), bin.join("amonhen")).unwrap();
    let engine = open(&daemons).unwrap();
    assert!(
        engine.contains("installed on the SSH host") && engine.contains("/bin/amonhen"),
        "{engine}"
    );
    std::fs::remove_dir_all(&bin).unwrap();

    // 送り込みを止めていて、使えるものがなければ、理由を返す。
    std::fs::remove_dir_all(&root).unwrap();
    let no_push = ssh::Daemons {
        dir: bundled,
        local: None,
        remote_engine: None,
        push: false,
    };
    let err = open(&no_push).unwrap_err().to_string();
    assert!(err.contains("No usable engine"), "{err}");
    assert!(engines(&home).is_empty());
}

/// 共有の daemon（`--serve`）: 2 つの相手が同じリポジトリを開くと、どちらにも変化の知らせが来る。開けない
/// パスは理由を返す。
#[test]
fn shares_one_daemon_between_clients() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("repo");
    std::fs::create_dir(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    // daemon が起動する git でもコミットできるように（CI には全体の設定がない）。
    git(&dir, &["config", "user.name", "a"]);
    git(&dir, &["config", "user.email", "a@example.com"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
    // ソケットのパスは短くする（macOS は 104 バイトまで）。
    let socket = tmp.path().join("d.sock");
    // History のキャッシュは試験の中に書かせる（手元のアプリのキャッシュに混ぜない）。
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_amonhend"))
        .env("AMONHEN_LANG", "en")
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
                Err(_) if started.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
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
    let caches: Vec<_> = walk(&home)
        .into_iter()
        .filter(|p| p.to_string_lossy().contains("history-"))
        .collect();
    assert_eq!(caches.len(), 1, "{caches:?}");

    git(&dir, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let mut heard = std::collections::HashSet::new();
    while heard.len() < 2 {
        heard.insert(
            rx.recv_timeout(Duration::from_secs(10))
                .expect("両方に知らせが来ない"),
        );
    }
    assert_eq!(rb.load_history().unwrap().graph.len(), 2);

    // History の作り直しは daemon のコアで行い、同じ 1 つのキャッシュを書き直す。
    let before = ra.load_history().unwrap();
    let rebuilt = ra.rebuild_history().unwrap();
    assert_eq!(rebuilt.graph, before.graph);
    assert!(rebuilt.generation > before.generation);
    let caches: Vec<_> = walk(&home)
        .into_iter()
        .filter(|p| p.to_string_lossy().contains("history-"))
        .collect();
    assert_eq!(caches.len(), 1, "{caches:?}");
    assert_eq!(rb.load_history().unwrap().graph.len(), 2);

    // 命令は daemon のコアで実行される（ブランチができる）。
    let reply =
        ra.run_json(r#"{"op":"createBranch","name":"topic","start":"main","checkout":false}"#);
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    let out = Command::new("git")
        .args(["branch", "--list", "topic"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("topic"));

    // コミットは daemon のコアで行い、Undo の記録も daemon の側に書く（daemon のコアの Undo で戻せる）。
    std::fs::write(dir.join("c.txt"), "c\n").unwrap();
    rb.stage(&["c.txt"]).unwrap();
    rb.commit("from b", false).unwrap();
    let log = Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "from b");
    let peek = ra.run_json(r#"{"op":"undoPeek"}"#);
    assert!(
        peek.contains("Commit"),
        "a からも daemon の記録が見える: {peek}"
    );

    // 変更を捨てる操作も daemon のコアで行い、daemon の Undo で戻せる。
    std::fs::write(dir.join("c.txt"), "changed\n").unwrap();
    let status = rb.load_status().unwrap();
    let entry = status
        .status
        .entries
        .iter()
        .find(|e| e.path == "c.txt")
        .unwrap();
    let diff = rb
        .working_diff(entry, false, &amonhen_projection::FrontendProfile::MACOS)
        .unwrap();
    let removed_and_added: Vec<usize> = (0..diff.file.lines.len())
        .filter(|&r| {
            diff.file.lines[r].kind == amonhen_layout::DiffRow::ADDED
                || diff.file.lines[r].kind == amonhen_layout::DiffRow::REMOVED
        })
        .collect();
    rb.discard_rows(&diff, &removed_and_added).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("c.txt")).unwrap(), "c\n");
    let undo = ra.run_json(r#"{"op":"undo"}"#);
    assert!(undo.contains("Discard lines in c.txt"), "{undo}");
    assert_eq!(
        std::fs::read_to_string(dir.join("c.txt")).unwrap(),
        "changed\n"
    );
    git(&dir, &["checkout", "--", "c.txt"]);

    // daemon 越しの差分は、手元で計算したものと同じになる。
    std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
    std::fs::write(dir.join("b.txt"), "x\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "files"]);
    let head = ra.load_history().unwrap().graph.ids()[0];
    let changes = ra.commit_changes(&head).unwrap();
    let remote = ra.stacked_diff(&changes).unwrap();
    let local = Repository::new(amonhen_backend_gix::GixBackend::open(&dir).unwrap())
        .stacked_diff(&changes)
        .unwrap();
    assert_eq!(remote.files.len(), 2);
    for ((rp, rd), (lp, ld)) in remote.files.iter().zip(&local.files) {
        assert_eq!(rp, lp);
        assert_eq!(rd.lines, ld.lines);
        assert_eq!(rd.new, ld.new);
    }

    // 開けないパスは、最初の要求に理由が返る。
    let bad = connect("/nonexistent-amonhen");
    let error = RemoteBackend::connect(bad)
        .err()
        .expect("開けないはず")
        .to_string();
    assert!(error.contains("Can’t open"), "{error}");

    // 2 つ目の daemon は、動いているものに任せてすぐ終わる。
    let second = Command::new(env!("CARGO_BIN_EXE_amonhend"))
        .env("AMONHEN_LANG", "en")
        .arg("--serve")
        .arg(&socket)
        .status()
        .unwrap();
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

/// 共有の daemon が、パスなしの `ssh://host` で SSH 先のリポジトリではなくその機械の設定を開く（Settings > SSH Hosts）。
/// SSH 先ではエンジンがパスなしの `--stdio` で動き、global の設定を読み書きする。
#[test]
fn shared_daemon_opens_an_ssh_host_without_a_repository() {
    use amonhen_remote::ssh;
    let tmp = tempfile::tempdir().unwrap();
    let remote_home = tmp.path().join("remote-home");
    let local_home = tmp.path().join("local-home");
    std::fs::create_dir(&remote_home).unwrap();
    std::fs::create_dir(&local_home).unwrap();
    let ssh_program = fake_ssh(tmp.path(), &remote_home);
    let bundled = tmp.path().join("daemons");
    std::fs::create_dir(&bundled).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_amonhend"),
        bundled.join(format!("amonhen-{}", ssh::local_platform())),
    )
    .unwrap();
    let socket = tmp.path().join("d.sock");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_amonhend"))
        .env("AMONHEN_LANG", "en")
        .arg("--serve")
        .arg(&socket)
        .env("HOME", &local_home)
        .env("AMONHEN_SSH", &ssh_program)
        .env("AMONHEN_ENGINES", &bundled)
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    let stream = loop {
        match std::os::unix::net::UnixStream::connect(&socket) {
            Ok(s) => break s,
            Err(_) if started.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(e) => panic!("つなげない: {e}"),
        }
    };
    let mut writer = stream.try_clone().unwrap();
    let env = amonhen_model::process::ClientEnv::from_vars([(
        "GIT_AMONHEN_TEST".to_owned(),
        "from the client".to_owned(),
    )]);
    amonhen_remote::local::write_preamble(
        &mut writer,
        &amonhen_remote::local::opening("ssh://dev", &env),
    )
    .unwrap();
    let conn = Connection::new(stream, std::io::BufWriter::new(writer));
    conn.mark_local();
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());

    // SSH 先（偽の ssh の HOME）の global の設定に書かれ、設定のリポジトリはそのデータの置き場所にできる。
    let set = repo
        .run(amonhen_core::command::Command::ConfigSet {
            key: "user.name".to_owned(),
            value: Some("Ada".to_owned()),
            scope: Some("global".to_owned()),
        })
        .unwrap();
    assert!(set.ok, "{set:?}");
    assert!(
        std::fs::read_to_string(remote_home.join(".gitconfig"))
            .unwrap()
            .contains("name = Ada")
    );
    assert!(
        remote_home
            .join(".local/share/amonhen/settings-repo/.git")
            .is_dir()
    );
    assert_eq!(engines(&remote_home).len(), 1);
    let programs = repo
        .run(amonhen_core::command::Command::Programs {
            name: "git".to_owned(),
        })
        .unwrap();
    assert!(
        !programs.info.unwrap()["programs"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    daemon.kill().unwrap();
    daemon.wait().unwrap();
}

/// 共有の daemon が SSH 先を開く: 2 つの相手が同じ SSH 先を開いても、SSH 先の daemon は 1 つだけ送り込み、
/// History は手元の daemon が作ってファイルで渡す。命令も手元の daemon のコアから SSH 先に届く。
#[test]
fn shared_daemon_opens_an_ssh_repository_for_everyone() {
    use amonhen_remote::ssh;
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    git(&repo_dir, &["init", "-q"]);
    git(&repo_dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&repo_dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
    // SSH 先のホーム（送り込んだ daemon が置かれる）と、手元の daemon のホーム（History のキャッシュ）。
    let remote_home = tmp.path().join("remote-home");
    let local_home = tmp.path().join("local-home");
    std::fs::create_dir(&remote_home).unwrap();
    std::fs::create_dir(&local_home).unwrap();
    let ssh_program = fake_ssh(tmp.path(), &remote_home);
    let bundled = tmp.path().join("daemons");
    std::fs::create_dir(&bundled).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_amonhend"),
        bundled.join(format!("amonhen-{}", ssh::local_platform())),
    )
    .unwrap();
    let socket = tmp.path().join("d.sock");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_amonhend"))
        .env("AMONHEN_LANG", "en")
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
                Err(_) if started.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(e) => panic!("つなげない: {e}"),
            }
        };
        let mut writer = stream.try_clone().unwrap();
        // FE の環境も送る（daemon は、このつながりのために動かす ssh・git にそれを渡す）。
        let env = amonhen_model::process::ClientEnv::from_vars([(
            "GIT_AMONHEN_TEST".to_owned(),
            "from the client".to_owned(),
        )]);
        amonhen_remote::local::write_preamble(
            &mut writer,
            &amonhen_remote::local::opening(location, &env),
        )
        .unwrap();
        let conn = Connection::new(stream, std::io::BufWriter::new(writer));
        conn.mark_local();
        conn
    };
    let location = format!("ssh://dev{}", repo_dir.display());
    let a = Repository::new(RemoteBackend::connect(connect(&location)).unwrap());
    let b = Repository::new(RemoteBackend::connect(connect(&location)).unwrap());
    assert_eq!(a.load_history().unwrap().graph.len(), 1);
    assert_eq!(b.load_history().unwrap().graph.len(), 1);
    // SSH 先にはエンジンを 1 つだけ送った（2 つ目の相手は同じつながりを使う）。
    assert_eq!(engines(&remote_home).len(), 1);
    // daemon は、つないだ FE の環境で ssh を動かした（daemon 自身は GIT_AMONHEN_TEST を持たない）。
    let seen = std::fs::read_to_string(tmp.path().join("ssh-env.log")).unwrap();
    assert!(
        !seen.is_empty() && seen.lines().all(|l| l == "from the client"),
        "{seen}"
    );

    // 命令は手元の daemon のコアから SSH 先の git に届き、History にも出る。
    let reply =
        a.run_json(r#"{"op":"createBranch","name":"topic","start":"main","checkout":false}"#);
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    git(
        &repo_dir,
        &["commit", "-q", "--allow-empty", "-m", "second"],
    );
    assert_eq!(b.load_history().unwrap().graph.len(), 2);

    // daemon の様子に、SSH 先のつながりが出る。
    let mut status = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    amonhen_remote::local::write_preamble(&mut status, "\0status").unwrap();
    let frame = amonhen_remote::frame::read(&mut status).unwrap().unwrap();
    let text = String::from_utf8_lossy(&frame.payload).into_owned();
    assert!(
        text.contains(r#""ssh":true"#) && text.contains("ssh://dev"),
        "{text}"
    );
    assert!(
        text.contains("sent now") || text.contains("sent earlier"),
        "SSH 先のエンジンが出る: {text}"
    );
    let _ = daemon.kill();
    let _ = daemon.wait();
}
