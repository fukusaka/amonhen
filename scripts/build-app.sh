#!/bin/sh
# 手元で Amon Hen.app を作って zip にする（コンパイル → Linux 用のエンジン → 組み立てと署名）。
#   scripts/build-app.sh [stable|snapshot]
# 出力: dist/<アプリ名>-<版>.zip（最後の行にその場所）と、エンジンの tar.gz。
# CI は同じことを仕事に分けて行う: scripts/compile-app.sh（macOS）と、OS と CPU ごとのエンジン
# （scripts/build-engine.sh、そのマシンで）を並べて走らせ、最後に scripts/package-app.sh。
# 署名: AMONHEN_SIGN_IDENTITY にキーチェーンの証明書の名前があればそれで、なければ ad-hoc。ad-hoc 署名はビルドの
# たびに身元が変わり、macOS のアクセス許可が更新のたびにリセットされる。
set -eu
cd "$(dirname "$0")/.."
mode=${1:-stable}
version=$(scripts/version.sh "$mode")
scripts/compile-app.sh "$mode" >&2
scripts/third-party-notices.sh >&2
# SSH 先に送る Linux 用のエンジン（zig で作る）。
AMONHEN_ENGINE_PLATFORMS="linux-x86_64 linux-aarch64" scripts/build-engine.sh "$version" >&2
scripts/package-app.sh "$mode"
