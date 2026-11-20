//! 一覧（ListView）で選んでいる行（windows-reactor の ListView は選んだ位置を 1 つしか知らせない）と、一覧から
//! 一覧への行のドラッグ。

use crate::bindings::{
    DataPackageOperation, DependencyObject, DragEventArgs, DragItemsStartingEventArgs,
    FrameworkElement, ListViewBase, UIElement, VisualTreeHelper,
};
use windows_core::{EventRevoker, HSTRING, IInspectable, Interface, Ref};

/// `element` の中の最初の一覧（深さ 8 まで）。
fn find_list(element: &IInspectable) -> Option<ListViewBase> {
    fn find(node: &DependencyObject, depth: u32) -> Option<ListViewBase> {
        if let Ok(list) = node.cast::<ListViewBase>() {
            return Some(list);
        }
        if depth == 0 {
            return None;
        }
        let count = VisualTreeHelper::GetChildrenCount(node).ok()?;
        (0..count).find_map(|i| {
            let child = VisualTreeHelper::GetChild(node, i).ok()?;
            find(&child, depth - 1)
        })
    }
    element
        .cast::<DependencyObject>()
        .ok()
        .and_then(|node| find(&node, 8))
}

/// 行（ListViewItem）の Tag の文字。
fn tag_of(item: &IInspectable) -> Option<String> {
    let tag = item.cast::<FrameworkElement>().ok()?.Tag().ok()?;
    let text = tag
        .cast::<windows_reference::IReference<HSTRING>>()
        .ok()?
        .Value()
        .ok()?;
    Some(text.to_string_lossy())
}

/// 引いた項目の Tag。項目が行の中身なら、親をたどって Tag のある行（ListViewItem）まで上がる。
fn tag_up(item: &IInspectable) -> Option<String> {
    if let Some(tag) = tag_of(item) {
        return Some(tag);
    }
    let mut node = item.cast::<DependencyObject>().ok()?;
    for _ in 0..8 {
        node = VisualTreeHelper::GetParent(&node).ok()?;
        if let Some(tag) = tag_of(&node.cast::<IInspectable>().ok()?) {
            return Some(tag);
        }
    }
    None
}

/// `element` の中の最初の一覧で選んでいる行の Tag（文字）。一覧が見つからなければ空。
pub fn selected_tags(element: &IInspectable) -> Vec<String> {
    let Some(list) = find_list(element) else {
        return Vec::new();
    };
    let Ok(items) = list.SelectedItems() else {
        return Vec::new();
    };
    let count = items.Size().unwrap_or(0);
    (0..count)
        .filter_map(|i| tag_of(&items.GetAt(i).ok()?))
        .collect()
}

/// 引いている行の Tag（この一覧から引き始めたとき。落とした先が読む）。
static DRAGGED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// `element` の中の最初の一覧の行を引いたら、引いた行の Tag を覚える（`drop_on_rows` で落とした先が受け取る）。
/// 一覧が見つからなければ None。
pub fn drag_tags(element: &IInspectable) -> Option<EventRevoker> {
    let list = find_list(element)?;
    list.DragItemsStarting(move |_, args: Ref<DragItemsStartingEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        let Ok(items) = args.Items() else { return };
        let count = items.Size().unwrap_or(0);
        let tags: Vec<String> = (0..count)
            .filter_map(|i| tag_up(&items.GetAt(i).ok()?))
            .collect();
        if let Ok(data) = args.Data() {
            let _ = data.SetText(&HSTRING::from(tags.join("\n")));
            let _ = data.SetRequestedOperation(DataPackageOperation(
                DataPackageOperation::Copy.0 | DataPackageOperation::Move.0,
            ));
        }
        *DRAGGED.lock().unwrap_or_else(|e| e.into_inner()) = tags;
        attach_drop();
    })
    .ok()
}

/// `element` の中の最初の一覧の行のうち、Tag が `allowed` のどれかのものだけを引けるようにする（ほかの行は
/// 引き始めたら取りやめる）。引いた行の Tag は `prefix` を付けて覚え（`drop_on_rows` の落とした先が受け取る）、
/// 引いてきたほかの一覧の行と見分けられるようにする。
pub fn drag_some_tags(
    element: &IInspectable,
    allowed: Vec<String>,
    prefix: &'static str,
) -> Option<EventRevoker> {
    let list = find_list(element)?;
    list.DragItemsStarting(move |_, args: Ref<DragItemsStartingEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        let Ok(items) = args.Items() else { return };
        let count = items.Size().unwrap_or(0);
        let tags: Vec<String> = (0..count)
            .filter_map(|i| tag_up(&items.GetAt(i).ok()?))
            .collect();
        if tags.is_empty() || !tags.iter().all(|t| allowed.contains(t)) {
            let _ = args.SetCancel(true);
            return;
        }
        if let Ok(data) = args.Data() {
            let _ = data.SetText(&HSTRING::from(tags.join("\n")));
            // 落とす先の受け口は Copy で受ける（右の一覧から引いた行と同じ）。
            let _ = data.SetRequestedOperation(DataPackageOperation(
                DataPackageOperation::Copy.0 | DataPackageOperation::Move.0,
            ));
        }
        *DRAGGED.lock().unwrap_or_else(|e| e.into_inner()) =
            tags.into_iter().map(|t| format!("{prefix}{t}")).collect();
        attach_drop();
    })
    .ok()
}

/// 落とす先（左の一覧を包む部品・落とせる行の Tag・落としたときに呼ぶもの）。行は画面に出てから作られるので、
/// 受け口は行を引き始めたときに付ける（`attach_drop`）。
type DropTarget = (
    IInspectable,
    Vec<String>,
    std::rc::Rc<dyn Fn(String, Vec<String>)>,
);

thread_local! {
    static DROP_TARGET: std::cell::RefCell<Option<DropTarget>> = const { std::cell::RefCell::new(None) };
    /// 今付けている行の受け口（引き始めるたびに付け直す）。
    static DROP_REVOKERS: std::cell::RefCell<Vec<EventRevoker>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `element` の中の行（Tag が `targets` のどれか）に、`drag_tags` の一覧から引いてきた行を落とせるようにする
/// （UI のスレッドで呼ぶ）。落としたら `dropped(行の Tag, 引いた行の Tag)`。
pub fn drop_on_rows(
    element: &IInspectable,
    targets: Vec<String>,
    dropped: impl Fn(String, Vec<String>) + 'static,
) {
    DROP_TARGET
        .with(|t| *t.borrow_mut() = Some((element.clone(), targets, std::rc::Rc::new(dropped))));
}

/// 落とす先の行に受け口を付ける（行を引き始めたとき）。受け口は行そのものに付けて処理済みにする（一覧に付けると、
/// 一覧が行の上のドラッグを自分で処理して受けないに戻す）。
fn attach_drop() {
    fn rows(
        node: &DependencyObject,
        depth: u32,
        targets: &[String],
        out: &mut Vec<(UIElement, String)>,
    ) {
        if let Some(tag) = node.cast::<IInspectable>().ok().and_then(|n| tag_of(&n))
            && targets.contains(&tag)
            && let Ok(ui) = node.cast::<UIElement>()
        {
            out.push((ui, tag));
            return;
        }
        if depth == 0 {
            return;
        }
        let count = VisualTreeHelper::GetChildrenCount(node).unwrap_or(0);
        for i in 0..count {
            if let Ok(child) = VisualTreeHelper::GetChild(node, i) {
                rows(&child, depth - 1, targets, out);
            }
        }
    }
    let Some((element, targets, _)) = DROP_TARGET.with(|t| t.borrow().clone()) else {
        return;
    };
    let mut found = Vec::new();
    if let Ok(node) = element.cast::<DependencyObject>() {
        rows(&node, 20, &targets, &mut found);
    }
    let mut revokers = Vec::new();
    for (ui, tag) in found {
        let _ = ui.SetAllowDrop(true);
        let over = ui.DragOver(move |_, args: Ref<DragEventArgs>| {
            let Some(args) = args.as_ref() else { return };
            let dragging = !DRAGGED.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
            let _ = args.SetAcceptedOperation(if dragging {
                DataPackageOperation::Copy
            } else {
                DataPackageOperation::None
            });
            let _ = args.SetHandled(true);
        });
        let drop = ui.Drop(move |_, args: Ref<DragEventArgs>| {
            let Some(args) = args.as_ref() else { return };
            let _ = args.SetHandled(true);
            let tags = std::mem::take(&mut *DRAGGED.lock().unwrap_or_else(|e| e.into_inner()));
            if !tags.is_empty() {
                let tag = tag.clone();
                // 呼ぶものは UI のスレッドのもの（受け口も UI のスレッドで呼ばれる）。
                DROP_TARGET.with(|t| {
                    if let Some((_, _, dropped)) = t.borrow().as_ref() {
                        dropped(tag, tags);
                    }
                });
            }
        });
        revokers.extend([over, drop].into_iter().filter_map(Result::ok));
    }
    DROP_REVOKERS.with(|r| *r.borrow_mut() = revokers);
}
