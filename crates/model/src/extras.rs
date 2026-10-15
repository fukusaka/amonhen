//! コミットの詳細のうち、git に頼んで求める分（署名の検証・notes・変更の行数）。どれも時間がかかるので、
//! FE はコミットの情報を先に出し、これは裏で読んでから足す。

use crate::{Oid, Result};

/// 署名の検証の結果（`git log --format=%G?` など）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SignatureCheck {
    /// `%G?` の 1 文字。G（正しい）、B（不正）、U（正しいが信用の度合いが不明）、X（期限切れの署名）、
    /// Y（期限切れの鍵）、R（失効した鍵）、E（検証できない。鍵がないなど）、N（署名なし）。
    pub status: char,
    /// 署名した人（`%GS`）。
    pub signer: String,
    /// 鍵の ID（`%GK`）と指紋（`%GF`）。
    pub key: String,
    pub fingerprint: String,
    /// 信用の度合い（`%GT`。ultimate・full・marginal・never・undefined）。
    pub trust: String,
    /// 署名した鍵に登録されたメールアドレス（GPG はすべてのユーザー ID、SSH は allowed signers のプリンシパル）。
    /// 読めなければ空（そのときはコミッターと一致するかを言い切れない）。
    pub emails: Vec<SignerEmail>,
}

/// 署名した鍵のメールアドレスと、その有効性（gpg の uid 行の 2 番目の欄。u・f・m は有効、r は失効、e は期限切れ、
/// `-`・q・空は不明。SSH の allowed signers のプリンシパルは f）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SignerEmail {
    pub email: String,
    pub validity: char,
}

/// 1 つのファイルの変更の行数。バイナリなら `None`。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileStat {
    pub path: String,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CommitExtras {
    /// 署名がなければ `None`。
    pub signature: Option<SignatureCheck>,
    /// git notes（refs/notes/commits）。なければ空。
    pub notes: String,
    /// 最初の親との差分の、ファイルごとの行数（`commit_changes` と同じ比べ方）。
    pub stats: Vec<FileStat>,
}

/// コミットのオブジェクトを受け取って署名を検証するもの。daemon 越しのリポジトリでも、手元の鍵で検証する
/// ために FE（デスクトップ）が注入する。結果は `git log --format=%G?` と同じ 1 文字。
pub trait SignatureVerifier: Send + Sync {
    /// `commits`（ID とコミットのオブジェクトの中身）を検証し、同じ順に結果を返す（署名がなければ N）。
    fn verify(&self, commits: &[(Oid, &[u8])]) -> Result<Vec<SignatureCheck>>;
    /// 1 つのコミットの検証の詳細（署名者・鍵・信用度）。署名がなければ `None`。
    fn details(&self, id: &Oid, raw: &[u8]) -> Result<Option<SignatureCheck>>;
}
