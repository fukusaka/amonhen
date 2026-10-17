# Development

[日本語](ja/development.md)

Building, testing and releasing the VSCode extension. For using it, see the [README](../README.md).

## Build and test

The engine is built from the repository root (`cargo build -p amonhen-cli`; tests find it there). To bundle the
engines into `bin/`, run `scripts/build-engine.sh` at the root, then `scripts/bundle-engine.sh` here.

```sh
npm install
npm run build      # generate dist/extension.js
npm run typecheck
npm test
```

Pressing F5 in VSCode opens a window with the extension loaded. Setting the log level of "Amon Hen" in Output to Trace shows the time taken by each git command.

| Directory | Description |
| --- | --- |
| `src/daemon/` | The engine: finding and installing it, the connection to the shared daemon, the front API client, and History, diffs and writes through it |
| `src/git/` | The git execution layer. Process spawning, the resident `cat-file`, output parsing, change detection by the modification times in `.git` |
| `src/diff/` | Line-level diffs, and applying only the selected changes |
| `src/state/` | The state management layer. Per-repository caches and updates |
| `src/history/`, `src/workingCopy/`, `src/view/` | Messages between each screen of the main view and the extension host, the History graph and building rebases |
| `src/ui/` | The sidebar nav, the main view panel and per-screen controllers, commands |
| `src/webview/` | The main view's Webview (Working Copy, History, diff rendering) |
| `test/` | Tests run with `node --test`. The git execution layer is checked against real repositories in temporary directories |
| `scripts/` | Bundling the engines into `bin/` and the snapshot version calculation |

## Release

| Channel | When it is built | Version |
| --- | --- | --- |
| stable | On pushing a `vscode-v*` tag (separate from the app and engine's `v*`) | `0.0.2` |
| snapshot | Whenever CI passes on main (put on the same `snapshot` pre-release as the app and engine) | `0.0.3-snapshot.<number of apps/vscode commits since the last release>+<sha>` |

A snapshot is treated as a pre-release of the next patch, so installing a stable release is judged newer than the snapshot.

Steps to cut a stable release:

1. In the "VSCode extension" section of the root [CHANGELOG.md](../../../CHANGELOG.md), change `Unreleased` to a heading with the version and date being released, and commit it
2. Include `npm version patch --no-git-tag-version` in the same commit as 1.
3. `git tag vscode-v<version> && git push origin main vscode-v<version>`
