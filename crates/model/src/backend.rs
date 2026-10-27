use std::sync::Arc;

use crate::{
    CommitExtras, CommitGraph, CommitText, FileChange, Oid, Operations, RawCommit, Refs, RepoFiles,
    RepoPaths, Status, topo,
};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// 任せた処理（`GitBackend::delegate`）の答え。大きなバイト列（差分のファイルの中身）は `parts` に分け、手元の
/// 共有の daemon からは共有メモリで受け取る（コピーしない）。
#[derive(Default)]
pub struct Delegated {
    pub reply: Vec<u8>,
    pub parts: Vec<amonhen_layout::Blob>,
}

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

    /// コミット `old` から `new` への変更（`old` が None なら全ファイルの追加）。名前の変更も見つける。
    fn changes_between(&self, old: Option<&Oid>, new: &Oid) -> Result<Vec<FileChange>>;

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

    /// 作業ツリーのファイルの中身を、git に入れる形で（`core.autocrlf`・`core.eol`・`.gitattributes` の改行の変換などを
    /// かけたもの。`git diff` と同じく、index や HEAD の blob とそのまま比べられる）。
    fn worktree_file(&self, path: &str) -> Result<Arc<Vec<u8>>>;

    /// git に入れる形の `content`（`path` の blob の中身）を、作業ツリーに書く形にする（checkout と同じ変換）。
    /// 変換しない実装はそのまま返す。
    fn to_worktree_form(&self, path: &str, content: &[u8]) -> Result<Vec<u8>> {
        let _ = path;
        Ok(content.to_vec())
    }

    /// `path` にかかる改行の変換（`text=auto eol=crlf`・`core.autocrlf=true`・`-text` など）。なければ空。
    /// 変換しない実装は空を返す。
    fn line_ending_rule(&self, path: &str) -> Result<String> {
        let _ = path;
        Ok(String::new())
    }

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

    /// commit-graph を書き出す（`git commit-graph write --reachable`）。`split` なら、今の commit-graph に
    /// ないコミットだけを層として書き足す（`--split`）。リポジトリの `.git` に書き込むので、利用者が
    /// 設定で選んだときだけ呼ぶ。対応しない実装は何もしない。
    fn write_commit_graph(&self, split: bool) -> Result<()> {
        let _ = split;
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

    /// `ids` の署名をまとめて検証し、結果（`status` は `%G?`、署名がなければ N）を同じ順に返す。対応しない
    /// 実装はすべて E（検証できない）を返す。
    fn verify_signatures(&self, ids: &[Oid]) -> Result<Vec<crate::SignatureCheck>> {
        Ok(vec![
            crate::SignatureCheck {
                status: 'E',
                ..Default::default()
            };
            ids.len()
        ])
    }

    /// タグのオブジェクトの中身（`ids` と同じ順。タグのオブジェクトでなければ `None`）。対応しない実装はすべて
    /// `None` を返す。
    fn tag_objects(&self, ids: &[Oid]) -> Result<Vec<Option<Vec<u8>>>> {
        Ok(vec![None; ids.len()])
    }

    /// `tags`（タグのオブジェクトの ID と中身）の署名を検証し、結果（`status` は `%G?` と同じ 1 文字、署名が
    /// なければ N）を同じ順に返す。中身を渡すのは、merge したタグがマージコミットの mergetag にしか残って
    /// いないことがあるため。対応しない実装はすべて E（検証できない）を返す。
    fn verify_tags(&self, tags: &[(Oid, Vec<u8>)]) -> Result<Vec<crate::SignatureCheck>> {
        Ok(vec![
            crate::SignatureCheck {
                status: 'E',
                ..Default::default()
            };
            tags.len()
        ])
    }

    /// daemon とのつながりの様子。手元の実装は `None`。
    fn connection_state(&self) -> Option<ConnectionState> {
        None
    }

    /// 件名・作者・日時だけを読む（History の行に出す分）。daemon 越しでは本文を送らずに済む。
    /// History を別の所（手元の共有の daemon）で用意してもらえるなら、その永続キャッシュのファイル。つないだ側は
    /// レイアウトを計算せず、このファイルを mmap して使う（同じ Mac の上なので、中身はコピーせずに共有される）。
    /// 用意してもらえなければ `None`。
    fn prepared_history(&self) -> Result<Option<std::path::PathBuf>> {
        Ok(None)
    }

    /// コアの処理（命令・差分）を手元の共有の daemon のコアに任せる。`request` と答えの形はコアが決める
    /// （`amonhen_core` の `delegate`）。任せられなければ `None`。
    fn delegate(&self, _request: &[u8]) -> Result<Option<Delegated>> {
        Ok(None)
    }

    fn commit_summaries(&self, ids: &[Oid]) -> Result<Vec<CommitText>> {
        self.commit_texts(ids)
    }

    /// `tips` から辿れるコミットを、本文の条件で探す（条件ごとに合ったコミット）。daemon 越しの実装は
    /// daemon の側で探して ID だけを返す。`None` なら呼び出し側が `commit_texts` を読んで探す。`mailmap` が
    /// あれば、作者は置き換えた名前でも探す（daemon の側は、自分の側で読んだ mailmap を使う）。
    fn search_texts(
        &self,
        tips: &[Oid],
        terms: &[crate::search::TextTerm],
        mailmap: Option<&crate::Mailmap>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<Vec<Vec<Oid>>>> {
        let _ = (tips, terms, mailmap, cancelled);
        Ok(None)
    }

    /// リポジトリの mailmap（`.mailmap`・`mailmap.blob`・`mailmap.file`）と `log.mailmap`。読むのはリポジトリの
    /// ある側。何度も呼ばれる（History の行を読むたび）ので、変わっていなければ覚えているものを安く返すこと。
    fn mailmap(&self) -> Result<std::sync::Arc<crate::Mailmap>> {
        Ok(Default::default())
    }

    /// コミットの本文。History で見えている行の分だけ呼ぶ。
    fn commit_texts(&self, ids: &[Oid]) -> Result<Vec<CommitText>>;

    /// ローカルブランチ `names` を作った点のコミット（reflog の最初の行が「なし → コミット」のもの）を同じ順に
    /// 返す。reflog がない・作った行が残っていないものは `None`。ref の置き場所（linked worktree の共通の
    /// git dir、reftable）は実装が知っているので、コアはファイルを読まずにここを使う。対応しない実装はすべて
    /// `None` を返す。
    fn branch_created_at(&self, names: &[&str]) -> Result<Vec<Option<Oid>>> {
        Ok(vec![None; names.len()])
    }

    /// ref `name`（`HEAD`・`refs/heads/main` など）の reflog を、新しい順に `limit` 行まで返す。`HEAD` は開いた
    /// worktree のもの。reflog がなければ空。対応しない実装は空を返す。
    fn reflog(&self, name: &str, limit: usize) -> Result<Vec<crate::ReflogEntry>> {
        let _ = (name, limit);
        Ok(Vec::new())
    }
}

/// daemon とのつながりの様子（FE が読み込みの進み具合や切断を出すのに使う）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionState {
    /// これまでに送った・受け取ったバイト数（通信路の上の大きさ）。
    pub sent: u64,
    pub received: u64,
    /// 切れていれば、その理由。
    pub closed: Option<String>,
    /// SSH 先で使っているエンジンと、その用意のしかた（SSH のつながりのときだけ）。
    pub engine: Option<String>,
}
