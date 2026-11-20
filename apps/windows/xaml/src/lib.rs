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

mod badge;
mod bindings;
mod list;
mod textbox;
pub use badge::put_badges;
pub use bindings::*;
pub use list::{drag_some_tags, drag_tags, drop_on_rows, selected_tags};
pub use textbox::{is_text_box, scroll_to_fraction, select_all};
