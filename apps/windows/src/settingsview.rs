//! Settings の画面（GTK4 版の設定の窓のうち、Windows 版に機能のあるもの）。Git は、すべてのリポジトリの段
//! （`--global`）の名前とメールだけ。GTK4 版・macOS 版は別の窓とタブにするが、ここではまだ History と右の列の
//! 場所に重ねる。

use amonhen_model::tr;
use windows_reactor::*;

pub struct State {
    /// `--global` の user.name・user.email（読んだものと、欄の今の値）。読むまでは None。
    pub git: Option<[(String, String); 2]>,
    pub auto_magic_merge: bool,
    pub file_history_merges: bool,
    /// 自動の fetch の間隔（分。0 はしない）。
    pub auto_fetch: u32,
}

/// 自動の fetch の間隔の選べる値（GTK4 版・macOS 版と同じ）。
pub const AUTO_FETCH_CHOICES: [u32; 5] = [0, 5, 15, 30, 60];

pub struct Callbacks<'a> {
    pub field: Box<dyn Fn(usize) -> Callback<String> + 'a>,
    pub save_git: Callback<()>,
    pub toggle: Box<dyn Fn(&'static str) -> Callback<bool> + 'a>,
    pub close: Callback<()>,
    /// 自動の fetch の間隔を選んだ（`AUTO_FETCH_CHOICES` の番号）。
    pub auto_fetch: Callback<Option<usize>>,
}

fn heading(text: String) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(15.0)
        .margin(Thickness::new(0.0, 16.0, 0.0, 6.0))
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let head = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            TextBlock::new()
                .text("Settings")
                .font_size(14.0)
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0),
            Button::new()
                .on_click(callbacks.close)
                .grid_column(1)
                .content(TextBlock::new().text(tr!("Close"))),
        ));
    let git: View = match &state.git {
        None => TextBlock::new().text(tr!("Loading…")).opacity(0.6).into(),
        Some(fields) => {
            let changed = fields.iter().any(|(read, now)| read != now);
            let field = |index: usize, label: String| {
                StackPanel::new().spacing(2.0).children((
                    TextBlock::new().text(label).font_size(12.0).opacity(0.75),
                    TextBox::new()
                        .text(fields[index].1.clone())
                        .on_text_changed((callbacks.field)(index)),
                ))
            };
            StackPanel::new().spacing(8.0).children((
                field(0, tr!("Name")),
                field(1, tr!("Email")),
                TextBlock::new()
                    .text("git config --global user.name / user.email")
                    .font_size(12.0)
                    .opacity(0.6),
                Button::new()
                    .is_enabled(changed)
                    .on_click(callbacks.save_git)
                    .content(TextBlock::new().text(tr!("Save"))),
            ))
        }
    };
    let check = |key: &'static str, value: bool, label: String| {
        CheckBox::new()
            .is_checked(value)
            .on_is_checked_changed((callbacks.toggle)(key))
            .content(TextBlock::new().text(label))
    };
    let body = ScrollViewer::new().grid_row(1).content(
        StackPanel::new()
            .max_width(560.0)
            .horizontal_alignment(HorizontalAlignment::Left)
            .margin(Thickness::new(24.0, 0.0, 24.0, 24.0))
            .children((
                heading(tr!("General")),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(12.0)
                    .children((
                        TextBlock::new()
                            .text(tr!("Fetch automatically"))
                            .vertical_alignment(VerticalAlignment::Center),
                        ComboBox::new()
                            .items_source(
                                AUTO_FETCH_CHOICES
                                    .iter()
                                    .map(|&m| {
                                        if m == 0 {
                                            tr!("Never")
                                        } else {
                                            tr!("Every {count} minutes", count = m)
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .selected_index(
                                AUTO_FETCH_CHOICES
                                    .iter()
                                    .position(|&m| m == state.auto_fetch)
                                    .unwrap_or(0),
                            )
                            .on_selection_changed(callbacks.auto_fetch),
                    )),
                heading("Git".to_owned()),
                git,
                heading(tr!("Diff & Merge")),
                check(
                    crate::prefs::AUTO_MAGIC_MERGE,
                    state.auto_magic_merge,
                    tr!("Resolve unambiguous conflicts when the merge editor opens"),
                ),
                heading("File History".to_owned()),
                check(
                    crate::prefs::FILE_HISTORY_MERGES,
                    state.file_history_merges,
                    tr!("Show merge commits"),
                ),
            )),
    );
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((head, body))
}
