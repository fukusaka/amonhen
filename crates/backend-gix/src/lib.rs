//! gix による GitBackend の実装。

use std::collections::HashSet;
use std::path::Path;

use amonhen_model::{CommitGraph, CommitGraphBuilder, GitBackend, Oid, Result};
use gix::traverse::commit::topo;

pub struct GixBackend {
    repo: gix::ThreadSafeRepository,
}

impl GixBackend {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let repo = gix::ThreadSafeRepository::discover(path.as_ref())?;
        Ok(Self { repo })
    }
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
        let mut seen = HashSet::new();
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

    fn commit_graph(&self, tips: &[Oid]) -> Result<CommitGraph> {
        let mut repo = self.repo.to_thread_local();
        repo.object_cache_size_if_unset(64 * 1024 * 1024);
        let walk = topo::Builder::new(&repo.objects)
            .with_tips(
                tips.iter()
                    .map(|t| gix::ObjectId::from_bytes_or_panic(&t.0)),
            )
            .sorting(topo::Sorting::TopoOrder)
            .with_commit_graph(repo.commit_graph_if_enabled()?)
            .build()?;

        let mut builder = CommitGraphBuilder::default();
        for info in walk {
            let info = info?;
            let parents = info
                .parent_ids
                .iter()
                .map(|p| to_oid(p.as_ref()))
                .collect::<Result<Vec<_>>>()?;
            builder.push(
                to_oid(info.id.as_ref())?,
                info.commit_time.unwrap_or(0),
                parents,
            );
        }
        Ok(builder.build())
    }
}
