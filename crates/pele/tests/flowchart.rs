//! pele 1.13.0 が描いた flowchart（`crates/core/tests/data/pele`）と、バイト単位で同じに描けるか。

use std::fs;
use std::path::Path;

fn data() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/data/pele")
}

/// 最初に違う所の前後。
fn first_difference(want: &str, got: &str) -> String {
    let (w, g) = (want.as_bytes(), got.as_bytes());
    let at = w
        .iter()
        .zip(g)
        .position(|(a, b)| a != b)
        .unwrap_or(w.len().min(g.len()));
    let around = |s: &str| {
        let mut a = at.saturating_sub(80);
        while !s.is_char_boundary(a) {
            a -= 1;
        }
        let mut b = (at + 80).min(s.len());
        while !s.is_char_boundary(b) {
            b += 1;
        }
        s[a..b].to_string()
    };
    format!(
        "at byte {at}:\n  want: {}\n  got:  {}",
        around(want),
        around(got)
    )
}

#[test]
fn flowchart_docs_match_pele() {
    let dir = data();
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mmd"))
        .collect();
    names.sort();
    assert!(!names.is_empty());
    let mut failed = Vec::new();
    for mmd in &names {
        let input = fs::read_to_string(mmd).unwrap();
        let svg = mmd.with_extension("svg");
        let err = mmd.with_extension("err");
        let name = mmd.file_name().unwrap().to_string_lossy().to_string();
        match amonhen_pele::render(&input) {
            Ok(r) if svg.exists() => {
                let want = fs::read_to_string(&svg).unwrap();
                if r.svg != want {
                    failed.push(format!("{name}: {}", first_difference(&want, &r.svg)));
                }
            }
            Ok(_) => failed.push(format!("{name}: rendered, but pele reports an error")),
            Err(e) if err.exists() => {
                let want = fs::read_to_string(&err).unwrap();
                if e.message != want {
                    failed.push(format!("{name}: error {:?}, want {:?}", e.message, want));
                }
            }
            Err(e) => failed.push(format!("{name}: error {}", e.message)),
        }
    }
    assert!(
        failed.is_empty(),
        "{}/{} differ:\n{}",
        failed.len(),
        names.len(),
        failed.join("\n")
    );
}
