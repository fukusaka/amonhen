//! gix による GitBackend の実装。

use std::path::Path;

use std::ops::Range;
use std::sync::Arc;

use amonhen_model::Operations as _;
use amonhen_model::{
    ChangeKind, CommitText, FileChange, GitBackend, Oid, OidSet, RawCommit, RefEntry, RefKind,
    Refs, RepoPaths, Result,
};
use gix::objs::Find;
use rayon::prelude::*;

/// 読み取りは gix で同じプロセスの中で行い、書き込みと status は git に任せる（`amonhen_backend_cli`）。
pub struct GixBackend {
    repo: gix::ThreadSafeRepository,
    /// bare リポジトリでは `None`。
    git: Option<amonhen_backend_cli::Git>,
}

impl GixBackend {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let repo = gix::ThreadSafeRepository::discover(path.as_ref())?;
        let git = repo.work_dir().map(amonhen_backend_cli::Git::new);
        Ok(Self { repo, git })
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

impl GixBackend {
    fn git(&self) -> Result<&amonhen_backend_cli::Git> {
        self.git
            .as_ref()
            .ok_or_else(|| "作業ツリーのないリポジトリ".into())
    }
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
            // stash はサイドバーで扱い、History には出さない。
            if reference.name().as_bstr() == "refs/stash" {
                continue;
            }
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
                    ahead: 0,
                    behind: 0,
                    gone: false,
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
                        ahead: 0,
                        behind: 0,
                        gone: false,
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
                    ahead: 0,
                    behind: 0,
                    gone: false,
                });
            }
            subs.sort_by(|a, b| a.name.cmp(&b.name));
            entries.extend(subs);
        }

        // 上流との差は git に数えさせる（gix で数えるより速く、git と同じ結果になる）。
        if let Some(git) = &self.git
            && let Ok(tracking) = git.branch_tracking()
        {
            for (name, ahead, behind, gone) in tracking {
                if let Some(e) = entries
                    .iter_mut()
                    .find(|e| e.kind == RefKind::Local && e.name == name)
                {
                    (e.ahead, e.behind, e.gone) = (ahead, behind, gone);
                }
            }
        }

        Ok(Refs {
            head_branch: repo.head_name()?.map(|n| n.as_ref().shorten().to_string()),
            head: self.head()?,
            entries,
        })
    }

    fn commit_changes(&self, commit: &Oid) -> Result<Vec<FileChange>> {
        use gix::object::tree::diff::ChangeDetached as C;
        let repo = self.repo.to_thread_local();
        let commit = repo.find_commit(gix::ObjectId::from_bytes_or_panic(&commit.0))?;
        let new_tree = commit.tree()?;
        let old_tree = match commit.parent_ids().next() {
            Some(parent) => Some(parent.object()?.into_commit().tree()?),
            None => None,
        };
        let opt = |id: gix::ObjectId| -> Result<Option<Oid>> { to_oid(id.as_ref()).map(Some) };
        let is_commit = |mode: gix::object::tree::EntryMode| mode.is_commit();
        let mut changes = Vec::new();
        for change in repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)? {
            let (mode, previous) = match &change {
                C::Addition { entry_mode, .. } | C::Deletion { entry_mode, .. } => {
                    (*entry_mode, *entry_mode)
                }
                C::Modification {
                    entry_mode,
                    previous_entry_mode,
                    ..
                } => (*entry_mode, *previous_entry_mode),
                C::Rewrite {
                    entry_mode,
                    source_entry_mode,
                    ..
                } => (*entry_mode, *source_entry_mode),
            };
            // ディレクトリそのものの変化は、中のファイルの変化として出る。
            if mode.is_tree() || previous.is_tree() {
                continue;
            }
            let submodule = is_commit(mode) || is_commit(previous);
            changes.push(match change {
                C::Addition { location, id, .. } => FileChange {
                    kind: ChangeKind::Added,
                    path: location.to_string(),
                    old_path: None,
                    old_id: None,
                    new_id: opt(id)?,
                    submodule,
                },
                C::Deletion { location, id, .. } => FileChange {
                    kind: ChangeKind::Deleted,
                    path: location.to_string(),
                    old_path: None,
                    old_id: opt(id)?,
                    new_id: None,
                    submodule,
                },
                C::Modification {
                    location,
                    previous_id,
                    id,
                    ..
                } => FileChange {
                    kind: ChangeKind::Modified,
                    path: location.to_string(),
                    old_path: None,
                    old_id: opt(previous_id)?,
                    new_id: opt(id)?,
                    submodule,
                },
                C::Rewrite {
                    source_location,
                    source_id,
                    location,
                    id,
                    copy,
                    ..
                } => FileChange {
                    kind: if copy {
                        ChangeKind::Copied
                    } else {
                        ChangeKind::Renamed
                    },
                    path: location.to_string(),
                    old_path: Some(source_location.to_string()),
                    old_id: opt(source_id)?,
                    new_id: opt(id)?,
                    submodule,
                },
            });
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    fn stash_changes(&self, stash: &Oid) -> Result<Vec<FileChange>> {
        let mut changes = self.commit_changes(stash)?;
        // untracked のファイルを含めた stash は、3 番目の親にそれらだけを持つ。
        let repo = self.repo.to_thread_local();
        let commit = repo.find_commit(gix::ObjectId::from_bytes_or_panic(&stash.0))?;
        if let Some(untracked) = commit.parent_ids().nth(2) {
            let tree = untracked.object()?.into_commit().tree()?;
            for change in repo.diff_tree_to_tree(None, Some(&tree), None)? {
                if let gix::object::tree::diff::ChangeDetached::Addition {
                    location,
                    id,
                    entry_mode,
                    ..
                } = change
                    && !entry_mode.is_tree()
                {
                    changes.push(FileChange {
                        kind: ChangeKind::Added,
                        path: location.to_string(),
                        old_path: None,
                        old_id: None,
                        new_id: Some(to_oid(id.as_ref())?),
                        submodule: false,
                    });
                }
            }
            changes.sort_by(|a, b| a.path.cmp(&b.path));
        }
        Ok(changes)
    }

    fn blob(&self, id: &Oid) -> Result<Arc<Vec<u8>>> {
        let repo = self.repo.to_thread_local();
        let mut buf = Vec::new();
        let len = repo
            .objects
            .try_find(&gix::ObjectId::from_bytes_or_panic(&id.0), &mut buf)?
            .ok_or_else(|| format!("blob {id} が見つからない"))?
            .data
            .len();
        buf.truncate(len);
        Ok(Arc::new(buf))
    }

    fn operations(&self) -> Result<&dyn amonhen_model::Operations> {
        Ok(self.git()?)
    }

    fn status(&self) -> Result<amonhen_model::Status> {
        self.git()?.status()
    }

    fn worktree_file(&self, path: &str) -> Result<Arc<Vec<u8>>> {
        Ok(Arc::new(std::fs::read(self.git()?.workdir().join(path))?))
    }

    fn stage(&self, paths: &[&str]) -> Result<()> {
        self.git()?.stage(paths)
    }

    fn unstage(&self, paths: &[&str]) -> Result<()> {
        self.git()?.unstage(paths)
    }

    fn set_index_content(&self, path: &str, mode: u32, content: &[u8]) -> Result<()> {
        self.git()?.set_index_content(path, mode, content)
    }

    fn commit(&self, message: &str, amend: bool) -> Result<()> {
        self.git()?.commit(message, amend)
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
