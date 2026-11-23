//! gitGraph の形（pele の `src/diagrams/git/db.ts`）。Mermaid の gitGraph の読み手がデータベースに求める呼び出しを
//! 同じ順に行い、コミットとブランチを組む。

use std::collections::HashMap;

use super::parser::{CommitTypeName, GitGraphAst, StatementAst};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;
use crate::preprocess::Config;
use crate::util::yaml::Value;

/// コミットの種類（TS の `commitType`）。
pub mod commit_type {
    pub const NORMAL: u8 = 0;
    pub const REVERSE: u8 = 1;
    pub const HIGHLIGHT: u8 = 2;
    pub const MERGE: u8 = 3;
    pub const CHERRY_PICK: u8 = 4;
}

#[derive(Clone, Debug)]
pub struct GitCommit {
    pub id: String,
    pub message: String,
    pub seq: usize,
    pub type_: u8,
    pub tags: Vec<String>,
    pub parents: Vec<String>,
    pub branch: String,
    pub custom_type: Option<u8>,
    pub custom_id: Option<bool>,
}

/// 入れた順を保つ、名前からの表（JS の `Map`）。同じ名前に入れ直しても位置は変わらない。
#[derive(Clone, Debug)]
pub struct Ordered<V> {
    pub entries: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> Default for Ordered<V> {
    fn default() -> Self {
        Ordered {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<V> Ordered<V> {
    pub fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).map(|&k| &self.entries[k].1)
    }

    pub fn has(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    pub fn set(&mut self, key: &str, value: V) {
        match self.index.get(key) {
            Some(&k) => self.entries[k].1 = value,
            None => {
                self.index.insert(key.to_string(), self.entries.len());
                self.entries.push((key.to_string(), value));
            }
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }
}

#[derive(Clone, Debug)]
pub struct GitModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// `LR`・`TB`・`BT`。
    pub direction: String,
    /// コミットの名前ごと、名前が初めて出た順。
    pub commits: Ordered<GitCommit>,
    /// ブランチごとの最新のコミットの名前。コミットがまだなければ None。
    pub branches: Ordered<Option<String>>,
    /// 描く順のブランチの名前: `order`、次に作った順。
    pub lanes: Vec<String>,
    pub current_branch: String,
    /// 今のコミットの名前。
    pub head: Option<String>,
    pub warnings: Vec<String>,
}

fn fail(message: String) -> PeleError {
    PeleError::typed(message, ErrorCode::Semantic, "gitGraph")
}

fn type_code(name: CommitTypeName) -> u8 {
    match name {
        CommitTypeName::Reverse => commit_type::REVERSE,
        CommitTypeName::Highlight => commit_type::HIGHLIGHT,
        CommitTypeName::Normal => commit_type::NORMAL,
    }
}

struct Builder {
    commits: Ordered<GitCommit>,
    branches: Ordered<Option<String>>,
    orders: Vec<(String, Option<f64>)>,
    warnings: Vec<String>,
    head: Option<String>,
    current: String,
    direction: String,
    seq: usize,
}

impl Builder {
    /// Mermaid は順の番号に 16 進の乱数 7 桁を足す。ここでは番号から作るので、同じ文字からいつも同じ名前になる。
    fn next_id(&self) -> String {
        let mut salt = (self.seq + 1) as f64;
        loop {
            let h = (js::to_int32(salt) as u32).wrapping_mul(0x9e3779b1);
            let hex = format!("{h:08x}");
            let id = format!("{}-{}", self.seq, &hex[..7]);
            if !self.commits.has(&id) {
                return id;
            }
            salt += 0x9e3779b1u32 as f64;
        }
    }

    fn add(&mut self, commit: GitCommit) {
        self.head = Some(commit.id.clone());
        self.branches
            .set(&self.current.clone(), Some(commit.id.clone()));
        let id = commit.id.clone();
        self.commits.set(&id, commit);
    }

    fn head_id(&self) -> Option<String> {
        self.head.clone()
    }

    fn checkout(&mut self, branch: &str) -> Result<()> {
        if !self.branches.has(branch) {
            return Err(fail(format!(
                "Trying to checkout branch which is not yet created. (Help try using \"branch {branch}\")"
            )));
        }
        self.current = branch.to_string();
        let id = self.branches.get(branch).cloned().flatten();
        self.head = id
            .filter(|id| !id.is_empty())
            .filter(|id| self.commits.has(id));
        Ok(())
    }

    fn commit(&mut self, msg: String, id: Option<String>, type_: u8, tags: Vec<String>) {
        let id = match id {
            Some(id) if !id.is_empty() => id,
            _ => self.next_id(),
        };
        let commit = GitCommit {
            id,
            message: msg,
            seq: self.seq,
            type_,
            tags,
            parents: self.head_id().into_iter().collect(),
            branch: self.current.clone(),
            custom_type: None,
            custom_id: None,
        };
        self.seq += 1;
        if self.commits.has(&commit.id) {
            self.warnings
                .push(format!("Commit ID {} already exists", commit.id));
        }
        self.add(commit);
    }

    fn branch(&mut self, name: &str, order: Option<f64>) -> Result<()> {
        if self.branches.has(name) {
            return Err(fail(format!(
                "Trying to create an existing branch. (Help: Either use a new name if you want create a new branch or try using \"checkout {name}\")"
            )));
        }
        let head = self.head_id();
        self.branches.set(name, head);
        self.orders.push((name.to_string(), order));
        self.checkout(name)
    }

    fn merge(
        &mut self,
        other: &str,
        custom_id: &str,
        type_: Option<u8>,
        tags: Vec<String>,
    ) -> Result<()> {
        let current = self.current.clone();
        let current_head = self.branches.get(&current).cloned().flatten();
        let other_head = self.branches.get(other).cloned().flatten();
        let lookup = |h: &Option<String>| {
            h.as_deref()
                .filter(|h| !h.is_empty())
                .and_then(|h| self.commits.get(h))
        };
        let current_commit = lookup(&current_head);
        let other_commit = lookup(&other_head);
        if let (Some(c), Some(_)) = (current_commit, other_commit)
            && c.branch == other
        {
            return Err(fail(format!("Cannot merge branch '{other}' into itself.")));
        }
        if current == other {
            return Err(fail(
                "Incorrect usage of \"merge\". Cannot merge a branch to itself".into(),
            ));
        }
        let Some(current_commit) = current_commit else {
            return Err(fail(format!(
                "Incorrect usage of \"merge\". Current branch ({current})has no commits"
            )));
        };
        if !self.branches.has(other) {
            return Err(fail(format!(
                "Incorrect usage of \"merge\". Branch to be merged ({other}) does not exist"
            )));
        }
        let Some(other_commit) = other_commit else {
            return Err(fail(format!(
                "Incorrect usage of \"merge\". Branch to be merged ({other}) has no commits"
            )));
        };
        // 同じ名前の表の項目は同じもの。
        if current_commit.id == other_commit.id {
            return Err(fail(
                "Incorrect usage of \"merge\". Both branches have same head".into(),
            ));
        }
        if !custom_id.is_empty() && self.commits.has(custom_id) {
            return Err(fail(format!(
                "Incorrect usage of \"merge\". Commit with id:{custom_id} already exists, use different custom id"
            )));
        }
        let id = if custom_id.is_empty() {
            self.next_id()
        } else {
            custom_id.to_string()
        };
        let parents = match self.head_id() {
            None => Vec::new(),
            Some(h) => vec![h, other_head.clone().unwrap_or_default()],
        };
        let commit = GitCommit {
            id,
            message: format!("merged branch {other} into {current}"),
            seq: self.seq,
            parents,
            branch: current,
            type_: commit_type::MERGE,
            custom_type: type_,
            custom_id: Some(!custom_id.is_empty()),
            tags,
        };
        self.seq += 1;
        self.add(commit);
        Ok(())
    }

    fn cherry_pick(
        &mut self,
        source_id: Option<&str>,
        tags: Option<Vec<String>>,
        parent: Option<&str>,
    ) -> Result<()> {
        let source = source_id
            .filter(|s| !s.is_empty())
            .and_then(|s| self.commits.get(s));
        let Some(source) = source.cloned() else {
            return Err(fail(
                "Incorrect usage of \"cherryPick\". Source commit id should exist and provided"
                    .into(),
            ));
        };
        let parent = parent.filter(|p| !p.is_empty());
        if let Some(p) = parent
            && !source.parents.iter().any(|x| x == p)
        {
            return Err(fail("Invalid operation: The specified parent commit is not an immediate parent of the cherry-picked commit.".into()));
        }
        if source.type_ == commit_type::MERGE && parent.is_none() {
            return Err(fail("Incorrect usage of cherry-pick: If the source commit is a merge commit, an immediate parent commit must be specified.".into()));
        }
        if source.branch == self.current {
            return Err(fail(
                "Incorrect usage of \"cherryPick\". Source commit is already on current branch"
                    .into(),
            ));
        }
        let current = self.current.clone();
        let current_head = self.branches.get(&current).cloned().flatten();
        if !current_head
            .as_deref()
            .is_some_and(|h| !h.is_empty() && self.commits.has(h))
        {
            return Err(fail(format!(
                "Incorrect usage of \"cherry-pick\". Current branch ({current})has no commits"
            )));
        }
        let tags = match tags {
            Some(tags) => tags.into_iter().filter(|t| !t.is_empty()).collect(),
            None => {
                // `${parent}` は、ないとき `undefined` と書かれる。
                let tail = if source.type_ == commit_type::MERGE {
                    format!("|parent:{}", parent_text(parent))
                } else {
                    String::new()
                };
                vec![format!("cherry-pick:{}{tail}", source.id)]
            }
        };
        let id = self.next_id();
        let parents = match self.head_id() {
            None => Vec::new(),
            Some(h) => vec![h, source.id.clone()],
        };
        let commit = GitCommit {
            id,
            message: format!("cherry-picked {} into {current}", source.message),
            seq: self.seq,
            parents,
            branch: current,
            type_: commit_type::CHERRY_PICK,
            custom_type: None,
            custom_id: None,
            tags,
        };
        self.seq += 1;
        self.add(commit);
        Ok(())
    }
}

/// 親の書き方（ここに来るのは親があるときだけ）。
fn parent_text(parent: Option<&str>) -> &str {
    parent.unwrap_or("undefined")
}

/// `^\s+` を除く。
fn trim_lead(text: &str) -> String {
    js::trim_start(text).to_string()
}

/// `\n\s+` を改行 1 つに。
fn squeeze_lines(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        i += 1;
        if c == '\n' {
            while i < chars.len() && js::is_js_space(chars[i]) {
                i += 1;
            }
        }
    }
    out
}

pub fn build_git(ast: GitGraphAst, config: &Config, title: Option<&str>) -> Result<GitModel> {
    let git = config.get("gitGraph").and_then(Value::as_obj);
    let main_name = match git.and_then(|g| g.get("mainBranchName")) {
        Some(Value::Str(s)) => s.clone(),
        _ => "main".to_string(),
    };
    let main_order = match git.and_then(|g| g.get("mainBranchOrder")) {
        Some(Value::Num(n)) => *n,
        _ => 0.0,
    };

    let mut b = Builder {
        commits: Ordered::default(),
        branches: Ordered::default(),
        orders: vec![(main_name.clone(), Some(main_order))],
        warnings: Vec::new(),
        head: None,
        current: main_name.clone(),
        direction: "LR".to_string(),
        seq: 0,
    };
    b.branches.set(&main_name, None);

    if let Some(dir) = &ast.dir {
        b.direction = dir.clone();
    }
    for statement in ast.statements {
        match statement {
            StatementAst::Commit(c) => {
                let type_ = c.type_.map_or(commit_type::NORMAL, type_code);
                b.commit(c.message.unwrap_or_default(), c.id, type_, c.tags);
            }
            StatementAst::Branch(br) => b.branch(&br.name, Some(br.order.unwrap_or(0.0)))?,
            StatementAst::Merge(m) => b.merge(
                &m.branch,
                m.id.as_deref().unwrap_or(""),
                m.type_.map(type_code),
                m.tags,
            )?,
            StatementAst::Checkout(c) => b.checkout(&c.branch)?,
            StatementAst::CherryPicking(c) => b.cherry_pick(
                c.id.as_deref(),
                (!c.tags.is_empty()).then_some(c.tags),
                c.parent.as_deref(),
            )?,
        }
    }

    // `order` のないブランチは、Mermaid と同じく順が 0 のものの中で作った順に並ぶ。
    let mut lanes: Vec<(String, f64)> = b
        .orders
        .iter()
        .enumerate()
        .map(|(i, (name, order))| {
            (
                name.clone(),
                order.unwrap_or_else(|| js::parse_float(&format!("0.{i}"))),
            )
        })
        .collect();
    lanes.sort_by(|a, b| js::order(a.1 - b.1));
    let lanes = lanes.into_iter().map(|(name, _)| name).collect();

    Ok(GitModel {
        title: ast
            .title
            .filter(|t| !t.is_empty())
            .or(title.map(str::to_string)),
        acc_title: ast
            .acc_title
            .filter(|t| !t.is_empty())
            .map(|t| trim_lead(&t)),
        acc_descr: ast
            .acc_descr
            .filter(|t| !t.is_empty())
            .map(|t| squeeze_lines(&t)),
        direction: b.direction,
        commits: b.commits,
        branches: b.branches,
        lanes,
        current_branch: b.current,
        head: b.head,
        warnings: b.warnings,
    })
}
