//! Reflog の画面（GTK4 版の `reflog.rs`、macOS 版の `ReflogViewController`）: HEAD の reflog（新しい順）。選んだ
//! 行のコミットの中身は右の列（History の詳細と差分の欄）に出す。reset・rebase・amend で外れたコミットを見つけて、
//! ブランチを作って取り戻すのに使う。Range Diff との比較はまだない。

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// `reflog` の答えの 1 行。
#[derive(Clone, PartialEq)]
pub struct Entry {
    /// `HEAD@{n}`。
    pub selector: String,
    /// この動きの後に HEAD が指したコミット。
    pub new: String,
    pub time: i64,
    /// git の言葉（`commit: …`・`checkout: moving from a to b`・`reset: moving to …`）。
    pub message: String,
    /// ref から辿れる（History にある）。
    pub reachable: bool,
}

pub fn entries_of(info: &Value) -> Vec<Entry> {
    info["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Entry {
            selector: e["selector"].as_str().unwrap_or_default().to_owned(),
            new: e["new"].as_str().unwrap_or_default().to_owned(),
            time: e["time"].as_i64().unwrap_or_default(),
            message: e["message"].as_str().unwrap_or_default().to_owned(),
            reachable: e["reachable"].as_bool().unwrap_or(true),
        })
        .collect()
}

/// 行の右クリックのメニュー（GTK4 版の `ReflogView::menu`）。キーは History の右クリックと同じ形。
/// `head` は今の HEAD（一番上の行のコミット）。ほかの行なら、それと比べる（Range Diff）。
pub fn menu_items(entry: &Entry, branch: &str, head: Option<&str>) -> Vec<MenuItem> {
    let hex = &entry.new;
    let short = &hex[..hex.len().min(8)];
    let mut items = vec![
        MenuItem::item(
            format!("branch:{hex}"),
            tr!("New Branch from {name}…", name = short),
        ),
        MenuItem::item(
            format!("tag:{hex}"),
            tr!("New Tag at {name}…", name = short),
        ),
        MenuItem::item(
            format!("detach:{hex}"),
            tr!("Checkout {id} (Detached)…", id = short),
        ),
        MenuItem::item(
            format!("reset:{hex}"),
            tr!("Reset {branch} to {id}…", branch = branch, id = short),
        ),
    ];
    if let Some(head) = head.filter(|h| *h != hex) {
        items.push(MenuItem::separator("-range"));
        items.push(MenuItem::item(
            format!("rangediff:{hex}...{head}"),
            tr!("Compare with Current HEAD (Range Diff)"),
        ));
    }
    if entry.reachable {
        items.push(MenuItem::separator("-show"));
        items.push(MenuItem::item(
            format!("show:{hex}"),
            tr!("Show in History"),
        ));
    }
    items.push(MenuItem::separator("-copy"));
    items.push(MenuItem::item(format!("copy:{hex}"), tr!("Copy Commit ID")));
    items.push(MenuItem::item(
        format!("copy:{}", entry.selector),
        tr!("Copy Name"),
    ));
    items
}

pub struct Callbacks<'a> {
    pub selected: Callback<Option<usize>>,
    /// 上のボタンと行のメニュー（キーは `menu_items` と同じ）。
    pub menu: Box<dyn Fn() -> Callback<String> + 'a>,
    pub button: Box<dyn Fn(String) -> Callback<()> + 'a>,
}

pub fn view(
    entries: &[Entry],
    selected: Option<usize>,
    branch: &str,
    callbacks: Callbacks,
) -> View {
    let chosen = selected.and_then(|i| entries.get(i));
    let button = |title: &str, key: Option<String>, tip: String| {
        crate::tip::holder(
            crate::small::button(title, |b| {
                b.is_enabled(key.is_some())
                    .on_click((callbacks.button)(key.unwrap_or_default()))
            }),
            tip,
            0,
        )
    };
    // 上の帯: 選んだ行のコミットにブランチを作る・detached でチェックアウトする（GTK4 版と同じ言葉）。
    let bar = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(6.0)
        .margin(Thickness::new(8.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            button(
                "New Branch…",
                chosen.map(|e| format!("branch:{}", e.new)),
                tr!("Create a branch at this commit and switch to it"),
            ),
            button(
                "Checkout",
                chosen.map(|e| format!("detach:{}", e.new)),
                tr!("Check out this commit as a detached HEAD"),
            ),
        ));
    let rows = entries.iter().map(|e| {
        let message = if e.message.is_empty() {
            "—"
        } else {
            &e.message
        };
        let mut parts = Vec::new();
        // ブランチから外れたコミットは、ここからしか辿れない（reflog が切れると gc で消える）。
        let lost = !e.reachable;
        parts.push(crate::render::local_date(e.time));
        parts.push(e.new[..e.new.len().min(7)].to_owned());
        let row = StackPanel::new().spacing(1.0).children((
            TextBlock::new()
                .text(format!("{}  {message}", e.selector))
                .text_trimming(TextTrimming::CharacterEllipsis),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((
                    if lost {
                        View::from(
                            TextBlock::new()
                                .text(tr!("not on any branch"))
                                .font_size(11.0)
                                .foreground(Brush::Solid(Color::rgb(0xe6, 0x61, 0x00))),
                        )
                    } else {
                        View::empty()
                    },
                    TextBlock::new()
                        .text(parts.join("  "))
                        .font_size(11.0)
                        .opacity(0.6),
                )),
        ));
        let row = row.menu(Menu::new(
            menu_items(e, branch, entries.first().map(|f| f.new.as_str())),
            (callbacks.menu)(),
        ));
        (
            format!("{}{}", e.selector, e.new),
            ListViewItem::new().content(row),
        )
    });
    let list = ListView::new()
        .selected_index(selected)
        .on_selection_changed(callbacks.selected)
        .collection_slot(ListViewSlot::Items, rows);
    // 動くたびに一覧ごと作り直す（reactor の ListView は消した項目の場所が空いたまま残る）。
    let key: String = entries.first().map_or(String::new(), |e| {
        format!("{}{}{}", entries.len(), e.new, e.time)
    });
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((
            bar,
            Border::new()
                .grid_row(1)
                .content(View::keyed_fragment([(key, list)])),
        ))
}
