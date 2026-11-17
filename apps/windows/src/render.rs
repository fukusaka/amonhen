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
use amonhen_model::RefKind;
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

/// History を 1 行で出すか（View の One Line per Commit。GTK4 版の `Display::one_line`）。
static ONE_LINE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 選んだコミットの祖先を強調するか（View の Highlight Ancestors。GTK4 版の `Display::highlight_ancestors`）。
static HIGHLIGHT_ANCESTORS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub fn highlight_ancestors() -> bool {
    HIGHLIGHT_ANCESTORS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_highlight_ancestors(value: bool) {
    HIGHLIGHT_ANCESTORS.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// 描く線と、その描く範囲（始まりの行、終わりの行）。
type EdgeSpan<'a> = (&'a Edge, u32, u32);

/// 祖先を強調するときの、ほかの点と線の不透明度（GTK4 版・macOS 版と同じ）。
const DIMMED_ALPHA: f32 = 0.18;
/// 祖先を 1 フレームに伸ばす行数の上限（GTK4 版と同じ。遠くへ飛んだとき、届くまでは強調せずに描く）。
const LINEAGE_ROWS_PER_FRAME: u32 = 150_000;

/// History の行に出す項目（View の Branches and Tags・Author・Date・Commit ID・Signature。GTK4 版の
/// `settings::items` と同じビット。CI の状態はこの FE にない）。
pub mod items {
    pub const REFS: u32 = 1;
    pub const AUTHOR: u32 = 2;
    pub const DATE: u32 = 4;
    pub const ID: u32 = 8;
    pub const SIGNATURE: u32 = 16;
    /// GitHub の CI の状態（この FE は出さないが、ビットは GTK4 版・macOS 版と同じ設定に残す）。
    pub const CI: u32 = 32;
    pub const ALL: u32 = REFS | AUTHOR | DATE | ID | SIGNATURE | CI;
    /// 作者・日時の行に出すもの。
    pub const META: u32 = AUTHOR | DATE | ID;
}

static ITEMS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(items::ALL);

pub fn history_items() -> u32 {
    ITEMS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_history_items(value: u32) {
    ITEMS.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// History の行の高さ（DIP）: ふだんは件名と、作者・日時の 2 行。1 行で出すなら件名の右に作者・日時。
pub fn row_height() -> f32 {
    if compact() { 24.0 } else { 38.0 }
}

pub fn one_line() -> bool {
    ONE_LINE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 実際に 1 行で出すか: 1 行にしたか、作者・日時の行に出す項目が 1 つもないとき（GTK4 版の `Display::compact`）。
fn compact() -> bool {
    one_line() || history_items() & items::META == 0
}

pub fn set_one_line(value: bool) {
    ONE_LINE.store(value, std::sync::atomic::Ordering::Relaxed);
}
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
/// 検索で一致した行の背景。
const MATCH: D2D1_COLOR_F = rgba(1.0, 0.80, 0.0, 0.22);
/// 選んだ行の背景（Windows の既定のアクセントの色を薄く）。
const SELECTION: D2D1_COLOR_F = rgba(0.0, 0.47, 0.84, 0.16);
/// ref のラベルの色（ローカルブランチは青、リモートブランチは紫、タグは橙。macOS 版・GTK4 版と同じ）。地は薄く、
/// チェックアウト中のブランチは濃く塗って白い文字にする。
const LABEL_LOCAL: (f32, f32, f32) = (0.21, 0.52, 0.89);
const LABEL_REMOTE: (f32, f32, f32) = (0.57, 0.38, 0.84);
const LABEL_TAG: (f32, f32, f32) = (0.90, 0.55, 0.15);
/// worktree の HEAD の札と、ほかの worktree が checkout しているブランチ。
const LABEL_WORKTREE: (f32, f32, f32) = (0.20, 0.66, 0.30);
const LABEL_TEXT: D2D1_COLOR_F = rgba(0.11, 0.11, 0.12, 0.9);
const LABEL_HEAD_TEXT: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 1.0);
const LABEL_HEIGHT: f32 = 17.0;
/// 差分の行の高さ（DIP）と、行番号の列の幅。
pub const DIFF_ROW_HEIGHT: f32 = 18.0;
const DIFF_NUMBER_WIDTH: f32 = 44.0;
const DIFF_TEXT_INSET: f32 = 10.0;
// 差分の色（GTK4 版・macOS 版の標準の色と同じ）。
const DIFF_ADDED: D2D1_COLOR_F = rgba(0.2, 0.7, 0.3, 0.16);
const DIFF_REMOVED: D2D1_COLOR_F = rgba(0.9, 0.25, 0.25, 0.16);
const DIFF_ADDED_WORD: D2D1_COLOR_F = rgba(0.2, 0.7, 0.3, 0.35);
const DIFF_REMOVED_WORD: D2D1_COLOR_F = rgba(0.9, 0.25, 0.25, 0.35);
const DIFF_HUNK: D2D1_COLOR_F = rgba(0.3, 0.5, 0.9, 0.10);
const DIFF_FILE: D2D1_COLOR_F = rgba(0.11, 0.11, 0.12, 0.07);
/// Working Copy の差分の、行ごとの含める印の列の幅。
pub const DIFF_MARK_WIDTH: f32 = 22.0;
const DIFF_MARK: D2D1_COLOR_F = rgba(0.0, 0.47, 0.84, 1.0);
const DIFF_GUTTER: D2D1_COLOR_F = rgba(0.11, 0.11, 0.12, 0.035);
const LABEL_PADDING: f32 = 6.0;
const LABEL_GAP: f32 = 4.0;
/// ラベルを並べる幅の上限（文字の幅に対する割合）。残りは件名に回す。
const LABEL_SHARE: f32 = 0.6;

/// 差分の行（hunk）を次のコミットに含めているか。
#[derive(Clone, Copy)]
enum Mark {
    All,
    Some,
    None,
}

/// 署名の検証の結果の色（GTK4 版の `signature_color`。まだ検証していなければ薄い灰色）。
fn signature_color(mark: Option<crate::data::Signature>) -> D2D1_COLOR_F {
    use crate::data::Signature;
    match mark {
        Some(Signature::Verified) => rgba(0.20, 0.70, 0.30, 1.0),
        Some(Signature::Caution) => rgba(0.90, 0.75, 0.10, 1.0),
        Some(Signature::Expired) => rgba(0.95, 0.55, 0.15, 1.0),
        Some(Signature::Bad) => rgba(0.90, 0.25, 0.25, 1.0),
        Some(Signature::Unknown) => rgba(0.11, 0.11, 0.12, 0.55),
        None => rgba(0.11, 0.11, 0.12, 0.25),
    }
}

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
    /// 選んだコミットの祖先（History の世代、選んだ行、求めた範囲）。
    lineage: Option<(u64, u32, amonhen_core::lineage::LineageWindow)>,
    /// 祖先が描く行まで届いていない（次のフレームで続ける）。
    pub lineage_pending: bool,
    /// `rows` を作ったときの行の項目（変わったら作り直す）。
    rows_items: u32,
    /// ラベルの文字（名前ごと）と、その幅。
    labels: HashMap<String, (IDWriteTextLayout, f32)>,
    label_format: IDWriteTextFormat,
    /// 差分の文字（等幅）と、見出しの文字。
    code_format: IDWriteTextFormat,
    file_format: IDWriteTextFormat,
    /// 差分の行の文字の形（今の差分の行ごと。差分が替わったら捨てる）。
    diff_rows: HashMap<u32, IDWriteTextLayout>,
    /// 構文の種類ごとの文字の色。
    syntax_brushes: HashMap<u8, ID2D1SolidColorBrush>,
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
            let label_format = format(11.0)?;
            let code_format = dwrite.CreateTextFormat(
                w!("Cascadia Mono"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.0,
                w!("ja-jp"),
            )?;
            code_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let file_format = dwrite.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.0,
                w!("ja-jp"),
            )?;
            file_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
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
                lineage: None,
                lineage_pending: false,
                rows_items: items::ALL,
                labels: HashMap::new(),
                label_format,
                code_format,
                file_format,
                diff_rows: HashMap::new(),
                syntax_brushes: HashMap::new(),
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

    /// 見えている行（`scroll_y` から画面 1 つ分）を描く。`scale` は DIP あたりの画素。`selected` は選んだ行。
    pub fn draw(
        &mut self,
        surface: &mut Surface,
        data: &mut Data,
        scroll_y: f32,
        scale: f32,
        selected: Option<u32>,
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
        let result = self.draw_history(data, scroll_y, width, height, selected, &mut stages);
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
        selected: Option<u32>,
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
        let first = ((scroll_y / row_height()).floor().max(0.0) as u32).min(count);
        let last = (((scroll_y + height) / row_height()).ceil() as u32 + 1).min(count);
        // 前後 1 画面分も読んでおく。
        let screen = last - first;
        data.want(first.saturating_sub(screen)..(last + screen).min(count));

        // 検索で一致した行の背景（見えている分だけ）。
        let from = data.matches.partition_point(|&r| r < first);
        for &row in data.matches[from..].iter().take_while(|&&r| r < last) {
            let top = row as f32 * row_height() - scroll_y;
            unsafe {
                self.brush.SetColor(&MATCH);
                self.d2d.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top,
                        right: width,
                        bottom: top + row_height(),
                    },
                    &self.brush,
                );
            }
        }
        // 複数選んだ行の背景。
        let from = data.multi.partition_point(|&r| r < first);
        for &row in data.multi[from..].iter().take_while(|&&r| r < last) {
            if Some(row) == selected {
                continue;
            }
            let top = row as f32 * row_height() - scroll_y;
            unsafe {
                self.brush.SetColor(&SELECTION);
                self.d2d.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top,
                        right: width,
                        bottom: top + row_height(),
                    },
                    &self.brush,
                );
            }
        }
        // 選んだ行の背景（グラフの下に敷く）。
        if let Some(row) = selected.filter(|r| (first..last).contains(r)) {
            let top = row as f32 * row_height() - scroll_y;
            unsafe {
                self.brush.SetColor(&SELECTION);
                self.d2d.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top,
                        right: width,
                        bottom: top + row_height(),
                    },
                    &self.brush,
                );
            }
        }

        // グラフ。レーンが少なければ狭く（History 全体のレーンの数で決め、スクロールで変えない）。
        let lanes = history.layout.lane_count() as f32;
        let graph_width = (LEFT_INSET + lanes * LANE_WIDTH)
            .clamp(40.0, GRAPH_WIDTH)
            .min(width * 0.4);
        let mut t = std::time::Instant::now();
        // 祖先は描く行まで求める（スクロールしたら続きから。GTK4 版の `update_lineage` と描くときの伸ばし方）。
        let target = selected.filter(|_| highlight_ancestors());
        match target {
            None => self.lineage = None,
            Some(row) => {
                if !self
                    .lineage
                    .as_ref()
                    .is_some_and(|(g, r, _)| *g == history.generation && *r == row)
                {
                    self.lineage = Some((
                        history.generation,
                        row,
                        amonhen_core::lineage::LineageWindow::new(&history.graph, row),
                    ));
                }
            }
        }
        let lineage = self.lineage.as_mut().map(|(_, _, w)| {
            let end = last.min(w.end().saturating_add(LINEAGE_ROWS_PER_FRAME));
            w.extend(&history.graph, &history.layout, end);
            w
        });
        self.lineage_pending = lineage.as_ref().is_some_and(|w| w.end() < last);
        let lineage = self
            .lineage
            .as_ref()
            .map(|(_, _, w)| w)
            .filter(|w| w.end() >= last);
        self.draw_graph(
            &history,
            first..last,
            scroll_y,
            graph_width,
            height,
            lineage,
        )?;
        stages[0] = lap(&mut t);
        if profiling {
            unsafe { self.d2d.Flush(None, None)? };
        }
        stages[1] = lap(&mut t);

        // 文字。
        let text_x = graph_width + TEXT_INSET;
        let text_width = (width - TEXT_INSET - text_x).max(0.0);
        if self.rows.len() > 4096 || self.rows_items != history_items() {
            self.rows.clear();
            self.rows_items = history_items();
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
            let top = row as f32 * row_height() - scroll_y;
            let compact = compact();
            let shown = history_items();
            let label_top = if compact {
                (row_height() - LABEL_HEIGHT) / 2.0
            } else {
                2.0
            };
            let signed = shown & items::SIGNATURE != 0 && chunk.signed(chunk.index(row));
            let mark_width = if signed { 14.0 } else { 0.0 };
            // 1 行なら、作者・日時を右端に寄せ（GTK4 版の 1 行表示と同じく件名の右）、ラベルと件名はその手前まで。
            let (meta_x, meta_y, subject_right) = unsafe {
                let meta = &self.rows[&row].meta;
                if compact {
                    meta.SetMaxWidth((text_width * 0.5 - mark_width).max(0.0))?;
                    let mut metrics = DWRITE_TEXT_METRICS::default();
                    meta.GetMetrics(&mut metrics)?;
                    let meta_x = text_x + text_width - mark_width - metrics.width;
                    (meta_x, top + 4.0, meta_x - 12.0)
                } else {
                    meta.SetMaxWidth((text_width - mark_width).max(0.0))?;
                    (text_x, top + 20.0, text_x + text_width)
                }
            };
            // ref のラベルを件名の前に並べる。
            let mut subject_x = text_x;
            // worktree の HEAD の札を先に。
            let labels = data
                .worktree_labels
                .get(&row)
                .into_iter()
                .chain(data.labels.get(&row))
                .flatten()
                .filter(|_| shown & items::REFS != 0);
            {
                let limit = (text_x + text_width * LABEL_SHARE).min(subject_right);
                for label in labels {
                    let (layout, w) = self.label(&label.name)?;
                    let pill = w + LABEL_PADDING * 2.0;
                    if subject_x + pill > limit {
                        break;
                    }
                    let elsewhere =
                        label.kind == RefKind::Local && data.elsewhere.contains(&label.name);
                    let place = (subject_x, top + label_top, pill);
                    self.draw_label(label, elsewhere, &layout, place)?;
                    subject_x += pill + LABEL_GAP;
                }
            }
            // 署名があれば、作者・日時の後ろに検証の結果の色の丸（GTK4 版・macOS 版の印）。
            let mark = signed.then(|| data.signature_mark(history.graph.ids()[row as usize]));
            let line = &self.rows[&row];
            unsafe {
                line.subject
                    .SetMaxWidth((subject_right - subject_x).max(0.0))?;
                if let Some(mark) = mark {
                    let mut metrics = DWRITE_TEXT_METRICS::default();
                    line.meta.GetMetrics(&mut metrics)?;
                    self.brush.SetColor(&signature_color(mark));
                    self.d2d.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: point(meta_x + metrics.width + 8.0, meta_y + 7.0),
                            radiusX: 3.5,
                            radiusY: 3.5,
                        },
                        &self.brush,
                    );
                }
                self.brush.SetColor(&FOREGROUND);
                self.d2d.DrawTextLayout(
                    point(subject_x, top + if compact { 3.0 } else { 2.0 }),
                    &line.subject,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                self.brush.SetColor(&SECONDARY);
                self.d2d.DrawTextLayout(
                    point(meta_x, meta_y),
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
        // 描いた行で頼まれた署名を、裏で検証する。
        data.verify_signatures();
        Ok(())
    }

    fn draw_graph(
        &self,
        history: &HistorySnapshot,
        rows: std::ops::Range<u32>,
        scroll_y: f32,
        graph_width: f32,
        height: f32,
        lineage: Option<&amonhen_core::lineage::LineageWindow>,
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
        let row_y = |row: u32| (row as f32 + 0.5) * row_height() - scroll_y;

        // 色と太さと薄さが同じ線を 1 つのパスにまとめる。祖先を強調するときは、光らない線（や縦線の光らない上の
        // 部分）を薄くする（GTK4 版の `graph::draw`）。線は (始まりの行, 終わりの行) を描く範囲として持つ。
        let mut groups: HashMap<(u16, bool, bool), Vec<EdgeSpan>> = HashMap::new();
        let edges = history.layout.edges();
        for i in history.layout.edges_in(rows.clone()) {
            let e = &edges[i as usize];
            let first_parent = e.kind & Edge::FIRST_PARENT != 0;
            let color = e.color % PALETTE.len() as u16;
            let from = lineage.map_or(e.row_start, |l| l.edge_from(i));
            if from == amonhen_core::lineage::NOT_LIT {
                groups
                    .entry((color, first_parent, true))
                    .or_default()
                    .push((e, e.row_start, e.row_end));
            } else if from > e.row_start && e.lane_start == e.lane_end {
                groups
                    .entry((color, first_parent, true))
                    .or_default()
                    .push((e, e.row_start, from));
                groups
                    .entry((color, first_parent, false))
                    .or_default()
                    .push((e, from, e.row_end));
            } else {
                groups
                    .entry((color, first_parent, false))
                    .or_default()
                    .push((e, e.row_start, e.row_end));
            }
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
            for ((color, first_parent, dimmed), edges) in groups {
                let geometry = self.d2d_factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                for (e, start, end) in edges {
                    let (ax, ay) = (x(e.lane_start), row_y(start));
                    let (bx, by) = (x(e.lane_end), row_y(end));
                    if e.lane_start == e.lane_end {
                        // 何十万行にまたがる線もあるので、見えている所に切り詰めてから渡す（Direct2D は
                        // 画面の外の分まで輪郭を作る）。
                        let top = ay.max(-row_height());
                        let bottom = by.min(height + row_height());
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
                self.brush
                    .SetColor(&palette(color, if dimmed { DIMMED_ALPHA } else { 1.0 }));
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
                let dimmed = lineage.is_some_and(|l| !l.contains(n.row));
                self.brush
                    .SetColor(&palette(n.color, if dimmed { DIMMED_ALPHA } else { 1.0 }));
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

    /// 差分が替わった（取っておいた行の文字を捨てる）。
    pub fn forget_diff(&mut self) {
        self.diff_rows.clear();
    }

    /// 差分を描く（`scroll_y` から画面 1 つ分）。`view` がなければ空の地だけ。
    pub fn draw_diff(
        &mut self,
        surface: &mut Surface,
        content: Option<crate::data::DiffContent>,
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
        let result = match content {
            Some(content) => self.draw_diff_rows(content, scroll_y, width, height),
            None => Ok(()),
        };
        unsafe { self.d2d.EndDraw(None, None)? };
        result
    }

    fn draw_diff_rows(
        &mut self,
        commit: crate::data::DiffContent,
        scroll_y: f32,
        width: f32,
        height: f32,
    ) -> Result<()> {
        use amonhen_layout::DiffRow;
        let view = commit.view;
        let count = view.rows.len() as u32;
        let first = ((scroll_y / DIFF_ROW_HEIGHT).floor().max(0.0) as u32).min(count);
        let last = (((scroll_y + height) / DIFF_ROW_HEIGHT).ceil() as u32 + 1).min(count);
        if self.diff_rows.len() > 2048 {
            self.diff_rows.clear();
        }
        // 行ごとの含める印（Working Copy の 1 本の差分）があれば、その列の右から行番号。
        let left = if commit.marks { DIFF_MARK_WIDTH } else { 0.0 };
        let text_x = left + DIFF_NUMBER_WIDTH * 2.0 + DIFF_TEXT_INSET;
        for i in first..last {
            let row = &view.rows[i as usize];
            let top = i as f32 * DIFF_ROW_HEIGHT - scroll_y;
            let rect = |left: f32, right: f32| D2D_RECT_F {
                left,
                top,
                right,
                bottom: top + DIFF_ROW_HEIGHT,
            };
            let file = row.kind == DiffRow::FILE;
            let fill = match row.kind {
                DiffRow::ADDED => Some(DIFF_ADDED),
                DiffRow::REMOVED => Some(DIFF_REMOVED),
                DiffRow::HUNK => Some(DIFF_HUNK),
                DiffRow::FILE => Some(DIFF_FILE),
                _ => None,
            };
            unsafe {
                if !file {
                    self.brush.SetColor(&DIFF_GUTTER);
                    self.d2d
                        .FillRectangle(&rect(0.0, left + DIFF_NUMBER_WIDTH * 2.0), &self.brush);
                }
                if let Some(fill) = fill {
                    self.brush.SetColor(&fill);
                    self.d2d.FillRectangle(&rect(0.0, width), &self.brush);
                }
            }
            if !self.diff_rows.contains_key(&i) {
                let mut text = Vec::with_capacity(128);
                if file {
                    // 畳んだファイルは ▸、開いているファイルは ▾（見出しを押すと切り替わる）。
                    let mark = if row.flags & DiffRow::COLLAPSED != 0 {
                        "▸ "
                    } else {
                        "▾ "
                    };
                    text.extend(mark.encode_utf16());
                }
                crate::data::push_utf16(
                    view.strings.bytes(row.text),
                    row.text.is_ascii(),
                    &mut text,
                );
                if file {
                    text.extend(format!("    +{}  −{}", row.new_no, row.old_no).encode_utf16());
                }
                let format = if file {
                    &self.file_format
                } else {
                    &self.code_format
                };
                let layout = unsafe {
                    self.dwrite
                        .CreateTextLayout(&text, format, 100_000.0, DIFF_ROW_HEIGHT)?
                };
                if let Some(spans) = commit.syntax.get(&i) {
                    for &(start, end, kind) in spans {
                        let Some(brush) = self.syntax_brush(kind)? else {
                            continue;
                        };
                        let range = DWRITE_TEXT_RANGE {
                            startPosition: start,
                            length: end.saturating_sub(start),
                        };
                        unsafe { layout.SetDrawingEffect(&brush, range)? };
                    }
                }
                self.diff_rows.insert(i, layout);
            }
            let layout = &self.diff_rows[&i];
            // 削除行と追加行の組の中で、変わった単語の地。
            if let Some(ranges) = commit.highlights.get(&i) {
                let fill = if row.kind == DiffRow::REMOVED {
                    DIFF_REMOVED_WORD
                } else {
                    DIFF_ADDED_WORD
                };
                for &(start, end) in ranges {
                    let (left, right) = (x_of(layout, start)?, x_of(layout, end)?);
                    unsafe {
                        self.brush.SetColor(&fill);
                        self.d2d.FillRectangle(
                            &rect(text_x + left, text_x + right.max(left + 2.0)),
                            &self.brush,
                        );
                    }
                }
            }
            unsafe {
                self.brush.SetColor(&FOREGROUND);
                let x = if file { DIFF_TEXT_INSET } else { text_x };
                self.d2d.DrawTextLayout(
                    point(x, top + 1.0),
                    layout,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                );
            }
            if !file && row.kind != DiffRow::HUNK {
                self.draw_number(row.old_no, left, top)?;
                self.draw_number(row.new_no, left + DIFF_NUMBER_WIDTH, top)?;
            }
            if commit.marks && matches!(row.kind, DiffRow::ADDED | DiffRow::REMOVED) {
                let mark = if row.flags & DiffRow::STAGED != 0 {
                    Mark::All
                } else {
                    Mark::None
                };
                self.draw_mark(mark, top)?;
            }
            // hunk の見出し: その hunk の行をすべて（☑）・一部（▣）・どれも（☐）含めているか。
            if commit.marks && row.kind == DiffRow::HUNK {
                let rows = crate::data::hunk_rows(&view.rows, i as usize);
                let staged = rows
                    .iter()
                    .filter(|&&r| view.rows[r].flags & DiffRow::STAGED != 0)
                    .count();
                let mark = match staged {
                    0 => Mark::None,
                    n if n == rows.len() => Mark::All,
                    _ => Mark::Some,
                };
                if !rows.is_empty() {
                    self.draw_mark(mark, top)?;
                }
            }
        }
        Ok(())
    }

    /// 構文の種類の文字の色（GTK4 版の System のテーマの明るい外観と同じ。ない種類は本文の色）。
    fn syntax_brush(&mut self, kind: u8) -> Result<Option<ID2D1SolidColorBrush>> {
        use amonhen_layout::SyntaxSpan;
        if let Some(brush) = self.syntax_brushes.get(&kind) {
            return Ok(Some(brush.clone()));
        }
        let (r, g, b) = match kind {
            SyntaxSpan::KEYWORD | SyntaxSpan::TAG | SyntaxSpan::VARIABLE => (0.80, 0.18, 0.45),
            SyntaxSpan::STRING => (0.77, 0.16, 0.16),
            SyntaxSpan::COMMENT => (0.45, 0.45, 0.48),
            SyntaxSpan::NUMBER | SyntaxSpan::CONSTANT => (0.50, 0.25, 0.75),
            SyntaxSpan::FUNCTION => (0.10, 0.40, 0.85),
            SyntaxSpan::TYPE => (0.10, 0.55, 0.60),
            SyntaxSpan::PROPERTY => (0.10, 0.50, 0.70),
            SyntaxSpan::ATTRIBUTE | SyntaxSpan::ESCAPE => (0.80, 0.45, 0.05),
            SyntaxSpan::MODULE => (0.55, 0.40, 0.25),
            _ => return Ok(None),
        };
        let brush = unsafe { self.d2d.CreateSolidColorBrush(&rgba(r, g, b, 1.0), None)? };
        self.syntax_brushes.insert(kind, brush.clone());
        Ok(Some(brush))
    }

    /// 行を次のコミットに含めているかの印（含めていれば塗った四角にチェック、一部なら横線、いなければ枠だけ）。
    fn draw_mark(&self, mark: Mark, top: f32) -> Result<()> {
        let size = 12.0;
        let (x, y) = (
            (DIFF_MARK_WIDTH - size) / 2.0,
            top + (DIFF_ROW_HEIGHT - size) / 2.0,
        );
        let rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: x,
                top: y,
                right: x + size,
                bottom: y + size,
            },
            radiusX: 2.5,
            radiusY: 2.5,
        };
        unsafe {
            match mark {
                Mark::All => {
                    self.brush.SetColor(&DIFF_MARK);
                    self.d2d.FillRoundedRectangle(&rect, &self.brush);
                    self.brush.SetColor(&BACKGROUND);
                    let a = point(x + 2.8, y + 6.2);
                    let b = point(x + 5.2, y + 8.8);
                    let c = point(x + 9.4, y + 3.6);
                    self.d2d.DrawLine(a, b, &self.brush, 1.6, &self.round);
                    self.d2d.DrawLine(b, c, &self.brush, 1.6, &self.round);
                }
                Mark::Some => {
                    self.brush.SetColor(&DIFF_MARK);
                    self.d2d.FillRoundedRectangle(&rect, &self.brush);
                    self.brush.SetColor(&BACKGROUND);
                    let (a, b) = (point(x + 3.0, y + 6.0), point(x + 9.0, y + 6.0));
                    self.d2d.DrawLine(a, b, &self.brush, 1.6, &self.round);
                }
                Mark::None => {
                    self.brush.SetColor(&SECONDARY);
                    self.d2d.DrawRoundedRectangle(&rect, &self.brush, 1.0, None);
                }
            }
        }
        Ok(())
    }

    /// 行番号（0 なら書かない）を、`x` から幅 `DIFF_NUMBER_WIDTH` の列に右寄せで書く。
    fn draw_number(&self, number: u32, x: f32, top: f32) -> Result<()> {
        if number == 0 {
            return Ok(());
        }
        let text: Vec<u16> = number.to_string().encode_utf16().collect();
        unsafe {
            let layout = self.dwrite.CreateTextLayout(
                &text,
                &self.meta_format,
                DIFF_NUMBER_WIDTH - 6.0,
                DIFF_ROW_HEIGHT,
            )?;
            layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
            self.brush.SetColor(&SECONDARY);
            self.d2d.DrawTextLayout(
                point(x, top + 2.0),
                &layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        Ok(())
    }

    /// ラベルの文字の形と幅（作ったものは名前ごとに取っておく）。
    fn label(&mut self, name: &str) -> Result<(IDWriteTextLayout, f32)> {
        if let Some(found) = self.labels.get(name) {
            return Ok(found.clone());
        }
        if self.labels.len() > 2048 {
            self.labels.clear();
        }
        let text: Vec<u16> = name.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&text, &self.label_format, 10000.0, LABEL_HEIGHT)?
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics)? };
        let made = (layout, metrics.widthIncludingTrailingWhitespace.ceil());
        self.labels.insert(name.to_string(), made.clone());
        Ok(made)
    }

    fn draw_label(
        &self,
        label: &crate::data::RefLabel,
        elsewhere: bool,
        layout: &IDWriteTextLayout,
        (x, y, width): (f32, f32, f32),
    ) -> Result<()> {
        // worktree の札と、ほかの worktree が checkout しているブランチは緑（macOS 版と同じ）。
        let (r, g, b) = match label.kind {
            _ if label.worktree || elsewhere => LABEL_WORKTREE,
            RefKind::Local => LABEL_LOCAL,
            RefKind::Remote => LABEL_REMOTE,
            _ => LABEL_TAG,
        };
        let alpha = match (label.head, label.worktree) {
            (true, _) => 0.85,
            (false, true) => 0.25,
            (false, false) => 0.22,
        };
        let fill = rgba(r, g, b, alpha);
        let rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + LABEL_HEIGHT,
            },
            radiusX: 4.0,
            radiusY: 4.0,
        };
        unsafe {
            self.brush.SetColor(&fill);
            self.d2d.FillRoundedRectangle(&rect, &self.brush);
            self.brush.SetColor(if label.head {
                &LABEL_HEAD_TEXT
            } else {
                &LABEL_TEXT
            });
            self.d2d.DrawTextLayout(
                point(x + LABEL_PADDING, y + 1.0),
                layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        Ok(())
    }

    fn row_text(&self, history: &HistorySnapshot, chunk: &Chunk, row: u32) -> Result<RowText> {
        let i = chunk.index(row);
        let mut subject = Vec::with_capacity(96);
        chunk.push_utf16(chunk.subject(i), &mut subject);
        // 作者・日時・ID のうち、出す項目だけを「  ·  」でつなぐ。
        let shown = history_items();
        let mut parts: Vec<Vec<u16>> = Vec::new();
        if shown & items::AUTHOR != 0 {
            let mut author = Vec::with_capacity(32);
            chunk.push_utf16(chunk.author(i), &mut author);
            parts.push(author);
        }
        if shown & items::DATE != 0 {
            parts.push(
                date(chunk.time(i) + self.utc_offset)
                    .encode_utf16()
                    .collect(),
            );
        }
        if shown & items::ID != 0 {
            let id = history.graph.ids()[row as usize].to_string();
            parts.push(id[..8].encode_utf16().collect());
        }
        // 折りたたんだ merge は、隠しているコミットの数を添える（GTK4 版と同じ）。
        if let Some(&hidden) = history.folded.get(row as usize)
            && hidden > 0
        {
            parts.push(
                format!("⊕ {}", amonhen_model::tr!("{count} folded", count = hidden))
                    .encode_utf16()
                    .collect(),
            );
        }
        let separator: Vec<u16> = "  ·  ".encode_utf16().collect();
        let meta = parts.join(&separator[..]);
        unsafe {
            Ok(RowText {
                subject: self.dwrite.CreateTextLayout(
                    &subject,
                    &self.subject_format,
                    10000.0,
                    row_height(),
                )?,
                meta: self.dwrite.CreateTextLayout(
                    &meta,
                    &self.meta_format,
                    10000.0,
                    row_height(),
                )?,
            })
        }
    }
}

/// UTC の秒を、この機械の時間帯で `YYYY-MM-DD HH:MM` にする（History の行と同じ書き方）。
pub fn local_date(utc: i64) -> String {
    let mut tz = TIME_ZONE_INFORMATION::default();
    // SAFETY: 書き込む先は手元の構造体。
    unsafe {
        GetTimeZoneInformation(&mut tz);
    }
    date(utc - i64::from(tz.Bias) * 60)
}

/// 地方時の秒を `YYYY-MM-DD HH:MM` にする（Howard Hinnant の civil_from_days）。
pub fn date(local: i64) -> String {
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

/// 文字の形の中で、`position`（UTF-16 の位置）の文字の左端の x。
fn x_of(layout: &IDWriteTextLayout, position: u32) -> Result<f32> {
    let (mut x, mut y) = (0.0, 0.0);
    let mut metrics = DWRITE_HIT_TEST_METRICS::default();
    unsafe { layout.HitTestTextPosition(position, false, &mut x, &mut y, &mut metrics)? };
    Ok(x)
}
