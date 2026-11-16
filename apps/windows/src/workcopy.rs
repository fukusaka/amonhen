//! Working Copy の画面: 上にコミットの入力と Commit、その下に変更のあるファイルの一覧（チェックで次のコミットに
//! 含めるか）。GTK4 版の `workcopy.rs`（macOS 版の `WorkingCopyViewController`）と同じ並び。
//!
//! チェックは index の状態そのもので、FE だけの選択状態は持たない（ターミナルで `git add` しても揃う）。

use amonhen_core::StatusSnapshot;
use amonhen_model::{StatusEntry, StatusKind, tr};
use windows_reactor::*;

/// 画面の中身を作るのに要るもの。
/// 件名と本文をコミットのメッセージにする（本文があれば空行で分ける）。
pub fn message(summary: &str, description: &str) -> String {
    let (summary, description) = (summary.trim(), description.trim());
    if description.is_empty() {
        summary.to_owned()
    } else {
        format!("{summary}\n\n{description}")
    }
}

pub struct Input<'a> {
    pub status: Option<&'a StatusSnapshot>,
    /// 件名と本文。
    pub summary: &'a str,
    pub description: &'a str,
    /// コミットしている最中（Commit を押せない）。
    pub committing: bool,
}

/// 画面が受ける操作。
pub struct Callbacks<'a> {
    pub summary: Callback<String>,
    pub description: Callback<String>,
    pub commit: Callback<()>,
    /// パスのファイルを含める（true）・外す（false）。
    pub stage: Box<dyn Fn(String) -> Callback<bool> + 'a>,
    pub selected: Callback<Option<usize>>,
}

/// 一覧の 1 行の印（M・A・D・R・?・U）。作業ツリー側に変更があればそちら、なければ index 側。
fn mark(entry: &StatusEntry) -> char {
    match entry.kind {
        StatusKind::Untracked => '?',
        StatusKind::Unmerged => 'U',
        StatusKind::Renamed => 'R',
        StatusKind::Changed => {
            let side = if entry.worktree != b'.' {
                entry.worktree
            } else {
                entry.index
            };
            char::from(side)
        }
    }
}

/// 次のコミットに含めているか（stage 済みで、作業ツリーに残りがない）。
fn included(entry: &StatusEntry) -> bool {
    entry.is_staged() && !entry.is_unstaged()
}

pub fn view(input: Input, callbacks: Callbacks) -> View {
    let entries: &[StatusEntry] = input.status.map_or(&[], |s| &s.status.entries);
    let staged = entries.iter().filter(|e| e.is_staged()).count();
    // 件名と本文（macOS 版と同じく、件名は 1 行、本文は空なら薄く「Description (optional)」）。
    let message = StackPanel::new().spacing(6.0).grid_row(0).children((
        TextBox::new()
            .text(input.summary)
            .placeholder_text("Summary")
            .on_text_changed(callbacks.summary),
        TextBox::new()
            .text(input.description)
            .placeholder_text(tr!("Description (optional)"))
            .accepts_return(true)
            .text_wrapping(TextWrapping::Wrap)
            .height(80.0)
            .on_text_changed(callbacks.description),
    ));
    let can_commit = !input.committing && staged > 0 && !input.summary.trim().is_empty();
    let commit = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .margin(Thickness::new(0.0, 8.0, 0.0, 8.0))
        .grid_row(1)
        .children((
            TextBlock::new()
                .text(format!("{staged} / {}", entries.len()))
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0),
            Button::new()
                .is_enabled(can_commit)
                .on_click(callbacks.commit)
                .grid_column(1)
                .content(TextBlock::new().text("Commit")),
        ));
    let rows = entries.iter().map(|entry| {
        let label = match &entry.orig_path {
            Some(from) => format!("{}  {from} → {}", mark(entry), entry.path),
            None => format!("{}  {}", mark(entry), entry.path),
        };
        let row = Grid::new()
            .columns([GridLength::Auto, GridLength::Star(1.0)])
            .children((
                CheckBox::new()
                    .is_checked(included(entry))
                    .on_is_checked_changed((callbacks.stage)(entry.path.clone()))
                    .grid_column(0),
                TextBlock::new()
                    .text(label)
                    .vertical_alignment(VerticalAlignment::Center)
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .grid_column(1),
            ));
        (
            entry.path.clone(),
            ListViewItem::new().tag(entry.path.clone()).content(row),
        )
    });
    let list = ListView::new()
        .on_selection_changed(callbacks.selected)
        .collection_slot(ListViewSlot::Items, rows);
    // reactor の ListView は、キーつきの項目を消すと、その場所が空いたまま残る。status が替わるたびに一覧ごと
    // 作り直して避ける（替わるのは stage・コミット・外での変更のとき）。
    let generation = input.status.map_or(0, |s| s.generation);
    let list = Border::new()
        .grid_row(2)
        .content(View::keyed_fragment([(generation, list)]));
    Grid::new()
        .rows([GridLength::Auto, GridLength::Auto, GridLength::Star(1.0)])
        .margin(Thickness::new(14.0, 10.0, 14.0, 0.0))
        .children((message, commit, list))
}
