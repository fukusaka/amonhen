# Development

[日本語](ja/development.md)

Building, testing, and launch options for checking. For the design see [design.md](design.md), and for working rules see [CLAUDE.md](../CLAUDE.md).

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
(cd apps/gtk4 && cargo clippy --all-targets -- -D warnings && cargo run --release -- <repo>)   # GTK4 app (needs GTK 4.14 or later: libgtk-4-dev, or brew install gtk4 pkgconf)
```

## Launch options for checking

`AmonHen <repo> --snapshot out.png [--select-first | --select-row N] [--working-copy] [--find TEXT] [--search TEXT] [--merge-editor PATH]`, `--blame PATH`, `--file-history PATH` (open that window) (write the screen to a PNG and exit), `--measure-scroll [--scroll-rows N]` (auto-scroll History for 600 frames and show the speed and a per-stage breakdown), `--measure-select N` (select N commits and show the time per stage until the details appear), `--log-scroll` (record trackpad scroll deceleration), `--show-latency` (show the time from input to display below the title).

The GTK4 app (`apps/gtk4`): `amonhen-gtk <repo> --snapshot out.png [--scroll-to N | --select-ref NAME] [--working-copy [--select-path PATH] [--toggle-row N]]` (`--toggle-row` acts as one click on that diff line's mark) (write the window to a PNG once the visible rows are read, and exit), `--measure-scroll [--scroll-rows N]` (scroll History 40 (or N) rows per frame for 600 frames and show the main thread's time per frame, the History snapshot's time and dropped frames; `GSK_RENDERER=cairo|gl|vulkan` picks the renderer).

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
```

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
| `scripts/` | Bundling the engines into `bin/` and the snapshot version calculation |
