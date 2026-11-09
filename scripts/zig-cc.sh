#!/bin/sh
# Rust の Linux（musl）向けのビルドで、リンカーとして zig cc を使う。ターゲットは ZIG_TARGET で渡す。
# C の部分（tree-sitter の文法）のコンパイラとしても使う（CC_<target>）。
# zig が受け付けない引数（aarch64 向けに rustc が付ける Cortex-A53 の回避策、cc-rs が付ける Rust の書き方の
# --target=…。ターゲットは ZIG_TARGET で渡している）は落とす。
for arg; do
    shift
    case "$arg" in
        -Wl,--fix-cortex-a53-843419) ;;
        --target=*) ;;
        *) set -- "$@" "$arg" ;;
    esac
done
exec zig cc -target "$ZIG_TARGET" "$@"
