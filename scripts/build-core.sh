#!/bin/sh
# Swift から使う Rust のコア（静的ライブラリと C のヘッダー）をビルドする。
set -eu
cd "$(dirname "$0")/.."
cargo build --release -p amonhen-ffi-c
