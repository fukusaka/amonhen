//! windows-reactor が出していない WinUI 3 の型（キー、ホイール、フォーカスなど）。部品の生のオブジェクトは
//! `ElementRef::request_native_element` で受け取り、ここの型に `cast` して使う。
//!
//! `bindings.rs` は `generate/` が作る（手で直さない）。

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    clippy::all
)]

mod acrylic;
mod badge;
mod bindings;
mod fills;
mod list;
mod tabview;
mod textbox;
pub use acrylic::Acrylic;
pub use badge::{pass_through, window_handle};
pub use bindings::*;
pub use fills::ControlFills;
pub use list::{
    clear_drop_target, drag_some_tags, drag_tags, drop_on_rows, dropped_nowhere, select_tags,
    selected_tags, set_drop_target, tag_text, tag_up_with, text_of, text_value,
};
pub use tabview::on_tab_moves;
pub use textbox::{is_text_box, scroll_to_fraction, select_all, set_caret};
/// 文字の段落・飾り（`BlockCollection`・`InlineCollection`）を番号で読むときの型。
pub use windows_collections::IVector;
