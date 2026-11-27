//! VSCode 拡張のアイコンのフォントと `contributes.icons` が、字の番号の表と今の形から作ったものと同じ（形を変えたら
//! `cargo run -p amonhen-icons --example vscode` で作り直す）。

#[path = "../tools/vscode_font.rs"]
mod vscode_font;

use vscode_font::Table;

fn crate_dir() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn app() -> std::path::PathBuf {
    crate_dir().join("../../apps/vscode")
}

/// Windows の取り出しは改行を CRLF にすることがある（core.autocrlf）。
fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn table() -> Table {
    vscode_font::parse(&read(&crate_dir().join(vscode_font::TABLE_PATH)))
}

#[test]
fn every_icon_has_a_codepoint() {
    let text = read(&crate_dir().join(vscode_font::TABLE_PATH));
    let table = vscode_font::parse(&text);
    assert_eq!(
        text,
        vscode_font::format(&table),
        "the table is written as the example writes it"
    );
    assert!(
        vscode_font::assigned(&table) == table,
        "{} lacks new icons: cargo run -p amonhen-icons --example vscode",
        vscode_font::TABLE_PATH
    );
    let mut codes: Vec<u32> = table.iter().map(|(c, _)| *c).collect();
    codes.dedup();
    assert_eq!(codes.len(), table.len(), "a codepoint is used twice");
}

#[test]
fn codepoints_stay_when_an_icon_is_added() {
    // 名前の順で前に来る形を足しても、前からの番号は変わらず、足した形は最後の番号の次になる。
    let table = vec![(0xE000, "pull".to_string()), (0xE001, "old".to_string())];
    let next = vscode_font::assigned(&table);
    assert_eq!(&next[..2], &table[..]);
    let account = next.iter().find(|(_, n)| n == "account").unwrap();
    assert_eq!(account.0, 0xE002);
}

/// 字の作り（輪郭の終わりの点の番号、曲線の上の点かの印）と、点の座標（絶対値）。
type Outline = (Vec<u16>, Vec<u8>, Vec<(i32, i32)>);

/// `glyf` の 1 つの字（このフォントの書き方: 印は点ごとに 1 バイト、座標は差を 2 バイトずつ）を読む。
fn outline(glyph: &[u8]) -> Outline {
    if glyph.is_empty() {
        return Default::default();
    }
    let contours = u16_at(glyph, 0);
    let ends: Vec<u16> = (0..contours)
        .map(|i| u16_at(glyph, 10 + 2 * i) as u16)
        .collect();
    let points = ends.last().map_or(0, |e| *e as usize + 1);
    let flags_at = 10 + 2 * contours + 2 + u16_at(glyph, 10 + 2 * contours);
    let flags = glyph[flags_at..flags_at + points].to_vec();
    let delta = |i: usize| i16::from_be_bytes([glyph[i], glyph[i + 1]]) as i32;
    let (xs, ys) = (flags_at + points, flags_at + 3 * points);
    let (mut x, mut y) = (0, 0);
    let coords = (0..points)
        .map(|n| {
            x += delta(xs + 2 * n);
            y += delta(ys + 2 * n);
            (x, y)
        })
        .collect();
    (ends, flags, coords)
}

/// 字ごとの `glyf` のバイト（loca で切り出す）。
fn glyphs(font: &[u8]) -> Vec<&[u8]> {
    let (loca, glyf) = (font_table(font, b"loca"), font_table(font, b"glyf"));
    let at = |n: usize| u32::from_be_bytes(loca[4 * n..4 * n + 4].try_into().unwrap()) as usize;
    (0..loca.len() / 4 - 1)
        .map(|n| &glyf[at(n)..at(n + 1)])
        .collect()
}

#[test]
fn font_is_current() {
    let committed = std::fs::read(app().join(vscode_font::FONT_PATH)).unwrap_or_default();
    let table = table();
    let made = vscode_font::font(&table);
    if committed == made {
        return;
    }
    let stale = format!(
        "{} is stale: cargo run -p amonhen-icons --example vscode",
        vscode_font::FONT_PATH
    );
    assert!(!committed.is_empty(), "{stale}");
    // 輪郭の計算（kurbo）は、rustc と CPU によって、丸めの境目にある座標が 1 単位（1/62.5 px）ずれることがある。
    // 字の作りは同じで、座標の差が 1 単位までなら同じフォントとみなす。字のほかの表は同じバイトであること。
    for tag in [
        b"OS/2", b"cmap", b"hhea", b"hmtx", b"loca", b"maxp", b"name", b"post",
    ] {
        assert!(
            font_table(&committed, tag) == font_table(&made, tag),
            "{stale} ({} differs)",
            String::from_utf8_lossy(tag)
        );
    }
    let head = |f: &[u8]| {
        let mut h = font_table(f, b"head").to_vec();
        h[8..12].fill(0); // checkSumAdjustment
        h
    };
    assert!(head(&committed) == head(&made), "{stale} (head differs)");
    let names = ["(notdef)"]
        .into_iter()
        .chain(vscode_font::glyph_bytes(&table).into_iter().map(|(n, _)| n));
    let differ: Vec<&str> = names
        .zip(glyphs(&committed).into_iter().zip(glyphs(&made)))
        .filter(|(_, (old, new))| {
            let ((oe, of, oc), (ne, nf, nc)) = (outline(old), outline(new));
            oe != ne
                || of != nf
                || oc
                    .iter()
                    .zip(&nc)
                    .any(|(a, b)| (a.0 - b.0).abs() > 1 || (a.1 - b.1).abs() > 1)
        })
        .map(|(name, _)| name)
        .collect();
    assert!(differ.is_empty(), "{stale} (glyphs differ: {differ:?})");
}

#[test]
fn contributions_are_current() {
    let package = read(&app().join("package.json"));
    assert!(
        package == vscode_font::with_contributions(&package, &table()),
        "contributes.icons in package.json is stale: cargo run -p amonhen-icons --example vscode"
    );
}

/// フォントの表（tag）の中身。
fn font_table<'a>(font: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
    let tables = u16::from_be_bytes([font[4], font[5]]) as usize;
    let record = (0..tables)
        .map(|i| &font[12 + 16 * i..28 + 16 * i])
        .find(|r| &r[..4] == tag)
        .unwrap();
    let at = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
    let len = u32::from_be_bytes(record[12..16].try_into().unwrap()) as usize;
    &font[at..at + len]
}

fn u16_at(b: &[u8], i: usize) -> usize {
    u16::from_be_bytes([b[i], b[i + 1]]) as usize
}

#[test]
fn codepoints_reach_their_shapes() {
    // 使わなくなった名前（old）の番号は空の字に、それ以外は形のある字になり、番号の表のとおりに引ける。
    let table = vec![
        (0xE000, "pull".to_string()),
        (0xE001, "old".to_string()),
        (0xE002, "push".to_string()),
    ];
    let font = vscode_font::font(&table);
    let loca = font_table(&font, b"loca");
    let size = |id: usize| {
        let at = |n: usize| u32::from_be_bytes(loca[4 * n..4 * n + 4].try_into().unwrap());
        at(id + 1) - at(id)
    };
    // cmap の形式 4 の最初の範囲を、字の番号の列で引く。
    let cmap = font_table(&font, b"cmap");
    let sub = &cmap[20..];
    // 見出し 14 バイト、終わり 2 つ、0、始まり 2 つ、idDelta 2 つ、idRangeOffset 2 つ、字の番号の列。
    let (start, end) = (u16_at(sub, 20), u16_at(sub, 14));
    let glyph_ids = 14 + 4 + 2 + 4 + 4 + 4;
    let id = |code: usize| u16_at(sub, glyph_ids + 2 * (code - start));
    assert_eq!((start, end), (0xE000, 0xE002));
    assert_eq!([id(0xE000), id(0xE001), id(0xE002)], [1, 0, 2]);
    assert_eq!(size(0), 0, ".notdef is empty");
    assert!(size(1) > 0 && size(2) > 0);
    assert_eq!(loca.len(), 4 * 4);
}
