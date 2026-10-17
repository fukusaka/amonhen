#!/bin/sh
# Amon Hen.app を組み立てて zip にする。出力: dist/<アプリ名>-<バージョン>.zip
#   scripts/build-app.sh [stable|snapshot]
# 署名: AMONHEN_SIGN_IDENTITY にキーチェーンの証明書の名前があればそれで、なければ ad-hoc。Developer ID の証明書なら
# hardened runtime と時刻の印を付けて署名し、公証の鍵（scripts/notarize.sh）があれば公証して結果を綴じ込む。
# ad-hoc 署名はビルドのたびに身元が変わり、macOS のアクセス許可が更新のたびにリセットされる。
set -eu
cd "$(dirname "$0")/.."
mode=${1:-stable}
version=$(scripts/version.sh "$mode")
case "$mode" in
stable)
    name="Amon Hen"
    bundle_id="io.github.fukusaka.amonhen"
    icon="AppIcon"
    ;;
snapshot)
    # 正式版と並べて入れられるように、名前とバンドル ID を変える。
    name="Amon Hen Snapshot"
    bundle_id="io.github.fukusaka.amonhen.snapshot"
    # 右下にオレンジの帯のあるアイコン（正式版と見分ける）。
    icon="AppIcon-Snapshot"
    ;;
esac
# CFBundleVersion は数字とピリオドだけ。snapshot は「次の patch」＋コミット数にする。
build_number=$(echo "$version" | sed -E 's/-snapshot\.([0-9]+).*/.\1/')

scripts/build-core.sh
# エンジン（3 つの OS と CPU）と、リリースに載せる tar.gz。
scripts/build-engine.sh "$version"
(cd apps/macos && swift build -c release --product AmonHen)

app="dist/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp apps/macos/.build/release/AmonHen "$app/Contents/MacOS/AmonHen"
# エンジン（CLI と daemon を兼ねる）。daemon 越しに開くときの共有の daemon にも、端末の CLI にもなる（PATH に
# 通すには scripts/install.sh がリンクを作る）。ファイル名は大文字と小文字を区別しないので、MacOS には置けない
# （AmonHen を上書きする）。
mkdir -p "$app/Contents/Helpers"
cp dist/engine/amonhen-darwin-aarch64 "$app/Contents/Helpers/amonhen"
# SSH 先に送り込むエンジン（SSH 先の OS と CPU に合うもの）。
mkdir -p "$app/Contents/Resources/engines"
cp dist/engine/amonhen-* "$app/Contents/Resources/engines/"
# アイコン（scripts/make-icon.swift で作ったもの）。
cp "apps/macos/Icons/$icon.icns" "$app/Contents/Resources/AppIcon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$name</string>
    <key>CFBundleDisplayName</key><string>$name</string>
    <key>CFBundleIdentifier</key><string>$bundle_id</string>
    <key>CFBundleExecutable</key><string>AmonHen</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key><string>$bundle_id</string>
            <key>CFBundleURLSchemes</key><array><string>amonhen</string></array>
        </dict>
    </array>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Folder</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>None</string>
            <key>LSItemContentTypes</key><array><string>public.folder</string></array>
        </dict>
    </array>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$build_number</string>
    <key>LSMinimumSystemVersion</key><string>14.4</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

# 中の実行ファイル（エンジン）を先に署名してから、アプリ全体に署名する。Developer ID なら公証の条件
# （hardened runtime・時刻の印）を付ける。
identity=${AMONHEN_SIGN_IDENTITY:--}
case $identity in
"Developer ID Application"*) sign() { codesign --force --sign "$identity" --options runtime --timestamp "$@"; } ;;
*) sign() { codesign --force --sign "$identity" --timestamp=none "$@"; } ;;
esac
sign dist/engine/amonhen-darwin-aarch64
cp dist/engine/amonhen-darwin-aarch64 "$app/Contents/Helpers/amonhen"
cp dist/engine/amonhen-darwin-aarch64 "$app/Contents/Resources/engines/amonhen-darwin-aarch64"
sign "$app"
codesign --verify --strict "$app"
# 署名したエンジンで、リリースに載せる macOS の tar.gz を作り直す。
work=$(mktemp -d)
cp dist/engine/amonhen-darwin-aarch64 "$work/amonhen"
tar -C "$work" -czf "dist/amonhen-$version-darwin-aarch64.tar.gz" amonhen
rm -rf "$work"

zip="dist/$(echo "$name" | tr ' ' '-')-$version.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
# 公証の鍵があれば、アプリとエンジンを公証する（なければ何もしない）。出力は標準エラーへ（最後の行は zip の場所）。
scripts/notarize.sh "$app" "$zip" dist/engine/amonhen-darwin-aarch64 >&2
echo "$zip"
