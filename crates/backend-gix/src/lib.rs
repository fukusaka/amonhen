//! gix による GitBackend の実装。

use std::path::Path;

use std::ops::Range;
use std::sync::Arc;

use amonhen_model::{CommitText, GitBackend, Oid, OidSet, RawCommit, Result};
use gix::objs::Find;
use rayon::prelude::*;

pub struct GixBackend {
    repo: gix::ThreadSafeRepository,
}

impl GixBackend {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let repo = gix::ThreadSafeRepository::discover(path.as_ref())?;
        Ok(Self { repo })
    }
}

/// `part` が `whole` の中で占める位置。
fn range_in(whole: &[u8], part: &[u8]) -> Range<u32> {
    let start = part.as_ptr() as usize - whole.as_ptr() as usize;
    start as u32..(start + part.len()) as u32
}

fn trim(bytes: &[u8]) -> &[u8] {
    bytes.trim_ascii()
}

/// コミット 1 つの親と日時。commit-graph にあればオブジェクトを展開せずに済ませる。
fn read_commit(
    repo: &gix::Repository,
    graph: Option<&gix::commitgraph::Graph>,
    hash: gix::hash::Kind,
    buf: &mut Vec<u8>,
    id: &Oid,
) -> Result<RawCommit> {
    let oid = gix::ObjectId::from_bytes_or_panic(&id.0);
    if let Some(graph) = graph
        && let Some(pos) = graph.lookup(oid)
    {
        let commit = graph.commit_at(pos);
        let parents = commit
            .iter_parents()
            .map(|p| to_oid(graph.id_at(p?)))
            .collect::<Result<_>>()?;
        return Ok(RawCommit {
            id: *id,
            commit_time: commit.committer_timestamp() as i64,
            parents,
        });
    }
    let data = repo
        .objects
        .try_find(&oid, buf)?
        .ok_or_else(|| format!("コミット {id} が見つからない"))?;
    let iter = || gix::objs::CommitRefIter::from_bytes(data.data, hash);
    let parents = iter()
        .parent_ids()
        .map(|p| to_oid(&p))
        .collect::<Result<_>>()?;
    let commit_time = iter().committer()?.time()?.seconds;
    Ok(RawCommit {
        id: *id,
        commit_time,
        parents,
    })
}

fn to_oid(id: &gix::oid) -> Result<Oid> {
    let bytes: [u8; 20] = id
        .as_bytes()
        .try_into()
        .map_err(|_| "SHA-1 以外のリポジトリには未対応")?;
    Ok(Oid(bytes))
}

impl GitBackend for GixBackend {
    fn head(&self) -> Result<Option<Oid>> {
        let repo = self.repo.to_thread_local();
        match repo.head_id() {
            Ok(id) => Ok(Some(to_oid(id.as_ref())?)),
            Err(_) if repo.head()?.is_unborn() => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn tips(&self) -> Result<Vec<Oid>> {
        let repo = self.repo.to_thread_local();
        let mut seen = OidSet::default();
        let mut tips = Vec::new();
        let mut add = |id: Oid| {
            if seen.insert(id) {
                tips.push(id);
            }
        };

        if let Some(head) = self.head()? {
            add(head);
        }
        for reference in repo.references()?.all()? {
            let mut reference = reference?;
            // コミット以外（ツリーや blob）を指すタグは History に出さない。
            if let Ok(commit) = reference.peel_to_commit() {
                add(to_oid(commit.id().as_ref())?);
            }
        }
        Ok(tips)
    }

    fn read_commits(
        &self,
        tips: &[Oid],
        known: &(dyn Fn(&Oid) -> bool + Sync),
    ) -> Result<Vec<RawCommit>> {
        let repo = self.repo.to_thread_local();
        let graph = repo.commit_graph_if_enabled()?;
        let hash = repo.object_hash();
        let mut seen = OidSet::default();
        let mut frontier: Vec<Oid> = tips
            .iter()
            .copied()
            .filter(|t| !known(t) && seen.insert(*t))
            .collect();
        let mut out = Vec::new();

        // 幅優先で辿り、同じ深さのコミットはまとめて並列に読む（展開が時間の大半を占める）。
        while !frontier.is_empty() {
            let read: Vec<RawCommit> = frontier
                .par_iter()
                .map_init(
                    || (self.repo.to_thread_local(), Vec::new()),
                    |(repo, buf), id| read_commit(repo, graph.as_ref(), hash, buf, id),
                )
                .collect::<Result<_>>()?;
            frontier.clear();
            for commit in &read {
                for p in &commit.parents {
                    if !known(p) && seen.insert(*p) {
                        frontier.push(*p);
                    }
                }
            }
            out.extend(read);
        }
        Ok(out)
    }

    fn commit_texts(&self, ids: &[Oid]) -> Result<Vec<CommitText>> {
        let mut repo = self.repo.to_thread_local();
        repo.object_cache_size_if_unset(64 * 1024 * 1024);
        let hash = repo.object_hash();
        ids.iter()
            .map(|id| {
                let oid = gix::ObjectId::from_bytes_or_panic(&id.0);
                // 展開先のバッファをそのまま持ち続ける（これが唯一の書き込み）。
                let mut buf = Vec::new();
                let len = repo
                    .objects
                    .try_find(&oid, &mut buf)?
                    .ok_or_else(|| format!("コミット {id} が見つからない"))?
                    .data
                    .len();
                buf.truncate(len);
                let (subject, author_name, author_time) = {
                    let commit = gix::objs::CommitRef::from_bytes(&buf, hash)?;
                    let author = commit.author()?;
                    let first_line = commit
                        .message
                        .split(|&b| b == b'\n')
                        .next()
                        .unwrap_or_default();
                    (
                        range_in(&buf, trim(first_line)),
                        range_in(&buf, trim(author.name)),
                        author.time()?.seconds,
                    )
                };
                Ok(CommitText {
                    raw: Arc::new(buf),
                    subject,
                    author_name,
                    author_time,
                })
            })
            .collect()
    }
}
