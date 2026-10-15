#!/bin/sh
# Swift から使う Rust のコア（静的ライブラリと C のヘッダー）と、daemon 越しに開くときの amonhend をビルドする。
set -eu
cd "$(dirname "$0")/.."
# アプリの対応する最も古い macOS（Package.swift の platforms）に合わせる。構文の色付けの文法は C で書かれていて、
# 指定しないと手元の macOS 向けにコンパイルされる。
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.4}"
cargo build --release -p amonhen-ffi-c -p amonhen-daemon
