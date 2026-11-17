//! WinUI 3 のバインディング（`xaml/src/bindings.rs`）を作る。引数は Windows App SDK の実行環境のディレクトリ
//! （winmd が並んでいる所）。

use std::path::{Path, PathBuf};

/// 要る型とメソッド。クラスは名前だけ、メソッドはインターフェースに `::{..}` で並べる（並べないと
/// メソッドは出ない）。足したら作り直す。
const FILTER: &[&str] = &[
    "Microsoft.UI.Xaml.UIElement",
    "Microsoft.UI.Xaml.RoutedEventArgs",
    "Microsoft.UI.Xaml.Input.KeyRoutedEventArgs",
    "Microsoft.UI.Xaml.Input.PointerRoutedEventArgs",
    "Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs",
    "Microsoft.UI.Xaml.Input.ContextRequestedEventArgs",
    "Microsoft.UI.Input.PointerPoint",
    "Microsoft.UI.Input.PointerPointProperties",
    "Microsoft.UI.Input.InputKeyboardSource",
    "Microsoft.UI.Xaml.FrameworkElement",
    "Microsoft.UI.Xaml.Controls.Primitives.FlyoutBase",
    "Microsoft.UI.Xaml.Controls.Primitives.FlyoutShowOptions",
    // サイドバーの右クリックで、押した所から親をたどって木の項目（DataContext が TreeViewNode）を探す。
    "Microsoft.UI.Xaml.DependencyObject",
    "Microsoft.UI.Xaml.Media.VisualTreeHelper",
    "Microsoft.UI.Xaml.Media.IVisualTreeHelperStatics::{GetParent,GetChild,GetChildrenCount}",
    "Microsoft.UI.Xaml.IFrameworkElement::{DataContext,Tag}",
    // Repositories の一覧で選んだ行（複数）。
    "Microsoft.UI.Xaml.Controls.ListViewBase",
    "Microsoft.UI.Xaml.Controls.IListViewBase::{SelectedItems}",
    "Microsoft.UI.Xaml.Controls.IListViewBaseFactory::{}",
    "Microsoft.UI.Xaml.Controls.IListViewBaseStatics::{}",
    "Microsoft.UI.Xaml.Controls.IItemsControlFactory::{}",
    "Microsoft.UI.Xaml.Controls.IItemsControlStatics::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.ISelectorStatics::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.ISelectorFactory::{}",
    // 等幅の字（Control に付けると、中の TextBlock が継ぐ。reactor の部品には字体の指定がない）。
    "Microsoft.UI.Xaml.Media.FontFamily",
    "Microsoft.UI.Xaml.Media.IFontFamilyFactory::{CreateInstanceWithName}",
    "Microsoft.UI.Xaml.Media.IFontFamilyStatics::{}",
    "Microsoft.UI.Xaml.Controls.Control",
    "Microsoft.UI.Xaml.Controls.IControlFactory::{}",
    "Microsoft.UI.Xaml.Controls.IControlStatics::{}",
    "Microsoft.UI.Xaml.Controls.IControl::{put_FontFamily}",
    // 境目のつまみの、左右・上下の矢印のカーソル（ProtectedCursor は派生した部品向けだが、インターフェースを
    // 引けば外から付けられる）。
    "Microsoft.UI.Xaml.IUIElementProtected::{put_ProtectedCursor}",
    "Microsoft.UI.Input.InputCursor",
    "Microsoft.UI.Input.InputSystemCursor",
    "Microsoft.UI.Input.IInputSystemCursorStatics::{Create}",
    "Microsoft.UI.Input.InputSystemCursorShape",
    "Microsoft.UI.Input.IInputCursorFactory::{}",
    "Microsoft.UI.Input.IInputCursorStatics::{}",
    // ハンドラーの型は丸ごと（`new` が要る）。
    "Microsoft.UI.Xaml.Input.KeyEventHandler",
    "Microsoft.UI.Xaml.Input.PointerEventHandler",
    "Microsoft.UI.Xaml.Input.DoubleTappedEventHandler",
    "Windows.Foundation.TypedEventHandler",
    // クラスが参照する、使わないインターフェースは名前だけ。
    "Microsoft.UI.Xaml.IDependencyObjectFactory::{}",
    "Microsoft.UI.Xaml.IRoutedEventArgsFactory::{}",
    "Microsoft.UI.Xaml.IUIElementStatics::{}",
    "Microsoft.UI.Input.IPointerPointStatics::{}",
    "Microsoft.UI.Input.IInputKeyboardSourceStatics2::{}",
    "Microsoft.UI.Input.IInputKeyboardSourceStatics3::{}",
    "Microsoft.UI.Xaml.IFrameworkElementFactory::{}",
    "Microsoft.UI.Xaml.IFrameworkElementStatics::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBaseFactory::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBaseStatics::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBaseStatics2::{}",
    // 右クリックのメニューを開く位置を渡すために作る。
    "Microsoft.UI.Xaml.Controls.Primitives.IFlyoutShowOptionsFactory::{CreateInstance}",
    "Microsoft.UI.Xaml.IUIElement::{KeyDown,remove_KeyDown,PointerPressed,remove_PointerPressed,\
     PointerMoved,remove_PointerMoved,PointerReleased,remove_PointerReleased,PointerWheelChanged,\
     remove_PointerWheelChanged,DoubleTapped,remove_DoubleTapped,ContextRequested,remove_ContextRequested,\
     IsTabStop,put_IsTabStop,Focus,CapturePointer,ReleasePointerCapture,ContextFlyout,put_ContextFlyout,PreviewKeyDown,remove_PreviewKeyDown}",
    "Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBase::{ShowAt,ShowAtWithOptions,Hide}",
    "Microsoft.UI.Xaml.Controls.Primitives.IFlyoutShowOptions::{put_Position}",
    "Microsoft.UI.Xaml.IRoutedEventArgs::{OriginalSource}",
    "Microsoft.UI.Xaml.Input.IKeyRoutedEventArgs::{Key,OriginalKey,Handled,put_Handled}",
    "Microsoft.UI.Xaml.Input.IPointerRoutedEventArgs::{GetCurrentPoint,KeyModifiers,Pointer,Handled,put_Handled}",
    "Microsoft.UI.Xaml.Input.IDoubleTappedRoutedEventArgs::{GetPosition,Handled,put_Handled}",
    "Microsoft.UI.Xaml.Input.IContextRequestedEventArgs::{TryGetPosition,Handled,put_Handled}",
    "Microsoft.UI.Input.IPointerPoint::{Position,Properties,Timestamp}",
    "Microsoft.UI.Input.IPointerPointProperties::{MouseWheelDelta,IsHorizontalMouseWheel,\
     IsLeftButtonPressed,IsRightButtonPressed,IsMiddleButtonPressed}",
    "Microsoft.UI.Input.IInputKeyboardSourceStatics::{GetKeyStateForCurrentThread}",
];

fn main() {
    let runtime = std::env::args()
        .nth(1)
        .expect("Windows App SDK の実行環境のディレクトリを渡す");
    let runtime = Path::new(&runtime);
    let out: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.rs");
    let mut args: Vec<String> = vec!["--in".into(), "default".into()];
    for name in [
        "Microsoft.UI.Xaml.winmd",
        "Microsoft.UI.winmd",
        "Microsoft.Foundation.winmd",
    ] {
        args.push(runtime.join(name).display().to_string());
    }
    args.extend([
        "--out".into(),
        out.display().to_string(),
        "--flat".into(),
        "--filter".into(),
    ]);
    args.extend(
        FILTER
            .iter()
            .map(|t| t.split_whitespace().collect::<String>()),
    );
    windows_bindgen::bindgen(&args);
}
