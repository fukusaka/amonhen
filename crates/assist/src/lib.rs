//! AI の呼び出し（docs/ux.md の「AI」）。コアの外の小さな crate で、手元の CLI（Claude Code・Codex）ごとに
//! [`Assistant`] の実装を持つ。プロンプトと標準入力で送り、流れてくる JSON を少しずつ読む。
//!
//! 用途ごとの組み込みのプロンプト・使える変数・返事の形（[`Purpose`]）、利用者とリポジトリのプロンプト
//! （[`Prompts`]・[`repository_prompt`]）、返事のキャッシュ（[`Cache`]・[`cache_key`]）もここに置く。
//! 置き場所（設定のディレクトリなど）と、送る前の確認は FE が持つ。macOS 版は同じことを Swift で持つ
//! （`apps/macos/Sources/AmonHen/Assistant.swift`）。端末内のモデル（Apple Intelligence）は macOS 版だけ。

mod cli;
mod difftext;
mod purpose;
mod store;

pub use cli::{Assistant, ClaudeCode, Cli, Codex, Parser, codex_models, stream, version};
pub use difftext::{DiffText, Line};
pub use purpose::{Proposal, Purpose, Variables, build, fill};
pub use store::{Cache, Prompts, cache_key, repository_prompt, sha256_hex};
