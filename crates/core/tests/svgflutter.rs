//! 本物の vector_graphics_compiler で変換する。手元に Dart と pub のキャッシュの vgc がなければ何もしない。

use std::path::Path;
use std::time::Instant;

use amonhen_core::svgflutter::{converter, vec_to_svg};

const FIXTURES: [&str; 6] = [
    "icon",
    "gradient",
    "style_filter",
    "text",
    "pattern_image",
    "blur",
];

#[test]
fn svgflutter_converts_with_the_real_compiler() {
    let Some(converter) = converter() else {
        eprintln!("skipped: no Dart or vector_graphics_compiler here");
        return;
    };
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/vec");
    for (i, name) in FIXTURES.iter().enumerate() {
        let svg = std::fs::read(data.join(format!("{name}.svg"))).unwrap();
        let started = Instant::now();
        let converted = converter.convert(&svg).unwrap();
        eprintln!(
            "{name}: {} ms{}",
            started.elapsed().as_millis(),
            if i == 0 { " (first)" } else { "" }
        );
        assert_eq!(converted.version, converter.version());
        assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
        // 試しの .vec は vgc 1.3.6 で作った
        if converter.version() == "1.3.6" {
            let expected = std::fs::read(data.join(format!("{name}.vec"))).unwrap();
            assert!(converted.vec == expected, "{name}.vec differs");
        }
        let out = vec_to_svg(&converted.vec).unwrap();
        assert!(out.ends_with("</svg>"));
    }
}

#[test]
fn svgflutter_reports_warnings_and_failures() {
    let Some(converter) = converter() else {
        eprintln!("skipped: no Dart or vector_graphics_compiler here");
        return;
    };
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image width="5" height="5" href="data:image/svg+xml;base64,PHN2Zy8+"/></svg>"#;
    let converted = converter.convert(svg).unwrap();
    assert_eq!(
        converted.warnings,
        ["Warning: Unsupported image format svg+xml"]
    );

    let error = converter.convert(b"<svg><rect/></svg>").unwrap_err();
    assert!(
        error.to_string().contains("SVG did not specify dimensions"),
        "{error}"
    );
}
