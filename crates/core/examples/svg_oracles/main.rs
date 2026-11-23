//! Writes what the real converters make of the SVG test files, so the Rust ports can be compared against them.
//! Needs Java and sdk-common (Android) or Dart and vector_graphics_compiler (flutter_svg) on this machine.
//!
//! `cargo run -p amonhen-core --example svg_oracles -- <android|flutter> <dir with *.svg> <out dir>`
use std::path::Path;

mod android;
mod flutter;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (which, dir, out) = (&args[1], Path::new(&args[2]), Path::new(&args[3]));
    std::fs::create_dir_all(out).expect("out dir");
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
    names.sort();
    for path in names {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let svg = std::fs::read(&path).expect("read");
        match which.as_str() {
            "android" => {
                let conv = android::converter().expect("no Svg2Vector here");
                match conv.convert(&svg) {
                    Ok(c) => {
                        std::fs::write(out.join(format!("{stem}.xml")), c.xml).unwrap();
                        std::fs::write(out.join(format!("{stem}.txt")), c.messages).unwrap();
                    }
                    Err(e) => {
                        std::fs::write(out.join(format!("{stem}.err")), e.to_string()).unwrap()
                    }
                }
            }
            _ => {
                let conv = flutter::converter().expect("no vector_graphics_compiler here");
                match conv.convert(&svg) {
                    Ok(c) => {
                        std::fs::write(out.join(format!("{stem}.vec")), c.vec).unwrap();
                        std::fs::write(out.join(format!("{stem}.txt")), c.warnings.join("\n"))
                            .unwrap();
                    }
                    Err(e) => {
                        std::fs::write(out.join(format!("{stem}.err")), e.to_string()).unwrap()
                    }
                }
            }
        }
        println!("{stem}");
    }
}
