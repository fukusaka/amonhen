//! 幾何（vgc の `geometry/basic_types.dart`・`matrix.dart`・`path.dart`）。

use std::hash::{Hash, Hasher};

use super::dart;

/// 浮動小数の値を Dart の `==` と同じに比べるための鍵（-0.0 と 0.0 は同じ、NaN はどれとも違う）。
pub fn hash_f64<H: Hasher>(v: f64, state: &mut H) {
    let v = if v == 0.0 { 0.0 } else { v };
    v.to_bits().hash(state);
}

pub fn hash_opt_f64<H: Hasher>(v: Option<f64>, state: &mut H) {
    match v {
        Some(v) => {
            1u8.hash(state);
            hash_f64(v, state);
        }
        None => 0u8.hash(state),
    }
}

/// `Point`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn lerp(a: Point, b: Point, t: f64) -> Point {
        Point::new(lerp_double(a.x, b.x, t), lerp_double(a.y, b.y, t))
    }

    pub fn distance(a: Point, b: Point) -> f64 {
        let x = a.x - b.x;
        let y = a.y - b.y;
        ((x * x) + (y * y)).sqrt()
    }
}

fn lerp_double(a: f64, b: f64, t: f64) -> f64 {
    (1.0 - t) * a + t * b
}

/// `Rect`（左・上・右・下）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Rect {
    pub const LARGEST: Rect = Rect::from_ltrb(-1e9, -1e9, 1e9, 1e9);
    pub const ZERO: Rect = Rect::from_ltrb(0.0, 0.0, 0.0, 0.0);

    pub const fn from_ltrb(left: f64, top: f64, right: f64, bottom: f64) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn from_ltwh(left: f64, top: f64, width: f64, height: f64) -> Self {
        Self::from_ltrb(left, top, left + width, top + height)
    }

    pub fn from_circle(x: f64, y: f64, r: f64) -> Self {
        Self::from_ltrb(x - r, y - r, x + r, y + r)
    }

    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }
}

/// `AffineMatrix`（2D のアフィン変換と、4x4 にしたときの [2][2] の値 `m4_10`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AffineMatrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
    pub m4_10: f64,
}

impl Hash for AffineMatrix {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for v in [self.a, self.b, self.c, self.d, self.e, self.f, self.m4_10] {
            hash_f64(v, state);
        }
    }
}

impl AffineMatrix {
    pub const IDENTITY: AffineMatrix = AffineMatrix::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0);

    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, m4_10: f64) -> Self {
        Self {
            a,
            b,
            c,
            d,
            e,
            f,
            m4_10,
        }
    }

    /// `m4_10` を省いたとき（`1.0 * a`）。
    pub fn new6(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self::new(a, b, c, d, e, f, 1.0 * a)
    }

    pub fn x_scale(&self) -> f64 {
        (self.a * self.a + self.c * self.c).sqrt()
    }

    pub fn y_scale(&self) -> f64 {
        (self.b * self.b + self.d * self.d).sqrt()
    }

    pub fn scale_stroke_width(&self, width: Option<f64>) -> Option<f64> {
        let width = width?;
        if self.a == 1.0 && self.d == 1.0 {
            return Some(width);
        }
        Some((self.x_scale() + self.y_scale()) / 2.0 * width)
    }

    pub fn rotated(&self, radians: f64) -> Self {
        if radians == 0.0 {
            return *self;
        }
        let cos = radians.cos();
        let sin = radians.sin();
        Self::new(
            (self.a * cos) + (self.c * sin),
            (self.b * cos) + (self.d * sin),
            (self.a * -sin) + (self.c * cos),
            (self.b * -sin) + (self.d * cos),
            self.e,
            self.f,
            self.m4_10,
        )
    }

    pub fn encodable_in_rect(&self) -> bool {
        self.a > 0.0 && self.b == 0.0 && self.c == 0.0 && self.d > 0.0 && self.m4_10 == self.a
    }

    pub fn scaled(&self, x: f64, y: Option<f64>) -> Self {
        let y = y.unwrap_or(x);
        if x == 1.0 && y == 1.0 {
            return *self;
        }
        Self::new(
            self.a * x,
            self.b * x,
            self.c * y,
            self.d * y,
            self.e,
            self.f,
            self.m4_10 * x,
        )
    }

    pub fn translated(&self, x: f64, y: f64) -> Self {
        Self::new(
            self.a,
            self.b,
            self.c,
            self.d,
            (self.a * x) + (self.c * y) + self.e,
            (self.b * x) + (self.d * y) + self.f,
            self.m4_10,
        )
    }

    pub fn multiplied(&self, o: &AffineMatrix) -> Self {
        Self::new(
            (self.a * o.a) + (self.c * o.b),
            (self.b * o.a) + (self.d * o.b),
            (self.a * o.c) + (self.c * o.d),
            (self.b * o.c) + (self.d * o.d),
            (self.a * o.e) + (self.c * o.f) + self.e,
            (self.b * o.e) + (self.d * o.f) + self.f,
            self.m4_10 * o.m4_10,
        )
    }

    pub fn transform_point(&self, p: Point) -> Point {
        Point::new(
            (self.a * p.x) + (self.c * p.y) + self.e,
            (self.b * p.x) + (self.d * p.y) + self.f,
        )
    }

    pub fn transform_rect(&self, rect: Rect) -> Rect {
        transform_rect(&self.to_matrix4(), rect)
    }

    pub fn to_matrix4(self) -> [f64; 16] {
        [
            self.a, self.b, 0.0, 0.0, //
            self.c, self.d, 0.0, 0.0, //
            0.0, 0.0, self.m4_10, 0.0, //
            self.e, self.f, 0.0, 1.0,
        ]
    }
}

/// package:flutter の `MatrixUtils.transformRect`。
fn transform_rect(storage: &[f64; 16], rect: Rect) -> Rect {
    let x = rect.left;
    let y = rect.top;
    let w = rect.right - x;
    let h = rect.bottom - y;
    let wx = storage[0] * w;
    let hx = storage[4] * h;
    let rx = storage[0] * x + storage[4] * y + storage[12];
    let wy = storage[1] * w;
    let hy = storage[5] * h;
    let ry = storage[1] * x + storage[5] * y + storage[13];
    // storage[3]・[7] は 0、[15] は 1（アフィン）
    let mut left = rx;
    let mut right = rx;
    if wx < 0.0 {
        left += wx;
    } else {
        right += wx;
    }
    if hx < 0.0 {
        left += hx;
    } else {
        right += hx;
    }
    let mut top = ry;
    let mut bottom = ry;
    if wy < 0.0 {
        top += wy;
    } else {
        bottom += wy;
    }
    if hy < 0.0 {
        top += hy;
    } else {
        bottom += hy;
    }
    Rect::from_ltrb(left, top, right, bottom)
}

/// `PathFillType`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FillType {
    NonZero = 0,
    EvenOdd = 1,
}

/// `PathCommand`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCommand {
    Move(f64, f64),
    Line(f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
    Close,
}

impl Hash for PathCommand {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match *self {
            PathCommand::Move(x, y) => {
                0u8.hash(state);
                hash_f64(x, state);
                hash_f64(y, state);
            }
            PathCommand::Line(x, y) => {
                1u8.hash(state);
                hash_f64(x, state);
                hash_f64(y, state);
            }
            PathCommand::Cubic(a, b, c, d, e, f) => {
                2u8.hash(state);
                for v in [a, b, c, d, e, f] {
                    hash_f64(v, state);
                }
            }
            PathCommand::Close => 3u8.hash(state),
        }
    }
}

impl PathCommand {
    pub fn transformed(&self, m: &AffineMatrix) -> Self {
        match *self {
            PathCommand::Move(x, y) => {
                let p = m.transform_point(Point::new(x, y));
                PathCommand::Move(p.x, p.y)
            }
            PathCommand::Line(x, y) => {
                let p = m.transform_point(Point::new(x, y));
                PathCommand::Line(p.x, p.y)
            }
            PathCommand::Cubic(x1, y1, x2, y2, x3, y3) => {
                let p1 = m.transform_point(Point::new(x1, y1));
                let p2 = m.transform_point(Point::new(x2, y2));
                let p3 = m.transform_point(Point::new(x3, y3));
                PathCommand::Cubic(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y)
            }
            PathCommand::Close => PathCommand::Close,
        }
    }
}

fn subdivide(start: Point, c1: Point, c2: Point, end: Point, t: f64) -> [Point; 7] {
    let ab = Point::lerp(start, c1, t);
    let bc = Point::lerp(c1, c2, t);
    let cd = Point::lerp(c2, end, t);
    let abc = Point::lerp(ab, bc, t);
    let bcd = Point::lerp(bc, cd, t);
    let abcd = Point::lerp(abc, bcd, t);
    [start, ab, abc, abcd, bcd, cd, end]
}

/// `CubicToCommand.computeLength`。
fn cubic_length(start: Point, c1: Point, c2: Point, end: Point) -> f64 {
    const TOLERANCE: f64 = 1.0 / 2.0 * 3.0;
    fn compute(p1: Point, cp1: Point, cp2: Point, p2: Point, mut distance: f64) -> f64 {
        if Point::distance(cp1, Point::lerp(p1, p2, 1.0 / 3.0)) > TOLERANCE
            || Point::distance(cp2, Point::lerp(p1, p2, 2.0 / 3.0)) > TOLERANCE
        {
            let p = subdivide(p1, cp1, cp2, p2, 0.5);
            distance = compute(p[0], p[1], p[2], p[3], distance);
            distance = compute(p[3], p[4], p[5], p[6], distance);
        } else {
            distance += Point::distance(p1, p2);
        }
        distance
    }
    compute(start, c1, c2, end, 0.0)
}

/// `Path`。
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Path {
    pub commands: Vec<PathCommand>,
    pub fill_type: FillType,
}

impl Path {
    pub fn new(commands: Vec<PathCommand>, fill_type: FillType) -> Self {
        Self {
            commands,
            fill_type,
        }
    }

    pub fn with_fill_type(&self, fill_type: FillType) -> Path {
        Path::new(self.commands.clone(), fill_type)
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn transformed(&self, m: &AffineMatrix) -> Path {
        Path::new(
            self.commands.iter().map(|c| c.transformed(m)).collect(),
            self.fill_type,
        )
    }

    pub fn bounds(&self) -> Rect {
        if self.commands.is_empty() {
            return Rect::ZERO;
        }
        let mut sx = f64::MAX;
        let mut sy = f64::MAX;
        let mut lx = -f64::MAX;
        let mut ly = -f64::MAX;
        let mut add = |x: f64, y: f64| {
            sx = dart::min(x, sx);
            sy = dart::min(y, sy);
            lx = dart::max(x, lx);
            ly = dart::max(y, ly);
        };
        for c in &self.commands {
            match *c {
                PathCommand::Move(x, y) | PathCommand::Line(x, y) => add(x, y),
                PathCommand::Cubic(x1, y1, x2, y2, x3, y3) => {
                    add(x1, y1);
                    add(x2, y2);
                    add(x3, y3);
                }
                PathCommand::Close => {}
            }
        }
        Rect::from_ltrb(sx, sy, lx, ly)
    }

    /// `Path.dashed`。間隔が尽きない（0 以下ばかりの）ときに止まらないのは vgc と同じだが、ここでは打ち切る。
    pub fn dashed(&self, intervals: &[f64]) -> Path {
        if intervals.is_empty() {
            return self.clone();
        }
        Dasher::new(intervals).dash(self)
    }
}

/// `PathBuilder`。
pub struct PathBuilder {
    pub commands: Vec<PathCommand>,
    pub fill_type: FillType,
}

/// 楕円の弧を 3 次ベジエで近づける係数（impeller と同じ）。
const ARC_MAGIC: f64 = 0.551915024494;

impl PathBuilder {
    pub fn new(fill_type: Option<FillType>) -> Self {
        Self {
            commands: Vec::new(),
            fill_type: fill_type.unwrap_or(FillType::NonZero),
        }
    }

    pub fn from_path(path: &Path) -> Self {
        Self {
            commands: path.commands.clone(),
            fill_type: path.fill_type,
        }
    }

    pub fn move_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.commands.push(PathCommand::Move(x, y));
        self
    }

    pub fn line_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.commands.push(PathCommand::Line(x, y));
        self
    }

    pub fn cubic_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> &mut Self {
        self.commands
            .push(PathCommand::Cubic(x1, y1, x2, y2, x3, y3));
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.commands.push(PathCommand::Close);
        self
    }

    pub fn add_path(&mut self, other: &Path) -> &mut Self {
        self.commands.extend_from_slice(&other.commands);
        self
    }

    pub fn add_oval(&mut self, oval: Rect) -> &mut Self {
        let r = Point::new(oval.width() * 0.5, oval.height() * 0.5);
        let c = Point::new(
            oval.left + (oval.width() * 0.5),
            oval.top + (oval.height() * 0.5),
        );
        let m = Point::new(ARC_MAGIC * r.x, ARC_MAGIC * r.y);
        self.move_to(c.x, c.y - r.y);
        self.cubic_to(c.x + m.x, c.y - r.y, c.x + r.x, c.y - m.y, c.x + r.x, c.y);
        self.cubic_to(c.x + r.x, c.y + m.y, c.x + m.x, c.y + r.y, c.x, c.y + r.y);
        self.cubic_to(c.x - m.x, c.y + r.y, c.x - r.x, c.y + m.y, c.x - r.x, c.y);
        self.cubic_to(c.x - r.x, c.y - m.y, c.x - m.x, c.y - r.y, c.x, c.y - r.y);
        self.close()
    }

    pub fn add_rect(&mut self, r: Rect) -> &mut Self {
        self.move_to(r.left, r.top);
        self.line_to(r.right, r.top);
        self.line_to(r.right, r.bottom);
        self.line_to(r.left, r.bottom);
        self.close()
    }

    pub fn add_rrect(&mut self, r: Rect, rx: f64, ry: f64) -> &mut Self {
        if rx == 0.0 && ry == 0.0 {
            return self.add_rect(r);
        }
        let mx = rx * ARC_MAGIC;
        let my = ry * ARC_MAGIC;
        let (w, h) = (r.width(), r.height());
        self.move_to(r.left + rx, r.top);
        self.line_to(r.left + w - rx, r.top);
        self.cubic_to(
            r.left + w - rx + mx,
            r.top,
            r.left + w,
            r.top + ry - my,
            r.left + w,
            r.top + ry,
        );
        self.line_to(r.left + w, r.top + h - ry);
        self.cubic_to(
            r.left + w,
            r.top + h - ry + my,
            r.left + w - rx + mx,
            r.top + h,
            r.left + w - rx,
            r.top + h,
        );
        self.line_to(r.left + rx, r.top + h);
        self.cubic_to(
            r.left + rx - mx,
            r.top + h,
            r.left,
            r.top + h - ry + my,
            r.left,
            r.top + h - ry,
        );
        self.line_to(r.left, r.top + ry);
        self.cubic_to(
            r.left,
            r.top + ry - my,
            r.left + rx - mx,
            r.top,
            r.left + rx,
            r.top,
        );
        self.close()
    }

    pub fn take_path(&mut self) -> Path {
        Path::new(std::mem::take(&mut self.commands), self.fill_type)
    }

    /// `toPath(reset: false)`。
    pub fn snapshot(&self) -> Path {
        Path::new(self.commands.clone(), self.fill_type)
    }
}

/// 破線の間隔を繰り返す。
struct Intervals<'a> {
    vals: &'a [f64],
    idx: usize,
}

impl Intervals<'_> {
    fn next(&mut self) -> f64 {
        if self.idx >= self.vals.len() {
            self.idx = 0;
        }
        let v = self.vals[self.idx];
        self.idx += 1;
        v
    }
}

/// 破線の分割が止まらないとき（間隔が 0 以下ばかり）の打ち切り。
const DASH_LIMIT: usize = 10_000_000;

/// `_PathDasher`。
struct Dasher<'a> {
    intervals: Intervals<'a>,
    length: f64,
    current: Point,
    subpath: Point,
    draw: bool,
    out: Vec<PathCommand>,
}

impl<'a> Dasher<'a> {
    fn new(intervals: &'a [f64]) -> Self {
        Self {
            intervals: Intervals {
                vals: intervals,
                idx: 0,
            },
            length: 0.0,
            current: Point::new(0.0, 0.0),
            subpath: Point::new(0.0, 0.0),
            draw: true,
            out: Vec::new(),
        }
    }

    fn line_to(&mut self, target: Point) {
        let mut distance = Point::distance(self.current, target);
        if distance <= 0.0 || self.length <= 0.0 {
            return;
        }
        while distance >= self.length {
            if self.out.len() > DASH_LIMIT {
                return;
            }
            let t = self.length / distance;
            self.current = Point::lerp(self.current, target, t);
            self.length = self.intervals.next();
            if self.draw {
                self.out
                    .push(PathCommand::Line(self.current.x, self.current.y));
            } else {
                self.out
                    .push(PathCommand::Move(self.current.x, self.current.y));
            }
            distance = Point::distance(self.current, target);
            self.draw = !self.draw;
        }
        if distance > 0.0 {
            self.length -= distance;
            if self.draw {
                self.out.push(PathCommand::Line(target.x, target.y));
            }
        }
        self.current = target;
    }

    fn cubic_to(&mut self, mut c1: Point, mut c2: Point, mut end: Point) {
        let mut distance = cubic_length(self.current, c1, c2, end);
        while distance >= self.length {
            if self.out.len() > DASH_LIMIT {
                return;
            }
            let t = self.length / distance;
            let p = subdivide(self.current, c1, c2, end, t);
            self.current = p[3];
            if self.draw {
                self.out.push(PathCommand::Cubic(
                    p[1].x, p[1].y, p[2].x, p[2].y, p[3].x, p[3].y,
                ));
            } else {
                self.out
                    .push(PathCommand::Move(self.current.x, self.current.y));
            }
            (c1, c2, end) = (p[4], p[5], p[6]);
            self.length = self.intervals.next();
            distance = cubic_length(self.current, c1, c2, end);
            self.draw = !self.draw;
        }
        self.length -= distance;
        self.current = end;
        if self.draw {
            self.out
                .push(PathCommand::Cubic(c1.x, c1.y, c2.x, c2.y, end.x, end.y));
        }
    }

    fn dash(mut self, path: &Path) -> Path {
        self.length = self.intervals.next();
        self.draw = true;
        for c in &path.commands {
            match *c {
                PathCommand::Move(x, y) => {
                    self.current = Point::new(x, y);
                    self.subpath = self.current;
                    self.out.push(*c);
                }
                PathCommand::Line(x, y) => self.line_to(Point::new(x, y)),
                PathCommand::Cubic(x1, y1, x2, y2, x3, y3) => {
                    self.cubic_to(Point::new(x1, y1), Point::new(x2, y2), Point::new(x3, y3))
                }
                PathCommand::Close => {
                    self.line_to(self.subpath);
                    self.current = self.subpath;
                }
            }
        }
        Path::new(self.out, path.fill_type)
    }
}
