//! `svg/*.svg` を読み、形ごとの線のパス（SVG のパスの書き方）と破線を `icons.rs` に書き出す。
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
            writeln!(out, "        Shape {{ d: {d:?}, dash: &{dash:?} }},").unwrap();
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
