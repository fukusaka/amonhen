#!/bin/sh
# Rust の Linux（musl）向けのビルドで、C の部分（tree-sitter の文法）のアーカイブを zig ar で作る。
# Mac の ar・ranlib は ELF のオブジェクトの目次を作れない。
exec zig ar "$@"
