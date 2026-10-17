#!/bin/sh
# アプリのバージョンを出力する。
#   stable:   Cargo.toml の workspace の version（リリースのときはタグ v<version> と一致させる）
#   snapshot: その次の patch の pre-release（例: v0.1.0 のあと 5 コミット → 0.1.1-snapshot.5+abc1234）。Cargo.toml の
#             版のタグがまだなければ（版を上げてからタグを打つまで）、その版の pre-release（0.2.0-snapshot.5+abc1234）。
#             どちらも、正式版との新旧の順（amonhen install が比べる）が正しくなる。
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
case "${1:-stable}" in
stable)
    echo "$version"
    ;;
snapshot)
    sha=$(git rev-parse --short HEAD)
    if git rev-parse -q --verify "refs/tags/v$version" >/dev/null; then
        major=${version%%.*}
        rest=${version#*.}
        minor=${rest%%.*}
        patch=${rest#*.}
        count=$(git rev-list --count "v$version..HEAD")
        echo "$major.$minor.$((patch + 1))-snapshot.$count+$sha"
    else
        # 前の正式版からのコミット数（正式版がまだなければ全部）。
        previous=$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)
        count=$(git rev-list --count "${previous:+$previous..}HEAD")
        echo "$version-snapshot.$count+$sha"
    fi
    ;;
*)
    echo "usage: $0 [stable|snapshot]" >&2
    exit 2
    ;;
esac
