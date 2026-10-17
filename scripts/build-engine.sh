#!/bin/sh
# エンジン（amonhen。CLI と daemon を兼ねる 1 つの実行ファイル）を、配る 3 つの OS と CPU 向けに作る。
#   scripts/build-engine.sh [<版>]
# 出力: dist/engine/amonhen-<os>-<arch>（darwin-aarch64・linux-x86_64・linux-aarch64）。<版> を渡すと、リリースに
# 載せる dist/amonhen-<版>-<os>-<arch>.tar.gz（中は amonhen の 1 つ）も作る。
# エンジンには配る版を埋め込む（`amonhen --version`。`amonhen install` が新旧を決める）。<版> がなければ snapshot の版。
# Linux 用は musl で静的リンクし（どの Linux でも動く）、リンクには zig を使う（brew install zig）。
set -eu
cd "$(dirname "$0")/.."
version=${1:-}
AMONHEN_VERSION=${version:-$(scripts/version.sh snapshot)}
export AMONHEN_VERSION
echo "engine version: $AMONHEN_VERSION"
rm -rf dist/engine
mkdir -p dist/engine
cargo build --release -p amonhen-cli
cp target/release/amonhen dist/engine/amonhen-darwin-aarch64
for arch in x86_64 aarch64; do
    triple="$arch-unknown-linux-musl"
    upper=$(echo "$triple" | tr 'a-z-' 'A-Z_')
    # 起動の部分（crt）と libc は zig のものを使い、Rust の同梱のものとの二重定義を避ける。
    # SSH 先に送るので、デバッグ情報は落として小さくする。
    env "CARGO_TARGET_${upper}_LINKER=$PWD/scripts/zig-cc.sh" ZIG_TARGET="$arch-linux-musl" \
        "CARGO_TARGET_${upper}_RUSTFLAGS=-C link-self-contained=no -C strip=symbols" \
        cargo build --release -p amonhen-cli --target "$triple"
    cp "target/$triple/release/amonhen" "dist/engine/amonhen-linux-$arch"
done
if [ -n "$version" ]; then
    for f in dist/engine/amonhen-*; do
        platform=${f#dist/engine/amonhen-}
        work=$(mktemp -d)
        cp "$f" "$work/amonhen"
        tar -C "$work" -czf "dist/amonhen-$version-$platform.tar.gz" amonhen
        rm -rf "$work"
    done
    ls -l dist/amonhen-"$version"-*.tar.gz
fi
ls -l dist/engine
