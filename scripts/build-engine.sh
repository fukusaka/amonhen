#!/bin/sh
# エンジン（amonhen。CLI と daemon を兼ねる 1 つの実行ファイル）を作る。
#   scripts/build-engine.sh [<版>]
# 作る OS と CPU は AMONHEN_ENGINE_PLATFORMS（空白区切り）。既定は手元の Mac で作れる 3 つ
# （darwin-aarch64・linux-x86_64・linux-aarch64）。CI は OS と CPU ごとに、そのマシンで 1 つずつ作る
# （windows-x86_64・windows-aarch64 も）。
# 出力: dist/engine/amonhen-<os>-<arch>（Windows は .exe）。リリースに載せる tar.gz・zip は scripts/package-app.sh が作る。
# エンジンには配る版を埋め込む（`amonhen --version`。`amonhen install` が新旧を決める）。<版> がなければ snapshot の版。
# Linux 用は musl で静的リンクする（どの Linux でも動く）。zig があればリンクに zig を使い（Mac から作るとき。
# brew install zig）、なければ musl-gcc を使う（Linux で作るとき。apt install musl-tools）。
set -eu
cd "$(dirname "$0")/.."
version=${1:-}
AMONHEN_VERSION=${version:-$(scripts/version.sh snapshot)}
export AMONHEN_VERSION
# アプリと同じ値にする（scripts/build-core.sh）。違うと、アプリのビルドのあとに C の部分から作り直しになる。
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.4}"
echo "engine version: $AMONHEN_VERSION"
mkdir -p dist/engine
for platform in ${AMONHEN_ENGINE_PLATFORMS:-darwin-aarch64 linux-x86_64 linux-aarch64}; do
    arch=${platform#*-}
    case "$platform" in
    darwin-*)
        cargo build --release -p amonhen-cli
        cp target/release/amonhen "dist/engine/amonhen-$platform"
        ;;
    linux-*)
        triple="$arch-unknown-linux-musl"
        upper=$(echo "$triple" | tr 'a-z-' 'A-Z_')
        lower=$(echo "$triple" | tr '-' '_')
        # SSH 先に送るので、デバッグ情報は落として小さくする。
        flags="-C strip=symbols"
        if command -v zig >/dev/null 2>&1; then
            # 起動の部分（crt）と libc は zig のものを使い、Rust の同梱のものとの二重定義を避ける。
            env "CARGO_TARGET_${upper}_LINKER=$PWD/scripts/zig-cc.sh" ZIG_TARGET="$arch-linux-musl" \
                "CARGO_TARGET_${upper}_RUSTFLAGS=-C link-self-contained=no $flags" \
                cargo build --release -p amonhen-cli --target "$triple"
        else
            env "CC_$lower=musl-gcc" "CARGO_TARGET_${upper}_RUSTFLAGS=$flags" \
                cargo build --release -p amonhen-cli --target "$triple"
        fi
        cp "target/$triple/release/amonhen" "dist/engine/amonhen-$platform"
        ;;
    windows-*)
        triple="$arch-pc-windows-msvc"
        cargo build --release -p amonhen-cli --target "$triple"
        cp "target/$triple/release/amonhen.exe" "dist/engine/amonhen-$platform.exe"
        ;;
    *)
        echo "unknown platform: $platform" >&2
        exit 2
        ;;
    esac
done
ls -l dist/engine
