//! 窓を透かすときの、ボタン・文字の欄・選ぶ欄の地。WinUI の部品は自分の地（明るい外観では 7 割ほどの白）を塗るので、
//! 窓の地を薄くすると部品だけが目立つ。窓の根の Resources で部品の地の鍵を上書きし、地の色の濃さを窓の地の濃さに
//! 合わせる（メニュー・選ぶ欄の一覧は窓の外に開くので変えない）。

use crate::bindings::{Color, ElementTheme, FrameworkElement, SolidColorBrush};
use windows_collections::IMap;
use windows_core::{HSTRING, IInspectable, Interface, Result};

/// 上書きする鍵と、もとの色（明るい外観・暗い外観。WinUI の ControlFillColor* の値、ARGB）。
const KEYS: [(&str, u32, u32); 9] = [
    ("ButtonBackground", 0xB3FF_FFFF, 0x0FFF_FFFF),
    ("ButtonBackgroundPointerOver", 0x80F9_F9F9, 0x15FF_FFFF),
    ("ButtonBackgroundPressed", 0x4DF9_F9F9, 0x08FF_FFFF),
    ("TextControlBackground", 0xB3FF_FFFF, 0x0FFF_FFFF),
    ("TextControlBackgroundPointerOver", 0x80F9_F9F9, 0x15FF_FFFF),
    ("TextControlBackgroundFocused", 0xFFFF_FFFF, 0xB31E_1E1E),
    ("ComboBoxBackground", 0xB3FF_FFFF, 0x0FFF_FFFF),
    ("ComboBoxBackgroundPointerOver", 0x80F9_F9F9, 0x15FF_FFFF),
    ("ComboBoxBackgroundPressed", 0x4DF9_F9F9, 0x08FF_FFFF),
];

/// 窓の根に入れた部品の地。
pub struct ControlFills {
    element: FrameworkElement,
    brushes: Vec<(SolidColorBrush, u32, u32)>,
}

/// 外観を付ける（窓の根は外観を決めて付けているので、外観が変わったら付け直す）。
fn set_theme(element: &FrameworkElement, dark: bool) -> Result<()> {
    element.SetRequestedTheme(if dark {
        ElementTheme::Dark
    } else {
        ElementTheme::Light
    })
}

fn color(argb: u32, alpha: f32) -> Color {
    let a = ((argb >> 24) as f32 * alpha.clamp(0.0, 1.0)).round() as u8;
    Color {
        A: a,
        R: (argb >> 16) as u8,
        G: (argb >> 8) as u8,
        B: argb as u8,
    }
}

impl ControlFills {
    /// `root`（窓の根の部品）の Resources に部品の地を入れ、濃さを `alpha` 倍にする。入れた後に外観を付け直して、
    /// もう並んでいる部品にも当てる。
    pub fn install(root: &IInspectable, dark: bool, alpha: f32) -> Result<Self> {
        let element = root.cast::<FrameworkElement>()?;
        let resources = element
            .Resources()?
            .cast::<IMap<IInspectable, IInspectable>>()?;
        let mut brushes = Vec::new();
        for (key, light, dark_color) in KEYS {
            let base = if dark { dark_color } else { light };
            let brush = SolidColorBrush::CreateInstanceWithColor(color(base, alpha))?;
            let key: IInspectable = windows_reference::IReference::from(HSTRING::from(key)).into();
            resources.Insert(&key, &brush.cast::<IInspectable>()?)?;
            brushes.push((brush, light, dark_color));
        }
        // ThemeResource は外観が変わったときに引き直すので、外観を付け直して、入れた地を引かせる。
        set_theme(&element, !dark)?;
        set_theme(&element, dark)?;
        Ok(Self { element, brushes })
    }

    /// 外観と濃さを変える（入れた地の色をその場で替えるので、部品は引き直さなくてよい）。
    pub fn update(&self, dark: bool, alpha: f32) -> Result<()> {
        set_theme(&self.element, dark)?;
        for (brush, light, dark_color) in &self.brushes {
            brush.SetColor(color(if dark { *dark_color } else { *light }, alpha))?;
        }
        Ok(())
    }
}
