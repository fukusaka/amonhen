//! gix による GitBackend の実装。

use amonhen_model::tr;
use std::path::Path;

use std::ops::Range;
use std::sync::{Arc, Mutex};

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
    filters: Mutex<Option<Filters>>,
}

/// 改行の変換などの pipeline と属性の引き方。作るのに大きなリポジトリでは数 ms かかる（`.gitattributes` の対応表を
/// index から作る）ので、index が変わるまで使い回す。
struct Filters {
    index_stamp: Option<(std::time::SystemTime, u64)>,
    pipeline: gix::filter::plumbing::Pipeline,
    attributes: gix::worktree::Stack,
    index: gix::worktree::IndexPersistedOrInMemory,
}

impl GixBackend {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let repo = gix::ThreadSafeRepository::discover(path.as_ref())?;
        let git = repo.work_dir().map(amonhen_backend_cli::Git::new);
        Ok(Self {
            repo,
            git,
            filters: Mutex::new(None),
        })
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

/// History を作るとき、同じ深さのコミットがこれだけあれば並列に読む（少なければ 1 本のスレッドで順に読む）。
/// git/git（8.2 万コミット、32 コア）で 16〜64 がいちばん速く、ほぼ同じだった。スレッドを起こす回数の少ない 64 にする。
const PARALLEL_READ_MIN: usize = 64;

/// 並列に読むときの、スレッドごとの読み手（リポジトリと、展開に使うバッファ）。
type Reader = Mutex<Option<(gix::Repository, Vec<u8>)>>;

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
        .ok_or_else(|| tr!("Commit {id} not found", id = id))?;
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
        .map_err(|_| tr!("Only SHA-1 repositories are supported"))?;
    Ok(Oid(bytes))
}

impl GixBackend {
    /// 改行の変換などの pipeline を使う（index が変わっていれば作り直す）。
    fn with_filters<T>(
        &self,
        f: impl FnOnce(&mut Filters, &gix::Repository) -> Result<T>,
    ) -> Result<T> {
        let repo = self.repo.to_thread_local();
        let stamp = std::fs::metadata(repo.index_path())
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        let mut cached = self.filters.lock().unwrap();
        if cached.as_ref().is_none_or(|c| c.index_stamp != stamp) {
            let (pipeline, attributes, index) = builtin_filters(&repo)?;
            *cached = Some(Filters {
                index_stamp: stamp,
                pipeline,
                attributes,
                index,
            });
        }
        f(cached.as_mut().expect("作ったところ"), &repo)
    }

    fn git(&self) -> Result<&amonhen_backend_cli::Git> {
        self.git
            .as_ref()
            .ok_or_else(|| tr!("The repository has no working tree").into())
    }
}

/// git の組み込みのフィルター（改行の変換・`working-tree-encoding`・`ident`）だけの pipeline と、属性の引き方、index。
/// `.gitattributes` の `filter=`（git-lfs など）の外部のプログラムは起動しない（差分を見るたびに起動して遅くなるため）。
/// 改行が混ざっていても失敗させない（`core.safecrlf` は書き込むときの git の確かめで、見るときには関係しない）。
fn builtin_filters(
    repo: &gix::Repository,
) -> Result<(
    gix::filter::plumbing::Pipeline,
    gix::worktree::Stack,
    gix::worktree::IndexPersistedOrInMemory,
)> {
    let (pipeline, index) = repo.filter_pipeline(None)?;
    let (mut filters, attributes) = pipeline.into_parts();
    let options = filters.options_mut();
    options.drivers.clear();
    options.crlf_roundtrip_check = gix::filter::plumbing::pipeline::CrlfRoundTripCheck::Skip;
    Ok((filters, attributes, index))
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
        let mut buf = Vec::new();
        // 並列に読むときの、スレッドごとの読み手（大きなバッファ付き）。rayon のスレッドの番号ごとに 1 つ、最初に
        // 使うときに作り、History を作り終えるまで使い回す。rayon は仕事を分けるたびに map_init の初期化をするので、
        // そのたびに作り直すと読むより高くつく（musl ではバッファのたびに mmap・munmap して、5 倍遅くなっていた）。
        let threads = rayon::current_num_threads();
        let readers: Vec<Reader> = (0..=threads).map(|_| Mutex::new(None)).collect();

        // 幅優先で辿り、同じ深さのコミットが多ければまとめて並列に読む（展開が時間の大半を占める）。ふつうの履歴は
        // ほとんどの深さで 1〜数個しかないので、そこはスレッドを起こさずに順に読む。
        while !frontier.is_empty() {
            let read: Vec<RawCommit> = if frontier.len() < PARALLEL_READ_MIN {
                frontier
                    .iter()
                    .map(|id| read_commit(&repo, graph.as_ref(), hash, &mut buf, id))
                    .collect::<Result<_>>()?
            } else {
                frontier
                    .par_iter()
                    .map(|id| {
                        let slot = rayon::current_thread_index()
                            .unwrap_or(threads)
                            .min(threads);
                        let mut reader = readers[slot].lock().unwrap_or_else(|e| e.into_inner());
                        let (repo, buf) =
                            reader.get_or_insert_with(|| (self.repo.to_thread_local(), Vec::new()));
                        read_commit(repo, graph.as_ref(), hash, buf, id)
                    })
                    .collect::<Result<_>>()?
            };
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

    fn files(&self) -> Arc<dyn amonhen_model::RepoFiles> {
        let paths = self.paths();
        amonhen_model::LocalFiles::new(paths.git_dir, paths.workdir)
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
        let repo = self.repo.to_thread_local();
        let commit = repo.find_commit(gix::ObjectId::from_bytes_or_panic(&commit.0))?;
        let parent = commit
            .parent_ids()
            .next()
            .map(|p| to_oid(p.as_ref()))
            .transpose()?;
        self.changes_between(parent.as_ref(), &to_oid(commit.id.as_ref())?)
    }

    fn changes_between(&self, old: Option<&Oid>, new: &Oid) -> Result<Vec<FileChange>> {
        use gix::object::tree::diff::ChangeDetached as C;
        let repo = self.repo.to_thread_local();
        let tree = |id: &Oid| -> Result<gix::Tree<'_>> {
            Ok(repo
                .find_commit(gix::ObjectId::from_bytes_or_panic(&id.0))?
                .tree()?)
        };
        let new_tree = tree(new)?;
        let old_tree = old.map(tree).transpose()?;
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
            .ok_or_else(|| tr!("Blob {id} not found", id = id))?
            .data
            .len();
        buf.truncate(len);
        Ok(Arc::new(buf))
    }

    fn operations(&self) -> Result<&dyn amonhen_model::Operations> {
        Ok(self.git()?)
    }

    fn commits_since(
        &self,
        tips: &[Oid],
        known_tips: &[Oid],
    ) -> Result<Vec<amonhen_model::RawCommit>> {
        self.git()?.commits_since(tips, known_tips)
    }

    fn status(&self) -> Result<amonhen_model::Status> {
        self.git()?.status()
    }

    fn commit_extras(&self, commit: &Oid) -> Result<amonhen_model::CommitExtras> {
        self.git()?.commit_extras(commit)
    }

    fn verify_signatures(&self, ids: &[Oid]) -> Result<Vec<amonhen_model::SignatureCheck>> {
        self.git()?.verify_signatures(ids)
    }

    /// gix が reflog の置き場所を決める（`refs/heads/*` のものは linked worktree からでも共通の git dir）。
    fn branch_created_at(&self, names: &[&str]) -> Result<Vec<Option<Oid>>> {
        let repo = self.repo.to_thread_local();
        names
            .iter()
            .map(|name| {
                let Ok(reference) = repo.find_reference(&format!("refs/heads/{name}")) else {
                    return Ok(None);
                };
                let mut log = reference.log_iter();
                let Some(mut lines) = log.all()? else {
                    return Ok(None);
                };
                let Some(first) = lines.next() else {
                    return Ok(None);
                };
                let first = first?;
                if !first.previous_oid().is_null() {
                    return Ok(None);
                }
                Ok(Some(to_oid(&first.new_oid())?))
            })
            .collect()
    }

    /// `HEAD` は開いた worktree の git dir の、ブランチは共通の git dir の reflog（置き場所は gix が決める）。
    fn reflog(&self, name: &str, limit: usize) -> Result<Vec<amonhen_model::ReflogEntry>> {
        let repo = self.repo.to_thread_local();
        let Ok(reference) = repo.find_reference(name) else {
            return Ok(Vec::new());
        };
        let mut log = reference.log_iter();
        let Some(lines) = log.rev()? else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        for line in lines.take(limit) {
            let line = line?;
            entries.push(amonhen_model::ReflogEntry {
                old: (!line.previous_oid.is_null())
                    .then(|| to_oid(&line.previous_oid))
                    .transpose()?,
                new: to_oid(&line.new_oid)?,
                time: line.signature.time.seconds,
                message: line.message.to_string(),
            });
        }
        Ok(entries)
    }

    fn worktree_file(&self, path: &str) -> Result<Arc<Vec<u8>>> {
        let data = std::fs::read(self.git()?.workdir().join(path))?;
        self.with_filters(|filters, repo| {
            let Filters {
                pipeline,
                attributes,
                index,
                ..
            } = filters;
            let rela = Path::new(path);
            let mut others =
                attributes.selected_attribute_matches(["working-tree-encoding", "ident"]);
            let entry = attributes.at_path(rela, None, &repo.objects)?;
            // 改行の変換は CR を取り除くだけなので、CR がなく、ほかの変換（文字コード・`$Id$`）もかからなければ
            // そのまま返す（大きなファイルでは変換に 1 ms ほどかかる）。
            entry.matching_attributes(&mut others);
            let other_filters = others
                .iter_selected()
                .any(|m| !matches!(m.assignment.state, gix::attrs::StateRef::Unspecified));
            if !other_filters && !data.contains(&b'\r') {
                return Ok(Arc::new(data));
            }
            let out = pipeline.convert_to_git(
                &data[..],
                rela,
                &mut |_, attrs| {
                    entry.matching_attributes(attrs);
                },
                // 自動の改行の変換（`text=auto`）は、index の blob に CR があれば変換しない（git と同じ）。
                &mut |buf| {
                    let Some(entry) = index.entry_by_path(path.into()) else {
                        return Ok(None);
                    };
                    Ok(repo
                        .objects
                        .try_find(&entry.id, buf)
                        .ok()
                        .flatten()
                        .filter(|obj| obj.kind == gix::object::Kind::Blob)
                        .map(|_| ()))
                },
            )?;
            use gix::filter::plumbing::pipeline::convert::ToGitOutcome;
            Ok(Arc::new(match out {
                ToGitOutcome::Unchanged(_) => data,
                ToGitOutcome::Buffer(buf) => buf.to_vec(),
                ToGitOutcome::Process(_) => unreachable!("外部のフィルターは外してある"),
            }))
        })
    }

    fn line_ending_rule(&self, path: &str) -> Result<String> {
        self.with_filters(|filters, repo| {
            let Filters {
                pipeline,
                attributes,
                ..
            } = filters;
            let mut matches = attributes.selected_attribute_matches([
                "text",
                "eol",
                "crlf",
                "working-tree-encoding",
            ]);
            attributes
                .at_entry(path, None, &repo.objects)?
                .matching_attributes(&mut matches);
            // `git check-attr` と同じ書き方（`text`・`-text`・`eol=crlf`）。
            let mut parts: Vec<String> = matches
                .iter_selected()
                .filter_map(|m| {
                    let name = m.assignment.name.as_str();
                    match m.assignment.state {
                        gix::attrs::StateRef::Set => Some(name.to_owned()),
                        gix::attrs::StateRef::Unset => Some(format!("-{name}")),
                        gix::attrs::StateRef::Value(v) => Some(format!("{name}={}", v.as_bstr())),
                        gix::attrs::StateRef::Unspecified => None,
                    }
                })
                .collect();
            // 設定（core.autocrlf・core.eol）が効くときだけ足す。属性でテキストでないとしたファイルと、`eol` の
            // 属性があるファイルには効かない。core.eol は、テキストとしたファイルにだけ効く。
            let has = |name: &str| parts.iter().any(|p| p == name);
            let binary = has("-text") || has("-crlf");
            let eol_attr = parts.iter().any(|p| p.starts_with("eol="));
            let text = has("text") || has("text=auto") || has("crlf");
            let config = &pipeline.options_mut().eol_config;
            use gix::filter::plumbing::eol::{AutoCrlf, Mode};
            match (config.auto_crlf, config.eol) {
                _ if binary || eol_attr => {}
                (AutoCrlf::Enabled, _) => parts.push("core.autocrlf=true".into()),
                (AutoCrlf::Input, _) => parts.push("core.autocrlf=input".into()),
                (AutoCrlf::Disabled, Some(Mode::Lf)) if text => parts.push("core.eol=lf".into()),
                (AutoCrlf::Disabled, Some(Mode::CrLf)) if text => {
                    parts.push("core.eol=crlf".into())
                }
                (AutoCrlf::Disabled, _) => {}
            }
            Ok(parts.join(" "))
        })
    }

    fn to_worktree_form(&self, path: &str, content: &[u8]) -> Result<Vec<u8>> {
        self.with_filters(|filters, repo| {
            let Filters {
                pipeline,
                attributes,
                ..
            } = filters;
            let entry = attributes.at_entry(path, None, &repo.objects)?;
            let out = pipeline.convert_to_worktree(
                content,
                path.into(),
                &mut |_, attrs| {
                    entry.matching_attributes(attrs);
                },
                Default::default(),
            )?;
            use gix::filter::plumbing::pipeline::convert::ToWorktreeOutcome;
            Ok(match out {
                ToWorktreeOutcome::Unchanged(_) => content.to_vec(),
                ToWorktreeOutcome::Buffer(buf) => buf.to_vec(),
                ToWorktreeOutcome::Process(_) => unreachable!("外部のフィルターは外してある"),
            })
        })
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
    fn write_commit_graph(&self, split: bool) -> Result<()> {
        let repo = self.repo.to_thread_local();
        let write = |args: &[&str]| -> Result<()> {
            let out = amonhen_model::process::command("git")
                .arg("--git-dir")
                .arg(repo.common_dir())
                .args(["commit-graph", "write", "--reachable"])
                .args(args)
                .output()?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr)
                    .trim()
                    .to_owned()
                    .into());
            }
            Ok(())
        };
        // `--split` は、今の commit-graph にないコミットだけを新しい層に書く（増えた数に比例し、linux で
        // 1 万 7 千コミットなら 2 秒。全体の書き直しは 140 秒）。層が増えすぎないよう、git が小さい層を
        // まとめる。最低の git 2.23 も知っている。
        let split: &[&str] = if split { &["--split"] } else { &[] };
        // 変更のあったパスの Bloom フィルタも書き、パスでの履歴の検索（`git log -- <path>`）を速くする。
        // `--changed-paths`（2.27）・`--no-progress`（2.24）を知らない git では、付けずに書き直す
        // （標準エラーは端末でないので、進み具合はもともと出ない）。
        write(&[split, &["--changed-paths", "--no-progress"]].concat()).or_else(|_| write(split))
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
                    .ok_or_else(|| tr!("Commit {id} not found", id = id))?
                    .data
                    .len();
                buf.truncate(len);
                let (subject, author_name, author_time, signature) = {
                    let commit = gix::objs::CommitRef::from_bytes(&buf, hash)?;
                    let signature = commit
                        .extra_headers
                        .iter()
                        .find(|(name, _)| *name == "gpgsig" || *name == "gpgsig-sha256")
                        .map_or(0, |(_, value)| CommitText::signature_kind(value));
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
                        signature,
                    )
                };
                Ok(CommitText {
                    raw: Arc::new(buf),
                    subject,
                    author_name,
                    author_time,
                    signature,
                })
            })
            .collect()
    }
}
