//! 一覧（ListView）で選んでいる行（windows-reactor の ListView は選んだ位置を 1 つしか知らせない）。

use crate::bindings::{DependencyObject, FrameworkElement, ListViewBase, VisualTreeHelper};
use windows_core::{HSTRING, IInspectable, Interface};

/// `element` の中の最初の一覧で選んでいる行の Tag（文字）。一覧が見つからなければ空。
pub fn selected_tags(element: &IInspectable) -> Vec<String> {
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
    let Some(list) = element
        .cast::<DependencyObject>()
        .ok()
        .and_then(|node| find(&node, 8))
    else {
        return Vec::new();
    };
    let Ok(items) = list.SelectedItems() else {
        return Vec::new();
    };
    let count = items.Size().unwrap_or(0);
    (0..count)
        .filter_map(|i| {
            let tag = items
                .GetAt(i)
                .ok()?
                .cast::<FrameworkElement>()
                .ok()?
                .Tag()
                .ok()?;
            let text = tag
                .cast::<windows_reference::IReference<HSTRING>>()
                .ok()?
                .Value()
                .ok()?;
            Some(text.to_string_lossy())
        })
        .collect()
}
