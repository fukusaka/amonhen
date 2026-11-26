//! アプリのアイコン（`res/AppIcon.ico`、`scripts/make-icon.swift --windows` で作る）を exe のリソースに入れる。
//! リソースコンパイラ（rc.exe）を使わず、.ico から .res を組み立ててリンカに渡す（MSVC のリンカは .res を
//! そのまま受け取る）。アイコンの番号は 1（窓のアイコンも `LoadImageW` でこの番号を読む）。

use std::path::PathBuf;

/// アイコンのグループの番号（`app.rs` の `APP_ICON` と同じ）。
const GROUP: u16 = 1;
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    let ico = "res/AppIcon.ico";
    println!("cargo:rerun-if-changed={ico}");
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target != "windows" || env != "msvc" {
        return;
    }
    let data = std::fs::read(ico).expect("res/AppIcon.ico を読めない");
    let res = resource(&data);
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.res");
    std::fs::write(&out, res).expect("app.res を書けない");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

/// .ico（ICONDIR と画像）から、画像ごとの RT_ICON と、それをまとめる RT_GROUP_ICON の .res を作る。
fn resource(ico: &[u8]) -> Vec<u8> {
    let u16_at = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(ico[at..at + 4].try_into().unwrap());
    assert!(u16_at(0) == 0 && u16_at(2) == 1, "アイコンの .ico ではない");
    let count = u16_at(4) as usize;
    // .res は空の項目で始まる。
    let mut res = Vec::new();
    entry(&mut res, 0, 0, &[]);
    let mut group = Vec::new();
    for at in [0u16, 1, count as u16] {
        group.extend_from_slice(&at.to_le_bytes());
    }
    for i in 0..count {
        let at = 6 + 16 * i;
        let (size, offset) = (u32_at(at + 8) as usize, u32_at(at + 12) as usize);
        let id = i as u16 + 1;
        entry(&mut res, RT_ICON, id, &ico[offset..offset + size]);
        // GRPICONDIRENTRY: ICONDIRENTRY の最初の 12 バイトと、ファイルの位置の代わりにリソースの番号。
        group.extend_from_slice(&ico[at..at + 12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    entry(&mut res, RT_GROUP_ICON, GROUP, &group);
    res
}

/// .res の 1 項目（種類と名前は番号）。見出しと中身は 4 バイトの境目にそろえる。
fn entry(res: &mut Vec<u8>, kind: u16, name: u16, data: &[u8]) {
    let mut put = |bytes: &[u8]| res.extend_from_slice(bytes);
    put(&(data.len() as u32).to_le_bytes());
    put(&32u32.to_le_bytes());
    put(&[0xff, 0xff]);
    put(&kind.to_le_bytes());
    put(&[0xff, 0xff]);
    put(&name.to_le_bytes());
    put(&0u32.to_le_bytes()); // DataVersion
    // MemoryFlags（空の項目は 0、ほかは MOVEABLE | PURE | DISCARDABLE）と、言語（中立）。
    put(&(if kind == 0 { 0u16 } else { 0x1030 }).to_le_bytes());
    put(&0u16.to_le_bytes());
    put(&0u32.to_le_bytes()); // Version
    put(&0u32.to_le_bytes()); // Characteristics
    put(data);
    while !res.len().is_multiple_of(4) {
        res.push(0);
    }
}
