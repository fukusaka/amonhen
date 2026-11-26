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
    "Microsoft.UI.Xaml.IFrameworkElement::{DataContext,Tag,ActualWidth,ActualHeight,put_HorizontalAlignment,put_VerticalAlignment,put_Margin,put_Width,put_Height,put_MaxWidth,put_MaxHeight,\
     Resources,RequestedTheme,put_RequestedTheme,SizeChanged,remove_SizeChanged}",
    // マージエディタの帯: 幅が変わったら、入りきらないボタンを後回しのものから隠す（macOS 版と同じ）。
    "Microsoft.UI.Xaml.SizeChangedEventHandler",
    "Microsoft.UI.Xaml.SizeChangedEventArgs",
    "Windows.Foundation.Size",
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
    "Microsoft.UI.Xaml.IDragEventArgs::{AcceptedOperation,put_AcceptedOperation,Handled,put_Handled,GetPosition}",
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
    "Microsoft.UI.Xaml.Controls.ITabView::{TabDroppedOutside,remove_TabDroppedOutside,put_CanDragTabs,\
     TabDragStarting,remove_TabDragStarting,TabStripDragOver,remove_TabStripDragOver,TabStripDrop,\
     remove_TabStripDrop,put_AllowDropTabs,TabDragCompleted,remove_TabDragCompleted}",
    "Microsoft.UI.Xaml.Controls.TabViewTabDragStartingEventArgs",
    // タブを別の窓のタブの並びへ引いて移す（引き始めたタブを覚えておく。窓はどれも同じプロセス）。
    "Microsoft.UI.Xaml.Controls.ITabViewTabDragStartingEventArgs::{Tab}",
    "Microsoft.UI.Xaml.Controls.TabViewTabDragCompletedEventArgs",
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
    "Microsoft.UI.Xaml.Controls.IControl::{put_FontFamily,put_HorizontalContentAlignment}",
    // 境目のつまみの、左右・上下の矢印のカーソル（ProtectedCursor は派生した部品向けだが、インターフェースを
    // 引けば外から付けられる）。
    "Microsoft.UI.Xaml.IUIElementProtected::{put_ProtectedCursor}",
    "Microsoft.UI.Input.InputCursor",
    "Microsoft.UI.Input.InputSystemCursor",
    "Microsoft.UI.Input.IInputSystemCursorStatics::{Create}",
    "Microsoft.UI.Input.InputSystemCursorShape",
    "Microsoft.UI.Input.IInputCursorFactory::{}",
    "Microsoft.UI.Input.IInputCursorStatics::{}",
    "Microsoft.UI.Xaml.Controls.IButtonFactory::{}",
    "Microsoft.UI.Xaml.Controls.IButtonStatics::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.IButtonBaseFactory::{}",
    "Microsoft.UI.Xaml.Controls.Primitives.IButtonBaseStatics::{}",
    "Microsoft.UI.Xaml.Controls.IContentControlFactory::{}",
    // サイドバーの行（TreeViewItem）の中身を、名前と右端の色の印に差し替える。
    "Microsoft.UI.Xaml.Controls.IContentControl::{Content,put_Content,put_ContentTemplate}",
    "Microsoft.UI.Xaml.DataTemplate",
    "Microsoft.UI.Xaml.IDataTemplateFactory::{}",
    "Microsoft.UI.Xaml.IDataTemplateStatics::{}",
    "Microsoft.UI.Xaml.IFrameworkTemplateFactory::{}",
    "Microsoft.UI.Xaml.Controls.IContentControlStatics::{}",
    "Microsoft.UI.Xaml.HorizontalAlignment",
    "Microsoft.UI.Xaml.VerticalAlignment",
    "Microsoft.UI.Xaml.Thickness",
    "Microsoft.UI.Xaml.Visibility",
    // サイドバーの行の中身（名前と、右端の色の印）を XAML から作る。
    "Microsoft.UI.Xaml.Markup.XamlReader",
    "Microsoft.UI.Xaml.Markup.IXamlReaderStatics::{Load}",
    "Microsoft.UI.Xaml.Markup.IXamlReader::{}",
    // Push の確認（ContentDialog）で、Enter で押すボタン（Push は色付き、Force Push は色なし）。
    "Microsoft.UI.Xaml.Controls.ContentDialog",
    "Microsoft.UI.Xaml.Controls.IContentDialog::{put_DefaultButton}",
    "Microsoft.UI.Xaml.Controls.ContentDialogButton",
    "Microsoft.UI.Xaml.Controls.IContentDialogFactory::{}",
    "Microsoft.UI.Xaml.Controls.IContentDialogStatics::{}",
    // マージエディタの Result の欄（TextBox）を、選んだコンフリクトの行へスクロールする（中の ScrollViewer を動かす）。
    "Microsoft.UI.Xaml.Controls.ScrollViewer",
    "Microsoft.UI.Xaml.Controls.IScrollViewer::{ExtentHeight,ViewportHeight,ViewportWidth,ScrollToVerticalOffset}",
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
     StartBringIntoView,StartBringIntoViewWithOptions,put_IsHitTestVisible,Visibility,put_Visibility,Measure,DesiredSize,DragOver,remove_DragOver,Drop,remove_Drop,put_AllowDrop,XamlRoot}",
    // 部品がどの窓のものか（窓の HWND。前に出ている窓の前のタブが、窓の入力の部品を持つ）。
    "Microsoft.UI.Xaml.XamlRoot",
    "Microsoft.UI.Xaml.IXamlRoot2::{ContentIslandEnvironment}",
    "Microsoft.UI.Content.ContentIslandEnvironment",
    "Microsoft.UI.Content.IContentIslandEnvironment::{AppWindowId}",
    "Microsoft.UI.WindowId",
    "Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBase::{ShowAt,ShowAtWithOptions,Hide}",
    // サイドバーの行の印を押したら吹き出しを出す（木の項目は文字だけなので、行の TextBlock の中で印の字の位置を
    // 求め、指がその上にあるかを見る）。
    "Microsoft.UI.Xaml.Controls.TextBlock",
    "Microsoft.UI.Xaml.Controls.ITextBlock::{Text,ContentStart,Inlines,TextHighlighters,put_Text,put_FontSize,put_FontFamily}",
    "Microsoft.UI.Xaml.Controls.ITextBlockStatics::{}",
    "Microsoft.UI.Xaml.Documents.TextPointer",
    "Microsoft.UI.Xaml.Documents.ITextPointer::{GetPositionAtOffset,GetCharacterRect,Offset}",
    "Microsoft.UI.Xaml.Documents.LogicalDirection",
    // `#` を打った所（カーソルの下）に Issue・PR の候補のメニューを出す（TextBox の字の矩形）。
    "Microsoft.UI.Xaml.Controls.TextBox",
    "Microsoft.UI.Xaml.Controls.ITextBox::{GetRectFromCharacterIndex}",
    "Microsoft.UI.Xaml.Controls.ITextBoxFactory::{}",
    "Microsoft.UI.Xaml.Controls.ITextBoxStatics::{}",
    // History の CI の印の吹き出し（印の位置に置いた見えないボタンの Flyout を開く）。
    "Microsoft.UI.Xaml.Controls.Button",
    "Microsoft.UI.Xaml.Controls.IButton::{Flyout}",
    "Microsoft.UI.Xaml.Controls.IButtonFactory::{}",
    "Microsoft.UI.Xaml.Controls.IButtonStatics::{}",
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
    // 窓を透かすとき、ボタン・文字の欄・選ぶ欄の地を窓の地と同じ薄さにする（窓の根の Resources で上書きする）。
    "Microsoft.UI.Xaml.ResourceDictionary",
    "Microsoft.UI.Xaml.IResourceDictionaryFactory::{}",
    "Microsoft.UI.Xaml.ElementTheme",
    "Microsoft.UI.Xaml.Media.SolidColorBrush",
    "Microsoft.UI.Xaml.Media.ISolidColorBrush::{Color,put_Color}",
    "Microsoft.UI.Xaml.Media.ISolidColorBrushFactory::{CreateInstanceWithColor}",
    "Microsoft.UI.Xaml.Media.ISolidColorBrushStatics::{}",
    "Microsoft.UI.Xaml.Media.IBrushFactory::{}",
    "Microsoft.UI.Xaml.Media.IBrushStatics::{}",
    // History の「すべてのファイル」の整形した Markdown の中を探す（並んだ文字の部品の文字を読み、一致に
    // TextHighlighter で色を付け、今の一致へ寄せる）。
    "Microsoft.UI.Xaml.Controls.RichTextBlock",
    "Microsoft.UI.Xaml.Controls.IRichTextBlock::{Blocks,TextHighlighters}",
    "Microsoft.UI.Xaml.Controls.IRichTextBlockStatics::{}",
    "Microsoft.UI.Xaml.Documents.Block",
    "Microsoft.UI.Xaml.Documents.IBlockFactory::{}",
    "Microsoft.UI.Xaml.Documents.IBlockStatics::{}",
    "Microsoft.UI.Xaml.Documents.Paragraph",
    "Microsoft.UI.Xaml.Documents.IParagraph::{Inlines}",
    "Microsoft.UI.Xaml.Documents.IParagraphStatics::{}",
    "Microsoft.UI.Xaml.Documents.Inline",
    "Microsoft.UI.Xaml.Documents.IInlineFactory::{}",
    "Microsoft.UI.Xaml.Documents.Run",
    "Microsoft.UI.Xaml.Documents.IRun::{Text}",
    "Microsoft.UI.Xaml.Documents.IRunStatics::{}",
    "Microsoft.UI.Xaml.Documents.Span",
    "Microsoft.UI.Xaml.Documents.ISpan::{Inlines}",
    "Microsoft.UI.Xaml.Documents.ISpanFactory::{}",
    "Microsoft.UI.Xaml.Documents.LineBreak",
    "Microsoft.UI.Xaml.Documents.TextElement",
    "Microsoft.UI.Xaml.Documents.ITextElement::{ContentStart}",
    "Microsoft.UI.Xaml.Documents.ITextElementFactory::{}",
    "Microsoft.UI.Xaml.Documents.ITextElementStatics::{}",
    "Microsoft.UI.Xaml.Documents.TextHighlighter",
    "Microsoft.UI.Xaml.Documents.ITextHighlighter::{Ranges,put_Background}",
    "Microsoft.UI.Xaml.Documents.ITextHighlighterFactory::{CreateInstance}",
    "Microsoft.UI.Xaml.Documents.ITextHighlighterStatics::{}",
    "Microsoft.UI.Xaml.Documents.TextRange",
    "Microsoft.UI.Xaml.BringIntoViewOptions",
    "Microsoft.UI.Xaml.IBringIntoViewOptions::{put_TargetRect,put_VerticalAlignmentRatio}",
    // Markdown の `<sub>`・`<sup>`（文字の飾りの付属の値で、上付き・下付きにする）。
    "Microsoft.UI.Xaml.Documents.Typography",
    "Microsoft.UI.Xaml.Documents.ITypographyStatics::{SetVariants}",
    "Microsoft.UI.Xaml.FontVariants",
    // Markdown の `<kbd>`: 文の中に置く枠付きの札（InlineUIContainer の中に Border と TextBlock）。
    "Microsoft.UI.Xaml.Documents.InlineUIContainer",
    "Microsoft.UI.Xaml.Documents.IInlineUIContainer::{put_Child}",
    "Microsoft.UI.Xaml.Controls.Border",
    "Microsoft.UI.Xaml.Controls.IBorder::{put_Child,put_BorderBrush,put_BorderThickness,put_CornerRadius,put_Padding,put_Background}",
    "Microsoft.UI.Xaml.Controls.IBorderStatics::{}",
    "Microsoft.UI.Xaml.CornerRadius",
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
    for name in ["ResourceDictionary", "BlockCollection", "InlineCollection"] {
        drop_into_iterator(&out, name);
    }
}

/// `name` の `impl IntoIterator`（値と参照の 2 つ）を取り除く。生成物は IIterable の `First` をクラスに付けずに
/// 呼ぶので通らない（Resources に鍵を入れるだけ、文字の段落・飾りは `IVector` にして番号で読む）。
fn drop_into_iterator(out: &Path, name: &str) {
    let mut text = std::fs::read_to_string(out).expect("生成したバインディングを読む");
    for head in [
        format!("impl IntoIterator for {name} {{"),
        format!("impl IntoIterator for &{name} {{"),
    ] {
        let Some(start) = text.find(&head) else {
            continue;
        };
        let end = text[start..]
            .find("\n}\n")
            .map(|i| start + i + 3)
            .expect("impl の終わり");
        text.replace_range(start..end, "");
    }
    std::fs::write(out, text).expect("生成したバインディングを書く");
}
