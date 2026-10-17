# Amon Hen

[日本語](README.ja.md)

The Git GUI I wanted to use, so I'm building it myself. What matters most: no waiting, even on large repositories.

- History: a list with a graph, commit search, commit editing (message, squash, reorder and more), cherry-pick and revert.
- Working Copy: line-by-line staging and discarding of diffs, and a 3-pane merge editor.
- Rewriting history, discarding changes, dropping stashes and the like can all be reverted with Undo.
- The same engine is also usable from the CLI (`amonhen`) and the VSCode extension, and repositories on SSH hosts work
  just like local ones. A shared daemon keeps History, the search index and SSH connections alive, shared by the app,
  the CLI and the extension.

## Requirements

- App: macOS 14.4 or later, Apple silicon (arm64).
- Engine (CLI and daemon): macOS arm64, Linux x86_64 and aarch64 (statically linked against musl, so it runs on any
  Linux), Windows x86_64 and aarch64.
- All of them use `git` (writes and some reads launch `git`).

## Installation

Get it from [Releases](https://github.com/fukusaka/amonhen/releases). Stable releases are `v<version>` (the VSCode
extension is `vscode-v<version>`), and the latest main is published as `snapshot` (pre-release).

- App: extract `Amon-Hen-<version>.zip` (the snapshot is `Amon-Hen-Snapshot-<version>.zip` and can be installed side by
  side with the stable release) and put it in `/Applications`. It is signed with Developer ID and notarized, so it
  opens as is. On first launch, the engine (CLI and daemon) is placed in `~/.local/share/amonhen` and is available as
  `~/.local/bin/amonhen`.
- Engine only: extract `amonhen-<version>-<OS>-<CPU>` (tar.gz for Linux and macOS, zip for Windows) and run
  `amonhen install` (on Windows it is placed in `%LOCALAPPDATA%\amonhen`, so add `%LOCALAPPDATA%\amonhen\bin` to PATH).
- VSCode extension: install `amonhen-vscode-<version>.vsix` with `code --install-extension <file>`. The extension
  bundles the engine, and once installed it uses the shared daemon.

You can check that a downloaded file was built by this repository's CI with
`gh attestation verify <file> --repo fukusaka/amonhen`.

With the [GitHub CLI](https://cli.github.com/), from a local clone, `scripts/install.sh [stable|snapshot]` (app) and
`apps/vscode/scripts/install.sh [stable|snapshot]` (extension) do everything from downloading to replacing in one go.

## Documents

- [Usage](docs/usage.md): app, SSH hosts, shared daemon, CLI
- [VSCode extension](apps/vscode)
- [Design](docs/design.md) and [front API contract](docs/front.md)
- [Development](docs/development.md) and [Release](docs/release.md)

## License

Use it under your choice of [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE) (`MIT OR Apache-2.0`).
