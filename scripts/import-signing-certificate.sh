#!/bin/sh
# CI で、secret の自己署名証明書を一時的なキーチェーンに入れ、build-app.sh が使う署名の名前を設定する。
# AMONHEN_CERT_P12（.p12 の base64）が空なら何もしない（ad-hoc 署名になる）。
set -eu
if [ -z "${AMONHEN_CERT_P12:-}" ]; then
    echo "署名用の証明書がないので ad-hoc 署名にする"
    exit 0
fi
keychain="$RUNNER_TEMP/amonhen.keychain-db"
echo "$AMONHEN_CERT_P12" | base64 --decode > "$RUNNER_TEMP/cert.p12"
security create-keychain -p "" "$keychain"
security set-keychain-settings "$keychain"
security unlock-keychain -p "" "$keychain"
security import "$RUNNER_TEMP/cert.p12" -k "$keychain" -P "${AMONHEN_CERT_PASSWORD:-}" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "" "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
rm -f "$RUNNER_TEMP/cert.p12"
echo "AMONHEN_SIGN_IDENTITY=${AMONHEN_CERT_NAME:-Amon Hen}" >> "$GITHUB_ENV"
