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
    "Microsoft.UI.Xaml.IFrameworkElement::{DataContext,Tag,put_HorizontalAlignment,put_VerticalAlignment,put_Margin}",
    // Repositories の一覧で選んだ行（複数）。
    "Microsoft.UI.Xaml.Controls.ListViewBase",
    "Microsoft.UI.Xaml.Controls.IListViewBase::{SelectedItems,DragItemsStarting,remove_DragItemsStarting}",
    // Repositories の行を、左のお気に入り・グループへ引いて入れる（引いた行の Tag を文字で載せる）。
    "Microsoft.UI.Xaml.Controls.DragItemsStartingEventArgs",
    "Microsoft.UI.Xaml.Controls.IDragItemsStartingEventArgs::{Items,Data,Cancel,put_Cancel}",
    "Microsoft.UI.Xaml.Controls.DragItemsStartingEventHandler",
    "Windows.ApplicationModel.DataTransfer.DataPackage",
    "Windows.ApplicationModel.DataTransfer.IDataPackage::{SetText,put_RequestedOperation}",
    "Windows.ApplicationModel.DataTransfer.DataPackageOperation",
    // 左のお気に入り・グループで、引いてきた行を受ける（reactor の Border の受け口は、外側の一覧が受けないに
    // 戻すので、一覧で直に受けて処理済みにする）。
    "Microsoft.UI.Xaml.DragEventArgs",
    "Microsoft.UI.Xaml.IDragEventArgs::{AcceptedOperation,put_AcceptedOperation,Handled,put_Handled}",
    "Microsoft.UI.Xaml.DragEventHandler",
    "Microsoft.UI.Xaml.Controls.IListViewBaseFactory::{}",
    "Microsoft.UI.Xaml.Controls.IListViewBaseStatics::{}",
    "Microsoft.UI.Xaml.Controls.IItemsControlFactory::{}",
    "Microsoft.UI.Xaml.Controls.IItemsControlStatics::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.ISelectorStatics::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.ISelectorFactory::{}",
    // サイドバーの木のドラッグ（WinUI の TreeView は、もとから項目を引いて別の項目の中・間へ動かせる）。お気に入りの
    // ブランチだけを引けるようにし、落とした後の並びをお気に入りの順にする。
    "Microsoft.UI.Xaml.Controls.TreeView",
    "Microsoft.UI.Xaml.Controls.ITreeView2::{DragItemsStarting,remove_DragItemsStarting,DragItemsCompleted,remove_DragItemsCompleted}",
    "Microsoft.UI.Xaml.Controls.ITreeViewFactory::{}",
    "Microsoft.UI.Xaml.Controls.ITreeViewStatics::{}",
    "Microsoft.UI.Xaml.Controls.ITreeViewStatics2::{}",
    "Microsoft.UI.Xaml.Controls.TreeViewDragItemsStartingEventArgs",
    "Microsoft.UI.Xaml.Controls.ITreeViewDragItemsStartingEventArgs::{Items,Cancel,put_Cancel}",
    "Microsoft.UI.Xaml.Controls.TreeViewDragItemsCompletedEventArgs",
    "Microsoft.UI.Xaml.Controls.ITreeViewDragItemsCompletedEventArgs::{Items}",
    "Microsoft.UI.Xaml.Controls.ITreeViewDragItemsCompletedEventArgs2::{NewParentItem}",
    "Microsoft.UI.Xaml.Controls.TreeViewNode",
    "Microsoft.UI.Xaml.Controls.ITreeViewNode::{Children}",
    "Microsoft.UI.Xaml.Controls.ITreeViewNodeFactory::{}",
    "Microsoft.UI.Xaml.Controls.ITreeViewNodeStatics::{}",
    // タブを窓の外へ引き出したら、そのリポジトリを新しい窓で開く（GTK4 版・macOS 版のタブの引き出し）。
    "Microsoft.UI.Xaml.Controls.TabView",
    "Microsoft.UI.Xaml.Controls.ITabView::{TabDroppedOutside,remove_TabDroppedOutside,put_CanDragTabs}",
    "Microsoft.UI.Xaml.Controls.ITabViewFactory::{}",
    "Microsoft.UI.Xaml.Controls.ITabViewStatics::{}",
    "Microsoft.UI.Xaml.Controls.TabViewTabDroppedOutsideEventArgs",
    "Microsoft.UI.Xaml.Controls.ITabViewTabDroppedOutsideEventArgs::{Tab}",
    "Microsoft.UI.Xaml.Controls.TabViewItem",
    "Microsoft.UI.Xaml.Controls.ITabViewItemFactory::{}",
    "Microsoft.UI.Xaml.Controls.ITabViewItemStatics::{}",
    "Microsoft.UI.Xaml.Controls.ITabViewStatics2::{}",
    "Microsoft.UI.Xaml.Controls.IListViewItemFactory::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.ISelectorItemFactory::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.ISelectorItemStatics::{}",
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
    // ツールバーの Pull・Push のボタンに、上流との差の数のバッジ（InfoBadge）を足す（reactor の AppBarButton には
    // 中身を足す口がないので、ボタンのテンプレートの根の Panel に入れる。Panel.Children は UIElementCollection
    // を返し、その生成物は IIterable の First を引けずに通らないので、`badge.rs` で IPanel を手で引く）。
    "Microsoft.UI.Xaml.Controls.InfoBadge",
    "Microsoft.UI.Xaml.Controls.IInfoBadge::{Value,put_Value}",
    "Microsoft.UI.Xaml.Controls.IInfoBadgeFactory::{CreateInstance}",
    "Microsoft.UI.Xaml.Controls.IInfoBadgeStatics::{}",
    "Microsoft.UI.Xaml.Controls.AppBarButton",
    "Microsoft.UI.Xaml.Controls.IAppBarButton::{Label}",
    "Microsoft.UI.Xaml.Controls.IAppBarButtonFactory::{}",
    "Microsoft.UI.Xaml.Controls.IAppBarButtonStatics::{}",
    "Microsoft.UI.Xaml.Controls.IButtonFactory::{}",
    "Microsoft.UI.Xaml.Controls.IButtonStatics::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.IButtonBaseFactory::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.IButtonBaseStatics::{}",
    "Microsoft.UI.Xaml.Controls.IContentControlFactory::{}",
    "Microsoft.UI.Xaml.Controls.IContentControlStatics::{}",
    "Microsoft.UI.Xaml.HorizontalAlignment",
    "Microsoft.UI.Xaml.VerticalAlignment",
    "Microsoft.UI.Xaml.Thickness",
    "Microsoft.UI.Xaml.Visibility",
    // マージエディタの Result の欄（TextBox）を、選んだコンフリクトの行へスクロールする（中の ScrollViewer を動かす）。
    "Microsoft.UI.Xaml.Controls.ScrollViewer",
    "Microsoft.UI.Xaml.Controls.IScrollViewer::{ExtentHeight,ViewportHeight,ScrollToVerticalOffset}",
    "Microsoft.UI.Xaml.Controls.IScrollViewerStatics::{}",
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
     IsTabStop,put_IsTabStop,Focus,CapturePointer,ReleasePointerCapture,ContextFlyout,put_ContextFlyout,PreviewKeyDown,remove_PreviewKeyDown,\
     StartBringIntoView,put_IsHitTestVisible,put_Visibility,DragOver,remove_DragOver,Drop,remove_Drop,put_AllowDrop}",
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
    // 窓の後ろを透かす Acrylic を自分で当てる（Window.SystemBackdrop の Acrylic は色の層の濃さを変えられず、透過率を
    // 上げても透けにくい）。色の層は 0 にして、透け具合はアプリが塗る地の濃さで決める。
    "Microsoft.UI.Composition.SystemBackdrops.DesktopAcrylicController",
    "Microsoft.UI.Composition.SystemBackdrops.IDesktopAcrylicController::{put_TintOpacity,put_LuminosityOpacity}",
    "Microsoft.UI.Composition.SystemBackdrops.ISystemBackdropControllerWithTargets::{AddSystemBackdropTarget,\
     RemoveAllSystemBackdropTargets,SetSystemBackdropConfiguration}",
    "Microsoft.UI.Composition.SystemBackdrops.SystemBackdropConfiguration",
    "Microsoft.UI.Composition.SystemBackdrops.ISystemBackdropConfiguration::{put_IsInputActive,put_Theme}",
    "Microsoft.UI.Composition.SystemBackdrops.SystemBackdropTheme",
    "Microsoft.UI.Composition.ICompositionSupportsSystemBackdrop",
    "Microsoft.UI.Composition.SystemBackdrops.IDesktopAcrylicControllerStatics::{}",
    "Microsoft.UI.Composition.ICompositionObjectStatics::{}",
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
