//! サイドバーが ref のほかに知っていること: ローカルブランチの分け方（コアの `branchGroups`）と、タグがどの
//! リモートにあるか（`tagRemotes`。fetch のときにコアが読んで覚えたもの）。macOS 版の `BranchGroups`・`TagRemotes`
//! （`AmonhenKit/Refs.swift`）と同じ形。

use std::collections::HashMap;

use amonhen_model::tr;
use serde_json::Value;

use crate::icon;

/// ブランチ 1 本の分け方。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    /// 先端のコミットの日時（UNIX 時間）。
    pub time: Option<i64>,
    /// 既定のブランチに取り込み済み。
    pub merged: bool,
    /// 既定のブランチの先端と同じ（作ったばかり）。
    pub at_base: bool,
    /// その上に積んであるブランチ（土台）。既定のブランチの上なら None。
    pub parent: Option<String>,
    /// 手でアーカイブした（`branch.<name>.amonhenArchived`）。
    pub archived: bool,
    /// お気に入りの順位（`branch.<name>.amonhenPinned`。小さいほど上）。
    pub pinned: Option<i64>,
}

/// `branchGroups` の答え。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchGroups {
    /// 既定のブランチ（`origin/main`・`main` など）。
    pub base: Option<String>,
    pub branches: HashMap<String, Branch>,
}

impl BranchGroups {
    pub fn from(info: &Value) -> Option<Self> {
        let list = info["branches"].as_array()?;
        let branches = list
            .iter()
            .filter_map(|b| {
                let name = b["name"].as_str()?.to_owned();
                let branch = Branch {
                    name: name.clone(),
                    time: b["time"].as_i64(),
                    merged: b["merged"].as_bool().unwrap_or(false),
                    at_base: b["atBase"].as_bool().unwrap_or(false),
                    parent: b["parent"].as_str().map(str::to_owned),
                    archived: b["archived"].as_bool().unwrap_or(false),
                    pinned: b["pinned"].as_i64(),
                };
                Some((name, branch))
            })
            .collect();
        Some(Self {
            base: info["base"].as_str().map(str::to_owned),
            branches,
        })
    }

    pub fn get(&self, name: &str) -> Option<&Branch> {
        self.branches.get(name)
    }

    /// 既定のブランチに取り込み済み（先端が同じ、作ったばかりのものは除く）。
    pub fn is_merged(&self, name: &str) -> bool {
        self.get(name).is_some_and(|b| b.merged && !b.at_base)
    }

    pub fn is_archived(&self, name: &str) -> bool {
        self.get(name).is_some_and(|b| b.archived)
    }

    /// 既定のブランチの名前（なければ main）。
    pub fn base_name(&self) -> &str {
        self.base.as_deref().unwrap_or("main")
    }
}
/// タグの、リモート 1 つでの様子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagState {
    /// 同じタグ（ref が同じオブジェクトを指す）。
    Same,
    /// 同じ名前の別のタグ。置き換えないと push できない。
    Different,
    /// そのリモートにはない。
    Missing,
}

/// `tagRemotes` の答え。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagRemotes {
    /// すべてのリモートと、タグを読んだ時刻（UNIX 時刻のミリ秒。まだ読んでいなければ None）。
    pub remotes: Vec<(String, Option<i64>)>,
    /// タグ → 読んだリモート → 様子（読んでいないリモートは入らない）。
    pub tags: HashMap<String, HashMap<String, TagState>>,
}

impl TagRemotes {
    pub fn from(info: &Value) -> Option<Self> {
        let remotes = info["remotes"]
            .as_array()?
            .iter()
            .filter_map(|r| Some((r["name"].as_str()?.to_owned(), r["checked"].as_i64())))
            .collect();
        let tags = info["tags"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(tag, states)| {
                let states = states
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(remote, s)| {
                        let state = match s.as_str()? {
                            "same" => TagState::Same,
                            "different" => TagState::Different,
                            _ => TagState::Missing,
                        };
                        Some((remote.clone(), state))
                    })
                    .collect();
                (tag.clone(), states)
            })
            .collect();
        Some(Self { remotes, tags })
    }

    /// `tag` の `remote` での様子。まだ読んでいないリモートなら None。
    pub fn state(&self, tag: &str, remote: &str) -> Option<TagState> {
        self.tags.get(tag)?.get(remote).copied()
    }

    /// `remote` のタグを読んだ時刻（ミリ秒）。
    pub fn checked(&self, remote: &str) -> Option<i64> {
        self.remotes
            .iter()
            .find(|(n, _)| n == remote)
            .and_then(|(_, c)| *c)
    }

    /// タグの行の右端の印（アイコン・意味・色の CSS のクラス）: どの（読んだ）リモートにもない、または、どれかの
    /// リモートに同じ名前の別のタグがある。
    pub fn mark(&self, tag: &str) -> Option<(icon::Kind, String, Option<&'static str>)> {
        let checked: Vec<&str> = self
            .remotes
            .iter()
            .filter(|(_, c)| c.is_some())
            .map(|(n, _)| n.as_str())
            .collect();
        if checked.is_empty() {
            return None;
        }
        let different: Vec<&str> = checked
            .iter()
            .copied()
            .filter(|r| self.state(tag, r) == Some(TagState::Different))
            .collect();
        if !different.is_empty() {
            return Some((
                icon::Kind::CloudAlert,
                tr!(
                    "A different tag with this name is on {remotes}",
                    remotes = different.join(", ")
                ),
                Some("warning"),
            ));
        }
        if checked
            .iter()
            .all(|r| self.state(tag, r) == Some(TagState::Missing))
        {
            return Some((
                icon::Kind::CloudSlash,
                tr!("Not on any remote (as of the last fetch)"),
                None,
            ));
        }
        None
    }
}

/// タグの、リモート 1 つでの様子の一文（ツールチップ）。
pub fn tag_state_text(tags: Option<&TagRemotes>, tag: &str, remote: &str) -> String {
    let Some(tags) = tags.filter(|t| t.checked(remote).is_some()) else {
        return tr!(
            "{remote}: not checked yet (Fetch reads it)",
            remote = remote
        );
    };
    match tags.state(tag, remote) {
        Some(TagState::Same) => tr!("{remote}: has it", remote = remote),
        Some(TagState::Different) => {
            tr!(
                "{remote}: has a different tag with this name",
                remote = remote
            )
        }
        _ => tr!("{remote}: doesn’t have it", remote = remote),
    }
}
