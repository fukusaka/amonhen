#!/bin/sh
# アプリのバージョンを出力する。
#   stable:   Cargo.toml の workspace の version（リリースのときはタグ v<version> と一致させる）
#   snapshot: その次の patch の pre-release（例: 0.1.0 のあと 5 コミット → 0.1.1-snapshot.5+abc1234）
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
case "${1:-stable}" in
stable)
    echo "$version"
    ;;
snapshot)
    major=${version%%.*}
    rest=${version#*.}
    minor=${rest%%.*}
    patch=${rest#*.}
    count=$(git rev-list --count "v$version..HEAD" 2>/dev/null || git rev-list --count HEAD)
    sha=$(git rev-parse --short HEAD)
    echo "$major.$minor.$((patch + 1))-snapshot.$count+$sha"
    ;;
*)
    echo "usage: $0 [stable|snapshot]" >&2
    exit 2
    ;;
esac
