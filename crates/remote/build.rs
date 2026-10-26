//! このビルドの指紋（`AMONHEN_BUILD_ID`）を作る。ワークスペースのソース（`crates/*/src` の下のすべて、各 crate の
//! `Cargo.toml`・`build.rs`、ルートの `Cargo.toml`・`Cargo.lock`）のハッシュ。daemon・SSH 先のエンジンとは、これが
//! 同じときだけつなぐ（`wire::BUILD_ID`）。
//!
//! 手で上げる版の数は、並んで進むブランチが同じ数を取ったり上げ忘れたりすると、形の違う相手を通してしまう。ソースから
//! 作れば、同じソースのビルドだけが同じ指紋になる（どの機械で作っても。改行は LF にそろえて読む）。同じ版のアプリと
//! VSCode 拡張・SSH 先へ送るエンジンは同じソースから作るので、同じ指紋になる。

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.ancestors().nth(2).unwrap().to_path_buf();
    let mut files = vec![root.join("Cargo.toml"), root.join("Cargo.lock")];
    let mut crates: Vec<PathBuf> = std::fs::read_dir(root.join("crates"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    crates.sort();
    for dir in &crates {
        files.push(dir.join("Cargo.toml"));
        if dir.join("build.rs").is_file() {
            files.push(dir.join("build.rs"));
        }
        let src = dir.join("src");
        // フォルダを見張ると、中のファイルが増えた・減ったときにも作り直す。
        println!("cargo:rerun-if-changed={}", src.display());
        collect(&src, &mut files);
    }
    let mut hash = sha1_smol::Sha1::new();
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        let relative = file.strip_prefix(&root).unwrap_or(file);
        hash.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hash.update(&[0]);
        // Windows で CRLF にして取り出しても、同じ指紋になるように。
        let lf: Vec<u8> = bytes.into_iter().filter(|&b| b != b'\r').collect();
        hash.update(&lf);
        hash.update(&[0]);
    }
    println!(
        "cargo:rustc-env=AMONHEN_BUILD_ID={}",
        &hash.digest().to_string()[..16]
    );
}

/// `dir` の下のファイル（名前の順）。
fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}
