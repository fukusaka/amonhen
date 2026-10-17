# Development

[日本語](ja/development.md)

Building, testing, and launch options for checking. For the design see [design.md](design.md), and for working rules see [CLAUDE.md](../CLAUDE.md).

## Build and test

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/build-core.sh && (cd apps/macos && swift test)
scripts/build-engine.sh          # engine into dist/engine (macOS, Linux; Linux needs brew install zig)
scripts/build-app.sh snapshot   # build the .app and zip into dist/
cargo build -p amonhen-cli && (cd apps/vscode && npm ci && npm test)   # VSCode extension
```

## Launch options for checking

`AmonHen <repo> --snapshot out.png [--select-first | --select-row N] [--working-copy] [--find TEXT] [--search TEXT] [--merge-editor PATH]`, `--blame PATH`, `--file-history PATH` (open that window) (write the screen to a PNG and exit), `--measure-scroll [--scroll-rows N]` (auto-scroll History for 600 frames and show the speed and a per-stage breakdown), `--measure-select N` (select N commits and show the time per stage until the details appear), `--log-scroll` (record trackpad scroll deceleration), `--show-latency` (show the time from input to display below the title).
