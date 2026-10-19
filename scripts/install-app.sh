#!/bin/sh
# GitHub Releases から Amon Hen を取ってきて /Applications に入れる（入っていれば置き換える）。
#   scripts/install-app.sh [--user|--system] [stable|snapshot]
# clone していなければ:
#   curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-app.sh | sh -s -- [--user|--system] [stable|snapshot]
# --user はユーザーのアプリフォルダ（~/Applications）に、--system は /Applications に入れる。どちらも付けなければ、
# 今入っているところ（~/Applications にだけあればそこ、なければ /Applications）を新しくする。
# curl で取ってくるファイルには quarantine 属性が付かない（署名・公証してあるので、付いていても開ける）。
set -eu
usage() {
    echo "usage: $0 [--user|--system] [stable|snapshot]" >&2
    exit 2
}
dir=
mode=
for arg in "$@"; do
    case "$arg" in
    --user) dir="$HOME/Applications" ;;
    --system) dir=/Applications ;;
    stable | snapshot) mode=$arg ;;
    *) usage ;;
    esac
done
mode=${mode:-stable}
api=https://api.github.com/repos/fukusaka/amonhen/releases

case "$mode" in
# アプリとエンジンの正式版はタグ v<数字>（VSCode 拡張の vscode-v* とは別）。一覧は新しい順。
stable) list="$api?per_page=100" pattern='/download/v[0-9][^/]*/Amon-Hen-[^/]*\.zip$' ;;
snapshot) list="$api/tags/snapshot" pattern='/download/snapshot/Amon-Hen-Snapshot-[^/]*\.zip$' ;;
esac
url=$(curl -fsSL "$list" | grep -o '"browser_download_url": *"[^"]*"' | sed 's/.*"\(https:[^"]*\)"$/\1/' |
    grep -E "$pattern" | head -1 || true)
if [ -z "$url" ]; then
    echo "no $mode release of the app found" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo "downloading $url"
curl -fsSL -o "$work/app.zip" "$url"
ditto -x -k "$work/app.zip" "$work/app"
for app in "$work"/app/*.app; do
    name=$(basename "$app")
    to=$dir
    if [ -z "$to" ]; then
        to=/Applications
        [ ! -e "/Applications/$name" ] && [ -e "$HOME/Applications/$name" ] && to="$HOME/Applications"
    fi
    mkdir -p "$to"
    target="$to/$name"
    # 起動中でも置き換えられるが、動いているほうは古い版のまま。終わりに知らせる。
    running=
    pgrep -f "^$target/Contents/MacOS/" >/dev/null 2>&1 && running=1
    rm -rf "$target"
    ditto "$app" "$target"
    # 万一付いていても外す。
    xattr -dr com.apple.quarantine "$target" 2>/dev/null || true
    echo "installed $target ($(defaults read "$target/Contents/Info" CFBundleShortVersionString))"
    # 置き場所を変えたときは、古いほうを残す（消すかどうかは使う人が決める）。
    for other in "/Applications/$name" "$HOME/Applications/$name"; do
        if [ "$other" != "$target" ] && [ -e "$other" ]; then
            echo "another copy is still in $other; remove it if you no longer need it"
        fi
    done
    # エンジン（CLI と daemon）を置き場所（~/.local/share/amonhen）に置き、~/.local/bin/amonhen を張る。今の
    # ものより新しいときだけ入れ替える（アプリも起動するたびに同じことをする）。
    if [ -x "$target/Contents/Helpers/amonhen" ]; then
        "$target/Contents/Helpers/amonhen" install
    fi
    if [ -n "$running" ]; then
        echo "$(basename "$target" .app) is running the old version; quit and reopen it to use the new one"
    fi
done
