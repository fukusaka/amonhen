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
    "Microsoft.UI.Xaml.IUIElement::{KeyDown,remove_KeyDown,PointerPressed,remove_PointerPressed,\
     PointerMoved,remove_PointerMoved,PointerReleased,remove_PointerReleased,PointerWheelChanged,\
     remove_PointerWheelChanged,DoubleTapped,remove_DoubleTapped,ContextRequested,remove_ContextRequested,\
     IsTabStop,put_IsTabStop,Focus,CapturePointer,ReleasePointerCapture}",
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
