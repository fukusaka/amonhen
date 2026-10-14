//! gix による GitBackend の実装。

use std::path::Path;

use std::ops::Range;
use std::sync::Arc;

use amonhen_model::{
    CommitText, GitBackend, Oid, OidSet, RawCommit, RefEntry, RefKind, Refs, RepoPaths, Result,
};
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

    fn paths(&self) -> RepoPaths {
        let repo = self.repo.to_thread_local();
        RepoPaths {
            git_dir: repo.git_dir().to_owned(),
            common_dir: repo.common_dir().to_owned(),
            workdir: repo.workdir().map(ToOwned::to_owned),
        }
    }

    fn refs(&self) -> Result<Refs> {
        let repo = self.repo.to_thread_local();
        let mut entries = Vec::new();
        let platform = repo.references()?;
        let mut push = |kind, iter: gix::reference::iter::Iter<'_, '_>| -> Result<()> {
            for r in iter {
                let mut r = r?;
                // origin/HEAD のような別名は出さない。
                if r.target().try_id().is_none() && kind == RefKind::Remote {
                    continue;
                }
                let name = r.name().shorten().to_string();
                let detail = (kind == RefKind::Local)
                    .then(|| {
                        repo.branch_remote_tracking_ref_name(
                            r.name(),
                            gix::remote::Direction::Fetch,
                        )
                    })
                    .flatten()
                    .and_then(|upstream| upstream.ok())
                    .map(|upstream| upstream.as_ref().shorten().to_string());
                let target = r
                    .peel_to_commit()
                    .ok()
                    .map(|c| to_oid(c.id().as_ref()))
                    .transpose()?;
                entries.push(RefEntry {
                    kind,
                    name,
                    target,
                    detail,
                });
            }
            Ok(())
        };
        push(RefKind::Local, platform.local_branches()?)?;
        push(RefKind::Remote, platform.remote_branches()?)?;
        push(RefKind::Tag, platform.tags()?)?;
        entries.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));

        if let Ok(stash) = repo.find_reference("refs/stash") {
            let mut log = stash.log_iter();
            if let Some(lines) = log.all()? {
                let lines: Vec<_> = lines.collect::<std::result::Result<_, _>>()?;
                for (i, line) in lines.iter().rev().enumerate() {
                    entries.push(RefEntry {
                        kind: RefKind::Stash,
                        name: format!("stash@{{{i}}}"),
                        target: Some(to_oid(&line.new_oid())?),
                        detail: Some(line.message.to_string()),
                    });
                }
            }
        }

        if let Some(submodules) = repo.submodules()? {
            let mut subs = Vec::new();
            for sm in submodules {
                let target = sm
                    .index_id()
                    .ok()
                    .flatten()
                    .map(|id| to_oid(id.as_ref()))
                    .transpose()?;
                subs.push(RefEntry {
                    kind: RefKind::Submodule,
                    name: sm.path()?.to_string(),
                    target,
                    detail: None,
                });
            }
            subs.sort_by(|a, b| a.name.cmp(&b.name));
            entries.extend(subs);
        }

        Ok(Refs {
            head_branch: repo.head_name()?.map(|n| n.as_ref().shorten().to_string()),
            head: self.head()?,
            entries,
        })
    }

    fn has_commit_graph(&self) -> Result<bool> {
        Ok(self
            .repo
            .to_thread_local()
            .commit_graph_if_enabled()?
            .is_some())
    }

    // gix には commit-graph を書く機能がないので git に任せる。書き込み系の CliBackend を作ったら移す。
    fn write_commit_graph(&self) -> Result<()> {
        let repo = self.repo.to_thread_local();
        let out = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(repo.common_dir())
            .args(["commit-graph", "write", "--reachable", "--no-progress"])
            .output()?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr)
                .trim()
                .to_owned()
                .into());
        }
        Ok(())
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
