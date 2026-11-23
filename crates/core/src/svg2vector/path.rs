//! パスのデータ: `PathParser`・`PathBuilder`・`VdPath.Node`・`EllipseSolver`・`VdNodeRender`（外接矩形を求める
//! ためだけ）。

use super::java::{self, AffineTransform, J, Path2D, Thrown};

// ---- PathParser ----

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    Svg,
    #[allow(dead_code)]
    Android,
}

/// パスの 1 区切り（`VdPath.Node`）。
#[derive(Clone, Debug)]
pub struct PathNode {
    pub kind: char,
    pub params: Vec<f32>,
}

struct Extracted {
    end: usize,
    explicit_separator: bool,
}

fn extract(s: &[char], start: usize, flag_mode: bool) -> Extracted {
    let mut found_separator = false;
    let mut explicit_separator = false;
    let mut second_dot = false;
    let mut is_exponential = false;
    let mut i = start;
    while i < s.len() {
        let is_prev_exponential = is_exponential;
        is_exponential = false;
        match s[i] {
            ' ' | ',' => {
                found_separator = true;
                explicit_separator = true;
            }
            '-' => {
                if i != start && !is_prev_exponential {
                    found_separator = true;
                }
            }
            '.' => {
                if second_dot {
                    found_separator = true;
                } else {
                    second_dot = true;
                }
            }
            'e' | 'E' => is_exponential = true,
            _ => {}
        }
        if found_separator || flag_mode && i > start {
            break;
        }
        i += 1;
    }
    Extracted {
        end: i,
        explicit_separator,
    }
}

fn get_floats(s: &[char], mode: ParseMode) -> J<Vec<f32>> {
    let command = s[0];
    if command == 'z' || command == 'Z' {
        return Ok(Vec::new());
    }
    let arc = command == 'a' || command == 'A';
    let mut results = Vec::new();
    let mut number = String::new();
    let mut start = 1;
    while start < s.len() {
        let flag_mode =
            mode == ParseMode::Svg && arc && (results.len() % 7 == 3 || results.len() % 7 == 4);
        let r = extract(s, start, flag_mode);
        if start < r.end {
            number.clear();
            number.extend(&s[start..r.end]);
            match java::parse_float(&number) {
                Ok(v) => results.push(v),
                Err(_) => {
                    let whole: String = s.iter().collect();
                    return Err(Thrown::new(
                        java::RUNTIME,
                        format!("Error in parsing \"{whole}\""),
                    ));
                }
            }
        }
        start = if r.explicit_separator {
            r.end + 1
        } else {
            r.end
        };
    }
    if arc {
        let count = results.len() as isize;
        let mut i = 0;
        while i < count - 1 {
            results[i as usize] = results[i as usize].abs();
            results[i as usize + 1] = results[i as usize + 1].abs();
            i += 7;
        }
    }
    Ok(results)
}

fn next_start(s: &[char], mut end: usize) -> usize {
    while end < s.len() {
        let c = s[end];
        if c.is_ascii_uppercase() && c != 'E' || c.is_ascii_lowercase() && c != 'e' {
            return end;
        }
        end += 1;
    }
    end
}

/// `PathParser.parsePath`。
pub fn parse_path(value: &str, mode: ParseMode) -> J<Vec<PathNode>> {
    let value: Vec<char> = java::trim(value).chars().collect();
    let mut list = Vec::new();
    let mut start = 0;
    let mut end = 1;
    while end < value.len() {
        end = next_start(&value, end);
        let s = &value[start..end];
        let command = s[0];
        let val = get_floats(s, mode)?;
        if start == 0 && command != 'M' && command != 'm' {
            list.push(PathNode {
                kind: 'M',
                params: vec![0.0, 0.0],
            });
        }
        list.push(PathNode {
            kind: command,
            params: val,
        });
        start = end;
        end += 1;
    }
    if end - start == 1 && start < value.len() {
        list.push(PathNode {
            kind: value[start],
            params: Vec::new(),
        });
    }
    Ok(list)
}

// ---- PathBuilder ----

/// `PathBuilder`。
#[derive(Default)]
pub struct PathBuilder {
    data: String,
}

impl PathBuilder {
    fn command(&mut self, c: char, values: &[f64]) -> J<&mut Self> {
        self.data.push(c);
        for (i, v) in values.iter().enumerate() {
            if i > 0 {
                self.data.push(',');
            }
            self.data.push_str(&java::format_float_value(*v)?);
        }
        Ok(self)
    }

    pub fn absolute_move_to(&mut self, x: f64, y: f64) -> J<&mut Self> {
        self.command('M', &[x, y])
    }

    pub fn relative_move_to(&mut self, x: f64, y: f64) -> J<&mut Self> {
        self.command('m', &[x, y])
    }

    pub fn absolute_line_to(&mut self, x: f64, y: f64) -> J<&mut Self> {
        self.command('L', &[x, y])
    }

    pub fn relative_line_to(&mut self, x: f64, y: f64) -> J<&mut Self> {
        self.command('l', &[x, y])
    }

    pub fn relative_vertical_to(&mut self, v: f64) -> J<&mut Self> {
        self.command('v', &[v])
    }

    pub fn relative_horizontal_to(&mut self, h: f64) -> J<&mut Self> {
        self.command('h', &[h])
    }

    #[allow(clippy::too_many_arguments)]
    fn arc(
        &mut self,
        c: char,
        rx: f64,
        ry: f64,
        rotation: bool,
        large_arc: bool,
        sweep: bool,
        x: f64,
        y: f64,
    ) -> J<&mut Self> {
        let flag = |b: bool| if b { '1' } else { '0' };
        self.data.push(c);
        self.data.push_str(&java::format_float_value(rx)?);
        self.data.push(',');
        self.data.push_str(&java::format_float_value(ry)?);
        for f in [rotation, large_arc, sweep] {
            self.data.push(',');
            self.data.push(flag(f));
        }
        self.data.push(',');
        self.data.push_str(&java::format_float_value(x)?);
        self.data.push(',');
        self.data.push_str(&java::format_float_value(y)?);
        Ok(self)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn absolute_arc_to(
        &mut self,
        rx: f64,
        ry: f64,
        rotation: bool,
        large_arc: bool,
        sweep: bool,
        x: f64,
        y: f64,
    ) -> J<&mut Self> {
        self.arc('A', rx, ry, rotation, large_arc, sweep, x, y)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn relative_arc_to(
        &mut self,
        rx: f64,
        ry: f64,
        rotation: bool,
        large_arc: bool,
        sweep: bool,
        x: f64,
        y: f64,
    ) -> J<&mut Self> {
        self.arc('a', rx, ry, rotation, large_arc, sweep, x, y)
    }

    pub fn relative_close(&mut self) -> &mut Self {
        self.data.push('z');
        self
    }

    pub fn finish(&self) -> String {
        self.data.clone()
    }
}

// ---- VdPath.Node ----

/// `VdPath.Node.hasRelMoveAfterClose`。
pub fn has_rel_move_after_close(nodes: &[PathNode]) -> bool {
    let mut prev = ' ';
    for n in nodes {
        if (prev == 'z' || prev == 'Z') && n.kind == 'm' {
            return true;
        }
        prev = n.kind;
    }
    false
}

/// `VdPath.Node.nodeListToString`。数は `format` で書く。
pub fn node_list_to_string(nodes: &[PathNode], format: impl Fn(f64) -> String) -> J<String> {
    let mut out = String::new();
    for node in nodes {
        out.push(node.kind);
        let len = node.params.len();
        let implicit_line_to = (node.kind == 'm' || node.kind == 'M') && len > 2;
        let line_to = if node.kind == 'm' { 'l' } else { 'L' };
        for (j, &param) in node.params.iter().enumerate() {
            if j > 0 {
                out.push(if j % 2 != 0 { ',' } else { ' ' });
            }
            if implicit_line_to && j == 2 {
                out.push(line_to);
            }
            if !param.is_finite() {
                return Err(Thrown::new(
                    java::ILLEGAL_ARGUMENT,
                    format!("Invalid number: {}", java::float_to_string(param)),
                ));
            }
            out.push_str(&format(param as f64));
        }
    }
    Ok(out)
}

fn command_step(c: char) -> Option<usize> {
    Some(match c {
        'z' | 'Z' | 'm' | 'M' | 'l' | 'L' | 't' | 'T' => 2,
        'h' | 'H' | 'v' | 'V' => 1,
        'c' | 'C' => 6,
        's' | 'S' | 'q' | 'Q' => 4,
        'a' | 'A' => 7,
        _ => return None,
    })
}

fn is_translation_only(t: &mut AffineTransform) -> bool {
    let kind = t.get_type();
    kind == java::TYPE_IDENTITY || kind == java::TYPE_TRANSLATION
}

fn delta_transform(t: &AffineTransform, coords: &mut [f32], offset: usize, len: usize) {
    for i in 0..len / 2 {
        let j = offset + 2 * i;
        let (x, y) = t.delta_transform_point(coords[j] as f64, coords[j + 1] as f64);
        coords[j] = x as f32;
        coords[j + 1] = y as f32;
    }
}

/// `VdPath.Node.transform(AffineTransform, Node[])`。
pub fn transform_nodes(t: &mut AffineTransform, nodes: &mut [PathNode]) -> J<()> {
    let mut current = (0f32, 0f32);
    let mut segment_start = (0f32, 0f32);
    let mut previous = ' ';
    for n in nodes.iter_mut() {
        transform_node(n, t, &mut current, &mut segment_start, previous)?;
        previous = n.kind;
    }
    Ok(())
}

fn transform_node(
    node: &mut PathNode,
    t: &mut AffineTransform,
    current: &mut (f32, f32),
    segment_start: &mut (f32, f32),
    previous: char,
) -> J<()> {
    let len = node.params.len();
    let mut temp = vec![0f32; 2 * len];
    let (mut cx, mut cy) = *current;
    let (mut sx, mut sy) = *segment_start;
    let Some(step) = command_step(node.kind) else {
        return Err(Thrown::new(
            java::NULL_POINTER,
            "Cannot invoke \"java.lang.Integer.intValue()\" because the return value of \
             \"com.google.common.collect.ImmutableMap.get(Object)\" is null",
        ));
    };
    let p = &mut node.params;
    let idx = |i: isize| -> J<usize> {
        if i >= 0 && (i as usize) < len {
            Ok(i as usize)
        } else {
            Err(java::index_out_of_bounds(i, len))
        }
    };
    match node.kind {
        'z' | 'Z' => {
            cx = sx;
            cy = sy;
        }
        'M' | 'L' | 'T' | 'C' | 'S' | 'Q' => {
            if node.kind == 'M' {
                sx = p[idx(0)?];
                sy = p[idx(1)?];
            }
            cx = p[idx(len as isize - 2)?];
            cy = p[idx(len as isize - 1)?];
            t.transform_floats(p, 0, len / 2);
        }
        'm' => {
            if previous == 'z' || previous == 'Z' {
                node.kind = 'M';
                p[idx(0)?] += sx;
                p[idx(1)?] += sy;
                sx = p[0];
                sy = p[1];
                let mut i = step;
                while i < len {
                    p[i] += p[i - step];
                    p[idx(i as isize + 1)?] += p[i + 1 - step];
                    i += step;
                }
                cx = p[len - 2];
                cy = p[len - 1];
                t.transform_floats(p, 0, len / 2);
            } else {
                let head = 2;
                cx += p[idx(0)?];
                cy += p[idx(1)?];
                sx = cx;
                sy = cy;
                if previous == ' ' {
                    t.transform_floats(p, 0, head / 2);
                } else if !is_translation_only(t) {
                    delta_transform(t, p, 0, head);
                }
                let mut i = head;
                while i < len {
                    cx += p[i];
                    cy += p[idx(i as isize + 1)?];
                    i += step;
                }
                if !is_translation_only(t) {
                    delta_transform(t, p, head, len - head);
                }
            }
        }
        'l' | 't' | 'c' | 's' | 'q' => {
            let mut i = 0isize;
            while i < len as isize - step as isize + 1 {
                cx += p[idx(i + step as isize - 2)?];
                cy += p[idx(i + step as isize - 1)?];
                i += step as isize;
            }
            if !is_translation_only(t) {
                delta_transform(t, p, 0, len);
            }
        }
        'H' => {
            node.kind = 'L';
            for i in 0..len {
                temp[i * 2] = p[i];
                temp[i * 2 + 1] = cy;
                cx = p[i];
            }
            t.transform_floats(&mut temp, 0, len);
            *p = temp;
        }
        'V' => {
            node.kind = 'L';
            for i in 0..len {
                temp[i * 2] = cx;
                temp[i * 2 + 1] = p[i];
                cy = p[i];
            }
            t.transform_floats(&mut temp, 0, len);
            *p = temp;
        }
        'h' => {
            for i in 0..len {
                cx += p[i];
                temp[i * 2] = p[i];
                temp[i * 2 + 1] = 0.0;
            }
            if !is_translation_only(t) {
                node.kind = 'l';
                delta_transform(t, &mut temp, 0, 2 * len);
                *p = temp;
            }
        }
        'v' => {
            for i in 0..len {
                temp[i * 2] = 0.0;
                temp[i * 2 + 1] = p[i];
                cy += p[i];
            }
            if !is_translation_only(t) {
                node.kind = 'l';
                delta_transform(t, &mut temp, 0, 2 * len);
                *p = temp;
            }
        }
        'A' => {
            let mut i = 0isize;
            while i < len as isize - step as isize + 1 {
                let u = i as usize;
                if !is_translation_only(t) {
                    let e = EllipseSolver::new(
                        t,
                        cx,
                        cy,
                        p[u],
                        p[u + 1],
                        p[u + 2],
                        p[u + 3],
                        p[u + 4],
                        p[u + 5],
                        p[u + 6],
                    );
                    p[u] = e.major_axis;
                    p[u + 1] = e.minor_axis;
                    p[u + 2] = e.rotation_degree;
                    if e.direction_changed {
                        p[u + 4] = 1.0 - p[u + 4];
                    }
                }
                cx = p[u + 5];
                cy = p[u + 6];
                t.transform_floats(p, u + 5, 1);
                i += step as isize;
            }
        }
        'a' => {
            let mut i = 0isize;
            while i < len as isize - step as isize + 1 {
                let u = i as usize;
                let (old_x, old_y) = (cx, cy);
                cx += p[u + 5];
                cy += p[u + 6];
                if !is_translation_only(t) {
                    let e = EllipseSolver::new(
                        t,
                        old_x,
                        old_y,
                        p[u],
                        p[u + 1],
                        p[u + 2],
                        p[u + 3],
                        p[u + 4],
                        old_x + p[u + 5],
                        old_y + p[u + 6],
                    );
                    delta_transform(t, p, u + 5, 2);
                    p[u] = e.major_axis;
                    p[u + 1] = e.minor_axis;
                    p[u + 2] = e.rotation_degree;
                    if e.direction_changed {
                        p[u + 4] = 1.0 - p[u + 4];
                    }
                }
                i += step as isize;
            }
        }
        other => {
            return Err(Thrown::new(
                java::ILLEGAL_STATE,
                format!("Unexpected type {other}"),
            ));
        }
    }
    *current = (cx, cy);
    *segment_start = (sx, sy);
    Ok(())
}

// ---- EllipseSolver ----

struct EllipseSolver {
    major_axis: f32,
    minor_axis: f32,
    rotation_degree: f32,
    direction_changed: bool,
}

impl EllipseSolver {
    #[allow(clippy::too_many_arguments)]
    fn new(
        t: &AffineTransform,
        current_x: f32,
        current_y: f32,
        rx: f32,
        ry: f32,
        x_axis_rotation: f32,
        large_arc_flag: f32,
        sweep_flag: f32,
        dest_x: f32,
        dest_y: f32,
    ) -> Self {
        let mut s = Self {
            major_axis: 0.0,
            minor_axis: 0.0,
            rotation_degree: 0.0,
            direction_changed: false,
        };
        if rx == 0.0 || ry == 0.0 {
            return s;
        }
        let large_arc = large_arc_flag != 0.0;
        let sweep = sweep_flag != 0.0;
        let center = original_center(
            current_x,
            current_y,
            rx,
            ry,
            x_axis_rotation,
            large_arc,
            sweep,
            dest_x,
            dest_y,
        );
        let rot = x_axis_rotation as f64;
        let mut major = rotate_point((rx as f64, 0.0), rot);
        let mut minor = rotate_point((0.0, ry as f64), rot);
        major.0 += center.0;
        major.1 += center.1;
        minor.0 += center.0;
        minor.1 += center.1;
        let middle_radians = std::f64::consts::PI / 4.0;
        let (rxd, ryd) = (rx as f64, ry as f64);
        // rx * ry は float の掛け算
        let middle_r =
            (rx * ry) as f64 / java_hypot(ryd * middle_radians.cos(), rxd * middle_radians.sin());
        let mut middle = rotate_point(
            (
                middle_r * middle_radians.cos(),
                middle_r * middle_radians.sin(),
            ),
            rot,
        );
        middle.0 += center.0;
        middle.1 += center.1;
        let dst_middle = t.transform_point(middle.0, middle.1);
        let dst_major = t.transform_point(major.0, major.1);
        let dst_minor = t.transform_point(minor.0, minor.1);
        let dst_center = t.transform_point(center.0, center.1);
        let rel_middle = (dst_middle.0 - dst_center.0, dst_middle.1 - dst_center.1);
        let rel_major = (dst_major.0 - dst_center.0, dst_major.1 - dst_center.1);
        let rel_minor = (dst_minor.0 - dst_center.0, dst_minor.1 - dst_center.1);
        let src_cross = cross(middle, major, minor);
        let dst_cross = cross(dst_middle, dst_major, dst_minor);
        s.direction_changed = src_cross * dst_cross < 0.0;
        s.compute_ab_theta(rel_middle, rel_major, rel_minor);
        s
    }

    fn compute_ab_theta(&mut self, mid: (f64, f64), maj: (f64, f64), min: (f64, f64)) -> bool {
        let m11 = mid.0 * mid.0;
        let m12 = mid.0 * mid.1;
        let m13 = mid.1 * mid.1;
        let m21 = maj.0 * maj.0;
        let m22 = maj.0 * maj.1;
        let m23 = maj.1 * maj.1;
        let m31 = min.0 * min.0;
        let m32 = min.0 * min.1;
        let m33 = min.1 * min.1;
        let det = -(m13 * m22 * m31 - m12 * m23 * m31 - m13 * m21 * m32
            + m11 * m23 * m32
            + m12 * m21 * m33
            - m11 * m22 * m33);
        if det == 0.0 {
            return true;
        }
        let a = (-m13 * m22 + m12 * m23 + m13 * m32 - m23 * m32 - m12 * m33 + m22 * m33) / det;
        let b = (m13 * m21 - m11 * m23 - m13 * m31 + m23 * m31 + m11 * m33 - m21 * m33) / det;
        let c = (m12 * m21 - m11 * m22 - m12 * m31 + m22 * m31 + m11 * m32 - m21 * m32) / (-det);
        if a - c == 0.0 {
            self.minor_axis = java_hypot(maj.0, maj.1) as f32;
            self.major_axis = self.minor_axis;
            self.rotation_degree = 0.0;
            return false;
        }
        let double_theta = (b / (a - c)).atan();
        let theta = double_theta / 2.0;
        if double_theta.sin() == 0.0 {
            self.minor_axis = (1.0 / c).sqrt() as f32;
            self.major_axis = (1.0 / a).sqrt() as f32;
            self.rotation_degree = 0.0;
            return false;
        }
        let b_sq_inv = (a + c + b / double_theta.sin()) / 2.0;
        let a_sq_inv = a + c - b_sq_inv;
        if b_sq_inv == 0.0 || a_sq_inv == 0.0 {
            return true;
        }
        self.minor_axis = (1.0 / b_sq_inv).sqrt() as f32;
        self.major_axis = (1.0 / a_sq_inv).sqrt() as f32;
        self.rotation_degree = java::to_degrees(std::f64::consts::PI / 2.0 + theta) as f32;
        false
    }
}

fn java_hypot(x: f64, y: f64) -> f64 {
    x.hypot(y)
}

fn cross(middle: (f64, f64), major: (f64, f64), minor: (f64, f64)) -> f64 {
    let a = (major.0 - middle.0, major.1 - middle.1);
    let b = (minor.0 - middle.0, minor.1 - middle.1);
    a.0 * b.1 - a.1 * b.0
}

fn rotate_point(p: (f64, f64), radians: f64) -> (f64, f64) {
    let (cos, sin) = (radians.cos(), radians.sin());
    (p.0 * cos - p.1 * sin, p.0 * sin + p.1 * cos)
}

#[allow(clippy::too_many_arguments)]
fn original_center(
    x1: f32,
    y1: f32,
    rx: f32,
    ry: f32,
    phi: f32,
    large_arc: bool,
    sweep: bool,
    x2: f32,
    y2: f32,
) -> (f64, f64) {
    let cos_phi = (phi as f64).cos();
    let sin_phi = (phi as f64).sin();
    // Java では (x1 - x2) / 2 は float の計算
    let x_delta = ((x1 - x2) / 2.0) as f64;
    let y_delta = ((y1 - y2) / 2.0) as f64;
    let temp_x1 = cos_phi * x_delta + sin_phi * y_delta;
    let temp_y1 = -sin_phi * x_delta + cos_phi * y_delta;
    // rx * rx も float
    let rx_sq = (rx * rx) as f64;
    let ry_sq = (ry * ry) as f64;
    let temp_x1_sq = temp_x1 * temp_x1;
    let temp_y1_sq = temp_y1 * temp_y1;
    let mut f = rx_sq * ry_sq - rx_sq * temp_y1_sq - ry_sq * temp_x1_sq;
    f /= rx_sq * temp_y1_sq + ry_sq * temp_x1_sq;
    if f < 0.0 {
        f = 0.0;
    }
    f = f.sqrt();
    if large_arc == sweep {
        f = -f;
    }
    let temp_cx = f * rx as f64 * temp_y1 / ry as f64;
    let temp_cy = -f * ry as f64 * temp_x1 / rx as f64;
    let x_center = ((x1 + x2) / 2.0) as f64;
    let y_center = ((y1 + y2) / 2.0) as f64;
    (
        cos_phi * temp_cx - sin_phi * temp_cy + x_center,
        sin_phi * temp_cx + cos_phi * temp_cy + y_center,
    )
}

// ---- VdNodeRender ----

/// `VdNodeRender.createPath`。
pub fn create_path(nodes: &[PathNode], path: &mut Path2D) {
    let mut current = [0f32; 6];
    let mut last = ' ';
    for node in nodes {
        add_command(path, &mut current, node.kind, last, &node.params);
        last = node.kind;
    }
}

fn add_command(path: &mut Path2D, current: &mut [f32; 6], cmd: char, mut last: char, val: &[f32]) {
    let [mut cx, mut cy, mut cpx, mut cpy, mut loop_x, mut loop_y] = *current;
    let incr = match cmd {
        'z' | 'Z' => {
            path.close_path();
            cx = loop_x;
            cy = loop_y;
            2
        }
        'm' | 'M' | 'l' | 'L' | 't' | 'T' => 2,
        'h' | 'H' | 'v' | 'V' => 1,
        'c' | 'C' => 6,
        's' | 'S' | 'q' | 'Q' => 4,
        'a' | 'A' => 7,
        _ => 2,
    };
    let f = |v: f32| v as f64;
    let mut k = 0;
    while k < val.len() {
        // Java は配列の外を読むと例外になるが、区切りの数がそろわないパスはここまで来ない
        let v = |i: usize| val.get(k + i).copied().unwrap_or(0.0);
        match cmd {
            'm' => {
                cx += v(0);
                cy += v(1);
                if k > 0 {
                    path.line_to(f(cx), f(cy));
                } else {
                    path.move_to(f(cx), f(cy));
                    loop_x = cx;
                    loop_y = cy;
                }
            }
            'M' => {
                cx = v(0);
                cy = v(1);
                if k > 0 {
                    path.line_to(f(cx), f(cy));
                } else {
                    path.move_to(f(cx), f(cy));
                    loop_x = cx;
                    loop_y = cy;
                }
            }
            'l' => {
                cx += v(0);
                cy += v(1);
                path.line_to(f(cx), f(cy));
            }
            'L' => {
                cx = v(0);
                cy = v(1);
                path.line_to(f(cx), f(cy));
            }
            'z' | 'Z' => {
                path.close_path();
                cx = loop_x;
                cy = loop_y;
            }
            'h' => {
                cx += v(0);
                path.line_to(f(cx), f(cy));
            }
            'H' => {
                path.line_to(f(v(0)), f(cy));
                cx = v(0);
            }
            'v' => {
                cy += v(0);
                path.line_to(f(cx), f(cy));
            }
            'V' => {
                path.line_to(f(cx), f(v(0)));
                cy = v(0);
            }
            'c' => {
                path.curve_to(
                    f(cx + v(0)),
                    f(cy + v(1)),
                    f(cx + v(2)),
                    f(cy + v(3)),
                    f(cx + v(4)),
                    f(cy + v(5)),
                );
                cpx = cx + v(2);
                cpy = cy + v(3);
                cx += v(4);
                cy += v(5);
            }
            'C' => {
                path.curve_to(f(v(0)), f(v(1)), f(v(2)), f(v(3)), f(v(4)), f(v(5)));
                cx = v(4);
                cy = v(5);
                cpx = v(2);
                cpy = v(3);
            }
            's' | 'S' => {
                let reflect = matches!(last, 'c' | 's' | 'C' | 'S');
                let (x1, y1) = if reflect {
                    (2.0 * cx - cpx, 2.0 * cy - cpy)
                } else {
                    (cx, cy)
                };
                if cmd == 's' {
                    path.curve_to(
                        f(x1),
                        f(y1),
                        f(cx + v(0)),
                        f(cy + v(1)),
                        f(cx + v(2)),
                        f(cy + v(3)),
                    );
                    cpx = cx + v(0);
                    cpy = cy + v(1);
                    cx += v(2);
                    cy += v(3);
                } else {
                    path.curve_to(f(x1), f(y1), f(v(0)), f(v(1)), f(v(2)), f(v(3)));
                    cpx = v(0);
                    cpy = v(1);
                    cx = v(2);
                    cy = v(3);
                }
            }
            'q' => {
                path.quad_to(f(cx + v(0)), f(cy + v(1)), f(cx + v(2)), f(cy + v(3)));
                cpx = cx + v(0);
                cpy = cy + v(1);
                cx += v(2);
                cy += v(3);
            }
            'Q' => {
                path.quad_to(f(v(0)), f(v(1)), f(v(2)), f(v(3)));
                cx = v(2);
                cy = v(3);
                cpx = v(0);
                cpy = v(1);
            }
            't' | 'T' => {
                let reflect = matches!(last, 'q' | 't' | 'Q' | 'T');
                let rx = if reflect { 2.0 * cx - cpx } else { cx };
                let ry = if reflect { 2.0 * cy - cpy } else { cy };
                if cmd == 't' {
                    path.quad_to(f(rx), f(ry), f(cx + v(0)), f(cy + v(1)));
                    cx += v(0);
                    cy += v(1);
                } else {
                    path.quad_to(f(rx), f(ry), f(v(0)), f(v(1)));
                    cx = v(0);
                    cy = v(1);
                }
                cpx = rx;
                cpy = ry;
            }
            'a' | 'A' => {
                let (x1, y1) = if cmd == 'a' {
                    (v(5) + cx, v(6) + cy)
                } else {
                    (v(5), v(6))
                };
                draw_arc(
                    path,
                    f(cx),
                    f(cy),
                    f(x1),
                    f(y1),
                    f(v(0).abs()),
                    f(v(1).abs()),
                    f(v(2)),
                    v(3) != 0.0,
                    v(4) != 0.0,
                );
                if cmd == 'a' {
                    cx += v(5);
                    cy += v(6);
                } else {
                    cx = v(5);
                    cy = v(6);
                }
                cpx = cx;
                cpy = cy;
            }
            _ => {}
        }
        last = cmd;
        k += incr;
    }
    *current = [cx, cy, cpx, cpy, loop_x, loop_y];
}

#[allow(clippy::too_many_arguments)]
fn draw_arc(
    p: &mut Path2D,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    a: f64,
    b: f64,
    theta: f64,
    more_than_half: bool,
    positive_arc: bool,
) {
    // Java: theta * Math.PI / 180.0f
    let theta_d = theta * std::f64::consts::PI / 180.0;
    let cos_t = theta_d.cos();
    let sin_t = theta_d.sin();
    let x0p = (x0 * cos_t + y0 * sin_t) / a;
    let y0p = (-x0 * sin_t + y0 * cos_t) / b;
    let x1p = (x1 * cos_t + y1 * sin_t) / a;
    let y1p = (-x1 * sin_t + y1 * cos_t) / b;
    let dx = x0p - x1p;
    let dy = y0p - y1p;
    let xm = (x0p + x1p) / 2.0;
    let ym = (y0p + y1p) / 2.0;
    let dsq = dx * dx + dy * dy;
    if dsq == 0.0 {
        return;
    }
    let disc = 1.0 / dsq - 1.0 / 4.0;
    if disc < 0.0 {
        let adjust = dsq.sqrt() / 1.99999;
        draw_arc(
            p,
            x0,
            y0,
            x1,
            y1,
            a * adjust,
            b * adjust,
            theta,
            more_than_half,
            positive_arc,
        );
        return;
    }
    let s = disc.sqrt();
    let sdx = s * dx;
    let sdy = s * dy;
    let (mut cx, mut cy) = if more_than_half == positive_arc {
        (xm - sdy, ym + sdx)
    } else {
        (xm + sdy, ym - sdx)
    };
    let eta0 = (y0p - cy).atan2(x0p - cx);
    let eta1 = (y1p - cy).atan2(x1p - cx);
    let mut sweep = eta1 - eta0;
    if positive_arc != (sweep >= 0.0) {
        if sweep > 0.0 {
            sweep -= 2.0 * std::f64::consts::PI;
        } else {
            sweep += 2.0 * std::f64::consts::PI;
        }
    }
    cx *= a;
    cy *= b;
    let tcx = cx;
    cx = cx * cos_t - cy * sin_t;
    cy = tcx * sin_t + cy * cos_t;
    arc_to_bezier(p, cx, cy, a, b, x0, y0, theta_d, eta0, sweep);
}

#[allow(clippy::too_many_arguments)]
fn arc_to_bezier(
    p: &mut Path2D,
    cx: f64,
    cy: f64,
    a: f64,
    b: f64,
    mut e1x: f64,
    mut e1y: f64,
    theta: f64,
    start: f64,
    sweep: f64,
) {
    let segments = (sweep * 4.0 / std::f64::consts::PI).abs().ceil() as i32;
    let mut eta1 = start;
    let cos_t = theta.cos();
    let sin_t = theta.sin();
    let cos_eta1 = eta1.cos();
    let sin_eta1 = eta1.sin();
    let mut ep1x = (-a * cos_t * sin_eta1) - (b * sin_t * cos_eta1);
    let mut ep1y = (-a * sin_t * sin_eta1) + (b * cos_t * cos_eta1);
    let per = sweep / segments as f64;
    for _ in 0..segments {
        let eta2 = eta1 + per;
        let sin_eta2 = eta2.sin();
        let cos_eta2 = eta2.cos();
        let e2x = cx + (a * cos_t * cos_eta2) - (b * sin_t * sin_eta2);
        let e2y = cy + (a * sin_t * cos_eta2) + (b * cos_t * sin_eta2);
        let ep2x = -a * cos_t * sin_eta2 - b * sin_t * cos_eta2;
        let ep2y = -a * sin_t * sin_eta2 + b * cos_t * cos_eta2;
        let tan_diff2 = ((eta2 - eta1) / 2.0).tan();
        let alpha =
            (eta2 - eta1).sin() * ((4.0 + (3.0 * tan_diff2 * tan_diff2)).sqrt() - 1.0) / 3.0;
        let q1x = e1x + alpha * ep1x;
        let q1y = e1y + alpha * ep1y;
        let q2x = e2x - alpha * ep2x;
        let q2y = e2y - alpha * ep2y;
        p.curve_to(q1x, q1y, q2x, q2y, e2x, e2y);
        eta1 = eta2;
        e1x = e2x;
        e1y = e2y;
        ep1x = ep2x;
        ep1y = ep2y;
    }
}
