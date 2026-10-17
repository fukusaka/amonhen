<h1><img src="docs/images/banner.png" alt="Amon Hen"></h1>

[![CI](https://github.com/fukusaka/amonhen/actions/workflows/ci.yml/badge.svg)](https://github.com/fukusaka/amonhen/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fukusaka/amonhen?filter=!vscode-*)](https://github.com/fukusaka/amonhen/releases/latest)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

[日本語](README.ja.md)

The Git GUI I wanted to use, so I'm building it myself. What matters most to me is never having to wait, even on
large repositories.

## What it does

- **History**: browse commits with a branch graph, search them, and edit them (reword, squash, reorder and more).
  Cherry-pick and revert too.
- **Working Copy**: pick exactly which lines go into a commit, throw away changes you don't want, and resolve conflicts
  in a three-pane merge editor.
- **Undo**: most operations can be undone, even rewriting history or deleting a stash.
- **Beyond the app**: the same features are available from the command line (`amonhen`) and from VSCode, and
  repositories on other machines over SSH work just like local ones.

The Git work is done by one program, the **engine**. The macOS app, the command line and the VSCode extension all use
the same engine, so they share what it has already loaded and stay fast together.

> [!WARNING]
> This is a work in progress. Features, file formats and commands may change without notice. It comes with no
> warranty (see [License](#license)), so keep backups of repositories that matter to you.

## Requirements

- **App**: macOS 14.4 or later on Apple silicon.
- **Engine** (command line, and what the VSCode extension uses): macOS on Apple silicon, Linux (x86_64 or arm64, any
  distribution), or Windows (x86_64 or arm64).
- `git` must be installed.

## Installation

Download from the [Releases](https://github.com/fukusaka/amonhen/releases) page. There are two kinds:

- **Stable** releases, tagged `v<version>` (the VSCode extension has its own tags, `vscode-v<version>`).
- **Snapshot**: the latest development build, updated whenever the code passes its tests.

> [!NOTE]
> Stable releases open without a security warning on macOS. Snapshots do not: install them with the scripts below, or
> allow them once in System Settings > Privacy & Security after downloading.

### App (macOS)

1. Download `Amon-Hen-<version>.zip` (the snapshot is `Amon-Hen-Snapshot-<version>.zip`).
2. Unzip it and move `Amon Hen.app` into `/Applications`. The snapshot is called `Amon Hen Snapshot.app`, so you can
   keep both.
3. Open it.

The first time it opens, the app also sets up the command line tool, `amonhen` (in `~/.local/bin`).

### Command line only

For Linux, Windows, or a Mac without the app.

On macOS or Linux, download `amonhen-<version>-<os>-<cpu>.tar.gz`, then:

```sh
tar xzf amonhen-<version>-linux-x86_64.tar.gz
./amonhen install    # copies it to ~/.local/share/amonhen and adds ~/.local/bin/amonhen
```

On Windows, download `amonhen-<version>-windows-<cpu>.zip`, unzip it, then:

```powershell
.\amonhen.exe install    # copies it to %LOCALAPPDATA%\amonhen
```

and add `%LOCALAPPDATA%\amonhen\bin` to your `PATH`.

### VSCode extension

1. Download `amonhen-vscode-<version>.vsix`.
2. Install it:

   ```sh
   code --install-extension amonhen-vscode-<version>.vsix
   ```

3. Reload the VSCode window.

The extension includes the engine, so there is nothing else to install. When you work on another machine through
Remote-SSH, run the command in that window's terminal so the extension is installed on that machine.

### Install or update in one line

These scripts find the latest release, download it and install it (or replace the one you have). They only need
`curl`:

```sh
# App (macOS)
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-app.sh | sh -s -- stable
# Command line only (macOS, Linux)
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.sh | sh -s -- stable
# VSCode extension
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-vscode.sh | sh -s -- stable
```

Replace `stable` with `snapshot` for the latest development build. Run the same line again to update. If the app is
open while you update it, the script tells you, and the new version is used the next time you open the app.

To check that a downloaded file really was built from this repository, see
[Release](docs/release.md#build-provenance-attestation).

## Documents

- [Usage](docs/usage.md): the app, repositories over SSH, the command line
- [VSCode extension](apps/vscode) and how it is built ([design](docs/vscode.md))
- [Design](docs/design.md), and the [protocol between the engine and the apps](docs/front.md)
- [Development](docs/development.md) and [making a release](docs/release.md)
- [Changelog](CHANGELOG.md)

## License

You may use it under either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), whichever you
prefer (`MIT OR Apache-2.0`).
