//! Range Diff の画面（GTK4 版の `rangediff.rs`、macOS 版の `RangeDiffViewController`）: 同じ一連のコミットの 2 つの
//! 版（rebase の前と後など）を比べる（`git range-diff`）。左の上に比べる範囲（書き直して比べ直せる）、その下に組
//! （同じ・変わった・消えた・足された）の一覧。右の列には、変わった組なら差分の差分、それ以外ならそのコミットの
//! 中身（History の詳細と差分の欄）を出す。開いている間だけ、サイドバーの Workspace に Range Diff の行を出す。
//! まだないもの: Changes Only、範囲の編集の補助、History の 2 つのコミットからの比較。

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// 組の片側（範囲の中の 1 始まりの番号と、コミット ID）。
#[derive(Clone, PartialEq)]
pub struct Side {
    pub index: u64,
    pub oid: String,
}

/// 組 1 つ（`rangeDiff` の答えの `pairs` の 1 つ）。
#[derive(Clone, PartialEq)]
pub struct Pair {
    /// `same`・`changed`・`removed`・`added`。
    pub status: String,
    pub old: Option<Side>,
    pub new: Option<Side>,
    pub subject: String,
    /// 差分の差分（changed のときだけ）。
    pub diff: Vec<String>,
}

pub fn pairs_of(info: &Value) -> Vec<Pair> {
    let side = |v: &Value| {
        Some(Side {
            index: v["index"].as_u64()?,
            oid: v["oid"].as_str()?.to_owned(),
        })
    };
    info["pairs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| Pair {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            old: side(&v["old"]),
            new: side(&v["new"]),
            subject: v["subject"].as_str().unwrap_or_default().to_owned(),
            diff: v["diff"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l.as_str().map(str::to_owned))
                .collect(),
        })
        .collect()
}

/// 組の印（git range-diff と同じ記号）、色、どうなったか（GTK4 版の `mark`）。
fn mark(status: &str) -> (&'static str, Option<Color>, String) {
    match status {
        "changed" => ("!", Some(Color::rgb(0xe6, 0x61, 0x00)), tr!("changed")),
        "removed" => ("<", Some(Color::rgb(0xc0, 0x1c, 0x28)), tr!("removed")),
        "added" => (">", Some(Color::rgb(0x26, 0xa2, 0x69)), tr!("added")),
        _ => ("=", None, tr!("same")),
    }
}

/// 40 桁の ID は短くして入力欄に出す（そのまま比べ直せる長さに。GTK4 版の `shorten`）。
pub fn shorten(range: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for c in range.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_hexdigit() {
            run.push(c);
            continue;
        }
        if run.len() == 40 {
            out.push_str(&run[..12]);
        } else {
            out.push_str(&run);
        }
        run.clear();
        out.push(c);
    }
    out.pop();
    out
}

pub struct State {
    /// 入力欄の範囲（空白区切り）と、その下の説明。
    pub ranges: String,
    pub summary: String,
    /// 比べた組（比べている間・比べられなかったときは None と理由）。
    pub pairs: Option<Vec<Pair>>,
    pub error: Option<String>,
    /// 選んでいる組（`pairs` の番号）。
    pub selected: Option<usize>,
    /// 両方で同じ組を隠す（Changes Only。GTK4 版と同じ）。
    pub changes_only: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            ranges: String::new(),
            summary: String::new(),
            pairs: None,
            error: None,
            selected: None,
            changes_only: false,
        }
    }

    /// 一覧に出している組の番号（Changes Only なら同じ組を除く）。
    pub fn shown(&self) -> Vec<usize> {
        self.pairs
            .iter()
            .flatten()
            .enumerate()
            .filter(|(_, p)| !self.changes_only || p.status != "same")
            .map(|(i, _)| i)
            .collect()
    }

    pub fn selected_pair(&self) -> Option<&Pair> {
        self.pairs.as_ref()?.get(self.selected?)
    }
}

pub struct Callbacks {
    pub ranges: Callback<String>,
    pub compare: Callback<()>,
    /// 一覧の行を選んだ（`shown` の番号）。
    pub selected: Callback<Option<usize>>,
    pub close: Callback<()>,
    pub changes_only: Callback<bool>,
}

/// 左の列（範囲と組の一覧）。
pub fn view(state: &State, callbacks: Callbacks) -> View {
    let head = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto, GridLength::Auto])
        .margin(Thickness::new(8.0, 6.0, 8.0, 2.0))
        .grid_row(0)
        .children((
            TextBox::new()
                .text(state.ranges.clone())
                .placeholder_text("A..B C..D")
                .on_text_changed(callbacks.ranges)
                .grid_column(0),
            crate::small::button(tr!("Compare"), |b| {
                b.on_click(callbacks.compare)
                    .margin(Thickness::new(6.0, 0.0, 0.0, 0.0))
                    .grid_column(1)
            }),
            crate::small::button(tr!("Close"), |b| {
                b.on_click(callbacks.close)
                    .margin(Thickness::new(6.0, 0.0, 0.0, 0.0))
                    .grid_column(2)
            }),
        ));
    let counts = match (&state.pairs, &state.error) {
        (_, Some(e)) => e.clone(),
        (None, None) => tr!("Comparing…"),
        (Some(pairs), None) => {
            let count = |s: &str| pairs.iter().filter(|p| p.status == s).count();
            format!(
                "= {}  ! {}  < {}  > {}",
                count("same"),
                count("changed"),
                count("removed"),
                count("added")
            )
        }
    };
    let summary = StackPanel::new()
        .spacing(2.0)
        .margin(Thickness::new(10.0, 2.0, 8.0, 6.0))
        .grid_row(1)
        .children((
            TextBlock::new()
                .text(state.summary.clone())
                .font_size(12.0)
                .opacity(0.7)
                .text_wrapping(TextWrapping::Wrap),
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .children((
                    TextBlock::new()
                        .text(counts)
                        .font_size(12.0)
                        .opacity(0.7)
                        .vertical_alignment(VerticalAlignment::Center)
                        .text_wrapping(TextWrapping::Wrap)
                        .grid_column(0),
                    crate::small::check(CheckBox::new())
                        .is_checked(state.changes_only)
                        .on_is_checked_changed(callbacks.changes_only)
                        .grid_column(1)
                        .content(TextBlock::new().text(tr!("Changes Only")).font_size(12.0))
                        .tooltip(tr!("Hide the commits that are the same in both versions")),
                )),
        ));
    let all = state.pairs.as_deref().unwrap_or_default();
    let shown = state.shown();
    let rows = shown
        .iter()
        .filter_map(|&i| Some((i, all.get(i)?)))
        .map(|(i, p)| {
            let (sign, color, what) = mark(&p.status);
            let index =
                |s: &Option<Side>| s.as_ref().map_or("-".to_owned(), |s| s.index.to_string());
            let sign = TextBlock::new()
                .text(sign)
                .foreground_optional(color.map(Brush::Solid))
                .grid_column(0)
                .tooltip(what);
            let row = Grid::new()
                .columns([
                    GridLength::Pixel(20.0),
                    GridLength::Pixel(56.0),
                    GridLength::Star(1.0),
                ])
                .children((
                    sign,
                    TextBlock::new()
                        .text(format!("{}:{}", index(&p.old), index(&p.new)))
                        .font_size(12.0)
                        .opacity(0.6)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(1),
                    TextBlock::new()
                        .text(p.subject.clone())
                        .text_trimming(TextTrimming::CharacterEllipsis)
                        .grid_column(2),
                ));
            (i, ListViewItem::new().content(row))
        });
    let list = ListView::new()
        .selected_index(
            state
                .selected
                .and_then(|s| shown.iter().position(|&i| i == s)),
        )
        .on_selection_changed(callbacks.selected)
        .collection_slot(ListViewSlot::Items, rows);
    // 比べ直すたびに一覧ごと作り直す（reactor の ListView は消した項目の場所が空いたまま残る）。
    let key = format!("{}:{}:{}", state.ranges, all.len(), state.changes_only);
    Grid::new()
        .rows([GridLength::Auto, GridLength::Auto, GridLength::Star(1.0)])
        .children((
            head,
            summary,
            Border::new()
                .grid_row(2)
                .content(View::keyed_fragment([(key, list)])),
        ))
}

/// 右の列に重ねる、変わった組の差分の差分（先頭の 1 文字が外側の記号、残りが内側の行）。
pub fn diff_view(pair: &Pair) -> View {
    let lines = pair.diff.iter().enumerate().map(|(i, line)| {
        let outer = line.chars().next().unwrap_or(' ');
        let background = match outer {
            '+' => crate::palette::diff_added(),
            '-' => crate::palette::diff_removed(),
            '@' => crate::palette::diff_hunk(),
            _ => crate::palette::background(),
        };
        (
            i,
            Border::new().background(Brush::Solid(background)).content(
                TextBlock::new()
                    .text(line.replace('\t', "    "))
                    .font_size(12.0)
                    .margin(Thickness::new(8.0, 1.0, 8.0, 1.0)),
            ),
        )
    });
    StackPanel::new().children((
        TextBlock::new()
            .text(pair.subject.clone())
            .font_size(14.0)
            .margin(Thickness::new(12.0, 8.0, 12.0, 8.0)),
        ScrollViewer::new().content(StackPanel::new().children((View::keyed_fragment(lines),))),
    ))
}
