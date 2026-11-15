//! History を Direct2D と DirectWrite で描く。Win32 の HWND にも WinUI 3 の SwapChainPanel にも同じものを
//! 描けるよう、swap chain の作り方（`Surface::for_hwnd`・`Surface::for_composition`）だけを分ける。
//!
//! swap chain は flip model（`FLIP_DISCARD`）。`waitable` なら前のフレームが出るのを待ってから描き始め
//! （`GetFrameLatencyWaitableObject`）、入力から画面までを縮める。`tearing` は垂直同期を待たずに出す
//! （Independent Flip に乗ったときだけ効く）。グラフの形は GTK4 版（`apps/gtk4/src/graph.rs`）と同じ。

use std::collections::HashMap;
use std::sync::Arc;

use amonhen_core::HistorySnapshot;
use amonhen_core::lanes::{LaneMetrics, lane_positions};
use amonhen_layout::Edge;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
use windows::core::{Interface, Result, w};
use windows_numerics::Vector2;

use crate::data::{Chunk, Data};

/// History の行の高さ（件名と、作者・日時の 2 行。DIP）。
pub const ROW_HEIGHT: f32 = 38.0;
const LANE_WIDTH: f32 = 14.0;
const LEFT_INSET: f32 = 10.0;
const NODE_RADIUS: f32 = 4.0;
const LINE_WIDTH: f32 = 1.6;
const FIRST_PARENT_WIDTH: f32 = 2.4;
const GRAPH_WIDTH: f32 = 160.0;
const TEXT_INSET: f32 = 8.0;

/// 色は `color % PALETTE.len()` で選ぶ。0 は幹線。
const PALETTE: [[f32; 3]; 8] = [
    [0.30, 0.55, 0.95],
    [0.93, 0.45, 0.30],
    [0.35, 0.75, 0.45],
    [0.75, 0.45, 0.85],
    [0.95, 0.70, 0.25],
    [0.30, 0.75, 0.80],
    [0.90, 0.40, 0.60],
    [0.60, 0.60, 0.65],
];

const BACKGROUND: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 1.0);
const FOREGROUND: D2D1_COLOR_F = rgba(0.11, 0.11, 0.12, 1.0);
const SECONDARY: D2D1_COLOR_F = rgba(0.11, 0.11, 0.12, 0.6);

const fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

fn palette(index: u16, alpha: f32) -> D2D1_COLOR_F {
    let [r, g, b] = PALETTE[index as usize % PALETTE.len()];
    rgba(r, g, b, alpha)
}

fn point(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

#[derive(Clone, Copy)]
pub struct Options {
    /// 前のフレームが画面に出るのを待ってから描く（`FRAME_LATENCY_WAITABLE_OBJECT`、最大の遅れは 1 フレーム）。
    pub waitable: bool,
    /// `Present` の sync interval。0 なら垂直同期を待たない。
    pub sync: u32,
    /// sync interval が 0 のとき、ティアリングを許す（`DXGI_PRESENT_ALLOW_TEARING`）。
    pub tearing: bool,
}

/// 描く先の swap chain。
pub struct Surface {
    pub chain: IDXGISwapChain2,
    pub waitable: Option<HANDLE>,
    target: Option<ID2D1Bitmap1>,
    pub width: u32,
    pub height: u32,
    flags: DXGI_SWAP_CHAIN_FLAG,
    options: Options,
}

impl Surface {
    /// ウィンドウ（HWND）に出す swap chain。
    pub fn for_hwnd(
        renderer: &Renderer,
        hwnd: windows::Win32::Foundation::HWND,
        width: u32,
        height: u32,
        options: Options,
    ) -> Result<Self> {
        let (desc, flags, options) = renderer.desc(width, height, options);
        let chain = unsafe {
            renderer
                .factory
                .CreateSwapChainForHwnd(&renderer.d3d, hwnd, &desc, None, None)?
        };
        Self::new(chain, width, height, flags, options)
    }

    /// Composition（WinUI 3 の SwapChainPanel）に出す swap chain。
    pub fn for_composition(
        renderer: &Renderer,
        width: u32,
        height: u32,
        options: Options,
    ) -> Result<Self> {
        let (desc, flags, options) = renderer.desc(width, height, options);
        let chain = unsafe {
            renderer
                .factory
                .CreateSwapChainForComposition(&renderer.d3d, &desc, None)?
        };
        Self::new(chain, width, height, flags, options)
    }

    fn new(
        chain: IDXGISwapChain1,
        width: u32,
        height: u32,
        flags: DXGI_SWAP_CHAIN_FLAG,
        options: Options,
    ) -> Result<Self> {
        let chain: IDXGISwapChain2 = chain.cast()?;
        let waitable = if options.waitable {
            unsafe {
                chain.SetMaximumFrameLatency(1)?;
                Some(chain.GetFrameLatencyWaitableObject())
            }
        } else {
            None
        };
        Ok(Self {
            chain,
            waitable,
            target: None,
            width,
            height,
            flags,
            options,
        })
    }

    pub fn resize(&mut self, renderer: &Renderer, width: u32, height: u32) -> Result<()> {
        if (width, height) == (self.width, self.height) || width == 0 || height == 0 {
            return Ok(());
        }
        unsafe { renderer.d2d.SetTarget(None) };
        self.target = None;
        unsafe {
            self.chain
                .ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, self.flags)?
        };
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// SwapChainPanel は swap chain を DIP の大きさで見せるので、画素の大きさで作った分を縮めて合わせる。
    pub fn set_scale(&self, scale: f32) -> Result<()> {
        let m = DXGI_MATRIX_3X2_F {
            _11: 1.0 / scale,
            _22: 1.0 / scale,
            ..Default::default()
        };
        unsafe { self.chain.SetMatrixTransform(&m) }
    }

    fn target(&mut self, renderer: &Renderer, scale: f32) -> Result<ID2D1Bitmap1> {
        if let Some(t) = &self.target {
            return Ok(t.clone());
        }
        let surface: IDXGISurface = unsafe { self.chain.GetBuffer(0)? };
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0 * scale,
            dpiY: 96.0 * scale,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            ..Default::default()
        };
        let target = unsafe {
            renderer
                .d2d
                .CreateBitmapFromDxgiSurface(&surface, Some(&props))?
        };
        self.target = Some(target.clone());
        Ok(target)
    }

    pub fn present(&self) -> Result<()> {
        let flags = if self.options.sync == 0 && self.options.tearing {
            DXGI_PRESENT_ALLOW_TEARING
        } else {
            DXGI_PRESENT(0)
        };
        unsafe { self.chain.Present(self.options.sync, flags).ok() }
    }
}

/// 並べ終えた行の文字。
struct RowText {
    subject: IDWriteTextLayout,
    meta: IDWriteTextLayout,
}

pub struct Renderer {
    d3d: ID3D11Device,
    factory: IDXGIFactory2,
    d2d_factory: ID2D1Factory1,
    d2d: ID2D1DeviceContext,
    dwrite: IDWriteFactory,
    subject_format: IDWriteTextFormat,
    meta_format: IDWriteTextFormat,
    brush: ID2D1SolidColorBrush,
    round: ID2D1StrokeStyle,
    tearing_supported: bool,
    /// UTC から地方時への差（秒）。
    utc_offset: i64,
    rows: HashMap<u32, RowText>,
    history: Option<Arc<HistorySnapshot>>,
    /// `AMONHEN_PROFILE` があれば、段階ごとの時間（ms）を残す。段階の間で `Flush` して、溜めた描画も
    /// その段階に数える（その分だけ全体は遅くなる）。
    pub profile: Option<Vec<[f64; 5]>>,
}

/// `Renderer::profile` の段階の名前。
pub const PROFILE_STAGES: [&str; 5] = [
    "graph (build)",
    "graph (flush)",
    "text layouts",
    "text (draw+flush)",
    "EndDraw",
];

impl Renderer {
    pub fn new() -> Result<Self> {
        unsafe {
            let mut d3d = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let d3d: ID3D11Device = d3d.expect("D3D11 device");
            let dxgi: IDXGIDevice = d3d.cast()?;
            let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
            let tearing_supported = factory.cast::<IDXGIFactory5>().is_ok_and(|f| {
                let mut allow = windows::core::BOOL(0);
                f.CheckFeatureSupport(
                    DXGI_FEATURE_PRESENT_ALLOW_TEARING,
                    (&mut allow as *mut windows::core::BOOL).cast(),
                    size_of_val(&allow) as u32,
                )
                .is_ok()
                    && allow.as_bool()
            });
            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d = d2d_factory
                .CreateDevice(&dxgi)?
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let format = |size: f32| -> Result<IDWriteTextFormat> {
                let f = dwrite.CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("ja-jp"),
                )?;
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                let sign = dwrite.CreateEllipsisTrimmingSign(&f)?;
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    ..Default::default()
                };
                f.SetTrimming(&trimming, &sign)?;
                Ok(f)
            };
            let subject_format = format(13.0)?;
            let meta_format = format(11.0)?;
            let brush = d2d.CreateSolidColorBrush(&FOREGROUND, None)?;
            let base: &ID2D1Factory = &d2d_factory;
            let round = base.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let mut tz = TIME_ZONE_INFORMATION::default();
            GetTimeZoneInformation(&mut tz);
            Ok(Self {
                d3d,
                factory,
                d2d_factory,
                d2d,
                dwrite,
                subject_format,
                meta_format,
                brush,
                round,
                tearing_supported,
                utc_offset: -(tz.Bias as i64) * 60,
                rows: HashMap::new(),
                history: None,
                profile: std::env::var_os("AMONHEN_PROFILE").map(|_| Vec::new()),
            })
        }
    }

    fn desc(
        &self,
        width: u32,
        height: u32,
        mut options: Options,
    ) -> (DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, Options) {
        options.tearing &= self.tearing_supported;
        let mut flags = DXGI_SWAP_CHAIN_FLAG(0);
        if options.waitable {
            flags |= DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT;
        }
        if options.tearing {
            flags |= DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING;
        }
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width.max(1),
            Height: height.max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: flags.0 as u32,
            ..Default::default()
        };
        (desc, flags, options)
    }

    pub fn tearing_supported(&self) -> bool {
        self.tearing_supported
    }

    /// 見えている行（`scroll_y` から画面 1 つ分）を描く。`scale` は DIP あたりの画素。
    pub fn draw(
        &mut self,
        surface: &mut Surface,
        data: &mut Data,
        scroll_y: f32,
        scale: f32,
    ) -> Result<()> {
        let target = surface.target(self, scale)?;
        let width = surface.width as f32 / scale;
        let height = surface.height as f32 / scale;
        unsafe {
            self.d2d.SetTarget(&target);
            self.d2d.SetDpi(96.0 * scale, 96.0 * scale);
            self.d2d.BeginDraw();
            self.d2d.Clear(Some(&BACKGROUND));
        }
        let mut stages = [0.0; 5];
        let result = self.draw_history(data, scroll_y, width, height, &mut stages);
        let t = std::time::Instant::now();
        unsafe { self.d2d.EndDraw(None, None)? };
        stages[4] = t.elapsed().as_secs_f64() * 1000.0;
        if let Some(p) = &mut self.profile {
            p.push(stages);
        }
        result
    }

    fn draw_history(
        &mut self,
        data: &mut Data,
        scroll_y: f32,
        width: f32,
        height: f32,
        stages: &mut [f64; 5],
    ) -> Result<()> {
        let profiling = self.profile.is_some();
        let lap = |t: &mut std::time::Instant| {
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            *t = std::time::Instant::now();
            ms
        };
        let Some(history) = data.history.clone() else {
            return Ok(());
        };
        if !self
            .history
            .as_ref()
            .is_some_and(|h| Arc::ptr_eq(h, &history))
        {
            self.rows.clear();
            self.history = Some(history.clone());
        }
        let count = history.layout.nodes().len() as u32;
        let first = ((scroll_y / ROW_HEIGHT).floor().max(0.0) as u32).min(count);
        let last = (((scroll_y + height) / ROW_HEIGHT).ceil() as u32 + 1).min(count);
        // 前後 1 画面分も読んでおく。
        let screen = last - first;
        data.want(first.saturating_sub(screen)..(last + screen).min(count));

        // グラフ。レーンが少なければ狭く（History 全体のレーンの数で決め、スクロールで変えない）。
        let lanes = history.layout.lane_count() as f32;
        let graph_width = (LEFT_INSET + lanes * LANE_WIDTH)
            .clamp(40.0, GRAPH_WIDTH)
            .min(width * 0.4);
        let mut t = std::time::Instant::now();
        self.draw_graph(&history, first..last, scroll_y, graph_width, height)?;
        stages[0] = lap(&mut t);
        if profiling {
            unsafe { self.d2d.Flush(None, None)? };
        }
        stages[1] = lap(&mut t);

        // 文字。
        let text_x = graph_width + TEXT_INSET;
        let text_width = (width - TEXT_INSET - text_x).max(0.0);
        if self.rows.len() > 4096 {
            self.rows.clear();
        }
        for row in first..last {
            let Some(chunk) = data.chunks.get(&(row / crate::data::CHUNK_ROWS)).cloned() else {
                continue;
            };
            if !self.rows.contains_key(&row) {
                let made = std::time::Instant::now();
                let text = self.row_text(&history, &chunk, row)?;
                self.rows.insert(row, text);
                stages[2] += made.elapsed().as_secs_f64() * 1000.0;
            }
            let line = &self.rows[&row];
            let top = row as f32 * ROW_HEIGHT - scroll_y;
            unsafe {
                line.subject.SetMaxWidth(text_width)?;
                line.meta.SetMaxWidth(text_width)?;
                self.brush.SetColor(&FOREGROUND);
                self.d2d.DrawTextLayout(
                    point(text_x, top + 2.0),
                    &line.subject,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                self.brush.SetColor(&SECONDARY);
                self.d2d.DrawTextLayout(
                    point(text_x, top + 20.0),
                    &line.meta,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
        if profiling {
            unsafe { self.d2d.Flush(None, None)? };
        }
        stages[3] = lap(&mut t) - stages[2];
        Ok(())
    }

    fn draw_graph(
        &self,
        history: &HistorySnapshot,
        rows: std::ops::Range<u32>,
        scroll_y: f32,
        graph_width: f32,
        height: f32,
    ) -> Result<()> {
        let metrics = LaneMetrics {
            full: LANE_WIDTH,
            thin: 3.0,
            min_full: 7.0,
            min_thin: 0.0,
            available: graph_width - (LEFT_INSET - LANE_WIDTH / 2.0) - 4.0,
        };
        let count = history.layout.nodes().len() as u32;
        let window = rows.start.saturating_sub(4)..(rows.end + 4).min(count);
        let centers = lane_positions(&history.layout, window, &metrics).0;
        let x = |lane: u16| {
            let c = centers
                .get(lane as usize)
                .copied()
                .unwrap_or(lane as f32 * LANE_WIDTH + LANE_WIDTH / 2.0);
            LEFT_INSET - LANE_WIDTH / 2.0 + c
        };
        let row_y = |row: u32| (row as f32 + 0.5) * ROW_HEIGHT - scroll_y;

        // 色と太さが同じ線を 1 つのパスにまとめる。
        let mut groups: HashMap<(u16, bool), Vec<&Edge>> = HashMap::new();
        let edges = history.layout.edges();
        for i in history.layout.edges_in(rows.clone()) {
            let e = &edges[i as usize];
            let first_parent = e.kind & Edge::FIRST_PARENT != 0;
            groups
                .entry((e.color % PALETTE.len() as u16, first_parent))
                .or_default()
                .push(e);
        }
        unsafe {
            self.d2d.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: graph_width,
                    bottom: f32::MAX,
                },
                D2D1_ANTIALIAS_MODE_ALIASED,
            );
            for ((color, first_parent), edges) in groups {
                let geometry = self.d2d_factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                for e in edges {
                    let (ax, ay) = (x(e.lane_start), row_y(e.row_start));
                    let (bx, by) = (x(e.lane_end), row_y(e.row_end));
                    if e.lane_start == e.lane_end {
                        // 何十万行にまたがる線もあるので、見えている所に切り詰めてから渡す（Direct2D は
                        // 画面の外の分まで輪郭を作る）。
                        let top = ay.max(-ROW_HEIGHT);
                        let bottom = by.min(height + ROW_HEIGHT);
                        sink.BeginFigure(point(ax, top), D2D1_FIGURE_BEGIN_HOLLOW);
                        sink.AddLine(point(bx, bottom));
                    } else {
                        sink.BeginFigure(point(ax, ay), D2D1_FIGURE_BEGIN_HOLLOW);
                        let my = (ay + by) / 2.0;
                        sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: point(ax, my),
                            point2: point(bx, my),
                            point3: point(bx, by),
                        });
                    }
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                }
                sink.Close()?;
                self.brush.SetColor(&palette(color, 1.0));
                let width = if first_parent {
                    FIRST_PARENT_WIDTH
                } else {
                    LINE_WIDTH
                };
                self.d2d
                    .DrawGeometry(&geometry, &self.brush, width, &self.round);
            }
            let nodes = history.layout.nodes();
            for n in &nodes[rows.start as usize..rows.end as usize] {
                self.brush.SetColor(&palette(n.color, 1.0));
                self.d2d.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: point(x(n.lane), row_y(n.row)),
                        radiusX: NODE_RADIUS,
                        radiusY: NODE_RADIUS,
                    },
                    &self.brush,
                );
            }
            self.d2d.PopAxisAlignedClip();
        }
        Ok(())
    }

    fn row_text(&self, history: &HistorySnapshot, chunk: &Chunk, row: u32) -> Result<RowText> {
        let i = chunk.index(row);
        let mut subject = Vec::with_capacity(96);
        chunk.push_utf16(chunk.subject(i), &mut subject);
        let mut meta = Vec::with_capacity(64);
        chunk.push_utf16(chunk.author(i), &mut meta);
        let id = history.graph.ids()[row as usize].to_string();
        let date = date(chunk.time(i) + self.utc_offset);
        meta.extend(format!("  ·  {date}  ·  {}", &id[..8]).encode_utf16());
        unsafe {
            Ok(RowText {
                subject: self.dwrite.CreateTextLayout(
                    &subject,
                    &self.subject_format,
                    10000.0,
                    ROW_HEIGHT,
                )?,
                meta: self.dwrite.CreateTextLayout(
                    &meta,
                    &self.meta_format,
                    10000.0,
                    ROW_HEIGHT,
                )?,
            })
        }
    }
}

/// 地方時の秒を `YYYY-MM-DD HH:MM` にする（Howard Hinnant の civil_from_days）。
fn date(local: i64) -> String {
    let days = local.div_euclid(86400);
    let secs = local.rem_euclid(86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60
    )
}

/// 段階ごとの p50 / p95（ms）。
pub fn profile_summary(frames: &[[f64; 5]]) -> String {
    let mut lines = vec![format!(
        "profile ({} frames, with Flush between stages)",
        frames.len()
    )];
    for (i, name) in PROFILE_STAGES.iter().enumerate() {
        let mut v: Vec<f64> = frames.iter().map(|f| f[i]).collect();
        v.sort_by(f64::total_cmp);
        let at = |p: f64| {
            v.get(((v.len().max(1) - 1) as f64 * p).round() as usize)
                .copied()
                .unwrap_or(0.0)
        };
        lines.push(format!(
            "  {name}: p50 {:.2} / p95 {:.2} ms",
            at(0.5),
            at(0.95)
        ));
    }
    lines.join("\n")
}
