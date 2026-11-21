//! ツールバーのボタンの数のバッジ（windows-reactor の AppBarButton には中身を足す口がないので、ボタンの
//! テンプレートの根の Panel に InfoBadge を入れる）。

use crate::bindings::{
    AppBarButton, CommandBar, CommandBarDefaultLabelPosition, DependencyObject,
    HorizontalAlignment, InfoBadge, Thickness, UIElement, VerticalAlignment, Visibility,
    VisualTreeHelper,
};
use windows_collections::IVector;
use windows_core::{IInspectable, Interface, Result};

// Panel の Children（IPanel の最初の項目）。生成した UIElementCollection は IIterable の First を引けずに
// 通らないので、IPanel を手で引き、Children は IVector<UIElement> として受ける。
windows_core::imp::define_interface!(IPanel, IPanel_Vtbl, 0x27a1b418_56f3_525e_b883_cefed905eed3);
#[repr(C)]
#[doc(hidden)]
pub struct IPanel_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    children: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}

/// `panel` の子の並び。
fn children(panel: &IPanel) -> Result<IVector<UIElement>> {
    let mut out = core::ptr::null_mut();
    unsafe {
        (Interface::vtable(panel).children)(Interface::as_raw(panel), &mut out).ok()?;
        Ok(IVector::from_raw(out))
    }
}

/// `element` の下の AppBarButton のうち、見出しが `counts` にあるものにその数のバッジを付ける（0 なら隠す）。
pub fn put_badges(element: &IInspectable, counts: &[(String, u32)]) -> Result<()> {
    let mut stack = vec![element.cast::<DependencyObject>()?];
    while let Some(node) = stack.pop() {
        // タイトルバーのツールバーはアイコンだけ（名前はツールチップ）。ラベルがなければ、CommandBar はラベルを
        // 広げる ⋯ を出さず、入りきらないボタンがあるときだけ ⋯ を出す。
        if let Ok(bar) = node.cast::<CommandBar>() {
            bar.SetDefaultLabelPosition(CommandBarDefaultLabelPosition::Collapsed)?;
        }
        if let Ok(button) = node.cast::<AppBarButton>() {
            let label = String::from_utf16_lossy(&button.Label()?);
            if let Some((_, count)) = counts.iter().find(|(name, _)| *name == label) {
                badge(&button, *count)?;
            }
            continue;
        }
        for i in 0..VisualTreeHelper::GetChildrenCount(&node)? {
            stack.push(VisualTreeHelper::GetChild(&node, i)?);
        }
    }
    Ok(())
}

/// `button` のバッジを `count` にする（なければ作る）。テンプレートがまだなければ何もしない。
fn badge(button: &AppBarButton, count: u32) -> Result<()> {
    if VisualTreeHelper::GetChildrenCount(button)? == 0 {
        return Ok(());
    }
    let Ok(panel) = VisualTreeHelper::GetChild(button, 0)?.cast::<IPanel>() else {
        return Ok(());
    };
    let children = children(&panel)?;
    let found =
        (0..children.Size()?).find_map(|i| children.GetAt(i).ok()?.cast::<InfoBadge>().ok());
    let badge = match found {
        Some(badge) => badge,
        None if count == 0 => return Ok(()),
        None => {
            let badge = InfoBadge::new()?;
            badge.SetHorizontalAlignment(HorizontalAlignment::Right)?;
            badge.SetVerticalAlignment(VerticalAlignment::Top)?;
            badge.SetMargin(Thickness {
                Left: 0.0,
                Top: 2.0,
                Right: 4.0,
                Bottom: 0.0,
            })?;
            badge.SetIsHitTestVisible(false)?;
            children.Append(&badge.cast::<UIElement>()?)?;
            badge
        }
    };
    badge.SetValue(i32::try_from(count).unwrap_or(i32::MAX))?;
    badge.SetVisibility(if count == 0 {
        Visibility::Collapsed
    } else {
        Visibility::Visible
    })
}

/// `element` を押す・引くの対象から外す（上に重ねた背景画像の下の部品を押せるように）。
pub fn pass_through(element: &windows_core::IInspectable) -> windows_core::Result<()> {
    element.cast::<UIElement>()?.SetIsHitTestVisible(false)
}

/// `element` の窓（その窓の HWND の値。WinUI の窓では WindowId がそのまま HWND）。まだ窓に入っていなければ None。
pub fn window_handle(element: &IInspectable) -> Option<u64> {
    let root = element.cast::<UIElement>().ok()?.XamlRoot().ok()?;
    let id = root.ContentIslandEnvironment().ok()?.AppWindowId().ok()?;
    (id.Value != 0).then_some(id.Value)
}
