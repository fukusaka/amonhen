#!/bin/sh
# GitHub Releases から .vsix を取ってきて VSCode に入れる。
#   scripts/install-vscode.sh            正式版（最新の Release）
#   scripts/install-vscode.sh snapshot   main の最新
# clone していなければ:
#   curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-vscode.sh | sh -s -- [stable|snapshot]
# Remote-SSH の接続先では、VSCode の統合ターミナルで実行すると接続先に入る。
set -eu

api=https://api.github.com/repos/fukusaka/amonhen/releases
channel=${1:-stable}

code=${CODE:-}
if [ -z "$code" ]; then
  if command -v code >/dev/null 2>&1; then
    code=code
  elif [ -x "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" ]; then
    code="/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
  else
    echo "code コマンドが見つかりません。CODE=/path/to/code で指定してください" >&2
    exit 1
  fi
fi

case $channel in
  # 拡張の正式版はタグ vscode-v*（アプリ・エンジンの v* とは別）。一覧は新しい順。
  stable) list="$api?per_page=100" pattern='/download/vscode-v[^/]*/[^/]*\.vsix$' ;;
  snapshot) list="$api/tags/snapshot" pattern='/download/snapshot/[^/]*\.vsix$' ;;
  *)
    echo "usage: $0 [stable|snapshot]" >&2
    exit 2
    ;;
esac
url=$(curl -fsSL "$list" | grep -o '"browser_download_url": *"[^"]*"' | sed 's/.*"\(https:[^"]*\)"$/\1/' |
  grep -E "$pattern" | head -1 || true)
if [ -z "$url" ]; then
  echo "$channel の .vsix が見つかりません" >&2
  exit 1
fi

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
file=$(basename "$url" | sed 's/%2B/+/g')
curl -fsSL -o "$dir/$file" "$url"
"$code" --install-extension "$dir/$file" --force
echo "インストールしました: $file。ウィンドウを再読み込みすると反映されます"
