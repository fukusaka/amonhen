//! VSCode 拡張のアイコンのフォント（`apps/vscode/media/amonhen-icons.ttf`）と、`package.json` の
//! `contributes.icons` を、今の形から作り直す。
//!   cargo run -p amonhen-icons --example vscode

#[path = "../tools/vscode_font.rs"]
mod vscode_font;

fn main() {
    let app = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/vscode");
    std::fs::write(app.join(vscode_font::FONT_PATH), vscode_font::font()).unwrap();
    let package = app.join("package.json");
    let text = std::fs::read_to_string(&package).unwrap();
    std::fs::write(&package, vscode_font::with_contributions(&text)).unwrap();
}
