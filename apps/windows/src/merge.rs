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
    /// 左右と Base の欄で、各コンフリクトがその欄の何行目から何行目か（0 始まり、終わりは含まない）。
    pub regions: [Vec<(usize, usize)>; 3],
    /// 開いたときのコンフリクトの両側（今残っているものが、左右の欄のどれに当たるかを決める）。
    originals: Vec<(String, String)>,
    /// Accept・Magic Merge などで書き換えた前と後（Result の欄の Ctrl+Z・Ctrl+Y で 1 つずつ行き来する。WinUI の
    /// TextBox は中身を書き換えると自分の Undo を忘れるので）。
    undo: Vec<(String, String)>,
    redo: Vec<(String, String)>,
    /// 別の窓に出したマージエディタか（⧉ を出さず、閉じたら窓を閉じる）。
    pub separate: bool,
}

/// Result に残っているコンフリクトの両側（GTK4 版の `regions`）。
fn found(text: &str) -> Vec<(String, String)> {
    let data = text.as_bytes();
    let ls = conflict::line_ranges(data);
    let join = |r: &std::ops::Range<usize>| -> String {
        r.clone()
            .map(|i| String::from_utf8_lossy(&data[ls[i].clone()]))
            .collect()
    };
    conflict::parse(data)
        .iter()
        .map(|c| (join(&c.ours), join(&c.theirs)))
        .collect()
}

/// `mergeSides` の欄の、コンフリクトの行範囲。
fn regions_of(side: &Value) -> Vec<(usize, usize)> {
    side["regions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r[0].as_u64()? as usize, r[1].as_u64()? as usize)))
        .collect()
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
            regions: Default::default(),
            originals: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            separate: false,
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
                self.regions = [
                    regions_of(&info["current"]),
                    regions_of(&info["incoming"]),
                    regions_of(&info["base"]),
                ];
                self.originals = found(&self.result);
                self.selected = 0;
                // 開いたときに、間違えようのないコンフリクトを解く（設定。GTK4 版と同じく、保存はしない）。
                if crate::prefs::bool(crate::prefs::AUTO_MAGIC_MERGE) {
                    let (merged, solved) =
                        conflict::magic(self.result.as_bytes(), MagicRules::Safe, None);
                    if solved > 0 {
                        let before = std::mem::replace(
                            &mut self.result,
                            String::from_utf8_lossy(&merged).into_owned(),
                        );
                        self.record(before);
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
    /// 今残っている各コンフリクトが、開いたときの何番目か（わからなければ None）。
    pub fn original_of(&self) -> Vec<Option<usize>> {
        conflict::match_originals(&self.originals, &found(&self.result))
    }

    /// 左右の欄の `original` 番目のコンフリクトを、その側（`choice`）で解決する（帯の ≫・≪）。
    pub fn pick(&mut self, original: usize, choice: Choice) -> bool {
        let Some(index) = self.original_of().iter().position(|o| *o == Some(original)) else {
            return false;
        };
        self.selected = index;
        self.accept(choice);
        true
    }

    pub fn conflicts(&self) -> usize {
        conflict::parse(self.result.as_bytes()).len()
    }

    pub fn dirty(&self) -> bool {
        self.result != self.saved
    }

    /// 選んでいるコンフリクトを `choice` で解く（次のコンフリクトが同じ番号に来る）。
    /// 書き換えた（`before` から今の Result へ）ことを覚える。
    fn record(&mut self, before: String) {
        if before != self.result {
            self.undo.push((before, self.result.clone()));
            self.redo.clear();
        }
    }

    /// Ctrl+Z で戻せる書き換えか（最後に覚えた書き換えの後、手で打っていない）。
    pub fn can_undo(&self) -> bool {
        self.undo
            .last()
            .is_some_and(|(_, after)| *after == self.result)
    }

    /// Ctrl+Y・Ctrl+Shift+Z でやり直せるか。
    pub fn can_redo(&self) -> bool {
        self.redo
            .last()
            .is_some_and(|(before, _)| *before == self.result)
    }

    /// 最後の書き換えを戻す（`redo` ならやり直す）。
    pub fn undo(&mut self, redo: bool) {
        let entry = if redo {
            self.redo.pop().filter(|(before, _)| *before == self.result)
        } else {
            self.undo.pop().filter(|(_, after)| *after == self.result)
        };
        let Some((before, after)) = entry else {
            return;
        };
        self.result = if redo { after.clone() } else { before.clone() };
        if redo {
            self.undo.push((before, after));
        } else {
            self.redo.push((before, after));
        }
        self.typed = None;
        self.clamp();
        self.note.clear();
    }

    pub fn accept(&mut self, choice: Choice) {
        if let Some(text) = conflict::resolve(self.result.as_bytes(), self.selected, choice) {
            let before = std::mem::replace(
                &mut self.result,
                String::from_utf8_lossy(&text).into_owned(),
            );
            self.record(before);
            self.typed = None;
            self.clamp();
            self.note.clear();
        }
    }

    /// 決まった規則で解けるコンフリクトをまとめて解く（GTK4 版の `apply_magic` の All）。
    pub fn magic(&mut self) {
        let (merged, solved) = conflict::magic(self.result.as_bytes(), MagicRules::All, None);
        if solved > 0 {
            let before = std::mem::replace(
                &mut self.result,
                String::from_utf8_lossy(&merged).into_owned(),
            );
            self.record(before);
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

    /// 選んでいるコンフリクトが Result の何割目の行から始まるか（Result の欄をそこへスクロールする）。
    pub fn selected_fraction(&self) -> Option<f64> {
        let text = self.typed.as_deref().unwrap_or(&self.result);
        let data = text.as_bytes();
        let lines = conflict::line_ranges(data).len();
        let start = conflict::parse(data).get(self.selected)?.lines.start;
        (lines > 0).then(|| start as f64 / lines as f64)
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
    /// 左右の欄のコンフリクトの ≫・≪（`ours:<番号>`・`theirs:<番号>`。番号は開いたときの何番目か）。
    pub pick: Box<dyn Fn(String) -> Callback<()> + 'a>,
    pub result: Callback<String>,
    /// Result の欄（Ctrl+Z・Ctrl+Y を付ける）。
    pub result_box: &'a ElementRef<TextBox>,
    /// 左右と Base の欄の、選んでいるコンフリクトの所（見える所へスクロールして、欄の位置をそろえる）。
    pub chosen: &'a [ElementRef<Grid>; 3],
    /// Base の欄を出すか。
    pub base: Callback<bool>,
    /// 表示の設定（View Options）の項目（今は `wrap-long-lines` だけ）。
    pub option: Callback<String>,
    /// 帯（狭いときにボタンを隠す。`fit_bar`）。
    pub bar: &'a ElementRef<Grid>,
}

/// 表示の設定（View Options）の項目。今は「コード」の「長い行を折り返す」だけ（macOS 版・GTK4 版と同じ）。
fn view_options_items() -> Vec<MenuItem> {
    let mark = if crate::data::wrap_lines() {
        "✓ "
    } else {
        ""
    };
    vec![
        MenuItem::disabled("h-code", tr!("Code")),
        MenuItem::item(
            "wrap-long-lines",
            format!("    {mark}{}", tr!("Wrap Long Lines")),
        )
        .with_tooltip(tr!(
            "Wrap lines wider than the diff instead of scrolling sideways (in every diff)"
        )),
    ]
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    // 長い行を折り返すか（アプリ全体の設定。折り返さなければ欄は横にスクロールする）。
    let wrap = crate::data::wrap_lines();
    let wrapping = if wrap {
        TextWrapping::Wrap
    } else {
        TextWrapping::NoWrap
    };
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
        crate::small::button(title, |b| {
            b.is_enabled(enabled).on_click((callbacks.button)(key))
        })
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
        crate::small::check(CheckBox::new())
            .is_checked(state.show_base)
            .on_is_checked_changed(callbacks.base.clone())
            .content(TextBlock::new().text("Base"))
            .tooltip(tr!("Show the common ancestor on the left."))
    } else {
        View::empty()
    };
    // 帯の並び（StackPanel の子の番号）。位置・∧・∨・✨・Accept の 3 つ（0〜6）、Base があれば Accept Base と
    // Base の切り替え、Save・Save & Mark Resolved・Close、埋め込みなら ⧉、知らせ。
    let (accept_base_at, toggle_at) = if has_base {
        (Some(7), Some(8))
    } else {
        (None, None)
    };
    let saves = if has_base { 9 } else { 7 };
    let popout_at = (!state.separate).then_some(saves + 3);
    // 狭いときに隠す順（macOS 版の fitBar と同じ: ⧉、✨、Base の切り替え、Accept の各ボタン）。
    let mut fold: Vec<usize> = popout_at.into_iter().collect();
    fold.push(3);
    fold.extend(toggle_at);
    fold.extend([4, 5, 6]);
    fold.extend(accept_base_at);
    FOLD.with(|f| *f.borrow_mut() = fold);
    let bar = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(6.0)
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .children((
            // Working Copy に埋め込んだときは、ファイルは左の一覧で選んでいるので位置だけ（帯を短くする）。
            TextBlock::new()
                .text(if state.separate {
                    format!("{} — {position}", state.path)
                } else {
                    position
                })
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
            // 別の窓に出す（GTK4 版・macOS 版の ⧉。打ちかけの Result も持って行く）。
            if state.separate {
                View::empty()
            } else {
                crate::small::button("⧉", |b| b.on_click((callbacks.button)("popout")))
                    .tooltip(tr!("Open in a Separate Window"))
            },
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
            let original_of = state.original_of();
            let chosen = original_of.get(state.selected).copied().flatten();
            // 左右と Base の欄: コンフリクトの所を塗る（Current は青、Incoming は緑、Base は灰。選んでいるものは
            // 濃く、解決済みは薄く）。左右の塗った所の ≫・≪ で、その側に解決する（GTK4 版の帯）。
            let side = |text: &str, which: usize| -> View {
                let lines: Vec<&str> = text.split_inclusive('\n').collect();
                let (r, g, b) = match which {
                    0 => (0x4d, 0x80, 0xe6),
                    1 => (0x33, 0xb3, 0x4d),
                    _ => (0x80, 0x80, 0x80),
                };
                let mut parts: Vec<(String, View)> = Vec::new();
                let mut at = 0;
                let plain = |from: usize, to: usize| -> View {
                    TextBlock::new()
                        .text(lines[from..to].concat().trim_end_matches('\n').to_owned())
                        .font_size(12.0)
                        .text_wrapping(wrapping)
                        .is_text_selection_enabled(true)
                        .margin(Thickness::new(10.0, 0.0, 6.0, 0.0))
                        .into()
                };
                for (i, &(start, end)) in state.regions[which].iter().enumerate() {
                    let (start, end) = (start.min(lines.len()), end.min(lines.len()));
                    if start < at {
                        continue;
                    }
                    if at < start {
                        parts.push((format!("t{i}"), plain(at, start)));
                    }
                    let open = original_of.contains(&Some(i));
                    let alpha = if chosen == Some(i) {
                        0x55
                    } else if open {
                        0x2e
                    } else {
                        0x14
                    };
                    let arrow: View = match (which, open) {
                        (0, true) => crate::small::button("≫", |b| {
                            b.on_click((callbacks.pick)(format!("ours:{i}")))
                                .horizontal_alignment(HorizontalAlignment::Right)
                                .vertical_alignment(VerticalAlignment::Top)
                        })
                        .tooltip(amonhen_model::i18n::translate("Accept Current").into_owned()),
                        (1, true) => crate::small::button("≪", |b| {
                            b.on_click((callbacks.pick)(format!("theirs:{i}")))
                                .horizontal_alignment(HorizontalAlignment::Left)
                                .vertical_alignment(VerticalAlignment::Top)
                        })
                        .tooltip(amonhen_model::i18n::translate("Accept Incoming").into_owned()),
                        _ => View::empty(),
                    };
                    let body = lines[start..end].concat();
                    let inner = if chosen == Some(i) {
                        Grid::new().element_ref(&callbacks.chosen[which])
                    } else {
                        Grid::new()
                    };
                    parts.push((
                        format!("c{i}:{alpha}"),
                        Border::new()
                            .background(Brush::Solid(Color::argb(alpha, r, g, b)))
                            .min_height(16.0)
                            .opacity(if open { 1.0 } else { 0.6 })
                            .content(
                                inner.children((
                                    TextBlock::new()
                                        .text(body.trim_end_matches('\n').to_owned())
                                        .font_size(12.0)
                                        .text_wrapping(wrapping)
                                        .is_text_selection_enabled(true)
                                        // ≫ は右、≪ は左に置くので、その側を空ける。
                                        .margin(if which == 1 {
                                            Thickness::new(30.0, 0.0, 10.0, 0.0)
                                        } else {
                                            Thickness::new(10.0, 0.0, 30.0, 0.0)
                                        }),
                                    arrow,
                                )),
                            ),
                    ));
                    at = end;
                }
                if at < lines.len() {
                    parts.push(("end".to_owned(), plain(at, lines.len())));
                }
                StackPanel::new()
                    .margin(Thickness::new(0.0, 6.0, 0.0, 6.0))
                    .children((View::keyed_fragment(parts),))
            };
            let pane = |title: String, text: &str, column: i32, editable: Option<usize>| -> View {
                // 左右は読むだけ（塗った文字の欄）。真ん中の Result は書き換えられる。
                let content: View = match editable {
                    None => TextBox::new()
                        .element_ref(callbacks.result_box)
                        .text(text)
                        .accepts_return(true)
                        .text_wrapping(wrapping)
                        .on_text_changed(callbacks.result.clone())
                        .into(),
                    Some(which) => side(text, which),
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
                        ScrollViewer::new()
                            .horizontal_scroll_bar_visibility(if wrap {
                                ScrollBarVisibility::Disabled
                            } else {
                                ScrollBarVisibility::Auto
                            })
                            .grid_row(1)
                            .content(content),
                    ))
            };
            // 列: Base（出すとき）・Current・Result・Incoming（同じ幅に分ける）。
            let base = base.as_deref().filter(|_| state.show_base);
            let first = i32::from(base.is_some());
            let base_pane: View = match base {
                Some(text) => pane(tr!("Base (common ancestor)"), text, 0, Some(2)),
                None => View::empty(),
            };
            Grid::new()
                .columns(vec![GridLength::Star(1.0); 3 + first as usize])
                .grid_row(2)
                .children((
                    base_pane,
                    pane(tr!("Current (your branch)"), current, first, Some(0)),
                    pane(
                        tr!("Result (editable)"),
                        state.typed.as_deref().unwrap_or(&state.result),
                        first + 1,
                        None,
                    ),
                    pane(
                        tr!("Incoming (being merged in)"),
                        incoming,
                        first + 2,
                        Some(1),
                    ),
                ))
        }
    };
    // マーカーの残っていないファイルの案内。rerere が当てた解決が違っていれば、忘れてコンフリクトからやり直せる
    // （GTK4 版・macOS 版と同じ）。
    let banner: View = match &state.banner {
        Some(text) => {
            let forget: View = if state.rerere {
                crate::small::button(tr!("Forget This Resolution"), |b| {
                    b.on_click((callbacks.button)("forget"))
                })
                .tooltip(tr!(
                        "Forgets what git rerere recorded for this file and brings its conflict back (git rerere forget). Undo brings back the file as it is now."
                    ))
            } else {
                View::empty()
            };
            Border::new()
                .background(Brush::Solid(crate::palette::note_band()))
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
            // 窓が狭ければ、後回しのボタンから隠す（`fit_bar`。macOS 版と同じ）。それでも入りきらなければ横に
            // スクロールできる。表示の設定は右端に置く（macOS 版・GTK4 版と同じ）。
            Grid::new()
                .element_ref(callbacks.bar)
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .grid_row(0)
                .children((
                    ScrollViewer::new()
                        .horizontal_scroll_bar_visibility(ScrollBarVisibility::Auto)
                        .vertical_scroll_bar_visibility(ScrollBarVisibility::Disabled)
                        .grid_column(0)
                        .content(bar),
                    DropDownButton::new()
                        .grid_column(1)
                        .vertical_alignment(VerticalAlignment::Center)
                        .margin(Thickness::new(0.0, 3.0, 8.0, 3.0))
                        .content(crate::small::drop_icon(crate::icons::icon(
                            "display-options",
                            16.0,
                        )))
                        .tooltip(tr!("View Options"))
                        .menu(Menu::new(view_options_items(), callbacks.option.clone())),
                )),
            banner,
            body,
        ))
}

thread_local! {
    /// 帯の、狭いときに隠すボタン（StackPanel の子の番号）を隠す順に（`view` が描くたびに決める）。
    static FOLD: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
    /// 帯の幅の変化の知らせ（帯の ScrollViewer ごとに 1 つ）。
    static RESIZED: std::cell::RefCell<Option<(usize, windows_core_reactor::EventRevoker)>> =
        const { std::cell::RefCell::new(None) };
}

/// 帯（`Callbacks::bar` の Grid）を今の幅に合わせ、幅が変わったら合わせ直すようにする（UI のスレッドで、描いた
/// 後に呼ぶ）。
pub fn fit_bar(element: &windows_core_reactor::IInspectable) {
    use amonhen_win_xaml as xaml;
    use windows_core_reactor::Interface;
    let Some(scroll) = element
        .cast::<xaml::DependencyObject>()
        .ok()
        .and_then(|grid| xaml::VisualTreeHelper::GetChild(&grid, 0).ok())
        .and_then(|child| child.cast::<xaml::ScrollViewer>().ok())
    else {
        return;
    };
    let id = scroll.as_raw() as usize;
    if RESIZED.with(|r| r.borrow().as_ref().map(|(i, _)| *i)) != Some(id)
        && let Ok(revoker) = scroll.cast::<xaml::FrameworkElement>().and_then(|f| {
            f.SizeChanged(|sender, _| {
                if let Some(scroll) = sender
                    .as_ref()
                    .and_then(|s| s.cast::<xaml::ScrollViewer>().ok())
                {
                    fold_bar(&scroll);
                }
            })
        })
    {
        RESIZED.with(|r| *r.borrow_mut() = Some((id, revoker)));
    }
    fold_bar(&scroll);
}

/// 帯のボタンを、入りきるまで後回しのものから隠す（入りきるなら出す）。知らせ（最後の子）は切り詰まる前提で、
/// 少しだけ数える（macOS 版と同じ）。
fn fold_bar(scroll: &amonhen_win_xaml::ScrollViewer) {
    use amonhen_win_xaml as xaml;
    use windows_core_reactor::Interface;
    let width = scroll
        .cast::<xaml::FrameworkElement>()
        .and_then(|f| f.ActualWidth())
        .unwrap_or(0.0) as f32;
    let Some(stack) = scroll
        .cast::<xaml::ContentControl>()
        .and_then(|c| c.Content())
        .ok()
        .and_then(|c| c.cast::<xaml::UIElement>().ok())
    else {
        return;
    };
    if width <= 0.0 {
        return;
    }
    let Ok(node) = stack.cast::<xaml::DependencyObject>() else {
        return;
    };
    let count = xaml::VisualTreeHelper::GetChildrenCount(&node).unwrap_or(0);
    let child = |i: usize| -> Option<xaml::UIElement> {
        xaml::VisualTreeHelper::GetChild(&node, i as i32)
            .ok()?
            .cast()
            .ok()
    };
    let unbounded = xaml::Size {
        Width: f32::INFINITY,
        Height: f32::INFINITY,
    };
    let desired = |element: &xaml::UIElement| -> f32 {
        let _ = element.Measure(unbounded);
        element.DesiredSize().map_or(0.0, |s| s.Width)
    };
    let needed = || -> f32 {
        let all = desired(&stack);
        let note = (count > 0)
            .then(|| child(count as usize - 1))
            .flatten()
            .map_or(0.0, |n| desired(&n));
        all - note + note.min(40.0)
    };
    let fold = FOLD.with(|f| f.borrow().clone());
    let fold: Vec<xaml::UIElement> = fold
        .iter()
        .filter(|&&i| i < count as usize)
        .filter_map(|&i| child(i))
        .collect();
    for element in &fold {
        let _ = element.SetVisibility(xaml::Visibility::Visible);
    }
    for element in &fold {
        if needed() <= width {
            break;
        }
        let _ = element.SetVisibility(xaml::Visibility::Collapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pick_resolves_the_matching_conflict() {
        let mut s = State::new("f".into());
        let result = "a\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\nb\n<<<<<<< ours\np\n=======\nq\n>>>>>>> theirs\n";
        s.load(Ok(json!({
            "result": result,
            "current": { "text": "a\nx\nb\np\n", "regions": [[1, 2], [3, 4]] },
            "incoming": { "text": "a\ny\nb\nq\n", "regions": [[1, 2], [3, 4]] },
        })));
        assert_eq!(s.regions[0], [(1, 2), (3, 4)]);
        // 2 つ目を Incoming で解くと、残る 1 つ目は開いたときの 0 番目。
        assert!(s.pick(1, Choice::Theirs));
        assert_eq!(s.original_of(), [Some(0)]);
        assert!(s.result.contains("\nq\n"));
        // 解いたものはもう選べない。
        assert!(!s.pick(1, Choice::Ours));
    }

    #[test]
    fn selected_fraction_points_at_the_conflict_start() {
        let mut s = State::new("f".into());
        let result = "a\nb\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\nc\n<<<<<<< ours\np\n=======\nq\n>>>>>>> theirs\n";
        s.load(Ok(json!({ "result": result })));
        // 13 行のうち、1 つ目は 2 行目、2 つ目は 8 行目から。
        assert_eq!(s.selected_fraction(), Some(2.0 / 13.0));
        s.step(true);
        assert_eq!(s.selected_fraction(), Some(8.0 / 13.0));
    }

    #[test]
    fn undo_steps_through_accepts_until_typed() {
        let mut s = State::new("f".into());
        let result = "<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n<<<<<<< ours\np\n=======\nq\n>>>>>>> theirs\n";
        s.load(Ok(json!({ "result": result })));
        s.accept(Choice::Ours);
        s.accept(Choice::Theirs);
        assert!(s.can_undo() && !s.can_redo());
        s.undo(false);
        assert_eq!(s.conflicts(), 1);
        s.undo(false);
        assert_eq!(s.result, result);
        s.undo(true);
        assert_eq!(s.conflicts(), 1);
        // 手で打ったら、TextBox の Undo に任せる。
        s.edit("typed");
        assert!(!s.can_undo() && !s.can_redo());
    }
}
