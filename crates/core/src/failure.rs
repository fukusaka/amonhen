//! git の失敗の見分け。エラーの文（git の stderr、`LC_ALL=C` で英語）から、FE が要点と次の一手を出せる種類を
//! 決める。見分けられなければ `None`（FE は git の文をそのまま出す）。
//!
//! 文で見分けるので、SSH 先・共有の daemon の先で起きた失敗（文字列で届く）にも同じように効く。

/// 失敗の種類と、それに関わるパス（ほかの worktree の場所、ロックファイル）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: &'static str,
    pub path: Option<String>,
}

impl Failure {
    fn new(kind: &'static str) -> Self {
        Self { kind, path: None }
    }
}

/// ほかの worktree でチェックアウトされているブランチ。
pub const WORKTREE_IN_USE: &str = "worktreeInUse";
/// 認証に通らない（HTTPS の資格情報、SSH の鍵）。
pub const AUTH: &str = "auth";
/// SSH の接続先のホスト鍵を確かめられない（known_hosts にない・変わった）。
pub const HOST_KEY: &str = "hostKey";
/// リモートにつながらない（名前を引けない、つながらない、時間切れ）。
pub const NETWORK: &str = "network";
/// push が断られた（リモートに手元にないコミットがある）。
pub const REJECTED: &str = "rejected";
/// fast-forward できない（手元と上流の両方に新しいコミットがある）。
pub const DIVERGED: &str = "diverged";
/// コミットしていない変更（や追跡していないファイル）を上書きしてしまうので止めた。
pub const LOCAL_CHANGES: &str = "localChanges";
/// ほかの git が動いている（ロックファイルがある）。
pub const LOCKED: &str = "locked";
/// 上流が決まっていない。
pub const NO_UPSTREAM: &str = "noUpstream";

/// `message` の失敗の種類。
pub fn classify(message: &str) -> Option<Failure> {
    let has = |s: &str| message.contains(s);
    if let Some(path) = quoted_after(message, "is already used by worktree at ")
        .or_else(|| quoted_after(message, "is already checked out at "))
    {
        return Some(Failure {
            kind: WORKTREE_IN_USE,
            path: Some(path),
        });
    }
    if has("Unable to create '") && has(".lock': File exists") {
        return Some(Failure {
            kind: LOCKED,
            path: quoted_after(message, "Unable to create "),
        });
    }
    if has("Host key verification failed") || has("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        return Some(Failure::new(HOST_KEY));
    }
    if has("could not read Username")
        || has("could not read Password")
        || has("Authentication failed")
        || has("Invalid username or password")
        || has("Permission denied (publickey")
        || has("The requested URL returned error: 401")
        || has("The requested URL returned error: 403")
        || (has("remote: Permission to") && has("denied"))
    {
        return Some(Failure::new(AUTH));
    }
    if has("Could not resolve host")
        || has("Could not resolve hostname")
        || has("Connection refused")
        || has("Connection timed out")
        || has("Operation timed out")
        || has("Network is unreachable")
        || has("Failed to connect to")
    {
        return Some(Failure::new(NETWORK));
    }
    if has("[rejected]") || has("Updates were rejected") {
        return Some(Failure::new(REJECTED));
    }
    if has("Not possible to fast-forward") || has("Diverging branches can't be fast-forwarded") {
        return Some(Failure::new(DIVERGED));
    }
    if has("Your local changes to the following files would be overwritten")
        || has("untracked working tree files would be overwritten")
    {
        return Some(Failure::new(LOCAL_CHANGES));
    }
    if has("There is no tracking information for the current branch")
        || has("has no upstream branch")
    {
        return Some(Failure::new(NO_UPSTREAM));
    }
    None
}

/// `marker` のすぐ後の `'…'` の中身。
fn quoted_after(message: &str, marker: &str) -> Option<String> {
    let rest = &message[message.find(marker)? + marker.len()..];
    let rest = rest.strip_prefix('\'')?;
    Some(rest[..rest.find('\'')?].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(message: &str) -> Option<&'static str> {
        classify(message).map(|f| f.kind)
    }

    #[test]
    fn finds_the_other_worktree() {
        let new = classify("fatal: 'topic' is already used by worktree at '/r/wt'").unwrap();
        assert_eq!(
            (new.kind, new.path.as_deref()),
            (WORKTREE_IN_USE, Some("/r/wt"))
        );
        let old = classify("fatal: 'topic' is already checked out at '/r/a b'").unwrap();
        assert_eq!(old.path.as_deref(), Some("/r/a b"));
    }

    #[test]
    fn finds_the_lock_file() {
        let f = classify(
            "fatal: Unable to create '/r/.git/index.lock': File exists.\n\nAnother git process seems to be running",
        )
        .unwrap();
        assert_eq!(
            (f.kind, f.path.as_deref()),
            (LOCKED, Some("/r/.git/index.lock"))
        );
    }

    #[test]
    fn tells_network_failures_apart() {
        assert_eq!(
            kind(
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
            ),
            Some(AUTH)
        );
        assert_eq!(
            kind(
                "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository."
            ),
            Some(AUTH)
        );
        assert_eq!(
            kind("Host key verification failed.\nfatal: Could not read from remote repository."),
            Some(HOST_KEY)
        );
        assert_eq!(
            kind("fatal: unable to access 'https://x/': Could not resolve host: x"),
            Some(NETWORK)
        );
        assert_eq!(
            kind(
                " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs"
            ),
            Some(REJECTED)
        );
        assert_eq!(
            kind("fatal: Not possible to fast-forward, aborting."),
            Some(DIVERGED)
        );
    }

    #[test]
    fn finds_local_changes_in_the_way() {
        assert_eq!(
            kind(
                "error: Your local changes to the following files would be overwritten by checkout:\n\ta.txt"
            ),
            Some(LOCAL_CHANGES)
        );
        assert_eq!(kind("fatal: something else"), None);
    }
}
