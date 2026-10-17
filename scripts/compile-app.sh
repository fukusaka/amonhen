#!/bin/sh
# アプリの組み立てに要るものをコンパイルする（Rust のコアと macOS のエンジン、Swift のアプリ）。組み立てと署名は
# scripts/package-app.sh。CI では、ほかの OS と CPU のエンジンを作るのと並んで走る。
#   scripts/compile-app.sh [stable|snapshot]
# 出力: dist/bin/AmonHen（アプリの実行ファイル）、dist/engine/amonhen-darwin-aarch64
set -eu
cd "$(dirname "$0")/.."
mode=${1:-stable}
# コアとエンジンを同じ環境で作る（版の埋め込みと配る macOS の版がそろっていれば、エンジンのビルドは作り直さない）。
AMONHEN_VERSION=$(scripts/version.sh "$mode")
export AMONHEN_VERSION
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.4}"
scripts/build-core.sh
AMONHEN_ENGINE_PLATFORMS=darwin-aarch64 scripts/build-engine.sh "$AMONHEN_VERSION"
(cd apps/macos && swift build -c release --product AmonHen)
mkdir -p dist/bin
cp apps/macos/.build/release/AmonHen dist/bin/AmonHen
