//! `git` を起動して行う操作（デスクトップ向け）。書き込み系の操作は正しさと互換性を優先して
//! git 本体に任せる。読み取りでも、git の仕組み（untracked cache・fsmonitor）が効く status はここ。

mod extras;
mod ops;
mod status;
mod verify;

use amonhen_model::tr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use amonhen_model::Result;

pub use status::parse_status;
pub use verify::LocalVerifier;

/// status で git の fsmonitor（`git fsmonitor--daemon`）を使うか。macOS と Windows でだけ効く。
static FSMONITOR: AtomicBool = AtomicBool::new(false);

/// status で fsmonitor を使うかを決める（アプリの設定から呼ぶ）。リポジトリごとに daemon が常駐し、
/// `.git` にソケットを作る。
pub fn set_fsmonitor(enabled: bool) {
    FSMONITOR.store(enabled, Ordering::Relaxed);
}

fn fsmonitor_enabled() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows")) && FSMONITOR.load(Ordering::Relaxed)
}

/// status の結果（untracked cache・fsmonitor の状態）を index に書き残す間隔。ふだんの status は
/// 利用者の git とロックを奪い合わないよう index に書かないので、ときどきだけ書いてキャッシュを新しくする。
const SAVE_STATUS_EVERY: Duration = Duration::from_secs(60);

/// `url` を `dest`（まだないフォルダ）に clone する。`dest` の親で git を起動する（Activity に残る）。
pub fn clone(url: &str, dest: &std::path::Path) -> Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| tr!("The clone destination folder is invalid"))?;
    std::fs::create_dir_all(parent)?;
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| tr!("The clone destination folder’s name is invalid"))?;
    Git::new(parent)
        .write(&["clone", "--", url, name])
        .map(drop)
}

/// リポジトリの一覧に出す軽い状態（`Git::peek`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Peek {
    /// 今のブランチ。detached なら None。
    pub branch: Option<String>,
    pub upstream: bool,
    pub ahead: u32,
    pub behind: u32,
    /// 変更のあるファイルの数（untracked は数えない）。
    pub changed: u32,
    /// HEAD のコミットの日時（UNIX 時間、コミッターの日時）と件名。まだコミットがなければ None。
    pub last_commit: Option<(i64, String)>,
}

/// 1 つのリポジトリで git を実行する。
#[derive(Clone, Debug)]
pub struct Git {
    workdir: PathBuf,
    /// status の結果を最後に index に書き残した時刻。
    status_saved: Arc<Mutex<Option<Instant>>>,
    /// ほかの worktree の変更の数と、その変化の見張り（`Operations::worktree_changes`）。
    pub(crate) worktrees: Arc<Mutex<ops::WorktreeCounts>>,
}

impl Git {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
            status_saved: Arc::default(),
            worktrees: Arc::default(),
        }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub(crate) fn command(&self, args: &[&str]) -> Command {
        let mut cmd = amonhen_model::process::command("git");
        cmd.current_dir(&self.workdir)
            // 利用者の端末に依存しない出力にする。
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["-c", "core.quotepath=off", "-c", "color.ui=false"])
            .args(args);
        // 端末がないので、資格情報は FE が渡した askpass（ダイアログ）で尋ねる。git は GIT_ASKPASS を、git が動かす
        // ssh は SSH_ASKPASS を使う（ssh は端末がなくても使うよう SSH_ASKPASS_REQUIRE）。
        if let Some(askpass) = std::env::var_os("AMONHEN_ASKPASS") {
            cmd.env("GIT_ASKPASS", &askpass)
                .env("SSH_ASKPASS", &askpass)
                .env("SSH_ASKPASS_REQUIRE", "prefer");
        }
        cmd
    }

    /// rebase で、積み重ねたブランチも一緒に動かすか（`--update-refs`）。git 2.38 から。利用者が
    /// `rebase.updateRefs` を false にしていれば動かさない。
    pub fn updates_refs_in_rebase(&self) -> bool {
        git_at_least(2, 38) && !self.rebase_update_refs_off()
    }

    /// `rebase.updateRefs` が false に設定されている。
    pub(crate) fn rebase_update_refs_off(&self) -> bool {
        self.read(&["config", "--bool", "--get", "rebase.updateRefs"])
            .is_ok_and(|out| String::from_utf8_lossy(&out).trim() == "false")
    }

    /// 読み取り系。利用者自身の git 操作とロックを奪い合わないよう、任意のロックを取らない。
    pub fn read(&self, args: &[&str]) -> Result<Vec<u8>> {
        let mut cmd = self.command(args);
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
        run(cmd, None)
    }

    /// リポジトリの一覧に出す、軽い状態（ブランチ・上流との差・変更のあるファイルの数）。untracked のファイルは
    /// 数えず、ロックも取らない（一覧の行ごとに裏で呼ぶ）。
    pub fn peek(&self) -> Result<Peek> {
        let out = self.read(&[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=no",
        ])?;
        let mut peek = Peek::default();
        for line in String::from_utf8_lossy(&out).lines() {
            if let Some(head) = line.strip_prefix("# branch.head ") {
                peek.branch = (head != "(detached)").then(|| head.to_owned());
            } else if line.starts_with("# branch.upstream ") {
                peek.upstream = true;
            } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
                for part in ab.split(' ') {
                    if let Some(n) = part.strip_prefix('+') {
                        peek.ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = part.strip_prefix('-') {
                        peek.behind = n.parse().unwrap_or(0);
                    }
                }
            } else if !line.starts_with('#') && !line.is_empty() {
                peek.changed += 1;
            }
        }
        // 最後のコミット（一覧で、いつ何をしたリポジトリかを見渡すため）。まだコミットがなければ失敗するので無視する。
        if let Ok(out) = self.read(&["log", "-1", "--format=%ct%x09%s"]) {
            let text = String::from_utf8_lossy(&out);
            if let Some((time, subject)) = text.trim_end().split_once('\t') {
                peek.last_commit = time.parse().ok().map(|t| (t, subject.to_owned()));
            }
        }
        Ok(peek)
    }

    /// 書き込み系。
    pub fn write(&self, args: &[&str]) -> Result<Vec<u8>> {
        run(self.command(args), None)
    }

    /// 読み取り系で、標準入力を渡す。
    pub fn read_with_input(&self, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
        let mut cmd = self.command(args);
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
        run(cmd, Some(input))
    }

    /// 書き込み系で、標準入力を渡す。
    pub fn write_with_input(&self, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
        run(self.command(args), Some(input))
    }
}

/// git を実行し、Activity に記録する（どのリポジトリで、何を、どれだけかかったか）。
fn run(cmd: Command, input: Option<&[u8]>) -> Result<Vec<u8>> {
    let repo = cmd
        .get_current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    // `-c` の設定は毎回同じなので省く。
    let mut label = String::from("git");
    let mut args = cmd.get_args();
    while let Some(arg) = args.next() {
        if arg == "-c" {
            args.next();
            continue;
        }
        label.push(' ');
        label.push_str(&arg.to_string_lossy());
    }
    // ネットワークを使う操作は、進み具合を流しながら読み、取り消せるようにする。
    let subcommand = cmd
        .get_args()
        .find(|a| !a.to_string_lossy().starts_with('-') && !a.to_string_lossy().contains('='))
        .map(|a| a.to_string_lossy().into_owned());
    let network = subcommand
        .as_deref()
        .is_some_and(|a| ["fetch", "pull", "push", "clone", "ls-remote"].contains(&a));
    if network && input.is_none() && !cmd.get_args().any(|a| a == "--") {
        // ls-remote は進み具合を出さない（`--progress` を受け付けない）。
        let progress = subcommand.as_deref() != Some("ls-remote");
        return amonhen_model::activity::track_with("git", &repo, label, |seq| {
            spawn_streaming(cmd, seq, progress)
        })
        .map_err(explain_old_git);
    }
    amonhen_model::activity::track_with("git", &repo, label, |seq| {
        let (out, err) = spawn_with_stderr(cmd, input)?;
        // 成功しても hooks などが標準エラーに書いていれば、記録に残す（コミットの後に見せる）。
        let err = String::from_utf8_lossy(&err);
        if !err.trim().is_empty() {
            amonhen_model::activity::set_output(seq, err.trim());
        }
        Ok(out)
    })
    .map_err(explain_old_git)
}

/// Amon Hen が使う git の最低の版（`git switch` が 2.23 から）。それより新しい機能は、版を見て古いやり方に戻す。
pub const MIN_GIT_VERSION: (u32, u32) = (2, 23);

/// `git` の版（`git version` の数の 3 つ）。読めなければ `None`。プロセスで 1 回だけ調べる。
pub fn git_version() -> Option<(u32, u32, u32)> {
    static VERSION: std::sync::OnceLock<Option<(u32, u32, u32)>> = std::sync::OnceLock::new();
    *VERSION.get_or_init(|| {
        let out = amonhen_model::process::command("git")
            .arg("version")
            .output()
            .ok()?;
        parse_git_version(&String::from_utf8_lossy(&out.stdout))
    })
}

/// git が `major.minor` 以上か（版を読めなければ新しいとみなす）。新しい機能は、これで見て古いやり方に戻す。
pub(crate) fn git_at_least(major: u32, minor: u32) -> bool {
    git_version().is_none_or(|(a, b, _)| (a, b) >= (major, minor))
}

/// `git version 2.39.5 (Apple Git-154)`・`git version 2.45.1.windows.1` などの、先頭の数を 3 つ読む。
fn parse_git_version(text: &str) -> Option<(u32, u32, u32)> {
    let rest = text.trim().strip_prefix("git version ")?;
    let mut numbers = rest
        .split(|c: char| !c.is_ascii_digit())
        .take_while(|part| !part.is_empty())
        .map(|part| part.parse::<u32>().ok());
    Some((
        numbers.next()??,
        numbers.next()??,
        numbers.next().flatten().unwrap_or(0),
    ))
}

/// git が失敗したとき、版が最低より古ければ、それが理由かもしれないと添える（古い git は知らない命令やオプションを
/// 分かりにくい言葉で断る）。取り消しには添えない。
fn explain_old_git(error: amonhen_model::Error) -> amonhen_model::Error {
    let message = error.to_string();
    match git_version() {
        Some((major, minor, patch))
            if (major, minor) < MIN_GIT_VERSION && !amonhen_model::i18n::is_cancelled(&message) =>
        {
            tr!(
                "{message}\n(git {version} is older than {minimum}, which Amon Hen needs. Update git.)",
                message = message,
                version = format!("{major}.{minor}.{patch}"),
                minimum = format!("{}.{}", MIN_GIT_VERSION.0, MIN_GIT_VERSION.1),
            )
            .into()
        }
        _ => error,
    }
}

/// ネットワークを使う git が、これだけ何も出さなければ止める（`--progress` なので、動いていれば数秒ごとに出す）。
const NETWORK_SILENCE: Duration = Duration::from_secs(120);
/// askpass（ダイアログ）があるときの上限。黙っているのは、人がダイアログに向かっている間かもしれない。
const NETWORK_SILENCE_WITH_ASKPASS: Duration = Duration::from_secs(600);

fn network_silence() -> Duration {
    if std::env::var_os("AMONHEN_ASKPASS").is_some() {
        NETWORK_SILENCE_WITH_ASKPASS
    } else {
        NETWORK_SILENCE
    }
}

/// `--progress` を付けて実行し、標準エラーの進み具合（\r で上書きされる行）を Activity に書く。取り消しを
/// 頼まれたら git を止める。
fn spawn_streaming(mut cmd: Command, seq: u64, progress: bool) -> Result<Vec<u8>> {
    use amonhen_model::activity;
    use std::io::Read;
    if progress {
        cmd.arg("--progress");
    }
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    // 取り消すときに git が起動した ssh なども止められるよう、別のプロセスグループにする。
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn()?;
    activity::set_cancellable(seq);
    // 最後に標準エラーに何か出た時刻（始めてからのミリ秒）。認証の入力待ちなどで黙ったままのときに止める。
    let started = Instant::now();
    let heard = Arc::new(AtomicU64::new(0));
    let heard_in_thread = heard.clone();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    // 標準エラーは、進み具合の行（\r 区切り）を拾いながら、ほかの行はエラーの文に残す。
    let err_thread = std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut line = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut chunk) {
            if n == 0 {
                break;
            }
            heard_in_thread.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
            for &b in &chunk[..n] {
                if b == b'\r' || b == b'\n' {
                    let text = String::from_utf8_lossy(&line).trim().to_owned();
                    if !text.is_empty() {
                        if text.contains('%') || text.contains("objects") {
                            activity::set_progress(seq, &text);
                        }
                        if b == b'\n' {
                            all.extend_from_slice(text.as_bytes());
                            all.push(b'\n');
                        }
                    }
                    line.clear();
                } else {
                    line.push(b);
                }
            }
        }
        all
    });
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        // 止めたあとは、出力を読む 2 つのスレッドを待たない。git の子（git-remote-https・ssh・ext:: の
        // コマンド）が親より先に切り離されていると、Windows の taskkill /T はそれを殺せず、パイプを握ったまま
        // 残る。待つと、その子が終わるまで（ext::sleep 30 なら 30 秒）返らない。スレッドはパイプが閉じれば
        // 勝手に終わる。
        if activity::take_cancel(seq) {
            kill_tree(&mut child);
            let _ = child.wait();
            return Err(amonhen_model::i18n::cancelled().into());
        }
        let silent = started.elapsed().as_millis() as u64 - heard.load(Ordering::Relaxed);
        let limit = network_silence();
        if silent > limit.as_millis() as u64 {
            kill_tree(&mut child);
            let _ = child.wait();
            return Err(tr!(
                "Stopped after {seconds} seconds without a response. It may have been waiting for a sign-in (a password or passphrase dialog, or the SSH agent) or for the network",
                seconds = limit.as_secs()
            )
            .into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let out = out_thread.join().unwrap_or_default();
    let err = err_thread.join().unwrap_or_default();
    if !status.success() {
        // 進み具合の行（remote: Counting など）を除いた、最後の数行をエラーの文にする。
        let text = String::from_utf8_lossy(&err);
        let message = text
            .lines()
            .filter(|l| !l.contains('%'))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_owned();
        return Err(if message.is_empty() {
            tr!("git failed: {status}", status = format!("{status:?}"))
        } else {
            message
        }
        .into());
    }
    Ok(out)
}

/// 実行して、標準出力と標準エラーを返す。失敗したら標準エラーの文を返す。
fn spawn_with_stderr(mut cmd: Command, input: Option<&[u8]>) -> Result<(Vec<u8>, Vec<u8>)> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = cmd.spawn()?;
    if let Some(input) = input {
        // 大きな入力で詰まらないよう、書き込みは別のスレッドで行う。
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_vec();
        std::thread::spawn(move || stdin.write_all(&input));
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let message = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(if message.is_empty() {
            tr!("git failed: {status}", status = format!("{:?}", out.status))
        } else {
            message
        }
        .into());
    }
    Ok((out.stdout, out.stderr))
}

/// History の読み込み。
impl Git {
    /// `tips` から辿れ、`known_tips` からは辿れないコミット（`git rev-list`。commit-graph があれば速い）。
    pub fn commits_since(
        &self,
        tips: &[amonhen_model::Oid],
        known_tips: &[amonhen_model::Oid],
    ) -> Result<Vec<amonhen_model::RawCommit>> {
        // 起点は標準入力で渡す（ref が数千あっても引数の長さの制限にかからない）。
        let mut input = String::new();
        for t in tips {
            input.push_str(&format!("{t}\n"));
        }
        for k in known_tips {
            input.push_str(&format!("^{k}\n"));
        }
        let mut cmd = self.command(&["rev-list", "--stdin", "--parents", "--timestamp"]);
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
        let out = run(cmd, Some(input.as_bytes()))?;
        // 1 行が「時刻 ID 親...」。
        Ok(String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|line| {
                let mut words = line.split(' ');
                let time = words.next()?.parse().ok()?;
                let id = amonhen_model::Oid::from_hex(words.next()?)?;
                let parents = words.filter_map(amonhen_model::Oid::from_hex).collect();
                Some(amonhen_model::RawCommit {
                    id,
                    commit_time: time,
                    parents,
                })
            })
            .collect())
    }
}

/// 作業ツリーと index の操作。
impl Git {
    pub fn status(&self) -> Result<amonhen_model::Status> {
        // 追跡していないファイルを探した結果をディレクトリごとに覚えておく（untracked cache）。
        // `--untracked-files=all` と設定が揃っていないとキャッシュを使わないので、両方を渡す。
        let mut args = vec![
            "-c",
            "core.untrackedCache=true",
            "-c",
            "status.showUntrackedFiles=all",
        ];
        if fsmonitor_enabled() {
            args.extend(["-c", "core.fsmonitor=true"]);
        }
        args.extend([
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ]);
        // 初めてと、前に書き残してから時間がたったときだけ、結果を index に書き残す（ロックは任意なので、
        // 取れなければ git は書かずに済ませる）。それ以外は書かずに、残してあるキャッシュを読むだけ。
        let save = {
            let mut saved = self.status_saved.lock().unwrap();
            let due = saved.is_none_or(|t| t.elapsed() >= SAVE_STATUS_EVERY);
            if due {
                *saved = Some(Instant::now());
            }
            due
        };
        parse_status(&if save {
            self.write(&args)?
        } else {
            self.read(&args)?
        })
    }

    /// `paths` の変更（追加・削除を含む）を index に入れる。
    pub fn stage(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["add", "-A", "--"];
        args.extend(paths);
        self.write(&args).map(drop)
    }

    /// `paths` を HEAD の状態に戻す（index だけ）。HEAD がなければ index から外す。
    pub fn unstage(&self, paths: &[&str]) -> Result<()> {
        let has_head = self.read(&["rev-parse", "-q", "--verify", "HEAD"]).is_ok();
        let mut args = if has_head {
            vec!["reset", "-q", "--"]
        } else {
            vec!["rm", "--cached", "-q", "-r", "--"]
        };
        args.extend(paths);
        self.write(&args).map(drop)
    }

    /// `path` の index の中身を `content` にする（行単位の stage・unstage に使う）。
    pub fn set_index_content(&self, path: &str, mode: u32, content: &[u8]) -> Result<()> {
        // 作業ツリーのファイルではないので、改行の変換などのフィルターは通さない。
        let oid =
            self.write_with_input(&["hash-object", "-w", "--no-filters", "--stdin"], content)?;
        let oid = String::from_utf8(oid)?;
        let entry = format!("{mode:o},{},{path}", oid.trim());
        self.write(&["update-index", "--add", "--cacheinfo", &entry])
            .map(drop)
    }

    /// stage した変更をコミットする。`amend` なら直前のコミットを作り直す。
    pub fn commit(&self, message: &str, amend: bool) -> Result<()> {
        let mut args = vec!["commit", "-q", "--cleanup=strip", "-F", "-"];
        if amend {
            args.push("--amend");
        }
        self.write_with_input(&args, message.as_bytes()).map(drop)
    }
}

/// 起動した git を、その子のプロセス（`git-remote-https`・ssh など）ごと止める。子が残ると、出力のパイプが閉じずに
/// 読み取りが終わらない。Unix はプロセスグループ（起動するときに分けてある）に、Windows は `taskkill /T` で。
pub(crate) fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    // SAFETY: 自分で起動したプロセスグループ（pid = グループの番号）にシグナルを送るだけ。
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    let _ = amonhen_model::process::command("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = child.kill();
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する
mod tests {
    use super::*;

    #[test]
    fn reads_the_git_version() {
        assert_eq!(
            parse_git_version("git version 2.39.5 (Apple Git-154)\n"),
            Some((2, 39, 5))
        );
        assert_eq!(
            parse_git_version("git version 2.45.1.windows.1"),
            Some((2, 45, 1))
        );
        assert_eq!(parse_git_version("git version 1.8.3.1"), Some((1, 8, 3)));
        assert_eq!(parse_git_version("git version 2.50"), Some((2, 50, 0)));
        assert_eq!(parse_git_version("usage: git"), None);
        assert!(git_version().is_some_and(|(major, minor, _)| (major, minor) >= MIN_GIT_VERSION));
    }

    fn repo() -> (tempfile::TempDir, Git) {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new(tmp.path());
        for args in [
            &["init", "-q"][..],
            &["symbolic-ref", "HEAD", "refs/heads/main"],
            &["config", "user.name", "a"],
            &["config", "user.email", "a@example.com"],
            // Windows の CI では autocrlf が有効で、取り出したファイルの改行が変わる。
            &["config", "core.autocrlf", "false"],
        ] {
            git.write(args).unwrap();
        }
        (tmp, git)
    }

    #[test]
    fn clones_into_a_new_folder() {
        let (tmp, git) = repo();
        std::fs::write(tmp.path().join("a.txt"), "1\n").unwrap();
        git.stage(&["a.txt"]).unwrap();
        git.commit("first", false).unwrap();
        let out = tempfile::tempdir().unwrap();
        let dest = out.path().join("deep/copy");
        clone(tmp.path().to_str().unwrap(), &dest).unwrap();
        // Windows の CI では、clone の取り出しで改行が \r\n になる（全体の autocrlf）。
        assert_eq!(
            std::fs::read_to_string(dest.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "1\n"
        );
        // もうあるフォルダには clone できない。
        assert!(clone(tmp.path().to_str().unwrap(), &dest).is_err());
    }

    #[test]
    fn peeks_branch_changes_and_tracking() {
        let (tmp, git) = repo();
        std::fs::write(tmp.path().join("a.txt"), "1\n").unwrap();
        git.stage(&["a.txt"]).unwrap();
        git.commit("first", false).unwrap();
        // 上流を作って 1 つ先に進める。
        git.write(&["branch", "up"]).unwrap();
        git.write(&["branch", "--set-upstream-to", "up"]).unwrap();
        std::fs::write(tmp.path().join("a.txt"), "2\n").unwrap();
        git.stage(&["a.txt"]).unwrap();
        git.commit("second", false).unwrap();
        std::fs::write(tmp.path().join("a.txt"), "3\n").unwrap();
        std::fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();
        let peek = git.peek().unwrap();
        assert_eq!(peek.branch.as_deref(), Some("main"));
        assert!(peek.upstream);
        assert_eq!((peek.ahead, peek.behind, peek.changed), (1, 0, 1));
        assert_eq!(peek.last_commit.map(|(_, s)| s).as_deref(), Some("second"));
    }

    #[test]
    fn stages_unstages_and_commits() {
        let (tmp, git) = repo();
        std::fs::write(tmp.path().join("a.txt"), "1\n").unwrap();
        assert_eq!(
            git.status().unwrap().entries[0].kind,
            amonhen_model::StatusKind::Untracked
        );

        git.stage(&["a.txt"]).unwrap();
        assert!(git.status().unwrap().entries[0].is_staged());
        // HEAD がなくても外せる。
        git.unstage(&["a.txt"]).unwrap();
        assert!(!git.status().unwrap().entries[0].is_staged());

        git.stage(&["a.txt"]).unwrap();
        git.commit("first\n\nbody", false).unwrap();
        assert!(git.status().unwrap().entries.is_empty());
        let log = git.read(&["log", "--format=%B"]).unwrap();
        assert_eq!(String::from_utf8(log).unwrap().trim(), "first\n\nbody");

        std::fs::write(tmp.path().join("a.txt"), "1\n2\n3\n").unwrap();
        git.set_index_content("a.txt", 0o100644, b"1\n2\n").unwrap();
        let s = git.status().unwrap();
        assert_eq!((s.entries[0].index, s.entries[0].worktree), (b'M', b'M'));
        let staged = git.read(&["show", ":a.txt"]).unwrap();
        assert_eq!(staged, b"1\n2\n");

        git.commit("second", false).unwrap();
        git.commit("second (amended)", true).unwrap();
        let log = git.read(&["log", "--format=%s"]).unwrap();
        assert_eq!(String::from_utf8(log).unwrap(), "second (amended)\nfirst\n");
    }
}

#[cfg(test)]
mod ops_tests {
    use super::*;
    use amonhen_model::{HeadState, Oid, Operations, Outcome, ResetMode, Sequence};

    fn repo() -> (tempfile::TempDir, Git) {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new(tmp.path());
        for args in [
            &["init", "-q"][..],
            &["symbolic-ref", "HEAD", "refs/heads/main"],
            &["config", "user.name", "a"],
            &["config", "user.email", "a@example.com"],
            // Windows の CI では autocrlf が有効で、取り出したファイルの改行が変わる。
            &["config", "core.autocrlf", "false"],
        ] {
            git.write(args).unwrap();
        }
        (tmp, git)
    }

    fn commit_file(tmp: &tempfile::TempDir, git: &Git, name: &str, content: &str) -> Oid {
        std::fs::write(tmp.path().join(name), content).unwrap();
        git.stage(&[name]).unwrap();
        git.commit(&format!("{name}: {content}"), false).unwrap();
        git.head_state().unwrap().oid.unwrap()
    }

    #[test]
    fn merges_rebases_and_aborts_conflicts() {
        let (tmp, git) = repo();
        let base = commit_file(&tmp, &git, "a.txt", "base\n");
        assert_eq!(
            git.head_state().unwrap(),
            HeadState {
                branch: Some("refs/heads/main".into()),
                oid: Some(base)
            }
        );

        git.create_branch("topic", "main", true).unwrap();
        commit_file(&tmp, &git, "a.txt", "topic\n");
        git.switch_branch("main").unwrap();
        commit_file(&tmp, &git, "a.txt", "main\n");

        assert_eq!(git.merge("topic").unwrap(), Outcome::Conflict);
        assert_eq!(git.sequence_in_progress().unwrap(), Some(Sequence::Merge));
        git.abort(Sequence::Merge).unwrap();
        assert_eq!(git.sequence_in_progress().unwrap(), None);

        commit_file(&tmp, &git, "b.txt", "b\n");
        git.switch_branch("topic").unwrap();
        assert_eq!(git.rebase("main").unwrap(), Outcome::Conflict);
        assert_eq!(git.sequence_in_progress().unwrap(), Some(Sequence::Rebase));
        git.abort(Sequence::Rebase).unwrap();
    }

    #[test]
    fn predicts_the_files_a_merge_would_conflict_in() {
        let (tmp, git) = repo();
        commit_file(&tmp, &git, "a.txt", "base\n");
        commit_file(&tmp, &git, "b c.txt", "base\n");
        git.create_branch("topic", "main", true).unwrap();
        commit_file(&tmp, &git, "a.txt", "topic\n");
        commit_file(&tmp, &git, "b c.txt", "topic\n");
        commit_file(&tmp, &git, "t.txt", "topic\n");
        git.switch_branch("main").unwrap();
        commit_file(&tmp, &git, "a.txt", "main\n");
        commit_file(&tmp, &git, "b c.txt", "main\n");
        let head = git.head_state().unwrap();

        let predicted = git.predict_merge("main", "topic").unwrap();
        let clean = git.predict_merge("topic", "topic~1").unwrap();
        if git_at_least(2, 38) {
            assert_eq!(predicted, Some(vec!["a.txt".into(), "b c.txt".into()]));
            assert_eq!(clean, Some(vec![]));
            assert!(git.predict_merge("main", "nothing").is_err());
        } else {
            assert_eq!(
                predicted, None,
                "merge-tree --write-tree がない git では調べない"
            );
            assert_eq!(clean, None);
        }
        // 作業ツリーと index、HEAD はそのまま。
        assert_eq!(git.head_state().unwrap(), head);
        assert!(git.status().unwrap().entries.is_empty());
        assert_eq!(git.sequence_in_progress().unwrap(), None);
    }

    #[test]
    fn moves_refs_only_when_they_are_where_we_expect() {
        let (tmp, git) = repo();
        let first = commit_file(&tmp, &git, "a.txt", "1\n");
        let second = commit_file(&tmp, &git, "a.txt", "2\n");
        let name = "refs/heads/other".to_string();

        git.update_ref(&name, Some(&first), None).unwrap();
        assert!(
            git.update_ref(&name, Some(&second), Some(&second)).is_err(),
            "期待した値と違う"
        );
        git.update_ref(&name, Some(&second), Some(&first)).unwrap();
        assert_eq!(
            git.resolve_refs(std::slice::from_ref(&name)).unwrap(),
            [(name.clone(), Some(second))]
        );
        git.update_ref(&name, None, Some(&second)).unwrap();
        assert_eq!(
            git.resolve_refs(std::slice::from_ref(&name)).unwrap(),
            [(name, None)]
        );

        git.reset(ResetMode::Soft, &first).unwrap();
        assert_eq!(git.head_state().unwrap().oid, Some(first));
        assert!(
            git.status().unwrap().entries[0].is_staged(),
            "soft なので変更は index に残る"
        );
    }

    #[test]
    fn drops_and_restores_stashes() {
        let (tmp, git) = repo();
        commit_file(&tmp, &git, "a.txt", "1\n");
        std::fs::write(tmp.path().join("a.txt"), "changed\n").unwrap();
        git.stash_push(Some("wip"), false).unwrap();
        let oid =
            Oid::from_hex(&text(git.read(&["rev-parse", "stash@{0}"]).unwrap()).unwrap()).unwrap();
        git.stash_drop("stash@{0}").unwrap();
        assert!(
            git.read(&["rev-parse", "-q", "--verify", "refs/stash"])
                .is_err()
        );

        git.stash_store(&oid, "wip").unwrap();
        assert_eq!(git.stash_apply("stash@{0}", true).unwrap(), Outcome::Done);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "changed\n"
        );
    }

    fn text(out: Vec<u8>) -> Result<String> {
        Ok(String::from_utf8(out)?.trim().to_owned())
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する
mod streaming_tests {
    use super::*;

    #[test]
    fn cancels_a_running_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(dir)
                .status()
                .unwrap()
                .success()
        );
        // いつまでも返らないリモート（ext:: で sleep を起動する）。
        let mut cmd = Command::new("git");
        cmd.current_dir(dir).args([
            "-c",
            "protocol.ext.allow=always",
            "fetch",
            "ext::sleep 30",
            "main",
        ]);
        let started = Instant::now();
        let handle = std::thread::spawn(move || run(cmd, None));
        let seq = loop {
            if let Some(r) = amonhen_model::activity::records()
                .into_iter()
                .find(|r| r.label.contains("ext::sleep") && r.cancellable && r.ok.is_none())
            {
                break r.seq;
            }
            assert!(started.elapsed().as_secs() < 10, "記録が出ない");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(amonhen_model::activity::cancel(seq));
        let result = handle.join().unwrap();
        assert!(amonhen_model::i18n::is_cancelled(
            &result.unwrap_err().to_string()
        ));
        assert!(started.elapsed().as_secs() < 10);
        let record = amonhen_model::activity::records()
            .into_iter()
            .find(|r| r.seq == seq)
            .unwrap();
        assert_eq!(record.ok, Some(false));
    }
}
