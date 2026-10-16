#!/bin/sh
# Amon Hen のエンジン（amonhen。CLI と daemon を兼ねる）を bin/ に写す。.vsix に同梱する。
# エンジンはリポジトリのルートの scripts/build-engine.sh で作る（dist/engine/amonhen-<os>-<arch>）。
set -eu
cd "$(dirname "$0")/.."
engines=../../dist/engine
if ! ls "$engines"/amonhen-* >/dev/null 2>&1; then
  echo "$engines にエンジンがありません。先にリポジトリのルートで scripts/build-engine.sh を実行してください" >&2
  exit 1
fi
rm -rf bin
mkdir -p bin
cp "$engines"/amonhen-* bin/
chmod 755 bin/amonhen-*
ls -l bin
