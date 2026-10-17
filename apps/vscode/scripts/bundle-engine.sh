#!/bin/sh
# Amon Hen のエンジン（amonhen。CLI と daemon を兼ねる）を bin/ に写す。.vsix に同梱する。
# エンジンはリポジトリのルートの scripts/build-engine.sh で作る（dist/engine/amonhen-<os>-<arch>）。
# ライセンスの文書（ルートの LICENSE-*・NOTICE と、依存のライセンスの一覧 dist/THIRD-PARTY-NOTICES.md。
# scripts/third-party-notices.sh で作る）も、拡張のフォルダに写して同梱する。
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
if [ ! -f ../../dist/THIRD-PARTY-NOTICES.md ]; then
  echo "../../dist/THIRD-PARTY-NOTICES.md がありません。先にリポジトリのルートで scripts/third-party-notices.sh を実行してください" >&2
  exit 1
fi
cp ../../LICENSE-MIT ../../LICENSE-APACHE ../../NOTICE ../../dist/THIRD-PARTY-NOTICES.md .
ls -l bin
