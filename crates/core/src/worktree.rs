//! worktree の一覧（`git worktree list --porcelain -z` を読む）。git の考え方どおり、worktree は対等で、元のもの
//! （`.git` を持つもの）が先頭に来る。

use std::path::Path;

use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// 作業ツリーの絶対パス。
    pub path: String,
    /// HEAD のコミット（40 桁）。まだコミットがなければ nil。
    pub head: Option<String>,
    /// checkout しているブランチ（`main`）。detached なら nil。
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    /// ロックしている（`Some("")` は理由なし）。
    pub locked: Option<String>,
    /// フォルダがなくなっていて、`prune` で片付けられる（理由）。
    pub prunable: Option<String>,
    /// 一覧の先頭（`.git` を持つ元の worktree）。
    pub main: bool,
    /// このリポジトリを開いた worktree。
    pub current: bool,
}

/// `out` を読む。`current` はこのリポジトリの作業ツリー（どれが今のものかを決める）。
pub fn parse(out: &[u8], current: Option<&Path>) -> Vec<Worktree> {
    let text = String::from_utf8_lossy(out);
    let current = current.map(canonical);
    let mut list: Vec<Worktree> = Vec::new();
    // 1 行が NUL で終わり、worktree の間に空の行（NUL がもう 1 つ）がある。
    for field in text.split('\0') {
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        match key {
            "worktree" => list.push(Worktree {
                path: value.to_owned(),
                main: list.is_empty(),
                ..Default::default()
            }),
            "HEAD" => {
                if let Some(w) = list.last_mut() {
                    w.head = (!value.chars().all(|c| c == '0')).then(|| value.to_owned());
                }
            }
            "branch" => {
                if let Some(w) = list.last_mut() {
                    w.branch = Some(
                        value
                            .strip_prefix("refs/heads/")
                            .unwrap_or(value)
                            .to_owned(),
                    );
                }
            }
            "detached" => list.last_mut().into_iter().for_each(|w| w.detached = true),
            "bare" => list.last_mut().into_iter().for_each(|w| w.bare = true),
            "locked" => list
                .last_mut()
                .into_iter()
                .for_each(|w| w.locked = Some(value.to_owned())),
            "prunable" => list
                .last_mut()
                .into_iter()
                .for_each(|w| w.prunable = Some(value.to_owned())),
            _ => {}
        }
    }
    if let Some(current) = current {
        for w in &mut list {
            w.current = canonical(Path::new(&w.path)) == current;
        }
    }
    list
}

/// git の記録（`<common dir>/worktrees/<name>/`）から、ロックとフォルダがないことを足す。`git worktree list` が
/// それを出すのは git 2.31 からなので、古い git でも同じに見えるように（ロックの理由は `locked`、場所は `gitdir`
/// のファイル）。一覧がもう出しているものは変えない。
pub fn fill_from_records(list: &mut [Worktree], common_dir: &Path) {
    let Ok(records) = std::fs::read_dir(common_dir.join("worktrees")) else {
        return;
    };
    for record in records.flatten() {
        let dir = record.path();
        let Ok(gitdir) = std::fs::read_to_string(dir.join("gitdir")) else {
            continue;
        };
        // `gitdir` は worktree の `.git` ファイルの場所。その親が worktree。
        let dot_git = Path::new(gitdir.trim());
        let Some(folder) = dot_git.parent() else {
            continue;
        };
        let folder = canonical(folder);
        let Some(w) = list
            .iter_mut()
            .find(|w| !w.main && canonical(Path::new(&w.path)) == folder)
        else {
            continue;
        };
        if w.locked.is_none()
            && let Ok(reason) = std::fs::read_to_string(dir.join("locked"))
        {
            w.locked = Some(reason.trim().to_owned());
        }
        if w.prunable.is_none() && !dot_git.exists() && w.locked.is_none() {
            w.prunable = Some("gitdir file points to non-existent location".to_owned());
        }
    }
}

/// シンボリックリンク（macOS の /var → /private/var など）を解いたパス。解けなければそのまま。
fn canonical(path: &Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_porcelain_list() {
        let out = "worktree /repo\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
                   worktree /repo/.claude/worktrees/a\0HEAD 2222222222222222222222222222222222222222\0detached\0locked\0\0\
                   worktree /gone\0HEAD 3333333333333333333333333333333333333333\0branch refs/heads/topic\0prunable gitdir file points to non-existent location\0\0";
        let list = parse(out.as_bytes(), Some(Path::new("/repo/.claude/worktrees/a")));
        assert_eq!(list.len(), 3);
        assert!(list[0].main && list[0].branch.as_deref() == Some("main") && !list[0].current);
        assert!(
            list[1].detached && list[1].branch.is_none() && list[1].locked.as_deref() == Some("")
        );
        assert!(list[1].current);
        assert_eq!(list[2].branch.as_deref(), Some("topic"));
        assert!(list[2].prunable.is_some() && !list[2].main);
    }

    /// 古い git（2.31 より前）は一覧にロックとフォルダがないことを出さないので、記録から足す。
    #[test]
    fn fills_locks_and_missing_folders_from_the_records() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join("main/.git");
        let here = tmp.path().join("here");
        std::fs::create_dir_all(&here).unwrap();
        std::fs::write(here.join(".git"), "gitdir: x").unwrap();
        let record = |name: &str, folder: &Path, locked: Option<&str>| {
            let dir = common.join("worktrees").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("gitdir"),
                format!("{}\n", folder.join(".git").display()),
            )
            .unwrap();
            if let Some(reason) = locked {
                std::fs::write(dir.join("locked"), reason).unwrap();
            }
        };
        let gone = tmp.path().join("gone");
        record("here", &here, Some("on a stick\n"));
        record("gone", &gone, None);
        let entry = |path: &Path, main: bool| Worktree {
            path: path.display().to_string(),
            main,
            ..Default::default()
        };
        let mut list = vec![
            entry(&tmp.path().join("main"), true),
            entry(&here, false),
            entry(&gone, false),
        ];
        fill_from_records(&mut list, &common);
        assert_eq!(list[0].locked, None);
        assert_eq!(list[1].locked.as_deref(), Some("on a stick"));
        assert_eq!(list[1].prunable, None);
        assert_eq!(list[2].locked, None);
        assert!(list[2].prunable.is_some());
    }
}
