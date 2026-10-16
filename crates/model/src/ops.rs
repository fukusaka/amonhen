use crate::{Oid, Result};

/// HEAD の状態。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HeadState {
    /// チェックアウト中のブランチの完全な名前（`refs/heads/main`）。detached なら `None`。
    pub branch: Option<String>,
    /// まだコミットがなければ `None`。
    pub oid: Option<Oid>,
}

/// pull での上流の取り込み方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum PullMode {
    /// merge する（fast-forward で済めばそうする）。
    #[default]
    Merge,
    Rebase,
    /// fast-forward で済むときだけ取り込む。
    FastForwardOnly,
}

/// git の設定の 1 項目（`git config --list --show-origin --show-scope`）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConfigEntry {
    /// "system"・"global"・"local"・"worktree"・"command"。
    pub scope: String,
    /// どこで決まっているか（`file:/Users/a/.gitconfig` など）。
    pub origin: String,
    pub key: String,
    /// 値のない項目（`[core] bare` のような真偽）は空。
    pub value: String,
}

/// リモート。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct RemoteEntry {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

/// リモートの変更。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(
        tag = "kind",
        rename_all = "camelCase",
        rename_all_fields = "camelCase"
    )
)]
pub enum RemoteChange {
    Add {
        name: String,
        url: String,
    },
    Rename {
        from: String,
        to: String,
    },
    SetUrl {
        name: String,
        url: String,
    },
    Remove {
        name: String,
    },
    /// リモートで消えたブランチの追跡を消す。
    Prune {
        name: String,
    },
}

/// 途中で止まりうる操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Sequence {
    Rebase,
    Merge,
    CherryPick,
    Revert,
}

impl Sequence {
    pub fn command(self) -> &'static str {
        match self {
            Sequence::Rebase => "rebase",
            Sequence::Merge => "merge",
            Sequence::CherryPick => "cherry-pick",
            Sequence::Revert => "revert",
        }
    }
}

/// merge・rebase などの結果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Outcome {
    Done,
    /// コンフリクトで止まった。
    Conflict,
    /// コンフリクトではなく途中で止まった（edit など）。
    Stopped,
}

/// Undo で現在のブランチを戻すときの reset の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResetMode {
    /// index と作業ツリーはそのまま（コミットの Undo）。
    Soft,
    /// 作業中の変更を残したまま戻す（merge・rebase の Undo）。
    Keep,
}

/// 書き込み系の操作。デスクトップは `git` を起動して行う。
pub trait Operations: Send + Sync {
    /// `tips` から辿れるコミットのうち、`path`（の下）を変えたもの、差分に `contents` を含む行があるもの
    /// （どちらも指定したときは両方を満たすもの）。`cancelled` が true を返したら途中でやめて `Err` を返す。
    fn log_matching(
        &self,
        tips: &[Oid],
        path: Option<&str>,
        contents: Option<&str>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<Oid>> {
        let _ = (tips, path, contents, cancelled);
        Err("この環境ではパス・内容で探せません".into())
    }

    /// `git blame --porcelain` の出力。`rev` がなければ作業ツリーの内容（まだコミットしていない行を含む）。
    fn blame(&self, path: &str, rev: Option<&Oid>) -> Result<Vec<u8>> {
        let _ = (path, rev);
        Err("この環境では blame できません".into())
    }

    /// HEAD から `path` を変えたコミット（名前の変更を追う）。`git log --follow -z --name-only` の、
    /// 各コミットが `\x01ID\0件名\0作者\0時刻\0\nその時点のパス\0` になる出力。
    fn file_log(&self, path: &str) -> Result<Vec<u8>> {
        let _ = path;
        Err("この環境ではファイルの履歴を読めません".into())
    }

    fn head_state(&self) -> Result<HeadState>;
    /// 完全な ref 名の値。なければ `None`。
    fn resolve_refs(&self, names: &[String]) -> Result<Vec<(String, Option<Oid>)>>;
    /// `expected` が今の値のときだけ書き換える（`None` は「まだない」）。`value` が `None` なら消す。
    fn update_ref(&self, name: &str, value: Option<&Oid>, expected: Option<&Oid>) -> Result<()>;
    fn reset(&self, mode: ResetMode, oid: &Oid) -> Result<()>;
    /// ブランチ（完全な名前）か、detached HEAD に切り替える。
    fn switch_to(&self, head: &HeadState) -> Result<()>;
    fn write_blob(&self, content: &[u8]) -> Result<Oid>;
    fn read_blob(&self, oid: &Oid) -> Result<Vec<u8>>;

    fn switch_branch(&self, name: &str) -> Result<()>;
    /// リモートブランチ（`origin/main`）を追跡するローカルブランチを作って切り替える。
    fn track_branch(&self, remote_branch: &str) -> Result<()>;
    fn create_branch(&self, name: &str, start: &str, checkout: bool) -> Result<()>;
    fn delete_branch(&self, name: &str, force: bool) -> Result<()>;
    fn rename_branch(&self, from: &str, to: &str) -> Result<()>;
    fn create_tag(&self, name: &str, target: &str, message: Option<&str>) -> Result<()>;
    /// ブランチ名として使えなければ理由。
    fn check_branch_name(&self, name: &str) -> Result<Option<String>>;

    /// `ancestor` が `descendant` の祖先か（fast-forward できるか）。
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool>;
    /// 名前（`main`・`origin/main`・コミット ID など）が指すコミット。
    fn resolve(&self, rev: &str) -> Result<Oid>;
    fn merge(&self, rev: &str) -> Result<Outcome>;
    fn rebase(&self, onto: &str) -> Result<Outcome>;
    fn cherry_pick(&self, oid: &Oid, is_merge: bool) -> Result<Outcome>;
    fn revert(&self, oid: &Oid, is_merge: bool) -> Result<Outcome>;
    /// 用意した todo で非対話の rebase を行う（コミットの編集）。`base` が `None` なら `--root`。
    fn rebase_todo(&self, base: Option<&Oid>, todo: &std::path::Path) -> Result<Outcome>;
    fn sequence_in_progress(&self) -> Result<Option<Sequence>>;
    /// rebase の途中なら、何番目か（今, 全体）。
    fn rebase_progress(&self) -> Result<Option<(u32, u32)>>;
    fn abort(&self, sequence: Sequence) -> Result<()>;
    fn continue_sequence(&self, sequence: Sequence) -> Result<Outcome>;
    fn skip_sequence(&self, sequence: Sequence) -> Result<Outcome>;

    /// コンフリクトしたファイルを片側（`ours` なら Current、そうでなければ Incoming）にして解決済みにする。
    /// その側でファイルが消えていれば、消して解決済みにする。
    fn take_side(&self, path: &str, ours: bool) -> Result<()>;

    fn fetch(&self) -> Result<()>;
    /// 今のブランチに上流を取り込む（`git pull`）。
    fn pull(&self, mode: PullMode) -> Result<Outcome>;
    /// ローカルブランチごとの、上流との差（名前, ahead, behind, gone）。
    fn branch_tracking(&self) -> Result<Vec<(String, u32, u32, bool)>>;
    fn push(
        &self,
        remote: &str,
        branch: &str,
        remote_branch: &str,
        force: bool,
        set_upstream: bool,
    ) -> Result<()>;

    /// 作業ツリーのファイルを index の内容に戻す（変更の破棄）。
    fn restore_worktree(&self, paths: &[&str]) -> Result<()>;
    fn stash_push(&self, message: Option<&str>, include_untracked: bool) -> Result<()>;
    /// stage していた状態も戻す。戻せなければ作業ツリーにだけ当てる。
    fn stash_apply(&self, name: &str, pop: bool) -> Result<Outcome>;
    fn stash_drop(&self, name: &str) -> Result<()>;
    /// 消した stash を戻す。
    fn stash_store(&self, oid: &Oid, message: &str) -> Result<()>;
    fn update_submodules(&self, paths: &[&str]) -> Result<()>;

    /// すべての設定と、それぞれがどこで決まっているか（後に出るものほど優先）。
    fn config_list(&self) -> Result<Vec<ConfigEntry>>;
    /// このリポジトリ（local）の設定を変える。`value` が None なら消す（上の段の値に戻る）。
    fn config_set(&self, key: &str, value: Option<&str>) -> Result<()>;
    fn remotes(&self) -> Result<Vec<RemoteEntry>>;
    fn change_remote(&self, change: &RemoteChange) -> Result<()>;
    /// HEAD と作業ツリーの、ファイルごとの追加・削除の行数（index の分も含む。untracked は含まない）。
    fn working_stats(&self) -> Result<Vec<crate::FileStat>>;
    /// .gitignore の書き方の確かめ: パターンごとに、合うファイル（追跡しているものと、していないもの。
    /// 中身ごと合うフォルダは 1 つにまとめる）の初めの `limit` 個と、合う数。
    fn ignore_matches(
        &self,
        patterns: &[String],
        limit: usize,
    ) -> Result<Vec<(Vec<String>, usize)>>;
    /// git を動かしている側（SSH 先なら daemon）の Activity の記録。手元で動かしているなら空（手元の記録は
    /// `activity::records` で読める）。
    fn activity(&self) -> Result<Vec<crate::activity::Entry>> {
        Ok(Vec::new())
    }
    /// git を動かしている側で、走っている操作を取り消す。
    fn cancel_activity(&self, seq: u64) -> Result<bool> {
        Ok(crate::activity::cancel(seq))
    }
}
