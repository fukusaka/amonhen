//! タブ（TabView）を窓の外へ引き出したとき（windows-reactor の TabView には、その知らせがない）。

use crate::bindings::{DependencyObject, FrameworkElement, TabView, VisualTreeHelper};
use windows_core::{EventRevoker, HSTRING, IInspectable, Interface};

/// `element` の中の最初の TabView（深さ 8 まで）。
fn find_tab_view(element: &IInspectable) -> Option<TabView> {
    fn find(node: &DependencyObject, depth: u32) -> Option<TabView> {
        if let Ok(tabs) = node.cast::<TabView>() {
            return Some(tabs);
        }
        if depth == 0 {
            return None;
        }
        let count = VisualTreeHelper::GetChildrenCount(node).ok()?;
        (0..count).find_map(|i| find(&VisualTreeHelper::GetChild(node, i).ok()?, depth - 1))
    }
    find(&element.cast().ok()?, 8)
}

/// `element` の中の TabView のタブを引けるようにし、窓の外へ落としたら `dropped(タブの Tag の文字)` を呼ぶ
/// （UI のスレッドで）。TabView が見つからなければ None。
pub fn on_tab_dropped_outside(
    element: &IInspectable,
    dropped: impl Fn(String) + Send + 'static,
) -> Option<EventRevoker> {
    let tabs = find_tab_view(element)?;
    let _ = tabs.SetCanDragTabs(true);
    tabs.TabDroppedOutside(move |_, args| {
        let Some(args) = args.as_ref() else { return };
        let Some(tag) = args
            .Tab()
            .ok()
            .and_then(|tab| tab.cast::<FrameworkElement>().ok()?.Tag().ok())
            .and_then(|tag| {
                tag.cast::<windows_reference::IReference<HSTRING>>()
                    .ok()?
                    .Value()
                    .ok()
            })
        else {
            return;
        };
        dropped(tag.to_string_lossy());
    })
    .ok()
}
