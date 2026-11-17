//! マージエディタ（GTK4 版の `merge.rs`、macOS 版の `MergeEditor.swift`）: コンフリクトのあるファイル 1 つを
//! 解決する。左に Current、真ん中に編集できる Result（今の作業ツリーの内容から始める）、右に Incoming（左右は、
//! すべてのコンフリクトをその側にした内容）。選んだコンフリクトを Accept Current・Incoming・Both で解き、
//! ✨ Magic Merge は決まった規則で解けるコンフリクトをまとめて解く。解決の計算はコアの `amonhen_core::conflict`、
//! 読むのは `mergeSides`、書くのは `saveMerge`。
//!
//! まだないもの: 列の間の帯と ≫・≪、コンフリクトの色・位置で揃えるスクロール、Base の欄、等幅のフォント（reactor
//! の TextBox にない）、AI、別の窓。

use amonhen_core::conflict::{self, Choice, MagicRules};
use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

pub struct State {
    pub path: String,
    /// 読んでいる間は None。読めなければ理由。
    pub sides: Option<Result<(String, String), String>>,
    /// 編集している Result と、最後に保存した（開いた）Result。
    pub result: String,
    pub saved: String,
    /// 選んでいるコンフリクト（Result の中の番号）。
    pub selected: usize,
    /// 知らせ（Magic Merge で解いた数など）。
    pub note: String,
    /// ファイルの改行が CRLF か（Result の欄から戻る文字の改行を、これに揃える）。
    pub crlf: bool,
}

impl State {
    pub fn new(path: String) -> Self {
        Self {
            path,
            sides: None,
            result: String::new(),
            saved: String::new(),
            selected: 0,
            note: String::new(),
            crlf: false,
        }
    }

    /// `mergeSides` の答えを取り込む。
    pub fn load(&mut self, info: Result<Value, String>) {
        match info {
            Ok(info) => {
                let text = |k: &str| info[k]["text"].as_str().unwrap_or_default().to_owned();
                self.result = info["result"].as_str().unwrap_or_default().to_owned();
                self.crlf = self.result.contains("\r\n");
                self.saved = self.result.clone();
                self.sides = Some(Ok((text("current"), text("incoming"))));
                self.selected = 0;
                // 開いたときに、間違えようのないコンフリクトを解く（設定。GTK4 版と同じく、保存はしない）。
                if crate::prefs::bool(crate::prefs::AUTO_MAGIC_MERGE) {
                    let (merged, solved) =
                        conflict::magic(self.result.as_bytes(), MagicRules::Safe, None);
                    if solved > 0 {
                        self.result = String::from_utf8_lossy(&merged).into_owned();
                        self.note = tr!(
                            "Resolved {count} when opened (Ctrl+Z to undo).",
                            count = solved
                        );
                    }
                }
            }
            Err(e) => self.sides = Some(Err(e)),
        }
    }

    /// Result の欄で書き換えた文字を取り込む。WinUI の TextBox は改行を `\r` で返すので、ファイルの改行に戻す。
    pub fn edit(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.result = if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        };
        self.clamp();
    }

    /// Result に残っているコンフリクトの数。
    pub fn conflicts(&self) -> usize {
        conflict::parse(self.result.as_bytes()).len()
    }

    pub fn dirty(&self) -> bool {
        self.result != self.saved
    }

    /// 選んでいるコンフリクトを `choice` で解く（次のコンフリクトが同じ番号に来る）。
    pub fn accept(&mut self, choice: Choice) {
        if let Some(text) = conflict::resolve(self.result.as_bytes(), self.selected, choice) {
            self.result = String::from_utf8_lossy(&text).into_owned();
            self.clamp();
            self.note.clear();
        }
    }

    /// 決まった規則で解けるコンフリクトをまとめて解く（GTK4 版の `apply_magic` の All）。
    pub fn magic(&mut self) {
        let (merged, solved) = conflict::magic(self.result.as_bytes(), MagicRules::All, None);
        if solved > 0 {
            self.result = String::from_utf8_lossy(&merged).into_owned();
            self.clamp();
        }
        // Undo は 1 つずつではなく、Result の欄の Ctrl+Z でまとめて戻る（GTK4 版の文言の後半は当てはまらない）。
        self.note = if solved > 0 {
            format!("✨ {solved}")
        } else {
            tr!("Nothing to resolve")
        };
    }

    pub fn step(&mut self, forward: bool) {
        let count = self.conflicts();
        if count == 0 {
            return;
        }
        self.selected = if forward {
            (self.selected + 1) % count
        } else {
            (self.selected + count - 1) % count
        };
    }

    fn clamp(&mut self) {
        let count = self.conflicts();
        self.selected = self.selected.min(count.saturating_sub(1));
    }
}

pub struct Callbacks<'a> {
    /// 操作のボタン（`accept-ours`・`accept-theirs`・`accept-both`・`magic`・`previous`・`next`・`save`・
    /// `resolve`・`close`）。
    pub button: Box<dyn Fn(&'static str) -> Callback<()> + 'a>,
    pub result: Callback<String>,
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let count = state.conflicts();
    let ready = matches!(state.sides, Some(Ok(_)));
    let position = if count == 0 {
        tr!("No conflicts")
    } else {
        tr!(
            "Conflict {index} of {count}",
            index = state.selected + 1,
            count = count
        )
    };
    let button = |key: &'static str, title: String, enabled: bool| -> View {
        Button::new()
            .is_enabled(enabled)
            .on_click((callbacks.button)(key))
            .content(TextBlock::new().text(title))
    };
    let has = ready && count > 0;
    let bar = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(6.0)
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            TextBlock::new()
                .text(format!("{} — {position}", state.path))
                .vertical_alignment(VerticalAlignment::Center),
            button("previous", "∧".to_owned(), has),
            button("next", "∨".to_owned(), has),
            button("magic", "✨ Magic Merge".to_owned(), has),
            button(
                "accept-ours",
                amonhen_model::i18n::translate("Accept Current").into_owned(),
                has,
            ),
            button(
                "accept-theirs",
                amonhen_model::i18n::translate("Accept Incoming").into_owned(),
                has,
            ),
            button(
                "accept-both",
                amonhen_model::i18n::translate("Accept Both").into_owned(),
                has,
            ),
            button("save", tr!("Save"), ready && state.dirty()),
            button("resolve", tr!("Save & Mark Resolved"), ready),
            button("close", tr!("Close"), true),
            TextBlock::new()
                .text(state.note.clone())
                .opacity(0.6)
                .vertical_alignment(VerticalAlignment::Center),
        ));
    let body: View = match &state.sides {
        None => TextBlock::new()
            .text(tr!("Loading…"))
            .opacity(0.6)
            .margin(16.0)
            .grid_row(1)
            .into(),
        Some(Err(e)) => TextBlock::new()
            .text(e.clone())
            .text_wrapping(TextWrapping::Wrap)
            .margin(16.0)
            .grid_row(1)
            .into(),
        Some(Ok((current, incoming))) => {
            let pane = |title: String, text: &str, column: i32, editable: bool| -> View {
                // 左右は読むだけ（reactor の TextBox に読むだけの設定がないので、選べる TextBlock）。
                let content: View = if editable {
                    TextBox::new()
                        .text(text)
                        .accepts_return(true)
                        .on_text_changed(callbacks.result.clone())
                        .into()
                } else {
                    TextBlock::new()
                        .text(text)
                        .font_size(12.0)
                        .is_text_selection_enabled(true)
                        .margin(Thickness::new(10.0, 6.0, 6.0, 6.0))
                        .into()
                };
                Grid::new()
                    .rows([GridLength::Auto, GridLength::Star(1.0)])
                    .margin(Thickness::new(4.0, 0.0, 4.0, 4.0))
                    .grid_column(column)
                    .children((
                        TextBlock::new()
                            .text(title)
                            .font_size(12.0)
                            .opacity(0.7)
                            .margin(Thickness::new(2.0, 0.0, 0.0, 4.0))
                            .grid_row(0),
                        ScrollViewer::new().grid_row(1).content(content),
                    ))
            };
            Grid::new()
                .columns([
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                ])
                .grid_row(1)
                .children((
                    pane(tr!("Current (your branch)"), current, 0, false),
                    pane(tr!("Result (editable)"), &state.result, 1, true),
                    pane(tr!("Incoming (being merged in)"), incoming, 2, false),
                ))
        }
    };
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((bar, body))
}
