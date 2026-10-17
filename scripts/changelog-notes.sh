#!/bin/sh
# CHANGELOG.md から、ある版の節を取り出す（Release の説明に使う）。
#   scripts/changelog-notes.sh "<節の見出し>" <版>     例: "App and engine" 0.1.1、"VSCode extension" 0.0.3
# 節（## <見出し>）の中の ### [<版>] の見出しの下から、次の見出しまで。見つからなければ失敗する（CHANGELOG を直し忘れたまま出さない）。
set -eu
cd "$(dirname "$0")/.."
notes=$(awk -v section="## $1" -v heading="### [$2]" '
    /^## / { in_section = ($0 == section); in_version = 0; next }
    in_section && /^### / { in_version = (index($0, heading) == 1); next }
    in_section && in_version { print }
' CHANGELOG.md)
if [ -z "$(echo "$notes" | tr -d '[:space:]')" ]; then
    echo "CHANGELOG.md の「$1」に [$2] の節がない" >&2
    exit 1
fi
printf '%s\n' "$notes" | sed -e '/./,$!d'
