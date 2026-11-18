#!/bin/sh
# PR の CI で macOS のジョブ（Rust・VSCode 拡張・Swift・CodeQL の Swift）を流すかを決め、GITHUB_OUTPUT に
# `macos=true` か `macos=false` を書く。macOS のランナーは数が少なく待ちが長いので、Windows のアプリ
# （apps/windows）と文書だけを変えた PR では流さない。main への push（snapshot・正式版の前の確認）や、ほかの
# ファイルに触れた PR では流す。
#   GH_TOKEN・GITHUB_REPOSITORY・PR（番号）を渡して、scripts/ci-needs-macos.sh
set -eu

out=${GITHUB_OUTPUT:-/dev/stdout}
if [ -z "${PR:-}" ]; then
  echo "macos=true" >> "$out"
  exit 0
fi
files=$(gh api --paginate "repos/$GITHUB_REPOSITORY/pulls/$PR/files" --jq '.[].filename')
# 一覧が取れなければ、流す側に倒す。
if [ -z "$files" ]; then
  echo "macos=true" >> "$out"
  exit 0
fi
others=$(printf '%s\n' "$files" | grep -Ev '^apps/windows/|^docs/|\.md$' || true)
if [ -n "$others" ]; then
  echo "macos=true" >> "$out"
else
  echo "Only apps/windows and documents changed: skipping the macOS jobs."
  echo "macos=false" >> "$out"
fi
