//! Stashes の画面: 上に Apply・Pop・Drop…・Branch…、その下に stash の一覧（新しい順）。選んだ stash の中身は
//! 右の列（History の詳細と差分の欄）に出す。GTK4 版の `stashes.rs`（macOS 版の `StashesViewController`）と同じ並び。

use amonhen_model::{RefEntry, tr};
use windows_reactor::*;

/// 一覧の 1 行の見出しと、作ったときのブランチ（GTK4 版の `Stash::new` と同じ読み方）。見出しは `-m` の
/// メッセージ、なければ WIP と、作ったときの HEAD の件名。
pub fn title(detail: Option<&str>) -> (String, Option<String>) {
    let message = detail.unwrap_or_default();
    let wip = message.strip_prefix("WIP on ");
    let parsed = wip
        .or_else(|| message.strip_prefix("On "))
        .and_then(|rest| rest.split_once(": "));
    match parsed {
        Some((branch, rest)) => {
            let title = if wip.is_some() {
                format!("WIP · {}", rest.split_once(' ').map_or("", |(_, s)| s))
            } else {
                rest.to_owned()
            };
            (title, Some(branch.to_owned()))
        }
        None => (message.to_owned(), None),
    }
}

/// stash の操作のメニュー（一覧の行の右クリック。GTK4 版の `stashes::menu`）。キーは `Message::HistoryMenu` で受ける。
pub fn menu_items(name: &str) -> Vec<MenuItem> {
    vec![
        MenuItem::item(
            format!("stash-apply:{name}"),
            tr!("Apply {name}…", name = name),
        ),
        MenuItem::item(format!("stash-pop:{name}"), tr!("Pop {name}…", name = name)),
        MenuItem::item(
            format!("stash-drop:{name}"),
            tr!("Drop {name}…", name = name),
        ),
        MenuItem::item(
            format!("stash-branch:{name}"),
            tr!("New Branch from {name}…", name = name),
        ),
        MenuItem::separator("-copy"),
        MenuItem::item(format!("copy:{name}"), tr!("Copy Name")),
    ]
}

pub struct Callbacks<'a> {
    /// 一覧で選んだ行。
    pub selected: Callback<Option<usize>>,
    /// ボタン（`stash-apply` などのキーの前半）。
    pub button: Box<dyn Fn(&'static str) -> Callback<()> + 'a>,
    /// 行の右クリックのメニュー（stash の名前ごと）。
    pub menu: Box<dyn Fn() -> Callback<String> + 'a>,
}

/// `stashes` は新しい順、`selected` は選んでいる stash の名前、`current` は今のブランチ（ほかのブランチで作った
/// stash は、ブランチの名前を薄くして見分ける）。
pub fn view(
    stashes: &[&RefEntry],
    selected: Option<&str>,
    current: Option<&str>,
    callbacks: Callbacks,
) -> View {
    let enabled = selected.is_some();
    let buttons = [
        (
            "stash-apply",
            "Apply".to_owned(),
            tr!("Apply the stash to the working tree and keep it in the list"),
        ),
        (
            "stash-pop",
            "Pop".to_owned(),
            tr!("Apply the stash to the working tree and remove it from the list"),
        ),
        (
            "stash-drop",
            tr!("Drop…"),
            tr!("Remove the stash without applying it (you can Undo it)"),
        ),
        (
            "stash-branch",
            tr!("Branch…"),
            tr!(
                "Create a branch at the commit the stash was made on, switch to it and apply the stash there (git stash branch)"
            ),
        ),
    ];
    let [a, b, c, d] = buttons.map(|(key, title, tip)| {
        Button::new()
            .is_enabled(enabled)
            .on_click((callbacks.button)(key))
            .content(TextBlock::new().text(title))
            .tooltip(tip)
    });
    let bar = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(6.0)
        .margin(Thickness::new(8.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((a, b, c, d));
    let rows = stashes.iter().map(|entry| {
        let (title, branch) = title(entry.detail.as_deref());
        let other = branch.as_deref().is_some_and(|b| Some(b) != current);
        let second = match &branch {
            Some(b) => format!("{b} · {}", entry.name),
            None => entry.name.clone(),
        };
        let row = StackPanel::new().spacing(1.0).children((
            TextBlock::new()
                .text(title)
                .text_trimming(TextTrimming::CharacterEllipsis),
            TextBlock::new()
                .text(second)
                .font_size(11.0)
                .opacity(if other { 0.45 } else { 0.7 }),
        ));
        let row = row.menu(Menu::new(menu_items(&entry.name), (callbacks.menu)()));
        (
            entry.name.clone(),
            ListViewItem::new().tag(entry.name.clone()).content(row),
        )
    });
    let index = selected.and_then(|name| stashes.iter().position(|e| e.name == name));
    let list = ListView::new()
        .selected_index(index)
        .on_selection_changed(callbacks.selected)
        .collection_slot(ListViewSlot::Items, rows);
    // stash が増減するたびに一覧ごと作り直す（reactor の ListView は消した項目の場所が空いたまま残る）。
    let key: String = stashes
        .iter()
        .map(|e| e.target.map(|t| t.to_string()).unwrap_or_default())
        .collect();
    let list = Border::new()
        .grid_row(1)
        .content(View::keyed_fragment([(key, list)]));
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((bar, list))
}
