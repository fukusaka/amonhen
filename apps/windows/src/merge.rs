//! マージエディタ（GTK4 版の `merge.rs`、macOS 版の `MergeEditor.swift`）: コンフリクトのあるファイル 1 つを
//! 解決する。左に Current、真ん中に編集できる Result（今の作業ツリーの内容から始める）、右に Incoming（左右は、
//! すべてのコンフリクトをその側にした内容）。選んだコンフリクトを Accept Current・Incoming・Both で解き、
//! ✨ Magic Merge は決まった規則で解けるコンフリクトをまとめて解く。解決の計算はコアの `amonhen_core::conflict`、
//! 読むのは `mergeSides`、書くのは `saveMerge`。
//!
//! 共通の祖先（diff3）があれば、Base を左端に出せる（GTK4 版と同じく、出すかは開くたびに切った状態から）。
//!
//! まだないもの: 列の間の帯と ≫・≪、コンフリクトの色・位置で揃えるスクロール、等幅のフォント（reactor の TextBox
//! にない）、AI、別の窓。

use amonhen_core::conflict::{self, Choice, MagicRules};
use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// 両側と共通の祖先の中身（Current、Incoming、Base）。
pub type Sides = (String, String, Option<String>);

pub struct State {
    pub path: String,
    /// 読んでいる間は None。読めなければ理由。Current、Incoming、Base（共通の祖先があれば）。
    pub sides: Option<Result<Sides, String>>,
    /// Base の欄を出すか。
    pub show_base: bool,
    /// マーカーの残っていないファイル（rerere が解決を当てた、ほかのエディタで直した）の案内。
    pub banner: Option<String>,
    /// rerere が解決を当てた（忘れてコンフリクトからやり直せる）。
    pub rerere: bool,
    /// 編集している Result と、最後に保存した（開いた）Result。
    pub result: String,
    pub saved: String,
    /// 選んでいるコンフリクト（Result の中の番号）。
    pub selected: usize,
    /// 知らせ（Magic Merge で解いた数など）。
    pub note: String,
    /// ファイルの改行が CRLF か（Result の欄から戻る文字の改行を、これに揃える）。
    pub crlf: bool,
    /// Result の欄に打ったままの文字（None なら `result` を出す）。揃えた値で欄を書き直すと、打つたびにカーソルが
    /// 先頭に戻るので、打っている間は欄の文字をそのまま返す。
    pub typed: Option<String>,
}

impl State {
    pub fn new(path: String) -> Self {
        Self {
            path,
            sides: None,
            show_base: false,
            banner: None,
            rerere: false,
            result: String::new(),
            saved: String::new(),
            selected: 0,
            note: String::new(),
            crlf: false,
            typed: None,
        }
    }

    /// `mergeSides` の答えを取り込む。
    pub fn load(&mut self, info: Result<Value, String>) {
        match info {
            Ok(info) => {
                let text = |k: &str| info[k]["text"].as_str().unwrap_or_default().to_owned();
                self.result = info["result"].as_str().unwrap_or_default().to_owned();
                self.typed = None;
                self.crlf = self.result.contains("\r\n");
                self.saved = self.result.clone();
                let base = info["base"]["text"].as_str().map(str::to_owned);
                // マーカーが残っていなければ、その案内（GTK4 版の `load` と同じ）。
                self.rerere = info["rerere"].as_bool() == Some(true);
                self.banner = (info["markers"].as_u64() == Some(0)).then(|| {
                    if self.rerere {
                        tr!(
                            "git rerere applied the earlier resolution. Check the result, then Mark Resolved."
                        )
                    } else {
                        tr!("No conflict markers are left. Check the result, then Mark Resolved.")
                    }
                });
                self.sides = Some(Ok((text("current"), text("incoming"), base)));
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
        self.typed = Some(text.to_owned());
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
            self.typed = None;
            self.clamp();
            self.note.clear();
        }
    }

    /// 決まった規則で解けるコンフリクトをまとめて解く（GTK4 版の `apply_magic` の All）。
    pub fn magic(&mut self) {
        let (merged, solved) = conflict::magic(self.result.as_bytes(), MagicRules::All, None);
        if solved > 0 {
            self.result = String::from_utf8_lossy(&merged).into_owned();
            self.typed = None;
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
    /// 操作のボタン（`accept-ours`・`accept-theirs`・`accept-both`・`accept-base`・`magic`・`previous`・
    /// `next`・`save`・`resolve`・`forget`・`close`）。
    pub button: Box<dyn Fn(&'static str) -> Callback<()> + 'a>,
    pub result: Callback<String>,
    /// Base の欄を出すか。
    pub base: Callback<bool>,
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
    let has_base = matches!(&state.sides, Some(Ok((_, _, Some(_)))));
    let accept_base: View = if has_base {
        button(
            "accept-base",
            amonhen_model::i18n::translate("Accept Base").into_owned(),
            has,
        )
    } else {
        View::empty()
    };
    let base_toggle: View = if has_base {
        CheckBox::new()
            .is_checked(state.show_base)
            .on_is_checked_changed(callbacks.base.clone())
            .content(TextBlock::new().text("Base"))
            .tooltip(tr!("Show the common ancestor on the left."))
    } else {
        View::empty()
    };
    let bar = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(6.0)
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
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
            accept_base,
            base_toggle,
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
            .grid_row(2)
            .into(),
        Some(Err(e)) => TextBlock::new()
            .text(e.clone())
            .text_wrapping(TextWrapping::Wrap)
            .margin(16.0)
            .grid_row(2)
            .into(),
        Some(Ok((current, incoming, base))) => {
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
            // 列: Base（出すとき）・Current・Result・Incoming（同じ幅に分ける）。
            let base = base.as_deref().filter(|_| state.show_base);
            let first = i32::from(base.is_some());
            let base_pane: View = match base {
                Some(text) => pane(tr!("Base (common ancestor)"), text, 0, false),
                None => View::empty(),
            };
            Grid::new()
                .columns(vec![GridLength::Star(1.0); 3 + first as usize])
                .grid_row(2)
                .children((
                    base_pane,
                    pane(tr!("Current (your branch)"), current, first, false),
                    pane(
                        tr!("Result (editable)"),
                        state.typed.as_deref().unwrap_or(&state.result),
                        first + 1,
                        true,
                    ),
                    pane(
                        tr!("Incoming (being merged in)"),
                        incoming,
                        first + 2,
                        false,
                    ),
                ))
        }
    };
    // マーカーの残っていないファイルの案内。rerere が当てた解決が違っていれば、忘れてコンフリクトからやり直せる
    // （GTK4 版・macOS 版と同じ）。
    let banner: View = match &state.banner {
        Some(text) => {
            let forget: View = if state.rerere {
                Button::new()
                    .on_click((callbacks.button)("forget"))
                    .vertical_alignment(VerticalAlignment::Center)
                    .content(TextBlock::new().text(tr!("Forget This Resolution")))
                    .tooltip(tr!(
                        "Forgets what git rerere recorded for this file and brings its conflict back (git rerere forget). Undo brings back the file as it is now."
                    ))
            } else {
                View::empty()
            };
            Border::new()
                .background(Brush::Solid(Color::rgb(0xff, 0xf4, 0xce)))
                .padding(Thickness::new(10.0, 6.0, 10.0, 6.0))
                .grid_row(1)
                .content(
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(12.0)
                        .children((
                            TextBlock::new()
                                .text(text.clone())
                                .vertical_alignment(VerticalAlignment::Center),
                            forget,
                        )),
                )
        }
        None => View::empty(),
    };
    Grid::new()
        .rows([GridLength::Auto, GridLength::Auto, GridLength::Star(1.0)])
        .children((
            // 窓が狭くても右端のボタン（閉じる）に届くよう、帯は横にスクロールできる。
            ScrollViewer::new()
                .horizontal_scroll_bar_visibility(ScrollBarVisibility::Auto)
                .vertical_scroll_bar_visibility(ScrollBarVisibility::Disabled)
                .grid_row(0)
                .content(bar),
            banner,
            body,
        ))
}
