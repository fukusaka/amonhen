//! VSCode 拡張のアイコンのフォントと `contributes.icons` が、今の形から作ったものと同じ（形を変えたら
//! `cargo run -p amonhen-icons --example vscode` で作り直す）。

#[path = "../tools/vscode_font.rs"]
mod vscode_font;

fn app() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/vscode")
}

#[test]
fn font_is_current() {
    let committed = std::fs::read(app().join(vscode_font::FONT_PATH)).unwrap_or_default();
    assert!(
        committed == vscode_font::font(),
        "{} is stale: cargo run -p amonhen-icons --example vscode",
        vscode_font::FONT_PATH
    );
}

#[test]
fn contributions_are_current() {
    // Windows の取り出しは改行を CRLF にすることがある（core.autocrlf）。
    let package = std::fs::read_to_string(app().join("package.json"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        package == vscode_font::with_contributions(&package),
        "contributes.icons in package.json is stale: cargo run -p amonhen-icons --example vscode"
    );
}

#[test]
fn every_icon_has_a_glyph() {
    let font = vscode_font::font();
    // loca（4 バイトずつ、字の数 + 1 個）で、0 番は空、n 番（ICONS の n - 1 番目）には中身がある。
    let tables = u16::from_be_bytes([font[4], font[5]]) as usize;
    let loca = (0..tables)
        .map(|i| &font[12 + 16 * i..28 + 16 * i])
        .find(|r| &r[..4] == b"loca")
        .unwrap();
    let (loca, len) = (
        u32::from_be_bytes(loca[8..12].try_into().unwrap()) as usize,
        u32::from_be_bytes(loca[12..16].try_into().unwrap()) as usize,
    );
    assert_eq!(len, 4 * (amonhen_icons::ICONS.len() + 2));
    let at =
        |n: usize| u32::from_be_bytes(font[loca + 4 * n..loca + 4 * n + 4].try_into().unwrap());
    assert_eq!(at(0), at(1), ".notdef is empty");
    for n in 1..=amonhen_icons::ICONS.len() {
        assert!(
            at(n + 1) > at(n),
            "{} has no outline",
            amonhen_icons::ICONS[n - 1].0
        );
    }
}
