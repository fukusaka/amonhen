//! C ABI。History・差分・status の行データのように速さが要る経路はここを通す。
//!
//! - 返したポインタが指すメモリは、対応する `*_release` / `*_free` を呼ぶまで変わらない。
//! - スナップショットは不変なので、読むのにロックは要らない。どのスレッドから読んでもよい。
//! - 失敗した関数は NULL か false を返し、理由は `amonhen_last_error` で取れる。

use amonhen_model::tr;
use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::MaybeUninit;
use std::sync::Arc;

use amonhen_backend_gix::GixBackend;
use amonhen_core::diff::DiffView;
use amonhen_core::{HistorySnapshot, RefsSnapshot, Repository, StatusSnapshot, WorkingDiff, cache};
use amonhen_layout::{DiffRow, Edge, Highlight, Node, StrRef, SyntaxSpan};
use amonhen_model::{FileChange, Oid};
use amonhen_projection::{FrontendProfile, HistoryChunk, StringEncoding};
use amonhen_watch::Watcher;
use std::path::Path;

/// 開いたリポジトリ。
pub struct AmonhenRepo {
    _private: [u8; 0],
}

/// History のスナップショット。
pub struct AmonhenHistory {
    _private: [u8; 0],
}

/// ref の一覧。
pub struct AmonhenRefs {
    _private: [u8; 0],
}

/// リポジトリの監視。
pub struct AmonhenWatch {
    _private: [u8; 0],
}

/// 手元ならファイルの監視、daemon 越しなら知らせを受けているかの印。
enum WatchHandle {
    Local(#[allow(dead_code)] Watcher),
    Remote(Arc<std::sync::atomic::AtomicBool>),
}

/// コミットで変わったファイルの一覧。
pub struct AmonhenChanges {
    _private: [u8; 0],
}

/// 作業ツリーと index の状態。
pub struct AmonhenStatus {
    _private: [u8; 0],
}

/// Working Copy の 1 ファイルの差分。
pub struct AmonhenWorkingDiff {
    _private: [u8; 0],
}

/// ファイル 1 つの差分。
pub struct AmonhenDiff {
    _private: [u8; 0],
}

/// コミットの全ファイルの差分（畳むファイルを変えて表示を作り直す）。
pub struct AmonhenStacked {
    _private: [u8; 0],
}

/// 差分の構文の色付け。
pub struct AmonhenSyntax {
    _private: [u8; 0],
}

/// 行番号の並び（検索の結果）。
pub struct AmonhenRows {
    _private: [u8; 0],
}

/// History の連続した行の、FE 向けの列。
pub struct AmonhenChunk {
    _private: [u8; 0],
}

/// ASCII 以外の文字列の符号化。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmonhenEncoding {
    Utf16 = 0,
    Utf8 = 1,
}

#[repr(C)]
pub struct AmonhenHistoryView {
    pub generation: u64,
    pub row_count: u32,
    pub lane_count: u16,
    /// 行ごとの点。`row_count` 個。
    pub nodes: *const Node,
    pub edges: *const Edge,
    pub edge_count: u32,
    /// 上からレイアウトの済んだ行の数（キャッシュから開いた直後は `row_count` より少なく、下の行は
    /// `Node::PENDING`）。
    pub laid_out_rows: u32,
}

#[repr(C)]
pub struct AmonhenChunkView {
    pub start_row: u32,
    pub row_count: u32,
    /// 選んだ列だけ `row_count` 個。選ばなかった列は NULL。
    pub subjects: *const StrRef,
    pub author_names: *const StrRef,
    pub author_times: *const i64,
    /// 行ごとの署名の種類（0 はなし、1 GPG、2 SSH、3 X.509）。`row_count` 個。検証の結果ではない。
    pub signatures: *const u8,
    /// 文字列置き場の先頭アドレス。`StrRef::buf` で引き、`off` バイト進めた所から読む。
    pub bases: *const *const u8,
    pub base_count: u32,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(e: impl std::fmt::Display) {
    let message = CString::new(e.to_string().replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|last| *last.borrow_mut() = message);
}

/// このスレッドで最後に失敗した関数の理由（UTF-8）。次に失敗するまで有効。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_last_error() -> *const c_char {
    LAST_ERROR.with(|last| last.borrow().as_ptr())
}

/// 引数に NULL を渡されたときに返すもの（失敗を表す値。ビューは何もない空のもの）。
trait Fallback {
    fn fallback() -> Self;
}

impl<T> Fallback for *mut T {
    fn fallback() -> Self {
        std::ptr::null_mut()
    }
}

impl<T> Fallback for *const T {
    fn fallback() -> Self {
        std::ptr::null()
    }
}

impl Fallback for () {
    fn fallback() -> Self {}
}

impl Fallback for bool {
    fn fallback() -> Self {
        false
    }
}

impl Fallback for u32 {
    fn fallback() -> Self {
        0
    }
}

impl Fallback for u64 {
    fn fallback() -> Self {
        0
    }
}

impl Fallback for i32 {
    fn fallback() -> Self {
        -1
    }
}

impl Fallback for i64 {
    fn fallback() -> Self {
        -1
    }
}

/// ビューは数と NULL のポインタだけの `#[repr(C)]` の構造体なので、すべて 0 のものが空のビューになる。
macro_rules! empty_view {
    ($($view:ty),* $(,)?) => {
        $(impl Fallback for $view {
            fn fallback() -> Self {
                // SAFETY: 整数・bool・生のポインタだけでできていて、すべて 0 は有効な値（0 個・NULL）。
                unsafe { std::mem::zeroed() }
            }
        })*
    };
}

empty_view!(
    AmonhenHistoryView,
    AmonhenChunkView,
    AmonhenRowsView,
    AmonhenRefsView,
    AmonhenChangesView,
    AmonhenDiffView,
    AmonhenSyntaxView,
    AmonhenStatusView,
    AmonhenLineEndings,
);

/// ハンドル（`ptr` が指す `ty`）を参照に戻す。NULL なら理由を残し、関数から `Fallback` の値を返す（呼ぶ側の
/// 確かめ忘れで、プロセスごと落ちないように）。NULL でなければ、有効なハンドルであることは呼ぶ側の約束。
macro_rules! handle {
    ($ptr:expr, $ty:ty) => {
        match unsafe { $ptr.cast::<$ty>().as_ref() } {
            Some(value) => value,
            None => {
                set_error(concat!("NULL was passed as ", stringify!($ptr)));
                return Fallback::fallback();
            }
        }
    };
}

macro_rules! repo {
    ($ptr:expr) => {
        handle!($ptr, Repository)
    };
}

macro_rules! history {
    ($ptr:expr) => {
        handle!($ptr, HistorySnapshot)
    };
}

/// コミットの ID（20 バイト）の引数。NULL なら `handle!` と同じく関数から返す。
macro_rules! oid {
    ($ptr:expr) => {
        Oid(*handle!($ptr, [u8; 20]))
    };
}

/// 読む配列（`$count` 個）。個数が 0 なら NULL でもよく、空になる。個数があるのに NULL なら `handle!` と同じく
/// 関数から返す。
macro_rules! array {
    ($ptr:expr, $count:expr) => {{
        let count = $count as usize;
        if count == 0 {
            &[][..]
        } else if $ptr.is_null() {
            set_error(concat!("NULL was passed as ", stringify!($ptr)));
            return Fallback::fallback();
        } else {
            unsafe { std::slice::from_raw_parts($ptr, count) }
        }
    }};
}

/// コミットの ID（20 バイト）が `$count` 個並んだ配列。NULL の扱いは `array!` と同じ。
macro_rules! oids {
    ($ptr:expr, $count:expr) => {
        array!($ptr.cast::<[u8; 20]>(), $count)
            .iter()
            .map(|id| Oid(*id))
            .collect::<Vec<Oid>>()
    };
}

/// 書く先の配列（`capacity` 個）。NULL なら 0 個として扱う（全体の数だけ返す）。
///
/// # Safety
/// NULL でなければ、`capacity` 個の `T` を書ける領域であること。
unsafe fn out_slice<'a, T>(out: *mut T, capacity: usize) -> &'a mut [MaybeUninit<T>] {
    if out.is_null() || capacity == 0 {
        &mut []
    } else {
        unsafe { std::slice::from_raw_parts_mut(out.cast(), capacity) }
    }
}

/// 書く先（1 個）に `value` を書く。NULL なら書かない。
///
/// # Safety
/// NULL でなければ、`T` を書ける領域であること。
unsafe fn put<T>(out: *mut T, value: T) {
    if let Some(out) = unsafe { out.cast::<MaybeUninit<T>>().as_mut() } {
        out.write(value);
    }
}

fn into_history(snapshot: Arc<HistorySnapshot>) -> *const AmonhenHistory {
    Arc::into_raw(snapshot).cast()
}

fn profile(encoding: AmonhenEncoding) -> FrontendProfile {
    FrontendProfile {
        string: match encoding {
            AmonhenEncoding::Utf16 => StringEncoding::Utf16,
            AmonhenEncoding::Utf8 => StringEncoding::Utf8,
        },
        ..FrontendProfile::MACOS
    }
}

fn non_null<T>(slice: &[T]) -> *const T {
    if slice.is_empty() {
        std::ptr::null()
    } else {
        slice.as_ptr()
    }
}

/// 文字列の引数が読めなかった理由。
#[derive(Debug)]
enum StrArgError {
    Null,
    Utf8(std::str::Utf8Error),
}

impl std::fmt::Display for StrArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str("NULL was passed as a string"),
            Self::Utf8(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for StrArgError {}

/// NUL 終端の UTF-8 の引数。NULL なら読まずに失敗にする。
fn str_arg<'a>(s: *const c_char) -> Result<&'a str, StrArgError> {
    if s.is_null() {
        return Err(StrArgError::Null);
    }
    unsafe { CStr::from_ptr(s) }
        .to_str()
        .map_err(StrArgError::Utf8)
}

/// SSH 先ごとの選択（`HostOptions` の JSON）。NULL か空なら何も選んでいない。
///
/// # Safety
/// `text` は NULL か NUL 終端の UTF-8 であること。
unsafe fn host_options_arg(
    text: *const c_char,
) -> amonhen_model::Result<amonhen_remote::ssh::HostOptions> {
    if text.is_null() {
        return Ok(Default::default());
    }
    Ok(amonhen_remote::ssh::HostOptions::from_json(str_arg(text)?))
}

/// 手元のリポジトリの status で、git の fsmonitor（`git fsmonitor--daemon`）を使うか（macOS と Windows）。
/// リポジトリごとに daemon が常駐し、`.git` にソケットを作る。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_set_fsmonitor(enabled: bool) {
    amonhen_backend_cli::set_fsmonitor(enabled);
}

/// 名前を変えたファイルの見せ方（git の `--stat` と同じく、共通の前と後ろをくくり出す: `src/{old.rs → new.rs}`）。
/// `amonhen_string_free` で解放する。引数が読めなければ NULL。
///
/// # Safety
/// `old`・`new` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_rename_label(
    old: *const c_char,
    new: *const c_char,
) -> *mut c_char {
    let (Ok(old), Ok(new)) = (str_arg(old), str_arg(new)) else {
        return std::ptr::null_mut();
    };
    CString::new(amonhen_model::rename_label(old, new))
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// 共通のアイコン（`amonhen-icons`、どの FE でも同じ形）`name`（NUL 終わり）の線の数。ないアイコンなら -1。
///
/// # Safety
/// `name` は NUL 終わりの文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_icon_shape_count(name: *const c_char) -> i32 {
    unsafe { icon(name) }.map_or(-1, |icon| icon.shapes.len() as i32)
}

/// 共通のアイコン `name` の `index` 本目の線。移動・直線・3 次ベジェ・閉じるの列（`amonhen_icons::Shape::segments`:
/// 操作の番号 0・1・2・3 に続けて、移動と直線は x y、ベジェは x1 y1 x2 y2 x y）を返し、その長さを `len` に、破線の
/// 長さ（なければ NULL と 0）を `dash`・`dash_len` に入れる。座標は 16×16 の格子で、線の太さは
/// `amonhen_icon_stroke_width`。返す列はプログラムが終わるまで有効で、解放しない。ないときは NULL。
///
/// # Safety
/// `name` は NUL 終わりの文字列、`len`・`dash`・`dash_len` は書けること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_icon_shape(
    name: *const c_char,
    index: usize,
    len: *mut usize,
    dash: *mut *const f32,
    dash_len: *mut usize,
) -> *const f32 {
    let Some(shape) = unsafe { icon(name) }.and_then(|icon| icon.shapes.get(index)) else {
        return std::ptr::null();
    };
    unsafe {
        *len = shape.segments.len();
        *dash = if shape.dash.is_empty() {
            std::ptr::null()
        } else {
            shape.dash.as_ptr()
        };
        *dash_len = shape.dash.len();
    }
    shape.segments.as_ptr()
}

/// 共通のアイコン `name` を、一辺 `size`（描く単位）の正方形に太さ `stroke` で描く線。縦・横の線は画素
/// （`pixel_scale` は描く単位あたりの画素の数）に合わせてある（`amonhen_icons::Icon::strokes`）。返す列は
/// `[線の数, (列の長さ, 破線の長さの数, 列…, 破線…) × 線の数]`（列は `amonhen_icon_shape` と同じ形、座標は描く単位）
/// で、長さを `len` に入れる。`amonhen_icon_strokes_free` で解放する。ないアイコンなら NULL。
///
/// # Safety
/// `name` は NUL 終わりの文字列、`len` は書けること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_icon_strokes(
    name: *const c_char,
    size: f32,
    pixel_scale: f32,
    stroke: f32,
    len: *mut usize,
) -> *mut f32 {
    let Some(icon) = (unsafe { icon(name) }) else {
        return std::ptr::null_mut();
    };
    let strokes = icon.strokes(size, pixel_scale, stroke);
    let mut out = vec![strokes.len() as f32];
    for s in &strokes {
        out.extend([s.segments.len() as f32, s.dash.len() as f32]);
        out.extend(&s.segments);
        out.extend(&s.dash);
    }
    let out = out.into_boxed_slice();
    unsafe { *len = out.len() };
    Box::into_raw(out) as *mut f32
}

/// `amonhen_icon_strokes` が返した列を解放する。
///
/// # Safety
/// `ptr` と `len` は `amonhen_icon_strokes` が返したものであること（NULL なら何もしない）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_icon_strokes_free(ptr: *mut f32, len: usize) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
    }
}

/// 共通のアイコンの線の太さ（16×16 の格子の単位）。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_icon_stroke_width() -> f32 {
    amonhen_icons::STROKE_WIDTH
}

unsafe fn icon(name: *const c_char) -> Option<&'static amonhen_icons::Icon> {
    if name.is_null() {
        return None;
    }
    let name = unsafe { CStr::from_ptr(name) }.to_str().ok()?;
    amonhen_icons::get(name)
}

/// SVG（`data` の `len` バイト）の、モバイル向けの描画部品で崩れる機能（JSON の配列。要素は
/// `{"targets": ["android"|"ios"|"flutter"|"reactNative", …], "feature", "message", "count"}`）。
/// `amonhen_string_free` で解放する。
///
/// # Safety
/// `data` は `len` バイト読めること（`len` が 0 なら NULL でもよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_svg_lint(data: *const u8, len: usize) -> *mut c_char {
    let bytes = if len == 0 || data.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    let issues: Vec<serde_json::Value> = amonhen_core::svglint::lint(bytes)
        .into_iter()
        .map(|i| {
            serde_json::json!({
                "targets": i.targets.iter().map(|t| t.key()).collect::<Vec<_>>(),
                "feature": i.feature,
                "message": i.message,
                "count": i.count,
            })
        })
        .collect();
    CString::new(serde_json::Value::Array(issues).to_string())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Markdown（`text`、UTF-8）を、FE が組み立てて見せるブロックの並びにした JSON（`amonhen_core::markdown::Block` の
/// 配列）。`github`（`https://github.com/owner/repo`、NULL か空なら使わない）があれば `#123`・`@user`・コミットの ID も
/// リンクにする。`amonhen_string_free` で解放する。
///
/// # Safety
/// `text` は `len` バイト読めること、`github` は NULL か NUL 終わりの文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_markdown_parse(
    text: *const u8,
    len: usize,
    github: *const c_char,
) -> *mut c_char {
    let bytes = if len == 0 || text.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(text, len) }
    };
    let text = String::from_utf8_lossy(bytes);
    let github = str_arg(github).ok().filter(|g| !g.is_empty());
    CString::new(amonhen_core::markdown::parse_json(&text, github))
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Markdown の図の SVG（`MarkdownBlock` の `diagram` の `svg`）に配色を当て、CoreSVG で描ける SVG にする。`scheme` は
/// 設定の「図の配色」の鍵（`syntax`・`pele`・`mermaid`・`mermaid-forest`・`mermaid-import:名前` など）、`dark` は
/// 暗い表示か、`theme` は構文の色のテーマの JSON `{"text": "#rrggbb", "background": "#rrggbb", "kinds": {…}}`
/// （`syntax` のときだけ使う）、`own` は図の中で指定したテーマ（`diagram` の `config` の JSON。設定より先）、
/// `imported` は `scheme` が読み込んだテーマのときの中身（mermaid-cli の設定の JSON）。`theme`・`own`・
/// `imported` は NULL でもよい。読めなければ NULL。`amonhen_string_free` で解放する。
///
/// # Safety
/// `svg`・`scheme` は NUL 終わりの文字列であること、ほかは NULL か NUL 終わりの文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_diagram_colorize(
    svg: *const c_char,
    scheme: *const c_char,
    dark: bool,
    theme: *const c_char,
    own: *const c_char,
    imported: *const c_char,
) -> *mut c_char {
    use amonhen_core::diagram::{
        MermaidConfig, Scheme, ThemeColors, colorize_for_coresvg, colors_for,
    };
    let (Ok(svg), Ok(scheme)) = (str_arg(svg), str_arg(scheme)) else {
        return std::ptr::null_mut();
    };
    let theme: serde_json::Value = str_arg(theme)
        .ok()
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_default();
    let text = |key: &str| theme[key].as_str().unwrap_or_default().to_owned();
    let colors = ThemeColors {
        text: text("text"),
        background: text("background"),
        kinds: theme["kinds"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default(),
    };
    let config = |p: *const c_char| -> Option<MermaidConfig> {
        str_arg(p).ok().and_then(|t| serde_json::from_str(t).ok())
    };
    let (own, imported) = (config(own), config(imported));
    let (vars, backdrop) = colors_for(
        own.as_ref(),
        Scheme::from_key(scheme),
        imported.as_ref(),
        dark,
        &colors,
    );
    colorize_for_coresvg(svg, &vars, backdrop)
        .and_then(|out| CString::new(out).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Markdown の数式の SVG（`MarkdownBlock` の `math` の `svg`、文中の式の `math` の `svg`）に文字の色 `fg`
/// （`#rrggbb`）を当て、CoreSVG で描ける SVG にする。`\color` で決めた色はそのまま。読めなければ NULL。
/// `amonhen_string_free` で解放する。
///
/// # Safety
/// `svg`・`fg` は NUL 終わりの文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_math_colorize(
    svg: *const c_char,
    fg: *const c_char,
) -> *mut c_char {
    let (Ok(svg), Ok(fg)) = (str_arg(svg), str_arg(fg)) else {
        return std::ptr::null_mut();
    };
    amonhen_core::math::colorize_for_coresvg(svg, fg)
        .and_then(|out| CString::new(out).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// 1 つの Markdown のファイルで数式を描く時間の上限を、`seconds` 秒にする（0 は上限なし）。次に読む Markdown から効く
/// （Settings > テーマ > 数式）。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_math_set_time_limit(seconds: u64) {
    amonhen_core::math::set_file_time_limit(
        (seconds > 0).then(|| std::time::Duration::from_secs(seconds)),
    );
}

/// 設定の見本の図の SVG（色は `amonhen_diagram_colorize` で当てる）。描けなければ NULL。`amonhen_string_free` で
/// 解放する。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_diagram_sample() -> *mut c_char {
    amonhen_core::diagram::sample()
        .and_then(|svg| CString::new(svg).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// mermaid-cli の設定ファイル（`{"theme": …, "themeVariables": {…}}`）を読み、アプリが覚える形の JSON にする
/// （`amonhen_diagram_colorize` の `imported` に渡す）。読めなければエラーを置いて NULL。`amonhen_string_free` で
/// 解放する。
///
/// # Safety
/// `text` は NUL 終わりの文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_mermaid_theme_parse(text: *const c_char) -> *mut c_char {
    let Ok(text) = str_arg(text) else {
        return std::ptr::null_mut();
    };
    match amonhen_core::diagram::MermaidConfig::parse(text) {
        Ok(config) => serde_json::to_string(&config)
            .ok()
            .and_then(|s| CString::new(s).ok())
            .map_or(std::ptr::null_mut(), CString::into_raw),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// FE が渡す時間帯（gantt の日付を数える）。`offset` は、UTC の時刻 `utc_ms`（1970 年からのミリ秒）での地方時と
/// UTC の差（ミリ秒、地方時 − UTC）を返す。pele の描画のスレッド（メインではない）から何度も呼ばれる。
struct ForeignZone {
    context: usize,
    offset: extern "C" fn(context: *const c_void, utc_ms: f64) -> f64,
}

impl amonhen_core::diagram::TimeZone for ForeignZone {
    fn offset_ms(&self, utc_ms: f64) -> f64 {
        (self.offset)(self.context as *const c_void, utc_ms)
    }
}

/// `amonhen_markdown_parse` と同じで、図（gantt）の日付を FE の時間帯で数える。`offset` が NULL なら UTC。
/// `context` は `offset` に渡す（この関数から戻るまで使う。どのスレッドから呼んでもよいものにする）。
///
/// # Safety
/// `amonhen_markdown_parse` と同じ。`offset` は、`context` を渡してどのスレッドから呼んでもよいこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_markdown_parse_in_zone(
    text: *const u8,
    len: usize,
    github: *const c_char,
    context: *const c_void,
    offset: Option<extern "C" fn(context: *const c_void, utc_ms: f64) -> f64>,
) -> *mut c_char {
    let bytes = if len == 0 || text.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(text, len) }
    };
    let text = String::from_utf8_lossy(bytes);
    let github = str_arg(github).ok().filter(|g| !g.is_empty());
    let zone = offset.map(|offset| {
        std::sync::Arc::new(ForeignZone {
            context: context as usize,
            offset,
        }) as std::sync::Arc<dyn amonhen_core::diagram::TimeZone>
    });
    CString::new(amonhen_core::markdown::parse_json_in_zone(
        &text, github, zone,
    ))
    .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// 画像の読み手が中身を渡す先（`amonhen_markdown_parse_with_images` の `read` が、読めたときだけ呼ぶ）。
/// `sink_context` は `read` に渡されたもの、`data` は `len` バイト（呼んでいる間だけ読めればよい）。
pub type AmonhenImageSink = extern "C" fn(sink_context: *mut c_void, data: *const u8, len: usize);

/// FE が渡す画像の読み手。`path` はリポジトリの根からのパス（NUL 終わりの UTF-8）。
struct ForeignImages {
    context: usize,
    read: extern "C" fn(
        context: *const c_void,
        path: *const c_char,
        sink: AmonhenImageSink,
        sink_context: *mut c_void,
    ),
}

extern "C" fn image_sink(sink_context: *mut c_void, data: *const u8, len: usize) {
    if sink_context.is_null() || (data.is_null() && len > 0) {
        return;
    }
    // 大きすぎるものは写さない（図が描かない大きさ）
    if len > amonhen_core::diagram::MAX_IMAGE_BYTES {
        return;
    }
    let out = unsafe { &mut *(sink_context as *mut Option<Vec<u8>>) };
    let bytes = if len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    *out = Some(bytes.to_vec());
}

impl ForeignImages {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let path = CString::new(path).ok()?;
        let mut out: Option<Vec<u8>> = None;
        (self.read)(
            self.context as *const c_void,
            path.as_ptr(),
            image_sink,
            &mut out as *mut Option<Vec<u8>> as *mut c_void,
        );
        out
    }
}

/// `amonhen_markdown_parse_in_zone` と同じで、DOT の図の画像（ノードの `image`・表の `<IMG>`）を FE から読む。
/// `path` は Markdown のファイルのパス（リポジトリの根から、NUL 終わり）。図の中の画像は、そのファイルのフォルダ
/// からの相対パスだけを読む（URL・絶対パス・根より上は読まない）。`read` は、リポジトリの根からのパスの中身を、
/// そのファイルと同じコミットから読み、読めれば `sink(sink_context, data, len)` を 1 回呼ぶ（読めなければ呼ばずに
/// 戻る）。`read` は図を描くスレッド（メインではない）から、この関数から戻るまでに呼ばれる。`path` か `read` が
/// NULL なら画像は読まない。
///
/// # Safety
/// `amonhen_markdown_parse_in_zone` と同じ。`path` は NULL か NUL 終わりの文字列、`read` は `image_context` を
/// 渡してどのスレッドから呼んでもよいこと。
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn amonhen_markdown_parse_with_images(
    text: *const u8,
    len: usize,
    github: *const c_char,
    context: *const c_void,
    offset: Option<extern "C" fn(context: *const c_void, utc_ms: f64) -> f64>,
    path: *const c_char,
    image_context: *const c_void,
    read: Option<
        extern "C" fn(
            context: *const c_void,
            path: *const c_char,
            sink: AmonhenImageSink,
            sink_context: *mut c_void,
        ),
    >,
) -> *mut c_char {
    let bytes = if len == 0 || text.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(text, len) }
    };
    let text = String::from_utf8_lossy(bytes);
    let github = str_arg(github).ok().filter(|g| !g.is_empty());
    let zone = offset.map(|offset| {
        std::sync::Arc::new(ForeignZone {
            context: context as usize,
            offset,
        }) as std::sync::Arc<dyn amonhen_core::diagram::TimeZone>
    });
    let images = match (str_arg(path).ok(), read) {
        (Some(from), Some(read)) if !from.is_empty() => {
            let foreign = ForeignImages {
                context: image_context as usize,
                read,
            };
            Some(amonhen_core::diagram::ImageSource {
                from: from.to_owned(),
                read: std::sync::Arc::new(move |p: &str| foreign.read(p)),
            })
        }
        _ => None,
    };
    CString::new(amonhen_core::markdown::parse_json_with_images(
        &text, github, zone, images,
    ))
    .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// この機械の git の版（`2.39.5`）。読めなければ NULL。`amonhen_string_free` で解放する。git を起動する
/// ことがあるので（初めの 1 回だけ）、裏のスレッドで呼ぶ。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_git_version() -> *mut c_char {
    amonhen_backend_cli::git_version()
        .map(|(a, b, c)| {
            CString::new(format!("{a}.{b}.{c}"))
                .unwrap_or_default()
                .into_raw()
        })
        .unwrap_or(std::ptr::null_mut())
}

/// この機械で署名に使える鍵（`format` は `openpgp`・`ssh`・`x509`）。JSON の配列（`id`・`label`・`emails`）。
/// 読めなければ NULL（`amonhen_last_error` に理由）。`amonhen_string_free` で解放する。gpg などを起動するので、
/// 裏のスレッドで呼ぶ。
///
/// # Safety
/// `format` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_signing_keys(format: *const c_char) -> *mut c_char {
    let result = str_arg(format)
        .map_err(|e| e.to_string())
        .and_then(|f| amonhen_backend_cli::signing_keys(f).map_err(|e| e.to_string()))
        .and_then(|keys| serde_json::to_string(&keys).map_err(|e| e.to_string()));
    match result {
        Ok(json) => CString::new(json).unwrap_or_default().into_raw(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// この機械の `path` のフォルダの中身（`~` 可）。JSON（`path`・`home`・`entries`。コアの `listDirectory` と同じ形）。
/// 読めなければ NULL（`amonhen_last_error` に理由）。`amonhen_string_free` で解放する。
///
/// # Safety
/// `path` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_list_directory(path: *const c_char) -> *mut c_char {
    let result = str_arg(path)
        .map_err(|e| e.to_string())
        .and_then(|p| amonhen_backend_cli::list_directory(p).map_err(|e| e.to_string()))
        .and_then(|dir| serde_json::to_string(&dir).map_err(|e| e.to_string()));
    match result {
        Ok(json) => CString::new(json).unwrap_or_default().into_raw(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// この機械のユーザー全体（`git config --global`）の `key` の値。なければ、または読めなければ NULL（読めなければ
/// `amonhen_last_error` に理由）。`amonhen_string_free` で解放する。git を起動するので、裏のスレッドで呼ぶ。
///
/// # Safety
/// `key` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_global_config(key: *const c_char) -> *mut c_char {
    let result = str_arg(key)
        .map_err(Into::into)
        .and_then(amonhen_backend_cli::global_config);
    match result {
        Ok(Some(value)) => CString::new(value).unwrap_or_default().into_raw(),
        Ok(None) => std::ptr::null_mut(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// この機械のユーザー全体（`git config --global`）の `key` を変える。`value` が NULL なら消す。失敗したら false
/// （理由は `amonhen_last_error`）。git を起動するので、裏のスレッドで呼ぶ。
///
/// # Safety
/// `key` は NUL 終端の UTF-8 文字列、`value` は NULL か NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_set_global_config(
    key: *const c_char,
    value: *const c_char,
) -> bool {
    let value = if value.is_null() {
        Ok(None)
    } else {
        str_arg(value).map(Some)
    };
    match (str_arg(key), value) {
        (Ok(key), Ok(value)) => done(amonhen_backend_cli::set_global_config(key, value)),
        (Err(e), _) | (_, Err(e)) => done(Err(e.into())),
    }
}

/// SSH 先のフォルダの下のリポジトリを探す（リポジトリの一覧の、SSH 先のスキャンするフォルダ）。`location` は
/// `ssh://[user@]host[:port]/path`。エンジンの用意は SSH 先のリポジトリを開くときと同じ（`daemon`・`daemons_dir`）。
/// `excludes` は入らないフォルダの名前を改行でつないだもの。見つけたリポジトリの場所（`ssh://host/path`）を改行で
/// つないで返す（`amonhen_string_free` で解放）。失敗したら NULL（`amonhen_last_error`）。
///
/// `host_options` は、SSH 先ごとの選択（エンジンと、その起動のしかた。`HostOptions` の JSON）。NULL か空なら何も選んでいない。
///
/// # Safety
/// `location`・`daemon`・`daemons_dir`・`excludes` は NUL 終端の UTF-8、`host_options` は NULL か NUL 終端の UTF-8
/// であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_find_remote_repositories(
    location: *const c_char,
    depth: u32,
    excludes: *const c_char,
    daemon: *const c_char,
    daemons_dir: *const c_char,
    host_options: *const c_char,
) -> *mut c_char {
    let result = (|| -> amonhen_model::Result<String> {
        use amonhen_remote::ssh;
        let location = str_arg(location)?;
        let options = unsafe { host_options_arg(host_options) }?;
        let target = ssh::Target::parse(location)?;
        let daemons = ssh::Daemons::new(
            str_arg(daemons_dir)?.into(),
            Some((ssh::local_platform(), str_arg(daemon)?.into())),
        );
        let excludes: Vec<String> = str_arg(excludes)?
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
        let found =
            ssh::find_repositories(&program, &target, &daemons, &options, depth, &excludes)?;
        // 場所の形に戻す（`ssh://` と接続先はそのまま、パスだけ見つけたものに）。
        let prefix = &location[..location.len() - target.path.len()];
        Ok(found
            .iter()
            .map(|path| format!("{prefix}{path}"))
            .collect::<Vec<_>>()
            .join("\n"))
    })();
    match result {
        Ok(text) => CString::new(text).unwrap_or_default().into_raw(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// daemon 越しのリポジトリを開く。`location` は `daemon:/path`（手元の共有の daemon。いなければ `daemon` を起動）か
/// `ssh://[user@]host[:port]/path`（SSH 先に daemon を送り込んで起動）。SSH 先に送る daemon は
/// `daemons_dir` の `amonhend-<os>-<arch>`（SSH 先が手元と同じ OS と CPU なら `daemon`）。ssh のコマンドは
/// 環境変数 `AMONHEN_SSH` で差し替えられる。`cache_dir` を渡すと History を永続化する。
///
/// `host_options` は、SSH 先ごとの選択（エンジンと、その起動のしかた。`HostOptions` の JSON）。NULL か空なら何も選んでいない。
/// `daemon:ssh://…` なら共有の daemon に送り、daemon がエンジンを起動するときに使う。
///
/// # Safety
/// `location`・`daemon`・`daemons_dir` は NUL 終端の UTF-8、`cache_dir`・`host_options` は NULL か NUL 終端の UTF-8
/// であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_open_location(
    location: *const c_char,
    daemon: *const c_char,
    daemons_dir: *const c_char,
    cache_dir: *const c_char,
    host_options: *const c_char,
) -> *mut AmonhenRepo {
    let result = (|| -> amonhen_model::Result<Repository> {
        let location = str_arg(location)?;
        let daemon = str_arg(daemon)?;
        let options = unsafe { host_options_arg(host_options) }?;
        let conn = if location.starts_with("ssh://") {
            use amonhen_remote::ssh;
            let daemons = ssh::Daemons::new(
                str_arg(daemons_dir)?.into(),
                Some((ssh::local_platform(), daemon.into())),
            );
            let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
            ssh::connect(&program, &ssh::Target::parse(location)?, &daemons, &options)?
        } else {
            // 手元の共有の daemon（いなければ `daemon` を起動する）につなぐ。
            let path = amonhen_remote::location::local_path(location)?;
            amonhen_remote::local::connect_with(path, daemon, &options)?
        };
        let backend = amonhen_remote::RemoteBackend::connect(conn)?;
        // 永続キャッシュは場所の文字列ごとに手元に持つ（つなぎ直したら新しいコミットだけを受け取る）。手元の共有の
        // daemon なら、daemon が用意した History のファイルを直接読むので、手元には持たない。
        Ok(if cache_dir.is_null() || !location.starts_with("ssh://") {
            Repository::new(backend)
        } else {
            Repository::with_cache(
                backend,
                cache::path_for(Path::new(str_arg(cache_dir)?), location),
            )
        })
    })();
    match result {
        Ok(repo) => Box::into_raw(Box::new(repo)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `path`（UTF-8）を含むリポジトリを開く。`cache_dir` を渡すと、History をそこに永続化する。
///
/// # Safety
/// `path` は NUL 終端の文字列、`cache_dir` は NULL か NUL 終端の文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_open(
    path: *const c_char,
    cache_dir: *const c_char,
) -> *mut AmonhenRepo {
    let (path, cache_dir) = match (
        str_arg(path),
        (!cache_dir.is_null())
            .then(|| str_arg(cache_dir))
            .transpose(),
    ) {
        (Ok(p), Ok(c)) => (p, c),
        (Err(e), _) | (_, Err(e)) => {
            set_error(e);
            return std::ptr::null_mut();
        }
    };
    match GixBackend::open(path) {
        Ok(backend) => {
            let key = std::fs::canonicalize(path)
                .map_or_else(|_| path.to_owned(), |p| p.display().to_string());
            let repo = match cache_dir {
                Some(dir) => Repository::with_cache(backend, cache::path_for(Path::new(dir), &key)),
                None => Repository::new(backend),
            };
            Box::into_raw(Box::new(repo)).cast()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `repo` は `amonhen_repo_open` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_free(repo: *mut AmonhenRepo) {
    if !repo.is_null() {
        drop(unsafe { Box::from_raw(repo.cast::<Repository>()) });
    }
}

/// History を読み直して公開し、そのスナップショットを返す（`amonhen_history_release` で手放す）。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_load(
    repo_ptr: *const AmonhenRepo,
) -> *const AmonhenHistory {
    match repo!(repo_ptr).load_history() {
        Ok(snapshot) => into_history(snapshot),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// History の並び順を変える（`order` は 0 が日付順、1 がトポロジー順）。公開している History があれば、
/// git を読み直さずにその順に並べ直して公開し直す（大きなリポジトリでは秒単位。裏のスレッドから呼ぶこと）。
/// 手元の共有の daemon につないでいれば、daemon の分も変える。失敗なら false。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_set_order(
    repo_ptr: *const AmonhenRepo,
    order: u32,
) -> bool {
    let order = match order {
        0 => amonhen_model::HistoryOrder::Date,
        1 => amonhen_model::HistoryOrder::Topo,
        _ => {
            set_error(tr!("Unknown History order"));
            return false;
        }
    };
    match repo!(repo_ptr).set_history_order(order) {
        Ok(()) => true,
        Err(e) => {
            set_error(e);
            false
        }
    }
}

/// History を最初から作り直して公開し、そのスナップショットを返す（`amonhen_history_release` で手放す）。
/// 永続キャッシュを捨てる（手元の共有の daemon につないでいれば、daemon の分も）。裏のスレッドから呼ぶこと。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_rebuild(
    repo_ptr: *const AmonhenRepo,
) -> *const AmonhenHistory {
    match repo!(repo_ptr).rebuild_history() {
        Ok(snapshot) => into_history(snapshot),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// 永続キャッシュにある前回の History を公開して返す。キャッシュがなければ NULL。
/// mmap するだけなので、起動直後にメインスレッドから呼んでよい。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_load_cached(
    repo_ptr: *const AmonhenRepo,
) -> *const AmonhenHistory {
    repo!(repo_ptr)
        .load_cached_history()
        .map_or(std::ptr::null(), into_history)
}

/// mailmap と `log.mailmap` の指紋。リポジトリが変わったときにこれが変わっていたら、作者名を出し直す（History の
/// 行を読み直す。History を作り直す必要はない）。daemon 越しなら、その側で読んだもの。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_mailmap_fingerprint(repo_ptr: *const AmonhenRepo) -> u64 {
    repo!(repo_ptr).mailmap_fingerprint()
}

/// commit-graph を最新にする（`git commit-graph write --reachable`）。なければ全体を書き出して 1、
/// あれば後から増えたコミットだけを書き足して（`--split`）0、失敗したら -1 を返す。git maintenance に
/// 登録してあれば（そちらが毎時書く）何もせずに 0。利用者が設定で選んだときだけ、裏のスレッドから呼ぶこと。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_update_commit_graph(repo_ptr: *const AmonhenRepo) -> i32 {
    match repo!(repo_ptr).update_commit_graph() {
        Ok(written) => i32::from(written),
        Err(e) => {
            set_error(e);
            -1
        }
    }
}

/// 最新のスナップショット。まだ読んでいなければ NULL。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_acquire(
    repo_ptr: *const AmonhenRepo,
) -> *const AmonhenHistory {
    repo!(repo_ptr)
        .history()
        .map_or(std::ptr::null(), into_history)
}

/// # Safety
/// `history` は `amonhen_history_load` / `amonhen_history_acquire` が返したもので、まだ手放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_release(history: *const AmonhenHistory) {
    if !history.is_null() {
        drop(unsafe { Arc::from_raw(history.cast::<HistorySnapshot>()) });
    }
}

/// # Safety
/// `history` は有効なスナップショットであること。返したポインタは手放すまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_view(
    history_ptr: *const AmonhenHistory,
) -> AmonhenHistoryView {
    let h = history!(history_ptr);
    AmonhenHistoryView {
        generation: h.generation,
        row_count: h.layout.nodes().len() as u32,
        lane_count: h.layout.lane_count(),
        nodes: non_null(h.layout.nodes()),
        edges: non_null(h.layout.edges()),
        edge_count: h.layout.edges().len() as u32,
        laid_out_rows: h.layout.laid_out(),
    }
}

/// `[start, end)` 行に重なる線の添字を `out` に最大 `capacity` 個書き、全体の個数を返す。
///
/// # Safety
/// `history` は有効なスナップショット、`out` は `capacity` 個の `u32` を書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_edges_in(
    history_ptr: *const AmonhenHistory,
    start: u32,
    end: u32,
    out: *mut u32,
    capacity: u32,
) -> u32 {
    let out = unsafe { out_slice(out, capacity as usize) };
    let mut count = 0;
    for i in history!(history_ptr).layout.edges_in(start..end) {
        if let Some(slot) = out.get_mut(count as usize) {
            slot.write(i);
        }
        count += 1;
    }
    count
}

pub enum AmonhenLineageWindow {}

/// `row` 行目のコミットの祖先を、描く範囲だけ求めるもの（`amonhen_lineage_window_extend` で伸ばし、
/// `amonhen_lineage_window_free` で解放する）。まだ何も求めていない。
///
/// # Safety
/// `history` は有効なスナップショットであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_lineage_window_new(
    history_ptr: *const AmonhenHistory,
    row: u32,
) -> *mut AmonhenLineageWindow {
    let h = history!(history_ptr);
    Box::into_raw(Box::new(amonhen_core::lineage::LineageWindow::new(
        &h.graph, row,
    )))
    .cast()
}

/// `window`（`amonhen_lineage_window_new` が返したもの）を書き換えられる参照に戻す。NULL なら `handle!` と
/// 同じく関数から返す。
macro_rules! lineage_window {
    ($ptr:expr) => {
        match unsafe { $ptr.cast::<amonhen_core::lineage::LineageWindow>().as_mut() } {
            Some(window) => window,
            None => {
                set_error(concat!("NULL was passed as ", stringify!($ptr)));
                return Fallback::fallback();
            }
        }
    };
}

/// `end` 行の手前まで求める。かかる時間は伸ばした行と線の数に比例する。
///
/// # Safety
/// `history` は `window` を作ったときのスナップショット、`window` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_lineage_window_extend(
    window: *mut AmonhenLineageWindow,
    history_ptr: *const AmonhenHistory,
    end: u32,
) {
    let h = history!(history_ptr);
    lineage_window!(window).extend(&h.graph, &h.layout, end);
}

/// ここまで求めた行（含まない）。
///
/// # Safety
/// `window` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_lineage_window_end(window: *mut AmonhenLineageWindow) -> u32 {
    lineage_window!(window).end()
}

/// `row` 行目のコミットが祖先か（求めた範囲の外なら false）。
///
/// # Safety
/// `window` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_lineage_window_contains(
    window: *mut AmonhenLineageWindow,
    row: u32,
) -> bool {
    lineage_window!(window).contains(row)
}

/// `edge` 番目の線を光らせ始める行（`u32::MAX` なら光らせない）。
///
/// # Safety
/// `window` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_lineage_window_edge_from(
    window: *mut AmonhenLineageWindow,
    edge: u32,
) -> u32 {
    // NULL なら、光らせない（`u32::MAX`）。
    if window.is_null() {
        set_error("NULL was passed as window");
        return u32::MAX;
    }
    lineage_window!(window).edge_from(edge)
}

/// # Safety
/// `window` は `amonhen_lineage_window_new` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_lineage_window_free(window: *mut AmonhenLineageWindow) {
    if !window.is_null() {
        drop(unsafe { Box::from_raw(window.cast::<amonhen_core::lineage::LineageWindow>()) });
    }
}

/// レーンの幅の決め方（`amonhen_history_lane_positions`）。単位は FE の座標。
#[repr(C)]
pub struct AmonhenLaneMetrics {
    /// 点のあるレーンの幅。
    pub full: f32,
    /// 線だけのレーンの幅。
    pub thin: f32,
    pub min_full: f32,
    pub min_thin: f32,
    /// 使える幅。
    pub available: f32,
}

/// `start..end` 行を表示するときの、レーンごとの中心の横位置（魚眼）を `out` に書く。見えている範囲に点の
/// あるレーンを広く、線だけのレーンを細く、何もないレーンを幅 0 にする。
/// レーンの数を返す（`capacity` を超えた分は書かない）。`width` が NULL でなければ、レーン全体の幅を書く。
///
/// # Safety
/// `history` は有効なスナップショット、`out` は `capacity` 個の `float` を書ける領域、`width` は NULL か
/// `float` を 1 つ書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_lane_positions(
    history_ptr: *const AmonhenHistory,
    start: u32,
    end: u32,
    metrics: AmonhenLaneMetrics,
    out: *mut f32,
    capacity: u32,
    width: *mut f32,
) -> u32 {
    let m = amonhen_core::lanes::LaneMetrics {
        full: metrics.full,
        thin: metrics.thin,
        min_full: metrics.min_full,
        min_thin: metrics.min_thin,
        available: metrics.available,
    };
    let (positions, total) =
        amonhen_core::lanes::lane_positions(&history!(history_ptr).layout, start..end, &m);
    let out = unsafe { out_slice(out, capacity as usize) };
    for (slot, x) in out.iter_mut().zip(&positions) {
        slot.write(*x);
    }
    unsafe { put(width, total) };
    positions.len() as u32
}

#[repr(C)]
pub struct AmonhenRowsView {
    pub rows: *const u32,
    pub count: u32,
}

/// `query` に合うコミットの行（昇順、`amonhen_rows_free` で解放する）。構文は `amonhen-core` の
/// `query`（`author:` `committer:` `subject:` `message:` `commit:` `path:` `contents:` `after:` `before:`、AND・OR・括弧）。すべてのコミットを読むので
/// 裏のスレッドで呼ぶ（linux の 148 万コミットで約 1.2 秒）。新しい検索を始めるか `amonhen_search_cancel` を
/// 呼ぶと、走っている検索は NULL を返す。
///
/// # Safety
/// `repo` と `history` は有効、`query` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_search(
    repo_ptr: *const AmonhenRepo,
    history_ptr: *const AmonhenHistory,
    query: *const c_char,
) -> *mut AmonhenRows {
    let (repo, history) = (repo!(repo_ptr), history!(history_ptr));
    let result = str_arg(query)
        .map_err(Into::into)
        .and_then(|q| repo.search_history(history, q));
    match result {
        Ok(rows) => Box::into_raw(Box::new(rows)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// partial clone で、`query` の内容の条件（`contents:`）を探す前に取る、手元にない中身の数。取るものがあれば 1
/// を返して `count` と `more`（上限を超えたので数えるのをやめた）を書き、なければ 0、失敗（取り消しも）なら -1。
/// 履歴を辿るので裏のスレッドで呼ぶ。`amonhen_search_cancel` でやめられる。
///
/// # Safety
/// `repo` と `history` は有効、`query` は NUL 終端の UTF-8、`count` と `more` は書けること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_contents_to_fetch(
    repo_ptr: *const AmonhenRepo,
    history_ptr: *const AmonhenHistory,
    query: *const c_char,
    count: *mut u32,
    more: *mut bool,
) -> i32 {
    let (repo, history) = (repo!(repo_ptr), history!(history_ptr));
    let result = str_arg(query)
        .map_err(Into::into)
        .and_then(|q| repo.count_contents_to_fetch(history, q));
    match result {
        Ok(Some(need)) => {
            unsafe {
                put(count, need.count as u32);
                put(more, need.more);
            }
            1
        }
        Ok(None) => 0,
        Err(e) => {
            set_error(e);
            -1
        }
    }
}

/// 走っている検索（`amonhen_history_search`）の、そこまでに分かった行（昇順、`amonhen_rows_free` で解放する）。
/// `done` に探し終えた割合（0〜1。内容の条件がなければ 0 のまま）を書く。
///
/// # Safety
/// `repo` は有効、`done` は書けること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_search_partial(
    repo_ptr: *const AmonhenRepo,
    done: *mut f32,
) -> *mut AmonhenRows {
    let (rows, fraction) = repo!(repo_ptr).search_partial();
    unsafe { put(done, fraction) };
    Box::into_raw(Box::new(rows)).cast()
}

/// 走っている検索をやめる。
///
/// # Safety
/// `repo` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_search_cancel(repo_ptr: *const AmonhenRepo) {
    repo!(repo_ptr).cancel_search();
}

/// # Safety
/// `rows` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_rows_view(rows: *const AmonhenRows) -> AmonhenRowsView {
    let rows = handle!(rows, Vec<u32>);
    AmonhenRowsView {
        rows: rows.as_ptr(),
        count: rows.len() as u32,
    }
}

/// # Safety
/// `rows` は `amonhen_history_search` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_rows_free(rows: *mut AmonhenRows) {
    if !rows.is_null() {
        drop(unsafe { Box::from_raw(rows.cast::<Vec<u32>>()) });
    }
}

/// `start` 行から `count` 行分の列を作る（`amonhen_chunk_free` で解放する）。
///
/// # Safety
/// `repo` と `history` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_chunk(
    repo_ptr: *const AmonhenRepo,
    history_ptr: *const AmonhenHistory,
    start: u32,
    count: u32,
    encoding: AmonhenEncoding,
) -> *mut AmonhenChunk {
    match repo!(repo_ptr).history_chunk(history!(history_ptr), start, count, &profile(encoding)) {
        Ok(chunk) => Box::into_raw(Box::new(chunk)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `chunk` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_chunk_view(chunk: *const AmonhenChunk) -> AmonhenChunkView {
    let c = handle!(chunk, HistoryChunk);
    AmonhenChunkView {
        start_row: c.start_row,
        row_count: c.len() as u32,
        subjects: non_null(&c.subjects),
        author_names: non_null(&c.author_names),
        author_times: non_null(&c.author_times),
        signatures: non_null(&c.signatures),
        bases: c.bases().as_ptr(),
        base_count: c.bases().len() as u32,
    }
}

/// # Safety
/// `chunk` は `amonhen_history_chunk` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_chunk_free(chunk: *mut AmonhenChunk) {
    if !chunk.is_null() {
        drop(unsafe { Box::from_raw(chunk.cast::<HistoryChunk>()) });
    }
}

/// `history` で `oid`（20 バイト）が何行目か。なければ -1。
///
/// # Safety
/// `history` は有効なスナップショット、`oid` は 20 バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_row_of(
    history_ptr: *const AmonhenHistory,
    oid: *const u8,
) -> i64 {
    let id = oid!(oid);
    history!(history_ptr).row_of(&id).map_or(-1, i64::from)
}

/// ref の 1 項目。文字列は `AmonhenRefsView::strings` の中の UTF-8。
#[repr(C)]
pub struct AmonhenRefEntry {
    /// 0: ローカルブランチ、1: リモートブランチ、2: タグ、3: stash、4: サブモジュール。
    pub kind: u8,
    pub has_target: u8,
    pub _pad: [u8; 2],
    pub name_off: u32,
    pub name_len: u32,
    /// 上流ブランチ・stash のメッセージ。なければ `detail_len` が `u32::MAX`。
    pub detail_off: u32,
    pub detail_len: u32,
    pub target: [u8; 20],
    /// ローカルブランチの、上流より先・後ろにあるコミットの数。
    pub ahead: u32,
    pub behind: u32,
    /// 上流のリモートブランチがもうない。
    pub gone: u8,
    pub _pad2: [u8; 3],
}

#[repr(C)]
pub struct AmonhenRefsView {
    pub generation: u64,
    pub entries: *const AmonhenRefEntry,
    pub count: u32,
    pub strings: *const u8,
    /// チェックアウト中のブランチ。detached HEAD なら `head_branch_len` が `u32::MAX`。
    pub head_branch_off: u32,
    pub head_branch_len: u32,
    /// HEAD のコミット。まだなければ `has_head` が 0。
    pub has_head: u8,
    pub head: [u8; 20],
}

/// 一覧は数百〜数千件なので、文字列は 1 つの領域に詰めて渡す。
struct RefsHandle {
    snapshot: Arc<RefsSnapshot>,
    entries: Vec<AmonhenRefEntry>,
    strings: Vec<u8>,
    head_branch: (u32, u32),
}

impl RefsHandle {
    fn new(snapshot: Arc<RefsSnapshot>) -> Self {
        let mut strings = Vec::new();
        let mut put = |s: Option<&str>| match s {
            Some(s) => {
                let off = strings.len() as u32;
                strings.extend_from_slice(s.as_bytes());
                (off, s.len() as u32)
            }
            None => (0, u32::MAX),
        };
        let head_branch = put(snapshot.refs.head_branch.as_deref());
        let entries = snapshot
            .refs
            .entries
            .iter()
            .map(|e| {
                let (name_off, name_len) = put(Some(&e.name));
                let (detail_off, detail_len) = put(e.detail.as_deref());
                AmonhenRefEntry {
                    kind: e.kind as u8,
                    has_target: u8::from(e.target.is_some()),
                    _pad: [0; 2],
                    name_off,
                    name_len,
                    detail_off,
                    detail_len,
                    target: e.target.map_or([0; 20], |o| o.0),
                    ahead: e.ahead,
                    behind: e.behind,
                    gone: u8::from(e.gone),
                    _pad2: [0; 3],
                }
            })
            .collect();
        Self {
            snapshot,
            entries,
            strings,
            head_branch,
        }
    }
}

/// ref の一覧を読み直して返す（`amonhen_refs_release` で手放す）。前回と同じなら同じ世代。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_refs_load(repo_ptr: *const AmonhenRepo) -> *const AmonhenRefs {
    match repo!(repo_ptr).load_refs() {
        Ok(snapshot) => Arc::into_raw(Arc::new(RefsHandle::new(snapshot))).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// # Safety
/// `refs` は `amonhen_refs_load` が返したもので、まだ手放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_refs_release(refs: *const AmonhenRefs) {
    if !refs.is_null() {
        drop(unsafe { Arc::from_raw(refs.cast::<RefsHandle>()) });
    }
}

/// # Safety
/// `refs` は有効であること。返したポインタは手放すまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_refs_view(refs: *const AmonhenRefs) -> AmonhenRefsView {
    let r = handle!(refs, RefsHandle);
    AmonhenRefsView {
        generation: r.snapshot.generation,
        entries: r.entries.as_ptr(),
        count: r.entries.len() as u32,
        strings: r.strings.as_ptr(),
        head_branch_off: r.head_branch.0,
        head_branch_len: r.head_branch.1,
        has_head: u8::from(r.snapshot.refs.head.is_some()),
        head: r.snapshot.refs.head.map_or([0; 20], |o| o.0),
    }
}

/// 変化の種類（`amonhen_repo_watch` のコールバックに渡すビット）。
pub const AMONHEN_CHANGE_REFS: u32 = 1 << 0;
pub const AMONHEN_CHANGE_INDEX: u32 = 1 << 1;
pub const AMONHEN_CHANGE_WORKTREE: u32 = 1 << 2;
pub const AMONHEN_CHANGE_CONFIG: u32 = 1 << 3;

struct Context(*mut std::ffi::c_void);

// FE が渡す文脈。コールバックで FE に返すだけで、Rust 側では触らない。
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

/// リポジトリの監視を始める。変化があると、短い間の変化をまとめてから `callback(context, 変化の種類)`
/// を監視用のスレッドから呼ぶ。`amonhen_watch_free` で止める。
///
/// # Safety
/// `repo` は有効なリポジトリ。`callback` は `amonhen_watch_free` まで、どのスレッドから呼んでもよいこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_watch(
    repo_ptr: *const AmonhenRepo,
    callback: extern "C" fn(*mut std::ffi::c_void, u32),
    context: *mut std::ffi::c_void,
) -> *mut AmonhenWatch {
    let context = Arc::new(Context(context));
    // daemon 越しなら daemon の知らせを使う。止めたら（`amonhen_watch_free`）呼ばない。
    let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (remote_context, remote_active) = (context.clone(), active.clone());
    if repo!(repo_ptr).subscribe_changes(Box::new(move |bits| {
        if remote_active.load(std::sync::atomic::Ordering::Relaxed) {
            callback(remote_context.0, bits);
        }
    })) {
        return Box::into_raw(Box::new(WatchHandle::Remote(active))).cast();
    }
    let watcher = Watcher::new(repo!(repo_ptr).paths(), move |changes| {
        callback(context.0, changes.0);
    });
    match watcher {
        Ok(w) => Box::into_raw(Box::new(WatchHandle::Local(w))).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `watch` は `amonhen_repo_watch` が返したもので、まだ止めていないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_watch_free(watch: *mut AmonhenWatch) {
    if !watch.is_null() {
        let handle = unsafe { Box::from_raw(watch.cast::<WatchHandle>()) };
        if let WatchHandle::Remote(active) = &*handle {
            active.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// 変わったファイル 1 つ。文字列は `AmonhenChangesView::strings` の中の UTF-8。
#[repr(C)]
pub struct AmonhenChangeEntry {
    /// 0: 追加、1: 削除、2: 変更、3: 名前の変更、4: コピー。
    pub kind: u8,
    pub submodule: u8,
    pub _pad: [u8; 2],
    pub path_off: u32,
    pub path_len: u32,
    /// 名前を変えた・コピーしたときの元のパス。なければ `old_path_len` が `u32::MAX`。
    pub old_path_off: u32,
    pub old_path_len: u32,
}

#[repr(C)]
pub struct AmonhenChangesView {
    pub entries: *const AmonhenChangeEntry,
    pub count: u32,
    pub strings: *const u8,
}

struct ChangesHandle {
    changes: Vec<FileChange>,
    entries: Vec<AmonhenChangeEntry>,
    strings: Vec<u8>,
}

/// `commit`（20 バイト）で変わったファイル（`amonhen_changes_free` で解放する）。
///
/// # Safety
/// `repo` は有効なリポジトリ、`commit` は 20 バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_commit_changes(
    repo_ptr: *const AmonhenRepo,
    commit: *const u8,
) -> *mut AmonhenChanges {
    let id = oid!(commit);
    changes_handle(repo!(repo_ptr).commit_changes(&id))
}

/// stash（20 バイト）の中身。untracked のファイルを含めていればそれも（`amonhen_changes_free` で解放する）。
///
/// # Safety
/// `repo` は有効なリポジトリ、`stash` は 20 バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stash_changes(
    repo_ptr: *const AmonhenRepo,
    stash: *const u8,
) -> *mut AmonhenChanges {
    let id = oid!(stash);
    changes_handle(repo!(repo_ptr).stash_changes(&id))
}

/// コミット `base`（20 バイト。NULL なら全ファイルの追加）から `tip`（20 バイト）への変更で変わったファイル。
/// push で送る範囲（upstream..HEAD）などに（`amonhen_changes_free` で解放する）。
///
/// # Safety
/// `repo` は有効なリポジトリ、`base` は NULL か 20 バイト読める領域、`tip` は 20 バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_range_changes(
    repo_ptr: *const AmonhenRepo,
    base: *const u8,
    tip: *const u8,
) -> *mut AmonhenChanges {
    let base = if base.is_null() {
        None
    } else {
        Some(oid!(base))
    };
    let tip = oid!(tip);
    changes_handle(repo!(repo_ptr).changes_between(base.as_ref(), &tip))
}

fn changes_handle(result: amonhen_model::Result<Vec<FileChange>>) -> *mut AmonhenChanges {
    let changes = match result {
        Ok(c) => c,
        Err(e) => {
            set_error(e);
            return std::ptr::null_mut();
        }
    };
    let mut strings = Vec::new();
    let mut put = |s: Option<&str>| match s {
        Some(s) => {
            let off = strings.len() as u32;
            strings.extend_from_slice(s.as_bytes());
            (off, s.len() as u32)
        }
        None => (0, u32::MAX),
    };
    let entries = changes
        .iter()
        .map(|c| {
            let (path_off, path_len) = put(Some(&c.path));
            let (old_path_off, old_path_len) = put(c.old_path.as_deref());
            AmonhenChangeEntry {
                kind: c.kind as u8,
                submodule: u8::from(c.submodule),
                _pad: [0; 2],
                path_off,
                path_len,
                old_path_off,
                old_path_len,
            }
        })
        .collect();
    Box::into_raw(Box::new(ChangesHandle {
        changes,
        entries,
        strings,
    }))
    .cast()
}

/// # Safety
/// `changes` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_changes_view(
    changes: *const AmonhenChanges,
) -> AmonhenChangesView {
    let c = handle!(changes, ChangesHandle);
    AmonhenChangesView {
        entries: c.entries.as_ptr(),
        count: c.entries.len() as u32,
        strings: c.strings.as_ptr(),
    }
}

/// # Safety
/// `changes` は `amonhen_commit_changes` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_changes_free(changes: *mut AmonhenChanges) {
    if !changes.is_null() {
        drop(unsafe { Box::from_raw(changes.cast::<ChangesHandle>()) });
    }
}

#[repr(C)]
pub struct AmonhenDiffView {
    pub rows: *const DiffRow,
    pub row_count: u32,
    /// 文字列置き場の先頭アドレス。`StrRef::buf` で引く。
    pub bases: *const *const u8,
    pub base_count: u32,
    pub binary: u8,
    pub added: u32,
    pub removed: u32,
    /// 削除行と追加行の組の中で、変わった単語の範囲（行の順）。
    pub highlights: *const Highlight,
    pub highlight_count: u32,
}

/// `changes` の `index` 番目のファイルの差分（`amonhen_diff_free` で解放する）。
///
/// # Safety
/// `repo` と `changes` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_file_diff(
    repo_ptr: *const AmonhenRepo,
    changes: *const AmonhenChanges,
    index: u32,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    let c = handle!(changes, ChangesHandle);
    let Some(change) = c.changes.get(index as usize) else {
        set_error(tr!("File index out of range"));
        return std::ptr::null_mut();
    };
    match repo!(repo_ptr).file_diff(change, &profile(encoding)) {
        Ok(view) => Box::into_raw(Box::new(view)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// blob `oid`（20 バイト）の中身。長さを `len` に書く（`amonhen_blob_free` で解放する）。読めなければ NULL
/// （`amonhen_last_error`）。画像の比較や、その時点の画像を見せるのに使う。
///
/// # Safety
/// `repo` は有効、`oid` は 20 バイトを読める領域、`len` は書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_blob_read(
    repo_ptr: *const AmonhenRepo,
    oid: *const u8,
    len: *mut usize,
) -> *mut u8 {
    let oid = oid!(oid);
    match repo!(repo_ptr).blob(&oid) {
        Ok(data) => blob_out(&data, len),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `changes` の `index` 番目のファイルの、変更前（`after` が false）か変更後の中身（`amonhen_blob_free` で解放
/// する）。その側にない（追加の前・削除の後）なら NULL で、`len` は 0。
///
/// # Safety
/// `repo` と `changes` は有効、`len` は書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_change_blob(
    repo_ptr: *const AmonhenRepo,
    changes: *const AmonhenChanges,
    index: u32,
    after: bool,
    len: *mut usize,
) -> *mut u8 {
    let c = handle!(changes, ChangesHandle);
    if let Some(len) = unsafe { len.as_mut() } {
        *len = 0;
    }
    let Some(change) = c.changes.get(index as usize) else {
        set_error(tr!("File index out of range"));
        return std::ptr::null_mut();
    };
    let Some(id) = (if after { change.new_id } else { change.old_id }) else {
        return std::ptr::null_mut();
    };
    match repo!(repo_ptr).blob(&id) {
        Ok(data) => blob_out(&data, len),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

fn blob_out(data: &[u8], len: *mut usize) -> *mut u8 {
    if let Some(len) = unsafe { len.as_mut() } {
        *len = data.len();
    }
    Box::into_raw(data.to_vec().into_boxed_slice()).cast()
}

/// SVG（`data` の `len` バイト）が、モバイル向けの描画部品 `target`（0 Android・1 iOS・2 flutter_svg・
/// 3 react-native-svg）でどう見えるか（コアの `svgmobile::preview`）。描ける SVG を返し、長さを `out_len` に書く
/// （`amonhen_blob_free` で解放する）。部品が読めない SVG なら NULL。`info` に JSON
/// `{"title", "caption", "messages": [...], "error": 文字列か null, "exact": bool}` を書く（`amonhen_string_free`
/// で解放する）。
///
/// # Safety
/// `data` は `len` バイト読めること（`len` が 0 なら NULL でもよい）。`out_len` と `info` は書けること（NULL でもよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_svg_preview(
    data: *const u8,
    len: usize,
    target: u8,
    out_len: *mut usize,
    info: *mut *mut c_char,
) -> *mut u8 {
    let bytes = if len == 0 || data.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    let preview = amonhen_core::svgmobile::preview(bytes, svg_target(target));
    if !info.is_null() {
        let json = serde_json::json!({
            "title": preview.title,
            "caption": preview.caption,
            "messages": preview.messages,
            "error": preview.error,
            "exact": preview.exact,
        })
        .to_string();
        unsafe { *info = CString::new(json).map_or(std::ptr::null_mut(), CString::into_raw) };
    }
    match preview.svg {
        Some(svg) => blob_out(&svg, out_len),
        None => {
            if !out_len.is_null() {
                unsafe { *out_len = 0 };
            }
            std::ptr::null_mut()
        }
    }
}

/// 0 Android・1 iOS・2 flutter_svg・3 react-native-svg。
fn svg_target(n: u8) -> amonhen_core::svglint::Target {
    use amonhen_core::svglint::Target;
    match n {
        1 => Target::Ios,
        2 => Target::Flutter,
        3 => Target::ReactNative,
        _ => Target::Android,
    }
}

/// 透けた絵の下の市松模様を暗くするのが既定か（コアの `checker::Tone`）。`pixels` は `height` 行 × `stride` バイトの、
/// 不透明度を掛けた RGBA（`bgra` なら BGRA）の画素。比べる 2 枚は 1 つの画素の並びに並べて渡す。`transparent` に
/// 透けた画素があったか（なければ市松模様は見えない）を書く。
///
/// # Safety
/// `pixels` は `height * stride` バイト読めること（NULL なら何も数えない）。`transparent` は書けること（NULL でもよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_checker_dark(
    pixels: *const u8,
    width: usize,
    height: usize,
    stride: usize,
    bgra: bool,
    transparent: *mut bool,
) -> bool {
    let mut tone = amonhen_core::checker::Tone::default();
    if !pixels.is_null() && height > 0 {
        let bytes = unsafe { std::slice::from_raw_parts(pixels, height * stride) };
        let order = if bgra {
            amonhen_core::checker::Order::Bgra
        } else {
            amonhen_core::checker::Order::Rgba
        };
        tone.add(bytes, width, height, stride, order);
    }
    if let Some(transparent) = unsafe { transparent.as_mut() } {
        *transparent = tone.transparent();
    }
    tone.dark()
}

/// `path`（NUL で終わる UTF-8）の中身（`data` の `len` バイト）が、History の「すべてのファイル」で絵にする Android の
/// VectorDrawable か（`.xml` で、根が `<vector>` か `<animated-vector>`。頭だけを見る）。
///
/// # Safety
/// `path` は NUL で終わる文字列、`data` は `len` バイト読めること（`len` が 0 なら NULL でもよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_is_vector_drawable(
    path: *const c_char,
    data: *const u8,
    len: usize,
) -> bool {
    let Ok(path) = str_arg(path) else {
        return false;
    };
    let bytes = if len == 0 || data.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    amonhen_core::vectordrawable::is_vector_drawable(path, bytes)
}

/// コミット `commit`（20 バイト）の `path` の Android の VectorDrawable（中身 `data` の `len` バイト）を、描ける SVG
/// にする（コアの `Repository::vector_drawable`。色の参照は同じコミットのリソースから引く）。SVG を返し、長さを
/// `out_len` に書く（`amonhen_blob_free` で解放する）。描けなければ NULL で、理由は `amonhen_last_error`。`notes` に
/// 絵の下に添える説明の JSON の配列（文字列）を書く（`amonhen_string_free` で解放する）。引けない色は `fallback`
/// （画面の文字の色の `#rrggbb`。NULL なら灰色）で描く。`dark` なら暗い外観の資源（`values-night` など）を先に
/// 引く。裏のスレッドで呼ぶ。
///
/// # Safety
/// `repo` は有効、`commit` は 20 バイトを読める領域、`path` は NUL で終わる UTF-8、`data` は `len` バイト読めること。
/// `fallback` は NULL か NUL で終わる UTF-8。`out_len` と `notes` は書けること（NULL でもよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_vector_drawable(
    repo_ptr: *const AmonhenRepo,
    commit: *const u8,
    path: *const c_char,
    data: *const u8,
    len: usize,
    fallback: *const c_char,
    dark: bool,
    out_len: *mut usize,
    notes: *mut *mut c_char,
) -> *mut u8 {
    if let Some(out_len) = unsafe { out_len.as_mut() } {
        *out_len = 0;
    }
    if let Some(notes) = unsafe { notes.as_mut() } {
        *notes = std::ptr::null_mut();
    }
    let commit = oid!(commit);
    let path = match str_arg(path) {
        Ok(path) => path,
        Err(e) => {
            set_error(e);
            return std::ptr::null_mut();
        }
    };
    let bytes = if len == 0 || data.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    let fallback = str_arg(fallback).ok();
    match repo!(repo_ptr).vector_drawable(&commit, path, bytes, fallback, dark) {
        Ok(picture) => {
            if let Some(notes) = unsafe { notes.as_mut() } {
                let json = serde_json::to_string(&picture.notes).unwrap_or_default();
                *notes = CString::new(json).map_or(std::ptr::null_mut(), CString::into_raw);
            }
            blob_out(picture.svg.as_bytes(), out_len)
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `amonhen_blob_read`・`amonhen_change_blob` が返したものを解放する。
///
/// # Safety
/// `data` と `len` は、それらが返した組であること（NULL ならなにもしない）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_blob_free(data: *mut u8, len: usize) {
    if !data.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(data, len)) });
    }
}

/// `amonhen_file_diff` の、前後 `context` 行のもの（`UINT32_MAX` ならファイル全体）。`whitespace` は行を比べるときに
/// 無視する空白（0 無視しない・1 行末・2 量の違い（`-b`）・3 すべて（`-w`））。
///
/// # Safety
/// `repo` と `changes` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_file_diff_context(
    repo_ptr: *const AmonhenRepo,
    changes: *const AmonhenChanges,
    index: u32,
    context: u32,
    whitespace: u8,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    let c = handle!(changes, ChangesHandle);
    let Some(change) = c.changes.get(index as usize) else {
        set_error(tr!("File index out of range"));
        return std::ptr::null_mut();
    };
    let whitespace = amonhen_core::diff::Whitespace::from_u8(whitespace);
    match repo!(repo_ptr).file_diff_ws(change, context, whitespace, &profile(encoding)) {
        Ok(view) => Box::into_raw(Box::new(view)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `changes` のすべてのファイルの差分（`amonhen_stacked_free` で解放する）。ファイルの数だけ blob を読むので、
/// 裏のスレッドで呼ぶ。`whitespace` は `amonhen_file_diff_context` と同じ。
///
/// # Safety
/// `repo` と `changes` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_diff(
    repo_ptr: *const AmonhenRepo,
    changes: *const AmonhenChanges,
    whitespace: u8,
) -> *mut AmonhenStacked {
    let c = handle!(changes, ChangesHandle);
    let whitespace = amonhen_core::diff::Whitespace::from_u8(whitespace);
    match repo!(repo_ptr).stacked_diff_ws(&c.changes, whitespace) {
        Ok(s) => Box::into_raw(Box::new(s)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// 全ファイルを見出しの行（`DiffRow::FILE`）でつないだ差分の表示（`amonhen_diff_free` で解放する。
/// `amonhen_diff_view`・`amonhen_diff_syntax` が使える）。`collapsed` の i 番目が true のファイルは見出しだけ。
///
/// # Safety
/// `stacked` は有効、`collapsed` は `count` 個の `bool` を読める領域（`count` が 0 なら NULL でもよい）であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_view(
    stacked: *const AmonhenStacked,
    collapsed: *const bool,
    count: u32,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    let s = handle!(stacked, amonhen_core::diff::StackedDiff);
    let collapsed = array!(collapsed, count);
    Box::into_raw(Box::new(s.view(collapsed, &profile(encoding)))).cast()
}

/// `stacked` の `file` 番目のファイルの差分を、前後 `context` 行で取り直す（`UINT32_MAX` ならファイル全体）。
/// まだ差分を取っていないファイル（見出しの `PENDING`）なら、ここで取る。表示は `amonhen_stacked_view` で作り直す。
/// 読めなければ false（`amonhen_last_error`）。
///
/// # Safety
/// `repo` と `stacked` は有効で、`stacked` をほかのスレッドで同時に使っていないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_set_context(
    repo_ptr: *const AmonhenRepo,
    stacked: *mut AmonhenStacked,
    file: u32,
    context: u32,
) -> bool {
    let Some(s) = (unsafe { stacked.cast::<amonhen_core::diff::StackedDiff>().as_mut() }) else {
        set_error("NULL was passed as stacked");
        return false;
    };
    match repo!(repo_ptr).set_stacked_context(s, file as usize, context) {
        Ok(()) => true,
        Err(e) => {
            set_error(e);
            false
        }
    }
}

/// コミット `commit`（20 バイト）の `path` の中身を、変わっていない行として並べた表示（`amonhen_diff_free` で
/// 解放する）。History の「すべてのファイル」で、このコミットで変えていないファイルを開く。
///
/// # Safety
/// `repo` は有効、`commit` は 20 バイトを読める領域、`path` は NUL で終わる UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_tree_file(
    repo_ptr: *const AmonhenRepo,
    commit: *const u8,
    path: *const c_char,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    let commit = oid!(commit);
    let path = match str_arg(path) {
        Ok(path) => path,
        Err(e) => {
            set_error(e);
            return std::ptr::null_mut();
        }
    };
    match repo!(repo_ptr).tree_file(&commit, path, &profile(encoding)) {
        Ok(view) => Box::into_raw(Box::new(view)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// History で選んだコミット（`rows` は行、順不同）の全ファイルの差分（`amonhen_stacked_free` で解放する）。
/// `kind` に見せ方を書く: 0 は first-parent でつながる範囲（一番古いものの親から一番新しいものまで）、1 はつながって
/// いない 2 つ（古いほうから新しいほうまで）、2 はとびとびの 3 つ以上（コミットごとの差分を、見出し
/// 「パス — 短い ID」でパス順に）。`pair` は 2 つのときの取り方: 0 は古いほうから新しいほう、1 は逆、2 は共通の
/// 祖先から新しいほう（A...B）。`whitespace` は `amonhen_file_diff_context` と同じ。裏のスレッドで呼ぶ。
///
/// # Safety
/// `repo` と `history` は有効、`rows` は `count` 個の `u32`、`kind` は書き込める `u8` であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_selection_diff(
    repo_ptr: *const AmonhenRepo,
    history_ptr: *const AmonhenHistory,
    rows: *const u32,
    count: u32,
    pair: u8,
    whitespace: u8,
    kind: *mut u8,
) -> *mut AmonhenStacked {
    let rows = array!(rows, count);
    let pair = match pair {
        1 => amonhen_core::PairMode::Reverse,
        2 => amonhen_core::PairMode::MergeBase,
        _ => amonhen_core::PairMode::Forward,
    };
    let whitespace = amonhen_core::diff::Whitespace::from_u8(whitespace);
    match repo!(repo_ptr).selection_diff_ws(history!(history_ptr), rows, pair, whitespace) {
        Ok((selection, stacked)) => {
            let k = match selection {
                amonhen_core::Selection::Range { .. } => 0,
                amonhen_core::Selection::Pair { .. } => 1,
                amonhen_core::Selection::Each(_) => 2,
            };
            unsafe { put(kind, k) };
            Box::into_raw(Box::new(stacked)).cast()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// ファイルの数。
///
/// # Safety
/// `stacked` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_count(stacked: *const AmonhenStacked) -> u32 {
    handle!(stacked, amonhen_core::diff::StackedDiff)
        .files
        .len() as u32
}

/// `file` 番目のファイルが生成物か（最初は畳む）。`.gitattributes` の `linguist-generated`、なければ lock ファイルなど。
///
/// # Safety
/// `stacked` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_generated(
    stacked: *const AmonhenStacked,
    file: u32,
) -> bool {
    handle!(stacked, amonhen_core::diff::StackedDiff).generated(file as usize)
}

/// # Safety
/// `stacked` は `amonhen_stacked_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_free(stacked: *mut AmonhenStacked) {
    if !stacked.is_null() {
        drop(unsafe { Box::from_raw(stacked.cast::<amonhen_core::diff::StackedDiff>()) });
    }
}

/// # Safety
/// `diff` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_diff_view(diff: *const AmonhenDiff) -> AmonhenDiffView {
    diff_view(handle!(diff, DiffView))
}

fn diff_view(d: &DiffView) -> AmonhenDiffView {
    AmonhenDiffView {
        rows: d.rows.as_ptr(),
        row_count: d.rows.len() as u32,
        bases: d.strings.bases().as_ptr(),
        base_count: d.strings.bases().len() as u32,
        binary: u8::from(d.binary),
        added: d.added,
        removed: d.removed,
        highlights: d.highlights.as_ptr(),
        highlight_count: d.highlights.len() as u32,
    }
}

/// # Safety
/// `diff` は `amonhen_file_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_diff_free(diff: *mut AmonhenDiff) {
    if !diff.is_null() {
        drop(unsafe { Box::from_raw(diff.cast::<DiffView>()) });
    }
}

#[repr(C)]
pub struct AmonhenSyntaxView {
    /// 行の順、行の中では位置の順。
    pub spans: *const SyntaxSpan,
    pub span_count: u32,
}

/// 差分の構文の色付け（`amonhen_syntax_free` で解放する）。ファイル全体を解析するので、差分を表示してから
/// 裏のスレッドで呼ぶ。言語がわからなければ空。
///
/// # Safety
/// `diff` は `amonhen_file_diff`・`amonhen_conflict_view` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_diff_syntax(diff: *const AmonhenDiff) -> *mut AmonhenSyntax {
    let spans = handle!(diff, DiffView).syntax();
    Box::into_raw(Box::new(spans)).cast()
}

/// # Safety
/// `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_syntax(
    diff: *const AmonhenWorkingDiff,
) -> *mut AmonhenSyntax {
    let spans = handle!(diff, WorkingDiff).view.syntax();
    Box::into_raw(Box::new(spans)).cast()
}

/// テキスト全体の構文の色付け（`amonhen_syntax_free` で解放する）。範囲はテキストの先頭からの UTF-16 の
/// 位置で、`row` は 0。言語は `path` の拡張子で決め、わからなければ空。時間がかかるので裏のスレッドで呼ぶ。
///
/// # Safety
/// `path` は NUL 終端の UTF-8、`text` は `len` バイト読める（`len` が 0 なら NULL でもよい）こと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_text_syntax(
    path: *const c_char,
    text: *const u8,
    len: usize,
) -> *mut AmonhenSyntax {
    let path = str_arg(path).unwrap_or_default();
    let text = array!(text, len);
    Box::into_raw(Box::new(amonhen_core::diff::text_syntax(path, text))).cast()
}

/// # Safety
/// `syntax` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_syntax_view(syntax: *const AmonhenSyntax) -> AmonhenSyntaxView {
    let spans = handle!(syntax, Vec<SyntaxSpan>);
    AmonhenSyntaxView {
        spans: spans.as_ptr(),
        span_count: spans.len() as u32,
    }
}

/// # Safety
/// `syntax` は `amonhen_diff_syntax`・`amonhen_working_diff_syntax`・`amonhen_text_syntax` が返したもので、
/// まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_syntax_free(syntax: *mut AmonhenSyntax) {
    if !syntax.is_null() {
        drop(unsafe { Box::from_raw(syntax.cast::<Vec<SyntaxSpan>>()) });
    }
}

/// `history` の `row` 行目のコミット ID を `out`（20 バイト）に書く。範囲外なら false。
///
/// # Safety
/// `history` は有効なスナップショット、`out` は 20 バイト書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_commit_id(
    history_ptr: *const AmonhenHistory,
    row: u32,
    out: *mut u8,
) -> bool {
    let Some(out) = (unsafe { out.cast::<MaybeUninit<[u8; 20]>>().as_mut() }) else {
        set_error("NULL was passed as out");
        return false;
    };
    match history!(history_ptr).graph.ids().get(row as usize) {
        Some(id) => {
            out.write(id.0);
            true
        }
        None => false,
    }
}

/// status の 1 項目。文字列は `AmonhenStatusView::strings` の中の UTF-8。
#[repr(C)]
pub struct AmonhenStatusEntry {
    /// 0: 変更、1: 名前の変更、2: コンフリクト、3: untracked。
    pub kind: u8,
    /// git の XY。変更なしは `.`、untracked は `?`。
    pub index: u8,
    pub worktree: u8,
    pub submodule: u8,
    pub path_off: u32,
    pub path_len: u32,
    /// 名前を変えたときの元のパス。なければ `orig_path_len` が `u32::MAX`。
    pub orig_path_off: u32,
    pub orig_path_len: u32,
}

#[repr(C)]
pub struct AmonhenStatusView {
    pub generation: u64,
    pub entries: *const AmonhenStatusEntry,
    pub count: u32,
    pub strings: *const u8,
    /// チェックアウト中のブランチ。detached HEAD なら `branch_len` が `u32::MAX`。
    pub branch_off: u32,
    pub branch_len: u32,
    /// 上流ブランチ（`origin/main`）。なければ `upstream_len` が `u32::MAX`。
    pub upstream_off: u32,
    pub upstream_len: u32,
    pub ahead: u32,
    pub behind: u32,
}

struct StatusHandle {
    snapshot: Arc<StatusSnapshot>,
    entries: Vec<AmonhenStatusEntry>,
    strings: Vec<u8>,
    branch: (u32, u32),
    upstream: (u32, u32),
}

/// 作業ツリーと index の状態を読み直す（`amonhen_status_release` で手放す）。前回と同じなら同じ世代。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_status_load(repo_ptr: *const AmonhenRepo) -> *const AmonhenStatus {
    let snapshot = match repo!(repo_ptr).load_status() {
        Ok(s) => s,
        Err(e) => {
            set_error(e);
            return std::ptr::null();
        }
    };
    let mut strings = Vec::new();
    let mut put = |s: Option<&str>| match s {
        Some(s) => {
            let off = strings.len() as u32;
            strings.extend_from_slice(s.as_bytes());
            (off, s.len() as u32)
        }
        None => (0, u32::MAX),
    };
    let branch = put(snapshot.status.branch.as_deref());
    let upstream = put(snapshot.status.upstream.as_deref());
    let entries = snapshot
        .status
        .entries
        .iter()
        .map(|e| {
            let (path_off, path_len) = put(Some(&e.path));
            let (orig_path_off, orig_path_len) = put(e.orig_path.as_deref());
            AmonhenStatusEntry {
                kind: e.kind as u8,
                index: e.index,
                worktree: e.worktree,
                submodule: u8::from(e.submodule),
                path_off,
                path_len,
                orig_path_off,
                orig_path_len,
            }
        })
        .collect();
    Arc::into_raw(Arc::new(StatusHandle {
        snapshot,
        entries,
        strings,
        branch,
        upstream,
    }))
    .cast()
}

/// # Safety
/// `status` は `amonhen_status_load` が返したもので、まだ手放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_status_release(status: *const AmonhenStatus) {
    if !status.is_null() {
        drop(unsafe { Arc::from_raw(status.cast::<StatusHandle>()) });
    }
}

/// # Safety
/// `status` は有効であること。返したポインタは手放すまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_status_view(status: *const AmonhenStatus) -> AmonhenStatusView {
    let s = handle!(status, StatusHandle);
    AmonhenStatusView {
        generation: s.snapshot.generation,
        entries: s.entries.as_ptr(),
        count: s.entries.len() as u32,
        strings: s.strings.as_ptr(),
        branch_off: s.branch.0,
        branch_len: s.branch.1,
        upstream_off: s.upstream.0,
        upstream_len: s.upstream.1,
        ahead: s.snapshot.status.ahead,
        behind: s.snapshot.status.behind,
    }
}

/// `status` の `index` 番目のファイルの差分。`staged` なら HEAD と index、そうでなければ index と作業ツリー
/// （`amonhen_working_diff_free` で解放する）。`whitespace` は `amonhen_file_diff_context` と同じで、無視した差分は
/// 行ごとの stage・破棄に使えない。
///
/// # Safety
/// `repo` と `status` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff(
    repo_ptr: *const AmonhenRepo,
    status: *const AmonhenStatus,
    index: u32,
    staged: bool,
    whitespace: u8,
    encoding: AmonhenEncoding,
) -> *mut AmonhenWorkingDiff {
    let s = handle!(status, StatusHandle);
    let Some(entry) = s.snapshot.status.entries.get(index as usize) else {
        set_error(tr!("File index out of range"));
        return std::ptr::null_mut();
    };
    let whitespace = amonhen_core::diff::Whitespace::from_u8(whitespace);
    match repo!(repo_ptr).working_diff_ws(entry, staged, whitespace, &profile(encoding)) {
        Ok(d) => Box::into_raw(Box::new(d)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `status` の `index` 番目のファイルの、HEAD と作業ツリーの差分。変更の行ごとに、index に入っているかを
/// `DiffRow` の `flags`（`STAGED`）で示す（`amonhen_working_diff_free` で解放する）。index がその形で表せない
/// ファイルでは、`amonhen_last_error` を空にして NULL を返す（FE は `amonhen_working_diff` で 2 つに分けて見せる）。
///
/// # Safety
/// `repo` と `status` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_combined(
    repo_ptr: *const AmonhenRepo,
    status: *const AmonhenStatus,
    index: u32,
    encoding: AmonhenEncoding,
) -> *mut AmonhenWorkingDiff {
    let s = handle!(status, StatusHandle);
    let Some(entry) = s.snapshot.status.entries.get(index as usize) else {
        set_error(tr!("File index out of range"));
        return std::ptr::null_mut();
    };
    match repo!(repo_ptr).working_diff_combined(entry, &profile(encoding)) {
        Ok(Some(d)) => Box::into_raw(Box::new(d)).cast(),
        Ok(None) => {
            set_error("");
            std::ptr::null_mut()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// Working Copy の差分のファイルの改行（`amonhen_working_diff_line_endings`）。改行は 0: なし、1: LF、2: CRLF、
/// 3: 混在。`present` が false なら情報がない（stage 済みの差分・サブモジュール・消したファイル）。
#[repr(C)]
pub struct AmonhenLineEndings {
    pub present: bool,
    /// git（index）に入っている形。
    pub index: u8,
    pub worktree: u8,
    /// index の内容を checkout したときの形。
    pub checkout: u8,
    /// 見せる値打ちがある（改行を変換している、または `warning`）。
    pub notable: bool,
    /// 改行が混ざっている、または作業ツリーが CRLF なのに checkout すると LF になる。
    pub warning: bool,
}

/// # Safety
/// `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_line_endings(
    diff: *const AmonhenWorkingDiff,
) -> AmonhenLineEndings {
    let d = handle!(diff, WorkingDiff);
    match &d.line_endings {
        Some(le) => AmonhenLineEndings {
            present: true,
            index: le.index as u8,
            worktree: le.worktree as u8,
            checkout: le.checkout as u8,
            notable: le.notable(),
            warning: le.warning(),
        },
        None => AmonhenLineEndings {
            present: false,
            index: 0,
            worktree: 0,
            checkout: 0,
            notable: false,
            warning: false,
        },
    }
}

/// かかっている改行の変換（`text=auto eol=crlf`・`core.autocrlf=true` など。`amonhen_string_free` で解放する）。
/// 見せる値打ちがないとき・変換がないときは NULL。
///
/// # Safety
/// `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_line_ending_rule(
    diff: *const AmonhenWorkingDiff,
) -> *mut c_char {
    let d = handle!(diff, WorkingDiff);
    d.line_endings
        .as_ref()
        .filter(|le| !le.rule.is_empty())
        .and_then(|le| CString::new(le.rule.clone()).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// # Safety
/// `diff` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_view(
    diff: *const AmonhenWorkingDiff,
) -> AmonhenDiffView {
    let d = handle!(diff, WorkingDiff);
    diff_view(&d.view)
}

/// # Safety
/// `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_free(diff: *mut AmonhenWorkingDiff) {
    if !diff.is_null() {
        drop(unsafe { Box::from_raw(diff.cast::<WorkingDiff>()) });
    }
}

fn paths<'a>(paths: *const *const c_char, count: u32) -> Result<Vec<&'a str>, StrArgError> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if paths.is_null() {
        return Err(StrArgError::Null);
    }
    unsafe { std::slice::from_raw_parts(paths, count as usize) }
        .iter()
        .map(|&p| str_arg(p))
        .collect()
}

fn done(result: amonhen_model::Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            set_error(e);
            false
        }
    }
}

/// `paths` の変更（追加・削除を含む）を stage する。
///
/// # Safety
/// `repo` は有効なリポジトリ、`paths` は NUL 終端の文字列 `count` 個の配列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stage(
    repo_ptr: *const AmonhenRepo,
    paths_ptr: *const *const c_char,
    count: u32,
) -> bool {
    match paths(paths_ptr, count) {
        Ok(p) => done(repo!(repo_ptr).stage(&p)),
        Err(e) => done(Err(e.into())),
    }
}

/// `paths` を unstage する（index を HEAD の状態に戻す）。
///
/// # Safety
/// `repo` は有効なリポジトリ、`paths` は NUL 終端の文字列 `count` 個の配列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_unstage(
    repo_ptr: *const AmonhenRepo,
    paths_ptr: *const *const c_char,
    count: u32,
) -> bool {
    match paths(paths_ptr, count) {
        Ok(p) => done(repo!(repo_ptr).unstage(&p)),
        Err(e) => done(Err(e.into())),
    }
}

/// `diff` の `rows` 行目の変更だけを stage する（stage 済みの差分なら unstage する）。
///
/// # Safety
/// `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stage_rows(
    repo_ptr: *const AmonhenRepo,
    diff: *const AmonhenWorkingDiff,
    rows: *const u32,
    count: u32,
) -> bool {
    let d = handle!(diff, WorkingDiff);
    let rows: Vec<usize> = array!(rows, count).iter().map(|&r| r as usize).collect();
    done(repo!(repo_ptr).stage_rows(d, &rows))
}

/// `amonhen_working_diff_combined` の差分の `rows` 行目の変更を、`staged` なら index に入れ、そうでなければ外す。
///
/// # Safety
/// `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_set_rows_staged(
    repo_ptr: *const AmonhenRepo,
    diff: *const AmonhenWorkingDiff,
    rows: *const u32,
    count: u32,
    staged: bool,
) -> bool {
    let d = handle!(diff, WorkingDiff);
    let rows: Vec<usize> = array!(rows, count).iter().map(|&r| r as usize).collect();
    done(repo!(repo_ptr).set_rows_staged(d, &rows, staged))
}

/// stage した変更をコミットする。`amend` なら直前のコミットを作り直す。
///
/// # Safety
/// `repo` は有効なリポジトリ、`message` は NUL 終端の文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_commit(
    repo_ptr: *const AmonhenRepo,
    message: *const c_char,
    amend: bool,
) -> bool {
    match str_arg(message) {
        Ok(m) => done(repo!(repo_ptr).commit(m, amend)),
        Err(e) => done(Err(e.into())),
    }
}

/// JSON の命令（`{"op":"merge","rev":"topic"}` など）を実行し、JSON の結果
/// （`{"ok":true,"outcome":"done"}` か `{"ok":false,"error":"…"}`）を返す。結果は
/// `amonhen_string_free` で解放する。頻度の低い操作はすべてここを通す。
///
/// # Safety
/// `repo` は有効なリポジトリ、`command` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_command(
    repo_ptr: *const AmonhenRepo,
    command: *const c_char,
) -> *mut c_char {
    let response = match str_arg(command) {
        Ok(json) => repo!(repo_ptr).run_json(json),
        Err(e) => format!(r#"{{"ok":false,"error":"{e}"}}"#),
    };
    CString::new(response).unwrap_or_default().into_raw()
}

/// Activity の記録（git の起動とコマンドの実行、古い順、最大 1000 件）の JSON の配列。`amonhen_string_free` で
/// 解放する。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_activity() -> *mut c_char {
    CString::new(amonhen_core::activity_json())
        .unwrap_or_default()
        .into_raw()
}

/// 手元で走っている操作（Activity の記録の番号 `seq`）を取り消す。取り消せれば true。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_activity_cancel(seq: u64) -> bool {
    amonhen_core::cancel_activity(seq)
}

/// `url` を `dest`（まだないフォルダ）に clone する。`filter`（NULL・空でなければ `--filter`。`blob:none` など）、
/// `depth`（0 でなければ `--depth`）、`sparse`（`--sparse`。根のファイルだけを取り出す。git 2.27 から）で取得する
/// 範囲と取り出すものを決め、`branch`（NULL・空でなければ `-b`）で取り出すブランチを決める（`url` が HEAD のない
/// bundle なら、決めなくても main・master・最初のブランチ）。`recurse_submodules` なら submodule も取り出す
/// （`--recurse-submodules`）。失敗したら false（理由は `amonhen_last_error`）。
/// 時間がかかるので裏のスレッドで呼ぶ。
///
/// # Safety
/// `url` と `dest` は NUL 終端の UTF-8 文字列、`filter` と `branch` は NULL か NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_clone(
    url: *const c_char,
    dest: *const c_char,
    filter: *const c_char,
    depth: u32,
    sparse: bool,
    branch: *const c_char,
    recurse_submodules: bool,
) -> bool {
    let optional = |p: *const c_char| {
        if p.is_null() {
            Ok(None)
        } else {
            str_arg(p).map(|f| Some(f.to_owned()).filter(|f| !f.is_empty()))
        }
    };
    match (
        str_arg(url),
        str_arg(dest),
        optional(filter),
        optional(branch),
    ) {
        (Ok(url), Ok(dest), Ok(filter), Ok(branch)) => {
            let options = amonhen_backend_cli::CloneOptions {
                filter,
                depth: (depth > 0).then_some(depth),
                sparse,
                branch,
                recurse_submodules,
            };
            done(amonhen_backend_cli::clone(
                url,
                std::path::Path::new(dest),
                &options,
            ))
        }
        (Err(e), _, _, _) | (_, Err(e), _, _) | (_, _, Err(e), _) | (_, _, _, Err(e)) => {
            done(Err(e.into()))
        }
    }
}

/// `path` が bundle のファイルなら、その見出しの JSON（`refs`: 入っている ref、`head`: HEAD が入っているか、
/// `prerequisites`: 前提のコミットの数（あれば clone できない）、`branch`: HEAD がないとき取り出すブランチ）。
/// bundle でなければ NULL。解放は `amonhen_string_free`。
///
/// # Safety
/// `path` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_bundle_info(path: *const c_char) -> *mut c_char {
    let Ok(path) = str_arg(path) else {
        return std::ptr::null_mut();
    };
    let Some(info) = amonhen_backend_cli::bundle_info(std::path::Path::new(path)) else {
        return std::ptr::null_mut();
    };
    let json = serde_json::json!({
        "refs": info.refs,
        "head": info.head,
        "prerequisites": info.prerequisites,
        "branch": amonhen_backend_cli::bundle_default_branch(&info),
    });
    CString::new(json.to_string())
        .unwrap_or_default()
        .into_raw()
}

/// `path` のリポジトリの軽い状態（リポジトリの一覧に出す）。行に分けた文字列で、順にブランチ（detached なら
/// 空）、上流があれば 1、ahead、behind、変更のあるファイルの数（untracked も数える）。読めなければ NULL。
/// `amonhen_string_free` で解放する。git を 1 回起動するので、裏のスレッドで呼ぶ。
///
/// # Safety
/// `path` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_peek(path: *const c_char) -> *mut c_char {
    let Ok(path) = str_arg(path) else {
        return std::ptr::null_mut();
    };
    match amonhen_backend_cli::Git::new(path).peek() {
        Ok(p) => {
            // 7 行: ブランチ、上流があるか、先、後ろ、変更の数、最後のコミットの日時（UNIX 時間）、その件名。
            let (time, subject) = p
                .last_commit
                .map(|(t, s)| (t.to_string(), s))
                .unwrap_or_default();
            let text = format!(
                "{}\n{}\n{}\n{}\n{}\n{}\n{}",
                p.branch.unwrap_or_default(),
                u8::from(p.upstream),
                p.ahead,
                p.behind,
                p.changed,
                time,
                subject.replace('\n', " ")
            );
            CString::new(text).unwrap_or_default().into_raw()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `s` は `amonhen_command`・`amonhen_peek` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// `diff` の `rows` 行目の変更を作業ツリーから取り除く（Undo できる）。stage していない差分にだけ使える。
///
/// # Safety
/// `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_discard_rows(
    repo_ptr: *const AmonhenRepo,
    diff: *const AmonhenWorkingDiff,
    rows: *const u32,
    count: u32,
) -> bool {
    let d = handle!(diff, WorkingDiff);
    let rows: Vec<usize> = array!(rows, count).iter().map(|&r| r as usize).collect();
    done(repo!(repo_ptr).discard_rows(d, &rows))
}

/// 作業ツリーのルート（`amonhen_string_free` で解放する）。bare リポジトリなら NULL。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_workdir(repo_ptr: *const AmonhenRepo) -> *mut c_char {
    repo!(repo_ptr)
        .paths()
        .workdir
        .and_then(|p| CString::new(p.display().to_string()).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// 覚えていた署名の検証の結果を捨てる（ほかのリポジトリや Settings > Git・SSH Hosts で検証の設定 `gpg.*` を
/// 書いたとき。次に頼まれたら検証し直す）。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_forget_signatures(repo_ptr: *const AmonhenRepo) {
    repo!(repo_ptr).forget_signatures();
}

/// daemon 越しのリポジトリのコミットの署名を、手元の鍵で検証するか（false なら SSH 先の git と鍵で検証する）。
/// 手元のリポジトリには効かない（いつも手元で検証する）。変えると、覚えていた検証の結果は捨てる。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_set_local_signature_check(
    repo_ptr: *const AmonhenRepo,
    local: bool,
) {
    static VERIFIER: std::sync::OnceLock<Arc<amonhen_backend_cli::LocalVerifier>> =
        std::sync::OnceLock::new();
    let repo = repo!(repo_ptr);
    if repo.connection_state().is_none() {
        return;
    }
    let verifier = local.then(|| {
        let v = VERIFIER.get_or_init(|| {
            // コミットのオブジェクトだけを書く作業用のリポジトリ（同じ ID は 1 回だけ書かれる）。
            Arc::new(amonhen_backend_cli::LocalVerifier::new(
                std::env::temp_dir().join("amonhen-verify.git"),
            ))
        });
        v.clone() as Arc<dyn amonhen_model::SignatureVerifier>
    });
    repo.set_signature_verifier(verifier);
}

/// daemon とのつながりの様子。手元のリポジトリなら -1、つながっていれば 0、切れていれば 1 を返す。
/// `received`・`sent` が NULL でなければ、これまでに受け取った・送ったバイト数を書く。切れた理由は
/// `amonhen_repo_disconnect_reason` で読む。
///
/// # Safety
/// `repo` は有効なリポジトリ、`received`・`sent` は NULL か書き込める `u64` を指すこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_connection(
    repo_ptr: *const AmonhenRepo,
    received: *mut u64,
    sent: *mut u64,
) -> i32 {
    let Some(state) = repo!(repo_ptr).connection_state() else {
        return -1;
    };
    unsafe {
        put(received, state.received);
        put(sent, state.sent);
    }
    i32::from(state.closed.is_some())
}

/// SSH 先で使っているエンジンと、その用意のしかた（`amonhen_string_free` で解放する）。SSH のつながりでなければ NULL。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_remote_engine(repo_ptr: *const AmonhenRepo) -> *mut c_char {
    repo!(repo_ptr)
        .connection_state()
        .and_then(|s| s.engine)
        .and_then(|e| CString::new(e).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// daemon との接続が切れた理由（`amonhen_string_free` で解放する）。切れていなければ NULL。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_disconnect_reason(
    repo_ptr: *const AmonhenRepo,
) -> *mut c_char {
    repo!(repo_ptr)
        .connection_state()
        .and_then(|s| s.closed)
        .and_then(|r| CString::new(r.replace('\0', " ")).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// コンフリクトのある作業ツリーのファイルを、差分と同じ形の行にする（`amonhen_diff_free` で解放する）。
/// 行の `kind` は Current・Incoming・Base・マーカーを表し、コンフリクトの行の `old_no` はその番号 + 1。
///
/// # Safety
/// `repo` は有効なリポジトリ、`path` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_conflict_view(
    repo_ptr: *const AmonhenRepo,
    path: *const c_char,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    let repo = repo!(repo_ptr);
    let result = str_arg(path)
        .map_err(Into::into)
        .and_then(|p| repo.conflict_view(p, &profile(encoding)));
    match result {
        Ok(view) => Box::into_raw(Box::new(view)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `git range-diff` の差分の差分（`rangeDiff` の答えの `diff` の行を `\n` でつないだもの）を、差分と同じ形の行に
/// する（`amonhen_diff_free` で解放する）。外側の記号で行の種類を決め、文字列はそれぞれの版のパッチの行。
///
/// # Safety
/// `text` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_range_diff_view(
    text: *const c_char,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    match str_arg(text) {
        Ok(text) => {
            let lines: Vec<&str> = if text.is_empty() {
                Vec::new()
            } else {
                text.split('\n').collect()
            };
            let file = amonhen_core::diff::range_diff_file(&lines);
            Box::into_raw(Box::new(DiffView::new(&file, &profile(encoding)))).cast()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `tips`（20 バイトの ID が `count` 個）から辿れるコミットだけの History（ブランチで絞り込む）。
/// `amonhen_history_release` で手放す。
///
/// # Safety
/// `repo` は有効なリポジトリ、`tips` は `count * 20` バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_filtered(
    repo_ptr: *const AmonhenRepo,
    tips: *const u8,
    count: u32,
) -> *const AmonhenHistory {
    let tips = oids!(tips, count);
    match repo!(repo_ptr).history_filtered(&tips) {
        Ok(s) => into_history(s),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// マージ済みブランチを折りたたんだ History。`all` なら全ブランチ、そうでなければ `tips`（20 バイトの ID が
/// `count` 個）から辿れるものだけ。`folded` なら merge を既定でたたみ、`except`（20 バイトの ID が
/// `except_count` 個）の merge はその逆にする。`amonhen_history_release` で手放す。
///
/// # Safety
/// `repo` は有効なリポジトリ、`tips` は `count * 20` バイト、`except` は `except_count * 20` バイト読める
/// 領域であること（個数が 0 なら NULL でよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_folded(
    repo_ptr: *const AmonhenRepo,
    all: bool,
    tips: *const u8,
    count: u32,
    folded: bool,
    except: *const u8,
    except_count: u32,
) -> *const AmonhenHistory {
    let tips = if all { None } else { Some(oids!(tips, count)) };
    let except = oids!(except, except_count);
    match repo!(repo_ptr).history_folded(tips.as_deref(), folded, &except) {
        Ok(s) => into_history(s),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// 折りたたんだ History で、`row` の merge が隠しているコミットの数（隠していなければ 0）。
///
/// # Safety
/// `history` は有効な History であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_hidden(
    history_ptr: *const AmonhenHistory,
    row: u32,
) -> u32 {
    history!(history_ptr)
        .folded
        .get(row as usize)
        .copied()
        .unwrap_or(0)
}

/// shallow clone の History か（境界より古いコミットを取得していない）。
///
/// # Safety
/// `history` は有効な History であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_is_shallow(history_ptr: *const AmonhenHistory) -> bool {
    !history!(history_ptr).shallow.is_empty()
}

/// shallow clone の境界のコミット（親を取得していない）の行を、昇順に `out` へ最大 `capacity` 個書き、全体の
/// 個数を返す。全行を見るので、大きな History では裏のスレッドで呼ぶ。
///
/// # Safety
/// `history` は有効な History、`out` は `capacity` 個の `u32` を書ける領域であること（0 個なら NULL でよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_boundary_rows(
    history_ptr: *const AmonhenHistory,
    out: *mut u32,
    capacity: u32,
) -> u32 {
    let rows = history!(history_ptr).boundary_rows();
    let out = unsafe { out_slice(out, capacity as usize) };
    for (slot, row) in out.iter_mut().zip(&rows) {
        slot.write(*row);
    }
    rows.len() as u32
}

/// スクロールバーに描く密度。`out` に区間ごとの [頻度, 活動量]（0〜1）を `buckets * 2` 個書き、書いた区間の数を
/// 返す（行が区間より少なければ行の数）。
///
/// # Safety
/// `history` は有効な History、`out` は `buckets * 2` 個の float を書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_density(
    history_ptr: *const AmonhenHistory,
    buckets: u32,
    out: *mut f32,
) -> u32 {
    let density = history!(history_ptr).density(buckets as usize);
    let out = unsafe { out_slice(out, buckets as usize * 2) };
    for ([slot_f, slot_a], [f, a]) in out.as_chunks_mut::<2>().0.iter_mut().zip(&density) {
        slot_f.write(*f);
        slot_a.write(*a);
    }
    density.len() as u32
}

/// `history` の `rows`（行が `count` 個）のコミットだけの History（検索で一致したものだけを出す）。線は最初の親を
/// たどって次に一致したコミットへつなぐ。`amonhen_history_release` で手放す。
///
/// # Safety
/// `repo`・`history` は有効なもの、`rows` は `count` 個の u32 を読める領域であること（0 個なら NULL でよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_matching(
    repo_ptr: *const AmonhenRepo,
    history_ptr: *const AmonhenHistory,
    rows: *const u32,
    count: u32,
) -> *const AmonhenHistory {
    let rows = array!(rows, count);
    into_history(repo!(repo_ptr).history_matching(history!(history_ptr), rows))
}

/// 手元の共有の daemon の様子（JSON。`amonhen_string_free` で解放する）。動いていなければ NULL。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_daemon_status() -> *mut c_char {
    match amonhen_remote::local::control("status") {
        Ok(Some(json)) => CString::new(json).map_or(std::ptr::null_mut(), CString::into_raw),
        Ok(None) => std::ptr::null_mut(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// 手元の共有の daemon を止める。動いていなければ false。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_daemon_stop() -> bool {
    matches!(amonhen_remote::local::control("stop"), Ok(Some(_)))
}

/// 利用者に見せる文（エラーなど）の言語を決める（`ja`・`en` など。訳のない言語は英語）。アプリの画面と同じ言語を
/// 起動したときに渡す。daemon・SSH 先にも、つなぐときにこの言語を伝える。
///
/// # Safety
/// `language` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_set_language(language: *const c_char) {
    if let Ok(language) = str_arg(language) {
        amonhen_model::i18n::set_default(amonhen_model::i18n::Lang::parse(language));
    }
}

/// SSH 先へ送るエンジン（`amonhen-<os>-<arch>`）の置き場所（`.app` なら `Contents/Resources/engines`、開発中は
/// このビルドのチェックアウトの `dist/engine`。`amonhen_remote::engines::bundled_dir`）。`amonhen_string_free` で解放する。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_engines_dir() -> *mut c_char {
    let dir = std::env::current_exe()
        .map(|exe| amonhen_remote::engines::bundled_dir(&exe))
        .unwrap_or_default();
    CString::new(dir.display().to_string())
        .unwrap_or_default()
        .into_raw()
}

/// 開発中に共有の daemon にするエンジン（このビルドのチェックアウトの `target/release/amonhen`）。`.app` の中・
/// まだ作っていなければ NULL。`amonhen_string_free` で解放する。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_development_engine() -> *mut c_char {
    std::env::current_exe()
        .ok()
        .and_then(|exe| amonhen_remote::engines::development_engine(&exe))
        .and_then(|path| CString::new(path.display().to_string()).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// 環境変数 `name` を、git と ssh がいつも使うものとして、いつも渡すか（`amonhen_model::process::always_passed`）。
/// 設定の「環境変数」の表で、外せない行にする。
///
/// # Safety
/// `name` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_env_always(name: *const c_char) -> bool {
    str_arg(name).is_ok_and(amonhen_model::process::always_passed)
}

/// git と ssh にいつも渡す名前を改行で区切って（設定の「環境変数」に出す。`GIT_*` は `GIT_*` と書き、このアプリが
/// 決める `AMONHEN_*` は除く）。`amonhen_string_free` で解放する。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_env_always_names() -> *mut c_char {
    let names: Vec<&str> = amonhen_model::process::ALWAYS_PASSED
        .iter()
        .copied()
        .filter(|n| !n.starts_with("AMONHEN_"))
        .chain(["GIT_*"])
        .collect();
    CString::new(names.join("\n"))
        .unwrap_or_default()
        .into_raw()
}

/// 環境変数 `name` を渡さない理由（足しても断る）。渡してよければ NULL。`amonhen_string_free` で解放する。
///
/// # Safety
/// `name` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_env_refusal(name: *const c_char) -> *mut c_char {
    str_arg(name)
        .ok()
        .and_then(amonhen_model::process::env_refusal)
        .and_then(|reason| CString::new(reason).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// この機械で使う git と、それをどこで決めたか（JSON。コアの `machineGit` の `info` と同じ形）。この機械の
/// Amon Hen の設定（`settings.json`）を読むだけで、git は起動しない。`amonhen_string_free` で解放する。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_machine_git() -> *mut c_char {
    let json = serde_json::to_string(&amonhen_model::settings::machine_git()).unwrap_or_default();
    CString::new(json).unwrap_or_default().into_raw()
}

/// この機械の Amon Hen の設定（`settings.json`）に、使う git を書く。NULL・空なら消す（自動）。書けなければ理由、
/// 書けたら NULL。`amonhen_string_free` で解放する。
///
/// # Safety
/// `git` は NULL か、NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_set_machine_git(git: *const c_char) -> *mut c_char {
    let git = if git.is_null() {
        None
    } else {
        str_arg(git).ok()
    };
    match amonhen_model::settings::set_git(git) {
        Ok(()) => std::ptr::null_mut(),
        Err(e) => CString::new(e.to_string()).unwrap_or_default().into_raw(),
    }
}

/// このプロセスの環境が変わったとき（設定・シェルの読み直し）に、共有の daemon への今のつながりに送り直す
/// （`amonhen_remote::local::update_environment`。つなぎ直さずに、この先の git・ssh に効く）。通信するので裏の
/// スレッドで呼ぶ。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_update_environment() {
    amonhen_remote::local::update_environment();
}

/// このログインの SSH の agent のソケット（アプリが起動したときの `SSH_AUTH_SOCK`）。コアが起動する ssh・git に渡す
/// `SSH_AUTH_SOCK` が死んでいれば（前回の起動で読んだシェルの環境が古いなど）、代わりにこれを渡す。起動したときに 1 回。
///
/// # Safety
/// `path` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_set_ssh_agent(path: *const c_char) {
    if let Ok(path) = str_arg(path)
        && !path.is_empty()
    {
        let path = std::ffi::OsString::from(path);
        amonhen_model::process::set_agent_finder(move || Some(path.clone()));
    }
}

/// `message`（`amonhen_last_error` や操作の記録の文）が、取り消したことを表すか（どの言語の文でも）。
///
/// # Safety
/// `message` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_is_cancelled(message: *const c_char) -> bool {
    str_arg(message).is_ok_and(amonhen_model::i18n::is_cancelled)
}

/// 手元の共有の daemon を起動する（`daemon` は `amonhend` のパス）。失敗したら false（理由は `amonhen_last_error`）。
///
/// # Safety
/// `daemon` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_daemon_start(daemon: *const c_char) -> bool {
    let result = str_arg(daemon)
        .map_err(Into::into)
        .and_then(amonhen_remote::local::start);
    match result {
        Ok(()) => true,
        Err(e) => {
            set_error(e);
            false
        }
    }
}
