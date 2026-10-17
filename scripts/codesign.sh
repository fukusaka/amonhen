#!/bin/sh
# macOS の実行ファイル・アプリに署名する。AMONHEN_SIGN_IDENTITY が Developer ID なら公証の条件（hardened runtime・
# 時刻の印）を付ける。なければ ad-hoc（`-`）。
#   scripts/codesign.sh <パス>...
set -eu
identity=${AMONHEN_SIGN_IDENTITY:--}
case $identity in
"Developer ID Application"*) codesign --force --sign "$identity" --options runtime --timestamp "$@" ;;
*) codesign --force --sign "$identity" --timestamp=none "$@" ;;
esac
