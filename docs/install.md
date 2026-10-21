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

## GTK4 app (Linux, early)

A native app for Linux, still early: History, Working Copy, the merge editor, Blame and File History, also for
repositories through the shared daemon or over SSH. Download `amonhen-gtk-<version>-linux-<cpu>.tar.gz`, then:

```sh
tar xzf amonhen-gtk-<version>-linux-x86_64.tar.gz
amonhen-gtk-linux-x86_64/install.sh
```

This puts it in `~/.local`, adds it to the desktop's app list and sets up the command line tool too. GTK is not
included; if it is missing, install it (`sudo apt install libgtk-4-1` on Ubuntu).

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

and add `%LOCALAPPDATA%\amonhen\bin` to your `PATH`. Or use the PowerShell script below, which does all of this
except the `PATH`.

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

On Windows, in PowerShell:

```powershell
# Command line only (Windows)
irm https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1 | iex
# the snapshot, or a given release (such as v0.2.4)
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1))) snapshot
```

It picks the zip for the machine's CPU, checks its attestation when the [GitHub CLI](https://cli.github.com/) is
installed and logged in (`-Verify require` to insist, `-Verify skip` to skip it), and runs `amonhen.exe install`.

Replace `stable` with `snapshot` for the latest development build. Run the same line again to update. If the app is
open while you update it, the script tells you, and the new version is used the next time you open the app.

The app goes into `/Applications`. Add `--user` (`sh -s -- --user stable`) to put it in your own Applications folder
(`~/Applications`) instead, `--system` for `/Applications`, or `--dir <folder>` for any folder (created if missing).
Without any of them, an update replaces the app where it is
already installed.

From a clone of the repository, run `scripts/install-app.sh`, `scripts/install-engine.sh`,
`scripts/install-vscode.sh` and `scripts\install-engine.ps1` directly.

## Checking a download

Every file on a Release carries a build provenance attestation, which proves it was built by this repository's CI
from a given commit. With the [GitHub CLI](https://cli.github.com/):

```sh
gh attestation verify <file> --repo fukusaka/amonhen
```

## Requirements

- **App**: macOS 14.4 or later on Apple silicon.
- **GTK4 app**: Linux (x86_64 or arm64) with GTK 4.14 or later (Ubuntu 24.04, Debian 13 or later).
- **Command line and VSCode extension**: macOS on Apple silicon, Linux (x86_64 or arm64, any distribution), or Windows
  (x86_64 or arm64).
- `git` must be installed.
