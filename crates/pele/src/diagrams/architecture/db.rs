//! architecture の形（pele の `src/diagrams/architecture/db.ts`）。

use std::collections::{HashMap, HashSet};

use super::parser::ArchitectureAst;
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

/// サービスか分岐点。
#[derive(Clone, Debug, PartialEq)]
pub struct ArchNode {
    pub id: String,
    pub junction: bool,
    pub icon: Option<String>,
    pub icon_text: Option<String>,
    pub title: Option<String>,
    pub in_: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArchGroup {
    pub id: String,
    pub icon: Option<String>,
    pub title: Option<String>,
    pub in_: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArchEdge {
    pub lhs_id: String,
    pub lhs_dir: char,
    pub lhs_into: bool,
    pub lhs_group: bool,
    pub rhs_id: String,
    pub rhs_dir: char,
    pub rhs_into: bool,
    pub rhs_group: bool,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArchLayoutHint {
    /// 'row' か 'column'。
    pub direction: String,
    pub members: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Registered {
    Node,
    Group,
}

impl Registered {
    fn name(self) -> &'static str {
        match self {
            Registered::Node => "node",
            Registered::Group => "group",
        }
    }
}

fn fail(message: String) -> PeleError {
    PeleError::typed(message, ErrorCode::Semantic, "architecture")
}

#[derive(Default)]
pub struct ArchitectureDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// 入れた順（JS の `Map`。id は重ならない）。
    pub nodes: Vec<ArchNode>,
    pub groups: Vec<ArchGroup>,
    pub edges: Vec<ArchEdge>,
    pub layout_hints: Vec<ArchLayoutHint>,
    node_index: HashMap<String, usize>,
    registered: HashMap<String, Registered>,
}

impl ArchitectureDb {
    fn register(&mut self, kind: &str, id: &str, parent: Option<&str>) -> Result<()> {
        if let Some(r) = self.registered.get(id) {
            return Err(fail(format!(
                "The {kind} id [{id}] is already in use by another {}",
                r.name()
            )));
        }
        if let Some(parent) = parent {
            if id == parent {
                return Err(fail(format!(
                    "The {kind} [{id}] cannot be placed within itself"
                )));
            }
            match self.registered.get(parent) {
                None => {
                    return Err(fail(format!(
                        "The {kind} [{id}]'s parent does not exist. Please make sure the parent is created before this {kind}"
                    )));
                }
                Some(Registered::Node) => {
                    return Err(fail(format!("The {kind} [{id}]'s parent is not a group")));
                }
                Some(Registered::Group) => {}
            }
        }
        self.registered.insert(
            id.to_string(),
            if kind == "group" {
                Registered::Group
            } else {
                Registered::Node
            },
        );
        Ok(())
    }

    pub fn add_node(&mut self, node: ArchNode) -> Result<()> {
        let kind = if node.junction { "junction" } else { "service" };
        self.register(kind, &node.id, node.in_.as_deref())?;
        self.node_index.insert(node.id.clone(), self.nodes.len());
        self.nodes.push(node);
        Ok(())
    }

    pub fn add_group(&mut self, group: ArchGroup) -> Result<()> {
        self.register("group", &group.id, group.in_.as_deref())?;
        self.groups.push(group);
        Ok(())
    }

    pub fn node(&self, id: &str) -> Option<&ArchNode> {
        self.node_index.get(id).map(|&k| &self.nodes[k])
    }

    pub fn add_edge(&mut self, edge: ArchEdge) -> Result<()> {
        let ends = [
            ("left", &edge.lhs_id, edge.lhs_group),
            ("right", &edge.rhs_id, edge.rhs_group),
        ];
        for (side, id, _) in ends {
            if !self.registered.contains_key(id.as_str()) {
                return Err(fail(format!(
                    "The {side}-hand id [{id}] does not yet exist. Please create the service/group before declaring an edge to it."
                )));
            }
        }
        // Mermaid はグループの id を上の確かめで通し、その後 TypeError で落ちる。
        for (side, id, _) in ends {
            if self.node(id).is_none() {
                return Err(fail(format!(
                    "The {side}-hand id [{id}] is a group. Add {{group}} to a service inside it instead."
                )));
            }
        }
        let parent = self.node(&edge.lhs_id).unwrap().in_.clone();
        if parent.is_some() && parent == self.node(&edge.rhs_id).unwrap().in_ {
            for (side, id, crosses) in ends {
                if crosses {
                    return Err(fail(format!(
                        "The {side}-hand id [{id}] is modified to traverse the group boundary, but the edge does not pass through two groups."
                    )));
                }
            }
        }
        self.edges.push(edge);
        Ok(())
    }

    pub fn add_layout_hint(&mut self, hint: ArchLayoutHint) -> Result<()> {
        if hint.members.len() < 2 {
            return Err(fail(format!(
                "An align directive requires at least two members; got {}",
                hint.members.len()
            )));
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for id in &hint.members {
            if self.registered.get(id.as_str()) != Some(&Registered::Node) {
                return Err(fail(format!(
                    "align {} references [{id}], which is not a service or junction",
                    hint.direction
                )));
            }
            if !seen.insert(id) {
                return Err(fail(format!(
                    "align {} lists [{id}] more than once",
                    hint.direction
                )));
            }
        }
        self.layout_hints.push(hint);
        Ok(())
    }
}

/// `.replace(/\n\s+/g, '\n')`
fn squeeze_indents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|&n| js::is_js_space(n)) {
                chars.next();
            }
        }
    }
    out
}

/// Mermaid の populateDb: グループをすべて、次にサービス、分岐点、辺、並べ方。
pub fn populate(ast: &ArchitectureAst, db: &mut ArchitectureDb) -> Result<()> {
    if let Some(d) = ast.acc_descr.as_deref().filter(|d| !d.is_empty()) {
        db.acc_descr = Some(squeeze_indents(d));
    }
    if let Some(t) = ast.acc_title.as_deref().filter(|t| !t.is_empty()) {
        db.acc_title = Some(js::trim_start(t).to_string());
    }
    if let Some(t) = ast.title.as_deref().filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    for g in &ast.groups {
        db.add_group(ArchGroup {
            id: g.id.clone(),
            icon: g.icon.clone(),
            title: g.title.clone(),
            in_: g.in_.clone(),
        })?;
    }
    for s in &ast.services {
        db.add_node(ArchNode {
            id: s.id.clone(),
            junction: false,
            icon: s.icon.clone(),
            icon_text: s.icon_text.clone(),
            title: s.title.clone(),
            in_: s.in_.clone(),
        })?;
    }
    for j in &ast.junctions {
        db.add_node(ArchNode {
            id: j.id.clone(),
            junction: true,
            icon: None,
            icon_text: None,
            title: None,
            in_: j.in_.clone(),
        })?;
    }
    for e in &ast.edges {
        db.add_edge(ArchEdge {
            lhs_id: e.lhs_id.clone(),
            lhs_dir: e.lhs_dir,
            lhs_into: e.lhs_into,
            lhs_group: e.lhs_group,
            rhs_id: e.rhs_id.clone(),
            rhs_dir: e.rhs_dir,
            rhs_into: e.rhs_into,
            rhs_group: e.rhs_group,
            title: e.title.clone(),
        })?;
    }
    for a in &ast.alignments {
        db.add_layout_hint(ArchLayoutHint {
            direction: a.direction.clone(),
            members: a.members.clone(),
        })?;
    }
    Ok(())
}
