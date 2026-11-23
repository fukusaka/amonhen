//! 描く木を描画命令にして（vgc の `CommandBuilderVisitor`・`DrawCommandBuilder`）、vector_graphics の
//! バイナリに書く（`_encodeInstructions` と vector_graphics_codec 1.2.0 の `VectorGraphicsCodec`）。
//! 塗り・パス・文字などは等しいものを 1 つにまとめ、最初に出てきた順に番号を振る。

use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use super::geometry::{AffineMatrix, PathCommand, Rect};
use super::geometry::{Path, hash_f64};
use super::paint::{
    DST_IN, Fill, Gradient, GradientKind, MaskType, Paint, TextConfig, TextPosition,
};
use super::resolver::RNode;

/// 等しいものに同じ番号を振る（Dart の `putIfAbsent(object, () => map.length)`）。
struct Interner<T: Hash + PartialEq + Clone> {
    ids: HashMap<Key<T>, usize>,
    items: Vec<T>,
}

/// `HashMap` の鍵（Dart の `==` と同じ比べ方。NaN を含むものはどれとも等しくない）。
#[derive(Clone)]
struct Key<T>(T);

impl<T: PartialEq> PartialEq for Key<T> {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}

impl<T: PartialEq> Eq for Key<T> {}

impl<T: Hash> Hash for Key<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T: Hash + PartialEq + Clone> Interner<T> {
    fn new() -> Self {
        Self {
            ids: HashMap::new(),
            items: Vec::new(),
        }
    }

    fn id(&mut self, item: &T) -> usize {
        let key = Key(item.clone());
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        let id = self.ids.len();
        self.ids.insert(key, id);
        self.items.push(item.clone());
        id
    }

    fn get(&self, item: &T) -> Option<usize> {
        self.ids.get(&Key(item.clone())).copied()
    }
}

/// `PatternData`。
#[derive(Clone, PartialEq)]
struct PatternData {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    transform: AffineMatrix,
}

impl Hash for PatternData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for v in [self.x, self.y, self.width, self.height] {
            hash_f64(v, state);
        }
        self.transform.hash(state);
    }
}

enum Command {
    Path {
        path: usize,
        paint: usize,
        pattern: Option<usize>,
    },
    SaveLayer(usize),
    Restore,
    Clip(usize),
    Mask,
    Text {
        text: usize,
        paint: usize,
        pattern: Option<usize>,
    },
    Image(usize),
    Pattern {
        data: usize,
    },
    TextPosition(usize),
}

struct DrawImage {
    image: usize,
    rect: Rect,
    transform: Option<AffineMatrix>,
}

/// `DrawCommandBuilder` と `CommandBuilderVisitor`。
pub struct Builder {
    paints: Interner<Paint>,
    paths: Interner<Path>,
    text: Interner<TextConfig>,
    images: Vec<(Rc<Vec<u8>>, u8)>,
    draw_images: Vec<DrawImage>,
    commands: Vec<Command>,
    patterns: Interner<String>,
    pattern_data: Interner<PatternData>,
    text_positions: Interner<TextPosition>,
    width: f64,
    height: f64,
    current_pattern: Option<String>,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            paints: Interner::new(),
            paths: Interner::new(),
            text: Interner::new(),
            images: Vec::new(),
            draw_images: Vec::new(),
            commands: Vec::new(),
            patterns: Interner::new(),
            pattern_data: Interner::new(),
            text_positions: Interner::new(),
            width: 0.0,
            height: 0.0,
            current_pattern: None,
        }
    }

    fn save_layer(&mut self, paint: &Paint) {
        let id = self.paints.id(paint);
        self.commands.push(Command::SaveLayer(id));
    }

    fn pattern_id(&self) -> Option<usize> {
        self.current_pattern
            .as_ref()
            .and_then(|p| self.patterns.get(p))
    }

    pub fn visit(&mut self, node: &RNode) {
        match node {
            RNode::Empty => {}
            RNode::Group(children) => {
                for c in children {
                    self.visit(c);
                }
            }
            RNode::Viewport {
                width,
                height,
                children,
            } => {
                self.width = *width;
                self.height = *height;
                for c in children {
                    self.visit(c);
                }
            }
            RNode::SaveLayer { paint, children } => {
                self.save_layer(paint);
                for c in children {
                    self.visit(c);
                }
                self.commands.push(Command::Restore);
            }
            RNode::Clip { clips, child } => {
                for clip in clips {
                    let id = self.paths.id(clip);
                    self.commands.push(Command::Clip(id));
                    self.visit(child);
                    self.commands.push(Command::Restore);
                }
            }
            RNode::Mask {
                child,
                mask,
                blend_mode,
                mask_type,
            } => {
                self.save_layer(&Paint::new(
                    *blend_mode,
                    Some(Fill::color(super::paint::Color::OPAQUE_BLACK)),
                    None,
                ));
                self.visit(child);
                if *mask_type == Some(MaskType::Alpha) {
                    self.save_layer(&Paint::new(
                        Some(DST_IN),
                        Some(Fill::color(super::paint::Color::OPAQUE_BLACK)),
                        None,
                    ));
                } else {
                    self.commands.push(Command::Mask);
                }
                self.visit(mask);
                self.commands.push(Command::Restore);
                self.commands.push(Command::Restore);
            }
            RNode::Path { paint, path } => {
                if path.is_empty() {
                    return;
                }
                let path = self.paths.id(path);
                let paint = self.paints.id(paint);
                let pattern = self.pattern_id();
                self.commands.push(Command::Path {
                    path,
                    paint,
                    pattern,
                });
            }
            RNode::TextPosition { position, children } => {
                let id = self.text_positions.id(position);
                self.commands.push(Command::TextPosition(id));
                for c in children {
                    self.visit(c);
                }
            }
            RNode::Text { config, paint } => {
                let paint = self.paints.id(paint);
                let text = self.text.id(config);
                let pattern = self.pattern_id();
                self.commands.push(Command::Text {
                    text,
                    paint,
                    pattern,
                });
            }
            RNode::Image {
                data,
                format,
                rect,
                transform,
            } => {
                // ImageData・DrawImageData は同じ物でしか等しくない（毎回新しい番号）
                self.images.push((data.clone(), *format));
                self.draw_images.push(DrawImage {
                    image: self.images.len() - 1,
                    rect: *rect,
                    transform: *transform,
                });
                self.commands
                    .push(Command::Image(self.draw_images.len() - 1));
            }
            RNode::Pattern {
                child,
                pattern,
                x,
                y,
                width,
                height,
                transform,
                id,
            } => {
                self.patterns.id(id);
                let data = self.pattern_data.id(&PatternData {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                    transform: *transform,
                });
                self.commands.push(Command::Pattern { data });
                self.visit(pattern);
                self.commands.push(Command::Restore);
                self.current_pattern = Some(id.clone());
                self.visit(child);
                self.current_pattern = None;
            }
        }
    }

    /// `_encodeInstructions`。
    pub fn encode(self) -> Vec<u8> {
        let mut b = Buffer::new();
        b.u8(SIZE);
        b.f32(self.width);
        b.f32(self.height);

        for (data, format) in &self.images {
            let id = b.next_image;
            b.next_image += 1;
            b.u8(IMAGE_CONFIG);
            b.u16(id);
            b.u8(*format);
            b.u32(data.len());
            b.bytes(data);
        }

        // 同じグラデーションは書くたびに番号を取り直す（Dart の Map を上書きする）
        let mut shader_ids: HashMap<Key<Gradient>, usize> = HashMap::new();
        for paint in &self.paints.items {
            for shader in [
                paint.fill.as_ref().and_then(|f| f.shader.as_ref()),
                paint.stroke.as_ref().and_then(|s| s.shader.as_ref()),
            ]
            .into_iter()
            .flatten()
            {
                let id = b.write_shader(shader);
                shader_ids.insert(Key(shader.clone()), id);
            }
        }

        let mut fill_ids: HashMap<usize, usize> = HashMap::new();
        let mut stroke_ids: HashMap<usize, usize> = HashMap::new();
        for (i, paint) in self.paints.items.iter().enumerate() {
            let shader_of =
                |s: Option<&Gradient>| s.and_then(|s| shader_ids.get(&Key(s.clone())).copied());
            if let Some(fill) = &paint.fill {
                let id = b.next_paint;
                b.next_paint += 1;
                b.u8(FILL_PAINT);
                b.u32(fill.color.0 as u32 as usize);
                b.u8(paint.blend_mode);
                b.u16(id);
                b.u16(shader_of(fill.shader.as_ref()).unwrap_or(MAX_ID));
                fill_ids.insert(i, id);
                if paint.blur_x.is_some() || paint.blur_y.is_some() {
                    b.write_blur(id, paint);
                }
            }
            if let Some(stroke) = &paint.stroke {
                let id = b.next_paint;
                b.next_paint += 1;
                b.u8(STROKE_PAINT);
                b.u32(stroke.color.0 as u32 as usize);
                b.u8(stroke.cap.unwrap_or(0));
                b.u8(stroke.join.unwrap_or(0));
                b.u8(paint.blend_mode);
                b.f32(stroke.miter_limit.unwrap_or(4.0));
                b.f32(stroke.width.unwrap_or(1.0));
                b.u16(id);
                b.u16(shader_of(stroke.shader.as_ref()).unwrap_or(MAX_ID));
                stroke_ids.insert(i, id);
                if paint.blur_x.is_some() || paint.blur_y.is_some() {
                    b.write_blur(id, paint);
                }
            }
        }

        let mut path_ids: Vec<usize> = Vec::with_capacity(self.paths.items.len());
        for path in &self.paths.items {
            let mut verbs = Vec::with_capacity(path.commands.len());
            let mut points = Vec::new();
            for c in &path.commands {
                match *c {
                    PathCommand::Move(x, y) => {
                        verbs.push(0u8);
                        points.extend([x, y]);
                    }
                    PathCommand::Line(x, y) => {
                        verbs.push(1);
                        points.extend([x, y]);
                    }
                    PathCommand::Cubic(x1, y1, x2, y2, x3, y3) => {
                        verbs.push(2);
                        points.extend([x1, y1, x2, y2, x3, y3]);
                    }
                    PathCommand::Close => verbs.push(3),
                }
            }
            let id = b.next_path;
            b.next_path += 1;
            b.u8(PATH);
            b.u8(path.fill_type as u8);
            b.u16(id);
            b.u32(verbs.len());
            b.bytes(&verbs);
            b.u32(points.len());
            b.align(4);
            for p in points {
                b.f32(p);
            }
            path_ids.push(id);
        }

        for position in &self.text_positions.items {
            let id = b.next_text_position;
            b.next_text_position += 1;
            b.u8(TEXT_POSITION);
            b.u16(id);
            for v in [position.x, position.y, position.dx, position.dy] {
                b.f32(v.unwrap_or(f64::NAN));
            }
            b.u8(u8::from(position.reset));
            b.transform(position.transform.map(|t| t.to_matrix4()));
        }

        for config in &self.text.items {
            let id = b.next_text;
            b.next_text += 1;
            b.u8(TEXT_CONFIG);
            b.u16(id);
            b.f32(config.x_anchor_multiplier);
            b.f32(config.font_size);
            b.u8(config.font_weight);
            b.u8(config.decoration);
            b.u8(config.decoration_style);
            b.u32(config.decoration_color.0 as u32 as usize);
            match &config.font_family {
                Some(family) => {
                    b.u16(family.len());
                    b.bytes(family.as_bytes());
                }
                None => b.u16(0),
            }
            b.u16(config.text.len());
            b.bytes(config.text.as_bytes());
        }

        for command in &self.commands {
            match command {
                Command::Path {
                    path,
                    paint,
                    pattern,
                } => {
                    for ids in [&fill_ids, &stroke_ids] {
                        if let Some(&paint_id) = ids.get(paint) {
                            b.begin_commands();
                            b.u8(DRAW_PATH);
                            b.u16(path_ids[*path]);
                            b.u16(paint_id);
                            b.u16(pattern.unwrap_or(MAX_ID));
                        }
                    }
                }
                Command::SaveLayer(paint) => {
                    b.begin_commands();
                    b.u8(SAVE_LAYER);
                    b.u16(fill_ids.get(paint).copied().unwrap_or(MAX_ID));
                }
                Command::Restore => {
                    b.begin_commands();
                    b.u8(RESTORE);
                }
                Command::Clip(path) => {
                    b.begin_commands();
                    b.u8(CLIP_PATH);
                    b.u16(path_ids[*path]);
                }
                Command::Mask => {
                    b.begin_commands();
                    b.u8(MASK);
                }
                Command::Pattern { data } => {
                    let d = &self.pattern_data.items[*data];
                    let id = b.next_pattern;
                    b.next_pattern += 1;
                    b.u8(PATTERN);
                    b.u16(id);
                    b.f32(d.x);
                    b.f32(d.y);
                    b.f32(d.width);
                    b.f32(d.height);
                    b.transform(Some(d.transform.to_matrix4()));
                }
                Command::TextPosition(id) => {
                    b.begin_commands();
                    b.u8(UPDATE_TEXT_POSITION);
                    b.u16(*id);
                }
                Command::Text {
                    text,
                    paint,
                    pattern,
                } => {
                    b.begin_commands();
                    b.u8(DRAW_TEXT);
                    b.u16(*text);
                    b.u16(fill_ids.get(paint).copied().unwrap_or(MAX_ID));
                    b.u16(stroke_ids.get(paint).copied().unwrap_or(MAX_ID));
                    b.u16(pattern.unwrap_or(MAX_ID));
                }
                Command::Image(i) => {
                    let d = &self.draw_images[*i];
                    b.begin_commands();
                    b.u8(DRAW_IMAGE);
                    b.u16(d.image);
                    b.f32(d.rect.left);
                    b.f32(d.rect.top);
                    b.f32(d.rect.width());
                    b.f32(d.rect.height());
                    b.transform(d.transform.map(|t| t.to_matrix4()));
                }
            }
        }
        b.data
    }
}

const MAX_ID: usize = 65535;
const MAGIC: u32 = 0x0088_2d62;
const VERSION: u8 = 1;
const PATH: u8 = 27;
const FILL_PAINT: u8 = 28;
const STROKE_PAINT: u8 = 29;
const DRAW_PATH: u8 = 30;
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
const PAINT_BLUR: u8 = 53;

/// `VectorGraphicsBuffer`（値はリトルエンディアン、配列は要素の大きさに揃える）。
struct Buffer {
    data: Vec<u8>,
    next_paint: usize,
    next_path: usize,
    next_shader: usize,
    next_text: usize,
    next_text_position: usize,
    next_image: usize,
    next_pattern: usize,
    added_commands: bool,
}

impl Buffer {
    fn new() -> Self {
        let mut b = Self {
            data: Vec::new(),
            next_paint: 0,
            next_path: 0,
            next_shader: 0,
            next_text: 0,
            next_text_position: 0,
            next_image: 0,
            next_pattern: 0,
            added_commands: false,
        };
        b.data.extend(MAGIC.to_le_bytes());
        b.u8(VERSION);
        b
    }

    fn u8(&mut self, v: u8) {
        self.data.push(v);
    }

    /// `setUint16` は下位 16 ビットを書く。
    fn u16(&mut self, v: usize) {
        self.data.extend((v as u16).to_le_bytes());
    }

    fn u32(&mut self, v: usize) {
        self.data.extend((v as u32).to_le_bytes());
    }

    fn f32(&mut self, v: f64) {
        self.data.extend((v as f32).to_le_bytes());
    }

    fn bytes(&mut self, b: &[u8]) {
        self.data.extend_from_slice(b);
    }

    fn align(&mut self, n: usize) {
        let m = self.data.len() % n;
        if m != 0 {
            self.data.resize(self.data.len() + n - m, 0);
        }
    }

    fn begin_commands(&mut self) {
        if !self.added_commands {
            self.u8(BEGIN_COMMANDS);
            self.added_commands = true;
        }
    }

    fn transform(&mut self, m: Option<[f64; 16]>) {
        match m {
            Some(m) => {
                self.u8(16);
                self.align(8);
                for v in m {
                    self.data.extend(v.to_le_bytes());
                }
            }
            None => self.u8(0),
        }
    }

    fn colors_and_offsets(&mut self, g: &Gradient) {
        let colors = g.colors.as_deref().unwrap_or(&[]);
        self.u16(colors.len());
        self.align(4);
        for c in colors {
            self.data.extend((c.0 as u32).to_le_bytes());
        }
        match &g.offsets {
            None => self.u16(0),
            Some(offsets) => {
                self.u16(offsets.len());
                self.align(4);
                for o in offsets {
                    self.f32(*o);
                }
            }
        }
    }

    fn write_shader(&mut self, g: &Gradient) -> usize {
        let id = self.next_shader;
        self.next_shader += 1;
        let tile = g.tile_mode.map_or(0, |t| t as u8);
        match g.kind {
            GradientKind::Linear { from, to } => {
                self.u8(LINEAR_GRADIENT);
                self.u16(id);
                self.f32(from.x);
                self.f32(from.y);
                self.f32(to.x);
                self.f32(to.y);
                self.colors_and_offsets(g);
                self.u8(tile);
            }
            GradientKind::Radial {
                center,
                radius,
                focal,
            } => {
                self.u8(RADIAL_GRADIENT);
                self.u16(id);
                self.f32(center.x);
                self.f32(center.y);
                self.f32(radius);
                match focal {
                    Some(f) => {
                        self.u8(1);
                        self.f32(f.x);
                        self.f32(f.y);
                    }
                    None => self.u8(0),
                }
                self.colors_and_offsets(g);
                let t = g
                    .transform
                    .filter(|t| *t != AffineMatrix::IDENTITY)
                    .map(|t| t.to_matrix4());
                self.transform(t);
                self.u8(tile);
            }
        }
        id
    }

    fn write_blur(&mut self, id: usize, paint: &Paint) {
        self.u8(PAINT_BLUR);
        self.u16(id);
        self.f32(paint.blur_x.unwrap_or(0.0));
        self.f32(paint.blur_y.unwrap_or(0.0));
    }
}
