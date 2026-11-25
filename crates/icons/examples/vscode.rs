//! VSCode 拡張の `apps/vscode/media/icons/` を、今の形から作り直す。
//!   cargo run -p amonhen-icons --example vscode

fn main() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/vscode/media/icons");
    let _ = std::fs::remove_dir_all(&dir);
    for (path, svg) in amonhen_icons::vscode_files() {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, svg).unwrap();
    }
}
