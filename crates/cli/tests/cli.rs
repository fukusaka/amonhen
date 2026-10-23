//! `amonhen` を実際に起動して確かめる。
#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する

use std::path::Path;
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn commit(dir: &Path, file: &str, message: &str) {
    std::fs::write(dir.join(file), message).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn amonhen(home: &Path, args: &[&str]) -> (String, i32) {
    // キャッシュは試験の中に置く（手元のアプリのキャッシュに混ぜない）。
    let out = Command::new(env!("CARGO_BIN_EXE_amonhen"))
        .env("AMONHEN_LANG", "en")
        .env("HOME", home)
        .args(args)
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (text, out.status.code().unwrap_or(-1))
}

/// main に、topic を取り込んだ merge がある。
fn repository() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(dir, &["config", "user.name", "a"]);
    git(dir, &["config", "user.email", "a@example.com"]);
    commit(dir, "a.txt", "first");
    git(dir, &["switch", "-q", "-c", "topic"]);
    commit(dir, "t.txt", "topic work");
    git(dir, &["switch", "-q", "main"]);
    commit(dir, "m.txt", "main work");
    git(
        dir,
        &["merge", "-q", "--no-ff", "-m", "merge topic", "topic"],
    );
    tmp
}

#[test]
fn log_draws_the_branch_and_the_merge() {
    let repo = repository();
    let home = tempfile::tempdir().unwrap();
    let path = repo.path().to_str().unwrap();
    let (out, code) = amonhen(home.path(), &["log", "-C", path, "--color=never"]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[0].starts_with('◉') || lines[0].starts_with('◆'),
        "{out}"
    );
    assert!(
        lines[0].contains("(HEAD → main)") && lines[0].contains("merge topic"),
        "{out}"
    );
    assert!(out.contains("├─╮"), "枝分かれのつなぎ:\n{out}");
    assert!(out.contains("╯"), "合流のつなぎ:\n{out}");
    assert!(out.contains("topic work") && out.contains("first"), "{out}");

    // 折りたたむと、topic の側は merge の数になる。
    let (out, _) = amonhen(home.path(), &["log", "-C", path, "--fold", "--color=never"]);
    assert!(!out.contains("topic work"), "{out}");
    assert!(out.contains("1 folded"), "{out}");

    // 検索はグラフなしで、合うものだけ。
    let (out, _) = amonhen(
        home.path(),
        &["log", "-C", path, "--color=never", "subject:topic"],
    );
    assert_eq!(out.lines().count(), 2, "{out}");
}

#[test]
fn undo_and_merge_file() {
    let repo = repository();
    let home = tempfile::tempdir().unwrap();
    let path = repo.path().to_str().unwrap();
    let (out, code) = amonhen(home.path(), &["undo", "-C", path, "--peek"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Nothing to undo"), "{out}");

    // 片側だけが変えたコンフリクトは、間違えようのない規則で解ける。両側が変えたものは残る。
    let file = repo.path().join("conflict.txt");
    std::fs::write(
        &file,
        "<<<<<<< ours\nsame\n||||||| base\nold\n=======\nsame\n>>>>>>> theirs\n\
         <<<<<<< ours\nx = 1\n||||||| base\nx = 0\n=======\nx = 2\n>>>>>>> theirs\n",
    )
    .unwrap();
    let file = file.to_str().unwrap();
    let (out, code) = amonhen(home.path(), &["merge-file", file]);
    assert_eq!(code, 1, "残りがあれば 1: {out}");
    assert!(out.contains("resolved 1 of 2 conflicts, 1 left"), "{out}");
    assert!(std::fs::read_to_string(file).unwrap().starts_with("same\n"));

    let (out, code) = amonhen(home.path(), &["log", "-C", "/"]);
    assert_eq!(code, 2, "{out}");
}

#[test]
fn worktree_lists_adds_and_removes() {
    let repo = repository();
    let home = tempfile::tempdir().unwrap();
    let path = repo.path().to_str().unwrap();
    let wt = repo.path().join(".worktrees").join("topic");
    let wt = wt.to_str().unwrap();
    let (out, code) = amonhen(
        home.path(),
        &["worktree", "-C", path, "add", wt, "-b", "wt-topic"],
    );
    assert_eq!(code, 0, "{out}");
    let (out, code) = amonhen(home.path(), &["worktree", "-C", path, "list"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("[wt-topic]") && out.contains("(current)"),
        "{out}"
    );
    // --changes で、それぞれの変更の数を出す。
    std::fs::write(Path::new(wt).join("new.txt"), "x").unwrap();
    let (out, code) = amonhen(home.path(), &["worktree", "-C", path, "list", "--changes"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1 changed"), "{out}");
    std::fs::remove_file(Path::new(wt).join("new.txt")).unwrap();
    // 開いた worktree が「現在」になる（git と同じく、実行した場所の worktree）。
    let (out, _) = amonhen(home.path(), &["worktree", "-C", wt, "list", "--json"]);
    let list: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(list[1]["current"], true, "{out}");
    let (out, code) = amonhen(home.path(), &["worktree", "-C", path, "remove", wt]);
    assert_eq!(code, 0, "{out}");
    let (out, _) = amonhen(home.path(), &["worktree", "-C", path, "list"]);
    assert!(!out.contains("[wt-topic]"), "{out}");
}

/// 3 つの開き方（手元で直接・`daemon:`・`ssh://`）で、同じ命令が同じように動く。CLI は VSCode 拡張やほかの FE の
/// 試金石なので、daemon・SSH の難所（コアに任せる操作・共有メモリの差分・知らせ）を全部通す。
#[cfg(unix)]
#[test]
fn works_the_same_directly_through_the_daemon_and_over_ssh() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&repo, &["config", "user.name", "a"]);
    git(&repo, &["config", "user.email", "a@example.com"]);
    std::fs::write(repo.join("f.txt"), "a\nb\nc\nd\ne\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let env = Env::new(tmp.path());
    let repo = repo.canonicalize().unwrap();
    let path = repo.to_str().unwrap().to_owned();
    let locations = [
        path.clone(),
        format!("daemon:{path}"),
        format!("ssh://dev{path}"),
    ];
    let mut shown = Vec::new();
    for (i, at) in locations.iter().enumerate() {
        let at = at.as_str();
        if i > 0 {
            std::fs::write(repo.join("f.txt"), "a\nb\nc\nd\ne\n").unwrap();
            git(&repo, &["commit", "-q", "-am", &format!("reset {i}")]);
        }
        // 2 行目と 5 行目を変える。
        std::fs::write(repo.join("f.txt"), "a\nB\nc\nd\nE\n").unwrap();
        let status = env.json(&["status", "-C", at, "--json"]);
        assert_eq!(status["entries"][0]["path"], "f.txt", "{at}: {status}");
        assert_eq!(status["entries"][0]["worktree"], "M", "{at}: {status}");

        // 行ごとに stage する（b → B だけ）。
        let diff = env.json(&["diff", "-C", at, "--combined", "--json"]);
        let rows = diff[0]["rows"].as_array().unwrap();
        let picked: Vec<String> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r["text"] == "b" || r["text"] == "B")
            .map(|(n, _)| n.to_string())
            .collect();
        assert_eq!(picked.len(), 2, "{at}: {diff}");
        env.ok(&["stage", "-C", at, "--rows", &picked.join(","), "f.txt"]);
        let staged = env.json(&["diff", "-C", at, "--staged", "--json"]);
        let texts: Vec<&str> = staged[0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] != "context" && r["kind"] != "hunk")
            .map(|r| r["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, ["b", "B"], "{at}: {staged}");

        // コミットし、残りの変更は捨てて、Undo で戻す。
        let message = format!("change {i}");
        let committed = env.json(&["commit", "-C", at, "-m", &message, "--json"]);
        assert_eq!(committed["ok"], true, "{at}: {committed}");
        env.ok(&["discard", "-C", at, "f.txt"]);
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "a\nB\nc\nd\ne\n",
            "{at}"
        );
        env.ok(&["undo", "-C", at]);
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "a\nB\nc\nd\nE\n",
            "{at}"
        );
        git(&repo, &["checkout", "--", "f.txt"]);

        let log = env.json(&["log", "-C", at, "--json", "-n", "1"]);
        assert_eq!(log[0]["subject"], message.as_str(), "{at}: {log}");
        assert_eq!(log[0]["refs"][0], "main", "{at}: {log}");
        shown.push(env.json(&["show", "-C", at, "--json", "HEAD~1"]));
        let refs = env.json(&["refs", "-C", at, "--json"]);
        assert_eq!(refs["headBranch"], "main", "{at}: {refs}");

        // 命令の窓口: 1 行 1 命令で、1 行ずつ答える。
        let branch = format!("b{i}");
        let replies = env.stdin(
            &["run", "-C", at],
            &format!("{{\"op\":\"undoPeek\"}}\n{{\"op\":\"createBranch\",\"name\":\"{branch}\",\"start\":\"main\",\"checkout\":false}}\n{{\"op\":\"nope\"}}\n"),
        );
        let replies: Vec<serde_json::Value> = replies
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(replies.len(), 3, "{at}: {replies:?}");
        assert_eq!(replies[1]["ok"], true, "{at}: {replies:?}");
        assert_eq!(replies[2]["ok"], false, "{at}: {replies:?}");

        // 知らせ: つないだまま、作業ツリーとブランチの変化を受ける。
        let mut watch = env.spawn(&["watch", "-C", at, "--json"]);
        let lines = watch.lines();
        assert!(
            lines
                .recv_timeout(std::time::Duration::from_secs(20))
                .is_ok(),
            "{at}: 最初の様子が出ない"
        );
        std::fs::write(repo.join("f.txt"), "changed\n").unwrap();
        watch.expect(&lines, "worktree", at);
        git(&repo, &["checkout", "--", "f.txt"]);
        git(&repo, &["branch", &format!("w{i}")]);
        watch.expect(&lines, "refs", at);
        watch.stop();
    }
    // daemon 越しと SSH 越しで、同じ中身のコミット（「reset」）は同じに見える。
    for key in ["rows", "files", "changes"] {
        assert!(
            shown[1][key].as_array().is_some_and(|a| !a.is_empty()),
            "{key}: {}",
            shown[1]
        );
        assert_eq!(shown[1][key], shown[2][key], "{key}");
    }
}

#[cfg(unix)]
/// 試験の中だけで動く環境（ホーム・共有の daemon のソケット・偽の ssh）。
struct Env {
    home: std::path::PathBuf,
    socket_dir: std::path::PathBuf,
    ssh: std::path::PathBuf,
    daemons: std::path::PathBuf,
    daemon: std::path::PathBuf,
}

#[cfg(unix)]
impl Env {
    fn new(dir: &Path) -> Self {
        let home = dir.join("home");
        let remote_home = dir.join("remote-home");
        let socket_dir = dir.join("s");
        let daemons = dir.join("daemons");
        for d in [&home, &remote_home, &socket_dir, &daemons] {
            std::fs::create_dir(d).unwrap();
        }
        // エンジンは 1 つの実行ファイル: daemon にも、SSH 先に送るものにも、この CLI を使う。
        let daemon = std::path::PathBuf::from(env!("CARGO_BIN_EXE_amonhen"));
        std::fs::copy(
            &daemon,
            daemons.join(format!("amonhen-{}", amonhen_remote::ssh::local_platform())),
        )
        .unwrap();
        let ssh = dir.join("fake-ssh");
        // ssh の代わり: 指定を読み飛ばし、ホスト名を捨て、SSH 先のホームで命令を実行する。
        std::fs::write(
            &ssh,
            format!(
                "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n    case \"$1\" in\n        -T) shift ;;\n        -o|-p) shift 2 ;;\n        *) break ;;\n    esac\ndone\nshift\nexec env -i HOME='{0}' PATH='{0}/bin:/usr/bin:/bin' sh -c \"$*\"\n",
                remote_home.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            home,
            socket_dir,
            ssh,
            daemons,
            daemon,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_amonhen"));
        c.args(args)
            .env("AMONHEN_LANG", "en")
            .env("HOME", &self.home)
            .env("AMONHEN_DAEMON_DIR", &self.socket_dir)
            .env("AMONHEN_DAEMON", &self.daemon)
            .env("AMONHEN_SSH", &self.ssh)
            .env("AMONHEN_ENGINES", &self.daemons);
        c
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.command(args).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{args:?}: {text}");
        text
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let text = self.ok(args);
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{args:?}: {e}: {text}"))
    }

    fn stdin(&self, args: &[&str], input: &str) -> String {
        use std::io::Write;
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{args:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn spawn(&self, args: &[&str]) -> Watch {
        Watch(self.command(args).stdout(Stdio::piped()).spawn().unwrap())
    }
}

#[cfg(unix)]
impl Drop for Env {
    /// 試験が失敗しても、試験の共有の daemon を残さない。
    fn drop(&mut self) {
        let _ = self.command(&["daemon", "stop"]).output();
    }
}

#[cfg(unix)]
struct Watch(std::process::Child);

#[cfg(unix)]
impl Watch {
    /// 出力を 1 行ずつ受け取る。
    fn lines(&mut self) -> std::sync::mpsc::Receiver<String> {
        use std::io::BufRead;
        let (tx, rx) = std::sync::mpsc::channel();
        let stdout = self.0.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                let _ = tx.send(line);
            }
        });
        rx
    }

    /// `change` の知らせを受けるまで待つ。
    fn expect(&self, lines: &std::sync::mpsc::Receiver<String>, change: &str, at: &str) {
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(20) {
            if let Ok(line) = lines.recv_timeout(std::time::Duration::from_secs(1))
                && line.contains(&format!("\"{change}\""))
            {
                return;
            }
        }
        panic!("{at}: {change} の知らせが来ない");
    }

    fn stop(mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `amonhen install` はエンジンを置き場所に置いて current と ~/.local/bin を張り、`amonhen engine` はそれを daemon に
/// 使うエンジンとして出す。2 回目は置き直さない。
#[cfg(unix)]
#[test]
fn installs_the_engine_and_picks_it_for_the_daemon() {
    let home = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_amonhen"))
            .args(args)
            .env("AMONHEN_LANG", "en")
            .env("HOME", home.path())
            .env_remove("XDG_DATA_HOME")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let first = run(&["install"]);
    assert!(
        first.contains("Installed") && first.contains("Set current to this version"),
        "{first}"
    );
    let data = home.path().join(".local/share/amonhen");
    let current = data.join("current/amonhen");
    assert!(current.is_file());
    assert_eq!(
        std::fs::read_link(home.path().join(".local/bin/amonhen")).unwrap(),
        current
    );
    let second = run(&["install"]);
    assert!(
        !second.contains("Installed") && second.contains("current is already this version"),
        "{second}"
    );
    assert_eq!(run(&["engine"]).trim(), current.display().to_string());
    // 置いたエンジンは、CLI としても daemon としても動く。
    let version = Command::new(home.path().join(".local/bin/amonhen"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("(protocol "));
}

/// 共有の daemon 越し（`-C daemon:<path>`）で、status・stage・commit・log・undo が動く。偽の ssh を使わないので、
/// Windows（名前付きパイプ）でも流れる。
#[test]
fn works_through_the_shared_daemon_on_every_platform() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&repo, &["config", "user.name", "a"]);
    git(&repo, &["config", "user.email", "a@example.com"]);
    git(&repo, &["config", "core.autocrlf", "false"]);
    commit(&repo, "a.txt", "first");
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    // 試験の daemon は、手元の daemon と分ける（ソケット・パイプの名前と、キャッシュの置き場所）。
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_amonhen"))
            .args(args)
            .env("AMONHEN_LANG", "en")
            .env("HOME", &home)
            .env("LOCALAPPDATA", &home)
            .env("AMONHEN_DAEMON_DIR", tmp.path().join("s"))
            .env("AMONHEN_DAEMON", env!("CARGO_BIN_EXE_amonhen"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "{args:?}: {text}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        text
    };
    let at = format!("daemon:{}", repo.display());
    std::fs::write(repo.join("a.txt"), "changed\n").unwrap();
    let status = run(&["status", "-C", &at, "--json"]);
    assert!(status.contains(r#""path":"a.txt""#), "{status}");
    run(&["stage", "-C", &at, "a.txt"]);
    run(&["commit", "-C", &at, "-m", "through the daemon"]);
    let log = run(&["log", "-C", &at, "--json", "-n", "1"]);
    assert!(log.contains("through the daemon"), "{log}");
    run(&["undo", "-C", &at]);
    let log = run(&["log", "-C", &at, "--json", "-n", "1"]);
    assert!(log.contains(r#""subject":"first""#), "{log}");
    let status = run(&["daemon", "status", "--json"]);
    assert!(status.contains("\"repos\""), "{status}");
    run(&["daemon", "stop"]);
}
