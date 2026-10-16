# amonhen-standalone

Amon Hen のスタンドアロン版。徹底的な低遅延を最優先にしたネイティブの Git クライアント。コアは Rust で書いて各 OS で使い回し、最初の FE（フロントエンド）は macOS（AppKit ＋ Metal）で作る。自分用で、GitHub Releases で配布する。

設計の詳細は `docs/design.md` を参照すること。VSCode 拡張版は別リポジトリ（vscode-amonhen）。

## 前提

- コア: Rust（edition 2024）の workspace。crate は `crates/` 以下、名前は `amonhen-<dir>`
- アプリ名は `Amon Hen`、バンドル ID は `io.github.fukusaka.amonhen`（snapshot 版は `.snapshot` を付ける）
- コアに OS 固有のコードを入れない。OS の機能は `amonhen-platform` のトレイトで FE から注入する
- git の読み取りは gix、書き込みは `GitBackend` の実装（デスクトップは `git` を spawn）
- FE に渡すデータはゼロコピーを原則とし、形式の変換は `amonhen-projection` に閉じ込める
- 速度に関わる変更は計測してから入れる（Rust は criterion、Swift は os_signpost）

## 構成

- `crates/layout`: FE 間で共通の `#[repr(C)]` レコード
- `crates/model`: 内部モデル、`GitBackend` トレイト
- `crates/core`: 状態・スナップショット・Intent の処理
- `crates/projection`: FE ごとの形への変換、`FrontendProfile`
- `crates/platform`: `Platform` トレイト
- `crates/backend-gix`: gix による `GitBackend`
- `crates/watch`: リポジトリの変化の検知（notify、デスクトップ向け）
- `crates/syntax`: tree-sitter による構文の色付け。文法が C なので、コアの `syntax` フィーチャーで切り離せる
- `crates/remote`: daemon との通信（フレーム・要求の形）、手元の `RemoteBackend` と daemon の `Server`
- `crates/daemon`: `amonhend --stdio <repo>`。SSH 先でリポジトリの git の操作を行う。`amonhend --serve` は手元に常駐する共有の daemon（Unix ドメインソケット。アプリ・CLI の `daemon:/path`・`daemon:ssh://host/path` がつなぐ。SSH 先は daemon が `--stdio` でつないで共有する）
- `crates/cli`: `amonhen`（CLI）。アプリで開く・グラフ付きの log・undo・merge-file。UI なしでコアを使い、History はアプリと同じ永続キャッシュを使う。`.app` では `Contents/Helpers/amonhen`（`MacOS` には置けない。大文字小文字を区別しないので `AmonHen` を上書きする）
- `crates/ffi-c`: C ABI。`build.rs` が `include/amonhen.h` を生成する（コミットする）
- `apps/macos`: SwiftPM のパッケージ。`AmonhenKit`（コアの Swift ラッパー）と `AmonHen`（AppKit のアプリ）

## 確認

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `scripts/build-core.sh && (cd apps/macos && swift test)`
- 見た目: `apps/macos/.build/debug/AmonHen <repo> --snapshot out.png`、速さ: `--measure-scroll`

## 進捗

`docs/design.md` の「実装順序」を参照。完了したステップはそこのチェックボックスを更新すること。
