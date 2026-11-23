//! venn の円の置き方（pele の `src/diagrams/venn/layout.ts`）。

use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use super::db::sort16;
use super::types::VennSubset;
use crate::js;
use crate::util::fdlibm;

#[derive(Clone, Debug, PartialEq)]
pub struct Circle {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub r: f64,
}

/// 領域の中身を置く所: `inside` のすべての中で、ほかのどの円の外でもあり、それらの輪郭からいちばん遠い点と、
/// その遠さ。0 以下なら、置いた円ではその領域は空。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub x: f64,
    pub y: f64,
    pub margin: f64,
}

const TAU: f64 = PI * 2.0;

/// 3 つ以上の集合の重なりごとに、それが含み図が述べていない 2 つずつの重なりを、小さい方の集合の 4 分の 1 ずつで
/// 足す（円が重なって描かれるように）。足すものがなければ受けた並びを返す。
pub fn ensure_pairwise_subsets(subsets: &[VennSubset]) -> Vec<VennSubset> {
    let mut present: HashSet<Vec<String>> = HashSet::new();
    let mut sizes: HashMap<&str, f64> = HashMap::new();
    for subset in subsets {
        let mut key = subset.sets.clone();
        sort16(&mut key);
        present.insert(key);
        if subset.sets.len() == 1 {
            sizes.insert(&subset.sets[0], subset.size);
        }
    }
    let mut added: Vec<VennSubset> = Vec::new();
    for subset in subsets {
        if subset.sets.len() < 3 {
            continue;
        }
        let mut members = subset.sets.clone();
        sort16(&mut members);
        for i in 0..members.len() - 1 {
            for j in i + 1..members.len() {
                let pair = vec![members[i].clone(), members[j].clone()];
                if present.contains(&pair) {
                    continue;
                }
                present.insert(pair.clone());
                let a = sizes.get(pair[0].as_str());
                let b = sizes.get(pair[1].as_str());
                let size = match (a, b) {
                    (Some(&a), Some(&b)) => js::min(a, b) / 4.0,
                    _ => 2.5,
                };
                added.push(VennSubset {
                    sets: pair,
                    size,
                    label: Some(String::new()),
                });
            }
        }
    }
    let mut out = subsets.to_vec();
    out.extend(added);
    out
}

/// 中心が d 離れた 2 つの円が分け合う広さ。
pub fn lens_area(r1: f64, r2: f64, d: f64) -> f64 {
    if d >= r1 + r2 {
        return 0.0;
    }
    let small = js::min(r1, r2);
    if d <= (r1 - r2).abs() {
        return PI * small * small;
    }
    let a1 = r1 * r1 * fdlibm::acos((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1));
    let a2 = r2 * r2 * fdlibm::acos((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2));
    let a3 = 0.5
        * js::max(
            (-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2),
            0.0,
        )
        .sqrt();
    a1 + a2 - a3
}

/// 重なりが決まった広さになる 2 つの円の離れ方。離れるほど広さは減るので、二分法で求める。
pub fn distance_for_overlap(r1: f64, r2: f64, area: f64) -> f64 {
    let mut low = (r1 - r2).abs();
    let mut high = r1 + r2;
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(area > 0.0) {
        return high;
    }
    if area >= PI * js::min(r1, r2).powi(2) {
        return low;
    }
    for _ in 0..48 {
        let middle = (low + high) / 2.0;
        if lens_area(r1, r2, middle) > area {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) / 2.0
}

/// 点が、中にいるべき・外にいるべきいちばん近い輪郭からどれだけ離れているか。`floor` に届かないと分かったら、
/// `floor` より小さい値ですぐやめる。
fn margin(
    circles: &[Circle],
    inside: &[usize],
    outside: &[usize],
    x: f64,
    y: f64,
    floor: f64,
) -> f64 {
    let mut best = f64::INFINITY;
    for &i in inside {
        let c = &circles[i];
        let dx = x - c.x;
        let dy = y - c.y;
        let room = c.r - (dx * dx + dy * dy).sqrt();
        if room < best {
            if room < floor {
                return room;
            }
            best = room;
        }
    }
    for &i in outside {
        let c = &circles[i];
        let dx = x - c.x;
        let dy = y - c.y;
        let room = (dx * dx + dy * dy).sqrt() - c.r;
        if room < best {
            if room < floor {
                return room;
            }
            best = room;
        }
    }
    best
}

/// 領域のいちばん広い点を、いちばん小さい円を格子で試し、いちばん良い所から坂を登って探す。同じなら領域の
/// 真ん中に近い点を取る（対称な配置に対称なラベルを置くため）。
pub fn find_spot(circles: &[Circle], inside: &[usize], outside: &[usize]) -> Spot {
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut smallest = &circles[inside[0]];
    let count = inside.len() as f64;
    for &i in inside {
        let c = &circles[i];
        cx += c.x / count;
        cy += c.y / count;
        if c.r < smallest.r {
            smallest = c;
        }
    }
    let r = smallest.r;
    let (sx, sy) = (smallest.x, smallest.y);
    let mut x = cx;
    let mut y = cy;
    let mut best = margin(circles, inside, outside, x, y, f64::NEG_INFINITY);
    let mut off = 0.0;
    let mut consider = |px: f64, py: f64, best: &mut f64, x: &mut f64, y: &mut f64| -> bool {
        let m = margin(circles, inside, outside, px, py, *best - r * 1e-9);
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(m > *best - r * 1e-9) {
            return false;
        }
        let distance = js::hypot(px - cx, py - cy);
        if m > *best + r * 1e-9 || distance < off - r * 1e-9 {
            *best = m;
            off = distance;
            *x = px;
            *y = py;
            return true;
        }
        false
    };
    const GRID: i32 = 10;
    let grid = GRID as f64;
    for gy in -GRID..=GRID {
        for gx in -GRID..=GRID {
            if gx * gx + gy * gy <= GRID * GRID {
                consider(
                    sx + (gx as f64 * r) / grid,
                    sy + (gy as f64 * r) / grid,
                    &mut best,
                    &mut x,
                    &mut y,
                );
            }
        }
    }
    let mut step = r / grid;
    let mut round = 0;
    while round < 40 && step > r * 1e-6 {
        let mut moved = false;
        for k in 0..8 {
            let angle = (k as f64 * TAU) / 8.0;
            let (px, py) = (x + fdlibm::cos(angle) * step, y + fdlibm::sin(angle) * step);
            if consider(px, py, &mut best, &mut x, &mut y) {
                moved = true;
            }
        }
        if !moved {
            step /= 2.0;
        }
        round += 1;
    }
    Spot { x, y, margin: best }
}

/// 集合ごとに 1 つの円を、広さを集合の大きさに比べて、2 つずつを重なりが求めるだけ離して置く: 1〜3 つの集合は
/// そのとおりに、それより多ければ円の輪をその離れ方へ緩めて。長さの単位は大きさの平方根。
pub fn layout_circles(subsets: &[VennSubset]) -> Vec<Circle> {
    let mut sizes: Vec<(String, f64)> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for subset in subsets {
        for id in &subset.sets {
            if !at.contains_key(id) {
                at.insert(id.clone(), sizes.len());
                sizes.push((id.clone(), 10.0));
            }
        }
    }
    for subset in subsets {
        if subset.sets.len() == 1 {
            sizes[at[&subset.sets[0]]].1 = subset.size;
        }
    }

    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut circles: Vec<Circle> = Vec::new();
    let mut largest = 0.0;
    for &(_, size) in &sizes {
        if size.is_finite() && size > largest {
            largest = size;
        }
    }
    if largest == 0.0 {
        largest = 10.0;
    }
    let widest = (largest / PI).sqrt();
    for (id, size) in &sizes {
        index.insert(id, circles.len());
        // 名前が入らないほど小さい集合や、大きさのない集合にも、見える円を描く。
        let r = if size.is_finite() && *size > 0.0 {
            (size / PI).sqrt()
        } else {
            0.0
        };
        circles.push(Circle {
            id: id.clone(),
            x: 0.0,
            y: 0.0,
            r: js::max(r, widest * 0.3),
        });
    }
    let n = circles.len();
    if n < 2 {
        return circles;
    }

    // 2 つずつの重なり（円の番号で）。
    let mut overlaps: HashMap<usize, f64> = HashMap::new();
    for subset in ensure_pairwise_subsets(subsets) {
        if subset.sets.len() != 2 {
            continue;
        }
        let a = index[subset.sets[0].as_str()];
        let b = index[subset.sets[1].as_str()];
        if a != b {
            overlaps.insert(a.min(b) * n + a.max(b), subset.size);
        }
    }
    // 重なる 2 つの離れ方。重ならない 2 つは隙間を保つだけ。
    let mut wanted: HashMap<usize, f64> = HashMap::new();
    for (&key, &area) in &overlaps {
        wanted.insert(
            key,
            distance_for_overlap(circles[key / n].r, circles[key % n].r, area),
        );
    }
    let gap = widest * 0.15;
    let distance = |circles: &[Circle], a: usize, b: usize| -> f64 {
        wanted
            .get(&(a * n + b))
            .copied()
            .unwrap_or(circles[a].r + circles[b].r + gap)
    };

    if n == 2 {
        circles[1].x = distance(&circles, 0, 1);
        return circles;
    }

    if n == 3 {
        // 2 つを並べ、3 つ目を下に: 辺が 3 つの離れ方の三角形の角。3 つの重なりがあるなら、それができるまで縮める。
        let mut ab = distance(&circles, 0, 1);
        let mut ac = distance(&circles, 0, 2);
        let mut bc = distance(&circles, 1, 2);
        let triple = subsets.iter().any(|s| {
            let mut seen: Vec<&String> = Vec::new();
            for id in &s.sets {
                if !seen.contains(&id) {
                    seen.push(id);
                }
            }
            seen.len() == 3
        });
        for _ in 0..40 {
            let x = if ab > 0.0 {
                (ab * ab + ac * ac - bc * bc) / (2.0 * ab)
            } else {
                0.0
            };
            circles[1].x = ab;
            circles[2].x = js::max(-ac, js::min(ac, x));
            circles[2].y = js::max(ac * ac - x * x, 0.0).sqrt();
            if !triple || find_spot(&circles, &[0, 1, 2], &[]).margin > widest * 0.12 {
                break;
            }
            ab *= 0.94;
            ac *= 0.94;
            bc *= 0.94;
        }
        return circles;
    }

    let mut total = 0.0;
    for c in &circles {
        total += 2.0 * c.r + gap;
    }
    let rounds = js::min(300.0, (3e6 / (n * n) as f64).floor()) as usize;
    if n <= 64 {
        let ring = total / TAU;
        for (i, c) in circles.iter_mut().enumerate() {
            let angle = (i as f64 * TAU) / n as f64 - PI / 2.0;
            c.x = fdlibm::cos(angle) * ring;
            c.y = fdlibm::sin(angle) * ring;
        }
    } else {
        let columns = (n as f64).sqrt().ceil() as usize;
        let cell = 2.0 * widest + gap;
        for (i, c) in circles.iter_mut().enumerate() {
            c.x = (i % columns) as f64 * cell;
            c.y = (i / columns) as f64 * cell;
        }
    }
    let mut dx = vec![0.0f64; n];
    let mut dy = vec![0.0f64; n];
    let mut pulls = vec![0i32; n];
    for _ in 0..rounds {
        dx.fill(0.0);
        dy.fill(0.0);
        pulls.fill(0);
        for a in 0..n {
            for b in a + 1..n {
                let overlap = wanted.get(&(a * n + b)).copied();
                let apart = circles[a].r + circles[b].r + gap;
                let mut ux = circles[b].x - circles[a].x;
                let mut uy = circles[b].y - circles[a].y;
                let mut length = js::hypot(ux, uy);
                if overlap.is_none() && length >= apart {
                    continue;
                }
                if length < 1e-9 {
                    ux = fdlibm::cos((a + b) as f64);
                    uy = fdlibm::sin((a + b) as f64);
                    length = 1.0;
                }
                let mv = (length - overlap.unwrap_or(apart)) / 2.0 / length;
                dx[a] += ux * mv;
                dy[a] += uy * mv;
                dx[b] -= ux * mv;
                dy[b] -= uy * mv;
                pulls[a] += 1;
                pulls[b] += 1;
            }
        }
        for i in 0..n {
            if pulls[i] == 0 {
                continue;
            }
            circles[i].x += dx[i] / pulls[i] as f64;
            circles[i].y += dy[i] / pulls[i] as f64;
        }
    }
    circles
}

struct Corner {
    x: f64,
    y: f64,
    a: usize,
    b: usize,
}

/// 与えた円がみな分け合う領域の輪郭の SVG のパス。分け合うものがなければ空。
pub fn region_path(circles: &[Circle], format: impl Fn(f64) -> String) -> String {
    let whole = |c: &Circle| -> String {
        format!(
            "M{},{}A{},{} 0 1 1 {},{}A{},{} 0 1 1 {},{}Z",
            format(c.x - c.r),
            format(c.y),
            format(c.r),
            format(c.r),
            format(c.x + c.r),
            format(c.y),
            format(c.r),
            format(c.r),
            format(c.x - c.r),
            format(c.y)
        )
    };
    if circles.len() == 1 {
        return whole(&circles[0]);
    }
    let within = |x: f64, y: f64, skip: usize, other: usize| -> bool {
        for (k, c) in circles.iter().enumerate() {
            if k != skip && k != other && js::hypot(x - c.x, y - c.y) > c.r * (1.0 + 1e-9) {
                return false;
            }
        }
        true
    };
    // 領域の角: 2 つの輪郭がほかのすべての円の中で交わる所。
    let mut corners: Vec<Corner> = Vec::new();
    for i in 0..circles.len() {
        for j in i + 1..circles.len() {
            let a = &circles[i];
            let b = &circles[j];
            let d = js::hypot(b.x - a.x, b.y - a.y);
            if d >= a.r + b.r || d <= (a.r - b.r).abs() || d == 0.0 {
                continue;
            }
            let along = (a.r * a.r - b.r * b.r + d * d) / (2.0 * d);
            let up = js::max(a.r * a.r - along * along, 0.0).sqrt();
            let mx = a.x + ((b.x - a.x) * along) / d;
            let my = a.y + ((b.y - a.y) * along) / d;
            for sign in [1.0, -1.0] {
                let x = mx + (sign * up * (b.y - a.y)) / d;
                let y = my - (sign * up * (b.x - a.x)) / d;
                if within(x, y, i, j) {
                    corners.push(Corner { x, y, a: i, b: j });
                }
            }
        }
    }
    if corners.len() < 2 {
        // 角がない: 1 つの円がほかのすべての中にあるか、分け合うものがない。
        let mut smallest = 0;
        for (k, c) in circles.iter().enumerate() {
            if c.r < circles[smallest].r {
                smallest = k;
            }
        }
        let s = &circles[smallest];
        for (k, c) in circles.iter().enumerate() {
            if k != smallest && js::hypot(c.x - s.x, c.y - s.y) + s.r > c.r * (1.0 + 1e-9) {
                return String::new();
            }
        }
        return whole(s);
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    let count = corners.len() as f64;
    for p in &corners {
        cx += p.x / count;
        cy += p.y / count;
    }
    corners.sort_by(|p, q| {
        js::order(fdlibm::atan2(p.y - cy, p.x - cx) - fdlibm::atan2(q.y - cy, q.x - cx))
    });
    let mut d = String::new();
    for i in 0..corners.len() {
        let from = &corners[i];
        let to = &corners[(i + 1) % corners.len()];
        if i == 0 {
            d = format!("M{},{}", format(from.x), format(from.y));
        }
        // 2 つの角の間の辺は、両方を通る円の弧。そのうち弧がほかの中にとどまるもの。
        let mut arc: Option<usize> = None;
        let mut large = 0;
        let mut narrowest = f64::INFINITY;
        for k in [from.a, from.b] {
            if k != to.a && k != to.b {
                continue;
            }
            let c = &circles[k];
            let start = fdlibm::atan2(from.y - c.y, from.x - c.x);
            let mut sweep = fdlibm::atan2(to.y - c.y, to.x - c.x) - start;
            if sweep <= 0.0 {
                sweep += TAU;
            }
            let mx = c.x + c.r * fdlibm::cos(start + sweep / 2.0);
            let my = c.y + c.r * fdlibm::sin(start + sweep / 2.0);
            let width = js::hypot(mx - (from.x + to.x) / 2.0, my - (from.y + to.y) / 2.0);
            if width < narrowest {
                narrowest = width;
                arc = Some(k);
                large = if sweep > PI { 1 } else { 0 };
            }
        }
        match arc {
            None => d.push_str(&format!("L{},{}", format(to.x), format(to.y))),
            Some(k) => {
                let r = circles[k].r;
                d.push_str(&format!(
                    "A{},{} 0 {large} 1 {},{}",
                    format(r),
                    format(r),
                    format(to.x),
                    format(to.y)
                ))
            }
        }
    }
    d + "Z"
}
