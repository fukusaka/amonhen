#!/bin/sh
# GitHub Releases から .vsix を取ってきて VSCode に入れる。
#   scripts/install.sh            正式版（最新の Release）
#   scripts/install.sh snapshot   main の最新
# Remote-SSH の接続先では、VSCode の統合ターミナルで実行すると接続先に入る。
set -eu

repo=fukusaka/amonhen
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

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

case $channel in
  stable)
    # 拡張の正式版はタグ vscode-v*（アプリ・エンジンの v* とは別）
    tag=$(gh release list --repo "$repo" --exclude-pre-releases --json tagName --jq '[.[] | select(.tagName | startswith("vscode-v"))][0].tagName')
    gh release download "$tag" --repo "$repo" --pattern '*.vsix' --dir "$dir" ;;
  snapshot) gh release download snapshot --repo "$repo" --pattern '*.vsix' --dir "$dir" ;;
  *)
    echo "usage: $0 [stable|snapshot]" >&2
    exit 2
    ;;
esac

"$code" --install-extension "$dir"/*.vsix --force
echo "インストールしました: $(basename "$dir"/*.vsix)。ウィンドウを再読み込みすると反映されます"
