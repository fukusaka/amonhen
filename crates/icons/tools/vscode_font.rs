//! VSCode 拡張のアイコンのフォント（`apps/vscode/media/amonhen-icons.ttf`）と、それを登録する `package.json` の
//! `contributes.icons`。VSCode は拡張の SVG を画像として描き、テーマの色を受け継げないので、形をフォントの字にして
//! codicon と同じように描かせる（テーマのアイコンの色・ハイコントラスト・選んだ行の色に従う）。
//!
//! 字は 16px の 1em に、太さ 1px の線を輪郭に直したもの（線は 1 倍の画面の画素に合わせてから直す）。字の番号は
//! `ICONS` の並び（名前の順）で U+E000 から。example の `vscode` と、作ったものが古くないかのテストが使う。

use amonhen_icons::{ICONS, Icon, Op};
use kurbo::{BezPath, Cap, Circle, CubicBez, Join, PathEl, Point, Shape, Stroke, StrokeOpts};

/// 字の大きさ（1em）の単位の数。
const EM: f64 = 1000.0;
/// 描く大きさ（px）。
const SIZE: f64 = 16.0;
/// 最初の字の番号（私用領域）。
pub const FIRST: u32 = 0xE000;
/// `package.json` から見たフォントの場所。
pub const FONT_PATH: &str = "media/amonhen-icons.ttf";

/// 線を塗りの輪郭に（px の単位、y は下向き）。
fn outline(icon: &Icon) -> BezPath {
    let mut out = BezPath::new();
    for stroke in icon.strokes(SIZE as f32, 1.0, 1.0) {
        let mut path = BezPath::new();
        // 長さ 0 の線（丸い端の点）は、kurbo では何も描かれないので丸を足す。
        let mut dots = Vec::new();
        let mut start = None;
        let s = &stroke.segments;
        let mut i = 0;
        while i < s.len() {
            let op = Op::from_code(s[i]).expect("op");
            let c: Vec<f64> = s[i + 1..i + 1 + op.arity()]
                .iter()
                .map(|v| *v as f64)
                .collect();
            match op {
                Op::Move => {
                    let p = Point::new(c[0], c[1]);
                    path.move_to(p);
                    start = Some(p);
                }
                Op::Line => {
                    let p = Point::new(c[0], c[1]);
                    if start.is_some_and(|s| s.distance(p) < 1e-6) {
                        dots.push(p);
                    }
                    path.line_to(p);
                    start = None;
                }
                Op::Cubic => {
                    path.curve_to((c[0], c[1]), (c[2], c[3]), (c[4], c[5]));
                    start = None;
                }
                Op::Close => {
                    path.close_path();
                    start = None;
                }
            }
            i += 1 + op.arity();
        }
        let mut style = Stroke::new(1.0)
            .with_caps(Cap::Round)
            .with_join(Join::Round);
        if !stroke.dash.is_empty() {
            style = style.with_dashes(0.0, stroke.dash.iter().map(|d| *d as f64));
        }
        out.extend(kurbo::stroke(
            path.iter(),
            &style,
            &StrokeOpts::default(),
            0.01,
        ));
        if stroke.dash.is_empty() {
            for p in dots {
                out.extend(Circle::new(p, 0.5).path_elements(0.01));
            }
        }
    }
    out
}

/// 字の点（x、y、曲線の上の点か）。単位は 1em = [`EM`]、y は上向き。
type Contour = Vec<(i16, i16, bool)>;

/// 輪郭を TrueType の輪郭（直線と 2 次ベジェ）にする。
fn contours(path: &BezPath) -> Vec<Contour> {
    let unit = EM / SIZE;
    let at = |p: Point| {
        (
            (p.x * unit).round() as i16,
            ((SIZE - p.y) * unit).round() as i16,
        )
    };
    let mut done = Vec::new();
    let mut current: Contour = Vec::new();
    let finish = |c: &mut Contour, done: &mut Vec<Contour>| {
        c.dedup();
        if c.len() > 1 && c.first() == c.last() {
            c.pop();
        }
        if c.len() >= 3 {
            done.push(std::mem::take(c));
        } else {
            c.clear();
        }
    };
    let mut last = Point::ZERO;
    for el in path.iter() {
        match el {
            PathEl::MoveTo(p) => {
                finish(&mut current, &mut done);
                let (x, y) = at(p);
                current.push((x, y, true));
            }
            PathEl::LineTo(p) => {
                let (x, y) = at(p);
                current.push((x, y, true));
            }
            PathEl::QuadTo(c, p) => {
                let ((cx, cy), (x, y)) = (at(c), at(p));
                current.extend([(cx, cy, false), (x, y, true)]);
            }
            PathEl::CurveTo(c1, c2, p) => {
                for (_, _, q) in CubicBez::new(last, c1, c2, p).to_quads(0.005) {
                    let ((cx, cy), (x, y)) = (at(q.p1), at(q.p2));
                    current.extend([(cx, cy, false), (x, y, true)]);
                }
            }
            PathEl::ClosePath => finish(&mut current, &mut done),
        }
        if let Some(p) = el.end_point() {
            last = p;
        }
    }
    finish(&mut current, &mut done);
    done
}

/// `glyf` の 1 つの字。
fn glyph(contours: &[Contour]) -> Vec<u8> {
    let mut b = Vec::new();
    if contours.is_empty() {
        return b;
    }
    let points: Vec<_> = contours.iter().flatten().copied().collect();
    let min_x = points.iter().map(|p| p.0).min().unwrap();
    let max_x = points.iter().map(|p| p.0).max().unwrap();
    let min_y = points.iter().map(|p| p.1).min().unwrap();
    let max_y = points.iter().map(|p| p.1).max().unwrap();
    put16(&mut b, contours.len() as u16);
    for v in [min_x, min_y, max_x, max_y] {
        put16(&mut b, v as u16);
    }
    let mut end = 0;
    for c in contours {
        end += c.len();
        put16(&mut b, (end - 1) as u16);
    }
    put16(&mut b, 0); // 命令（ヒント）はなし
    // 印は曲線の上の点かだけ。座標は前の点からの差を 2 バイトずつ。
    b.extend(points.iter().map(|p| u8::from(p.2)));
    let (mut x, mut y) = (0i16, 0i16);
    let mut ys = Vec::new();
    for p in &points {
        put16(&mut b, (p.0 - x) as u16);
        put16(&mut ys, (p.1 - y) as u16);
        (x, y) = (p.0, p.1);
    }
    b.extend(ys);
    b
}

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend(v.to_be_bytes());
}

fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend(v.to_be_bytes());
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// フォント（TrueType）。0 番の字は空、`ICONS` の n 番目は U+E000+n。
pub fn font() -> Vec<u8> {
    let count = ICONS.len() as u16 + 1;
    let em = EM as u16;
    let mut glyf = Vec::new();
    // loca は字ごとの始まりと、最後の終わり。0 番（.notdef）は空。
    let mut loca = Vec::new();
    put32(&mut loca, 0);
    put32(&mut loca, 0);
    let (mut max_points, mut max_contours) = (0, 0);
    for (_, icon) in ICONS {
        let c = contours(&outline(icon));
        max_points = c.iter().map(Vec::len).sum::<usize>().max(max_points);
        max_contours = c.len().max(max_contours);
        glyf.extend(glyph(&c));
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
        put32(&mut loca, glyf.len() as u32);
    }
    // 字の箱は 1em の正方形。行の高さも 1em で、下の端が基準の線（codicon と同じ置き方）。
    let mut head = Vec::new();
    put32(&mut head, 0x0001_0000);
    put32(&mut head, 0x0001_0000);
    put32(&mut head, 0); // checkSumAdjustment（後で入れる）
    put32(&mut head, 0x5F0F_3CF5);
    put16(&mut head, 0x000B);
    put16(&mut head, em);
    head.extend([0; 16]); // 作った日と変えた日（入れない。作り直しても同じバイトになるように）
    for v in [0, 0, em, em] {
        put16(&mut head, v);
    }
    for v in [0, 8, 2, 1, 0] {
        put16(&mut head, v); // macStyle、lowestRecPPEM、fontDirectionHint、indexToLocFormat（loca は 4 バイト）、glyphDataFormat
    }
    let mut hhea = Vec::new();
    put32(&mut hhea, 0x0001_0000);
    // 上・下・行間、幅の最大、左右の余白の最小、右端の最大、カーソルの傾き（縦・横・ずれ）、予約 4 つ、形式
    for v in [em, 0, 0, em, 0, 0, em, 1, 0, 0, 0, 0, 0, 0, 0] {
        put16(&mut hhea, v);
    }
    put16(&mut hhea, count);
    let mut hmtx = Vec::new();
    for _ in 0..count {
        put16(&mut hmtx, em);
        put16(&mut hmtx, 0);
    }
    let mut maxp = Vec::new();
    put32(&mut maxp, 0x0001_0000);
    put16(&mut maxp, count);
    put16(&mut maxp, max_points as u16);
    put16(&mut maxp, max_contours as u16);
    for v in [0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0] {
        put16(&mut maxp, v);
    }
    let last = (FIRST + ICONS.len() as u32 - 1) as u16;
    let mut os2 = Vec::new();
    for v in [4, em, 400, 5, 0] {
        put16(&mut os2, v); // version、xAvgCharWidth、usWeightClass、usWidthClass、fsType
    }
    os2.extend([0; 20 + 2 + 10 + 16]); // 上付き・下付き・取り消し線、sFamilyClass、panose、ulUnicodeRange
    os2.extend(*b"AMHN");
    for v in [0x0040, FIRST as u16, last, em, 0, 0, em, 0] {
        put16(&mut os2, v); // fsSelection、最初と最後の字、typo の上・下・行間、win の上・下
    }
    put32(&mut os2, 1); // ulCodePageRange1（Latin 1）
    put32(&mut os2, 0);
    for v in [0, 0, 0, 0, 0] {
        put16(&mut os2, v);
    }
    // cmap: 形式 4、U+E000 から続く 1 つの範囲と、最後の 0xFFFF。
    let mut sub = Vec::new();
    for v in [4, 0, 0, 4, 4, 1, 0, last, 0xFFFF, 0, FIRST as u16, 0xFFFF] {
        put16(&mut sub, v);
    }
    put16(&mut sub, 1u16.wrapping_sub(FIRST as u16)); // idDelta: U+E000 → 1 番の字
    for v in [1, 0, 0] {
        put16(&mut sub, v);
    }
    let len = sub.len() as u16;
    sub[2..4].copy_from_slice(&len.to_be_bytes());
    let mut cmap = Vec::new();
    for v in [0, 2, 0, 3] {
        put16(&mut cmap, v);
    }
    put32(&mut cmap, 20);
    for v in [3, 1] {
        put16(&mut cmap, v);
    }
    put32(&mut cmap, 20);
    cmap.extend(sub);
    let names = [
        (1, "Amon Hen Icons"),
        (2, "Regular"),
        (4, "Amon Hen Icons"),
        (6, "AmonHenIcons"),
    ];
    let mut records = Vec::new();
    let mut strings = Vec::new();
    for (id, text) in names {
        let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for v in [3, 1, 0x409, id, utf16.len() as u16, strings.len() as u16] {
            put16(&mut records, v);
        }
        strings.extend(utf16);
    }
    let mut name = Vec::new();
    for v in [0, names.len() as u16, 6 + records.len() as u16] {
        put16(&mut name, v);
    }
    name.extend(records);
    name.extend(strings);
    let mut post = Vec::new();
    put32(&mut post, 0x0003_0000);
    post.extend([0; 28]);
    let tables: [(&[u8; 4], Vec<u8>); 10] = [
        (b"OS/2", os2),
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
        (b"name", name),
        (b"post", post),
    ];
    let mut out = Vec::new();
    put32(&mut out, 0x0001_0000);
    for v in [tables.len() as u16, 128, 3, tables.len() as u16 * 16 - 128] {
        put16(&mut out, v);
    }
    let mut offset = 12 + 16 * tables.len() as u32;
    let mut body = Vec::new();
    let mut head_at = 0;
    for (tag, data) in &tables {
        if *tag == b"head" {
            head_at = offset as usize;
        }
        out.extend(*tag);
        put32(&mut out, checksum(data));
        put32(&mut out, offset);
        put32(&mut out, data.len() as u32);
        body.extend(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() as u32 + body.len() as u32;
    }
    out.extend(body);
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adjust.to_be_bytes());
    out
}

/// `package.json` の `contributes.icons`（`"icons": {` から対の `}` まで、2 段の字下げ）。
pub fn contributions() -> String {
    let rows: Vec<String> = ICONS
        .iter()
        .enumerate()
        .map(|(n, (name, _))| {
            format!(
                "      \"amonhen-{name}\": {{\n        \"description\": \"Amon Hen: {name}\",\n        \"default\": {{ \"fontPath\": \"{FONT_PATH}\", \"fontCharacter\": \"\\\\{:X}\" }}\n      }}",
                FIRST + n as u32
            )
        })
        .collect();
    format!("\"icons\": {{\n{}\n    }}", rows.join(",\n"))
}

/// `package.json` の中の `contributes.icons` を、今の形からのものに置き換える。
pub fn with_contributions(package: &str) -> String {
    const KEY: &str = "\n    \"icons\": {";
    let start = package
        .find(KEY)
        .expect("contributes.icons in package.json")
        + 5;
    // 対の `}` まで（中に文字列の `{`・`}` はない）。
    let mut depth = 0;
    let mut end = start;
    for (i, ch) in package[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    format!(
        "{}{}{}",
        &package[..start],
        contributions(),
        &package[end..]
    )
}
