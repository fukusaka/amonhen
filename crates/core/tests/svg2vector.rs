//! Rust に移した Svg2Vector を、本物（Android Studio の sdk-common 32.1.0）が出したものと比べる。
//! `tests/data/svg/android` の `<名前>.xml`・`.txt`（または例外の `.err`）は `examples/svg_oracles` で作った。
//! `tests/data/svg2vector` は取り込みの端（例外・エラーの行・use の循環・グラデーションの変換など）を試す SVG。

use std::collections::BTreeMap;
use std::path::Path;

use amonhen_core::svg2vector;

/// 要素の木を、属性の並びによらない形に（Java は属性を HashMap の順で書く）。
#[derive(Debug, PartialEq)]
struct Element {
    name: String,
    attributes: BTreeMap<String, String>,
    children: Vec<Element>,
}

fn tree(xml: &str) -> Option<Element> {
    if xml.trim().is_empty() {
        return None;
    }
    let doc = roxmltree::Document::parse(xml).unwrap_or_else(|e| panic!("{e}\n{xml}"));
    fn element(node: roxmltree::Node) -> Element {
        let qualified = |ns: Option<&str>, name: &str| match ns {
            Some(ns) => format!("{{{ns}}}{name}"),
            None => name.to_owned(),
        };
        Element {
            name: qualified(node.tag_name().namespace(), node.tag_name().name()),
            attributes: node
                .attributes()
                .map(|a| (qualified(a.namespace(), a.name()), a.value().to_owned()))
                .collect(),
            children: node
                .children()
                .filter(|c| c.is_element())
                .map(element)
                .collect(),
        }
    }
    Some(element(doc.root_element()))
}

#[test]
fn svg2vector_matches_android_studio() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let mut failures = Vec::new();
    for dir in ["svg", "svg2vector"] {
        compare(&data.join(dir), &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn compare(data: &Path, failures: &mut Vec<String>) {
    let mut names: Vec<_> = std::fs::read_dir(data)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for path in names {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let svg = std::fs::read(&path).unwrap();
        let expected = data.join("android");
        let result = svg2vector::convert(&svg);
        if let Ok(err) = std::fs::read_to_string(expected.join(format!("{name}.err"))) {
            let first = err.lines().next().unwrap_or("").trim();
            match result {
                Err(e) if e == first => {}
                other => failures.push(format!("{name}: expected error {first:?}, got {other:?}")),
            }
            continue;
        }
        let xml = std::fs::read_to_string(expected.join(format!("{name}.xml"))).unwrap();
        let messages = std::fs::read_to_string(expected.join(format!("{name}.txt"))).unwrap();
        match result {
            Err(e) => failures.push(format!("{name}: unexpected error {e}")),
            Ok(converted) => {
                if tree(&converted.xml) != tree(&xml) {
                    failures.push(format!(
                        "{name}: XML differs\n--- expected\n{xml}\n--- actual\n{}",
                        converted.xml
                    ));
                }
                if converted.messages.trim() != messages.trim() {
                    failures.push(format!(
                        "{name}: messages differ\n--- expected\n{messages}\n--- actual\n{}",
                        converted.messages
                    ));
                }
            }
        }
    }
}
