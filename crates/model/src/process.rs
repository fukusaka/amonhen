//! 子プロセスの起動。

use std::ffi::{OsStr, OsString};
use std::process::Command;

/// `program` を起動する `Command`。Windows では窓を出さない（`CREATE_NO_WINDOW`）。Unix では、子のシグナルマスクを
/// 空にする。
///
/// 共有の daemon はコンソールを持たない（`DETACHED_PROCESS` で起動する）。コンソールを持たないプロセスが
/// git や ssh のようなコンソールのプログラムを起動すると、Windows はそのたびに新しいコンソールの窓を開く。
///
/// Rust の `Command` は、起動したスレッドのシグナルマスクをそのまま子に渡す。macOS のアプリ（AppKit・Swift
/// Concurrency）のスレッドは SIGCHLD などを塞いでいるので、そこから起動した git も、git が起動する git-remote-https や
/// credential helper も、塞いだまま動く。Git Credential Manager（.NET）は子の終了を SIGCHLD で知るので、起動した
/// `git version` が終わっても気づかず、HTTPS の push・fetch・pull が永遠に終わらなかった（端末からは通る）。
/// アプリが起動した daemon も同じマスクを受け継ぐ。塞がれたシグナルがあるときだけ、fork と exec の間でマスクを
/// 空にする（`pre_exec` があると posix_spawn の速い経路を使えないので、要らないときは付けない）。
pub fn command(program: impl AsRef<OsStr>) -> Command {
    // ここだけが Command::new を直接呼ぶ（clippy.toml の disallowed-methods）
    #[allow(unused_mut, clippy::disallowed_methods)]
    let mut cmd = Command::new(if program.as_ref() == "git" {
        git_program()
    } else {
        program.as_ref().to_owned()
    });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    if any_signal_blocked() {
        use std::os::unix::process::CommandExt;
        // SAFETY: fork と exec の間で、ゼロ埋めの集合（空）を pthread_sigmask に渡すだけ。どちらも
        // async-signal-safe で、メモリを確保しない。
        unsafe {
            cmd.pre_exec(|| {
                let empty: libc::sigset_t = std::mem::zeroed();
                libc::pthread_sigmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
                Ok(())
            });
        }
    }
    client::apply(&mut cmd);
    #[cfg(unix)]
    if let Some(agent) = agent::replacement() {
        cmd.env("SSH_AUTH_SOCK", agent);
    }
    cmd
}

/// 使う git。`AMONHEN_GIT`（実行ファイルのパス。アプリの Settings > Git で選んだもの）があればそれ、なければ
/// `PATH` の `git`。共有の daemon では、要求した FE の値（`with_client_env`）。
pub fn git_program() -> std::ffi::OsString {
    resolve_git(var_os("AMONHEN_GIT"))
}

fn resolve_git(chosen: Option<std::ffi::OsString>) -> std::ffi::OsString {
    chosen
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "git".into())
}

/// 共有の daemon につないだ FE の環境のうち、その FE のために動かす git・ssh に渡すもの。
///
/// 共有の daemon は、最初に起動した FE（アプリか VSCode 拡張か CLI）の環境を終わるまで持つ。そのままでは、資格情報を
/// 尋ねるダイアログ（`AMONHEN_ASKPASS`。アプリだけが持つ）・使う git（`PATH`）・SSH の agent・`GIT_*` の設定が、
/// どの FE が先に起動したかで変わる。FE はつなぐときにこれを送り（`amonhen_remote::local::opening`）、daemon はその
/// FE の要求を `with_client_env` の中で動かす。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientEnv {
    vars: Vec<(String, String)>,
}

impl ClientEnv {
    /// このプロセスの環境から、渡すもの。
    pub fn current() -> Self {
        Self::from_vars(std::env::vars())
    }

    /// `vars` のうち、渡すもの（名前の順）。
    pub fn from_vars(vars: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut vars: Vec<_> = vars
            .into_iter()
            .filter(|(k, _)| client::passes(k))
            .collect();
        vars.sort();
        vars.dedup_by(|a, b| a.0 == b.0);
        Self { vars }
    }

    /// `NAME=value` を NUL で区切ったもの。
    pub fn encode(&self) -> String {
        self.vars
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("\0")
    }

    /// `encode` の形（VSCode 拡張は環境をすべて送る）から、渡すものだけを読む。
    pub fn decode(text: &str) -> Self {
        Self::from_vars(text.split('\0').filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.to_owned(), v.to_owned()))
        }))
    }

    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// `f` の間、このスレッドで起動する子に `env` を渡す（共有の daemon が、つないだ FE の要求を動かすとき）。`env` が
/// 空なら（前の版の FE・SSH 越し）、daemon の環境のまま。
pub fn with_client_env<R>(env: &ClientEnv, f: impl FnOnce() -> R) -> R {
    if env.is_empty() {
        return f();
    }
    let previous = client::CURRENT.with(|c| c.replace(Some(env.clone())));
    struct Restore(Option<ClientEnv>);
    impl Drop for Restore {
        fn drop(&mut self) {
            client::CURRENT.with(|c| *c.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(previous);
    f()
}

/// 環境変数 `name`。`with_client_env` の中で、FE が送る名前なら、その FE の値（送っていなければ None）。
pub fn var_os(name: &str) -> Option<OsString> {
    client::CURRENT.with(|c| match &*c.borrow() {
        Some(env) if client::passes(name) => env.get(name).map(OsString::from),
        _ => std::env::var_os(name),
    })
}

mod client {
    use super::ClientEnv;
    use std::cell::RefCell;
    use std::process::Command;

    thread_local! {
        pub(super) static CURRENT: RefCell<Option<ClientEnv>> = const { RefCell::new(None) };
    }

    /// FE から受け取る名前。
    const NAMES: [&str; 6] = [
        "PATH",
        "AMONHEN_ASKPASS",
        "AMONHEN_GIT",
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "XDG_CONFIG_HOME",
    ];

    /// `GIT_` で始まるもののうち、受け取らないもの: リポジトリを指すもの（git のフックから起動した CLI などは持って
    /// いる。渡すと別のリポジトリを操作してしまう）と、askpass（FE の askpass は `AMONHEN_ASKPASS` で決める。
    /// VSCode の askpass はその窓口の変数がないと動かない）。
    const NOT_GIT: [&str; 10] = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
        "GIT_QUARANTINE_PATH",
        "GIT_ASKPASS",
    ];

    pub(super) fn passes(name: &str) -> bool {
        NAMES.contains(&name) || (name.starts_with("GIT_") && !NOT_GIT.contains(&name))
    }

    /// `with_client_env` の中なら、FE の値を渡し、daemon だけが持つ（FE が送っていない）ものは渡さない。PATH は、
    /// 送っていなければ daemon のまま（なければ git を見つけられない）。
    pub(super) fn apply(cmd: &mut Command) {
        CURRENT.with(|c| {
            let Some(env) = &*c.borrow() else { return };
            for (k, _) in std::env::vars_os() {
                if let Some(k) = k.to_str()
                    && passes(k)
                    && k != "PATH"
                    && env.get(k).is_none()
                {
                    cmd.env_remove(k);
                }
            }
            for (k, v) in &env.vars {
                cmd.env(k, v);
            }
        });
    }
}

/// 環境の `SSH_AUTH_SOCK` が死んでいたときに、そのログインの SSH の agent のソケットを探す手を登録する（FE・daemon が
/// 起動したときに 1 回。macOS なら launchd が用意する agent）。探し方は OS ごとなので、ここは確かめるところまで。
pub fn set_agent_finder(find: impl Fn() -> Option<OsString> + Send + Sync + 'static) {
    #[cfg(unix)]
    let _ = agent::FINDER.set(Box::new(find));
    #[cfg(not(unix))]
    let _ = find;
}

/// 公開鍵で断られた失敗の文に、agent がいないことを添える（agent の鍵が使えなかったのが原因かもしれない。ssh は
/// パスフレーズを尋ねられない）。agent がいる・ほかの失敗なら、そのまま返す。文の頭は git・ssh の英語のままにする
/// （`amonhen_core::failure` が見分ける）。
pub fn explain_agent(message: String) -> String {
    if !message.contains("Permission denied (publickey") {
        return message;
    }
    #[cfg(unix)]
    let note = match var_os("SSH_AUTH_SOCK") {
        None => Some(crate::tr!(
            "No SSH agent is set (SSH_AUTH_SOCK), so keys that need a passphrase can’t be used"
        )),
        Some(path) if !agent::is_socket(&path) && agent::replacement().is_none() => {
            Some(crate::tr!(
                "The SSH agent ({path}) isn’t running, so keys that need a passphrase can’t be used",
                path = std::path::Path::new(&path).display()
            ))
        }
        Some(_) => None,
    };
    #[cfg(not(unix))]
    let note: Option<String> = None;
    match note {
        Some(note) => format!("{message}\n{note}"),
        None => message,
    }
}

/// 子に渡す SSH の agent。
///
/// 環境の `SSH_AUTH_SOCK` は古くなっていることがある。agent のソケットはログインのたびに変わるのに、アプリは前回の
/// 起動で読んだログインシェルの環境を使い、共有の daemon は起動した FE の環境を終わるまで持ち続ける。死んだ
/// ソケットを渡すと、ssh は agent の鍵を使えず（パスフレーズは尋ねられない）、端末では通る接続が公開鍵で断られる。
/// そこで子を起動するたびに確かめ、死んでいれば登録された手で探した生きているものを渡す（再起動しなくても直る）。
#[cfg(unix)]
mod agent {
    use std::cell::Cell;
    use std::ffi::{OsStr, OsString};
    use std::os::unix::fs::FileTypeExt;
    use std::sync::{Mutex, OnceLock};

    pub(super) type Finder = Box<dyn Fn() -> Option<OsString> + Send + Sync>;

    pub(super) static FINDER: OnceLock<Finder> = OnceLock::new();

    /// 前に見つけた（死んでいた値, 代わりの生きている値）。死んだ値のままの間は、探し直さずにこれを使う。
    static FOUND: Mutex<Option<(OsString, OsString)>> = Mutex::new(None);

    thread_local! {
        /// 探している途中（探す手が子を起動する launchctl で、また探し始めないように）。
        static FINDING: Cell<bool> = const { Cell::new(false) };
    }

    /// 環境の値が死んでいれば、代わりに渡すもの。生きている・値がない・代わりがなければ None（環境のまま）。
    pub(super) fn replacement() -> Option<OsString> {
        let current = super::var_os("SSH_AUTH_SOCK")?;
        if is_socket(&current) || FINDING.with(Cell::get) {
            return None;
        }
        let find = FINDER.get()?;
        let _finding = Finding::start();
        resolve(&current, find, &FOUND)
    }

    /// `current`（死んでいる）の代わり。`found` に前の結果を覚える。
    pub(super) fn resolve(
        current: &OsStr,
        find: &dyn Fn() -> Option<OsString>,
        found: &Mutex<Option<(OsString, OsString)>>,
    ) -> Option<OsString> {
        let mut found = found.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((dead, live)) = &*found
            && dead == current
            && is_socket(live)
        {
            return Some(live.clone());
        }
        let live = find().filter(|live| live != current && is_socket(live))?;
        *found = Some((current.to_owned(), live.clone()));
        Some(live)
    }

    /// ソケットのファイルがあるか（つないで確かめると、launchd が agent を起こしてしまう）。ログインの間だけの
    /// agent（launchd の `/var/run/com.apple.launchd.*`）は、ログインが終わるとフォルダごと消える。
    pub(super) fn is_socket(path: &OsStr) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.file_type().is_socket())
    }

    /// 探している印。抜けるとき（探す手が panic しても）外す。
    struct Finding;

    impl Finding {
        fn start() -> Self {
            FINDING.with(|f| f.set(true));
            Self
        }
    }

    impl Drop for Finding {
        fn drop(&mut self) {
            FINDING.with(|f| f.set(false));
        }
    }
}

/// このスレッドのシグナルマスクに、塞がれたシグナルがあるか。
#[cfg(unix)]
fn any_signal_blocked() -> bool {
    // SAFETY: 今のマスクを読むだけ（`set` が NULL なら `how` は無視される）。
    unsafe {
        let mut current: libc::sigset_t = std::mem::zeroed();
        if libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut current) != 0 {
            return false;
        }
        (1..libc::SIGXFSZ + 1).any(|signal| libc::sigismember(&current, signal) == 1)
    }
}

#[test]
fn chooses_the_git_from_the_environment() {
    assert_eq!(resolve_git(None), "git");
    assert_eq!(resolve_git(Some("".into())), "git");
    assert_eq!(
        resolve_git(Some("/opt/homebrew/bin/git".into())),
        "/opt/homebrew/bin/git"
    );
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// AppKit のスレッドのように SIGTERM を塞いだスレッドから起動しても、子は塞がれていない（自分に送った
    /// SIGTERM で死ぬ）。
    #[test]
    fn child_starts_with_an_empty_signal_mask() {
        use std::os::unix::process::ExitStatusExt;
        // SAFETY: このテストのスレッドのマスクに SIGTERM を足し、終わったら戻す。
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGTERM);
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        }
        assert!(any_signal_blocked());
        let output = command("sh")
            .args(["-c", "kill -TERM $$; echo survived"])
            .output()
            .expect("sh");
        unsafe {
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        }
        assert_eq!(
            output.status.signal(),
            Some(libc::SIGTERM),
            "the child inherited the blocked SIGTERM: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    /// 環境の agent が死んでいれば、探す手が見つけた生きているものを渡す。死んだ値のままの間は探し直さない。
    #[test]
    fn replaces_a_dead_agent_with_a_live_one() {
        use std::cell::Cell;
        use std::os::unix::net::UnixListener;
        use std::sync::Mutex;
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.sock");
        let _listener = UnixListener::bind(&live).unwrap();
        let dead = dir.path().join("gone/Listeners");
        assert!(agent::is_socket(live.as_os_str()));
        assert!(!agent::is_socket(dead.as_os_str()));

        let calls = Cell::new(0);
        let find = || {
            calls.set(calls.get() + 1);
            Some(live.clone().into_os_string())
        };
        let found = Mutex::new(None);
        for _ in 0..2 {
            assert_eq!(
                agent::resolve(dead.as_os_str(), &find, &found),
                Some(live.clone().into_os_string())
            );
        }
        assert_eq!(calls.get(), 1);

        // 探す手の答えも死んでいれば、差し替えない。
        let nothing = Mutex::new(None);
        let other = dir.path().join("other.sock").into_os_string();
        assert_eq!(
            agent::resolve(dead.as_os_str(), &|| Some(other.clone()), &nothing),
            None
        );
    }

    /// 公開鍵で断られた失敗だけに添え、元の文は頭にそのまま残す（失敗の見分けが元の英語で効くように）。
    #[test]
    fn explains_only_public_key_refusals() {
        let other = "fatal: Could not resolve hostname x".to_owned();
        assert_eq!(explain_agent(other.clone()), other);
        let refused = "git@example.com: Permission denied (publickey).".to_owned();
        assert!(explain_agent(refused.clone()).starts_with(&refused));
    }

    /// FE から受け取るのは git・ssh に関わる名前だけ。リポジトリを指す `GIT_*` と `GIT_ASKPASS` は受け取らない。
    #[test]
    fn passes_only_what_git_and_ssh_use() {
        let env = ClientEnv::decode(
            "PATH=/fe/bin\0HOME=/home/x\0GIT_SSH_COMMAND=ssh -v\0GIT_DIR=/elsewhere/.git\0GIT_ASKPASS=/vscode/askpass.sh\0AMONHEN_ASKPASS=/app/askpass\0SECRET_TOKEN=x\0broken",
        );
        assert_eq!(
            env.encode(),
            "AMONHEN_ASKPASS=/app/askpass\0GIT_SSH_COMMAND=ssh -v\0PATH=/fe/bin"
        );
        assert_eq!(ClientEnv::decode(&env.encode()), env);
        assert!(ClientEnv::decode("").is_empty());
    }

    /// `with_client_env` の中で起動した子は、FE の環境で動く（`PATH` で探し、FE の値を受け取る）。外では daemon の
    /// 環境のまま。
    #[test]
    fn children_get_the_client_environment() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("amonhen-client-env-test");
        std::fs::write(
            &program,
            "#!/bin/sh\necho \"$GIT_AMONHEN_TEST|$XDG_CONFIG_HOME\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = ClientEnv::from_vars([
            (
                "PATH".to_owned(),
                format!("{}:/usr/bin:/bin", dir.path().display()),
            ),
            ("GIT_AMONHEN_TEST".to_owned(), "from the client".to_owned()),
        ]);
        let out = with_client_env(&env, || {
            assert_eq!(
                var_os("GIT_AMONHEN_TEST").as_deref(),
                Some(std::ffi::OsStr::new("from the client"))
            );
            // FE が送っていない名前は、daemon が持っていても渡さない。
            assert_eq!(var_os("XDG_CONFIG_HOME"), None);
            command("amonhen-client-env-test").output().unwrap()
        });
        assert_eq!(String::from_utf8_lossy(&out.stdout), "from the client|\n");
        assert!(
            command("amonhen-client-env-test").output().is_err(),
            "outside it, the daemon's PATH"
        );
    }
}
