//! VSCode 拡張のアイコンのフォント（`apps/vscode/media/amonhen-icons.ttf`）と、`package.json` の
//! `contributes.icons` を、今の形から作り直す。足した形には字の番号を振り、表（`tools/vscode-codepoints.tsv`）に足す。
//!   cargo run -p amonhen-icons --example vscode

#[path = "../tools/vscode_font.rs"]
mod vscode_font;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let table_path = dir.join(vscode_font::TABLE_PATH);
    let table = vscode_font::parse(&std::fs::read_to_string(&table_path).unwrap_or_default());
    let table = vscode_font::assigned(&table);
    std::fs::write(&table_path, vscode_font::format(&table)).unwrap();
    let app = dir.join("../../apps/vscode");
    std::fs::write(app.join(vscode_font::FONT_PATH), vscode_font::font(&table)).unwrap();
    let package = app.join("package.json");
    let text = std::fs::read_to_string(&package).unwrap();
    std::fs::write(&package, vscode_font::with_contributions(&text, &table)).unwrap();
}
