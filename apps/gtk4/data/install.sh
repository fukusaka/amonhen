#!/bin/sh
# Amon Hen（GTK4）を ~/.local に置く。tar.gz を展開したフォルダーで実行する。
#   ./install.sh
# 置くもの:
#   ~/.local/lib/amonhen-gtk/        実行ファイルと、SSH 先に送るエンジン（engines/）
#   ~/.local/bin/amonhen-gtk         実行ファイルへのリンク
#   ~/.local/share/applications/     アプリの一覧に出す .desktop
#   ~/.local/share/icons/hicolor/    アイコン
# 同梱のエンジンのうち手元の CPU に合うものを `amonhen install` で置く（daemon: で開くときの共有の daemon）。
set -eu
here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}
lib="$prefix/lib/amonhen-gtk"
mkdir -p "$lib" "$prefix/bin" "$prefix/share/applications" "$prefix/share/icons/hicolor/256x256/apps"
rm -rf "$lib/engines"
cp "$here/amonhen-gtk" "$lib/amonhen-gtk"
if [ -d "$here/engines" ]; then
    cp -R "$here/engines" "$lib/engines"
fi
ln -sf "$lib/amonhen-gtk" "$prefix/bin/amonhen-gtk"
sed "s|^Exec=amonhen-gtk|Exec=$lib/amonhen-gtk|" "$here/io.github.fukusaka.amonhen.desktop" \
    > "$prefix/share/applications/io.github.fukusaka.amonhen.desktop"
cp "$here/io.github.fukusaka.amonhen.png" "$prefix/share/icons/hicolor/256x256/apps/"
engine="$lib/engines/amonhen-linux-$(uname -m | sed 's/arm64/aarch64/')"
if [ -x "$engine" ]; then
    # 端末の CLI のリンク（~/.local/bin/amonhen）は、既定の置き場所に置くときだけ張る（PREFIX を替えて試すときに、
    # ふだん使っているリンクを書き換えない）。
    if [ "$prefix" = "$HOME/.local" ]; then
        "$engine" install --quiet
    else
        "$engine" install --quiet --no-link
    fi
fi
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
echo "installed: $prefix/bin/amonhen-gtk"
