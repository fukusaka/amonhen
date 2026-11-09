//! FE から受け取る操作。頻度の低い操作はまとめてここを通し、Undo の記録もここで行う。

use amonhen_model::tr;
use amonhen_model::{Oid, Outcome, PullMode, RefEntry, RefKind, Result, Root};
use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::rewrite::{self, ChainCommit, HistoryEdit};

/// 編集できる範囲を調べるときに辿るコミットの数。
const CHAIN_LIMIT: usize = 500;

/// `Command::Reset` の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResetKind {
    Soft,
    Mixed,
    Hard,
}

/// 名前 1 つか、いくつもの名前（JSON の文字列か配列）。
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Revs {
    One(String),
    Many(Vec<String>),
}

impl Revs {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Revs::One(rev) => vec![rev],
            Revs::Many(revs) => revs,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    Checkout {
        branch: String,
    },
    /// リモートブランチ（`origin/main`）を追跡するローカルブランチを作って切り替える。
    Track {
        remote_branch: String,
    },
    /// コミットを detached HEAD でチェックアウトする。
    Detach {
        oid: String,
    },
    /// 今のブランチ（detached HEAD なら HEAD）を `oid` に動かす（`git reset --soft|--mixed|--hard`）。Undo できる。
    /// hard で作業中の変更（untracked のファイルは除く）があれば、消す前に stash に残す（`info.stashed`）。
    Reset {
        oid: String,
        mode: ResetKind,
    },
    CreateBranch {
        name: String,
        start: String,
        checkout: bool,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    /// 履歴のない新しいブランチ `name` に切り替える。`keepFiles` なら今のファイルを stage したまま残す
    /// （`git checkout --orphan`）、でなければ空から（`git switch --orphan`。作業中の変更があれば git が断る）。
    CreateOrphanBranch {
        name: String,
        #[serde(default)]
        keep_files: bool,
    },
    /// rerere が当てた `path` の解決を忘れ、コンフリクトのある形に戻す（作業ツリーのファイルは Undo で戻る）。
    ForgetResolution {
        path: String,
    },
    /// rerere が覚えたすべての解決を消す（`.git/rr-cache`。Undo では戻せない）。`info.count`: 消した数。
    ForgetAllResolutions,
    /// ignore されたファイルとフォルダ（`info.paths`。`git clean -n -d -X`）。
    IgnoredFiles,
    /// ignore されたファイル `paths` を消す（`git clean -f -d -X`。Undo では戻せない）。
    DeleteIgnored {
        paths: Vec<String>,
    },
    RenameBranch {
        from: String,
        to: String,
    },
    /// 片付けてよいローカルブランチ（`info.branches`。シート「Clean Up Branches…」）。`reason` は `merged`（既定の
    /// ブランチに取り込み済み）・`pullRequest`（`pullRequests` の PR が、今の先端を head にしてマージされた。squash・
    /// rebase で取り込んだもの）・`gone`（上流が消えた）。`force` なら `git branch -D` が要る。`checked` は最初に選んで
    /// おくか（`gone` だけのものは選ばない）。今のブランチ・お気に入り・ほかの worktree で使っているもの・既定の
    /// ブランチと、既定のブランチの先端で作ってまだコミットしていないものは入れない。`info.base`: 既定のブランチ。
    BranchCleanup {
        #[serde(default)]
        pull_requests: Vec<MergedPullRequest>,
    },
    /// ローカルブランチ `branches` をまとめて消す（Undo は 1 回でまとめて戻す）。途中で失敗しても、消せた分は記録する。
    DeleteBranches {
        branches: Vec<BranchToDelete>,
    },
    CreateTag {
        name: String,
        target: String,
        message: Option<String>,
    },
    /// 手元のタグを消す（Undo できる。注釈付きのタグも、タグのオブジェクトごと戻る）。
    DeleteTag {
        name: String,
    },
    /// タグ `names` を `remote` に push する。`force` なら、同じ名前で違うコミットを指すリモートのタグを置き換える。
    /// `delete` なら `remote` から消す（リモートのことなので Undo できない）。
    PushTags {
        remote: String,
        names: Vec<String>,
        #[serde(default)]
        force: bool,
        #[serde(default)]
        delete: bool,
    },
    /// 手元のタグごとの、リモートでの様子（fetch のときに読んで覚えたもの。`crate::remote_tags::status`）。
    TagRemotes,
    /// 今のブランチに `rev` を merge する。`rev` が配列なら、すべてを 1 つのコミットにまとめる（octopus、
    /// `git merge a b c`）。`oneByOne` なら 1 つずつ merge する（merge コミットがいくつもできる。コンフリクトで
    /// 止まったら、まだのものを `info.remaining` に返す）。どちらも Undo 1 回で戻る。
    Merge {
        rev: Revs,
        #[serde(default)]
        one_by_one: bool,
    },
    /// ブランチ `target` に `rev` を merge する。`target` を checkout していなくても、fast-forward で
    /// 済むならブランチだけ進める（`rev` が 1 つのとき）。そうでなければ checkout してから merge する。
    MergeInto {
        target: String,
        rev: Revs,
        #[serde(default)]
        one_by_one: bool,
    },
    /// 今のブランチを `onto` の上に rebase する。
    Rebase {
        onto: String,
        /// 積み重ねたブランチを一緒に動かすかの、この操作だけの指定（なければ git の `rebase.updateRefs`）。
        #[serde(default)]
        update_refs: Option<bool>,
    },
    /// `oid` か、いくつもなら `oids`（渡した順に、1 回の `git cherry-pick A B C` で）。コンフリクトで止まったら、
    /// 残りは git が覚えていて Continue・Skip で続き、Abort なら全体を始める前に戻す（git と同じ）。`is_merge` は
    /// どれかが merge（`-m 1`）。
    CherryPick {
        #[serde(default)]
        oid: String,
        #[serde(default)]
        oids: Vec<String>,
        #[serde(default)]
        is_merge: bool,
    },
    /// `CherryPick` と同じ形。
    Revert {
        #[serde(default)]
        oid: String,
        #[serde(default)]
        oids: Vec<String>,
        #[serde(default)]
        is_merge: bool,
    },
    /// コミット `oids` を、渡した順（古い順に渡す）にパッチにする（`git format-patch`。マージは git と同じく
    /// 飛ばす）。`dir` があればそのフォルダに 1 コミット 1 ファイル（git が付けた名前。同じ名前のファイルは
    /// 上書き）、`file` があればそのファイルに 1 つの mbox。`info.files` に書いたパス。番号の付け方・宛先・署名
    /// などは git の `format.*` の設定に任せる。
    FormatPatch {
        oids: Vec<String>,
        dir: Option<String>,
        file: Option<String>,
        #[serde(default)]
        cover_letter: bool,
        reroll: Option<u32>,
    },
    /// `rev`（コミット・タグ・ブランチ）の時点のファイルを `format`（`zip`・`tar`・`tar.gz`）にまとめて `file` に書く
    /// （`git archive`。中身の先頭に `prefix` のフォルダを置き、`paths` があればその下だけ。外すファイルなどは git の
    /// `.gitattributes` に任せる）。`info.file`・`info.size`。SSH 先や daemon 越しでも、ファイルはこの機械に書く。
    /// partial clone で手元にない中身は先にまとめて取る（数は `archiveContents` で前もって数える）。
    Archive {
        rev: String,
        format: String,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        paths: Vec<String>,
        file: String,
    },
    /// `Archive` の前に取ってくる、partial clone で手元にない中身の数（`info.count`。数えるのをやめたら
    /// `info.more`）。partial clone でなければ 0。
    ArchiveContents {
        rev: String,
        #[serde(default)]
        paths: Vec<String>,
    },
    /// `refs`（`refs/heads/main` など。空なら HEAD とすべてのブランチ・タグ）のコミットを 1 つのファイルにまとめて
    /// `file` に書く（`git bundle create`）。`bases` があれば、受け取る側が持っているはずのそのコミットまでは入れない
    /// （`^<base>`）。`info.file`・`info.size`。SSH 先や daemon 越しでも、ファイルはこの機械に書く。shallow clone で
    /// 取っていない履歴まで要るなら断る。partial clone で手元にない中身は先にまとめて取る。
    Bundle {
        #[serde(default)]
        refs: Vec<String>,
        #[serde(default)]
        bases: Vec<String>,
        file: String,
    },
    /// `Bundle` の前に見せること（`info.commits`: 入るコミットの数、`info.contents`・`info.more`: partial clone で
    /// 先に取る中身の数、`info.beyondShallow`: shallow clone で作れない）。
    BundlePreview {
        #[serde(default)]
        refs: Vec<String>,
        #[serde(default)]
        bases: Vec<String>,
    },
    /// bundle のファイル `file`（この機械のもの）の見出しと、取り込めるか（`info.refs`: 入っているブランチ・タグ、
    /// `info.head`、`info.prerequisites`: 前提のコミットの数、`info.missing`: そのうちこのリポジトリにないものの数、
    /// `info.name`: 取り込む名前の候補（ファイルの名前から））。bundle でなければ断る。
    BundleCheck {
        file: String,
    },
    /// bundle のファイル `file`（この機械のもの）のブランチを `refs/remotes/<name>/*` に取り込む（`git fetch`。
    /// タグは取らない）。SSH 先や daemon 越しでも、中身を送って取り込む。`info.refs`: 取り込んだ ref。
    FetchBundle {
        file: String,
        name: String,
    },
    /// パッチのファイル `paths`（並べた順につなぐ）の中身（`info`: `kind`（`mailbox` なら Am、`diff` なら
    /// ApplyPatch で当てる）・`patches`（subject・author・authorEmail・date・files（path・oldPath・added・removed・
    /// binary・status））・`applies`（今のリポジトリにそのまま当たるか。当たらなければ `error` に git の答え）・
    /// `threeWay`（git の `am.threeWay`））。
    ReadPatch {
        paths: Vec<String>,
    },
    /// メールの形のパッチ `paths` を、並べた順に今のブランチにコミットとして当てる（`git am`。Undo できる）。
    /// `threeWay` がなければ git の `am.threeWay` に任せる。当たらなければ止まる（Continue・Skip・Abort）。
    Am {
        paths: Vec<String>,
        three_way: Option<bool>,
    },
    /// `git am` が当てられずに止まったパッチを 3-way merge で当て直す。きれいに当たれば続け、当たらなければ
    /// コンフリクトをマーカーつきで残す。
    AmThreeWay,
    /// diff `paths` を作業ツリーに当てる（`git apply`。Undo でファイルの中身は戻る。index は戻らない）。
    /// `index` なら index にも、`threeWay` なら当たらないところを 3-way merge にしてコンフリクトを残す。
    ApplyPatch {
        paths: Vec<String>,
        #[serde(default)]
        index: bool,
        #[serde(default)]
        three_way: bool,
    },
    Abort,
    Continue,
    Skip,
    /// `remote` から取ってくる（なければすべてのリモート）。
    Fetch {
        #[serde(default)]
        remote: Option<String>,
    },
    /// shallow clone の履歴を取り足す。`commits` ならさらにその数だけ古いコミットまで（`git fetch --deepen`）、
    /// `since` ならその日付（`YYYY-MM-DD` など git が読む形）より新しいコミットをすべて（`--shallow-since`）。
    /// 履歴を足すだけなので Undo には記録しない。shallow clone でなければ断る。
    Deepen {
        commits: Option<u32>,
        since: Option<String>,
    },
    /// shallow clone の履歴をすべて取る（`git fetch --unshallow`）。
    Unshallow,
    /// partial clone で取らずにおいた中身をすべて取る（promisor の remote の filter の設定を消して
    /// `git fetch --refetch <remote>`。git 2.36 から）。Fetch と同じく動かし、Undo には記録しない。取れなければ
    /// filter の設定を戻す。partial clone でなければ断る。
    FetchAllContents,
    /// 今のブランチに上流を取り込む（Undo できる）。
    Pull {
        #[serde(default)]
        mode: PullMode,
    },
    /// チェックアウトしていないブランチを上流まで fast-forward する（`git fetch . <上流>:<branch>` と同じ。Undo
    /// できる）。上流は最後の Fetch の時点のもの。分かれている・どこかの worktree でチェックアウト中なら断る。
    FastForward {
        branch: String,
    },
    Push {
        remote: String,
        branch: String,
        remote_branch: String,
        force: bool,
        /// `force` のとき、リモートのブランチがこのコミット（16 進）のときだけ置き換える。なければ追跡ブランチと比べる。
        #[serde(default)]
        expect: Option<String>,
        set_upstream: bool,
    },
    /// 作業ツリーのファイルを index の内容に戻す（Undo できる）。
    Discard {
        paths: Vec<String>,
    },
    /// 作業ツリーのファイル `paths` を `rev` の版にする（`git restore --source`。`rev` にないファイルは消す）。
    /// `staged` なら index にも入れる。作業ツリーのファイルは Undo で戻る（index は戻さない）。
    RestoreFiles {
        rev: String,
        paths: Vec<String>,
        #[serde(default)]
        staged: bool,
    },
    /// 作業中の変更を stash する。`keepIndex` なら stage した変更を残し、`staged` なら stage した変更だけ（git 2.35
    /// から）、`paths` があればそのファイルの変更だけ。
    StashPush {
        message: Option<String>,
        include_untracked: bool,
        #[serde(default)]
        keep_index: bool,
        #[serde(default)]
        staged: bool,
        #[serde(default)]
        paths: Vec<String>,
    },
    StashApply {
        name: String,
        pop: bool,
    },
    /// stash を消す（Undo できる）。
    StashDrop {
        name: String,
    },
    /// stash を作ったときのコミットから `branch` を作って切り替え、そこに stash を戻す（`git stash branch`。戻せたら
    /// stash は消える）。
    StashBranch {
        name: String,
        branch: String,
    },
    /// stash ごとの土台（作ったときの HEAD）と、今の HEAD の離れ具合（`info.stashes`: name・base・`ahead`（HEAD に
    /// だけあるコミットの数）・`behind`（土台にだけあるコミットの数））。HEAD がまだなければ空。
    StashBases,
    /// ref の reflog（`info.entries`: 新しい順に `selector`（`HEAD@{0}`）・`old`（ref を作った行なら null）・`new`・
    /// `time`・`message`・`subject`（`new` の件名。読めなければ空）・`reachable`（ref から辿れる、つまり History に
    /// ある））。`name` の既定は `HEAD`（書き込む先の worktree のもの）、`limit` の既定は 500。
    Reflog {
        name: Option<String>,
        limit: Option<usize>,
    },
    /// 同じ一連のコミットの 2 つの版を比べる（`git range-diff`）。`ranges` は git と同じ形（`["A...B"]`、
    /// `["<base>", "A", "B"]`、`["<base>..A", "<base>..B"]`）。`info.pairs`: git の出力の順に `status`（same・
    /// changed・removed・added）・`old`・`new`（`index`（範囲の中の 1 始まりの番号）・`oid`。その側になければ
    /// null）・`subject`・`diff`（changed のときの差分の差分。行ごとに、先頭の 1 文字が外側の記号（` `・`-`・`+`、
    /// 見出しは `@`）、残りがそれぞれの版のパッチの行）。merge コミットは比べない（git と同じ）。
    RangeDiff {
        ranges: Vec<String>,
        creation_factor: Option<u32>,
    },
    /// ブランチ（既定は今のブランチ）の、最後に書き換える前の版（reflog から。rebase・コミットの編集・amend・
    /// reset など、前の先端が新しい先端の祖先でなくなった動き）。`info.previous`: `branch`・`selector`（前の版の
    /// `<branch>@{n}`）・`message`（その動きの reflog の行）・`time`・`old`（前の版の先端）・`new`（今の先端）・
    /// `onto`（rebase の載せ先。わからなければ null）・`ranges`（`rangeDiff` に渡す範囲。載せ先があれば
    /// `[onto, old, new]`、なければ `["old...new"]`）。見つからなければ `previous` は null。
    PreviousVersion {
        branch: Option<String>,
    },
    /// submodule `paths` を取り出し、記録したコミットにする（`git submodule update --init`）。`recursive` なら中の
    /// submodule も、`remote` なら記録したコミットではなく追うブランチのリモートの最新にする。
    UpdateSubmodules {
        paths: Vec<String>,
        #[serde(default)]
        recursive: bool,
        #[serde(default)]
        remote: bool,
    },
    /// submodule（`info.submodules`: `path`・`name`・`url`・`branch`・`initialized`・`moved`（記録と違うコミット）・
    /// `changed`（中に変更）・`conflict`）。
    Submodules,
    /// submodule を足す・外す・URL や追うブランチを変える・URL を写す（`change.kind`: `add`・`remove`・`setUrl`・
    /// `setBranch`・`sync`）。
    ChangeSubmodule {
        change: amonhen_model::SubmoduleChange,
    },
    /// subtree（`info.available`: この機械の git で `git subtree` が使えるか、`info.subtrees`: `dir`・`squashed`
    /// （最新の記録が squash）・覚えている `url`・`ref`・`pushRef`・`squash`）。
    Subtrees,
    /// subtree の操作（`action.kind`: `add`・`pull`（`dir`・`url`・`ref`・`squash`）、`push`（`dir`・`url`・`ref`）、
    /// `split`（`dir`・`branch`））。Add・Pull・Split は Undo できる。Pull はコンフリクトすれば止まる。
    Subtree {
        action: amonhen_model::SubtreeOp,
    },
    Undo,
    /// 次に Undo される操作の名前（`label`）と、何を戻すか（`info.kind`: `refs`・`files`・`stash`・`rename`）。
    UndoPeek,
    /// 操作ログ（`info.entries`: 新しい順の `label`・`time`（UNIX 時刻のミリ秒）・`kind`（UndoPeek と同じ））。
    UndoLog,
    /// コミットの編集（reword・squash・fixup・drop・edit・split・move）。
    Rewrite {
        label: String,
        edit: HistoryEdit,
        /// 積み重ねたブランチを一緒に動かすかの、この操作だけの指定（なければ git の `rebase.updateRefs`）。
        #[serde(default)]
        update_refs: Option<bool>,
    },
    /// 書き換える前の見通し（docs/design.md の「Rewriting operations and stacked branches」）。`edit` は History の
    /// 編集、`onto` は今のブランチをそこへ rebase すること（どちらか一方）。作業ツリー・index・ref には触らない。
    /// `info`: `moving`（一緒に動くブランチ: name・upstream）、`staying`（書き換えるコミットに先頭があるのに残る
    /// ブランチ: name・upstream・reason（`setting`: rebase.updateRefs がオフ、`git`: git が 2.38 より古い、
    /// `checkedOut`: ほかの worktree で checkout 中）・worktree）、`current`・`currentUpstream`（今のブランチと
    /// 上流。上流があれば、書き換えた後の push には Force Push が要る）、`conflicts`（コンフリクトしそうな
    /// コミットとファイル: oid・files。調べられなければ null）。
    RewritePreview {
        #[serde(default)]
        edit: Option<HistoryEdit>,
        #[serde(default)]
        onto: Option<String>,
        /// この操作だけの指定（`Rebase`・`Rewrite` の `update_refs` と同じ）。false なら残るブランチの理由は `off`。
        #[serde(default)]
        update_refs: Option<bool>,
    },
    /// コミットのメッセージ全文（`text`）。
    CommitMessage {
        oid: String,
    },
    /// 途中の操作（`text` に "rebase"・"merge"・"cherry-pick"・"revert"、なければなし）。
    Sequence,
    /// 編集できるコミット（HEAD から first-parent で最初の merge の手前まで、新しい順）。`text` に改行区切り。
    /// `through_merges` なら merge を越えて first-parent で辿り、merge の行には ` merge` を添える（History の編集
    /// モード。merge は作り直す）。
    EditableCommits {
        #[serde(default)]
        through_merges: bool,
    },
    /// ローカルブランチの分け方（`info`）。`base` は既定のブランチ（`origin/HEAD` の先、なければ main・master）、
    /// `branches` はブランチごとの `name`・`time`（先端のコミットの日時）・`merged`（既定のブランチに取り込み済み）・
    /// `atBase`（既定のブランチの先端と同じ）・
    /// `mine`（取り込んでいない自分のコミットがある）・`parent`（その上に積んであるブランチ）・
    /// `archived`（`branch.<name>.amonhenArchived`）・`pinned`（お気に入りの順位。`branch.<name>.amonhenPinned`）・
    /// `pullRequest`（取り込んでいないが、`pullRequests` の PR が今の先端を head にしてマージされた。squash・rebase。
    /// その番号）。
    BranchGroups {
        #[serde(default)]
        pull_requests: Vec<MergedPullRequest>,
    },
    /// `oid` を親にまとめるときの、メッセージの初期値（親のメッセージ ＋ `oid` のメッセージ）。
    SquashMessage {
        oid: String,
    },
    /// 連続したコミットをまとめるときの、メッセージの初期値（`oids` のメッセージを古い順に並べる）。
    /// `info.oldest`・`info.newest` に、編集できる範囲の中で連続していることを確かめた両端を返す
    /// （`info.message` にも同じメッセージ）。
    SquashRangeMessage {
        oids: Vec<String>,
    },
    /// `path` の `index` 番目のコンフリクトを解決する（Undo できる）。
    ResolveConflict {
        path: String,
        index: usize,
        choice: crate::conflict::Choice,
    },
    /// コミットの詳細（`info`: message・author・authorEmail・authorTime・authorTz・committer・committerEmail・
    /// committerTime・committerTz・parents・parentsNotFetched（shallow clone の境界で、親を取得していない）・refs・
    /// signed・mergetag・trailers）。すぐ返せる分だけ。名前とメールは
    /// mailmap で置き換えたもので、記録と違えば recordedAuthor・recordedCommitter（name・email）に記録のものを添える。
    CommitInfo {
        oid: String,
    },
    /// コミットの詳細のうち時間のかかる分（`info`: signature・notes・stats・children・branches・tags・tagCount・
    /// firstTag・previousTag）。裏で読む。
    CommitExtras {
        oid: String,
    },
    /// 署名の検証の結果（`info`: `statuses` に `oids` と同じ順の `%G?` の 1 文字）。一度検証したものは覚えている。
    VerifySignatures {
        oids: Vec<String>,
    },
    /// タグの署名（`info`: `tags` に `names` と同じ順で name・id（注釈付きのタグのオブジェクト。軽量タグは
    /// null）・signed（gpg・ssh・x509、署名がなければ null）・taggerEmail・message（署名を除いたメッセージ）・
    /// signature（検証の結果。署名がなければ null））。署名のあるものだけを検証し、一度検証したものは覚えている。
    TagSignatures {
        names: Vec<String>,
    },
    /// 名前や短い ID が指すコミット（`text`: 40 桁の ID）。メッセージの中の ID のリンクに使う。
    ResolveCommit {
        rev: String,
    },
    /// `path` を片側にして解決済みにする。
    TakeSide {
        path: String,
        ours: bool,
    },
    /// マージエディタの中身（`info`）。`current`・`incoming`・`base`（diff3 のときだけ）は、すべての
    /// コンフリクトをその側にした `text` と、各コンフリクトの行範囲 `regions`（`[開始, 終わり)`、0 始まり）。
    /// `result` は今の作業ツリーの内容（マーカーつき）。
    MergeSides {
        path: String,
    },
    /// `text` の `index` 番目のコンフリクトを解決した文章（`text`）。ファイルには書かない。
    ResolveText {
        text: String,
        index: usize,
        choice: crate::conflict::Choice,
    },
    /// Magic Merge: `text` のコンフリクトのうち、決まった規則で解けるものを解いた文章（`info.text`）と、解いた数
    /// （`info.resolved`）・残った数（`info.remaining`）。ファイルには書かない。
    MagicMerge {
        text: String,
        /// 間違えようのない規則だけにする（ファイルを開いたときの自動の適用）。
        #[serde(default)]
        safe: bool,
        /// この番号のコンフリクトだけを解く（1 つずつ Undo できるように）。
        index: Option<usize>,
    },
    /// `text` に残っているコンフリクト（`info.conflicts`）。それぞれ、マーカーを含む行範囲 `lines` と、
    /// 両側の中身 `current`・`incoming`（マージエディタの上の欄のどのコンフリクトかを突き合わせる）。
    /// `originals`（最初にあったコンフリクトの両側の中身 `[current, incoming]` の並び）を渡すと、それぞれが
    /// その何番目に当たるかを `original` に入れる（手で書き換えたものも結ぶ。わからなければ null）。
    ConflictRegions {
        text: String,
        originals: Option<Vec<(String, String)>>,
    },
    /// `path` の各行を最後に変えたコミット（`info`: `commits` と、各行の `c`（commits の添字）・`t`（内容））。
    /// `rev` がなければ作業ツリーの内容。
    Blame {
        path: String,
        rev: Option<String>,
    },
    /// HEAD から `path` を変えたコミット（`info.entries`: oid・subject・author・time・その時点の path）。`limit`
    /// があれば新しい方からその数までで、`info.more` はその先もあるか。`merges` ならマージも出す（既定は出さない）。
    FileHistory {
        path: String,
        limit: Option<usize>,
        #[serde(default)]
        merges: bool,
    },
    /// `tip` から辿れて `base` から辿れないコミット（`git log base..tip`）。`info`: 全部の数 `count` と、新しい順に
    /// `limit` 個（既定 50）までの `commits`（oid・subject・author・time）。Push・Pull の前に、送る・取り込む
    /// コミットを見せる。
    Between {
        base: String,
        tip: String,
        limit: Option<usize>,
    },
    /// ref を `from` から `to` へ動かしたときの、確認のシートに描く前と後（Force Push・Push・Reset など。
    /// docs/design.md の「Previewing graph changes」）。`info`: `gainedCount`（`to` だけから辿れるコミットの数）、
    /// `lostCount`（`from` だけから辿れる、外れるコミットの数）、`files`（`to` から `from` までに変わったファイルの数。
    /// Reset で外したコミットの変更がどこへ行くかの文に使う）、`upstream`（`branch` の上流）と `upstreamHas`（その上流が、
    /// 外れるコミットをいくつ持っているか。Reset の後の Push に Force Push が要るか）、`drawing`（`gained`・`lost` は新しい順に
    /// 先頭のいくつか、`gainedMore`・`lostMore` は残りの数、`base` は分かれたところ、`labels` はそれぞれに付く
    /// ブランチ。同じなら null）。
    MovePreview {
        from: String,
        to: String,
        #[serde(default)]
        branch: Option<String>,
    },
    /// `theirs` を `ours`（既定は HEAD）に merge したときの前と後（Merge・Pull の確認に描く）。fast-forward で
    /// 済み、`ff_setting`（既定は `merge.ff`。Pull は `pull.ff`）が false でなければ `info.fastForward` を true に
    /// して、`info.move` に `MovePreview` と同じ答えを返す。取り込むものがなければ `info.upToDate`。そうでなければ
    /// `info.drawing`: `ours`・`theirs`（分かれてからのコミット。新しい順、長ければ真ん中を省く。`oursMore`・
    /// `oursMoreAt`・`theirsMore`・`theirsMoreAt`）、`base`（分かれたところと日時）、`labels`、`subject`（git が
    /// 書く merge コミットの件名。Pull（`ff_setting` が `pull.ff`）で `theirs` が上流なら、git pull と同じく
    /// `Merge branch 'x' of <URL>`）。
    MergePreview {
        theirs: String,
        #[serde(default)]
        ours: Option<String>,
        #[serde(default)]
        ff_setting: Option<String>,
    },
    /// いくつもの `theirs` を今のブランチに 1 つのコミットで merge したとき（octopus）の前と後。取り込むものが
    /// なければ `info.upToDate`。そうでなければ `info.drawing`: `ours`（すべてと分かれてからの今のブランチの
    /// コミット。新しい順、長ければ真ん中を省く。`oursMore`・`oursMoreAt`）、`sides`（`theirs` の順に `rev`・
    /// `tip`（先端。今のブランチにもうあれば null）・`more`（先端のほかに、それだけにあるコミットの数））、`base`
    /// （すべてが分かれたところと日時）、`labels`。
    OctopusPreview {
        theirs: Vec<String>,
    },
    /// コミット `oids`（古い順）を今のブランチに cherry-pick・revert したときの前と後。`info.drawing`: `head`
    /// （今の先端とその親）、`picks`（作るコミットの件名。新しい順、長ければ真ん中を省く。`picksMore`・
    /// `picksMoreAt`）、`labels`。
    PickPreview {
        #[serde(default)]
        oids: Vec<String>,
        #[serde(default)]
        revert: bool,
        /// `oids` の代わりに、作るコミットの件名（古い順。パッチの取り込みのように、まだコミットがないとき）。
        #[serde(default)]
        subjects: Vec<String>,
    },
    /// ブランチ `branch` を消したときの前と後（`MovePreview` と同じ形の答え）。`against`（今のブランチか上流）と
    /// 分かれたところへ動かしたものとして、`branch` にしかないコミットを外れるコミットとして返す。
    DeletePreview {
        branch: String,
        against: String,
    },
    /// `theirs` を `ours`（既定は HEAD）にマージしたらコンフリクトするファイル（`info.conflicts`。なければ空、
    /// git が古くて調べられなければ null）。作業ツリーと index には触らない（`git merge-tree --write-tree`）。
    /// Merge・Pull の前に見せる。rebase は最後の結果が同じになるので、目安に使える。`theirs` が配列なら、
    /// octopus と同じ順に重ねて調べ、`info.steps` に 1 つずつ（`rev`・`conflicts`）、`info.conflicts` に全部を返す。
    PredictMerge {
        ours: Option<String>,
        theirs: Revs,
    },
    /// untracked のファイルを消す（git の記録にないので Undo では戻せない）。
    DeleteUntracked {
        paths: Vec<String>,
    },
    /// `path` に残っているコンフリクトの数（`info.count`）。
    ConflictCount {
        path: String,
    },
    /// 直前に読んだ status の内容のコンフリクトそれぞれの、残っているマーカーの数と、rerere が前の解決を
    /// 当てたか（`info.files`: `{path, markers, rerere}`。`Repository::conflict_files`）。
    ConflictFiles,
    /// マージエディタの結果を `path` に書く（Undo できる）。`stage` なら解決済みにする（index に入れる）。
    SaveMerge {
        path: String,
        text: String,
        stage: bool,
    },
    /// すべての設定と、それぞれがどこで決まっているか（`info.entries`。後のものほど優先）。
    ConfigList,
    /// git が知っている設定の変数の名前（`info.keys`。設定の有無は問わない。`remote.<name>.url` のように
    /// 名前が入るものは含まない）。
    ConfigKeys,
    /// `format`（`openpgp`・`ssh`・`x509`）で署名に使える鍵（`info.keys`: `id`・`label`・`emails`）。
    /// SSH 先のリポジトリなら SSH 先で集める。集められなければ空。
    SigningKeys {
        format: String,
    },
    /// git を動かしている機械にある `name`（`git`・`gpg`・`gpgsm`・`ssh-keygen`）の候補（`info.programs`:
    /// `path`・`version`。先頭は PATH で最初に見つかるもの）。SSH 先のリポジトリなら SSH 先で探す。
    Programs {
        name: String,
    },
    /// git を動かしている機械のフォルダ `path` の中身（`info`: `path`（`~` を展開した絶対パス）・`home`・
    /// `entries`（`name`・`kind`（`folder`・`file`・`other`。シンボリックリンクならその先の種類）・`link`・
    /// `executable`・`repository`（`.git` のあるフォルダか）））。`~` で始まるパス・相対パスはその機械の
    /// ホームから。SSH 先のリポジトリなら SSH 先のもの。
    ListDirectory {
        path: String,
    },
    /// 設定を変える。`value` がなければ消す。`scope` が `"global"` なら、git を動かしているユーザー全体の
    /// 設定（SSH 先のリポジトリなら SSH 先のユーザーのもの）、なければこのリポジトリ（local）。`values` があれば
    /// `value` の代わりに、その段の値をすべてそれにする（いくつも値を持てる `remote.origin.fetch` などを消した後に
    /// 戻すとき）。
    ConfigSet {
        key: String,
        value: Option<String>,
        #[serde(default)]
        scope: Option<String>,
        #[serde(default)]
        values: Option<Vec<String>>,
    },
    /// git を動かす機械で使う git と、それをどこで決めたか（`info`: `chosen`（その機械の Amon Hen の設定
    /// `settings.json` で選んだもの。なければ null で自動）・`file`・`program`・`source`（`environment`・`settings`・
    /// `fallback`・`path`）・`problem`（使えないときの理由））。SSH 先のリポジトリなら SSH 先のもの。
    MachineGit,
    /// git を動かす機械の Amon Hen の設定に、使う git（`git`: 実行ファイルのパス）を書く。なければ消す（自動）。
    SetMachineGit {
        #[serde(default)]
        git: Option<String>,
    },
    /// このリポジトリを扱う git の版（`info.version`、読めなければ null）と、rebase で積み重ねたブランチを
    /// 一緒に動かせるか（`info.updateRefs`。git 2.38 から。版が読めなければ true）、`rr-cache` があるか
    /// （`info.rerereCache`。git は `rerere.enabled` が書かれていなければ、これがあるとき rerere を使う）。
    GitVersion,
    /// リポジトリの形（`info`: shallow・partial（remote・filter。なければ null）・sparse（cone・folders。cone mode
    /// なら取り出しているフォルダ、そうでなければパターン。なければ null）・maintenance（`git maintenance register`
    /// で登録してある）・workingTree・gitVersion・canFetchAllContents（git 2.36 から）・canSparseCheckout（git 2.25
    /// から）・canMaintain（git 2.31 から））。
    RepositoryForm,
    /// cone mode の sparse checkout で、取り出すフォルダを `folders`（根からの相対パス）にする。sparse checkout で
    /// なければ始める（git 2.25 から）。外れるフォルダに commit していない変更があれば断る。
    SparseCheckout {
        folders: Vec<String>,
    },
    /// sparse checkout をやめて、すべてのフォルダを取り出す（`git sparse-checkout disable`）。
    DisableSparseCheckout,
    /// `rev`（既定は HEAD）の木の `path`（`/crates` のように `/` から。`/` は根）にあるフォルダ（`info`:
    /// `listDirectory` と同じ形。path・home（null）・entries（kind は folder））。sparse checkout で取り出すフォルダや、
    /// アーカイブに書き出すフォルダを選ぶのに使う。
    TreeFolders {
        path: String,
        rev: Option<String>,
    },
    /// `on` なら、このリポジトリを git maintenance に登録して裏で手入れさせる（`git maintenance start`: ユーザー全体の
    /// `maintenance.repo` に足し、OS の予定がなければ作る）。でなければ登録を外す（`git maintenance unregister`。
    /// 予定はほかのリポジトリも使うので残し、自動の手入れを止める `maintenance.auto=false` は外す）。git を動かす機械
    /// （SSH 先ならそこ）の設定に書く。git 2.31 から。Undo には記録しない。
    Maintenance {
        on: bool,
    },
    /// git の設定から決まる、pull と fetch のふるまい（`info.pull`: `merge`・`rebase`・`fastForwardOnly`、
    /// `info.prune`）。Pull のシートが「git の設定に従う」の中身を見せるのに使う。今のブランチの
    /// `branch.<name>.rebase` は `pull.rebase` より先に見る（git と同じ）。
    ///
    /// `info.pullRefuses`: どれも決めていない（`branch.<name>.rebase`・`pull.rebase`・`pull.ff` がない）ときに、git
    /// が pull を断る場合。git 2.33.1 から、取り込み方を決めずに分かれたブランチを pull すると断る（`Need to specify
    /// how to reconcile divergent branches`）。2.34.1 からは両方に新しいコミットがあるとき（`diverged`）だけで、
    /// 2.33.1〜2.34.0 は fast-forward できなければ（手元だけが進んでいても）断る（`notFastForward`）。それより前の
    /// git や、どれかを決めていれば null（git は merge する）。
    SyncDefaults,
    /// リモート（`info.remotes`）。
    Remotes,
    /// HEAD と作業ツリーの、ファイルごとの追加・削除の行数（`info.stats`: `path`・`added`・`removed`。
    /// バイナリは null。untracked は含まない）。
    WorkingStats,
    /// `.gitignore` の書きかけの中身（`text`）の各行に合うファイルの例（`info.lines`: 行の番号・パターン・
    /// 例・合う数）。空行・# の行・! の行は飛ばす。多すぎないよう 40 行まで。
    IgnoreMatches {
        text: String,
    },
    /// hooks と Git LFS の様子（`info`: hooksPath・hooks（name・enabled）・lfs（入っているか）・lfsPatterns）。
    HooksAndLfs,
    /// SSH 先の daemon の Activity の記録（`info.records`。手元のリポジトリなら空）。
    RemoteActivity,
    /// 走っている操作を取り消す（`remote` なら SSH 先の daemon の記録の番号）。
    CancelActivity {
        seq: u64,
        #[serde(default)]
        remote: bool,
    },
    ChangeRemote {
        change: amonhen_model::RemoteChange,
    },
    /// `url` につないで、ブランチとタグの数と HEAD のブランチを読む（`info.branches`・`info.tags`・`info.head`。
    /// `git ls-remote`。SSH 先のリポジトリなら SSH 先から）。
    CheckRemote {
        url: String,
    },
    /// worktree の一覧（`info.worktrees`: path・head・branch・detached・bare・locked・prunable・main・current）。
    Worktrees,
    /// `paths` の worktree の変更の数（`info.worktrees`: path・count・stamp）。開いたリポジトリから読むので、
    /// worktree ごとに開かなくてよい。変わっていない worktree は前の数を返す（`fresh` なら全部を数え直す）。
    WorktreeChanges {
        paths: Vec<String>,
        #[serde(default)]
        fresh: bool,
    },
    /// `path` に worktree を作る（`git worktree add`）。`new_branch` があれば `start`（なければ HEAD）から作る。
    WorktreeAdd {
        path: String,
        start: Option<String>,
        new_branch: Option<String>,
    },
    /// `path` の worktree を消す（`git worktree remove`。`force` なら変更が残っていても）。Undo では戻せない。
    WorktreeRemove {
        path: String,
        #[serde(default)]
        force: bool,
    },
    /// フォルダがなくなった worktree の記録を片付ける（`git worktree prune`）。
    WorktreePrune,
    /// `path` の worktree をロックする（`git worktree lock`。prune・remove・move で消されない）。`reason` は理由。
    WorktreeLock {
        path: String,
        reason: Option<String>,
    },
    WorktreeUnlock {
        path: String,
    },
    /// `path` の worktree のフォルダを `to` に移す（`git worktree move`）。
    WorktreeMove {
        path: String,
        to: String,
    },
    /// 手で移した worktree とリポジトリのつながりを直す（`git worktree repair`、git 2.29 から）。`paths` は移した
    /// worktree の今の場所（空なら記録にある場所だけ）。
    WorktreeRepair {
        #[serde(default)]
        paths: Vec<String>,
    },
    /// `.gitignore`（作業ツリーの直下）と `.git/info/exclude` の中身（`info.gitignore`・`info.exclude`）。
    IgnoreFiles,
    /// フックの実行の許可を付ける・外す（`enabled`）。フックの置き場所は `hooksAndLfs` と同じ。
    SetHookEnabled {
        name: String,
        enabled: bool,
    },
    /// フックの中身（`info`: text。なければ null）。置き場所は `hooksAndLfs` と同じ。
    Hook {
        name: String,
    },
    /// フックを作る: `#!/bin/sh` だけの雛形を置いて実行の許可を付ける。すでにあればエラー。
    CreateHook {
        name: String,
    },
    /// `git lfs track <pattern>`（`track` が false なら `untrack`）。
    LfsTrack {
        pattern: String,
        track: bool,
    },
    /// git を動かしているユーザー全体の、無視するパターンのファイル（`core.excludesFile`。なければ git の既定
    /// `$XDG_CONFIG_HOME/git/ignore`・`~/.config/git/ignore`）のパスと中身（`info.path`・`info.text`。ファイルが
    /// なければ null）。SSH 先のリポジトリなら SSH 先のもの。
    ExcludesFile,
    /// そのファイルを書く（途中のフォルダがなければ作る）。
    SaveExcludesFile {
        text: String,
    },
    /// git を動かしているプロセスの環境変数（`info.vars`: `name`・`value`。名前の順）。SSH 先のリポジトリなら
    /// SSH 先のエンジンのもので、そのまま git に渡る（Settings > SSH Hosts の「環境変数」）。
    ProcessEnvironment,
    /// `.gitignore` と `.git/info/exclude` を書く。
    SaveIgnoreFiles {
        gitignore: String,
        exclude: String,
    },
    /// このリポジトリ専用のコミットのメッセージのテンプレート（`<git dir>/amonhen/commit-template`）の中身
    /// （`info.text`。なければ null）と、git の `commit.template` に書くそのパス（`info.path`）。
    CommitTemplate,
    /// 専用のテンプレートを書き、`commit.template`（local）をそれに向ける。
    SaveCommitTemplate {
        text: String,
    },
}

/// リポジトリ専用のコミットのテンプレートの置き場所（`<git dir>` から）。
const COMMIT_TEMPLATE: &str = "amonhen/commit-template";

/// コミットの本文（ヘッダ）から、最初の親。
fn first_parent(raw: &[u8]) -> Option<Oid> {
    raw.split(|&b| b == b'\n')
        .take_while(|line| !line.is_empty())
        .find_map(|line| line.strip_prefix(b"parent "))
        .and_then(|hex| Oid::from_hex(std::str::from_utf8(hex).ok()?.trim()))
}

/// 前と後の絵に描くコミットの件名と日時（`Repository::drawing_texts`）。
type DrawingTexts = std::collections::HashMap<Oid, (String, i64)>;

/// git pull が書く merge コミットの件名（`into` の前まで）。`branch` の上流（`branch.<branch>.remote` の
/// `branch.<branch>.merge`）が `theirs` のときだけ。git は取ってきたところ（FETCH_HEAD）から、`Merge branch 'main' of
/// <URL>` と書く（URL は利用者名とパスワードを除き、末尾の `/` と `.git` を落とす。手元のブランチ（remote が `.`）
/// なら `of` を付けない）。
fn pull_merge_subject(
    ops: &dyn amonhen_model::Operations,
    branch: &str,
    theirs: &str,
) -> Result<Option<String>> {
    let config = ops.config_list()?;
    let value = |key: &str| {
        config
            .iter()
            .rev()
            .find(|e| e.key == key)
            .map(|e| e.value.clone())
    };
    let (Some(remote), Some(merge)) = (
        value(&format!("branch.{branch}.remote")),
        value(&format!("branch.{branch}.merge")),
    ) else {
        return Ok(None);
    };
    let what = if let Some(name) = merge.strip_prefix("refs/heads/") {
        format!("branch '{name}'")
    } else if let Some(name) = merge.strip_prefix("refs/tags/") {
        format!("tag '{name}'")
    } else {
        format!("'{merge}'")
    };
    if remote == "." {
        return Ok(
            (theirs == merge.strip_prefix("refs/heads/").unwrap_or(&merge))
                .then(|| format!("Merge {what}")),
        );
    }
    let tracking = merge
        .strip_prefix("refs/heads/")
        .map(|name| format!("{remote}/{name}"));
    if tracking.as_deref() != Some(theirs) {
        return Ok(None);
    }
    let Some(url) = value(&format!("remote.{remote}.url")) else {
        return Ok(None);
    };
    Ok(Some(format!("Merge {what} of {}", fetched_url(&url))))
}

/// git fetch が FETCH_HEAD に書く URL（`transport_anonymize_url` で利用者名とパスワードを除き、末尾の `/` と
/// `.git` を落とす）。
fn fetched_url(url: &str) -> String {
    let anonymous = (|| {
        let at = url.find('@')?;
        let rest = &url[at + 1..];
        match url.find("://") {
            Some(scheme) => {
                let ok = url[..scheme]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
                // パスの中の @ は数えない。
                let slash = url[scheme + 3..].find('/').map(|i| i + scheme + 3);
                (ok && slash.is_none_or(|s| s > at))
                    .then(|| format!("{}{rest}", &url[..scheme + 3]))
            }
            // `me@there:/path` の形だけ（`:` がなければ手元のパス）。
            None => (rest.contains(':') && !is_local_path(url)).then(|| rest.to_owned()),
        }
    })()
    .unwrap_or_else(|| url.to_owned());
    let trimmed = anonymous.trim_end_matches('/');
    trimmed
        .strip_suffix(".git")
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed)
        .to_owned()
}

/// git が手元のパスとみなす URL（`url_is_local_not_ssh`: 最初の `:` より前に `/` がある、`:` がない、または
/// Windows のドライブ（`C:`）で始まる）。
fn is_local_path(url: &str) -> bool {
    let bytes = url.as_bytes();
    if cfg!(windows) && bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return true;
    }
    match (url.find(':'), url.find('/')) {
        (None, _) => true,
        (Some(colon), Some(slash)) => slash < colon,
        (Some(_), None) => false,
    }
}

/// 前と後の絵に描くコミット（新しい順）を、長ければ真ん中で省く: 新しい 3 つと、分かれたところに近い古い 2 つ。
/// 描くもの、省いた数、省いたところ（描くものの何番目の前か）を返す。
fn elide<T: Clone>(commits: &[T]) -> (Vec<T>, usize, usize) {
    const NEW: usize = 3;
    const OLD: usize = 2;
    if commits.len() <= NEW + OLD + 1 {
        return (commits.to_vec(), 0, commits.len());
    }
    let mut shown = commits[..NEW].to_vec();
    shown.extend_from_slice(&commits[commits.len() - OLD..]);
    (shown, commits.len() - NEW - OLD, NEW)
}

impl Repository {
    /// 専用のテンプレートのファイルのパス（git の `commit.template` に書く。絶対パス）。
    fn commit_template_path(&self) -> String {
        self.backend()
            .paths()
            .git_dir
            .join(COMMIT_TEMPLATE)
            .to_string_lossy()
            .into_owned()
    }
}

impl Repository {
    /// shallow clone の履歴を取り足す（`Command::Deepen`・`Command::Unshallow`）。shallow clone でなければ、git の
    /// 分かりにくいエラー（`--unshallow on a complete repository`）の前に断る。
    fn deepen(
        &self,
        ops: &dyn amonhen_model::Operations,
        how: &amonhen_model::Deepen,
    ) -> Result<()> {
        if self.backend().shallow()?.is_empty() {
            return Err(tr!("This repository has all of its history (not a shallow clone)").into());
        }
        ops.deepen(how)
    }

    /// `Command::PreviousVersion` の答え（なければ `None`）。
    fn previous_version(
        &self,
        ops: &dyn amonhen_model::Operations,
        branch: Option<&str>,
    ) -> Result<Option<serde_json::Value>> {
        let full = match branch {
            Some(name) if name.starts_with("refs/") => name.to_owned(),
            Some(name) => heads(name),
            None => ops
                .head_state()?
                .branch
                .ok_or_else(|| tr!("HEAD isn’t on a branch"))?,
        };
        let short = full.strip_prefix("refs/heads/").unwrap_or(&full).to_owned();
        let entries = self.backend().reflog(&full, 500)?;
        let Some(tip) = entries.first().map(|e| e.new) else {
            return Ok(None);
        };
        for (i, e) in entries.iter().enumerate() {
            let Some(old) = e.old else { continue };
            if old == e.new || amonhen_model::reflog_keeps_history(&e.message) {
                continue;
            }
            // 消えた（gc 済みの）コミットは比べられないので、そこで探すのをやめる。
            let Ok(kept) = ops.is_ancestor(&old.to_string(), &e.new.to_string()) else {
                break;
            };
            if kept {
                continue;
            }
            let onto = match amonhen_model::rebase_onto(&e.message) {
                Some(o) => ops.resolve(o).ok(),
                None if e.message.starts_with("rebase") => self
                    .backend()
                    .reflog("HEAD", 500)
                    .ok()
                    .and_then(|head| amonhen_model::rebase_onto_from_head(&head, e)),
                None => None,
            }
            .map(|o| o.to_string());
            let (old, tip) = (old.to_string(), tip.to_string());
            let ranges = match &onto {
                Some(onto) => vec![onto.clone(), old.clone(), tip.clone()],
                None => vec![format!("{old}...{tip}")],
            };
            return Ok(Some(serde_json::json!({
                "branch": short,
                "selector": format!("{short}@{{{}}}", i + 1),
                "message": e.message,
                "time": e.time,
                "old": old,
                "new": tip,
                "onto": onto,
                "ranges": ranges,
            })));
        }
        Ok(None)
    }
}

/// マージされた PR（FE が GitHub から読む）。`branch` は head のブランチの名前、`oid` はマージしたときの head。
#[derive(Clone, Debug, Deserialize)]
pub struct MergedPullRequest {
    pub number: u64,
    pub branch: String,
    pub oid: String,
}

/// `git branch -d` が断った文に、わけと次の一手を添える（取り込んだ先が今のブランチでも上流でもなければ断る。Clean Up
/// Branches は既定のブランチで見分けるので、別のブランチから実行すると起きる）。ほかの失敗は、そのまま。
fn explain_refused_delete(
    name: &str,
    deleted: usize,
    error: amonhen_model::Error,
) -> amonhen_model::Error {
    if !error.to_string().contains("not fully merged") {
        return error;
    }
    let reason = tr!(
        "“{name}” wasn’t deleted: git only deletes a branch merged into the current branch or its upstream. Switch to the branch it’s merged into and try again.",
        name = name
    );
    match deleted {
        0 => reason.into(),
        _ => format!(
            "{reason} {}",
            tr!("Already deleted: {count}.", count = deleted)
        )
        .into(),
    }
}

/// `Command::DeleteBranches` で消すブランチ。`force` なら取り込んでいなくても消す（`git branch -D`）。
#[derive(Clone, Debug, Deserialize)]
pub struct BranchToDelete {
    pub name: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub ok: bool,
    /// "done"・"conflict"・"stopped"。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<&'static str>,
    /// Undo した・次に Undo される操作の名前。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 失敗の種類（`failure` の定数）。見分けられなければ付けない。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<&'static str>,
    /// 失敗に関わるパス（ほかの worktree の場所、ロックファイル）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_path: Option<String>,
}

/// 内容のコンフリクトのファイル 1 つの状態（`Repository::conflict_files`）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ConflictFile {
    pub path: String,
    /// 作業ツリーに残っているコンフリクトのマーカーの数。0 なら、中身はもう解決されているかもしれない。
    pub markers: usize,
    /// git の rerere が前の解決を当てた。
    pub rerere: bool,
}

impl Response {
    /// 失敗の答え。種類を見分けられれば添える。
    pub fn failed(message: String) -> Self {
        let failure = crate::failure::classify(&message);
        Self {
            error_kind: failure.as_ref().map(|f| f.kind),
            error_path: failure.and_then(|f| f.path),
            error: Some(message),
            ..Default::default()
        }
    }
}

fn outcome_name(o: Outcome) -> &'static str {
    match o {
        Outcome::Done => "done",
        Outcome::Conflict => "conflict",
        Outcome::Stopped => "stopped",
    }
}

/// 命令の JSON の `op`（Activity に出す名前）。
fn op_name(json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("op").and_then(|o| o.as_str()).map(str::to_owned))
        .unwrap_or_default()
}

/// 手元のタグ `names` の ref が指すオブジェクト（注釈付きのタグはタグのオブジェクト）。リモートのタグと比べる
/// のに使う: 同じコミットでも、作り直した注釈付きのタグは別のタグで、push は断られる。
fn tag_objects<'a>(
    ops: &dyn amonhen_model::Operations,
    names: &[&'a str],
) -> Result<Vec<(&'a str, Option<Oid>)>> {
    let full: Vec<String> = names.iter().map(|n| format!("refs/tags/{n}")).collect();
    let objects = ops.resolve_refs(&full)?;
    Ok(names
        .iter()
        .zip(objects)
        .map(|(&n, (_, oid))| (n, oid))
        .collect())
}

fn oid(hex: &str) -> Result<Oid> {
    Oid::from_hex(hex).ok_or_else(|| tr!("Can’t read the commit ID: {hex}", hex = hex).into())
}

/// Cherry-pick・Revert のコミット（`oids` があればそれ、なければ `oid`）と、Undo の名前に出す言い方（1 つなら
/// 短い ID、いくつもなら数）。
fn picked(one: String, many: Vec<String>) -> Result<(Vec<Oid>, String)> {
    let hexes = if many.is_empty() { vec![one] } else { many };
    let ids = hexes.iter().map(|h| oid(h)).collect::<Result<Vec<_>>>()?;
    let what = match hexes.as_slice() {
        [one] => one[..one.len().min(8)].to_owned(),
        _ => format!("{} commits", hexes.len()),
    };
    Ok((ids, what))
}

/// `git am` が止まったパッチを 3-way で当て直したときに、その番号（`rebase-apply/next`）を書いておくファイル。
/// am が終わるか中止されると git が rebase-apply ごと消すので、次の am に残らない。
const AM_RETRIED: &str = "rebase-apply/amonhen-retried";

/// パッチのファイルを並べた順につなぐ。メールの区切りの行（`From `）がないもの（.eml など）には、
/// `git format-patch` と同じ形の区切りを足す（`git am` が 1 通ずつに分けられるように）。
fn read_patches(paths: &[String]) -> Result<Vec<u8>> {
    if paths.is_empty() {
        return Err(tr!("No patch files").into());
    }
    let mut out = Vec::new();
    for path in paths {
        let data = std::fs::read(path)
            .map_err(|e| tr!("Can’t read {path}: {e}", path = path, e = e.to_string()))?;
        if !String::from_utf8_lossy(&data)
            .trim_start()
            .starts_with("From ")
            && crate::patch::parse(&data).kind == crate::patch::Kind::Mailbox
        {
            out.extend_from_slice(
                b"From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001\n",
            );
        }
        out.extend_from_slice(&data);
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
    }
    Ok(out)
}

fn heads(name: &str) -> String {
    format!("refs/heads/{name}")
}

/// Undo の記録に出す、いくつかの名前（`a`、`a and b`、`a, b and c`。git の merge のメッセージと同じ形）。
fn names_label(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

impl Repository {
    /// `revs` を今のブランチに merge する（`Command::Merge`）。コンフリクトや途中で止まったときは記録しない
    /// （戻すのは Abort の役目）。1 つずつのときは、止まる前に済んだ分を 1 つの記録にする。
    fn merge_revs(
        &self,
        ops: &dyn amonhen_model::Operations,
        undo: &crate::undo::UndoLog,
        label: &str,
        revs: &[String],
        one_by_one: bool,
    ) -> Result<Response> {
        if revs.is_empty() {
            return Err(tr!("Nothing to merge").into());
        }
        if !one_by_one || revs.len() == 1 {
            let outcome = undo.record(
                ops,
                label,
                &[],
                false,
                || ops.merge(revs),
                |o| *o != Outcome::Done,
            )?;
            return Ok(Response {
                ok: true,
                outcome: Some(outcome_name(outcome)),
                ..Default::default()
            });
        }
        // 途中で失敗しても、済んだ分は記録してからエラーを返す。
        let (outcome, remaining, error) = undo.record(
            ops,
            label,
            &[],
            false,
            || {
                for (i, rev) in revs.iter().enumerate() {
                    match ops.merge(std::slice::from_ref(rev)) {
                        Ok(Outcome::Done) => {}
                        Ok(outcome) => return Ok((outcome, revs[i + 1..].to_vec(), None)),
                        Err(e) => return Ok((Outcome::Stopped, revs[i..].to_vec(), Some(e))),
                    }
                }
                Ok((Outcome::Done, Vec::new(), None))
            },
            |_| false,
        )?;
        if let Some(e) = error {
            return Err(e);
        }
        Ok(Response {
            ok: true,
            outcome: Some(outcome_name(outcome)),
            info: (!remaining.is_empty()).then(|| serde_json::json!({ "remaining": remaining })),
            ..Default::default()
        })
    }

    /// `Command::BranchCleanup` の答え。
    fn branch_cleanup(&self, pull_requests: &[MergedPullRequest]) -> Result<serde_json::Value> {
        let groups = self.branch_groups(pull_requests)?;
        let refs = self.refs()?;
        let entries = &refs.refs.entries;
        let base = groups["base"].as_str();
        // チェックアウトしているブランチ（この worktree もほかの worktree も。git も消させない）。
        let mut checked_out: std::collections::HashSet<String> = self
            .backend()
            .operations()?
            .worktree_list()
            .map(|list| {
                crate::worktree::parse(&list, self.paths().workdir.as_deref())
                    .into_iter()
                    .filter_map(|w| w.branch)
                    .collect()
            })
            .unwrap_or_default();
        checked_out.extend(refs.refs.head_branch.clone());
        // 既定のブランチの先端にあるもののうち、作ってからコミットしていないもの（reflog で作った点が先端のまま。
        // 読めなければそう見なす）。fast-forward で取り込んだものは、作った点が先端より前にあるので片付けてよい。
        let at_base: Vec<&str> = groups["branches"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["atBase"] == true)
            .filter_map(|b| b["name"].as_str())
            .collect();
        let fresh: std::collections::HashSet<&str> = at_base
            .iter()
            .zip(self.backend().branch_created_at(&at_base)?)
            .filter(|(name, created)| {
                let tip = entries
                    .iter()
                    .find(|e| e.kind == RefKind::Local && e.name == **name)
                    .and_then(|e| e.target);
                created.is_none() || *created == tip
            })
            .map(|(name, _)| *name)
            .collect();
        let mut found = Vec::new();
        for b in groups["branches"].as_array().into_iter().flatten() {
            let Some(name) = b["name"].as_str() else {
                continue;
            };
            let Some(entry) = entries
                .iter()
                .find(|e| e.kind == RefKind::Local && e.name == name)
            else {
                continue;
            };
            // 既定のブランチ（手元のものと、上流が既定のブランチのもの）・お気に入り・作ったばかりのものは残す。
            if checked_out.contains(name)
                || Some(name) == base
                || (base.is_some() && entry.detail.as_deref() == base)
                || !b["pinned"].is_null()
                || fresh.contains(name)
            {
                continue;
            }
            let pr = b["pullRequest"].as_u64();
            let (reason, force, checked) = if b["merged"] == true {
                ("merged", false, true)
            } else if pr.is_some() {
                ("pullRequest", true, true)
            } else if entry.gone {
                ("gone", true, false)
            } else {
                continue;
            };
            found.push(serde_json::json!({
                "name": name,
                "reason": reason,
                "force": force,
                "checked": checked,
                "pullRequest": pr,
                "gone": entry.gone,
                "upstream": entry.detail,
                "time": b["time"],
            }));
        }
        Ok(serde_json::json!({ "base": base, "branches": found }))
    }

    /// `Command::BranchGroups` の答え。
    fn branch_groups(&self, pull_requests: &[MergedPullRequest]) -> Result<serde_json::Value> {
        use crate::branches;
        let history = self.load_history()?;
        // History と同じく今の ref を読む（変化の知らせより先に頼まれることがある。git で作った直後など）。
        let refs = self.load_refs()?;
        let entries = &refs.refs.entries;
        let config = self.backend().operations()?.config_list()?;
        let config_value = |key: &str| {
            config
                .iter()
                .rev()
                .find(|e| e.key.eq_ignore_ascii_case(key))
                .map(|e| e.value.clone())
        };
        // 既定のブランチ: 利用者が `amonhen.defaultBranch` で決めていればそのローカルブランチ。なければリモートの
        // HEAD（origin を先に）が指すもの、それもなければ手元の main・master。
        let chosen = config_value("amonhen.defaultBranch").and_then(|name| {
            entries
                .iter()
                .find(|e| e.kind == RefKind::Local && e.name == name)
        });
        let remote_head = entries
            .iter()
            .filter(|e| e.kind == RefKind::Remote && e.name.ends_with("/HEAD"))
            .min_by_key(|e| !e.name.starts_with("origin/"));
        let base = chosen
            .or_else(|| {
                remote_head.and_then(|h| {
                    let remote = h.name.trim_end_matches("/HEAD");
                    entries.iter().find(|e| {
                        e.kind == RefKind::Remote
                            && e.target == h.target
                            && e.name.starts_with(remote)
                            && !e.name.ends_with("/HEAD")
                    })
                })
            })
            .or_else(|| {
                ["main", "master"].iter().find_map(|n| {
                    entries
                        .iter()
                        .find(|e| e.kind == RefKind::Local && e.name == *n)
                })
            });
        let graph = &history.graph;
        // 既定のブランチは、手元のもの（main）とリモートのもの（origin/main）のどちらかが先に進んでいることが多い。
        // 両方とも起点にする（片方だけだと、もう片方にしかないコミットを固有と見誤る）。
        let local_base = base.and_then(|b| match b.kind {
            RefKind::Local => Some(b),
            _ => entries
                .iter()
                .find(|l| l.kind == RefKind::Local && l.detail.as_ref() == Some(&b.name)),
        });
        let upstream_base = local_base.and_then(|l| l.detail.as_ref()).and_then(|u| {
            entries
                .iter()
                .find(|e| e.kind == RefKind::Remote && &e.name == u)
        });
        let roots: Vec<u32> = [base, local_base, upstream_base]
            .into_iter()
            .flatten()
            .filter_map(|e| e.target.as_ref().and_then(|t| history.row_of(t)))
            .collect();
        let in_base = branches::reachable(graph, &roots);
        // 固有のコミットは 1 本につき 200 まで見る（自分のものか・共有しているかには十分）。
        const LIMIT: usize = 200;
        let locals: Vec<&RefEntry> = entries
            .iter()
            .filter(|e| e.kind == RefKind::Local)
            .collect();
        let facts: Vec<Option<branches::BranchFacts>> = locals
            .iter()
            .map(|e| {
                e.target
                    .as_ref()
                    .and_then(|t| history.row_of(t))
                    .map(|row| branches::facts(graph, &in_base, row, LIMIT))
            })
            .collect();
        // 自分のコミットか: 固有のコミットの作者のメールアドレスを、まとめて 1 回で読む。
        let me = config_value("user.email").map(|e| e.to_lowercase());
        let mut mine_rows = std::collections::HashSet::new();
        if let Some(me) = &me {
            let mut rows: Vec<u32> = facts
                .iter()
                .flatten()
                .flat_map(|f| f.unique.iter().copied())
                .collect();
            rows.sort_unstable();
            rows.dedup();
            let ids: Vec<Oid> = rows.iter().map(|&r| graph.ids()[r as usize]).collect();
            // mailmap で置き換えたメールでもよい（前のメールで書いたコミットも自分のもの）。
            let mailmap = self.mailmap();
            for (row, text) in rows.iter().zip(self.backend().commit_texts(&ids)?) {
                let mapped = text.author(mailmap.as_deref()).email;
                if branches::author_email(&text.raw).as_deref() == Some(me.as_str())
                    || mapped.eq_ignore_ascii_case(me.as_bytes())
                {
                    mine_rows.insert(*row);
                }
            }
        }
        let tips: Vec<Option<u32>> = locals
            .iter()
            .map(|e| e.target.as_ref().and_then(|t| history.row_of(t)))
            .collect();
        // 土台を探す手がかり。上流が `origin/x` のブランチは、上流が同じ `x`（幹）の上に積んだものと見なす。
        let local_of_upstream = |upstream: &str| {
            locals.iter().position(|l| {
                l.detail.as_deref() == Some(upstream)
                    && upstream.rsplit_once('/').is_some_and(|(_, n)| n == l.name)
            })
        };
        // ブランチを作った点（reflog の最初の行）。土台が分かれたあとに進んでも、土台と積んだほうを取り違えない
        // ため。取り込み済みのものには要らない。置き場所（linked worktree では共通の git dir）はバックエンドが
        // 知っているので、まとめて 1 回で聞く（daemon 越しでも往復は 1 回）。
        let merged: Vec<bool> = facts
            .iter()
            .map(|f| f.as_ref().is_some_and(|f| f.merged))
            .collect();
        let wanted: Vec<usize> = (0..locals.len())
            .filter(|&i| !merged[i] && tips[i].is_some())
            .collect();
        let names: Vec<&str> = wanted.iter().map(|&i| locals[i].name.as_str()).collect();
        let mut created_at = vec![None; locals.len()];
        for (&i, oid) in wanted.iter().zip(self.backend().branch_created_at(&names)?) {
            created_at[i] = oid.and_then(|oid| history.row_of(&oid));
        }
        let candidates: Vec<branches::Candidate> = locals
            .iter()
            .zip(&tips)
            .zip(merged.iter().zip(&created_at))
            .map(|((e, &tip), (&merged, &created_at))| branches::Candidate {
                tip,
                trunk: e
                    .detail
                    .as_deref()
                    .is_some_and(|u| u.rsplit_once('/').is_some_and(|(_, n)| n == e.name)),
                has_upstream: e.detail.is_some(),
                upstream_local: e.detail.as_deref().and_then(local_of_upstream),
                merged,
                created_at,
            })
            .collect();
        let parents = branches::parents(graph, &in_base, &candidates);
        // 取り込んでいないが、PR が今の先端を head にしてマージされたもの（squash・rebase で取り込んだ）。
        let pull_request_of = |e: &RefEntry, merged: bool| {
            let tip = e.target.map(|t| t.to_string())?;
            pull_requests
                .iter()
                .find(|p| !merged && p.branch == e.name && p.oid == tip)
                .map(|p| p.number)
        };
        let branches: Vec<serde_json::Value> = locals
            .iter()
            .zip(&facts)
            .zip(tips.iter().zip(&parents))
            .map(|((e, f), (&row, parent))| {
                let unique = f.as_ref().map_or(&[][..], |f| &f.unique[..]);
                serde_json::json!({
                    "name": e.name,
                    "time": row.map(|r| graph.commit_times()[r as usize]),
                    "merged": f.as_ref().is_some_and(|f| f.merged),
                    // 既定のブランチの先端と同じ（作ったばかりで、まだコミットしていない）。
                    "atBase": row.is_some_and(|r| roots.contains(&r)),
                    "mine": unique.iter().any(|r| mine_rows.contains(r)),
                    "parent": parent.map(|p| &locals[p].name),
                    "archived": config_value(&format!("branch.{}.amonhenarchived", e.name)).is_some_and(|v| v == "true"),
                    // お気に入り（サイドバーの一番上に出す）の順位。小さいほど上。
                    "pinned": config_value(&format!("branch.{}.amonhenpinned", e.name)).and_then(|v| v.parse::<i64>().ok()),
                    "pullRequest": pull_request_of(e, f.as_ref().is_some_and(|f| f.merged)),
                })
            })
            .collect();
        Ok(serde_json::json!({ "base": base.map(|b| &b.name), "branches": branches }))
    }

    /// 途中の操作（rebase・merge・cherry-pick・revert）の詳しいこと。FE の帯に、何をどこへしている途中で、
    /// どこで止まっているかを出す。git dir の中の git の作業ファイルから読む（読めないものは付けない）。
    ///
    /// `{"kind", "current", "total", "branch", "onto"（rebase の先）, "from"（merge で取り込む側）,
    /// "merging"（octopus で取り込むものすべて）,
    /// "step": {"action", "oid", "subject"}, "editing"}`
    fn sequence_info(
        &self,
        sequence: amonhen_model::Sequence,
        progress: Option<(u32, u32)>,
    ) -> Result<serde_json::Value> {
        use amonhen_model::Sequence;
        let files = self.backend().files();
        let read = |path: &str| -> Option<String> {
            let data = files.read(Root::GitDir, path).ok()??;
            Some(String::from_utf8_lossy(&data).trim().to_owned()).filter(|t| !t.is_empty())
        };
        let mut info = serde_json::json!({ "kind": sequence.command() });
        if let Some((current, total)) = progress {
            info["current"] = current.into();
            info["total"] = total.into();
        }
        // コミットを指す ref の名前（ローカルブランチ、リモートブランチ、タグの順）。なければ短い ID。
        let name_of = |oid: &Oid| -> String {
            let refs = self.refs().ok();
            let mut found: Vec<&RefEntry> = refs
                .iter()
                .flat_map(|r| r.refs.entries.iter())
                .filter(|e| e.target.as_ref() == Some(oid) && (e.kind as u8) <= RefKind::Tag as u8)
                .collect();
            found.sort_by_key(|e| e.kind as u8);
            found
                .first()
                .map_or_else(|| oid.to_string()[..8].to_owned(), |e| e.name.clone())
        };
        let subject_of = |oid: &Oid| -> Option<String> {
            let text = self
                .backend()
                .commit_texts(std::slice::from_ref(oid))
                .ok()?
                .pop()?;
            let range = text.subject.start as usize..text.subject.end as usize;
            Some(String::from_utf8_lossy(text.raw.get(range)?).into_owned())
        };
        match sequence {
            Sequence::Rebase => {
                let dir = if read("rebase-merge/head-name").is_some() {
                    "rebase-merge"
                } else {
                    "rebase-apply"
                };
                if let Some(head) = read(&format!("{dir}/head-name")) {
                    info["branch"] = head.strip_prefix("refs/heads/").unwrap_or(&head).into();
                }
                if let Some(onto) = read(&format!("{dir}/onto")).and_then(|h| Oid::from_hex(&h)) {
                    info["onto"] = name_of(&onto).into();
                }
                // 最後に行った手順（`pick 1a2b3c4 件名`）。ここで止まっている。
                if let Some(done) = read("rebase-merge/done")
                    && let Some(line) = done
                        .lines()
                        .rev()
                        .find(|l| !l.starts_with('#') && !l.trim().is_empty())
                {
                    let mut parts = line.splitn(3, ' ');
                    let action = parts.next().unwrap_or_default();
                    let mut step = serde_json::json!({ "action": action });
                    if action == "exec" {
                        step["subject"] = line["exec".len()..].trim().into();
                    } else {
                        if let Some(oid) = parts.next() {
                            step["oid"] = oid.into();
                        }
                        if let Some(subject) = parts.next() {
                            step["subject"] = subject.trim_start_matches("# ").into();
                        }
                    }
                    info["step"] = step;
                }
                // edit・break で止めた（コンフリクトではない）。
                info["editing"] = read("rebase-merge/amend").is_some().into();
            }
            Sequence::Merge => {
                // octopus なら MERGE_HEAD に取り込むものが 1 行ずつ並ぶ（`merging` に全部）。
                let heads: Vec<String> = read("MERGE_HEAD")
                    .map(|h| {
                        h.lines()
                            .filter_map(Oid::from_hex)
                            .map(|o| name_of(&o))
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(first) = heads.first() {
                    info["from"] = first.clone().into();
                }
                if heads.len() > 1 {
                    info["merging"] = heads.into();
                }
                if let Some(message) = read("MERGE_MSG") {
                    info["step"] = serde_json::json!({
                        "action": "merge",
                        "subject": message.lines().next().unwrap_or_default(),
                    });
                }
            }
            Sequence::CherryPick | Sequence::Revert => {
                let head = if sequence == Sequence::Revert {
                    "REVERT_HEAD"
                } else {
                    "CHERRY_PICK_HEAD"
                };
                if let Some(oid) = read(head).and_then(|h| Oid::from_hex(&h)) {
                    info["step"] = serde_json::json!({
                        "action": sequence.command(),
                        "oid": oid.to_string(),
                        "subject": subject_of(&oid),
                    });
                }
            }
            Sequence::Am => {
                // 止まっているパッチの件名（`info` は git mailinfo が書いたもの。なければメッセージの 1 行目）。
                let subject = read("rebase-apply/info")
                    .and_then(|info| {
                        info.lines()
                            .find_map(|l| l.strip_prefix("Subject: ").map(str::to_owned))
                    })
                    .or_else(|| {
                        read("rebase-apply/final-commit")
                            .and_then(|m| m.lines().next().map(str::to_owned))
                    });
                info["step"] = serde_json::json!({ "action": "am", "subject": subject });
                // 3-way で当てようとしたか（していなければ、当て直せる）。
                info["threeWay"] = (read("rebase-apply/threeway").as_deref() == Some("t")).into();
                // diff のないパッチ（送り状など）で止まった。
                info["empty"] = read("rebase-apply/patch").is_none().into();
                // このパッチを 3-way で当て直したところ（残っているのはコンフリクトか、その解決）。
                info["retried"] = (read(AM_RETRIED).is_some()
                    && read(AM_RETRIED) == read("rebase-apply/next"))
                .into();
            }
        }
        if let Some(branch) = self.refs().ok().and_then(|r| r.refs.head_branch.clone())
            && info.get("branch").is_none()
        {
            info["branch"] = branch.into();
        }
        Ok(info)
    }

    /// コミットのメッセージ全文。
    pub fn commit_message(&self, commit: &Oid) -> Result<String> {
        let text = self
            .backend()
            .commit_texts(std::slice::from_ref(commit))?
            .remove(0);
        // ヘッダーの後の最初の空行から後ろがメッセージ。
        let raw = &text.raw[..];
        let start = raw
            .windows(2)
            .position(|w| w == b"\n\n")
            .map_or(raw.len(), |i| i + 2);
        Ok(String::from_utf8_lossy(&raw[start..]).into_owned())
    }

    /// コミットの詳細。ヘッダーの行（parent・author・committer）とメッセージ、そのコミットを指す ref。
    pub fn commit_info(&self, commit: &Oid) -> Result<serde_json::Value> {
        let text = self
            .backend()
            .commit_texts(std::slice::from_ref(commit))?
            .remove(0);
        let raw = String::from_utf8_lossy(&text.raw).into_owned();
        let (header, message) = raw.split_once("\n\n").unwrap_or((&raw, ""));
        // "Name <email> 1700000000 +0900" を名前・メール・時刻・タイムゾーンに分ける。
        let signature = |line: &str| -> (String, String, i64, String) {
            let (name, rest) = line.split_once(" <").unwrap_or((line, ""));
            let (email, rest) = rest.split_once("> ").unwrap_or((rest, ""));
            let mut parts = rest.split(' ');
            let time = parts.next().and_then(|t| t.parse().ok()).unwrap_or(0);
            let tz = parts.next().unwrap_or_default().to_owned();
            (name.to_owned(), email.to_owned(), time, tz)
        };
        let mut parents = Vec::new();
        let (mut author, mut committer) = (None, None);
        // 署名の種類（gpgsig の最初の行で分かる）と、署名付きのタグを merge したか。
        let (mut signed, mut mergetag) = (None, false);
        for line in header.lines() {
            if let Some(p) = line.strip_prefix("parent ") {
                parents.push(p.to_owned());
            } else if let Some(a) = line.strip_prefix("author ") {
                author = Some(signature(a));
            } else if let Some(c) = line.strip_prefix("committer ") {
                committer = Some(signature(c));
            } else if let Some(sig) = line
                .strip_prefix("gpgsig ")
                .or_else(|| line.strip_prefix("gpgsig-sha256 "))
            {
                signed = Some(if sig.contains("SSH SIGNATURE") {
                    "ssh"
                } else if sig.contains("SIGNED MESSAGE") {
                    "x509"
                } else {
                    "gpg"
                });
            } else if line.starts_with("mergetag ") {
                mergetag = true;
            }
        }
        let merged_tags: Vec<serde_json::Value> = merged_tags(&text.raw)
            .iter()
            .map(|raw| {
                let t = TagObject::read(raw);
                serde_json::json!({
                    "id": t.id.to_string(),
                    "name": t.name,
                    "tagger": t.tagger,
                    "taggerEmail": t.tagger_email,
                    "subject": t.subject,
                    "message": t.message,
                    "signed": t.signed,
                })
            })
            .collect();
        let (recorded_author, recorded_email, author_time, author_tz) = author.unwrap_or_default();
        let (recorded_committer, recorded_committer_email, committer_time, committer_tz) =
            committer.unwrap_or_default();
        // mailmap で置き換えた名前とメール。記録と違えば、記録のものも添える（署名は記録のものに関わる）。
        let mailmap = self.mailmap();
        let identity = |name: &str, email: &str| {
            let mapped = mailmap.as_ref().map_or_else(
                || (name.to_owned(), email.to_owned()),
                |m| m.map_str(name, email),
            );
            let recorded = (mapped.0 != name || mapped.1 != email)
                .then(|| serde_json::json!({ "name": name, "email": email }));
            (mapped.0, mapped.1, recorded)
        };
        let (author, email, recorded_author) = identity(&recorded_author, &recorded_email);
        let (committer, committer_email, recorded_committer) =
            identity(&recorded_committer, &recorded_committer_email);
        let trailers: Vec<serde_json::Value> = trailers(message)
            .into_iter()
            .map(|(key, value)| serde_json::json!({ "key": key, "value": value }))
            .collect();
        // ref はディスクから読み直さず、最後に読んだものを使う（linux では読み直すと 15〜30 ms かかる）。
        let refs: Vec<String> = self
            .refs()?
            .refs
            .entries
            .iter()
            .filter(|e| {
                e.target.as_ref() == Some(commit) && e.kind != amonhen_model::RefKind::Stash
            })
            .map(|e| e.name.clone())
            .collect();
        // shallow clone の境界のコミットは、親を名指ししていてもその親を取得していない。境界は読み込んだ History の
        // ものを使う（SSH 先では読むたびに往復するので）。まだなければ git dir から読む。
        let parents_not_fetched = !parents.is_empty()
            && match self.history() {
                Some(history) => history.shallow.contains(commit),
                None => self.backend().shallow()?.contains(commit),
            };
        Ok(serde_json::json!({
            "oid": commit.to_string(),
            "message": message,
            "author": author,
            "authorEmail": email,
            "authorTime": author_time,
            "authorTz": author_tz,
            "committer": committer,
            "committerEmail": committer_email,
            "committerTime": committer_time,
            "committerTz": committer_tz,
            "recordedAuthor": recorded_author,
            "recordedCommitter": recorded_committer,
            "parents": parents,
            "parentsNotFetched": parents_not_fetched,
            "refs": refs,
            "signed": signed,
            "mergetag": mergetag,
            "mergedTags": merged_tags,
            "trailers": trailers,
        }))
    }

    /// `status` の内容のコンフリクト（両側にファイルがある UU・AA）それぞれの、作業ツリーに残っているマーカーの
    /// 数と、git の rerere が前の解決を当てたか。作業ツリーのファイルと `.git/MERGE_RR` を読むだけ（git は呼ばない）。
    /// 片側で消えたコンフリクトや、マーカーのないテキストでないファイルなど、マーカーで表せないものは入れない。
    pub fn conflict_files(&self, status: &amonhen_model::Status) -> Result<Vec<ConflictFile>> {
        let files = self.backend().files();
        let mut tracked = None;
        let mut out = Vec::new();
        for e in &status.entries {
            if e.kind != amonhen_model::StatusKind::Unmerged
                || !matches!((e.index, e.worktree), (b'U', b'U') | (b'A', b'A'))
            {
                continue;
            }
            let data = files.read(Root::Workdir, &e.path)?;
            let markers = data.as_ref().map_or(0, |d| crate::conflict::parse(d).len());
            // バイナリなどのテキストでないファイルは、もともとマーカーが出ない（解決したわけではない）。
            if markers == 0 && data.is_some_and(|d| !crate::conflict::is_text(&d)) {
                continue;
            }
            let rerere = markers == 0
                && match &tracked {
                    Some(t) => Self::rerere_applied(t, &e.path),
                    None => Self::rerere_applied(tracked.insert(self.rerere_tracked()?), &e.path),
                };
            out.push(ConflictFile {
                path: e.path.clone(),
                markers,
                rerere,
            });
        }
        Ok(out)
    }

    /// rerere が見張っている（まだ解決を当てていない）パス。rerere が使われていなければ `None`。
    fn rerere_tracked(&self) -> Result<Option<std::collections::HashSet<String>>> {
        let data = self.backend().files().read(Root::GitDir, "MERGE_RR")?;
        Ok(data.map(|d| crate::conflict::rerere_paths(&d)))
    }

    /// rerere が `path` に前の解決を当てた（rerere が使われていて、見張りのパスから消えている）か。
    fn rerere_applied(tracked: &Option<std::collections::HashSet<String>>, path: &str) -> bool {
        tracked.as_ref().is_some_and(|t| !t.contains(path))
    }

    /// コミットの詳細のうち時間のかかる分。署名の検証・notes・行数は git に頼み、子コミット・含むブランチと
    /// タグ・前後のタグは History のグラフから求める（History をまだ読んでいなければ空）。
    pub fn commit_extras(&self, commit: &Oid) -> Result<serde_json::Value> {
        const MAX_MERGED: usize = 50;
        let generation = self.signature_generation();
        let mut extras = self.backend().commit_extras(commit)?;
        // 手元で検証するなら、署名の結果だけは手元の鍵で求め直す（notes と行数はリポジトリ側のまま）。
        let generation = match self.local_signature_details(commit) {
            Some(result) => {
                let (g, signature) = result?;
                extras.signature = signature;
                g
            }
            None => generation,
        };
        if let Some(s) = &extras.signature {
            self.remember_signatures(generation, [(*commit, s.clone())]);
        }
        let mut children = Vec::new();
        let (mut branches, mut tags) = (Vec::new(), Vec::new());
        let (mut local_branches, mut remote_branches) = (Vec::new(), Vec::new());
        let (mut first_tag, mut previous_tag) = (None, None);
        let (mut merged_rows, mut head_distance) = (Vec::new(), None);
        let mut head_rev: Option<String> = None;
        let mut summaries: Vec<Oid> = Vec::new();
        if let Some(history) = self.history()
            && let Some(row) = history.row_of(commit)
        {
            let graph = &history.graph;
            let (contains, child_rows) = crate::lineage::descendants(graph, row);
            children = child_rows
                .iter()
                .map(|&r| graph.ids()[r as usize].to_string())
                .collect();
            let refs = self.refs()?;
            let row_of = |e: &amonhen_model::RefEntry| e.target.and_then(|t| history.row_of(&t));
            let in_set = |r: u32| contains[r as usize / 64] & (1 << (r % 64)) != 0;
            // 含むタグは、古い（行が下の）ものから。いちばん古いものが「最初に含んだリリース」。
            let mut tag_rows = Vec::new();
            for e in &refs.refs.entries {
                let Some(r) = row_of(e) else { continue };
                match e.kind {
                    amonhen_model::RefKind::Local if in_set(r) => {
                        branches.push(e.name.clone());
                        local_branches.push(e.name.clone());
                    }
                    amonhen_model::RefKind::Remote if in_set(r) => {
                        branches.push(e.name.clone());
                        remote_branches.push(e.name.clone());
                    }
                    amonhen_model::RefKind::Tag if in_set(r) => tag_rows.push((r, e.name.clone())),
                    _ => {}
                }
            }
            tag_rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            first_tag = tag_rows.first().map(|(_, n)| n.clone());
            tags = tag_rows.into_iter().map(|(_, n)| n).collect();
            // このコミットより前（祖先）で、いちばん近いタグ。
            let mut tagged: std::collections::HashMap<u32, &str> = std::collections::HashMap::new();
            for e in &refs.refs.entries {
                if e.kind == amonhen_model::RefKind::Tag
                    && let Some(r) = row_of(e)
                {
                    tagged.entry(r).or_insert(&e.name);
                }
            }
            previous_tag = crate::lineage::nearest_ancestor(graph, row, |r| {
                r != row && tagged.contains_key(&r)
            })
            .map(|r| tagged[&r].to_owned());
            // HEAD が含むなら、HEAD から何コミット前か（HEAD の祖先のうち、このコミットを含むものの数 - 1）。
            if let Some(head) = refs.refs.head.and_then(|h| history.row_of(&h))
                && in_set(head)
            {
                let ancestors = crate::lineage::ancestors(graph, head);
                let both: u32 = ancestors
                    .iter()
                    .zip(&contains)
                    .map(|(a, c)| (a & c).count_ones())
                    .sum();
                head_distance = Some(both.saturating_sub(1));
                head_rev =
                    crate::lineage::rev_path(graph, head, row).map(|path| format!("HEAD{path}"));
            }
            merged_rows = crate::lineage::merged_commits(graph, row);
            summaries = merged_rows
                .iter()
                .take(MAX_MERGED)
                .map(|&r| graph.ids()[r as usize])
                .collect();
        }
        // メッセージが参照するコミット（cherry-pick・revert の元、Fixes: など）。件名を添える。
        let references = self.message_references(commit)?;
        summaries.extend(references.iter().map(|(_, id)| *id));
        let texts = if summaries.is_empty() {
            Vec::new()
        } else {
            self.backend().commit_summaries(&summaries)?
        };
        let mailmap = self.mailmap();
        let summary = |i: usize| {
            let t = &texts[i];
            (
                String::from_utf8_lossy(t.subject()).into_owned(),
                String::from_utf8_lossy(&t.author(mailmap.as_deref()).name).into_owned(),
            )
        };
        let shown_merged = summaries.len() - references.len();
        let merged: Vec<serde_json::Value> = (0..shown_merged)
            .map(|i| {
                let (subject, author) = summary(i);
                serde_json::json!({ "oid": summaries[i].to_string(), "subject": subject, "author": author })
            })
            .collect();
        let references: serde_json::Map<String, serde_json::Value> = references
            .iter()
            .enumerate()
            .map(|(i, (token, id))| {
                let (subject, _) = summary(shown_merged + i);
                (
                    token.clone(),
                    serde_json::json!({ "oid": id.to_string(), "subject": subject }),
                )
            })
            .collect();
        // このコミットを指すタグ（注釈付きならタガーとメッセージ）と、merge したタグ。署名のあるものを検証する。
        let pointing: Vec<String> = self
            .refs()?
            .refs
            .entries
            .iter()
            .filter(|e| e.kind == amonhen_model::RefKind::Tag && e.target.as_ref() == Some(commit))
            .map(|e| e.name.clone())
            .collect();
        let pointing: Vec<&str> = pointing.iter().map(String::as_str).collect();
        let pointing = self.annotated_tags(self.backend().operations()?, &pointing)?;
        let text = self
            .backend()
            .commit_texts(std::slice::from_ref(commit))?
            .remove(0);
        let merged_tags: Vec<TagObject> = merged_tags(&text.raw)
            .iter()
            .map(|r| TagObject::read(r))
            .collect();
        let tag_signatures = self.verify_signed_tags(
            pointing
                .iter()
                .filter_map(|(_, t)| t.as_ref())
                .chain(&merged_tags),
        );
        let tag_objects: Vec<serde_json::Value> = pointing
            .iter()
            .map(|(name, tag)| match tag {
                None => serde_json::json!({ "name": name, "id": null }),
                Some(t) => serde_json::json!({
                    "name": name,
                    "id": t.id.to_string(),
                    "tagger": t.tagger,
                    "taggerEmail": t.tagger_email,
                    "time": t.time,
                    "tz": t.tz,
                    "subject": t.subject,
                    "message": t.message,
                    "signed": t.signed,
                    "signature": tag_signatures.get(&t.id).map(signature_json),
                }),
            })
            .collect();
        let merged_tag_signatures: Vec<serde_json::Value> = merged_tags
            .iter()
            .map(|t| {
                tag_signatures
                    .get(&t.id)
                    .map_or(serde_json::Value::Null, signature_json)
            })
            .collect();
        let signature = extras.signature.as_ref().map(signature_json);
        let stats: Vec<serde_json::Value> = extras
            .stats
            .iter()
            .map(|s| serde_json::json!({ "path": s.path, "added": s.added, "removed": s.removed }))
            .collect();
        // タグは多いことがある（linux の古いコミットは 900 以上）ので、数と古い順の先頭だけを返す。
        const MAX_TAGS: usize = 50;
        Ok(serde_json::json!({
            "signature": signature,
            "notes": extras.notes,
            "stats": stats,
            "children": children,
            "branches": branches,
            "tagCount": tags.len(),
            "tags": tags.into_iter().take(MAX_TAGS).collect::<Vec<_>>(),
            "firstTag": first_tag,
            "previousTag": previous_tag,
            "originUrl": self.origin_url(),
            "localBranches": local_branches,
            "remoteBranches": remote_branches,
            "headDistance": head_distance,
            "headRev": head_rev,
            "mergedCount": merged_rows.len(),
            "merged": merged,
            "references": references,
            "tagObjects": tag_objects,
            "mergedTagSignatures": merged_tag_signatures,
        }))
    }

    /// タグの名前から、注釈付きのタグのオブジェクトを読む（`names` と同じ順。軽量タグやないタグは `None`）。
    fn annotated_tags<'a>(
        &self,
        ops: &dyn amonhen_model::Operations,
        names: &[&'a str],
    ) -> Result<Vec<(&'a str, Option<TagObject>)>> {
        let objects = tag_objects(ops, names)?;
        let ids: Vec<Oid> = objects.iter().filter_map(|(_, id)| *id).collect();
        let mut raws = if ids.is_empty() {
            Vec::new()
        } else {
            self.backend().tag_objects(&ids)?
        }
        .into_iter();
        Ok(objects
            .into_iter()
            .map(|(name, id)| {
                let raw = id.and_then(|_| raws.next().flatten());
                (name, raw.map(|raw| TagObject::read(&raw)))
            })
            .collect())
    }

    /// 署名のあるタグを検証し、タグのオブジェクトの ID ごとの結果を返す。検証できなければ（git が動かないなど）
    /// 空のまま返す（タグの情報は出せるので、詳細全体を失敗にしない）。
    fn verify_signed_tags<'a>(
        &self,
        tags: impl Iterator<Item = &'a TagObject>,
    ) -> std::collections::HashMap<Oid, amonhen_model::SignatureCheck> {
        let signed: Vec<(Oid, Vec<u8>)> = tags
            .filter(|t| t.signed.is_some())
            .map(|t| (t.id, t.raw.clone()))
            .collect();
        if signed.is_empty() {
            return Default::default();
        }
        match self.verify_tags(&signed) {
            Ok(checks) => signed.iter().map(|(id, _)| *id).zip(checks).collect(),
            Err(_) => Default::default(),
        }
    }

    /// メッセージに出てくるコミット ID（7〜40 桁の 16 進）のうち、引けたもの（書かれていた文字列と ID）。
    /// 多すぎるメッセージは先頭の 10 個まで。
    fn message_references(&self, commit: &Oid) -> Result<Vec<(String, Oid)>> {
        let message = self.commit_message(commit)?;
        let mut tokens: Vec<&str> = Vec::new();
        for word in message.split(|c: char| !c.is_ascii_alphanumeric()) {
            let hex = word.len() >= 7
                && word.len() <= 40
                && word.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                // 数字だけの語（日付・番号）は ID とみなさない。
                && word.bytes().any(|b| b.is_ascii_alphabetic());
            if hex && !tokens.contains(&word) && word != commit.to_string() {
                tokens.push(word);
            }
            if tokens.len() == 10 {
                break;
            }
        }
        let history = self.history();
        let ops = self.backend().operations()?;
        let mut out = Vec::new();
        for token in tokens {
            let full = Oid::from_hex(token)
                .filter(|id| history.as_ref().is_some_and(|h| h.row_of(id).is_some()));
            let id = match full {
                Some(id) => Some(id),
                None => ops.resolve(token).ok(),
            };
            if let Some(id) = id {
                out.push((token.to_owned(), id));
            }
        }
        Ok(out)
    }

    /// `remote.origin.url`（メッセージの `#123` をリンクにするため）。読めなければ `None`。
    fn origin_url(&self) -> Option<String> {
        let config = self.backend().files().read(Root::GitDir, "config").ok()??;
        origin_url(&String::from_utf8_lossy(&config))
    }

    /// HEAD から first-parent で辿り、最初の merge の手前まで（新しい順）。
    /// HEAD から first-parent で辿ったコミット（新しい順、merge を越えて。merge なら true）。
    fn first_parent_chain(&self) -> Result<Vec<(ChainCommit, bool)>> {
        let (history, mut row) = self.history_at_head()?;
        let graph = &history.graph;
        let mut chain = Vec::new();
        while chain.len() < CHAIN_LIMIT {
            let parents = graph.parents(row);
            let parent = parents.first().map(|&p| graph.ids()[p as usize]);
            chain.push((
                ChainCommit {
                    oid: graph.ids()[row],
                    parent,
                },
                parents.len() > 1,
            ));
            match parents.first() {
                Some(&p) => row = p as usize,
                None => break,
            }
        }
        Ok(chain)
    }

    /// 覚えている History（なければ読む）と、その中の HEAD の行。HEAD が動いた直後で覚えている History が古ければ、
    /// 読み直す。
    fn history_at_head(&self) -> Result<(std::sync::Arc<crate::HistorySnapshot>, usize)> {
        let head = self
            .backend()
            .head()?
            .ok_or_else(|| tr!("No commits yet"))?;
        if let Some(history) = self.history()
            && let Some(row) = history.row_of(&head)
        {
            return Ok((history, row as usize));
        }
        let history = self.load_history()?;
        let row = history
            .row_of(&head)
            .ok_or_else(|| tr!("HEAD isn’t in the History (reload it)"))?;
        Ok((history, row as usize))
    }

    fn editable_chain(&self) -> Result<Vec<ChainCommit>> {
        let (history, mut row) = self.history_at_head()?;
        let graph = &history.graph;
        let mut chain = Vec::new();
        while chain.len() < CHAIN_LIMIT {
            let parents = graph.parents(row);
            if parents.len() > 1 {
                break;
            }
            let parent = parents.first().map(|&p| graph.ids()[p as usize]);
            chain.push(ChainCommit {
                oid: graph.ids()[row],
                parent,
            });
            match parents.first() {
                Some(&p) => row = p as usize,
                None => break,
            }
        }
        Ok(chain)
    }

    /// ローカルブランチ `branch`（短い名前）をチェックアウトしている worktree のパス。ブランチだけを動かすと、その
    /// worktree の作業ツリーとずれる。worktree を読めなければ None。
    fn worktree_using(&self, ops: &dyn amonhen_model::Operations, branch: &str) -> Option<String> {
        let list = ops.worktree_list().ok()?;
        let paths = self.paths();
        crate::worktree::parse(&list, paths.workdir.as_deref())
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(branch))
            .map(|w| w.path)
    }

    /// `FastForward` で動かす ref の完全な名前と、今の先頭・動かす先（上流の先頭）。動かせなければその理由のエラー。
    fn fast_forward_target(
        &self,
        ops: &dyn amonhen_model::Operations,
        branch: &str,
    ) -> Result<(String, Oid, Oid)> {
        let refs = self.load_refs()?;
        let entries = &refs.refs.entries;
        let local = entries
            .iter()
            .find(|e| e.kind == RefKind::Local && e.name == branch)
            .ok_or_else(|| tr!("{name} not found", name = branch))?;
        let upstream = local
            .detail
            .as_deref()
            .ok_or_else(|| tr!("{branch} has no upstream", branch = branch))?;
        if refs.refs.head_branch.as_deref() == Some(branch) {
            return Err(tr!(
                "{branch} is checked out here; Pull it instead",
                branch = branch
            )
            .into());
        }
        if let Some(path) = self.worktree_using(ops, branch) {
            return Err(tr!(
                "{branch} is checked out in the worktree {path}; update it there",
                branch = branch,
                path = path
            )
            .into());
        }
        // 上流はふつうリモートブランチ。`branch.<name>.remote` が `.` なら手元のブランチ。
        let target = entries
            .iter()
            .find(|e| e.kind == RefKind::Remote && e.name == upstream)
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.kind == RefKind::Local && e.name == upstream)
            })
            .and_then(|e| e.target)
            .ok_or_else(|| tr!("{upstream} not found (Fetch first)", upstream = upstream))?;
        let tip = local
            .target
            .ok_or_else(|| tr!("{name} not found", name = branch))?;
        if tip == target {
            return Err(tr!(
                "{branch} is already the same as {upstream}",
                branch = branch,
                upstream = upstream
            )
            .into());
        }
        if !ops.is_ancestor(&tip.to_string(), &target.to_string())? {
            return Err(tr!(
                "{branch} and {upstream} have diverged; check out {branch} to merge or rebase",
                branch = branch,
                upstream = upstream
            )
            .into());
        }
        Ok((format!("refs/heads/{branch}"), tip, target))
    }

    /// 先頭が `ids` のどれかにあるローカルブランチ（先頭と完全な名前）。rebase で一緒に動きうる。チェックアウト中の
    /// ブランチと、ほかの worktree でチェックアウト中のもの（git が動かさない）は除く。
    fn stacked_branches(
        &self,
        ops: &dyn amonhen_model::Operations,
        ids: &std::collections::HashSet<Oid>,
    ) -> Result<Vec<(Oid, String)>> {
        let refs = self.load_refs()?;
        let mut busy: Vec<String> = refs.refs.head_branch.iter().cloned().collect();
        if let Ok(list) = ops.worktree_list() {
            let paths = self.paths();
            busy.extend(
                crate::worktree::parse(&list, paths.workdir.as_deref())
                    .into_iter()
                    .filter_map(|w| w.branch),
            );
        }
        Ok(refs
            .refs
            .entries
            .iter()
            .filter(|e| e.kind == RefKind::Local && !busy.contains(&e.name))
            .filter_map(|e| {
                let tip = e.target.filter(|t| ids.contains(t))?;
                Some((tip, format!("refs/heads/{}", e.name)))
            })
            .collect())
    }

    /// `names` のうち、`before` から動いたブランチの短い名前。
    fn moved_branches(
        ops: &dyn amonhen_model::Operations,
        before: &[(String, Option<Oid>)],
    ) -> Result<Vec<String>> {
        let names: Vec<String> = before.iter().map(|(n, _)| n.clone()).collect();
        Ok(ops
            .resolve_refs(&names)?
            .into_iter()
            .zip(before)
            .filter(|((_, now), (_, was))| now != was)
            .map(|((name, _), _)| name.trim_start_matches("refs/heads/").to_owned())
            .collect())
    }

    /// rebase で git のエディタに渡すメッセージを `.git/amonhen` に書く。1 つならファイル、いくつもあれば git が
    /// エディタを開く順に番号を付けたディレクトリ（バックエンドが 1 つずつ写す）。なければ None。
    fn write_rebase_messages(&self, messages: &[String]) -> Result<Option<std::path::PathBuf>> {
        let dir = self.backend().paths().git_dir.join("amonhen");
        let files = self.backend().files();
        // エディタで保存したファイルと同じく、改行で終える（commit-msg のフックが行を足しても、最後の行に
        // つながらない）。
        let ended = |m: &String| {
            if m.ends_with('\n') {
                m.clone()
            } else {
                format!("{m}\n")
            }
        };
        let message = match messages {
            [] => None,
            [one] => {
                files.write(
                    Root::GitDir,
                    "amonhen/rebase-message",
                    ended(one).as_bytes(),
                )?;
                Some(dir.join("rebase-message"))
            }
            // いくつもあれば、git がエディタを開く順に番号を付けて置く（バックエンドが 1 つずつ写す）。
            many => {
                for (i, m) in many.iter().enumerate() {
                    files.write(
                        Root::GitDir,
                        &format!("amonhen/rebase-messages/{}", i + 1),
                        ended(m).as_bytes(),
                    )?;
                }
                files.write(Root::GitDir, "amonhen/rebase-messages/next", b"1\n")?;
                Some(dir.join("rebase-messages"))
            }
        };
        Ok(message)
    }

    /// 絵に描くコミットの件名と日時、それぞれに付くブランチの札（`ids` のコミットに付くもの）。
    fn drawing_texts(&self, ids: &[Oid]) -> Result<(DrawingTexts, Vec<serde_json::Value>)> {
        let texts = self.commit_texts(ids)?;
        let map = ids
            .iter()
            .zip(&texts)
            .map(|(id, t)| {
                (
                    *id,
                    (
                        String::from_utf8_lossy(t.subject()).into_owned(),
                        t.author_time,
                    ),
                )
            })
            .collect();
        let refs = self.load_refs()?;
        let labels = refs
            .refs
            .entries
            .iter()
            .filter(|e| matches!(e.kind, RefKind::Local | RefKind::Remote))
            .filter_map(|e| {
                let t = e.target.filter(|t| ids.contains(t))?;
                Some(serde_json::json!({
                    "oid": t.to_string(), "name": e.name,
                    "kind": if e.kind == RefKind::Local { "local" } else { "remote" },
                }))
            })
            .collect();
        Ok((map, labels))
    }

    /// `MergePreview` の中身。
    fn merge_preview(
        &self,
        ops: &dyn amonhen_model::Operations,
        ours: &str,
        theirs: &str,
        ff_setting: &str,
    ) -> Result<serde_json::Value> {
        let (ours_id, theirs_id) = (ops.resolve(ours)?, ops.resolve(theirs)?);
        let history = self.load_history()?;
        let graph = &history.graph;
        let row = |id: &Oid, rev: &str| -> Result<usize> {
            history
                .row_of(id)
                .map(|r| r as usize)
                .ok_or_else(|| tr!("{rev} isn’t in the History (reload it)", rev = rev).into())
        };
        let (ours_row, theirs_row) = (row(&ours_id, ours)?, row(&theirs_id, theirs)?);
        let reach = |start: usize| {
            let mut mark = vec![false; graph.len()];
            mark[start] = true;
            for r in start..graph.len() {
                if mark[r] {
                    for &p in graph.parents(r) {
                        mark[p as usize] = true;
                    }
                }
            }
            mark
        };
        let (ours_reach, theirs_reach) = (reach(ours_row), reach(theirs_row));
        if ours_reach[theirs_row] {
            return Ok(serde_json::json!({ "upToDate": true, "drawing": null }));
        }
        let ff_off = ops
            .config_list()?
            .iter()
            .rev()
            .find(|e| e.key.eq_ignore_ascii_case(ff_setting))
            .is_some_and(|e| e.value.eq_ignore_ascii_case("false"));
        let current = ops
            .head_state()?
            .branch
            .map(|b| b.strip_prefix("refs/heads/").unwrap_or(&b).to_owned());
        if theirs_reach[ours_row] && !ff_off {
            let branch = (ours == "HEAD").then_some(current).flatten();
            return Ok(serde_json::json!({
                "fastForward": true,
                "move": self.move_preview(ops, ours, theirs, branch.as_deref())?,
            }));
        }
        let ids_of = |mark: &[bool], other: &[bool]| {
            (0..graph.len())
                .filter(|&r| mark[r] && !other[r])
                .map(|r| graph.ids()[r])
                .collect::<Vec<_>>()
        };
        let (shown_ours, ours_more, ours_at) = elide(&ids_of(&ours_reach, &theirs_reach));
        let (shown_theirs, theirs_more, theirs_at) = elide(&ids_of(&theirs_reach, &ours_reach));
        let base = crate::repository::merge_base(graph, ours_row as u32, theirs_row as u32)
            .map(|r| graph.ids()[r as usize]);
        let mut ids = shown_ours.clone();
        ids.extend(&shown_theirs);
        ids.extend(base);
        let (texts, labels) = self.drawing_texts(&ids)?;
        let commit = |id: &Oid| serde_json::json!({ "oid": id.to_string(), "subject": texts.get(id).map(|t| t.0.clone()) });
        // git の merge コミットの件名（`Merge branch 'x'`。main・master に取り込むときは「into」を付けない）。
        let refs = self.load_refs()?;
        let kind = refs
            .refs
            .entries
            .iter()
            .find(|e| e.name == theirs)
            .map(|e| e.kind);
        let pulled = (ff_setting == "pull.ff" && ours == "HEAD")
            .then(|| pull_merge_subject(ops, current.as_deref()?, theirs).transpose())
            .flatten()
            .transpose()?;
        let mut subject = match (pulled, kind) {
            (Some(pulled), _) => pulled,
            (None, Some(RefKind::Local)) => format!("Merge branch '{theirs}'"),
            (None, Some(RefKind::Remote)) => format!("Merge remote-tracking branch '{theirs}'"),
            _ => format!("Merge commit '{theirs}'"),
        };
        if let Some(branch) = current.filter(|b| ours == "HEAD" && b != "main" && b != "master") {
            subject.push_str(&format!(" into {branch}"));
        }
        Ok(serde_json::json!({
            "fastForward": false,
            "drawing": {
                "ours": shown_ours.iter().map(commit).collect::<Vec<_>>(),
                "oursMore": ours_more,
                "oursMoreAt": ours_at,
                "theirs": shown_theirs.iter().map(commit).collect::<Vec<_>>(),
                "theirsMore": theirs_more,
                "theirsMoreAt": theirs_at,
                "base": base.map(|b| {
                    let mut c = commit(&b);
                    c["time"] = serde_json::json!(texts.get(&b).map_or(0, |t| t.1));
                    c
                }),
                "labels": labels,
                "subject": subject,
            },
        }))
    }

    /// `OctopusPreview` の中身。
    fn octopus_preview(
        &self,
        ops: &dyn amonhen_model::Operations,
        theirs: &[String],
    ) -> Result<serde_json::Value> {
        let head = ops.head_state()?.oid.ok_or_else(|| tr!("No commits yet"))?;
        let history = self.load_history()?;
        let graph = &history.graph;
        let row = |id: &Oid, rev: &str| -> Result<usize> {
            history
                .row_of(id)
                .map(|r| r as usize)
                .ok_or_else(|| tr!("{rev} isn’t in the History (reload it)", rev = rev).into())
        };
        let ours_row = row(&head, "HEAD")?;
        let mut side_rows = Vec::new();
        for rev in theirs {
            side_rows.push(row(&ops.resolve(rev)?, rev)?);
        }
        let reach = |start: usize| {
            let mut mark = vec![false; graph.len()];
            mark[start] = true;
            for r in start..graph.len() {
                if mark[r] {
                    for &p in graph.parents(r) {
                        mark[p as usize] = true;
                    }
                }
            }
            mark
        };
        let ours_reach = reach(ours_row);
        if side_rows.iter().all(|&r| ours_reach[r]) {
            return Ok(serde_json::json!({ "upToDate": true, "drawing": null }));
        }
        let side_reach: Vec<Vec<bool>> = side_rows.iter().map(|&r| reach(r)).collect();
        // すべてが分かれたところ（git merge-base --octopus と同じく、順に重ねる）。
        let base = side_rows.iter().try_fold(ours_row as u32, |b, &r| {
            crate::repository::merge_base(graph, b, r as u32)
        });
        let below_base = base.map(|b| reach(b as usize));
        let since_base = |r: usize| below_base.as_ref().is_none_or(|m| !m[r]);
        // 今のブランチのコミット: 取り込むどれにもないもの。取り込む側: 今のブランチにも、前に並ぶものにもないもの。
        let ours: Vec<Oid> = (0..graph.len())
            .filter(|&r| ours_reach[r] && since_base(r) && !side_reach.iter().any(|m| m[r]))
            .map(|r| graph.ids()[r])
            .collect();
        let (shown_ours, ours_more, ours_at) = elide(&ours);
        let mut sides = Vec::new();
        let mut ids = shown_ours.clone();
        for (i, &tip) in side_rows.iter().enumerate() {
            let own = (0..graph.len())
                .filter(|&r| {
                    side_reach[i][r] && !ours_reach[r] && !side_reach[..i].iter().any(|m| m[r])
                })
                .count();
            let tip = (!ours_reach[tip]).then(|| graph.ids()[tip]);
            ids.extend(tip);
            sides.push((tip, own.saturating_sub(1)));
        }
        let base = base.map(|b| graph.ids()[b as usize]);
        ids.extend(base);
        let (texts, labels) = self.drawing_texts(&ids)?;
        let commit = |id: &Oid| serde_json::json!({ "oid": id.to_string(), "subject": texts.get(id).map(|t| t.0.clone()) });
        Ok(serde_json::json!({
            "drawing": {
                "ours": shown_ours.iter().map(commit).collect::<Vec<_>>(),
                "oursMore": ours_more,
                "oursMoreAt": ours_at,
                "sides": theirs.iter().zip(&sides).map(|(rev, (tip, more))| serde_json::json!({
                    "rev": rev, "tip": tip.as_ref().map(commit), "more": more,
                })).collect::<Vec<_>>(),
                "base": base.map(|b| {
                    let mut c = commit(&b);
                    c["time"] = serde_json::json!(texts.get(&b).map_or(0, |t| t.1));
                    c
                }),
                "labels": labels,
            },
        }))
    }

    /// `PickPreview` の中身。
    fn pick_preview(
        &self,
        ops: &dyn amonhen_model::Operations,
        oids: &[String],
        revert: bool,
        subjects: &[String],
    ) -> Result<serde_json::Value> {
        let head = ops.head_state()?.oid.ok_or_else(|| tr!("No commits yet"))?;
        let history = self.load_history()?;
        let graph = &history.graph;
        let mut context = vec![head];
        if let Some(parent) = history
            .row_of(&head)
            .and_then(|r| graph.parents(r as usize).first().copied())
        {
            context.push(graph.ids()[parent as usize]);
        }
        let picked: Vec<Oid> = oids.iter().map(|o| ops.resolve(o)).collect::<Result<_>>()?;
        // 作るコミットは新しい順（最後に渡したものが一番上）。
        let newest_first: Vec<Oid> = picked.iter().rev().copied().collect();
        let (shown, more, at) = elide(&newest_first);
        let mut ids = context.clone();
        ids.extend(&shown);
        let (texts, labels) = self.drawing_texts(&ids)?;
        let subject = |id: &Oid| texts.get(id).map(|t| t.0.clone()).unwrap_or_default();
        let (picks, more, at) = if subjects.is_empty() {
            let picks: Vec<_> = shown
                .iter()
                .map(|id| {
                    let s = subject(id);
                    serde_json::json!({
                        "oid": id.to_string(),
                        "subject": if revert { format!("Revert \"{s}\"") } else { s },
                    })
                })
                .collect();
            (picks, more, at)
        } else {
            // まだコミットのないもの（パッチ）は、件名だけ（新しい順）。
            let newest_first: Vec<(usize, &String)> = subjects.iter().enumerate().rev().collect();
            let (shown, more, at) = elide(&newest_first);
            let picks = shown
                .iter()
                .map(|(i, s)| serde_json::json!({ "oid": format!("patch-{i}"), "subject": s }))
                .collect();
            (picks, more, at)
        };
        Ok(serde_json::json!({
            "drawing": {
                "head": context.iter().map(|id| serde_json::json!({ "oid": id.to_string(), "subject": subject(id) })).collect::<Vec<_>>(),
                "picks": picks,
                "picksMore": more,
                "picksMoreAt": at,
                "labels": labels,
            },
        }))
    }

    /// `MovePreview` の中身。
    fn move_preview(
        &self,
        ops: &dyn amonhen_model::Operations,
        from: &str,
        to: &str,
        branch: Option<&str>,
    ) -> Result<serde_json::Value> {
        let (from_id, to_id) = (ops.resolve(from)?, ops.resolve(to)?);
        let history = self.load_history()?;
        let graph = &history.graph;
        let row = |id: &Oid, rev: &str| -> Result<usize> {
            history
                .row_of(id)
                .map(|r| r as usize)
                .ok_or_else(|| tr!("{rev} isn’t in the History (reload it)", rev = rev).into())
        };
        // 行き先が History にない（Undo で戻る、どこからも辿れなくなったコミット）なら、親を辿って History に
        // 合流するところまでを、行き先だけのコミットとして先に並べる。
        let mut outside: Vec<Oid> = Vec::new();
        let mut join = to_id;
        while history.row_of(&join).is_none() {
            if outside.len() >= 100 {
                return Err(tr!("{rev} isn’t in the History (reload it)", rev = to).into());
            }
            outside.push(join);
            let texts = self.commit_texts(&[join])?;
            join = texts
                .first()
                .and_then(|t| first_parent(&t.raw))
                .ok_or_else(|| tr!("{rev} isn’t in the History (reload it)", rev = to))?;
        }
        let (from_row, to_row) = (row(&from_id, from)?, row(&join, to)?);
        // それぞれから辿れる行に印を付ける（行はトポロジカル順で、親は子より後ろ）。
        let reach = |start: usize| {
            let mut mark = vec![false; graph.len()];
            mark[start] = true;
            for r in start..graph.len() {
                if mark[r] {
                    for &p in graph.parents(r) {
                        mark[p as usize] = true;
                    }
                }
            }
            mark
        };
        let (from_reach, to_reach) = (reach(from_row), reach(to_row));
        // History の行は新しい順。
        let gained: Vec<usize> = (0..graph.len())
            .filter(|&r| to_reach[r] && !from_reach[r])
            .collect();
        let lost: Vec<usize> = (0..graph.len())
            .filter(|&r| from_reach[r] && !to_reach[r])
            .collect();
        let base = crate::repository::merge_base(graph, from_row as u32, to_row as u32)
            .map(|r| graph.ids()[r as usize]);
        let files = if lost.is_empty() {
            0
        } else {
            self.changes_between(Some(&to_id), &from_id)?.len()
        };
        let refs = self.load_refs()?;
        // 上流が、外れるコミットをいくつ持っているか（Reset の後の Push は Force Push になる）。
        let upstream = branch.and_then(|name| {
            refs.refs
                .entries
                .iter()
                .find(|e| e.kind == RefKind::Local && e.name == name && !e.gone)
                .and_then(|e| e.detail.clone())
        });
        let upstream_has = upstream
            .as_deref()
            .and_then(|upstream| ops.resolve(upstream).ok())
            .and_then(|id| history.row_of(&id))
            .map_or(0, |r| {
                let up = reach(r as usize);
                lost.iter().filter(|&&l| up[l]).count()
            });
        let mut drawing = serde_json::Value::Null;
        if from_id != to_id {
            let ids_of = |rows: &[usize]| rows.iter().map(|&r| graph.ids()[r]).collect::<Vec<_>>();
            let mut gained_ids = outside.clone();
            gained_ids.extend(ids_of(&gained));
            let (shown_gained, gained_more, gained_at) = elide(&gained_ids);
            let (shown_lost, lost_more, lost_at) = elide(&ids_of(&lost));
            let mut ids: Vec<Oid> = shown_gained.clone();
            ids.extend(&shown_lost);
            ids.extend(base);
            let texts = self.commit_texts(&ids)?;
            let subject = |id: &Oid| {
                ids.iter()
                    .position(|i| i == id)
                    .and_then(|i| texts.get(i))
                    .map(|t| String::from_utf8_lossy(t.subject()).into_owned())
                    .unwrap_or_default()
            };
            let commit =
                |id: &Oid| serde_json::json!({ "oid": id.to_string(), "subject": subject(id) });
            let labels: Vec<serde_json::Value> = refs
                .refs
                .entries
                .iter()
                .filter(|e| matches!(e.kind, RefKind::Local | RefKind::Remote))
                .filter_map(|e| {
                    let t = e.target.filter(|t| ids.contains(t))?;
                    Some(serde_json::json!({
                        "oid": t.to_string(), "name": e.name,
                        "kind": if e.kind == RefKind::Local { "local" } else { "remote" },
                    }))
                })
                .collect();
            let time = |id: &Oid| {
                ids.iter()
                    .position(|i| i == id)
                    .and_then(|i| texts.get(i))
                    .map_or(0, |t| t.author_time)
            };
            drawing = serde_json::json!({
                // どこからも辿れない（Reflog にだけある）ものには印を付ける。
                "gained": shown_gained
                    .iter()
                    .map(|id| {
                        let mut c = commit(id);
                        if outside.contains(id) {
                            c["hidden"] = serde_json::json!(true);
                        }
                        c
                    })
                    .collect::<Vec<_>>(),
                "gainedMore": gained_more,
                "gainedMoreAt": gained_at,
                "lost": shown_lost.iter().map(commit).collect::<Vec<_>>(),
                "lostMore": lost_more,
                "lostMoreAt": lost_at,
                "base": base.map(|b| {
                    let mut c = commit(&b);
                    c["time"] = serde_json::json!(time(&b));
                    c
                }),
                "labels": labels,
            });
        }
        Ok(serde_json::json!({
            "gainedCount": gained.len() + outside.len(),
            "lostCount": lost.len(),
            "files": files,
            "upstream": upstream,
            "upstreamHas": upstream_has,
            "drawing": drawing,
        }))
    }

    /// `RewritePreview` の中身。
    fn rewrite_preview(
        &self,
        ops: &dyn amonhen_model::Operations,
        edit: Option<&HistoryEdit>,
        onto: Option<&str>,
        update_refs: Option<bool>,
    ) -> Result<serde_json::Value> {
        let head = ops.head_state()?.oid.ok_or_else(|| tr!("No commits yet"))?;
        // 載せ直す土台、載せ直す順のコミット（予測に使う）、書き換わるコミット（ブランチを見る）。
        let base: Option<Oid>;
        let mut picks: Vec<Oid> = Vec::new();
        let mut rewritten: std::collections::HashSet<Oid> = Default::default();
        let mut forecast = true;
        // rebase のとき、確認のシートで前と後を描くための、載せ先で進んだコミット（新しい順）と、分かれたところ。
        let mut onto_commits: Vec<Oid> = Vec::new();
        let mut fork: Option<Oid> = None;
        // History の編集のとき、確認のシートで前と後を描くための、書き換わるコミット（新しい順）と手順。
        let mut edit_range: Option<(Vec<Oid>, &[rewrite::PlanStep])> = None;
        match (edit, onto) {
            (Some(edit), _) => {
                self.load_history()?;
                let chain = self.editable_chain()?;
                let past_merge = match edit {
                    HistoryEdit::Plan { steps } => steps.iter().any(|s| {
                        Oid::from_hex(&s.oid).is_none_or(|o| !chain.iter().any(|c| c.oid == o))
                    }),
                    _ => false,
                };
                if past_merge {
                    // merge を越えるとき: 最初の親の並びの、いちばん古い編集から HEAD まで（取り込んだ側も作り直す）。
                    // merge を作り直す見込みは調べない。
                    let line = self.first_parent_chain()?;
                    let HistoryEdit::Plan { steps } = edit else {
                        unreachable!()
                    };
                    let oldest = steps.first().and_then(|s| Oid::from_hex(&s.oid));
                    let start = line
                        .iter()
                        .position(|(c, _)| Some(c.oid) == oldest)
                        .ok_or_else(|| tr!("Nothing to edit"))?;
                    base = line[start].0.parent;
                    if let Some(b) = base {
                        rewritten.extend(
                            self.backend()
                                .commits_since(&[head], &[b])?
                                .into_iter()
                                .map(|c| c.id),
                        );
                    }
                    forecast = false;
                } else {
                    let plan = rewrite::plan_rebase(&chain, edit, &[])?;
                    base = plan.base;
                    if let HistoryEdit::Plan { steps } = edit {
                        let before = chain
                            .iter()
                            .map(|c| c.oid)
                            .take_while(|o| Some(*o) != base)
                            .collect();
                        edit_range = Some((before, steps));
                    }
                    for line in plan.todo.lines() {
                        let mut words = line.split(' ');
                        let (Some(command), Some(oid)) =
                            (words.next(), words.next().and_then(Oid::from_hex))
                        else {
                            continue;
                        };
                        rewritten.insert(oid);
                        if command != "drop" {
                            picks.push(oid);
                        }
                    }
                }
            }
            (None, Some(onto)) => {
                let b = ops.resolve(onto)?;
                base = Some(b);
                // rebase は merge を載せない。古い順にする。
                let commits = self.backend().commits_since(&[head], &[b])?;
                rewritten.extend(commits.iter().map(|c| c.id));
                let mut linear: Vec<_> = commits
                    .iter()
                    .filter(|c| c.parents.len() <= 1)
                    .map(|c| c.id)
                    .collect();
                linear.reverse();
                fork = linear
                    .first()
                    .and_then(|first| commits.iter().find(|c| c.id == *first))
                    .and_then(|c| c.parents.first().copied());
                picks = linear;
                onto_commits = self
                    .backend()
                    .commits_since(&[b], &[head])?
                    .into_iter()
                    .map(|c| c.id)
                    .collect();
            }
            (None, None) => return Err(tr!("Nothing to preview").into()),
        }
        // コンフリクトの見込み（土台がない、root からの書き換えは調べない）。
        let conflicts = match base.filter(|_| forecast && !picks.is_empty()) {
            Some(b) => {
                let hexes: Vec<String> = picks.iter().map(|o| o.to_string()).collect();
                ops.predict_replay(&b.to_string(), &hexes)?.map(|steps| {
                    hexes
                        .iter()
                        .zip(steps)
                        .filter(|(_, files)| !files.is_empty())
                        .map(|(oid, files)| serde_json::json!({ "oid": oid, "files": files }))
                        .collect::<Vec<_>>()
                })
            }
            None => None,
        };
        // 書き換えるコミットに先頭があるブランチ。動くのは git と同じ条件（rebase.updateRefs と git 2.38 以降）で、
        // ほかの worktree で checkout 中のものは動かない。
        let refs = self.load_refs()?;
        let current = refs.refs.head_branch.clone();
        let upstream_of = |name: &str| {
            refs.refs
                .entries
                .iter()
                .find(|e| e.kind == RefKind::Local && e.name == name)
                .and_then(|e| e.detail.clone().filter(|_| !e.gone))
        };
        let worktrees: Vec<(String, String)> = ops
            .worktree_list()
            .map(|list| {
                crate::worktree::parse(&list, self.paths().workdir.as_deref())
                    .into_iter()
                    .filter_map(|w| Some((w.branch?, w.path)))
                    .collect()
            })
            .unwrap_or_default();
        let git_ok = ops.git_version()?.is_none_or(|(a, b, _)| (a, b) >= (2, 38));
        let configured = ops
            .config_list()?
            .iter()
            .rev()
            .find(|e| e.key.eq_ignore_ascii_case("rebase.updateRefs"))
            .is_some_and(|e| e.value.eq_ignore_ascii_case("true"));
        let setting = update_refs.unwrap_or(configured);
        let mut moving = Vec::new();
        let mut staying = Vec::new();
        for entry in refs
            .refs
            .entries
            .iter()
            .filter(|e| e.kind == RefKind::Local)
        {
            if Some(&entry.name) == current.as_ref()
                || !entry.target.is_some_and(|t| rewritten.contains(&t))
            {
                continue;
            }
            let upstream = upstream_of(&entry.name);
            let elsewhere = worktrees
                .iter()
                .find(|(branch, _)| *branch == entry.name)
                .map(|(_, path)| path.clone());
            let reason = if elsewhere.is_some() {
                Some("checkedOut")
            } else if !git_ok {
                Some("git")
            } else if update_refs == Some(false) {
                Some("off")
            } else if !setting {
                Some("setting")
            } else {
                None
            };
            match reason {
                None => moving.push(serde_json::json!({ "name": entry.name, "upstream": upstream })),
                Some(reason) => staying.push(serde_json::json!({
                    "name": entry.name, "upstream": upstream, "reason": reason, "worktree": elsewhere,
                })),
            }
        }
        // 前と後を描く材料（rebase のとき）: 載せ直すコミット（古い順）、載せ先で進んだコミット（新しい順、先頭の
        // いくつか）、分かれたところ、それぞれに付いているブランチ。
        let mut drawing = serde_json::Value::Null;
        if onto.is_some() && !picks.is_empty() {
            let (shown_onto, onto_more, onto_at) = elide(&onto_commits);
            // 載せ直すコミットも、長ければ真ん中を省く（新しい順で省いてから、古い順に戻す）。
            let newest_first: Vec<Oid> = picks.iter().rev().copied().collect();
            let (mut shown_picks, picks_more, picks_at) = elide(&newest_first);
            shown_picks.reverse();
            let mut ids: Vec<Oid> = shown_picks.clone();
            ids.extend(&shown_onto);
            ids.extend(fork);
            let texts = self.commit_texts(&ids)?;
            let subject = |id: &Oid| {
                ids.iter()
                    .position(|i| i == id)
                    .and_then(|i| texts.get(i))
                    .map(|t| String::from_utf8_lossy(t.subject()).into_owned())
                    .unwrap_or_default()
            };
            let commit =
                |id: &Oid| serde_json::json!({ "oid": id.to_string(), "subject": subject(id) });
            let labels: Vec<serde_json::Value> = refs
                .refs
                .entries
                .iter()
                .filter(|e| matches!(e.kind, RefKind::Local | RefKind::Remote))
                .filter_map(|e| {
                    let t = e.target.filter(|t| ids.contains(t))?;
                    Some(serde_json::json!({
                        "oid": t.to_string(), "name": e.name,
                        "kind": if e.kind == RefKind::Local { "local" } else { "remote" },
                    }))
                })
                .collect();
            drawing = serde_json::json!({
                "picks": shown_picks.iter().map(commit).collect::<Vec<_>>(),
                "picksMore": picks_more,
                // 新しい順に並べたときの、省いたところ。
                "picksMoreAt": picks_at,
                "onto": shown_onto.iter().map(commit).collect::<Vec<_>>(),
                "ontoMore": onto_more,
                "ontoMoreAt": onto_at,
                "base": fork.map(|b| {
                    let mut c = commit(&b);
                    c["time"] = serde_json::json!(ids
                        .iter()
                        .position(|i| *i == b)
                        .and_then(|i| texts.get(i))
                        .map_or(0, |t| t.author_time));
                    c
                }),
                "labels": labels,
            });
        }
        let edit_drawing = match edit_range {
            Some((before, steps)) => self.edit_drawing(&before, steps, base)?,
            None => serde_json::Value::Null,
        };
        Ok(serde_json::json!({
            "moving": moving,
            "staying": staying,
            "current": current,
            "currentUpstream": current.as_deref().and_then(upstream_of),
            "conflicts": conflicts,
            "updateRefs": { "configured": configured, "available": git_ok },
            "drawing": drawing,
            "editDrawing": edit_drawing,
        }))
    }

    /// History の編集の前と後（`RewritePreview` の `editDrawing`）。`before` は書き換わるコミット（新しい順）、
    /// `steps` は手順（古い順）。`before`（`oid`・`subject`・`action`）と、`after`（できるコミット: 元のコミットの
    /// `oid`、件名（reword・squash では新しいメッセージの 1 行目）、`combined`（まとめたコミットの数）、`action`）、
    /// どちらも新しい順で長ければ真ん中を省き（`beforeMore`・`beforeMoreAt`・`afterMore`・`afterMoreAt`）、
    /// `base`（書き換えない、いちばん新しいコミット。日時つき）と `labels`。
    fn edit_drawing(
        &self,
        before: &[Oid],
        steps: &[rewrite::PlanStep],
        base: Option<Oid>,
    ) -> Result<serde_json::Value> {
        let step_of = |o: &Oid| steps.iter().find(|s| Oid::from_hex(&s.oid) == Some(*o));
        let first_line = |m: &str| m.lines().next().unwrap_or_default().to_owned();
        // できるコミット（古い順）: drop は消え、squash・fixup は 1 つ前にまとまる。
        let mut groups: Vec<(Oid, Option<String>, usize, String)> = Vec::new();
        for step in steps {
            let Some(oid) = Oid::from_hex(&step.oid).filter(|o| before.contains(o)) else {
                continue;
            };
            match step.action.as_str() {
                "drop" => {}
                "squash" | "fixup" if !groups.is_empty() => {
                    let last = groups.last_mut().unwrap();
                    last.2 += 1;
                    if step.action == "squash"
                        && let Some(message) = &step.message
                    {
                        last.1 = Some(first_line(message));
                    }
                }
                action => groups.push((
                    oid,
                    (action == "reword")
                        .then(|| step.message.as_deref().map(first_line))
                        .flatten(),
                    0,
                    action.to_owned(),
                )),
            }
        }
        groups.reverse();
        let (shown_before, before_more, before_at) = elide(before);
        let (shown_after, after_more, after_at) = elide(&groups);
        let mut ids = shown_before.clone();
        ids.extend(shown_after.iter().map(|g| g.0));
        ids.extend(base);
        let (texts, labels) = self.drawing_texts(&ids)?;
        let subject = |id: &Oid| texts.get(id).map(|t| t.0.clone()).unwrap_or_default();
        Ok(serde_json::json!({
            "before": shown_before.iter().map(|o| serde_json::json!({
                "oid": o.to_string(), "subject": subject(o),
                "action": step_of(o).map_or("pick", |s| s.action.as_str()),
            })).collect::<Vec<_>>(),
            "beforeMore": before_more,
            "beforeMoreAt": before_at,
            "after": shown_after.iter().map(|(o, new, combined, action)| serde_json::json!({
                "oid": o.to_string(), "subject": new.clone().unwrap_or_else(|| subject(o)),
                "combined": combined, "action": action,
            })).collect::<Vec<_>>(),
            "afterMore": after_more,
            "afterMoreAt": after_at,
            "base": base.map(|b| serde_json::json!({
                "oid": b.to_string(), "subject": subject(&b), "time": texts.get(&b).map_or(0, |t| t.1),
            })),
            "labels": labels,
        }))
    }

    /// merge を越える編集（History の編集モード）。git の `--rebase-merges` で merge を作り直し、git が作った todo の
    /// うち、変えるコミットの命令だけを替える。並べ替えはしない（merge をまたぐと、何をどこへ動かすかが一意でない）。
    fn rewrite_through_merges(
        &self,
        ops: &dyn amonhen_model::Operations,
        label: &str,
        steps: &[rewrite::PlanStep],
        update_refs: Option<bool>,
    ) -> Result<Outcome> {
        let not_editable = || {
            tr!(
                "This commit can’t be edited (it isn’t reachable from HEAD on the first-parent line)"
            )
        };
        // 古い順。
        let original: Vec<(ChainCommit, bool)> =
            self.first_parent_chain()?.into_iter().rev().collect();
        let first = Oid::from_hex(&steps.first().ok_or_else(|| tr!("Nothing to edit"))?.oid)
            .ok_or_else(not_editable)?;
        let start = original
            .iter()
            .position(|(c, _)| c.oid == first)
            .ok_or_else(not_editable)?;
        let range = &original[start..];
        if range.len() != steps.len()
            || range
                .iter()
                .zip(steps)
                .any(|((c, _), s)| Oid::from_hex(&s.oid) != Some(c.oid))
        {
            return Err(tr!("Commits can’t be reordered across a merge").into());
        }
        for (i, ((_, merge), step)) in range.iter().zip(steps).enumerate() {
            if *merge && step.action != "pick" {
                return Err(tr!(
                    "A merge is recreated as it is; only the commits around it can change"
                )
                .into());
            }
            let older_is_merge = if i == 0 {
                start == 0 || original[start - 1].1
            } else {
                range[i - 1].1
            };
            if matches!(step.action.as_str(), "squash" | "fixup") && older_is_merge {
                return Err(tr!("A commit can’t be combined with a merge").into());
            }
        }
        let messages = rewrite::plan_messages(steps)?;
        let message = self.write_rebase_messages(&messages)?;
        let dir = self.backend().paths().git_dir.join("amonhen");
        self.backend().files().write(
            Root::GitDir,
            "amonhen/rebase-merges.awk",
            rewrite::rebase_merges_program(steps).as_bytes(),
        )?;
        let program = dir.join("rebase-merges.awk");
        let base = range[0].0.parent;
        self.undo_log()?.record_stoppable(
            ops,
            label,
            &[],
            || {
                ops.rebase_todo(
                    base.as_ref(),
                    &program,
                    message.as_deref(),
                    true,
                    update_refs,
                )
            },
            |o| *o == Outcome::Conflict,
            |o| *o == Outcome::Stopped,
        )
    }

    /// コミットを編集する。非対話の rebase で行い、Undo できるように記録する。
    pub fn rewrite(&self, label: &str, edit: &HistoryEdit) -> Result<Outcome> {
        self.rewrite_with(label, edit, None)
    }

    /// `rewrite` に、積み重ねたブランチを一緒に動かすかのこの操作だけの指定を添えたもの。
    pub fn rewrite_with(
        &self,
        label: &str,
        edit: &HistoryEdit,
        update_refs: Option<bool>,
    ) -> Result<Outcome> {
        let ops = self.backend().operations()?;
        if ops.sequence_in_progress()?.is_some() {
            return Err(tr!("A merge or rebase is in progress. Finish or abort it first").into());
        }
        // 書き換える前に、最新の History で範囲を決める。
        self.load_history()?;
        let chain = self.editable_chain()?;
        // 編集モードの並びが最初の merge より先まであれば、merge を作り直しながら書き換える。
        if let HistoryEdit::Plan { steps } = edit
            && steps
                .iter()
                .any(|s| Oid::from_hex(&s.oid).is_none_or(|o| !chain.iter().any(|c| c.oid == o)))
        {
            return self.rewrite_through_merges(ops, label, steps, update_refs);
        }
        // メッセージと todo は `.git/amonhen` に書く。git に渡すのはリポジトリ側の絶対パス。
        let dir = self.backend().paths().git_dir.join("amonhen");
        let ids = chain.iter().map(|c| c.oid).collect();
        let stacked = self.stacked_branches(ops, &ids)?;
        let plan = rewrite::plan_rebase(&chain, edit, &stacked)?;
        let names: Vec<String> = stacked.into_iter().map(|(_, name)| name).collect();
        let files = self.backend().files();
        let message = self.write_rebase_messages(&plan.messages)?;
        files.write(Root::GitDir, "amonhen/rebase-todo", plan.todo.as_bytes())?;
        let todo = dir.join("rebase-todo");
        self.undo_log()?.record_stoppable(
            ops,
            label,
            &names,
            || {
                let outcome = ops.rebase_todo(
                    plan.base.as_ref(),
                    &todo,
                    message.as_deref(),
                    false,
                    update_refs,
                )?;
                // split: `edit` で止めたコミットを取り消し、変更を作業ツリーに戻す。
                if plan.reset_on_stop && outcome == Outcome::Stopped {
                    let parent = ops.resolve("HEAD^")?;
                    ops.reset(amonhen_model::ResetMode::Mixed, &parent)?;
                }
                Ok(outcome)
            },
            |o| *o == Outcome::Conflict,
            |o| *o == Outcome::Stopped,
        )
    }

    /// JSON の命令を実行し、JSON の結果を返す。
    pub fn run_json(&self, json: &str) -> String {
        // 手元の共有の daemon につないでいれば、daemon のコアで実行する（操作と Undo の記録を 1 か所にする）。
        // 手元のつながりそのものについての命令（daemon の記録を読む・取り消す）だけは、ここで実行する。
        if !matches!(op_name(json).as_str(), "remoteActivity" | "cancelActivity") {
            match self.delegate(&crate::delegate::Request::Command(json.to_owned())) {
                Ok(Some((crate::delegate::Reply::Command(reply), _))) => return reply,
                Ok(Some(_)) => {
                    let response = Response {
                        error: Some(tr!("Unexpected reply from the daemon")),
                        ..Default::default()
                    };
                    return serde_json::to_string(&response).unwrap_or_default();
                }
                Ok(None) => {}
                Err(e) => {
                    let response = Response {
                        error: Some(e.to_string()),
                        ..Default::default()
                    };
                    return serde_json::to_string(&response).unwrap_or_default();
                }
            }
        }
        let response = match serde_json::from_str::<Command>(json) {
            Ok(command) => match self.run_tracked(command, op_name(json)) {
                Ok(r) => r,
                Err(e) => Response::failed(e.to_string()),
            },
            Err(e) => Response {
                error: Some(tr!("Can’t read the command: {e}", e = e)),
                ..Default::default()
            },
        };
        serde_json::to_string(&response).unwrap_or_default()
    }

    /// 命令を実行し、Activity に記録する（命令の名前と、かかった時間）。
    fn run_tracked(&self, command: Command, name: String) -> Result<Response> {
        let repo = self
            .paths()
            .workdir
            .map(|w| w.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Activity が数秒ごとに daemon の記録を読む問い合わせは、記録しない（一覧がそれで埋まる）。
        if name == "remoteActivity" {
            return self.run(command);
        }
        amonhen_model::activity::track("command", &repo, name, || self.run(command))
    }

    pub fn run(&self, command: Command) -> Result<Response> {
        let ops = self.backend().operations()?;
        let undo = self.undo_log()?;
        let not_done = |o: &Outcome| *o != Outcome::Done;
        let done = |outcome: Outcome| {
            Ok(Response {
                ok: true,
                outcome: Some(outcome_name(outcome)),
                ..Default::default()
            })
        };
        let ok = || done(Outcome::Done);
        match command {
            Command::Checkout { branch } => {
                ops.switch_branch(&branch)?;
                ok()
            }
            Command::ForgetResolution { path } => {
                undo.record_files(
                    ops,
                    &format!("Forget Resolution of {path}"),
                    &[&path],
                    || ops.rerere_forget(&path),
                )?;
                ok()
            }
            Command::IgnoredFiles => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "paths": ops.ignored_files()? })),
                ..Default::default()
            }),
            Command::DeleteIgnored { paths } => {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                ops.delete_ignored(&paths)?;
                ok()
            }
            Command::ForgetAllResolutions => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "count": ops.rerere_forget_all()? })),
                ..Default::default()
            }),
            Command::CreateOrphanBranch { name, keep_files } => {
                ops.orphan_branch(name.trim(), keep_files)?;
                ok()
            }
            Command::Track { remote_branch } => {
                ops.track_branch(&remote_branch)?;
                ok()
            }
            Command::Detach { oid: o } => {
                ops.switch_to(&amonhen_model::HeadState {
                    branch: None,
                    oid: Some(oid(&o)?),
                })?;
                ok()
            }
            Command::Reset { oid: o, mode } => {
                if ops.sequence_in_progress()?.is_some() {
                    return Err(
                        tr!("A merge or rebase is in progress. Finish or abort it first").into(),
                    );
                }
                let target = oid(&o)?;
                let short = &target.to_string()[..8];
                let (mode, name) = match mode {
                    ResetKind::Soft => (amonhen_model::ResetMode::Soft, "soft"),
                    ResetKind::Mixed => (amonhen_model::ResetMode::Mixed, "mixed"),
                    ResetKind::Hard => (amonhen_model::ResetMode::Hard, "hard"),
                };
                // hard は作業中の変更を消し、Undo でも戻らないので、先に stash に残す（untracked は hard でも残る）。
                let stashed = mode == amonhen_model::ResetMode::Hard
                    && self
                        .backend()
                        .status()?
                        .entries
                        .iter()
                        .any(|e| e.kind != amonhen_model::StatusKind::Untracked);
                if stashed {
                    ops.stash_push(Some(&format!("Before reset --hard to {short}")), false)?;
                }
                undo.record_reset(ops, &format!("Reset ({name}) to {short}"), mode, || {
                    ops.reset(mode, &target)
                })?;
                Ok(Response {
                    ok: true,
                    outcome: Some(outcome_name(Outcome::Done)),
                    info: Some(serde_json::json!({ "stashed": stashed })),
                    ..Default::default()
                })
            }
            Command::CreateBranch {
                name,
                start,
                checkout,
            } => {
                undo.record(
                    ops,
                    &format!("Create branch {name}"),
                    &[heads(&name)],
                    false,
                    || ops.create_branch(&name, &start, checkout),
                    |_| false,
                )?;
                ok()
            }
            Command::DeleteBranch { name, force } => {
                let error = undo.record_branch_delete(
                    ops,
                    &format!("Delete branch {name}"),
                    &[&name],
                    || Ok(ops.delete_branch(&name, force).err()),
                )?;
                if let Some(e) = error {
                    return Err(e);
                }
                ok()
            }
            Command::DeleteBranches { branches } => {
                let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
                let label = match &branches[..] {
                    [one] => format!("Delete branch {}", one.name),
                    many => format!("Delete {} branches", many.len()),
                };
                // 途中で失敗しても、消せた分は記録してからエラーを返す。
                let error = undo.record_branch_delete(ops, &label, &names, || {
                    Ok(branches.iter().enumerate().find_map(|(deleted, b)| {
                        let error = ops.delete_branch(&b.name, b.force).err()?;
                        Some(explain_refused_delete(&b.name, deleted, error))
                    }))
                })?;
                if let Some(e) = error {
                    return Err(e);
                }
                ok()
            }
            Command::RenameBranch { from, to } => {
                undo.record_rename(ops, &format!("Rename {from} to {to}"), &from, &to)?;
                ok()
            }
            Command::DeleteTag { name } => {
                undo.record(
                    ops,
                    &format!("Delete tag {name}"),
                    &[format!("refs/tags/{name}")],
                    false,
                    || ops.delete_tag(&name),
                    |_| false,
                )?;
                ok()
            }
            Command::PushTags {
                remote,
                names,
                force,
                delete,
            } => {
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                ops.push_tags(&remote, &names, force, delete)?;
                let tags = tag_objects(ops, &names)?;
                crate::remote_tags::record_push(&*self.backend().files(), &remote, &tags, delete)?;
                ok()
            }
            Command::TagRemotes => {
                let refs = self.refs()?;
                let names: Vec<&str> = refs
                    .refs
                    .entries
                    .iter()
                    .filter(|e| e.kind == RefKind::Tag)
                    .map(|e| e.name.as_str())
                    .collect();
                let tags = tag_objects(ops, &names)?;
                Ok(Response {
                    ok: true,
                    info: Some(crate::remote_tags::status(
                        ops,
                        &*self.backend().files(),
                        &tags,
                    )?),
                    ..Default::default()
                })
            }
            Command::CreateTag {
                name,
                target,
                message,
            } => {
                undo.record(
                    ops,
                    &format!("Create tag {name}"),
                    &[format!("refs/tags/{name}")],
                    false,
                    || ops.create_tag(&name, &target, message.as_deref()),
                    |_| false,
                )?;
                ok()
            }
            Command::Merge { rev, one_by_one } => {
                let revs = rev.into_vec();
                let label = format!("Merge {}", names_label(&revs));
                self.merge_revs(ops, undo, &label, &revs, one_by_one)
            }
            Command::MergeInto {
                target,
                rev,
                one_by_one,
            } => {
                let revs = rev.into_vec();
                let current = ops.head_state()?.branch;
                let full = heads(&target);
                // ほかの worktree でチェックアウト中なら、fast-forward でも動かさない（その作業ツリーとずれる。merge なら
                // git が checkout を断る）。
                if current.as_deref() != Some(full.as_str())
                    && let Some(path) = self.worktree_using(ops, &target)
                {
                    return Err(tr!(
                        "{branch} is checked out in the worktree {path}; merge there",
                        branch = target,
                        path = path
                    )
                    .into());
                }
                if let [rev] = revs.as_slice()
                    && current.as_deref() != Some(full.as_str())
                    && ops.is_ancestor(&full, rev)?
                {
                    let from = ops.resolve(&full)?;
                    let to = ops.resolve(rev)?;
                    undo.record(
                        ops,
                        &format!("Fast-forward {target} to {rev}"),
                        std::slice::from_ref(&full),
                        false,
                        || ops.update_ref(&full, Some(&to), Some(&from)),
                        |_| false,
                    )?;
                    return ok();
                }
                if current.as_deref() != Some(full.as_str()) {
                    ops.switch_branch(&target)?;
                }
                let label = format!("Merge {} into {target}", names_label(&revs));
                self.merge_revs(ops, undo, &label, &revs, one_by_one)
            }
            Command::Rebase { onto, update_refs } => {
                // 載せ直すコミット（onto..HEAD）に先頭があるブランチは、`rebase.updateRefs` が true なら git が
                // 一緒に動かす。動きうるので Undo に記録する。
                let stacked = match (ops.head_state()?.oid, ops.resolve(&onto)) {
                    (Some(head), Ok(base)) => {
                        let ids = self
                            .backend()
                            .commits_since(&[head], &[base])?
                            .into_iter()
                            .map(|c| c.id)
                            .collect();
                        self.stacked_branches(ops, &ids)?
                    }
                    _ => Vec::new(),
                };
                let names: Vec<String> = stacked.into_iter().map(|(_, name)| name).collect();
                let before = ops.resolve_refs(&names)?;
                let outcome = undo.record(
                    ops,
                    &format!("Rebase onto {onto}"),
                    &names,
                    false,
                    || ops.rebase(&onto, update_refs),
                    not_done,
                )?;
                let moved = Self::moved_branches(ops, &before)?;
                Ok(Response {
                    ok: true,
                    outcome: Some(outcome_name(outcome)),
                    info: (!moved.is_empty())
                        .then(|| serde_json::json!({ "movedBranches": moved })),
                    ..Default::default()
                })
            }
            Command::CherryPick {
                oid: o,
                oids,
                is_merge,
            } => {
                let (ids, what) = picked(o, oids)?;
                done(undo.record(
                    ops,
                    &format!("Cherry-pick {what}"),
                    &[],
                    false,
                    || ops.cherry_pick(&ids, is_merge),
                    not_done,
                )?)
            }
            Command::Revert {
                oid: o,
                oids,
                is_merge,
            } => {
                let (ids, what) = picked(o, oids)?;
                done(undo.record(
                    ops,
                    &format!("Revert {what}"),
                    &[],
                    false,
                    || ops.revert(&ids, is_merge),
                    not_done,
                )?)
            }
            Command::FormatPatch {
                oids,
                dir,
                file,
                cover_letter,
                reroll,
            } => {
                let commits = oids.iter().map(|o| oid(o)).collect::<Result<Vec<_>>>()?;
                let options = amonhen_model::FormatPatch {
                    cover_letter,
                    reroll,
                    single: file.is_some(),
                };
                let patches = ops.format_patch(&commits, &options)?;
                let mut written = Vec::new();
                match (dir, file) {
                    (_, Some(file)) => {
                        let content: Vec<u8> =
                            patches.into_iter().flat_map(|p| p.content).collect();
                        std::fs::write(&file, content)?;
                        written.push(file);
                    }
                    (Some(dir), None) => {
                        let dir = std::path::Path::new(&dir);
                        std::fs::create_dir_all(dir)?;
                        for patch in patches {
                            let path = dir.join(&patch.name);
                            std::fs::write(&path, &patch.content)?;
                            written.push(path.to_string_lossy().into_owned());
                        }
                    }
                    (None, None) => {
                        return Err(tr!("Choose a folder or a file for the patches").into());
                    }
                }
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "files": written })),
                    ..Default::default()
                })
            }
            Command::Archive {
                rev,
                format,
                prefix,
                paths,
                file,
            } => {
                let data = ops.archive(&amonhen_model::Archive {
                    rev,
                    format,
                    prefix,
                    paths,
                })?;
                std::fs::write(&file, &data)
                    .map_err(|e| tr!("Couldn’t write {file}: {error}", file = file, error = e))?;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "file": file, "size": data.len() })),
                    ..Default::default()
                })
            }
            Command::ArchiveContents { rev, paths } => {
                let need = ops.archive_contents_to_fetch(
                    &rev,
                    &paths,
                    crate::repository::CONTENTS_FETCH_LIMIT,
                )?;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "count": need.ids.len(), "more": need.more })),
                    ..Default::default()
                })
            }
            Command::Bundle { refs, bases, file } => {
                let data = ops.bundle(&amonhen_model::Bundle { refs, bases })?;
                std::fs::write(&file, &data)
                    .map_err(|e| tr!("Couldn’t write {file}: {error}", file = file, error = e))?;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "file": file, "size": data.len() })),
                    ..Default::default()
                })
            }
            Command::BundlePreview { refs, bases } => {
                let preview = ops.bundle_preview(
                    &amonhen_model::Bundle { refs, bases },
                    crate::repository::CONTENTS_FETCH_LIMIT,
                )?;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "commits": preview.commits,
                        "contents": preview.contents.ids.len(),
                        "more": preview.contents.more,
                        "beyondShallow": preview.beyond_shallow,
                    })),
                    ..Default::default()
                })
            }
            Command::BundleCheck { file } => {
                let path = std::path::Path::new(&file);
                let info = std::fs::File::open(path)
                    .ok()
                    .and_then(|f| amonhen_model::parse_bundle_header(std::io::BufReader::new(f)))
                    .ok_or_else(|| tr!("{file} isn’t a bundle file", file = file))?;
                let missing = ops.missing_commits(&info.prerequisite_ids)?;
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let name: String = stem
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect::<String>()
                    .trim_matches(|c| c == '-' || c == '.')
                    .to_owned();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "refs": info.refs,
                        "head": info.head,
                        "prerequisites": info.prerequisites,
                        "missing": missing.len(),
                        "name": if name.is_empty() { "bundle".to_owned() } else { name },
                    })),
                    ..Default::default()
                })
            }
            Command::FetchBundle { file, name } => {
                let data = std::fs::read(&file)
                    .map_err(|e| tr!("Couldn’t read {file}: {error}", file = file, error = e))?;
                let refs = ops.fetch_bundle(&data, name.trim())?;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "refs": refs })),
                    ..Default::default()
                })
            }
            Command::ReadPatch { paths } => {
                let data = read_patches(&paths)?;
                let summary = crate::patch::parse(&data);
                let mailbox = summary.kind == crate::patch::Kind::Mailbox;
                // am は index にも当てるので、index と作業ツリーの両方に当たるかを調べる。
                let check = amonhen_model::ApplyPatch {
                    index: mailbox,
                    check: true,
                    ..Default::default()
                };
                let error = match summary.patches.iter().all(|p| p.files.is_empty()) {
                    true => Some(tr!("There are no changes in the patch").to_string()),
                    false => ops.apply_patch(&data, &check).err().map(|e| e.to_string()),
                };
                let config = ops.config_list()?;
                let three_way = config
                    .iter()
                    .rev()
                    .find(|e| e.key.eq_ignore_ascii_case("am.threeway"))
                    .is_some_and(|e| {
                        matches!(
                            e.value.to_ascii_lowercase().as_str(),
                            "" | "true" | "yes" | "on" | "1"
                        )
                    });
                let mut info = serde_json::to_value(&summary)?;
                info["applies"] = error.is_none().into();
                info["error"] = error.into();
                info["threeWay"] = three_way.into();
                Ok(Response {
                    ok: true,
                    info: Some(info),
                    ..Default::default()
                })
            }
            Command::Am { paths, three_way } => {
                let data = read_patches(&paths)?;
                let count = crate::patch::parse(&data).patches.len();
                let label = match count {
                    1 => "Apply 1 patch".to_owned(),
                    n => format!("Apply {n} patches"),
                };
                // 止まったら、終わったとき（Continue・Skip）にまとめて 1 つ記録する。Abort なら残らない。
                done(undo.record_stoppable(
                    ops,
                    &label,
                    &[],
                    || ops.am(&data, three_way),
                    |_| false,
                    not_done,
                )?)
            }
            Command::AmThreeWay => {
                let outcome = ops.am_three_way()?;
                if outcome != Outcome::Done {
                    let files = self.backend().files();
                    if let Some(next) = files.read(Root::GitDir, "rebase-apply/next")? {
                        files.write(Root::GitDir, AM_RETRIED, &next)?;
                    }
                }
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::ApplyPatch {
                paths,
                index,
                three_way,
            } => {
                let data = read_patches(&paths)?;
                let touched = crate::patch::parse(&data).paths();
                let touched: Vec<&str> = touched.iter().map(String::as_str).collect();
                let label = match paths.as_slice() {
                    [one] => format!(
                        "Apply {}",
                        std::path::Path::new(one)
                            .file_name()
                            .map_or_else(|| one.clone(), |n| n.to_string_lossy().into_owned())
                    ),
                    many => format!("Apply {} patches", many.len()),
                };
                let options = amonhen_model::ApplyPatch {
                    index,
                    three_way,
                    check: false,
                };
                done(undo.record_files(ops, &label, &touched, || ops.apply_patch(&data, &options))?)
            }
            Command::Abort => {
                let sequence = ops
                    .sequence_in_progress()?
                    .ok_or_else(|| tr!("No operation in progress"))?;
                ops.abort(sequence)?;
                undo.settle_pending(ops)?;
                ok()
            }
            Command::Continue => {
                let sequence = ops
                    .sequence_in_progress()?
                    .ok_or_else(|| tr!("No operation in progress"))?;
                let outcome = ops.continue_sequence(sequence)?;
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::Skip => {
                let sequence = ops
                    .sequence_in_progress()?
                    .ok_or_else(|| tr!("No operation in progress"))?;
                let outcome = ops.skip_sequence(sequence)?;
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::Fetch { remote } => {
                ops.fetch(remote.as_deref())?;
                // リモートのタグは手元の ref からは分からないので、ついでに読んでおく（読めなくても fetch は成功）。
                let _ = crate::remote_tags::refresh(ops, &*self.backend().files());
                ok()
            }
            Command::Deepen { commits, since } => {
                let how = match (commits, since) {
                    (Some(n), None) if n > 0 => amonhen_model::Deepen::Commits(n),
                    (None, Some(date)) if !date.trim().is_empty() => {
                        amonhen_model::Deepen::Since(date.trim().to_owned())
                    }
                    _ => return Err(tr!("Give either commits (1 or more) or since").into()),
                };
                self.deepen(ops, &how)?;
                ok()
            }
            Command::Unshallow => {
                self.deepen(ops, &amonhen_model::Deepen::All)?;
                ok()
            }
            Command::FetchAllContents => {
                self.fetch_all_contents(ops)?;
                ok()
            }
            Command::Pull { mode } => {
                done(undo.record(ops, "Pull", &[], false, || ops.pull(mode), not_done)?)
            }
            Command::FastForward { branch } => {
                let (name, from, to) = self.fast_forward_target(ops, &branch)?;
                undo.record(
                    ops,
                    &format!("Fast-forward {branch}"),
                    std::slice::from_ref(&name),
                    false,
                    || ops.update_ref(&name, Some(&to), Some(&from)),
                    |_| false,
                )?;
                ok()
            }
            Command::Push {
                remote,
                branch,
                remote_branch,
                force,
                expect,
                set_upstream,
            } => {
                let expect = expect.as_deref().map(oid).transpose()?;
                ops.push(
                    &remote,
                    &branch,
                    &remote_branch,
                    force,
                    expect.as_ref(),
                    set_upstream,
                )?;
                ok()
            }
            Command::Discard { paths } => {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                let label = match paths.as_slice() {
                    [one] => format!("Discard {one}"),
                    many => format!("Discard {} files", many.len()),
                };
                undo.record_files(ops, &label, &paths, || ops.restore_worktree(&paths))?;
                ok()
            }
            Command::RestoreFiles { rev, paths, staged } => {
                if paths.is_empty() {
                    return Err(tr!("Give the files to restore").into());
                }
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                let short: String = rev.chars().take(7).collect();
                let label = match paths.as_slice() {
                    [one] => format!("Restore {one} from {short}"),
                    many => format!("Restore {} files from {short}", many.len()),
                };
                undo.record_files(ops, &label, &paths, || {
                    ops.restore_from(&rev, &paths, staged)
                })?;
                ok()
            }
            Command::StashPush {
                message,
                include_untracked,
                keep_index,
                staged,
                paths,
            } => {
                ops.stash_push_with(&amonhen_model::StashOptions {
                    message,
                    include_untracked,
                    keep_index,
                    staged,
                    paths,
                })?;
                ok()
            }
            Command::StashApply { name, pop } => done(ops.stash_apply(&name, pop)?),
            Command::StashDrop { name } => {
                let refs = self.load_refs()?;
                let entry = refs
                    .refs
                    .entries
                    .iter()
                    .find(|e| e.kind == amonhen_model::RefKind::Stash && e.name == name)
                    .ok_or_else(|| tr!("{name} not found", name = name))?;
                let target = entry.target.ok_or_else(|| tr!("The stash has no commit"))?;
                let message = entry.detail.clone().unwrap_or_else(|| name.clone());
                undo.record_stash_drop(ops, &name, &target, &message)?;
                ok()
            }
            Command::StashBranch { name, branch } => done(ops.stash_branch(&branch, &name)?),
            Command::Reflog { name, limit } => {
                let name = name.as_deref().unwrap_or("HEAD");
                let entries = self.backend().reflog(name, limit.unwrap_or(500))?;
                let history = self.load_history()?;
                let ids: Vec<Oid> = entries.iter().map(|e| e.new).collect();
                // 消えたコミット（gc 済み）があるとまとめて読めないので、そのときは件名なしで出す。
                let texts = self.commit_texts(&ids).unwrap_or_default();
                let entries: Vec<serde_json::Value> = entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        serde_json::json!({
                            "selector": format!("{name}@{{{i}}}"),
                            "old": e.old.map(|o| o.to_string()),
                            "new": e.new.to_string(),
                            "time": e.time,
                            "message": e.message,
                            "subject": texts.get(i).map(|t| String::from_utf8_lossy(t.subject()).into_owned()).unwrap_or_default(),
                            "reachable": history.row_of(&e.new).is_some(),
                        })
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "entries": entries })),
                    ..Default::default()
                })
            }
            Command::RangeDiff {
                ranges,
                creation_factor,
            } => {
                let pairs = ops.range_diff(&ranges, creation_factor)?;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "pairs": pairs })),
                    ..Default::default()
                })
            }
            Command::PreviousVersion { branch } => {
                let previous = self.previous_version(ops, branch.as_deref())?;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "previous": previous })),
                    ..Default::default()
                })
            }
            Command::StashBases => {
                let refs = self.load_refs()?;
                let mut stashes = Vec::new();
                if ops.resolve("HEAD").is_ok() {
                    for entry in refs.refs.entries.iter() {
                        let (amonhen_model::RefKind::Stash, Some(target)) =
                            (entry.kind, entry.target)
                        else {
                            continue;
                        };
                        let base = ops.resolve(&format!("{target}^1"))?;
                        let (ahead, behind) = ops.count_apart("HEAD", &base.to_string())?;
                        stashes.push(serde_json::json!({
                            "name": entry.name,
                            "base": base.to_string(),
                            "ahead": ahead,
                            "behind": behind,
                        }));
                    }
                }
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "stashes": stashes })),
                    ..Default::default()
                })
            }
            Command::UpdateSubmodules {
                paths,
                recursive,
                remote,
            } => {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                let options = amonhen_model::SubmoduleUpdate { recursive, remote };
                ops.update_submodules(&paths, &options)?;
                ok()
            }
            Command::Submodules => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "submodules": ops.submodules()? })),
                ..Default::default()
            }),
            Command::ChangeSubmodule { change } => {
                ops.change_submodule(&change)?;
                ok()
            }
            Command::Subtrees => Ok(Response {
                ok: true,
                info: Some(serde_json::to_value(ops.subtrees()?)?),
                ..Default::default()
            }),
            Command::Subtree { action: op } => {
                use amonhen_model::SubtreeOp;
                let (label, refs) = match &op {
                    SubtreeOp::Add { dir, .. } => (format!("Subtree Add {dir}"), Vec::new()),
                    SubtreeOp::Pull { dir, .. } => (format!("Subtree Pull {dir}"), Vec::new()),
                    SubtreeOp::Push { .. } => {
                        ops.subtree(&op)?;
                        return ok();
                    }
                    SubtreeOp::Split { dir, branch } => {
                        (format!("Subtree Split {dir}"), vec![heads(branch.trim())])
                    }
                };
                let outcome = undo.record(
                    ops,
                    &label,
                    &refs,
                    false,
                    || ops.subtree(&op),
                    |o| *o != Outcome::Done,
                )?;
                Ok(Response {
                    ok: true,
                    outcome: Some(outcome_name(outcome)),
                    ..Default::default()
                })
            }
            Command::Undo => {
                let label = undo.undo(ops)?;
                Ok(Response {
                    ok: true,
                    label,
                    ..Default::default()
                })
            }
            Command::UndoLog => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "entries": undo.entries() })),
                ..Default::default()
            }),
            Command::UndoPeek => {
                // 外で rebase を終わらせた場合も、ここで確定させる。
                undo.settle_pending(ops)?;
                let peek = undo.peek_kind();
                // 動くブランチ（今と戻る先）。確認の前と後の絵に使う。
                let (head, moves) = undo.peek_moves().unwrap_or_default();
                let moves: Vec<_> = moves
                    .into_iter()
                    .map(|(name, from, to)| {
                        let short = name.strip_prefix("refs/heads/").unwrap_or(&name).to_owned();
                        serde_json::json!({ "name": short, "from": from, "to": to })
                    })
                    .collect();
                let head = head.map(|h| h.strip_prefix("refs/heads/").unwrap_or(&h).to_owned());
                Ok(Response {
                    ok: true,
                    info: peek.as_ref().map(|(_, kind)| {
                        serde_json::json!({ "kind": kind, "head": head, "moves": moves })
                    }),
                    label: peek.map(|(label, _)| label),
                    ..Default::default()
                })
            }
            Command::Rewrite {
                label,
                edit,
                update_refs,
            } => done(self.rewrite_with(&label, &edit, update_refs)?),
            Command::CommitMessage { oid: o } => {
                // "HEAD" なら今のコミット（Amend の初期値）。
                let id = if o == "HEAD" {
                    self.backend()
                        .head()?
                        .ok_or_else(|| tr!("No commits yet"))?
                } else {
                    oid(&o)?
                };
                Ok(Response {
                    ok: true,
                    text: Some(self.commit_message(&id)?),
                    ..Default::default()
                })
            }
            Command::BranchCleanup { pull_requests } => Ok(Response {
                ok: true,
                info: Some(self.branch_cleanup(&pull_requests)?),
                ..Default::default()
            }),
            Command::BranchGroups { pull_requests } => Ok(Response {
                ok: true,
                info: Some(self.branch_groups(&pull_requests)?),
                ..Default::default()
            }),
            Command::RewritePreview {
                edit,
                onto,
                update_refs,
            } => Ok(Response {
                ok: true,
                info: Some(self.rewrite_preview(
                    ops,
                    edit.as_ref(),
                    onto.as_deref(),
                    update_refs,
                )?),
                ..Default::default()
            }),
            Command::EditableCommits { through_merges } => {
                let text = if through_merges {
                    self.first_parent_chain()?
                        .iter()
                        .map(|(c, merge)| {
                            if *merge {
                                format!("{} merge", c.oid)
                            } else {
                                c.oid.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    self.editable_chain()?
                        .iter()
                        .map(|c| c.oid.to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                Ok(Response {
                    ok: true,
                    text: Some(text),
                    ..Default::default()
                })
            }
            Command::SquashRangeMessage { oids } => {
                let ids = oids.iter().map(|o| oid(o)).collect::<Result<Vec<_>>>()?;
                let chain = self.editable_chain()?;
                // chain は新しい順。選んだコミットの位置が隙間なく続いていること。
                let mut positions = ids
                    .iter()
                    .map(|id| {
                        chain.iter().position(|c| c.oid == *id).ok_or_else(|| {
                            tr!("Some commits are outside the editable range (not reachable from HEAD, or before a merge)")
                        })
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                positions.sort_unstable();
                positions.dedup();
                if positions.len() < 2 || positions.windows(2).any(|w| w[1] != w[0] + 1) {
                    return Err(tr!("To squash, select two or more consecutive commits").into());
                }
                let newest = chain[positions[0]].oid;
                let oldest = chain[*positions.last().unwrap()].oid;
                let text = positions
                    .iter()
                    .rev()
                    .map(|&p| {
                        self.commit_message(&chain[p].oid)
                            .map(|m| m.trim_end().to_owned())
                    })
                    .collect::<Result<Vec<_>>>()?
                    .join("\n\n");
                Ok(Response {
                    ok: true,
                    text: Some(text.clone()),
                    info: Some(serde_json::json!({
                        "oldest": oldest.to_string(),
                        "newest": newest.to_string(),
                        "message": text,
                    })),
                    ..Default::default()
                })
            }
            Command::SquashMessage { oid: o } => {
                let id = oid(&o)?;
                let chain = self.editable_chain()?;
                let parent = chain
                    .iter()
                    .find(|c| c.oid == id)
                    .and_then(|c| c.parent)
                    .ok_or_else(|| tr!("Can’t squash: the parent isn’t in the editable range"))?;
                let text = format!(
                    "{}\n\n{}",
                    self.commit_message(&parent)?.trim_end(),
                    self.commit_message(&id)?.trim_end()
                );
                Ok(Response {
                    ok: true,
                    text: Some(text),
                    ..Default::default()
                })
            }
            Command::ResolveConflict {
                path,
                index,
                choice,
            } => {
                let files = self.backend().files();
                let data = files
                    .read(Root::Workdir, &path)?
                    .ok_or_else(|| tr!("File not found"))?;
                let resolved = crate::conflict::resolve(&data, index, choice)
                    .ok_or_else(|| tr!("That conflict no longer exists"))?;
                undo.record_files(
                    ops,
                    &format!("Resolve conflict in {path}"),
                    &[&path],
                    || files.write(Root::Workdir, &path, &resolved),
                )?;
                ok()
            }
            Command::CommitInfo { oid: o } => Ok(Response {
                ok: true,
                info: Some(self.commit_info(&oid(&o)?)?),
                ..Default::default()
            }),
            Command::VerifySignatures { oids } => {
                let ids = oids.iter().map(|o| oid(o)).collect::<Result<Vec<_>>>()?;
                let found = self.verify_signatures(&ids)?;
                // 署名者がコミッターと一致するかを FE が確かめられるよう、正しい署名にはコミッターのメールを添える。
                let signed: Vec<Oid> = ids
                    .iter()
                    .zip(&found)
                    .filter(|(_, c)| matches!(c.status, 'G' | 'U'))
                    .map(|(id, _)| *id)
                    .collect();
                let mut committers = std::collections::HashMap::new();
                if !signed.is_empty() {
                    for (id, text) in signed.iter().zip(self.backend().commit_texts(&signed)?) {
                        committers.insert(*id, committer_email(&text.raw));
                    }
                }
                let checks: Vec<serde_json::Value> = ids
                    .iter()
                    .zip(&found)
                    .map(|(id, c)| {
                        let mut json = signature_json(c);
                        json["committerEmail"] =
                            committers.get(id).cloned().unwrap_or_default().into();
                        json
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "checks": checks })),
                    ..Default::default()
                })
            }
            Command::TagSignatures { names } => {
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                let tags = self.annotated_tags(ops, &names)?;
                let signatures =
                    self.verify_signed_tags(tags.iter().filter_map(|(_, t)| t.as_ref()));
                let tags: Vec<serde_json::Value> = names
                    .iter()
                    .zip(&tags)
                    .map(|(name, (_, tag))| {
                        let Some(tag) = tag else {
                            return serde_json::json!({ "name": name, "id": null, "signed": null });
                        };
                        serde_json::json!({
                            "name": name,
                            "id": tag.id.to_string(),
                            "signed": tag.signed,
                            "taggerEmail": tag.tagger_email,
                            "message": tag.message,
                            "signature": signatures.get(&tag.id).map(signature_json),
                        })
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "tags": tags })),
                    ..Default::default()
                })
            }
            Command::ResolveCommit { rev } => Ok(Response {
                ok: true,
                text: Some(ops.resolve(&rev)?.to_string()),
                ..Default::default()
            }),
            Command::CommitExtras { oid: o } => Ok(Response {
                ok: true,
                info: Some(self.commit_extras(&oid(&o)?)?),
                ..Default::default()
            }),
            Command::TakeSide { path, ours } => {
                ops.take_side(&path, ours)?;
                ok()
            }
            Command::MergeSides { path } => {
                let data = self
                    .backend()
                    .files()
                    .read(Root::Workdir, &path)?
                    .ok_or_else(|| tr!("File not found"))?;
                // UTF-8 でなければ開かない（文字列にするときに置き換えた内容を、保存で書いてしまう）。
                let not_text = || {
                    tr!(
                        "It isn’t UTF-8 text, so it can’t be edited here. Use Take Current File or Take Incoming File."
                    )
                };
                if !crate::conflict::is_text(&data) {
                    return Err(not_text().into());
                }
                let markers = crate::conflict::parse(&data).len();
                // マーカーが残っていない（rerere が解決を当てた、ほかのエディタで直した）なら、左右には index に
                // 残っている両側の内容を出す（3 欄が同じにならないように）。
                let stages = if markers == 0 {
                    let backend = self.backend();
                    let [base, ours, theirs] = ops.conflict_stages(&path)?;
                    let text = |id: Option<Oid>| -> Result<Option<serde_json::Value>> {
                        id.map(|id| {
                            let blob = backend.blob(&id)?;
                            let text = backend.to_worktree_form(&path, &blob)?;
                            if !crate::conflict::is_text(&text) {
                                return Err(not_text().into());
                            }
                            let text = String::from_utf8_lossy(&text);
                            Ok(serde_json::json!({ "text": text, "regions": [] }))
                        })
                        .transpose()
                    };
                    match (ours, theirs) {
                        (Some(_), Some(_)) => Some((text(ours)?, text(theirs)?, text(base)?)),
                        _ => None,
                    }
                } else {
                    None
                };
                let side = |choice| {
                    crate::conflict::side(&data, choice).map(|s| {
                        serde_json::json!({
                            "text": String::from_utf8_lossy(&s.text),
                            "regions": s.regions.iter().map(|r| [r.start, r.end]).collect::<Vec<_>>(),
                        })
                    })
                };
                use crate::conflict::Choice;
                let (current, incoming, base) = stages.unwrap_or_else(|| {
                    (side(Choice::Ours), side(Choice::Theirs), side(Choice::Base))
                });
                let rerere = markers == 0 && Self::rerere_applied(&self.rerere_tracked()?, &path);
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "current": current,
                        "incoming": incoming,
                        "base": base,
                        "result": String::from_utf8_lossy(&data),
                        "markers": markers,
                        "rerere": rerere,
                    })),
                    ..Default::default()
                })
            }
            Command::ResolveText {
                text,
                index,
                choice,
            } => {
                let resolved = crate::conflict::resolve(text.as_bytes(), index, choice)
                    .ok_or_else(|| tr!("That conflict no longer exists"))?;
                Ok(Response {
                    ok: true,
                    text: Some(String::from_utf8_lossy(&resolved).into_owned()),
                    ..Default::default()
                })
            }
            Command::MagicMerge { text, safe, index } => {
                let rules = if safe {
                    crate::conflict::MagicRules::Safe
                } else {
                    crate::conflict::MagicRules::All
                };
                let (merged, resolved) = crate::conflict::magic(text.as_bytes(), rules, index);
                let remaining = crate::conflict::parse(&merged).len();
                let text = String::from_utf8_lossy(&merged).into_owned();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "text": text, "resolved": resolved, "remaining": remaining
                    })),
                    ..Default::default()
                })
            }
            Command::ConflictRegions { text, originals } => {
                let data = text.as_bytes();
                let ls = crate::conflict::line_ranges(data);
                let join = |r: &std::ops::Range<usize>| -> String {
                    r.clone()
                        .map(|i| String::from_utf8_lossy(&data[ls[i].clone()]))
                        .collect()
                };
                let found: Vec<_> = crate::conflict::parse(data)
                    .iter()
                    .map(|c| (c.lines.clone(), join(&c.ours), join(&c.theirs)))
                    .collect();
                let matched = originals.map(|o| {
                    let sides: Vec<_> = found
                        .iter()
                        .map(|(_, c, i)| (c.clone(), i.clone()))
                        .collect();
                    crate::conflict::match_originals(&o, &sides)
                });
                let conflicts: Vec<_> = found
                    .into_iter()
                    .enumerate()
                    .map(|(n, (lines, current, incoming))| {
                        let mut c = serde_json::json!({
                            "lines": [lines.start, lines.end],
                            "current": current,
                            "incoming": incoming,
                        });
                        if let Some(m) = &matched {
                            c["original"] = serde_json::json!(m[n]);
                        }
                        c
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "conflicts": conflicts })),
                    ..Default::default()
                })
            }
            Command::Blame { path, rev } => {
                let rev = rev.map(|r| oid(&r)).transpose()?;
                let blame = crate::blame::parse_blame(&ops.blame(&path, rev.as_ref())?);
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::to_value(blame)?),
                    ..Default::default()
                })
            }
            Command::FileHistory {
                path,
                limit,
                merges,
            } => {
                // 1 つ多く読んで、その先があるかを知る。
                let mut entries = crate::blame::parse_file_log(&ops.file_log(
                    &path,
                    limit.map(|n| n + 1),
                    merges,
                )?);
                let more = limit.is_some_and(|n| entries.len() > n);
                if let Some(n) = limit {
                    entries.truncate(n);
                }
                if let Some(mailmap) = self.mailmap() {
                    for e in &mut entries {
                        e.apply_mailmap(&mailmap);
                    }
                }
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "entries": entries, "more": more })),
                    ..Default::default()
                })
            }
            Command::PredictMerge { ours, theirs } => {
                let theirs = theirs.into_vec();
                let steps = ops.predict_merge(ours.as_deref().unwrap_or("HEAD"), &theirs)?;
                let mut info = serde_json::json!({ "conflicts": null, "steps": null });
                if let Some(steps) = steps {
                    let mut all: Vec<&String> = steps.iter().flatten().collect();
                    all.sort();
                    all.dedup();
                    info["conflicts"] = serde_json::json!(all);
                    info["steps"] = theirs
                        .iter()
                        .zip(&steps)
                        .map(|(rev, conflicts)| serde_json::json!({ "rev": rev, "conflicts": conflicts }))
                        .collect();
                }
                Ok(Response {
                    ok: true,
                    info: Some(info),
                    ..Default::default()
                })
            }
            Command::MergePreview {
                theirs,
                ours,
                ff_setting,
            } => Ok(Response {
                ok: true,
                info: Some(self.merge_preview(
                    ops,
                    ours.as_deref().unwrap_or("HEAD"),
                    &theirs,
                    ff_setting.as_deref().unwrap_or("merge.ff"),
                )?),
                ..Default::default()
            }),
            Command::OctopusPreview { theirs } => Ok(Response {
                ok: true,
                info: Some(self.octopus_preview(ops, &theirs)?),
                ..Default::default()
            }),
            Command::PickPreview {
                oids,
                revert,
                subjects,
            } => Ok(Response {
                ok: true,
                info: Some(self.pick_preview(ops, &oids, revert, &subjects)?),
                ..Default::default()
            }),
            Command::DeletePreview { branch, against } => {
                let history = self.load_history()?;
                let row = |rev: &str| -> Result<u32> {
                    let id = ops.resolve(rev)?;
                    history.row_of(&id).ok_or_else(|| {
                        tr!("{rev} isn’t in the History (reload it)", rev = rev).into()
                    })
                };
                let base =
                    crate::repository::merge_base(&history.graph, row(&branch)?, row(&against)?)
                        .map(|r| history.graph.ids()[r as usize].to_string())
                        .ok_or_else(|| {
                            tr!("{rev} isn’t in the History (reload it)", rev = against)
                        })?;
                Ok(Response {
                    ok: true,
                    info: Some(self.move_preview(ops, &branch, &base, None)?),
                    ..Default::default()
                })
            }
            Command::MovePreview { from, to, branch } => Ok(Response {
                ok: true,
                info: Some(self.move_preview(ops, &from, &to, branch.as_deref())?),
                ..Default::default()
            }),
            Command::Between { base, tip, limit } => {
                let history = self.load_history()?;
                let graph = &history.graph;
                let row = |rev: &str| -> Result<usize> {
                    let id = ops.resolve(rev)?;
                    history.row_of(&id).map(|r| r as usize).ok_or_else(|| {
                        tr!("{rev} isn’t in the History (reload it)", rev = rev).into()
                    })
                };
                let (base, tip) = (row(&base)?, row(&tip)?);
                // base から辿れるものに印を付けてから、tip から印のないものだけを辿る。
                let walk = |start: usize, stop: &[bool], seen: &mut Vec<bool>| {
                    let mut stack = vec![start];
                    while let Some(r) = stack.pop() {
                        if seen[r] || stop[r] {
                            continue;
                        }
                        seen[r] = true;
                        stack.extend(graph.parents(r).iter().map(|&p| p as usize));
                    }
                };
                let none = vec![false; graph.len()];
                let mut reachable = vec![false; graph.len()];
                walk(base, &none, &mut reachable);
                let mut only = vec![false; graph.len()];
                walk(tip, &reachable, &mut only);
                // History の行は新しい順。
                let rows: Vec<usize> = (0..graph.len()).filter(|&r| only[r]).collect();
                let count = rows.len();
                let ids: Vec<Oid> = rows
                    .iter()
                    .take(limit.unwrap_or(50))
                    .map(|&r| graph.ids()[r])
                    .collect();
                let texts = self.commit_texts(&ids)?;
                let mailmap = self.mailmap();
                let commits: Vec<_> = ids
                    .iter()
                    .zip(&texts)
                    .map(|(id, t)| {
                        serde_json::json!({
                            "oid": id.to_string(),
                            "subject": String::from_utf8_lossy(t.subject()),
                            "author": String::from_utf8_lossy(&t.author(mailmap.as_deref()).name),
                            "time": t.author_time,
                        })
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "count": count, "commits": commits })),
                    ..Default::default()
                })
            }
            Command::DeleteUntracked { paths } => {
                let files = self.backend().files();
                for path in &paths {
                    files.remove(Root::Workdir, path)?;
                }
                ok()
            }
            Command::ConflictCount { path } => {
                let data = self
                    .backend()
                    .files()
                    .read(Root::Workdir, &path)?
                    .unwrap_or_default();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "count": crate::conflict::parse(&data).len() })),
                    ..Default::default()
                })
            }
            Command::ConflictFiles => {
                let status = self.last_status()?;
                Ok(Response {
                    ok: true,
                    info: Some(
                        serde_json::json!({ "files": self.conflict_files(&status.status)? }),
                    ),
                    ..Default::default()
                })
            }
            Command::ConfigList => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "entries": ops.config_list()? })),
                ..Default::default()
            }),
            Command::ConfigKeys => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "keys": ops.config_keys()? })),
                ..Default::default()
            }),
            Command::SigningKeys { format } => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "keys": ops.signing_keys(&format)? })),
                ..Default::default()
            }),
            Command::Programs { name } => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "programs": ops.programs(&name)? })),
                ..Default::default()
            }),
            Command::ListDirectory { path } => Ok(Response {
                ok: true,
                info: Some(serde_json::to_value(ops.list_directory(&path)?)?),
                ..Default::default()
            }),
            Command::ConfigSet {
                key,
                value,
                scope,
                values,
            } => {
                let global = match scope.as_deref() {
                    None | Some("local") => false,
                    Some("global") => true,
                    Some(other) => return Err(tr!("Unknown scope: {scope}", scope = other).into()),
                };
                match (values, global) {
                    (Some(values), _) => ops.config_set_all(&key, &values, global)?,
                    (None, false) => ops.config_set(&key, value.as_deref())?,
                    (None, true) => ops.config_set_global(&key, value.as_deref())?,
                }
                ok()
            }
            Command::SyncDefaults => {
                let config = ops.config_list()?;
                // 後のものほど優先（`git config --list` の順）。
                let value = |key: &str| {
                    config
                        .iter()
                        .rev()
                        .find(|e| e.key.eq_ignore_ascii_case(key))
                        .map(|e| e.value.to_ascii_lowercase())
                };
                // 今のブランチの branch.<name>.rebase が pull.rebase より先（git の pull と同じ）。ブランチの名前は
                // 大文字と小文字を区別する。
                let branch_rebase = ops
                    .head_state()?
                    .branch
                    .and_then(|b| b.strip_prefix("refs/heads/").map(str::to_owned))
                    .and_then(|b| {
                        config
                            .iter()
                            .rev()
                            .find(|e| {
                                e.key
                                    .get(..7)
                                    .is_some_and(|s| s.eq_ignore_ascii_case("branch."))
                                    && e.key
                                        .get(7..)
                                        .and_then(|s| s.strip_prefix(b.as_str()))
                                        .is_some_and(|s| s.eq_ignore_ascii_case(".rebase"))
                            })
                            .map(|e| e.value.to_ascii_lowercase())
                    });
                let rebase = branch_rebase.or_else(|| value("pull.rebase"));
                let ff = value("pull.ff");
                // どれも決めていなければ、git 2.33.1 から分かれたブランチの pull を断る（`builtin/pull.c` の
                // `show_advice_pull_non_ff`）。2.34.1 からは、手元だけが進んでいるときは断らない。
                let pull_refuses = if rebase.is_none() && ff.is_none() {
                    match ops.git_version()? {
                        Some(v) if v < (2, 33, 1) => None,
                        Some(v) if v < (2, 34, 1) => Some("notFastForward"),
                        _ => Some("diverged"),
                    }
                } else {
                    None
                };
                let pull = if ff.as_deref() == Some("only") {
                    "fastForwardOnly"
                } else if matches!(
                    rebase.as_deref(),
                    Some("true" | "yes" | "on" | "1" | "merges" | "interactive")
                ) {
                    "rebase"
                } else {
                    "merge"
                };
                let prune = matches!(
                    value("fetch.prune").as_deref(),
                    Some("true" | "yes" | "on" | "1")
                );
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "pull": pull,
                        "pullRefuses": pull_refuses,
                        "prune": prune,
                    })),
                    ..Default::default()
                })
            }
            Command::MachineGit => Ok(Response {
                ok: true,
                info: Some(serde_json::to_value(ops.machine_git()?)?),
                ..Default::default()
            }),
            Command::SetMachineGit { git } => {
                ops.set_machine_git(git.as_deref())?;
                Ok(Response {
                    ok: true,
                    ..Default::default()
                })
            }
            Command::GitVersion => {
                let version = ops.git_version()?;
                let rerere_cache = self
                    .backend()
                    .files()
                    .mode(Root::CommonDir, "rr-cache")?
                    .is_some();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "version": version.map(|(a, b, c)| format!("{a}.{b}.{c}")),
                        "updateRefs": version.is_none_or(|(a, b, _)| (a, b) >= (2, 38)),
                        "rerereCache": rerere_cache,
                    })),
                    ..Default::default()
                })
            }
            Command::RepositoryForm => Ok(Response {
                ok: true,
                info: Some(self.repository_form(ops)?),
                ..Default::default()
            }),
            Command::SparseCheckout { folders } => {
                self.set_sparse_folders(ops, &folders)?;
                ok()
            }
            Command::DisableSparseCheckout => {
                self.disable_sparse_checkout(ops)?;
                ok()
            }
            Command::TreeFolders { path, rev } => Ok(Response {
                ok: true,
                info: Some(self.tree_folders(ops, rev.as_deref().unwrap_or("HEAD"), &path)?),
                ..Default::default()
            }),
            Command::Maintenance { on } => {
                self.set_maintenance(ops, on)?;
                ok()
            }
            Command::IgnoreMatches { text } => {
                let lines: Vec<(usize, String)> = text
                    .lines()
                    .enumerate()
                    .map(|(i, l)| (i + 1, l.trim().to_owned()))
                    .filter(|(_, l)| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!'))
                    .take(40)
                    .collect();
                let patterns: Vec<String> = lines.iter().map(|(_, l)| l.clone()).collect();
                let matches = ops.ignore_matches(&patterns, 5)?;
                let lines: Vec<_> = lines
                    .iter()
                    .zip(matches)
                    .map(|((line, pattern), (examples, count))| {
                        serde_json::json!({"line": line, "pattern": pattern, "examples": examples, "count": count})
                    })
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "lines": lines })),
                    ..Default::default()
                })
            }
            Command::RemoteActivity => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "records": ops.activity()? })),
                ..Default::default()
            }),
            Command::CancelActivity { seq, remote } => {
                let cancelled = if remote {
                    ops.cancel_activity(seq)?
                } else {
                    amonhen_model::activity::cancel(seq)
                };
                if !cancelled {
                    return Err(tr!(
                        "Can’t cancel (it already finished, or it can’t be cancelled)"
                    )
                    .into());
                }
                Ok(Response {
                    ok: true,
                    ..Default::default()
                })
            }
            Command::SetHookEnabled { name, enabled } => {
                if !HOOK_NAMES.contains(&name.as_str()) {
                    return Err(tr!("Unknown hook: {name}", name = name).into());
                }
                if cfg!(not(unix)) && !enabled {
                    return Err(tr!(
                        "Hooks can’t be disabled on Windows (git runs them whatever their permissions)"
                    )
                    .into());
                }
                let (root, dir) = hooks_location(ops.config_list()?.as_slice());
                let files = self.backend().files();
                let path = format!("{dir}/{name}");
                let Some(mode) = (!dir.is_empty())
                    .then(|| files.mode(root, &path))
                    .transpose()?
                    .flatten()
                else {
                    return Err(tr!("There is no hook named {name}", name = name).into());
                };
                files.set_mode(
                    root,
                    &path,
                    if enabled { mode | 0o111 } else { mode & !0o111 },
                )?;
                ok()
            }
            Command::Hook { name } => {
                if !HOOK_NAMES.contains(&name.as_str()) {
                    return Err(tr!("Unknown hook: {name}", name = name).into());
                }
                let (root, dir) = hooks_location(ops.config_list()?.as_slice());
                let text = (!dir.is_empty())
                    .then(|| self.backend().files().read(root, &format!("{dir}/{name}")))
                    .transpose()?
                    .flatten()
                    .map(|b| String::from_utf8_lossy(&b).into_owned());
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "text": text })),
                    ..Default::default()
                })
            }
            Command::CreateHook { name } => {
                if !HOOK_NAMES.contains(&name.as_str()) {
                    return Err(tr!("Unknown hook: {name}", name = name).into());
                }
                let (root, dir) = hooks_location(ops.config_list()?.as_slice());
                if dir.is_empty() {
                    return Err(tr!("core.hooksPath is outside the working tree").into());
                }
                let files = self.backend().files();
                let path = format!("{dir}/{name}");
                if files.mode(root, &path)?.is_some() {
                    return Err(tr!("There is already a hook named {name}", name = name).into());
                }
                files.write(root, &path, b"#!/bin/sh\n\n")?;
                files.set_mode(root, &path, 0o755)?;
                ok()
            }
            Command::LfsTrack { pattern, track } => {
                ops.lfs_track(&pattern, track)?;
                ok()
            }
            Command::HooksAndLfs => {
                let config = ops.config_list()?;
                let value = |key: &str| {
                    config
                        .iter()
                        .rev()
                        .find(|e| e.key.eq_ignore_ascii_case(key))
                        .map(|e| e.value.clone())
                };
                let hooks_path = value("core.hooksPath");
                let (root, dir) = hooks_location(&config);
                let files = self.backend().files();
                let mut hooks = Vec::new();
                if !dir.is_empty() {
                    for name in HOOK_NAMES {
                        if let Some(mode) = files.mode(root, &format!("{dir}/{name}"))? {
                            hooks.push(
                                serde_json::json!({"name": name, "enabled": mode & 0o111 != 0}),
                            );
                        }
                    }
                }
                let attributes = files
                    .read(Root::Workdir, ".gitattributes")?
                    .unwrap_or_default();
                let lfs_patterns: Vec<String> = String::from_utf8_lossy(&attributes)
                    .lines()
                    .filter(|l| l.split_whitespace().any(|a| a == "filter=lfs"))
                    .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "hooksPath": hooks_path,
                        "hooks": hooks,
                        "lfs": value("filter.lfs.process").or(value("filter.lfs.smudge")).is_some(),
                        "lfsPatterns": lfs_patterns,
                        "lfsFiles": ops.lfs_file_count()?,
                    })),
                    ..Default::default()
                })
            }
            Command::WorkingStats => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "stats": ops.working_stats()? })),
                ..Default::default()
            }),
            Command::Remotes => Ok(Response {
                ok: true,
                info: Some(serde_json::json!({ "remotes": ops.remotes()? })),
                ..Default::default()
            }),
            Command::ChangeRemote { change } => {
                ops.change_remote(&change)?;
                ok()
            }
            Command::CheckRemote { url } => Ok(Response {
                ok: true,
                info: Some(serde_json::to_value(ops.check_remote(&url)?)?),
                ..Default::default()
            }),
            Command::Worktrees => {
                let paths = self.backend().paths();
                let list = crate::worktree::parse(&ops.worktree_list()?, paths.workdir.as_deref());
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "worktrees": list })),
                    ..Default::default()
                })
            }
            Command::WorktreeChanges { paths, fresh } => Ok(Response {
                ok: true,
                info: Some(
                    serde_json::json!({ "worktrees": ops.worktree_changes(&paths, fresh)? }),
                ),
                ..Default::default()
            }),
            Command::WorktreeAdd {
                path,
                start,
                new_branch,
            } => {
                ops.change_worktree(&amonhen_model::WorktreeChange::Add {
                    path,
                    start,
                    new_branch,
                })?;
                ok()
            }
            Command::WorktreeRemove { path, force } => {
                ops.change_worktree(&amonhen_model::WorktreeChange::Remove { path, force })?;
                ok()
            }
            Command::WorktreePrune => {
                ops.change_worktree(&amonhen_model::WorktreeChange::Prune)?;
                ok()
            }
            Command::WorktreeLock { path, reason } => {
                ops.change_worktree(&amonhen_model::WorktreeChange::Lock { path, reason })?;
                ok()
            }
            Command::WorktreeUnlock { path } => {
                ops.change_worktree(&amonhen_model::WorktreeChange::Unlock { path })?;
                ok()
            }
            Command::WorktreeMove { path, to } => {
                ops.change_worktree(&amonhen_model::WorktreeChange::Move { path, to })?;
                ok()
            }
            Command::WorktreeRepair { paths } => {
                ops.change_worktree(&amonhen_model::WorktreeChange::Repair { paths })?;
                ok()
            }
            Command::IgnoreFiles => {
                let files = self.backend().files();
                let read = |root, path| -> Result<String> {
                    Ok(files
                        .read(root, path)?
                        .map(|b| String::from_utf8_lossy(&b).into_owned())
                        .unwrap_or_default())
                };
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({
                        "gitignore": read(Root::Workdir, ".gitignore")?,
                        "exclude": read(Root::GitDir, "info/exclude")?,
                    })),
                    ..Default::default()
                })
            }
            Command::ExcludesFile => {
                let (path, text) = ops.excludes_file()?;
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "path": path, "text": text })),
                    ..Default::default()
                })
            }
            Command::SaveExcludesFile { text } => {
                ops.save_excludes_file(&text)?;
                ok()
            }
            Command::ProcessEnvironment => {
                let vars: Vec<_> = ops
                    .process_environment()?
                    .into_iter()
                    .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
                    .collect();
                Ok(Response {
                    ok: true,
                    info: Some(serde_json::json!({ "vars": vars })),
                    ..Default::default()
                })
            }
            Command::SaveIgnoreFiles { gitignore, exclude } => {
                let files = self.backend().files();
                files.write(Root::Workdir, ".gitignore", gitignore.as_bytes())?;
                files.write(Root::GitDir, "info/exclude", exclude.as_bytes())?;
                ok()
            }
            Command::CommitTemplate => {
                let text = self
                    .backend()
                    .files()
                    .read(Root::GitDir, COMMIT_TEMPLATE)?
                    .map(|b| String::from_utf8_lossy(&b).into_owned());
                Ok(Response {
                    ok: true,
                    info: Some(
                        serde_json::json!({ "text": text, "path": self.commit_template_path() }),
                    ),
                    ..Default::default()
                })
            }
            Command::SaveCommitTemplate { text } => {
                self.backend()
                    .files()
                    .write(Root::GitDir, COMMIT_TEMPLATE, text.as_bytes())?;
                ops.config_set("commit.template", Some(&self.commit_template_path()))?;
                ok()
            }
            Command::SaveMerge { path, text, stage } => {
                let files = self.backend().files();
                undo.record_files(ops, &format!("Merge {path}"), &[&path], || {
                    files.write(Root::Workdir, &path, text.as_bytes())
                })?;
                if stage {
                    self.stage(&[&path])?;
                }
                ok()
            }
            Command::Sequence => {
                // rebase なら何番目かも添える（"rebase (2/5)"）。
                let sequence = ops.sequence_in_progress()?;
                let progress = match sequence {
                    Some(amonhen_model::Sequence::Rebase | amonhen_model::Sequence::Am) => {
                        ops.rebase_progress()?
                    }
                    _ => None,
                };
                let text = sequence.map(|s| match (s, progress) {
                    (s, Some((current, total))) => {
                        format!("{} ({current}/{total})", s.command())
                    }
                    (s, None) => s.command().to_owned(),
                });
                let info = match sequence {
                    Some(s) => Some(self.sequence_info(s, progress)?),
                    None => None,
                };
                Ok(Response {
                    ok: true,
                    text,
                    info,
                    ..Default::default()
                })
            }
        }
    }
}

/// コミットのオブジェクトの `committer` の行のメールアドレス。
fn committer_email(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.lines()
        .take_while(|l| !l.is_empty())
        .find_map(|l| l.strip_prefix("committer "))
        .and_then(|l| l.split_once('<'))
        .and_then(|(_, rest)| rest.split_once('>'))
        .map_or_else(String::new, |(email, _)| email.to_owned())
}

/// 署名の検証の結果を JSON に（`status` は `%G?` の 1 文字）。
fn signature_json(s: &amonhen_model::SignatureCheck) -> serde_json::Value {
    serde_json::json!({
        "status": s.status.to_string(),
        "signer": s.signer,
        "key": s.key,
        "fingerprint": s.fingerprint,
        "trust": s.trust,
        "emails": s.emails.iter().map(|e| serde_json::json!({
            "email": e.email,
            "validity": e.validity.to_string(),
        })).collect::<Vec<_>>(),
    })
}

/// 注釈付きのタグのオブジェクト（`git cat-file tag` の中身）を読んだもの。
struct TagObject {
    /// オブジェクトの ID（中身から求める。merge したタグはリポジトリにないことがある）。
    id: Oid,
    raw: Vec<u8>,
    name: String,
    tagger: String,
    tagger_email: String,
    time: i64,
    tz: String,
    /// メッセージの 1 行目と、署名を除いたメッセージ。
    subject: String,
    message: String,
    /// 署名の種類（gpg・ssh・x509）。署名がなければ `None`。
    signed: Option<&'static str>,
}

impl TagObject {
    fn read(raw: &[u8]) -> Self {
        let mut hasher = sha1_smol::Sha1::new();
        hasher.update(format!("tag {}\0", raw.len()).as_bytes());
        hasher.update(raw);
        let id = Oid(hasher.digest().bytes());
        let text = String::from_utf8_lossy(raw);
        let (header, body) = text.split_once("\n\n").unwrap_or((&text, ""));
        let field = |key: &str| {
            header
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .unwrap_or_default()
                .to_owned()
        };
        // "Name <email> 1700000000 +0900"
        let tagger = field("tagger ");
        let (name, rest) = tagger.split_once(" <").unwrap_or((&tagger, ""));
        let (email, rest) = rest.split_once("> ").unwrap_or((rest, ""));
        let mut parts = rest.split(' ');
        let time = parts.next().and_then(|t| t.parse().ok()).unwrap_or(0);
        let tz = parts.next().unwrap_or_default().to_owned();
        let signed = match amonhen_model::CommitText::tag_signature_kind(raw) {
            amonhen_model::CommitText::SIGNED_GPG => Some("gpg"),
            amonhen_model::CommitText::SIGNED_SSH => Some("ssh"),
            amonhen_model::CommitText::SIGNED_X509 => Some("x509"),
            _ => None,
        };
        // 署名はメッセージの後ろ（最後の `-----BEGIN ` の行から）。
        let message = match signed {
            Some(_) => body
                .rfind("-----BEGIN ")
                .filter(|&i| i == 0 || body.as_bytes()[i - 1] == b'\n')
                .map_or(body, |i| &body[..i]),
            None => body,
        };
        let subject = message
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .trim()
            .to_owned();
        TagObject {
            id,
            raw: raw.to_vec(),
            name: field("tag "),
            tagger: name.to_owned(),
            tagger_email: email.to_owned(),
            time,
            tz,
            subject,
            message: message.trim_end().to_owned(),
            signed,
        }
    }
}

/// コミットのヘッダーの `mergetag`（merge したタグのオブジェクトそのもの）を取り出す。値は複数行で、続きの
/// 行は空白 1 つで始まる。
fn merged_tags(raw: &[u8]) -> Vec<Vec<u8>> {
    let header_end = raw
        .windows(2)
        .position(|w| w == b"\n\n")
        .unwrap_or(raw.len());
    let mut tags: Vec<Vec<u8>> = Vec::new();
    let mut in_tag = false;
    for line in raw[..header_end].split(|&b| b == b'\n') {
        if let Some(first) = line.strip_prefix(b"mergetag ") {
            tags.push(first.to_vec());
            tags.last_mut().unwrap().push(b'\n');
            in_tag = true;
        } else if let Some(rest) = line.strip_prefix(b" ")
            && in_tag
        {
            let tag = tags.last_mut().unwrap();
            tag.extend_from_slice(rest);
            tag.push(b'\n');
        } else {
            in_tag = false;
        }
    }
    tags
}

/// git の config の `[remote "origin"]` の `url`。
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == r#"[remote"origin"]"#;
        } else if in_origin
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

/// メッセージの最後の段落にあるトレーラー（`Signed-off-by: 名前 <メール>` など）。段落のすべての行が
/// `キー: 値`（か、その続きの字下げした行）のときだけトレーラーとみなす。
fn trailers(message: &str) -> Vec<(String, String)> {
    // 件名だけの段落はトレーラーにしない（`Fix: ...` のような件名がある）。
    let message = message.trim_end();
    let Some((_, last)) = message.rsplit_once("\n\n") else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for line in last.lines() {
        if line.starts_with([' ', '\t']) {
            match out.last_mut() {
                Some((_, value)) => {
                    value.push(' ');
                    value.push_str(line.trim());
                }
                None => return Vec::new(),
            }
        } else if let Some((key, value)) = trailer(line) {
            out.push((key.to_owned(), value.to_owned()));
        } else {
            return Vec::new();
        }
    }
    out
}

fn trailer(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(": ")?;
    let valid = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then_some((key, value.trim()))
}

/// git が呼ぶ hook の名前（`git help hooks` の順）。
/// フックの置き場所: `core.hooksPath` が作業ツリーの中（相対）ならそこ、絶対パスなら読めない（空）、なければ
/// `.git/hooks`。
fn hooks_location(config: &[amonhen_model::ConfigEntry]) -> (Root, String) {
    let hooks_path = config
        .iter()
        .rev()
        .find(|e| e.key.eq_ignore_ascii_case("core.hooksPath"))
        .map(|e| e.value.clone());
    match &hooks_path {
        Some(p) if !p.starts_with('/') && !p.starts_with('~') => {
            (Root::Workdir, p.trim_end_matches('/').to_owned())
        }
        Some(_) => (Root::GitDir, String::new()),
        None => (Root::GitDir, "hooks".to_owned()),
    }
}

const HOOK_NAMES: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する
mod tests {
    use super::*;
    use amonhen_backend_gix::GixBackend;
    use amonhen_projection::FrontendProfile;
    use std::process::Command as Process;

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        amonhen_model::testing::isolate_from_user_settings();
        let out = Process::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        // テスト用のリポジトリは、CI の環境（利用者名がない・Windows の autocrlf）や、利用者の global の設定（コミットの
        // 署名。テストするコードの git は global を読むので、並べて走らせると gpg が失敗することがある）に左右されないようにする。
        if args.first() == Some(&"init") {
            for (key, value) in [
                ("user.name", "a"),
                ("user.email", "a@example.com"),
                ("core.autocrlf", "false"),
                ("commit.gpgsign", "false"),
                ("tag.gpgsign", "false"),
            ] {
                git(dir, &["config", key, value]);
            }
        }
        out
    }

    /// 標準入力を渡して git を実行し、標準出力を返す（前後の空白を除く）。
    fn git_with_input(dir: &std::path::Path, args: &[&str], input: &str) -> String {
        use std::io::Write;
        let mut child = Process::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn setup() -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q"]);
        git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(dir, &["config", "user.name", "a"]);
        git(dir, &["config", "user.email", "a@example.com"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        (tmp, repo)
    }

    fn run(repo: &Repository, json: &str) -> serde_json::Value {
        serde_json::from_str(&repo.run_json(json)).unwrap()
    }

    /// 何を渡されても G を返し、呼ばれた回数を数える検証。
    struct AlwaysGood(std::sync::atomic::AtomicUsize);

    impl amonhen_model::SignatureVerifier for AlwaysGood {
        fn verify(&self, commits: &[(Oid, &[u8])]) -> Result<Vec<amonhen_model::SignatureCheck>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(vec![
                amonhen_model::SignatureCheck {
                    status: 'G',
                    ..Default::default()
                };
                commits.len()
            ])
        }
        fn details(&self, _: &Oid, _: &[u8]) -> Result<Option<amonhen_model::SignatureCheck>> {
            Ok(Some(amonhen_model::SignatureCheck {
                status: 'G',
                ..Default::default()
            }))
        }
        fn verify_tags(&self, tags: &[(Oid, &[u8])]) -> Result<Vec<amonhen_model::SignatureCheck>> {
            self.verify(tags)
        }
    }

    /// タグの署名: 署名のあるタグだけを検証し、結果はタグのオブジェクトの ID で覚える（同じタグを merge した
    /// マージコミットの mergetag とも分け合う）。軽量タグと署名のないタグには結果がない。
    #[test]
    fn verifies_signed_tags_and_merged_tags_once() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let head = git(dir, &["rev-parse", "HEAD"]);
        git(dir, &["tag", "light"]);
        git(dir, &["tag", "-a", "-m", "Plain release", "plain"]);
        let signed = format!(
            "object {head}\ntype commit\ntag signed\ntagger T <t@example.com> 5 +0900\n\nSigned release\n\nMore words\n-----BEGIN PGP SIGNATURE-----\nabc\n-----END PGP SIGNATURE-----\n"
        );
        let tag = git_with_input(dir, &["hash-object", "-t", "tag", "-w", "--stdin"], &signed);
        git(dir, &["update-ref", "refs/tags/signed", &tag]);
        let indented = signed.trim_end().replace('\n', "\n ");
        let merge = format!(
            "tree {}\nparent {head}\nparent {head}\nauthor a <a@example.com> 6 +0000\ncommitter a <a@example.com> 6 +0000\nmergetag {indented}\n\nMerge tag 'signed'\n",
            git(dir, &["rev-parse", "HEAD^{tree}"])
        );
        let merge = git_with_input(
            dir,
            &["hash-object", "-t", "commit", "-w", "--stdin"],
            &merge,
        );
        repo.load_refs().unwrap();
        let local = std::sync::Arc::new(AlwaysGood(Default::default()));
        repo.set_signature_verifier(Some(local.clone()));

        let reply = run(
            &repo,
            r#"{"op":"tagSignatures","names":["signed","plain","light","missing"]}"#,
        );
        let tags = &reply["info"]["tags"];
        assert_eq!(tags[0]["id"], tag.as_str());
        assert_eq!(tags[0]["signed"], "gpg");
        assert_eq!(tags[0]["taggerEmail"], "t@example.com");
        assert_eq!(tags[0]["message"], "Signed release\n\nMore words");
        assert_eq!(tags[0]["signature"]["status"], "G");
        assert!(tags[1]["id"].is_string());
        assert!(tags[1]["signed"].is_null() && tags[1]["signature"].is_null());
        assert!(tags[2]["id"].is_null());
        assert!(tags[3]["id"].is_null());
        assert_eq!(local.0.load(std::sync::atomic::Ordering::Relaxed), 1);

        let extras = run(&repo, &format!(r#"{{"op":"commitExtras","oid":"{head}"}}"#));
        let objects = extras["info"]["tagObjects"].as_array().unwrap();
        let by_name = |n: &str| objects.iter().find(|t| t["name"] == n).unwrap();
        assert_eq!(by_name("signed")["subject"], "Signed release");
        assert_eq!(by_name("signed")["message"], "Signed release\n\nMore words");
        assert_eq!(by_name("signed")["time"], 5);
        assert_eq!(by_name("signed")["signature"]["status"], "G");
        assert_eq!(by_name("plain")["tagger"], "a");
        assert!(by_name("plain")["signature"].is_null());
        assert!(by_name("light")["id"].is_null());

        let info = run(&repo, &format!(r#"{{"op":"commitInfo","oid":"{merge}"}}"#));
        assert_eq!(info["info"]["mergedTags"][0]["id"], tag.as_str());
        assert_eq!(info["info"]["mergedTags"][0]["signed"], "gpg");
        assert_eq!(
            info["info"]["mergedTags"][0]["message"],
            "Signed release\n\nMore words"
        );
        let extras = run(
            &repo,
            &format!(r#"{{"op":"commitExtras","oid":"{merge}"}}"#),
        );
        assert_eq!(extras["info"]["mergedTagSignatures"][0]["status"], "G");
        // 同じタグなので、検証し直さない。
        assert_eq!(local.0.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn reports_merged_commits_head_distance_and_references() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let first = git(dir, &["rev-parse", "HEAD"]);
        git(dir, &["switch", "-q", "-c", "topic"]);
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "topic work"]);
        git(dir, &["switch", "-q", "main"]);
        git(
            dir,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                "Merge branch 'topic'",
                "topic",
            ],
        );
        let merge = git(dir, &["rev-parse", "HEAD"]);
        // 短い ID は、数字だけだと ID と見なされない（日付や番号と区別がつかない）。英字が入る長さまで取る。
        let short = (10..=40)
            .map(|n| &first[..n])
            .find(|s| s.bytes().any(|b| b.is_ascii_alphabetic()))
            .unwrap();
        git(
            dir,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("Follow up\n\nFixes: {short} (\"first\")"),
            ],
        );
        let follow = git(dir, &["rev-parse", "HEAD"]);
        repo.load_history().unwrap();
        repo.load_refs().unwrap();
        let extras = |oid: &str| {
            run(&repo, &format!(r#"{{"op":"commitExtras","oid":"{oid}"}}"#))["info"].clone()
        };

        let m = extras(&merge);
        assert_eq!(m["mergedCount"], 1);
        assert_eq!(m["merged"][0]["subject"], "topic work");
        assert_eq!(m["headDistance"], 1, "HEAD の 1 つ前");
        assert_eq!(m["headRev"], "HEAD~1");
        assert_eq!(m["localBranches"], serde_json::json!(["main"]));
        assert_eq!(m["remoteBranches"], serde_json::json!([]));
        let f = extras(&follow);
        assert_eq!(f["headDistance"], 0);
        assert_eq!(f["references"][short]["oid"], first.as_str());
        assert_eq!(f["references"][short]["subject"], "first");
        assert_eq!(extras(&first)["mergedCount"], 0);
    }

    fn statuses(repo: &Repository, id: Oid) -> Vec<char> {
        repo.verify_signatures(&[id])
            .unwrap()
            .iter()
            .map(|c| c.status)
            .collect()
    }

    #[test]
    fn reads_the_history_again_when_head_has_moved_since() {
        // 編集できるコミットを読んで History を覚えたあとで、ほかのツールがコミットしても、新しい HEAD から読む。
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let list = |through: bool| {
            run(
                &repo,
                &serde_json::json!({"op": "editableCommits", "throughMerges": through}).to_string(),
            )
        };
        assert_eq!(list(true)["ok"], true);
        std::fs::write(dir.join("later.txt"), "x").unwrap();
        git(dir, &["add", "later.txt"]);
        git(dir, &["commit", "-q", "-m", "later"]);
        let head = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        for through in [true, false] {
            let listed = list(through);
            assert_eq!(listed["ok"], true, "{listed}");
            assert!(
                listed["text"].as_str().unwrap().starts_with(head.trim()),
                "{listed}"
            );
        }
    }

    #[test]
    fn adds_lists_and_removes_worktrees() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let wt = dir.join(".worktrees/topic");
        // JSON は json! で作る（Windows のパスの \ を文字列の中に書けるように）。
        let command = |value: serde_json::Value| value.to_string();
        let added = run(
            &repo,
            &command(serde_json::json!({"op": "worktreeAdd", "path": wt, "newBranch": "topic"})),
        );
        assert_eq!(added["ok"], true, "{added}");
        let list = run(&repo, r#"{"op":"worktrees"}"#);
        let list = list["info"]["worktrees"].as_array().unwrap().clone();
        assert_eq!(list.len(), 2);
        assert!(list[0]["main"] == true && list[0]["current"] == true);
        assert_eq!(list[1]["branch"], "topic");
        assert_eq!(list[1]["current"], false);
        // 変更が残っていれば、force なしでは消せない（git が断る）。
        std::fs::write(wt.join("new.txt"), "x").unwrap();
        // ほかの worktree の変更の数は、開いたリポジトリから読める（一覧にないパスは読まない）。
        let path = list[1]["path"].as_str().unwrap();
        let changes = run(
            &repo,
            &command(serde_json::json!({"op": "worktreeChanges", "paths": [path, "/nowhere"]})),
        );
        let changes = changes["info"]["worktrees"].as_array().unwrap().clone();
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0]["path"], path);
        assert_eq!(changes[0]["count"], 1);
        // HEAD と index は見張るので、stage すれば（fresh なしで）数え直される。
        std::fs::write(wt.join("more.txt"), "y").unwrap();
        git(&wt, &["add", "more.txt"]);
        let started = std::time::Instant::now();
        loop {
            let changes = run(
                &repo,
                &command(serde_json::json!({"op": "worktreeChanges", "paths": [path]})),
            );
            if changes["info"]["worktrees"][0]["count"] == 2 {
                break;
            }
            assert!(started.elapsed().as_secs() < 5, "{changes}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let refused = repo.run_json(&command(
            serde_json::json!({"op": "worktreeRemove", "path": wt}),
        ));
        assert!(refused.contains(r#""ok":false"#), "{refused}");
        let removed = run(
            &repo,
            &command(serde_json::json!({"op": "worktreeRemove", "path": wt, "force": true})),
        );
        assert_eq!(removed["ok"], true, "{removed}");
        let list = run(&repo, r#"{"op":"worktrees"}"#);
        assert_eq!(list["info"]["worktrees"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn removes_a_submodule_whose_folder_is_still_in_use_for_a_moment() {
        // Windows では、ほかのプロセスが中のフォルダーを使っている間は git rm が消せない。少しして手放されれば、消し切ること。
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let lib = tempfile::tempdir().unwrap();
        git(lib.path(), &["init", "-q"]);
        git(lib.path(), &["config", "user.name", "a"]);
        git(lib.path(), &["config", "user.email", "a@example.com"]);
        std::fs::write(lib.path().join("l.txt"), "l\n").unwrap();
        git(lib.path(), &["add", "."]);
        git(lib.path(), &["commit", "-q", "-m", "lib"]);
        let url = lib.path().to_str().unwrap().replace('\\', "/");
        let change = |json: serde_json::Value| {
            run(
                &repo,
                &serde_json::json!({ "op": "changeSubmodule", "change": json }).to_string(),
            )
        };
        let r =
            change(serde_json::json!({ "kind": "add", "url": url, "path": "lib", "branch": null }));
        assert_eq!(r["ok"], true, "{r}");
        git(dir, &["commit", "-q", "-m", "add lib"]);
        let mut holder = if cfg!(windows) {
            let mut c = amonhen_model::process::command("powershell");
            c.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
            c
        } else {
            let mut c = amonhen_model::process::command("sleep");
            c.arg("30");
            c
        };
        // deinit と rm が動く間は掴んでいて、試し直す 2 秒のうちに手放す
        let holder = std::sync::Mutex::new(holder.current_dir(dir.join("lib")).spawn().unwrap());
        let removed = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let mut holder = holder.lock().unwrap();
                let _ = holder.kill();
                let _ = holder.wait();
            });
            change(serde_json::json!({ "kind": "remove", "path": "lib", "deleteGitDir": true }))
        });
        assert_eq!(removed["ok"], true, "{removed}");
        assert!(!dir.join("lib").exists());
    }

    #[test]
    fn removes_a_worktree_whose_folder_is_still_in_use_for_a_moment() {
        // Windows では、ほかのプロセスが作業ディレクトリにしている間はフォルダーを消せず、git は登録だけ外して
        // フォルダーを残す。少しして手放されれば、消し切ること。
        let (tmp, repo) = setup();
        let wt = tmp.path().join(".worktrees/busy");
        let command = |value: serde_json::Value| value.to_string();
        let added = run(
            &repo,
            &command(serde_json::json!({"op": "worktreeAdd", "path": wt, "newBranch": "busy"})),
        );
        assert_eq!(added["ok"], true, "{added}");
        // 掴み続けるプロセスを作り、消し始めてから少しして終わらせる（起動にかかる時間に左右されないように）
        let mut holder = if cfg!(windows) {
            let mut c = amonhen_model::process::command("powershell");
            c.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
            c
        } else {
            let mut c = amonhen_model::process::command("sleep");
            c.arg("30");
            c
        };
        let holder = std::sync::Mutex::new(holder.current_dir(&wt).spawn().unwrap());
        let removed = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let mut holder = holder.lock().unwrap();
                let _ = holder.kill();
                let _ = holder.wait();
            });
            run(
                &repo,
                &command(serde_json::json!({"op": "worktreeRemove", "path": wt, "force": true})),
            )
        });
        assert_eq!(removed["ok"], true, "{removed}");
        assert!(!wt.exists());
    }

    #[test]
    fn lists_commits_on_one_side_only() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        for name in ["b", "c"] {
            std::fs::write(dir.join(name), "x\n").unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", &format!("add {name}")]);
        }
        let between = run(&repo, r#"{"op":"between","base":"main","tip":"topic"}"#);
        assert_eq!(between["info"]["count"], 2);
        // 新しい順。
        assert_eq!(between["info"]["commits"][0]["subject"], "add c");
        assert_eq!(between["info"]["commits"][1]["subject"], "add b");
        let limited = run(
            &repo,
            r#"{"op":"between","base":"main","tip":"topic","limit":1}"#,
        );
        assert_eq!(limited["info"]["count"], 2);
        assert_eq!(limited["info"]["commits"].as_array().unwrap().len(), 1);
        let back = run(&repo, r#"{"op":"between","base":"topic","tip":"main"}"#);
        assert_eq!(back["info"]["count"], 0);
    }

    #[test]
    fn elides_the_middle_of_long_runs() {
        let ids: Vec<Oid> = (0..10)
            .map(|i| Oid::from_hex(&format!("{i:040}")).unwrap())
            .collect();
        let (shown, more, at) = super::elide(&ids);
        assert_eq!(shown, [&ids[..3], &ids[8..]].concat());
        assert_eq!((more, at), (5, 3));
        // 6 つまでは省かない（1 つだけ省くなら、その 1 つを描く）。
        let (shown, more, at) = super::elide(&ids[..6]);
        assert_eq!((shown.len(), more, at), (6, 0, 6));
    }

    #[test]
    fn pull_merge_subjects_match_git() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let remote = tempfile::tempdir().unwrap();
        let origin = remote.path().join("origin.git");
        std::fs::create_dir(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare"]);
        git(&origin, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(dir, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(dir, &["push", "-q", "-u", "origin", "main"]);
        let other = remote.path().join("other");
        git(
            remote.path(),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(&other, &["config", "user.name", "b"]);
        git(&other, &["config", "user.email", "b@example.com"]);
        git(&other, &["config", "commit.gpgsign", "false"]);
        std::fs::write(other.join("b"), "b\n").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "upstream"]);
        git(&other, &["push", "-q", "origin", "main"]);
        // 上流が進む前に作った、origin/main を上流にするもう 1 つのブランチ（件名に「into work」が付く）。
        git(
            dir,
            &["switch", "-q", "-c", "work", "--track", "origin/main"],
        );
        git(dir, &["fetch", "-q"]);
        for branch in ["main", "work"] {
            git(dir, &["switch", "-q", branch]);
            std::fs::write(dir.join(branch), "x\n").unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", branch]);
            let preview = run(
                &repo,
                r#"{"op":"mergePreview","theirs":"origin/main","ffSetting":"pull.ff"}"#,
            );
            let subject = preview["info"]["drawing"]["subject"]
                .as_str()
                .unwrap()
                .to_owned();
            git(dir, &["-c", "pull.rebase=false", "pull", "-q"]);
            assert_eq!(subject, git(dir, &["log", "-1", "--format=%s"]));
        }
    }

    #[test]
    fn fetched_urls_are_anonymized_and_trimmed() {
        use super::fetched_url;
        assert_eq!(
            fetched_url("https://u:p@example.com/a/b.git"),
            "https://example.com/a/b"
        );
        assert_eq!(
            fetched_url("https://example.com/a/b/"),
            "https://example.com/a/b"
        );
        assert_eq!(fetched_url("git@example.com:a/b.git"), "example.com:a/b");
        assert_eq!(
            fetched_url("ssh://git@example.com/a/b.git/"),
            "ssh://example.com/a/b"
        );
        assert_eq!(fetched_url("/srv/git/a@b.git"), "/srv/git/a@b");
        assert_eq!(
            fetched_url("https://example.com/a@b/c"),
            "https://example.com/a@b/c"
        );
    }

    #[test]
    fn previews_merges_and_picks() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        for name in ["b", "c"] {
            std::fs::write(dir.join(name), format!("{name}\n")).unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", &format!("add {name}")]);
        }
        git(dir, &["switch", "-q", "main"]);
        std::fs::write(dir.join("d"), "d\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "add d"]);
        let merge = run(&repo, r#"{"op":"mergePreview","theirs":"topic"}"#);
        let info = &merge["info"];
        assert_eq!(info["fastForward"], false);
        assert_eq!(info["drawing"]["ours"][0]["subject"], "add d");
        assert_eq!(info["drawing"]["theirs"][0]["subject"], "add c");
        assert_eq!(info["drawing"]["base"]["subject"], "first");
        assert_eq!(info["drawing"]["subject"], "Merge branch 'topic'");
        // 取り込むものがない・fast-forward で済む。
        let none = run(&repo, r#"{"op":"mergePreview","theirs":"main~1"}"#);
        assert_eq!(none["info"]["upToDate"], true);
        let ff = run(
            &repo,
            r#"{"op":"mergePreview","ours":"main~1","theirs":"main"}"#,
        );
        assert_eq!(ff["info"]["fastForward"], true);
        assert_eq!(ff["info"]["move"]["gainedCount"], 1);
        // cherry-pick・revert で作るコミット（新しい順）。
        let pick = run(
            &repo,
            r#"{"op":"pickPreview","oids":["topic~1","topic"],"revert":true}"#,
        );
        let d = &pick["info"]["drawing"];
        assert_eq!(d["head"][0]["subject"], "add d");
        assert_eq!(d["picks"][0]["subject"], "Revert \"add c\"");
        assert_eq!(d["picks"][1]["subject"], "Revert \"add b\"");
        // パッチの取り込み（件名だけ。新しい順）。
        let am = run(&repo, r#"{"op":"pickPreview","subjects":["one","two"]}"#);
        assert_eq!(am["info"]["drawing"]["picks"][0]["subject"], "two");
        // topic と main~1 を octopus で（main~1 はもう取り込み済み）。
        let octopus = run(
            &repo,
            r#"{"op":"octopusPreview","theirs":["topic","main~1"]}"#,
        );
        let d = &octopus["info"]["drawing"];
        assert_eq!(d["ours"][0]["subject"], "add d", "{octopus}");
        assert_eq!(d["sides"][0]["tip"]["subject"], "add c");
        assert_eq!(d["sides"][0]["more"], 1);
        assert!(d["sides"][1]["tip"].is_null());
        assert_eq!(d["base"]["subject"], "first");
        let none = run(&repo, r#"{"op":"octopusPreview","theirs":["main~1"]}"#);
        assert_eq!(none["info"]["upToDate"], true);
        // topic を消すと、main にない 2 つが外れる。
        let delete = run(
            &repo,
            r#"{"op":"deletePreview","branch":"topic","against":"main"}"#,
        );
        assert_eq!(delete["info"]["lostCount"], 2);
        assert_eq!(delete["info"]["gainedCount"], 0);
        assert_eq!(delete["info"]["drawing"]["base"]["subject"], "first");
    }

    #[test]
    fn previews_moving_to_a_commit_no_ref_reaches() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("d"), "d\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "add d"]);
        let gone = git(dir, &["rev-parse", "main"]);
        git(dir, &["reset", "-q", "--hard", "HEAD~1"]);
        // git reset が残す ORIG_HEAD から辿れないように。
        git(dir, &["update-ref", "-d", "ORIG_HEAD"]);
        // Undo で戻る、どこからも辿れなくなったコミット: 親を辿って History に合流するまでを足し、印を付ける。
        let back = run(
            &repo,
            &serde_json::json!({"op": "movePreview", "from": "main", "to": gone}).to_string(),
        );
        assert_eq!(back["info"]["gainedCount"], 1, "{back}");
        assert_eq!(back["info"]["drawing"]["gained"][0]["subject"], "add d");
        assert_eq!(back["info"]["drawing"]["gained"][0]["hidden"], true);
    }

    #[test]
    fn previews_moving_a_ref() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        for name in ["b", "c"] {
            std::fs::write(dir.join(name), format!("{name}\n")).unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", &format!("add {name}")]);
        }
        git(dir, &["switch", "-q", "main"]);
        std::fs::write(dir.join("d"), "d\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "add d"]);
        git(
            dir,
            &["remote", "add", "origin", "https://example.invalid/x.git"],
        );
        git(dir, &["update-ref", "refs/remotes/origin/main", "main"]);
        git(
            dir,
            &["branch", "-q", "--set-upstream-to=origin/main", "main"],
        );
        // main を topic へ（main の 1 つが外れ、topic の 2 つが増える）。
        let side = run(&repo, r#"{"op":"movePreview","from":"main","to":"topic"}"#);
        let info = &side["info"];
        assert_eq!(info["gainedCount"], 2);
        assert_eq!(info["lostCount"], 1);
        assert_eq!(info["files"], 3);
        assert_eq!(info["drawing"]["gained"][0]["subject"], "add c");
        assert_eq!(info["drawing"]["lost"][0]["subject"], "add d");
        assert_eq!(info["drawing"]["base"]["subject"], "first");
        // 戻すだけ（増えるものはなく、分かれたところは行き先）。
        let back = run(
            &repo,
            r#"{"op":"movePreview","from":"topic","to":"topic~2"}"#,
        );
        assert_eq!(back["info"]["gainedCount"], 0);
        assert_eq!(back["info"]["lostCount"], 2);
        assert_eq!(back["info"]["drawing"]["base"]["subject"], "first");
        // 上流が外れるコミットを持っていれば数える。
        // 上流の設定を読むよう、開き直す。
        let reopened = Repository::new(GixBackend::open(dir).unwrap());
        let reset = run(
            &reopened,
            r#"{"op":"movePreview","from":"main","to":"main~1","branch":"main"}"#,
        );
        assert_eq!(reset["info"]["upstream"], "origin/main");
        assert_eq!(reset["info"]["upstreamHas"], 1);
        // 同じところなら絵はない。
        let same = run(&repo, r#"{"op":"movePreview","from":"main","to":"main"}"#);
        assert!(same["info"]["drawing"].is_null());
    }

    #[test]
    fn predicts_merge_conflicts_without_touching_the_worktree() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        std::fs::write(dir.join("a.txt"), "1\ntopic\n3\n").unwrap();
        git(dir, &["commit", "-q", "-am", "topic"]);
        git(dir, &["switch", "-q", "main"]);
        std::fs::write(dir.join("a.txt"), "1\nmain\n3\n").unwrap();
        git(dir, &["commit", "-q", "-am", "main"]);
        let conflicts = run(&repo, r#"{"op":"predictMerge","theirs":"topic"}"#);
        let clean = run(
            &repo,
            r#"{"op":"predictMerge","ours":"topic","theirs":"main~1"}"#,
        );
        assert_eq!(conflicts["ok"], true, "{conflicts}");
        if amonhen_backend_cli::git_version().is_none_or(|(a, b, _)| (a, b) >= (2, 38)) {
            assert_eq!(conflicts["info"]["conflicts"], serde_json::json!(["a.txt"]));
            assert_eq!(clean["info"]["conflicts"], serde_json::json!([]));
        } else {
            // 古い git では調べられない（わからない）。
            assert!(conflicts["info"]["conflicts"].is_null(), "{conflicts}");
            assert!(clean["info"]["conflicts"].is_null(), "{clean}");
        }
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nmain\n3\n"
        );
        assert_eq!(
            run(&repo, r#"{"op":"sequence"}"#)["text"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn does_not_carry_signature_results_over_when_the_checker_changes() {
        let (tmp, repo) = setup();
        let head = Oid::from_hex(&git(tmp.path(), &["rev-parse", "HEAD"])).unwrap();
        // リポジトリ側の git では、署名がないので N。
        assert_eq!(statuses(&repo, head), ['N']);
        // 手元で検証するものに替えると、覚えていた N は使わずに検証し直す。2 回目は覚えている。
        let local = std::sync::Arc::new(AlwaysGood(Default::default()));
        repo.set_signature_verifier(Some(local.clone()));
        assert_eq!(statuses(&repo, head), ['G']);
        assert_eq!(statuses(&repo, head), ['G']);
        assert_eq!(local.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        let extras = run(&repo, &format!(r#"{{"op":"commitExtras","oid":"{head}"}}"#));
        assert_eq!(extras["info"]["signature"]["status"], "G");
        // 戻すと、手元の G は引き継がない。
        let before = repo.signature_generation();
        repo.set_signature_verifier(None);
        assert_eq!(statuses(&repo, head), ['N']);
        // 替える前に始めた検証の結果が後から届いても覚えない。
        repo.remember_signatures(
            before,
            [(
                head,
                amonhen_model::SignatureCheck {
                    status: 'G',
                    ..Default::default()
                },
            )],
        );
        assert_eq!(statuses(&repo, head), ['N']);
    }

    #[test]
    fn reads_merged_tags_from_the_header() {
        let raw = b"tree t\nparent a\nparent b\nauthor x <x@y> 1 +0000\nmergetag object b\n type commit\n tag v6.5-fixes\n tagger Greg <g@k> 2 +0200\n \n USB fixes for 6.5\n \n -----BEGIN PGP SIGNATURE-----\n abc\n -----END PGP SIGNATURE-----\ncommitter x <x@y> 3 +0000\n\nMerge tag\n";
        let tags = merged_tags(raw);
        assert_eq!(tags.len(), 1);
        // mergetag の値はタグのオブジェクトそのもの（ID も元のタグと同じになる）。
        assert_eq!(
            tags[0],
            b"object b\ntype commit\ntag v6.5-fixes\ntagger Greg <g@k> 2 +0200\n\nUSB fixes for 6.5\n\n-----BEGIN PGP SIGNATURE-----\nabc\n-----END PGP SIGNATURE-----\n"
        );
        let tag = TagObject::read(&tags[0]);
        assert_eq!(
            (
                tag.name.as_str(),
                tag.tagger.as_str(),
                tag.tagger_email.as_str()
            ),
            ("v6.5-fixes", "Greg", "g@k")
        );
        assert_eq!((tag.time, tag.tz.as_str()), (2, "+0200"));
        assert_eq!(tag.subject, "USB fixes for 6.5");
        assert_eq!(tag.message, "USB fixes for 6.5");
        assert_eq!(tag.signed, Some("gpg"));
        assert!(merged_tags(b"tree t\nauthor x <x@y> 1 +0000\n\nmergetag x\n").is_empty());
    }

    /// タグの ID は中身から求める（`git hash-object -t tag` と同じ）。
    #[test]
    fn computes_the_id_of_a_tag_object() {
        let raw = b"object 26e76af3ea3bc42b40e826a4619f1bb69b865cdd\ntype commit\ntag v0\ntagger T <t@example.com> 1 +0000\n\nplain\n";
        let tag = TagObject::read(raw);
        let tmp = tempfile::tempdir().unwrap();
        let git = amonhen_backend_cli::Git::new(tmp.path());
        git.write(&["init", "-q"]).unwrap();
        let id = git
            .write_with_input(&["hash-object", "-t", "tag", "--stdin"], raw)
            .unwrap();
        assert_eq!(tag.id.to_string(), String::from_utf8(id).unwrap().trim());
        assert_eq!((tag.signed, tag.message.as_str()), (None, "plain"));
    }

    #[test]
    fn reads_the_origin_url_from_the_config() {
        let config = "[core]\n\turl = no\n[remote \"upstream\"]\n\turl = u\n[remote \"origin\"]\n\turl = git@github.com:a/b.git\n";
        assert_eq!(
            origin_url(config).as_deref(),
            Some("git@github.com:a/b.git")
        );
        assert_eq!(origin_url("[core]\n"), None);
    }

    #[test]
    fn reads_trailers_only_from_the_last_paragraph() {
        assert_eq!(
            trailers("Fix it\n\nBody.\n\nSigned-off-by: A <a@x>\nCo-authored-by: B\n  <b@x>\n"),
            [
                ("Signed-off-by".to_owned(), "A <a@x>".to_owned()),
                ("Co-authored-by".to_owned(), "B <b@x>".to_owned()),
            ]
        );
        assert!(trailers("Fix: subject only").is_empty());
        assert!(trailers("Fix\n\nNot a trailer line\nKey: value").is_empty());
    }

    /// linked worktree で開いても、ブランチを作った点（reflog）が読めて、土台が後から進んだ積み重ねの土台が分かる。
    #[test]
    fn reads_where_branches_were_created_from_a_linked_worktree() {
        let (tmp, _) = setup();
        let dir = tmp.path();
        // push 済み（幹）の base の上に stacked を積み、そのあと base が進む。コミットの日時を決めて、作った点が
        // 分からないときの「先端の古いほうが土台」が stacked を base の土台にしてしまう形にする。
        // push 先は作業ツリーの外に置く（中にあると、push のあとに git が裏で始める maintenance のロックの
        // ファイルを `git add .` が読みに行き、消えていて失敗することがある）。
        let remotes = tempfile::tempdir().unwrap();
        let remote = remotes.path().join("remote.git");
        git(dir, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let commit_at = |name: &str, offset: u64| {
            std::fs::write(dir.join(name), "x\n").unwrap();
            git(dir, &["add", "."]);
            let date = format!("{} +0000", now + offset);
            let out = Process::new("git")
                .args(["commit", "-q", "-m", name])
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_DATE", &date)
                .env("GIT_COMMITTER_DATE", &date)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            git(dir, &["rev-parse", "HEAD"])
        };
        let first = git(dir, &["rev-parse", "HEAD"]);
        git(dir, &["switch", "-q", "-c", "base"]);
        let b1 = commit_at("b1", 100);
        git(dir, &["push", "-q", "-u", "origin", "base"]);
        git(dir, &["switch", "-q", "-c", "stacked"]);
        commit_at("s1", 200);
        git(dir, &["push", "-q", "-u", "origin", "stacked"]);
        git(dir, &["switch", "-q", "base"]);
        commit_at("b2", 300);
        let wt = dir.join("wt");
        git(
            dir,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "stacked"],
        );

        let repo = Repository::new(GixBackend::open(&wt).unwrap());
        let paths = repo.backend().paths();
        assert_ne!(
            paths.git_dir, paths.common_dir,
            "opened as a linked worktree"
        );
        // 作った点は共通の git dir の reflog から読める（worktree の git dir にはない）。
        let created = repo
            .backend()
            .branch_created_at(&["base", "stacked", "nonexistent"])
            .unwrap();
        assert_eq!(
            created,
            vec![
                Some(Oid::from_hex(&first).unwrap()),
                Some(Oid::from_hex(&b1).unwrap()),
                None
            ]
        );
        let groups = run(&repo, r#"{"op":"branchGroups"}"#);
        let branches = groups["info"]["branches"].as_array().unwrap();
        let parent_of = |name: &str| {
            branches
                .iter()
                .find(|b| b["name"] == name)
                .map(|b| b["parent"].clone())
                .unwrap()
        };
        assert_eq!(parent_of("stacked"), "base", "{groups}");
        assert_eq!(parent_of("base"), serde_json::Value::Null, "{groups}");
    }

    #[test]
    fn reports_details_from_git_and_the_graph() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["tag", "v1"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n4\n").unwrap();
        git(
            dir,
            &[
                "commit",
                "-q",
                "-am",
                "second\n\nSigned-off-by: a <a@example.com>",
            ],
        );
        git(dir, &["tag", "v2"]);
        git(dir, &["branch", "topic"]);
        let first = git(dir, &["rev-parse", "HEAD~1"]);
        let second = git(dir, &["rev-parse", "HEAD"]);
        repo.load_history().unwrap();
        repo.load_refs().unwrap();

        let info =
            run(&repo, &format!(r#"{{"op":"commitInfo","oid":"{second}"}}"#))["info"].clone();
        assert_eq!(info["trailers"][0]["key"], "Signed-off-by");
        assert_eq!(info["committerEmail"], "a@example.com");
        assert!(info["signed"].is_null());

        let extras = run(
            &repo,
            &format!(r#"{{"op":"commitExtras","oid":"{first}"}}"#),
        )["info"]
            .clone();
        assert_eq!(extras["children"][0], second.as_str());
        assert_eq!(extras["branches"], serde_json::json!(["main", "topic"]));
        assert_eq!(
            extras["tags"],
            serde_json::json!(["v1", "v2"]),
            "古いタグから"
        );
        assert_eq!(extras["firstTag"], "v1");
        assert!(extras["previousTag"].is_null());
        assert!(extras["signature"].is_null());
        let extras = run(
            &repo,
            &format!(r#"{{"op":"commitExtras","oid":"{second}"}}"#),
        )["info"]
            .clone();
        assert_eq!(extras["previousTag"], "v1");
        let resolved = run(
            &repo,
            &format!(r#"{{"op":"resolveCommit","rev":"{}"}}"#, &first[..8]),
        );
        assert_eq!(resolved["text"], first.as_str());
        let verified = run(
            &repo,
            &format!(r#"{{"op":"verifySignatures","oids":["{first}","{second}"]}}"#),
        );
        assert_eq!(verified["info"]["checks"][0]["status"], "N");
        assert_eq!(
            committer_email(b"tree t\ncommitter A B <a@x.y> 1 +0000\n\nmsg"),
            "a@x.y"
        );
        assert_eq!(verified["info"]["checks"][1]["status"], "N");
        assert_eq!(extras["stats"][0]["added"], 1);
    }

    #[test]
    fn draws_a_history_edit_before_and_after() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let mut oids = Vec::new();
        for name in ["w", "x", "y", "z"] {
            std::fs::write(dir.join(name), format!("{name}\n")).unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", &format!("add {name}")]);
            oids.push(git(dir, &["rev-parse", "HEAD"]));
        }
        // w はそのまま、x に y をまとめ、z のメッセージを変える。
        let json = serde_json::json!({"op": "rewritePreview", "edit": {"kind": "plan", "steps": [
            {"oid": oids[0], "action": "pick"}, {"oid": oids[1], "action": "pick"},
            {"oid": oids[2], "action": "squash", "message": "add x and y\n\nbody"},
            {"oid": oids[3], "action": "reword", "message": "add z again"}]}});
        let info = run(&repo, &json.to_string())["info"].clone();
        let d = &info["editDrawing"];
        let subjects = |list: &str| {
            d[list]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["subject"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(subjects("before"), ["add z", "add y", "add x"], "{d}");
        assert_eq!(d["before"][1]["action"], "squash");
        assert_eq!(subjects("after"), ["add z again", "add x and y"]);
        assert_eq!(d["after"][1]["combined"], 1);
        assert_eq!(d["base"]["subject"], "add w");
        assert_eq!(d["labels"][0]["name"], "main");
    }

    #[test]
    fn a_preview_tells_which_branches_move_and_what_conflicts() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        // first → b（one）→ c（今のブランチ two）。c は b が足した行を書き換える。
        std::fs::write(dir.join("a.txt"), "1\n2\n3\nb\n").unwrap();
        git(dir, &["commit", "-qam", "b"]);
        let b = git(dir, &["rev-parse", "HEAD"]);
        git(dir, &["branch", "one"]);
        git(dir, &["switch", "-q", "-c", "two"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\nc\n").unwrap();
        git(dir, &["commit", "-qam", "c"]);
        let c = git(dir, &["rev-parse", "HEAD"]);
        let preview = |updates: &str| {
            git(dir, &["config", "rebase.updateRefs", updates]);
            let json = serde_json::json!({"op": "rewritePreview", "edit": {"kind": "plan", "steps": [
                {"oid": b, "action": "drop"}, {"oid": c, "action": "pick"}]}});
            run(&repo, &json.to_string())["info"].clone()
        };
        let off = preview("false");
        assert_eq!(off["current"], "two");
        let find = |v: &serde_json::Value, list: &str, name: &str| {
            v[list]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["name"] == name)
                .cloned()
        };
        // git 2.38 より古ければ、設定にかかわらず git の版が理由になる。
        let (major, minor) = git_version();
        let stays_because = if (major, minor) >= (2, 38) {
            "setting"
        } else {
            "git"
        };
        assert_eq!(
            find(&off, "staying", "one").unwrap()["reason"],
            stays_because
        );
        assert!(off["moving"].as_array().unwrap().is_empty());
        let on = preview("true");
        if (major, minor) >= (2, 38) {
            assert!(find(&on, "moving", "one").is_some(), "{on}");
        }
        // b を消すと、c は b の足した行を書き換えるので当たらない（git 2.40 から調べられる）。
        if (major, minor) >= (2, 40) {
            assert_eq!(on["conflicts"][0]["oid"], c.as_str(), "{on}");
            assert_eq!(on["conflicts"][0]["files"][0], "a.txt");
        } else {
            assert!(on["conflicts"].is_null());
        }
        // ほかの worktree で checkout しているブランチは動かない。
        let wt = tmp.path().join("wt");
        git(dir, &["worktree", "add", "-q", wt.to_str().unwrap(), "one"]);
        let busy = preview("true");
        assert_eq!(
            find(&busy, "staying", "one").unwrap()["reason"],
            "checkedOut",
            "{busy}"
        );
        // rebase の見通し: この操作だけ動かさないと、理由は off。前と後を描く材料も返す。
        let first = git(dir, &["rev-parse", "HEAD~2"]);
        let onto = run(
            &repo,
            &serde_json::json!({"op": "rewritePreview", "onto": first, "updateRefs": false})
                .to_string(),
        )["info"]
            .clone();
        // git 2.38 より古ければ、この操作の指定にかかわらず git の版が理由になる。
        let not_this_time = if (major, minor) >= (2, 38) {
            "off"
        } else {
            "git"
        };
        assert_eq!(
            find(&onto, "staying", "main").unwrap()["reason"],
            not_this_time,
            "{onto}"
        );
        assert_eq!(
            onto["drawing"]["picks"].as_array().unwrap().len(),
            2,
            "{onto}"
        );
        assert_eq!(onto["drawing"]["picks"][1]["subject"], "c");
        assert_eq!(onto["updateRefs"]["configured"], true);
        // 何も触っていない。
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), c);
    }

    #[test]
    fn a_rebase_can_move_stacked_branches_this_once() {
        let (major, minor) = git_version();
        if (major, minor) < (2, 38) {
            return;
        }
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let commit = |name: &str| {
            std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", name]);
            git(dir, &["rev-parse", "HEAD"])
        };
        git(dir, &["switch", "-q", "-c", "one"]);
        let one = commit("one");
        git(dir, &["switch", "-q", "-c", "two"]);
        commit("two");
        git(dir, &["switch", "-q", "main"]);
        commit("main moves");
        git(dir, &["switch", "-q", "two"]);
        git(dir, &["config", "rebase.updateRefs", "false"]);
        let out = run(&repo, r#"{"op":"rebase","onto":"main","updateRefs":true}"#);
        assert!(out["error"].is_null(), "{out}");
        // 設定はオフでも、この操作では one も一緒に動いた。
        assert_ne!(git(dir, &["rev-parse", "one"]), one);
        assert_eq!(
            git(dir, &["rev-parse", "two~1"]),
            git(dir, &["rev-parse", "one"])
        );
    }

    /// テストを動かしている git の版（major, minor）。
    fn git_version() -> (u32, u32) {
        let out = Process::new("git").arg("--version").output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parts = text
            .split_whitespace()
            .nth(2)
            .unwrap_or("0.0")
            .split('.')
            .map(|p| p.parse().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    }

    #[test]
    fn a_plan_past_a_merge_recreates_the_merge() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let commit = |name: &str| {
            std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", name]);
            git(dir, &["rev-parse", "HEAD"])
        };
        let two = commit("two");
        git(dir, &["switch", "-q", "-c", "side"]);
        commit("side");
        git(dir, &["switch", "-q", "main"]);
        let three = commit("three");
        git(dir, &["merge", "-q", "--no-ff", "-m", "Merge side", "side"]);
        let merge = git(dir, &["rev-parse", "HEAD"]);
        let four = commit("four");
        let plan = |steps: serde_json::Value| {
            run(&repo, &serde_json::json!({"op": "rewrite", "label": "edit", "edit": {"kind": "plan", "steps": steps}}).to_string())
        };
        // 並べ替えと、merge を変えることは断る。
        assert!(
            plan(serde_json::json!([
            {"oid": two, "action": "pick"}, {"oid": merge, "action": "pick"},
            {"oid": three, "action": "pick"}, {"oid": four, "action": "pick"}]))["error"]
                .is_string()
        );
        assert!(
            plan(serde_json::json!([
            {"oid": two, "action": "pick"}, {"oid": three, "action": "pick"},
            {"oid": merge, "action": "drop"}, {"oid": four, "action": "pick"}]))["error"]
                .is_string()
        );
        // merge より古いコミットを書き換え、新しいものを消す。merge は作り直されて残る。
        let out = plan(serde_json::json!([
            {"oid": two, "action": "reword", "message": "two, reworded"},
            {"oid": three, "action": "pick"},
            {"oid": merge, "action": "pick"},
            {"oid": four, "action": "drop"}]));
        assert!(out["error"].is_null(), "{out}");
        assert_eq!(git(dir, &["log", "--merges", "--format=%s"]), "Merge side");
        assert!(git(dir, &["log", "--format=%s"]).contains("two, reworded"));
        assert!(!dir.join("four.txt").exists() && dir.join("side.txt").exists());
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), four);

        // 空のコミットも、書き換えて最後まで進み、残る。
        git(dir, &["commit", "-q", "--allow-empty", "-m", "empty"]);
        let empty = git(dir, &["rev-parse", "HEAD"]);
        let out = plan(serde_json::json!([
            {"oid": two, "action": "reword", "message": "two, reworded"},
            {"oid": three, "action": "pick"},
            {"oid": merge, "action": "pick"},
            {"oid": four, "action": "pick"},
            {"oid": empty, "action": "reword", "message": "empty, reworded"}]));
        assert_eq!(out["outcome"], "done", "{out}");
        assert!(!dir.join(".git/rebase-merge").exists());
        assert_eq!(
            git(dir, &["log", "-2", "--format=%s"]),
            "empty, reworded\nfour"
        );
        assert_eq!(
            git(dir, &["rev-parse", "HEAD^{tree}"]),
            git(dir, &["rev-parse", "HEAD^^{tree}"])
        );
    }

    #[test]
    fn a_plan_reorders_squashes_rewords_and_drops_in_one_rebase() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let mut ids = Vec::new();
        for name in ["b", "c", "d", "e"] {
            std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", name]);
            ids.push(git(dir, &["rev-parse", "HEAD"]));
        }
        let [b, c, d, e] = [&ids[0], &ids[1], &ids[2], &ids[3]];
        // 古い順に: c を b にまとめて新しいメッセージ、e を d より前へ動かして書き換え、d を消す。
        let json = serde_json::json!({"op": "rewrite", "label": "edit", "edit": {"kind": "plan", "steps": [
            {"oid": b, "action": "pick"},
            {"oid": c, "action": "squash", "message": "b and c"},
            {"oid": e, "action": "reword", "message": "e reworded"},
            {"oid": d, "action": "drop"},
        ]}});
        let out = run(&repo, &json.to_string());
        assert!(out["error"].is_null(), "{out}");
        assert_eq!(
            git(dir, &["log", "--format=%s", "-3"]),
            "e reworded\nb and c\nfirst"
        );
        assert!(!dir.join("d.txt").exists());
        // 範囲のコミットが抜けていれば断る。
        let head = git(dir, &["rev-parse", "HEAD"]);
        let partial = serde_json::json!({"op": "rewrite", "label": "edit", "edit": {"kind": "plan", "steps": [
            {"oid": head, "action": "drop"},
            {"oid": head, "action": "pick"},
        ]}});
        assert!(run(&repo, &partial.to_string())["error"].is_string());
        // Undo で元に戻る。
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), *e);
    }

    #[test]
    fn fetch_takes_one_remote_or_all_of_them() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        // 2 つのリモートに、それぞれ手元にないコミットを置く。
        for name in ["origin", "backup"] {
            let bare = dir.join(format!("{name}.git"));
            git(dir, &["init", "-q", "--bare", bare.to_str().unwrap()]);
            git(dir, &["remote", "add", name, bare.to_str().unwrap()]);
            git(dir, &["push", "-q", name, "main"]);
            let other = dir.join(format!("{name}-clone"));
            git(
                dir,
                &[
                    "clone",
                    "-q",
                    "-b",
                    "main",
                    bare.to_str().unwrap(),
                    other.to_str().unwrap(),
                ],
            );
            git(
                &other,
                &[
                    "-c",
                    "user.name=b",
                    "-c",
                    "user.email=b@example.com",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    name,
                ],
            );
            git(&other, &["push", "-q", "origin", "main"]);
        }
        let subject = |r: &str| git(dir, &["log", "-1", "--format=%s", r]);
        let out = run(&repo, r#"{"op":"fetch","remote":"backup"}"#);
        assert!(out["error"].is_null(), "{out}");
        assert_eq!(subject("backup/main"), "backup");
        assert_eq!(subject("origin/main"), "first");
        run(&repo, r#"{"op":"fetch"}"#);
        assert_eq!(subject("origin/main"), "origin");
        // オプションに見えるものは断る。
        assert!(run(&repo, r#"{"op":"fetch","remote":"--upload-pack=x"}"#)["error"].is_string());
    }

    #[test]
    fn force_push_replaces_only_the_commit_it_was_shown() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let remote = dir.join("remote.git");
        git(dir, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(dir, &["push", "-q", "-u", "origin", "main"]);
        let shown = git(dir, &["rev-parse", "origin/main"]);
        // 別の人が push し、手元は追跡ブランチだけを進める（自動の Fetch と同じ）。
        let other = dir.join("other");
        git(
            dir,
            &[
                "clone",
                "-q",
                "-b",
                "main",
                remote.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(
            &other,
            &[
                "-c",
                "user.name=b",
                "-c",
                "user.email=b@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "theirs",
            ],
        );
        git(&other, &["push", "-q", "origin", "main"]);
        git(dir, &["fetch", "-q", "origin"]);
        git(dir, &["commit", "-q", "--amend", "-m", "rewritten"]);
        let push = |expect: &str| {
            repo.run_json(&format!(
                r#"{{"op":"push","remote":"origin","branch":"main","remoteBranch":"main","force":true,"expect":"{expect}","setUpstream":false}}"#
            ))
        };
        // 確認で見せたのは他人の push の前のコミット。リモートはもう違うので断る（他人のコミットは消えない）。
        let refused: serde_json::Value = serde_json::from_str(&push(&shown)).unwrap();
        assert!(refused["error"].is_string(), "{refused}");
        assert_ne!(
            git(&remote, &["rev-parse", "main"]),
            git(dir, &["rev-parse", "HEAD"])
        );
        // 今のリモートのコミットを渡せば置き換わる。
        let now = git(&remote, &["rev-parse", "main"]);
        let done: serde_json::Value = serde_json::from_str(&push(&now)).unwrap();
        assert!(done["error"].is_null(), "{done}");
        assert_eq!(
            git(&remote, &["rev-parse", "main"]),
            git(dir, &["rev-parse", "HEAD"])
        );
    }

    #[test]
    fn runs_commands_and_undoes_them() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let r = run(
            &repo,
            r#"{"op":"createBranch","name":"topic","start":"main","checkout":false}"#,
        );
        assert_eq!(r["ok"], true);
        let peek = run(&repo, r#"{"op":"undoPeek"}"#);
        assert_eq!(peek["label"], "Create branch topic");
        assert_eq!(peek["info"]["kind"], "refs", "{peek}");
        // 作ったブランチは、戻すと消える（戻る先がない）。
        assert_eq!(peek["info"]["moves"][0]["name"], "topic", "{peek}");
        assert!(peek["info"]["moves"][0]["to"].is_null(), "{peek}");
        assert_eq!(
            run(&repo, r#"{"op":"undo"}"#)["label"],
            "Create branch topic"
        );
        assert!(git(dir, &["branch", "--list", "topic"]).is_empty());

        // ファイルの中身を戻す記録は files（HEAD とブランチには触らない）。
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        let r = run(&repo, r#"{"op":"discard","paths":["a.txt"]}"#);
        assert_eq!(r["ok"], true, "{r}");
        let peek = run(&repo, r#"{"op":"undoPeek"}"#);
        assert_eq!(peek["label"], "Discard a.txt");
        assert_eq!(peek["info"]["kind"], "files", "{peek}");
        let log = run(&repo, r#"{"op":"undoLog"}"#);
        assert_eq!(log["info"]["entries"][0]["kind"], "files", "{log}");
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "changed\n"
        );

        let bad = run(&repo, r#"{"op":"merge","rev":"no-such-branch"}"#);
        assert_eq!(bad["ok"], false);
        assert!(!bad["error"].as_str().unwrap().is_empty());
        assert_eq!(run(&repo, r#"{"op":"nonsense"}"#)["ok"], false);
    }

    #[test]
    fn measures_stash_bases_and_branches_from_a_stash() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let base = git(dir, &["rev-parse", "HEAD"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n4\n").unwrap();
        git(dir, &["stash", "-q"]);
        // stash を作った後に HEAD が 2 つ進んだ。
        commit_file(dir, "b.txt", "second");
        commit_file(dir, "c.txt", "third");

        let bases = run(&repo, r#"{"op":"stashBases"}"#);
        assert_eq!(bases["ok"], true, "{bases}");
        let stashes = bases["info"]["stashes"].as_array().unwrap();
        assert_eq!(stashes.len(), 1);
        assert_eq!(stashes[0]["name"], "stash@{0}");
        assert_eq!(stashes[0]["base"], base);
        assert_eq!(
            (stashes[0]["ahead"].as_u64(), stashes[0]["behind"].as_u64()),
            (Some(2), Some(0))
        );

        // 土台から枝を作って戻すので、HEAD が進んでいても重ならない。戻せた stash は消える。
        let r = run(
            &repo,
            r#"{"op":"stashBranch","name":"stash@{0}","branch":"wip"}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(git(dir, &["branch", "--show-current"]), "wip");
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), base);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\n2\n3\n4\n"
        );
        assert!(git(dir, &["stash", "list"]).is_empty());
        let none = run(&repo, r#"{"op":"stashBases"}"#);
        assert_eq!(none["info"]["stashes"].as_array().unwrap().len(), 0);
    }

    /// 消したタグは Undo で戻り、注釈付きのタグはタグのオブジェクトごと戻る。タグはまとめてリモートに push でき、
    /// リモートから消せる。
    #[test]
    fn deletes_tags_with_undo_and_pushes_and_deletes_them_on_a_remote() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["tag", "-a", "v1", "-m", "first release"]);
        git(dir, &["tag", "light"]);
        let object = git(dir, &["rev-parse", "refs/tags/v1"]);

        let r = run(&repo, r#"{"op":"deleteTag","name":"v1"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(git(dir, &["tag", "--list"]), "light");
        assert_eq!(run(&repo, r#"{"op":"undoPeek"}"#)["label"], "Delete tag v1");
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(git(dir, &["rev-parse", "refs/tags/v1"]), object);
        assert_eq!(git(dir, &["cat-file", "-t", "refs/tags/v1"]), "tag");

        // push 先は作業ツリーの外に置く（中にあると、push のあとに git が裏で始める maintenance のロックの
        // ファイルを `git add .` が読みに行き、消えていて失敗することがある）。
        let remotes = tempfile::tempdir().unwrap();
        let remote = remotes.path().join("remote.git");
        git(dir, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
        let tags = |dir: &std::path::Path| git(dir, &["tag", "--list"]);
        let r = run(
            &repo,
            r#"{"op":"pushTags","remote":"origin","names":["v1","light"]}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(tags(&remote), "light\nv1");
        assert_eq!(git(&remote, &["cat-file", "-t", "refs/tags/v1"]), "tag");
        let r = run(
            &repo,
            r#"{"op":"pushTags","remote":"origin","names":["light"],"delete":true}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(tags(&remote), "v1");
        // 手元のタグは残る。
        assert_eq!(tags(dir), "light\nv1");
    }

    /// fetch のついでにリモートのタグを読み、タグごとに「同じ・別のタグ・ない」を返す。push・削除の結果は
    /// fetch しなくても反映され、まだ読んでいないリモートは分からないまま。覚えは worktree で共有。
    #[test]
    fn tells_which_remotes_have_each_tag() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        commit_file(dir, "b.txt", "second");
        git(dir, &["tag", "-a", "v1", "-m", "first release"]);
        git(dir, &["tag", "light", "HEAD~1"]);
        // push 先は作業ツリーの外に置く（中にあると、push のあとに git が裏で始める maintenance のロックの
        // ファイルを `git add .` が読みに行き、消えていて失敗することがある）。
        let remotes = tempfile::tempdir().unwrap();
        for name in ["origin", "backup"] {
            let remote = remotes.path().join(format!("{name}.git"));
            git(dir, &["init", "-q", "--bare", remote.to_str().unwrap()]);
            git(dir, &["remote", "add", name, remote.to_str().unwrap()]);
            git(dir, &["push", "-q", name, "main"]);
        }
        git(dir, &["push", "-q", "origin", "v1"]);
        git(dir, &["push", "-q", "backup", "light"]);
        // backup の light と、手元の light は違うコミットを指す。
        git(dir, &["tag", "-f", "light", "HEAD"]);
        let states = |repo: &Repository| {
            let r = run(repo, r#"{"op":"tagRemotes"}"#);
            assert_eq!(r["ok"], true, "{r}");
            r["info"].clone()
        };
        // まだ読んでいない。
        let before = states(&repo);
        assert_eq!(before["remotes"][0]["checked"], serde_json::Value::Null);
        assert_eq!(before["tags"]["v1"], serde_json::json!({}));

        assert_eq!(run(&repo, r#"{"op":"fetch"}"#)["ok"], true);
        let after = states(&repo);
        assert!(after["remotes"][0]["checked"].is_u64(), "{after}");
        assert_eq!(
            after["tags"]["v1"],
            serde_json::json!({"origin": "same", "backup": "missing"})
        );
        assert_eq!(
            after["tags"]["light"],
            serde_json::json!({"origin": "missing", "backup": "different"})
        );

        // 置き換えの push と削除は、その場で反映される。
        let r = run(
            &repo,
            r#"{"op":"pushTags","remote":"backup","names":["light"],"force":true}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        let r = run(
            &repo,
            r#"{"op":"pushTags","remote":"origin","names":["v1"],"delete":true}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        let pushed = states(&repo);
        assert_eq!(pushed["tags"]["light"]["backup"], "same");
        assert_eq!(pushed["tags"]["v1"]["origin"], "missing");

        // 同じコミットに作り直した注釈付きのタグは、別のタグ（置き換えないと push できない）。
        git(dir, &["push", "-q", "origin", "v1"]);
        assert_eq!(run(&repo, r#"{"op":"fetch"}"#)["ok"], true);
        assert_eq!(states(&repo)["tags"]["v1"]["origin"], "same");
        git(dir, &["tag", "-f", "-a", "v1", "-m", "again", "v1^{}"]);
        assert_eq!(states(&repo)["tags"]["v1"]["origin"], "different");

        // linked worktree から開いても、同じ覚えを読む。足したばかりのリモートは分からないまま。
        let wt = dir.join("wt");
        git(
            dir,
            &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
        );
        git(
            dir,
            &[
                "remote",
                "add",
                "later",
                remotes.path().join("origin.git").to_str().unwrap(),
            ],
        );
        let linked = Repository::new(GixBackend::open(&wt).unwrap());
        let seen = states(&linked);
        assert_eq!(
            seen["tags"]["light"],
            serde_json::json!({"origin": "missing", "backup": "same"})
        );
        let later = seen["remotes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "later")
            .unwrap();
        assert_eq!(later["checked"], serde_json::Value::Null);
    }

    /// soft・mixed・hard の reset は、それぞれ index と作業ツリーを git と同じに扱い、Undo で前の状態に戻る。hard は
    /// 作業中の変更を先に stash に残す。
    #[test]
    fn resets_softly_mixed_and_hard_and_undoes_each() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let first = git(dir, &["rev-parse", "HEAD"]);
        let second = commit_file(dir, "b.txt", "second");
        let reset = |mode: &str| {
            let r = run(
                &repo,
                &format!(r#"{{"op":"reset","oid":"{first}","mode":"{mode}"}}"#),
            );
            assert_eq!(r["ok"], true, "{r}");
            r
        };
        let undo = || {
            let r = run(&repo, r#"{"op":"undo"}"#);
            assert_eq!(r["ok"], true, "{r}");
            assert_eq!(git(dir, &["rev-parse", "HEAD"]), second);
            assert_eq!(git(dir, &["status", "--porcelain"]), "");
        };

        // soft: 外したコミットの変更は stage されたまま。
        reset("soft");
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), first);
        assert_eq!(git(dir, &["status", "--porcelain"]), "A  b.txt");
        assert_eq!(
            run(&repo, r#"{"op":"undoPeek"}"#)["label"],
            format!("Reset (soft) to {}", &first[..8])
        );
        undo();

        // mixed: 変更は作業ツリーにだけ残る（stage されない）。
        reset("mixed");
        assert_eq!(git(dir, &["status", "--porcelain"]), "?? b.txt");
        undo();

        // hard: 作業中の変更は stash に残してから、作業ツリーごと動かす。
        std::fs::write(dir.join("a.txt"), "edited\n").unwrap();
        let r = reset("hard");
        assert_eq!(r["info"]["stashed"], true);
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), first);
        assert_eq!(git(dir, &["status", "--porcelain"]), "");
        assert!(!dir.join("b.txt").exists());
        assert!(
            git(dir, &["stash", "list"])
                .contains(&format!("Before reset --hard to {}", &first[..8]))
        );
        undo();
        assert!(dir.join("b.txt").exists());
        // 変更がなければ stash は作らない。
        assert_eq!(reset("hard")["info"]["stashed"], false);
        assert_eq!(git(dir, &["stash", "list"]).lines().count(), 1);
    }

    /// ブランチの reflog から rebase の前の版と載せ先を見つけ、その範囲で 2 つの版を比べる。
    #[test]
    fn compares_a_branch_with_its_version_before_a_rebase() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["checkout", "-q", "-b", "topic"]);
        commit_file(dir, "a.txt", "a");
        commit_file(dir, "b.txt", "b");
        let lines: String = (1..=20).map(|n| format!("{n}\n")).collect();
        std::fs::write(dir.join("c.txt"), &lines).unwrap();
        git(dir, &["add", "c.txt"]);
        git(dir, &["commit", "-q", "-m", "c"]);
        let before = git(dir, &["rev-parse", "HEAD"]);
        // 作る・コミットするだけでは書き換えていない。
        let none = run(&repo, r#"{"op":"previousVersion"}"#);
        assert_eq!(none["info"]["previous"], serde_json::Value::Null, "{none}");

        git(dir, &["checkout", "-q", "main"]);
        let onto = commit_file(dir, "m.txt", "main");
        git(dir, &["checkout", "-q", "topic"]);
        // b を落とし、c を書き換えて rebase する。
        git(
            dir,
            &[
                "-c",
                "sequence.editor=sed -i.bak -e '/ b$/d'",
                "rebase",
                "-q",
                "-i",
                "main",
            ],
        );
        std::fs::write(dir.join("c.txt"), lines.replace("5\n", "five\n")).unwrap();
        git(dir, &["commit", "-q", "-a", "--amend", "--no-edit"]);
        commit_file(dir, "d.txt", "d");
        let after = git(dir, &["rev-parse", "HEAD"]);

        let r = run(&repo, r#"{"op":"previousVersion","branch":"topic"}"#);
        let previous = &r["info"]["previous"];
        // 一番新しい書き換えは amend（載せ先なし）。
        assert!(
            previous["message"]
                .as_str()
                .unwrap()
                .starts_with("commit (amend)"),
            "{r}"
        );
        assert_eq!(previous["new"], after);
        assert_eq!(previous["onto"], serde_json::Value::Null);
        assert_eq!(previous["selector"], "topic@{2}");

        // amend の前の rebase と比べる。
        git(dir, &["reset", "-q", "--hard", "HEAD@{2}"]);
        let r = run(&repo, r#"{"op":"previousVersion"}"#);
        let previous = &r["info"]["previous"];
        assert!(
            previous["message"].as_str().unwrap().starts_with("reset"),
            "{r}"
        );
        git(dir, &["update-ref", "-d", "--no-deref", "refs/heads/topic"]);
        git(dir, &["update-ref", "refs/heads/topic", &before]);
        git(dir, &["checkout", "-q", "-f", "topic"]);
        git(
            dir,
            &[
                "-c",
                "sequence.editor=sed -i.bak -e '/ b$/d'",
                "rebase",
                "-q",
                "-i",
                "main",
            ],
        );
        let r = run(&repo, r#"{"op":"previousVersion"}"#);
        let previous = &r["info"]["previous"];
        assert!(
            previous["message"].as_str().unwrap().starts_with("rebase"),
            "{r}"
        );
        assert_eq!(previous["old"], before);
        assert_eq!(previous["onto"], onto);
        let ranges = previous["ranges"].clone();
        assert_eq!(ranges.as_array().unwrap().len(), 3);

        let r = run(
            &repo,
            &serde_json::json!({ "op": "rangeDiff", "ranges": ranges }).to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
        let pairs = r["info"]["pairs"].as_array().unwrap();
        let statuses: Vec<&str> = pairs
            .iter()
            .map(|p| p["status"].as_str().unwrap())
            .collect();
        assert_eq!(statuses, ["same", "removed", "same"], "{r}");
        assert_eq!(pairs[1]["subject"], "b");
        assert_eq!(pairs[1]["old"]["index"], 2);
        assert_eq!(pairs[1]["new"], serde_json::Value::Null);
        assert_eq!(pairs[2]["old"]["oid"].as_str().unwrap().len(), 40);
        assert_eq!(pairs[2]["new"]["index"], 2);

        // 中身を変えた組は、差分の差分を持つ。足したコミットは added。
        std::fs::write(dir.join("c.txt"), lines.replace("5\n", "five\n")).unwrap();
        git(dir, &["commit", "-q", "-a", "--amend", "--no-edit"]);
        commit_file(dir, "d.txt", "d");
        let r = run(
            &repo,
            &serde_json::json!({ "op": "rangeDiff", "ranges": [onto, before, "HEAD"] }).to_string(),
        );
        let pairs = r["info"]["pairs"].as_array().unwrap();
        let statuses: Vec<&str> = pairs
            .iter()
            .map(|p| p["status"].as_str().unwrap())
            .collect();
        assert_eq!(statuses, ["same", "removed", "changed", "added"], "{r}");
        let diff: Vec<&str> = pairs[2]["diff"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap())
            .collect();
        assert!(
            diff.contains(&"-+5") && diff.contains(&"++five"),
            "{diff:?}"
        );
        let bad = run(&repo, r#"{"op":"rangeDiff","ranges":["--output=x"]}"#);
        assert_eq!(bad["ok"], false);
    }

    /// HEAD の reflog は新しい順で、reset で外れたコミットは辿れない（History にない）と分かる。linked worktree で
    /// 開けば、その worktree の HEAD の reflog を読む。
    #[test]
    fn reads_the_reflog_and_marks_commits_left_behind() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let first = git(dir, &["rev-parse", "HEAD"]);
        let lost = commit_file(dir, "b.txt", "second");
        git(dir, &["reset", "-q", "--hard", "HEAD~1"]);

        let r = run(&repo, r#"{"op":"reflog"}"#);
        assert_eq!(r["ok"], true, "{r}");
        let entries = r["info"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 3, "{r}");
        assert_eq!(entries[0]["selector"], "HEAD@{0}");
        assert_eq!(entries[0]["new"], first);
        assert_eq!(entries[0]["old"], lost);
        assert!(
            entries[0]["message"]
                .as_str()
                .unwrap()
                .starts_with("reset: moving to")
        );
        assert_eq!(entries[0]["reachable"], true);
        assert_eq!(entries[1]["new"], lost);
        assert_eq!(entries[1]["subject"], "second");
        assert_eq!(entries[1]["reachable"], false);
        // 最初のコミットの行は、HEAD を作った行。
        assert_eq!(entries[2]["old"], serde_json::Value::Null);
        assert_eq!(entries[2]["subject"], "first");

        let limited = run(&repo, r#"{"op":"reflog","limit":1}"#);
        assert_eq!(limited["info"]["entries"].as_array().unwrap().len(), 1);
        let branch = run(&repo, r#"{"op":"reflog","name":"refs/heads/main"}"#);
        assert_eq!(
            branch["info"]["entries"][0]["selector"],
            "refs/heads/main@{0}"
        );
        let none = run(&repo, r#"{"op":"reflog","name":"refs/heads/nothing"}"#);
        assert_eq!(none["info"]["entries"].as_array().unwrap().len(), 0);

        // linked worktree の HEAD の reflog は、その worktree のもの（作ったときの行と、そこでのコミット）。
        let wt = dir.join("wt");
        git(
            dir,
            &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
        );
        let side = commit_file(&wt, "c.txt", "on side");
        let linked = Repository::new(GixBackend::open(&wt).unwrap());
        let r = run(&linked, r#"{"op":"reflog"}"#);
        let entries = r["info"]["entries"].as_array().unwrap();
        assert_eq!(entries[0]["new"], side, "{r}");
        assert!(entries.iter().skip(1).all(|e| e["new"] == first), "{r}");
    }

    fn log(dir: &std::path::Path) -> Vec<String> {
        git(dir, &["log", "--format=%s"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// いくつものコミットの Cherry-pick は 1 回の git cherry-pick で、コンフリクトで止まっても残りは失われず、
    /// Continue で続く（git cherry-pick A B C と同じ）。Revert も同じ形。
    #[test]
    fn picks_several_commits_and_continues_after_a_conflict() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["checkout", "-q", "-b", "side"]);
        let b = commit_file(dir, "b.txt", "b");
        std::fs::write(dir.join("a.txt"), "side\n").unwrap();
        git(dir, &["commit", "-q", "-am", "conflicting"]);
        let c = git(dir, &["rev-parse", "HEAD"]);
        let d = commit_file(dir, "d.txt", "d");
        git(dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("a.txt"), "main\n").unwrap();
        git(dir, &["commit", "-q", "-am", "main change"]);

        let pick = serde_json::json!({"op": "cherryPick", "oids": [b, c, d]});
        let r = run(&repo, &pick.to_string());
        assert_eq!(r["outcome"], "conflict", "{r}");
        assert_eq!(log(dir), ["b", "main change", "first"]);
        std::fs::write(dir.join("a.txt"), "both\n").unwrap();
        git(dir, &["add", "a.txt"]);
        let r = run(&repo, r#"{"op":"continue"}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["d", "conflicting", "b", "main change", "first"]);

        // Revert も渡した順に（新しいものから戻す）。
        let d = git(dir, &["rev-parse", "HEAD"]);
        let b = git(dir, &["rev-parse", "HEAD~2"]);
        let revert = serde_json::json!({"op": "revert", "oids": [d, b]});
        let r = run(&repo, &revert.to_string());
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir)[..2], ["Revert \"b\"", "Revert \"d\""]);
        assert!(!dir.join("b.txt").exists() && !dir.join("d.txt").exists());
    }

    fn commit_file(dir: &std::path::Path, name: &str, subject: &str) -> String {
        std::fs::write(dir.join(name), subject).unwrap();
        git(dir, &["add", name]);
        git(dir, &["commit", "-q", "-m", subject]);
        git(dir, &["rev-parse", "HEAD"])
    }

    /// 積み重ねたブランチが一緒に動くはずか（`rebase.updateRefs` が true で、git が 2.38 以降）。backend と同じ
    /// 見方で読む（利用者の全体の設定も含む）。
    fn updates_stacked_branches(dir: &std::path::Path) -> bool {
        amonhen_backend_cli::Git::new(dir).updates_refs_in_rebase()
    }

    /// main ─ one ─ two と積み、two をチェックアウトする。`setting` があればリポジトリの
    /// `rebase.updateRefs` に書く。one と two の先頭を返す。
    fn stack(dir: &std::path::Path, setting: Option<&str>) -> (String, String) {
        if let Some(value) = setting {
            git(dir, &["config", "rebase.updateRefs", value]);
        }
        git(dir, &["switch", "-q", "-c", "one"]);
        let one = commit_file(dir, "one.txt", "one");
        git(dir, &["switch", "-q", "-c", "two"]);
        let two = commit_file(dir, "two.txt", "two");
        (one, two)
    }

    #[test]
    fn rebase_moves_stacked_branches_only_when_the_setting_is_on() {
        // 書いていなければ git の既定（オフ）。Amon Hen が勝手に有効にしない。
        for setting in [None, Some("false"), Some("true")] {
            let (tmp, repo) = setup();
            let dir = tmp.path();
            let (one, two) = stack(dir, setting);
            git(dir, &["switch", "-q", "main"]);
            commit_file(dir, "main.txt", "main");
            git(dir, &["switch", "-q", "two"]);
            let moves = updates_stacked_branches(dir);
            if setting == Some("false") {
                assert!(!moves);
            }

            let r = run(&repo, r#"{"op":"rebase","onto":"main"}"#);
            assert_eq!(r["outcome"], "done", "{setting:?} {r}");
            assert_eq!(
                git(dir, &["rev-parse", "two^^"]),
                git(dir, &["rev-parse", "main"])
            );
            if moves {
                assert_eq!(r["info"]["movedBranches"], serde_json::json!(["one"]));
                assert_eq!(
                    git(dir, &["rev-parse", "two^"]),
                    git(dir, &["rev-parse", "one"])
                );
            } else {
                assert!(r["info"].is_null(), "{setting:?} {r}");
                assert_eq!(git(dir, &["rev-parse", "one"]), one);
            }

            let r = run(&repo, r#"{"op":"undo"}"#);
            assert_eq!(r["ok"], true, "{setting:?} {r}");
            assert_eq!(git(dir, &["rev-parse", "one"]), one);
            assert_eq!(git(dir, &["rev-parse", "two"]), two);
        }
    }

    #[test]
    fn history_edits_move_stacked_branches_only_when_the_setting_is_on() {
        for setting in [None, Some("false"), Some("true")] {
            let (tmp, repo) = setup();
            let dir = tmp.path();
            let (one, two) = stack(dir, setting);
            let first = git(dir, &["rev-parse", "main"]);
            let moves = updates_stacked_branches(dir);
            if setting == Some("false") {
                assert!(!moves);
            }

            // one の先頭を消す。動かすなら one はその前（main）へ、動かさなければ消したコミットに残る。
            let r = rewrite(&repo, serde_json::json!({"kind": "drop", "oid": one}));
            assert_eq!(r["outcome"], "done", "{setting:?} {r}");
            assert_eq!(git(dir, &["rev-parse", "two^"]), first);
            let expected = if moves { &first } else { &one };
            assert_eq!(&git(dir, &["rev-parse", "one"]), expected, "{setting:?}");

            let r = run(&repo, r#"{"op":"undo"}"#);
            assert_eq!(r["ok"], true, "{setting:?} {r}");
            assert_eq!(git(dir, &["rev-parse", "one"]), one);
            assert_eq!(git(dir, &["rev-parse", "two"]), two);
        }
    }

    fn rewrite(repo: &Repository, edit: serde_json::Value) -> serde_json::Value {
        let json = serde_json::json!({"op": "rewrite", "label": "edit", "edit": edit});
        run(repo, &json.to_string())
    }

    #[test]
    fn folds_merged_branches_in_a_real_repository() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "t1.txt", "1");
        commit_file(dir, "t2.txt", "2");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "m.txt", "m");
        git(
            dir,
            &["merge", "-q", "--no-ff", "-m", "merge topic", "topic"],
        );
        let merge = git(dir, &["rev-parse", "HEAD"]);
        repo.load_history().unwrap();
        // HEAD は起点（main）にも含まれる。重複しても動くこと。
        let folded = repo.history_folded(None, &[]).unwrap();
        // topic は merge に取り込み済みなので、ブランチが残っていても折りたたむ。
        assert_eq!(folded.graph.len(), 3);
        let all = repo
            .history_folded(Some(&[Oid::from_hex(&merge).unwrap()]), &[])
            .unwrap();
        assert_eq!(all.graph.len(), 3, "merge・m・first だけ");
        let row = all.row_of(&Oid::from_hex(&merge).unwrap()).unwrap() as usize;
        assert_eq!(all.folded[row], 2);
        assert!(
            all.layout.nodes()[row].flags & amonhen_layout::Node::MERGE != 0,
            "折りたたんでも merge"
        );
        let opened = repo
            .history_folded(
                Some(&[Oid::from_hex(&merge).unwrap()]),
                &[Oid::from_hex(&merge).unwrap()],
            )
            .unwrap();
        assert_eq!(opened.graph.len(), 5);
        assert!(!all.density(4).is_empty());
    }

    #[test]
    fn rewrites_history_and_undoes_it() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let b = commit_file(dir, "b.txt", "b");
        let c = commit_file(dir, "c.txt", "c");
        commit_file(dir, "d.txt", "d");
        assert_eq!(log(dir), ["d", "c", "b", "first"]);

        assert_eq!(
            run(&repo, &format!(r#"{{"op":"commitMessage","oid":"{c}"}}"#))["text"],
            "c\n"
        );
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": c, "message": "C!"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["d", "C!", "b", "first"]);
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(log(dir), ["d", "c", "b", "first"]);

        rewrite(&repo, serde_json::json!({"kind": "fixup", "oid": c}));
        assert_eq!(log(dir), ["d", "b", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        rewrite(&repo, serde_json::json!({"kind": "drop", "oid": b}));
        assert_eq!(log(dir), ["d", "c", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        rewrite(
            &repo,
            serde_json::json!({"kind": "move", "oid": b, "to": 0}),
        );
        assert_eq!(log(dir), ["b", "d", "c", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["d", "c", "b", "first"]);
    }

    #[test]
    fn rewrites_empty_commits_and_keeps_them() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let b = commit_file(dir, "b.txt", "b");
        git(dir, &["commit", "-q", "--allow-empty", "-m", "empty"]);
        let empty = git(dir, &["rev-parse", "HEAD"]);
        let empty_tree = |rev: &str| git(dir, &["rev-parse", &format!("{rev}^{{tree}}")]);
        let assert_empty = |rev: &str| {
            assert_eq!(empty_tree(rev), empty_tree(&format!("{rev}^")), "{rev}");
        };
        let assert_finished = |r: &serde_json::Value| {
            assert_eq!(r["outcome"], "done", "{r}");
            assert!(!dir.join(".git/rebase-merge").exists(), "{r}");
        };

        // 空のコミットが先頭（HEAD）でも、途中でも、書き換えて最後まで進む。
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": empty, "message": "empty!"}),
        );
        assert_finished(&r);
        assert_eq!(log(dir), ["empty!", "b", "first"]);
        assert_empty("HEAD");
        run(&repo, r#"{"op":"undo"}"#);

        let d = commit_file(dir, "d.txt", "d");
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": empty, "message": "empty!"}),
        );
        assert_finished(&r);
        assert_eq!(log(dir), ["d", "empty!", "b", "first"]);
        assert_empty("HEAD^");
        run(&repo, r#"{"op":"undo"}"#);

        // 編集モードでまとめて決めても同じ。
        let json = serde_json::json!({"op": "rewrite", "label": "edit", "edit": {"kind": "plan", "steps": [
            {"oid": b, "action": "reword", "message": "b!"},
            {"oid": empty, "action": "reword", "message": "empty!"},
            {"oid": d, "action": "pick"},
        ]}});
        let r = run(&repo, &json.to_string());
        assert_finished(&r);
        assert_eq!(log(dir), ["d", "empty!", "b!", "first"]);
        assert_empty("HEAD^");
        run(&repo, r#"{"op":"undo"}"#);

        // squash・fixup で前のコミットにまとめても、edit で止めても進む。
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "squash", "oid": empty, "message": "b+empty"}),
        );
        assert_finished(&r);
        assert_eq!(log(dir), ["d", "b+empty", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        let r = rewrite(&repo, serde_json::json!({"kind": "fixup", "oid": empty}));
        assert_finished(&r);
        assert_eq!(log(dir), ["d", "b", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        let r = rewrite(&repo, serde_json::json!({"kind": "edit", "oid": empty}));
        assert_eq!(r["outcome"], "stopped", "{r}");
        assert_eq!(log(dir), ["empty", "b", "first"]);
        let r = run(&repo, r#"{"op":"continue"}"#);
        assert_finished(&r);
        assert_eq!(log(dir), ["d", "empty", "b", "first"]);
        assert_empty("HEAD^");

        // 前のコミットを書き換えても、空のコミットを落とさない。
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": b, "message": "b!"}),
        );
        assert_finished(&r);
        assert_eq!(log(dir), ["d", "empty", "b!", "first"]);
        assert_empty("HEAD^");
    }

    #[test]
    fn shows_files_matching_ignore_patterns_and_hooks() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("build/out")).unwrap();
        std::fs::write(dir.join("build/out/a.o"), "").unwrap();
        std::fs::write(dir.join("x.log"), "").unwrap();
        std::fs::write(dir.join("y.log"), "").unwrap();
        let r = run(
            &repo,
            &serde_json::json!({"op": "ignoreMatches", "text": "# c\n*.log\n\nbuild/\n!keep.log\nnone.txt"}).to_string(),
        );
        let lines = r["info"]["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 3, "{r}");
        assert_eq!(lines[0]["line"], 2);
        assert_eq!(lines[0]["count"], 2);
        assert_eq!(lines[1]["examples"][0], "build/");
        assert_eq!(lines[2]["count"], 0);

        let hook = dir.join(".git/hooks/pre-commit");
        std::fs::write(&hook, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(
            dir.join(".gitattributes"),
            "*.psd filter=lfs diff=lfs merge=lfs -text\n*.txt text\n",
        )
        .unwrap();
        let r = run(&repo, r#"{"op":"hooksAndLfs"}"#);
        let hooks = r["info"]["hooks"].as_array().unwrap();
        assert!(
            hooks
                .iter()
                .any(|h| h["name"] == "pre-commit" && h["enabled"] == true),
            "{r}"
        );
        // サンプル（.sample）は数えない。
        assert!(
            !hooks
                .iter()
                .any(|h| h["name"].as_str().unwrap().ends_with(".sample"))
        );
        assert_eq!(r["info"]["lfsPatterns"], serde_json::json!(["*.psd"]));
    }

    #[test]
    fn squashes_a_range_of_commits() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let b = commit_file(dir, "b.txt", "b");
        let c = commit_file(dir, "c.txt", "c");
        let d = commit_file(dir, "d.txt", "d");
        commit_file(dir, "e.txt", "e");
        // 選んだ順に関わらず、古い順のメッセージと両端を返す。
        let r = run(
            &repo,
            &serde_json::json!({"op": "squashRangeMessage", "oids": [d, b, c]}).to_string(),
        );
        assert_eq!(r["text"], "b\n\nc\n\nd", "{r}");
        assert_eq!(r["info"]["oldest"], b.as_str());
        assert_eq!(r["info"]["newest"], d.as_str());
        // 飛び飛びは断る。
        let gap = run(
            &repo,
            &serde_json::json!({"op": "squashRangeMessage", "oids": [b, d]}).to_string(),
        );
        assert_eq!(gap["ok"], false, "{gap}");

        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "squashRange", "oid": b, "last": d, "message": "b to d"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["e", "b to d", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["e", "d", "c", "b", "first"]);
    }

    /// コミットの notes（`git log` の `%N`）。
    fn note(dir: &std::path::Path, rev: &str) -> String {
        git(dir, &["log", "-1", "--format=%N", rev])
    }

    #[test]
    fn squash_carries_the_notes_like_git() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let mut oids = Vec::new();
        for name in ["b", "c", "d", "e"] {
            oids.push(commit_file(dir, &format!("{name}.txt"), name));
            git(dir, &["notes", "add", "-m", &format!("note {name}")]);
        }
        let [b, c, d, _] = &oids[..] else {
            unreachable!()
        };

        // notes.rewriteRef がなければ、git と同じく写さない。
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "squash", "oid": c, "message": "b+c"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["e", "d", "b+c", "first"]);
        assert_eq!(note(dir, "HEAD~2"), "");
        run(&repo, r#"{"op":"undo"}"#);

        // あれば、git の squash と同じく、まとめた側の notes もつなげて写す（notes.rewriteMode の既定）。
        git(dir, &["config", "notes.rewriteRef", "refs/notes/commits"]);
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "squash", "oid": c, "message": "b+c"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["e", "d", "b+c", "first"]);
        assert_eq!(note(dir, "HEAD~2"), "note b\n\nnote c");
        assert_eq!(note(dir, "HEAD~1"), "note d");
        run(&repo, r#"{"op":"undo"}"#);

        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "squashRange", "oid": b, "last": d, "message": "b to d"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["e", "b to d", "first"]);
        assert_eq!(note(dir, "HEAD~1"), "note b\n\nnote c\n\nnote d");
        run(&repo, r#"{"op":"undo"}"#);

        // fixup は git の todo そのもの。
        rewrite(&repo, serde_json::json!({"kind": "fixup", "oid": c}));
        assert_eq!(log(dir), ["e", "d", "b", "first"]);
        assert_eq!(note(dir, "HEAD~2"), "note b\n\nnote c");
    }

    #[test]
    fn reword_and_squash_follow_the_notes_settings_like_git() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["config", "notes.rewriteRef", "refs/notes/commits"]);
        let b = commit_file(dir, "b.txt", "b");
        git(dir, &["notes", "add", "-m", "note b"]);
        let c = commit_file(dir, "c.txt", "c");
        git(dir, &["notes", "add", "-m", "note c"]);

        // amend で写さない設定でも、git の reword・squash は rebase として写す。
        git(dir, &["config", "notes.rewrite.amend", "false"]);
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": c, "message": "c2"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["c2", "b", "first"]);
        assert_eq!(note(dir, "HEAD"), "note c");
        run(&repo, r#"{"op":"undo"}"#);
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "squash", "oid": c, "message": "b+c"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["b+c", "first"]);
        assert_eq!(note(dir, "HEAD"), "note b\n\nnote c");
        run(&repo, r#"{"op":"undo"}"#);

        // rebase で写さない設定なら、git と同じく、書き換えたコミット（まとめた先）の notes だけが amend で残る。
        git(dir, &["config", "--unset", "notes.rewrite.amend"]);
        git(dir, &["config", "notes.rewrite.rebase", "false"]);
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": b, "message": "b2"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["c", "b2", "first"]);
        assert_eq!(note(dir, "HEAD~1"), "note b");
        assert_eq!(note(dir, "HEAD"), "");
        run(&repo, r#"{"op":"undo"}"#);
        // 前と同じメッセージだと、同じ秒のうちなら前にまとめたコミットと同じ ID になり、その notes が残っている。
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "squash", "oid": c, "message": "b and c"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(note(dir, "HEAD"), "note b");
    }

    #[test]
    fn reword_keeps_the_message_as_written_and_runs_commit_msg_like_git() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let c = commit_file(dir, "c.txt", "c");
        // git の reword と同じく、commit-msg のフックが走る。
        let hook = dir.join(".git/hooks/commit-msg");
        std::fs::write(&hook, "#!/bin/sh\necho 'Change-Id: I1' >> \"$1\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // `#` で始まる行は、コミットの画面と同じく書いたとおりに残す。
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": c, "message": "c2\n\n# kept"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(
            git(dir, &["log", "-1", "--format=%B"]),
            "c2\n\n# kept\nChange-Id: I1"
        );
        run(&repo, r#"{"op":"undo"}"#);
        // commit.cleanup を設定していれば、それに従う。
        git(dir, &["config", "commit.cleanup", "strip"]);
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": c, "message": "c2\n\n# dropped\n"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(
            git(dir, &["log", "-1", "--format=%B"]),
            "c2\n\nChange-Id: I1"
        );
    }

    #[test]
    fn splits_a_commit_and_records_undo_after_continuing() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("x.txt"), "x").unwrap();
        std::fs::write(dir.join("y.txt"), "y").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "x and y"]);
        git(dir, &["notes", "add", "-m", "note x and y"]);
        git(dir, &["config", "notes.rewriteRef", "refs/notes/commits"]);
        let xy = git(dir, &["rev-parse", "HEAD"]);
        commit_file(dir, "z.txt", "z");

        let r = rewrite(&repo, serde_json::json!({"kind": "split", "oid": xy}));
        assert_eq!(r["outcome"], "stopped", "{r}");
        // todo は edit・pick の 2 行で、edit で止まり、変更が作業ツリーに戻っている。
        assert_eq!(run(&repo, r#"{"op":"sequence"}"#)["text"], "rebase (1/2)");
        assert_eq!(log(dir), ["first"]);
        assert_eq!(git(dir, &["status", "--porcelain"]), "?? x.txt\n?? y.txt");
        // 止まっている間は記録しない（保留）。
        assert!(run(&repo, r#"{"op":"undoPeek"}"#)["label"].is_null());
        git(dir, &["add", "x.txt"]);
        git(dir, &["commit", "-q", "-m", "x"]);
        git(dir, &["add", "y.txt"]);
        git(dir, &["commit", "-q", "-m", "y"]);
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(log(dir), ["z", "y", "x", "first"]);
        // git の split の手順と同じく、notes は最後に分けたコミットへ写る（notes.rewriteRef があるとき）。
        assert_eq!(note(dir, "HEAD~1"), "note x and y");
        assert_eq!(note(dir, "HEAD~2"), "");
        assert_eq!(run(&repo, r#"{"op":"undoPeek"}"#)["label"], "edit");
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["z", "x and y", "first"]);
    }

    #[test]
    fn edit_stops_and_continues_like_git() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["config", "notes.rewriteRef", "refs/notes/commits"]);
        let b = commit_file(dir, "b.txt", "b");
        git(dir, &["notes", "add", "-m", "note b"]);
        commit_file(dir, "c.txt", "c");

        // git の edit で止まる。止まっている間に足したコミットにも、git と同じく b の notes が写る。
        let r = rewrite(&repo, serde_json::json!({"kind": "edit", "oid": b}));
        assert_eq!(r["outcome"], "stopped", "{r}");
        assert_eq!(run(&repo, r#"{"op":"sequence"}"#)["text"], "rebase (1/2)");
        assert_eq!(log(dir), ["b", "first"]);
        commit_file(dir, "x.txt", "x");
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(log(dir), ["c", "x", "b", "first"]);
        assert_eq!(note(dir, "HEAD~1"), "note b");
        assert_eq!(note(dir, "HEAD~2"), "note b");
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["c", "b", "first"]);

        // stage した変更は、続けるときに止めたコミットへ amend される（メッセージはそのまま）。
        let r = rewrite(&repo, serde_json::json!({"kind": "edit", "oid": b}));
        assert_eq!(r["outcome"], "stopped", "{r}");
        std::fs::write(dir.join("b.txt"), "b2").unwrap();
        git(dir, &["add", "b.txt"]);
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(log(dir), ["c", "b", "first"]);
        assert_eq!(git(dir, &["show", "HEAD~1:b.txt"]), "b2");
        assert_eq!(note(dir, "HEAD~1"), "note b");
        assert_eq!(git(dir, &["status", "--porcelain"]), "");
    }

    /// tar の中のパス（`tar -t`。Windows にも tar がある）。
    fn tar_list(file: &std::path::Path, gzip: bool) -> Vec<String> {
        let out = Process::new("tar")
            .arg(if gzip { "-tzf" } else { "-tf" })
            .arg(file)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut paths: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.ends_with('/'))
            .map(str::to_owned)
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn exports_a_commit_as_an_archive_with_a_prefix_and_some_folders() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/guide.md"), "guide").unwrap();
        std::fs::write(dir.join(".gitattributes"), "secret.txt export-ignore\n").unwrap();
        std::fs::write(dir.join("secret.txt"), "s").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "docs"]);
        git(dir, &["tag", "-a", "-m", "v1", "v1"]);
        // 書き出すのはコミットした中身だけ（作業ツリーの変更は入らない）。
        std::fs::write(dir.join("a.txt"), "changed").unwrap();
        let out = tempfile::tempdir().unwrap();
        let export = |json: serde_json::Value| run(&repo, &json.to_string());

        let tar = out.path().join("v1.tar");
        let r = export(serde_json::json!({
            "op": "archive", "rev": "v1", "format": "tar", "prefix": "app-1/", "file": tar,
        }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["info"]["size"], std::fs::metadata(&tar).unwrap().len());
        // export-ignore は git のとおりに外れる。
        assert_eq!(
            tar_list(&tar, false),
            ["app-1/.gitattributes", "app-1/a.txt", "app-1/docs/guide.md"]
        );

        let tgz = out.path().join("docs.tar.gz");
        let r = export(serde_json::json!({
            "op": "archive", "rev": "HEAD", "format": "tar.gz", "paths": ["docs"], "file": tgz,
        }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(tar_list(&tgz, true), ["docs/guide.md"]);

        let zip = out.path().join("first.zip");
        let r = export(serde_json::json!({
            "op": "archive", "rev": "HEAD~1", "format": "zip", "file": zip,
        }));
        assert_eq!(r["ok"], true, "{r}");
        assert!(std::fs::read(&zip).unwrap().starts_with(b"PK\x03\x04"));

        // 知らない形・ない時点は書かずに断る。
        // 書けない所なら、どのファイルかを言う。
        let nowhere = out.path().join("no-such-folder").join("a.zip");
        let r = export(serde_json::json!({
            "op": "archive", "rev": "HEAD", "format": "zip", "file": nowhere,
        }));
        assert!(r["error"].as_str().unwrap().contains("a.zip"), "{r}");
        let bad = out.path().join("bad");
        let r = export(serde_json::json!({
            "op": "archive", "rev": "HEAD", "format": "rar", "file": bad,
        }));
        assert_eq!(r["ok"], false, "{r}");
        let r = export(serde_json::json!({
            "op": "archive", "rev": "no-such-tag", "format": "zip", "file": bad,
        }));
        assert_eq!(r["ok"], false, "{r}");
        assert!(!bad.exists());
        // 書き出すフォルダは、その時点の木から選ぶ。
        let folders = |rev: &str| {
            run(
                &repo,
                &serde_json::json!({"op": "treeFolders", "path": "/", "rev": rev}).to_string(),
            )["info"]["entries"]
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!((folders("v1"), folders("HEAD~1")), (1, 0));
        // partial clone でなければ取るものはない。
        let need = run(&repo, r#"{"op":"archiveContents","rev":"HEAD"}"#);
        assert_eq!(need["info"], serde_json::json!({"count": 0, "more": false}));
    }

    /// partial clone では、書き出す時点の手元にない中身を数え、書き出す前にまとめて取る（git 2.29 から。それより古い
    /// git は、git archive が 1 つずつ取る）。
    #[test]
    fn counts_and_fetches_the_contents_an_archive_needs_in_a_partial_clone() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        git(&src, &["init", "-q"]);
        std::fs::create_dir_all(src.join("docs")).unwrap();
        for i in 1..=3 {
            std::fs::write(src.join("a.txt"), format!("a{i}")).unwrap();
            std::fs::write(src.join("docs/d.txt"), format!("d{i}")).unwrap();
            git(&src, &["add", "."]);
            git(&src, &["commit", "-q", "-m", &format!("c{i}")]);
        }
        git(&src, &["config", "uploadpack.allowFilter", "true"]);
        git(&src, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
        let path = src.to_str().unwrap().replace('\\', "/");
        let url = if path.starts_with('/') {
            format!("file://{path}")
        } else {
            format!("file:///{path}")
        };
        git(
            tmp.path(),
            &["clone", "-q", "--filter=blob:none", &url, "partial"],
        );
        let dir = tmp.path().join("partial");
        let repo = Repository::new(GixBackend::open(&dir).unwrap());

        // 最初のコミットの 2 つの中身は手元にない（いまのコミットのものは取り出したときに取った）。
        let need = run(&repo, r#"{"op":"archiveContents","rev":"HEAD~2"}"#);
        assert_eq!(
            need["info"],
            serde_json::json!({"count": 2, "more": false}),
            "{need}"
        );
        let need = run(
            &repo,
            r#"{"op":"archiveContents","rev":"HEAD~2","paths":["docs"]}"#,
        );
        assert_eq!(need["info"]["count"], 1, "{need}");
        assert_eq!(
            run(&repo, r#"{"op":"archiveContents","rev":"HEAD"}"#)["info"]["count"],
            0
        );

        let file = tmp.path().join("old.tar");
        let r = run(
            &repo,
            &serde_json::json!({"op": "archive", "rev": "HEAD~2", "format": "tar", "file": file})
                .to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(tar_list(&file, false), ["a.txt", "docs/d.txt"]);
        assert_eq!(
            run(&repo, r#"{"op":"archiveContents","rev":"HEAD~2"}"#)["info"]["count"],
            0
        );
    }

    /// `file://` の URL（Windows は file:///C:/…）。
    fn file_url_of(path: &std::path::Path) -> String {
        let path = path.to_str().unwrap().replace('\\', "/");
        if path.starts_with('/') {
            format!("file://{path}")
        } else {
            format!("file:///{path}")
        }
    }

    #[test]
    fn saves_commits_as_a_bundle_that_git_can_clone_or_fetch() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        commit_file(dir, "b.txt", "second");
        git(dir, &["tag", "v1"]);
        commit_file(dir, "c.txt", "third");
        git(dir, &["branch", "topic", "HEAD~1"]);
        let out = tempfile::tempdir().unwrap();
        let run_json = |json: serde_json::Value| run(&repo, &json.to_string());

        // すべて（HEAD とブランチ・タグ）。そのまま clone できる。
        let all = out.path().join("all.bundle");
        let preview = run_json(serde_json::json!({"op": "bundlePreview"}));
        assert_eq!(preview["info"]["commits"], 3, "{preview}");
        assert_eq!(preview["info"]["beyondShallow"], false);
        let r = run_json(serde_json::json!({"op": "bundle", "file": all}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["info"]["size"], std::fs::metadata(&all).unwrap().len());
        git(out.path(), &["clone", "-q", all.to_str().unwrap(), "copy"]);
        assert_eq!(
            git(&out.path().join("copy"), &["log", "--format=%s"]),
            "third\nsecond\nfirst"
        );

        // v1 より後だけ。v1 を持っているリポジトリなら取り込める。
        let newer = out.path().join("newer.bundle");
        let refs = serde_json::json!(["refs/heads/main"]);
        let preview =
            run_json(serde_json::json!({"op": "bundlePreview", "refs": refs, "bases": ["v1"]}));
        assert_eq!(preview["info"]["commits"], 1, "{preview}");
        let r = run_json(
            serde_json::json!({"op": "bundle", "refs": refs, "bases": ["v1"], "file": newer}),
        );
        assert_eq!(r["ok"], true, "{r}");
        let copy = out.path().join("copy");
        git(&copy, &["reset", "-q", "--hard", "HEAD~1"]);
        git(
            &copy,
            &[
                "fetch",
                "-q",
                newer.to_str().unwrap(),
                "main:refs/remotes/b/main",
            ],
        );
        assert_eq!(git(&copy, &["log", "-1", "--format=%s", "b/main"]), "third");

        // 入るコミットがなければ作らない。
        let empty = out.path().join("empty.bundle");
        let r = run_json(
            serde_json::json!({"op": "bundle", "refs": ["refs/tags/v1"], "bases": ["v1"], "file": empty}),
        );
        assert_eq!(r["ok"], false, "{r}");
        assert!(!empty.exists());
    }

    /// shallow clone から、取っていない履歴まで要る bundle は作らない（git は作れてしまうが、clone できない）。
    #[test]
    fn refuses_a_bundle_that_needs_history_a_shallow_clone_lacks() {
        let (src_tmp, _) = setup();
        let src = src_tmp.path();
        commit_file(src, "b.txt", "second");
        commit_file(src, "c.txt", "third");
        commit_file(src, "d.txt", "fourth");
        let tmp = tempfile::tempdir().unwrap();
        git(
            tmp.path(),
            &["clone", "-q", "--depth", "2", &file_url_of(src), "shallow"],
        );
        let dir = tmp.path().join("shallow");
        let repo = Repository::new(GixBackend::open(&dir).unwrap());
        let file = tmp.path().join("x.bundle");
        let preview = run(&repo, r#"{"op":"bundlePreview"}"#);
        assert_eq!(preview["info"]["beyondShallow"], true, "{preview}");
        let r = run(
            &repo,
            &serde_json::json!({"op": "bundle", "file": file}).to_string(),
        );
        assert_eq!(r["ok"], false, "{r}");
        assert!(!file.exists());
        // 取得した範囲の中の基点からなら作れる。
        let r = run(
            &repo,
            &serde_json::json!({"op": "bundle", "refs": ["HEAD"], "bases": ["HEAD~1"], "file": file})
                .to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
    }

    /// partial clone では、入る中身のうち手元にないものを数え、作る前にまとめて取る（git 2.29 から。それより古い
    /// git は、git bundle が 1 つずつ取る）。
    #[test]
    fn counts_and_fetches_the_contents_a_bundle_needs_in_a_partial_clone() {
        let (src_tmp, _) = setup();
        let src = src_tmp.path();
        for i in 2..=3 {
            std::fs::write(src.join("a.txt"), format!("{i}")).unwrap();
            git(src, &["commit", "-q", "-am", &format!("c{i}")]);
        }
        git(src, &["config", "uploadpack.allowFilter", "true"]);
        git(src, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
        let tmp = tempfile::tempdir().unwrap();
        git(
            tmp.path(),
            &[
                "clone",
                "-q",
                "--filter=blob:none",
                &file_url_of(src),
                "partial",
            ],
        );
        let dir = tmp.path().join("partial");
        let repo = Repository::new(GixBackend::open(&dir).unwrap());
        // 前の 2 つの版の a.txt が手元にない。
        let preview = run(&repo, r#"{"op":"bundlePreview"}"#);
        assert_eq!(preview["info"]["contents"], 2, "{preview}");
        let file = tmp.path().join("all.bundle");
        let r = run(
            &repo,
            &serde_json::json!({"op": "bundle", "file": file}).to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            run(&repo, r#"{"op":"bundlePreview"}"#)["info"]["contents"],
            0
        );
        git(tmp.path(), &["clone", "-q", file.to_str().unwrap(), "copy"]);
        assert_eq!(
            git(&tmp.path().join("copy"), &["show", "HEAD~2:a.txt"]),
            "1\n2\n3"
        );
    }

    #[test]
    fn fetches_the_branches_of_a_bundle_under_a_name_of_its_own() {
        let (src_tmp, src_repo) = setup();
        let src = src_tmp.path();
        commit_file(src, "b.txt", "second");
        git(src, &["tag", "v1"]);
        commit_file(src, "c.txt", "third");
        let out = tempfile::tempdir().unwrap();
        let file = out.path().join("from-usb.bundle");
        let r = run(
            &src_repo,
            &serde_json::json!({"op": "bundle", "refs": ["refs/heads/main"], "bases": ["v1"], "file": file})
                .to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
        // v1 まで持っているリポジトリ（clone して main を v1 に戻した）。
        git(out.path(), &["clone", "-q", src.to_str().unwrap(), "old"]);
        let old = out.path().join("old");
        git(&old, &["reset", "-q", "--hard", "v1"]);
        let repo = Repository::new(GixBackend::open(&old).unwrap());
        let call = |json: serde_json::Value| run(&repo, &json.to_string());
        let check = call(serde_json::json!({"op": "bundleCheck", "file": file}));
        assert_eq!(
            check["info"]["refs"],
            serde_json::json!(["refs/heads/main"]),
            "{check}"
        );
        assert_eq!(check["info"]["prerequisites"], 1);
        assert_eq!(check["info"]["missing"], 0);
        assert_eq!(check["info"]["name"], "from-usb");

        let r = call(serde_json::json!({"op": "fetchBundle", "file": file, "name": "usb"}));
        assert_eq!(
            r["info"]["refs"],
            serde_json::json!(["refs/remotes/usb/main"]),
            "{r}"
        );
        assert_eq!(
            git(&old, &["log", "-1", "--format=%s", "usb/main"]),
            "third"
        );
        // 使えない名前や、remote と同じ名前は断る（その remote の追跡ブランチを書き換えない）。
        for name in ["origin", "a b", "", "x/y"] {
            let r = call(serde_json::json!({"op": "fetchBundle", "file": file, "name": name}));
            assert_eq!(r["ok"], false, "{name}: {r}");
        }

        // 前提のコミットを持っていないリポジトリ。
        let (_empty_tmp, empty) = setup();
        let check = run(
            &empty,
            &serde_json::json!({"op": "bundleCheck", "file": file}).to_string(),
        );
        assert_eq!(check["info"]["missing"], 1, "{check}");
        let r = run(
            &empty,
            &serde_json::json!({"op": "fetchBundle", "file": file, "name": "usb"}).to_string(),
        );
        assert_eq!(r["ok"], false, "{r}");
        // bundle でないファイル。
        let r = run(
            &empty,
            &serde_json::json!({"op": "bundleCheck", "file": old.join("a.txt")}).to_string(),
        );
        assert_eq!(r["ok"], false, "{r}");
    }

    #[test]
    fn saves_commits_as_patches_and_applies_them_with_am_undoably() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let second = commit_file(dir, "b.txt", "second");
        let third = commit_file(dir, "c.txt", "third");
        let out = tempfile::tempdir().unwrap();
        let folder = out.path().join("patches");
        let r = run(
            &repo,
            &serde_json::json!({"op": "formatPatch", "oids": [second, third], "dir": folder})
                .to_string(),
        );
        let files: Vec<String> = serde_json::from_value(r["info"]["files"].clone()).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|f| {
                std::path::Path::new(f)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
            })
            .collect();
        assert_eq!(names, ["0001-second.patch", "0002-third.patch"], "{r}");
        // 1 つの mbox にもできる。
        let mbox = out.path().join("all.mbox");
        run(
            &repo,
            &serde_json::json!({"op": "formatPatch", "oids": [second, third], "file": mbox})
                .to_string(),
        );
        let text = std::fs::read_to_string(&mbox).unwrap();
        assert_eq!(text.matches("\nSubject: [PATCH ").count(), 2, "{text}");

        git(dir, &["reset", "-q", "--hard", "HEAD~2"]);
        let r = run(
            &repo,
            &serde_json::json!({"op": "readPatch", "paths": files}).to_string(),
        );
        assert_eq!(r["info"]["kind"], "mailbox", "{r}");
        assert_eq!(r["info"]["patches"][1]["subject"], "third");
        assert_eq!(r["info"]["patches"][1]["files"][0]["path"], "c.txt");
        assert_eq!(r["info"]["applies"], true, "{r}");
        let r = run(
            &repo,
            &serde_json::json!({"op": "am", "paths": files, "threeWay": false}).to_string(),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["third", "second", "first"]);
        assert_eq!(
            run(&repo, r#"{"op":"undoPeek"}"#)["label"],
            "Apply 2 patches"
        );
        // diff を当てて Undo で戻すと、ファイルは同じ中身のまま書き直される。その後でも am を Undo できる（reset
        // --keep が index の古い stat で断らない）。
        let diff = out.path().join("c.diff");
        std::fs::write(
            &diff,
            "diff --git a/c.txt b/c.txt\n--- a/c.txt\n+++ b/c.txt\n@@ -1 +1 @@\n-third\n\\ No newline at end of file\n+THIRD\n",
        )
        .unwrap();
        let r = run(
            &repo,
            &serde_json::json!({"op": "applyPatch", "paths": [diff]}).to_string(),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(std::fs::read_to_string(dir.join("c.txt")).unwrap(), "third");
        let r = run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(log(dir), ["first"]);
    }

    #[test]
    fn stops_am_where_a_patch_does_not_apply_and_retries_it_with_a_three_way_merge() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "1\nx\n3\n").unwrap();
        git(dir, &["commit", "-q", "-am", "change"]);
        let out = tempfile::tempdir().unwrap();
        git(
            dir,
            &[
                "format-patch",
                "-q",
                "-1",
                "-o",
                &out.path().to_string_lossy(),
            ],
        );
        let patch = out.path().join("0001-change.patch");
        git(dir, &["reset", "-q", "--hard", "HEAD~1"]);
        std::fs::write(dir.join("a.txt"), "1\ny\n3\n").unwrap();
        git(dir, &["commit", "-q", "-am", "other"]);

        let r = run(
            &repo,
            &serde_json::json!({"op": "readPatch", "paths": [patch]}).to_string(),
        );
        assert_eq!(r["info"]["applies"], false, "{r}");
        let r = run(
            &repo,
            &serde_json::json!({"op": "am", "paths": [patch], "threeWay": false}).to_string(),
        );
        assert_eq!(r["outcome"], "conflict", "{r}");
        let r = run(&repo, r#"{"op":"sequence"}"#);
        assert_eq!(r["text"], "am (1/1)", "{r}");
        assert_eq!(r["info"]["kind"], "am");
        assert_eq!(r["info"]["step"]["subject"], "change");
        assert_eq!(r["info"]["threeWay"], false);
        assert_eq!(r["info"]["retried"], false);
        // 止まっている間は記録しない（保留）。
        assert!(run(&repo, r#"{"op":"undoPeek"}"#)["label"].is_null());

        let r = run(&repo, r#"{"op":"amThreeWay"}"#);
        assert_eq!(r["outcome"], "conflict", "{r}");
        assert_eq!(run(&repo, r#"{"op":"sequence"}"#)["info"]["retried"], true);
        let text = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(text.contains("<<<<<<<"), "{text}");
        std::fs::write(dir.join("a.txt"), "1\nx\n3\n").unwrap();
        git(dir, &["add", "a.txt"]);
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(log(dir), ["change", "other", "first"]);
        assert_eq!(run(&repo, r#"{"op":"undoPeek"}"#)["label"], "Apply 1 patch");
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["other", "first"]);
        // 当て直した印は、am が終わると rebase-apply ごと消え、次の am に残らない。
        let r = run(
            &repo,
            &serde_json::json!({"op": "am", "paths": [patch], "threeWay": false}).to_string(),
        );
        assert_eq!(r["outcome"], "conflict", "{r}");
        assert_eq!(run(&repo, r#"{"op":"sequence"}"#)["info"]["retried"], false);
    }

    #[test]
    fn applies_a_diff_to_the_working_tree_undoably() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "1\nchanged\n3\n").unwrap();
        std::fs::write(dir.join("new.txt"), "new\n").unwrap();
        git(dir, &["add", "-N", "new.txt"]);
        let diff = git(dir, &["diff"]) + "\n";
        // intent-to-add のファイルは reset --hard で消える。
        git(dir, &["reset", "-q", "--hard"]);
        let out = tempfile::tempdir().unwrap();
        let patch = out.path().join("change.diff");
        std::fs::write(&patch, diff).unwrap();

        let r = run(
            &repo,
            &serde_json::json!({"op": "readPatch", "paths": [patch]}).to_string(),
        );
        assert_eq!(r["info"]["kind"], "diff", "{r}");
        assert_eq!(r["info"]["patches"][0]["files"][1]["status"], "added");
        assert_eq!(r["info"]["applies"], true, "{r}");
        let r = run(
            &repo,
            &serde_json::json!({"op": "applyPatch", "paths": [patch]}).to_string(),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nchanged\n3\n"
        );
        assert!(dir.join("new.txt").exists());
        // 既定では index に触らない（git apply と同じ）。
        assert_eq!(git(dir, &["diff", "--cached", "--name-only"]), "");
        assert_eq!(
            run(&repo, r#"{"op":"undoPeek"}"#)["label"],
            "Apply change.diff"
        );
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\n2\n3\n"
        );
        assert!(!dir.join("new.txt").exists());
    }

    #[test]
    fn counts_added_and_removed_lines_against_head() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        // a.txt は 1 2 3 → 1 x 3 y（index に一部、作業ツリーに残り）。
        std::fs::write(dir.join("a.txt"), "1\nx\n3\n").unwrap();
        git(dir, &["add", "a.txt"]);
        std::fs::write(dir.join("a.txt"), "1\nx\n3\ny\n").unwrap();
        std::fs::write(dir.join("new.txt"), "untracked\n").unwrap();
        let r = run(&repo, r#"{"op":"workingStats"}"#);
        let stats = r["info"]["stats"].as_array().unwrap();
        assert_eq!(stats.len(), 1, "{r}");
        assert_eq!(stats[0]["path"], "a.txt");
        assert_eq!(
            (stats[0]["added"].as_u64(), stats[0]["removed"].as_u64()),
            (Some(2), Some(1))
        );
    }

    #[test]
    fn lists_the_undo_log_newest_first() {
        let (_tmp, repo) = setup();
        for name in ["a", "b"] {
            let r = run(
                &repo,
                &format!(
                    r#"{{"op":"createBranch","name":"{name}","start":"HEAD","checkout":true}}"#
                ),
            );
            assert_eq!(r["ok"], true, "{r}");
        }
        let log = run(&repo, r#"{"op":"undoLog"}"#);
        let entries = log["info"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0]["label"].as_str().unwrap().contains('b'));
        assert!(entries[0]["time"].as_u64().unwrap() >= entries[1]["time"].as_u64().unwrap());
    }

    #[test]
    fn the_default_branch_can_be_chosen_in_config() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["branch", "trunk"]);
        let base = || run(&repo, r#"{"op":"branchGroups"}"#)["info"]["base"].clone();
        assert_eq!(base(), "main");
        git(dir, &["config", "amonhen.defaultBranch", "trunk"]);
        assert_eq!(base(), "trunk");
        // ないブランチなら、判定したものに戻る。
        git(dir, &["config", "amonhen.defaultBranch", "gone"]);
        assert_eq!(base(), "main");
    }

    #[test]
    fn reads_pull_and_fetch_defaults_from_git_config() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let defaults = || run(&repo, r#"{"op":"syncDefaults"}"#)["info"].clone();
        let d = defaults();
        assert_eq!(d["pull"], "merge", "{d}");
        assert_eq!(d["prune"], false, "{d}");

        git(dir, &["config", "pull.rebase", "merges"]);
        git(dir, &["config", "fetch.prune", "true"]);
        let d = defaults();
        assert_eq!(d["pull"], "rebase", "{d}");
        assert_eq!(d["prune"], true, "{d}");

        // pull.ff=only は pull.rebase より強い。
        git(dir, &["config", "pull.ff", "only"]);
        assert_eq!(defaults()["pull"], "fastForwardOnly");
    }

    #[test]
    fn pull_without_a_reconcile_setting_follows_git_on_diverged_branches() {
        let (upstream, _) = setup();
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("upstream.git");
        git(
            tmp.path(),
            &[
                "clone",
                "-q",
                "--bare",
                upstream.path().to_str().unwrap(),
                "upstream.git",
            ],
        );
        git(
            tmp.path(),
            &["clone", "-q", bare.to_str().unwrap(), "clone"],
        );
        git(
            tmp.path(),
            &["clone", "-q", bare.to_str().unwrap(), "other"],
        );
        let (dir, other) = (tmp.path().join("clone"), tmp.path().join("other"));
        for d in [&dir, &other] {
            for (key, value) in [("user.name", "a"), ("user.email", "a@example.com")] {
                git(d, &["config", key, value]);
            }
        }
        // 上流と手元の両方で進め、分かれさせる。
        std::fs::write(other.join("b.txt"), "b\n").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "upstream"]);
        git(&other, &["push", "-q"]);
        std::fs::write(dir.join("c.txt"), "c\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "local"]);
        let repo = Repository::new(GixBackend::open(&dir).unwrap());
        let defaults = || run(&repo, r#"{"op":"syncDefaults"}"#)["info"].clone();
        let config = || git(&dir, &["config", "--local", "--list"]);
        let before = config();

        let (major, minor) = git_version();
        let refuses = defaults()["pullRefuses"].clone();
        if (major, minor) >= (2, 35) {
            assert_eq!(refuses, "diverged");
            // git の設定に任せると、git は取り込み方が決まっていないので断る。
            let r = run(&repo, r#"{"op":"pull"}"#);
            assert_eq!(r["ok"], false, "{r}");
            assert!(r["error"].as_str().unwrap().contains("divergent"), "{r}");
        } else if (major, minor) < (2, 33) {
            assert!(refuses.is_null(), "{refuses}");
        }
        // この pull だけ選べば取り込める。設定は書かない。
        let r = run(&repo, r#"{"op":"pull","mode":"rebase"}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(git(&dir, &["log", "--format=%s"]), "local\nupstream\nfirst");
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        let r = run(&repo, r#"{"op":"pull","mode":"merge"}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(git(&dir, &["rev-list", "--count", "--merges", "HEAD"]), "1");
        assert_eq!(config(), before);

        // どれかを決めていれば断らない。branch.<name>.rebase は pull.rebase より先。
        git(&dir, &["config", "pull.rebase", "false"]);
        let d = defaults();
        assert!(d["pullRefuses"].is_null(), "{d}");
        assert_eq!(d["pull"], "merge", "{d}");
        git(&dir, &["config", "branch.main.rebase", "true"]);
        assert_eq!(defaults()["pull"], "rebase");
        git(&dir, &["config", "--unset", "pull.rebase"]);
        let d = defaults();
        assert!(d["pullRefuses"].is_null(), "{d}");
        assert_eq!(d["pull"], "rebase", "{d}");
        git(&dir, &["config", "--unset", "branch.main.rebase"]);
        git(&dir, &["config", "pull.ff", "true"]);
        assert!(defaults()["pullRefuses"].is_null());
    }

    #[test]
    fn reads_and_writes_the_git_in_the_machines_settings() {
        let (_tmp, repo) = setup();
        let config = tempfile::tempdir().unwrap();
        let env = amonhen_model::process::ClientEnv::from_vars(
            amonhen_model::settings::config_vars(config.path().to_str().unwrap()),
        );
        amonhen_model::process::with_client_env(&env, || {
            let r = run(&repo, r#"{"op":"machineGit"}"#);
            assert_eq!(r["info"]["chosen"], serde_json::Value::Null, "{r}");
            assert_eq!(r["info"]["source"], "path");
            let file = config.path().join("amonhen").join("settings.json");
            assert_eq!(r["info"]["file"], file.display().to_string());

            let r = run(&repo, r#"{"op":"setMachineGit","git":"/no/such/git"}"#);
            assert_eq!(r["ok"], true, "{r}");
            let r = run(&repo, r#"{"op":"machineGit"}"#);
            assert_eq!(r["info"]["chosen"], "/no/such/git");
            assert_eq!(r["info"]["source"], "settings");
            assert!(
                r["info"]["problem"]
                    .as_str()
                    .unwrap()
                    .contains("/no/such/git")
            );
            // 選んだ git がなければ、git を動かす操作はエラーになる（PATH の git では動かさない）。
            let r = run(&repo, r#"{"op":"configSet","key":"user.name","value":"x"}"#);
            assert_eq!(r["ok"], false, "{r}");

            run(&repo, r#"{"op":"setMachineGit"}"#);
            let r = run(&repo, r#"{"op":"machineGit"}"#);
            assert_eq!(r["info"]["source"], "path", "{r}");
        });
    }

    #[test]
    #[cfg(unix)]
    fn lists_a_folder_of_the_machine_running_git() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (_tmp, repo) = setup();
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir(d.join("bin")).unwrap();
        std::fs::create_dir_all(d.join("repo").join(".git")).unwrap();
        std::fs::write(d.join("tool"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(d.join("tool"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(d.join(".hidden"), "").unwrap();
        symlink(d.join("bin"), d.join("to-bin")).unwrap();
        symlink(d.join("gone"), d.join("broken")).unwrap();

        let path = d.join("bin").join("..");
        let r = run(
            &repo,
            &serde_json::json!({ "op": "listDirectory", "path": path }).to_string(),
        );
        let info = &r["info"];
        assert_eq!(info["path"], d.display().to_string(), "{r}");
        let entries = info["entries"].as_array().unwrap();
        let names: Vec<&str> = entries
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [".hidden", "bin", "broken", "repo", "to-bin", "tool"]
        );
        let get = |name: &str| entries.iter().find(|e| e["name"] == name).unwrap();
        assert_eq!(get("bin")["kind"], "folder");
        assert_eq!(get("bin")["repository"], false);
        assert_eq!(get("repo")["repository"], true);
        assert_eq!(get("tool")["executable"], true);
        assert_eq!(get(".hidden")["executable"], false);
        assert_eq!(get("to-bin")["kind"], "folder");
        assert_eq!(get("to-bin")["link"], true);
        assert_eq!(get("broken")["kind"], "other");

        // `~` はホーム。
        let r = run(&repo, r#"{"op":"listDirectory","path":"~"}"#);
        assert_eq!(r["info"]["path"], r["info"]["home"], "{r}");

        let r = run(
            &repo,
            &serde_json::json!({ "op": "listDirectory", "path": d.join("tool") }).to_string(),
        );
        assert_eq!(r["ok"], false, "{r}");
    }

    #[test]
    fn reports_the_git_version_and_whether_rebase_can_move_stacked_branches() {
        let (tmp, repo) = setup();
        let r = run(&repo, r#"{"op":"gitVersion"}"#);
        let version = r["info"]["version"].as_str().unwrap();
        let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap());
        let (major, minor) = (parts.next().unwrap(), parts.next().unwrap());
        assert_eq!(r["info"]["updateRefs"], (major, minor) >= (2, 38), "{r}");
        assert_eq!(r["info"]["rerereCache"], false, "{r}");
        std::fs::create_dir(tmp.path().join(".git/rr-cache")).unwrap();
        let r = run(&repo, r#"{"op":"gitVersion"}"#);
        assert_eq!(r["info"]["rerereCache"], true, "{r}");

        // 段は local・global だけ。
        let r = run(
            &repo,
            r#"{"op":"configSet","key":"pull.rebase","value":"true","scope":"system"}"#,
        );
        assert!(r["error"].as_str().unwrap().contains("system"), "{r}");
    }

    #[test]
    fn lists_and_deletes_ignored_files() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join(".gitignore"), "target/\n*.log\n").unwrap();
        git(dir, &["add", ".gitignore"]);
        git(dir, &["commit", "-q", "-m", "ignore"]);
        std::fs::create_dir_all(dir.join("target/debug")).unwrap();
        std::fs::write(dir.join("target/debug/bin"), "b").unwrap();
        std::fs::write(dir.join("a.log"), "l").unwrap();
        std::fs::write(dir.join("new file.log"), "l").unwrap();
        std::fs::write(dir.join("untracked.txt"), "u").unwrap();
        let list = || {
            let r = run(&repo, r#"{"op":"ignoredFiles"}"#);
            let mut paths: Vec<String> = r["info"]["paths"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap().to_owned())
                .collect();
            paths.sort();
            paths
        };
        // フォルダは 1 つにまとめ、untracked（ignore されていない）ものは入らない。
        assert_eq!(list(), ["a.log", "new file.log", "target/"]);
        let r = run(
            &repo,
            r#"{"op":"deleteIgnored","paths":["target/","a.log"]}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert!(!dir.join("target").exists() && !dir.join("a.log").exists());
        assert!(dir.join("untracked.txt").exists());
        assert_eq!(list(), ["new file.log"]);
    }

    #[test]
    fn forgets_a_resolution_rerere_applied() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["config", "rerere.enabled", "true"]);
        git(dir, &["checkout", "-q", "-b", "topic"]);
        std::fs::write(dir.join("a.txt"), "topic\n").unwrap();
        git(dir, &["commit", "-q", "-am", "topic"]);
        git(dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("a.txt"), "main\n").unwrap();
        git(dir, &["commit", "-q", "-am", "main"]);
        // 一度解決して rerere に覚えさせ、merge をやり直すと rerere がその解決を当てる。
        let _ = Process::new("git")
            .args(["merge", "topic"])
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output();
        std::fs::write(dir.join("a.txt"), "resolved\n").unwrap();
        git(dir, &["add", "a.txt"]);
        git(dir, &["commit", "-q", "-m", "merge"]);
        git(dir, &["reset", "-q", "--hard", "HEAD~1"]);
        let _ = Process::new("git")
            .args(["merge", "topic"])
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "resolved\n"
        );
        let r = run(&repo, r#"{"op":"forgetResolution","path":"a.txt"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .contains("<<<<<<<")
        );
        // Undo で、rerere が当てた形のファイルに戻る。
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "resolved\n"
        );
        let r = run(&repo, r#"{"op":"forgetAllResolutions"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert!(r["info"]["count"].as_u64().unwrap() >= 1, "{r}");
        assert!(!dir.join(".git/rr-cache").exists());
    }

    #[test]
    fn lists_branches_to_clean_up_made_after_the_last_look() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let names = |r: &serde_json::Value| -> Vec<String> {
            r["info"]["branches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["name"].as_str().unwrap().to_owned())
                .collect()
        };
        let cleanup = r#"{"op":"branchCleanup","pullRequests":[]}"#;
        let r = run(&repo, cleanup);
        assert_eq!(r["ok"], true, "{r}");
        assert!(names(&r).is_empty(), "{r}");
        // 変化の知らせを待たずに（git で作ってすぐ）頼まれても、今の ref で選ぶ。
        git(dir, &["switch", "-q", "-c", "later", "main"]);
        std::fs::write(dir.join("later.txt"), "later").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "later"]);
        git(dir, &["switch", "-q", "main"]);
        git(dir, &["merge", "-q", "--no-ff", "--no-edit", "later"]);
        let r = run(&repo, cleanup);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(names(&r), vec!["later".to_owned()], "{r}");
    }

    #[test]
    fn lists_branches_to_clean_up_and_deletes_them_at_once() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let commit_on = |branch: &str, file: &str| {
            git(dir, &["switch", "-q", "-c", branch, "main"]);
            std::fs::write(dir.join(file), branch).unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", branch]);
            git(dir, &["switch", "-q", "main"]);
        };
        // 取り込み済み・squash でマージした PR・PR とは先端が違う・上流が消えた・お気に入り・ほかの worktree で使っている。
        commit_on("done", "d.txt");
        git(dir, &["merge", "-q", "--ff-only", "done"]);
        commit_on("squashed", "s.txt");
        commit_on("moved", "m.txt");
        commit_on("lost", "l.txt");
        git(dir, &["config", "remote.origin.url", "/nowhere"]);
        git(
            dir,
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        );
        git(dir, &["config", "branch.lost.remote", "origin"]);
        git(dir, &["config", "branch.lost.merge", "refs/heads/lost"]);
        git(dir, &["branch", "fav", "done"]);
        git(dir, &["config", "branch.fav.amonhenPinned", "1"]);
        git(dir, &["branch", "used", "done"]);
        let wt = tempfile::tempdir().unwrap();
        let wt_path = wt.path().join("wt");
        git(
            dir,
            &["worktree", "add", "-q", wt_path.to_str().unwrap(), "used"],
        );
        // 作ったばかり（既定のブランチの先端）。
        git(dir, &["branch", "fresh", "main"]);
        let tip = |b: &str| git(dir, &["rev-parse", b]);
        let prs = serde_json::json!([
            { "number": 7, "branch": "squashed", "oid": tip("squashed") },
            { "number": 8, "branch": "moved", "oid": tip("main") },
        ]);
        let r = run(
            &repo,
            &serde_json::json!({ "op": "branchCleanup", "pullRequests": prs }).to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["info"]["base"], "main");
        let found: Vec<(String, String, bool, bool)> = r["info"]["branches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                (
                    b["name"].as_str().unwrap().to_owned(),
                    b["reason"].as_str().unwrap().to_owned(),
                    b["force"].as_bool().unwrap(),
                    b["checked"].as_bool().unwrap(),
                )
            })
            .collect();
        let mut found = found;
        found.sort();
        let owned = |n: &str, r: &str, f, c| (n.to_owned(), r.to_owned(), f, c);
        assert_eq!(
            found,
            vec![
                owned("done", "merged", false, true),
                owned("lost", "gone", true, false),
                owned("squashed", "pullRequest", true, true),
            ]
        );
        let squashed = r["info"]["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"] == "squashed")
            .unwrap();
        assert_eq!(squashed["pullRequest"], 7);
        // サイドバーの分け方にも、PR でマージ済みのものの番号が付く（先端が違うものには付かない）。
        let groups = run(
            &repo,
            &serde_json::json!({ "op": "branchGroups", "pullRequests": prs }).to_string(),
        );
        let pr_of = |name: &str| {
            groups["info"]["branches"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["name"] == name)
                .unwrap()["pullRequest"]
                .clone()
        };
        assert_eq!(pr_of("squashed"), 7);
        assert_eq!(pr_of("moved"), serde_json::Value::Null);
        assert_eq!(pr_of("done"), serde_json::Value::Null);
        // まとめて消し、1 回の Undo でまとめて戻る。git が一緒に消す `branch.<name>.*` の設定（上流・アーカイブ）も戻る。
        git(dir, &["config", "branch.done.amonhenArchived", "true"]);
        let before = (tip("done"), tip("squashed"), tip("lost"));
        let r = run(
            &repo,
            r#"{"op":"deleteBranches","branches":[{"name":"done"},{"name":"squashed","force":true},{"name":"lost","force":true}]}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            git(dir, &["branch", "--list", "done", "squashed", "lost"]),
            ""
        );
        let config = git(dir, &["config", "--list", "--local"]);
        assert!(
            !config.contains("branch.lost.") && !config.contains("branch.done."),
            "{config}"
        );
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!((tip("done"), tip("squashed"), tip("lost")), before);
        assert_eq!(git(dir, &["config", "branch.lost.remote"]), "origin");
        assert_eq!(
            git(dir, &["config", "branch.lost.merge"]),
            "refs/heads/lost"
        );
        assert_eq!(git(dir, &["config", "branch.done.amonhenArchived"]), "true");
        // 1 本だけ消すときも同じ。
        let r = run(&repo, r#"{"op":"deleteBranch","name":"lost","force":true}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(git(dir, &["config", "branch.lost.remote"]), "origin");
        // 取り込んでいないものは -d では消えず、それより前に消せた分は記録してある。
        let r = run(
            &repo,
            r#"{"op":"deleteBranches","branches":[{"name":"done"},{"name":"moved"}]}"#,
        );
        assert_eq!(r["ok"], false, "{r}");
        let error = r["error"].as_str().unwrap();
        assert!(
            error.contains("“moved”") && error.contains("deleted: 1"),
            "{r}"
        );
        assert_eq!(git(dir, &["branch", "--list", "done"]), "");
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(tip("done"), before.0);
    }

    /// worktree の新しい置き場所の `~` は、git が動く機械のホーム（SSH 先のパスは FE が広げられない）。
    #[test]
    fn moves_a_worktree_to_a_path_under_home() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let name = format!("amonhen-moved-{}", std::process::id());
        let wt = tmp.path().with_extension("wt-home");
        let wt = wt.to_str().unwrap();
        let r = run(
            &repo,
            &serde_json::json!({ "op": "worktreeAdd", "path": wt, "newBranch": "moves-home" })
                .to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
        let r = run(
            &repo,
            &serde_json::json!({ "op": "worktreeMove", "path": wt, "to": format!("~/{name}") })
                .to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
        let moved = std::env::home_dir().unwrap().join(&name);
        assert!(moved.join(".git").exists(), "{}", moved.display());
        assert!(!dir.join("~").exists());
        let r = run(
            &repo,
            &serde_json::json!({ "op": "worktreeRemove", "path": moved, "force": true })
                .to_string(),
        );
        assert_eq!(r["ok"], true, "{r}");
    }

    #[test]
    fn starts_orphan_branches_empty_or_with_the_files() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let orphan = |json: serde_json::Value| {
            let mut command = json;
            command["op"] = "createOrphanBranch".into();
            run(&repo, &command.to_string())
        };
        // 空から（git switch --orphan）: 追跡していたファイルは作業ツリーから消え、ブランチはまだない。
        let r = orphan(serde_json::json!({ "name": "pages" }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(git(dir, &["symbolic-ref", "HEAD"]), "refs/heads/pages");
        assert!(!dir.join("a.txt").exists());
        assert_eq!(git(dir, &["branch", "--list", "pages"]), "");
        git(dir, &["switch", "-q", "main"]);
        // 今のファイルを残す（git checkout --orphan）: stage したまま。
        let r = orphan(serde_json::json!({ "name": "fresh", "keepFiles": true }));
        assert_eq!(r["ok"], true, "{r}");
        assert!(dir.join("a.txt").exists());
        assert_eq!(git(dir, &["status", "--porcelain"]), "A  a.txt");
        let r = orphan(serde_json::json!({ "name": "-x" }));
        assert_eq!(r["ok"], false);
    }

    #[test]
    fn locks_moves_and_repairs_worktrees() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let other = tempfile::tempdir().unwrap();
        let wt = other.path().join("wt").to_str().unwrap().replace('\\', "/");
        let moved = other
            .path()
            .join("moved")
            .to_str()
            .unwrap()
            .replace('\\', "/");
        let cmd = |json: serde_json::Value| run(&repo, &json.to_string());
        let r = cmd(serde_json::json!({ "op": "worktreeAdd", "path": wt, "newBranch": "side" }));
        assert_eq!(r["ok"], true, "{r}");
        let list = || git(dir, &["worktree", "list", "--porcelain"]);
        let r =
            cmd(serde_json::json!({ "op": "worktreeLock", "path": wt, "reason": "on a USB disk" }));
        assert_eq!(r["ok"], true, "{r}");
        // 古い git の `worktree list --porcelain` は理由を出さないので、git が書くファイルを見る。
        let locked = dir.join(".git/worktrees/wt/locked");
        assert_eq!(
            std::fs::read_to_string(&locked).unwrap().trim(),
            "on a USB disk"
        );
        // ロックしていると git は移さない。
        let r = cmd(serde_json::json!({ "op": "worktreeMove", "path": wt, "to": moved }));
        assert_eq!(r["ok"], false);
        let r = cmd(serde_json::json!({ "op": "worktreeUnlock", "path": wt }));
        assert_eq!(r["ok"], true, "{r}");
        assert!(!locked.exists());
        let r = cmd(serde_json::json!({ "op": "worktreeMove", "path": wt, "to": moved }));
        assert_eq!(r["ok"], true, "{r}");
        assert!(std::path::Path::new(&moved).join("a.txt").exists());
        // 手で戻すと、つながりが切れる。repair に今の場所を渡すと直る（git 2.29 から）。
        std::fs::rename(&moved, &wt).unwrap();
        let r = cmd(serde_json::json!({ "op": "worktreeRepair", "paths": [wt] }));
        if r["ok"] == true {
            assert!(
                list().contains(&format!("worktree {wt}")) || list().contains("wt\n"),
                "{}",
                list()
            );
            assert_eq!(
                git(
                    std::path::Path::new(&wt),
                    &["rev-parse", "--abbrev-ref", "HEAD"]
                ),
                "side"
            );
        } else {
            assert!(r["error"].as_str().unwrap().contains("2.29"), "{r}");
        }
    }

    #[test]
    fn stashes_some_files_the_staged_changes_or_keeps_the_index() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "b"]);
        let stash = |json: serde_json::Value| {
            let mut command = json;
            command["op"] = "stashPush".into();
            command["includeUntracked"] = command
                .get("includeUntracked")
                .cloned()
                .unwrap_or(false.into());
            run(&repo, &command.to_string())
        };
        let status = || git(dir, &["status", "--porcelain"]);
        // ファイルを選ぶ: ほかのファイルの変更は残る。
        std::fs::write(dir.join("a.txt"), "A\n").unwrap();
        std::fs::write(dir.join("b.txt"), "B\n").unwrap();
        let r = stash(serde_json::json!({ "message": "only a", "paths": ["a.txt"] }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(status(), "M b.txt");
        assert!(git(dir, &["stash", "list"]).contains("only a"));
        git(dir, &["checkout", "--", "b.txt"]);
        // index を残す: stage した変更は作業ツリーと index に残る。
        std::fs::write(dir.join("a.txt"), "staged\n").unwrap();
        git(dir, &["add", "a.txt"]);
        std::fs::write(dir.join("b.txt"), "B\n").unwrap();
        let r = stash(serde_json::json!({ "keepIndex": true }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(status(), "M  a.txt");
        // stage した変更だけ（git 2.35 から）。それより前の git ではわけを言って断る。
        std::fs::write(dir.join("b.txt"), "B\n").unwrap();
        let r = stash(serde_json::json!({ "staged": true }));
        if r["ok"] == true {
            assert_eq!(status(), "M b.txt");
        } else {
            assert!(r["error"].as_str().unwrap().contains("2.35"), "{r}");
        }
        let r = stash(serde_json::json!({ "staged": true, "includeUntracked": true }));
        assert_eq!(r["ok"], false);
    }

    #[test]
    fn restores_files_from_another_commit() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.join("new.txt"), "new\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "second"]);
        let restore = |json: serde_json::Value| {
            let mut command = json;
            command["op"] = "restoreFiles".into();
            run(&repo, &command.to_string())
        };
        // 作業ツリーだけ（git の既定）: index は HEAD のまま。
        let r = restore(serde_json::json!({ "rev": "HEAD~1", "paths": ["a.txt"] }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\n2\n3\n"
        );
        assert_eq!(
            git(dir, &["status", "--porcelain", "--", "a.txt"]),
            "M a.txt"
        );
        // Undo で作業ツリーのファイルが戻る。
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "changed\n"
        );
        // index にも入れる。前の版にないファイルは消す。
        let r = restore(
            serde_json::json!({ "rev": "HEAD~1", "paths": ["a.txt", "new.txt"], "staged": true }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert!(!dir.join("new.txt").exists());
        let status = git(dir, &["status", "--porcelain"]);
        assert!(
            status.contains("M  a.txt") && status.contains("D  new.txt"),
            "{status}"
        );
        let r = restore(serde_json::json!({ "rev": "HEAD", "paths": [] }));
        assert_eq!(r["ok"], false);
    }

    #[test]
    fn adds_pulls_pushes_and_splits_subtrees() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let lib = tempfile::tempdir().unwrap();
        git(lib.path(), &["init", "-q"]);
        git(lib.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
        std::fs::write(lib.path().join("l.txt"), "1\n").unwrap();
        git(lib.path(), &["add", "."]);
        git(lib.path(), &["commit", "-q", "-m", "lib 1"]);
        let url = lib.path().to_str().unwrap().replace('\\', "/");
        let action = |json: serde_json::Value| {
            run(
                &repo,
                &serde_json::json!({ "op": "subtree", "action": json }).to_string(),
            )
        };
        let list = || run(&repo, r#"{"op":"subtrees"}"#)["info"].clone();
        // git subtree は contrib なので、ない git もある（ソースから作った git など）。
        if list()["available"] != true {
            assert_eq!(list()["subtrees"], serde_json::json!([]));
            let r = action(serde_json::json!({ "kind": "split", "dir": "a", "branch": "b" }));
            assert!(r["error"].as_str().unwrap().contains("subtree"), "{r}");
            return;
        }
        assert_eq!(list()["subtrees"], serde_json::json!([]));
        let r = action(
            serde_json::json!({ "kind": "add", "dir": "vendor/lib", "url": url, "ref": "main", "squash": true }),
        );
        assert_eq!(r["ok"], true, "{r}");
        let subtrees = list()["subtrees"].clone();
        assert_eq!(subtrees[0]["dir"], "vendor/lib");
        assert_eq!(subtrees[0]["squashed"], true);
        assert_eq!(subtrees[0]["url"], url.as_str());
        assert_eq!(subtrees[0]["ref"], "main");
        assert_eq!(subtrees[0]["squash"], true);
        // Undo で Add の前に戻り、一覧からも消える（読んだ HEAD の子孫でなくなったので読み直す）。
        let before = git(dir, &["rev-parse", "HEAD"]);
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_ne!(git(dir, &["rev-parse", "HEAD"]), before);
        assert_eq!(list()["subtrees"], serde_json::json!([]));
        let r = action(
            serde_json::json!({ "kind": "add", "dir": "vendor/lib", "url": url, "ref": "main", "squash": false }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(list()["subtrees"][0]["squashed"], false);

        std::fs::write(lib.path().join("l.txt"), "1\n2\n").unwrap();
        git(lib.path(), &["commit", "-q", "-am", "lib 2"]);
        let r = action(
            serde_json::json!({ "kind": "pull", "dir": "vendor/lib", "url": url, "ref": "main", "squash": false }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join("vendor/lib/l.txt")).unwrap(),
            "1\n2\n"
        );

        std::fs::write(dir.join("vendor/lib/m.txt"), "mine\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "change in lib"]);
        let r = action(
            serde_json::json!({ "kind": "push", "dir": "vendor/lib", "url": url, "ref": "topic" }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            git(lib.path(), &["log", "-1", "--format=%s", "topic"]),
            "change in lib"
        );
        assert_eq!(list()["subtrees"][0]["pushRef"], "topic");
        let r = action(
            serde_json::json!({ "kind": "split", "dir": "vendor/lib", "branch": "lib-only" }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            git(dir, &["rev-parse", "lib-only"]),
            git(lib.path(), &["rev-parse", "topic"])
        );
        let r = action(serde_json::json!({ "kind": "split", "dir": "--x", "branch": "y" }));
        assert_eq!(r["ok"], false);
        // 消したフォルダは出さない。
        git(dir, &["rm", "-q", "-r", "vendor/lib"]);
        git(dir, &["commit", "-q", "-m", "drop lib"]);
        assert_eq!(list()["subtrees"], serde_json::json!([]));
    }

    #[test]
    fn absorbs_submodule_git_dirs_and_runs_commands_in_each() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        // 自分の .git フォルダを持つ submodule（手で clone したものを足す）。
        let lib = tempfile::tempdir().unwrap();
        git(lib.path(), &["init", "-q"]);
        git(lib.path(), &["commit", "-q", "--allow-empty", "-m", "lib"]);
        let url = lib.path().to_str().unwrap().replace('\\', "/");
        git(dir, &["clone", "-q", &url, "lib"]);
        git(dir, &["submodule", "add", "-q", &url, "lib"]);
        let state = || run(&repo, r#"{"op":"submodules"}"#)["info"]["submodules"][0].clone();
        assert_eq!(state()["ownGitDir"], true, "{}", state());
        let change = |json: serde_json::Value| {
            run(
                &repo,
                &serde_json::json!({ "op": "changeSubmodule", "change": json }).to_string(),
            )
        };
        let r = change(serde_json::json!({ "kind": "absorb", "paths": ["lib"] }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(state()["ownGitDir"], false);
        assert!(dir.join(".git/modules/lib").is_dir());
        assert!(dir.join("lib/.git").is_file());
        let r = change(
            serde_json::json!({ "kind": "foreach", "command": "git log -1 --format=%s > ran.txt", "recursive": false }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join("lib/ran.txt"))
                .unwrap()
                .trim(),
            "lib"
        );
        let r =
            change(serde_json::json!({ "kind": "foreach", "command": " ", "recursive": false }));
        assert_eq!(r["ok"], false);
    }

    #[test]
    fn adds_inspects_changes_and_removes_submodules() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let lib = tempfile::tempdir().unwrap();
        git(lib.path(), &["init", "-q"]);
        git(lib.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
        std::fs::write(lib.path().join("l.txt"), "l\n").unwrap();
        git(lib.path(), &["add", "."]);
        git(lib.path(), &["commit", "-q", "-m", "lib"]);
        let url = lib.path().to_str().unwrap().replace('\\', "/");
        let change = |json: serde_json::Value| {
            run(
                &repo,
                &serde_json::json!({ "op": "changeSubmodule", "change": json }).to_string(),
            )
        };
        let submodules = || run(&repo, r#"{"op":"submodules"}"#)["info"]["submodules"].clone();
        assert_eq!(submodules(), serde_json::json!([]));

        let r =
            change(serde_json::json!({ "kind": "add", "url": url, "path": "lib", "branch": null }));
        assert_eq!(r["ok"], true, "{r}");
        let list = submodules();
        assert_eq!(list[0]["path"], "lib");
        assert_eq!(list[0]["name"], "lib");
        assert_eq!(list[0]["url"], url.as_str());
        assert_eq!(
            (
                &list[0]["initialized"],
                &list[0]["moved"],
                &list[0]["changed"]
            ),
            (
                &serde_json::json!(true),
                &serde_json::json!(false),
                &serde_json::json!(false)
            )
        );
        git(dir, &["commit", "-q", "-m", "add lib"]);

        // 中の変更と、記録と違うコミット。
        std::fs::write(dir.join("lib/new.txt"), "n\n").unwrap();
        assert_eq!(submodules()[0]["changed"], true);
        git(&dir.join("lib"), &["config", "user.name", "a"]);
        git(&dir.join("lib"), &["config", "user.email", "a@example.com"]);
        git(&dir.join("lib"), &["add", "."]);
        git(&dir.join("lib"), &["commit", "-q", "-m", "more"]);
        let list = submodules();
        assert_eq!(
            (&list[0]["moved"], &list[0]["changed"]),
            (&serde_json::json!(true), &serde_json::json!(false))
        );
        // update で記録したコミットに戻す。
        let r = run(&repo, r#"{"op":"updateSubmodules","paths":["lib"]}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(submodules()[0]["moved"], false);

        let r = change(serde_json::json!({ "kind": "setBranch", "path": "lib", "branch": "main" }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(submodules()[0]["branch"], "main");
        let r = change(serde_json::json!({ "kind": "setBranch", "path": "lib", "branch": null }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(submodules()[0]["branch"], serde_json::Value::Null);
        let r = change(
            serde_json::json!({ "kind": "setUrl", "path": "lib", "url": "https://example.com/lib.git" }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(submodules()[0]["url"], "https://example.com/lib.git");
        // .git/config にも写っている。
        assert_eq!(
            git(dir, &["config", "submodule.lib.url"]),
            "https://example.com/lib.git"
        );
        let r = change(serde_json::json!({ "kind": "sync", "recursive": true }));
        assert_eq!(r["ok"], true, "{r}");

        let r =
            change(serde_json::json!({ "kind": "remove", "path": "nope", "deleteGitDir": false }));
        assert_eq!(r["ok"], false);
        // .gitmodules に stage していない変更があれば、何もせずに断る。
        let r =
            change(serde_json::json!({ "kind": "remove", "path": "lib", "deleteGitDir": true }));
        assert!(r["error"].as_str().unwrap().contains(".gitmodules"), "{r}");
        assert_eq!(submodules()[0]["initialized"], true);
        git(dir, &["commit", "-q", "-am", "url"]);
        // 既定は .git/modules を残す。消すと選べば消す。
        let modules = dir.join(".git/modules/lib");
        assert!(modules.is_dir());
        let r =
            change(serde_json::json!({ "kind": "remove", "path": "lib", "deleteGitDir": true }));
        assert_eq!(r["ok"], true, "{r}");
        assert!(!modules.exists());
        assert!(!dir.join("lib").exists());
        assert_eq!(submodules(), serde_json::json!([]));
    }

    #[test]
    fn edits_local_config_remotes_and_ignore_files() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        assert_eq!(
            run(
                &repo,
                r#"{"op":"configSet","key":"pull.rebase","value":"true"}"#
            )["ok"],
            true
        );
        let list = run(&repo, r#"{"op":"configList"}"#);
        let entries = list["info"]["entries"].as_array().unwrap();
        // 全体の設定（Windows の CI には system の pull.rebase がある）ではなく、このリポジトリの設定を見る。
        let pull = entries
            .iter()
            .find(|e| e["key"] == "pull.rebase" && e["scope"] == "local")
            .unwrap();
        assert_eq!(
            (pull["scope"].as_str(), pull["value"].as_str()),
            (Some("local"), Some("true"))
        );
        assert!(pull["origin"].as_str().unwrap().ends_with("config"));
        // 消す（もともとないものを消しても失敗しない）。
        for _ in 0..2 {
            assert_eq!(
                run(
                    &repo,
                    r#"{"op":"configSet","key":"pull.rebase","value":null}"#
                )["ok"],
                true
            );
        }
        let list = run(&repo, r#"{"op":"configList"}"#);
        let keys = list["info"]["entries"].as_array().unwrap();
        assert!(
            !keys
                .iter()
                .any(|e| e["key"] == "pull.rebase" && e["scope"] == "local")
        );

        // いくつも値を持てる設定は、`values` ですべて書き戻せる（消した後の Undo）。1 つの値で上書きしていても。
        let fetch = |repo: &Repository| -> Vec<String> {
            run(repo, r#"{"op":"configList"}"#)["info"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["key"] == "remote.x.fetch" && e["scope"] == "local")
                .map(|e| e["value"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            run(
                &repo,
                r#"{"op":"configSet","key":"remote.x.fetch","value":"a"}"#
            )["ok"],
            true
        );
        let r = run(
            &repo,
            r#"{"op":"configSet","key":"remote.x.fetch","values":["b","c"]}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(fetch(&repo), ["b", "c"]);
        let r = run(
            &repo,
            r#"{"op":"configSet","key":"remote.x.fetch","value":null}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert!(fetch(&repo).is_empty());

        // リモートにつないで確かめる: 自分自身を URL にすると、ブランチ 1 つと HEAD が読める。
        let check = run(
            &repo,
            &format!(
                r#"{{"op":"checkRemote","url":{}}}"#,
                serde_json::json!(dir.to_str().unwrap())
            ),
        );
        assert_eq!(check["info"]["branches"], 1, "{check}");
        assert_eq!(check["info"]["head"], "main", "{check}");
        assert_eq!(
            run(&repo, r#"{"op":"checkRemote","url":"/nonexistent/repo"}"#)["ok"],
            false
        );

        // フックの実行の許可: .git/hooks に置いたフックを有効・無効にできる（Windows には実行権限がない）。
        #[cfg(unix)]
        {
            std::fs::write(dir.join(".git/hooks/pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
            let hooks = |on: bool| {
                let r = run(&repo, r#"{"op":"hooksAndLfs"}"#);
                let h = r["info"]["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|h| h["name"] == "pre-commit")
                    .cloned()
                    .unwrap();
                assert_eq!(h["enabled"], on, "{r}");
            };
            hooks(false);
            assert_eq!(
                run(
                    &repo,
                    r#"{"op":"setHookEnabled","name":"pre-commit","enabled":true}"#
                )["ok"],
                true
            );
            hooks(true);
            assert_eq!(
                run(
                    &repo,
                    r#"{"op":"setHookEnabled","name":"pre-commit","enabled":false}"#
                )["ok"],
                true
            );
            hooks(false);
            assert_eq!(
                run(
                    &repo,
                    r#"{"op":"setHookEnabled","name":"pre-push","enabled":true}"#
                )["ok"],
                false
            );
        }

        // フックの作成: 雛形を置き、一覧に実行可で出る。二度目はエラー。
        assert_eq!(
            run(&repo, r#"{"op":"createHook","name":"commit-msg"}"#)["ok"],
            true
        );
        let r = run(&repo, r#"{"op":"hooksAndLfs"}"#);
        let created = r["info"]["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["name"] == "commit-msg")
            .cloned()
            .unwrap();
        assert_eq!(created["enabled"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join(".git/hooks/commit-msg")).unwrap(),
            "#!/bin/sh\n\n"
        );
        assert_eq!(
            run(&repo, r#"{"op":"hook","name":"commit-msg"}"#)["info"]["text"],
            "#!/bin/sh\n\n"
        );
        assert_eq!(
            run(&repo, r#"{"op":"hook","name":"pre-rebase"}"#)["info"]["text"],
            serde_json::Value::Null
        );
        assert_eq!(
            run(&repo, r#"{"op":"createHook","name":"commit-msg"}"#)["ok"],
            false
        );
        assert_eq!(
            run(&repo, r#"{"op":"createHook","name":"nope"}"#)["ok"],
            false
        );

        // リポジトリ専用のコミットのテンプレート: 書くと commit.template がそれを指す。
        assert_eq!(
            run(&repo, r#"{"op":"commitTemplate"}"#)["info"]["text"],
            serde_json::Value::Null
        );
        assert_eq!(
            run(&repo, r#"{"op":"saveCommitTemplate","text":"{issue}: \n"}"#)["ok"],
            true
        );
        let t = run(&repo, r#"{"op":"commitTemplate"}"#);
        assert_eq!(t["info"]["text"], "{issue}: \n");
        let path = t["info"]["path"].as_str().unwrap().to_owned();
        assert!(path.ends_with("commit-template"), "{path}");
        let list = run(&repo, r#"{"op":"configList"}"#);
        let set = list["info"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["key"] == "commit.template" && e["scope"] == "local")
            .unwrap();
        assert_eq!(set["value"], path);

        // 署名の鍵: 知らない形式は断る（形式ごとの一覧は環境次第なので、ここでは読まない）。
        let r = run(&repo, r#"{"op":"signingKeys","format":"smime"}"#);
        assert!(r["error"].as_str().unwrap().contains("smime"), "{r}");

        // git が知っている変数の名前（設定の有無は問わない）。名前が入るものはない。
        let known = run(&repo, r#"{"op":"configKeys"}"#);
        let known = known["info"]["keys"].as_array().unwrap();
        assert!(known.iter().any(|k| k == "pull.rebase"), "{known:?}");
        assert!(known.iter().any(|k| k == "core.fileMode"), "{known:?}");
        assert!(!known.iter().any(|k| k.as_str().unwrap().ends_with('.')));

        let change = |json: &str| {
            run(
                &repo,
                &format!(r#"{{"op":"changeRemote","change":{json}}}"#),
            )
        };
        assert_eq!(
            change(r#"{"kind":"add","name":"up","url":"https://example.com/a.git"}"#)["ok"],
            true
        );
        assert_eq!(
            change(r#"{"kind":"rename","from":"up","to":"origin"}"#)["ok"],
            true
        );
        assert_eq!(
            change(r#"{"kind":"setUrl","name":"origin","url":"git@example.com:a.git"}"#)["ok"],
            true
        );
        let remotes = run(&repo, r#"{"op":"remotes"}"#);
        assert_eq!(remotes["info"]["remotes"][0]["name"], "origin");
        assert_eq!(
            remotes["info"]["remotes"][0]["fetchUrl"],
            "git@example.com:a.git"
        );
        assert_eq!(change(r#"{"kind":"remove","name":"origin"}"#)["ok"], true);
        assert_eq!(
            run(&repo, r#"{"op":"remotes"}"#)["info"]["remotes"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let r = run(
            &repo,
            r#"{"op":"saveIgnoreFiles","gitignore":"target/\n","exclude":"*.local\n"}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "target/\n"
        );
        let files = run(&repo, r#"{"op":"ignoreFiles"}"#);
        assert_eq!(files["info"]["exclude"], "*.local\n");

        // ユーザー全体の無視するパターンのファイル（ここでは `core.excludesFile` で場所を決める）。
        let excludes = dir.join("global-ignore");
        git(
            dir,
            &["config", "core.excludesFile", excludes.to_str().unwrap()],
        );
        let before = run(&repo, r#"{"op":"excludesFile"}"#);
        assert_eq!(before["info"]["path"], excludes.to_str().unwrap());
        assert!(before["info"]["text"].is_null());
        let r = run(&repo, r#"{"op":"saveExcludesFile","text":"*.swp\n"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(std::fs::read_to_string(&excludes).unwrap(), "*.swp\n");
        assert_eq!(
            run(&repo, r#"{"op":"excludesFile"}"#)["info"]["text"],
            "*.swp\n"
        );
    }

    #[test]
    fn fast_forwards_a_branch_that_is_not_checked_out() {
        let (upstream, _) = setup();
        git(upstream.path(), &["branch", "feat"]);
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("clone");
        git(
            tmp.path(),
            &["clone", "-q", upstream.path().to_str().unwrap(), "clone"],
        );
        for (key, value) in [("user.name", "a"), ("user.email", "a@example.com")] {
            git(&dir, &["config", key, value]);
        }
        git(&dir, &["branch", "-q", "feat", "origin/feat"]);
        git(&dir, &["branch", "-q", "solo"]);
        // 上流の feat と main を進めて取ってくる。
        git(upstream.path(), &["checkout", "-q", "feat"]);
        git(
            upstream.path(),
            &["commit", "-q", "--allow-empty", "-m", "feat"],
        );
        git(upstream.path(), &["checkout", "-q", "main"]);
        git(
            upstream.path(),
            &["commit", "-q", "--allow-empty", "-m", "main"],
        );
        git(&dir, &["fetch", "-q"]);
        let repo = Repository::new(GixBackend::open(&dir).unwrap());
        let head = git(&dir, &["rev-parse", "HEAD"]);

        let r = run(&repo, r#"{"op":"fastForward","branch":"feat"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            git(&dir, &["rev-parse", "feat"]),
            git(&dir, &["rev-parse", "origin/feat"])
        );
        // チェックアウト中のブランチは動かさない。
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(git(&dir, &["rev-parse", "feat"]), head);

        // チェックアウト中（Pull を使う）、上流がない、分かれているものは断る。
        for json in [
            r#"{"op":"fastForward","branch":"main"}"#,
            r#"{"op":"fastForward","branch":"solo"}"#,
        ] {
            assert_eq!(run(&repo, json)["ok"], false, "{json}");
        }
        git(&dir, &["checkout", "-q", "feat"]);
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "local"]);
        git(&dir, &["checkout", "-q", "main"]);
        let r = run(&repo, r#"{"op":"fastForward","branch":"feat"}"#);
        assert_eq!(r["ok"], false, "{r}");
        // ほかの worktree でチェックアウト中のものも断る（その worktree の作業ツリーとずれるため）。
        git(&dir, &["branch", "-q", "-f", "feat", &head]);
        git(
            &dir,
            &[
                "worktree",
                "add",
                "-q",
                tmp.path().join("wt").to_str().unwrap(),
                "feat",
            ],
        );
        let r = run(&repo, r#"{"op":"fastForward","branch":"feat"}"#);
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!(git(&dir, &["rev-parse", "feat"]), head);
    }

    #[test]
    fn pulls_by_merge_rebase_or_fast_forward_only() {
        let (upstream, _) = setup();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("clone");
        git(
            tmp.path(),
            &["clone", "-q", upstream.path().to_str().unwrap(), "clone"],
        );
        for (key, value) in [("user.name", "a"), ("user.email", "a@example.com")] {
            git(&dir, &["config", key, value]);
        }
        // 上流と手元の両方で進め、分かれさせる。
        std::fs::write(upstream.path().join("b.txt"), "b\n").unwrap();
        git(upstream.path(), &["add", "."]);
        git(upstream.path(), &["commit", "-q", "-m", "upstream"]);
        std::fs::write(dir.join("c.txt"), "c\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "local"]);
        let repo = Repository::new(GixBackend::open(&dir).unwrap());

        // 分かれていると fast-forward だけでは取り込めない。
        let r = run(&repo, r#"{"op":"pull","mode":"fastForwardOnly"}"#);
        assert_eq!(r["ok"], false, "{r}");
        let r = run(&repo, r#"{"op":"pull","mode":"rebase"}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        // rebase なので merge コミットはなく、手元のコミットが上流の上に載る。
        assert_eq!(git(&dir, &["log", "--format=%s"]), "local\nupstream\nfirst");
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        // mode を省くと git の設定に従う（端末の `git pull` と同じ）。pull.rebase=false なら merge。
        git(&dir, &["config", "pull.rebase", "false"]);
        let r = run(&repo, r#"{"op":"pull"}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(git(&dir, &["rev-list", "--count", "--merges", "HEAD"]), "1");
    }

    #[test]
    fn resolves_merge_conflicts() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "a.txt", "1\nTOPIC\n3\n");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "a.txt", "1\nMAIN\n3\n");
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );

        let v = repo
            .conflict_view("a.txt", &FrontendProfile::MACOS)
            .unwrap();
        assert_eq!(v.removed, 1, "コンフリクトの数");
        let r = run(
            &repo,
            r#"{"op":"resolveConflict","path":"a.txt","index":0,"choice":"theirs"}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nTOPIC\n3\n"
        );
        // 解決は Undo できる。
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .contains("<<<<<<<")
        );

        assert_eq!(
            run(&repo, r#"{"op":"takeSide","path":"a.txt","ours":true}"#)["ok"],
            true
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nMAIN\n3\n"
        );
        let status = repo.load_status().unwrap();
        assert!(
            status
                .status
                .entries
                .iter()
                .all(|e| e.kind != amonhen_model::StatusKind::Unmerged)
        );
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(
            run(&repo, r#"{"op":"sequence"}"#)["text"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn tells_conflicts_without_markers_and_rerere_resolutions() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["config", "rerere.enabled", "true"]);
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "a.txt", "1\nTOPIC A\n3\n");
        commit_file(dir, "b.txt", "1\nTOPIC B\n3\n");
        commit_file(dir, "c.txt", "1\nTOPIC C\n3\n");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "a.txt", "1\nMAIN A\n3\n");
        commit_file(dir, "b.txt", "1\nMAIN B\n3\n");
        commit_file(dir, "c.txt", "1\nMAIN C\n3\n");
        // 1 回目: a.txt だけ解決し、rerere に覚えさせてやめる。
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );
        std::fs::write(dir.join("a.txt"), "1\nBOTH A\n3\n").unwrap();
        git(dir, &["rerere"]);
        git(dir, &["merge", "--abort"]);
        // 2 回目: rerere が a.txt を解く。b.txt はほかのエディタで直した。c.txt はそのまま。
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nBOTH A\n3\n"
        );
        std::fs::write(dir.join("b.txt"), "1\nBOTH B\n3\n").unwrap();
        repo.load_status().unwrap();
        let files = run(&repo, r#"{"op":"conflictFiles"}"#);
        assert_eq!(
            files["info"]["files"],
            serde_json::json!([
                {"path": "a.txt", "markers": 0, "rerere": true},
                {"path": "b.txt", "markers": 0, "rerere": false},
                {"path": "c.txt", "markers": 1, "rerere": false},
            ]),
            "{files}"
        );
        // マーカーのないファイルのマージエディタは、左右に index の両側を出す。
        let sides = run(&repo, r#"{"op":"mergeSides","path":"a.txt"}"#);
        assert_eq!(
            sides["info"]["current"]["text"], "1\nMAIN A\n3\n",
            "{sides}"
        );
        assert_eq!(sides["info"]["incoming"]["text"], "1\nTOPIC A\n3\n");
        assert_eq!(sides["info"]["result"], "1\nBOTH A\n3\n");
        assert_eq!(sides["info"]["markers"], 0);
        assert_eq!(sides["info"]["rerere"], true);
        let sides = run(&repo, r#"{"op":"mergeSides","path":"c.txt"}"#);
        assert_eq!(
            sides["info"]["current"]["text"], "1\nMAIN C\n3\n",
            "{sides}"
        );
        assert_eq!(sides["info"]["markers"], 1);
        assert_eq!(sides["info"]["rerere"], false);
    }

    #[test]
    fn does_not_open_files_that_are_not_utf8_in_the_merge_editor() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let write = |name: &str, bytes: &[u8], message: &str| {
            std::fs::write(dir.join(name), bytes).unwrap();
            git(dir, &["add", name]);
            git(dir, &["commit", "-q", "-m", message]);
        };
        write("a.png", b"\x89PNG\0\xff base", "base png");
        write("sjis.txt", b"\x82\xa0 base\n", "base sjis");
        write("zero.dat", b"a\0 base", "base nul");
        git(dir, &["switch", "-q", "-c", "topic"]);
        write("a.png", b"\x89PNG\0\xff topic", "topic png");
        write("sjis.txt", b"\x82\xa0 topic\n", "topic sjis");
        write("zero.dat", b"a\0 topic", "topic nul");
        git(dir, &["switch", "-q", "-"]);
        write("a.png", b"\x89PNG\0\xff main", "main png");
        write("sjis.txt", b"\x82\xa0 main\n", "main sjis");
        write("zero.dat", b"a\0 main", "main nul");
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );
        repo.load_status().unwrap();
        // マーカーのないバイナリは「マーカーのないコンフリクト」に数えない。Shift-JIS はマーカーがあるので載る。
        let files = run(&repo, r#"{"op":"conflictFiles"}"#);
        assert_eq!(
            files["info"]["files"],
            serde_json::json!([{"path": "sjis.txt", "markers": 1, "rerere": false}]),
            "{files}"
        );
        for path in ["a.png", "sjis.txt", "zero.dat"] {
            let command = serde_json::json!({ "op": "mergeSides", "path": path }).to_string();
            let sides = run(&repo, &command);
            assert!(
                sides["error"].as_str().unwrap().contains("UTF-8"),
                "{sides}"
            );
        }
        // 片側を選ぶのは、バイトのまま。
        let taken = run(&repo, r#"{"op":"takeSide","path":"a.png","ours":false}"#);
        assert_eq!(taken["ok"], true, "{taken}");
        assert_eq!(
            std::fs::read(dir.join("a.png")).unwrap(),
            b"\x89PNG\0\xff topic"
        );
    }

    #[test]
    fn leaves_out_conflicts_that_markers_cannot_show() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        commit_file(dir, "a.txt", "1\n");
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "a.txt", "TOPIC\n");
        git(dir, &["switch", "-q", "main"]);
        git(dir, &["rm", "-q", "a.txt"]);
        git(dir, &["commit", "-q", "-m", "remove"]);
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );
        repo.load_status().unwrap();
        // 片側で消したコンフリクト（DU）にはマーカーがないが、解決済みではない。
        let files = run(&repo, r#"{"op":"conflictFiles"}"#);
        assert_eq!(files["info"]["files"], serde_json::json!([]), "{files}");
    }

    #[test]
    fn merge_editor_reads_both_sides_resolves_text_and_saves() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "a.txt", "1\nTOPIC\n3\n");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "a.txt", "1\nMAIN\n3\n");
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );

        let sides = run(&repo, r#"{"op":"mergeSides","path":"a.txt"}"#);
        let info = &sides["info"];
        assert_eq!(info["current"]["text"], "1\nMAIN\n3\n", "{sides}");
        assert_eq!(info["incoming"]["text"], "1\nTOPIC\n3\n");
        assert_eq!(info["current"]["regions"], serde_json::json!([[1, 2]]));
        assert_eq!(
            info["base"],
            serde_json::Value::Null,
            "diff3 でなければ共通の祖先はない"
        );
        let result = info["result"].as_str().unwrap().to_owned();

        let command = |op: serde_json::Value| run(&repo, &op.to_string());
        let regions = command(serde_json::json!({"op": "conflictRegions", "text": result}));
        let conflict = &regions["info"]["conflicts"][0];
        assert_eq!(conflict["lines"], serde_json::json!([1, 6]));
        assert_eq!(
            (conflict["current"].as_str(), conflict["incoming"].as_str()),
            (Some("MAIN\n"), Some("TOPIC\n"))
        );
        // 最初のコンフリクトを渡すと、手で書き換えたものもどれに当たるかを返す。
        let edited = command(serde_json::json!({
            "op": "conflictRegions",
            "text": result.replace("TOPIC\n", "TOPIC_EDIT\n"),
            "originals": [["MAIN\n", "TOPIC\n"]],
        }));
        assert_eq!(edited["info"]["conflicts"][0]["original"], 0, "{edited}");
        let both = command(
            serde_json::json!({"op": "resolveText", "text": result, "index": 0, "choice": "both"}),
        );
        assert_eq!(both["text"], "1\nMAIN\nTOPIC\n3\n");
        // ファイルにはまだ書かない。
        assert!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .contains("<<<<<<<")
        );

        let saved = command(
            serde_json::json!({"op": "saveMerge", "path": "a.txt", "text": "1\nMERGED\n3\n", "stage": true}),
        );
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nMERGED\n3\n"
        );
        let status = repo.load_status().unwrap();
        assert!(
            status
                .status
                .entries
                .iter()
                .all(|e| e.kind != amonhen_model::StatusKind::Unmerged)
        );
        assert_eq!(run(&repo, r#"{"op":"undoPeek"}"#)["label"], "Merge a.txt");
    }

    #[test]
    fn blames_lines_and_follows_a_renamed_file() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        commit_file(dir, "old.txt", "one\ntwo\n");
        git(dir, &["mv", "old.txt", "new.txt"]);
        git(dir, &["commit", "-q", "-m", "rename"]);
        commit_file(dir, "new.txt", "one\nTWO\n");
        std::fs::write(dir.join("new.txt"), "one\nTWO\nthree\n").unwrap();

        let blame = run(&repo, r#"{"op":"blame","path":"new.txt"}"#);
        let lines = blame["info"]["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 3, "{blame}");
        let commits = blame["info"]["commits"].as_array().unwrap();
        let oid_of = |i: usize| {
            commits[lines[i]["c"].as_u64().unwrap() as usize]["oid"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(lines[1]["t"], "TWO");
        assert_ne!(oid_of(0), oid_of(1), "1 行目と 2 行目は別のコミット");
        assert!(
            oid_of(2).bytes().all(|b| b == b'0'),
            "まだコミットしていない行"
        );

        let history = run(&repo, r#"{"op":"fileHistory","path":"new.txt"}"#);
        let entries = history["info"]["entries"].as_array().unwrap();
        let paths: Vec<&str> = entries
            .iter()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths.first(), Some(&"new.txt"), "{history}");
        assert_eq!(paths.last(), Some(&"old.txt"), "名前を変える前も追う");
    }

    #[test]
    fn follows_a_file_renamed_twice_across_a_merge() {
        // a.txt は根のコミット（first）で生まれる。
        let (tmp, repo) = setup();
        let dir = tmp.path();
        commit_file(dir, "a.txt", "second");
        git(dir, &["mv", "a.txt", "b.txt"]);
        git(dir, &["commit", "-q", "-m", "to b"]);
        git(dir, &["switch", "-q", "-c", "side"]);
        commit_file(dir, "b.txt", "side");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "other.txt", "other");
        git(dir, &["merge", "-q", "--no-ff", "-m", "merge", "side"]);
        git(dir, &["mv", "b.txt", "c.txt"]);
        git(dir, &["commit", "-q", "-m", "to c"]);
        commit_file(dir, "c.txt", "last");

        let history = run(&repo, r#"{"op":"fileHistory","path":"c.txt"}"#);
        let entries = history["info"]["entries"].as_array().unwrap();
        let got: Vec<(&str, &str)> = entries
            .iter()
            .map(|e| (e["subject"].as_str().unwrap(), e["path"].as_str().unwrap()))
            .collect();
        assert_eq!(
            got,
            [
                ("last", "c.txt"),
                ("to c", "c.txt"),
                ("side", "b.txt"),
                ("to b", "b.txt"),
                ("second", "a.txt"),
                ("first", "a.txt"),
            ],
            "マージは出さない"
        );
        let oids: Vec<&str> = entries.iter().map(|e| e["oid"].as_str().unwrap()).collect();
        let follow = git(
            dir,
            &[
                "log",
                "--follow",
                "--no-merges",
                "--format=%H",
                "--",
                "c.txt",
            ],
        );
        assert_eq!(oids, follow.lines().collect::<Vec<_>>(), "--follow と同じ");
        assert_eq!(history["info"]["more"], false);

        // 件数を限ると、改名をまたいでその数で止め、その先があるかを返す。
        for (limit, more) in [(1, true), (4, true), (6, false), (9, false)] {
            let json = serde_json::json!({"op": "fileHistory", "path": "c.txt", "limit": limit});
            let part = run(&repo, &json.to_string());
            let subjects: Vec<&str> = part["info"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["subject"].as_str().unwrap())
                .collect();
            let want: Vec<&str> = got.iter().take(limit).map(|(s, _)| *s).collect();
            assert_eq!(subjects, want, "{limit}");
            assert_eq!(part["info"]["more"], more, "{limit}");
        }
    }

    #[test]
    fn shows_merges_in_file_history_only_when_asked() {
        // 両方の枝で f.txt の別の行を変えて merge する（merge の中身はどちらの親とも違う）。その後で名前を変える。
        let (tmp, repo) = setup();
        let dir = tmp.path();
        commit_file(dir, "f.txt", "1\n2\n3\n4\n5\n6\n");
        git(dir, &["switch", "-q", "-c", "side"]);
        commit_file(dir, "f.txt", "one\n2\n3\n4\n5\n6\n");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "f.txt", "1\n2\n3\n4\n5\nsix\n");
        git(dir, &["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
        git(dir, &["mv", "f.txt", "g.txt"]);
        git(dir, &["commit", "-q", "-m", "rename"]);

        let subjects = |json: &str| -> Vec<(String, String)> {
            run(&repo, json)["info"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["subject"].as_str().unwrap().to_owned(),
                        e["path"].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        };
        let without = subjects(r#"{"op":"fileHistory","path":"g.txt"}"#);
        assert!(
            !without.iter().any(|(s, _)| s == "merge side"),
            "{without:?}"
        );
        let with = subjects(r#"{"op":"fileHistory","path":"g.txt","merges":true}"#);
        assert_eq!(with.len(), without.len() + 1, "{with:?}");
        assert!(
            with.contains(&("merge side".to_owned(), "f.txt".to_owned())),
            "マージにはその時点のパスを添える: {with:?}"
        );
    }

    #[test]
    fn follows_a_file_copied_from_one_changed_in_the_same_commit() {
        // 訳す前の README を README.ja.md に移し、README は書き直す（ファイルとしては変更とコピー）。
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let text = "one\ntwo\nthree\nfour\nfive\n";
        commit_file(dir, "README", text);
        std::fs::write(dir.join("README.ja"), format!("{text}six\n")).unwrap();
        std::fs::write(dir.join("README"), "translated\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "translate"]);

        let history = run(&repo, r#"{"op":"fileHistory","path":"README.ja"}"#);
        let paths: Vec<&str> = history["info"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, ["README.ja", "README"], "{history}");
    }

    #[test]
    fn fast_forwards_a_branch_without_checking_it_out() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["branch", "old"]);
        commit_file(dir, "b.txt", "b");
        let head = git(dir, &["rev-parse", "HEAD"]);
        let r = run(&repo, r#"{"op":"mergeInto","target":"old","rev":"main"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(git(dir, &["rev-parse", "old"]), head);
        assert_eq!(
            git(dir, &["symbolic-ref", "--short", "HEAD"]),
            "main",
            "checkout しない"
        );
        run(&repo, r#"{"op":"undo"}"#);
        assert_ne!(git(dir, &["rev-parse", "old"]), head);
    }

    /// main から分かれた a・b（それぞれ自分のファイルを変える）と、main と同じ行を変える x。
    fn octopus_setup() -> (tempfile::TempDir, Repository) {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        for (name, file) in [("a", "a-file"), ("b", "b-file"), ("x", "a.txt")] {
            git(dir, &["switch", "-q", "-c", name, "main"]);
            commit_file(dir, file, name);
        }
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "a.txt", "main\n");
        (tmp, repo)
    }

    #[test]
    fn merges_several_branches_in_one_commit_and_undoes_it() {
        let (tmp, repo) = octopus_setup();
        let dir = tmp.path();
        let head = git(dir, &["rev-parse", "HEAD"]);
        let r = run(&repo, r#"{"op":"merge","rev":["a","b"]}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(
            git(dir, &["log", "-1", "--format=%P"]).split(' ').count(),
            3
        );
        // git の既定のメッセージ（古い git は main にも " into main" を付ける）。
        assert!(git(dir, &["log", "-1", "--format=%s"]).starts_with("Merge branches 'a' and 'b'"));
        let r = run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), head);
    }

    #[test]
    fn names_every_branch_of_an_octopus_stopped_at_a_conflict() {
        let (tmp, repo) = octopus_setup();
        let r = run(&repo, r#"{"op":"merge","rev":["a","b","x"]}"#);
        assert_eq!(r["outcome"], "conflict", "{r}");
        let r = run(&repo, r#"{"op":"sequence"}"#);
        assert_eq!(r["info"]["from"], "a", "{r}");
        assert_eq!(
            r["info"]["merging"],
            serde_json::json!(["a", "b", "x"]),
            "{r}"
        );
        run(&repo, r#"{"op":"abort"}"#);
        assert!(git(tmp.path(), &["status", "--porcelain"]).is_empty());
    }

    #[test]
    fn says_when_an_octopus_gives_up_and_changes_nothing() {
        let (tmp, repo) = octopus_setup();
        let dir = tmp.path();
        let head = git(dir, &["rev-parse", "HEAD"]);
        let r = run(&repo, r#"{"op":"merge","rev":["a","x","b"]}"#);
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!(r["errorKind"], "octopus", "{r}");
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), head);
        assert!(git(dir, &["status", "--porcelain"]).is_empty());
        assert_eq!(
            run(&repo, r#"{"op":"undoPeek"}"#)["label"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn merges_one_by_one_until_a_conflict() {
        let (tmp, repo) = octopus_setup();
        let dir = tmp.path();
        let head = git(dir, &["rev-parse", "HEAD"]);
        let r = run(
            &repo,
            r#"{"op":"merge","rev":["a","x","b"],"oneByOne":true}"#,
        );
        assert_eq!(r["outcome"], "conflict", "{r}");
        assert_eq!(r["info"]["remaining"], serde_json::json!(["b"]), "{r}");
        // a は merge コミットになり、x で止まっている。
        assert!(git(dir, &["log", "-1", "--format=%s"]).starts_with("Merge branch 'a'"));
        run(&repo, r#"{"op":"abort"}"#);
        // 済んだ分は 1 回の Undo で戻る。
        let r = run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), head);

        let r = run(&repo, r#"{"op":"merge","rev":["a","b"],"oneByOne":true}"#);
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(git(dir, &["rev-list", "--count", "--merges", "HEAD"]), "2");
    }

    #[test]
    fn predicts_each_branch_of_an_octopus() {
        let (_tmp, repo) = octopus_setup();
        let r = run(&repo, r#"{"op":"predictMerge","theirs":["a","x","b"]}"#);
        if amonhen_backend_cli::git_version().is_none_or(|(a, b, _)| (a, b) >= (2, 38)) {
            assert_eq!(r["info"]["conflicts"], serde_json::json!(["a.txt"]), "{r}");
            assert_eq!(r["info"]["steps"][1]["rev"], "x", "{r}");
            assert_eq!(
                r["info"]["steps"][1]["conflicts"],
                serde_json::json!(["a.txt"])
            );
            assert_eq!(r["info"]["steps"][2]["conflicts"], serde_json::json!([]));
        } else {
            assert!(r["info"]["steps"].is_null(), "{r}");
        }
    }

    #[test]
    fn does_not_fast_forward_a_branch_checked_out_in_another_worktree() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["branch", "old"]);
        commit_file(dir, "b.txt", "b");
        let old = git(dir, &["rev-parse", "old"]);
        let wt = tempfile::tempdir().unwrap();
        let path = wt.path().join("wt");
        git(
            dir,
            &["worktree", "add", "-q", path.to_str().unwrap(), "old"],
        );
        let r = run(&repo, r#"{"op":"mergeInto","target":"old","rev":"main"}"#);
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!(git(dir, &["rev-parse", "old"]), old);
    }

    #[test]
    fn commits_and_discards_lines_undoably() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        let status = repo.load_status().unwrap();
        let diff = repo
            .working_diff(&status.status.entries[0], false, &FrontendProfile::MACOS)
            .unwrap();
        // @@, -1, +one, 2, -3, +three: 1 → one だけ戻す。
        repo.discard_rows(&diff, &[1, 2]).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\n2\nthree\n"
        );
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "one\n2\nthree\n"
        );

        repo.stage(&["a.txt"]).unwrap();
        repo.commit("second", false).unwrap();
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["label"], "Commit");
        assert_eq!(git(dir, &["log", "--format=%s"]), "first");
        assert_eq!(git(dir, &["diff", "--cached", "--name-only"]), "a.txt");
    }

    /// 改行を変換するリポジトリ。`committed` を変換なしで commit し、`config` を設定してから開く
    /// （gix は開いたときの設定を使う）。
    fn setup_converting(
        config: &[(&str, &str)],
        committed: &[u8],
    ) -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q"]);
        git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        std::fs::write(dir.join("a.txt"), committed).unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        for (key, value) in config {
            git(dir, &["config", key, value]);
        }
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        (tmp, repo)
    }

    fn index_blob(dir: &std::path::Path) -> Vec<u8> {
        Process::new("git")
            .args(["cat-file", "blob", ":a.txt"])
            .current_dir(dir)
            .output()
            .unwrap()
            .stdout
    }

    fn unstaged_diff(repo: &Repository) -> crate::WorkingDiff {
        let status = repo.load_status().unwrap();
        repo.working_diff(&status.status.entries[0], false, &FrontendProfile::MACOS)
            .unwrap()
    }

    #[test]
    fn diffs_and_stages_lines_through_autocrlf() {
        // Windows の既定（core.autocrlf=true）: 作業ツリーは CRLF、index は LF。
        let (tmp, repo) = setup_converting(&[("core.autocrlf", "true")], b"1\n2\n3\n");
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "1\r\nTWO\r\n3\r\n4\r\n").unwrap();
        let diff = unstaged_diff(&repo);
        assert_eq!(
            (diff.file.added, diff.file.removed),
            (2, 1),
            "改行の違いは差分にしない"
        );
        // @@, 1, -2, +TWO, 3, +4: 4 だけを stage する。index に CR を入れない。
        repo.stage_rows(&diff, &[5]).unwrap();
        assert_eq!(index_blob(dir), b"1\n2\n3\n4\n");
    }

    #[test]
    fn discards_lines_keeping_the_other_line_endings() {
        // 改行が混ざったファイル（2 行目だけ LF）。選んだ行のほかは 1 バイトも変えない。
        let (tmp, repo) = setup_converting(&[("core.autocrlf", "true")], b"1\n2\n3\n4\n");
        let dir = tmp.path();
        let before = b"1\r\nTWO\n3\r\nFOUR\r\n";
        std::fs::write(dir.join("a.txt"), before).unwrap();
        let diff = unstaged_diff(&repo);
        // @@, 1, -2, +TWO, 3, -4, +FOUR: 4 → FOUR だけ戻す。戻す行は checkout と同じ CRLF。
        repo.discard_rows(&diff, &[5, 6]).unwrap();
        assert_eq!(
            std::fs::read(dir.join("a.txt")).unwrap(),
            b"1\r\nTWO\n3\r\n4\r\n"
        );
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), before);
    }

    #[test]
    fn discards_lines_in_a_file_whose_endings_differ_from_its_eol_attribute() {
        // `eol=lf` なのに作業ツリーは CRLF。戻す行は LF、ほかの行は CRLF のまま。
        let (tmp, repo) = setup_converting(&[], b"1\n2\n3\n");
        let dir = tmp.path();
        std::fs::write(dir.join(".git/info/attributes"), "*.txt eol=lf\n").unwrap();
        let repo = {
            drop(repo);
            Repository::new(GixBackend::open(dir).unwrap())
        };
        std::fs::write(dir.join("a.txt"), "1\r\n2x\r\n3\r\n").unwrap();
        let diff = unstaged_diff(&repo);
        assert_eq!((diff.file.added, diff.file.removed), (1, 1));
        repo.discard_rows(&diff, &[2, 3]).unwrap();
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"1\r\n2\n3\r\n");
    }

    #[test]
    fn leaves_crlf_committed_files_unconverted() {
        // CRLF のまま commit されたファイルは、core.autocrlf=true でも変換しない（git と同じ）。
        let (tmp, repo) = setup_converting(&[("core.autocrlf", "true")], b"1\r\n2\r\n3\r\n");
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "1\r\nTWO\r\n3\r\n").unwrap();
        let diff = unstaged_diff(&repo);
        assert_eq!((diff.file.added, diff.file.removed), (1, 1));
        // @@, 1, -2, +TWO, 3: TWO を足すことだけを stage する。
        repo.stage_rows(&diff, &[3]).unwrap();
        assert_eq!(index_blob(dir), b"1\r\n2\r\nTWO\r\n3\r\n");
    }

    #[test]
    fn reports_line_endings_beside_the_diff() {
        use amonhen_model::{Eol, LineEndings};
        let le = |repo: &Repository| unstaged_diff(repo).line_endings.unwrap();
        // 普段のファイル（変換なし、どちらも LF）。見せない。
        let (tmp, repo) = setup_converting(&[], b"1\n2\n");
        std::fs::write(tmp.path().join("a.txt"), "1\nTWO\n").unwrap();
        let plain = le(&repo);
        assert_eq!(
            (plain.index, plain.worktree, plain.checkout),
            (Eol::Lf, Eol::Lf, Eol::Lf)
        );
        assert!(!plain.notable() && plain.rule.is_empty());

        // core.autocrlf=true: 作業ツリーは CRLF、git には LF。
        let (tmp, repo) = setup_converting(&[("core.autocrlf", "true")], b"1\n2\n");
        std::fs::write(tmp.path().join("a.txt"), "1\r\nTWO\r\n").unwrap();
        assert_eq!(
            le(&repo),
            LineEndings {
                index: Eol::Lf,
                worktree: Eol::Crlf,
                checkout: Eol::Crlf,
                rule: "core.autocrlf=true".into(),
            }
        );
        assert!(le(&repo).notable() && !le(&repo).warning());
        // stage 済みの差分には出さない。
        let status = repo.load_status().unwrap();
        let staged = repo
            .working_diff(&status.status.entries[0], true, &FrontendProfile::MACOS)
            .unwrap();
        assert!(staged.line_endings.is_none());

        // 改行が混ざっている。
        std::fs::write(tmp.path().join("a.txt"), "1\r\nTWO\n").unwrap();
        assert!(le(&repo).warning());

        // eol=lf なのに作業ツリーが CRLF（checkout すると LF になる）。
        let (tmp, _) = setup_converting(&[], b"1\n2\n");
        let dir = tmp.path();
        std::fs::write(dir.join(".git/info/attributes"), "*.txt eol=lf\n").unwrap();
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        std::fs::write(dir.join("a.txt"), "1\r\nTWO\r\n").unwrap();
        let attr = le(&repo);
        assert_eq!((attr.worktree, attr.checkout), (Eol::Crlf, Eol::Lf));
        assert_eq!(attr.rule, "eol=lf");
        assert!(attr.warning());
    }

    /// shallow clone の履歴を、数・日付・すべての 3 通りで取り足す。境界のコミットの詳細は、親を取得していないと
    /// 言う。shallow clone でなければ断る。
    #[test]
    fn fetches_more_of_a_shallow_clone() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        git(&src, &["init", "-q"]);
        git(&src, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        // --shallow-since はコミッターの日時で切るので、1 年ずつ離す。
        for year in 2020..2025 {
            let out = Process::new("git")
                .args(["commit", "--allow-empty", "-q", "-m", &year.to_string()])
                .current_dir(&src)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_DATE", format!("{year}-01-01T00:00:00Z"))
                .env("GIT_COMMITTER_DATE", format!("{year}-01-01T00:00:00Z"))
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
        }
        // --depth はパスの clone では効かないので file:// にする（Windows は file:///C:/…）。
        let path = src.to_str().unwrap().replace('\\', "/");
        let url = if path.starts_with('/') {
            format!("file://{path}")
        } else {
            format!("file:///{path}")
        };
        git(
            tmp.path(),
            &["clone", "-q", "--depth", "1", &url, "shallow"],
        );
        let dir = tmp.path().join("shallow");
        let repo = Repository::new(GixBackend::open(&dir).unwrap());
        let rows = |repo: &Repository| repo.load_history().unwrap().graph.len();
        assert_eq!(rows(&repo), 1);
        let boundary = git(&dir, &["rev-parse", "HEAD"]);
        let info = run(
            &repo,
            &format!(r#"{{"op":"commitInfo","oid":"{boundary}"}}"#),
        );
        assert_eq!(info["info"]["parentsNotFetched"], true, "{info}");
        assert_eq!(info["info"]["parents"].as_array().unwrap().len(), 1);

        // 数も日付もない・0 個は断る。
        for bad in [
            r#"{"op":"deepen"}"#,
            r#"{"op":"deepen","commits":0}"#,
            r#"{"op":"deepen","commits":1,"since":"2021-01-01"}"#,
        ] {
            assert_eq!(run(&repo, bad)["ok"], false, "{bad}");
        }

        let r = run(&repo, r#"{"op":"deepen","commits":1}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(rows(&repo), 2);
        // 2021-06-01 より新しいもの（2022・2023・2024）。
        let r = run(&repo, r#"{"op":"deepen","since":"2021-06-01"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(rows(&repo), 3);
        let info = run(
            &repo,
            &format!(r#"{{"op":"commitInfo","oid":"{boundary}"}}"#),
        );
        assert_eq!(info["info"]["parentsNotFetched"], false, "{info}");

        let r = run(&repo, r#"{"op":"unshallow"}"#);
        assert_eq!(r["ok"], true, "{r}");
        let history = repo.load_history().unwrap();
        assert_eq!(history.graph.len(), 5);
        assert!(history.shallow.is_empty());
        // 根のコミットには親がない（取得していないのではない）。
        let root = git(&dir, &["rev-list", "--max-parents=0", "HEAD"]);
        let info = run(&repo, &format!(r#"{{"op":"commitInfo","oid":"{root}"}}"#));
        assert_eq!(info["info"]["parentsNotFetched"], false, "{info}");

        let r = run(&repo, r#"{"op":"unshallow"}"#);
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!(run(&repo, r#"{"op":"deepen","commits":10}"#)["ok"], false);
    }
}
