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
}
