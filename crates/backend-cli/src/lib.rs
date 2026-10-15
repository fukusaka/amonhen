//! `git` を起動して行う操作（デスクトップ向け）。書き込み系の操作は正しさと互換性を優先して
//! git 本体に任せる。読み取りでも、git の仕組み（untracked cache・fsmonitor）が効く status はここ。

mod extras;
mod ops;
mod status;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use amonhen_model::Result;

pub use status::parse_status;

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

/// 1 つのリポジトリで git を実行する。
#[derive(Clone, Debug)]
pub struct Git {
    workdir: PathBuf,
    /// status の結果を最後に index に書き残した時刻。
    status_saved: Arc<Mutex<Option<Instant>>>,
}

impl Git {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
            status_saved: Arc::default(),
        }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub(crate) fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.workdir)
            // 利用者の端末に依存しない出力にする。
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["-c", "core.quotepath=off", "-c", "color.ui=false"])
            .args(args);
        cmd
    }

    /// 読み取り系。利用者自身の git 操作とロックを奪い合わないよう、任意のロックを取らない。
    pub fn read(&self, args: &[&str]) -> Result<Vec<u8>> {
        let mut cmd = self.command(args);
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
        run(cmd, None)
    }

    /// 書き込み系。
    pub fn write(&self, args: &[&str]) -> Result<Vec<u8>> {
        run(self.command(args), None)
    }

    /// 書き込み系で、標準入力を渡す。
    pub fn write_with_input(&self, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
        run(self.command(args), Some(input))
    }
}

fn run(mut cmd: Command, input: Option<&[u8]>) -> Result<Vec<u8>> {
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
            format!("git が失敗した: {:?}", out.status)
        } else {
            message
        }
        .into());
    }
    Ok(out.stdout)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> (tempfile::TempDir, Git) {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new(tmp.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
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
            &["init", "-q", "-b", "main"][..],
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
