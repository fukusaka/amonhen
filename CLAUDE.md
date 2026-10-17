# amonhen

Amon Hen（リポジトリは fukusaka/amonhen。もとの名前は amonhen-standalone）。ネイティブのアプリ（スタンドアロン版）と VSCode 拡張（`apps/vscode`）、両方が使うエンジン（Rust）をここで作る。アプリは徹底的な低遅延を最優先にしたネイティブの Git クライアント。コアは Rust で書いて各 OS で使い回し、最初の FE（フロントエンド）は macOS（AppKit ＋ Metal）で作る。自分用で、GitHub Releases で配布する。

設計の詳細は `docs/design.md` を参照すること。VSCode 拡張は `apps/vscode`（設計は `docs/vscode.md`）。

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
- `crates/front`: FE 向けの窓口（命令と答えは JSON、大きなデータはレコード。取り決めは `docs/front.md`）。共有の daemon が出し、VSCode 拡張と CLI が使う。同じ処理を同じプロセスでも使える（`InProcess`）
- `crates/daemon`: daemon（ライブラリ）。エンジンは CLI と 1 つの実行ファイル `amonhen` で、`--stdio <repo>` は SSH 先でリポジトリの git の操作を行い、`--serve` は手元に常駐する共有の daemon（Unix ドメインソケット。アプリ・CLI の `daemon:/path`・`daemon:ssh://host/path` がつなぐ。SSH 先は daemon が `--stdio` でつないで共有する）。配るエンジンは `scripts/build-engine.sh` で作る（darwin-aarch64、musl の linux-x86_64・linux-aarch64）。置き場所は `~/.local/share/amonhen`（`amonhen install`・`amonhen engine`、`crates/cli/src/install.rs`）
- `crates/cli`: `amonhen`（CLI）。コアでできることはすべて CLI から使える（VSCode 拡張・ほかの FE の試金石）。アプリで開く・グラフ付きの log・status・diff・show・stage・discard・commit・refs・watch・run（コアの命令の JSON）・undo・merge-file。どれも `--json` あり。コアへは `amonhen-front` の窓口だけを通る（手元は同じプロセス、`daemon:`・`ssh://` は共有の daemon のコア）、History はアプリと同じ永続キャッシュを使う。`.app` では `Contents/Helpers/amonhen`（`MacOS` には置けない。大文字小文字を区別しないので `AmonHen` を上書きする）
- `crates/ffi-c`: C ABI。`build.rs` が `include/amonhen.h` を生成する（コミットする）
- `apps/vscode`: VSCode 拡張（TypeScript。もとは vscode-amonhen のリポジトリ。履歴ごと取り込んだ）。エンジンを同梱し、共有の daemon の FE 向けの窓口を使う。決まりは下の「VSCode 拡張」。正式版のタグは `vscode-v*`（アプリ・エンジンは `v*`）
- `apps/macos`: SwiftPM のパッケージ。`AmonhenKit`（コアの Swift ラッパー）と `AmonHen`（AppKit のアプリ）

## 確認

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `scripts/build-core.sh && (cd apps/macos && swift test)`
- 見た目: `apps/macos/.build/debug/AmonHen <repo> --snapshot out.png`、速さ: `--measure-scroll`
- VSCode 拡張: `cargo build -p amonhen-cli && (cd apps/vscode && npm ci && npm run typecheck && npm test)`

## VSCode 拡張（`apps/vscode`）

- TypeScript。`package.json` の `name` は `amonhen`、表示名は `Amon Hen`。コマンド ID・設定キー・View ID の接頭辞は `amonhen.`、独自 URI スキームは `amonhen:`
- `extensionKind: ["workspace"]`。Remote-SSH では拡張ホストが SSH 先で動き、エンジンと git もそこで動く
- 既定では、同梱したエンジンの共有の daemon で History・差分・操作を行う（`src/daemon/`。取り決めは `docs/front.md`）。エンジンを使わないとき（`amonhen.daemon.enabled` を切る、合うエンジンがない）は、拡張の git 実行層（`git` を spawn、常駐 `cat-file`）で動く。状態変化の検知は組み込みの `vscode.git` 拡張 API を使う
- ナビはサイドバーの TreeView、中身（Working Copy・History と差分）はエディタ領域の 1 つの Webview（メインビュー）。コンフリクトの解決は標準の 3-way マージエディタ
- 履歴を書き換える操作は、前に `HEAD` と関係する ref を記録して Undo できるようにする（記録はコアと同じ `<git dir>/amonhen/undo.json`）
- git 実行層では、読み取りに `--no-optional-locks`、status は `--porcelain=v2 -z`、log は `%x00` 区切りで分けて取り、Webview には最小限だけ送る（Remote-SSH では postMessage が SSH を通る）
- 作り方・試し方は `docs/development.md`、リリースは `docs/release.md` の「VSCode extension」

## 進捗

`docs/design.md` の「Implementation order」を参照（VSCode 拡張は `docs/vscode.md` も）。完了したステップはそこのチェックボックスを更新すること。

## 文書

文書は英語が正（`README.md`・`docs/*.md`）で、日本語版を `README.ja.md`・`docs/ja/` に置く（`apps/vscode` は README だけで、`README.ja.md` を並べる）。文書を変えるときは、日本語版も同じように直す（チェックボックスも両方）。

利用者から見える変更を加えたら、ルートの `CHANGELOG.md`（英語だけ）の該当する節（App and engine か VSCode extension）の `Unreleased` に追記する（コミット単位ではなく、リリース単位でまとめる）。
