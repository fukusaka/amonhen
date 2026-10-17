#!/bin/sh
# Swift から使う Rust のコア（静的ライブラリと C のヘッダー）と、daemon 越しに開くときの amonhend、端末から使う
# amonhen（CLI）をビルドする。
set -eu
cd "$(dirname "$0")/.."
# アプリの対応する最も古い macOS（Package.swift の platforms）に合わせる。構文の色付けの文法は C で書かれていて、
# 指定しないと手元の macOS 向けにコンパイルされる。
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.4}"
# コア（Swift から使う）は構文の色付けを入れ、エンジン（amonhen・amonhend）は入れない。一緒にビルドすると機能が
# 合わさって（cargo の feature の統一）色付け入りのエンジンになり、配るエンジン（scripts/build-engine.sh）を作るときに
# 作り直しになるので、分けてビルドする。
cargo build --release -p amonhen-ffi-c
cargo build --release -p amonhen-daemon -p amonhen-cli
