//! flutter_svg が SVG をどう描くかを見せる（History の「すべてのファイル」）。flutter_svg は SVG を
//! vector_graphics_compiler（vgc）で描画命令のバイナリ（`.vec`）に変え、それを Flutter の Canvas で描く。`.vec` は
//! vgc を移した `vgc` で作り、ここで CoreSVG・librsvg で描ける SVG に戻す（`vec_to_svg`）。`.vec` の形は
//! vector_graphics_codec 1.2.0 のソースから。
//!
//! vgc は flutter_svg と同じ設定で呼ぶ: 最適化（masks・clips・overdraw）はすべて切る（flutter_svg の `SvgLoader` は
//! `encodeSvg` を最適化なしで呼ぶ）、currentColor は黒、font-size は 14（`SvgTheme` の既定）。
//!
//! SVG に戻せないもの（描いた絵が Flutter と違う所）:
//! - Porter-Duff の合成（`src`・`dstOut`・`xor` など）。`srcOver` 以外で CSS の `mix-blend-mode` にあるもの（`multiply`
//!   など、`plus` は `plus-lighter`）は写す。レイヤーの `dstIn` はアルファのマスクとして写す。ほかは捨てて、注釈を残す。
//! - タイルの `decal`（端の外を透明に）。SVG にはないので `pad` にする。
//! - 文字の幅。Flutter は文字の並び（run）ごとに幅を測って次の位置を決める。SVG では同じ `<text>` の中の `<tspan>` に
//!   続けて書いて描き手に測らせる。途中で別の命令が挟まって `<text>` を閉じたあとは、前の run の始まりから書く。
//!   `text-anchor` は x を与えた run にしか効かない。既定の書体（family なし）は描き手の既定になる（Flutter は
//!   Android で Roboto、iOS で SF）。
//! - 輝度のマスク。Flutter は輝度だけをアルファにし（マスクのアルファは見ない）、SVG は輝度とアルファを掛ける。
//! - パターンのタイル。Flutter はタイルの絵を `width`×`height` の画像に焼いてから並べる（拡大するとぼやける）。
//!   SVG は線のまま並べる。タイルの `x`・`y` は Flutter と同じく使わない。
//! - 太さ 0 の線。Flutter（Skia）は 1 画素の細線で描き、SVG は描かない。拡大しても 1 の太さの線にする。
//! - 画像の WebP・BMP は CoreSVG・librsvg が描けないことがある（data: URI のまま埋める）。

use std::collections::HashMap;
use std::fmt::Write as _;

use amonhen_model::{Result, tr};

// --- vector_graphics のバイナリ ---

const MAGIC: u32 = 0x0088_2d62;
const VERSION: u8 = 1;
/// id がないことを表す値（`kMaxId`）。
const NONE: u16 = 0xffff;

const PATH: u8 = 27;
const FILL_PAINT: u8 = 28;
const STROKE_PAINT: u8 = 29;
const DRAW_PATH: u8 = 30;
const DRAW_VERTICES: u8 = 31;
const SAVE_LAYER: u8 = 37;
const RESTORE: u8 = 38;
const LINEAR_GRADIENT: u8 = 39;
const RADIAL_GRADIENT: u8 = 40;
const SIZE: u8 = 41;
const CLIP_PATH: u8 = 42;
const MASK: u8 = 43;
const DRAW_TEXT: u8 = 44;
const TEXT_CONFIG: u8 = 45;
const IMAGE_CONFIG: u8 = 46;
const DRAW_IMAGE: u8 = 47;
const BEGIN_COMMANDS: u8 = 48;
const PATTERN: u8 = 49;
const TEXT_POSITION: u8 = 50;
const UPDATE_TEXT_POSITION: u8 = 51;
const PATH_HALF: u8 = 52;
const PAINT_BLUR: u8 = 53;

/// dart:ui の `BlendMode` の並び。
const BLEND_MODES: [&str; 29] = [
    "clear",
    "src",
    "dst",
    "srcOver",
    "dstOver",
    "srcIn",
    "dstIn",
    "srcOut",
    "dstOut",
    "srcATop",
    "dstATop",
    "xor",
    "plus",
    "modulate",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "colorDodge",
    "colorBurn",
    "hardLight",
    "softLight",
    "difference",
    "exclusion",
    "multiply",
    "hue",
    "saturation",
    "color",
    "luminosity",
];
const SRC_OVER: u8 = 3;
const DST_IN: u8 = 6;

/// 先頭から読む（値はリトルエンディアン、配列は先頭からの位置で要素の大きさに揃える）。
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let Some(end) = end else {
            return Err(tr!("The vector_graphics data ends early").into());
        };
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn align(&mut self, n: usize) {
        let rem = self.pos % n;
        if rem != 0 {
            self.pos += n - rem;
        }
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }

    fn f32(&mut self) -> Result<f64> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into()?).into())
    }

    /// NaN は「なし」。
    fn opt_f32(&mut self) -> Result<Option<f64>> {
        let v = self.f32()?;
        Ok((!v.is_nan()).then_some(v))
    }

    fn f32s(&mut self, n: usize) -> Result<Vec<f64>> {
        self.align(4);
        Ok(self
            .take(n.saturating_mul(4))?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c).into())
            .collect())
    }

    fn u32s(&mut self, n: usize) -> Result<Vec<u32>> {
        self.align(4);
        Ok(self
            .take(n.saturating_mul(4))?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect())
    }

    fn u16s(&mut self, n: usize) -> Result<Vec<u16>> {
        self.align(2);
        Ok(self
            .take(n.saturating_mul(2))?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect())
    }

    fn transform(&mut self) -> Result<Option<[f64; 16]>> {
        let len = self.u8()?;
        if len == 0 {
            return Ok(None);
        }
        if len != 16 {
            return Err(tr!("The vector_graphics data is broken").into());
        }
        self.align(8);
        let mut m = [0.0; 16];
        for (v, c) in m.iter_mut().zip(self.take(16 * 8)?.as_chunks::<8>().0) {
            *v = f64::from_le_bytes(*c);
        }
        Ok(Some(m))
    }

    fn string(&mut self) -> Result<String> {
        let len = self.u16()?.into();
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }
}

/// IEEE 754 の半精度を単精度に。
fn half_to_f64(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f64::from(h & 0x3ff);
    sign * match exp {
        0 => frac * 2f64.powi(-24),
        0x1f if frac == 0.0 => f64::INFINITY,
        0x1f => f64::NAN,
        _ => (1.0 + frac / 1024.0) * 2f64.powi(exp - 15),
    }
}

/// vector_graphics のバイナリ（`.vec`）を、CoreSVG・librsvg で描ける SVG にする。何が写らないかはこのモジュールの説明に。
pub fn vec_to_svg(bytes: &[u8]) -> Result<String> {
    let mut r = Reader {
        data: bytes,
        pos: 0,
    };
    if bytes.len() < 5 || r.u32()? != MAGIC {
        return Err(tr!("This isn’t vector_graphics data").into());
    }
    let version = r.u8()?;
    if version != VERSION {
        return Err(tr!(
            "vector_graphics data version {version} isn’t supported",
            version = version
        )
        .into());
    }
    let mut w = Writer::default();
    w.frames.push(Frame {
        kind: FrameKind::Root,
        body: String::new(),
    });
    while r.pos < bytes.len() {
        match r.u8()? {
            BEGIN_COMMANDS => {}
            SIZE => {
                w.width = r.f32()?;
                w.height = r.f32()?;
            }
            IMAGE_CONFIG => {
                let id = r.u16()?;
                let format = r.u8()?;
                let len = r.u32()? as usize;
                let data = r.take(len)?;
                let mime = match format {
                    0 => "image/png",
                    1 => "image/jpeg",
                    2 => "image/webp",
                    3 => "image/gif",
                    _ => "image/bmp",
                };
                w.images
                    .insert(id, format!("data:{mime};base64,{}", base64(data)));
            }
            LINEAR_GRADIENT => {
                let id = r.u16()?;
                let [x1, y1, x2, y2] = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                let n = r.u16()?.into();
                let colors = r.u32s(n)?;
                let n = r.u16()?.into();
                let offsets = r.f32s(n)?;
                let tile = r.u8()?;
                let _ = write!(
                    w.defs,
                    r#"<linearGradient id="g{id}" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}"{}>{}</linearGradient>"#,
                    num(x1),
                    num(y1),
                    num(x2),
                    num(y2),
                    spread(tile),
                    stops(&colors, &offsets)
                );
            }
            RADIAL_GRADIENT => {
                let id = r.u16()?;
                let [cx, cy, radius] = [r.f32()?, r.f32()?, r.f32()?];
                let focal = if r.u8()? == 1 {
                    Some((r.f32()?, r.f32()?))
                } else {
                    None
                };
                let n = r.u16()?.into();
                let colors = r.u32s(n)?;
                let n = r.u16()?.into();
                let offsets = r.f32s(n)?;
                let transform = r.transform()?;
                let tile = r.u8()?;
                let focal = focal.map_or(String::new(), |(fx, fy)| {
                    format!(r#" fx="{}" fy="{}""#, num(fx), num(fy))
                });
                let transform = transform.map_or(String::new(), |m| {
                    format!(r#" gradientTransform="{}""#, matrix(&m))
                });
                let _ = write!(
                    w.defs,
                    r#"<radialGradient id="g{id}" gradientUnits="userSpaceOnUse" cx="{}" cy="{}" r="{}"{focal}{transform}{}>{}</radialGradient>"#,
                    num(cx),
                    num(cy),
                    num(radius),
                    spread(tile),
                    stops(&colors, &offsets)
                );
            }
            FILL_PAINT => {
                let color = r.u32()?;
                let blend = r.u8()?;
                let id = r.u16()?;
                let shader = r.u16()?;
                w.paints.insert(
                    id,
                    Paint {
                        color,
                        blend,
                        stroke: None,
                        shader: (shader != NONE).then_some(shader),
                        blur: None,
                    },
                );
            }
            STROKE_PAINT => {
                let color = r.u32()?;
                let cap = r.u8()?;
                let join = r.u8()?;
                let blend = r.u8()?;
                let miter = r.f32()?;
                let width = r.f32()?;
                let id = r.u16()?;
                let shader = r.u16()?;
                w.paints.insert(
                    id,
                    Paint {
                        color,
                        blend,
                        stroke: Some(Stroke {
                            cap,
                            join,
                            miter,
                            width,
                        }),
                        shader: (shader != NONE).then_some(shader),
                        blur: None,
                    },
                );
            }
            PAINT_BLUR => {
                let id = r.u16()?;
                let (sx, sy) = (r.f32()?, r.f32()?);
                let filter = w.next_id();
                let (x, y, fw, fh) = w.region();
                let _ = write!(
                    w.defs,
                    r#"<filter id="b{filter}" filterUnits="userSpaceOnUse" x="{x}" y="{y}" width="{fw}" height="{fh}"><feGaussianBlur stdDeviation="{} {}"/></filter>"#,
                    num(sx),
                    num(sy)
                );
                if let Some(paint) = w.paints.get_mut(&id) {
                    paint.blur = Some(filter);
                }
            }
            tag @ (PATH | PATH_HALF) => {
                let fill_type = r.u8()?;
                let id = r.u16()?;
                let n = r.u32()? as usize;
                let verbs = r.take(n)?;
                let n = r.u32()? as usize;
                let points = if tag == PATH_HALF {
                    r.u16s(n)?.into_iter().map(half_to_f64).collect()
                } else {
                    r.f32s(n)?
                };
                let d = path_data(verbs, &points)?;
                w.paths.insert(id, (d, fill_type == 1));
            }
            TEXT_POSITION => {
                let id = r.u16()?;
                let position = TextPosition {
                    x: r.opt_f32()?,
                    y: r.opt_f32()?,
                    dx: r.opt_f32()?,
                    dy: r.opt_f32()?,
                    reset: r.u8()? != 0,
                    transform: r.transform()?,
                };
                w.text_positions.insert(id, position);
            }
            TEXT_CONFIG => {
                let id = r.u16()?;
                let anchor = r.f32()?;
                let size = r.f32()?;
                let weight = r.u8()?;
                let decoration = r.u8()?;
                let decoration_style = r.u8()?;
                let decoration_color = r.u32()?;
                let family = r.string()?;
                let text = r.string()?;
                w.texts.insert(
                    id,
                    TextConfig {
                        text,
                        family: (!family.is_empty()).then_some(family),
                        anchor,
                        size,
                        weight,
                        decoration,
                        decoration_style,
                        decoration_color,
                    },
                );
            }
            DRAW_PATH => {
                let path = r.u16()?;
                let paint = r.u16()?;
                let pattern = r.u16()?;
                w.draw_path(path, paint, (pattern != NONE).then_some(pattern));
            }
            DRAW_VERTICES => {
                let paint = r.u16()?;
                let n = r.u16()?.into();
                let vertices = r.f32s(n)?;
                let n: usize = r.u16()?.into();
                let indices = if n == 0 { None } else { Some(r.u16s(n)?) };
                w.draw_vertices(&vertices, indices.as_deref(), paint);
            }
            SAVE_LAYER => {
                let paint = r.u16()?;
                w.save_layer(paint);
            }
            RESTORE => w.restore(),
            CLIP_PATH => {
                let path = r.u16()?;
                w.clip(path);
            }
            MASK => {
                w.close_text();
                w.frames.push(Frame {
                    kind: FrameKind::Mask { alpha: false },
                    body: String::new(),
                });
            }
            DRAW_TEXT => {
                let text = r.u16()?;
                let fill = r.u16()?;
                let stroke = r.u16()?;
                let pattern = r.u16()?;
                w.draw_text(
                    text,
                    (fill != NONE).then_some(fill),
                    (stroke != NONE).then_some(stroke),
                    (pattern != NONE).then_some(pattern),
                );
            }
            DRAW_IMAGE => {
                let id = r.u16()?;
                let [x, y, iw, ih] = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                let transform = r.transform()?;
                w.draw_image(id, [x, y, iw, ih], transform);
            }
            PATTERN => {
                let id = r.u16()?;
                let [_x, _y, pw, ph] = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                let transform = r.transform()?;
                w.close_text();
                w.frames.push(Frame {
                    kind: FrameKind::Pattern {
                        id,
                        width: pw,
                        height: ph,
                        transform,
                    },
                    body: String::new(),
                });
            }
            UPDATE_TEXT_POSITION => {
                let id = r.u16()?;
                w.update_text_position(id);
            }
            tag => {
                return Err(tr!("Unknown vector_graphics command {tag}", tag = tag).into());
            }
        }
    }
    Ok(w.finish())
}

fn path_data(verbs: &[u8], points: &[f64]) -> Result<String> {
    let mut d = String::new();
    let mut p = points.iter().copied();
    let mut next = || {
        p.next()
            .ok_or_else(|| amonhen_model::Error::from(tr!("The vector_graphics data is broken")))
    };
    for &verb in verbs {
        if !d.is_empty() {
            d.push(' ');
        }
        match verb {
            0 => {
                let (x, y) = (next()?, next()?);
                let _ = write!(d, "M{} {}", num(x), num(y));
            }
            1 => {
                let (x, y) = (next()?, next()?);
                let _ = write!(d, "L{} {}", num(x), num(y));
            }
            2 => {
                let c = [next()?, next()?, next()?, next()?, next()?, next()?];
                let _ = write!(
                    d,
                    "C{} {} {} {} {} {}",
                    num(c[0]),
                    num(c[1]),
                    num(c[2]),
                    num(c[3]),
                    num(c[4]),
                    num(c[5])
                );
            }
            3 => d.push('Z'),
            _ => return Err(tr!("The vector_graphics data is broken").into()),
        }
    }
    Ok(d)
}

struct Stroke {
    cap: u8,
    join: u8,
    miter: f64,
    width: f64,
}

struct Paint {
    /// ARGB。
    color: u32,
    blend: u8,
    stroke: Option<Stroke>,
    shader: Option<u16>,
    /// ぼかしの `<filter>` の番号。
    blur: Option<usize>,
}

struct TextPosition {
    x: Option<f64>,
    y: Option<f64>,
    dx: Option<f64>,
    dy: Option<f64>,
    reset: bool,
    transform: Option<[f64; 16]>,
}

struct TextConfig {
    text: String,
    family: Option<String>,
    anchor: f64,
    size: f64,
    /// `FontWeight` の番号（0 が w100）。
    weight: u8,
    decoration: u8,
    decoration_style: u8,
    decoration_color: u32,
}

enum FrameKind {
    Root,
    /// `saveLayer`（属性つきの `<g>`）。
    Layer(String),
    /// `save` ＋ `clipPath`。
    Clip(usize),
    /// これまでの中身（親の frame）に、この中身をマスクとしてかける。
    Mask {
        alpha: bool,
    },
    Pattern {
        id: u16,
        width: f64,
        height: f64,
        transform: Option<[f64; 16]>,
    },
}

/// Canvas の save・saveLayer・パターンの 1 段。
struct Frame {
    kind: FrameKind,
    body: String,
}

/// Flutter の文字の位置（vector_graphics の listener と同じ動き）を、SVG の `<text>`・`<tspan>` に写す。
struct TextState {
    /// `<text>` を開いていれば、その変換。
    open: Option<Option<[f64; 16]>>,
    /// 次の run の x。`None` は前の run の後ろ（幅は描き手が測る）。
    x: Option<f64>,
    /// 前の run の後ろからずらす量。
    pending_dx: f64,
    y: f64,
    y_dirty: bool,
    transform: Option<[f64; 16]>,
    /// 最後に x を与えた run の x（`<text>` を開き直すときの代わり）。
    last_x: f64,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            open: None,
            x: Some(0.0),
            pending_dx: 0.0,
            y: 0.0,
            y_dirty: true,
            transform: None,
            last_x: 0.0,
        }
    }
}

#[derive(Default)]
struct Writer {
    width: f64,
    height: f64,
    defs: String,
    frames: Vec<Frame>,
    images: HashMap<u16, String>,
    paints: HashMap<u16, Paint>,
    /// SVG の `d` と、evenodd か。
    paths: HashMap<u16, (String, bool)>,
    text_positions: HashMap<u16, TextPosition>,
    texts: HashMap<u16, TextConfig>,
    text: TextState,
    next_id: usize,
}

impl Writer {
    fn next_id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    fn body(&mut self) -> &mut String {
        &mut self
            .frames
            .last_mut()
            .expect("the root frame is never popped")
            .body
    }

    /// マスク・フィルタの範囲（描く範囲の周りも含める）。
    fn region(&self) -> (String, String, String, String) {
        let (w, h) = (self.width.max(1.0), self.height.max(1.0));
        (num(-w), num(-h), num(3.0 * w), num(3.0 * h))
    }

    fn note(&mut self, text: &str) {
        let text = text.replace("--", "- -");
        let _ = write!(self.body(), "<!-- {text} -->");
    }

    fn close_text(&mut self) {
        if self.text.open.take().is_some() {
            self.body().push_str("</text>");
        }
    }

    /// 塗りか線の属性。`pattern` があれば色や勾配の代わりにパターンで塗る。
    fn paint_attrs(&mut self, id: u16, pattern: Option<u16>, evenodd: bool) -> String {
        let Some(paint) = self.paints.get(&id) else {
            // Flutter は既定の Paint（黒の塗り）で描く
            return if evenodd {
                r#" fill-rule="evenodd""#.into()
            } else {
                String::new()
            };
        };
        let mut a = String::new();
        let (value, opacity) = paint_value(paint, pattern);
        match &paint.stroke {
            Some(stroke) => {
                let _ = write!(a, r#" fill="none" stroke="{value}""#);
                if opacity < 1.0 {
                    let _ = write!(a, r#" stroke-opacity="{}""#, num(opacity));
                }
                if stroke.width > 0.0 {
                    let _ = write!(a, r#" stroke-width="{}""#, num(stroke.width));
                } else {
                    // Skia の細線（1 画素）
                    a.push_str(r#" stroke-width="1" vector-effect="non-scaling-stroke""#);
                }
                match stroke.cap {
                    1 => a.push_str(r#" stroke-linecap="round""#),
                    2 => a.push_str(r#" stroke-linecap="square""#),
                    _ => {}
                }
                match stroke.join {
                    1 => a.push_str(r#" stroke-linejoin="round""#),
                    2 => a.push_str(r#" stroke-linejoin="bevel""#),
                    _ => {}
                }
                if stroke.miter != 4.0 {
                    let _ = write!(a, r#" stroke-miterlimit="{}""#, num(stroke.miter));
                }
            }
            None => {
                let _ = write!(a, r#" fill="{value}""#);
                if opacity < 1.0 {
                    let _ = write!(a, r#" fill-opacity="{}""#, num(opacity));
                }
                if evenodd {
                    a.push_str(r#" fill-rule="evenodd""#);
                }
            }
        }
        if let Some(blur) = paint.blur {
            let _ = write!(a, r#" filter="url(#b{blur})""#);
        }
        let blend = paint.blend;
        a.push_str(&self.blend_attr(blend));
        a
    }

    /// `mix-blend-mode` の属性。SVG で表せない合成は注釈を残して捨てる。
    fn blend_attr(&mut self, blend: u8) -> String {
        if blend == SRC_OVER {
            return String::new();
        }
        match css_blend(blend) {
            Some(css) => format!(r#" style="mix-blend-mode:{css}""#),
            None => {
                let name = BLEND_MODES.get(usize::from(blend)).unwrap_or(&"?");
                self.note(&format!(
                    "blend mode {name} can't be drawn in SVG; drawn as srcOver"
                ));
                String::new()
            }
        }
    }

    fn draw_path(&mut self, path: u16, paint: u16, pattern: Option<u16>) {
        self.close_text();
        let Some((d, evenodd)) = self.paths.get(&path).cloned() else {
            self.note(&format!("path {path} is missing"));
            return;
        };
        let attrs = self.paint_attrs(paint, pattern, evenodd);
        let _ = write!(self.body(), r#"<path d="{d}"{attrs}/>"#);
    }

    fn draw_vertices(&mut self, vertices: &[f64], indices: Option<&[u16]>, paint: u16) {
        self.close_text();
        let point = |i: usize| {
            (
                vertices.get(2 * i).copied(),
                vertices.get(2 * i + 1).copied(),
            )
        };
        let order: Vec<usize> = match indices {
            Some(indices) => indices.iter().map(|&i| usize::from(i)).collect(),
            None => (0..vertices.len() / 2).collect(),
        };
        let mut d = String::new();
        for tri in order.as_chunks::<3>().0 {
            for (k, &i) in tri.iter().enumerate() {
                if let (Some(x), Some(y)) = point(i) {
                    let _ = write!(
                        d,
                        "{}{} {} ",
                        if k == 0 { "M" } else { "L" },
                        num(x),
                        num(y)
                    );
                }
            }
            d.push_str("Z ");
        }
        let attrs = self.paint_attrs(paint, None, false);
        let _ = write!(self.body(), r#"<path d="{}"{attrs}/>"#, d.trim_end());
    }

    fn save_layer(&mut self, paint: u16) {
        self.close_text();
        let (alpha, blend, blur) = self
            .paints
            .get(&paint)
            .map_or((255, SRC_OVER, None), |p| (p.color >> 24, p.blend, p.blur));
        if blend == DST_IN {
            self.frames.push(Frame {
                kind: FrameKind::Mask { alpha: true },
                body: String::new(),
            });
            return;
        }
        let mut attrs = String::new();
        if alpha < 255 {
            let _ = write!(attrs, r#" opacity="{}""#, num(f64::from(alpha) / 255.0));
        }
        if let Some(blur) = blur {
            let _ = write!(attrs, r#" filter="url(#b{blur})""#);
        }
        attrs.push_str(&self.blend_attr(blend));
        self.frames.push(Frame {
            kind: FrameKind::Layer(attrs),
            body: String::new(),
        });
    }

    fn clip(&mut self, path: u16) {
        self.close_text();
        let id = self.next_id();
        let (d, evenodd) = self.paths.get(&path).cloned().unwrap_or_default();
        let rule = if evenodd {
            r#" clip-rule="evenodd""#
        } else {
            ""
        };
        let _ = write!(
            self.defs,
            r#"<clipPath id="c{id}" clipPathUnits="userSpaceOnUse"><path d="{d}"{rule}/></clipPath>"#
        );
        self.frames.push(Frame {
            kind: FrameKind::Clip(id),
            body: String::new(),
        });
    }

    fn restore(&mut self) {
        self.close_text();
        if self.frames.len() <= 1 {
            // Flutter も余った restore は何もしない
            return;
        }
        let frame = self.frames.pop().expect("checked above");
        let body = frame.body;
        match frame.kind {
            FrameKind::Root => unreachable!("the root frame is never popped"),
            FrameKind::Layer(attrs) if attrs.is_empty() => self.body().push_str(&body),
            FrameKind::Layer(attrs) => {
                let _ = write!(self.body(), "<g{attrs}>{body}</g>");
            }
            FrameKind::Clip(id) => {
                let _ = write!(self.body(), r#"<g clip-path="url(#c{id})">{body}</g>"#);
            }
            FrameKind::Mask { alpha } => {
                let id = self.next_id();
                let (x, y, w, h) = self.region();
                let kind = if alpha { "alpha" } else { "luminance" };
                let _ = write!(
                    self.defs,
                    r#"<mask id="m{id}" maskUnits="userSpaceOnUse" maskContentUnits="userSpaceOnUse" x="{x}" y="{y}" width="{w}" height="{h}" mask-type="{kind}" style="mask-type:{kind}">{body}</mask>"#
                );
                let masked = std::mem::take(self.body());
                let _ = write!(self.body(), r#"<g mask="url(#m{id})">{masked}</g>"#);
            }
            FrameKind::Pattern {
                id,
                width,
                height,
                transform,
            } => {
                let transform = transform
                    .filter(|m| !is_identity(m))
                    .map_or(String::new(), |m| {
                        format!(r#" patternTransform="{}""#, matrix(&m))
                    });
                let _ = write!(
                    self.defs,
                    r#"<pattern id="p{id}" patternUnits="userSpaceOnUse" x="0" y="0" width="{}" height="{}"{transform}>{body}</pattern>"#,
                    num(width),
                    num(height)
                );
            }
        }
    }

    fn draw_image(&mut self, id: u16, [x, y, w, h]: [f64; 4], transform: Option<[f64; 16]>) {
        self.close_text();
        let Some(uri) = self.images.get(&id).cloned() else {
            self.note(&format!("image {id} is missing"));
            return;
        };
        let transform =
            transform.map_or(String::new(), |m| format!(r#" transform="{}""#, matrix(&m)));
        let _ = write!(
            self.body(),
            r#"<image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none"{transform} xlink:href="{uri}"/>"#,
            num(x),
            num(y),
            num(w),
            num(h)
        );
    }

    fn update_text_position(&mut self, id: u16) {
        let Some(p) = self.text_positions.get(&id) else {
            return;
        };
        let t = &mut self.text;
        if p.reset {
            t.x = Some(0.0);
            t.pending_dx = 0.0;
            t.y = 0.0;
            t.y_dirty = true;
        }
        if let Some(x) = p.x {
            t.x = Some(x);
            t.pending_dx = 0.0;
        }
        if let Some(y) = p.y {
            t.y = y;
            t.y_dirty = true;
        }
        if let Some(dx) = p.dx {
            match &mut t.x {
                Some(x) => *x += dx,
                None => t.pending_dx += dx,
            }
        }
        if let Some(dy) = p.dy {
            t.y += dy;
            t.y_dirty = true;
        }
        t.transform = p.transform;
    }

    fn draw_text(&mut self, id: u16, fill: Option<u16>, stroke: Option<u16>, pattern: Option<u16>) {
        let Some(config) = self.texts.get(&id) else {
            self.close_text();
            self.note(&format!("text {id} is missing"));
            return;
        };
        let mut attrs = String::new();
        if let Some(family) = &config.family {
            let _ = write!(attrs, r#" font-family="{}""#, escape(family));
        }
        let _ = write!(attrs, r#" font-size="{}""#, num(config.size));
        let weight = (u32::from(config.weight) + 1).min(9) * 100;
        if weight != 400 {
            let _ = write!(attrs, r#" font-weight="{weight}""#);
        }
        if config.anchor >= 0.75 {
            attrs.push_str(r#" text-anchor="end""#);
        } else if config.anchor >= 0.25 {
            attrs.push_str(r#" text-anchor="middle""#);
        }
        let mut lines = Vec::new();
        for (mask, name) in [(1, "underline"), (2, "overline"), (4, "line-through")] {
            if config.decoration & mask != 0 {
                lines.push(name);
            }
        }
        if !lines.is_empty() {
            let style = ["solid", "double", "dotted", "dashed", "wavy"]
                .get(usize::from(config.decoration_style))
                .unwrap_or(&"solid");
            let c = config.decoration_color;
            let color = format!(
                "rgba({},{},{},{})",
                (c >> 16) & 0xff,
                (c >> 8) & 0xff,
                c & 0xff,
                num(f64::from(c >> 24) / 255.0)
            );
            let _ = write!(
                attrs,
                r#" text-decoration="{}" style="text-decoration-style:{style};text-decoration-color:{color}""#,
                lines.join(" "),
            );
        }
        let content = escape(&config.text);
        let mut paint = String::new();
        match fill {
            Some(fill) => paint.push_str(&self.paint_attrs(fill, pattern, false)),
            None => paint.push_str(r#" fill="none""#),
        }
        if let Some(stroke) = stroke {
            let s = self.paint_attrs(stroke, pattern, false);
            // 線の属性のうち、塗りの "none" は塗りの側に任せる
            paint.push_str(&s.replacen(r#" fill="none""#, "", 1));
        }
        // CoreSVG は <tspan> の x・y を読まない（0 に描く）ので、位置を与える run は <text> を開き直してそこに書く
        let positioned = self.text.x.is_some() || self.text.y_dirty;
        if self
            .text
            .open
            .is_some_and(|t| t != self.text.transform || positioned)
        {
            self.close_text();
        }
        let t = &mut self.text;
        let mut out = String::new();
        if t.open.is_none() {
            let transform = t
                .transform
                .map_or(String::new(), |m| format!(r#" transform="{}""#, matrix(&m)));
            if t.x.is_none() {
                // 前の run の幅は測れないので、その始まりから
                t.x = Some(t.last_x + t.pending_dx);
                t.pending_dx = 0.0;
            }
            let x = t.x.unwrap_or_default();
            t.last_x = x;
            let _ = write!(
                out,
                r#"<text xml:space="preserve"{transform} x="{}" y="{}">"#,
                num(x),
                num(t.y)
            );
            t.open = Some(t.transform);
        }
        let mut position = String::new();
        if t.pending_dx != 0.0 {
            let _ = write!(position, r#" dx="{}""#, num(t.pending_dx));
        }
        t.x = None;
        t.pending_dx = 0.0;
        t.y_dirty = false;
        let _ = write!(out, "<tspan{position}{attrs}{paint}>{content}</tspan>");
        self.body().push_str(&out);
    }

    fn finish(mut self) -> String {
        self.close_text();
        while self.frames.len() > 1 {
            self.restore();
        }
        let body = std::mem::take(self.body());
        let (w, h) = (num(self.width), num(self.height));
        let defs = if self.defs.is_empty() {
            String::new()
        } else {
            format!("<defs>{}</defs>", self.defs)
        };
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{defs}{body}</svg>"#
        )
    }
}

/// 塗りの値（色・`url(#…)`）と不透明度。勾配やパターンでも Paint のアルファがかかる。
fn paint_value(paint: &Paint, pattern: Option<u16>) -> (String, f64) {
    let (color, opacity) = rgba(paint.color);
    match (pattern, paint.shader) {
        (Some(p), _) => (format!("url(#p{p})"), opacity),
        (None, Some(g)) => (format!("url(#g{g})"), opacity),
        (None, None) => (color, opacity),
    }
}

/// `mix-blend-mode` の名前。Porter-Duff の合成（`plus` を除く）は表せない。
fn css_blend(blend: u8) -> Option<&'static str> {
    Some(match blend {
        12 => "plus-lighter",
        // modulate は乗算（不透明なら multiply と同じ）
        13 | 24 => "multiply",
        14 => "screen",
        15 => "overlay",
        16 => "darken",
        17 => "lighten",
        18 => "color-dodge",
        19 => "color-burn",
        20 => "hard-light",
        21 => "soft-light",
        22 => "difference",
        23 => "exclusion",
        25 => "hue",
        26 => "saturation",
        27 => "color",
        28 => "luminosity",
        _ => return None,
    })
}

/// `TileMode`（clamp・repeated・mirror・decal）。decal は SVG にないので pad。
fn spread(tile: u8) -> &'static str {
    match tile {
        1 => r#" spreadMethod="repeat""#,
        2 => r#" spreadMethod="reflect""#,
        _ => "",
    }
}

fn stops(colors: &[u32], offsets: &[f64]) -> String {
    let mut s = String::new();
    let last = colors.len().saturating_sub(1).max(1) as f64;
    for (i, &c) in colors.iter().enumerate() {
        let offset = offsets.get(i).copied().unwrap_or(i as f64 / last);
        let (color, opacity) = rgba(c);
        let _ = write!(s, r#"<stop offset="{}" stop-color="{color}""#, num(offset));
        if opacity < 1.0 {
            let _ = write!(s, r#" stop-opacity="{}""#, num(opacity));
        }
        s.push_str("/>");
    }
    s
}

/// ARGB を `#rrggbb` と不透明度に。
fn rgba(argb: u32) -> (String, f64) {
    (
        format!("#{:06x}", argb & 0xff_ffff),
        f64::from(argb >> 24) / 255.0,
    )
}

/// 4×4（列優先）の 2D の部分。
fn matrix(m: &[f64; 16]) -> String {
    format!(
        "matrix({} {} {} {} {} {})",
        num(m[0]),
        num(m[1]),
        num(m[4]),
        num(m[5]),
        num(m[12]),
        num(m[13])
    )
}

fn is_identity(m: &[f64; 16]) -> bool {
    m.iter()
        .enumerate()
        .all(|(i, &v)| v == if i % 5 == 0 { 1.0 } else { 0.0 })
}

/// 小数 4 桁まで、末尾の 0 を落とす。
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_owned() }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: &[u8] = include_bytes!("../tests/data/vec/icon.vec");
    const GRADIENT: &[u8] = include_bytes!("../tests/data/vec/gradient.vec");
    const STYLE_FILTER: &[u8] = include_bytes!("../tests/data/vec/style_filter.vec");
    const TEXT: &[u8] = include_bytes!("../tests/data/vec/text.vec");
    const PATTERN_IMAGE: &[u8] = include_bytes!("../tests/data/vec/pattern_image.vec");
    const BLUR: &[u8] = include_bytes!("../tests/data/vec/blur.vec");

    fn svg(vec: &[u8]) -> String {
        vec_to_svg(vec).unwrap()
    }

    #[test]
    fn an_icon_keeps_paths_fill_rules_and_strokes() {
        let s = svg(ICON);
        assert!(
            s.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg""#),
            "{s}"
        );
        assert!(
            s.contains(r#"width="24" height="24" viewBox="0 0 24 24""#),
            "{s}"
        );
        assert!(s.contains(r##"<path d="M3 3 L21 3 L21 21 L3 21 Z M7 7 L7 17 L17 17 L17 7 Z" fill="#336699" fill-rule="evenodd"/>"##), "{s}");
        assert!(s.contains(r##"fill="none" stroke="#ee3333" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="bevel""##), "{s}");
        // currentColor は黒、stroke-opacity は線の色のアルファに
        assert!(s.contains(r##"stroke="#000000" stroke-opacity="0.498" stroke-width="1" stroke-linecap="square" stroke-linejoin="round""##), "{s}");
        assert!(s.contains("C13.6557 9 15 10.3443 15 12"), "{s}");
        assert!(s.ends_with("</svg>"));
    }

    #[test]
    fn gradients_layers_and_blend_modes() {
        let s = svg(GRADIENT);
        assert!(s.contains(r#"<linearGradient id="g0" gradientUnits="userSpaceOnUse" x1="5" y1="5" x2="50" y2="5" spreadMethod="reflect">"#), "{s}");
        assert!(
            s.contains(r##"<stop offset="0.5" stop-color="#00ff00" stop-opacity="0.498"/>"##),
            "{s}"
        );
        assert!(s.contains(r#"<radialGradient id="g1" gradientUnits="userSpaceOnUse" cx="75" cy="30" r="20" fx="70" fy="25" gradientTransform="matrix(0.9397 0.342 -0.342 0.9397 14.7837 -23.8423)">"#), "{s}");
        assert!(s.contains(r#"fill="url(#g0)""#), "{s}");
        // 不透明度 0.6 の multiply のレイヤー、中の 2 つも multiply
        assert!(
            s.contains(r#"<g opacity="0.6" style="mix-blend-mode:multiply"><path"#),
            "{s}"
        );
        assert!(
            s.contains(r#"fill="url(#g1)" style="mix-blend-mode:multiply"/>"#),
            "{s}"
        );
    }

    #[test]
    fn style_is_dropped_and_clips_and_masks_are_kept() {
        let s = svg(STYLE_FILTER);
        // filter の feGaussianBlur は塗りのぼかしに
        assert!(s.contains(r##"fill="#0099cc" filter="url(#b1)"/>"##), "{s}");
        // vgc は <style> を読まないので .accent は黒
        assert!(
            s.contains(r##"<path d="M4 4 L28 4 L28 28 L4 28 Z" fill="#000000"/>"##),
            "{s}"
        );
        assert!(
            s.contains(r#"<clipPath id="c2" clipPathUnits="userSpaceOnUse"><path d="M32 12 C"#),
            "{s}"
        );
        assert!(s.contains(r##"<g clip-path="url(#c2)"><path d="M0 0 L64 0 L64 64 L0 64 Z" fill="#33aa33"/></g>"##), "{s}");
        assert!(s.contains(r#"mask-type="luminance""#), "{s}");
        assert!(s.contains(r##"<g mask="url(#m3)"><path d="M32 36"##), "{s}");
        assert!(
            s.contains(r##"<path d="M0 0 L64 0 L64 32 L0 32 Z" fill="#ffffff"/></mask>"##),
            "{s}"
        );
    }

    #[test]
    fn text_runs_continue_in_one_text_element() {
        let s = svg(TEXT);
        assert!(s.contains(r##"<text xml:space="preserve" x="10" y="20"><tspan font-family="Helvetica" font-size="12" fill="#222222">Hello</tspan><tspan font-family="Helvetica" font-size="12" font-weight="700" fill="#cc0000"> bold</tspan><tspan font-family="Helvetica" font-size="12" fill="#222222"> &amp; more</tspan>"##), "{s}");
        assert!(s.contains(r##"<text xml:space="preserve" x="60" y="45"><tspan font-size="16" text-anchor="middle" text-decoration="underline""##), "{s}");
        assert!(
            s.contains(r##"fill="none" stroke="#0066cc" stroke-width="0.5">Mid</tspan></text>"##),
            "{s}"
        );
    }

    #[test]
    fn patterns_and_images() {
        let s = svg(PATTERN_IMAGE);
        assert!(s.contains(r##"<pattern id="p0" patternUnits="userSpaceOnUse" x="0" y="0" width="10" height="10"><path d="M5 2 C"##), "{s}");
        assert!(s.contains(r#"fill="url(#p0)""#), "{s}");
        assert!(s.contains(r#"<image x="10" y="22" width="16" height="16" preserveAspectRatio="none" xlink:href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg=="/>"#), "{s}");
    }

    #[test]
    fn blurs_become_filters() {
        let s = svg(BLUR);
        assert!(s.contains(r#"<feGaussianBlur stdDeviation="2 3"/>"#), "{s}");
        assert!(s.contains(r#"<g filter="url(#b1)">"#), "{s}");
        assert!(s.contains(r##"fill="#ff0000" filter="url(#b2)"/>"##), "{s}");
    }

    #[test]
    fn broken_data_is_an_error() {
        assert!(vec_to_svg(b"").is_err());
        assert!(vec_to_svg(b"<svg/>").is_err());
        assert!(vec_to_svg(&ICON[..ICON.len() - 3]).is_err());
        let mut v = ICON.to_vec();
        v[4] = 2;
        assert!(vec_to_svg(&v).is_err());
        let mut v = ICON[..5].to_vec();
        v.push(200);
        assert!(vec_to_svg(&v).is_err());
    }

    /// 手で組んだ: 半精度の点・頂点・Porter-Duff・dstIn のレイヤー・パターンの変換・文字の reset と dx。
    #[test]
    fn hand_built_commands() {
        let mut b = Vec::new();
        b.extend(MAGIC.to_le_bytes());
        b.push(1);
        let f = |b: &mut Vec<u8>, v: f32| b.extend(v.to_le_bytes());
        let align = |b: &mut Vec<u8>, n: usize| {
            while !b.len().is_multiple_of(n) {
                b.push(0);
            }
        };
        b.push(SIZE);
        f(&mut b, 10.0);
        f(&mut b, 10.0);
        // 塗り 0: xor、塗り 1: dstIn
        b.push(FILL_PAINT);
        b.extend(0xff00_00ffu32.to_le_bytes());
        b.push(11);
        b.extend(0u16.to_le_bytes());
        b.extend(NONE.to_le_bytes());
        b.push(FILL_PAINT);
        b.extend(0xff00_0000u32.to_le_bytes());
        b.push(DST_IN);
        b.extend(1u16.to_le_bytes());
        b.extend(NONE.to_le_bytes());
        // 半精度の線: M1 2 L0.5 -3
        b.push(PATH_HALF);
        b.push(0);
        b.extend(0u16.to_le_bytes());
        b.extend(2u32.to_le_bytes());
        b.extend([0, 1]);
        b.extend(4u32.to_le_bytes());
        align(&mut b, 2);
        for h in [0x3c00u16, 0x4000, 0x3800, 0xc200] {
            b.extend(h.to_le_bytes());
        }
        b.push(TEXT_POSITION);
        b.extend(0u16.to_le_bytes());
        for v in [f32::NAN, f32::NAN, 3.0, f32::NAN] {
            f(&mut b, v);
        }
        b.push(1);
        b.push(0);
        b.push(TEXT_CONFIG);
        b.extend(0u16.to_le_bytes());
        f(&mut b, 1.0);
        f(&mut b, 10.0);
        b.push(3);
        b.push(0);
        b.push(0);
        b.extend(0u32.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(b"<a");
        b.push(BEGIN_COMMANDS);
        b.push(DRAW_PATH);
        b.extend(0u16.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(NONE.to_le_bytes());
        b.push(SAVE_LAYER);
        b.extend(1u16.to_le_bytes());
        b.push(DRAW_VERTICES);
        b.extend(7u16.to_le_bytes());
        b.extend(6u16.to_le_bytes());
        align(&mut b, 4);
        for v in [0.0, 0.0, 4.0, 0.0, 0.0, 4.0] {
            f(&mut b, v);
        }
        b.extend(0u16.to_le_bytes());
        b.push(RESTORE);
        b.push(PATTERN);
        b.extend(0u16.to_le_bytes());
        for v in [0.0, 0.0, 2.0, 2.0] {
            f(&mut b, v);
        }
        b.push(16);
        align(&mut b, 8);
        for v in [
            2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 0.0, 1.0f64,
        ] {
            b.extend(v.to_le_bytes());
        }
        b.push(RESTORE);
        b.push(UPDATE_TEXT_POSITION);
        b.extend(0u16.to_le_bytes());
        b.push(DRAW_TEXT);
        b.extend(0u16.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(NONE.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.push(RESTORE);
        let s = vec_to_svg(&b).unwrap();
        assert!(
            s.contains("<!-- blend mode xor can't be drawn in SVG; drawn as srcOver -->"),
            "{s}"
        );
        assert!(
            s.contains(r##"<g mask="url(#m1)"><!-- blend mode xor"##),
            "{s}"
        );
        assert!(
            s.contains(r##"<path d="M1 2 L0.5 -3" fill="#0000ff"/>"##),
            "{s}"
        );
        assert!(s.contains(r#"mask-type="alpha""#), "{s}");
        // 頂点の三角形、Paint がなければ既定（黒の塗り）
        assert!(s.contains(r#"<path d="M0 0 L4 0 L0 4 Z"/></mask>"#), "{s}");
        assert!(
            s.contains(r#"patternTransform="matrix(2 0 0 2 5 6)""#),
            "{s}"
        );
        assert!(s.contains(r##"<text xml:space="preserve" x="3" y="0"><tspan font-size="10" text-anchor="end" fill="url(#p0)">&lt;a</tspan>"##), "{s}");
    }

    #[test]
    fn small_helpers() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(half_to_f64(0x3555), 0.333_251_953_125);
        assert_eq!(half_to_f64(0x0001), 2f64.powi(-24));
    }
}
