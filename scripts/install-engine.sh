#!/bin/sh
# GitHub Releases からエンジン（CLI と daemon）を取ってきて置く（macOS・Linux）。置き場所は ~/.local/share/amonhen、
# ~/.local/bin/amonhen を張る。今のものより新しいときだけ入れ替える。2 つ目からの引数は `amonhen install` に渡す。
#   scripts/install-engine.sh [stable|snapshot] [--no-link|--force]
# clone していなければ:
#   curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.sh | sh -s -- [stable|snapshot]
# Windows は scripts/install-engine.ps1。
set -eu
mode=${1:-stable}
[ $# -gt 0 ] && shift
api=https://api.github.com/repos/fukusaka/amonhen/releases

case "$(uname -s)" in
Darwin) os=darwin ;;
Linux) os=linux ;;
*)
    echo "unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac
case "$(uname -m)" in
arm64 | aarch64) cpu=aarch64 ;;
x86_64 | amd64) cpu=x86_64 ;;
*)
    echo "unsupported CPU: $(uname -m)" >&2
    exit 1
    ;;
esac

case "$mode" in
# アプリとエンジンの正式版はタグ v<数字>（VSCode 拡張の vscode-v* とは別）。一覧は新しい順。
stable) list="$api?per_page=100" pattern="/download/v[0-9][^/]*/amonhen-[^/]*-$os-$cpu\\.tar\\.gz\$" ;;
snapshot) list="$api/tags/snapshot" pattern="/download/snapshot/amonhen-[^/]*-$os-$cpu\\.tar\\.gz\$" ;;
*)
    echo "usage: $0 [stable|snapshot] [--no-link|--force]" >&2
    exit 2
    ;;
esac
url=$(curl -fsSL "$list" | grep -o '"browser_download_url": *"[^"]*"' | sed 's/.*"\(https:[^"]*\)"$/\1/' |
    grep -E "$pattern" | head -1 || true)
if [ -z "$url" ]; then
    echo "no $mode engine for $os-$cpu found" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo "downloading $url"
curl -fsSL "$url" | tar -C "$work" -xzf -
"$work/amonhen" install "$@"
