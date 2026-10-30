use crate::{Oid, Result};

/// ref の書き換え 1 つ（`Operations::update_refs`）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RefUpdate {
    /// 完全な ref 名。
    pub name: String,
    /// 新しい値。`None` なら消す。
    pub value: Option<Oid>,
    /// 今の値がこれのときだけ書き換える（`None` は「まだない」）。
    pub expected: Option<Oid>,
}

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
    /// git の設定（`pull.rebase`・`pull.ff`）に任せる。端末の `git pull` と同じ。
    #[default]
    Git,
    /// merge する（fast-forward で済めばそうする）。
    Merge,
    Rebase,
    /// fast-forward で済むときだけ取り込む。
    FastForwardOnly,
}

/// shallow clone の履歴をどこまで取り足すか（`Operations::deepen`）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Deepen {
    /// 今の境界からさらにこれだけ古いコミットまで（`git fetch --deepen=<n>`）。
    Commits(u32),
    /// この日時より新しいコミットをすべて（`git fetch --shallow-since=<date>`。日付は git が読む形）。
    Since(String),
    /// すべて（`git fetch --unshallow`）。
    All,
}

/// リポジトリの形のうち、git の設定とファイルから読むもの（`Operations::repository_form`）。shallow clone かは
/// `GitBackend::shallow` で分かるので入れない。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RepositoryForm {
    /// partial clone なら、その様子。
    pub partial: Option<PartialClone>,
    /// sparse checkout（`core.sparseCheckout`）なら、その様子。
    pub sparse: Option<SparseCheckout>,
    /// `git maintenance register` で登録してある（ユーザー全体の `maintenance.repo` にこのリポジトリがある）。
    pub maintenance: bool,
}

/// partial clone の様子。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PartialClone {
    /// 足りないオブジェクトを取ってくる remote（`extensions.partialClone`、なければ `remote.<name>.promisor` か
    /// `partialclonefilter` のある remote）。
    pub remote: String,
    /// 取らずにおいたもの（`blob:none` など）。`remote.<name>.partialclonefilter`（git 2.23 は
    /// `core.partialclonefilter`）。書いていなければ `None`。
    pub filter: Option<String>,
}

/// sparse checkout の様子。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SparseCheckout {
    /// cone mode（`core.sparseCheckoutCone`。フォルダで選ぶ）。
    pub cone: bool,
    /// cone mode なら取り出しているフォルダ（根からの相対パス。最上位のファイルはいつも取り出す）、そうでなければ
    /// `info/sparse-checkout` のパターン。
    pub folders: Vec<String>,
}

impl SparseCheckout {
    /// `path`（ファイルの、根からの相対パス）を取り出しているか。cone mode は、取り出すフォルダの下すべてと、
    /// 根とその途中のフォルダの直下のファイル。cone mode でなければパターンを読まないので、取り出しているとみなす。
    pub fn contains(&self, path: &str) -> bool {
        if !self.cone {
            return true;
        }
        let Some((parent, _)) = path.rsplit_once('/') else {
            return true;
        };
        self.folders.iter().any(|f| {
            let f = f.trim_matches('/');
            parent == f
                || parent.strip_prefix(f).is_some_and(|r| r.starts_with('/'))
                || f.strip_prefix(parent).is_some_and(|r| r.starts_with('/'))
        })
    }
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

/// git を動かす機械で使う git と、それをどこで決めたか（Settings > Git > Programs の git の行）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct MachineGit {
    /// この機械の Amon Hen の設定で選んだ git（`settings.json` の `"git"`）。なければ自動。
    pub chosen: Option<String>,
    /// 設定のファイル。
    pub file: Option<String>,
    /// 今使う git（パス、または PATH で探す `git`）。
    pub program: String,
    /// どこで決めたか: `environment`（`AMONHEN_GIT`）・`settings`・`fallback`（`AMONHEN_FALLBACK_GIT`）・`path`。
    pub source: String,
    /// 使えないときの理由（git を動かす操作はこれでエラーになる）。
    pub problem: Option<String>,
}

/// 署名に使える鍵（リポジトリの設定の「鍵」の一覧の 1 つ）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SigningKey {
    /// `user.signingkey` に書く値（OpenPGP・X.509 は指紋、SSH は公開鍵のパスか `key::…`）。
    pub id: String,
    /// 人に見せる名前（ユーザー ID、公開鍵のコメントとファイル名）。
    pub label: String,
    /// 鍵のメールアドレス（`user.email` のものを先に出すため）。
    pub emails: Vec<String>,
}

/// この機械にあるプログラム（git・gpg・gpgsm・ssh-keygen）の 1 つ（設定の「プログラム」の候補）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Program {
    /// 実行ファイルのパス。
    pub path: String,
    /// 版（`2.45.1`・`2.4.5`・`9.9p2`）。読めなければ `None`。
    pub version: Option<String>,
}

/// git を動かす機械のフォルダの中身（`Operations::list_directory`。SSH 先のパスを選ぶシートが使う）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Directory {
    /// 開いたフォルダの絶対パス（`~` を展開したもの）。
    pub path: String,
    /// その機械のホーム。決められなければ `None`。
    pub home: Option<String>,
    /// 中身（名前の順。`.` で始まるものも含む）。
    pub entries: Vec<DirectoryEntry>,
}

/// フォルダの中の 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DirectoryEntry {
    pub name: String,
    /// `folder`・`file`・`other`（シンボリックリンクなら、その先の種類。先がなければ `other`）。
    pub kind: String,
    /// シンボリックリンクか。
    pub link: bool,
    /// 実行できるファイルか（Unix は実行の権限、Windows は拡張子）。
    pub executable: bool,
    /// git のリポジトリ（`.git` のあるフォルダ）か。
    pub repository: bool,
}

/// worktree の作成・削除・片付け（`git worktree add`・`remove`・`prune`）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WorktreeChange {
    /// `path` に worktree を作る。`new_branch` があればそのブランチを `start`（なければ HEAD）から作って
    /// checkout、なければ `start`（ブランチかコミット）を checkout する（ブランチでなければ detached）。
    Add {
        path: String,
        start: Option<String>,
        new_branch: Option<String>,
    },
    /// `path` の worktree を消す。`force` なら変更が残っていても・ロックしていても消す（`--force` を 2 回）。
    Remove { path: String, force: bool },
    /// フォルダがなくなった worktree の記録を片付ける。
    Prune,
}

/// ほかの worktree の変更の数（`Operations::worktree_changes`）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct WorktreeChanges {
    pub path: String,
    /// 変更のあるファイルの数（untracked も 1 つずつ数える。Working Copy と同じ）。読めなければ None。
    pub count: Option<u32>,
    /// status の中身の指紋（変わったかどうかを見分ける）。
    pub stamp: u64,
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
    /// push の URL を fetch と別にする（`url` が None なら同じに戻す）。
    SetPushUrl {
        name: String,
        url: Option<String>,
    },
}

/// リモートの URL につないで分かったこと（`git ls-remote`）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RemoteCheck {
    pub branches: usize,
    pub tags: usize,
    /// 向こうの HEAD が指すブランチ（分かれば）。
    pub head: Option<String>,
}

/// 途中で止まりうる操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Sequence {
    Rebase,
    Merge,
    CherryPick,
    Revert,
    /// `git am`（メールのパッチを当てている途中）。
    Am,
}

impl Sequence {
    pub fn command(self) -> &'static str {
        match self {
            Sequence::Rebase => "rebase",
            Sequence::Merge => "merge",
            Sequence::CherryPick => "cherry-pick",
            Sequence::Revert => "revert",
            Sequence::Am => "am",
        }
    }
}

/// `git format-patch` の選び方。ほかのこと（番号の付け方・宛先・署名など）は git の `format.*` の設定に任せる。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FormatPatch {
    /// 送り状（`0000-cover-letter.patch`）も作る（`--cover-letter`）。
    pub cover_letter: bool,
    /// 出し直しの版（`-v <n>`。`[PATCH v2 1/3]`、`v2-0001-….patch`）。
    pub reroll: Option<u32>,
    /// 1 つの mbox にまとめる（`--stdout`）。`PatchFile::name` は空になる。
    pub single: bool,
}

/// 書き出したパッチ 1 つ（git が付けたファイル名と中身）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PatchFile {
    pub name: String,
    pub content: Vec<u8>,
}

/// `git apply` の当て方。既定は git と同じく作業ツリーだけ。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ApplyPatch {
    /// index にも当てる（`--index`）。
    pub index: bool,
    /// 当たらなければ 3-way merge にしてコンフリクトを残す（`--3way`。index にも当たる）。
    pub three_way: bool,
    /// 当てずに、当たるかだけ調べる（`--check`）。
    pub check: bool,
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

/// 今のブランチ（または detached HEAD）を動かす reset の種類（`git reset --<mode>`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResetMode {
    /// index と作業ツリーはそのまま（コミットの Undo）。
    Soft,
    /// index も動かし、作業ツリーはそのまま。
    Mixed,
    /// index と作業ツリーも合わせる（作業中の変更は消える）。
    Hard,
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
        Err(crate::tr!("Can’t search by path or content here").into())
    }

    /// `git blame --porcelain` の出力。`rev` がなければ作業ツリーの内容（まだコミットしていない行を含む）。
    fn blame(&self, path: &str, rev: Option<&Oid>) -> Result<Vec<u8>> {
        let _ = (path, rev);
        Err(crate::tr!("Can’t blame here").into())
    }

    /// HEAD から `path` を変えたコミット（名前の変更を追う。`merges` でなければマージは出さない）。
    /// `git log -z --name-only` の、各コミットが `\x01ID\0件名\0作者\0時刻\0\nその時点のパス\0` になる出力。
    /// `limit` があれば新しい方からその数まで。
    fn file_log(&self, path: &str, limit: Option<usize>, merges: bool) -> Result<Vec<u8>> {
        let _ = (path, limit, merges);
        Err(crate::tr!("Can’t read file history here").into())
    }

    fn head_state(&self) -> Result<HeadState>;
    /// 完全な ref 名の値。なければ `None`。
    fn resolve_refs(&self, names: &[String]) -> Result<Vec<(String, Option<Oid>)>>;
    /// `expected` が今の値のときだけ書き換える（`None` は「まだない」）。`value` が `None` なら消す。
    fn update_ref(&self, name: &str, value: Option<&Oid>, expected: Option<&Oid>) -> Result<()>;
    /// 複数の ref を、すべて書き換えるか何も変えないかのどちらかで書き換える。対応しない実装は 1 つずつ。
    fn update_refs(&self, updates: &[RefUpdate]) -> Result<()> {
        for u in updates {
            self.update_ref(&u.name, u.value.as_ref(), u.expected.as_ref())?;
        }
        Ok(())
    }
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
    fn delete_tag(&self, name: &str) -> Result<()>;
    /// `remote` にあるタグと、それぞれの ref が指すオブジェクト（注釈付きのタグはタグのオブジェクト）。
    /// `git ls-remote --tags`。
    fn remote_tags(&self, remote: &str) -> Result<Vec<(String, Oid)>>;
    /// タグ `names` を `remote` に push する（`force` なら同じ名前のリモートのタグを置き換え、`delete` なら
    /// `remote` から消す）。
    fn push_tags(&self, remote: &str, names: &[&str], force: bool, delete: bool) -> Result<()>;
    /// ブランチ名として使えなければ理由。
    fn check_branch_name(&self, name: &str) -> Result<Option<String>>;

    /// `ancestor` が `descendant` の祖先か（fast-forward できるか）。
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool>;
    /// `theirs` を `ours` にマージしたらコンフリクトするファイル（なければ空）を、`theirs` の順に 1 つずつ。
    /// いくつもあれば octopus と同じく、前のものまでを合わせた結果に重ねて調べる。作業ツリーと index には
    /// 触らない。git が古くて調べられなければ `None`。
    fn predict_merge(&self, ours: &str, theirs: &[String]) -> Result<Option<Vec<Vec<String>>>>;
    /// `picks` を順に `onto` へ載せ直したら（rebase・History の編集）、それぞれでコンフリクトするファイル（なければ
    /// 空）。各コミットは、その親との差分を、ここまで載せた結果に当てて調べる。作業ツリーと index には触らない。
    /// git が古くて（`merge-tree --merge-base` は 2.40 から）調べられなければ `None`。
    fn predict_replay(&self, onto: &str, picks: &[String]) -> Result<Option<Vec<Vec<String>>>>;
    /// 同じ一連のコミットの 2 つの版を比べる（`git range-diff`）。`ranges` は git と同じ形（`A...B`、
    /// `<base> <A> <B>`、`<base>..<A> <base>..<B>`）。`creation_factor` は git の `--creation-factor`
    /// （`None` なら git の既定）。コミット ID は完全な形で返す。
    fn range_diff(
        &self,
        ranges: &[String],
        creation_factor: Option<u32>,
    ) -> Result<Vec<crate::RangePair>>;
    /// 名前（`main`・`origin/main`・コミット ID など）が指すコミット。
    fn resolve(&self, rev: &str) -> Result<Oid>;
    /// `revs` を今のブランチに merge する（2 つ以上なら 1 つのコミットにまとめる octopus）。octopus が途中で
    /// 断ったら、作業ツリーと index を始める前に戻してからエラーを返す。
    fn merge(&self, revs: &[String]) -> Result<Outcome>;
    /// 今のブランチを `onto` へ載せ直す。`update_refs` は、積み重ねたブランチを一緒に動かすかのこの操作だけの指定
    /// （`--update-refs`・`--no-update-refs`、git 2.38 から）。`None` なら git の `rebase.updateRefs` のとおり。
    fn rebase(&self, onto: &str, update_refs: Option<bool>) -> Result<Outcome>;
    fn cherry_pick(&self, oid: &Oid, is_merge: bool) -> Result<Outcome>;
    fn revert(&self, oid: &Oid, is_merge: bool) -> Result<Outcome>;
    /// 用意した todo で非対話の rebase を行う（コミットの編集）。`base` が `None` なら `--root`。`message` は、
    /// todo の `reword`・`squash` で git が開くエディタに書かせるメッセージのファイル。
    /// 対話的な rebase を、用意した todo で行う。`rebase_merges` なら `--rebase-merges` で merge を作り直し、
    /// `todo` は todo そのものではなく、git が作った todo を書き換える awk のプログラム。
    fn rebase_todo(
        &self,
        base: Option<&Oid>,
        todo: &std::path::Path,
        message: Option<&std::path::Path>,
        rebase_merges: bool,
        update_refs: Option<bool>,
    ) -> Result<Outcome>;
    fn sequence_in_progress(&self) -> Result<Option<Sequence>>;
    /// rebase・am の途中なら、何番目か（今, 全体）。
    fn rebase_progress(&self) -> Result<Option<(u32, u32)>>;
    fn abort(&self, sequence: Sequence) -> Result<()>;
    fn continue_sequence(&self, sequence: Sequence) -> Result<Outcome>;
    fn skip_sequence(&self, sequence: Sequence) -> Result<Outcome>;

    /// `commits` を、渡した順にパッチにする（`git format-patch`。マージは git と同じく飛ばす）。
    fn format_patch(&self, commits: &[Oid], options: &FormatPatch) -> Result<Vec<PatchFile>>;
    /// mbox のパッチを今のブランチにコミットとして当てる（`git am`）。`three_way` が `None` なら git の
    /// `am.threeWay` に任せる。当たらないパッチがあれば、そこで止まる（`Outcome::Conflict`）。
    fn am(&self, mbox: &[u8], three_way: Option<bool>) -> Result<Outcome>;
    /// `git am` が当てられずに止まったパッチを 3-way merge で当て直す。きれいに当たれば続ける。
    fn am_three_way(&self) -> Result<Outcome>;
    /// diff を当てる（`git apply`）。3-way でコンフリクトが残れば `Outcome::Conflict`。
    fn apply_patch(&self, patch: &[u8], options: &ApplyPatch) -> Result<Outcome>;

    /// コンフリクトしたファイルを片側（`ours` なら Current、そうでなければ Incoming）にして解決済みにする。
    /// その側でファイルが消えていれば、消して解決済みにする。
    fn take_side(&self, path: &str, ours: bool) -> Result<()>;
    /// コンフリクトしたファイルの index の blob（共通の祖先・Current・Incoming の順。その側になければ `None`）。
    fn conflict_stages(&self, path: &str) -> Result<[Option<Oid>; 3]>;

    /// `remote` から取ってくる（`None` ならすべてのリモート、`git fetch --all`）。消えたブランチを片づけるかは
    /// git の `fetch.prune` のとおり。
    fn fetch(&self, remote: Option<&str>) -> Result<()>;
    /// shallow clone の履歴を取り足す（既定のリモートから）。
    fn deepen(&self, how: &Deepen) -> Result<()>;
    /// partial clone で取らずにおいた中身をすべて取る: `remote` の filter の設定を消してから
    /// `git fetch --refetch <remote>`（git 2.36 から）。取れなければ filter の設定を戻す。
    fn fetch_all_contents(&self, remote: &str) -> Result<()> {
        let _ = remote;
        Err(crate::tr!("Can’t fetch contents here").into())
    }
    /// 今のブランチに上流を取り込む（`git pull`）。
    fn pull(&self, mode: PullMode) -> Result<Outcome>;
    /// ローカルブランチごとの、上流との差（名前, ahead, behind, gone）。
    fn branch_tracking(&self) -> Result<Vec<(String, u32, u32, bool)>>;
    /// `force` なら `--force-with-lease`。`expect` があれば、リモートのブランチがそのコミットのときだけ置き換える
    /// （確認で見せた上流。自動の Fetch で追跡ブランチが進んでいても、見ていないコミットは消さない）。
    fn push(
        &self,
        remote: &str,
        branch: &str,
        remote_branch: &str,
        force: bool,
        expect: Option<&Oid>,
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
    /// `git stash branch`: stash を作ったときのコミットから `branch` を作って切り替え、そこに stash を戻す
    /// （戻せたら stash を消す）。
    fn stash_branch(&self, branch: &str, name: &str) -> Result<Outcome>;
    /// `left` にだけあるコミットと、`right` にだけあるコミットの数（`git rev-list --left-right --count`）。
    fn count_apart(&self, left: &str, right: &str) -> Result<(u32, u32)>;
    fn update_submodules(&self, paths: &[&str]) -> Result<()>;

    /// すべての設定と、それぞれがどこで決まっているか（後に出るものほど優先）。
    fn config_list(&self) -> Result<Vec<ConfigEntry>>;
    /// git が知っている設定の変数の名前（`core.fileMode` のような綴りで。設定の有無は問わない）。
    /// `remote.<name>.url` のように名前が入るものは含まない。読めなければ空。
    fn config_keys(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
    /// このリポジトリ（local）の設定を変える。`value` が None なら消す（上の段の値に戻る）。
    fn config_set(&self, key: &str, value: Option<&str>) -> Result<()>;
    /// git を動かしているユーザー全体（global）の設定を変える。SSH 先のリポジトリなら SSH 先のユーザーのもの。
    fn config_set_global(&self, key: &str, value: Option<&str>) -> Result<()> {
        let _ = (key, value);
        Err(crate::tr!("Can’t change the user’s git settings here").into())
    }
    /// git を動かす機械で使う git（その機械の Amon Hen の設定と環境から決まる。`crate::settings`）。
    fn machine_git(&self) -> Result<MachineGit> {
        Err(crate::tr!("Can’t read Amon Hen’s settings of the machine running git here").into())
    }
    /// git を動かす機械の Amon Hen の設定に、使う git を書く。None なら消す（自動）。
    fn set_machine_git(&self, git: Option<&str>) -> Result<()> {
        let _ = git;
        Err(crate::tr!("Can’t change Amon Hen’s settings of the machine running git here").into())
    }
    /// このリポジトリを扱う git の版（major, minor, patch）。読めなければ `None`。
    fn git_version(&self) -> Result<Option<(u32, u32, u32)>> {
        Ok(None)
    }
    fn remotes(&self) -> Result<Vec<RemoteEntry>>;
    /// `format`（`openpgp`・`ssh`・`x509`）で署名に使える鍵。集められなければ空。
    fn signing_keys(&self, format: &str) -> Result<Vec<SigningKey>> {
        let _ = format;
        Ok(Vec::new())
    }
    /// git を動かしている機械にある `name`（`git`・`gpg`・`gpgsm`・`ssh-keygen`）の候補。先頭は PATH で最初に
    /// 見つかるもの（何も設定しなければ使われるもの）。集められなければ空。
    fn programs(&self, name: &str) -> Result<Vec<Program>> {
        let _ = name;
        Ok(Vec::new())
    }
    /// git を動かしている機械の `path` のフォルダの中身。`~` で始まればその機械のホームから、相対パスもホームから。
    fn list_directory(&self, path: &str) -> Result<Directory> {
        let _ = path;
        Err(crate::tr!("Can’t look at folders of the machine running git here").into())
    }
    /// git を動かしているユーザー全体の、無視するパターンのファイル（`core.excludesFile`。なければ git の既定
    /// `$XDG_CONFIG_HOME/git/ignore`・`~/.config/git/ignore`）: そのパスと中身（ファイルがなければ `None`）。
    fn excludes_file(&self) -> Result<(String, Option<String>)> {
        Err(crate::tr!("Can’t read the excludes file here").into())
    }
    /// git を動かしているプロセスの環境変数（名前の順）。SSH 先ならエンジンのもので、そのまま git に渡る。
    fn process_environment(&self) -> Result<Vec<(String, String)>> {
        Ok(Vec::new())
    }
    /// そのファイルを書く（途中のフォルダがなければ作る）。
    fn save_excludes_file(&self, text: &str) -> Result<()> {
        let _ = text;
        Err(crate::tr!("Can’t write the excludes file here").into())
    }
    fn change_remote(&self, change: &RemoteChange) -> Result<()>;
    /// `url` につないで、ブランチとタグの数と HEAD のブランチを読む（`git ls-remote`。保存する前の確かめ）。
    fn check_remote(&self, url: &str) -> Result<RemoteCheck> {
        let _ = url;
        Err(crate::tr!("Can’t reach remotes from here").into())
    }
    /// `git lfs track <pattern>`（`track` が false なら `untrack`）。`.gitattributes` を書き換える。
    fn lfs_track(&self, pattern: &str, track: bool) -> Result<()> {
        let _ = (pattern, track);
        Err(crate::tr!("Can’t change LFS tracking here").into())
    }
    /// LFS にあるファイルの数（`git lfs ls-files`）。git-lfs がなければ `None`。
    fn lfs_file_count(&self) -> Result<Option<usize>> {
        Ok(None)
    }
    /// partial clone・sparse checkout・maintenance の登録（`RepositoryForm`）。
    fn repository_form(&self) -> Result<RepositoryForm> {
        Ok(RepositoryForm::default())
    }
    /// worktree の一覧。`git worktree list --porcelain -z` の出力。
    fn worktree_list(&self) -> Result<Vec<u8>> {
        Err(crate::tr!("Can’t list worktrees here").into())
    }
    fn change_worktree(&self, change: &WorktreeChange) -> Result<()> {
        let _ = change;
        Err(crate::tr!("Can’t change worktrees here").into())
    }
    /// `paths` の worktree の変更の数。worktree は `.git` を共有するので、開いたリポジトリから読む（worktree ごとに
    /// 開かない。SSH 先でも 1 本のつながりで済む）。前に数えてから変わっていない worktree は数え直さない（変化を
    /// 見張れる側だけ）。`fresh` なら全部を数え直す。
    fn worktree_changes(&self, paths: &[String], fresh: bool) -> Result<Vec<WorktreeChanges>> {
        let _ = (paths, fresh);
        Err(crate::tr!("Can’t list worktrees here").into())
    }
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
