//! 押す・引くの対象から部品を外す（上に重ねた背景画像の下の部品を押せるように）、部品の窓。

use crate::bindings::UIElement;
use windows_core::{IInspectable, Interface};

/// `element` を押す・引くの対象から外す（上に重ねた背景画像の下の部品を押せるように）。
pub fn pass_through(element: &IInspectable) -> windows_core::Result<()> {
    element.cast::<UIElement>()?.SetIsHitTestVisible(false)
}

/// `element` の窓（その窓の HWND の値。WinUI の窓では WindowId がそのまま HWND）。まだ窓に入っていなければ None。
pub fn window_handle(element: &IInspectable) -> Option<u64> {
    let root = element.cast::<UIElement>().ok()?.XamlRoot().ok()?;
    let id = root.ContentIslandEnvironment().ok()?.AppWindowId().ok()?;
    (id.Value != 0).then_some(id.Value)
}
