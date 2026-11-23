//! Rust に移した vector_graphics_compiler を、本物の vgc 1.3.6 が作った `.vec`（失敗するものは `.err`）と比べる。

use std::path::{Path, PathBuf};

use amonhen_core::svgflutter::vec_to_svg;
use amonhen_core::vgc::compile;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// `dir` の SVG と、`expected` の中の同じ名前の `.vec`・`.err`。
fn pairs(dir: &Path, expected: &Path) -> Vec<(String, PathBuf, PathBuf)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .map(|p| {
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            (stem, p, expected.to_owned())
        })
        .collect();
    out.sort();
    out
}

/// 違ったときの手がかり（SVG に戻したものの最初の違い）。
fn explain(actual: &[u8], expected: &[u8]) -> String {
    let a = vec_to_svg(actual).unwrap_or_else(|e| format!("(not decodable: {e})"));
    let b = vec_to_svg(expected).unwrap_or_else(|e| format!("(not decodable: {e})"));
    let first = actual
        .iter()
        .zip(expected)
        .position(|(x, y)| x != y)
        .unwrap_or(actual.len().min(expected.len()));
    let line = a
        .lines()
        .zip(b.lines())
        .find(|(x, y)| x != y)
        .map(|(x, y)| format!("\n  port: {x}\n  vgc:  {y}"))
        .unwrap_or_default();
    format!(
        "{} vs {} bytes, first difference at byte {first}{line}",
        actual.len(),
        expected.len()
    )
}

#[test]
fn vgc_port_matches_the_real_compiler() {
    let data = data();
    let mut all = pairs(&data.join("svg"), &data.join("svg/flutter"));
    all.extend(pairs(&data.join("vec"), &data.join("vec")));
    assert!(all.len() > 30);
    let mut failures = Vec::new();
    for (name, svg, expected) in &all {
        let input = std::fs::read(svg).unwrap();
        let vec = expected.join(format!("{name}.vec"));
        let err = expected.join(format!("{name}.err"));
        match compile(&input) {
            Ok(compiled) => {
                if let Ok(expected) = std::fs::read(&vec) {
                    if compiled.vec != expected {
                        failures.push(format!("{name}: {}", explain(&compiled.vec, &expected)));
                    }
                } else {
                    let err = std::fs::read_to_string(&err).unwrap_or_default();
                    failures.push(format!("{name}: compiled, but vgc failed with {err:?}"));
                }
            }
            Err(e) => match std::fs::read_to_string(&err) {
                Ok(expected) if expected.trim_end() == e.to_string() => {}
                Ok(expected) => failures.push(format!(
                    "{name}: error {:?}, vgc {expected:?}",
                    e.to_string()
                )),
                Err(_) => failures.push(format!("{name}: failed with {e}, vgc compiled it")),
            },
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn vgc_port_reports_warnings() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image width="5" height="5" href="data:image/svg+xml;base64,PHN2Zy8+"/><text font-weight="heavy">a</text></svg>"#;
    let compiled = compile(svg).unwrap();
    assert_eq!(
        compiled.warnings,
        [
            "Warning: Unsupported image format svg+xml",
            "Warning: Invalid \"font-weight\": heavy. Ignoring invalid value."
        ]
    );
    let error = compile(b"<svg><rect/></svg>").unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("Bad state: SVG did not specify dimensions"),
        "{error}"
    );
}
