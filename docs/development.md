# 開発

ビルド・テストと、確認用の起動オプション。設計は [design.md](design.md)、作業の決まりは [CLAUDE.md](../CLAUDE.md) を参照。

## ビルドとテスト

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/build-core.sh && (cd apps/macos && swift test)
scripts/build-engine.sh          # dist/engine にエンジン（macOS・Linux。Linux 用は brew install zig が要る）
scripts/build-app.sh snapshot   # dist/ に .app と zip を作る
cargo build -p amonhen-cli && (cd apps/vscode && npm ci && npm test)   # VSCode 拡張
```

## 確認用の起動オプション

`AmonHen <repo> --snapshot out.png [--select-first | --select-row N] [--working-copy] [--find TEXT] [--search TEXT] [--merge-editor PATH]`、`--blame PATH`・`--file-history PATH`（そのウィンドウを開く）（画面を PNG に書き出して終了）、`--measure-scroll [--scroll-rows N]`（History を 600 フレーム自動でスクロールして速さと処理ごとの内訳を表示）、`--measure-select N`（コミットを N 個選び、詳細が出るまでの段階ごとの時間を表示）、`--log-scroll`（トラックパッドでのスクロールの減速を記録）、`--show-latency`（入力から表示までの時間をタイトルの下に出す）。
