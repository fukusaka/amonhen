//! File History の画面: 見出し（パスとコミットの数）と、ファイルを変えたコミットの一覧（新しい順。名前の変更も
//! 追う）。選んだコミットでのそのファイルの差分は、右の列（History の詳細と差分の欄）に出す。GTK4 版の
//! `filetools.rs` の File History（macOS 版の `FileTools.swift`）と同じ中身。GTK4 版・macOS 版はタブにするが、
//! ここではまだ History の場所に重ねる。

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// `fileHistory` の答えのコミット。
#[derive(Clone, PartialEq)]
pub struct Entry {
    pub oid: String,
    pub subject: String,
    pub author: String,
    pub time: i64,
    /// そのコミットでのファイルの名前（名前を変える前は前の名前）。
    pub path: String,
}

pub fn entries_of(info: &Value) -> Vec<Entry> {
    info["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Entry {
            oid: e["oid"].as_str().unwrap_or_default().to_owned(),
            subject: e["subject"].as_str().unwrap_or_default().to_owned(),
            author: e["author"].as_str().unwrap_or_default().to_owned(),
            time: e["time"].as_i64().unwrap_or_default(),
            path: e["path"].as_str().unwrap_or_default().to_owned(),
        })
        .collect()
}

/// 読んでいる・読んだ File History。
pub struct State {
    pub path: String,
    /// 読み終えるまでは None。
    pub entries: Option<Result<Vec<Entry>, String>>,
    pub selected: Option<usize>,
}

pub struct Callbacks<'a> {
    pub selected: Callback<Option<usize>>,
    pub close: Callback<()>,
    /// 行の右クリックのメニュー（`blame:<oid>`）。
    pub menu: Box<dyn Fn() -> Callback<String> + 'a>,
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let title = match &state.entries {
        Some(Ok(entries)) => tr!(
            "File History — {path} ({count} commits)",
            path = &state.path,
            count = entries.len()
        ),
        _ => tr!("File History — {path} (loading…)", path = &state.path),
    };
    let head = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            TextBlock::new()
                .text(title)
                .font_size(14.0)
                .vertical_alignment(VerticalAlignment::Center)
                .text_trimming(TextTrimming::CharacterEllipsis)
                .grid_column(0),
            crate::small::button(tr!("Close"), |b| b.on_click(callbacks.close).grid_column(1)),
        ));
    let body: View = match &state.entries {
        None => TextBlock::new()
            .text(tr!("Loading…"))
            .opacity(0.6)
            .margin(16.0)
            .into(),
        Some(Err(e)) => TextBlock::new()
            .text(e.clone())
            .text_wrapping(TextWrapping::Wrap)
            .margin(16.0)
            .into(),
        Some(Ok(entries)) if entries.is_empty() => TextBlock::new()
            .text(tr!("No commits changed this file."))
            .opacity(0.6)
            .margin(16.0)
            .into(),
        Some(Ok(entries)) => {
            let rows = entries.iter().map(|e| {
                // 名前を変える前のコミットには、そのときの名前も出す。
                let mut meta = format!(
                    "{} · {} · {}",
                    e.author,
                    crate::render::local_date(e.time),
                    &e.oid[..e.oid.len().min(8)]
                );
                if e.path != state.path {
                    meta.push_str(&format!(" · {}", e.path));
                }
                let row = StackPanel::new().spacing(1.0).children((
                    TextBlock::new()
                        .text(e.subject.clone())
                        .text_trimming(TextTrimming::CharacterEllipsis),
                    TextBlock::new().text(meta).font_size(11.0).opacity(0.7),
                ));
                // そのコミットの時点の Blame（GTK4 版の変更ファイルのメニューと同じ）。
                // その版に戻す・その前の版に戻す（GTK4 版の `restoresheet::items`）。
                let row = row.menu(Menu::new(
                    [
                        MenuItem::item(
                            format!("blame:{}", e.oid),
                            tr!("Blame at {commit}", commit = &e.oid[..e.oid.len().min(8)]),
                        ),
                        MenuItem::separator("-restore"),
                        MenuItem::item(format!("restore:{}", e.oid), tr!("Restore This Version…")),
                        MenuItem::item(
                            format!("restore-before:{}", e.oid),
                            tr!("Restore Version Before This Commit…"),
                        ),
                    ],
                    (callbacks.menu)(),
                ));
                (
                    e.oid.clone(),
                    ListViewItem::new().tag(e.oid.clone()).content(row),
                )
            });
            ListView::new()
                .selected_index(state.selected)
                .on_selection_changed(callbacks.selected)
                .collection_slot(ListViewSlot::Items, rows)
        }
    };
    // 読み直すたびに一覧ごと作り直す（reactor の ListView は消した項目の場所が空いたまま残る）。
    let key = format!(
        "{}:{}",
        state.path,
        state
            .entries
            .as_ref()
            .map_or(0, |e| e.as_ref().map_or(0, Vec::len))
    );
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((
            head,
            Border::new()
                .grid_row(1)
                .content(View::keyed_fragment([(key, body)])),
        ))
}
