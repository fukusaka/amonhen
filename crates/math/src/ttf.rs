//! KaTeX の TrueType のフォントから、字形の輪郭を SVG の `d` にする小さな読み手。読むのは同梱の KaTeX のフォント
//! だけなので、要るものだけを読む: `head`（em の大きさと `loca` の形）・`cmap`（形式 4）・`loca`・`glyf`（単純な
//! 字形と、ほかの字形を組み合わせた字形）。読めない所は字形なしとして扱い、panic しない（範囲は毎回確かめる）。

/// 読んだフォント（中身は借りたまま）。
pub struct Font {
    data: &'static [u8],
    units_per_em: u16,
    long_loca: bool,
    loca: (usize, usize),
    glyf: (usize, usize),
    cmap4: Option<usize>,
}

/// 字形の輪郭（フォントの単位、y は上向き）と外枠。
pub struct Outline {
    /// SVG の `d`（数はフォントの単位。整数でないものは小数 1 桁）。
    pub d: String,
    /// 外枠 (x_min, y_min, x_max, y_max)。
    pub bbox: (f64, f64, f64, f64),
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    u16_at(data, at).map(|v| v as i16)
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// 組み合わせた字形をたどる深さの上限。
const MAX_COMPOSITE_DEPTH: u32 = 8;

impl Font {
    pub fn parse(data: &'static [u8]) -> Option<Font> {
        let tables = u16_at(data, 4)? as usize;
        let find = |tag: &[u8; 4]| -> Option<(usize, usize)> {
            (0..tables).find_map(|i| {
                let at = 12 + i * 16;
                (data.get(at..at + 4)? == tag).then_some(())?;
                let offset = u32_at(data, at + 8)? as usize;
                let length = u32_at(data, at + 12)? as usize;
                (offset.checked_add(length)? <= data.len()).then_some((offset, length))
            })
        };
        let head = find(b"head")?;
        let units_per_em = u16_at(data, head.0 + 18).filter(|&u| u > 0)?;
        let long_loca = i16_at(data, head.0 + 50)? == 1;
        let loca = find(b"loca")?;
        let glyf = find(b"glyf")?;
        let cmap4 = find(b"cmap").and_then(|(cmap, _)| {
            let count = u16_at(data, cmap + 2)? as usize;
            // Windows の Unicode（3, 1）を先に、なければ Unicode（0, *）。
            let mut best = None;
            for i in 0..count {
                let at = cmap + 4 + i * 8;
                let platform = u16_at(data, at)?;
                let encoding = u16_at(data, at + 2)?;
                let sub = cmap + u32_at(data, at + 4)? as usize;
                if u16_at(data, sub)? != 4 {
                    continue;
                }
                if (platform, encoding) == (3, 1) {
                    return Some(sub);
                }
                if platform == 0 {
                    best = Some(sub);
                }
            }
            best
        });
        Some(Font {
            data,
            units_per_em,
            long_loca,
            loca,
            glyf,
            cmap4,
        })
    }

    pub fn units_per_em(&self) -> f64 {
        f64::from(self.units_per_em)
    }

    /// 文字の字形の番号（なければ None。0 の「字形なし」も None）。
    pub fn glyph_id(&self, ch: char) -> Option<u16> {
        let code = u32::from(ch);
        if code > 0xFFFF {
            return None;
        }
        let code = code as u16;
        let data = self.data;
        let sub = self.cmap4?;
        let segments = (u16_at(data, sub + 6)? / 2) as usize;
        let ends = sub + 14;
        let starts = ends + segments * 2 + 2;
        let deltas = starts + segments * 2;
        let ranges = deltas + segments * 2;
        for i in 0..segments {
            let end = u16_at(data, ends + i * 2)?;
            if code > end {
                continue;
            }
            let start = u16_at(data, starts + i * 2)?;
            if code < start {
                return None;
            }
            let delta = u16_at(data, deltas + i * 2)?;
            let range_at = ranges + i * 2;
            let range = u16_at(data, range_at)?;
            let id = if range == 0 {
                code.wrapping_add(delta)
            } else {
                let at = range_at + range as usize + (code - start) as usize * 2;
                let raw = u16_at(data, at)?;
                if raw == 0 {
                    return None;
                }
                raw.wrapping_add(delta)
            };
            return (id != 0).then_some(id);
        }
        None
    }

    /// 字形の `glyf` の中の範囲（中身のない字形は None）。
    fn glyph_range(&self, id: u16) -> Option<(usize, usize)> {
        let id = id as usize;
        let size = if self.long_loca { 4 } else { 2 };
        if (id + 2) * size > self.loca.1 {
            return None;
        }
        let (start, end) = if self.long_loca {
            (
                u32_at(self.data, self.loca.0 + id * 4)? as usize,
                u32_at(self.data, self.loca.0 + id * 4 + 4)? as usize,
            )
        } else {
            (
                u16_at(self.data, self.loca.0 + id * 2)? as usize * 2,
                u16_at(self.data, self.loca.0 + id * 2 + 2)? as usize * 2,
            )
        };
        (start < end && end <= self.glyf.1).then_some((self.glyf.0 + start, self.glyf.0 + end))
    }

    /// 字形の輪郭。中身のない字形（空白）は空の `d`。
    pub fn outline(&self, id: u16) -> Option<Outline> {
        let mut out = Path::default();
        self.append(id, Transform::IDENTITY, &mut out, 0)?;
        Some(Outline {
            d: out.d,
            bbox: out.bbox.unwrap_or((0.0, 0.0, 0.0, 0.0)),
        })
    }

    fn append(&self, id: u16, t: Transform, out: &mut Path, depth: u32) -> Option<()> {
        if depth > MAX_COMPOSITE_DEPTH {
            return None;
        }
        let Some((start, end)) = self.glyph_range(id) else {
            return Some(());
        };
        let g = &self.data[start..end];
        let contours = i16_at(g, 0)?;
        if contours >= 0 {
            simple(g, contours as usize, t, out)
        } else {
            self.composite(g, t, out, depth)
        }
    }

    fn composite(&self, g: &[u8], t: Transform, out: &mut Path, depth: u32) -> Option<()> {
        const WORDS: u16 = 0x0001;
        const XY_VALUES: u16 = 0x0002;
        const SCALE: u16 = 0x0008;
        const MORE: u16 = 0x0020;
        const XY_SCALE: u16 = 0x0040;
        const TWO_BY_TWO: u16 = 0x0080;
        let mut at = 10;
        loop {
            let flags = u16_at(g, at)?;
            let glyph = u16_at(g, at + 2)?;
            at += 4;
            let (dx, dy) = if flags & WORDS != 0 {
                let v = (f64::from(i16_at(g, at)?), f64::from(i16_at(g, at + 2)?));
                at += 4;
                v
            } else {
                let v = (
                    f64::from(*g.get(at)? as i8),
                    f64::from(*g.get(at + 1)? as i8),
                );
                at += 2;
                v
            };
            let f2dot14 = |at: usize| i16_at(g, at).map(|v| f64::from(v) / 16384.0);
            let (mut a, mut b, mut c, mut d) = (1.0, 0.0, 0.0, 1.0);
            if flags & SCALE != 0 {
                a = f2dot14(at)?;
                d = a;
                at += 2;
            } else if flags & XY_SCALE != 0 {
                a = f2dot14(at)?;
                d = f2dot14(at + 2)?;
                at += 4;
            } else if flags & TWO_BY_TWO != 0 {
                a = f2dot14(at)?;
                b = f2dot14(at + 2)?;
                c = f2dot14(at + 4)?;
                d = f2dot14(at + 6)?;
                at += 8;
            }
            // 点の番号で合わせる形（ARGS_ARE_XY_VALUES なし）は KaTeX のフォントにないので、ずらさずに置く。
            let (dx, dy) = if flags & XY_VALUES != 0 {
                (dx, dy)
            } else {
                (0.0, 0.0)
            };
            let inner = Transform { a, b, c, d, dx, dy };
            self.append(glyph, t.then(inner), out, depth + 1)?;
            if flags & MORE == 0 {
                return Some(());
            }
        }
    }
}

/// 2 次元の変換（x' = a·x + c·y + dx、y' = b·x + d·y + dy）。
#[derive(Clone, Copy)]
struct Transform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    dx: f64,
    dy: f64,
}

impl Transform {
    const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        dx: 0.0,
        dy: 0.0,
    };

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.dx,
            self.b * x + self.d * y + self.dy,
        )
    }

    /// `inner` を当ててから `self` を当てる変換。
    fn then(&self, inner: Transform) -> Transform {
        let (dx, dy) = self.apply(inner.dx, inner.dy);
        Transform {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            dx,
            dy,
        }
    }
}

#[derive(Default)]
struct Path {
    d: String,
    bbox: Option<(f64, f64, f64, f64)>,
}

impl Path {
    fn point(&mut self, (x, y): (f64, f64)) {
        if !self.d.is_empty() && !self.d.ends_with(['M', 'L', 'Q']) {
            self.d.push(' ');
        }
        crate::svg::num(&mut self.d, x);
        self.d.push(' ');
        crate::svg::num(&mut self.d, y);
        self.bbox = Some(match self.bbox {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
    }

    fn command(&mut self, c: char) {
        if !self.d.is_empty() && !self.d.ends_with(' ') {
            self.d.push(' ');
        }
        self.d.push(c);
    }
}

/// 単純な字形（輪郭の点の並び）を `out` に足す。
fn simple(g: &[u8], contours: usize, t: Transform, out: &mut Path) -> Option<()> {
    const ON_CURVE: u8 = 0x01;
    const X_SHORT: u8 = 0x02;
    const Y_SHORT: u8 = 0x04;
    const REPEAT: u8 = 0x08;
    const X_SAME: u8 = 0x10;
    const Y_SAME: u8 = 0x20;
    let mut ends = Vec::with_capacity(contours);
    for i in 0..contours {
        ends.push(u16_at(g, 10 + i * 2)? as usize);
    }
    let Some(&last) = ends.last() else {
        return Some(());
    };
    let points = last + 1;
    // 点の数は 16 ビットの番号に収まる
    if points > 0xFFFF || ends.windows(2).any(|w| w[1] < w[0]) {
        return None;
    }
    let instructions = u16_at(g, 10 + contours * 2)? as usize;
    let mut at = 12 + contours * 2 + instructions;
    let mut flags = Vec::with_capacity(points);
    while flags.len() < points {
        let flag = *g.get(at)?;
        at += 1;
        flags.push(flag);
        if flag & REPEAT != 0 {
            let count = *g.get(at)?;
            at += 1;
            for _ in 0..count {
                flags.push(flag);
            }
        }
    }
    flags.truncate(points);
    let mut read = |short: u8, same: u8| -> Option<Vec<i32>> {
        let mut v = 0i32;
        let mut out = Vec::with_capacity(points);
        for &flag in &flags {
            if flag & short != 0 {
                let delta = i32::from(*g.get(at)?);
                at += 1;
                v += if flag & same != 0 { delta } else { -delta };
            } else if flag & same == 0 {
                v += i32::from(i16_at(g, at)?);
                at += 2;
            }
            out.push(v);
        }
        Some(out)
    };
    let xs = read(X_SHORT, X_SAME)?;
    let ys = read(Y_SHORT, Y_SAME)?;
    let mut begin = 0;
    for &end in &ends {
        let pts: Vec<((f64, f64), bool)> = (begin..=end)
            .map(|i| {
                (
                    t.apply(f64::from(xs[i]), f64::from(ys[i])),
                    flags[i] & ON_CURVE != 0,
                )
            })
            .collect();
        begin = end + 1;
        contour(&pts, out);
    }
    Some(())
}

/// 2 次ベジェの輪郭 1 つを `M … Q … Z` にする（続く制御点の間は、その中点を曲線上の点とみる）。
fn contour(pts: &[((f64, f64), bool)], out: &mut Path) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    // 曲線上の点から始める（なければ最初の 2 つの制御点の中点）。
    let first_on = pts.iter().position(|p| p.1);
    let (start, offset) = match first_on {
        Some(i) => (pts[i].0, i),
        None => (mid(pts[0].0, pts[1].0), 0),
    };
    out.command('M');
    out.point(start);
    let mut control: Option<(f64, f64)> = None;
    let steps = if first_on.is_some() { n } else { n + 1 };
    for k in 1..=steps {
        let (p, on) = pts[(offset + k) % n];
        let p_is_start = k == steps;
        match (on || p_is_start && first_on.is_none(), control) {
            (true, None) => {
                let target = if p_is_start { start } else { p };
                out.command('L');
                out.point(target);
            }
            (true, Some(c)) => {
                let target = if p_is_start { start } else { p };
                out.command('Q');
                out.point(c);
                out.point(target);
                control = None;
            }
            (false, None) => control = Some(p),
            (false, Some(c)) => {
                let m = mid(c, p);
                out.command('Q');
                out.point(c);
                out.point(m);
                control = Some(p);
            }
        }
    }
    out.command('Z');
}
