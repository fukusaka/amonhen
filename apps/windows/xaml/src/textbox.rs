//! TextBox の選択範囲（`generate/` の型に入れていない分を手で書く）。`ITextBox` の vtable の先頭から
//! SelectionStart の設定までだけを並べる（並びは WinUI 3 の `Microsoft.UI.Xaml.Controls.ITextBox` と同じ）。

use windows_core::Interface as _;

windows_core::imp::define_interface!(
    ITextBoxSelection,
    ITextBoxSelection_Vtbl,
    0x873af7c2_ab89_5d76_8dbe_3d6325669df5
);

#[repr(C)]
pub struct ITextBoxSelection_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SetText: usize,
    SelectedText: usize,
    SetSelectedText: usize,
    SelectionLength: usize,
    pub SetSelectionLength:
        unsafe extern "system" fn(*mut core::ffi::c_void, i32) -> windows_core::HRESULT,
    SelectionStart: usize,
    pub SetSelectionStart:
        unsafe extern "system" fn(*mut core::ffi::c_void, i32) -> windows_core::HRESULT,
}

/// TextBox（の生のオブジェクト）の文字をすべて選ぶ（入れてある名前を打ち替えやすく。GTK4 版・macOS 版と同じ）。
pub fn select_all(text_box: &windows_core::IInspectable) -> windows_core::Result<()> {
    let text_box: ITextBoxSelection = text_box.cast()?;
    // SAFETY: 上の IID の vtable の並びどおりの位置を、そのオブジェクトの生のポインタで呼ぶ。Text の答えは
    // HSTRING の持ち主ごと受け取る。
    unsafe {
        let mut raw = core::ptr::null_mut();
        (text_box.vtable().Text)(text_box.as_raw(), &mut raw).ok()?;
        let text: windows_core::HSTRING = core::mem::transmute(raw);
        (text_box.vtable().SetSelectionStart)(text_box.as_raw(), 0).ok()?;
        (text_box.vtable().SetSelectionLength)(text_box.as_raw(), text.len() as i32).ok()
    }
}

/// `element`（TextBox）を包む ScrollViewer を、中身の `fraction`（0〜1）の所が上から 3 分の 1 あたりに来るように
/// スクロールする（マージエディタの Result を、選んだコンフリクトへ。欄は中身の高さに伸び、外の ScrollViewer が
/// 動く）。包む ScrollViewer がなければ何もしない。
pub fn scroll_to_fraction(element: &windows_core::IInspectable, fraction: f64) {
    use crate::bindings::{DependencyObject, ScrollViewer, VisualTreeHelper};
    let Ok(mut node) = element.cast::<DependencyObject>() else {
        return;
    };
    while let Ok(parent) = VisualTreeHelper::GetParent(&node) {
        if let Ok(viewer) = parent.cast::<ScrollViewer>() {
            let (Ok(extent), Ok(viewport)) = (viewer.ExtentHeight(), viewer.ViewportHeight())
            else {
                return;
            };
            let offset = (extent * fraction.clamp(0.0, 1.0) - viewport / 3.0).max(0.0);
            let _ = viewer.ScrollToVerticalOffset(offset);
            return;
        }
        node = parent;
    }
}
