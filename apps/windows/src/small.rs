//! 小さなボタン（macOS 版の controlSize small、GTK4 版の `small-push`）。帯に並べるボタンは WinUI のふつうの
//! 大きさ（高さ 32）だと太すぎるので、文字を 12 にして、余白を詰める。ボタンの余白（`ButtonPadding`）は既定の
//! スタイルが StaticResource で引くので部品ごとには替えられず、reactor の Button には Padding もない。そこで
//! 中の字に負の外側の余白を付け、ボタンの余白の分を取り返す（高さ 23 ほど）。
//!
//! 切り替えも Button で作り、選んでいるものを Accent にする（ToggleButton の押した見た目と同じ）。中身を入れると
//! View になるので、置き場所・押したときなどは `configure` で先に付ける。

use windows_reactor::*;

/// 小さなボタン。
pub fn button(text: impl Into<String>, configure: impl FnOnce(Button) -> Button) -> View {
    let button = Button::new().vertical_alignment(VerticalAlignment::Center);
    configure(button).content(
        TextBlock::new()
            .text(text.into())
            .font_size(12.0)
            .margin(Thickness::new(-3.0, -3.0, -3.0, -3.0)),
    )
}

/// 小さな切り替え（`on` なら押した見た目）。つないだ組（Unstaged・Staged・All など）にも使う。
pub fn toggle(text: impl Into<String>, on: bool, configure: impl FnOnce(Button) -> Button) -> View {
    let style = if on {
        ButtonStyle::Accent
    } else {
        ButtonStyle::Default
    };
    button(text, |b| configure(b.style(style)))
}

/// 小さなアイコンのボタン。アイコン（`crate::icons::icon`・`FontIcon`・字）は大きさを変えず、字と同じ負の余白で
/// ボタンの余白を取り返す。
pub fn icon_button(content: impl Into<View>, configure: impl FnOnce(Button) -> Button) -> View {
    let button = Button::new().vertical_alignment(VerticalAlignment::Center);
    configure(button).content(
        Border::new()
            .margin(Thickness::new(-3.0, -3.0, -3.0, -3.0))
            .content(content),
    )
}

/// 小さなアイコンの切り替え（`on` なら押した見た目）。
pub fn icon_toggle(
    content: impl Into<View>,
    on: bool,
    configure: impl FnOnce(Button) -> Button,
) -> View {
    let style = if on {
        ButtonStyle::Accent
    } else {
        ButtonStyle::Default
    };
    icon_button(content, |b| configure(b.style(style)))
}

/// DropDownButton の中身（字）を小さなボタンと同じ大きさにする。
pub fn drop_text(text: impl Into<String>) -> TextBlock {
    TextBlock::new()
        .text(text.into())
        .font_size(12.0)
        .margin(Thickness::new(-3.0, -3.0, -3.0, -3.0))
}

/// DropDownButton の中身（アイコン）を小さなボタンと同じ余白にする。
pub fn drop_icon(content: impl Into<View>) -> View {
    Border::new()
        .margin(Thickness::new(-3.0, -3.0, -3.0, -3.0))
        .content(content)
}

/// 一覧の行の高さ（WinUI の一覧の行の最小は 40 で、macOS 版の 1 行のほぼ倍。詰める）。
pub const ROW_HEIGHT: f64 = 26.0;

/// 詰めたチェックボックス（WinUI のチェックボックスは最小の幅 120・高さ 32 で、行が高くなる）。
pub fn check(check: CheckBox) -> CheckBox {
    check.min_width(0.0).min_height(0.0)
}

/// 小さな切り替えを並べた組（RadioButtons の代わり。RadioButtons は丸と字で行が高い）。押したものの番号を
/// `on` に送る。今のものを押したときは `None`（何もしない）。
pub fn picker(
    titles: &[String],
    selected: usize,
    on: &Callback<Option<usize>>,
    configure: impl FnOnce(StackPanel) -> StackPanel,
) -> View {
    let buttons = titles.iter().enumerate().map(|(i, title)| {
        let on = on.clone();
        let pick = Callback::new(move |()| {
            let _ = on.call((i != selected).then_some(i));
        });
        (
            i.to_string(),
            toggle(title.clone(), i == selected, |b| b.on_click(pick)),
        )
    });
    configure(
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(2.0)
            .vertical_alignment(VerticalAlignment::Center),
    )
    .children((View::keyed_fragment(buttons),))
}
