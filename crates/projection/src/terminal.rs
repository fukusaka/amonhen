//! グラフを端末のセル（罫線の文字）にする。`amonhen log` と TUI が同じものを使う。
//!
//! 点と線はコアのレイアウト（アプリと同じ）をそのまま使う。レーンが多いリポジトリ（linux で 1 行を横切る線が
//! 数百本）でも幅に収まるよう、渡された行の範囲に点か曲線のあるレーンだけに列を取り、線が通るだけのレーンは
//! 隣り合うものを細い 1 列（┆）にまとめる（アプリの魚眼と同じ考え）。
//!
//! 曲線はどれも隣り合う 2 行をつなぐ。描き方は 2 つ:
//! - コミットの行の間に、曲線があるときだけ「つなぎの行」を挟む（[`Graph::commit_line`]・
//!   [`Graph::connector_line`]）。`amonhen log` の形。
//! - 1 コミットを必ず 1 行にし、曲線をコミットの行に描く（[`Graph::row_line`]）。行の高さがそろうので、
//!   スクロールする画面（TUI）の形。

use std::collections::{BTreeSet, HashMap, HashSet};

use amonhen_layout::{Edge, Node};

const UP: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;

/// 線の色（アプリの色の番号を、端末の 256 色に寄せる）。
pub const PALETTE: [u8; 8] = [75, 114, 179, 176, 80, 209, 141, 150];

/// レイアウトの色の番号から、端末の 256 色の番号。
pub fn palette(color: u16) -> u8 {
    PALETTE[color as usize % PALETTE.len()]
}

/// 線の向きの組から罫線の文字。
fn line_char(dirs: u8) -> char {
    match dirs {
        0 => ' ',
        d if d == UP | DOWN || d == UP || d == DOWN => '│',
        d if d == LEFT | RIGHT || d == LEFT || d == RIGHT => '─',
        d if d == UP | RIGHT => '╰',
        d if d == UP | LEFT => '╯',
        d if d == DOWN | RIGHT => '╭',
        d if d == DOWN | LEFT => '╮',
        d if d == UP | DOWN | RIGHT => '├',
        d if d == UP | DOWN | LEFT => '┤',
        d if d == LEFT | RIGHT | DOWN => '┬',
        d if d == LEFT | RIGHT | UP => '┴',
        _ => '┼',
    }
}

/// 1 行分の格子（半列単位: 偶数がレーン、奇数がその間）。
pub struct Line {
    dirs: Vec<u8>,
    glyph: Vec<Option<char>>,
    color: Vec<Option<u16>>,
}

impl Line {
    fn new(width: usize) -> Self {
        Self {
            dirs: vec![0; width],
            glyph: vec![None; width],
            color: vec![None; width],
        }
    }

    fn add(&mut self, x: usize, dirs: u8, color: u16) {
        if x < self.dirs.len() {
            self.dirs[x] |= dirs;
            self.color[x].get_or_insert(color);
        }
    }

    /// セルの数（どの行も [`Graph::width`]）。
    pub fn len(&self) -> usize {
        self.dirs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty()
    }

    /// セルごとの文字と、レイアウトの色の番号（空白は色なし）。
    pub fn cells(&self) -> impl Iterator<Item = (char, Option<u16>)> + '_ {
        (0..self.dirs.len()).map(|x| {
            let ch = self.glyph[x].unwrap_or_else(|| line_char(self.dirs[x]));
            (ch, if ch == ' ' { None } else { self.color[x] })
        })
    }
}

/// 表示する行の範囲の点と線と、レーンから列への対応。
pub struct Graph<'a> {
    edges: &'a [Edge],
    /// レーン → 半列の位置。
    column: HashMap<u16, usize>,
    /// 細い列にまとめたレーン。
    thin: HashSet<u16>,
    width: usize,
}

impl<'a> Graph<'a> {
    /// `nodes`（表示する行の点）と `edges`（その範囲に重なる線）から、列の割り当てを決める。
    pub fn new(nodes: &[Node], edges: &'a [Edge]) -> Self {
        // 点か曲線の端があるレーンは広く、線が通るだけのレーンは細く。
        let mut wide = BTreeSet::new();
        for n in nodes {
            wide.insert(n.lane);
        }
        let mut through = BTreeSet::new();
        for e in edges {
            if e.lane_start != e.lane_end {
                wide.insert(e.lane_start);
                wide.insert(e.lane_end);
            } else {
                through.insert(e.lane_start);
            }
        }
        let max_lane = wide
            .iter()
            .chain(through.iter())
            .copied()
            .max()
            .unwrap_or(0);
        let mut column = HashMap::new();
        let mut thin = HashSet::new();
        let mut col = 0;
        let mut in_thin_run = false;
        for lane in 0..=max_lane {
            if wide.contains(&lane) {
                column.insert(lane, col);
                col += 1;
                in_thin_run = false;
            } else if through.contains(&lane) {
                if !in_thin_run {
                    col += 1;
                    in_thin_run = true;
                }
                column.insert(lane, col - 1);
                thin.insert(lane);
            }
        }
        Self {
            edges,
            column,
            thin,
            width: col * 2,
        }
    }

    /// 1 行のセルの数。
    pub fn width(&self) -> usize {
        self.width
    }

    fn x(&self, lane: u16) -> usize {
        self.column.get(&lane).copied().unwrap_or(0) * 2
    }

    fn node_glyph(node: &Node) -> char {
        if node.flags & Node::HEAD != 0 {
            '◉'
        } else if node.flags & Node::MERGE != 0 {
            '◆'
        } else {
            '●'
        }
    }

    fn put_node(&self, line: &mut Line, node: &Node) {
        let x = self.x(node.lane);
        if x < line.glyph.len() {
            line.glyph[x] = Some(Self::node_glyph(node));
            line.color[x] = Some(node.color);
        }
    }

    fn mark_thin(&self, line: &mut Line, lane: u16) {
        if self.thin.contains(&lane) {
            let x = self.x(lane);
            if x < line.glyph.len() {
                line.glyph[x] = Some('┆');
            }
        }
    }

    /// `a` から `b` へ横に渡す線（`a` の側は `at_a`、`b` の側は `at_b` の向きを足す）。
    fn across(line: &mut Line, a: usize, b: usize, at_a: u8, at_b: u8, color: u16) {
        if b > a {
            line.add(a, at_a | RIGHT, color);
            (a + 1..b).for_each(|x| line.add(x, LEFT | RIGHT, color));
            line.add(b, LEFT | at_b, color);
        } else {
            line.add(a, at_a | LEFT, color);
            (b + 1..a).for_each(|x| line.add(x, LEFT | RIGHT, color));
            line.add(b, RIGHT | at_b, color);
        }
    }

    /// コミットの行（つなぎの行を挟む形）。
    pub fn commit_line(&self, row: u32, node: &Node) -> Line {
        let mut line = Line::new(self.width);
        for e in self.edges {
            if e.row_start <= row && row <= e.row_end {
                // 曲線は、端の行ではそのレーンの縦線として出す（向きを変えるのはつなぎの行）。
                let lane = if e.lane_start == e.lane_end || e.row_start == row {
                    e.lane_start
                } else {
                    e.lane_end
                };
                line.add(self.x(lane), UP | DOWN, e.color);
                self.mark_thin(&mut line, lane);
            }
        }
        self.put_node(&mut line, node);
        line
    }

    /// `row` と次の行の間（曲線がなければ None）。
    pub fn connector_line(&self, row: u32) -> Option<Line> {
        let mut line = Line::new(self.width);
        let mut curved = false;
        for e in self.edges {
            if e.lane_start != e.lane_end && e.row_start == row {
                curved = true;
                Self::across(
                    &mut line,
                    self.x(e.lane_start),
                    self.x(e.lane_end),
                    UP,
                    DOWN,
                    e.color,
                );
            } else if e.lane_start == e.lane_end && e.row_start <= row && row < e.row_end {
                line.add(self.x(e.lane_start), UP | DOWN, e.color);
                self.mark_thin(&mut line, e.lane_start);
            }
        }
        curved.then_some(line)
    }

    /// コミットの行（1 コミット 1 行の形）。曲線もこの行に描く: コミットの点から出る曲線（始まりの曲線）は
    /// その点の行に、親の点に入る曲線（終わりの曲線）は親の行に。`node` が None なら線だけ（まだ読んでいない行）。
    pub fn row_line(&self, row: u32, node: Option<&Node>) -> Line {
        let mut line = Line::new(self.width);
        for e in self.edges {
            if row < e.row_start || e.row_end < row {
                continue;
            }
            if e.lane_start == e.lane_end {
                let mut dirs = 0;
                if row > e.row_start {
                    dirs |= UP;
                }
                if row < e.row_end {
                    dirs |= DOWN;
                }
                line.add(self.x(e.lane_start), dirs, e.color);
                self.mark_thin(&mut line, e.lane_start);
                continue;
            }
            let (a, b) = (self.x(e.lane_start), self.x(e.lane_end));
            // 始まりの曲線は上の行（子の点の行）で曲げ、終わりの曲線は下の行（親の点の行）で曲げる。隣の行の
            // 親に直接つなぐ曲線は、first-parent なら親の行（ブランチが分かれた所へ戻る形）、マージの線なら
            // 子の行（マージから枝が出る形）で曲げる。
            let turns_at_start = e.child == e.row_start
                && (e.parent != e.row_end || e.kind & Edge::FIRST_PARENT == 0);
            match (row == e.row_start, turns_at_start) {
                (true, true) => Self::across(&mut line, a, b, 0, DOWN, e.color),
                (true, false) => line.add(a, DOWN, e.color),
                (false, true) => line.add(b, UP, e.color),
                (false, false) => Self::across(&mut line, a, b, UP, 0, e.color),
            }
        }
        if let Some(node) = node {
            self.put_node(&mut line, node);
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(row: u32, lane: u16, flags: u16) -> Node {
        Node {
            row,
            lane,
            flags,
            ..Node::default()
        }
    }

    fn edge(rows: (u32, u32), lanes: (u16, u16), child: u32, parent: u32) -> Edge {
        Edge {
            kind: if lanes.0 >= lanes.1 {
                Edge::FIRST_PARENT
            } else {
                0
            },
            row_start: rows.0,
            row_end: rows.1,
            lane_start: lanes.0,
            lane_end: lanes.1,
            child,
            parent,
            ..Edge::default()
        }
    }

    fn text(line: &Line) -> String {
        line.cells()
            .map(|c| c.0)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    /// 0 がマージ（親は 1 と 2）、1 は横のブランチ、2 が幹で 1 の親でもある。
    fn merge() -> (Vec<Node>, Vec<Edge>) {
        (
            vec![node(0, 0, Node::MERGE), node(1, 1, 0), node(2, 0, 0)],
            vec![
                edge((0, 2), (0, 0), 0, 2),
                edge((0, 1), (0, 1), 0, 1),
                edge((1, 2), (1, 0), 1, 2),
            ],
        )
    }

    /// 1 コミット 1 行: 始まりの曲線は子の行、終わりの曲線は親の行に描く
    #[test]
    fn one_line_per_commit_draws_the_curves_on_the_commit_rows() {
        let (nodes, edges) = merge();
        let g = Graph::new(&nodes, &edges);
        let lines: Vec<String> = nodes
            .iter()
            .map(|n| text(&g.row_line(n.row, Some(n))))
            .collect();
        assert_eq!(lines, ["◆─╮", "│ ●", "●─╯"]);
    }

    /// つなぎの行を挟む形は、曲線のある所だけ行が増える
    #[test]
    fn connector_lines_appear_only_where_a_curve_is() {
        let (nodes, edges) = merge();
        let g = Graph::new(&nodes, &edges);
        assert_eq!(text(&g.commit_line(0, &nodes[0])), "◆");
        assert_eq!(text(&g.connector_line(0).unwrap()), "├─╮");
        assert_eq!(text(&g.commit_line(1, &nodes[1])), "│ ●");
        assert_eq!(text(&g.connector_line(1).unwrap()), "├─╯");
        assert_eq!(text(&g.commit_line(2, &nodes[2])), "●");
        assert!(g.connector_line(2).is_none());
    }

    /// 通るだけのレーンは、隣り合うものを細い 1 列にまとめる
    #[test]
    fn lanes_that_only_pass_through_share_one_thin_column() {
        let nodes = vec![node(5, 0, 0)];
        let edges = vec![
            edge((0, 9), (1, 1), 0, 9),
            edge((0, 9), (2, 2), 0, 9),
            edge((0, 9), (3, 3), 0, 9),
        ];
        let g = Graph::new(&nodes, &edges);
        assert_eq!(g.width(), 4);
        assert_eq!(text(&g.row_line(5, Some(&nodes[0]))), "● ┆");
    }
}
