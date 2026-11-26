//! 読んだ文からグラフ（ノード・辺・クラスタ・ランクの組）を作る。属性の既定（`graph|node|edge [..]`）は subgraph
//! ごとの範囲を持ち、ノードと辺は作られたときの既定を受け継ぐ（Graphviz と同じ）。

use std::collections::HashMap;

use crate::Error;
use crate::parse::{AttrKind, AttrList, EdgeEnd, Graph, NodeRef, Stmt, Subgraph, Value};

#[derive(Clone, Debug, Default)]
pub struct Attrs(HashMap<String, Value>);

impl Attrs {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|v| v.text.as_str())
    }

    pub fn value(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }

    fn extend(&mut self, list: &AttrList) {
        for (k, v) in list {
            self.0.insert(k.clone(), v.clone());
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// 数（読めなければ None）。
    pub fn num(&self, key: &str) -> Option<f64> {
        self.get(key)?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }

    /// 真偽（`true`・`yes`・0 でない数）。
    pub fn flag(&self, key: &str) -> Option<bool> {
        let v = self.get(key)?.trim().to_ascii_lowercase();
        Some(match v.as_str() {
            "true" | "yes" => true,
            "false" | "no" | "" => false,
            _ => v.parse::<f64>().map(|n| n != 0.0).unwrap_or(false),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub attrs: Attrs,
    /// いちばん内のクラスタ。
    pub cluster: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub tail: usize,
    pub head: usize,
    pub tail_port: Option<String>,
    pub head_port: Option<String>,
    pub attrs: Attrs,
}

#[derive(Clone, Debug)]
pub struct Cluster {
    pub name: String,
    pub attrs: Attrs,
    pub parent: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankKind {
    Same,
    Min,
    Max,
    Source,
    Sink,
}

#[derive(Clone, Debug)]
pub struct RankSet {
    pub kind: RankKind,
    pub nodes: Vec<usize>,
    /// クラスタの部分グラフそのものに書いた `rank`（dot はクラスタごとにランクを決めるとき読まない）。
    pub on_cluster: bool,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub directed: bool,
    pub strict: bool,
    pub name: String,
    pub attrs: Attrs,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub clusters: Vec<Cluster>,
    pub rank_sets: Vec<RankSet>,
}

/// 上限。
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub nodes: usize,
    pub edges: usize,
}

#[derive(Clone)]
struct Scope {
    graph: Attrs,
    node: Attrs,
    edge: Attrs,
    cluster: Option<usize>,
}

struct Builder {
    model: Model,
    index: HashMap<String, usize>,
    cluster_index: HashMap<String, usize>,
    /// 辺のある組（strict のとき）。
    pairs: HashMap<(usize, usize), usize>,
    limits: Limits,
}

impl Builder {
    fn is_ancestor(&self, a: usize, mut b: usize) -> bool {
        loop {
            if a == b {
                return true;
            }
            match self.model.clusters[b].parent {
                Some(p) => b = p,
                None => return false,
            }
        }
    }

    fn node(&mut self, id: &str, scope: &Scope) -> Result<usize, Error> {
        let i = match self.index.get(id) {
            Some(&i) => i,
            None => {
                if self.model.nodes.len() >= self.limits.nodes {
                    return Err(Error::TooManyNodes {
                        limit: self.limits.nodes,
                    });
                }
                let i = self.model.nodes.len();
                self.model.nodes.push(Node {
                    name: id.to_owned(),
                    attrs: scope.node.clone(),
                    cluster: None,
                });
                self.index.insert(id.to_owned(), i);
                i
            }
        };
        if let Some(c) = scope.cluster {
            // 先に入ったクラスタの外の別のクラスタには移さない（入れ子の内へは移す）
            let move_in = match self.model.nodes[i].cluster {
                None => true,
                Some(old) => old != c && self.is_ancestor(old, c),
            };
            if move_in {
                self.model.nodes[i].cluster = Some(c);
            }
        }
        Ok(i)
    }

    fn edge(
        &mut self,
        tail: usize,
        head: usize,
        tail_port: Option<String>,
        head_port: Option<String>,
        attrs: &Attrs,
    ) -> Result<(), Error> {
        if self.model.strict {
            let key = if self.model.directed || tail <= head {
                (tail, head)
            } else {
                (head, tail)
            };
            if let Some(&e) = self.pairs.get(&key) {
                let edge = &mut self.model.edges[e];
                for (k, v) in &attrs.0 {
                    edge.attrs.set(k, v.clone());
                }
                return Ok(());
            }
            self.pairs.insert(key, self.model.edges.len());
        }
        if self.model.edges.len() >= self.limits.edges {
            return Err(Error::TooManyEdges {
                limit: self.limits.edges,
            });
        }
        self.model.edges.push(Edge {
            tail,
            head,
            tail_port,
            head_port,
            attrs: attrs.clone(),
        });
        Ok(())
    }

    /// 文を読み、触れたノードを返す。
    fn stmts(
        &mut self,
        stmts: &[Stmt],
        scope: &mut Scope,
        own: &mut Attrs,
    ) -> Result<Vec<usize>, Error> {
        let mut touched: Vec<usize> = Vec::new();
        for stmt in stmts {
            match stmt {
                Stmt::Attr(AttrKind::Graph, list) => {
                    scope.graph.extend(list);
                    own.extend(list);
                }
                Stmt::Assign(k, v) => {
                    scope.graph.set(k, v.clone());
                    own.set(k, v.clone());
                }
                Stmt::Attr(AttrKind::Node, list) => scope.node.extend(list),
                Stmt::Attr(AttrKind::Edge, list) => scope.edge.extend(list),
                Stmt::Node(NodeRef { id, .. }, list) => {
                    let i = self.node(id, scope)?;
                    self.model.nodes[i].attrs.extend(list);
                    touched.push(i);
                }
                Stmt::Subgraph(sub) => {
                    let nodes = self.subgraph(sub, scope)?;
                    touched.extend(nodes);
                }
                Stmt::Edge(ends, list) => {
                    let mut attrs = scope.edge.clone();
                    attrs.extend(list);
                    let mut prev: Option<Vec<(usize, Option<String>)>> = None;
                    for end in ends {
                        let group: Vec<(usize, Option<String>)> = match end {
                            EdgeEnd::Node(NodeRef { id, port }) => {
                                let i = self.node(id, scope)?;
                                touched.push(i);
                                vec![(i, port.clone())]
                            }
                            EdgeEnd::Subgraph(sub) => {
                                let nodes = self.subgraph(sub, scope)?;
                                touched.extend(nodes.iter().copied());
                                // subgraph はノードの集まりなので、中で 2 度出てくるノードにも辺は 1 本
                                // （`a -> {b c b}` は b へ 1 本。Graphviz と同じ）
                                let mut seen = std::collections::HashSet::new();
                                nodes
                                    .into_iter()
                                    .filter(|n| seen.insert(*n))
                                    .map(|n| (n, None))
                                    .collect()
                            }
                        };
                        if let Some(prev) = &prev {
                            if prev.len().saturating_mul(group.len()) > self.limits.edges {
                                return Err(Error::TooManyEdges {
                                    limit: self.limits.edges,
                                });
                            }
                            for (t, tp) in prev {
                                for (h, hp) in &group {
                                    self.edge(*t, *h, tp.clone(), hp.clone(), &attrs)?;
                                }
                            }
                        }
                        prev = Some(group);
                    }
                }
            }
        }
        Ok(touched)
    }

    fn subgraph(&mut self, sub: &Subgraph, parent: &Scope) -> Result<Vec<usize>, Error> {
        let mut scope = parent.clone();
        let name = sub.id.clone().unwrap_or_default();
        let mut own = Attrs::default();
        // クラスタの名前の印（`cluster` で始まる）。`cluster=true` は読んだあとで見る。
        let named_cluster = name.starts_with("cluster");
        let mut cluster = None;
        if named_cluster {
            cluster = Some(self.cluster(&name, &scope));
            scope.cluster = cluster;
        }
        let nodes = self.stmts(&sub.stmts, &mut scope, &mut own)?;
        if cluster.is_none() && own.flag("cluster") == Some(true) {
            // `cluster=true` は読んだあとでわかる。ノードを入れ直す。
            let c = self.cluster(&name, parent);
            for &n in &nodes {
                let move_in = match self.model.nodes[n].cluster {
                    None => true,
                    Some(old) => old != c && self.is_ancestor(old, c),
                };
                if move_in {
                    self.model.nodes[n].cluster = Some(c);
                }
            }
            cluster = Some(c);
        }
        if let Some(c) = cluster {
            let attrs = &mut self.model.clusters[c].attrs;
            for (k, v) in &scope.graph.0 {
                attrs.set(k, v.clone());
            }
        }
        if let Some(rank) = own.get("rank") {
            let kind = match rank.trim().to_ascii_lowercase().as_str() {
                "same" => Some(RankKind::Same),
                "min" => Some(RankKind::Min),
                "max" => Some(RankKind::Max),
                "source" => Some(RankKind::Source),
                "sink" => Some(RankKind::Sink),
                _ => None,
            };
            if let Some(kind) = kind {
                let mut list = nodes.clone();
                list.sort_unstable();
                list.dedup();
                if !list.is_empty() {
                    self.model.rank_sets.push(RankSet {
                        kind,
                        nodes: list,
                        on_cluster: cluster.is_some(),
                    });
                }
            }
        }
        Ok(nodes)
    }

    fn cluster(&mut self, name: &str, scope: &Scope) -> usize {
        if let Some(&c) = self.cluster_index.get(name).filter(|_| !name.is_empty()) {
            return c;
        }
        let c = self.model.clusters.len();
        self.model.clusters.push(Cluster {
            name: name.to_owned(),
            attrs: scope.graph.clone(),
            parent: scope.cluster,
        });
        self.cluster_index.insert(name.to_owned(), c);
        c
    }
}

/// 読んだグラフからモデルを作る。
pub fn build(graph: &Graph, limits: Limits) -> Result<Model, Error> {
    let mut b = Builder {
        model: Model {
            directed: graph.directed,
            strict: graph.strict,
            name: graph.id.clone().unwrap_or_default(),
            attrs: Attrs::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
            clusters: Vec::new(),
            rank_sets: Vec::new(),
        },
        index: HashMap::new(),
        cluster_index: HashMap::new(),
        pairs: HashMap::new(),
        limits,
    };
    let mut scope = Scope {
        graph: Attrs::default(),
        node: Attrs::default(),
        edge: Attrs::default(),
        cluster: None,
    };
    let mut own = Attrs::default();
    b.stmts(&graph.stmts, &mut scope, &mut own)?;
    b.model.attrs = scope.graph;
    Ok(b.model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    const LIMITS: Limits = Limits {
        nodes: 1000,
        edges: 1000,
    };

    fn model(src: &str) -> Model {
        build(&parse(src).unwrap(), LIMITS).unwrap()
    }

    #[test]
    fn an_edge_to_a_subgraph_reaches_each_node_once() {
        // 中で 2 度出てくるノード（並べ直しや、中の辺の端）にも 1 本
        let m = model("digraph { a -> { b c b }; { x; y; x -> y } -> z }");
        let pairs: Vec<(&str, &str)> = m
            .edges
            .iter()
            .map(|e| (m.nodes[e.tail].name.as_str(), m.nodes[e.head].name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [("a", "b"), ("a", "c"), ("x", "y"), ("x", "z"), ("y", "z")]
        );
    }

    #[test]
    fn scopes_defaults() {
        let m = model(
            "digraph { a; node [shape=box]; b; subgraph s { node [color=red]; c; a } d; edge [color=blue]; b -> d; { edge [style=dashed] c -> d } }",
        );
        let get = |n: usize, k: &str| m.nodes[n].attrs.get(k).map(str::to_owned);
        assert_eq!(m.nodes[0].name, "a");
        assert_eq!(get(0, "shape"), None);
        assert_eq!(get(1, "shape").as_deref(), Some("box"));
        assert_eq!(get(2, "color").as_deref(), Some("red"));
        // 前に作ったノードは subgraph の既定を受けない
        assert_eq!(get(0, "color"), None);
        assert_eq!(get(3, "color"), None);
        assert_eq!(m.edges[0].attrs.get("color"), Some("blue"));
        assert_eq!(m.edges[1].attrs.get("style"), Some("dashed"));
        assert_eq!(m.edges[1].attrs.get("color"), Some("blue"));
    }

    #[test]
    fn expands_edge_chains_and_groups() {
        let m = model("digraph { a -> b -> c; {x y} -> z; a:p:n -> b:s }");
        let pairs: Vec<(String, String)> = m
            .edges
            .iter()
            .map(|e| (m.nodes[e.tail].name.clone(), m.nodes[e.head].name.clone()))
            .collect();
        assert_eq!(
            pairs,
            [("a", "b"), ("b", "c"), ("x", "z"), ("y", "z"), ("a", "b")]
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
        );
        assert_eq!(m.edges[4].tail_port.as_deref(), Some("p:n"));
        assert_eq!(m.edges[4].head_port.as_deref(), Some("s"));
    }

    #[test]
    fn strict_merges_edges() {
        let m = model("strict graph { a -- b; b -- a [color=red]; a -- a }");
        assert_eq!(m.edges.len(), 2);
        assert_eq!(m.edges[0].attrs.get("color"), Some("red"));
    }

    #[test]
    fn finds_clusters_and_ranks() {
        let m = model(
            "digraph { label=root; subgraph cluster_a { label=A; a; subgraph cluster_b { b } } subgraph x { rank=same; c; d } { rank=min; e } subgraph y { cluster=true; f } a; }",
        );
        assert_eq!(m.clusters.len(), 3);
        assert_eq!(m.clusters[0].attrs.get("label"), Some("A"));
        assert_eq!(m.clusters[1].parent, Some(0));
        assert_eq!(m.nodes[0].cluster, Some(0));
        assert_eq!(m.nodes[1].cluster, Some(1));
        assert_eq!(m.nodes[5].cluster, Some(2));
        // 入れ子のクラスタは上のラベルを受け継ぐ（Graphviz と同じ）
        assert_eq!(m.clusters[1].attrs.get("label"), Some("A"));
        assert_eq!(m.rank_sets.len(), 2);
        assert_eq!(m.rank_sets[0].kind, RankKind::Same);
        assert_eq!(m.rank_sets[0].nodes, vec![2, 3]);
        assert_eq!(m.attrs.get("label"), Some("root"));
    }

    #[test]
    fn limits_sizes() {
        let tiny = Limits { nodes: 3, edges: 2 };
        assert!(build(&parse("digraph { a b c d }").unwrap(), tiny).is_err());
        assert!(build(&parse("digraph { a -> b -> c -> a }").unwrap(), tiny).is_err());
        assert!(build(&parse("digraph { {a b} -> {c} }").unwrap(), tiny).is_ok());
    }
}
