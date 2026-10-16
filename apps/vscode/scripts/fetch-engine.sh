#!/bin/sh
# Amon Hen のエンジン（スタンドアロン版の amonhen。CLI と daemon を兼ねる）を取ってきて bin/ に置く。.vsix に同梱する。
#   scripts/fetch-engine.sh                 package.json の amonhenEngine のリリース（"snapshot" なら最新の snapshot）
#   scripts/fetch-engine.sh <tag>           そのリリース
#   scripts/fetch-engine.sh --local <dir>   手元でビルドしたもの（amonhen-standalone の dist/engine）
# 出力: bin/amonhen-<os>-<arch>（darwin-aarch64・linux-x86_64・linux-aarch64）。リリースは非公開のリポジトリなので、
# CI では読める鍵を GH_TOKEN に渡す。
set -eu
cd "$(dirname "$0")/.."
repo=fukusaka/amonhen-standalone
rm -rf bin
mkdir -p bin
if [ "${1:-}" = --local ]; then
  cp "$2"/amonhen-* bin/
else
  tag=${1:-$(node -p "require('./package.json').amonhenEngine")}
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  gh release download "$tag" --repo "$repo" --pattern 'amonhen-*.tar.gz' --dir "$work"
  for f in "$work"/amonhen-*.tar.gz; do
    platform=$(basename "$f" .tar.gz | sed -E 's/.*-((darwin|linux)-(aarch64|x86_64))$/\1/')
    mkdir -p "$work/x"
    tar -C "$work/x" -xzf "$f" amonhen
    mv "$work/x/amonhen" "bin/amonhen-$platform"
  done
fi
chmod 755 bin/amonhen-*
ls -l bin
