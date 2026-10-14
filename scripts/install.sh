#!/bin/sh
# GitHub Releases から Amon Hen を取ってきて /Applications に入れる（入っていれば置き換える）。
#   scripts/install.sh [stable|snapshot]
# gh で取ってくるファイルには quarantine 属性が付かないので、公証なしで起動できる。
set -eu
mode=${1:-stable}
repo=fukusaka/amonhen-standalone
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

case "$mode" in
stable) gh release download --repo "$repo" --pattern '*.zip' --dir "$work" ;;
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
done
