#!/bin/sh
# GitHub Releases から Amon Hen を取ってきて /Applications に入れる（入っていれば置き換える）。
#   scripts/install.sh [stable|snapshot]
# gh で取ってくるファイルには quarantine 属性が付かないので、公証なしで起動できる。
set -eu
mode=${1:-stable}
repo=fukusaka/amonhen
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

case "$mode" in
stable)
    # アプリとエンジンの正式版はタグ v<数字>（VSCode 拡張の vscode-v* とは別）
    tag=$(gh release list --repo "$repo" --exclude-pre-releases --json tagName --jq '[.[] | select(.tagName | test("^v[0-9]"))][0].tagName')
    gh release download "$tag" --repo "$repo" --pattern '*.zip' --dir "$work" ;;
snapshot) gh release download snapshot --repo "$repo" --pattern '*.zip' --dir "$work" ;;
*)
    echo "usage: $0 [stable|snapshot]" >&2
    exit 2
    ;;
esac

ditto -x -k "$work"/*.zip "$work/app"
for app in "$work"/app/*.app; do
    target="/Applications/$(basename "$app")"
    rm -rf "$target"
    ditto "$app" "$target"
    # 万一付いていても外す。
    xattr -dr com.apple.quarantine "$target" 2>/dev/null || true
    echo "installed $target ($(defaults read "$target/Contents/Info" CFBundleShortVersionString))"
    # エンジン（CLI と daemon）を置き場所（~/.local/share/amonhen）に置き、~/.local/bin/amonhen を張る。今の
    # ものより新しいときだけ入れ替える（アプリも起動するたびに同じことをする）。
    if [ -x "$target/Contents/Helpers/amonhen" ]; then
        "$target/Contents/Helpers/amonhen" install
    fi
done
