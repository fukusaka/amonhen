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
