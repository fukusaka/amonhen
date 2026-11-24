//! 画像（ノードの `image`・HTML の `<IMG>`）: 呼ぶ側が渡す読み手でバイトを読み、大きさを頭から読んで、SVG に
//! `data:` の URI で埋める。形式は PNG・JPEG・GIF・WebP・SVG（中身はほどかず、大きさだけを読む）。
//!
//! 読み手がない・読めない・大きすぎる・上限を超えたものは描かない（呼ぶ側が枠と注記にする）。上限は 1 枚の
//! バイト数と画素数・図ぜんぶで埋めるバイト数と画素数（同じ画像を何度使っても、そのたびに数える）・読む画像の数。

use std::collections::HashMap;
use std::sync::Arc;

/// 画像の読み手。`src` は文に書いたままの値（`image="img/a.png"` の `img/a.png`）。読めなければ None。
/// どこから読むか（パスの決まり）は呼ぶ側が決める。
pub type Loader = dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync;

/// 1 枚のバイト数の上限。
pub const MAX_BYTES: usize = 1024 * 1024;
/// 図ぜんぶで埋めるバイト数の上限（base64 で 4/3 倍になる。出力の上限 400 万字に収まるように）。
pub const MAX_TOTAL: usize = 2 * 1024 * 1024;
/// 読む画像（違う `src`）の数の上限。
pub const MAX_COUNT: usize = 32;
/// 1 枚の画素数の上限（PNG・JPEG・GIF・WebP。描き手は画像をほどいてから縮めるので、バイト数が小さくても画素が
/// 多いと重い。一色の 8192 × 8192 の PNG は 228 KB だが、CoreSVG で 1.1 秒・3.8 GB、librsvg で 2 秒・1 GB
/// かかった。2048 × 2048 は CoreSVG で 0.09 秒・0.26 GB、librsvg で 0.14 秒・0.08 GB）。
pub const MAX_PIXELS: f64 = 2048.0 * 2048.0;
/// 図ぜんぶで描く画素数の上限（同じ画像も使うたびに数える）。
pub const MAX_TOTAL_PIXELS: f64 = 2.0 * MAX_PIXELS;
/// 大きさの上限（点）。
const MAX_SIDE: f64 = 20_000.0;

/// 読めた画像。
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    /// 画素の大きさ（SVG は文書の単位を px として）。
    pub px: (f64, f64),
    /// `data:` の URI。
    pub uri: Arc<str>,
    bytes: usize,
    /// ほどいたときの画素数（SVG は 0）。
    pixels: f64,
}

impl Picture {
    /// 点での大きさ（1 画素を `scale` 点として）。
    pub fn size(&self, scale: f64) -> (f64, f64) {
        (self.px.0 * scale, self.px.1 * scale)
    }
}

/// 図ひとつぶんの画像（読んだものを覚え、上限を数える）。
pub struct Images<'a> {
    loader: Option<&'a Loader>,
    cache: HashMap<String, Option<Arc<Picture>>>,
    total: usize,
    pixels: f64,
}

impl<'a> Images<'a> {
    pub fn new(loader: Option<&'a Loader>) -> Self {
        Images {
            loader,
            cache: HashMap::new(),
            total: 0,
            pixels: 0.0,
        }
    }

    /// `src` の画像を 1 回使う。読めない・上限を超えたら None。
    pub fn get(&mut self, src: &str) -> Option<Arc<Picture>> {
        let loader = self.loader?;
        let src = src.trim();
        if src.is_empty() {
            return None;
        }
        if !self.cache.contains_key(src) {
            if self.cache.len() >= MAX_COUNT {
                return None;
            }
            let picture = loader(src)
                .filter(|b| b.len() <= MAX_BYTES)
                .and_then(|b| read(&b))
                .map(Arc::new);
            self.cache.insert(src.to_owned(), picture);
        }
        let picture = self.cache.get(src)?.clone()?;
        if self.total + picture.bytes > MAX_TOTAL || self.pixels + picture.pixels > MAX_TOTAL_PIXELS
        {
            return None;
        }
        self.total += picture.bytes;
        self.pixels += picture.pixels;
        Some(picture)
    }
}

/// バイトから画像にする（大きさの読めない・知らない形式・画素の多すぎるものは None）。
pub fn read(bytes: &[u8]) -> Option<Picture> {
    let (mime, w, h) = probe(bytes)?;
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let pixels = if mime == "image/svg+xml" { 0.0 } else { w * h };
    if pixels > MAX_PIXELS {
        return None;
    }
    let mut uri = String::with_capacity(bytes.len() * 4 / 3 + 40);
    uri.push_str("data:");
    uri.push_str(mime);
    uri.push_str(";base64,");
    base64(bytes, &mut uri);
    Some(Picture {
        px: (w.min(MAX_SIDE), h.min(MAX_SIDE)),
        uri: uri.into(),
        bytes: bytes.len(),
        pixels,
    })
}

/// 形式（MIME）と画素の大きさ。
pub fn probe(b: &[u8]) -> Option<(&'static str, f64, f64)> {
    if let Some((w, h)) = png(b) {
        return Some(("image/png", w as f64, h as f64));
    }
    if let Some((w, h)) = jpeg(b) {
        return Some(("image/jpeg", w as f64, h as f64));
    }
    if let Some((w, h)) = gif(b) {
        return Some(("image/gif", w as f64, h as f64));
    }
    if let Some((w, h)) = webp(b) {
        return Some(("image/webp", w as f64, h as f64));
    }
    svg(b).map(|(w, h)| ("image/svg+xml", w, h))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn le24(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 3)?;
    Some(s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16)
}

fn nonzero((w, h): (u32, u32)) -> Option<(u32, u32)> {
    (w > 0 && h > 0).then_some((w, h))
}

/// PNG: 署名の後の IHDR の幅と高さ。
fn png(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") || b.get(12..16)? != b"IHDR" {
        return None;
    }
    nonzero((be32(b, 16)?, be32(b, 20)?))
}

/// JPEG: 区切り（marker）をたどり、SOF（C0〜CF の C4・C8・CC を除く）の高さと幅。
fn jpeg(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let m = b[i + 1];
        match m {
            // 詰め物
            0xFF => {
                i += 1;
                continue;
            }
            // 長さのない区切り
            0x01 | 0xD0..=0xD7 => {
                i += 2;
                continue;
            }
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let len = be16(b, i + 2)? as usize;
        if len < 2 {
            return None;
        }
        if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
            let h = be16(b, i + 5)? as u32;
            let w = be16(b, i + 7)? as u32;
            return nonzero((w, h));
        }
        i += 2 + len;
    }
    None
}

/// GIF: 論理画面の幅と高さ。
fn gif(b: &[u8]) -> Option<(u32, u32)> {
    if !(b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        return None;
    }
    nonzero((le16(b, 6)? as u32, le16(b, 8)? as u32))
}

/// WebP: RIFF の中の VP8（非可逆）・VP8L（可逆）・VP8X（拡張）。
fn webp(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(0..4)? != b"RIFF" || b.get(8..12)? != b"WEBP" {
        return None;
    }
    match b.get(12..16)? {
        b"VP8 " => {
            // フレームの頭（3 バイト）、開始の印 9D 01 2A、14 ビットの幅と高さ
            if b.get(23..26)? != [0x9D, 0x01, 0x2A] {
                return None;
            }
            nonzero((
                (le16(b, 26)? & 0x3FFF) as u32,
                (le16(b, 28)? & 0x3FFF) as u32,
            ))
        }
        b"VP8L" => {
            if *b.get(20)? != 0x2F {
                return None;
            }
            let v = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
            nonzero(((v & 0x3FFF) + 1, ((v >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => nonzero((le24(b, 24)? + 1, le24(b, 27)? + 1)),
        _ => None,
    }
}

/// SVG: 根の `<svg>` の `width`・`height`（単位つき。百分率なら使わない）、なければ `viewBox`。
fn svg(b: &[u8]) -> Option<(f64, f64)> {
    // 頭の 64 KiB（文字の途中で切れたら、その前まで）
    let s = &b[..b.len().min(64 * 1024)];
    let text = match std::str::from_utf8(s) {
        Ok(t) => t,
        Err(e) => std::str::from_utf8(&s[..e.valid_up_to()]).ok()?,
    };
    // 前にあってよいのは空白・XML の宣言（`<?…?>`）・コメントと DOCTYPE（`<!…>`）だけ
    let mut rest = text.trim_start_matches('\u{feff}').trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("<?") {
            rest = r[r.find("?>")? + 2..].trim_start();
        } else if let Some(r) = rest.strip_prefix("<!--") {
            rest = r[r.find("-->")? + 3..].trim_start();
        } else if let Some(r) = rest.strip_prefix("<!") {
            rest = r[r.find('>')? + 1..].trim_start();
        } else {
            break;
        }
    }
    let rest = rest.strip_prefix("<svg")?;
    if !rest.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
        return None;
    }
    let tag = &rest[..rest.find('>')?];
    let attr = |name: &str| -> Option<&str> {
        let mut at = 0;
        while let Some(i) = tag[at..].find(name) {
            let i = at + i;
            at = i + name.len();
            let before = tag[..i].chars().next_back();
            if !before.is_some_and(char::is_whitespace) {
                continue;
            }
            let after = tag[at..].trim_start();
            let Some(after) = after.strip_prefix('=') else {
                continue;
            };
            let after = after.trim_start();
            let q = after.chars().next()?;
            if q != '"' && q != '\'' {
                continue;
            }
            let body = &after[1..];
            return body.find(q).map(|e| &body[..e]);
        }
        None
    };
    let view = attr("viewBox").and_then(|v| {
        let n: Vec<f64> = v
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        (n.len() == 4 && n[2] > 0.0 && n[3] > 0.0).then(|| (n[2], n[3]))
    });
    let w = attr("width").and_then(length);
    let h = attr("height").and_then(length);
    match (w, h, view) {
        (Some(w), Some(h), _) => Some((w, h)),
        (Some(w), None, Some((vw, vh))) => Some((w, w * vh / vw)),
        (None, Some(h), Some((vw, vh))) => Some((h * vw / vh, h)),
        (_, _, Some(v)) => Some(v),
        _ => None,
    }
}

/// SVG の長さ（px に。百分率・読めないものは None）。
fn length(s: &str) -> Option<f64> {
    let s = s.trim();
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(end);
    let v: f64 = num.parse().ok()?;
    let k = match unit.trim() {
        "" | "px" => 1.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        _ => return None,
    };
    let v = v * k;
    (v.is_finite() && v > 0.0).then_some(v)
}

/// base64（RFC 4648、詰め物つき）。
fn base64(bytes: &[u8], out: &mut String) {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (c.get(1).copied().unwrap_or(0) as u32) << 8
            | c.get(2).copied().unwrap_or(0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
}

/// 画像の大きさの合わせ方（ノードの `imagescale`、HTML の `SCALE`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// そのままの大きさ（はみ出す向きは縦横比を保って縮める）。
    Natural,
    /// 縦横比を保って箱いっぱいに。
    Fit,
    /// 幅だけを箱に。
    Width,
    /// 高さだけを箱に。
    Height,
    /// 縦横とも箱に。
    Both,
}

impl Scale {
    pub fn parse(s: Option<&str>) -> Scale {
        match s.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("true" | "yes") => Scale::Fit,
            Some("width") => Scale::Width,
            Some("height") => Scale::Height,
            Some("both") => Scale::Both,
            _ => Scale::Natural,
        }
    }
}

/// 箱 (w, h) に置く画像の大きさ。`natural` は点での大きさ。
pub fn fit(natural: (f64, f64), area: (f64, f64), scale: Scale) -> (f64, f64) {
    let (nw, nh) = natural;
    let (aw, ah) = (area.0.max(0.0), area.1.max(0.0));
    let (w, h) = match scale {
        Scale::Natural => (nw, nh),
        Scale::Fit => {
            let k = (aw / nw).min(ah / nh);
            (nw * k, nh * k)
        }
        Scale::Width => (aw, nh),
        Scale::Height => (nw, ah),
        Scale::Both => (aw, ah),
    };
    // はみ出す向きは縮める（そのままの大きさのときは縦横比を保つ）
    if scale == Scale::Natural {
        let k = (aw / w).min(ah / h).min(1.0);
        (w * k, h * k)
    } else {
        (w.min(aw), h.min(ah))
    }
}

/// `<image>` の要素。
pub fn element(p: &Picture, x: f64, y: f64, w: f64, h: f64) -> String {
    use amonhen_pele::svg::builder::num;
    format!(
        "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"{}\"/>",
        num(x),
        num(y),
        num(w),
        num(h),
        p.uri
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    #[test]
    fn reads_png_sizes() {
        assert_eq!(probe(&png_bytes(96, 48)), Some(("image/png", 96.0, 48.0)));
        assert_eq!(probe(&png_bytes(0, 48)), None);
        assert_eq!(probe(&png_bytes(96, 48)[..20]), None);
    }

    #[test]
    fn reads_jpeg_sizes_after_other_segments() {
        let mut b = vec![0xFF, 0xD8];
        // APP0（JFIF）
        b.extend_from_slice(&[0xFF, 0xE0, 0, 16]);
        b.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        // 詰め物と SOF2（プログレッシブ）: 長さ・精度・高さ 30・幅 40
        b.extend_from_slice(&[0xFF, 0xFF, 0xC2, 0, 17, 8, 0, 30, 0, 40, 3]);
        b.extend_from_slice(&[0; 9]);
        assert_eq!(probe(&b), Some(("image/jpeg", 40.0, 30.0)));
        // DHT（C4）は SOF ではない
        let mut d = vec![0xFF, 0xD8, 0xFF, 0xC4, 0, 4, 0, 0];
        d.extend_from_slice(&[0xFF, 0xC0, 0, 17, 8, 0, 5, 0, 7, 3]);
        assert_eq!(probe(&d), Some(("image/jpeg", 7.0, 5.0)));
        assert_eq!(probe(&[0xFF, 0xD8, 0xFF, 0xDA, 0, 2]), None);
        assert_eq!(probe(&[0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xFF]), None);
    }

    #[test]
    fn reads_gif_and_webp_sizes() {
        let mut g = b"GIF89a".to_vec();
        g.extend_from_slice(&[10, 1, 20, 0]);
        assert_eq!(probe(&g), Some(("image/gif", 266.0, 20.0)));
        let riff = |kind: &[u8], body: &[u8]| {
            let mut b = b"RIFF\0\0\0\0WEBP".to_vec();
            b.extend_from_slice(kind);
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(body);
            b
        };
        // VP8: フレームの頭 3 バイト、9D 01 2A、幅 300・高さ 200（上の 2 ビットは拡大の印）
        let vp8 = riff(
            b"VP8 ",
            &[0, 0, 0, 0x9D, 0x01, 0x2A, 0x2C, 0x41, 0xC8, 0x80],
        );
        assert_eq!(probe(&vp8), Some(("image/webp", 300.0, 200.0)));
        // VP8L: 印 2F、幅−1 と高さ−1 の 14 ビットずつ
        let v: u32 = 99 | (49 << 14);
        let mut body = vec![0x2F];
        body.extend_from_slice(&v.to_le_bytes());
        assert_eq!(
            probe(&riff(b"VP8L", &body)),
            Some(("image/webp", 100.0, 50.0))
        );
        // VP8X: 印 4 バイトの後に幅−1・高さ−1 の 24 ビットずつ
        let x = riff(b"VP8X", &[0, 0, 0, 0, 63, 1, 0, 15, 0, 0]);
        assert_eq!(probe(&x), Some(("image/webp", 320.0, 16.0)));
        assert_eq!(probe(&riff(b"VP8 ", &[0; 10])), None);
    }

    #[test]
    fn reads_svg_sizes() {
        let p = |s: &str| probe(s.as_bytes());
        assert_eq!(
            p("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20px\"/>"),
            Some(("image/svg+xml", 40.0, 20.0))
        );
        assert_eq!(
            p("<?xml version=\"1.0\"?>\n<!-- c -->\n<svg viewBox='0 0 100 50' width='200'></svg>"),
            Some(("image/svg+xml", 200.0, 100.0))
        );
        assert_eq!(
            p("<svg width=\"100%\" height=\"100%\" viewBox=\"0,0,30,10\"/>"),
            Some(("image/svg+xml", 30.0, 10.0))
        );
        assert_eq!(
            p("<svg width=\"1in\" height=\"72pt\"/>"),
            Some(("image/svg+xml", 96.0, 96.0))
        );
        // data-width は width ではない
        assert_eq!(
            p("<svg data-width=\"5\" viewBox=\"0 0 8 4\"/>"),
            Some(("image/svg+xml", 8.0, 4.0))
        );
        assert_eq!(p("<svg/>"), None);
        assert_eq!(p("<html><svg width=\"1\" height=\"1\"/></html>"), None);
        assert_eq!(p("<svgx width=\"1\" height=\"1\"/>"), None);
        assert_eq!(p("plain text"), None);
    }

    #[test]
    fn encodes_base64() {
        for (src, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            let mut out = String::new();
            base64(src.as_bytes(), &mut out);
            assert_eq!(out, want);
        }
        let p = read(&png_bytes(2, 1)).unwrap();
        assert!(p.uri.starts_with("data:image/png;base64,iVBORw0KGgo"));
    }

    #[test]
    fn counts_the_limits() {
        let small = png_bytes(4, 4);
        let loader = move |src: &str| match src {
            "a.png" => Some(small.clone()),
            "big.png" => Some(vec![0; MAX_BYTES + 1]),
            _ => None,
        };
        let loader: &Loader = &loader;
        let mut images = Images::new(Some(loader));
        assert!(images.get("a.png").is_some());
        assert!(images.get(" a.png ").is_some());
        assert!(images.get("big.png").is_none());
        assert!(images.get("missing.png").is_none());
        assert_eq!(images.total, 2 * png_bytes(4, 4).len());
        // 違う画像の数の上限
        let mut many = Images::new(Some(loader));
        for i in 0..MAX_COUNT {
            many.get(&format!("x{i}"));
        }
        assert!(many.get("a.png").is_none());
        assert!(Images::new(None).get("a.png").is_none());
    }

    #[test]
    fn counts_pixels() {
        // 画素の多すぎる 1 枚（ヘッダだけでも大きさで決まる）
        assert!(read(&png_bytes(2049, 2048)).is_none());
        let full = png_bytes(2048, 2048);
        assert!(read(&full).is_some());
        // 図ぜんぶでは 2 枚分まで（同じ画像も使うたびに数える）
        let loader = move |_: &str| Some(full.clone());
        let loader: &Loader = &loader;
        let mut images = Images::new(Some(loader));
        assert!(images.get("a.png").is_some());
        assert!(images.get("a.png").is_some());
        assert!(images.get("a.png").is_none());
        // SVG は画素で数えない
        let svg = b"<svg width=\"20000\" height=\"20000\"/>".to_vec();
        assert!(read(&svg).is_some());
    }

    #[test]
    fn scales_into_the_box() {
        let n = (96.0, 48.0);
        assert_eq!(fit(n, (200.0, 100.0), Scale::Natural), (96.0, 48.0));
        assert_eq!(fit(n, (48.0, 100.0), Scale::Natural), (48.0, 24.0));
        assert_eq!(fit(n, (300.0, 50.0), Scale::Fit), (100.0, 50.0));
        assert_eq!(fit(n, (300.0, 50.0), Scale::Width), (300.0, 48.0));
        assert_eq!(fit(n, (300.0, 20.0), Scale::Height), (96.0, 20.0));
        assert_eq!(fit(n, (30.0, 20.0), Scale::Both), (30.0, 20.0));
        assert_eq!(Scale::parse(Some(" TRUE ")), Scale::Fit);
        assert_eq!(Scale::parse(Some("false")), Scale::Natural);
        assert_eq!(Scale::parse(None), Scale::Natural);
    }
}
