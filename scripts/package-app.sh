#!/bin/sh
# Amon Hen.app を組み立てて署名し、リリースに載せるファイルを作る。scripts/compile-app.sh のあとに、dist/engine に
# 配るエンジン（SSH 先に送る Linux 用なども）をそろえてから実行する。
#   scripts/package-app.sh [stable|snapshot]
# dist/THIRD-PARTY-NOTICES.md（scripts/third-party-notices.sh）も先に作っておく。
# 出力: dist/<アプリ名>-<版>.zip（最後の行にその場所）、dist/amonhen-<版>-<os>-<arch>.tar.gz（Windows は .zip）
# 署名: AMONHEN_SIGN_IDENTITY にキーチェーンの証明書の名前があればそれで、なければ ad-hoc（scripts/codesign.sh）。
# 公証の鍵（scripts/notarize.sh）があれば公証して結果を綴じ込む。
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
*)
    echo "usage: $0 [stable|snapshot]" >&2
    exit 2
    ;;
esac
# CFBundleVersion は数字とピリオドだけ。snapshot は「次の patch」＋コミット数にする。
build_number=$(echo "$version" | sed -E 's/-snapshot\.([0-9]+).*/.\1/')
# CI では成果物を渡すときに実行の印が落ちる。
chmod 755 dist/bin/AmonHen dist/engine/amonhen-*

# 中の実行ファイル（エンジン）を先に署名してから、アプリ全体に署名する。
scripts/codesign.sh dist/engine/amonhen-darwin-aarch64

app="dist/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp dist/bin/AmonHen "$app/Contents/MacOS/AmonHen"
# エンジン（CLI と daemon を兼ねる）。daemon 越しに開くときの共有の daemon にも、端末の CLI にもなる（PATH に
# 通すには scripts/install-app.sh がリンクを作る）。ファイル名は大文字と小文字を区別しないので、MacOS には置けない
# （AmonHen を上書きする）。
mkdir -p "$app/Contents/Helpers"
cp dist/engine/amonhen-darwin-aarch64 "$app/Contents/Helpers/amonhen"
# git と ssh が資格情報を尋ねるときのダイアログ（apps/macos/Sources/AmonHen/main.swift の Askpass）。
cp apps/macos/Helpers/amonhen-askpass "$app/Contents/Helpers/amonhen-askpass"
# SSH 先に送り込むエンジン（SSH 先の OS と CPU に合うもの。Windows のものは SSH 先には送らない）。
mkdir -p "$app/Contents/Resources/engines"
for f in dist/engine/amonhen-darwin-* dist/engine/amonhen-linux-*; do
    [ -e "$f" ] && cp "$f" "$app/Contents/Resources/engines/"
done
# ライセンスと、中に入っている依存のライセンスの一覧（scripts/third-party-notices.sh）。
legal="LICENSE-MIT LICENSE-APACHE NOTICE dist/THIRD-PARTY-NOTICES.md"
cp $legal "$app/Contents/Resources/"
# アイコン（scripts/make-icon.swift で作ったもの）。
cp "apps/macos/Icons/$icon.icns" "$app/Contents/Resources/AppIcon.icns"
# 画面の文字の訳（apps/macos/Sources/AmonHen/Localization.swift）。
cp -R apps/macos/Localization/*.lproj "$app/Contents/Resources/"
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
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleLocalizations</key><array><string>en</string><string>ja</string></array>
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
scripts/codesign.sh "$app"
codesign --verify --strict "$app"

# エンジンだけを使う人向けに、OS と CPU ごとに 1 つずつ（中は amonhen か amonhen.exe と、ライセンスの文書）。
for f in dist/engine/amonhen-*; do
    platform=$(basename "$f" .exe)
    platform=${platform#amonhen-}
    work=$(mktemp -d)
    cp $legal "$work/"
    case "$f" in
    *.exe)
        cp "$f" "$work/amonhen.exe"
        rm -f "dist/amonhen-$version-$platform.zip"
        (cd "$work" && zip -q "$OLDPWD/dist/amonhen-$version-$platform.zip" ./*)
        ;;
    *)
        cp "$f" "$work/amonhen"
        tar -C "$work" -czf "dist/amonhen-$version-$platform.tar.gz" .
        ;;
    esac
    rm -rf "$work"
done

zip="dist/$(echo "$name" | tr ' ' '-')-$version.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
# 公証の鍵があれば、アプリとエンジンを公証する（なければ何もしない）。出力は標準エラーへ（最後の行は zip の場所）。
scripts/notarize.sh "$app" "$zip" dist/engine/amonhen-darwin-aarch64 >&2
echo "$zip"
