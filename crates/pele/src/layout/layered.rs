//! 1 つの平らなグラフの層状（Sugiyama 風）の配置。pele の `src/layout/layered.ts` を移したもの。ランクは y の
//! 下向きに並び、ほかの向きは呼ぶ側が回す。
//!
//! 辺の配置が目指すこと（上ほど大事。ぶつかるときは上が決める。括弧はそれを行う所）:
//!
//! 1. 線が見分けられる。触れ合わず、沿って走らず、ノードやラベルを通らない。ノードの 1 辺の端はそれぞれの
//!    場所を持ち（`spread`）、数画素の内を通る曲線はそれぞれの段で曲がる（`assign_tracks`）。
//! 2. ノードの順が許すかぎり交差を少なく（`reduce_crossings`、`assign_tracks` の段の順）。
//! 3. 辺は短く単純に。まっすぐ、だめなら 1 つの曲線、だめなら段を走る曲線。いくつものランクをまたぐ辺は片方の
//!    端と一直線に走り、もう一方で 1 度曲がる（`align_labels`、`straighten`）。端は辺に沿って滑り、ノードも少し
//!    動いて辺をまっすぐにする（`settle`、`snap`）。
//! 4. 辺は辺の真ん中に着く。1 辺を独り占めする端は真ん中に、分け合う端は行き先の順に真ん中のまわりに。上の
//!    目的のためにだけ真ん中を離れる（`settle`）。
//! 5. 釣り合いよく、似たものは似せて描く。ラベルは見える線の真ん中、両側の曲線は同じほど曲がる余地を持ち、
//!    曲線の多くが交差する隙間ではどれも段なしの同じ曲線にする（`layered` の `head_room`、`assign_tracks`）。
//! 6. 辺はノードに直角に出入りし、矢じりはノードをまっすぐ指す（svg/edges の `route_path`）。
//! 7. 上のどれも言わないところでは、ノードは書かれた順を保つ。
//! 8. 小さくまとめる。ランクの隙間は段を収めるか、交差する曲線をはっきりした角度で交わらせるためにだけ広がる。
//!
//! JS の型付き配列は、Int32Array を `i32`（添字に使うものは `usize`）、Float64Array を `f64`、Uint8Array を `u8`
//! で写す。比べる関数での並べ替えは、V8 と同じく安定な並べ替え（`sort_by`）で行う。

#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::type_complexity
)]

use std::collections::{HashMap, HashSet};

use crate::js;

/// ノードの種類。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Node = 0,
    Dummy = 1,
    Label = 2,
    StartPort = 3,
    EndPort = 4,
}

const K_NODE: u8 = Kind::Node as u8;
const K_DUMMY: u8 = Kind::Dummy as u8;
const K_LABEL: u8 = Kind::Label as u8;
const K_START: u8 = Kind::StartPort as u8;
const K_END: u8 = Kind::EndPort as u8;

#[derive(Clone, Debug)]
pub struct LNode {
    pub w: f64,
    pub h: f64,
    pub kind: Kind,
    pub pin: f64,
    /// ポートの流れを横切る向きの行き先。分からなければ NaN。ポートのランクがすべて持っていれば、その順を保つ。
    pub hint: f64,
    /// 流れを横切る向きの初めの位置。分からなければ NaN。ランクのノードがすべて持つときだけ使う。
    pub key: f64,
    /// 辺が着く辺のうち、辺どうしで分け合ってよい長さ。0 ならすべて真ん中。
    pub span: f64,
    pub x: f64,
    pub y: f64,
    pub rank: i32,
}

#[derive(Clone, Debug)]
pub struct LEdge {
    pub tail: usize,
    pub head: usize,
    pub minlen: f64,
    pub label_w: f64,
    pub label_h: f64,
    pub tail_dx: f64,
    pub head_dx: f64,
    /// ずらしの位置に留める端。ノードの辺を分け合うのに加わらない。
    pub tail_fixed: bool,
    pub head_fixed: bool,
    pub reversed: bool,
    pub points: Vec<f64>,
    pub label_x: f64,
    pub label_y: f64,
}

/// グループ: `of` はノードからグループ、`parent` はグループから親。どちらもないときは -1。
#[derive(Clone, Debug)]
pub struct Groups {
    pub of: Vec<i32>,
    pub parent: Vec<i32>,
}

/// TS の省いてよい項目は、`bool` は false、`head_room` は 0、`groups` は `None` が「なし」。
#[derive(Clone, Debug, Default)]
pub struct LayeredOptions {
    pub node_sep: f64,
    pub edge_sep: f64,
    pub rank_sep: f64,
    pub port_sep: f64,
    /// 辺を直角に、2 つのランクの隙間をそれぞれの道（track）で横切らせる。
    pub tracks: bool,
    /// どの辺も点から点へまっすぐ。曲がる段を与えない。
    pub straight: bool,
    /// ノードの 1 辺の端どうしを呼ぶ側が離すので、同じ 2 ノード間の 2 本の辺に曲がりは要らない。
    pub apart: bool,
    /// 辺の頭の矢じりが取る長さ。辺がみな同じ向きのラベルのランクは、その半分だけ尾の側に寄る。
    pub head_room: f64,
    /// ランクの中でグループをひとまとまりに保つ。
    pub groups: Option<Groups>,
    /// 道筋を引かずに、流れを横切る位置だけを `x` に出す。
    pub order_only: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LayeredResult {
    pub width: f64,
    pub height: f64,
    /// `order_only` のとき: 最後の交差の数と、まとめ・並べ替えの前の交差の数。
    pub crossings: Option<f64>,
    pub declared: Option<f64>,
}

pub fn lnode(w: f64, h: f64, kind: Kind) -> LNode {
    LNode {
        w,
        h,
        kind,
        pin: 0.0,
        hint: f64::NAN,
        key: f64::NAN,
        span: 0.0,
        x: 0.0,
        y: 0.0,
        rank: 0,
    }
}

pub fn ledge(tail: usize, head: usize, minlen: f64, label_w: f64, label_h: f64) -> LEdge {
    LEdge {
        tail,
        head,
        minlen,
        label_w,
        label_h,
        tail_dx: 0.0,
        head_dx: 0.0,
        tail_fixed: false,
        head_fixed: false,
        reversed: false,
        points: Vec::new(),
        label_x: 0.0,
        label_y: 0.0,
    }
}

const MAX_SWEEPS: usize = 24;

// 辺は飛ばすランクごとに曲がり点を持つ。これを超えたら長い辺から 1 本の曲線で描く（巨大なグラフで時間と
// メモリを使い切らないように）。
const MAX_DUMMIES: i64 = 20000;

// ノードの 1 辺に着く 2 本の辺の間の幅と、矢じりを離しておける最小。
const PORT_PITCH: f64 = 16.0;
const MIN_PITCH: f64 = 8.0;

// 段を与えないところでは、ランクの隙間は横切る辺を少なくともこの傾きに保つまで、ふつうの高さのこの倍まで広がる。
const MIN_SLOPE: f64 = 0.125;
const MAX_GAP: f64 = 3.0;

// 道: ランクから最も近い道までの余白、隙間が広がるときの道どうしの幅、余裕があるときに広がる最大。
const TRACK_MARGIN: f64 = 18.0;
const TRACK_PITCH: f64 = 10.0;
const TRACK_SPREAD: f64 = 14.0;
// 曲線は、隙間の高さのこの倍より遠く横切り、沿って走るほかの曲線もそうなら、それぞれの段で横切る。段は
// ランクからこれだけ、互いにこれだけ離れる。
const LEVEL_WIDE: f64 = 2.0;
const LEVEL_MARGIN: f64 = 28.0;
const LEVEL_PITCH: f64 = 12.0;
// 近すぎる 2 本の曲線もそれぞれの段で曲がる。片方が遠く横切るなら LEVEL_CLEAR、そうでなければ LEVEL_TOUCH。
// LEVEL_LEAD は曲線が隙間の両端でまっすぐ保つとみなす長さ。
const LEVEL_LEAD: f64 = 12.0;
const LEVEL_CLEAR: f64 = 8.0;
const LEVEL_TOUCH: f64 = 4.0;
// 隙間の曲線が沿うより多く交差するなら、段は交差を長く浅くするだけなので、どれもふつうの S にし、隙間を
// 最も広く横切る曲線のこの倍の高さにする（MAX_GAP まで）。
const CROSSED_SLOPE: f64 = 0.5;
// この内のずれなら一直線とみなす。
const IN_LINE: f64 = 0.5;
// 両端でこの内なら双子の辺とみなす。
const TWIN: f64 = 1.0;
// ノードは一直線に並ぶものと一緒に、辺をもう 1 本まっすぐにするためにこれだけまで動く。
const SNAP: f64 = 12.0;
// 一直線のノードのない曲がり点はこれだけまで動く。
const BEND_SNAP: f64 = 24.0;
// 一直線にできない端は辺の行き先へ傾くが、辺を広げてよい部分の端からこれだけ離れる。
const LEAN_CLEAR: f64 = 40.0;
// 1 つの隙間を横切る辺がこれより多ければ、道は単純な順に与える（すべての組を比べると時間がかかる）。
const MAX_WEIGHED: usize = 400;
const SAMPLES: usize = 16;

const HELD: u8 = 0;
const LEAVES: u8 = 1;
const ARRIVES: u8 = 2;

fn is_dummy(kind: u8) -> bool {
    kind == K_DUMMY || kind == K_LABEL
}

/// `a || b`（数）。
fn or(a: f64, b: f64) -> f64 {
    if js::truthy(a) { a } else { b }
}

/// 比べる関数（負なら前）による安定な並べ替え（`Array.prototype.sort` と同じ結果）。
pub fn sort_by<T: Copy>(list: &mut [T], mut cmp: impl FnMut(T, T) -> f64) {
    let n = list.len();
    if n < 2 {
        return;
    }
    let mut buf: Vec<T> = list.to_vec();
    merge_sort(list, &mut buf, &mut cmp);
}

fn merge_sort<T: Copy>(list: &mut [T], buf: &mut [T], cmp: &mut impl FnMut(T, T) -> f64) {
    let n = list.len();
    if n <= 10 {
        for i in 1..n {
            let item = list[i];
            let mut j = i;
            while j > 0 && cmp(item, list[j - 1]) < 0.0 {
                list[j] = list[j - 1];
                j -= 1;
            }
            list[j] = item;
        }
        return;
    }
    let mid = n / 2;
    {
        let (a, b) = list.split_at_mut(mid);
        let (ba, bb) = buf.split_at_mut(mid);
        merge_sort(a, ba, cmp);
        merge_sort(b, bb, cmp);
    }
    buf[..n].copy_from_slice(list);
    let (mut i, mut j, mut k) = (0, mid, 0);
    while i < mid && j < n {
        if cmp(buf[j], buf[i]) < 0.0 {
            list[k] = buf[j];
            j += 1;
        } else {
            list[k] = buf[i];
            i += 1;
        }
        k += 1;
    }
    while i < mid {
        list[k] = buf[i];
        i += 1;
        k += 1;
    }
    while j < n {
        list[k] = buf[j];
        j += 1;
        k += 1;
    }
}

fn sort_range(list: &mut [usize], from: usize, to: usize, key: &[i32]) {
    for i in (from + 1)..to {
        let item = list[i];
        let k = key[item];
        let mut j = i as i64 - 1;
        while j >= from as i64
            && (key[list[j as usize]] > k
                || (key[list[j as usize]] == k && list[j as usize] > item))
        {
            list[(j + 1) as usize] = list[j as usize];
            j -= 1;
        }
        list[(j + 1) as usize] = item;
    }
}

// 組み込みの sort の比べる関数の重さを避ける。
fn sort_by_bary(
    list: &mut [usize],
    count: usize,
    bary: &[f64],
    pos: &[i32],
    right: bool,
    spare: &mut [usize],
) {
    let before = |a: usize, b: usize| -> bool {
        bary[a] < bary[b]
            || (bary[a] == bary[b]
                && (if right {
                    pos[a] > pos[b]
                } else {
                    pos[a] < pos[b]
                }))
    };
    const RUN: usize = 8;
    let mut s = 0;
    while s < count {
        let end = (s + RUN).min(count);
        for i in (s + 1)..end {
            let item = list[i];
            let mut j = i as i64 - 1;
            while j >= s as i64 && before(item, list[j as usize]) {
                list[(j + 1) as usize] = list[j as usize];
                j -= 1;
            }
            list[(j + 1) as usize] = item;
        }
        s += RUN;
    }
    // from と to を入れ替えながら併合する。`in_list` は今の結果が list にあるか。
    let mut in_list = true;
    let mut width = RUN;
    while width < count {
        {
            let (from, to): (&[usize], &mut [usize]) = if in_list {
                (&*list, &mut *spare)
            } else {
                (&*spare, &mut *list)
            };
            // 借用の都合で、写しを作ってから併合する。
            let src: Vec<usize> = from[..count].to_vec();
            let mut s = 0;
            while s < count {
                let mid = (s + width).min(count);
                let end = (s + 2 * width).min(count);
                let (mut i, mut j, mut k) = (s, mid, s);
                while i < mid && j < end {
                    if before(src[j], src[i]) {
                        to[k] = src[j];
                        j += 1;
                    } else {
                        to[k] = src[i];
                        i += 1;
                    }
                    k += 1;
                }
                while i < mid {
                    to[k] = src[i];
                    i += 1;
                    k += 1;
                }
                while j < end {
                    to[k] = src[j];
                    j += 1;
                    k += 1;
                }
                s += 2 * width;
            }
        }
        in_list = !in_list;
        width *= 2;
    }
    if !in_list {
        list[..count].copy_from_slice(&spare[..count]);
    }
}

// 1 つの隙間を横切る辺の切れ端。上のランクを出る所と下のランクに着く所、上のランク、終わる曲がり点（辺の
// 最後のノードなら -1）、横切る道（なければ -1）。一部だけに段を与えるときは、`wide` が遠く横切るものの印、
// `levels` はその切れ端の組が取る道の数（要らなければ 0）で、道は組の最初から数える。`wide` でない切れ端は
// 自分の段の 1 つ上から曲がる。
struct Pieces {
    from: Vec<f64>,
    to: Vec<f64>,
    rank: Vec<usize>,
    lower: Vec<i64>,
    track: Vec<i32>,
    levels: Vec<i32>,
    wide: Vec<u8>,
}

struct Graph {
    n: usize,
    real: usize,
    max_rank: usize,
    w: Vec<f64>,
    kind: Vec<u8>,
    rank: Vec<i32>,
    // 切れ端の両端と、それぞれの端で着く所。
    ef: Vec<usize>,
    et: Vec<usize>,
    efd: Vec<f64>,
    etd: Vec<f64>,
    succ_start: Vec<usize>,
    succ_e: Vec<usize>,
    pred_start: Vec<usize>,
    pred_e: Vec<usize>,
    layer_start: Vec<usize>,
    order: Vec<usize>,
    pos: Vec<i32>,
    offsets: bool,
    // 順を変えないランク。
    fixed: Vec<u8>,
    grp: Option<Vec<i32>>,
    group_parent: Option<Vec<i32>>,
}

fn sort_rank(
    order: &mut [usize],
    layer_start: &[usize],
    r: usize,
    key: &dyn Fn(usize) -> f64,
) -> bool {
    let s = layer_start[r];
    let e = layer_start[r + 1];
    if e < s + 2 {
        return false;
    }
    for i in s..e {
        if key(order[i]).is_nan() {
            return false;
        }
    }
    let mut layer: Vec<(usize, usize)> = order[s..e]
        .iter()
        .enumerate()
        .map(|(i, &v)| (v, i))
        .collect();
    sort_by(&mut layer, |a, b| {
        or(key(a.0) - key(b.0), a.1 as f64 - b.1 as f64)
    });
    for (i, (v, _)) in layer.into_iter().enumerate() {
        order[s + i] = v;
    }
    true
}

pub fn layered(nodes: &mut [LNode], edges: &mut [LEdge], opt: &LayeredOptions) -> LayeredResult {
    let real = nodes.len();
    if real == 0 {
        return LayeredResult::default();
    }
    let m = edges.len();
    if real == 1 && m == 0 && nodes[0].kind == Kind::Node {
        let node = &mut nodes[0];
        node.rank = 0;
        node.x = node.w / 2.0;
        node.y = node.h / 2.0;
        return LayeredResult {
            width: node.w,
            height: node.h,
            crossings: None,
            declared: None,
        };
    }

    // ラベルや並んだ辺が場所を要るときは、どの辺もまんなかのランクを持つ。
    let mut step: i32 = 1;
    if m > 0 {
        let mut seen = HashSet::new();
        for e in edges.iter() {
            let a = e.tail.min(e.head);
            let b = e.tail.max(e.head);
            let key = a * real + b;
            if e.label_w > 0.0 || seen.contains(&key) {
                step = 2;
                break;
            }
            seen.insert(key);
        }
    }

    orient(nodes, edges);
    rank(nodes, edges, step);

    let mut span = vec![0i32; m];
    let mut dummies: i64 = 0;
    for ei in 0..m {
        let e = &edges[ei];
        span[ei] = (nodes[e.head].rank - nodes[e.tail].rank).abs();
        if span[ei] > 1 {
            dummies += (span[ei] - 1) as i64;
        }
    }
    let mut direct = vec![0u8; m];
    if dummies > MAX_DUMMIES {
        let mut longest: Vec<usize> = Vec::new();
        for ei in 0..m {
            if span[ei] > 1 {
                longest.push(ei);
            }
        }
        sort_by(&mut longest, |a, b| {
            or((span[b] - span[a]) as f64, a as f64 - b as f64)
        });
        for ei in longest {
            if dummies <= MAX_DUMMIES {
                break;
            }
            direct[ei] = 1;
            dummies -= (span[ei] - 1) as i64;
        }
    }

    let n = real + dummies as usize;
    let mut w = vec![0f64; n];
    let mut h = vec![0f64; n];
    let mut kind = vec![0u8; n];
    let mut rank_of = vec![0i32; n];
    let mut max_rank: usize = 0;
    for i in 0..real {
        let node = &nodes[i];
        w[i] = node.w;
        h[i] = node.h;
        kind[i] = node.kind as u8;
        rank_of[i] = node.rank;
        if node.rank as i64 > max_rank as i64 {
            max_rank = node.rank as usize;
        }
    }

    // 並べて位置を決めるグラフ: どの辺もランクからランクへの切れ端に分ける。
    let mut segments = 0usize;
    for ei in 0..m {
        if direct[ei] == 0 {
            segments += span[ei] as usize;
        }
    }
    let mut ef = vec![0usize; segments];
    let mut et = vec![0usize; segments];
    let mut efd = vec![0f64; segments];
    let mut etd = vec![0f64; segments];
    let mut first_dummy = vec![-1i64; m];
    // 各ランクでラベルのある辺の頭の向き: 1 下、2 上、3 両方。
    let mut heads = vec![0u8; max_rank + 1];
    let mut label_node = vec![-1i64; m];
    let mut spans = false;
    for i in 0..real {
        if nodes[i].span > 0.0 && nodes[i].kind == Kind::Node {
            spans = true;
        }
    }
    // 切れ端の端がノードにとって何か（ノードの辺を動いてよい端について）。
    let mut efk: Option<Vec<u8>> = if spans {
        Some(vec![0u8; segments])
    } else {
        None
    };
    let mut etk: Option<Vec<u8>> = if spans {
        Some(vec![0u8; segments])
    } else {
        None
    };
    let mut first_seg = vec![-1i64; m];
    let mut last_seg = vec![-1i64; m];
    let mut key = vec![f64::NAN; n];
    for i in 0..real {
        key[i] = nodes[i].key;
    }
    let mut offsets = false;
    {
        let mut next = real;
        let mut ce = 0usize;
        for ei in 0..m {
            if direct[ei] != 0 {
                continue;
            }
            let e = &edges[ei];
            let a = if e.reversed { e.head } else { e.tail };
            let b = if e.reversed { e.tail } else { e.head };
            let a_dx = if e.reversed { e.head_dx } else { e.tail_dx };
            let b_dx = if e.reversed { e.tail_dx } else { e.head_dx };
            if a_dx != 0.0 || b_dx != 0.0 {
                offsets = true;
            }
            let ra = rank_of[a];
            let rb = rank_of[b];
            let mut label_rank: i32 = -1;
            if e.label_w > 0.0 {
                // ラベルは半ばに。ノードのランクを通る辺は、そのランクのどれか（ノードの脇、高さを取らない）に
                // 置く: 半ばのもの、または半ばの両側のうち辺の出るノードに近いほう（流れに逆らう辺なら下のほう）。
                // 隣り合うランクの間の辺は、その間のランクに。
                label_rank = ra + ((rb - ra) >> 1);
                if step == 2 && rb - ra >= 4 {
                    if (label_rank & 1) == 1 {
                        label_rank += if e.reversed { 1 } else { -1 };
                    }
                } else if step == 2 && (label_rank & 1) == 0 {
                    label_rank += if e.reversed { 1 } else { -1 };
                }
            }
            let mut prev = a;
            if rb - ra > 1 {
                first_dummy[ei] = next as i64;
            }
            first_seg[ei] = ce as i64;
            if let Some(efk) = efk.as_mut()
                && !(if e.reversed {
                    e.head_fixed
                } else {
                    e.tail_fixed
                })
            {
                efk[ce] = if e.reversed { ARRIVES } else { LEAVES };
            }
            for r in (ra + 1)..rb {
                let d = next;
                next += 1;
                rank_of[d] = r;
                key[d] = key[a] + ((key[b] - key[a]) * (r - ra) as f64) / (rb - ra) as f64;
                if r == label_rank {
                    w[d] = e.label_w;
                    h[d] = e.label_h;
                    kind[d] = K_LABEL;
                    label_node[ei] = d as i64;
                    heads[r as usize] |= if e.reversed { 2 } else { 1 };
                } else {
                    kind[d] = K_DUMMY;
                }
                ef[ce] = prev;
                et[ce] = d;
                efd[ce] = if prev == a { a_dx } else { 0.0 };
                ce += 1;
                prev = d;
            }
            ef[ce] = prev;
            et[ce] = b;
            efd[ce] = if prev == a { a_dx } else { 0.0 };
            etd[ce] = b_dx;
            if let Some(etk) = etk.as_mut()
                && !(if e.reversed {
                    e.tail_fixed
                } else {
                    e.head_fixed
                })
            {
                etk[ce] = if e.reversed { LEAVES } else { ARRIVES };
            }
            last_seg[ei] = ce as i64;
            ce += 1;
        }
    }

    // 曲がり点は両端を含む最も内のグループに属する。
    let mut grp: Option<Vec<i32>> = None;
    if let Some(groups) = &opt.groups {
        let of = &groups.of;
        let parent = &groups.parent;
        let mut g = vec![0i32; n];
        g[..real].copy_from_slice(&of[..real]);
        let depth = |mut gr: i32| -> i32 {
            let mut d = 0;
            while gr >= 0 {
                d += 1;
                gr = parent[gr as usize];
            }
            d
        };
        let lca = |mut x: i32, mut y: i32| -> i32 {
            let mut dx = depth(x);
            let mut dy = depth(y);
            while dx > dy {
                x = parent[x as usize];
                dx -= 1;
            }
            while dy > dx {
                y = parent[y as usize];
                dy -= 1;
            }
            while x != y {
                x = parent[x as usize];
                y = parent[y as usize];
            }
            x
        };
        for ei in 0..m {
            if first_dummy[ei] < 0 {
                continue;
            }
            let e = &edges[ei];
            let gr = lca(of[e.tail], of[e.head]);
            let d0 = first_dummy[ei] as usize;
            let end = d0 + span[ei] as usize - 1;
            for d in d0..end {
                g[d] = gr;
            }
        }
        grp = Some(g);
    }

    let mut succ_start = vec![0usize; n + 1];
    let mut pred_start = vec![0usize; n + 1];
    for e in 0..segments {
        succ_start[ef[e] + 1] += 1;
        pred_start[et[e] + 1] += 1;
    }
    for i in 0..n {
        succ_start[i + 1] += succ_start[i];
        pred_start[i + 1] += pred_start[i];
    }
    let mut succ_e = vec![0usize; segments];
    let mut pred_e = vec![0usize; segments];
    {
        let mut sf = succ_start[..n].to_vec();
        let mut pf = pred_start[..n].to_vec();
        for e in 0..segments {
            succ_e[sf[ef[e]]] = e;
            sf[ef[e]] += 1;
            pred_e[pf[et[e]]] = e;
            pf[et[e]] += 1;
        }
    }

    let mut layer_start = vec![0usize; max_rank + 2];
    for i in 0..n {
        layer_start[rank_of[i] as usize + 1] += 1;
    }
    for r in 0..=max_rank {
        layer_start[r + 1] += layer_start[r];
    }
    let mut order = vec![0usize; n];
    let pos = vec![0i32; n];

    // 初めの順: 上からの深さ優先で、部分木がまとまって始まるように。
    {
        let mut by_rank = vec![0usize; n];
        let mut fill = layer_start[..max_rank + 1].to_vec();
        for i in 0..n {
            let r = rank_of[i] as usize;
            by_rank[fill[r]] = i;
            fill[r] += 1;
        }
        fill[..=max_rank].copy_from_slice(&layer_start[..=max_rank]);
        let mut visited = vec![0u8; n];
        let mut stack: Vec<usize> = Vec::new();
        for k in 0..n {
            if visited[by_rank[k]] != 0 {
                continue;
            }
            stack.push(by_rank[k]);
            while let Some(v) = stack.pop() {
                if visited[v] != 0 {
                    continue;
                }
                visited[v] = 1;
                let r = rank_of[v] as usize;
                order[fill[r]] = v;
                fill[r] += 1;
                let mut j = succ_start[v + 1] as i64 - 1;
                while j >= succ_start[v] as i64 {
                    let wv = et[succ_e[j as usize]];
                    if visited[wv] == 0 {
                        stack.push(wv);
                    }
                    j -= 1;
                }
            }
        }
    }

    for r in 0..=max_rank {
        sort_rank(&mut order, &layer_start, r, &|v| key[v]);
    }

    let mut pinned = false;
    for i in 0..real {
        if nodes[i].pin != 0.0 {
            pinned = true;
        }
    }
    if pinned {
        for r in 0..=max_rank {
            let s = layer_start[r];
            let e = layer_start[r + 1];
            let mut layer: Vec<(usize, usize)> = order[s..e]
                .iter()
                .enumerate()
                .map(|(i, &v)| (v, i))
                .collect();
            let pin_of = |v: usize| -> f64 { if v < real { nodes[v].pin } else { 0.0 } };
            sort_by(&mut layer, |a, b| {
                or(pin_of(a.0) - pin_of(b.0), a.1 as f64 - b.1 as f64)
            });
            for (i, (v, _)) in layer.into_iter().enumerate() {
                order[s + i] = v;
            }
        }
    }
    // ポートの順を保ち、グループの外での交差を避ける。
    let mut fixed = vec![0u8; max_rank + 1];
    {
        let hint = |v: usize| -> f64 {
            if v < real && (nodes[v].kind == Kind::StartPort || nodes[v].kind == Kind::EndPort) {
                nodes[v].hint
            } else {
                f64::NAN
            }
        };
        for r in [0, max_rank] {
            if sort_rank(&mut order, &layer_start, r, &hint) {
                fixed[r] = 1;
            }
        }
    }

    let group_parent = opt.groups.as_ref().map(|g| g.parent.clone());
    let mut g = Graph {
        n,
        real,
        max_rank,
        w,
        kind,
        rank: rank_of,
        ef,
        et,
        efd,
        etd,
        succ_start,
        succ_e,
        pred_start,
        pred_e,
        layer_start,
        order,
        pos,
        offsets,
        fixed,
        grp,
        group_parent,
    };
    settle_positions(&mut g);

    let mut crossings = 0f64;
    let mut declared = 0f64;
    if segments > 0 && max_rank > 0 {
        let (c, d) = reduce_crossings(&mut g, nodes);
        crossings = c;
        declared = d;
        settle_positions(&mut g);
    }
    if opt.order_only {
        // 点は（ランク、流れを横切る位置）の組。
        let at = position(&mut g, opt);
        for i in 0..real {
            nodes[i].x = at[i];
        }
        for ei in 0..m {
            let e = &mut edges[ei];
            let a = if e.reversed { e.head } else { e.tail };
            let b = if e.reversed { e.tail } else { e.head };
            let mut pts = vec![g.rank[a] as f64, at[a]];
            if direct[ei] == 0 && first_dummy[ei] >= 0 {
                let d0 = first_dummy[ei] as usize;
                for d in d0..(d0 + span[ei] as usize - 1) {
                    pts.push(g.rank[d] as f64);
                    pts.push(at[d]);
                }
            }
            pts.push(g.rank[b] as f64);
            pts.push(at[b]);
            e.points = pts;
        }
        return LayeredResult {
            width: 0.0,
            height: 0.0,
            crossings: Some(crossings),
            declared: Some(declared),
        };
    }

    if let (Some(efk), Some(etk)) = (&efk, &etk) {
        spread(&mut g, nodes, efk, etk);
    }

    let mut xs = position(&mut g, opt);

    let empty = empty_ranks(&g);
    if let (Some(efk), Some(etk)) = (&efk, &etk) {
        let free = free_ends(&g, efk, etk);
        align_labels(&g, nodes, &mut xs, &free, opt, empty.as_deref());
        settle(&mut g, nodes, &xs, efk, etk, &free, empty.as_deref());
        snap(&g, &mut xs, empty.as_deref(), opt);
        for ei in 0..m {
            if direct[ei] != 0 {
                continue;
            }
            let e = &mut edges[ei];
            let a_dx = g.efd[first_seg[ei] as usize];
            let b_dx = g.etd[last_seg[ei] as usize];
            e.tail_dx = if e.reversed { b_dx } else { a_dx };
            e.head_dx = if e.reversed { a_dx } else { b_dx };
        }
    }

    align_ports(&g, &mut xs, empty.as_deref());

    // 曲がり点しかないランクは、ほかの辺にラベルか双子があるからだけある。その曲がり点は辺の両端を結ぶ直線へ
    // 動き、乗ったものは道筋から外して、辺を 1 本の曲線で描く。
    let mut on_line = vec![0u8; n];
    if let Some(empty) = &empty
        && !opt.tracks
    {
        straighten(
            &g,
            &mut xs,
            empty,
            &first_dummy,
            &span,
            if opt.apart { 0.0 } else { opt.edge_sep },
            &mut on_line,
        );
    }
    if !opt.tracks {
        past_labels(&g, &mut xs, &first_dummy, &span, opt.edge_sep, &mut on_line);
    }
    // 辺は何かのあるランクを通る所で切れ、切れ端はそれぞれ 1 つの隙間を横切る。`tracks` なら、まっすぐでない
    // 切れ端はみな直角に横切る道を持つ。そうでなく端がノードに広がるなら、曲がる段が要るものだけが持つ。
    let mut piece_start = vec![0usize; m + 1];
    let mut pieces: Option<Pieces> = if opt.tracks || (efk.is_some() && !opt.straight) {
        Some(Pieces {
            from: vec![],
            to: vec![],
            rank: vec![],
            lower: vec![],
            track: vec![],
            levels: vec![],
            wide: vec![],
        })
    } else {
        None
    };
    let mut track_count = vec![0i32; max_rank + 1];
    if let Some(pieces) = pieces.as_mut() {
        for ei in 0..m {
            piece_start[ei] = pieces.from.len();
            if direct[ei] != 0 {
                continue;
            }
            let d0 = first_dummy[ei];
            let end = if d0 < 0 { d0 } else { d0 + span[ei] as i64 - 1 };
            // 双子から離れるために曲がり点を保つ辺は、その曲がり点を通ってそのまま描く。
            let mut twin = false;
            let mut d = d0;
            while d < end {
                // ラベルのランクで曲がり点を外した辺もそう。その曲線はそのランクを横切る。
                let bare = empty
                    .as_ref()
                    .is_some_and(|em| em[g.rank[d as usize] as usize] == 1);
                if !opt.tracks && bare != (on_line[d as usize] == 1) {
                    twin = true;
                }
                d += 1;
            }
            if twin {
                continue;
            }
            let fs = first_seg[ei] as usize;
            let ls = last_seg[ei] as usize;
            let mut x = xs[g.ef[fs]] + g.efd[fs];
            let mut rank = g.rank[g.ef[fs]] as usize;
            let mut d = d0;
            while d < end {
                let du = d as usize;
                if empty
                    .as_ref()
                    .is_some_and(|em| em[g.rank[du] as usize] != 0)
                {
                    d += 1;
                    continue;
                }
                pieces.from.push(x);
                pieces.to.push(xs[du]);
                pieces.rank.push(rank);
                pieces.lower.push(d);
                x = xs[du];
                rank = g.rank[du] as usize;
                d += 1;
            }
            pieces.from.push(x);
            pieces.to.push(xs[g.et[ls]] + g.etd[ls]);
            pieces.rank.push(rank);
            pieces.lower.push(-1);
        }
        piece_start[m] = pieces.from.len();
    }

    // 境のポートとその通り抜けの曲がり点しかないランクは、自分の場所をあまり要らない。
    let port_only = |r: usize, k0: u8| -> bool {
        let mut found = false;
        for i in g.layer_start[r]..g.layer_start[r + 1] {
            let k = g.kind[g.order[i]];
            if k == k0 {
                found = true;
            } else if k != K_DUMMY {
                return false;
            }
        }
        found
    };
    let start_ranks: i64 = if port_only(0, K_START) {
        step as i64
    } else {
        0
    };
    let end_ranks: i64 = if port_only(max_rank, K_END) {
        step as i64
    } else {
        0
    };
    // 各ランクの下の隙間で、辺が横切る最も長い距離。
    let mut run = vec![0f64; max_rank + 1];
    for e in 0..segments {
        let across = (xs[g.ef[e]] + g.efd[e] - xs[g.et[e]] - g.etd[e]).abs();
        let r = g.rank[g.ef[e]] as usize;
        if across > run[r] {
            run[r] = across;
        }
    }
    let mut band_top = vec![0f64; max_rank + 1];
    let mut band_bottom = vec![0f64; max_rank + 1];
    let stepf = step as f64;
    let usual = |r: usize| -> f64 {
        (if (r as i64) < start_ranks || r as i64 >= max_rank as i64 - end_ranks {
            opt.port_sep
        } else {
            opt.rank_sep
        }) / stepf
    };
    // 各ランクの下の場所。切れ端に分けない辺は、遠く横切る所でもっと要る。
    let mut gap_under = vec![0f64; max_rank + 1];
    for r in 0..max_rank {
        gap_under[r] = if pieces.is_some() {
            usual(r)
        } else {
            js::max(
                usual(r),
                js::min((MAX_GAP * opt.rank_sep) / stepf, run[r] * MIN_SLOPE),
            )
        };
    }
    // ノードのランクの間で、ラベルと曲がり点だけを持つランク。
    let mut between = vec![1u8; max_rank + 1];
    for i in 0..real {
        between[g.rank[i] as usize] = 0;
    }
    for r in 1..max_rank {
        if opt.tracks || between[r] == 0 || (heads[r] != 1 && heads[r] != 2) {
            continue;
        }
        let shift = js::min(
            opt.head_room / 2.0,
            gap_under[if heads[r] == 1 { r - 1 } else { r }] / 2.0,
        );
        gap_under[r - 1] += if heads[r] == 1 { -shift } else { shift };
        gap_under[r] += if heads[r] == 1 { shift } else { -shift };
    }
    // ランクの道ごとの数と、ランクから次に何かのあるランク。
    let margin = if opt.tracks {
        TRACK_MARGIN
    } else {
        LEVEL_MARGIN
    };
    let least_pitch = if opt.tracks { TRACK_PITCH } else { LEVEL_PITCH };
    let most_pitch = if opt.tracks {
        TRACK_SPREAD
    } else {
        LEVEL_PITCH
    };
    let mut below = vec![0usize; max_rank + 1];
    if let Some(pieces) = pieces.as_mut() {
        let mut room = vec![0f64; max_rank + 1];
        for r in 0..max_rank {
            if empty.as_ref().is_some_and(|em| em[r] != 0) {
                continue;
            }
            let mut next = r + 1;
            room[r] = gap_under[r];
            while next < max_rank && empty.as_ref().is_some_and(|em| em[next] != 0) {
                room[r] += gap_under[next];
                next += 1;
            }
            below[r] = next;
        }
        for k in 0..pieces.from.len() {
            let v = if (pieces.from[k] - pieces.to[k]).abs() > LEVEL_WIDE * room[pieces.rank[k]] {
                1
            } else {
                0
            };
            pieces.wide.push(v);
        }
        let mut tall = vec![0f64; max_rank + 1];
        assign_tracks(pieces, &mut track_count, !opt.tracks, &room, &mut tall);
        for r in 0..max_rank {
            if track_count[r] > 0 {
                gap_under[r] += js::max(
                    0.0,
                    2.0 * margin + (track_count[r] - 1) as f64 * least_pitch - room[r],
                );
            } else if tall[r] > 0.0 {
                gap_under[r] += js::max(0.0, js::min(MAX_GAP * opt.rank_sep, tall[r]) - room[r]);
            }
        }
    }
    let mut y = 0f64;
    for r in 0..=max_rank {
        let mut hh = 0f64;
        for i in g.layer_start[r]..g.layer_start[r + 1] {
            if h[g.order[i]] > hh {
                hh = h[g.order[i]];
            }
        }
        band_top[r] = y;
        band_bottom[r] = y + hh;
        y += hh + gap_under[r];
    }
    let height = y;
    // 曲がり点やラベルで辺がランクをまっすぐ通る部分。ラベルの脇の曲がり点は真ん中の半分に収まる。
    let inset = |d: usize| -> f64 {
        let r = g.rank[d] as usize;
        if g.kind[d] == K_DUMMY && between[r] != 0 && !opt.tracks {
            (band_bottom[r] - band_top[r]) / 4.0
        } else {
            0.0
        }
    };
    // 切れ端が隙間を横切る高さ（またはその組の別の道での高さ）。道は隙間の真ん中に、最初が上に並ぶ。
    let track_y = |pieces: &Pieces, piece: usize, on_track: i32| -> f64 {
        let r = pieces.rank[piece];
        let top = band_bottom[r];
        let bottom = band_top[below[r]];
        let count = if opt.tracks {
            track_count[r]
        } else {
            pieces.levels[piece]
        } as f64;
        let pitch = if count > 1.0 {
            js::min(most_pitch, (bottom - top - 2.0 * margin) / (count - 1.0))
        } else {
            0.0
        };
        (top + bottom) / 2.0 + (on_track as f64 - (count - 1.0) / 2.0) * pitch
    };

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    for i in 0..n {
        let half = g.w[i] / 2.0;
        if xs[i] - half < min_x {
            min_x = xs[i] - half;
        }
        if xs[i] + half > max_x {
            max_x = xs[i] + half;
        }
    }
    // 端がノードに広がるとき、送るだけのランクは 1 本の線に立ち、受けるだけのランクは 1 本の線から下がる。
    // ほかのランクのノードは真ん中に。
    let mut arrives = vec![0u8; max_rank + 1];
    let mut leaves = vec![0u8; max_rank + 1];
    for i in 0..real {
        if g.kind[i] != K_NODE || efk.is_none() {
            continue;
        }
        if g.pred_start[i + 1] > g.pred_start[i] {
            arrives[g.rank[i] as usize] = 1;
        }
        if g.succ_start[i + 1] > g.succ_start[i] {
            leaves[g.rank[i] as usize] = 1;
        }
    }
    for i in 0..real {
        let node = &mut nodes[i];
        let r = node.rank as usize;
        node.x = xs[i] - min_x;
        if g.kind[i] == K_NODE && leaves[r] != 0 && arrives[r] == 0 {
            node.y = band_bottom[r] - node.h / 2.0;
        } else if g.kind[i] == K_NODE && arrives[r] != 0 && leaves[r] == 0 {
            node.y = band_top[r] + node.h / 2.0;
        } else {
            node.y = (band_top[r] + band_bottom[r]) / 2.0;
        }
    }

    for ei in 0..m {
        let (fx, fy, fh, lx, ly, lh) = {
            let e = &edges[ei];
            let first = &nodes[if e.reversed { e.head } else { e.tail }];
            let last = &nodes[if e.reversed { e.tail } else { e.head }];
            (first.x, first.y, first.h, last.x, last.y, last.h)
        };
        let e = &mut edges[ei];
        let mut pts: Vec<f64> = vec![
            fx + (if e.reversed { e.head_dx } else { e.tail_dx }),
            fy + fh / 2.0,
        ];
        let d0 = first_dummy[ei];
        if let Some(pieces) = pieces
            .as_ref()
            .filter(|_| piece_start[ei + 1] > piece_start[ei])
        {
            for k in piece_start[ei]..piece_start[ei + 1] {
                if pieces.track[k] >= 0 && (opt.tracks || pieces.wide[k] == 1) {
                    let across = track_y(pieces, k, pieces.track[k]);
                    pts.extend_from_slice(&[
                        pieces.from[k] - min_x,
                        across,
                        pieces.to[k] - min_x,
                        across,
                    ]);
                } else if pieces.track[k] > 0 {
                    // 段の間の短い切れ端は、自分の 1 つ前の段を過ぎるまでまっすぐ走り、そこから 1 つの S で曲がる。
                    pts.push(pieces.from[k] - min_x);
                    pts.push(track_y(pieces, k, pieces.track[k] - 1));
                }
                let d = pieces.lower[k];
                if d < 0 {
                    continue;
                }
                let d = d as usize;
                let r = g.rank[d] as usize;
                pts.push(xs[d] - min_x);
                pts.push(band_top[r] + inset(d));
                if band_bottom[r] > band_top[r] {
                    pts.push(xs[d] - min_x);
                    pts.push(band_bottom[r] - inset(d));
                }
            }
        } else if d0 >= 0 {
            let d0 = d0 as usize;
            for d in d0..(d0 + span[ei] as usize - 1) {
                if on_line[d] != 0 {
                    continue;
                }
                let x = xs[d] - min_x;
                let r = g.rank[d] as usize;
                let top = band_top[r];
                let bottom = band_bottom[r];
                pts.push(x);
                pts.push(top + inset(d));
                if bottom > top {
                    pts.push(x);
                    pts.push(bottom - inset(d));
                }
            }
        }
        pts.push(lx + (if e.reversed { e.tail_dx } else { e.head_dx }));
        pts.push(ly - lh / 2.0);
        if e.reversed {
            let mut i = 0usize;
            let mut j = pts.len() - 2;
            while i < j {
                pts.swap(i, j);
                pts.swap(i + 1, j + 1);
                i += 2;
                j -= 2;
            }
        }
        let ln = label_node[ei];
        if ln >= 0 {
            let ln = ln as usize;
            let r = g.rank[ln] as usize;
            e.label_x = xs[ln] - min_x;
            e.label_y = (band_top[r] + band_bottom[r]) / 2.0;
        } else if e.label_w > 0.0 {
            e.label_x = (pts[0] + pts[pts.len() - 2]) / 2.0;
            e.label_y = (pts[1] + pts[pts.len() - 1]) / 2.0;
        }
        e.points = pts;
    }

    LayeredResult {
        width: max_x - min_x,
        height,
        crossings: None,
        declared: None,
    }
}

/// 閉路を作る辺を、宣言順の深さ優先で見つけて逆にする。
pub fn orient(nodes: &[LNode], edges: &mut [LEdge]) {
    let n = nodes.len();
    let m = edges.len();
    let mut start = vec![0usize; n + 1];
    for i in 0..m {
        let e = &mut edges[i];
        e.reversed = nodes[e.head].kind == Kind::StartPort || nodes[e.tail].kind == Kind::EndPort;
        if !e.reversed {
            start[e.tail + 1] += 1;
        }
    }
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut out = vec![0usize; start[n]];
    {
        let mut fill = start[..n].to_vec();
        for i in 0..m {
            if !edges[i].reversed {
                let t = edges[i].tail;
                out[fill[t]] = i;
                fill[t] += 1;
            }
        }
    }
    let mut state = vec![0u8; n];
    let mut cursor = start[..n].to_vec();
    let mut stack: Vec<usize> = Vec::new();
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        stack.push(root);
        state[root] = 1;
        while let Some(&v) = stack.last() {
            if cursor[v] < start[v + 1] {
                let ei = out[cursor[v]];
                cursor[v] += 1;
                let w = edges[ei].head;
                if state[w] == 1 {
                    edges[ei].reversed = true;
                } else if state[w] == 0 {
                    state[w] = 1;
                    stack.push(w);
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
}

pub fn rank(nodes: &mut [LNode], edges: &[LEdge], step: i32) {
    let n = nodes.len();
    let m = edges.len();
    let mut from = vec![0usize; m];
    let mut to = vec![0usize; m];
    let mut len = vec![0i32; m];
    let mut out_start = vec![0usize; n + 1];
    let mut in_start = vec![0usize; n + 1];
    for i in 0..m {
        let e = &edges[i];
        let a = if e.reversed { e.head } else { e.tail };
        let b = if e.reversed { e.tail } else { e.head };
        from[i] = a;
        to[i] = b;
        len[i] = js::to_int32(js::max(1.0, e.minlen) * step as f64);
        out_start[a + 1] += 1;
        in_start[b + 1] += 1;
    }
    for i in 0..n {
        out_start[i + 1] += out_start[i];
        in_start[i + 1] += in_start[i];
    }
    let mut out_e = vec![0usize; m];
    let mut in_e = vec![0usize; m];
    {
        let mut of = out_start[..n].to_vec();
        let mut inf = in_start[..n].to_vec();
        for i in 0..m {
            out_e[of[from[i]]] = i;
            of[from[i]] += 1;
            in_e[inf[to[i]]] = i;
            inf[to[i]] += 1;
        }
    }

    let mut r = vec![0i32; n];
    // 始まりのポートはあとでどのノードより上に置くので、行き先を 1 ランク押し下げないように。
    for i in 0..n {
        if nodes[i].kind == Kind::StartPort {
            r[i] = -step;
        }
    }
    let mut indeg = vec![0i64; n];
    for i in 0..n {
        indeg[i] = (in_start[i + 1] - in_start[i]) as i64;
    }
    let mut topo = vec![0usize; n];
    let mut count = 0usize;
    for i in 0..n {
        if indeg[i] == 0 {
            topo[count] = i;
            count += 1;
        }
    }
    let mut q = 0;
    while q < count {
        let v = topo[q];
        for k in out_start[v]..out_start[v + 1] {
            let ei = out_e[k];
            let w = to[ei];
            if r[v] + len[ei] > r[w] {
                r[w] = r[v] + len[ei];
            }
            indeg[w] -= 1;
            if indeg[w] == 0 {
                topo[count] = w;
                count += 1;
            }
        }
        q += 1;
    }

    // 最長路のランク付けは、ゆるくつながるノードを隣から遠くに置く。辺の多い側へ動かす。
    for _pass in 0..8 {
        let mut changed = false;
        for q in (0..count).rev() {
            let v = topo[q];
            let ins = in_start[v + 1] - in_start[v];
            let outs = out_start[v + 1] - out_start[v];
            if ins == 0 && outs == 0 {
                continue;
            }
            let mut lo = f64::NEG_INFINITY;
            let mut hi = f64::INFINITY;
            for k in in_start[v]..in_start[v + 1] {
                let ei = in_e[k];
                lo = js::max(lo, (r[from[ei]] + len[ei]) as f64);
            }
            for k in out_start[v]..out_start[v + 1] {
                let ei = out_e[k];
                hi = js::min(hi, (r[to[ei]] - len[ei]) as f64);
            }
            let mut next = r[v] as f64;
            if outs > ins && hi != f64::INFINITY {
                next = hi;
            } else if ins > outs && lo != f64::NEG_INFINITY {
                next = lo;
            }
            if next != r[v] as f64 {
                r[v] = js::to_int32(next);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut min = i64::MAX;
    let mut max = i64::MIN;
    let mut start_ports = false;
    for i in 0..n {
        let kind = nodes[i].kind;
        if kind == Kind::StartPort {
            start_ports = true;
        }
        if kind == Kind::StartPort || kind == Kind::EndPort {
            continue;
        }
        if (r[i] as i64) < min {
            min = r[i] as i64;
        }
        if r[i] as i64 > max {
            max = r[i] as i64;
        }
    }
    if min == i64::MAX {
        min = 0;
        max = 0;
    }
    for i in 0..n {
        if nodes[i].kind == Kind::StartPort {
            r[i] = (min - step as i64) as i32;
        } else if nodes[i].kind == Kind::EndPort {
            r[i] = (max + step as i64) as i32;
        }
    }
    if start_ports {
        min -= step as i64;
    }
    for i in 0..n {
        nodes[i].rank = (r[i] as i64 - min) as i32;
    }
}

// ノードの 1 辺（0 は辺の着く側、1 はもう一方）を動いてよい辺の端を、交差を避けて向こうのノードの順に並べる。
// `ids` と `room` は空で渡す。`ids` に端、`room` に間の最小を入れ、間の和を返す。幅が足りなければ、並んで
// 出る辺（矢じりがない）から詰め、さらに足りなければ同じ向きの隣どうしで場所を分け合う。
fn side_ends(
    g: &Graph,
    v: usize,
    side: usize,
    span: f64,
    kinds: &[u8],
    ids: &mut Vec<usize>,
    room: &mut Vec<f64>,
) -> f64 {
    let start = if side == 0 {
        &g.pred_start
    } else {
        &g.succ_start
    };
    let list = if side == 0 { &g.pred_e } else { &g.succ_e };
    let other = if side == 0 { &g.ef } else { &g.et };
    let pos = &g.pos;
    for k in start[v]..start[v + 1] {
        if kinds[list[k]] != HELD {
            ids.push(list[k]);
        }
    }
    if ids.len() < 2 {
        return 0.0;
    }
    sort_by(ids, |a, b| {
        or((pos[other[a]] - pos[other[b]]) as f64, a as f64 - b as f64)
    });
    room.clear();
    room.resize(ids.len(), 0.0);
    let fill = |room: &mut Vec<f64>, share: &dyn Fn(bool, bool) -> f64| -> f64 {
        let mut total = 0.0;
        room[0] = 0.0;
        for k in 1..ids.len() {
            let same = kinds[ids[k]] == kinds[ids[k - 1]];
            room[k] = share(same && kinds[ids[k]] == LEAVES, same);
            total += room[k];
        }
        total
    };
    let mut total = fill(room, &|_, _| 1.0);
    if span / total < MIN_PITCH {
        total = fill(room, &|leaves, _| if leaves { 0.5 } else { 1.0 });
    }
    if span / total < MIN_PITCH {
        total = fill(room, &|_, same| if same { 0.0 } else { 1.0 });
    }
    if total == 0.0 {
        return 0.0;
    }
    let pitch = js::min(PORT_PITCH, span / total);
    for k in 1..ids.len() {
        room[k] *= pitch;
    }
    total * pitch
}

// ノードの 1 辺に着く辺に、真ん中のまわりにそれぞれの場所を与える。
fn spread(g: &mut Graph, nodes: &[LNode], efk: &[u8], etk: &[u8]) {
    for v in 0..g.real {
        let span = js::min(nodes[v].span, g.w[v]);
        if span <= 0.0 || nodes[v].kind != Kind::Node {
            continue;
        }
        for side in 0..2 {
            let mut ids: Vec<usize> = Vec::new();
            let mut room: Vec<f64> = Vec::new();
            let total = side_ends(
                g,
                v,
                side,
                span,
                if side == 0 { etk } else { efk },
                &mut ids,
                &mut room,
            );
            if total == 0.0 {
                continue;
            }
            let dx = if side == 0 { &mut g.etd } else { &mut g.efd };
            let mut at = -total / 2.0;
            for k in 0..ids.len() {
                at += room[k];
                dx[ids[k]] = at;
            }
        }
    }
}

// 切れ端 `e` から上（side 0）か下へ、ほかに何もないランクの曲がり点を過ぎて、次に留まった点に着く切れ端。
fn follow(g: &Graph, empty: Option<&[u8]>, mut e: usize, side: usize) -> usize {
    let Some(empty) = empty else { return e };
    for _hops in 0..100000 {
        let u = if side == 0 { g.ef[e] } else { g.et[e] };
        if g.kind[u] != K_DUMMY || empty[g.rank[u] as usize] == 0 {
            break;
        }
        e = if side == 0 {
            g.pred_e[g.pred_start[u]]
        } else {
            g.succ_e[g.succ_start[u]]
        };
    }
    e
}

// 境のポートをノードの端と一直線にし、辺が隙間をまっすぐ横切るように。ポートは置くとき `edge_sep` 離すので
// （短い辺の端より広い）、これがないと辺は境の外で曲がり、隙間でまた曲がる。どのポートも隣を越えず触れずに
// 動けるときだけランクを動かす。
fn align_ports(g: &Graph, xs: &mut [f64], empty: Option<&[u8]>) {
    for (r, kind) in [(0usize, K_START), (g.max_rank, K_END)] {
        let s = g.layer_start[r];
        let end = g.layer_start[r + 1];
        let mut to: Vec<f64> = Vec::new();
        for i in s..end {
            let v = g.order[i];
            let start = kind == K_START;
            let deg = if start {
                g.succ_start[v + 1] - g.succ_start[v]
            } else {
                g.pred_start[v + 1] - g.pred_start[v]
            };
            if g.kind[v] != kind || deg != 1 {
                break;
            }
            let e = follow(
                g,
                empty,
                if start {
                    g.succ_e[g.succ_start[v]]
                } else {
                    g.pred_e[g.pred_start[v]]
                },
                if start { 1 } else { 0 },
            );
            let at = if start {
                xs[g.et[e]] + g.etd[e]
            } else {
                xs[g.ef[e]] + g.efd[e]
            };
            if let Some(&last) = to.last()
                && at < last + TWIN
            {
                break;
            }
            to.push(at);
        }
        if to.is_empty() || to.len() != end - s {
            continue;
        }
        for i in s..end {
            xs[g.order[i]] = to[i - s];
        }
    }
}

// 各ノードが、辺の着く側ともう一方に、動いてよい端をいくつ持つか。
fn free_ends(g: &Graph, efk: &[u8], etk: &[u8]) -> [Vec<i32>; 2] {
    let mut free = [vec![0i32; g.n], vec![0i32; g.n]];
    for e in 0..g.ef.len() {
        if etk[e] != HELD {
            free[0][g.et[e]] += 1;
        }
        if efk[e] != HELD {
            free[1][g.ef[e]] += 1;
        }
    }
    free
}

/// ふつうの S の取っ手が流れに沿って届く割合（沿う距離に対して）。半分で均等な S。横切る距離が沿う距離の 8 倍を
/// 超えると早めに曲がり、16 倍で 4 分の 1 まで。配置は曲線どうしの近さを測るのに、描画は描くのに使う。
pub fn ease(across: f64, along: f64) -> f64 {
    let a = across.abs();
    js::max(
        0.25,
        js::min(
            0.5,
            (4.0 * along.abs()) / if js::truthy(a) { a } else { 1.0 },
        ),
    )
}

fn placed_dummy(g: &Graph, empty: Option<&[u8]>, v: usize) -> bool {
    g.kind[v] == K_LABEL
        || (g.kind[v] == K_DUMMY && !empty.is_some_and(|em| em[g.rank[v] as usize] == 1))
}

// 各辺のラベルと曲がり点を辺の一方の端と一直線にし、その端からまっすぐ走ってもう一方で 1 度だけ曲がるように:
// ノードの横の辺が少ない端（辺の真ん中に近いほう）、さもなくばもう一方と、ランクの隣が許すかぎり近く。もう
// 一方の端はノードに沿って動いて線に会う。ほかに何もないランクの曲がり点は `straighten` に任せる。ラベルと
// 曲がり点のすべてが乗れる線のない辺は、それぞれを上か下の留まった最も近い点と一直線にする。
fn align_labels(
    g: &Graph,
    nodes: &[LNode],
    xs: &mut [f64],
    free: &[Vec<i32>; 2],
    opt: &LayeredOptions,
    empty: Option<&[u8]>,
) {
    let sep = opt.edge_sep;
    let real = g.real;
    // 切れ端 `e` の上（side 0）か下の端と、そこでノードの辺を分け合う端の数（ノードでなければ 0）。
    let end = |xs: &[f64], e: usize, side: usize| -> (f64, i32) {
        let u = if side == 0 { g.ef[e] } else { g.et[e] };
        let x = if side == 0 {
            xs[u] + g.efd[e]
        } else {
            xs[u] + g.etd[e]
        };
        (
            x,
            if u < real && nodes[u].kind == Kind::Node {
                free[1 - side][u]
            } else {
                0
            },
        )
    };
    // ランクの隣がラベルや曲がり点に残す場所: ノードや 2 つのラベルとは隙間をまるごと、ラベルとそれを通る線とは半分。
    let room = |xs: &[f64], v: usize| -> (f64, f64) {
        let r = g.rank[v] as usize;
        let s = g.layer_start[r];
        let i = s + g.pos[v] as usize;
        let clear = |other: usize| -> f64 {
            g.w[other] / 2.0
                + g.w[v] / 2.0
                + if g.kind[other] == K_NODE {
                    opt.node_sep / 2.0 + sep / 2.0
                } else if g.kind[other] == g.kind[v] {
                    sep
                } else {
                    sep / 2.0
                }
        };
        (
            if i > s {
                xs[g.order[i - 1]] + clear(g.order[i - 1])
            } else {
                f64::NEG_INFINITY
            },
            if i + 1 < g.layer_start[r + 1] {
                xs[g.order[i + 1]] - clear(g.order[i + 1])
            } else {
                f64::INFINITY
            },
        )
    };
    // `lo` と `hi` の間で 2 つの端のどちらかと一直線の所、なければ最初の選びに最も近い所。
    let choose = |x: f64, up: (f64, i32), down: (f64, i32), lo: f64, hi: f64| -> f64 {
        let up_first = if up.1 != down.1 {
            up.1 < down.1
        } else {
            (up.0 - x).abs() <= (down.0 - x).abs()
        };
        let choices = if up_first {
            [up.0, down.0]
        } else {
            [down.0, up.0]
        };
        for c in choices {
            if c >= lo && c <= hi {
                return c;
            }
        }
        js::max(lo, js::min(hi, choices[0]))
    };
    let mut done = vec![0u8; g.n];
    let mut chain: Vec<usize> = Vec::new();
    for r in 1..g.max_rank {
        for i in g.layer_start[r]..g.layer_start[r + 1] {
            if !placed_dummy(g, empty, g.order[i]) || done[g.order[i]] != 0 {
                continue;
            }
            // 辺の最初のノードから最後までのラベルと曲がり点と、それらが分け合う場所。
            let mut top = g.order[i];
            while is_dummy(g.kind[g.ef[g.pred_e[g.pred_start[top]]]]) {
                top = g.ef[g.pred_e[g.pred_start[top]]];
            }
            let mut bottom = top;
            let mut lo = f64::NEG_INFINITY;
            let mut hi = f64::INFINITY;
            chain.clear();
            let mut d = top;
            while is_dummy(g.kind[d]) {
                bottom = d;
                if placed_dummy(g, empty, d) {
                    done[d] = 1;
                    chain.push(d);
                    let (least, most) = room(xs, d);
                    if least > lo {
                        lo = least;
                    }
                    if most < hi {
                        hi = most;
                    }
                }
                d = g.et[g.succ_e[g.succ_start[d]]];
            }
            if lo <= hi {
                let x = choose(
                    xs[chain[0]],
                    end(xs, g.pred_e[g.pred_start[top]], 0),
                    end(xs, g.succ_e[g.succ_start[bottom]], 1),
                    lo,
                    hi,
                );
                for &d in &chain {
                    xs[d] = x;
                }
                continue;
            }
            for &d in &chain {
                let (least, most) = room(xs, d);
                if least > most {
                    continue;
                }
                let up = end(xs, follow(g, empty, g.pred_e[g.pred_start[d]], 0), 0);
                let down = end(xs, follow(g, empty, g.succ_e[g.succ_start[d]], 1), 1);
                xs[d] = choose(xs[d], up, down, least, most);
            }
        }
    }
}

struct SideRun {
    v: usize,
    side: usize,
    span: f64,
    ids: Vec<usize>,
    room: Vec<f64>,
    all: Vec<usize>,
}

fn dx_of(g: &Graph, side: usize, e: usize) -> f64 {
    if side == 0 { g.etd[e] } else { g.efd[e] }
}

fn set_dx(g: &mut Graph, side: usize, e: usize, v: f64) {
    if side == 0 {
        g.etd[e] = v;
    } else {
        g.efd[e] = v;
    }
}

// ノードの場所が決まったあと、辺をまっすぐにできるなら端を辺に沿って動かす: 辺の行き先の点、または行き先の辺の
// どこかが、端の辺と一直線のとき。ほかの端は辺の行き先へ傾き（辺が十分長ければ）、辺を独り占めする端は真ん中に。
fn settle(
    g: &mut Graph,
    nodes: &[LNode],
    xs: &[f64],
    efk: &[u8],
    etk: &[u8],
    free: &[Vec<i32>; 2],
    empty: Option<&[u8]>,
) {
    let real = g.real;
    // ノード `v` での切れ端 `e` の端の、辺の真ん中からの望む位置。切れ端から上（side 0）か下へ、ほかに何もない
    // ランクの曲がり点を過ぎて、留まった点か別のノードの辺に着くまでたどる。
    let wanted = |g: &Graph, e: usize, side: usize, v: usize, span: f64, again: bool| -> f64 {
        if free[side][v] < 2 {
            return 0.0;
        }
        let e = follow(g, empty, e, side);
        let u = if side == 0 { g.ef[e] } else { g.et[e] };
        let point = if side == 0 {
            xs[u] + g.efd[e]
        } else {
            xs[u] + g.etd[e]
        };
        // もう一方の端が場所を得たら、できればそれと一直線に。
        if again && (point - xs[v]).abs() <= span / 2.0 + IN_LINE {
            return js::max(-span / 2.0, js::min(span / 2.0, point - xs[v]));
        }
        // 向こうの辺のうち、その端が動いてよい部分。
        let mut middle = point;
        let mut width = 0.0;
        if u < real
            && nodes[u].kind == Kind::Node
            && (if side == 0 { efk } else { etk })[e] != HELD
            && free[1 - side][u] >= 2
        {
            width = js::max(0.0, js::min(nodes[u].span, g.w[u]));
            if width > 0.0 {
                middle = xs[u];
            }
        }
        let lo = js::max(xs[v] - span / 2.0, middle - width / 2.0);
        let hi = js::min(xs[v] + span / 2.0, middle + width / 2.0);
        // 一直線の部分のない 2 つの辺: 端は向こうへ傾く（辺が十分長ければ）。
        if lo > hi + IN_LINE {
            let lean = js::max(0.0, span / 2.0 - LEAN_CLEAR);
            return js::max(-lean, js::min(lean, point - xs[v]));
        }
        // 両端は 2 つの辺が分け合う部分の 1 つの所に: 狭いほうの真ん中寄り。
        let place = (xs[v] * width + middle * span) / (width + span);
        js::max(lo, js::min(hi, place)) - xs[v]
    };
    // 端の順と間は通しで変わらない。留まった端が動く端を区切る。
    let mut sides: Vec<SideRun> = Vec::new();
    for v in 0..real {
        let span = js::min(nodes[v].span, g.w[v]);
        if span <= 0.0 || nodes[v].kind != Kind::Node {
            continue;
        }
        for side in 0..2 {
            let mut ids: Vec<usize> = Vec::new();
            let mut room: Vec<f64> = Vec::new();
            side_ends(
                g,
                v,
                side,
                span,
                if side == 0 { etk } else { efk },
                &mut ids,
                &mut room,
            );
            if ids.is_empty() {
                continue;
            }
            if ids.len() == 1 {
                room.clear();
                room.push(0.0);
            }
            let (start, list, other) = if side == 0 {
                (&g.pred_start, &g.pred_e, &g.ef)
            } else {
                (&g.succ_start, &g.succ_e, &g.et)
            };
            let mut all: Vec<usize> = Vec::new();
            for k in start[v]..start[v + 1] {
                all.push(list[k]);
            }
            let pos = &g.pos;
            sort_by(&mut all, |a, b| {
                or((pos[other[a]] - pos[other[b]]) as f64, a as f64 - b as f64)
            });
            sides.push(SideRun {
                v,
                side,
                span,
                ids,
                room,
                all,
            });
        }
    }
    // 辺の両端は互いに追うので、どれも動かなくなるまで繰り返す（実際は数回まで）。
    let mut pass = 0;
    let mut moved = true;
    while moved && pass < 6 {
        moved = false;
        for s in &sides {
            let (v, side, span) = (s.v, s.side, s.span);
            let (ids, room, all) = (&s.ids, &s.room, &s.all);
            let kinds = if side == 0 { etk } else { efk };
            let mut k = 0usize;
            let mut lo = -span / 2.0;
            for i in 0..=all.len() {
                let held = i < all.len() && kinds[all[i]] == HELD;
                if i < all.len() && !held {
                    continue;
                }
                // 前の留まった端からの動く端は ids[k..k + size)。
                let mut size = 0usize;
                let mut j = i as i64 - 1;
                while j >= 0 && kinds[all[j as usize]] != HELD {
                    size += 1;
                    j -= 1;
                }
                let hi = if held {
                    dx_of(g, side, all[i]) - PORT_PITCH
                } else {
                    span / 2.0
                };
                if size > 0 {
                    // 単調な当てはめ: 近すぎる隣をまとめて、順と間を保つ。
                    let mut sum: Vec<f64> = Vec::new();
                    let mut count: Vec<f64> = Vec::new();
                    let mut before = 0.0;
                    for j in 0..size {
                        if j > 0 {
                            before += room[k + j];
                        }
                        sum.push(wanted(g, ids[k + j], side, v, span, pass > 0) - before);
                        count.push(1.0);
                        // 間を詰めて同じ場所を分け合った端はまとめておく。
                        let mut together = j > 0 && room[k + j] == 0.0;
                        while sum.len() > 1
                            && (together
                                || sum[sum.len() - 2] / count[count.len() - 2]
                                    > sum[sum.len() - 1] / count[count.len() - 1])
                        {
                            together = false;
                            let top = sum.pop().unwrap();
                            let pooled = count.pop().unwrap();
                            let ls = sum.len() - 1;
                            sum[ls] += top;
                            let lc = count.len() - 1;
                            count[lc] += pooled;
                        }
                    }
                    let total = before;
                    let mut j = 0usize;
                    before = 0.0;
                    for b in 0..sum.len() {
                        // 留まった端の間に収まらなければ真ん中に。
                        let level = if hi - total < lo {
                            (lo + hi - total) / 2.0
                        } else {
                            js::max(lo, js::min(hi - total, sum[b] / count[b]))
                        };
                        let mut c = 0.0;
                        while c < count[b] {
                            if j > 0 {
                                before += room[k + j];
                            }
                            if (dx_of(g, side, ids[k + j]) - level - before).abs() > 0.01 {
                                moved = true;
                            }
                            set_dx(g, side, ids[k + j], level + before);
                            c += 1.0;
                            j += 1;
                        }
                    }
                    k += size;
                }
                if held {
                    lo = dx_of(g, side, all[i]) + PORT_PITCH;
                }
            }
        }
        pass += 1;
    }
}

// ほぼまっすぐな辺を、ノードか曲がり点を（すでに一直線のものと一緒に）流れを横切って少し動かして一直線にする
// （横のノードが場所を残すところで）。一直線のものは一直線のままなので、動くたびにまっすぐな辺が 1 本増える。
fn snap(g: &Graph, xs: &mut [f64], empty: Option<&[u8]>, opt: &LayeredOptions) {
    let n = g.n;
    let passed = |u: usize| -> bool {
        empty.is_some_and(|em| g.kind[u] == K_DUMMY && em[g.rank[u] as usize] == 1)
    };
    // 留まった点から次の点への辺（ほかに何もないランクの曲がり点を過ぎて）。
    let mut a: Vec<usize> = Vec::new();
    let mut b: Vec<usize> = Vec::new();
    let mut da: Vec<f64> = Vec::new();
    let mut db: Vec<f64> = Vec::new();
    for e in 0..g.ef.len() {
        if passed(g.ef[e]) {
            continue;
        }
        let f = follow(g, empty, e, 1);
        a.push(g.ef[e]);
        b.push(g.et[f]);
        da.push(g.efd[e]);
        db.push(g.etd[f]);
    }
    let off = |xs: &[f64], k: usize| -> f64 { xs[b[k]] + db[k] - xs[a[k]] - da[k] };
    // 一直線のノードはひとまとまりで動く。
    let mut block: Vec<usize> = (0..n).collect();
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let join = |block: &mut Vec<usize>, members: &mut Vec<Vec<usize>>, x: usize, y: usize| {
        let mut keep = block[x];
        let mut drop = block[y];
        if keep == drop {
            return;
        }
        if members[keep].len() < members[drop].len() {
            std::mem::swap(&mut keep, &mut drop);
        }
        let moved = std::mem::take(&mut members[drop]);
        for v in moved {
            block[v] = keep;
            members[keep].push(v);
        }
    };
    let mut near: Vec<usize> = Vec::new();
    for k in 0..a.len() {
        let d = off(xs, k).abs();
        if d < 0.01 {
            join(&mut block, &mut members, a[k], b[k]);
        } else if d <= BEND_SNAP {
            near.push(k);
        }
    }
    if near.is_empty() {
        return;
    }
    {
        let xs_ro: &[f64] = xs;
        sort_by(&mut near, |x, y| {
            or(
                off(xs_ro, x).abs() - off(xs_ro, y).abs(),
                x as f64 - y as f64,
            )
        });
    }
    let gap = |v: usize| -> f64 { half_room(g, opt, v) };
    // ブロックのどのノードも `shift` 動いて、横のものとの場所を保てるか。
    let free =
        |xs: &[f64], block: &[usize], members: &[Vec<usize>], id: usize, shift: f64| -> bool {
            for &v in &members[id] {
                let r = g.rank[v] as usize;
                let s = g.layer_start[r];
                let end = g.layer_start[r + 1];
                let i = s + g.pos[v] as usize;
                if i > s
                    && block[g.order[i - 1]] != id
                    && xs[v] + shift - xs[g.order[i - 1]] < gap(v) + gap(g.order[i - 1]) - 0.01
                {
                    return false;
                }
                if i + 1 < end
                    && block[g.order[i + 1]] != id
                    && xs[g.order[i + 1]] - xs[v] - shift < gap(v) + gap(g.order[i + 1]) - 0.01
                {
                    return false;
                }
            }
            true
        };
    for &k in &near {
        let d = off(xs, k);
        if d.abs() < 0.01 {
            join(&mut block, &mut members, a[k], b[k]);
            continue;
        }
        if d.abs() > BEND_SNAP || block[a[k]] == block[b[k]] {
            continue;
        }
        // 2 つのブロックの小さいほうが動く（できれば）。だめならもう一方。
        let upper = block[a[k]];
        let lower = block[b[k]];
        let first = if members[upper].len() <= members[lower].len() {
            upper
        } else {
            lower
        };
        for id in [first, if first == upper { lower } else { upper }] {
            let shift = if id == upper { d } else { -d };
            if d.abs() > SNAP && !members[id].iter().all(|&v| g.kind[v] == K_DUMMY) {
                continue;
            }
            if !free(xs, &block, &members, id, shift) {
                continue;
            }
            for &v in &members[id] {
                xs[v] += shift;
            }
            join(&mut block, &mut members, a[k], b[k]);
            break;
        }
    }
}

// 辺の曲がり点しかないランクに印を付ける（あれば）。
fn empty_ranks(g: &Graph) -> Option<Vec<u8>> {
    let mut empty = vec![0u8; g.max_rank + 1];
    let mut any = false;
    for r in 1..g.max_rank {
        let mut only = g.layer_start[r + 1] > g.layer_start[r];
        let mut i = g.layer_start[r];
        while only && i < g.layer_start[r + 1] {
            if g.kind[g.order[i]] != K_DUMMY {
                only = false;
            }
            i += 1;
        }
        if only {
            empty[r] = 1;
            any = true;
        }
    }
    if any { Some(empty) } else { None }
}

struct Spans {
    lo: Vec<f64>,
    hi: Vec<f64>,
}

impl Spans {
    fn within(&self, x: f64, k: usize) -> i32 {
        if x >= self.lo[k] - IN_LINE && x <= self.hi[k] + IN_LINE {
            1
        } else {
            0
        }
    }
}

fn mates(from: &[f64], to: &[f64], a: usize, b: usize) -> bool {
    (from[a] - from[b]).abs() < IN_LINE || (to[a] - to[b]).abs() < IN_LINE
}

fn overlap(sp: &Spans, a: usize, b: usize) -> bool {
    sp.lo[a] < sp.hi[b] - IN_LINE && sp.lo[b] < sp.hi[a] - IN_LINE
}

// a と b がそれぞれの道を要るか。
fn clash(sp: &Spans, from: &[f64], to: &[f64], a: usize, b: usize) -> bool {
    overlap(sp, a, b) && !mates(from, to, a, b)
}

// どちらが上かによって、a と b の一方がもう一方と交差しうるか。
fn meet(sp: &Spans, from: &[f64], to: &[f64], a: usize, b: usize) -> bool {
    overlap(sp, a, b)
        || sp.within(to[a], b) + sp.within(from[b], a) + sp.within(to[b], a) + sp.within(from[a], b)
            > 0
}

// a が上の道のとき a と b が交差する数: a が b を通って下りる所と、b が a を通って自分へ下りる所。
fn crossings_of(sp: &Spans, from: &[f64], to: &[f64], a: usize, b: usize) -> i32 {
    sp.within(to[a], b) + sp.within(from[b], a)
}

// リストの添字の組（小さいほうが先）。抜けた組は交わりもぶつかりもしない。
fn near_pairs(sp: &Spans, list: &[usize]) -> Vec<usize> {
    let mut by_lo: Vec<usize> = (0..list.len()).collect();
    sort_by(&mut by_lo, |a, b| sp.lo[list[a]] - sp.lo[list[b]]);
    let mut pairs: Vec<usize> = Vec::new();
    for x in 0..by_lo.len() {
        let i = by_lo[x];
        let reach = sp.hi[list[i]] + 4.0 * IN_LINE;
        let mut y = x + 1;
        while y < by_lo.len() && sp.lo[list[by_lo[y]]] <= reach {
            let j = by_lo[y];
            pairs.push(i.min(j));
            pairs.push(i.max(j));
            y += 1;
        }
    }
    pairs
}

// 1 つの隙間の切れ端 `list` に道を与え、それぞれに関わる切れ端どうしが取る道の数を与え、自分の道をその最初から
// 数える。道の数を返す。切れ端は道なしで来る。
fn lay(
    list: &mut [usize],
    sp: &Spans,
    from: &[f64],
    to: &[f64],
    track: &mut [i32],
    levels: &mut [i32],
    curved: bool,
) -> i32 {
    // 初めの順（何も言わないところで保つ順）: 遠くまで届く切れ端が先、扇の切れ端が入れ子になるように。
    sort_by(list, |a, b| {
        or(
            or(
                sp.hi[b] - sp.lo[b] - (sp.hi[a] - sp.lo[a]),
                sp.lo[a] - sp.lo[b],
            ),
            a as f64 - b as f64,
        )
    });
    let count = list.len();
    let mut above: Vec<Vec<usize>> = Vec::new();
    let mut group: Vec<usize> = (0..count).collect();
    fn find(group: &mut [usize], mut i: usize) -> usize {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    }
    let weighed = count <= MAX_WEIGHED;
    // 込んだ隙間で組を 2 乗の数だけ持たないように。
    let pairs = if weighed {
        near_pairs(sp, list)
    } else {
        Vec::new()
    };
    let mut clashes: Vec<Vec<usize>> = if weighed {
        vec![Vec::new(); count]
    } else {
        Vec::new()
    };
    let mut p = 0;
    while p < pairs.len() {
        let i = pairs[p];
        let j = pairs[p + 1];
        p += 2;
        if !clash(sp, from, to, list[i], list[j]) {
            continue;
        }
        clashes[i].push(j);
        clashes[j].push(i);
    }
    let mut sequence: Vec<usize> = (0..count).collect();
    if weighed {
        // above[j] は list[j] より上にあるべき切れ端。順はできるだけそれに従う: 上にあるべきものがみな置かれたら
        // 置き、空いたものがなければ待ちの最も少ないものを。
        let mut waiting = vec![0i32; count];
        let mut after: Vec<Vec<usize>> = Vec::new();
        for _ in 0..count {
            above.push(Vec::new());
            after.push(Vec::new());
        }
        let mut p = 0;
        while p < pairs.len() {
            let i = pairs[p];
            let j = pairs[p + 1];
            p += 2;
            let a = list[i];
            let b = list[j];
            if !meet(sp, from, to, a, b) {
                continue;
            }
            let fi = find(&mut group, i);
            let fj = find(&mut group, j);
            group[fi] = fj;
            let ab = crossings_of(sp, from, to, a, b);
            let ba = crossings_of(sp, from, to, b, a);
            if ab == ba {
                continue;
            }
            let first = if ab < ba { i } else { j };
            let second = if ab < ba { j } else { i };
            above[second].push(first);
            after[first].push(second);
            waiting[second] += 1;
        }
        let mut placed = vec![0u8; count];
        sequence = Vec::new();
        for _done in 0..count {
            let mut pick: i64 = -1;
            for i in 0..count {
                if placed[i] != 0 {
                    continue;
                }
                if pick < 0 || waiting[i] < waiting[pick as usize] {
                    pick = i as i64;
                }
                if waiting[i] == 0 {
                    break;
                }
            }
            let pick = pick as usize;
            placed[pick] = 1;
            sequence.push(pick);
            for &j in &after[pick] {
                waiting[j] -= 1;
            }
        }
    }
    let mut taken: HashSet<i32> = HashSet::new();
    let mut on_track: Vec<Vec<usize>> = Vec::new();
    let mut rows = 0i32;
    for &at in &sequence {
        let k = list[at];
        let mut t = 0i32;
        if let Some(ab) = above.get(at) {
            for &i in ab {
                if track[list[i]] >= t {
                    t = track[list[i]] + 1;
                }
            }
        }
        if weighed {
            taken.clear();
            for &i in &clashes[at] {
                taken.insert(track[list[i]]);
            }
            while taken.contains(&t) {
                t += 1;
            }
        } else {
            while on_track
                .get(t as usize)
                .is_some_and(|row| row.iter().any(|&other| clash(sp, from, to, k, other)))
            {
                t += 1;
            }
            while on_track.len() <= t as usize {
                on_track.push(Vec::new());
            }
            on_track[t as usize].push(k);
        }
        track[k] = t;
        if t >= rows {
            rows = t + 1;
        }
    }
    let mut first = vec![rows; count];
    let mut last = vec![-1i32; count];
    for i in 0..count {
        let root = find(&mut group, i);
        first[root] = first[root].min(track[list[i]]);
        last[root] = last[root].max(track[list[i]]);
    }
    for i in 0..count {
        let root = find(&mut group, i);
        levels[list[i]] = last[root] - first[root] + 1;
        if curved {
            track[list[i]] -= first[root];
        }
    }
    rows
}

// まっすぐ下りない切れ端に隙間を横切る道を与え、各隙間の道を数える。同じ隙間の 2 つの切れ端が道を分け合う
// のは取り違えようがないところだけ: 重ならないか、木の枝のように同じ点から出るか同じ点に着くところ。どちらが
// 上かは交差の少ないほう。`curved` なら道は曲線が曲がる段で、ほかと沿うか近すぎる切れ端だけが持ち、その組
// ごとに道を最初から数える。`room` は各ランクの下の隙間の高さ、`tall` は曲線の多くが交差する隙間が段の代わりに
// 持つべき高さを受ける。
fn assign_tracks(
    pieces: &mut Pieces,
    track_count: &mut [i32],
    curved: bool,
    room: &[f64],
    tall: &mut [f64],
) {
    let total = pieces.from.len();
    let from = &pieces.from;
    let to = &pieces.to;
    let rank = &pieces.rank;
    let wide = &pieces.wide;
    let track = &mut pieces.track;
    let levels = &mut pieces.levels;
    let mut by_rank: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut index: HashMap<usize, usize> = HashMap::new();
    for k in 0..total {
        track.push(-1);
        levels.push(0);
        if (from[k] - to[k]).abs() < IN_LINE {
            continue;
        }
        match index.get(&rank[k]) {
            Some(&at) => by_rank[at].1.push(k),
            None => {
                index.insert(rank[k], by_rank.len());
                by_rank.push((rank[k], vec![k]));
            }
        }
    }
    if by_rank.is_empty() {
        return;
    }
    // 切れ端は組ごとに比べるので、横切る向きの始まりと終わりは 1 度だけ求める。
    let mut sp = Spans {
        lo: vec![0.0; total],
        hi: vec![0.0; total],
    };
    for k in 0..total {
        sp.lo[k] = js::min(from[k], to[k]);
        sp.hi[k] = js::max(from[k], to[k]);
    }

    let sample = |k: usize, h: f64, out: &mut [f64; 2 * SAMPLES + 2]| {
        let c = ease(to[k] - from[k], h);
        for i in 0..=SAMPLES {
            let t = i as f64 / SAMPLES as f64;
            let u = 1.0 - t;
            out[2 * i] = from[k] + (to[k] - from[k]) * t * t * (3.0 - 2.0 * t);
            out[2 * i + 1] = h * (3.0 * c * t * u * u + 3.0 * (1.0 - c) * t * t * u + t * t * t);
        }
    };
    let reaches =
        |points: &[f64; 2 * SAMPLES + 2], line: &[f64; 2 * SAMPLES + 2], limit: f64| -> bool {
            // Math.hypot の前の安い距離の確かめに丸めの誤差を許す。
            let bound = limit * limit * (1.0 + 1e-9);
            for i in 0..=SAMPLES {
                let px = points[2 * i];
                let py = points[2 * i + 1];
                for j in 0..SAMPLES {
                    let dx = line[2 * j + 2] - line[2 * j];
                    let dy = line[2 * j + 3] - line[2 * j + 1];
                    let den = dx * dx + dy * dy;
                    let t = js::max(
                        0.0,
                        js::min(
                            1.0,
                            ((px - line[2 * j]) * dx + (py - line[2 * j + 1]) * dy)
                                / if js::truthy(den) { den } else { 1.0 },
                        ),
                    );
                    let ex = px - line[2 * j] - t * dx;
                    let ey = py - line[2 * j + 1] - t * dy;
                    if ex * ex + ey * ey < bound && js::hypot(ex, ey) < limit {
                        return true;
                    }
                }
            }
            false
        };
    let near = |a: usize, b: usize, height: f64, limit: f64| -> bool {
        let h = js::max(height / 2.0, height - 2.0 * LEVEL_LEAD);
        let mut line_a = [0f64; 2 * SAMPLES + 2];
        let mut line_b = [0f64; 2 * SAMPLES + 2];
        sample(a, h, &mut line_a);
        sample(b, h, &mut line_b);
        reaches(&line_a, &line_b, limit) || reaches(&line_b, &line_a, limit)
    };

    for (r, all) in by_rank.iter_mut() {
        let r = *r;
        if !curved {
            track_count[r] = lay(all, &sp, from, to, track, levels, curved);
            continue;
        }
        if all.len() > MAX_WEIGHED {
            continue;
        }
        // 段で曲がる切れ端: まず遠く横切り、見分けるべき同じ切れ端がほかにあるもの、次にふつうの曲線では近すぎる 2 つ。
        let mut far: Vec<usize> = all.iter().copied().filter(|&k| wide[k] == 1).collect();
        lay(&mut far, &sp, from, to, track, levels, curved);
        let mut level: HashSet<usize> = far.iter().copied().filter(|&k| levels[k] > 1).collect();
        let pairs = near_pairs(&sp, all);
        let mut p = 0;
        while p < pairs.len() {
            let a = all[pairs[p]];
            let b = all[pairs[p + 1]];
            p += 2;
            if (level.contains(&a) && level.contains(&b))
                || !clash(&sp, from, to, a, b)
                || (from[a] < to[a]) != (from[b] < to[b])
            {
                continue;
            }
            if near(
                a,
                b,
                room[r],
                if wide[a] == 1 || wide[b] == 1 {
                    LEVEL_CLEAR
                } else {
                    LEVEL_TOUCH
                },
            ) {
                level.insert(a);
                level.insert(b);
            }
        }
        for &k in &far {
            track[k] = -1;
            levels[k] = 0;
        }
        if level.is_empty() {
            continue;
        }
        let mut crossing = 0;
        let mut alongside = 0;
        let mut beside: HashSet<usize> = HashSet::new();
        let mut p = 0;
        while p < pairs.len() {
            let a = all[pairs[p]];
            let b = all[pairs[p + 1]];
            p += 2;
            if level.contains(&a) && level.contains(&b) {
                if !overlap(&sp, a, b) {
                    continue;
                }
                if (from[a] < to[a]) != (from[b] < to[b]) {
                    crossing += 1;
                } else if !mates(from, to, a, b) {
                    alongside += 1;
                }
            } else if (level.contains(&a) || level.contains(&b)) && meet(&sp, from, to, a, b) {
                beside.insert(a);
                beside.insert(b);
            }
        }
        if crossing > alongside {
            for &k in all.iter() {
                if (sp.hi[k] - sp.lo[k]) * CROSSED_SLOPE > tall[r] {
                    tall[r] = (sp.hi[k] - sp.lo[k]) * CROSSED_SLOPE;
                }
            }
            continue;
        }
        // それらと交差しうる短い切れ端も一緒に、段の中にそれぞれの場所を持ち、どれとも交差しないように。
        let mut members: Vec<usize> = all
            .iter()
            .copied()
            .filter(|k| level.contains(k) || (wide[*k] != 1 && beside.contains(k)))
            .collect();
        lay(&mut members, &sp, from, to, track, levels, curved);
        let mut most = 0;
        for &k in &members {
            if levels[k] > most {
                most = levels[k];
            }
        }
        track_count[r] = most;
    }
}

// 曲がり点しかないランクの曲がり点を、辺の留まった最も近い点どうしを結ぶ直線に動かし、道筋から外せるものに
// 印を付ける。`first_dummy` と `span` は各辺の曲がり点の並び（ランクごとに 1 つ）。`sep` は双子の辺の曲がり点の
// 間に空ける幅、0 ならそれらも外す。
fn straighten(
    g: &Graph,
    xs: &mut [f64],
    empty: &[u8],
    first_dummy: &[i64],
    span: &[i32],
    sep: f64,
    on_line: &mut [u8],
) {
    let mut want = vec![0f64; xs.len()];
    // 各曲がり点の辺が、曲がり点しかないランクを横切って結ぶ点。
    let mut from = vec![0f64; xs.len()];
    let mut to = vec![0f64; xs.len()];
    for ei in 0..first_dummy.len() {
        let d0 = first_dummy[ei];
        if d0 < 0 {
            continue;
        }
        let d0 = d0 as usize;
        let end = d0 + span[ei] as usize - 1;
        // 曲がり点のランクは並びの中の場所で、辺の最初のノードの次のランクから数える。
        let base = g.rank[d0] as usize;
        let mut d = d0;
        while d < end {
            if empty[base + d - d0] == 0 {
                d += 1;
                continue;
            }
            let mut last = d;
            while last + 1 < end && empty[base + last + 1 - d0] != 0 {
                last += 1;
            }
            let above = g.pred_e[g.pred_start[d]];
            let below = g.succ_e[g.succ_start[last]];
            let x0 = xs[g.ef[above]] + g.efd[above];
            let x1 = xs[g.et[below]] + g.etd[below];
            let steps = (last - d + 2) as f64;
            for k in d..=last {
                want[k] = x0 + ((x1 - x0) * (k - d + 1) as f64) / steps;
                from[k] = x0;
                to[k] = x1;
            }
            d = last + 1;
        }
    }

    // そのようなランクには曲がり点の邪魔はないので、どれも線に乗って道筋から外れる。残るのは双子だけ: 同じ 2 点
    // を結ぶ辺の曲がり点で、さもないと 1 本に描かれる。線のまわりに少し離れて並ぶ。
    let mut twins: Vec<usize> = Vec::new();
    for r in 1..g.max_rank {
        if empty[r] == 0 {
            continue;
        }
        let s = g.layer_start[r];
        let end = g.layer_start[r + 1];
        let mut row: Vec<usize> = g.order[s..end].to_vec();
        sort_by(&mut row, |a, b| {
            or(or(from[a] - from[b], to[a] - to[b]), a as f64 - b as f64)
        });
        let mut i = 0;
        while i < row.len() {
            twins.clear();
            let mut j = i;
            while j < row.len()
                && (from[row[j]] - from[row[i]]).abs() < TWIN
                && (to[row[j]] - to[row[i]]).abs() < TWIN
            {
                twins.push(row[j]);
                j += 1;
            }
            // 双子はランクの順を保ち、途中で 2 本の辺が入れ替わらないように。
            sort_by(&mut twins, |a, b| (g.pos[a] - g.pos[b]) as f64);
            let len = twins.len();
            for k in 0..len {
                let d = twins[k];
                xs[d] = want[d] + (k as f64 - (len as f64 - 1.0) / 2.0) * sep;
                if len == 1 || sep == 0.0 {
                    on_line[d] = 1;
                }
            }
            i = j;
        }
    }
}

// ラベルと曲がり点はあるがノードのないランクで、曲がり点は辺をラベルの間にまっすぐ通すためにある。どのラベルにも
// 当たらずに通れるなら、曲がり点は外し、辺は両側のランクの間の高さまるごとで曲がる。
fn past_labels(
    g: &Graph,
    xs: &mut [f64],
    first_dummy: &[i64],
    span: &[i32],
    clear: f64,
    on_line: &mut [u8],
) {
    let mut labelled = vec![0u8; g.max_rank + 1];
    let mut any = false;
    for r in 1..g.max_rank {
        let mut labels = false;
        let mut others = false;
        for i in g.layer_start[r]..g.layer_start[r + 1] {
            let kind = g.kind[g.order[i]];
            if kind == K_LABEL {
                labels = true;
            } else if kind != K_DUMMY {
                others = true;
            }
        }
        if labels && !others {
            labelled[r] = 1;
            any = true;
        }
    }
    if !any {
        return;
    }
    for ei in 0..first_dummy.len() {
        let d0 = first_dummy[ei];
        if d0 < 0 {
            continue;
        }
        let d0 = d0 as usize;
        let end = d0 + span[ei] as usize - 1;
        for d in d0..end {
            if g.kind[d] != K_DUMMY || labelled[g.rank[d] as usize] == 0 {
                continue;
            }
            // 両側で辺が留まる点（すでに外した曲がり点を過ぎて）。
            let mut first = d;
            let mut last = d;
            while first > d0 && on_line[first - 1] != 0 {
                first -= 1;
            }
            while last + 1 < end && on_line[last + 1] != 0 {
                last += 1;
            }
            let above = g.pred_e[g.pred_start[first]];
            let below = g.succ_e[g.succ_start[last]];
            let from = xs[g.ef[above]] + g.efd[above];
            let to = xs[g.et[below]] + g.etd[below];
            let lo = js::min(from, to) - clear;
            let hi = js::max(from, to) + clear;
            let mut free = true;
            let r = g.rank[d] as usize;
            let mut i = g.layer_start[r];
            while free && i < g.layer_start[r + 1] {
                let label = g.order[i];
                if g.kind[label] == K_LABEL
                    && xs[label] + g.w[label] / 2.0 > lo
                    && xs[label] - g.w[label] / 2.0 < hi
                {
                    free = false;
                }
                i += 1;
            }
            if !free {
                continue;
            }
            on_line[d] = 1;
            // 2 点の間の外した曲がり点は、その間の直線に乗る。
            let steps = (last - first + 2) as f64;
            for k in first..=last {
                xs[k] = from + ((to - from) * (k - first + 1) as f64) / steps;
            }
        }
    }
}

// 交差を数える道具。
struct Counter {
    place: Option<Vec<i32>>,
    place_in: Vec<i32>,
    places: Vec<i32>,
    leaving: Vec<usize>,
    mixed: Vec<u8>,
    tree: Vec<i32>,
    targets: Vec<i32>,
}

impl Counter {
    fn count(&mut self, g: &Graph) -> f64 {
        let mut total = 0f64;
        for r in 0..g.max_rank {
            let mut south_len = (g.layer_start[r + 2] - g.layer_start[r + 1]) as i64;
            if let Some(place) = self.place.as_mut() {
                let mut next = 0i32;
                for i in g.layer_start[r + 1]..g.layer_start[r + 2] {
                    let v = g.order[i];
                    for k in g.pred_start[v]..g.pred_start[v + 1] {
                        place[g.pred_e[k]] = next + self.place_in[g.pred_e[k]];
                    }
                    next += self.places[v];
                }
                south_len = next as i64;
            }
            if south_len < 2 {
                continue;
            }
            let mut t = 0usize;
            for i in g.layer_start[r]..g.layer_start[r + 1] {
                let v = g.order[i];
                let from = g.succ_start[v];
                let to = g.succ_start[v + 1];
                let t0 = t;
                for k in from..to {
                    self.targets[t] = match &self.place {
                        Some(place) => place[self.leaving[k]],
                        None => g.pos[g.et[g.succ_e[k]]],
                    };
                    t += 1;
                }
                let d = t - t0;
                if d < 2 {
                    continue;
                }
                if self.place.is_some() && self.mixed[v] != 0 {
                    // 同じずらしの辺を行き先で並べ、共有する端を交差と数えないように。
                    for a in (t0 + 1)..t {
                        let item = self.targets[a];
                        let dx = g.efd[self.leaving[from + a - t0]];
                        let mut b = a as i64 - 1;
                        while b >= t0 as i64
                            && g.efd[self.leaving[from + b as usize - t0]] == dx
                            && self.targets[b as usize] > item
                        {
                            self.targets[(b + 1) as usize] = self.targets[b as usize];
                            b -= 1;
                        }
                        self.targets[(b + 1) as usize] = item;
                    }
                } else if d <= 12 {
                    for a in (t0 + 1)..t {
                        let item = self.targets[a];
                        let mut b = a as i64 - 1;
                        while b >= t0 as i64 && self.targets[b as usize] > item {
                            self.targets[(b + 1) as usize] = self.targets[b as usize];
                            b -= 1;
                        }
                        self.targets[(b + 1) as usize] = item;
                    }
                } else {
                    self.targets[t0..t].sort();
                }
            }
            if t < 2 {
                continue;
            }
            let mut leaves: i64 = 1;
            while leaves < south_len {
                leaves <<= 1;
            }
            let leaves = leaves as usize;
            for x in &mut self.tree[..2 * leaves] {
                *x = 0;
            }
            for k in 0..t {
                let mut i = self.targets[k] as usize + leaves;
                self.tree[i] += 1;
                while i > 1 {
                    if (i & 1) == 0 {
                        total += self.tree[i + 1] as f64;
                    }
                    i >>= 1;
                    self.tree[i] += 1;
                }
            }
        }
        total
    }
}

fn settle_positions(g: &mut Graph) {
    for r in 0..=g.max_rank {
        for i in g.layer_start[r]..g.layer_start[r + 1] {
            g.pos[g.order[i]] = (i - g.layer_start[r]) as i32;
        }
    }
}

// グループをまとめ、それぞれの平均の鍵で並べる。
struct Arranger {
    depth: Vec<i32>,
    group_sum: Vec<f64>,
    group_count: Vec<i32>,
    group_first: Vec<usize>,
}

impl Arranger {
    fn arrange(&mut self, g: &mut Graph, r: usize, key: &[f64]) {
        let grp = g.grp.as_ref().unwrap();
        let parent = g.group_parent.as_ref().unwrap();
        let n = g.n;
        let s = g.layer_start[r];
        let e = g.layer_start[r + 1];
        for i in s..e {
            let mut gr = grp[g.order[i]];
            while gr >= 0 {
                self.group_sum[gr as usize] = 0.0;
                self.group_count[gr as usize] = 0;
                gr = parent[gr as usize];
            }
        }
        for i in s..e {
            let v = g.order[i];
            let mut gr = grp[v];
            while gr >= 0 {
                let gu = gr as usize;
                if self.group_count[gu] == 0 {
                    self.group_first[gu] = i;
                }
                self.group_sum[gu] += key[v];
                self.group_count[gu] += 1;
                gr = parent[gu];
            }
        }
        // 兄弟は最も近い共通のグループの中で比べる。グループの項目は n + グループの番号。
        let value = |item: usize| -> f64 {
            if item >= n {
                self.group_sum[item - n] / self.group_count[item - n] as f64
            } else {
                key[item]
            }
        };
        let pos = &g.pos;
        let first = |item: usize| -> f64 {
            if item >= n {
                self.group_first[item - n] as f64
            } else {
                (s as i64 + pos[item] as i64) as f64
            }
        };
        let depth = &self.depth;
        let compare = |a: usize, b: usize| -> f64 {
            let mut ia = a;
            let mut ib = b;
            let mut ca = grp[a];
            let mut cb = grp[b];
            let mut da = if ca < 0 { 0 } else { depth[ca as usize] };
            let mut db = if cb < 0 { 0 } else { depth[cb as usize] };
            while da > db {
                ia = n + ca as usize;
                ca = parent[ca as usize];
                da -= 1;
            }
            while db > da {
                ib = n + cb as usize;
                cb = parent[cb as usize];
                db -= 1;
            }
            while ca != cb {
                ia = n + ca as usize;
                ca = parent[ca as usize];
                ib = n + cb as usize;
                cb = parent[cb as usize];
            }
            or(value(ia) - value(ib), first(ia) - first(ib))
        };
        let mut row: Vec<usize> = g.order[s..e].to_vec();
        sort_by(&mut row, compare);
        g.order[s..e].copy_from_slice(&row);
        for i in s..e {
            g.pos[g.order[i]] = (i - s) as i32;
        }
    }
}

// [最後の交差, まとめ・並べ替えの前の交差] を返す。
fn reduce_crossings(g: &mut Graph, nodes: &[LNode]) -> (f64, f64) {
    let n = g.n;
    let real = g.real;
    let max_rank = g.max_rank;
    let mut widest = 0usize;
    for r in 0..=max_rank {
        widest = widest.max(g.layer_start[r + 1] - g.layer_start[r]);
    }
    if widest < 2 {
        return (0.0, 0.0);
    }
    let segs = g.ef.len();
    // ずらしがあるとき、1 つのノードに違うずらしで着く辺は、ノードに沿ってそれぞれの場所を取る。
    let has_place = g.offsets;
    // ノードの中の添字と数は、ノードの順によらない。
    let mut place_in = vec![0i32; if has_place { segs } else { 0 }];
    let mut places = vec![0i32; if has_place { n } else { 0 }];
    let mut leaving = g.succ_e.clone();
    let mut mixed = vec![0u8; if has_place { n } else { 0 }];
    if has_place {
        let mut arriving = g.pred_e.clone();
        let by_offset = |list: &mut [usize], from: usize, to: usize, dx: &[f64]| -> bool {
            for k in (from + 1)..to {
                if dx[list[k]] == dx[list[from]] {
                    continue;
                }
                sort_by(&mut list[from..to], |a, b| dx[a] - dx[b]);
                return true;
            }
            false
        };
        for v in 0..n {
            places[v] = 1;
            if by_offset(&mut arriving, g.pred_start[v], g.pred_start[v + 1], &g.etd) {
                for k in (g.pred_start[v] + 1)..g.pred_start[v + 1] {
                    if g.etd[arriving[k]] != g.etd[arriving[k - 1]] {
                        places[v] += 1;
                    }
                    place_in[arriving[k]] = places[v] - 1;
                }
            }
            if by_offset(&mut leaving, g.succ_start[v], g.succ_start[v + 1], &g.efd) {
                mixed[v] = 1;
            }
        }
    }
    let mut size = 1usize;
    while size < (if has_place { widest + segs } else { widest }) {
        size <<= 1;
    }
    let mut counter = Counter {
        place: if has_place {
            Some(vec![0i32; segs])
        } else {
            None
        },
        place_in,
        places,
        leaving,
        mixed,
        tree: vec![0i32; 2 * size],
        targets: vec![0i32; segs],
    };

    let declared = counter.count(g);
    let mut bary = vec![0f64; n];
    let mut moving = vec![0usize; widest];
    let mut spare = vec![0usize; widest];

    // グループをまとめ、それぞれの平均の鍵で並べる。
    let groups = g.group_parent.as_ref().map_or(0, |p| p.len());
    let mut arranger = Arranger {
        depth: vec![0i32; groups],
        group_sum: vec![0f64; groups],
        group_count: vec![0i32; groups],
        group_first: vec![0usize; groups],
    };
    if let Some(parent) = g.group_parent.as_ref() {
        for gr in 0..groups {
            let mut p = gr as i32;
            while p >= 0 {
                arranger.depth[gr] += 1;
                p = parent[p as usize];
            }
        }
    }
    let has_grp = g.grp.is_some();
    if has_grp {
        let mut at = vec![0f64; n];
        for r in 0..=max_rank {
            for i in g.layer_start[r]..g.layer_start[r + 1] {
                at[g.order[i]] = (i - g.layer_start[r]) as f64;
            }
            arranger.arrange(g, r, &at);
        }
    }
    let mut best = if has_grp { counter.count(g) } else { declared };
    if best == 0.0 {
        return (0.0, declared);
    }
    let mut best_order = g.order.clone();
    // まとめた初めの順とまとめない初めの順のどちらよりも良くなることを求める。
    let start = js::min(best, declared);

    let pin_of = |v: usize| -> f64 { if v < real { nodes[v].pin } else { 0.0 } };

    // 隣の入れ替えで、重心の掃き出しが見落とした交差を消す。
    let at = |g: &Graph, e: usize, up: bool| -> f64 {
        let u = if up { g.ef[e] } else { g.et[e] };
        if g.offsets && g.w[u] > 0.0 {
            g.pos[u] as f64 + (if up { g.efd[e] } else { g.etd[e] }) / g.w[u]
        } else {
            g.pos[u] as f64
        }
    };
    // a が b の左にあるとしての a と b の交差。
    let between = |g: &Graph, a: usize, b: usize| -> i64 {
        let mut c = 0i64;
        for k in g.pred_start[a]..g.pred_start[a + 1] {
            let x = at(g, g.pred_e[k], true);
            for j in g.pred_start[b]..g.pred_start[b + 1] {
                if x > at(g, g.pred_e[j], true) {
                    c += 1;
                }
            }
        }
        for k in g.succ_start[a]..g.succ_start[a + 1] {
            let x = at(g, g.succ_e[k], false);
            for j in g.succ_start[b]..g.succ_start[b + 1] {
                if x > at(g, g.succ_e[j], false) {
                    c += 1;
                }
            }
        }
        c
    };
    // 1 つのランクのノードの組のうち、与えられた順でないものの数。
    let out_of_order = |g: &Graph| -> f64 {
        let mut total = 0f64;
        for r in 0..=max_rank {
            for i in g.layer_start[r]..g.layer_start[r + 1] {
                if g.order[i] >= real {
                    continue;
                }
                for j in (i + 1)..g.layer_start[r + 1] {
                    if g.order[j] < g.order[i] {
                        total += 1.0;
                    }
                }
            }
        }
        total
    };

    // 掃き出しは同じ始まりから 2 度、下から先と上から先に行う。交差は同じでも順が違うことが多く、そのときは
    // 与えられた順をより多く保つほうが驚きが少ない。2 度目は 1 度目と同じだけかかるので、ノードを 1 つずつ
    // 読める小さなグラフだけ。
    let initial = g.order.clone();
    let small = n <= 600;
    let mut chosen = best_order.clone();
    let mut chosen_out = f64::INFINITY;
    for attempt in 0..(if small { 2 } else { 1 }) {
        if attempt > 0 {
            g.order.copy_from_slice(&initial);
            settle_positions(g);
        }
        let mut found = start;
        let mut found_order = initial.clone();
        let mut i = 0usize;
        let mut stale = 0;
        while stale < 4 && i < MAX_SWEEPS {
            sweep(
                g,
                nodes,
                (i + attempt).is_multiple_of(2),
                i % 4 >= 2,
                &mut bary,
                &mut moving,
                &mut spare,
                &mut arranger,
            );
            let c = counter.count(g);
            if c < found {
                found = c;
                found_order.copy_from_slice(&g.order);
                stale = 0;
                if c == 0.0 {
                    break;
                }
            }
            i += 1;
            stale += 1;
        }
        g.order.copy_from_slice(&found_order);
        if small && found > 0.0 {
            settle_positions(g);
            // transpose
            for _pass in 0..8 {
                let mut improved = false;
                for r in 0..=max_rank {
                    if g.fixed[r] != 0 {
                        continue;
                    }
                    let s = g.layer_start[r];
                    let mut i = s;
                    while i + 1 < g.layer_start[r + 1] {
                        let a = g.order[i];
                        let b = g.order[i + 1];
                        let skip = pin_of(a) != pin_of(b)
                            || g.grp.as_ref().is_some_and(|grp| grp[a] != grp[b]);
                        if !skip && between(g, b, a) < between(g, a, b) {
                            g.order[i] = b;
                            g.order[i + 1] = a;
                            g.pos[b] = (i - s) as i32;
                            g.pos[a] = (i + 1 - s) as i32;
                            improved = true;
                        }
                        i += 1;
                    }
                }
                if !improved {
                    break;
                }
            }
            let c = counter.count(g);
            if c < found {
                found = c;
                found_order.copy_from_slice(&g.order);
            }
        }
        let out = if small { out_of_order(g) } else { 0.0 };
        if attempt == 0 || found < best || (found == best && out < chosen_out) {
            best = found;
            chosen = found_order;
            chosen_out = out;
        }
    }
    g.order.copy_from_slice(&chosen);
    settle_positions(g);
    best_order.copy_from_slice(&chosen);
    // まとめた順はまとめない閾値を超えうるので数え直す。
    if has_grp {
        best = counter.count(g);
    }

    // 絵とその鏡像の交差も同じ。その 2 つのうちでも同じように選ぶ。
    if !small {
        return (best, declared);
    }
    for i in 0..real {
        if nodes[i].pin != 0.0 {
            return (best, declared);
        }
    }
    if chosen_out == 0.0 || g.fixed[0] != 0 || g.fixed[max_rank] != 0 {
        return (best, declared);
    }
    for r in 0..=max_rank {
        let (s, e) = (g.layer_start[r], g.layer_start[r + 1]);
        g.order[s..e].reverse();
    }
    settle_positions(g);
    if counter.count(g) > best || out_of_order(g) >= chosen_out {
        g.order.copy_from_slice(&best_order);
    }
    (best, declared)
}

fn sweep(
    g: &mut Graph,
    nodes: &[LNode],
    down: bool,
    bias_right: bool,
    bary: &mut [f64],
    moving: &mut [usize],
    spare: &mut [usize],
    arranger: &mut Arranger,
) {
    let real = g.real;
    let max_rank = g.max_rank;
    let has_grp = g.grp.is_some();
    for step in 1..=max_rank {
        let r = if down { step } else { max_rank - step };
        if g.fixed[r] != 0 {
            continue;
        }
        let s = g.layer_start[r];
        let e = g.layer_start[r + 1];
        let mut movable = 0usize;
        for i in s..e {
            let v = g.order[i];
            let pin = if v < real { nodes[v].pin } else { 0.0 };
            if pin != 0.0 {
                bary[v] = pin * 1e9;
                moving[movable] = v;
                movable += 1;
                continue;
            }
            let (start, list, other, other_dx) = if down {
                (&g.pred_start, &g.pred_e, &g.ef, &g.efd)
            } else {
                (&g.succ_start, &g.succ_e, &g.et, &g.etd)
            };
            let from = start[v];
            let to = start[v + 1];
            if from == to {
                if has_grp {
                    // 隣のランクに相手がなければ、相対の位置を保つ。
                    let across = if down {
                        g.layer_start[r] - g.layer_start[r - 1]
                    } else {
                        g.layer_start[r + 2] - g.layer_start[r + 1]
                    };
                    bary[v] = ((i - s) as f64 * across as f64) / (e - s) as f64;
                    continue;
                }
                bary[v] = -1.0;
                continue;
            }
            let mut sum = 0f64;
            for k in from..to {
                let u = other[list[k]];
                sum += g.pos[u] as f64;
                if g.offsets && g.w[u] > 0.0 {
                    sum += other_dx[list[k]] / g.w[u];
                }
            }
            bary[v] = sum / (to - from) as f64;
            moving[movable] = v;
            movable += 1;
        }
        if has_grp {
            arranger.arrange(g, r, bary);
            continue;
        }
        if movable < 2 {
            continue;
        }
        sort_by_bary(moving, movable, bary, &g.pos, bias_right, spare);
        let mut k = 0;
        for i in s..e {
            if bary[g.order[i]] != -1.0 {
                g.order[i] = moving[k];
                k += 1;
            }
        }
        for i in s..e {
            g.pos[g.order[i]] = (i - s) as i32;
        }
    }
}

// ノード・曲がり点・ポートが流れを横切って取る場所の半分（隣との隙間の分け前を含む）。グループの境のポートは
// 辺が通る点で、曲がり点と同じだけしか要らない。
fn half_room(g: &Graph, opt: &LayeredOptions, v: usize) -> f64 {
    g.w[v] / 2.0
        + (if g.kind[v] == K_NODE {
            opt.node_sep
        } else {
            opt.edge_sep
        }) / 2.0
}

// Brandes-Köpf の横の座標: 4 つのそろえた配置の釣り合い。
fn position(g: &mut Graph, opt: &LayeredOptions) -> Vec<f64> {
    let n = g.n;
    let max_rank = g.max_rank;
    let segments = g.ef.len();
    let mut gap = vec![0f64; n];
    for i in 0..n {
        gap[i] = half_room(g, opt, i);
    }

    // 隣を左から右の順に並べ、中央値をそのまま読めるように。
    let mut key = vec![0i32; segments];
    for e in 0..segments {
        key[e] = g.pos[g.et[e]];
    }
    for v in 0..n {
        if g.succ_start[v + 1] - g.succ_start[v] > 1 {
            let (a, b) = (g.succ_start[v], g.succ_start[v + 1]);
            sort_range(&mut g.succ_e, a, b, &key);
        }
    }
    for e in 0..segments {
        key[e] = g.pos[g.ef[e]];
    }
    for v in 0..n {
        if g.pred_start[v + 1] - g.pred_start[v] > 1 {
            let (a, b) = (g.pred_start[v], g.pred_start[v + 1]);
            sort_range(&mut g.pred_e, a, b, &key);
        }
    }
    let g: &Graph = g;

    // 2 つの実ノードの間の辺は、曲がり点どうしの切れ端と交差してはならない。そうなるものに印を付ける。
    let mut conflict = vec![0u8; segments];
    for r in 1..=max_rank {
        let prev_len = (g.layer_start[r] - g.layer_start[r - 1]) as i32;
        let s = g.layer_start[r];
        let end = g.layer_start[r + 1];
        let mut k0 = 0i32;
        let mut scan = s;
        for i in s..end {
            let v = g.order[i];
            let mut inner: i64 = -1;
            if is_dummy(g.kind[v]) {
                let u = g.ef[g.pred_e[g.pred_start[v]]];
                if is_dummy(g.kind[u]) {
                    inner = u as i64;
                }
            }
            let k1 = if inner >= 0 {
                g.pos[inner as usize]
            } else {
                prev_len
            };
            if inner >= 0 || i == end - 1 {
                for j in scan..=i {
                    let w = g.order[j];
                    let w_dummy = is_dummy(g.kind[w]);
                    for k in g.pred_start[w]..g.pred_start[w + 1] {
                        let e = g.pred_e[k];
                        let up = g.pos[g.ef[e]];
                        if (up < k0 || k1 < up) && !(w_dummy && is_dummy(g.kind[g.ef[e]])) {
                            conflict[e] = 1;
                        }
                    }
                }
                scan = i + 1;
                k0 = k1;
            }
        }
    }

    let mut results: Vec<Vec<f64>> = Vec::new();
    let mut root = vec![0usize; n];
    let mut align = vec![0usize; n];
    // ブロックの根からの各ノードのずれ。そろえた辺が着く所で会うように。
    let mut shift = vec![0f64; n];
    let mut e_from = vec![0usize; n];
    let mut e_to = vec![0usize; n];
    let mut e_w = vec![0f64; n];
    let mut indeg = vec![0i32; n];
    let mut out_start = vec![0usize; n + 1];
    let mut out_e = vec![0usize; n];
    let mut fill = vec![0usize; n];
    let mut topo = vec![0usize; n];
    let mut x = vec![0f64; n];

    for variant in 0..4 {
        let up = variant < 2;
        let right = (variant & 1) == 1;
        let (start, list, other, own_dx, other_dx) = if up {
            (&g.pred_start, &g.pred_e, &g.ef, &g.etd, &g.efd)
        } else {
            (&g.succ_start, &g.succ_e, &g.et, &g.efd, &g.etd)
        };
        for i in 0..n {
            root[i] = i;
            align[i] = i;
            shift[i] = 0.0;
            indeg[i] = 0;
            out_start[i + 1] = 0;
            x[i] = 0.0;
        }

        for step in 0..=max_rank {
            let r = if up { step } else { max_rank - step };
            let s = g.layer_start[r];
            let end = g.layer_start[r + 1];
            let adjacent: i64 = if up { r as i64 - 1 } else { r as i64 + 1 };
            let adjacent_last: i64 = if adjacent >= 0 && adjacent <= max_rank as i64 {
                g.layer_start[adjacent as usize + 1] as i64
                    - g.layer_start[adjacent as usize] as i64
                    - 1
            } else {
                0
            };
            let mut prev_idx: i64 = -1;
            let count = end - s;
            for c in 0..count {
                let i = if right { end - 1 - c } else { s + c };
                let v = g.order[i];
                let from = start[v];
                let d = start[v + 1] - from;
                if d == 0 {
                    continue;
                }
                let lo = from + ((d - 1) >> 1);
                let hi = from + (d >> 1);
                for pass in 0..2 {
                    if pass == 1 && lo == hi {
                        break;
                    }
                    let e = list[if (pass == 0) != right { lo } else { hi }];
                    let w = other[e];
                    let idx = if right {
                        adjacent_last - g.pos[w] as i64
                    } else {
                        g.pos[w] as i64
                    };
                    if align[v] == v && prev_idx < idx && conflict[e] == 0 {
                        align[w] = v;
                        root[v] = root[w];
                        align[v] = root[v];
                        shift[v] = shift[w] + other_dx[e] - own_dx[e];
                        prev_idx = idx;
                    }
                }
            }
        }

        let mut block_edges = 0usize;
        for r in 0..=max_rank {
            let s = g.layer_start[r];
            let end = g.layer_start[r + 1];
            for i in (s + 1)..end {
                let a = if right { g.order[i] } else { g.order[i - 1] };
                let b = if right { g.order[i - 1] } else { g.order[i] };
                let sep = gap[a] + gap[b];
                e_from[block_edges] = root[a];
                e_to[block_edges] = root[b];
                e_w[block_edges] = if right {
                    sep - shift[a] + shift[b]
                } else {
                    sep + shift[a] - shift[b]
                };
                indeg[root[b]] += 1;
                out_start[root[a] + 1] += 1;
                block_edges += 1;
            }
        }
        for i in 0..n {
            out_start[i + 1] += out_start[i];
        }
        fill[..n].copy_from_slice(&out_start[..n]);
        for e in 0..block_edges {
            out_e[fill[e_from[e]]] = e;
            fill[e_from[e]] += 1;
        }

        let mut count = 0usize;
        for i in 0..n {
            if root[i] == i && indeg[i] == 0 {
                topo[count] = i;
                count += 1;
            }
        }
        let mut q = 0;
        while q < count {
            let v = topo[q];
            for k in out_start[v]..out_start[v + 1] {
                let e = out_e[k];
                let w = e_to[e];
                if x[v] + e_w[e] > x[w] {
                    x[w] = x[v] + e_w[e];
                }
                indeg[w] -= 1;
                if indeg[w] == 0 {
                    topo[count] = w;
                    count += 1;
                }
            }
            q += 1;
        }
        for q in (0..count).rev() {
            let v = topo[q];
            let mut min = f64::INFINITY;
            for k in out_start[v]..out_start[v + 1] {
                let e = out_e[k];
                let cand = x[e_to[e]] - e_w[e];
                if cand < min {
                    min = cand;
                }
            }
            if min != f64::INFINITY && min > x[v] {
                x[v] = min;
            }
        }
        let mut xs = vec![0f64; n];
        for i in 0..n {
            xs[i] = (if right { -x[root[i]] } else { x[root[i]] }) + shift[i];
        }
        results.push(xs);
    }

    let mut narrowest = 0usize;
    let mut best_width = f64::INFINITY;
    let mut mins: Vec<f64> = Vec::new();
    let mut maxs: Vec<f64> = Vec::new();
    for k in 0..4 {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let xs = &results[k];
        for i in 0..n {
            let half = g.w[i] / 2.0;
            if xs[i] - half < lo {
                lo = xs[i] - half;
            }
            if xs[i] + half > hi {
                hi = xs[i] + half;
            }
        }
        mins.push(lo);
        maxs.push(hi);
        if hi - lo < best_width {
            best_width = hi - lo;
            narrowest = k;
        }
    }
    for k in 0..4 {
        let delta = if (k & 1) == 0 {
            mins[narrowest] - mins[k]
        } else {
            maxs[narrowest] - maxs[k]
        };
        if delta != 0.0 {
            for v in results[k].iter_mut() {
                *v += delta;
            }
        }
    }

    let mut out = vec![0f64; n];
    for i in 0..n {
        let mut a = results[0][i];
        let mut b = results[1][i];
        let mut c = results[2][i];
        let mut d = results[3][i];
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if c > d {
            std::mem::swap(&mut c, &mut d);
        }
        if a > c {
            std::mem::swap(&mut a, &mut c);
        }
        if b > d {
            std::mem::swap(&mut b, &mut d);
        }
        if b > c {
            std::mem::swap(&mut b, &mut c);
        }
        let _ = (a, d);
        out[i] = (b + c) / 2.0;
    }
    out
}
