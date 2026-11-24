//! HTML のラベルの表（`<TABLE>`・`<TR>`・`<TD>`）: 読んで、升目に組み、SVG に描く。
//!
//! 属性は Graphviz の説明のとおり: 表の `border`（既定 1）・`cellborder`（既定は `border`）・`cellspacing`
//! （既定 2）・`cellpadding`（既定 2）・`bgcolor`・`color`・`width`・`height`・`fixedsize`・`sides`・`style`
//! （rounded）・`columns`・`rows`（`*` で間の線）・`port`・`align`・`valign`。升の `colspan`・`rowspan`・`border`・
//! `cellpadding`・`bgcolor`・`color`・`align`（`text` も）・`valign`・`balign`・`width`・`height`・`fixedsize`・
//! `sides`・`port`。`<HR/>`・`<VR/>` は行・升の間の線。中の文字は `text` の HTML（`<font>`・`<b>`・`<br/>` など）、
//! 升の中の表は入れ子にする。`<IMG>` は枠だけ（呼ぶ側が注記する）。

use crate::color::{Color, parse as parse_color};
use crate::text::{self, Font, Just, Piece, Text, attr, color_attr, inline_label, styled};
use amonhen_pele::svg::builder::num;

/// 切れ端の上限（壊れた・巨大な HTML でも時間を抑える）。
const MAX_PIECES: usize = 100_000;
/// 入れ子の深さの上限。
const MAX_DEPTH: usize = 20;
/// 升の数・行・列の上限。
const MAX_CELLS: usize = 20_000;
const MAX_SPAN: usize = 200;
/// 大きさの上限（点）。
const MAX_SIZE: f64 = 20_000.0;
/// 画像の代わりの枠の大きさ。
const IMAGE: (f64, f64) = (36.0, 24.0);
/// 升の文字の幅のゆとり。幅は Arial の送りで見積もるが、アプリの書体（macOS の sans-serif、Linux の DejaVu Sans・
/// Noto Sans など）は 1 割ほど広いことがあり、升の余白（`cellpadding`）が小さいと枠からはみ出る。ノードの
/// ラベルは周りの余白が吸うが、升は詰まっているので見積もりを広げておく。
const TEXT_SLACK: f64 = 1.1;
/// 既定の線の色（ノードの枠と同じ）。
const BORDER: &str = "var(--_b)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Middle,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Center,
    Left,
    Right,
    /// 行ごとの寄せ（`<br align>`）を升の幅いっぱいで。
    Text,
}

/// 線を引く辺（左・上・右・下）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sides([bool; 4]);

impl Sides {
    const ALL: Sides = Sides([true; 4]);

    fn parse(s: Option<&str>) -> Sides {
        let Some(s) = s else { return Sides::ALL };
        let s = s.to_ascii_lowercase();
        Sides([
            s.contains('l'),
            s.contains('t'),
            s.contains('r'),
            s.contains('b'),
        ])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Empty,
    Text(Text),
    Table(Box<Table>),
    Image,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// 表の左上からの位置と大きさ。
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    border: f64,
    padding: f64,
    color: Option<Color>,
    bg: Option<Color>,
    sides: Sides,
    align: Align,
    valign: VAlign,
    pub port: Option<String>,
    content: Content,
    // 組む前の値
    row: usize,
    col: usize,
    rowspan: usize,
    colspan: usize,
    min_w: f64,
    min_h: f64,
    fixed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub w: f64,
    pub h: f64,
    border: f64,
    color: Option<Color>,
    bg: Option<Color>,
    sides: Sides,
    rounded: bool,
    pub port: Option<String>,
    pub cells: Vec<Cell>,
    /// 間の線（表の左上から、両端）。
    rules: Vec<((f64, f64), (f64, f64))>,
    /// 升の中で表を置く寄せ。
    align: Align,
    valign: VAlign,
    min_w: f64,
    min_h: f64,
    fixed: bool,
}

/// 読んだときに見つけたもの（呼ぶ側の注記のため）。
#[derive(Default)]
struct Found {
    image: bool,
}

fn number(attrs: &[(String, String)], key: &str) -> Option<f64> {
    attr(attrs, key)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

fn align_of(attrs: &[(String, String)]) -> Align {
    match attr(attrs, "align").map(|s| s.trim().to_ascii_lowercase()) {
        Some(s) if s == "left" => Align::Left,
        Some(s) if s == "right" => Align::Right,
        Some(s) if s == "text" => Align::Text,
        _ => Align::Center,
    }
}

fn valign_of(attrs: &[(String, String)]) -> VAlign {
    match attr(attrs, "valign").map(|s| s.trim().to_ascii_lowercase()) {
        Some(s) if s == "top" => VAlign::Top,
        Some(s) if s == "bottom" => VAlign::Bottom,
        _ => VAlign::Middle,
    }
}

fn flag(attrs: &[(String, String)], key: &str) -> bool {
    attr(attrs, key).is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}

fn color(attrs: &[(String, String)], key: &str, scheme: Option<&str>) -> Option<Color> {
    // 色の並び（グラデーション）は最初の色
    let v = attr(attrs, key)?;
    let first = v.split(':').next().unwrap_or("");
    parse_color(first.split(';').next().unwrap_or(""), scheme)
}

struct Parser<'a> {
    src: &'a str,
    pieces: Vec<(Piece, usize, usize)>,
    i: usize,
    scheme: Option<&'a str>,
    found: Found,
    cells: usize,
}

/// HTML のラベルに表があれば、組んだ表のラベル。なければ None。
pub fn label(src: &str, base: &Font, scheme: Option<&str>) -> Option<Text> {
    let pieces = text::pieces(src, MAX_PIECES);
    let at = pieces
        .iter()
        .position(|(p, _, _)| matches!(p, Piece::Open(n, _) if n == "table"))?;
    // 表の前の飾り（`<font>` などで表を包む）
    let mut fonts = vec![base.clone()];
    let mut open: Vec<bool> = Vec::new();
    for (p, _, _) in &pieces[..at] {
        match p {
            Piece::Open(n, a) => {
                let cur = fonts.last().cloned().unwrap_or_else(|| base.clone());
                match styled(&cur, n, a, scheme) {
                    Some(f) => {
                        fonts.push(f);
                        open.push(true);
                    }
                    None => open.push(false),
                }
            }
            Piece::Close(_) if open.pop() == Some(true) && fonts.len() > 1 => {
                fonts.pop();
            }
            _ => {}
        }
    }
    let font = fonts.last().cloned().unwrap_or_else(|| base.clone());
    let mut p = Parser {
        src,
        pieces,
        i: at,
        scheme,
        found: Found::default(),
        cells: 0,
    };
    let mut table = p.table(&font, 0);
    table.place();
    Some(Text {
        lines: vec![],
        width: table.w,
        height: table.h,
        table: false,
        image: p.found.image,
        html: Some(Box::new(table)),
    })
}

impl Parser<'_> {
    /// `<table>` の位置から、対応する `</table>` の後まで。
    fn table(&mut self, font: &Font, depth: usize) -> Table {
        let attrs = match &self.pieces[self.i].0 {
            Piece::Open(_, a) => a.clone(),
            _ => vec![],
        };
        self.i += 1;
        let border = number(&attrs, "border").unwrap_or(1.0).clamp(0.0, 127.0);
        let cellborder = number(&attrs, "cellborder")
            .unwrap_or(border)
            .clamp(0.0, 127.0);
        let spacing = number(&attrs, "cellspacing")
            .unwrap_or(2.0)
            .clamp(-127.0, 127.0);
        let padding = number(&attrs, "cellpadding")
            .unwrap_or(2.0)
            .clamp(0.0, 255.0);
        let tcolor = color(&attrs, "color", self.scheme);
        let all_cols = attr(&attrs, "columns").is_some_and(|v| v.contains('*'));
        let all_rows = attr(&attrs, "rows").is_some_and(|v| v.contains('*'));
        let mut cells: Vec<Cell> = Vec::new();
        // 行の前の HR、升の前の VR
        let mut hr_before: Vec<usize> = Vec::new();
        let mut vr_after: Vec<usize> = Vec::new();
        let mut row = 0usize;
        let mut taken: Vec<Vec<bool>> = Vec::new();
        let mut pending_hr = false;
        while self.i < self.pieces.len() {
            match &self.pieces[self.i].0 {
                Piece::Close(n) if n == "table" => {
                    self.i += 1;
                    break;
                }
                Piece::Empty(n, _) if n == "hr" => {
                    pending_hr = true;
                    self.i += 1;
                }
                Piece::Open(n, _) if n == "tr" => {
                    self.i += 1;
                    if pending_hr && row > 0 {
                        hr_before.push(row);
                    }
                    pending_hr = false;
                    let mut col = 0usize;
                    while self.i < self.pieces.len() {
                        match &self.pieces[self.i].0 {
                            Piece::Close(n) if n == "tr" => {
                                self.i += 1;
                                break;
                            }
                            Piece::Open(n, _) if n == "tr" => break,
                            Piece::Close(n) if n == "table" => break,
                            Piece::Empty(n, _) if n == "vr" => {
                                vr_after.push(cells.len());
                                self.i += 1;
                            }
                            Piece::Open(n, a) if n == "td" => {
                                let a = a.clone();
                                self.i += 1;
                                let content = self.cell_content(font, depth);
                                if self.cells >= MAX_CELLS {
                                    continue;
                                }
                                self.cells += 1;
                                let colspan = number(&a, "colspan")
                                    .unwrap_or(1.0)
                                    .clamp(1.0, MAX_SPAN as f64)
                                    as usize;
                                let rowspan = number(&a, "rowspan")
                                    .unwrap_or(1.0)
                                    .clamp(1.0, MAX_SPAN as f64)
                                    as usize;
                                // 上の行から伸びた升を飛ばす
                                while taken
                                    .get(row)
                                    .is_some_and(|r| r.get(col).copied().unwrap_or(false))
                                {
                                    col += 1;
                                }
                                for r in row..row + rowspan {
                                    if taken.len() <= r {
                                        taken.resize(r + 1, Vec::new());
                                    }
                                    if taken[r].len() < col + colspan {
                                        taken[r].resize(col + colspan, false);
                                    }
                                    taken[r][col..col + colspan].fill(true);
                                }
                                let content = match content {
                                    Content::Text(mut t) => {
                                        // balign: 寄せのない行の寄せ
                                        let balign = match attr(&a, "balign")
                                            .map(|s| s.trim().to_ascii_lowercase())
                                        {
                                            Some(s) if s == "left" => Some(Just::Left),
                                            Some(s) if s == "right" => Some(Just::Right),
                                            _ => None,
                                        };
                                        if let Some(j) = balign {
                                            for l in &mut t.lines {
                                                if l.just == Just::Center {
                                                    l.just = j;
                                                }
                                            }
                                        }
                                        Content::Text(t)
                                    }
                                    c => c,
                                };
                                cells.push(Cell {
                                    x: 0.0,
                                    y: 0.0,
                                    w: 0.0,
                                    h: 0.0,
                                    border: number(&a, "border")
                                        .unwrap_or(cellborder)
                                        .clamp(0.0, 127.0),
                                    padding: number(&a, "cellpadding")
                                        .unwrap_or(padding)
                                        .clamp(0.0, 255.0),
                                    color: color(&a, "color", self.scheme).or(tcolor),
                                    bg: color(&a, "bgcolor", self.scheme),
                                    sides: Sides::parse(attr(&a, "sides")),
                                    align: align_of(&a),
                                    valign: valign_of(&a),
                                    port: attr(&a, "port").map(str::to_owned),
                                    content,
                                    row,
                                    col,
                                    rowspan,
                                    colspan,
                                    min_w: number(&a, "width").unwrap_or(0.0).clamp(0.0, MAX_SIZE),
                                    min_h: number(&a, "height").unwrap_or(0.0).clamp(0.0, MAX_SIZE),
                                    fixed: flag(&a, "fixedsize"),
                                });
                                col += colspan;
                            }
                            _ => self.i += 1,
                        }
                    }
                    row += 1;
                }
                _ => self.i += 1,
            }
        }
        let mut table = Table {
            w: 0.0,
            h: 0.0,
            border,
            color: tcolor,
            bg: color(&attrs, "bgcolor", self.scheme),
            sides: Sides::parse(attr(&attrs, "sides")),
            rounded: attr(&attrs, "style")
                .is_some_and(|s| s.to_ascii_lowercase().contains("rounded")),
            port: attr(&attrs, "port").map(str::to_owned),
            cells,
            rules: vec![],
            align: align_of(&attrs),
            valign: valign_of(&attrs),
            min_w: number(&attrs, "width").unwrap_or(0.0).clamp(0.0, MAX_SIZE),
            min_h: number(&attrs, "height").unwrap_or(0.0).clamp(0.0, MAX_SIZE),
            fixed: flag(&attrs, "fixedsize"),
        };
        table.size(spacing, &hr_before, &vr_after, all_rows, all_cols);
        table
    }

    /// `<td>` の後から、対応する `</td>` の後まで。中身を返す。
    fn cell_content(&mut self, font: &Font, depth: usize) -> Content {
        let start = self.pieces.get(self.i).map_or(self.src.len(), |p| p.1);
        // 対応する閉じを探す（入れ子の表の中の td は数えない）
        let mut level = 0usize;
        let mut end = self.i;
        while end < self.pieces.len() {
            match &self.pieces[end].0 {
                Piece::Open(n, _) if n == "table" => level += 1,
                Piece::Close(n) if n == "table" => {
                    if level == 0 {
                        break;
                    }
                    level -= 1;
                }
                Piece::Close(n) if n == "td" && level == 0 => break,
                Piece::Open(n, _) if (n == "td" || n == "tr") && level == 0 => break,
                Piece::Close(n) if n == "tr" && level == 0 => break,
                _ => {}
            }
            end += 1;
        }
        let stop = self.pieces.get(end).map_or(self.src.len(), |p| p.1);
        let inner = self.i..end;
        let after = if self
            .pieces
            .get(end)
            .is_some_and(|p| matches!(&p.0, Piece::Close(n) if n == "td"))
        {
            end + 1
        } else {
            end
        };
        // 入れ子の表（前の飾りを書体に）
        let mut fonts = vec![font.clone()];
        let mut open: Vec<bool> = Vec::new();
        let mut nested = None;
        let mut image = false;
        for k in inner.clone() {
            match &self.pieces[k].0 {
                Piece::Open(n, _) if n == "table" => {
                    nested = Some(k);
                    break;
                }
                Piece::Open(n, a) => {
                    let cur = fonts.last().cloned().unwrap_or_else(|| font.clone());
                    match styled(&cur, n, a, self.scheme) {
                        Some(f) => {
                            fonts.push(f);
                            open.push(true);
                        }
                        None => open.push(false),
                    }
                }
                Piece::Close(_) => {
                    if open.pop() == Some(true) && fonts.len() > 1 {
                        fonts.pop();
                    }
                }
                Piece::Empty(n, _) if n == "img" => image = true,
                _ => {}
            }
        }
        let content = if let Some(k) = nested {
            if depth + 1 >= MAX_DEPTH {
                Content::Empty
            } else {
                let f = fonts.last().cloned().unwrap_or_else(|| font.clone());
                self.i = k;
                let t = self.table(&f, depth + 1);
                Content::Table(Box::new(t))
            }
        } else if image {
            self.found.image = true;
            Content::Image
        } else {
            let t = inline_label(&self.src[start..stop.max(start)], font, self.scheme);
            if t.is_empty() {
                Content::Empty
            } else {
                Content::Text(t)
            }
        };
        self.i = after.max(self.i);
        content
    }
}

impl Cell {
    /// 中身の大きさ。
    fn content_size(&self) -> (f64, f64) {
        match &self.content {
            Content::Empty => (0.0, 0.0),
            Content::Text(t) => (t.width * TEXT_SLACK, t.height),
            Content::Table(t) => (t.w, t.h),
            Content::Image => IMAGE,
        }
    }

    fn need(&self) -> (f64, f64) {
        let (cw, ch) = self.content_size();
        let extra = 2.0 * (self.padding + self.border);
        if self.fixed {
            (self.min_w, self.min_h)
        } else {
            ((cw + extra).max(self.min_w), (ch + extra).max(self.min_h))
        }
    }
}

/// 列（行）の大きさを、またがる升も収まるように決める。`items` は (始め, 数, 要る大きさ)。
fn sizes(count: usize, items: &[(usize, usize, f64)], spacing: f64) -> Vec<f64> {
    let mut size = vec![0f64; count];
    let mut order: Vec<&(usize, usize, f64)> = items.iter().collect();
    order.sort_by_key(|(_, span, _)| *span);
    for &&(start, span, need) in &order {
        let end = (start + span).min(count);
        if end <= start {
            continue;
        }
        let have: f64 = size[start..end].iter().sum::<f64>() + spacing * (end - start - 1) as f64;
        if need > have {
            let add = (need - have) / (end - start) as f64;
            for s in &mut size[start..end] {
                *s += add;
            }
        }
    }
    size
}

impl Table {
    fn size(
        &mut self,
        spacing: f64,
        hr_before: &[usize],
        vr_after: &[usize],
        all_rows: bool,
        all_cols: bool,
    ) {
        let rows = self
            .cells
            .iter()
            .map(|c| c.row + c.rowspan)
            .max()
            .unwrap_or(0);
        let cols = self
            .cells
            .iter()
            .map(|c| c.col + c.colspan)
            .max()
            .unwrap_or(0);
        let mut col_w = sizes(
            cols,
            &self
                .cells
                .iter()
                .map(|c| (c.col, c.colspan, c.need().0))
                .collect::<Vec<_>>(),
            spacing,
        );
        let mut row_h = sizes(
            rows,
            &self
                .cells
                .iter()
                .map(|c| (c.row, c.rowspan, c.need().1))
                .collect::<Vec<_>>(),
            spacing,
        );
        let natural =
            |v: &[f64]| v.iter().sum::<f64>() + spacing * (v.len() + 1) as f64 + 2.0 * self.border;
        // 表の幅・高さ（広げるときは列・行に等しく）
        for (v, min) in [(&mut col_w, self.min_w), (&mut row_h, self.min_h)] {
            let have = natural(v);
            if min > have && !v.is_empty() {
                let add = (min - have) / v.len() as f64;
                for s in v.iter_mut() {
                    *s += add;
                }
            }
        }
        let mut w = natural(&col_w);
        let mut h = natural(&row_h);
        if cols == 0 {
            w = 2.0 * self.border + spacing.max(0.0);
        }
        if rows == 0 {
            h = 2.0 * self.border + spacing.max(0.0);
        }
        if self.fixed {
            if self.min_w > 0.0 {
                w = self.min_w;
            }
            if self.min_h > 0.0 {
                h = self.min_h;
            }
        }
        self.w = w.clamp(0.0, MAX_SIZE);
        self.h = h.clamp(0.0, MAX_SIZE);
        let at = |v: &[f64], i: usize| -> f64 {
            self.border + spacing + v[..i.min(v.len())].iter().sum::<f64>() + spacing * i as f64
        };
        let col_x: Vec<f64> = (0..=cols).map(|i| at(&col_w, i)).collect();
        let row_y: Vec<f64> = (0..=rows).map(|i| at(&row_h, i)).collect();
        for c in &mut self.cells {
            let c1 = (c.col + c.colspan).min(cols);
            let r1 = (c.row + c.rowspan).min(rows);
            c.x = col_x[c.col];
            c.y = row_y[c.row];
            c.w = col_x[c1] - spacing - c.x;
            c.h = row_y[r1] - spacing - c.y;
        }
        // 間の線: 行・列の間の空きの中ほど
        let (x0, x1) = (self.border, self.w - self.border);
        let (y0, y1) = (self.border, self.h - self.border);
        let mut rules = Vec::new();
        let between = |pos: &[f64], i: usize| pos[i] - spacing / 2.0;
        let mut hr: Vec<usize> = hr_before.to_vec();
        if all_rows {
            hr.extend(1..rows);
        }
        hr.sort_unstable();
        hr.dedup();
        for r in hr.into_iter().filter(|&r| r > 0 && r < rows) {
            let y = between(&row_y, r);
            rules.push(((x0, y), (x1, y)));
        }
        let mut vr: Vec<(usize, usize, usize)> = Vec::new();
        for &k in vr_after {
            // k 番目の升の前（同じ行の前の升の右）
            if let Some(c) = self.cells.get(k)
                && c.col > 0
            {
                vr.push((c.col, c.row, c.row + c.rowspan));
            }
        }
        if all_cols {
            for col in 1..cols {
                vr.push((col, 0, rows));
            }
        }
        for (col, r0, r1) in vr {
            let x = between(&col_x, col);
            let (a, b) = if all_cols {
                (y0, y1)
            } else {
                (
                    row_y[r0] - spacing / 2.0,
                    row_y[r1.min(rows)] - spacing / 2.0,
                )
            };
            rules.push(((x, a.max(y0)), (x, b.min(y1))));
        }
        self.rules = rules;
    }

    /// 中の表の位置を決める（升の中の寄せ）。大きさは `size` で決めてある。
    fn place(&mut self) {
        for c in &mut self.cells {
            if let Content::Table(t) = &mut c.content {
                t.place();
            }
        }
    }

    /// ポートの箱（表の左上から）。
    pub fn port(&self, name: &str) -> Option<[f64; 4]> {
        if self.port.as_deref() == Some(name) {
            return Some([0.0, 0.0, self.w, self.h]);
        }
        for c in &self.cells {
            if c.port.as_deref() == Some(name) {
                return Some([c.x, c.y, c.x + c.w, c.y + c.h]);
            }
            if let Content::Table(t) = &c.content
                && let Some(b) = t.port(name)
            {
                let (ox, oy) = c.inner_origin(t);
                return Some([b[0] + ox, b[1] + oy, b[2] + ox, b[3] + oy]);
            }
        }
        None
    }
}

impl Cell {
    /// 升の中の表の左上（升の寄せ）。
    fn inner_origin(&self, t: &Table) -> (f64, f64) {
        let inset = self.border + self.padding;
        let (iw, ih) = (self.w - 2.0 * inset, self.h - 2.0 * inset);
        let x = match t.align.max_by(self.align) {
            Align::Left => self.x + inset,
            Align::Right => self.x + inset + iw - t.w,
            _ => self.x + inset + (iw - t.w) / 2.0,
        };
        let y = match t.valign.max_by(self.valign) {
            VAlign::Top => self.y + inset,
            VAlign::Bottom => self.y + inset + ih - t.h,
            VAlign::Middle => self.y + inset + (ih - t.h) / 2.0,
        };
        (x, y)
    }
}

trait Prefer: Sized + PartialEq + Copy {
    const DEFAULT: Self;
    /// 自分が既定でなければ自分、既定なら `other`。
    fn max_by(self, other: Self) -> Self {
        if self == Self::DEFAULT { other } else { self }
    }
}

impl Prefer for Align {
    const DEFAULT: Align = Align::Center;
}

impl Prefer for VAlign {
    const DEFAULT: VAlign = VAlign::Middle;
}

fn paint(name: &str, c: Option<Color>, fallback: &str) -> String {
    match c {
        Some(c) => color_attr(name, &c),
        None => format!(" {name}=\"{fallback}\""),
    }
}

/// 枠（線の太さ `b`、辺 `sides`）を描く。線は箱の内側に収める。
#[allow(clippy::too_many_arguments)]
fn frame(
    out: &mut String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    b: f64,
    sides: Sides,
    stroke: Option<Color>,
    fill: Option<Color>,
    rounded: bool,
) {
    if let Some(f) = fill {
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{} stroke=\"none\"/>",
            num(x),
            num(y),
            num(w.max(0.0)),
            num(h.max(0.0)),
            if rounded { " rx=\"6\"" } else { "" },
            color_attr("fill", &f)
        ));
    }
    if b <= 0.0 || w <= 0.0 || h <= 0.0 {
        return;
    }
    let stroke = paint("stroke", stroke, BORDER);
    let width = if (b - 1.0).abs() > 1e-9 {
        format!(" stroke-width=\"{}\"", num(b))
    } else {
        String::new()
    };
    let (x0, y0, x1, y1) = (x + b / 2.0, y + b / 2.0, x + w - b / 2.0, y + h - b / 2.0);
    if sides == Sides::ALL {
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{} fill=\"none\"{stroke}{width}/>",
            num(x0),
            num(y0),
            num((x1 - x0).max(0.0)),
            num((y1 - y0).max(0.0)),
            if rounded { " rx=\"6\"" } else { "" },
        ));
        return;
    }
    let mut d = String::new();
    let lines = [
        (sides.0[0], (x0, y0), (x0, y1)),
        (sides.0[1], (x0, y0), (x1, y0)),
        (sides.0[2], (x1, y0), (x1, y1)),
        (sides.0[3], (x0, y1), (x1, y1)),
    ];
    for (on, a, c) in lines {
        if on {
            d.push_str(&format!(
                "M{},{}L{},{}",
                num(a.0),
                num(a.1),
                num(c.0),
                num(c.1)
            ));
        }
    }
    if !d.is_empty() {
        out.push_str(&format!("<path d=\"{d}\" fill=\"none\"{stroke}{width}/>"));
    }
}

/// 表を (left, top) から描く。`root`・`attrs` は文字の `<text>` の書体と属性、`ink` は表の外の塗りの上の文字の
/// 色。表・升の `bgcolor` の上では、いちばん内側の塗りで読める色にする（文が色を決めていれば文の色）。
pub fn draw(
    t: &Table,
    left: f64,
    top: f64,
    root: &Font,
    attrs: &str,
    ink: Option<&str>,
    out: &mut String,
) {
    frame(
        out, left, top, t.w, t.h, t.border, t.sides, t.color, t.bg, t.rounded,
    );
    let ink = t.bg.and_then(Color::ink).or(ink);
    for c in &t.cells {
        let ink = c.bg.and_then(Color::ink).or(ink);
        let (x, y) = (left + c.x, top + c.y);
        frame(out, x, y, c.w, c.h, c.border, c.sides, c.color, c.bg, false);
        let inset = c.border + c.padding;
        let (iw, ih) = ((c.w - 2.0 * inset).max(0.0), (c.h - 2.0 * inset).max(0.0));
        let (cw, ch) = c.content_size();
        let cx = match c.align {
            Align::Left => x + inset + cw / 2.0,
            Align::Right => x + inset + iw - cw / 2.0,
            Align::Center | Align::Text => x + inset + iw / 2.0,
        };
        let cy = match c.valign {
            VAlign::Top => y + inset + ch / 2.0,
            VAlign::Bottom => y + inset + ih - ch / 2.0,
            VAlign::Middle => y + inset + ih / 2.0,
        };
        match &c.content {
            Content::Empty => {}
            Content::Text(text) => {
                let box_w = if c.align == Align::Text { iw } else { 0.0 };
                out.push_str(&text::text_svg(text, cx, cy, box_w, root, attrs, ink));
            }
            Content::Table(inner) => {
                let (ox, oy) = c.inner_origin(inner);
                draw(inner, left + ox, top + oy, root, attrs, ink, out);
            }
            Content::Image => {
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{BORDER}\" stroke-dasharray=\"3,2\"/>",
                    num(cx - IMAGE.0 / 2.0),
                    num(cy - IMAGE.1 / 2.0),
                    num(IMAGE.0),
                    num(IMAGE.1)
                ));
            }
        }
    }
    let stroke = paint("stroke", t.color, BORDER);
    for (a, b) in &t.rules {
        out.push_str(&format!(
            "<path d=\"M{},{}L{},{}\" fill=\"none\"{stroke}/>",
            num(left + a.0),
            num(top + a.1),
            num(left + b.0),
            num(top + b.1)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(src: &str) -> Table {
        let t = label(src, &Font::new(14.0), None).expect("a table");
        *t.html.unwrap()
    }

    #[test]
    fn lays_out_a_grid_with_spans() {
        let t = table(
            "<TABLE BORDER=\"0\" CELLSPACING=\"0\" CELLPADDING=\"4\"><TR><TD COLSPAN=\"2\">wide header</TD></TR><TR><TD PORT=\"a\">a</TD><TD ROWSPAN=\"2\">b</TD></TR><TR><TD>c</TD></TR></TABLE>",
        );
        assert_eq!(t.cells.len(), 4);
        let (h, a, b, c) = (&t.cells[0], &t.cells[1], &t.cells[2], &t.cells[3]);
        assert_eq!((a.row, a.col, b.col, c.row, c.col), (1, 0, 1, 2, 0));
        // 見出しは 2 列ぶん、b は 2 行ぶん
        assert!((h.w - (a.w + b.w)).abs() < 1e-6, "{} {} {}", h.w, a.w, b.w);
        assert!((b.h - (a.h + c.h)).abs() < 1e-6);
        assert!((t.w - h.w).abs() < 1e-6 && (t.h - (h.h + b.h)).abs() < 1e-6);
        assert_eq!(t.port("a"), Some([a.x, a.y, a.x + a.w, a.y + a.h]));
        assert_eq!(t.port("nope"), None);
    }

    #[test]
    fn uses_defaults_and_sizes() {
        let t = table(
            "<table><tr><td width=\"80\" height=\"30\" fixedsize=\"true\">long text here</td></tr></table>",
        );
        let c = &t.cells[0];
        assert_eq!((c.w, c.h), (80.0, 30.0));
        // 枠 1・間 2: 1 + 2 + 80 + 2 + 1
        assert_eq!((t.w, t.h), (86.0, 36.0));
        let t = table("<table width=\"200\"><tr><td>a</td><td>b</td></tr></table>");
        assert_eq!(t.w, 200.0);
        assert!((t.cells[0].w - t.cells[1].w).abs() < 1e-6);
        // 升は文字の見積もりより 1 割広く（アプリの書体が Arial より広くても余白の内に収まる）
        let t = table("<table cellpadding=\"2\"><tr><td>user_id</td></tr></table>");
        let Content::Text(text) = &t.cells[0].content else {
            panic!("no text");
        };
        let inner = t.cells[0].w - 2.0 * (2.0 + 1.0);
        assert!(
            (inner - text.width * TEXT_SLACK).abs() < 1e-6,
            "{inner} {}",
            text.width
        );
    }

    #[test]
    fn nests_tables_and_fonts() {
        let t = table(
            "<font point-size=\"20\"><table><tr><td><table port=\"in\"><tr><td><b>x</b></td></tr></table></td><td><img src=\"a.png\"/></td></tr><hr/><tr><td>y</td><vr/><td>z</td></tr></table></font>",
        );
        let Content::Table(inner) = &t.cells[0].content else {
            panic!("no nested table");
        };
        let Content::Text(x) = &inner.cells[0].content else {
            panic!("no text");
        };
        assert!(x.lines[0].spans[0].font.bold && x.lines[0].spans[0].font.size == 20.0);
        assert_eq!(t.cells[1].content, Content::Image);
        assert_eq!(t.rules.len(), 2);
        assert!(t.port("in").is_some());
        let lbl = label(
            "<table><tr><td><img src=\"a\"/></td></tr></table>",
            &Font::new(14.0),
            None,
        )
        .unwrap();
        assert!(lbl.image && !lbl.table);
        let mut out = String::new();
        draw(&t, 0.0, 0.0, &Font::new(14.0), "", None, &mut out);
        assert!(
            out.contains("font-weight=\"bold\"") || out.contains(">x</text>"),
            "{out}"
        );
    }

    #[test]
    fn survives_broken_tables() {
        for src in [
            "<table>",
            "<table><tr><td>",
            "<table><td>a</td></table>",
            "<table><tr><td colspan=\"99999\">a</td></tr></table>",
            "<table><tr><td><table><tr><td></table></td></tr></table>",
            "a <table></table> b",
            "<table border=\"-5\" cellspacing=\"-500\"><tr><td>a</td></tr></table>",
        ] {
            let t = label(src, &Font::new(14.0), None).unwrap();
            assert!(t.width.is_finite() && t.height.is_finite(), "{src}");
        }
        let deep: String = "<table><tr><td>".repeat(60) + "x";
        let _ = label(&deep, &Font::new(14.0), None);
        assert!(label("<b>no table</b>", &Font::new(14.0), None).is_none());
    }
}
