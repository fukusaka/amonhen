//! 押す・引くの対象から部品を外す（上に重ねた背景画像の下の部品を押せるように）。

use crate::bindings::UIElement;
use windows_core::Interface;

/// `element` を押す・引くの対象から外す（上に重ねた背景画像の下の部品を押せるように）。
pub fn pass_through(element: &windows_core::IInspectable) -> windows_core::Result<()> {
    element.cast::<UIElement>()?.SetIsHitTestVisible(false)
}
