//! History の永続キャッシュの形式の指紋（`AMONHEN_CACHE_FORMAT`）。キャッシュに書くものを決めるソース（`cache.rs`、
//! 並びを作る `graph.rs`、`CommitGraph`、amonhen-layout のレコード）のハッシュ。
//!
//! 手で上げる版の数は、上げ忘れると別の形式・別の並べ方のファイルを読んでしまい、並んで動く別のビルド（開発中の
//! worktree の daemon とリリースの daemon など）が同じファイルを互いに書き換え合っていた。形式ごとに別のファイルにする。

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let crates = manifest.parent().unwrap().to_path_buf();
    let mut files = vec![
        crates.join("core/src/cache.rs"),
        crates.join("core/src/graph.rs"),
        crates.join("model/src/graph.rs"),
    ];
    let layout = crates.join("layout/src");
    println!("cargo:rerun-if-changed={}", layout.display());
    collect(&layout, &mut files);
    let mut hash = sha1_smol::Sha1::new();
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        let bytes = std::fs::read(file).unwrap_or_default();
        let relative = file.strip_prefix(&crates).unwrap_or(file);
        hash.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hash.update(&[0]);
        // Windows で CRLF にして取り出しても、同じ指紋になるように。
        let lf: Vec<u8> = bytes.into_iter().filter(|&b| b != b'\r').collect();
        hash.update(&lf);
        hash.update(&[0]);
    }
    println!(
        "cargo:rustc-env=AMONHEN_CACHE_FORMAT={}",
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
