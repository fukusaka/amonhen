#!/bin/sh
# 作ったエンジンを、配る前に動かして確かめる（そのエンジンが動く OS と CPU のマシンで）。
#   scripts/check-engine.sh <エンジン> [<版>]
# 版が合うか（<版> を渡したとき）、Linux 用なら静的リンクか、このリポジトリを読めるか（log・status）。
set -eu
engine=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
expected=${2:-}
cd "$(dirname "$0")/.."
out=$("$engine" --version)
echo "$out"
if [ -n "$expected" ]; then
    case "$out" in
    "amonhen $expected (protocol "*) ;;
    *)
        echo "版が違う: $out（期待した版: $expected）" >&2
        exit 1
        ;;
    esac
fi
case "$(basename "$engine")" in
amonhen-linux-*)
    # どの Linux でも動くよう、動的なライブラリに頼っていないこと。
    info=$(file "$engine")
    echo "$info"
    case "$info" in
    *"statically linked"* | *"static-pie linked"*) ;;
    *)
        echo "静的リンクになっていない" >&2
        exit 1
        ;;
    esac
    ;;
esac
"$engine" log -n 3 --no-graph
"$engine" status >/dev/null
echo "ok: $(basename "$engine")"
