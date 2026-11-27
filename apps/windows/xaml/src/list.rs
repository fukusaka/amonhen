//! 一覧（ListView）で選んでいる行（windows-reactor の ListView は選んだ位置を 1 つしか知らせない）と、一覧から
//! 一覧への行のドラッグ。

use crate::bindings::{
    ContentControl, DataPackageOperation, DependencyObject, DragEventArgs,
    DragItemsCompletedEventArgs, DragItemsStartingEventArgs, FrameworkElement, ListViewBase,
    UIElement, VisualTreeHelper,
};
use windows_core::{EventRevoker, HSTRING, IInspectable, Interface, Ref};

/// `element` の中の最初の一覧（深さ 8 まで）。ScrollViewer などの中身は、テンプレートがまだ付いていなくても
/// たどれるよう、中身（Content）からもたどる（ツールバーの並びは ScrollViewer の中）。
fn find_list(element: &IInspectable) -> Option<ListViewBase> {
    fn find(node: &DependencyObject, depth: u32) -> Option<ListViewBase> {
        if let Ok(list) = node.cast::<ListViewBase>() {
            return Some(list);
        }
        if depth == 0 {
            return None;
        }
        if let Some(content) = node
            .cast::<ContentControl>()
            .ok()
            .and_then(|c| c.Content().ok())
            .and_then(|c| c.cast::<DependencyObject>().ok())
            && let Some(list) = find(&content, depth - 1)
        {
            return Some(list);
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

/// `from` から親をたどって（`depth` 段まで）、`prefix` で始まる文字の Tag を持つ最初の部品の Tag。
pub fn tag_up_with(from: &IInspectable, prefix: &str, depth: u32) -> Option<String> {
    let mut node: DependencyObject = from.cast().ok()?;
    for _ in 0..depth {
        if let Some(tag) = node.cast::<IInspectable>().ok().and_then(|n| tag_of(&n))
            && tag.starts_with(prefix)
        {
            return Some(tag);
        }
        node = VisualTreeHelper::GetParent(&node).ok()?;
    }
    None
}

/// 文字を、部品の中身（Content）に入れられる値にする。
pub fn text_value(text: &str) -> IInspectable {
    windows_reference::IReference::from(HSTRING::from(text)).into()
}

/// 部品の中身（Content）が文字なら、その文字。
pub fn text_of(value: &IInspectable) -> Option<String> {
    Some(
        value
            .cast::<windows_reference::IReference<HSTRING>>()
            .ok()?
            .Value()
            .ok()?
            .to_string_lossy(),
    )
}

/// 部品の Tag の文字。
pub fn tag_text(item: &IInspectable) -> Option<String> {
    tag_of(item)
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

/// `element` の中の最初の一覧で、Tag が `tags` のどれかの行を選ぶ（作り直した一覧で、前に選んでいた行を選び
/// 直す）。もう選んでいる行はそのまま、ほかの行は外さない。行は作ってある（見えている）ものだけで、作ったばかりの
/// 一覧でまだ行がなければ、一覧の大きさが決まったとき（行を作った後）に 1 度だけ選ぶ。
pub fn select_tags(element: &IInspectable, tags: &[String]) {
    let Some(list) = find_list(element) else {
        return;
    };
    if select_in(&list, tags) {
        return;
    }
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tags = tags.to_vec();
    let revoker = list.SizeChanged(move |sender, _| {
        if done.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if let Some(list) = sender.as_ref().and_then(|s| s.cast::<ListViewBase>().ok())
            && select_in(&list, &tags)
        {
            done.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    if let Ok(revoker) = revoker {
        PENDING_SELECT.with(|p| *p.borrow_mut() = Some(revoker));
    }
}

thread_local! {
    /// 大きさが決まるのを待って選び直す一覧の知らせ（次に頼まれたら替える）。
    static PENDING_SELECT: std::cell::RefCell<Option<EventRevoker>> = const { std::cell::RefCell::new(None) };
}

/// 一覧 `list` の作ってある行のうち、Tag が `tags` のどれかの行を選ぶ。行が 1 つも見つからなければ false。
fn select_in(list: &ListViewBase, tags: &[String]) -> bool {
    fn walk(node: &DependencyObject, depth: u32, out: &mut Vec<(String, IInspectable)>) {
        if let Ok(item) = node.cast::<IInspectable>()
            && let Some(tag) = tag_of(&item)
        {
            out.push((tag, item));
            return;
        }
        if depth == 0 {
            return;
        }
        let count = VisualTreeHelper::GetChildrenCount(node).unwrap_or(0);
        for i in 0..count {
            if let Ok(child) = VisualTreeHelper::GetChild(node, i) {
                walk(&child, depth - 1, out);
            }
        }
    }
    let Ok(selected) = list.SelectedItems() else {
        return true;
    };
    let count = selected.Size().unwrap_or(0);
    let already: Vec<String> = (0..count)
        .filter_map(|i| tag_of(&selected.GetAt(i).ok()?))
        .collect();
    let mut items = Vec::new();
    if let Ok(node) = list.cast::<DependencyObject>() {
        walk(&node, 24, &mut items);
    }
    if items.is_empty() {
        return false;
    }
    for (tag, item) in items {
        if tags.contains(&tag) && !already.contains(&tag) {
            let _ = selected.Append(&item);
        }
    }
    true
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

/// `element` の中の最初の一覧から引いた行を、どこも受けない所に落としたら `dropped(引いた行の Tag)` を呼ぶ
/// （ツールバーのタイルを帯の外に落として外す。NSToolbar と同じ）。一覧の中の並べ替えや、受け口に落としたときは
/// 呼ばない。
pub fn dropped_nowhere(
    element: &IInspectable,
    dropped: impl Fn(Vec<String>) + Send + 'static,
) -> Option<EventRevoker> {
    let list = find_list(element)?;
    list.DragItemsCompleted(move |_, args: Ref<DragItemsCompletedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        if !args
            .DropResult()
            .is_ok_and(|r| r == DataPackageOperation::None)
        {
            return;
        }
        let Ok(items) = args.Items() else { return };
        let tags: Vec<String> = (0..items.Size().unwrap_or(0))
            .filter_map(|i| tag_up(&items.GetAt(i).ok()?))
            .collect();
        if !tags.is_empty() {
            dropped(tags);
        }
    })
    .ok()
}

/// 落とす先（行を探す部品・落とせる行の Tag・受ける引いた行の Tag の前置き（None ならどれも）・落としたときに
/// 呼ぶもの（落とした行の Tag、引いた行の Tag、行の右半分に落としたか））。行は画面に出てから作られるので、受け口は
/// 行を引き始めたときに付ける（`attach_drop`）。
struct DropTarget {
    element: IInspectable,
    targets: Vec<String>,
    prefix: Option<&'static str>,
    dropped: std::rc::Rc<dyn Fn(String, Vec<String>, bool)>,
}

thread_local! {
    /// 落とす先（名前ごと。左の一覧の `rows`、ツールバーの並びの `toolbar` など）。
    static DROP_TARGETS: std::cell::RefCell<Vec<(&'static str, DropTarget)>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
    set_drop_target("rows", element, targets, None, move |tag, tags, _| {
        dropped(tag, tags)
    });
}

/// 落とす先 `name` を置き換える（`drop_on_rows` の一般の形）。`prefix` があれば、引いた行の Tag がどれもそれで
/// 始まるときだけ受ける（受けないドラッグは行の一覧に任せる。一覧の中の並べ替えなど）。落としたら
/// `dropped(行の Tag、前置きを外した引いた行の Tag、行の右半分か)`。
pub fn set_drop_target(
    name: &'static str,
    element: &IInspectable,
    targets: Vec<String>,
    prefix: Option<&'static str>,
    dropped: impl Fn(String, Vec<String>, bool) + 'static,
) {
    let target = DropTarget {
        element: element.clone(),
        targets,
        prefix,
        dropped: std::rc::Rc::new(dropped),
    };
    DROP_TARGETS.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|(n, _)| *n != name);
        t.push((name, target));
    });
}

/// 落とす先 `name` を外す。
pub fn clear_drop_target(name: &'static str) {
    DROP_TARGETS.with(|t| t.borrow_mut().retain(|(n, _)| *n != name));
}

/// 引いている行の Tag が `prefix` で受けられるものなら、前置きを外したもの。
fn accepted(prefix: Option<&str>) -> Option<Vec<String>> {
    let dragged = DRAGGED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if dragged.is_empty() {
        return None;
    }
    match prefix {
        None => Some(dragged),
        Some(p) => dragged
            .iter()
            .map(|t| t.strip_prefix(p).map(str::to_owned))
            .collect(),
    }
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
    let targets: Vec<(
        &'static str,
        IInspectable,
        Vec<String>,
        Option<&'static str>,
    )> = DROP_TARGETS.with(|t| {
        t.borrow()
            .iter()
            .map(|(n, t)| (*n, t.element.clone(), t.targets.clone(), t.prefix))
            .collect()
    });
    let mut revokers = Vec::new();
    for (name, element, tags, prefix) in targets {
        let mut found = Vec::new();
        if let Ok(node) = element.cast::<DependencyObject>() {
            rows(&node, 20, &tags, &mut found);
        }
        for (ui, tag) in found {
            let _ = ui.SetAllowDrop(true);
            let over = ui.DragOver(move |_, args: Ref<DragEventArgs>| {
                let Some(args) = args.as_ref() else { return };
                // 受けないドラッグは、行の一覧に任せる（処理済みにしない）。
                if accepted(prefix).is_none() {
                    return;
                }
                let _ = args.SetAcceptedOperation(DataPackageOperation::Copy);
                let _ = args.SetHandled(true);
            });
            let row = ui.clone();
            let drop = ui.Drop(move |_, args: Ref<DragEventArgs>| {
                let Some(args) = args.as_ref() else { return };
                let Some(tags) = accepted(prefix) else { return };
                let _ = args.SetHandled(true);
                DRAGGED.lock().unwrap_or_else(|e| e.into_inner()).clear();
                // 行の右半分に落としたか（ツールバーの並びは、そのタイルの後ろに入れる）。
                let after = (|| {
                    let x = args.GetPosition(&row).ok()?.X;
                    let width = row.cast::<FrameworkElement>().ok()?.ActualWidth().ok()?;
                    Some(f64::from(x) > width / 2.0)
                })()
                .unwrap_or(false);
                let tag = tag.clone();
                // 呼ぶものは UI のスレッドのもの（受け口も UI のスレッドで呼ばれる）。
                let dropped = DROP_TARGETS.with(|t| {
                    t.borrow()
                        .iter()
                        .find(|(n, _)| *n == name)
                        .map(|(_, t)| t.dropped.clone())
                });
                if let Some(dropped) = dropped {
                    dropped(tag, tags, after);
                }
            });
            revokers.extend([over, drop].into_iter().filter_map(Result::ok));
        }
    }
    DROP_REVOKERS.with(|r| *r.borrow_mut() = revokers);
}
