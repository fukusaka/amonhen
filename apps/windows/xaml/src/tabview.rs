//! タブ（TabView）を窓の外へ引き出したとき・ほかの窓のタブの並びへ落としたとき（windows-reactor の TabView には、
//! その知らせがない）。

use crate::bindings::{
    DataPackageOperation, DependencyObject, FrameworkElement, TabView, VisualTreeHelper,
};
use windows_core::{EventRevoker, HSTRING, IInspectable, Interface};

thread_local! {
    /// 引いているタブ（その Tag の文字）。窓はどれも同じプロセス・同じスレッドなので、落とす窓がここで読む。
    static DRAGGING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

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

/// タブ（TabViewItem）の Tag の文字。
fn tag_of(tab: &IInspectable) -> Option<String> {
    let tag = tab.cast::<FrameworkElement>().ok()?.Tag().ok()?;
    let tag = tag
        .cast::<windows_reference::IReference<HSTRING>>()
        .ok()?
        .Value()
        .ok()?;
    Some(tag.to_string_lossy())
}

/// `element` の中の TabView のタブを引けるようにする。窓の外へ落としたら `dropped(タブの Tag の文字)` を、
/// ほかの窓から引いてきたタブをタブの並びへ落としたら `received(そのタブの Tag の文字)` を呼ぶ（UI のスレッドで）。
/// TabView が見つからなければ空。
pub fn on_tab_moves(
    element: &IInspectable,
    dropped: impl Fn(String) + Send + 'static,
    received: impl Fn(String) + Send + 'static,
) -> Vec<EventRevoker> {
    let Some(tabs) = find_tab_view(element) else {
        return Vec::new();
    };
    let _ = tabs.SetCanDragTabs(true);
    let _ = tabs.SetAllowDropTabs(true);
    let mut revokers = Vec::new();
    // 引き始め: 引くタブを覚える（終わったら忘れる）。
    revokers.extend(
        tabs.TabDragStarting(|_, args| {
            let tag = args
                .as_ref()
                .and_then(|args| args.Tab().ok())
                .and_then(|tab| tag_of(&tab.into()));
            DRAGGING.with(|d| *d.borrow_mut() = tag);
        })
        .ok(),
    );
    revokers.extend(
        tabs.TabDragCompleted(|_, _| {
            DRAGGING.with(|d| d.borrow_mut().take());
        })
        .ok(),
    );
    // タブの並びの上: 引いているのが Amon Hen のタブなら受ける。
    revokers.extend(
        tabs.TabStripDragOver(|_, args| {
            let Some(args) = args.as_ref() else { return };
            if dragged().is_some() {
                let _ = args.SetAcceptedOperation(DataPackageOperation::Move);
                let _ = args.SetHandled(true);
            }
        })
        .ok(),
    );
    revokers.extend(
        tabs.TabStripDrop(move |_, args| {
            let Some(args) = args.as_ref() else { return };
            if let Some(tag) = dragged() {
                let _ = args.SetHandled(true);
                received(tag);
            }
        })
        .ok(),
    );
    revokers.extend(
        tabs.TabDroppedOutside(move |_, args| {
            let Some(args) = args.as_ref() else { return };
            if let Some(tag) = args.Tab().ok().and_then(|tab| tag_of(&tab.into())) {
                dropped(tag);
            }
        })
        .ok(),
    );
    revokers
}

/// 引いているのが Amon Hen のタブなら、その Tag の文字。
fn dragged() -> Option<String> {
    DRAGGING.with(|d| d.borrow().clone())
}
