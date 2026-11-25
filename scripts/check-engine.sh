#!/bin/sh
# 作ったエンジンを、配る前に動かして確かめる（そのエンジンが動く OS と CPU のマシンで）。
#   scripts/check-engine.sh <エンジン> [<版>]
# 版が合うか（<版> を渡したとき）、Linux 用なら静的リンクか、このリポジトリを読めるか（log・status）。
# 同じ OS と CPU の VSCode 拡張の絵の手伝い（scripts/build-engine.sh が作る dist/render/amonhen-render-<os>-<arch>）も、
# 版と、図と数式を描けるかを確かめる。
set -eu
engine=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
expected=${2:-}
cd "$(dirname "$0")/.."
out=$("$engine" --version)
echo "$out"
if [ -n "$expected" ]; then
    case "$out" in
    "amonhen $expected (build "*) ;;
    *)
        echo "版が違う: ${out}（期待した版: ${expected}）" >&2
        exit 1
        ;;
    esac
fi
# どの Linux でも動くよう、動的なライブラリに頼っていないこと。
static() {
    info=$(file "$1")
    echo "$info"
    case "$info" in
    *"statically linked"* | *"static-pie linked"*) ;;
    *)
        echo "静的リンクになっていない" >&2
        exit 1
        ;;
    esac
}
case "$(basename "$engine")" in
amonhen-linux-*) static "$engine" ;;
esac
"$engine" log -n 3 --no-graph
"$engine" status >/dev/null
echo "ok: $(basename "$engine")"

name=$(basename "$engine")
render=$(cd "$(dirname "$engine")/.." && pwd)/render/amonhen-render-${name#amonhen-}
if [ ! -f "$render" ]; then
    echo "絵の手伝いがない: $render" >&2
    exit 1
fi
out=$("$render" --version)
echo "$out"
if [ -n "$expected" ] && [ "$out" != "amonhen-render $expected" ]; then
    echo "版が違う: ${out}（期待した版: ${expected}）" >&2
    exit 1
fi
case "$name" in
amonhen-linux-*) static "$render" ;;
esac
out=$(printf '%s\n' \
    '{"id":1,"op":"diagram","language":"mermaid","text":"flowchart LR\n  A --> B","colors":{"text":"#000000","background":"#ffffff"}}' \
    '{"id":2,"op":"math","color":"#000000","formulas":[{"tex":"x^2","display":true}]}' | "$render")
case "$out" in
*'"id":1'*'"svg":"<svg'* ) ;;
*)
    echo "図を描けない: $out" >&2
    exit 1
    ;;
esac
case "$out" in
*'"id":2'* ) ;;
*)
    echo "数式を描けない: $out" >&2
    exit 1
    ;;
esac
case "$out" in
*'"formulas":[{'*'"svg":"<svg'*) ;;
*)
    echo "数式を描けない: $out" >&2
    exit 1
    ;;
esac
echo "ok: $(basename "$render")"
