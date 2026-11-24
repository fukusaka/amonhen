//! dot のほかの配置（`layout=neato` など）。どれも公開の論文の手法を自分で書いたもの（Graphviz のコードは使わない）。
//!
//! - neato: 最短路の距離の stress を majorization で小さくする（Gansner, Koren, North 2004）。初めの位置は
//!   pivot MDS（Brandes, Pich 2006）。
//! - fdp: ばねと斥力（Fruchterman, Reingold 1991）。クラスタは内から 1 つの箱として配置する。
//! - sfdp: 多段の粗くしたグラフ（Walshaw 2000）と Barnes–Hut の斥力（Barnes, Hut 1986; Hu 2005）。
//! - twopi: 根からの幅優先の木を同心円に置く（Eades 1992 の放射状の配置、葉の数で角を分ける）。
//! - circo: 二重連結成分ごとに円に置き、切断点でつなぐ（Six, Tollis 1999）。
//! - osage: クラスタを箱として配列に詰める。patchwork: クラスタの木を squarified treemap（Bruls, Huizing,
//!   van Wijk 2000）で敷き詰める。
//!
//! 座標は点（pt）で y が下向き。どれも乱数を使わないか、決まった種の乱数だけを使い、同じ文からはいつも同じ絵になる
//! （繰り返しの計算は四則と平方根だけにし、OS の数学関数の差で結果が変わらないようにする）。

mod common;
mod force;
mod stress;

use std::collections::BTreeSet;

use crate::Note;
use crate::graph::Attrs;

pub use common::P;

/// 配置の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Dot,
    Neato,
    /// `neato -n` に当たるもの（`layout=nop`・`nop1`・`nop2`）: `pos` を点のまま使う。
    Nop,
    Fdp,
    Sfdp,
}

impl Engine {
    /// `layout` の値や Markdown のフェンスの言葉から。知らない名前は None。
    pub fn from_name(name: &str) -> Option<Engine> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "dot" | "graphviz" | "gv" => Engine::Dot,
            "neato" => Engine::Neato,
            "nop" | "nop1" | "nop2" => Engine::Nop,
            "fdp" => Engine::Fdp,
            "sfdp" => Engine::Sfdp,
            _ => return None,
        })
    }

    /// クラスタの箱を描くか（sfdp は Graphviz でも描かない）。
    pub fn draws_clusters(self) -> bool {
        !matches!(self, Engine::Sfdp)
    }
}

/// 配置するノード。
#[derive(Clone, Debug)]
pub struct Item {
    /// 外形の大きさ（pt）。
    pub w: f64,
    pub h: f64,
    pub cluster: Option<usize>,
    /// `pos`（pt、y は下向き）。
    pub pos: Option<P>,
    /// 動かさない（`pin=true` か `pos` の `!`）。
    pub pinned: bool,
    /// patchwork の面積（pt²）。
    pub area: f64,
    /// twopi の根の印（`root=true`、またはグラフの `root` の名前）。
    pub root: bool,
    /// osage の並びの順（`sortv`）。
    pub sortv: Option<f64>,
}

/// 配置する辺。
#[derive(Clone, Debug)]
pub struct Link {
    pub tail: usize,
    pub head: usize,
    /// `len`（pt）。なければ配置ごとの既定。
    pub len: Option<f64>,
    pub weight: f64,
}

/// クラスタ。
#[derive(Clone, Debug)]
pub struct Group {
    pub parent: Option<usize>,
    pub margin: f64,
    /// 題の大きさ（pt）。
    pub label: (f64, f64),
    pub sortv: Option<f64>,
}

/// 配置の結果。
#[derive(Clone, Debug)]
pub struct Placement {
    pub nodes: Vec<P>,
    /// クラスタの箱（x0, y0, x1, y1）。描かないものは None。
    pub clusters: Vec<Option<[f64; 4]>>,
    /// 配置が決めたノードの大きさ（patchwork の升）。
    pub sizes: Option<Vec<(f64, f64)>>,
}

/// 配置する。`g` はグラフの属性。近いもので済ませた属性は `notes` に。
pub fn place(
    engine: Engine,
    items: &[Item],
    links: &[Link],
    groups: &[Group],
    g: &Attrs,
    notes: &mut BTreeSet<Note>,
) -> Placement {
    let (nodes, mut clusters) = match engine {
        Engine::Dot | Engine::Neato | Engine::Nop => {
            let nodes = stress::layout(items, links, g, engine == Engine::Nop, notes);
            (nodes, None)
        }
        Engine::Sfdp => (force::sfdp(items, links, g, notes), None),
        Engine::Fdp => {
            let (nodes, boxes) = force::fdp(items, links, groups, g, notes);
            (nodes, Some(boxes))
        }
    };
    if !engine.draws_clusters() {
        clusters = Some(vec![None; groups.len()]);
    }
    let clusters = clusters.unwrap_or_else(|| {
        // neato は外側のクラスタだけ、中のノードを囲む箱を描く（Graphviz と同じ）
        let mut b = common::cluster_bounds(items, groups, &nodes);
        for (c, g) in groups.iter().enumerate() {
            if g.parent.is_some() {
                b[c] = None;
            }
        }
        b
    });
    Placement {
        nodes,
        clusters,
        sizes: None,
    }
}
