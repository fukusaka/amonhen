#!/bin/sh
# Amon Hen.app を組み立てて zip にする。出力: dist/<アプリ名>-<バージョン>.zip
#   scripts/build-app.sh [stable|snapshot]
# 署名: AMONHEN_SIGN_IDENTITY にキーチェーンの証明書の名前があればそれで、なければ ad-hoc。
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
scripts/build-linux-daemon.sh
(cd apps/macos && swift build -c release --product AmonHen)

app="dist/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp apps/macos/.build/release/AmonHen "$app/Contents/MacOS/AmonHen"
# daemon 越しに開くときに起動する（`daemon:` の場所）。
cp target/release/amonhend "$app/Contents/MacOS/amonhend"
# SSH 先に送り込む amonhend（Linux 用。SSH 先が macOS なら、上の amonhend を送る）。
mkdir -p "$app/Contents/Resources/daemons"
cp dist/linux/amonhend-linux-* "$app/Contents/Resources/daemons/"
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
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$build_number</string>
    <key>LSMinimumSystemVersion</key><string>14.4</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

# 中の実行ファイル（amonhend）を先に署名してから、アプリ全体に署名する。
codesign --force --sign "${AMONHEN_SIGN_IDENTITY:--}" --timestamp=none "$app/Contents/MacOS/amonhend"
codesign --force --sign "${AMONHEN_SIGN_IDENTITY:--}" --timestamp=none "$app"
codesign --verify --strict "$app"

zip="dist/$(echo "$name" | tr ' ' '-')-$version.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
echo "$zip"
