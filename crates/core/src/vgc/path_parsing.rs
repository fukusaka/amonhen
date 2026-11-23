//! SVG のパスのデータ（path_parsing 1.1.0。もとは Chromium の blink の SVG のパスの読み手を Dart にしたもの）。
//! 弧は 3 次ベジエに分ける。弧の変換の行列は vector_math の `Matrix4`（`vector_math.dart` の方なので要素は
//! float32）で、丸めも同じにする。
//!
//! path_parsing のライセンス:
//!
//! ```text
//! Copyright (c) 2018 Dan Field
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.
//! ```

use super::dart::{Error, R};
use super::geometry::{FillType, Path, PathBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seg {
    Unknown,
    Close,
    MoveToAbs,
    MoveToRel,
    LineToAbs,
    LineToRel,
    CubicToAbs,
    CubicToRel,
    QuadToAbs,
    QuadToRel,
    ArcToAbs,
    ArcToRel,
    LineToHorizontalAbs,
    LineToHorizontalRel,
    LineToVerticalAbs,
    LineToVerticalRel,
    SmoothCubicToAbs,
    SmoothCubicToRel,
    SmoothQuadToAbs,
    SmoothQuadToRel,
}

fn letter(c: i32) -> Seg {
    match u8::try_from(c).map(char::from) {
        Ok('Z' | 'z') => Seg::Close,
        Ok('M') => Seg::MoveToAbs,
        Ok('m') => Seg::MoveToRel,
        Ok('L') => Seg::LineToAbs,
        Ok('l') => Seg::LineToRel,
        Ok('C') => Seg::CubicToAbs,
        Ok('c') => Seg::CubicToRel,
        Ok('Q') => Seg::QuadToAbs,
        Ok('q') => Seg::QuadToRel,
        Ok('A') => Seg::ArcToAbs,
        Ok('a') => Seg::ArcToRel,
        Ok('H') => Seg::LineToHorizontalAbs,
        Ok('h') => Seg::LineToHorizontalRel,
        Ok('V') => Seg::LineToVerticalAbs,
        Ok('v') => Seg::LineToVerticalRel,
        Ok('S') => Seg::SmoothCubicToAbs,
        Ok('s') => Seg::SmoothCubicToRel,
        Ok('T') => Seg::SmoothQuadToAbs,
        Ok('t') => Seg::SmoothQuadToRel,
        _ => Seg::Unknown,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Offset {
    dx: f64,
    dy: f64,
}

impl Offset {
    const ZERO: Offset = Offset { dx: 0.0, dy: 0.0 };

    fn new(dx: f64, dy: f64) -> Self {
        Self { dx, dy }
    }

    fn translate(self, x: f64, y: f64) -> Self {
        Self::new(self.dx + x, self.dy + y)
    }

    fn add(self, o: Offset) -> Self {
        Self::new(self.dx + o.dx, self.dy + o.dy)
    }

    fn sub(self, o: Offset) -> Self {
        Self::new(self.dx - o.dx, self.dy - o.dy)
    }

    fn mul(self, k: f64) -> Self {
        Self::new(self.dx * k, self.dy * k)
    }

    fn direction(self) -> f64 {
        self.dy.atan2(self.dx)
    }
}

#[derive(Clone, Copy)]
struct Segment {
    command: Seg,
    target: Offset,
    point1: Offset,
    point2: Offset,
    arc_sweep: bool,
    arc_large: bool,
}

impl Segment {
    fn new() -> Self {
        Self {
            command: Seg::Unknown,
            target: Offset::ZERO,
            point1: Offset::ZERO,
            point2: Offset::ZERO,
            arc_sweep: false,
            arc_large: false,
        }
    }
}

/// `SvgPathStringSource`。Dart は UTF-16 の単位で読むが、意味のある文字はどれも ASCII なのでバイトで読む。
struct Source<'a> {
    s: &'a [u8],
    previous: Seg,
    idx: usize,
}

fn is_html_space(c: i32) -> bool {
    c <= 32 && (c == 32 || c == 10 || c == 9 || c == 13 || c == 12)
}

fn is_number_start(c: i32) -> bool {
    (48..=57).contains(&c) || c == 43 || c == 45 || c == 46
}

fn valid_range(x: f64) -> bool {
    (-f64::MAX..=f64::MAX).contains(&x)
}

impl<'a> Source<'a> {
    fn new(s: &'a str) -> Self {
        let mut src = Self {
            s: s.as_bytes(),
            previous: Seg::Unknown,
            idx: 0,
        };
        src.skip_spaces();
        src
    }

    fn at(&self, i: usize) -> i32 {
        i32::from(self.s[i])
    }

    fn skip_spaces(&mut self) -> i32 {
        loop {
            if self.idx >= self.s.len() {
                return -1;
            }
            let c = self.at(self.idx);
            if !is_html_space(c) {
                return c;
            }
            self.idx += 1;
        }
    }

    fn skip_spaces_or_comma(&mut self) {
        let c = self.skip_spaces();
        if c == 44 {
            self.idx += 1;
            self.skip_spaces();
        }
    }

    fn maybe_implicit(&self, lookahead: i32, next: Seg) -> Seg {
        if !is_number_start(lookahead) || self.previous == Seg::Close {
            return next;
        }
        match self.previous {
            Seg::MoveToAbs => Seg::LineToAbs,
            Seg::MoveToRel => Seg::LineToRel,
            p => p,
        }
    }

    fn read(&mut self) -> i32 {
        if self.idx >= self.s.len() {
            return -1;
        }
        let c = self.at(self.idx);
        self.idx += 1;
        c
    }

    fn number(&mut self) -> R<f64> {
        self.skip_spaces();
        let mut sign = 1.0;
        let mut c = self.read();
        if c == 43 {
            c = self.read();
        } else if c == 45 {
            sign = -1.0;
            c = self.read();
        }
        if !(48..=57).contains(&c) && c != 46 {
            return Err(Error::state(
                "First character of a number must be one of [0-9+-.].",
            ));
        }
        let mut integer = 0.0;
        while (48..=57).contains(&c) {
            integer = integer * 10.0 + f64::from(c - 48);
            c = self.read();
        }
        if !valid_range(integer) {
            return Err(Error::state("Numeric overflow"));
        }
        let mut decimal = 0.0;
        if c == 46 {
            c = self.read();
            if !(48..=57).contains(&c) {
                return Err(Error::state(
                    "There must be at least one digit following the .",
                ));
            }
            let mut frac = 1.0;
            while (48..=57).contains(&c) {
                frac *= 0.1;
                decimal += f64::from(c - 48) * frac;
                c = self.read();
            }
        }
        let mut number = integer + decimal;
        number *= sign;
        if self.idx < self.s.len()
            && (c == 101 || c == 69)
            && (self.at(self.idx) != 120 && self.at(self.idx) != 109)
        {
            c = self.read();
            let mut negative = false;
            if c == 43 {
                c = self.read();
            } else if c == 45 {
                c = self.read();
                negative = true;
            }
            if !(48..=57).contains(&c) {
                return Err(Error::state("Missing exponent"));
            }
            let mut exponent = 0.0;
            while (48..=57).contains(&c) {
                exponent *= 10.0;
                exponent += f64::from(c - 48);
                c = self.read();
            }
            if negative {
                exponent = -exponent;
            }
            if !(-37.0..=38.0).contains(&exponent) {
                return Err(Error::state(format!(
                    "Invalid exponent {}",
                    super::dart::double_to_string(exponent)
                )));
            }
            if exponent != 0.0 {
                number *= 10f64.powf(exponent);
            }
        }
        if !valid_range(number) {
            return Err(Error::state("Numeric overflow"));
        }
        if c != -1 {
            self.idx -= 1;
            self.skip_spaces_or_comma();
        }
        Ok(number)
    }

    fn arc_flag(&mut self) -> R<bool> {
        if self.idx >= self.s.len() {
            return Err(Error::state("Expected more data"));
        }
        let c = self.at(self.idx);
        self.idx += 1;
        self.skip_spaces_or_comma();
        match c {
            48 => Ok(false),
            49 => Ok(true),
            _ => Err(Error::state("Invalid flag value")),
        }
    }

    fn has_more(&self) -> bool {
        self.idx < self.s.len()
    }

    fn segment(&mut self) -> R<Segment> {
        let mut seg = Segment::new();
        let lookahead = self.at(self.idx);
        let mut command = letter(lookahead);
        if self.previous == Seg::Unknown {
            if command != Seg::MoveToRel && command != Seg::MoveToAbs {
                return Err(Error::state("Expected to find moveTo command"));
            }
            self.idx += 1;
        } else if command == Seg::Unknown {
            command = self.maybe_implicit(lookahead, command);
            if command == Seg::Unknown {
                return Err(Error::state("Expected a path command"));
            }
        } else {
            self.idx += 1;
        }
        self.previous = command;
        seg.command = command;
        match command {
            Seg::CubicToRel | Seg::CubicToAbs => {
                seg.point1 = Offset::new(self.number()?, self.number()?);
                seg.point2 = Offset::new(self.number()?, self.number()?);
                seg.target = Offset::new(self.number()?, self.number()?);
            }
            Seg::SmoothCubicToRel | Seg::SmoothCubicToAbs => {
                seg.point2 = Offset::new(self.number()?, self.number()?);
                seg.target = Offset::new(self.number()?, self.number()?);
            }
            Seg::MoveToRel
            | Seg::MoveToAbs
            | Seg::LineToRel
            | Seg::LineToAbs
            | Seg::SmoothQuadToRel
            | Seg::SmoothQuadToAbs => {
                seg.target = Offset::new(self.number()?, self.number()?);
            }
            Seg::LineToHorizontalRel | Seg::LineToHorizontalAbs => {
                seg.target = Offset::new(self.number()?, seg.target.dy);
            }
            Seg::LineToVerticalRel | Seg::LineToVerticalAbs => {
                seg.target = Offset::new(seg.target.dx, self.number()?);
            }
            Seg::Close => {
                self.skip_spaces();
            }
            Seg::QuadToRel | Seg::QuadToAbs => {
                seg.point1 = Offset::new(self.number()?, self.number()?);
                seg.target = Offset::new(self.number()?, self.number()?);
            }
            Seg::ArcToRel | Seg::ArcToAbs => {
                seg.point1 = Offset::new(self.number()?, self.number()?);
                let angle = self.number()?;
                seg.point2 = Offset::new(angle, seg.point2.dy);
                seg.arc_large = self.arc_flag()?;
                seg.arc_sweep = self.arc_flag()?;
                seg.target = Offset::new(self.number()?, self.number()?);
            }
            Seg::Unknown => return Err(Error::state("Unknown segment command")),
        }
        Ok(seg)
    }
}

fn reflected(reflect_in: Offset, p: Offset) -> Offset {
    Offset::new(2.0 * reflect_in.dx - p.dx, 2.0 * reflect_in.dy - p.dy)
}

const ONE_OVER_THREE: f64 = 1.0 / 3.0;

fn blend(p1: Offset, p2: Offset) -> Offset {
    Offset::new(
        (p1.dx + 2.0 * p2.dx) * ONE_OVER_THREE,
        (p1.dy + 2.0 * p2.dy) * ONE_OVER_THREE,
    )
}

fn is_cubic(c: Seg) -> bool {
    matches!(
        c,
        Seg::CubicToAbs | Seg::CubicToRel | Seg::SmoothCubicToAbs | Seg::SmoothCubicToRel
    )
}

fn is_quad(c: Seg) -> bool {
    matches!(
        c,
        Seg::QuadToAbs | Seg::QuadToRel | Seg::SmoothQuadToAbs | Seg::SmoothQuadToRel
    )
}

/// vector_math の `Matrix4`（float32 の要素）。弧の変換に使う所だけ。
struct Matrix4([f32; 16]);

impl Matrix4 {
    fn identity() -> Self {
        let mut m = [0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        Self(m)
    }

    fn get(&self, i: usize) -> f64 {
        f64::from(self.0[i])
    }

    fn rotate_z(&mut self, angle: f64) {
        let cos = angle.cos();
        let sin = angle.sin();
        let mut t = [0f64; 8];
        for k in 0..4 {
            t[k] = self.get(k) * cos + self.get(k + 4) * sin;
            t[k + 4] = self.get(k) * -sin + self.get(k + 4) * cos;
        }
        for (k, v) in t.iter().enumerate() {
            self.0[k] = *v as f32;
        }
    }

    fn scale(&mut self, sx: f64, sy: f64) {
        let sz = sx;
        for k in 0..4 {
            self.0[k] = (self.get(k) * sx) as f32;
            self.0[k + 4] = (self.get(k + 4) * sy) as f32;
            self.0[k + 8] = (self.get(k + 8) * sz) as f32;
            self.0[k + 12] = (self.get(k + 12) * 1.0) as f32;
        }
    }

    fn map(&self, p: Offset) -> Offset {
        Offset::new(
            self.get(0) * p.dx + self.get(4) * p.dy + self.get(12),
            self.get(1) * p.dx + self.get(5) * p.dy + self.get(13),
        )
    }
}

/// `SvgPathNormalizer`。
struct Normalizer {
    current: Offset,
    subpath: Offset,
    control: Offset,
    last: Seg,
}

impl Normalizer {
    fn new() -> Self {
        Self {
            current: Offset::ZERO,
            subpath: Offset::ZERO,
            control: Offset::ZERO,
            last: Seg::Unknown,
        }
    }

    fn emit(&mut self, segment: Segment, path: &mut PathBuilder) -> R<()> {
        let mut n = segment;
        match segment.command {
            Seg::QuadToRel => {
                n.point1 = n.point1.add(self.current);
                n.target = n.target.add(self.current);
            }
            Seg::CubicToRel => {
                n.point1 = n.point1.add(self.current);
                n.point2 = n.point2.add(self.current);
                n.target = n.target.add(self.current);
            }
            Seg::SmoothCubicToRel => {
                n.point2 = n.point2.add(self.current);
                n.target = n.target.add(self.current);
            }
            Seg::MoveToRel
            | Seg::LineToRel
            | Seg::LineToHorizontalRel
            | Seg::LineToVerticalRel
            | Seg::SmoothQuadToRel
            | Seg::ArcToRel => {
                n.target = n.target.add(self.current);
            }
            Seg::LineToHorizontalAbs => n.target = Offset::new(n.target.dx, self.current.dy),
            Seg::LineToVerticalAbs => n.target = Offset::new(self.current.dx, n.target.dy),
            Seg::Close => n.target = self.subpath,
            _ => {}
        }
        match segment.command {
            Seg::MoveToRel | Seg::MoveToAbs => {
                self.subpath = n.target;
                path.move_to(n.target.dx, n.target.dy);
            }
            Seg::LineToRel
            | Seg::LineToAbs
            | Seg::LineToHorizontalRel
            | Seg::LineToHorizontalAbs
            | Seg::LineToVerticalRel
            | Seg::LineToVerticalAbs => {
                path.line_to(n.target.dx, n.target.dy);
            }
            Seg::Close => {
                path.close();
            }
            Seg::SmoothCubicToRel | Seg::SmoothCubicToAbs | Seg::CubicToRel | Seg::CubicToAbs => {
                if matches!(
                    segment.command,
                    Seg::SmoothCubicToRel | Seg::SmoothCubicToAbs
                ) {
                    n.point1 = if is_cubic(self.last) {
                        reflected(self.current, self.control)
                    } else {
                        self.current
                    };
                }
                self.control = n.point2;
                path.cubic_to(
                    n.point1.dx,
                    n.point1.dy,
                    n.point2.dx,
                    n.point2.dy,
                    n.target.dx,
                    n.target.dy,
                );
            }
            Seg::SmoothQuadToRel | Seg::SmoothQuadToAbs | Seg::QuadToRel | Seg::QuadToAbs => {
                if matches!(segment.command, Seg::SmoothQuadToRel | Seg::SmoothQuadToAbs) {
                    n.point1 = if is_quad(self.last) {
                        reflected(self.current, self.control)
                    } else {
                        self.current
                    };
                }
                self.control = n.point1;
                n.point1 = blend(self.current, self.control);
                n.point2 = blend(n.target, self.control);
                path.cubic_to(
                    n.point1.dx,
                    n.point1.dy,
                    n.point2.dx,
                    n.point2.dy,
                    n.target.dx,
                    n.target.dy,
                );
            }
            Seg::ArcToRel | Seg::ArcToAbs => {
                if !decompose_arc(self.current, &n, path)? {
                    path.line_to(n.target.dx, n.target.dy);
                }
            }
            Seg::Unknown => return Err(Error::state("Invalid command type in path")),
        }
        self.current = n.target;
        if !is_cubic(segment.command) && !is_quad(segment.command) {
            self.control = self.current;
        }
        self.last = segment.command;
        Ok(())
    }
}

const TWO_PI: f64 = std::f64::consts::PI * 2.0;
const PI_OVER_TWO: f64 = std::f64::consts::PI / 2.0;

fn decompose_arc(current: Offset, arc: &Segment, path: &mut PathBuilder) -> R<bool> {
    let mut rx = arc.point1.dx.abs();
    let mut ry = arc.point1.dy.abs();
    if rx == 0.0 || ry == 0.0 {
        return Ok(false);
    }
    if arc.target == current {
        return Ok(false);
    }
    // vector_math の radians（度 × (π/180)）
    let angle = arc.point2.dx * (std::f64::consts::PI / 180.0);
    let mid = current.sub(arc.target).mul(0.5);
    let mut m = Matrix4::identity();
    m.rotate_z(-angle);
    let tmid = m.map(mid);
    let square_rx = rx * rx;
    let square_ry = ry * ry;
    let square_x = tmid.dx * tmid.dx;
    let square_y = tmid.dy * tmid.dy;
    let radii_scale = square_x / square_rx + square_y / square_ry;
    if radii_scale > 1.0 {
        rx *= radii_scale.sqrt();
        ry *= radii_scale.sqrt();
    }
    m = Matrix4::identity();
    m.scale(1.0 / rx, 1.0 / ry);
    m.rotate_z(-angle);
    let mut point1 = m.map(current);
    let mut point2 = m.map(arc.target);
    let mut delta = point2.sub(point1);
    let d = delta.dx * delta.dx + delta.dy * delta.dy;
    let scale_squared = super::dart::max(1.0 / d - 0.25, 0.0);
    let mut scale = scale_squared.sqrt();
    if !scale.is_finite() {
        scale = 0.0;
    }
    if arc.arc_sweep == arc.arc_large {
        scale = -scale;
    }
    delta = delta.mul(scale);
    let center = point1.add(point2).mul(0.5).translate(-delta.dy, delta.dx);
    let theta1 = point1.sub(center).direction();
    let theta2 = point2.sub(center).direction();
    let mut theta_arc = theta2 - theta1;
    if theta_arc < 0.0 && arc.arc_sweep {
        theta_arc += TWO_PI;
    } else if theta_arc > 0.0 && !arc.arc_sweep {
        theta_arc -= TWO_PI;
    }
    m = Matrix4::identity();
    m.rotate_z(angle);
    m.scale(rx, ry);
    let segments_f = (theta_arc / (PI_OVER_TWO + 0.001)).abs().ceil();
    if !segments_f.is_finite() {
        return Err(Error::to_int());
    }
    let segments = segments_f as i64;
    for i in 0..segments {
        let start_theta = theta1 + i as f64 * theta_arc / segments as f64;
        let end_theta = theta1 + (i + 1) as f64 * theta_arc / segments as f64;
        let t = (8.0 / 6.0) * (0.25 * (end_theta - start_theta)).tan();
        if !t.is_finite() {
            return Ok(false);
        }
        let sin_start = start_theta.sin();
        let cos_start = start_theta.cos();
        let sin_end = end_theta.sin();
        let cos_end = end_theta.cos();
        point1 = Offset::new(cos_start - t * sin_start, sin_start + t * cos_start)
            .translate(center.dx, center.dy);
        let target = Offset::new(cos_end, sin_end).translate(center.dx, center.dy);
        point2 = target.translate(t * sin_end, -t * cos_end);
        let p1 = m.map(point1);
        let p2 = m.map(point2);
        let tp = m.map(target);
        path.cubic_to(p1.dx, p1.dy, p2.dx, p2.dy, tp.dx, tp.dy);
    }
    Ok(true)
}

/// `parseSvgPathData`。
pub fn parse_svg_path_data(svg: &str, fill_type: Option<FillType>) -> R<Path> {
    if svg.is_empty() {
        return Ok(Path::new(
            Vec::new(),
            fill_type.unwrap_or(FillType::NonZero),
        ));
    }
    let mut source = Source::new(svg);
    let mut builder = PathBuilder::new(fill_type);
    let mut normalizer = Normalizer::new();
    while source.has_more() {
        let seg = source.segment()?;
        normalizer.emit(seg, &mut builder)?;
    }
    Ok(builder.take_path())
}
