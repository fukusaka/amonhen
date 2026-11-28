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
    /// GitHub の CI の状態（行の右端の印）。
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
/// CI の印に空ける幅（行の右端から。GTK4 版と同じ）。
const CI_SPACE: f32 = 18.0;

/// History の幅 `width`（DIP）の行で、`x` が CI の印の上か（GTK4 版の `click_ci`。印があるかは見ない）。
pub fn on_ci_mark(width: f32, x: f32) -> bool {
    let end = width - TEXT_INSET;
    x >= end - CI_SPACE && x <= end + 2.0
}

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

/// 地（窓全体を透かすときは窓の地と同じ濃さ。SwapChainPanel の下には窓の地が見えないので自分で塗る。swap chain
/// はアルファを合成に使う）。
fn background() -> D2D1_COLOR_F {
    let c = themed((1.0, 1.0, 1.0, 1.0), (0.118, 0.118, 0.118, 1.0));
    with_alpha(c, crate::palette::bar_ground())
}
fn foreground() -> D2D1_COLOR_F {
    themed((0.11, 0.11, 0.12, 1.0), (0.91, 0.91, 0.91, 1.0))
}
fn secondary() -> D2D1_COLOR_F {
    themed((0.11, 0.11, 0.12, 0.6), (0.91, 0.91, 0.91, 0.6))
}
/// 検索で一致した行の背景。
const MATCH: D2D1_COLOR_F = rgba(1.0, 0.80, 0.0, 0.22);
/// 選んだ行の背景（Windows の既定のアクセントの色を薄く）。
const SELECTION: D2D1_COLOR_F = rgba(0.0, 0.47, 0.84, 0.16);
/// 編集の境目・運ぶ先の印の色（アクセント）。
const ACCENT: D2D1_COLOR_F = rgba(0.21, 0.52, 0.89, 1.0);
/// ref のラベルの色（ローカルブランチは青、リモートブランチは紫、タグは橙。macOS 版・GTK4 版と同じ）。地は薄く、
/// チェックアウト中のブランチは濃く塗って白い文字にする。
const LABEL_LOCAL: (f32, f32, f32) = (0.21, 0.52, 0.89);
const LABEL_REMOTE: (f32, f32, f32) = (0.57, 0.38, 0.84);
const LABEL_TAG: (f32, f32, f32) = (0.90, 0.55, 0.15);
/// worktree の HEAD の札と、ほかの worktree が checkout しているブランチ。
const LABEL_WORKTREE: (f32, f32, f32) = (0.20, 0.66, 0.30);
fn label_text() -> D2D1_COLOR_F {
    themed((0.11, 0.11, 0.12, 0.9), (0.91, 0.91, 0.91, 0.9))
}
const LABEL_HEAD_TEXT: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 1.0);
const LABEL_HEIGHT: f32 = 17.0;
/// 差分の行の高さ（DIP）と、行番号の列の幅。
pub const DIFF_ROW_HEIGHT: f32 = 18.0;
const DIFF_NUMBER_WIDTH: f32 = 44.0;
/// 差分の行の文字の左端（含める印の列・行番号の列の右。その時点のファイルなら行番号は 1 列）。
pub fn text_left(marks: bool, file_mode: bool) -> f32 {
    let left = if marks { DIFF_MARK_WIDTH } else { 0.0 };
    let numbers = if file_mode {
        DIFF_NUMBER_WIDTH
    } else {
        DIFF_NUMBER_WIDTH * 2.0
    };
    left + numbers + DIFF_TEXT_INSET
}

/// History の差分の行番号の欄の幅（hunk の見出しのここを押すと、前後の行を広げる）。
pub const DIFF_GUTTER_WIDTH: f32 = DIFF_NUMBER_WIDTH * 2.0;
const DIFF_TEXT_INSET: f32 = 10.0;
/// ファイルの見出しの、パスの左端（左に ▸・▾ と、足した・消した・変えたの印。macOS 版と同じ並び）。
const HEADER_PATH_X: f32 = 42.0;
/// ファイルの見出しの右端の、いつも出す ⋯ の幅（ここを押すとファイルのメニュー。macOS 版と同じ 30pt）。
pub const HEADER_MENU_WIDTH: f32 = 30.0;
/// 木で見せるときの、深さ 1 つ分の字下げ（macOS 版と同じ 14pt）。
const TREE_INDENT: f32 = 14.0;
const ADDED_TEXT: D2D1_COLOR_F = rgba(0.16, 0.62, 0.26, 1.0);
const REMOVED_TEXT: D2D1_COLOR_F = rgba(0.86, 0.22, 0.20, 1.0);
const MODIFIED_TEXT: D2D1_COLOR_F = rgba(0.93, 0.55, 0.10, 1.0);
// 差分の色（GTK4 版・macOS 版の標準の色と同じ）。
/// 差分の地の色（Settings > Theme のテーマの色。標準の色にしていればこれまでの色）。
fn diff_color(kind: crate::theme::DiffColor) -> D2D1_COLOR_F {
    let (r, g, b, a) = crate::theme::diff(kind);
    rgba(r, g, b, a)
}
fn diff_file() -> D2D1_COLOR_F {
    themed((0.11, 0.11, 0.12, 0.07), (1.0, 1.0, 1.0, 0.08))
}
/// Working Copy の差分の、行の左の帯の列の幅。
pub const DIFF_MARK_WIDTH: f32 = 22.0;

/// 隙間の行の置き場所（差分の欄の座標）。字は形と、その幅・高さ。
pub struct GapLayout {
    /// 中央の札（0 行の見出しの行にはない）と、選んだ行の数を出しているか。
    pub pill: Option<(D2D_RECT_F, (IDWriteTextLayout, f32, f32))>,
    pub pill_selected: bool,
    /// 札の右の開く・畳む（マウスが乗っているときだけ）と、右端の hunk の操作。
    pub buttons: Vec<(
        crate::data::GapAction,
        D2D_RECT_F,
        (IDWriteTextLayout, f32, f32),
    )>,
    /// 左の関数名の左端と字。
    pub function: Option<(f32, (IDWriteTextLayout, f32, f32))>,
    /// hunk の操作が効く行（選んだ行か、hunk の変更すべて）。
    pub target: Vec<usize>,
}

impl GapLayout {
    /// (x, y) にあるボタン（押しやすいよう少し広く取る）。
    pub fn button_at(&self, x: f32, y: f32) -> Option<crate::data::GapAction> {
        self.buttons
            .iter()
            .find(|(_, r, _)| near(r, x, y))
            .map(|(a, _, _)| *a)
    }

    /// (x, y) が札の上か。
    pub fn on_pill(&self, x: f32, y: f32) -> bool {
        self.pill.as_ref().is_some_and(|(r, _)| near(r, x, y))
    }
}

/// `r` を左右 2・上下 3 だけ広げた中に (x, y) があるか。
fn near(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left - 2.0 && x <= r.right + 2.0 && y >= r.top - 3.0 && y <= r.bottom + 3.0
}
const DIFF_MARK: D2D1_COLOR_F = rgba(0.0, 0.47, 0.84, 1.0);
fn diff_gutter() -> D2D1_COLOR_F {
    themed((0.11, 0.11, 0.12, 0.035), (1.0, 1.0, 1.0, 0.04))
}
const LABEL_PADDING: f32 = 6.0;
const LABEL_GAP: f32 = 4.0;
/// ラベルを並べる幅の上限（文字の幅に対する割合）。残りは件名に回す。
const LABEL_SHARE: f32 = 0.6;

/// 署名の検証の結果の色（GTK4 版の `signature_color`。まだ検証していなければ薄い灰色）。
fn signature_color(mark: Option<crate::data::Signature>) -> D2D1_COLOR_F {
    use crate::data::Signature;
    match mark {
        Some(Signature::Verified) => rgba(0.20, 0.70, 0.30, 1.0),
        Some(Signature::Caution) => rgba(0.90, 0.75, 0.10, 1.0),
        Some(Signature::Expired) => rgba(0.95, 0.55, 0.15, 1.0),
        Some(Signature::Bad) => rgba(0.90, 0.25, 0.25, 1.0),
        Some(Signature::Unknown) => with_alpha(foreground(), 0.55),
        None => with_alpha(foreground(), 0.25),
    }
}

/// 外観（明るい・暗い）ごとの色。
fn themed(light: (f32, f32, f32, f32), dark: (f32, f32, f32, f32)) -> D2D1_COLOR_F {
    let (r, g, b, a) = if crate::palette::dark() { dark } else { light };
    rgba(r, g, b, a)
}

fn with_alpha(c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a, ..c }
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
    /// アルファを合成に使うか（Composition の swap chain。窓全体を透かすときに地が透ける）。
    premultiplied: bool,
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
        let (desc, flags, options) = renderer.desc(width, height, options, false);
        let chain = unsafe {
            renderer
                .factory
                .CreateSwapChainForHwnd(&renderer.d3d, hwnd, &desc, None, None)?
        };
        Self::new(chain, width, height, flags, options, false)
    }

    /// Composition（WinUI 3 の SwapChainPanel）に出す swap chain。
    pub fn for_composition(
        renderer: &Renderer,
        width: u32,
        height: u32,
        options: Options,
    ) -> Result<Self> {
        let (desc, flags, options) = renderer.desc(width, height, options, true);
        let chain = unsafe {
            renderer
                .factory
                .CreateSwapChainForComposition(&renderer.d3d, &desc, None)?
        };
        Self::new(chain, width, height, flags, options, true)
    }

    fn new(
        chain: IDXGISwapChain1,
        width: u32,
        height: u32,
        flags: DXGI_SWAP_CHAIN_FLAG,
        options: Options,
        premultiplied: bool,
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
            premultiplied,
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
                alphaMode: if self.premultiplied {
                    D2D1_ALPHA_MODE_PREMULTIPLIED
                } else {
                    D2D1_ALPHA_MODE_IGNORE
                },
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
    /// 差分の中の検索の一致（行、UTF-16 の始まりと終わり。行の文字の形の中の位置）と、今の一致。
    pub diff_find: Vec<(u32, u32, u32)>,
    pub diff_current: Option<usize>,
    /// shallow clone の境界のコミットの行（History の世代ごと）。
    boundary: (u64, std::collections::HashSet<u32>),
    /// `rows` を作ったときの行の項目（変わったら作り直す）。
    rows_items: u32,
    /// ラベルの文字（名前ごと）と、その幅。
    labels: HashMap<String, (IDWriteTextLayout, f32)>,
    label_format: IDWriteTextFormat,
    /// 差分の文字（等幅）と、見出しの文字。
    code_format: IDWriteTextFormat,
    file_format: IDWriteTextFormat,
    icon_format: IDWriteTextFormat,
    /// 隙間の行のボタンの字（小さめの太字）。
    gap_bold_format: IDWriteTextFormat,
    /// 画像の比べる欄の、読んだ前と後の絵（差分の番号とファイルごと。差分が替わったら捨てる）。
    images: HashMap<(u64, usize), ImageEntry>,
    /// 差分の行の文字の形（今の差分の行ごと。差分が替わったら捨てる）。
    diff_rows: HashMap<u32, IDWriteTextLayout>,
    /// 構文の種類ごとの文字の色。
    syntax_brushes: HashMap<u8, ID2D1SolidColorBrush>,
    /// 色を付けた行を作ったときのテーマ（変わったら行と筆を作り直す）。
    theme_generation: u32,
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
            let gap_bold_format = dwrite.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.0,
                w!("ja-jp"),
            )?;
            gap_bold_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            // ファイルの見出しの山形（Segoe Fluent Icons の ChevronRight・ChevronDown）。
            let icon_format = dwrite.CreateTextFormat(
                w!("Segoe Fluent Icons"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                10.0,
                w!("ja-jp"),
            )?;
            let brush = d2d.CreateSolidColorBrush(&foreground(), None)?;
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
                diff_find: Vec::new(),
                diff_current: None,
                boundary: (u64::MAX, std::collections::HashSet::new()),
                rows_items: items::ALL,
                labels: HashMap::new(),
                label_format,
                code_format,
                file_format,
                icon_format,
                gap_bold_format,
                images: HashMap::new(),
                diff_rows: HashMap::new(),
                syntax_brushes: HashMap::new(),
                theme_generation: crate::theme::generation(),
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
        premultiplied: bool,
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
            AlphaMode: if premultiplied {
                DXGI_ALPHA_MODE_PREMULTIPLIED
            } else {
                DXGI_ALPHA_MODE_IGNORE
            },
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
            // 地が透けるときは、ClearType（不透明な地が要る）でなく濃淡で文字をなめらかにする。
            self.d2d
                .SetTextAntialiasMode(if crate::palette::bar_ground() < 1.0 {
                    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE
                } else {
                    D2D1_TEXT_ANTIALIAS_MODE_DEFAULT
                });
            self.d2d.Clear(Some(&background()));
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
        // History の上で編集しているなら、範囲の行と描くコミットをこの History の行で求める。
        if let Some(edit) = data.edit.as_mut() {
            edit.sync(&history);
            edit.chips.clear();
        }
        let first = ((scroll_y / row_height()).floor().max(0.0) as u32).min(count);
        let last = (((scroll_y + height) / row_height()).ceil() as u32 + 1).min(count);
        // 前後 1 画面分も読んでおく。
        let screen = last - first;
        data.want(first.saturating_sub(screen)..(last + screen).min(count));

        // 検索で一致した行の背景（見えている分だけ）。
        // 一致したコミットだけを出しているときは、すべて一致なので塗らない。
        let matches: &[u32] = if data.is_filtered() {
            &[]
        } else {
            &data.matches
        };
        let from = matches.partition_point(|&r| r < first);
        for &row in matches[from..].iter().take_while(|&&r| r < last) {
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
        if self.boundary.0 != history.generation {
            self.boundary = (
                history.generation,
                history.boundary_rows().into_iter().collect(),
            );
        }
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
            // 運んでいる行は空けておく（つかんだ行は指の所に描く）。
            if data.drag.as_ref().is_some_and(|d| d.moving && d.row == row) {
                continue;
            }
            // History の上で編集しているときは、範囲の行に並べ替えた後のコミットを描く（グラフはそのまま）。
            let (src, edit_row) = match data.edit.as_ref() {
                Some(e) => (
                    e.shown_row(row),
                    e.slots
                        .get(&row)
                        .and_then(|&i| e.marks.rows.get(i).cloned()),
                ),
                None => (row, None),
            };
            let Some(chunk) = data.chunks.get(&(src / crate::data::CHUNK_ROWS)).cloned() else {
                continue;
            };
            if !self.rows.contains_key(&src) {
                let made = std::time::Instant::now();
                let text = self.row_text(&history, &chunk, src)?;
                self.rows.insert(src, text);
                stages[2] += made.elapsed().as_secs_f64() * 1000.0;
            }
            let top = row as f32 * row_height() - scroll_y;
            let compact = compact();
            let shown = history_items();
            // GitHub の CI の状態（知っていれば）。行の右端に置き、署名の印とは離す（GTK4 版と同じ）。
            let ci = (shown & items::CI != 0)
                .then(|| data.ci.get(&history.graph.ids()[src as usize]).copied())
                .flatten();
            let text_width = if ci.is_some() {
                (text_width - CI_SPACE).max(0.0)
            } else {
                text_width
            };
            let label_top = if compact {
                (row_height() - LABEL_HEIGHT) / 2.0
            } else {
                2.0
            };
            let signed = shown & items::SIGNATURE != 0 && chunk.signed(chunk.index(src));
            let mark_width = if signed { 14.0 } else { 0.0 };
            // 1 行なら、作者・日時を右端に寄せ（GTK4 版の 1 行表示と同じく件名の右）、ラベルと件名はその手前まで。
            let (meta_x, meta_y, subject_right) = unsafe {
                let meta = &self.rows[&src].meta;
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
            // 操作の印（git の言葉の札）は、件名の場所の右端に（merge の行には出さない）。
            let subject_right = match edit_row.as_ref().filter(|r| !r.merge) {
                Some(r) => {
                    let w = self.draw_edit_chip(r, subject_right, top + label_top)?;
                    if let Some(edit) = data.edit.as_mut() {
                        edit.chips.insert(row, (subject_right - w, subject_right));
                    }
                    subject_right - w - 8.0
                }
                None => subject_right,
            };
            // ref のラベルを件名の前に並べる。
            let mut subject_x = text_x;
            // worktree の HEAD の札を先に。
            let labels = data
                .worktree_labels
                .get(&src)
                .into_iter()
                .chain(data.labels.get(&src))
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
            let mark = signed.then(|| data.signature_mark(history.graph.ids()[src as usize]));
            // 直したメッセージは、その件名を描く。取り消す行は線で消す。まとめる行・取り消す行の件名は薄く。
            let custom = match edit_row.as_ref() {
                Some(r) if r.subject.is_some() || r.action == crate::historyedit::Action::Drop => {
                    Some(self.edited_subject(r, &chunk, src)?)
                }
                _ => None,
            };
            let faint = edit_row.as_ref().is_some_and(|r| {
                use crate::historyedit::Action;
                matches!(r.action, Action::Drop | Action::Squash | Action::Fixup)
            });
            let line = &self.rows[&src];
            let subject = custom.as_ref().unwrap_or(&line.subject);
            unsafe {
                subject.SetMaxWidth((subject_right - subject_x).max(0.0))?;
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
                self.brush
                    .SetColor(&if faint { secondary() } else { foreground() });
                self.d2d.DrawTextLayout(
                    point(subject_x, top + if compact { 3.0 } else { 2.0 }),
                    subject,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                self.brush.SetColor(&secondary());
                self.d2d.DrawTextLayout(
                    point(meta_x, meta_y),
                    &line.meta,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            if let Some(ci) = ci {
                self.draw_ci(ci, text_x + text_width + 9.0, top + row_height() / 2.0);
            }
        }
        self.draw_edit_overlay(data, first..last, scroll_y, width)?;
        self.draw_drag(data, &history, scroll_y, width, graph_width)?;
        if profiling {
            unsafe { self.d2d.Flush(None, None)? };
        }
        stages[3] = lap(&mut t) - stages[2];
        // 描いた行で頼まれた署名を、裏で検証する。
        data.verify_signatures();
        Ok(())
    }

    /// 編集の操作の印（git の言葉の札）を、右端 `right`・上端 `top` に描いて幅を返す。そのままは薄い灰色、取り消すは
    /// 赤、ほかは濃い色、コンフリクトしそうなら橙（macOS 版の `drawEditChip`、GTK4 版の `draw_edit_chip`）。
    fn draw_edit_chip(
        &mut self,
        row: &crate::historyedit::MarkRow,
        right: f32,
        top: f32,
    ) -> Result<f32> {
        use crate::historyedit::Action;
        let (layout, w) = self.label(row.action.name())?;
        let width = (w + 12.0).round();
        let fg = foreground();
        let (fill, ink) = if row.conflict {
            (rgba(0.96, 0.55, 0.10, 0.85), rgba(1.0, 1.0, 1.0, 1.0))
        } else {
            match row.action {
                Action::Pick => (with_alpha(fg, 0.08), with_alpha(fg, 0.6)),
                Action::Drop => (rgba(0.90, 0.25, 0.25, 0.8), rgba(1.0, 1.0, 1.0, 1.0)),
                _ => (with_alpha(fg, 0.7), background()),
            }
        };
        let x = right - width;
        unsafe {
            self.brush.SetColor(&fill);
            self.d2d.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top,
                        right,
                        bottom: top + LABEL_HEIGHT,
                    },
                    radiusX: LABEL_HEIGHT / 2.0,
                    radiusY: LABEL_HEIGHT / 2.0,
                },
                &self.brush,
            );
            self.brush.SetColor(&ink);
            self.d2d.DrawTextLayout(
                point(x + 6.0, top + 1.0),
                &layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        Ok(width)
    }

    /// 編集で直した件名（直していなければ元の件名）の文字の形。取り消す行は線で消す。
    fn edited_subject(
        &self,
        row: &crate::historyedit::MarkRow,
        chunk: &Chunk,
        src: u32,
    ) -> Result<IDWriteTextLayout> {
        let text: Vec<u16> = match &row.subject {
            Some(subject) => subject.encode_utf16().collect(),
            None => {
                let mut text = Vec::with_capacity(96);
                chunk.push_utf16(chunk.subject(chunk.index(src)), &mut text);
                text
            }
        };
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&text, &self.subject_format, 10000.0, row_height())?
        };
        if row.action == crate::historyedit::Action::Drop {
            unsafe {
                layout.SetStrikethrough(
                    true,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: text.len() as u32,
                    },
                )?;
            }
        }
        Ok(layout)
    }

    /// 編集の範囲の外の行をグラフごと薄くし（上に地の色を重ねる。macOS 版・GTK4 版と同じ）、範囲のいちばん下に
    /// 境目（下へ引くと広げ、上へ引くと狭める）を引く。
    fn draw_edit_overlay(
        &self,
        data: &Data,
        rows: std::ops::Range<u32>,
        scroll_y: f32,
        width: f32,
    ) -> Result<()> {
        let Some(edit) = data.edit.as_ref() else {
            return Ok(());
        };
        let veil = with_alpha(background(), 0.62);
        unsafe {
            self.brush.SetColor(&veil);
            for row in rows.clone().filter(|r| !edit.slots.contains_key(r)) {
                let top = row as f32 * row_height() - scroll_y;
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
            if let Some(row) = edit.boundary_row().filter(|r| rows.contains(r)) {
                let y = (row + 1) as f32 * row_height() - scroll_y;
                self.brush.SetColor(&ACCENT);
                self.d2d.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top: y - 1.0,
                        right: width,
                        bottom: y + 1.0,
                    },
                    &self.brush,
                );
                self.d2d.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: 6.0,
                            top: y - 4.0,
                            right: 36.0,
                            bottom: y + 4.0,
                        },
                        radiusX: 4.0,
                        radiusY: 4.0,
                    },
                    &self.brush,
                );
            }
        }
        Ok(())
    }

    /// コミットを運んでいる間: 落とせない行を薄くし、行に重ねるならその行を囲み、行の間ならそこに線を引き、つかんだ
    /// 行を指の所に描く（macOS 版・GTK4 版と同じ）。
    fn draw_drag(
        &mut self,
        data: &Data,
        history: &HistorySnapshot,
        scroll_y: f32,
        width: f32,
        graph_width: f32,
    ) -> Result<()> {
        let Some(drag) = data.drag.as_ref().filter(|d| d.moving) else {
            return Ok(());
        };
        let count = history.layout.nodes().len() as u32;
        let first = ((scroll_y / row_height()).floor().max(0.0) as u32).min(count);
        let last = (first + (drag.height / row_height()).ceil() as u32 + 2).min(count);
        let rect = |row: u32| {
            let top = row as f32 * row_height() - scroll_y;
            D2D_RECT_F {
                left: 0.0,
                top,
                right: width,
                bottom: top + row_height(),
            }
        };
        unsafe {
            // 編集の外では、編集できる並びの外の行を薄くする（並びを読み終えるまでは薄くしない）。
            if data.edit.is_none()
                && let Some(chain) = drag.chain.as_ref()
            {
                let ids = history.graph.ids();
                let movable = ids
                    .get(drag.row as usize)
                    .is_some_and(|id| chain.contains(id));
                self.brush.SetColor(&with_alpha(background(), 0.62));
                for row in first..last {
                    if movable && ids.get(row as usize).is_some_and(|id| chain.contains(id)) {
                        continue;
                    }
                    self.d2d.FillRectangle(&rect(row), &self.brush);
                }
            }
            match drag.target {
                Some((row, true)) => {
                    let r = rect(row);
                    self.brush.SetColor(&with_alpha(ACCENT, 0.18));
                    self.d2d.FillRectangle(&r, &self.brush);
                    self.brush.SetColor(&ACCENT);
                    self.d2d.DrawRectangle(
                        &D2D_RECT_F {
                            left: r.left + 1.0,
                            top: r.top + 1.0,
                            right: r.right - 1.0,
                            bottom: r.bottom - 1.0,
                        },
                        &self.brush,
                        2.0,
                        None,
                    );
                }
                Some((row, false)) => {
                    let y = row as f32 * row_height() - scroll_y;
                    self.brush.SetColor(&ACCENT);
                    self.d2d.FillRectangle(
                        &D2D_RECT_F {
                            left: graph_width,
                            top: y - 1.0,
                            right: width,
                            bottom: y + 1.0,
                        },
                        &self.brush,
                    );
                    self.d2d.DrawEllipse(
                        &D2D1_ELLIPSE {
                            point: point(graph_width - 4.0, y),
                            radiusX: 4.0,
                            radiusY: 4.0,
                        },
                        &self.brush,
                        2.0,
                        None,
                    );
                }
                None => {}
            }
        }
        // つかんだ行（点・ラベル・件名・作者）を、指の所に浮かせて描く。
        let src = data.shown_row(drag.row);
        let Some(text) = self.rows.get(&src) else {
            return Ok(());
        };
        let (subject, meta) = (text.subject.clone(), text.meta.clone());
        let top = drag.y - drag.offset;
        let text_x = graph_width + TEXT_INSET;
        unsafe {
            let card = D2D_RECT_F {
                left: 2.0,
                top,
                right: width - 2.0,
                bottom: top + row_height(),
            };
            self.brush.SetColor(&with_alpha(background(), 0.95));
            self.d2d.FillRectangle(&card, &self.brush);
            self.brush.SetColor(&with_alpha(ACCENT, 0.7));
            self.d2d.DrawRectangle(&card, &self.brush, 1.0, None);
            if let Some(node) = history.layout.nodes().get(src as usize) {
                let x = LEFT_INSET + node.lane as f32 * LANE_WIDTH;
                self.brush.SetColor(&palette(node.color, 1.0));
                self.d2d.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: point(x.min(graph_width - 6.0), top + row_height() / 2.0),
                        radiusX: NODE_RADIUS,
                        radiusY: NODE_RADIUS,
                    },
                    &self.brush,
                );
            }
        }
        let mut x = text_x;
        let compact = compact();
        let label_top = if compact {
            (row_height() - LABEL_HEIGHT) / 2.0
        } else {
            2.0
        };
        if history_items() & items::REFS != 0 {
            let labels: Vec<crate::data::RefLabel> = data
                .worktree_labels
                .get(&src)
                .into_iter()
                .chain(data.labels.get(&src))
                .flatten()
                .cloned()
                .collect();
            for label in &labels {
                let (layout, w) = self.label(&label.name)?;
                let pill = w + LABEL_PADDING * 2.0;
                if x + pill > text_x + (width - text_x) * LABEL_SHARE {
                    break;
                }
                self.draw_label(label, false, &layout, (x, top + label_top, pill))?;
                x += pill + LABEL_GAP;
            }
        }
        unsafe {
            subject.SetMaxWidth((width - TEXT_INSET - x).max(0.0))?;
            self.brush.SetColor(&foreground());
            self.d2d.DrawTextLayout(
                point(x, top + if compact { 3.0 } else { 2.0 }),
                &subject,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            if !compact {
                meta.SetMaxWidth((width - TEXT_INSET - text_x).max(0.0))?;
                self.brush.SetColor(&secondary());
                self.d2d.DrawTextLayout(
                    point(text_x, top + 20.0),
                    &meta,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
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
                // shallow clone の境界のコミット（親はあるが取っていない）は、点から下へ短い点線（GTK4 版・
                // macOS 版の `unfetched_stub`。行の下半分に、点の縁から 3.5 離して 4 ずつ）。
                if self.boundary.1.contains(&n.row) {
                    let alpha = if dimmed { DIMMED_ALPHA } else { 1.0 } * 0.8;
                    self.brush.SetColor(&palette(n.color, alpha));
                    let mut y = NODE_RADIUS + 3.5;
                    while y <= row_height() / 2.0 + 1.0 {
                        self.d2d.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: point(x(n.lane), row_y(n.row) + y),
                                radiusX: 1.1,
                                radiusY: 1.1,
                            },
                            &self.brush,
                        );
                        y += 4.0;
                    }
                }
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
            // 地が透けるときは、ClearType（不透明な地が要る）でなく濃淡で文字をなめらかにする。
            self.d2d
                .SetTextAntialiasMode(if crate::palette::bar_ground() < 1.0 {
                    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE
                } else {
                    D2D1_TEXT_ANTIALIAS_MODE_DEFAULT
                });
            self.d2d.Clear(Some(&background()));
        }
        let result = match content {
            // すべてのファイルで開いた画像は 1 枚の絵、読めない・バイナリのファイルは「バイナリファイル」。
            Some(content) if content.picture.is_some() => {
                self.draw_single_picture(&content, width, height)
            }
            Some(content) if content.file_mode && content.view.binary => {
                let format = self.file_format.clone();
                self.draw_centered(
                    &amonhen_model::tr!("Binary file"),
                    &format,
                    crate::imagediff::Rect {
                        x: 0.0,
                        y: 0.0,
                        w: width,
                        h: height,
                    },
                    &secondary(),
                )
            }
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
        // 表示の行（木で見せるときは、フォルダの行を差し込み、隠したファイルを除いた並び）。
        let count = commit.line_count() as u32;
        let first = ((scroll_y / DIFF_ROW_HEIGHT).floor().max(0.0) as u32).min(count);
        let last = (((scroll_y + height) / DIFF_ROW_HEIGHT).ceil() as u32 + 1).min(count);
        if self.diff_rows.len() > 2048 {
            self.diff_rows.clear();
        }
        if self.theme_generation != crate::theme::generation() {
            self.theme_generation = crate::theme::generation();
            self.diff_rows.clear();
            self.syntax_brushes.clear();
        }
        // 行ごとの含める印（Working Copy の 1 本の差分）があれば、その列の右から行番号。
        let left = if commit.marks { DIFF_MARK_WIDTH } else { 0.0 };
        // その時点のファイルは、行番号が 1 列（後の側。ファイルごと消したなら前の側）。
        let numbers = if commit.file_mode {
            DIFF_NUMBER_WIDTH
        } else {
            DIFF_NUMBER_WIDTH * 2.0
        };
        let text_x = left + numbers + DIFF_TEXT_INSET;
        // 画像の比べる欄は、見えている最初の行で欄ごと描く（欄の行は 1 度だけ）。
        let mut drawn_panel = None;
        for n in first..last {
            let top = n as f32 * DIFF_ROW_HEIGHT - scroll_y;
            // 折り返した行は、その行の文字の範囲（続きの行なら Some）。最初の表示の行は、最初の範囲の終わりまで。
            let mut segment: Option<(u32, u32)> = None;
            let i = match commit.line(n as usize) {
                Some(crate::data::DiffLine::Row(i)) => {
                    if let Some(&end) = commit.tree.and_then(|t| t.first_end.get(&i)) {
                        segment = Some((0, end));
                    }
                    i
                }
                Some(crate::data::DiffLine::Cont(i, start, end)) => {
                    segment = Some((start, end));
                    i
                }
                Some(crate::data::DiffLine::Deleted(d)) => {
                    let count = commit
                        .tree
                        .and_then(|t| t.deletions.get(d as usize).copied())
                        .unwrap_or(0);
                    self.draw_deletion_mark(count, left + numbers, top)?;
                    continue;
                }
                Some(crate::data::DiffLine::Panel(file, k)) => {
                    if drawn_panel != Some(file) {
                        drawn_panel = Some(file);
                        let panel_top = top - k as f32 * DIFF_ROW_HEIGHT;
                        self.draw_image_panel(&commit, file as usize, panel_top, width)?;
                    }
                    continue;
                }
                Some(crate::data::DiffLine::Folder(f)) => {
                    if let Some(folder) = commit.tree.and_then(|t| t.folders.get(f as usize)) {
                        self.draw_folder(folder, top, width)?;
                    }
                    continue;
                }
                None => continue,
            };
            let row = &view.rows[i as usize];
            let rect = |left: f32, right: f32| D2D_RECT_F {
                left,
                top,
                right,
                bottom: top + DIFF_ROW_HEIGHT,
            };
            let file = row.kind == DiffRow::FILE;
            // その時点のファイルの、変えた行（消した行に続けて足した行）は青（macOS 版・GTK4 版と同じ）。
            let modified = commit.file_mode && commit.tree.is_some_and(|t| t.modified.contains(&i));
            let fill = match row.kind {
                _ if modified => Some(rgba(0.21, 0.52, 0.89, 0.2)),
                DiffRow::ADDED => Some(diff_color(crate::theme::DiffColor::Added)),
                DiffRow::REMOVED => Some(diff_color(crate::theme::DiffColor::Removed)),
                // 畳んだ差分の隙間の行は地を塗らない（破線と札。GTK4 版・macOS 版と同じ）。
                DiffRow::HUNK if commit.folds.is_none() => {
                    Some(diff_color(crate::theme::DiffColor::Hunk))
                }
                DiffRow::FILE => Some(diff_file()),
                _ => None,
            };
            unsafe {
                if !file {
                    self.brush.SetColor(&diff_gutter());
                    self.d2d
                        .FillRectangle(&rect(0.0, left + numbers), &self.brush);
                }
                if let Some(fill) = fill {
                    self.brush.SetColor(&fill);
                    self.d2d.FillRectangle(&rect(0.0, width), &self.brush);
                }
                // 選んだ行（Working Copy の差分。macOS 版・GTK4 版の行の選択）。
                if commit.selection.is_some_and(|s| s.contains(&i)) {
                    self.brush.SetColor(&SELECTION);
                    self.d2d.FillRectangle(&rect(left, width), &self.brush);
                }
            }
            if let Some(folds) = commit.folds
                && row.kind == DiffRow::HUNK
            {
                self.draw_gap_row(folds, i, top, width, text_x)?;
                continue;
            }
            let title = self.header_title(&commit, i);
            self.diff_layout(view, commit.syntax, i, title.as_deref())?;
            let layout = &self.diff_rows[&i];
            // 木の見出しはパスの最後だけを出すので、検索の位置はその前の分をずらす（前の分の一致は出さない）。
            let shift = title.as_ref().map_or(0, |t| {
                let path = String::from_utf8_lossy(view.strings.bytes(row.text));
                path.encode_utf16()
                    .count()
                    .saturating_sub(t.encode_utf16().count()) as u32
            });
            // 折り返した行: その範囲の左端だけ左へずらして描き、範囲の外は切る。
            let continued = segment.is_some_and(|(start, _)| start > 0);
            let (x0, x1) = match segment {
                Some((start, end)) => (x_of(layout, start)?, x_of(layout, end)?),
                None => (0.0, f32::MAX),
            };
            let base = if file {
                HEADER_PATH_X + self.header_indent(&commit, i)
            } else {
                text_x - x0
            };
            if segment.is_some() {
                unsafe {
                    self.d2d.PushAxisAlignedClip(
                        &rect(text_x, (text_x + x1 - x0 + 1.0).min(width)),
                        D2D1_ANTIALIAS_MODE_ALIASED,
                    );
                }
            }
            // 差分の中の検索の一致（黄色、今の一致は橙。GTK4 版の差分の検索の帯）。
            let from = self.diff_find.partition_point(|m| m.0 < i);
            for (k, &(_, start, end)) in self.diff_find[from..]
                .iter()
                .enumerate()
                .take_while(|(_, m)| m.0 == i)
            {
                if start < shift {
                    continue;
                }
                let (left, right) = (x_of(layout, start - shift)?, x_of(layout, end - shift)?);
                let fill = if self.diff_current == Some(from + k) {
                    rgba(1.0, 0.6, 0.1, 0.65)
                } else {
                    rgba(1.0, 0.85, 0.2, 0.55)
                };
                unsafe {
                    self.brush.SetColor(&fill);
                    self.d2d.FillRectangle(
                        &rect(base + left, base + right.max(left + 2.0)),
                        &self.brush,
                    );
                }
            }
            // 削除行と追加行の組の中で、変わった単語の地。
            if let Some(ranges) = commit.highlights.get(&i) {
                let fill = if row.kind == DiffRow::REMOVED {
                    diff_color(crate::theme::DiffColor::RemovedWord)
                } else {
                    diff_color(crate::theme::DiffColor::AddedWord)
                };
                for &(start, end) in ranges {
                    let (left, right) = (x_of(layout, start)?, x_of(layout, end)?);
                    unsafe {
                        self.brush.SetColor(&fill);
                        self.d2d.FillRectangle(
                            &rect(base + left, base + right.max(left + 2.0)),
                            &self.brush,
                        );
                    }
                }
            }
            if file {
                self.draw_file_header(&commit, i, top, width)?;
            } else {
                unsafe {
                    self.brush.SetColor(&foreground());
                    self.d2d.DrawTextLayout(
                        point(base, top + 1.0),
                        layout,
                        &self.brush,
                        D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    );
                }
            }
            if segment.is_some() {
                unsafe { self.d2d.PopAxisAlignedClip() };
            }
            // 続きの行には、行番号・印を出さない（最初の表示の行だけ。macOS 版・GTK4 版と同じ）。
            if continued {
                continue;
            }
            if commit.file_mode {
                let line = if row.new_no == 0 {
                    row.old_no
                } else {
                    row.new_no
                };
                self.draw_number(line, left, top)?;
            } else if !file && row.kind != DiffRow::HUNK {
                self.draw_number(row.old_no, left, top)?;
                self.draw_number(row.new_no, left + DIFF_NUMBER_WIDTH, top)?;
            }
            // hunk の見出しの行番号の欄に、前後の行を広げる印（macOS 版の `drawExpandMark`）。
            if commit.widen && row.kind == DiffRow::HUNK {
                unsafe {
                    self.brush.SetColor(&rgba(0.0, 0.47, 0.84, 0.18));
                    self.d2d
                        .FillRectangle(&rect(0.0, DIFF_GUTTER_WIDTH), &self.brush);
                }
                let format = self.file_format.clone();
                let text: Vec<u16> = "⤒  ⤓".encode_utf16().collect();
                unsafe {
                    let layout = self.dwrite.CreateTextLayout(
                        &text,
                        &format,
                        DIFF_GUTTER_WIDTH,
                        DIFF_ROW_HEIGHT,
                    )?;
                    layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                    self.brush.SetColor(&DIFF_MARK);
                    self.d2d.DrawTextLayout(
                        point(0.0, top + 1.0),
                        &layout,
                        &self.brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
            }
            // 変更の行の左の帯（All は staged か、「変更の帯」の形ではかたまりの印）。
            if commit.marks
                && let Some(folds) = commit.folds
                && matches!(row.kind, DiffRow::ADDED | DiffRow::REMOVED)
            {
                self.draw_bar(folds, i, row, top, left + numbers);
            }
        }
        // 見出しの行が上に隠れたファイルは、見出しを上に張り付かせる（次のファイルの見出しが来たら押し上げる。
        // macOS 版・GTK4 版と同じ）。
        if let Some((n, top)) = sticky_header(commit.file_lines(), scroll_y)
            && let Some(crate::data::DiffLine::Row(i)) = commit.line(n)
        {
            let i = i as usize;
            let title = self.header_title(&commit, i as u32);
            self.diff_layout(view, commit.syntax, i as u32, title.as_deref())?;
            unsafe {
                self.brush.SetColor(&background());
                self.d2d.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top,
                        right: width,
                        bottom: top + DIFF_ROW_HEIGHT,
                    },
                    &self.brush,
                );
                self.brush.SetColor(&diff_file());
                self.d2d.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top,
                        right: width,
                        bottom: top + DIFF_ROW_HEIGHT,
                    },
                    &self.brush,
                );
            }
            self.draw_file_header(&commit, i as u32, top, width)?;
        }
        Ok(())
    }

    /// ファイルの見出しの行の中身（地は塗ってあるもの）: 左に ▸・▾、足した（A）・消した（D）・変えた（M）の印、
    /// パス。右に足した・消した行の数（バイナリなら Binary）と、いつも出す ⋯（右クリックと同じメニュー）。
    /// macOS 版の `drawCompactHeader` と同じ並び。
    fn draw_file_header(
        &mut self,
        commit: &crate::data::DiffContent,
        i: u32,
        top: f32,
        width: f32,
    ) -> Result<()> {
        use amonhen_layout::DiffRow;
        let row = &commit.view.rows[i as usize];
        let file = commit.file_rows.binary_search(&i).ok();
        let indent = self.header_indent(commit, i);
        // 比べる欄を出している画像は、差分を畳んでいても開いた ▾。
        let open = row.flags & DiffRow::COLLAPSED == 0
            || file.is_some_and(|f| commit.tree.is_some_and(|t| t.panels.contains(&f)));
        // 右から置く: ⋯、行の数。
        let mut right = width - HEADER_MENU_WIDTH;
        // ⋯ は、ほかより一回り大きく（macOS 版と同じ）。Working Copy で並べた差分では、そのファイルの Stage・
        // Unstage・Discard…（macOS 版・GTK4 版と同じ）。
        let large = self.subject_format.clone();
        self.draw_small(
            "⋯",
            &large,
            width - HEADER_MENU_WIDTH,
            top,
            HEADER_MENU_WIDTH - 6.0,
            &secondary(),
            true,
        )?;
        // まだ差分を取っていないファイルは、詳細の `stats` の行の数（macOS 版の `fileCounts`）。
        let (added, removed) = if row.flags & DiffRow::PENDING != 0 {
            let path = String::from_utf8_lossy(commit.view.strings.bytes(row.text));
            commit
                .counts
                .and_then(|c| c.get(path.as_ref()).copied())
                .unwrap_or((0, 0))
        } else {
            (row.new_no, row.old_no)
        };
        let path = String::from_utf8_lossy(commit.view.strings.bytes(row.text));
        // partial clone で中身が手元にないファイルは「まだ手元にない」、画像は行の数の代わりに「画像」（macOS 版の
        // `fileNotes`）。
        let counts: Vec<(String, D2D1_COLOR_F)> = if row.flags & DiffRow::MISSING != 0 {
            vec![(amonhen_model::tr!("Not here yet"), secondary())]
        } else if commit.stacked.is_some() && crate::imagediff::is_image(&path) {
            vec![(amonhen_model::tr!("Image"), secondary())]
        } else if row.flags & DiffRow::BINARY != 0 {
            vec![(amonhen_model::tr!("Binary"), secondary())]
        } else if added == 0 && removed == 0 {
            Vec::new()
        } else {
            vec![
                (format!("+{added}"), ADDED_TEXT),
                (format!("−{removed}"), REMOVED_TEXT),
            ]
        };
        let format = self.file_format.clone();
        let mut placed = Vec::new();
        for (text, color) in counts.iter().rev() {
            let (layout, w) = self.measured(text, &format)?;
            right -= w;
            placed.push((layout, right, *color));
            right -= 6.0;
        }
        let right = right - 8.0;
        unsafe {
            self.d2d.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: 0.0,
                    top,
                    right: right.max(HEADER_PATH_X + indent),
                    bottom: top + DIFF_ROW_HEIGHT,
                },
                D2D1_ANTIALIAS_MODE_ALIASED,
            );
        }
        let drawn = (|| -> Result<()> {
            self.draw_small(
                if open { "\u{E70D}" } else { "\u{E76C}" },
                &self.icon_format.clone(),
                DIFF_TEXT_INSET + indent,
                top + 3.0,
                14.0,
                &secondary(),
                false,
            )?;
            if let Some(mark) = file.and_then(|f| commit.file_marks.get(f)) {
                let color = match mark {
                    'A' => ADDED_TEXT,
                    'D' => REMOVED_TEXT,
                    _ => MODIFIED_TEXT,
                };
                self.draw_small(
                    &mark.to_string(),
                    &self.code_format.clone(),
                    DIFF_TEXT_INSET + indent + 16.0,
                    top,
                    14.0,
                    &color,
                    false,
                )?;
            }
            let layout = &self.diff_rows[&i];
            unsafe {
                self.brush.SetColor(&foreground());
                self.d2d.DrawTextLayout(
                    point(HEADER_PATH_X + indent, top + 1.0),
                    layout,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                );
            }
            Ok(())
        })();
        unsafe { self.d2d.PopAxisAlignedClip() };
        drawn?;
        for (layout, x, color) in placed {
            unsafe {
                self.brush.SetColor(&color);
                self.d2d.DrawTextLayout(
                    point(x, top + 1.0),
                    &layout,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
        Ok(())
    }

    /// 差分の `row` 行目（ファイルの見出し）の木の字下げ（リストなら 0）。
    fn header_indent(&self, commit: &crate::data::DiffContent, row: u32) -> f32 {
        commit
            .file_rows
            .binary_search(&row)
            .map_or(0.0, |f| commit.file_title(f).0 as f32 * TREE_INDENT)
    }

    /// 差分の `row` 行目（ファイルの見出し）に、パスの代わりに出す名前（木のとき。リストなら None）。
    fn header_title(&self, commit: &crate::data::DiffContent, row: u32) -> Option<String> {
        let file = commit.file_rows.binary_search(&row).ok()?;
        commit.file_title(file).1.map(str::to_owned)
    }

    /// 木のフォルダの行: 字下げして、山形・フォルダの印・名前（押すと中のファイルごと畳む・開く。macOS 版と同じ）。
    fn draw_folder(
        &mut self,
        folder: &crate::data::FolderLine,
        top: f32,
        width: f32,
    ) -> Result<()> {
        let indent = folder.depth as f32 * TREE_INDENT;
        unsafe {
            self.brush.SetColor(&diff_file());
            self.d2d.FillRectangle(
                &D2D_RECT_F {
                    left: 0.0,
                    top,
                    right: width,
                    bottom: top + DIFF_ROW_HEIGHT,
                },
                &self.brush,
            );
        }
        let icons = self.icon_format.clone();
        self.draw_small(
            if folder.collapsed {
                "\u{E76C}"
            } else {
                "\u{E70D}"
            },
            &icons,
            DIFF_TEXT_INSET + indent,
            top + 3.0,
            14.0,
            &secondary(),
            false,
        )?;
        self.draw_icon(
            "folder",
            DIFF_TEXT_INSET + indent + 16.0,
            top + (DIFF_ROW_HEIGHT - 14.0) / 2.0,
            14.0,
            &secondary(),
        )?;
        let format = self.file_format.clone();
        self.draw_small(
            &folder.name,
            &format,
            HEADER_PATH_X + indent,
            top,
            (width - HEADER_PATH_X - indent).max(0.0),
            &secondary(),
            false,
        )
    }

    /// その時点のファイルの、消した行の所の印（赤い札に「N 行消した」。GTK4 版の `draw_deletion_marks`）。
    fn draw_deletion_mark(&self, count: u32, gutter: f32, top: f32) -> Result<()> {
        let red = rgba(0.88, 0.11, 0.14, 1.0);
        let text = amonhen_model::tr!("{count} lines deleted here", count = count);
        let (layout, w) = self.measured(&text, &self.meta_format)?;
        unsafe {
            self.brush.SetColor(&rgba(0.88, 0.11, 0.14, 0.16));
            self.d2d.FillRectangle(
                &D2D_RECT_F {
                    left: gutter + 4.0,
                    top: top + 2.0,
                    right: gutter + 4.0 + w + 16.0,
                    bottom: top + DIFF_ROW_HEIGHT - 2.0,
                },
                &self.brush,
            );
            self.brush.SetColor(&red);
            self.d2d.DrawTextLayout(
                point(gutter + 12.0, top + 2.0),
                &layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        Ok(())
    }

    /// 画像の比べる欄の、前と後を重ねる大きさ（読んでいなければ 0）。
    pub fn image_common(&self, key: u64, file: usize) -> (f32, f32) {
        let size =
            |p: &Option<crate::imagediff::Pixels>| p.as_ref().map_or((0.0, 0.0), |p| p.size());
        self.images.get(&(key, file)).map_or((0.0, 0.0), |e| {
            let (a, b) = (size(&e.before), size(&e.after));
            (a.0.max(b.0), a.1.max(b.1))
        })
    }

    /// すべてのファイルで開いた画像を、欄いっぱいに 1 枚で描く（GTK4 版の `imagediff::single`）: 右上に拡大縮小、
    /// 下に大きさとバイト数。読めなければ「バイナリファイル」。
    fn draw_single_picture(
        &mut self,
        content: &crate::data::DiffContent,
        width: f32,
        height: f32,
    ) -> Result<()> {
        use crate::imagediff as image;
        let Some((bytes, view)) = content.picture else {
            return Ok(());
        };
        let key = (content.key, usize::MAX);
        if !self.images.contains_key(&key) {
            self.images.retain(|k, _| k.0 == content.key);
            // WIC で読めなければ、SVG として描く（中に <svg があるときだけ）。
            let svg = || bytes.windows(4).any(|w| w == b"<svg");
            let pixels = image::decode(bytes)
                .or_else(|| svg().then(|| self.rasterize_svg(bytes).ok()).flatten());
            let caption = match content.picture_caption {
                Some(caption) => caption.to_owned(),
                None => image::describe(Some(bytes), pixels.as_ref()),
            };
            let bitmap = pixels.as_ref().and_then(|p| self.bitmap(p).ok());
            let tone = image::tone(&[pixels.as_ref()]);
            self.images.insert(
                key,
                ImageEntry {
                    before: None,
                    after: pixels,
                    before_bitmap: None,
                    after_bitmap: bitmap,
                    caption,
                    difference: None,
                    tone,
                },
            );
        }
        let entry = &self.images[&key];
        let format = self.file_format.clone();
        let full = image::Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        };
        let Some(bitmap) = entry.after_bitmap.clone() else {
            return self.draw_centered(
                &amonhen_model::tr!("Binary file"),
                &format,
                full,
                &secondary(),
            );
        };
        let size = entry.after.as_ref().map_or((0.0, 0.0), |p| p.size());
        let mut caption = entry.caption.clone();
        let tone = entry.tone;
        let dark = image::dark_checker(&view, tone);
        let layout = image::single_layout(width, height);
        unsafe {
            self.d2d
                .PushAxisAlignedClip(&d2d_rect(layout.canvas), D2D1_ANTIALIAS_MODE_ALIASED);
        }
        match content.mobile {
            // モバイルで見る SVG: 元の絵と、その部品での絵を並べる（GTK4 版の `imagediff::compare`）。
            Some(mobile) => {
                let mobile_key = (content.key, usize::MAX - 1);
                if !self.images.contains_key(&mobile_key) {
                    let pixels = mobile
                        .preview
                        .svg
                        .as_deref()
                        .and_then(|svg| self.rasterize_svg(svg).ok());
                    let bitmap = pixels.as_ref().and_then(|p| self.bitmap(p).ok());
                    let tone = image::tone(&[pixels.as_ref()]);
                    self.images.insert(
                        mobile_key,
                        ImageEntry {
                            before: None,
                            after: pixels,
                            before_bitmap: None,
                            after_bitmap: bitmap,
                            caption: String::new(),
                            difference: None,
                            tone,
                        },
                    );
                }
                let rendered = &self.images[&mobile_key];
                let (rendered_bitmap, rendered_size) = (
                    rendered.after_bitmap.clone(),
                    rendered.after.as_ref().map_or((0.0, 0.0), |p| p.size()),
                );
                caption = format!("{caption}    {}", mobile.preview.caption);
                let titles = [amonhen_model::tr!("Original"), mobile.preview.title.clone()];
                for (side, (bitmap, size)) in [
                    (Some(bitmap.clone()), size),
                    (rendered_bitmap, rendered_size),
                ]
                .into_iter()
                .enumerate()
                {
                    let area = image::two_up_area(layout.canvas, side);
                    self.draw_small(
                        &titles[side],
                        &format,
                        area.x,
                        area.y - 20.0,
                        area.w,
                        &secondary(),
                        false,
                    )?;
                    // その部品が読めない SVG は空けておく（理由は下の一覧の上に）。
                    if let Some(bitmap) = bitmap {
                        unsafe {
                            self.d2d
                                .PushAxisAlignedClip(&d2d_rect(area), D2D1_ANTIALIAS_MODE_ALIASED);
                        }
                        let rect = image::fit(size, area, &view);
                        self.checker(rect, dark);
                        self.draw_bitmap(&bitmap, rect, 1.0);
                        unsafe { self.d2d.PopAxisAlignedClip() };
                    }
                }
            }
            None => {
                let rect = image::fit(size, layout.canvas, &view);
                self.checker(rect, dark);
                self.draw_bitmap(&bitmap, rect, 1.0);
            }
        }
        unsafe { self.d2d.PopAxisAlignedClip() };
        self.draw_centered(
            &image::zoom_text(&view),
            &format,
            layout.zoom_label,
            &secondary(),
        )?;
        for (r, glyph) in [
            (layout.zoom_out, "zoom-out"),
            (layout.zoom_in, "zoom-in"),
            (layout.zoom_fit, "zoom-fit"),
        ] {
            unsafe {
                self.brush.SetColor(&diff_file());
                self.d2d.FillRectangle(&d2d_rect(r), &self.brush);
            }
            self.draw_icon_centered(glyph, r, 16.0, &foreground())?;
        }
        self.draw_checker_button(layout.checker, tone, dark)?;
        self.draw_small(
            &caption,
            &self.meta_format.clone(),
            layout.caption.x,
            layout.caption.y,
            layout.caption.w,
            &secondary(),
            false,
        )
    }

    /// 画像の前と後を読んだもの（なければ読む）。
    fn image_entry(
        &mut self,
        commit: &crate::data::DiffContent,
        file: usize,
    ) -> Option<&mut ImageEntry> {
        let key = (commit.key, file);
        if !self.images.contains_key(&key) {
            // 別の差分の絵は捨てる。
            self.images.retain(|k, _| k.0 == commit.key);
            let (path, diff) = commit.stacked?.files.get(file)?;
            let side = |blob: &[u8]| (!blob.is_empty()).then(|| blob.to_vec());
            let (old, new) = (side(&diff.old), side(&diff.new));
            let svg = crate::imagediff::is_svg(path);
            let decode = |b: &Option<Vec<u8>>| {
                let bytes = b.as_deref()?;
                if svg {
                    self.rasterize_svg(bytes).ok()
                } else {
                    crate::imagediff::decode(bytes)
                }
            };
            let (before, after) = (decode(&old), decode(&new));
            let caption = crate::imagediff::caption(
                crate::imagediff::describe(old.as_deref(), before.as_ref()),
                crate::imagediff::describe(new.as_deref(), after.as_ref()),
            );
            let before_bitmap = before.as_ref().and_then(|p| self.bitmap(p).ok());
            let after_bitmap = after.as_ref().and_then(|p| self.bitmap(p).ok());
            let tone = crate::imagediff::tone(&[before.as_ref(), after.as_ref()]);
            self.images.insert(
                key,
                ImageEntry {
                    before,
                    after,
                    before_bitmap,
                    after_bitmap,
                    caption,
                    difference: None,
                    tone,
                },
            );
        }
        self.images.get_mut(&key)
    }

    /// SVG を Direct2D で絵にする（その絵の大きさで。別の描き先で描いて、ピクセルを読み出す）。
    fn rasterize_svg(&self, bytes: &[u8]) -> Result<crate::imagediff::Pixels> {
        use windows::Win32::UI::Shell::SHCreateMemStream;
        // 拡げても粗くならないよう、絵の大きさの 4 倍（2048 まで）の細かさで描く。
        let (w, h) = crate::imagediff::svg_size(bytes);
        let scale = (2048 / w.max(h)).clamp(1, 4);
        let (width, height) = (w * scale, h * scale);
        let size = D2D_SIZE_U { width, height };
        let props = |options| D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: options,
            ..Default::default()
        };
        unsafe {
            let device = self.d2d.GetDevice()?;
            let dc: ID2D1DeviceContext5 = device
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?
                .cast()?;
            let stream = SHCreateMemStream(Some(bytes)).ok_or_else(|| {
                windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY)
            })?;
            // 文書は絵の大きさで作り（`width`・`height` があればそれで描かれる）、拡大の変換で細かく描く。
            let document = dc.CreateSvgDocument(
                &stream,
                D2D_SIZE_F {
                    width: w as f32,
                    height: h as f32,
                },
            )?;
            let target = dc.CreateBitmap(size, None, 0, &props(D2D1_BITMAP_OPTIONS_TARGET))?;
            dc.SetTarget(&target);
            dc.BeginDraw();
            dc.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
            let s = scale as f32;
            dc.SetTransform(&windows_numerics::Matrix3x2 {
                M11: s,
                M12: 0.0,
                M21: 0.0,
                M22: s,
                M31: 0.0,
                M32: 0.0,
            });
            dc.DrawSvgDocument(&document);
            dc.EndDraw(None, None)?;
            let cpu = dc.CreateBitmap(
                size,
                None,
                0,
                &props(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW),
            )?;
            cpu.CopyFromBitmap(None, &target, None)?;
            let mapped = cpu.Map(D2D1_MAP_OPTIONS_READ)?;
            let row = width as usize * 4;
            let mut data = vec![0u8; row * height as usize];
            for y in 0..height as usize {
                let from =
                    std::slice::from_raw_parts(mapped.bits.add(y * mapped.pitch as usize), row);
                data[y * row..(y + 1) * row].copy_from_slice(from);
            }
            cpu.Unmap()?;
            Ok(crate::imagediff::Pixels {
                width,
                height,
                data,
                scale,
            })
        }
    }

    /// 読んだ絵から、描ける絵を作る。
    fn bitmap(&self, pixels: &crate::imagediff::Pixels) -> Result<ID2D1Bitmap1> {
        unsafe {
            self.d2d.CreateBitmap(
                D2D_SIZE_U {
                    width: pixels.width,
                    height: pixels.height,
                },
                Some(pixels.data.as_ptr().cast()),
                pixels.width * 4,
                &D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    ..Default::default()
                },
            )
        }
    }

    /// 画像の比べる欄（macOS 版の `ImageDiffPanel`、GTK4 版の `imagediff::panel`）: 上の帯に比べ方・スワイプと
    /// オニオンスキンの位置・拡大縮小、その下に絵、下に大きさの説明（違うピクセルなら、違う数）。
    fn draw_image_panel(
        &mut self,
        commit: &crate::data::DiffContent,
        file: usize,
        top: f32,
        width: f32,
    ) -> Result<()> {
        use crate::imagediff::{self as image, Mode};
        let height = image::PANEL_LINES as f32 * DIFF_ROW_HEIGHT;
        let mode = image::mode();
        let view = commit
            .image_views
            .and_then(|v| v.get(&file).copied())
            .unwrap_or_default();
        let svg = commit
            .stacked
            .and_then(|s| s.files.get(file))
            .is_some_and(|(path, _)| image::is_svg(path));
        let layout = image::layout(top, width, mode, svg);
        let accent = rgba(0.21, 0.52, 0.89, 1.0);
        let fill = |r: &Self, rect: image::Rect, color: D2D1_COLOR_F| unsafe {
            r.brush.SetColor(&color);
            r.d2d.FillRectangle(&d2d_rect(rect), &r.brush);
        };
        // 地と、下の仕切りの線。
        fill(
            self,
            image::Rect {
                x: 0.0,
                y: top,
                w: width,
                h: height,
            },
            background(),
        );
        fill(
            self,
            image::Rect {
                x: 0.0,
                y: top + height - 1.0,
                w: width,
                h: 1.0,
            },
            diff_file(),
        );
        // 比べ方（つないだボタン。選んでいるものはアクセントの色）。
        let format = self.file_format.clone();
        for (i, m) in Mode::ALL.into_iter().enumerate() {
            let r = layout.modes[i];
            let on = m == mode;
            fill(self, r, if on { accent } else { diff_file() });
            let color = if on {
                rgba(1.0, 1.0, 1.0, 1.0)
            } else {
                foreground()
            };
            self.draw_centered(&m.title(), &format, r, &color)?;
        }
        // SVG の「テキストで見る」（Segoe Fluent Icons の Document）。
        if let Some(r) = layout.text {
            fill(self, r, diff_file());
            self.draw_icon_centered("files", r, 16.0, &foreground())?;
        }
        // スワイプ・オニオンスキンの位置のスライダー。
        if let Some(slider) = layout.slider {
            let mid = slider.y + slider.h / 2.0;
            fill(
                self,
                image::Rect {
                    x: slider.x,
                    y: mid - 1.5,
                    w: slider.w,
                    h: 3.0,
                },
                diff_file(),
            );
            fill(
                self,
                image::Rect {
                    x: slider.x,
                    y: mid - 1.5,
                    w: slider.w * view.position,
                    h: 3.0,
                },
                accent,
            );
            self.fill_circle(slider.x + slider.w * view.position, mid, 7.0, &accent);
        }
        // 拡大縮小: 倍率（押すと 100% と合わせるを切り替える）・縮める・広げる・合わせる（Segoe Fluent Icons）。
        self.draw_centered(
            &image::zoom_text(&view),
            &format,
            layout.zoom_label,
            &secondary(),
        )?;
        for (r, glyph) in [
            (layout.zoom_out, "zoom-out"),
            (layout.zoom_in, "zoom-in"),
            (layout.zoom_fit, "zoom-fit"),
        ] {
            fill(self, r, diff_file());
            self.draw_icon_centered(glyph, r, 16.0, &foreground())?;
        }
        let Some(entry) = self.image_entry(commit, file) else {
            return Ok(());
        };
        let size = |p: &Option<image::Pixels>| p.as_ref().map_or((0.0, 0.0), |p| p.size());
        let (before_size, after_size) = (size(&entry.before), size(&entry.after));
        let common = (
            before_size.0.max(after_size.0),
            before_size.1.max(after_size.1),
        );
        let (before, after) = (entry.before_bitmap.clone(), entry.after_bitmap.clone());
        let mut caption = entry.caption.clone();
        let tone = entry.tone;
        let dark = image::dark_checker(&view, tone);
        if mode == Mode::Difference {
            if entry.difference.is_none() {
                let (pixels, changed, total) =
                    image::difference(entry.before.as_ref(), entry.after.as_ref());
                let bitmap = pixels.as_ref().and_then(|p| self.bitmap(p).ok());
                if let Some(entry) = self.images.get_mut(&(commit.key, file)) {
                    entry.difference = Some((bitmap, changed, total));
                }
            }
            if let Some(Some((_, changed, total))) = self
                .images
                .get(&(commit.key, file))
                .map(|e| e.difference.as_ref())
            {
                caption = image::difference_summary(*changed, *total);
            }
        }
        self.draw_checker_button(layout.checker, tone, dark)?;
        let canvas = layout.canvas;
        unsafe {
            self.d2d
                .PushAxisAlignedClip(&d2d_rect(canvas), D2D1_ANTIALIAS_MODE_ALIASED);
        }
        let titles = (amonhen_model::tr!("Before"), amonhen_model::tr!("After"));
        let drawn = (|| -> Result<()> {
            match mode {
                Mode::TwoUp => {
                    let red = rgba(0.88, 0.11, 0.14, 0.8);
                    let green = rgba(0.18, 0.76, 0.49, 0.8);
                    for (side, (bitmap, size, title, color)) in [
                        (&before, before_size, &titles.0, red),
                        (&after, after_size, &titles.1, green),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let area = image::two_up_area(canvas, side);
                        self.draw_small(
                            title,
                            &format,
                            area.x,
                            area.y - 20.0,
                            area.w,
                            &secondary(),
                            false,
                        )?;
                        match bitmap {
                            Some(bitmap) => {
                                unsafe {
                                    self.d2d.PushAxisAlignedClip(
                                        &d2d_rect(area),
                                        D2D1_ANTIALIAS_MODE_ALIASED,
                                    );
                                }
                                let rect = image::fit(size, area, &view);
                                self.checker(rect, dark);
                                self.draw_bitmap(bitmap, rect, 1.0);
                                unsafe {
                                    self.brush.SetColor(&color);
                                    self.d2d.DrawRectangle(
                                        &d2d_rect(image::Rect {
                                            x: rect.x - 1.5,
                                            y: rect.y - 1.5,
                                            w: rect.w + 3.0,
                                            h: rect.h + 3.0,
                                        }),
                                        &self.brush,
                                        2.0,
                                        None::<&ID2D1StrokeStyle>,
                                    );
                                    self.d2d.PopAxisAlignedClip();
                                }
                            }
                            None => {
                                let dash = image::Rect {
                                    x: area.x + 30.0,
                                    y: area.y + 20.0,
                                    w: (area.w - 60.0).max(0.0),
                                    h: (area.h - 40.0).max(0.0),
                                };
                                self.dashed(dash, &secondary())?;
                                self.draw_centered(
                                    &amonhen_model::tr!("None"),
                                    &format,
                                    dash,
                                    &secondary(),
                                )?;
                            }
                        }
                    }
                }
                Mode::Swipe | Mode::Onion => {
                    let rect = image::fit(common, image::overlay_area(canvas), &view);
                    self.checker(rect, dark);
                    let before_rect = image::fitted(common, before_size, rect);
                    let after_rect = image::fitted(common, after_size, rect);
                    if mode == Mode::Onion {
                        if let Some(b) = &before {
                            self.draw_bitmap(b, before_rect, 1.0);
                        }
                        if let Some(a) = &after {
                            self.draw_bitmap(a, after_rect, view.position);
                        }
                    } else {
                        if let Some(a) = &after {
                            self.draw_bitmap(a, after_rect, 1.0);
                        }
                        let split = rect.x + rect.w * view.position;
                        let left = image::Rect {
                            x: rect.x,
                            y: rect.y,
                            w: (split - rect.x).max(0.0),
                            h: rect.h,
                        };
                        unsafe {
                            self.d2d
                                .PushAxisAlignedClip(&d2d_rect(left), D2D1_ANTIALIAS_MODE_ALIASED);
                        }
                        self.checker(rect, dark);
                        if let Some(b) = &before {
                            self.draw_bitmap(b, before_rect, 1.0);
                        }
                        unsafe { self.d2d.PopAxisAlignedClip() };
                        fill(
                            self,
                            image::Rect {
                                x: split - 1.0,
                                y: rect.y - 4.0,
                                w: 2.0,
                                h: rect.h + 8.0,
                            },
                            accent,
                        );
                        self.fill_circle(split, rect.y + rect.h / 2.0, 7.0, &accent);
                    }
                    self.draw_small(
                        &titles.0,
                        &format,
                        rect.x - 56.0,
                        rect.y - 1.0,
                        52.0,
                        &secondary(),
                        true,
                    )?;
                    self.draw_small(
                        &titles.1,
                        &format,
                        rect.x + rect.w + 8.0,
                        rect.y - 1.0,
                        52.0,
                        &secondary(),
                        false,
                    )?;
                }
                Mode::Difference => {
                    let rect = image::fit(common, image::overlay_area(canvas), &view);
                    fill(self, rect, rgba(0.0, 0.0, 0.0, 1.0));
                    let bitmap = self
                        .images
                        .get(&(commit.key, file))
                        .and_then(|e| e.difference.as_ref())
                        .and_then(|d| d.0.clone());
                    if let Some(bitmap) = bitmap {
                        self.draw_bitmap(&bitmap, rect, 1.0);
                    }
                }
            }
            Ok(())
        })();
        unsafe { self.d2d.PopAxisAlignedClip() };
        drawn?;
        self.draw_small(
            &caption,
            &self.meta_format.clone(),
            layout.caption.x,
            layout.caption.y,
            layout.caption.w,
            &secondary(),
            false,
        )
    }

    /// 絵を `rect` に描く（2 倍以上に拡げるならぼかさない）。
    fn draw_bitmap(&self, bitmap: &ID2D1Bitmap1, rect: crate::imagediff::Rect, opacity: f32) {
        // 絵のピクセルの 2 倍以上に拡げるならぼかさない（SVG は細かく描いてあるので、そのピクセルで比べる）。
        let pixels = unsafe { bitmap.GetPixelSize() }.width as f32;
        let mode = if rect.w >= pixels * 2.0 {
            D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR
        } else {
            D2D1_INTERPOLATION_MODE_LINEAR
        };
        let dest = d2d_rect(rect);
        unsafe {
            self.d2d
                .DrawBitmap(bitmap, Some(&dest as *const _), opacity, mode, None, None);
        }
    }

    /// 透けたところを見せる市松模様。
    /// 透けた絵の下の市松模様（`dark` なら暗い 2 色。色はコアの `checker` の、macOS 版・GTK4 版と同じもの）。
    fn checker(&self, rect: crate::imagediff::Rect, dark: bool) {
        self.checker_squares(rect, dark, 8.0);
    }

    fn checker_squares(&self, rect: crate::imagediff::Rect, dark: bool, s: f32) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        use amonhen_core::checker::{DARK, LIGHT};
        let (a, b) = if dark { DARK } else { LIGHT };
        let (a, b) = (a as f32, b as f32);
        unsafe {
            self.brush.SetColor(&rgba(a, a, a, 1.0));
            self.d2d.FillRectangle(&d2d_rect(rect), &self.brush);
            self.brush.SetColor(&rgba(b, b, b, 1.0));
        }
        let (right, bottom) = (rect.x + rect.w, rect.y + rect.h);
        let mut y = rect.y;
        let mut row = 0;
        while y < bottom {
            let mut x = rect.x + if row % 2 == 0 { 0.0 } else { s };
            while x < right {
                unsafe {
                    self.d2d.FillRectangle(
                        &D2D_RECT_F {
                            left: x,
                            top: y,
                            right: (x + s).min(right),
                            bottom: (y + s).min(bottom),
                        },
                        &self.brush,
                    );
                }
                x += 2.0 * s;
            }
            y += s;
            row += 1;
        }
    }

    /// 帯の市松模様の明暗の切り替え（透けた画素があるときだけ。暗くしている間は押した形）。
    fn draw_checker_button(
        &self,
        r: crate::imagediff::Rect,
        tone: amonhen_core::checker::Tone,
        dark: bool,
    ) -> Result<()> {
        if !tone.transparent() {
            return Ok(());
        }
        unsafe {
            self.brush.SetColor(&if dark {
                rgba(0.21, 0.52, 0.89, 0.35)
            } else {
                diff_file()
            });
            self.d2d.FillRectangle(&d2d_rect(r), &self.brush);
        }
        // 小さな市松模様（3 × 3）を、今の明暗で。
        let icon = crate::imagediff::Rect {
            x: r.x + (r.w - 12.0) / 2.0,
            y: r.y + (r.h - 12.0) / 2.0,
            w: 12.0,
            h: 12.0,
        };
        self.checker_squares(icon, dark, 4.0);
        unsafe {
            self.brush.SetColor(&with_alpha(foreground(), 0.5));
            self.d2d
                .DrawRectangle(&d2d_rect(icon), &self.brush, 1.0, None::<&ID2D1StrokeStyle>);
        }
        Ok(())
    }

    /// 市松模様の明暗の既定と、透けた画素があるか（読んだ絵のもの。読んでいなければ None）。
    pub fn image_tone(&self, key: u64, file: usize) -> Option<amonhen_core::checker::Tone> {
        self.images.get(&(key, file)).map(|e| e.tone)
    }

    /// 点線の枠。
    fn dashed(&self, rect: crate::imagediff::Rect, color: &D2D1_COLOR_F) -> Result<()> {
        unsafe {
            let base: &ID2D1Factory = &self.d2d_factory;
            let style = base.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    dashStyle: D2D1_DASH_STYLE_DASH,
                    ..Default::default()
                },
                None,
            )?;
            self.brush.SetColor(color);
            self.d2d
                .DrawRectangle(&d2d_rect(rect), &self.brush, 1.0, &style);
        }
        Ok(())
    }

    fn fill_circle(&self, x: f32, y: f32, r: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush.SetColor(color);
            self.d2d.FillEllipse(
                &D2D1_ELLIPSE {
                    point: point(x, y),
                    radiusX: r,
                    radiusY: r,
                },
                &self.brush,
            );
        }
    }

    /// 短い文字を `rect` の真ん中に描く。
    fn draw_centered(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        rect: crate::imagediff::Rect,
        color: &D2D1_COLOR_F,
    ) -> Result<()> {
        let text: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = self
                .dwrite
                .CreateTextLayout(&text, format, rect.w, rect.h)?;
            layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            layout.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            self.brush.SetColor(color);
            self.d2d.DrawTextLayout(
                point(rect.x, rect.y),
                &layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
        Ok(())
    }

    /// どの FE でも同じ形のアイコン（`crates/icons`）`name` を、左上 (`x`, `y`) から一辺 `size` に、Segoe Fluent
    /// Icons と同じ 1px の線で描く（WinUI の部分の `crate::icons` と同じ）。縦・横の線は画素に合わせる。
    fn draw_icon(&self, name: &str, x: f32, y: f32, size: f32, color: &D2D1_COLOR_F) -> Result<()> {
        let Some(icon) = amonhen_icons::get(name) else {
            return Ok(());
        };
        let stroke = crate::icons::STROKE;
        unsafe {
            let (mut dpi_x, mut dpi_y) = (96.0f32, 96.0f32);
            self.d2d.GetDpi(&mut dpi_x, &mut dpi_y);
            let scale = (dpi_x / 96.0).max(1.0);
            // 置く位置も画素の角に合わせる（中の線を画素に合わせたのが崩れないように）。
            let (x, y) = ((x * scale).round() / scale, (y * scale).round() / scale);
            self.brush.SetColor(color);
            let base: &ID2D1Factory = &self.d2d_factory;
            for s in icon.strokes(size, scale, stroke) {
                let geometry = self.d2d_factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                let p = |c: &[f32], k: usize| point(x + c[k], y + c[k + 1]);
                let mut open = false;
                let mut i = 0;
                while let Some(op) = s
                    .segments
                    .get(i)
                    .and_then(|&c| amonhen_icons::Op::from_code(c))
                {
                    let c = &s.segments[i + 1..i + 1 + op.arity()];
                    match op {
                        amonhen_icons::Op::Move => {
                            if open {
                                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            }
                            sink.BeginFigure(p(c, 0), D2D1_FIGURE_BEGIN_HOLLOW);
                            open = true;
                        }
                        amonhen_icons::Op::Line => sink.AddLine(p(c, 0)),
                        amonhen_icons::Op::Cubic => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: p(c, 0),
                            point2: p(c, 2),
                            point3: p(c, 4),
                        }),
                        amonhen_icons::Op::Close => {
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            open = false;
                        }
                    }
                    i += 1 + op.arity();
                }
                if open {
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                }
                sink.Close()?;
                // 破線は線の太さを 1 とした長さ。
                let dashes: Vec<f32> = s.dash.iter().map(|d| d / stroke).collect();
                let style = base.CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        startCap: D2D1_CAP_STYLE_ROUND,
                        endCap: D2D1_CAP_STYLE_ROUND,
                        dashCap: D2D1_CAP_STYLE_ROUND,
                        lineJoin: D2D1_LINE_JOIN_ROUND,
                        miterLimit: 10.0,
                        dashStyle: if dashes.is_empty() {
                            D2D1_DASH_STYLE_SOLID
                        } else {
                            D2D1_DASH_STYLE_CUSTOM
                        },
                        dashOffset: 0.0,
                    },
                    (!dashes.is_empty()).then_some(dashes.as_slice()),
                )?;
                self.d2d
                    .DrawGeometry(&geometry, &self.brush, stroke, &style);
            }
        }
        Ok(())
    }

    /// `draw_icon` を `rect` の真ん中に。
    fn draw_icon_centered(
        &self,
        name: &str,
        rect: crate::imagediff::Rect,
        size: f32,
        color: &D2D1_COLOR_F,
    ) -> Result<()> {
        self.draw_icon(
            name,
            rect.x + (rect.w - size) / 2.0,
            rect.y + (rect.h - size) / 2.0,
            size,
            color,
        )
    }

    /// 短い文字を `x` から幅 `width` に描く（`trailing` なら右に寄せる）。
    #[allow(clippy::too_many_arguments)]
    fn draw_small(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        x: f32,
        top: f32,
        width: f32,
        color: &D2D1_COLOR_F,
        trailing: bool,
    ) -> Result<()> {
        let text: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = self
                .dwrite
                .CreateTextLayout(&text, format, width, DIFF_ROW_HEIGHT)?;
            if trailing {
                layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
            }
            self.brush.SetColor(color);
            self.d2d.DrawTextLayout(
                point(x, top + 1.0),
                &layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        Ok(())
    }

    /// 文字の形と、その幅。
    fn measured(&self, text: &str, format: &IDWriteTextFormat) -> Result<(IDWriteTextLayout, f32)> {
        let text: Vec<u16> = text.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&text, format, 10_000.0, DIFF_ROW_HEIGHT)?
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics)? };
        Ok((layout, metrics.widthIncludingTrailingWhitespace.ceil()))
    }

    /// 差分の `i` 行目の文字の配置を作って取っておく（あれば何もしない）。
    fn diff_layout(
        &mut self,
        view: &amonhen_core::diff::DiffView,
        syntax: &std::collections::HashMap<u32, Vec<(u32, u32, u8)>>,
        i: u32,
        title: Option<&str>,
    ) -> Result<()> {
        use amonhen_layout::DiffRow;
        let row = &view.rows[i as usize];
        let file = row.kind == DiffRow::FILE;
        if !self.diff_rows.contains_key(&i) {
            // ファイルの見出しはパスだけ（木なら名前だけ。▸・印・行の数は `draw_file_header` が別に描く）。
            let mut text = Vec::with_capacity(128);
            match title {
                Some(title) => text.extend(title.encode_utf16()),
                None => crate::data::push_utf16(
                    view.strings.bytes(row.text),
                    row.text.is_ascii(),
                    &mut text,
                ),
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
            if let Some(spans) = syntax.get(&i) {
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
        Ok(())
    }

    /// 構文の種類の文字の色（GTK4 版の System のテーマの明るい外観と同じ。ない種類は本文の色）。
    fn syntax_brush(&mut self, kind: u8) -> Result<Option<ID2D1SolidColorBrush>> {
        if let Some(brush) = self.syntax_brushes.get(&kind) {
            return Ok(Some(brush.clone()));
        }
        let Some((r, g, b)) = syntax_rgb(kind) else {
            return Ok(None);
        };
        let brush = unsafe { self.d2d.CreateSolidColorBrush(&rgba(r, g, b, 1.0), None)? };
        self.syntax_brushes.insert(kind, brush.clone());
        Ok(Some(brush))
    }

    /// 隙間の行の字の形と、その幅・高さ（小さめの字。`bold` ならボタンの太字）。
    fn small_text(&self, text: &str, bold: bool) -> Result<(IDWriteTextLayout, f32, f32)> {
        let format = if bold {
            &self.gap_bold_format
        } else {
            &self.meta_format
        };
        let text: Vec<u16> = text.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&text, format, 10_000.0, DIFF_ROW_HEIGHT)?
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics)? };
        Ok((
            layout,
            metrics.widthIncludingTrailingWhitespace.ceil(),
            metrics.height,
        ))
    }

    /// 隙間の行の置き場所（差分の欄の座標。GTK4 版の `gap_layout`）。左に関数名、中央に札、札の右に開く・畳む
    /// （マウスが乗っているとき）、右端にその下の hunk の操作。
    pub fn gap_layout(
        &self,
        info: crate::data::GapInfo,
        top: f32,
        width: f32,
        text_x: f32,
    ) -> Result<GapLayout> {
        let height = DIFF_ROW_HEIGHT - 2.0;
        let y = top + (DIFF_ROW_HEIGHT - height) / 2.0;
        let rect = |x: f32, w: f32| D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + height,
        };
        let mut buttons = Vec::new();
        // 右端の hunk の操作（右から並べる）。
        let mut right = width - 10.0;
        for (action, title) in info.hunk_actions.iter().rev() {
            let text = self.small_text(title, true)?;
            let w = text.1 + 12.0;
            buttons.push((*action, rect(right - w, w), text));
            right -= w + 2.0;
        }
        // 中央の札（自分の幅だけで中央に置く。ボタンが出ても動かない）と、その右の開く・畳む。
        let mut pill = None;
        if !info.pill.is_empty() {
            let text = self.small_text(&info.pill, false)?;
            let w = text.1 + 20.0;
            let mut x = (text_x + (width - text_x - w) / 2.0).max(text_x + 8.0);
            pill = Some((rect(x, w), text));
            x += w + 6.0;
            for (action, title) in &info.gap_actions {
                let text = self.small_text(title, true)?;
                let cw = text.1 + 16.0;
                if x + cw >= right - 8.0 {
                    break;
                }
                buttons.push((*action, rect(x, cw), text));
                x += cw + 6.0;
            }
        }
        // 左の関数名（札にかかるなら出さない）。
        let mut function = None;
        if !info.function.is_empty() {
            let text = self.small_text(&format!("ƒ {}", info.function), false)?;
            let end = text_x + 8.0 + text.1;
            let limit = pill.as_ref().map_or(right, |(r, _)| r.left);
            if end + 12.0 < limit {
                function = Some((text_x + 8.0, text));
            }
        }
        Ok(GapLayout {
            pill,
            pill_selected: info.selected,
            buttons,
            function,
            target: info.target,
        })
    }

    /// 隙間の行: 破線と、札・ボタン・関数名（GTK4 版の `draw_gap_row`）。
    fn draw_gap_row(
        &self,
        folds: crate::data::Folds,
        index: u32,
        top: f32,
        width: f32,
        text_x: f32,
    ) -> Result<()> {
        use crate::data::GapAction;
        let working = folds.working;
        let layout = self.gap_layout(working.gap_info(index, folds.bar), top, width, text_x)?;
        let hovered = working.pointer.hovered_gap;
        let red = rgba(0.88, 0.27, 0.25, 1.0);
        let faint = |a: f32| with_alpha(foreground(), a);
        let mid = (top + DIFF_ROW_HEIGHT / 2.0).floor();
        let mut holes: Vec<(f32, f32)> = layout
            .buttons
            .iter()
            .map(|(_, r, _)| (r.left - 6.0, r.right + 6.0))
            .collect();
        if let Some((r, _)) = &layout.pill {
            holes.push((r.left - 6.0, r.right + 6.0));
        }
        if let Some((x, text)) = &layout.function {
            holes.push((x - 4.0, x + text.1 + 8.0));
        }
        unsafe {
            self.brush.SetColor(&faint(0.25));
            let mut x = text_x + 4.0;
            while x < width - 4.0 {
                let inside = |v: f32| holes.iter().any(|&(a, b)| v >= a && v <= b);
                if !inside(x) && !inside(x + 4.0) {
                    self.d2d.FillRectangle(
                        &D2D_RECT_F {
                            left: x,
                            top: mid,
                            right: x + 4.0,
                            bottom: mid + 1.0,
                        },
                        &self.brush,
                    );
                }
                x += 8.0;
            }
        }
        let rounded = |rect: &D2D_RECT_F, radius: f32, color: &D2D1_COLOR_F| unsafe {
            self.brush.SetColor(color);
            self.d2d.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: *rect,
                    radiusX: radius,
                    radiusY: radius,
                },
                &self.brush,
            );
        };
        let put = |text: &(IDWriteTextLayout, f32, f32),
                   rect: &D2D_RECT_F,
                   color: &D2D1_COLOR_F| unsafe {
            self.brush.SetColor(color);
            self.d2d.DrawTextLayout(
                point(
                    rect.left + (rect.right - rect.left - text.1) / 2.0,
                    rect.top + (rect.bottom - rect.top - text.2) / 2.0,
                ),
                &text.0,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        };
        if let Some((rect, text)) = &layout.pill {
            let fill = if layout.pill_selected {
                with_alpha(ACCENT, 0.18)
            } else {
                faint(if hovered == Some((index, None)) {
                    0.22
                } else {
                    0.12
                })
            };
            rounded(rect, (rect.bottom - rect.top) / 2.0, &fill);
            put(
                text,
                rect,
                &if layout.pill_selected {
                    ACCENT
                } else {
                    faint(0.7)
                },
            );
        }
        for (action, rect, text) in &layout.buttons {
            let on = hovered == Some((index, Some(*action)));
            let color = if *action == GapAction::Discard {
                red
            } else {
                ACCENT
            };
            if action.is_hunk() {
                // hunk の操作は字だけ（乗ったときだけ薄く塗る）。
                if on {
                    rounded(rect, 4.0, &with_alpha(color, 0.14));
                }
            } else {
                rounded(
                    rect,
                    (rect.bottom - rect.top) / 2.0,
                    &with_alpha(color, if on { 0.30 } else { 0.14 }),
                );
            }
            put(text, rect, &color);
        }
        if let Some((x, text)) = &layout.function {
            unsafe {
                self.brush.SetColor(&faint(0.45));
                self.d2d.DrawTextLayout(
                    point(*x, top + (DIFF_ROW_HEIGHT - text.2) / 2.0),
                    &text.0,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
        Ok(())
    }

    /// 変更の行の左の帯（GTK4 版の `draw_bar`）: All では staged なら塗り、まだなら枠だけ。片側（「変更の帯」の
    /// 形）では、かたまりの印（乗ると太く、行番号の欄も薄く塗る）。`gutter_right` は行番号の欄の右端。
    fn draw_bar(
        &self,
        folds: crate::data::Folds,
        index: u32,
        row: &amonhen_layout::DiffRow,
        top: f32,
        gutter_right: f32,
    ) {
        use amonhen_layout::DiffRow;
        let pointer = &folds.working.pointer;
        let hot = pointer
            .hovered_block
            .is_some_and(|(a, b)| index >= a && index < b)
            || pointer
                .number_drag
                .as_ref()
                .is_some_and(|(_, rows)| rows.contains(&index));
        let x = 8.0;
        let bottom = top + DIFF_ROW_HEIGHT;
        let bar = if hot {
            unsafe {
                self.brush.SetColor(&with_alpha(ACCENT, 0.14));
                self.d2d.FillRectangle(
                    &D2D_RECT_F {
                        left: DIFF_MARK_WIDTH,
                        top,
                        right: gutter_right,
                        bottom,
                    },
                    &self.brush,
                );
            }
            (x - 1.0, x + 5.0)
        } else {
            (x, x + 4.0)
        };
        let fill = |left: f32, right: f32, color: &D2D1_COLOR_F| unsafe {
            self.brush.SetColor(color);
            self.d2d.FillRectangle(
                &D2D_RECT_F {
                    left,
                    top,
                    right,
                    bottom,
                },
                &self.brush,
            );
        };
        if folds.working.diff.combined.is_none() {
            fill(
                bar.0,
                bar.1,
                &with_alpha(ACCENT, if hot { 1.0 } else { 0.55 }),
            );
        } else if row.flags & DiffRow::STAGED != 0 {
            fill(bar.0, bar.1, &ACCENT);
        } else {
            let line = with_alpha(foreground(), 0.5);
            fill(bar.0, bar.0 + 1.0, &line);
            fill(bar.1 - 1.0, bar.1, &line);
        }
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
            self.brush.SetColor(&secondary());
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

    /// CI の印を (cx, cy) を中心に描く: 枠のないチェック・バツ・点（署名の丸と形で見分ける。GTK4 版の `draw_ci`）。
    /// 色は macOS 版と同じく、成功は緑・失敗は赤・途中は橙。
    fn draw_ci(&self, ci: crate::github::Ci, cx: f32, cy: f32) {
        use crate::github::Ci;
        let color = match ci {
            Ci::Passing => rgba(0.20, 0.70, 0.30, 1.0),
            Ci::Failing => rgba(0.90, 0.25, 0.25, 1.0),
            Ci::Pending => rgba(0.95, 0.55, 0.15, 1.0),
            Ci::None => with_alpha(foreground(), 0.55),
        };
        unsafe {
            self.brush.SetColor(&color);
            match ci {
                Ci::Passing => {
                    let (a, b, c) = (
                        point(cx - 3.8, cy + 0.2),
                        point(cx - 1.2, cy + 2.9),
                        point(cx + 4.0, cy - 3.2),
                    );
                    self.d2d.DrawLine(a, b, &self.brush, 1.6, &self.round);
                    self.d2d.DrawLine(b, c, &self.brush, 1.6, &self.round);
                }
                Ci::Failing => {
                    let d = 3.2;
                    self.d2d.DrawLine(
                        point(cx - d, cy - d),
                        point(cx + d, cy + d),
                        &self.brush,
                        1.6,
                        &self.round,
                    );
                    self.d2d.DrawLine(
                        point(cx + d, cy - d),
                        point(cx - d, cy + d),
                        &self.brush,
                        1.6,
                        &self.round,
                    );
                }
                Ci::Pending => self.d2d.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: point(cx, cy),
                        radiusX: 3.0,
                        radiusY: 3.0,
                    },
                    &self.brush,
                ),
                Ci::None => self.d2d.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: point(cx, cy),
                        radiusX: 3.0,
                        radiusY: 3.0,
                    },
                    &self.brush,
                    1.0,
                    None,
                ),
            }
        }
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
            self.brush.SetColor(&if label.head {
                LABEL_HEAD_TEXT
            } else {
                label_text()
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

/// 差分の行から `query` を探す（大文字小文字を区別しない。GTK4 版の `find_all` と同じく、ファイルの見出しも含め
/// すべての行から）。位置は行の文字の形（`draw_diff_rows` が作るもの）の中の UTF-16 の位置。
pub fn find_in_diff(view: &amonhen_core::diff::DiffView, query: &str) -> Vec<(u32, u32, u32)> {
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, row) in view.rows.iter().enumerate() {
        let text = String::from_utf8_lossy(view.strings.bytes(row.text));
        // 文字ごとの（小文字、UTF-16 の位置）。
        let mut chars: Vec<(char, u32)> = Vec::new();
        let mut at = 0;
        for c in text.chars() {
            let lower: Vec<char> = c.to_lowercase().collect();
            let lower = if lower.len() == 1 { lower[0] } else { c };
            chars.push((lower, at));
            at += c.len_utf16() as u32;
        }
        let mut k = 0;
        while k + needle.len() <= chars.len() {
            if chars[k..k + needle.len()]
                .iter()
                .zip(&needle)
                .all(|((c, _), n)| c == n)
            {
                let end = chars
                    .get(k + needle.len())
                    .map_or(at, |&(_, position)| position);
                out.push((i as u32, chars[k].1, end));
                k += needle.len();
            } else {
                k += 1;
            }
        }
    }
    out
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

/// 構文の種類の文字の色（GTK4 版の System のテーマの明るい外観と同じ。0〜1。本文の色のままの種類は None）。
/// 差分と Blame で使う。
pub fn syntax_rgb(kind: u8) -> Option<(f32, f32, f32)> {
    crate::theme::syntax(kind, system_syntax_rgb)
}

/// System のテーマの構文の色（GTK4 版の System の色と同じ。暗い外観では明るく）。
fn system_syntax_rgb(kind: u8) -> Option<(f32, f32, f32)> {
    use amonhen_layout::SyntaxSpan;
    let dark = crate::palette::dark();
    let pick =
        |light: (f32, f32, f32), on_dark: (f32, f32, f32)| Some(if dark { on_dark } else { light });
    match kind {
        SyntaxSpan::KEYWORD | SyntaxSpan::TAG | SyntaxSpan::VARIABLE => {
            pick((0.80, 0.18, 0.45), (1.0, 0.45, 0.65))
        }
        SyntaxSpan::STRING => pick((0.77, 0.16, 0.16), (1.0, 0.45, 0.40)),
        SyntaxSpan::COMMENT => pick((0.45, 0.45, 0.48), (0.60, 0.60, 0.64)),
        SyntaxSpan::NUMBER | SyntaxSpan::CONSTANT => pick((0.50, 0.25, 0.75), (0.78, 0.55, 1.0)),
        SyntaxSpan::FUNCTION => pick((0.10, 0.40, 0.85), (0.40, 0.65, 1.0)),
        SyntaxSpan::TYPE => pick((0.10, 0.55, 0.60), (0.35, 0.80, 0.85)),
        SyntaxSpan::PROPERTY => pick((0.10, 0.50, 0.70), (0.40, 0.80, 0.95)),
        SyntaxSpan::ATTRIBUTE | SyntaxSpan::ESCAPE => pick((0.80, 0.45, 0.05), (1.0, 0.65, 0.30)),
        SyntaxSpan::MODULE => pick((0.55, 0.40, 0.25), (0.80, 0.65, 0.45)),
        _ => None,
    }
}

/// 画像の比べる欄の、読んだ前と後の絵（読めなければ None）と説明、違うピクセルの絵と数（はじめて要るときに作る）。
struct ImageEntry {
    before: Option<crate::imagediff::Pixels>,
    after: Option<crate::imagediff::Pixels>,
    before_bitmap: Option<ID2D1Bitmap1>,
    after_bitmap: Option<ID2D1Bitmap1>,
    caption: String,
    difference: Option<(Option<ID2D1Bitmap1>, usize, usize)>,
    /// 絵の明るさ（市松模様の明暗の既定と、透けた画素があるか）。
    tone: amonhen_core::checker::Tone,
}

fn d2d_rect(r: crate::imagediff::Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    }
}

/// 上に張り付かせるファイルの見出し（見出しの行の番号と、見えている範囲の上端からの位置）。見出しの行が見えている
/// 範囲より上にあるファイルのものを出し、次のファイルの見出しが近づいたら押し上げる（GTK4 版の `sticky`）。
/// `file_rows` は見出しの行の番号（昇順）。
pub fn sticky_header(file_rows: &[u32], scroll_y: f32) -> Option<(usize, f32)> {
    let first = (scroll_y / DIFF_ROW_HEIGHT).floor().max(0.0) as u32;
    let at = file_rows.partition_point(|&r| r <= first).checked_sub(1)?;
    let file = file_rows[at] as usize;
    if file as f32 * DIFF_ROW_HEIGHT >= scroll_y {
        return None;
    }
    let top = file_rows.get(at + 1).map_or(0.0, |&next| {
        (next as f32 * DIFF_ROW_HEIGHT - scroll_y - DIFF_ROW_HEIGHT).min(0.0)
    });
    Some((file, top))
}

/// 等幅の字（Cascadia Mono）の「0」の幅（DIP）。コミットの説明の欄の 72 桁の目安の線の位置に使う（macOS 版・
/// GTK4 版と同じく「0」の幅から決める）。測れなければ None。
pub fn monospace_zero_width(size: f32) -> Option<f32> {
    let measure = || -> Result<f32> {
        unsafe {
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let format = dwrite.CreateTextFormat(
                w!("Cascadia Mono"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("ja-jp"),
            )?;
            // 1 字では丸めが効くので、72 字の幅から求める。
            let text: Vec<u16> = "0".repeat(72).encode_utf16().collect();
            let layout = dwrite.CreateTextLayout(&text, &format, 100_000.0, 100.0)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            Ok(metrics.widthIncludingTrailingWhitespace / 72.0)
        }
    };
    measure().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sticky_header_sticks_and_is_pushed_up() {
        let rh = DIFF_ROW_HEIGHT;
        let files = [0, 10];
        // 一番上で見出しが見えていれば張り付かせない。
        assert_eq!(sticky_header(&files, 0.0), None);
        // 見出しが上に隠れたら、上端に張り付く。
        assert_eq!(sticky_header(&files, 3.0 * rh), Some((0, 0.0)));
        // 次の見出しが 2 行目に来たら、その分だけ押し上げる。
        assert_eq!(sticky_header(&files, 9.5 * rh), Some((0, -0.5 * rh)));
        // 次のファイルに入ったら、そのファイルの見出し。
        assert_eq!(sticky_header(&files, 12.0 * rh), Some((10, 0.0)));
        assert_eq!(sticky_header(&[], 5.0 * rh), None);
    }
}
