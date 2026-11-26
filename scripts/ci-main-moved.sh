#!/bin/sh
# main への push の CI で、main がもうこのコミットより先に進んでいたら、GITHUB_ENV に `MAIN_MOVED_ON=true` を
# 書く（ci.yml の macOS のジョブは、そのとき残りのステップを飛ばす）。macOS のランナーは同時に 5 つまでで、マージが
# 続くと、先に進まれたコミットの試験が PR の CI を待たせる。同じ中身は PR の CI で試してあり、先のコミットの CI が
# 試す。取り消すとそのコミットの CI が通らなかったことになる（scripts/require-ci.sh）ので、飛ばして success にする。
#   GH_TOKEN・GITHUB_REPOSITORY・GITHUB_SHA を渡して、scripts/ci-main-moved.sh
set -eu

# GitHub の API はときどき 502 などで失敗するので、間を空けて 3 回まで試す。取れなければ試験を流す側に倒す。
head=
for wait in 0 10 30; do
  sleep "$wait"
  if head=$(gh api "repos/$GITHUB_REPOSITORY/commits/main" --jq .sha); then
    break
  fi
  head=
  echo "Couldn't read main's tip; trying again." >&2
done
if [ -n "$head" ] && [ "$head" != "$GITHUB_SHA" ]; then
  echo "main has moved on to $head; leaving the tests to its CI"
  echo "MAIN_MOVED_ON=true" >> "${GITHUB_ENV:-/dev/stdout}"
fi
