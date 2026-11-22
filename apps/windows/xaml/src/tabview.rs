//! タブ（TabView）を窓の外へ引き出したとき・ほかの窓のタブの並びへ落としたとき（windows-reactor の TabView には、
//! その知らせがない）。

use crate::bindings::{
    DataPackageOperation, DependencyObject, FrameworkElement, TabView, TabViewItem, UIElement,
    VisualTreeHelper,
};
use windows_core::{EventRevoker, HSTRING, IInspectable, Interface};

thread_local! {
    /// 引いているタブ（その Tag の文字）。窓はどれも同じプロセス・同じスレッドなので、落とす窓がここで読む。
    static DRAGGING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// ポインタが乗っているタブ（その Tag の文字）。引き始めの知らせの `Tab` は、前に出ているタブを引くと
    /// 別のタブを返すことがあるので、引いたタブはこちらで決める。
    static HOVERED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// 最後に引き始めたタブ（窓の外へ落としたときに読む。引き終わりの知らせが先に来ても残す）。
    static LAST_DRAGGED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
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

/// `source`（ポインタの下の部品）を含むタブの Tag の文字。
fn tab_under(source: &IInspectable) -> Option<String> {
    let mut node: DependencyObject = source.cast().ok()?;
    for _ in 0..32 {
        if node.cast::<TabViewItem>().is_ok() {
            return tag_of(&node.cast().ok()?);
        }
        node = VisualTreeHelper::GetParent(&node).ok()?;
    }
    None
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
/// ほかの窓から引いてきたタブをタブの並びへ落としたら `received(そのタブの Tag の文字)` を呼ぶ。タブを引き
/// 始めたら `started` を、引き終えたら `ended` を呼ぶ（どれも UI のスレッドで）。TabView が見つからなければ空。
pub fn on_tab_moves(
    element: &IInspectable,
    dropped: impl Fn(String) + Send + 'static,
    received: impl Fn(String) + Send + 'static,
    started: impl Fn() + Send + 'static,
    ended: impl Fn() + Send + 'static,
) -> Vec<EventRevoker> {
    let Some(tabs) = find_tab_view(element) else {
        return Vec::new();
    };
    let _ = tabs.SetCanDragTabs(true);
    let _ = tabs.SetAllowDropTabs(true);
    let mut revokers = Vec::new();
    // ポインタが乗っているタブを覚える（タブの上でなければ忘れる）。
    if let Ok(element) = tabs.cast::<UIElement>() {
        revokers.extend(
            element
                .PointerMoved(|_, args| {
                    let tag = args
                        .as_ref()
                        .and_then(|args| args.OriginalSource().ok())
                        .and_then(|source| tab_under(&source));
                    HOVERED.with(|h| *h.borrow_mut() = tag);
                })
                .ok(),
        );
    }
    // 引き始め: 引くタブを覚える（終わったら忘れる）。ポインタの下のタブで、分からなければ知らせの `Tab`。
    revokers.extend(
        tabs.TabDragStarting(move |_, args| {
            started();
            let tag = HOVERED.with(|h| h.borrow().clone()).or_else(|| {
                let tab = args.as_ref()?.Tab().ok()?;
                tag_of(&tab.into())
            });
            LAST_DRAGGED.with(|d| d.borrow_mut().clone_from(&tag));
            DRAGGING.with(|d| *d.borrow_mut() = tag);
        })
        .ok(),
    );
    revokers.extend(
        tabs.TabDragCompleted(move |_, _| {
            DRAGGING.with(|d| d.borrow_mut().take());
            ended();
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
            // 引き始めに覚えたタブ。
            let tag = LAST_DRAGGED.with(|d| d.borrow_mut().take()).or_else(|| {
                let tab = args.as_ref()?.Tab().ok()?;
                tag_of(&tab.into())
            });
            if let Some(tag) = tag {
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
