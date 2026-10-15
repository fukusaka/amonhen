#!/bin/sh
# Rust の Linux（musl）向けのビルドで、リンカーとして zig cc を使う。ターゲットは ZIG_TARGET で渡す。
# zig のリンカーが受け付けない引数（aarch64 向けに rustc が付ける Cortex-A53 の回避策）は落とす。
for arg; do
    shift
    case "$arg" in
        -Wl,--fix-cortex-a53-843419) ;;
        *) set -- "$@" "$arg" ;;
    esac
done
exec zig cc -target "$ZIG_TARGET" "$@"
