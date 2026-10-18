#!/bin/sh
# あるコミットで、main への push の CI（ci.yml）が通っているかを確かめる（正式版を出す前に）。走っている途中なら
# 終わるまで待つ（最大 60 分）。テストをもう一度流す代わりに、CI と同じ基準（3 つの OS の Rust・Swift・拡張）で判断する。
#   scripts/require-ci.sh <コミット>
set -eu
sha=$1
for _ in $(seq 120); do
    # 走っている途中は conclusion が空なので、欄が詰まらないよう "-" にする。CI がなければ何も出さない。
    run=$(gh run list --workflow CI --commit "$sha" --event push --limit 1 --json status,conclusion,url \
        --jq '.[0] // empty | "\(.status) \(if .conclusion == "" or .conclusion == null then "-" else .conclusion end) \(.url)"')
    set -- $run
    case "${1:-}" in
    completed)
        if [ "$2" = success ]; then
            echo "CI passed: $3"
            exit 0
        fi
        echo "CI did not pass for $sha ($2): $3" >&2
        exit 1
        ;;
    "")
        echo "no CI run for $sha (push it to main first)" >&2
        exit 1
        ;;
    esac
    echo "CI is $1, waiting: $3"
    sleep 30
done
echo "timed out waiting for CI on $sha" >&2
exit 1
