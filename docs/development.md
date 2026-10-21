# Development

[日本語](ja/development.md)

Building, testing, and launch options for checking. For the design see [design.md](design.md), and for working rules see [CLAUDE.md](../CLAUDE.md). Setting up and driving a real Windows machine is in [windows.md](windows.md).

## Build and test

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/build-core.sh && (cd apps/macos && swift test)
scripts/build-engine.sh          # engines into dist/engine (macOS, Linux; Linux needs brew install zig)
scripts/build-app.sh snapshot   # build the .app and zip into dist/ (compile-app.sh, Linux engines, package-app.sh)
scripts/check-engine.sh dist/engine/amonhen-darwin-aarch64   # run a built engine once (version, reads this repository)
scripts/third-party-notices.sh   # dist/THIRD-PARTY-NOTICES.md (cargo install --locked cargo-about --features cli)
cargo deny check licenses        # dependencies use only the licenses allowed in deny.toml (cargo install --locked cargo-deny)
cargo build -p amonhen-cli && (cd apps/vscode && npm ci && npm test)   # VSCode extension
(cd apps/vscode && npm run test:e2e)   # VSCode extension in a real VSCode (after the line above)
(cd apps/gtk4 && cargo clippy --all-targets -- -D warnings && cargo run --release -- <repo>)   # GTK4 app (needs GTK 4.14 or later: libgtk-4-dev, or brew install gtk4 pkgconf)
scripts/package-gtk4.sh          # GTK4 app into dist/amonhen-gtk-<version>-linux-<arch>.tar.gz (on Linux; includes dist/engine's Linux engines if built)
```

## Launch options for checking

`AmonHen <repo> --snapshot out.png [--select-first | --select-row N] [--working-copy] [--find TEXT] [--search TEXT] [--merge-editor PATH]`, `--blame PATH`, `--file-history PATH` (open that window) (write the screen to a PNG and exit), `--measure-scroll [--scroll-rows N]` (auto-scroll History for 600 frames and show the speed and a per-stage breakdown), `--measure-select N` (select N commits and show the time per stage until the details appear), `--log-scroll` (record trackpad scroll deceleration), `--show-latency` (show the time from input to display below the title). `--select-first` and `--select-row N` also work
without `--snapshot`.

The README's screenshot is the real window, not a `--snapshot` (which leaves the toolbar blank): launch the debug build
on a repository with `--select-row N`, then capture the window with `screencapture -o -l<window id>`. To hide avatars for
the picture, set `defaults write AmonHen showAvatars -bool NO` beforehand (the debug build's own defaults domain) and
delete it afterwards. The window size comes from `NSWindow Frame Repository <path>` in the same domain (the README uses
1200×720: `"1300 500 1200 720 0 0 <screen width> <screen height> "`).

The GTK4 app (`apps/gtk4`): `amonhen-gtk <repo> --snapshot out.png [--scroll-to N | --select-rows N,N,... | --select-ref NAME] [--menu-ref NAME | --menu-row N] [--search QUERY [--only-matching]] [--blame PATH | --file-history PATH | --activity operations|history|performance|events|daemon | --settings git|appearance|performance|advanced | --merge-editor PATH [--accept ours|theirs|both|base]] [--open-menu repository|primary] [--perform JSON] [--working-copy [--select-path PATH] [--toggle-row N] [--layout list|tree|flat]]` (`--menu-ref` opens that ref's right-click menu, `--perform` runs one core command the way the menus do, `--toggle-row` acts as one click on that diff line's mark) (write the window to a PNG once the visible rows are read, and exit), `--measure-scroll [--scroll-rows N]` (scroll History 40 (or N) rows per frame for 600 frames and show the main thread's time per frame, the History snapshot's time, dropped frames and the time from a frame's start to its presentation; `GSK_RENDERER=cairo|gl|vulkan` picks the renderer).

## VSCode extension

The design is in [vscode.md](vscode.md).

The engine is built from the repository root (`cargo build -p amonhen-cli`; tests find it there). To bundle the
engines into `bin/`, run `scripts/build-engine.sh` at the root, then `scripts/bundle-engine.sh` in `apps/vscode`.

```sh
cd apps/vscode
npm install
npm run build      # generate dist/extension.js
npm run typecheck
npm test
npm run test:e2e   # in a real VSCode (downloads VSCode 1.120.0 into .vscode-test/)
```

`npm run test:e2e` (`e2e/run.mjs`) builds the extension and lays it out like the `.vsix` in a temporary directory, with
the engine built at the root in `bin/`, clones a repository there with `core.autocrlf=true` (so its files are CRLF), and
opens it in VSCode with the extension (`@vscode/test-cli`, settings in `.vscode-test.mjs`). The engine's data directory,
the shared daemon's socket and cache, and `HOME` point into the temporary directory, so a daemon or app already running
is left alone. The tests (`e2e/*.test.ts`, mocha) run in the extension host. They drive the main view by handing the
messages its Webview would send to the extension host (`activate` returns the panel) and check the messages sent back
and the repository with git. They cover activation with the bundled engine, History through the shared daemon, line
staging, discarding lines, Undo and commit on a CRLF file both through the daemon and with `amonhen.daemon.enabled`
off, one daemon shared with another client and the CLI, that every process the extension starts has `windowsHide`, and that its git processes end when the repositories
are closed. A run takes about 10 seconds once VSCode is downloaded. On Linux
without a display, use `xvfb-run -a npm run test:e2e` (as CI does).

Pressing F5 in VSCode opens a window with the extension loaded. Setting the log level of "Amon Hen" in Output to Trace shows the time taken by each git command.

Directories under `apps/vscode`:

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
| `e2e/` | Tests run in a real VSCode with the extension loaded (`npm run test:e2e`) |
| `scripts/` | Bundling the engines into `bin/`, the snapshot version calculation, and running `test:e2e` on a Windows desktop over ssh |
