//! アプリの外観（明るい・暗い）と、外観ごとの色。外観は Windows の設定（個人用設定 > 色 > アプリ モード）に合わせる
//! （macOS 版・GTK4 版と同じく、アプリに切り替えはない）。変わったら描き直す（`refresh` を定期的に呼ぶ）。

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use windows_reactor::Color;

static DARK: AtomicBool = AtomicBool::new(false);
/// 中身の地の濃さ（窓全体を透かすときは `1 - 透過率`。f32 のビット）と、変わったか（描き直す）。
static GROUND: AtomicU32 = AtomicU32::new(0x3f80_0000);
static GROUND_CHANGED: AtomicBool = AtomicBool::new(false);

/// 中身の地の濃さを決める（窓全体を透かすとき 1 より小さい）。変わったら次のフレームで描き直す。
pub fn set_ground(alpha: f32) {
    let bits = alpha.clamp(0.0, 1.0).to_bits();
    if GROUND.swap(bits, Ordering::Relaxed) != bits {
        GROUND_CHANGED.store(true, Ordering::Relaxed);
    }
}

/// 中身の地の濃さ。
pub fn ground() -> f32 {
    f32::from_bits(GROUND.load(Ordering::Relaxed))
}

/// 地の濃さが変わったか（読んだら戻す）。
pub fn take_ground_changed() -> bool {
    GROUND_CHANGED.swap(false, Ordering::Relaxed)
}

/// `color` を地の濃さで薄くする。
fn grounded(color: Color) -> Color {
    Color::argb(
        (f32::from(color.a) * ground()).round() as u8,
        color.r,
        color.g,
        color.b,
    )
}
/// 試しで外観を決めておく（`--appearance light|dark`。0 は Windows に合わせる、1 は明るい、2 は暗い）。
static FORCED: AtomicU8 = AtomicU8::new(0);

/// 暗い外観か。
pub fn dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// 外観を決めておく（試し用。Windows の設定を変えずに暗い外観を見る）。
pub fn force(dark: bool) {
    FORCED.store(if dark { 2 } else { 1 }, Ordering::Relaxed);
    DARK.store(dark, Ordering::Relaxed);
}

/// Windows の設定を読み直す。変わっていれば true（構文の色のテーマも外観ごとなので読み直す）。
pub fn refresh() -> bool {
    let dark = match FORCED.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => system_dark(),
    };
    let changed = DARK.swap(dark, Ordering::Relaxed) != dark;
    if changed {
        crate::theme::reload();
    }
    changed
}

/// アプリ モードが暗いか（`AppsUseLightTheme` が 0）。読めなければ明るい。
fn system_dark() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;
    let mut value = 1u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    read.is_ok() && value == 0
}

fn pick(light: Color, dark: Color) -> Color {
    if self::dark() { dark } else { light }
}

/// 窓の地。
pub fn window() -> Color {
    pick(Color::rgb(0xff, 0xff, 0xff), Color::rgb(0x20, 0x20, 0x20))
}

/// 中身の地（History・差分・一覧。窓全体を透かすときは地の濃さで薄く）。
pub fn background() -> Color {
    grounded(pick(
        Color::rgb(0xff, 0xff, 0xff),
        Color::rgb(0x1e, 0x1e, 0x1e),
    ))
}

/// 帯の地（ステータスバー・見出し。窓全体を透かすときは地の濃さで薄く）。
pub fn bar() -> Color {
    grounded(pick(
        Color::rgb(0xf3, 0xf3, 0xf3),
        Color::rgb(0x2b, 0x2b, 0x2b),
    ))
}

/// 少し濃い帯の地（設定の見出し・選んだ行。窓全体を透かすときは地の濃さで薄く）。
pub fn bar_strong() -> Color {
    grounded(pick(
        Color::rgb(0xec, 0xec, 0xec),
        Color::rgb(0x33, 0x33, 0x33),
    ))
}

/// 少し薄い帯の地（設定の欄・交互の行）。
pub fn bar_soft() -> Color {
    pick(Color::rgb(0xf6, 0xf6, 0xf6), Color::rgb(0x26, 0x26, 0x26))
}

/// 区切りの線。
pub fn border() -> Color {
    pick(Color::rgb(0xe5, 0xe5, 0xe5), Color::rgb(0x3c, 0x3c, 0x3c))
}

/// 本文の文字。
pub fn foreground() -> Color {
    pick(Color::rgb(0x1f, 0x1f, 0x1f), Color::rgb(0xe8, 0xe8, 0xe8))
}

/// 控えめな文字（日時・パス）。
pub fn secondary() -> Color {
    pick(
        Color::argb(0x99, 0x1c, 0x1c, 0x1f),
        Color::argb(0x99, 0xf0, 0xf0, 0xf0),
    )
}

/// 薄い文字（数・注記）。
pub fn faint() -> Color {
    pick(
        Color::argb(0xa0, 0, 0, 0),
        Color::argb(0xa0, 0xff, 0xff, 0xff),
    )
}

/// 知らせの帯の地（黄色。続きの操作・注意）。
pub fn note_band() -> Color {
    pick(Color::rgb(0xff, 0xf4, 0xce), Color::rgb(0x43, 0x3a, 0x19))
}

/// 失敗の帯の地（赤）。
pub fn failure_band() -> Color {
    pick(Color::rgb(0xfd, 0xe7, 0xe9), Color::rgb(0x44, 0x27, 0x26))
}

/// 終わった操作の帯の地。
pub fn done_band() -> Color {
    pick(Color::rgb(0xf0, 0xf0, 0xf0), Color::rgb(0x2b, 0x2b, 0x2b))
}

/// 注意の文字（橙）。暗い地では明るく。
pub fn warning_text() -> Color {
    pick(Color::rgb(0xc4, 0x6a, 0x00), Color::rgb(0xff, 0xa8, 0x40))
}

/// 差分の地（追加・削除・hunk の見出し。Range Diff の行）。
pub fn diff_added() -> Color {
    pick(Color::rgb(0xe6, 0xf4, 0xea), Color::rgb(0x1f, 0x3a, 0x26))
}

pub fn diff_removed() -> Color {
    pick(Color::rgb(0xfb, 0xe9, 0xeb), Color::rgb(0x45, 0x23, 0x25))
}

pub fn diff_hunk() -> Color {
    pick(Color::rgb(0xee, 0xf1, 0xf8), Color::rgb(0x25, 0x2c, 0x3a))
}
