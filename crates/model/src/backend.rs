use std::sync::Arc;

use crate::{
    CommitExtras, CommitGraph, CommitText, FileChange, Oid, Operations, RawCommit, Refs, RepoFiles,
    RepoPaths, Status, topo,
};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// git の操作。デスクトップは gix ＋ `git` の spawn、モバイルは gix ＋ libgit2 で実装する。
///
/// 書き込み系の操作は、実装を足すときにここへ加える。
pub trait GitBackend: Send + Sync {
    /// HEAD が指すコミット。生まれたばかりのブランチなどでコミットがなければ `None`。
    fn head(&self) -> Result<Option<Oid>>;

    /// History の起点（HEAD・ローカルとリモートのブランチ・タグが指すコミット）。
    fn tips(&self) -> Result<Vec<Oid>>;

    /// `tips` から辿れるコミットを順不同で読む。`known` が真を返すコミットとその先は辿らない
    /// （前回読んだ History に当たったら止めるため）。各コミットは 1 回だけ読む。
    fn read_commits(
        &self,
        tips: &[Oid],
        known: &(dyn Fn(&Oid) -> bool + Sync),
    ) -> Result<Vec<RawCommit>>;

    /// `read_commits` に前回の History の起点 `known_tips` を添えたもの（`known` は `known_tips` から辿れる
    /// コミット）。関数を送れないリモートの実装は、`known_tips` で既知の範囲を決める。
    fn read_new_commits(
        &self,
        tips: &[Oid],
        known_tips: &[Oid],
        known: &(dyn Fn(&Oid) -> bool + Sync),
    ) -> Result<Vec<RawCommit>> {
        let _ = known_tips;
        self.read_commits(tips, known)
    }

    /// `tips` から辿れ、`known_tips` からは辿れないコミット（daemon が `read_new_commits` に答えるときに使う）。
    fn commits_since(&self, tips: &[Oid], known_tips: &[Oid]) -> Result<Vec<RawCommit>> {
        // 既定は前回の範囲をすべて読んで印を付ける（遅い。git が使える実装は rev-list に任せる）。
        let known: crate::OidSet = self
            .read_commits(known_tips, &|_| false)?
            .into_iter()
            .map(|c| c.id)
            .collect();
        self.read_commits(tips, &|id| known.contains(id))
    }

    /// `tips` から辿れるコミットを、子が親より先に来るトポロジカル順で返す。
    fn commit_graph(&self, tips: &[Oid]) -> Result<CommitGraph> {
        Ok(topo::sort(self.read_commits(tips, &|_| false)?))
    }

    fn paths(&self) -> RepoPaths;

    /// 作業ツリーと `.git` の中のファイルの読み書き（コアはファイルに直接触れない）。
    fn files(&self) -> Arc<dyn RepoFiles>;

    /// サイドバーに出す ref の一覧。
    fn refs(&self) -> Result<Refs>;

    /// `commit` で変わったファイル（最初の親との差分。親がなければ全ファイルの追加）。
    /// 名前の変更も見つける。
    fn commit_changes(&self, commit: &Oid) -> Result<Vec<FileChange>>;

    /// stash の中身（作業中の変更と、含めていれば untracked のファイル）。
    fn stash_changes(&self, stash: &Oid) -> Result<Vec<FileChange>> {
        self.commit_changes(stash)
    }

    /// blob の中身。展開したバッファをそのまま返し、差分の各行はここを直接指す。
    fn blob(&self, id: &Oid) -> Result<Arc<Vec<u8>>>;

    /// 書き込み系の操作。対応しない実装（作業ツリーがない・モバイル）はエラー。
    fn operations(&self) -> Result<&dyn Operations>;

    /// 作業ツリーと index の状態。
    fn status(&self) -> Result<Status>;

    /// 作業ツリーのファイルの中身。
    fn worktree_file(&self, path: &str) -> Result<Arc<Vec<u8>>>;

    /// `paths` の変更（追加・削除を含む）を index に入れる。
    fn stage(&self, paths: &[&str]) -> Result<()>;

    /// `paths` を HEAD の状態に戻す（index だけ）。
    fn unstage(&self, paths: &[&str]) -> Result<()>;

    /// `path` の index の中身を `content` にする（hunk・行単位の stage・unstage）。
    fn set_index_content(&self, path: &str, mode: u32, content: &[u8]) -> Result<()>;

    /// stage した変更をコミットする。
    fn commit(&self, message: &str, amend: bool) -> Result<()>;

    /// commit-graph ファイルがあるか。なければ History の初回の読み込みが数倍遅い。
    fn has_commit_graph(&self) -> Result<bool> {
        Ok(true)
    }

    /// commit-graph を書き出す（`git commit-graph write --reachable`）。リポジトリの `.git` に
    /// 書き込むので、利用者が設定で選んだときだけ呼ぶ。対応しない実装は何もしない。
    fn write_commit_graph(&self) -> Result<()> {
        Ok(())
    }

    /// リポジトリの変化を `f` に知らせてもらう（ビットは `amonhen_watch::Changes`）。daemon 越しの実装は
    /// daemon の知らせを渡して true を返す。手元の実装は false を返し、呼び出し側がファイルを監視する。
    fn subscribe_changes(&self, f: Box<dyn Fn(u32) + Send + Sync>) -> bool {
        let _ = f;
        false
    }

    /// 署名の検証・notes・変更の行数。git を起動するので時間がかかる（裏で呼ぶ）。対応しない実装は空を返す。
    fn commit_extras(&self, commit: &Oid) -> Result<CommitExtras> {
        let _ = commit;
        Ok(CommitExtras::default())
    }

    /// daemon とのつながりの様子。手元の実装は `None`。
    fn connection_state(&self) -> Option<ConnectionState> {
        None
    }

    /// 件名・作者・日時だけを読む（History の行に出す分）。daemon 越しでは本文を送らずに済む。
    fn commit_summaries(&self, ids: &[Oid]) -> Result<Vec<CommitText>> {
        self.commit_texts(ids)
    }

    /// `tips` から辿れるコミットを、本文の条件で探す（条件ごとに合ったコミット）。daemon 越しの実装は
    /// daemon の側で探して ID だけを返す。`None` なら呼び出し側が `commit_texts` を読んで探す。
    fn search_texts(
        &self,
        tips: &[Oid],
        terms: &[crate::search::TextTerm],
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<Vec<Vec<Oid>>>> {
        let _ = (tips, terms, cancelled);
        Ok(None)
    }

    /// コミットの本文。History で見えている行の分だけ呼ぶ。
    fn commit_texts(&self, ids: &[Oid]) -> Result<Vec<CommitText>>;
}

/// daemon とのつながりの様子（FE が読み込みの進み具合や切断を出すのに使う）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionState {
    /// これまでに送った・受け取ったバイト数（通信路の上の大きさ）。
    pub sent: u64,
    pub received: u64,
    /// 切れていれば、その理由。
    pub closed: Option<String>,
}
