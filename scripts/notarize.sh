#!/bin/sh
# Apple の公証（notarization）に出す。Developer ID で署名したものだけが通る。
#   scripts/notarize.sh <app> <app の zip> [<エンジン>...]
#   scripts/notarize.sh --engines <エンジン>...   （アプリなしで、エンジンだけ）
# アプリは公証の結果を綴じ込み（ネットにつながらない Mac でも確かめられる）、zip を作り直す。単体の実行ファイル（エンジン）
# には綴じ込めないので、公証に出すだけ（初めて動かすときに Mac が Apple に確かめる）。
# 鍵は App Store Connect の API キー: AMONHEN_NOTARY_KEY（.p8 の中身を base64）・AMONHEN_NOTARY_KEY_ID・
# AMONHEN_NOTARY_ISSUER。どれかがなければ何もしない。
set -eu
if [ "$1" = "--engines" ]; then
    app= zip=
    shift
else
    app=$1 zip=$2
    shift 2
fi
if [ -z "${AMONHEN_NOTARY_KEY:-}" ] || [ -z "${AMONHEN_NOTARY_KEY_ID:-}" ] || [ -z "${AMONHEN_NOTARY_ISSUER:-}" ]; then
    echo "公証の鍵がないので、公証しない"
    exit 0
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo "$AMONHEN_NOTARY_KEY" | base64 --decode > "$work/key.p8"
submit() {
    xcrun notarytool submit "$1" --key "$work/key.p8" --key-id "$AMONHEN_NOTARY_KEY_ID" \
        --issuer "$AMONHEN_NOTARY_ISSUER" --wait --timeout 30m
}
if [ -n "$app" ]; then
    submit "$zip"
    xcrun stapler staple "$app"
    xcrun stapler validate "$app"
    rm -f "$zip"
    ditto -c -k --keepParent "$app" "$zip"
fi
for binary in "$@"; do
    ditto -c -k "$binary" "$work/engine.zip"
    submit "$work/engine.zip"
    rm -f "$work/engine.zip"
done
