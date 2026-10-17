# Installation

[日本語](ja/install.md)

Everything is on the [Releases](https://github.com/fukusaka/amonhen/releases) page. There are two kinds of release:

- **Stable** releases, tagged `v<version>`. The VSCode extension has its own tags, `vscode-v<version>`.
- **Snapshot**: the latest development build, updated whenever the code passes its tests.

> [!NOTE]
> Stable releases open without a security warning on macOS. Snapshots do not: install them with the scripts below, or
> allow them once in System Settings > Privacy & Security after downloading.

## App (macOS)

1. Download `Amon-Hen-<version>.zip` (the snapshot is `Amon-Hen-Snapshot-<version>.zip`).
2. Unzip it and move `Amon Hen.app` into `/Applications`. The snapshot is called `Amon Hen Snapshot.app`, so you can
   keep both.
3. Open it.

The first time it opens, the app also sets up the command line tool, `amonhen` (in `~/.local/bin`).

## Command line only

For Linux, Windows, or a Mac without the app. The command line tool is the same program the app and the VSCode
extension use (the engine).

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

## VSCode extension

1. Download `amonhen-vscode-<version>.vsix`.
2. Install it:

   ```sh
   code --install-extension amonhen-vscode-<version>.vsix
   ```

3. Reload the VSCode window.

The extension includes the engine, so there is nothing else to install. When you work on another machine through
Remote-SSH, run the command in that window's terminal so the extension is installed on that machine.

## Install or update in one line

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

From a clone of the repository, run `scripts/install-app.sh`, `scripts/install-engine.sh` and
`scripts/install-vscode.sh` directly.

## Checking a download

Every file on a Release carries a build provenance attestation, which proves it was built by this repository's CI
from a given commit. With the [GitHub CLI](https://cli.github.com/):

```sh
gh attestation verify <file> --repo fukusaka/amonhen
```

## Requirements

- **App**: macOS 14.4 or later on Apple silicon.
- **Command line and VSCode extension**: macOS on Apple silicon, Linux (x86_64 or arm64, any distribution), or Windows
  (x86_64 or arm64).
- `git` must be installed.
