#!/bin/sh
# SSH 先に送り込む Linux 用の amonhend を作る（x86_64 と aarch64、musl で静的リンク）。
# リンクには zig を使う（brew install zig）。出力: dist/linux/amonhend-linux-<arch>
set -eu
cd "$(dirname "$0")/.."
mkdir -p dist/linux
for arch in x86_64 aarch64; do
    triple="$arch-unknown-linux-musl"
    upper=$(echo "$triple" | tr 'a-z-' 'A-Z_')
    # 起動の部分（crt）と libc は zig のものを使い、Rust の同梱のものとの二重定義を避ける。
    # SSH 先に送るので、デバッグ情報は落として小さくする。
    env "CARGO_TARGET_${upper}_LINKER=$PWD/scripts/zig-cc.sh" ZIG_TARGET="$arch-linux-musl" \
        "CARGO_TARGET_${upper}_RUSTFLAGS=-C link-self-contained=no -C strip=symbols" \
        cargo build --release -p amonhen-daemon --target "$triple"
    cp "target/$triple/release/amonhend" "dist/linux/amonhend-linux-$arch"
done
ls -l dist/linux
