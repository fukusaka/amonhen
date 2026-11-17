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

mod bindings;
mod textbox;
pub use bindings::*;
pub use textbox::select_all;
