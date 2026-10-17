#!/bin/sh
# GTK4 のアプリ（apps/gtk4）を配る形にまとめる。Linux のそのマシンの CPU 向けに作る。
#   scripts/package-gtk4.sh [<版>]
# 出力: dist/amonhen-gtk-<版>-linux-<arch>.tar.gz（中身は amonhen-gtk-linux-<arch>/ に、実行ファイル、engines/、
# .desktop、アイコン、install.sh）。<版> がなければ snapshot の版（scripts/version.sh）で、About に出す。
# engines/ には dist/engine の Linux のエンジン（scripts/build-engine.sh が作る）を入れ、SSH 先に送るのに使う
# （なければ入れない。そのときは SSH 先に amonhen が要る）。
# GTK は同梱せず、そのマシンの GTK 4.14 以降を使う（Ubuntu 24.04 以降・Debian 13 以降など）。
set -eu
cd "$(dirname "$0")/.."
version=${1:-}
AMONHEN_VERSION=${version:-$(scripts/version.sh snapshot)}
export AMONHEN_VERSION
arch=$(uname -m | sed 's/arm64/aarch64/')
name="amonhen-gtk-linux-$arch"
cargo build --release --manifest-path apps/gtk4/Cargo.toml
out="dist/$name"
rm -rf "$out"
mkdir -p "$out"
cp apps/gtk4/target/release/amonhen-gtk "$out/"
cp apps/gtk4/data/io.github.fukusaka.amonhen.desktop apps/gtk4/data/io.github.fukusaka.amonhen.png "$out/"
cp apps/gtk4/data/install.sh "$out/"
chmod +x "$out/install.sh"
cp LICENSE-MIT LICENSE-APACHE NOTICE "$out/"
for engine in dist/engine/amonhen-linux-*; do
    [ -f "$engine" ] || continue
    mkdir -p "$out/engines"
    cp "$engine" "$out/engines/"
    chmod 755 "$out/engines/$(basename "$engine")"
done
tar -C dist -czf "dist/amonhen-gtk-$AMONHEN_VERSION-linux-$arch.tar.gz" "$name"
echo "dist/amonhen-gtk-$AMONHEN_VERSION-linux-$arch.tar.gz"
