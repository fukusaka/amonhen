#!/bin/sh
# 配るものに入る依存のライセンスの一覧を作る（about.toml・about.hbs。cargo install --locked cargo-about --features cli）。
#   scripts/third-party-notices.sh
# 出力: dist/THIRD-PARTY-NOTICES.md。scripts/package-app.sh がアプリとエンジンの tar.gz・zip に、
# apps/vscode/scripts/bundle-engine.sh が .vsix に入れる。
set -eu
cd "$(dirname "$0")/.."
mkdir -p dist
cargo about generate --fail about.hbs -o dist/THIRD-PARTY-NOTICES.md
echo dist/THIRD-PARTY-NOTICES.md
