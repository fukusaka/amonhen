//! `svg/*.svg` を読み、形ごとの線のパス（SVG のパスの書き方）と破線を `icons.rs` に書き出す。
//! パスは、曲線の描き方の違う FE（CGPath・Direct2D）のために、移動・直線・3 次ベジェ・閉じるだけの列にも直す（弧は
//! ベジェで近づける）。
//! 自分で描いた SVG だけを読むので、使う要素（path・circle・rect）と属性だけを解く。知らないものはビルドを止める。

use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("svg");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<_> = fs::read_dir(&dir)
        .expect("svg dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
    names.sort();
    let mut out = String::from("pub static ICONS: &[(&str, Icon)] = &[\n");
    for path in &names {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path.file_stem().unwrap().to_str().unwrap();
        let text = fs::read_to_string(path).unwrap();
        let shapes = shapes(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        writeln!(out, "    ({name:?}, Icon {{ shapes: &[").unwrap();
        for (d, dash) in shapes {
            let segments = segments(&d).unwrap_or_else(|e| panic!("{}: {e}: {d}", path.display()));
            writeln!(
                out,
                "        Shape {{ d: {d:?}, dash: &{dash:?}, segments: &{segments:?} }},"
            )
            .unwrap();
        }
        writeln!(out, "    ] }}),").unwrap();
    }
    out.push_str("];\n");
    let dest = Path::new(&env::var("OUT_DIR").unwrap()).join("icons.rs");
    fs::write(dest, out).unwrap();
}

/// 線の形を順に返す（パス、破線の長さ）。
fn shapes(svg: &str) -> Result<Vec<(String, Vec<f32>)>, String> {
    let root = svg.find("<svg").ok_or("no <svg>")?;
    let body = &svg[svg[root..].find('>').ok_or("bad <svg>")? + root + 1..];
    let mut shapes = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find('<') {
        let end = rest[start..].find('>').ok_or("unclosed tag")? + start;
        let tag = &rest[start + 1..end];
        rest = &rest[end + 1..];
        if tag.starts_with('/') {
            continue;
        }
        let tag = tag.trim_end_matches('/').trim();
        let (kind, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        let attrs = parse_attrs(attrs)?;
        let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let num = |k: &str| -> Result<f64, String> {
            get(k)
                .ok_or(format!("{kind}: no {k}"))?
                .parse()
                .map_err(|_| format!("{kind}: bad {k}"))
        };
        for (n, _) in &attrs {
            if !matches!(
                n.as_str(),
                "d" | "cx"
                    | "cy"
                    | "r"
                    | "x"
                    | "y"
                    | "width"
                    | "height"
                    | "rx"
                    | "stroke-dasharray"
            ) {
                return Err(format!("{kind}: unsupported attribute {n}"));
            }
        }
        let d = match kind {
            "path" => get("d").ok_or("path: no d")?.to_owned(),
            "circle" => {
                let (cx, cy, r) = (num("cx")?, num("cy")?, num("r")?);
                format!(
                    "M{} {}A{r} {r} 0 1 0 {} {}A{r} {r} 0 1 0 {} {}Z",
                    cx - r,
                    cy,
                    cx + r,
                    cy,
                    cx - r,
                    cy
                )
            }
            "rect" => {
                let (x, y, w, h) = (num("x")?, num("y")?, num("width")?, num("height")?);
                let r = get("rx").map_or(Ok(0.0), |_| num("rx"))?;
                format!(
                    "M{} {y}H{}A{r} {r} 0 0 1 {} {}V{}A{r} {r} 0 0 1 {} {}H{}A{r} {r} 0 0 1 {x} {}V{}A{r} {r} 0 0 1 {} {y}Z",
                    x + r,
                    x + w - r,
                    x + w,
                    y + r,
                    y + h - r,
                    x + w - r,
                    y + h,
                    x + r,
                    y + h - r,
                    y + r,
                    x + r
                )
            }
            other => return Err(format!("unsupported element <{other}>")),
        };
        let dash = match get("stroke-dasharray") {
            Some(v) => v
                .split([' ', ','])
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f32>().map_err(|_| format!("bad dash {v}")))
                .collect::<Result<_, _>>()?,
            None => Vec::new(),
        };
        shapes.push((d, dash));
    }
    Ok(shapes)
}

fn parse_attrs(s: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let eq = rest.find('=').ok_or(format!("bad attributes: {rest}"))?;
        let name = rest[..eq].trim().to_owned();
        let after = rest[eq + 1..].trim_start();
        let after = after.strip_prefix('"').ok_or(format!("unquoted {name}"))?;
        let close = after.find('"').ok_or(format!("unclosed {name}"))?;
        out.push((name, after[..close].to_owned()));
        rest = after[close + 1..].trim_start();
    }
    Ok(out)
}

/// 列の中の操作の番号（`src/lib.rs` の `Op` と同じ）。
const MOVE: f32 = 0.0;
const LINE: f32 = 1.0;
const CUBIC: f32 = 2.0;
const CLOSE: f32 = 3.0;

/// SVG のパスを、操作の番号と座標を並べた列にする（移動 x y、直線 x y、ベジェ x1 y1 x2 y2 x y、閉じる）。
fn segments(d: &str) -> Result<Vec<f32>, String> {
    let tokens = tokenize(d)?;
    let mut out = Vec::new();
    let (mut cx, mut cy, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut i = 0;
    let mut cmd = ' ';
    // 直前の 3 次ベジェの 2 つ目の制御点（S の 1 つ目の制御点はその折り返し）。
    let mut last_c2: Option<(f64, f64)> = None;
    let num = |i: &mut usize| -> Result<f64, String> {
        match tokens.get(*i) {
            Some(Token::Num(n)) => {
                *i += 1;
                Ok(*n)
            }
            _ => Err("expected a number".into()),
        }
    };
    while i < tokens.len() {
        if let Token::Cmd(c) = tokens[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                out.push(CLOSE);
                (cx, cy) = (sx, sy);
                last_c2 = None;
                continue;
            }
        } else if cmd == ' ' {
            return Err("path must start with a command".into());
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                out.extend([MOVE, x as f32, y as f32]);
                (cx, cy, sx, sy) = (x, y, x, y);
                // 続く座標の組は直線。
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                out.extend([LINE, x as f32, y as f32]);
                (cx, cy) = (x, y);
            }
            'H' => {
                let x = num(&mut i)? + ox;
                out.extend([LINE, x as f32, cy as f32]);
                cx = x;
            }
            'V' => {
                let y = num(&mut i)? + if rel { cy } else { 0.0 };
                out.extend([LINE, cx as f32, y as f32]);
                cy = y;
            }
            'C' => {
                let mut v = [0.0; 6];
                for (k, slot) in v.iter_mut().enumerate() {
                    *slot = num(&mut i)? + if k % 2 == 0 { ox } else { oy };
                }
                out.push(CUBIC);
                out.extend(v.iter().map(|&n| n as f32));
                (cx, cy) = (v[4], v[5]);
                last_c2 = Some((v[2], v[3]));
                continue;
            }
            'S' => {
                let (x2, y2) = (num(&mut i)? + ox, num(&mut i)? + oy);
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                let (x1, y1) = last_c2.map_or((cx, cy), |(px, py)| (2.0 * cx - px, 2.0 * cy - py));
                out.push(CUBIC);
                out.extend([x1, y1, x2, y2, x, y].map(|n| n as f32));
                (cx, cy) = (x, y);
                last_c2 = Some((x2, y2));
                continue;
            }
            'A' => {
                let (rx, ry, rot) = (num(&mut i)?, num(&mut i)?, num(&mut i)?);
                let (large, sweep) = (num(&mut i)? != 0.0, num(&mut i)? != 0.0);
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                arc(&mut out, (cx, cy), (rx, ry), rot, large, sweep, (x, y));
                (cx, cy) = (x, y);
            }
            other => return Err(format!("unsupported command {other}")),
        }
        last_c2 = None;
    }
    Ok(out)
}

enum Token {
    Cmd(char),
    Num(f64),
}

/// SVG のパスの文字を、命令と数に分ける（`.5.5`・`1-2`・`1e-3` のような詰めた書き方も読む）。弧の旗（0 か 1）は
/// 区切りなしで続くことがあるが、ここで描く形では区切っているので、ふつうの数として読む。
fn tokenize(d: &str) -> Result<Vec<Token>, String> {
    let b = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            out.push(Token::Cmd(c));
            i += 1;
        } else if c.is_ascii_whitespace() || c == ',' {
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut dot = c == '.';
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !dot {
                    dot = true;
                    i += 1;
                } else if (ch == 'e' || ch == 'E') && i + 1 < b.len() {
                    i += 1;
                    if b[i] == b'-' || b[i] == b'+' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            let n = d[start..i]
                .parse()
                .map_err(|_| format!("bad number {}", &d[start..i]))?;
            out.push(Token::Num(n));
        } else {
            return Err(format!("unexpected {c}"));
        }
    }
    Ok(out)
}

/// SVG の弧（端点の形）を、90 度以下ずつの 3 次ベジェにして足す（SVG 1.1 の付録 F.6.5 の手順）。
fn arc(
    out: &mut Vec<f32>,
    from: (f64, f64),
    r: (f64, f64),
    rot: f64,
    large: bool,
    sweep: bool,
    to: (f64, f64),
) {
    use std::f64::consts::PI;
    let (x1, y1) = from;
    let (x2, y2) = to;
    let (mut rx, mut ry) = (r.0.abs(), r.1.abs());
    if (x1 == x2 && y1 == y2) || rx == 0.0 || ry == 0.0 {
        out.extend([LINE, x2 as f32, y2 as f32]);
        return;
    }
    let phi = rot.to_radians();
    let (sin, cos) = phi.sin_cos();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos * dx + sin * dy;
    let y1p = -sin * dx + cos * dy;
    let lambda = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = (num / den).max(0.0).sqrt();
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cos * cxp - sin * cyp + (x1 + x2) / 2.0;
    let cy = sin * cxp + cos * cyp + (y1 + y2) / 2.0;
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = angle(1.0, 0.0, ux, uy);
    let mut delta = angle(ux, uy, vx, vy);
    if !sweep && delta > 0.0 {
        delta -= 2.0 * PI;
    } else if sweep && delta < 0.0 {
        delta += 2.0 * PI;
    }
    let pieces = (delta.abs() / (PI / 2.0)).ceil().max(1.0) as usize;
    let step = delta / pieces as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |t: f64| {
        let (s, c) = t.sin_cos();
        (
            cx + rx * c * cos - ry * s * sin,
            cy + rx * c * sin + ry * s * cos,
        )
    };
    let deriv = |t: f64| {
        let (s, c) = t.sin_cos();
        (-rx * s * cos - ry * c * sin, -rx * s * sin + ry * c * cos)
    };
    for n in 0..pieces {
        let t0 = theta1 + step * n as f64;
        let t1 = t0 + step;
        let (p0, p3) = (
            point(t0),
            if n + 1 == pieces { (x2, y2) } else { point(t1) },
        );
        let (d0, d1) = (deriv(t0), deriv(t1));
        out.extend([
            CUBIC,
            (p0.0 + k * d0.0) as f32,
            (p0.1 + k * d0.1) as f32,
            (p3.0 - k * d1.0) as f32,
            (p3.1 - k * d1.1) as f32,
            p3.0 as f32,
            p3.1 as f32,
        ]);
    }
}
