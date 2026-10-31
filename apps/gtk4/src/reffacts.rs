//! サイドバーが ref のほかに知っていること: ローカルブランチの分け方（コアの `branchGroups`）と、タグがどの
//! リモートにあるか（`tagRemotes`。fetch のときにコアが読んで覚えたもの）。macOS 版の `BranchGroups`・`TagRemotes`
//! （`AmonhenKit/Refs.swift`）と同じ形。

use std::collections::HashMap;

use serde_json::Value;

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
