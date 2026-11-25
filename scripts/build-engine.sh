#!/bin/sh
# エンジン（amonhen。CLI と daemon を兼ねる 1 つの実行ファイル）を作る。
#   scripts/build-engine.sh [<版>]
# 作る OS と CPU は AMONHEN_ENGINE_PLATFORMS（空白区切り）。既定は手元の Mac で作れる 3 つ
# （darwin-aarch64・linux-x86_64・linux-aarch64）。CI は OS と CPU ごとに、そのマシンで 1 つずつ作る
# （windows-x86_64・windows-aarch64 も）。
# 出力: dist/engine/amonhen-<os>-<arch>（Windows は .exe）。リリースに載せる tar.gz・zip は scripts/package-app.sh が作る。
# VSCode 拡張の絵の手伝い（amonhen-render。エンジンに入れない図・数式などの絵の機能）も同じ OS と CPU で作り、
# dist/render/amonhen-render-<os>-<arch> に置く（拡張に同梱するだけで、アプリ・エンジンの tar.gz・zip には入れない）。
# フィーチャーがエンジンに混ざらないよう、cargo は別々に呼ぶ。
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
mkdir -p dist/engine dist/render
for platform in ${AMONHEN_ENGINE_PLATFORMS:-darwin-aarch64 linux-x86_64 linux-aarch64}; do
    arch=${platform#*-}
    for package in amonhen-cli amonhen-render; do
        case "$package" in
        amonhen-cli) bin=amonhen out=dist/engine/amonhen-$platform ;;
        *) bin=amonhen-render out=dist/render/amonhen-render-$platform ;;
        esac
        case "$platform" in
        darwin-*)
            cargo build --release -p "$package"
            cp "target/release/$bin" "$out"
            ;;
        linux-*)
            triple="$arch-unknown-linux-musl"
            upper=$(echo "$triple" | tr 'a-z-' 'A-Z_')
            lower=$(echo "$triple" | tr '-' '_')
            # SSH 先に送るので、デバッグ情報は落として小さくする。
            flags="-C strip=symbols"
            if command -v zig >/dev/null 2>&1; then
                # 起動の部分（crt）と libc は zig のものを使い、Rust の同梱のものとの二重定義を避ける。
                # C の部分（構文の色付けの tree-sitter）も zig cc でコンパイルし、zig ar でまとめる。
                env "CARGO_TARGET_${upper}_LINKER=$PWD/scripts/zig-cc.sh" ZIG_TARGET="$arch-linux-musl" \
                    "CC_$lower=$PWD/scripts/zig-cc.sh" "AR_$lower=$PWD/scripts/zig-ar.sh" \
                    "CARGO_TARGET_${upper}_RUSTFLAGS=-C link-self-contained=no $flags" \
                    cargo build --release -p "$package" --target "$triple"
            else
                env "CC_$lower=musl-gcc" "CARGO_TARGET_${upper}_RUSTFLAGS=$flags" \
                    cargo build --release -p "$package" --target "$triple"
            fi
            cp "target/$triple/release/$bin" "$out"
            ;;
        windows-*)
            triple="$arch-pc-windows-msvc"
            cargo build --release -p "$package" --target "$triple"
            cp "target/$triple/release/$bin.exe" "$out.exe"
            ;;
        *)
            echo "unknown platform: $platform" >&2
            exit 2
            ;;
        esac
    done
done
ls -l dist/engine dist/render
